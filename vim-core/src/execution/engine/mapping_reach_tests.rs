use std::collections::HashSet;

use super::seen;
use crate::execution::config::{
    ConfigAction, ConfigCommand, ConfigHost, ConfigText, MappingReach, Opaque, Reach, Verdict,
};
use crate::execution::key_notation::parse_key_notation_with;
use crate::execution::{HostRequest, HostSession, VimEngine};
use crate::grammar::ex_parser::MAX_NESTING;
use crate::grammar::split_ex_pipeline;
use crate::grammar::types::{ExCommand, MapModePrefix};
use crate::keymap::{KeyEvent, MappingFlags, MappingKind};
use crate::primitives::Mode;
use proptest::prelude::*;

/// A host that allows everything and keeps each mapping defined, with what
/// its keys reach.
#[derive(Default)]
struct Maps(Vec<(MapModePrefix, String, MappingReach)>);

impl ConfigHost for Maps {
    fn judge(&mut self, command: &ConfigCommand<'_>) -> Verdict {
        if let ConfigAction::Map(map) = command.action {
            self.0
                .push((map.modes, map.lhs.to_owned(), map.reach.clone()));
        }
        Verdict::Allow
    }
}

/// What the mappings `config` defines reach, in order.
fn reaches(config: &str) -> Vec<MappingReach> {
    let mut engine = VimEngine::new();
    let mut maps = Maps::default();
    let _ = engine.source_script(ConfigText::new("test.vim", config), &mut maps);
    maps.0.into_iter().map(|(.., reach)| reach).collect()
}

/// What the last mapping `config` defines reaches.
fn reach(config: &str) -> MappingReach {
    reaches(config).pop().expect("a mapping")
}

/// The texts of the commands it reaches, closed ones bare and open ones
/// after `open: `.
fn texts(reach: &MappingReach) -> Vec<String> {
    reach
        .commands
        .iter()
        .map(|command| {
            if command.open {
                format!("open: {}", command.text)
            } else {
                command.text.to_string()
            }
        })
        .collect()
}

fn has(reach: &MappingReach, text: &str) -> bool {
    texts(reach).iter().any(|known| known == text)
}

// ── command lines ─────────────────────────────────────────────────────

#[test]
fn a_command_line_the_keys_commit_is_reached() {
    let reach = reach("nnoremap x :!ls<CR>");
    assert!(has(&reach, "!ls"), "{:?}", texts(&reach));
    assert!(has(&reach, "'<,'>!ls"), "{:?}", texts(&reach));
    assert!(reach.reach().contains(Reach::SHELL));
    assert_eq!(reach.opaque, vec![Opaque::FilterOperator]);
}

#[test]
fn each_command_of_a_bar_line_is_reached() {
    let reach = reach("nnoremap x :noh<Bar>so f<CR>");
    assert!(has(&reach, "noh"));
    assert!(has(&reach, "so f"));
    assert!(reach.reach().contains(Reach::SOURCE));
}

#[test]
fn a_cmd_command_is_reached() {
    let reach = reach("nnoremap x <Cmd>so f<CR>");
    assert_eq!(texts(&reach), ["so f"]);
    assert!(reach.opaque.is_empty());
}

#[test]
fn edits_in_the_command_line_are_those_of_the_handler() {
    let reach = reach("nnoremap x :ab<Home>so f <End><CR>");
    assert!(has(&reach, "so f ab"), "{:?}", texts(&reach));
    let reach = self::reach("nnoremap x :!ls<C-u>noh<CR>");
    assert!(has(&reach, "noh"));
    assert!(!reach
        .commands
        .iter()
        .any(|c| c.reach.contains(Reach::SHELL)));
    let reach = self::reach("nnoremap x :<BS>so f<CR>");
    assert!(!has(&reach, "so f"), "Backspace cancels an empty line");
}

#[test]
fn a_line_left_open_is_reached_with_what_the_user_may_type() {
    let reach = reach("nnoremap x :sof<Left>");
    // `so|f` is two commands.
    for text in [
        "open: sof",
        "open: so f",
        "open: so",
        "open: f",
        "open: sof1",
    ] {
        assert!(has(&reach, text), "{text}: {:?}", texts(&reach));
    }
    let reach = self::reach("nnoremap x <Cmd>so f");
    assert!(has(&reach, "open: so f"));
    assert!(has(&reach, "open: so f1"));
}

