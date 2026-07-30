//! Action dispatcher.
//!
//! Maps `Grammar::Action` to `commands::actions` implementations.
//! This is the ONLY place to update when adding actions.
//!
//! # Design
//!
//! Grammar layer has `Action` enum for parsing.
//! Commands layer has organized implementations (put.rs, mark.rs, etc).
//! This dispatcher bridges them via exhaustive match.
//!
//! # Bridge Invariant
//!
//! This file is a **bridge** — it maps `grammar::Action` to `commands::actions`
//! implementations. The only non-trivial logic is line-visual expansion
//! (expanding selection to full lines before dispatch), which uses
//! `state::VisualType` and `commands::visual::selection`.
//!
//! All effect-producing logic lives in `commands/actions/`.
//!
//! # Adding New Actions
//!
//! 1. Add variant to `grammar::Action` enum
//! 2. Create implementation in `commands/actions/`
//! 3. Add match arm HERE in `dispatch_action()`

pub use crate::commands::actions::ActionContext;
use crate::commands::actions::{
    case, delete_char, delete_to_end, intent_repeat, join, jump, number, put, substitute, undo,
    visual_block, visual_put,
};
use crate::commands::CommandResult;
use crate::grammar::types::Action;

/// Dispatch an Action to the appropriate implementation.
///
/// This is the **exhaustive match** for all actions.
/// Adding a new Action variant will cause a compile error here.
///
/// No dyn traits in the hot path: exhaustive match dispatch, which is
/// inlinable and allocation-free.
///
/// # Architecture
///
/// Maps grammar actions to commands implementations. Line-visual expansion
/// (the only pre-dispatch policy) uses `state::VisualType` to detect line
/// mode and delegates to `commands::visual::selection`.
///
/// # Arguments
/// * `action` - The action from Grammar
/// * `ctx` - Action context with cursor, register, count, etc.
///
/// # Returns
/// * `CommandResult` with effects to apply
#[inline]
pub fn dispatch_action(action: Action, ctx: &ActionContext<'_>) -> CommandResult {
    // Line-visual expansion: if visual_type == Line, expand selection to full lines.
    // This policy lives here (dispatch), not in the executor.
    let expanded_ctx;
    let ctx = if ctx.visual_type == Some(crate::primitives::VisualType::Line) {
        if let Some(ref sel) = ctx.selection {
            let expanded_sel =
                crate::commands::visual::selection::expand_selection_to_lines(ctx.text, sel);
            expanded_ctx = ActionContext {
                selection: Some(expanded_sel),
                ..ctx.clone()
            };
            &expanded_ctx
        } else {
            ctx
        }
    } else {
        ctx
    };

    match action {
        // Put: visual mode replaces selection, normal mode pastes
        Action::Put => {
            if ctx.selection.is_some() {
                visual_put::execute_visual_put(ctx)
            } else {
                put::execute_put(ctx)
            }
        }
        Action::PutBefore => {
            if ctx.selection.is_some() {
                visual_put::execute_visual_put(ctx)
            } else {
                put::execute_put_before(ctx)
            }
        }

        Action::DeleteChar => delete_char::execute_delete_char(ctx),
        Action::DeleteCharBack => delete_char::execute_delete_char_back(ctx),

        // D, C, Y compound commands
        Action::DeleteToEnd => delete_to_end::execute_delete_to_end(ctx),
        Action::ChangeToEnd => delete_to_end::execute_change_to_end(ctx),
        Action::YankLine => delete_to_end::execute_yank_line(ctx),

        // Undo/Redo
        Action::Undo => undo::execute_undo(ctx),
        Action::Redo => undo::execute_redo(ctx),
        Action::UndoLine => undo::execute_undo_line(ctx),

        // Join lines — handles both normal and visual mode via ctx.selection
        Action::Join => join::execute_join(ctx),

        // Case toggle
        Action::SwapCase => case::execute_swap_case(ctx),

        // Substitute
        Action::Substitute => substitute::execute_substitute(ctx),

        // Jump list navigation
        Action::JumpOlder => jump::execute_jump_older(ctx),
        Action::JumpNewer => jump::execute_jump_newer(ctx),

        // Block visual insert/append
        Action::BlockInsert => visual_block::execute_block_insert(ctx),
        Action::BlockAppend => visual_block::execute_block_append(ctx),

        // Number increment/decrement
        Action::IncrementNumber => number::execute_increment_number(ctx),
        Action::DecrementNumber => number::execute_decrement_number(ctx),

        // ]p/[p — put with indent adjustment
        Action::PutIndentAfter => put::execute_put_indent_after(ctx),
        Action::PutIndentBefore => put::execute_put_indent_before(ctx),

        // gp/gP — put with cursor after pasted text
        Action::PutAfterCursorAfter => {
            if ctx.selection.is_some() {
                visual_put::execute_visual_put(ctx)
            } else {
                put::execute_put_after_cursor_after(ctx)
            }
        }
        Action::PutBeforeCursorAfter => {
            if ctx.selection.is_some() {
                visual_put::execute_visual_put(ctx)
            } else {
                put::execute_put_before_cursor_after(ctx)
            }
        }

        // Informational
        Action::ShowFileInfo => {
            use crate::commands::actions::info;
            CommandResult::effects_only(info::show_file_info(ctx.text, ctx.cursor))
        }
        Action::KeywordLookup => {
            CommandResult::effects_only(crate::effects::Effects::new().show_documentation())
        }

        // IntentRepeat is handled at the executor level (needs RepeatState
        // access). This arm is unreachable but required for exhaustiveness.
        Action::IntentRepeat => {
            CommandResult::effects_only(intent_repeat::execute_intent_repeat(None))
        }

        // Handled at executor level (needs substitute state).
        Action::RepeatSubstitute | Action::RepeatSubstituteGlobal => {
            CommandResult::empty(ctx.cursor)
        }

        // Alternate file (Ctrl-^): requires host buffer management.
        // In a single-buffer context, always emits E23.
        Action::AlternateFile => {
            let effects = crate::effects::Effects::new().show_error(
                crate::errors::VimError::HostFailure("E23: No alternate file".into()),
            );
            CommandResult::effects_only(effects)
        }

        // Multi-cursor actions (gb/gB/gs): intercepted at executor level
        // via ExecutorOutput.multi_cursor_command. This arm is unreachable
        // but required for exhaustiveness.
        Action::AddNextMatchCursor | Action::AddPrevMatchCursor | Action::SkipMatchCursor => {
            CommandResult::empty(ctx.cursor)
        }
    }
}

/// Dispatch a `gJ` (join-no-space) command.
///
/// `gJ` is a prefix command, not an `Action` variant, so it needs a separate
/// dispatch entry point. Handles both normal and visual mode via ctx.selection.
#[inline]
pub fn dispatch_join_no_space(ctx: &ActionContext<'_>) -> CommandResult {
    join::execute_join_no_space(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;

    fn make_context(text: &str) -> ActionContext<'_> {
        ActionContext::new(
            text,
            Offset::new(5),        // cursor in middle of text
            Offset::new(0),        // operator start
            Offset::new(10),       // operator end
            Some(Offset::new(11)), // line end
            1,                     // count
        )
    }

    #[test]
    fn test_dispatch_join() {
        let ctx = make_context("hello\nworld");
        let result = dispatch_action(Action::Join, &ctx);
        assert!(!result.is_empty());
    }
}
