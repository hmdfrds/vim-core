use std::sync::{Arc, Mutex};

use super::seen::{self, Event};
use crate::execution::config::{ConfigText, TrustAll};
use crate::execution::key_notation::parse_key_notation_sequence;
use crate::execution::{
    CommandLineJudge, HostRequest, HostSession, Reach, RuntimeCommand, TrustTag, Verdict,
};
use crate::keymap::KeyEvent;
use proptest::prelude::*;

/// What a command can reach that a sandbox refuses.
const SANDBOXED: Reach = Reach::OPTIONS
    .union(Reach::SECURE_OPTION)
    .union(Reach::MAPPING)
    .union(Reach::ABBREVIATION)
    .union(Reach::LEADER)
    .union(Reach::HANDLER)
    .union(Reach::SHELL)
    .union(Reach::FILE_READ)
    .union(Reach::FILE_WRITE)
    .union(Reach::SOURCE)
    .union(Reach::QUIT)
    .union(Reach::ACTION)
    .union(Reach::CUSTOM)
    .union(Reach::REGISTER_EXECUTION)
    .union(Reach::VARIABLE);

const REFUSAL: &str = "E48: Not allowed in sandbox";

/// One call of the judge.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Judged {
    text: String,
    tags: Vec<u32>,
    refused: bool,
}

type Log = Arc<Mutex<Vec<Judged>>>;

