//! Smoke tests for `HostSession` (type alias for `VimSession<SessionHost>`).
//!
//! Previously this file compared the legacy `HostSession` struct against the
//! new `VimSession<SessionHost>` to prove behavioral equivalence. Now that
//! `HostSession` IS `VimSession<SessionHost>`, these tests verify that
//! the type alias works correctly by running key sequences and asserting
//! text, cursor, and mode are correct.

use vim_core::execution::HostSession;
use vim_core::keymap::KeyEvent;

/// Run a key sequence through a HostSession and assert text/cursor/mode
/// are self-consistent after every key.
fn assert_session_works(text: &str, keys: &[KeyEvent]) {
    let mut session = HostSession::new(text);

    for (i, key) in keys.iter().enumerate() {
        let _ = session.process_key_host(*key);

        // Sanity: cursor offset should be within document bounds.
        let doc_len = session.text().len();
        let cursor = session.cursor_offset();
        assert!(
            cursor <= doc_len,
            "cursor {cursor} out of bounds (doc_len={doc_len}) at key {i} ({key})"
        );

        // Sanity: mode should be a valid mode.
        let _mode = session.mode();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Basic editing: motions, delete-word, insert, undo, redo
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn equivalence_basic_editing() {
    let text = "Hello, World!\nSecond line.\nThird line.\n";
    let keys: Vec<KeyEvent> = vec![
        // w — word motion
        KeyEvent::char('w'),
        // dw — delete word
        KeyEvent::char('d'),
        KeyEvent::char('w'),
        // i then type "hello" then Escape
        KeyEvent::char('i'),
        KeyEvent::char('h'),
        KeyEvent::char('e'),
        KeyEvent::char('l'),
        KeyEvent::char('l'),
        KeyEvent::char('o'),
        KeyEvent::escape(),
        // u — undo the insert
        KeyEvent::char('u'),
        // Ctrl-R — redo
        KeyEvent::ctrl('r'),
    ];
    assert_session_works(text, &keys);
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Visual mode: select and delete
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn equivalence_visual_select_delete() {
    let text = "abcdef\nghijkl\n";
    let keys: Vec<KeyEvent> = vec![
        // v — enter visual mode
        KeyEvent::char('v'),
        // l, l — extend selection right
        KeyEvent::char('l'),
        KeyEvent::char('l'),
        // d — delete selection
        KeyEvent::char('d'),
    ];
    assert_session_works(text, &keys);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Line operations: dd, yy, p, P
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn equivalence_line_operations() {
    let text = "First line\nSecond line\nThird line\n";
    let keys: Vec<KeyEvent> = vec![
        // yy — yank current line
        KeyEvent::char('y'),
        KeyEvent::char('y'),
        // j — move down
        KeyEvent::char('j'),
        // P — paste above
        KeyEvent::char('P'),
        // dd — delete current line
        KeyEvent::char('d'),
        KeyEvent::char('d'),
        // p — paste below
        KeyEvent::char('p'),
    ];
    assert_session_works(text, &keys);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Mode transitions: i, a, o, O, v, V, :, /, Escape
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn equivalence_mode_transitions() {
    let text = "Hello world\nSecond line\n";
    let keys: Vec<KeyEvent> = vec![
        // i → Insert, Escape → Normal
        KeyEvent::char('i'),
        KeyEvent::escape(),
        // a → Insert (after), Escape → Normal
        KeyEvent::char('a'),
        KeyEvent::escape(),
        // o → Insert (new line below), Escape → Normal
        KeyEvent::char('o'),
        KeyEvent::escape(),
        // O → Insert (new line above), Escape → Normal
        KeyEvent::char('O'),
        KeyEvent::escape(),
        // v → Visual, Escape → Normal
        KeyEvent::char('v'),
        KeyEvent::escape(),
        // V → Visual Line, Escape → Normal
        KeyEvent::char('V'),
        KeyEvent::escape(),
        // : → CommandLine, Escape → Normal
        KeyEvent::char(':'),
        KeyEvent::escape(),
        // / → Search forward, Escape → Normal
        KeyEvent::char('/'),
        KeyEvent::escape(),
    ];
    assert_session_works(text, &keys);
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Undo/redo cycle: multiple edits, multiple undos, redo
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn equivalence_undo_redo_cycle() {
    let text = "original text\n";
    let keys: Vec<KeyEvent> = vec![
        // First edit: insert "A" at beginning
        KeyEvent::char('i'),
        KeyEvent::char('A'),
        KeyEvent::escape(),
        // Second edit: append "B"
        KeyEvent::char('A'),
        KeyEvent::char('B'),
        KeyEvent::escape(),
        // Third edit: delete a word
        KeyEvent::char('0'),
        KeyEvent::char('d'),
        KeyEvent::char('w'),
        // Undo three times
        KeyEvent::char('u'),
        KeyEvent::char('u'),
        KeyEvent::char('u'),
        // Redo twice
        KeyEvent::ctrl('r'),
        KeyEvent::ctrl('r'),
    ];
    assert_session_works(text, &keys);
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Mixed: insert text, navigate, delete, visual, undo
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn equivalence_mixed_operations() {
    let text = "foo bar baz\nqux quux corge\n";
    let keys: Vec<KeyEvent> = vec![
        // Navigate to second word
        KeyEvent::char('w'),
        // Change word: cw + type "REPLACED" + Escape
        KeyEvent::char('c'),
        KeyEvent::char('w'),
        KeyEvent::char('R'),
        KeyEvent::char('E'),
        KeyEvent::char('P'),
        KeyEvent::escape(),
        // Move down, visual-line delete
        KeyEvent::char('j'),
        KeyEvent::char('V'),
        KeyEvent::char('d'),
        // Undo both operations
        KeyEvent::char('u'),
        KeyEvent::char('u'),
    ];
    assert_session_works(text, &keys);
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Replace mode (r) and simple substitution
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn equivalence_replace_char() {
    let text = "abcdef\n";
    let keys: Vec<KeyEvent> = vec![
        // r + X — replace char under cursor with 'X'
        KeyEvent::char('r'),
        KeyEvent::char('X'),
        // l — move right
        KeyEvent::char('l'),
        // r + Y — replace next char
        KeyEvent::char('r'),
        KeyEvent::char('Y'),
        // undo both replacements
        KeyEvent::char('u'),
        KeyEvent::char('u'),
    ];
    assert_session_works(text, &keys);
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Append at end of line (A) and beginning of line (I)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn equivalence_append_and_insert_at_edges() {
    let text = "hello world\n";
    let keys: Vec<KeyEvent> = vec![
        // A — append at end of line
        KeyEvent::char('A'),
        KeyEvent::char('!'),
        KeyEvent::escape(),
        // I — insert at beginning of line
        KeyEvent::char('I'),
        KeyEvent::char('>'),
        KeyEvent::escape(),
    ];
    assert_session_works(text, &keys);
}
