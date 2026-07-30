//! Undo/Redo actions.
//!
//! Moved from `dispatch/action.rs` to keep dispatch as a pure bridge.

use super::types::ActionContext;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::Mode;

/// Exit visual/select mode before undo/redo.
///
/// Real Vim always exits visual mode on undo/redo. Without this,
/// the shell retains a stale `BufferState.visual` that would be
/// re-injected into the engine on the next keystroke.
#[inline]
fn exit_visual_if_needed(effects: Effects, ctx: &ActionContext<'_>) -> Effects {
    if ctx.visual_type.is_some() {
        effects.clear_selection().set_mode(Mode::Normal)
    } else {
        effects
    }
}

/// Execute undo (u command).
#[inline]
pub fn execute_undo(ctx: &ActionContext<'_>) -> CommandResult {
    let effects = exit_visual_if_needed(Effects::new(), ctx)
        .push_jump_list(ctx.cursor)
        .undo(ctx.count);
    CommandResult::new(effects, ctx.cursor)
}

/// Execute line-local undo (U command).
#[inline]
pub fn execute_undo_line(ctx: &ActionContext<'_>) -> CommandResult {
    let mut effects = exit_visual_if_needed(Effects::new(), ctx).undo_line(ctx.count);
    // Clear sticky column so it gets recomputed from the post-undo cursor
    // position, matching Neovim's w_set_curswant=TRUE after undo-line.
    effects.push(crate::effects::Effect::SetStickyColumn { column: None });
    CommandResult::new(effects, ctx.cursor)
}

/// Execute redo (Ctrl-R command).
#[inline]
pub fn execute_redo(ctx: &ActionContext<'_>) -> CommandResult {
    let effects = exit_visual_if_needed(Effects::new(), ctx)
        .push_jump_list(ctx.cursor)
        .redo(ctx.count);
    CommandResult::new(effects, ctx.cursor)
}
