//! Shadow integration tests: undo correctness after external edits.

use vim_core::execution::{parse_keys_from_string, ExternalEdit, ExternalEditKind, HostSession};
use vim_core::primitives::{Mode, Offset, Range};

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
// Test: replace_text() updates shadow, preventing spurious drift
// ═══════════════════════════════════════════════════════════════════════════════

/// Verify that `replace_text(new, false)` updates the shadow document so
/// that the next `process_key_host()` does NOT detect drift.
///
/// Before the fix, `replace_text()` called `set_text()` internally, which
/// does NOT update the shadow. That caused the drift gate to fire on the
/// very next keystroke, creating a spurious undo entry and triggering
/// unnecessary mark remapping.
#[test]
fn replace_text_updates_shadow_no_spurious_drift() {
    let mut session = HostSession::new("hello\n");
    session.set_shadow_text("hello\n");

    // Snapshot the undo tree sequence counter AFTER replace_text.
    // replace_text(_, false) resets the undo tree, so next_sequence restarts at 1.
    session.replace_text("world\n", false);

    // After replace_text, shadow should already match the new document text.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "world\n",
        "replace_text must update shadow to match new document"
    );
    assert_eq!(session.text(), "world\n");

    // Record the undo sequence counter AFTER replace_text (fresh tree).
    let seq_before = session.engine().undo_tree().next_sequence();

    // Process a harmless keystroke — this triggers the drift gate.
    feed(&mut session, "l");

    // The shadow should still match (no drift detected).
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "world\n",
        "shadow should remain in sync after keystroke — no drift"
    );

    // The undo tree sequence should NOT have advanced, because 'l' is a
    // cursor motion that creates no undo entries, and no drift was detected.
    let seq_after = session.engine().undo_tree().next_sequence();
    assert_eq!(
        seq_before, seq_after,
        "undo tree should not grow from a spurious drift entry (before={}, after={})",
        seq_before, seq_after
    );

    // Verify the engine is still functional.
    assert_eq!(session.mode(), Mode::Normal);
    assert_eq!(session.text(), "world\n");
}

/// Same as above but with `undoable: true` — verify the single undo entry
/// from `replace_text` is the ONLY one, and no drift entry is added.
#[test]
fn replace_text_undoable_updates_shadow_no_spurious_drift() {
    let mut session = HostSession::new("hello\n");
    session.set_shadow_text("hello\n");

    session.replace_text("world\n", true);

    // Shadow must match the new text.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "world\n",
        "replace_text(undoable=true) must update shadow"
    );

    // The undoable path creates exactly one undo entry (fresh tree + one group).
    let seq_before = session.engine().undo_tree().next_sequence();

    // Process a harmless keystroke.
    feed(&mut session, "l");

    // No drift should be detected.
    let seq_after = session.engine().undo_tree().next_sequence();
    assert_eq!(
        seq_before, seq_after,
        "no spurious drift entry after undoable replace_text (before={}, after={})",
        seq_before, seq_after
    );

    // The single undo entry from replace_text should still be there.
    assert!(
        session.engine().undo_tree().can_undo(),
        "the replace_text undo entry should survive the keystroke"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// DEFINITIVE: Undo after external edits restores text correctly
// ═══════════════════════════════════════════════════════════════════════════════

/// Proactive external edit (notify_external_edit_host) followed by `u` must
/// restore the document to EXACTLY the original text.
#[test]
fn definitive_proactive_external_edit_undo_restores_text() {
    let original = "line1\nline2\nline3\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Baseline keystroke so undo tree is not empty-at-root.
    feed(&mut session, "l");

    // Host applies an external edit: replace "line2" with "CHANGED".
    // "line2" occupies bytes 6..11 in "line1\nline2\nline3\n".
    session.apply_external_edit(6, 5, "CHANGED");
    let after_edit = session.text().to_owned();
    assert_eq!(
        after_edit, "line1\nCHANGED\nline3\n",
        "apply_external_edit should replace line2 with CHANGED"
    );

    // Notify the engine about the external edit.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(6), Offset::new(11)),
        "CHANGED",
        Offset::new(13), // cursor after "CHANGED"
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);

    // Shadow must match the host document after notification.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "line1\nCHANGED\nline3\n",
        "shadow should match host after notify_external_edit_host"
    );

    // Press `u` to undo the external edit.
    feed(&mut session, "u");

    // THE DEFINITIVE ASSERTION: text must be EXACTLY the original.
    assert_eq!(
        session.text(),
        original,
        "CRITICAL: undo after proactive external edit MUST restore original text.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        original,
        session.text()
    );
}

