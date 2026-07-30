use super::*;
use crate::state::mark_snapshot::MarkSnapshot;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Commit a simple group: begin → mark_edit_at(offset) → end.
fn commit_group(
    tree: &mut UndoTree,
    cursor_before: usize,
    cursor_after: usize,
    edit_offset: usize,
    timestamp: u64,
) -> NodeId {
    tree.begin_group(
        Offset::new(cursor_before),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(edit_offset));
    tree.end_group(Offset::new(cursor_after), timestamp, None)
        .expect("group should commit")
}

/// Commit a group with `UndoCursorStrategy::EntryPosition`.
fn commit_forced_group(
    tree: &mut UndoTree,
    cursor_before: usize,
    cursor_after: usize,
    edit_offset: usize,
    timestamp: u64,
) -> NodeId {
    tree.begin_group(
        Offset::new(cursor_before),
        crate::primitives::UndoCursorStrategy::EntryPosition,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(edit_offset));
    tree.end_group(Offset::new(cursor_after), timestamp, None)
        .expect("group should commit")
}

/// Create a dummy Marks for undo/redo calls (tests that don't care about marks).
fn dummy_marks() -> Marks {
    Marks::new()
}

/// Undo N times on the tree, returning the last step.
fn undo_n(tree: &mut UndoTree, marks: &mut Marks, n: u32) -> Option<UndoStep> {
    let mut last = None;
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    for _ in 0..n {
        match tree.undo(marks, &mut _lv) {
            Some(step) => last = Some(step),
            None => break,
        }
    }
    last
}

/// Redo N times on the tree, returning the last step.
fn redo_n(tree: &mut UndoTree, marks: &mut Marks, n: u32) -> Option<UndoStep> {
    let mut last = None;
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    for _ in 0..n {
        match tree.redo(marks, &mut _lv) {
            Some(step) => last = Some(step),
            None => break,
        }
    }
    last
}

// ═══════════════════════════════════════════════════════════════════════════
// Basic construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn new_tree_at_root() {
    let tree = UndoTree::new();
    assert_eq!(tree.current(), NodeId::ROOT);
    assert_eq!(tree.node_count(), 1);
    assert_eq!(tree.change_count(), 0);
    assert_eq!(tree.depth(), 0);
    assert_eq!(tree.branch_count(), 0);
    assert!(!tree.can_undo());
    assert!(!tree.can_redo());
    assert!(!tree.has_pending_group());
}

#[test]
fn default_is_same_as_new() {
    let a = UndoTree::new();
    let b = UndoTree::default();
    assert_eq!(a.node_count(), b.node_count());
    assert_eq!(a.current(), b.current());
}

#[test]
fn root_node_info() {
    let tree = UndoTree::new();
    let info = tree.node_info(NodeId::ROOT).unwrap();
    assert_eq!(info.id, NodeId::ROOT);
    assert_eq!(info.parent, None);
    assert_eq!(info.child_count, 0);
    assert_eq!(info.sequence, 0);
    assert_eq!(info.depth, 0);
    assert_eq!(info.cursor_before, Offset::ZERO);
    assert_eq!(info.cursor_after, Offset::ZERO);
    assert_eq!(info.first_edit_offset, None);
    assert_eq!(info.timestamp, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Group lifecycle
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn single_group_creates_node() {
    let mut tree = UndoTree::new();
    let id = commit_group(&mut tree, 10, 15, 10, 100);

    assert_eq!(id, NodeId::new(1));
    assert_eq!(tree.current(), id);
    assert_eq!(tree.node_count(), 2);
    assert_eq!(tree.change_count(), 1);
    assert_eq!(tree.depth(), 1);
    assert!(tree.can_undo());
    assert!(!tree.can_redo());
}

#[test]
fn empty_group_still_recorded() {
    let mut tree = UndoTree::new();

    // Neovim records undo groups even without edits (cursor-only operations).
    tree.begin_group(
        Offset::new(10),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    let result = tree.end_group(Offset::new(15), 100, None);
    assert!(
        result.is_some(),
        "empty group should be recorded (Neovim parity)"
    );
    assert_ne!(tree.current(), NodeId::ROOT);
    assert_eq!(tree.node_count(), 2); // Root + new node
}

#[test]
fn has_pending_group_lifecycle() {
    let mut tree = UndoTree::new();
    assert!(!tree.has_pending_group());

    tree.begin_group(
        Offset::ZERO,
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    assert!(tree.has_pending_group());

    tree.mark_edit();
    assert!(tree.has_pending_group());

    tree.end_group(Offset::new(5), 100, None);
    assert!(!tree.has_pending_group());
}

#[test]
fn end_group_without_begin_returns_none() {
    let mut tree = UndoTree::new();
    assert_eq!(tree.end_group(Offset::new(10), 100, None), None);
}

#[test]
fn mark_edit_without_offset() {
    let mut tree = UndoTree::new();
    tree.begin_group(
        Offset::ZERO,
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit(); // No offset
    let id = tree
        .end_group(Offset::new(5), 100, None)
        .expect("group should commit");

    let info = tree.node_info(id).unwrap();
    assert_eq!(info.first_edit_offset, None);
}

#[test]
fn mark_edit_at_tracks_minimum() {
    let mut tree = UndoTree::new();
    tree.begin_group(
        Offset::ZERO,
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(20));
    tree.mark_edit_at(Offset::new(5));
    tree.mark_edit_at(Offset::new(15));
    let id = tree
        .end_group(Offset::new(25), 100, None)
        .expect("group should commit");

    let info = tree.node_info(id).unwrap();
    assert_eq!(info.first_edit_offset, Some(Offset::new(5)));
}

#[test]
fn node_info_accuracy() {
    let mut tree = UndoTree::new();
    let id = commit_group(&mut tree, 10, 25, 12, 500);

    let info = tree.node_info(id).unwrap();
    assert_eq!(info.id, id);
    assert_eq!(info.parent, Some(NodeId::ROOT));
    assert_eq!(info.child_count, 0);
    assert_eq!(info.sequence, 1);
    assert_eq!(info.cursor_before, Offset::new(10));
    assert_eq!(info.cursor_after, Offset::new(25));
    assert_eq!(info.first_edit_offset, Some(Offset::new(12)));
    assert_eq!(info.timestamp, 500);
    assert_eq!(info.depth, 1);
}

#[test]
fn current_info_matches_node_info() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 100);
    assert_eq!(tree.current_info(), tree.node_info(tree.current()).unwrap());
}

#[test]
fn node_info_invalid_id_returns_none() {
    let tree = UndoTree::new();
    assert_eq!(tree.node_info(NodeId::new(999)), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Linear undo/redo
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn undo_returns_to_parent() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 10, 15, 10, 100);

    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, NodeId::ROOT);
    assert_eq!(tree.current(), NodeId::ROOT);
    assert!(!tree.can_undo());
    assert!(tree.can_redo());
}

#[test]
fn redo_after_undo() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let id = commit_group(&mut tree, 10, 15, 10, 100);

    tree.undo(&mut marks, &mut _lv);
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, id);
    assert_eq!(tree.current(), id);
    assert!(tree.can_undo());
    assert!(!tree.can_redo());
}

#[test]
fn undo_at_root_is_none() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    assert_eq!(tree.undo(&mut marks, &mut _lv), None);
}

#[test]
fn redo_at_leaf_is_none() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 100);
    assert_eq!(tree.redo(&mut marks, &mut _lv), None);
}

#[test]
fn redo_at_root_no_children_is_none() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    assert_eq!(tree.redo(&mut marks, &mut _lv), None);
}

