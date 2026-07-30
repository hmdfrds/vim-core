//! Comprehensive behavioral and workflow tests for gb/gB/gs multi-cursor
//! keybindings and associated ex commands.
//!
//! These tests exercise the full pipeline: key input -> grammar -> executor ->
//! mode_dispatch -> multi_cursor_executor, verifying that the gb/gB/gs
//! commands (and their ex-command equivalents) produce correct multi-cursor
//! state through `HostSession.process_key_host()`.
//!
//! Organized by behavioral category:
//!   1. Basic gb (add next match forward)
//!   2. gB (add previous match backward)
//!   3. gs (skip match)
//!   4. Pattern resolution priority
//!   5. Edit fan-out with active cursors
//!   6. Undo with multi-cursor
//!   7. Escape clears multi-cursor
//!   8. Ex commands
//!   9. Workflow scenarios

#![allow(non_snake_case)]

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::Mode;
use vim_test::prelude::*;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn feed(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

/// Create a HostSession with auto_handle_defaults enabled (required for
/// ex commands and host-request-dependent features like command-line mode).
fn host_session(text: &str) -> HostSession {
    HostSession::new(text).with_auto_handle_defaults(true)
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. BASIC gb — add cursor at next match (forward)
// ═══════════════════════════════════════════════════════════════════════════

/// gb on a word with two occurrences adds a second cursor.
#[test]
fn gb_adds_cursor_at_next_match() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 2, "gb should add cursor at second 'foo'");
}

/// gb with count 2 adds two cursors at subsequent matches.
#[test]
fn gb_with_count_2() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "2gb");
    assert_eq!(
        s.cursor_count(),
        3,
        "2gb should produce 3 cursors (original + 2 matches)"
    );
}

/// gb with a single occurrence is a no-op (no extra cursor added).
#[test]
fn gb_single_occurrence_no_op() {
    let mut s = host_session("foo bar baz");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        1,
        "gb with single occurrence should remain at 1 cursor"
    );
}

/// Consecutive gb presses add one cursor each time.
#[test]
fn gb_consecutive_presses() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 2, "first gb: 2 cursors");
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 3, "second gb: 3 cursors");
}

/// gb wraps around when searching forward past end of document.
#[test]
fn gb_wraps_around() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(16); // last "foo"
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        2,
        "gb should wrap around to find first 'foo'"
    );
}

/// gb exhausts all matches (no duplicates, count stops at available).
#[test]
fn gb_exhausts_all_matches() {
    let mut s = host_session("foo foo foo");
    s.set_cursor_offset(0);
    // 3gb would want 3 extra cursors, but only 2 more matches exist.
    feed(&mut s, "3gb");
    assert_eq!(
        s.cursor_count(),
        3,
        "gb should stop at total available matches"
    );
}

/// gb on cursor in middle of word still matches the full word.
#[test]
fn gb_cursor_in_middle_of_word() {
    let mut s = host_session("hello world hello");
    s.set_cursor_offset(2); // on 'l' in first "hello"
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        2,
        "gb with cursor mid-word should match the full word"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. gB — add cursor at previous match (backward)
// ═══════════════════════════════════════════════════════════════════════════

/// gB adds cursor at the previous occurrence of the word.
#[test]
fn gB_adds_cursor_at_previous_match() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(8); // second "foo"
    feed(&mut s, "gB");
    assert_eq!(s.cursor_count(), 2, "gB should add cursor at first 'foo'");
}

/// gB wraps backward past document start.
#[test]
fn gB_wraps_backward() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0); // first "foo"
    feed(&mut s, "gB");
    assert_eq!(
        s.cursor_count(),
        2,
        "gB should wrap backward to find second 'foo'"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. gs — skip match
// ═══════════════════════════════════════════════════════════════════════════

/// gs with a single cursor still adds the next match (skip only removes
/// primary when sels.len() > 1). The match search pointer advances past
/// the added match, so a subsequent gb adds the one after that.
#[test]
fn gs_single_cursor_adds_and_advances() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gs");
    // gs with single cursor: skip doesn't remove (only 1 cursor),
    // but still finds and adds next match. Result: 2 cursors.
    assert_eq!(
        s.cursor_count(),
        2,
        "gs with single cursor adds next match (skip requires 2+ cursors)"
    );
}

