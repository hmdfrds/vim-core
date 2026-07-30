//! Fidelity tests: content-dependent motions must produce correct ranges.
//!
//! These tests validate that per-cursor re-execution produces CORRECT Delete
//! ranges when motion-dependent operators are used with multiple cursors on
//! words/regions of different lengths.
//!
//! Under the earlier algebraic replication scheme, the primary cursor's range
//! length was blindly copied to all secondary cursors. That fails whenever the
//! motion result depends on the text under each cursor:
//!
//! - `dw`: word length varies per cursor position
//! - `ciw`: inner-word extent varies per cursor position
//! - `d$`: distance to EOL varies per cursor line
//! - `de`: word-end distance varies per cursor position
//! - `dt{char}`: find-target distance varies per cursor position
//!
//! Per-cursor re-execution fixes this by re-running the full command (operator +
//! motion) in a scratch fork for each cursor, producing independently correct
//! ranges.

#![allow(non_snake_case)]

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::Mode;

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
// Test 1: `dw` on words of different lengths
// =============================================================================

/// `dw` with cursors on "ab" (2-char word) and "cdef" (4-char word).
///
/// Document: "ab cdef gh"
/// Cursors at offset 0 (on "ab") and offset 3 (on "cdef").
///
/// `dw` deletes from cursor to start of next word (inclusive of trailing space):
///   - Cursor at 0: "ab " is the word motion range (3 bytes: 'a','b',' ')
///   - Cursor at 3: "cdef " is the word motion range (5 bytes: 'c','d','e','f',' ')
///
/// After both deletions (applied in correct order): "gh"
///
/// BUG (algebraic replication): Primary produces Delete{0..3} (3 bytes).
/// Rebased to cursor@3: Delete{3..6} — only deletes "cde", leaving "f gh".
/// Result with bug: "f gh" instead of "gh".
#[test]
fn dw_different_word_lengths() {
    let mut session = session_with_cursors("ab cdef gh", &[0, 3]);
    feed(&mut session, "dw");
    assert_eq!(
        session.text(),
        "gh",
        "`dw` with cursors on 'ab' (2 chars) and 'cdef' (4 chars). \
         Each cursor should independently compute its word-motion range. \
         Primary deletes 'ab ' (3 bytes), secondary deletes 'cdef ' (5 bytes). \
         Algebraic replication would copy primary's 3-byte range to secondary.",
    );
}

/// Same as above but with three words of increasing length.
///
/// Document: "a bc def"
/// Cursors at 0, 2, 5.
///   - Cursor@0: `dw` deletes "a " (2 bytes)
///   - Cursor@2: `dw` deletes "bc " (3 bytes)
///   - Cursor@5: `dw` deletes "def" (3 bytes — last word, no trailing space)
///
/// Correct: ""
#[test]
fn dw_three_words_increasing_length() {
    let mut session = session_with_cursors("a bc def", &[0, 2, 5]);
    feed(&mut session, "dw");
    assert_eq!(
        session.text(),
        "",
        "`dw` with 3 cursors on words of length 1, 2, 3. Each cursor's \
         word-motion range is different. All text should be deleted.",
    );
}

/// `dw` where word lengths differ drastically (1 char vs 10 chars).
///
/// Document: "x abcdefghij z"
/// Cursors at 0 (on "x") and 2 (on "abcdefghij").
///   - Cursor@0: `dw` deletes "x " (2 bytes)
///   - Cursor@2: `dw` deletes "abcdefghij " (11 bytes)
///
/// Correct: "z"
#[test]
fn dw_drastic_length_difference() {
    let mut session = session_with_cursors("x abcdefghij z", &[0, 2]);
    feed(&mut session, "dw");
    assert_eq!(
        session.text(),
        "z",
        "`dw` with 1-char word and 10-char word. The range difference is \
         extreme (2 bytes vs 11 bytes). Algebraic replication would leave 9 chars \
         of the long word intact.",
    );
}

// =============================================================================
// Test 2: `ciw` on words of different lengths
// =============================================================================

/// `ciw` with cursors on "hi" (2-char word) and "supercalifragilistic" (20-char word).
///
/// Document: "hi supercalifragilistic bye"
/// Cursors at offset 0 (inside "hi") and offset 3 (inside "supercalifragilistic").
///
/// `ciw` deletes the inner word (no surrounding whitespace) and enters Insert mode:
///   - Cursor@0: deletes "hi" (2 bytes)
///   - Cursor@3: deletes "supercalifragilistic" (20 bytes)
///
/// After both deletions: " " + " bye" = "  bye"
/// (space before "supercalifragilistic" + space before "bye" both survive)
///
/// BUG (algebraic replication): Primary deletes 2 bytes at offset 0.
/// Rebased to cursor@3: deletes 2 bytes at offset 3 — only removes "su",
/// leaving "percalifragilistic bye".
#[test]
fn ciw_different_word_lengths() {
    let mut session = session_with_cursors("hi supercalifragilistic bye", &[0, 3]);
    feed(&mut session, "ciw");

    assert_eq!(
        session.mode(),
        Mode::Insert,
        "ciw should leave the editor in Insert mode",
    );

    // "hi" deleted -> " supercalifragilistic bye"
    // "supercalifragilistic" deleted -> "  bye"
    assert_eq!(
        session.text(),
        "  bye",
        "`ciw` with cursors on 'hi' (2 chars) and 'supercalifragilistic' \
         (20 chars). Each cursor should delete its own inner word independently. \
         Algebraic replication uses primary's 2-byte range for all cursors.",
    );
}

