//! What the keys of a mapping can reach when it runs, for a host that
//! judges a mapping before it does.
//!
//! The keys are followed as `process()` reads them, on scratch state: a
//! command line through the command-line handler and
//! [`CommandLineState::apply_edit`], a `<Cmd>` buffer as the engine fills
//! it. Normal, Visual and Operator-pending mode are one worst-case phase,
//! where every `:` opens a command line and the keys after it are read
//! both ways. Where the keys can go more than one way, each way is a
//! branch.

use std::collections::{HashMap, HashSet};

use super::VimEngine;
use crate::execution::config::{MappingReach, Opaque, ReachedCommand};
use crate::grammar::ex_parser::{nest, parse_ex_piece_at};
use crate::grammar::split_ex_pipeline;
use crate::grammar::types::{ExCommand, MapModePrefix, Reach};
use crate::keymap::{
    Key, KeyEvent, LangmapTable, MappingFlags, MappingKind, MappingMode, Modifiers, NameRegistry,
};
use crate::mode::{command_line_step, CommandLineAction, CommandLineStep};
use crate::primitives::CommandLineEdit;
use crate::state::{CommandLinePrompt, CommandLineState};

/// The most keys followed. vim-core keeps 10,000 keys of typeahead and
/// drops the rest, so a longer right-hand side may not run to its end.
const MAX_KEYS: usize = 2048;

/// The most branches followed at once.
const MAX_BRANCHES: usize = 64;

/// The work a right-hand side may take, for each of its bytes, and at
/// least: a unit for each byte of a line a key edits, and for each byte of
/// a line parsed. It keeps the analysis linear in the right-hand side.
const WORK_PER_BYTE: usize = 64;
const MIN_WORK: usize = 1 << 16;

/// The range the engine puts in a command line opened in Visual mode.
const VISUAL_RANGE: &str = "'<,'>";

/// Stands for what the user typed in a command line before a
/// command-line mapping runs, which is not known.
const USER_TEXT: char = '\u{e000}';

/// What the user may have typed before a command-line mapping runs:
/// nothing, a range or a mark its keys extend, or a `:set ` or `:let `
/// they complete.
const TYPED_BEFORE: [&str; 5] = ["", "1", "'", "set ", "let "];

/// What the user may type into a line a mapping leaves open, at its
/// cursor or its end, before Enter: a blank, which can end a command
/// name, and a `|` or a digit, which can end a command or start one.
const TYPED_INTO: [char; 3] = [' ', '|', '1'];

impl VimEngine {
    /// What the keys of a mapping can reach when it runs: the command
    /// lines and `<Cmd>` commands they can run, what they do that cannot
    /// be followed, and the `<Action>` and `<Plug>` names they hold.
    ///
    /// `rhs` is read as a mapping of `modes` would store it, with the
    /// leader and the 'langmap' in force now. The source of a config hands
    /// this to [`ConfigHost::judge`](crate::execution::ConfigHost::judge)
    /// with each mapping it defines.
    #[must_use]
    pub fn mapping_reach(
        &self,
        modes: MapModePrefix,
        rhs: &str,
        kind: MappingKind,
        flags: MappingFlags,
    ) -> MappingReach {
        let mut analysis = Analysis::new(self, rhs.len());
        if flags.expr {
            analysis.opaque(Opaque::Expr);
            return analysis.reach;
        }
        if kind == MappingKind::Recursive {
            analysis.opaque(Opaque::Recursive);
        }
        let (keys, plug, action) = self.mapping_rhs_keys(rhs);
        for key in &keys {
            analysis.name(*key, &plug, &action);
        }
        if keys.len() > MAX_KEYS || keys.contains(&KeyEvent::char(USER_TEXT)) {
            analysis.opaque(Opaque::TooComplex);
            return analysis.reach;
        }
        let (normal, insert, command_line) = start_phases(modes);
        let mut start = Vec::new();
        if normal {
            start.push(Branch::new(Phase::Normal { after_q: false }));
        }
        if insert {
            start.push(Branch::new(Phase::Insert));
        }
        let ended = analysis.run(&keys, start);
        analysis.leave_open(&ended);
        if command_line {
            let mut line = CommandLineState::new();
            line.begin(CommandLinePrompt::Ex);
            line.insert_char(USER_TEXT);
            let ended = analysis.run(
                &keys,
                vec![Branch::new(Phase::Line(Box::new(Line::new(line))))],
            );
            analysis.leave_open(&ended);
            // Pressed again while the line is open, the mapping types into
            // what it typed the first time.
            let open: Vec<Branch> = ended
                .into_iter()
                .filter(|branch| matches!(branch.phase, Phase::Line(_)))
                .collect();
            let ended = analysis.run(&keys, open);
            analysis.leave_open(&ended);
        }
        analysis.reach
    }

