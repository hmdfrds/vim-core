//! Pre-flight key classification for the passthrough system.
//!
//! Provides `VimEngine::would_handle_key()` — a read-only query that answers
//! whether the engine would do something meaningful with a key in the current
//! mode and parser state. Used by the shell's passthrough system to decide
//! whether to route a key to the engine or let the host editor handle it.
//!
//! # Why this exists
//!
//! The shell must decide *before* calling `engine.process()` whether a key
//! should pass through to the host. In Normal mode the engine consumes all
//! keys (even unrecognized ones produce an error beep), so the shell cannot
//! rely on the `consumed` flag in the response. This method gives a truthful
//! pre-flight answer derived from the engine's actual keymap tables and
//! grammar handler logic — no hardcoded shadow copies.

use super::VimEngine;
use crate::keymap::{Key, KeyClass, KeyEvent, MappingMode, Modifiers};
use crate::primitives::Mode;

impl VimEngine {
    /// Whether the engine would do something meaningful with this key.
    ///
    /// Returns `true` if the key would produce a command, motion, mode switch,
    /// or other intentional action. Returns `false` if the key would be
    /// ignored (Insert/Replace) or produce only an error beep (Normal/Visual).
    ///
    /// This is a **read-only** query — it does not mutate any engine state.
    ///
    /// # Usage
    ///
    /// Called by the shell's passthrough system *before* `process()`. The
    /// shell composes this with mapping checks and user override lists to
    /// decide the final routing.
    ///
    /// # Scope
    ///
    /// This method answers about the engine's **built-in** command set only.
    /// User mappings are handled separately by `could_start_mapping()`.
    ///
    /// # Complexity
    ///
    /// Time: O(1) — handler map lookup is O(1) amortized (hash map), parser
    /// state check is O(1), and keymap classification is O(1) (table lookup).
    ///
    /// Space: O(1)
    #[must_use]
    pub fn would_handle_key(&self, key: KeyEvent) -> bool {
        let mode = self.state.mode();

        // 1. sethandler delegation: if the key is delegated to the host
        //    via `:sethandler`, the engine explicitly does not handle it.
        if let Some(mm) = MappingMode::from_mode(mode) {
            if self.handler_map.is_host_handled(key, mm) {
                return false;
            }
        }

        // Layout normalization: apply the same normalization as process()
        // so the passthrough system routes normalized keys correctly.
        let key = self.apply_langmap_and_normalize(key);

        // 2. Non-ready parser state: the engine is mid-sequence (operator
        //    pending, awaiting char/mark/register/prefix/etc.). Any key
        //    could complete or continue the sequence.
        if !self.parser.state().is_ready() {
            return true;
        }

        // 3. Mode-specific classification (parser is in Ready state).
        match mode {
            // CommandLine mode owns all input — every key is either a
            // character typed into the command line or a control key
            // (Enter, Escape, Tab, arrows) that the command-line handler
            // processes.
            Mode::CommandLine => would_handle_command_line_key(key),

            // Normal, Visual, OperatorPending: the core keymap table
            // is the authoritative source. `classify_core` returns
            // `KeyClass::Unknown` for keys the engine doesn't handle
            // (e.g. Ctrl+S, Ctrl+Z). It correctly classifies Escape,
            // Ctrl-C, and Ctrl-[ as `KeyClass::Escape`.
            // Select mode handles ALL keys: printable chars replace the
            // selection, Escape exits, Ctrl-G toggles to Visual, and
            // everything else delegates to Visual grammar with return_to.
            Mode::Select(_) => true,

            Mode::Normal | Mode::Visual(_) | Mode::OperatorPending(_) => {
                self.keymap.classify_core(key, mode) != KeyClass::Unknown
            }

            // Insert, Replace, VirtualReplace: the core keymap has no
            // table for these modes (classify_core returns Unknown for
            // everything). We replicate the grammar's `handle_insert`
            // logic as a read-only classification.
            Mode::Insert | Mode::Replace | Mode::VirtualReplace => {
                would_handle_insert_key(key, self.native_insert, |k| {
                    self.keymap.classify_core(k, mode)
                })
            }
        }
    }
}

