//! Insert mode key handler.
//!
//! Handles key presses when in Insert mode.
//! Follows Neovim's `insert_execute` dispatch pattern.

use crate::grammar::command::{Command, InsertKind};
use crate::grammar::parser::Parser;
use crate::grammar::result::GrammarResult;
use crate::keymap::{Key, KeyEvent, Modifiers};
use crate::primitives::CompletionKind;
use std::num::NonZeroU32;

impl Parser {
    /// Handle a key press in Insert mode.
    ///
    /// Insert mode has its own key processing, separate from normal mode grammar.
    /// Most keys are inserted literally, with special handling for control keys.
    ///
    /// # Key Dispatch (matching Neovim edit.c)
    ///
    /// | Key | Action |
    /// |-----|--------|
    /// | ESC, Ctrl-[, Ctrl-C | Exit insert mode |
    /// | Printable chars | Insert character |
    /// | Backspace, Ctrl-H | Delete backward (not handled here, via effects) |
    /// | Enter | Insert newline (not handled here, via effects) |
    ///
    /// Note: Escape/Ctrl-C/Ctrl-[ are classified as `KeyClass::Escape` by
    /// the keymap and intercepted by the mode handler before reaching here.
    pub fn handle_insert(&mut self, key: KeyEvent) -> GrammarResult {
        // Try special keys first (Tab, Enter, Backspace, Delete, arrows, Home, End)
        if let Some(result) = handle_insert_special_key(&key) {
            return result;
        }

        // Try Ctrl-modified character keys
        if let Some(result) = handle_insert_ctrl(&key) {
            return result;
        }

        // Printable characters are inserted
        if let Some(c) = key.as_char() {
            if c >= ' ' || c == '\t' || c == '\n' || c == '\r' {
                return GrammarResult::Execute(Command::Insert(InsertKind::Char { char: c }));
            }
        }

        GrammarResult::Invalid
    }
}

