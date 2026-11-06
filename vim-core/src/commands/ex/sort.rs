//! Sort command (`:sort [options]`).
//!
//! Sort lines in a range with various options.

use super::range::resolve_range;
use super::types::{ExContext, ExResult};
use crate::effects::Effects;
use crate::grammar::types::{ExRange, SortOptions};
use crate::primitives::{MarkName, Offset, Range};

/// Execute sort command.
///
/// Sorts lines in the specified range according to options.
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn sort(range: &ExRange, options: &SortOptions, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_offset, end_offset) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    // Collect lines
    let mut lines: Vec<&str> = Vec::new();
    for line_idx in resolved.start()..=resolved.end() {
        if let Some(line_text) = ctx.line_text(line_idx) {
            lines.push(line_text);
        }
    }

    // Compile pattern regex if present (for sort key extraction).
    let pattern_re = options
        .pattern()
        .and_then(|p| crate::regex::VimRegex::new(p).ok());

    // Sort based on options (stable sort — Vim preserves relative order of equal elements).
    // When a pattern is given, the sort key is the text AFTER the pattern match.
    #[allow(
        clippy::stable_sort_primitive,
        reason = "Vim :sort preserves relative order of equal elements"
    )]
    if let Some(ref re) = pattern_re {
        // Sort by text after pattern match. If no match, use entire line.
        let mut cache = re.create_cache();
        lines.sort_by_cached_key(|s| {
            let ctx = crate::regex::MatchContext::simple(s);
            if let Ok(Some(m)) = re.find_with_cache(&mut cache, &ctx) {
                s[m.range.end..].to_string()
            } else {
                s.to_string()
            }
        });
    } else if options.numeric() {
        lines.sort_by_key(|s| extract_leading_number(s));
    } else if options.ignore_case() {
        lines.sort_by_cached_key(|s| s.to_lowercase());
    } else {
        lines.sort();
    }

    // Reverse if needed
    if options.reverse() {
        lines.reverse();
    }

    // Remove duplicates if unique
    if options.unique() {
        lines.dedup_by(|a, b| {
            if options.ignore_case() {
                a.to_lowercase() == b.to_lowercase()
            } else {
                a == b
            }
        });
    }

    // Build replacement text
    let mut new_text = String::new();
    for (i, line) in lines.iter().enumerate() {
        new_text.push_str(line);
        // Add newline unless it's the last line AND it was the last line of file
        if i < lines.len() - 1 || resolved.end() + 1 < ctx.total_lines {
            new_text.push('\n');
        }
    }

    let line_count = resolved.line_count();
    let effects = Effects::new()
        .begin_undo()
        .replace(Range::new(start_offset, end_offset), &new_text)
        .set_cursor(start_offset)
        .show_message(format!("{line_count} lines sorted"))
        .end_undo();

    Ok(effects)
}

/// Extract leading number from a string for numeric sort.
fn extract_leading_number(s: &str) -> i64 {
    let trimmed = s.trim_start();
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() {
        return 0;
    }
    let (neg, start) = match bytes.first() {
        Some(b'-') => (true, 1),
        Some(b'+') => (false, 1),
        _ => (false, 0),
    };
    let mut val: i64 = 0;
    for &b in bytes.get(start..).unwrap_or(&[]) {
        if b.is_ascii_digit() {
            val = val.saturating_mul(10).saturating_add(i64::from(b - b'0'));
        } else {
            break;
        }
    }
    if neg {
        val.saturating_neg()
    } else {
        val
    }
}

