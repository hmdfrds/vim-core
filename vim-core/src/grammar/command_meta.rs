//! Per-command metadata for Ex commands.
//!
//! Provides `CmdFlags` bitflags and `CommandMeta` structs that describe
//! each Ex command's capabilities: whether it accepts a range, bang,
//! register argument, trailing bar separator, etc.
//!
//! The data is Vim's own command table (`ex_cmds.h`), and a command name is
//! found the way Vim's `find_ex_command()` finds it, with the engine's own
//! commands first. The same lookup decides where each command of an Ex line
//! ends, see [`split_ex_pipeline`](super::split_ex_pipeline).
//!
//! # Layering
//!
//! Grammar-only module: it imports nothing from `execution`, `effects` or
//! `commands`.

use bitflags::bitflags;

use super::vim_commands::{ex, Addr, VimCommand, COMMANDS};

bitflags! {
    /// Per-command capability flags, from Vim's `EX_*` defines in `ex_cmds.h`.
    ///
    /// Only the flags relevant to this engine are included.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CmdFlags: u16 {
        /// Command accepts a line range (`:1,5d`).
        const RANGE    = 0x001;
        /// Command accepts a `!` suffix (`:w!`, `:q!`).
        const BANG     = 0x002;
        /// Command accepts extra arguments after the name.
        const EXTRA    = 0x004;
        /// Default range is the entire file (`1,$`) instead of current line.
        const DFLALL   = 0x008;
        /// Command accepts a count argument (`:3d`, `:5bn`).
        const COUNT    = 0x010;
        /// Command accepts a register argument (`:d a`, `:y b`).
        const REGSTR   = 0x020;
        /// Command can be followed by `|` to chain another command.
        /// When set, the pipeline splitter treats `|` as a separator.
        const TRLBAR   = 0x040;
        /// No trailing comment allowed — `"` is part of the argument,
        /// not a comment delimiter. Overrides `TRLBAR` for comment stripping.
        const NOTRLCOM = 0x080;
        /// Command modifies the buffer (forbidden in non-modifiable buffers).
        const MODIFY   = 0x100;
        /// Command requires at least one argument (`:normal`, `:windo`).
        const NEEDARG  = 0x200;
        /// A Ctrl-V in the argument is kept (mappings, `:normal`).
        const CTRLV    = 0x400;
        /// The argument is a file name, with wildcards expanded.
        const XFILE    = 0x800;
        /// The argument is an expression, which decides where it ends.
        const EXPR_ARG = 0x1000;
        /// Vim allows the command in its sandbox.
        const SBOXOK   = 0x2000;
    }
}

impl CmdFlags {
    /// The flags among Vim's `EX_*` bits `vim`.
    fn from_vim(vim: u32) -> Self {
        const MAP: [(u32, CmdFlags); 14] = [
            (ex::RANGE, CmdFlags::RANGE),
            (ex::BANG, CmdFlags::BANG),
            (ex::EXTRA, CmdFlags::EXTRA),
            (ex::DFLALL, CmdFlags::DFLALL),
            (ex::COUNT, CmdFlags::COUNT),
            (ex::REGSTR, CmdFlags::REGSTR),
            (ex::TRLBAR, CmdFlags::TRLBAR),
            (ex::NOTRLCOM, CmdFlags::NOTRLCOM),
            (ex::MODIFY, CmdFlags::MODIFY),
            (ex::NEEDARG, CmdFlags::NEEDARG),
            (ex::CTRLV, CmdFlags::CTRLV),
            (ex::XFILE, CmdFlags::XFILE),
            (ex::EXPR_ARG, CmdFlags::EXPR_ARG),
            (ex::SBOXOK, CmdFlags::SBOXOK),
        ];
        MAP.iter()
            .filter(|(bit, _)| vim & bit != 0)
            .fold(Self::empty(), |flags, &(_, flag)| flags.union(flag))
    }
}

/// Address type for ex command ranges.
///
/// Determines how numeric range arguments are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddrType {
    /// Buffer line numbers (most common).
    Lines,
    /// Window numbers (`:close`, `:only`, `:quit`).
    Windows,
    /// Buffer numbers (`:buffer`, `:bdelete`).
    Buffers,
    /// Tab page numbers (`:tabnext`, `:tabclose`).
    Tabs,
    /// Positive count or zero, defaults to 1 (`:cnext`, `:cquit`).
    Unsigned,
    /// Other address interpretation (`:earlier`, `:bnext`).
    Other,
    /// No range used (`:echo`, `:set`, `:map`).
    None,
}

impl AddrType {
    const fn from_vim(addr: Addr) -> Self {
        match addr {
            Addr::Lines => Self::Lines,
            Addr::Windows => Self::Windows,
            Addr::LoadedBuffers | Addr::Buffers => Self::Buffers,
            Addr::Tabs | Addr::TabsRelative => Self::Tabs,
            Addr::QuickfixValid | Addr::Quickfix | Addr::Unsigned => Self::Unsigned,
            Addr::Arguments | Addr::Other => Self::Other,
            Addr::None => Self::None,
        }
    }
}

