//! Visual mode handler (charwise, linewise, blockwise).
//!
//! Routes keystrokes through the capability framework with the VISUAL
//! profile. The grammar parser receives `Mode::Visual(ty)` via the
//! current state, enabling visual-specific grammar rules.

use super::capabilities::{self, VISUAL};
use super::{ModeAction, ModeContext, ModeHandler};
use crate::keymap::KeyEvent;

/// Visual mode handler (charwise, linewise, blockwise).
///
/// Delegates to [`capabilities::route_through_capabilities`] with the
/// [`VISUAL`] profile. The capability framework tries each capability
/// in priority order, then falls back to the grammar parser — which
/// receives `Mode::Visual(ty)` from the current state, enabling
/// visual-specific grammar rules (e.g., `gv`, visual operators).
///
/// While structurally similar to Normal mode, Visual mode is a distinct
/// handler because:
/// 1. The VISUAL profile includes the `Selection` capability.
/// 2. Future visual-only keybindings have a dedicated home.
/// 3. Each mode has its own handler — no aliasing.
#[derive(Debug, Default, Clone, Copy)]
pub struct VisualModeHandler;

impl ModeHandler for VisualModeHandler {
    fn handle_key(&self, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
        capabilities::route_through_capabilities(key, ctx, &VISUAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Parser;
    use crate::keymap::Keymap;
    use crate::primitives::{Mode, VisualType};
    use crate::state::VimState;

    #[test]
    fn visual_handler_is_zero_sized() {
        assert_eq!(std::mem::size_of::<VisualModeHandler>(), 0);
    }

    #[test]
    fn charwise_visual_returns_pipeline() {
        let handler = VisualModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Visual(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char('d'), &mut ctx);
        assert!(matches!(action, ModeAction::Pipeline(_)));
    }

    #[test]
    fn linewise_visual_returns_pipeline() {
        let handler = VisualModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Visual(VisualType::Line));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char('y'), &mut ctx);
        assert!(matches!(action, ModeAction::Pipeline(_)));
    }

    #[test]
    fn blockwise_visual_returns_pipeline() {
        let handler = VisualModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Visual(VisualType::Block));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char('c'), &mut ctx);
        assert!(matches!(action, ModeAction::Pipeline(_)));
    }
}
