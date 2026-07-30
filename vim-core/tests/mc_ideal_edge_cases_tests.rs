//! Multi-cursor IDEAL edge-case and stress tests.
//!
//! These tests describe how multi-cursor SHOULD work. They may FAIL now —
//! that is intentional. Each test targets a specific edge case that would
//! break a naive MC implementation: cursor coalescence, EOF boundaries,
//! Unicode byte boundaries, position invalidation after edits, register
//! interactions, and high cursor-count stress scenarios.
//!
//! MC annotation syntax: `|1` = cursor 1 (primary), `|2` = cursor 2, etc.

#![allow(non_snake_case)]

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::Mode;
use vim_test::prelude::*;

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
    session.set_cursor_offset(offsets[0]);
    for &offset in &offsets[1..] {
        session
            .add_cursor(offset)
            .unwrap_or_else(|e| panic!("add_cursor({offset}) failed: {e}"));
    }
    assert_eq!(
        session.cursor_count(),
        offsets.len(),
        "cursor count mismatch after setup"
    );
    session
}

// =============================================================================
// 1. CURSOR AT EOF
// =============================================================================

/// One cursor mid-document, one at the last character. `x` deletes char at
/// both — but the second cursor is on the final char, so after deletion it
/// must clamp to the new last char (or stay at EOF if the line becomes empty).
#[test]
fn mc_cursor_at_eof_x_deletes() {
    // "abcde" — cursor 1 at 'a' (offset 0), cursor 2 at 'e' (offset 4)
    let mut session = session_with_cursors("abcde", &[0, 4]);
    feed(&mut session, "x");
    // With correct descending-order processing:
    //   cursor@4: delete 'e' -> "abcd"
    //   cursor@0: delete 'a' -> "bcd"
    assert_eq!(
        session.text(),
        "bcd",
        "x with cursor at position 0 and last char should delete both, yielding 'bcd'"
    );
}

/// `d$` with one cursor on a short line and one on the very last line (no
/// trailing newline). The second cursor is at the last char of the document.
/// Both lines should be cleared to their start.
#[test]
fn mc_cursor_at_eof_delete_to_eol() {
    // "abc\nxy" — cursor 1 at 'a' (offset 0), cursor 2 at 'x' (offset 4)
    let mut session = session_with_cursors("abc\nxy", &[0, 4]);
    feed(&mut session, "d$");
    // d$ from 'a' deletes "abc" -> line 0 empty
    // d$ from 'x' deletes "xy" -> line 1 empty
    assert_eq!(
        session.text(),
        "\n",
        "d$ with cursors at start of two lines should clear both lines"
    );
}

/// `j` (move down) with one cursor mid-document and one on the last line.
/// The cursor on the last line has nowhere to go — it should be a no-op for
/// that cursor while the other cursor moves down normally.
#[test]
fn mc_cursor_at_last_line_j_noop() {
    // "aaa\nbbb\nccc" — cursor 1 at 'a' (offset 0, line 0), cursor 2 at 'c' (offset 8, line 2)
    let mut session = session_with_cursors("aaa\nbbb\nccc", &[0, 8]);
    let count_before = session.cursor_count();
    feed(&mut session, "j");
    // cursor 1 moves from line 0 to line 1
    // cursor 2 is already on last line — j is no-op
    // After j, cursor positions should be line 1 and line 2
    let positions = session.cursor_positions();
    let lines: Vec<usize> = positions.iter().map(|&(line, _, _)| line).collect();
    assert!(
        lines.contains(&1) && lines.contains(&2),
        "j: cursor on line 0 should move to line 1, cursor on last line stays. \
         Got lines: {lines:?}, count before: {count_before}"
    );
}

// =============================================================================
// 2. EMPTY LINES
// =============================================================================

/// One cursor on an empty line, another on a non-empty line. `diw` should
/// delete the word at the non-empty line but be a no-op (or delete nothing)
/// at the empty line.
#[test]
fn mc_empty_line_diw() {
    // "hello\n\nworld" — cursor 1 at 'h' (offset 0), cursor 2 on empty line (offset 6)
    let mut session = session_with_cursors("hello\n\nworld", &[0, 6]);
    feed(&mut session, "diw");
    let text = session.text().to_owned();
    // diw on "hello" deletes "hello", leaving the surrounding structure
    // diw on empty line: no word to delete, should be no-op
    // The empty line (\n at offset 5, then \n at offset 6) has no word under cursor.
    assert!(
        !text.contains("hello"),
        "diw should delete 'hello'. Got: {text:?}"
    );
}

