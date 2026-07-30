//! Ideal multi-cursor DELETE operation tests.
//!
//! These tests describe how multi-cursor delete operations SHOULD work.
//! Each test focuses on content-dependent scenarios where cursors operate
//! on text of different lengths/content, exposing cases where a naive
//! algebraic rebase model would produce wrong deletion ranges.
//!
//! Key principle: each cursor's delete is computed independently against
//! the document content at that cursor's position. The shorter word at
//! cursor 1 must NOT affect the deletion range computed for the longer
//! word at cursor 2.

#![allow(non_snake_case)]

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. diw — delete inner word (different-length words)
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on words of different lengths. `diw` must delete each word
/// independently: "ab" (2 chars) and "fghij" (5 chars).
///
/// Bug scenario: algebraic rebase uses primary's 2-byte range for the
/// secondary cursor, deleting only "fg" instead of "fghij".
#[test]
fn mc_diw_different_length_words() {
    vim_mc("|1ab cde |2fghij end")
        .keys("diw")
        .expect_text("|1 cde |2 end")
        .labeled("diw deletes 2-char and 5-char words independently")
        .run();
}

/// Three cursors on words of lengths 1, 4, and 7. Each inner word
/// deletion must use the correct range for its own word.
#[test]
fn mc_diw_three_different_lengths() {
    vim_mc("|1a bb |2cccc ddd |3eeeeeee end")
        .keys("diw")
        .expect_text("|1 bb |2 ddd |3 end")
        .labeled("diw on 1, 4, and 7 char words")
        .run();
}

