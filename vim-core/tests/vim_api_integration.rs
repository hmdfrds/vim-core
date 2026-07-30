//! Integration and system tests for the vim-core Universal API.
//!
//! These tests prove the VimApi layer works end-to-end with real engine state,
//! real keystroke processing, and real effect application.

use vim_core::effects::Effect;
use vim_core::execution::{ApiError, HostSession, InvocationContext, VimApi};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{
    CallerId, CapabilityTier, Mode, MotionType, SearchDirection, SelectionShape, VimValue,
};

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
// 1. BASIC API ACCESS — prove all domain views return correct initial state
// ===========================================================================

#[test]
fn api_buffer_view_reads_document_text() {
    let session = HostSession::new("hello\nworld\nfoo");
    let api = VimApi::from_session(&session, host_ctx());

    let buf = api.buffer();
    assert_eq!(buf.text(), "hello\nworld\nfoo");
    assert_eq!(buf.len(), 15);
    assert_eq!(buf.line_count(), 3);
    assert!(!buf.is_empty());
}

#[test]
fn api_buffer_view_line_access() {
    let session = HostSession::new("alpha\nbeta\ngamma");
    let api = VimApi::from_session(&session, host_ctx());
    let buf = api.buffer();

    assert_eq!(buf.line(0), Some("alpha\n"));
    assert_eq!(buf.line(1), Some("beta\n"));
    assert_eq!(buf.line(2), Some("gamma"));
    assert_eq!(buf.line(3), None);

    assert_eq!(buf.line_start(0), 0);
    assert_eq!(buf.line_start(1), 6);
    assert_eq!(buf.line_start(2), 11);

    assert_eq!(buf.line_end(0), 5);
    assert_eq!(buf.line_end(1), 10);
    assert_eq!(buf.line_end(2), 16);

    assert_eq!(buf.line_number(0), 0);
    assert_eq!(buf.line_number(6), 1);
    assert_eq!(buf.line_number(11), 2);
}

#[test]
fn api_buffer_view_slice_clamps() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    let buf = api.buffer();

    assert_eq!(buf.slice(0, 5), "hello");
    assert_eq!(buf.slice(0, 100), "hello");
    assert_eq!(buf.slice(3, 5), "lo");
    assert_eq!(buf.slice(5, 5), "");
}

#[test]
fn api_buffer_view_utf8() {
    let session = HostSession::new("café");
    let api = VimApi::from_session(&session, host_ctx());
    let buf = api.buffer();

    assert_eq!(buf.char_at(0), Some('c'));
    assert_eq!(buf.char_at(3), Some('é'));
    assert_eq!(buf.char_len_at(3), 2); // é is 2 bytes in UTF-8
    assert_eq!(buf.next_char_boundary(3), 5);
    assert_eq!(buf.prev_char_boundary(5), 3);
}

#[test]
fn api_cursor_view_initial_position() {
    let session = HostSession::new("hello\nworld");
    let api = VimApi::from_session(&session, host_ctx());

    assert_eq!(api.cursor().offset(), 0);
    assert!(api.cursor().selection().is_none());
}

#[test]
fn api_state_view_initial() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    let st = api.state();

    assert_eq!(st.mode(), Mode::Normal);
    assert!(st.search_pattern().is_none());
    assert!(!st.is_recording());
}

#[test]
fn api_options_view_defaults() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    let opts = api.options();

    assert!(opts.tabstop() > 0);
    assert!(opts.shiftwidth() > 0);
}

// ===========================================================================
// 2. POST-EDIT STATE — process keys, then verify API sees updated state
// ===========================================================================

#[test]
fn api_sees_cursor_after_motion() {
    let mut session = HostSession::new("hello world");
    feed_keys(&mut session, "w");

    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(api.cursor().offset(), 6); // 'w' moves to "world"
    assert_eq!(api.buffer().text(), "hello world");
}

#[test]
fn api_sees_text_after_insert() {
    let mut session = HostSession::new("hello");
    feed_keys(&mut session, "A world\x1b");

    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(api.buffer().text(), "hello world");
    assert_eq!(api.state().mode(), Mode::Normal);
}

#[test]
fn api_sees_registers_after_yank() {
    let mut session = HostSession::new("hello world");
    feed_keys(&mut session, "yiw"); // yank inner word "hello"

    let api = VimApi::from_session(&session, host_ctx());
    let regs = api.registers();

    let yanked = regs.get('"');
    assert!(yanked.is_some(), "unnamed register should have yanked text");
    assert_eq!(yanked.unwrap(), "hello");
}