#[test]
fn multiple_groups_linear() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let id1 = commit_group(&mut tree, 0, 5, 0, 100);
    let id2 = commit_group(&mut tree, 5, 10, 5, 200);
    let id3 = commit_group(&mut tree, 10, 15, 10, 300);

    assert_eq!(tree.depth(), 3);
    assert_eq!(tree.change_count(), 3);

    // Undo all the way to root
    let s3 = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(s3.node, id2);
    let s2 = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(s2.node, id1);
    let s1 = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(s1.node, NodeId::ROOT);
    assert_eq!(tree.undo(&mut marks, &mut _lv), None);

    // Redo all the way back
    let r1 = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(r1.node, id1);
    let r2 = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(r2.node, id2);
    let r3 = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(r3.node, id3);
    assert_eq!(tree.redo(&mut marks, &mut _lv), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cursor placement
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cursor_on_undo_uses_first_edit_offset() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // cursor_before=10, edit_offset=5 (edit was before cursor)
    commit_group(&mut tree, 10, 15, 5, 100);

    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(
        step.cursor(),
        Offset::new(5),
        "undo cursor should use first_edit_offset"
    );
}

#[test]
fn cursor_on_undo_falls_back_to_cursor_before() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // No first_edit_offset (mark_edit without offset)
    tree.begin_group(
        Offset::new(10),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit();
    tree.end_group(Offset::new(15), 100, None);

    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(
        step.cursor(),
        Offset::new(10),
        "undo cursor should fall back to cursor_before"
    );
}

#[test]
fn cursor_on_undo_entry_position_ignores_edit_offset() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // EntryPosition strategy, edit at offset 5, cursor_before=10
    commit_forced_group(&mut tree, 10, 15, 5, 100);

    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(
        step.cursor(),
        Offset::new(10),
        "EntryPosition strategy should use cursor_before, not edit_offset"
    );
}

#[test]
fn cursor_on_redo_uses_cursor_before() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 10, 25, 10, 100);

    tree.undo(&mut marks, &mut _lv);
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    // Neovim uses uh_cursor (cursor at change start) for both undo and redo.
    // With FirstEdit strategy and edit_offset=10, redo cursor = first_edit_offset.
    assert_eq!(
        step.cursor(),
        Offset::new(10),
        "redo cursor should use cursor_before/first_edit_offset (matching Neovim's uh_cursor)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Branching
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn branch_on_edit_after_undo() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    //   Root → A → B
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let _b = commit_group(&mut tree, 5, 10, 5, 200);

    // Undo to A, then make new edit C
    tree.undo(&mut marks, &mut _lv); // at A
    let c = commit_group(&mut tree, 5, 20, 5, 300);

    //   Root → A → { B, C }
    assert_eq!(tree.current(), c);
    assert_eq!(tree.depth(), 2);

    // A should now have 2 children
    let a_info = tree.node_info(NodeId::new(1)).unwrap();
    assert_eq!(a_info.child_count, 2);
}

#[test]
fn redo_follows_latest_branch() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let _b = commit_group(&mut tree, 5, 10, 5, 200);

    // Undo to A, create branch C
    tree.undo(&mut marks, &mut _lv); // at A
    let c = commit_group(&mut tree, 5, 20, 5, 300);

    // Undo to A, redo should go to C (latest, preferred)
    tree.undo(&mut marks, &mut _lv); // at A
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(
        step.node, c,
        "redo should follow the latest branch (C, not B)"
    );

    // Now undo to A, undo to root, redo to A, redo should still go to C
    tree.undo(&mut marks, &mut _lv); // at A
    tree.undo(&mut marks, &mut _lv); // at root
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, a);
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, c, "redo should still follow C");
}

#[test]
fn undo_sets_preferred_child() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);

    // Undo to A, then undo to root
    tree.undo(&mut marks, &mut _lv); // at A, parent's preferred = A
    tree.undo(&mut marks, &mut _lv); // at root, root's preferred child = A

    // Redo from root should go to A
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, a);

    // Redo from A should go to B (B was the node we undid from)
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, b);
}

#[test]
fn three_branches_preferred_follows_latest_visited() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let a = commit_group(&mut tree, 0, 5, 0, 100);

    // Branch B from A
    let _b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // back to A

    // Branch C from A
    let _c = commit_group(&mut tree, 5, 15, 5, 300);
    tree.undo(&mut marks, &mut _lv); // back to A

    // Branch D from A
    let d = commit_group(&mut tree, 5, 20, 5, 400);
    tree.undo(&mut marks, &mut _lv); // back to A

    // Redo from A should go to D (last created = last preferred)
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, d);

    // A should have 3 children
    let a_info = tree.node_info(a).unwrap();
    assert_eq!(a_info.child_count, 3);
}

#[test]
fn branch_count_at_current() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    assert_eq!(tree.branch_count(), 0); // root, no children

    commit_group(&mut tree, 0, 5, 0, 100);
    assert_eq!(tree.branch_count(), 0); // at node 1, which is a leaf

    tree.undo(&mut marks, &mut _lv); // at root
    assert_eq!(tree.branch_count(), 1); // root has 1 child
}

#[test]
fn depth_tracking_across_branches() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    assert_eq!(tree.depth(), 0);

    commit_group(&mut tree, 0, 5, 0, 100);
    assert_eq!(tree.depth(), 1);

    commit_group(&mut tree, 5, 10, 5, 200);
    assert_eq!(tree.depth(), 2);

    tree.undo(&mut marks, &mut _lv); // depth 1
    assert_eq!(tree.depth(), 1);

    commit_group(&mut tree, 5, 15, 5, 300); // new branch at depth 2
    assert_eq!(tree.depth(), 2);

    tree.undo(&mut marks, &mut _lv); // depth 1
    tree.undo(&mut marks, &mut _lv); // depth 0 (root)
    assert_eq!(tree.depth(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Count-based navigation (:earlier N / :later N)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn earlier_by_count_basic() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 100);
    commit_group(&mut tree, 5, 10, 5, 200);
    commit_group(&mut tree, 10, 15, 10, 300);

    let (count, cursor) = tree.earlier_by_count(2).unwrap();
    assert_eq!(count, 2);
    // Peek-only: tree has NOT moved yet
    assert_eq!(tree.depth(), 3);
    // cursor comes from probe which uses first_edit_offset
    assert_eq!(cursor, Offset::new(5)); // first_edit_offset of node 2

    // Perform the navigation
    undo_n(&mut tree, &mut marks, 2);
    assert_eq!(tree.depth(), 1);
}

#[test]
fn earlier_by_count_partial() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 100);
    commit_group(&mut tree, 5, 10, 5, 200);

    // Ask for 5 undos but only 2 available
    let (count, _cursor) = tree.earlier_by_count(5).unwrap();
    assert_eq!(count, 2);
    // Peek-only: tree has NOT moved
    assert_eq!(tree.depth(), 2);

    // Perform the navigation
    undo_n(&mut tree, &mut marks, count);
    assert_eq!(tree.current(), NodeId::ROOT);
}

#[test]
fn earlier_by_count_at_root_is_none() {
    let tree = UndoTree::new();
    assert_eq!(tree.earlier_by_count(1), None);
}

#[test]
fn later_by_count_basic() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let _b = commit_group(&mut tree, 5, 10, 5, 200);
    let c = commit_group(&mut tree, 10, 15, 10, 300);

    // Navigate to root first
    undo_n(&mut tree, &mut marks, 3);
    assert_eq!(tree.current(), NodeId::ROOT);

    let (count, cursor) = tree.later_by_count(2).unwrap();
    assert_eq!(count, 2);
    // Peek-only: tree has NOT moved
    assert_eq!(tree.depth(), 0);
    assert_eq!(cursor, Offset::new(5)); // first_edit_offset of node B (Neovim places cursor at start of change)

    // Perform the navigation
    redo_n(&mut tree, &mut marks, 2);
    assert_eq!(tree.depth(), 2);
    assert_ne!(tree.current(), c); // at B, not C
}

#[test]
fn later_by_count_partial() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 100);
    commit_group(&mut tree, 5, 10, 5, 200);

    // Navigate to root
    undo_n(&mut tree, &mut marks, 2);

    // Ask for 5 redos but only 2 available
    let (count, _cursor) = tree.later_by_count(5).unwrap();
    assert_eq!(count, 2);
    // Peek-only: tree has NOT moved
    assert_eq!(tree.depth(), 0);

    // Perform the navigation
    redo_n(&mut tree, &mut marks, count);
    assert_eq!(tree.depth(), 2);
}