/// A judge that refuses the commands `refuse` picks and logs each call.
struct TestJudge {
    log: Log,
    refuse: fn(&RuntimeCommand<'_>) -> bool,
}

impl CommandLineJudge for TestJudge {
    fn judge(&mut self, command: &RuntimeCommand<'_>) -> Verdict {
        let refused = (self.refuse)(command);
        if let Ok(mut log) = self.log.lock() {
            log.push(Judged {
                text: command.text.to_owned(),
                tags: command.tags.iter().map(|tag| tag.get()).collect(),
                refused,
            });
        }
        if refused {
            Verdict::Refuse(REFUSAL.into())
        } else {
            Verdict::Allow
        }
    }
}

fn sandbox(command: &RuntimeCommand<'_>) -> bool {
    command.reach.intersects(SANDBOXED)
}

fn shell(command: &RuntimeCommand<'_>) -> bool {
    command.reach.contains(Reach::SHELL)
}

fn shell_but_global(command: &RuntimeCommand<'_>) -> bool {
    shell(command) && !command.text.starts_with('g')
}

fn innermost(command: &RuntimeCommand<'_>) -> bool {
    !command.text.starts_with("tabdo")
}

const fn allow(_: &RuntimeCommand<'_>) -> bool {
    false
}

const fn refuse(_: &RuntimeCommand<'_>) -> bool {
    true
}

const TEXT: &str = "a: b/c?d\nfoo:bar\n\nx\n";

/// The tag of a project config.
const PROJECT: u32 = 2;

/// A session that sourced `config` with the tag `tag`, and the log of
/// the judge `refuse` when there is one.
fn sourced(
    config: &str,
    tag: u32,
    refuse: Option<fn(&RuntimeCommand<'_>) -> bool>,
) -> (HostSession, Log) {
    let mut session = HostSession::new(TEXT);
    let _ = session.source_script(
        ConfigText::new("project.vim", config).with_trust(TrustTag::new(tag)),
        &mut TrustAll::new(),
    );
    let log = Log::default();
    if let Some(refuse) = refuse {
        session.set_command_line_judge(Some(Box::new(TestJudge {
            log: Arc::clone(&log),
            refuse,
        })));
    }
    (session, log)
}

fn keys(notation: &str) -> Vec<KeyEvent> {
    parse_key_notation_sequence(notation, None)
}

/// Press `notation`; the host requests and errors it gave.
fn press(session: &mut HostSession, notation: &str) -> (Vec<HostRequest>, Vec<String>) {
    let mut requests = Vec::new();
    let mut errors = Vec::new();
    for key in keys(notation) {
        let response = session.process_key_host(key);
        requests.extend(response.host_requests);
        if let Some(crate::state::StatusMessage::Error(error)) = &response.message {
            errors.push(error.to_string());
        }
    }
    (requests, errors)
}

/// Run `command` as a host runs the command of `:windo` in a window.
fn host_runs(session: &mut HostSession, command: &str) -> Vec<HostRequest> {
    let doc = super::super::shadow_document::OwnedDocument::new(session.text());
    let ctx = crate::execution::InputContext::new(&doc, 0).validate_clamped();
    session
        .engine_mut()
        .execute_ex(command, ctx)
        .take_host_requests()
}

/// Act as a host that runs the command of each `:windo`, `:bufdo` and
/// `:tabdo` among `requests` in two windows, then completes the request.
fn host_each(session: &mut HostSession, requests: Vec<HostRequest>, depth: u8) {
    for request in requests {
        let (HostRequest::ForEachWindow { command, .. }
        | HostRequest::ForEachBuffer { command, .. }
        | HostRequest::ForEachTab { command, .. }) = &request
        else {
            continue;
        };
        if depth < 4 {
            for _ in 0..2 {
                let nested = host_runs(session, command);
                host_each(session, nested, depth + 1);
            }
        }
        let _ = session.complete_request_host(&crate::execution::HostResult::Success {
            id: request.id(),
            message: None,
        });
    }
}

/// Press `notation` with a host that runs each `:windo` and its kind as
/// it comes; the host requests and errors it gave.
fn press_hosted(session: &mut HostSession, notation: &str) -> (Vec<HostRequest>, Vec<String>) {
    let mut requests = Vec::new();
    let mut errors = Vec::new();
    for key in keys(notation) {
        let response = session.process_key_host(key);
        host_each(session, response.host_requests.clone(), 0);
        requests.extend(response.host_requests);
        if let Some(crate::state::StatusMessage::Error(error)) = &response.message {
            errors.push(error.to_string());
        }
    }
    (requests, errors)
}

/// Whether a request is one an Ex command asks of the host.
const fn from_ex(request: &HostRequest) -> bool {
    matches!(
        request,
        HostRequest::ExternalCommand { .. }
            | HostRequest::ReadConfigFile { .. }
            | HostRequest::CustomExCommand { .. }
            | HostRequest::ReadFile { .. }
            | HostRequest::WriteFile { .. }
            | HostRequest::EditFile { .. }
            | HostRequest::ForEachTab { .. }
            | HostRequest::ForEachWindow { .. }
            | HostRequest::ExecuteNorm { .. }
            | HostRequest::FilterDocumentRange { .. }
    )
}

fn asks(requests: &[HostRequest]) -> bool {
    requests.iter().any(from_ex)
}

fn log(log: &Log) -> Vec<Judged> {
    log.lock().map(|log| log.clone()).unwrap_or_default()
}

fn refused_texts(log: &Log) -> Vec<String> {
    self::log(log)
        .into_iter()
        .filter(|judged| judged.refused)
        .map(|judged| judged.text)
        .collect()
}

fn leader(session: &HostSession) -> Vec<KeyEvent> {
    session.engine().leader_keys().to_vec()
}

/// A vector: its config, the keys the user presses, and whether running
/// it changes what `changed` looks at.
struct Vector {
    config: &'static str,
    keys: &'static str,
    /// The command the judge refuses.
    refused: &'static str,
}

/// Run `vector` under a sandbox judge, then with no judge: the judge
/// refuses the command, nothing of it is asked of the host, and `changed`
/// sees no change; with no judge the line runs as before.
fn refused_at_run_time(vector: &Vector, changed: fn(&HostSession) -> bool) {
    let (mut judged, log) = sourced(vector.config, PROJECT, Some(sandbox));
    let (requests, errors) = press(&mut judged, vector.keys);
    assert!(
        refused_texts(&log)
            .iter()
            .any(|text| text == vector.refused),
        "{}: {:?}",
        vector.keys,
        self::log(&log)
    );
    assert!(errors.iter().any(|error| error == REFUSAL), "{errors:?}");
    assert!(
        !requests.iter().any(|request| matches!(
            request,
            HostRequest::ExternalCommand { .. }
                | HostRequest::ReadConfigFile { .. }
                | HostRequest::FilterDocumentRange { .. }
        )),
        "{requests:?}"
    );
    assert!(!changed(&judged), "{} took effect", vector.keys);
    assert!(self::log(&log)
        .iter()
        .all(|judged| judged.tags == [PROJECT]));

    let (mut plain, _) = sourced(vector.config, PROJECT, None);
    let (requests, _) = press(&mut plain, vector.keys);
    assert!(
        changed(&plain) || asks(&requests),
        "{}: with no judge the line runs",
        vector.keys
    );
}

fn langremap(session: &HostSession) -> bool {
    session.engine().options().langremap()
}

fn comma_leader(session: &HostSession) -> bool {
    leader(session) == [KeyEvent::char(',')]
}

const fn nothing(_: &HostSession) -> bool {
    false
}

// ── lines the user finishes ─────────────────────────────────────────

/// A letter typed into a line a mapping left open completes the option.
#[test]
fn a_letter_typed_into_a_line_left_open_is_judged() {
    refused_at_run_time(
        &Vector {
            config: "set nolangremap\nnnoremap x :set langremap<Left><C-h>",
            keys: "xa<CR>",
            refused: "set langremap",
        },
        langremap,
    );
}

/// A `|` the user typed before a command-line mapping ends the user's
/// command, so the mapping's own runs.
#[test]
fn a_bar_typed_before_a_cmap_is_judged_with_it() {
    refused_at_run_time(
        &Vector {
            config: "cnoremap <CR> let mapleader=','<Bar><CR>",
            keys: ":noh |<CR>",
            refused: "let mapleader=','",
        },
        comma_leader,
    );
}

/// A command the user starts, which a command-line mapping completes.
#[test]
fn a_command_a_cmap_completes_is_judged() {
    refused_at_run_time(
        &Vector {
            config: "cnoremap x <Space>f",
            keys: ":sox<CR>",
            refused: "so f",
        },
        nothing,
    );
}

/// A `!` the user types into a line a mapping left open.
#[test]
fn a_bang_typed_into_a_line_left_open_is_judged() {
    refused_at_run_time(
        &Vector {
            config: "nnoremap x :r",
            keys: "x !echo hi<CR>",
            refused: "r !echo hi",
        },
        nothing,
    );
}

// ── lines a mapping makes ────────────────────────────────────────────

/// A search line runs no command: it is never judged, however deep its
/// pattern nests, and the engine stops the pattern itself.
#[test]
fn a_search_line_is_not_judged() {
    let deep = format!("nnoremap x /{}a{}<CR>", "\\%(".repeat(12), "\\)".repeat(12));
    for config in [
        deep.as_str(),
        "vnoremap // y/<C-r>\"<CR>",
        "nnoremap x /<C-r><C-w><CR>",
    ] {
        let (mut session, log) = sourced(config, PROJECT, Some(refuse));
        let lhs = if config.starts_with('v') { "vl//" } else { "x" };
        let (requests, _) = press(&mut session, lhs);
        assert!(
            self::log(&log).is_empty(),
            "{config}: {:?}",
            self::log(&log)
        );
        assert!(!asks(&requests), "{config}");
    }
}

/// A right-hand side of many `<` reaches no command line.
#[test]
fn a_run_of_angle_brackets_reaches_no_judge() {
    let config = format!("nnoremap x {}", "<".repeat(3000));
    let (mut session, log) = sourced(&config, PROJECT, Some(refuse));
    let _ = press(&mut session, "x");
    assert!(self::log(&log).is_empty());
}

/// A `:` that may be the argument of the key before it opens a line that
/// is judged when it does open one.
#[test]
fn a_colon_after_a_key_that_takes_one_is_judged() {
    for rhs in [
        "f:w:!echo hi<CR>",
        "t:j:!echo hi<CR>",
        "wya:x:!echo hi<CR>",
        "d:T<Del>*w:<Left>!echo hi<CR>",
    ] {
        let config = format!("nnoremap x {rhs}");
        let (mut judged, log) = sourced(&config, PROJECT, Some(sandbox));
        let (requests, _) = press(&mut judged, "x");
        assert!(
            !requests
                .iter()
                .any(|request| matches!(request, HostRequest::ExternalCommand { .. })),
            "{rhs}: {requests:?}"
        );
        let judged = self::log(&log);
        assert!(
            judged.iter().all(|judged| judged.tags == [PROJECT]),
            "{rhs}: {judged:?}"
        );
        let ran = !judged.is_empty();
        let (mut plain, _) = sourced(&config, PROJECT, None);
        let (requests, _) = press(&mut plain, "x");
        assert_eq!(
            ran,
            asks(&requests),
            "{rhs}: judged only what runs a command, {judged:?}, {requests:?}"
        );
    }
}

/// A `<Cmd>` whose `<CR>` never comes, as when the typeahead drops a long
/// right-hand side's tail, runs with the user's Enter and is judged.
#[test]
fn a_cmd_left_open_is_judged_at_the_users_enter() {
    refused_at_run_time(
        &Vector {
            config: "nnoremap x <Cmd>!echo hi",
            keys: "x<CR>",
            refused: "!echo hi",
        },
        nothing,
    );
    let config = format!("nnoremap x <Cmd>!echo hi{}<Esc>", "<Left>".repeat(3000));
    let (mut session, log) = sourced(&config, PROJECT, Some(sandbox));
    let (requests, _) = press(&mut session, "x<CR>");
    assert!(self::log(&log).is_empty());
    assert!(!asks(&requests));
}

/// A `<Cmd>` error that cuts a mapping short leaves its line open, and
/// the tag stays on it.
#[test]
fn a_line_a_cmd_error_left_open_keeps_its_tag() {
    refused_at_run_time(
        &Vector {
            config: "nnoremap x :so<Cmd>nosuchcommand<CR><Space>f",
            keys: "x<CR>",
            refused: "so",
        },
        nothing,
    );
}

/// A line left open is judged with whatever is typed into it.
#[test]
fn a_line_left_open_is_judged_with_what_the_user_types() {
    for (config, keys, refused, changed) in [
        (
            "nnoremap x :let mapleader=','<Bar>",
            "x <CR>",
            "let mapleader=','",
            comma_leader as fn(&HostSession) -> bool,
        ),
        (
            "set nolrm\nnnoremap x :set langremap<Bar>",
            "x1<CR>",
            "set langremap",
            langremap,
        ),
        (
            "set nolrm\nnnoremap x :set lrm<Bar>",
            "x1<CR>",
            "set lrm",
            langremap,
        ),
        (
            "set nolrm\nnnoremap x :setlocal langremap<Bar>",
            "x1<CR>",
            "setlocal langremap",
            langremap,
        ),
        (
            "set nolrm\nnnoremap x :setglobal langremap<Bar>",
            "x1<CR>",
            "setglobal langremap",
            langremap,
        ),
        ("nnoremap x :sof<Left>", "x <CR>", "so f", nothing),
    ] {
        refused_at_run_time(
            &Vector {
                config,
                keys,
                refused,
            },
            changed,
        );
    }
}

/// A command-line mapping types into what the user typed before it, once
/// or twice.
#[test]
fn a_cmap_is_judged_with_the_users_text() {
    for (config, keys, refused, changed) in [
        (
            "set nolrm\ncnoremap x langremap",
            ":set x<CR>",
            "set langremap",
            langremap as fn(&HostSession) -> bool,
        ),
        (
            "cnoremap x mapleader=','",
            ":let x<CR>",
            "let mapleader=','",
            comma_leader,
        ),
        (
            "cnoremap x let mapleader=','<Bar>",
            ":xx<CR>",
            "let mapleader=','",
            comma_leader,
        ),
    ] {
        refused_at_run_time(
            &Vector {
                config,
                keys,
                refused,
            },
            changed,
        );
    }
    // After a `'` the user typed, a mark range.
    let (mut session, log) = sourced("cnoremap x <,'>!echo hi", PROJECT, Some(sandbox));
    let (requests, _) = press(&mut session, "Vj<Esc>:'x<CR>");
    assert_eq!(refused_texts(&log), ["'<,'>!echo hi"]);
    assert!(!asks(&requests));
}

/// A command nested to the depth limit is judged at every level.
#[test]
fn each_level_of_a_nested_line_is_judged() {
    let config = format!(
        "nnoremap x :{}tabdo gl! so<Cmd><CR><Left><Cmd><CR>",
        "tabdo ".repeat(7)
    );
    let (mut session, log) = sourced(&config, PROJECT, Some(innermost));
    let (requests, _) = press(&mut session, "x<CR>");
    let judged = self::log(&log);
    assert!(judged.len() >= 9, "{judged:?}");
    assert!(judged.iter().any(|judged| judged.refused), "{judged:?}");
    assert!(!asks(&requests), "{requests:?}");
}

/// Many `<Cmd>` ends in a line left open run in time, each judged once.
#[test]
fn many_cmd_ends_are_judged_once_each() {
    let config = format!(
        "nnoremap x {}{}{}",
        ":".repeat(16),
        "1,".repeat(500),
        "<Cmd><CR>".repeat(500)
    );
    let (mut session, log) = sourced(&config, PROJECT, Some(allow));
    let _ = press(&mut session, "x");
    assert!(self::log(&log).len() <= 500);
}

/// A Visual `:` puts `'<,'>` before the line, which is judged as it
/// runs, with the lines it names.
#[test]
fn a_visual_range_is_judged_with_the_line() {
    refused_at_run_time(
        &Vector {
            config: "vnoremap x :!echo hi<CR>",
            keys: "Vx",
            refused: "1,1!echo hi",
        },
        nothing,
    );
}

// ── what keeps the tag ──────────────────────────────────────────────

/// A typed line is never judged, nor one only an untagged mapping made.
#[test]
fn an_untagged_line_is_never_judged() {
    let (mut session, log) = sourced("nnoremap x :!echo hi<CR>", 0, Some(refuse));
    let (requests, _) = press(&mut session, ":!echo typed<CR>x");
    assert!(self::log(&log).is_empty());
    assert_eq!(
        requests
            .iter()
            .filter(|request| matches!(request, HostRequest::ExternalCommand { .. }))
            .count(),
        2
    );
}

/// The tags of a command-line mapping on a line it only passed through.
#[test]
fn a_line_keeps_the_tags_of_every_mapping() {
    let mut session = HostSession::new(TEXT);
    for (tag, config) in [(1, "cnoremap x a"), (3, "nnoremap y :")] {
        let _ = session.source_script(
            ConfigText::new("c.vim", config).with_trust(TrustTag::new(tag)),
            &mut TrustAll::new(),
        );
    }
    let log = Log::default();
    session.set_command_line_judge(Some(Box::new(TestJudge {
        log: Arc::clone(&log),
        refuse: allow,
    })));
    let _ = press(&mut session, "ynohx<BS><CR>");
    assert_eq!(
        self::log(&log),
        [Judged {
            text: "noh".into(),
            tags: vec![1, 3],
            refused: false
        }]
    );
}

/// `@:`, `:@:`, a recalled line, `<C-r>:` and the command window run the
/// last line with its tags.
#[test]
fn the_last_line_and_history_keep_the_tag() {
    for keys in [
        "@:",
        ":@:<CR>",
        ":<Up><CR>",
        ":<C-r>:<CR>",
        ":2<Up><Up><CR>",
    ] {
        let (mut session, log) = sourced("nnoremap x :2,$s/^/-/<CR>", PROJECT, Some(refuse));
        let _ = press(&mut session, "x");
        let first = self::log(&log).len();
        assert_eq!(first, 1, "{keys}");
        let (_, errors) = press(&mut session, keys);
        assert!(
            self::log(&log).len() > first,
            "{keys}: {:?}",
            self::log(&log)
        );
        assert!(errors.iter().any(|error| error == REFUSAL), "{keys}");
    }
}

/// A count before `@:` repeats the line, judged each time.
#[test]
fn a_count_on_at_colon_repeats_with_the_tag() {
    let (mut session, log) = sourced("nnoremap x :s/^/-/<CR>", PROJECT, Some(allow));
    let _ = press(&mut session, "x");
    let _ = press(&mut session, "3@:");
    assert!(self::log(&log)
        .iter()
        .all(|judged| judged.tags == [PROJECT]));
    assert_eq!(self::log(&log).len(), 4, "{:?}", self::log(&log));
}

/// The keys of `:normal` and of a register run from a tagged line keep
/// its tag.
#[test]
fn normal_and_register_keys_keep_the_tag() {
    let (mut session, log) = sourced(
        "nnoremap x :normal :!echo hi<lt>CR><CR>",
        PROJECT,
        Some(shell),
    );
    let (requests, _) = press(&mut session, "x");
    assert_eq!(refused_texts(&log), ["!echo hi"]);
    assert!(!asks(&requests), "{requests:?}");

    let (mut session, log) = sourced("nnoremap x :@a<CR>", PROJECT, Some(shell));
    let _ = session.engine_mut().registers_mut().set(
        crate::primitives::RegisterName::new_unchecked('a'),
        crate::primitives::RegisterContent::char_wise("!echo hi"),
    );
    let (requests, _) = press(&mut session, "x");
    assert_eq!(refused_texts(&log), ["!echo hi"]);
    assert!(!asks(&requests), "{requests:?}");

    let (mut session, log) = sourced("nnoremap x @a", PROJECT, Some(sandbox));
    let _ = session.engine_mut().registers_mut().set(
        crate::primitives::RegisterName::new_unchecked('a'),
        crate::primitives::RegisterContent::char_wise(":!echo hi\r"),
    );
    let (requests, _) = press(&mut session, "x");
    assert_eq!(refused_texts(&log), ["!echo hi"]);
    assert!(!asks(&requests), "{requests:?}");
}

/// A register a mapping filled, which the user runs, is the user's.
#[test]
fn a_register_the_user_runs_is_not_tagged() {
    let (mut session, log) = sourced("nnoremap x \"ayy", PROJECT, Some(refuse));
    let _ = session.process_key_host(KeyEvent::char('x'));
    let _ = press(&mut session, ":@a<CR>");
    assert!(self::log(&log).is_empty());
}

/// An `<expr>` mapping's keys carry its tag.
#[test]
fn the_keys_of_an_expr_mapping_carry_its_tag() {
    let (mut session, log) = sourced("nnoremap <expr> x 'foo'", PROJECT, Some(refuse));
    let response = session.process_key_host(KeyEvent::char('x'));
    let request = response
        .host_requests
        .iter()
        .find(|request| matches!(request, HostRequest::EvaluateMapping { .. }))
        .expect("an evaluation");
    let result = crate::execution::HostResult::Data {
        id: request.id(),
        data: ":!echo hi<CR>".into(),
        offset: None,
    };
    let _ = session.complete_request_host(&result);
    assert_eq!(refused_texts(&log), ["!echo hi"]);
}

/// A recursive mapping that runs the user's own mapping keeps its tag.
#[test]
fn a_mapping_run_by_a_tagged_key_carries_both_tags() {
    let mut session = HostSession::new(TEXT);
    let _ = session.source_script(
        ConfigText::new("user.vim", "nnoremap y :!echo hi<CR>"),
        &mut TrustAll::new(),
    );
    let _ = session.source_script(
        ConfigText::new("project.vim", "nmap x y").with_trust(TrustTag::new(PROJECT)),
        &mut TrustAll::new(),
    );
    let log = Log::default();
    session.set_command_line_judge(Some(Box::new(TestJudge {
        log: Arc::clone(&log),
        refuse: sandbox,
    })));
    let (requests, _) = press(&mut session, "x");
    assert_eq!(refused_texts(&log), ["!echo hi"]);
    assert!(!asks(&requests));
}

/// The judge sees every `|` command before any runs, and one refused
/// stops the line there.
#[test]
fn every_command_is_judged_before_any_runs() {
    let (mut session, _) = sourced(
        "nnoremap x :s/^/1/<Bar>!echo hi<Bar>s/^/2/<CR>",
        PROJECT,
        None,
    );
    let log = Log::default();
    session.set_command_line_judge(Some(Box::new(TestJudge {
        log: Arc::clone(&log),
        refuse: sandbox,
    })));
    let _ = seen::take();
    let _ = press(&mut session, "x");
    let events = seen::take();
    let judged = events
        .iter()
        .take_while(|event| matches!(event, Event::Judged(..)))
        .count();
    assert_eq!(judged, 2, "{events:?}");
    assert_eq!(
        events.get(2),
        Some(&Event::Ran("s/^/1/".into(), true)),
        "{events:?}"
    );
    assert_eq!(session.text(), "1a: b/c?d\nfoo:bar\n\nx\n");
}

/// Each command of a `:g` list is judged after the `:g`.
#[test]
fn a_global_list_is_judged() {
    let (mut session, log) = sourced(
        "nnoremap x :g/foo/s/^/-/<Bar>!echo hi<CR>",
        PROJECT,
        Some(shell_but_global),
    );
    let (requests, _) = press(&mut session, "x");
    let texts: Vec<String> = self::log(&log)
        .into_iter()
        .map(|judged| judged.text)
        .collect();
    assert_eq!(texts, ["g/foo/s/^/-/|!echo hi", "s/^/-/", "!echo hi"]);
    assert!(!asks(&requests));
    assert_eq!(session.text(), TEXT);
}

/// With no judge installed a tagged line runs as an untagged one does.
#[test]
fn no_judge_runs_every_line() {
    let (mut tagged, _) = sourced("nnoremap x :s/^/-/<Bar>!echo hi<CR>", PROJECT, None);
    let (mut untagged, _) = sourced("nnoremap x :s/^/-/<Bar>!echo hi<CR>", 0, None);
    let (a, _) = press(&mut tagged, "x");
    let (b, _) = press(&mut untagged, "x");
    assert_eq!(tagged.text(), untagged.text());
    assert_eq!(a.len(), b.len());
}

// ── commit paths ─────────────────────────────────────────────────────

/// The places a command line runs. A change here is a way to run a line
/// the judge may not see: give it the line's tags, then update the counts.
#[test]
fn a_command_line_runs_only_where_it_is_judged() {
    let engine = [
        include_str!("../engine.rs"),
        include_str!("command_line.rs"),
        include_str!("command_line_exec.rs"),
        include_str!("ex_line.rs"),
        include_str!("mode_dispatch.rs"),
        include_str!("host_completion.rs"),
        include_str!("public_api.rs"),
        include_str!("source.rs"),
        include_str!("mapping.rs"),
        include_str!("conditionals.rs"),
        include_str!("insert.rs"),
        include_str!("substitute_confirm.rs"),
        include_str!("../executor_ex.rs"),
        include_str!("../host_api.rs"),
        include_str!("../session_host.rs"),
    ];
    let count = |needle: &str| -> usize {
        engine
            .iter()
            .map(|source| {
                let code = ["\n#[cfg(test)]\nmod ", "\n#[cfg(test)]\n#[path"]
                    .iter()
                    .filter_map(|end| source.find(end))
                    .min()
                    .map_or(*source, |end| &source[..end]);
                code.matches(needle).count()
            })
            .sum()
    };
    // Enter, a recalled line, Visual `:`, `execute_ex` and a command
    // window line (`submit_command_line_text`) run through
    // `execute_ex_command_line`; `@:` and the end of a `<Cmd>` through
    // `run_ex_line`; the lines of `:@:` and `:@r` through the one
    // `run_nested_line` that judges them, and `:g` lists, judged with
    // their `:g`, through the other two; each command through
    // `execute_ex_piece`, which with `&` and a config's own judged
    // commands are all that run `execute_ex_command`.
    assert_eq!(
        (
            count(".execute_ex_command_line("),
            count(".run_ex_line("),
            count(".run_nested_line("),
            count("execute_ex_piece("),
            count("execute_ex_command("),
        ),
        (3, 3, 3, 1, 3),
        "a command line runs in a new place: judge it"
    );
}

/// Each commit path of a tagged line goes to the judge.
#[test]
fn every_commit_path_is_judged() {
    for (config, keys) in [
        ("nnoremap x :!echo hi<CR>", "x"),
        ("nnoremap x <Cmd>!echo hi<CR>", "x"),
        ("nnoremap x :!echo hi", "x<CR>"),
        ("vnoremap x :!echo hi<CR>", "vx"),
        ("nnoremap x @:", "x"),
        ("nnoremap x :@:<CR>", "x"),
        ("nnoremap x :<Up><CR>", "x"),
    ] {
        let (mut session, log) = sourced(config, PROJECT, Some(refuse));
        // A typed line, for the paths that run the last one.
        let _ = press(&mut session, ":noh<CR>");
        let _ = press(&mut session, keys);
        assert!(!self::log(&log).is_empty(), "{config}");
    }
}

/// Run `line` as the host runs a line of the command window.
fn submit(session: &mut HostSession, line: &str) {
    let doc = super::super::shadow_document::OwnedDocument::new(TEXT);
    let ctx = crate::execution::InputContext::new(&doc, 0).validate_clamped();
    let _ = session.engine_mut().submit_command_line_text(line, ctx);
}

/// A line of the command window a tagged key opened is judged.
#[test]
fn a_command_window_line_is_judged() {
    let (mut session, log) = sourced("nnoremap x q:", PROJECT, Some(refuse));
    let _ = press(&mut session, "x");
    submit(&mut session, "!echo hi");
    assert_eq!(refused_texts(&log), ["!echo hi"]);

    // A history entry a tagged line left, run from a window the user opened.
    let (mut session, log) = sourced("nnoremap x :s/^/-/<CR>", PROJECT, Some(refuse));
    let _ = press(&mut session, "x:!echo typed<CR>q:");
    submit(&mut session, "s/^/-/");
    assert_eq!(refused_texts(&log), ["s/^/-/", "s/^/-/"]);
    let _ = press(&mut session, "q:");
    submit(&mut session, "!echo typed");
    assert_eq!(refused_texts(&log), ["s/^/-/", "s/^/-/"]);
}

// ── what a tagged line defines ───────────────────────────────────────

/// The tag of the normal-mode mapping of `lhs`, if there is one.
fn mapping_tag(session: &HostSession, lhs: char) -> Option<u32> {
    session
        .engine()
        .keymap()
        .get_user_mapping(KeyEvent::char(lhs), crate::primitives::Mode::Normal)
        .map(|entry| entry.trust().get())
}

/// A mapping a tagged line defines keeps the tag, so the line its keys
/// make is judged, however the line ran.
#[test]
fn a_mapping_a_tagged_line_defines_keeps_its_tag() {
    for config in [
        "nnoremap x :nnoremap y :!echo hi<lt>CR><CR>",
        "nnoremap x <Cmd>nnoremap y :!echo hi<lt>CR><CR>",
        "nnoremap x :g/foo/nnoremap y :!echo hi<lt>CR><CR>",
        "nnoremap x :normal :nnoremap y :!echo hi<lt>lt>CR><lt>CR><CR>",
        "nnoremap x <Cmd>normal :nnoremap y :!echo hi<lt>lt>CR><lt>CR><CR>",
        "nnoremap x <Cmd>g/foo/nnoremap y :!echo hi<lt>CR><CR>",
    ] {
        let (mut session, log) = sourced(config, PROJECT, Some(shell));
        let _ = press(&mut session, "x");
        assert_eq!(mapping_tag(&session, 'y'), Some(PROJECT), "{config}");
        let (requests, errors) = press(&mut session, "y");
        assert_eq!(refused_texts(&log), ["!echo hi"], "{config}");
        assert!(errors.iter().any(|error| error == REFUSAL), "{config}");
        assert!(!asks(&requests), "{config}: {requests:?}");
    }
}

/// `@:` of a line that defined a mapping defines it with the line's tag.
#[test]
fn a_mapping_at_colon_defines_keeps_the_tag() {
    let (mut session, log) = sourced(
        "nnoremap x :nnoremap y :!echo hi<lt>CR><CR>",
        PROJECT,
        Some(shell),
    );
    let _ = press(&mut session, "x");
    let _ = session
        .engine_mut()
        .unmap(crate::keymap::MappingMode::Normal, &keys("y"));
    assert_eq!(mapping_tag(&session, 'y'), None);
    let _ = press(&mut session, "@:");
    assert_eq!(mapping_tag(&session, 'y'), Some(PROJECT));
    let (requests, _) = press(&mut session, "y");
    assert_eq!(refused_texts(&log), ["!echo hi"]);
    assert!(!asks(&requests), "{requests:?}");
}

/// A mapping defined by a mapping a tagged line defined keeps the tag
/// too.
#[test]
fn a_chain_of_defined_mappings_keeps_the_tag() {
    let (mut session, log) = sourced(
        "nnoremap x :nnoremap y :nnoremap z :!echo hi<lt>lt>CR><lt>CR><CR>",
        PROJECT,
        Some(shell),
    );
    let _ = press(&mut session, "xy");
    assert_eq!(mapping_tag(&session, 'y'), Some(PROJECT));
    assert_eq!(mapping_tag(&session, 'z'), Some(PROJECT));
    let (requests, _) = press(&mut session, "z");
    assert_eq!(refused_texts(&log), ["!echo hi"]);
    assert!(!asks(&requests), "{requests:?}");
    assert!(self::log(&log)
        .iter()
        .all(|judged| judged.tags == [PROJECT]));
}

/// An abbreviation a tagged line defines keeps the tag.
#[test]
fn an_abbreviation_a_tagged_line_defines_keeps_its_tag() {
    for (config, trigger) in [
        ("nnoremap x :cnoreabbrev zz !echo hi<CR>", "zz"),
        ("nnoremap x :inoreabbrev zz foo<CR>", "zz"),
        ("nnoremap x <Cmd>cabbrev zz !echo hi<CR>", "zz"),
        ("nnoremap x :g/foo/cabbrev zz !echo hi<CR>", "zz"),
    ] {
        let (mut session, _) = sourced(config, PROJECT, Some(shell));
        let _ = press(&mut session, "x");
        let tag = session
            .engine()
            .abbreviations()
            .get(trigger)
            .map(|entry| entry.trust.get());
        assert_eq!(tag, Some(PROJECT), "{config}");
    }
}

/// A line made by keys of several tags gives what it defines the
/// highest of them, and nothing else defines with a tag.
#[test]
fn a_definition_keeps_the_highest_tag_of_its_line() {
    let mut session = HostSession::new(TEXT);
    for (tag, config) in [
        (3, "nnoremap x :"),
        (1, "cnoremap w nnoremap y :!echo hi<lt>CR>"),
    ] {
        let _ = session.source_script(
            ConfigText::new("c.vim", config).with_trust(TrustTag::new(tag)),
            &mut TrustAll::new(),
        );
    }
    let log = Log::default();
    session.set_command_line_judge(Some(Box::new(TestJudge {
        log: Arc::clone(&log),
        refuse: allow,
    })));
    let _ = press(&mut session, "xw<CR>");
    assert_eq!(mapping_tag(&session, 'y'), Some(3));
    let _ = press(&mut session, ":nnoremap z :noh<lt>CR><CR>");
    assert_eq!(mapping_tag(&session, 'z'), Some(0));
    let _ = press(&mut session, "y");
    assert_eq!(
        self::log(&log).last(),
        Some(&Judged {
            text: "!echo hi".into(),
            tags: vec![3],
            refused: false
        })
    );
}

/// The command `:windo`, `:bufdo` and `:tabdo` give the host keeps the
/// tag of their line, so what it defines keeps it too.
#[test]
fn a_command_the_host_runs_for_windo_keeps_the_tag() {
    for config in [
        "nnoremap x :windo nnoremap y :!echo hi<lt>CR><CR>",
        "nnoremap x :bufdo nnoremap y :!echo hi<lt>CR><CR>",
        "nnoremap x :tabdo nnoremap y :!echo hi<lt>CR><CR>",
        "nnoremap x <Cmd>windo nnoremap y :!echo hi<lt>CR><CR>",
    ] {
        let (mut session, log) = sourced(config, PROJECT, Some(shell));
        let _ = press_hosted(&mut session, "x");
        // With the line, then each time the host runs it.
        let judged = Judged {
            text: "nnoremap y :!echo hi<CR>".into(),
            tags: vec![PROJECT],
            refused: false,
        };
        assert_eq!(
            self::log(&log)
                .iter()
                .filter(|seen| **seen == judged)
                .count(),
            3,
            "{config}: {:?}",
            self::log(&log)
        );
        assert_eq!(mapping_tag(&session, 'y'), Some(PROJECT), "{config}");
        assert!(session.engine().run_tags.each.is_empty(), "{config}");
        let (requests, errors) = press_hosted(&mut session, "y");
        assert_eq!(refused_texts(&log), ["!echo hi"], "{config}");
        assert!(errors.iter().any(|error| error == REFUSAL), "{config}");
        assert!(!asks(&requests), "{config}: {requests:?}");
    }

    let (mut session, log) = sourced("", PROJECT, Some(refuse));
    let _ = press_hosted(&mut session, ":windo nnoremap y :!echo hi<lt>CR><CR>");
    assert_eq!(mapping_tag(&session, 'y'), Some(0));
    let (requests, _) = press_hosted(&mut session, "y");
    assert!(self::log(&log).is_empty(), "{:?}", self::log(&log));
    assert!(asks(&requests), "{requests:?}");
}

/// The tag of a `:windo` lasts until its request completes or is
/// cancelled.
#[test]
fn the_tag_of_windo_ends_with_its_request() {
    let (mut session, _) = sourced("nnoremap x :windo set ic<CR>", PROJECT, Some(allow));
    for complete in [true, false] {
        let (requests, _) = press(&mut session, "x");
        let each: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request, HostRequest::ForEachWindow { .. }))
            .collect();
        let [request] = each.as_slice() else {
            panic!("{requests:?}");
        };
        let _ = host_runs(&mut session, "nnoremap y :noh<CR>");
        assert_eq!(mapping_tag(&session, 'y'), Some(PROJECT));
        if complete {
            let _ = session.complete_request_host(&crate::execution::HostResult::Success {
                id: request.id(),
                message: None,
            });
        } else {
            assert!(session.cancel_request(request.id()));
        }
        let _ = host_runs(&mut session, "nnoremap y :noh<CR>");
        assert_eq!(mapping_tag(&session, 'y'), Some(0), "complete: {complete}");
    }
}