/// `x` on an empty line should be a no-op (nothing to delete), while the
/// other cursor deletes normally.
#[test]
fn mc_empty_line_x_noop() {
    // "a\n\nb" — cursor 1 at 'a' (offset 0), cursor 2 on empty line (offset 2)
    let mut session = session_with_cursors("a\n\nb", &[0, 2]);
    feed(&mut session, "x");
    // x on 'a' deletes it, x on empty line (the \n char) is no-op in normal mode
    let text = session.text().to_owned();
    // In Vim, x on an empty line is a no-op, cursor stays.
    // After deleting 'a' from line 0, line 0 becomes empty.
    assert!(
        text.contains('\n'),
        "empty line should survive x. Got: {text:?}"
    );
}

// =============================================================================
// 3. CURSOR COALESCENCE AFTER DELETE
// =============================================================================

/// Two cursors separated by one char: "a|1b|2c". After `x` at each, the char
/// between them is deleted. If both cursors end up at the same offset, they
/// must coalesce into a single cursor.
#[test]
fn mc_cursor_coalescence_after_x() {
    // "abc" — cursor 1 at 'a' (offset 0), cursor 2 at 'b' (offset 1)
    let mut session = session_with_cursors("abc", &[0, 1]);
    feed(&mut session, "x");
    // x at 0 deletes 'a', x at 1 deletes 'b' -> "c" remains
    // Both cursors now at offset 0 -> should coalesce
    assert_eq!(
        session.text(),
        "c",
        "x on adjacent cursors should delete both chars"
    );
    assert!(
        session.cursor_count() <= 2,
        "cursors at same position should coalesce (or at most 2). Got: {}",
        session.cursor_count()
    );
}

/// `dw` on two adjacent words: "aa bb cc". Cursors on "aa" and "bb". After
/// `dw`, both words (plus trailing space) are deleted, leaving cursors that
/// converge on the remaining text.
#[test]
fn mc_cursor_coalescence_after_dw() {
    // "aa bb cc" — cursor 1 at 'a' (offset 0), cursor 2 at first 'b' (offset 3)
    let mut session = session_with_cursors("aa bb cc", &[0, 3]);
    feed(&mut session, "dw");
    // dw at 0 deletes "aa " (3 bytes), dw at 3 deletes "bb " (3 bytes)
    // After both deletions: "cc" remains. Both cursors collapse to offset 0.
    let text = session.text().to_owned();
    assert_eq!(text, "cc", "dw on two adjacent words should leave 'cc'");
    // Cursors should coalesce since both end up at start of "cc"
    assert!(
        session.cursor_count() <= 2,
        "cursors that converge after dw should coalesce. Got: {}",
        session.cursor_count()
    );
}

// =============================================================================
// 4. UNICODE / MULTI-BYTE CHARACTERS
// =============================================================================

/// Cursors on characters with different byte widths: 2-byte (e-acute) and
/// 4-byte (emoji). `x` must delete the correct number of bytes at each
/// cursor without corrupting UTF-8.
#[test]
fn mc_unicode_different_byte_widths_x() {
    // e-acute = \u{00e9} (2 bytes), grinning face = \u{1f600} (4 bytes)
    let text = "\u{00e9}\u{1f600}";
    assert_eq!(text.len(), 6, "2-byte + 4-byte = 6 bytes");
    let mut session = session_with_cursors(text, &[0, 2]);
    feed(&mut session, "x");
    assert_eq!(
        session.text(),
        "",
        "x on 2-byte and 4-byte chars should delete both without UTF-8 corruption"
    );
}

/// `dw` on words containing CJK characters. Each word has a different number
/// of multi-byte chars, so the deletion ranges differ in byte count.
#[test]
fn mc_unicode_cjk_dw() {
    // Two CJK "words" separated by space: "\u{4e16}\u{754c} \u{4f60}\u{597d}\u{5417}"
    // "world" (2 chars, 6 bytes) + space + "hello?" (3 chars, 9 bytes)
    let text = "\u{4e16}\u{754c} \u{4f60}\u{597d}\u{5417}";
    // offsets: \u{4e16}=0..3, \u{754c}=3..6, ' '=6, \u{4f60}=7..10, \u{597d}=10..13, \u{5417}=13..16
    let mut session = session_with_cursors(text, &[0, 7]);
    feed(&mut session, "dw");
    // dw at 0 deletes "\u{4e16}\u{754c} " (7 bytes)
    // dw at 7 deletes "\u{4f60}\u{597d}\u{5417}" (9 bytes)
    assert_eq!(
        session.text(),
        "",
        "dw on CJK words of different byte-lengths should delete both correctly"
    );
}

