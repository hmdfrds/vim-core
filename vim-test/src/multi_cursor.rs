//! Multi-cursor testing utilities.

use crate::session::TestSession;

/// Assert all cursor positions match expected `(line, col)` pairs.
///
/// Primary cursor is the first element.
#[track_caller]
pub fn assert_cursors(session: &TestSession, expected: &[(usize, usize)]) {
    let actual: Vec<(usize, usize)> = session
        .cursor_positions()
        .iter()
        .map(|&(line, col, _)| (line, col))
        .collect();

    assert!(
        actual == expected,
        "MULTI-CURSOR POSITION MISMATCH\n\
         \x20 expected: {expected:?}\n\
         \x20 actual:   {actual:?}\n\
         \x20 cursor_count: {}",
        session.cursor_count()
    );
}

/// Assert the number of active cursors.
#[track_caller]
pub fn assert_cursor_count(session: &TestSession, expected: usize) {
    let actual = session.cursor_count();
    assert!(
        actual == expected,
        "CURSOR COUNT MISMATCH\n\
         \x20 expected: {expected}\n\
         \x20 actual:   {actual}"
    );
}
