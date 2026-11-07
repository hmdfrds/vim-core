//! Sourcing a config through the host that read it.
//!
//! A host hands [`VimEngine::source_script`](crate::execution::VimEngine::source_script)
//! a [`ConfigText`] and a [`ConfigHost`], which answers what the engine
//! cannot know itself while the config runs, and judges each command
//! before it runs.

use crate::execution::{HostResponse, Response};
pub use crate::grammar::types::Reach;
use crate::grammar::types::{ExCommand, MapModePrefix, ModifierFlags};
use crate::keymap::{MappingFlags, MappingKind};
use crate::primitives::AbbrevMode;
pub use crate::primitives::TrustTag;

/// A config a host has read: its name, its text and a tag of the host's.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct ConfigText<'a> {
    /// The name its errors are reported under, such as `"init.vim"`; `""`
    /// for none.
    pub name: &'a str,
    /// The text of the config.
    pub text: &'a str,
    /// Opaque to vim-core, kept for the host.
    pub trust: TrustTag,
}

impl<'a> ConfigText<'a> {
    /// The config `text` named `name`, with the default [`TrustTag`].
    #[must_use]
    pub const fn new(name: &'a str, text: &'a str) -> Self {
        Self {
            name,
            text,
            trust: TrustTag(0),
        }
    }

    /// The same config with the tag `trust`.
    #[must_use]
    pub const fn with_trust(self, trust: TrustTag) -> Self {
        Self { trust, ..self }
    }
}

/// What the host answers while a config runs.
pub trait ConfigHost {
    /// Decide about one command of the config, once the engine has parsed
    /// it and found nothing wrong with it, before it changes anything.
    /// The engine runs the very command it hands here.
    ///
    /// Asked once for each `|` command that would run, a joined
    /// continuation line being one line. Not asked for comments, blank
    /// lines, the lines of a heredoc or function body, the commands of
    /// `:if`, `:while`, `:try` and `:finish`, which change nothing, or a
    /// command the engine refuses itself: one that does not parse, a Vim
    /// command vim-core lacks (E319), a `:source` too deep (E169) and one
    /// that needs a buffer (E523).
    ///
    /// By default everything is allowed.
    fn judge(&mut self, _command: &ConfigCommand<'_>) -> Verdict {
        Verdict::Allow
    }

    /// Whether `has({feature})` is 1 for a feature vim-core does not
    /// answer itself, such as one the host names after itself. The name
    /// comes in lowercase, as Vim compares feature names without case.
    fn has_feature(&self, _feature: &str) -> bool {
        false
    }

    /// What to do with a `:source` in the config. By default it becomes a
    /// [`HostRequest::ReadConfigFile`](crate::execution::HostRequest::ReadConfigFile)
    /// request, as for a host that reads no file while a config runs.
    fn source(&mut self, _request: &SourceRequest<'_>) -> SourceReply {
        SourceReply::Defer
    }
}

/// What a host answers about a command line that keys from a mapping with
/// a non-default [`TrustTag`] opened, edited or ran, before it runs.
///
/// Set with
/// [`VimEngine::set_command_line_judge`](crate::execution::VimEngine::set_command_line_judge).
/// A line only typed, or made by mappings with the default tag, is never
/// judged. A tag stays on a line: keys the user types into a line a
/// mapping left open, the history entry and the last command line keep
/// it, and so do the keys `:normal` and a register run from the line. A
/// mapping or abbreviation a tagged line defines keeps the highest tag of
/// the line (see [`TrustTag`]), so the lines its keys make are judged too.
/// A register a tagged mapping filled is not tagged when the user runs it
/// with `@a` or `:@a`: the user ran it.
pub trait CommandLineJudge: Send {
    /// Decide about one command of a tagged line. Every `|` command of the
    /// line is judged before any of them runs, each command of a `:g` or
    /// `:v` list and the command of `:windo`, `:bufdo`, `:tabdo`, `:sx`
    /// and `:sy` after the command that runs it, and the lines of a `:@r`
    /// before they run. A command that does not parse is not judged: the
    /// engine gives its error itself.
    ///
    /// [`Verdict::Refuse`] is an error, and nothing of the line from that
    /// command on runs. After [`Verdict::Claim`] the command does not run
    /// and nothing is shown, as for a host that runs it itself; for a
    /// command that another runs, such as one of a `:g` list, that one
    /// does not run either.
    fn judge(&mut self, command: &RuntimeCommand<'_>) -> Verdict;
}

/// A command of a tagged command line, for [`CommandLineJudge::judge`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct RuntimeCommand<'a> {
    /// The command as split from its line, its range and modifiers
    /// included, without the blanks around it.
    pub text: &'a str,
    /// The non-default tags of the keys that made the line, each once, in
    /// order of their numbers; never empty.
    pub tags: &'a [TrustTag],
    /// Its modifiers.
    pub modifiers: ModifierFlags,
    /// The command as the engine parses it to run it.
    pub command: &'a ExCommand,
    /// What running it reaches: the [`ExCommand::reach`] of `command`.
    pub reach: Reach,
}

