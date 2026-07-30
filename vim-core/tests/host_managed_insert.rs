// Regression tests for NativeInsert-specific issues (host-managed insert path).

use vim_core::execution::HostSession;
use vim_core::keymap::KeyEvent;

#[test]
fn record_insert_text_does_not_corrupt_last_inserted_text() {
    let mut session = HostSession::new("world\n");
    session.process_key_host(KeyEvent::char('i'));

    session.record_insert_text("h");
    session.record_insert_text("e");
    session.record_insert_text("l");
    session.record_insert_text("l");
    session.record_insert_text("o");

    // record_insert_text must NOT touch last_inserted_text directly.
    // The authoritative store happens in handle_insert_exit from accumulated_text.
    // Before the fix, each call overwrote last_inserted_text with the fragment.
    let last = session.engine().state().last_inserted_text();
    assert_ne!(
        last, "o",
        "last_inserted_text should not be overwritten with each fragment"
    );
}

#[test]
fn record_insert_text_sets_had_text_mutation() {
    let mut session = HostSession::new("hello\n");
    session.process_key_host(KeyEvent::char('i'));

    session.record_insert_text("x");

    let had_mutation = session
        .engine()
        .state()
        .insert_state()
        .map(|is| is.had_text_mutation())
        .unwrap_or(false);
    assert!(
        had_mutation,
        "had_text_mutation must be true after record_insert_text"
    );
}
