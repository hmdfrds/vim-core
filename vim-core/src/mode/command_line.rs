//! Command-line mode handler.
//!
//! Routes command-line keystrokes through the command-line
//! key handler and converts results to `ModeAction`.

use super::{ModeAction, ModeContext, ModeHandler};
use crate::grammar::CommandLineEdit;
use crate::keymap::{Key, KeyEvent, Modifiers};

/// Command-line mode handler (`:`, `/`, `?` prompts).
///
/// Classifies keystrokes (escape, enter, editing keys) via the local
/// `handle_key` function and converts results to `ModeAction` for the engine.
#[derive(Debug, Default, Clone, Copy)]
pub struct CommandLineModeHandler;

impl ModeHandler for CommandLineModeHandler {
    fn handle_key(&self, key: KeyEvent, ctx: &mut ModeContext<'_>) -> ModeAction {
        let (parser, _keymap) = ctx.parser_and_keymap();

        // Ctrl-\ Ctrl-N universal escape: use parser state to track intermediate.
        if matches!(
            parser.state(),
            crate::grammar::InputState::AwaitingCtrlBackslashN
        ) {
            parser.reset();
            if key == KeyEvent::ctrl('n') {
                return ModeAction::Pipeline(crate::grammar::GrammarResult::ModeChange(
                    crate::primitives::Mode::Normal,
                    None,
                ));
            }
            // Not Ctrl-N: cancel and fall through to normal command-line handling
        }
        if key == KeyEvent::ctrl('\\') {
            parser.set_state(crate::grammar::InputState::AwaitingCtrlBackslashN);
            return ModeAction::Pending;
        }

        let action = handle_key(key);
        action.into()
    }
}

/// Command-line key handling outcome `(:`, `/`, `?`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandLineAction {
    /// Edit the command line text.
    Edit(CommandLineEdit),
    /// Commit current input.
    Commit,
    /// Cancel command-line.
    Cancel,
    /// Enter register-awaiting sub-state (Ctrl-R).
    AwaitRegister,
    /// Open command-line window with current input as prefill (`Ctrl-F` / `cedit`).
    OpenCommandWindow,
    /// Ignore key / continue.
    Ignore,
}

/// Process one key in command-line mode.
pub(crate) fn handle_key(key: KeyEvent) -> CommandLineAction {
    // Ctrl-[ is equivalent to Escape in Vim
    if key.key() == Key::Escape || key == KeyEvent::ctrl('c') || key == KeyEvent::ctrl('[') {
        return CommandLineAction::Cancel;
    }
    if key.key() == Key::Enter {
        return CommandLineAction::Commit;
    }

    match key.key() {
        Key::Backspace => CommandLineAction::Edit(CommandLineEdit::Backspace),
        Key::Delete => CommandLineAction::Edit(CommandLineEdit::Delete),
        Key::Left
            if key.modifiers().contains(Modifiers::CTRL)
                || key.modifiers().contains(Modifiers::SHIFT) =>
        {
            CommandLineAction::Edit(CommandLineEdit::MoveWordLeft)
        }
        Key::Right
            if key.modifiers().contains(Modifiers::CTRL)
                || key.modifiers().contains(Modifiers::SHIFT) =>
        {
            CommandLineAction::Edit(CommandLineEdit::MoveWordRight)
        }
        // Left/Right with Alt/Meta (but not Ctrl/Shift, handled above) → ignore.
        Key::Left | Key::Right if key.modifiers().intersects(Modifiers::ALT | Modifiers::META) => {
            CommandLineAction::Ignore
        }
        Key::Left => CommandLineAction::Edit(CommandLineEdit::MoveLeft),
        Key::Right => CommandLineAction::Edit(CommandLineEdit::MoveRight),
        // Home/End/Up/Down: reject Ctrl/Alt/Meta-modified variants (pass through to host).
        // Bare and Shift variants are handled.
        Key::Home | Key::End | Key::Up | Key::Down
            if key
                .modifiers()
                .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::META) =>
        {
            CommandLineAction::Ignore
        }
        Key::Home => CommandLineAction::Edit(CommandLineEdit::MoveToStart),
        Key::End => CommandLineAction::Edit(CommandLineEdit::MoveToEnd),
        Key::Up => CommandLineAction::Edit(CommandLineEdit::HistoryPrev),
        Key::Down => CommandLineAction::Edit(CommandLineEdit::HistoryNext),
        Key::Char(ch) if key.modifiers().contains(Modifiers::CTRL) => {
            apply_ctrl_action(ch).unwrap_or(CommandLineAction::Ignore)
        }
        Key::Tab if key.modifiers().contains(Modifiers::SHIFT) => {
            CommandLineAction::Edit(CommandLineEdit::CompletePrev)
        }
        Key::Tab => CommandLineAction::Edit(CommandLineEdit::CompleteNext),
        Key::Char(ch)
            if !key
                .modifiers()
                .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::META) =>
        {
            CommandLineAction::Edit(CommandLineEdit::InsertChar(ch))
        }
        // drift: unrecognised key types (e.g. F-keys, Insert) in command-line mode are silently ignored
        _ => CommandLineAction::Ignore,
    }
}