/// A `:source` in a config, for [`ConfigHost::source`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRequest<'a> {
    /// The file as typed, unexpanded; `None` for a bare `:source`. The host
    /// expands `~`, `$VAR`, `%` and `<sfile>`, which names `from_file`.
    pub path: Option<&'a str>,
    /// The name of the config the `:source` is in.
    pub from_file: &'a str,
    /// The tag of that config.
    pub from_trust: TrustTag,
    /// The line of the `:source`.
    pub line: u32,
    /// How many files would be running with this one: the config the host
    /// passed is 1, a file it sources 2. The file is refused with E169
    /// before it would be 201, so the host is never asked for more than
    /// 200.
    pub depth: u32,
}

/// What [`ConfigHost::source`] answers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceReply {
    /// The text of the file, which runs now, before the commands after the
    /// `:source`, named `name` in its errors and tagged `trust`.
    Text {
        /// The name its errors are reported under.
        name: String,
        /// The text of the file.
        text: String,
        /// Opaque to vim-core, handed back for the `:source` commands in it.
        trust: TrustTag,
    },
    /// The host sourced the file itself; the message, if any, is shown.
    Done(Option<String>),
    /// The file is not sourced, and the text is shown as the error of the
    /// `:source`, a [`VimError::Refused`](crate::errors::VimError::Refused)
    /// as written, such as `E484: Can't open file b.vim`.
    Refuse(String),
    /// The `:source` becomes a
    /// [`HostRequest::ReadConfigFile`](crate::execution::HostRequest::ReadConfigFile)
    /// request, answered once the config has run.
    Defer,
}

/// What [`HostSession::source_script`](crate::execution::HostSession::source_script)
/// returns.
#[non_exhaustive]
#[derive(Debug)]
pub struct HostConfigResponse {
    /// The session after the config ran, with its messages delivered.
    pub response: HostResponse,
    /// What it came to.
    pub outcome: ConfigOutcome,
}

/// A host that allows every command and leaves each nested `:source` for
/// later as a
/// [`HostRequest::ReadConfigFile`](crate::execution::HostRequest::ReadConfigFile)
/// request.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrustAll;

impl TrustAll {
    /// The host that allows everything.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ConfigHost for TrustAll {}

/// What [`ConfigHost::judge`] decides about a command.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The command runs.
    Allow,
    /// The command does not run, and the text is shown as its error, a
    /// [`VimError::Refused`](crate::errors::VimError::Refused) at its
    /// line. As after any error, the commands after it on its line do not
    /// run, nor, inside a block, those up to the end of the outermost
    /// block; `:silent!` before it hides the error, and then they do.
    Refuse(Box<str>),
    /// The command does not run and nothing is shown: the host has taken
    /// it, such as a line it runs itself.
    Claim,
}

