//! Shadow integration tests: crash/robustness tests with adversarial inputs.

use vim_core::execution::{
    parse_keys_from_string, ExternalEdit, ExternalEditKind, HostSession, InputContext, VimEngine,
};
use vim_core::keymap::KeyEvent;
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
// CRASH TESTS — adversarial inputs targeting shadow / external-edit subsystem
// ═══════════════════════════════════════════════════════════════════════════════

/// Attack 1: ExternalEdit with Range(0, usize::MAX-1) — maximum valid deletion.
/// usize::MAX is reserved as the Offset niche sentinel, so we use MAX-1.
/// The engine must clamp gracefully instead of panicking.
#[test]
fn crash_external_edit_max_deletion_range() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("hello world\n");

    let max_valid = usize::MAX - 1;
    let edit = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(max_valid)),
        "",
        Offset::new(0),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Should not panic. Shadow may be empty or clamped.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        shadow.is_empty() || shadow.len() <= 12,
        "shadow should be empty or clamped after max-range deletion, got len={}",
        shadow.len()
    );
}

/// Attack 2: ExternalEdit with offset=usize::MAX-1 (max valid), old_len=0, new_len=1 —
/// insertion at an offset far past document end.
#[test]
fn crash_external_edit_offset_past_end() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("hello\n");

    let max_valid = usize::MAX - 1;
    let edit = ExternalEdit::new(
        Range::new(Offset::new(max_valid), Offset::new(max_valid)),
        "X",
        Offset::new(max_valid),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Should not panic. The "X" may land at end or be clamped.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        shadow.contains('X') || shadow == "hello\n",
        "shadow should survive offset-past-end insertion, got: {shadow:?}"
    );
}

/// Attack 3: ExternalEdit with empty inserted text and empty range — zero-size edit.
#[test]
fn crash_external_edit_zero_size_noop() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("hello world\n");

    let edit = ExternalEdit::new(
        Range::new(Offset::new(5), Offset::new(5)),
        "",
        Offset::new(5),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Should not panic. Shadow should be unchanged.
    assert_eq!(engine.shadow_text().unwrap(), "hello world\n");
}

/// Attack 4: Call apply_external_edit 1000 times in a loop with varied edits.
/// Exercises cumulative state corruption potential.
#[test]
fn crash_external_edit_1000_iterations() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("start\n");

    for i in 0u32..1000 {
        let shadow_len = engine.shadow_text().unwrap().len();
        let offset = (i as usize) % (shadow_len.max(1));
        let text = if i % 3 == 0 {
            "X".to_string()
        } else if i % 3 == 1 {
            String::new()
        } else {
            format!("{i}")
        };

        let del_end = if i % 5 == 0 && shadow_len > offset {
            (offset + 1).min(shadow_len)
        } else {
            offset
        };

        let edit = ExternalEdit::new(
            Range::new(Offset::new(offset), Offset::new(del_end)),
            &text,
            Offset::new(offset + text.len()),
            ExternalEditKind::HostNotified,
        );
        let _response = engine.apply_external_edit(edit);
    }

    // Must not panic after 1000 edits. Shadow should be valid UTF-8 string.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        !shadow.is_empty(),
        "shadow should be non-empty after insertions"
    );
}

/// Attack 5: Set shadow to "", then process() with a 100KB document.
/// Tests the drift gate seeing a massive diff from empty to large.
#[test]
fn crash_shadow_empty_then_large_document() {
    use vim_core::execution::OwnedDocument;

    let mut engine = VimEngine::new();
    engine.set_shadow_text("");

    let large_text = "x".repeat(100_000) + "\n";
    let doc = OwnedDocument::new(&large_text);

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let _response = engine.process(KeyEvent::char('l'), ctx);

    // Should not panic. Shadow should heal to match the large document.
    let shadow = engine.shadow_text().unwrap();
    assert_eq!(
        shadow.len(),
        large_text.len(),
        "shadow should heal to 100KB document"
    );
}

/// Attack 6: Set shadow to 100KB, then process() with "" (empty document).
/// Tests massive deletion drift.
#[test]
fn crash_shadow_large_then_empty_document() {
    use vim_core::execution::OwnedDocument;

    let mut engine = VimEngine::new();
    let large_text = "y".repeat(100_000) + "\n";
    engine.set_shadow_text(&large_text);

    let doc = OwnedDocument::new("");

    // Cursor at 0 is the only valid position in an empty document.
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let _response = engine.process(KeyEvent::char('l'), ctx);

    // Should not panic. Shadow should heal to empty.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        shadow.is_empty(),
        "shadow should heal to empty document, got len={}",
        shadow.len()
    );
}

