//! Case toggle actions (`~`, `g~{motion}`).
//!
//! Toggles the case of characters (uppercase ↔ lowercase).
//! Implemented as plain functions, not trait methods — no dynamic dispatch.

use std::num::NonZeroU32;

use super::types::ActionContext;
use crate::commands::helpers::toggle_case_chars;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{MarkName, Offset, Range};
use unicode_segmentation::UnicodeSegmentation;

/// Toggle the case of the character under the cursor (`~` command).
///
/// Toggles uppercase <-> lowercase and advances cursor by one character.
///
/// # Arguments
/// * `ctx` - Case context with text and cursor position
///
/// # Returns
/// * `CommandResult` with effects to apply and new cursor position
pub fn execute_toggle_case_char(ctx: &ActionContext<'_>) -> CommandResult {
    let cursor = ctx.cursor.get();

    // Bounds check
    if cursor >= ctx.text.len() {
        return CommandResult::empty(ctx.cursor);
    }

    // UTF-8 safety: ensure cursor is at a valid character boundary
    if !ctx.text.is_char_boundary(cursor) {
        return CommandResult::empty(ctx.cursor);
    }

    // Get the grapheme cluster at cursor position (handles ZWJ emoji and
    // other multi-codepoint sequences correctly)
    let Some(grapheme) = ctx.text[cursor..].graphemes(true).next() else {
        return CommandResult::empty(ctx.cursor);
    };

    let grapheme_len = grapheme.len();
    let range = Range::from_raw(cursor, cursor + grapheme_len);

    // Toggle case for every codepoint in the grapheme
    let toggled: String = grapheme
        .chars()
        .map(|ch| {
            if ch.is_uppercase() {
                ch.to_lowercase().collect::<String>()
            } else if ch.is_lowercase() {
                ch.to_uppercase().collect::<String>()
            } else {
                // Non-alphabetic: char doesn't change, but Neovim still creates an undo
                // entry (the cursor movement is undoable). Emit a same-char Replace so
                // the undo group records an edit and isn't discarded.
                ch.to_string()
            }
        })
        .collect();

    let new_cursor = ctx.cursor.saturating_add_raw(toggled.len());

    CommandResult::new(
        Effects::new()
            .begin_undo()
            .replace(range, toggled)
            .end_undo(),
        new_cursor,
    )
}

/// Toggle the case of characters in a range (`g~{motion}`).
///
/// Toggles uppercase <-> lowercase for all alphabetic characters in range.
///
/// # Arguments
/// * `text` - Document text
/// * `range` - Range to toggle
///
/// # Returns
/// * `CommandResult` with effects to apply
pub fn toggle_case_range(text: &str, range: Range) -> CommandResult {
    transform_range(text, range, toggle_case_chars)
}

/// Convert range to uppercase (`gU{motion}`).
pub fn uppercase_range(text: &str, range: Range) -> CommandResult {
    transform_range(text, range, str::to_uppercase)
}

/// Convert range to lowercase (`gu{motion}`).
pub fn lowercase_range(text: &str, range: Range) -> CommandResult {
    transform_range(text, range, str::to_lowercase)
}

/// Apply a case transformation to a range of text.
///
/// Shared implementation for `toggle_case_range`, `uppercase_range`, and
/// `lowercase_range`. Handles bounds checking and effect construction.
fn transform_range(
    text: &str,
    range: Range,
    transform: impl FnOnce(&str) -> String,
) -> CommandResult {
    let clamped = range.clamp_end(Offset::new(text.len()));
    let start = clamped.start().get();
    let end = clamped.end().get();

    if start >= end || start >= text.len() {
        return CommandResult::empty(Offset::new(range.start().get()));
    }

    let slice = &text[start..end];
    let transformed = transform(slice);

    CommandResult::new(
        Effects::new().replace(range, transformed),
        Offset::new(range.start().get()),
    )
}

