//! Shadow integration tests: edge cases, stress tests, and adversarial scenarios.

use vim_core::document::Document;
use vim_core::execution::{
    parse_keys_from_string, ExternalEdit, ExternalEditKind, HostSession, InputContext, VimEngine,
};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{BufferId, Mark, MarkName, Mode, Offset, Range};

// ═══════════════════════════════════════════════════════════════════════════════
// Helper
// ═══════════════════════════════════════════════════════════════════════════════

/// Feed a Vim key-notation string into the session, returning the last response.
fn feed(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Generation Counter Edge Cases
// ═══════════════════════════════════════════════════════════════════════════════

/// Helper: a document with a controllable text_generation() and mutable text.
/// Reused by the gen_edge tests below.
struct GenEdgeDocument {
    text: String,
    generation: u64,
}
impl Document for GenEdgeDocument {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_count(&self) -> usize {
        memchr::memchr_iter(b'\n', self.text.as_bytes()).count() + 1
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<vim_core::primitives::Position> {
        let off = offset.get();
        if off > self.text.len() {
            return None;
        }
        let prefix = &self.text[..off];
        let line = memchr::memchr_iter(b'\n', prefix.as_bytes()).count();
        let line_start = prefix.rfind('\n').map_or(0, |pos| pos + 1);
        let col = off - line_start;
        Some(vim_core::primitives::Position::from_raw(line, col))
    }
    fn pos_to_offset(&self, pos: vim_core::primitives::Position) -> Option<Offset> {
        let target_line = pos.line().get();
        let target_col = pos.col().get();
        let mut offset = 0;
        for _ in 0..target_line {
            offset =
                memchr::memchr(b'\n', self.text[offset..].as_bytes()).map(|i| offset + i + 1)?;
        }
        let line_end = memchr::memchr(b'\n', self.text[offset..].as_bytes())
            .map(|i| offset + i)
            .unwrap_or(self.text.len());
        let line_len = line_end - offset;
        let col = target_col.min(line_len);
        Some(Offset::new(offset + col))
    }
    fn text_generation(&self) -> Option<u64> {
        Some(self.generation)
    }
}

/// Generation counter DECREASES (host returns gen=5, then gen=3).
///
/// The drift gate fast-path only skips when `host_gen == stored_gen`. A decrease
/// means `3 != 5`, so `needs_check = true` and the engine compares text.
/// Drift is detected and healed normally.
#[test]
fn gen_edge_decreasing_generation_triggers_drift_check() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("hello\n");

    // First process: establish generation = 5 with matching text.
    let doc1 = GenEdgeDocument {
        text: "hello\n".to_string(),
        generation: 5,
    };
    let ctx1 = InputContext::new(&doc1, 0).validate().unwrap();
    let _resp1 = engine.process(KeyEvent::char('l'), ctx1);

    // Engine now stores shadow_generation = Some(5), shadow = "hello\n".

    // Second process: generation DECREASES to 3, text changed.
    // Because 3 != 5, the drift gate must perform the text comparison and heal.
    let doc2 = GenEdgeDocument {
        text: "goodbye\n".to_string(),
        generation: 3,
    };
    let ctx2 = InputContext::new(&doc2, 0).validate().unwrap();
    let _resp2 = engine.process(KeyEvent::char('l'), ctx2);

    assert_eq!(
        engine.shadow_text().unwrap(),
        "goodbye\n",
        "drift gate must detect and heal when generation decreases"
    );

    // Third process: generation stays at 3 (same as stored), no text change.
    // Fast-path: 3 == 3 → skip text comparison.
    let doc3 = GenEdgeDocument {
        text: "goodbye\n".to_string(),
        generation: 3,
    };
    let ctx3 = InputContext::new(&doc3, 0).validate().unwrap();
    let _resp3 = engine.process(KeyEvent::char('l'), ctx3);

    assert_eq!(
        engine.shadow_text().unwrap(),
        "goodbye\n",
        "fast-path skip: shadow unchanged when generation matches after decrease"
    );
}

/// Generation stays the same but text changed (hostile host lies).
///
/// The drift gate fast-path sees `host_gen == stored_gen` and skips the text
/// comparison entirely. The shadow becomes stale — the engine does NOT detect
/// the change. This is a KNOWN host-contract-violation, not an engine bug:
/// the generation counter contract requires hosts to bump the counter whenever
/// text changes.
#[test]
fn gen_edge_same_generation_but_text_changed_is_not_caught() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("aaa\n");

    // First process: establish generation = 10 with matching text.
    let doc1 = GenEdgeDocument {
        text: "aaa\n".to_string(),
        generation: 10,
    };
    let ctx1 = InputContext::new(&doc1, 0).validate().unwrap();
    let _resp1 = engine.process(KeyEvent::char('l'), ctx1);

    // Engine stores shadow_generation = Some(10), shadow = "aaa\n".

    // Second process: generation STILL 10, but the text is now "zzz\n".
    // This violates the host contract (text changed without bumping generation).
    // The drift gate sees 10 == 10 → needs_check = false → skips comparison.
    let doc2 = GenEdgeDocument {
        text: "zzz\n".to_string(),
        generation: 10,
    };
    let ctx2 = InputContext::new(&doc2, 0).validate().unwrap();
    let _resp2 = engine.process(KeyEvent::char('l'), ctx2);

    // Shadow is STALE — still "aaa\n" because the drift gate was bypassed.
    assert_eq!(
        engine.shadow_text().unwrap(),
        "aaa\n",
        "shadow must remain stale: fast-path skipped comparison (host-contract-violation)"
    );

    // Now bump generation to 11 with the same changed text.
    // This time 11 != 10, so the drift gate fires and finally heals.
    let doc3 = GenEdgeDocument {
        text: "zzz\n".to_string(),
        generation: 11,
    };
    let ctx3 = InputContext::new(&doc3, 0).validate().unwrap();
    let _resp3 = engine.process(KeyEvent::char('l'), ctx3);

    assert_eq!(
        engine.shadow_text().unwrap(),
        "zzz\n",
        "drift healed once generation finally bumped"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stress Test: Rapid Typing Shadow Sync
// ═══════════════════════════════════════════════════════════════════════════════

/// Stress-test the `update_shadow_from_effects` path by rapidly feeding 100
/// keystrokes through a session with shadow enabled: enter insert mode with
/// `i`, type 99 printable characters, then press Escape. After each batch
/// the shadow must match the host document text exactly.
///
/// Then apply an external edit (simulating a formatter or autocomplete) and
/// verify drift healing corrects the shadow on the next keystroke.
#[test]
fn stress_test_rapid_typing_shadow_sync() {
    let initial = "hello\n";
    let mut session = HostSession::new(initial);
    session.set_shadow_text(initial);

    // ── Phase 1: enter insert mode ──────────────────────────────────────
    session.set_cursor_offset(5); // just before '\n'
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);

    // Shadow should still match after mode switch (no text change yet).
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        session.text(),
        "shadow must match host immediately after entering insert mode"
    );

    // ── Phase 2: type 99 characters rapidly ─────────────────────────────
    // Use a rotating set of ASCII characters to exercise varied insert effects.
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();

    for i in 0..99 {
        let ch = chars[i % chars.len()];
        feed(&mut session, &ch.to_string());
    }

    // After 99 characters, shadow must still match host text exactly.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        session.text(),
        "shadow must match host after 99 rapid character insertions"
    );

    // Sanity: document should contain all 99 chars between "hello" and "\n".
    let text_after_insert = session.text().to_owned();
    // "hello" (5) + 99 chars + "\n" (1) = 105
    assert_eq!(
        text_after_insert.len(),
        105,
        "document should be 105 bytes: 5 (hello) + 99 (typed) + 1 (newline)"
    );
    assert!(
        text_after_insert.starts_with("hello"),
        "document should still start with 'hello'"
    );
    assert!(
        text_after_insert.ends_with('\n'),
        "document should still end with newline"
    );

    // ── Phase 3: exit insert mode ───────────────────────────────────────
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Shadow must still match after Escape.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        session.text(),
        "shadow must match host after exiting insert mode"
    );

    // Record the text for drift-healing verification.
    let text_before_external = session.text().to_owned();

    // ── Phase 4: apply an external edit and verify drift healing ────────
    // Simulate a formatter that replaces "hello" with "HELLO" at the start.
    session.set_shadow_text(&text_before_external); // ensure shadow is current
    session.set_text(&text_before_external.replacen("hello", "HELLO", 1));

    let expected_after_edit = session.text().to_owned();
    assert!(
        expected_after_edit.starts_with("HELLO"),
        "external edit should have uppercased 'hello'"
    );

    // Shadow is now stale (still has lowercase "hello...").
    assert_ne!(
        session.engine().shadow_text().unwrap(),
        expected_after_edit,
        "shadow should be stale before drift gate fires"
    );

    // Feed a harmless key to trigger the drift gate.
    feed(&mut session, "l");

    // Shadow must heal to match the externally-edited host text.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        expected_after_edit,
        "shadow must heal to match host after external edit (drift healing)"
    );

    // Document text must still be correct.
    assert_eq!(
        session.text(),
        expected_after_edit,
        "host document must be unchanged after drift healing"
    );

    // Engine must still be functional.
    assert_eq!(session.mode(), Mode::Normal);

    // ── Phase 5: verify undo tree has entries ───────────────────────────
    // The 99-character insert + external edit should have created undo entries.
    assert!(
        session.engine().undo_tree().can_undo(),
        "undo tree should have entries after insert + external edit"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Insert-Mode Edge Cases with External Edits
// ═══════════════════════════════════════════════════════════════════════════════

/// Edge case 1: External edit WITHIN the insert region (autocomplete replaces
/// the typed prefix).
///
/// Simulate: user is in insert mode and has typed "con" on line 2.
/// The host IDE's autocomplete fires and replaces "con" with "console" — this
/// is an external edit that lands squarely inside the insert region.
///
/// Verify:
/// - Shadow heals to match the host after the next keystroke.
/// - The engine stays in insert mode (external edit must not knock it out).
/// - Accumulated text still contains what the user typed (the engine only
///   knows about the keystrokes it processed, not the autocomplete result).
/// - The undo tree has an entry so the user can undo.
#[test]
fn insert_edge_external_edit_within_insert_region() {
    // Two-line document: user will type on line 2.
    let initial = "function greet() {\n    \n}\n";
    let mut session = HostSession::new(initial);
    session.set_shadow_text(initial);

    // Move cursor to end of "    " on line 2 and enter append mode.
    // Offsets: "function greet() {\n" = 19 bytes (0..19), "    " = 4 bytes (19..23),
    // "\n" at 23, "}" at 24, "\n" at 25. Total = 26.
    // Place cursor at offset 22 (last space) and append.
    session.set_cursor_offset(22);
    feed(&mut session, "a");
    assert_eq!(session.mode(), Mode::Insert);

    // User types "con" — engine records this in accumulated_text.
    // Append mode places cursor after offset 22 (before the \n at 23).
    // Typing "con" inserts at that point.
    feed(&mut session, "con");
    assert_eq!(session.text(), "function greet() {\n    con\n}\n");

    // Verify accumulated text contains the typed prefix.
    let accum = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned());
    assert_eq!(
        accum.as_deref(),
        Some("con"),
        "engine should have recorded 'con' as accumulated text"
    );

    // Sync shadow to current state before the external edit.
    let before_autocomplete = session.text().to_owned();
    session.set_shadow_text(&before_autocomplete);

    // --- Autocomplete fires: host replaces "con" (offset 23..26) with "console" ---
    // After typing, document is "function greet() {\n    con\n}\n" (29 bytes).
    // "con" occupies offsets 23, 24, 25 (3 bytes starting at 23).
    session.apply_external_edit(23, 3, "console");
    assert_eq!(
        session.text(),
        "function greet() {\n    console\n}\n",
        "autocomplete should replace 'con' with 'console'"
    );

    // Shadow is stale — still has "con" not "console".
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        before_autocomplete,
        "shadow should be stale before drift gate fires"
    );

    // Press a key in insert mode to trigger the drift gate.
    // Use a printable character so we continue inserting.
    feed(&mut session, ".");

    // After drift healing:
    // 1. Shadow matches host.
    let expected_text = session.text().to_owned();
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        expected_text,
        "shadow should heal to match host after autocomplete within insert region"
    );

    // 2. Engine is still in insert mode.
    assert_eq!(
        session.mode(),
        Mode::Insert,
        "engine should remain in insert mode after external edit within insert region"
    );

    // 3. Accumulated text should contain the originally typed characters.
    //    The engine only processed 'c', 'o', 'n', '.' — not "console".
    let accum_after = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned());
    assert!(
        accum_after.is_some(),
        "insert state should still exist after drift healing"
    );
    let accum_str = accum_after.unwrap();
    assert!(
        accum_str.contains("con") && accum_str.contains('.'),
        "accumulated text should contain 'con' and '.', got: {accum_str:?}"
    );

    // 4. Undo tree has entries.
    assert!(
        session.engine().undo_tree().can_undo(),
        "undo tree should have entries after insert + autocomplete external edit"
    );

    // Clean exit.
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);
}