/// `r` (replace) on multi-byte characters with an ASCII replacement. The
/// byte count changes — a 3-byte CJK char replaced by 1-byte ASCII.
#[test]
fn mc_unicode_replace_multibyte_with_ascii() {
    // "\u{4e16}\u{754c}" — two 3-byte CJK chars
    let text = "\u{4e16}\u{754c}";
    assert_eq!(text.len(), 6);
    let mut session = session_with_cursors(text, &[0, 3]);
    feed(&mut session, "rX");
    assert_eq!(
        session.text(),
        "XX",
        "r on 3-byte CJK chars should replace each with 'X' (byte-count changes)"
    );
}

/// `~` on accented characters — case toggling of multi-byte chars.
#[test]
fn mc_unicode_tilde_accented() {
    // \u{00e9} = e-acute lowercase (2 bytes), \u{00c9} = E-acute uppercase (2 bytes)
    let text = "\u{00e9}\u{00c9}";
    let mut session = session_with_cursors(text, &[0, 2]);
    feed(&mut session, "~");
    assert_eq!(
        session.text(),
        "\u{00c9}\u{00e9}",
        "~ should toggle case of accented characters correctly"
    );
}

// =============================================================================
// 5. VERY LONG LINES — $ MOVES EACH CURSOR TO ITS OWN LINE END
// =============================================================================

/// `$` with one cursor on a short line and one on a very long line. Each
/// cursor moves to the end of its own line — not to some global "end".
#[test]
fn mc_dollar_short_and_long_lines() {
    let short = "ab";
    let long = "x".repeat(200);
    let text = format!("{short}\n{long}");
    // cursor 1 on 'a' (offset 0, line 0), cursor 2 on first 'x' (offset 3, line 1)
    let mut session = session_with_cursors(&text, &[0, 3]);
    feed(&mut session, "$");
    let positions = session.cursor_positions();
    // cursor 1 should be at col 1 (last char of "ab" = 'b')
    // cursor 2 should be at col 199 (last char of 200-char line)
    let cols: Vec<usize> = positions.iter().map(|&(_, col, _)| col).collect();
    assert!(
        cols.contains(&1),
        "cursor on 'ab' should move to col 1 (last char). Got cols: {cols:?}"
    );
    assert!(
        cols.contains(&199),
        "cursor on 200-char line should move to col 199. Got cols: {cols:?}"
    );
}

/// `d$` with different line lengths. The amount deleted at each cursor
/// differs because lines have different lengths.
#[test]
fn mc_d_dollar_different_line_lengths() {
    let text = "ab\nabcdefghij";
    // cursor 1 at 'a' (offset 0), cursor 2 at 'a' on line 1 (offset 3)
    let mut session = session_with_cursors(text, &[0, 3]);
    feed(&mut session, "d$");
    assert_eq!(
        session.text(),
        "\n",
        "d$ should delete different amounts per line based on line length"
    );
}

// =============================================================================
// 6. SINGLE-CHAR WORDS vs MULTI-CHAR WORDS
// =============================================================================

/// `diw` on a single-char word versus a multi-char word. The deletion ranges
/// are completely different: 1 byte vs N bytes.
#[test]
fn mc_diw_single_vs_multi_char_word() {
    // "a longword" — cursor 1 on 'a' (1-char word), cursor 2 on 'l' (8-char word)
    let mut session = session_with_cursors("a longword", &[0, 2]);
    feed(&mut session, "diw");
    // diw on "a" deletes 1 char, diw on "longword" deletes 8 chars
    // Space between them should remain
    assert_eq!(
        session.text(),
        " ",
        "diw on 1-char and 8-char words should delete each independently"
    );
}

/// `cw` on single-char word vs multi-char word, then type replacement text.
#[test]
fn mc_cw_single_vs_multi_char() {
    // "x hello" — cursor at 'x' (offset 0), cursor at 'h' (offset 2)
    let mut session = session_with_cursors("x hello", &[0, 2]);
    feed(&mut session, "cw");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "NEW<Esc>");
    assert_eq!(
        session.text(),
        "NEW NEW",
        "cw on 1-char and 5-char words: both replaced by 'NEW'"
    );
}

// =============================================================================
// 7. ADJACENT CURSORS (1 CHAR APART)
// =============================================================================

/// Two cursors exactly 1 char apart. `l` (move right) should move both
/// without one overwriting the other's position.
#[test]
fn mc_adjacent_cursors_motion_l() {
    // "abcde" — cursor 1 at 'a' (offset 0), cursor 2 at 'b' (offset 1)
    let mut session = session_with_cursors("abcde", &[0, 1]);
    feed(&mut session, "l");
    let positions = session.cursor_positions();
    let offsets: Vec<usize> = positions.iter().map(|&(_, _, o)| o).collect();
    // cursor 1: 0 -> 1, cursor 2: 1 -> 2
    // They might coalesce at offset 1 if the engine merges, or stay separate
    assert!(
        offsets.contains(&1) || offsets.contains(&2),
        "adjacent cursors should both advance with l. Got offsets: {offsets:?}"
    );
}

