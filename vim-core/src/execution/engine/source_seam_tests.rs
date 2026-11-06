//! The seam between a config and [`ConfigHost::judge`]: what the host is
//! asked about, and that only what it allows changes anything.

use super::source::seam;
use crate::effects::{Effect, SourceContext};
use crate::execution::config::{
    ConfigCommand, ConfigHost, ConfigOutcome, ConfigText, Reach, SourceReply, SourceRequest,
    TrustTag, Verdict,
};
use crate::execution::{Response, VimEngine};
use crate::grammar::types::ModifierFlags;
use crate::keymap::KeyEvent;
use crate::primitives::Mode;
use proptest::prelude::*;

/// What the host saw of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    file: String,
    trust: u32,
    depth: u32,
    line: u32,
    text: String,
    reach: Reach,
    action: String,
}

/// A host that records each command it judges, decides by its place
/// among them, and answers `:source` with the files it has.
struct Recorder {
    verdicts: Vec<Verdict>,
    files: &'static [(&'static str, &'static str, u32)],
    seen: Vec<Seen>,
}

impl Recorder {
    fn new(verdicts: Vec<Verdict>) -> Self {
        Self {
            verdicts,
            files: &[],
            seen: Vec::new(),
        }
    }

    fn with_files(self, files: &'static [(&'static str, &'static str, u32)]) -> Self {
        Self { files, ..self }
    }
}

impl ConfigHost for Recorder {
    fn judge(&mut self, command: &ConfigCommand<'_>) -> Verdict {
        let verdict = match self.verdicts.len() {
            0 => Verdict::Allow,
            n => self.verdicts[self.seen.len() % n].clone(),
        };
        self.seen.push(Seen {
            file: command.file.to_owned(),
            trust: command.trust.get(),
            depth: command.depth,
            line: command.line,
            text: command.text.to_owned(),
            reach: command.reach,
            action: format!("{:?}", command.action),
        });
        verdict
    }

    fn source(&mut self, request: &SourceRequest<'_>) -> SourceReply {
        let path = request.path.unwrap_or_default();
        match self.files.iter().find(|(name, ..)| *name == path) {
            Some((name, text, trust)) => SourceReply::Text {
                name: (*name).to_owned(),
                text: (*text).to_owned(),
                trust: TrustTag::new(*trust),
            },
            None if path == "refused.vim" => SourceReply::Refuse("no file".to_owned()),
            None => SourceReply::Defer,
        }
    }
}

/// Files a [`Recorder`] can source, one of which sources itself.
const FILES: &[(&str, &str, u32)] = &[
    ("b.vim", "set ic\nsource c.vim | set tw=7\n", 2),
    (
        "c.vim",
        "nnoremap x y\nif 1\nset nosuch\nset sw=2\nendif\n",
        3,
    ),
    ("r.vim", "source r.vim\n", 4),
];

/// Run `text` with `host`, recording the seam.
fn run(text: &str, host: &mut Recorder) -> (VimEngine, Response, ConfigOutcome, Vec<seam::Entry>) {
    let mut engine = VimEngine::new();
    seam::start();
    let config = ConfigText::new("a.vim", text).with_trust(TrustTag::new(1));
    let sourced = engine.source_script(config, host);
    let entries = seam::take();
    (engine, sourced.response, sourced.outcome, entries)
}

/// What every command the config ran keeps to: a command that changed the
/// engine or asked the host anything was judged once and allowed, and ran
/// as judged; one refused or claimed did nothing.
fn check_seam(entries: &[seam::Entry], host: &Recorder, outcome: ConfigOutcome) {
    let mut judged = 0;
    for entry in entries {
        let text = &entry.text;
        assert!(entry.judged.len() <= 1, "{text:?} judged twice");
        judged += entry.judged.len();
        if let Some((command, Verdict::Allow)) = entry.judged.first() {
            assert_eq!(entry.executed, std::slice::from_ref(command), "{text:?}");
        } else {
            assert!(entry.executed.is_empty(), "{text:?} ran unjudged");
            assert!(!entry.changed, "{text:?} changed the engine");
            assert_eq!(entry.requests, 0, "{text:?} asked the host");
        }
    }
    assert_eq!(judged, host.seen.len());
    assert_eq!(u32::try_from(judged).ok(), Some(outcome.commands));
    let count = |f: fn(&Verdict) -> bool| {
        let n = entries
            .iter()
            .filter(|e| e.judged.first().is_some_and(|(_, v)| f(v)))
            .count();
        u32::try_from(n).unwrap_or(u32::MAX)
    };
    assert_eq!(count(|v| matches!(v, Verdict::Claim)), outcome.claimed);
    assert_eq!(count(|v| matches!(v, Verdict::Refuse(_))), outcome.refused);
}

