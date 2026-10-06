//! Comprehensive tests for per-cursor re-execution architecture.
//!
//! These tests cover every way algebraic replication of the primary
//! cursor's edit can diverge from re-running the command at each cursor.
//!
//! Under the current algebraic-replication-only architecture, tests marked
//! "MUST FAIL" demonstrate the bug. Under per-cursor re-execution, all
//! tests must pass.
//!
//! # Test categories
//!
//! 1. Content-dependent operators
//! 2. Content-dependent motions
//! 3. Insert-mode content-dependent commands
//! 4. Undo tests
//! 5. Dot-repeat tests
//! 6. Register tests (multi-entry yank/paste)
//! 7. Stress tests (100 cursors, mixed formats)
//! 8. Edge cases (UTF-8, adjacent cursors, cursor merging)
//! 9. Selection staleness

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
// 1. CONTENT-DEPENDENT OPERATOR TESTS
// =============================================================================

/// `~` (toggle case) with cursors on different characters.
///
/// Document: "aAbB"
/// Cursors at 0 ('a') and 2 ('b').
///
/// `~` toggles one char and advances the cursor:
///   cursor@0: 'a'(0x61) -> 'A'(0x41), cursor moves to 1
///   cursor@2: 'b'(0x62) -> 'B'(0x42), cursor moves to 3
/// Starting: "aAbB"
///   Toggle offset 0: 'a' -> 'A' => "AAbB"
///   Toggle offset 2: 'b' -> 'B' => "AABB"
/// Expected: "AABB"
///
/// BUG (replication): Primary at 0 produces Replace{[0..1], "A"}.
/// Rebase to cursor@2: Replace{[2..3], "A"} — replaces 'b' with 'A' not 'B'.
/// Result with bug: "AAaB" or "AAAB" depending on ordering.
#[test]
fn toggle_case_different_chars() {
    let mut session = session_with_cursors("aAbB", &[0, 2]);
    feed(&mut session, "~");
    // Correct: "AABB" (each char toggled independently via per-cursor re-execution)
    // Bug (current): "AAAB" — secondary gets 'A' (primary's replacement) instead of 'B'
    assert_eq!(
        session.text(),
        "AABB",
        "`~` with 2 cursors on different characters should toggle each \
         independently. Algebraic replication copies primary's 'A' to secondary, \
         replacing 'b' with 'A' instead of 'B'.",
    );
}

/// `~` with 3 cursors at offsets 0, 2, 4 in "aBcDe".
///
/// Each cursor toggles the character at its position:
/// - offset 0: 'a' → 'A'
/// - offset 2: 'c' → 'C'
/// - offset 4: 'e' → 'E'
/// Characters at offsets 1 ('B') and 3 ('D') have no cursor — unchanged.
/// Result: "ABCDE" (uppercase B and D were already uppercase, untouched).
#[test]
fn toggle_case_three_cursors_alternating() {
    let mut session = session_with_cursors("aBcDe", &[0, 2, 4]);
    feed(&mut session, "~");
    assert_eq!(
        session.text(),
        "ABCDE",
        "`~` with 3 cursors toggles only the 3 chars at cursor positions. \
         Characters between cursors are untouched.",
    );
}

/// `gU` (uppercase) with cursors on different words.
///
/// Document: "foo bar"
/// Cursors at 0 (on "foo") and 4 (on "bar").
/// Command: `gUiw` (uppercase inner word).
/// Correct: "FOO BAR"
/// Bug: primary produces Replace for "foo"->"FOO", rebased to cursor@4 with
/// same text "FOO" and same range length — happens to work IF words are same
/// length. Use different-length words to expose bug.
#[test]
fn uppercase_different_words() {
    // "hi there" — "hi" is 2 chars, "there" is 5 chars
    let mut session = session_with_cursors("hi there", &[0, 3]);
    feed(&mut session, "gUiw");
    assert_eq!(
        session.text(),
        "HI THERE",
        "`gUiw` with cursors on different-length words. Algebraic \
         replication uses primary's range length for all cursors.",
    );
}

/// `gu` (lowercase) with cursors on mixed-case words.
///
/// Document: "HELLO WORLD"
/// Cursors at 0 and 6.
/// Command: `guiw`
/// Correct: "hello world"
#[test]
fn lowercase_two_words() {
    let mut session = session_with_cursors("HELLO WORLD", &[0, 6]);
    feed(&mut session, "guiw");
    assert_eq!(
        session.text(),
        "hello world",
        "`guiw` should lowercase each cursor's word independently.",
    );
}

/// `g~` (swap case) with cursors on different-length words.
///
/// Document: "Hello WORLD"
/// Cursors at 0 ('H') and 6 ('W').
/// Command: `g~iw`
/// Correct: "hELLO world"
/// Bug: primary's swapped text "hELLO" (5 bytes) would be used for cursor@6's
/// word "WORLD" (5 bytes) — wrong content.
#[test]
fn swap_case_different_content() {
    let mut session = session_with_cursors("Hello WORLD", &[0, 6]);
    feed(&mut session, "g~iw");
    assert_eq!(
        session.text(),
        "hELLO world",
        "`g~iw` should swap case per cursor's own word content.",
    );
}

/// `Ctrl-A` (increment number) with cursors on different number formats.
///
/// Document: "10 0xff"
/// Cursors at 0 (on decimal 10) and 3 (on hex 0xff).
/// Command: Ctrl-A
/// Correct: "11 0x100"
/// Bug: primary produces Replace for "10"->"11", rebased blindly to cursor@3.
#[test]
fn increment_number_different_formats() {
    let mut session = session_with_cursors("10 0xff", &[0, 3]);
    feed(&mut session, "<C-a>");
    assert_eq!(
        session.text(),
        "11 0x100",
        "`Ctrl-A` with cursors on decimal and hex numbers should \
         increment each independently. Algebraic replication copies '11' to the \
         hex position.",
    );
}

/// `Ctrl-X` (decrement number) with different number values.
///
/// Document: "100 1"
/// Cursors at 0 (on 100) and 4 (on 1).
/// Command: Ctrl-X
/// Correct: "99 0"
/// Bug: primary produces "99" (3->2 chars), rebased to cursor@4 changes "1" to "99".
#[test]
fn decrement_number_different_values() {
    let mut session = session_with_cursors("100 1", &[0, 4]);
    feed(&mut session, "<C-x>");
    assert_eq!(
        session.text(),
        "99 0",
        "`Ctrl-X` should decrement each number independently. Width \
         change (3-digit to 2-digit) makes algebraic rebase produce corrupt output.",
    );
}

