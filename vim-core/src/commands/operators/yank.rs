//! Yank operator (y).
//!
//! Copies text to a register without deleting it.
//!
//! # Behavior
//!
//! - `yw` - Yank word
//! - `yy` - Yank line
//! - `y$` - Yank to end of line
//!
//! # Register Handling
//!
//! - Always updates `"` (unnamed) and `0` (yank register)
//! - If register specified: only updates that register

use super::types::{extract_range_text, normalize_linewise_register, OperatorContext};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{MarkName, MotionType, Offset};

/// Execute yank operator.
///
/// No traits, just functions + enum dispatch.
pub fn execute(ctx: &OperatorContext<'_>) -> CommandResult {
    // For linewise yanks, even empty ranges should produce a newline
    // (e.g., yy on empty buffer produces "\n" in Vim)
    let is_linewise = ctx.motion_type == MotionType::LineWise;

    // Empty range for charwise: still set marks (Neovim behavior),
    // but don't yank text or show messages.
    if ctx.is_empty() && !is_linewise {
        let effects = Effects::new()
            .set_mark(MarkName::CHANGE_START, ctx.range.start(), None)
            .set_mark(MarkName::CHANGE_END, ctx.range.start(), None)
            .set_cursor(ctx.cursor);
        return CommandResult::new(effects, ctx.cursor);
    }

    // Extract yanked text and normalize for register storage.
    // Uses shared linewise normalization (handles EOF separator vs blank line content).
    let yanked_text = extract_range_text(ctx.text, ctx.range);
    let yanked_text = if is_linewise {
        compact_str::CompactString::from(normalize_linewise_register(
            yanked_text.as_str(),
            ctx.text,
            ctx.range,
        ))
    } else {
        yanked_text
    };

    // Cursor goes to oap->start = min(cursor, motion_target).
    // For charwise, range.start() already equals this.
    // For linewise, range.start() is line-expanded (col 0), but
    // Neovim preserves the actual byte column of the min position.
    let new_cursor = if is_linewise {
        Offset::new(ctx.cursor.get().min(ctx.motion_target.get()))
    } else {
        Offset::new(ctx.range.start().get())
    };

    // Route to registers using shared logic
    let mut effects = super::registers::route_yank_registers(
        Effects::new(),
        &yanked_text,
        ctx.motion_type,
        ctx.register,
    )
    // mark.[ = start of yanked range (line-expanded for linewise).
    // Neovim uses oap->start which is (line, col 0) for linewise.
    // For EOF yanks, extend_to_full_lines backs up to include preceding '\n';
    // Neovim's mark.[ points at the line content, not the preceding newline.
    .set_mark(
        MarkName::CHANGE_START,
        {
            let start = ctx.range.start().get();
            // Skip the preceding '\n' only when it was added by
            // extend_to_full_lines (EOF adjustment). When start == 0,
            // the '\n' IS content (e.g., empty first line in "\n").
            if ctx.is_linewise() && start > 0 && ctx.text.as_bytes().get(start) == Some(&b'\n') {
                Offset::new(start + 1)
            } else {
                ctx.range.start()
            }
        },
        None,
    )
    .set_mark(
        MarkName::CHANGE_END,
        // ] mark is inclusive (last byte of yanked text), not exclusive end.
        // Neovim sets b_op_end = oap->end which is the inclusive byte position.
        // For multi-byte chars, this is the LAST byte of the character, not the
        // first byte. Use `end - 1` (not prev_char_boundary which gives the
        // first byte of the previous character).
        {
            let end_offset = ctx.range.clamp_end(Offset::new(ctx.text.len())).end().get();
            Offset::new(end_offset.saturating_sub(1))
        },
        None,
    )
    .set_cursor(new_cursor);

    // Emit YankPost event (e.g., for HighlightedYank)
    effects = effects.event(crate::primitives::VimEvent::YankPost {
        start: ctx.range.start(),
        end: ctx.range.end(),
        register: ctx.register,
        motion_type: ctx.motion_type,
    });

    // Emit yank highlight effect (gated by HostCapability::YankHighlight at session layer)
    effects.push(crate::effects::Effect::SetHighlightRange {
        owner: compact_str::CompactString::from(crate::effects::HIGHLIGHT_OWNER_YANK),
        range: ctx.range,
        group: compact_str::CompactString::new_inline("yank"),
        shape: match ctx.motion_type {
            MotionType::LineWise => crate::primitives::SelectionShape::Line,
            _ => crate::primitives::SelectionShape::Char,
        },
    });

    // Show message if yanking multiple lines
    let line_count = ctx.line_count();
    if line_count >= 2 {
        effects = effects.show_message(format!("{line_count} lines yanked"));
    }

    CommandResult::new(effects, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{MotionType, Offset, Range, RegisterName};

    fn assert_register_effect(effects: &[&Effect], name: RegisterName) {
        assert!(effects.iter().any(
            |effect| matches!(effect, Effect::SetRegister { name: actual, .. } if *actual == name)
        ));
    }

    fn assert_mark_effect(effects: &[&Effect], name: MarkName, offset: usize) {
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetMark { name: actual, offset: actual_offset, .. }
                if *actual == name && actual_offset.get() == offset
        )));
    }

    #[test]
    fn test_yank_word() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        let effects: Vec<_> = result.effects.iter().collect();

        assert_register_effect(
            &effects,
            crate::primitives::RegisterName::new_unchecked('"'),
        );
        assert_register_effect(
            &effects,
            crate::primitives::RegisterName::new_unchecked('0'),
        );
        assert_mark_effect(&effects, MarkName::CHANGE_START, 0);
        assert_mark_effect(&effects, MarkName::CHANGE_END, 4);
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, Effect::SetCursor { offset } if offset.get() == 0)));
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn test_yank_multiple_lines_shows_message() {
        let ctx = OperatorContext::new(
            "line1\nline2\nline3\n",
            Range::from_raw(0, 12),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Should have message for 2+ lines
        let has_message = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { .. }));
        assert!(has_message, "Should show message for multi-line yank");
    }

    #[test]
    fn test_yank_with_register() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            Some(RegisterName::new_unchecked('a')),
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Should only update register 'a', not 0
        let has_reg_a = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == crate::primitives::RegisterName::new_unchecked('a')));
        let has_reg_0 = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == crate::primitives::RegisterName::new_unchecked('0')));

        assert!(has_reg_a);
        assert!(
            !has_reg_0,
            "Named register yank should not update register 0"
        );
    }

    #[test]
    fn test_yank_no_undo_group() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute(&ctx);

        // Yank should not have undo groups (no document modification)
        let has_undo = result.effects.iter().any(|e| {
            matches!(
                e,
                Effect::BeginUndoGroup { .. } | Effect::EndUndoGroup { .. }
            )
        });
        assert!(!has_undo, "Yank should not create undo groups");
    }
}