/// A command of a config, for [`ConfigHost::judge`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct ConfigCommand<'a> {
    /// The name of the config it is in.
    pub file: &'a str,
    /// The tag of that config.
    pub trust: TrustTag,
    /// How many files run with this one: the config the host passed is 1,
    /// a file it sources 2, as in [`SourceRequest::depth`].
    pub depth: u32,
    /// Its line, the first of a continued line.
    pub line: u32,
    /// The command as written, its modifiers included, without the blanks
    /// around it.
    pub text: &'a str,
    /// Its modifiers, with those of the `:source` that runs its file.
    pub modifiers: ModifierFlags,
    /// What it does.
    pub action: ConfigAction<'a>,
    /// What running it reaches. For a command the engine runs as the
    /// command line does, the [`ExCommand::reach`] of `command`.
    pub reach: Reach,
    /// The command in full, as the engine runs it. A `:let` that assigns
    /// a variable or an option vim-core lacks is the `Custom` command of
    /// its text, and `:let &name` the `:set` it amounts to.
    pub command: &'a ExCommand,
}

/// What a config command does, for [`ConfigHost::judge`]. A later version
/// may add kinds; a host that allows by kind should refuse the others.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction<'a> {
    /// Sets or shows options: `:set`, `:setlocal`, `:setglobal` and
    /// `:let &name`.
    #[non_exhaustive]
    SetOptions {
        /// The options it names, in order.
        options: &'a [OptionTarget],
    },
    /// Defines a mapping.
    Map(&'a MapDefinition<'a>),
    /// Removes a mapping: `:unmap` and its mode forms.
    #[non_exhaustive]
    Unmap {
        /// The modes.
        modes: MapModePrefix,
        /// The keys as written.
        lhs: &'a str,
    },
    /// Removes the mappings of modes: `:mapclear` and its mode forms.
    #[non_exhaustive]
    MapClear {
        /// The modes.
        modes: MapModePrefix,
    },
    /// Defines an abbreviation.
    #[non_exhaustive]
    Abbreviate {
        /// The modes.
        mode: AbbrevMode,
        /// The text typed.
        trigger: &'a str,
        /// The text it becomes, as written.
        replacement: &'a str,
    },
    /// Removes an abbreviation.
    #[non_exhaustive]
    Unabbreviate {
        /// The modes.
        mode: AbbrevMode,
        /// The text typed.
        trigger: &'a str,
    },
    /// Removes the abbreviations of modes: `:abclear` and its mode forms.
    #[non_exhaustive]
    AbClear {
        /// The modes.
        mode: AbbrevMode,
    },
    /// Sets the leader: `:let mapleader` or `:let maplocalleader`.
    #[non_exhaustive]
    Leader {
        /// The local leader.
        local: bool,
        /// The value it takes, or for the command line's `:let` form the
        /// expression.
        value: &'a str,
    },
    /// Changes the `:sethandler` table.
    Handler,
    /// Shows a message or a listing: `:echo`, `:nohlsearch`,
    /// `:registers`, `:messages`, and the map and abbreviation commands
    /// that list.
    Message,
    /// Sources a file.
    #[non_exhaustive]
    Source {
        /// The file as typed, unexpanded; `None` for a bare `:source`.
        path: Option<&'a str>,
    },
    /// Becomes a host request, such as `:write` or `:action`.
    Host,
    /// A command vim-core does not run itself, which becomes a
    /// [`HostRequest::CustomExCommand`](crate::execution::HostRequest::CustomExCommand)
    /// request: `:filetype`, or a command that is not Vim's.
    #[non_exhaustive]
    Custom {
        /// Its name, in full for one of Vim's.
        name: &'a str,
        /// The text after the name.
        args: &'a str,
        /// It is one of Vim's commands.
        vim_command: bool,
    },
    /// Something else the engine runs itself, such as `:let` of a
    /// variable or `:unlet`.
    Other,
}

/// An option a command names.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionTarget {
    /// Its full name, for an option of vim-core or of Vim; otherwise the
    /// name as written.
    pub name: Box<str>,
    /// vim-core has the option; setting one it lacks does nothing.
    pub known: bool,
    /// Vim keeps it from a modeline and the sandbox (`P_SECURE` in
    /// optiondefs.h), such as 'langmap' or 'shell'.
    pub secure: bool,
}

/// A mapping a config command defines.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapDefinition<'a> {
    /// The modes.
    pub modes: MapModePrefix,
    /// The keys as written.
    pub lhs: &'a str,
    /// The keys they become, as written.
    pub rhs: &'a str,
    /// Whether the keys it gives are mapped again.
    pub kind: MappingKind,
    /// `<nowait>`, `<silent>`, `<expr>` and `<unique>`.
    pub flags: MappingFlags,
    /// What its keys can reach when it runs, as
    /// [`VimEngine::mapping_reach`](crate::execution::VimEngine::mapping_reach)
    /// gives it.
    pub reach: &'a MappingReach,
}

