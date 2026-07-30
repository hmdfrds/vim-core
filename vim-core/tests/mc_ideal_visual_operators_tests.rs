//! Ideal multi-cursor visual-mode operator tests.
//!
//! These tests describe how multi-cursor SHOULD work for visual mode
//! selections and operators applied to those selections. Each cursor has
//! its own independent selection. Some tests may FAIL with the current
//! implementation -- that is expected. They define the target behavior.
//!
//! Categories tested:
//! - `viw`  -- visual select inner word at each cursor independently
//! - `v$`   -- visual select to end of line at each cursor
//! - `V`    -- visual line select at each cursor's line
//! - Visual `d`  -- delete visual selection at each cursor
//! - Visual `c`  -- change visual selection at each cursor
//! - Visual `y`  -- yank visual selection at each cursor (per-cursor entries)
//! - Visual `~`  -- swap case of selection at each cursor
//! - Visual `U`  -- uppercase selection at each cursor
//! - Visual `>`  -- indent selection at each cursor
//! - `viwp` -- visual select word then paste (replace) at each cursor
//! - Visual mode extend: `v` then `e` to extend selection
//! - `gv` after MC edit -- reselect last visual at each cursor

#![allow(non_snake_case)]

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. `viw` -- VISUAL SELECT INNER WORD
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different words; `viwd` selects the inner word at each
/// cursor independently, then `d` deletes both selections.
/// Cursor 1 is on "hello" (5 chars), cursor 2 is on "foo" (3 chars).
// FAILS: visual selections may not be per-cursor independent
#[test]
fn mc_viw_delete_different_word_lengths() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("viwd")
        .expect_text("| world\n bar")
        .expect_mode(Mode::Normal)
        .labeled("viwd deletes each cursor's own inner word")
        .run();
}