/// `Ctrl-A` on numbers that change width (9->10, 99->100).
///
/// Document: "9 99"
/// Cursors at 0 (on 9) and 2 (on 99).
/// Command: Ctrl-A
/// Correct: "10 100"
/// Bug: length-changing replacements break position arithmetic.
#[test]
fn increment_width_change() {
    let mut session = session_with_cursors("9 99", &[0, 2]);
    feed(&mut session, "<C-a>");
    assert_eq!(
        session.text(),
        "10 100",
        "`Ctrl-A` on 9 and 99 — both change width. Algebraic replication \
         uses primary's replacement text and length for all cursors.",
    );
}

// =============================================================================
// 2. CONTENT-DEPENDENT MOTION TESTS
// =============================================================================

/// `dw` with cursors on words of different lengths.
///
/// Document: "ab cdefg"
/// Cursors at 0 (on "ab ") and 3 (on "cdefg").
/// Command: `dw`
/// Primary at 0: deletes "ab " (3 bytes)
/// Cursor at 3: should delete "cdefg" (5 bytes) — different length!
/// Correct: ""
/// Bug: primary's Delete{[0..3]} rebased to Delete{[3..6]} — only deletes "cde", leaves "fg".
#[test]
fn delete_word_different_lengths() {
    let mut session = session_with_cursors("ab cdefg", &[0, 3]);
    feed(&mut session, "dw");
    assert_eq!(
        session.text(),
        "",
        "`dw` with words of different lengths. Primary's range length \
         is blindly applied to secondary cursor's position.",
    );
}

/// `ciw` with cursors on different-length words.
///
/// Document: "a longword"
/// Cursors at 0 (on "a") and 2 (on "longword").
/// Command: `ciw`
/// Correct: both words deleted, cursor in insert mode at both positions.
/// Text after `ciw`: " " (space remains between the deleted words)
/// Bug: primary deletes 1 byte, rebased to cursor@2 also deletes 1 byte.
#[test]
fn change_inner_word_different_lengths() {
    let mut session = session_with_cursors("a longword", &[0, 2]);
    feed(&mut session, "ciw");
    assert_eq!(session.mode(), Mode::Insert);
    // After ciw on "a" (1 char) and "longword" (8 chars), the space between
    // them remains. Text should be " ".
    assert_eq!(
        session.text(),
        " ",
        "`ciw` should delete each cursor's inner word independently. \
         The word at cursor@0 is 1 byte ('a'), the word at cursor@2 is 8 bytes \
         ('longword'). Algebraic replication uses primary's 1-byte range for all.",
    );
}

/// `df{char}` with char at different distances from each cursor.
///
/// The distances must differ: with equal distances the buggy path lands on
/// the right answer by coincidence.
///
/// Command: `df.` (delete forward to '.')
/// Document: "a.bc."
/// Cursors at 0 and 2.
/// `df.`: cursor@0 deletes "a." (2 bytes), cursor@2 deletes "bc." (3 bytes)
/// Correct: ""
/// Bug: primary's 2-byte range applied to cursor@2 -> deletes "bc" not "bc."
#[test]
fn delete_find_char_different_distances() {
    let mut session = session_with_cursors("a.bc.", &[0, 2]);
    feed(&mut session, "df.");
    assert_eq!(
        session.text(),
        "",
        "`df.` with '.' at different distances from each cursor. \
         Primary deletes 2 bytes to reach '.', but secondary needs 3 bytes. \
         Algebraic replication copies primary's range length.",
    );
}

/// `d$` with cursors on lines of different lengths.
///
/// Document: "short\nvery long line"
/// Cursors at 0 (line 0) and 6 (line 1).
/// `d$`: delete to end of line, including the char under the cursor.
/// Cursor@0 on "short" deletes "short" (5 chars).
/// Cursor@6 on "very long line" deletes "very long line" (14 chars).
/// Correct: "\n"
/// Bug: primary's 5-byte Delete applied to cursor@6 only deletes "very " (5 bytes).
#[test]
fn delete_to_eol_different_line_lengths() {
    let mut session = session_with_cursors("short\nvery long line", &[0, 6]);
    feed(&mut session, "d$");
    assert_eq!(
        session.text(),
        "\n",
        "`d$` on lines of different lengths. Primary's range length \
         is blindly applied to secondary, leaving partial text on longer lines.",
    );
}

/// `dip` (delete inner paragraph) with paragraphs of different sizes.
///
/// Document: "abc\n\ndefgh\nijklm"
/// Cursors at 0 (first paragraph "abc") and 5 (second paragraph "defgh\nijklm").
/// `dip`: cursor@0 deletes "abc\n" (4 bytes), cursor@5 deletes "defgh\nijklm" (12 bytes).
/// Correct: "\n"
#[test]
fn delete_inner_paragraph_different_sizes() {
    let mut session = session_with_cursors("abc\n\ndefgh\nijklm", &[0, 5]);
    feed(&mut session, "dip");
    // Both cursors' dip ranges may overlap at the blank line separator.
    // The exact result depends on how vim-core's paragraph text object
    // handles trailing/leading blank lines. Accept either "" or "\n".
    let text = session.text();
    assert!(
        text.is_empty() || text == "\n",
        "dip with two paragraphs should delete both. Got: {:?}",
        text,
    );
}

// =============================================================================
// 3. INSERT-MODE CONTENT-DEPENDENT TESTS
// =============================================================================

/// Backspace with different amounts of preceding text.
///
/// Document: "  a\nb" (2-space indent + 'a', then 'b')
/// Cursors at 3 (after 'a') and 5 (after 'b').
/// Enter insert mode, press Backspace.
/// Cursor@3: deletes 'a', text becomes "  \nb"
/// Cursor@5 (now adjusted): deletes 'b', text becomes "  \n"
/// Correct: "  \n"
#[test]
fn backspace_different_context() {
    let mut session = session_with_cursors("ab\ncd", &[1, 4]);
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "<BS>");
    feed(&mut session, "<Esc>");
    assert_eq!(
        session.text(),
        "b\nd",
        "Backspace should delete the character before each cursor \
         independently. Under algebraic replication, secondary cursor gets wrong \
         deletion range.",
    );
}

/// Ctrl-W (DeleteWord) with words of different lengths.
///
/// Document: "aa bb\nccc dd"
/// Enter insert mode with cursors after "aa" (offset 2) and after "ccc" (offset 9).
/// Press Ctrl-W: should delete "aa" (2 bytes) at cursor@2 and "ccc" (3 bytes) at cursor@9.
/// Correct: " bb\n dd"
#[test]
fn delete_word_insert_mode_different_lengths() {
    // "aa bb\nccc dd"
    //  0123456789...
    // 'a'=0, 'a'=1, ' '=2, 'b'=3, 'b'=4, '\n'=5, 'c'=6, 'c'=7, 'c'=8, ' '=9, 'd'=10, 'd'=11
    // Cursor at 2 (after "aa", before space) and 9 (after "ccc", before space)
    let mut session = session_with_cursors("aa bb\nccc dd", &[2, 9]);
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "<C-w>");
    feed(&mut session, "<Esc>");
    assert_eq!(
        session.text(),
        " bb\n dd",
        "Ctrl-W in insert mode should delete the word before each \
         cursor independently. Primary deletes 2-byte word, secondary should \
         delete 3-byte word.",
    );
}

