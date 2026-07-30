//! Select mode handler.
//!
//! Select mode behaves like GUI-style selection: typing a printable
//! character deletes the selection and enters Insert mode. Non-printable
//! keys temporarily switch to Visual mode for one command, then return
//! to Select mode via the `ReturnTo` mechanism.

use super::{ModeAction, ModeContext, ModeHandler};
use crate::grammar::command::{Command, VisualKind};
use crate::grammar::result::GrammarResult;
use crate::keymap::{Key, KeyClass, KeyEvent};
use crate::primitives::{Mode, ReturnTo, VisualType};

/// Select mode handler (charwise, linewise, blockwise).
///
/// Select mode is Vim's GUI-style selection mode. It intercepts keystrokes
/// and classifies them into one of four categories:
///
/// 1. **Printable characters** — delete selection, enter Insert mode, insert char
///    (`ModeAction::SelectReplace { char }`)
/// 2. **Backspace / Delete** — delete selection, return to Normal mode
///    (`ModeAction::SelectDelete`)
/// 3. **Escape / Ctrl-C** — exit selection (reuses Visual exit pipeline)
/// 4. **Ctrl-G** — toggle back to Visual mode
/// 5. **Everything else** — delegate to Visual grammar for one command,
///    then return to Select mode via `ReturnTo::Select(vt)`
#[derive(Debug, Default, Clone, Copy)]
pub struct SelectModeHandler;

impl ModeHandler for SelectModeHandler {
    fn handle_key(&self, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
        let mode = ctx.state().mode();
        let vt = mode.select_type().unwrap_or_else(|| {
            debug_assert!(
                false,
                "SelectModeHandler invoked for non-Select mode: {mode:?}"
            );
            VisualType::Char
        });

        // 1. Escape / Ctrl-C — exit Select mode (reuse Visual exit path).
        //    Must classify before taking any mutable borrows.
        // Select mode bypasses user mappings — uses core classification directly
        let class = ctx.keymap().classify_core(key, mode);
        if class == KeyClass::Escape {
            return ModeAction::Pipeline(GrammarResult::Execute(Command::Visual(VisualKind::Exit)));
        }

        // 2. Ctrl-G — toggle to Visual mode.
        if key == KeyEvent::ctrl('g') {
            return ModeAction::Pipeline(GrammarResult::Execute(Command::Visual(
                VisualKind::ToggleSelect,
            )));
        }

        // 3. Backspace / Delete — delete selection.
        if matches!(key.key(), Key::Backspace | Key::Delete) {
            return ModeAction::SelectDelete;
        }

        // 4. Ctrl-\ Ctrl-N universal escape — intercept before Select-specific handling.
        {
            let (parser, _) = ctx.parser_and_keymap();
            if matches!(
                parser.state(),
                crate::grammar::InputState::AwaitingCtrlBackslashN
            ) {
                parser.reset();
                if key == KeyEvent::ctrl('n') {
                    return ModeAction::Pipeline(GrammarResult::ModeChange(Mode::Normal, None));
                }
                // Not Ctrl-N: cancel, fall through to normal Select handling
            }
            if key == KeyEvent::ctrl('\\') {
                parser.set_state(crate::grammar::InputState::AwaitingCtrlBackslashN);
                return ModeAction::Pending;
            }
        }

        // 5. Printable characters — replace selection with typed char.
        if let Some(c) = key.as_char() {
            if c >= ' ' {
                return ModeAction::SelectReplace { char: c };
            }
        }

        // 6. Everything else — delegate to Visual grammar for one command.
        //    Set return_to so we come back to Select after the command completes.
        ctx.state_mut().set_return_to(ReturnTo::Select(vt));
        ctx.state_mut().set_mode(Mode::Visual(vt));
        let (parser, keymap) = ctx.parser_and_keymap();
        let result = parser.process(key, keymap, Mode::Visual(vt));
        ModeAction::Pipeline(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Parser;
    use crate::keymap::Keymap;
    use crate::state::VimState;

    #[test]
    fn select_handler_is_zero_sized() {
        assert_eq!(std::mem::size_of::<SelectModeHandler>(), 0);
    }

    #[test]
    fn printable_char_returns_select_replace() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char('a'), &mut ctx);
        match action {
            ModeAction::SelectReplace { char: 'a' } => {}
            other => panic!("Expected SelectReplace {{ char: 'a' }}, got {:?}", other),
        }
    }