/// Metadata for a single ex command.
///
/// Combines capability flags with the address type to fully describe
/// what arguments and modifiers a command accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandMeta {
    /// Capability flags for this command.
    pub flags: CmdFlags,
    /// How range arguments are interpreted.
    pub addr_type: AddrType,
}

impl CommandMeta {
    /// Create a new `CommandMeta`.
    #[inline]
    const fn new(flags: CmdFlags, addr_type: AddrType) -> Self {
        Self { flags, addr_type }
    }

    /// Returns `true` if trailing comments (`" ...`) should be stripped.
    ///
    /// A trailing comment is allowed when `TRLBAR` is set AND `NOTRLCOM` is NOT set.
    #[inline]
    #[must_use]
    pub const fn allows_trailing_comment(&self) -> bool {
        self.flags.contains(CmdFlags::TRLBAR) && !self.flags.contains(CmdFlags::NOTRLCOM)
    }

    /// Returns `true` if this command can be followed by `|` in a pipeline.
    #[inline]
    #[must_use]
    pub const fn allows_bar(&self) -> bool {
        self.flags.contains(CmdFlags::TRLBAR)
    }
}

/// How one of the engine's own commands ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnKind {
    /// At `|` or a `"` comment, as a Vim command with `EX_TRLBAR` does.
    Bar,
    /// At the first unescaped `|`; a `"` is part of the argument.
    Plain,
    /// As `Plain`, after a delimited pattern argument such as `/a|b/`.
    Pattern,
}

/// One of the engine's own commands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OwnCommand {
    /// The shortest form.
    pub(crate) min: &'static str,
    /// The full name.
    pub(crate) full: &'static str,
    /// How the command ends.
    pub(crate) kind: OwnKind,
}

const fn own(min: &'static str, full: &'static str, kind: OwnKind) -> OwnCommand {
    OwnCommand { min, full, kind }
}

/// The engine's own commands, in the order the parser tries them, which is
/// before Vim's table. `sp[lit]` is one of them only with a pattern.
static OWN_COMMANDS: [OwnCommand; 28] = [
    own("sel", "select", OwnKind::Pattern),
    own("sp", "split", OwnKind::Pattern),
    own("kee", "keep", OwnKind::Pattern),
    own("remo", "remove", OwnKind::Pattern),
    own("tri", "trim", OwnKind::Plain),
    own("ali", "align", OwnKind::Plain),
    own("rot", "rotate", OwnKind::Plain),
    own("addn", "addnext", OwnKind::Plain),
    own("addp", "addprev", OwnKind::Plain),
    own("skip", "skipmatch", OwnKind::Plain),
    own("addc", "addcursor", OwnKind::Plain),
    own("selecta", "selectall", OwnKind::Plain),
    own("cursorf", "cursorfilter", OwnKind::Pattern),
    own("cursorfl", "cursorflip", OwnKind::Plain),
    own("cursorfo", "cursorforward", OwnKind::Plain),
    own("cursorco", "cursorcollapse", OwnKind::Plain),
    own("cursorm", "cursormerge", OwnKind::Plain),
    own("cursorp", "cursorprimary", OwnKind::Plain),
    own("cursorrem", "cursorremove", OwnKind::Plain),
    own("cursorrot", "cursorrotate", OwnKind::Plain),
    own("cursorsplits", "cursorsplitsel", OwnKind::Pattern),
    own("cursorsp", "cursorsplit", OwnKind::Plain),
    own("cursorsel", "cursorselect", OwnKind::Pattern),
    own("cursort", "cursortrim", OwnKind::Plain),
    own("cursora", "cursoralign", OwnKind::Plain),
    own("seth", "sethandler", OwnKind::Bar),
    own("actionl", "actionlist", OwnKind::Bar),
    own("undot", "undotree", OwnKind::Bar),
];

/// The forms the parser reads as another command than Vim does, with the
/// command the parser reads. Deviation: Vim reads `:sm` as `:smagic`,
/// `:sn` as `:snext`, `:sno` as `:snomagic`, `:su` as `:substitute`, `:sun`
/// as `:sunhide` and `:cha` as `:change`.
const PARSER_FORMS: [(&str, &str); 12] = [
    ("sm", "smap"),
    ("sma", "smap"),
    ("sn", "snoremap"),
    ("sno", "snoremap"),
    ("su", "sunmap"),
    ("sun", "sunmap"),
    ("cha", "changes"),
    ("chan", "changes"),
    ("chang", "changes"),
    ("change", "changes"),
    ("centr", "center"),
    ("centre", "center"),
];

