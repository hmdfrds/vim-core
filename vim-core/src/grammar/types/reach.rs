//! What running an Ex command can touch, for a host that judges commands.

use bitflags::bitflags;

use super::ex_command::{ExCommand, SetAssignment};
use crate::grammar::ex_parser::{nest, parse_ex_piece_at};
use crate::grammar::split_ex_pipeline;
use crate::primitives::option_table;

bitflags! {
    /// What running a command can touch or start.
    ///
    /// A command that runs others, such as `:g` or `:windo`, holds what
    /// they reach as well. A host that allows by flag should refuse a flag
    /// it does not know: a later version may add flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[non_exhaustive]
    pub struct Reach: u32 {
        /// Sets or shows options: `:set`, `:setlocal`, `:setglobal`.
        const OPTIONS            = 1 << 0;
        /// Sets an option Vim keeps from a modeline and the sandbox
        /// (`P_SECURE`), such as 'langmap' or 'shell'.
        const SECURE_OPTION      = 1 << 1;
        /// Defines or removes mappings.
        const MAPPING            = 1 << 2;
        /// Defines or removes abbreviations.
        const ABBREVIATION       = 1 << 3;
        /// Sets the leader or the local leader.
        const LEADER             = 1 << 4;
        /// Changes the `:sethandler` table.
        const HANDLER            = 1 << 5;
        /// Shows a message or a listing.
        const MESSAGE            = 1 << 6;
        /// Reads or changes the buffer, its cursor, marks, folds or undo
        /// history.
        const BUFFER             = 1 << 7;
        /// Becomes a host request.
        const HOST               = 1 << 8;
        /// Runs a shell command.
        const SHELL              = 1 << 9;
        /// Reads a file.
        const FILE_READ          = 1 << 10;
        /// Writes a file.
        const FILE_WRITE         = 1 << 11;
        /// Names a file in its argument.
        const NAMED_PATH         = 1 << 12;
        /// Sources a file.
        const SOURCE             = 1 << 13;
        /// Quits.
        const QUIT               = 1 << 14;
        /// Opens, closes or moves between windows, tabs or buffers.
        const WINDOW             = 1 << 15;
        /// Runs a host action: `:action`.
        const ACTION             = 1 << 16;
        /// A command vim-core does not run itself: the host's own, or a
        /// Vim command vim-core lacks.
        const CUSTOM             = 1 << 17;
        /// Runs other commands or keys: `:g`, `:windo`, `:normal`.
        const NESTED             = 1 << 18;
        /// Runs a register's text as commands: `:@a`.
        const REGISTER_EXECUTION = 1 << 19;
        /// Assigns or removes variables: `:let`, `:unlet` in a config.
        const VARIABLE           = 1 << 20;
    }
}

impl ExCommand {
    /// What running this command can touch or start.
    ///
    /// The match has no wildcard, so a new command cannot be added without
    /// saying what it reaches.
    #[must_use]
    pub fn reach(&self) -> Reach {
        self.reach_at(0)
    }

