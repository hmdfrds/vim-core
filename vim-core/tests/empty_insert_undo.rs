//! An Insert or Replace session that changes nothing leaves no undo entry.
//!
//! Vim saves for undo only when Insert mode is about to change text
//! (edit.c `stop_arrow()` calls undo.c `u_save()` through `ins_need_undo`), so
//! `i<Esc>` adds nothing and the next `u` undoes the change before it.

use vim_test::prelude::*;

/// The `node_id` of every `EndUndoGroup` in the most recent keystroke.
fn end_group_ids(s: &TestSession) -> Vec<Option<vim_core::primitives::NodeId>> {
    s.last_effects()
        .iter()
        .filter_map(|e| match e {
            Effect::EndUndoGroup { node_id } => Some(*node_id),
            _ => None,
        })
        .collect()
}

/// Undo state that an empty session must leave untouched.
fn undo_info(s: &TestSession) -> (usize, u64, u64) {
    let tree = s.session().engine().undo_tree();
    (
        s.change_count(),
        tree.next_sequence(),
        tree.current_info().sequence(),
    )
}

/// Feeds `x`, then `session` (ending in Normal mode), then `u`, and checks
/// that the `x` is what `u` undoes.
fn assert_empty_session_leaves_no_node(session: &str) {
    let mut s = TestSession::new("|abc\ndef\n");
    s.feed("x");
    assert_eq!(s.text(), "bc\ndef\n");
    let before = undo_info(&s);

    s.feed(session);
    assert_eq!(s.mode(), Mode::Normal, "{session}");
    assert_eq!(s.text(), "bc\ndef\n", "{session}");
    assert!(!s.has_pending_group(), "{session}: group left open");
    assert_eq!(undo_info(&s), before, "{session}: undo info changed");

    s.feed("u");
    assert_eq!(
        s.text(),
        "abc\ndef\n",
        "{session}: `u` did not undo the `x`"
    );
}

/// edit.c `edit()`: `i` with nothing typed never calls `u_save()`.
#[test]
fn empty_i_esc_then_u_undoes_previous_change() {
    assert_empty_session_leaves_no_node("i<Esc>");
}

/// edit.c `edit()`: same for `a`.
#[test]
fn empty_a_esc_then_u_undoes_previous_change() {
    assert_empty_session_leaves_no_node("a<Esc>");
}

/// normal.c `nv_Replace()` enters `edit()` with `REPLACE`; nothing typed,
/// nothing saved.
#[test]
fn empty_replace_esc_then_u_undoes_previous_change() {
    assert_empty_session_leaves_no_node("R<Esc>");
}

/// edit.c `ins_ctrl_o()` runs a motion; `l` changes nothing.
#[test]
fn ctrl_o_motion_then_esc_then_u_undoes_previous_change() {
    assert_empty_session_leaves_no_node("i<C-o>l<Esc>");
}

/// edit.c `ins_bs()` beeps at line 1 column 0 before `stop_arrow()`.
#[test]
fn backspace_at_buffer_start_then_u_undoes_previous_change() {
    assert_empty_session_leaves_no_node("i<BS><Esc>");
}

/// A count repeats nothing when nothing was typed.
#[test]
fn counted_empty_insert_then_u_undoes_previous_change() {
    assert_empty_session_leaves_no_node("3i<Esc>");
}

/// edit.c `ins_ctrl_g()` `u`: `u_sync()` only; an empty sequence after it
/// adds no entry, and the typed text is one entry.
#[test]
fn ctrl_g_u_before_typing_adds_one_node() {
    let mut s = TestSession::new("|abc\n");
    s.feed("x");
    s.feed("i<C-g>uX<Esc>");
    assert_eq!(s.text(), "Xbc\n");
    assert_eq!(s.change_count(), 2);
    s.feed("u");
    assert_eq!(s.text(), "bc\n");
    s.feed("u");
    assert_eq!(s.text(), "abc\n");
}

#[test]
fn empty_insert_end_group_effect_carries_none() {
    let mut s = TestSession::new("|abc\n");
    s.feed("i");
    s.feed("<Esc>");
    assert_eq!(end_group_ids(&s), vec![None]);
}

#[test]
fn typed_insert_end_group_effect_carries_some() {
    let mut s = TestSession::new("|abc\n");
    s.feed("iX");
    s.feed("<Esc>");
    let ids = end_group_ids(&s);
    assert_eq!(ids.len(), 1);
    assert!(ids[0].is_some(), "typed Insert must commit a node: {ids:?}");
}

/// normal.c `n_opencmd()` saves for undo before `edit()`: `o<Esc>` keeps
/// its node.
#[test]
fn open_line_esc_keeps_its_node() {
    let mut s = TestSession::new("|abc\n");
    s.feed("x");
    s.feed("o");
    s.feed("<Esc>");
    let ids = end_group_ids(&s);
    assert_eq!(ids.len(), 1);
    assert!(ids[0].is_some(), "`o` changed the text: {ids:?}");
    assert_eq!(s.text(), "bc\n\n");
    assert_eq!(s.change_count(), 2);
    s.feed("u");
    assert_eq!(s.text(), "bc\n");
    s.feed("u");
    assert_eq!(s.text(), "abc\n");
}

