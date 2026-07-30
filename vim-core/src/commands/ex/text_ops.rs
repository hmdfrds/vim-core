//! Text-manipulation ex commands (`:put`, `:retab`, `:left`/`:right`/`:center`).
//!
//! Pure functions that transform text ranges and produce `Effects`.
//! All options/state are resolved by the caller (executor layer) and passed
//! as explicit parameters.

use super::range::resolve_range;
use super::types::{ExContext, ExResult};
use crate::effects::Effects;
use crate::grammar::types::ExRange;
use crate::primitives::{MarkName, Offset, Range};
use unicode_width::UnicodeWidthChar;

// ─────────────────────────────────────────────────────────────────────────────
// :put
// ─────────────────────────────────────────────────────────────────────────────

/// Put register text after/before a line (`:put [register]`).
///
/// `register_text` is the pre-resolved register content.
/// `before` corresponds to `:put!`.
///
/// # Errors
///
/// Returns `VimError` if the target line is out of range.
pub fn put(
    ctx: &ExContext,
    target_line: Option<usize>,
    register_text: &str,
    before: bool,
) -> ExResult {
    let mut insert_text = String::from(register_text);
    // Linewise paste: ensure text ends with newline
    if !insert_text.ends_with('\n') {
        insert_text.push('\n');
    }

    let insert_offset = match target_line {
        // `:0put` — before first line
        None => Offset::new(0),
        Some(tl) if before => ctx.line_start_offset(tl).unwrap_or_else(|| Offset::new(0)),
        Some(tl) if tl + 1 < ctx.total_lines => ctx
            .line_start_offset(tl + 1)
            .unwrap_or_else(|| Offset::new(ctx.text.len())),
        Some(_) => {
            // After last line — append at end
            let off = Offset::new(ctx.text.len());
            if !ctx.text.is_empty() && !ctx.text.ends_with('\n') {
                insert_text.insert(0, '\n');
            }
            off
        }
    };

    // Cursor: first non-blank of first inserted line
    let nl_prefix = usize::from(insert_text.starts_with('\n'));
    let first_line_start = insert_offset.get() + nl_prefix;
    let first_line_text = &insert_text[nl_prefix..];
    let first_line_end = first_line_text.find('\n').unwrap_or(first_line_text.len());
    let fnb = crate::commands::helpers::first_non_blank_in_line(&first_line_text[..first_line_end]);
    let cursor_pos = Offset::new(first_line_start + fnb);

    // Neovim marks after :put:
    //   '[' = start of first inserted content line (after any prepended \n)
    //   '.' = same as '['
    //   ']' is handled correctly by sync_change_marks (auto-mark)
    let mark_start = Offset::new(first_line_start);
    Ok(Effects::new()
        .insert(insert_offset, insert_text)
        .set_mark(MarkName::CHANGE_START, mark_start, None)
        .set_mark(MarkName::LAST_CHANGE, mark_start, None)
        .set_cursor(cursor_pos))
}

// ─────────────────────────────────────────────────────────────────────────────
// :retab
// ─────────────────────────────────────────────────────────────────────────────

/// Replace tabs with spaces (or spaces with tabs if `to_tabs`).
///
/// `tabstop` is the resolved tab width (from `:retab N` or option fallback).
/// `to_tabs` corresponds to the bang (`:retab!`): when true, converts spaces to
/// tabs and operates on ALL whitespace; when false, converts tabs to spaces and
/// only touches leading whitespace (matching Vim's `:retab` semantics).
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn retab(range: &ExRange, tabstop: usize, to_tabs: bool, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_off, end_off) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    let original = &ctx.text[start_off.get()..end_off.get()];
    let tabstop = tabstop.max(1);
    let mut new_text = String::with_capacity(original.len());

    // Vim: `:retab` (no bang) only converts leading whitespace.
    // `:retab!` (bang) converts all whitespace in the line.
    let leading_only = !to_tabs;

    if to_tabs {
        retab_to_tabs(original, tabstop, leading_only, &mut new_text);
    } else {
        retab_to_spaces(original, tabstop, leading_only, &mut new_text);
    }

    if original == new_text {
        return Ok(Effects::new());
    }

    // Cursor position: Neovim places cursor at (tabstop - 1) from the start
    // of the first modified line after retab.  This mirrors the visual column
    // of the last cell occupied by the original first tab stop.
    let first_line_end = new_text.find('\n').unwrap_or(new_text.len());
    let first_line = &new_text[..first_line_end];
    let cursor_col = tabstop
        .saturating_sub(1)
        .min(first_line.len().saturating_sub(1));
    let cursor_off = Offset::new(start_off.get() + cursor_col);

    let line_count = resolved.line_count();
    Ok(Effects::new()
        .replace(Range::new(start_off, end_off), &new_text)
        .set_cursor(cursor_off)
        .show_message(format!("{line_count} lines retabbed")))
}

