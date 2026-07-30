//! Comprehensive multi-cursor test suite using the vim-test framework.
//!
//! All tests use the fluent builder API (`vim_mc().keys().expect_text().run()`)
//! or `mc_vim_suite!` macros. Direct `TestSession` usage is limited to tests
//! that exercise cursor management APIs (add_cursor, clear, rotate) which
//! are inherently programmatic, not key-driven.

use vim_test::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// 1. CURSOR MANAGEMENT (programmatic — TestSession is correct here)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_setup_two_cursors() {
    let s = TestSession::new_multi("|1hello |2world");
    assert_cursor_count(&s, 2);
    assert_text(&s, "|hello world");
}

#[test]
fn mc_setup_three_cursors() {
    let s = TestSession::new_multi("|1aaa |2bbb |3ccc");
    assert_cursor_count(&s, 3);
}

#[test]
fn mc_add_cursor_programmatic() {
    let mut s = TestSession::new("|hello world");
    s.session_mut().add_cursor(6).unwrap();
    assert_cursor_count(&s, 2);
}

#[test]
fn mc_clear_secondary() {
    let mut s = TestSession::new_multi("|1hello |2world |3test");
    assert_cursor_count(&s, 3);
    s.session_mut().clear_secondary_cursors();
    assert_cursor_count(&s, 1);
}

