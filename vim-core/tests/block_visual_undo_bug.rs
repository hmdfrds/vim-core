//! Tests proving the undo bug in visual block `s` and `c` operations.
//!
//! BUG SUMMARY:
//! In visual block mode, `s` and `c` produce mismatched undo groups:
//! - The deletion is wrapped in a complete `begin_undo`...`end_undo` group
//! - Then insert mode begins WITHOUT opening a new undo group
//! - On insert exit, `exit_finalize` emits `end_undo()` with no matching `begin_undo()`
//!
//! OBSERVED MANIFESTATION:
//! The engine's effect processor force-merges the orphaned `end_undo` into the
//! existing undo group, so a single `u` does undo the full operation. However,
//! the redo data is corrupted: `Ctrl-R` only re-applies the deletion (not the
//! typed text), because the insert text was recorded outside proper undo group
//! boundaries. This proves the undo group structure is broken even though the
//! undo direction happens to work due to the force-merge fallback.
//!
//! These tests MUST FAIL with the current code, proving the bug exists.

#![allow(non_snake_case)]

use vim_core::execution::{parse_keys_from_string, HostSession};
use vim_core::primitives::{Mode, VisualType};

// =============================================================================
// Helper
// =============================================================================

/// Feed a Vim key-notation string into the session.
fn feed(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

// =============================================================================
// TEST 1: Visual block `s` redo is broken (proves mismatched undo groups)
// =============================================================================

/// Visual block `s` + type + Escape, then undo + redo should round-trip.
///
/// After the operation: "XYZlo\nXYZld\nXYZpy"
/// After undo: back to original "hello\nworld\nhappy"
/// After redo (Ctrl-R): should return to "XYZlo\nXYZld\nXYZpy"
///
/// BUG: Redo only re-applies the deletion, producing "lo\nld\npy" (the typed
/// "XYZ" is lost). This proves the undo group for the insert is not properly
/// recorded in the redo data.
#[test]
fn block_visual_s_redo_restores_edited_text() {
    let original = "hello\nworld\nhappy";
    let mut session = HostSession::new(original);

    // Enter visual block mode, select 3 lines x 3 columns
    feed(&mut session, "<C-v>jjll");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Block));

    // Press `s` to substitute the block, type "XYZ", exit insert
    feed(&mut session, "s");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "XYZ<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    let after_edit = session.text().to_owned();
    assert_ne!(after_edit, original, "Edit should change the text");

    // Undo
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "Undo should restore original (this works due to force-merge)"
    );

    // Redo -- this is where the bug manifests
    feed(&mut session, "<C-r>");

    assert_eq!(
        session.text(),
        after_edit,
        "BUG PROVEN: Redo after visual block `s` does not restore the edited text.\n\
         Expected (after edit): {:?}\n\
         Got (after redo):      {:?}\n\
         \n\
         The redo only re-applied the deletion but not the typed text 'XYZ'.\n\
         This proves the undo group structure is broken: the insert text\n\
         was recorded outside proper undo group boundaries.",
        after_edit,
        session.text(),
    );
}

// =============================================================================
// TEST 2: Visual block `c` redo is broken (same root cause)
// =============================================================================

/// Visual block `c` (change) redo should round-trip after undo.
/// Same bug as `s` -- both route through the Change operator in block_visual.rs.
#[test]
fn block_visual_c_redo_restores_edited_text() {
    let original = "hello\nworld\nhappy";
    let mut session = HostSession::new(original);

    // Enter visual block mode, select 3 lines x 3 columns
    feed(&mut session, "<C-v>jjll");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Block));

    // Press `c` to change the block, type "ABC", exit insert
    feed(&mut session, "c");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "ABC<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    let after_edit = session.text().to_owned();
    assert_ne!(after_edit, original);

    // Undo + Redo
    feed(&mut session, "u");
    assert_eq!(session.text(), original);
    feed(&mut session, "<C-r>");

    assert_eq!(
        session.text(),
        after_edit,
        "BUG PROVEN: Redo after visual block `c` does not restore the edited text.\n\
         Expected (after edit): {:?}\n\
         Got (after redo):      {:?}\n\
         Root cause: mismatched undo groups in block_visual.rs Change path.",
        after_edit,
        session.text(),
    );
}

// =============================================================================
// TEST 3: Visual block `s` with single char -- redo still broken
// =============================================================================

/// Even with a single character typed, the redo bug manifests.
#[test]
fn block_visual_s_single_char_redo_broken() {
    let original = "aaa\nbbb\nccc";
    let mut session = HostSession::new(original);

    // Select first column of all 3 lines
    feed(&mut session, "<C-v>jj");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Block));

    // Substitute with single char
    feed(&mut session, "s");
    feed(&mut session, "X<Esc>");

    let after_edit = session.text().to_owned();
    assert_ne!(after_edit, original);

    // Undo + Redo
    feed(&mut session, "u");
    assert_eq!(session.text(), original);
    feed(&mut session, "<C-r>");

    assert_eq!(
        session.text(),
        after_edit,
        "BUG PROVEN: Redo after single-char block `s` loses the typed character.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        after_edit,
        session.text(),
    );
}

// =============================================================================
// TEST 4: Contrast -- block `I` redo works correctly
// =============================================================================

