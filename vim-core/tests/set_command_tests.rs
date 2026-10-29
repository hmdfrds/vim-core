//! `:set` reading its arguments and reporting errors as Vim's `do_set()`
//! and `do_set_option()` do.

mod common;

use common::document::TestDocument;
use vim_core::effects::{Effect, InfoMessage};
use vim_core::execution::{InputContext, VimEngine};
use vim_core::keymap::KeyEvent;

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

/// Run an ex command that must fail with exactly one error.
fn error(engine: &mut VimEngine, command: &str) -> String {
    let (_, mut errors) = run_ex(engine, command);
    assert_eq!(errors.len(), 1, ":{command} gave {errors:?}");
    errors.remove(0)
}

#[test]
fn the_first_bad_argument_ends_the_set() {
    let mut engine = VimEngine::new();
    assert_eq!(
        error(&mut engine, "set nosuch ts=7"),
        "E518: Unknown option: nosuch"
    );
    assert_eq!(engine.options().tabstop(), 4);
    assert_eq!(
        error(&mut engine, "set ts=7 nosuch sw=2"),
        "E518: Unknown option: nosuch"
    );
    assert_eq!(engine.options().tabstop(), 7);
    assert_eq!(engine.options().shiftwidth(), 4);
}

#[test]
fn errors_name_the_argument_as_written() {
    let mut engine = VimEngine::new();
    for (command, message) in [
        ("set nosuch=5", "E518: Unknown option: nosuch=5"),
        ("set tw=abc", "E521: Number required after =: tw=abc"),
        ("set tw=", "E521: Number required after =: tw="),
        ("set tw=1abc", "E521: Number required after =: tw=1abc"),
        ("set ic=1", "E474: Invalid argument: ic=1"),
        ("set ic+=1", "E474: Invalid argument: ic+=1"),
        ("set notw=5", "E474: Invalid argument: notw=5"),
        ("set noic=1", "E474: Invalid argument: noic=1"),
        ("set fo=tZ", "E539: Illegal character <Z>: fo=tZ"),
        ("set sw=-1", "E487: Argument must be positive: sw=-1"),
    ] {
        assert_eq!(error(&mut engine, command), message, ":{command}");
    }
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
}

#[test]
fn values_shown_before_an_error_stay_shown() {
    let mut engine = VimEngine::new();
    assert_eq!(
        run_ex(&mut engine, "set tw ts=x sw?"),
        (
            vec!["textwidth=0".to_owned()],
            vec!["E521: Number required after =: ts=x".to_owned()]
        )
    );
}

#[test]
fn a_query_ignores_a_no_or_inv_prefix() {
    // do_set_option() shows the option for `?` whatever its prefix.
    let mut engine = VimEngine::new();
    assert_eq!(run_ex(&mut engine, "set noic?").0, ["noignorecase"]);
    assert_eq!(run_ex(&mut engine, "set notw?").0, ["textwidth=0"]);
    assert_eq!(
        error(&mut engine, "set nosuch?"),
        "E518: Unknown option: nosuch?"
    );
}

#[test]
fn a_bang_after_a_prefix_inverts_a_boolean() {
    let mut engine = VimEngine::new();
    let _ = run_ex(&mut engine, "set noic!");
    assert!(engine.options().ignorecase());
    assert_eq!(
        error(&mut engine, "set notw!"),
        "E474: Invalid argument: notw!"
    );
}

#[test]
fn belloff_is_a_string_option() {
    let mut engine = VimEngine::new();
    assert_eq!(run_ex(&mut engine, "set belloff=all"), (vec![], vec![]));
    assert!(engine.options().belloff());
    assert_eq!(run_ex(&mut engine, "set bo?").0, ["belloff=all"]);
    assert_eq!(run_ex(&mut engine, "set bo").0, ["belloff=all"]);
    assert_eq!(
        error(&mut engine, "set bo=x"),
        "E474: Invalid argument: bo=x"
    );
    assert_eq!(
        error(&mut engine, "set nobo"),
        "E474: Invalid argument: nobo"
    );
    assert!(engine.options().belloff());
}

#[test]
fn each_set_of_a_typed_line_stops_on_its_own() {
    let mut engine = VimEngine::new();
    assert_eq!(
        error(&mut engine, "set nosuch ts=7 | set ic"),
        "E518: Unknown option: nosuch"
    );
    assert_eq!(engine.options().tabstop(), 4);
    assert!(engine.options().ignorecase());
}