/// Each string literal of `source`, a Rust file, that holds a line end.
fn literals(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '\'' => {
                // A char literal such as '"'; a lifetime has no closing quote
                // two characters on.
                let mut ahead = chars.clone();
                let first = ahead.next();
                if first == Some('\\') {
                    ahead.next();
                }
                if ahead.next() == Some('\'') {
                    chars = ahead;
                }
            }
            '"' => {
                let mut text = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => text.push('\n'),
                            Some('t') => text.push('\t'),
                            Some('r') => text.push('\r'),
                            Some('0') => text.push('\0'),
                            Some('u') => {
                                let code: String =
                                    chars.by_ref().skip(1).take_while(|&c| c != '}').collect();
                                let code = u32::from_str_radix(&code, 16).ok();
                                text.extend(code.and_then(char::from_u32));
                            }
                            Some('\n') => while chars.next_if(|c| c.is_whitespace()).is_some() {},
                            Some(c) => text.push(c),
                            None => {}
                        },
                        c => text.push(c),
                    }
                }
                if text.contains('\n') {
                    found.push(text);
                }
            }
            _ => {}
        }
    }
    found
}

/// A shared vimrc as many are written: Vim commands vim-core lacks, a
/// block for the GUI, an augroup, a function, a Lua heredoc.
const TYPICAL: &str = r#"set nocompatible
filetype plugin indent on
syntax on
set encoding=utf-8
let mapleader = "\<Space>"
nnoremap <leader>w :w<CR>
nnoremap <leader>q :q<CR>
nnoremap <silent> <leader>h :nohlsearch<CR>
set number relativenumber
set tabstop=4 shiftwidth=4 expandtab
set ignorecase smartcase incsearch hlsearch
set textwidth=99
if has('gui_running')
  set guifont=Monospace\ 11
else
  set textwidth=99
endif
augroup vimrc
  autocmd!
  autocmd FileType gitcommit setlocal textwidth=72
augroup END
function! StripTrailing()
  %s/\s\+$//e
endfunction
command! Strip call StripTrailing()
iabbrev teh the
lua << EOF
x = 1
EOF
echo "vimrc loaded"
"#;

