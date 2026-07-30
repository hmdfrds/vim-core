//! Tests for [`VimEngine::notify_ime_composition`].

use super::super::VimEngine;
use crate::state::InsertState;
use crate::primitives::InsertEntryType;

/// Helper: create an engine in insert mode with an active `InsertState`.
fn engine_in_insert() -> VimEngine {
    let mut engine = VimEngine::new();
    engine.set_mode(crate::primitives::Mode::Insert);
    engine.state.start_insert(InsertState::new(InsertEntryType::BeforeCursor));
    engine
}

// ── Basic composition ────────────────────────────────────────────────────────

/// Composition with replace_len=0 is a pure insertion (no chars removed).
#[test]
fn composition_replace_zero_is_pure_insert() {
    let mut engine = engine_in_insert();
    engine.record_insert_text("hello");

    engine.notify_ime_composition("世界", 0);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(acc, "hello世界");
}

/// Composition replaces pre-composition characters.
#[test]
fn composition_replaces_precomposition_chars() {
    let mut engine = engine_in_insert();
    // Simulate typing "nihon" as pre-composition chars
    engine.record_insert_text("nihon");
    assert_eq!(
        engine.state.insert_state().unwrap().accumulated_text(),
        "nihon",
    );

    // IME commits "日本", replacing 5 pre-composition chars
    engine.notify_ime_composition("日本", 5);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(acc, "日本");
}

/// Mixed chars + composition: text before the composition is preserved.
#[test]
fn mixed_chars_then_composition() {
    let mut engine = engine_in_insert();
    engine.record_insert_text("prefix ");
    engine.record_insert_text("nihon");

    engine.notify_ime_composition("日本", 5);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(acc, "prefix 日本");
}

/// Multiple compositions in sequence.
#[test]
fn multiple_compositions() {
    let mut engine = engine_in_insert();

    // First composition: "nihon" -> "日本"
    engine.record_insert_text("nihon");
    engine.notify_ime_composition("日本", 5);

    // Type a space
    engine.record_insert_text(" ");

    // Second composition: "go" -> "語"
    engine.record_insert_text("go");
    engine.notify_ime_composition("語", 2);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(acc, "日本 語");
}

// ── Edge cases ───────────────────────────────────────────────────────────────

/// Composition when replace_len exceeds accumulated text length pops what is
/// available without panicking.
#[test]
fn composition_replace_len_exceeds_accumulated() {
    let mut engine = engine_in_insert();
    engine.record_insert_text("ab");

    // replace_len=10 but only 2 chars accumulated -- pops both, no panic
    engine.notify_ime_composition("X", 10);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(acc, "X");
}

/// Composition with empty committed text and nonzero replace_len just deletes.
#[test]
fn composition_empty_text_nonzero_replace() {
    let mut engine = engine_in_insert();
    engine.record_insert_text("abc");

    engine.notify_ime_composition("", 2);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(acc, "a");
}

/// Composition with empty text and zero replace_len is a no-op.
#[test]
fn composition_empty_text_zero_replace_is_noop() {
    let mut engine = engine_in_insert();
    engine.record_insert_text("hello");

    engine.notify_ime_composition("", 0);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(acc, "hello");
}

/// Composition when no insert state is active is a silent no-op.
#[test]
fn composition_without_insert_state_is_noop() {
    let mut engine = VimEngine::new();
    // Engine is in Normal mode, no InsertState
    engine.notify_ime_composition("日本", 5);
    // No panic, no crash
    assert!(engine.state.insert_state().is_none());
}

// ── Dot-repeat fidelity ──────────────────────────────────────────────────────

/// After insert exit, `last_inserted_text` contains the composition result
/// (not pre-composition chars), ensuring dot-repeat replays the committed text.
#[test]
fn last_inserted_text_reflects_composition() {
    let mut engine = engine_in_insert();
    engine.record_insert_text("hello ");
    engine.record_insert_text("nihon");
    engine.notify_ime_composition("日本", 5);
    engine.record_insert_text("!");

    // Simulate what handle_insert_exit does: read accumulated_text and store it
    let accumulated = engine
        .state
        .insert_state()
        .unwrap()
        .accumulated_text()
        .to_owned();
    engine.state.store_last_inserted_text(&accumulated);

    assert_eq!(engine.state.last_inserted_text(), "hello 日本!");
}

/// Composition with multibyte pre-composition characters.
#[test]
fn composition_with_multibyte_precomposition() {
    let mut engine = engine_in_insert();
    // Some IMEs produce intermediate multibyte candidates
    engine.record_insert_text("にほん");

    // IME commits the final form, replacing 3 chars (each multibyte)
    engine.notify_ime_composition("日本語", 3);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(acc, "日本語");
}