#[test]
fn later_by_count_at_leaf_is_none() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 100);
    assert_eq!(tree.later_by_count(1), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Time-based navigation (:earlier Ns / :later Ns)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn earlier_by_time_basic() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 10);
    commit_group(&mut tree, 5, 10, 5, 20);
    commit_group(&mut tree, 10, 15, 10, 30);

    // current_time=35, go back 10s → target=25 → undo past node@30
    let (count, _cursor) = tree.earlier_by_time(10, 35).unwrap();
    assert_eq!(count, 1);
    // Peek-only: tree has NOT moved
    assert_eq!(tree.depth(), 3);

    // Perform the navigation
    undo_n(&mut tree, &mut marks, count);
    assert_eq!(tree.depth(), 2); // at node 2 (timestamp=20 ≤ 25)
}

#[test]
fn earlier_by_time_multiple_undos() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 10);
    commit_group(&mut tree, 5, 10, 5, 20);
    commit_group(&mut tree, 10, 15, 10, 30);

    // current_time=35, go back 30s → target=5 → undo past all 3 nodes
    let (count, _cursor) = tree.earlier_by_time(30, 35).unwrap();
    assert_eq!(count, 3);

    // Perform the navigation
    undo_n(&mut tree, &mut marks, count);
    assert_eq!(tree.current(), NodeId::ROOT);
}

#[test]
fn earlier_by_time_already_past_target() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 10);

    // current_time=15, go back 2s → target=13 → node@10 ≤ 13, no undo
    assert_eq!(tree.earlier_by_time(2, 15), None);
}

#[test]
fn later_by_time_basic() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 10);
    commit_group(&mut tree, 5, 10, 5, 20);
    commit_group(&mut tree, 10, 15, 10, 30);

    // Navigate to root first
    undo_n(&mut tree, &mut marks, 3);
    assert_eq!(tree.current(), NodeId::ROOT);

    // current_time=5, go forward 10s → target=15 → redo node@10 only
    let (count, _cursor) = tree.later_by_time(10).unwrap();
    assert_eq!(count, 1);
    // Peek-only: tree has NOT moved
    assert_eq!(tree.depth(), 0);

    // Perform the navigation
    redo_n(&mut tree, &mut marks, count);
    assert_eq!(tree.depth(), 1); // at node 1 (timestamp=10 ≤ 15)
}

#[test]
fn later_by_time_already_past_target() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 10);
    commit_group(&mut tree, 5, 10, 5, 20);

    tree.undo(&mut marks, &mut _lv); // at node 1 (timestamp=10)

    // current_time=5, go forward 3s → target=8 → next child@20 > 8, no redo
    assert_eq!(tree.later_by_time(3), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Leaf enumeration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn leaves_empty_tree() {
    let tree = UndoTree::new();
    let leaves = tree.leaves();
    assert_eq!(leaves.len(), 1); // root is the only leaf
    assert_eq!(leaves[0].node, NodeId::ROOT);
    assert_eq!(leaves[0].depth, 0);
}

#[test]
fn leaves_linear() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 100);
    commit_group(&mut tree, 5, 10, 5, 200);
    let c = commit_group(&mut tree, 10, 15, 10, 300);

    let leaves = tree.leaves();
    assert_eq!(leaves.len(), 1);
    assert_eq!(leaves[0].node, c);
    assert_eq!(leaves[0].depth, 3);
    assert_eq!(leaves[0].sequence, 3);
    assert_eq!(leaves[0].timestamp, 300);
}

#[test]
fn leaves_with_branches() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    //   Root → A → B
    //              → C → D
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // at A
    commit_group(&mut tree, 5, 15, 5, 300); // C
    let d = commit_group(&mut tree, 15, 20, 15, 400); // D

    let leaves = tree.leaves();
    assert_eq!(leaves.len(), 2);

    // Sorted by sequence: B(seq=2), D(seq=4)
    assert_eq!(leaves[0].node, b);
    assert_eq!(leaves[0].sequence, 2);
    assert_eq!(leaves[1].node, d);
    assert_eq!(leaves[1].sequence, 4);
}

#[test]
fn leaves_three_branches() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    //   Root → A → B1
    //              → B2
    //              → B3
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let b1 = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // at A
    let b2 = commit_group(&mut tree, 5, 15, 5, 300);
    tree.undo(&mut marks, &mut _lv); // at A
    let b3 = commit_group(&mut tree, 5, 20, 5, 400);

    let leaves = tree.leaves();
    assert_eq!(leaves.len(), 3);
    assert_eq!(leaves[0].node, b1);
    assert_eq!(leaves[1].node, b2);
    assert_eq!(leaves[2].node, b3);
}

// ═══════════════════════════════════════════════════════════════════════════
// NodeId
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn node_id_display() {
    assert_eq!(format!("{}", NodeId::ROOT), "0");
    assert_eq!(format!("{}", NodeId::new(42)), "42");
}

#[test]
fn node_id_ordering() {
    assert!(NodeId::new(1) > NodeId::ROOT);
    assert!(NodeId::new(5) < NodeId::new(10));
}

#[test]
fn node_id_root_constant() {
    assert_eq!(NodeId::ROOT.index(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge cases and stress
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn begin_group_replaces_pending() {
    let mut tree = UndoTree::new();
    tree.begin_group(
        Offset::new(10),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(10));
    // Begin again without ending — replaces the pending group
    tree.begin_group(
        Offset::new(20),
        crate::primitives::UndoCursorStrategy::EntryPosition,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(20));
    let id = tree.end_group(Offset::new(25), 100, None).unwrap();

    let info = tree.node_info(id).unwrap();
    assert_eq!(
        info.cursor_before,
        Offset::new(20),
        "should use second begin_group's cursor"
    );
}

#[test]
fn undo_redo_roundtrip_preserves_tree() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);

    // Undo all, redo all, check state is same
    tree.undo(&mut marks, &mut _lv);
    tree.undo(&mut marks, &mut _lv);
    tree.redo(&mut marks, &mut _lv);
    tree.redo(&mut marks, &mut _lv);

    assert_eq!(tree.current(), b);
    assert_eq!(tree.node_count(), 3); // root + 2 nodes

    // Check parent's preferred child is consistent
    tree.undo(&mut marks, &mut _lv);
    assert_eq!(tree.current(), a);
    tree.redo(&mut marks, &mut _lv);
    assert_eq!(tree.current(), b);
}

#[test]
fn branch_after_deep_undo() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // Create chain: Root → 1 → 2 → 3 → 4
    for i in 0u64..4 {
        let offset = i as usize * 5;
        commit_group(&mut tree, offset, offset + 5, offset, (i + 1) * 100);
    }
    assert_eq!(tree.depth(), 4);

    // Undo to node 2 (depth=2)
    tree.undo(&mut marks, &mut _lv); // at 3
    tree.undo(&mut marks, &mut _lv); // at 2

    // Branch from node 2
    let branch = commit_group(&mut tree, 10, 30, 10, 500);
    assert_eq!(tree.depth(), 3);

    // Undo to node 2, redo should go to branch (not old node 3)
    tree.undo(&mut marks, &mut _lv);
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, branch);
}

#[test]
fn large_tree_stress() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create 200 linear changes
    for i in 0u64..200 {
        commit_group(&mut tree, i as usize, (i + 1) as usize, i as usize, i * 10);
    }
    assert_eq!(tree.node_count(), 201);
    assert_eq!(tree.change_count(), 200);
    assert_eq!(tree.depth(), 200);

    // Peek 100 undos
    let (count, _) = tree.earlier_by_count(100).unwrap();
    assert_eq!(count, 100);

    // Perform the navigation
    undo_n(&mut tree, &mut marks, 100);
    assert_eq!(tree.depth(), 100);

    // Create branch
    let _branch = commit_group(&mut tree, 100, 300, 100, 5000);
    assert_eq!(tree.depth(), 101);

    // Verify leaves: original leaf (node 200) + branch leaf (node 201)
    let leaves = tree.leaves();
    assert_eq!(leaves.len(), 2);

    // Can redo back to branch
    tree.undo(&mut marks, &mut _lv);
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node, NodeId::new(201));
}