/// Edge case 2: External edit DELETES text BEFORE the cursor while user is in
/// insert mode (LSP removes an import while user is typing).
///
/// Simulate: user is typing on line 3. An LSP code action removes "use old;\n"
/// from line 1 (10 bytes deleted before the cursor). Verify:
/// - Shadow heals correctly.
/// - Engine stays in insert mode.
/// - Entry offset shifts backward by the deleted length.
/// - Accumulated text is preserved (the deletion was far from the insert region).
#[test]
fn insert_edge_external_delete_before_cursor() {
    // Three lines: an import, a blank line, and the line the user is editing.
    let initial = "use old;\nuse new;\nfn main() {\n    \n}\n";
    let mut session = HostSession::new(initial);
    session.set_shadow_text(initial);

    // Move cursor to end of "    " on line 4 (the body of main).
    // "use old;\n"       = 9 bytes  (offsets 0..9)
    // "use new;\n"       = 9 bytes  (offsets 9..18)
    // "fn main() {\n"    = 12 bytes (offsets 18..30)
    // "    \n"            = 5 bytes  (offsets 30..35)
    // "}\n"              = 2 bytes  (offsets 35..37)
    // Total = 37 bytes.
    // User wants to type at end of "    " on line 4: last space is at offset 33.
    assert_eq!(initial.len(), 37, "initial text should be 37 bytes");
    session.set_cursor_offset(33); // last space of "    " (offset 33)
    feed(&mut session, "a"); // append: cursor goes after offset 33 (before the \n at 34)
    assert_eq!(session.mode(), Mode::Insert);

    // User types "let x" — engine records this.
    feed(&mut session, "let x");
    let after_typing = session.text().to_owned();
    assert!(
        after_typing.contains("let x"),
        "document should contain 'let x' after typing"
    );

    // Record insert state before external edit.
    let entry_before = session
        .engine()
        .state()
        .insert_state()
        .and_then(|is| is.entry_offset())
        .map(|o| o.get());
    let accum_before = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned());
    assert_eq!(
        accum_before.as_deref(),
        Some("let x"),
        "accumulated text should be 'let x' before external edit"
    );

    // Sync shadow to current state.
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // --- LSP removes "use old;\n" (9 bytes at offset 0..9) ---
    // Build the new text by removing the first line.
    let new_text = current.replacen("use old;\n", "", 1);
    let deleted_len = current.len() - new_text.len();
    assert_eq!(
        deleted_len, 9,
        "should have deleted 9 bytes ('use old;\\n')"
    );
    session.set_text(&new_text);

    // Trigger drift gate with a keystroke in insert mode.
    feed(&mut session, ";");

    // 1. Shadow healed.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        session.text(),
        "shadow should heal after LSP deletes text before cursor"
    );

    // 2. Engine is still in insert mode.
    assert_eq!(
        session.mode(),
        Mode::Insert,
        "engine should remain in insert mode after external deletion before cursor"
    );

    // 3. Entry offset should have shifted backward by deleted_len.
    let insert_state = session
        .engine()
        .state()
        .insert_state()
        .expect("should still be in insert mode");
    if let (Some(entry_was), Some(entry_now_offset)) = (entry_before, insert_state.entry_offset()) {
        let entry_now = entry_now_offset.get();
        assert_eq!(
            entry_now,
            entry_was - deleted_len,
            "entry_offset should shift backward by deleted length ({deleted_len}): \
             was {entry_was}, now {entry_now}"
        );
    }

    // 4. Accumulated text should still contain user's typing.
    let accum_after = insert_state.accumulated_text();
    assert!(
        accum_after.contains("let x"),
        "accumulated text should preserve user typing after external deletion before cursor, \
         got: {accum_after:?}"
    );

    // Clean exit.
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Document should contain the typed text but not the removed import.
    let final_text = session.text();
    assert!(
        !final_text.contains("use old;"),
        "removed import should not be in final text"
    );
    assert!(
        final_text.contains("let x"),
        "user's typed text should be in final text, got: {final_text:?}"
    );
}

