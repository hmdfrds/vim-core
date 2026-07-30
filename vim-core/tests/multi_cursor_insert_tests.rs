//! Tests proving that multi-cursor insert-mode replication works correctly.
//!
//! BUG SUMMARY:
//! When multiple cursors are active and the user enters insert mode, typed
//! characters (and backspace) should be replicated to ALL cursors. Currently,
//! only the primary cursor receives the edit -- secondary cursors are ignored.
//!
//! These tests MUST FAIL with the current code, proving the bug exists.
//! After the fix, they should all pass.

#![allow(non_snake_case)]

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::Mode;

// =============================================================================
// Helper
// =============================================================================

/// Feed a Vim key-notation string into the session, returning all responses.
fn feed(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

/// Feed a single key and return the HostResponse (for inspecting edits).
fn feed_one(session: &mut HostSession, keys: &str) -> Vec<vim_core::execution::EditOp> {
    let parsed = parse_keys_from_string(keys);
    let mut all_edits = Vec::new();
    for key in parsed {
        let resp = session.process_key_host(key);
        all_edits.extend(resp.edits);
    }
    all_edits
}

// =============================================================================
// TEST 1: Basic character insert with 2 cursors
// =============================================================================

/// Text "aaa\nbbb", cursors at offset 1 and 5.
/// Enter insert mode, type 'X'.
/// Expected: "aXaa\nbXbb"
///
/// BUG: Only the primary cursor gets the insert. The secondary cursor is
/// ignored, producing "aXaa\nbbb" (or "aaa\nbXbb" depending on which is primary).
#[test]
fn basic_char_insert_two_cursors() {
    let mut session = HostSession::new("aaa\nbbb");

    // Primary cursor starts at offset 0. Move it to offset 1.
    feed(&mut session, "l");
    assert_eq!(session.cursor_offset(), 1);

    // Add a secondary cursor at offset 5 (the 'b' at position 1 of line 2).
    session.add_cursor(5).expect("add_cursor should succeed");
    assert_eq!(session.cursor_count(), 2);

    // Enter insert mode
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);

    // Type 'X' -- should produce edits at BOTH cursor positions
    let edits = feed_one(&mut session, "X");

    // Exit insert mode
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Verify the final text
    assert_eq!(
        session.text(),
        "aXaa\nbXbb",
        "BUG PROVEN: Character insert with 2 cursors did not replicate to all cursors.\n\
         Expected: \"aXaa\\nbXbb\"\n\
         Got:      {:?}\n\
         \n\
         Only the primary cursor received the insert; secondary cursors were ignored.",
        session.text(),
    );

    // Verify we got insert edits for both cursors
    let insert_edits: Vec<_> = edits
        .iter()
        .filter(|e| !e.insert.is_empty() && e.delete == 0)
        .collect();
    assert!(
        insert_edits.len() >= 2,
        "BUG PROVEN: Expected 2 insert edits (one per cursor), got {}.\n\
         Edits: {:?}",
        insert_edits.len(),
        edits,
    );
}

// =============================================================================
// TEST 2: Backspace with 2 cursors
// =============================================================================

/// Text "aaa\nbbb", cursors at offset 2 and 6.
/// Enter insert mode, press Backspace.
/// Expected: "aa\nbb"
///
/// BUG: Only the primary cursor's character is deleted.
#[test]
fn backspace_two_cursors() {
    let mut session = HostSession::new("aaa\nbbb");

    // Move primary cursor to offset 2
    feed(&mut session, "ll");
    assert_eq!(session.cursor_offset(), 2);

    // Add secondary cursor at offset 6
    session.add_cursor(6).expect("add_cursor should succeed");
    assert_eq!(session.cursor_count(), 2);

    // Enter insert mode
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);

    // Press Backspace -- should delete at BOTH cursor positions
    let edits = feed_one(&mut session, "<BS>");

    // Exit insert mode
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Verify the final text
    assert_eq!(
        session.text(),
        "aa\nbb",
        "BUG PROVEN: Backspace with 2 cursors did not replicate to all cursors.\n\
         Expected: \"aa\\nbb\"\n\
         Got:      {:?}\n\
         \n\
         Only the primary cursor's character was deleted.",
        session.text(),
    );

    // Verify we got delete edits for both cursors
    let delete_edits: Vec<_> = edits.iter().filter(|e| e.delete > 0).collect();
    assert!(
        delete_edits.len() >= 2,
        "BUG PROVEN: Expected 2 delete edits (one per cursor), got {}.\n\
         Edits: {:?}",
        delete_edits.len(),
        edits,
    );
}

