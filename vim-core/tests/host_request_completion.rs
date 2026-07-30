//! Integration tests for host-request completion behavior.

#[path = "common/mod.rs"]
mod common;

use common::TestDocument;
use vim_core::effects::Effect;
use vim_core::execution::{
    HostRequest, HostRequestId, HostResult, InputContext, Response, VimEngine,
};
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

#[test]
fn complete_host_request_without_pending_returns_message() {
    let mut engine = VimEngine::new();
    let response = engine.complete_host_request(&HostResult::Success {
        id: HostRequestId::new(42),
        message: None,
    });

    assert!(!response.consumed());
    assert!(!response.pending());
    assert!(response
        .message()
        .is_some_and(|msg| msg.contains("unexpected host result id 42")));
}

#[test]
fn external_command_data_completion_surfaces_message_effect() {
    let doc = TestDocument::from_text("alpha");
    let mut engine = VimEngine::new();
    let response = run_command_line(&mut engine, &doc, "!echo hi");

    assert_eq!(response.host_requests().len(), 1);
    let request = response.host_requests()[0].clone();
    assert!(matches!(request, HostRequest::ExternalCommand { .. }));

    let completion = engine.complete_host_request(&HostResult::Data {
        id: request.id(),
        data: "shell output".into(),
        offset: None,
    });

    assert!(completion.effects().iter().any(
        |effect| matches!(effect, Effect::ShowInfo { info: vim_core::effects::InfoMessage::Text(text) } if text.as_str() == "shell output")
    ));
}

#[test]
fn read_file_data_completion_inserts_payload() {
    let doc = TestDocument::from_text("alpha");
    let mut engine = VimEngine::new();
    let response = run_command_line(&mut engine, &doc, "r /tmp/example.txt");

    assert_eq!(response.host_requests().len(), 1);
    let request = response.host_requests()[0].clone();
    assert!(matches!(request, HostRequest::ReadFile { .. }));

    let completion = engine.complete_host_request(&HostResult::Data {
        id: request.id(),
        data: "payload".into(),
        offset: Some(3),
    });

    assert!(completion
        .effects()
        .iter()
        .any(|effect| matches!(effect, Effect::Insert { offset, text } if offset.get() == 3 && text.as_str() == "payload")));
    assert!(completion
        .effects()
        .iter()
        .any(|effect| matches!(effect, Effect::SetCursor { offset } if offset.get() == 3)));
}

// ── SyncCommandLine emission tests ────────────────────────────────────────

fn find_sync_command_line(response: &Response) -> Option<&HostRequest> {
    response
        .host_requests()
        .iter()
        .find(|r| matches!(r, HostRequest::SyncCommandLine { .. }))
}

#[test]
fn entering_command_line_emits_sync() {
    let mut engine = VimEngine::new();
    let doc = TestDocument::from_text("hello");
    let response = process_key(&mut engine, &doc, KeyEvent::char(':'));

    let sync = find_sync_command_line(&response).expect("entering : should emit SyncCommandLine");
    match sync {
        HostRequest::SyncCommandLine { input, prompt, .. } => {
            assert_eq!(input.as_str(), "");
            assert_eq!(*prompt, vim_core::state::CommandLinePrompt::Ex);
        }
        _ => unreachable!(),
    }
}

#[test]
fn typing_in_command_line_emits_sync() {
    let mut engine = VimEngine::new();
    let doc = TestDocument::from_text("hello");

    let _ = process_key(&mut engine, &doc, KeyEvent::char(':'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('w'));

    let sync = find_sync_command_line(&response)
        .expect("typing in command line should emit SyncCommandLine");
    match sync {
        HostRequest::SyncCommandLine { input, .. } => {
            assert_eq!(input.as_str(), "w");
        }
        _ => unreachable!(),
    }
}

#[test]
fn replacing_command_line_text_emits_sync_for_host_fallback() {
    let mut engine = VimEngine::new();
    let doc = TestDocument::from_text("hello");

    let _ = process_key(&mut engine, &doc, KeyEvent::char(':'));
    let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
    let response = engine.replace_command_line_text("write", &ctx);

    let sync = find_sync_command_line(&response)
        .expect("host fallback text replacement should emit SyncCommandLine");
    match sync {
        HostRequest::SyncCommandLine {
            input,
            prompt,
            cursor,
            ..
        } => {
            assert_eq!(input.as_str(), "write");
            assert_eq!(*cursor, 5);
            assert_eq!(*prompt, vim_core::state::CommandLinePrompt::Ex);
        }
        _ => unreachable!(),
    }
}

#[test]
fn replacing_search_command_line_text_emits_exact_highlights() {
    let mut engine = VimEngine::new();
    let doc = TestDocument::from_text("alpha beta alpha");

    let _ = process_key(&mut engine, &doc, KeyEvent::char('/'));
    let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
    let response = engine.replace_command_line_text("alpha", &ctx);

    assert!(response.effects().iter().any(|effect| matches!(
        effect,
        Effect::HighlightMatches { ranges }
            if ranges.iter().map(|range| (range.start().get(), range.end().get())).collect::<Vec<_>>()
                == vec![(0, 5), (11, 16)]
    )));
}

#[test]
fn search_forward_emits_correct_prompt() {
    let mut engine = VimEngine::new();
    let doc = TestDocument::from_text("hello");

    let _ = process_key(&mut engine, &doc, KeyEvent::char('/'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('t'));

    let sync =
        find_sync_command_line(&response).expect("typing in search should emit SyncCommandLine");
    match sync {
        HostRequest::SyncCommandLine { input, prompt, .. } => {
            assert_eq!(input.as_str(), "t");
            assert_eq!(*prompt, vim_core::state::CommandLinePrompt::SearchForward);
        }
        _ => unreachable!(),
    }
}

#[test]
fn cancel_command_line_emits_sync() {
    let mut engine = VimEngine::new();
    let doc = TestDocument::from_text("hello");

    let _ = process_key(&mut engine, &doc, KeyEvent::char(':'));
    let response = process_key(&mut engine, &doc, KeyEvent::escape());

    let sync = find_sync_command_line(&response)
        .expect("cancelling command line should emit SyncCommandLine");
    match sync {
        HostRequest::SyncCommandLine { input, .. } => {
            assert_eq!(input.as_str(), "");
        }
        _ => unreachable!(),
    }
}