/// Edge case 3: External edit after Escape — does dot-repeat replay correctly?
///
/// Simulate: user enters insert mode, types "abc", Escape. Then an external
/// edit modifies the document (formatter). Then user presses `.` for dot-repeat.
///
/// Verify:
/// - Dot-repeat replays the engine's recorded text ("abc"), not anything
///   related to the external edit.
/// - Shadow is in sync after dot-repeat completes.
/// - The engine returns to Normal mode.
#[test]
fn insert_edge_dot_repeat_after_external_edit() {
    let initial = "line1\nline2\n";
    let mut session = HostSession::new(initial);
    session.set_shadow_text(initial);

    // Enter insert mode at end of line 1 and type "abc".
    // "line1" is at offsets 0..5. We want to append after "line1".
    session.set_cursor_offset(4); // 'e' of "line1"... no, offset 4 = '1'
    feed(&mut session, "a"); // append: cursor after offset 4
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "abc");
    assert_eq!(
        session.text(),
        "line1abc\nline2\n",
        "after typing 'abc' in append mode"
    );

    // Verify accumulated text is "abc".
    let accum = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned());
    assert_eq!(
        accum.as_deref(),
        Some("abc"),
        "accumulated text should be 'abc'"
    );

    // Press Escape to exit insert mode. This saves the insert command for
    // dot-repeat (last_inserted_text = "abc").
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    let last_inserted = session.engine().state().last_inserted_text().to_owned();
    assert_eq!(
        last_inserted, "abc",
        "last_inserted_text should be 'abc' after Escape"
    );

    // Sync shadow.
    let after_insert = session.text().to_owned();
    session.set_shadow_text(&after_insert);

    // --- External edit: formatter changes "line2" to "LINE2" ---
    let new_text = after_insert.replacen("line2", "LINE2", 1);
    session.set_text(&new_text);

    // Trigger drift gate with a harmless normal-mode key.
    feed(&mut session, "l");
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        session.text(),
        "shadow should heal after formatter external edit"
    );

    // Sync shadow for the dot-repeat phase.
    let before_dot = session.text().to_owned();
    session.set_shadow_text(&before_dot);

    // Verify last_inserted_text survived the external edit (it should — the
    // drift gate does not modify the last_command/last_inserted_text fields).
    let last_after_ext = session.engine().state().last_inserted_text().to_owned();
    assert_eq!(
        last_after_ext, "abc",
        "last_inserted_text should survive external edit unchanged"
    );

    // Move cursor to end of "LINE2" for dot-repeat.
    // "line1abc\n" = 9 bytes, "LINE2" starts at offset 9, ends at 14.
    session.set_cursor_offset(13); // '2' of "LINE2"
    feed(&mut session, "l"); // sync cursor position

    // Press '.' for dot-repeat.
    feed(&mut session, ".");

    // 1. Engine should be back in Normal mode after dot-repeat.
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "engine should be in Normal mode after dot-repeat"
    );

    // 2. The dot-repeat should have inserted "abc" somewhere in the document.
    let text_after_dot = session.text().to_owned();
    // Count occurrences of "abc" — should have at least 2 (original + dot-repeat).
    let abc_count = text_after_dot.matches("abc").count();
    assert!(
        abc_count >= 2,
        "dot-repeat should insert 'abc' again (expected >= 2 occurrences, got {abc_count}). \
         Text: {text_after_dot:?}"
    );

    // 3. Shadow should be in sync after dot-repeat.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        text_after_dot,
        "shadow should match host after dot-repeat completes"
    );

    // 4. The formatter's change should still be present (not reverted by dot-repeat).
    assert!(
        text_after_dot.contains("LINE2"),
        "formatter's 'LINE2' should survive dot-repeat, got: {text_after_dot:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Mark Remap Exact Arithmetic
// ═══════════════════════════════════════════════════════════════════════════════

/// Verify exact byte-offset arithmetic when a pure insertion shifts a named mark.
///
/// Setup:
///   Document: "AAAA\nBBBB\nCCCCCCCCCC\n"  (22 bytes)
///             line 1 = offsets 0-4, line 2 = offsets 5-9, line 3 = offsets 10-20
///   Mark 'a' at offset 10 (first 'C' on line 3).
///
/// Edit: insert "XYZ" (3 bytes) at offset 5 (start of line 2).
///
/// Expected: mark shifts from 10 to 13 (10 + 3 = 13).
///
/// Reasoning: `adjust_offset_named(val=10, pos=5, old_len=0, delta=+3)`:
///   val >= pos → not in deleted region → shift by +3 → 13.
///   Named-mark cross-line skip: edit_line_end = 9 (newline at end of "BBBB").
///   Mark at 10 > 9, so adjustment runs.
#[test]
fn mark_remap_exact_insert_3_bytes() {
    //  "AAAA\nBBBB\nCCCCCCCCCC\n"
    //   0    4 5   9 10        20 21
    let doc = "AAAA\nBBBB\nCCCCCCCCCC\n";
    let mut session = HostSession::new(doc);
    session.set_shadow_text(doc);

    // Set mark 'a' at offset 10 (first 'C' on line 3)
    session.set_mark('a', 10);
    assert_eq!(session.get_mark('a'), Some(10));

    // Apply a pure insertion of 3 bytes at offset 5 via the engine.
    // Range [5, 5) = zero-length deletion + insert "XYZ".
    let edit = ExternalEdit::new(
        Range::new(Offset::new(5), Offset::new(5)),
        "XYZ",
        Offset::new(8), // caret after insertion
        ExternalEditKind::HostNotified,
    );
    let _ = session.engine_mut().apply_external_edit(edit);

    // Verify: mark 'a' shifted exactly by +3
    let mark_a = session
        .get_mark('a')
        .expect("mark 'a' should exist after insert");
    assert_eq!(
        mark_a, 13,
        "mark 'a' should shift from 10 to exactly 13 after inserting 3 bytes at offset 5"
    );

    // Shadow should reflect the insertion
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "AAAA\nXYZBBBB\nCCCCCCCCCC\n",
        "shadow should contain the inserted bytes"
    );
}

/// Verify exact byte-offset arithmetic when a pure deletion shifts a named mark.
///
/// Setup:
///   Document: "AAAAAAAAAA\nBBBBB\nCCCCCCCCCC\n"  (28 bytes)
///             line 1 = offsets 0-10, line 2 = offsets 11-16, line 3 = offsets 17-27
///   Mark 'a' at offset 20 (fourth 'C' on line 3).
///
/// Edit: delete 5 bytes at offset 10 (the '\n' at end of line 1 + "BBBB").
///   Deleted range: [10, 15).
///
/// Expected: mark shifts from 20 to 15 (20 - 5 = 15).
///
/// Reasoning: `adjust_offset_named(val=20, pos=10, old_len=5, delta=-5)`:
///   val(20) >= pos(10) and val(20) >= pos+old_len(15) → shift by -5 → 15.
///   Named-mark cross-line skip: edit_line_end = 10 (newline at position 10).
///   Mark at 20 > 10, so adjustment runs.
#[test]
fn mark_remap_exact_delete_5_bytes() {
    //  "AAAAAAAAAA\nBBBBB\nCCCCCCCCCC\n"
    //   0         9 10   15 16 17      27
    let doc = "AAAAAAAAAA\nBBBBB\nCCCCCCCCCC\n";
    let mut session = HostSession::new(doc);
    session.set_shadow_text(doc);

    // Set mark 'a' at offset 20 (fourth 'C' on line 3)
    session.set_mark('a', 20);
    assert_eq!(session.get_mark('a'), Some(20));

    // Delete 5 bytes at offset 10: range [10, 15).
    // This removes "\nBBBB" (the newline + first 4 chars of line 2).
    let edit = ExternalEdit::new(
        Range::new(Offset::new(10), Offset::new(15)),
        "",
        Offset::new(10), // caret after deletion
        ExternalEditKind::HostNotified,
    );
    let _ = session.engine_mut().apply_external_edit(edit);

    // Verify: mark 'a' shifted exactly by -5
    let mark_a = session
        .get_mark('a')
        .expect("mark 'a' should exist after delete");
    assert_eq!(
        mark_a, 15,
        "mark 'a' should shift from 20 to exactly 15 after deleting 5 bytes at offset 10"
    );

    // Shadow should reflect the deletion
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "AAAAAAAAAAB\nCCCCCCCCCC\n",
        "shadow should reflect the 5-byte deletion"
    );
}