/// Map Ctrl+key to a command-line action.
///
/// Returns `Some(action)` for recognized Ctrl sequences, `None` for unknown.
const fn apply_ctrl_action(ch: char) -> Option<CommandLineAction> {
    match ch {
        'w' => Some(CommandLineAction::Edit(CommandLineEdit::DeleteWord)),
        'u' => Some(CommandLineAction::Edit(CommandLineEdit::DeleteToStart)),
        'k' => Some(CommandLineAction::Edit(CommandLineEdit::DeleteToEnd)),
        'h' => Some(CommandLineAction::Edit(CommandLineEdit::Backspace)),
        'r' => Some(CommandLineAction::AwaitRegister),
        'f' => Some(CommandLineAction::OpenCommandWindow),
        'd' => Some(CommandLineAction::Edit(CommandLineEdit::ListCompletions)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Parser;
    use crate::keymap::Keymap;
    use crate::state::{CommandLinePrompt, VimState};

    #[test]
    fn command_line_handler_is_zero_sized() {
        assert_eq!(std::mem::size_of::<CommandLineModeHandler>(), 0);
    }

    #[test]
    fn typing_char_returns_pending() {
        let handler = CommandLineModeHandler;
        let mut state = VimState::default();
        state.command_line_mut().begin(CommandLinePrompt::Ex);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::char('w'), &mut ctx);
        assert!(
            matches!(
                action,
                ModeAction::CommandLine(super::super::CommandLineResult::Edit(_))
            ),
            "Typing in command line should return Edit"
        );
    }

    #[test]
    fn escape_returns_cancel() {
        let handler = CommandLineModeHandler;
        let mut state = VimState::default();
        state.command_line_mut().begin(CommandLinePrompt::Ex);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::escape(), &mut ctx);
        assert!(
            matches!(
                action,
                ModeAction::CommandLine(super::super::CommandLineResult::Cancel)
            ),
            "Escape should return CommandLine(Cancel)"
        );
    }

    #[test]
    fn ctrl_f_returns_open_command_window() {
        let handler = CommandLineModeHandler;
        let mut state = VimState::default();
        state.command_line_mut().begin(CommandLinePrompt::Ex);
        let keymap = Keymap::default();
        let mut parser = Parser::new();
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

        let action = handler.handle_key(KeyEvent::ctrl('f'), &mut ctx);
        assert!(
            matches!(
                action,
                ModeAction::CommandLine(super::super::CommandLineResult::OpenCommandWindow)
            ),
            "Ctrl-F should return CommandLine(OpenCommandWindow)"
        );
    }

    #[test]
    fn ctrl_f_raw_action() {
        let action = handle_key(KeyEvent::ctrl('f'));
        assert_eq!(action, CommandLineAction::OpenCommandWindow);
    }

    #[test]
    fn ctrl_shift_w_triggers_delete_word() {
        let key = KeyEvent::new(Key::Char('w'), Modifiers::CTRL | Modifiers::SHIFT);
        let action = handle_key(key);
        assert!(
            matches!(action, CommandLineAction::Edit(CommandLineEdit::DeleteWord)),
            "Ctrl+Shift+W should trigger DeleteWord, got {:?}",
            action
        );
    }

    #[test]
    fn ctrl_left_moves_word_left() {
        let key = KeyEvent::new(Key::Left, Modifiers::CTRL);
        let action = handle_key(key);
        assert_eq!(
            action,
            CommandLineAction::Edit(CommandLineEdit::MoveWordLeft)
        );
    }

    #[test]
    fn ctrl_right_moves_word_right() {
        let key = KeyEvent::new(Key::Right, Modifiers::CTRL);
        let action = handle_key(key);
        assert_eq!(
            action,
            CommandLineAction::Edit(CommandLineEdit::MoveWordRight)
        );
    }

    #[test]
    fn shift_left_moves_word_left() {
        let key = KeyEvent::new(Key::Left, Modifiers::SHIFT);
        let action = handle_key(key);
        assert_eq!(
            action,
            CommandLineAction::Edit(CommandLineEdit::MoveWordLeft)
        );
    }

    #[test]
    fn shift_right_moves_word_right() {
        let key = KeyEvent::new(Key::Right, Modifiers::SHIFT);
        let action = handle_key(key);
        assert_eq!(
            action,
            CommandLineAction::Edit(CommandLineEdit::MoveWordRight)
        );
    }

    #[test]
    fn ctrl_home_ignored() {
        let key = KeyEvent::new(Key::Home, Modifiers::CTRL);
        let action = handle_key(key);
        assert_eq!(action, CommandLineAction::Ignore);
    }

    #[test]
    fn ctrl_end_ignored() {
        let key = KeyEvent::new(Key::End, Modifiers::CTRL);
        let action = handle_key(key);
        assert_eq!(action, CommandLineAction::Ignore);
    }

    #[test]
    fn ctrl_up_ignored() {
        let key = KeyEvent::new(Key::Up, Modifiers::CTRL);
        let action = handle_key(key);
        assert_eq!(action, CommandLineAction::Ignore);
    }

    #[test]
    fn ctrl_down_ignored() {
        let key = KeyEvent::new(Key::Down, Modifiers::CTRL);
        let action = handle_key(key);
        assert_eq!(action, CommandLineAction::Ignore);
    }

    #[test]
    fn alt_left_ignored() {
        let key = KeyEvent::new(Key::Left, Modifiers::ALT);
        let action = handle_key(key);
        assert_eq!(action, CommandLineAction::Ignore);
    }

    #[test]
    fn alt_right_ignored() {
        let key = KeyEvent::new(Key::Right, Modifiers::ALT);
        let action = handle_key(key);
        assert_eq!(action, CommandLineAction::Ignore);
    }

    #[test]
    fn alt_up_ignored() {
        let key = KeyEvent::new(Key::Up, Modifiers::ALT);
        let action = handle_key(key);
        assert_eq!(action, CommandLineAction::Ignore);
    }

    #[test]
    fn bare_home_moves_to_start() {
        let key = KeyEvent::new(Key::Home, Modifiers::NONE);
        let action = handle_key(key);
        assert_eq!(
            action,
            CommandLineAction::Edit(CommandLineEdit::MoveToStart)
        );
    }

    #[test]
    fn bare_up_history_prev() {
        let key = KeyEvent::new(Key::Up, Modifiers::NONE);
        let action = handle_key(key);
        assert_eq!(
            action,
            CommandLineAction::Edit(CommandLineEdit::HistoryPrev)
        );
    }

    #[test]
    fn shift_home_moves_to_start() {
        let key = KeyEvent::new(Key::Home, Modifiers::SHIFT);
        let action = handle_key(key);
        assert_eq!(
            action,
            CommandLineAction::Edit(CommandLineEdit::MoveToStart)
        );
    }

    #[test]
    fn shift_char_inserts() {
        let key = KeyEvent::new(Key::Char('A'), Modifiers::SHIFT);
        let action = handle_key(key);
        assert!(
            matches!(
                action,
                CommandLineAction::Edit(CommandLineEdit::InsertChar('A'))
            ),
            "Shift+char should insert the character, got {:?}",
            action
        );
    }

    // ── Ctrl-D completion listing ────────────────────────────────

    #[test]
    fn task_8_5_ctrl_d_returns_list_completions() {
        let action = handle_key(KeyEvent::ctrl('d'));
        assert_eq!(
            action,
            CommandLineAction::Edit(CommandLineEdit::ListCompletions),
            "Ctrl-D should produce ListCompletions edit"
        );
    }
}