/// Execute swap case action (~ command) with count and line-end clamping.
///
/// Wraps `execute_toggle_case_char` in a count loop and clamps cursor to line end.
/// Moved from `dispatch/action.rs` to keep dispatch as a pure bridge.
pub fn execute_swap_case(ctx: &ActionContext<'_>) -> CommandResult {
    // On an empty line, ~ is a no-op -- Vim does not advance the cursor.
    if ctx.line_start == ctx.line_end {
        let effects = Effects::new()
            .begin_undo()
            .set_cursor(ctx.cursor)
            .end_undo();
        return CommandResult::new(effects, ctx.cursor);
    }

    let start_cursor = ctx.cursor;
    let mut open = Effects::new().begin_undo();
    let mut cursor = ctx.cursor;
    let mut did_change = false;
    for _ in 0..ctx.count {
        // Don't advance past the end of the current line
        if cursor.get() >= ctx.line_end.get() {
            break;
        }
        // Check if this character is alphabetic (will actually toggle case).
        // Non-alpha chars: just advance cursor without emitting Replace.
        // This avoids spurious sync_change_marks calls that would incorrectly
        // update `[`, `]`, `.` marks for no-op character swaps.
        let grapheme = ctx.text[cursor.get()..].graphemes(true).next();
        let is_alpha = grapheme
            .as_ref()
            .is_some_and(|g| g.chars().any(char::is_alphabetic));
        if is_alpha {
            did_change = true;
            let scratch = ActionContext::from_text_and_cursor(ctx.text, cursor, NonZeroU32::MIN);
            let result = execute_toggle_case_char(&scratch);
            open.extend(result.effects);
            cursor = result.cursor.unwrap_or(cursor);
        } else if let Some(g) = grapheme {
            // Non-alpha: advance cursor by grapheme length without any Replace
            cursor = cursor.saturating_add_raw(g.len());
        }
    }
    // Clamp cursor: in normal mode, ~ can't advance past the last
    // character on the line. Find the last valid position.
    // Uses grapheme clusters for correct handling of ZWJ emoji and
    // other multi-codepoint sequences.
    let line_end = ctx.line_end.get();
    if cursor.get() >= line_end && line_end > ctx.line_start.get() {
        let line_text = &ctx.text[ctx.line_start.get()..line_end];
        if let Some(last_grapheme) = line_text.graphemes(true).next_back() {
            cursor = Offset::new(line_end - last_grapheme.len());
        }
    }

    // Neovim's `n_swapchar` sets marks only when at least one character
    // actually changed case (`did_change`). Non-alpha characters don't
    // trigger mark updates.
    //   mark `[` = start position (b_op_start = curpos before loop)
    //   mark `]` = cursor_final - 1 (b_op_end, one before post-advance cursor)
    //   mark `.` = start position
    if did_change {
        let mark_end = cursor.saturating_sub_raw(1);
        open = open
            .set_mark(MarkName::CHANGE_START, start_cursor, None)
            .set_mark(MarkName::CHANGE_END, mark_end, None)
            .set_mark(MarkName::LAST_CHANGE, start_cursor, None);
    }
    let effects = open.set_cursor(cursor).end_undo();
    CommandResult::new(effects, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(text: &str, cursor: usize) -> ActionContext<'_> {
        ActionContext::from_text_and_cursor(text, Offset::new(cursor), NonZeroU32::MIN)
    }

    #[test]
    fn test_toggle_case_char_lowercase() {
        let ctx = make_ctx("hello", 0);
        let result = execute_toggle_case_char(&ctx);
        assert!(!result.effects.is_empty());
        // 'h' -> 'H'
    }

    #[test]
    fn test_toggle_case_char_uppercase() {
        let ctx = make_ctx("HELLO", 0);
        let result = execute_toggle_case_char(&ctx);
        assert!(!result.effects.is_empty());
        // 'H' -> 'h'
    }

    #[test]
    fn test_toggle_case_char_non_alpha() {
        let ctx = make_ctx("123", 0);
        let result = execute_toggle_case_char(&ctx);
        // Non-alpha: emits Replace('1' -> '1') for undo tracking (Neovim parity).
        // The char is unchanged but the undo group records the cursor movement.
        assert!(
            !result.effects.is_empty(),
            "should emit same-char replace for undo"
        );
        assert_eq!(result.cursor.unwrap().get(), 1);
    }

    #[test]
    fn test_toggle_case_char_empty() {
        let ctx = make_ctx("", 0);
        let result = execute_toggle_case_char(&ctx);
        assert!(result.effects.is_empty());
    }

    #[test]
    fn test_toggle_case_range() {
        let range = Range::from_raw(0, 5);
        let result = toggle_case_range("Hello", range);
        assert!(!result.effects.is_empty());
        // "Hello" -> "hELLO"
    }

    #[test]
    fn test_uppercase_range() {
        let range = Range::from_raw(0, 5);
        let result = uppercase_range("hello", range);
        assert!(!result.effects.is_empty());
    }

    #[test]
    fn test_lowercase_range() {
        let range = Range::from_raw(0, 5);
        let result = lowercase_range("HELLO", range);
        assert!(!result.effects.is_empty());
    }

    #[test]
    fn test_toggle_case_unicode() {
        let ctx = make_ctx("uber", 0);
        let result = execute_toggle_case_char(&ctx);
        assert!(!result.effects.is_empty());
        // 'u' -> 'U'
    }
}
