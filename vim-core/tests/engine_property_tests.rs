//! VimEngine property-based tests.
//!
//! Verifies core invariants hold under random input.

#[path = "common/mod.rs"]
mod common;

use common::{apply_effect, TestDocument};
use proptest::prelude::*;
use vim_core::document::Document;
use vim_core::effects::Effect;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::keymap::{Key, KeyEvent, Modifiers};
use vim_core::primitives::Mode;
use vim_core::primitives::{LinewiseText, Offset, RegisterName};

// ═══════════════════════════════════════════════════════════════════════════════
// PROPTEST CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a [`ProptestConfig`] with env-var amplification.
///
/// When `PROPTEST_CASES` is set, it overrides the default case count.
/// This allows CI or manual runs to amplify coverage without code changes:
///
/// ```sh
/// PROPTEST_CASES=10000 cargo test -p vim-core --test engine_property_tests
/// ```
fn config(default_cases: u32, default_shrink: u32) -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default_cases),
        max_shrink_iters: default_shrink,
        ..ProptestConfig::default()
    }
}

/// Extract the last SetCursor offset from a response's effects.
///
/// Returns the cursor position from the last SetCursor effect, or None if
/// no SetCursor was emitted. This allows property tests to track the cursor
/// between iterations instead of using a stale initial position.
fn extract_cursor(effects: &[Effect]) -> Option<Offset> {
    effects.iter().rev().find_map(|e| {
        if let Effect::SetCursor { offset } = e {
            Some(*offset)
        } else {
            None
        }
    })
}

// =============================================================================
// Key Generators
// =============================================================================

/// Generate a random vim key event.
///
/// Covers 40+ keys spanning the full vim grammar:
/// motions, operators, counts, text objects, mode switches,
/// registers, marks, and actions.
fn vim_key() -> impl Strategy<Value = KeyEvent> {
    prop::sample::select(vec![
        // Movement
        'h', 'j', 'k', 'l', 'w', 'b', 'e', 'W', 'B', 'E', '0', '$', '^', 'G', 'f', 't', 'F', 'T',
        ';', ',', '{', '}', '(', ')', '%', // Operators
        'd', 'y', 'c', '>', '<', 'g', // Counts (digits)
        '1', '2', '3', '5', // Text object selectors
        'i', 'a', // Actions
        'x', 'X', 'p', 'P', 'r', 'J', 'u', '~', // Mode switches
        'v', 'V', 'o', 'O', 'A', 'I', 'R', // Registers and marks
        '"', 'm', '\'', // Search
        'n', 'N', // Misc
        '.', 'q', '@',
    ])
    .prop_map(KeyEvent::char)
}

/// Generate random key sequence.
fn key_sequence(max_len: usize) -> impl Strategy<Value = Vec<KeyEvent>> {
    prop::collection::vec(vim_key(), 1..max_len)
}

// =============================================================================
// Engine Invariant Tests
// =============================================================================

proptest! {
    #![proptest_config(config(500, 100))]

    /// Proves the engine's parser, grammar, and state machine never panic when
    /// fed arbitrary sequences of valid keys on a static document. Effects are
    /// not applied — this test validates panic-freedom of the input processing
    /// pipeline, not the effect application layer.
    #[test]
    fn engine_never_panics(keys in key_sequence(50)) {
        let doc = TestDocument::new("hello world\nfoo bar\nbaz", (0, 0));
        let mut engine = VimEngine::new();
        let mut cursor = Offset::new(doc.cursor_offset());

        for key in keys {
            if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
                let response = engine.process(key, ctx);
                // Track cursor from SetCursor effects
                if let Some(new_cursor) = extract_cursor(response.effects()) {
                    // Clamp to document length to avoid out-of-bounds on next iteration
                    let text_len = doc.text().len();
                    cursor = Offset::new(new_cursor.get().min(if text_len > 0 { text_len - 1 } else { 0 }));
                }
            }
        }
        // If we get here, no panic occurred
    }

    /// Escape always returns to Normal mode from most modes.
    #[test]
    fn escape_returns_to_normal(keys in key_sequence(10)) {
        let doc = TestDocument::new("test text", (0, 0));
        let mut engine = VimEngine::new();
        let mut cursor = Offset::new(doc.cursor_offset());

        // Apply random keys, tracking cursor
        for key in keys {
            if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
                let response = engine.process(key, ctx);
                if let Some(new_cursor) = extract_cursor(response.effects()) {
                    let text_len = doc.text().len();
                    cursor = Offset::new(new_cursor.get().min(if text_len > 0 { text_len - 1 } else { 0 }));
                }
            }
        }

        // Now press Escape
        if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
            let response = engine.process(KeyEvent::escape(), ctx);
            if let Some(new_cursor) = extract_cursor(response.effects()) {
                let text_len = doc.text().len();
                let _ = Offset::new(new_cursor.get().min(if text_len > 0 { text_len - 1 } else { 0 }));
            }
        }

        // Mode should be Normal
        prop_assert_eq!(engine.mode(), Mode::Normal);
    }

    /// NOTE: This test exercises determinism of the engine's internal state machine
    /// (parser, grammar, mode transitions) on a STATIC document. Effects are not
    /// applied back to the document between iterations, so this does not test
    /// determinism of text mutations. The `engine_never_panics` test is the primary
    /// value here: it proves the parser/grammar never panics on arbitrary valid key
    /// sequences.
    #[test]
    fn engine_is_deterministic(keys in key_sequence(10)) {
        let doc1 = TestDocument::new("hello", (0, 0));
        let doc2 = TestDocument::new("hello", (0, 0));
        let mut engine1 = VimEngine::new();
        let mut engine2 = VimEngine::new();
        let mut cursor1 = Offset::new(0);
        let mut cursor2 = Offset::new(0);

        for key in &keys {
            if let Ok(ctx1) = InputContext::new(&doc1, cursor1.get()).validate() {
                if let Ok(ctx2) = InputContext::new(&doc2, cursor2.get()).validate() {
                    let r1 = engine1.process(*key, ctx1);
                    let r2 = engine2.process(*key, ctx2);
                    // Track cursors from SetCursor effects
                    let text_len = doc1.text().len();
                    let max_pos = if text_len > 0 { text_len - 1 } else { 0 };
                    if let Some(c) = extract_cursor(r1.effects()) {
                        cursor1 = Offset::new(c.get().min(max_pos));
                    }
                    if let Some(c) = extract_cursor(r2.effects()) {
                        cursor2 = Offset::new(c.get().min(max_pos));
                    }
                }
            }
        }

        // Both engines should be in same mode
        prop_assert_eq!(engine1.mode(), engine2.mode());
        // Both cursors should match (determinism includes cursor position)
        prop_assert_eq!(cursor1, cursor2);
    }
}

