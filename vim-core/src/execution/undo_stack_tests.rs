use super::*;
use crate::primitives::{NodeId, Offset};

#[test]
fn basic_undo_restores_before_state() {
    let mut store = UndoStore::new();
    store.begin_group("hello", Offset::new(3));
    store.end_group(Some(NodeId::new(1)), "hello world", Offset::new(5));

    let result = store.undo_step(NodeId::new(1), "hello world").unwrap();
    assert_eq!(result.text, "hello");
    assert_eq!(result.cursor, Offset::new(3));
    assert!(!result.ops.is_empty());
}

#[test]
fn basic_redo_restores_after_state() {
    let mut store = UndoStore::new();
    store.begin_group("hello", Offset::new(3));
    store.end_group(Some(NodeId::new(1)), "hello world", Offset::new(5));

    let result = store.redo_step(NodeId::new(1), "hello").unwrap();
    assert_eq!(result.text, "hello world");
    assert_eq!(result.cursor, Offset::new(5));
    assert!(!result.ops.is_empty());
}

#[test]
fn missing_node_returns_none() {
    let mut store = UndoStore::new();
    assert!(store.undo_step(NodeId::new(99), "anything").is_none());
    assert!(store.redo_step(NodeId::new(99), "anything").is_none());
}

#[test]
fn empty_group_discarded() {
    let mut store = UndoStore::new();
    store.begin_group("text", Offset::ZERO);
    store.end_group(None, "text", Offset::ZERO); // empty group
                                                 // No NodeId assigned, so nothing stored
    assert!(store.undo_step(NodeId::new(1), "text").is_none());
}

#[test]
fn begin_group_idempotent() {
    let mut store = UndoStore::new();
    store.begin_group("first", Offset::ZERO);
    store.begin_group("second", Offset::new(1)); // should NOT overwrite
    store.end_group(Some(NodeId::new(1)), "after", Offset::new(5));

    let result = store.undo_step(NodeId::new(1), "after").unwrap();
    assert_eq!(result.text, "first"); // first begin_group wins
    assert_eq!(result.cursor, Offset::ZERO);
}

#[test]
fn multiple_groups_independent() {
    let mut store = UndoStore::new();

    store.begin_group("state0", Offset::ZERO);
    store.end_group(Some(NodeId::new(1)), "state1", Offset::new(1));

    store.begin_group("state1", Offset::new(1));
    store.end_group(Some(NodeId::new(2)), "state2", Offset::new(2));

    // Each node has its own before/after
    let r1 = store.undo_step(NodeId::new(1), "state1").unwrap();
    assert_eq!(r1.text, "state0");
    assert_eq!(r1.cursor, Offset::ZERO);

    let r2 = store.undo_step(NodeId::new(2), "state2").unwrap();
    assert_eq!(r2.text, "state1");
    assert_eq!(r2.cursor, Offset::new(1));

    // Redo gives after-state
    let r1r = store.redo_step(NodeId::new(1), "state0").unwrap();
    assert_eq!(r1r.text, "state1");
    assert_eq!(r1r.cursor, Offset::new(1));
}

// ---------------------------------------------------------------------------
// New tests
// ---------------------------------------------------------------------------

#[test]
fn undo_redo_multibyte_utf8() {
    let mut store = UndoStore::new();
    let node = NodeId::new(1);
    store.begin_group("hello 世界", Offset::ZERO);
    store.end_group(Some(node), "goodbye 🌍", Offset::new(5));
    let result = store.undo_step(node, "goodbye 🌍").unwrap();
    assert_eq!(result.text, "hello 世界");
    let result = store.redo_step(node, "hello 世界").unwrap();
    assert_eq!(result.text, "goodbye 🌍");
}

#[test]
fn sequential_multi_step_undo() {
    let mut store = UndoStore::new();
    let texts = ["a", "ab", "abc", "abcd", "abcde"];
    let nodes: Vec<NodeId> = (1..=4).map(NodeId::new).collect();
    for i in 0..4 {
        store.begin_group(texts[i], Offset::ZERO);
        store.end_group(Some(nodes[i]), texts[i + 1], Offset::ZERO);
    }
    // Chain undo back to "a"
    let mut current = texts[4].to_string();
    for i in (0..4).rev() {
        let result = store.undo_step(nodes[i], &current).unwrap();
        current = result.text;
    }
    assert_eq!(current, "a");
    // Chain redo forward to "abcde"
    for i in 0..4 {
        let result = store.redo_step(nodes[i], &current).unwrap();
        current = result.text;
    }
    assert_eq!(current, "abcde");
}

#[test]
fn empty_edit_group_identity() {
    let mut store = UndoStore::new();
    let node = NodeId::new(1);
    store.begin_group("unchanged", Offset::new(5));
    store.end_group(Some(node), "unchanged", Offset::new(5));
    let result = store.undo_step(node, "unchanged").unwrap();
    assert_eq!(result.text, "unchanged");
    assert_eq!(result.cursor, Offset::new(5));
}