#[test]
fn a_line_open_when_a_cmd_command_runs_may_be_left_open() {
    // The command may fail, which drops the keys after it.
    let reach = reach("nnoremap x :!ls<Cmd>echo<CR><C-u><Esc>");
    assert!(has(&reach, "open: !ls"), "{:?}", texts(&reach));
}

#[test]
fn every_colon_opens_a_line_and_the_keys_after_it_are_commands_too() {
    // `f:` takes the `:` as its argument; the second `:` opens the line.
    let reach = reach("nnoremap x f:w:!echo hi<CR>");
    assert!(has(&reach, "!echo hi"), "{:?}", texts(&reach));
    let reach = self::reach("nnoremap x d:T<Del>*w:<Left>!echo hi<CR>");
    assert!(reach.reach().contains(Reach::SHELL), "{:?}", texts(&reach));
}

// ── phases ────────────────────────────────────────────────────────────

#[test]
fn insert_mode_keys_are_text_until_one_leaves_it() {
    let reach = reach("inoremap x a:b<Esc>:!ls<CR>");
    assert!(has(&reach, "!ls"));
    assert!(!has(&reach, "b"), "{:?}", texts(&reach));
    let reach = self::reach("inoremap x <C-o>:!ls<CR>");
    assert!(has(&reach, "!ls"));
}

#[test]
fn a_command_line_mapping_types_into_what_the_user_typed() {
    let reach = reach("cnoremap x !ls");
    assert!(has(&reach, "open: !ls"));
    assert!(has(&reach, "open: '!ls"), "{:?}", texts(&reach));
    let reach = self::reach("cnoremap x <Home>so <End><CR>");
    assert!(has(&reach, "so set"), "{:?}", texts(&reach));
    // Pressed twice, it types into what it typed the first time.
    let reach = self::reach("cnoremap x let mapleader=','<Bar>");
    assert!(
        has(&reach, "open: let mapleader=','"),
        "{:?}",
        texts(&reach)
    );
}

#[test]
fn a_command_line_mapping_that_deletes_part_of_the_user_s_text_is_opaque() {
    assert_eq!(reach("cnoremap x <BS>").opaque, vec![Opaque::UserText]);
    assert!(reach("cnoremap x <C-u>!ls<CR>").opaque.is_empty());
    assert!(reach("cnoremap <C-a> <Home>").opaque.is_empty());
}

#[test]
fn an_alt_key_may_be_escape_and_the_key() {
    let reach = reach("nnoremap x <M-:>!ls<CR>");
    assert!(has(&reach, "!ls"), "{:?}", texts(&reach));
}

// ── opaque ────────────────────────────────────────────────────────────

#[test]
fn what_the_analysis_cannot_follow_is_opaque() {
    for (config, kind) in [
        ("nnoremap <expr> x ':!ls<CR>'", Opaque::Expr),
        ("nmap x :echo<CR>", Opaque::Recursive),
        ("nnoremap x @a", Opaque::RegisterExecution),
        ("nnoremap x @:", Opaque::RegisterExecution),
        ("nnoremap x !!ls<CR>", Opaque::FilterOperator),
        ("nnoremap x q:", Opaque::CommandWindow),
        ("nnoremap x :<C-f>", Opaque::CommandWindow),
        ("nnoremap x :e<Tab><CR>", Opaque::CommandLineCompletion),
        ("nnoremap x :<Up><CR>", Opaque::CommandLineHistory),
        ("nnoremap x :<C-r>a<CR>", Opaque::RegisterInsert),
        ("inoremap x <C-r>a", Opaque::RegisterInsert),
        ("nnoremap x :g/a/normal dd<CR>", Opaque::NormalKeys),
        ("nnoremap x :normal dd<CR>", Opaque::NormalKeys),
    ] {
        assert!(reach(config).opaque.contains(&kind), "{config}");
    }
    assert_eq!(reach("nnoremap <expr> x ':!ls<CR>'").commands, []);
}

#[test]
fn a_right_hand_side_too_long_to_follow_is_opaque() {
    let config = format!("nnoremap x <Cmd>!ls{}<Esc>", "<Left>".repeat(3000));
    assert_eq!(reach(&config).opaque, vec![Opaque::TooComplex]);
}

