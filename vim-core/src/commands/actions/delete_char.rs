//! Delete character command (x/X).
//!
//! Deletes characters at/before cursor.
//!
//! # Behavior
//!
//! | Command | Action |
//! |---------|--------|
//! | `x` | Delete character at cursor |
//! | `X` | Delete character before cursor |
//! | `3x` | Delete 3 characters at cursor |
//!
//! # Register Behavior
//!
//! Deleted text is stored in the small delete register (`"-`) and unnamed register (`""`).

use unicode_segmentation::UnicodeSegmentation;

use super::effects::route_action_delete_registers;
use super::types::ActionContext;
use crate::commands::helpers::prev_char_boundary;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{MotionType, Offset, Range};

/// Execute delete character (x command).
///
/// Deletes `count` characters starting at cursor.
/// Stores deleted text in small delete (`-`) and unnamed (`"`) registers.
#[inline]
pub fn execute_delete_char(ctx: &ActionContext<'_>) -> CommandResult {
    let cursor_pos = ctx.cursor.get();
    let text_len = ctx.text.len();

    // Nothing to delete if cursor at end or past it.
    // Still emit SetCursor so the effect processor's auto-emit
    // recomputes sticky_column (curswant). In Neovim, `x` goes through
    // the operator pipeline even when nothing is deleted, which resets
    // curswant from END_OF_LINE to the actual column.
    if cursor_pos >= text_len {
        let effects = Effects::new().set_cursor(ctx.cursor);
        return CommandResult::new(effects, ctx.cursor);
    }

    // Walk grapheme cluster boundaries — Neovim's `x` deletes whole grapheme
    // clusters (base char + combining marks, emoji sequences, etc.), not
    // individual Unicode codepoints. In Vim, `x` never deletes newline
    // characters — stop at line boundary.
    let mut end = cursor_pos;
    let remaining = &ctx.text[cursor_pos..];
    for g in remaining.graphemes(true).take(ctx.count_usize()) {
        if g == "\n" {
            break;
        }
        end += g.len();
    }

    if end == cursor_pos {
        let effects = Effects::new().set_cursor(ctx.cursor);
        return CommandResult::new(effects, ctx.cursor);
    }

    // Extract the text to delete (for register storage)
    let deleted_text = &ctx.text[cursor_pos..end];
    let delete_range = Range::new(Offset::new(cursor_pos), Offset::new(end));

    // New cursor stays at same position, clamped to end of line if needed
    let after_delete_text_len = text_len - (end - cursor_pos);
    let new_cursor_pos = if end >= text_len {
        prev_char_boundary(ctx.text, cursor_pos).max(ctx.line_start.get())
    } else if cursor_pos < after_delete_text_len
        && ctx.text.as_bytes().get(end).copied() == Some(b'\n')
    {
        if cursor_pos > ctx.line_start.get() {
            prev_char_boundary(ctx.text, cursor_pos).max(ctx.line_start.get())
        } else {
            ctx.line_start.get()
        }
    } else {
        cursor_pos
    };

    let mut open = Effects::new().begin_undo().delete(delete_range);
    open = route_action_delete_registers(
        open,
        ctx.register_name,
        deleted_text,
        MotionType::CharWise,
        false,
    );
    let effects = open.set_cursor(Offset::new(new_cursor_pos)).end_undo();

    CommandResult::new(effects, Offset::new(new_cursor_pos))
}

/// Execute delete character before cursor (X command).
///
/// Deletes `count` characters before cursor.
/// Stores deleted text in small delete (`-`) and unnamed (`"`) registers.
#[inline]
pub fn execute_delete_char_back(ctx: &ActionContext<'_>) -> CommandResult {
    let cursor_pos = ctx.cursor.get();
    let line_start = ctx.line_start.get();

    // Nothing to delete if at start of line
    if cursor_pos <= line_start {
        return CommandResult::empty(ctx.cursor);
    }

    // Walk grapheme cluster boundaries backward — Neovim's `X` deletes whole
    // grapheme clusters (base char + combining marks, etc.), not individual
    // codepoints. Uses `rev()` on the grapheme iterator for reverse traversal.
    let before_cursor = &ctx.text[line_start..cursor_pos];
    let grapheme_byte_lens: Vec<usize> = before_cursor
        .graphemes(true)
        .rev()
        .take(ctx.count_usize())
        .map(str::len)
        .collect();
    let bytes_back: usize = grapheme_byte_lens.iter().sum();
    let delete_start = cursor_pos - bytes_back;

    if delete_start == cursor_pos {
        return CommandResult::empty(ctx.cursor);
    }

    let deleted_text = &ctx.text[delete_start..cursor_pos];
    let delete_range = Range::new(Offset::new(delete_start), Offset::new(cursor_pos));

    // Cursor moves left by number of deleted characters
    let new_cursor_pos = delete_start;

    let mut open = Effects::new().begin_undo().delete(delete_range);
    open = route_action_delete_registers(
        open,
        ctx.register_name,
        deleted_text,
        MotionType::CharWise,
        false,
    );
    let effects = open.set_cursor(Offset::new(new_cursor_pos)).end_undo();

    CommandResult::new(effects, Offset::new(new_cursor_pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(text: &str, cursor: usize, line_start: usize, count: u32) -> ActionContext<'_> {
        ActionContext::new(
            text,
            Offset::new(cursor),
            Offset::new(line_start),
            Offset::new(text.len()),
            None,
            count,
        )
    }

    #[test]
    fn test_delete_char_single() {
        let ctx = make_context("hello", 0, 0, 1);
        let result = execute_delete_char(&ctx);
        assert!(!result.is_empty());
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn test_delete_char_with_count() {
        let ctx = make_context("hello", 0, 0, 3);
        let result = execute_delete_char(&ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_delete_char_at_end() {
        let ctx = make_context("hello", 5, 0, 1);
        let result = execute_delete_char(&ctx);
        // Even when nothing is deleted, SetCursor is emitted so the effect
        // processor's auto-emit resets curswant (Neovim compatibility).
        assert_eq!(result.cursor.unwrap().get(), 5);
    }

    #[test]
    fn test_delete_char_back_single() {
        let ctx = make_context("hello", 3, 0, 1);
        let result = execute_delete_char_back(&ctx);
        assert!(!result.is_empty());
        assert_eq!(result.cursor.unwrap().get(), 2);
    }

    #[test]
    fn test_delete_char_back_at_start() {
        let ctx = make_context("hello", 0, 0, 1);
        let result = execute_delete_char_back(&ctx);
        assert!(result.is_empty());
    }
}
