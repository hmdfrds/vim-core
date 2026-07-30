//! Format operator (gq).
//!
//! Formats text, typically by wrapping to textwidth.
//!
//! # Behavior
//!
//! - `gqq` - Format current line
//! - `gqap` - Format paragraph
//!
//! In many implementations, this delegates to the shell for actual formatting.

use super::types::{extract_range_text, OperatorContext};
use crate::commands::helpers::{
    first_non_blank_in_line, line_end_for_offset, line_start_for_offset,
};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{MarkName, Offset};
use compact_str::CompactString;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

/// Execute format operator (gq).
///
/// No traits, just functions + enum dispatch.
pub fn execute(ctx: &OperatorContext<'_>) -> CommandResult {
    if ctx.is_empty() {
        // Neovim always sets `[` and `]` marks even on empty buffer gq.
        let effects = Effects::new()
            .set_mark(MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(MarkName::CHANGE_END, ctx.cursor, None)
            .set_cursor(ctx.cursor);
        return CommandResult::new(effects, ctx.cursor);
    }

    // Get text in range
    let original = extract_range_text(ctx.text, ctx.range);

    // Neovim: when textwidth=0, gq uses 79 as the wrap width.
    let textwidth = if ctx.textwidth == 0 {
        79
    } else {
        ctx.textwidth
    };

    // If the text is a single line and fits within textwidth, no formatting needed.
    // gq only reformats when text exceeds textwidth or spans multiple lines.
    let needs_wrap =
        original.contains('\n') || UnicodeWidthStr::width(original.as_str()) > textwidth;

    let mut formatted = if needs_wrap {
        wrap_text(&original, textwidth)
    } else {
        original.to_string()
    };

    // Preserve trailing newlines from original text.
    // Count how many trailing newlines the original had, and ensure
    // the formatted text has the same number. This preserves blank line
    // separators (e.g., gq} includes trailing \n\n for paragraph boundary).
    let orig_trailing = original.bytes().rev().take_while(|&b| b == b'\n').count();
    let fmt_trailing = formatted.bytes().rev().take_while(|&b| b == b'\n').count();
    if orig_trailing > fmt_trailing {
        for _ in 0..(orig_trailing - fmt_trailing) {
            formatted.push('\n');
        }
    }

    // Only create effect if text actually changed
    if formatted == original.as_str() {
        // No change -- cursor goes to first non-blank of last line in range
        // for text objects, or line at range_end for motions.
        // Neovim always sets `[` and `]` marks even when gq is a no-op.
        let range_end = ctx.range.end().min(Offset::new(ctx.text.len()));
        let new_cursor = if ctx.origin == super::types::OperatorOrigin::TextObject {
            let last_line_pos =
                find_last_line_in_range(ctx.text, ctx.range.start().get(), range_end.get());
            Offset::new(find_first_non_blank(ctx.text, last_line_pos))
        } else if range_end.get() >= ctx.text.len() {
            Offset::new(find_first_non_blank(ctx.text, ctx.range.start().get()))
        } else {
            Offset::new(find_first_non_blank(ctx.text, range_end.get()))
        };
        // Neovim's gq sets `[` = start of range, `]` = cursor position
        // after formatting (b_op_end = curwin->w_cursor from op_format).
        let mark_start = ctx.range.start();
        CommandResult::new(
            Effects::new()
                .set_mark(MarkName::CHANGE_START, mark_start, None)
                .set_mark(MarkName::CHANGE_END, new_cursor, None)
                .set_cursor(new_cursor),
            new_cursor,
        )
    } else {
        // Build the new text to compute cursor position correctly
        let mut new_text = String::with_capacity(ctx.text.len());
        new_text.push_str(&ctx.text[..ctx.range.start().get()]);
        new_text.push_str(&formatted);
        let range_end = ctx.range.end().min(Offset::new(ctx.text.len()));
        new_text.push_str(&ctx.text[range_end.get()..]);

        let formatted_end = ctx.range.start().get() + formatted.len();

        // Cursor placement after text change:
        // - Text objects: first non-blank of last line in formatted region
        // - Motions where formatted text is strictly shorter (bytes reduced from
        //   joining lines): first non-blank of first line in range
        // - Otherwise: first non-blank of line at formatted_end
        let new_cursor = if ctx.origin == super::types::OperatorOrigin::TextObject {
            let last_line_pos =
                find_last_line_in_range(&new_text, ctx.range.start().get(), formatted_end);
            Offset::new(find_first_non_blank(&new_text, last_line_pos))
        } else if formatted.len() < original.len() {
            // Formatted text is strictly shorter -- lines were joined and indentation
            // was removed. Cursor at first line of range.
            Offset::new(find_first_non_blank(&new_text, ctx.range.start().get()))
        } else if formatted_end >= new_text.len() {
            Offset::new(find_first_non_blank(&new_text, ctx.range.start().get()))
        } else {
            Offset::new(find_first_non_blank(&new_text, formatted_end))
        };

        // Neovim sets `[` = start of formatted range, `]` = cursor position
        // after formatting (first non-blank of last formatted line).
        // Mark `.` = end of formatted region (range_start + formatted.len()),
        // matching Neovim's changed_lines() which records the end of the
        // change range. The Replace effect's sync_change_marks sets mark `.`
        // to range start, so we override it explicitly.
        let mark_start = ctx.range.start();
        // Mark `.` = end of formatted content. In Neovim, format_lines()
        // only operates on content lines. One trailing newline is the
        // paragraph terminator (part of content); any beyond that are
        // blank-line separators preserved from the original, which mark `.`
        // should NOT include.
        let extra_preserved_newlines = orig_trailing.saturating_sub(1);
        let mark_dot = Offset::new(formatted_end - extra_preserved_newlines);
        let effects = Effects::new()
            .begin_undo()
            .replace(ctx.range, CompactString::new(&formatted))
            .set_mark(MarkName::CHANGE_START, mark_start, None)
            .set_mark(MarkName::CHANGE_END, new_cursor, None)
            .set_mark(MarkName::LAST_CHANGE, mark_dot, None)
            .set_cursor(new_cursor)
            .end_undo();

        CommandResult::new(effects, new_cursor)
    }
}

/// Execute format operator keeping cursor position (gw).
///
/// Same as `execute` (gq) but the cursor stays at the original position
/// instead of moving to the first non-blank after the formatted region.
pub fn execute_keep_cursor(ctx: &OperatorContext<'_>) -> CommandResult {
    if ctx.is_empty() {
        return CommandResult::empty(ctx.cursor);
    }

    let original = extract_range_text(ctx.text, ctx.range);
    let textwidth = if ctx.textwidth == 0 {
        79
    } else {
        ctx.textwidth
    };

    // Same as gq: only wrap if text spans lines or exceeds textwidth.
    let needs_wrap =
        original.contains('\n') || UnicodeWidthStr::width(original.as_str()) > textwidth;

    let mut formatted = if needs_wrap {
        wrap_text(&original, textwidth)
    } else {
        original.to_string()
    };

    // Preserve trailing newlines from original text.
    let orig_trailing = original.bytes().rev().take_while(|&b| b == b'\n').count();
    let fmt_trailing = formatted.bytes().rev().take_while(|&b| b == b'\n').count();
    if orig_trailing > fmt_trailing {
        for _ in 0..(orig_trailing - fmt_trailing) {
            formatted.push('\n');
        }
    }

    // Neovim's gw sets `[` = start, `]` = first non-blank of first line,
    // regardless of whether text changed (same as gq marks).
    let mark_start = ctx.range.start();
    let mark_end = Offset::new(find_first_non_blank(ctx.text, ctx.range.start().get()));

    if formatted == original.as_str() {
        // No change -- cursor stays where it was
        CommandResult::new(
            Effects::new()
                .set_mark(MarkName::CHANGE_START, mark_start, None)
                .set_mark(MarkName::CHANGE_END, mark_end, None)
                .set_cursor(ctx.cursor),
            ctx.cursor,
        )
    } else {
        // Build new text for correct first-non-blank computation
        let mut new_text = String::with_capacity(ctx.text.len());
        new_text.push_str(&ctx.text[..ctx.range.start().get()]);
        new_text.push_str(&formatted);
        let range_end = ctx.range.end().min(Offset::new(ctx.text.len()));
        new_text.push_str(&ctx.text[range_end.get()..]);
        let mark_end_changed =
            Offset::new(find_first_non_blank(&new_text, ctx.range.start().get()));

        let effects = Effects::new()
            .begin_undo()
            .replace(ctx.range, CompactString::new(&formatted))
            .set_mark(MarkName::CHANGE_START, mark_start, None)
            .set_mark(MarkName::CHANGE_END, mark_end_changed, None)
            .set_cursor(ctx.cursor)
            .end_undo();

        CommandResult::new(effects, ctx.cursor)
    }
}

/// Simple word-wrap algorithm.
///
/// Wraps text to the specified width while preserving paragraph breaks
/// and first-line indentation (Vim behavior).
fn wrap_text(text: &str, width: usize) -> String {
    let mut result = String::with_capacity(text.len());

    for paragraph in text.split("\n\n") {
        if !result.is_empty() {
            result.push_str("\n\n");
        }

        // Capture first line's indentation as a slice
        let indent_end = paragraph
            .chars()
            .take_while(|c| c.is_whitespace() && *c != '\n')
            .map(char::len_utf8)
            .sum::<usize>();
        let indent = &paragraph[..indent_end];
        let indent_len = indent.chars().fold(0usize, |col, c| {
            if c == '\t' {
                // Tab advances to next tabstop (default 8)
                col + (8 - col % 8)
            } else {
                col + UnicodeWidthChar::width(c).unwrap_or(0)
            }
        });
        let effective_width = width.saturating_sub(indent_len);

        // Re-wrap words directly without intermediate Vec
        let mut current_line_len = 0;
        for word in paragraph.split_whitespace() {
            let word_len = UnicodeWidthStr::width(word);

            if current_line_len == 0 {
                // Start of line -- prepend indent
                result.push_str(indent);
                result.push_str(word);
                current_line_len = word_len;
            } else if current_line_len + 1 + word_len <= effective_width {
                // Fits on current line
                result.push(' ');
                result.push_str(word);
                current_line_len += 1 + word_len;
            } else {
                // Need new line -- prepend indent
                result.push('\n');
                result.push_str(indent);
                result.push_str(word);
                current_line_len = word_len;
            }
        }
    }

    result
}

/// Find a position on the last line within the range [start, end).
///
/// If the range ends with a newline, the last line is the one just before that
/// trailing newline (not the line after it). This matches Vim's `gq` behavior
/// where cursor lands on the last line of the formatted text.
fn find_last_line_in_range(text: &str, start: usize, end: usize) -> usize {
    let end = end.min(text.len());
    if end == start {
        return start;
    }
    // If range ends exactly at or past a newline, back up to the previous line.
    // `end == 0` was excluded above (`end == start` returns early only when they
    // are equal, so guard the subtraction explicitly).
    let byte_before_end = end.checked_sub(1).and_then(|i| text.as_bytes().get(i));
    let ends_on_newline = byte_before_end == Some(&b'\n');
    let last_byte = if end > 0 && (end >= text.len() || ends_on_newline) {
        // Back up past the trailing newline to find the last content line
        if end > start && ends_on_newline {
            end - 1
        } else {
            end
        }
    } else {
        end
    };
    // Now find the start of the line containing last_byte
    line_start_for_offset(text, last_byte.min(text.len().saturating_sub(1)))
}

/// Find first non-blank character on the line containing `offset`.
///
/// Uses canonical helpers from `commands/helpers`.
fn find_first_non_blank(text: &str, offset: usize) -> usize {
    let start = offset.min(text.len());
    let ls = line_start_for_offset(text, start);
    let le = line_end_for_offset(text, ls);
    let line = &text[ls..le];
    ls + first_non_blank_in_line(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{MotionType, Range};

    #[test]
    fn test_wrap_text_simple() {
        let text = "hello world foo bar";
        let wrapped = wrap_text(text, 12);
        // "hello world" is 11 chars, fits
        // "foo bar" is 7 chars, fits on next line
        assert!(wrapped.contains('\n'));
    }

    #[test]
    fn test_wrap_preserves_paragraphs() {
        let text = "para one\n\npara two";
        let wrapped = wrap_text(text, 80);
        assert!(wrapped.contains("\n\n"), "Should preserve paragraph breaks");
    }

    #[test]
    fn test_format_operator() {
        let ctx = OperatorContext::new(
            "hello world foo bar baz qux",
            Range::from_raw(0, 27),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Should have effects (at minimum set_cursor)
        assert!(result.effects.len() >= 1);
    }

    #[test]
    fn test_find_first_non_blank() {
        assert_eq!(find_first_non_blank("  hello", 0), 2);
        assert_eq!(find_first_non_blank("hello", 0), 0);
        assert_eq!(find_first_non_blank("\thello", 0), 1);
    }
}