/// Read-only classification of whether a key would be handled in Insert/Replace mode.
///
/// Mirrors the logic in `grammar/handlers/insert.rs` (`handle_insert`,
/// `handle_insert_special_key`, `handle_insert_ctrl`) without any mutation.
///
/// The `classify_escape` closure checks whether the key is an Escape-class key
/// (Escape, Ctrl-C, Ctrl-[) using the keymap's `classify_core`. This avoids
/// duplicating the escape-detection logic.
fn would_handle_insert_key(
    key: KeyEvent,
    native_insert: bool,
    classify_escape: impl Fn(KeyEvent) -> KeyClass,
) -> bool {
    // Escape-class keys (Escape, Ctrl-C, Ctrl-[) exit insert mode.
    // These are intercepted by the mode handler before reaching the grammar,
    // but they ARE handled by the engine.
    if classify_escape(key) == KeyClass::Escape {
        return true;
    }

    // Named special keys handled by handle_insert_special_key.
    // Only claim when no command modifiers (Ctrl/Alt/Meta) are active —
    // specific modifier combos like Ctrl+Left/Right and Shift+arrows are
    // handled by dedicated blocks below.
    if !key
        .modifiers()
        .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::META)
    {
        // Shift+Home/End/PageUp/PageDown pass through (no Vim semantics).
        let shift_nav_passthrough = key.modifiers().contains(Modifiers::SHIFT)
            && matches!(
                key.key(),
                Key::Home | Key::End | Key::PageUp | Key::PageDown
            );
        if !shift_nav_passthrough {
            match key.key() {
                // Enter passes through to the host for language-aware auto-indent
                // when native_insert is true. When native_insert is false
                // (Godot), the engine handles Enter directly.
                Key::Tab | Key::Backspace | Key::Delete | Key::Insert => return true,
                Key::Enter if !native_insert => return true,
                Key::Up | Key::Down | Key::Left | Key::Right => return true,
                Key::Home | Key::End => return true,
                Key::PageUp | Key::PageDown => return true,
                // drift: modifier+key combos not listed above (e.g. Ctrl+F-key) are not handled by vim-core and pass through to the host
                _ => {}
            }
        }
    }

    // Ctrl+Left / Ctrl+Right (word movement in insert mode).
    if key.modifiers().contains(Modifiers::CTRL) && matches!(key.key(), Key::Left | Key::Right) {
        return true;
    }

    // Shift+arrow keys (word/page movement in insert mode).
    if key.modifiers().contains(Modifiers::SHIFT)
        && matches!(key.key(), Key::Left | Key::Right | Key::Up | Key::Down)
    {
        return true;
    }

    // Ctrl-modified character keys — exact set from handle_insert_ctrl.
    if key.modifiers().contains(Modifiers::CTRL) {
        if let Key::Char(c) = key.key() {
            return matches!(
                c,
                'h' | 'j'
                    | 'k'
                    | 'm'
                    | 'i'
                    | 't'
                    | 'd'
                    | 'w'
                    | 'u'
                    | 'o'
                    | 'a'
                    | 'v'
                    | 'r'
                    | 'e'
                    | 'y'
                    | 'g'
                    | 'x'
                    | 'n'
                    | 'p'
                    | '@'
            );
        }
        return false;
    }

    // Printable characters (no modifiers) — NOT handled by the engine.
    //
    // Plain printable characters in insert mode are delegated to the host
    // editor for native typing. This enables the host's completion popup,
    // auto-close brackets, parameter hints, format-on-type, and all other
    // native editor behaviors that depend on receiving the typed character.
    //
    // The caller (VimEngine::would_handle_key) already checks:
    //   1. Parser sub-state (AwaitingInsertRegister, AwaitingInsertDigraph1,
    //      AwaitingInsertCtrlX, etc.) — caught by `!parser.state().is_ready()`
    //   2. sethandler delegation — caught before mode dispatch
    //
    // The host_session layer additionally checks:
    //   3. Mapping prefix (e.g., `j` when `jk` → Escape is mapped) — caught
    //      by `could_start_mapping()`
    //
    // Printable characters with no command modifiers: gated on native_insert.
    // When native_insert is true, return false to let the host handle them
    // natively. When native_insert is false (Godot), the engine handles all
    // printable characters itself.
    //
    // Keys that are NOT printable chars (e.g., Alt+Up, Meta+Left,
    // Shift+Home) are never handled by the engine in insert mode.
    if let Key::Char(_) = key.key() {
        if !key
            .modifiers()
            .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::META)
        {
            return !native_insert;
        }
    }

    false
}