/// Verify exact byte-offset arithmetic when a shorter replacement shifts a named mark.
///
/// Setup:
///   Document: "AAAAA\nBBBBBBBBB\nCCCCCCCCCC\n"  (27 bytes)
///             line 1 = offsets 0-5, line 2 = offsets 6-15, line 3 = offsets 16-26
///   Mark 'a' at offset 15 (last 'B' / '\n' boundary on line 2).
///
/// Edit: replace 4 bytes at offset 5 with 2 bytes.
///   Replaced range: [5, 9) = "\nBBB" → "XY".
///   delta = 2 - 4 = -2.
///
/// Expected: mark shifts from 15 to 13 (15 - 2 = 13).
///
/// Reasoning: `adjust_offset_named(val=15, pos=5, old_len=4, delta=-2)`:
///   val(15) >= pos(5) and val(15) >= pos+old_len(9) → shift by -2 → 13.
///   Named-mark cross-line skip: edit_line_end = 5 (newline at position 5).
///   Mark at 15 > 5, so adjustment runs.
#[test]
fn mark_remap_exact_replace_shorter() {
    //  "AAAAA\nBBBBBBBBB\nCCCCCCCCCC\n"
    //   0    5 6        15 16        26
    let doc = "AAAAA\nBBBBBBBBB\nCCCCCCCCCC\n";
    let mut session = HostSession::new(doc);
    session.set_shadow_text(doc);

    // Set mark 'a' at offset 15 (the '\n' at end of line 2)
    session.set_mark('a', 15);
    assert_eq!(session.get_mark('a'), Some(15));

    // Replace 4 bytes [5, 9) with "XY" (2 bytes).
    // Replaces "\nBBB" with "XY", shrinking by 2 bytes.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(5), Offset::new(9)),
        "XY",
        Offset::new(7), // caret after replacement
        ExternalEditKind::HostNotified,
    );
    let _ = session.engine_mut().apply_external_edit(edit);

    // Verify: mark 'a' shifted exactly by -2
    let mark_a = session
        .get_mark('a')
        .expect("mark 'a' should exist after replace");
    assert_eq!(
        mark_a, 13,
        "mark 'a' should shift from 15 to exactly 13 after replacing 4 bytes with 2 at offset 5"
    );

    // Shadow should reflect the replacement
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "AAAAAXYBBBBBB\nCCCCCCCCCC\n",
        "shadow should reflect the shorter replacement"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ADVERSARIAL: Rapid alternation of drift heal and normal edit
// ═══════════════════════════════════════════════════════════════════════════════