#[test]
fn earlier_later_roundtrip_with_time() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 10);
    commit_group(&mut tree, 5, 10, 5, 20);
    commit_group(&mut tree, 10, 15, 10, 30);
    commit_group(&mut tree, 15, 20, 15, 40);

    // At node 4 (timestamp=40). Go back 15s → target=25 → undo past 30, 40
    let (undo_count, _) = tree.earlier_by_time(15, 40).unwrap();
    assert_eq!(undo_count, 2);

    // Perform the navigation
    undo_n(&mut tree, &mut marks, undo_count);
    assert_eq!(tree.depth(), 2); // at node 2 (timestamp=20 ≤ 25)

    // Now go forward 15s → target=35 → redo node@30 only (40 > 35)
    let (redo_count, _) = tree.later_by_time(15).unwrap();
    assert_eq!(redo_count, 1);

    // Perform the navigation
    redo_n(&mut tree, &mut marks, redo_count);
    assert_eq!(tree.depth(), 3); // at node 3 (timestamp=30 ≤ 35)
}

#[test]
fn earlier_by_time_zero_seconds() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 10);

    // Go back 0 seconds from time 10 → target=10, node@10 ≤ 10, no undo
    assert_eq!(tree.earlier_by_time(0, 10), None);
}

#[test]
fn earlier_by_count_zero() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 100);
    assert_eq!(tree.earlier_by_count(0), None);
}

#[test]
fn later_by_count_zero() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 100);
    tree.undo(&mut marks, &mut _lv);
    assert_eq!(tree.later_by_count(0), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// Pruning (undolevels)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn prune_noop_when_below_limit() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 100);
    commit_group(&mut tree, 5, 10, 5, 200);
    commit_group(&mut tree, 10, 15, 10, 300);

    // With 3 changes and max=5, nothing should be pruned.
    tree.prune(5);
    assert_eq!(tree.change_count(), 3);
    assert_eq!(tree.node_count(), 4); // root + 3
}

#[test]
fn prune_noop_when_exactly_at_limit() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 100);
    commit_group(&mut tree, 5, 10, 5, 200);
    commit_group(&mut tree, 10, 15, 10, 300);

    tree.prune(3);
    assert_eq!(tree.change_count(), 3);
}

#[test]
fn prune_reduces_live_count_to_max() {
    // Linear chain of 5, then undo to root and create 5 more branches.
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create main branch: root → a1 → a2 → a3 → a4 → a5 (current)
    for i in 0u64..5 {
        commit_group(
            &mut tree,
            i as usize,
            (i + 1) as usize,
            i as usize,
            (i + 1) * 10,
        );
    }
    // Undo all the way to root
    undo_n(&mut tree, &mut marks, 5);
    // Create 5 branch leaves from root
    for i in 0u64..5 {
        commit_group(&mut tree, 0, (100 + i) as usize, 0, 100 + i * 10);
        tree.undo(&mut marks, &mut _lv); // back to root
    }

    // Current is at root. Live count = 10.
    // Ancestor path = just root. All 10 changes are prunable.
    assert_eq!(tree.change_count(), 10);

    tree.prune(5);
    assert_eq!(tree.change_count(), 5);
    // Arena size is still 11 (root + 10 nodes, tombstoned but not freed)
    assert_eq!(tree.node_count(), 11);
}

#[test]
fn prune_never_prunes_root() {
    let mut tree = UndoTree::new();
    commit_group(&mut tree, 0, 5, 0, 100);
    commit_group(&mut tree, 5, 10, 5, 200);

    tree.prune(0);
    // Root must always survive
    assert!(tree.node_info(NodeId::ROOT).is_some());
}

#[test]
fn prune_never_prunes_current_path() {
    let mut tree = UndoTree::new();
    let id1 = commit_group(&mut tree, 0, 5, 0, 100);
    let id2 = commit_group(&mut tree, 5, 10, 5, 200);
    let id3 = commit_group(&mut tree, 10, 15, 10, 300);

    // Current is at id3. Prune aggressively to 0.
    tree.prune(0);

    // All ancestors of current (root, id1, id2, id3) must survive.
    assert!(tree.node_info(NodeId::ROOT).is_some());
    assert!(tree.node_info(id1).is_some());
    assert!(tree.node_info(id2).is_some());
    assert!(tree.node_info(id3).is_some());
}

#[test]
fn prune_with_zero_limit_leaves_only_current_path() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // Root → A → B
    //            → C
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let _b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // back to A
    let c = commit_group(&mut tree, 5, 15, 5, 300); // current = C

    // C is at depth 2, current path is root→A→C.
    // B is NOT on the current path and should be pruned.
    tree.prune(0);

    // C and its ancestors survive
    assert!(tree.node_info(c).is_some());
    // B is pruned
    assert!(tree.node_info(NodeId::new(2)).is_none());
}

#[test]
fn prune_prunes_oldest_sequence_first() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // Root → A (seq=1) → B (seq=2)
    //        A → C (seq=3)  [branch]
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // back to A
    let c = commit_group(&mut tree, 5, 15, 5, 300); // current

    // 3 live changes (a, b, c). Prune to 2.
    // B has the oldest sequence on a non-ancestor branch → pruned first.
    tree.prune(2);
    assert_eq!(tree.change_count(), 2);

    // B (seq=2) should be pruned, A and C survive
    assert!(
        tree.node_info(b).is_none(),
        "B (oldest non-ancestor leaf) should be pruned"
    );
    assert!(
        tree.node_info(a).is_some(),
        "A is ancestor of current, must survive"
    );
    assert!(tree.node_info(c).is_some(), "C is current, must survive");
}

#[test]
fn prune_cascades_to_parent_when_all_children_pruned() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // Root → A → B → C (leaf, non-ancestor)
    //                    [current stays at root after undo]
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    let c = commit_group(&mut tree, 10, 15, 10, 300);

    // Undo all the way to root so current = root (A, B, C are all non-ancestors)
    undo_n(&mut tree, &mut marks, 3);
    assert_eq!(tree.current(), NodeId::ROOT);

    // Prune to 0: all of A, B, C should be pruned (cascade from C up through B to A)
    tree.prune(0);
    assert_eq!(tree.change_count(), 0);
    assert!(tree.node_info(c).is_none(), "C should be pruned");
    assert!(tree.node_info(b).is_none(), "B should be cascade-pruned");
    assert!(tree.node_info(a).is_none(), "A should be cascade-pruned");
    // Root is never pruned
    assert!(tree.node_info(NodeId::ROOT).is_some());
}

#[test]
fn prune_cascade_stops_at_nodes_with_live_children() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // Root → A → B (leaf, seq=2)
    //            → C → D (leaf, seq=4) [current]
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // back to A
    let _c = commit_group(&mut tree, 5, 15, 5, 300);
    let d = commit_group(&mut tree, 15, 20, 15, 400); // current

    // 4 live changes. Prune to 3 — should prune B (oldest non-ancestor leaf).
    // A still has C as live child, so cascade stops.
    tree.prune(3);
    assert_eq!(tree.change_count(), 3);
    assert!(tree.node_info(b).is_none(), "B should be pruned");
    // A must survive (it has live child C)
    assert!(
        tree.node_info(NodeId::new(1)).is_some(),
        "A must not be cascade-pruned"
    );
    assert!(tree.node_info(d).is_some(), "D (current) must survive");
}

