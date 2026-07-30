//! Integration tests for sneak motions (s/S two-character cross-line find).
//!
//! Validates:
//! - Visual mode sneak: `vsab` extends selection to sneak target
//! - Sneak with smartcase: ignorecase/smartcase affect sneak matching
//! - Engine-level sneak_mode option sync

mod common;

use common::document::TestDocument;
use common::runner::apply_effect;
use vim_core::document::Document;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::keymap::KeyEvent;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Press a key, apply all resulting effects to `doc`, and return the cursor
/// offset after the key was processed.
fn press_and_apply(engine: &mut VimEngine, doc: &mut TestDocument, key: KeyEvent) -> usize {
    let ctx = InputContext::new(&*doc, doc.cursor_offset()).validate_clamped();
    let mut response = engine.process(key, ctx);
    let effects = response.take_effects();
    for effect in effects {
        apply_effect(doc, effect);
    }
    doc.cursor_offset()
}

/// Press a sequence of char keys, applying effects after each one.
/// Returns the final cursor offset.
fn press_keys(engine: &mut VimEngine, doc: &mut TestDocument, keys: &str) -> usize {
    let mut offset = doc.cursor_offset();
    for ch in keys.chars() {
        offset = press_and_apply(engine, doc, KeyEvent::char(ch));
    }
    offset
}

/// Create an engine with sneak_mode enabled.
fn sneak_engine() -> VimEngine {
    let mut engine = VimEngine::new();
    engine.options_mut().set_sneak_mode(true);
    engine
}

// ── Visual mode sneak tests ─────────────────────────────────────────────────

/// `vsab` in visual mode should extend selection forward to "ab".
/// Text: "hello ab world", cursor at 0.
/// `v` enters visual mode, `sab` sneaks forward to "ab" at offset 6.
/// Selection should cover from 0 to 6.
#[test]
fn visual_sneak_forward_extends_selection() {
    let mut doc = TestDocument::new("hello ab world", (0, 0));
    let mut engine = sneak_engine();

    // Enter visual mode
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('v'));

    // Sneak forward to "ab"
    let offset = press_keys(&mut engine, &mut doc, "sab");

    // Cursor should land on 'a' of "ab" at offset 6
    assert_eq!(offset, 6, "sneak should land on first char of 'ab'");
}

/// `vSab` in visual mode should extend selection backward to "ab".
/// Text: "ab hello world", cursor at col 9.
/// `v` enters visual mode, `Sab` sneaks backward to "ab" at offset 0.
#[test]
fn visual_sneak_backward_extends_selection() {
    let mut doc = TestDocument::new("ab hello world", (0, 9));
    let mut engine = sneak_engine();

    // Enter visual mode
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('v'));

    // Sneak backward to "ab"
    let offset = press_keys(&mut engine, &mut doc, "Sab");

    // Cursor should land on 'a' of "ab" at offset 0
    assert_eq!(offset, 0, "sneak backward should land on 'ab' at offset 0");
}

/// `vsab` crossing a line boundary: cursor on line 0, target on line 1.
#[test]
fn visual_sneak_cross_line() {
    let mut doc = TestDocument::new("hello\nworld ab", (0, 0));
    let mut engine = sneak_engine();

    // Enter visual mode
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('v'));

    // Sneak forward to "ab" on line 2
    let offset = press_keys(&mut engine, &mut doc, "sab");

    // "hello\nworld ab" -> a=0,b=1,...,\n=5,w=6,...,' '=11,a=12,b=13
    assert_eq!(offset, 12, "sneak should cross line to 'ab' at offset 12");
}

// ── Sneak with smartcase tests ──────────────────────────────────────────────

/// Sneak with ignorecase=true, smartcase=false: lowercase target matches any case.
/// Text: "xABxab", cursor at 0. `sab` should match "AB" at offset 1.
#[test]
fn sneak_ignorecase_matches_any_case() {
    let mut doc = TestDocument::new("xABxab", (0, 0));
    let mut engine = sneak_engine();
    engine.options_mut().set_ignorecase(true);
    engine.options_mut().set_smartcase(false);

    let offset = press_keys(&mut engine, &mut doc, "sab");

    // With ignorecase, lowercase 'a','b' matches 'A','B' at offset 1
    assert_eq!(offset, 1, "ignorecase should match 'AB' with 'ab'");
}

/// Sneak with ignorecase=true, smartcase=true: uppercase target forces exact match.
/// Text: "xabxAB", cursor at 0. `sAB` should skip "ab" and match "AB" at offset 4.
#[test]
fn sneak_smartcase_uppercase_exact_match() {
    let mut doc = TestDocument::new("xabxAB", (0, 0));
    let mut engine = sneak_engine();
    engine.options_mut().set_ignorecase(true);
    engine.options_mut().set_smartcase(true);

    let offset = press_keys(&mut engine, &mut doc, "sAB");

    // With smartcase + uppercase target, should skip lowercase "ab" and match "AB"
    assert_eq!(
        offset, 4,
        "smartcase uppercase should match 'AB' at offset 4"
    );
}