    /// The keys a mapping stores for the right-hand side `rhs`, with the
    /// `<Plug>` and `<Action>` names as the keymap would register them,
    /// without registering them.
    fn mapping_rhs_keys(&self, rhs: &str) -> (Vec<KeyEvent>, NameRegistry, NameRegistry) {
        let (mut plug, mut action) = self.keymap.registries();
        let keys =
            crate::execution::key_notation::parse_key_notation_with(rhs, &mut plug, &mut action);
        (keys, plug, action)
    }
}

/// Whether a mapping of `modes` starts in the worst-case phase, in Insert
/// mode and in a command line.
const fn start_phases(modes: MapModePrefix) -> (bool, bool, bool) {
    match modes {
        MapModePrefix::All
        | MapModePrefix::Normal
        | MapModePrefix::Visual
        | MapModePrefix::Operator
        | MapModePrefix::VisualOnly
        | MapModePrefix::SelectOnly => (true, false, false),
        MapModePrefix::Insert => (false, true, false),
        MapModePrefix::Command => (false, false, true),
        MapModePrefix::InsertCommand => (false, true, true),
    }
}

/// Where the keys are.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Normal, Visual or Operator-pending mode, read as the worst case.
    /// `after_q` when the key before may have been `q`.
    Normal { after_q: bool },
    /// Insert mode.
    Insert,
    /// An open command line.
    Line(Box<Line>),
}

/// A command line on scratch state.
#[derive(Debug, Clone)]
struct Line {
    state: CommandLineState,
    /// The key before was `<C-\>`.
    after_ctrl_backslash: bool,
    /// The key before was `<C-r>`: this key names a register.
    awaiting_register: bool,
}

impl Line {
    const fn new(state: CommandLineState) -> Self {
        Self {
            state,
            after_ctrl_backslash: false,
            awaiting_register: false,
        }
    }

    /// A line opened by `:`, after `prefix`.
    fn opened(prefix: &str) -> Self {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::Ex);
        state.insert_str(prefix);
        Self::new(state)
    }
}

impl PartialEq for Line {
    fn eq(&self, other: &Self) -> bool {
        self.state.input() == other.state.input()
            && self.state.cursor() == other.state.cursor()
            && self.after_ctrl_backslash == other.after_ctrl_backslash
            && self.awaiting_register == other.awaiting_register
    }
}

impl Eq for Line {}

/// One way the keys can go: the phase and an open `<Cmd>` buffer, which
/// takes the keys before any mode does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Branch {
    phase: Phase,
    cmd: Option<String>,
}

impl Branch {
    const fn new(phase: Phase) -> Self {
        Self { phase, cmd: None }
    }
}

struct Analysis<'e> {
    engine: &'e VimEngine,
    /// The 'langmap' a key a mapping gives goes through, with 'langremap'
    /// on (`LANGMAP_ADJUST()`).
    langmap: Option<LangmapTable>,
    reach: MappingReach,
    /// The lines already parsed, and whether as open.
    parsed: HashSet<(String, bool)>,
    /// Where each command noted is in `reach.commands`, by its text and
    /// whether open, with the least depth it was parsed at.
    noted: HashMap<(Box<str>, bool), (usize, u8)>,
    work: usize,
    stopped: bool,
}