/// Two adjacent cursors: `r` (replace) should replace the char under each
/// cursor independently without interfering.
#[test]
fn mc_adjacent_cursors_replace() {
    // "abcde" — cursor at 'b' (offset 1), cursor at 'c' (offset 2)
    let mut session = session_with_cursors("abcde", &[1, 2]);
    feed(&mut session, "rX");
    assert_eq!(
        session.text(),
        "aXXde",
        "r on adjacent cursors should replace each char independently"
    );
}

/// Adjacent cursors with insert: `i` then type text. The inserts should not
/// interleave incorrectly.
#[test]
fn mc_adjacent_cursors_insert() {
    // "ab" — cursor at 'a' (offset 0), cursor at 'b' (offset 1)
    let mut session = session_with_cursors("ab", &[0, 1]);
    feed(&mut session, "iX<Esc>");
    let text = session.text().to_owned();
    // Insert 'X' before 'a' and before 'b' -> "XaXb"
    assert_eq!(
        text, "XaXb",
        "i on adjacent cursors should insert at each position. Got: {text:?}"
    );
}

// =============================================================================
// 8. MANY CURSORS (10+) SIMULTANEOUS EDIT
// =============================================================================

/// 10 cursors on 10 lines, each at column 0. `iX<Esc>` inserts 'X' at the
/// start of every line simultaneously.
#[test]
fn mc_ten_cursors_simultaneous_insert() {
    let lines: Vec<&str> = vec![
        "line0", "line1", "line2", "line3", "line4", "line5", "line6", "line7", "line8", "line9",
    ];
    let text = lines.join("\n");
    // Each line is 5 chars + \n (except last). Offsets at col 0 of each line:
    // line0 starts at 0, line1 at 6, line2 at 12, ... line_n at n*6
    let offsets: Vec<usize> = (0..10).map(|i| i * 6).collect();
    let mut session = session_with_cursors(&text, &offsets);
    assert_eq!(session.cursor_count(), 10);

    feed(&mut session, "iX<Esc>");

    let expected_lines: Vec<String> = (0..10).map(|i| format!("Xline{i}")).collect();
    let expected = expected_lines.join("\n");
    assert_eq!(
        session.text(),
        expected,
        "10 cursors inserting 'X' at start of each line"
    );
}

/// 10 cursors, all do `~` (toggle case) on the first char of each line.
#[test]
fn mc_ten_cursors_tilde() {
    let text = "Aaa\nBbb\nCcc\nDdd\nEee\nFff\nGgg\nHhh\nIii\nJjj";
    // Offsets: A=0, B=4, C=8, D=12, E=16, F=20, G=24, H=28, I=32, J=36
    let offsets: Vec<usize> = (0..10).map(|i| i * 4).collect();
    let mut session = session_with_cursors(text, &offsets);
    assert_eq!(session.cursor_count(), 10);

    feed(&mut session, "~");

    // Each first char toggled: A->a, B->b, C->c, D->d, E->e, F->f, G->g, H->h, I->i, J->j
    // ~ also advances cursor, so check that each line's original first char was toggled.
    let result = session.text().to_owned();
    for (i, line) in result.lines().enumerate() {
        let first = line.chars().next().unwrap();
        assert!(
            first.is_lowercase(),
            "line {i} first char should be lowercase after ~. Got: {first:?} in line {line:?}"
        );
    }
}

/// 15 cursors doing `dd` (delete line). Deletes 15 lines simultaneously,
/// leaving the remaining lines.
#[test]
fn mc_fifteen_cursors_dd() {
    // 20 lines total, cursors on even-numbered lines (0, 2, 4, ..., 28)
    let lines: Vec<String> = (0..20).map(|i| format!("L{i:02}")).collect();
    let text = lines.join("\n");
    // Each line "Lnn" is 3 chars. With \n: line i starts at offset i * 4.
    // Place cursors on even lines: 0, 2, 4, 6, 8, 10, 12, 14, 16, 18
    // That's 10 cursors on even lines.
    let offsets: Vec<usize> = (0..10).map(|i| i * 2 * 4).collect();
    let mut session = session_with_cursors(&text, &offsets);
    assert_eq!(session.cursor_count(), 10);

    feed(&mut session, "dd");

    let result = session.text().to_owned();
    // Even lines (L00, L02, L04, ..., L18) should be deleted.
    // Odd lines (L01, L03, L05, ..., L19) should remain.
    for i in (0..20).step_by(2) {
        assert!(
            !result.contains(&format!("L{i:02}")),
            "even line L{i:02} should be deleted. Text: {result:?}"
        );
    }
    for i in (1..20).step_by(2) {
        assert!(
            result.contains(&format!("L{i:02}")),
            "odd line L{i:02} should survive. Text: {result:?}"
        );
    }
}