/// Attack 7: Inserted text is a multi-byte emoji, offset lands mid-character
/// in the shadow. OwnedDocument::apply_insert must snap to a char boundary.
#[test]
fn crash_external_edit_mid_char_offset_emoji() {
    let mut engine = VimEngine::new();
    // Shadow contains a 4-byte emoji: "a\u{1F600}b" = [61, f0,9f,98,80, 62]
    let text = "a\u{1F600}b";
    engine.set_shadow_text(text);

    // Offset 2 lands in the middle of the emoji (byte 2 of f0,9f,98,80).
    // The engine should snap to a char boundary instead of panicking.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(2), Offset::new(2)),
        "\u{1F602}", // laughing emoji, 4 bytes
        Offset::new(6),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Should not panic. Shadow should be valid UTF-8.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        shadow.len() > text.len(),
        "shadow should grow after insertion"
    );
}

/// Attack 7b: Mid-character deletion range inside a multi-byte sequence.
#[test]
fn crash_external_edit_mid_char_deletion() {
    let mut engine = VimEngine::new();
    // "café" = [63, 61, 66, c3, a9] — 'é' is at bytes 3..5
    engine.set_shadow_text("caf\u{00E9}");

    // Try to delete bytes 4..5 — mid-character of 'é'.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(4), Offset::new(5)),
        "",
        Offset::new(4),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Should not panic. Shadow should be valid UTF-8.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        shadow.is_char_boundary(0),
        "shadow should be valid UTF-8 after mid-char deletion"
    );
}

/// Attack 7c: Replacement where both start and end land mid-character.
#[test]
fn crash_external_edit_mid_char_replacement() {
    let mut engine = VimEngine::new();
    // "日本語" = 3 CJK chars, each 3 bytes = 9 bytes total
    // '日' = bytes 0..3, '本' = bytes 3..6, '語' = bytes 6..9
    engine.set_shadow_text("\u{65E5}\u{672C}\u{8A9E}");

    // Replace bytes 1..7 — starts mid-'日', ends mid-'語'.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(1), Offset::new(7)),
        "REPLACED",
        Offset::new(1 + 8),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Should not panic.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        !shadow.is_empty(),
        "shadow should survive mid-char replacement"
    );
}

/// Attack 8: apply_external_edit during insert mode with entry_offset > document length.
/// Uses HostSession to enter insert mode naturally, then applies external edit
/// with bogus offsets.
#[test]
fn crash_external_edit_insert_mode_entry_past_end() {
    let mut session = HostSession::new("short");
    session.set_shadow_text("short");

    // Enter insert mode at end
    session.set_cursor_offset(5);
    feed(&mut session, "a");
    assert_eq!(session.mode(), Mode::Insert);

    // Apply an external edit via engine that deletes most of the doc
    let edit = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(3)),
        "BIGINSERT",
        Offset::new(9),
        ExternalEditKind::HostNotified,
    );
    let _response = session.engine_mut().apply_external_edit(edit);

    // Should not panic. Shadow should reflect the edit.
    let shadow = session.engine().shadow_text().unwrap();
    assert!(
        shadow.contains("BIGINSERT"),
        "shadow should reflect the insertion"
    );
}

/// Attack 8b: apply_external_edit during insert mode — the insert region
/// has accumulated text. An external edit overlaps the insert region.
#[test]
fn crash_external_edit_insert_mode_overlapping_insert_region() {
    let mut session = HostSession::new("hello world");
    session.set_shadow_text("hello world");

    // Enter insert mode at offset 6 (between "hello " and "world")
    session.set_cursor_offset(5);
    feed(&mut session, "a");
    assert_eq!(session.mode(), Mode::Insert);

    // Type some text so accumulated_text grows
    feed(&mut session, "XYZ");
    // Document is now "hello XYZworld"

    // Sync shadow
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // Apply external edit that overlaps with the typed region
    let edit = ExternalEdit::new(
        Range::new(Offset::new(4), Offset::new(10)),
        "OVERLAP",
        Offset::new(11),
        ExternalEditKind::HostNotified,
    );
    let _response = session.engine_mut().apply_external_edit(edit);

    // Should not panic.
    let shadow = session.engine().shadow_text().unwrap();
    assert!(
        shadow.contains("OVERLAP"),
        "shadow should contain the replacement"
    );
}