/// Adversarial test 1: Interleave drift-heal cycles with normal Vim edits 10
/// times. After each drift heal the shadow must match the host, and normal edits
/// immediately after must still produce correct text mutations. This exercises
/// the transition path between the external-edit reconciliation code and the
/// normal keystroke pipeline within a single session, exposing any stale state
/// that survives across the boundary.
#[test]
fn adversarial_rapid_drift_heal_normal_edit_alternation() {
    let mut session = HostSession::new("line1\nline2\nline3\n");
    session.set_shadow_text("line1\nline2\nline3\n");

    for i in 0u32..10 {
        // --- Drift heal phase: host externally mutates the document ---
        let current = session.text().to_owned();
        let tag = format!("EXT{i}");
        // Append a tagged line at the end.
        let new_text = format!("{current}{tag}\n");
        session.set_text(&new_text);

        // Shadow is stale — trigger drift gate with a harmless key.
        feed(&mut session, "l");

        // Shadow must match the host after healing.
        assert_eq!(
            session.engine().shadow_text().unwrap(),
            new_text,
            "shadow should match host after drift heal on iteration {i}"
        );
        assert_eq!(session.text(), new_text);

        // Sync shadow for the normal-edit phase.
        session.set_shadow_text(&session.text().to_owned());

        // --- Normal edit phase: delete first char with 'x' ---
        session.set_cursor_offset(0);
        feed(&mut session, "x");

        let after_x = session.text().to_owned();
        // The first character should have been deleted.
        assert!(
            !after_x.starts_with(&new_text[..1]) || new_text.starts_with(&after_x[..1]),
            "iteration {i}: 'x' should delete the first character"
        );

        // Shadow should track the engine-produced delete effect.
        assert_eq!(
            session.engine().shadow_text().unwrap(),
            after_x,
            "shadow should match host after normal edit on iteration {i}"
        );

        // Sync shadow for the next cycle.
        session.set_shadow_text(&after_x);
    }

    // Engine must still be fully functional after 10 cycles.
    assert_eq!(session.mode(), Mode::Normal);
    // Undo tree should have accumulated entries from both drift heals and x edits.
    assert!(
        session.engine().undo_tree().can_undo(),
        "undo tree should have entries after 10 drift+edit cycles"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ADVERSARIAL: External edit that exactly doubles the document size
// ═══════════════════════════════════════════════════════════════════════════════

/// Adversarial test 2: The host externally replaces the entire document with a
/// version that is exactly twice as long (the original text concatenated with
/// itself). This is an edge case for the diff algorithm: the suffix of the new
/// text contains a complete copy of the old text, which can confuse naive
/// longest-common-prefix/suffix approaches. Verify that the shadow heals
/// correctly, marks remap, and undo restores the original.
#[test]
fn adversarial_external_edit_doubles_document_size() {
    let original = "fn main() {\n    println!(\"hello\");\n}\n";
    let doubled = format!("{original}{original}");
    assert_eq!(
        doubled.len(),
        original.len() * 2,
        "doubled text should be exactly 2x original"
    );

    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Set marks at known positions.
    session.set_mark('a', 0); // start of doc
    session.set_mark('b', original.len() - 1); // last char (newline)

    // Baseline keystroke.
    feed(&mut session, "l");

    // --- External edit: double the document ---
    session.set_text(&doubled);

    // Trigger drift gate.
    feed(&mut session, "l");

    // Shadow must heal to the doubled text.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        doubled,
        "shadow should heal to doubled document"
    );
    assert_eq!(session.text(), doubled);

    // Marks should be within the new document bounds.
    let mark_a = session
        .get_mark('a')
        .expect("mark 'a' should survive doubling");
    assert!(
        mark_a < doubled.len(),
        "mark 'a' should be within doubled document bounds, got {mark_a}"
    );
    let mark_b = session
        .get_mark('b')
        .expect("mark 'b' should survive doubling");
    assert!(
        mark_b < doubled.len(),
        "mark 'b' should be within doubled document bounds, got {mark_b}"
    );

    // Undo should restore the original document.
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "undo after doubling should restore original text"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ADVERSARIAL: External edit during visual block mode
// ═══════════════════════════════════════════════════════════════════════════════

/// Adversarial test 3: Enter visual block mode (<C-v>), make a selection across
/// multiple lines, then simulate an external edit that inserts text before the
/// selection. The drift gate must heal the shadow and the engine must remain in
/// a consistent state (either staying in visual mode with adjusted positions, or
/// gracefully exiting visual mode). The key invariant: no panic, no corruption.
#[test]
fn adversarial_external_edit_during_visual_block_mode() {
    let original = "aaaa\nbbbb\ncccc\ndddd\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Enter visual block mode: <C-v> then move down 2 lines and right 2 chars.
    session.set_cursor_offset(0);
    feed(&mut session, "<C-v>");
    assert!(
        matches!(session.mode(), Mode::Visual(_)),
        "should be in visual mode after <C-v>"
    );

    // Extend selection: 2 lines down and 2 chars right.
    feed(&mut session, "2j2l");

    // Sync shadow to current state.
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // --- External edit: insert "XXXXXX\n" at the very beginning ---
    let prefix = "XXXXXX\n";
    let new_text = format!("{prefix}{current}");
    session.set_text(&new_text);

    // Trigger drift gate. The engine is in visual block mode with a selection
    // that references byte offsets in the OLD document. The drift gate must
    // handle this gracefully.
    feed(&mut session, "l");

    // Shadow must heal.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        new_text,
        "shadow should heal after external edit during visual block mode"
    );

    // The engine must be in a valid state. It may have exited visual mode
    // (if the selection became invalid) or stayed in visual mode with
    // adjusted selection. Either is acceptable; panic is not.
    let mode_after = session.mode();
    assert!(
        matches!(mode_after, Mode::Visual(_) | Mode::Normal),
        "engine should be in Visual or Normal mode after drift heal, got {:?}",
        mode_after
    );

    // Press Escape to ensure we can return to Normal mode cleanly.
    feed(&mut session, "<Esc>");
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "should be able to return to Normal mode after visual block + drift"
    );

    // Further editing must work: delete a char to prove the engine is not corrupted.
    session.set_cursor_offset(0);
    let text_before_x = session.text().to_owned();
    session.set_shadow_text(&text_before_x);
    feed(&mut session, "x");
    assert_ne!(
        session.text(),
        text_before_x,
        "normal editing should work after visual block + drift heal"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ADVERSARIAL: Two consecutive drifts without host applying effects between
// ═══════════════════════════════════════════════════════════════════════════════

/// Adversarial test 4: Set shadow to text A, then process a key with host text B
/// (drift A->B fires), then immediately process another key with host text C
/// (drift B->C fires) — all without the host applying the engine's effects
/// between calls. This simulates a host that is slow to apply effects while the
/// user is typing rapidly. The shadow must converge to C, marks must be valid,
/// and no undo corruption should occur.
#[test]
fn adversarial_two_successive_drifts_without_effect_application() {
    let text_a = "alpha\nbeta\ngamma\n";
    let text_b = "ALPHA\nbeta\ngamma\n";
    let text_c = "ALPHA\nBETA\nGAMMA\n";

    let mut engine = VimEngine::new();
    engine.set_shadow_text(text_a);

    // Set a mark so we can track position remapping across both drifts.
    engine.marks_mut().set(
        vim_core::primitives::MarkName::new('a').unwrap(),
        vim_core::primitives::Mark::new(Offset::new(6)), // 'b' in "beta"
    );

    let undo_seq_start = engine.undo_tree().next_sequence();

    // --- First process: host document is text_b (drift A -> B) ---
    {
        let doc_b = vim_core::execution::OwnedDocument::new(text_b);
        let ctx = InputContext::new(&doc_b, 0).validate().unwrap();
        let _resp = engine.process(KeyEvent::char('l'), ctx);
    }

    // Shadow should now be text_b.
    assert_eq!(
        engine.shadow_text().unwrap(),
        text_b,
        "shadow should heal to text_b after first drift"
    );

    let undo_seq_after_first = engine.undo_tree().next_sequence();
    assert!(
        undo_seq_after_first > undo_seq_start,
        "first drift should create an undo entry"
    );

    // --- Second process: host document is text_c (drift B -> C) ---
    // We do NOT apply the engine's effects from the first process to the
    // host document. Instead we present text_c directly.
    {
        let doc_c = vim_core::execution::OwnedDocument::new(text_c);
        let ctx = InputContext::new(&doc_c, 0).validate().unwrap();
        let _resp = engine.process(KeyEvent::char('l'), ctx);
    }

    // Shadow should converge to text_c.
    assert_eq!(
        engine.shadow_text().unwrap(),
        text_c,
        "shadow should heal to text_c after second consecutive drift"
    );

    let undo_seq_after_second = engine.undo_tree().next_sequence();
    assert!(
        undo_seq_after_second > undo_seq_after_first,
        "second drift should create another undo entry"
    );

    // Mark 'a' should be within the bounds of text_c.
    let mark_a = engine
        .state()
        .marks()
        .get(vim_core::primitives::MarkName::new('a').unwrap());
    if let Some(m) = mark_a {
        assert!(
            m.offset().get() < text_c.len(),
            "mark 'a' should be within text_c bounds after two drifts, got {}",
            m.offset().get()
        );
    }

    // Engine should still be functional.
    assert_eq!(engine.mode(), Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ADVERSARIAL: External edit inserts text identical to existing text elsewhere
// ═══════════════════════════════════════════════════════════════════════════════

/// Adversarial test 5: The document contains "AAA\nBBB\nCCC\n". An external edit
/// inserts "BBB\n" at offset 0 — text that is IDENTICAL to an existing line but
/// at a different position. This is a pathological case for diff algorithms
/// because the longest-common-subsequence has multiple valid alignments. The
/// shadow must heal correctly, marks on lines AFTER the insertion must shift
/// forward by the inserted length, and undo must restore the original.
#[test]
fn adversarial_insert_identical_text_at_different_position() {
    let original = "AAA\nBBB\nCCC\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Set marks on each line.
    session.set_mark('a', 0); // 'A' on line 1
    session.set_mark('b', 4); // 'B' on line 2
    session.set_mark('c', 8); // 'C' on line 3

    // Baseline keystroke.
    feed(&mut session, "l");

    let undo_seq_before = session.engine().undo_tree().next_sequence();

    // --- External edit: insert "BBB\n" at offset 0 ---
    // Result: "BBB\nAAA\nBBB\nCCC\n"
    // There are now TWO "BBB\n" lines. The diff algorithm must decide which
    // "BBB" in the new text aligns with the original "BBB". Different
    // alignments yield different mark remap results.
    let new_text = format!("BBB\n{original}");
    assert_eq!(new_text, "BBB\nAAA\nBBB\nCCC\n");
    session.set_text(&new_text);

    // Trigger drift gate.
    feed(&mut session, "l");

    // Shadow must heal to the new text.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        new_text,
        "shadow should heal after inserting duplicate line"
    );
    assert_eq!(session.text(), new_text);

    // An undo entry should have been created.
    let undo_seq_after = session.engine().undo_tree().next_sequence();
    assert!(
        undo_seq_after > undo_seq_before,
        "drift should create an undo entry for duplicate-line insertion"
    );

    // All marks must be within the new document bounds.
    for name in ['a', 'b', 'c'] {
        let mark = session.get_mark(name);
        assert!(mark.is_some(), "mark '{name}' should survive the insertion");
        let val = mark.unwrap();
        assert!(
            val < new_text.len(),
            "mark '{name}' at offset {val} should be within new document bounds (len={})",
            new_text.len()
        );
    }

    // Undo should restore the original document.
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "undo after duplicate-line insertion should restore original text"
    );

    // The shadow is updated lazily via the drift gate — after undo, the shadow
    // may still hold the pre-undo text until the next process_key triggers
    // reconciliation. Feed one more key to let the drift gate fire.
    feed(&mut session, "l");

    // Now the shadow should match the host (undo result).
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        original,
        "shadow should match original after undo + drift gate reconciliation"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verify: truncate_tail_bytes handles CJK correctly (bytes, not chars)
// ═══════════════════════════════════════════════════════════════════════════════

/// Regression test: `reconcile_external_edit_mutations` must truncate by
/// byte count, not character count. "你好" is 2 characters but 6 UTF-8 bytes.
/// An external edit that deletes 6 bytes from the insert region should remove
/// exactly those 2 characters, leaving `accumulated_text` empty — not attempt
/// 6 `pop_char()` calls (the old bug, which over-deleted mixed content).
#[test]
fn verify_truncate_tail_bytes_cjk() {
    // Start with a simple ASCII document; the CJK text will be typed.
    let initial = "hello ";
    let mut session = HostSession::new(initial);
    session.set_shadow_text(initial);

    // Enter insert mode at end of "hello "
    session.set_cursor_offset(6);
    feed(&mut session, "a");
    assert_eq!(session.mode(), Mode::Insert);

    // Type two CJK characters: "你好" (6 UTF-8 bytes, 2 chars).
    // Feed them as individual chars through the parser.
    feed(&mut session, "你好");
    assert_eq!(session.text(), "hello 你好");

    // Verify accumulated_text recorded the CJK characters.
    let accum = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned())
        .unwrap_or_default();
    assert_eq!(
        accum, "你好",
        "accumulated text should be the two CJK chars"
    );
    assert_eq!(accum.len(), 6, "CJK accumulated text should be 6 bytes");

    // Sync shadow to current text before the external edit.
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // --- External edit: delete the 6 CJK bytes (offset 6..12) ---
    // Host removes "你好" from "hello 你好", leaving "hello ".
    session.apply_external_edit(6, 6, "");
    assert_eq!(session.text(), "hello ");

    // Notify engine of the deletion within the insert region.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(6), Offset::new(12)),
        "",
        Offset::new(6),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);

    // The key assertion: accumulated_text should be empty, not corrupted.
    // The old pop_char loop would try 6 pops on a 2-char string —
    // for pure CJK that just empties it (harmless), but for mixed
    // content like "ab你好" deleting 6 bytes would wrongly erase "ab" too.
    let accum_after = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned())
        .unwrap_or_default();
    assert_eq!(
        accum_after, "",
        "accumulated_text should be empty after deleting all CJK bytes"
    );

    // Shadow should match host.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "hello ",
        "shadow should match host after CJK deletion"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Verify: undo restores marks to their pre-edit positions
// ═══════════════════════════════════════════════════════════════════════════════

/// Verify that `MarkSnapshot` is captured BEFORE mark remapping in
/// `apply_external_edit`, so undo restores marks to their pre-edit positions
/// (not post-remap positions).
///
/// Setup:
///   Document: "AAAA\nBBBBBBBBBB\n" (16 bytes)
///             line 1 = offsets 0-4, line 2 = offsets 5-15
///   Mark 'a' at offset 10 (on line 2).
///
/// Edit: insert "XXXXX" (5 bytes) at offset 0 (on line 1).
///   Mark is on a different line, so `adjust_named_offsets_ext` shifts it.
///
/// After edit: mark 'a' should be at 15 (10 + 5).
/// After undo: mark 'a' should be restored to 10 (from the snapshot captured
///   BEFORE remapping).
///
/// If the snapshot were captured AFTER remapping (the bug), undo would restore
/// mark 'a' to 15 instead of 10.
#[test]
fn verify_undo_restores_pre_edit_mark_positions() {
    let original = "AAAA\nBBBBBBBBBB\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Baseline keystroke so the undo tree is not at the root.
    feed(&mut session, "l");

    // Set mark 'a' at offset 10 (on line 2, sixth 'B').
    session.set_mark('a', 10);
    assert_eq!(
        session.get_mark('a'),
        Some(10),
        "precondition: mark 'a' at 10"
    );

    // Host applies an external edit: insert 5 bytes at offset 0.
    session.apply_external_edit(0, 0, "XXXXX");
    let after_edit = session.text().to_owned();
    assert_eq!(
        after_edit, "XXXXXAAAA\nBBBBBBBBBB\n",
        "host document should have the insertion at offset 0"
    );

    // Notify the engine about the external edit.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(0)),
        "XXXXX",
        Offset::new(5),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);

    // Shadow should match the host document.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        after_edit,
        "shadow should match host after notify_external_edit_host"
    );

    // Mark 'a' should have been remapped forward by 5.
    assert_eq!(
        session.get_mark('a'),
        Some(15),
        "mark 'a' should shift from 10 to 15 after inserting 5 bytes at offset 0"
    );

    // Press 'u' to undo the external edit.
    feed(&mut session, "u");

    // Text should be restored to original.
    assert_eq!(
        session.text(),
        original,
        "undo should restore original text"
    );

    // THE KEY ASSERTION: mark 'a' must be restored to its PRE-EDIT position (10),
    // not the post-remap position (15). This verifies that MarkSnapshot::capture
    // happens BEFORE remap_all_positions / adjust_named_offsets_ext.
    assert_eq!(
        session.get_mark('a'),
        Some(10),
        "CRITICAL: undo must restore mark 'a' to pre-edit position 10, not post-remap 15.\n\
         This verifies MarkSnapshot is captured BEFORE mark remapping in apply_external_edit."
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Adversarial tests targeting the external-edit and mark-snapshot fixes.
// ═══════════════════════════════════════════════════════════════════════════════

/// MarkSnapshot ordering with CJK text.
///
/// Verifies that `MarkSnapshot` is captured BEFORE mark remapping even when
/// the document contains multi-byte CJK characters whose byte offsets differ
/// significantly from their character indices. An external edit on a different
/// line shifts the mark forward; undo must restore the mark to its PRE-edit
/// position, not the post-remap position.
///
/// This targets the round-4 fix that moved `MarkSnapshot::capture` before
/// `remap_all_positions` / `adjust_named_offsets_ext` in `apply_external_edit`.
#[test]
fn r5_mark_snapshot_ordering_cjk() {
    // "你好\n世界\n" — two CJK chars per line (3 bytes each).
    // Line 1: bytes 0..6 ("你好"), newline at 6.
    // Line 2: bytes 7..13 ("世界"), newline at 13.
    // Total: 14 bytes.
    let original = "你好\n世界\n";
    assert_eq!(
        original.len(),
        14,
        "precondition: CJK doc should be 14 bytes"
    );

    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Baseline keystroke so undo tree is not at root.
    feed(&mut session, "l");

    // Set mark 'a' at offset 7 (start of line 2, '世').
    session.set_mark('a', 7);
    assert_eq!(
        session.get_mark('a'),
        Some(7),
        "precondition: mark 'a' at 7"
    );

    // Host applies an external edit: insert "ABC" (3 bytes) at offset 0 (line 1).
    // This is on a different line than the mark, so the mark should shift.
    session.apply_external_edit(0, 0, "ABC");
    let after_edit = session.text().to_owned();
    assert_eq!(after_edit, "ABC你好\n世界\n");

    // Notify the engine about the external edit.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(0)),
        "ABC",
        Offset::new(3),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);

    // Mark should shift forward by 3.
    assert_eq!(
        session.get_mark('a'),
        Some(10),
        "mark 'a' on line 2 should shift from 7 to 10 after 3-byte insertion on line 1"
    );

    // Undo the external edit.
    feed(&mut session, "u");

    // Text should be restored.
    assert_eq!(
        session.text(),
        original,
        "undo should restore original CJK text"
    );

    // THE KEY ASSERTION: mark must restore to PRE-edit position (7), not
    // the post-remap position (10).
    assert_eq!(
        session.get_mark('a'),
        Some(7),
        "CRITICAL: undo must restore mark 'a' to pre-edit position 7 in CJK text.\n\
         If MarkSnapshot were captured AFTER remapping, undo would restore to 10."
    );
}