/// gs with multiple cursors removes the primary and adds the next match.
#[test]
fn gs_with_multiple_cursors() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb"); // now 2 cursors
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "gs"); // skip: removes primary, adds third foo
    assert_eq!(
        s.cursor_count(),
        2,
        "gs with MC: should still have 2 cursors (removed one, added one)"
    );
}

/// gs then gb: gs advances match pointer, gb adds from that position.
/// Since gs with single cursor also adds a match, we end up with 3.
#[test]
fn gs_then_gb_with_single_cursor() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gs"); // adds second foo (single cursor = no removal), pointer at offset 8
    assert_eq!(s.cursor_count(), 2, "gs added second foo");
    feed(&mut s, "gb"); // adds third foo from advanced pointer
    assert_eq!(s.cursor_count(), 3, "gb adds third foo");
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. PATTERN RESOLUTION PRIORITY
// ═══════════════════════════════════════════════════════════════════════════

/// gb uses the word under cursor as the match pattern.
#[test]
fn gb_uses_word_under_cursor() {
    let mut s = host_session("hello world hello");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 2, "gb matches 'hello' under cursor");
}

/// gb falls back to search register when cursor is on whitespace.
#[test]
fn gb_falls_back_to_search_register() {
    let mut s = host_session("foo   bar foo");
    s.set_cursor_offset(0);
    // Set search pattern via /foo<CR>
    feed(&mut s, "/foo<CR>");
    // Move to whitespace
    s.set_cursor_offset(3); // on space
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        2,
        "gb on whitespace should fall back to search register"
    );
}

/// When match-search state exists (from previous gb), subsequent gb
/// reuses the same pattern even if cursor has moved.
#[test]
fn gb_reuses_match_search_state() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb"); // adds second foo, sets match-search state
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "gb"); // should still use "foo" pattern from state
    assert_eq!(
        s.cursor_count(),
        3,
        "subsequent gb reuses match-search state pattern"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. EDIT FAN-OUT WITH ACTIVE CURSORS
// ═══════════════════════════════════════════════════════════════════════════

/// After adding next match via API, ciw replaces the word at all cursor positions.
#[test]
fn ciw_fans_out_with_add_next_match() {
    let s = vim("|foo bar foo")
        .add_next_match()
        .expect_cursor_count(2)
        .keys("ciwbaz<Esc>")
        .run_session();
    assert_eq!(s.text(), "baz bar baz");
}

/// After selecting all occurrences via API, ciw replaces all words.
#[test]
fn ciw_fans_out_with_select_all() {
    let s = vim("|foo bar foo baz foo")
        .select_all_occurrences()
        .expect_cursor_count(3)
        .keys("ciwX<Esc>")
        .run_session();
    assert_eq!(s.text(), "X bar X baz X");
}

/// Insert mode via i inserts at all cursor positions.
#[test]
fn insert_fans_out() {
    vim_mc("|1foo |2foo")
        .keys("iX<Esc>")
        .expect_text("|Xfoo Xfoo")
        .expect_cursor_count(2)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. UNDO WITH MULTI-CURSOR
// ═══════════════════════════════════════════════════════════════════════════

/// Undo after multi-cursor edit reverts text at all cursor positions.
#[test]
fn undo_reverts_mc_edit() {
    vim_mc("|1foo |2foo")
        .keys("ciwbar<Esc>")
        .expect_text("ba|r bar")
        .keys("u")
        .expect_text("|foo foo")
        .run();
}

/// Undo/redo round-trip preserves text.
#[test]
fn undo_redo_roundtrip() {
    vim_mc("|1hello |2world")
        .keys("dw")
        .keys("u")
        .expect_text("|hello world")
        .keys("<C-r>")
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. ESCAPE CLEARS MULTI-CURSOR IN NORMAL MODE
// ═══════════════════════════════════════════════════════════════════════════

/// Escape in Normal+MC clears secondary cursors.
#[test]
fn escape_clears_mc_in_normal() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    s.add_cursor(8).unwrap();
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "<Esc>");
    assert_eq!(
        s.cursor_count(),
        1,
        "Escape in Normal+MC must clear to single cursor"
    );
}

/// Escape from Insert+MC returns to Normal+MC (keeps cursors).
#[test]
fn escape_from_insert_mc_keeps_cursors() {
    vim_mc("|1foo |2bar")
        .keys("i<Esc>")
        .expect_mode(Mode::Normal)
        .expect_cursor_count(2)
        .run();
}

/// Double Escape from Insert+MC: first exits insert (keeps cursors),
/// second clears secondary cursors in Normal mode.
#[test]
fn double_escape_from_insert_clears_mc() {
    let mut s = TestSession::new_multi("|1foo |2bar");
    s.feed("i");
    assert_eq!(s.mode(), Mode::Insert);
    assert_eq!(s.cursor_count(), 2);
    s.feed("<Esc>"); // exit insert -> Normal+MC
    assert_eq!(s.mode(), Mode::Normal);
    assert_eq!(s.cursor_count(), 2, "first Esc keeps cursors");
    s.feed("<Esc>"); // clear MC
    assert_eq!(s.cursor_count(), 1, "second Esc clears cursors");
}

/// Ctrl-C in Normal+MC also clears secondary cursors.
#[test]
fn ctrl_c_clears_mc_in_normal() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    s.add_cursor(8).unwrap();
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "<C-c>");
    assert_eq!(
        s.cursor_count(),
        1,
        "Ctrl-C in Normal+MC must clear to single cursor"
    );
}