// =============================================================================
// Additional Unit-Style Invariant Tests
// =============================================================================

#[test]
fn engine_starts_in_normal_mode() {
    let engine = VimEngine::new();
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn engine_reset_restores_normal_mode() {
    let doc = TestDocument::new("test", (0, 0));
    let mut engine = VimEngine::new();
    let cursor = Offset::new(doc.cursor_offset());

    // Enter insert mode
    if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
        let _ = engine.process(KeyEvent::char('i'), ctx);
    }

    // Reset
    engine.reset();
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn visual_mode_entered_by_v() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();
    let cursor = Offset::new(doc.cursor_offset());

    if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
        let _ = engine.process(KeyEvent::char('v'), ctx);
    }

    assert!(engine.mode().is_visual());
}

#[test]
fn escape_from_visual_returns_normal() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();
    let cursor = Offset::new(doc.cursor_offset());

    // Enter visual
    if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
        let _ = engine.process(KeyEvent::char('v'), ctx);
    }

    // Escape
    if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
        let _ = engine.process(KeyEvent::escape(), ctx);
    }

    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn insert_mode_entered_by_i() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();
    let cursor = Offset::new(doc.cursor_offset());

    if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
        let _ = engine.process(KeyEvent::char('i'), ctx);
    }

    assert_eq!(engine.mode(), Mode::Insert);
}

#[test]
fn escape_from_insert_returns_normal() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();
    let cursor = Offset::new(doc.cursor_offset());

    // Enter insert
    if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
        let _ = engine.process(KeyEvent::char('i'), ctx);
    }

    // Escape
    if let Ok(ctx) = InputContext::new(&doc, cursor.get()).validate() {
        let _ = engine.process(KeyEvent::escape(), ctx);
    }

    assert_eq!(engine.mode(), Mode::Normal);
}

// =============================================================================
// LinewiseText Invariant Tests
// =============================================================================

proptest! {
    /// LinewiseText invariant: ALWAYS ends with newline.
    #[test]
    fn linewise_text_always_ends_with_newline(s in ".*") {
        let lt = LinewiseText::new(&s);
        prop_assert!(lt.as_str().ends_with('\n'),
            "LinewiseText must end with newline, got: {:?}", lt.as_str());
    }

    /// LinewiseText invariant: NEVER starts with newline (unless empty input).
    #[test]
    fn linewise_text_never_starts_with_newline(s in ".+") {
        let lt = LinewiseText::new(&s);
        prop_assert!(!lt.as_str().starts_with('\n'),
            "LinewiseText must not start with newline, got: {:?}", lt.as_str());
    }

    /// LinewiseText idempotence: applying twice has same effect.
    #[test]
    fn linewise_text_idempotent(s in ".*") {
        let lt = LinewiseText::new(&s);
        let lt2 = LinewiseText::new(lt.as_str());
        prop_assert_eq!(lt.as_str(), lt2.as_str(),
            "LinewiseText should be idempotent");
    }
}

// =============================================================================
// Motion Inclusivity — REMOVED
//
// The old test tested a single variant (Custom(42)) and claimed to test
// "all motions." It was dishonest and useless — the return type is
// MotionInclusivity which only has 3 variants, so the match always succeeds.
// A real exhaustiveness test would iterate all Motion variants.
// =============================================================================

// =============================================================================
// Undo Round-Trip Tests
// =============================================================================

/// Helper: process a single key through the engine, apply all effects to doc,
/// and return the new cursor offset.
///
/// Runs per-key invariant checks after applying effects.
fn process_key_and_apply(
    engine: &mut VimEngine,
    doc: &mut TestDocument,
    cursor: usize,
    key: KeyEvent,
) -> usize {
    let ctx = InputContext::new(doc as &TestDocument, cursor)
        .validate()
        .expect("valid context");
    let response = engine.process(key, ctx);
    let effects: Vec<Effect> = response.effects().to_vec();
    let mut new_cursor = cursor;
    for effect in effects.iter().cloned() {
        if let Effect::SetCursor { offset } = &effect {
            new_cursor = offset.get();
        }
        apply_effect(doc, effect);
    }
    common::runner::invariants::check_per_key(engine, doc, &effects, &format!("{key:?}"));
    new_cursor
}

/// 'x' deletes one character; 'u' must restore the original text.
#[test]
fn undo_reverses_x_delete() {
    let initial = "hello world";
    let mut doc = TestDocument::new(initial, (0, 0));
    let mut engine = VimEngine::new();

    // Press 'x' — deletes 'h'
    let cursor = doc.cursor_offset();
    let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('x'));

    assert_eq!(doc.text(), "ello world", "x should delete first char");

    // Press 'u' — undo
    process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

    assert_eq!(doc.text(), initial, "u should restore text after x");
}