/// Lines for each way Vim reads a sourced file: continuation, the bar,
/// modifiers, ranges, commands that need a buffer, `:finish`, errors,
/// blocks, bodies, `:let`, `:autocmd`, lines a command takes, and the
/// commands vim-core runs itself.
const VECTORS: &[&str] = &[
    "set tw=1\n  \\ 0\n\"\\ set ic\nset ts=2\n",
    "map x\n\\ y\n",
    "set ic | set tw=3\nmap x a|b\nmap y a\\|b\nnnoremap z :echo 'a'<Bar>echo 'b'<CR>\n",
    "normal! ihi|there\nexecute 'set ic' | set tw=3\ng/x/s/a/b/|set ic\n",
    "autocmd BufRead * set ts=2 | set sw=3\ncommand! Foo echo 1 | set ic\n",
    "set ic \" comment | set tw=3\necho \"a|b\" | set ts=4\nlet x = 'a|b' | set sw=1\n",
    "map <buffer> x y\nr !ls | set ic\n!ls | set ic\nw !cat|set ic\n",
    "silent! set nosuch | set ic\nsilent set tw?\nverbose set tw?\n3verbose set ic\n",
    "noautocmd set ic\nkeepmarks set ic\nleftabove split\nrightbelow vsplit\n",
    "unsilent echo 1\nsandbox set ic\nsilent! source b.vim\nvim9cmd set ic\nlegacy set ic\n",
    "topleft new\ntab split\nkeepjumps set ic\nkeeppatterns set ic\nlockmarks set ic\n",
    "hide | set ic\nnoswapfile e x\n1,5d\n%s/a/b/\n:3|set ic\n'<,'>d\n.\n3\n1,$y\n",
    "normal ggdd\nd\nput a\nk a\nmark a\n",
    "g/x/s/a/b/|set ic|nnoremap a b\nv/x/d|set tw=3|map y z\nka|set ic\n'a,'bmark b|set tw=4\n",
    "set ic\nfinish\nset tw=3\n",
    "if 1 | finish | endif\nset tw=3\n",
    "set tw=abc\nset ic\nif 1\nset nosuch\nset ic\nendif\nset tw=3\n",
    "if 0\nset ic\nelseif 1\nset tw=3\nelse\nset ts=2\nendif\n",
    "if has('nvim') | set ic | else | set tw=3 | endif\nif\nendif\nif 1\nendif junk\n",
    "while 1\nset ic\nendwhile\nfor x in [1]\nset tw=3\nendfor\ntry\nset ts=2\nendtry\n",
    "function! F()\n  set ic\nendfunction\ncall F()\ndef G()\nenddef\n",
    "let g:mapleader = ','\nlet maplocalleader = '\\\\'\nlet &tw = 30\nlet &l:sw = 2\n",
    "let &g:ts = 3\nlet g:x = 1\nlet g:x += 2\nunlet g:x\nunlet\nunlet! g:y\n",
    "let &shell = 'sh'\nlet &lmap = 'ab'\nlet &mouse = 'a'\nunlet g:mapleader\n",
    "let x =<< trim END\n  set ic\nEND\nset tw=3\n",
    "augroup x\nau!\nau BufRead * nested set ic\naugroup END\n",
    "autocmd BufRead * {\n set ic\n}\nset tw=3\n",
    "append\nset ic\n.\ninsert\nfoo\n.\npython3 << EOF\nimport os\nEOF\n",
    "loadkeymap\na b\nset ic\n",
    "set langmap=abc;def\nset lmap=\nset langnoremap\nset nolrm\n",
    "source b.vim\nsource\nsource c.vim | set ic\nso! d.vim\nruntime x.vim\n",
    "source refused.vim | set ic\nsilent! source refused.vim | set tw=3\n",
    "source r.vim\nset ic\n",
    "sethandler <C-a> a-n:vim\nsethandler <C-b> godot\nsethandler\n",
    "iabbrev a b\ncabbrev c d\nunabbreviate a\niunabbrev c\nabclear\nabbreviate\n",
    "nnoremap <expr> x 'y'\nnmap <unique> <silent> <nowait> z y\nunmap z\nnunmap x\n",
    "mapclear\nimapclear\nmap\nnmap x\n",
    "w\nq\nwq\nx\ne foo\nsp\nvs\ntabnew\nbn\ncnext\naction Foo\n",
    "messages clear\nregisters\nnohlsearch\nclearjumps\nmkvimrc\n",
    "Foo bar\nfiletype on\nscriptencoding utf-8\nvim9script\nset ic\n",
];

/// Every config of the corpus: the loader's tests, a typical vimrc and
/// the vectors.
fn corpus() -> Vec<String> {
    let mut texts = literals(include_str!("../../../tests/config_loader.rs"));
    texts.extend(literals(include_str!("../../../tests/source_command.rs")));
    texts.push(TYPICAL.to_owned());
    texts.extend(VECTORS.iter().map(|&text| text.to_owned()));
    texts
}

fn refuse() -> Verdict {
    Verdict::Refuse("no".into())
}

#[test]
fn the_corpus_is_read() {
    let texts = corpus();
    assert!(texts.len() > 100, "{}", texts.len());
    assert!(texts
        .iter()
        .any(|t| t == "lua << EOF\nx = 1\nEOF\nset tw=33\n"));
}

#[test]
fn only_what_the_host_allows_changes_anything() {
    let modes = [
        vec![],
        vec![refuse()],
        vec![Verdict::Claim],
        vec![Verdict::Allow, refuse(), Verdict::Claim],
        vec![refuse(), Verdict::Allow],
    ];
    for text in corpus() {
        for verdicts in &modes {
            let mut host = Recorder::new(verdicts.clone()).with_files(FILES);
            let (_, _, outcome, entries) = run(&text, &mut host);
            check_seam(&entries, &host, outcome);
        }
    }
}

