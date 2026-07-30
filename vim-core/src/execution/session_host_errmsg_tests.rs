//! Tests for the `errmsg()` / `last_error` field on `VimSession<SessionHost>`.

use crate::execution::parse_keys_from_string;
use crate::execution::session_host::SessionHost;
use crate::execution::VimSession;

fn process_keys(session: &mut VimSession<SessionHost>, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

#[test]
fn errmsg_returns_motion_failed_on_j_at_last_line() {
    let mut s = VimSession::<SessionHost>::new("only line");
    process_keys(&mut s, "j");
    assert_eq!(s.errmsg().as_deref(), Some("motion failed"));
}

#[test]
fn errmsg_returns_e_coded_error() {
    let mut s = VimSession::<SessionHost>::new("hello");
    process_keys(&mut s, "/zzzzz\n");
    let msg = s.errmsg().expect("should have error");
    assert!(msg.starts_with("E486:"), "got: {msg}");
}

#[test]
fn errmsg_cleared_after_successful_command() {
    let mut s = VimSession::<SessionHost>::new("line one\nline two");
    // Trigger an error first.
    process_keys(&mut s, "/zzzzz\n");
    assert!(s.errmsg().is_some());
    // Successful motion should clear the error.
    process_keys(&mut s, "j");
    assert_eq!(s.errmsg(), None);
}

#[test]
fn errmsg_returns_last_error_in_sequence() {
    let mut s = VimSession::<SessionHost>::new("only line");
    // First error: pattern not found.
    process_keys(&mut s, "/zzzzz\n");
    let first = s.errmsg().expect("should have error");
    assert!(first.starts_with("E486:"));
    // Second error: motion failed (j at last line).
    process_keys(&mut s, "j");
    assert_eq!(s.errmsg().as_deref(), Some("motion failed"));
}

#[test]
fn errmsg_no_previous_pattern_on_n() {
    // `n` with no previous search produces E35: No previous regular expression.
    let mut s = VimSession::<SessionHost>::new("hello");
    process_keys(&mut s, "n");
    let msg = s.errmsg().expect("should have error");
    assert!(msg.starts_with("E35:"), "got: {msg}");
}

#[test]
fn errmsg_mark_not_set() {
    let mut s = VimSession::<SessionHost>::new("hello");
    // Try to jump to mark 'z' which was never set.
    process_keys(&mut s, "'z");
    let msg = s.errmsg().expect("should have error");
    assert!(msg.starts_with("E20:"), "got: {msg}");
}

#[test]
fn errmsg_motion_failed_k_at_first_line() {
    let mut s = VimSession::<SessionHost>::new("only line");
    process_keys(&mut s, "k");
    assert_eq!(s.errmsg().as_deref(), Some("motion failed"));
}

#[test]
fn errmsg_cleared_per_keystroke() {
    let mut s = VimSession::<SessionHost>::new("line one\nline two\nline three");
    // Trigger error.
    process_keys(&mut s, "/zzzzz\n");
    assert!(s.errmsg().is_some());
    // Move down (success) — error should clear.
    process_keys(&mut s, "j");
    assert_eq!(s.errmsg(), None);
    // Move down again (success).
    process_keys(&mut s, "j");
    assert_eq!(s.errmsg(), None);
    // Now at last line, j fails.
    process_keys(&mut s, "j");
    assert_eq!(s.errmsg().as_deref(), Some("motion failed"));
}
