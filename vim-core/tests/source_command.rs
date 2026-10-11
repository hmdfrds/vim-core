//! `:source` across the host boundary.

use vim_core::effects::Effect;
use vim_core::execution::{
    HostCallbacks, HostRequest, HostSession, InputContext, Response, VimEngine,
};
use vim_core::keymap::KeyEvent;
use vim_core::test_utils::SimpleDocument;

fn process_key(engine: &mut VimEngine, doc: &SimpleDocument, key: KeyEvent) -> Response {
    let ctx = InputContext::new(doc, 0).validate_clamped();
    engine.process(key, ctx)
}

fn run(engine: &mut VimEngine, doc: &SimpleDocument, command: &str) -> Response {
    let _ = process_key(engine, doc, KeyEvent::char(':'));
    for ch in command.chars() {
        let _ = process_key(engine, doc, KeyEvent::char(ch));
    }
    process_key(engine, doc, KeyEvent::enter())
}

fn request(response: &Response) -> HostRequest {
    assert_eq!(response.host_requests().len(), 1, "{response:?}");
    response.host_requests()[0].clone()
}

/// A host whose only file is `extra.vim`.
struct OneFile(&'static str);

impl HostCallbacks for OneFile {
    fn write_file(&mut self, _path: &str, _text: &str) -> Result<(), String> {
        Err("read-only".into())
    }

    fn read_file(&self, path: &str) -> Result<String, String> {
        if path == "extra.vim" {
            Ok(self.0.into())
        } else {
            Err("not found".into())
        }
    }

    fn quit(&mut self, _force: bool) {}
}

fn run_in_session(session: &mut HostSession, host: &mut OneFile, command: &str) {
    let _ = session.process_key_with_callbacks(KeyEvent::char(':'), host);
    for ch in command.chars() {
        let _ = session.process_key_with_callbacks(KeyEvent::char(ch), host);
    }
    let _ = session.process_key_with_callbacks(KeyEvent::enter(), host);
}

#[test]
fn bare_source_asks_the_host_for_its_default() {
    let doc = SimpleDocument::new("alpha");
    for command in ["source", "so", "source \" reload"] {
        let mut engine = VimEngine::new();
        let req = request(&run(&mut engine, &doc, command));
        assert!(
            matches!(req, HostRequest::ReadConfigFile { path: None, .. }),
            "{command}: {req:?}"
        );
    }
}

#[test]
fn source_passes_the_path_unexpanded() {
    let doc = SimpleDocument::new("alpha");
    let mut engine = VimEngine::new();
    let req = request(&run(&mut engine, &doc, "so $MYVIMRC"));
    assert!(
        matches!(req, HostRequest::ReadConfigFile { path: Some(ref p), .. } if p.as_str() == "$MYVIMRC"),
        "{req:?}"
    );
}

#[test]
fn host_session_has_no_default_config() {
    let mut session = HostSession::new("alpha");
    let mut host = OneFile("set tw=33");
    run_in_session(&mut session, &mut host, "source");
    assert_eq!(session.errmsg().as_deref(), Some("E471: Argument required"));
    assert_eq!(session.options().textwidth(), 0);
}

#[test]
fn source_forms_vim_has_and_the_engine_lacks_report_e319() {
    let doc = SimpleDocument::new("alpha\nbeta");
    for (command, message) in [
        (
            "source! x",
            "E319: Sorry, the command is not available in this version: source!",
        ),
        (
            "1,2source",
            "E319: Sorry, the command is not available in this version: [range]source",
        ),
    ] {
        let mut engine = VimEngine::new();
        let response = run(&mut engine, &doc, command);
        assert!(response.host_requests().is_empty(), "{command}");
        assert!(
            response.effects().iter().any(
                |e| matches!(e, Effect::ShowError { error, .. } if error.to_string() == message)
            ),
            "{command}: {response:?}"
        );
    }
}