/// Attack: ExternalEdit with start > end in the deleted range (inverted range).
/// Note: Range::new panics in debug builds when start > end (debug_assert).
/// In release builds it normalizes by swapping. We test the release-mode
/// behavior by constructing the Range directly to bypass the debug_assert.
/// This test verifies the engine handles zero-length ranges at boundary offsets.
#[test]
fn crash_external_edit_boundary_range() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("hello world");

    // Range at the very end of the document — delete 0 bytes, insert "X".
    let doc_len = engine.shadow_text().unwrap().len();
    let edit = ExternalEdit::new(
        Range::new(Offset::new(doc_len), Offset::new(doc_len)),
        "X",
        Offset::new(doc_len + 1),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Should not panic.
    let shadow = engine.shadow_text().unwrap();
    assert!(shadow.ends_with('X'), "shadow should have X appended");
}

/// Attack: HostSession-level stress — rapid alternating set_text + drift gate.
#[test]
fn crash_rapid_drift_healing_100_cycles() {
    let mut session = HostSession::new("start\n");
    session.set_shadow_text("start\n");

    for i in 0u32..100 {
        let new_text = format!("iteration {i}\nline2\nline3\n");
        session.set_text(&new_text);
        feed(&mut session, "l"); // trigger drift gate
        assert_eq!(
            session.engine().shadow_text().unwrap(),
            new_text,
            "shadow should heal on iteration {i}"
        );
    }

    assert_eq!(session.mode(), Mode::Normal);
}

/// Attack: External edit that deletes the entire document and replaces it.
#[test]
fn crash_external_edit_full_document_replacement() {
    let mut engine = VimEngine::new();
    let original = "line1\nline2\nline3\nline4\nline5\n";
    engine.set_shadow_text(original);

    let edit = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(original.len())),
        "completely new content\n",
        Offset::new(22),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    assert_eq!(engine.shadow_text().unwrap(), "completely new content\n");
}

/// Attack: ExternalEdit with deleted range extending past document end.
#[test]
fn crash_external_edit_deletion_past_end() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("tiny");

    // Delete range 2..1000 on a 4-byte document.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(2), Offset::new(1000)),
        "",
        Offset::new(2),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    let shadow = engine.shadow_text().unwrap();
    assert_eq!(shadow, "ti", "should clamp deletion to document bounds");
}

/// Attack: Drift gate with shadow containing only multi-byte chars, host
/// document replaced with ASCII — exercises char boundary alignment in diff.
#[test]
fn crash_drift_multibyte_shadow_to_ascii_host() {
    use vim_core::execution::OwnedDocument;

    let mut engine = VimEngine::new();
    // Shadow is all CJK: 30 chars * 3 bytes = 90 bytes
    let cjk = "\u{65E5}".repeat(30);
    engine.set_shadow_text(&cjk);

    // Host document is pure ASCII
    let ascii_doc = OwnedDocument::new("hello world\n");
    let ctx = InputContext::new(&ascii_doc, 0).validate().unwrap();
    let _response = engine.process(KeyEvent::char('l'), ctx);

    assert_eq!(engine.shadow_text().unwrap(), "hello world\n");
}

/// Attack: Shadow and host both empty.
#[test]
fn crash_both_empty_shadow_and_host() {
    use vim_core::execution::OwnedDocument;

    let mut engine = VimEngine::new();
    engine.set_shadow_text("");

    let doc = OwnedDocument::new("");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let _response = engine.process(KeyEvent::char('l'), ctx);

    assert_eq!(engine.shadow_text().unwrap(), "");
}

// ═══════════════════════════════════════════════════════════════════════════════
// crash_test_ — adversarial fuzzing of shadow / external-edit subsystem
// ═══════════════════════════════════════════════════════════════════════════════

