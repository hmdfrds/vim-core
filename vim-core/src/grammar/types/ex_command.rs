//! Ex command types.
//!
//! Parsed Ex command representations for `:s`, `:g`, `:d`, etc.
//!
//! Per EBNF:
//! ```text
//! ex_command  = [range] cmd_name [args]
//! substitute  = [range] "s" "/" pattern "/" replacement "/" [sub_flags]
//! global_cmd  = [range] ("g" | "v") "/" pattern "/" cmd_name
//! ```

use compact_str::CompactString;

use super::ex_range::ExRange;
use crate::primitives::{AbbrevMode, SubFlags};

/// Sort command options.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(
    clippy::struct_excessive_bools,
    reason = "options are inherently boolean"
)]
pub struct SortOptions {
    /// Numeric sort (`n`).
    numeric: bool,
    /// Ignore case (`i`).
    ignore_case: bool,
    /// Unique lines only (`u`).
    unique: bool,
    /// Reverse order (`!` or `r`).
    reverse: bool,
    /// Optional pattern: sort by text after the pattern match (`:sort /pat/`).
    pattern: Option<String>,
}

impl SortOptions {
    /// Parse options from a string.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        let mut opts = Self::default();
        let s = s.trim();

        // Check for pattern: /pattern/ at the beginning (after optional flags)
        let remaining = Self::parse_pattern(s, &mut opts);

        for c in remaining.chars() {
            match c {
                'n' => opts.numeric = true,
                'i' => opts.ignore_case = true,
                'u' => opts.unique = true,
                'r' | '!' => opts.reverse = true,
                _ => {}
            }
        }
        opts
    }

    /// Extract pattern from sort options string if present.
    /// Parses flags from both before and after the pattern, returns `""` so
    /// the outer loop has nothing left to re-parse.
    fn parse_pattern<'a>(s: &'a str, opts: &mut Self) -> &'a str {
        // Pattern can appear after flags: `:sort i /pat/` or `:sort /pat/ i`
        // Look for `/pattern/` anywhere in the string.
        if let Some(start) = s.find('/') {
            if let Some(end) = s[start + 1..].find('/') {
                let pattern = &s[start + 1..start + 1 + end];
                if !pattern.is_empty() {
                    opts.pattern = Some(pattern.to_owned());
                }
                // Parse flags from both sides of the pattern
                let before = &s[..start];
                let after = &s[start + 1 + end + 1..];
                for c in before.chars().chain(after.chars()) {
                    match c {
                        'n' => opts.numeric = true,
                        'i' => opts.ignore_case = true,
                        'u' => opts.unique = true,
                        'r' | '!' => opts.reverse = true,
                        _ => {}
                    }
                }
                return "";
            }
        }
        s
    }

    /// Parse options with reverse flag (`:sort!`).
    #[must_use]
    pub fn parse_reversed(s: &str) -> Self {
        let mut opts = Self::parse(s);
        opts.reverse = true;
        opts
    }

    /// Numeric sort (`n` flag).
    #[must_use]
    pub const fn numeric(&self) -> bool {
        self.numeric
    }
    /// Ignore case (`i` flag).
    #[must_use]
    pub const fn ignore_case(&self) -> bool {
        self.ignore_case
    }
    /// Unique lines only (`u` flag).
    #[must_use]
    pub const fn unique(&self) -> bool {
        self.unique
    }
    /// Reverse order (`!` or `r` flag).
    #[must_use]
    pub const fn reverse(&self) -> bool {
        self.reverse
    }
    /// Optional pattern for sort key extraction.
    #[must_use]
    pub fn pattern(&self) -> Option<&str> {
        self.pattern.as_deref()
    }
}

/// Display style for the `:z` window command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ZWindowStyle {
    /// `:z` or `:z+` — show lines below the target.
    Below,
    /// `:z-` — show lines above the target.
    Above,
    /// `:z.` — center the target line in the window.
    Centered,
    /// `:z=` — center the target line and highlight it with dashes.
    Highlighted,
    /// `:z^` — show the window before the previous `:z` window.
    PrevWindow,
    /// `:z#` — show lines below with line numbers.
    Numbered,
}