/// Move lines to a target address (`:m`).
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn move_lines(
    range: &ExRange,
    target_line: Option<usize>, // 0-indexed destination, None = before first line
    ctx: &ExContext,
) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (first, last) = (resolved.start(), resolved.end());
    let line_start = |line: usize| ctx.line_start_offset(line).map(Offset::get);
    let invalid = || crate::errors::VimError::InvalidRange;
    let len = ctx.text.len();
    // ex_cmds.c do_move(), with `dest` the 1-based line the lines go after.
    let dest = target_line.map_or(0, |t| t.min(ctx.total_lines.saturating_sub(1)) + 1);
    if dest > first && dest <= last {
        return Err(crate::errors::VimError::MoveIntoItself);
    }
    let first_start = line_start(first).ok_or_else(invalid)?;
    let last_start = line_start(last).ok_or_else(invalid)?;
    if dest == first || dest == last + 1 {
        // Nothing moves; the cursor goes to the last line of the range.
        return Ok(Effects::new().set_cursor(Offset::new(last_start)));
    }
    // The lines with their `\n`, or without one when the last is the text's.
    let after_last = line_start(last + 1);
    let moved = ctx
        .text
        .get(first_start..after_last.unwrap_or(len))
        .ok_or_else(invalid)?;

    // Insert at the destination first, then delete the source, as
    // do_move() appends before it deletes.
    let (insert_at, insert_text, delete, block_start) = if dest > last {
        let source = Range::from_raw(first_start, after_last.ok_or_else(invalid)?);
        match line_start(dest) {
            Some(dest_start) => (
                dest_start,
                moved.to_owned(),
                source,
                dest_start - source.len(),
            ),
            // After the last line, which has no `\n`.
            None => (
                len,
                format!("\n{}", moved.strip_suffix('\n').unwrap_or(moved)),
                source,
                len - source.len() + 1,
            ),
        }
    } else {
        let dest_start = line_start(dest).ok_or_else(invalid)?;
        let (insert_text, source) = match after_last {
            Some(end) => (moved.to_owned(), Range::from_raw(first_start, end)),
            // The last line moves up: its `\n` is the one before it.
            None => (
                format!("{moved}\n"),
                Range::from_raw(first_start.saturating_sub(1), len),
            ),
        };
        let shifted = Range::from_raw(
            source.start().get() + insert_text.len(),
            source.end().get() + insert_text.len(),
        );
        (dest_start, insert_text, shifted, dest_start)
    };
    let cursor_pos = block_start + (last_start - first_start);

    // Neovim marks after :move:
    //   '[' = start of first moved line in final position
    //   ']' = start of last moved line in final position
    //   '.' = min of source start and insert position (first affected byte)
    let mark_start = Offset::new(block_start);
    let mark_end = Offset::new(cursor_pos);
    let mark_dot = Offset::new(first_start.min(block_start));

    let line_count = resolved.line_count();
    // Neovim's do_move() calls u_save() twice (once before the insert at
    // the destination, once before the delete at the source), producing two
    // changelist entries.  We use two undo groups so that sync_effects_with_text
    // resets changelist coalescing at the second BeginUndoGroup, allowing each
    // mutation to push its own changelist entry.  Insert first matches Neovim's
    // operation order, yielding changelist entries [insert_pos, delete_pos].
    let effects = Effects::new()
        .begin_undo()
        .insert(Offset::new(insert_at), insert_text)
        .end_undo()
        .begin_undo()
        .delete(delete)
        .set_mark(MarkName::CHANGE_START, mark_start, None)
        .set_mark(MarkName::CHANGE_END, mark_end, None)
        .set_mark(MarkName::LAST_CHANGE, mark_dot, None)
        .set_cursor(Offset::new(cursor_pos))
        .show_message(format!("{line_count} lines moved"))
        .end_undo();

    Ok(effects)
}

