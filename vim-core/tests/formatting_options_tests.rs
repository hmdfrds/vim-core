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
fn host_setting_replaces_a_window_local_value() {
    // set_option() drops a window-local value as `:set` does, so the host
    // value wins over an earlier `:setlocal`.
    let mut engine = VimEngine::new();
    set(&mut engine, "setlocal so=5");
    assert_eq!(query(&mut engine, "set so?"), "scrolloff=5");
    engine.set_option(OptionId::ScrollOff, &OptionValue::Unsigned(0));
    assert_eq!(query(&mut engine, "set so?"), "scrolloff=0");
}

#[test]
fn host_setting_reaches_buffers_left_after_set() {
    // set_option() drops the local value of the current buffer only, as
    // `:set` does. clear_local_option() drops it from a saved buffer, so a
    // host setting applied to every buffer wins over an older `:set` there.
    let mut engine = VimEngine::new();
    set(&mut engine, "set tw=20");
    let mut a = engine.on_buffer_leave(0);
    engine.on_buffer_enter(BufferLocalState::default());
    engine.set_option(OptionId::TextWidth, &OptionValue::Unsigned(0));
    assert_eq!(query(&mut engine, "set tw?"), "textwidth=0");
    let b = engine.on_buffer_leave(0);
    engine.on_buffer_enter(a.clone());
    assert_eq!(query(&mut engine, "set tw?"), "textwidth=20");
    let _ = engine.on_buffer_leave(0);
    a.clear_local_option(OptionId::TextWidth);
    engine.on_buffer_enter(a);
    assert_eq!(query(&mut engine, "set tw?"), "textwidth=0");
    let _ = engine.on_buffer_leave(0);
    engine.on_buffer_enter(b);
    assert_eq!(query(&mut engine, "set tw?"), "textwidth=0");
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

// ── :set +=, -=, ^= ──────────────────────────────────────────────────────────

#[test]
fn remove_t_from_formatoptions() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set fo-=t");
    assert_eq!(engine.options().formatoptions(), "cqj");
    assert!(!engine.resolved_options().auto_format_text());
}

#[test]
fn formatoptions_operators_chain() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set fo=tcq");
    set(&mut engine, "set fo-=t fo+=r fo^=l");
    assert_eq!(engine.options().formatoptions(), "lcqr");
}

#[test]
fn formatoptions_operator_unknown_flag_is_e539() {
    let mut engine = VimEngine::new();
    // Vim 9.1: "E539: Illegal character <Z>: fo+=Z"
    assert_eq!(
        error(&mut engine, "set fo+=Z"),
        "E539: Illegal character <Z>: fo+=Z"
    );
    assert_eq!(engine.options().formatoptions(), "tcqj");
}

// Vim 9.1 with `setglobal fo=tcq | setlocal fo=cq`:
//   set fo+=r       -> global cqr, local cqr
//   setlocal fo+=r  -> global tcq, local cqr
//   setglobal fo+=r -> global tcqr, local cq
#[test]
fn operator_scopes_match_vim() {
    let start = |engine: &mut VimEngine| {
        set(engine, "setglobal fo=tcq");
        set(engine, "setlocal fo=cq");
    };

    let mut engine = VimEngine::new();
    start(&mut engine);
    set(&mut engine, "set fo+=r");
    assert_eq!(engine.options().formatoptions(), "cqr");
    assert_eq!(effective_str(&engine, OptionId::FormatOptions), "cqr");

    let mut engine = VimEngine::new();
    start(&mut engine);
    set(&mut engine, "setlocal fo+=r");
    assert_eq!(engine.options().formatoptions(), "tcq");
    assert_eq!(effective_str(&engine, OptionId::FormatOptions), "cqr");

    let mut engine = VimEngine::new();
    start(&mut engine);
    set(&mut engine, "setglobal fo+=r");
    assert_eq!(engine.options().formatoptions(), "tcqr");
    assert_eq!(effective_str(&engine, OptionId::FormatOptions), "cq");
}

// Vim 9.1 with `setglobal tw=10 | setlocal tw=20`: `set tw+=1` gives 21/21,
// `setglobal tw+=1` 11/20, `setlocal tw+=1` 10/21.
#[test]
fn number_operator_scopes_match_vim() {
    let mut engine = VimEngine::new();
    set(&mut engine, "setglobal tw=10");
    set(&mut engine, "setlocal tw=20");
    set(&mut engine, "setglobal tw+=1");
    assert_eq!(engine.options().textwidth(), 11);
    assert_eq!(
        engine.effective_option(OptionId::TextWidth),
        OptionValue::Unsigned(20)
    );
    set(&mut engine, "setlocal tw+=1");
    assert_eq!(engine.options().textwidth(), 11);
    assert_eq!(
        engine.effective_option(OptionId::TextWidth),
        OptionValue::Unsigned(21)
    );
    set(&mut engine, "set tw+=1");
    assert_eq!(engine.options().textwidth(), 22);
    assert_eq!(
        engine.effective_option(OptionId::TextWidth),
        OptionValue::Unsigned(22)
    );
}