impl<'e> Analysis<'e> {
    fn new(engine: &'e VimEngine, rhs_len: usize) -> Self {
        let options = &engine.options;
        let langmap = options
            .langremap()
            .then(|| LangmapTable::parse(options.langmap()).ok())
            .flatten()
            .filter(|table| !table.is_empty());
        Self {
            engine,
            langmap,
            reach: MappingReach::default(),
            parsed: HashSet::new(),
            noted: HashMap::new(),
            work: rhs_len.saturating_mul(WORK_PER_BYTE).max(MIN_WORK),
            stopped: false,
        }
    }

    fn opaque(&mut self, kind: Opaque) {
        if !self.reach.opaque.contains(&kind) {
            self.reach.opaque.push(kind);
        }
    }

    /// Spend `work`; past the budget the analysis stops.
    fn spend(&mut self, work: usize) {
        match self.work.checked_sub(work) {
            Some(left) => self.work = left,
            None => self.stop(),
        }
    }

    fn stop(&mut self) {
        self.stopped = true;
        self.opaque(Opaque::TooComplex);
    }

    /// Note the name of an `<Action>` or `<Plug>` key.
    fn name(&mut self, key: KeyEvent, plug: &NameRegistry, action: &NameRegistry) {
        let (names, registry, id) = match key.key() {
            Key::Action(id) => (&mut self.reach.actions, action, id),
            Key::Plug(id) => (&mut self.reach.plugs, plug, id),
            _ => return,
        };
        if let Some(name) = registry.get_name(id) {
            if !names.iter().any(|known| &**known == name) {
                names.push(name.into());
            }
        }
    }

    /// Follow `keys` from each of `start`, and return where they end.
    fn run(&mut self, keys: &[KeyEvent], start: Vec<Branch>) -> Vec<Branch> {
        let mut branches = start;
        for &key in keys {
            let mut next = Vec::new();
            for branch in branches {
                self.key(branch, key, &mut next);
            }
            branches = Vec::new();
            for branch in next {
                if !branches.contains(&branch) {
                    branches.push(branch);
                }
            }
            if branches.len() > MAX_BRANCHES {
                self.stop();
            }
            if self.stopped {
                return Vec::new();
            }
        }
        branches
    }

    /// `key` from `branch`. With 'altsendsesc' on, the engine reads an Alt
    /// key no mapping takes as `<Esc>` and the key without Alt; the option
    /// can change after the mapping is defined, so both are followed.
    fn key(&mut self, branch: Branch, key: KeyEvent, out: &mut Vec<Branch>) {
        if !key.modifiers().contains(Modifiers::ALT) {
            return self.plain_key(branch, key, out);
        }
        let base = KeyEvent::new(key.key(), key.modifiers() - Modifiers::ALT);
        let mut escaped = Vec::new();
        self.plain_key(branch.clone(), KeyEvent::escape(), &mut escaped);
        for escaped in escaped {
            self.plain_key(escaped, base, out);
        }
        self.plain_key(branch, key, out);
    }

    /// `key` from `branch`, in the order `process()` reads it.
    fn plain_key(&mut self, mut branch: Branch, key: KeyEvent, out: &mut Vec<Branch>) {
        // A key the host handles in the mode is not processed, and the
        // host stops the mapping there, so what is open stays open.
        if self.host_handles(&branch, key) {
            self.leave_open(std::slice::from_ref(&branch));
            out.push(branch.clone());
        }
        match key.key() {
            Key::Action(_) => return out.push(branch),
            Key::Cmd => {
                branch.cmd = Some(String::new());
                return out.push(branch);
            }
            _ => {}
        }
        if let Some(cmd) = &mut branch.cmd {
            match key.key() {
                Key::Enter => {
                    let text = std::mem::take(cmd);
                    branch.cmd = None;
                    self.reached(&text, false);
                    // The command may fail, which drops the keys after it,
                    // so a line open now may stay open.
                    self.leave_open(std::slice::from_ref(&branch));
                }
                Key::Escape => branch.cmd = None,
                Key::Char(c) => {
                    cmd.push(c);
                    self.spend(1);
                }
                _ => {}
            }
            return out.push(branch);
        }
        match branch.phase {
            Phase::Line(line) => self.line_key(line, key, out),
            Phase::Insert => self.insert_key(key, out),
            Phase::Normal { after_q } => self.normal_key(after_q, key, out),
        }
    }