// =============================================================================
// TEST 3: Multi-char typing with 2 cursors
// =============================================================================

/// Text "aaa\nbbb", cursors at offset 1 and 5.
/// Enter insert, type "XY", exit.
/// Expected: "aXYaa\nbXYbb"
///
/// BUG: Only the primary cursor gets the typed text.
#[test]
fn multi_char_typing_two_cursors() {
    let mut session = HostSession::new("aaa\nbbb");

    // Move primary cursor to offset 1
    feed(&mut session, "l");
    assert_eq!(session.cursor_offset(), 1);

    // Add secondary cursor at offset 5
    session.add_cursor(5).expect("add_cursor should succeed");
    assert_eq!(session.cursor_count(), 2);

    // Enter insert mode, type "XY", exit
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "XY");
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Verify the final text
    assert_eq!(
        session.text(),
        "aXYaa\nbXYbb",
        "BUG PROVEN: Multi-char typing with 2 cursors did not replicate to all cursors.\n\
         Expected: \"aXYaa\\nbXYbb\"\n\
         Got:      {:?}\n\
         \n\
         Only the primary cursor received the typed text; secondary cursors were ignored.",
        session.text(),
    );
}

// =============================================================================
// TEST 4: Undo after multi-cursor insert
// =============================================================================

/// After multi-cursor insert (test 3 scenario), pressing 'u' should undo the
/// entire multi-cursor insert as one atomic operation.
///
/// Text "aaa\nbbb", cursors at 1 and 5, type "XY", exit.
/// Result: "aXYaa\nbXYbb"
/// After 'u': should be back to "aaa\nbbb"
///
/// This test depends on the insert replication working first (test 3).
/// It verifies the multi-cursor edit happened correctly, THEN checks undo.
#[test]
fn undo_after_multi_cursor_insert() {
    let original = "aaa\nbbb";
    let mut session = HostSession::new(original);

    // Move primary cursor to offset 1
    feed(&mut session, "l");

    // Add secondary cursor at offset 5
    session.add_cursor(5).expect("add_cursor should succeed");
    assert_eq!(session.cursor_count(), 2);

    // Enter insert mode, type "XY", exit
    feed(&mut session, "i");
    feed(&mut session, "XY");
    feed(&mut session, "<Esc>");

    let after_edit = session.text().to_owned();

    // First: verify the multi-cursor edit produced the correct result
    assert_eq!(
        after_edit, "aXYaa\nbXYbb",
        "BUG PROVEN (prerequisite): Multi-cursor insert did not replicate.\n\
         Expected: \"aXYaa\\nbXYbb\"\n\
         Got:      {:?}\n\
         \n\
         This test depends on multi-cursor insert replication working.",
        after_edit,
    );

    // Undo -- should revert ALL cursors' inserts atomically
    feed(&mut session, "u");

    assert_eq!(
        session.text(),
        original,
        "BUG PROVEN: Undo after multi-cursor insert did not restore original text.\n\
         Expected (original): {:?}\n\
         Got (after undo):    {:?}\n\
         \n\
         The undo should revert all cursor inserts atomically.",
        original,
        session.text(),
    );
}

// =============================================================================
// TEST 5: 3 cursors typing
// =============================================================================

/// Text "aaa\nbbb\nccc", cursors at 1, 5, 9.
/// Enter insert, type 'Z', exit.
/// Expected: "aZaa\nbZbb\ncZcc"
///
/// BUG: Only the primary cursor gets the insert.
#[test]
fn three_cursors_typing() {
    let mut session = HostSession::new("aaa\nbbb\nccc");

    // Move primary cursor to offset 1
    feed(&mut session, "l");
    assert_eq!(session.cursor_offset(), 1);

    // Add secondary cursors at offset 5 and 9
    session
        .add_cursor(5)
        .expect("add_cursor at 5 should succeed");
    session
        .add_cursor(9)
        .expect("add_cursor at 9 should succeed");
    assert_eq!(session.cursor_count(), 3);

    // Enter insert mode, type 'Z', exit
    feed(&mut session, "i");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "Z");
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Verify the final text
    assert_eq!(
        session.text(),
        "aZaa\nbZbb\ncZcc",
        "BUG PROVEN: 3-cursor insert did not replicate to all cursors.\n\
         Expected: \"aZaa\\nbZbb\\ncZcc\"\n\
         Got:      {:?}\n\
         \n\
         Only the primary cursor received the insert; other cursors were ignored.",
        session.text(),
    );
}