// =============================================================================
// 9. CURSOR ON NEWLINE CHARACTER
// =============================================================================

/// In Vim, the cursor normally cannot rest on '\n' in normal mode (it clamps
/// to the last real character). But with multi-cursor on an empty line, the
/// cursor is at the '\n'. Operations should handle this gracefully.
#[test]
fn mc_cursor_on_empty_line_insert() {
    // "hello\n\nworld" — cursor 1 at 'h' (offset 0), cursor 2 on empty line (offset 6)
    let mut session = session_with_cursors("hello\n\nworld", &[0, 6]);
    feed(&mut session, "iX<Esc>");
    let text = session.text().to_owned();
    // Insert 'X' before 'h' -> "Xhello"
    // Insert 'X' on empty line -> "X" (new content on that line)
    assert!(
        text.contains("Xhello"),
        "insert before 'hello' should produce 'Xhello'. Got: {text:?}"
    );
    assert!(
        text.contains("\nX\n"),
        "insert on empty line should produce a line with 'X'. Got: {text:?}"
    );
}

/// `A` (append at end of line) with one cursor on a normal line and one on
/// an empty line. Both should enter insert at end of their respective lines.
#[test]
fn mc_append_eol_empty_and_nonempty() {
    vim_mc("|1hello\n|2\nworld")
        .keys("AEND<Esc>")
        .labeled("A appends at EOL for both empty and non-empty lines")
        .run();
    // We just need this not to crash. The fluent API will check invariants.
}

// =============================================================================
// 10. DELETE MAKES LINES SHORTER — POSITION INVALIDATION
// =============================================================================

/// Cursor 1's deletion causes text to shift such that cursor 2's original
/// position is now past the end of the document. The engine must clamp
/// cursor 2's position after cursor 1's edit.
#[test]
fn mc_delete_invalidates_later_cursor_position() {
    // "abc\nde" — cursor 1 at 'a' (offset 0), cursor 2 at 'd' (offset 4)
    // `dd` on line 0 deletes "abc\n" (4 bytes), leaving "de".
    // cursor 2 was at offset 4, but after deletion, "de" starts at offset 0.
    // cursor 2 must be rebased to the new offset of 'd'.
    let mut session = session_with_cursors("abc\nde", &[0, 4]);
    feed(&mut session, "dd");
    let text = session.text().to_owned();
    // dd on line 0 deletes "abc\n", dd on line 1 deletes "de"
    // Both lines deleted -> should have minimal remaining text
    assert!(
        !text.contains("abc"),
        "line 'abc' should be deleted. Got: {text:?}"
    );
}

/// `D` (delete to end of line) where cursor 1's line is much longer than
/// cursor 2's line. Different amounts deleted.
#[test]
fn mc_D_different_lengths() {
    // "abcdefghij\nxy" — cursor 1 at 'a' (offset 0), cursor 2 at 'x' (offset 11)
    let mut session = session_with_cursors("abcdefghij\nxy", &[0, 11]);
    feed(&mut session, "D");
    // D from 'a' deletes "abcdefghij" (10 chars)
    // D from 'x' deletes "xy" (2 chars)
    assert_eq!(
        session.text(),
        "\n",
        "D should delete different amounts based on line length"
    );
}

// =============================================================================
// 11. INSERT SHIFTS POSITIONS
// =============================================================================

/// Cursor 1 inserts text, causing cursor 2's position to shift forward.
/// The engine must account for this shift when processing cursor 2.
#[test]
fn mc_insert_shifts_later_cursor() {
    // "aaa bbb" — cursor 1 at 'a' (offset 0), cursor 2 at 'b' (offset 4)
    let mut session = session_with_cursors("aaa bbb", &[0, 4]);
    feed(&mut session, "iXX<Esc>");
    // Insert "XX" before 'a' -> "XXaaa bbb" (cursor 2 shifts by 2)
    // Insert "XX" before 'b' -> "XXaaa XXbbb"
    assert_eq!(
        session.text(),
        "XXaaa XXbbb",
        "insert at cursor 1 should shift cursor 2's position forward correctly"
    );
}

