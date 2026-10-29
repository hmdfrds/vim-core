//! Cases where an earlier version matched Vim and a later change did not,
//! found by replaying random key sequences through headless Vim 9.1 and
//! vim-core side by side.
//!
//! The fixture was recorded with `vim -Nu NONE --not-a-term`, `nostartofline`,
//! `nojoinspaces`, `backspace=indent,eol,start` and explicit `:setlocal`
//! options. The keys go through `feedkeys()` one at a time (an
//! `InsertCharPre` autocommand makes Vim read them as typed), and a sequence
//! that ends in Insert or Replace mode gets an `<Esc>`. Each case checks the
//! text, the cursor (line and byte column), the mode and the unnamed
//! register with its type. A text that ends in `\n` has an empty last line.
//!
//! Every case runs twice: as recorded, and with an empty fold provider, as
//! a host that supports folds has when nothing is folded.

use serde::Deserialize;
use vim_core::execution::{parse_keys_from_string, HostRequest, HostResult, HostSession};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{Mode, VimOptions};

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    lines: Vec<String>,
    cursor: [usize; 2],
    keys: String,
    opts: Opts,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct Opts {
    tw: usize,
    fo: String,
    ai: bool,
    et: bool,
    ts: usize,
    sw: usize,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Expected {
    lines: Vec<String>,
    cursor: [usize; 2],
    mode: String,
    reg: String,
    regtype: String,
}

fn options(o: &Opts) -> VimOptions {
    let mut opts = VimOptions::default();
    opts.set_textwidth(o.tw);
    opts.set_formatoptions(o.fo.as_str());
    opts.set_tabstop(o.ts);
    opts.set_shiftwidth(o.sw);
    opts.set_expandtab(o.et);
    opts.set_autoindent(o.ai);
    opts.set_smartindent(false);
    opts.set_auto_pairs(None);
    opts
}

/// Answer every host request with success, as a host that has nothing to do
/// for them would.
fn complete(session: &mut HostSession, requests: &[HostRequest]) {
    let mut pending = requests.to_vec();
    for _ in 0..64 {
        if pending.is_empty() {
            break;
        }
        for request in std::mem::take(&mut pending) {
            let result = HostResult::Success {
                id: request.id(),
                message: None,
            };
            pending.extend(session.complete_request_host(&result).host_requests);
        }
    }
}

fn press(session: &mut HostSession, key: KeyEvent) {
    let result = session.process_key_host(key);
    complete(session, &result.host_requests);
}

fn mode_name(mode: Mode) -> String {
    match mode {
        Mode::Visual(v) => match format!("{v:?}") {
            s if s.starts_with("Line") => "V".to_owned(),
            s if s.starts_with("Block") => "B".to_owned(),
            _ => "v".to_owned(),
        },
        Mode::Normal | Mode::OperatorPending(..) => "n".to_owned(),
        other => format!("{other:?}"),
    }
}

fn run(case: &Case, folds: bool) -> Expected {
    let text = case.lines.join("\n");
    let mut session = HostSession::new(&text);
    session.set_options(options(&case.opts));
    let [row, col] = case.cursor;
    let offset = case.lines[..row].iter().map(|l| l.len() + 1).sum::<usize>() + col;
    session.set_cursor_offset(offset);
    if folds {
        session.set_fold_state(Vec::new());
    }
    for key in parse_keys_from_string(&case.keys) {
        press(&mut session, key);
    }
    if matches!(session.mode(), Mode::Insert | Mode::Replace) {
        press(&mut session, KeyEvent::escape());
    }
    let text = session.text().to_owned();
    // Bytes, so that a cursor inside a character still shows.
    let before = &text.as_bytes()[..session.cursor_offset().min(text.len())];
    let cursor = [
        before.iter().filter(|&&b| b == b'\n').count(),
        before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(before.len(), |nl| before.len() - nl - 1),
    ];
    let (reg, regtype) = match session.get_register('"') {
        Some((reg, kind)) if !reg.is_empty() => {
            let kind = format!("{kind:?}");
            let kind = if kind.starts_with("Line") {
                "V"
            } else if kind.starts_with("Block") {
                "B"
            } else {
                "v"
            };
            (reg, kind.to_owned())
        }
        _ => (String::new(), String::new()),
    };
    Expected {
        lines: text.split('\n').map(str::to_owned).collect(),
        cursor,
        mode: mode_name(session.mode()),
        reg,
        regtype,
    }
}

#[test]
fn matches_vim_oracle() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/vim_gate_oracle.json")).unwrap();
    assert!(!fixture.cases.is_empty());
    let failures: Vec<String> = fixture
        .cases
        .iter()
        .flat_map(|case| [(case, false), (case, true)])
        .filter_map(|(case, folds)| {
            let actual = run(case, folds);
            (actual != case.expected).then(|| {
                format!(
                    "{}{} ({:?} at {:?}, keys {:?})\n  expected {:?}\n  actual   {:?}",
                    case.name,
                    if folds { " [empty fold provider]" } else { "" },
                    case.lines.join("\n"),
                    case.cursor,
                    case.keys,
                    case.expected,
                    actual
                )
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} runs differ from Vim:\n\n{}",
        failures.len(),
        2 * fixture.cases.len(),
        failures.join("\n\n")
    );
}