#[test]
fn a_refused_or_claimed_config_changes_nothing() {
    for verdict in [refuse(), Verdict::Claim] {
        let pristine = VimEngine::new();
        let mut host = Recorder::new(vec![verdict]).with_files(FILES);
        let (engine, response, _, _) = run(TYPICAL, &mut host);
        assert_eq!(engine.options(), pristine.options());
        assert!(engine
            .keymap()
            .get_user_mapping(KeyEvent::char(' '), Mode::Normal)
            .is_none());
        assert!(engine.abbrev_table.is_empty());
        assert_eq!(response.host_requests(), []);
        assert_ne!(host.seen, []);
    }
}

#[test]
fn the_host_sees_where_each_command_is_and_what_it_does() {
    let text = "set ic | silent! set lmap=x\nsource b.vim\nset ts=4\n";
    let mut host = Recorder::new(vec![]).with_files(FILES);
    let (engine, response, outcome, _) = run(text, &mut host);
    let at = |s: &Seen| (s.file.clone(), s.trust, s.depth, s.line, s.text.clone());
    let seen: Vec<_> = host.seen.iter().map(at).collect();
    let row = |file: &str, trust, depth, line, text: &str| {
        (file.to_owned(), trust, depth, line, text.to_owned())
    };
    assert_eq!(
        seen,
        [
            row("a.vim", 1, 1, 1, "set ic"),
            row("a.vim", 1, 1, 1, "silent! set lmap=x"),
            row("a.vim", 1, 1, 2, "source b.vim"),
            row("b.vim", 2, 2, 1, "set ic"),
            row("b.vim", 2, 2, 2, "source c.vim"),
            row("c.vim", 3, 3, 1, "nnoremap x y"),
            row("c.vim", 3, 3, 3, "set nosuch"),
            row("b.vim", 2, 2, 2, "set tw=7"),
            row("a.vim", 1, 1, 3, "set ts=4"),
        ]
    );
    let lmap = &host.seen[1];
    assert_eq!(lmap.reach, Reach::OPTIONS | Reach::SECURE_OPTION);
    assert!(lmap
        .action
        .contains("name: \"langmap\", known: true, secure: true"));
    assert_eq!(
        host.seen[2].reach,
        Reach::HOST | Reach::SOURCE | Reach::FILE_READ | Reach::NAMED_PATH
    );
    assert_eq!(host.seen[2].action, "Source { path: Some(\"b.vim\") }");
    let mapping = engine
        .keymap()
        .get_user_mapping(KeyEvent::char('x'), Mode::Normal);
    assert_eq!(mapping.map(|m| m.trust().get()), Some(3));
    assert_eq!(outcome.commands, 9);
    assert_eq!(response.host_requests(), []);
}

#[test]
fn the_host_sees_the_action_of_each_kind_of_command() {
    let text = "let mapleader = ','\nlet g:x = 1\nlet &sh = 'x'\nnnoremap <silent> a b\n\
                iabbrev t the\nfiletype on\nFoo bar\nw\necho 1\nunlet g:x\n";
    let mut host = Recorder::new(vec![]);
    let _ = run(text, &mut host);
    let kinds: Vec<_> = host
        .seen
        .iter()
        .map(|s| {
            s.action
                .split([' ', '('])
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "Leader",
            "Other",
            "SetOptions",
            "Map",
            "Abbreviate",
            "Custom",
            "Custom",
            "Host",
            "Message",
            "Other"
        ]
    );
    assert_eq!(host.seen[0].reach, Reach::VARIABLE | Reach::LEADER);
    assert_eq!(host.seen[1].reach, Reach::VARIABLE);
    assert_eq!(host.seen[2].reach, Reach::OPTIONS | Reach::SECURE_OPTION);
    assert!(host.seen[2]
        .action
        .contains("\"shell\", known: false, secure: true"));
    assert!(host.seen[5]
        .action
        .contains("name: \"filetype\", args: \"on\", vim_command: true"));
    assert!(host.seen[6]
        .action
        .contains("name: \"Foo\", args: \"bar\", vim_command: false"));
    assert_eq!(host.seen[7].reach, Reach::HOST | Reach::FILE_WRITE);
}