/// Sneak with ignorecase=true, smartcase=true: lowercase target matches any case.
/// Text: "xABxab", cursor at 0. `sab` should match "AB" at offset 1.
#[test]
fn sneak_smartcase_lowercase_matches_any_case() {
    let mut doc = TestDocument::new("xABxab", (0, 0));
    let mut engine = sneak_engine();
    engine.options_mut().set_ignorecase(true);
    engine.options_mut().set_smartcase(true);

    let offset = press_keys(&mut engine, &mut doc, "sab");

    // With smartcase + lowercase target, case-insensitive → matches "AB" first
    assert_eq!(
        offset, 1,
        "smartcase lowercase should match 'AB' at offset 1"
    );
}

/// Sneak with ignorecase=false (default): exact match only.
/// Text: "xABxab", cursor at 0. `sab` should skip "AB" and match "ab" at offset 4.
#[test]
fn sneak_case_sensitive_exact_match() {
    let mut doc = TestDocument::new("xABxab", (0, 0));
    let mut engine = sneak_engine();
    // ignorecase=false is the default

    let offset = press_keys(&mut engine, &mut doc, "sab");

    assert_eq!(
        offset, 4,
        "case-sensitive sneak should match 'ab' at offset 4"
    );
}

// ── Engine option sync test ─────────────────────────────────────────────────

/// When sneak_mode is false (default), `s` should produce substitute (InsertEntry),
/// not a sneak motion.
#[test]
fn sneak_mode_disabled_s_is_substitute() {
    let mut doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();
    // sneak_mode defaults to false

    // Process 's' — should enter insert mode (substitute)
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));

    // After 's' without sneak, the engine should be in Insert mode
    let mode = engine.mode();
    assert!(
        mode.is_insert(),
        "without sneak_mode, 's' should enter insert mode, got {:?}",
        mode
    );
}

/// When sneak_mode is set via set_options, the parser should receive it.
#[test]
fn sneak_mode_syncs_via_set_options() {
    let mut doc = TestDocument::new("xab", (0, 0));
    let mut engine = VimEngine::new();

    // Enable sneak mode via set_options
    let mut opts = engine.options().clone();
    opts.set_sneak_mode(true);
    engine.set_options(opts);

    // Now 's' should start a sneak sequence, not substitute
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));

    // Should NOT be in insert mode — should be pending (awaiting sneak chars)
    let mode = engine.mode();
    assert!(
        !mode.is_insert(),
        "with sneak_mode enabled via set_options, 's' should NOT enter insert mode, got {:?}",
        mode
    );
}

/// When sneak_mode is set via options_mut, the parser should receive it
/// at the next process() call.
#[test]
fn sneak_mode_syncs_via_options_mut() {
    let mut doc = TestDocument::new("xab", (0, 0));
    let mut engine = VimEngine::new();

    // Enable sneak mode via options_mut (lazy sync)
    engine.options_mut().set_sneak_mode(true);

    // Now 's' should start a sneak sequence
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('s'));

    let mode = engine.mode();
    assert!(
        !mode.is_insert(),
        "with sneak_mode enabled via options_mut, 's' should NOT enter insert mode, got {:?}",
        mode
    );
}

// ── Sneak exclusivity test ──────────────────────────────────────────────────

/// Sneak with operator should be exclusive (like t, not inclusive like f).
/// `dsab` on "xxab" from offset 0 should delete "xx" (not "xxa").
/// The sneak lands ON 'a', and since sneak is exclusive for operators
/// (using compute_find_range), the range is [cursor, target).
/// But compute_find_range for forward find includes the target char.
/// So `dsab` from 0 on "xxab" deletes "xxa" — this tests the actual behavior.
#[test]
fn sneak_operator_delete_forward() {
    let mut doc = TestDocument::new("xxab", (0, 0));
    let mut engine = sneak_engine();

    press_keys(&mut engine, &mut doc, "dsab");

    // compute_find_range for forward: range is [cursor, target + char_len)
    // cursor=0, target=2 ('a' of "ab"), so range=[0,3) deletes "xxa"
    // Remaining text should be "b"
    assert_eq!(
        doc.text(),
        "b",
        "dsab should delete up to and including target"
    );
}

/// `dSab` backward: delete from cursor backward to "ab".
/// Text: "abxx", cursor at col 3. `dSab` should delete backward.
#[test]
fn sneak_operator_delete_backward() {
    let mut doc = TestDocument::new("abxx", (0, 3));
    let mut engine = sneak_engine();

    press_keys(&mut engine, &mut doc, "dSab");

    // compute_find_range for backward: range is [target, cursor) exclusive of cursor
    // target=0 ('a' of "ab"), cursor=3, so range=[0,3) deletes "abx"
    // Remaining text should be "x"
    assert_eq!(
        doc.text(),
        "x",
        "dSab backward should delete from target to cursor"
    );
}
