//! Integration tests for the FlatApi projection layer.
//!
//! Verifies that FlatApi functions work correctly with a real HostSession,
//! including type conversions, sentinel values, scope parsing, and effect emission.

use vim_core::execution::{FlatApi, HostSession, InvocationContext, VimApi};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{CallerId, CapabilityTier, VimValue};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn host_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Host, CapabilityTier::Mutating)
}

fn readonly_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Expression, CapabilityTier::ReadOnly)
}

fn feed_keys(session: &mut HostSession, keys: &str) {
    for ch in keys.chars() {
        let key = match ch {
            '\x1b' => KeyEvent::escape(),
            '\r' => KeyEvent::enter(),
            _ => KeyEvent::char(ch),
        };
        let _ = session.process_key_host(key);
    }
}

// ===========================================================================
// 1. BUFFER DOMAIN — verify all buffer flat functions
// ===========================================================================

#[test]
fn flat_buf_text_returns_full_document() {
    let session = HostSession::new("hello\nworld");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_text(&api), "hello\nworld");
}

#[test]
fn flat_buf_len_returns_i64() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    let len: i64 = FlatApi::buf_len(&api);
    assert_eq!(len, 5);
}

#[test]
fn flat_buf_line_count_multiline() {
    let session = HostSession::new("a\nb\nc\nd");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_line_count(&api), 4);
}

#[test]
fn flat_buf_line_includes_newline() {
    let session = HostSession::new("first\nsecond\nthird");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_line(&api, 0).unwrap(), "first\n");
    assert_eq!(FlatApi::buf_line(&api, 2).unwrap(), "third");
}

#[test]
fn flat_buf_line_error_on_invalid() {
    let session = HostSession::new("one");
    let api = VimApi::from_session(&session, host_ctx());
    let err = FlatApi::buf_line(&api, 99).unwrap_err();
    assert_eq!(err.as_str(), "line out of range");
}

#[test]
fn flat_buf_line_at_returns_current_line() {
    let session = HostSession::new("alpha\nbeta\ngamma");
    let api = VimApi::from_session(&session, host_ctx());
    // offset 7 is within "beta"
    assert_eq!(FlatApi::buf_line_at(&api, 7), "beta");
}

#[test]
fn flat_buf_line_number_returns_i64() {
    let session = HostSession::new("abc\ndef\nghi");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_line_number(&api, 4), 1); // 'd' is on line 1
}

#[test]
fn flat_buf_line_start_and_end() {
    let session = HostSession::new("foo\nbar\nbaz");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_line_start(&api, 1), 4);
    assert_eq!(FlatApi::buf_line_end(&api, 1), 7);
}

#[test]
fn flat_buf_slice_clamps_to_bounds() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_slice(&api, 0, 100), "hello");
    assert_eq!(FlatApi::buf_slice(&api, 2, 4), "ll");
}

#[test]
fn flat_buf_char_at_utf8() {
    let session = HostSession::new("caf\u{00e9}");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_char_at(&api, 3).unwrap(), "\u{00e9}");
}

#[test]
fn flat_buf_char_at_out_of_range_returns_err() {
    let session = HostSession::new("ab");
    let api = VimApi::from_session(&session, host_ctx());
    assert!(FlatApi::buf_char_at(&api, 10).is_err());
}

#[test]
fn flat_buf_char_len_at_multibyte() {
    let session = HostSession::new("\u{1F600}"); // smiley emoji, 4 bytes
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_char_len_at(&api, 0), 4);
}

#[test]
fn flat_buf_is_word_byte_boundaries() {
    let session = HostSession::new("a_1 .");
    let api = VimApi::from_session(&session, host_ctx());
    assert!(FlatApi::buf_is_word_byte(&api, 0)); // 'a'
    assert!(FlatApi::buf_is_word_byte(&api, 1)); // '_'
    assert!(FlatApi::buf_is_word_byte(&api, 2)); // '1'
    assert!(!FlatApi::buf_is_word_byte(&api, 3)); // ' '
    assert!(!FlatApi::buf_is_word_byte(&api, 4)); // '.'
}

#[test]
fn flat_buf_find_forward_sentinel() {
    let session = HostSession::new("aaa bbb ccc");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_find_forward(&api, 0, "bbb"), 4);
    assert_eq!(FlatApi::buf_find_forward(&api, 5, "bbb"), -1);
}