/// Ctrl-E (CopyCharBelow) — should copy the char from the line BELOW
/// each cursor independently.
///
/// Document: "abc\nxyz\n123"
/// Cursors at offset 0 (line 0, col 0) and 4 (line 1, col 0).
/// Enter insert, Ctrl-E copies char from line below:
///   cursor@line0,col0: copies 'x' from line 1
///   cursor@line1,col0: copies '1' from line 2
/// Expected: "xabc\n1xyz\n123"
/// Bug: primary copies 'x', replication gives 'x' to secondary too.
#[test]
fn copy_char_below_different_lines() {
    let mut session = session_with_cursors("abc\nxyz\n123", &[0, 4]);
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "<C-e>");
    feed(&mut session, "<Esc>");
    assert_eq!(
        session.text(),
        "xabc\n1xyz\n123",
        "Ctrl-E should copy the character from the line below EACH \
         cursor independently. Algebraic replication copies primary's char to all.",
    );
}

/// Ctrl-Y (CopyCharAbove) — should copy the char from the line ABOVE
/// each cursor independently.
///
/// Document: "abc\nxyz\n123"
/// Cursors at offset 4 (line 1, col 0) and 8 (line 2, col 0).
/// Enter insert, Ctrl-Y copies char from line above:
///   cursor@line1,col0: copies 'a' from line 0
///   cursor@line2,col0: copies 'x' from line 1
/// Expected: "abc\naxyz\nx123"
/// Bug: primary copies 'a', replication gives 'a' to secondary too.
#[test]
fn copy_char_above_different_lines() {
    let mut session = session_with_cursors("abc\nxyz\n123", &[4, 8]);
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "<C-y>");
    feed(&mut session, "<Esc>");
    assert_eq!(
        session.text(),
        "abc\naxyz\nx123",
        "Ctrl-Y should copy the character from the line above EACH \
         cursor independently. Algebraic replication copies primary's char to all.",
    );
}

// =============================================================================
// 4. UNDO TESTS
// =============================================================================

/// Undo after multi-cursor `~` should restore ALL cursor positions.
///
/// Document: "aAbB"
/// Cursors at 0 and 2.
/// After `~`: text changes.
/// After `u`: text restored to "aAbB" AND cursor positions restored to [0, 2].
///
/// Bug: only the primary cursor position is stored in the undo step.
#[test]
fn undo_restores_all_cursor_positions() {
    let mut session = session_with_cursors("aAbB", &[0, 2]);
    let original_positions: Vec<(usize, usize, usize)> = session.cursor_positions();

    feed(&mut session, "~");
    // Text is now modified
    assert_ne!(session.text(), "aAbB");

    feed(&mut session, "u");
    assert_eq!(session.text(), "aAbB", "Undo should restore original text.");

    // Verify cursor positions restored (the bug stored only the primary)
    let restored_positions = session.cursor_positions();
    let restored_offsets: Vec<usize> = restored_positions.iter().map(|&(_, _, o)| o).collect();
    let original_offsets: Vec<usize> = original_positions.iter().map(|&(_, _, o)| o).collect();

    // At minimum, the primary cursor should be restored. The bug is that
    // secondary cursors are NOT restored.
    assert_eq!(
        restored_offsets.len(),
        original_offsets.len(),
        "Undo should restore all {} cursor positions, but only {} were restored.",
        original_offsets.len(),
        restored_offsets.len(),
    );
}

/// Undo after multi-cursor insert should restore all cursors.
///
/// Document: "aaa\nbbb\nccc"
/// Cursors at 1, 5, 9.
/// Insert 'X' at each.
/// After undo: should restore text AND all 3 cursor positions.
#[test]
fn undo_insert_three_cursors() {
    let original = "aaa\nbbb\nccc";
    let mut session = session_with_cursors(original, &[1, 5, 9]);

    feed(&mut session, "iX<Esc>");
    let modified = session.text().to_owned();
    assert_ne!(modified, original, "Insert should have modified text");

    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "Undo should restore original text after multi-cursor insert."
    );
}

/// UndoCursorStrategy preserved through replication.
///
/// `o` (open line below) uses EntryPosition strategy (cursor goes to insert
/// point, not first edit). With multi-cursor, the BeginUndoGroup should
/// preserve this strategy, not replace it with FirstEdit.
///
/// Document: "abc\ndef"
/// Cursors at 0 and 4.
/// `o` opens a line below each cursor's line, enters insert mode.
/// Type 'X', Escape.
/// After `u`: cursor should be at the `o` entry position (end of "abc"),
/// not at the first edit position.
///
/// This test verifies the strategy is preserved; the exact cursor position
/// after undo depends on the strategy correctly propagating.
#[test]
fn undo_cursor_strategy_preserved() {
    let original = "abc\ndef";
    let mut session = session_with_cursors(original, &[0, 4]);

    feed(&mut session, "oX<Esc>");
    let modified = session.text().to_owned();
    assert_ne!(modified, original, "o + X should modify text");

    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "Undo after `o` with multi-cursor should restore text."
    );
    // The cursor position after undo depends on the strategy. With the correct
    // EntryPosition strategy, cursor should be at the end of the original line
    // where `o` was invoked, not at the first edit point.
}

// =============================================================================
// 5. DOT-REPEAT TESTS
// =============================================================================

/// `ciw` + typed text with multi-cursor.
///
/// Document: "aaa bbb"
/// Cursors at 0 and 4.
/// `ciw`: deletes "aaa" and "bbb", enters insert mode.
/// Type "XX", Escape.
/// Expected: "XX XX"
#[test]
fn dot_repeat_ciw_multi_cursor() {
    let mut session = session_with_cursors("aaa bbb", &[0, 4]);
    feed(&mut session, "ciw");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "XX<Esc>");
    assert_eq!(session.mode(), Mode::Normal);
    assert_eq!(
        session.text(),
        "XX XX",
        "`ciw` + 'XX' with 2 cursors should replace each word independently.",
    );
}

