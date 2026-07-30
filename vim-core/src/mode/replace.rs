//! Replace mode handler.
//!
//! Replace mode uses the same insert grammar but routes commands
//! through a different execution path (overwrite vs insert).
//! Having a dedicated handler ensures Replace-specific logic has
//! a clear home and the `insert_mode` field is guaranteed correct.

use super::{InsertMode, ModeAction, ModeContext, ModeHandler};
use crate::grammar::{Command, GrammarResult};
use crate::keymap::{KeyClass, KeyEvent};
use crate::primitives::Mode;

/// Replace mode handler.
///
/// Structurally similar to Insert, but:
/// 1. Always sets `insert_mode: InsertMode::Replace` on `ModeAction::InsertCommand`
/// 2. Future Replace-only keybindings (e.g., `gR` for virtual replace)
///    have a dedicated home.
/// 3. Each mode has its own handler — no aliasing.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReplaceModeHandler;

impl ModeHandler for ReplaceModeHandler {
    fn handle_key(&self, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
        let mode = ctx.state().mode();
        let insert_mode = if mode == Mode::VirtualReplace {
            InsertMode::VirtualReplace
        } else {
            InsertMode::Replace
        };
        let (parser, keymap) = ctx.parser_and_keymap();

        // Fast path: Escape exits replace mode — but NOT when awaiting
        // a literal character (Ctrl-V sequence).
        if keymap.classify(key, mode) == KeyClass::Escape
            && !matches!(parser.state(), crate::grammar::InputState::InsertLiteral(_))
        {
            return ModeAction::InsertExit;
        }

        let result = parser.process(key, keymap, Mode::Insert);

        match result {
            GrammarResult::Execute(Command::InsertExit) => ModeAction::InsertExit,
            GrammarResult::Execute(command) => ModeAction::InsertCommand {
                command,
                insert_mode,
            },
            GrammarResult::Continue(_) => ModeAction::Pending,
            // Ctrl-\ Ctrl-N universal escape produces ModeChange from any mode.
            GrammarResult::ModeChange(..) => ModeAction::Pipeline(result),
            _ => ModeAction::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::{InsertKind, Parser};
    use crate::keymap::Keymap;
    use crate::state::VimState;

    #[test]
    fn replace_handler_is_zero_sized() {
        assert_eq!(std::mem::size_of::<ReplaceModeHandler>(), 0);
    }

    #[test]
    fn char_always_sets_is_replace() {
        let handler = ReplaceModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Replace);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char('x'), &mut ctx);
        match action {
            ModeAction::InsertCommand {
                insert_mode: InsertMode::Replace,
                ..
            } => {}
            other => panic!(
                "Expected InsertCommand with insert_mode=Replace, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn escape_returns_insert_exit() {
        let handler = ReplaceModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Replace);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::escape(), &mut ctx);
        assert!(matches!(action, ModeAction::InsertExit));
    }

    #[test]
    fn backspace_sets_is_replace() {
        let handler = ReplaceModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Replace);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::backspace(), &mut ctx);
        match action {
            ModeAction::InsertCommand {
                command: Command::Insert(InsertKind::Backspace),
                insert_mode: InsertMode::Replace,
            } => {}
            other => panic!(
                "Expected InsertBackspace with insert_mode=Replace, got {:?}",
                other
            ),
        }
    }
}