#[test]
fn number_operator_errors_match_vim() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set tw=10");
    assert_eq!(
        error(&mut engine, "set tw-=40"),
        "E487: Argument must be positive: tw-=40"
    );
    assert_eq!(
        error(&mut engine, "set tw+=x"),
        "E521: Number required after =: tw+=x"
    );
    assert_eq!(
        error(&mut engine, "set tw=-1"),
        "E487: Argument must be positive: tw=-1"
    );
    assert_eq!(
        error(&mut engine, "set ai+=1"),
        "E474: Invalid argument: ai+=1"
    );
    assert_eq!(engine.options().textwidth(), 10);
}

#[test]
fn number_assignment_accepts_vim_number_syntax() {
    let mut engine = VimEngine::new();
    // Vim 9.1: `tw=010` and `tw=0o10` are 8, `tw+=0x10` adds 16.
    set(&mut engine, "set tw=010");
    assert_eq!(engine.options().textwidth(), 8);
    set(&mut engine, "set tw+=0x10");
    assert_eq!(engine.options().textwidth(), 24);
}

#[test]
fn comments_operators_match_vim() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set com-=b:#");
    assert_eq!(
        engine.options().comments(),
        "s1:/*,mb:*,ex:*/,://,:%,:XCOMM,n:>,fb:-"
    );
    set(&mut engine, "set com^=b:#");
    assert_eq!(
        engine.options().comments(),
        "b:#,s1:/*,mb:*,ex:*/,://,:%,:XCOMM,n:>,fb:-"
    );
    // Already present: unchanged.
    set(&mut engine, "set com+=b:#");
    assert_eq!(
        engine.options().comments(),
        "b:#,s1:/*,mb:*,ex:*/,://,:%,:XCOMM,n:>,fb:-"
    );
}

#[test]
fn comments_operator_result_is_validated() {
    let mut engine = VimEngine::new();
    // Vim 9.1: "E524: Missing colon: com+=b"
    assert_eq!(
        error(&mut engine, "set com+=b"),
        "E524: Missing colon: com+=b"
    );
}

#[test]
fn escaped_space_in_commentstring() {
    let mut engine = VimEngine::new();
    set(&mut engine, r"setlocal commentstring=#\ %s");
    assert_eq!(effective_str(&engine, OptionId::CommentString), "# %s");
}

#[test]
fn whichwrap_operators_treat_items_as_flags() {
    // Vim 9.1: 'whichwrap' is a comma list whose items are flags, so += on
    // an item that is there moves it to the end.
    let mut engine = VimEngine::new();
    set(&mut engine, "set ww=b,s");
    set(&mut engine, "set ww+=b");
    assert_eq!(query(&mut engine, "set ww?"), "whichwrap=s,b");
    set(&mut engine, "set ww=b,s,h");
    set(&mut engine, "set ww-=s");
    assert_eq!(query(&mut engine, "set ww?"), "whichwrap=b,h");
    set(&mut engine, "set ww=s");
    set(&mut engine, "set ww^=b,s");
    assert_eq!(query(&mut engine, "set ww?"), "whichwrap=b,s");
}

#[test]
fn operator_on_option_without_id() {
    let mut engine = VimEngine::new();
    set(&mut engine, "set mlfr+=2");
    assert_eq!(engine.options().multiline_find_range(), 7);
    assert_eq!(
        error(&mut engine, "set mlf+=1"),
        "E474: Invalid argument: mlf+=1"
    );
}

// ── Commands read the resolved options ───────────────────────────────────────

mod commands_read_resolved_options {
    use vim_core::execution::{parse_keys_from_string, HostSession};
    use vim_core::primitives::{OptionId, OptionValue, VimOptions};

    fn feed(session: &mut HostSession, keys: &str) {
        for key in parse_keys_from_string(keys) {
            session.process_key_host(key);
        }
    }

    fn session_with_textwidth(text: &str, tw: usize) -> HostSession {
        let mut session = HostSession::new(text);
        let mut opts = VimOptions::default();
        opts.set_textwidth(tw);
        session.set_options(opts);
        session
    }

    const LONG: &str = "aaaa bbbb cccc dddd eeee ffff";

    #[test]
    fn global_textwidth_wraps() {
        // Baseline for the tests below: the global value alone wraps.
        let mut session = session_with_textwidth("", 20);
        feed(&mut session, &format!("i{LONG}<Esc>"));
        assert!(session.text().to_string().contains('\n'));
    }

    #[test]
    fn setlocal_textwidth_zero_stops_wrapping() {
        let mut session = session_with_textwidth("", 20);
        feed(&mut session, ":setlocal tw=0<CR>");
        feed(&mut session, &format!("i{LONG}<Esc>"));
        assert_eq!(session.text().to_string(), LONG);
    }

    #[test]
    fn setlocal_textwidth_enables_wrapping() {
        let mut session = session_with_textwidth("", 0);
        feed(&mut session, ":setlocal tw=20<CR>");
        feed(&mut session, &format!("i{LONG}<Esc>"));
        assert!(session.text().to_string().contains('\n'));
    }