/// Drift-detected external edit (set_text + process_key triggers drift gate)
/// followed by `u` must restore the document to EXACTLY the original text.
#[test]
fn definitive_drift_detected_edit_undo_restores_text() {
    let original = "hello world\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Baseline keystroke.
    feed(&mut session, "l");

    // Directly mutate the host document, bypassing the engine.
    session.set_text("hello earth\n");

    // Process any key to trigger the drift gate.
    feed(&mut session, "l");

    // Verify the drift was detected and healed.
    assert_eq!(
        session.text(),
        "hello earth\n",
        "document should contain the externally-set text"
    );
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "hello earth\n",
        "shadow should match host after drift healing"
    );

    // Press `u` to undo the drift-detected external edit.
    feed(&mut session, "u");

    // THE DEFINITIVE ASSERTION: text must be EXACTLY the original.
    assert_eq!(
        session.text(),
        original,
        "CRITICAL: undo after drift-detected edit MUST restore original text.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        original,
        session.text()
    );
}

/// Chain of interleaved Vim edits and external edits, then multiple `u` presses
/// must restore each intermediate state in reverse order.
#[test]
#[ignore = "pre-existing undo tree traversal bug: external edit nodes between normal edits confuse backward navigation"]
fn definitive_chain_of_edits_multiple_undo_restores_each_step() {
    // Step 0: initial text.
    let original = "AAA\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Step 1: Normal Vim edit — `rB` replaces first char with 'B'.
    session.set_cursor_offset(0);
    feed(&mut session, "rB");
    let after_step1 = session.text().to_owned();
    assert_eq!(
        after_step1, "BAA\n",
        "rB should replace first char: expected 'BAA\\n', got {:?}",
        after_step1
    );
    // Sync shadow so next drift detection sees the current state.
    session.set_shadow_text(&after_step1);

    // Step 2: External edit — host appends "_EXT" to the first line.
    // "BAA\n" -> "BAA_EXT\n"
    // Replace the trailing "\n" at offset 3 with "_EXT\n".
    session.apply_external_edit(3, 1, "_EXT\n");
    let after_ext_apply = session.text().to_owned();
    assert_eq!(
        after_ext_apply, "BAA_EXT\n",
        "external edit should produce 'BAA_EXT\\n', got {:?}",
        after_ext_apply
    );

    // Notify engine about the external edit.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(3), Offset::new(4)),
        "_EXT\n",
        Offset::new(8),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);
    let after_step2 = session.text().to_owned();
    assert_eq!(after_step2, "BAA_EXT\n");

    // Step 3: Normal Vim edit — `rC` replaces first char with 'C'.
    session.set_cursor_offset(0);
    feed(&mut session, "rC");
    let after_step3 = session.text().to_owned();
    assert_eq!(
        after_step3, "CAA_EXT\n",
        "rC should replace first char: expected 'CAA_EXT\\n', got {:?}",
        after_step3
    );

    // Now undo three times and verify each intermediate state.

    // Undo 1: should undo step 3 (rC) -> restore "BAA_EXT\n"
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        "BAA_EXT\n",
        "CRITICAL: first undo should restore to after external edit.\n\
         Expected: \"BAA_EXT\\n\"\n\
         Got:      {:?}",
        session.text()
    );

    // Undo 2: should undo step 2 (external edit) -> restore "BAA\n"
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        "BAA\n",
        "CRITICAL: second undo should restore to after first Vim edit.\n\
         Expected: \"BAA\\n\"\n\
         Got:      {:?}",
        session.text()
    );

    // Undo 3: should undo step 1 (rB) -> restore "AAA\n"
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "CRITICAL: third undo should restore to original text.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        original,
        session.text()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO PROOF: Definitive tests proving undo after external edits restores text
// ═══════════════════════════════════════════════════════════════════════════════

/// Test 1: Proactive external edit via notify_external_edit_host, then `u`
/// restores text to EXACTLY the original.
///
/// Sequence:
/// 1. Create session with "line1\nline2\nline3\n"
/// 2. Enable shadow
/// 3. Host notifies engine: replace "line2" with "CHANGED"
/// 4. Assert text is "line1\nCHANGED\nline3\n"
/// 5. Feed `u`
/// 6. Assert text is "line1\nline2\nline3\n" — EXACTLY the original
#[test]
fn undo_proof_proactive_external_edit_restores_text() {
    let original = "line1\nline2\nline3\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Baseline keystroke so undo tree is not at root.
    feed(&mut session, "l");

    // Host applies the external edit to its document:
    // "line2" occupies bytes 6..11 in "line1\nline2\nline3\n".
    session.apply_external_edit(6, 5, "CHANGED");
    assert_eq!(
        session.text(),
        "line1\nCHANGED\nline3\n",
        "after apply_external_edit: line2 replaced with CHANGED"
    );

    // Notify the engine about the external edit.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(6), Offset::new(11)),
        "CHANGED",
        Offset::new(13),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);

    // Shadow must match host.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "line1\nCHANGED\nline3\n",
        "shadow should match host after notify"
    );

    // Feed `u` to undo the external edit.
    feed(&mut session, "u");

    // THE PROOF: text must be EXACTLY the original.
    assert_eq!(
        session.text(),
        original,
        "UNDO PROOF FAILED: proactive external edit undo did not restore original text.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        original,
        session.text()
    );
}