/// The errors the response shows, with their lines.
fn located(response: &Response) -> Vec<(u32, String)> {
    response
        .effects()
        .iter()
        .filter_map(|e| match e {
            Effect::ShowError {
                error,
                source: Some(SourceContext { line, .. }),
            } => Some((*line, error.to_string())),
            _ => None,
        })
        .collect()
}

/// A refusal is an error: the rest of its line does not run, nor, inside
/// a block, the rest of the outermost block (ex_docmd.c, do_cmdline():
/// "stop skipping cmds for an error msg after all endif/while/for").
#[test]
fn a_refusal_inside_a_block_skips_to_the_outermost_end() {
    let text = "if 1\nset ic\nif 1\nset tw=10 | set et\nendif\nset ts=3\nendif\nset sw=5\n";
    let mut host = Recorder::new(vec![Verdict::Allow, refuse(), Verdict::Allow]);
    let (engine, response, outcome, entries) = run(text, &mut host);
    check_seam(&entries, &host, outcome);
    let texts: Vec<_> = host.seen.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["set ic", "set tw=10", "set sw=5"]);
    assert_eq!(located(&response), [(4, "no".to_owned())]);
    assert!(engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    assert_eq!(
        engine.options().tabstop(),
        VimEngine::new().options().tabstop()
    );
    assert_eq!(engine.options().shiftwidth(), 5);
    assert_eq!(
        (outcome.commands, outcome.refused, outcome.errors),
        (3, 1, 1)
    );
}

/// `:g` takes the rest of the line, every `|` command in it, as one piece
/// (ex_cmds.c, `ex_global()`); with no buffer the config gives an error
/// for the whole piece.
#[test]
fn a_global_takes_the_commands_after_it() {
    let text = "g/x/s/a/b/|set ic|set tw=3\nset sw=5\n";
    let mut host = Recorder::new(Vec::new());
    let (engine, response, outcome, entries) = run(text, &mut host);
    check_seam(&entries, &host, outcome);
    let texts: Vec<_> = host.seen.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["set sw=5"]);
    let errors = located(&response);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].0, 1);
    assert!(
        errors[0].1.contains("g/x/s/a/b/|set ic|set tw=3"),
        "{errors:?}"
    );
    assert!(!engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
}

