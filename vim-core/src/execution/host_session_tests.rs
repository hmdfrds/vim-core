//! Integration tests for `HostSession`.

use super::HostSession;
use crate::dispatch::ViewportInfo;
use crate::execution::host::{
    CmdlineCompletionEntry, HostRequest, HostRequestId, HostRequestKind, HostResult,
};
use crate::execution::host_response::{CursorShape, HostResponse};
use crate::execution::host_session::UnknownRequestError;
use crate::execution::parse_keys_from_string;
use crate::execution::session_host::changeset_to_edit_ops;
use crate::execution::{ExternalEdit, ExternalEditKind};
use crate::primitives::{Mode, Offset, Range, SelectionShape, VisualType};

// ─────────────────────────────────────────────────────────────────────────────
// Helper
// ─────────────────────────────────────────────────────────────────────────────

/// Feed a Vim key-notation string into the session, returning the last response.
fn process_keys(session: &mut HostSession, keys: &str) -> HostResponse {
    let parsed = parse_keys_from_string(keys);
    let mut last = None;
    for key in parsed {
        last = Some(session.process_key_host(key));
    }
    last.expect("at least one key")
}

// ─────────────────────────────────────────────────────────────────────────────
// Lifecycle tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn new_session_starts_in_normal_mode() {
    let session = HostSession::new("hello world");
    assert_eq!(session.mode(), Mode::Normal);
    assert_eq!(session.cursor_offset(), 0);
    assert_eq!(session.text(), "hello world");
}

#[test]
fn set_text_replaces_document() {
    let mut session = HostSession::new("original");
    session.set_text("replaced");
    assert_eq!(session.text(), "replaced");
    assert_eq!(session.line_count(), 1);
}

#[test]
fn backspace_in_tab_indent_uses_engine_tabstop() {
    // Regression for hmdfrds/godot-vim#50: smarttab Backspace must align on
    // DISPLAY columns using the engine's tabstop, not raw byte offsets.
    //
    // noexpandtab, shiftwidth=4, tabstop=2: two leading tabs span 4 display
    // columns = exactly one shiftwidth, so Backspace deletes BOTH tabs. If
    // the engine failed to thread tabstop into InsertContext (defaulting to
    // 4), it would measure 8 columns and delete only one tab. This exercises
    // the full key-dispatch path, so it catches a missing `.with_tabstop`.
    let mut session = HostSession::new("\t\t");
    let mut opts = crate::primitives::VimOptions::default();
    opts.set_autoindent(false);
    opts.set_expandtab(false);
    opts.set_shiftwidth(4);
    opts.set_tabstop(2);
    session.set_options(opts);

    // `A` appends at end of line (cursor after both tabs, in insert mode).
    process_keys(&mut session, "A<BS>");

    assert_eq!(session.text(), "");
}

fn tab_indent_session(text: &str) -> HostSession {
    let mut session = HostSession::new(text);
    let mut opts = crate::primitives::VimOptions::default();
    opts.set_autoindent(false);
    opts.set_expandtab(false);
    opts.set_shiftwidth(4);
    opts.set_tabstop(4);
    session.set_options(opts);
    session
}

#[test]
fn outdent_removes_a_tab_on_tab_indented_line() {
    // Sibling of hmdfrds/godot-vim#50: `<<` on a tab-indented line removed
    // nothing under the old space-only scan. With noexpandtab/ts=4/sw=4 it
    // now removes exactly one tab (one indent level).
    let mut session = tab_indent_session("\thello");
    process_keys(&mut session, "<<");
    assert_eq!(session.text(), "hello");
}

#[test]
fn ctrl_d_outdents_tab_in_insert_mode() {
    // Insert-mode Ctrl-D removes one indent level; on a tab indent it now
    // removes a tab instead of doing nothing.
    let mut session = tab_indent_session("\t\thello");
    process_keys(&mut session, "i<C-d>");
    assert_eq!(session.text(), "\thello");
}

#[test]
fn text_returns_current_content() {
    let session = HostSession::new("line1\nline2\nline3");
    assert_eq!(session.line_count(), 3);
    assert_eq!(session.line(0), Some("line1"));
    assert_eq!(session.line(1), Some("line2"));
    assert_eq!(session.line(2), Some("line3"));
    assert_eq!(session.line(3), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// Mode transition tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn mode_transitions() {
    let mut session = HostSession::new("hello");
    assert_eq!(session.mode(), Mode::Normal);

    let resp = process_keys(&mut session, "i");
    assert_eq!(resp.mode, Mode::Insert);

    let resp = process_keys(&mut session, "<Esc>");
    assert_eq!(resp.mode, Mode::Normal);
}

#[test]
fn cursor_shape_per_mode() {
    let mut session = HostSession::new("hello");

    let resp = process_keys(&mut session, "i");
    assert_eq!(resp.cursor_shape, CursorShape::VerticalBar);

    process_keys(&mut session, "<Esc>");
    let resp = process_keys(&mut session, "R");
    assert_eq!(resp.cursor_shape, CursorShape::HorizontalBar);

    let resp = process_keys(&mut session, "<Esc>");
    assert_eq!(resp.cursor_shape, CursorShape::Block);
}

// ─────────────────────────────────────────────────────────────────────────────
// Document mutation tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn insert_char_updates_document() {
    let mut session = HostSession::new("");
    process_keys(&mut session, "iHello<Esc>");
    assert_eq!(session.text(), "Hello");
}

#[test]
fn delete_char_in_normal_mode() {
    let mut session = HostSession::new("hello");
    process_keys(&mut session, "x");
    assert_eq!(session.text(), "ello");
}

// ─────────────────────────────────────────────────────────────────────────────
// Dirty tracking tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn single_line_edit_not_full_redraw() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, "x");
    assert!(
        !resp.dirty.full_redraw(),
        "single-line delete should not trigger full redraw"
    );
}

#[test]
fn newline_insert_triggers_full_redraw() {
    let mut session = HostSession::new("hello");
    // `o` opens a new line (line count changes) — that response carries full_redraw.
    // We test `o` alone because `<Esc>` resets the dirty tracker for its own frame.
    let resp = process_keys(&mut session, "o");
    assert!(
        resp.dirty.full_redraw(),
        "opening a new line should trigger full redraw"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame ID tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn frame_id_increments() {
    let mut session = HostSession::new("hello");
    assert_eq!(session.frame_id(), 0);
    process_keys(&mut session, "i");
    assert!(session.frame_id() > 0);
    let prev = session.frame_id();
    process_keys(&mut session, "a");
    assert!(session.frame_id() > prev);
}

// ─────────────────────────────────────────────────────────────────────────────
// Consumed flag
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn consumed_reported() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, "i");
    assert!(resp.consumed, "'i' should be consumed");
}

// ─────────────────────────────────────────────────────────────────────────────
// HostResponse cursor line/col
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cursor_position_in_response() {
    let mut session = HostSession::new("hello\nworld");
    let resp = process_keys(&mut session, "j");
    assert_eq!(resp.cursor_line, 1);
    assert_eq!(resp.cursor_col, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Line count in response
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn line_count_in_response() {
    let mut session = HostSession::new("one\ntwo\nthree");
    let resp = process_keys(&mut session, "j");
    assert_eq!(resp.line_count, 3);
}

// ─────────────────────────────────────────────────────────────────────────────
// Command line info
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn command_line_info_populated() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, ":");
    assert_eq!(resp.mode, Mode::CommandLine);
    assert!(resp.command_line.is_some());
    let cl = resp.command_line.unwrap();
    assert_eq!(cl.prompt, ':');
}

// ─────────────────────────────────────────────────────────────────────────────
// Visual mode
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn visual_type_reported() {
    let mut session = HostSession::new("hello world");
    let resp = process_keys(&mut session, "v");
    assert_eq!(resp.visual_type, Some(VisualType::Char));

    process_keys(&mut session, "<Esc>");
    let resp = process_keys(&mut session, "V");
    assert_eq!(resp.visual_type, Some(VisualType::Line));
}

// ─────────────────────────────────────────────────────────────────────────────
// Configuration delegates
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn set_viewport_works() {
    let mut session = HostSession::new("hello");
    session.set_viewport(crate::dispatch::ViewportInfo {
        first_line: 0,
        height: 50,
        width: 120,
    });
    // Just verify it doesn't panic — viewport is internal
}

#[test]
fn set_timeoutlen_works() {
    let mut session = HostSession::new("hello");
    session.set_timeoutlen(500);
    assert_eq!(session.timeoutlen(), 500);
}

// ─────────────────────────────────────────────────────────────────────────────
// apply_external_edit — selection adjustment tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn external_edit_before_selection_shifts_both_offsets() {
    // "hello world" — selection on "world" (6..11), insert "XX" at 0 with no delete
    let mut session = HostSession::new("hello world");
    session.set_selection_raw(6, 11);
    session.apply_external_edit(0, 0, "XX");
    // Both anchor and head should shift by +2
    assert_eq!(session.selection_raw(), Some((8, 13)));
}

#[test]
fn external_edit_after_selection_leaves_selection_unchanged() {
    // "hello world" — selection on "hello" (0..5), edit at offset 6
    let mut session = HostSession::new("hello world");
    session.set_selection_raw(0, 5);
    session.apply_external_edit(6, 5, "earth");
    assert_eq!(session.selection_raw(), Some((0, 5)));
}

#[test]
fn external_edit_overlapping_selection_clears_it() {
    // "hello world" — selection on "lo wo" (3..8), delete range covers 2..9
    let mut session = HostSession::new("hello world");
    session.set_selection_raw(3, 8);
    session.apply_external_edit(2, 7, "");
    // Edit range [2, 9) fully covers selection [3, 8] → cleared
    assert_eq!(session.selection_raw(), None);
}

#[test]
fn external_edit_with_no_selection_does_not_crash() {
    let mut session = HostSession::new("hello world");
    // selection is None by default
    assert_eq!(session.selection_raw(), None);
    session.apply_external_edit(0, 5, "hi");
    assert_eq!(session.selection_raw(), None);
    assert_eq!(session.text(), "hi world");
}

#[test]
fn external_edit_selection_clamp_to_document_bounds() {
    // "ab" — selection (0, 2), delete everything and insert "x"
    let mut session = HostSession::new("ab");
    session.set_selection_raw(0, 2);
    // delete_end covers both → clear
    session.apply_external_edit(0, 2, "x");
    assert_eq!(session.selection_raw(), None);
    assert_eq!(session.text(), "x");
}

#[test]
fn external_edit_partially_overlapping_selection_adjusts() {
    // "abcdefgh" — selection anchor=2, head=6
    // Edit: delete 2 chars at offset 4 ("ef"), insert "XY"
    // delete_end = 6, anchor=2 (edit after anchor → no change), head=6 (delete_end <= head → shift)
    let mut session = HostSession::new("abcdefgh");
    session.set_selection_raw(2, 6);
    session.apply_external_edit(4, 2, "XY");
    // anchor=2: delete_end(6) > anchor(2), offset(4) > anchor(2) → edit after → no change → 2
    // head=6: delete_end(6) <= head(6) → shift by net delta (2 - 2 = 0) → 6
    assert_eq!(session.selection_raw(), Some((2, 6)));
}

#[test]
fn external_edit_delete_before_selection_shifts_back() {
    // "XXXhello" — selection on "hello" (3..8)
    // Delete "XXX" at offset 0
    let mut session = HostSession::new("XXXhello");
    session.set_selection_raw(3, 8);
    session.apply_external_edit(0, 3, "");
    // Both shift by -3
    assert_eq!(session.selection_raw(), Some((0, 5)));
    assert_eq!(session.text(), "hello");
}

// ─────────────────────────────────────────────────────────────────────────────
// cursor_pos() convenience
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cursor_pos_returns_line_and_col() {
    let mut session = HostSession::new("hello\nworld");
    let resp = process_keys(&mut session, "j");
    assert_eq!(resp.cursor_pos(), (1, 0));
    assert_eq!(resp.cursor_pos(), (resp.cursor_line, resp.cursor_col));
}

