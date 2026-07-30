//! Integration tests for diagnostic navigation ex commands (:cn, :cp, :cl, :cc).
//!
//! Verifies the full pipeline from parser through executor to HostRequest emission:
//! parse_ex_command → ExCommand::{CNext,CPrev,CList,CC} → executor → HostRequest::Diagnostic*.

#[path = "common/mod.rs"]
mod common;

use common::TestDocument;
use vim_core::execution::{HostRequest, InputContext, Response, VimEngine};
use vim_core::grammar::parse_ex_command;
use vim_core::grammar::types::ExCommand;
use vim_core::keymap::KeyEvent;

fn process_key(engine: &mut VimEngine, doc: &TestDocument, key: KeyEvent) -> Response {
    let ctx = InputContext::new(doc, doc.cursor_offset()).validate_clamped();
    engine.process(key, ctx)
}

fn run_command_line(engine: &mut VimEngine, doc: &TestDocument, command: &str) -> Response {
    let _ = process_key(engine, doc, KeyEvent::char(':'));
    for ch in command.chars() {
        let _ = process_key(engine, doc, KeyEvent::char(ch));
    }
    process_key(engine, doc, KeyEvent::enter())
}

// ═══════════════════════════════════════════════════════════════════════════════
// mutates_text: diagnostic commands must NOT mutate text
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_commands_do_not_mutate_text() {
    let commands = vec![
        ExCommand::CNext { count: 1 },
        ExCommand::CNext { count: 3 },
        ExCommand::CPrev { count: 1 },
        ExCommand::CPrev { count: 5 },
        ExCommand::CList,
        ExCommand::CC { index: None },
        ExCommand::CC { index: Some(2) },
    ];
    for cmd in &commands {
        assert!(!cmd.mutates_text(), "{cmd:?} should NOT mutate text");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Parser → ExCommand: confirm parse_ex_command produces correct variants
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn parse_cnext_with_count_3() {
    let cmd = parse_ex_command("cnext 3").unwrap();
    assert!(matches!(cmd, ExCommand::CNext { count: 3 }));
}

#[test]
fn parse_cn_abbreviated_with_count() {
    let cmd = parse_ex_command("cn 3").unwrap();
    assert!(matches!(cmd, ExCommand::CNext { count: 3 }));
}

#[test]
fn parse_cprev_with_count() {
    let cmd = parse_ex_command("cprev 2").unwrap();
    assert!(matches!(cmd, ExCommand::CPrev { count: 2 }));
}

#[test]
fn parse_cc_with_index() {
    let cmd = parse_ex_command("cc 7").unwrap();
    assert!(matches!(cmd, ExCommand::CC { index: Some(7) }));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Full pipeline: keystroke → parse → execute → HostRequest
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn cnext_emits_diagnostic_next_host_request() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "cnext");

    assert_eq!(response.host_requests().len(), 1);
    match &response.host_requests()[0] {
        HostRequest::DiagnosticNext { count, .. } => {
            assert_eq!(*count, 1);
        }
        other => panic!("Expected DiagnosticNext, got: {other:?}"),
    }
}

#[test]
fn cnext_with_count_emits_diagnostic_next_with_count() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "cnext 3");

    assert_eq!(response.host_requests().len(), 1);
    match &response.host_requests()[0] {
        HostRequest::DiagnosticNext { count, .. } => {
            assert_eq!(*count, 3);
        }
        other => panic!("Expected DiagnosticNext with count=3, got: {other:?}"),
    }
}

#[test]
fn cn_abbreviated_emits_diagnostic_next() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "cn");

    assert_eq!(response.host_requests().len(), 1);
    assert!(matches!(
        &response.host_requests()[0],
        HostRequest::DiagnosticNext { count: 1, .. }
    ));
}

#[test]
fn cprev_emits_diagnostic_prev_host_request() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "cprevious");

    assert_eq!(response.host_requests().len(), 1);
    match &response.host_requests()[0] {
        HostRequest::DiagnosticPrev { count, .. } => {
            assert_eq!(*count, 1);
        }
        other => panic!("Expected DiagnosticPrev, got: {other:?}"),
    }
}

#[test]
fn cprev_with_count_emits_diagnostic_prev_with_count() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "cp 5");

    assert_eq!(response.host_requests().len(), 1);
    match &response.host_requests()[0] {
        HostRequest::DiagnosticPrev { count, .. } => {
            assert_eq!(*count, 5);
        }
        other => panic!("Expected DiagnosticPrev with count=5, got: {other:?}"),
    }
}

#[test]
fn clist_emits_diagnostic_list_host_request() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "clist");

    assert_eq!(response.host_requests().len(), 1);
    assert!(matches!(
        &response.host_requests()[0],
        HostRequest::DiagnosticList { .. }
    ));
}

#[test]
fn cl_abbreviated_emits_diagnostic_list() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "cl");

    assert_eq!(response.host_requests().len(), 1);
    assert!(matches!(
        &response.host_requests()[0],
        HostRequest::DiagnosticList { .. }
    ));
}

#[test]
fn cc_emits_diagnostic_goto_host_request() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "cc");

    assert_eq!(response.host_requests().len(), 1);
    match &response.host_requests()[0] {
        HostRequest::DiagnosticGoto { index, .. } => {
            // :cc without argument defaults to index 1
            assert_eq!(*index, 1);
        }
        other => panic!("Expected DiagnosticGoto, got: {other:?}"),
    }
}

#[test]
fn cc_with_index_emits_diagnostic_goto_with_index() {
    let doc = TestDocument::from_text("hello world");
    let mut engine = VimEngine::new();

    let response = run_command_line(&mut engine, &doc, "cc 4");

    assert_eq!(response.host_requests().len(), 1);
    match &response.host_requests()[0] {
        HostRequest::DiagnosticGoto { index, .. } => {
            assert_eq!(*index, 4);
        }
        other => panic!("Expected DiagnosticGoto with index=4, got: {other:?}"),
    }
}