/// Visual block `I` (insert before) redo works correctly.
/// This proves the test infrastructure is sound and the bug is specific to `s`/`c`.
///
/// In visual_block.rs (I/A path): begin_undo() is left OPEN, then exit_finalize
/// closes it -- forming a single atomic undo group with correct redo data.
///
/// In block_visual.rs (c/s path): begin_undo() + delete + end_undo() CLOSES
/// the group prematurely, then begin_insert without begin_undo -- mismatched.
#[test]
fn block_visual_I_redo_works_correctly_contrast() {
    let original = "hello\nworld\nhappy";
    let mut session = HostSession::new(original);

    // Enter visual block mode, select 3 lines x 1 column
    feed(&mut session, "<C-v>jj");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Block));

    // Press `I` to insert before the block, type "XYZ", exit
    feed(&mut session, "I");
    assert_eq!(session.mode(), Mode::Insert);
    feed(&mut session, "XYZ<Esc>");
    assert_eq!(session.mode(), Mode::Normal);

    let after_edit = session.text().to_owned();
    assert_ne!(after_edit, original);

    // Undo + Redo -- both should work correctly for `I`
    feed(&mut session, "u");
    assert_eq!(
        session.text(),
        original,
        "Block `I` undo should restore original"
    );
    feed(&mut session, "<C-r>");
    assert_eq!(
        session.text(),
        after_edit,
        "Block `I` redo should restore the edited text.\n\
         If this fails, the test infrastructure is broken.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        after_edit,
        session.text(),
    );
}

// =============================================================================
// TEST 5: Multiple undo/redo cycles expose corruption
// =============================================================================

/// Performing multiple undo/redo cycles with the broken groups eventually
/// corrupts the text state, proving the structural issue.
#[test]
fn block_visual_s_multiple_undo_redo_cycles() {
    let original = "hello\nworld\nhappy";
    let mut session = HostSession::new(original);

    // Perform visual block s
    feed(&mut session, "<C-v>jjll");
    feed(&mut session, "s");
    feed(&mut session, "XYZ<Esc>");
    let after_edit = session.text().to_owned();

    // Cycle 1: u then Ctrl-R
    feed(&mut session, "u");
    let after_undo1 = session.text().to_owned();
    feed(&mut session, "<C-r>");
    let after_redo1 = session.text().to_owned();

    // Cycle 2: u then Ctrl-R again
    feed(&mut session, "u");
    let after_undo2 = session.text().to_owned();
    feed(&mut session, "<C-r>");
    let after_redo2 = session.text().to_owned();

    // For correct behavior:
    // - All undos should return to `original`
    // - All redos should return to `after_edit`
    // - The cycles should be idempotent
    assert_eq!(after_undo1, original, "First undo should restore original");
    assert_eq!(after_undo2, original, "Second undo should restore original");

    // This is where the bug shows: redos don't match the original edit
    let redo_correct = after_redo1 == after_edit && after_redo2 == after_edit;
    assert!(
        redo_correct,
        "BUG PROVEN: Undo/redo cycles are not idempotent for visual block `s`.\n\
         Original:    {:?}\n\
         After edit:  {:?}\n\
         After redo1: {:?}\n\
         After redo2: {:?}\n\
         \n\
         Correct behavior: all redos should produce {:?}",
        original, after_edit, after_redo1, after_redo2, after_edit,
    );
}

// =============================================================================
// TEST 6: Dot repeat after block `s` may also be affected
// =============================================================================

/// After visual block `s` + type + Escape, the `.` command should repeat
/// the full operation (delete block + insert typed text). If the undo group
/// structure is broken, dot repeat might only replay the delete OR only
/// the insert -- not both.
#[test]
fn block_visual_s_dot_repeat_replays_full_operation() {
    let original = "hello\nworld\nhappy";
    let mut session = HostSession::new(original);

    // First operation: visual block s on first 3 chars of all lines
    feed(&mut session, "<C-v>jjll");
    feed(&mut session, "s");
    feed(&mut session, "XYZ<Esc>");
    let after_first = session.text().to_owned();

    // Undo to get back to original
    feed(&mut session, "u");
    assert_eq!(session.text(), original);

    // Now do visual block selection again and use `.` to repeat
    feed(&mut session, "<C-v>jjll");
    feed(&mut session, ".");

    // Dot should reproduce the exact same result as the first operation
    assert_eq!(
        session.text(),
        after_first,
        "BUG (possible): Dot repeat after visual block `s` does not reproduce the operation.\n\
         Expected (first op result): {:?}\n\
         Got (after dot repeat):     {:?}\n\
         \n\
         If this fails, the undo group mismatch also corrupts dot-repeat recording.",
        after_first,
        session.text(),
    );
}

// =============================================================================
// TEST 7: Block `c` with multi-char replacement -- redo loses typed text
// =============================================================================

/// Visual block `c` with a longer replacement string also has broken redo.
/// The typed text "HELLO" should be part of the redo, but it is lost.
#[test]
fn block_visual_c_multichar_redo_loses_typed_text() {
    let original = "xxxx\nyyyy\nzzzz";
    let mut session = HostSession::new(original);

    // Select 2-char wide block on all 3 lines
    feed(&mut session, "<C-v>jjl");
    assert_eq!(session.mode(), Mode::Visual(VisualType::Block));

    // Change block and type longer replacement
    feed(&mut session, "c");
    feed(&mut session, "HELLO<Esc>");

    let after_edit = session.text().to_owned();
    assert_ne!(after_edit, original);

    // Undo + Redo
    feed(&mut session, "u");
    assert_eq!(session.text(), original, "Undo should restore original");
    feed(&mut session, "<C-r>");

    assert_eq!(
        session.text(),
        after_edit,
        "BUG PROVEN: Redo after block `c` with multi-char replacement loses typed text.\n\
         Expected: {:?}\n\
         Got:      {:?}",
        after_edit,
        session.text(),
    );
}
