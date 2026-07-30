//! Find motion dispatcher.
//!
//! Maps `Grammar::CharCommand` (find variants) to `commands::motions::find` implementations.
//! This is the ONLY place to update when adding find motions.
//!
//! # Design
//!
//! Grammar layer has `CharCommand` enum for parsing (f, F, t, T, r).
//! Commands layer has find implementations in motions/find.rs.
//! This dispatcher bridges them via exhaustive match.
//!
//! Note: Replace (r) is NOT a motion - it's an action. We return None for it.

use std::num::NonZeroU32;

use crate::commands::motions::{find, MotionContext, MotionResult};
use crate::grammar::types::CharCommand;

/// Dispatch a `CharCommand` find motion to the appropriate implementation.
///
/// Returns `Some(MotionResult)` for find motions (f/F/t/T).
/// Returns `None` for non-motion `CharCommands` (Replace).
///
/// # Arguments
/// * `cmd` - The `CharCommand` from Grammar
/// * `ctx` - Motion context with text, cursor, count, and `target_char`
///
/// # Returns
/// * `Some(MotionResult)` for find motions
/// * `None` if the command isn't a motion (e.g., Replace)
#[inline]
#[must_use]
pub fn dispatch_find(cmd: CharCommand, ctx: &MotionContext<'_>) -> Option<MotionResult> {
    match cmd {
        CharCommand::FindForward => Some(find::f(ctx)),
        CharCommand::FindBackward => Some(find::F(ctx)),
        CharCommand::TillForward => Some(find::t(ctx)),
        CharCommand::TillBackward => Some(find::T(ctx)),
        CharCommand::Replace => None, // Replace is an action, not a motion
    }
}

/// Input context for [`dispatch_char_command`].
///
/// Bundles the parameters needed to dispatch a full `CharCommand` —
/// find motions (f/F/t/T) or replace (r).
#[derive(Debug)]
pub struct CharCommandInput<'a> {
    /// The char-command variant (f/F/t/T/r).
    pub cmd: CharCommand,
    /// The target character.
    pub ch: char,
    /// Repeat count.
    pub count: NonZeroU32,
    /// Document text.
    pub text: &'a str,
    /// Cursor byte offset.
    pub cursor: usize,
    /// Active visual selection, if any.
    pub selection: Option<&'a crate::primitives::SelectionRange>,
    /// Current Vim mode.
    pub mode: crate::primitives::Mode,
    /// Engine options (search flags, word boundaries, etc.).
    pub options: &'a crate::primitives::VimOptions,
}

/// Dispatch a full `CharCommand` — find motions (f/F/t/T) or replace (r).
///
/// This is the high-level entry point that the execution layer should call.
/// Encapsulates:
/// - Replace commands → `execute_replace_char` directly
/// - Find motions → `dispatch_find` + `find_with_tracking` (records last find
///   for `;`/`,` repeat, even on failure, matching Neovim behavior)
#[inline]
pub fn dispatch_char_command(input: &CharCommandInput<'_>) -> crate::commands::CommandResult {
    use crate::commands::actions::replace_char::execute_replace_char;
    use crate::commands::actions::ReplaceCharContext;
    use crate::commands::motions::find::{char_command_to_direction, find_with_tracking_with_case};
    use crate::primitives::Offset;

    let CharCommandInput {
        cmd,
        ch,
        count,
        text,
        cursor,
        selection,
        mode,
        options,
    } = *input;

    // Replace command: route to replace dispatch
    if cmd == CharCommand::Replace {
        // Derive visual type from mode (policy lives here, not executor)
        let visual_type = match mode {
            crate::primitives::Mode::Visual(vt) => Some(vt),
            _ => None,
        };
        let rctx = ReplaceCharContext {
            text,
            ch,
            cursor: Offset::new(cursor),
            count: count.get() as usize,
            selection,
            visual_type,
        };
        return execute_replace_char(&rctx);
    }

    // All other CharCommands are find motions
    let direction = match char_command_to_direction(cmd) {
        Some(d) => d,
        None => {
            return crate::commands::CommandResult::new(
                crate::effects::Effects::new(),
                crate::primitives::Offset::new(cursor),
            )
        }
    };

    let motion_ctx = MotionContext::new(
        text,
        crate::primitives::Offset::new(cursor),
        count.get(),
        options,
    )
    .with_target_char(ch);
    let motion_result = dispatch_find(cmd, &motion_ctx);

    // Resolve case flags now so `;`/`,` repeats use the same case sensitivity.
    let ic = options.ignorecase();
    let sc = options.smartcase();

    // In visual mode, emit SetSelection (extend_selection) instead of just
    // SetCursor so the shell updates BufferState.visual and the VimCursor
    // overlay follows the selection head.
    if let Some(sel) = selection {
        if let Some(MotionResult::Position(new_offset)) = motion_result {
            let shape = mode.visual_type().map_or(
                crate::primitives::SelectionShape::Char,
                crate::primitives::SelectionShape::from,
            );
            let mut effects =
                crate::effects::Effects::new().set_last_find_with_case(direction, ch, ic, sc);
            effects.extend(crate::commands::visual::extend_selection(
                sel, new_offset, shape,
            ));
            // f/F/t/T are horizontal motions — update sticky column (curswant)
            // so that subsequent operators using coladvance get the correct column.
            // Without this, the sticky column stays stale from the previous motion.
            if new_offset.get() != cursor {
                let tabstop = options.tabstop();
                let column = Some(crate::primitives::VirtualColumn::new(
                    crate::commands::helpers::curswant_of(text, new_offset.get(), tabstop),
                ));
                effects.push(crate::effects::Effect::SetStickyColumn { column });
            }
            return crate::commands::CommandResult::effects_only(effects);
        }
    }

    let mut result = find_with_tracking_with_case(direction, ch, motion_result.as_ref(), ic, sc);
    // f/F/t/T are horizontal motions — update sticky column (curswant).
    // The CharCommand path bypasses the motion dispatcher's sticky_column
    // logic, so we must emit SetStickyColumn here.
    if let Some(MotionResult::Position(new_offset)) = motion_result {
        if new_offset.get() != cursor {
            let tabstop = options.tabstop();
            let column = Some(crate::primitives::VirtualColumn::new(
                crate::commands::helpers::curswant_of(text, new_offset.get(), tabstop),
            ));
            result
                .effects
                .push(crate::effects::Effect::SetStickyColumn { column });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::motions::MotionContext;
    use crate::grammar::types::CharCommand;
    use crate::primitives::Offset;

    #[test]
    fn test_dispatch_find_forward() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("hello world", Offset::new(0), 1, &opts).with_target_char('o');
        let result = dispatch_find(CharCommand::FindForward, &ctx);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), MotionResult::Position(o) if o.get() == 4));
    }

    #[test]
    fn test_dispatch_find_backward() {
        let opts = crate::primitives::VimOptions::default();
        let ctx =
            MotionContext::new("hello world", Offset::new(10), 1, &opts).with_target_char('o');
        let result = dispatch_find(CharCommand::FindBackward, &ctx);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), MotionResult::Position(o) if o.get() == 7));
    }

    #[test]
    fn test_dispatch_replace_returns_none() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("hello", Offset::new(0), 1, &opts).with_target_char('x');
        let result = dispatch_find(CharCommand::Replace, &ctx);
        assert!(result.is_none());
    }
}