/// `:normal` in a `<Cmd>` runs, as in Vim, where `<Cmd>` runs its line
/// as an Ex command (getchar.c `getcmdkeycmd()`, ex_docmd.c
/// `ex_normal()`); from a tagged one its keys keep the tag.
#[test]
fn normal_in_a_cmd_runs_with_the_tag() {
    let (mut session, _) = sourced("nnoremap x <Cmd>normal! dd<CR>", 0, None);
    let _ = press(&mut session, "x");
    assert_eq!(session.text(), "foo:bar\n\nx\n");

    let (mut session, log) = sourced(
        "nnoremap x <Cmd>normal :!echo hi<lt>CR><CR>",
        PROJECT,
        Some(shell),
    );
    let (requests, _) = press(&mut session, "x");
    assert_eq!(refused_texts(&log), ["!echo hi"]);
    assert!(!asks(&requests), "{requests:?}");
}

// ── random mappings ──────────────────────────────────────────────────

/// Keys a user types that cannot open or run a command line themselves.
const USER_KEYS: &str = r"([a-fh-pr-z0-9 |!/]|<CR>|<Esc>|<BS>|<Left>|<C-u>|<C-h>){0,6}";

/// Keys of a right-hand side.
const RHS_KEYS: &str =
    r"([a-z0-9:!| ]|<CR>|<Esc>|<BS>|<Left>|<C-u>|<C-r>:|<Cmd>|<Up>|@:|q:|\x22a){1,14}";

