//! Change operator (c).
//!
//! Deletes text and enters insert mode.
//!
//! # Behavior
//!
//! - `cw` - Change word (note: cw at word boundary works like ce)
//! - `cc` - Change line
//! - `c$` - Change to end of line (same as C)
//!
//! This is essentially delete + enter insert mode.

use super::types::{extract_range_text, normalize_linewise_register, OperatorContext};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::InsertEntryType;
use crate::primitives::{Offset, Range};

/// Execute change operator.
///
/// No traits, just functions + enum dispatch.
///
/// The undo group is left **open** (no `end_undo()`) — it is closed by
/// `exit_finalize()` when the user leaves insert mode, so that both the
/// deletion and the subsequently typed text form a single undoable atom.
pub fn execute(ctx: &OperatorContext<'_>) -> CommandResult {
    // Empty range: for charwise, just enter insert mode
    // For linewise (cc on empty line), still need to store newline in register
    if ctx.is_empty() && !ctx.is_linewise() {
        let effects = Effects::new().begin_undo().begin_insert(
            InsertEntryType::ChangeOperator,
            1,
            0,
            ctx.cursor,
        );
        return CommandResult::new(effects, ctx.cursor);
    }

    // Handle linewise on effectively empty line (stores newline in register)
    if ctx.is_empty() && ctx.is_linewise() {
        // cc on empty line: enter insert mode and route registers properly
        let register_text = "\n".to_owned();
        let effects = super::registers::route_delete_registers(
            Effects::new().begin_undo(),
            &register_text,
            ctx.motion_type,
            ctx.register,
            true,
            ctx.force_numbered_register,
        )
        .begin_insert(InsertEntryType::ChangeOperator, 1, 0, ctx.cursor);
        return CommandResult::new(effects, ctx.cursor);
    }

    // For linewise change, preserve the trailing newline visually
    // But store the full linewise text (with newline) in the register
    let effective_range = if ctx.is_linewise() {
        let range_text = ctx.range_text();
        let mut start_offset = ctx.range.start().get();
        let mut end_offset = ctx.range.end().get();

        // Strip trailing newline from deletion
        if range_text.ends_with('\n') {
            end_offset = end_offset.saturating_sub(1);
        }

        // EOF backward extension fix: when changing the last line(s),
        // extend_to_full_lines may include the preceding newline separator
        // (so that a linewise delete doesn't leave a trailing \n).
        // For *change* this is wrong: we want to keep the preceding newline
        // and just replace the line content.  Detect and skip it.
        if start_offset < end_offset && ctx.text.as_bytes().get(start_offset) == Some(&b'\n') {
            start_offset += 1;
        }

        // Autoindent: preserve leading whitespace of the first line.
        // Instead of deleting the whole line and re-inserting whitespace,
        // skip the whitespace in the deletion so it stays in place.
        // This matches Vim's `cc`/`S`/`c_` behavior.
        let line_text = &ctx.text[start_offset..end_offset.min(ctx.text.len())];
        let first_line = line_text.split('\n').next().unwrap_or("");
        let ws_len = first_line.len() - first_line.trim_start().len();
        start_offset += ws_len;

        Range::from_raw(start_offset, end_offset)
    } else {
        ctx.range
    };

    // Extract deleted text and normalize for register storage.
    // Uses shared linewise normalization (handles EOF separator vs blank line content).
    let raw_text = extract_range_text(ctx.text, ctx.range);
    let register_text = if ctx.is_linewise() {
        normalize_linewise_register(raw_text.as_str(), ctx.text, ctx.range)
    } else {
        std::borrow::Cow::Owned(raw_text.to_string())
    };
    let new_cursor = Offset::new(effective_range.start().get());

    // Route to registers using shared logic
    let is_multiline = ctx.spans_multiple_lines() || ctx.is_linewise();
    let effects = super::registers::route_delete_registers(
        Effects::new().begin_undo(),
        &register_text,
        ctx.motion_type,
        ctx.register,
        is_multiline,
        ctx.force_numbered_register,
    );

    // Undo group left OPEN — exit_finalize() will close it so the delete
    // and typed text are a single undoable atom (matching Vim behavior).
    //
    // Use Replace(range, "") instead of Delete(range) so that named marks
    // at the start of the changed region survive. Neovim's op_change keeps
    // the first line (truncating it), so marks on that line are preserved.
    // Our Delete handler calls invalidate_named_in_range which would kill
    // marks at the start; Replace does not invalidate, matching Neovim.
    let effects = effects
        .replace(effective_range, "")
        .set_cursor(new_cursor)
        .begin_insert(InsertEntryType::ChangeOperator, 1, 0, new_cursor);

    CommandResult::new(effects, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{MotionType, Offset, Range};

    #[test]
    fn test_change_word() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Should use begin_insert (BeginInsert effect), not SetMode
        let has_begin_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginInsert { .. }));
        assert!(
            has_begin_insert,
            "Change should enter insert mode via BeginInsert"
        );

        // Should have replace effect (Replace with "" preserves marks at start)
        let has_replace = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Replace { .. }));
        assert!(has_replace, "Change should replace text with empty string");
    }

    #[test]
    fn test_change_line() {
        let ctx = OperatorContext::new(
            "line1\nline2\n",
            Range::from_raw(0, 6),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Should use begin_insert (not SetMode)
        let has_begin_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginInsert { .. }));
        assert!(has_begin_insert);

        // Cursor should be at start
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn test_change_empty_range() {
        let ctx = OperatorContext::new(
            "hello",
            Range::EMPTY,
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Empty range should begin_undo + begin_insert (undo group left open)
        assert_eq!(result.effects.len(), 2);
        let effects: Vec<_> = result.effects.iter().collect();
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(effects[1], Effect::BeginInsert { .. }));
    }

    #[test]
    fn test_change_has_open_undo_group() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Undo group starts with BeginUndoGroup but is intentionally left OPEN —
        // exit_finalize() closes it so delete + typed text form one undoable atom.
        let effects: Vec<_> = result.effects.iter().collect();
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(
            !matches!(effects.last().unwrap(), Effect::EndUndoGroup { .. }),
            "Undo group should be left open for insert mode"
        );
    }
}
