//! Ideal multi-cursor motion tests.
//!
//! These tests describe how multi-cursor SHOULD work for basic motions.
//! Each cursor moves independently based on the text around IT, not the
//! primary cursor. Some tests may FAIL with the current algebraic rebase
//! model — that is expected. They define the target behavior.
//!
//! Motions tested:
//! - `w` (word forward)
//! - `b` (word backward)
//! - `e` (end of word)
//! - `0` (line start)
//! - `$` (line end)
//! - `^` (first non-blank)
//! - `f{char}` (find char forward)
//! - `t{char}` (till char forward)
//! - `F{char}` (find char backward)
//! - `gg` / `G` (document-level motions)

#![allow(non_snake_case)]

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. `w` — WORD FORWARD
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different lines with different word lengths.
/// Cursor 1 on "ab" (2-char word), cursor 2 on "ghijk" (5-char word).
/// After `w`: cursor 1 -> start of "cdef", cursor 2 -> start of "lm".
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_w_different_lines_different_word_lengths() {
    vim_mc("|1ab cdef\n|2ghijk lm")
        .keys("w")
        .expect_text("ab |1cdef\nghijk |2lm")
        .labeled("w moves each cursor to its own next word start")
        .run();
}

/// Three cursors, each on a line with a different first word length.
/// cursor 1 on "x" (1 char) -> next word "end" at col 2
/// cursor 2 on "longer_word" (11 chars) -> next word "tail" at col 12
/// cursor 3 on "ab" (2 chars) -> next word "cd" at col 3
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_w_three_cursors_varied_content() {
    vim_mc("|1x end\n|2longer_word tail\n|3ab cd ef")
        .keys("w")
        .expect_text("x |1end\nlonger_word |2tail\nab |3cd ef")
        .labeled("w with 3 cursors on lines of different structure")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. `b` — WORD BACKWARD
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different lines, each at the start of the second word.
/// cursor 1 on "end" -> `b` goes to "short" (6 chars back)
/// cursor 2 on "tail" -> `b` goes to "longer_word" (12 chars back)
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_b_different_lines_different_word_lengths() {
    vim_mc("short |1end\nlonger_word |2tail")
        .keys("b")
        .expect_text("|1short end\n|2longer_word tail")
        .labeled("b moves each cursor to its own previous word start")
        .run();
}

/// Three cursors going backward, each with different preceding content.
/// cursor 1 on "cd" -> `b` goes to "ab" (3 chars back)
/// cursor 2 on "kl" -> `b` goes to "efghij" (7 chars back)
/// cursor 3 on "n" -> `b` goes to "m" (2 chars back)
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_b_three_cursors_varied_distances() {
    vim_mc("ab |1cd\nefghij |2kl\nm |3n")
        .keys("b")
        .expect_text("|1ab cd\n|2efghij kl\n|3m n")
        .labeled("b with 3 cursors, different distances to previous word")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. `e` — END OF WORD
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors at word starts on different lines.
/// cursor 1 on "ab" -> `e` goes to 'b' (end of "ab", offset +1)
/// cursor 2 on "ghijk" -> `e` goes to 'k' (end of "ghijk", offset +4)
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_e_different_word_lengths() {
    vim_mc("|1ab cdef\n|2ghijk lm")
        .keys("e")
        .expect_text("a|1b cdef\nghij|2k lm")
        .labeled("e moves each cursor to end of its own word")
        .run();
}

/// Three cursors on words of different lengths.
/// cursor 1 on "aa" -> `e` goes to 'a' at offset 1
/// cursor 2 on "hello" -> `e` goes to 'o' at offset 4 within word
/// cursor 3 on "abcdef" -> `e` goes to 'f' at offset 5 within word
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_e_three_cursors_varied_words() {
    vim_mc("|1aa end\n|2hello world\n|3abcdef tail")
        .keys("e")
        .expect_text("a|1a end\nhell|2o world\nabcde|3f tail")
        .labeled("e with 3 cursors on words of length 2, 5, and 6")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. `0` — LINE START
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors at different columns on different lines. `0` sends each to
/// column 0 of its own line.
#[test]
fn mc_0_moves_each_cursor_to_own_line_start() {
    vim_mc("hello |1world\nfoo |2bar")
        .keys("0")
        .expect_text("|1hello world\n|2foo bar")
        .labeled("0 moves each cursor to column 0 of its line")
        .run();
}

/// Three cursors at varied positions — all go to column 0 of their lines.
#[test]
fn mc_0_three_cursors() {
    vim_mc("abc|1def\n  gh|2ij\nklmnop|3q")
        .keys("0")
        .expect_text("|1abcdef\n|2  ghij\n|3klmnopq")
        .labeled("0 with 3 cursors at varied columns")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. `$` — LINE END
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors at start of different-length lines. `$` sends each to the
/// last character of its own line.
/// Line 1: "short" (5 chars) -> last char at col 4
/// Line 2: "very long line here" (19 chars) -> last char at col 18
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_dollar_different_line_lengths() {
    vim_mc("|1short\n|2very long line here")
        .keys("$")
        .expect_text("shor|1t\nvery long line her|2e")
        .labeled("$ moves each cursor to end of its own line")
        .run();
}

/// Three cursors on lines of length 2, 6, and 1.
/// "ab" -> last char at col 1
/// "cdefgh" -> last char at col 5
/// "x" -> last char at col 0 (single char line, stays put)
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_dollar_three_cursors_varied_lengths() {
    vim_mc("|1ab\n|2cdefgh\n|3x")
        .keys("$")
        .expect_text("a|1b\ncdefg|2h\n|3x")
        .labeled("$ with 3 cursors on lines of very different lengths")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. `^` — FIRST NON-BLANK
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on lines with different indentation. `^` moves each to
/// the first non-whitespace character of its own line.
/// Line 1: "  hello world" -> first non-blank at col 2
/// Line 2: "    foo bar" -> first non-blank at col 4
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_caret_different_indentation() {
    vim_mc("  hello |1world\n    foo |2bar")
        .keys("^")
        .expect_text("  |1hello world\n    |2foo bar")
        .labeled("^ moves each cursor to first non-blank on its line")
        .run();
}

/// Three cursors with varying indentation: no indent, 2 spaces, 1 tab.
/// Line 1: "hello world" -> first non-blank at col 0
/// Line 2: "  foo bar" -> first non-blank at col 2
/// Line 3: "\tindented text" -> first non-blank at col 1 (byte offset of char after tab)
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_caret_three_cursors_varied_indent() {
    vim_mc("hello |1world\n  foo |2bar\n\tindented |3text")
        .keys("^")
        .expect_text("|1hello world\n  |2foo bar\n\t|3indented text")
        .labeled("^ with 3 cursors: no indent, 2-space indent, tab indent")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. `f{char}` — FIND CHAR FORWARD
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different lines where the target char 'x' appears at
/// different column offsets. Each cursor finds 'x' on its own line.
/// Line 1: "|1hello xworld" -> 'x' is at col 6, delta = +6
/// Line 2: "|2abxdef" -> 'x' is at col 2, delta = +2
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_f_finds_char_independently_per_line() {
    vim_mc("|1hello xworld\n|2abxdef")
        .keys("fx")
        .expect_text("hello |1xworld\nab|2xdef")
        .labeled("fx finds 'x' at different distances on each line")
        .run();
}

/// Three cursors searching for 'z' at very different positions.
/// Line 1: "|1azbcd" -> 'z' at col 1, delta +1
/// Line 2: "|2mnopqrz" -> 'z' at col 6, delta +6
/// Line 3: "|3xyzzy" -> 'z' at col 2, delta +2
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_f_three_cursors_varied_distances() {
    vim_mc("|1azbcd\n|2mnopqrz\n|3xyzzy")
        .keys("fz")
        .expect_text("a|1zbcd\nmnopqr|2z\nxy|3zzy")
        .labeled("fz with target at col 1, col 6, and col 2")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. `t{char}` — TILL CHAR FORWARD
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on different lines. `tx` moves each cursor to one position
/// before 'x' on its own line.
/// Line 1: "|1hello xworld" -> one before 'x' is col 5 (space), delta +5
/// Line 2: "|2abxdef" -> one before 'x' is col 1 ('b'), delta +1
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_t_till_char_independently_per_line() {
    vim_mc("|1hello xworld\n|2abxdef")
        .keys("tx")
        .expect_text("hello|1 xworld\na|2bxdef")
        .labeled("tx lands one before 'x' on each line independently")
        .run();
}

/// Three cursors with 'z' at different distances.
/// Line 1: "|1azbcd" -> one before 'z' at col 1 is col 0, delta 0 (stays)
/// Line 2: "|2mnopqrz" -> one before 'z' at col 6 is col 5, delta +5
/// Line 3: "|3xyzzy" -> one before 'z' at col 2 is col 1, delta +1
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_t_three_cursors_varied_distances() {
    vim_mc("|1azbcd\n|2mnopqrz\n|3xyzzy")
        .keys("tz")
        .expect_text("|1azbcd\nmnopq|2rz\nx|3yzzy")
        .labeled("tz with target at col 1, col 6, and col 2")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. `F{char}` — FIND CHAR BACKWARD
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors at end of different lines. `Fx` moves each backward
/// to 'x' on its own line.
/// Line 1: "xhello worl|1d" -> 'x' at col 0, delta -10
/// Line 2: "abxde|2f" -> 'x' at col 2, delta -3
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_F_finds_char_backward_independently() {
    vim_mc("xhello worl|1d\nabxde|2f")
        .keys("Fx")
        .expect_text("|1xhello world\nab|2xdef")
        .labeled("Fx finds 'x' backward on each line independently")
        .run();
}

/// Three cursors searching backward for 'a' at different distances.
/// Line 1: "abcde|1f" -> 'a' at col 0, delta -5
/// Line 2: "xyzaw|2q" -> 'a' at col 3, delta -2
/// Line 3: "amnopqrst|3u" -> 'a' at col 0, delta -9
// FAILS: algebraic rebase copies primary's delta to all cursors
#[test]
fn mc_F_three_cursors_varied_distances() {
    vim_mc("abcde|1f\nxyzaw|2q\namnopqrst|3u")
        .keys("Fa")
        .expect_text("|1abcdef\nxyz|2awq\n|3amnopqrstu")
        .labeled("Fa backward: target at col 0, col 3, and col 0")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. `gg` / `G` — DOCUMENT-LEVEL MOTIONS
// ═══════════════════════════════════════════════════════════════════════════

/// `gg` sends all cursors to line 0. Multiple cursors merge into one
/// because they all land at offset 0.
#[test]
fn mc_gg_all_cursors_go_to_first_line() {
    let s = vim_mc("aaa\n|1bbb\nccc\n|2ddd").keys("gg").run_session();
    let offset = s.cursor_offset();
    assert_eq!(offset, 0, "gg should move primary to offset 0");
    // All cursors converge on line 0, so cursor count may drop to 1
    // after merging. The key point: no cursor stays on its original line.
    assert!(
        s.cursor_count() <= 2,
        "gg should merge overlapping cursors (count: {})",
        s.cursor_count()
    );
}

/// `G` sends all cursors to the last line. Multiple cursors converge.
/// Uses run_session() and manual assertions since cursor merge is expected.
#[test]
fn mc_G_all_cursors_go_to_last_line() {
    let s = vim_mc("|1aaa\nbbb\n|2ccc\nddd").keys("G").run_session();
    let positions = s.cursor_positions();
    let last_line = s.text().lines().count() - 1;
    // All cursors should be on the last line
    for &(line, _col, _offset) in &positions {
        assert_eq!(
            line, last_line,
            "G should move all cursors to last line (line {last_line}), got line {line}"
        );
    }
}

/// `gg` with 3 cursors scattered across lines — all converge to line 0.
#[test]
fn mc_gg_three_cursors_converge() {
    let s = vim_mc("|1first\n|2second\nthird\n|3fourth")
        .keys("gg")
        .run_session();
    let offset = s.cursor_offset();
    assert_eq!(offset, 0, "gg should move primary to offset 0");
}

/// `G` with 3 cursors — all go to the last line of the document.
/// Uses run_session() and manual assertions since cursor merge is expected.
#[test]
fn mc_G_three_cursors_converge_to_last_line() {
    let s = vim_mc("|1first\nsecond\n|2third\nfourth\n|3fifth")
        .keys("G")
        .run_session();
    let positions = s.cursor_positions();
    let last_line = s.text().lines().count() - 1;
    for &(line, _col, _offset) in &positions {
        assert_eq!(
            line, last_line,
            "G: all cursors should land on last line {last_line}, but one is on line {line}"
        );
    }
}