// ─────────────────────────────────────────────────────────────────────────────
// complete_request_checked
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn complete_request_checked_ok_for_valid_request() {
    let mut session = HostSession::new("hello");
    // :q!<CR> triggers a Quit host request
    let resp = process_keys(&mut session, ":q!<CR>");
    assert!(
        !resp.host_requests.is_empty(),
        "expected a host request from :q!"
    );
    let req_id = resp.host_requests[0].id();

    let result = HostResult::Success {
        id: req_id,
        message: None,
    };
    let checked = session.complete_request_checked(&result);
    assert!(checked.is_ok(), "valid request should return Ok");
}

#[test]
fn complete_request_checked_err_for_unknown_id() {
    let mut session = HostSession::new("hello");
    let bogus_id = HostRequestId::new(9999);
    let result = HostResult::Success {
        id: bogus_id,
        message: None,
    };
    let checked = session.complete_request_checked(&result);
    assert!(checked.is_err(), "unknown ID should return Err");
    assert_eq!(checked.unwrap_err(), UnknownRequestError { id: bogus_id });
}

#[test]
fn complete_request_checked_double_complete_returns_err() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, ":q!<CR>");
    assert!(!resp.host_requests.is_empty());
    let req_id = resp.host_requests[0].id();

    let result = HostResult::Success {
        id: req_id,
        message: None,
    };

    // First completion succeeds
    let first = session.complete_request_checked(&result);
    assert!(first.is_ok(), "first completion should succeed");

    // Second completion of the same ID should fail
    let second = session.complete_request_checked(&result);
    assert!(second.is_err(), "double-complete should return Err");
    assert_eq!(second.unwrap_err(), UnknownRequestError { id: req_id });
}

// ─────────────────────────────────────────────────────────────────────────────
// Visual marks tests (bug fixes)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn visual_tilde_sets_visual_marks() {
    let mut session = HostSession::new("hello world");
    // V~ : linewise visual select + toggle case
    process_keys(&mut session, "V~");
    assert_eq!(session.mode(), Mode::Normal);
    assert_eq!(session.text(), "HELLO WORLD");
    // After visual exit, '<' mark should be set to 0 (start of first selected line)
    let mark_lt = session.get_mark('<');
    let mark_gt = session.get_mark('>');
    eprintln!("mark.<: {:?}, mark.>: {:?}", mark_lt, mark_gt);
    assert!(
        mark_lt.is_some(),
        "'<' mark should be set after visual exit"
    );
    assert!(
        mark_gt.is_some(),
        "'>' mark should be set after visual exit"
    );
    assert_eq!(
        mark_lt.unwrap(),
        0,
        "'<' should be at offset 0 for linewise visual on first line"
    );
}

#[test]
fn regression_9_visual_marks() {
    let text = "sc zjwxi lgdkpys rdpm! gvyungy gdkjaf xmx ubs ztmlj nqyqfib noqnsa, xnzdq vjhn gwuvih vrg gvd kkhntj ltsgm wiatg; uxuyi tytg rpygtn eiohdh zlc;";
    let mut session = HostSession::new(text);
    // Set cursor to col 85
    session.set_cursor_offset(85);
    // The key sequence that produced the divergence
    let keys = "ya[VjeTreh~\"hp~";
    process_keys(&mut session, keys);
    let mark_lt = session.get_mark('<');
    let mark_gt = session.get_mark('>');
    eprintln!("regression_9: mark.<: {:?}, mark.>: {:?}", mark_lt, mark_gt);
    eprintln!("text: {:?}", session.text());
    eprintln!("cursor: {}", session.cursor_offset());
    eprintln!("mode: {:?}", session.mode());
    assert!(mark_lt.is_some(), "'<' mark should be set");
    assert_eq!(
        mark_lt.unwrap(),
        0,
        "'<' should be 0 for linewise visual on first line"
    );
}

#[test]
fn backslash_normal_mode_is_noop() {
    let mut session = HostSession::new("hello world");
    let before = session.text().to_string();
    process_keys(&mut session, "\\");
    assert_eq!(session.text(), before, "backslash should not modify text");
    assert_eq!(session.mode(), Mode::Normal, "should stay in normal mode");
}

#[test]
fn ya_backslash_does_not_modify_text() {
    let mut session = HostSession::new("hello world");
    let before = session.text().to_string();
    let _resp = process_keys(&mut session, "ya\\");
    assert_eq!(
        session.text(),
        before,
        "ya\\ should not modify text (invalid text object)"
    );
    assert_eq!(session.mode(), Mode::Normal, "should return to normal mode");
}

// ─────────────────────────────────────────────────────────────────────────────
// EditOp tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn insert_char_produces_edit_op() {
    let mut session = HostSession::new("hello");
    let _ = process_keys(&mut session, "i");
    let resp = process_keys(&mut session, "X");
    assert_eq!(resp.edits.len(), 1);
    assert_eq!(resp.edits[0].offset, 0);
    assert_eq!(resp.edits[0].delete, 0);
    assert_eq!(resp.edits[0].insert, "X");
}

#[test]
fn delete_char_produces_edit_op() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, "x");
    assert_eq!(resp.edits.len(), 1);
    assert_eq!(resp.edits[0].offset, 0);
    assert!(resp.edits[0].delete > 0);
    assert!(resp.edits[0].insert.is_empty());
}

#[test]
fn dd_produces_edit_op() {
    let mut session = HostSession::new("line one\nline two\nline three");
    let resp = process_keys(&mut session, "dd");
    assert!(!resp.edits.is_empty(), "dd should produce edit ops");
    assert_eq!(session.line_count(), 2);
}

#[test]
fn cursor_movement_produces_no_edit_ops() {
    let mut session = HostSession::new("hello\nworld");
    let resp = process_keys(&mut session, "j");
    assert!(
        resp.edits.is_empty(),
        "cursor movement should not produce edits"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Yank highlight tests
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Search match info tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn search_match_info_after_search() {
    let mut session = HostSession::new("foo bar foo baz foo");
    session.set_viewport(ViewportInfo {
        first_line: 0,
        height: 24,
        width: 80,
    });
    let _ = process_keys(&mut session, "/foo\n");
    let info = session.search_match_info();
    assert!(info.is_some(), "should have search match info after /foo");
    let info = info.unwrap();
    assert_eq!(info.total, 3, "should find 3 occurrences of 'foo'");
    assert!(
        info.current >= 1 && info.current <= 3,
        "current should be 1-indexed"
    );
}

#[test]
fn search_highlights_returns_match_positions() {
    let mut session = HostSession::new("foo bar foo baz foo");
    session.set_viewport(ViewportInfo {
        first_line: 0,
        height: 24,
        width: 80,
    });
    let _ = process_keys(&mut session, "/foo\n");
    let highlights = session.search_highlights();
    assert_eq!(highlights.len(), 3, "should have 3 matches");
    for (start, end) in highlights {
        assert!(end > start, "end should be after start");
    }
}

#[test]
fn search_match_info_none_without_search() {
    let session = HostSession::new("hello world");
    assert!(session.search_match_info().is_none());
}

#[test]
fn search_highlights_empty_without_search() {
    let session = HostSession::new("hello world");
    assert!(session.search_highlights().is_empty());
}

#[test]
fn search_highlights_cleared_by_nohlsearch() {
    let mut session = HostSession::new("foo bar foo baz foo");
    session.set_viewport(ViewportInfo {
        first_line: 0,
        height: 24,
        width: 80,
    });
    let _ = process_keys(&mut session, "/foo\n");
    assert!(
        !session.search_highlights().is_empty(),
        "should have highlights after search"
    );
    let _ = process_keys(&mut session, ":noh\n");
    assert!(
        session.search_highlights().is_empty(),
        "highlights should be cleared after :noh"
    );
    assert!(
        session.search_match_info().is_none(),
        "match info should be cleared after :noh"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Recording register tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn recording_register_tracks_macro() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, "qa");
    assert_eq!(
        resp.recording_register,
        Some('a'),
        "should be recording into register a"
    );
    let resp = process_keys(&mut session, "q");
    assert_eq!(resp.recording_register, None, "should stop recording");
}

#[test]
fn undo_restores_cursor_to_edit_position() {
    // Divergence found against Neovim: da` (no-op), then ~ at cursor, then G
    // (end), then u (undo). Undo should restore cursor to where ~ was applied.
    let text = "use std::collections::HashMap;\n\nfn word_count(text: &str) -> HashMap<&str, usize> {\n    let mut counts = HashMap::new();\n    for word in text.split_whitespace() {\n        *counts.entry(word).or_insert(0) += 1;\n    }\n    counts\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn test_count() {\n        let result = word_count(\"hello world hello\");\n        assert_eq!(result[\"hello\"], 2);\n        assert_eq!(result[\"world\"], 1);\n    }\n}";
    let mut session = HostSession::new(text);
    session.set_cursor_offset(297); // '_' in "test_count" on line 15

    // da` = delete around backtick (no backticks → no-op)
    process_keys(&mut session, "da`");
    assert_eq!(
        session.cursor_offset(),
        297,
        "da` no-op should not move cursor"
    );

    // ~ toggles '_' to... '_' doesn't change case. It advances cursor.
    process_keys(&mut session, "~");
    let cursor_after_tilde = session.cursor_offset();

    // G goes to last line
    process_keys(&mut session, "G");
    let cursor_after_g = session.cursor_offset();
    assert_ne!(
        cursor_after_g, cursor_after_tilde,
        "G should move to last line"
    );

    // u undoes the ~ toggle
    process_keys(&mut session, "u");
    assert_eq!(
        session.cursor_offset(),
        297,
        "undo should restore cursor to byte 297 (where ~ was applied), got {}",
        session.cursor_offset()
    );
}

#[test]
fn visual_outdent_preserves_cursor_column() {
    let mut session = HostSession::new("abcde fghij\nklmno pqrst");
    session.set_cursor_offset(5); // col 5 = 'f'
    process_keys(&mut session, "VG<");
    // After visual outdent (no-op), cursor should stay at col 5
    // matching Neovim's coladvance(old_col) behavior.
    assert_eq!(
        session.cursor_offset(),
        5,
        "Visual outdent should preserve cursor column. Got {}",
        session.cursor_offset()
    );
}

#[test]
fn visual_fx_consumes_char_on_failure() {
    // Divergence found against Neovim: in visual mode, Fx (find 'x' backward)
    // should consume 'x' as F's char argument even when the search fails.
    // 'x' must NOT fall through as a visual delete command.
    let mut session = HostSession::new("hello world\nabcd efgh");
    session.set_cursor_offset(15); // line 1, col 3 = 'd' in "abcd"

    // V = visual line, l = extend right
    process_keys(&mut session, "Vl");
    assert!(session.mode().is_visual(), "should be in visual mode");

    // Fx = find 'x' backward. No 'x' exists. Should fail silently.
    // 'x' MUST be consumed as F's argument, NOT processed as visual delete.
    let text_before = session.text().to_string();
    process_keys(&mut session, "Fx");
    assert_eq!(
        session.text(),
        text_before,
        "Fx failure should NOT delete text. Text was modified!"
    );
    assert!(
        session.mode().is_visual(),
        "After failed Fx, should stay in visual mode, not {:?}",
        session.mode()
    );
}

#[test]
fn oracle_93_ci_angle_no_phantom_insert() {
    // Divergence found against Neovim: ci< on text without angle brackets,
    // followed by normal keys. vim-core should NOT enter insert mode from a
    // failed ci<.
    let text = "sqmzkfp hjifnlei rasow xtriayb eiy nyy scnrx paenv dhausowz duyizb tjzumqdf mhz xixqyqm ikev qcm snnb ucmadjf ymf vlwfillm giljsiho ygotmmho dweirasf xxepclkd xxguwaju uoknjuzg caa tjlehus cvfdfal afopmh ivhriccj gkxeido jvrm vyuytt ggiem jbbepkx rhminf eonnar";
    let mut session = HostSession::new(text);
    session.set_cursor_offset(79);

    // ci< should fail (no brackets) → no text change, no mode change
    process_keys(&mut session, "ci<");
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "ci< should stay in Normal mode"
    );
    assert_eq!(session.text(), text, "ci< should not modify text");

    // guu should lowercase line (already lowercase → no-op)
    process_keys(&mut session, "guu");
    assert_eq!(
        session.text(),
        text,
        "guu on lowercase should not change text"
    );

    // da( should fail (no parens) → no text change
    process_keys(&mut session, "da(");
    assert_eq!(session.text(), text, "da( should not modify text");
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "da( should stay in Normal mode"
    );
}