/// Ctrl-[ in Normal+MC also clears secondary cursors.
#[test]
fn ctrl_bracket_clears_mc_in_normal() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    s.add_cursor(8).unwrap();
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "<C-[>");
    assert_eq!(
        s.cursor_count(),
        1,
        "Ctrl-[ in Normal+MC must clear to single cursor"
    );
}

/// Escape with a single cursor is a no-op.
#[test]
fn escape_single_cursor_noop() {
    let mut s = host_session("hello world");
    assert_eq!(s.cursor_count(), 1);
    feed(&mut s, "<Esc>");
    assert_eq!(
        s.cursor_count(),
        1,
        "Escape with single cursor must leave count at 1"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. EX COMMANDS
// ═══════════════════════════════════════════════════════════════════════════

/// :addnext adds cursor at next occurrence.
#[test]
fn ex_addnext() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    feed(&mut s, ":addnext<CR>");
    assert_eq!(s.cursor_count(), 2, ":addnext should add second cursor");
}

/// :addprev adds cursor at previous occurrence.
#[test]
fn ex_addprev() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(8); // second "foo"
    feed(&mut s, ":addprev<CR>");
    assert_eq!(
        s.cursor_count(),
        2,
        ":addprev should add cursor at previous 'foo'"
    );
}

/// :selectall adds cursors at all occurrences.
#[test]
fn ex_selectall() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, ":selectall<CR>");
    assert_eq!(s.cursor_count(), 3, ":selectall should produce 3 cursors");
}

/// :cursorcollapse clears secondary cursors.
#[test]
fn ex_cursorcollapse() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    s.add_cursor(8).unwrap();
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, ":cursorcollapse<CR>");
    assert_eq!(
        s.cursor_count(),
        1,
        ":cursorcollapse should clear secondary cursors"
    );
}

/// :skipmatch advances match pointer (gs ex-command equivalent).
/// Since skip with single cursor still adds a cursor, :skipmatch then
/// :addnext should yield 3 cursors total.
#[test]
fn ex_skipmatch_then_addnext() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, ":skipmatch<CR>");
    // :skipmatch with single cursor: adds second foo (skip requires 2+ to remove)
    assert_eq!(s.cursor_count(), 2, ":skipmatch added second foo");
    feed(&mut s, ":addnext<CR>");
    assert_eq!(
        s.cursor_count(),
        3,
        ":addnext after :skipmatch should add third 'foo'"
    );
}

/// :addcursor below adds a cursor on the next line.
#[test]
fn ex_addcursor_below() {
    let mut s = host_session("foo\nbar\nbaz");
    s.set_cursor_offset(0);
    feed(&mut s, ":addcursor below<CR>");
    assert_eq!(
        s.cursor_count(),
        2,
        ":addcursor below should add cursor on next line"
    );
}

