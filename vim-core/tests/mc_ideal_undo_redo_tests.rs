//! Ideal multi-cursor undo/redo behavior tests.
//!
//! These tests describe how multi-cursor undo/redo SHOULD work. They codify
//! the design rules:
//!
//! - `u` reverts text at all cursors as ONE undo group
//! - Cursors PERSIST after undo (undo is text-only, cursor management is separate)
//! - `<C-r>` redo restores the undone changes at all cursors
//! - Multiple edits = multiple undo groups
//!
//! Tests may FAIL if the undo/redo system does not yet meet these ideals.
//! They serve as a specification for the target behavior.

#![allow(non_snake_case)]

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. BASIC UNDO: ciw + Esc + u — reverts at all cursors, cursors persist
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_ciw_reverts_all_cursors_preserves_count() {
    vim_mc("|1foo bar\n|2baz qux")
        .keys("ciwX<Esc>")
        .expect_text("|X bar\nX qux")
        .expect_cursor_count(2)
        .labeled("ciw+X replaces first word on each line")
        .keys("u")
        .expect_text("|foo bar\nbaz qux")
        .expect_cursor_count(2)
        .labeled("undo reverts ALL cursors; cursors MUST persist")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. UNDO + REDO ROUND TRIP
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_redo_round_trip() {
    vim_mc("|1aaa\n|2bbb")
        .keys("ciwX<Esc>")
        .expect_text("|X\nX")
        .labeled("ciw+X replaces words")
        .keys("u")
        .expect_text("|aaa\nbbb")
        .labeled("undo restores original")
        .keys("<C-r>")
        .expect_text("|X\nX")
        .labeled("redo restores the change")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. MULTIPLE EDITS: undo only undoes the last edit
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_multiple_edits_undoes_last_only() {
    vim_mc("|1alpha beta\n|2gamma delta")
        .keys("ciwX<Esc>")
        .expect_text("|X beta\nX delta")
        .labeled("first edit: replace first word with X")
        .keys("wciwY<Esc>")
        .expect_text("X |Y\nX Y")
        .labeled("second edit: replace second word with Y")
        .keys("u")
        .expect_text("X |beta\nX delta")
        .labeled("undo reverts only the LAST edit (Y -> original second words)")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. dd + u — delete line at each cursor, undo restores both
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_dd_restores_all_deleted_lines() {
    vim_mc("|1aaa\nbbb\n|2ccc\nddd")
        .keys("dd")
        .expect_text("|bbb\nddd")
        .labeled("dd deletes line at each cursor")
        .keys("u")
        .expect_text("|aaa\nbbb\nccc\nddd")
        .labeled("undo restores both deleted lines")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. u preserves cursor count — assert cursor_count after undo
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_preserves_cursor_count_after_x() {
    vim_mc("|1hello |2world")
        .keys("x")
        .expect_cursor_count(2)
        .labeled("two cursors after x")
        .keys("u")
        .expect_text("|hello world")
        .expect_cursor_count(2)
        .labeled("cursor count preserved after undo")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. u after single edit with 3 cursors — all 3 revert
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_three_cursors_all_revert() {
    vim_mc("|1aaa\n|2bbb\n|3ccc")
        .keys("ciwZ<Esc>")
        .expect_text("|Z\nZ\nZ")
        .expect_cursor_count(3)
        .labeled("all three words replaced with Z")
        .keys("u")
        .expect_text("|aaa\nbbb\nccc")
        .expect_cursor_count(3)
        .labeled("undo reverts all 3 cursors atomically")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. MULTIPLE UNDO: u u undoes two edits
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_double_undo_reverts_two_edits() {
    vim_mc("|1foo bar\n|2baz qux")
        .keys("ciwA<Esc>")
        .expect_text("|A bar\nA qux")
        .labeled("first edit: replace first word with A")
        .keys("wciwB<Esc>")
        .expect_text("A |B\nA B")
        .labeled("second edit: replace second word with B")
        .keys("u")
        .expect_text("A |bar\nA qux")
        .labeled("first undo reverts second edit")
        .keys("u")
        .expect_text("|foo bar\nbaz qux")
        .labeled("second undo reverts first edit")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. UNDO AFTER INSERT MODE: i text Esc + u
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_after_insert_mode() {
    vim_mc("|1hello\n|2world")
        .keys("iXY<Esc>")
        .expect_text("X|Yhello\nXYworld")
        .expect_cursor_count(2)
        .labeled("insert XY at beginning of each line")
        .keys("u")
        .expect_text("|hello\nworld")
        .labeled("undo removes inserted text from all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. REDO AFTER UNDO PRESERVES CURSORS TOO
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_redo_preserves_cursor_count() {
    vim_mc("|1cat\n|2dog")
        .keys("ciwpet<Esc>")
        .expect_text("pe|t\npet")
        .expect_cursor_count(2)
        .labeled("both words replaced")
        .keys("u")
        .expect_text("|cat\ndog")
        .expect_cursor_count(2)
        .labeled("undo: cursors persist")
        .keys("<C-r>")
        .expect_text("|pet\npet")
        .expect_cursor_count(2)
        .labeled("redo: cursors STILL persist")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. UNDO DOES NOT CLEAR MC STATE — cursors survive
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_does_not_clear_mc_state() {
    vim_mc("|1abc\n|2def")
        .keys("x")
        .expect_text("|bc\nef")
        .expect_cursor_count(2)
        .labeled("x deletes first char at each cursor")
        .keys("u")
        .expect_text("|abc\ndef")
        .expect_cursor_count(2)
        .labeled("undo restores text; MC state NOT cleared")
        .keys("x")
        .expect_text("|bc\nef")
        .expect_cursor_count(2)
        .labeled("MC still active: x works at both cursors after undo")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. UNDO ATOMIC: ciw + typed text = one undo group
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_ciw_plus_typing_is_one_undo_group() {
    vim_mc("|1foo |2bar")
        .keys("ciwREPLACED<Esc>")
        .expect_text("REPLACE|D REPLACED")
        .expect_cursor_count(2)
        .labeled("ciw + typing = single compound edit")
        .keys("u")
        .expect_text("|foo bar")
        .labeled("single u reverts the entire ciw+typing atomically")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. UNDO/REDO WITH DIFFERENT WORD LENGTHS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_redo_different_word_lengths() {
    vim_mc("|1hi |2there")
        .keys("ciwX<Esc>")
        .expect_text("|X X")
        .labeled("different-length words both replaced with X")
        .keys("u")
        .expect_text("|hi there")
        .labeled("undo restores different-length words correctly")
        .keys("<C-r>")
        .expect_text("|X X")
        .labeled("redo re-applies correctly")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. DELETE CHAR (x) + UNDO/REDO FULL CYCLE
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_x_undo_redo_full_cycle() {
    vim_mc("|1abcd\n|2efgh")
        .keys("x")
        .expect_text("|bcd\nfgh")
        .labeled("x deletes first char at each cursor")
        .keys("u")
        .expect_text("|abcd\nefgh")
        .labeled("undo restores deleted chars")
        .keys("<C-r>")
        .expect_text("|bcd\nfgh")
        .labeled("redo re-deletes chars")
        .keys("u")
        .expect_text("|abcd\nefgh")
        .labeled("second undo round-trip still works")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. SUBSTITUTE (s) + UNDO
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_substitute_undo() {
    vim_mc("|1abc\n|2def")
        .keys("sX<Esc>")
        .expect_text("|Xbc\nXef")
        .labeled("s replaces first char at each cursor with X")
        .keys("u")
        .expect_text("|abc\ndef")
        .labeled("undo restores substituted chars")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. CHANGE LINE (cc) + UNDO
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_cc_undo() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("ccnew<Esc>")
        .expect_text("ne|w\nnew")
        .labeled("cc replaces entire line content at each cursor")
        .keys("u")
        .expect_text("|hello world\nfoo bar")
        .labeled("undo restores both lines")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. DELETE TO END (D) + UNDO
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_D_undo() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("D")
        .expect_text("|\n")
        .labeled("D deletes to end of line at each cursor")
        .keys("u")
        .expect_text("|hello world\nfoo bar")
        .labeled("undo restores deleted text")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. THREE UNDO GROUPS: three edits, three undos back to original
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_three_edits_three_undos() {
    vim_mc("|1aaa bbb ccc\n|2ddd eee fff")
        .keys("ciwX<Esc>")
        .expect_text("|X bbb ccc\nX eee fff")
        .labeled("edit 1: replace first word")
        .keys("wciwY<Esc>")
        .expect_text("X |Y ccc\nX Y fff")
        .labeled("edit 2: replace second word")
        .keys("wciwZ<Esc>")
        .expect_text("X Y |Z\nX Y Z")
        .labeled("edit 3: replace third word")
        .keys("u")
        .expect_text("X Y |ccc\nX Y fff")
        .labeled("undo 1: reverts third edit")
        .keys("u")
        .expect_text("X |bbb ccc\nX eee fff")
        .labeled("undo 2: reverts second edit")
        .keys("u")
        .expect_text("|aaa bbb ccc\nddd eee fff")
        .labeled("undo 3: back to original")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. UNDO AFTER APPEND (A) + TYPING
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_after_append_end_of_line() {
    vim_mc("|1hello\n|2world")
        .keys("A!!<Esc>")
        .expect_text("hello!|!\nworld!!")
        .labeled("A appends !! at end of each line")
        .keys("u")
        .expect_text("hell|o\nworld")
        .labeled("undo removes appended text from both lines")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. REDO AFTER MULTIPLE UNDOS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_redo_after_multiple_undos() {
    vim_mc("|1foo\n|2bar")
        .keys("ciwA<Esc>")
        .expect_text("|A\nA")
        .labeled("edit 1: replace with A")
        .keys("ciwB<Esc>")
        .expect_text("|B\nB")
        .labeled("edit 2: replace A with B")
        .keys("u")
        .expect_text("|A\nA")
        .labeled("undo 1: back to A")
        .keys("u")
        .expect_text("|foo\nbar")
        .labeled("undo 2: back to original")
        .keys("<C-r>")
        .expect_text("|A\nA")
        .labeled("redo 1: forward to A")
        .keys("<C-r>")
        .expect_text("|B\nB")
        .labeled("redo 2: forward to B")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. UNDO AFTER OPERATIONS THAT CONTINUE WORKING POST-UNDO
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_then_new_edit_branches_undo_tree() {
    vim_mc("|1aaa\n|2bbb")
        .keys("ciwX<Esc>")
        .expect_text("|X\nX")
        .labeled("edit 1: replace with X")
        .keys("u")
        .expect_text("|aaa\nbbb")
        .labeled("undo back to original")
        .keys("ciwY<Esc>")
        .expect_text("|Y\nY")
        .labeled("new edit after undo: replace with Y (branches undo tree)")
        .keys("u")
        .expect_text("|aaa\nbbb")
        .labeled("undo the new branch edit")
        .run();
}