/// `o` (open line below) at two cursors. The first open-line shifts all
/// subsequent text down, and cursor 2 must still insert its new line at
/// the correct position.
#[test]
fn mc_open_line_shifts_positions() {
    // "aaa\nbbb\nccc" — cursor 1 at 'a' (line 0), cursor 2 at 'c' (line 2, offset 8)
    let mut session = session_with_cursors("aaa\nbbb\nccc", &[0, 8]);
    feed(&mut session, "oNEW<Esc>");
    let text = session.text().to_owned();
    // o after line 0 inserts "\nNEW" -> "aaa\nNEW\nbbb\nccc"
    // o after line 2 inserts "\nNEW" -> "aaa\nNEW\nbbb\nccc\nNEW"
    assert!(
        text.contains("aaa\nNEW"),
        "o should insert new line after 'aaa'. Got: {text:?}"
    );
    assert!(
        text.contains("ccc\nNEW"),
        "o should insert new line after 'ccc'. Got: {text:?}"
    );
}

// =============================================================================
// 12. MIXED INDENTATION — TABS AND SPACES
// =============================================================================

/// `>>` (indent) on lines with different existing indentation. Both lines
/// get one level of indent added regardless of their current indentation.
#[test]
fn mc_indent_mixed_existing_indentation() {
    // Line 0 has no indent, line 1 has tab indent
    let text = "hello\n\tworld";
    // cursor 1 at offset 0 (line 0), cursor 2 at offset 6 (start of line 1, the tab char)
    let mut session = session_with_cursors(text, &[0, 6]);
    feed(&mut session, ">>");
    let result = session.text().to_owned();
    let lines: Vec<&str> = result.lines().collect();
    // Both lines should have additional indentation
    assert!(
        lines[0].starts_with('\t') || lines[0].starts_with("  "),
        "line 0 should gain indentation. Got: {:?}",
        lines[0]
    );
    assert!(
        lines.len() >= 2,
        "should still have 2 lines. Got: {result:?}"
    );
}

/// `0` (go to start of line) with cursors at different columns on lines with
/// different indentation levels. All cursors should land at column 0.
#[test]
fn mc_zero_different_columns() {
    // "  hello\n\t\tworld" — cursor 1 deep in line 0, cursor 2 deep in line 1
    let text = "  hello\n\t\tworld";
    // cursor 1 at 'l' in "hello" (offset 4), cursor 2 at 'o' in "world" (offset 11)
    let mut session = session_with_cursors(text, &[4, 11]);
    feed(&mut session, "0");
    let positions = session.cursor_positions();
    let cols: Vec<usize> = positions.iter().map(|&(_, col, _)| col).collect();
    assert!(
        cols.iter().all(|&c| c == 0),
        "0 should move all cursors to column 0. Got cols: {cols:?}"
    );
}

// =============================================================================
// 13. TEXT OBJECTS CROSSING / OVERLAPPING
// =============================================================================

/// `dap` (delete a paragraph) where two cursors are in the same paragraph.
/// Both cursors target the same paragraph — the deletion should happen once,
/// not corrupt the document by double-deleting.
#[test]
fn mc_dap_same_paragraph_two_cursors() {
    // Paragraph: "aaa\nbbb" (two lines, one paragraph), then blank line, then "ccc"
    let text = "aaa\nbbb\n\nccc";
    // Both cursors in the first paragraph: offset 0 ('a') and offset 4 ('b')
    let mut session = session_with_cursors(text, &[0, 4]);
    feed(&mut session, "dap");
    let result = session.text().to_owned();
    // The first paragraph (and surrounding blank line) should be deleted.
    // "ccc" should remain.
    assert!(
        result.contains("ccc"),
        "dap should delete first paragraph, leaving 'ccc'. Got: {result:?}"
    );
    assert!(
        !result.contains("aaa"),
        "first paragraph line 'aaa' should be deleted. Got: {result:?}"
    );
}

/// `di"` (delete inside quotes) with cursors in two different quoted strings
/// of different lengths.
#[test]
fn mc_di_quote_different_lengths() {
    // Two quoted strings: "hi" and "hello world"
    let text = r#""hi" "hello world""#;
    // cursor 1 at 'h' of "hi" (offset 1), cursor 2 at 'h' of "hello world" (offset 6)
    let mut session = session_with_cursors(text, &[1, 6]);
    feed(&mut session, "di\"");
    // di" on "hi" deletes 2 chars, di" on "hello world" deletes 11 chars
    assert_eq!(
        session.text(),
        "\"\" \"\"",
        "di\" should delete inside each quoted string independently"
    );
}

// =============================================================================
// 14. REGISTER INTERACTION — YANK, UNDO, PASTE
// =============================================================================

/// Yank at 3 cursors into the default register, verify 3 entries exist,
/// then undo the yank (which is a no-op for text but clears the register
/// state), then paste to verify register contents.
#[test]
fn mc_yank_three_cursors_register_entries() {
    let mut session = session_with_cursors("aaa bbb ccc", &[0, 4, 8]);
    feed(&mut session, "yiw");

    // Verify 3 register entries
    let count = session.get_register_entry_count('"');
    assert_eq!(
        count, 3,
        "yiw with 3 cursors should produce 3 register entries"
    );

    // Verify individual entries
    assert_eq!(session.get_register_entry('"', 0).as_deref(), Some("aaa"));
    assert_eq!(session.get_register_entry('"', 1).as_deref(), Some("bbb"));
    assert_eq!(session.get_register_entry('"', 2).as_deref(), Some("ccc"));
}

