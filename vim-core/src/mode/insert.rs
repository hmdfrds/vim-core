//! Insert mode handler.
//!
//! Routes insert-mode keystrokes: escape detection and grammar parsing.
//! Replace mode has its own dedicated handler (`ReplaceModeHandler`).
//! The actual command execution stays in `execution/insert_handler.rs`.

use super::{InsertMode, ModeAction, ModeContext, ModeHandler};
use crate::grammar::{Command, GrammarResult};
use crate::keymap::{KeyClass, KeyEvent};
use crate::primitives::Mode;

/// Insert mode handler.
///
/// Handles the mode-specific routing decision:
/// 1. Escape detection (fast-path exit)
/// 2. Grammar parsing for insert commands
///
/// Always returns `insert_mode: InsertMode::Insert` — Replace mode has its own handler.
///
/// The handler does NOT execute commands — it returns `ModeAction`
/// instructions that the engine acts on.
#[derive(Debug, Default, Clone, Copy)]
pub struct InsertModeHandler;

impl ModeHandler for InsertModeHandler {
    fn handle_key(&self, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
        let (parser, keymap) = ctx.parser_and_keymap();

        // Fast path: Escape bypasses grammar parser entirely — but NOT when
        // the parser is awaiting a literal character (Ctrl-V sequence).
        if keymap.classify(key, Mode::Insert) == KeyClass::Escape
            && !matches!(parser.state(), crate::grammar::InputState::InsertLiteral(_))
        {
            return ModeAction::InsertExit;
        }

        let result = parser.process(key, keymap, Mode::Insert);

        match result {
            GrammarResult::Execute(Command::InsertExit) => ModeAction::InsertExit,
            GrammarResult::Execute(command) => ModeAction::InsertCommand {
                command,
                insert_mode: InsertMode::Insert, // Always Insert — this is the Insert handler
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
    fn insert_handler_is_zero_sized() {
        assert_eq!(std::mem::size_of::<InsertModeHandler>(), 0);
    }

    #[test]
    fn escape_returns_insert_exit() {
        let handler = InsertModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Insert);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::escape(), &mut ctx);
        assert!(
            matches!(action, ModeAction::InsertExit),
            "Escape should return InsertExit"
        );
    }

    #[test]
    fn normal_char_returns_insert_command_not_replace() {
        let handler = InsertModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Insert);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char('a'), &mut ctx);
        match action {
            ModeAction::InsertCommand {
                command: Command::Insert(InsertKind::Char { char: 'a' }),
                insert_mode: InsertMode::Insert,
            } => {}
            other => panic!(
                "Expected InsertCommand with 'a' and insert_mode=Insert, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn backspace_returns_insert_command() {
        let handler = InsertModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Insert);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::backspace(), &mut ctx);
        match action {
            ModeAction::InsertCommand {
                command: Command::Insert(InsertKind::Backspace),
                insert_mode: InsertMode::Insert,
            } => {}
            other => panic!(
                "Expected InsertBackspace with insert_mode=Insert, got {:?}",
                other
            ),
        }
    }
}