/// `ciw` with three words of different lengths.
///
/// Document: "ab cdefgh ij"
/// Cursors at 0 (on "ab"), 3 (on "cdefgh"), 10 (on "ij").
///
/// `ciw`:
///   - Cursor@0: deletes "ab" (2 bytes)
///   - Cursor@3: deletes "cdefgh" (6 bytes)
///   - Cursor@10: deletes "ij" (2 bytes)
///
/// Correct: " " + " " = "  " (two spaces remain — one between each pair)
#[test]
fn ciw_three_different_words() {
    let mut session = session_with_cursors("ab cdefgh ij", &[0, 3, 10]);
    feed(&mut session, "ciw");

    assert_eq!(session.mode(), Mode::Insert);
    assert_eq!(
        session.text(),
        "  ",
        "`ciw` with 3 cursors on words of length 2, 6, 2. Each inner-word \
         range must be computed independently. The two spaces (separators) survive.",
    );
}

/// `ciw` where cursor is in the middle of a long word.
///
/// Document: "a encyclopedia z"
/// Cursors at 0 (on "a") and 8 (in the middle of "encyclopedia").
///
/// `ciw`:
///   - Cursor@0: deletes "a" (1 byte)
///   - Cursor@8: deletes "encyclopedia" (12 bytes)
///
/// Correct: " " + " z" = "  z"
#[test]
fn ciw_cursor_mid_word() {
    let mut session = session_with_cursors("a encyclopedia z", &[0, 8]);
    feed(&mut session, "ciw");

    assert_eq!(session.mode(), Mode::Insert);
    assert_eq!(
        session.text(),
        "  z",
        "`ciw` with cursor in the middle of 'encyclopedia'. The inner-word \
         extent should encompass the entire word regardless of cursor position \
         within it.",
    );
}

// =============================================================================
// Test 3: `d$` on lines of different lengths
// =============================================================================

/// `d$` with cursors on lines of different lengths.
///
/// Document: "ab\nlonger line\nxy"
/// Cursors at offset 0 (line 0, col 0) and offset 15 (line 2, col 0).
///
/// `d$` deletes from cursor to end of line (inclusive):
///   - Cursor@0 on line "ab": deletes "ab" (2 bytes)
///   - Cursor@15 on line "xy": deletes "xy" (2 bytes)
///
/// Note: line 1 "longer line" is untouched. Lines 0 and 2 become empty.
/// Correct: "\nlonger line\n"
///
/// Offset mapping:
///   "ab\nlonger line\nxy"
///    0123456789...
///   offset 0 = 'a' (line 0)
///   offset 3 = 'l' (line 1)
///   offset 15 = 'x' (line 2)
#[test]
fn d_dollar_different_line_lengths() {
    // Verify our offset math: "ab\nlonger line\nxy"
    let text = "ab\nlonger line\nxy";
    assert_eq!(text.as_bytes()[0], b'a'); // offset 0
    assert_eq!(text.as_bytes()[15], b'x'); // offset 15

    let mut session = session_with_cursors(text, &[0, 15]);
    feed(&mut session, "d$");
    assert_eq!(
        session.text(),
        "\nlonger line\n",
        "`d$` with cursors on line 0 ('ab', 2 chars) and line 2 ('xy', 2 chars). \
         Each cursor should delete to its own line end independently.",
    );
}

/// `d$` where the line length difference is extreme.
///
/// Document: "a\nthis is a much longer line with many words"
/// Cursors at offset 0 (line 0, 1 char) and offset 2 (line 1, 42 chars).
///
/// `d$`:
///   - Cursor@0: deletes "a" (1 byte)
///   - Cursor@2: deletes "this is a much longer line with many words" (42 bytes)
///
/// Correct: "\n"
#[test]
fn d_dollar_extreme_length_difference() {
    let text = "a\nthis is a much longer line with many words";
    assert_eq!(text.as_bytes()[0], b'a');
    assert_eq!(text.as_bytes()[2], b't');

    let mut session = session_with_cursors(text, &[0, 2]);
    feed(&mut session, "d$");
    assert_eq!(
        session.text(),
        "\n",
        "`d$` with 1-char line and 42-char line. Algebraic replication would \
         apply primary's 1-byte range to the 42-char line, leaving 41 chars.",
    );
}

