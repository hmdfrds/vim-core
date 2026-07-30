//! Undo testing utilities.
//!
//! `assert_atomic` verifies a compound operation creates exactly one undo
//! group. Uses `change_count()` which is stable across undo/redo.

use crate::session::TestSession;

/// Pre-operation state capture for undo assertions.
#[derive(Debug, Clone)]
pub struct UndoSnapshot {
    /// Document text before the operation.
    pub text: String,
    /// Primary cursor offset before the operation.
    pub cursor_offset: usize,
    /// Committed undo group count before the operation.
    pub change_count: usize,
}

impl UndoSnapshot {
    /// Capture current state from a `TestSession`.
    pub fn capture(session: &TestSession) -> Self {
        Self {
            text: session.text().to_owned(),
            cursor_offset: session.cursor_offset(),
            change_count: session.change_count(),
        }
    }
}

/// Assert that `keys` produce exactly ONE undo group (atomic operation).
///
/// 1. Captures `(text, cursor, change_count)` before
/// 2. Feeds `keys`
/// 3. Asserts `change_count` increased by exactly 1
/// 4. Feeds `u` to undo
/// 5. Asserts text matches original
/// 6. Feeds `<C-r>` to restore
#[track_caller]
pub fn assert_atomic(session: &mut TestSession, keys: &str) {
    let snapshot = UndoSnapshot::capture(session);

    session.feed(keys);

    let after_count = session.change_count();
    let groups_created = after_count - snapshot.change_count;

    assert!(
        groups_created == 1,
        "ATOMICITY ASSERTION FAILED\n\
         \x20 keys: {keys:?}\n\
         \x20 expected: 1 undo group\n\
         \x20 actual: {groups_created} undo groups\n\
         \x20 change_count before: {}\n\
         \x20 change_count after: {after_count}",
        snapshot.change_count
    );

    session.feed("u");

    let undo_text = session.text().to_owned();
    assert!(
        undo_text == snapshot.text,
        "ATOMICITY ASSERTION FAILED — undo did not restore original text\n\
         \x20 keys: {keys:?}\n\
         \x20 original: {:?}\n\
         \x20 after undo: {undo_text:?}",
        snapshot.text
    );

    session.feed("<C-r>");
}

/// Assert a full undo/redo round-trip: do → undo → verify original → redo → verify post-edit.
#[track_caller]
pub fn assert_round_trip(session: &mut TestSession, keys: &str) {
    let before_text = session.text().to_owned();
    let before_count = session.change_count();

    session.feed(keys);

    let after_text = session.text().to_owned();
    let groups_created = session.change_count() - before_count;

    for _ in 0..groups_created {
        session.feed("u");
    }

    let undo_text = session.text().to_owned();
    assert!(
        undo_text == before_text,
        "ROUND-TRIP ASSERTION FAILED — undo did not restore original\n\
         \x20 keys: {keys:?}\n\
         \x20 original: {before_text:?}\n\
         \x20 after undo: {undo_text:?}\n\
         \x20 groups undone: {groups_created}"
    );

    for _ in 0..groups_created {
        session.feed("<C-r>");
    }

    let redo_text = session.text().to_owned();
    assert!(
        redo_text == after_text,
        "ROUND-TRIP ASSERTION FAILED — redo did not restore post-edit state\n\
         \x20 keys: {keys:?}\n\
         \x20 post-edit: {after_text:?}\n\
         \x20 after redo: {redo_text:?}\n\
         \x20 groups redone: {groups_created}"
    );
}