/// Handle special (non-character) keys in insert mode.
///
/// Dispatches named keys with explicit modifier handling:
/// 1. Ctrl+Left/Right → word movement
/// 2. Shift+Left/Right/Up/Down → word/page movement
/// 3. Reject remaining Ctrl/Alt/Meta-modified named keys
/// 4. Shift+Tab → outdent (Ctrl+D equivalent)
/// 5. Shift+Home/End/PageUp/PageDown → pass through (no Vim semantics)
/// 6. Bare named keys (Tab, Enter, arrows, etc.)
///
/// Returns `None` if the key is not handled here.
const fn handle_insert_special_key(key: &KeyEvent) -> Option<GrammarResult> {
    use crate::grammar::types::Motion;

    // Ctrl+Left/Right: word movement (before the modifier guard)
    if key.modifiers.contains(Modifiers::CTRL) {
        match key.key {
            Key::Left => {
                return Some(GrammarResult::Execute(Command::Motion {
                    motion: Motion::WordBackward,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                }));
            }
            Key::Right => {
                return Some(GrammarResult::Execute(Command::Motion {
                    motion: Motion::WordForward,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                }));
            }
            // drift: Ctrl+non-arrow named keys are not insert-mode motions; fall through to char-based Ctrl handling
            _ => {}
        }
    }

    // Shift+arrows: word/page movement (before the modifier guard)
    if key.modifiers.contains(Modifiers::SHIFT) {
        match key.key {
            Key::Left => {
                return Some(GrammarResult::Execute(Command::Motion {
                    motion: Motion::WordBackward,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                }));
            }
            Key::Right => {
                return Some(GrammarResult::Execute(Command::Motion {
                    motion: Motion::WordForward,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                }));
            }
            Key::Up => {
                return Some(GrammarResult::Execute(Command::Motion {
                    motion: Motion::ScrollFullUp,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                }));
            }
            Key::Down => {
                return Some(GrammarResult::Execute(Command::Motion {
                    motion: Motion::ScrollFullDown,
                    count: NonZeroU32::MIN,
                    explicit_count: false,
                }));
            }
            // drift: Shift+non-arrow keys (Home/End/PageUp/PageDown) pass through further below; other Shift+named keys are unhandled
            _ => {}
        }
    }

    // Reject remaining Ctrl/Alt/Meta-modified named keys.
    // Ctrl+Home, Ctrl+Tab, Alt+Enter, etc. pass through to the host.
    if key
        .modifiers
        .intersects(Modifiers::CTRL.union(Modifiers::ALT).union(Modifiers::META))
    {
        return None;
    }

    // Shift+Tab: dedent (Ctrl+D equivalent in real Vim)
    if matches!(key.key, Key::Tab) && key.modifiers.contains(Modifiers::SHIFT) {
        return Some(GrammarResult::Execute(Command::Insert(InsertKind::Outdent)));
    }

    // Shift+Home/End/PageUp/PageDown: no Vim semantics, pass through.
    if key.modifiers.contains(Modifiers::SHIFT) {
        match key.key {
            Key::Home | Key::End | Key::PageUp | Key::PageDown => return None,
            // drift: other Shift+named keys (e.g. Shift+Insert) are not pass-through; fall to the bare-key dispatcher
            _ => {}
        }
    }

    // Bare named keys (no command modifiers at this point).
    match key.key {
        Key::Tab => Some(GrammarResult::Execute(Command::Insert(InsertKind::Char {
            char: '\t',
        }))),
        Key::Enter => Some(GrammarResult::Execute(Command::Insert(InsertKind::Char {
            char: '\n',
        }))),
        Key::Backspace => Some(GrammarResult::Execute(Command::Insert(
            InsertKind::Backspace,
        ))),
        Key::Delete => Some(GrammarResult::Execute(Command::Insert(
            InsertKind::DeleteUnder,
        ))),
        Key::Insert => Some(GrammarResult::Execute(Command::Insert(
            InsertKind::ToggleReplace,
        ))),
        Key::Up => Some(GrammarResult::Execute(Command::Motion {
            motion: Motion::Up,
            count: NonZeroU32::MIN,
            explicit_count: false,
        })),
        Key::Down => Some(GrammarResult::Execute(Command::Motion {
            motion: Motion::Down,
            count: NonZeroU32::MIN,
            explicit_count: false,
        })),
        Key::Left => Some(GrammarResult::Execute(Command::Motion {
            motion: Motion::Left,
            count: NonZeroU32::MIN,
            explicit_count: false,
        })),
        Key::Right => Some(GrammarResult::Execute(Command::Motion {
            motion: Motion::Right,
            count: NonZeroU32::MIN,
            explicit_count: false,
        })),
        Key::Home => Some(GrammarResult::Execute(Command::Motion {
            motion: Motion::LineStart,
            count: NonZeroU32::MIN,
            explicit_count: false,
        })),
        Key::End => Some(GrammarResult::Execute(Command::Motion {
            motion: Motion::LineEnd,
            count: NonZeroU32::MIN,
            explicit_count: false,
        })),
        Key::PageUp => Some(GrammarResult::Execute(Command::Motion {
            motion: Motion::ScrollFullUp,
            count: NonZeroU32::MIN,
            explicit_count: false,
        })),
        Key::PageDown => Some(GrammarResult::Execute(Command::Motion {
            motion: Motion::ScrollFullDown,
            count: NonZeroU32::MIN,
            explicit_count: false,
        })),
        // drift: F-keys and other named keys without insert-mode semantics yield None; caller passes them to the host
        _ => None,
    }
}