#[test]
fn api_sees_marks_after_mark_set() {
    let mut session = HostSession::new("hello\nworld");
    feed_keys(&mut session, "ma"); // set mark 'a' at position 0

    let api = VimApi::from_session(&session, host_ctx());
    let marks = api.marks();

    assert!(marks.get('a').is_some(), "mark 'a' should be set");
    assert_eq!(marks.get('a').unwrap(), 0);
}

#[test]
fn api_sees_search_state() {
    let mut session = HostSession::new("hello world hello");
    feed_keys(&mut session, "/world\r");

    let api = VimApi::from_session(&session, host_ctx());
    let st = api.state();

    assert_eq!(st.search_pattern(), Some("world"));
    assert_eq!(st.search_direction(), SearchDirection::Forward);
}

#[test]
fn api_sees_mode_changes() {
    let mut session = HostSession::new("hello");

    // Enter insert mode
    feed_keys(&mut session, "i");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(api.state().mode(), Mode::Insert);

    // Back to normal
    feed_keys(&mut session, "\x1b");
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(api.state().mode(), Mode::Normal);
}

// ===========================================================================
// 3. EFFECT EMISSION — prove emitter produces correct effects
// ===========================================================================

#[test]
fn api_emit_insert_produces_effect() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit().insert(5, " world").unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::Insert { offset, text } => {
            assert_eq!(offset.get(), 5);
            assert_eq!(text.as_str(), " world");
        }
        other => panic!("expected Insert effect, got {:?}", other),
    }
}

#[test]
fn api_emit_delete_produces_effect() {
    let session = HostSession::new("hello world");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit().delete(5, 11).unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::Delete { range } => {
            assert_eq!(range.start().get(), 5);
            assert_eq!(range.end().get(), 11);
        }
        other => panic!("expected Delete effect, got {:?}", other),
    }
}

#[test]
fn api_emit_replace_produces_effect() {
    let session = HostSession::new("hello world");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit().replace(0, 5, "goodbye").unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::Replace { range, text } => {
            assert_eq!(range.start().get(), 0);
            assert_eq!(range.end().get(), 5);
            assert_eq!(text.as_str(), "goodbye");
        }
        other => panic!("expected Replace effect, got {:?}", other),
    }
}

#[test]
fn api_emit_cursor_produces_effect() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit().set_cursor(3).unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::SetCursor { offset } => {
            assert_eq!(offset.get(), 3);
        }
        other => panic!("expected SetCursor effect, got {:?}", other),
    }
}

#[test]
fn api_emit_set_register_produces_effect() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit()
        .set_register('a', "yanked text", MotionType::CharWise)
        .unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::SetRegister {
            name,
            text,
            motion_type,
        } => {
            assert_eq!(name.char(), 'a');
            assert_eq!(text.as_str(), "yanked text");
            assert_eq!(motion_type, &MotionType::CharWise);
        }
        other => panic!("expected SetRegister effect, got {:?}", other),
    }
}

#[test]
fn api_emit_set_mark_produces_effect() {
    let session = HostSession::new("hello\nworld");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit().set_mark('a', 6).unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::SetMark { name, offset, .. } => {
            assert_eq!(name.char(), 'a');
            assert_eq!(offset.get(), 6);
        }
        other => panic!("expected SetMark effect, got {:?}", other),
    }
}

#[test]
fn api_emit_multiple_effects_preserves_order() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit().insert(0, "A").unwrap();
    api.emit().insert(1, "B").unwrap();
    api.emit().set_cursor(2).unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 3);
    assert!(matches!(&effects[0], Effect::Insert { text, .. } if text.as_str() == "A"));
    assert!(matches!(&effects[1], Effect::Insert { text, .. } if text.as_str() == "B"));
    assert!(matches!(&effects[2], Effect::SetCursor { .. }));
}

#[test]
fn api_emit_undo_group() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit().begin_undo_group().unwrap();
    api.emit().insert(0, "X").unwrap();
    api.emit().delete(1, 2).unwrap();
    api.emit().end_undo_group().unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 4);
    assert!(matches!(&effects[0], Effect::BeginUndoGroup { .. }));
    assert!(matches!(&effects[1], Effect::Insert { .. }));
    assert!(matches!(&effects[2], Effect::Delete { .. }));
    assert!(matches!(&effects[3], Effect::EndUndoGroup { .. }));
}

// ===========================================================================
// 4. TIER ENFORCEMENT — ReadOnly blocks mutations
// ===========================================================================

#[test]
fn readonly_blocks_insert() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    let result = api.emit().insert(0, "x");
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        ApiError::InsufficientTier {
            required: CapabilityTier::Mutating,
            have: CapabilityTier::ReadOnly,
        }
    );
}