/// How a tagged line defines `y` (or `z`, through `y`) with a right-hand
/// side, which goes in once escaped for `{1}`, twice for `{2}`.
const DEFINERS: [&str; 11] = [
    ":nmap y {1}<CR>",
    "<Cmd>nmap y {1}<CR>",
    ":g/foo/nmap y {1}<CR>",
    "<Cmd>g/foo/nmap y {1}<CR>",
    ":normal :nmap y {2}<lt>CR><CR>",
    "<Cmd>normal :nmap y {2}<lt>CR><CR>",
    ":nmap y :nmap z {2}<lt>CR><CR>",
    ":cnoreabbrev y {1}<CR>",
    ":windo nmap y {1}<CR>",
    "<Cmd>bufdo nmap y {1}<CR>",
    ":tabdo nmap y :nmap z {2}<lt>CR><CR>",
];

/// `keys` as text a line types, which a mapping then reads as keys.
fn escaped(keys: &str) -> String {
    keys.replace('<', "<lt>").replace('|', "<lt>Bar>")
}

/// A right-hand side: random keys, or a definition of random keys.
fn rhs_keys() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => RHS_KEYS.prop_map(|keys| keys),
        1 => (0..DEFINERS.len(), RHS_KEYS).prop_map(|(definer, keys)| {
            let once = escaped(&keys);
            DEFINERS[definer]
                .replace("{2}", &escaped(&once))
                .replace("{1}", &once)
        }),
    ]
}