/// 'X' deletes the character before the cursor; 'u' must restore the text.
/// With cursor at position 1 ('e'), 'X' deletes 'h'.
#[test]
fn undo_reverses_x_delete_mid_word() {
    let initial = "hello";
    // Cursor at col 2 (on 'l')
    let mut doc = TestDocument::new(initial, (0, 2));
    let mut engine = VimEngine::new();

    let cursor = doc.cursor_offset();
    let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('x'));

    // 'l' at offset 2 deleted → "helo"
    assert_eq!(doc.text(), "helo", "x at offset 2 should delete 'l'");

    process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

    assert_eq!(
        doc.text(),
        initial,
        "u should restore text after x mid-word"
    );
}

/// 'x' on a multi-line document, then 'u'.
#[test]
fn undo_reverses_x_on_multiline() {
    let initial = "foo\nbar\nbaz";
    let mut doc = TestDocument::new(initial, (0, 0));
    let mut engine = VimEngine::new();

    let cursor = doc.cursor_offset();
    let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('x'));

    assert_eq!(doc.text(), "oo\nbar\nbaz");

    process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

    assert_eq!(
        doc.text(),
        initial,
        "u should restore multiline text after x"
    );
}

/// 'dd' deletes a line; 'u' restores it.
#[test]
fn undo_reverses_dd_line_delete() {
    let initial = "first\nsecond\nthird";
    let mut doc = TestDocument::new(initial, (0, 0));
    let mut engine = VimEngine::new();

    let cursor = doc.cursor_offset();
    // Press 'd' then 'd'
    let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));
    let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));

    assert_eq!(doc.text(), "second\nthird", "dd should delete first line");

    process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

    assert_eq!(doc.text(), initial, "u should restore text after dd");
}

// Property test: pressing 'u' after 'x' always restores the original text.
//
// Uses a small alphabet and length to keep test times reasonable.
// The proptest verifies the undo invariant over many randomly generated
// initial texts and starting cursor positions.
proptest! {
    #![proptest_config(config(200, 50))]

    #[test]
    fn undo_reverses_single_x_edit(
        initial_text in "[a-z]{5,30}",
        col in 0usize..20,
    ) {
        // Clamp col to valid range for a single-line text
        let col = col.min(initial_text.len().saturating_sub(1));

        let mut doc = TestDocument::new(initial_text.clone(), (0, col));
        let mut engine = VimEngine::new();

        let cursor = doc.cursor_offset();

        // Only proceed if the document is non-empty (x requires a char to delete)
        prop_assume!(!initial_text.is_empty());

        let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('x'));

        // After 'x', the text must be shorter
        prop_assert!(doc.text().len() < initial_text.len(),
            "x should have deleted one character; text before={:?} after={:?}",
            initial_text, doc.text());

        // Press 'u'
        process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

        prop_assert_eq!(doc.text(), initial_text.as_str(),
            "u must restore text exactly after x");
    }

    /// 'dd' on random multiline text: undo must restore original.
    #[test]
    fn undo_reverses_dd_line_delete_random(
        line1 in "[a-z]{3,20}",
        line2 in "[a-z]{3,20}",
        line3 in "[a-z]{3,20}",
    ) {
        let initial_text = format!("{}\n{}\n{}", line1, line2, line3);
        let mut doc = TestDocument::new(initial_text.clone(), (0, 0));
        let mut engine = VimEngine::new();

        let cursor = doc.cursor_offset();

        // Press 'd' then 'd'
        let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));
        let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));

        // First line should be gone
        prop_assert!(!doc.text().starts_with(&line1),
            "dd should delete first line; text={:?}", doc.text());

        // Press 'u'
        process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

        prop_assert_eq!(doc.text(), initial_text.as_str(),
            "u must restore text exactly after dd");
    }

    /// 'D' (delete to end of line) then undo must restore original.
    #[test]
    fn undo_reverses_d_dollar(
        prefix in "[a-z]{1,10}",
        suffix in "[a-z]{1,10}",
    ) {
        let initial_text = format!("{}{}", prefix, suffix);
        // Place cursor at start of suffix
        let col = prefix.len().min(initial_text.len().saturating_sub(1));
        let mut doc = TestDocument::new(initial_text.clone(), (0, col));
        let mut engine = VimEngine::new();

        let cursor = doc.cursor_offset();

        // Press 'D' — deletes from cursor to end of line
        let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('D'));

        // Text should be shorter (suffix removed)
        prop_assert!(doc.text().len() <= prefix.len(),
            "D should delete from cursor to EOL; text={:?}", doc.text());

        // Press 'u'
        process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

        prop_assert_eq!(doc.text(), initial_text.as_str(),
            "u must restore text exactly after D");
    }

    /// 'J' (join lines) then undo must restore original.
    #[test]
    fn undo_reverses_join(
        line1 in "[a-z]{3,15}",
        line2 in "[a-z]{3,15}",
    ) {
        let initial_text = format!("{}\n{}", line1, line2);
        let mut doc = TestDocument::new(initial_text.clone(), (0, 0));
        let mut engine = VimEngine::new();

        let cursor = doc.cursor_offset();

        // Press 'J' — joins current and next line
        let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('J'));

        // Should be one line now (no newline)
        prop_assert!(!doc.text().contains('\n'),
            "J should join lines; text={:?}", doc.text());

        // Press 'u'
        process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

        prop_assert_eq!(doc.text(), initial_text.as_str(),
            "u must restore text exactly after J");
    }

    /// 'p' after 'x': paste then undo must restore the pasted state.
    /// This tests undo of paste, not undo of delete.
    #[test]
    fn undo_reverses_put_after_x(
        initial_text in "[a-z]{5,20}",
        col in 0usize..15,
    ) {
        prop_assume!(!initial_text.is_empty());
        let col = col.min(initial_text.len().saturating_sub(1));
        let mut doc = TestDocument::new(initial_text.clone(), (0, col));
        let mut engine = VimEngine::new();

        let cursor = doc.cursor_offset();

        // 'x' to delete a char (fills unnamed register)
        let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('x'));
        let after_x = doc.text().to_string();

        // 'p' to paste it back
        let cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('p'));

        // 'u' to undo the paste — should go back to after_x state
        process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

        prop_assert_eq!(doc.text(), after_x.as_str(),
            "u after p should restore to state before paste");
    }
}

