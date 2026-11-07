//! How deep Ex commands nest: 30 levels of `:g`, `:windo`, `:normal`, `@:`
//! and the like, then `E169: Command too recursive` (ex_docmd.c
//! `do_cmdline()`), each test on a 1 MiB stack.

use vim_core::effects::Effect;
use vim_core::execution::{
    parse_keys_from_string, ConfigText, HostSession, Response, TrustAll, VimEngine,
};
use vim_core::grammar::parse_ex_command;

const E169: &str = "E169: Command too recursive";

fn keys(session: &mut HostSession, keys: &str) {
    for key in parse_keys_from_string(keys) {
        session.process_key_host(key);
    }
}

/// Run `test` on a thread with a 1 MiB stack.
fn on_small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(test)
        .expect("a thread")
        .join()
        .expect("no panic");
}

/// The error a fresh session on `text` shows after `typed`, and its text.
fn typed(text: &str, typed: &str) -> (Option<String>, String) {
    let mut session = HostSession::new(text);
    keys(&mut session, typed);
    (session.errmsg(), session.text().to_owned())
}

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

#[test]
fn global_nests_30_deep() {
    on_small_stack(|| {
        for prefix in ["g/x/", "v/y/"] {
            let (error, text) = typed("x\nx", &format!(":{}d<CR>", prefix.repeat(30)));
            assert_eq!((error, text.as_str()), (None, ""), "{prefix}");
            let (error, text) = typed("x\nx", &format!(":{}d<CR>", prefix.repeat(31)));
            assert_eq!(error.as_deref(), Some(E169), "{prefix}");
            assert_eq!(text, "x\nx", "{prefix}");
        }
    });
}

#[test]
fn commands_with_a_command_argument_nest_30_deep() {
    on_small_stack(|| {
        for prefix in ["windo ", "bufdo ", "tabdo ", "sx/x/", "sy/x/"] {
            let (error, _) = typed("x", &format!(":{}d<CR>", prefix.repeat(30)));
            assert_ne!(error.as_deref(), Some(E169), "{prefix}");
            let (error, text) = typed("x", &format!(":{}d<CR>", prefix.repeat(31)));
            assert_eq!(error.as_deref(), Some(E169), "{prefix}");
            assert_eq!(text, "x", "{prefix}");
        }
    });
}

#[test]
fn the_parser_gives_e169_past_30_levels() {
    on_small_stack(|| {
        for prefix in ["windo ", "sx/x/", "windo sx/x/"] {
            let line = |n: usize| format!("{}d", prefix.repeat(n));
            let deepest = 30 / prefix.split(' ').filter(|p| !p.is_empty()).count();
            assert!(parse_ex_command(&line(deepest)).is_ok(), "{prefix}");
            let error = parse_ex_command(&line(deepest + 1)).expect_err(prefix);
            assert_eq!(error.to_string(), E169);
        }
    });
}

/// `:nnoremap ,000 :normal ,001<CR>` and on, `,{depth}` inserting `y`.
fn normal_chain(session: &mut HostSession, depth: usize, last: &str) {
    for level in 0..depth {
        let next = level + 1;
        keys(
            session,
            &format!(":nnoremap ,{level:03} :normal ,{next:03}<lt>CR><CR>"),
        );
    }
    keys(session, &format!(":nnoremap ,{depth:03} {last}<CR>"));
    keys(session, ",000");
}

#[test]
fn normal_nests_30_deep() {
    on_small_stack(|| {
        let mut session = HostSession::new("x");
        normal_chain(&mut session, 30, "iy<lt>Esc>");
        assert_eq!((session.errmsg(), session.text()), (None, "yx"));
        let mut session = HostSession::new("x");
        normal_chain(&mut session, 31, "iy<lt>Esc>");
        assert_eq!(session.errmsg().as_deref(), Some(E169));
        assert_eq!(session.text(), "x");
    });
}

#[test]
fn a_line_inside_29_normals_nests_one_level() {
    on_small_stack(|| {
        let mut session = HostSession::new("x\nx");
        normal_chain(&mut session, 29, ":g/x/d<lt>CR>");
        assert_eq!((session.errmsg(), session.text()), (None, ""));
        let mut session = HostSession::new("x\nx");
        normal_chain(&mut session, 29, ":g/x/g/x/d<lt>CR>");
        assert_eq!(session.errmsg().as_deref(), Some(E169));
        assert_eq!(session.text(), "x\nx");
    });
}

#[test]
fn a_typed_line_of_150_globals_gives_e169() {
    on_small_stack(|| {
        let (error, text) = typed("x\nx", &format!(":{}d<CR>", "g/x/".repeat(150)));
        assert_eq!(error.as_deref(), Some(E169));
        assert_eq!(text, "x\nx");
    });
}

#[test]
fn a_register_and_the_last_line_each_count_one_level() {
    on_small_stack(|| {
        for (n, error) in [(29, None), (30, Some(E169))] {
            let line = format!("{}d", "g/x/".repeat(n));
            let mut session = HostSession::new(&format!("{line}\nx"));
            keys(&mut session, "\"ayy:@a<CR>");
            assert_eq!(session.errmsg().as_deref(), error, ":@a, {n}");
            let mut session = HostSession::new("x\nx");
            keys(&mut session, &format!(":{line}<CR>u@:"));
            assert_eq!(session.errmsg().as_deref(), error, "@:, {n}");
        }
    });
}

#[test]
fn a_register_that_runs_itself_gives_e169() {
    on_small_stack(|| {
        let (error, text) = typed("@a\nx", "\"ayy:@a<CR>");
        assert_eq!((error.as_deref(), text.as_str()), (Some(E169), "@a\nx"));
    });
}

/// ex_docmd.c `ex_normal()` gives E192 past 'maxmapdepth'; vim-core E169.
#[test]
fn a_mapping_that_runs_normal_on_its_own_key_gives_e169() {
    on_small_stack(|| {
        let (error, text) = typed("x", ":nnoremap x :norm x<lt>CR><CR>x");
        assert_eq!((error.as_deref(), text.as_str()), (Some(E169), "x"));
    });
}

/// The other depth limits fit the same stack: the expression evaluator's
/// 300 (eval.c E1169) and the regex group nesting.
#[test]
fn every_depth_limit_fits_a_small_stack() {
    on_small_stack(|| {
        let parens = |n: usize| format!("echo {}1{}\n", "(".repeat(n), ")".repeat(n));
        let groups = |n: usize| format!("/{}x{}<CR>", r"\%(".repeat(n), r"\)".repeat(n));
        let echo = |text: String| {
            errors(
                &VimEngine::new()
                    .source_script(ConfigText::new("", &text), &mut TrustAll::new())
                    .response,
            )
        };
        assert_eq!(echo(parens(299)), Vec::<String>::new());
        assert!(echo(parens(300))[0].starts_with("E1169"));
        assert_eq!(typed("x", &groups(10)).0, None);
        assert!(typed("x", &groups(11)).0.is_some());
        let lines = format!(":{}{}d<CR>", "g/x/".repeat(15), "windo ".repeat(15));
        assert_eq!(typed("x", &lines).0, None);
    });
}
