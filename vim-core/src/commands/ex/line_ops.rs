//! Line operations (`:d`, `:y`).
//!
//! Delete and yank lines by range.

use super::range::resolve_range;
use super::types::{ExContext, ExResult};
use crate::effects::Effects;
use crate::grammar::types::ExRange;
use crate::primitives::RegisterName;
use crate::primitives::{MarkName, MotionType, Offset, Range};

/// Delete lines in range (`:d`).
///
/// Deletes the specified line range and optionally yanks to register.
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn delete(range: &ExRange, register: Option<RegisterName>, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_offset, end_offset) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    // Get deleted text (for register, use original range).
    // In Neovim, linewise operations always include a trailing newline,
    // even on the last line or an empty buffer. If the slice is empty
    // (empty buffer `:d`), use "\n" to match Neovim convention.
    let raw_text = Range::new(start_offset, end_offset).slice(ctx.text);
    let deleted_text: &str = if raw_text.is_empty() { "\n" } else { raw_text };

    // If we're deleting the last line(s) and there's a preceding newline,
    // extend the range backward to include the newline separator.
    // This prevents leaving a trailing newline in the document.
    let (actual_start, actual_end) =
        if resolved.end() + 1 >= ctx.total_lines && resolved.start() > 0 {
            // Deleting to the end — include preceding newline
            (start_offset.prev(), end_offset)
        } else {
            (start_offset, end_offset)
        };

    // Build delete effects (cursor + message are shared for all register paths)
    let deleting_at_end = actual_start != start_offset;
    let mut effects = Effects::new().delete(Range::new(actual_start, actual_end));

    // When deleting at the end of the document, the delete range was extended
    // backward to include the preceding newline. The auto-marks from
    // sync_change_marks would point to `actual_start` (the newline), but
    // Neovim's marks point to `start_offset` (the logical start of the first
    // deleted line in the original text, which in post-delete text corresponds
    // to new_text_len + 1 due to the implicit trailing newline).
    if deleting_at_end {
        effects = effects
            .set_mark(MarkName::CHANGE_START, start_offset, None)
            .set_mark(MarkName::CHANGE_END, start_offset, None)
            .set_mark(MarkName::LAST_CHANGE, start_offset, None);
    }

    // Route to registers using the standard delete register logic so that
    // numbered registers 1-9 shift correctly (matching Neovim's :d behavior).
    let reg = register.unwrap_or(RegisterName::UNNAMED);
    if !reg.is_blackhole() {
        effects = crate::commands::operators::registers::route_delete_registers(
            effects,
            deleted_text,
            MotionType::LineWise,
            reg,
            true, // linewise delete is always multiline
            false,
        );
    }

    // Cursor position after delete: if we deleted at end of document, go to
    // start of the new last line; otherwise stay at original start.
    let delete_len = actual_start.distance(actual_end);
    let new_text_len = ctx.text.len() - delete_len;
    let cursor_pos = if actual_start.get() >= new_text_len && new_text_len > 0 {
        let remaining = &ctx.text[..actual_start.get()];
        let last_nl = remaining.rfind('\n').map_or(0, |p| p + 1);
        Offset::new(last_nl)
    } else {
        start_offset
    };
    // Neovim resets curswant after :d because the cursor lands at column 0
    // (or the first non-blank). Emit SetStickyColumn to match.
    let cursor_col = crate::commands::helpers::column_of(ctx.text, cursor_pos.get());
    effects = effects.set_cursor(cursor_pos);
    effects.push(crate::effects::Effect::SetStickyColumn {
        column: Some(crate::primitives::VirtualColumn::new(cursor_col)),
    });

    // Show message
    if resolved.line_count() > 1 {
        let line_count = resolved.line_count();
        effects = effects.show_message(format!("{line_count} fewer lines"));
    }

    Ok(effects)
}