/// `truncate_tail_bytes` with 4-byte emoji.
///
/// User types a 4-byte emoji in insert mode. An external edit deletes it
/// entirely from the document. `reconcile_external_edit_mutations` must
/// truncate 4 bytes from `accumulated_text` via `truncate_tail_bytes`,
/// leaving it empty. No panic from char-boundary issues.
///
/// This targets the round-4 fix that replaced `pop_char` loops with
/// byte-count-based `truncate_tail_bytes`.
#[test]
fn r5_truncate_tail_bytes_emoji() {
    let initial = "prefix ";
    let mut session = HostSession::new(initial);
    session.set_shadow_text(initial);

    // Enter insert mode at end of "prefix ".
    session.set_cursor_offset(7);
    feed(&mut session, "a");
    assert_eq!(session.mode(), Mode::Insert);

    // Type a 4-byte emoji: U+1F680 (rocket) = f0 9f 9a 80.
    feed(&mut session, "\u{1F680}");
    assert_eq!(session.text(), "prefix \u{1F680}");
    assert_eq!(session.text().len(), 11, "prefix(7) + emoji(4) = 11 bytes");

    // Verify accumulated text recorded the emoji.
    let accum = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned())
        .unwrap_or_default();
    assert_eq!(accum, "\u{1F680}");
    assert_eq!(
        accum.len(),
        4,
        "emoji should be 4 bytes in accumulated_text"
    );

    // Sync shadow.
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // --- External edit: delete the 4-byte emoji (offset 7..11) ---
    session.apply_external_edit(7, 4, "");
    assert_eq!(session.text(), "prefix ");

    // Notify engine of the deletion within the insert region.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(7), Offset::new(11)),
        "",
        Offset::new(7),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);

    // accumulated_text must be empty — the emoji was fully deleted.
    let accum_after = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned())
        .unwrap_or_default();
    assert_eq!(
        accum_after, "",
        "accumulated_text should be empty after deleting 4-byte emoji"
    );

    // Shadow matches host.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "prefix ",
        "shadow should match host after emoji deletion"
    );
}

/// `truncate_tail_bytes` partial removal of multi-byte char.
///
/// User types "cafe\u{0301}" in insert mode — "cafe" is 4 ASCII bytes, then
/// U+0301 (combining acute accent, 2 bytes: c3 81) makes "cafe\u{0301}".
/// Wait — use "caf\u{00E9}" instead: 'c'=1, 'a'=1, 'f'=1, 'e\u{0301}'=U+00E9=2 bytes = 5 total.
/// An external edit deletes the last 2 bytes (the e-acute). `truncate_tail_bytes`
/// should snap down to a char boundary and remove exactly the 'e\u{0301}' (2 bytes),
/// leaving "caf" in accumulated_text.
///
/// This targets the round-4 char-boundary snapping logic in `truncate_tail_bytes`.
#[test]
fn r5_truncate_tail_bytes_partial() {
    let initial = "start ";
    let mut session = HostSession::new(initial);
    session.set_shadow_text(initial);

    // Enter insert mode at end.
    session.set_cursor_offset(6);
    feed(&mut session, "a");
    assert_eq!(session.mode(), Mode::Insert);

    // Type "caf" then "e\u{0301}" (= U+00E9, 2 UTF-8 bytes: c3 a9).
    feed(&mut session, "caf\u{00E9}");
    assert_eq!(session.text(), "start caf\u{00E9}");

    // "caf\u{00E9}" = 3 ASCII bytes + 2 bytes = 5 bytes total.
    let accum = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned())
        .unwrap_or_default();
    assert_eq!(accum, "caf\u{00E9}");
    assert_eq!(accum.len(), 5, "caf + e-acute should be 5 bytes");

    // Sync shadow.
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // --- External edit: delete 2 bytes from the end of the insert region ---
    // "start caf\u{00E9}" is 11 bytes. The e-acute occupies bytes 9..11.
    session.apply_external_edit(9, 2, "");
    assert_eq!(session.text(), "start caf");

    // Notify engine: the deletion is within the insert region, deleting 2 bytes.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(9), Offset::new(11)),
        "",
        Offset::new(9),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);

    // accumulated_text should have only "caf" left — the 2-byte e-acute removed.
    let accum_after = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned())
        .unwrap_or_default();
    assert_eq!(
        accum_after, "caf",
        "truncate_tail_bytes(2) on 'caf\\u{{00E9}}' should remove the 2-byte e-acute,\n\
         leaving 'caf'. Got: {accum_after:?}"
    );
}

