//! Per-command metadata table for ex commands.
//!
//! Provides `CmdFlags` bitflags and `CommandMeta` structs that describe
//! each ex command's capabilities: whether it accepts a range, bang,
//! register argument, trailing bar separator, etc.
//!
//! This metadata drives:
//! - **Trailing comment stripping** — commands with `TRLBAR` (and without
//!   `NOTRLCOM`) allow `" comment` after the command arguments.
//! - **Pipeline splitting** — only `TRLBAR` commands split at `|`.
//! - **Range/bang validation** — callers can check before parsing.
//!
//! Flag values and addr types are derived from Vim's `ex_cmds.h`.
//!
//! # Layering
//!
//! Grammar-only module: it imports nothing from `execution`, `effects` or
//! `commands`.

use bitflags::bitflags;

bitflags! {
    /// Per-command capability flags, derived from Vim's `EX_*` defines in `ex_cmds.h`.
    ///
    /// Only the flags relevant to this engine are included. Vim-internal flags
    /// like `EX_SBOXOK`, `EX_CMDWIN`, `EX_LOCK_OK`, `EX_EXPAND` are omitted
    /// because they govern sandbox/cmdline-window policy that this engine does
    /// not implement.
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

// ─── Abbreviation matching (reused from ex_parser) ────────────────────────

/// Check if `name` matches the abbreviation range from `min` to `full` (case-insensitive).
///
/// Implements Vim's bracket abbreviation notation: `d[elete]` means `min="d"`, `full="delete"`.
/// Any prefix of `full` that is at least `min.len()` characters long matches.
#[inline]
fn matches_abbrev(name: &str, min: &str, full: &str) -> bool {
    let n = name.len();
    n >= min.len() && n <= full.len() && name.eq_ignore_ascii_case(&full[..n])
}

/// Look up metadata for a named ex command.
///
/// Accepts any valid abbreviation of the command name (case-insensitive),
/// matching the same abbreviation rules used by `ex_parser::parse_named_command`.
///
/// Returns `None` for unknown commands (would fall through to `ExCommand::Custom`).
///
/// # Examples
///
/// ```ignore
/// use vim_core::grammar::command_meta::meta_for_command;
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
    // ── Text manipulation ──────────────────────────────────────────────

    if matches_abbrev(name, "d", "delete") {
        // :delete — EX_RANGE|EX_WHOLEFOLD|EX_REGSTR|EX_COUNT|EX_TRLBAR|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::REGSTR)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "y", "yank") {
        // :yank — EX_RANGE|EX_WHOLEFOLD|EX_REGSTR|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::REGSTR)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "m", "move") {
        // :move — EX_RANGE|EX_WHOLEFOLD|EX_EXTRA|EX_TRLBAR|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if name.eq_ignore_ascii_case("t") || matches_abbrev(name, "co", "copy") {
        // :copy/:t — EX_RANGE|EX_WHOLEFOLD|EX_EXTRA|EX_TRLBAR|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "j", "join") {
        // :join — EX_BANG|EX_RANGE|EX_WHOLEFOLD|EX_COUNT|EX_TRLBAR|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "sor", "sort") {
        // :sort — EX_RANGE|EX_DFLALL|EX_WHOLEFOLD|EX_BANG|EX_EXTRA|EX_NOTRLCOM|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::DFLALL)
                .union(CmdFlags::BANG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::NOTRLCOM)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "pu", "put") {
        // :put — EX_RANGE|EX_WHOLEFOLD|EX_BANG|EX_REGSTR|EX_TRLBAR|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::BANG)
                .union(CmdFlags::REGSTR)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "ret", "retab") {
        // :retab — EX_TRLBAR|EX_RANGE|EX_WHOLEFOLD|EX_DFLALL|EX_BANG|EX_EXTRA|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::TRLBAR
                .union(CmdFlags::RANGE)
                .union(CmdFlags::DFLALL)
                .union(CmdFlags::BANG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "le", "left") {
        // :left — EX_TRLBAR|EX_RANGE|EX_WHOLEFOLD|EX_EXTRA|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::TRLBAR
                .union(CmdFlags::RANGE)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "ri", "right") {
        // :right — EX_TRLBAR|EX_RANGE|EX_WHOLEFOLD|EX_EXTRA|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::TRLBAR
                .union(CmdFlags::RANGE)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "ce", "center") || matches_abbrev(name, "ce", "centre") {
        // :center — EX_TRLBAR|EX_RANGE|EX_WHOLEFOLD|EX_EXTRA|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::TRLBAR
                .union(CmdFlags::RANGE)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }

    // ── Substitute / Global ────────────────────────────────────────────

    // Note: :substitute and :global are typically matched by delimiter before
    // reaching parse_named_command, but we include them for completeness.
    if name.eq_ignore_ascii_case("substitute") {
        // :substitute — EX_RANGE|EX_WHOLEFOLD|EX_EXTRA (no TRLBAR, no NOTRLCOM in Vim)
        // Practically: does NOT split at |, | inside pattern is literal.
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if name.eq_ignore_ascii_case("global") {
        // :global — EX_RANGE|EX_WHOLEFOLD|EX_BANG|EX_EXTRA|EX_DFLALL
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::BANG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::DFLALL)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }

    // ── Informational ──────────────────────────────────────────────────

    if matches_abbrev(name, "noh", "nohlsearch") {
        // :nohlsearch — EX_TRLBAR
        return Some(CommandMeta::new(CmdFlags::TRLBAR, AddrType::None));
    }
    if matches_abbrev(name, "reg", "registers") || matches_abbrev(name, "di", "display") {
        // :registers/:display — EX_EXTRA|EX_NOTRLCOM|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::NOTRLCOM)
                .union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "mar", "marks") {
        // :marks — EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "ju", "jumps") {
        // :jumps — EX_TRLBAR
        return Some(CommandMeta::new(CmdFlags::TRLBAR, AddrType::None));
    }
    if matches_abbrev(name, "cha", "changes") {
        // :changes — EX_TRLBAR
        return Some(CommandMeta::new(CmdFlags::TRLBAR, AddrType::None));
    }
    if matches_abbrev(name, "clearj", "clearjumps") {
        return Some(CommandMeta::new(CmdFlags::TRLBAR, AddrType::None));
    }
    if matches_abbrev(name, "mes", "messages") {
        // :messages — EX_EXTRA|EX_TRLBAR|EX_RANGE
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::RANGE),
            AddrType::Other,
        ));
    }

    // ── Abbreviation commands (must precede mapping commands) ──────────

    if matches_abbrev(name, "ab", "abbreviate") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "ia", "iabbrev") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "ca", "cabbrev") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "norea", "noreabbrev") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "inorea", "inoreabbrev") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "cnorea", "cnoreabbrev") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "una", "unabbreviate") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "iuna", "iunabbrev") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "cuna", "cunabbrev") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "abc", "abclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "iabc", "iabclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "cabc", "cabclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }

    // ── Mapping commands (recursive) ───────────────────────────────────

    if name.eq_ignore_ascii_case("map") {
        // :map — EX_BANG|EX_EXTRA|EX_TRLBAR|EX_NOTRLCOM
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "nm", "nmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "vm", "vmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "im", "imap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "om", "omap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "cm", "cmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }

    // ── Mapping commands (non-recursive) ───────────────────────────────

    if matches_abbrev(name, "no", "noremap") {
        // :noremap — EX_BANG|EX_EXTRA|EX_TRLBAR|EX_NOTRLCOM
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "nn", "nnoremap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "vn", "vnoremap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "ino", "inoremap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "ono", "onoremap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "cno", "cnoremap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }

    // ── Unmap commands ─────────────────────────────────────────────────

    if matches_abbrev(name, "unm", "unmap") {
        // :unmap — EX_BANG|EX_EXTRA|EX_TRLBAR|EX_NOTRLCOM
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "nun", "nunmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "vu", "vunmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "iu", "iunmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "ou", "ounmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "cu", "cunmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }

    // ── xmap/xnoremap/xunmap (visual-only) ────────────────────────────

    if matches_abbrev(name, "xm", "xmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "xn", "xnoremap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "xu", "xunmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }

    // ── smap/snoremap/sunmap (select-only) ────────────────────────────

    if matches_abbrev(name, "sn", "snoremap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "su", "sunmap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "sm", "smap") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }

    // ── mapclear commands ─────────────────────────────────────────────

    if name.eq_ignore_ascii_case("mapclear") {
        // :mapclear — EX_BANG|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "nmapc", "nmapclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "vmapc", "vmapclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "imapc", "imapclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "omapc", "omapclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "cmapc", "cmapclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "xmapc", "xmapclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "smapc", "smapclear") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }

    // ── Options (:set variants) — specific prefixes first ──────────────

    if matches_abbrev(name, "seth", "sethandler") {
        // :sethandler uses same flags as :set
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::EXTRA),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "setl", "setlocal") {
        // :setlocal — EX_BANG|EX_TRLBAR|EX_EXTRA
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::EXTRA),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "setg", "setglobal") {
        // :setglobal — EX_BANG|EX_TRLBAR|EX_EXTRA
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::EXTRA),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "se", "set") {
        // :set — EX_BANG|EX_TRLBAR|EX_EXTRA
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::EXTRA),
            AddrType::None,
        ));
    }

    // ── File / Session commands ─────────────────────────────────────────

    if matches_abbrev(name, "w", "write") {
        // :write — EX_RANGE|EX_WHOLEFOLD|EX_BANG|EX_DFLALL|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::BANG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::DFLALL)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "q", "quit") {
        // :quit — EX_BANG|EX_RANGE|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Windows,
        ));
    }
    if name.eq_ignore_ascii_case("wq") || matches_abbrev(name, "x", "xit") {
        // :wq/:xit — EX_RANGE|EX_WHOLEFOLD|EX_BANG|EX_DFLALL|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::BANG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::DFLALL)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "e", "edit") {
        // :edit — EX_BANG|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "r", "read") {
        // :read — EX_BANG|EX_RANGE|EX_EXTRA|EX_TRLBAR|EX_MODIFY
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::MODIFY),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "up", "update") {
        // :update — EX_RANGE|EX_WHOLEFOLD|EX_BANG|EX_DFLALL|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::BANG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::DFLALL)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "so", "source") {
        // :source — EX_RANGE|EX_DFLALL|EX_BANG|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::DFLALL)
                .union(CmdFlags::BANG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }

    // ── Normal mode ────────────────────────────────────────────────────

    if matches_abbrev(name, "norm", "normal") {
        // :normal — EX_RANGE|EX_BANG|EX_EXTRA|EX_NEEDARG|EX_NOTRLCOM
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::BANG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::NEEDARG)
                .union(CmdFlags::NOTRLCOM),
            AddrType::Lines,
        ));
    }

    // ── Display / Misc ─────────────────────────────────────────────────

    if matches_abbrev(name, "ec", "echo") {
        // :echo — EX_EXTRA|EX_NOTRLCOM
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if name.eq_ignore_ascii_case("let") {
        // :let — EX_EXTRA|EX_NOTRLCOM
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::NOTRLCOM),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "p", "print") {
        // :print — EX_RANGE|EX_WHOLEFOLD|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "nu", "number") || name == "#" {
        // :number/:#  — EX_RANGE|EX_WHOLEFOLD|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "l", "list") {
        // :list — EX_RANGE|EX_WHOLEFOLD|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }

    // ── Action commands ────────────────────────────────────────────────

    if name.eq_ignore_ascii_case("action") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA
                .union(CmdFlags::NEEDARG)
                .union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "actionl", "actionlist") {
        return Some(CommandMeta::new(
            CmdFlags::EXTRA.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }

    // ── Undo / Redo ────────────────────────────────────────────────────

    if matches_abbrev(name, "ea", "earlier") {
        // :earlier — EX_TRLBAR|EX_EXTRA
        return Some(CommandMeta::new(
            CmdFlags::TRLBAR.union(CmdFlags::EXTRA),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "lat", "later") {
        // :later — EX_TRLBAR|EX_EXTRA
        return Some(CommandMeta::new(
            CmdFlags::TRLBAR.union(CmdFlags::EXTRA),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "undol", "undolist") {
        return Some(CommandMeta::new(CmdFlags::TRLBAR, AddrType::None));
    }
    if matches_abbrev(name, "undot", "undotree") {
        return Some(CommandMeta::new(CmdFlags::TRLBAR, AddrType::None));
    }
    if matches_abbrev(name, "red", "redo") {
        // :redo — EX_TRLBAR
        return Some(CommandMeta::new(CmdFlags::TRLBAR, AddrType::None));
    }
    if name.eq_ignore_ascii_case("undo") || name.eq_ignore_ascii_case("u") {
        // :undo — EX_RANGE|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }
    if matches_abbrev(name, "undoj", "undojoin") {
        // :undojoin — EX_TRLBAR
        return Some(CommandMeta::new(CmdFlags::TRLBAR, AddrType::None));
    }

    // ── Buffer navigation ──────────────────────────────────────────────

    if matches_abbrev(name, "b", "buffer") {
        // :buffer — EX_BANG|EX_RANGE|EX_COUNT|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Buffers,
        ));
    }
    if matches_abbrev(name, "bn", "bnext") {
        // :bnext — EX_BANG|EX_RANGE|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }
    if matches_abbrev(name, "bp", "bprevious") {
        // :bprevious — EX_BANG|EX_RANGE|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }
    if matches_abbrev(name, "bf", "bfirst") || matches_abbrev(name, "bre", "brewind") {
        // :bfirst — EX_BANG|EX_RANGE|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }
    if matches_abbrev(name, "bl", "blast") {
        // :blast — EX_BANG|EX_RANGE|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }
    if name.eq_ignore_ascii_case("ls")
        || matches_abbrev(name, "buffers", "buffers")
        || matches_abbrev(name, "files", "files")
    {
        // :buffers/:ls — EX_BANG|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "bd", "bdelete") {
        // :bdelete — EX_BANG|EX_RANGE|EX_COUNT|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Buffers,
        ));
    }
    if matches_abbrev(name, "bw", "bwipeout") {
        // :bwipeout — EX_BANG|EX_RANGE|EX_COUNT|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Buffers,
        ));
    }

    // ── Tab navigation ─────────────────────────────────────────────────

    if name.eq_ignore_ascii_case("tabnew") || matches_abbrev(name, "tabe", "tabedit") {
        // :tabnew — EX_BANG|EX_RANGE|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Tabs,
        ));
    }
    if matches_abbrev(name, "tabn", "tabnext") {
        // :tabnext — EX_RANGE|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Tabs,
        ));
    }
    if matches_abbrev(name, "tabp", "tabprevious") {
        // :tabprevious — EX_RANGE|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Tabs,
        ));
    }
    if matches_abbrev(name, "tabc", "tabclose") {
        // :tabclose — EX_BANG|EX_RANGE|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Tabs,
        ));
    }

    // ── Window commands ────────────────────────────────────────────────

    if matches_abbrev(name, "sp", "split") {
        // :split — EX_BANG|EX_RANGE|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }
    if matches_abbrev(name, "vs", "vsplit") {
        // :vsplit — EX_BANG|EX_RANGE|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }

    // ── Diagnostic navigation (before :close to avoid prefix clash) ───

    if matches_abbrev(name, "cn", "cnext") {
        // :cnext — EX_RANGE|EX_COUNT|EX_TRLBAR|EX_BANG
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::BANG),
            AddrType::Unsigned,
        ));
    }
    if matches_abbrev(name, "cp", "cprevious") || matches_abbrev(name, "cp", "cprev") {
        // :cprevious — EX_RANGE|EX_COUNT|EX_TRLBAR|EX_BANG
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::BANG),
            AddrType::Unsigned,
        ));
    }
    if matches_abbrev(name, "cl", "clist") {
        // :clist — EX_BANG|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if name.eq_ignore_ascii_case("cc") {
        // :cc — EX_RANGE|EX_COUNT|EX_TRLBAR|EX_BANG
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::BANG),
            AddrType::Unsigned,
        ));
    }

    if matches_abbrev(name, "clo", "close") {
        // :close — EX_BANG|EX_RANGE|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Windows,
        ));
    }
    if name.eq_ignore_ascii_case("new") {
        // :new — EX_BANG|EX_EXTRA|EX_RANGE|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::RANGE)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }
    if matches_abbrev(name, "vne", "vnew") {
        // :vnew — EX_BANG|EX_EXTRA|EX_RANGE|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::RANGE)
                .union(CmdFlags::TRLBAR),
            AddrType::Other,
        ));
    }
    if matches_abbrev(name, "on", "only") {
        // :only — EX_BANG|EX_RANGE|EX_COUNT|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::RANGE)
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR),
            AddrType::Windows,
        ));
    }
    if matches_abbrev(name, "wa", "wall") {
        // :wall — EX_BANG|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "qa", "qall") {
        // :qall — EX_BANG|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }
    if matches_abbrev(name, "wqa", "wqall") || matches_abbrev(name, "xa", "xall") {
        // :wqall/:xall — EX_BANG|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG.union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }

    // ── Delete marks ───────────────────────────────────────────────────

    if matches_abbrev(name, "delm", "delmarks") {
        // :delmarks — EX_BANG|EX_EXTRA|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::TRLBAR),
            AddrType::None,
        ));
    }

    // ── Quit with error code ───────────────────────────────────────────

    if matches_abbrev(name, "cq", "cquit") {
        // :cquit — EX_RANGE|EX_COUNT|EX_TRLBAR|EX_BANG
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::COUNT)
                .union(CmdFlags::TRLBAR)
                .union(CmdFlags::BANG),
            AddrType::Unsigned,
        ));
    }

    // ── Fold commands ──────────────────────────────────────────────────

    if matches_abbrev(name, "fo", "fold") {
        // :fold — EX_RANGE|EX_WHOLEFOLD|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE.union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "foldo", "foldopen") {
        // :foldopen — EX_RANGE|EX_BANG|EX_WHOLEFOLD|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::BANG)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }
    if matches_abbrev(name, "foldc", "foldclose") {
        // :foldclose — EX_RANGE|EX_BANG|EX_WHOLEFOLD|EX_TRLBAR
        return Some(CommandMeta::new(
            CmdFlags::RANGE
                .union(CmdFlags::BANG)
                .union(CmdFlags::TRLBAR),
            AddrType::Lines,
        ));
    }

    // ── Iterator commands ──────────────────────────────────────────────

    if matches_abbrev(name, "windo", "windo") {
        // :windo — EX_NEEDARG|EX_EXTRA|EX_NOTRLCOM|EX_RANGE|EX_DFLALL
        return Some(CommandMeta::new(
            CmdFlags::NEEDARG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::NOTRLCOM)
                .union(CmdFlags::RANGE)
                .union(CmdFlags::DFLALL),
            AddrType::Windows,
        ));
    }
    if matches_abbrev(name, "bufdo", "bufdo") {
        // :bufdo — EX_BANG|EX_NEEDARG|EX_EXTRA|EX_NOTRLCOM|EX_RANGE|EX_DFLALL
        return Some(CommandMeta::new(
            CmdFlags::BANG
                .union(CmdFlags::NEEDARG)
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::NOTRLCOM)
                .union(CmdFlags::RANGE)
                .union(CmdFlags::DFLALL),
            AddrType::Buffers,
        ));
    }
    if matches_abbrev(name, "tabdo", "tabdo") {
        // :tabdo — EX_NEEDARG|EX_EXTRA|EX_NOTRLCOM|EX_RANGE|EX_DFLALL
        return Some(CommandMeta::new(
            CmdFlags::NEEDARG
                .union(CmdFlags::EXTRA)
                .union(CmdFlags::NOTRLCOM)
                .union(CmdFlags::RANGE)
                .union(CmdFlags::DFLALL),
            AddrType::Tabs,
        ));
    }

    // ── No match — unknown command ─────────────────────────────────────
    None
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