    /// Whether the host handles `key` in a mode `branch` can be in, where
    /// that leaves a command line or `<Cmd>` buffer open.
    fn host_handles(&self, branch: &Branch, key: KeyEvent) -> bool {
        let handlers = &self.engine.handler_map;
        let modes: &[MappingMode] = match branch.phase {
            Phase::Line(_) => &[MappingMode::Command],
            Phase::Insert if branch.cmd.is_some() => &[MappingMode::Insert],
            Phase::Normal { .. } if branch.cmd.is_some() => &[
                MappingMode::Normal,
                MappingMode::Visual,
                MappingMode::Operator,
                MappingMode::VisualOnly,
                MappingMode::SelectOnly,
            ],
            Phase::Insert | Phase::Normal { .. } => &[],
        };
        modes
            .iter()
            .any(|&mode| handlers.is_host_handled(key, mode))
    }

    /// `key` in a command line, as the command-line handler and the
    /// engine's `execute_command_line_result()` take it.
    fn line_key(&mut self, mut line: Box<Line>, key: KeyEvent, out: &mut Vec<Branch>) {
        if std::mem::take(&mut line.awaiting_register) {
            return out.push(Branch::new(Phase::Line(line)));
        }
        let after = std::mem::take(&mut line.after_ctrl_backslash);
        let action = match command_line_step(key, after) {
            CommandLineStep::CtrlBackslash => {
                line.after_ctrl_backslash = true;
                return out.push(Branch::new(Phase::Line(line)));
            }
            CommandLineStep::ToNormal => return out.push(normal()),
            CommandLineStep::Key(action) => action,
        };
        match action {
            CommandLineAction::Cancel => return out.push(normal()),
            CommandLineAction::Commit => {
                let text = line.state.input().to_owned();
                self.reached_ex_line(&text, false);
                return out.push(normal());
            }
            CommandLineAction::AwaitRegister => {
                self.opaque(Opaque::RegisterInsert);
                line.awaiting_register = true;
            }
            CommandLineAction::OpenCommandWindow => {
                self.opaque(Opaque::CommandWindow);
                return out.push(normal());
            }
            CommandLineAction::Edit(
                CommandLineEdit::CompleteNext
                | CommandLineEdit::CompletePrev
                | CommandLineEdit::ListCompletions,
            ) => self.opaque(Opaque::CommandLineCompletion),
            CommandLineAction::Edit(
                CommandLineEdit::HistoryPrev | CommandLineEdit::HistoryNext,
            ) => {
                self.opaque(Opaque::CommandLineHistory);
            }
            // Backspace in an empty line cancels it.
            CommandLineAction::Edit(CommandLineEdit::Backspace) if line.state.is_empty() => {
                return out.push(normal());
            }
            CommandLineAction::Edit(edit) => {
                let user_text = line.state.input().contains(USER_TEXT);
                line.state.apply_edit(edit);
                self.spend(line.state.input().len() + 1);
                // Deleting all of what the user typed leaves a known line;
                // deleting part of it leaves one that is not.
                let whole = matches!(
                    edit,
                    CommandLineEdit::DeleteToStart | CommandLineEdit::DeleteToEnd
                );
                if user_text && !whole && !line.state.input().contains(USER_TEXT) {
                    self.opaque(Opaque::UserText);
                }
            }
            CommandLineAction::Ignore => {}
        }
        out.push(Branch::new(Phase::Line(line)));
    }

