//! Visual mode put (replace selection with register content).
//!
//! Moved from `executor.rs` inline visual put short-circuit to keep
//! action logic in the commands layer.

use super::types::ActionContext;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::Mode;
use crate::primitives::{MotionType, Offset, Range, RegisterName, VisualType};

/// Execute visual put (p/P in visual mode).
///
/// Replaces the current selection with register content.
/// Deleted text goes to unnamed register, insert text from specified register.
pub fn execute_visual_put(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(selection) = ctx.selection else {
        return CommandResult::empty(ctx.cursor);
    };

    let text = ctx.text;
    let put_text = ctx.register_content.map_or("", |r| r.text());

    // Compute selection range. For visual line mode, the selection is already
    // expanded to full lines by dispatch_action — use it directly.
    // For charwise, use inclusive range (next_char_boundary on end).
    let is_visual_line = ctx.visual_type.is_some_and(VisualType::is_line);
    let (sel_start, range_end) = if is_visual_line {
        (
            selection.start().get(),
            selection.end().get().min(text.len()),
        )
    } else {
        let sel_start = selection.start().get();
        let sel_end = selection.end().get();
        (
            sel_start,
            crate::commands::helpers::next_char_boundary(text, sel_end),
        )
    };
    let deleted_text = if sel_start < text.len() {
        text.get(sel_start..range_end).unwrap_or("")
    } else {
        ""
    };

    // Detect linewise selection: explicit linewise visual mode, or selection
    // covers full lines (ends with newline or covers to end of document at line start)
    let is_linewise = is_visual_line
        || deleted_text.ends_with('\n')
        || (range_end == text.len()
            && (sel_start == 0 || text.as_bytes().get(sel_start.wrapping_sub(1)) == Some(&b'\n')));

    // Build replacement (put content × count)
    let replacement = put_text.repeat(ctx.count_usize());

    // Cursor position depends on selection type
    let new_cursor = if is_linewise {
        // Linewise: cursor goes to start of first replaced line
        Offset::new(sel_start)
    } else {
        // Cursor on start of last character of replacement.
        let last_start =
            crate::commands::helpers::prev_char_boundary(&replacement, replacement.len());
        Offset::new(sel_start + last_start)
    };

    let motion_type = if is_linewise {
        MotionType::LineWise
    } else {
        MotionType::CharWise
    };

    let is_blackhole = ctx.register_name.is_some_and(RegisterName::is_blackhole);

    let mut effects = Effects::new().begin_undo();
    if is_blackhole {
        // Blackhole register: do the replacement but suppress register writes
        // for the deleted text.
        effects.extend(Effects::new().replace(Range::from_raw(sel_start, range_end), &replacement));
    } else if ctx.preserve_register {
        // Preserve-register paste: replace text and write to numbered registers
        // but skip writing deleted text to the unnamed register.
        effects.extend(build_preserve_replace_effects(
            Range::from_raw(sel_start, range_end),
            &replacement,
            deleted_text,
            motion_type,
        ));
    } else {
        effects.extend(build_replace_effects(
            Range::from_raw(sel_start, range_end),
            &replacement,
            deleted_text,
            motion_type,
        ));
    }
    // Explicitly set mark '>' to the end of the replacement text.
    // Neovim sets visual marks AFTER the text mutation, so the mark
    // reflects the post-replace position, not the pre-replace selection.
    let mark_end = if !replacement.is_empty() && !is_linewise {
        let last_char_start =
            crate::commands::helpers::prev_char_boundary(&replacement, replacement.len());
        Offset::new(sel_start + last_char_start)
    } else {
        Offset::new(sel_start)
    };
    let effects = effects
        .set_cursor(new_cursor)
        .clear_selection()
        .set_mark(crate::primitives::MarkName::VISUAL_END, mark_end, None)
        .set_mode(Mode::Normal)
        .end_undo();

    CommandResult::new(effects, new_cursor)
}

/// Build effects for replacing selection without writing to unnamed register.
///
/// Still writes to numbered registers (register 1 for linewise, small-delete
/// for short charwise) so that numbered-register rotation works correctly.
fn build_preserve_replace_effects(
    range: Range,
    replacement: &str,
    deleted_text: &str,
    motion_type: MotionType,
) -> Effects {
    let effects = Effects::new().replace(range, replacement);

    if motion_type == MotionType::LineWise {
        effects.set_register(RegisterName::NUMBERED_1, deleted_text, motion_type)
    } else if !deleted_text.contains('\n') {
        effects.set_register(
            RegisterName::SMALL_DELETE,
            deleted_text,
            MotionType::CharWise,
        )
    } else {
        effects
    }
}