    #[test]
    fn escape_returns_visual_exit_pipeline() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::escape(), &mut ctx);
        match action {
            ModeAction::Pipeline(GrammarResult::Execute(Command::Visual(VisualKind::Exit))) => {}
            other => panic!("Expected Pipeline(Execute(Visual(Exit))), got {:?}", other),
        }
    }

    #[test]
    fn ctrl_c_returns_visual_exit_pipeline() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::ctrl('c'), &mut ctx);
        match action {
            ModeAction::Pipeline(GrammarResult::Execute(Command::Visual(VisualKind::Exit))) => {}
            other => panic!(
                "Expected Pipeline(Execute(Visual(Exit))) for Ctrl-C, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn backspace_returns_select_delete() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::backspace(), &mut ctx);
        assert!(
            matches!(action, ModeAction::SelectDelete),
            "Expected SelectDelete for Backspace, got {:?}",
            action
        );
    }

    #[test]
    fn delete_key_returns_select_delete() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(
            KeyEvent::new(Key::Delete, crate::keymap::Modifiers::NONE),
            &mut ctx,
        );
        assert!(
            matches!(action, ModeAction::SelectDelete),
            "Expected SelectDelete for Delete key, got {:?}",
            action
        );
    }

    #[test]
    fn ctrl_g_returns_toggle_select() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::ctrl('g'), &mut ctx);
        match action {
            ModeAction::Pipeline(GrammarResult::Execute(Command::Visual(
                VisualKind::ToggleSelect,
            ))) => {}
            other => panic!(
                "Expected Pipeline(Execute(Visual(ToggleSelect))), got {:?}",
                other
            ),
        }
    }

    #[test]
    fn non_printable_delegates_to_visual() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        // Arrow key is non-printable, non-special — should delegate to Visual grammar
        let down_key = KeyEvent::from(Key::Down);
        let action = handler.handle_key(down_key, &mut ctx);

        // Verify the action is Pipeline (delegated to grammar)
        assert!(
            matches!(action, ModeAction::Pipeline(_)),
            "Expected Pipeline for Down arrow delegation, got {:?}",
            action
        );

        // Verify return_to was set to Select(Char)
        assert_eq!(
            ctx.state().return_to(),
            ReturnTo::Select(VisualType::Char),
            "return_to should be Select(Char) after delegation"
        );

        // Verify mode was switched to Visual(Char)
        assert_eq!(
            ctx.state().mode(),
            Mode::Visual(VisualType::Char),
            "mode should be Visual(Char) after delegation"
        );
    }

    #[test]
    fn linewise_select_delegates_with_correct_visual_type() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Line));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let down_key = KeyEvent::from(Key::Down);
        let action = handler.handle_key(down_key, &mut ctx);

        assert!(matches!(action, ModeAction::Pipeline(_)));
        assert_eq!(ctx.state().return_to(), ReturnTo::Select(VisualType::Line));
        assert_eq!(ctx.state().mode(), Mode::Visual(VisualType::Line));
    }

    #[test]
    fn blockwise_select_delegates_with_correct_visual_type() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Block));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let down_key = KeyEvent::from(Key::Down);
        let action = handler.handle_key(down_key, &mut ctx);

        assert!(matches!(action, ModeAction::Pipeline(_)));
        assert_eq!(ctx.state().return_to(), ReturnTo::Select(VisualType::Block));
        assert_eq!(ctx.state().mode(), Mode::Visual(VisualType::Block));
    }

    #[test]
    fn space_is_printable_returns_select_replace() {
        let handler = SelectModeHandler;
        let mut state = VimState::default();
        state.set_mode(Mode::Select(VisualType::Char));
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char(' '), &mut ctx);
        match action {
            ModeAction::SelectReplace { char: ' ' } => {}
            other => panic!("Expected SelectReplace {{ char: ' ' }}, got {:?}", other),
        }
    }
}