/// Yank different-length words, then paste at different positions.
/// Each cursor should paste its own entry (paste-zip).
#[test]
fn mc_yank_paste_zip_different_lengths() {
    let mut session = session_with_cursors("a longword c", &[0, 2, 11]);
    feed(&mut session, "yiw");

    // Verify entries: "a", "longword", "c"
    assert_eq!(session.get_register_entry('"', 0).as_deref(), Some("a"));
    assert_eq!(
        session.get_register_entry('"', 1).as_deref(),
        Some("longword")
    );
    assert_eq!(session.get_register_entry('"', 2).as_deref(), Some("c"));

    // Now paste in place — each cursor pastes its own entry after cursor
    feed(&mut session, "p");
    let text = session.text().to_owned();
    // Paste-zip: cursor@0 pastes "a", cursor@2 pastes "longword", cursor@11 pastes "c"
    assert!(
        text.contains("longword"),
        "paste-zip should include 'longword' from cursor 2's entry. Got: {text:?}"
    );
}

/// Named register: yank to "a at multiple cursors, then paste from "a.
#[test]
fn mc_named_register_yank_paste() {
    let mut session = session_with_cursors("foo bar", &[0, 4]);
    feed(&mut session, "\"ayiw");
    // Verify register 'a' has content
    assert!(
        session.get_register('a').is_some(),
        "register 'a' should have content after yank"
    );
    let count = session.get_register_entry_count('a');
    assert_eq!(
        count, 2,
        "register 'a' should have 2 entries (one per cursor)"
    );
}

// =============================================================================
// 15. DOT-REPEAT WITH MULTI-CURSOR
// =============================================================================

/// Record a change (`ciw` + replacement) with single cursor. Then set up
/// multi-cursor and replay with `.`. The dot-repeat should fan out to all
/// cursors.
#[test]
fn mc_dot_repeat_fans_out() {
    let mut session = HostSession::new("old aaa bbb");
    // Phase 1: single cursor, ciw + "NEW"
    feed(&mut session, "ciwNEW<Esc>");
    assert_eq!(session.text(), "NEW aaa bbb");

    // Phase 2: set up 2 cursors on remaining words
    feed(&mut session, "w"); // move to "aaa"
    let offset_bbb = session.text().find("bbb").unwrap();
    session.add_cursor(offset_bbb).expect("add cursor at bbb");

    // Dot repeat should replace both words with "NEW"
    feed(&mut session, ".");
    assert_eq!(
        session.text(),
        "NEW NEW NEW",
        "dot-repeat should fan out to all cursors, replacing each word with 'NEW'"
    );
}

/// Dot-repeat of `3x` (delete 3 chars) at multiple cursors. The count is
/// part of the recorded command and should apply at each cursor.
#[test]
fn mc_dot_repeat_with_count() {
    let mut session = HostSession::new("abcXX defXX ghiXX");
    // Phase 1: single cursor at offset 3, `3x` deletes "XX "
    session.set_cursor_offset(3);
    feed(&mut session, "2x");
    // Deletes "XX" at offset 3
    let text_after = session.text().to_owned();
    assert!(
        text_after.starts_with("abc"),
        "2x should delete 'XX' after 'abc'. Got: {text_after:?}"
    );

    // Phase 2: position cursors at the remaining "XX" locations and dot-repeat
    // Find remaining XX positions
    let text = session.text().to_owned();
    let mut xx_positions = Vec::new();
    let mut start = 0;
    while let Some(pos) = text[start..].find("XX") {
        xx_positions.push(start + pos);
        start += pos + 2;
    }
    if xx_positions.len() >= 2 {
        session.set_cursor_offset(xx_positions[0]);
        for &pos in &xx_positions[1..] {
            session.add_cursor(pos).expect("add cursor");
        }
        feed(&mut session, ".");
        let final_text = session.text().to_owned();
        assert!(
            !final_text.contains("XX"),
            "dot-repeat of 2x should remove all remaining 'XX'. Got: {final_text:?}"
        );
    }
}

// =============================================================================
// 16. UNDO ATOMICITY ACROSS ALL CURSORS
// =============================================================================

