//! `:set` support for the formatting options: `formatoptions`, `comments`
//! and `commentstring`, their scopes, queries and error messages.
//!
//! Expected values and messages were checked against headless Vim 9.1
//! (`vim -u NONE -N -es`).

mod common;

use common::document::TestDocument;
use vim_core::effects::{Effect, InfoMessage};
use vim_core::execution::{BufferLocalState, InputContext, VimEngine};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{OptionId, OptionValue};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn press(
    engine: &mut VimEngine,
    doc: &TestDocument,
    key: KeyEvent,
) -> vim_core::execution::Response {
    let ctx = InputContext::new(doc, doc.cursor_offset()).validate_clamped();
    engine.process(key, ctx)
}

/// Run an ex command and return the messages and errors it produced.
fn run_ex(engine: &mut VimEngine, command: &str) -> (Vec<String>, Vec<String>) {
    let doc = TestDocument::new("hello", (0, 0));
    let _ = press(engine, &doc, KeyEvent::char(':'));
    for ch in command.chars() {
        let _ = press(engine, &doc, KeyEvent::char(ch));
    }
    let response = press(engine, &doc, KeyEvent::enter());
    let mut messages = Vec::new();
    let mut errors = Vec::new();
    for effect in response.effects() {
        match effect {
            Effect::ShowInfo {
                info: InfoMessage::Text(text),
            } => messages.push(text.to_string()),
            Effect::ShowError { error, .. } => errors.push(error.to_string()),
            _ => {}
        }
    }
    (messages, errors)
}

/// Run an ex command that must succeed silently.
fn set(engine: &mut VimEngine, command: &str) {
    let (messages, errors) = run_ex(engine, command);
    assert!(errors.is_empty(), ":{command} failed: {errors:?}");
    assert!(messages.is_empty(), ":{command} printed {messages:?}");
}

/// Run an ex command that must print exactly one message.
fn query(engine: &mut VimEngine, command: &str) -> String {
    let (mut messages, errors) = run_ex(engine, command);
    assert!(errors.is_empty(), ":{command} failed: {errors:?}");
    assert_eq!(messages.len(), 1, ":{command} printed {messages:?}");
    messages.remove(0)
}

/// Run an ex command that must fail with exactly one error.
fn error(engine: &mut VimEngine, command: &str) -> String {
    let (_, mut errors) = run_ex(engine, command);
    assert_eq!(errors.len(), 1, ":{command} gave {errors:?}");
    errors.remove(0)
}

fn effective_str(engine: &VimEngine, id: OptionId) -> String {
    match engine.effective_option(id) {
        OptionValue::Str(s) => s.to_string(),
        other => panic!("{id:?} is not a string: {other:?}"),
    }
}

// ── formatoptions ────────────────────────────────────────────────────────────

#[test]
fn set_formatoptions_long_and_short_name() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set formatoptions=cq");
    assert_eq!(engine.options().formatoptions(), "cq");
    set(&mut engine, "set fo=cqj");
    assert_eq!(engine.options().formatoptions(), "cqj");
    assert_eq!(effective_str(&engine, OptionId::FormatOptions), "cqj");
    assert!(!engine.resolved_options().auto_format_text());
}

#[test]
fn setlocal_formatoptions_leaves_global_alone() {
    let mut engine = VimEngine::new();
    set(&mut engine, "setlocal fo=cq");
    assert_eq!(engine.options().formatoptions(), "tcqj");
    assert_eq!(effective_str(&engine, OptionId::FormatOptions), "cq");
    assert!(!engine.resolved_options().auto_format_text());
    assert!(engine.options().auto_format_text());
}

#[test]
fn setglobal_formatoptions_leaves_local_alone() {
    let mut engine = VimEngine::new();
    set(&mut engine, "setlocal fo=cq");
    set(&mut engine, "setglobal fo=tq");
    assert_eq!(engine.options().formatoptions(), "tq");
    assert_eq!(effective_str(&engine, OptionId::FormatOptions), "cq");
}

#[test]
fn formatoptions_override_follows_the_buffer() {
    let mut engine = VimEngine::new();
    set(&mut engine, "setlocal fo=cq");
    let saved = engine.on_buffer_leave(0);
    // Another buffer sees the global value.
    engine.on_buffer_enter(BufferLocalState::default());
    assert_eq!(effective_str(&engine, OptionId::FormatOptions), "tcqj");
    let _ = engine.on_buffer_leave(0);
    engine.on_buffer_enter(saved);
    assert_eq!(effective_str(&engine, OptionId::FormatOptions), "cq");
}