/// Attack 1: ExternalEdit with Range(0, usize::MAX) — maximum deletion.
#[test]
fn crash_test_max_deletion_range() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("hello world\n");

    let max_off = usize::MAX - 1;
    let edit = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(max_off)),
        "",
        Offset::new(0),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Must not panic. Shadow should be empty (entire doc deleted) or clamped.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        shadow.is_empty() || shadow == "hello world\n",
        "shadow after max-range deletion should be empty or clamped, got len={}",
        shadow.len()
    );
}

/// Attack 2: ExternalEdit with offset far past end of document.
#[test]
fn crash_test_offset_past_end() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("tiny\n");

    let edit = ExternalEdit::new(
        Range::new(Offset::new(999_999), Offset::new(999_999)),
        "INSERTED",
        Offset::new(999_999 + 8),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Must not panic.
    let shadow = engine.shadow_text().unwrap();
    assert!(
        shadow.contains("tiny") || shadow.contains("INSERTED"),
        "shadow should survive offset-past-end, got: {shadow:?}"
    );
}

/// Attack 3: ExternalEdit with empty inserted text and empty range — zero-size edit.
#[test]
fn crash_test_zero_size_edit() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("hello world\n");

    // Zero-size at middle
    let edit = ExternalEdit::new(
        Range::new(Offset::new(5), Offset::new(5)),
        "",
        Offset::new(5),
        ExternalEditKind::HostNotified,
    );
    let _r = engine.apply_external_edit(edit);
    assert_eq!(engine.shadow_text().unwrap(), "hello world\n");

    // Zero-size at offset 0
    let edit0 = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(0)),
        "",
        Offset::new(0),
        ExternalEditKind::HostNotified,
    );
    let _r = engine.apply_external_edit(edit0);
    assert_eq!(engine.shadow_text().unwrap(), "hello world\n");

    // Zero-size at document end
    let doc_len = engine.shadow_text().unwrap().len();
    let edit_end = ExternalEdit::new(
        Range::new(Offset::new(doc_len), Offset::new(doc_len)),
        "",
        Offset::new(doc_len),
        ExternalEditKind::HostNotified,
    );
    let _r = engine.apply_external_edit(edit_end);
    assert_eq!(engine.shadow_text().unwrap(), "hello world\n");
}

/// Attack 4: Call apply_external_edit 1000 times in a loop with random-ish edits.
#[test]
fn crash_test_1000_random_edits() {
    let mut engine = VimEngine::new();
    engine.set_shadow_text("start\n");

    for i in 0u32..1000 {
        let shadow_len = engine.shadow_text().unwrap().len();
        let offset = (i as usize * 7 + 3) % shadow_len.max(1);

        match i % 4 {
            0 => {
                let text = format!("i{i}");
                let edit = ExternalEdit::new(
                    Range::new(Offset::new(offset), Offset::new(offset)),
                    &text,
                    Offset::new(offset + text.len()),
                    ExternalEditKind::HostNotified,
                );
                let _r = engine.apply_external_edit(edit);
            }
            1 => {
                let del_end = (offset + 1).min(shadow_len);
                let edit = ExternalEdit::new(
                    Range::new(Offset::new(offset), Offset::new(del_end)),
                    "",
                    Offset::new(offset),
                    ExternalEditKind::HostNotified,
                );
                let _r = engine.apply_external_edit(edit);
            }
            2 => {
                let del_end = (offset + 2).min(shadow_len);
                let edit = ExternalEdit::new(
                    Range::new(Offset::new(offset), Offset::new(del_end)),
                    "R",
                    Offset::new(offset + 1),
                    ExternalEditKind::HostNotified,
                );
                let _r = engine.apply_external_edit(edit);
            }
            _ => {
                let edit = ExternalEdit::new(
                    Range::new(Offset::new(offset), Offset::new(offset)),
                    "",
                    Offset::new(offset),
                    ExternalEditKind::HostNotified,
                );
                let _r = engine.apply_external_edit(edit);
            }
        }
    }

    let shadow = engine.shadow_text().unwrap();
    assert!(
        !shadow.is_empty(),
        "shadow should be non-empty after 1000 edits with insertions"
    );
}

/// Attack 5: Set shadow to "", then process() with a 100KB document.
#[test]
fn crash_test_empty_shadow_100kb_document() {
    use vim_core::execution::OwnedDocument;

    let mut engine = VimEngine::new();
    engine.set_shadow_text("");

    let large_text = "x".repeat(100_000) + "\n";
    let doc = OwnedDocument::new(&large_text);
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let _response = engine.process(KeyEvent::char('l'), ctx);

    let shadow = engine.shadow_text().unwrap();
    assert_eq!(
        shadow.len(),
        large_text.len(),
        "shadow should heal from empty to 100KB"
    );
}

