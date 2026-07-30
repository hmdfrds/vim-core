//! Delete operator (d).
//!
//! Deletes text in the specified range and stores it in a register.
//!
//! # Behavior
//!
//! - `dw` - Delete word
//! - `dd` - Delete line
//! - `d$` - Delete to end of line
//! - `d3j` - Delete 4 lines
//!
//! # Register Handling
//!
//! - If ≥1 line deleted: updates `"` and shifts `1-9`
//! - If <1 line deleted: updates `"` and `-`
//! - If register specified: only updates that register

use super::types::{
    cursor_after_delete, extract_range_text, normalize_linewise_register, OperatorContext,
};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::MarkName;

/// Execute delete operator.
///
/// No traits, just functions + enum dispatch.
pub fn execute(ctx: &OperatorContext<'_>) -> CommandResult {
    // Empty range = no deletion, no register change — except for linewise
    // operations on a non-empty buffer (e.g. Vd, dd on an empty LINE in
    // a buffer that has content). Vim stores "\n" for empty-line linewise
    // deletes, but NOT when the entire buffer is empty (no content at all).
    if ctx.is_empty() {
        if ctx.is_linewise()
            && (!ctx.text.is_empty() || ctx.origin == super::types::OperatorOrigin::Visual)
        {
            // Linewise delete of empty line: no text change, but register gets "\n"
            // Also applies for visual line mode on an empty buffer (Vd).
            let is_multiline = true;
            let effects = super::registers::route_delete_registers(
                Effects::new(),
                "\n",
                ctx.motion_type,
                ctx.register,
                is_multiline,
                ctx.force_numbered_register,
            );
            return CommandResult::new(effects, ctx.cursor);
        }
        return CommandResult::empty(ctx.cursor);
    }

    // Extract deleted text
    let deleted_text = extract_range_text(ctx.text, ctx.range);

    // For linewise operations, normalize register content via shared helper.
    // Handles EOF edge case (leading \n separator vs blank line content)
    // and guarantees trailing \n.
    let register_text = if ctx.is_linewise() {
        normalize_linewise_register(deleted_text.as_str(), ctx.text, ctx.range)
    } else {
        std::borrow::Cow::Owned(deleted_text.to_string())
    };

    let new_cursor = {
        let mut base = cursor_after_delete(ctx.text, ctx.range, ctx.motion_type, ctx.cursor);
        // Neovim's nosol + linewise delete: coladvance(curwin->w_curswant).
        // When a sticky column (curswant) is provided and differs from the
        // column derived from ctx.cursor, recompute the target column using
        // the sticky column on the surviving line. This matters for visual
        // line delete where ctx.cursor is at line-start but curswant carries
        // the desired column from before the V command.
        if ctx.motion_type.is_line_wise() {
            if let Some(vcol) = ctx.sticky_column {
                if !vcol.is_end_of_line() {
                    let target_col = vcol.get();
                    // Find line start of surviving line in post-delete text
                    let raw_start = ctx.range.start().get();
                    let raw_end = ctx
                        .range
                        .clamp_end(crate::primitives::Offset::new(ctx.text.len()))
                        .end()
                        .get();
                    let (start, end) =
                        super::types::adjust_to_char_boundaries(ctx.text, raw_start, raw_end);
                    let after_delete = end.min(ctx.text.len());
                    let surviving = &ctx.text[after_delete..];
                    let surviving_line_len = surviving.find('\n').unwrap_or(surviving.len());
                    if surviving_line_len > 0 {
                        let clamped = target_col.min(surviving_line_len.saturating_sub(1));
                        base = crate::primitives::Offset::new(start + clamped);
                    }
                }
            }
        }
        // For bracket text-object linewise deletes (e.g. `di(`, `di{`),
        // Neovim places cursor at column 0 of the surviving line. The standard
        // cursor_after_delete preserves the old column, which is wrong here
        // because the deletion removes interior lines and the cursor should
        // land at the start of the line that remains (the closing bracket line).
        //
        // Only apply this override when the range start is NOT at the beginning
        // of the document and the range doesn't extend to the end — these
        // conditions distinguish bracket interiors from paragraph/sentence text
        // objects that delete at document boundaries.
        if ctx.origin == super::types::OperatorOrigin::TextObject
            && ctx.is_linewise()
            && ctx.range.start().get() > 0
            && ctx.range.end().get() < ctx.text.len()
        {
            crate::primitives::Offset::new(ctx.range.start().get())
        } else {
            base
        }
    };

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

    // Neovim's op_delete (ops.c:1018-1027) sets both b_op_start and
    // b_op_end to oap->start — the START of the operated range.
    // In our byte-offset model, range.start() is already a post-deletion
    // offset (it's below the deleted region so byte values don't shift).
    //
    // Special case: EOF linewise deletes where extend_to_full_lines backs
    // up to include the preceding '\n'. Neovim's mark points at line
    // content, not the preceding newline.
    let mark_bracket = {
        let start = ctx.range.start();
        if ctx.is_linewise()
            && start.get() > 0
            && ctx.text.as_bytes().get(start.get()) == Some(&b'\n')
            && ctx.range.end().get() == ctx.text.len()
        {
            crate::primitives::Offset::new(start.get() + 1)
        } else {
            start
        }
    };

    // mark.. (LAST_CHANGE): Neovim's deleted_lines() calls
    // changed_lines(buf, lnum, 0, ...) where lnum is the first deleted line.
    // For EOF linewise deletes, extend_to_full_lines backs up to include the
    // preceding '\n'. Neovim's mark.. points at the line content start, not
    // the preceding newline. Detect and skip.
    let mark_dot = if ctx.is_linewise() {
        let start = ctx.range.start().get();
        // Only skip the leading '\n' when it is the EOF-backup separator
        // inserted by extend_to_full_lines (range covers to end of file and
        // the '\n' at `start` is the preceding line's terminator, not an
        // empty line's content). When start == 0, the '\n' IS content
        // (e.g. dd on a buffer that starts with an empty line), not a backup.
        if start > 0
            && ctx.text.as_bytes().get(start) == Some(&b'\n')
            && ctx.range.end().get() == ctx.text.len()
        {
            crate::primitives::Offset::new(start + 1)
        } else {
            ctx.range.start()
        }
    } else {
        // Charwise delete: Neovim's op_delete for multiline charwise deletes
        // does truncate_line + del_lines + del_bytes + do_join. The last
        // changed_bytes() call inside do_join fires on the line BELOW the
        // joined result, so mark '.' lands at the start of the next line
        // in the post-delete text.
        //
        // Check if the delete spans multiple lines by looking for '\n' in
        // the deleted region [start, end) of the pre-edit text.
        let start = ctx.range.start().get();
        let end = ctx.range.end().get();
        let deleted_crosses_line = ctx.text.get(start..end).is_some_and(|s| s.contains('\n'));
        if deleted_crosses_line {
            // Find the first '\n' at or after `end` in the pre-edit text.
            // In the post-delete text, this '\n' will be at offset
            // `(its pre-edit position) - (end - start)`, and mark '.'
            // should be at the byte after it (start of next line).
            if let Some(nl_pos) = ctx.text[end..].find('\n').map(|i| end + i) {
                let post_delete_nl = nl_pos - (end - start);
                crate::primitives::Offset::new(post_delete_nl + 1)
            } else {
                ctx.range.start()
            }
        } else {
            ctx.range.start()
        }
    };
    let effects = effects
        .delete(ctx.range)
        .set_mark(MarkName::CHANGE_START, mark_bracket, None)
        .set_mark(MarkName::CHANGE_END, mark_bracket, None)
        .set_mark(MarkName::LAST_CHANGE, mark_dot, None)
        .set_cursor(new_cursor)
        .end_undo();

    CommandResult::new(effects, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{MarkName, MotionType, Offset, Range, RegisterName};

    fn assert_mark_effect(effects: &[&Effect], name: MarkName, offset: usize) {
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetMark { name: actual, offset: actual_offset, .. }
                    if *actual == name && actual_offset.get() == offset
            )),
            "Expected SetMark({:?}, {}) not found in effects",
            name,
            offset
        );
    }

    #[test]
    fn test_delete_word() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Should have: BeginUndo, SetRegister("), SetRegister(-), Delete, SetCursor, EndUndo
        assert!(result.effects.len() >= 5);

        let effects: Vec<_> = result.effects.iter().collect();
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(
            effects.last().unwrap(),
            Effect::EndUndoGroup { .. }
        ));
    }

    #[test]
    fn test_delete_line() {
        let ctx = OperatorContext::new(
            "line1\nline2\n",
            Range::from_raw(0, 6),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Linewise should update register 1, not register -
        let has_reg_1 = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == crate::primitives::RegisterName::new_unchecked('1')));
        assert!(has_reg_1, "Linewise delete should update register 1");
    }

    #[test]
    fn test_delete_with_register() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            Some(RegisterName::new_unchecked('a')),
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Should only update register 'a', not numbered or small delete
        let has_reg_a = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == crate::primitives::RegisterName::new_unchecked('a')));
        let has_reg_1 = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == crate::primitives::RegisterName::new_unchecked('1')));
        let has_reg_dash = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == crate::primitives::RegisterName::new_unchecked('-')));

        assert!(has_reg_a);
        assert!(!has_reg_1);
        assert!(!has_reg_dash);
    }

    #[test]
    fn test_delete_sets_change_marks_to_range_start() {
        // Neovim: mark.[ = mark.] = oap->start = min(curpos, start_pos).
        // For text-object deletes (e.g. `daw` in the middle of a word),
        // cursor may be after range.start(); marks should be at range.start().
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(3),
        );

        let result = execute(&ctx);
        let effects: Vec<_> = result.effects.iter().collect();

        assert_mark_effect(&effects, MarkName::CHANGE_START, 0);
        assert_mark_effect(&effects, MarkName::CHANGE_END, 0);
    }

    #[test]
    fn test_linewise_delete_sets_marks_to_range_start() {
        // Forward linewise delete (e.g. dj from line2): cursor inside range,
        // motion_target defaults to cursor → falls through to range.start().
        let ctx = OperatorContext::new(
            "line1\nline2\nline3\n",
            Range::from_raw(6, 18),
            MotionType::LineWise,
            None,
            1,
            Offset::new(10),
        );

        let result = execute(&ctx);
        let effects: Vec<_> = result.effects.iter().collect();

        assert_mark_effect(&effects, MarkName::CHANGE_START, 6);
        assert_mark_effect(&effects, MarkName::CHANGE_END, 6);
    }

    #[test]
    fn test_backward_delete_adjusts_marks() {
        // dgg: cursor at byte 10, deleting range 0..6 (lines before cursor)
        // Neovim: b_op_start = b_op_end = oap->start = range.start() = 0
        let ctx = OperatorContext::new(
            "line1\nline2\nline3\n",
            Range::from_raw(0, 6), // delete "line1\n"
            MotionType::LineWise,
            None,
            1,
            Offset::new(10), // cursor was on line2, col 4
        );

        let result = execute(&ctx);
        let effects: Vec<_> = result.effects.iter().collect();

        // Neovim: b_op_start = oap->start = (0, 0) → byte 0
        assert_mark_effect(&effects, MarkName::CHANGE_START, 0);
        assert_mark_effect(&effects, MarkName::CHANGE_END, 0);
    }

    #[test]
    fn test_delete_empty_range() {
        let ctx = OperatorContext::new(
            "hello",
            Range::EMPTY,
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);
        assert!(result.is_empty(), "Empty range should produce no effects");
    }
}