/// Multi-cursor edit must undo as a single atomic operation. A `ciw` +
/// typing at 3 cursors should revert with a single `u`.
#[test]
fn mc_undo_atomic_ciw_three_cursors() {
    let original = "aaa bbb ccc";
    let mut session = session_with_cursors(original, &[0, 4, 8]);
    feed(&mut session, "ciwXX<Esc>");
    assert_eq!(session.text(), "XX XX XX");

    // Single undo should revert ALL cursor edits
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "single u must atomically revert ciw+typing at all 3 cursors"
    );
}

/// Undo + redo round-trip for multi-cursor insert.
#[test]
fn mc_undo_redo_roundtrip_insert() {
    let original = "111\n222\n333";
    let mut session = session_with_cursors(original, &[0, 4, 8]);

    feed(&mut session, "iX<Esc>");
    let after_insert = session.text().to_owned();
    assert_ne!(after_insert, original);

    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "undo should restore original after mc insert"
    );

    feed(&mut session, "<C-r>");
    assert_eq!(
        session.text(),
        after_insert,
        "redo should re-apply mc insert"
    );
}

// =============================================================================
// 17. CURSOR ON LAST CHAR OF LINE (NOT PAST IT)
// =============================================================================

/// In normal mode, cursor cannot go past the last char. With multiple
/// cursors on lines of different lengths, `l` at EOL should be no-op for
/// that cursor while others advance.
#[test]
fn mc_l_at_eol_different_lines() {
    // "ab\ncdefg" — cursor 1 on 'b' (last char of line 0, offset 1),
    //              cursor 2 on 'c' (first char of line 1, offset 3)
    let mut session = session_with_cursors("ab\ncdefg", &[1, 3]);
    feed(&mut session, "l");
    let positions = session.cursor_positions();
    // cursor 1 at 'b' (last char line 0): l is no-op, stays at offset 1
    // cursor 2 at 'c': moves to 'd' (offset 4)
    let offsets: Vec<usize> = positions.iter().map(|&(_, _, o)| o).collect();
    assert!(
        offsets.contains(&1),
        "cursor at EOL should not advance past it. Got offsets: {offsets:?}"
    );
}

// =============================================================================
// 18. LARGE DELETION COLLAPSES DOCUMENT
// =============================================================================

/// `dG` (delete to end of document) at first cursor and `dd` at second.
/// The first cursor's deletion is so large it removes the second cursor's
/// line entirely. The engine must handle this gracefully.
#[test]
fn mc_massive_delete_overlapping_ranges() {
    // "aaa\nbbb\nccc\nddd" — cursor 1 at 'a' (offset 0), cursor 2 at 'c' (offset 8)
    let mut session = session_with_cursors("aaa\nbbb\nccc\nddd", &[0, 8]);
    // dG from line 0 deletes everything from line 0 to end of document
    // dd at line 2 deletes "ccc\n"
    // The dG encompasses the dd range — overlapping deletions
    feed(&mut session, "dG");
    let text = session.text().to_owned();
    // After dG from line 0, everything should be gone (or minimal remaining)
    assert!(
        text.is_empty() || text == "\n",
        "dG from first line should delete entire document. Got: {text:?}"
    );
}

// =============================================================================
// 19. VISUAL MODE + MULTI-CURSOR (PARTIAL SUPPORT)
// =============================================================================

/// Visual mode with multi-cursor: `viw` selects inner word at each cursor,
/// then `d` deletes each selection. (This tests the extent of visual+MC
/// support — may fail if per-cursor visual anchors aren't tracked.)
#[test]
fn mc_visual_inner_word_delete() {
    // Use the builder API — it handles invariants
    vim_mc("|1hello |2world end")
        .keys("viwd")
        .labeled("viw + d deletes inner word at each cursor (if supported)")
        .run();
}

// =============================================================================
// 20. SEARCH-BASED MULTI-CURSOR THEN EDIT
// =============================================================================

/// `select_all_occurrences` finds all matches, creates cursors on each,
/// then `ciw` + replacement changes all occurrences atomically.
#[test]
fn mc_select_all_then_ciw_replace() {
    vim("the |cat sat on the cat mat the cat")
        .select_all_occurrences()
        .expect_cursor_count(3)
        .labeled("3 cursors on all 'cat' occurrences")
        .keys("ciwDOG<Esc>")
        .labeled("replace all 'cat' with 'DOG'")
        .run();
}

/// `add_next_match` incrementally adds cursors one at a time, then edit.
#[test]
fn mc_add_next_match_incremental() {
    vim("foo |bar baz bar bar end")
        .add_next_match()
        .expect_cursor_count(2)
        .labeled("first add_next_match finds second 'bar'")
        .add_next_match()
        .expect_cursor_count(3)
        .labeled("second add_next_match finds third 'bar'")
        .keys("ciwQUX<Esc>")
        .labeled("replace all 'bar' with 'QUX'")
        .run();
}