/// Handle Ctrl-modified character keys in insert mode.
///
/// Dispatches Ctrl-H (backspace), Ctrl-J/M (newline), Ctrl-I (tab),
/// Ctrl-T (indent), Ctrl-D (outdent), Ctrl-W (delete word),
/// Ctrl-U (delete to start), Ctrl-O (one-shot), Ctrl-A (last inserted),
/// Ctrl-V (paste), and Ctrl-R (register). Returns `None` if the key
/// is not a Ctrl-modified character or is an unhandled Ctrl combination.
const fn handle_insert_ctrl(key: &KeyEvent) -> Option<GrammarResult> {
    use crate::grammar::input_state::InputState;

    if !key.modifiers.contains(Modifiers::CTRL) {
        return None;
    }

    if let Key::Char(c) = key.key {
        return Some(match c {
            'h' => GrammarResult::Execute(Command::Insert(InsertKind::Backspace)),
            'j' | 'm' => GrammarResult::Execute(Command::Insert(InsertKind::Char { char: '\n' })),
            'i' => GrammarResult::Execute(Command::Insert(InsertKind::Char { char: '\t' })),
            't' => GrammarResult::Execute(Command::Insert(InsertKind::Indent)),
            'd' => GrammarResult::Execute(Command::Insert(InsertKind::Outdent)),
            'w' => GrammarResult::Execute(Command::Insert(InsertKind::DeleteWord)),
            'u' => GrammarResult::Execute(Command::Insert(InsertKind::DeleteToStart)),
            'o' => GrammarResult::Execute(Command::Insert(InsertKind::OneShot)),
            'a' => GrammarResult::Execute(Command::Insert(InsertKind::LastInserted)),
            '@' => GrammarResult::Execute(Command::Insert(InsertKind::LastInsertedAndExit)),
            'v' => GrammarResult::Continue(InputState::InsertLiteral(
                crate::grammar::input_state::InsertLiteralState::AwaitingFirst,
            )),
            'r' => GrammarResult::Continue(InputState::AwaitingInsertRegister),
            'e' => GrammarResult::Execute(Command::Insert(InsertKind::CopyCharBelow)),
            'y' => GrammarResult::Execute(Command::Insert(InsertKind::CopyCharAbove)),
            'g' => GrammarResult::Continue(InputState::AwaitingInsertCtrlG),
            'k' => GrammarResult::Continue(InputState::AwaitingInsertDigraph1),
            'x' => GrammarResult::Continue(InputState::AwaitingInsertCtrlX),
            // Ctrl-^ (Ctrl-6): toggle langmap in insert mode
            '^' | '6' => GrammarResult::Execute(Command::Insert(InsertKind::ToggleLangmap)),
            'n' => GrammarResult::Execute(Command::Insert(InsertKind::RequestCompletion {
                kind: CompletionKind::KeywordNext,
            })),
            'p' => GrammarResult::Execute(Command::Insert(InsertKind::RequestCompletion {
                kind: CompletionKind::KeywordPrev,
            })),
            _ => GrammarResult::Invalid,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Modifiers;

    #[test]
    fn test_insert_char() {
        let mut parser = Parser::new();

        // Regular character
        let result = parser.handle_insert(KeyEvent::char('a'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::Char { char: 'a' }))
        );

        // Space
        let result = parser.handle_insert(KeyEvent::char(' '));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::Char { char: ' ' }))
        );

        // Tab
        let result = parser.handle_insert(KeyEvent::char('\t'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::Char { char: '\t' }))
        );

        // Enter
        let result = parser.handle_insert(KeyEvent::char('\n'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::Char { char: '\n' }))
        );
    }

    #[test]
    fn test_insert_escape_handled_by_keymap() {
        // Ctrl-C and Ctrl-[ are classified as KeyClass::Escape by the keymap
        // and intercepted by the mode handler before reaching the grammar.
        // The grammar handler correctly does NOT handle them.
        let mut parser = Parser::new();

        // Ctrl-C is NOT handled by grammar (caught by mode handler)
        let result = parser.handle_insert(KeyEvent::ctrl('c'));
        assert_eq!(result, GrammarResult::Invalid);

        // Ctrl-[ is NOT handled by grammar (caught by mode handler)
        let result = parser.handle_insert(KeyEvent::ctrl('['));
        assert_eq!(result, GrammarResult::Invalid);
    }

    #[test]
    fn test_insert_backspace() {
        let mut parser = Parser::new();

        // Backspace key
        let result = parser.handle_insert(KeyEvent::new(Key::Backspace, Modifiers::NONE));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::Backspace))
        );

        // Ctrl-H is same as Backspace
        let result = parser.handle_insert(KeyEvent::ctrl('h'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::Backspace))
        );
    }

    #[test]
    fn test_insert_delete_under() {
        let mut parser = Parser::new();

        // Delete key deletes character under cursor
        let result = parser.handle_insert(KeyEvent::new(Key::Delete, Modifiers::NONE));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::DeleteUnder))
        );
    }

    #[test]
    fn test_insert_delete_word() {
        let mut parser = Parser::new();

        // Ctrl-W deletes word backward
        let result = parser.handle_insert(KeyEvent::ctrl('w'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::DeleteWord))
        );
    }

    #[test]
    fn test_insert_delete_to_start() {
        let mut parser = Parser::new();

        // Ctrl-U deletes to start of line
        let result = parser.handle_insert(KeyEvent::ctrl('u'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::DeleteToStart))
        );
    }

    #[test]
    fn test_insert_one_shot() {
        let mut parser = Parser::new();

        // Ctrl-O enters one-shot normal mode
        let result = parser.handle_insert(KeyEvent::ctrl('o'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::OneShot))
        );
    }

    #[test]
    fn test_insert_last_inserted() {
        let mut parser = Parser::new();

        // Ctrl-A inserts previously inserted text
        let result = parser.handle_insert(KeyEvent::ctrl('a'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::LastInserted))
        );
    }

    #[test]
    fn test_insert_last_inserted_and_exit() {
        let mut parser = Parser::new();

        // Ctrl-@ inserts previously inserted text and exits to Normal mode
        let result = parser.handle_insert(KeyEvent::ctrl('@'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::LastInsertedAndExit))
        );
    }

    #[test]
    fn test_insert_register() {
        use crate::grammar::InputState;
        let mut parser = Parser::new();

        // Ctrl-R enters register insert mode (awaits register name)
        let result = parser.handle_insert(KeyEvent::ctrl('r'));
        assert_eq!(
            result,
            GrammarResult::Continue(InputState::AwaitingInsertRegister)
        );
    }

    #[test]
    fn test_insert_tab_and_enter_keys() {
        let mut parser = Parser::new();

        // Tab key (not Ctrl-I, but actual Tab)
        let result = parser.handle_insert(KeyEvent::new(Key::Tab, Modifiers::NONE));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::Char { char: '\t' }))
        );

        // Enter key (not Ctrl-M, but actual Enter)
        let result = parser.handle_insert(KeyEvent::enter());
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::Char { char: '\n' }))
        );
    }

    // ── Ctrl-X completion sub-mode ──────────────────────────────────────

    #[test]
    fn test_insert_ctrl_x_starts_completion_submode() {
        let mut parser = Parser::new();

        // Ctrl-X in insert mode should enter the AwaitingInsertCtrlX state
        let result = parser.handle_insert(KeyEvent::ctrl('x'));
        assert_eq!(
            result,
            GrammarResult::Continue(crate::grammar::InputState::AwaitingInsertCtrlX)
        );
    }

    // ── Digraph input (Ctrl-K) tests ────────────────────────────────────

    #[test]
    fn test_insert_ctrl_k_starts_digraph() {
        let mut parser = Parser::new();

        // Ctrl-K in insert mode should enter the AwaitingInsertDigraph1 state
        let result = parser.handle_insert(KeyEvent::ctrl('k'));
        assert_eq!(
            result,
            GrammarResult::Continue(crate::grammar::InputState::AwaitingInsertDigraph1)
        );
    }

    #[test]
    fn test_insert_digraph_two_chars() {
        use crate::grammar::InputState;

        let mut parser = Parser::new();
        let keymap = crate::keymap::Keymap::default();

        // Step 1: Ctrl-K → AwaitingInsertDigraph1
        let r1 = parser.process(
            KeyEvent::ctrl('k'),
            &keymap,
            crate::primitives::Mode::Insert,
        );
        assert_eq!(
            r1,
            GrammarResult::Continue(InputState::AwaitingInsertDigraph1),
            "Ctrl-K should transition to AwaitingInsertDigraph1"
        );

        // Step 2: 'a' → AwaitingInsertDigraph2 { c1: 'a' }
        let r2 = parser.process(
            KeyEvent::char('a'),
            &keymap,
            crate::primitives::Mode::Insert,
        );
        assert_eq!(
            r2,
            GrammarResult::Continue(InputState::AwaitingInsertDigraph2 { c1: 'a' }),
            "First char 'a' should transition to AwaitingInsertDigraph2"
        );

        // Step 3: '\'' (apostrophe) → emits Digraph { c1: 'a', c2: '\'' }
        // Resolution is deferred to the execution layer (DigraphRegistry).
        let r3 = parser.process(
            KeyEvent::char('\''),
            &keymap,
            crate::primitives::Mode::Insert,
        );
        assert_eq!(
            r3,
            GrammarResult::Execute(Command::Insert(InsertKind::Digraph { c1: 'a', c2: '\'' })),
            "Digraph (a, ') should emit Digraph variant for deferred resolution"
        );
    }

    #[test]
    fn test_insert_digraph_unknown_pair() {
        use crate::grammar::InputState;

        let mut parser = Parser::new();
        let keymap = crate::keymap::Keymap::default();

        // Step 1: Ctrl-K → AwaitingInsertDigraph1
        let r1 = parser.process(
            KeyEvent::ctrl('k'),
            &keymap,
            crate::primitives::Mode::Insert,
        );
        assert_eq!(
            r1,
            GrammarResult::Continue(InputState::AwaitingInsertDigraph1)
        );

        // Step 2: 'z' → AwaitingInsertDigraph2 { c1: 'z' }
        let r2 = parser.process(
            KeyEvent::char('z'),
            &keymap,
            crate::primitives::Mode::Insert,
        );
        assert_eq!(
            r2,
            GrammarResult::Continue(InputState::AwaitingInsertDigraph2 { c1: 'z' })
        );

        // Step 3: 'z' → emits Digraph { c1: 'z', c2: 'z' }
        // Resolution (fallback to literal 'z') happens in the execution layer.
        let r3 = parser.process(
            KeyEvent::char('z'),
            &keymap,
            crate::primitives::Mode::Insert,
        );
        assert_eq!(
            r3,
            GrammarResult::Execute(Command::Insert(InsertKind::Digraph { c1: 'z', c2: 'z' })),
            "Unknown digraph (z, z) should emit Digraph variant for deferred resolution"
        );
    }

    #[test]
    fn test_insert_digraph_escape_cancels() {
        use crate::grammar::InputState;

        let mut parser = Parser::new();
        let keymap = crate::keymap::Keymap::default();

        // Step 1: Ctrl-K → AwaitingInsertDigraph1
        let r1 = parser.process(
            KeyEvent::ctrl('k'),
            &keymap,
            crate::primitives::Mode::Insert,
        );
        assert_eq!(
            r1,
            GrammarResult::Continue(InputState::AwaitingInsertDigraph1)
        );

        // Step 2: Escape → should cancel (Escape is intercepted at the top
        // of the insert block in parser.process() before reaching the digraph handler)
        let r2 = parser.process(KeyEvent::escape(), &keymap, crate::primitives::Mode::Insert);
        assert_eq!(
            r2,
            GrammarResult::Cancel,
            "Escape during digraph input should cancel"
        );
    }

    // ── Ctrl-^ (langmap toggle) tests ──────────────────────────────────

    #[test]
    fn test_insert_ctrl_caret_toggles_langmap() {
        let mut parser = Parser::new();

        // Ctrl-^ in insert mode should emit ToggleLangmap
        let result = parser.handle_insert(KeyEvent::ctrl('^'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::ToggleLangmap))
        );
    }

    #[test]
    fn test_insert_ctrl_6_toggles_langmap() {
        let mut parser = Parser::new();

        // Ctrl-6 is the same as Ctrl-^ (both toggle langmap)
        let result = parser.handle_insert(KeyEvent::ctrl('6'));
        assert_eq!(
            result,
            GrammarResult::Execute(Command::Insert(InsertKind::ToggleLangmap))
        );
    }
}