/// A command found at the start of an Ex command.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FoundCommand {
    /// A row of Vim's command table.
    Vim(&'static VimCommand),
    /// One of the engine's own commands.
    Own(&'static OwnCommand),
    /// The engine's `:sx` or `:sy`, followed by a pattern.
    Structural,
}

impl FoundCommand {
    fn meta(self) -> CommandMeta {
        let (flags, addr) = match self {
            Self::Vim(command) => (
                CmdFlags::from_vim(command.flags),
                AddrType::from_vim(command.addr),
            ),
            Self::Own(OwnCommand {
                kind: OwnKind::Bar, ..
            }) => (
                CmdFlags::BANG
                    .union(CmdFlags::EXTRA)
                    .union(CmdFlags::TRLBAR),
                AddrType::None,
            ),
            Self::Own(_) | Self::Structural => (
                CmdFlags::RANGE
                    .union(CmdFlags::BANG)
                    .union(CmdFlags::EXTRA)
                    .union(CmdFlags::TRLBAR)
                    .union(CmdFlags::NOTRLCOM),
                AddrType::Lines,
            ),
        };
        CommandMeta::new(flags, addr)
    }
}

/// Check if `name` matches the abbreviation range from `min` to `full` (case-insensitive).
#[inline]
fn matches_abbrev(name: &str, min: &str, full: &str) -> bool {
    let n = name.len();
    n >= min.len() && n <= full.len() && name.eq_ignore_ascii_case(&full[..n])
}

/// Whether the parser reads `byte` after `s`, `g`, `v`, `sx` or `sy` as a
/// pattern delimiter.
pub(crate) const fn is_pattern_delimiter(byte: Option<u8>) -> bool {
    match byte {
        None => false,
        Some(b) => !b.is_ascii_alphanumeric() && b != b'_',
    }
}

/// The command `text` starts with and the length of its name, found as
/// Vim's `find_ex_command()` and `one_letter_cmd()` find it in a legacy
/// script, with the engine's own commands first, as the parser takes them.
///
/// Deviation: a name is read in lowercase, as the parser reads it; Vim
/// matches case, so `:Set` is a user command and `:bN` is `:bNext`.
pub(crate) fn find_command(text: &str) -> Option<(FoundCommand, usize)> {
    let bytes = text.as_bytes();
    let alpha_len = bytes
        .iter()
        .position(|b| !b.is_ascii_alphabetic())
        .unwrap_or(bytes.len());
    let alpha = &text[..alpha_len];

    if (alpha.starts_with("sx") || alpha.starts_with("sy"))
        && alpha_len == 2
        && is_pattern_delimiter(bytes.get(2).copied())
    {
        return Some((FoundCommand::Structural, 2));
    }
    if let Some(own) = own_command(text, alpha) {
        return Some((FoundCommand::Own(own), alpha_len));
    }
    // A modifier the engine does not know, used as a command: Vim takes it
    // before `one_letter_cmd()` sees `:keepmarks` as `:k`.
    if let Some(&(name, _, _)) = super::vim_commands::MODIFIERS
        .iter()
        .find(|(name, min, _)| alpha_len >= *min && matches_abbrev(alpha, &name[..*min], name))
    {
        return vim_command(name).map(|row| (FoundCommand::Vim(row), alpha_len));
    }
    if let Some(&(_, name)) = PARSER_FORMS
        .iter()
        .find(|(form, _)| alpha.eq_ignore_ascii_case(form))
    {
        return vim_command(name).map(|row| (FoundCommand::Vim(row), alpha_len));
    }
    if let Some(row) = one_letter_command(bytes) {
        return Some((FoundCommand::Vim(row), 1));
    }

    let mut len = alpha_len;
    if alpha
        .get(..2)
        .is_some_and(|start| start.eq_ignore_ascii_case("py"))
    {
        len += bytes
            .iter()
            .skip(len)
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
    } else if bytes.get(len) == Some(&b'9') && alpha.eq_ignore_ascii_case("vim") {
        len += 1 + bytes
            .iter()
            .skip(len + 1)
            .take_while(|b| b.is_ascii_alphabetic())
            .count();
    }
    if len == 0 {
        let first = *bytes.first()?;
        if !b"@*!=><&~#}".contains(&first) {
            return None;
        }
        len = 1;
    }
    let name = text.get(..len)?;
    // `:dl`, `:dell`, ... `:deletel` are `:delete` with the `l` flag, and the
    // same for `p`.
    let lookup = match name.strip_suffix(['l', 'p']) {
        Some(head) if !head.is_empty() && "delete".starts_with(head) => head,
        _ => name,
    };
    let typed = lookup.to_ascii_lowercase();
    let row = COMMANDS
        .iter()
        .find(|row| row.name.starts_with(typed.as_str()))?;
    // `:*` is the star command only with `*` in 'cpoptions'.
    if row.name == "*" {
        return None;
    }
    // `:fina` is `:finally` in a legacy script.
    let row = if row.name == "final" && len == 4 {
        vim_command("finally")?
    } else {
        row
    };
    Some((FoundCommand::Vim(row), len))
}

/// What `exists(':name')` gives for `name`, as Vim's `cmd_exists()`
/// decides it: 2 for the full name of a modifier or a command, 1 for a
/// shorter form, 0 for no command or text after the name.
///
/// A command is found as [`find_command`] finds it, so its name is read
/// in lowercase.
pub(crate) fn cmd_exists(name: &str) -> i64 {
    if let Some(&(full, _, _)) = super::vim_commands::MODIFIERS
        .iter()
        .find(|(full, min, _)| name.len() >= *min && full.starts_with(name))
    {
        return if full.len() == name.len() { 2 } else { 1 };
    }
    // `:2match` and `:3match`.
    let digit = name.starts_with(|c: char| c.is_ascii_digit());
    let text = name.strip_prefix(['2', '3']).unwrap_or(name);
    let Some((found, len)) = find_command(text) else {
        return 0;
    };
    let full = match found {
        FoundCommand::Vim(row) => {
            if digit && row.name != "match" {
                return 0;
            }
            row.name.len() == len
        }
        FoundCommand::Own(own) if !digit => own.full.len() == len,
        FoundCommand::Own(_) | FoundCommand::Structural => return 0,
    };
    if !text[len..].trim_start_matches([' ', '\t']).is_empty() {
        return 0;
    }
    if full {
        2
    } else {
        1
    }
}

/// The own command `text` starts with, whose name is `alpha`. `:action` is
/// only that whole word, and `:sp[lit]` only with a pattern after it.
fn own_command(text: &str, alpha: &str) -> Option<&'static OwnCommand> {
    if alpha.eq_ignore_ascii_case("action") {
        return Some(&ACTION);
    }
    let own = OWN_COMMANDS
        .iter()
        .find(|own| matches_abbrev(alpha, own.min, own.full))?;
    if own.full == "split" {
        let after = text[alpha.len()..].trim_start_matches('!');
        let first = after.trim_start_matches([' ', '\t']).chars().next()?;
        if !is_split_pattern_delimiter(first) {
            return None;
        }
    }
    Some(own)
}

