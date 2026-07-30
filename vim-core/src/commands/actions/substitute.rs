//! Substitute command (s).
//!
//! Deletes characters at cursor and enters insert mode.
//!
//! # Behavior
//!
//! | Command | Action |
//! |---------|--------|
//! | `s`     | Delete char at cursor, enter insert mode |
//! | `5s`    | Delete 5 chars at cursor, enter insert mode |
//!
//! Equivalent to `cl` (change-right) but as a single action.
//!
//! # Register Behavior
//!
//! Deleted text stored in small delete register (`"-`) and unnamed register (`""`).

use super::effects::route_action_delete_registers;
use super::types::ActionContext;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::InsertEntryType;
use crate::primitives::{MotionType, Offset, Range};

/// Execute substitute command (`s`).
///
/// Deletes `count` characters at cursor (char-boundary aware), then
/// enters insert mode via `BeginInsert`. Cursor stays at deletion start.
#[inline]
pub fn execute_substitute(ctx: &ActionContext<'_>) -> CommandResult {
    let cursor_pos = ctx.cursor.get();
    let text = ctx.text;

    // If cursor is at or past end, just enter insert mode at cursor
    if cursor_pos >= text.len() {
        let effects = Effects::new()
            .begin_undo()
            .begin_insert(
                InsertEntryType::SubstituteChar,
                1,
                0,
                Offset::new(cursor_pos),
            )
            .into_raw_closed();
        return CommandResult::new(effects, ctx.cursor);
    }

    // Walk character boundaries — count is CHARACTER count
    let mut end = cursor_pos;
    for c in text[cursor_pos..].chars().take(ctx.count_usize()) {
        // Don't cross newline boundaries — `s` stays within current line
        if c == '\n' {
            break;
        }
        end += c.len_utf8();
    }

    if end == cursor_pos {
        // Nothing to delete (e.g., cursor on newline), just enter insert
        let effects = Effects::new()
            .begin_undo()
            .begin_insert(
                InsertEntryType::SubstituteChar,
                1,
                0,
                Offset::new(cursor_pos),
            )
            .into_raw_closed();
        return CommandResult::new(effects, ctx.cursor);
    }

    let deleted_text = &text[cursor_pos..end];
    let delete_range = Range::new(Offset::new(cursor_pos), Offset::new(end));

    // Build effects: undo → delete → register → cursor → begin_insert
    // The undo group stays open — insert mode exit (exit_finalize) will close it.
    // This ensures the delete and subsequent insert are a single undo operation.
    let mut effects = Effects::new()
        .begin_undo()
        .delete(delete_range)
        .set_cursor(Offset::new(cursor_pos));

    effects = route_action_delete_registers(
        effects,
        ctx.register_name,
        deleted_text,
        MotionType::CharWise,
        false,
    );
    let effects = effects.begin_insert(
        InsertEntryType::SubstituteChar,
        1,
        0,
        Offset::new(cursor_pos),
    );

    CommandResult::new(effects.into_raw_closed(), Offset::new(cursor_pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(text: &str, cursor: usize, count: u32) -> ActionContext<'_> {
        ActionContext::new(
            text,
            Offset::new(cursor),
            Offset::new(0),
            Offset::new(text.len()),
            None,
            count,
        )
    }

    #[test]
    fn substitute_single_char() {
        let ctx = make_ctx("hello", 0, 1);
        let result = execute_substitute(&ctx);
        assert!(!result.is_empty());
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn substitute_with_count() {
        let ctx = make_ctx("hello", 0, 3);
        let result = execute_substitute(&ctx);
        assert!(!result.is_empty());
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn substitute_at_end() {
        let ctx = make_ctx("hello", 5, 1);
        let result = execute_substitute(&ctx);
        // Should still enter insert mode even at end
        assert!(!result.is_empty());
    }

    #[test]
    fn substitute_cjk() {
        let ctx = make_ctx("你好世界", 0, 1);
        let result = execute_substitute(&ctx);
        assert!(!result.is_empty());
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn substitute_stops_at_newline() {
        let ctx = make_ctx("ab\ncd", 1, 5);
        let result = execute_substitute(&ctx);
        // Should only delete 'b', not cross newline
        assert!(!result.is_empty());
        assert_eq!(result.cursor.unwrap().get(), 1);
    }
}
