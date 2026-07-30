//! Ideal multi-cursor CHANGE operation tests.
//!
//! These tests describe how multi-cursor change operations SHOULD work.
//! They may fail under the current algebraic-replication architecture —
//! that is expected and intentional.
//!
//! The key scenario tested here: two or more cursors operating on content
//! of DIFFERENT lengths. Under algebraic rebase, the primary cursor's
//! edit range/text is blindly replicated to secondary cursors. When content
//! differs in length (e.g., `ciw` on "hi" vs "there"), the rebased range
//! is wrong. Per-cursor re-execution fixes this by computing each cursor's
//! edit independently.
//!
//! MC syntax: `|1` = cursor 1 (primary), `|2` = cursor 2, etc.

#![allow(non_snake_case)]

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. ciw — CHANGE INNER WORD
// ═══════════════════════════════════════════════════════════════════════════

/// `ciw` on words of different lengths: "short" (5) vs "longword" (8).
/// After `ciw` + typing "X" + Esc, both positions should become "X".
/// Algebraic rebase bug: primary deletes 5 bytes, secondary also gets a
/// 5-byte delete instead of 8 — leaves "ord" behind.
#[test]
fn mc_ciw_different_length_words() {
    vim_mc("|1short and |2longword here")
        .keys("ciwX<Esc>")
        .expect_text("|1X and |2X here")
        .labeled("ciw replaces each word independently regardless of length")
        .run();
}

/// `ciw` on words across different lines.
/// Each cursor's word is independent — no cross-line interference.
#[test]
fn mc_ciw_different_lines() {
    vim_mc("|1foo bar\n|2baz qux")
        .keys("ciwNEW<Esc>")
        .expect_text("|1NEW bar\n|2NEW qux")
        .labeled("ciw on different lines replaces each line's word")
        .run();
}