/// A command nests as deep in the analysis as when it runs: past
/// [`MAX_NESTING`] levels the engine gives E169 and runs nothing of it.
#[test]
fn the_analysis_nests_as_deep_as_the_engine() {
    let deepest = usize::from(MAX_NESTING);
    for (unit, deepest) in [
        ("g/a/", deepest),
        ("windo ", deepest),
        ("sx/a/", deepest),
        ("windo g/a/", deepest / 2),
    ] {
        let writes = |n: usize| {
            reach(&format!("nnoremap x :{}w f<CR>", unit.repeat(n)))
                .reach()
                .contains(Reach::FILE_WRITE)
        };
        assert!(writes(deepest), "{unit}");
        assert!(!writes(deepest + 1), "{unit}");
    }
}

/// The parser stops at the limit, so a line nested far deeper is followed
/// on a 2 MiB stack, which it overflowed before.
#[test]
fn a_command_nested_past_the_limit_reaches_nothing_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let engine = VimEngine::new();
            for unit in [
                "windo ",
                "bufdo ",
                "tabdo ",
                "sx/a/",
                "sy/a/",
                "g/a/",
                "g/a/windo ",
            ] {
                let rhs = format!("<Cmd>{}w f<CR>", unit.repeat(200));
                let reach = engine.mapping_reach(
                    MapModePrefix::Normal,
                    &rhs,
                    MappingKind::NonRecursive,
                    MappingFlags::default(),
                );
                assert_eq!(reach.opaque, [], "{unit}");
                assert!(!reach.reach().contains(Reach::FILE_WRITE), "{unit}");
            }
        })
        .expect("a thread")
        .join()
        .expect("no panic");
}

#[test]
fn the_commands_of_a_global_are_reached() {
    let reach = reach("nnoremap x :g/a/so f<Bar>!ls<CR>");
    assert!(has(&reach, "!ls"), "{:?}", texts(&reach));
    assert!(has(&reach, "so f"));
}

#[test]
fn action_and_plug_names_are_listed() {
    let reach = reach("nnoremap x <Action>(save)<Plug>(go)<Action>(save)");
    assert_eq!(&*reach.actions, [Box::from("save")]);
    assert_eq!(&*reach.plugs, [Box::from("go")]);
}

// ── leader and langmap ────────────────────────────────────────────────

/// Vim replaces `<Leader>` in the right-hand side too (map.c `do_map()`
/// calls term.c `replace_termcodes()` on it); vim-core keeps it as a key
/// there, which runs nothing, and the analysis reads it as vim-core does.
#[test]
fn a_leader_in_the_right_hand_side_is_read_as_the_mapping_stores_it() {
    let reach = reach("let mapleader = \":\"\nnnoremap x <Leader>so f<CR>");
    assert!(
        !reach.reach().contains(Reach::SOURCE),
        "{:?}",
        texts(&reach)
    );
    let mut session = HostSession::new("a\n");
    let _ = session.source_script(
        ConfigText::new(
            "t.vim",
            "let mapleader = \":\"\nnnoremap x <Leader>so f<CR>",
        ),
        &mut Maps::default(),
    );
    seen::start();
    let _ = session.process_key_host(KeyEvent::char('x'));
    assert_eq!(seen::take(), Vec::<String>::new());
}

/// With 'langremap' on, the keys a mapping gives go through 'langmap'
/// (macros.h `LANGMAP_ADJUST()`, as normal.c `normal_cmd()` uses it).
#[test]
fn langmap_applies_to_the_keys_with_langremap_on() {
    let on = reach("set langmap=j: langremap\nnnoremap x 0js/a/b/<CR>");
    assert!(has(&on, "s/a/b/"), "{:?}", texts(&on));
    let off = reach("set langmap=j: nolangremap\nnnoremap x 0js/a/b/<CR>");
    assert!(!has(&off, "s/a/b/"), "{:?}", texts(&off));
}

// ── the analysis against the engine ───────────────────────────────────

/// What the user may type into a line a mapping leaves open, as the
/// analysis expects it.
const TYPED: [&str; 4] = ["", " ", "|", "1"];

/// A command as split, without the blanks and colons around it.
fn normal_form(piece: &str) -> &str {
    piece
        .trim_matches([' ', '\t'])
        .trim_start_matches([':', ' ', '\t'])
}