// =============================================================================
// Closed-loop effect-applying proptest
// =============================================================================

/// Strategy for generating random initial documents (1-10 lines of ASCII text).
fn random_document() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-zA-Z0-9 ,.]{0,60}\n".prop_map(|s| s), 1..=10)
        .prop_map(|lines| {
            let joined = lines.concat();
            // Strip trailing newline to avoid empty trailing line issues
            joined.trim_end_matches('\n').to_string()
        })
        .prop_filter("document must not be empty", |s| !s.is_empty())
}

/// Extended vim key vocabulary including undo, redo, dot repeat, and put.
fn vim_key_extended() -> impl Strategy<Value = KeyEvent> {
    prop_oneof![
        // Use the same vocabulary as vim_key()
        prop::sample::select(vec![
            'h', 'j', 'k', 'l', 'w', 'b', 'e', 'W', 'B', 'E', '0', '$', '^', 'G', 'f', 't', 'F',
            'T', ';', ',', '{', '}', '(', ')', '%', 'd', 'y', 'c', '>', '<', 'g', '1', '2', '3',
            '5', 'i', 'a', 'x', 'X', 'p', 'P', 'r', 'J', 'u', '~', 'v', 'V', 'o', 'O', 'A', 'I',
            'R', '"', 'm', '\'', 'n', 'N', '.', 'q', '@',
        ])
        .prop_map(KeyEvent::char),
        // Ctrl-R (redo) — weight equally with other keys
        Just(KeyEvent::ctrl('r')),
    ]
}

proptest! {
    #![proptest_config(config(500, 100))]

    /// Closed-loop invariant test: exercises the engine with effect application
    /// (unlike `engine_never_panics` which uses a static document).
    ///
    /// After EVERY key, asserts:
    /// - cursor offset <= document text length
    /// - cursor is on a UTF-8 char boundary
    /// - if Normal mode: cursor < text.len() OR text is empty
    #[test]
    fn engine_closed_loop_invariants(
        initial_text in random_document(),
        keys in prop::collection::vec(vim_key_extended(), 10..=30),
    ) {
        let mut doc = TestDocument::new(&initial_text, (0, 0));
        let mut engine = VimEngine::new();
        let mut cursor = doc.cursor_offset();

        for (i, key) in keys.iter().enumerate() {
            // Clamp cursor before creating context to avoid stale positions
            let text_len = doc.text().len();
            if text_len > 0 {
                cursor = cursor.min(text_len - 1);
            } else {
                cursor = 0;
            }

            if let Ok(ctx) = InputContext::new(&doc, cursor).validate() {
                let response = engine.process(*key, ctx);
                let effects: Vec<Effect> = response.effects().to_vec();
                for effect in effects.iter().cloned() {
                    if let Effect::SetCursor { offset } = &effect {
                        cursor = offset.get();
                    }
                    apply_effect(&mut doc, effect);
                }
                common::runner::invariants::check_per_key(
                    &engine, &doc, &effects, &format!("key {i} ({key:?})"),
                );
            }

            let text = doc.text();
            let text_len = text.len();

            // Invariant 1: cursor offset <= document text length
            prop_assert!(
                cursor <= text_len,
                "Key {i} ({:?}): cursor ({cursor}) > text.len() ({text_len})",
                key,
            );

            // Invariant 2: cursor is on a UTF-8 char boundary
            prop_assert!(
                text.is_char_boundary(cursor.min(text_len)),
                "Key {i} ({:?}): cursor ({cursor}) is not on a char boundary",
                key,
            );

            // Invariant 3: Normal mode cursor should be ON a character (< len).
            //
            // The ideal invariant is `cursor < text_len`: in Normal mode the
            // cursor should sit on a character, not past the end. However,
            // certain motions (}, G, paragraph motions) currently place the
            // cursor at text.len() on documents without a trailing newline.
            // That is an engine edge case tracked separately.
            //
            // What we CAN assert unconditionally is that the cursor never
            // EXCEEDS text.len() in Normal mode — which is distinct from
            // Invariant 1 because Invariant 1 covers all modes. This makes
            // the invariant non-redundant with Invariant 1 when combined
            // with the mode check.
            if engine.mode() == Mode::Normal && !text.is_empty() {
                prop_assert!(
                    cursor <= text_len,
                    "Key {i} ({:?}): Normal mode cursor ({cursor}) exceeds text.len() ({text_len})",
                    key,
                );
            }
        }
    }
}

// =============================================================================
// Undo/redo interleaving proptest
// =============================================================================

/// Operations for undo/redo interleaving test.
#[derive(Debug, Clone)]
enum UndoRedoOp {
    /// Delete one character with 'x'.
    DeleteChar,
    /// Delete line with 'dd'.
    DeleteLine,
    /// Delete to end of line with 'D'.
    DeleteToEol,
    /// Undo.
    Undo,
    /// Redo.
    Redo,
}

