//! Ideal multi-cursor tests for CASE, REPLACE, and FORMATTING operations.
//!
//! These tests describe how multi-cursor SHOULD work for case toggling,
//! character/line replacement, join, indent/outdent, and formatting.
//! They may FAIL under the current architecture if per-cursor re-execution
//! is incomplete for these command classes.
//!
//! All tests use the vim-test fluent builder API (`vim_mc`, `vim`).
//!
//! Cursor annotation convention:
//!   `|`  = single-cursor annotation (checks primary cursor only)
//!   `|N` = multi-cursor annotation (checks all cursor positions)

#![allow(non_snake_case)]

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. TILDE (~) — swap case of character under each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Each cursor toggles the case of its own character independently.
///
/// Input: "AabB" with cursor 1 on 'A' (offset 0), cursor 2 on 'b' (offset 2).
/// `~` toggles case and advances cursor by one char.
///   cursor 1: 'A' -> 'a', cursor moves to offset 1
///   cursor 2: 'b' -> 'B', cursor moves to offset 3
/// Result: "aaBB" with cursors at offsets 1 and 3.
#[test]
fn mc_tilde_toggles_independently() {
    vim_mc("|1Aa|2bB")
        .keys("~")
        .expect_text("a|1aB|2B")
        .labeled("~ toggles A->a at cursor 1, b->B at cursor 2")
        .run();
}

/// Three cursors on non-adjacent alternating case characters.
///
/// Input: "aXbXcX" with cursors at 0 ('a'), 2 ('b'), 4 ('c').
/// After `~`:
///   offset 0: 'a' -> 'A', cursor -> 1
///   offset 2: 'b' -> 'B', cursor -> 3
///   offset 4: 'c' -> 'C', cursor -> 5
/// Result: "AXBXCX" with cursors at 1, 3, 5 (non-adjacent, no merging).
#[test]
fn mc_tilde_three_cursors_spaced() {
    vim_mc("|1aX|2bX|3cX")
        .keys("~")
        .expect_text("A|1XB|2XC|3X")
        .labeled("~ toggles each char independently: a->A, b->B, c->C")
        .run();
}

/// Tilde on identical characters at all cursors — verifies no aliasing.
///
/// Both cursors are on 'a'. Both should produce 'A'. Under algebraic
/// replication, the primary's 'A' replacement would be correct by
/// coincidence — but per-cursor re-execution must produce the same result.
#[test]
fn mc_tilde_same_char_at_all_cursors() {
    vim_mc("|1aaa |2aaa")
        .keys("~")
        .expect_text("A|1aa A|2aa")
        .labeled("~ on same char 'a' at both cursors toggles each to 'A'")
        .run();
}