#[test]
fn formatoptions_unknown_flag_is_e539() {
    let mut engine = VimEngine::new();
    // Vim 9.1: "E539: Illegal character <Z>: fo=tZ"
    assert_eq!(
        error(&mut engine, "set fo=tZ"),
        "E539: Illegal character <Z>: fo=tZ"
    );
    assert_eq!(engine.options().formatoptions(), "tcqj");
}

#[test]
fn formatoptions_repeated_flag_keeps_last() {
    let mut engine = VimEngine::new();
    // Vim 9.1: `:set fo=tt` stores "t".
    set(&mut engine, "set fo=tt");
    assert_eq!(engine.options().formatoptions(), "t");
}

#[test]
fn formatoptions_empty_is_allowed() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set fo=");
    assert_eq!(engine.options().formatoptions(), "");
    assert!(engine.options().format_flags().is_empty());
}

#[test]
fn query_formatoptions_shows_effective_value() {
    let mut engine = VimEngine::new();
    assert_eq!(query(&mut engine, "set fo?"), "formatoptions=tcqj");
    set(&mut engine, "setlocal fo=cq");
    // Vim: `:set` and `:setlocal` show the local value, `:setglobal` the
    // global one.
    assert_eq!(query(&mut engine, "set fo?"), "formatoptions=cq");
    assert_eq!(query(&mut engine, "setlocal fo?"), "formatoptions=cq");
    assert_eq!(query(&mut engine, "setglobal fo?"), "formatoptions=tcqj");
}

#[test]
fn bare_name_of_string_option_shows_value() {
    let mut engine = VimEngine::new();
    // Vim: `:set fo` and `:set tw` show the value, like `:set fo?`.
    assert_eq!(query(&mut engine, "set fo"), "formatoptions=tcqj");
    assert_eq!(query(&mut engine, "set tw"), "textwidth=0");
}

#[test]
fn bool_syntax_on_value_option_is_an_error() {
    let mut engine = VimEngine::new();
    assert_eq!(
        error(&mut engine, "set nofo"),
        "E474: Invalid argument: nofo"
    );
    assert_eq!(
        error(&mut engine, "set notw"),
        "E474: Invalid argument: notw"
    );
    assert_eq!(
        error(&mut engine, "set tw!"),
        "E488: Trailing characters: tw!"
    );
    assert_eq!(engine.options().textwidth(), 0);
}

// ── comments ─────────────────────────────────────────────────────────────────

#[test]
fn comments_defaults_to_vim_list() {
    let mut engine = VimEngine::new();
    assert_eq!(
        query(&mut engine, "set com?"),
        "comments=s1:/*,mb:*,ex:*/,://,b:#,:%,:XCOMM,n:>,fb:-"
    );
}

#[test]
fn set_comments_long_and_short_name() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set comments=b:##,b:#");
    assert_eq!(engine.options().comments(), "b:##,b:#");
    set(&mut engine, "set com=://");
    assert_eq!(engine.options().comments(), "://");
    assert_eq!(engine.resolved_options().comment_spec().parts().len(), 1);
}

#[test]
fn setlocal_comments_is_buffer_local() {
    let mut engine = VimEngine::new();
    set(&mut engine, "setlocal com=b:#");
    assert_eq!(effective_str(&engine, OptionId::Comments), "b:#");
    assert_eq!(
        engine.options().comments(),
        "s1:/*,mb:*,ex:*/,://,b:#,:%,:XCOMM,n:>,fb:-"
    );
    assert!(engine
        .resolved_options()
        .comment_spec()
        .match_line("// x")
        .is_none());
}

#[test]
fn comments_errors_match_vim() {
    let mut engine = VimEngine::new();
    assert_eq!(
        error(&mut engine, "set com=x"),
        "E524: Missing colon: com=x"
    );
    assert_eq!(
        error(&mut engine, "set com=s:"),
        "E525: Zero length string: com=s:"
    );
    assert_eq!(
        error(&mut engine, "set com=q:x"),
        "E539: Illegal character <q>: com=q:x"
    );
    assert_eq!(
        engine.options().comments(),
        "s1:/*,mb:*,ex:*/,://,b:#,:%,:XCOMM,n:>,fb:-"
    );
}

// ── commentstring ────────────────────────────────────────────────────────────

#[test]
fn commentstring_short_name_and_query() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set cms=#%s");
    assert_eq!(engine.options().commentstring(), "#%s");
    assert_eq!(query(&mut engine, "set cms?"), "commentstring=#%s");
}