#[test]
fn visual_u_lowercase_cursor_placement() {
    // After Vu (visual line + lowercase), cursor should go to col 0.
    let mut session = HostSession::new("HELLO WORLD\nABCD EFGH");
    session.set_cursor_offset(5); // col 5
    process_keys(&mut session, "Vu");
    assert_eq!(session.text(), "hello world\nABCD EFGH");
    assert_eq!(
        session.cursor_offset(),
        0,
        "After Vu, cursor should be at col 0 (oap->start.col=0 for visual linewise). Got {}",
        session.cursor_offset()
    );
}

#[test]
fn gUgg_uppercases_all_lines() {
    // Divergence found against Neovim: gUgg from line 2 should uppercase
    // ALL 3 lines.
    let mut session = HostSession::new("short\nmedium text here\nthe longest line in the buffer");
    session.set_cursor_offset(23); // line 2, col 0 = byte 23 (NOT 22 which is \n)

    process_keys(&mut session, "gUgg");

    assert_eq!(
        session.text(),
        "SHORT\nMEDIUM TEXT HERE\nTHE LONGEST LINE IN THE BUFFER",
        "gUgg from line 2 should uppercase ALL lines including line 2"
    );
}

#[test]
fn trace_oracle_78() {
    // Randomised-keystroke regression: 3>>i3CzzL9a9pJ<Esc>guu%$kGvhc
    // Text: "(wgbc [wn {blfbv }])\n{ffwum}\n(pujgm)\n[cqu]"
    // Cursor [3,0] = line 4, col 0 (0-based line 3) = byte offset 37
    //
    // Neovim reference trace (settings: noai expandtab sw=4 ts=4):
    //   INIT           cursor=(4, 0) byteoff=37  mode=n  textlen=42  line4='[cqu]'
    //   3>>            cursor=(4, 0) byteoff=37  mode=n  textlen=42  line4='[cqu]'      (NO-OP: 3>> on last line)
    //   i..J<Esc>      cursor=(4, 9) byteoff=46  mode=n  textlen=52  line4='3CzzL9a9pJ[cqu]'
    //   guu            cursor=(4, 0) byteoff=37  mode=n  textlen=52  line4='3czzl9a9pj[cqu]'
    //   %              cursor=(4,14) byteoff=51  mode=n  textlen=52  line4='3czzl9a9pj[cqu]'
    //   $              cursor=(4,14) byteoff=51  mode=n  textlen=52  line4='3czzl9a9pj[cqu]'
    //   k              cursor=(3, 6) byteoff=35  mode=n  textlen=52  line4='3czzl9a9pj[cqu]'
    //   G              cursor=(4,14) byteoff=51  mode=n  textlen=52  line4='3czzl9a9pj[cqu]'
    //   v              cursor=(4,14) byteoff=51  mode=v  textlen=52  line4='3czzl9a9pj[cqu]'
    //   h              cursor=(4,13) byteoff=50  mode=v  textlen=52  line4='3czzl9a9pj[cqu]'
    //   c              cursor=(4,12) byteoff=49  mode=n  textlen=50  line4='3czzl9a9pj[cq'
    //
    // Expected final: "(wgbc [wn {blfbv }])\n{ffwum}\n(pujgm)\n3czzl9a9pj[cq"

    let text = "(wgbc [wn {blfbv }])\n{ffwum}\n(pujgm)\n[cqu]";
    let mut session = HostSession::new(text);
    session.set_cursor_offset(37); // byte offset for cursor [3,0]

    // Match the reference Neovim settings: noai expandtab sw=4 ts=4
    let mut opts = crate::primitives::VimOptions::default();
    opts.set_autoindent(false);
    session.set_options(opts);

    let groups: &[(&str, &str)] = &[
        ("3>>", "3>>"),
        ("i..J", "i3CzzL9a9pJ"),
        ("<Esc>", "<Esc>"),
        ("guu", "guu"),
        ("%", "%"),
        ("$", "$"),
        ("k", "k"),
        ("G", "G"),
        ("v", "v"),
        ("h", "h"),
        ("c", "c"),
    ];

    for (label, keys) in groups {
        let resp = process_keys(&mut session, keys);
        eprintln!(
            "[78] {:<10} offset={:>3}  mode={:?}  textlen={:>3}  text='{}'",
            label,
            session.cursor_offset(),
            resp.mode,
            session.text().len(),
            session.text().replace('\n', "\\n"),
        );
    }

    eprintln!("[78] FINAL TEXT: '{}'", session.text().replace('\n', "\\n"));
    eprintln!(
        "[78] FINAL offset={} mode={:?}",
        session.cursor_offset(),
        session.mode()
    );
}

#[test]
fn trace_oracle_93() {
    // Randomised-keystroke regression: ci<guuda(GVehwth<dkas lXfk3lX<Esc>
    // Single line, cursor at byte 79
    //
    // Neovim reference trace:
    //   ci<  → no-op (no <> pair)
    //   guu  → no-op (line already lowercase)
    //   da(  → no-op (no () pair)
    //   G    → goto last line (same line, end-ish)
    //   V    → visual line
    //   e    → end of word
    //   h    → left
    //   w    → next word
    //   th   → find 'h' backward on line (t motion)
    //   <    → visual outdent (no-op since no indent, but cursor repositioned)
    //
    // After '<' (visual outdent): Neovim cursor=7, vim-core was giving cursor=8.
    // This traces every step to find the exact divergence point.

    let text = "sqmzkfp hjifnlei rasow xtriayb eiy nyy scnrx paenv dhausowz duyizb tjzumqdf mhz xixqyqm ikev qcm snnb ucmadjf ymf vlwfillm giljsiho ygotmmho dweirasf xxepclkd xxguwaju uoknjuzg caa tjlehus cvfdfal afopmh ivhriccj gkxeido jvrm vyuytt ggiem jbbepkx rhminf eonnar";
    let mut session = HostSession::new(text);
    session.set_cursor_offset(79);

    // Match the reference Neovim settings: noai expandtab sw=4 ts=4
    let mut opts = crate::primitives::VimOptions::default();
    opts.set_autoindent(false);
    session.set_options(opts);

    let groups: &[(&str, &str)] = &[
        ("ci<", "ci<"),
        ("guu", "guu"),
        ("da(", "da("),
        ("G", "G"),
        ("V", "V"),
        ("e", "e"),
        ("h", "h"),
        ("w", "w"),
        ("th", "th"),
        ("<", "<"), // visual outdent — THIS is where divergence should be
        ("dk", "dk"),
        ("a", "a"),
        ("text", "s lXfk3lX"),
        ("<Esc>", "<Esc>"),
    ];

    for (label, keys) in groups {
        let resp = process_keys(&mut session, keys);
        let sel_info = resp
            .selection
            .as_ref()
            .map(|s| {
                format!(
                    "sel=({},{})..({},{})",
                    s.anchor_line, s.anchor_col, s.head_line, s.head_col
                )
            })
            .unwrap_or_default();
        eprintln!(
            "[93] {:<6}: offset={:>3}  col={:>3}  mode={:?}  textlen={:>3}  {}  text='{}'",
            label,
            session.cursor_offset(),
            resp.cursor_col,
            resp.mode,
            session.text().len(),
            sel_info,
            &session.text()[..80.min(session.text().len())],
        );
    }

    eprintln!("[93] FINAL TEXT: '{}'", session.text());
    eprintln!(
        "[93] FINAL offset={} mode={:?}",
        session.cursor_offset(),
        session.mode()
    );

    // Verify final state matches the Neovim reference output
    let expected_text = "sqmzkfp hjifnlei rasow xtriayb eiy nyy scnrx paenv ds lXfk3lXhausowz duyizb tjzumqdf mhz xixqyqm ikev qcm snnb ucmadjf ymf vlwfillm giljsiho ygotmmho dweirasf xxepclkd xxguwaju uoknjuzg caa tjlehus cvfdfal afopmh ivhriccj gkxeido jvrm vyuytt ggiem jbbepkx rhminf eonnar";
    assert_eq!(session.text(), expected_text, "text mismatch");
    assert_eq!(session.cursor_offset(), 60, "cursor_offset mismatch");
    assert_eq!(session.mode(), Mode::Normal, "mode mismatch");
}

#[test]
fn trace_oracle_70() {
    // Randomised-keystroke regression: Fg$gUggvbyhdT{>gg
    // Initial cursor [36, 12] = 0-based line 36, col 12 = byte offset 1144
    //
    // Neovim reference trace:
    //   INIT           cursor=(37, 12) line36="mbjdmg jpszvbv vws nigijn"  (1-based line 37)
    //   Fg             cursor=(37,  5) = found 'g' backward at col 5
    //   $              cursor=(37, 24) = end of line (last char 'n')
    //   gUgg           cursor=( 1, 43) = uppercased lines 1-37, cursor at line 1 col 43 (curswant=MAXCOL from $)
    //   vby            yanked "TZOT" (cols 40-43), cursor at col 40
    //   h              cursor=( 1, 39)
    //   dT{            no-op (no '{' found on line)
    //   >gg            indented line 1, cursor at col 39
    //
    // Expected final: cursor_offset=39, text has 4-space indent on line 0, lines 0-36 uppercased

    let text = "upcqaal oor cgqwau vyh vs fpgprz tousmg tzot\nrzap tuaavw zjtzfxv icto snydivh przdnwi biikvig\njxphvoo ph kmy\newh xczzkda qesome\ncg qp pxqb bm woppcwo zv yetej awadxef\nphk dadkroz bahzoj irbke obzn myjbtq uztb\nbsc jov zumhfez\nfemgpy ogak kuquw rxfxehr\nvlmkqal gezzg uwi xoiid kpst ipozfev jomw xehlpt\nnnecx ski rvypye zmtt\njern rsfaed jqe\nbdsppbm lcghdz bf lxuci\nxdoo kmapm kvh xtwwsll ebczfpy ewukmwn xqe\nituavz mtu wonhc xdsa lmaaqtg dgklmpc qrees aas\njq ceelw wrk dfkqf ibzx uwnnp\nxkdl dpksiw ex ikryifh\nqre ccn jxsji epoche ghpyajt qncrypj\nrlbyj bt nmbvra hm knyoo hbql iqssz vh\ndoh im\neye ksqtxk dttjcon olvxtsr gu ikathww\nlcsk paar bxowgoa rrp ta aij qtqikz yuyg\nnpwe rf lkmewu aefpgoy\nodf svhhsr oelltq nmzppev kzkhqtz\nuxvtj yi rrnkw\nrntvzyb aclslq wkwq ngpaovg gnonc idwcv\nqqygfj mlinsxb kbwq fotaal uebbr swqtbh iylu ciggy\nsr xrn rth dlg rsttrxd bm nal\ngg ktgr vgrio poof ukv ivgi\nun ojjr pds idvp hajnrp pzyvtd nlni euytqug\nweyq lk fcehzoc cgtfwy gykgpvv\nia bntej accpxxr vuys mmiqxrb\nupkpac kzvby khtevzg avcoxo\ntyymhxp vsnzp ilh usbw uyhro udy\nhdatgxt quk ejbds vnjd ac miivnz lngvwcz\ngc yoghyon xswi\nmgcmlr jeck yfwjijy\nmbjdmg jpszvbv vws nigijn\nriwqnnw fb atrgxb cxio grqwlci mxaojdf kcvjl\npx yawnqs tjpsa xq uh zcx\njizl nzbbdxu ghnbt psrhkpx mbqjeo kv\ndzf mdbhdkx";
    let mut session = HostSession::new(text);
    session.set_cursor_offset(1144); // byte offset for [36, 12] = 'b' in "jpszvbv"

    // Match the reference Neovim settings: noai expandtab sw=4 ts=4
    let mut opts = crate::primitives::VimOptions::default();
    opts.set_autoindent(false);
    session.set_options(opts);

    eprintln!("\n[70] === TRACE START ===");
    eprintln!(
        "[70] INIT       offset={:>4}  mode={:?}  textlen={:>4}",
        session.cursor_offset(),
        session.mode(),
        session.text().len()
    );

    let groups: &[(&str, &str)] = &[
        ("Fg", "Fg"),
        ("$", "$"),
        ("gUgg", "gUgg"),
        ("v", "v"),
        ("b", "b"),
        ("y", "y"),
        ("h", "h"),
        ("dT{", "dT{"),
        (">gg", ">gg"),
    ];

    for (label, keys) in groups {
        let resp = process_keys(&mut session, keys);
        // Show cursor line/col from the response
        eprintln!(
            "[70] {:<8}  offset={:>4}  line={:>2}  col={:>2}  mode={:?}  textlen={:>4}",
            label,
            session.cursor_offset(),
            resp.cursor_line,
            resp.cursor_col,
            resp.mode,
            session.text().len(),
        );
    }

    eprintln!("[70] === TRACE END ===");
    eprintln!(
        "[70] FINAL TEXT (first 80 chars): '{}'",
        &session.text()[..80.min(session.text().len())]
    );
    eprintln!(
        "[70] FINAL TEXT line 0: '{}'",
        session.text().lines().next().unwrap_or("")
    );
}

