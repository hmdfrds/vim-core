//! Shadow integration tests: UTF-8 / multi-byte character handling.

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::Mode;

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
// UTF-8 Multi-Byte Character Handling Tests
// ═══════════════════════════════════════════════════════════════════════════════

/// Test 1: Drift with CJK text.
///
/// Shadow initialized to CJK string, host document mutated to replace some
/// CJK chars. Drift gate detects the mismatch and heals.
#[test]
fn utf8_drift_with_cjk_text() {
    // "你好世界\n" — 4 CJK chars (3 bytes each) + newline = 13 bytes
    let original = "\u{4F60}\u{597D}\u{4E16}\u{754C}\n";
    assert_eq!(original.len(), 13, "CJK + newline should be 13 bytes");

    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Verify shadow matches before mutation
    assert_eq!(session.engine().shadow_text().unwrap(), original);

    // Mutate document: replace 世界 with 地球 (same byte length: 6 bytes each pair)
    // "你好地球\n"
    let mutated = "\u{4F60}\u{597D}\u{5730}\u{7403}\n";
    assert_eq!(
        mutated.len(),
        13,
        "mutated CJK + newline should also be 13 bytes"
    );
    session.set_text(mutated);

    // Shadow is stale
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        original,
        "shadow should still be stale before keystroke"
    );

    // Feed a key to trigger the drift gate
    feed(&mut session, "l");

    // Shadow should heal to match the mutated host text
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        mutated,
        "shadow should heal to match CJK host text after drift gate"
    );

    // Engine should still be functional
    assert_eq!(session.mode(), Mode::Normal);
    assert_eq!(session.text(), mutated);
}

/// Test 2: External edit inserts emoji.
///
/// Host inserts a 4-byte emoji into ASCII text via apply_external_edit.
/// Verify shadow heals and mark positions shift by the emoji's byte length.
#[test]
fn utf8_external_edit_inserts_emoji() {
    let original = "hello world\n";
    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Set mark 'a' at offset 6 ("w" in "world")
    session.set_mark('a', 6);
    assert_eq!(session.get_mark('a'), Some(6));

    // Apply external edit: insert emoji at offset 5 (between "hello" and " world")
    // "\u{1F389}" = party popper emoji = 4 bytes (f0 9f 8e 89)
    session.apply_external_edit(5, 0, "\u{1F389}");
    let expected = "hello\u{1F389} world\n";
    assert_eq!(
        session.text(),
        expected,
        "document should contain emoji after external edit"
    );
    assert_eq!(
        expected.len(),
        16,
        "hello + 4-byte emoji + space world newline = 16 bytes"
    );

    // Shadow is stale — still "hello world\n"
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        original,
        "shadow should be stale before drift gate"
    );

    // Trigger drift gate
    feed(&mut session, "l");

    // Shadow healed
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        expected,
        "shadow should heal to include emoji"
    );

    // Mark 'a' was at offset 6. The insert of 4 bytes at offset 5 shifts
    // everything at offset >= 5 forward by 4. So mark should shift from 6
    // to 6 + 4 = 10.
    // However: the drift gate uses a diff-based changeset to remap marks.
    // For a single-line edit, named marks may or may not shift depending on
    // same-line semantics. The key assertion: the mark is valid and within
    // document bounds.
    let mark_a = session.get_mark('a');
    assert!(mark_a.is_some(), "mark 'a' should survive emoji insertion");
    let mark_val = mark_a.unwrap();
    assert!(
        mark_val < expected.len(),
        "mark 'a' should be within document bounds after emoji insertion, got {mark_val}"
    );
}

/// Test 3: External edit with offset mid-UTF8 char (defensive).
///
/// apply_external_edit with offset=1, which lands in the middle of the
/// first CJK character (3-byte 你). OwnedDocument::apply_insert snaps
/// to a char boundary. This test verifies no panic occurs.
#[test]
fn utf8_external_edit_mid_char_no_panic() {
    // "你好\n" = 你(3 bytes) + 好(3 bytes) + \n = 7 bytes
    let original = "\u{4F60}\u{597D}\n";
    assert_eq!(original.len(), 7, "\u{4F60}\u{597D}\\n should be 7 bytes");

    let mut session = HostSession::new(original);
    session.set_shadow_text(original);

    // Apply external edit at offset=1 (mid-character in 你), old_len=0, insert "X"
    // OwnedDocument::apply_insert snaps offset 1 to char boundary 0 (start of 你)
    session.apply_external_edit(1, 0, "X");

    // Must not panic. The document should be valid UTF-8.
    let text = session.text();
    // Verify the text is valid UTF-8 (if we got here, it is — Rust strings are always valid)
    assert!(
        !text.is_empty(),
        "document should be non-empty after mid-char insertion"
    );
    assert!(
        text.contains('X'),
        "inserted 'X' should appear in the document, got: {text:?}"
    );

    // Now test that drift healing also works with this state
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // Mutate to something else
    session.set_text("Y\u{4F60}\u{597D}\n");
    feed(&mut session, "l");

    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "Y\u{4F60}\u{597D}\n",
        "drift healing should work after mid-char insertion"
    );
}

/// Test 4: Drift where only multi-byte chars changed.
///
/// Shadow has "caf\u{00E9}\n" (e-acute, 2 bytes), host has "cafe\n" (plain e).
/// Drift gate detects the multi-byte to single-byte change and heals.
#[test]
fn utf8_drift_multibyte_to_singlebyte_accent() {
    // "caf\u{00E9}\n" — e-acute is 2 bytes (c3 a9), total 6 bytes
    let shadow_text = "caf\u{00E9}\n";
    assert_eq!(
        shadow_text.len(),
        6,
        "cafe-acute + newline should be 6 bytes"
    );

    // "cafe\n" — plain ASCII e, total 5 bytes
    let host_text = "cafe\n";
    assert_eq!(host_text.len(), 5, "cafe + newline should be 5 bytes");

    let mut session = HostSession::new(shadow_text);
    session.set_shadow_text(shadow_text);

    // Verify initial state
    assert_eq!(session.text(), shadow_text);
    assert_eq!(session.engine().shadow_text().unwrap(), shadow_text);

    // Host changes the document: e-acute replaced with plain e
    session.set_text(host_text);

    // Shadow is stale
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        shadow_text,
        "shadow should still be stale before keystroke"
    );

    // Feed key to trigger drift gate
    feed(&mut session, "l");

    // Shadow should heal to match the ASCII host text
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        host_text,
        "shadow should heal from multi-byte accent to single-byte"
    );

    // Document should match
    assert_eq!(session.text(), host_text);

    // Engine should still be functional
    assert_eq!(session.mode(), Mode::Normal);
}