#[test]
fn change_count_reflects_live_count_after_pruning() {
    // Create 10 branches from root so all are prunable.
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    for i in 0u64..10 {
        commit_group(&mut tree, 0, (i + 1) as usize, 0, (i + 1) * 10);
        tree.undo(&mut marks, &mut _lv); // back to root
    }
    assert_eq!(tree.change_count(), 10);
    // current = root, all branches prunable
    tree.prune(7);
    assert_eq!(tree.change_count(), 7);
    tree.prune(3);
    assert_eq!(tree.change_count(), 3);
}

#[test]
fn snapshot_excludes_pruned_nodes() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // back to A
    let _c = commit_group(&mut tree, 5, 15, 5, 300); // current

    // 3 live changes. Prune B (oldest non-ancestor leaf).
    tree.prune(2);

    let snap = tree.snapshot();
    // B should not appear in the snapshot
    let b_in_snap = snap.nodes.iter().any(|n| n.id == b);
    assert!(!b_in_snap, "Pruned node B should be absent from snapshot");
    // Root, A, C must appear
    assert_eq!(snap.nodes.len(), 3, "root + A + C = 3 live nodes");
    assert_eq!(snap.change_count, 2);
}

#[test]
fn leaves_excludes_pruned_nodes() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // back to A
    let c = commit_group(&mut tree, 5, 15, 5, 300); // current

    // Before pruning: two leaves (b, c)
    let leaves_before = tree.leaves();
    assert_eq!(leaves_before.len(), 2);

    // Prune B
    tree.prune(2);

    // After pruning: only C is a live leaf
    let leaves_after = tree.leaves();
    assert_eq!(leaves_after.len(), 1);
    assert_eq!(leaves_after[0].node, c);

    // B should not be in leaves
    let b_in_leaves = leaves_after.iter().any(|l| l.node == b);
    assert!(!b_in_leaves, "Pruned node B should not appear in leaves");
}

#[test]
fn node_info_returns_none_for_pruned_node() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // back to A
    let _c = commit_group(&mut tree, 5, 15, 5, 300); // current

    tree.prune(2);

    // B is pruned — node_info should return None
    assert_eq!(
        tree.node_info(b),
        None,
        "node_info should be None for pruned node"
    );
}

#[test]
fn redo_after_prune_finds_live_child() {
    // After pruning the oldest branch from root, redo to the surviving branch works.
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let a = commit_group(&mut tree, 0, 5, 0, 100); // seq=1, root.pref=0(A)
    tree.undo(&mut marks, &mut _lv);
    let b = commit_group(&mut tree, 0, 10, 0, 200); // seq=2, root.pref=1(B)
    tree.undo(&mut marks, &mut _lv); // root.pref=1(B)

    // 2 live changes. Prune oldest (A, seq=1). root.pref=1(B, still live).
    tree.prune(1);
    assert!(tree.node_info(a).is_none(), "A should be pruned");
    assert!(tree.node_info(b).is_some(), "B should survive");

    // Redo from root: preferred index 1 (B) is live.
    let step = tree.redo(&mut marks, &mut _lv).expect("redo to B");
    assert_eq!(step.node, b);
}

#[test]
fn redo_after_prune_with_branched_children() {
    // Verifies redo works correctly after pruning removes one branch sibling.
    // The positive skip case (preferred child pruned, fallback to live sibling)
    // is a defensive guard unreachable via the public API, since preferred_child_idx
    // always tracks the latest committed/visited child.
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let _a = commit_group(&mut tree, 0, 5, 0, 10);
    let b = commit_group(&mut tree, 5, 10, 5, 20); // A.pref=0=B
    tree.undo(&mut marks, &mut _lv); // at A, A.pref=0=B
    let c = commit_group(&mut tree, 5, 15, 5, 30); // A.pref=1=C
    tree.undo(&mut marks, &mut _lv); // at A, A.pref=1=C

    // live_count = 3 (A, B, C are all non-root live nodes). prune(2) prunes 1 node
    // (B, oldest non-ancestor leaf), leaving live_count=2=max_levels. C survives.
    // A.pref=1=C (set by the undo-from-C call above), so redo goes directly to C.
    tree.prune(2);
    assert!(tree.node_info(b).is_none(), "B should be pruned");
    assert!(tree.node_info(c).is_some(), "C should survive");

    // Redo from A: preferred=1(C, live) → no skip needed, but code path is hit.
    let step = tree.redo(&mut marks, &mut _lv).expect("redo from A to C");
    assert_eq!(step.node, c, "redo should find C");
}

#[test]
fn can_redo_false_when_all_children_pruned() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    commit_group(&mut tree, 0, 5, 0, 100);
    tree.undo(&mut marks, &mut _lv); // back to root

    // Prune the only child (undolevels=0)
    tree.prune(0);

    // can_redo should be false since the only child is pruned
    assert!(
        !tree.can_redo(),
        "can_redo should be false when all children pruned"
    );
}

#[test]
fn prune_linear_chain_from_undo_position() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // Create 5 linear changes, then undo 2 steps so current = node 3
    let _n1 = commit_group(&mut tree, 0, 1, 0, 10);
    let _n2 = commit_group(&mut tree, 1, 2, 1, 20);
    let n3 = commit_group(&mut tree, 2, 3, 2, 30);
    let n4 = commit_group(&mut tree, 3, 4, 3, 40);
    let n5 = commit_group(&mut tree, 4, 5, 4, 50);

    tree.undo(&mut marks, &mut _lv); // at n4
    tree.undo(&mut marks, &mut _lv); // at n3

    assert_eq!(tree.current(), n3);

    // Prune to 2 levels. Ancestor path is root→n1→n2→n3 (all protected).
    // Non-ancestor nodes: n4 (seq=4, leaf), n5 (seq=5, leaf).
    // We have 5 live, want 2 → prune 3 nodes. But n1,n2,n3 are ancestors,
    // so we can only prune n4 and n5 (2 nodes). live_count becomes 3.
    tree.prune(2);

    // Only 3 nodes can survive (n1, n2, n3 are all ancestors)
    // n4, n5 are pruned
    assert!(tree.node_info(n4).is_none(), "n4 should be pruned");
    assert!(tree.node_info(n5).is_none(), "n5 should be pruned");
    // live_count is capped by ancestor protection (cannot go below ancestor count)
    assert!(
        tree.change_count() >= 3,
        "ancestors are protected, minimum 3 live nodes"
    );
}

#[test]
fn undolevels_none_means_unlimited() {
    // VimOptions::undolevels() returns None by default → no pruning
    let opts = crate::primitives::VimOptions::default();
    assert_eq!(
        opts.undolevels(),
        None,
        "default undolevels should be None (unlimited)"
    );
}

#[test]
fn undolevels_setter_and_getter() {
    let mut opts = crate::primitives::VimOptions::default();
    opts.set_undolevels(Some(100));
    assert_eq!(opts.undolevels(), Some(100));
    opts.set_undolevels(None);
    assert_eq!(opts.undolevels(), None);
    opts.set_undolevels(Some(0));
    assert_eq!(opts.undolevels(), Some(0));
}

#[test]
fn prune_with_branches_prunes_oldest_branch_leaves_first() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    // Root → A (seq=1)
    //      → A → B (seq=2) [branch 1 leaf]
    //      → A → C (seq=3) [branch 2 leaf, current]
    //      → A → D (seq=4) [branch 3 leaf]
    let _a = commit_group(&mut tree, 0, 5, 0, 10);
    let b = commit_group(&mut tree, 5, 10, 5, 20);
    tree.undo(&mut marks, &mut _lv); // back to A
    let c = commit_group(&mut tree, 5, 15, 5, 30);
    tree.undo(&mut marks, &mut _lv); // back to A
    let d = commit_group(&mut tree, 5, 20, 5, 40);
    tree.undo(&mut marks, &mut _lv); // back to A
                                     // current is at A

    // 4 live changes (a, b, c, d). Prune to 3.
    // Oldest non-ancestor leaf: B (seq=2). A is ancestor of current (A).
    tree.prune(3);

    assert_eq!(tree.change_count(), 3);
    assert!(
        tree.node_info(b).is_none(),
        "B (oldest leaf) should be pruned first"
    );
    // C and D survive
    assert!(tree.node_info(c).is_some());
    assert!(tree.node_info(d).is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// Sequence numbering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sequence_numbers_are_monotonic() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // at A
    let c = commit_group(&mut tree, 5, 15, 5, 300); // branch

    let a_info = tree.node_info(a).unwrap();
    let b_info = tree.node_info(b).unwrap();
    let c_info = tree.node_info(c).unwrap();

    assert_eq!(a_info.sequence, 1);
    assert_eq!(b_info.sequence, 2);
    assert_eq!(c_info.sequence, 3);
    assert!(a_info.sequence < b_info.sequence);
    assert!(b_info.sequence < c_info.sequence);
}