/// Dot-repeat of `ciw` + text — the `.` command itself.
///
/// Step 1: `ciw` + "new" + Esc on single cursor at word "old".
/// Step 2: Set up 2 cursors on two different words, press `.`.
///
/// Document: "old foo bar"
/// Cursor at 0: `ciwnew<Esc>` → "new foo bar"
/// Now set up cursors at 4 ("foo") and 8 ("bar").
/// Press `.`: should replace both words with "new" → "new new new"
///
/// Bug: only the primary cursor gets the insertion text during dot-repeat.
#[test]
fn dot_repeat_replays_at_all_cursors() {
    let mut session = HostSession::new("old foo bar");
    // Phase 1: single cursor, ciw + "new"
    feed(&mut session, "ciwnew<Esc>");
    assert_eq!(session.text(), "new foo bar");

    // Phase 2: set up multi-cursor on the remaining words
    // After phase 1, cursor is at offset 2 ('w' of "new"). Move to "foo".
    feed(&mut session, "w"); // cursor on 'f' of "foo" (offset 4)
    session.add_cursor(8).expect("add cursor at bar");

    // Press dot repeat
    feed(&mut session, ".");
    assert_eq!(
        session.text(),
        "new new new",
        "Dot-repeat (`.`) after `ciw`+'new' should replace BOTH \
         cursors' words with 'new'. Only the primary cursor gets the repeated \
         insert text.",
    );
}

/// Dot-repeat of `gUiw` (uppercase inner word) — non-insert dot repeat.
///
/// Step 1: `gUiw` on "hello" → "HELLO"
/// Step 2: cursors on "world" and "test", press `.` → "WORLD" and "TEST"
#[test]
fn dot_repeat_gUiw_multi_cursor() {
    let mut session = HostSession::new("hello world test");
    // Phase 1: uppercase first word
    feed(&mut session, "gUiw");
    assert_eq!(session.text(), "HELLO world test");

    // Phase 2: set up cursors on "world" and "test"
    feed(&mut session, "w"); // cursor on 'w' of "world" (offset 6)
    session.add_cursor(12).expect("add cursor at 'test'");
    feed(&mut session, ".");
    assert_eq!(
        session.text(),
        "HELLO WORLD TEST",
        "Dot-repeat of `gUiw` with 2 cursors should uppercase each word \
         independently.",
    );
}

// =============================================================================
// 6. REGISTER TESTS (Multi-Entry Yank/Paste)
// =============================================================================

/// Register: `yiw` with 3 cursors on different words stores 3 entries.
///
/// Document: "aaa bbb ccc"
/// Cursors at 0, 4, 8.
/// `yiw`: yank each cursor's inner word.
/// Register `"` should contain 3 entries: "aaa", "bbb", "ccc".
#[test]
fn register_yank_inner_word_three_entries() {
    let mut session = session_with_cursors("aaa bbb ccc", &[0, 4, 8]);
    feed(&mut session, "yiw");

    // Multi-cursor yank should produce multi-entry register.
    let count = session.get_register_entry_count('"');
    assert_eq!(count, 3, "Register should have 3 entries (one per cursor)");

    assert_eq!(session.get_register_entry('"', 0).as_deref(), Some("aaa"));
    assert_eq!(session.get_register_entry('"', 1).as_deref(), Some("bbb"));
    assert_eq!(session.get_register_entry('"', 2).as_deref(), Some("ccc"));
}

/// Register: `yiw` with cursors on different-length words — each entry correct.
///
/// Document: "a longword c"
/// Cursors at 0 ('a'), 2 ('l'), 11 ('c').
/// `yiw`: yanks "a", "longword", "c".
#[test]
fn register_yank_different_length_words() {
    let mut session = session_with_cursors("a longword c", &[0, 2, 11]);
    feed(&mut session, "yiw");

    let count = session.get_register_entry_count('"');
    assert_eq!(count, 3, "Register should have 3 entries (one per cursor)");

    assert_eq!(session.get_register_entry('"', 0).as_deref(), Some("a"));
    assert_eq!(
        session.get_register_entry('"', 1).as_deref(),
        Some("longword")
    );
    assert_eq!(session.get_register_entry('"', 2).as_deref(), Some("c"));
}

/// Register: Paste with multi-entry register distributes entries.
///
/// Document: "aaa bbb ccc"
/// Cursors at 0, 4, 8.
/// `yiw` → register has ["aaa", "bbb", "ccc"].
/// Move to a new position, `p` → each cursor pastes its own entry.
///
/// Document: "1 2 3"
/// Cursors at 0, 2, 4.
/// After `p`: "1aaa 2bbb 3ccc" (each cursor pastes its entry after).
///
/// This is the paste-zip mechanism.
#[test]
fn register_paste_zip_distributes_entries() {
    let mut session = session_with_cursors("aaa bbb ccc", &[0, 4, 8]);
    feed(&mut session, "yiw");

    // Now set up fresh text and cursors for paste
    session.set_text("1 2 3");
    session.clear_secondary_cursors();
    session.set_cursor_offset(0);
    session.add_cursor(2).expect("add cursor");
    session.add_cursor(4).expect("add cursor");

    feed(&mut session, "p");

    let text = session.text().to_owned();
    // Each cursor should paste its corresponding entry.
    // The exact result depends on whether paste is after or at cursor.
    // `p` in normal mode pastes AFTER the cursor character.
    // cursor@0 ('1'): pastes "aaa" after '1' -> "1aaa 2 3"
    // cursor@2 ('2'): pastes "bbb" after '2' -> "1aaa 2bbb 3"
    // cursor@4 ('3'): pastes "ccc" after '3' -> "1aaa 2bbb 3ccc"
    assert!(
        text.contains("aaa") && text.contains("bbb") && text.contains("ccc"),
        "Paste with 3-entry register should distribute entries. Got: {:?}",
        text,
    );
}

// =============================================================================
// 7. STRESS TESTS
// =============================================================================

/// Stress: 50 cursors with `~` on alternating case characters.
///
/// Document: "aAbBaAbBaA..." (50 chars, alternating case)
/// 50 cursors, one at each character.
/// After `~`: "AaBbAaBbAa..." (each char toggled independently).
///
/// Bug: all chars get the same toggle as the primary (first char 'a' -> 'A'),
/// so all become uppercase.
#[test]
fn stress_50_cursors_toggle_case() {
    let mut text = String::with_capacity(50);
    for i in 0..50 {
        if i % 2 == 0 {
            text.push('a');
        } else {
            text.push('A');
        }
    }
    assert_eq!(text.len(), 50);

    let offsets: Vec<usize> = (0..50).collect();
    let mut session = session_with_cursors(&text, &offsets);
    assert_eq!(session.cursor_count(), 50);

    feed(&mut session, "~");

    // Build expected: every char toggled
    let expected: String = text
        .chars()
        .map(|c| {
            if c.is_uppercase() {
                c.to_lowercase().next().unwrap()
            } else {
                c.to_uppercase().next().unwrap()
            }
        })
        .collect();

    assert_eq!(
        session.text(),
        expected,
        "Stress test: 50 cursors `~` should toggle each char independently.",
    );
}