#[test]
fn flat_buf_find_backward_sentinel() {
    let session = HostSession::new("aaa bbb aaa");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_find_backward(&api, 11, "aaa"), 8);
    assert_eq!(FlatApi::buf_find_backward(&api, 3, "bbb"), -1);
}

#[test]
fn flat_buf_word_at_finds_word() {
    let session = HostSession::new("foo_bar baz");
    let api = VimApi::from_session(&session, host_ctx());
    let (start, end) = FlatApi::buf_word_at(&api, 2).unwrap();
    assert_eq!(start, 0);
    assert_eq!(end, 7); // "foo_bar"
}

#[test]
fn flat_buf_word_at_not_on_word() {
    let session = HostSession::new("a b");
    let api = VimApi::from_session(&session, host_ctx());
    let err = FlatApi::buf_word_at(&api, 1).unwrap_err();
    assert_eq!(err.as_str(), "no word at offset");
}

#[test]
fn flat_buf_line_indent_spaces() {
    let session = HostSession::new("  foo\n    bar");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::buf_line_indent(&api, 0), 2);
    assert_eq!(FlatApi::buf_line_indent(&api, 1), 4);
}

// ===========================================================================
// 2. CURSOR DOMAIN
// ===========================================================================

#[test]
fn flat_cursor_offset_after_motion() {
    let mut session = HostSession::new("hello world");
    feed_keys(&mut session, "w");
    let api = VimApi::from_session(&session, host_ctx());
    // After 'w', cursor should be at start of "world" (offset 6)
    assert!(FlatApi::cursor_offset(&api) > 0);
}

// ===========================================================================
// 3. STATE DOMAIN
// ===========================================================================

#[test]
fn flat_mode_initial_is_normal() {
    let session = HostSession::new("test");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::mode(&api), "Normal");
}

#[test]
fn flat_search_pattern_after_search() {
    let mut session = HostSession::new("hello world hello");
    feed_keys(&mut session, "/hello\r");
    let api = VimApi::from_session(&session, host_ctx());
    let pat = FlatApi::search_pattern(&api);
    assert!(pat.is_ok());
    assert_eq!(pat.unwrap(), "hello");
}

#[test]
fn flat_search_pattern_none_initially() {
    let session = HostSession::new("test");
    let api = VimApi::from_session(&session, host_ctx());
    assert!(FlatApi::search_pattern(&api).is_err());
}

#[test]
fn flat_is_recording_false_by_default() {
    let session = HostSession::new("test");
    let api = VimApi::from_session(&session, host_ctx());
    assert!(!FlatApi::is_recording(&api));
}

// ===========================================================================
// 4. OPTIONS DOMAIN
// ===========================================================================

#[test]
fn flat_options_return_positive_defaults() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    assert!(FlatApi::opt_shiftwidth(&api) > 0);
    assert!(FlatApi::opt_tabstop(&api) > 0);
}

#[test]
fn flat_opt_commentstring_non_empty() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    let cs = FlatApi::opt_commentstring(&api);
    assert!(!cs.is_empty());
}

#[test]
fn flat_opt_iskeyword_non_empty() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    let isk = FlatApi::opt_iskeyword(&api);
    assert!(!isk.is_empty());
}

// ===========================================================================
// 5. REGISTER DOMAIN
// ===========================================================================

#[test]
fn flat_reg_get_after_yank() {
    let mut session = HostSession::new("hello");
    feed_keys(&mut session, "yiw");
    let api = VimApi::from_session(&session, host_ctx());
    // After yiw, the unnamed register should have "hello"
    let result = FlatApi::reg_get(&api, "\"");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "hello");
}

#[test]
fn flat_reg_get_invalid_empty_name() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    let err = FlatApi::reg_get(&api, "").unwrap_err();
    assert_eq!(err.as_str(), "empty register name");
}

// ===========================================================================
// 6. MARK DOMAIN
// ===========================================================================

#[test]
fn flat_mark_get_after_set() {
    let mut session = HostSession::new("hello world");
    feed_keys(&mut session, "llma"); // move to offset 2, set mark 'a'
    let api = VimApi::from_session(&session, host_ctx());
    let offset = FlatApi::mark_get(&api, "a");
    assert!(offset >= 0);
}