/// Expand tabs to spaces.
///
/// When `leading_only` is true (`:retab` without bang), only tabs in the
/// leading whitespace of each line are expanded; tabs after the first
/// non-whitespace character are left untouched.
fn retab_to_spaces(text: &str, tabstop: usize, leading_only: bool, out: &mut String) {
    debug_assert!(tabstop > 0);
    let mut col = 0;
    let mut in_leading = true;
    for ch in text.chars() {
        if ch == '\t' && (in_leading || !leading_only) {
            let spaces = tabstop - (col % tabstop);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else if ch == '\n' {
            out.push('\n');
            col = 0;
            in_leading = true;
        } else {
            if ch == '\t' {
                // Tab after leading whitespace in leading_only mode — keep as-is.
                // Advance column to next tab stop for correct alignment tracking.
                col += tabstop - (col % tabstop);
            } else {
                col += UnicodeWidthChar::width(ch).unwrap_or(0);
            }
            if ch != ' ' && ch != '\t' {
                in_leading = false;
            }
            out.push(ch);
        }
    }
}

/// Compress spaces to tabs (`:retab!`).
///
/// When `leading_only` is true, only leading whitespace is converted to tabs.
/// When `leading_only` is false (`:retab!`), all runs of spaces in the line
/// that align to tab stops are also compressed to tabs.
fn retab_to_tabs(text: &str, tabstop: usize, leading_only: bool, out: &mut String) {
    debug_assert!(tabstop > 0);
    for line in text.split_inclusive('\n') {
        let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
        let indent = &line[..indent_len];
        let rest = &line[indent_len..];

        let visual_width = indent_visual_width(indent, tabstop);

        // Convert leading whitespace to tabs + remaining spaces
        let tabs = visual_width / tabstop;
        let spaces = visual_width % tabstop;
        for _ in 0..tabs {
            out.push('\t');
        }
        for _ in 0..spaces {
            out.push(' ');
        }

        if leading_only {
            out.push_str(rest);
        } else {
            compress_body_spaces(rest, tabstop, visual_width, out);
        }
    }
}

/// Compute the visual column width of an indent string containing tabs and spaces.
fn indent_visual_width(indent: &str, tabstop: usize) -> usize {
    let mut width = 0;
    for ch in indent.chars() {
        if ch == '\t' {
            width += tabstop - (width % tabstop);
        } else {
            width += UnicodeWidthChar::width(ch).unwrap_or(0);
        }
    }
    width
}

/// Convert runs of spaces in the line body to tabs at tab-stop boundaries.
fn compress_body_spaces(rest: &str, tabstop: usize, start_col: usize, out: &mut String) {
    let has_nl = rest.ends_with('\n');
    let body = if has_nl {
        &rest[..rest.len() - 1]
    } else {
        rest
    };
    let mut col = start_col;
    let mut space_run = 0;

    for ch in body.chars() {
        if ch == ' ' {
            space_run += 1;
            col += 1;
            if col.is_multiple_of(tabstop) && space_run >= 1 {
                out.push('\t');
                space_run = 0;
            }
        } else {
            for _ in 0..space_run {
                out.push(' ');
            }
            space_run = 0;
            if ch == '\t' {
                col += tabstop - (col % tabstop);
                out.push('\t');
            } else {
                col += UnicodeWidthChar::width(ch).unwrap_or(0);
                out.push(ch);
            }
        }
    }
    for _ in 0..space_run {
        out.push(' ');
    }
    if has_nl {
        out.push('\n');
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// :left / :right / :center
// ─────────────────────────────────────────────────────────────────────────────

/// Left-align lines (`:left [indent]`).
///
/// Strips leading whitespace and optionally prepends `indent` spaces.
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn left(range: &ExRange, indent: usize, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_off, end_off) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    let original = &ctx.text[start_off.get()..end_off.get()];
    let indent_str: String = " ".repeat(indent);
    let mut new_text = String::with_capacity(original.len());

    for line in original.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !trimmed.is_empty() && trimmed != "\n" {
            new_text.push_str(&indent_str);
        }
        new_text.push_str(trimmed);
    }

    if original == new_text {
        return Ok(Effects::new());
    }

    let cursor_off = first_non_blank_offset_in_text(&new_text, start_off);
    Ok(Effects::new()
        .replace(Range::new(start_off, end_off), &new_text)
        .set_cursor(cursor_off))
}

/// Right-align lines (`:right [width]`).
///
/// `width` is the resolved column width (from `:right N` or textwidth/80 fallback).
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn right(range: &ExRange, width: usize, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_off, end_off) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    let original = &ctx.text[start_off.get()..end_off.get()];
    let mut new_text = String::with_capacity(original.len());

    for line in original.split_inclusive('\n') {
        let has_nl = line.ends_with('\n');
        let content = line.trim();
        if content.is_empty() {
            new_text.push_str(line);
        } else {
            let pad = width.saturating_sub(content.len());
            for _ in 0..pad {
                new_text.push(' ');
            }
            new_text.push_str(content);
            if has_nl {
                new_text.push('\n');
            }
        }
    }

    if original == new_text {
        return Ok(Effects::new());
    }

    let cursor_off = first_non_blank_offset_in_text(&new_text, start_off);
    Ok(Effects::new()
        .replace(Range::new(start_off, end_off), &new_text)
        .set_cursor(cursor_off))
}