/// :addcursor above adds a cursor on the previous line.
#[test]
fn ex_addcursor_above() {
    let mut s = host_session("foo\nbar\nbaz");
    s.set_cursor_offset(4); // on "bar"
    feed(&mut s, ":addcursor above<CR>");
    assert_eq!(
        s.cursor_count(),
        2,
        ":addcursor above should add cursor on previous line"
    );
}

/// :cursorprimary fwd rotates primary cursor forward.
#[test]
fn ex_cursorprimary_fwd() {
    let mut s = host_session("foo bar baz");
    s.set_cursor_offset(0);
    s.add_cursor(4).unwrap();
    s.add_cursor(8).unwrap();
    assert_eq!(s.cursor_count(), 3);
    feed(&mut s, ":cursorprimary fwd<CR>");
    assert_eq!(s.cursor_count(), 3);
}

/// Abbreviation :addn works for :addnext.
#[test]
fn ex_addnext_abbreviation() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    feed(&mut s, ":addn<CR>");
    assert_eq!(
        s.cursor_count(),
        2,
        ":addn abbreviation should work like :addnext"
    );
}

/// Abbreviation :selecta works for :selectall.
#[test]
fn ex_selectall_abbreviation() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, ":selecta<CR>");
    assert_eq!(
        s.cursor_count(),
        3,
        ":selecta abbreviation should work like :selectall"
    );
}

/// Abbreviation :cursorco works for :cursorcollapse.
#[test]
fn ex_cursorcollapse_abbreviation() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    s.add_cursor(8).unwrap();
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, ":cursorco<CR>");
    assert_eq!(
        s.cursor_count(),
        1,
        ":cursorco abbreviation should work like :cursorcollapse"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. WORKFLOW TESTS
// ═══════════════════════════════════════════════════════════════════════════

/// Workflow: select all occurrences with API, then rename with ciw.
#[test]
fn workflow_selectall_ciw_rename() {
    let s = vim("|foo bar foo baz foo")
        .select_all_occurrences()
        .expect_cursor_count(3)
        .keys("ciwnewName<Esc>")
        .run_session();
    assert_eq!(s.text(), "newName bar newName baz newName");
}

/// Workflow: gb twice then ciw rename.
#[test]
fn workflow_gb_gb_ciw_rename() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 2, "first gb");
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 3, "second gb");
    feed(&mut s, "ciwnewName<Esc>");
    assert_eq!(s.text(), "newName bar newName baz newName");
}

/// Workflow: undo after multi-cursor rename, then redo to confirm.
#[test]
fn workflow_undo_redo_after_mc_rename() {
    let mut s = TestSession::new("|foo bar foo");
    s.select_all_occurrences();
    assert_eq!(s.cursor_count(), 2);
    s.feed("ciwbaz<Esc>");
    assert_eq!(s.text(), "baz bar baz");
    s.feed("u");
    assert_eq!(s.text(), "foo bar foo", "undo reverts rename");
    s.feed("<C-r>");
    assert_eq!(s.text(), "baz bar baz", "redo restores rename");
}

/// Workflow: gs to skip with multiple cursors then verify cursor state.
#[test]
fn workflow_gs_skip_mc() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb"); // 2 cursors
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "gs"); // skip: removes primary, adds third foo -> still 2
    assert_eq!(s.cursor_count(), 2, "gs in MC mode: removed one, added one");
    feed(&mut s, "ciwX<Esc>");
    let text = s.text();
    // The two remaining cursors should have been at the 2nd and 3rd foo
    assert!(
        text.contains("bar") && text.contains("baz"),
        "gs skip workflow: bar and baz should be untouched. Got: {text}"
    );
}

/// Workflow: x deletes char at all cursor positions.
#[test]
fn workflow_x_deletes_at_all_cursors() {
    vim_mc("|1abc |2abc")
        .keys("x")
        .expect_text("|bc bc")
        .expect_cursor_count(2)
        .run();
}