/// Cursors mid-word on different-length words. The inner word text object
/// finds the full word boundary regardless of cursor column.
/// Use separate lines to prevent cursor merging after deletion.
#[test]
fn mc_diw_cursor_mid_word() {
    vim_mc("he|1llo end1\nwor|2ld end2")
        .keys("diw")
        .expect_text("|1 end1\n|2 end2")
        .labeled("diw with cursor in the middle of each word")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. daw — delete a word (includes surrounding whitespace)
// ═══════════════════════════════════════════════════════════════════════════

/// `daw` deletes the word plus surrounding whitespace. With words of
/// different lengths, the total deletion range differs per cursor.
#[test]
fn mc_daw_different_length_words() {
    vim_mc("|1short and |2longer end")
        .keys("daw")
        .expect_text("|1and |2end")
        .labeled("daw deletes word + trailing space independently per cursor")
        .run();
}

/// `daw` on words with different lengths: "short" (5) and "toolong" (7).
/// The "a word" text object includes trailing whitespace. Each cursor's
/// deletion range is determined by its own word length + surrounding space.
#[test]
fn mc_daw_words_different_lengths() {
    vim_mc("begin |1short middle |2toolong end")
        .keys("daw")
        .expect_text("begin |1middle |2end")
        .labeled("daw on 5-char and 7-char words with trailing space")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. dw — delete to next word
// ═══════════════════════════════════════════════════════════════════════════

/// Classic content-dependent scenario: cursor 1 on "hello" (5 chars + space),
/// cursor 2 on "hi" (2 chars + space). Each deletes to its own next word.
#[test]
fn mc_dw_different_content() {
    vim_mc("|1hello world\n|2hi there")
        .keys("dw")
        .expect_text("|1world\n|2there")
        .labeled("dw deletes to next word independently per cursor")
        .run();
}

/// `dw` with three cursors on words of lengths 2, 5, and 1.
/// The motion finds the next word boundary from each cursor position.
#[test]
fn mc_dw_three_cursors_varied_lengths() {
    vim_mc("|1ab cd\n|2efghi jk\n|3l mn")
        .keys("dw")
        .expect_text("|1cd\n|2jk\n|3mn")
        .labeled("dw with 3 cursors on 2, 5, and 1 char words")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. dd — delete line (each cursor on different line)
// ═══════════════════════════════════════════════════════════════════════════

/// Lines of different lengths. `dd` deletes the entire line at each cursor.
/// The line lengths (5, 14) differ, so algebraic rebase would apply the
/// wrong range to the secondary cursor.
#[test]
fn mc_dd_different_line_lengths() {
    vim_mc("|1short\nkeep\n|2very long line\nstay")
        .keys("dd")
        .expect_text("|1keep\n|2stay")
        .labeled("dd deletes lines of different lengths independently")
        .run();
}

/// Three cursors on alternating lines. Only those lines are deleted;
/// the lines between them are preserved.
#[test]
fn mc_dd_three_alternating_lines() {
    vim_mc("|1aaa\nbbb\n|2ccc\nddd\n|3eee\nfff")
        .keys("dd")
        .expect_text("|1bbb\n|2ddd\n|3fff")
        .labeled("dd on lines 0, 2, 4 preserves lines 1, 3, 5")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. D — delete to end of line
// ═══════════════════════════════════════════════════════════════════════════

/// Cursors at different columns on lines of different lengths. `D` deletes
/// from cursor to end of line — the deletion range depends on how much
/// text remains after the cursor on each line.
/// After `D`, cursor lands on the last remaining character on the line
/// (Vim behavior: `D` = `d$`, cursor moves back to last char).
#[test]
fn mc_D_different_remaining_lengths() {
    vim_mc("ab|1cde\nf|2ghijklm")
        .keys("D")
        .expect_text("a|1b\n|2f")
        .labeled("D deletes different amounts based on remaining line length")
        .run();
}

/// `D` with cursor at start of lines of very different lengths.
#[test]
fn mc_D_from_line_start() {
    vim_mc("|1hello world\n|2hi")
        .keys("D")
        .expect_text("|1\n|2")
        .labeled("D from start of 11-char and 2-char lines")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. x — delete char at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Basic `x` with two cursors. The character under each cursor is deleted.
/// With ASCII, this is 1 byte each, so algebraic rebase happens to work.
/// The real test is ensuring both characters are removed.
#[test]
fn mc_x_basic_two_cursors() {
    vim_mc("|1abcd |2efgh")
        .keys("x")
        .expect_text("|1bcd |2fgh")
        .labeled("x deletes char at each cursor")
        .run();
}

/// `3x` deletes 3 chars at each cursor. Cursor 1 has 6 chars available,
/// cursor 2 only has 2 (clamped). After deletion, cursor 2 moves back to
/// the last valid position.
#[test]
fn mc_x_with_count() {
    vim_mc("|1abcdef |2gh")
        .keys("3x")
        .expect_text("|1def|2 ")
        .labeled("3x deletes 3 chars at cursor 1, clamped at cursor 2")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. X — delete char before each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// `X` deletes the character before the cursor. Each cursor's preceding
/// character is independent.
#[test]
fn mc_X_basic_two_cursors() {
    vim_mc("a|1bcd e|2fgh")
        .keys("X")
        .expect_text("|1bcd |2fgh")
        .labeled("X deletes char before each cursor")
        .run();
}

/// `X` where one cursor is at column 0 (no char to delete) and the other
/// is mid-line. Column-0 cursor should be a no-op.
#[test]
fn mc_X_one_at_column_zero() {
    vim_mc("|1abcd\ne|2fgh")
        .keys("X")
        .expect_text("|1abcd\n|2fgh")
        .labeled("X at col 0 is no-op, mid-line cursor deletes backward")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. di" — delete inside quotes at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Cursors inside quoted strings of different lengths. `di"` should delete
/// the content inside quotes at each cursor independently.
///
/// Bug scenario: primary's quoted content is 2 bytes ("ab"), secondary's
/// is 5 bytes ("hello"). Rebase applies primary's 2-byte range to the
/// secondary, deleting only "he" instead of "hello".
#[test]
fn mc_di_quote_different_lengths() {
    vim_mc("\"a|1b\" \"hel|2lo\"")
        .keys("di\"")
        .expect_text("\"|1\" \"|2\"")
        .labeled("di\" on 2-char and 5-char quoted strings")
        .run();
}

/// Three cursors inside quotes with content lengths 1, 3, and 6.
#[test]
fn mc_di_quote_three_cursors() {
    vim_mc("\"|1x\" \"|2abc\" \"|3fghijk\"")
        .keys("di\"")
        .expect_text("\"|1\" \"|2\" \"|3\"")
        .labeled("di\" on 1, 3, and 6 char quoted strings")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. di( — delete inside parens at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Cursors inside parenthesized groups of different sizes.
///
/// Bug scenario: primary's paren content is 2 bytes, secondary's is 7.
/// Algebraic rebase uses primary's range length for both.
#[test]
fn mc_di_paren_different_lengths() {
    vim_mc("(a|1b) (hel|2lo w)")
        .keys("di(")
        .expect_text("(|1) (|2)")
        .labeled("di( on 2-char and 7-char paren contents")
        .run();
}

/// Mixed bracket types are independent — `di(` only cares about `()`.
/// Each cursor must find its own surrounding `()`.
#[test]
fn mc_di_paren_nested_context() {
    vim_mc("(|1x) [skip] (|2hello world)")
        .keys("di(")
        .expect_text("(|1) [skip] (|2)")
        .labeled("di( with different-size paren groups, bracket group ignored")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. df{char} — delete find char
// ═══════════════════════════════════════════════════════════════════════════

/// `df.` where the target char '.' is at different distances from each
/// cursor. Cursor 1 needs to delete 2 bytes to reach '.', cursor 2 needs
/// to delete 4 bytes.
///
/// Bug scenario: primary's 2-byte range is applied to secondary, deleting
/// only "he" instead of "hell" + the '.'.
#[test]
fn mc_df_char_different_distances() {
    vim_mc("|1a.bc |2hell.o")
        .keys("df.")
        .expect_text("|1bc |2o")
        .labeled("df. with '.' at distance 2 and 5 from each cursor")
        .run();
}

/// `df,` with three cursors at varying distances to the next ','.
/// Use separate lines to prevent cursor merging after deletion.
#[test]
fn mc_df_char_three_cursors() {
    vim_mc("|1a, rest1\n|2bcd, rest2\n|3efghij, rest3")
        .keys("df,")
        .expect_text("|1 rest1\n|2 rest2\n|3 rest3")
        .labeled("df, at distances 2, 4, and 7")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. COMPOSITE: diw + undo round-trip
// ═══════════════════════════════════════════════════════════════════════════

/// `diw` on different-length words, then undo should restore the original
/// text atomically. This validates that the undo group captures ALL
/// per-cursor deletions.
#[test]
fn mc_diw_undo_round_trip() {
    vim_mc("|1short |2longword end")
        .keys("diw")
        .expect_text("|1 |2 end")
        .labeled("diw removes both words")
        .keys("u")
        .expect_text("|short longword end")
        .labeled("undo restores original text atomically")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. dd on adjacent lines with very different content
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on adjacent lines. `dd` on both should delete both lines,
/// leaving only the surrounding lines.
#[test]
fn mc_dd_adjacent_lines() {
    vim_mc("keep\n|1delete me\n|2also delete\nstay")
        .keys("dd")
        .expect_text("keep\n|1stay")
        .labeled("dd on two adjacent lines")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. D with cursors at different columns
// ═══════════════════════════════════════════════════════════════════════════

/// D where one cursor is near the start and another near the end of its
/// line. The amount deleted differs dramatically.
/// Cursor at col 0 on "abcdefghij": D deletes everything, cursor stays
/// at the now-empty line. Cursor at col 8 on "klmnopqrst": D deletes "st",
/// cursor moves back to 'r' (last remaining char).
#[test]
fn mc_D_asymmetric_columns() {
    vim_mc("|1abcdefghij\nklmnopqr|2st")
        .keys("D")
        .expect_text("|1\nklmnopq|2r")
        .labeled("D from col 0 (10 chars) vs col 8 (2 chars)")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. x on multi-byte (UTF-8) characters
// ═══════════════════════════════════════════════════════════════════════════

/// `x` where one cursor is on an ASCII char (1 byte) and the other is on
/// a CJK character (3 bytes). Algebraic rebase would use the 1-byte width
/// for both, corrupting the 3-byte character.
/// Use separate lines to prevent cursor merging.
#[test]
fn mc_x_mixed_byte_widths() {
    vim_mc("|1ab\n|2\u{4e16}\u{754c}")
        .keys("x")
        .expect_text("|1b\n|2\u{754c}")
        .labeled("x on 1-byte ASCII and 3-byte CJK character")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. dw where last word has no trailing space
// ═══════════════════════════════════════════════════════════════════════════

/// `dw` at end-of-line (no next word on same line). Each cursor's `w`
/// motion target depends on local content. After deleting the last word,
/// the cursor moves back to the last remaining character on the line.
#[test]
fn mc_dw_end_of_line_words() {
    vim_mc("aa |1bb\ncc |2dd")
        .keys("dw")
        .expect_text("aa|1 \ncc|2 ")
        .labeled("dw deletes last word on each line")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. di{ — delete inside braces
// ═══════════════════════════════════════════════════════════════════════════

/// Cursors inside brace groups of different sizes.
#[test]
fn mc_di_brace_different_lengths() {
    vim_mc("{|1ab} {|2cdefg}")
        .keys("di{")
        .expect_text("{|1} {|2}")
        .labeled("di{ on 2-char and 5-char brace contents")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. dt{char} — delete till char (exclusive)
// ═══════════════════════════════════════════════════════════════════════════

/// `dt.` deletes up to but not including '.'. Different distances.
#[test]
fn mc_dt_char_different_distances() {
    vim_mc("|1a.rest |2hijk.end")
        .keys("dt.")
        .expect_text("|1.rest |2.end")
        .labeled("dt. with target at distance 1 and 4")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Combined: delete + cursor position verification
// ═══════════════════════════════════════════════════════════════════════════

/// After `diw`, each cursor should land at the start of the deleted
/// region. Verify both text and cursor count.
#[test]
fn mc_diw_preserves_cursor_count() {
    vim_mc("|1hello |2world end")
        .keys("diw")
        .expect_text("|1 |2 end")
        .expect_cursor_count(2)
        .labeled("diw preserves cursor count and positions")
        .run();
}

/// After `dd`, cursor count should be preserved (unless cursors merge).
#[test]
fn mc_dd_preserves_cursor_count() {
    vim_mc("|1aaa\nbbb\n|2ccc\nddd")
        .keys("dd")
        .expect_text("|1bbb\n|2ddd")
        .expect_cursor_count(2)
        .labeled("dd preserves two cursors on non-adjacent lines")
        .run();
}
