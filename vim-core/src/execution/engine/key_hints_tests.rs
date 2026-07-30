//! Tests for [`VimEngine::key_hints()`].

use super::super::VimEngine;
use crate::keymap::Keymap;

#[test]
fn key_hints_none_when_ready() {
    let engine = VimEngine::new();
    let keymap = Keymap::new();
    assert!(engine.key_hints(&keymap).is_none());
}

#[test]
fn key_hints_none_in_insert_mode() {
    let mut engine = VimEngine::new();
    engine.set_mode(crate::primitives::Mode::Insert);
    let keymap = Keymap::new();
    assert!(engine.key_hints(&keymap).is_none());
}