    /// [`Self::reach`] of a command that runs `depth` levels deep.
    pub(crate) fn reach_at(&self, depth: u8) -> Reach {
        const BUFFER: Reach = Reach::BUFFER;
        const HOST_WINDOW: Reach = Reach::HOST.union(Reach::WINDOW);
        match self {
            Self::Substitute { .. }
            | Self::Delete { .. }
            | Self::Yank { .. }
            | Self::Move { .. }
            | Self::Copy { .. }
            | Self::Join { .. }
            | Self::Sort { .. }
            | Self::Mark { .. }
            | Self::Marks { .. }
            | Self::Jumps
            | Self::Changes
            | Self::Put { .. }
            | Self::Retab { .. }
            | Self::Left { .. }
            | Self::Right { .. }
            | Self::Center { .. }
            | Self::Earlier { .. }
            | Self::Later { .. }
            | Self::UndoList
            | Self::UndoTree
            | Self::UndoSequence { .. }
            | Self::PrintLines { .. }
            | Self::ZWindow { .. }
            | Self::GotoLine { .. }
            | Self::SelectMatches { .. }
            | Self::SplitMatches { .. }
            | Self::KeepMatches { .. }
            | Self::RemoveMatches { .. }
            | Self::TrimSelections
            | Self::AlignSelections
            | Self::RotateContents
            | Self::RotateContentsDir { .. }
            | Self::AddNext { .. }
            | Self::AddPrev { .. }
            | Self::SkipMatch
            | Self::AddCursorDir { .. }
            | Self::SelectAll
            | Self::CursorCollapse
            | Self::CursorRemove
            | Self::CursorPrimary { .. }
            | Self::CursorSplitBlock
            | Self::CursorFlip
            | Self::CursorForward
            | Self::CursorMerge
            | Self::RepeatSubstitute { .. }
            | Self::SubTilde { .. }
            | Self::DelMarks { .. }
            | Self::Redo
            | Self::UndoJoin => BUFFER,
            Self::Fold { .. } | Self::FoldOpen { .. } | Self::FoldClose { .. } => {
                BUFFER | Reach::HOST
            }
            // Deeper than the limit, `:g` gives an error before it runs.
            Self::Global { commands, .. } => {
                BUFFER
                    | Reach::NESTED
                    | nest(depth).map_or(Reach::empty(), |depth| line_reach(commands, depth))
            }
            Self::StructuralExtract { command, .. }
            | Self::StructuralComplement { command, .. } => {
                BUFFER | Reach::NESTED | command.reach_at(depth.saturating_add(1))
            }
            Self::Norm { .. } => BUFFER | Reach::NESTED,
            // The register's lines run as commands, which can ask the host.
            Self::ExecuteRegister { .. } => {
                BUFFER | Reach::REGISTER_EXECUTION | Reach::NESTED | Reach::HOST
            }
            Self::Read { path, .. } => {
                let read = if is_shell(path) {
                    Reach::SHELL
                } else {
                    Reach::FILE_READ | Reach::NAMED_PATH
                };
                BUFFER | Reach::HOST | read
            }
            Self::Filter { .. } => BUFFER | Reach::HOST | Reach::SHELL,
            Self::Registers { .. } | Self::Echo { .. } | Self::NoHighlight => Reach::MESSAGE,
            Self::Messages { clear: true } => Reach::MESSAGE,
            Self::Messages { clear: false } | Self::ActionList { .. } => {
                Reach::MESSAGE | Reach::HOST
            }
            Self::Map { rhs, buffer, .. } => {
                let reach = if rhs.is_some() {
                    Reach::MAPPING
                } else {
                    Reach::MESSAGE
                };
                reach | buffer_reach(*buffer)
            }
            Self::Unmap { buffer, .. } | Self::MapClear { buffer, .. } => {
                Reach::MAPPING | buffer_reach(*buffer)
            }
            Self::Abbreviate {
                trigger: Some(_),
                replacement: Some(_),
                ..
            }
            | Self::Unabbreviate { .. }
            | Self::AbClear { .. } => Reach::ABBREVIATION,
            Self::Abbreviate { .. } => Reach::MESSAGE,
            Self::Write { path, .. } | Self::Update { path, .. } => {
                Reach::HOST | written_file(path.as_deref())
            }
            Self::WriteAll | Self::MkVimrc { .. } => Reach::HOST | Reach::FILE_WRITE,
            Self::Quit { .. } | Self::QuitAll { .. } | Self::CQuit { .. } => {
                Reach::HOST | Reach::QUIT
            }
            Self::WriteQuit { .. } | Self::WriteQuitAll => {
                Reach::HOST | Reach::QUIT | Reach::FILE_WRITE
            }
            Self::Edit { .. } => HOST_WINDOW | Reach::FILE_READ | Reach::NAMED_PATH,
            Self::External { .. } => Reach::HOST | Reach::SHELL,
            Self::Split { path } | Self::VSplit { path } | Self::TabNew { path } => {
                HOST_WINDOW | named_file(path.is_some())
            }
            Self::Close { .. }
            | Self::Only { .. }
            | Self::New
            | Self::VNew
            | Self::BufferDelete { .. }
            | Self::BufferWipeout { .. }
            | Self::Buffer { .. }
            | Self::BufferNext { .. }
            | Self::BufferPrev { .. }
            | Self::BufferFirst
            | Self::BufferLast
            | Self::BufferList
            | Self::TabNext { .. }
            | Self::TabPrev { .. }
            | Self::TabClose { .. } => HOST_WINDOW,
            Self::CNext { .. } | Self::CPrev { .. } | Self::CList | Self::CC { .. } => Reach::HOST,
            Self::Custom { .. } => Reach::HOST | Reach::CUSTOM,
            Self::Action { .. } => Reach::HOST | Reach::ACTION,
            Self::Source { path } => {
                Reach::HOST | Reach::SOURCE | Reach::FILE_READ | named_file(path.is_some())
            }
            Self::LetLeader { .. } => Reach::LEADER,
            Self::Set { assignments }
            | Self::SetLocal { assignments }
            | Self::SetGlobal { assignments } => {
                let secure = assignments
                    .iter()
                    .filter_map(SetAssignment::option)
                    .any(|name| option_target(name).secure);
                if secure {
                    Reach::OPTIONS | Reach::SECURE_OPTION
                } else {
                    Reach::OPTIONS
                }
            }
            Self::SetHandler { .. } => Reach::HANDLER,
            Self::WinDo { command, .. }
            | Self::BufDo { command, .. }
            | Self::TabDo { command, .. } => {
                HOST_WINDOW | Reach::NESTED | command.reach_at(depth.saturating_add(1))
            }
            // The jump list belongs to the engine, and a script block or
            // :finish changes nothing a host sees.
            Self::ClearJumps | Self::Block { .. } | Self::Finish => Reach::empty(),
        }
    }
}

/// What every `|` command of `line` reaches, as `:g` parses and runs them
/// `depth` levels deep on each line.
fn line_reach(line: &str, depth: u8) -> Reach {
    split_ex_pipeline(line)
        .iter()
        .filter_map(|piece| parse_ex_piece_at(piece, depth).1.ok())
        .fold(Reach::empty(), |reach, command| {
            reach | command.reach_at(depth)
        })
}