/// `ciw` where one word is 1 char and the other is 10 chars.
/// Maximum length disparity — if algebraic rebase uses primary's 1-byte
/// range on the secondary cursor, it only deletes 1 byte of "longerword".
#[test]
fn mc_ciw_extreme_length_disparity() {
    vim_mc("|1a middle |2longerword end")
        .keys("ciwZ<Esc>")
        .expect_text("|1Z middle |2Z end")
        .labeled("ciw with 1-char vs 10-char word — extreme length mismatch")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. caw — CHANGE A WORD (includes surrounding whitespace)
// ═══════════════════════════════════════════════════════════════════════════

/// `caw` on words of different lengths.
/// "a word" includes the word plus adjacent whitespace. The trailing space
/// after "hi" (1 space) vs after "there" (1 space) is the same, but word
/// length differs: 2 vs 5 bytes.
#[test]
fn mc_caw_different_length_words() {
    vim_mc("start |1hi end |2there end")
        .keys("cawX<Esc>")
        .expect_text("start |1Xend |2Xend")
        .labeled("caw deletes word+space at each cursor independently")
        .run();
}

/// `caw` on first vs last word of a line.
/// First word: trailing space consumed. Last word: leading space consumed.
/// Different boundary behavior at each cursor.
/// "alpha" caw -> deletes "alpha " (6 bytes: word + trailing space).
/// "delta" caw -> deletes " delta" (6 bytes: leading space + word).
#[test]
fn mc_caw_first_and_last_word() {
    vim_mc("|1alpha beta gamma |2delta")
        .keys("cawR<Esc>")
        .expect_text("|1Rbeta gamma|2R")
        .labeled("caw first word eats trailing space, last word eats leading space")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. cw — CHANGE TO END OF WORD
// ═══════════════════════════════════════════════════════════════════════════

/// `cw` on words of different lengths.
/// From cursor to end of word: "hi" = 2 bytes from start, "there" = 5 bytes.
/// Algebraic rebase copies primary's 2-byte range to secondary.
#[test]
fn mc_cw_different_lengths() {
    vim_mc("|1hi |2there end")
        .keys("cwX<Esc>")
        .expect_text("|1X |2X end")
        .labeled("cw changes to end of each cursor's word independently")
        .run();
}

/// `cw` on mid-word positions with different remaining lengths.
/// Cursor 1 is 1 char from end of "abc" (at 'c'), cursor 2 is 4 chars
/// from end of "defghij" (at 'g'). Remaining lengths: 1 vs 4.
#[test]
fn mc_cw_mid_word_different_remaining() {
    vim_mc("ab|1c xx defg|2hij yy")
        .keys("cwR<Esc>")
        .expect_text("ab|1R xx defg|2R yy")
        .labeled("cw from mid-word with different chars remaining")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. cc — CHANGE ENTIRE LINE
// ═══════════════════════════════════════════════════════════════════════════

/// `cc` on lines of different lengths.
/// Line 1: "short" (5 chars). Line 2: "a much longer line" (18 chars).
/// `cc` deletes the entire line content, enters insert mode.
/// Each line's content is independently replaced.
#[test]
fn mc_cc_different_line_lengths() {
    vim_mc("|1short\n|2a much longer line")
        .keys("ccX<Esc>")
        .expect_text("|1X\n|2X")
        .labeled("cc changes entire line at each cursor regardless of line length")
        .run();
}

/// `cc` on lines with different indentation levels.
/// Vim's `cc` preserves leading indentation (autoindent). With multi-cursor,
/// each cursor's line has its own indentation level.
#[test]
fn mc_cc_different_indentation() {
    vim_mc("|1  indented\n|2    more indented")
        .keys("ccX<Esc>")
        .expect_cursor_count(2)
        .labeled("cc on lines with different indentation")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. C — CHANGE TO END OF LINE
// ═══════════════════════════════════════════════════════════════════════════

/// `C` on lines where cursors are at different columns.
/// Cursor 1 at col 2 on a 5-char line (3 chars to delete).
/// Cursor 2 at col 1 on a 10-char line (9 chars to delete).
/// Algebraic rebase: primary's 3-char delete applied to secondary = wrong.
#[test]
fn mc_C_different_remaining_lengths() {
    vim_mc("ab|1cde\na|2bcdefghij")
        .keys("CX<Esc>")
        .expect_text("ab|1X\na|2X")
        .labeled("C deletes to EOL independently at each cursor")
        .run();
}

/// `C` with cursors at start of lines of different lengths.
/// Deletes the entire line content from column 0.
#[test]
fn mc_C_from_line_start() {
    vim_mc("|1hi\n|2a very long line here")
        .keys("CX<Esc>")
        .expect_text("|1X\n|2X")
        .labeled("C from col 0 on lines of 2 vs 20 chars")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. s — SUBSTITUTE CHARACTER
// ═══════════════════════════════════════════════════════════════════════════

/// `s` on characters of different byte widths (ASCII vs multi-byte).
/// Cursor 1 on 'a' (1 byte), cursor 2 on a 2-byte char.
/// Under algebraic rebase, primary's 1-byte delete is applied to secondary
/// — but the secondary's char is 2 bytes, so only half is deleted (corruption).
#[test]
fn mc_s_different_byte_widths() {
    vim_mc("|1a |2\u{00e9} end")
        .keys("sX<Esc>")
        .expect_text("|1X |2X end")
        .labeled("s substitutes chars of different byte widths independently")
        .run();
}

/// `s` on same-width ASCII characters — baseline.
/// This should work even with algebraic rebase since all chars are 1 byte.
#[test]
fn mc_s_same_byte_width_baseline() {
    vim_mc("|1abc |2def")
        .keys("sX<Esc>")
        .expect_text("|1Xbc |2Xef")
        .labeled("s baseline: same-width ASCII chars")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. ci" — CHANGE INSIDE QUOTES
// ═══════════════════════════════════════════════════════════════════════════

/// `ci"` with different-length quoted strings.
/// Cursor 1 inside "hi" (2 chars), cursor 2 inside "world" (5 chars).
/// Algebraic rebase copies primary's 2-byte inner range to secondary —
/// only deletes "wo" instead of "world".
#[test]
fn mc_ci_quote_different_lengths() {
    vim_mc("a \"|1hi\" b \"|2world\" c")
        .keys("ci\"X<Esc>")
        .expect_text("a \"|1X\" b \"|2X\" c")
        .labeled("ci\" on quoted strings of different lengths")
        .run();
}

/// `ci"` with one empty quoted string and one non-empty.
/// Edge case: primary's inner range is 0 bytes (empty string ""),
/// secondary's is 3 bytes ("abc").
#[test]
fn mc_ci_quote_empty_and_nonempty() {
    vim_mc("\"|1\" and \"|2abc\" end")
        .keys("ci\"X<Esc>")
        .expect_text("\"|1X\" and \"|2X\" end")
        .labeled("ci\" with empty vs non-empty quoted strings")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. ci( — CHANGE INSIDE PARENS
// ═══════════════════════════════════════════════════════════════════════════

/// `ci(` with paren groups of different sizes.
/// Cursor 1 inside (ab) — 2 chars. Cursor 2 inside (cdefgh) — 6 chars.
/// Algebraic rebase bug: primary's 2-byte inner range applied to secondary.
#[test]
fn mc_ci_paren_different_sizes() {
    vim_mc("(|1ab) (|2cdefgh)")
        .keys("ci(X<Esc>")
        .expect_text("(|1X) (|2X)")
        .labeled("ci( on paren groups of 2 vs 6 chars inside")
        .run();
}

/// `ci(` with nested and non-nested parens — structural difference.
/// Cursor 1 in simple parens (x). Cursor 2 in parens with nested: (a(b)c).
/// Inner for cursor 2 is "a(b)c" — includes the nested parens.
#[test]
fn mc_ci_paren_nested_vs_simple() {
    vim_mc("(|1x) (|2a(b)c)")
        .keys("ci(R<Esc>")
        .expect_text("(|1R) (|2R)")
        .labeled("ci( simple vs nested parens — different inner content")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. ct{char} — CHANGE TILL CHARACTER
// ═══════════════════════════════════════════════════════════════════════════

/// `ct.` where the target char '.' is at different distances from each cursor.
/// Cursor 1: 2 chars to '.'. Cursor 2: 5 chars to '.'.
/// Algebraic rebase: primary's 2-byte range applied to secondary = only
/// deletes 2 of the 5 chars before '.'.
#[test]
fn mc_ct_char_different_distances() {
    vim_mc("|1ab. |2cdefg. end")
        .keys("ct.X<Esc>")
        .expect_text("|1X. |2X. end")
        .labeled("ct. with target at different distances from each cursor")
        .run();
}

/// `ct;` across lines where ';' distances differ dramatically.
/// Cursor 1 is 1 char from ';'. Cursor 2 is 8 chars from ';'.
#[test]
fn mc_ct_char_extreme_distance() {
    vim_mc("|1a;\n|2bcdefghi; end")
        .keys("ct;X<Esc>")
        .expect_text("|1X;\n|2X; end")
        .labeled("ct; with 1-char vs 8-char distance")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. cf{char} — CHANGE FIND CHARACTER (inclusive)
// ═══════════════════════════════════════════════════════════════════════════

/// `cf.` — like ct. but inclusive (deletes the '.' too).
/// Cursor 1: "ab." (3 chars including '.'). Cursor 2: "cdefg." (6 chars).
/// Algebraic rebase copies primary's 3-byte range to secondary.
#[test]
fn mc_cf_char_different_distances() {
    vim_mc("|1ab. |2cdefg. end")
        .keys("cf.X<Esc>")
        .expect_text("|1X |2X end")
        .labeled("cf. inclusive change at different distances")
        .run();
}

/// `cf,` where the comma is at position 1 vs position 6 from cursor.
/// After cf, + "R" + Esc: both become "R".
#[test]
fn mc_cf_char_short_vs_long() {
    vim_mc("|1a, |2bcdefg, rest")
        .keys("cf,R<Esc>")
        .expect_text("|1R |2R rest")
        .labeled("cf, short vs long span to target character")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// COMPOSITE TESTS — multi-operation sequences
// ═══════════════════════════════════════════════════════════════════════════

/// `ciw` + type replacement + Esc, then undo should atomically revert ALL
/// cursors' changes in a single `u`.
#[test]
fn mc_ciw_type_then_undo() {
    vim_mc("|1short |2longword end")
        .keys("ciwNEW<Esc>")
        .expect_text("|1NEW |2NEW end")
        .labeled("ciw + type NEW at each cursor")
        .keys("u")
        .expect_text("|1short longword end")
        .labeled("single u reverts all cursor changes atomically")
        .run();
}

/// `C` then undo — verifies atomic undo for change-to-EOL.
#[test]
fn mc_C_then_undo() {
    vim_mc("aa|1bb\ncc|2dddddd")
        .keys("CX<Esc>")
        .expect_text("aa|1X\ncc|2X")
        .labeled("C + X at each cursor")
        .keys("u")
        .expect_text("aa|1bb\nccdddddd")
        .labeled("undo restores original line endings")
        .run();
}

/// `cc` on three cursors on three lines, each of different length.
/// Three-way length disparity stresses the rebase model.
#[test]
fn mc_cc_three_cursors_different_lengths() {
    vim_mc("|1a\n|2medium\n|3very long line here")
        .keys("ccZ<Esc>")
        .expect_text("|1Z\n|2Z\n|3Z")
        .labeled("cc with 3 cursors on lines of 1, 6, and 19 chars")
        .run();
}

/// `ci"` + undo round-trip — verifies the full change+undo path for
/// text objects with different inner ranges.
#[test]
fn mc_ci_quote_undo_roundtrip() {
    vim_mc("x \"|1ab\" y \"|2cdefg\" z")
        .keys("ci\"NEW<Esc>")
        .expect_text("x \"|1NEW\" y \"|2NEW\" z")
        .labeled("ci\" replaces different-length quoted content")
        .keys("u")
        .expect_text("x \"|1ab\" y \"cdefg\" z")
        .labeled("undo restores both original quoted strings")
        .run();
}

/// Sequence: `ciw` + type + Esc, then `w` motion, then `.` dot-repeat.
/// Tests that dot-repeat of a change operation works at new cursor positions
/// where the words may have different lengths again.
#[test]
fn mc_ciw_then_dot_repeat_at_new_position() {
    vim_mc("|1aa bb\n|2cc dd")
        .keys("ciwX<Esc>")
        .expect_text("|1X bb\n|2X dd")
        .labeled("ciw + X replaces first word on each line")
        .keys("w")
        .keys(".")
        .expect_text("X |1X\nX |2X")
        .labeled("dot-repeat replaces second word on each line")
        .run();
}
