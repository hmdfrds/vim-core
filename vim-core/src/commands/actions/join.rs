//! Join lines actions (`J`, `gJ`).
//!
//! Joins the current line with the line(s) below.
//! Implemented as plain functions, not trait methods — no dynamic dispatch.

use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::byte_delta;
use crate::primitives::{JoinStyle, Offset, Range};

use super::types::ActionContext;

/// Find the end of the current line (newline position).
fn find_line_end(text: &str, from: usize) -> Option<usize> {
    text[from..].find('\n').map(|pos| from + pos)
}

/// Join lines with a space replacing the newline (`J` command).
///
/// Handles both normal and visual mode via `ctx.selection`:
/// - Normal mode: joins `count` lines at cursor
/// - Visual mode: joins all lines spanned by the selection
///
/// # Returns
/// * `CommandResult` with effects to apply
pub fn execute_join(ctx: &ActionContext<'_>) -> CommandResult {
    if let Some(ref selection) = ctx.selection {
        let result = join_from_selection(
            ctx.text,
            selection.anchor().get(),
            selection.head().get(),
            JoinStyle::WithSpace,
        );
        append_visual_exit(result)
    } else {
        join_lines_impl(ctx.text, ctx.cursor, ctx.count, JoinStyle::WithSpace)
    }
}

/// Join lines without adding spaces (`gJ` command).
///
/// Handles both normal and visual mode via `ctx.selection`:
/// - Normal mode: joins `count` lines at cursor
/// - Visual mode: joins all lines spanned by the selection
///
/// # Returns
/// * `CommandResult` with effects to apply
pub fn execute_join_no_space(ctx: &ActionContext<'_>) -> CommandResult {
    if let Some(ref selection) = ctx.selection {
        let result = join_from_selection(
            ctx.text,
            selection.anchor().get(),
            selection.head().get(),
            JoinStyle::NoSpace,
        );
        append_visual_exit(result)
    } else {
        join_lines_impl(ctx.text, ctx.cursor, ctx.count, JoinStyle::NoSpace)
    }
}

/// Join lines from a visual selection.
///
/// Computes the line range from anchor/head, then delegates to `join_lines_impl`.
fn join_from_selection(text: &str, anchor: usize, head: usize, style: JoinStyle) -> CommandResult {
    let sel_start = anchor.min(head);
    let sel_end = anchor.max(head);

    // Gap indexing: sel_end is the exclusive boundary. To find the line of
    // the last *included* character, step back to the previous char boundary.
    // This prevents counting the next line when the head sits right at a line start.
    //
    // Exception: when sel_end == sel_start (zero-width gap selection), don't
    // step back — both endpoints are on the same line.
    let inclusive_end = if sel_end > sel_start {
        crate::primitives::text_util::prev_char_boundary(text, sel_end)
    } else {
        sel_end
    };

    let start_line = text[..sel_start].matches('\n').count();
    let end_line = text[..inclusive_end].matches('\n').count();
    let num_lines = (end_line - start_line + 1).max(2);

    // Find start of first selected line
    let line_start = crate::commands::helpers::line_start_for_offset(text, sel_start);

    let num_lines_u32 = byte_delta::to_u32(num_lines);
    join_lines_impl(text, Offset::new(line_start), num_lines_u32, style)
}

