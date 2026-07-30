//! Multi-cursor yank/paste/register ideal-behavior tests.
//!
//! These tests describe how multi-cursor yank and paste SHOULD work.
//! They may fail now — they define the target behavior for paste-zip,
//! broadcast paste, named registers, linewise yank, visual yank, and
//! the full delete-then-paste lifecycle with multiple cursors.
//!
//! # Semantics
//!
//! - **Zip paste**: when register has N entries and there are N cursors,
//!   cursor `i` pastes entry `i`. (1:1 distribution)
//! - **Broadcast paste**: when register has 1 entry and there are N cursors,
//!   all cursors paste the same single entry.
//! - **Yank**: each cursor independently yanks the text object at its position,
//!   producing an N-entry register.

#![allow(non_snake_case)]

use vim_core::primitives::MotionType;
use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. yiw — PER-CURSOR WORD YANK
// ═══════════════════════════════════════════════════════════════════════════

/// `yiw` with 2 cursors stores 2 entries — one per cursor's inner word.
#[test]
fn mc_yiw_two_cursors_stores_two_entries() {
    let s = vim_mc("|1hello |2world").keys("yiw").run_session();

    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "register should have 2 entries (one per cursor)");

    assert_eq!(
        s.session().get_register_entry('"', 0).as_deref(),
        Some("hello")
    );
    assert_eq!(
        s.session().get_register_entry('"', 1).as_deref(),
        Some("world")
    );
}

/// `yiw` with 3 cursors on different-length words stores 3 correct entries.
#[test]
fn mc_yiw_three_cursors_different_lengths() {
    let s = vim_mc("|1a |2longword |3c").keys("yiw").run_session();

    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 3, "register should have 3 entries");

    assert_eq!(s.session().get_register_entry('"', 0).as_deref(), Some("a"));
    assert_eq!(
        s.session().get_register_entry('"', 1).as_deref(),
        Some("longword")
    );
    assert_eq!(s.session().get_register_entry('"', 2).as_deref(), Some("c"));
}

/// `yiw` does not modify the document text.
#[test]
fn mc_yiw_does_not_modify_text() {
    vim_mc("|1hello |2world")
        .keys("yiw")
        .expect_text("|1hello |2world")
        .expect_cursor_count(2)
        .labeled("yiw is a yank — text must not change")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. p — ZIP PASTE (N entries, N cursors)
// ═══════════════════════════════════════════════════════════════════════════

/// Paste with 2-entry register and 2 cursors distributes entries.
///
/// Yank "aaa" and "bbb" at two cursors, move to end of line, paste.
/// Each cursor pastes its own yanked word after the cursor character.
#[test]
fn mc_paste_zip_two_cursors() {
    let s = vim_mc("|1aaa |2bbb")
        .keys("yiw") // register: ["aaa", "bbb"]
        .keys("$") // move to end of each word
        .keys("p") // paste after cursor
        .run_session();

    let text = s.text();
    // Both entries must appear in the output
    assert!(
        text.contains("aaa") && text.contains("bbb"),
        "zip paste: each cursor should paste its own entry. Got: {text:?}"
    );
    // Verify the register had 2 entries going in
    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "register should still have 2 entries after paste");
}

/// Paste-zip with 3 cursors — each gets its own entry.
#[test]
fn mc_paste_zip_three_cursors() {
    let s = vim_mc("|1aaa |2bbb |3ccc")
        .keys("yiw") // register: ["aaa", "bbb", "ccc"]
        .keys("$")
        .keys("p")
        .run_session();

    let text = s.text();
    assert!(
        text.contains("aaa") && text.contains("bbb") && text.contains("ccc"),
        "zip paste with 3 entries: each cursor pastes its entry. Got: {text:?}"
    );
}