/// Whether a host request comes only from an Ex command.
const fn from_ex(request: &HostRequest) -> bool {
    matches!(
        request,
        HostRequest::ExternalCommand { .. }
            | HostRequest::ReadConfigFile { .. }
            | HostRequest::CustomExCommand { .. }
            | HostRequest::MkVimrc { .. }
            | HostRequest::ExecuteNorm { .. }
            | HostRequest::ReadFile { .. }
            | HostRequest::WriteFile { .. }
            | HostRequest::EditFile { .. }
    )
}

/// One run of keys against what was predicted.
struct Press<'a> {
    config: &'a str,
    text: &'a str,
    reach: &'a MappingReach,
    /// The user types into the line before the mapping runs, so a command
    /// may hold text the analysis cannot know.
    user_text: bool,
}

impl Press<'_> {
    /// A session that has sourced the config.
    fn session(&self) -> HostSession {
        let mut session = HostSession::new(self.text);
        let _ = session.source_script(
            ConfigText::new("corpus.vim", self.config),
            &mut Maps::default(),
        );
        session
    }

    /// Press `setup`, then `keys`, then `typed` and Enter when a line or
    /// `<Cmd>` is open; return what ran that was not predicted.
    fn run(&self, setup: &[KeyEvent], keys: &[KeyEvent], typed: &str) -> Vec<String> {
        let mut session = self.session();
        for &key in setup {
            let _ = session.process_key_host(key);
        }
        seen::start();
        let mut requests = Vec::new();
        for &key in keys {
            requests.extend(session.process_key_host(key).host_requests);
        }
        let open =
            session.engine().mode() == Mode::CommandLine || session.engine().cmd_buffer.is_some();
        if open {
            for c in typed.chars() {
                requests.extend(session.process_key_host(KeyEvent::char(c)).host_requests);
            }
            requests.extend(session.process_key_host(KeyEvent::enter()).host_requests);
        }
        let ran = seen::take();
        self.unpredicted(&ran, &requests)
    }

    fn unpredicted(&self, ran: &[String], requests: &[HostRequest]) -> Vec<String> {
        let reach = self.reach;
        if !reach.opaque.is_empty() || !reach.plugs.is_empty() {
            return Vec::new();
        }
        let predicted: HashSet<&str> = reach
            .commands
            .iter()
            .map(|command| normal_form(&command.text))
            .collect();
        let mut missed = Vec::new();
        for line in ran {
            for piece in split_ex_pipeline(line) {
                let piece = normal_form(&piece);
                if predicted.contains(piece) || self.user_text && self.within_reach(piece) {
                    continue;
                }
                missed.push(format!("ran {piece:?} of {line:?}"));
            }
        }
        for request in requests {
            let explained = match request {
                HostRequest::RunAction { name, .. } => {
                    reach.actions.iter().any(|known| **known == **name)
                }
                request if from_ex(request) => reach.reach().contains(Reach::HOST),
                _ => true,
            };
            if !explained {
                missed.push(format!("asked {:?}", request.kind()));
            }
        }
        missed
    }

    /// Whether `piece`, which holds text the user typed, reaches no more
    /// than the predicted commands together.
    fn within_reach(&self, piece: &str) -> bool {
        let reach = crate::grammar::ex_parser::parse_ex_piece(piece)
            .1
            .map_or(Reach::empty(), |command: ExCommand| command.reach());
        self.reach.reach().contains(reach)
    }
}

/// The keys that put the engine in a mode a mapping of `modes` runs in.
fn setups(modes: MapModePrefix) -> Vec<&'static str> {
    match modes {
        MapModePrefix::All | MapModePrefix::Normal => vec![""],
        MapModePrefix::Visual | MapModePrefix::VisualOnly => vec!["v"],
        MapModePrefix::SelectOnly => vec!["gh"],
        MapModePrefix::Operator => vec!["d"],
        MapModePrefix::Insert => vec!["i"],
        MapModePrefix::Command => vec![":"],
        MapModePrefix::InsertCommand => vec!["i", ":"],
    }
}

fn keys(notation: &str) -> Vec<KeyEvent> {
    crate::execution::key_notation::parse_key_notation_sequence(notation, None)
}

