//! Tests for `<Action>(name)` and `<Plug>(name)` key notation parsing.
//!
//! Included via `#[path = "key_notation_tests.rs"] mod tests;` in `key_notation.rs`.

use super::*;
use crate::keymap::{Key, KeyEvent, Keymap};

// ── Basic <Action>(name) parsing ────────────────────────────────────

#[test]
fn action_notation_parses_with_keymap() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Action>(Rename)", Some(&mut keymap));
    assert_eq!(seq.len(), 1, "Should parse as single key event");
    assert!(
        matches!(seq[0].key(), Key::Action(id) if id == 0),
        "First registered action should get id 0, got {:?}",
        seq[0],
    );
    // The name should be registered in the keymap
    assert_eq!(keymap.action_name(0), Some("Rename"));
}

#[test]
fn action_notation_different_names_get_different_ids() {
    let mut keymap = Keymap::new();
    let seq1 = parse_key_notation_sequence("<Action>(Rename)", Some(&mut keymap));
    let seq2 = parse_key_notation_sequence("<Action>(Debug)", Some(&mut keymap));

    let id1 = match seq1[0].key() {
        Key::Action(id) => id,
        other => panic!("Expected Action, got {other:?}"),
    };
    let id2 = match seq2[0].key() {
        Key::Action(id) => id,
        other => panic!("Expected Action, got {other:?}"),
    };

    assert_ne!(id1, id2, "Different action names should get different ids");
    assert_eq!(keymap.action_name(id1), Some("Rename"));
    assert_eq!(keymap.action_name(id2), Some("Debug"));
}

#[test]
fn action_notation_same_name_returns_same_id() {
    let mut keymap = Keymap::new();
    let seq1 = parse_key_notation_sequence("<Action>(Rename)", Some(&mut keymap));
    let seq2 = parse_key_notation_sequence("<Action>(Rename)", Some(&mut keymap));

    assert_eq!(
        seq1[0], seq2[0],
        "Same action name should produce same KeyEvent"
    );
}

// ── <Plug>(name) parsing ────────────────────────────────────────────

#[test]
fn plug_notation_parses_with_keymap() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Plug>(surround-word)", Some(&mut keymap));
    assert_eq!(seq.len(), 1);
    assert!(matches!(seq[0].key(), Key::Plug(0)));
    assert_eq!(keymap.plug_name(0), Some("surround-word"));
}

// ── Mixed notation parsing ──────────────────────────────────────────

#[test]
fn action_notation_mixed_with_regular_keys() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Leader>r<Action>(Rename)", Some(&mut keymap));
    // <Leader> + 'r' + <Action>(Rename) = 3 keys
    assert_eq!(seq.len(), 3);
    assert_eq!(seq[0].key(), Key::Leader);
    assert_eq!(seq[1], KeyEvent::char('r'));
    assert!(matches!(seq[2].key(), Key::Action(_)));
}

#[test]
fn multiple_actions_in_sequence() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Action>(First)<Action>(Second)", Some(&mut keymap));
    assert_eq!(seq.len(), 2);
    let id1 = match seq[0].key() {
        Key::Action(id) => id,
        other => panic!("Expected Action, got {other:?}"),
    };
    let id2 = match seq[1].key() {
        Key::Action(id) => id,
        other => panic!("Expected Action, got {other:?}"),
    };
    assert_ne!(id1, id2);
    assert_eq!(keymap.action_name(id1), Some("First"));
    assert_eq!(keymap.action_name(id2), Some("Second"));
}

// ── Edge cases ──────────────────────────────────────────────────────

#[test]
fn action_without_parens_produces_sentinel() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Action>jj", Some(&mut keymap));
    // <Action> without (name) → sentinel Action(u32::MAX) + literal 'j', 'j'
    assert_eq!(seq.len(), 3);
    assert!(matches!(seq[0].key(), Key::Action(u32::MAX)));
    assert_eq!(seq[1], KeyEvent::char('j'));
    assert_eq!(seq[2], KeyEvent::char('j'));
}

#[test]
fn action_with_empty_name_produces_sentinel() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Action>()jj", Some(&mut keymap));
    // <Action>() — empty name → sentinel Action(u32::MAX), then '(', ')', 'j', 'j'
    // The parser sees <Action> sentinel, then looks at "()jj",
    // finds '(' and ')' at index 1 but name is empty → returns None.
    // So we get: Action(u32::MAX), '(', ')', 'j', 'j'
    assert_eq!(seq.len(), 5);
    assert!(matches!(seq[0].key(), Key::Action(u32::MAX)));
    assert_eq!(seq[1], KeyEvent::char('('));
    assert_eq!(seq[2], KeyEvent::char(')'));
}

#[test]
fn action_with_unclosed_paren_produces_sentinel_and_literals() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Action>(Foo", Some(&mut keymap));
    // No closing ')' → sentinel Action(u32::MAX), then literal '(', 'F', 'o', 'o'
    assert_eq!(seq.len(), 5);
    assert!(matches!(seq[0].key(), Key::Action(u32::MAX)));
    assert_eq!(seq[1], KeyEvent::char('('));
}

#[test]
fn action_notation_case_insensitive() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<action>(Rename)", Some(&mut keymap));
    assert_eq!(seq.len(), 1);
    assert!(matches!(seq[0].key(), Key::Action(_)));
    assert_eq!(keymap.action_name(0), Some("Rename"));
}

#[test]
fn action_name_preserves_case() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Action>(ReformatCode)", Some(&mut keymap));
    assert_eq!(seq.len(), 1);
    let id = match seq[0].key() {
        Key::Action(id) => id,
        other => panic!("Expected Action, got {other:?}"),
    };
    assert_eq!(
        keymap.action_name(id),
        Some("ReformatCode"),
        "Action name should preserve original case"
    );
}

#[test]
fn action_name_with_special_chars() {
    let mut keymap = Keymap::new();
    let seq = parse_key_notation_sequence("<Action>(my-action.name_v2)", Some(&mut keymap));
    assert_eq!(seq.len(), 1);
    assert!(matches!(seq[0].key(), Key::Action(_)));
    assert_eq!(keymap.action_name(0), Some("my-action.name_v2"));
}

// ── Without keymap (no registry) ────────────────────────────────────

#[test]
fn action_notation_without_keymap_still_parses_compound() {
    // When keymap is None, <Action>(name) should still be recognized
    // as a compound token, but with sentinel id u32::MAX
    let seq = parse_key_notation_sequence("<Action>(Rename)", None);
    assert_eq!(
        seq.len(),
        1,
        "Should still parse as single key event without keymap"
    );
    assert!(matches!(seq[0].key(), Key::Action(u32::MAX)));
}