// ═══════════════════════════════════════════════════════════════════════════
// Snapshot (visualization)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn snapshot_empty_tree_has_one_node() {
    let tree = UndoTree::new();
    let snap = tree.snapshot();

    assert_eq!(snap.nodes.len(), 1, "empty tree has only the root node");
    assert_eq!(snap.current, NodeId::ROOT);
    assert_eq!(snap.change_count, 0);

    let root = &snap.nodes[0];
    assert_eq!(root.id, NodeId::ROOT);
    assert_eq!(root.parent, None);
    assert!(root.children.is_empty());
    assert!(root.is_current, "root should be current in empty tree");
    assert_eq!(root.sequence, 0);
    assert_eq!(root.timestamp, 0);
    assert_eq!(root.cursor_before, Offset::new(0));
}

#[test]
fn snapshot_linear_chain() {
    let mut tree = UndoTree::new();
    let _a = commit_group(&mut tree, 10, 15, 10, 100);
    let _b = commit_group(&mut tree, 15, 20, 15, 200);
    let c = commit_group(&mut tree, 20, 25, 20, 300);

    let snap = tree.snapshot();
    assert_eq!(snap.nodes.len(), 4, "root + 3 edits");
    assert_eq!(snap.current, c);
    assert_eq!(snap.change_count, 3);

    // Root has 1 child (A)
    assert_eq!(snap.nodes[0].children.len(), 1);
    assert_eq!(snap.nodes[0].children[0], NodeId::new(1));
    assert!(!snap.nodes[0].is_current);

    // A has 1 child (B), parent is root
    assert_eq!(snap.nodes[1].parent, Some(NodeId::ROOT));
    assert_eq!(snap.nodes[1].children.len(), 1);
    assert_eq!(snap.nodes[1].cursor_before, Offset::new(10));
    assert_eq!(snap.nodes[1].sequence, 1);
    assert_eq!(snap.nodes[1].timestamp, 100);
    assert!(!snap.nodes[1].is_current);

    // B has 1 child (C)
    assert_eq!(snap.nodes[2].parent, Some(NodeId::new(1)));
    assert_eq!(snap.nodes[2].children.len(), 1);
    assert!(!snap.nodes[2].is_current);

    // C is leaf and current
    assert_eq!(snap.nodes[3].parent, Some(NodeId::new(2)));
    assert!(snap.nodes[3].children.is_empty());
    assert!(snap.nodes[3].is_current, "last node should be current");
}

#[test]
fn snapshot_branched_tree() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    //   Root → A → B
    //              → C
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let b = commit_group(&mut tree, 5, 10, 5, 200);
    tree.undo(&mut marks, &mut _lv); // at A
    let c = commit_group(&mut tree, 5, 20, 5, 300);

    let snap = tree.snapshot();
    assert_eq!(snap.nodes.len(), 4, "root + A + B + C");
    assert_eq!(snap.current, c);
    assert_eq!(snap.change_count, 3);

    // A should have 2 children: B and C
    let a_view = &snap.nodes[a.index()];
    assert_eq!(a_view.children.len(), 2);
    assert_eq!(a_view.children[0], b);
    assert_eq!(a_view.children[1], c);

    // B is NOT current
    assert!(!snap.nodes[b.index()].is_current);
    // C IS current
    assert!(snap.nodes[c.index()].is_current);
}

#[test]
fn snapshot_is_current_marks_correct_node() {
    let mut tree = UndoTree::new();
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let _b = commit_group(&mut tree, 5, 10, 5, 200);

    // Current is at B
    let snap = tree.snapshot();
    let current_count = snap.nodes.iter().filter(|n| n.is_current).count();
    assert_eq!(current_count, 1, "exactly one node should be current");
    assert!(!snap.nodes[a.index()].is_current);
    assert!(snap.nodes[NodeId::new(2).index()].is_current);
}

#[test]
fn snapshot_after_undo_moves_is_current() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let a = commit_group(&mut tree, 0, 5, 0, 100);
    let _b = commit_group(&mut tree, 5, 10, 5, 200);

    tree.undo(&mut marks, &mut _lv); // at A

    let snap = tree.snapshot();
    assert_eq!(snap.current, a);
    assert!(snap.nodes[a.index()].is_current);
    assert!(
        !snap.nodes[NodeId::new(2).index()].is_current,
        "B should not be current after undo"
    );
}

#[test]
fn snapshot_timestamps_and_sequences_correct() {
    let mut tree = UndoTree::new();
    let a = commit_group(&mut tree, 0, 5, 0, 42);
    let b = commit_group(&mut tree, 5, 10, 5, 99);

    let snap = tree.snapshot();

    // Root
    assert_eq!(snap.nodes[0].sequence, 0);
    assert_eq!(snap.nodes[0].timestamp, 0);

    // A
    assert_eq!(snap.nodes[a.index()].sequence, 1);
    assert_eq!(snap.nodes[a.index()].timestamp, 42);

    // B
    assert_eq!(snap.nodes[b.index()].sequence, 2);
    assert_eq!(snap.nodes[b.index()].timestamp, 99);
}

// ═══════════════════════════════════════════════════════════════════════════
// Free-list slot reuse
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn arena_reuses_pruned_slots() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create 10 branch leaves from root (each is independently prunable).
    for i in 0..10u64 {
        commit_group(&mut tree, 0, 1, 0, i);
        tree.undo(&mut marks, &mut _lv); // back to root
    }

    // Arena should have 11 nodes (root + 10)
    assert_eq!(tree.node_count(), 11);
    assert_eq!(tree.change_count(), 10);
    assert_eq!(tree.current(), NodeId::ROOT);

    // Prune to 5 levels — should tombstone oldest 5 branch leaves
    tree.prune(5);
    assert_eq!(tree.change_count(), 5);
    let arena_after_prune = tree.node_count();

    // Now create 5 more undo groups — they should reuse pruned slots
    for i in 10..15u64 {
        commit_group(&mut tree, 0, 1, 0, i);
        tree.undo(&mut marks, &mut _lv); // back to root
    }

    // Arena should NOT have grown by 5 — it should have reused pruned slots
    assert!(
        tree.node_count() <= arena_after_prune,
        "arena grew from {} to {} — expected reuse of pruned slots",
        arena_after_prune,
        tree.node_count()
    );
}

#[test]
fn arena_converges_to_undolevels_size() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;
    let max_levels = 5;

    // Make 100 edits, pruning to 5 after each.
    // Each prune happens while current is on the latest node, so
    // the ancestor path protects root + up to max_levels nodes.
    // We undo to root before each prune so all non-root nodes are
    // prunable.
    for i in 0..100u64 {
        commit_group(&mut tree, 0, 1, 0, i);
        undo_n(&mut tree, &mut marks, 100); // back to root
        tree.prune(max_levels);
    }

    // Arena should have converged — not grown to 101
    assert!(
        tree.node_count() <= max_levels + 5,
        "arena size {} should have converged near undolevels {}",
        tree.node_count(),
        max_levels
    );
    assert!(tree.change_count() <= max_levels);
}