#[test]
fn readonly_blocks_delete() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert!(api.emit().delete(0, 1).is_err());
}

#[test]
fn readonly_blocks_replace() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert!(api.emit().replace(0, 1, "x").is_err());
}

#[test]
fn readonly_blocks_set_mode() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert!(api.emit().set_mode(Mode::Insert).is_err());
}

#[test]
fn readonly_blocks_undo_group() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert!(api.emit().begin_undo_group().is_err());
    assert!(api.emit().end_undo_group().is_err());
}

#[test]
fn readonly_blocks_set_register() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert!(api
        .emit()
        .set_register('a', "text", MotionType::CharWise)
        .is_err());
}

#[test]
fn readonly_blocks_set_mark() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert!(api.emit().set_mark('a', 0).is_err());
}

#[test]
fn readonly_allows_cursor_movement() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert!(api.emit().set_cursor(3).is_ok());
    assert!(api.emit().set_selection(0, 3, SelectionShape::Char).is_ok());
}

#[test]
fn readonly_allows_messages() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert!(api.emit().message("info").is_ok());
    assert!(api.emit().error("oops").is_ok());
}

#[test]
fn readonly_allows_all_reads() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    assert_eq!(api.buffer().text(), "hello");
    assert_eq!(api.cursor().offset(), 0);
    assert_eq!(api.state().mode(), Mode::Normal);
    assert!(api.options().tabstop() > 0);
}

// ===========================================================================
// 5. VALIDATION — invalid register/mark chars
// ===========================================================================

#[test]
fn emit_set_register_rejects_invalid_name() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    let result = api.emit().set_register('!', "text", MotionType::CharWise);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), ApiError::RegisterNotFound('!'));
}

#[test]
fn emit_set_mark_rejects_invalid_name() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    let result = api.emit().set_mark('!', 0);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), ApiError::MarkNotFound('!'));
}

// ===========================================================================
// 6. CALLER ID — metadata tracking
// ===========================================================================

#[test]
fn api_reports_caller_id() {
    let session = HostSession::new("hello");

    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(*api.caller_id(), CallerId::Host);
    assert_eq!(api.capability_tier(), CapabilityTier::Mutating);

    let api = VimApi::from_session(&session, readonly_ctx());
    assert_eq!(*api.caller_id(), CallerId::Expression);
    assert_eq!(api.capability_tier(), CapabilityTier::ReadOnly);
}

// ===========================================================================
// 7. SYSTEM TEST — full plugin-like workflow
// ===========================================================================

#[test]
fn system_test_commentary_style_workflow() {
    // Simulate what a commentary plugin would do:
    // 1. Read buffer lines
    // 2. Check commentstring option
    // 3. Emit replacement effects to toggle comments

    let mut session = HostSession::new("hello\nworld\nfoo");
    // Move to second line
    feed_keys(&mut session, "j");

    let api = VimApi::from_session(&session, host_ctx());

    // 1. Read the current line
    let cursor_offset = api.cursor().offset();
    let line_num = api.buffer().line_number(cursor_offset);
    assert_eq!(line_num, 1);

    let line_start = api.buffer().line_start(line_num);
    let line_end = api.buffer().line_end(line_num);
    let line_text = api.buffer().slice(line_start, line_end);
    assert_eq!(line_text, "world");

    // 2. Read commentstring
    let _cs = api.options().commentstring();

    // 3. Emit a replacement (prepend "// ")
    let commented = format!("// {}", line_text);
    api.emit()
        .replace(line_start, line_end, &commented)
        .unwrap();

    // 4. Verify the effect was accumulated
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::Replace { range, text } => {
            assert_eq!(range.start().get(), line_start);
            assert_eq!(range.end().get(), line_end);
            assert_eq!(text.as_str(), "// world");
        }
        other => panic!("expected Replace, got {:?}", other),
    }
}

#[test]
fn system_test_surround_style_workflow() {
    // Simulate what a surround plugin would do:
    // 1. Get the operator range (simulated as word under cursor)
    // 2. Insert closing delimiter after range end
    // 3. Insert opening delimiter before range start

    let session = HostSession::new("hello world");
    let api = VimApi::from_session(&session, host_ctx());

    // Simulate operator range: "hello" = bytes 0..5
    let range_start = 0;
    let range_end = 5;
    let word = api.buffer().slice(range_start, range_end);
    assert_eq!(word, "hello");

    // Insert in reverse order (highest offset first) so offsets stay valid
    api.emit().begin_undo_group().unwrap();
    api.emit().insert(range_end, ")").unwrap();
    api.emit().insert(range_start, "(").unwrap();
    api.emit().end_undo_group().unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 4);
    assert!(matches!(&effects[0], Effect::BeginUndoGroup { .. }));
    assert!(matches!(&effects[1], Effect::Insert { text, .. } if text.as_str() == ")"));
    assert!(matches!(&effects[2], Effect::Insert { text, .. } if text.as_str() == "("));
    assert!(matches!(&effects[3], Effect::EndUndoGroup { .. }));
}