/// Center-align lines (`:center [width]`).
///
/// `width` is the resolved column width (from `:center N` or textwidth/80 fallback).
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn center(range: &ExRange, width: usize, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_off, end_off) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    let original = &ctx.text[start_off.get()..end_off.get()];
    let mut new_text = String::with_capacity(original.len());

    for line in original.split_inclusive('\n') {
        let has_nl = line.ends_with('\n');
        let content = line.trim();
        if content.is_empty() {
            new_text.push_str(line);
        } else {
            let pad = width.saturating_sub(content.len()) / 2;
            for _ in 0..pad {
                new_text.push(' ');
            }
            new_text.push_str(content);
            if has_nl {
                new_text.push('\n');
            }
        }
    }

    if original == new_text {
        return Ok(Effects::new());
    }

    let cursor_off = first_non_blank_offset_in_text(&new_text, start_off);
    Ok(Effects::new()
        .replace(Range::new(start_off, end_off), &new_text)
        .set_cursor(cursor_off))
}

/// Compute the offset of the first non-blank character in the first line of
/// `text`, adding `base_off` to produce a document-level offset.
fn first_non_blank_offset_in_text(text: &str, base_off: Offset) -> Offset {
    let first_line_end = text.find('\n').unwrap_or(text.len());
    let fnb = crate::commands::helpers::first_non_blank_in_line(&text[..first_line_end]);
    Offset::new(base_off.get() + fnb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    fn ctx() -> ExContext<'static> {
        ExContext::new("  hello\n  world\n  test", 0)
    }

    #[test]
    fn test_left_strips_indent() {
        let range = ExRange::entire_file();
        let effects = left(&range, 0, &ctx()).unwrap();
        let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
        assert!(replace.is_some());
    }

    #[test]
    fn test_left_with_indent() {
        let range = ExRange::entire_file();
        let effects = left(&range, 4, &ctx()).unwrap();
        assert!(effects.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_right_align() {
        let ctx = ExContext::new("hello\nworld", 0);
        let range = ExRange::entire_file();
        let effects = right(&range, 20, &ctx).unwrap();
        assert!(effects.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_center_align() {
        let ctx = ExContext::new("hello\nworld", 0);
        let range = ExRange::entire_file();
        let effects = center(&range, 20, &ctx).unwrap();
        assert!(effects.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_retab_to_spaces() {
        let ctx = ExContext::new("\thello\n\t\tworld", 0);
        let range = ExRange::entire_file();
        let effects = retab(&range, 4, false, &ctx).unwrap();
        assert!(effects.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_retab_to_tabs() {
        let ctx = ExContext::new("    hello\n        world", 0);
        let range = ExRange::entire_file();
        let effects = retab(&range, 4, true, &ctx).unwrap();
        assert!(effects.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_put_after_line() {
        let ctx = ExContext::new("line1\nline2\nline3", 1);
        let effects = put(&ctx, Some(1), "inserted", false).unwrap();
        assert!(effects.iter().any(|e| matches!(e, Effect::Insert { .. })));
    }

    #[test]
    fn test_put_before_line() {
        let ctx = ExContext::new("line1\nline2\nline3", 1);
        let effects = put(&ctx, Some(1), "inserted", true).unwrap();
        assert!(effects.iter().any(|e| matches!(e, Effect::Insert { .. })));
    }

    #[test]
    fn test_no_change_returns_empty() {
        let ctx = ExContext::new("hello\nworld", 0);
        let range = ExRange::entire_file();
        let effects = left(&range, 0, &ctx).unwrap();
        assert!(effects.is_empty());
    }
}