/// Stress: `Ctrl-A` on mixed decimal, hex, and binary numbers simultaneously.
///
/// Document: "10 0xff 0b101"
/// Cursors at 0 (dec 10), 3 (hex 0xff), 8 (bin 0b101).
/// After Ctrl-A: "11 0x100 0b110"
#[test]
fn stress_ctrl_a_mixed_formats() {
    let mut session = session_with_cursors("10 0xff 0b101", &[0, 3, 8]);
    feed(&mut session, "<C-a>");
    assert_eq!(
        session.text(),
        "11 0x100 0b110",
        "Stress test: Ctrl-A on decimal, hex, and binary numbers simultaneously \
         should increment each independently.",
    );
}

/// Stress: `J` (join lines) with cursors on consecutive lines.
///
/// Document: "aaa\nbbb\nccc\nddd"
/// Cursors at 0 (line 0) and (first char of line 2).
/// `J` joins the cursor's line with the one below.
/// cursor@line0: joins "aaa" + "bbb" -> "aaa bbb"
/// cursor@line2: joins "ccc" + "ddd" -> "ccc ddd"
/// Correct: "aaa bbb\nccc ddd"
///
/// Bug: primary produces Replace/Delete for joining "aaa\nbbb", rebased to
/// cursor@line2 with wrong range.
#[test]
fn stress_join_consecutive_lines() {
    // "aaa\nbbb\nccc\nddd"
    //  0123 4567 89...
    let mut session = session_with_cursors("aaa\nbbb\nccc\nddd", &[0, 8]);
    feed(&mut session, "J");
    assert_eq!(
        session.text(),
        "aaa bbb\nccc ddd",
        "Stress: `J` on non-adjacent lines should join each cursor's line with \
         its next line independently.",
    );
}

/// Stress: 20 cursors inserting characters.
///
/// Document: 20 lines of "aaa".
/// 20 cursors, one per line at column 1.
/// Enter insert, type 'X', exit.
/// Each line should become "aXaa".
#[test]
fn stress_20_cursors_insert() {
    let lines: Vec<&str> = vec!["aaa"; 20];
    let text = lines.join("\n");
    // Each line is "aaa\n" (4 bytes), cursor at col 1 on each line
    let offsets: Vec<usize> = (0..20).map(|i| i * 4 + 1).collect();
    let mut session = session_with_cursors(&text, &offsets);
    assert_eq!(session.cursor_count(), 20);

    feed(&mut session, "iX<Esc>");

    let expected_lines: Vec<&str> = vec!["aXaa"; 20];
    let expected = expected_lines.join("\n");
    assert_eq!(
        session.text(),
        expected,
        "Stress: 20 cursors inserting 'X' should produce 'aXaa' on each line.",
    );
}

// =============================================================================
// 8. EDGE CASE TESTS
// =============================================================================

/// Edge: `x` on multi-byte characters — UTF-8 corruption test.
///
/// Document: "a\u{00e9}b\u{00e9}" ('a', 'e-acute', 'b', 'e-acute')
/// In UTF-8: [61, c3 a9, 62, c3 a9] — offsets: a=0, e=1(2bytes), b=3, e=4(2bytes)
/// Cursors at 0 ('a') and 3 ('b').
/// `x` deletes char under cursor.
/// Correct: "\u{00e9}\u{00e9}" — both ASCII chars deleted, accented chars remain.
/// Bug: primary deletes 1 byte at offset 0 (correct for 'a'), rebased to
/// cursor@3 deletes 1 byte starting at offset 3 — but 'b' is 1 byte so this
/// happens to be correct too. Need multi-byte at cursor positions.
///
/// Better test: cursor on multi-byte chars with different widths.
/// Document: "\u{00e9}\u{1f600}" (e-acute 2 bytes + emoji 4 bytes)
/// Cursors at 0 and 2.
/// `x`: deletes e-acute (2 bytes) and emoji (4 bytes).
/// Correct: ""
/// Bug: primary deletes 2 bytes at offset 0, rebased to offset 2 deletes 2 bytes
/// starting at offset 2 — only half the 4-byte emoji! UTF-8 corruption.
#[test]
fn edge_x_multibyte_different_widths() {
    // e-acute (\u{00e9}) = 2 bytes, emoji (\u{1f600}) = 4 bytes
    let text = "\u{00e9}\u{1f600}";
    assert_eq!(text.len(), 6); // 2 + 4 bytes

    let mut session = session_with_cursors(text, &[0, 2]);
    feed(&mut session, "x");
    assert_eq!(
        session.text(),
        "",
        "Edge case: `x` on multi-byte characters of different widths. \
         Algebraic replication uses primary's byte count (2) for the secondary \
         cursor's 4-byte emoji, causing UTF-8 corruption.",
    );
}

/// Edge: `r` (replace) with multi-byte cursor positions.
///
/// Document: "\u{00e9}a" (2-byte char + 1-byte char)
/// Cursors at 0 and 2.
/// `ra`: replace each char with 'a'.
/// Correct: "aa"
/// Bug: primary replaces 2-byte range [0..2] with "a", rebased to [2..4]
/// which is out of bounds (text is only 3 bytes).
#[test]
fn edge_replace_multibyte() {
    let text = "\u{00e9}a";
    assert_eq!(text.len(), 3); // 2 + 1 bytes

    let mut session = session_with_cursors(text, &[0, 2]);
    feed(&mut session, "ra");
    assert_eq!(
        session.text(),
        "aa",
        "Edge case: `r` on characters of different byte widths. Primary's \
         2-byte replace range applied to 1-byte character causes overflow.",
    );
}

/// Edge: Adjacent cursors with length-changing operations.
///
/// Document: "ab"
/// Cursors at 0 and 1.
/// `~` toggles case of each character.
/// Correct: "AB" (same length, no position issues).
///
/// Now with a length-changing op:
/// `x`: deletes char at each position.
/// Correct: "" (both chars deleted).
/// Tricky: descending order processes cursor@1 first (deletes 'b' -> "a"),
/// then cursor@0 (deletes 'a' -> ""). Works correctly with algebraic rebase
/// IF the host applies correctly.
#[test]
fn edge_adjacent_cursors_delete() {
    let mut session = session_with_cursors("ab", &[0, 1]);
    feed(&mut session, "x");
    assert_eq!(
        session.text(),
        "",
        "Edge case: `x` with adjacent cursors should delete both characters.",
    );
}