#[test]
fn reused_slot_is_fully_functional() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create 3 branches from root, then prune 2.
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    tree.undo(&mut marks, &mut _lv); // root
    let _b = commit_group(&mut tree, 0, 10, 0, 200);
    tree.undo(&mut marks, &mut _lv); // root
    let _c = commit_group(&mut tree, 0, 15, 0, 300);
    tree.undo(&mut marks, &mut _lv); // root

    // 3 live changes from root, all prunable. Prune to 1.
    tree.prune(1);
    assert_eq!(tree.change_count(), 1);

    // Create a new node that should reuse a pruned slot.
    let arena_before = tree.node_count();
    let d = commit_group(&mut tree, 0, 20, 0, 400);
    assert_eq!(
        tree.node_count(),
        arena_before,
        "new node should reuse a pruned slot, not grow arena"
    );

    // The reused node should be fully functional.
    let info = tree.node_info(d).unwrap();
    assert_eq!(info.parent(), Some(NodeId::ROOT));
    assert_eq!(info.cursor_after(), Offset::new(20));
    assert_eq!(info.timestamp(), 400);
    assert_eq!(info.depth(), 1);

    // Undo and redo should work through the reused slot.
    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node(), NodeId::ROOT);
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.node(), d);
}

#[test]
fn free_list_root_never_reused() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create and prune a single node.
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    tree.undo(&mut marks, &mut _lv);
    tree.prune(0);

    // Create a new node; it must NOT be placed at index 0 (root).
    let b = commit_group(&mut tree, 0, 10, 0, 200);
    assert_ne!(
        b,
        NodeId::ROOT,
        "root slot must never be reused for a non-root node"
    );
    // Root must still be valid.
    assert!(tree.node_info(NodeId::ROOT).is_some());
}

#[test]
fn snapshot_correct_after_slot_reuse() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create two branches from root.
    let _a = commit_group(&mut tree, 0, 5, 0, 100);
    tree.undo(&mut marks, &mut _lv);
    let _b = commit_group(&mut tree, 0, 10, 0, 200);
    tree.undo(&mut marks, &mut _lv);

    // Prune one.
    tree.prune(1);

    // Create a new node that reuses the pruned slot.
    let c = commit_group(&mut tree, 0, 20, 0, 300);

    let snap = tree.snapshot();
    // Should have root + survivor + new node = 3 live nodes.
    // (Survivor might have been pruned too if it was oldest; check generically.)
    let live_count = snap.nodes.len();
    assert!(live_count >= 2, "at least root + new node");

    // The new node should be in the snapshot with correct data.
    let c_view = snap.nodes.iter().find(|n| n.id == c);
    assert!(c_view.is_some(), "reused-slot node must appear in snapshot");
    let c_view = c_view.unwrap();
    assert_eq!(c_view.timestamp, 300);
    assert!(c_view.is_current);
}

// ═══════════════════════════════════════════════════════════════════════════
// External group (merge break)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn begin_external_group_breaks_merge() {
    let mut tree = UndoTree::new();

    // Start merge (simulates macro replay).
    tree.begin_merge();
    assert!(tree.is_merging());

    // First macro keystroke: begin_group + edit + end_group.
    // During merge, end_group does NOT commit — it stays pending.
    tree.begin_group(
        Offset::new(0),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(5));
    tree.end_group(Offset::new(10), 100, None);

    // The group is still pending (not committed) because merge is active.
    assert!(tree.has_pending_group());
    assert_eq!(
        tree.change_count(),
        0,
        "no committed nodes yet during merge"
    );

    // External edit arrives — should break the merge and commit accumulated work.
    let (broke, _fc) = tree.begin_external_group(Offset::new(20), MarkSnapshot::new(), None, 200);
    assert!(broke, "merge was active, should return true");
    assert!(!tree.is_merging(), "merge ended by begin_external_group");

    // The merged macro group should now be committed (change_count = 1).
    assert_eq!(tree.change_count(), 1, "merged group committed");

    // The external group is now pending.
    assert!(tree.has_pending_group());

    // Simulate the external edit and commit the external group.
    tree.mark_edit_at(Offset::new(25));
    tree.end_group(Offset::new(30), 200, None);

    // Now two nodes exist: the merged macro group + the external group.
    assert_eq!(tree.change_count(), 2);
}

#[test]
fn begin_external_group_without_merge_returns_false() {
    let mut tree = UndoTree::new();

    // No merge active — should just begin a normal group.
    let (broke, _fc) = tree.begin_external_group(Offset::new(0), MarkSnapshot::new(), None, 100);
    assert!(!broke, "no merge active, should return false");
    assert!(tree.has_pending_group());

    // Commit normally.
    tree.mark_edit_at(Offset::new(5));
    tree.end_group(Offset::new(10), 100, None);
    assert_eq!(tree.change_count(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Mode tracking per undo group
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn undo_step_carries_mode_from_group() {
    use crate::primitives::{Mode, VisualType};

    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create a group in visual-line mode.
    tree.begin_group(
        Offset::new(10),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        Mode::Visual(VisualType::Line),
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(10));
    tree.end_group(Offset::new(20), 100, None);

    // Undo should return the mode that was active when the group was created.
    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.mode(), Mode::Visual(VisualType::Line));
}

#[test]
fn redo_step_carries_mode_from_group() {
    use crate::primitives::{Mode, VisualType};

    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create a group in visual-block mode.
    tree.begin_group(
        Offset::new(0),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        Mode::Visual(VisualType::Block),
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(0));
    tree.end_group(Offset::new(5), 100, None);

    tree.undo(&mut marks, &mut _lv);
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.mode(), Mode::Visual(VisualType::Block));
}

#[test]
fn normal_mode_undo_group_default() {
    use crate::primitives::Mode;

    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create a group in normal mode.
    tree.begin_group(
        Offset::new(0),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(0));
    tree.end_group(Offset::new(5), 100, None);

    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.mode(), Mode::Normal);
}

#[test]
fn visual_block_mode_undo_returns_stored_mode() {
    use crate::primitives::{Mode, VisualType};

    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create a group in visual-block mode (e.g. block insert via Ctrl-V + I).
    tree.begin_group(
        Offset::new(0),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        Mode::Visual(VisualType::Block),
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(0));
    tree.end_group(Offset::new(10), 100, None);

    // Undo should carry the visual-block mode from the original group.
    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.mode(), Mode::Visual(VisualType::Block));
}

#[test]
fn insert_mode_undo_returns_stored_mode() {
    use crate::primitives::Mode;

    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Create a group in insert mode (the mode active when the undo group opened).
    tree.begin_group(
        Offset::new(5),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        Mode::Insert,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(5));
    tree.end_group(Offset::new(15), 200, None);

    // Undo should carry insert mode.
    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.mode(), Mode::Insert);

    // Redo should also carry insert mode.
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.mode(), Mode::Insert);
}

#[test]
fn mixed_modes_undo_redo_preserves_each() {
    use crate::primitives::{Mode, VisualType};

    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Group 1: normal mode edit.
    tree.begin_group(
        Offset::new(0),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(0));
    tree.end_group(Offset::new(5), 100, None);

    // Group 2: insert mode edit.
    tree.begin_group(
        Offset::new(5),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        Mode::Insert,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(5));
    tree.end_group(Offset::new(10), 200, None);

    // Group 3: visual-block mode edit.
    tree.begin_group(
        Offset::new(10),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        Mode::Visual(VisualType::Block),
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(10));
    tree.end_group(Offset::new(20), 300, None);

    // Undo all three and verify each returns the correct mode.
    let step3 = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step3.mode(), Mode::Visual(VisualType::Block));

    let step2 = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step2.mode(), Mode::Insert);

    let step1 = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step1.mode(), Mode::Normal);

    // Redo all three and verify modes are preserved.
    let redo1 = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(redo1.mode(), Mode::Normal);

    let redo2 = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(redo2.mode(), Mode::Insert);

    let redo3 = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(redo3.mode(), Mode::Visual(VisualType::Block));
}