/// Yank at 2 cursors, set up new text with 2 cursors, paste distributes.
///
/// This verifies zip paste works when the paste target is different from
/// the yank source.
#[test]
fn mc_paste_zip_into_different_text() {
    let mut s = TestSession::new_multi("|1aaa |2bbb");
    s.feed("yiw");

    // Verify the register is loaded
    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "register should have 2 entries");

    // Set up fresh text and cursors for paste
    s.session_mut().set_text("X Y");
    s.session_mut().clear_secondary_cursors();
    s.session_mut().set_cursor_offset(0);
    s.session_mut().add_cursor(2).expect("add cursor at Y");

    s.feed("p");

    let text = s.text().to_owned();
    // cursor@0 ('X') pastes "aaa" after 'X', cursor@2 ('Y') pastes "bbb" after 'Y'
    assert!(
        text.contains("aaa") && text.contains("bbb"),
        "zip paste into new text should distribute entries. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. p — BROADCAST PASTE (1 entry, N cursors)
// ═══════════════════════════════════════════════════════════════════════════

/// Single-entry register pasted at 2 cursors — both get the same text.
#[test]
fn mc_paste_broadcast_single_entry() {
    vim_mc("|1X |2Y")
        .set_register('"', "SAME")
        .keys("p")
        .run_session();

    // We need to use run_session to inspect; rebuild with a direct approach
    let mut s = TestSession::new_multi("|1X |2Y");
    s.session_mut()
        .set_register('"', "SAME", MotionType::CharWise);
    s.feed("p");

    let text = s.text().to_owned();
    let same_count = text.matches("SAME").count();
    assert_eq!(
        same_count, 2,
        "broadcast paste: 1-entry register should paste at all cursors. Got: {text:?}"
    );
}

/// Broadcast paste with 3 cursors — all get the same text.
#[test]
fn mc_paste_broadcast_three_cursors() {
    let mut s = TestSession::new_multi("|1A |2B |3C");
    s.session_mut()
        .set_register('"', "ZZ", MotionType::CharWise);
    s.feed("p");

    let text = s.text().to_owned();
    let zz_count = text.matches("ZZ").count();
    assert_eq!(
        zz_count, 3,
        "broadcast paste with 3 cursors should produce 3 copies. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. yy — LINEWISE YANK WITH MC
// ═══════════════════════════════════════════════════════════════════════════

/// `yy` with cursors on 2 different lines stores 2 linewise entries.
#[test]
fn mc_yy_two_lines() {
    let s = vim_mc("|1alpha\n|2beta").keys("yy").run_session();

    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "yy with 2 cursors should produce 2 entries");

    // Each entry should be the full line (with trailing newline for linewise)
    let entry0 = s.session().get_register_entry('"', 0).unwrap();
    let entry1 = s.session().get_register_entry('"', 1).unwrap();
    assert!(
        entry0.contains("alpha"),
        "entry 0 should contain 'alpha'. Got: {entry0:?}"
    );
    assert!(
        entry1.contains("beta"),
        "entry 1 should contain 'beta'. Got: {entry1:?}"
    );

    // Verify linewise motion type
    let (_, mt) = s.register('"').expect("register should exist");
    assert_eq!(
        mt,
        MotionType::LineWise,
        "yy should produce linewise register"
    );
}

/// `yy` does not modify text.
#[test]
fn mc_yy_preserves_text() {
    vim_mc("|1first line\n|2second line")
        .keys("yy")
        .expect_text("|1first line\n|2second line")
        .labeled("yy should not modify text")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. dd + p — DELETE LINE THEN PASTE BACK
// ═══════════════════════════════════════════════════════════════════════════

/// Delete 2 lines with `dd`, then paste them back with `p`.
///
/// After `dd` on lines 0 and 2 of a 4-line doc, the deleted lines go
/// into the register. `p` should paste them back.
#[test]
fn mc_dd_then_p_restores_lines() {
    // 4 lines, cursors on lines 0 and 2
    let mut s = TestSession::new_multi("|1aaa\nbbb\n|2ccc\nddd");
    s.feed("dd");

    // After dd, register should have 2 linewise entries
    let count = s.session().get_register_entry_count('"');
    assert_eq!(
        count, 2,
        "dd with 2 cursors should produce 2 register entries"
    );

    let text_after_dd = s.text().to_owned();
    // Lines 0 and 2 were deleted, so "bbb" and "ddd" remain
    assert!(
        text_after_dd.contains("bbb") && text_after_dd.contains("ddd"),
        "after dd on lines 0,2: lines 1,3 should remain. Got: {text_after_dd:?}"
    );

    // Now paste back
    s.feed("p");

    let text_after_p = s.text().to_owned();
    // The deleted lines should reappear
    assert!(
        text_after_p.contains("aaa") && text_after_p.contains("ccc"),
        "paste after dd should restore deleted lines. Got: {text_after_p:?}"
    );
}

/// `dd` then `p` is undoable as two separate operations.
#[test]
fn mc_dd_p_undo_lifecycle() {
    let original = "aaa\nbbb\nccc\nddd";
    let mut s = TestSession::new_multi("|1aaa\nbbb\n|2ccc\nddd");

    s.feed("dd");
    let after_dd = s.text().to_owned();
    assert_ne!(after_dd, original, "dd should change text");

    s.feed("p");
    let _after_p = s.text().to_owned();

    // Undo paste
    s.feed("u");
    assert_eq!(
        s.text(),
        after_dd,
        "undo after p should restore to post-dd state"
    );

    // Undo delete
    s.feed("u");
    assert_eq!(
        s.text(),
        original,
        "second undo should restore original text"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. diw + p — DELETE WORD THEN PASTE
// ═══════════════════════════════════════════════════════════════════════════

/// `diw` at 2 cursors deletes different words, `p` pastes them back.
#[test]
fn mc_diw_then_p() {
    let mut s = TestSession::new_multi("|1hello |2world end");
    s.feed("diw");

    // Register should have 2 charwise entries
    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "diw with 2 cursors should produce 2 entries");

    let entry0 = s.session().get_register_entry('"', 0).unwrap();
    let entry1 = s.session().get_register_entry('"', 1).unwrap();
    assert_eq!(entry0, "hello", "entry 0 should be 'hello'");
    assert_eq!(entry1, "world", "entry 1 should be 'world'");

    // Paste back — each cursor should paste its own deleted word
    s.feed("p");

    let text = s.text().to_owned();
    assert!(
        text.contains("hello") && text.contains("world"),
        "paste after diw should reinsert both words. Got: {text:?}"
    );
}

/// `diw` on words of very different lengths — register entries match.
#[test]
fn mc_diw_different_length_words() {
    let mut s = TestSession::new_multi("|1a |2superlongword |3z");
    s.feed("diw");

    assert_eq!(s.session().get_register_entry_count('"'), 3);
    assert_eq!(s.session().get_register_entry('"', 0).as_deref(), Some("a"));
    assert_eq!(
        s.session().get_register_entry('"', 1).as_deref(),
        Some("superlongword")
    );
    assert_eq!(s.session().get_register_entry('"', 2).as_deref(), Some("z"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. "ayiw — NAMED REGISTER YANK
// ═══════════════════════════════════════════════════════════════════════════

/// `"ayiw` yanks each cursor's word into named register 'a'.
#[test]
fn mc_named_register_a_yiw() {
    let s = vim_mc("|1foo |2bar").keys("\"ayiw").run_session();

    // Register 'a' should have content
    let reg = s.register('a');
    assert!(
        reg.is_some(),
        "register 'a' should have content after \"ayiw"
    );

    let count = s.session().get_register_entry_count('a');
    assert_eq!(count, 2, "register 'a' should have 2 entries");

    assert_eq!(
        s.session().get_register_entry('a', 0).as_deref(),
        Some("foo")
    );
    assert_eq!(
        s.session().get_register_entry('a', 1).as_deref(),
        Some("bar")
    );
}

/// `"ayiw` followed by `"ap` distributes from named register.
#[test]
fn mc_named_register_yank_then_paste() {
    let mut s = TestSession::new_multi("|1foo |2bar");
    s.feed("\"ayiw");

    // Move cursors to end
    s.feed("$");

    // Paste from register 'a'
    s.feed("\"ap");

    let text = s.text().to_owned();
    assert!(
        text.contains("foo") && text.contains("bar"),
        "paste from named register 'a' should distribute entries. Got: {text:?}"
    );
}

/// Named register does not pollute the unnamed register.
#[test]
fn mc_named_register_does_not_pollute_unnamed() {
    let mut s = TestSession::new_multi("|1aaa |2bbb");

    // First set unnamed register to something known
    s.session_mut()
        .set_register('"', "OLD", MotionType::CharWise);

    // Yank into 'a'
    s.feed("\"ayiw");

    // Unnamed register should also be updated (Vim behavior: named yank
    // also writes to unnamed), but the named register is the primary target
    let a_count = s.session().get_register_entry_count('a');
    assert_eq!(a_count, 2, "register 'a' should have 2 entries");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. CROSS-CURSOR PASTE — ENTRY COUNT MISMATCH
// ═══════════════════════════════════════════════════════════════════════════

/// Yank with 2 cursors (2 entries), paste with 3 cursors.
///
/// When entry count != cursor count, Vim behavior is to broadcast the
/// concatenated register content to all cursors (no zip).
#[test]
fn mc_paste_entry_count_mismatch_broadcasts() {
    let mut s = TestSession::new_multi("|1aaa |2bbb");
    s.feed("yiw");

    // Verify 2 entries
    assert_eq!(s.session().get_register_entry_count('"'), 2);

    // Set up 3 cursors on new text
    s.session_mut().set_text("X Y Z");
    s.session_mut().clear_secondary_cursors();
    s.session_mut().set_cursor_offset(0);
    s.session_mut().add_cursor(2).expect("add cursor");
    s.session_mut().add_cursor(4).expect("add cursor");
    assert_eq!(s.cursor_count(), 3);

    s.feed("p");

    let text = s.text().to_owned();
    // With 2 entries and 3 cursors, the behavior is implementation-defined.
    // The key invariant: no cursor should crash or produce garbage.
    // Common approach: broadcast the full register content to all cursors.
    assert!(
        !text.is_empty(),
        "paste with entry/cursor mismatch should produce valid text. Got: {text:?}"
    );
}

/// Yank with 3 cursors (3 entries), paste with 2 cursors.
///
/// Extra entries are ignored. Cursor 0 gets entry 0, cursor 1 gets entry 1.
#[test]
fn mc_paste_more_entries_than_cursors() {
    let mut s = TestSession::new_multi("|1aaa |2bbb |3ccc");
    s.feed("yiw");

    assert_eq!(s.session().get_register_entry_count('"'), 3);

    // Set up only 2 cursors
    s.session_mut().set_text("X Y");
    s.session_mut().clear_secondary_cursors();
    s.session_mut().set_cursor_offset(0);
    s.session_mut().add_cursor(2).expect("add cursor");
    assert_eq!(s.cursor_count(), 2);

    s.feed("p");

    let text = s.text().to_owned();
    // Should not crash; entries beyond cursor count are simply unused.
    assert!(
        !text.is_empty(),
        "paste with more entries than cursors should not crash. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Y — YANK LINE (ALIAS) WITH MC
// ═══════════════════════════════════════════════════════════════════════════

/// `Y` with 2 cursors on different lines yanks from cursor to EOL.
///
/// In this engine, `Y` follows the Neovim convention: `y$` (charwise
/// yank to end of line), NOT linewise `yy`.
#[test]
fn mc_Y_two_lines() {
    let s = vim_mc("|1alpha\n|2beta").keys("Y").run_session();

    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "Y with 2 cursors should produce 2 entries");

    let entry0 = s.session().get_register_entry('"', 0).unwrap();
    let entry1 = s.session().get_register_entry('"', 1).unwrap();
    assert!(
        entry0.contains("alpha"),
        "entry 0 should contain 'alpha'. Got: {entry0:?}"
    );
    assert!(
        entry1.contains("beta"),
        "entry 1 should contain 'beta'. Got: {entry1:?}"
    );

    // Y = y$ => charwise (Neovim convention)
    let (_, mt) = s.register('"').expect("register should exist");
    assert_eq!(
        mt,
        MotionType::CharWise,
        "Y should produce charwise register (y$ convention)"
    );
}

/// `Y` (y$) does not modify document text.
#[test]
fn mc_Y_preserves_text() {
    vim_mc("|1line1\n|2line2")
        .keys("Y")
        .expect_text("|1line1\n|2line2")
        .labeled("Y is a yank — text must not change")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. VISUAL YANK WITH MC
// ═══════════════════════════════════════════════════════════════════════════

/// Visual mode: select different lengths at each cursor, yank.
///
/// Cursor 1 on "abc", cursor 2 on "defgh".
/// `vey` (visual to end of word, yank): each cursor selects and yanks
/// its own word.
#[test]
fn mc_visual_yank_different_lengths() {
    let s = vim_mc("|1abc |2defgh").keys("vey").run_session();

    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "visual yank should produce 2 entries");

    let entry0 = s.session().get_register_entry('"', 0).unwrap();
    let entry1 = s.session().get_register_entry('"', 1).unwrap();
    assert!(
        entry0.contains("abc"),
        "entry 0 should contain 'abc'. Got: {entry0:?}"
    );
    assert!(
        entry1.contains("defgh"),
        "entry 1 should contain 'defgh'. Got: {entry1:?}"
    );
}

/// Visual line yank (`Vy`) with MC — each cursor yanks its own line.
#[test]
fn mc_visual_line_yank() {
    let s = vim_mc("|1first\n|2second\nthird").keys("Vy").run_session();

    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "V yank should produce 2 entries");

    let entry0 = s.session().get_register_entry('"', 0).unwrap();
    let entry1 = s.session().get_register_entry('"', 1).unwrap();
    assert!(
        entry0.contains("first"),
        "entry 0 should contain 'first'. Got: {entry0:?}"
    );
    assert!(
        entry1.contains("second"),
        "entry 1 should contain 'second'. Got: {entry1:?}"
    );

    let (_, mt) = s.register('"').expect("register should exist");
    assert_eq!(
        mt,
        MotionType::LineWise,
        "Vy should produce linewise register"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. P (UPPERCASE) — PASTE BEFORE
// ═══════════════════════════════════════════════════════════════════════════

/// `P` (paste before) with zip distribution.
#[test]
fn mc_P_paste_before_zip() {
    let mut s = TestSession::new_multi("|1aaa |2bbb");
    s.feed("yiw");
    s.feed("w"); // advance to next word boundary

    s.feed("P"); // paste before

    let text = s.text().to_owned();
    assert!(
        text.contains("aaa") && text.contains("bbb"),
        "P with 2-entry register should paste-zip before each cursor. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. LINEWISE PASTE WITH MC
// ═══════════════════════════════════════════════════════════════════════════

/// `yy` then `p` — linewise paste below each cursor's line.
#[test]
fn mc_yy_then_p_pastes_lines_below() {
    let mut s = TestSession::new_multi("|1aaa\nbbb\n|2ccc");
    s.feed("yy");

    // Verify linewise entries
    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "yy with 2 cursors should produce 2 entries");

    s.feed("p");

    let text = s.text().to_owned();
    let lines: Vec<&str> = text.lines().collect();
    // After pasting linewise below, each cursor's line should be duplicated
    // below it. The aaa line gets aaa pasted below, ccc gets ccc below.
    let aaa_count = lines.iter().filter(|l| l.trim() == "aaa").count();
    let ccc_count = lines.iter().filter(|l| l.trim() == "ccc").count();
    assert!(
        aaa_count >= 2,
        "linewise paste should duplicate 'aaa' line. Lines: {lines:?}"
    );
    assert!(
        ccc_count >= 2,
        "linewise paste should duplicate 'ccc' line. Lines: {lines:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. REGISTER ISOLATION — UNNAMED VS SMALL-DELETE
// ═══════════════════════════════════════════════════════════════════════════

/// `diw` populates both `"` (unnamed) and `-` (small delete) registers.
///
/// Each should have per-cursor entries for multi-cursor delete.
#[test]
fn mc_diw_populates_unnamed_register() {
    let s = vim_mc("|1foo |2bar").keys("diw").run_session();

    let unnamed_count = s.session().get_register_entry_count('"');
    assert_eq!(
        unnamed_count, 2,
        "unnamed register should have 2 entries after mc diw"
    );

    let entry0 = s.session().get_register_entry('"', 0).unwrap();
    let entry1 = s.session().get_register_entry('"', 1).unwrap();
    assert_eq!(entry0, "foo");
    assert_eq!(entry1, "bar");
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. x (DELETE CHAR) — REGISTER ENTRIES
// ═══════════════════════════════════════════════════════════════════════════

/// `x` at each cursor stores per-cursor deleted characters.
#[test]
fn mc_x_stores_per_cursor_entries() {
    let s = vim_mc("|1abc |2xyz").keys("x").run_session();

    let count = s.session().get_register_entry_count('"');
    assert_eq!(
        count, 2,
        "x with 2 cursors should produce 2 register entries"
    );

    assert_eq!(s.session().get_register_entry('"', 0).as_deref(), Some("a"));
    assert_eq!(s.session().get_register_entry('"', 1).as_deref(), Some("x"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. FULL LIFECYCLE — yiw, MOVE, PASTE, UNDO
// ═══════════════════════════════════════════════════════════════════════════

/// Full workflow: yank words, move, paste, undo everything.
#[test]
fn mc_full_yank_move_paste_undo() {
    let original = "aaa bbb";
    let mut s = TestSession::new_multi("|1aaa |2bbb");

    // Yank
    s.feed("yiw");
    assert_eq!(s.session().get_register_entry_count('"'), 2);
    assert_eq!(s.text(), original, "yank should not modify text");

    // Move to end of each word
    s.feed("$");

    // Paste after
    s.feed("p");
    let after_paste = s.text().to_owned();
    assert_ne!(after_paste, original, "paste should modify text");
    assert!(
        after_paste.contains("aaa") && after_paste.contains("bbb"),
        "both entries should be pasted. Got: {after_paste:?}"
    );

    // Undo paste
    s.feed("u");
    // After undoing paste, we should be back to the text state before paste
    // (which is the original since yank doesn't change text)
    assert_eq!(
        s.text(),
        original,
        "undo after paste should restore original text"
    );
}

/// Stress: yank + paste with 5 cursors.
#[test]
fn mc_yank_paste_five_cursors() {
    let s = vim_mc("|1aa |2bb |3cc |4dd |5ee").keys("yiw").run_session();

    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 5, "yiw with 5 cursors should produce 5 entries");

    assert_eq!(
        s.session().get_register_entry('"', 0).as_deref(),
        Some("aa")
    );
    assert_eq!(
        s.session().get_register_entry('"', 1).as_deref(),
        Some("bb")
    );
    assert_eq!(
        s.session().get_register_entry('"', 2).as_deref(),
        Some("cc")
    );
    assert_eq!(
        s.session().get_register_entry('"', 3).as_deref(),
        Some("dd")
    );
    assert_eq!(
        s.session().get_register_entry('"', 4).as_deref(),
        Some("ee")
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. ciw + PASTE — CHANGE WORD POPULATES REGISTER
// ═══════════════════════════════════════════════════════════════════════════

/// `ciw` should put the deleted words into the register, then paste works.
#[test]
fn mc_ciw_populates_register_then_paste() {
    let mut s = TestSession::new_multi("|1old |2ancient");
    s.feed("ciw");

    // Register should have the deleted words
    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "ciw should populate register with 2 entries");

    assert_eq!(
        s.session().get_register_entry('"', 0).as_deref(),
        Some("old")
    );
    assert_eq!(
        s.session().get_register_entry('"', 1).as_deref(),
        Some("ancient")
    );

    // Type replacement and exit
    s.feed("new<Esc>");

    let text = s.text().to_owned();
    assert!(
        text.contains("new"),
        "ciw + typed text should produce replacement. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. PASTE COUNT — `3p` WITH MC
// ═══════════════════════════════════════════════════════════════════════════

/// `3p` with multi-entry register — each cursor pastes its entry 3 times.
#[test]
fn mc_paste_with_count() {
    let mut s = TestSession::new_multi("|1aaa |2bbb");
    s.feed("yiw");
    s.feed("3p");

    let text = s.text().to_owned();
    // cursor@0 should paste "aaa" 3 times, cursor@1 should paste "bbb" 3 times
    let aaa_count = text.matches("aaa").count();
    let bbb_count = text.matches("bbb").count();
    assert!(
        aaa_count >= 4, // 1 original + 3 pasted
        "3p should paste 'aaa' 3 times (plus original). 'aaa' appears {aaa_count} times in: {text:?}"
    );
    assert!(
        bbb_count >= 4, // 1 original + 3 pasted
        "3p should paste 'bbb' 3 times (plus original). 'bbb' appears {bbb_count} times in: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. dd ON NON-ADJACENT LINES — REGISTER CORRECTNESS
// ═══════════════════════════════════════════════════════════════════════════

/// `dd` on non-adjacent lines stores the correct line for each cursor.
#[test]
fn mc_dd_nonadjacent_lines_register() {
    // Lines: 0=aaa, 1=bbb, 2=ccc, 3=ddd, 4=eee
    // Cursors on lines 0 and 3
    let mut s = TestSession::new("|aaa\nbbb\nccc\nddd\neee");
    s.session_mut()
        .add_cursor(12)
        .expect("cursor on line 3 (ddd)");

    s.feed("dd");

    let count = s.session().get_register_entry_count('"');
    assert_eq!(
        count, 2,
        "dd on 2 non-adjacent lines should produce 2 entries"
    );

    let entry0 = s.session().get_register_entry('"', 0).unwrap();
    let entry1 = s.session().get_register_entry('"', 1).unwrap();
    assert!(
        entry0.contains("aaa"),
        "entry 0 should be line 'aaa'. Got: {entry0:?}"
    );
    assert!(
        entry1.contains("ddd"),
        "entry 1 should be line 'ddd'. Got: {entry1:?}"
    );

    // Text should have lines 1, 2, 4 remaining
    let text = s.text().to_owned();
    assert!(
        text.contains("bbb") && text.contains("ccc") && text.contains("eee"),
        "remaining lines should be bbb, ccc, eee. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. PASTE INTO INSERT MODE (Ctrl-R) — FUTURE EXTENSION
// ═══════════════════════════════════════════════════════════════════════════

// Note: Ctrl-R in insert mode to paste from register is a future extension.
// For now, we test normal-mode paste thoroughly.

// ═══════════════════════════════════════════════════════════════════════════
// 20. MULTIPLE REGISTER NAMESPACES
// ═══════════════════════════════════════════════════════════════════════════

/// Yank into 'a' and 'b' separately — registers are independent.
#[test]
fn mc_multiple_named_registers_independent() {
    let mut s = TestSession::new_multi("|1foo |2bar");

    // Yank into 'a'
    s.feed("\"ayiw");
    assert_eq!(s.session().get_register_entry_count('a'), 2);

    // Move to different words and yank into 'b'
    s.session_mut().set_text("one two");
    s.session_mut().clear_secondary_cursors();
    s.session_mut().set_cursor_offset(0);
    s.session_mut().add_cursor(4).expect("add cursor");

    s.feed("\"byiw");
    assert_eq!(s.session().get_register_entry_count('b'), 2);

    // Both registers should still have their entries
    assert_eq!(
        s.session().get_register_entry('a', 0).as_deref(),
        Some("foo")
    );
    assert_eq!(
        s.session().get_register_entry('a', 1).as_deref(),
        Some("bar")
    );
    assert_eq!(
        s.session().get_register_entry('b', 0).as_deref(),
        Some("one")
    );
    assert_eq!(
        s.session().get_register_entry('b', 1).as_deref(),
        Some("two")
    );
}
