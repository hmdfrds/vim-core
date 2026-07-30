//! Ideal multi-cursor INSERT MODE tests.
//!
//! These tests describe how multi-cursor insert mode SHOULD work. They may
//! fail against the current implementation -- that is intentional. Each test
//! documents the correct behavior so that when the implementation catches up,
//! these tests become the regression suite.
//!
//! Coverage:
//!   - `i`  insert before cursor, typing fans out
//!   - `a`  append after cursor, typing fans out
//!   - `I`  insert at line start, each cursor's line
//!   - `A`  append at line end, each cursor's line
//!   - `o`  open line below each cursor
//!   - `O`  open line above each cursor
//!   - Full insert cycle (`i` + typing + `<Esc>`)
//!   - Backspace (`<BS>`) during insert at each cursor
//!   - Enter (`<CR>`) during insert at each cursor
//!   - `ciw` + type replacement at all cursors
//!
//! Annotation convention:
//!   - Input uses `|1`, `|2`, `|3` for multi-cursor setup.
//!   - `expect_text` uses single `|` for primary cursor position only
//!     (the framework's standard pattern for multi-cursor builders).
//!   - `expect_cursor_count` separately asserts the number of active cursors.

#![allow(non_snake_case)]

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. `i` — INSERT BEFORE CURSOR
// ═══════════════════════════════════════════════════════════════════════════

/// `i` enters insert mode at each cursor. Typing a string fans out to all
/// cursor positions. After `<Esc>`, all cursors return to normal mode.
#[test]
fn mc_i_typing_fans_out_to_all_cursors() {
    vim_mc("|1foo\n|2bar")
        .keys("iHELLO <Esc>")
        .expect_text("HELLO| foo\nHELLO bar")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("i + typing fans out to both cursors")
        .run();
}