#[test]
fn replace_char_on_empty_is_noop() {
    // 'rz' on empty text should be a no-op (Neovim behavior).
    let mut session = HostSession::new("");
    process_keys(&mut session, "rz");
    assert_eq!(
        session.text(),
        "",
        "rz on empty should not create text. Got {:?}",
        session.text()
    );
}

#[test]
fn oracle_446_xXrz_undo() {
    // Randomised-keystroke regression: "xXrz<0guutru^" on "z" should end with
    // empty text.
    let mut session = HostSession::new("z");

    // x deletes 'z' → ""
    process_keys(&mut session, "x");
    assert_eq!(session.text(), "", "x should delete 'z'");

    // X on empty → no-op
    process_keys(&mut session, "X");
    assert_eq!(session.text(), "");

    // rz on empty → no-op
    process_keys(&mut session, "rz");
    assert_eq!(session.text(), "");

    // <0 → outdent to col 0. On empty → no-op
    process_keys(&mut session, "<0");
    assert_eq!(session.text(), "");

    // guu → lowercase line. On empty → no-op
    process_keys(&mut session, "guu");
    assert_eq!(session.text(), "");

    // tru: t=find-before, r=char → tr (find 'r' before cursor). Fails on empty.
    // Then u = UNDO.
    // In Neovim: u undoes guu (no-op undo), then text stays empty.
    // Actually: if guu didn't create an undo entry, u undoes x → "z".
    // Neovim says empty. So guu or something DID create an undo entry.
    process_keys(&mut session, "tr");
    process_keys(&mut session, "u");
    eprintln!(
        "[446] After u: text={:?} cursor={}",
        session.text(),
        session.cursor_offset()
    );

    // ^ = first non-blank
    process_keys(&mut session, "^");
    eprintln!(
        "[446] Final: text={:?} cursor={}",
        session.text(),
        session.cursor_offset()
    );
}

#[test]
fn trace_regression_5_mark_dot() {
    // Randomised-keystroke regression: Neovim sets mark. = 4 due to a cindent
    // '^' boundary.
    //
    // ROOT CAUSE in Neovim: the literal '^' character (0x5E) triggers cindent
    // reindentation which creates an extra change boundary. Our engine does
    // not implement cindent, so mark `.` correctly tracks the LAST change
    // position in insert mode (matching Neovim's changed_bytes behavior
    // from change.c:461 → changed_common → RESET_FMARK at change.c:260).
    //
    // Key sequence: OV6QeKJlWc2<Esc>0ddd%ef0cto^On;V3nw!<Esc>Fo
    // The 'cto' changes text till 'o', then '^On;V3nw!' is inserted.
    // Without cindent, mark `.` = 11 (position of last insert '!').
    let mut session = HostSession::new(
        "gcub fbwfft kuxfb eucmb jiwc rfcf aow; dubmsh edt ot bkwl oqpg? prvzo hsfz dnr.",
    );
    session.set_cursor_offset(62);
    process_keys(&mut session, "OV6QeKJlWc2<Esc>0ddd%ef0cto^On;V3nw!<Esc>Fo");

    // Text and cursor match Neovim exactly
    assert_eq!(
        session.text(),
        "gcu^On;V3nw!ow; dubmsh edt ot bkwl oqpg? prvzo hsfz dnr."
    );
    assert_eq!(session.cursor_offset(), 11);

    // mark[ and mark] and mark^ match Neovim
    assert_eq!(session.get_mark('['), Some(3));
    assert_eq!(session.get_mark(']'), Some(12));
    assert_eq!(session.get_mark('^'), Some(12));

    // mark `.` = Neovim reports 4.  The inserted text "^On;V3nw!" starts
    // with '^' which is ISSPECIAL in Neovim's batching logic (edit.c:1917).
    // '^' goes through ins_char_bytes individually.  Then "On;V3nw!" is
    // all batchable ASCII (no_abbr=true, '3' is not ISSPECIAL), so the
    // entire run batches into one ins_str call → changed_bytes(lnum, 1)
    // where col=1 is the position after '^'.  mark_dot = insert_start(3)
    // + 1 = 4.  This now matches Neovim exactly.
    let mark_dot = session.get_mark('.');
    assert_eq!(
        mark_dot,
        Some(4),
        "mark. = col after '^' (ISSPECIAL boundary), matching Neovim"
    );
}

#[test]
fn mark_dot_after_insert_0_caret() {
    // Proof tests: mark.. position relative to '0' and '^' in insert mode.
    // Neovim sets mark.. to the position of the char AFTER '0'/'^'.

    // Test 1: Sabc0def → mark.. should be at col 4 ('d', char after '0')
    let mut session = HostSession::new("hello");
    process_keys(&mut session, "Sabc0def");
    process_keys(&mut session, "\x1b");
    let mark = session.get_mark('.').unwrap_or(999);
    eprintln!("Sabc0def: mark..={} (expect 4)", mark);

    // Test 2: Sxyz^ABC → mark.. at col 4 ('A', char after '^')
    let mut session2 = HostSession::new("hello");
    process_keys(&mut session2, "Sxyz^ABC");
    process_keys(&mut session2, "\x1b");
    let mark2 = session2.get_mark('.').unwrap_or(999);
    eprintln!("Sxyz^ABC: mark..={} (expect 4)", mark2);

    // Test 3: S0xyz → mark.. at col 1 ('x', char after '0')
    let mut session3 = HostSession::new("hello");
    process_keys(&mut session3, "S0xyz");
    process_keys(&mut session3, "\x1b");
    let mark3 = session3.get_mark('.').unwrap_or(999);
    eprintln!("S0xyz: mark..={} (expect 1)", mark3);
}

// ─────────────────────────────────────────────────────────────────────────────
// changeset_to_edit_ops unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn changeset_to_edit_ops_empty() {
    let ops = changeset_to_edit_ops(&[]);
    assert!(ops.is_empty());
}

#[test]
fn changeset_to_edit_ops_retain_then_insert() {
    use crate::primitives::TextOp;
    use compact_str::CompactString;
    let ops = changeset_to_edit_ops(&[
        TextOp::Retain(5),
        TextOp::Insert(CompactString::from("abc")),
    ]);
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].offset, 5);
    assert_eq!(ops[0].delete, 0);
    assert_eq!(ops[0].insert, "abc");
}

#[test]
fn changeset_to_edit_ops_retain_then_delete() {
    use crate::primitives::TextOp;
    let ops = changeset_to_edit_ops(&[TextOp::Retain(3), TextOp::Delete(2)]);
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].offset, 3);
    assert_eq!(ops[0].delete, 2);
    assert_eq!(ops[0].insert, "");
}

#[test]
fn changeset_to_edit_ops_insert_at_zero() {
    use crate::primitives::TextOp;
    use compact_str::CompactString;
    let ops = changeset_to_edit_ops(&[TextOp::Insert(CompactString::from("hello"))]);
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].offset, 0);
    assert_eq!(ops[0].delete, 0);
    assert_eq!(ops[0].insert, "hello");
}

#[test]
fn changeset_to_edit_ops_mixed_sequence() {
    use crate::primitives::TextOp;
    use compact_str::CompactString;
    // [Retain(2), Delete(1), Retain(3), Insert("X")]
    let ops = changeset_to_edit_ops(&[
        TextOp::Retain(2),
        TextOp::Delete(1),
        TextOp::Retain(3),
        TextOp::Insert(CompactString::from("X")),
    ]);
    assert_eq!(ops.len(), 2);
    // First: delete 1 byte at offset 2
    assert_eq!(ops[0].offset, 2);
    assert_eq!(ops[0].delete, 1);
    assert_eq!(ops[0].insert, "");
    // Second: insert "X" at offset 2 + 3 = 5 (delete doesn't advance, retain(3) does)
    assert_eq!(ops[1].offset, 5);
    assert_eq!(ops[1].delete, 0);
    assert_eq!(ops[1].insert, "X");
}

// ─────────────────────────────────────────────────────────────────────────────
// Undo/redo EditOps integration tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn undo_emits_edit_ops() {
    let mut session = HostSession::new("hello\n");
    // Insert some text: "iWorld<Esc>" at the beginning → "Worldhello\n"
    // `i` enters insert at position 0, so typing "World" produces "Worldhello\n".
    process_keys(&mut session, "iWorld");
    process_keys(&mut session, "<Esc>");
    let text_after_insert = session.text().to_owned();
    assert_eq!(text_after_insert, "Worldhello\n");

    // Now undo — should revert to "hello\n" and produce non-empty edits
    let resp = process_keys(&mut session, "u");
    assert!(
        !resp.edits.is_empty(),
        "undo should emit EditOps but got empty edits"
    );
    assert_eq!(session.text(), "hello\n");
}

#[test]
fn redo_emits_edit_ops() {
    let mut session = HostSession::new("hello\n");
    process_keys(&mut session, "iWorld");
    process_keys(&mut session, "<Esc>");
    assert_eq!(session.text(), "Worldhello\n");

    // Undo
    process_keys(&mut session, "u");
    assert_eq!(session.text(), "hello\n");

    // Redo — should restore "Worldhello\n" and emit non-empty edits
    let resp = process_keys(&mut session, "<C-r>");
    assert!(
        !resp.edits.is_empty(),
        "redo should emit EditOps but got empty edits"
    );
    assert_eq!(session.text(), "Worldhello\n");
}