/// Three cursors on words of different lengths. Each `viw` selects only
/// the word under that specific cursor.
// FAILS: visual selections may not be per-cursor independent
#[test]
fn mc_viw_delete_three_cursors_varied_words() {
    vim_mc("|1ab cdef\n|2ghijk lm\n|3x end")
        .keys("viwd")
        .expect_text("| cdef\n lm\n end")
        .expect_mode(Mode::Normal)
        .labeled("viwd with 3 cursors on words of length 2, 5, and 1")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. `v$` -- VISUAL SELECT TO END OF LINE
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors at different columns on different-length lines. `v$d`
/// seletes from each cursor to the end of its own line.
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_v_dollar_delete_different_line_lengths() {
    vim_mc("ab|1cdef\nxy|2z")
        .keys("v$d")
        .expect_text("ab\nxy")
        .expect_mode(Mode::Normal)
        .labeled("v$d deletes from each cursor to its own EOL")
        .run();
}

/// Three cursors at different columns. `v$d` should delete from each
/// cursor position to the end of its respective line.
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_v_dollar_delete_three_cursors() {
    vim_mc("|1hello world\nab|2cde\n|3x")
        .keys("v$d")
        .expect_text("\nab\n")
        .expect_mode(Mode::Normal)
        .labeled("v$d with 3 cursors at varied columns")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. `V` -- VISUAL LINE SELECT
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on separate lines. `V` enters linewise visual mode for
/// each cursor's line independently.
#[test]
fn mc_V_line_select_enters_visual_line() {
    vim_mc("|1hello\n|2world")
        .keys("V")
        .expect_mode(Mode::Visual(VisualType::Line))
        .expect_cursor_count(2)
        .labeled("V enters visual-line mode with both cursors active")
        .run();
}

/// Two cursors on different lines. `Vd` deletes each cursor's entire line.
// FAILS: visual line operations may not be per-cursor independent
#[test]
fn mc_V_delete_each_cursors_line() {
    vim_mc("|1aaa\nbbb\n|2ccc\nddd")
        .keys("Vd")
        .expect_text("|bbb\nddd")
        .expect_mode(Mode::Normal)
        .labeled("Vd deletes the line at each cursor independently")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. VISUAL `d` -- DELETE VISUAL SELECTION
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors; charwise visual select a word at each, then `d`.
/// This is essentially `viwd` but split into steps to verify the selection
/// phase is independent before the delete.
// FAILS: visual selections may not be per-cursor independent
#[test]
fn mc_visual_d_after_iw_selection() {
    vim_mc("|1hello |2world")
        .keys("viw")
        .expect_mode(Mode::Visual(VisualType::Char))
        .expect_cursor_count(2)
        .labeled("viw creates charwise visual selection at both cursors")
        .keys("d")
        .expect_text("| ")
        .expect_mode(Mode::Normal)
        .labeled("d in visual mode deletes each selection")
        .run();
}

/// Cursors on different lines with different word lengths. Visual select
/// inner word, then delete. Text between the words should survive.
// FAILS: visual selections may not be per-cursor independent
#[test]
fn mc_visual_d_different_lines_different_words() {
    vim_mc("|1short end\n|2longer_word tail")
        .keys("viwd")
        .expect_text("| end\n tail")
        .expect_mode(Mode::Normal)
        .labeled("visual d deletes different-length words on different lines")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. VISUAL `c` -- CHANGE VISUAL SELECTION
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different words. `viwc` deletes each word and enters
/// insert mode. Typing a replacement should appear at both cursor positions.
// FAILS: visual change with MC may not work per-cursor
#[test]
fn mc_visual_c_replaces_each_selection() {
    vim_mc("|1hello |2world")
        .keys("viwcX<Esc>")
        .expect_text("|X X")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("viwc + typing replaces each word independently")
        .run();
}

/// Cursors on different lines. `viwc` + typed replacement should work
/// at each cursor location with its own word length.
// FAILS: visual change with MC may not work per-cursor
#[test]
fn mc_visual_c_different_lines() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("viwcRE<Esc>")
        .expect_text("R|E world\nRE bar")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("viwc replaces words on different lines")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. VISUAL `y` -- YANK VISUAL SELECTION (per-cursor register entries)
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different words. `viwy` yanks each word into the
/// default register. The register should have 2 entries: "hello" and "world".
// FAILS: per-cursor register entries may not be stored
#[test]
fn mc_visual_yank_stores_per_cursor_entries() {
    let s = vim_mc("|1hello |2world").keys("viwy").run_session();
    let count = s.session().get_register_entry_count('"');
    assert_eq!(
        count, 2,
        "viwy with 2 cursors should store 2 register entries, got {count}"
    );
    let entry0 = s.session().get_register_entry('"', 0);
    let entry1 = s.session().get_register_entry('"', 1);
    assert_eq!(
        entry0.as_deref(),
        Some("hello"),
        "first entry should be 'hello'"
    );
    assert_eq!(
        entry1.as_deref(),
        Some("world"),
        "second entry should be 'world'"
    );
}

/// Three cursors on words of different length. Yank should create 3 entries.
// FAILS: per-cursor register entries may not be stored
#[test]
fn mc_visual_yank_three_cursors_different_words() {
    let s = vim_mc("|1ab |2cdefg |3hi").keys("viwy").run_session();
    let count = s.session().get_register_entry_count('"');
    assert_eq!(
        count, 3,
        "viwy with 3 cursors should store 3 register entries, got {count}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. VISUAL `~` -- SWAP CASE OF SELECTION
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different words. `viw~` toggles case of each word
/// independently.
// FAILS: visual tilde with MC may not work per-cursor
#[test]
fn mc_visual_tilde_swap_case_each_word() {
    vim_mc("|1Hello |2World")
        .keys("viw~")
        .expect_text("|hELLO wORLD")
        .expect_mode(Mode::Normal)
        .labeled("viw~ swaps case of each word independently")
        .run();
}

/// Mixed-case words on different lines. Each cursor's word gets its case
/// toggled independently based on the characters under that selection.
// FAILS: visual tilde with MC may not work per-cursor
#[test]
fn mc_visual_tilde_different_lines() {
    vim_mc("|1aBC\n|2Def")
        .keys("viw~")
        .expect_text("|Abc\ndEF")
        .expect_mode(Mode::Normal)
        .labeled("viw~ on different lines with mixed case")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. VISUAL `U` -- UPPERCASE SELECTION
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on lowercase words. `viwU` uppercases each word.
// FAILS: visual uppercase with MC may not work per-cursor
#[test]
fn mc_visual_U_uppercase_each_word() {
    vim_mc("|1hello |2world")
        .keys("viwU")
        .expect_text("|HELLO WORLD")
        .expect_mode(Mode::Normal)
        .labeled("viwU uppercases each word independently")
        .run();
}

/// Lowercase `u` in visual mode lowercases. Mixed-case input at each cursor.
// FAILS: visual lowercase with MC may not work per-cursor
#[test]
fn mc_visual_u_lowercase_each_word() {
    vim_mc("|1HELLO |2WORLD")
        .keys("viwu")
        .expect_text("|hello world")
        .expect_mode(Mode::Normal)
        .labeled("viwu lowercases each word independently")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. VISUAL `>` -- INDENT SELECTION
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different lines. `V>` indents each cursor's line.
/// Default shiftwidth is used for indentation.
// FAILS: visual indent with MC may not work per-cursor
#[test]
fn mc_visual_line_indent_both_lines() {
    let s = vim_mc("|1hello\n|2world").keys("V>").run_session();
    let text = s.text();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() >= 2, "should still have 2 lines, got: {text:?}");
    assert!(
        lines[0].starts_with(' ') || lines[0].starts_with('\t'),
        "line 0 should be indented: {:?}",
        lines[0]
    );
    assert!(
        lines[1].starts_with(' ') || lines[1].starts_with('\t'),
        "line 1 should be indented: {:?}",
        lines[1]
    );
    assert_eq!(
        s.mode(),
        Mode::Normal,
        "should return to normal after indent"
    );
}

/// Three cursors on three lines. `V>` should indent all three.
// FAILS: visual indent with MC may not work per-cursor
#[test]
fn mc_visual_line_indent_three_lines() {
    let s = vim_mc("|1aaa\n|2bbb\n|3ccc").keys("V>").run_session();
    let text = s.text();
    for (i, line) in text.lines().enumerate() {
        assert!(
            line.starts_with(' ') || line.starts_with('\t'),
            "line {i} should be indented: {line:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. `viwp` -- VISUAL SELECT WORD THEN PASTE (REPLACE)
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different words. First yank "NEW" into the register,
/// then `viwp` at each cursor replaces its word with the register contents.
// FAILS: visual paste-replace with MC may not work per-cursor
#[test]
fn mc_viw_paste_replaces_each_word() {
    vim_mc("|1hello |2world")
        .set_register('"', "NEW")
        .keys("viwp")
        .expect_text("|NEW NEW")
        .expect_mode(Mode::Normal)
        .labeled("viwp replaces each word with register contents")
        .run();
}

/// Paste-replace with different-length words. The replacement should be
/// independent of the original word length at each cursor.
// FAILS: visual paste-replace with MC may not work per-cursor
#[test]
fn mc_viw_paste_different_word_lengths() {
    vim_mc("|1ab |2cdefgh")
        .set_register('"', "X")
        .keys("viwp")
        .expect_text("|X X")
        .expect_mode(Mode::Normal)
        .labeled("viwp replaces words of different lengths with same register")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. VISUAL MODE EXTEND: `v` THEN `e` TO EXTEND SELECTION
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors at word starts. `v` enters visual, then `e` extends each
/// selection to the end of its own word. `d` deletes both selections.
/// Word lengths differ (5 vs 3), so each selection covers a different range.
// FAILS: visual extend with MC may not work per-cursor
#[test]
fn mc_visual_extend_with_e_then_delete() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("ved")
        .expect_text("| world\n bar")
        .expect_mode(Mode::Normal)
        .labeled("v then e extends to each word end, d deletes each selection")
        .run();
}

/// Three cursors. `vel` extends visual selection to end-of-word then one
/// more character at each cursor. Each word has different length.
// FAILS: visual extend with MC may not work per-cursor
#[test]
fn mc_visual_extend_e_different_words_then_delete() {
    vim_mc("|1ab cd\n|2efghij kl\n|3m n")
        .keys("ved")
        .expect_text("| cd\n kl\n n")
        .expect_mode(Mode::Normal)
        .labeled("ve extends to word end independently at each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. `gv` AFTER MC EDIT -- RESELECT LAST VISUAL AT EACH CURSOR
// ═══════════════════════════════════════════════════════════════════════════

/// After a visual operation (`viw~`), `gv` should reselect the last visual
/// selection region at each cursor. Then another operator can be applied.
// FAILS: gv with MC may not track per-cursor last-visual regions
#[test]
fn mc_gv_reselect_after_visual_tilde() {
    vim_mc("|1hello |2world")
        .keys("viw~")
        .expect_text("|hELLO wORLD")
        .labeled("viw~ first pass toggles case")
        .keys("gv~")
        .expect_text("|Hello World")
        .expect_mode(Mode::Normal)
        .labeled("gv~ reselects and toggles case back")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. UNDO ATOMICITY FOR VISUAL MC OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════

/// `viwd` across two cursors should be a single undo group. One `u` should
/// revert all deletions.
// FAILS: undo atomicity with MC visual operations may be broken
#[test]
fn mc_visual_delete_undo_atomic() {
    vim_mc("|1hello |2world end")
        .keys("viwd")
        .expect_text("| | end")
        .labeled("viwd deletes both words")
        .keys("u")
        .expect_text("|hello world end")
        .labeled("single u reverts both deletions atomically")
        .run();
}

/// `viwcX<Esc>` (visual change + typed text) across two cursors. One `u`
/// should revert the entire operation (delete + insert) at all cursors.
// FAILS: undo atomicity with MC visual change may be broken
#[test]
fn mc_visual_change_undo_atomic() {
    vim_mc("|1hello |2world")
        .keys("viwcX<Esc>")
        .expect_text("|X X")
        .labeled("viwc + type replaces both words")
        .keys("u")
        .expect_text("|hello world")
        .labeled("single u reverts visual change at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. VISUAL MODE WITH CURSORS ON SAME LINE
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on the same line, on different words. Visual inner-word
/// select + delete should remove both words without interfering.
// FAILS: same-line MC visual operations are particularly tricky
#[test]
fn mc_viw_delete_same_line_two_words() {
    vim_mc("|1hello |2world end")
        .keys("viwd")
        .expect_text("|  end")
        .expect_mode(Mode::Normal)
        .labeled("viwd deletes two words on the same line")
        .run();
}