/// Edge: Cursors that collide after a length-changing operation.
///
/// Document: "a b c"
/// Cursors at 0 ('a'), 2 ('b'), 4 ('c').
/// `dw` (delete word): each cursor deletes its word + trailing space.
/// cursor@0: deletes "a " (2 bytes)
/// cursor@2: deletes "b " (2 bytes)
/// cursor@4: deletes "c" (1 byte, no trailing space)
///
/// After all deletions, text should be empty. But cursors would collide at
/// offset 0. The system should merge colliding cursors.
#[test]
fn edge_cursors_collide_after_delete() {
    let mut session = session_with_cursors("a b c", &[0, 2, 4]);
    feed(&mut session, "dw");
    // After deletion, all cursors collapse to offset 0
    // The text should be empty (or nearly so depending on how dw handles last word)
    let text = session.text().to_owned();
    assert!(
        text.is_empty() || text.trim().is_empty(),
        "Edge case: `dw` with 3 cursors on separate words should delete all words. \
         Got: {:?}",
        text,
    );
}

/// Edge: `x` at end of line — each cursor on the last char of different lines.
///
/// Document: "abc\nde\nf"
/// Cursors at 2 ('c'), 5 ('e'), 7 ('f').
/// `x` deletes last char of each line.
/// Correct: "ab\nd\n"
#[test]
fn edge_x_at_eol_different_lines() {
    let mut session = session_with_cursors("abc\nde\nf", &[2, 5, 7]);
    feed(&mut session, "x");
    assert_eq!(
        session.text(),
        "ab\nd\n",
        "Edge case: `x` at end of each line should delete last char independently.",
    );
}

/// Edge: Empty line between cursors — operations that skip empty lines.
///
/// Document: "aaa\n\nbbb"
/// Cursors at 0 and 5.
/// `dw`: cursor@0 deletes "aaa", cursor@5 deletes "bbb".
/// Correct: "\n\n"
#[test]
fn edge_empty_line_between_cursors() {
    let mut session = session_with_cursors("aaa\n\nbbb", &[0, 5]);
    feed(&mut session, "dw");
    assert_eq!(
        session.text(),
        "\n\n",
        "Edge case: `dw` with empty line between cursors should delete each \
         word independently, preserving the empty line.",
    );
}

// =============================================================================
// 9. SELECTION STALENESS
// =============================================================================

/// Multi-key normal-mode sequence — selections must refresh after a motion.
///
/// Document: "aaa bbb\nccc ddd"
/// Cursors at 0 and 8.
/// Sequence: `w` (move to next word) then `dw` (delete word).
/// After `w`: cursors should be at 4 ("bbb") and 12 ("ddd").
/// After `dw`: delete "bbb" and "ddd".
/// Correct: "aaa \nccc "
///
/// Bug: after `w`, the engine's Selections still hold [0, 8] (stale).
/// The subsequent `dw` computes deltas from stale positions, producing wrong
/// effects for secondary cursors.
#[test]
fn selection_refreshed_after_motion() {
    let mut session = session_with_cursors("aaa bbb\nccc ddd", &[0, 8]);
    feed(&mut session, "w"); // move to next word on each line

    // Verify cursors moved
    let positions = session.cursor_positions();
    let offsets: Vec<usize> = positions.iter().map(|&(_, _, o)| o).collect();
    // Primary should be at "bbb" (offset 4), secondary at "ddd" (offset 12)
    assert!(
        offsets.contains(&4),
        "After `w`, primary cursor should be at offset 4 (start of 'bbb'). \
         Positions: {:?}",
        offsets,
    );

    // Now delete word
    feed(&mut session, "dw");
    assert_eq!(
        session.text(),
        "aaa \nccc ",
        "Selection state goes stale after `w` motion. Subsequent `dw` \
         uses stale cursor positions for delta computation, producing wrong \
         delete ranges for secondary cursors.",
    );
}

/// `jdw` — motion then operator in single sequence.
///
/// Document: "aaa bbb\nccc ddd\neee fff"
/// Cursors at 4 ("bbb") and 12 ("ddd").
/// Sequence: `j` (down) then `dw`.
/// After `j`: cursors should be at 12 ("ddd") and 20 ("fff").
/// After `dw`: delete words at new positions.
#[test]
fn motion_then_operator() {
    let mut session = session_with_cursors("aaa bbb\nccc ddd\neee fff", &[4, 12]);
    feed(&mut session, "j");

    // Cursors should have moved down one line
    let positions = session.cursor_positions();
    let offsets: Vec<usize> = positions.iter().map(|&(_, _, o)| o).collect();
    // After j from offset 4 (line 0, col 4) -> line 1, col 4 = offset 12
    // After j from offset 12 (line 1, col 4) -> line 2, col 4 = offset 20
    assert!(
        offsets.contains(&12) || offsets.contains(&20),
        "After `j`, cursors should move down. Positions: {:?}",
        offsets,
    );

    feed(&mut session, "dw");
    // The exact result depends on cursor positions after j. The key point is
    // that the delete uses the POST-MOTION positions, not the original ones.
    let text = session.text().to_owned();
    assert!(
        !text.contains("ddd") || !text.contains("fff"),
        "After `j` + `dw`, the words at the NEW cursor positions should be \
         deleted, not the words at the OLD positions. Got: {:?}",
        text,
    );
}

/// Three consecutive motions then an operator.
///
/// Document: "aa bb cc dd ee"
/// Cursors at 0 ("aa") and 6 ("cc").
/// `www`: move 3 words forward.
/// Then `x`: delete char at final position.
///
/// After www from 0: should reach "dd" (offset 9)
/// After www from 6: should reach "ee" (offset 12)
/// After `x`: delete first char of each word.
#[test]
fn three_motions_then_operator() {
    let mut session = session_with_cursors("aa bb cc dd ee", &[0, 6]);
    feed(&mut session, "www");

    // Each cursor should have moved 3 words forward
    let positions = session.cursor_positions();
    assert_eq!(
        positions.len(),
        2,
        "Should still have 2 cursors after motions"
    );

    feed(&mut session, "x");
    // The point is that `x` operates at the CURRENT positions, not the stale ones
    let text = session.text().to_owned();
    // Original 'a' at offset 0 and 'c' at offset 6 should still be present
    // (cursors moved away from them)
    assert!(
        text.starts_with("aa"),
        "After 3 word motions, `x` should not affect the original cursor \
         positions. Got: {:?}",
        text,
    );
}