// ─────────────────────────────────────────────────────────────────────────────
// Undo cursor placement for o-command
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn undo_cursor_after_o_insert_and_dd() {
    // Neovim fidelity: iLine one is good.<CR>Line two is bad.<CR>Line three is fine.<Esc>2Gddoorrr Line two is okay.<Esc>u
    // After undo, Neovim cursor is at offset 36 (the '.' at end of "Line three is fine.").
    //
    // The cursor chain:
    //   <Esc> → offset 53 ('.' end of line 3), sticky_column = vcol 18
    //   2G    → coladvance(18) on "Line two is bad." (16 chars) → clamped col 15, offset 33
    //   dd    → surviving line "Line three is fine." (19 chars), coladvance(18) → col 18, offset 36
    //   o+type+<Esc> → inserts new line 3, undo group cursor_before = 36
    //   u     → EntryPosition strategy → cursor_before = 36
    let mut session = HostSession::new("");

    process_keys(
        &mut session,
        "iLine one is good.<CR>Line two is bad.<CR>Line three is fine.<Esc>",
    );
    assert_eq!(
        session.text(),
        "Line one is good.\nLine two is bad.\nLine three is fine."
    );

    process_keys(&mut session, "2G");
    // nostartofline: preserves sticky column (vcol 18 from insert exit)
    assert_eq!(
        session.cursor_offset(),
        33,
        "2G with nostartofline: col clamped to end of shorter line 2"
    );

    process_keys(&mut session, "dd");
    assert_eq!(session.text(), "Line one is good.\nLine three is fine.");
    assert_eq!(
        session.cursor_offset(),
        36,
        "dd: coladvance(curswant=18) on surviving line"
    );

    process_keys(&mut session, "oorrr Line two is okay.<Esc>");
    assert_eq!(
        session.text(),
        "Line one is good.\nLine three is fine.\norrr Line two is okay."
    );

    process_keys(&mut session, "u");
    assert_eq!(session.text(), "Line one is good.\nLine three is fine.");
    assert_eq!(
        session.cursor_offset(),
        36,
        "undo of o-insert restores cursor to pre-command position"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// process_mouse_selection tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn process_mouse_selection_enters_visual_char() {
    let mut session = HostSession::new("hello world\nfoo bar\n");
    let resp = session.process_mouse_selection_host(2, 8, SelectionShape::Char);
    assert!(resp.consumed, "mouse selection should be consumed");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Char));
    assert_eq!(
        session.selection_raw(),
        Some((2, 8)),
        "selection anchor/head should match the drag offsets"
    );
    assert_eq!(
        session.cursor_offset(),
        8,
        "cursor should land at the head offset"
    );
}

#[test]
fn process_mouse_selection_enters_visual_line() {
    let mut session = HostSession::new("hello world\nfoo bar\n");
    let resp = session.process_mouse_selection_host(0, 12, SelectionShape::Line);
    assert!(resp.consumed, "mouse selection should be consumed");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Line));
    assert_eq!(
        session.selection_raw(),
        Some((0, 12)),
        "line selection anchor/head should match the drag offsets"
    );
    assert_eq!(
        session.cursor_offset(),
        12,
        "cursor should land at the head offset for line selection"
    );
}

#[test]
fn process_mouse_selection_resets_parser() {
    let mut session = HostSession::new("hello world\n");
    // 'd' puts the parser into operator-pending (response is pending)
    let resp = process_keys(&mut session, "d");
    assert_eq!(
        resp.mode,
        Mode::Normal,
        "state mode should still be Normal during operator-pending"
    );
    // Mouse selection should cancel the pending operator and enter Visual
    let resp = session.process_mouse_selection_host(0, 5, SelectionShape::Char);
    assert!(resp.consumed, "mouse selection should be consumed");
    assert_eq!(
        session.mode(),
        Mode::Visual(VisualType::Char),
        "mouse selection should cancel operator-pending and enter Visual"
    );
    assert_eq!(
        session.selection_raw(),
        Some((0, 5)),
        "selection should span the dragged range after parser reset"
    );
    assert_eq!(
        session.cursor_offset(),
        5,
        "cursor should land at head offset after parser reset"
    );
}

#[test]
fn process_mouse_selection_enters_visual_block() {
    let mut session = HostSession::new("hello world\nfoo bar\n");
    let resp = session.process_mouse_selection_host(0, 5, SelectionShape::Block);
    assert!(resp.consumed, "mouse selection should be consumed");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Block));
    assert_eq!(
        session.selection_raw(),
        Some((0, 5)),
        "block selection anchor/head should match the drag offsets"
    );
    assert_eq!(
        session.cursor_offset(),
        5,
        "cursor should land at the head offset for block selection"
    );
}

#[test]
fn process_mouse_selection_reselects_from_visual_mode() {
    let mut session = HostSession::new("hello world\nfoo bar\n");

    // Enter Visual mode via keyboard ('v')
    process_keys(&mut session, "v");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Char));

    // Move cursor to extend selection via keyboard
    process_keys(&mut session, "ll");
    let old_selection = session.selection_raw();
    assert!(old_selection.is_some(), "should have a keyboard selection");

    // Now do a mouse drag — should replace the keyboard selection with a new one
    let resp = session.process_mouse_selection_host(6, 10, SelectionShape::Char);
    assert!(resp.consumed, "mouse selection should be consumed");
    assert_eq!(
        session.mode(),
        Mode::Visual(VisualType::Char),
        "should remain in Visual Char mode"
    );
    assert_eq!(
        session.selection_raw(),
        Some((6, 10)),
        "mouse drag should replace the previous keyboard selection"
    );
    assert_eq!(
        session.cursor_offset(),
        10,
        "cursor should land at the new head offset"
    );
}

#[test]
fn process_mouse_selection_changes_visual_type_from_existing() {
    let mut session = HostSession::new("hello world\nfoo bar\n");

    // Enter Visual Char mode via keyboard
    process_keys(&mut session, "v");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Char));

    // Mouse drag with Line shape should switch to Visual Line
    let resp = session.process_mouse_selection_host(0, 12, SelectionShape::Line);
    assert!(resp.consumed, "mouse selection should be consumed");
    assert_eq!(
        session.mode(),
        Mode::Visual(VisualType::Line),
        "mouse drag should switch visual type from Char to Line"
    );
    assert_eq!(
        session.selection_raw(),
        Some((0, 12)),
        "selection should reflect the new mouse drag range"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Shadow execution macro replay regression tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn shadow_macro_ciw_insert_ordering() {
    // Regression test: macro `0fwciwbruh<Esc>` on "hello world" should produce
    // "hello bruh", not "hello ruhb". The bug manifests when shadow execution
    // is enabled — the shadow document's cursor tracking during the `ciw` +
    // insert sequence can reverse the inserted characters.

    let mut session = HostSession::new("hello world");
    session.set_shadow_execution(true);

    // Step 1: Record macro into register 'a': 0fwciwbruh<Esc>
    //   0     → go to column 0
    //   fw    → find 'w' (moves to 'w' in "world")
    //   ciw   → change inner word (deletes "world", enters insert)
    //   bruh  → type "bruh"
    //   <Esc> → exit insert mode
    process_keys(&mut session, "qa0fwciwbruh<Esc>q");

    // After recording, text should be "hello bruh" with cursor on 'h' of "bruh"
    eprintln!(
        "[after recording] text={:?}, cursor={}, mode={:?}",
        session.text(),
        session.cursor_offset(),
        session.mode()
    );
    assert_eq!(
        session.text(),
        "hello bruh",
        "recording itself should produce correct text"
    );

    // Step 2: Reset the document for replay
    session.set_text("hello world");
    session.set_cursor_offset(0);

    eprintln!(
        "[before replay] text={:?}, cursor={}, mode={:?}",
        session.text(),
        session.cursor_offset(),
        session.mode()
    );

    // Step 3: Replay macro @a (with shadow execution enabled)
    process_keys(&mut session, "@a");

    eprintln!(
        "[after replay] text={:?}, cursor={}, mode={:?}",
        session.text(),
        session.cursor_offset(),
        session.mode()
    );

    // The critical assertion: text must be "hello bruh", NOT "hello ruhb"
    assert_eq!(
        session.text(),
        "hello bruh",
        "shadow macro replay of ciw+insert should produce 'hello bruh', not reversed text"
    );
    assert_eq!(
        session.mode(),
        Mode::Normal,
        "should be back in Normal mode after macro replay"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Replace mode undo tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn replace_mode_undo_restores_original_text() {
    let mut session = HostSession::new("hello\n");

    // Enter replace mode and type abc, then exit
    process_keys(&mut session, "Rabc<Esc>");
    assert_eq!(session.text(), "abclo\n");
    assert_eq!(session.mode(), Mode::Normal);

    // Undo -- should restore the entire replace session
    let undo_resp = process_keys(&mut session, "u");
    assert_eq!(
        session.text(),
        "hello\n",
        "undo after Rabc<Esc> should restore original text"
    );
    assert!(
        !undo_resp.edits.is_empty(),
        "undo should produce edit ops for the host"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Command-line completion round-trip
// ─────────────────────────────────────────────────────────────────────────────

/// Helper: find the first `RequestCmdlineCompletion` in a response's host_requests.
fn find_cmdline_completion_request(resp: &HostResponse) -> Option<&HostRequest> {
    resp.host_requests
        .iter()
        .find(|r| r.kind() == HostRequestKind::RequestCmdlineCompletion)
}

/// Helper: find the first `SyncCommandLine` in a response's host_requests.
fn find_sync_command_line(resp: &HostResponse) -> Option<&HostRequest> {
    resp.host_requests
        .iter()
        .find(|r| r.kind() == HostRequestKind::SyncCommandLine)
}

/// Helper: extract the input string from a `SyncCommandLine` request.
fn sync_input(req: &HostRequest) -> &str {
    if let HostRequest::SyncCommandLine { input, .. } = req {
        input.as_str()
    } else {
        panic!("expected SyncCommandLine, got {req:?}");
    }
}

#[test]
fn cmdline_completion_tab_after_edit_emits_request() {
    let mut session = HostSession::new("hello");
    // Type `:edit ` then press Tab
    let resp = process_keys(&mut session, ":edit <Tab>");
    assert_eq!(session.mode(), Mode::CommandLine);

    // The response should contain a RequestCmdlineCompletion
    let req = find_cmdline_completion_request(&resp);
    assert!(
        req.is_some(),
        "Tab after `:edit ` should emit RequestCmdlineCompletion; \
         got host_requests: {:?}",
        resp.host_requests
            .iter()
            .map(|r| r.kind())
            .collect::<Vec<_>>()
    );

    // Verify the request fields
    if let Some(HostRequest::RequestCmdlineCompletion { kind, prefix, .. }) = req {
        assert_eq!(
            *kind,
            crate::execution::host::CmdlineCompletionKind::FilePath,
            "`:edit` should request FilePath completion"
        );
        assert!(
            prefix.is_empty(),
            "prefix should be empty since input after `edit ` is empty"
        );
    }
}

#[test]
fn cmdline_completion_fulfillment_updates_command_line() {
    let mut session = HostSession::new("hello");
    // Type `:edit ` then press Tab
    let resp = process_keys(&mut session, ":edit <Tab>");
    let req = find_cmdline_completion_request(&resp).expect("should emit RequestCmdlineCompletion");
    let req_id = req.id();

    // Simulate host response with candidates.
    let result = HostResult::CmdlineCompletionCandidates {
        id: req_id,
        candidates: vec![
            CmdlineCompletionEntry {
                text: "file_a.rs".into(),
                description: None,
                detail: None,
            },
            CmdlineCompletionEntry {
                text: "file_b.rs".into(),
                description: None,
                detail: None,
            },
        ],
    };
    let fulfill_resp = session.complete_request_host(&result);

    // The command-line text should now contain the first candidate.
    let sync =
        find_sync_command_line(&fulfill_resp).expect("fulfillment should emit SyncCommandLine");
    let input = sync_input(sync);
    assert!(
        input.contains("file_a.rs"),
        "command line should contain first candidate 'file_a.rs', got: {input}"
    );
}

#[test]
fn cmdline_completion_subsequent_tab_cycles_cached_candidates() {
    let mut session = HostSession::new("hello");
    // Type `:edit ` then press Tab
    let resp = process_keys(&mut session, ":edit <Tab>");
    let req = find_cmdline_completion_request(&resp).expect("should emit RequestCmdlineCompletion");
    let req_id = req.id();

    // Fulfill with candidates.
    let result = HostResult::CmdlineCompletionCandidates {
        id: req_id,
        candidates: vec![
            CmdlineCompletionEntry {
                text: "alpha.txt".into(),
                description: None,
                detail: None,
            },
            CmdlineCompletionEntry {
                text: "beta.txt".into(),
                description: None,
                detail: None,
            },
            CmdlineCompletionEntry {
                text: "gamma.txt".into(),
                description: None,
                detail: None,
            },
        ],
    };
    let fulfill_resp = session.complete_request_host(&result);

    // First fulfillment should show "alpha.txt"
    let sync = find_sync_command_line(&fulfill_resp).expect("SyncCommandLine");
    assert!(
        sync_input(sync).contains("alpha.txt"),
        "first candidate should be alpha.txt"
    );

    // Press Tab again — should cycle to "beta.txt" using cached candidates.
    let tab2 = process_keys(&mut session, "<Tab>");
    let sync2 = find_sync_command_line(&tab2).expect("SyncCommandLine on second Tab");
    assert!(
        sync_input(sync2).contains("beta.txt"),
        "second Tab should cycle to beta.txt, got: {}",
        sync_input(sync2)
    );

    // Press Tab again — should cycle to "gamma.txt".
    let tab3 = process_keys(&mut session, "<Tab>");
    let sync3 = find_sync_command_line(&tab3).expect("SyncCommandLine on third Tab");
    assert!(
        sync_input(sync3).contains("gamma.txt"),
        "third Tab should cycle to gamma.txt, got: {}",
        sync_input(sync3)
    );

    // Press Tab again — should cycle back to original (empty arg).
    let tab4 = process_keys(&mut session, "<Tab>");
    let sync4 = find_sync_command_line(&tab4).expect("SyncCommandLine on fourth Tab");
    let input4 = sync_input(sync4);
    assert!(
        !input4.contains("alpha.txt")
            && !input4.contains("beta.txt")
            && !input4.contains("gamma.txt"),
        "fourth Tab should cycle back to original input, got: {input4}"
    );

    // Press Tab once more — back to first candidate.
    let tab5 = process_keys(&mut session, "<Tab>");
    let sync5 = find_sync_command_line(&tab5).expect("SyncCommandLine on fifth Tab");
    assert!(
        sync_input(sync5).contains("alpha.txt"),
        "fifth Tab should return to alpha.txt, got: {}",
        sync_input(sync5)
    );
}

#[test]
fn cmdline_completion_empty_candidates_is_noop() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, ":edit <Tab>");
    let req = find_cmdline_completion_request(&resp).expect("should emit RequestCmdlineCompletion");
    let req_id = req.id();

    // Fulfill with empty candidates.
    let result = HostResult::CmdlineCompletionCandidates {
        id: req_id,
        candidates: vec![],
    };
    let fulfill_resp = session.complete_request_host(&result);

    // Command line should remain unchanged (still `:edit `).
    let sync = find_sync_command_line(&fulfill_resp).expect("SyncCommandLine");
    let input = sync_input(sync);
    assert_eq!(
        input, "edit ",
        "empty candidates should not change the command line"
    );
}

#[test]
fn cmdline_completion_failure_shows_error() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, ":edit <Tab>");
    let req = find_cmdline_completion_request(&resp).expect("should emit RequestCmdlineCompletion");
    let req_id = req.id();

    // Fulfill with failure.
    let result = HostResult::Failure {
        id: req_id,
        error: "permission denied".into(),
    };
    let fulfill_resp = session.complete_request_host(&result);

    // Should show an error message, not crash.
    assert!(
        fulfill_resp.message.is_some(),
        "failure should produce a status message"
    );
}

#[test]
fn cmdline_completion_buffer_kind_for_buffer_command() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, ":buffer <Tab>");

    let req = find_cmdline_completion_request(&resp);
    assert!(
        req.is_some(),
        "Tab after `:buffer ` should emit RequestCmdlineCompletion"
    );

    if let Some(HostRequest::RequestCmdlineCompletion { kind, .. }) = req {
        assert_eq!(
            *kind,
            crate::execution::host::CmdlineCompletionKind::Buffer,
            "`:buffer` should request Buffer completion"
        );
    }
}