/// What the keys of a mapping can reach when it runs, from
/// [`VimEngine::mapping_reach`](crate::execution::VimEngine::mapping_reach).
///
/// It follows the keys as the engine reads them, in the mode the mapping
/// starts in: in Insert mode they are text until a key that leaves it; in
/// a command line they edit it; in any other mode every `:` opens a
/// command line, and every key after it runs as a command too, since the
/// `:` may be the argument of the key before it. A key whose effect it
/// cannot follow is in [`opaque`](Self::opaque), so a host that allows a
/// mapping by what it reaches should refuse one with any.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MappingReach {
    /// Each command a command line or `<Cmd>` of the keys can run, once,
    /// in the order found, with the commands of a `:g` list after it.
    pub commands: Vec<ReachedCommand>,
    /// What the keys do that it cannot follow, each kind once.
    pub opaque: Vec<Opaque>,
    /// The names of the `<Action>(name)` keys, each once.
    pub actions: Vec<Box<str>>,
    /// The names of the `<Plug>(name)` keys, each once. A `<Plug>` key
    /// runs the mapping of its name even from a non-recursive mapping.
    pub plugs: Vec<Box<str>>,
}

impl MappingReach {
    /// What its commands reach together.
    #[must_use]
    pub fn reach(&self) -> Reach {
        self.commands
            .iter()
            .fold(Reach::empty(), |reach, command| reach | command.reach)
    }
}

/// A command the keys of a mapping can run, one `|` command of the line
/// they type.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachedCommand {
    /// The command as split from its line, its range and modifiers
    /// included. A line opened in Visual mode starts with `'<,'>`.
    pub text: Box<str>,
    /// The line is still open when the keys end, or may be: the user
    /// types into it and runs it with Enter. Such a line is also given
    /// with a blank, a `|` and a digit typed at its cursor and at its end.
    pub open: bool,
    /// The command as the engine parses it to run it.
    pub parsed: Result<(ModifierFlags, ExCommand), crate::errors::VimError>,
    /// What running it reaches; empty for one that does not parse.
    pub reach: Reach,
}

/// What the keys of a mapping do that
/// [`MappingReach`] cannot follow. A later version may add kinds.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opaque {
    /// An `<expr>` mapping: its keys come from an expression when it runs.
    Expr,
    /// A recursive mapping: its keys can run other mappings.
    Recursive,
    /// `@`, which runs a register as keys, or `@:` the last command line.
    RegisterExecution,
    /// `!`, the filter operator, which runs a shell command.
    FilterOperator,
    /// `q:`, `q/`, `q?` or `<C-f>`: the command-line window.
    CommandWindow,
    /// Completion in a command line, which puts text there.
    CommandLineCompletion,
    /// History in a command line, which puts an earlier line there.
    CommandLineHistory,
    /// `<C-r>` in Insert mode or a command line, which puts a register
    /// there.
    RegisterInsert,
    /// `:normal`, which runs its keys.
    NormalKeys,
    /// A command-line mapping that deletes part of what the user typed
    /// before it, which is not known.
    UserText,
    /// More keys, branches or work than the analysis follows.
    TooComplex,
}

/// What running a config came to.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigOutcome {
    /// The errors it showed; those `:silent!` hid are not counted.
    pub errors: u32,
    /// The commands the host judged.
    pub commands: u32,
    /// Of those, the ones the host refused.
    pub refused: u32,
    /// Of those, the ones the host claimed.
    pub claimed: u32,
}

/// What [`VimEngine::source_script`](crate::execution::VimEngine::source_script)
/// returns.
#[non_exhaustive]
#[derive(Debug)]
#[must_use = "ConfigResponse contains effects that must be applied to the editor"]
pub struct ConfigResponse {
    /// Its messages, errors and host requests.
    pub response: Response,
    /// What it came to.
    pub outcome: ConfigOutcome,
}