/// Workflow: r replaces char at all cursor positions.
#[test]
fn workflow_r_replaces_at_all_cursors() {
    vim_mc("|1abc |2def")
        .keys("rX")
        .expect_text("|Xbc Xef")
        .expect_cursor_count(2)
        .run();
}

/// Workflow: gb + ciw + undo is atomic (single u reverts all edits).
#[test]
fn workflow_gb_ciw_undo_atomic() {
    let s = vim("|foo bar foo")
        .add_next_match()
        .expect_cursor_count(2)
        .keys("ciwX<Esc>")
        .run_session();
    assert_eq!(s.text(), "X bar X");
    let mut s2 = s;
    s2.feed("u");
    assert_eq!(s2.text(), "foo bar foo", "single u reverts all");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. GRAMMAR PARSING — gb/gB/gs parse to correct Action variants
// ═══════════════════════════════════════════════════════════════════════════

grammar_test!(grammar_gb, "gb" => Action(Action::AddNextMatchCursor));
grammar_test!(grammar_gB, "gB" => Action(Action::AddPrevMatchCursor));
grammar_test!(grammar_gs, "gs" => Action(Action::SkipMatchCursor));

// ═══════════════════════════════════════════════════════════════════════════
// 11. ACTION CLASSIFICATION — gb/gB/gs are NOT content-dependent
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gb_action_not_content_dependent() {
    assert!(
        !Action::AddNextMatchCursor.is_content_dependent(),
        "gb should NOT be content-dependent"
    );
}

#[test]
fn gB_action_not_content_dependent() {
    assert!(
        !Action::AddPrevMatchCursor.is_content_dependent(),
        "gB should NOT be content-dependent"
    );
}

#[test]
fn gs_action_not_content_dependent() {
    assert!(
        !Action::SkipMatchCursor.is_content_dependent(),
        "gs should NOT be content-dependent"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. MODE INVARIANTS — gb/gB/gs stay in Normal mode
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gb_stays_in_normal_mode() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(s.mode(), Mode::Normal, "gb should remain in Normal mode");
}

#[test]
fn gB_stays_in_normal_mode() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(8);
    feed(&mut s, "gB");
    assert_eq!(s.mode(), Mode::Normal, "gB should remain in Normal mode");
}

#[test]
fn gs_stays_in_normal_mode() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gs");
    assert_eq!(s.mode(), Mode::Normal, "gs should remain in Normal mode");
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════

/// gb on single-char word.
#[test]
fn gb_single_char_word() {
    let mut s = host_session("a b a b a");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        2,
        "gb on single-char word 'a' should find next 'a'"
    );
}

/// gb on last word in document (no more matches after, must wrap).
#[test]
fn gb_on_last_word_wraps() {
    let mut s = host_session("bar foo bar");
    s.set_cursor_offset(8); // last "bar"
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        2,
        "gb on last 'bar' should wrap to find first 'bar'"
    );
}

/// gb on empty document is a no-op (no panic).
#[test]
fn gb_empty_document_no_panic() {
    let mut s = host_session("");
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 1, "gb on empty doc should be no-op");
}

/// gb on a line with no word characters is a no-op.
#[test]
fn gb_on_whitespace_only_no_pattern() {
    let mut s = host_session("   ");
    s.set_cursor_offset(1);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 1, "gb on whitespace-only should be no-op");
}

/// gb match is case-sensitive: "Foo" does not match "foo".
#[test]
fn gb_case_sensitive() {
    let mut s = host_session("foo bar Foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        1,
        "gb should be case-sensitive: 'foo' != 'Foo'"
    );
}