#[test]
fn cmdline_completion_with_prefix() {
    let mut session = HostSession::new("hello");
    // Type `:edit src/` then press Tab
    let resp = process_keys(&mut session, ":edit src/<Tab>");

    let req = find_cmdline_completion_request(&resp);
    assert!(
        req.is_some(),
        "Tab after `:edit src/` should emit RequestCmdlineCompletion"
    );

    if let Some(HostRequest::RequestCmdlineCompletion { prefix, .. }) = req {
        assert_eq!(
            prefix.as_str(),
            "src/",
            "prefix should be 'src/' since that's the partial input"
        );
    }
}

#[test]
fn cmdline_completion_checked_rejects_unknown_id() {
    let mut session = HostSession::new("hello");
    let _resp = process_keys(&mut session, ":edit <Tab>");

    // Try to complete with a bogus ID.
    let bogus_result = HostResult::CmdlineCompletionCandidates {
        id: HostRequestId::new(99999),
        candidates: vec![CmdlineCompletionEntry {
            text: "should_not_appear.rs".into(),
            description: None,
            detail: None,
        }],
    };
    let checked = session.complete_request_checked(&bogus_result);
    assert!(
        checked.is_err(),
        "completing with unknown ID should return Err"
    );
}

#[test]
fn cmdline_completion_preserves_candidate_metadata() {
    let mut session = HostSession::new("hello");
    let resp = process_keys(&mut session, ":edit <Tab>");
    let req = find_cmdline_completion_request(&resp).expect("should emit RequestCmdlineCompletion");
    let req_id = req.id();

    // Fulfill with candidates that have description and detail.
    let result = HostResult::CmdlineCompletionCandidates {
        id: req_id,
        candidates: vec![CmdlineCompletionEntry {
            text: "main.rs".into(),
            description: Some("Rust source".into()),
            detail: Some("src/main.rs".into()),
        }],
    };
    let fulfill_resp = session.complete_request_host(&result);

    // Verify the command line text was updated.
    let cl = fulfill_resp
        .command_line()
        .expect("should have command_line info after fulfillment");
    assert!(
        cl.input.contains("main.rs"),
        "command line should contain 'main.rs', got: {}",
        cl.input
    );

    // Verify candidate metadata is carried through.
    let candidates = cl
        .candidates
        .as_ref()
        .expect("candidates should be present");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].text.as_str(), "main.rs");
    assert_eq!(candidates[0].description.as_deref(), Some("Rust source"));
    assert_eq!(candidates[0].detail.as_deref(), Some("src/main.rs"));
    assert_eq!(
        cl.selected_index,
        Some(0),
        "first Tab should select index 0"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// replace_text tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn replace_text_non_undoable_clears_undo_tree() {
    let mut session = HostSession::new("hello world");
    // Make an edit so there's something to undo
    process_keys(&mut session, "iInserted ");
    process_keys(&mut session, "<Esc>");
    assert_eq!(session.text(), "Inserted hello world");
    assert!(session.engine().undo_tree().can_undo());

    // Replace text non-undoable
    session.replace_text("brand new content", false);
    assert_eq!(session.text(), "brand new content");
    assert_eq!(session.cursor_offset(), 0);
    assert_eq!(session.mode(), Mode::Normal);

    // Undo tree is cleared — can_undo is false
    assert!(
        !session.engine().undo_tree().can_undo(),
        "undo tree should be cleared after non-undoable replace_text"
    );

    // Pressing 'u' should not change the text
    process_keys(&mut session, "u");
    assert_eq!(session.text(), "brand new content");
}

#[test]
fn replace_text_undoable_allows_undo_to_restore_old_text() {
    let mut session = HostSession::new("original text");
    // Make an edit first
    process_keys(&mut session, "Amore");
    process_keys(&mut session, "<Esc>");
    assert_eq!(session.text(), "original textmore");

    // Replace text undoable
    session.replace_text("completely different", true);
    assert_eq!(session.text(), "completely different");
    assert_eq!(session.cursor_offset(), 0);
    assert_eq!(session.mode(), Mode::Normal);

    // Undo tree has exactly one group — can_undo is true
    assert!(
        session.engine().undo_tree().can_undo(),
        "undo tree should have one group after undoable replace_text"
    );

    // Pressing 'u' should restore the OLD text (the text at the time of replace_text)
    process_keys(&mut session, "u");
    assert_eq!(
        session.text(),
        "original textmore",
        "undo after undoable replace_text should restore the previous document text"
    );
}

#[test]
fn replace_text_undoable_no_op_when_text_identical() {
    let mut session = HostSession::new("same content");
    // Make an edit to have undo history
    process_keys(&mut session, "iPrefix ");
    process_keys(&mut session, "<Esc>");
    assert_eq!(session.text(), "Prefix same content");

    // Replace with same text — should be early exit after mode/abandon
    session.replace_text("Prefix same content", true);
    assert_eq!(session.text(), "Prefix same content");

    // The existing undo history from the prefix insert should still work
    // (early exit path does NOT clear the undo tree since text is identical)
    process_keys(&mut session, "u");
    assert_eq!(session.text(), "same content");
}

#[test]
fn replace_text_forces_normal_mode_from_insert() {
    let mut session = HostSession::new("hello");
    // Enter insert mode
    process_keys(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);

    session.replace_text("new text", false);
    assert_eq!(session.mode(), Mode::Normal);
    assert_eq!(session.text(), "new text");
}

#[test]
fn replace_text_clears_marks_and_changelist() {
    let mut session = HostSession::new("hello world");
    // Set a local mark
    process_keys(&mut session, "ma");
    // Make an edit to populate changelist
    process_keys(&mut session, "x");

    session.replace_text("new", false);

    // After replace, local marks should be cleared
    // The mark 'a' should no longer be set
    use crate::primitives::MarkName;
    let mark_a = session
        .engine_mut()
        .marks_mut()
        .get(MarkName::new('a').unwrap());
    assert!(
        mark_a.is_none(),
        "local marks should be cleared after replace_text"
    );
}

#[test]
fn macro_with_search_and_delete_produces_correct_edit_ops() {
    let mut session = HostSession::new("keep foo remove foo keep\n");

    // Record macro: search for "foo", delete word
    let _ = process_keys(&mut session, "qa/foo<CR>dwq");
    assert_eq!(session.text(), "keep remove foo keep\n", "after recording");

    // Replay macro
    let response = process_keys(&mut session, "@a");
    assert_eq!(session.text(), "keep remove keep\n", "after @a replay");

    // Verify edit ops: should be a single DELETE at offset 12, len 4
    let edits = response.edits();
    assert!(!edits.is_empty(), "should have edit ops, got {:?}", edits);
    let first = &edits[0];
    assert_eq!(
        first.offset, 12,
        "edit offset should be 12 (start of second 'foo')"
    );
    assert_eq!(first.delete, 4, "delete len should be 4 ('foo ')");

    // Verify the edit offset is correct
    assert_eq!(first.offset, 12, "edit should be at offset 12");
    assert_eq!(first.delete, 4, "delete should be 4 bytes");
}