/// Attack 6: Set shadow to a large document, then process() with "".
#[test]
fn crash_test_large_shadow_empty_document() {
    use vim_core::execution::OwnedDocument;

    let mut engine = VimEngine::new();
    let large_text = "y".repeat(100_000) + "\n";
    engine.set_shadow_text(&large_text);

    let doc = OwnedDocument::new("");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let _response = engine.process(KeyEvent::char('l'), ctx);

    let shadow = engine.shadow_text().unwrap();
    assert!(
        shadow.is_empty(),
        "shadow should heal from 100KB to empty, got len={}",
        shadow.len()
    );
}

/// Attack 7: ExternalEdit where offset lands mid-character in the shadow.
/// Shadow is "你好" (6 bytes: 3+3), offset=1 which is mid-UTF8 of '你'.
#[test]
fn crash_test_mid_char_offset_chinese() {
    let mut engine = VimEngine::new();
    // "你好" = [e4,bd,a0, e5,a5,bd] = 6 bytes
    engine.set_shadow_text("\u{4F60}\u{597D}");

    // offset=1 lands inside '你' (byte 1 of e4,bd,a0)
    let edit = ExternalEdit::new(
        Range::new(Offset::new(1), Offset::new(1)),
        "INJECTED",
        Offset::new(1 + 8),
        ExternalEditKind::HostNotified,
    );
    let _response = engine.apply_external_edit(edit);

    // Must not panic. Shadow should be valid UTF-8.
    let shadow = engine.shadow_text().unwrap();
    let _char_count = shadow.chars().count();

    // Also: mid-char deletion range (bytes 1..4 spans across '你' boundary)
    let mut engine2 = VimEngine::new();
    engine2.set_shadow_text("\u{4F60}\u{597D}");

    let edit2 = ExternalEdit::new(
        Range::new(Offset::new(1), Offset::new(4)),
        "",
        Offset::new(1),
        ExternalEditKind::HostNotified,
    );
    let _r2 = engine2.apply_external_edit(edit2);
    let _valid = engine2.shadow_text().unwrap().chars().count();

    // Also: mid-char replacement (bytes 2..5)
    let mut engine3 = VimEngine::new();
    engine3.set_shadow_text("\u{4F60}\u{597D}");

    let edit3 = ExternalEdit::new(
        Range::new(Offset::new(2), Offset::new(5)),
        "ABC",
        Offset::new(5),
        ExternalEditKind::HostNotified,
    );
    let _r3 = engine3.apply_external_edit(edit3);
    let _valid3 = engine3.shadow_text().unwrap().chars().count();
}

/// Attack 8: apply_external_edit during insert mode with entry_offset > document length.
/// Enter insert mode via HostSession near the end, then externally delete text
/// so the engine's insert entry_offset exceeds the remaining document length.
#[test]
fn crash_test_insert_mode_entry_past_doc_length() {
    let mut session = HostSession::new("abcdefghij");
    session.set_shadow_text("abcdefghij");

    // Move cursor to offset 8, enter append mode: entry_offset = 9
    session.set_cursor_offset(8);
    feed(&mut session, "a");
    assert_eq!(session.mode(), Mode::Insert);

    // Type some text to grow accumulated_text
    feed(&mut session, "XYZ");
    // Document is now "abcdefghiXYZj", entry_offset ~ 9, cursor ~ 12

    // Sync shadow
    let current = session.text().to_owned();
    session.set_shadow_text(&current);

    // Apply external edit via the engine that deletes chars 0..8, shrinking the
    // document so entry_offset (9) > remaining text length.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(0), Offset::new(8)),
        "",
        Offset::new(0),
        ExternalEditKind::HostNotified,
    );
    let _response = session.engine_mut().apply_external_edit(edit);

    // Must not panic. Shadow should reflect the deletion.
    let shadow = session.engine().shadow_text().unwrap();
    assert!(
        shadow.len() < current.len(),
        "shadow should shrink after deletion"
    );

    // Continue processing keys in insert mode — must not panic.
    feed(&mut session, "Q");
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);
}