// ═══════════════════════════════════════════════════════════════════════════
// Multi-cursor selection history per undo group
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_cursor_begin_group_stores_multiple_cursors() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Begin a group with 3 cursor positions.
    let cursors_before = [Offset::new(10), Offset::new(20), Offset::new(30)];
    tree.begin_group_multi(
        &cursors_before,
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(10));
    tree.end_group(Offset::new(15), 100, None);

    // Undo should return all 3 cursor positions.
    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.cursor(), Offset::new(10), "cursor() returns first");
    assert_eq!(
        step.cursors(),
        &[Offset::new(10), Offset::new(20), Offset::new(30)],
        "cursors() returns all positions"
    );
}

#[test]
fn multi_cursor_undo_redo_roundtrip() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    let cursors_before = [Offset::new(5), Offset::new(15), Offset::new(25)];
    tree.begin_group_multi(
        &cursors_before,
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(5));
    tree.end_group(Offset::new(10), 100, None);

    // Undo: all cursors restored.
    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.cursors(), &cursors_before);

    // Redo: all cursors restored.
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.cursors(), &cursors_before);
}

#[test]
fn single_cursor_begin_group_backward_compat() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    // Existing begin_group API with single cursor still works.
    tree.begin_group(
        Offset::new(42),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(42));
    tree.end_group(Offset::new(50), 100, None);

    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.cursor(), Offset::new(42));
    assert_eq!(step.cursors(), &[Offset::new(42)]);
}

#[test]
fn undo_step_cursors_single_cursor_default() {
    // Without multi-cursor feature, cursors() still returns a single-element slice.
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    tree.begin_group(
        Offset::new(7),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(7));
    tree.end_group(Offset::new(12), 100, None);

    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.cursor(), Offset::new(7));
    assert_eq!(step.cursors(), &[Offset::new(7)]);
}

#[test]
fn undo_with_five_cursors_roundtrip() {
    // Verify that 5 cursor positions survive undo and redo roundtrip.
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    let five_cursors = [
        Offset::new(10),
        Offset::new(25),
        Offset::new(40),
        Offset::new(55),
        Offset::new(70),
    ];
    tree.begin_group_multi(
        &five_cursors,
        crate::primitives::UndoCursorStrategy::EntryPosition,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    tree.mark_edit_at(Offset::new(10));
    tree.end_group(Offset::new(80), 100, None);

    // Undo: all 5 cursors restored (EntryPosition → cursors_before as-is).
    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.cursor(), Offset::new(10), "cursor() returns primary");
    assert_eq!(
        step.cursors(),
        &five_cursors,
        "all 5 cursors must survive undo"
    );

    // Redo: all 5 cursors restored again.
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(step.cursor(), Offset::new(10));
    assert_eq!(
        step.cursors(),
        &five_cursors,
        "all 5 cursors must survive redo"
    );
}

#[test]
fn first_edit_offset_override_applies_only_to_primary() {
    // With FirstEdit strategy, first_edit_offset overrides only the primary
    // cursor (index 0). Secondary cursors retain their stored positions.
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut _lv: Option<crate::primitives::LastVisualInfo> = None;

    let cursors_before = [
        Offset::new(50), // primary — will be overridden by first_edit_offset
        Offset::new(100),
        Offset::new(150),
        Offset::new(200),
        Offset::new(250),
    ];
    tree.begin_group_multi(
        &cursors_before,
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        None,
        false,
    );
    // Edit at offset 20, which is before cursor_before[0]=50.
    // This should override the primary cursor to 20.
    tree.mark_edit_at(Offset::new(20));
    tree.end_group(Offset::new(60), 100, None);

    // Undo: primary should be first_edit_offset=20, secondaries unchanged.
    let step = tree.undo(&mut marks, &mut _lv).unwrap();
    assert_eq!(
        step.cursor(),
        Offset::new(20),
        "primary cursor overridden by first_edit_offset"
    );
    assert_eq!(
        step.cursors(),
        &[
            Offset::new(20),  // overridden primary
            Offset::new(100), // secondary unchanged
            Offset::new(150),
            Offset::new(200),
            Offset::new(250),
        ],
        "only primary cursor is overridden; secondaries keep stored positions"
    );

    // Redo: same override logic applies.
    let step = tree.redo(&mut marks, &mut _lv).unwrap();
    assert_eq!(
        step.cursor(),
        Offset::new(20),
        "redo primary also uses first_edit_offset"
    );
    assert_eq!(step.cursors()[1], Offset::new(100));
    assert_eq!(step.cursors()[4], Offset::new(250));
}

// ═══════════════════════════════════════════════════════════════════════════
// Visual Info in Undo Nodes
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn task_5_3_undo_swaps_last_visual() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();

    let vi = crate::primitives::LastVisualInfo::char_wise(3);
    // Begin group with last_visual set.
    tree.begin_group(
        Offset::new(0),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        Some(vi),
        false,
    );
    tree.mark_edit_at(Offset::new(0));
    tree.end_group(Offset::new(5), 100, None);

    // Current live last_visual is different.
    let vi2 = crate::primitives::LastVisualInfo::line_wise(5);
    let mut live_lv: Option<crate::primitives::LastVisualInfo> = Some(vi2);

    // Undo swaps: node gets vi2 (was live), live gets vi (was node).
    let step = tree.undo(&mut marks, &mut live_lv).unwrap();

    // The step reports the node's post-swap value (vi2, what was live).
    assert_eq!(step.last_visual(), Some(vi2));

    // Live should now have vi (from the node before swap).
    assert_eq!(live_lv, Some(vi));
}

#[test]
fn task_5_3_redo_swaps_last_visual() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();

    let vi = crate::primitives::LastVisualInfo::char_wise(3);
    tree.begin_group(
        Offset::new(0),
        crate::primitives::UndoCursorStrategy::FirstEdit,
        MarkSnapshot::new(),
        None,
        crate::primitives::Mode::Normal,
        Some(vi),
        false,
    );
    tree.mark_edit_at(Offset::new(0));
    tree.end_group(Offset::new(5), 100, None);

    let mut live_lv: Option<crate::primitives::LastVisualInfo> = None;

    // Undo, then redo.
    tree.undo(&mut marks, &mut live_lv);

    let vi3 = crate::primitives::LastVisualInfo::block_wise(2, 4);
    live_lv = Some(vi3);

    let step = tree.redo(&mut marks, &mut live_lv).unwrap();
    // Redo should swap: node gets vi3, live gets whatever node had.
    assert!(step.last_visual().is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// Buffer Changed Flag / Save Point Detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn task_5_4_undo_to_save_point() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut lv: Option<crate::primitives::LastVisualInfo> = None;

    // Commit an edit, mark it as save point.
    commit_group(&mut tree, 0, 5, 0, 100);
    tree.mark_save();

    // Commit another edit (not saved).
    commit_group(&mut tree, 5, 10, 5, 200);

    // Undo the second edit — we should land on the save point.
    let step = tree.undo(&mut marks, &mut lv).unwrap();
    assert!(
        step.is_at_save_point(),
        "undoing past save should report is_at_save_point"
    );
}

#[test]
fn task_5_4_undo_to_root_is_save_point() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut lv: Option<crate::primitives::LastVisualInfo> = None;

    commit_group(&mut tree, 0, 5, 0, 100);

    let step = tree.undo(&mut marks, &mut lv).unwrap();
    assert!(
        step.is_at_save_point(),
        "undoing to root should report is_at_save_point (unmodified buffer)"
    );
}

#[test]
fn task_5_4_undo_not_at_save_point() {
    let mut tree = UndoTree::new();
    let mut marks = dummy_marks();
    let mut lv: Option<crate::primitives::LastVisualInfo> = None;

    commit_group(&mut tree, 0, 5, 0, 100);
    commit_group(&mut tree, 5, 10, 5, 200);

    // Undo once — should be at the first group, which is NOT a save point.
    let step = tree.undo(&mut marks, &mut lv).unwrap();
    assert!(
        !step.is_at_save_point(),
        "landing on non-saved node should not report is_at_save_point"
    );
}