#[test]
fn system_test_autoformat_on_save_workflow() {
    // Simulate an autocmd handler for BufWrite:
    // 1. Read entire buffer text
    // 2. "Format" it (trim trailing whitespace)
    // 3. Replace entire buffer

    let session = HostSession::new("hello   \nworld  \nfoo");
    let api = VimApi::from_session(&session, host_ctx());

    let original = api.buffer().text();
    let formatted: String = original
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(formatted, "hello\nworld\nfoo");

    api.emit()
        .replace(0, api.buffer().len(), &formatted)
        .unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::Replace { range, text } => {
            assert_eq!(range.start().get(), 0);
            assert_eq!(text.as_str(), "hello\nworld\nfoo");
        }
        other => panic!("expected Replace, got {:?}", other),
    }
}

#[test]
fn system_test_read_after_complex_editing() {
    // Prove the API sees real engine state after a complex editing sequence
    let mut session = HostSession::new("line one\nline two\nline three");

    // Delete first line, yank second, search for "three"
    feed_keys(&mut session, "dd");
    feed_keys(&mut session, "yy");
    feed_keys(&mut session, "/three\r");

    let api = VimApi::from_session(&session, host_ctx());

    // Buffer should have 2 lines now (first was deleted)
    assert_eq!(api.buffer().line_count(), 2);
    assert!(api.buffer().text().starts_with("line two"));

    // Unnamed register should have the yanked line
    let yanked = api.registers().get('"');
    assert!(yanked.is_some());
    assert!(yanked.unwrap().contains("line two"));

    // Search should be active
    assert_eq!(api.state().search_pattern(), Some("three"));
    assert_eq!(api.state().search_direction(), SearchDirection::Forward);
}

#[test]
fn system_test_multiple_api_instances_independent() {
    // Prove that two VimApi instances from the same session don't interfere
    let session = HostSession::new("hello");

    let api1 = VimApi::from_session(&session, host_ctx());
    let api2 = VimApi::from_session(&session, readonly_ctx());

    // Both read the same state
    assert_eq!(api1.buffer().text(), api2.buffer().text());

    // api1 can emit (mutating), api2 cannot
    assert!(api1.emit().insert(0, "x").is_ok());
    assert!(api2.emit().insert(0, "x").is_err());

    // Effects are independent
    let effects1 = api1.drain_effects();
    let effects2 = api2.drain_effects();
    assert_eq!(effects1.len(), 1);
    assert_eq!(effects2.len(), 0);
}

// ===========================================================================
// 8. LAST COMMAND + OPTION GET — final spec gap closers
// ===========================================================================

#[test]
fn api_sees_last_command_after_ex() {
    let mut session = HostSession::new("hello\nworld");
    feed_keys(&mut session, ":set tabstop=4\r");

    let api = VimApi::from_session(&session, host_ctx());
    let st = api.state();
    let last = st.last_command();
    assert!(
        last.is_some(),
        "last_command should be set after ex command"
    );
    assert!(
        last.unwrap().contains("tabstop"),
        "last_command should contain the ex command text"
    );
}

#[test]
fn api_option_get_by_name() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    let opts = api.options();

    // Full name
    let ts = opts.get("tabstop");
    assert!(ts.is_some());
    assert!(matches!(ts.unwrap(), VimValue::Int(_)));

    // Abbreviation
    let et = opts.get("et");
    assert!(et.is_some());
    assert!(matches!(et.unwrap(), VimValue::Bool(_)));

    // Unknown option
    assert!(opts.get("nonexistent").is_none());

    // String option
    let isk = opts.get("iskeyword");
    assert!(isk.is_some());
    assert!(matches!(isk.unwrap(), VimValue::String(_)));
}

#[test]
fn api_empty_document() {
    let session = HostSession::new("");
    let api = VimApi::from_session(&session, host_ctx());

    assert_eq!(api.buffer().text(), "");
    assert_eq!(api.buffer().len(), 0);
    assert!(api.buffer().is_empty());
    assert_eq!(api.buffer().line_count(), 1);
    assert_eq!(api.cursor().offset(), 0);
    assert!(api.buffer().char_at(0).is_none());
    assert_eq!(api.buffer().line(0), Some(""));
    assert!(api.buffer().find_forward(0, "x").is_none());
}
