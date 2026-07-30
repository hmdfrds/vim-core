//! Operator-pending mode handler.
//!
//! Active after typing an operator key (d, c, y, etc.) while waiting
//! for a motion or text object. Routes through the capability framework
//! with the OPERATOR_PENDING profile, which enables text-object grammar
//! rules (e.g., `iw` is a text object in OP, not insert in Normal).

use super::capabilities::{self, OPERATOR_PENDING};
use super::{ModeAction, ModeContext, ModeHandler};
use crate::keymap::KeyEvent;

/// Operator-pending mode handler.
///
/// Delegates to [`capabilities::route_through_capabilities`] with the
/// [`OPERATOR_PENDING`] profile. The capability framework tries each
/// capability in priority order, then falls back to the grammar parser —
/// which receives `Mode::OperatorPending(op)` from the current state,
/// enabling text-object grammar rules (`iw`, `a"`, etc.).
///
/// While structurally similar to Normal mode, OperatorPending is distinct
/// because:
/// 1. The OPERATOR_PENDING profile includes `TextObjects` but excludes
///    `Actions`, `Register`, and `Window`.
/// 2. Escape in OP cancels the operator — different from Normal's Escape.
/// 3. Each mode has its own handler — no aliasing.
#[derive(Debug, Default, Clone, Copy)]
pub struct OperatorPendingModeHandler;

impl ModeHandler for OperatorPendingModeHandler {
    fn handle_key(&self, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
        capabilities::route_through_capabilities(key, ctx, &OPERATOR_PENDING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Operator;
    use crate::grammar::Parser;
    use crate::keymap::Keymap;
    use crate::primitives::Mode;
    use crate::state::VimState;

    #[test]
    fn operator_pending_handler_is_zero_sized() {
        assert_eq!(std::mem::size_of::<OperatorPendingModeHandler>(), 0);
    }

    #[test]
    fn operator_pending_returns_pipeline() {
        let handler = OperatorPendingModeHandler;
        let mut state = VimState::default();
        // Simulate operator-pending after 'd'
        state.set_mode(Mode::OperatorPending(Operator::Delete));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        // Press 'w' for motion
        let action = handler.handle_key(KeyEvent::char('w'), &mut ctx);
        assert!(matches!(action, ModeAction::Pipeline(_)));
    }

    #[test]
    fn escape_in_operator_pending_returns_pipeline() {
        let handler = OperatorPendingModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::OperatorPending(Operator::Yank));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        // Escape cancels the pending operator (via grammar → Cancel)
        let action = handler.handle_key(KeyEvent::escape(), &mut ctx);
        assert!(matches!(action, ModeAction::Pipeline(_)));
    }
}