/// `d$` on four lines of wildly different lengths, all cursors at col 0.
///
/// Document: "a\nbcde\nfghijklmn\nop"
/// Cursors at offset 0 (line 0, "a"), 2 (line 1, "bcde"), 7 (line 2, "fghijklmn"), 17 (line 3, "op").
///
/// `d$`:
///   - Cursor@0: deletes "a" (1 byte)
///   - Cursor@2: deletes "bcde" (4 bytes)
///   - Cursor@7: deletes "fghijklmn" (9 bytes)
///   - Cursor@17: deletes "op" (2 bytes)
///
/// Correct: "\n\n\n" (four empty lines)
#[test]
fn d_dollar_four_lines_different_lengths() {
    let text = "a\nbcde\nfghijklmn\nop";
    assert_eq!(text.as_bytes()[0], b'a');
    assert_eq!(text.as_bytes()[2], b'b');
    assert_eq!(text.as_bytes()[7], b'f');
    assert_eq!(text.as_bytes()[17], b'o');

    let mut session = session_with_cursors(text, &[0, 2, 7, 17]);
    feed(&mut session, "d$");
    assert_eq!(
        session.text(),
        "\n\n\n",
        "`d$` with 4 cursors on lines of length 1, 4, 9, 2. Each cursor \
         should delete to its own line end. Algebraic replication uses primary's \
         1-byte range for all cursors, leaving most of the longer lines intact.",
    );
}

/// `d$` with three cursors on three lines of different lengths.
///
/// Document: "ab\ncdefgh\ni"
/// Cursors at offset 0 (line 0), 3 (line 1), 10 (line 2).
///
/// `d$`:
///   - Cursor@0: deletes "ab" (2 bytes)
///   - Cursor@3: deletes "cdefgh" (6 bytes)
///   - Cursor@10: deletes "i" (1 byte)
///
/// Correct: "\n\n" (three empty lines)
#[test]
fn d_dollar_three_lines() {
    let text = "ab\ncdefgh\ni";
    assert_eq!(text.as_bytes()[0], b'a');
    assert_eq!(text.as_bytes()[3], b'c');
    assert_eq!(text.as_bytes()[10], b'i');

    let mut session = session_with_cursors(text, &[0, 3, 10]);
    feed(&mut session, "d$");
    assert_eq!(
        session.text(),
        "\n\n",
        "`d$` with 3 cursors on lines of length 2, 6, 1. Each cursor should \
         delete to its own line end. Algebraic replication uses primary's 2-byte \
         range for all.",
    );
}

// =============================================================================
// Test 4: `de` (delete to end of word) on words of different lengths
// =============================================================================

/// `de` with cursors on words of different lengths.
///
/// Document: "ab cdefg hi"
/// Cursors at 0 (on "ab") and 3 (on "cdefg").
///
/// `de` deletes from cursor to end of current word (inclusive):
///   - Cursor@0: deletes "ab" (2 bytes)
///   - Cursor@3: deletes "cdefg" (5 bytes)
///
/// Correct: " " + " hi" = "  hi"
#[test]
fn de_different_word_lengths() {
    let mut session = session_with_cursors("ab cdefg hi", &[0, 3]);
    feed(&mut session, "de");
    assert_eq!(
        session.text(),
        "  hi",
        "`de` with cursors on 'ab' (2 chars) and 'cdefg' (5 chars). \
         Each should delete to word-end independently. Algebraic replication \
         uses primary's 2-byte range for the 5-byte word.",
    );
}

// =============================================================================
// Test 5: `dt{char}` (delete till char) at different distances
// =============================================================================

/// `dt.` with targets at different distances from each cursor.
///
/// Document: "ab.cdefg."
/// Cursors at 0 (on "a") and 3 (on "c").
///
/// `dt.` deletes from cursor up to (but not including) the next '.':
///   - Cursor@0: deletes "ab" (2 bytes, next '.' at offset 2)
///   - Cursor@3: deletes "cdefg" (5 bytes, next '.' at offset 8)
///
/// Correct: ".."
#[test]
fn dt_char_different_distances() {
    let mut session = session_with_cursors("ab.cdefg.", &[0, 3]);
    feed(&mut session, "dt.");
    assert_eq!(
        session.text(),
        "..",
        "`dt.` with '.' at 2 bytes from cursor@0 but 5 bytes from cursor@3. \
         Each cursor should independently find its next target. Algebraic \
         replication uses primary's 2-byte distance for all.",
    );
}

// =============================================================================
// Test 6: `cw` (change word) on words of different lengths
// =============================================================================

