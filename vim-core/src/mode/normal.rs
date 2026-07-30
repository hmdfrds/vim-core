//! Normal mode handler.
//!
//! Routes keystrokes through the capability framework, which iterates
//! per-capability handlers in priority order before falling back to
//! the grammar parser.

use super::capabilities::{self, NORMAL};
use super::{ModeAction, ModeContext, ModeHandler};
use crate::keymap::KeyEvent;

/// Normal mode handler.
///
/// Delegates to [`capabilities::route_through_capabilities`] with the
/// [`NORMAL`] profile. The capability framework tries each capability
/// in priority order, then falls back to the grammar parser — producing
/// exactly the same `ModeAction::Pipeline(result)` as the original
/// direct-delegation implementation.
///
/// Visual and OperatorPending each have their own handlers —
/// Normal handles only `Mode::Normal`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NormalModeHandler;

impl ModeHandler for NormalModeHandler {
    fn handle_key(&self, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
        capabilities::route_through_capabilities(key, ctx, &NORMAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Parser;
    use crate::keymap::Keymap;
    use crate::primitives::Mode;
    use crate::state::VimState;

    #[test]
    fn normal_handler_is_zero_sized() {
        assert_eq!(std::mem::size_of::<NormalModeHandler>(), 0);
    }

    #[test]
    fn normal_handler_returns_pipeline_action() {
        let handler = NormalModeHandler;
        let mut state = VimState::default();
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char('j'), &mut ctx);
        assert!(
            matches!(action, ModeAction::Pipeline(_)),
            "Normal handler should always return Pipeline variant"
        );
    }

    #[test]
    fn normal_mode_is_normal() {
        let handler = NormalModeHandler;
        let mut state = VimState::default();
        assert_eq!(state.mode(), Mode::Normal);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        // Should use Mode::Normal, not Visual or OperatorPending
        let action = handler.handle_key(KeyEvent::char('w'), &mut ctx);
        assert!(matches!(action, ModeAction::Pipeline(_)));
    }
}