/// Define each mapping of `config` on a document, press its keys and
/// return each command line or host request the engine ran that the
/// analysis did not predict.
fn unpredicted(config: &str, text: &str) -> Vec<String> {
    let mut engine = VimEngine::new();
    let mut maps = Maps::default();
    let _ = engine.source_script(ConfigText::new("corpus.vim", config), &mut maps);
    let mut missed = Vec::new();
    for (modes, lhs, reach) in &maps.0 {
        let (mut plug, mut action) = engine.keymap.registries();
        let lhs_keys = parse_key_notation_with(lhs, &mut plug, &mut action);
        let lhs_keys = engine.keymap.resolved_keys(&lhs_keys).to_vec();
        if lhs_keys.is_empty() {
            continue;
        }
        for setup in setups(*modes) {
            // Keys that do not map are typed into the line.
            let user_text = setup.contains(':');
            let run = Press {
                config,
                text,
                reach,
                user_text,
            };
            for typed in TYPED {
                missed.extend(
                    run.run(&keys(setup), &lhs_keys, typed)
                        .into_iter()
                        .map(|miss| format!("{lhs} after {setup:?} then {typed:?}: {miss}")),
                );
            }
        }
    }
    missed
}

/// Blocks of the corpus: the config and the document.
fn corpus() -> Vec<(String, String)> {
    let file = include_str!("../../../tests/fixtures/mapping_reach_corpus.vim");
    file.split("\"---\n")
        .filter_map(|block| {
            let mut config = String::new();
            let mut text = "a: b/c?d\nfoo:bar\n\nx\n".to_owned();
            for line in block.lines() {
                if let Some(doc) = line.strip_prefix("\" text: ") {
                    text = doc.replace("\\n", "\n");
                } else if !line.starts_with('"') || line.starts_with("\"\\") {
                    config.push_str(line);
                    config.push('\n');
                }
            }
            (!config.trim().is_empty()).then_some((config, text))
        })
        .collect()
}

/// Generated vectors: command lines nested and long, and right-hand sides
/// of many keys.
fn generated() -> Vec<String> {
    let mut configs = Vec::new();
    for unit in [
        "g/x/",
        "g//",
        "g!/x/",
        "v/x/",
        "sx/x/",
        "sy/x/",
        "windo ",
        "bufdo ",
        "tabdo ",
        "silent windo g/x/",
        "1g/x/",
        "'ag/x/",
    ] {
        let commands = format!("{}d", unit.repeat(9));
        configs.push(format!("nnoremap x :{commands}<CR>"));
        configs.push(format!("nnoremap x <Cmd>{commands}<CR>"));
        configs.push(format!("nnoremap x :{commands}<C-u>noh<CR>"));
        configs.push(format!("nnoremap x :{commands}<Esc>"));
    }
    // Around the nesting limit, past which the engine gives E169.
    let deepest = usize::from(MAX_NESTING);
    for (unit, levels) in [
        ("g/x/", deepest),
        ("windo ", deepest),
        ("windo g/x/", deepest / 2),
    ] {
        for n in levels - 1..=levels + 1 {
            configs.push(format!("nnoremap x :{}w f<CR>", unit.repeat(n)));
        }
    }
    for (start, end) in [
        (":", "<CR>"),
        (":", "<Esc>"),
        (":", "<C-u>noh<CR>"),
        ("<Cmd>", "<Esc>"),
    ] {
        configs.push(format!(
            "nnoremap x {start}{}s/{}{}a{}{}/b/{end}",
            "windo ".repeat(4),
            "\\%(".repeat(2),
            "\\%[".repeat(2),
            "]".repeat(2),
            "\\)".repeat(2),
        ));
        configs.push(format!("nnoremap x {start}{:1025}{end}", "noh"));
    }
    let groups = format!("{}a{}", "\\%(".repeat(9), "\\)".repeat(9));
    for rhs in [
        format!("/{groups}<CR>"),
        format!("?{groups}<CR>"),
        format!("d/{groups}<CR>"),
        format!("i<C-o>/{groups}<CR>"),
        format!(":s/{groups}/y/<CR>"),
        format!(":g/{groups}/d<CR>"),
        format!("<Cmd>s/{groups}/y/<CR>"),
    ] {
        configs.push(format!("nnoremap x {rhs}"));
    }
    configs.push(format!("nnoremap x {}", "<".repeat(3000)));
    configs.push(format!(
        "nnoremap x <Cmd>!echo hi{}<Esc>",
        "<Left>".repeat(3000)
    ));
    for (map, rhs) in [
        (
            "nnoremap x",
            format!(
                "{}{}{}",
                ":".repeat(16),
                "1,".repeat(50),
                "<Cmd><CR>".repeat(50)
            ),
        ),
        (
            "nnoremap x",
            format!(
                "{}{}{}",
                ":".repeat(16),
                "1,".repeat(50),
                "<Left><Cmd><CR>".repeat(50)
            ),
        ),
        (
            "nnoremap x",
            format!(
                "{}{}{}",
                ":".repeat(16),
                "1,".repeat(50),
                "<BS>1<Cmd><CR>".repeat(50)
            ),
        ),
        (
            "nnoremap x",
            format!("{}{}", ":".repeat(16), "a ".repeat(50)),
        ),
        (
            "cnoremap x",
            format!("{}{}", ":".repeat(8), "1,".repeat(50)),
        ),
        ("nnoremap x", format!(":{}<CR>", "1,".repeat(50))),
        (
            "nnoremap x",
            format!(
                ":{}tabdo gl! so<Cmd><CR><Left><Cmd><CR>",
                "tabdo ".repeat(7)
            ),
        ),
    ] {
        configs.push(format!("{map} {rhs}"));
    }
    configs
}