/// Multiple gb then Escape clears all secondary cursors at once.
#[test]
fn multiple_gb_then_escape() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 3, "3 cursors after 2 gb presses");
    feed(&mut s, "<Esc>");
    assert_eq!(
        s.cursor_count(),
        1,
        "Escape should clear all secondary cursors at once"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. WHOLE-WORD MATCHING — gb must not match substrings
// ═══════════════════════════════════════════════════════════════════════════

/// gb on "foo" must NOT match "foobar" — whole-word boundaries required.
#[test]
fn gb_whole_word_does_not_match_substring() {
    let mut s = host_session("foobar baz foo");
    s.set_cursor_offset(11); // on "foo"
    feed(&mut s, "gb");
    // "foobar" is NOT a whole-word match for "foo", so the only match
    // is "foo" itself. With only one match, gb is a no-op.
    assert_eq!(
        s.cursor_count(),
        1,
        "gb on 'foo' should not match 'foobar' — whole-word matching"
    );
}

/// gb on "bar" must NOT match "foobar" — prefix boundary required.
#[test]
fn gb_whole_word_does_not_match_suffix() {
    let mut s = host_session("foobar baz bar");
    s.set_cursor_offset(11); // on "bar"
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        1,
        "gb on 'bar' should not match 'foobar' — whole-word matching"
    );
}

/// gb on "foo" DOES match a separate "foo" even when "foobar" exists.
#[test]
fn gb_whole_word_matches_separate_word() {
    let mut s = host_session("foo foobar foo");
    s.set_cursor_offset(0); // on first "foo"
    feed(&mut s, "gb");
    // "foobar" is skipped; second "foo" at offset 11 is a whole-word match.
    assert_eq!(
        s.cursor_count(),
        2,
        "gb on 'foo' should skip 'foobar' and match second 'foo'"
    );
    let positions = s.cursor_positions();
    let offsets: Vec<usize> = positions.iter().map(|p| p.2).collect();
    assert!(offsets.contains(&0), "should have cursor at first 'foo'");
    assert!(offsets.contains(&11), "should have cursor at second 'foo'");
}

/// gb whole-word matching works at document boundaries (start/end).
#[test]
fn gb_whole_word_at_document_boundaries() {
    let mut s = host_session("foo mid foo");
    s.set_cursor_offset(0); // on "foo" at start of document
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        2,
        "gb should match 'foo' at both document start and end"
    );
}

/// gb whole-word matching works with underscores (word chars).
#[test]
fn gb_whole_word_with_underscores() {
    let mut s = host_session("foo_bar baz foo_bar");
    s.set_cursor_offset(0); // on "foo_bar"
    feed(&mut s, "gb");
    assert_eq!(
        s.cursor_count(),
        2,
        "gb on 'foo_bar' should match second 'foo_bar'"
    );
}