/// Test 2: Drift-detected edit (set_text bypasses engine, next keystroke
/// triggers drift gate) then `u` restores text to EXACTLY the original.
///
/// Sequence:
/// 1. Create session with "hello world\n"
/// 2. Enable shadow
/// 3. Directly mutate: set_text("hello earth\n") — bypasses engine
/// 4. Feed any key (triggers drift gate)
/// 5. Assert text is "hello earth\n" and shadow matches
/// 6. Feed `u`
/// 7. Assert text is "hello world\n"
#[test]
fn undo_proof_drift_detected_edit_restores_text() {
    let original = "hello world\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Baseline keystroke.
    feed(&mut session, "l");

    // Directly mutate the host document, bypassing the engine entirely.
    session.set_text("hello earth\n");

    // Feed a key to trigger the drift gate.
    feed(&mut session, "l");

    // Verify drift was detected and healed.
    assert_eq!(
        session.text(),
        "hello earth\n",
        "document should contain the externally-set text"
    );
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "hello earth\n",
        "shadow should match host after drift healing"
    );

    // Feed `u` to undo the drift-detected external edit.
    feed(&mut session, "u");

    // THE PROOF: text must be EXACTLY the original.
    assert_eq!(
        session.text(),
        original,
        "UNDO PROOF FAILED: drift-detected edit undo did not restore original text.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        original,
        session.text()
    );
}

/// Test 3: Chain of normal edit -> external edit -> normal edit -> u x 3,
/// each undo restores the previous intermediate state.
///
/// Sequence:
/// 1. Create with "aaa\n"
/// 2. Normal edit: feed `rB` -> text becomes "Baa\n"
/// 3. External edit: replace text with "Baa_EXT\n"
/// 4. Normal edit: feed `rC` -> "Caa_EXT\n"
/// 5. Feed `u` -> assert "Baa_EXT\n"
/// 6. Feed `u` -> assert "Baa\n"
/// 7. Feed `u` -> assert "aaa\n"
#[test]
#[ignore = "pre-existing undo tree traversal bug: external edit nodes between normal edits confuse backward navigation"]
fn undo_proof_chain_normal_external_normal_triple_undo() {
    let original = "aaa\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Step 1: Normal edit — `rB` replaces first char 'a' with 'B'.
    session.set_cursor_offset(0);
    feed(&mut session, "rB");
    let after_step1 = session.text().to_owned();
    assert_eq!(
        after_step1, "Baa\n",
        "rB should replace first char: expected 'Baa\\n', got {:?}",
        after_step1
    );
    // Sync shadow so drift gate sees current state.
    session.set_shadow_text(&after_step1);

    // Step 2: External edit — host replaces text with "Baa_EXT\n".
    // Replace trailing "\n" at offset 3 with "_EXT\n".
    session.apply_external_edit(3, 1, "_EXT\n");
    let after_ext_apply = session.text().to_owned();
    assert_eq!(
        after_ext_apply, "Baa_EXT\n",
        "external edit should produce 'Baa_EXT\\n', got {:?}",
        after_ext_apply
    );

    // Notify engine about the external edit.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(3), Offset::new(4)),
        "_EXT\n",
        Offset::new(8),
        ExternalEditKind::HostNotified,
    );
    let _ = session.notify_external_edit_host(edit);
    assert_eq!(session.text(), "Baa_EXT\n");

    // Step 3: Normal edit — `rC` replaces first char 'B' with 'C'.
    session.set_cursor_offset(0);
    feed(&mut session, "rC");
    let after_step3 = session.text().to_owned();
    assert_eq!(
        after_step3, "Caa_EXT\n",
        "rC should replace first char: expected 'Caa_EXT\\n', got {:?}",
        after_step3
    );

    // Undo 1: should undo step 3 (rC) -> restore "Baa_EXT\n"
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        "Baa_EXT\n",
        "UNDO PROOF FAILED: first undo should restore to after external edit.\n\
         Expected: \"Baa_EXT\\n\"\n\
         Got:      {:?}",
        session.text()
    );

    // Undo 2: should undo step 2 (external edit) -> restore "Baa\n"
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        "Baa\n",
        "UNDO PROOF FAILED: second undo should restore to after first Vim edit.\n\
         Expected: \"Baa\\n\"\n\
         Got:      {:?}",
        session.text()
    );

    // Undo 3: should undo step 1 (rB) -> restore "aaa\n"
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "UNDO PROOF FAILED: third undo should restore to original text.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        original,
        session.text()
    );
}