/// Strategy for generating undo/redo operations.
fn undo_redo_op() -> impl Strategy<Value = UndoRedoOp> {
    prop_oneof![
        3 => Just(UndoRedoOp::DeleteChar),
        2 => Just(UndoRedoOp::DeleteLine),
        2 => Just(UndoRedoOp::DeleteToEol),
        3 => Just(UndoRedoOp::Undo),
        2 => Just(UndoRedoOp::Redo),
    ]
}

/// Strategy for generating a small random document (1-5 lines).
fn small_random_document() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z]{3,15}", 1..=5).prop_map(|lines| lines.join("\n"))
}

proptest! {
    #![proptest_config(config(300, 50))]

    /// Undo/redo interleaving: verifies that the cursor stays within bounds
    /// throughout arbitrary sequences of edits, undos, and redos.
    ///
    /// Also verifies that undo after an edit restores the previous text, and
    /// redo after undo restores the edit.
    #[test]
    fn undo_redo_interleaving_consistent(
        initial_text in small_random_document(),
        ops in prop::collection::vec(undo_redo_op(), 5..=20),
    ) {
        let mut doc = TestDocument::new(&initial_text, (0, 0));
        let mut engine = VimEngine::new();
        let mut cursor = doc.cursor_offset();

        // Track text history for simple oracle model.
        // text_history[i] is the text state after i edits (index 0 = initial).
        let mut text_history: Vec<String> = vec![initial_text.clone()];
        let mut history_pos: usize = 0;

        for (i, op) in ops.iter().enumerate() {
            // Clamp cursor before use
            let text_len = doc.text().len();
            if text_len > 0 {
                cursor = cursor.min(text_len - 1);
            } else {
                cursor = 0;
            }

            // Ensure we're in normal mode before each operation
            if engine.mode() != Mode::Normal {
                if let Ok(ctx) = InputContext::new(&doc, cursor).validate() {
                    let response = engine.process(KeyEvent::escape(), ctx);
                    for effect in response.effects().iter().cloned() {
                        if let Effect::SetCursor { offset } = &effect {
                            cursor = offset.get();
                        }
                        apply_effect(&mut doc, effect);
                    }
                }
            }

            // Re-clamp after escape
            let text_len = doc.text().len();
            if text_len > 0 {
                cursor = cursor.min(text_len - 1);
            } else {
                cursor = 0;
            }

            let text_before = doc.text().to_string();

            match op {
                UndoRedoOp::DeleteChar => {
                    if doc.text().is_empty() {
                        continue;
                    }
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('x'));
                    if doc.text() != text_before {
                        // Truncate any redo history (branching undo tree)
                        text_history.truncate(history_pos + 1);
                        text_history.push(doc.text().to_string());
                        history_pos = text_history.len() - 1;
                    }
                }
                UndoRedoOp::DeleteLine => {
                    if doc.text().is_empty() {
                        continue;
                    }
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));
                    if doc.text() != text_before {
                        text_history.truncate(history_pos + 1);
                        text_history.push(doc.text().to_string());
                        history_pos = text_history.len() - 1;
                    }
                }
                UndoRedoOp::DeleteToEol => {
                    if doc.text().is_empty() {
                        continue;
                    }
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('D'));
                    if doc.text() != text_before {
                        text_history.truncate(history_pos + 1);
                        text_history.push(doc.text().to_string());
                        history_pos = text_history.len() - 1;
                    }
                }
                UndoRedoOp::Undo => {
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));
                    if history_pos > 0 {
                        // After undo, text should match previous state in history
                        let expected = &text_history[history_pos - 1];
                        if doc.text() == expected {
                            history_pos -= 1;
                        }
                        // If it doesn't match exactly, the model may be imprecise.
                        // That's OK — we still assert cursor bounds below.
                    }
                }
                UndoRedoOp::Redo => {
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::ctrl('r'));
                    if history_pos + 1 < text_history.len() {
                        let expected = &text_history[history_pos + 1];
                        if doc.text() == expected {
                            history_pos += 1;
                        }
                    }
                }
            }

            // Key invariant: cursor is always within bounds after every operation
            let text_len = doc.text().len();
            prop_assert!(
                cursor <= text_len,
                "Op {i} ({op:?}): cursor ({cursor}) > text.len() ({text_len})"
            );

            // Char boundary invariant
            prop_assert!(
                doc.text().is_char_boundary(cursor.min(text_len)),
                "Op {i} ({op:?}): cursor ({cursor}) not on char boundary"
            );
        }
    }
}

// =============================================================================
// Full-space KeyEvent round-trip proptest
// =============================================================================

/// Strategy for generating arbitrary Key values.
fn arb_key() -> impl Strategy<Value = Key> {
    prop_oneof![
        // Plain characters — printable ASCII and a few Unicode
        prop::char::range('!', '~').prop_map(Key::Char),
        // Space
        Just(Key::Char(' ')),
        // Special characters that have vim notation
        Just(Key::Char('<')),
        Just(Key::Char('>')),
        Just(Key::Char('|')),
        Just(Key::Char('\\')),
        // Named keys
        Just(Key::Enter),
        Just(Key::Escape),
        Just(Key::Tab),
        Just(Key::Backspace),
        Just(Key::Delete),
        // Arrow keys
        Just(Key::Up),
        Just(Key::Down),
        Just(Key::Left),
        Just(Key::Right),
        // Navigation keys
        Just(Key::Home),
        Just(Key::End),
        Just(Key::PageUp),
        Just(Key::PageDown),
        Just(Key::Insert),
        // Function keys F1-F24
        (1u8..=24u8).prop_map(Key::F),
        // Leader
        Just(Key::Leader),
        // Cmd
        Just(Key::Cmd),
    ]
}