/// Yank lines in range (`:y`).
///
/// Copies the specified line range to register without deleting.
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn yank(range: &ExRange, register: Option<RegisterName>, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_offset, end_offset) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    // Get yanked text.
    // In Neovim, linewise operations always include a trailing newline,
    // even on the last line or an empty buffer. Use "\n" for empty slices.
    let raw_text = Range::new(start_offset, end_offset).slice(ctx.text);
    let yanked_text: &str = if raw_text.is_empty() { "\n" } else { raw_text };

    let reg = register.unwrap_or(RegisterName::UNNAMED);

    // Blackhole register: yank to blackhole is a no-op
    if reg.is_blackhole() {
        return Ok(Effects::new());
    }

    let mut effects = Effects::new().set_register(reg, yanked_text, MotionType::LineWise);

    // Vim always populates unnamed register on yank; register "0" (last yank)
    // is only set when no explicit register was specified (same as normal-mode yy).
    if reg == RegisterName::UNNAMED {
        effects = effects.set_register(RegisterName::LAST_YANK, yanked_text, MotionType::LineWise);
    } else {
        effects = effects.set_register(RegisterName::UNNAMED, yanked_text, MotionType::LineWise);
    }

    // Neovim sets [ and ] marks for :y (same as normal yank).
    // [ = start of yanked range, ] = end (inclusive).
    // For linewise yank, ] points to the last char of the last yanked line
    // (or end-of-buffer for empty/single-line).
    effects = effects.set_mark(MarkName::CHANGE_START, start_offset, None);
    let mark_end = if end_offset.get() > 0 {
        Offset::new(end_offset.get().saturating_sub(1))
    } else {
        end_offset
    };
    effects = effects.set_mark(MarkName::CHANGE_END, mark_end, None);

    // Show message
    if resolved.line_count() > 1 {
        let line_count = resolved.line_count();
        effects = effects.show_message(format!("{line_count} lines yanked"));
    }

    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    fn ctx() -> ExContext<'static> {
        ExContext::new("line1\nline2\nline3\nline4", 1)
    }

    #[test]
    fn test_delete_single_line() {
        let range = ExRange::single_line(2);
        let effects = delete(&range, None, &ctx()).unwrap();

        // Should have Delete, SetRegister, SetCursor
        assert!(effects.len() >= 2);
        assert!(matches!(effects.as_slice()[0], Effect::Delete { .. }));
    }

    #[test]
    fn test_delete_range() {
        let range = ExRange::lines(2, 3);
        let effects = delete(
            &range,
            Some(crate::primitives::RegisterName::new_unchecked('a')),
            &ctx(),
        )
        .unwrap();

        // Check register is 'a'
        assert!(effects
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == crate::primitives::RegisterName::new_unchecked('a'))));
    }

    #[test]
    fn test_yank_lines() {
        let range = ExRange::lines(1, 2);
        let effects = yank(&range, None, &ctx()).unwrap();

        assert!(
            matches!(effects.as_slice()[0], Effect::SetRegister { name, .. } if name == crate::primitives::RegisterName::new_unchecked('"'))
        );
    }

    // ── compute_join_text tests ──────────────────────────────────────────

    #[test]
    fn join_text_single_line_no_separator() {
        // A single line should be returned as-is (no space prepended).
        let result = compute_join_text("hello", false);
        assert_eq!(result, "hello\n");
    }

    #[test]
    fn join_text_single_line_is_last_range() {
        // Single line, last range — no trailing newline.
        let result = compute_join_text("hello", true);
        assert_eq!(result, "hello");
    }

    #[test]
    fn join_text_two_lines() {
        let result = compute_join_text("hello\nworld", false);
        assert_eq!(result, "hello world\n");
    }

    #[test]
    fn join_text_empty_lines_between_content() {
        // Empty lines should still produce space separators.
        let result = compute_join_text("aaa\n\nbbb", false);
        assert_eq!(result, "aaa  bbb\n");
    }

    #[test]
    fn join_text_is_last_range_true_no_trailing_newline() {
        let result = compute_join_text("foo\nbar\nbaz", true);
        assert_eq!(result, "foo bar baz");
    }

    #[test]
    fn join_text_is_last_range_false_trailing_newline() {
        let result = compute_join_text("foo\nbar\nbaz", false);
        assert_eq!(result, "foo bar baz\n");
    }

    #[test]
    fn join_text_trailing_whitespace_preserved() {
        // We use trim_end_matches('\n'), NOT trim_end(), so trailing
        // spaces/tabs within a line are preserved.
        let result = compute_join_text("hello   \nworld", true);
        assert_eq!(result, "hello    world");
    }

    #[test]
    fn join_text_lines_ending_with_newline() {
        // Input lines that end with \n — trim_end_matches('\n') removes
        // the newline but preserves other trailing whitespace.
        let result = compute_join_text("alpha\n\nbeta\n", true);
        assert_eq!(result, "alpha  beta");
    }
}