/// Every command line a mapping of the corpus runs when pressed, and every
/// host request it makes, is one the analysis predicted, unless it reported
/// something it cannot follow.
#[test]
fn the_analysis_predicts_what_the_engine_runs() {
    let mut missed = Vec::new();
    let mut checked = 0;
    let generated = generated();
    let corpus = corpus().into_iter().chain(
        generated
            .into_iter()
            .map(|config| (config, "a: b\nx\n".to_owned())),
    );
    for (config, text) in corpus {
        checked += 1;
        for miss in unpredicted(&config, &text) {
            missed.push(format!("{}: {miss}", config.trim_end()));
        }
    }
    assert!(checked > 1600, "{checked}");
    assert!(
        missed.is_empty(),
        "{} unpredicted:\n{}",
        missed.len(),
        missed.join("\n")
    );
}

// ── what the analysis follows ─────────────────────────────────────────

/// The places in the grammar that open an Ex command line. A change here
/// is a way into a command line that `mapping_reach` may not know: read it
/// there, then update the counts.
#[test]
fn the_grammar_opens_an_ex_command_line_where_mapping_reach_expects() {
    let handlers = [
        include_str!("../../grammar/handlers/awaiting.rs"),
        include_str!("../../grammar/handlers/helpers.rs"),
        include_str!("../../grammar/handlers/insert.rs"),
        include_str!("../../grammar/handlers/mod.rs"),
        include_str!("../../grammar/handlers/operator.rs"),
        include_str!("../../grammar/handlers/prefix.rs"),
        include_str!("../../grammar/handlers/ready.rs"),
        include_str!("../../grammar/handlers/surround.rs"),
        include_str!("../../grammar/handlers/visual.rs"),
    ];
    let count = |needle: &str| -> usize {
        handlers
            .iter()
            .map(|source| {
                let code = source.split("#[cfg(test)]").next().unwrap_or(source);
                code.matches(needle).count()
            })
            .sum()
    };
    assert_eq!(
        (count("Mode::CommandLine"), count("CommandLinePrompt::Ex")),
        (5, 2),
        "the grammar opens a command line in a new place: teach mapping_reach"
    );
}

proptest! {
    /// The keys the analysis reads are the keys the mapping stores.
    #[test]
    fn the_keys_analysed_are_the_keys_stored(
        rhs in r"([a-z:!<>()\x16 ]|<CR>|<C-u>|<Leader>|<Action>|<Plug>|<Cmd>|\(x\)){1,12}"
    ) {
        let line = format!("nnoremap x {rhs}");
        let Ok(ExCommand::Map { rhs: Some(raw), .. }) = crate::grammar::parse_ex_command(&line) else {
            return Ok(());
        };
        let mut engine = VimEngine::new();
        let analysed = engine.mapping_rhs_keys(&raw).0;
        let _ = engine.source_script(ConfigText::new("t.vim", &line), &mut Maps::default());
        let stored = engine
            .keymap()
            .get_user_mapping(KeyEvent::char('x'), Mode::Normal)
            .map(|entry| entry.sequence().to_vec());
        prop_assert_eq!(Some(analysed), stored);
    }
}