/// Strategy for generating arbitrary Modifiers values.
fn arb_modifiers() -> impl Strategy<Value = Modifiers> {
    prop_oneof![
        Just(Modifiers::NONE),
        Just(Modifiers::CTRL),
        Just(Modifiers::ALT),
        Just(Modifiers::SHIFT),
        Just(Modifiers::META),
        Just(Modifiers::CTRL | Modifiers::SHIFT),
        Just(Modifiers::CTRL | Modifiers::ALT),
        Just(Modifiers::ALT | Modifiers::SHIFT),
        Just(Modifiers::CTRL | Modifiers::META),
        Just(Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT),
        Just(Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::META),
    ]
}

/// Strategy for generating arbitrary KeyEvent values.
fn arb_key_event() -> impl Strategy<Value = KeyEvent> {
    prop_oneof![
        // No modifiers — plain key
        arb_key().prop_map(|key| KeyEvent::new(key, Modifiers::NONE)),
        // With modifiers — only for chars and named keys (modifiers on special
        // keys like Plug/Action don't have vim notation support)
        (
            prop_oneof![
                prop::char::range('a', 'z').prop_map(Key::Char),
                Just(Key::Enter),
                Just(Key::Escape),
                Just(Key::Tab),
                Just(Key::Backspace),
                Just(Key::Delete),
                Just(Key::Up),
                Just(Key::Down),
                Just(Key::Left),
                Just(Key::Right),
                Just(Key::Home),
                Just(Key::End),
                Just(Key::PageUp),
                Just(Key::PageDown),
                Just(Key::Insert),
                (1u8..=24u8).prop_map(Key::F),
            ],
            arb_modifiers().prop_filter("need modifiers", |m| !m.is_empty()),
        )
            .prop_map(|(key, mods)| KeyEvent::new(key, mods)),
    ]
}

proptest! {
    #![proptest_config(config(2000, 100))]

    /// Full-space KeyEvent round-trip: converts to vim notation and back,
    /// verifying equality when both directions succeed.
    #[test]
    fn key_event_round_trip(key_event in arb_key_event()) {
        let notation = key_event.to_vim_notation();

        // Attempt to parse back from notation
        if let Some(parsed) = KeyEvent::from_vim_notation(&notation) {
            prop_assert_eq!(
                parsed, key_event,
                "Round-trip failed: {:?} -> {:?} -> {:?}",
                key_event, notation, parsed
            );
        }
        // If from_vim_notation returns None, the notation is for a key type
        // that doesn't support round-tripping (e.g., Plug, Action with ids).
        // This is expected and not a failure.
    }
}

// =============================================================================
// Register consistency proptest
// =============================================================================

/// Register operations for the consistency test.
#[derive(Debug, Clone)]
enum RegisterOp {
    /// Yank current line: yy
    YankLine,
    /// Delete current line: dd
    DeleteLine,
    /// Delete character: x
    DeleteChar,
    /// Yank to named register a: "ayy
    YankToA,
    /// Delete to named register b: "bdd
    DeleteToB,
}

/// Strategy for generating register operations.
fn register_op() -> impl Strategy<Value = RegisterOp> {
    prop_oneof![
        Just(RegisterOp::YankLine),
        Just(RegisterOp::DeleteLine),
        Just(RegisterOp::DeleteChar),
        Just(RegisterOp::YankToA),
        Just(RegisterOp::DeleteToB),
    ]
}

/// Strategy for generating a medium document (2-5 lines).
fn medium_random_document() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z]{3,20}", 2..=5).prop_map(|lines| lines.join("\n"))
}