#[test]
fn mc_rotate_primary() {
    let mut s = TestSession::new_multi("|1aaa |2bbb |3ccc");
    s.session_mut().rotate_primary(true);
    assert_cursor_count(&s, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. CONTENT-DEPENDENT NORMAL MODE (per-cursor re-execution)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_tilde() {
    vim_mc("|1Aa |2Bb")
        .keys("~")
        .expect_text("a|1a b|2b")
        .labeled("tilde toggles case at each cursor")
        .run();
}

#[test]
fn mc_delete_word_different_lengths() {
    vim_mc("|1short |2longer_word end")
        .keys("dw")
        .expect_text("|end")
        .labeled("dw deletes different-length words")
        .run();
}

#[test]
fn mc_change_word() {
    vim_mc("|1hello |2world")
        .keys("cwX<Esc>")
        .expect_text("|X X")
        .expect_cursor_count(2)
        .labeled("cw replaces each word")
        .run();
}

#[test]
fn mc_delete_char_x() {
    vim_mc("|1abcd |2efgh")
        .keys("x")
        .expect_text("|bcd fgh")
        .labeled("x deletes first char at each cursor")
        .run();
}

#[test]
fn mc_delete_char_back_x() {
    vim_mc("a|1bcd e|2fgh")
        .keys("X")
        .expect_text("|bcd fgh")
        .labeled("X deletes backward at each cursor")
        .run();
}

#[test]
fn mc_replace_char() {
    vim_mc("|1abcd |2efgh")
        .keys("rX")
        .expect_text("|Xbcd Xfgh")
        .labeled("r replaces char at each cursor")
        .run();
}

#[test]
fn mc_delete_to_end() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("D")
        .expect_text("|\n")
        .labeled("D deletes to EOL at each cursor")
        .run();
}

#[test]
fn mc_substitute_char() {
    vim_mc("|1abc |2def")
        .keys("sX<Esc>")
        .expect_text("|Xbc Xef")
        .labeled("s substitutes char at each cursor")
        .run();
}

#[test]
fn mc_change_to_end() {
    vim_mc("|1hello world\n|2foo bar")
        .keys("CX<Esc>")
        .expect_text("|X\nX")
        .labeled("C changes to EOL at each cursor")
        .run();
}

#[test]
fn mc_uppercase_inner_word() {
    vim_mc("|1hello |2world")
        .keys("gUiw")
        .expect_text("|HELLO WORLD")
        .labeled("gUiw uppercases each word")
        .run();
}

#[test]
fn mc_lowercase_inner_word() {
    vim_mc("|1HELLO |2WORLD")
        .keys("guiw")
        .expect_text("|hello world")
        .labeled("guiw lowercases each word")
        .run();
}

#[test]
fn mc_delete_line() {
    vim_mc("|1aaa\nbbb\n|2ccc\nddd")
        .keys("dd")
        .expect_text("|bbb\nddd")
        .labeled("dd deletes line at each cursor")
        .run();
}

#[test]
fn mc_delete_inner_word() {
    vim_mc("|1hello |2world end")
        .keys("diw")
        .expect_text("| | end")
        .labeled("diw deletes inner word at each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. POSITION-INDEPENDENT NORMAL MODE (algebraic rebase)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_motion_w() {
    vim_mc("|1hello |2world test")
        .keys("w")
        .expect_cursor_count(2)
        .labeled("w advances both cursors")
        .run();
}

#[test]
fn mc_motion_j() {
    vim_mc("|1aaa\n|2bbb\nccc\nddd")
        .keys("j")
        .expect_cursor_count(2)
        .labeled("j moves both cursors down")
        .run();
}

#[test]
fn mc_motion_dollar() {
    vim_mc("|1hello\n|2world")
        .keys("$")
        .expect_cursor_count(2)
        .labeled("$ moves to EOL at each cursor")
        .run();
}

#[test]
fn mc_insert_entry_i() {
    vim_mc("|1hello |2world")
        .keys("iX<Esc>")
        .expect_text("|Xhello Xworld")
        .expect_cursor_count(2)
        .labeled("i inserts at each cursor")
        .run();
}

#[test]
fn mc_insert_entry_a() {
    vim_mc("|1hello |2world")
        .keys("aX<Esc>")
        .expect_text("h|Xello wXorld")
        .expect_cursor_count(2)
        .labeled("a appends at each cursor")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. INSERT MODE MULTI-CURSOR
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_insert_typing_multiple_chars() {
    vim_mc("|1aaa |2bbb")
        .keys("iXYZ<Esc>")
        .expect_text("XY|Zaaa XYZbbb")
        .expect_cursor_count(2)
        .labeled("typing inserts at all cursors")
        .run();
}

#[test]
fn mc_insert_backspace() {
    vim_mc("a|1bc d|2ef")
        .keys("i<BS><Esc>")
        .expect_text("|bc ef")
        .labeled("backspace deletes at all cursors")
        .run();
}

#[test]
fn mc_insert_then_normal() {
    vim_mc("|1hello |2world")
        .keys("iX<Esc>")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .labeled("Esc returns to normal with all cursors")
        .keys("x")
        .expect_cursor_count(2)
        .labeled("x in normal mode works at all cursors")
        .run();
}

#[test]
fn mc_utf8_insert() {
    vim_mc("|1abc |2def")
        .keys("i日<Esc>")
        .expect_text("|日abc 日def")
        .labeled("UTF-8 insert at all cursors")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. UNDO/REDO WITH MULTI-CURSOR
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_undo_restores_text() {
    vim_mc("|1hello |2world")
        .keys("x")
        .keys("u")
        .expect_text("|hello world")
        .labeled("undo restores text after mc delete")
        .run();
}

#[test]
fn mc_undo_redo_roundtrip() {
    vim_mc("|1hello |2world")
        .keys("dw")
        .keys("u")
        .expect_text("|hello world")
        .labeled("undo after mc dw")
        .keys("<C-r>")
        .labeled("redo after undo")
        .run();
}

#[test]
fn mc_undo_atomic_x() {
    let mut s = TestSession::new_multi("|1hello |2world");
    assert_atomic(&mut s, "x");
}

#[test]
fn mc_undo_atomic_insert() {
    let mut s = TestSession::new_multi("|1hello |2world");
    assert_atomic(&mut s, "iXYZ<Esc>");
}

#[test]
fn mc_undo_round_trip_x() {
    let mut s = TestSession::new_multi("|1hello |2world");
    assert_round_trip(&mut s, "x");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. REGISTER & PASTE
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_yank_creates_multi_entries() {
    vim_mc("|1hello |2world")
        .keys("yiw")
        .expect_register('"', "hello")
        .labeled("yiw populates register")
        .run();
}

#[test]
fn mc_named_register_yank() {
    let s = vim_mc("|1hello |2world").keys("\"ayiw").run_session();
    assert!(
        s.register('a').is_some(),
        "register 'a' should have content"
    );
}

#[test]
fn mc_yank_entry_count() {
    let s = vim_mc("|1short |2longer").keys("yiw").run_session();
    let count = s.session().get_register_entry_count('"');
    assert_eq!(count, 2, "should have 2 register entries");
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_cursors_same_line_dollar() {
    let s = vim_mc("|1hello |2world").keys("$").run_session();
    assert!(s.cursor_count() <= 2, "may merge on same-line $");
}

#[test]
fn mc_utf8_tilde() {
    let s = vim_mc("|1Ä |2ö").keys("~").run_session();
    let text = s.text();
    assert!(
        text.contains('ä') || text.contains('Ö'),
        "tilde toggles UTF-8: {text}"
    );
}

#[test]
fn mc_utf8_delete_char() {
    let s = vim_mc("|1日本 |2世界").keys("x").run_session();
    let text = s.text();
    assert!(!text.starts_with('日'), "x deletes CJK char: {text}");
}

#[test]
fn mc_cursor_at_start_and_end() {
    let s = TestSession::new_multi("|1hello|2");
    assert_cursor_count(&s, 2);
}

#[test]
fn mc_join_consecutive_lines() {
    let s = vim_mc("|1aaa\n|2bbb\nccc").keys("J").run_session();
    let text = s.text();
    assert!(!text.starts_with("aaa\n"), "join should work: {text}");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. COUNT WITH MULTI-CURSOR
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_count_delete_char() {
    let s = vim_mc("|1abcdef |2ghijkl").keys("3x").run_session();
    let text = s.text();
    assert!(text.starts_with("def"), "3x deletes 3 chars: {text}");
}

#[test]
fn mc_count_motion() {
    vim_mc("|1a b c d |2e f g h")
        .keys("2w")
        .expect_cursor_count(2)
        .labeled("2w advances both cursors")
        .run();
}

#[test]
fn mc_insert_with_count() {
    let s = vim_mc("|1aaa |2bbb").keys("2iX<Esc>").run_session();
    let text = s.text();
    assert!(
        text.contains("XXaaa"),
        "2i inserts twice at cursor 1: {text}"
    );
    assert!(
        text.contains("XXbbb"),
        "2i inserts twice at cursor 2: {text}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_dot_repeat_change_word() {
    vim_mc("|1hello |2world")
        .keys("cwX<Esc>")
        .expect_text("|X X")
        .labeled("cw replaces words")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. INTEGRATION SCENARIOS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_ciw_type_replacement_undo_atomic() {
    vim_mc("|1foo bar |2baz qux |3quux end")
        .keys("ciw")
        .expect_mode(Mode::Insert)
        .expect_cursor_count(3)
        .labeled("ciw enters insert, words deleted")
        .keys("REPLACED<Esc>")
        .expect_mode(Mode::Normal)
        .expect_text("REPLACE|D bar REPLACED qux REPLACED end")
        .expect_cursor_count(3)
        .labeled("all three words replaced")
        .keys("u")
        .expect_text("|foo bar baz qux quux end")
        .labeled("single u reverts everything")
        .run();
}

#[test]
fn mc_ctrl_d_ciw_replace_undo() {
    vim("want |world change\nwant world change\nwant world change")
        .select_all_occurrences()
        .expect_cursor_count(3)
        .labeled("3 cursors on all 'world' occurrences")
        .keys("ciw")
        .expect_mode(Mode::Insert)
        .labeled("ciw enters insert mode")
        .keys("REPLACED<Esc>")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(3)
        .labeled("all three worlds replaced")
        .keys("u")
        .expect_text("want |world change\nwant world change\nwant world change")
        .labeled("atomic undo restores all")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 10b. STRUCTURAL UNDO GROUP TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mc_ciw_undo_group_stays_open() {
    let mut s = TestSession::new_multi("|1foo |2bar");
    s.feed("ciw");

    assert!(
        s.has_pending_group(),
        "ciw with multi-cursor must leave the undo group open for INSERT mode"
    );
    assert_eq!(s.mode(), Mode::Insert);

    s.feed("X<Esc>");
    assert_eq!(s.mode(), Mode::Normal);
    assert!(!s.has_pending_group(), "Esc must close the undo group");

    let fallbacks = s.checkpoint_fallback_count();
    s.feed("u");
    assert_eq!(
        s.checkpoint_fallback_count(),
        fallbacks,
        "undo must succeed via changeset path, not checkpoint fallback"
    );
    assert_eq!(
        s.text(),
        "foo bar",
        "single u must revert ciw+typing atomically"
    );
}

#[test]
fn mc_substitute_undo_group_stays_open() {
    let mut s = TestSession::new_multi("|1abc |2def");
    s.feed("s");

    assert!(
        s.has_pending_group(),
        "s with multi-cursor must leave the undo group open for INSERT mode"
    );
    assert_eq!(s.mode(), Mode::Insert);

    s.feed("X<Esc>");
    let fallbacks = s.checkpoint_fallback_count();
    s.feed("u");
    assert_eq!(s.checkpoint_fallback_count(), fallbacks);
    assert_eq!(s.text(), "abc def");
}

#[test]
fn mc_cc_undo_group_stays_open() {
    let mut s = TestSession::new_multi("|1aaa\n|2bbb");
    s.feed("cc");

    assert!(
        s.has_pending_group(),
        "cc with multi-cursor must leave the undo group open"
    );

    s.feed("X<Esc>");
    let fallbacks = s.checkpoint_fallback_count();
    s.feed("u");
    assert_eq!(s.checkpoint_fallback_count(), fallbacks);
    assert_eq!(s.text(), "aaa\nbbb");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. SPEC SUITES (bulk tests via mc_vim_suite!)
// ═══════════════════════════════════════════════════════════════════════════

mc_vim_suite!(mc_operators {
    tilde:          "AaBb",       [0, 2], "~"   => "a|abb";
    x_basic:        "abcd",       [0, 2], "x"   => "|bd";
    three_x:        "abcdef",     [0, 2, 4], "x" => "|bdf";
});

mc_vim_suite!(mc_insert_suite {
    basic:          "aaa\nbbb",   [0, 4], "iX<Esc>" => "|Xaaa\nXbbb";
});

mc_vim_suite!(mc_smoke {
    smoke_x:        "hello",      [0, 3], "x";
    smoke_tilde:    "Hello",      [0, 3], "~";
});