/// Internal implementation for join.
///
/// In Vim, `J` joins current line with next (count=1 or default).
/// `3J` joins 3 lines into one (2 join operations).
/// Emits a single consolidated Replace effect for the entire join range.
fn join_lines_impl(text: &str, cursor: Offset, count: u32, style: JoinStyle) -> CommandResult {
    let cursor_pos = cursor.get();
    let count = count as usize;
    let num_joins = if count > 1 { count - 1 } else { 1 };

    let Some(first_newline) = find_line_end(text, cursor_pos) else {
        return CommandResult::empty(cursor);
    };

    if first_newline + 1 > text.len() {
        return CommandResult::empty(cursor);
    }

    // Edge case: the newline is at the very end of the text (joining with an
    // implied trailing empty line). Neovim treats this as a valid join —
    // delete the trailing newline and set marks.
    if first_newline + 1 == text.len() {
        let range = Range::from_raw(first_newline, text.len());
        // Neovim's J on trailing newline sets cursor to the join point
        // (end of the first line, which is first_newline - 1 for non-empty
        // lines, or 0 for empty lines).
        let cursor_result = if first_newline == 0 {
            Offset::new(0)
        } else {
            Offset::new(first_newline - 1)
        };
        let effects = Effects::new()
            .begin_undo_force_entry()
            .delete(range)
            .set_mark(
                crate::primitives::MarkName::CHANGE_START,
                Offset::new(first_newline),
                None,
            )
            .set_mark(
                crate::primitives::MarkName::CHANGE_END,
                Offset::new(first_newline),
                None,
            )
            .set_mark(
                crate::primitives::MarkName::LAST_CHANGE,
                Offset::new(first_newline),
                None,
            )
            .end_undo()
            .set_cursor(cursor_result);
        return CommandResult::new(effects, cursor_result);
    }

    let prefix = &text[..first_newline];
    let mut remaining = &text[first_newline..];
    let mut parts: Vec<String> = Vec::new();
    let mut total_consumed = first_newline;
    let mut cursor_in_result = first_newline;

    for i in 0..num_joins {
        if remaining.is_empty() || !remaining.starts_with('\n') {
            break;
        }

        let after_newline = &remaining[1..];

        // For gJ: don't strip whitespace. For J: strip leading whitespace.
        let content_offset = if style.adds_space() {
            after_newline
                .char_indices()
                .find(|(_, c)| !c.is_whitespace() || *c == '\n')
                .map_or(after_newline.len(), |(idx, _)| idx)
        } else {
            0
        };

        let line_end_char = if i == 0 {
            prefix.chars().last()
        } else {
            parts.last().and_then(|s: &String| s.chars().last())
        };

        // Check if the next line is empty
        let next_content = &after_newline[content_offset..];
        let next_newline_pos = next_content.find('\n').unwrap_or(next_content.len());
        let next_line_is_empty = next_newline_pos == 0;

        // Current line is empty if it ends with \n or has no content before join point
        let current_line_empty =
            matches!(line_end_char, Some('\n') | None) || (i == 0 && prefix.is_empty());

        let separator = if style.adds_space() {
            if current_line_empty || next_line_is_empty || matches!(line_end_char, Some(' ' | '\t'))
            {
                "" // current/next line empty or already has trailing space
            } else {
                " " // nvim defaults joinspaces=false (no double space after .!?)
            }
        } else {
            ""
        };

        // Cursor at the join point (end of accumulated content so far)
        let prev_parts_len: usize = parts.iter().map(std::string::String::len).sum();
        cursor_in_result = first_newline + prev_parts_len;

        let part = format!("{}{}", separator, &next_content[..next_newline_pos]);

        // If this join adds nothing (empty line, no separator), cursor should
        // be at the last char before the join, not at the join point itself.
        if part.is_empty() && cursor_in_result > 0 {
            cursor_in_result -= 1;
        }

        parts.push(part);

        let consumed = 1 + content_offset + next_newline_pos;
        total_consumed += consumed;
        remaining = &text[total_consumed..];
    }

    let replacement = parts.join("");
    let replacement_len = replacement.len();
    let mark_bracket_start = Offset::new(first_newline);
    let mark_bracket_end = Offset::new(first_newline + replacement_len);
    // Neovim's mark '.' after join always points one past the end of
    // the replacement text. This holds regardless of whether lines
    // remain after the joined region.
    let mark_dot = Offset::new(first_newline + replacement_len + 1);
    let range = Range::from_raw(first_newline, total_consumed);
    let new_cursor = Offset::new(cursor_in_result);

    // Compute the column of the cursor in the post-join line.
    // cursor_in_result is a byte offset into the joined result where
    // everything collapses onto the first line. The column is simply
    // the distance from the start of that line.
    let line_start = crate::commands::helpers::line_start_for_offset(text, cursor_pos);
    let sticky_col = cursor_in_result - line_start;

    let mut effects = Effects::new()
        .begin_undo_force_entry()
        .replace(range, replacement)
        .set_mark(
            crate::primitives::MarkName::CHANGE_START,
            mark_bracket_start,
            None,
        )
        .set_mark(
            crate::primitives::MarkName::CHANGE_END,
            mark_bracket_end,
            None,
        )
        .set_mark(crate::primitives::MarkName::LAST_CHANGE, mark_dot, None)
        .end_undo()
        .set_cursor(new_cursor);
    effects.push(crate::effects::Effect::SetStickyColumn {
        column: Some(crate::primitives::VirtualColumn::new(sticky_col)),
    });

    CommandResult::new(effects, new_cursor)
}