// =============================================================================
// TEST 6: Insert exit with count (2i + text + Esc)
// =============================================================================

/// Text "ab\ncd", cursors at 1 and 4.
/// `2i` then type 'X' then Escape.
/// Expected: "aXXb\ncXXd" (count=2 repeats the inserted text).
///
/// BUG: Only the primary cursor gets the repeated insert.
#[test]
fn insert_with_count_two_cursors() {
    let mut session = HostSession::new("ab\ncd");

    // Move primary cursor to offset 1
    feed(&mut session, "l");
    assert_eq!(session.cursor_offset(), 1);

    // Add secondary cursor at offset 4
    // "ab\ncd" => a=0, b=1, \n=2, c=3, d=4
    session.add_cursor(4).expect("add_cursor should succeed");
    assert_eq!(session.cursor_count(), 2);

    // `2i` to enter insert with count=2, type 'X', then Escape
    feed(&mut session, "2i");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "X");
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Verify the final text -- count=2 means 'X' is inserted twice at each cursor
    assert_eq!(
        session.text(),
        "aXXb\ncXXd",
        "BUG PROVEN: Insert with count + 2 cursors did not replicate correctly.\n\
         Expected: \"aXXb\\ncXXd\"\n\
         Got:      {:?}\n\
         \n\
         The count=2 repeat and/or multi-cursor replication failed.",
        session.text(),
    );
}

// =============================================================================
// TEST 7: `o` (open line below) with 3 cursors
// =============================================================================

/// Text: 3 identical lines "aaa\nbbb\nccc".
/// Cursors at offset 1, 5, 9 (on 'a', 'b', 'c' of each line).
/// Press `o` to open line below, type "lol", press Escape.
/// Expected: each line gets a new line below containing "lol".
///
/// BUG: Without the fix, the selection update was skipped when entering insert
/// mode, causing secondary cursor positions to be stale (T0 offsets). Subsequent
/// typing used wrong deltas, inserting text at incorrect positions.
#[test]
fn open_line_below_three_cursors() {
    let mut session = HostSession::new("aaa\nbbb\nccc");

    // Move primary cursor to offset 1
    feed(&mut session, "l");
    assert_eq!(session.cursor_offset(), 1);

    // Add secondary cursors at offset 5 and 9
    session
        .add_cursor(5)
        .expect("add_cursor at 5 should succeed");
    session
        .add_cursor(9)
        .expect("add_cursor at 9 should succeed");
    assert_eq!(session.cursor_count(), 3);

    // Press `o` to open line below at each cursor
    feed(&mut session, "o");
    assert_eq!(session.mode(), Mode::Insert);

    // Type "lol"
    feed(&mut session, "lol");

    // Exit insert mode
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Verify: each original line has a "lol" line below it
    assert_eq!(
        session.text(),
        "aaa\nlol\nbbb\nlol\nccc\nlol",
        "BUG: `o` with 3 cursors did not insert text at correct positions.\n\
         Expected: \"aaa\\nlol\\nbbb\\nlol\\nccc\\nlol\"\n\
         Got:      {:?}\n\
         \n\
         After `o`, selection heads were not updated for the insert-mode transition,\n\
         causing subsequent typing to use stale T0 offsets for delta computation.",
        session.text(),
    );
}

// =============================================================================
// TEST 8: `O` (open line above) with 2 cursors
// =============================================================================

/// Text: "aaa\nbbb".
/// Cursors at offset 1 and 5.
/// Press `O` to open line above, type "hi", press Escape.
/// Expected: new lines above each cursor's line containing "hi".
#[test]
fn open_line_above_two_cursors() {
    let mut session = HostSession::new("aaa\nbbb");

    // Move primary cursor to offset 1
    feed(&mut session, "l");
    assert_eq!(session.cursor_offset(), 1);

    // Add secondary cursor at offset 5
    session.add_cursor(5).expect("add_cursor should succeed");
    assert_eq!(session.cursor_count(), 2);

    // Press `O` to open line above at each cursor
    feed(&mut session, "O");
    assert_eq!(session.mode(), Mode::Insert);

    // Type "hi"
    feed(&mut session, "hi");

    // Exit insert mode
    feed(&mut session, "<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    // Verify: new "hi" lines above each original line
    assert_eq!(
        session.text(),
        "hi\naaa\nhi\nbbb",
        "BUG: `O` with 2 cursors did not insert text at correct positions.\n\
         Expected: \"hi\\naaa\\nhi\\nbbb\"\n\
         Got:      {:?}",
        session.text(),
    );
}