/// Parsed Ex command.
///
/// Each variant represents a fully parsed Ex command ready for execution.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExCommand {
    // === Text manipulation (core handles) ===
    /// Substitute: `:s/pattern/replacement/flags`
    Substitute {
        /// Line range to substitute within.
        range: ExRange,
        /// Regex pattern to search for.
        pattern: CompactString,
        /// Replacement text.
        replacement: CompactString,
        /// Substitute flags (g, c, i, I, n).
        flags: SubFlags,
    },
    /// Global: `:g/pattern/command`
    Global {
        /// Line range to search within.
        range: ExRange,
        /// Pattern to match lines against.
        pattern: CompactString,
        /// Command to execute on matching lines.
        command: Box<Self>,
        /// If true, this is `:v` (inverse match).
        invert: bool,
    },
    /// Delete lines: `:d` or `:5,10d`
    Delete {
        /// Range of lines to delete.
        range: ExRange,
        /// Target register for deleted text.
        register: Option<crate::primitives::RegisterName>,
    },
    /// Yank lines: `:y` or `:5,10y`
    Yank {
        /// Range of lines to yank.
        range: ExRange,
        /// Target register for yanked text.
        register: Option<crate::primitives::RegisterName>,
    },
    /// Move lines: `:m{address}` or `:5,10m$`
    Move {
        /// Range of lines to move.
        range: ExRange,
        /// Destination line address.
        target: super::ex_range::LineSpec,
    },
    /// Copy lines: `:t{address}` or `:co{address}`
    Copy {
        /// Range of lines to copy.
        range: ExRange,
        /// Destination line address.
        target: super::ex_range::LineSpec,
    },
    /// Join lines: `:j` / `:j!`
    Join {
        /// Range of lines to join.
        range: ExRange,
        /// When true (`:j!`), join without inserting spaces.
        bang: bool,
    },
    /// Sort lines: `:sort [options]`
    Sort {
        /// Range of lines to sort.
        range: ExRange,
        /// Sort options (n, i, u, r).
        options: SortOptions,
    },

    // === Informational (read state, display) ===
    /// Show registers: `:reg[isters]` or `:di[splay]`
    Registers {
        /// Optional filter: show only these register names.
        filter: Option<CompactString>,
    },
    /// Show marks: `:marks`
    Marks {
        /// Optional filter: show only these mark names.
        filter: Option<CompactString>,
    },
    /// Show jump list: `:jumps`
    Jumps,
    /// Show change list: `:changes`
    Changes,
    /// Show message history: `:messages [clear]`
    Messages {
        /// If `true`, clear the history instead of displaying it (`:messages clear`).
        clear: bool,
    },

    /// Put register contents: `:put [register]`
    Put {
        /// Line to insert after (or before with `!`).
        range: ExRange,
        /// Register to put from (default: unnamed).
        register: Option<crate::primitives::RegisterName>,
        /// If true (`:put!`), insert before instead of after.
        before: bool,
    },
    /// Replace tabs: `:retab[!] [new_tabstop]`
    Retab {
        /// Range to retab.
        range: ExRange,
        /// New tabstop value (None = use current option).
        new_tabstop: Option<usize>,
        /// If true (`:retab!`), also replace sequences of spaces with tabs.
        to_tabs: bool,
    },
    /// Left-align: `:[range]left [indent]`
    Left {
        /// Range to align.
        range: ExRange,
        /// Indent width (default 0 = flush left).
        indent: usize,
    },
    /// Right-align: `:[range]right [width]`
    Right {
        /// Range to align.
        range: ExRange,
        /// Width to align to (None = use textwidth or 80).
        width: Option<usize>,
    },
    /// Center-align: `:[range]center [width]`
    Center {
        /// Range to align.
        range: ExRange,
        /// Width to center within (None = use textwidth or 80).
        width: Option<usize>,
    },

    /// Define mapping: `:map`, `:nmap`, `:noremap`, `:nnoremap`, etc.
    Map {
        /// Which mode(s) the mapping applies to.
        mode_prefix: MapModePrefix,
        /// Left-hand side key notation (e.g. `<Leader>w`, `jk`).
        lhs: CompactString,
        /// Right-hand side key notation. `None` = list mappings for lhs.
        rhs: Option<CompactString>,
        /// Recursive (`:map`) vs non-recursive (`:noremap`).
        kind: crate::keymap::MappingKind,
        /// Consolidated boolean attribute flags (`<nowait>`, `<silent>`, `<expr>`).
        flags: crate::keymap::MappingFlags,
    },
    /// Remove mapping: `:unmap`, `:nunmap`, etc.
    Unmap {
        /// Which mode(s) the mapping applies to.
        mode_prefix: MapModePrefix,
        /// Left-hand side to remove.
        lhs: CompactString,
    },
    /// Clear all mappings for a mode: `:mapclear`, `:nmapclear`, etc.
    MapClear {
        /// Which mode(s) to clear.
        mode: MapModePrefix,
        /// Whether the command was invoked with `!` (`:mapclear!` clears local mappings).
        force: bool,
    },

    // === UI (core emits effects) ===
    /// Clear search highlights: `:noh[lsearch]`
    NoHighlight,

    // === File operations (shell executes) ===
    /// Write file: `:w [path]`
    Write {
        /// Optional path to write to.
        path: Option<CompactString>,
        /// Force write (`:w!`).
        force: bool,
    },
    /// Quit: `:q[!]`
    Quit {
        /// Force quit without saving (`:q!`).
        force: bool,
    },
    /// Write and quit: `:wq`
    WriteQuit {
        /// Force (`:wq!`).
        force: bool,
    },
    /// Edit file: `:e {path}`
    Edit {
        /// Path to file to edit.
        path: CompactString,
        /// Force open even if host reports unsaved changes.
        force: bool,
    },
    /// Read file: `:r {path}`
    Read {
        /// Path to file to read.
        path: CompactString,
        /// Line to insert after (None = current).
        after_line: Option<u32>,
    },
    /// Filter range through external command: `:[range]!cmd`
    Filter {
        /// Range to filter.
        range: ExRange,
        /// External command.
        command: CompactString,
    },
    /// External shell command: `:!cmd`
    External {
        /// External command.
        command: CompactString,
    },
    /// Split window: `:sp[lit] [path]`
    Split {
        /// Optional path to open in the new split.
        path: Option<CompactString>,
    },
    /// Vertical split: `:vs[plit] [path]`
    VSplit {
        /// Optional path to open in the new vertical split.
        path: Option<CompactString>,
    },
    /// Close current window: `:clo[se][!]`
    Close {
        /// Force close without saving.
        force: bool,
    },
    /// Close all other windows: `:on[ly][!]`
    Only {
        /// Force close without saving.
        force: bool,
    },
    /// Open new empty window: `:new`
    New,
    /// Open new empty vertical window: `:vne[w]`
    VNew,
    /// Write all buffers: `:wa[ll]`
    WriteAll,
    /// Quit all windows: `:qa[ll][!]`
    QuitAll {
        /// Force quit without saving.
        force: bool,
    },
    /// Write all and quit all: `:wqa[ll]` / `:xa[ll]`
    WriteQuitAll,
    /// Delete buffer: `:bd[elete][!] [target]`
    BufferDelete {
        /// Force delete without saving.
        force: bool,
        /// Buffer target (number or name); host resolves.
        target: Option<CompactString>,
    },
    /// Wipeout buffer: `:bw[ipeout][!] [target]`
    BufferWipeout {
        /// Force wipeout without saving.
        force: bool,
        /// Buffer target (number or name); host resolves.
        target: Option<CompactString>,
    },
    /// Define abbreviation: `:abbreviate`, `:iabbrev`, `:cabbrev`, `:noreabbrev`, etc.
    Abbreviate {
        /// Trigger text (None = list abbreviations).
        trigger: Option<CompactString>,
        /// Replacement text (None = show matching abbreviation).
        replacement: Option<CompactString>,
        /// Which mode(s) the abbreviation applies to.
        mode: AbbrevMode,
        /// Whether remapping is suppressed (noremap variant).
        noremap: bool,
    },
    /// Remove abbreviation: `:unabbreviate`, `:iunabbrev`, `:cunabbrev`.
    Unabbreviate {
        /// Trigger text to remove.
        trigger: CompactString,
        /// Which mode(s) to remove from.
        mode: AbbrevMode,
    },
    /// Clear abbreviations: `:abclear`, `:iabclear`, `:cabclear`.
    AbClear {
        /// Which mode(s) to clear.
        mode: AbbrevMode,
    },

    /// Fallback custom ex command forwarded to host.
    Custom {
        /// Raw command text.
        command: CompactString,
    },
    /// Execute a host action by name: `:action {name}`
    Action {
        /// Action name to execute (e.g., "ReformatCode").
        name: CompactString,
    },
    /// List available host actions: `:actionlist [filter]`
    ActionList {
        /// Optional name filter substring.
        filter: Option<CompactString>,
    },
    /// Source a config file: `:source {path}` or `:so {path}`
    Source {
        /// Path to the config file to load.
        path: CompactString,
    },
    /// Navigate earlier in undo history: `:earlier {amount}`
    ///
    /// Amount can be a change count (`:earlier 5`) or time-based
    /// (`:earlier 10s`, `:earlier 5m`, `:earlier 1h`).
    Earlier {
        /// How far back to navigate.
        amount: TimeAmount,
    },
    /// Navigate later in undo history: `:later {amount}`
    Later {
        /// How far forward to navigate.
        amount: TimeAmount,
    },
    /// Display the undo tree leaf nodes: `:undolist`
    UndoList,
    /// Visualize the full undo tree: `:undotree`
    UndoTree,
    /// Jump to a specific undo sequence number: `:undo N`
    ///
    /// Navigates the undo tree to the node with the given sequence number,
    /// regardless of whether it requires undo or redo steps (and branch switches).
    UndoSequence {
        /// Target sequence number (1-based; must match a committed undo node).
        seq: u64,
    },

    // === Diagnostic navigation (host handles) ===
    /// Jump to next diagnostic: `:cn[ext] [count]`
    CNext {
        /// Count of diagnostics to advance.
        count: u32,
    },
    /// Jump to previous diagnostic: `:cp[revious] [count]`
    CPrev {
        /// Count of diagnostics to go back.
        count: u32,
    },
    /// Show diagnostic list: `:cl[ist]`
    CList,
    /// Jump to specific diagnostic by index: `:cc [index]`
    CC {
        /// 1-based diagnostic index (None = current).
        index: Option<u32>,
    },

    // === Buffer navigation (host handles) ===
    /// Switch to buffer by number: `:buffer N` or `:b N`
    Buffer {
        /// Buffer number to switch to.
        number: u32,
    },
    /// Switch to next buffer: `:bnext` or `:bn`
    BufferNext {
        /// Count of buffers to advance.
        count: u32,
    },
    /// Switch to previous buffer: `:bprev` or `:bp`
    BufferPrev {
        /// Count of buffers to go back.
        count: u32,
    },
    /// Switch to first buffer: `:bfirst` or `:bf`
    BufferFirst,
    /// Switch to last buffer: `:blast` or `:bl`
    BufferLast,
    /// List buffers: `:ls` or `:buffers`
    BufferList,

    // === Tab navigation (host handles) ===
    /// Open new tab: `:tabnew [path]`
    TabNew {
        /// Optional path to open in new tab.
        path: Option<CompactString>,
    },
    /// Switch to next tab: `:tabnext` or `:tabn`
    TabNext {
        /// Count of tabs to advance.
        count: u32,
    },
    /// Switch to previous tab: `:tabprev` or `:tabp`
    TabPrev {
        /// Count of tabs to go back.
        count: u32,
    },
    /// Close current tab: `:tabclose` or `:tabc`
    TabClose {
        /// Force close without saving.
        force: bool,
    },

    // === Display / Misc ===
    /// Echo a message: `:echo {msg}`
    Echo {
        /// Message text to display.
        message: CompactString,
    },
    /// Set the mapleader variable: `:let mapleader = "{char}"`
    LetMapleader {
        /// Leader key character.
        leader: char,
    },
    /// Display lines: `:[range]print`, `:[range]number`, `:[range]list`, `:[range]#`
    ///
    /// Combines `:print` (`:p`), `:number` (`:nu`, `:#`), and `:list` (`:l`)
    /// into a single variant. Vim allows combining flags (`:nu l`, `:#l`).
    PrintLines {
        /// Range of lines to display.
        range: ExRange,
        /// Show line numbers (`:number`, `:#`).
        number: bool,
        /// Show control characters and EOL markers (`:list`).
        list: bool,
    },
    /// Window display: `:[line]z[+-=.^#] [count]`
    ZWindow {
        /// Target line address.
        range: ExRange,
        /// Display style modifier.
        style: ZWindowStyle,
        /// Optional window size (number of lines).
        count: Option<u32>,
    },
    /// Normal command: `:[range]norm[al] {keys}`
    Norm {
        /// Range of lines to execute on.
        range: ExRange,
        /// Normal-mode keystrokes to execute on each line.
        keys: CompactString,
        /// Whether mappings should be applied during replay.
        remap: bool,
    },
    /// Goto line: `:{linespec}` (e.g., `:3`, `:$`, `:1`)
    GotoLine {
        /// Line address to jump to.
        range: ExRange,
    },
    /// Structural extract: `:sx/pattern/command` — apply command to each match.
    StructuralExtract {
        /// Regex pattern to match regions.
        pattern: CompactString,
        /// Sub-command to apply to each matched region.
        command: Box<Self>,
        /// Structural regex flags.
        flags: crate::primitives::StructuralFlags,
    },
    /// Structural complement: `:sy/pattern/command` — apply command to gaps between matches.
    StructuralComplement {
        /// Regex pattern whose matches define the boundaries.
        pattern: CompactString,
        /// Sub-command to apply to each gap region.
        command: Box<Self>,
        /// Structural regex flags.
        flags: crate::primitives::StructuralFlags,
    },

    /// Select matches within selections: `:select /pattern/`
    SelectMatches {
        /// Optional line range (unused — operates on selections, not line ranges).
        range: Option<ExRange>,
        /// Regex pattern to match within each selection.
        pattern: String,
    },
    /// Split selections at match boundaries: `:split /pattern/`
    SplitMatches {
        /// Optional line range (unused — operates on selections, not line ranges).
        range: Option<ExRange>,
        /// Regex pattern whose matches define split points.
        pattern: String,
    },
    /// Keep only selections matching pattern: `:keep /pattern/`
    KeepMatches {
        /// Optional line range (unused — operates on selections, not line ranges).
        range: Option<ExRange>,
        /// Regex pattern that selections must match to be kept.
        pattern: String,
    },
    /// Remove selections matching pattern: `:remove /pattern/`
    RemoveMatches {
        /// Optional line range (unused — operates on selections, not line ranges).
        range: Option<ExRange>,
        /// Regex pattern that selections must match to be removed.
        pattern: String,
    },
    /// Trim whitespace from selections: `:trim`
    TrimSelections,
    /// Align selections by inserting padding: `:align`
    AlignSelections,
    /// Rotate text contents between selections: `:rotate`
    RotateContents,
    /// Rotate text contents with explicit direction: `:cursorrotate fwd|bwd`
    RotateContentsDir {
        /// Rotation direction: Forward or Backward.
        direction: crate::primitives::Direction,
    },

    /// Add cursor at next match: `:addnext [count]`
    AddNext {
        /// Repeat count (how many times to add next match).
        count: Option<u32>,
    },
    /// Add cursor at previous match: `:addprev [count]`
    AddPrev {
        /// Repeat count (how many times to add previous match).
        count: Option<u32>,
    },
    /// Skip current match and advance: `:skipmatch`
    SkipMatch,
    /// Add cursor above or below: `:addcursor above|below [count]`
    AddCursorDir {
        /// Direction: Backward = above, Forward = below.
        direction: crate::primitives::Direction,
        /// Repeat count.
        count: Option<u32>,
    },
    /// Select all occurrences: `:selectall`
    SelectAll,
    /// Clear secondary cursors: `:cursorcollapse`
    CursorCollapse,
    /// Remove cursor at primary: `:cursorremove`
    CursorRemove,
    /// Rotate primary cursor: `:cursorprimary next|prev`
    CursorPrimary {
        /// Direction to rotate: Forward = next, Backward = prev.
        direction: crate::primitives::Direction,
    },
    /// Visual block to cursors: `:cursorsplit`
    CursorSplitBlock,
    /// Flip anchor/head on all selections: `:cursorflip`
    CursorFlip,
    /// Ensure all selections face forward: `:cursorforward`
    CursorForward,
    /// Merge consecutive selections: `:cursormerge`
    CursorMerge,

    /// Repeat last substitute: `:&` (no flags) or `:&&` (keep previous flags).
    ///
    /// - `:&`  — rerun last `:s` with same pattern and replacement, but no flags.
    /// - `:&&` — rerun last `:s` with same pattern, replacement, **and** flags.
    RepeatSubstitute {
        /// Optional line range.  `None` means "current line" (resolved at execution time).
        range: Option<ExRange>,
        /// `true` for `:&&` (reuse previous flags), `false` for `:&` (no flags).
        use_previous_flags: bool,
    },

    /// Substitute using last search pattern + last substitute replacement: `:~`.
    ///
    /// Unlike `:s` with an empty pattern (which reuses the substitute pattern
    /// via `RE_LAST`), `:~` always uses the last `/`-search pattern (`@/`)
    /// combined with the last substitute replacement string.
    SubTilde {
        /// Line range to substitute within.
        range: ExRange,
        /// Substitute flags (g, c, i, I, n).
        flags: SubFlags,
    },

    /// Set option: `:set expandtab`, `:set tabstop=8`, `:set nohlsearch`
    Set {
        /// List of option assignments.
        assignments: smallvec::SmallVec<[SetAssignment; 2]>,
    },
    /// Set option local to buffer/window: `:setlocal expandtab`
    SetLocal {
        /// List of option assignments.
        assignments: smallvec::SmallVec<[SetAssignment; 2]>,
    },
    /// Set option in global scope: `:setglobal expandtab`
    SetGlobal {
        /// List of option assignments.
        assignments: smallvec::SmallVec<[SetAssignment; 2]>,
    },
    /// Configure per-key, per-mode shortcut conflict resolution: `:sethandler`.
    ///
    /// IdeaVim-compatible syntax:
    /// - `:sethandler <C-A> n:vim i:ide` — Ctrl-A: Vim in Normal, IDE in Insert
    /// - `:sethandler <C-C> n-v:ide i:vim` — dash-separated modes
    /// - `:sethandler n:vim i:ide` — no key → applies to ALL keys (global default)
    SetHandler {
        /// Optional key notation (e.g., `<C-A>`). If `None`, applies to all keys.
        key: Option<CompactString>,
        /// Mode:handler assignments (e.g., `("n", "vim")`, `("i", "ide")`).
        ///
        /// Each tuple is `(mode_chars, handler_name)` where:
        /// - `mode_chars` is a dash-separated list of mode characters
        ///   (`n` Normal, `i` Insert, `v` Visual, `x` Visual-only, `a` all)
        /// - `handler_name` is `"vim"` or `"ide"`/`"host"`
        assignments: Vec<(CompactString, CompactString)>,
    },

    /// Delete marks: `:delm[arks] {marks}` or `:delmarks!` (clear all a-z).
    DelMarks {
        /// The marks string (e.g., "abc" → delete marks a, b, c).
        marks: CompactString,
        /// If true (`:delmarks!`), clear all lowercase marks (a-z).
        clear_all: bool,
    },

    /// Quit with non-zero exit code: `:cq[uit] [exit_code]`
    CQuit {
        /// Exit code to return (default: 1).
        exit_code: i32,
    },

    /// Update (write only if modified): `:up[date][!] [path]`
    Update {
        /// Optional path to write to.
        path: Option<CompactString>,
        /// Force write (`:update!`).
        force: bool,
    },

    /// Redo: `:red[o]` — redo one change (same as Ctrl-R).
    Redo,
    /// Clear the jump list: `:clearj[umps]`
    ClearJumps,

    /// Create a fold from the range: `:[range]fold`
    Fold {
        /// Range of lines to fold.
        range: ExRange,
    },

    /// Open folds in range: `:[range]foldopen[!]`
    FoldOpen {
        /// Range of lines to open folds for.
        range: ExRange,
        /// If true (`:foldopen!`), open recursively.
        recursive: bool,
    },

    /// Close folds in range: `:[range]foldclose[!]`
    FoldClose {
        /// Range of lines to close folds for.
        range: ExRange,
        /// If true (`:foldclose!`), close recursively.
        recursive: bool,
    },

    // === Iterator commands (host handles iteration) ===
    /// Execute command in each window: `:windo {cmd}`
    WinDo {
        /// The sub-command to execute in each window.
        command: Box<Self>,
        /// Original source text of the sub-command (for host re-parsing).
        source: compact_str::CompactString,
    },
    /// Execute command in each buffer: `:bufdo {cmd}`
    BufDo {
        /// The sub-command to execute in each buffer.
        command: Box<Self>,
        /// Original source text of the sub-command (for host re-parsing).
        source: compact_str::CompactString,
    },
    /// Execute command in each tab: `:tabdo {cmd}`
    TabDo {
        /// The sub-command to execute in each tab.
        command: Box<Self>,
        /// Original source text of the sub-command (for host re-parsing).
        source: compact_str::CompactString,
    },

    /// Execute register contents as ex commands: `:@{register}`
    ExecuteRegister {
        /// Which register to read and execute.
        register: crate::primitives::RegisterName,
    },

    /// Join next change into previous undo group: `:undojoin`
    UndoJoin,

    /// Generate `.godot-vimrc` template: `:mkvimrc[!]`
    MkVimrc {
        /// Force overwrite if file already exists.
        force: bool,
    },
}