/// Append visual exit effects to a join result.
///
/// Used by visual join to exit visual mode cleanly.
fn append_visual_exit(mut result: CommandResult) -> CommandResult {
    result.effects = result
        .effects
        .clear_selection()
        .set_mode(crate::primitives::Mode::Normal);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;
    use std::num::NonZeroU32;

    fn make_ctx(text: &str, cursor: usize, count: u32) -> ActionContext<'_> {
        ActionContext::from_text_and_cursor(
            text,
            Offset::new(cursor),
            NonZeroU32::new(count).unwrap_or(NonZeroU32::MIN),
        )
    }

    #[test]
    fn test_join_basic() {
        let ctx = make_ctx("hello\nworld", 0, 1);
        let result = execute_join(&ctx);
        assert_eq!(result.effects.len(), 8);
        // "hello\nworld" -> "hello world"
    }

    #[test]
    fn test_join_no_space() {
        let ctx = make_ctx("hello\nworld", 0, 1);
        let result = execute_join_no_space(&ctx);
        assert_eq!(result.effects.len(), 8);
        // "hello\nworld" -> "helloworld"
    }

    #[test]
    fn test_join_with_leading_whitespace() {
        let ctx = make_ctx("hello\n    world", 0, 1);
        let result = execute_join(&ctx);
        assert_eq!(result.effects.len(), 8);
        // "hello\n    world" -> "hello world"
    }

    #[test]
    fn test_join_last_line() {
        let ctx = make_ctx("hello", 0, 1);
        let result = execute_join(&ctx);
        assert!(result.effects.is_empty());
    }

    #[test]
    fn test_join_sentence_end() {
        let ctx = make_ctx("Hello.\nWorld", 0, 1);
        let result = execute_join(&ctx);
        assert_eq!(result.effects.len(), 8);
    }

    #[test]
    fn test_join_fidelity_sentence() {
        // nvim 0.11.6 defaults joinspaces=false: single space after '.'
        let ctx = make_ctx("First sentence.\nSecond sentence.", 0, 1);
        let result = execute_join(&ctx);
        let replace = result
            .effects
            .iter()
            .find(|e| matches!(e, crate::effects::Effect::Replace { .. }));
        if let Some(crate::effects::Effect::Replace { range, text }) = replace {
            assert_eq!(
                text.as_str(),
                " Second sentence.",
                "single space - no joinspaces"
            );
            assert_eq!(range.start().get(), 15);
            assert_eq!(range.end().get(), 32);
        } else {
            panic!("expected Replace effect");
        }
    }

    #[test]
    fn test_join_trailing_space_cursor() {
        // "hello  \n  world" — trailing spaces + indent
        let ctx = make_ctx("hello  \n  world", 0, 1);
        let result = execute_join(&ctx);
        let replace = result
            .effects
            .iter()
            .find(|e| matches!(e, crate::effects::Effect::Replace { .. }));
        if let Some(crate::effects::Effect::Replace { range, text }) = replace {
            assert_eq!(text.as_str(), "world");
        } else {
            panic!("expected Replace effect");
        }
        // Cursor at join point = position of 'w' in "hello  world"
        assert_eq!(result.cursor.unwrap().get(), 7);
    }

    #[test]
    fn test_join_visual() {
        let mut ctx = make_ctx("line1\nline2\nline3", 0, 1);
        ctx.selection = Some(crate::primitives::SelectionRange::new(
            Offset::new(0),
            Offset::new(10),
        ));
        let result = execute_join(&ctx);
        assert!(!result.is_empty());
    }
}