    #[test]
    fn setlocal_formatoptions_without_t_stops_wrapping() {
        let mut session = session_with_textwidth("", 20);
        feed(&mut session, ":setlocal fo-=t<CR>");
        feed(&mut session, &format!("i{LONG}<Esc>"));
        assert_eq!(session.text().to_string(), LONG);
    }

    #[test]
    fn ex_commands_see_setlocal_textwidth() {
        // `:center` without a width uses 'textwidth'. Vim 9.1 gives
        // "    abc" for tw=11.
        let mut session = HostSession::new("abc");
        feed(&mut session, ":setlocal tw=11<CR>:center<CR>");
        assert_eq!(session.text().to_string(), "    abc");
    }

    #[test]
    fn visual_gq_reads_textwidth() {
        // Vim 9.1 with tw=12.
        let mut session = session_with_textwidth("aa bb cc dd ee ff gg hh ii jj", 12);
        feed(&mut session, "Vgq");
        assert_eq!(
            session.text().to_string(),
            "aa bb cc dd\nee ff gg hh\nii jj"
        );
    }

    #[test]
    fn gq_to_a_mark_reads_textwidth() {
        // Vim 9.1 with tw=12.
        let mut session = session_with_textwidth("aa bb cc dd ee ff\ngg hh ii jj", 12);
        feed(&mut session, "majgq'a");
        assert_eq!(
            session.text().to_string(),
            "aa bb cc dd\nee ff gg hh\nii jj"
        );
    }

    // The Visual, mark and find paths of gq read 'comments' and
    // 'formatoptions' too, not only 'textwidth'. Expected values from Vim
    // 9.1; with the default comments, which have no "--" part, the lines
    // would join as "-- aa -- bb" and a broken line would get no leader.

    #[test]
    fn visual_gq_reads_comments_and_formatoptions() {
        let mut session = HostSession::new("-- aa\n-- bb");
        feed(&mut session, ":setlocal com=:--<CR>Vjgq");
        assert_eq!(session.text().to_string(), "-- aa bb");

        let mut session = HostSession::new("# aa\n# bb");
        feed(&mut session, ":setlocal fo-=q<CR>Vjgq");
        assert_eq!(session.text().to_string(), "# aa # bb");
    }

    #[test]
    fn gq_to_a_mark_reads_comments() {
        let mut session = HostSession::new("-- aa\n-- bb");
        feed(&mut session, ":setlocal com=:--<CR>majgq'a");
        assert_eq!(session.text().to_string(), "-- aa bb");
    }

    #[test]
    fn gq_with_a_find_reads_comments() {
        for keys in ["gqfb", "gqtb"] {
            let mut session = session_with_textwidth("-- aaaa bbbb", 8);
            feed(&mut session, ":setlocal com=:--<CR>");
            feed(&mut session, keys);
            assert_eq!(session.text().to_string(), "-- aaaa\n-- bbbb", "{keys}");
        }
    }

    #[test]
    fn host_set_option_after_ex_set_wins() {
        // `:set tw=20` writes the global value and the buffer's own value.
        // A value the host sets later with set_option() replaces both, as
        // a later `:set` would, so the last change wins.
        let mut session = HostSession::new("");
        feed(&mut session, ":set tw=20<CR>");
        session.set_option(OptionId::TextWidth, &OptionValue::Unsigned(0));
        assert_eq!(
            session.effective_option(OptionId::TextWidth),
            OptionValue::Unsigned(0)
        );
        feed(&mut session, &format!("i{LONG}<Esc>"));
        assert_eq!(session.text().to_string(), LONG);
    }

    #[test]
    fn host_set_option_after_setlocal_wins() {
        let mut session = session_with_textwidth("", 0);
        feed(&mut session, ":setlocal fo-=t<CR>");
        session.set_option(OptionId::TextWidth, &OptionValue::Unsigned(20));
        session.set_option(OptionId::FormatOptions, &OptionValue::Str("tcq".into()));
        feed(&mut session, &format!("i{LONG}<Esc>"));
        assert!(session.text().to_string().contains('\n'));
        assert_eq!(
            session.options().formatoptions(),
            "tcq",
            "the global value is written too"
        );
    }

    #[test]
    fn host_global_write_keeps_buffer_value() {
        // options_mut() and set_options() write the global layer only, as
        // `:setglobal` does, so the buffer's own value from `:set` stays.
        let mut session = HostSession::new("");
        feed(&mut session, ":set tw=20<CR>");
        session.options_mut().set_textwidth(0);
        session.invalidate_option_cache();
        assert_eq!(
            session.effective_option(OptionId::TextWidth),
            OptionValue::Unsigned(20)
        );
    }

    #[test]
    fn setlocal_shiftwidth_reaches_every_cursor() {
        // Ctrl-T is re-run per cursor; both the primary and the secondary
        // must indent by the buffer-local shiftwidth, not the global 4.
        let mut session = HostSession::new("a\nb");
        feed(&mut session, ":setlocal sw=2<CR>");
        session.set_cursor_offset(0);
        session.add_cursor(2).unwrap();
        feed(&mut session, "i<C-t><Esc>");
        assert_eq!(session.text().to_string(), "  a\n  b");
    }
}