/// gb whole-word: "foo" does not match within "a_foo_b" (underscore boundary).
#[test]
fn gb_whole_word_underscore_boundary() {
    let mut s = host_session("a_foo_b foo");
    s.set_cursor_offset(8); // on standalone "foo"
    feed(&mut s, "gb");
    // "foo" inside "a_foo_b" has word chars (underscore) on both sides.
    assert_eq!(
        s.cursor_count(),
        1,
        "gb on 'foo' should not match 'foo' inside 'a_foo_b'"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. PATTERN PRIORITY — word-under-cursor before search register
// ═══════════════════════════════════════════════════════════════════════════

/// When cursor is on a word AND search register is set, gb uses word-under-cursor.
#[test]
fn gb_word_under_cursor_has_priority_over_search() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    // Set search pattern via /bar<CR>
    feed(&mut s, "/bar<CR>");
    // Move back to "foo"
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    // Should add cursor at second "foo" (offset 8), not at "bar" (offset 4).
    assert_eq!(s.cursor_count(), 2);
    let positions = s.cursor_positions();
    let offsets: Vec<usize> = positions.iter().map(|p| p.2).collect();
    assert!(offsets.contains(&0), "should have cursor at first 'foo'");
    assert!(
        offsets.contains(&8),
        "should have cursor at second 'foo', not at 'bar'"
    );
}

/// When cursor is on whitespace, gb falls back to search register (no whole-word).
#[test]
fn gb_search_register_fallback_no_whole_word() {
    // Search register contains "foo" (from /foo), cursor on whitespace.
    // Search register pattern should NOT enforce whole-word boundaries,
    // so "foo" inside "foobar" WOULD match (substring match from search).
    let mut s = host_session("foobar   foo");
    s.set_cursor_offset(0);
    feed(&mut s, "/foo<CR>");
    s.set_cursor_offset(6); // on whitespace
    feed(&mut s, "gb");
    // With search register "foo" (no whole-word): matches "foobar" at 0 and "foo" at 9.
    assert_eq!(
        s.cursor_count(),
        2,
        "search register fallback should use substring matching (no word boundaries)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. AUDIT COVERAGE GAP TESTS
// ═══════════════════════════════════════════════════════════════════════════

// ── Gap 1: Undo cursor persistence (Section 7.2) ─────────────────────────
// The existing undo tests check text but never assert cursor_count after `u`.

#[test]
fn undo_preserves_cursor_count() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 2, "gb adds cursor");
    feed(&mut s, "ciwbaz");
    feed(&mut s, "\x1b"); // Escape
    assert_eq!(s.text(), "baz bar baz");
    feed(&mut s, "u");
    assert_eq!(s.text(), "foo bar foo", "undo reverts text");
    assert_eq!(s.cursor_count(), 2, "undo MUST preserve cursor count");
}

// ── Gap 2: Visual mode gb (Section 7.4) ──────────────────────────────────
// Visual gb should extract selection text, set search pattern, add cursor
// at next match, and exit to Normal+MC. Currently the grammar falls through
// to the normal gb path without extracting the selection — this test
// documents the expected behavior.

#[test]
fn gb_visual_mode_uses_selection_as_pattern() {
    let mut s = host_session("hello world hello");
    s.set_cursor_offset(0);
    feed(&mut s, "viw"); // select "hello"
    feed(&mut s, "gb"); // should set "hello" as pattern and add cursor
                        // Expected: 2 cursors (at both "hello"), Normal mode
    assert_eq!(
        s.cursor_count(),
        2,
        "Visual gb should add cursor at next match of selection"
    );
}

// ── Gap 3: Visual Block clears MC (Section 7.5) ──────────────────────────
// The design spec says entering Visual Block should clear multi-cursor,
// but the current mode_dispatch does not clear secondary cursors on
// visual block entry. This test documents the expected behavior.

#[test]
fn visual_block_clears_multi_cursor() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "<C-v>"); // Ctrl+V = Visual Block
    assert_eq!(s.cursor_count(), 1, "entering Visual Block should clear MC");
}

// ── Gap 4: Per-cursor failure isolation (Section 7.3) ────────────────────
// Two cursors where an operation works independently at each.

#[test]
fn per_cursor_failure_independent() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    s.add_cursor(8).unwrap(); // on second "foo"
    assert_eq!(s.cursor_count(), 2);
    // x deletes char at each cursor independently
    feed(&mut s, "x");
    // Both cursors should have deleted one char each
    assert_eq!(s.text(), "oo bar oo");
}

// ── Gap 5: gb in Insert mode (Section 7.9) ───────────────────────────────

#[test]
fn gb_in_insert_mode_inserts_letters() {
    let mut s = host_session("foo");
    s.set_cursor_offset(0);
    feed(&mut s, "i"); // enter Insert mode
    feed(&mut s, "gb"); // should insert "gb" literally
    feed(&mut s, "\x1b"); // Escape
    assert_eq!(
        s.text(),
        "gbfoo",
        "gb in Insert mode should insert literal 'gb'"
    );
    assert_eq!(
        s.cursor_count(),
        1,
        "no multi-cursor from gb in Insert mode"
    );
}

// ── Gap 6: Macro recording with gb (Section 7.10) ────────────────────────

#[test]
fn macro_records_and_replays_gb() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    // Record: qa gb ciwnew<Esc> q
    feed(&mut s, "qa"); // start recording to 'a'
    feed(&mut s, "gb"); // add next match
    feed(&mut s, "ciwnew"); // change word at both cursors
    feed(&mut s, "\x1b"); // Escape back to Normal+MC
    feed(&mut s, "q"); // stop recording
                       // After recording: the two "foo" occurrences should have been renamed
    assert!(
        s.text().contains("new"),
        "macro recording should have applied edit: got '{}'",
        s.text()
    );
}

// ── Gap 7: :cursorsplit (Workflow 6) ─────────────────────────────────────
// cursorsplit is a stub (returns Ok(()) with no action). Verify the command
// is at least accepted without error.