/// `i` with cursors mid-word inserts at each cursor's position independently.
/// Cursor 1 is at offset 1 (after 'a'), cursor 2 at offset 4 (after 'd').
/// `i` inserts before the cursor, so 'X' appears before 'b' and before 'e'.
#[test]
fn mc_i_mid_word_inserts_at_each_position() {
    vim_mc("a|1bc d|2ef")
        .keys("iX<Esc>")
        .expect_text("a|Xbc dXef")
        .expect_cursor_count(2)
        .labeled("i mid-word inserts at both positions")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. `a` — APPEND AFTER CURSOR
// ═══════════════════════════════════════════════════════════════════════════

/// `a` moves one char right then inserts. Each cursor appends independently.
#[test]
fn mc_a_append_after_each_cursor() {
    vim_mc("|1hello |2world")
        .keys("aX<Esc>")
        .expect_text("h|Xello wXorld")
        .expect_cursor_count(2)
        .labeled("a appends after each cursor")
        .run();
}

/// `a` on single-char words: cursor steps past the char then inserts.
#[test]
fn mc_a_single_char_words() {
    vim_mc("|1a |2b end")
        .keys("aZ<Esc>")
        .expect_text("a|Z bZ end")
        .expect_cursor_count(2)
        .labeled("a after single-char words")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. `I` — INSERT AT LINE START
// ═══════════════════════════════════════════════════════════════════════════

/// `I` moves to the first non-blank of each cursor's line, then inserts.
/// After `<Esc>`, cursor backs up one position (standard Vim behavior).
#[test]
fn mc_I_inserts_at_line_start() {
    vim_mc("hel|1lo\nwor|2ld")
        .keys("I>> <Esc>")
        .expect_text(">>| hello\n>> world")
        .expect_cursor_count(2)
        .labeled("I inserts at start of each cursor's line")
        .run();
}

/// `I` with indented lines inserts at the first non-blank character.
#[test]
fn mc_I_respects_leading_whitespace() {
    vim_mc("  hel|1lo\n    wor|2ld")
        .keys("IX<Esc>")
        .expect_text("  |Xhello\n    Xworld")
        .expect_cursor_count(2)
        .labeled("I goes to first non-blank on each line")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. `A` — APPEND AT LINE END
// ═══════════════════════════════════════════════════════════════════════════

/// `A` moves to end of each cursor's line, then inserts.
#[test]
fn mc_A_appends_at_each_line_end() {
    vim_mc("|1short\n|2longerline")
        .keys("A;<Esc>")
        .expect_text("short|;\nlongerline;")
        .expect_cursor_count(2)
        .labeled("A appends semicolons at each line end")
        .run();
}

/// `A` with lines of very different lengths.
#[test]
fn mc_A_different_line_lengths() {
    vim_mc("|1x\n|2abcdef\n|3hi")
        .keys("A!<Esc>")
        .expect_text("x|!\nabcdef!\nhi!")
        .expect_cursor_count(3)
        .labeled("A at end of lines with different lengths")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. `o` — OPEN LINE BELOW
// ═══════════════════════════════════════════════════════════════════════════

/// `o` opens a new line below each cursor's line and enters insert mode.
#[test]
fn mc_o_open_below_each_cursor() {
    vim_mc("|1aaa\n|2bbb")
        .keys("onew<Esc>")
        .expect_text("aaa\nne|w\nbbb\nnew")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("o opens line below each cursor")
        .run();
}

/// `o` with three cursors on three lines.
#[test]
fn mc_o_three_cursors_three_lines() {
    vim_mc("|1aaa\n|2bbb\n|3ccc")
        .keys("olol<Esc>")
        .expect_text("aaa\nlo|l\nbbb\nlol\nccc\nlol")
        .expect_cursor_count(3)
        .labeled("o with 3 cursors inserts line below each")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. `O` — OPEN LINE ABOVE
// ═══════════════════════════════════════════════════════════════════════════

/// `O` opens a new line above each cursor's line and enters insert mode.
#[test]
fn mc_O_open_above_each_cursor() {
    vim_mc("|1aaa\n|2bbb")
        .keys("Ohi<Esc>")
        .expect_text("h|i\naaa\nhi\nbbb")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("O opens line above each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. FULL INSERT CYCLE: `i` + typing + `<Esc>`
// ═══════════════════════════════════════════════════════════════════════════

/// Complete round-trip: enter insert, type multi-char string, exit.
/// Text appears at all cursors, mode returns to Normal, cursor count preserved.
#[test]
fn mc_i_full_cycle_text_at_all_cursors() {
    vim_mc("|1abc\n|2def")
        .keys("i")
        .expect_mode(Mode::Insert)
        .expect_cursor_count(2)
        .labeled("i enters insert with both cursors")
        .keys("XYZ")
        .expect_cursor_count(2)
        .labeled("cursors preserved during typing")
        .keys("<Esc>")
        .expect_mode(Mode::Normal)
        .expect_text("XY|Zabc\nXYZdef")
        .expect_cursor_count(2)
        .labeled("full insert cycle complete")
        .run();
}

/// Full cycle with `a` entry point.
#[test]
fn mc_a_full_cycle() {
    vim_mc("|1abc\n|2def")
        .keys("aXYZ<Esc>")
        .expect_text("aXY|Zbc\ndXYZef")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("a full cycle inserts after each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. `i` + `<BS>` — BACKSPACE DURING INSERT
// ═══════════════════════════════════════════════════════════════════════════

/// Backspace in insert mode deletes one char backward at each cursor.
#[test]
fn mc_i_backspace_deletes_at_each_cursor() {
    vim_mc("a|1bc d|2ef")
        .keys("i<BS><Esc>")
        .expect_text("|bc ef")
        .expect_cursor_count(2)
        .labeled("BS deletes backward at each cursor")
        .run();
}

/// Typing then backspace: net effect is the typed char minus one.
#[test]
fn mc_i_type_then_backspace() {
    vim_mc("|1abc\n|2def")
        .keys("iXY<BS><Esc>")
        .expect_text("|Xabc\nXdef")
        .expect_cursor_count(2)
        .labeled("type XY then BS leaves X at each cursor")
        .run();
}

/// Multiple backspaces delete multiple characters.
#[test]
fn mc_i_multiple_backspaces() {
    vim_mc("ab|1cd\nef|2gh")
        .keys("i<BS><BS><Esc>")
        .expect_text("|cd\ngh")
        .expect_cursor_count(2)
        .labeled("two backspaces delete two chars at each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. `i` + `<CR>` — ENTER DURING INSERT
// ═══════════════════════════════════════════════════════════════════════════

/// Enter in insert mode splits the line at each cursor position.
#[test]
fn mc_i_enter_splits_at_each_cursor() {
    vim_mc("ab|1cd\nef|2gh")
        .keys("i<CR><Esc>")
        .expect_text("ab\n|cd\nef\ngh")
        .expect_cursor_count(2)
        .labeled("CR splits line at each cursor")
        .run();
}

/// Enter at line start creates an empty line above.
#[test]
fn mc_i_enter_at_line_start() {
    vim_mc("|1abc\n|2def")
        .keys("i<CR><Esc>")
        .expect_text("\n|abc\n\ndef")
        .expect_cursor_count(2)
        .labeled("CR at line start creates empty line above at each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. `ciw` + TYPE + `<Esc>` — CHANGE INNER WORD
// ═══════════════════════════════════════════════════════════════════════════

/// `ciw` deletes the word under each cursor, enters insert, typed text
/// replaces each word independently.
#[test]
fn mc_ciw_replace_word_at_each_cursor() {
    vim_mc("|1hello |2world")
        .keys("ciw")
        .expect_mode(Mode::Insert)
        .expect_cursor_count(2)
        .labeled("ciw enters insert, deletes words")
        .keys("REPLACED<Esc>")
        .expect_text("REPLACE|D REPLACED")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("ciw replaced both words")
        .run();
}

/// `ciw` with words of different lengths.
#[test]
fn mc_ciw_different_length_words() {
    vim_mc("|1hi |2goodbye end")
        .keys("ciwX<Esc>")
        .expect_text("|X X end")
        .expect_cursor_count(2)
        .labeled("ciw handles different word lengths")
        .run();
}

/// `ciw` on three cursors, then undo reverts all atomically.
#[test]
fn mc_ciw_three_cursors_then_undo() {
    vim_mc("|1foo |2bar |3baz")
        .keys("ciwZ<Esc>")
        .expect_text("|Z Z Z")
        .expect_cursor_count(3)
        .labeled("ciw replaced all three words")
        .keys("u")
        .expect_text("|foo bar baz")
        .labeled("single undo reverts all ciw replacements")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// ADDITIONAL: EDGE CASES AND COMBINATIONS
// ═══════════════════════════════════════════════════════════════════════════

/// UTF-8 content: inserting multibyte characters fans out correctly.
#[test]
fn mc_i_utf8_insert_fans_out() {
    vim_mc("|1abc\n|2def")
        .keys("i\u{00e9}<Esc>") // e-acute
        .expect_text("|\u{00e9}abc\n\u{00e9}def")
        .expect_cursor_count(2)
        .labeled("UTF-8 char insert fans out to all cursors")
        .run();
}

/// `s` (substitute) deletes char under cursor and enters insert at each cursor.
#[test]
fn mc_s_substitute_at_each_cursor() {
    vim_mc("|1abc |2def")
        .keys("sX<Esc>")
        .expect_text("|Xbc Xef")
        .expect_cursor_count(2)
        .labeled("s substitutes at each cursor")
        .run();
}

/// `cc` changes entire line at each cursor, then typed text replaces each line.
#[test]
fn mc_cc_change_line_at_each_cursor() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("ccnew<Esc>")
        .expect_text("ne|w\nnew")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("cc replaces each cursor's entire line")
        .run();
}

/// `C` (change to end of line) at each cursor, then type replacement.
#[test]
fn mc_C_change_to_eol_at_each_cursor() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("CX<Esc>")
        .expect_text("|X\nX")
        .expect_cursor_count(2)
        .labeled("C changes to EOL at each cursor and types replacement")
        .run();
}

/// `S` (substitute line) at each cursor, equivalent to `cc`.
#[test]
fn mc_S_substitute_line_at_each_cursor() {
    vim_mc("|1aaa\n|2bbb")
        .keys("Snew<Esc>")
        .expect_text("ne|w\nnew")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("S replaces each cursor's line")
        .run();
}