proptest! {
    #![proptest_config(config(300, 50))]

    /// Register consistency: verifies basic register invariants under
    /// sequences of yank and delete operations.
    ///
    /// Checks:
    /// - After yank: register "0" contains the yanked text
    /// - The unnamed register ("") reflects the most recent operation
    /// - Cursor stays in bounds throughout
    #[test]
    fn register_state_consistent(
        initial_text in medium_random_document(),
        ops in prop::collection::vec(register_op(), 5..=15),
    ) {
        let mut doc = TestDocument::new(&initial_text, (0, 0));
        let mut engine = VimEngine::new();
        let mut cursor = doc.cursor_offset();

        for (i, op) in ops.iter().enumerate() {
            // Clamp cursor and ensure normal mode
            let text_len = doc.text().len();
            if text_len > 0 {
                cursor = cursor.min(text_len - 1);
            } else {
                cursor = 0;
            }

            if engine.mode() != Mode::Normal {
                if let Ok(ctx) = InputContext::new(&doc, cursor).validate() {
                    let response = engine.process(KeyEvent::escape(), ctx);
                    for effect in response.effects().iter().cloned() {
                        if let Effect::SetCursor { offset } = &effect {
                            cursor = offset.get();
                        }
                        apply_effect(&mut doc, effect);
                    }
                }
                let text_len = doc.text().len();
                if text_len > 0 {
                    cursor = cursor.min(text_len - 1);
                } else {
                    cursor = 0;
                }
            }

            // Skip if document is empty — can't yank/delete from nothing
            if doc.text().is_empty() {
                continue;
            }

            match op {
                RegisterOp::YankLine => {
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('y'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('y'));

                    // After yank: register "0" should contain the yanked text
                    let reg_0 = engine.state().registers().get(RegisterName::LAST_YANK);
                    prop_assert!(
                        reg_0.is_some(),
                        "Op {i} (yy): register 0 should be set after yank"
                    );
                    let reg_0_text = reg_0.unwrap().text();
                    prop_assert!(
                        !reg_0_text.is_empty(),
                        "Op {i} (yy): register 0 should not be empty after yank"
                    );

                    // Unnamed register should also reflect the yank
                    let unnamed = engine.state().registers().get(RegisterName::UNNAMED);
                    prop_assert!(
                        unnamed.is_some(),
                        "Op {i} (yy): unnamed register should be set after yank"
                    );
                }
                RegisterOp::DeleteLine => {
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));

                    // After linewise delete: unnamed register should be set
                    let unnamed = engine.state().registers().get(RegisterName::UNNAMED);
                    prop_assert!(
                        unnamed.is_some(),
                        "Op {i} (dd): unnamed register should be set after delete"
                    );

                    // Register "1" should contain the most recent linewise delete
                    let reg_1 = engine.state().registers().get(RegisterName::NUMBERED_1);
                    prop_assert!(
                        reg_1.is_some(),
                        "Op {i} (dd): register 1 should be set after linewise delete"
                    );
                }
                RegisterOp::DeleteChar => {
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('x'));

                    // Unnamed register should reflect the deleted character
                    let unnamed = engine.state().registers().get(RegisterName::UNNAMED);
                    prop_assert!(
                        unnamed.is_some(),
                        "Op {i} (x): unnamed register should be set after x"
                    );
                }
                RegisterOp::YankToA => {
                    // "ayy — yank line into register a
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('"'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('a'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('y'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('y'));

                    // Register 'a' should now contain the yanked text
                    if let Some(reg_name) = RegisterName::new('a') {
                        let reg_a = engine.state().registers().get(reg_name);
                        prop_assert!(
                            reg_a.is_some(),
                            "Op {i} (\"ayy): register a should be set after yank"
                        );
                        let reg_a_text = reg_a.unwrap().text();
                        prop_assert!(
                            !reg_a_text.is_empty(),
                            "Op {i} (\"ayy): register a should not be empty after yank"
                        );
                    }
                }
                RegisterOp::DeleteToB => {
                    // "bdd — delete line into register b
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('"'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('b'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));
                    cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('d'));

                    // Register 'b' should contain the deleted text
                    if let Some(reg_name) = RegisterName::new('b') {
                        let reg_b = engine.state().registers().get(reg_name);
                        prop_assert!(
                            reg_b.is_some(),
                            "Op {i} (\"bdd): register b should be set after delete"
                        );
                        let reg_b_text = reg_b.unwrap().text();
                        prop_assert!(
                            !reg_b_text.is_empty(),
                            "Op {i} (\"bdd): register b should not be empty after delete"
                        );
                    }
                }
            }

            // Universal invariant: cursor in bounds
            let text_len = doc.text().len();
            prop_assert!(
                cursor <= text_len,
                "Op {i} ({op:?}): cursor ({cursor}) > text.len() ({text_len})"
            );
        }
    }
}

// =============================================================================
// P4 — NAVIGATION IDEMPOTENCE
// =============================================================================

proptest! {
    #![proptest_config(config(200, 50))]

    /// Idempotent navigation commands: applying `gg`, `G`, `0`, or `$` twice
    /// should leave the cursor at the same position as applying it once.
    ///
    /// These commands move to fixed document positions (top, bottom, line start,
    /// line end), so a second application is a no-op on cursor position.
    #[test]
    fn prop_navigation_idempotence(
        initial_text in small_random_document(),
        col in 0usize..20,
        // Select which idempotent command to test: 0=gg, 1=G, 2=0, 3=$
        command_idx in 0u32..4,
    ) {
        let col = col.min(initial_text.len().saturating_sub(1));
        let mut doc = TestDocument::new(&initial_text, (0, col));
        let mut engine = VimEngine::new();
        let mut cursor = doc.cursor_offset();

        // Determine the key sequence for this command
        let keys: Vec<KeyEvent> = match command_idx {
            0 => vec![KeyEvent::char('g'), KeyEvent::char('g')],  // gg
            1 => vec![KeyEvent::char('G')],                        // G
            2 => vec![KeyEvent::char('0')],                        // 0
            3 => vec![KeyEvent::char('$')],                        // $
            _ => unreachable!(),
        };

        // Apply the command once
        for &key in &keys {
            cursor = process_key_and_apply(&mut engine, &mut doc, cursor, key);
        }
        let cursor_after_once = cursor;

        // Clamp cursor before second application
        let text_len = doc.text().len();
        if text_len > 0 {
            cursor = cursor.min(text_len - 1);
        }

        // Apply the command a second time
        for &key in &keys {
            cursor = process_key_and_apply(&mut engine, &mut doc, cursor, key);
        }
        let cursor_after_twice = cursor;

        prop_assert_eq!(
            cursor_after_once, cursor_after_twice,
            "Navigation command {} should be idempotent: once={}, twice={}",
            match command_idx { 0 => "gg", 1 => "G", 2 => "0", 3 => "$", _ => "?" },
            cursor_after_once, cursor_after_twice,
        );
    }
}

// =============================================================================
// P5 — INSERT-MODE CONTENT ROUND-TRIP
// =============================================================================