/// Amount for `:earlier` / `:later` navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TimeAmount {
    /// Navigate by number of changes (`:earlier 5`).
    Changes(u32),
    /// Navigate by seconds (`:earlier 10s`).
    Seconds(u64),
    /// Navigate by minutes (`:earlier 5m`).
    Minutes(u64),
    /// Navigate by hours (`:earlier 1h`).
    Hours(u64),
    /// Navigate by number of file saves (`:earlier 2f`).
    FileSaves(u32),
}

impl TimeAmount {
    /// Convert to seconds for time-based navigation.
    ///
    /// Returns `None` for `Changes` and `FileSaves` variants (not time-based).
    #[must_use]
    pub const fn to_seconds(self) -> Option<u64> {
        match self {
            Self::Changes(_) | Self::FileSaves(_) => None,
            Self::Seconds(s) => Some(s),
            Self::Minutes(m) => Some(m * 60),
            Self::Hours(h) => Some(h * 3600),
        }
    }

    /// Get the change count, if this is a `Changes` variant.
    #[must_use]
    pub const fn as_changes(self) -> Option<u32> {
        match self {
            Self::Changes(n) => Some(n),
            _ => None,
        }
    }

    /// Get the file-save count, if this is a `FileSaves` variant.
    #[must_use]
    pub const fn as_file_saves(self) -> Option<u32> {
        match self {
            Self::FileSaves(n) => Some(n),
            _ => None,
        }
    }
}