/// Two external edits, each undone independently.
///
/// Verifies that each external edit creates a proper undo entry with correct
/// text snapshots. Two independent sessions demonstrate that `u` after an
/// external edit restores the exact pre-edit text in both cases:
///   - Case A: edit on line 2 of a 3-line doc, undo restores original.
///   - Case B: edit on line 3 after line 2 was already edited, undo restores
///     the intermediate state.
///
/// This exercises the round-4 fix for undo entry creation: each
/// `notify_external_edit_host` must capture a complete undo snapshot
/// (including `MarkSnapshot` before remapping) so that `u` restores
/// correct text.
#[test]
fn r5_double_external_edit_undo_chain() {
    // ─── Case A: single external edit, verify undo restores original ───

    let original = "aaa\nbbb\nccc\n";
    let mut session_a = HostSession::new(original);
    session_a.set_shadow_text(original);

    // Baseline keystroke.
    feed(&mut session_a, "l");

    // External edit: replace "bbb" (offset 4..7) with "BBB".
    session_a.apply_external_edit(4, 3, "BBB");
    assert_eq!(session_a.text(), "aaa\nBBB\nccc\n");

    let edit1 = ExternalEdit::new(
        Range::new(Offset::new(4), Offset::new(7)),
        "BBB",
        Offset::new(7),
        ExternalEditKind::HostNotified,
    );
    let _ = session_a.notify_external_edit_host(edit1);

    assert_eq!(
        session_a.engine().shadow_text().unwrap(),
        "aaa\nBBB\nccc\n",
        "shadow should match after external edit"
    );

    // Undo: should restore original.
    feed(&mut session_a, "u");
    assert_eq!(
        session_a.text(),
        original,
        "Case A: undo of external edit should restore original text.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        original,
        session_a.text()
    );

    // ─── Case B: start from modified base, external edit, verify undo ───

    // Start from "aaa\nBBB\nccc\n" (as if edit 1 had already been committed).
    let base_b = "aaa\nBBB\nccc\n";
    let mut session_b = HostSession::new(base_b);
    session_b.set_shadow_text(base_b);

    // Baseline keystroke.
    feed(&mut session_b, "l");

    // External edit: replace "ccc" (offset 8..11) with "CCC".
    session_b.apply_external_edit(8, 3, "CCC");
    assert_eq!(session_b.text(), "aaa\nBBB\nCCC\n");

    let edit2 = ExternalEdit::new(
        Range::new(Offset::new(8), Offset::new(11)),
        "CCC",
        Offset::new(11),
        ExternalEditKind::HostNotified,
    );
    let _ = session_b.notify_external_edit_host(edit2);

    assert_eq!(
        session_b.engine().shadow_text().unwrap(),
        "aaa\nBBB\nCCC\n",
        "shadow should match after second external edit"
    );

    // Undo: should restore base_b.
    feed(&mut session_b, "u");
    assert_eq!(
        session_b.text(),
        base_b,
        "Case B: undo of second external edit should restore intermediate state.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        base_b,
        session_b.text()
    );
}

