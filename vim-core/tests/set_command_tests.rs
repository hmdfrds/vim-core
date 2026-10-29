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

#[test]
fn text_after_the_name_is_an_error() {
    let mut engine = VimEngine::new();
    assert_eq!(
        error(&mut engine, "set tw?x"),
        "E488: Trailing characters: tw?x"
    );
    assert_eq!(
        error(&mut engine, "set ic,"),
        "E488: Trailing characters: ic,"
    );
    assert_eq!(
        error(&mut engine, "set nosuch?x"),
        "E518: Unknown option: nosuch?x"
    );
    // The value of a number or string option shows first.
    assert_eq!(
        run_ex(&mut engine, "set tw+5"),
        (
            vec!["textwidth=0".to_owned()],
            vec!["E488: Trailing characters: tw+5".to_owned()]
        )
    );
}

#[test]
fn reset_forms_are_not_available() {
    let mut engine = VimEngine::new();
    for (command, message) in [
        (
            "set fo&",
            "E319: Sorry, the command is not available in this version: fo&",
        ),
        (
            "set ic&vim",
            "E319: Sorry, the command is not available in this version: ic&vim",
        ),
        (
            "set tw<",
            "E319: Sorry, the command is not available in this version: tw<",
        ),
        (
            "set all&",
            "E319: Sorry, the command is not available in this version: all&",
        ),
        ("set nosuch&", "E518: Unknown option: nosuch&"),
    ] {
        assert_eq!(error(&mut engine, command), message, ":{command}");
    }
}

#[test]
fn an_option_this_build_lacks_takes_every_form_that_sets_it() {
    // do_set_option(): an option of Vim's table that vim-core lacks acts
    // as a hidden one, which a set command for is ignored.
    let mut engine = VimEngine::new();
    let before = engine.options().clone();
    for command in [
        "set nocompatible",
        "set compatible",
        "set encoding=utf-8",
        "set mouse=a",
        "set mouse+=v mouse-=a mouse^=n mouse:a mouse! mouse& mouse<",
        "set nomouse=a invmouse!",
        "set termguicolors notgc invtgc tgc! tgc, tgc!x",
        "set t_Co=256",
        r"set guifont=Monospace\ 11",
        "set gfn =x",
        "setlocal mouse=a",
        "setglobal mouse=a",
        // vim-core has 'showmatch', but `:set` cannot name it.
        "set showmatch",
    ] {
        assert_eq!(run_ex(&mut engine, command), (vec![], vec![]), ":{command}");
    }
    assert_eq!(engine.options(), &before);
}

#[test]
fn asking_for_an_option_this_build_lacks_is_e519() {
    // do_set_option(): a query of a hidden option, or a bare name of one
    // that is not a boolean.
    let mut engine = VimEngine::new();
    for (command, message) in [
        ("set mouse?", "E519: Option not supported: mouse?"),
        ("set mouse", "E519: Option not supported: mouse"),
        ("set nomouse", "E519: Option not supported: nomouse"),
        ("set nomouse?", "E519: Option not supported: nomouse?"),
        ("set mouse?x", "E519: Option not supported: mouse?x"),
        ("set mouse,", "E519: Option not supported: mouse,"),
        ("set tgc?", "E519: Option not supported: tgc?"),
        ("set t_Co?", "E519: Option not supported: t_Co?"),
        ("set t_Co", "E519: Option not supported: t_Co"),
        ("set nosuch", "E518: Unknown option: nosuch"),
        ("set t_ku=x", "E518: Unknown option: t_ku=x"),
    ] {
        assert_eq!(error(&mut engine, command), message, ":{command}");
    }
    // The first bad argument ends the :set.
    assert_eq!(
        error(&mut engine, "set mouse? ic"),
        "E519: Option not supported: mouse?"
    );
    assert!(!engine.options().ignorecase());
}

#[test]
fn inv_inverts_a_boolean() {
    let mut engine = VimEngine::new();
    let _ = run_ex(&mut engine, "set invic");
    assert!(engine.options().ignorecase());
    let _ = run_ex(&mut engine, "set invic");
    assert!(!engine.options().ignorecase());
}

#[test]
fn a_string_value_loses_its_backslashes() {
    // stropt_copy_value(): a backslash before any character goes.
    let mut engine = VimEngine::new();
    assert_eq!(
        run_ex(&mut engine, r"set commentstring=/*\ %s\ */ isk=@,\\,48-57"),
        (vec![], vec![])
    );
    assert_eq!(engine.options().commentstring(), "/* %s */");
    assert_eq!(engine.options().iskeyword(), r"@,\,48-57");
    // An escaped blank joins the argument, and the error shows it as
    // written.
    assert_eq!(
        error(&mut engine, r"set nosuch=Monospace\ 11 ic"),
        r"E518: Unknown option: nosuch=Monospace\ 11"
    );
    assert!(!engine.options().ignorecase());
    assert_eq!(
        error(&mut engine, r"set fo=t\Z"),
        r"E539: Illegal character <Z>: fo=t\Z"
    );
    assert_eq!(
        error(&mut engine, r"set fo+=\Z"),
        r"E539: Illegal character <Z>: fo+=\Z"
    );
}

#[test]
fn blanks_may_come_before_an_operator() {
    let mut engine = VimEngine::new();
    assert_eq!(run_ex(&mut engine, "set sw =2 tw +=3"), (vec![], vec![]));
    assert_eq!(engine.options().shiftwidth(), 2);
    assert_eq!(engine.options().textwidth(), 3);
    assert_eq!(run_ex(&mut engine, "set tw  ?").0, ["textwidth=3"]);
}

#[test]
fn langnoremap_is_langremap_inverted() {
    // did_set_langnoremap() and did_set_langremap() set each other.
    let mut engine = VimEngine::new();
    assert_eq!(run_ex(&mut engine, "set langnoremap"), (vec![], vec![]));
    assert!(!engine.options().langremap());
    assert_eq!(run_ex(&mut engine, "set lnr?").0, ["langnoremap"]);
    assert_eq!(run_ex(&mut engine, "set lrm?").0, ["nolangremap"]);
    assert_eq!(run_ex(&mut engine, "set nolnr"), (vec![], vec![]));
    assert!(engine.options().langremap());
    assert_eq!(run_ex(&mut engine, "set langnoremap?").0, ["nolangnoremap"]);
    assert_eq!(run_ex(&mut engine, "set lnr!"), (vec![], vec![]));
    assert!(!engine.options().langremap());
}