/// Build effects for replacing selection and setting registers.
fn build_replace_effects(
    range: Range,
    replacement: &str,
    deleted_text: &str,
    motion_type: MotionType,
) -> Effects {
    let effects = Effects::new().replace(range, replacement).set_register(
        RegisterName::UNNAMED,
        deleted_text,
        motion_type,
    );

    if motion_type == MotionType::LineWise {
        // Linewise deletes go to numbered register 1
        effects.set_register(RegisterName::NUMBERED_1, deleted_text, motion_type)
    } else if !deleted_text.contains('\n') {
        // Small delete register for single-line charwise deletes
        effects.set_register(
            RegisterName::SMALL_DELETE,
            deleted_text,
            MotionType::CharWise,
        )
    } else {
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{Offset, RegisterContent, SelectionRange};
    use std::num::NonZeroU32;

    fn make_visual_put_ctx<'text>(
        text: &'text str,
        anchor: usize,
        head: usize,
        reg_text: &'text str,
    ) -> (ActionContext<'text>, RegisterContent) {
        let sel = SelectionRange::new(Offset::new(anchor), Offset::new(head));
        let content = RegisterContent::char_wise(reg_text);
        let ctx = ActionContext::from_text_and_cursor(text, Offset::new(anchor), NonZeroU32::MIN)
            .with_selection(sel);
        (ctx, content)
    }

    #[test]
    fn test_visual_put_no_selection_returns_empty() {
        let ctx = ActionContext::from_text_and_cursor("hello", Offset::new(0), NonZeroU32::MIN);
        let result = execute_visual_put(&ctx);
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn test_visual_put_charwise_replaces_selection() {
        let (mut ctx, content) = make_visual_put_ctx("hello world", 0, 4, "hi");
        ctx = ctx.with_register(&content);
        let result = execute_visual_put(&ctx);

        // Should have Replace effect
        let has_replace = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Replace { .. }));
        assert!(has_replace, "Should have Replace effect");

        // Should enter normal mode
        let has_normal = result.effects.iter().any(|e| {
            matches!(
                e,
                Effect::SetMode {
                    mode: Mode::Normal,
                    ..
                }
            )
        });
        assert!(has_normal, "Should enter normal mode");
    }

    #[test]
    fn test_visual_put_saves_deleted_text_to_register() {
        let (mut ctx, content) = make_visual_put_ctx("hello world", 0, 4, "hi");
        ctx = ctx.with_register(&content);
        let result = execute_visual_put(&ctx);

        // Should save deleted text to unnamed register
        let has_set_reg = result.effects.iter().any(
            |e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::UNNAMED),
        );
        assert!(has_set_reg, "Should save deleted text to unnamed register");
    }

    #[test]
    fn test_visual_put_empty_register_results_in_deletion() {
        let (ctx, _content) = make_visual_put_ctx("hello world", 0, 4, "");
        // No register content set — empty replacement
        let result = execute_visual_put(&ctx);
        // Cursor should stay at selection start
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn test_visual_put_preserve_register_skips_unnamed() {
        let (mut ctx, content) = make_visual_put_ctx("hello world", 0, 4, "hi");
        ctx = ctx.with_register(&content).with_preserve_register();
        let result = execute_visual_put(&ctx);

        // Should NOT save deleted text to unnamed register
        let has_unnamed = result.effects.iter().any(
            |e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::UNNAMED),
        );
        assert!(
            !has_unnamed,
            "preserve_register should skip unnamed register write"
        );

        // Should still have a Replace effect (text was replaced)
        let has_replace = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Replace { .. }));
        assert!(has_replace, "Should still replace text");
    }

    #[test]
    fn test_visual_put_preserve_register_still_writes_small_delete() {
        let (mut ctx, content) = make_visual_put_ctx("hello world", 0, 4, "hi");
        ctx = ctx.with_register(&content).with_preserve_register();
        let result = execute_visual_put(&ctx);

        // Small delete register should still be written for short charwise deletes
        let has_small_delete = result.effects.iter().any(
            |e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::SMALL_DELETE),
        );
        assert!(
            has_small_delete,
            "preserve_register should still write small-delete register"
        );
    }
}