/// Compute the joined text for `:j` (join lines).
///
/// Pure: `(text, is_last_range) → joined_string`.
/// Trims trailing whitespace per line, joins with single space.
/// Appends newline unless joining includes the last line of the document.
#[must_use]
pub fn compute_join_text(text: &str, is_last_range: bool) -> String {
    compute_join_text_inner(text, is_last_range, false)
}

/// Inner join implementation.
///
/// When `no_space` is true (`:j!` bang), continuation lines are concatenated
/// directly without inserting a space separator.
#[must_use]
fn compute_join_text_inner(text: &str, is_last_range: bool, no_space: bool) -> String {
    let mut joined = String::new();
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            if !no_space {
                joined.push(' ');
            }
            // Strip leading whitespace from continuation lines per Vim :join spec.
            joined.push_str(line.trim_start().trim_end_matches('\n'));
        } else {
            joined.push_str(line.trim_end_matches('\n'));
        }
    }
    if !is_last_range {
        joined.push('\n');
    }
    joined
}

/// Go to target line (`:3`, `:$`, etc.).
///
/// Resolves the range, moves cursor to the first non-blank character
/// of the target (end) line.
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if the target line is invalid.
pub fn goto_line(range: &ExRange, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let target_line = resolved.end();
    let offset = ctx
        .line_start_offset(target_line)
        .ok_or(crate::errors::VimError::InvalidRange)?;
    let line_start = offset.get();
    let line_text_from_start = &ctx.text[line_start..];
    let line_end = line_text_from_start
        .find('\n')
        .unwrap_or(line_text_from_start.len());
    let first_non_ws = line_start
        + crate::commands::helpers::first_non_blank_in_line(&line_text_from_start[..line_end]);
    Ok(super::effects::goto_line(first_non_ws))
}

/// Join lines in range (`:j`).
///
/// Resolves the range, expands single-line ranges to include the next line,
/// computes the joined text, and produces the replacement effects.
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if the range is invalid.
pub fn join(range: &ExRange, bang: bool, ctx: &ExContext) -> ExResult {
    let mut resolved = resolve_range(range, ctx)?;

    // `:j` on a single line expands to join with the next line
    if resolved.is_single_line() && resolved.end() + 1 < ctx.total_lines {
        resolved = resolved.extend_end(1);
    }
    if resolved.is_single_line() {
        return Ok(Effects::new().show_message(compact_str::CompactString::from("0 lines joined")));
    }

    let (start, end) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;
    let text = &ctx.text[start.get()..end.get()];
    let is_last_range = resolved.end() + 1 >= ctx.total_lines;
    let joined = compute_join_text_inner(text, is_last_range, bang);

    // Neovim marks after :join:
    //   '[' = first join point (position of first \n in original range)
    //   ']' = end of replacement (exclusive if no trailing \n, at \n if trailing \n)
    //   '.' = exclusive end of replacement, +1 for implicit trailing \n when is_last_range
    let first_nl = text.find('\n').unwrap_or(text.len());
    let mark_bracket_start = Offset::new(start.get() + first_nl);
    let mark_bracket_end = if joined.ends_with('\n') {
        Offset::new(start.get() + joined.len() - 1)
    } else {
        Offset::new(start.get() + joined.len())
    };
    let mark_dot = Offset::new(start.get() + joined.len() + usize::from(is_last_range));

    Ok(super::effects::join_result(
        Range::new(start, end),
        compact_str::CompactString::from(joined),
        start,
        resolved.line_count(),
    )
    .set_mark(MarkName::CHANGE_START, mark_bracket_start, None)
    .set_mark(MarkName::CHANGE_END, mark_bracket_end, None)
    .set_mark(MarkName::LAST_CHANGE, mark_dot, None))
}