#[test]
fn full_document_replacement() {
    let mut store = UndoStore::new();
    let node = NodeId::new(1);
    let before = "a".repeat(10_000);
    let after = "b".repeat(10_000);
    store.begin_group(&before, Offset::ZERO);
    store.end_group(Some(node), &after, Offset::ZERO);
    let result = store.undo_step(node, &after).unwrap();
    assert_eq!(result.text, before);
    let result = store.redo_step(node, &before).unwrap();
    assert_eq!(result.text, after);
}

#[test]
fn desync_falls_back_to_checkpoint() {
    // The first committed node (sequence 0) is always a checkpoint,
    // so desync fallback works for it.
    let mut store = UndoStore::new();
    let node = NodeId::new(1);
    store.begin_group("hello", Offset::ZERO);
    store.end_group(Some(node), "world", Offset::new(5));
    // Wrong text length -> changeset apply fails -> falls back to checkpoint.
    // Undo: applies inverse to checkpoint_text_after to derive text_before.
    let undo = store
        .undo_step(node, "completely different length text")
        .unwrap();
    assert_eq!(undo.text, "hello");
    assert_eq!(undo.cursor, Offset::ZERO);
    assert!(
        !undo.ops.is_empty(),
        "fallback should produce diff-based ops"
    );

    // Redo: uses checkpoint_text_after directly.
    let redo = store.redo_step(node, "also wrong").unwrap();
    assert_eq!(redo.text, "world");
    assert_eq!(redo.cursor, Offset::new(5));
    assert!(
        !redo.ops.is_empty(),
        "fallback should produce diff-based ops"
    );
}

#[test]
fn desync_undo_after_external_edit() {
    // Simulates the exact scenario from the FFI test:
    // 1. Start with "abcdef"
    // 2. 'x' deletes 'a' -> "bcdef"
    // 3. External edit appends "Z" -> "bcdefZ"
    // 4. Undo should restore to "abcdef" (the before-state of the 'x')
    //
    // The first node (sequence 0) is always a checkpoint, so this works.
    let mut store = UndoStore::new();
    let node = NodeId::new(1);
    store.begin_group("abcdef", Offset::ZERO);
    store.end_group(Some(node), "bcdef", Offset::ZERO);
    // External edit changes "bcdef" -> "bcdefZ" (length mismatch: 6 vs expected 5)
    let result = store.undo_step(node, "bcdefZ").unwrap();
    assert_eq!(result.text, "abcdef");
    assert_eq!(result.cursor, Offset::ZERO);
}

#[test]
fn nested_begin_end_groups() {
    let mut store = UndoStore::new();
    let node = NodeId::new(1);
    store.begin_group("start", Offset::ZERO);
    store.begin_group("ignored", Offset::new(99)); // no-op
    store.end_group(Some(node), "finish", Offset::new(5));
    store.end_group(Some(NodeId::new(2)), "extra", Offset::ZERO); // no-op, pending consumed
    let result = store.undo_step(node, "finish").unwrap();
    assert_eq!(result.text, "start");
    assert_eq!(result.cursor, Offset::ZERO);
    assert!(store.undo_step(NodeId::new(2), "extra").is_none());
}

#[test]
fn undo_after_branch() {
    let mut store = UndoStore::new();
    let node1 = NodeId::new(1);
    let node2 = NodeId::new(2);
    let node3 = NodeId::new(3);
    // "a" -> "ab" -> "abc"
    store.begin_group("a", Offset::ZERO);
    store.end_group(Some(node1), "ab", Offset::new(2));
    store.begin_group("ab", Offset::new(2));
    store.end_group(Some(node2), "abc", Offset::new(3));
    // Undo node2: "abc" -> "ab"
    let result = store.undo_step(node2, "abc").unwrap();
    assert_eq!(result.text, "ab");
    // Branch: "ab" -> "abx"
    store.begin_group("ab", Offset::new(2));
    store.end_group(Some(node3), "abx", Offset::new(3));
    // Undo node3: "abx" -> "ab"
    let result = store.undo_step(node3, "abx").unwrap();
    assert_eq!(result.text, "ab");
    // Undo node1: "ab" -> "a"
    let result = store.undo_step(node1, "ab").unwrap();
    assert_eq!(result.text, "a");
    // Redo node1: "a" -> "ab"
    let result = store.redo_step(node1, "a").unwrap();
    assert_eq!(result.text, "ab");
}

// ---------------------------------------------------------------------------
// UndoResult.ops tests
// ---------------------------------------------------------------------------

#[test]
fn undo_result_ops_are_inverse() {
    let mut store = UndoStore::new();
    let node = NodeId::new(1);
    store.begin_group("hello", Offset::ZERO);
    store.end_group(Some(node), "hello world", Offset::new(5));

    let result = store.undo_step(node, "hello world").unwrap();
    // The inverse ops should contain at least a Delete (to remove " world")
    assert!(!result.ops.is_empty());
    // Verify the ops produce the correct text when applied conceptually:
    // the text field is the authoritative result.
    assert_eq!(result.text, "hello");
}