#[test]
fn macro_replay_separate_keys_with_resolve_timeout() {
    let mut session = HostSession::new("keep foo remove foo keep\n");

    // Record macro: feed each key separately, the way a host does
    for key in crate::execution::parse_keys_from_string("qa/foo") {
        let resp = session.process_key_host(key);
        // Fulfill any SyncCommandLine requests
        for req in resp.host_requests() {
            let result = HostResult::Success {
                id: req.id(),
                message: None,
            };
            let _ = session.complete_request_host(&result);
        }
    }
    // <CR> is special
    for key in crate::execution::parse_keys_from_string("<CR>dw") {
        let resp = session.process_key_host(key);
        for req in resp.host_requests() {
            let result = HostResult::Success {
                id: req.id(),
                message: None,
            };
            let _ = session.complete_request_host(&result);
        }
    }
    // Stop recording
    let resp = process_keys(&mut session, "q");
    for req in resp.host_requests() {
        let result = HostResult::Success {
            id: req.id(),
            message: None,
        };
        let _ = session.complete_request_host(&result);
    }
    assert_eq!(session.text(), "keep remove foo keep\n", "after recording");

    // Replay with separate keys and a pending-key timeout resolved between
    let resp_at = session.process_key_host(crate::execution::parse_keys_from_string("@")[0]);
    for req in resp_at.host_requests() {
        let result = HostResult::Success {
            id: req.id(),
            message: None,
        };
        let _ = session.complete_request_host(&result);
    }
    // Simulate the host's pending-prefix timeout firing
    let _ = session.resolve_timeout();

    // Then send 'a'
    let resp_a = session.process_key_host(crate::execution::parse_keys_from_string("a")[0]);
    assert_eq!(session.text(), "keep remove keep\n", "after @a replay");

    let edits = resp_a.edits();
    assert!(!edits.is_empty(), "should have edit ops");
    let first = &edits[0];
    eprintln!(
        "replay edit: offset={} delete={}",
        first.offset, first.delete
    );
    assert_eq!(first.offset, 12, "edit offset should be 12");
    assert_eq!(first.delete, 4, "delete len should be 4");
}

#[test]
fn macro_replay_shadow_search_delete_edit_offset() {
    // Reproducer: with shadow_enabled=true (the path used by hosts that keep
    // their own copy of the document), macro replay of /foo<CR>dw should
    // produce an edit at offset 12, not offset 5.
    // Shadow execution replays all pending macro keys in-memory against a
    // snapshot, then diffs to produce EditOps. If the diff is computed
    // against the pre-first-edit document, offsets will be wrong.

    let mut session = HostSession::new("keep foo remove foo keep\n");
    session.set_shadow_execution(true);

    // Record macro: feed each key separately, the way a host does
    for key in crate::execution::parse_keys_from_string("qa/foo") {
        let resp = session.process_key_host(key);
        for req in resp.host_requests() {
            let result = HostResult::Success {
                id: req.id(),
                message: None,
            };
            let _ = session.complete_request_host(&result);
        }
    }
    for key in crate::execution::parse_keys_from_string("<CR>dw") {
        let resp = session.process_key_host(key);
        for req in resp.host_requests() {
            let result = HostResult::Success {
                id: req.id(),
                message: None,
            };
            let _ = session.complete_request_host(&result);
        }
    }
    // Stop recording
    let resp = process_keys(&mut session, "q");
    for req in resp.host_requests() {
        let result = HostResult::Success {
            id: req.id(),
            message: None,
        };
        let _ = session.complete_request_host(&result);
    }
    assert_eq!(session.text(), "keep remove foo keep\n", "after recording");

    // Replay: separate '@' then 'a', with a resolve_timeout in between — the
    // sequence a host produces when it settles a pending prefix
    let resp_at = session.process_key_host(crate::execution::parse_keys_from_string("@")[0]);
    for req in resp_at.host_requests() {
        let result = HostResult::Success {
            id: req.id(),
            message: None,
        };
        let _ = session.complete_request_host(&result);
    }
    let _ = session.resolve_timeout();

    let resp_a = session.process_key_host(crate::execution::parse_keys_from_string("a")[0]);

    eprintln!(
        "[shadow replay] text={:?}, cursor={}, mode={:?}",
        session.text(),
        session.cursor_offset(),
        session.mode()
    );

    // BUG: With shadow_enabled=true, the shadow replay operates on a stale
    // text snapshot. It finds "foo" at offset 5 in the snapshot (where the
    // first "foo" was before recording deleted it), producing a diff of
    // offset=5, delete=7 — which deletes "remove " instead of "foo ".
    // Actual result: text="keep foo keep\n", edit={offset:5, delete:7}
    // Correct result: text="keep remove keep\n", edit={offset:12, delete:4}
    let edits = resp_a.edits();
    eprintln!("shadow replay edits: {:?}", edits);
    assert!(!edits.is_empty(), "should have edit ops from shadow replay");
    let first = &edits[0];
    eprintln!(
        "shadow replay edit: offset={} delete={} insert={:?}",
        first.offset, first.delete, first.insert
    );

    assert_eq!(
        session.text(),
        "keep remove keep\n",
        "after @a replay with shadow"
    );
    assert_eq!(first.offset, 12, "shadow edit offset should be 12, not 5");
    assert_eq!(first.delete, 4, "shadow edit delete should be 4 bytes");
}

#[test]
fn macro_replay_shadow_undo_insert() {
    let mut session = HostSession::new("\n");
    session.set_shadow_execution(true);

    // Record: insert "hello\n"
    process_keys(&mut session, "qaihello<CR><Esc>q");
    assert_eq!(session.text(), "hello\n\n", "after recording");

    // Replay
    let replay_resp = process_keys(&mut session, "@a");
    assert_eq!(session.text(), "hello\nhello\n\n", "after replay");

    // Check undo group: the replay response should have EndUndoGroup with node_id set
    // (Print edits and effects for debugging)
    eprintln!("replay edits: {:?}", replay_resp.edits());

    // Undo
    let undo_resp = process_keys(&mut session, "u");
    eprintln!(
        "after u: text={:?}, edits={:?}",
        session.text(),
        undo_resp.edits()
    );
    assert_eq!(
        session.text(),
        "hello\n\n",
        "after u should undo replay only"
    );
}

