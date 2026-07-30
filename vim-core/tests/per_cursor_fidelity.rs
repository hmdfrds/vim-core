//! Per-cursor re-execution fidelity.
//!
//! These tests validate that the per-cursor re-execution path produces
//! correct replacement text for each cursor independently.
//! Under pure algebraic replication, the primary cursor's replacement text
//! would be blindly copied to all cursors, producing wrong results when the
//! text under each cursor differs.
//!
//! Each test documents:
//!   - What algebraic replication would produce (the bug)
//!   - What per-cursor re-execution should produce (the fix)
//!   - The specific content-dependent operator being tested
//!
//! These tests exercise the per-cursor code path through HostSession's
//! process_key_host -> execute_effect_plan -> execute_per_cursor pipeline.

#![allow(non_snake_case)]

use vim_core::execution::{parse_keys_from_string, HostSession};

// =============================================================================
// Helpers
// =============================================================================

/// Feed a Vim key-notation string into the session.
fn feed(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

/// Create a session with multi-cursor at the given byte offsets.
/// The first offset is the primary cursor.
fn session_with_cursors(text: &str, offsets: &[usize]) -> HostSession {
    let mut session = HostSession::new(text);
    if offsets.is_empty() {
        return session;
    }
    // Move primary to the first offset
    session.set_cursor_offset(offsets[0]);
    // Add secondary cursors
    for &offset in &offsets[1..] {
        session
            .add_cursor(offset)
            .expect("add_cursor should succeed");
    }
    assert_eq!(
        session.cursor_count(),
        offsets.len(),
        "cursor count mismatch after setup"
    );
    session
}

// =============================================================================
// 1. `~` (swap case) per-cursor fidelity
// =============================================================================

/// `~` on uppercase 'A' and lowercase 'b' — different toggle directions.
///
/// Document: "AaBb"
/// Cursors at 0 ('A') and 2 ('B').
///
/// Per-cursor re-execution (correct):
///   cursor@0: 'A'(0x41) -> Replace{[0..1], "a"} — uppercase toggled to lowercase
///   cursor@2: 'B'(0x42) -> Replace{[2..3], "b"} — uppercase toggled to lowercase
///   Result: "aabb"
///
/// Algebraic replication (bug):
///   Primary@0 produces Replace{[0..1], "a"}.
///   Rebased to cursor@2: Replace{[2..3], "a"} — copies "a" instead of "b".
///   Result: "aaab" — wrong! The 'B' at position 2 was replaced with 'a' not 'b'.
#[test]
fn phase1_tilde_opposite_case_two_cursors() {
    let mut session = session_with_cursors("AaBb", &[0, 2]);
    feed(&mut session, "~");
    assert_eq!(
        session.text(),
        "aabb",
        "`~` at cursor@0 ('A') should produce 'a', and at cursor@2 \
         ('B') should produce 'b'. Algebraic replication would copy primary's 'a' \
         to cursor@2, producing 'aaab' instead of 'aabb'.",
    );
}

/// `~` on mixed-case where primary toggles up but secondary
/// should toggle down.
///
/// Document: "xY"
/// Cursors at 0 ('x') and 1 ('Y').
///
/// Per-cursor (correct):
///   cursor@0: 'x' -> Replace{[0..1], "X"} — lowercase to uppercase
///   cursor@1: 'Y' -> Replace{[1..2], "y"} — uppercase to lowercase
///   Result: "Xy"
///
/// Algebraic replication (bug):
///   Primary@0 produces Replace{[0..1], "X"}.
///   Rebased to cursor@1: Replace{[1..2], "X"} — replaces 'Y' with 'X' not 'y'.
///   Result: "XX" — wrong!
#[test]
fn phase1_tilde_up_and_down_adjacent() {
    let mut session = session_with_cursors("xY", &[0, 1]);
    feed(&mut session, "~");
    assert_eq!(
        session.text(),
        "Xy",
        "`~` on 'x' (-> 'X') and 'Y' (-> 'y') must toggle each \
         independently. Algebraic replication would produce 'XX' (copying 'X' to both).",
    );
}

/// `~` with count=2 on two cursors at different positions.
///
/// Document: "aaBB"
/// Cursors at 0 ('a') and 2 ('B').
/// Command: `2~` (toggle 2 characters at each cursor).
///
/// Per-cursor (correct):
///   cursor@0: toggles 'a' and 'a' -> Replace{[0..2], "AA"}
///   cursor@2: toggles 'B' and 'B' -> Replace{[2..4], "bb"}
///   Result: "AAbb"
///
/// Algebraic replication (bug):
///   Primary@0 produces Replace{[0..2], "AA"}.
///   Rebased to cursor@2: Replace{[2..4], "AA"} — copies "AA" instead of "bb".
///   Result: "AAAA" — wrong!
#[test]
fn phase1_tilde_count2_different_content() {
    let mut session = session_with_cursors("aaBB", &[0, 2]);
    feed(&mut session, "2~");
    assert_eq!(
        session.text(),
        "AAbb",
        "`2~` toggles 2 chars at each cursor. cursor@0 toggles \
         'aa' -> 'AA', cursor@2 toggles 'BB' -> 'bb'. Algebraic replication would \
         copy 'AA' to cursor@2, producing 'AAAA'.",
    );
}

// =============================================================================
// 2. `gUiw` / `guiw` / `g~iw` per-cursor fidelity
// =============================================================================

/// `gUiw` on two cursors at same-length words.
///
/// Document: "hello world"
/// Cursors at 0 (on "hello") and 6 (on "world").
///
/// Per-cursor (correct):
///   cursor@0: uppercases "hello" -> Replace{[0..5], "HELLO"}
///   cursor@6: uppercases "world" -> Replace{[6..11], "WORLD"}
///   Result: "HELLO WORLD"
///
/// Algebraic replication happens to produce the right result here because
/// both words are the same length. This validates that per-cursor also
/// works correctly for this common case.
#[test]
fn phase1_gUiw_same_length_words() {
    let mut session = session_with_cursors("hello world", &[0, 6]);
    feed(&mut session, "gUiw");
    assert_eq!(
        session.text(),
        "HELLO WORLD",
        "`gUiw` on 'hello' and 'world' — both same length. \
         Per-cursor re-execution should uppercase each independently.",
    );
}

/// `gUiw` on words of DIFFERENT lengths — exposes algebraic bug.
///
/// Document: "ab cdefg"
/// Cursors at 0 (on "ab", 2 chars) and 3 (on "cdefg", 5 chars).
///
/// Per-cursor (correct):
///   cursor@0: uppercases "ab" -> Replace{[0..2], "AB"}
///   cursor@3: uppercases "cdefg" -> Replace{[3..8], "CDEFG"}
///   Result: "AB CDEFG"
///
/// Algebraic replication (bug):
///   Primary@0 produces Replace{[0..2], "AB"} (range length 2).
///   Rebased to cursor@3 with same range length: Replace{[3..5], "AB"}.
///   This only uppercases "cd" (2 chars), leaving "efg" untouched.
///   Result: "AB ABefg" or similar corruption.
#[test]
fn phase1_gUiw_different_length_words() {
    let mut session = session_with_cursors("ab cdefg", &[0, 3]);
    feed(&mut session, "gUiw");
    assert_eq!(
        session.text(),
        "AB CDEFG",
        "`gUiw` with different-length words ('ab' vs 'cdefg'). \
         Per-cursor re-execution must determine each word's boundaries independently. \
         Algebraic replication uses primary's range length (2) for all cursors, \
         leaving part of 'cdefg' untouched.",
    );
}

/// `guiw` on two already-mixed-case words.
///
/// Document: "HeLLo WoRLd"
/// Cursors at 0 (on "HeLLo") and 6 (on "WoRLd").
///
/// Per-cursor (correct):
///   cursor@0: lowercases "HeLLo" -> "hello"
///   cursor@6: lowercases "WoRLd" -> "world"
///   Result: "hello world"
///
/// Algebraic replication (bug):
///   Primary produces Replace{[0..5], "hello"}.
///   Rebased to cursor@6: Replace{[6..11], "hello"} — wrong text for "WoRLd"!
///   Result: "hello hello" — secondary got primary's replacement text.
#[test]
fn phase1_guiw_mixed_case_words() {
    let mut session = session_with_cursors("HeLLo WoRLd", &[0, 6]);
    feed(&mut session, "guiw");
    assert_eq!(
        session.text(),
        "hello world",
        "`guiw` on 'HeLLo' and 'WoRLd'. Per-cursor must lowercase \
         each word's own content. Algebraic replication copies primary's 'hello' \
         to cursor@6, producing 'hello hello' instead of 'hello world'.",
    );
}

/// `g~iw` (swap case entire word) on two different words.
///
/// Document: "ABcd EFgh"
/// Cursors at 0 ("ABcd") and 5 ("EFgh").
///
/// Per-cursor (correct):
///   cursor@0: swap "ABcd" -> "abCD"
///   cursor@5: swap "EFgh" -> "efGH"
///   Result: "abCD efGH"
///
/// Algebraic replication (bug):
///   Primary@0 produces Replace{[0..4], "abCD"}.
///   Rebased to cursor@5: Replace{[5..9], "abCD"} — wrong text! Should be "efGH".
///   Result: "abCD abCD"
#[test]
fn phase1_swap_case_word_different_content() {
    let mut session = session_with_cursors("ABcd EFgh", &[0, 5]);
    feed(&mut session, "g~iw");
    assert_eq!(
        session.text(),
        "abCD efGH",
        "`g~iw` swaps case of each word independently. \
         cursor@0: 'ABcd' -> 'abCD', cursor@5: 'EFgh' -> 'efGH'. \
         Algebraic replication copies 'abCD' to both positions.",
    );
}

// =============================================================================
// 3. `Ctrl-A` / `Ctrl-X` per-cursor fidelity
// =============================================================================

/// `Ctrl-A` on two cursors with different decimal numbers.
///
/// Document: "10 20"
/// Cursors at 0 (on "10") and 3 (on "20").
///
/// Per-cursor (correct):
///   cursor@0: increment 10 -> 11, Replace{[0..2], "11"}
///   cursor@3: increment 20 -> 21, Replace{[3..5], "21"}
///   Result: "11 21"
///
/// Algebraic replication (bug):
///   Primary@0 produces Replace{[0..2], "11"}.
///   Rebased to cursor@3: Replace{[3..5], "11"} — replaces "20" with "11".
///   Result: "11 11" — secondary got primary's replacement text.
#[test]
fn phase1_ctrl_a_different_decimal_numbers() {
    let mut session = session_with_cursors("10 20", &[0, 3]);
    feed(&mut session, "<C-a>");
    assert_eq!(
        session.text(),
        "11 21",
        "`Ctrl-A` with cursors on '10' and '20'. Per-cursor \
         re-execution must increment each number independently (10->11, 20->21). \
         Algebraic replication copies '11' to cursor@3, producing '11 11'.",
    );
}

/// `Ctrl-A` on two single-digit numbers (no width change).
///
/// Document: "3 7"
/// Cursors at 0 (on "3") and 2 (on "7").
///
/// Per-cursor (correct): 3->4, 7->8 -> "4 8"
/// Algebraic replication (bug): copies "4" to cursor@2 -> "4 4"
#[test]
fn phase1_ctrl_a_single_digit_numbers() {
    let mut session = session_with_cursors("3 7", &[0, 2]);
    feed(&mut session, "<C-a>");
    assert_eq!(
        session.text(),
        "4 8",
        "`Ctrl-A` on '3' and '7'. Each should be independently \
         incremented to '4' and '8'. Algebraic replication: '4 4'.",
    );
}

/// `Ctrl-X` on two different-value numbers.
///
/// Document: "50 30"
/// Cursors at 0 (on "50") and 3 (on "30").
///
/// Per-cursor (correct): 50->49, 30->29 -> "49 29"
/// Algebraic replication (bug): copies "49" -> "49 49"
#[test]
fn phase1_ctrl_x_different_values() {
    let mut session = session_with_cursors("50 30", &[0, 3]);
    feed(&mut session, "<C-x>");
    assert_eq!(
        session.text(),
        "49 29",
        "`Ctrl-X` on '50' and '30'. Each should be independently \
         decremented to '49' and '29'. Algebraic replication: '49 49'.",
    );
}

// =============================================================================
// 4. EditOp-level inspection — proves per-cursor replacement text
// =============================================================================

/// Inspect actual EditOps from `~` to verify per-cursor text.
///
/// This test goes beyond checking final text — it examines the EditOps
/// returned by process_key_host to confirm each cursor produces its own
/// replacement text, not a copy of the primary's.
///
/// Document: "xY"
/// Cursors at 0 ('x') and 1 ('Y').
///
/// Expected EditOps (per-cursor):
///   EditOp { offset: 1, delete: 1, insert: "y" }  — cursor@1: 'Y' -> 'y'
///   EditOp { offset: 0, delete: 1, insert: "X" }  — cursor@0: 'x' -> 'X'
///   (descending order: higher offset first)
///
/// Bug EditOps (algebraic):
///   EditOp { offset: 1, delete: 1, insert: "X" }  — cursor@1 gets 'X' (wrong!)
///   EditOp { offset: 0, delete: 1, insert: "X" }  — cursor@0 gets 'X' (correct)
#[test]
fn phase1_tilde_editops_verify_per_cursor_text() {
    let mut session = session_with_cursors("xY", &[0, 1]);
    let keys = parse_keys_from_string("~");
    let mut all_edits = Vec::new();
    for key in keys {
        let resp = session.process_key_host(key);
        all_edits.extend(resp.edits);
    }

    // We should have edits for both cursors.
    assert!(
        all_edits.len() >= 2,
        "Expected at least 2 EditOps (one per cursor), got {}. Edits: {:?}",
        all_edits.len(),
        all_edits,
    );

    // Find the edit at offset 1 (cursor@1 on 'Y') — should insert "y" not "X".
    let edit_at_1 = all_edits.iter().find(|e| e.offset == 1);
    assert!(
        edit_at_1.is_some(),
        "Expected an EditOp at offset 1 for cursor@1. Got: {:?}",
        all_edits,
    );
    let edit_at_1 = edit_at_1.unwrap();
    assert_eq!(
        edit_at_1.insert.as_str(),
        "y",
        "EditOp at offset 1 should insert 'y' (toggle of 'Y'), \
         not 'X' (primary's replacement). Got EditOp: {:?}. \
         All edits: {:?}",
        edit_at_1,
        all_edits,
    );

    // Find the edit at offset 0 (cursor@0 on 'x') — should insert "X".
    let edit_at_0 = all_edits.iter().find(|e| e.offset == 0);
    assert!(
        edit_at_0.is_some(),
        "Expected an EditOp at offset 0 for cursor@0. Got: {:?}",
        all_edits,
    );
    let edit_at_0 = edit_at_0.unwrap();
    assert_eq!(
        edit_at_0.insert.as_str(),
        "X",
        "EditOp at offset 0 should insert 'X' (toggle of 'x'). \
         Got: {:?}",
        edit_at_0,
    );
}

/// Inspect EditOps from `gUiw` to verify per-cursor text.
///
/// Document: "hi there"
/// Cursors at 0 (on "hi") and 3 (on "there").
///
/// Expected EditOps:
///   EditOp { offset: 3, delete: 5, insert: "THERE" }  — cursor@3
///   EditOp { offset: 0, delete: 2, insert: "HI" }     — cursor@0
///
/// Bug EditOps (algebraic):
///   EditOp { offset: 3, delete: 2, insert: "HI" }     — wrong range and text!
#[test]
fn phase1_gUiw_editops_different_length_words() {
    let mut session = session_with_cursors("hi there", &[0, 3]);
    let keys = parse_keys_from_string("gUiw");
    let mut all_edits = Vec::new();
    for key in keys {
        let resp = session.process_key_host(key);
        all_edits.extend(resp.edits);
    }

    // Verify final text first.
    assert_eq!(session.text(), "HI THERE");

    // Check EditOp for cursor@3 (on "there") — should be "THERE" (5 chars).
    let edit_at_3 = all_edits.iter().find(|e| e.offset == 3);
    assert!(
        edit_at_3.is_some(),
        "Expected EditOp at offset 3 for cursor@3. Got: {:?}",
        all_edits,
    );
    let edit_at_3 = edit_at_3.unwrap();
    assert_eq!(
        edit_at_3.insert.as_str(),
        "THERE",
        "EditOp at offset 3 should insert 'THERE' (uppercase of 'there'), \
         not 'HI' (primary's replacement). Got: {:?}",
        edit_at_3,
    );
    assert_eq!(
        edit_at_3.delete, 5,
        "EditOp at offset 3 should delete 5 bytes ('there'), \
         not 2 (primary's range length). Got: {:?}",
        edit_at_3,
    );

    // Check EditOp for cursor@0 (on "hi") — should be "HI" (2 chars).
    let edit_at_0 = all_edits.iter().find(|e| e.offset == 0);
    assert!(
        edit_at_0.is_some(),
        "Expected EditOp at offset 0. Got: {:?}",
        all_edits,
    );
    let edit_at_0 = edit_at_0.unwrap();
    assert_eq!(
        edit_at_0.insert.as_str(),
        "HI",
        "EditOp at offset 0 should insert 'HI'. Got: {:?}",
        edit_at_0,
    );
}

/// Inspect EditOps from `Ctrl-A` to verify per-cursor replacement.
///
/// Document: "3 7"
/// Cursors at 0 (on "3") and 2 (on "7").
///
/// Expected EditOps:
///   EditOp { offset: 2, delete: 1, insert: "8" }  — cursor@2: 7->8
///   EditOp { offset: 0, delete: 1, insert: "4" }  — cursor@0: 3->4
///
/// Bug EditOps (algebraic):
///   EditOp { offset: 2, delete: 1, insert: "4" }  — copies "4" to cursor@2
#[test]
fn phase1_ctrl_a_editops_verify_per_cursor_increment() {
    let mut session = session_with_cursors("3 7", &[0, 2]);
    let keys = parse_keys_from_string("<C-a>");
    let mut all_edits = Vec::new();
    for key in keys {
        let resp = session.process_key_host(key);
        all_edits.extend(resp.edits);
    }

    // Verify final text.
    assert_eq!(
        session.text(),
        "4 8",
        "Ctrl-A on '3' and '7' should produce '4 8'.",
    );

    // Check EditOp for cursor@2 — should insert "8" not "4".
    let edit_at_2 = all_edits.iter().find(|e| e.offset == 2);
    assert!(
        edit_at_2.is_some(),
        "Expected EditOp at offset 2 for cursor@2. Got: {:?}",
        all_edits,
    );
    let edit_at_2 = edit_at_2.unwrap();
    assert_eq!(
        edit_at_2.insert.as_str(),
        "8",
        "EditOp at offset 2 should insert '8' (7+1), \
         not '4' (primary's 3+1). Got: {:?}",
        edit_at_2,
    );
}

// =============================================================================
// 5. Cursor position verification after per-cursor re-execution
// =============================================================================

/// After `~`, each cursor should advance by 1.
///
/// Document: "xY"
/// Cursors at 0 and 1.
/// After `~`: text is "Xy", cursors should advance.
#[test]
fn phase1_tilde_cursor_positions_advance() {
    let mut session = session_with_cursors("xY", &[0, 1]);
    feed(&mut session, "~");
    assert_eq!(session.text(), "Xy");
    // After `~`, cursor advances by 1. The primary cursor offset should be 1
    // (advanced from 0).
    let offset = session.cursor_offset();
    assert!(
        offset <= 1,
        "Primary cursor should advance from 0 to 1 after `~`. Got: {}",
        offset,
    );
}

/// `gUiw` with different-length words — cursor count preserved.
///
/// Document: "ab cdefg"
/// Cursors at 0 ("ab") and 3 ("cdefg").
/// After `gUiw`: text is "AB CDEFG".
#[test]
fn phase1_gUiw_cursor_positions_different_words() {
    let mut session = session_with_cursors("ab cdefg", &[0, 3]);
    feed(&mut session, "gUiw");
    assert_eq!(session.text(), "AB CDEFG");
    // Cursor count should still be 2.
    assert_eq!(
        session.cursor_count(),
        2,
        "Should still have 2 cursors after gUiw.",
    );
}

// =============================================================================
// 6. Multi-cursor with 3+ cursors — validates correct ordering
// =============================================================================

/// `~` on 3 cursors with alternating case.
///
/// Document: "AbCd"
/// Cursors at 0 ('A'), 1 ('b'), 2 ('C').
///
/// Per-cursor (correct):
///   cursor@0: 'A' -> 'a'
///   cursor@1: 'b' -> 'B'
///   cursor@2: 'C' -> 'c'
///   Result: "aBcd"
///
/// Algebraic replication (bug):
///   Primary@0 produces Replace{[0..1], "a"}.
///   Rebased to cursor@1: Replace{[1..2], "a"} (wrong — should be "B")
///   Rebased to cursor@2: Replace{[2..3], "a"} (wrong — should be "c")
///   Result: "aaad"
#[test]
fn phase1_tilde_three_cursors_alternating() {
    let mut session = session_with_cursors("AbCd", &[0, 1, 2]);
    feed(&mut session, "~");
    assert_eq!(
        session.text(),
        "aBcd",
        "`~` on 3 cursors ('A', 'b', 'C') should toggle each \
         independently: 'A'->'a', 'b'->'B', 'C'->'c'. Result 'aBcd'. \
         Algebraic replication copies 'a' to all: 'aaad'.",
    );
}

/// `gUiw` on 3 words of different lengths.
///
/// Document: "a bb ccc"
/// Cursors at 0 ("a", 1 char), 2 ("bb", 2 chars), 5 ("ccc", 3 chars).
///
/// Per-cursor (correct):
///   cursor@0: "a" -> "A"
///   cursor@2: "bb" -> "BB"
///   cursor@5: "ccc" -> "CCC"
///   Result: "A BB CCC"
///
/// Algebraic replication (bug):
///   Primary produces Replace{[0..1], "A"} (range length 1).
///   cursor@2: Replace{[2..3], "A"} — only uppercases 1 char of "bb"
///   cursor@5: Replace{[5..6], "A"} — only uppercases 1 char of "ccc"
///   Result: "A Ab Acc" — wrong!
#[test]
fn phase1_gUiw_three_different_length_words() {
    let mut session = session_with_cursors("a bb ccc", &[0, 2, 5]);
    feed(&mut session, "gUiw");
    assert_eq!(
        session.text(),
        "A BB CCC",
        "`gUiw` on 3 words of different lengths ('a', 'bb', 'ccc'). \
         Each word must be uppercased in full. Algebraic replication uses primary's \
         range length (1) for all, producing 'A Ab Acc'.",
    );
}

// =============================================================================
// 7. UTF-8 multi-byte character handling
// =============================================================================

/// `~` on multi-byte UTF-8 characters.
///
/// Document: "a\u{00C4}"  (a followed by Latin capital A with diaeresis)
/// 'a' is 1 byte at offset 0, '\u{00C4}' is 2 bytes at offset 1.
/// Cursors at 0 ('a') and 1 ('\u{00C4}').
///
/// Per-cursor (correct):
///   cursor@0: 'a' -> 'A' (Replace{[0..1], "A"})
///   cursor@1: '\u{00C4}' -> '\u{00E4}' (Replace{[1..3], "\u{00E4}"}) — 2-byte char
///   Result: "A\u{00E4}"
///
/// Algebraic replication (bug):
///   Primary produces Replace{[0..1], "A"} (1-byte range).
///   Rebased to cursor@1: Replace{[1..2], "A"} — wrong range! '\u{00C4}' is 2 bytes.
///   This truncates the multi-byte character, producing corrupt UTF-8.
#[test]
fn phase1_tilde_utf8_multibyte() {
    let mut session = session_with_cursors("a\u{00C4}", &[0, 1]);
    feed(&mut session, "~");
    assert_eq!(
        session.text(),
        "A\u{00E4}",
        "`~` on ASCII 'a' and multi-byte '\u{00C4}'. Per-cursor handles \
         byte widths correctly. Algebraic replication uses primary's 1-byte range \
         on a 2-byte character, causing corruption.",
    );
}