#[test]
fn redo_result_ops_are_forward() {
    let mut store = UndoStore::new();
    let node = NodeId::new(1);
    store.begin_group("hello", Offset::ZERO);
    store.end_group(Some(node), "hello world", Offset::new(5));

    let result = store.redo_step(node, "hello").unwrap();
    // The forward ops should contain at least an Insert (to add " world")
    assert!(!result.ops.is_empty());
    assert_eq!(result.text, "hello world");
}

// ---------------------------------------------------------------------------
// Checkpoint interval tests
// ---------------------------------------------------------------------------

#[test]
fn checkpoint_stored_at_interval_boundaries() {
    let mut store = UndoStore::new();

    // Commit CHECKPOINT_INTERVAL + 1 groups (nodes 1..=65).
    // Sequence 0 is a checkpoint (node 1), sequence 64 is a checkpoint (node 65).
    // Sequences 1..63 are NOT checkpoints.
    for i in 1..=(CHECKPOINT_INTERVAL + 1) {
        let before = format!("state{}", i - 1);
        let after = format!("state{i}");
        store.begin_group(&before, Offset::ZERO);
        store.end_group(Some(NodeId::new(i as u32)), &after, Offset::ZERO);
    }

    // Node 1 (sequence 0): checkpoint -- desync fallback should work.
    let undo = store.undo_step(NodeId::new(1), "wrong length text");
    assert!(
        undo.is_some(),
        "sequence 0 is a checkpoint, desync fallback should succeed"
    );

    // Node 2 (sequence 1): NOT a checkpoint -- desync fallback should return None.
    let undo = store.undo_step(NodeId::new(2), "wrong length text");
    assert!(
        undo.is_none(),
        "sequence 1 is not a checkpoint, desync fallback should return None"
    );

    // Node 65 (sequence 64): checkpoint -- desync fallback should work.
    let undo = store.undo_step(
        NodeId::new((CHECKPOINT_INTERVAL + 1) as u32),
        "wrong length text",
    );
    assert!(
        undo.is_some(),
        "sequence 64 is a checkpoint, desync fallback should succeed"
    );
}

#[test]
fn non_checkpoint_node_undo_redo_works_when_synced() {
    // Even non-checkpoint nodes work fine when the document is in sync
    // (the changeset apply() path succeeds without needing a fallback).
    let mut store = UndoStore::new();

    // Commit 3 groups. Sequence 0 = checkpoint, sequences 1-2 = not.
    store.begin_group("aaa", Offset::ZERO);
    store.end_group(Some(NodeId::new(1)), "bbb", Offset::ZERO); // seq 0

    store.begin_group("bbb", Offset::ZERO);
    store.end_group(Some(NodeId::new(2)), "ccc", Offset::ZERO); // seq 1

    store.begin_group("ccc", Offset::ZERO);
    store.end_group(Some(NodeId::new(3)), "ddd", Offset::ZERO); // seq 2

    // Non-checkpoint undo/redo works fine when text is in sync.
    let r = store.undo_step(NodeId::new(2), "ccc").unwrap();
    assert_eq!(r.text, "bbb");

    let r = store.redo_step(NodeId::new(2), "bbb").unwrap();
    assert_eq!(r.text, "ccc");

    let r = store.undo_step(NodeId::new(3), "ddd").unwrap();
    assert_eq!(r.text, "ccc");

    let r = store.redo_step(NodeId::new(3), "ccc").unwrap();
    assert_eq!(r.text, "ddd");
}

#[test]
fn desync_non_checkpoint_returns_none() {
    // When a non-checkpoint node's changeset apply fails (desync),
    // the fallback returns None because there's no checkpoint text.
    let mut store = UndoStore::new();

    // Consume sequence 0 (checkpoint) on a dummy group.
    store.begin_group("dummy", Offset::ZERO);
    store.end_group(Some(NodeId::new(100)), "dummy2", Offset::ZERO);

    // Node 2 gets sequence 1 (NOT a checkpoint).
    store.begin_group("hello", Offset::ZERO);
    store.end_group(Some(NodeId::new(2)), "world", Offset::ZERO);

    // Desync undo on non-checkpoint node returns None.
    let result = store.undo_step(NodeId::new(2), "completely different");
    assert!(
        result.is_none(),
        "desync on non-checkpoint node should return None"
    );

    // Desync redo on non-checkpoint node returns None.
    let result = store.redo_step(NodeId::new(2), "completely different");
    assert!(
        result.is_none(),
        "desync redo on non-checkpoint node should return None"
    );
}

#[test]
fn sequence_counter_increments_correctly() {
    let mut store = UndoStore::new();
    assert_eq!(store.next_sequence, 0);

    store.begin_group("a", Offset::ZERO);
    store.end_group(Some(NodeId::new(1)), "b", Offset::ZERO);
    assert_eq!(store.next_sequence, 1);

    // Discarded group (None node_id) should NOT increment sequence.
    store.begin_group("b", Offset::ZERO);
    store.end_group(None, "b", Offset::ZERO);
    assert_eq!(store.next_sequence, 1);

    store.begin_group("b", Offset::ZERO);
    store.end_group(Some(NodeId::new(2)), "c", Offset::ZERO);
    assert_eq!(store.next_sequence, 2);
}