/// ops.c `op_change()` -> `op_delete()`: an empty region still calls
/// `u_save_cursor()`, so `c0` at column 0 keeps its node.
#[test]
fn empty_change_region_keeps_its_node() {
    let mut s = TestSession::new("|abc\n");
    s.feed("x");
    s.feed("c0");
    assert_eq!(s.mode(), Mode::Insert);
    s.feed("<Esc>");
    let ids = end_group_ids(&s);
    assert_eq!(ids.len(), 1);
    assert!(ids[0].is_some(), "`c` saves for undo: {ids:?}");
    assert_eq!(s.change_count(), 2);
    s.feed("u");
    assert_eq!(s.text(), "bc\n");
    s.feed("u");
    assert_eq!(s.text(), "abc\n");
}

/// undo.c `ex_undojoin()` gives E790 while `b_u_curhead` is set; an empty
/// Insert does not clear it.
#[test]
fn empty_insert_after_undo_keeps_last_was_undo() {
    let mut s = TestSession::new("|abc\n");
    s.feed("x");
    s.feed("u");
    assert!(s.session().engine().undo_tree().last_was_undo());
    s.feed("i<Esc>");
    assert!(s.session().engine().undo_tree().last_was_undo());
}

/// edit.c `ins_bs()` calls `stop_arrow()` once past its beep checks, so a
/// Replace backspace that only moves back still makes an undo step.
#[test]
fn replace_backspace_that_moves_back_keeps_its_node() {
    let mut s = TestSession::new("|abcd\n");
    s.feed("xllR<BS><Esc>");
    assert_eq!(s.text(), "bcd\n");
    assert_eq!(s.change_count(), 2);
    s.feed("u");
    assert_eq!(s.text(), "bcd\n");
}

/// A change made through `<C-o>` keeps its node.
#[test]
fn ctrl_o_change_keeps_its_node() {
    let mut s = TestSession::new("|abc\n");
    s.feed("i<C-o>x<Esc>");
    assert_eq!(s.text(), "bc\n");
    s.feed("u");
    assert_eq!(s.text(), "abc\n");
}

#[test]
fn multi_cursor_empty_insert_then_u_undoes_previous_change() {
    let mut s = TestSession::new_multi("|1abc\n|2def\n");
    s.feed("x");
    assert_eq!(s.text(), "bc\nef\n");
    let before = undo_info(&s);
    s.feed("i");
    s.feed("<Esc>");
    assert!(
        end_group_ids(&s).iter().all(Option::is_none),
        "{:?}",
        end_group_ids(&s)
    );
    assert!(!s.has_pending_group());
    assert_eq!(undo_info(&s), before);
    s.feed("u");
    assert_eq!(s.text(), "abc\ndef\n");
}

/// `.` repeats the empty Insert, which again saves nothing.
#[test]
fn dot_after_empty_insert_then_u_undoes_previous_change() {
    assert_empty_session_leaves_no_node("i<Esc>.");
}

/// `.` after `cw<Esc>` repeats the delete: one node, closed by the `.`.
#[test]
fn dot_after_empty_change_keeps_its_node() {
    let mut s = TestSession::new("|aa bb cc\n");
    s.feed("cw<Esc>");
    assert_eq!(s.text(), " bb cc\n");
    s.feed("w.");
    assert_eq!(s.text(), "  cc\n");
    assert!(!s.has_pending_group(), "`.` left its group open");
    assert_eq!(s.change_count(), 2);
    s.feed("u");
    assert_eq!(s.text(), " bb cc\n");
    s.feed("u");
    assert_eq!(s.text(), "aa bb cc\n");
}

#[test]
fn multi_cursor_dot_after_empty_insert_then_u_undoes_previous_change() {
    let mut s = TestSession::new_multi("|1abc\n|2def\n");
    s.feed("x");
    let before = undo_info(&s);
    s.feed("i<Esc>.");
    assert_eq!(s.text(), "bc\nef\n");
    assert!(!s.has_pending_group());
    assert_eq!(undo_info(&s), before);
    s.feed("u");
    assert_eq!(s.text(), "abc\ndef\n");
}

/// Text the host typed itself still commits the Insert's node.
#[test]
fn host_recorded_insert_text_keeps_its_node() {
    let mut session = vim_core::execution::HostSession::new("abc\n");
    session.process_key_host(vim_core::keymap::KeyEvent::char('i'));
    session.record_insert_text("x");
    session.process_key_host(vim_core::keymap::KeyEvent::escape());
    assert_eq!(session.engine().undo_tree().change_count(), 1);
}