/// `BUFFER` for a `<buffer>` mapping command.
const fn buffer_reach(buffer: bool) -> Reach {
    if buffer {
        Reach::BUFFER
    } else {
        Reach::empty()
    }
}

/// What naming a file in the argument reaches.
const fn named_file(named: bool) -> Reach {
    if named {
        Reach::FILE_READ.union(Reach::NAMED_PATH)
    } else {
        Reach::empty()
    }
}

/// Whether the argument of `:r` or `:w` is a shell command: `:r !cmd`.
fn is_shell(path: &str) -> bool {
    path.trim_start_matches([' ', '\t']).starts_with('!')
}

/// What `:w` or `:update` with the argument `path` reaches besides the
/// host: a shell command for `:w !cmd`, a file otherwise.
fn written_file(path: Option<&str>) -> Reach {
    match path {
        Some(path) if is_shell(path) => Reach::SHELL,
        Some(_) => Reach::FILE_WRITE.union(Reach::NAMED_PATH),
        None => Reach::FILE_WRITE,
    }
}

impl SetAssignment {
    /// The option name as written, with any `no` or `inv` before it.
    pub(crate) fn option(&self) -> Option<&str> {
        match self {
            Self::SetBool(name)
            | Self::UnsetBool(name)
            | Self::ToggleBool(name)
            | Self::Query(name)
            | Self::Assign(name, _)
            | Self::Append(name, _)
            | Self::Remove(name, _)
            | Self::Prepend(name, _) => Some(name),
            Self::InvertBool(name) | Self::Reset { name, .. } | Self::Invalid { name, .. } => {
                Some(name)
            }
            Self::ShowAll => None,
        }
    }
}

/// The option `written` names, as `:set` reads a name with `no` or `inv`
/// before it (option.c `get_option_prefix()`): vim-core's, Vim's that
/// vim-core lacks, or neither.
pub(crate) fn option_target(written: &str) -> option_table::OptionName {
    let named = option_table::option_name(written);
    if named.full.is_some() || written.starts_with("novice") {
        return named;
    }
    written
        .strip_prefix("no")
        .or_else(|| written.strip_prefix("inv"))
        .map(option_table::option_name)
        .filter(|named| named.full.is_some())
        .unwrap_or(named)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::parse_ex_command;

    #[test]
    fn nested_commands_carry_what_they_run() {
        let reach = |line: &str| {
            parse_ex_command(line)
                .map(|c| c.reach())
                .unwrap_or_default()
        };
        assert!(reach("g/a/!ls").contains(Reach::SHELL | Reach::NESTED | Reach::BUFFER));
        assert!(reach("windo set lmap=x").contains(Reach::SECURE_OPTION | Reach::NESTED));
        assert!(reach("sx/a/w f").contains(Reach::FILE_WRITE | Reach::NAMED_PATH));
    }

    #[test]
    fn a_command_nested_too_deep_to_run_reaches_nothing() {
        let reach = |prefix: &str, n: usize| {
            parse_ex_command(&format!("{}w f", prefix.repeat(n)))
                .map(|c| c.reach())
                .unwrap_or_default()
        };
        for (prefix, deepest) in [("g/a/", 30), ("windo g/a/", 15)] {
            assert!(
                reach(prefix, deepest).contains(Reach::FILE_WRITE),
                "{prefix}"
            );
            assert!(
                !reach(prefix, deepest + 1).contains(Reach::FILE_WRITE),
                "{prefix}"
            );
        }
    }

    #[test]
    fn reach_of_paths_shells_and_options() {
        let reach = |line: &str| {
            parse_ex_command(line)
                .map(|c| c.reach())
                .unwrap_or_default()
        };
        assert_eq!(reach("w"), Reach::HOST | Reach::FILE_WRITE);
        assert_eq!(
            reach("w f"),
            Reach::HOST | Reach::FILE_WRITE | Reach::NAMED_PATH
        );
        assert_eq!(reach("w !sh"), Reach::HOST | Reach::SHELL);
        assert_eq!(reach("r !ls"), Reach::BUFFER | Reach::HOST | Reach::SHELL);
        assert_eq!(
            reach("source"),
            Reach::HOST | Reach::SOURCE | Reach::FILE_READ
        );
        assert_eq!(reach("set ic tw=3"), Reach::OPTIONS);
        assert_eq!(
            reach("set ic lmap=x"),
            Reach::OPTIONS | Reach::SECURE_OPTION
        );
        assert_eq!(reach("set noshell"), Reach::OPTIONS | Reach::SECURE_OPTION);
        assert_eq!(
            reach("set invlangmap"),
            Reach::OPTIONS | Reach::SECURE_OPTION
        );
        assert_eq!(reach("nnoremap x"), Reach::MESSAGE);
        assert_eq!(reach("iabbrev"), Reach::MESSAGE);
        assert_eq!(
            reach("nnoremap <buffer> x y"),
            Reach::MAPPING | Reach::BUFFER
        );
    }
}