/// `cw` with cursors on words of different lengths, then type replacement text.
///
/// Document: "ab cdefgh ij"
/// Cursors at 0 (on "ab") and 3 (on "cdefgh").
///
/// `cw` deletes the word (not trailing space) and enters insert mode:
///   - Cursor@0: deletes "ab" (2 bytes)
///   - Cursor@3: deletes "cdefgh" (6 bytes)
///
/// Then type "X" at each cursor:
///   - Both cursors insert "X"
///
/// Correct: "X X ij"
#[test]
fn cw_different_lengths_then_insert() {
    let mut session = session_with_cursors("ab cdefgh ij", &[0, 3]);
    feed(&mut session, "cw");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "X<Esc>");
    assert_eq!(
        session.text(),
        "X X ij",
        "`cw` + insert 'X' with cursors on 2-char and 6-char words. Each \
         word should be deleted independently before entering insert mode.",
    );
}

// =============================================================================
// Test 7: `diw` (delete inner word) on words of different lengths
// =============================================================================

/// `diw` with cursors on different-length words.
///
/// Document: "ab cdefg hi"
/// Cursors at 0 (on "ab") and 3 (on "cdefg").
///
/// `diw` deletes the inner word (no surrounding space):
///   - Cursor@0: deletes "ab" (2 bytes)
///   - Cursor@3: deletes "cdefg" (5 bytes)
///
/// Correct: " " + " hi" = "  hi"
#[test]
fn diw_different_word_lengths() {
    let mut session = session_with_cursors("ab cdefg hi", &[0, 3]);
    feed(&mut session, "diw");
    assert_eq!(
        session.text(),
        "  hi",
        "`diw` with cursors on 'ab' (2 chars) and 'cdefg' (5 chars). \
         Each inner-word range must be computed independently per cursor.",
    );
}

// =============================================================================
// Test 8: `daw` (delete a word) including trailing space
// =============================================================================

/// `daw` with cursors on different-length words.
///
/// Document: "ab cdefg hi"
/// Cursors at 0 (on "ab") and 3 (on "cdefg").
///
/// `daw` deletes the word plus surrounding whitespace:
///   - Cursor@0: deletes "ab " (3 bytes — word + trailing space)
///   - Cursor@3: deletes "cdefg " (6 bytes — word + trailing space)
///
/// Correct: "hi"
#[test]
fn daw_different_word_lengths() {
    let mut session = session_with_cursors("ab cdefg hi", &[0, 3]);
    feed(&mut session, "daw");
    assert_eq!(
        session.text(),
        "hi",
        "`daw` with cursors on 'ab' (2 chars) and 'cdefg' (5 chars). \
         The 'a word' text object includes trailing whitespace, making the \
         range even more different between the two cursors.",
    );
}

// =============================================================================
// Test 9: `dW` (delete WORD) on WORDs of different lengths
// =============================================================================

/// `dW` with cursors on WORDs of different lengths.
///
/// Document: "a-b c-d-e-f-g hi"
/// Cursors at 0 (on WORD "a-b") and 4 (on WORD "c-d-e-f-g").
///
/// `dW` deletes from cursor to start of next WORD (inclusive of trailing space):
///   - Cursor@0: deletes "a-b " (4 bytes)
///   - Cursor@4: deletes "c-d-e-f-g " (10 bytes)
///
/// Correct: "hi"
#[test]
fn dW_different_WORD_lengths() {
    let mut session = session_with_cursors("a-b c-d-e-f-g hi", &[0, 4]);
    feed(&mut session, "dW");
    assert_eq!(
        session.text(),
        "hi",
        "`dW` with WORDs of length 3 and 9. The WORD motion range differs \
         per cursor because WORD boundaries depend on the text under each cursor. \
         Algebraic replication uses primary's 4-byte range for the 10-byte WORD.",
    );
}

// =============================================================================
// Test 10: Combined test — verify cursor positions after content-dependent delete
// =============================================================================

/// After `dw` on words of different lengths, verify cursor positions are correct.
///
/// Document: "abc defghij klm"
/// Cursors at 0 (on "abc") and 4 (on "defghij").
///
/// `dw`:
///   - Cursor@0: deletes "abc " (4 bytes)
///   - Cursor@4: deletes "defghij " (8 bytes)
///
/// Correct text: "klm"
/// Both cursors should merge at offset 0 (or be at correct positions).
#[test]
fn dw_verify_final_text() {
    let mut session = session_with_cursors("abc defghij klm", &[0, 4]);
    feed(&mut session, "dw");
    assert_eq!(
        session.text(),
        "klm",
        "`dw` with 3-char and 7-char words. After deletion, only 'klm' \
         should remain. This validates the per-cursor re-execution path produces \
         correct ranges and they are applied without corruption.",
    );
}