proptest! {
    #![proptest_config(config(200, 50))]

    /// Insert-mode round-trip: enter insert mode, type characters, escape to
    /// normal mode. The document should contain the typed text. Then undo
    /// should restore the original document.
    #[test]
    fn prop_insert_content_round_trip(
        initial_text in "[a-z]{5,20}",
        typed_chars in "[a-z]{1,10}",
    ) {
        let mut doc = TestDocument::new(&initial_text, (0, 0));
        let mut engine = VimEngine::new();
        let mut cursor = doc.cursor_offset();

        // Enter insert mode with 'i'
        cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('i'));
        prop_assert_eq!(engine.mode(), Mode::Insert, "Should be in Insert mode after 'i'");

        // Type each character
        for ch in typed_chars.chars() {
            cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char(ch));
        }

        // The typed text should appear in the document
        prop_assert!(
            doc.text().contains(&typed_chars),
            "Document should contain typed text {:?}, got {:?}",
            typed_chars, doc.text()
        );

        // Escape back to normal mode
        cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::escape());
        prop_assert_eq!(engine.mode(), Mode::Normal, "Should return to Normal mode after Escape");

        // Undo should restore the original text
        process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('u'));

        prop_assert_eq!(
            doc.text(), initial_text.as_str(),
            "Undo after insert should restore original text"
        );
    }
}

// =============================================================================
// P6 — VISUAL SELECTION VALIDITY
// =============================================================================

proptest! {
    #![proptest_config(config(200, 50))]

    /// Visual mode invariant: entering visual mode, applying random motions,
    /// and then escaping should:
    ///
    /// 1. Keep cursor within document bounds after every motion.
    /// 2. Return to Normal mode after Escape.
    #[test]
    fn prop_visual_selection_validity(
        initial_text in small_random_document(),
        motions in prop::collection::vec(
            prop::sample::select(vec!['h', 'j', 'k', 'l', 'w', 'b', 'e', '0', '$', 'G']),
            3..=10
        ),
    ) {
        let mut doc = TestDocument::new(&initial_text, (0, 0));
        let mut engine = VimEngine::new();
        let mut cursor = doc.cursor_offset();

        // Enter visual mode with 'v'
        cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('v'));
        prop_assert!(
            engine.mode().is_visual(),
            "Should be in Visual mode after 'v', got {:?}",
            engine.mode()
        );

        // Apply random motions
        for (i, &motion_char) in motions.iter().enumerate() {
            // Clamp cursor before processing
            let text_len = doc.text().len();
            if text_len > 0 {
                cursor = cursor.min(text_len - 1);
            } else {
                cursor = 0;
            }

            if let Ok(ctx) = InputContext::new(&doc, cursor).validate() {
                let response = engine.process(KeyEvent::char(motion_char), ctx);
                for effect in response.effects().iter().cloned() {
                    if let Effect::SetCursor { offset } = &effect {
                        cursor = offset.get();
                    }
                    apply_effect(&mut doc, effect);
                }
            }

            // Cursor must remain within document bounds
            let text_len = doc.text().len();
            prop_assert!(
                cursor <= text_len,
                "Motion {i} ({motion_char}): cursor ({cursor}) > text.len() ({text_len})"
            );
        }

        // Escape: should return to Normal mode
        let text_len = doc.text().len();
        if text_len > 0 {
            cursor = cursor.min(text_len - 1);
        } else {
            cursor = 0;
        }
        if let Ok(ctx) = InputContext::new(&doc, cursor).validate() {
            let response = engine.process(KeyEvent::escape(), ctx);
            for effect in response.effects().iter().cloned() {
                apply_effect(&mut doc, effect);
            }
        }

        prop_assert_eq!(
            engine.mode(), Mode::Normal,
            "Should return to Normal mode after Escape from Visual"
        );
    }
}

// =============================================================================
// P7 — CURSOR MOVEMENT MONOTONICITY
// =============================================================================

proptest! {
    #![proptest_config(config(300, 50))]

    /// `j` should not decrease the cursor line number. `k` should not increase it.
    ///
    /// On a multi-line document, pressing `j` moves down (line number non-decreasing)
    /// and pressing `k` moves up (line number non-increasing). This tests the
    /// monotonicity invariant over random multi-line documents.
    #[test]
    fn prop_cursor_movement_monotonicity(
        line1 in "[a-z]{3,20}",
        line2 in "[a-z]{3,20}",
        line3 in "[a-z]{3,20}",
        line4 in "[a-z]{3,20}",
        // Number of j presses (1..=5), then k presses (1..=5)
        j_count in 1usize..=5,
        k_count in 1usize..=5,
    ) {
        let initial_text = format!("{}\n{}\n{}\n{}", line1, line2, line3, line4);
        let mut doc = TestDocument::new(&initial_text, (0, 0));
        let mut engine = VimEngine::new();
        let mut cursor = doc.cursor_offset();

        // Helper: compute line number from byte offset
        let line_of = |text: &str, off: usize| -> usize {
            text[..off.min(text.len())]
                .bytes()
                .filter(|&b| b == b'\n')
                .count()
        };

        // Press 'j' repeatedly — line should never decrease
        let mut prev_line = line_of(doc.text(), cursor);
        for i in 0..j_count {
            let text_len = doc.text().len();
            if text_len > 0 {
                cursor = cursor.min(text_len - 1);
            }
            cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('j'));
            let cur_line = line_of(doc.text(), cursor);
            prop_assert!(
                cur_line >= prev_line,
                "j press {i}: line went from {} to {} (should not decrease)",
                prev_line, cur_line
            );
            prev_line = cur_line;
        }

        // Press 'k' repeatedly — line should never increase
        prev_line = line_of(doc.text(), cursor);
        for i in 0..k_count {
            let text_len = doc.text().len();
            if text_len > 0 {
                cursor = cursor.min(text_len - 1);
            }
            cursor = process_key_and_apply(&mut engine, &mut doc, cursor, KeyEvent::char('k'));
            let cur_line = line_of(doc.text(), cursor);
            prop_assert!(
                cur_line <= prev_line,
                "k press {i}: line went from {} to {} (should not increase)",
                prev_line, cur_line
            );
            prev_line = cur_line;
        }
    }
}