/// Tilde on two cursors, each on words of different lengths.
///
/// Input: "Hello WORLD" with cursor 1 on 'H' (offset 0), cursor 2 on 'W' (offset 6).
/// `~` toggles: 'H'->'h', cursor 1 -> 1. 'W'->'w', cursor 2 -> 7.
/// Result: "hello wORLD" with cursors at 1, 7.
///
/// This test exposes the algebraic replication bug: primary toggles 'H'->'h'
/// (replacement text "h"), and replication copies "h" to cursor 2's position,
/// replacing 'W' with 'h' instead of 'w'.
#[test]
fn mc_tilde_different_letters() {
    vim_mc("|1Hello |2WORLD")
        .keys("~")
        .expect_text("h|1ello w|2ORLD")
        .labeled("~ toggles H->h and W->w independently")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. g~iw — swap case of inner word at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on words with different case patterns.
///
/// Cursor 1 on "Hello" (mixed), cursor 2 on "WORLD" (all upper).
/// g~iw swaps each char's case within the inner word at each cursor:
///   "Hello" -> "hELLO"
///   "WORLD" -> "world"
/// Cursor stays at start of the word (offset 0 and offset 6).
#[test]
fn mc_g_tilde_iw_different_case_patterns() {
    vim_mc("|1Hello |2WORLD")
        .keys("g~iw")
        .expect_text("|hELLO world")
        .labeled("g~iw swaps case of each word independently")
        .run();
}

/// g~iw on words of different lengths — verifies per-word range.
///
/// "hi" is 2 chars, "THERE" is 5 chars. Under algebraic replication, the
/// primary's 2-char range would be blindly applied to the secondary, only
/// toggling "TH" instead of "THERE".
#[test]
fn mc_g_tilde_iw_different_lengths() {
    vim_mc("|1hi |2THERE end")
        .keys("g~iw")
        .expect_text("|HI there end")
        .labeled("g~iw handles 2-char and 5-char words independently")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. gUiw — uppercase inner word at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on lowercase words.
#[test]
fn mc_gUiw_two_words() {
    vim_mc("|1hello |2world")
        .keys("gUiw")
        .expect_text("|HELLO WORLD")
        .labeled("gUiw uppercases each word independently")
        .run();
}

/// Mixed case words — gUiw should make all chars uppercase.
#[test]
fn mc_gUiw_mixed_case_words() {
    vim_mc("|1hElLo |2wOrLd")
        .keys("gUiw")
        .expect_text("|HELLO WORLD")
        .labeled("gUiw uppercases mixed-case words")
        .run();
}

/// Three cursors on words of varying lengths.
///
/// "a" (1 char), "bb" (2 chars), "ccc" (3 chars). Per-cursor re-execution
/// must compute the correct inner-word range for each.
#[test]
fn mc_gUiw_three_different_length_words() {
    vim_mc("|1a |2bb |3ccc")
        .keys("gUiw")
        .expect_text("|A BB CCC")
        .labeled("gUiw uppercases three words of lengths 1, 2, 3")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. guiw — lowercase inner word at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on uppercase words on different lines.
///
/// Cursor 1 on "HELLO" (line 0), cursor 2 on "FOO" (line 1).
/// guiw lowercases each cursor's inner word. The words "world" and "bar"
/// (already lowercase, no cursor on them) should remain untouched.
#[test]
fn mc_guiw_lowercases_each_word() {
    vim_mc("|1HELLO world\n|2FOO bar")
        .keys("guiw")
        .expect_text("|hello world\nfoo bar")
        .labeled("guiw lowercases HELLO and FOO independently")
        .run();
}

/// guiw on already-lowercase words — should be a no-op on content.
#[test]
fn mc_guiw_already_lowercase_noop() {
    vim_mc("|1hello |2world")
        .keys("guiw")
        .expect_text("|hello world")
        .labeled("guiw on lowercase words is a no-op on content")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. r{char} — replace character at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Basic replace: each cursor replaces its character with 'X'.
///
/// `r` does not move the cursor — cursors stay at their original offsets.
#[test]
fn mc_replace_char_at_each_cursor() {
    vim_mc("|1abc\n|2def")
        .keys("rX")
        .expect_text("|Xbc\nXef")
        .labeled("rX replaces 'a' and 'd' with 'X'")
        .run();
}

/// Replace with different characters under each cursor — same replacement.
///
/// Cursor 1 on 'x' (letter), cursor 2 on 'y' (letter). Both get replaced
/// with 'Z'. This verifies each cursor replaces its own character.
#[test]
fn mc_replace_char_different_originals() {
    vim_mc("|1x23 |2ybc")
        .keys("rZ")
        .expect_text("|Z23 Zbc")
        .labeled("rZ replaces 'x' and 'y' with 'Z'")
        .run();
}

/// Replace on three adjacent cursors.
///
/// Input: "abc" with cursors at 0, 1, 2. After rZ: "ZZZ".
/// Cursors remain at 0, 1, 2.
#[test]
fn mc_replace_char_three_adjacent() {
    vim_mc("|1a|2b|3c")
        .keys("rZ")
        .expect_text("|1Z|2Z|3Z")
        .labeled("rZ replaces all three adjacent chars with Z")
        .run();
}

/// Replace with newline at each cursor.
///
/// Each cursor's character is replaced by a newline character.
/// After r<CR>, cursor lands on the beginning of the new line.
#[test]
fn mc_replace_char_with_newline() {
    let s = vim_mc("|1a b |2c d").keys("r<CR>").run_session();
    assert_eq!(
        s.text(),
        "\n b \n d",
        "r<CR> should replace 'a' and 'c' with newlines"
    );
    assert_eq!(s.cursor_count(), 2, "should still have 2 cursors");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. R (replace mode) — overtype at each cursor
// ═══════════════════════════════════════════════════════════════════════════

/// Enter replace mode at two cursors, type 'XY', then Escape.
///
/// Each cursor overtypes 2 characters. After Esc, cursor backs up one
/// position (vim convention: Esc in insert/replace moves cursor left by 1).
/// Cursor 1: offset 0, overtypes 'a'->'X', 'b'->'Y', Esc -> offset 1.
/// Cursor 2: offset 5 (line 2 start), overtypes 'e'->'X', 'f'->'Y', Esc -> offset 6.
#[test]
fn mc_replace_mode_overtype_two_chars() {
    vim_mc("|1abcd\n|2efgh")
        .keys("RXY<Esc>")
        .expect_mode(Mode::Normal)
        .expect_text("X|Ycd\nXYgh")
        .labeled("R mode overtypes 2 chars at each cursor")
        .run();
}

/// Replace mode with single character then escape.
#[test]
fn mc_replace_mode_single_char() {
    vim_mc("|1hello |2world")
        .keys("RZ<Esc>")
        .expect_mode(Mode::Normal)
        .expect_text("|Zello Zorld")
        .labeled("R + single char overtypes at each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. J (join lines) — each cursor joins its own line with the next
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on non-adjacent lines.
///
/// Cursor 1 on line 0 ("aaa"), cursor 2 on line 2 ("ccc").
/// `J` joins each cursor's line with the line below:
///   line 0 + line 1: "aaa" + "bbb" -> "aaa bbb"
///   line 2 + line 3: "ccc" + "ddd" -> "ccc ddd"
/// After J, cursor goes to the join point (last char of first line before space).
/// Cursor 1 -> offset 3 (the space in "aaa bbb").
#[test]
fn mc_join_non_adjacent_lines() {
    let s = vim_mc("|1aaa\nbbb\n|2ccc\nddd").keys("J").run_session();
    assert_eq!(
        s.text(),
        "aaa bbb\nccc ddd",
        "J should join line 0+1 and line 2+3 independently"
    );
    assert_eq!(s.cursor_count(), 2, "should still have 2 cursors after J");
}

/// Join where lines have different lengths.
///
/// This exposes the algebraic replication bug: primary's join replacement
/// range (based on "short\n") would be applied to "verylongline\n",
/// producing corrupt output.
#[test]
fn mc_join_different_line_lengths() {
    let s = vim_mc("|1short\na\n|2verylongline\nb")
        .keys("J")
        .run_session();
    assert_eq!(
        s.text(),
        "short a\nverylongline b",
        "J on lines of different lengths should join each independently"
    );
    assert_eq!(s.cursor_count(), 2, "should still have 2 cursors after J");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. >> (indent) — indent each cursor's line
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on non-adjacent lines, shiftwidth=2, expandtab.
///
/// `>>` adds one shiftwidth (2 spaces) of indent to each cursor's line.
/// Lines without a cursor are untouched.
#[test]
fn mc_indent_non_adjacent_lines() {
    vim_mc("|1aaa\nbbb\n|2ccc\nddd")
        .with_option("shiftwidth", OptionValue::Unsigned(2))
        .with_option("expandtab", OptionValue::Bool(true))
        .keys(">>")
        .expect_text("|  aaa\nbbb\n  ccc\nddd")
        .labeled(">> indents lines at cursor 1 and cursor 2 only")
        .run();
}

/// Indent already-indented lines — should add another level.
#[test]
fn mc_indent_stacks_on_existing() {
    vim_mc("|1  aaa\n|2  bbb")
        .with_option("shiftwidth", OptionValue::Unsigned(2))
        .with_option("expandtab", OptionValue::Bool(true))
        .keys(">>")
        .expect_text("|    aaa\n    bbb")
        .labeled(">> adds another indent level to both already-indented lines")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. << (outdent) — outdent each cursor's line
// ═══════════════════════════════════════════════════════════════════════════

/// Two cursors on indented lines, shiftwidth=2, expandtab.
///
/// `<<` removes one shiftwidth (2 spaces) of indent from each cursor's line.
/// Lines without a cursor are untouched.
#[test]
fn mc_outdent_non_adjacent_lines() {
    vim_mc("|1  aaa\nbbb\n|2  ccc\nddd")
        .with_option("shiftwidth", OptionValue::Unsigned(2))
        .with_option("expandtab", OptionValue::Bool(true))
        .keys("<<")
        .expect_text("|aaa\nbbb\nccc\nddd")
        .labeled("<< removes indent from cursor 1 and cursor 2 lines only")
        .run();
}

/// Outdent where one line has indent and the other does not.
///
/// The line with no indent should be a no-op (cannot outdent past column 0).
#[test]
fn mc_outdent_mixed_indent_levels() {
    vim_mc("|1  aaa\n|2bbb")
        .with_option("shiftwidth", OptionValue::Unsigned(2))
        .with_option("expandtab", OptionValue::Bool(true))
        .keys("<<")
        .expect_text("|aaa\nbbb")
        .labeled("<< removes indent from line 1, no-op on line 2")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. gq (format) — format at each cursor
//
// gq is host-delegated in vim-core. Without a host formatter, it falls
// back to internal behavior. These tests verify the command does not
// corrupt text with multiple cursors.
// ═══════════════════════════════════════════════════════════════════════════

/// gqq (format current line) at two cursors — smoke test.
#[test]
fn mc_gqq_smoke_no_corruption() {
    let s = vim_mc("|1hello world\n|2foo bar baz")
        .keys("gqq")
        .run_session();
    let text = s.text();
    assert!(
        text.contains("hello") && text.contains("foo"),
        "gqq must not corrupt text. Got: {text:?}"
    );
}

/// gqj (format current + next line) at two cursors on non-adjacent lines.
#[test]
fn mc_gqj_smoke_no_corruption() {
    let s = vim_mc("|1aaa\nbbb\n|2ccc\nddd").keys("gqj").run_session();
    let text = s.text();
    assert!(
        text.contains("aaa") && text.contains("ccc"),
        "gqj must not corrupt text. Got: {text:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ADDITIONAL: LINEWISE CASE OPERATORS
// ═══════════════════════════════════════════════════════════════════════════

/// gUU (uppercase entire line) at two cursors on separate lines.
#[test]
fn mc_gUU_uppercases_each_line() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("gUU")
        .expect_text("|HELLO WORLD\nFOO BAR")
        .labeled("gUU uppercases entire line at each cursor")
        .run();
}

/// guu (lowercase entire line) at two cursors on separate lines.
#[test]
fn mc_guu_lowercases_each_line() {
    vim_mc("|1HELLO WORLD\n|2FOO BAR")
        .keys("guu")
        .expect_text("|hello world\nfoo bar")
        .labeled("guu lowercases entire line at each cursor")
        .run();
}

/// g~~ (swap case entire line) at two cursors with different case mixes.
///
/// Line 1: "Hello World" -> "hELLO wORLD"
/// Line 2: "FOO bar"     -> "foo BAR"
/// Each line is swapped independently based on its own content.
#[test]
fn mc_g_tilde_tilde_swaps_each_line() {
    vim_mc("|1Hello World\n|2FOO bar")
        .keys("g~~")
        .expect_text("|hELLO wORLD\nfoo BAR")
        .labeled("g~~ swaps case of entire line at each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// UNDO ATOMICITY TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// >> indent + undo: single `u` must restore original text.
#[test]
fn mc_indent_undo_atomic() {
    vim_mc("|1aaa\n|2bbb")
        .with_option("shiftwidth", OptionValue::Unsigned(2))
        .with_option("expandtab", OptionValue::Bool(true))
        .keys(">>")
        .expect_text("|  aaa\n  bbb")
        .labeled(">> indents both lines")
        .keys("u")
        .expect_text("|aaa\nbbb")
        .labeled("u undoes indent atomically")
        .run();
}

/// rX replace + undo: single `u` must restore original characters.
#[test]
fn mc_replace_char_undo_atomic() {
    vim_mc("|1abc\n|2def")
        .keys("rX")
        .expect_text("|Xbc\nXef")
        .labeled("rX replaces at both cursors")
        .keys("u")
        .expect_text("|abc\ndef")
        .labeled("u undoes replace atomically")
        .run();
}

/// gUiw + undo + redo round-trip.
#[test]
fn mc_gUiw_undo_redo_roundtrip() {
    vim_mc("|1hello |2world")
        .keys("gUiw")
        .expect_text("|HELLO WORLD")
        .labeled("gUiw uppercases both words")
        .keys("u")
        .expect_text("|hello world")
        .labeled("u restores original lowercase")
        .keys("<C-r>")
        .expect_text("|HELLO WORLD")
        .labeled("Ctrl-R redoes uppercase")
        .run();
}