/// Copy lines to a target address (`:t` or `:copy`).
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn copy_lines(
    range: &ExRange,
    target_line: Option<usize>, // 0-indexed destination, None = before first line
    ctx: &ExContext,
) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (first, last) = (resolved.start(), resolved.end());
    let line_start = |line: usize| ctx.line_start_offset(line).map(Offset::get);
    let invalid = || crate::errors::VimError::InvalidRange;
    let len = ctx.text.len();
    let first_start = line_start(first).ok_or_else(invalid)?;
    let last_start = line_start(last).ok_or_else(invalid)?;
    let copied = ctx
        .text
        .get(first_start..line_start(last + 1).unwrap_or(len))
        .ok_or_else(invalid)?;
    // ex_cmds.c ex_copy(), with `dest` the 1-based line the copy goes after.
    let dest = target_line.map_or(0, |t| t.min(ctx.total_lines.saturating_sub(1)) + 1);
    let (target_offset, insert_text, base_cursor) = match line_start(dest) {
        Some(at) if copied.ends_with('\n') => (at, copied.to_owned(), at),
        Some(at) => (at, format!("{copied}\n"), at),
        // After the last line, which has no `\n`.
        None => (
            len,
            format!("\n{}", copied.strip_suffix('\n').unwrap_or(copied)),
            len + 1,
        ),
    };
    let target_offset = Offset::new(target_offset);
    let cursor_pos = base_cursor + (last_start - first_start);

    // Neovim marks after :copy:
    //   '[' = start of first copied line (content, after any prepended \n)
    //   ']' = start of last copied line
    //   '.' = same as '['
    let mark_start = Offset::new(base_cursor);
    let mark_end = Offset::new(cursor_pos);

    let line_count = resolved.line_count();
    let effects = Effects::new()
        .begin_undo()
        .insert(target_offset, insert_text)
        .set_mark(MarkName::CHANGE_START, mark_start, None)
        .set_mark(MarkName::CHANGE_END, mark_end, None)
        .set_mark(MarkName::LAST_CHANGE, mark_start, None)
        .set_cursor(Offset::new(cursor_pos))
        .show_message(format!("{line_count} lines copied"))
        .end_undo();

    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    fn ctx() -> ExContext<'static> {
        ExContext::new("cherry\napple\nbanana", 0)
    }

    fn applied(text: &str, effects: &Effects) -> String {
        let mut out = text.to_owned();
        for effect in effects.iter() {
            match effect {
                Effect::Insert { offset, text } => out.insert_str(offset.get(), text),
                Effect::Delete { range } => {
                    out.replace_range(range.start().get()..range.end().get(), "");
                }
                _ => {}
            }
        }
        out
    }

    #[test]
    fn test_sort_alphabetic() {
        let range = ExRange::entire_file();
        let result = sort(&range, &SortOptions::default(), &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_sort_reverse() {
        let range = ExRange::entire_file();
        let options = SortOptions::parse("r");
        let result = sort(&range, &options, &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_sort_numeric() {
        let ctx = ExContext::new("10 items\n2 items\n100 items", 0);
        let range = ExRange::entire_file();
        let options = SortOptions::parse("n");
        let result = sort(&range, &options, &ctx).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_copy_lines() {
        let range = ExRange::single_line(1);
        let result = copy_lines(&range, Some(2), &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Insert { .. })));
    }

    // ── move_lines tests ─────────────────────────────────────────────────

    #[test]
    fn move_lines_down() {
        // Move line 1 ("cherry") after line 3 ("banana").
        // "cherry\napple\nbanana" → delete "cherry\n", insert after "banana".
        let range = ExRange::single_line(1);
        let result = move_lines(&range, Some(2), &ctx()).unwrap();

        // Should have Delete + Insert effects (inside undo group).
        assert!(result.iter().any(|e| matches!(e, Effect::Delete { .. })));
        assert!(result.iter().any(|e| matches!(e, Effect::Insert { .. })));
    }

    #[test]
    fn move_lines_up() {
        // Move line 3 ("banana") before line 1 (target=0 means "before line 1").
        // "cherry\napple\nbanana" → delete "banana", insert at top.
        let ctx = ExContext::new("cherry\napple\nbanana\n", 0);
        let range = ExRange::single_line(3);
        let result = move_lines(&range, None, &ctx).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Delete { .. })));
        assert!(result.iter().any(|e| matches!(e, Effect::Insert { .. })));
    }

    /// ex_cmds.c do_move(): lines moved to where they are do not move, and
    /// the cursor goes to the last of them.
    #[test]
    fn move_lines_after_themselves_does_nothing() {
        for (range, target) in [
            (ExRange::single_line(2), Some(1)),
            (ExRange::single_line(2), Some(0)),
            (ExRange::single_line(1), None),
            (ExRange::lines(1, 2), Some(1)),
        ] {
            let result = move_lines(&range, target, &ctx()).unwrap();
            assert!(!result.iter().any(|e| e.is_text_mutation()), "{range:?}");
        }
        let result = move_lines(&ExRange::lines(1, 2), Some(1), &ctx()).unwrap();
        assert!(result
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { offset } if offset.get() == 7)));
    }

    /// ex_cmds.c do_move(): the last line has no `\n` before or after the
    /// move.
    #[test]
    fn move_lines_keeps_the_text_without_a_last_newline() {
        let apply = |text: &str, range: ExRange, target: Option<usize>| {
            applied(
                text,
                &move_lines(&range, target, &ExContext::new(text, 0)).unwrap(),
            )
        };
        assert_eq!(
            apply("ab\ncd\n😀", ExRange::single_line(3), None),
            "😀\nab\ncd"
        );
        assert_eq!(
            apply("ab\ncd\n😀", ExRange::single_line(1), Some(2)),
            "cd\n😀\nab"
        );
        assert_eq!(apply("a\nb\n", ExRange::single_line(1), Some(2)), "b\n\na");
        assert_eq!(apply("a\nb\nc", ExRange::lines(2, 3), None), "b\nc\na");
        assert_eq!(
            apply("a\nb\nc", ExRange::single_line(1), Some(1)),
            "b\na\nc"
        );
    }

    /// ex_cmds.c `ex_copy()` appends whole lines after the destination.
    #[test]
    fn copy_lines_keeps_the_text_without_a_last_newline() {
        let apply = |text: &str, range: ExRange, target: Option<usize>| {
            let effects = copy_lines(&range, target, &ExContext::new(text, 0)).unwrap();
            let cursor = effects.iter().find_map(|effect| match effect {
                Effect::SetCursor { offset } => Some(offset.get()),
                _ => None,
            });
            (applied(text, &effects), cursor)
        };
        assert_eq!(
            apply("\na\na", ExRange::lines(2, 3), None),
            ("a\na\n\na\na".into(), Some(2))
        );
        assert_eq!(
            apply("ab\n😀", ExRange::single_line(2), Some(1)),
            ("ab\n😀\n😀".into(), Some(8))
        );
        assert_eq!(
            apply("a\nb\n", ExRange::single_line(1), Some(2)),
            ("a\nb\n\na".into(), Some(5))
        );
        assert_eq!(
            apply("a\nb", ExRange::single_line(2), Some(0)),
            ("a\nb\nb".into(), Some(2))
        );
        assert_eq!(
            apply("", ExRange::single_line(1), Some(0)),
            ("\n".into(), Some(1))
        );
    }

    #[test]
    fn move_lines_range_within_itself_returns_error() {
        // Moving lines 1-3 to target 2 (inside the range) → E134 error.
        let range = ExRange::lines(1, 3);
        let result = move_lines(&range, Some(1), &ctx());

        assert!(matches!(
            result,
            Err(crate::errors::VimError::MoveIntoItself)
        ));
    }

    // ── extract_leading_number ───────────────────────────────────────────

    #[test]
    fn extract_leading_number_empty_string() {
        assert_eq!(extract_leading_number(""), 0);
    }

    #[test]
    fn extract_leading_number_whitespace_only() {
        assert_eq!(extract_leading_number("   "), 0);
    }

    #[test]
    fn extract_leading_number_bare_plus() {
        assert_eq!(extract_leading_number("+"), 0);
    }

    #[test]
    fn extract_leading_number_bare_minus() {
        assert_eq!(extract_leading_number("-"), 0);
    }

    #[test]
    fn extract_leading_number_positive() {
        assert_eq!(extract_leading_number("42 apples"), 42);
    }

    #[test]
    fn extract_leading_number_negative() {
        assert_eq!(extract_leading_number("-7 degrees"), -7);
    }

    #[test]
    fn extract_leading_number_explicit_plus() {
        assert_eq!(extract_leading_number("+99 items"), 99);
    }

    #[test]
    fn extract_leading_number_leading_whitespace() {
        assert_eq!(extract_leading_number("  123xyz"), 123);
    }

    #[test]
    fn extract_leading_number_non_digit_prefix() {
        assert_eq!(extract_leading_number("abc123"), 0);
    }

    #[test]
    fn extract_leading_number_zero() {
        assert_eq!(extract_leading_number("0"), 0);
    }

    #[test]
    fn extract_leading_number_negative_zero() {
        assert_eq!(extract_leading_number("-0"), 0);
    }

    #[test]
    fn extract_leading_number_overflow_saturates() {
        // i64::MAX = 9_223_372_036_854_775_807 — use a string far exceeding that
        assert_eq!(
            extract_leading_number("99999999999999999999999999999"),
            i64::MAX
        );
    }

    #[test]
    fn extract_leading_number_negative_overflow_saturates() {
        // Positive accumulation saturates to i64::MAX, then saturating_neg()
        // yields -i64::MAX (= i64::MIN + 1), not i64::MIN.
        assert_eq!(
            extract_leading_number("-99999999999999999999999999999"),
            -i64::MAX
        );
    }
}