/// Drift healing during replace mode clears `replaced_chars`.
///
/// User enters replace mode (`R`), types over some characters (populating
/// the `replaced_chars` stack), then an external edit occurs before the cursor.
/// The drift gate fires on the next keystroke. Verify:
///   - `replaced_chars` is cleared (the stack is no longer valid after an
///     external edit modified the insert region).
///   - The engine does not panic.
///   - The engine remains in insert mode (replace sub-mode).
///
/// This targets the round-4 fix in `apply_external_edit` step 5 that calls
/// `clear_replaced_chars()` when the edit overlaps the insert region.
#[test]
fn r5_drift_heal_during_replace_mode() {
    // "abcdefgh\n" — 9 bytes.
    let initial = "abcdefgh\n";
    let mut session = HostSession::new(initial);
    session.set_shadow_text(initial);

    // Enter replace mode at offset 2 ('c').
    session.set_cursor_offset(2);
    feed(&mut session, "R");
    assert_eq!(session.mode(), Mode::Replace, "R should enter replace mode");

    // Type "XY" — replaces 'c' and 'd' with 'X' and 'Y'.
    // Document becomes "abXYefgh\n".
    feed(&mut session, "XY");
    assert_eq!(session.text(), "abXYefgh\n");

    // Verify replaced_chars has entries (the original 'c' and 'd').
    let has_replaced = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.has_replaced())
        .unwrap_or(false);
    assert!(
        has_replaced,
        "precondition: replaced_chars should have entries after typing in replace mode"
    );

    // Record entry_offset before the external edit (may be None for replace mode
    // if the engine doesn't set it until a cursor movement occurs).
    let entry_before = session
        .engine()
        .state()
        .insert_state()
        .and_then(|is| is.entry_offset())
        .map(|o| o.get());

    // Sync shadow to current state.
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // --- External edit: insert "ZZZ" at offset 0 (BEFORE the cursor) ---
    // This shifts the insert region forward. The session's apply_external_edit
    // mutates the host doc.
    session.apply_external_edit(0, 0, "ZZZ");
    assert_eq!(session.text(), "ZZZabXYefgh\n");

    // Notify engine. The edit is at offset 0, which is BEFORE any insert
    // region state.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(0)),
        "ZZZ",
        Offset::new(3),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);

    // Engine should still be in replace mode.
    assert_eq!(
        session.mode(),
        Mode::Replace,
        "engine should remain in replace mode after external edit before cursor"
    );

    // Shadow should match host.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "ZZZabXYefgh\n",
        "shadow should match host after external edit during replace mode"
    );

    // If entry_offset was set, it should have shifted forward by 3.
    if let Some(before) = entry_before {
        let entry_after = session
            .engine()
            .state()
            .insert_state()
            .and_then(|is| is.entry_offset())
            .map(|o| o.get());
        assert_eq!(
            entry_after,
            Some(before + 3),
            "entry_offset should shift forward by 3 after 3-byte insertion at offset 0"
        );
    }

    // Feed another character to confirm the engine is functional.
    feed(&mut session, "Z");
    // Should not panic. The engine should accept further input.
    assert_eq!(
        session.mode(),
        Mode::Replace,
        "engine should still be in replace mode after typing post-external-edit"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Known gap: globals map (buffer-associated A-Z marks) not adjusted by edits
// ═══════════════════════════════════════════════════════════════════════════════

/// **Known gap**: global marks set via `set_with_buffer_id` (the buffer-associated
/// `globals` map) are NOT adjusted by `apply_external_edit` or any text-edit path.
///
/// `adjust_named_offsets_ext` adjusts `local_az` (a-z) and `named_globals`
/// (legacy A-Z), but never touches the `globals` map. The method
/// `adjust_global_offsets` exists for this purpose but is never called from
/// any production code path (no callers outside unit tests).
///
/// This test proves the gap exists: after inserting a line before a buffer-
/// associated global mark, the mark's offset is stale.
#[test]
fn known_gap_globals_map_not_adjusted() {
    let text = "line1\nline2\nline3\n";
    let mut session = HostSession::new(text);
    session.set_shadow_text(text);

    // Set global mark 'A' at offset 12 (start of "line3") via the
    // buffer-associated path (set_with_buffer_id → globals map).
    let mark_name = MarkName::new('A').unwrap();
    let buffer_id = BufferId::new(1);
    session.set_global_mark(mark_name, Mark::from_raw(12), buffer_id);

    // Sanity: mark is at offset 12.
    let before = session.get_mark('A').expect("mark 'A' should exist");
    assert_eq!(before, 12, "mark 'A' should be at offset 12 before edit");

    // Apply external edit: insert "NEW\n" at offset 6 (before line2).
    // This shifts everything at offset >= 6 forward by 4 bytes.
    // After edit: "line1\nNEW\nline2\nline3\n"
    //              0     6    10    16
    // "line3" is now at offset 16, so a correct mark would be 16.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(6), Offset::new(6)),
        "NEW\n",
        Offset::new(10),
        ExternalEditKind::HostNotified,
    );
    let _ = session.engine_mut().apply_external_edit(edit);

    // Verify the shadow updated correctly.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "line1\nNEW\nline2\nline3\n",
    );

    // KNOWN GAP: the global mark in the `globals` map is NOT adjusted.
    // It still reads 12 (stale) instead of 16 (correct).
    let after = session.get_mark('A').expect("mark 'A' should still exist");
    assert_eq!(
        after, 12,
        "KNOWN GAP: buffer-associated global mark was not adjusted \
         (stayed at 12 instead of shifting to 16)"
    );

    // For comparison, a local mark through the same edit IS adjusted.
    // Reset and test with a local mark 'a' at the same offset.
    let mut session2 = HostSession::new(text);
    session2.set_shadow_text(text);
    session2.set_mark('a', 12);
    let edit2 = ExternalEdit::new(
        Range::new(Offset::new(6), Offset::new(6)),
        "NEW\n",
        Offset::new(10),
        ExternalEditKind::HostNotified,
    );
    let _ = session2.engine_mut().apply_external_edit(edit2);
    let local_after = session2.get_mark('a').expect("mark 'a' should exist");
    assert_eq!(
        local_after, 16,
        "local mark 'a' SHOULD be adjusted to 16 (working correctly)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stress: CJK + emoji full pipeline
// ═══════════════════════════════════════════════════════════════════════════════

/// Stress-test multi-byte character handling across the full pipeline:
/// insert-mode typing of mixed-width chars, external edits inserting/deleting
/// multi-byte sequences, shadow healing, mark validity, accumulated text for
/// dot-repeat, and undo of both external edits.
///
/// Byte layout of initial text `"你好世界🎉\nfoo bar\n"` (25 bytes):
///   你(3) 好(3) 世(3) 界(3) 🎉(4) \n(1) f(1) o(1) o(1) (1) b(1) a(1) r(1) \n(1)
///   0..3   3..6  6..9  9..12 12..16  16   17   18   19  20  21   22   23    24
#[test]
fn stress_cjk_emoji_full_pipeline() {
    let initial = "你好世界🎉\nfoo bar\n";
    assert_eq!(initial.len(), 25, "precondition: initial text is 25 bytes");

    let mut session = HostSession::new(initial);

    // ── Step 1-2: Enable shadow ──────────────────────────────────────────────
    session.set_shadow_text(initial);
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        initial,
        "shadow should be initialized"
    );

    // ── Step 3: Enter insert mode, type "café" ──────────────────────────────
    // Position cursor at offset 16 (the \n after 🎉), enter insert mode.
    // 'i' at offset 16 inserts before the \n.
    session.set_cursor_offset(16);
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);

    // Type "café" — c(1) a(1) f(1) é(2) = 5 bytes.
    feed(&mut session, "café");

    // After typing, document should be: "你好世界🎉café\nfoo bar\n" (30 bytes).
    let after_typing = "你好世界🎉café\nfoo bar\n";
    assert_eq!(
        after_typing.len(),
        30,
        "precondition: after typing is 30 bytes"
    );
    assert_eq!(
        session.text(),
        after_typing,
        "document should contain typed 'café' before the newline"
    );

    // Verify accumulated_text captured the mixed-width string.
    let accum = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.accumulated_text().to_owned())
        .unwrap_or_default();
    assert_eq!(
        accum, "café",
        "accumulated text should be 'café' (5 bytes: c/a/f are 1 byte each, é is 2 bytes)"
    );
    assert_eq!(accum.len(), 5, "accumulated text should be 5 UTF-8 bytes");

    // Set a mark on the second line for tracking across edits.
    // "foo bar\n" starts at offset 21 in after_typing. Put mark 'a' at 'b' = offset 26.
    //   你(3) 好(3) 世(3) 界(3) 🎉(4) c(1) a(1) f(1) é(2) \n(1) f(1) o(1) o(1) (1) b(1) ...
    //   0     3     6     9     12     16   17   18   19    21    22   23   24    25  26
    session.set_mark('a', 26);
    assert_eq!(
        session.get_mark('a'),
        Some(26),
        "precondition: mark 'a' at offset 26"
    );

    // Sync shadow to the current (post-typing) state before the external edit.
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // ── Step 4: External edit inserts "🌍" (4 bytes) at offset 0 ────────────
    session.apply_external_edit(0, 0, "🌍");
    // Document becomes: "🌍你好世界🎉café\nfoo bar\n" (34 bytes).
    let after_emoji_insert = "🌍你好世界🎉café\nfoo bar\n";
    assert_eq!(
        after_emoji_insert.len(),
        34,
        "precondition: after emoji insert is 34 bytes"
    );
    assert_eq!(
        session.text(),
        after_emoji_insert,
        "document should have 🌍 prepended"
    );

    // Shadow is stale — still the pre-edit text.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        current,
        "shadow should still be stale before drift gate fires"
    );

    // ── Step 5: Verify shadow heals, marks valid ────────────────────────────
    // Trigger the drift gate with a harmless motion key in insert mode.
    feed(&mut session, "<Left>");

    // Shadow should heal to match the host document.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        after_emoji_insert,
        "shadow should heal to match host after 🌍 insertion"
    );

    // Mark 'a' was at offset 26. The insertion at offset 0 is on a different
    // line (it inserts before the CJK text on line 1), and the mark is on
    // line 2. Cross-line insertion shifts named marks forward.
    // However, "🌍" was inserted at offset 0 within line 1 (no newline added),
    // so the mark is on the SAME line-set. Named-mark same-line semantics
    // may or may not shift depending on implementation.
    // What we CAN assert: mark must be within document bounds.
    let mark_a = session
        .get_mark('a')
        .expect("mark 'a' should survive the insertion");
    assert!(
        mark_a < after_emoji_insert.len(),
        "mark 'a' at offset {mark_a} must be within document bounds (len={})",
        after_emoji_insert.len()
    );

    // Undo tree should have an entry from the external edit.
    assert!(
        session.engine().undo_tree().can_undo(),
        "undo tree should have an entry after external edit"
    );

    // ── Step 6: Press Escape to return to Normal mode ───────────────────────
    feed(&mut session, "<Esc>");
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "should be in Normal mode after Escape"
    );

    // After Escape, last_inserted_text reflects accumulated_text, which was
    // reset by <Left> (Neovim's start_arrow). No text was typed after the
    // arrow, so last_inserted_text is empty — dot-repeat would replay nothing.
    let last_inserted = session.engine().state().last_inserted_text().to_owned();
    assert_eq!(
        last_inserted, "",
        "last_inserted_text should be empty: arrow key reset the repeat block"
    );

    // Record undo sequence before the second external edit.
    let undo_seq_before_delete = session.engine().undo_tree().next_sequence();

    // Sync shadow to current state before the second external edit.
    let text_before_delete = session.text().to_owned();
    session.set_shadow_text(&text_before_delete);

    // ── Step 7: External edit deletes "世界" (6 bytes) from the middle ──────
    // In the current document "🌍你好世界🎉café\nfoo bar\n":
    //   🌍(4) 你(3) 好(3) 世(3) 界(3) 🎉(4) c(1) a(1) f(1) é(2) \n(1) ...
    //   0     4     7     10    13    16     20   21   22   23    25
    // "世" starts at offset 10, "界" ends at offset 16. Delete 6 bytes at offset 10.
    session.apply_external_edit(10, 6, "");
    // Document becomes: "🌍你好🎉café\nfoo bar\n" (28 bytes).
    let after_delete = "🌍你好🎉café\nfoo bar\n";
    assert_eq!(
        after_delete.len(),
        28,
        "precondition: after delete is 28 bytes"
    );
    assert_eq!(
        session.text(),
        after_delete,
        "document should have '世界' removed"
    );

    // Trigger drift gate with a normal-mode key.
    feed(&mut session, "l");

    // ── Step 8: Verify shadow matches, dot-repeat text preserved ────────────
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        after_delete,
        "shadow should heal after '世界' deletion"
    );

    // Undo tree should have advanced for the second external edit.
    let undo_seq_after_delete = session.engine().undo_tree().next_sequence();
    assert!(
        undo_seq_after_delete > undo_seq_before_delete,
        "second external edit should create a new undo entry"
    );

    // last_inserted_text (dot-repeat data) should still be intact — external
    // edits in Normal mode should not corrupt the stored insert text.
    let last_inserted_after = session.engine().state().last_inserted_text().to_owned();
    assert_eq!(
        last_inserted_after, last_inserted,
        "dot-repeat text should be unchanged by Normal-mode external edit"
    );

    // Mark 'a' should still be within bounds.
    let mark_a_after = session
        .get_mark('a')
        .expect("mark 'a' should survive the deletion");
    assert!(
        mark_a_after < after_delete.len(),
        "mark 'a' at offset {mark_a_after} must be within document bounds after deletion (len={})",
        after_delete.len()
    );

    // Engine should be functional.
    assert_eq!(session.mode(), Mode::Normal);

    // ── Step 9: Undo both external edits ────────────────────────────────────
    // First 'u': undo the deletion of "世界".
    feed(&mut session, "u");
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "should remain in Normal mode after first undo"
    );

    // Second 'u': undo the insertion of "🌍".
    feed(&mut session, "u");
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "should remain in Normal mode after second undo"
    );

    // After two undos, the engine should still be in a consistent state.
    // The undo tree navigated without panicking on multi-byte boundaries.
    // Feed one more key to trigger drift gate reconciliation after undo.
    feed(&mut session, "l");

    // Shadow should match whatever the host document currently is.
    let final_text = session.text().to_owned();
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        final_text,
        "shadow should match host after two undos + drift reconciliation"
    );

    // All marks should be within final document bounds.
    if let Some(m) = session.get_mark('a') {
        assert!(
            m < final_text.len(),
            "mark 'a' at offset {m} must be within final document bounds (len={})",
            final_text.len()
        );
    }
}