/// What pressing a mapping of `rhs`, then `user` and Enter, then `y` and
/// `z` judged and ran, and the tags of what that defined; `None` when
/// `rhs` defines no mapping.
fn run_random(
    rhs: &str,
    cmap: bool,
    user: &str,
    refuse: fn(&RuntimeCommand<'_>) -> bool,
) -> Option<(Vec<Event>, Vec<u32>)> {
    let config = if cmap {
        format!("cnoremap x {rhs}")
    } else {
        format!("nnoremap x {rhs}")
    };
    let (mut session, _) = sourced(&config, PROJECT, Some(refuse));
    let mode = if cmap {
        crate::primitives::Mode::CommandLine
    } else {
        crate::primitives::Mode::Normal
    };
    session
        .engine()
        .keymap()
        .get_user_mapping(KeyEvent::char('x'), mode)?;
    let _ = session.engine_mut().registers_mut().set(
        crate::primitives::RegisterName::new_unchecked('a'),
        crate::primitives::RegisterContent::char_wise(":s/^/-/\r"),
    );
    let _ = seen::take();
    let start = if cmap { ":" } else { "" };
    let _ = press_hosted(
        &mut session,
        &format!("{start}x{user}<CR><Esc>y<Esc>z<Esc>"),
    );
    let mut defined: Vec<u32> = ['y', 'z']
        .into_iter()
        .filter_map(|lhs| mapping_tag(&session, lhs))
        .collect();
    defined.extend(
        session
            .engine()
            .abbreviations()
            .get("y")
            .map(|entry| entry.trust.get()),
    );
    Some((seen::take(), defined))
}

proptest! {
    /// Under a judge that refuses everything no command of a tagged line
    /// runs; under one that allows everything each command of a tagged
    /// line was judged before it ran, and what a tagged line defined keeps
    /// the tag.
    #[test]
    fn every_command_of_a_tagged_line_is_judged_first(
        rhs in rhs_keys(),
        cmap in any::<bool>(),
        user in USER_KEYS,
    ) {
        let run = run_random(&rhs, cmap, &user, refuse);
        prop_assume!(run.is_some());
        let (events, _) = run.unwrap_or_default();
        for event in &events {
            if let Event::Ran(piece, tagged) = event {
                // The user's keys open no line, so every line is tagged.
                prop_assert!(*tagged, "{rhs} {user}: {piece} ran untagged: {events:?}");
                // Only to give its error.
                let parses = crate::grammar::ex_parser::parse_ex_piece(piece).1.is_ok();
                prop_assert!(!parses, "{rhs} {user}: {piece} ran: {events:?}");
            }
        }
        let (events, defined) = run_random(&rhs, cmap, &user, allow).unwrap_or_default();
        prop_assert!(
            defined.iter().all(|tag| *tag == PROJECT),
            "{rhs} {user}: defined {defined:?}"
        );
        let mut judged: Vec<&str> = Vec::new();
        for event in &events {
            match event {
                Event::Judged(text, _) => judged.push(text),
                Event::Ran(piece, tagged) => {
                    prop_assert!(*tagged, "{rhs} {user}: {piece} ran untagged: {events:?}");
                    let parses = crate::grammar::ex_parser::parse_ex_piece(piece).1.is_ok();
                    prop_assert!(
                        !parses || judged.iter().any(|text| *text == piece.as_str()),
                        "{rhs} {user}: {piece} ran unjudged: {events:?}"
                    );
                }
            }
        }
    }
}