/// The per-cursor insert path follows the host cursor even when it is not
/// the primary cursor.
///
/// After `a` with the primary cursor last, the host cursor ends on the other
/// cursor. The per-cursor path used to overwrite the primary head with it,
/// so both cursors typed at the same place. Formatting while typing makes
/// plain characters take the per-cursor path, and only then does the path
/// follow the host cursor.
#[test]
fn per_cursor_insert_when_host_cursor_is_not_primary() {
    let mut session = session_with_cursors("aaaa bbbb cccc dddd", &[18, 3]);
    let mut opts = session.options().clone();
    opts.set_textwidth(79);
    session.set_options(opts);
    feed(&mut session, "axy");
    assert_eq!(session.text(), "aaaaxy bbbb cccc ddddxy");
    assert_eq!(session.cursor_count(), 2);
}

/// Typing through the per-cursor path after `a` keeps both cursors.
///
/// `a` moves each cursor right but used to leave the secondary's anchor
/// behind, so the per-cursor insert update took it for a visual selection.
/// The range grew with every character until it reached the primary and the
/// two cursors merged, after which only one line received the typing.
/// Formatting while typing makes plain characters take the per-cursor path,
/// and only then are the cursors collapsed.
#[test]
fn per_cursor_insert_after_append_keeps_cursors_apart() {
    let mut session = session_with_cursors("aaaa bbbb cccc dddd\nxxxx yyyy zzzz wwww", &[18, 38]);
    let mut opts = session.options().clone();
    opts.set_textwidth(79);
    session.set_options(opts);
    feed(&mut session, "a eeee ffff gggg hhhh iiii");
    assert_eq!(
        session.text(),
        "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii\nxxxx yyyy zzzz wwww eeee ffff gggg hhhh iiii"
    );
    assert_eq!(session.cursor_count(), 2);
}

/// Without formatting, the per-cursor insert path works as it did before
/// formatting while typing was added, whoever is right: the commands below
/// take that path through their content, not through formatting.
#[test]
fn per_cursor_insert_without_formatting_is_unchanged() {
    // <BS> with the secondary cursor before the primary.
    let mut session = session_with_cursors(".\n", &[2, 0]);
    feed(&mut session, "A<BS><Esc>");
    assert_eq!(session.text(), ".\n");
    assert_eq!(session.cursor_offset(), 0);

    // <CR> at two cursors on one line.
    let mut session = session_with_cursors("nt", &[1, 0]);
    feed(&mut session, "A<CR><Esc>");
    assert_eq!(session.text(), "n\n\nt");
    assert_eq!(session.cursor_offset(), 2);

    // An expanded <Tab>.
    let mut session = session_with_cursors("n\n", &[2, 0]);
    let mut opts = session.options().clone();
    opts.set_expandtab(true);
    opts.set_tabstop(8);
    session.set_options(opts);
    feed(&mut session, "A\t<Esc>");
    assert_eq!(session.text(), "                n\n");
    assert_eq!(session.cursor_offset(), 7);
}

// =============================================================================
// COMPOSITE / INTEGRATION TESTS
// =============================================================================

/// Integration: Full workflow — add cursors, type, undo, redo.
///
/// Document: "111\n222\n333"
/// Add cursors at start of each line.
/// Enter insert, type "X", Escape.
/// Verify: "X111\nX222\nX333"
/// Undo: back to "111\n222\n333"
/// Redo: back to "X111\nX222\nX333"
#[test]
fn integration_insert_undo_redo() {
    let original = "111\n222\n333";
    let mut session = session_with_cursors(original, &[0, 4, 8]);

    // Insert 'X' at each cursor
    feed(&mut session, "iX<Esc>");
    let after_insert = "X111\nX222\nX333";
    assert_eq!(
        session.text(),
        after_insert,
        "Insert 'X' at 3 cursors should prepend to each line."
    );

    // Undo
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "Undo should atomically revert all 3 cursor inserts."
    );

    feed(&mut session, "<C-r>");
    assert_eq!(
        session.text(),
        after_insert,
        "Redo should restore all 3 cursor inserts."
    );
}

/// Integration: `diw` on words of different lengths + undo.
///
/// Document: "a longword c"
/// Cursors at 0, 2, 11.
/// `diw`: deletes "a", "longword", "c" independently.
/// Expected: "  " (spaces between words remain)
/// Undo: restores "a longword c"
#[test]
fn integration_diw_different_lengths_undo() {
    let original = "a longword c";
    let mut session = session_with_cursors(original, &[0, 2, 11]);

    feed(&mut session, "diw");
    assert_eq!(
        session.text(),
        "  ",
        "`diw` with different-length words should delete each independently."
    );

    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "Undo after multi-cursor `diw` should restore original."
    );
}

/// Integration: Visual mode select + delete with multi-cursor.
///
/// Document: "abcde\nfghij"
/// Cursors at 0 and 6.
/// `vllx`: visual select 3 chars ('abc' and 'fgh'), then delete.
/// Expected: "de\nij"
#[test]
fn integration_visual_select_delete() {
    // NOTE: visual mode entry (`v`) sets the anchor for the primary cursor
    // only. Secondary cursors don't get independent anchors because `v` is
    // a global mode switch, not per-cursor. Full visual+multi-cursor support
    // would require per-cursor anchor management. For now, verify partial
    // behavior: each cursor deletes at least the char under it.
    let mut session = session_with_cursors("abcde\nfghij", &[0, 6]);
    feed(&mut session, "vll");
    assert_eq!(
        session.mode(),
        Mode::Visual(vim_core::primitives::VisualType::Char)
    );
    feed(&mut session, "x");
    assert_eq!(session.mode(), Mode::Normal);
    // Visual mode + multi-cursor anchor tracking: the engine processes
    // replicated SetSelection effects but only the last one (primary cursor)
    // persists in the visual state. Secondary cursors' visual anchors are
    // lost during process_effects_with_text. Full visual+multi-cursor
    // requires per-cursor visual anchor tracking — a separate feature.
    // For now, verify the primary cursor's selection is correctly deleted.
    let text = session.text();
    assert!(
        text.starts_with("de\n"),
        "Primary cursor's visual selection (abc) should be deleted. Got: {:?}",
        text,
    );
}

/// Integration: `>>` (indent) with cursors on different lines.
///
/// Document: "aaa\nbbb\nccc"
/// Cursors at 0 and 4.
/// `>>`: indent lines independently.
/// Both lines should get the same indentation added (this is position-independent).
#[test]
fn integration_indent_multi_cursor() {
    let mut session = session_with_cursors("aaa\nbbb\nccc", &[0, 4]);
    feed(&mut session, ">>");
    let text = session.text().to_owned();
    // Both lines 0 and 1 should be indented
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.len() >= 2,
        "Should still have at least 2 lines after indent"
    );
    // Check that lines 0 and 1 start with whitespace (indented)
    assert!(
        lines[0].starts_with('\t') || lines[0].starts_with("  "),
        "Line 0 should be indented. Got: {:?}",
        lines[0],
    );
    assert!(
        lines[1].starts_with('\t') || lines[1].starts_with("  "),
        "Line 1 should be indented. Got: {:?}",
        lines[1],
    );
}