#[test]
fn flat_mark_get_unset_returns_minus_one() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(FlatApi::mark_get(&api, "z"), -1);
}

// ===========================================================================
// 7. VARIABLE DOMAIN
// ===========================================================================

#[test]
fn flat_var_get_nonexistent() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    assert!(FlatApi::var_get(&api, "g", "nope").is_err());
    assert!(FlatApi::var_get(&api, "buffer", "nope").is_err());
}

#[test]
fn flat_var_get_invalid_scope() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    let err = FlatApi::var_get(&api, "w", "foo").unwrap_err();
    assert!(err.contains("unknown scope"));
}

#[test]
fn flat_var_exists_false_for_missing() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(
        FlatApi::var_exists(&api, "global", "missing").unwrap(),
        false
    );
    assert_eq!(FlatApi::var_exists(&api, "b", "missing").unwrap(), false);
}

// ===========================================================================
// 8. EFFECT EMISSION
// ===========================================================================

#[test]
fn flat_emit_insert_produces_effect() {
    let session = HostSession::new("abc");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_insert(&api, 3, "def").unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_delete_produces_effect() {
    let session = HostSession::new("abcdef");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_delete(&api, 0, 3).unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_replace_produces_effect() {
    let session = HostSession::new("hello world");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_replace(&api, 6, 11, "rust").unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_cursor_produces_effect() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_cursor(&api, 3).unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_message_and_error() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_message(&api, "info").unwrap();
    FlatApi::emit_error(&api, "bad").unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 2);
}

#[test]
fn flat_emit_set_register_charwise() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_set_register(&api, "a", "content", false).unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_set_register_linewise() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_set_register(&api, "b", "line\n", true).unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_set_mark_produces_effect() {
    let session = HostSession::new("hello world");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_set_mark(&api, "a", 5).unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_set_variable_global() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_set_variable(&api, "g", "myvar", VimValue::Int(42)).unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_set_variable_buffer() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_set_variable(&api, "buffer", "local", VimValue::Bool(true)).unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_delete_variable_produces_effect() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_delete_variable(&api, "g", "cleanup").unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_emit_undo_group_produces_two_effects() {
    let session = HostSession::new("x");
    let api = VimApi::from_session(&session, host_ctx());
    FlatApi::emit_begin_undo(&api).unwrap();
    FlatApi::emit_end_undo(&api).unwrap();
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 2);
}

#[test]
fn flat_emit_readonly_blocks_mutations() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());
    assert!(FlatApi::emit_insert(&api, 0, "x").is_err());
    assert!(FlatApi::emit_delete(&api, 0, 1).is_err());
    assert!(FlatApi::emit_replace(&api, 0, 1, "y").is_err());
    assert!(FlatApi::emit_set_register(&api, "a", "t", false).is_err());
    assert!(FlatApi::emit_set_mark(&api, "a", 0).is_err());
    assert!(FlatApi::emit_begin_undo(&api).is_err());
    assert!(FlatApi::emit_end_undo(&api).is_err());
}

#[test]
fn flat_emit_readonly_allows_cursor_and_messages() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());
    assert!(FlatApi::emit_cursor(&api, 2).is_ok());
    assert!(FlatApi::emit_message(&api, "hi").is_ok());
    assert!(FlatApi::emit_error(&api, "err").is_ok());
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 3);
}

// ===========================================================================
// 9. COMBINED WORKFLOWS
// ===========================================================================

#[test]
fn flat_combined_read_then_emit() {
    let session = HostSession::new("foo bar baz");
    let api = VimApi::from_session(&session, host_ctx());

    // Use flat API to find "bar" and replace it.
    let pos = FlatApi::buf_find_forward(&api, 0, "bar");
    assert_eq!(pos, 4);
    FlatApi::emit_replace(&api, pos, pos + 3, "qux").unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}

#[test]
fn flat_combined_word_boundaries_then_delete() {
    let session = HostSession::new("hello world");
    let api = VimApi::from_session(&session, host_ctx());

    let (start, end) = FlatApi::buf_word_at(&api, 7).unwrap();
    assert_eq!(start, 6);
    assert_eq!(end, 11);
    FlatApi::emit_delete(&api, start, end).unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
}