#[test]
fn silent_bang_hides_a_refusal_and_the_line_goes_on() {
    let text = "silent! set tw=10 | set ic\nset ts=3 | set sw=5\n";
    let mut host = Recorder::new(vec![refuse(), Verdict::Allow, refuse()]);
    let (engine, response, outcome, _) = run(text, &mut host);
    assert_eq!(located(&response), [(2, "no".to_owned())]);
    let modifiers: Vec<_> = host
        .seen
        .iter()
        .map(|s| s.text.starts_with("silent!"))
        .collect();
    assert_eq!(modifiers, [true, false, false]);
    assert!(engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    assert_eq!(
        engine.options().shiftwidth(),
        VimEngine::new().options().shiftwidth()
    );
    assert_eq!(
        (outcome.commands, outcome.refused, outcome.errors),
        (3, 2, 1)
    );
}

#[test]
fn a_claimed_command_shows_nothing_and_the_line_goes_on() {
    let text = "set tw=10 | set ic\n";
    let mut host = Recorder::new(vec![Verdict::Claim, Verdict::Allow]);
    let (engine, response, outcome, _) = run(text, &mut host);
    assert!(response.effects().is_empty(), "{:?}", response.effects());
    assert!(engine.options().ignorecase());
    assert_eq!(engine.options().textwidth(), 0);
    assert_eq!(
        (outcome.commands, outcome.claimed, outcome.errors),
        (2, 1, 0)
    );
}

#[test]
fn the_modifiers_of_a_command_reach_the_host() {
    struct Modifiers(Vec<ModifierFlags>);
    impl ConfigHost for Modifiers {
        fn judge(&mut self, command: &ConfigCommand<'_>) -> Verdict {
            self.0.push(command.modifiers);
            Verdict::Allow
        }
        fn source(&mut self, _request: &SourceRequest<'_>) -> SourceReply {
            SourceReply::Text {
                name: "b.vim".to_owned(),
                text: "set ic\n".to_owned(),
                trust: TrustTag::default(),
            }
        }
    }
    let mut host = Modifiers(Vec::new());
    let mut engine = VimEngine::new();
    let text = ConfigText::new(
        "a.vim",
        "silent! set tw=1\nsilent source b.vim\n3verbose noa set ts=2\n",
    );
    let _ = engine.source_script(text, &mut host);
    assert_eq!(
        host.0,
        [
            ModifierFlags::SILENT | ModifierFlags::SILENT_BANG,
            ModifierFlags::SILENT,
            ModifierFlags::SILENT,
            ModifierFlags::VERBOSE | ModifierFlags::NOAUTOCMD,
        ]
    );
}

/// `:if` and `:elseif` conditions only read: the host judges nothing, is
/// asked nothing and nothing changes (eval0() has no side effect for the
/// functions vim-core has).
#[test]
fn conditions_are_pure() {
    let text = "if has('x') && exists('g:y') || &tw == 0\nendif\n\
                if 0\nelseif has('eval') && 1 + 1 == 2\nendif\n";
    let mut host = Recorder::new(vec![]);
    let (engine, response, outcome, entries) = run(text, &mut host);
    assert_eq!(host.seen, []);
    assert_eq!(outcome.commands, 0);
    assert_eq!(response.host_requests(), []);
    assert_eq!(entries.len(), 5);
    assert!(entries.iter().all(|e| !e.changed && e.judged.is_empty()));
    assert_eq!(engine.options(), VimEngine::new().options());
}

/// A piece of config text, from the kinds of commands the loader treats
/// apart.
fn piece() -> impl Strategy<Value = String> {
    let words = prop::sample::select(vec![
        "set ic",
        "set tw=33",
        "set nosuch",
        "set lmap=ab",
        "silent! set tw=x",
        "nnoremap a b",
        "map x a|b",
        "iabbrev t the",
        "unmap a",
        "mapclear",
        "abclear",
        "let mapleader = ','",
        "let g:x = 1",
        "let &tw = 5",
        "unlet g:x",
        "unlet! g:x",
        "echo 'hi'",
        "echo x",
        "source b.vim",
        "source c.vim",
        "source r.vim",
        "source refused.vim",
        "if 1",
        "if 0",
        "elseif 1",
        "else",
        "endif",
        "while 1",
        "endwhile",
        "finish",
        "filetype on",
        "Foo",
        "w",
        "q",
        "d",
        "normal x",
        "g/a/s//b/|set ic",
        "v/x/d|set tw=3|nnoremap a b",
        "g/x/g/y/s/a/b/|map y z",
        "ka",
        "mark b",
        "lua << EOF",
        "EOF",
        "function F()",
        "endfunction",
        "let x =<< END",
        "END",
        "sethandler <C-a> vim",
        "vim9script",
        "\"comment",
        "\\ continued",
        "  ",
        "messages clear",
        "3verbose set ic",
    ]);
    prop_oneof![
        4 => words.prop_map(str::to_owned),
        1 => "[ -~]{0,16}",
    ]
}

fn config_text() -> impl Strategy<Value = String> {
    let line = prop::collection::vec(piece(), 1..4).prop_map(|pieces| pieces.join(" | "));
    prop::collection::vec(line, 0..12).prop_map(|lines| {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    })
}

fn verdicts() -> impl Strategy<Value = Vec<Verdict>> {
    let verdict = prop_oneof![
        3 => Just(Verdict::Allow),
        1 => Just(Verdict::Refuse("no".into())),
        1 => Just(Verdict::Claim),
    ];
    prop::collection::vec(verdict, 0..8)
}

proptest! {
    #[test]
    fn random_configs_and_verdicts_keep_the_seam(text in config_text(), verdicts in verdicts()) {
        let mut host = Recorder::new(verdicts).with_files(FILES);
        let (_, _, outcome, entries) = run(&text, &mut host);
        check_seam(&entries, &host, outcome);
    }
}