    /// `key` in Insert mode. A key known to stay in Insert mode is text or
    /// an edit; one that leaves it goes to the worst-case phase; any other
    /// may do either.
    fn insert_key(&mut self, key: KeyEvent, out: &mut Vec<Branch>) {
        let ctrl = key.modifiers().contains(Modifiers::CTRL);
        let plain = !key
            .modifiers()
            .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::META);
        let leaves =
            key.key() == Key::Escape || key == KeyEvent::ctrl('c') || key == KeyEvent::ctrl('[');
        if leaves {
            return out.push(normal());
        }
        if key == KeyEvent::ctrl('r') {
            self.opaque(Opaque::RegisterInsert);
            return out.push(Branch::new(Phase::Insert));
        }
        let stays = match key.key() {
            Key::Char(_) => plain,
            Key::Enter | Key::Tab | Key::Backspace | Key::Delete | Key::Insert => plain,
            Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown => true,
            _ => false,
        } || (ctrl
            && matches!(
                key.key(),
                Key::Char(
                    'h' | 'j'
                        | 'm'
                        | 'i'
                        | 't'
                        | 'd'
                        | 'w'
                        | 'u'
                        | 'a'
                        | 'e'
                        | 'y'
                        | 'n'
                        | 'p'
                        | 'v'
                        | 'k'
                        | 'g'
                        | 'x'
                        | '^'
                        | '6'
                )
            ));
        out.push(Branch::new(Phase::Insert));
        if !stays {
            out.push(normal());
        }
    }

    /// `key` in the worst-case phase. Every `:` opens a command line, as
    /// from Normal and from Visual mode, and the phase goes on too, since
    /// the `:` may be the argument of a key before it (`f:`, `t:`, `m:`, a
    /// text object, a register name).
    fn normal_key(&mut self, after_q: bool, key: KeyEvent, out: &mut Vec<Branch>) {
        let read = |key: KeyEvent| match key.key() {
            Key::Char(c)
                if !key
                    .modifiers()
                    .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::META) =>
            {
                Some(c)
            }
            _ => None,
        };
        let mapped = self
            .langmap
            .as_ref()
            .map(|table| table.remap_key_event(key));
        let chars: Vec<char> = std::iter::once(key)
            .chain(mapped)
            .filter_map(read)
            .collect();
        let is = |wanted: &[char]| chars.iter().any(|c| wanted.contains(c));
        if after_q && is(&[':', '/', '?']) {
            self.opaque(Opaque::CommandWindow);
        }
        if is(&['@']) {
            self.opaque(Opaque::RegisterExecution);
        }
        if is(&['!']) {
            self.opaque(Opaque::FilterOperator);
        }
        if is(&[':']) {
            out.push(Branch::new(Phase::Line(Box::new(Line::opened("")))));
            out.push(Branch::new(Phase::Line(Box::new(Line::opened(
                VISUAL_RANGE,
            )))));
        }
        out.push(Branch::new(Phase::Normal {
            after_q: is(&['q']),
        }));
    }

    /// Note what is open in each of `branches` as a line the user may run:
    /// as it stands, and with each of [`TYPED_INTO`] typed into it.
    fn leave_open(&mut self, branches: &[Branch]) {
        for branch in branches {
            if let Some(cmd) = &branch.cmd {
                self.reached(cmd, true);
                for c in TYPED_INTO {
                    self.reached(&format!("{cmd}{c}"), true);
                }
            }
            if let Phase::Line(line) = &branch.phase {
                let text = line.state.input();
                self.reached_ex_line(text, true);
                let cursor = line.state.cursor();
                for at in [cursor, text.len()] {
                    for c in TYPED_INTO {
                        let mut typed = text.to_owned();
                        typed.insert(at, c);
                        self.reached_ex_line(&typed, true);
                    }
                }
            }
        }
    }

    /// Note a command line that runs. One opened in Visual mode runs with
    /// `'<,'>` put before it when it does not start with that.
    fn reached_ex_line(&mut self, text: &str, open: bool) {
        self.reached(text, open);
        let command = text.trim_start();
        if !command.starts_with(VISUAL_RANGE) {
            self.reached(&format!("{VISUAL_RANGE}{command}"), open);
        }
    }

    /// Note the line `text` as one that runs, `open` when the user may
    /// type into it first. A line that holds what the user typed before a
    /// command-line mapping is noted with each of [`TYPED_BEFORE`] there.
    fn reached(&mut self, text: &str, open: bool) {
        if text.contains(USER_TEXT) {
            for typed in TYPED_BEFORE {
                self.reached_line(&text.replace(USER_TEXT, typed), open);
            }
        } else {
            self.reached_line(text, open);
        }
    }

    fn reached_line(&mut self, text: &str, open: bool) {
        if self.stopped || !self.parsed.insert((text.to_owned(), open)) {
            return;
        }
        self.spend(text.len() + 16);
        for piece in split_ex_pipeline(text) {
            self.reached_command(&piece, open, 0);
        }
    }

    /// Note one command of a line, run `depth` levels deep, with the
    /// commands it runs. A command noted deeper is noted again, since it
    /// may nest past the limit there and not here.
    fn reached_command(&mut self, piece: &str, open: bool, depth: u8) {
        let text: Box<str> = piece.trim_matches([' ', '\t']).into();
        let key = (text.clone(), open);
        let known = self.noted.get(&key).copied();
        if self.stopped || known.is_some_and(|(_, at)| at <= depth) {
            return;
        }
        let (modifiers, parsed) = parse_ex_piece_at(piece, depth);
        let command = ReachedCommand {
            text,
            open,
            reach: parsed
                .as_ref()
                .map_or(Reach::empty(), |command| command.reach_at(depth)),
            parsed: parsed.map(|command| (modifiers, command)),
        };
        let index = known.map_or(self.reach.commands.len(), |(index, _)| index);
        match self.reach.commands.get_mut(index) {
            Some(slot) => *slot = command,
            None => self.reach.commands.push(command),
        }
        self.noted.insert(key, (index, depth));
        let nested = self
            .reach
            .commands
            .get(index)
            .and_then(|command| command.parsed.as_ref().ok())
            .map(|(_, command)| command.clone());
        if let Some(command) = nested {
            self.nested(&command, open, depth);
        }
    }

    /// The commands `command`, run `depth` levels deep, runs: those of a
    /// `:g` list are noted, and the keys of `:normal` are opaque.
    fn nested(&mut self, command: &ExCommand, open: bool, depth: u8) {
        match command {
            ExCommand::Norm { .. } => self.opaque(Opaque::NormalKeys),
            ExCommand::Global { commands, .. } => {
                // Deeper than the limit, `:g` gives an error before it runs.
                if let Ok(depth) = nest(depth) {
                    self.spend(commands.len() + 16);
                    for piece in split_ex_pipeline(commands) {
                        self.reached_command(&piece, open, depth);
                    }
                }
            }
            ExCommand::StructuralExtract { command, .. }
            | ExCommand::StructuralComplement { command, .. }
            | ExCommand::WinDo { command, .. }
            | ExCommand::BufDo { command, .. }
            | ExCommand::TabDo { command, .. } => {
                self.nested(command, open, depth.saturating_add(1));
            }
            _ => {}
        }
    }
}

/// The worst-case phase, after a command line closes or Insert mode ends.
const fn normal() -> Branch {
    Branch::new(Phase::Normal { after_q: false })
}

/// The command lines and `<Cmd>` commands the engine runs on this thread,
/// for tests that compare them with the analysis.
#[cfg(test)]
pub(crate) mod seen {
    use std::cell::RefCell;

    thread_local! {
        static LINES: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    }

    /// Start recording.
    pub(crate) fn start() {
        LINES.with(|lines| *lines.borrow_mut() = Some(Vec::new()));
    }

    pub(crate) fn ran(line: &str) {
        LINES.with(|lines| {
            if let Some(lines) = lines.borrow_mut().as_mut() {
                lines.push(line.to_owned());
            }
        });
    }

    /// What ran since the last call, recording on.
    pub(crate) fn take() -> Vec<String> {
        LINES.with(|lines| lines.borrow_mut().replace(Vec::new()).unwrap_or_default())
    }
}

#[cfg(test)]
#[path = "mapping_reach_tests.rs"]
mod tests;
