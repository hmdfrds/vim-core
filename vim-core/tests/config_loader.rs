//! A config file read and run as Vim sources one.

use vim_core::effects::Effect;
use vim_core::execution::{HostRequest, Response, VimEngine};

/// The text of each error the response shows.
fn errors(response: &Response) -> Vec<String> {
    response
        .effects()
        .iter()
        .filter_map(|e| match e {
            Effect::ShowError { error, .. } => Some(error.to_string()),
            _ => None,
        })
        .collect()
}

/// The command of each custom command the response sends to the host.
fn host_commands(response: &Response) -> Vec<String> {
    response
        .host_requests()
        .iter()
        .map(|request| match request {
            HostRequest::CustomExCommand { command, .. } => command.to_string(),
            other => format!("{:?}", other.kind()),
        })
        .collect()
}

#[test]
fn a_script_heredoc_never_runs_as_commands() {
    // The Lua line `x = 1` would be `:x`, which writes and quits.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("lua << EOF\nx = 1\nEOF\nset tw=33\n");
    assert_eq!(host_commands(&response), ["lua << EOF"]);
    assert_eq!(engine.options().textwidth(), 33);
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
}

#[test]
fn a_let_heredoc_never_runs_as_commands() {
    let mut engine = VimEngine::new();
    engine.source_config_text("let x =<< END\nset tw=33\nEND\n");
    assert_eq!(engine.options().textwidth(), 0);
}

#[test]
fn a_let_heredoc_with_text_after_its_marker_fails() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("let x =<< END | set ic\nset tw=33\nEND\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    assert_eq!(errors(&response), ["E488: Trailing characters:  | set ic"]);
    assert!(response.host_requests().is_empty());
}

#[test]
fn a_function_body_never_runs_and_the_rest_of_its_end_line_does() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("function! F()\n  set ic\nendfunction | set tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(host_commands(&response), ["function! F()"]);
}

#[test]
fn a_function_without_its_end_gives_e126_and_runs_nothing() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("function F()\nset ic\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(errors(&response), ["E126: Missing :endfunction"]);
    assert!(response.host_requests().is_empty());

    let response = engine.source_config_text("silent! function F()\nset ic\n");
    assert!(errors(&response).is_empty(), "{:?}", errors(&response));
}

#[test]
fn a_body_is_skipped_after_a_command_that_fails() {
    // `:if` gives E319 until the loader runs blocks; the heredoc inside is
    // still skipped.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("if 0\nlua << EOF\nset ic\nEOF\nendif\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(
        errors(&response),
        [
            "E319: Sorry, the command is not available in this version: if 0",
            "E319: Sorry, the command is not available in this version: endif"
        ]
    );
}

#[test]
fn append_text_never_runs_as_commands() {
    let mut engine = VimEngine::new();
    engine.source_config_text("1append\nset ic\n.\nset tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
}

#[test]
fn a_command_block_never_runs_as_commands() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("command! Foo {\n  set ic\n}\nset tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(host_commands(&response), ["command! Foo {"]);
}

#[test]
fn a_command_after_an_error_on_its_line_still_reads_a_heredoc() {
    // `do_one_cmd()` skips the rest of the line but still calls `:let`,
    // which reads its heredoc.
    let mut engine = VimEngine::new();
    let response = engine.source_config_text("%d | let x =<< END\nset ic\nEND\nset tw=33\n");
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(errors(&response).len(), 1, "{:?}", errors(&response));
    assert!(response.host_requests().is_empty());
}

#[test]
fn a_command_after_an_error_on_its_line_reads_no_append_text() {
    // Vim does not call `:append` while skipping, so its text runs.
    let mut engine = VimEngine::new();
    engine.source_config_text("%d | 1append\nset ic\n");
    assert!(engine.options().ignorecase());
}

#[test]
fn the_rest_of_an_end_line_after_an_error_still_reads_a_heredoc() {
    let mut engine = VimEngine::new();
    let response = engine.source_config_text(
        "%d | function F()\nendfunction | let x =<< END\nset ic\nEND\nset tw=33\n",
    );
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 33);
    assert_eq!(errors(&response), ["E523: Not allowed here: %d"]);
}