/// Read-only classification of whether a key would be handled in CommandLine mode.
///
/// Mirrors `mode/command_line.rs::handle_key` — returns `true` for keys that
/// produce an action other than `CommandLineAction::Ignore`.
fn would_handle_command_line_key(key: KeyEvent) -> bool {
    // Escape-class keys cancel the command line.
    if matches!(key.key(), Key::Escape) || key == KeyEvent::ctrl('c') || key == KeyEvent::ctrl('[')
    {
        return true;
    }
    // Enter commits.
    if matches!(key.key(), Key::Enter) {
        return true;
    }
    match key.key() {
        // Editing keys (any modifier combo).
        Key::Backspace | Key::Delete => true,
        Key::Tab => true,
        // Ctrl/Shift+Left/Right — word movement (reject Alt/Meta).
        Key::Left | Key::Right if !key.modifiers().intersects(Modifiers::ALT | Modifiers::META) => {
            true
        }
        // Home/End/Up/Down — bare or Shift only (reject Ctrl/Alt/Meta).
        Key::Home | Key::End | Key::Up | Key::Down
            if !key
                .modifiers()
                .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::META) =>
        {
            true
        }
        // Ctrl+character editing commands.
        Key::Char(c) if key.modifiers().contains(Modifiers::CTRL) => {
            matches!(c, 'w' | 'u' | 'k' | 'h' | 'r' | 'f')
        }
        // Printable characters (no command modifiers) — inserted as text.
        Key::Char(_)
            if !key
                .modifiers()
                .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::META) =>
        {
            true
        }
        // Everything else passes through.
        // drift: unrecognised key types in command-line mode are not handled by vim-core and pass through to the host
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::VimEngine;
    use crate::keymap::KeyEvent;

    // ── Normal mode ──────────────────────────────────────────────────────

    #[test]
    fn normal_mode_plain_keys_handled() {
        let engine = VimEngine::new();
        // All plain letter keys are handled in Normal mode (motions, operators,
        // actions, mode switches, etc.). Even keys that produce Invalid are
        // classified as something other than Unknown in the core keymap.
        for c in "hjklwWeEbBdcyxXpPuUiIaAoOvVrRsSnN".chars() {
            assert!(
                engine.would_handle_key(KeyEvent::char(c)),
                "Normal: plain '{}' should be handled",
                c
            );
        }
    }

    #[test]
    fn normal_mode_ctrl_keys_handled() {
        let engine = VimEngine::new();
        // Ctrl keys in the Normal core keymap: a,b,d,e,f,g,i,m,n,o,p,r,u,v,w,x,y
        // Plus Ctrl+C and Ctrl+[ which are Escape-class (handled before table).
        for c in "abcdefgimnopruvwxy[".chars() {
            assert!(
                engine.would_handle_key(KeyEvent::ctrl(c)),
                "Normal: Ctrl+{} should be handled",
                c
            );
        }
    }

    #[test]
    fn normal_mode_ctrl_keys_not_handled() {
        let engine = VimEngine::new();
        // Ctrl keys NOT in the Normal core keymap → should not be handled.
        for c in "klqstz".chars() {
            assert!(
                !engine.would_handle_key(KeyEvent::ctrl(c)),
                "Normal: Ctrl+{} should NOT be handled",
                c
            );
        }
    }

    #[test]
    fn normal_mode_escape_handled() {
        let engine = VimEngine::new();
        assert!(engine.would_handle_key(KeyEvent::escape()));
        assert!(engine.would_handle_key(KeyEvent::ctrl('c')));
        assert!(engine.would_handle_key(KeyEvent::ctrl('[')));
    }

    // ── Insert mode ──────────────────────────────────────────────────────

    #[test]
    fn insert_mode_printable_chars_not_handled() {
        // Plain printable characters in insert mode are delegated to the host
        // for native typing (completion popup, auto-close brackets, etc.)
        // when native_insert is enabled.
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        engine.set_native_insert(true);

        for c in "abcdefghijklmnopqrstuvwxyz0123456789 ".chars() {
            assert!(
                !engine.would_handle_key(KeyEvent::char(c)),
                "Insert: printable '{}' should NOT be handled (native insert path)",
                c
            );
        }
    }

    #[test]
    fn insert_mode_special_keys_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        engine.set_native_insert(true);

        assert!(engine.would_handle_key(KeyEvent::new(Key::Tab, Modifiers::NONE)));
        // Enter passes through to host for language-aware auto-indent
        // when native_insert is true
        assert!(!engine.would_handle_key(KeyEvent::enter()));
        assert!(engine.would_handle_key(KeyEvent::backspace()));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Delete, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Up, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Down, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Right, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Home, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::End, Modifiers::NONE)));
    }

    #[test]
    fn insert_mode_ctrl_word_movement() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::CTRL)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Right, Modifiers::CTRL)));
    }

    #[test]
    fn insert_mode_ctrl_keys_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        // Exact set from handle_insert_ctrl: h,j,k,m,i,t,d,w,u,o,a,v,r,e,y,g,x,n,p
        for c in "hjkmitdwuoavreygxnp".chars() {
            assert!(
                engine.would_handle_key(KeyEvent::ctrl(c)),
                "Insert: Ctrl+{} should be handled",
                c
            );
        }
    }

    #[test]
    fn insert_mode_ctrl_keys_not_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        // Ctrl keys NOT handled by the insert grammar.
        // Note: Ctrl+C and Ctrl+[ are Escape-class → handled.
        for c in "bflqsz".chars() {
            assert!(
                !engine.would_handle_key(KeyEvent::ctrl(c)),
                "Insert: Ctrl+{} should NOT be handled",
                c
            );
        }
    }

    #[test]
    fn insert_mode_escape_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(engine.would_handle_key(KeyEvent::escape()));
        assert!(engine.would_handle_key(KeyEvent::ctrl('c')));
        assert!(engine.would_handle_key(KeyEvent::ctrl('[')));
    }

    // ── CommandLine would_handle precision ──────────────────────────────

    fn make_cmdline_engine() -> VimEngine {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::CommandLine);
        assert!(matches!(engine.mode(), Mode::CommandLine));
        engine
    }

    #[test]
    fn cmdline_handles_escape() {
        let engine = make_cmdline_engine();
        assert!(engine.would_handle_key(KeyEvent::escape()));
    }

    #[test]
    fn cmdline_handles_enter() {
        let engine = make_cmdline_engine();
        assert!(engine.would_handle_key(KeyEvent::enter()));
    }

    #[test]
    fn cmdline_handles_backspace_delete() {
        let engine = make_cmdline_engine();
        assert!(engine.would_handle_key(KeyEvent::backspace()));
        assert!(engine.would_handle_key(KeyEvent::delete()));
    }

    #[test]
    fn cmdline_handles_arrows() {
        let engine = make_cmdline_engine();
        assert!(engine.would_handle_key(KeyEvent::left()));
        assert!(engine.would_handle_key(KeyEvent::right()));
        assert!(engine.would_handle_key(KeyEvent::up()));
        assert!(engine.would_handle_key(KeyEvent::down()));
    }

    #[test]
    fn cmdline_handles_home_end() {
        let engine = make_cmdline_engine();
        assert!(engine.would_handle_key(KeyEvent::new(Key::Home, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::End, Modifiers::NONE)));
    }

    #[test]
    fn cmdline_handles_tab() {
        let engine = make_cmdline_engine();
        assert!(engine.would_handle_key(KeyEvent::tab()));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Tab, Modifiers::SHIFT)));
    }

    #[test]
    fn cmdline_handles_ctrl_editing() {
        let engine = make_cmdline_engine();
        for c in ['w', 'u', 'k', 'h', 'r', 'f'] {
            assert!(
                engine.would_handle_key(KeyEvent::ctrl(c)),
                "Ctrl+{c} should be handled in CommandLine"
            );
        }
    }

    #[test]
    fn cmdline_handles_printable_chars() {
        let engine = make_cmdline_engine();
        assert!(engine.would_handle_key(KeyEvent::char('a')));
        assert!(engine.would_handle_key(KeyEvent::char('Z')));
        assert!(engine.would_handle_key(KeyEvent::char('0')));
        assert!(engine.would_handle_key(KeyEvent::char(' ')));
    }

    #[test]
    fn cmdline_does_not_handle_pageup_pagedown() {
        let engine = make_cmdline_engine();
        assert!(!engine.would_handle_key(KeyEvent::new(Key::PageUp, Modifiers::NONE)));
        assert!(!engine.would_handle_key(KeyEvent::new(Key::PageDown, Modifiers::NONE)));
    }

    #[test]
    fn cmdline_does_not_handle_fkeys() {
        let engine = make_cmdline_engine();
        assert!(!engine.would_handle_key(KeyEvent::f(1)));
        assert!(!engine.would_handle_key(KeyEvent::f(5)));
    }

    #[test]
    fn cmdline_does_not_handle_ctrl_up_down() {
        let engine = make_cmdline_engine();
        assert!(!engine.would_handle_key(KeyEvent::new(Key::Up, Modifiers::CTRL)));
        assert!(!engine.would_handle_key(KeyEvent::new(Key::Down, Modifiers::CTRL)));
    }

    #[test]
    fn cmdline_handles_ctrl_left_right() {
        let engine = make_cmdline_engine();
        assert!(
            engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::CTRL)),
            "Ctrl+Left should be handled (word movement)"
        );
        assert!(
            engine.would_handle_key(KeyEvent::new(Key::Right, Modifiers::CTRL)),
            "Ctrl+Right should be handled (word movement)"
        );
    }

    #[test]
    fn cmdline_handles_shift_left_right() {
        let engine = make_cmdline_engine();
        assert!(
            engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::SHIFT)),
            "Shift+Left should be handled (word movement)"
        );
        assert!(
            engine.would_handle_key(KeyEvent::new(Key::Right, Modifiers::SHIFT)),
            "Shift+Right should be handled (word movement)"
        );
    }

    #[test]
    fn cmdline_does_not_handle_ctrl_home_end() {
        let engine = make_cmdline_engine();
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Home, Modifiers::CTRL)),
            "Ctrl+Home should NOT be handled"
        );
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::End, Modifiers::CTRL)),
            "Ctrl+End should NOT be handled"
        );
    }

    #[test]
    fn cmdline_does_not_handle_alt_arrows() {
        let engine = make_cmdline_engine();
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::ALT)),
            "Alt+Left should NOT be handled"
        );
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Right, Modifiers::ALT)),
            "Alt+Right should NOT be handled"
        );
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Up, Modifiers::ALT)),
            "Alt+Up should NOT be handled"
        );
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Down, Modifiers::ALT)),
            "Alt+Down should NOT be handled"
        );
    }

    #[test]
    fn cmdline_shift_home_end_handled() {
        let engine = make_cmdline_engine();
        assert!(
            engine.would_handle_key(KeyEvent::new(Key::Home, Modifiers::SHIFT)),
            "Shift+Home should be handled"
        );
        assert!(
            engine.would_handle_key(KeyEvent::new(Key::End, Modifiers::SHIFT)),
            "Shift+End should be handled"
        );
    }

    #[test]
    fn cmdline_does_not_handle_unknown_ctrl() {
        let engine = make_cmdline_engine();
        assert!(!engine.would_handle_key(KeyEvent::ctrl('z')));
        assert!(!engine.would_handle_key(KeyEvent::ctrl('s')));
    }

    // ── Non-ready parser state ───────────────────────────────────────────

    #[test]
    fn non_ready_state_handles_all_keys() {
        let engine = VimEngine::new();

        // Feed 'd' to enter operator-pending state.
        let doc = crate::test_utils::SimpleDocument::new("hello\nworld\n");
        let ctx = crate::execution::InputContext::new(&doc, 0).validate_clamped();
        let mut engine2 = VimEngine::new();
        let _ = engine2.process(KeyEvent::char('d'), ctx);

        // Parser is now in non-ready state (operator pending).
        // Any key should be "handled" — it could be a motion or text object.
        assert!(engine2.would_handle_key(KeyEvent::ctrl('s')));
        assert!(engine2.would_handle_key(KeyEvent::ctrl('z')));
        assert!(engine2.would_handle_key(KeyEvent::char('q')));
    }

    #[test]
    fn insert_ctrl_at_sign_handled() {
        // Ctrl+@ is 'LastInsertedAndExit' in grammar (insert.rs:145).
        // would_handle must agree.
        let mut engine2 = VimEngine::new();
        let doc = crate::test_utils::SimpleDocument::new("hello");
        let ctx = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        engine2.process(KeyEvent::char('i'), ctx);
        // Now in Insert mode
        assert!(
            engine2.would_handle_key(KeyEvent::ctrl('@')),
            "Ctrl+@ should be handled in Insert mode (LastInsertedAndExit)"
        );
    }

    // ── Fidelity: Insert Ctrl keys match grammar handler ─────────────────

    #[test]
    fn insert_ctrl_keys_match_grammar_handler() {
        // Verify that `would_handle_insert_key` agrees with
        // `Parser::handle_insert` for every Ctrl+{a-z} key.
        // This prevents drift between the would_handle classification
        // and the actual grammar handler.
        let mut parser = crate::grammar::Parser::new();
        let keymap = crate::keymap::Keymap::new();

        for c in 'a'..='z' {
            let key = KeyEvent::ctrl(c);

            // Check if the grammar handler produces a non-Invalid result.
            let grammar_result = parser.handle_insert(key);
            let grammar_handles = !matches!(grammar_result, crate::grammar::GrammarResult::Invalid);

            // Check if our classification agrees.
            let classify_fn = |k: KeyEvent| keymap.classify_core(k, Mode::Insert);
            let would_handle = would_handle_insert_key(key, true, classify_fn);

            // Escape-class keys (c, [) are handled by the mode handler
            // before reaching the grammar, so the grammar returns Invalid
            // but would_handle correctly returns true. Account for this.
            let is_escape_class = keymap.classify_core(key, Mode::Insert) == KeyClass::Escape;

            if is_escape_class {
                assert!(
                    would_handle,
                    "Ctrl+{}: escape-class key should be handled",
                    c
                );
            } else {
                assert_eq!(
                    grammar_handles, would_handle,
                    "Ctrl+{}: grammar says {}, would_handle says {} — DRIFT DETECTED",
                    c, grammar_handles, would_handle
                );
            }

            // Reset parser for next iteration (handle_insert may mutate state
            // for keys like Ctrl+R which enter AwaitingInsertRegister).
            parser.reset();
        }

        // Also verify non-letter Ctrl chars that the grammar handles.
        // '@' is not in 'a'..='z' but handle_insert_ctrl matches it.
        for c in ['@'] {
            let key = KeyEvent::ctrl(c);
            let grammar_result = parser.handle_insert(key);
            let grammar_handles = !matches!(grammar_result, crate::grammar::GrammarResult::Invalid);
            let classify_fn = |k: KeyEvent| keymap.classify_core(k, Mode::Insert);
            let would_handle = would_handle_insert_key(key, true, classify_fn);
            assert_eq!(
                grammar_handles, would_handle,
                "Ctrl+{}: grammar says {}, would_handle says {} — DRIFT DETECTED",
                c, grammar_handles, would_handle
            );
            parser.reset();
        }
    }

    // ── Insert mode modifier precision ──────────────────────────────────

    #[test]
    fn insert_mode_bare_named_keys_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        // Enter passes through to host for language-aware auto-indent
        for key in [
            Key::Tab,
            Key::Backspace,
            Key::Delete,
            Key::Insert,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ] {
            assert!(
                engine.would_handle_key(KeyEvent::new(key, Modifiers::NONE)),
                "Insert: bare {:?} should be handled",
                key
            );
        }
        // Enter passes through to host for auto-indent when native_insert=true
        engine.set_native_insert(true);
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Enter, Modifiers::NONE)),
            "Insert: Enter should pass through to host for auto-indent (native_insert=true)"
        );
    }

    #[test]
    fn insert_mode_alt_named_keys_not_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        for key in [
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ] {
            assert!(
                !engine.would_handle_key(KeyEvent::new(key, Modifiers::ALT)),
                "Insert: Alt+{:?} should NOT be handled",
                key
            );
        }
    }

    #[test]
    fn insert_mode_ctrl_home_end_not_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Home, Modifiers::CTRL)),
            "Insert: Ctrl+Home should NOT be handled"
        );
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::End, Modifiers::CTRL)),
            "Insert: Ctrl+End should NOT be handled"
        );
    }

    #[test]
    fn insert_mode_meta_arrows_not_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::META)),
            "Insert: Meta+Left should NOT be handled"
        );
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Right, Modifiers::META)),
            "Insert: Meta+Right should NOT be handled"
        );
    }

    #[test]
    fn insert_mode_ctrl_left_right_still_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::CTRL)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Right, Modifiers::CTRL)));
    }

    #[test]
    fn insert_mode_shift_arrows_still_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::SHIFT)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Right, Modifiers::SHIFT)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Up, Modifiers::SHIFT)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Down, Modifiers::SHIFT)));
    }

    #[test]
    fn insert_mode_shift_home_end_not_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::Home, Modifiers::SHIFT)),
            "<S-Home> should pass through"
        );
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::End, Modifiers::SHIFT)),
            "<S-End> should pass through"
        );
    }

    #[test]
    fn insert_mode_pageup_pagedown_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(engine.would_handle_key(KeyEvent::new(Key::PageUp, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::PageDown, Modifiers::NONE)));
    }

    #[test]
    fn shift_tab_handled_in_insert() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        let key = KeyEvent::new(Key::Tab, Modifiers::SHIFT);
        assert!(
            engine.would_handle_key(key),
            "<S-Tab> should be handled (outdent)"
        );
    }

    #[test]
    fn shift_home_not_handled_in_insert() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        let key = KeyEvent::new(Key::Home, Modifiers::SHIFT);
        assert!(
            !engine.would_handle_key(key),
            "<S-Home> should pass through"
        );
    }

    #[test]
    fn shift_end_not_handled_in_insert() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        let key = KeyEvent::new(Key::End, Modifiers::SHIFT);
        assert!(!engine.would_handle_key(key), "<S-End> should pass through");
    }

    #[test]
    fn shift_pageup_not_handled_in_insert() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        let key = KeyEvent::new(Key::PageUp, Modifiers::SHIFT);
        assert!(
            !engine.would_handle_key(key),
            "<S-PageUp> should pass through"
        );
    }

    #[test]
    fn shift_pagedown_not_handled_in_insert() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        let key = KeyEvent::new(Key::PageDown, Modifiers::SHIFT);
        assert!(
            !engine.would_handle_key(key),
            "<S-PageDown> should pass through"
        );
    }

    #[test]
    fn insert_mode_ctrl_pageup_pagedown_not_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);

        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::PageUp, Modifiers::CTRL)),
            "Insert: Ctrl+PageUp should NOT be handled"
        );
        assert!(
            !engine.would_handle_key(KeyEvent::new(Key::PageDown, Modifiers::CTRL)),
            "Insert: Ctrl+PageDown should NOT be handled"
        );
    }

    #[test]
    fn visual_ctrl_g_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Visual(crate::primitives::VisualType::Char));
        assert!(
            engine.would_handle_key(KeyEvent::ctrl('g')),
            "Visual: Ctrl+G should be handled (ToggleSelect)"
        );
    }

    #[test]
    fn visual_uppercase_s_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Visual(crate::primitives::VisualType::Char));
        assert!(
            engine.would_handle_key(KeyEvent::char('S')),
            "Visual: S should be handled (substitute/surround)"
        );
    }

    // ── Native insert path: printable chars delegated to host ───────────

    #[test]
    fn insert_mode_unicode_printable_not_handled() {
        // Unicode printable characters (CJK, accented, emoji, etc.) are also
        // delegated to the host for native typing when native_insert is true.
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        engine.set_native_insert(true);

        for c in ['\u{00E9}', '\u{4E16}', '\u{0410}', '\u{03B1}'] {
            assert!(
                !engine.would_handle_key(KeyEvent::char(c)),
                "Insert: Unicode '{}' should NOT be handled (native insert path)",
                c
            );
        }
    }

    #[test]
    fn insert_mode_parser_substate_handles_printable() {
        // When the parser is in an insert sub-state (e.g., AwaitingInsertRegister
        // after Ctrl-R), ALL keys are handled — the parser.state().is_ready()
        // check catches this before we reach the insert key classification.
        let mut engine = VimEngine::new();
        let doc = crate::test_utils::SimpleDocument::new("hello");
        let ctx = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();

        // Enter Insert mode
        engine.process(KeyEvent::char('i'), ctx);
        assert!(matches!(engine.mode(), Mode::Insert));

        // Press Ctrl-R to enter AwaitingInsertRegister state
        let ctx2 = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        engine.process(KeyEvent::ctrl('r'), ctx2);

        // Now a plain printable character should be handled (it completes the
        // register name sequence). Parser is non-ready so all keys are handled.
        assert!(
            engine.would_handle_key(KeyEvent::char('a')),
            "Insert sub-state: plain 'a' should be handled (register name)"
        );
    }

    #[test]
    fn insert_mode_digraph_substate_handles_printable() {
        // After Ctrl-K in insert mode, the parser enters AwaitingInsertDigraph1.
        // Printable characters should be handled as digraph input.
        let mut engine = VimEngine::new();
        let doc = crate::test_utils::SimpleDocument::new("hello");
        let ctx = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();

        engine.process(KeyEvent::char('i'), ctx);
        let ctx2 = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        engine.process(KeyEvent::ctrl('k'), ctx2);

        // 'e' should be handled as first digraph character
        assert!(
            engine.would_handle_key(KeyEvent::char('e')),
            "Digraph sub-state: plain 'e' should be handled"
        );
    }

    #[test]
    fn insert_mode_ctrl_x_substate_handles_printable() {
        // After Ctrl-X in insert mode, the parser enters AwaitingInsertCtrlX.
        // Keys (including Ctrl+keys) should be handled.
        let mut engine = VimEngine::new();
        let doc = crate::test_utils::SimpleDocument::new("hello");
        let ctx = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();

        engine.process(KeyEvent::char('i'), ctx);
        let ctx2 = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        engine.process(KeyEvent::ctrl('x'), ctx2);

        // Ctrl-N should be handled as completion command
        assert!(
            engine.would_handle_key(KeyEvent::ctrl('n')),
            "Ctrl-X sub-state: Ctrl+N should be handled (completion)"
        );
    }

    #[test]
    fn insert_mode_literal_substate_handles_printable() {
        // After Ctrl-V in insert mode, the parser enters InsertLiteral(AwaitingFirst).
        // The next key is inserted verbatim.
        let mut engine = VimEngine::new();
        let doc = crate::test_utils::SimpleDocument::new("hello");
        let ctx = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();

        engine.process(KeyEvent::char('i'), ctx);
        let ctx2 = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        engine.process(KeyEvent::ctrl('v'), ctx2);

        // Even Escape should be handled (inserted literally)
        assert!(
            engine.would_handle_key(KeyEvent::escape()),
            "Literal sub-state: Escape should be handled (inserted verbatim)"
        );
    }

    #[test]
    fn replace_mode_printable_chars_not_handled() {
        // Replace mode uses the same `would_handle_insert_key` path.
        // Printable characters should also be delegated to the host
        // when native_insert is true.
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Replace);
        engine.set_native_insert(true);

        assert!(
            !engine.would_handle_key(KeyEvent::char('x')),
            "Replace: printable 'x' should NOT be handled (native path)"
        );
    }

    #[test]
    fn virtual_replace_mode_printable_chars_not_handled() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::VirtualReplace);
        engine.set_native_insert(true);

        assert!(
            !engine.would_handle_key(KeyEvent::char('z')),
            "VirtualReplace: printable 'z' should NOT be handled (native path)"
        );
    }

    // ── NativeInsert capability gating ──────────────────────────────────

    #[test]
    fn default_engine_handles_printable_in_insert() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        assert!(!engine.native_insert(), "default is false");
        assert!(
            engine.would_handle_key(KeyEvent::char('a')),
            "Insert: 'a' should be handled when native_insert=false"
        );
        assert!(
            engine.would_handle_key(KeyEvent::char(' ')),
            "Insert: space should be handled when native_insert=false"
        );
    }

    #[test]
    fn default_engine_handles_enter_in_insert() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        assert!(
            engine.would_handle_key(KeyEvent::enter()),
            "Insert: Enter should be handled when native_insert=false"
        );
    }

    #[test]
    fn native_insert_passes_through_printable() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        engine.set_native_insert(true);
        assert!(
            !engine.would_handle_key(KeyEvent::char('a')),
            "Insert: 'a' should pass through when native_insert=true"
        );
    }

    #[test]
    fn native_insert_passes_through_enter() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        engine.set_native_insert(true);
        assert!(
            !engine.would_handle_key(KeyEvent::enter()),
            "Insert: Enter should pass through when native_insert=true"
        );
    }

    #[test]
    fn native_insert_still_handles_special_keys() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Insert);
        engine.set_native_insert(true);
        assert!(engine.would_handle_key(KeyEvent::backspace()));
        assert!(engine.would_handle_key(KeyEvent::escape()));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Tab, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Delete, Modifiers::NONE)));
        assert!(engine.would_handle_key(KeyEvent::new(Key::Left, Modifiers::NONE)));
    }

    #[test]
    fn native_insert_replace_mode_same_behavior() {
        let mut engine = VimEngine::new();
        engine.set_mode(Mode::Replace);
        engine.set_native_insert(true);
        assert!(
            !engine.would_handle_key(KeyEvent::char('x')),
            "Replace: printable should pass through with native_insert=true"
        );
        engine.set_native_insert(false);
        assert!(
            engine.would_handle_key(KeyEvent::char('x')),
            "Replace: printable should be handled with native_insert=false"
        );
    }
}