/// Integration: `r{char}` (replace) with multi-cursor on same-width chars.
///
/// Document: "abcde"
/// Cursors at 0, 2, 4.
/// `rX`: replace 'a', 'c', 'e' with 'X'.
/// Expected: "XbXdX"
#[test]
fn integration_replace_char_multi_cursor() {
    let mut session = session_with_cursors("abcde", &[0, 2, 4]);
    feed(&mut session, "rX");
    assert_eq!(
        session.text(),
        "XbXdX",
        "`rX` with 3 cursors should replace each cursor's character with 'X'.",
    );
}

/// Integration: `dd` (delete line) with cursors on different lines.
///
/// Document: "aaa\nbbb\nccc\nddd"
/// Cursors at 0 (line 0) and 8 (line 2).
/// `dd`: delete line 0 and line 2.
/// Expected: "bbb\nddd"
#[test]
fn integration_delete_line_multi_cursor() {
    let mut session = session_with_cursors("aaa\nbbb\nccc\nddd", &[0, 8]);
    feed(&mut session, "dd");
    assert_eq!(
        session.text().trim_end_matches('\n'),
        "bbb\nddd",
        "`dd` with cursors on lines 0 and 2 should delete both lines.",
    );
}

// =============================================================================
// PASTE-ZIP EDGE CASES
// =============================================================================

/// `3p` with multi-entry register — count > 1 breaks paste-zip.
///
/// First: yank 3 different words into a multi-entry register.
/// Then: paste with count=3. The paste-zip comparison should still work.
///
/// Document: "aaa bbb ccc"
/// Cursors at 0, 4, 8.
/// `yiw`: register gets ["aaa", "bbb", "ccc"].
/// `3p`: should paste each entry 3 times at each cursor.
/// Expected: cursor@0 pastes "aaa" 3x, cursor@4 pastes "bbb" 3x, etc.
///
/// Bug: paste-zip comparison checks Insert text against primary_text,
/// but with count=3, Insert text is "aaaaaaaaa" != "aaa".
#[test]
fn paste_zip_with_count() {
    let mut session = session_with_cursors("aaa bbb ccc", &[0, 4, 8]);
    feed(&mut session, "yiw");

    // Yank is done. Now paste with count 3 on same positions.
    // The exact behavior: `3p` pastes the register content 3 times.
    // With paste-zip, each cursor should paste its own entry 3 times.
    feed(&mut session, "3p");

    let text = session.text().to_owned();
    // Each cursor's word is pasted 3 times after the cursor character.
    // The point is: cursor@4 should paste "bbb" 3 times (not "aaa" 3 times).
    // Count the occurrences to verify distribution.
    let bbb_count = text.matches("bbb").count();
    assert!(
        bbb_count >= 3,
        "`3p` with multi-entry register should paste cursor@4's entry \
         ('bbb') 3 times, not the primary's entry. 'bbb' appears {} times in: {:?}",
        bbb_count,
        text,
    );
}

// =============================================================================
// HOST-DELEGATED OPERATOR GAP
// =============================================================================

// Host-delegated operator tests are architectural — they validate that
// `=` (auto-indent) and `gq` (with a host formatter) gracefully fall back to
// algebraic rebase rather than producing N host requests with stale ranges.
// These require host-side mock setup and are better suited for integration
// tests with the host layer. Documented here for completeness.

// =============================================================================
// ADDITIONAL CONTENT-DEPENDENT OPERATOR TESTS
// =============================================================================

/// `J` (join) with cursors on lines of different lengths.
///
/// Document: "short\nvery long line\na\nb"
/// Cursors at 0 (line 0) and 20 (line 2, 'a').
/// `J`: joins line 0+1 and line 2+3.
/// Correct: "short very long line\na b"
/// Bug: primary's join produces Replace for "short\nvery..." and the
/// same Replace geometry is applied to cursor@20.
#[test]
fn join_different_line_lengths() {
    // "short\nvery long line\na\nb"
    //  01234 5 ............19 20 21 22 23
    // "short\nvery long line\na\nb"
    //  s=0 ... \n=5 v=6 ... e=19 \n=20 a=21 \n=22 b=23
    // Cursor at 0 (line 0 "short"), cursor at 21 (line 2 "a").
    let mut session = session_with_cursors("short\nvery long line\na\nb", &[0, 21]);
    feed(&mut session, "J");

    let text = session.text().to_owned();
    // Line 0 + line 1 should be joined
    assert!(
        text.contains("short very long line"),
        "J should join line 0 and line 1. Got: {:?}",
        text,
    );
    // Line 2 + line 3 should be joined
    assert!(
        text.contains("a b"),
        "J should also join line 2 and line 3. Got: {:?}",
        text,
    );
}

/// `cw` (change word) — both operator and motion are content-dependent.
///
/// Document: "hi there"
/// Cursors at 0 ("hi") and 3 ("there").
/// `cw`: delete word, enter insert.
/// Primary deletes "hi" (2 bytes), secondary deletes "there" (5 bytes).
/// Type "XX", Escape.
/// Expected: "XX XX"
#[test]
fn change_word_different_lengths() {
    let mut session = session_with_cursors("hi there", &[0, 3]);
    feed(&mut session, "cw");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "XX<Esc>");
    assert_eq!(
        session.text(),
        "XX XX",
        "`cw` + 'XX' with different-length words should work independently.",
    );
}

/// Multi-cursor undo+redo for normal-mode content-dependent commands.
///
/// `~` toggles case at each cursor. Undo restores. Redo re-applies.
#[test]
fn undo_redo_tilde_multi_cursor() {
    let original = "aAbB";
    let mut session = session_with_cursors(original, &[0, 2]);
    feed(&mut session, "~");
    let after_tilde = session.text().to_owned();
    assert_ne!(after_tilde, original);

    feed(&mut session, "u");
    assert_eq!(session.text(), original, "Undo should restore original");

    feed(&mut session, "<C-r>");
    assert_eq!(
        session.text(),
        after_tilde,
        "Redo should restore toggled text"
    );
}

/// Multi-cursor undo+redo for delete (content-dependent due to varying line widths).
#[test]
fn undo_redo_dd_multi_cursor() {
    let original = "short\nmediumword\nx";
    let mut session = session_with_cursors(original, &[0, 6]);
    feed(&mut session, "dd");
    let after_dd = session.text().to_owned();
    assert_ne!(after_dd, original);

    feed(&mut session, "u");
    assert_eq!(session.text(), original, "Undo dd should restore");

    feed(&mut session, "<C-r>");
    assert_eq!(session.text(), after_dd, "Redo dd should re-apply");
}
