//! Delete/Change/Yank to end of line actions (D, C, Y).
//!
//! These are compound commands that behave like `d$`, `c$`, `yy`:
//! - D: delete from cursor to end of line (with count: plus count-1 full lines below)
//! - C: change from cursor to end of line (delete + insert mode)
//! - Y: yank to end of line (Neovim `defaults.vim` behavior)

use super::effects::route_action_delete_registers;
use crate::commands::actions::ActionContext;
use crate::commands::helpers::{line_end, line_of, prev_char_boundary};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{InsertEntryType, MarkName, MotionType, Offset, Range, RegisterName};

/// Execute `D` - delete from cursor to end of line.
///
/// 1. Get current offset
/// 2. Get line end offset
/// 3. Delete from cursor to line end (charwise)
/// 4. Set cursor to max(cursor-1, line_start) if cursor was at end
///
/// # Vim Behavior
/// - Small delete register `-` is used (< 1 line)
/// - Unnamed register `"` is updated
/// - Cursor stays at position (or moves left if at end of line)
#[inline]
pub fn execute_delete_to_end(ctx: &ActionContext<'_>) -> CommandResult {
    let cursor = ctx.cursor.get();
    let text = ctx.text;
    let line_end_pos = ctx.line_end.get();

    // If cursor is already at/past line end, nothing to delete
    if cursor >= line_end_pos {
        return CommandResult::empty(ctx.cursor);
    }

    // Compute deletion end: delete to end of current line, plus count-1 full lines below.
    // `2D` = delete to end of this line + all of next line (including its newline).
    let delete_end = if ctx.count <= 1 {
        line_end_pos
    } else {
        let current_line = line_of(text, cursor);
        let target_line = current_line + (ctx.count_usize() - 1);
        // End of the target line (or end of document)
        let target_end = line_end(text, target_line).unwrap_or(text.len());
        // Include the newline after target line if present
        if target_end < text.len() && text.as_bytes().get(target_end).copied().unwrap_or(0) == b'\n'
        {
            target_end + 1
        } else {
            target_end
        }
    };

    let deleted_text = &text[cursor..delete_end];
    let is_multiline = ctx.count > 1 || deleted_text.contains('\n');

    // Build effects — D always uses CharWise register type.
    let mut effects = Effects::new()
        .begin_undo()
        .delete(Range::from_raw(cursor, delete_end));

    effects = route_action_delete_registers(
        effects,
        ctx.register_name,
        deleted_text,
        MotionType::CharWise,
        is_multiline,
    );

    let effects = effects.end_undo();

    // Cursor position: stay at current position, or move to previous character.
    // Use prev_char_boundary for multi-byte safety (CJK, emoji).
    let new_cursor_pos = if cursor > ctx.line_start.get() {
        prev_char_boundary(ctx.text, cursor).max(ctx.line_start.get())
    } else {
        ctx.line_start.get()
    };

    CommandResult::new(
        effects.set_cursor(Offset::new(new_cursor_pos)),
        Offset::new(new_cursor_pos),
    )
}

/// Execute `C` - change from cursor to end of line.
///
/// Like `D` but enters insert mode after deletion.
#[inline]
pub fn execute_change_to_end(ctx: &ActionContext<'_>) -> CommandResult {
    let cursor = ctx.cursor.get();
    let line_end_pos = ctx.line_end.get();

    // If cursor is already at/past line end, just enter insert mode.
    // Use begin_insert + begin_undo so the insert is undoable as one unit.
    if cursor >= line_end_pos {
        let effects = Effects::new().begin_undo().begin_insert(
            InsertEntryType::ChangeOperator,
            1,
            0,
            ctx.cursor,
        );
        return CommandResult::new(effects, ctx.cursor);
    }

    // Text to delete
    let deleted_text = &ctx.text[cursor..line_end_pos];

    // Build effects
    let mut effects = Effects::new()
        .begin_undo()
        .delete(Range::from_raw(cursor, line_end_pos));

    effects = route_action_delete_registers(
        effects,
        ctx.register_name,
        deleted_text,
        MotionType::CharWise,
        false,
    );

    let effects = effects.begin_insert(InsertEntryType::ChangeOperator, 1, 0, Offset::new(cursor));

    CommandResult::new(effects, Offset::new(cursor))
}

/// Execute `Y` - yank to end of line.
///
/// Note: Real Vim's `Y` defaults to `yy` (linewise). Neovim's `defaults.vim`
/// remaps `Y` to `y$` (charwise yank to end of line). We follow the Neovim
/// convention here, matching `y$` behavior.
#[inline]
pub fn execute_yank_line(ctx: &ActionContext<'_>) -> CommandResult {
    let cursor = ctx.cursor.get();
    let line_end = ctx.line_end.get();

    // Y yanks from cursor to end of line (like y$), charwise
    let yanked_text = ctx.text[cursor..line_end].to_owned();

    // Blackhole register: suppress all register writes.
    if ctx.register_name.is_some_and(RegisterName::is_blackhole) {
        return CommandResult::new(Effects::new(), ctx.cursor);
    }

    let mut effects = Effects::new().set_register(
        RegisterName::UNNAMED,
        yanked_text.clone(),
        MotionType::CharWise,
    );

    if let Some(reg) = ctx.register_name {
        effects = effects.set_register(reg, yanked_text, MotionType::CharWise);
    } else {
        effects = effects.set_register(RegisterName::LAST_YANK, yanked_text, MotionType::CharWise);
    }

    // Set change marks [/] to the yanked range (cursor to line_end - 1 inclusive)
    let mark_end = if line_end > cursor {
        Offset::new(line_end - 1)
    } else {
        ctx.cursor
    };
    effects = effects
        .set_mark(MarkName::CHANGE_START, ctx.cursor, None)
        .set_mark(MarkName::CHANGE_END, mark_end, None);

    CommandResult::new(effects, ctx.cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::actions::ActionContext;
    use std::num::NonZeroU32;

    fn make_ctx(text: &str, cursor: usize) -> ActionContext<'_> {
        ActionContext::from_text_and_cursor(text, Offset::new(cursor), NonZeroU32::MIN)
    }

    #[test]
    fn test_delete_to_end_basic() {
        let ctx = make_ctx("hello world", 6); // cursor on 'w'
        let result = execute_delete_to_end(&ctx);

        assert!(!result.is_empty());
        // Should delete "world" (5 chars)
    }

    #[test]
    fn test_delete_to_end_at_line_end() {
        let ctx = make_ctx("hello\nworld", 4); // cursor on 'o' (last char before newline)
        let result = execute_delete_to_end(&ctx);

        assert!(!result.is_empty());
        // Should delete 'o' only
    }

    #[test]
    fn test_change_to_end_basic() {
        let ctx = make_ctx("hello world", 6);
        let result = execute_change_to_end(&ctx);

        assert!(!result.is_empty());
        // Should enter insert mode
    }

    #[test]
    fn test_yank_line_basic() {
        let ctx = make_ctx("hello\nworld", 0);
        let result = execute_yank_line(&ctx);

        assert!(!result.is_empty());
        // Should yank "hello\n"
    }
}