/// Whether `:sp[lit]` followed by `c` splits the cursors at a pattern.
/// Not after `|` or `"`, which end Vim's `:split` or start a comment, nor
/// after `\`, which escapes them in a file name: `:sp \|x` opens `|x`.
pub(crate) fn is_split_pattern_delimiter(c: char) -> bool {
    !(c.is_alphanumeric() || c.is_whitespace() || matches!(c, '!' | '|' | '"' | '\\'))
}

static ACTION: OwnCommand = own("action", "action", OwnKind::Bar);

/// `:k` followed by anything, and the `s` forms Vim reads as
/// `:substitute` with flags (`one_letter_cmd()`).
fn one_letter_command(p: &[u8]) -> Option<&'static VimCommand> {
    let at = |i: usize| p.get(i).copied().unwrap_or(0);
    if at(0) == b'k' {
        return vim_command("k");
    }
    let substitute = at(0) == b's'
        && ((at(1) == b'c'
            && (at(2) == 0
                || (at(2) != b's'
                    && at(2) != b'r'
                    && (at(3) == 0 || (at(3) != b'i' && at(4) != b'p')))))
            || at(1) == b'g'
            || (at(1) == b'i' && at(2) != b'm' && at(2) != b'l' && at(2) != b'g')
            || at(1) == b'I'
            || (at(1) == b'r' && at(2) != b'e'));
    if substitute {
        vim_command("substitute")
    } else {
        None
    }
}

/// The row of Vim's table with the full name `name`.
pub(crate) fn vim_command(name: &str) -> Option<&'static VimCommand> {
    COMMANDS.iter().find(|row| row.name == name)
}

/// Look up metadata for a named ex command.
///
/// Accepts every abbreviation of a command name that the parser accepts:
/// Vim's commands, found in Vim's command table the way Vim finds them
/// (`:s` is `:substitute`, `:r` is `:read`, `:fina` is `:finally`), and the
/// engine's own commands, such as `:sethandler`. Case does not matter.
///
/// Returns `None` for a name that is no command, such as a user command.
///
/// # Examples
///
/// ```
/// use vim_core::grammar::command_meta::{meta_for_command, CmdFlags};
///
/// let meta = meta_for_command("d").unwrap();
/// assert!(meta.flags.contains(CmdFlags::RANGE));
/// assert!(meta.flags.contains(CmdFlags::REGSTR));
///
/// let meta = meta_for_command("echo").unwrap();
/// assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
/// assert!(!meta.allows_trailing_comment());
/// ```
#[must_use]
pub fn meta_for_command(name: &str) -> Option<CommandMeta> {
    match find_command(name) {
        Some((found, len)) if len == name.len() => Some(found.meta()),
        _ => None,
    }
}