pub use crate::primitives::MapModePrefix;

/// A single `:set` assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SetAssignment {
    /// Set boolean on: `:set expandtab`
    SetBool(CompactString),
    /// Set boolean off: `:set noexpandtab`
    UnsetBool(CompactString),
    /// Toggle boolean: `:set expandtab!`
    ToggleBool(CompactString),
    /// Query option value: `:set expandtab?`
    Query(CompactString),
    /// Assign numeric/string value: `:set tabstop=8`
    Assign(CompactString, CompactString),
    /// Add to a number, append to a string, or add a flag or list item:
    /// `:set tw+=4`, `:set fo+=c`, `:set com+=b:##`
    Append(CompactString, CompactString),
    /// Subtract from a number, or remove a substring, flag or list item:
    /// `:set fo-=t`, `:set com-=b:#`
    Remove(CompactString, CompactString),
    /// Multiply a number, or prepend to a string or list: `:set com^=b:##`
    Prepend(CompactString, CompactString),
    /// Show all options: `:set all`
    ShowAll,
}

impl ExCommand {
    /// Returns `true` if this command mutates document text, requiring undo grouping.
    ///
    /// This is the authoritative list for undo-group wrapping in `executor_ex.rs`.
    /// Adding a new text-mutating ex command here automatically enables undo support —
    /// no separate patch to the executor is needed.
    #[must_use]
    pub const fn mutates_text(&self) -> bool {
        #[allow(
            unreachable_patterns,
            reason = "cfg-gated arms may overlap with wildcard"
        )]
        match self {
            Self::Substitute { .. }
            | Self::RepeatSubstitute { .. }
            | Self::SubTilde { .. }
            | Self::Global { .. }
            | Self::Delete { .. }
            | Self::Move { .. }
            | Self::Copy { .. }
            | Self::Join { .. }
            | Self::Sort { .. }
            | Self::Put { .. }
            | Self::Retab { .. }
            | Self::Left { .. }
            | Self::Right { .. }
            | Self::Center { .. }
            | Self::Norm { .. }
            | Self::Filter { .. } => true,
            Self::StructuralExtract { .. } | Self::StructuralComplement { .. } => true,
            Self::AlignSelections | Self::RotateContents | Self::RotateContentsDir { .. } => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::CaseSensitivity;

    #[test]
    fn test_sub_flags_parse() {
        let flags = SubFlags::parse("gi");
        assert!(flags.global());
        assert_eq!(flags.case(), CaseSensitivity::IgnoreCase);
        assert!(!flags.confirm());
    }

    #[test]
    fn test_sub_flags_r_flag() {
        let flags = SubFlags::parse("r");
        assert!(flags.use_last_search());
        assert!(!flags.global());
        assert!(!flags.reuse_flags());
    }

    #[test]
    fn test_sub_flags_ampersand_flag() {
        let flags = SubFlags::parse("&");
        assert!(flags.reuse_flags());
        assert!(!flags.global());
        assert!(!flags.use_last_search());
    }

    #[test]
    fn test_sub_flags_combined_r_and_g() {
        let flags = SubFlags::parse("gr");
        assert!(flags.global());
        assert!(flags.use_last_search());
    }

    #[test]
    fn test_sub_flags_combined_ampersand_and_g() {
        let flags = SubFlags::parse("g&");
        assert!(flags.global());
        assert!(flags.reuse_flags());
    }

    #[test]
    fn test_sub_flags_case_sensitivity_default() {
        let flags = SubFlags::parse("g");
        assert_eq!(flags.case(), CaseSensitivity::Default);
    }

    #[test]
    fn test_sub_flags_case_sensitivity_ignore_case() {
        let flags = SubFlags::parse("i");
        assert_eq!(flags.case(), CaseSensitivity::IgnoreCase);
    }

    #[test]
    fn test_sub_flags_case_sensitivity_force_case_sensitive() {
        let flags = SubFlags::parse("I");
        assert_eq!(flags.case(), CaseSensitivity::CaseSensitive);
    }

    #[test]
    fn test_sub_flags_case_last_one_wins_i_then_upper_i() {
        // Last-one-wins: `iI` → CaseSensitive
        let flags = SubFlags::parse("iI");
        assert_eq!(flags.case(), CaseSensitivity::CaseSensitive);
    }

    #[test]
    fn test_sub_flags_case_last_one_wins_upper_i_then_i() {
        // Last-one-wins: `Ii` → IgnoreCase
        let flags = SubFlags::parse("Ii");
        assert_eq!(flags.case(), CaseSensitivity::IgnoreCase);
    }

    #[test]
    fn test_sort_options_parse() {
        let opts = SortOptions::parse("nu!");
        assert!(opts.numeric());
        assert!(opts.unique());
        assert!(opts.reverse());
    }

    #[test]
    fn test_sort_options_flags_after_pattern() {
        let opts = SortOptions::parse("/pat/ u");
        assert!(opts.unique());
        assert_eq!(opts.pattern(), Some("pat"));
    }

    #[test]
    fn test_sort_options_flags_before_and_after_pattern() {
        let opts = SortOptions::parse("i /pat/ u");
        assert!(opts.ignore_case());
        assert!(opts.unique());
        assert_eq!(opts.pattern(), Some("pat"));
    }

    #[test]
    fn test_sort_options_pattern_no_flags() {
        let opts = SortOptions::parse("/pat/");
        assert!(!opts.numeric());
        assert!(!opts.ignore_case());
        assert!(!opts.unique());
        assert!(!opts.reverse());
        assert_eq!(opts.pattern(), Some("pat"));
    }

    #[test]
    fn test_sort_options_flags_before_pattern_only() {
        let opts = SortOptions::parse("u /pat/");
        assert!(opts.unique());
        assert_eq!(opts.pattern(), Some("pat"));
    }

    #[test]
    fn test_sort_options_multiple_flags_after_pattern() {
        let opts = SortOptions::parse("/pat/ nir");
        assert!(opts.numeric());
        assert!(opts.ignore_case());
        assert!(opts.reverse());
        assert_eq!(opts.pattern(), Some("pat"));
    }
}