/// Same as `macro_replay_shadow_undo_insert` but with shadow_execution=false
/// to exercise the `drain_pending_keys` path directly.
#[test]
fn macro_replay_noshadow_undo_insert() {
    let mut session = HostSession::new("\n");
    session.set_shadow_execution(false);

    // Record: insert "hello\n"
    process_keys(&mut session, "qaihello<CR><Esc>q");
    assert_eq!(session.text(), "hello\n\n", "after recording");

    // Replay
    process_keys(&mut session, "@a");
    assert_eq!(session.text(), "hello\nhello\n\n", "after replay");

    // Undo should revert the entire replay as a single undo entry
    process_keys(&mut session, "u");
    assert_eq!(
        session.text(),
        "hello\n\n",
        "after u should undo replay only"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2@a undo with shadow execution — reproducer for multi-count macro undo bug
// ─────────────────────────────────────────────────────────────────────────────

/// With shadow execution enabled (the path used by hosts that keep their own
/// copy of the document), `2@a` should produce a SINGLE undo group covering
/// both replays. The first `u` must revert both lines back, not just one.
/// This test reproduces the bug where shadow
/// execution processes macro keys inside `engine.process()` before
/// `drain_pending_keys` captures `text_before_drain`, causing the UndoStore to
/// miss the first replay's changes.
#[test]
fn shadow_undo_2_at_a_covers_both_replays() {
    let mut session = HostSession::new(
        "i want to change the world\ni want to change the world\ni want to change the world",
    );
    session.set_shadow_execution(true);

    // Record macro into register 'a':
    //   0           → go to column 0
    //   /change<CR> → search for "change"
    //   ciw         → change inner word (deletes "change", enters insert)
    //   lol         → type "lol"
    //   <Esc>       → exit insert mode
    //   j           → move down one line
    process_keys(&mut session, "qa0/change<CR>ciwlol<Esc>jq");

    eprintln!(
        "[after recording] text={:?}, cursor={}, mode={:?}",
        session.text(),
        session.cursor_offset(),
        session.mode()
    );

    // After recording, line 0 should have "lol" instead of "change",
    // and cursor should be on line 1.
    assert_eq!(
        session.line(0),
        Some("i want to lol the world"),
        "recording: line 0 should have 'lol'"
    );
    assert_eq!(
        session.line(1),
        Some("i want to change the world"),
        "recording: line 1 should still have 'change'"
    );
    assert_eq!(
        session.line(2),
        Some("i want to change the world"),
        "recording: line 2 should still have 'change'"
    );

    // Replay macro twice: 2@a
    // This should apply the macro to lines 1 and 2.
    let replay_resp = process_keys(&mut session, "2@a");

    eprintln!(
        "[after 2@a] text={:?}, cursor={}, mode={:?}",
        session.text(),
        session.cursor_offset(),
        session.mode()
    );
    eprintln!("[after 2@a] edits: {:?}", replay_resp.edits());

    assert_eq!(
        session.line(0),
        Some("i want to lol the world"),
        "after 2@a: line 0 should still have 'lol'"
    );
    assert_eq!(
        session.line(1),
        Some("i want to lol the world"),
        "after 2@a: line 1 should have 'lol'"
    );
    assert_eq!(
        session.line(2),
        Some("i want to lol the world"),
        "after 2@a: line 2 should have 'lol'"
    );

    // First undo: should revert BOTH lines changed by 2@a (lines 1 and 2).
    // The bug: with shadow execution, only one line's changes are covered by
    // the undo group, so `u` only reverts one line.
    let undo1_resp = process_keys(&mut session, "u");

    eprintln!(
        "[after first u] text={:?}, edits={:?}",
        session.text(),
        undo1_resp.edits()
    );

    // The undo edits must cover both lines 1 and 2.
    let undo1_edits = undo1_resp.edits();
    assert!(
        !undo1_edits.is_empty(),
        "first undo should produce edit ops"
    );

    // After first undo, lines 1 and 2 should be back to "change".
    // Line 0 stays "lol" (that was from the recording, a separate undo group).
    assert_eq!(
        session.line(0),
        Some("i want to lol the world"),
        "after first u: line 0 should still have 'lol' (recording's undo group)"
    );
    assert_eq!(
        session.line(1),
        Some("i want to change the world"),
        "after first u: line 1 should be reverted to 'change'"
    );
    assert_eq!(
        session.line(2),
        Some("i want to change the world"),
        "after first u: line 2 should be reverted to 'change'"
    );

    // Count how many bytes the undo edits touched — should cover changes on
    // BOTH lines, not just one.
    let total_undo_delete: usize = undo1_edits.iter().map(|e| e.delete).sum();
    let total_undo_insert: usize = undo1_edits.iter().map(|e| e.insert.len()).sum();
    eprintln!("undo1 total: delete={total_undo_delete} bytes, insert={total_undo_insert} bytes");
    // "lol" (3 bytes) -> "change" (6 bytes) on two lines means at least 6 deleted, 12 inserted.
    assert!(
        total_undo_insert >= 12,
        "first undo should insert at least 12 bytes (two 'change' restorations), got {total_undo_insert}"
    );

    // Second undo: should revert line 0 (the recording's change).
    let undo2_resp = process_keys(&mut session, "u");

    eprintln!(
        "[after second u] text={:?}, edits={:?}",
        session.text(),
        undo2_resp.edits()
    );

    assert_eq!(
        session.line(0),
        Some("i want to change the world"),
        "after second u: line 0 should be reverted to 'change'"
    );
    assert_eq!(
        session.line(1),
        Some("i want to change the world"),
        "after second u: line 1 should still be 'change'"
    );
    assert_eq!(
        session.line(2),
        Some("i want to change the world"),
        "after second u: line 2 should still be 'change'"
    );
}

/// Companion test: same as `shadow_undo_2_at_a_covers_both_replays` but
/// WITHOUT shadow execution. Both paths must produce identical undo behavior.
#[test]
fn noshadow_undo_2_at_a_covers_both_replays() {
    let mut session = HostSession::new(
        "i want to change the world\ni want to change the world\ni want to change the world",
    );
    session.set_shadow_execution(false);

    // Record macro into register 'a'
    process_keys(&mut session, "qa0/change<CR>ciwlol<Esc>jq");

    assert_eq!(
        session.line(0),
        Some("i want to lol the world"),
        "recording: line 0 should have 'lol'"
    );
    assert_eq!(
        session.line(1),
        Some("i want to change the world"),
        "recording: line 1 should still have 'change'"
    );
    assert_eq!(
        session.line(2),
        Some("i want to change the world"),
        "recording: line 2 should still have 'change'"
    );

    // Replay macro twice: 2@a
    let replay_resp = process_keys(&mut session, "2@a");

    eprintln!(
        "[noshadow after 2@a] text={:?}, edits={:?}",
        session.text(),
        replay_resp.edits()
    );

    assert_eq!(
        session.line(0),
        Some("i want to lol the world"),
        "after 2@a: line 0 should still have 'lol'"
    );
    assert_eq!(
        session.line(1),
        Some("i want to lol the world"),
        "after 2@a: line 1 should have 'lol'"
    );
    assert_eq!(
        session.line(2),
        Some("i want to lol the world"),
        "after 2@a: line 2 should have 'lol'"
    );

    // First undo: should revert BOTH lines changed by 2@a.
    let undo1_resp = process_keys(&mut session, "u");

    eprintln!(
        "[noshadow after first u] text={:?}, edits={:?}",
        session.text(),
        undo1_resp.edits()
    );

    assert_eq!(
        session.line(0),
        Some("i want to lol the world"),
        "after first u: line 0 should still have 'lol'"
    );
    assert_eq!(
        session.line(1),
        Some("i want to change the world"),
        "after first u: line 1 should be reverted to 'change'"
    );
    assert_eq!(
        session.line(2),
        Some("i want to change the world"),
        "after first u: line 2 should be reverted to 'change'"
    );

    // Verify undo edits cover both lines.
    let undo1_edits = undo1_resp.edits();
    assert!(
        !undo1_edits.is_empty(),
        "first undo should produce edit ops"
    );
    let total_undo_insert: usize = undo1_edits.iter().map(|e| e.insert.len()).sum();
    assert!(
        total_undo_insert >= 12,
        "first undo should insert at least 12 bytes (two 'change' restorations), got {total_undo_insert}"
    );

    // Second undo: should revert the recording's change on line 0.
    process_keys(&mut session, "u");

    assert_eq!(
        session.line(0),
        Some("i want to change the world"),
        "after second u: line 0 should be reverted to 'change'"
    );
    assert_eq!(
        session.line(1),
        Some("i want to change the world"),
        "after second u: line 1 should still be 'change'"
    );
    assert_eq!(
        session.line(2),
        Some("i want to change the world"),
        "after second u: line 2 should still be 'change'"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Engine tracing tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "engine-tracing")]
fn trace_events_collected_during_process_key() {
    use crate::execution::trace::TraceEvent;
    let mut session = HostSession::new("hello");
    session.set_tracing_enabled(true);
    process_keys(&mut session, "l");
    let events = session.drain_trace_events();
    assert!(!events.is_empty());
    assert!(events
        .iter()
        .any(|e| matches!(e, TraceEvent::ProcessKey { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, TraceEvent::ProcessKeyDone { .. })));
}

#[test]
#[cfg(feature = "engine-tracing")]
fn trace_events_empty_when_disabled() {
    let mut session = HostSession::new("hello");
    // tracing disabled by default
    process_keys(&mut session, "l");
    let events = session.drain_trace_events();
    assert!(events.is_empty());
}

#[test]
#[cfg(feature = "engine-tracing")]
fn inspect_returns_snapshot() {
    let mut session = HostSession::new("hello world");
    session.set_tracing_enabled(true);
    let snapshot = session.inspect();
    assert_eq!(snapshot.mode.as_str(), "NORMAL");
    assert!(snapshot.document_len > 0);
}

#[test]
fn large_count_put_200() {
    let mut session = HostSession::new("i want to change the world\n");
    process_keys(&mut session, "yy");
    process_keys(&mut session, "200p");
    let line_count = session.text().lines().count();
    assert!(line_count >= 201, "expected >= 201 lines, got {line_count}");
}

#[test]
fn large_count_put_1000() {
    let mut session = HostSession::new("i want to change the world\n");
    process_keys(&mut session, "yy");
    process_keys(&mut session, "1000p");
    let line_count = session.text().lines().count();
    assert!(
        line_count >= 1001,
        "expected >= 1001 lines, got {line_count}"
    );
}

#[test]
fn shadow_syncs_after_undo() {
    let mut session = HostSession::new("hello world");
    session.set_shadow_text("hello world");

    // Make an edit: delete "world" with dw
    process_keys(&mut session, "wdw");
    assert_eq!(session.text(), "hello ");

    // Undo the edit
    process_keys(&mut session, "u");
    assert_eq!(session.text(), "hello world");

    // Shadow must match host text (no stale pre-undo text)
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        "hello world",
        "shadow should be synced to host text after undo"
    );

    // Process another key — should NOT create a drift-healing node
    let nodes_before_j = session.engine().undo_tree().node_count();
    process_keys(&mut session, "j");
    assert_eq!(
        session.engine().undo_tree().node_count(),
        nodes_before_j,
        "cursor motion after undo should not create drift node"
    );
}

#[test]
fn consecutive_undo_no_phantom_nodes() {
    let mut session = HostSession::new("aaa bbb\nccc ddd");

    // Edit 1: change first word
    process_keys(&mut session, "ciw");
    process_keys(&mut session, "xxx<Esc>");
    let nodes_after_first = session.engine().undo_tree().node_count();

    // Edit 2: insert before cursor on same line
    process_keys(&mut session, "ea");
    process_keys(&mut session, "yyy<Esc>");
    let nodes_after_second = session.engine().undo_tree().node_count();
    assert_eq!(
        nodes_after_second - nodes_after_first,
        1,
        "second edit should add exactly 1 node"
    );

    // Undo #1: should undo second edit
    process_keys(&mut session, "u");
    assert_eq!(session.text(), "xxx bbb\nccc ddd");

    // Undo #2: should undo first edit without any ghost step
    process_keys(&mut session, "u");
    assert_eq!(session.text(), "aaa bbb\nccc ddd");
}

// ─────────────────────────────────────────────────────────────────────────────
// External edit undo integration
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn external_edit_node_is_undoable() {
    // 1. Start with "hello world" and initialize the shadow document.
    let mut session = HostSession::new("hello world");
    session.set_shadow_text("hello world");
    assert_eq!(session.text(), "hello world");

    // 2. Normal edit: `wdw` — move to "world", delete word → "hello "
    process_keys(&mut session, "wdw");
    assert_eq!(session.text(), "hello ");

    // 3. External edit: insert "earth" at offset 6 (appending) → "hello earth"
    //    First, mutate the host document (apply_external_edit updates host only).
    session.apply_external_edit(6, 0, "earth");
    assert_eq!(session.text(), "hello earth");

    //    Then notify the engine so it records an undo node.
    //    ExternalEditKind::Refactor does NOT merge into the current undo group,
    //    so this creates a separate, undoable node.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(6), Offset::new(6)), // empty deleted range (pure insert)
        "earth",
        Offset::new(11), // caret after the inserted text
        ExternalEditKind::Refactor,
    );
    session.notify_external_edit_host(edit);

    // Verify the combined state is correct.
    assert_eq!(session.text(), "hello earth");

    // 4. Undo → should undo the external edit, restoring "hello "
    process_keys(&mut session, "u");
    assert_eq!(
        session.text(),
        "hello ",
        "first undo should revert the external edit"
    );

    // 5. Undo again → should undo the `dw`, restoring "hello world"
    process_keys(&mut session, "u");
    assert_eq!(
        session.text(),
        "hello world",
        "second undo should revert the dw command"
    );
}

#[test]
fn macro_with_undo_no_drift_nodes() {
    let mut session = HostSession::new("aaa bbb");
    session.set_shadow_text("aaa bbb");

    // Record macro: delete word, undo it (net no-op but exercises undo in drain)
    // qa dw u q
    process_keys(&mut session, "qa");
    process_keys(&mut session, "dw");
    process_keys(&mut session, "u");
    process_keys(&mut session, "q");
    assert_eq!(
        session.text(),
        "aaa bbb",
        "macro recording should end with original text"
    );

    // Replay the macro: @a
    // During replay, `dw` deletes "aaa " → "bbb", then `u` inside the replay
    // undoes a *prior* undo node (the undo from recording), not the in-flight
    // replay edit.  The net result is "bbb" — the `dw` effect sticks.
    process_keys(&mut session, "@a");
    let text_after_replay = session.text();

    // The key invariant: shadow must match host text after replay — no phantom
    // drift nodes from the intra-macro undo.
    assert_eq!(
        session.engine().shadow_text().unwrap(),
        text_after_replay,
        "shadow must match host text after macro replay with undo"
    );
}

#[test]
fn force_committed_insert_undo() {
    // Verify that a non-merging external edit during INSERT mode force-commits
    // the pending INSERT undo group, and all resulting undo nodes are traversable
    // without "no snapshot for node N" errors.

    // 1. Start with "ab" and initialize shadow.
    let mut session = HostSession::new("ab");
    session.set_shadow_text("ab");
    assert_eq!(session.text(), "ab");

    // 2. Enter INSERT mode at the beginning and type "XY".
    //    `i` enters insert mode; `X` and `Y` insert characters.
    process_keys(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);
    process_keys(&mut session, "XY");
    assert_eq!(session.text(), "XYab");

    // 3. While still in INSERT mode, apply a non-merging external edit
    //    (Refactor) that appends "99" at the end of the document.
    //    This should force-commit the pending INSERT group ("XY" insertion).
    //    First, mutate the host document:
    let text_len = session.text().len(); // "XYab" = 4
    session.apply_external_edit(text_len, 0, "99");
    assert_eq!(session.text(), "XYab99");

    //    Then notify the engine so it records undo nodes.
    let edit = ExternalEdit::new(
        Range::new(Offset::new(text_len), Offset::new(text_len)),
        "99",
        Offset::new(text_len + 2),
        ExternalEditKind::Refactor,
    );
    session.notify_external_edit_host(edit);
    assert_eq!(session.text(), "XYab99");
    assert_eq!(
        session.mode(),
        Mode::Insert,
        "should still be in INSERT mode after external edit"
    );

    // 4. Type more text in the INSERT continuation: "ZZ".
    process_keys(&mut session, "ZZ");
    assert_eq!(session.text(), "XYZZab99");

    // 5. Exit INSERT mode.
    process_keys(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);
    assert_eq!(session.text(), "XYZZab99");

    // 6. Undo chain: each `u` must succeed (no ghost nodes).
    //    Expected undo order (most recent first):
    //    - Continuation INSERT group: undo "ZZ" → "XYab99"
    //    - External edit node: undo "99" append → "XYab"
    //    - Force-committed INSERT group: undo "XY" → "ab"
    //
    //    We verify each step individually, but the critical invariant is
    //    that ALL undos succeed and we arrive back at the original "ab".

    let mut undo_texts = Vec::new();
    for _ in 0..3 {
        process_keys(&mut session, "u");
        undo_texts.push(session.text().to_owned());
    }

    // The final undo must restore the original text.
    assert_eq!(
        undo_texts.last().unwrap(),
        "ab",
        "all three undos should restore original text; undo sequence: {undo_texts:?}"
    );

    // Verify the intermediate steps match expected ordering.
    assert_eq!(
        undo_texts[0], "XYab99",
        "first undo should revert the INSERT continuation (ZZ)"
    );
    assert_eq!(
        undo_texts[1], "XYab",
        "second undo should revert the external edit (99)"
    );
    assert_eq!(
        undo_texts[2], "ab",
        "third undo should revert the force-committed INSERT group (XY)"
    );
}