/// Strip a trailing comment from an ex command argument string.
///
/// In Vim, commands with `TRLBAR` (but not `NOTRLCOM`) treat `"` as a
/// comment delimiter: everything from an unescaped `"` to end-of-line
/// is a comment. The `"` must be preceded by whitespace (or be at the
/// start) to count as a comment — a `"` embedded in an argument is not.
///
/// # Rules
///
/// 1. A `"` preceded by a backslash (`\"`) is an escaped quote, not a comment.
/// 2. A `"` preceded by whitespace (or at position 0) starts a comment.
/// 3. A `"` not preceded by whitespace is part of the argument.
///
/// **Important**: only call this for commands where `allows_trailing_comment()`
/// returns `true` (i.e., commands with `TRLBAR` but not `NOTRLCOM`). For commands
/// like `:echo` or `:normal`, `"` is part of the argument, not a comment.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(strip_trailing_comment("set number \" enable line numbers"), "set number");
/// assert_eq!(strip_trailing_comment("set ts=4"), "set ts=4");
/// assert_eq!(strip_trailing_comment("\" full line comment"), "");
/// ```
#[must_use]
#[allow(
    clippy::indexing_slicing,
    reason = "i is bounded by `while i < len` and starts at 1, so i and i-1 are always valid"
)]
pub fn strip_trailing_comment(input: &str) -> &str {
    let bytes = input.as_bytes();
    let len = bytes.len();

    // Position 0: a `"` at the very start is a full-line comment.
    if bytes.first() == Some(&b'"') {
        return "";
    }

    let mut i = 1;
    while i < len {
        if bytes[i] == b'"' {
            // Skip escaped quotes.
            if bytes[i - 1] == b'\\' {
                i += 1;
                continue;
            }
            // Comment requires preceding whitespace.
            if bytes[i - 1] == b' ' || bytes[i - 1] == b'\t' {
                // Trim trailing whitespace before the comment.
                return input[..i].trim_end();
            }
        }
        i += 1;
    }

    // No comment found — return the full input (trimmed trailing whitespace).
    input.trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_trailing_comment ─────────────────────────────────────────

    #[test]
    fn strip_simple_trailing_comment() {
        assert_eq!(
            strip_trailing_comment("set number \" enable line numbers"),
            "set number"
        );
    }

    #[test]
    fn strip_no_comment() {
        assert_eq!(strip_trailing_comment("set ts=4"), "set ts=4");
    }

    #[test]
    fn strip_full_line_comment() {
        assert_eq!(strip_trailing_comment("\" this is a comment"), "");
    }

    #[test]
    fn strip_empty_input() {
        assert_eq!(strip_trailing_comment(""), "");
    }

    #[test]
    fn strip_escaped_quote_not_comment() {
        // A backslash-escaped quote should not be treated as a comment start.
        assert_eq!(strip_trailing_comment("echo \\\"hello"), "echo \\\"hello");
    }

    #[test]
    fn strip_quote_without_preceding_whitespace() {
        // A `"` not preceded by whitespace is part of the argument.
        // Note: `strip_trailing_comment` is only called for commands where
        // `allows_trailing_comment()` is true. For :echo (NOTRLCOM), the
        // caller should NOT strip comments.
        assert_eq!(strip_trailing_comment("set opt=\"val\""), "set opt=\"val\"");
    }

    #[test]
    fn strip_comment_after_tab() {
        assert_eq!(
            strip_trailing_comment("set number\t\" comment"),
            "set number"
        );
    }

    #[test]
    fn strip_trailing_whitespace() {
        assert_eq!(strip_trailing_comment("set number   "), "set number");
    }

    #[test]
    fn strip_multiple_quotes_first_wins() {
        // The first whitespace-preceded `"` starts the comment.
        assert_eq!(
            strip_trailing_comment("set ts=4 \" comment \" more"),
            "set ts=4"
        );
    }

    // ── Vim's command table ────────────────────────────────────────────

    fn found(text: &str) -> (Option<&'static str>, usize) {
        match find_command(text) {
            Some((FoundCommand::Vim(row), len)) => (Some(row.name), len),
            Some((_, len)) => (None, len),
            None => (None, 0),
        }
    }

    #[test]
    fn table_holds_every_command_of_ex_cmds_h() {
        assert_eq!(COMMANDS.len(), 594);
        let count = |flag| COMMANDS.iter().filter(|c| c.flags & flag != 0).count();
        assert_eq!(count(ex::TRLBAR), 456);
        assert_eq!(count(ex::EXPR_ARG), 23);
        assert_eq!(super::super::vim_commands::MODIFIERS.len(), 26);
    }

    #[test]
    fn table_flags_are_those_of_ex_cmds_h() {
        use ex::*;
        for (name, flags) in [
            ("set", BANG | TRLBAR | EXTRA | CMDWIN | LOCK_OK | SBOXOK),
            (
                "map",
                BANG | EXTRA | TRLBAR | NOTRLCOM | CTRLV | CMDWIN | LOCK_OK,
            ),
            (
                "if",
                EXTRA | NOTRLCOM | EXPR_ARG | SBOXOK | CMDWIN | LOCK_OK,
            ),
            (
                "echo",
                EXTRA | NOTRLCOM | EXPR_ARG | SBOXOK | CMDWIN | LOCK_OK,
            ),
            (
                "normal",
                RANGE | BANG | EXTRA | NEEDARG | NOTRLCOM | CTRLV | SBOXOK | CMDWIN | LOCK_OK,
            ),
            (
                "source",
                RANGE | DFLALL | BANG | FILE1 | TRLBAR | SBOXOK | CMDWIN | LOCK_OK,
            ),
            ("finish", TRLBAR | SBOXOK | CMDWIN | LOCK_OK | WHOLE),
            ("filetype", EXTRA | TRLBAR | CMDWIN | LOCK_OK),
        ] {
            assert_eq!(vim_command(name).unwrap().flags, flags, ":{name}");
        }
    }

    #[test]
    fn a_name_finds_the_first_row_it_starts_as_find_ex_command_does() {
        for (typed, name) in [
            ("s", "substitute"),
            ("r", "read"),
            ("n", "next"),
            ("fu", "function"),
            ("so", "source"),
            ("sil", "silent"),
            ("fin", "find"),
            ("fini", "finish"),
            ("fina", "finally"),
            ("final", "final"),
            ("py3", "py3"),
            ("python3", "python3"),
            ("vim9s", "vim9script"),
            ("dl", "delete"),
            ("deletep", "delete"),
            ("k", "k"),
            ("ka", "k"),
            ("sg", "substitute"),
            ("snom", "snomagic"),
            ("smag", "smagic"),
            ("smap", "smap"),
            ("e", "edit"),
            ("en", "endif"),
            ("&", "&"),
            ("#", "#"),
            ("filet", "filetype"),
            ("pw", "pwd"),
        ] {
            assert_eq!(found(typed).0, Some(name), ":{typed}");
        }
        assert_eq!(found("dl"), (Some("delete"), 2));
        assert_eq!(found("py3file x"), (Some("py3file"), 7));
        assert_eq!(found("set|x"), (Some("set"), 3));
    }

    #[test]
    fn own_commands_come_before_vim_s_table() {
        for typed in [
            "seth",
            "sethandler",
            "kee",
            "sel",
            "selectall",
            "undot",
            "actionl",
            "action",
            "sp /a/",
            "split! /a/",
        ] {
            assert!(
                matches!(find_command(typed), Some((FoundCommand::Own(_), _))),
                ":{typed}"
            );
        }
        assert!(matches!(
            find_command("sx/a/d"),
            Some((FoundCommand::Structural, 2))
        ));
        // `|` ends Vim's `:split`, `"` starts a comment after it and `\`
        // escapes either in a file name.
        for typed in ["sp x", "sp | e x", "sp!|e x", "sp \"x\"", "sp \\|x", "sp"] {
            assert_eq!(found(typed), (Some("split"), 2), ":{typed}");
        }
        assert!(find_command("actio").is_none());
    }

    #[test]
    fn a_modifier_used_as_a_command_is_its_own_row() {
        assert_eq!(found("keepmarks"), (Some("keepmarks"), 9));
        assert_eq!(found("noa"), (Some("noautocmd"), 3));
        assert_eq!(found("verb"), (Some("verbose"), 4));
    }

    /// Every form of a Vim command or an own command that the parser reads
    /// as a command is found in the table as a command the parser reads the
    /// same way, and all the forms the parser reads as one command are one
    /// row, so the splitter ends each by the row the parser runs.
    #[test]
    fn the_table_finds_the_command_the_parser_reads() {
        use super::super::parse_ex_command;
        use super::super::types::ExCommand;

        // The names in the parser's source, so that one the table lacks is
        // tried too.
        let parser_words = include_str!("ex_parser.rs")
            .split('"')
            .filter(|word| !word.is_empty() && word.bytes().all(|b| b.is_ascii_lowercase()));
        let forms: std::collections::BTreeSet<&str> = COMMANDS
            .iter()
            .map(|row| row.name)
            .chain(OWN_COMMANDS.iter().map(|own| own.full))
            .chain(PARSER_FORMS.iter().map(|&(form, _)| form))
            .chain(parser_words)
            .flat_map(|name| (1..=name.len()).map(move |len| &name[..len]))
            .collect();
        let mut wrong = Vec::new();
        let mut read_forms = Vec::new();
        for typed in forms {
            let read = parse_ex_command(typed);
            if matches!(read, Ok(ExCommand::Custom { .. })) {
                continue;
            }
            let full = match find_command(typed) {
                Some((FoundCommand::Vim(row), len)) if len == typed.len() => row.name,
                Some((FoundCommand::Own(own), len)) if len == typed.len() => own.full,
                // `:ke` is `:k e`, `:sg` is `:s g` and `:dl` is `:d l`.
                Some(_) => continue,
                // `:+`, `:-` and `:*` are ranges.
                None if !typed.starts_with(|c: char| c.is_ascii_alphabetic()) => continue,
                None => {
                    wrong.push(format!(":{typed} is no command in the table"));
                    continue;
                }
            };
            if parse_ex_command(full) != read {
                wrong.push(format!(":{typed} is {full} in the table"));
            }
            read_forms.push((typed, read, full));
        }
        // Many commands give the same error without an argument.
        read_forms.retain(|(_, read, _)| read.is_ok());
        for (typed, read, full) in &read_forms {
            for (longer, longer_read, longer_full) in &read_forms {
                if longer.len() > typed.len()
                    && longer.starts_with(typed)
                    && longer_read == read
                    && longer_full != full
                {
                    wrong.push(format!(":{typed} is {full} but :{longer} is {longer_full}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn no_command_is_found_for_these() {
        for typed in ["", "Foo", "*", "{", "\u{a0}set", "1"] {
            assert!(find_command(typed).is_none(), "{typed:?}");
        }
    }

    // ── meta_for_command ───────────────────────────────────────────────

    #[test]
    fn meta_delete_has_range_regstr_trlbar_modify() {
        let meta = meta_for_command("d").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::REGSTR));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
        assert_eq!(meta.addr_type, AddrType::Lines);
    }

    #[test]
    fn meta_delete_full_name() {
        let meta = meta_for_command("delete").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
    }

    #[test]
    fn meta_delete_partial_abbreviation() {
        let meta = meta_for_command("del").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
    }

    #[test]
    fn meta_yank_has_range_regstr_trlbar() {
        let meta = meta_for_command("y").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::REGSTR));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(!meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_put_has_range_bang_regstr_trlbar_modify() {
        let meta = meta_for_command("put").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::REGSTR));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_set_has_bang_trlbar_extra() {
        let meta = meta_for_command("set").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.allows_trailing_comment());
    }

    #[test]
    fn meta_has_vim_s_ctrlv_xfile_expr_arg_and_sboxok_flags() {
        let flags = |name| meta_for_command(name).unwrap().flags;
        assert!(flags("nn").contains(CmdFlags::CTRLV));
        assert!(flags("norm").contains(CmdFlags::CTRLV));
        assert!(!flags("se").contains(CmdFlags::CTRLV));
        assert!(flags("e").contains(CmdFlags::XFILE));
        assert!(flags("so").contains(CmdFlags::XFILE));
        assert!(!flags("se").contains(CmdFlags::XFILE));
        assert!(flags("let").contains(CmdFlags::EXPR_ARG));
        assert!(flags("ec").contains(CmdFlags::EXPR_ARG));
        assert!(!flags("wh").contains(CmdFlags::EXPR_ARG));
        assert!(flags("se").contains(CmdFlags::SBOXOK));
        assert!(flags("wh").contains(CmdFlags::SBOXOK));
        assert!(!flags("w").contains(CmdFlags::SBOXOK));
    }

    #[test]
    fn meta_set_abbreviation() {
        let meta = meta_for_command("se").unwrap();
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_substitute_no_trlbar() {
        let meta = meta_for_command("substitute").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(!meta.flags.contains(CmdFlags::TRLBAR));
        assert!(!meta.allows_trailing_comment());
    }

    #[test]
    fn meta_echo_notrlcom() {
        let meta = meta_for_command("echo").unwrap();
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
        assert!(!meta.allows_trailing_comment());
    }

    #[test]
    fn meta_echo_abbreviation() {
        let meta = meta_for_command("ec").unwrap();
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
    }

    #[test]
    fn meta_global_has_range_bang_extra_dflall() {
        let meta = meta_for_command("global").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::DFLALL));
    }

    #[test]
    fn meta_normal_has_range_bang_extra_needarg_notrlcom() {
        let meta = meta_for_command("normal").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::NEEDARG));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
        assert!(!meta.allows_trailing_comment());
    }

    #[test]
    fn meta_normal_abbreviation() {
        let meta = meta_for_command("norm").unwrap();
        assert!(meta.flags.contains(CmdFlags::NEEDARG));
    }

    #[test]
    fn meta_map_has_bang_extra_trlbar_notrlcom() {
        let meta = meta_for_command("map").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
    }

    #[test]
    fn meta_nmap_has_extra_trlbar_notrlcom() {
        let meta = meta_for_command("nmap").unwrap();
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
        assert!(!meta.flags.contains(CmdFlags::BANG));
    }

    #[test]
    fn meta_write_has_range_bang_trlbar() {
        let meta = meta_for_command("write").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_write_abbreviation() {
        let meta = meta_for_command("w").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
    }

    #[test]
    fn meta_quit_has_bang_trlbar() {
        let meta = meta_for_command("quit").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert_eq!(meta.addr_type, AddrType::Windows);
    }

    #[test]
    fn meta_quit_abbreviation() {
        let meta = meta_for_command("q").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
    }

    #[test]
    fn meta_edit_has_bang_extra_trlbar() {
        let meta = meta_for_command("edit").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_sort_has_range_dflall_bang_extra_notrlcom_modify() {
        let meta = meta_for_command("sort").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::DFLALL));
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_join_has_bang_range_count_trlbar_modify() {
        let meta = meta_for_command("join").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::COUNT));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_move_has_range_extra_trlbar_modify() {
        let meta = meta_for_command("move").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_copy_has_range_extra_trlbar_modify() {
        let meta = meta_for_command("copy").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_copy_t_alias() {
        let meta = meta_for_command("t").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_nohlsearch() {
        let meta = meta_for_command("noh").unwrap();
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.allows_trailing_comment());
    }

    #[test]
    fn meta_split() {
        let meta = meta_for_command("split").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_vsplit() {
        let meta = meta_for_command("vsplit").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_close() {
        let meta = meta_for_command("close").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert_eq!(meta.addr_type, AddrType::Windows);
    }

    #[test]
    fn meta_only() {
        let meta = meta_for_command("only").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert_eq!(meta.addr_type, AddrType::Windows);
    }

    #[test]
    fn meta_wqall() {
        let meta = meta_for_command("wqall").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_xall() {
        let meta = meta_for_command("xall").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_qall() {
        let meta = meta_for_command("qall").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_wall() {
        let meta = meta_for_command("wall").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_unknown_returns_none() {
        assert!(meta_for_command("nonexistentcommand").is_none());
    }

    #[test]
    fn meta_case_insensitive() {
        let lower = meta_for_command("delete").unwrap();
        let upper = meta_for_command("DELETE").unwrap();
        assert_eq!(lower.flags, upper.flags);
    }

    // ── allows_trailing_comment ────────────────────────────────────────

    #[test]
    fn allows_trailing_comment_trlbar_only() {
        // :quit has TRLBAR but not NOTRLCOM → comments allowed.
        let meta = meta_for_command("quit").unwrap();
        assert!(meta.allows_trailing_comment());
    }

    #[test]
    fn disallows_trailing_comment_notrlcom() {
        // :map has both TRLBAR and NOTRLCOM → comments NOT allowed.
        let meta = meta_for_command("map").unwrap();
        assert!(!meta.allows_trailing_comment());
    }

    #[test]
    fn disallows_trailing_comment_no_trlbar() {
        // :echo has NOTRLCOM without TRLBAR → no comments.
        let meta = meta_for_command("echo").unwrap();
        assert!(!meta.allows_trailing_comment());
    }

    // ── allows_bar ─────────────────────────────────────────────────────

    #[test]
    fn allows_bar_trlbar_command() {
        let meta = meta_for_command("quit").unwrap();
        assert!(meta.allows_bar());
    }

    #[test]
    fn disallows_bar_no_trlbar() {
        let meta = meta_for_command("substitute").unwrap();
        assert!(!meta.allows_bar());
    }

    // ── Iterator commands ──────────────────────────────────────────────

    #[test]
    fn meta_windo() {
        let meta = meta_for_command("windo").unwrap();
        assert!(meta.flags.contains(CmdFlags::NEEDARG));
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
        assert_eq!(meta.addr_type, AddrType::Windows);
    }

    #[test]
    fn meta_bufdo() {
        let meta = meta_for_command("bufdo").unwrap();
        assert!(meta.flags.contains(CmdFlags::NEEDARG));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
        assert_eq!(meta.addr_type, AddrType::Buffers);
    }

    #[test]
    fn meta_tabdo() {
        let meta = meta_for_command("tabdo").unwrap();
        assert!(meta.flags.contains(CmdFlags::NEEDARG));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
        assert_eq!(meta.addr_type, AddrType::Tabs);
    }

    // ── Fold commands ──────────────────────────────────────────────────

    #[test]
    fn meta_fold() {
        let meta = meta_for_command("fold").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_foldopen() {
        let meta = meta_for_command("foldopen").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_foldclose() {
        let meta = meta_for_command("foldclose").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    // ── Buffer/Tab navigation ──────────────────────────────────────────

    #[test]
    fn meta_buffer() {
        let meta = meta_for_command("buffer").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::COUNT));
        assert_eq!(meta.addr_type, AddrType::Buffers);
    }

    #[test]
    fn meta_bnext() {
        let meta = meta_for_command("bnext").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::COUNT));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_bdelete() {
        let meta = meta_for_command("bdelete").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert_eq!(meta.addr_type, AddrType::Buffers);
    }

    #[test]
    fn meta_tabnew() {
        let meta = meta_for_command("tabnew").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert_eq!(meta.addr_type, AddrType::Tabs);
    }

    #[test]
    fn meta_tabclose() {
        let meta = meta_for_command("tabclose").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert_eq!(meta.addr_type, AddrType::Tabs);
    }

    // ── Diagnostic navigation ──────────────────────────────────────────

    #[test]
    fn meta_cnext() {
        let meta = meta_for_command("cnext").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::COUNT));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert_eq!(meta.addr_type, AddrType::Unsigned);
    }

    #[test]
    fn meta_cprevious() {
        let meta = meta_for_command("cprevious").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::COUNT));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    // ── Undo ───────────────────────────────────────────────────────────

    #[test]
    fn meta_undo() {
        let meta = meta_for_command("undo").unwrap();
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::COUNT));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_redo() {
        let meta = meta_for_command("redo").unwrap();
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_undojoin() {
        let meta = meta_for_command("undojoin").unwrap();
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    // ── Retab / Alignment ──────────────────────────────────────────────

    #[test]
    fn meta_retab() {
        let meta = meta_for_command("retab").unwrap();
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::DFLALL));
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_left() {
        let meta = meta_for_command("left").unwrap();
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    #[test]
    fn meta_center() {
        let meta = meta_for_command("center").unwrap();
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }

    // ── Let / Source / Read ────────────────────────────────────────────

    #[test]
    fn meta_let_notrlcom() {
        let meta = meta_for_command("let").unwrap();
        assert!(meta.flags.contains(CmdFlags::EXTRA));
        assert!(meta.flags.contains(CmdFlags::NOTRLCOM));
    }

    #[test]
    fn meta_source() {
        let meta = meta_for_command("source").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
    }

    #[test]
    fn meta_read() {
        let meta = meta_for_command("read").unwrap();
        assert!(meta.flags.contains(CmdFlags::BANG));
        assert!(meta.flags.contains(CmdFlags::RANGE));
        assert!(meta.flags.contains(CmdFlags::TRLBAR));
        assert!(meta.flags.contains(CmdFlags::MODIFY));
    }
}