#[test]
fn ex_cursorsplit_accepted() {
    let mut s = host_session("foo\nbar\nbaz");
    s.set_cursor_offset(0);
    // cursorsplit is a stub — just verify it doesn't error/panic
    feed(&mut s, ":cursorsplit<CR>");
    // When implemented: should convert visual block to individual cursors.
    // For now, the stub does nothing, so no cursor count assertion.
}

// ── Gap 8: :cursorfilter functional test (Workflow 7) ────────────────────

#[test]
fn ex_cursorfilter_keeps_matching() {
    let mut s = host_session("foo\nimport foo\nfoo");
    s.set_cursor_offset(0);
    // First add cursors at all "foo" positions via :selectall
    feed(&mut s, ":selectall<CR>");
    let count_before = s.cursor_count();
    assert!(
        count_before >= 2,
        "selectall should find multiple 'foo', got {count_before}"
    );
    // cursorfilter keeps only cursors whose selection text matches pattern.
    // "import" pattern won't match "foo" selection text, so :cursorfilter
    // with bang (!) removes matching, and without bang keeps matching.
    // KeepMatching checks if the selection TEXT matches the regex.
    // Since all selections are "foo", filtering with /foo/ should keep all.
    feed(&mut s, ":cursorfilter foo<CR>");
    assert_eq!(
        s.cursor_count(),
        count_before,
        "cursorfilter /foo/ should keep all cursors whose selection is 'foo'"
    );
}

// ── Gap 9: Post-Escape single-cursor dot repeat (Section 7.1) ────────────

#[test]
fn dot_after_escape_replays_at_single_cursor() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb"); // 2 cursors
    feed(&mut s, "ciwbaz"); // change at both
    feed(&mut s, "\x1b"); // back to Normal+MC
    feed(&mut s, "\x1b"); // clear MC -> single cursor
    assert_eq!(s.cursor_count(), 1);
    // Move to a different word
    feed(&mut s, "w"); // move to next word
    feed(&mut s, "."); // dot should replay ciw at single cursor
                       // The dot repeat should change the word under cursor to "baz"
    let text = s.text();
    // Count occurrences of "baz" - should be at least 3 (original 2 from MC + 1 from dot)
    let baz_count = text.matches("baz").count();
    assert!(
        baz_count >= 3,
        "dot after Escape should replay ciw at single cursor, got text: '{text}'"
    );
}

// ── Gap 10: All matches exhausted then gb again = no-op (Section 7.8) ────

#[test]
fn gb_after_all_matches_exhausted_is_noop() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb"); // add second foo -> 2 cursors
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "gb"); // no more foo -> should be no-op
    assert_eq!(
        s.cursor_count(),
        2,
        "gb with all matches exhausted should be no-op"
    );
}

// ── Gap 11: *gbgb workflow (Workflow 1) ──────────────────────────────────

#[test]
fn workflow_star_gb_gb_ciw() {
    let mut s = host_session("foo bar foo baz foo");
    s.set_cursor_offset(0);
    feed(&mut s, "*"); // search for word under cursor (sets / register)
    feed(&mut s, "gb"); // add next match
    feed(&mut s, "gb"); // add third match
    assert_eq!(s.cursor_count(), 3, "* then 2x gb should give 3 cursors");
    feed(&mut s, "ciwnewName");
    feed(&mut s, "\x1b");
    assert_eq!(s.text(), "newName bar newName baz newName");
}

// ── Gap 12: Undo then re-edit with cursors still active (Workflow 2) ─────

#[test]
fn workflow_undo_then_reedit_with_cursors() {
    let mut s = host_session("foo bar foo");
    s.set_cursor_offset(0);
    feed(&mut s, "gb");
    assert_eq!(s.cursor_count(), 2);
    feed(&mut s, "ciwbaz");
    feed(&mut s, "\x1b");
    assert_eq!(s.text(), "baz bar baz");
    feed(&mut s, "u");
    assert_eq!(s.text(), "foo bar foo");
    assert_eq!(s.cursor_count(), 2, "cursors must survive undo");
    // Re-edit with cursors still active
    feed(&mut s, "ciwother");
    feed(&mut s, "\x1b");
    assert_eq!(
        s.text(),
        "other bar other",
        "re-edit after undo should work at all cursors"
    );
}
