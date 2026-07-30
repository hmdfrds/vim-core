//! Parsed command types.
//!
//! Fully parsed commands ready for execution.

use std::num::NonZeroU32;

use compact_str::CompactString;

use super::types::Operator;
use super::types::{Action, CharCommand, MarkType, Motion, TextObject};
use crate::primitives::{
    CommandProperties, CompletionKind, InsertEntryType, MarkName, Mode, RegisterName,
    RepeatBehavior, StickyTarget, VisualType,
};

/// Insert-mode sub-commands routed via `dispatch_insert`.
///
/// These are commands generated while in insert mode (character input,
/// deletion, indentation, etc.). They are distinct from:
/// - `InsertEntry` — normal→insert mode transition (executor path)
/// - `InsertExit` — insert→normal mode transition (mode handler path)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum InsertKind {
    /// Character typed in insert mode.
    /// The character to insert.
    Char {
        /// Character value.
        char: char,
    },
    /// Backspace / Ctrl-H — delete character backward.
    Backspace,
    /// Ctrl-W — delete word backward.
    DeleteWord,
    /// Ctrl-U — delete to start of line.
    DeleteToStart,
    /// Delete key — delete character under cursor.
    DeleteUnder,
    /// Ctrl-T — indent current line.
    Indent,
    /// Ctrl-D — outdent current line.
    Outdent,
    /// ^Ctrl-D — remove all indent on current line, restore on next newline.
    OutdentTemporary,
    /// 0Ctrl-D — remove all indent on current line permanently.
    OutdentClear,
    /// Ctrl-A — insert previously inserted text.
    LastInserted,
    /// Ctrl-@ — insert previously inserted text and exit to Normal mode.
    LastInsertedAndExit,
    /// Ctrl-R {reg} — insert contents of a register.
    /// The register to insert from.
    Register {
        /// Register name (a-z, 0-9, or special).
        register: RegisterName,
    },
    /// Ctrl-V — request paste from system clipboard.
    Paste,
    /// Ctrl-O — one-shot normal mode (execute one command, return to insert).
    OneShot,
    /// Ctrl-E — copy character from line below.
    CopyCharBelow,
    /// Ctrl-Y — copy character from line above.
    CopyCharAbove,
    /// Ctrl-G u — break undo sequence.
    BreakUndoSequence,
    /// Ctrl-G U — prevent undo break on next insert-mode cursor movement.
    DontSyncUndo,
    /// Ctrl-X {key} — request completion of a specific kind.
    RequestCompletion {
        /// The kind of completion requested.
        kind: CompletionKind,
    },
    /// Host-injected text during insert mode (completion, snippet, AI suggestion).
    ///
    /// Hosts use this to report text injected through their own UI (e.g., LSP
    /// completions, snippet expansion, AI suggestions) so that:
    /// - The text is tracked in `accumulated_text` for dot-repeat fidelity
    /// - Macro recording captures the injected content
    /// - `Ctrl-A` (insert last inserted) includes the host-mediated text
    ///
    /// The actual text is stored in `VimState::pending_host_insert` via
    /// `VimEngine::stage_host_insert()` before sending this command.
    /// Precomputation reads and clears the staged text.
    ///
    /// This is the grammar-level counterpart to `VimEngine::apply_external_edit`.
    /// Use this when the host wants to go through the normal key-processing
    /// pipeline; use `apply_external_edit` for side-channel reconciliation.
    HostInserted,
    /// Literal character insertion (Ctrl-V {char}).
    ///
    /// Inserted without interpretation — bypasses expandtab, auto-pairs, etc.
    LiteralChar {
        /// Character value.
        char: char,
    },
    /// Expression register result (`<C-r>=expr<CR>`).
    ///
    /// The expression text to evaluate. For simple numeric/string literals,
    /// the text itself is the result. The execution layer evaluates the
    /// expression and inserts the result.
    ExpressionResult {
        /// The expression string.
        expression: String,
    },
    /// No-op — used when cancelling expression input (`<Esc>` during `<C-r>=`).
    Nop,
    /// Digraph input (Ctrl-K + two chars).
    ///
    /// Deferred to the execution layer for resolution so the grammar stays
    /// pure/stateless — the `DigraphRegistry` lives on `VimEngine` and is
    /// consulted during the insert pre-compute phase.
    Digraph {
        /// First character of the digraph pair.
        c1: char,
        /// Second character of the digraph pair.
        c2: char,
    },
    /// `<Insert>` key — toggle between Insert and Replace mode.
    ToggleReplace,
    /// `Ctrl-^` (Ctrl-6) — toggle langmap in insert mode.
    ///
    /// In normal mode, `Ctrl-^` switches to the alternate file.
    /// In insert mode, it toggles the keyboard language mapping.
    ToggleLangmap,
    /// `Ctrl-R Ctrl-W` — insert word under cursor.
    InsertWordUnderCursor,
    /// `Ctrl-R Ctrl-A` — insert WORD under cursor.
    InsertWORDUnderCursor,
    /// `Ctrl-R Ctrl-L` — insert current line.
    InsertCurrentLine,
}

impl InsertKind {
    /// Check if this insert sub-command modifies text.
    #[must_use]
    pub const fn is_mutating(&self) -> bool {
        match self {
            Self::Char { .. }
            | Self::LiteralChar { .. }
            | Self::Backspace
            | Self::DeleteWord
            | Self::DeleteToStart
            | Self::DeleteUnder
            | Self::Indent
            | Self::Outdent
            | Self::OutdentTemporary
            | Self::OutdentClear
            | Self::LastInserted
            | Self::LastInsertedAndExit
            | Self::Register { .. }
            | Self::Paste
            | Self::CopyCharBelow
            | Self::CopyCharAbove
            | Self::HostInserted => true,
            Self::ExpressionResult { .. }
            | Self::Digraph { .. }
            | Self::InsertWordUnderCursor
            | Self::InsertWORDUnderCursor
            | Self::InsertCurrentLine => true,
            Self::OneShot
            | Self::BreakUndoSequence
            | Self::DontSyncUndo
            | Self::RequestCompletion { .. }
            | Self::Nop
            | Self::ToggleReplace
            | Self::ToggleLangmap => false,
        }
    }

    /// Get the register for this insert command, if any.
    #[must_use]
    pub const fn register(&self) -> Option<RegisterName> {
        match self {
            Self::Register { register } => Some(*register),
            _ => None,
        }
    }

    /// Whether this insert sub-command's result depends on document content
    /// at the cursor position.
    ///
    /// Content-dependent commands read surrounding text (indentation, word
    /// boundaries, adjacent characters for auto-pairs, column position for
    /// expandtab, etc.) and therefore may produce different effects at
    /// different cursor positions even with identical input.
    ///
    /// Position-independent commands produce the same text mutation
    /// regardless of where the cursor sits (plain char insert, register
    /// paste, mode toggles, etc.).
    ///
    /// The `expandtab` and `auto_pairs` parameters reflect runtime editor
    /// settings that change whether certain characters need to inspect the
    /// document (tab-stop expansion reads column; auto-pairs reads adjacent
    /// characters).
    #[must_use]
    pub const fn is_content_dependent(&self, expandtab: bool, auto_pairs: bool) -> bool {
        match self {
            // ── Char: depends on the character and settings ──────────
            Self::Char { char: '\n' } => true,
            Self::Char { char: '\t' } => expandtab,
            Self::Char { .. } => auto_pairs,

            // ── Always content-dependent (read surrounding text) ─────
            Self::Backspace => true,
            Self::DeleteWord => true,
            Self::DeleteToStart => true,
            Self::DeleteUnder => true,
            Self::Indent => true,
            Self::Outdent => true,
            Self::OutdentTemporary => true,
            Self::OutdentClear => true,
            Self::CopyCharBelow => true,
            Self::CopyCharAbove => true,
            Self::ExpressionResult { .. } => true,
            Self::InsertWordUnderCursor => true,
            Self::InsertWORDUnderCursor => true,
            Self::InsertCurrentLine => true,

            // ── Always position-independent ──────────────────────────
            Self::LiteralChar { .. } => false,
            Self::Register { .. } => false,
            Self::LastInserted => false,
            Self::LastInsertedAndExit => false,
            Self::Paste => false,
            Self::OneShot => false,
            Self::BreakUndoSequence => false,
            Self::DontSyncUndo => false,
            Self::Nop => false,
            Self::RequestCompletion { .. } => false,
            Self::ToggleReplace => false,
            Self::HostInserted => false,
            Self::Digraph { .. } => false,
            Self::ToggleLangmap => false,
        }
    }
}

/// Visual-mode sub-commands routed via `dispatch_visual`.
///
/// These are commands generated while in visual mode (enter, exit,
/// switch, swap ends/corners, reselect). Distinct from:
/// - `OperatorSelection` — operator applied to visual selection (executor path)
/// - `VisualTextObject` — text object in visual mode (textobject dispatch path)
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum VisualKind {
    /// Enter visual mode from normal mode (`v`, `V`, `Ctrl-V`).
    /// When `count` is `Some`, scales the last visual selection by that factor.
    Enter {
        /// The visual type to enter.
        visual_type: VisualType,
        /// Optional count for scaling previous visual selection (`3v`).
        count: Option<u32>,
    },
    /// Exit visual mode (`Esc`, same-mode key).
    Exit,
    /// Switch visual mode type (`V` in char visual, etc.).
    Switch {
        /// The visual type to switch to.
        visual_type: VisualType,
    },
    /// Swap selection anchor and head (`o` command).
    SwapEnds,
    /// Swap block selection corners (`O` in block mode).
    SwapCorner,
    /// Reselect previous visual selection (`gv`).
    Reselect,
    /// Toggle between Visual and Select mode (`Ctrl-G`).
    ToggleSelect,
}

/// Macro sub-commands (record, stop, play).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MacroKind {
    /// Start recording a macro (`qa`).
    Record {
        /// Register to record into (a-z).
        register: RegisterName,
    },
    /// Stop recording a macro (`q` when recording).
    Stop,
    /// Play a macro from a register (`@a`, `@@`).
    Play {
        /// Register containing the macro.
        register: RegisterName,
        /// Number of times to replay.
        count: NonZeroU32,
    },
    /// Repeat the last ex command (`@:`).
    RepeatLastEx {
        /// Number of times to repeat.
        count: NonZeroU32,
    },
}

/// Typed prefix command — replaces raw `(prefix: char, key: char)` pairs.
///
/// Only recognized prefix combinations are representable; unrecognized
/// prefixes are rejected at the grammar level (`GrammarResult::Invalid`),
/// eliminating impossible-state match arms in the executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PrefixCommand {
    // ── g-prefix ──────────────────────────────────────────────────────
    /// `gi` — jump to insert-stop mark and enter Insert mode.
    GotoInsertStop,
    /// `gJ` — join lines without inserting a space.
    JoinNoSpace,

    // ── z-prefix (scroll) ────────────────────────────────────────────
    /// `zz` — center viewport on cursor line.
    ScrollCenter,
    /// `zt` — scroll cursor line to top of viewport.
    ScrollTop,
    /// `zb` — scroll cursor line to bottom of viewport.
    ScrollBottom,
    /// `z<CR>` — move cursor to first non-blank, then scroll to top.
    FirstNonBlankTop,
    /// `z.` — move cursor to first non-blank, then center.
    FirstNonBlankCenter,
    /// `z-` — move cursor to first non-blank, then scroll to bottom.
    FirstNonBlankBottom,

    // ── z-prefix (horizontal scroll) ─────────────────────────────────
    /// `zh` — scroll viewport one column left.
    ScrollColumnLeft,
    /// `zl` — scroll viewport one column right.
    ScrollColumnRight,
    /// `zH` — scroll viewport half a screen left.
    ScrollHalfScreenLeft,
    /// `zL` — scroll viewport half a screen right.
    ScrollHalfScreenRight,
    /// `zs` — scroll viewport to place cursor at left edge.
    ScrollCursorToLeft,
    /// `ze` — scroll viewport to place cursor at right edge.
    ScrollCursorToRight,

    // ── z-prefix (fold) ────────────────────────────────────────────
    /// `zc` — close fold at cursor line.
    FoldClose,
    /// `zo` — open fold at cursor line.
    FoldOpen,
    /// `za` — toggle fold at cursor line.
    FoldToggle,
    /// `zA` — recursively toggle fold at cursor line.
    FoldToggleRecursive,
    /// `zM` — close all folds in the document.
    FoldCloseAll,
    /// `zR` — open all folds in the document.
    FoldOpenAll,

    // ── Z-prefix ─────────────────────────────────────────────────────
    /// `ZZ` — write file and quit.
    WriteQuit,
    /// `ZQ` — quit without saving (force quit).
    ForceQuit,

    // ── g-prefix (LSP navigation) ─────────────────────────────────────
    /// `gd` — go to definition of symbol under cursor.
    GotoDefinition,

    // ── g-prefix (informational) ─────────────────────────────────────
    /// `ga` — show ASCII/Unicode value of character under cursor.
    ShowAscii,
    /// `g8` — show UTF-8 byte sequence of character under cursor.
    ShowUtf8,

    // ── g-prefix (undo branch navigation) ─────────────────────────────
    /// `g-` — navigate to earlier undo state (`:earlier 1` equivalent).
    UndoEarlier,
    /// `g+` — navigate to later undo state (`:later 1` equivalent).
    UndoLater,

    // ── g-prefix (visual sequential) ─────────────────────────────────
    /// `g Ctrl-A` — sequential increment (visual mode).
    SequentialIncrement,
    /// `g Ctrl-X` — sequential decrement (visual mode).
    SequentialDecrement,

    // ── q-prefix (command-line window) ───────────────────────────────
    /// `q:` — open command-line history window.
    OpenExHistory,
    /// `q/` — open forward search history window.
    OpenSearchForwardHistory,
    /// `q?` — open backward search history window.
    OpenSearchBackwardHistory,

    // ── Ctrl-W prefix (window commands) ───────────────────────────────
    /// `Ctrl-W s` — split window horizontally.
    WindowSplit,
    /// `Ctrl-W n` — open a new empty buffer in a split window (`:new`).
    WindowNew,
    /// `Ctrl-W v` — split window vertically.
    WindowVSplit,
    /// `Ctrl-W c` — close current window.
    WindowClose,
    /// `Ctrl-W o` — close all other windows.
    WindowOnly,
    /// `Ctrl-W w` — move to next window.
    WindowNext,
    /// `Ctrl-W W` — move to previous window.
    WindowPrev,
    /// `Ctrl-W h` — move to left window.
    WindowMoveLeft,
    /// `Ctrl-W l` — move to right window.
    WindowMoveRight,
    /// `Ctrl-W k` — move to window above.
    WindowMoveUp,
    /// `Ctrl-W j` — move to window below.
    WindowMoveDown,
    /// `Ctrl-W =` — equalize all window sizes.
    WindowEqualSize,
    /// `Ctrl-W +` — increase window height.
    WindowIncreaseHeight,
    /// `Ctrl-W -` — decrease window height.
    WindowDecreaseHeight,
    /// `Ctrl-W >` — increase window width.
    WindowIncreaseWidth,
    /// `Ctrl-W <` — decrease window width.
    WindowDecreaseWidth,
    /// `Ctrl-W r` — rotate windows downward/rightward.
    WindowRotateDown,
    /// `Ctrl-W R` — rotate windows upward/leftward.
    WindowRotateUp,

    // ── z-prefix (additional fold commands) ────────────────────────────
    /// `zO` — recursively open all folds at cursor line.
    FoldOpenRecursive,
    /// `zC` — recursively close all folds at cursor line.
    FoldCloseRecursive,
    /// `zd` — delete fold at cursor.
    FoldDelete,
    /// `zD` — recursively delete folds at cursor.
    FoldDeleteRecursive,
    /// `zE` — eliminate all folds in document.
    FoldEliminateAll,
    /// `zi` — toggle foldenable option.
    FoldToggleEnable,
    /// `zn` — fold none (disable folding).
    FoldDisable,
    /// `zN` — fold normal (enable folding).
    FoldEnable,

    // ── Bracket-prefix (vim-unimpaired) ─────────────────────────────
    /// `[<Space>` — insert blank line above.
    InsertBlankAbove,
    /// `]<Space>` — insert blank line below.
    InsertBlankBelow,

    // ── g-prefix (incremental syntax selection) ──────────────────────
    /// `g[` — expand selection to parent syntax node.
    SelectParentNode,
    /// `g]` — shrink selection to child syntax node (or pop history).
    SelectChildNode,
    /// `g{` — select previous sibling syntax node.
    SelectPrevSibling,
    /// `g}` — select next sibling syntax node.
    SelectNextSibling,
    /// `g(` -- select all sibling syntax nodes (fan-out to multi-cursor).
    SelectAllSiblings,
    /// `g)` -- select all child syntax nodes (fan-out to multi-cursor).
    SelectAllChildren,

    // ── Sticky sub-mode entry ───────────────────────────────────────────
    /// Enter sticky sub-mode for the given prefix group.
    StickyEnter {
        /// The prefix group to make sticky.
        target: StickyTarget,
    },
}

impl PrefixCommand {
    /// Whether this prefix command modifies text.
    #[must_use]
    pub const fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::JoinNoSpace
                | Self::SequentialIncrement
                | Self::SequentialDecrement
                | Self::InsertBlankAbove
                | Self::InsertBlankBelow
        )
    }

    /// Classify which sticky sub-mode group this prefix command belongs to.
    ///
    /// Returns `Some(StickyTarget::Window)` for Ctrl-W window commands,
    /// `Some(StickyTarget::ZPrefix)` for z-prefix scroll/fold commands,
    /// or `None` for commands that don't belong to a sticky-eligible group
    /// (g-prefix, Z-prefix, q-prefix, bracket-prefix, StickyEnter itself).
    #[must_use]
    pub const fn sticky_group(&self) -> Option<StickyTarget> {
        match self {
            // Ctrl-W window commands
            Self::WindowSplit
            | Self::WindowNew
            | Self::WindowVSplit
            | Self::WindowClose
            | Self::WindowOnly
            | Self::WindowNext
            | Self::WindowPrev
            | Self::WindowMoveLeft
            | Self::WindowMoveRight
            | Self::WindowMoveUp
            | Self::WindowMoveDown
            | Self::WindowEqualSize
            | Self::WindowIncreaseHeight
            | Self::WindowDecreaseHeight
            | Self::WindowIncreaseWidth
            | Self::WindowDecreaseWidth
            | Self::WindowRotateDown
            | Self::WindowRotateUp => Some(StickyTarget::Window),

            // z-prefix scroll commands
            Self::ScrollCenter
            | Self::ScrollTop
            | Self::ScrollBottom
            | Self::FirstNonBlankTop
            | Self::FirstNonBlankCenter
            | Self::FirstNonBlankBottom
            | Self::ScrollColumnLeft
            | Self::ScrollColumnRight
            | Self::ScrollHalfScreenLeft
            | Self::ScrollHalfScreenRight
            | Self::ScrollCursorToLeft
            | Self::ScrollCursorToRight => Some(StickyTarget::ZPrefix),

            // z-prefix fold commands
            Self::FoldClose
            | Self::FoldOpen
            | Self::FoldToggle
            | Self::FoldToggleRecursive
            | Self::FoldCloseAll
            | Self::FoldOpenAll
            | Self::FoldOpenRecursive
            | Self::FoldCloseRecursive
            | Self::FoldDelete
            | Self::FoldDeleteRecursive
            | Self::FoldEliminateAll
            | Self::FoldToggleEnable
            | Self::FoldDisable
            | Self::FoldEnable => Some(StickyTarget::ZPrefix),

            // Not sticky-eligible: g-prefix, Z-prefix, q-prefix, bracket-prefix,
            // syntax selection, StickyEnter itself.
            Self::GotoInsertStop
            | Self::JoinNoSpace
            | Self::WriteQuit
            | Self::ForceQuit
            | Self::GotoDefinition
            | Self::ShowAscii
            | Self::ShowUtf8
            | Self::SequentialIncrement
            | Self::SequentialDecrement
            | Self::OpenExHistory
            | Self::OpenSearchForwardHistory
            | Self::OpenSearchBackwardHistory
            | Self::InsertBlankAbove
            | Self::InsertBlankBelow
            | Self::SelectParentNode
            | Self::SelectChildNode
            | Self::SelectPrevSibling
            | Self::SelectNextSibling
            | Self::SelectAllSiblings
            | Self::SelectAllChildren
            | Self::UndoEarlier
            | Self::UndoLater
            | Self::StickyEnter { .. } => None,
        }
    }
}

/// Fully parsed command ready for execution.
///
/// This is the output of the grammar parser when a complete
/// command has been recognized. Commands must be executed or
/// explicitly discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "parsed commands must be executed or explicitly discarded"]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Command {
    /// Pure motion command (no operator).
    ///
    /// Examples: `j`, `3w`, `$`
    Motion {
        /// Effective count (product of all counts)
        count: NonZeroU32,
        /// The motion
        motion: Motion,
        /// Whether count was explicitly provided (for G motion: 1G vs G)
        explicit_count: bool,
    },

    /// Operator applied to a motion.
    ///
    /// Examples: `dw`, `3cj`, `y$`, `dv$` (forced charwise)
    OperatorMotion {
        /// Effective count
        count: NonZeroU32,
        /// Target register
        register: Option<RegisterName>,
        /// The operator
        operator: Operator,
        /// The motion
        motion: Motion,
        /// Motion force override (`dv$` = charwise, `dVj` = linewise, `d<C-v>j` = blockwise).
        /// When set, overrides the motion's natural inclusivity/type.
        force_type: Option<crate::primitives::MotionType>,
    },

    /// Operator applied to a text object.
    ///
    /// Examples: `diw`, `ca(`, `yap`
    OperatorTextObject {
        /// Effective count
        count: NonZeroU32,
        /// Target register
        register: Option<RegisterName>,
        /// The operator
        operator: Operator,
        /// The text object
        textobject: TextObject,
    },

    /// Linewise operator (doubled key).
    ///
    /// Examples: `dd`, `yy`, `cc`, `3dd`
    OperatorLine {
        /// Effective count (number of lines)
        count: NonZeroU32,
        /// Target register
        register: Option<RegisterName>,
        /// The operator
        operator: Operator,
    },

    /// Operator applied to a mark motion.
    ///
    /// Examples: `y'a`, `d'b`, `` d`a ``
    OperatorMark {
        /// Effective count
        count: NonZeroU32,
        /// Target register
        register: Option<RegisterName>,
        /// The operator
        operator: Operator,
        /// Mark name
        mark: MarkName,
        /// Mark type (JumpLine = 'a, JumpExact = `a)
        mark_type: MarkType,
    },

    /// Standalone action command.
    ///
    /// Examples: `x`, `3p`, `u`, `.`, `"0p`, `"ap`
    Action {
        /// Effective count
        count: NonZeroU32,
        /// Target register (for put/yank actions)
        register: Option<RegisterName>,
        /// The action
        action: Action,
    },

    /// Character command result.
    ///
    /// Examples: `fa`, `tb`, `r!`, `dfa`
    ///
    /// The `target` field is a `CompactString` rather than a plain `char` to
    /// support grapheme clusters composed of a base character followed by one
    /// or more Unicode combining marks (e.g. `e` + U+0301 = `é`).  For the
    /// common single-character case the string contains exactly one codepoint.
    CharCommand {
        /// Effective count
        count: NonZeroU32,
        /// Target register (for operator + char command)
        register: Option<RegisterName>,
        /// Optional operator (for `dfa`, `ct;`)
        operator: Option<Operator>,
        /// The char command type
        command: CharCommand,
        /// The target grapheme (base char + optional combining marks).
        target: CompactString,
    },

    /// Sneak motion command (two-character cross-line find).
    ///
    /// Examples: `sab`, `Sxy`, `dsab`
    Sneak {
        /// Effective count
        count: NonZeroU32,
        /// Target register (for operator + sneak)
        register: Option<RegisterName>,
        /// Optional operator (for `dsab`, `csab`)
        operator: Option<Operator>,
        /// First target character
        c1: char,
        /// Second target character
        c2: char,
        /// Search direction: true = forward (`s`), false = backward (`S`)
        forward: bool,
    },

    /// Mark command.
    ///
    /// Examples: `ma`, `'b`, `` `c ``
    Mark {
        /// Effective count (for jump with count)
        count: NonZeroU32,
        /// The mark command type
        mark_type: MarkType,
        /// The mark name
        mark: MarkName,
    },

    /// Mode switch command.
    ///
    /// Examples: `i`, `v`, `V`, `:`
    ModeSwitch {
        /// Target mode
        mode: Mode,
    },

    /// Prefix command result.
    ///
    /// Examples: `gi`, `gJ`, `zz`, `zt`
    Prefix {
        /// Effective count
        count: NonZeroU32,
        /// Target register
        register: Option<RegisterName>,
        /// The typed prefix command
        command: PrefixCommand,
    },

    /// Insert-mode sub-command (character input, deletion, indentation, etc.).
    ///
    /// Routed via `dispatch_insert` by the engine. See `InsertKind` for variants.
    Insert(InsertKind),

    /// Enter insert mode (i/I/a/A/o/O/s/S).
    ///
    /// Produces `BeginUndoGroup` + cursor positioning + SetMode(Insert)
    InsertEntry {
        /// Effective count
        count: NonZeroU32,
        /// How to enter insert mode
        entry_type: InsertEntryType,
        /// Optional register (e.g. `"_s` uses blackhole register)
        register: Option<RegisterName>,
    },

    /// Exit insert mode with proper cleanup.
    ///
    /// Produces `EndUndoGroup` + `SetMark`('^') + SetMode(Normal)
    InsertExit,

    /// Visual-mode sub-command (enter, exit, switch, swap, reselect).
    ///
    /// Routed via `dispatch_visual` by the executor. See `VisualKind` for variants.
    Visual(VisualKind),

    /// Operator applied to visual selection.
    ///
    /// Examples: `d` in visual mode, `y` in visual mode, `c` in visual mode
    /// The selection range comes from the context, not a motion.
    OperatorSelection {
        /// Target register
        register: Option<RegisterName>,
        /// The operator to apply
        operator: Operator,
    },

    /// Visual block `zy` — yank trimming trailing whitespace from each line.
    ///
    /// Neovim's `excl_tr_ws` behavior: in block visual mode, `zy` yanks
    /// the block selection but strips trailing whitespace from each
    /// register part before writing to the register.
    YankTrimmed {
        /// Target register.
        register: Option<RegisterName>,
    },

    /// Text object in visual mode - sets selection to text object range.
    ///
    /// Examples: `viw`, `va(`, `vip`
    /// Unlike motions, text objects SET the selection range (anchor + cursor)
    /// rather than extending from the current anchor.
    ///
    /// - If single-char selection OR anchor outside text object: reset anchor
    /// - Otherwise: preserve anchor and extend
    VisualTextObject {
        /// Effective count
        count: NonZeroU32,
        /// The text object
        textobject: TextObject,
        /// Register selected before text object (preserved for subsequent operator)
        register: Option<RegisterName>,
    },

    /// Macro command (record, stop, play).
    ///
    /// See `MacroKind` for variants.
    Macro(MacroKind),

    /// Enter select mode from normal mode (`gh`, `gH`, `g<Ctrl-H>`).
    SelectEnter {
        /// The visual/select type to enter.
        visual_type: VisualType,
    },

    /// Surround add — `ys{motion}{char}` or visual `S{char}`.
    ///
    /// Wraps the operator range with the given delimiter character.
    /// The range comes from a motion/text-object (ys) or visual selection (S).
    SurroundAdd {
        /// Effective count
        count: NonZeroU32,
        /// The motion that defines the range (None for visual S)
        motion: Option<Motion>,
        /// Text object if used instead of motion
        textobject: Option<TextObject>,
        /// The surround delimiter character
        char: char,
    },

    /// Surround delete — `ds{char}`.
    ///
    /// Finds and deletes the surrounding pair identified by `char`.
    SurroundDelete {
        /// The delimiter character identifying the pair to delete
        char: char,
    },

    /// Surround change — `cs{old}{new}`.
    ///
    /// Finds the surrounding pair identified by `old_char` and replaces
    /// both delimiters with the pair identified by `new_char`.
    SurroundChange {
        /// The existing delimiter to find
        old_char: char,
        /// The new delimiter to replace with
        new_char: char,
    },
}

impl Command {
    /// Get the effective count for this command.
    ///
    /// All commands have a count of at least 1.
    #[must_use]
    pub const fn count(&self) -> NonZeroU32 {
        match self {
            Self::Motion { count, .. }
            | Self::OperatorMotion { count, .. }
            | Self::OperatorTextObject { count, .. }
            | Self::OperatorLine { count, .. }
            | Self::OperatorMark { count, .. }
            | Self::Action { count, .. }
            | Self::CharCommand { count, .. }
            | Self::Sneak { count, .. }
            | Self::Mark { count, .. }
            | Self::Prefix { count, .. }
            | Self::InsertEntry { count, .. }
            | Self::VisualTextObject { count, .. }
            | Self::SurroundAdd { count, .. } => *count,
            Self::Macro(MacroKind::Play { count, .. } | MacroKind::RepeatLastEx { count }) => {
                *count
            }
            Self::Insert(_)
            | Self::InsertExit
            | Self::ModeSwitch { .. }
            | Self::Visual(_)
            | Self::OperatorSelection { .. }
            | Self::YankTrimmed { .. }
            | Self::SelectEnter { .. }
            | Self::SurroundDelete { .. }
            | Self::SurroundChange { .. }
            | Self::Macro(MacroKind::Record { .. } | MacroKind::Stop) => NonZeroU32::MIN,
        }
    }

    /// Returns true if this is an insert-mode-specific command.
    ///
    /// These commands arrive via `ModeAction::InsertCommand` and are
    /// routed directly to `dispatch_insert` by the engine (bypassing executor).
    ///
    /// NOT included (they use different routing paths):
    /// - `InsertEntry` → normal-mode grammar → executor → `entry::execute()`
    /// - `InsertExit` → mode handler → `ModeAction::InsertExit` → `handle_insert_exit()`
    #[inline]
    #[must_use]
    pub const fn is_insert_specific(&self) -> bool {
        matches!(self, Self::Insert(_))
    }

    /// Check if this command should preserve the pre-operation mark `.`.
    ///
    /// Returns `true` for commands that modify text but should NOT update
    /// mark `.` — matching Neovim behavior where `Ctrl-A`/`Ctrl-X` do not
    /// go through the operator-level mark update path.
    #[must_use]
    pub const fn preserves_mark_dot(&self) -> bool {
        match self {
            Self::Action { action, .. } => action.preserves_mark_dot(),
            _ => false,
        }
    }

    /// Get the register for this command, if any.
    #[must_use]
    pub const fn register(&self) -> Option<RegisterName> {
        match self {
            Self::OperatorMotion { register, .. }
            | Self::OperatorTextObject { register, .. }
            | Self::OperatorLine { register, .. }
            | Self::OperatorMark { register, .. }
            | Self::Action { register, .. }
            | Self::CharCommand { register, .. }
            | Self::Sneak { register, .. }
            | Self::Prefix { register, .. }
            | Self::OperatorSelection { register, .. }
            | Self::YankTrimmed { register, .. }
            | Self::VisualTextObject { register, .. } => *register,
            Self::Insert(kind) => kind.register(),
            _ => None,
        }
    }

    /// Check if this command modifies text.
    #[must_use]
    pub const fn is_mutating(&self) -> bool {
        match self {
            Self::Motion { .. }
            | Self::ModeSwitch { .. }
            | Self::Mark { .. }
            | Self::InsertExit
            | Self::Visual(_)
            | Self::YankTrimmed { .. }
            | Self::SelectEnter { .. }
            | Self::VisualTextObject { .. }
            | Self::Macro(_) => false,
            Self::Prefix { command, .. } => command.is_mutating(),
            Self::OperatorMotion { operator, .. }
            | Self::OperatorTextObject { operator, .. }
            | Self::OperatorLine { operator, .. }
            | Self::OperatorMark { operator, .. }
            | Self::OperatorSelection { operator, .. } => operator.is_mutating(),
            Self::Action { action, .. } => action.is_mutating(),
            Self::CharCommand {
                operator, command, ..
            } => operator.is_some() || matches!(command, CharCommand::Replace),
            Self::Sneak { operator, .. } => operator.is_some(),
            Self::Insert(kind) => kind.is_mutating(),
            Self::InsertEntry { .. } => true, // Entry starts insert session
            Self::SurroundAdd { .. }
            | Self::SurroundDelete { .. }
            | Self::SurroundChange { .. } => true,
        }
    }

    /// Returns `true` if this command's execution depends on document content.
    ///
    /// Content-dependent commands need access to text at the cursor position
    /// (or nearby) to determine their effect. Position-independent commands
    /// produce the same effects regardless of what text is in the buffer.
    ///
    /// This is used on the normal-mode `execute_effect_plan` path. For
    /// insert-mode commands, use [`InsertKind::is_content_dependent`] directly
    /// (it requires runtime `expandtab`/`auto_pairs` parameters).
    ///
    /// The match is exhaustive with no wildcard arm.
    #[must_use]
    pub const fn is_content_dependent(&self) -> bool {
        match self {
            // Standalone motions: always CD for multi-cursor dispatch.
            // Each cursor must independently compute its motion target
            // because the result depends on the text/line geometry around
            // that specific cursor. Algebraic rebase (flat delta shift)
            // produces wrong results for all non-trivial motions.
            Self::Motion { .. } => true,

            // Operator + motion: CD if the operator mutates text (Delete/Change
            // produce range-width-dependent effects — different lines/words have
            // different byte widths), reads content (case/format transforms), or
            // the motion depends on content. Yank is PI because it only reads
            // text for register content (handled by RangeSource override).
            Self::OperatorMotion {
                operator, motion, ..
            } => {
                operator.is_mutating_text()
                    || operator.is_content_reading()
                    || motion.is_content_dependent()
            }

            // Operator + text object: always CD — text objects scan content
            // to find boundaries.
            Self::OperatorTextObject { .. } => true,

            // Linewise operator (dd/cc): CD if the operator mutates text
            // (line byte widths differ per cursor) or reads content.
            // yy is PI — range width is irrelevant for yank.
            Self::OperatorLine { operator, .. } => {
                operator.is_mutating_text() || operator.is_content_reading()
            }

            // Operator + mark: CD if the operator mutates text (distance
            // from cursor to mark varies) or reads content.
            Self::OperatorMark { operator, .. } => {
                operator.is_mutating_text() || operator.is_content_reading()
            }

            // Operator on visual selection: CD if the operator mutates text
            // (selection extents differ per cursor) or reads content.
            Self::OperatorSelection { operator, .. } => {
                operator.is_mutating_text() || operator.is_content_reading()
            }

            // Char command: CD if operator present (find distance varies by
            // content), Replace (reads grapheme width), or find/till (target
            // char appears at different positions per cursor line — algebraic
            // rebase copies primary's search result delta blindly).
            Self::CharCommand {
                operator, command, ..
            } => {
                operator.is_some()
                    || matches!(
                        command,
                        CharCommand::Replace
                            | CharCommand::FindForward
                            | CharCommand::FindBackward
                            | CharCommand::TillForward
                            | CharCommand::TillBackward
                    )
            }

            // Sneak with operator: always CD — match position varies.
            // Without operator: just a cursor move.
            Self::Sneak { operator, .. } => operator.is_some(),

            // Surround operations scan content to find/modify delimiters.
            Self::SurroundAdd { .. }
            | Self::SurroundDelete { .. }
            | Self::SurroundChange { .. } => true,

            // Standalone action: delegates to Action::is_content_dependent().
            Self::Action { action, .. } => action.is_content_dependent(),

            // Insert entry (i/I/a/A/o/O/s/S): position-independent for
            // per-cursor dispatch. While some entries read text (o/O for
            // autoindent, s/S for substitution), they involve mode
            // transitions and undo group management that must happen
            // exactly once via algebraic rebase, not per-cursor.
            Self::InsertEntry { .. } => false,

            // Prefix commands: most are PI, but some produce mutating effects
            // that depend on text content.
            Self::Prefix { command, .. } => matches!(
                command,
                PrefixCommand::JoinNoSpace
                    | PrefixCommand::SequentialIncrement
                    | PrefixCommand::SequentialDecrement
            ),

            // Mode switch, marks, visual, insert sub-commands, macros,
            // select enter, visual text object: all position-independent.
            Self::ModeSwitch { .. }
            | Self::Mark { .. }
            | Self::Visual(_)
            | Self::YankTrimmed { .. }
            | Self::Insert(_)
            | Self::InsertExit
            | Self::Macro(_)
            | Self::SelectEnter { .. }
            | Self::VisualTextObject { .. } => false,
        }
    }

    /// Returns `true` if this command is a global-only navigation that should
    /// NOT be replicated per cursor in multi-cursor mode.
    ///
    /// Jump list and changelist navigation operate on a single global position
    /// list. Replicating them per cursor would produce nonsensical results
    /// (each cursor jumping to a delta-shifted position instead of the actual
    /// stored position).
    #[inline]
    #[must_use]
    pub const fn is_global_only(&self) -> bool {
        matches!(
            self,
            Self::Action {
                action: Action::JumpOlder
                    | Action::JumpNewer
                    | Action::Undo
                    | Action::Redo
                    | Action::UndoLine,
                ..
            } | Self::Motion {
                motion: Motion::ChangelistOlder | Motion::ChangelistNewer,
                ..
            }
        )
    }

    /// Returns `true` if this command can produce an `OperatorFilter` effect.
    ///
    /// Used to avoid eagerly cloning document text on every command execution.
    #[inline]
    #[must_use]
    pub const fn may_produce_operator_filter(&self) -> bool {
        matches!(
            self,
            Self::OperatorMotion {
                operator: Operator::Filter | Operator::Reindent,
                ..
            } | Self::OperatorTextObject {
                operator: Operator::Filter | Operator::Reindent,
                ..
            } | Self::OperatorLine {
                operator: Operator::Filter | Operator::Reindent,
                ..
            } | Self::OperatorMark {
                operator: Operator::Filter | Operator::Reindent,
                ..
            } | Self::OperatorSelection {
                operator: Operator::Filter | Operator::Reindent,
                ..
            } | Self::CharCommand {
                operator: Some(Operator::Filter | Operator::Reindent),
                ..
            } | Self::Sneak {
                operator: Some(Operator::Filter | Operator::Reindent),
                ..
            }
        )
    }

    /// Check if this command should be stored for repeat (.).
    #[must_use]
    pub const fn is_repeatable(&self) -> bool {
        self.is_mutating()
    }

    /// Check if this command is an insert-mode sub-command.
    ///
    /// Insert sub-commands (char input, backspace, exit, etc.) should NOT
    /// overwrite `last_command` in the parser, because the repeatable unit
    /// is the parent `InsertEntry` command that started the insert session.
    #[must_use]
    pub const fn is_insert_subcommand(&self) -> bool {
        matches!(self, Self::Insert(_) | Self::InsertExit)
    }

    /// Return the behavioral properties of this command.
    ///
    /// Describes how the command interacts with repeat (`.`), the jump list,
    /// visual mode persistence, cursor movement, operator suppression, and
    /// idempotency. The match is exhaustive with no wildcard arm.
    #[must_use]
    pub const fn properties(&self) -> CommandProperties {
        match self {
            // Pure motion: skip repeat, keep visual selection, idempotent.
            Self::Motion { .. } => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: true,
                move_point: false,
                suppress_operator: false,
                idempotent: true,
            },

            // Operator + motion: record for repeat only if operator is mutating (not yank).
            Self::OperatorMotion { operator, .. } => CommandProperties {
                repeat: if operator.is_mutating() {
                    RepeatBehavior::Record
                } else {
                    RepeatBehavior::Skip
                },
                jump: false,
                keep_visual: false,
                move_point: true,
                suppress_operator: false,
                idempotent: false,
            },

            // Operator + text object: record for repeat only if operator is mutating.
            Self::OperatorTextObject { operator, .. } => CommandProperties {
                repeat: if operator.is_mutating() {
                    RepeatBehavior::Record
                } else {
                    RepeatBehavior::Skip
                },
                jump: false,
                keep_visual: false,
                move_point: true,
                suppress_operator: false,
                idempotent: false,
            },

            // Linewise operator (dd/yy/cc): record for repeat only if operator is mutating.
            Self::OperatorLine { operator, .. } => CommandProperties {
                repeat: if operator.is_mutating() {
                    RepeatBehavior::Record
                } else {
                    RepeatBehavior::Skip
                },
                jump: false,
                keep_visual: false,
                move_point: true,
                suppress_operator: false,
                idempotent: false,
            },

            // Operator + mark motion: record for repeat only if operator is mutating.
            Self::OperatorMark { operator, .. } => CommandProperties {
                repeat: if operator.is_mutating() {
                    RepeatBehavior::Record
                } else {
                    RepeatBehavior::Skip
                },
                jump: false,
                keep_visual: false,
                move_point: true,
                suppress_operator: false,
                idempotent: false,
            },

            // Standalone action (x, p, u, .): record for repeat only if mutating.
            // Non-mutating actions (undo, redo, yank-line, jumps, info) skip repeat.
            Self::Action { action, .. } => CommandProperties {
                repeat: if action.is_mutating() {
                    RepeatBehavior::Record
                } else {
                    RepeatBehavior::Skip
                },
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Char command (f/t/r/d+f etc.): depends on whether an operator is present.
            // With operator (dfa, ct;): operator motion — record + move_point.
            // Without operator: standalone. Replace (r) is mutating → record.
            // Find motions (f/F/t/T) are non-mutating → skip + keep_visual.
            Self::CharCommand {
                operator, command, ..
            } => {
                if operator.is_some() {
                    CommandProperties {
                        repeat: RepeatBehavior::Record,
                        jump: false,
                        keep_visual: false,
                        move_point: true,
                        suppress_operator: false,
                        idempotent: false,
                    }
                } else if matches!(command, CharCommand::Replace) {
                    CommandProperties {
                        repeat: RepeatBehavior::Record,
                        jump: false,
                        keep_visual: false,
                        move_point: false,
                        suppress_operator: false,
                        idempotent: false,
                    }
                } else {
                    CommandProperties {
                        repeat: RepeatBehavior::Skip,
                        jump: false,
                        keep_visual: true,
                        move_point: false,
                        suppress_operator: false,
                        idempotent: true,
                    }
                }
            }

            // Sneak motion: same pattern as CharCommand (f/t).
            // With operator: record + move_point. Without: skip + keep_visual.
            Self::Sneak { operator, .. } => {
                if operator.is_some() {
                    CommandProperties {
                        repeat: RepeatBehavior::Record,
                        jump: false,
                        keep_visual: false,
                        move_point: true,
                        suppress_operator: false,
                        idempotent: false,
                    }
                } else {
                    CommandProperties {
                        repeat: RepeatBehavior::Skip,
                        jump: false,
                        keep_visual: true,
                        move_point: false,
                        suppress_operator: false,
                        idempotent: true,
                    }
                }
            }

            // Mark jump/set: skip repeat, jumps add to jump list, keep visual.
            Self::Mark { .. } => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: true,
                move_point: false,
                suppress_operator: false,
                idempotent: true,
            },

            // Mode switch (i, v, V, :): skip repeat.
            Self::ModeSwitch { .. } => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Prefix commands (gi, gJ, zz, etc.): mutating prefixes (gJ, [e, ]e,
            // [<Space>, ]<Space>, g<C-a>, g<C-x>) record for repeat; all others skip.
            Self::Prefix { command, .. } => CommandProperties {
                repeat: if command.is_mutating() {
                    RepeatBehavior::Record
                } else {
                    RepeatBehavior::Skip
                },
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Insert sub-command: skip repeat (the repeatable unit is InsertEntry).
            Self::Insert(_) => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Enter insert mode (i/I/a/A/o/O): record for repeat.
            Self::InsertEntry { .. } => CommandProperties {
                repeat: RepeatBehavior::Record,
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Exit insert mode: skip — the insert text replay is handled separately.
            Self::InsertExit => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Visual sub-commands (enter, exit, switch, swap, reselect): skip repeat.
            Self::Visual(_) => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Operator applied to visual selection: record for repeat only if mutating.
            Self::OperatorSelection { operator, .. } => CommandProperties {
                repeat: if operator.is_mutating() {
                    RepeatBehavior::Record
                } else {
                    RepeatBehavior::Skip
                },
                jump: false,
                keep_visual: false,
                move_point: true,
                suppress_operator: false,
                idempotent: false,
            },

            // YankTrimmed: skip repeat (non-mutating yank), exits visual.
            Self::YankTrimmed { .. } => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: false,
                move_point: true,
                suppress_operator: false,
                idempotent: false,
            },

            // Text object in visual mode: skip repeat, keep visual selection.
            Self::VisualTextObject { .. } => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: true,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Macros manage their own repeat semantics.
            Self::Macro(_) => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Enter select mode: skip repeat.
            Self::SelectEnter { .. } => CommandProperties {
                repeat: RepeatBehavior::Skip,
                jump: false,
                keep_visual: false,
                move_point: false,
                suppress_operator: false,
                idempotent: false,
            },

            // Surround operations: record for repeat.
            Self::SurroundAdd { .. }
            | Self::SurroundDelete { .. }
            | Self::SurroundChange { .. } => CommandProperties {
                repeat: RepeatBehavior::Record,
                jump: false,
                keep_visual: false,
                move_point: true,
                suppress_operator: false,
                idempotent: false,
            },
        }
    }

    /// Zero-allocation discriminant tag for provenance tracking.
    ///
    /// Returns a `&'static str` naming this command's variant. Used instead of
    /// `format!("{command}")` in hot paths (e.g., effect provenance tagging)
    /// to eliminate per-keystroke heap allocations.
    ///
    /// The returned tag is a stable, short variant name — not a human-readable
    /// description. For detailed logging use the `Display` impl instead.
    #[inline]
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::Motion { .. } => "Motion",
            Self::OperatorMotion { .. } => "OperatorMotion",
            Self::OperatorTextObject { .. } => "OperatorTextObject",
            Self::OperatorLine { .. } => "OperatorLine",
            Self::OperatorMark { .. } => "OperatorMark",
            Self::Action { .. } => "Action",
            Self::CharCommand { .. } => "CharCommand",
            Self::Sneak { .. } => "Sneak",
            Self::Mark { .. } => "Mark",
            Self::ModeSwitch { .. } => "ModeSwitch",
            Self::Prefix { .. } => "Prefix",
            Self::Insert(_) => "Insert",
            Self::InsertEntry { .. } => "InsertEntry",
            Self::InsertExit => "InsertExit",
            Self::Visual(_) => "Visual",
            Self::OperatorSelection { .. } => "OperatorSelection",
            Self::YankTrimmed { .. } => "YankTrimmed",
            Self::VisualTextObject { .. } => "VisualTextObject",
            Self::Macro(_) => "Macro",
            Self::SelectEnter { .. } => "SelectEnter",
            Self::SurroundAdd { .. } => "SurroundAdd",
            Self::SurroundDelete { .. } => "SurroundDelete",
            Self::SurroundChange { .. } => "SurroundChange",
        }
    }

    /// Return a stable `u16` discriminant for this command variant.
    ///
    /// The discriminant is unique per `Command` variant and stable across
    /// instances — two commands of the same variant always return the same value,
    /// regardless of their field contents. Different variants always return
    /// different values.
    ///
    /// Used by [`PropertyOverlay`](crate::execution::PropertyOverlay) to key
    /// runtime property overrides without depending on the full enum layout.
    #[must_use]
    pub const fn discriminant(&self) -> u16 {
        match self {
            Self::Motion { .. } => 0,
            Self::OperatorMotion { .. } => 1,
            Self::OperatorTextObject { .. } => 2,
            Self::OperatorLine { .. } => 3,
            Self::OperatorMark { .. } => 4,
            Self::Action { .. } => 5,
            Self::CharCommand { .. } => 6,
            Self::Sneak { .. } => 18,
            Self::Mark { .. } => 7,
            Self::ModeSwitch { .. } => 8,
            Self::Prefix { .. } => 9,
            Self::Insert(_) => 10,
            Self::InsertEntry { .. } => 11,
            Self::InsertExit => 12,
            Self::Visual(_) => 13,
            Self::OperatorSelection { .. } => 14,
            Self::VisualTextObject { .. } => 15,
            Self::Macro(_) => 16,
            Self::SelectEnter { .. } => 17,
            Self::SurroundAdd { .. } => 19,
            Self::SurroundDelete { .. } => 20,
            Self::SurroundChange { .. } => 21,
            Self::YankTrimmed { .. } => 22,
        }
    }

    /// Return a copy of this command with the count replaced.
    ///
    /// Used for dot repeat with count override (e.g., `3.`).
    /// Matches on `&self` and copies individual `Copy` fields —
    /// avoids cloning the entire enum just to replace one field.
    pub fn with_count(&self, new_count: NonZeroU32) -> Self {
        match self {
            Self::Motion {
                motion,
                explicit_count,
                ..
            } => Self::Motion {
                count: new_count,
                motion: *motion,
                explicit_count: *explicit_count,
            },
            Self::OperatorMotion {
                register,
                operator,
                motion,
                force_type,
                ..
            } => Self::OperatorMotion {
                count: new_count,
                register: *register,
                operator: *operator,
                motion: *motion,
                force_type: *force_type,
            },
            Self::OperatorTextObject {
                register,
                operator,
                textobject,
                ..
            } => Self::OperatorTextObject {
                count: new_count,
                register: *register,
                operator: *operator,
                textobject: *textobject,
            },
            Self::OperatorLine {
                register, operator, ..
            } => Self::OperatorLine {
                count: new_count,
                register: *register,
                operator: *operator,
            },
            Self::OperatorMark {
                register,
                operator,
                mark,
                mark_type,
                ..
            } => Self::OperatorMark {
                count: new_count,
                register: *register,
                operator: *operator,
                mark: *mark,
                mark_type: *mark_type,
            },
            Self::Action {
                action, register, ..
            } => Self::Action {
                count: new_count,
                register: *register,
                action: *action,
            },
            Self::CharCommand {
                register,
                operator,
                command,
                target,
                ..
            } => Self::CharCommand {
                count: new_count,
                register: *register,
                operator: *operator,
                command: *command,
                target: target.clone(),
            },
            Self::Sneak {
                register,
                operator,
                c1,
                c2,
                forward,
                ..
            } => Self::Sneak {
                count: new_count,
                register: *register,
                operator: *operator,
                c1: *c1,
                c2: *c2,
                forward: *forward,
            },
            Self::Mark {
                mark_type, mark, ..
            } => Self::Mark {
                count: new_count,
                mark_type: *mark_type,
                mark: *mark,
            },
            Self::Prefix {
                register, command, ..
            } => Self::Prefix {
                count: new_count,
                register: *register,
                command: *command,
            },
            Self::InsertEntry {
                entry_type,
                register,
                ..
            } => Self::InsertEntry {
                count: new_count,
                entry_type: *entry_type,
                register: *register,
            },
            // SelectEnter has no count field — count is intentionally ignored.
            Self::SelectEnter { visual_type } => Self::SelectEnter {
                visual_type: *visual_type,
            },
            // No-count variants: clone as-is (rare path during dot-repeat)
            other => other.clone(),
        }
    }

    /// Return a copy of this command with the register replaced.
    ///
    /// Used for dot-repeat register auto-increment: when repeating a command
    /// that used numbered register 0-8, the register increments by 1 each
    /// time `.` is pressed. Register 9 stays at 9.
    ///
    /// Follows the same `match &self` + field-copy pattern as `with_count()`.
    pub fn with_register(&self, new_register: Option<RegisterName>) -> Self {
        match self {
            Self::OperatorMotion {
                count,
                operator,
                motion,
                force_type,
                ..
            } => Self::OperatorMotion {
                count: *count,
                register: new_register,
                operator: *operator,
                motion: *motion,
                force_type: *force_type,
            },
            Self::OperatorTextObject {
                count,
                operator,
                textobject,
                ..
            } => Self::OperatorTextObject {
                count: *count,
                register: new_register,
                operator: *operator,
                textobject: *textobject,
            },
            Self::OperatorLine {
                count, operator, ..
            } => Self::OperatorLine {
                count: *count,
                register: new_register,
                operator: *operator,
            },
            Self::OperatorMark {
                count,
                operator,
                mark,
                mark_type,
                ..
            } => Self::OperatorMark {
                count: *count,
                register: new_register,
                operator: *operator,
                mark: *mark,
                mark_type: *mark_type,
            },
            Self::Action { count, action, .. } => Self::Action {
                count: *count,
                register: new_register,
                action: *action,
            },
            Self::CharCommand {
                count,
                operator,
                command,
                target,
                ..
            } => Self::CharCommand {
                count: *count,
                register: new_register,
                operator: *operator,
                command: *command,
                target: target.clone(),
            },
            Self::Sneak {
                count,
                operator,
                c1,
                c2,
                forward,
                ..
            } => Self::Sneak {
                count: *count,
                register: new_register,
                operator: *operator,
                c1: *c1,
                c2: *c2,
                forward: *forward,
            },
            Self::Prefix { count, command, .. } => Self::Prefix {
                count: *count,
                register: new_register,
                command: *command,
            },
            Self::OperatorSelection { operator, .. } => Self::OperatorSelection {
                register: new_register,
                operator: *operator,
            },
            Self::YankTrimmed { .. } => Self::YankTrimmed {
                register: new_register,
            },
            Self::VisualTextObject {
                count, textobject, ..
            } => Self::VisualTextObject {
                count: *count,
                register: new_register,
                textobject: *textobject,
            },
            // Variants without a register field: clone as-is.
            other => other.clone(),
        }
    }
}

impl std::fmt::Display for Command {
    /// Human-readable one-line summary for logging and provenance.
    ///
    /// Designed to be immediately understandable in log output:
    /// - `Down`                      (j motion)
    /// - `Change(inner-Paren)`       (ci( operator+textobject)
    /// - `Delete(3-lines)`           (3dd)
    /// - `Delete(WordForward)`       (dw operator+motion)
    /// - `Replace('x')`             (rx)
    /// - `Action(Undo)`              (u)
    /// - `InsertEntry(Append)`       (a)
    /// - `InsertExit`                (Esc in insert)
    /// - `Insert(Char('a'))`         (typing 'a' in insert)
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Motion { motion, .. } => write!(f, "{motion}"),
            Self::OperatorMotion {
                operator, motion, ..
            } => {
                write!(f, "{operator}({motion})")
            }
            Self::OperatorTextObject {
                operator,
                textobject,
                ..
            } => {
                let scope = match textobject.scope {
                    super::types::TextObjectScope::Inner => "inner",
                    super::types::TextObjectScope::Around => "around",
                };
                write!(f, "{operator}({scope}-{})", textobject.kind)
            }
            Self::OperatorLine {
                operator, count, ..
            } => {
                if count.get() == 1 {
                    write!(f, "{operator}(line)")
                } else {
                    write!(f, "{operator}({count}-lines)")
                }
            }
            Self::OperatorMark {
                operator,
                mark,
                mark_type,
                ..
            } => {
                write!(f, "{operator}({mark_type}'{mark}')")
            }
            Self::Action { action, .. } => write!(f, "{action}"),
            Self::CharCommand {
                command,
                target,
                operator,
                ..
            } => {
                if let Some(op) = operator {
                    write!(f, "{op}({command}'{target}')")
                } else {
                    write!(f, "{command}('{target}')")
                }
            }
            Self::Sneak {
                c1,
                c2,
                forward,
                operator,
                ..
            } => {
                let dir = if *forward { "s" } else { "S" };
                if let Some(op) = operator {
                    write!(f, "{op}({dir}'{c1}{c2}')")
                } else {
                    write!(f, "{dir}('{c1}{c2}')")
                }
            }
            Self::Mark {
                mark_type, mark, ..
            } => {
                write!(f, "{mark_type}('{mark}')")
            }
            Self::ModeSwitch { mode } => write!(f, "ModeSwitch({mode})"),
            Self::Prefix { command, .. } => write!(f, "{command:?}"),
            Self::Insert(kind) => write!(f, "Insert({kind:?})"),
            Self::InsertEntry { entry_type, .. } => {
                write!(f, "InsertEntry({entry_type:?})")
            }
            Self::InsertExit => write!(f, "InsertExit"),
            Self::Visual(kind) => write!(f, "Visual({kind:?})"),
            Self::OperatorSelection { operator, .. } => {
                write!(f, "{operator}(selection)")
            }
            Self::YankTrimmed { .. } => write!(f, "YankTrimmed(selection)"),
            Self::VisualTextObject { textobject, .. } => {
                let scope = match textobject.scope {
                    super::types::TextObjectScope::Inner => "inner",
                    super::types::TextObjectScope::Around => "around",
                };
                write!(f, "VisualTextObject({scope}-{})", textobject.kind)
            }
            Self::Macro(kind) => match kind {
                MacroKind::Record { register } => {
                    write!(f, "MacroRecord('{}')", register.char())
                }
                MacroKind::Stop => write!(f, "MacroStop"),
                MacroKind::Play { register, count } => {
                    write!(f, "MacroPlay('{}' x{})", register.char(), count)
                }
                MacroKind::RepeatLastEx { count } => {
                    write!(f, "RepeatLastEx(x{count})")
                }
            },
            Self::SelectEnter { visual_type } => {
                write!(f, "SelectEnter({visual_type:?})")
            }
            Self::SurroundAdd { char, .. } => {
                write!(f, "SurroundAdd('{char}')")
            }
            Self::SurroundDelete { char } => {
                write!(f, "SurroundDelete('{char}')")
            }
            Self::SurroundChange { old_char, new_char } => {
                write!(f, "SurroundChange('{old_char}'->'{new_char}')")
            }
        }
    }
}

impl Action {
    /// Check if this action should preserve the pre-operation mark `.`.
    ///
    /// In Neovim, `Ctrl-A`/`Ctrl-X` (increment/decrement) modify text but
    /// do NOT update mark `.` — it retains its pre-operation value.
    /// This is because they go through `buf_addsub()` which doesn't call
    /// the operator-level mark update path.
    #[must_use]
    pub const fn preserves_mark_dot(&self) -> bool {
        matches!(self, Self::IncrementNumber | Self::DecrementNumber)
    }

    /// Check if this action modifies text.
    ///
    /// Undo/Redo restore history snapshots — they are state transitions,
    /// NOT text mutations. Per `:help .` they must not overwrite dot-repeat.
    #[must_use]
    pub const fn is_mutating(&self) -> bool {
        match self {
            // Text-modifying actions (repeatable via dot)
            Self::DeleteChar
            | Self::DeleteCharBack
            | Self::Put
            | Self::PutBefore
            | Self::Join
            | Self::SwapCase
            | Self::DeleteToEnd
            | Self::ChangeToEnd
            | Self::Substitute
            | Self::BlockInsert
            | Self::BlockAppend
            | Self::IncrementNumber
            | Self::DecrementNumber
            | Self::PutAfterCursorAfter
            | Self::PutBeforeCursorAfter
            | Self::PutIndentAfter
            | Self::PutIndentBefore
            | Self::RepeatSubstitute
            | Self::RepeatSubstituteGlobal => true,
            // State transitions — NOT text mutations
            Self::Undo | Self::Redo | Self::UndoLine => false,
            // Non-modifying actions (IntentRepeat is meta — like dot-repeat, it
            // replays a prior command; the replayed command is what gets recorded)
            Self::YankLine
            | Self::JumpOlder
            | Self::JumpNewer
            | Self::ShowFileInfo
            | Self::KeywordLookup
            | Self::IntentRepeat
            | Self::AlternateFile
            | Self::AddNextMatchCursor
            | Self::AddPrevMatchCursor
            | Self::SkipMatchCursor => false,
        }
    }
}

/// Convert an `Option<u32>` parser count to `NonZeroU32`, defaulting to 1.
///
/// Grammar handlers store counts as `Option<u32>` (None = not typed).
/// This converts to the type-safe `NonZeroU32` for `Command` fields.
/// Input is clamped to `MAX_COUNT` to match `compute_count` behaviour.
#[inline]
#[must_use]
pub const fn count_or_default(count: Option<u32>) -> NonZeroU32 {
    match count {
        Some(c) => {
            let clamped = if c > MAX_COUNT { MAX_COUNT } else { c };
            match NonZeroU32::new(clamped) {
                Some(n) => n,
                None => NonZeroU32::MIN,
            }
        }
        None => NonZeroU32::MIN,
    }
}

/// Maximum reasonable count to prevent overflow and runaway repeats.
const MAX_COUNT: u32 = 10_000;

/// Compute effective count from two optional counts.
///
/// The total is the product of both counts, with each defaulting to 1.
/// Result is clamped to reasonable bounds and always >= 1.
#[must_use]
pub const fn compute_count(count1: Option<u32>, count2: Option<u32>) -> NonZeroU32 {
    let c1 = match count1 {
        Some(c) => c,
        None => 1,
    };
    let c2 = match count2 {
        Some(c) => c,
        None => 1,
    };
    // Clamp to prevent overflow, max reasonable count
    let product = c1.saturating_mul(c2);
    let clamped = if product < MAX_COUNT {
        product
    } else {
        MAX_COUNT
    };
    // SAFETY: product of two values where each defaults to 1 is always >= 1,
    // and MAX_COUNT is > 0.
    match NonZeroU32::new(clamped) {
        Some(n) => n,
        None => NonZeroU32::MIN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{
        CompletionKind, InsertEntryType, Mode, RegisterName, RepeatBehavior, VisualType,
    };

    // ═══════════════════════════════════════════════════════════════════════
    // compute_count
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn compute_count_both_none_defaults_to_one() {
        let result = compute_count(None, None);
        assert_eq!(result.get(), 1);
    }

    #[test]
    fn compute_count_first_some_second_none() {
        let result = compute_count(Some(5), None);
        assert_eq!(result.get(), 5);
    }

    #[test]
    fn compute_count_first_none_second_some() {
        let result = compute_count(None, Some(7));
        assert_eq!(result.get(), 7);
    }

    #[test]
    fn compute_count_both_present_multiplied() {
        let result = compute_count(Some(3), Some(4));
        assert_eq!(result.get(), 12);
    }

    #[test]
    fn compute_count_clamps_at_max_count() {
        // MAX_COUNT is 10_000
        let result = compute_count(Some(200), Some(200));
        // 200 * 200 = 40_000, which exceeds MAX_COUNT (10_000)
        assert_eq!(result.get(), 10_000);
    }

    #[test]
    fn compute_count_exact_max_is_clamped() {
        // Exactly MAX_COUNT should be clamped (product < MAX_COUNT is false)
        let result = compute_count(Some(10_000), Some(1));
        assert_eq!(result.get(), 10_000);
    }

    #[test]
    fn compute_count_just_below_max() {
        let result = compute_count(Some(9_999), Some(1));
        assert_eq!(result.get(), 9_999);
    }

    #[test]
    fn compute_count_saturating_mul_overflow() {
        // u32::MAX * 2 would overflow, but saturating_mul clamps to u32::MAX,
        // which then gets clamped to MAX_COUNT (10_000)
        let result = compute_count(Some(u32::MAX), Some(2));
        assert_eq!(result.get(), 10_000);
    }

    #[test]
    fn compute_count_zero_count1_produces_zero_product_then_min() {
        // 0 * 1 = 0, which fails NonZeroU32::new, so falls to NonZeroU32::MIN (1)
        let result = compute_count(Some(0), None);
        assert_eq!(result.get(), 1);
    }

    #[test]
    fn compute_count_zero_count2_produces_zero_product_then_min() {
        let result = compute_count(None, Some(0));
        assert_eq!(result.get(), 1);
    }

    #[test]
    fn compute_count_both_zero_produces_min() {
        let result = compute_count(Some(0), Some(0));
        assert_eq!(result.get(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // count_or_default
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn count_or_default_none_is_one() {
        assert_eq!(count_or_default(None).get(), 1);
    }

    #[test]
    fn count_or_default_some_nonzero() {
        assert_eq!(count_or_default(Some(42)).get(), 42);
    }

    #[test]
    fn count_or_default_some_zero_is_one() {
        // 0 can't be NonZeroU32, so it falls through to NonZeroU32::MIN
        assert_eq!(count_or_default(Some(0)).get(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // InsertKind::is_mutating
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn insert_char_is_mutating() {
        assert!(InsertKind::Char { char: 'a' }.is_mutating());
    }

    #[test]
    fn insert_backspace_is_mutating() {
        assert!(InsertKind::Backspace.is_mutating());
    }

    #[test]
    fn insert_oneshot_is_not_mutating() {
        assert!(!InsertKind::OneShot.is_mutating());
    }

    #[test]
    fn insert_nop_is_not_mutating() {
        assert!(!InsertKind::Nop.is_mutating());
    }

    #[test]
    fn insert_break_undo_is_not_mutating() {
        assert!(!InsertKind::BreakUndoSequence.is_mutating());
    }

    #[test]
    fn insert_toggle_replace_is_not_mutating() {
        assert!(!InsertKind::ToggleReplace.is_mutating());
    }

    #[test]
    fn insert_digraph_is_mutating() {
        assert!(InsertKind::Digraph { c1: 'a', c2: 'e' }.is_mutating());
    }

    #[test]
    fn insert_outdent_temporary_is_mutating() {
        assert!(InsertKind::OutdentTemporary.is_mutating());
    }

    #[test]
    fn insert_outdent_clear_is_mutating() {
        assert!(InsertKind::OutdentClear.is_mutating());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // InsertKind::register
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn insert_register_returns_register() {
        let reg = RegisterName::new_unchecked('a');
        let kind = InsertKind::Register { register: reg };
        assert_eq!(kind.register(), Some(reg));
    }

    #[test]
    fn insert_char_register_returns_none() {
        assert_eq!(InsertKind::Char { char: 'x' }.register(), None);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // InsertKind::is_content_dependent
    // ═══════════════════════════════════════════════════════════════════════

    // ── Always content-dependent ─────────────────────────────────────────

    #[test]
    fn cd_backspace() {
        assert!(InsertKind::Backspace.is_content_dependent(false, false));
    }

    #[test]
    fn cd_delete_word() {
        assert!(InsertKind::DeleteWord.is_content_dependent(false, false));
    }

    #[test]
    fn cd_delete_to_start() {
        assert!(InsertKind::DeleteToStart.is_content_dependent(false, false));
    }

    #[test]
    fn cd_delete_under() {
        assert!(InsertKind::DeleteUnder.is_content_dependent(false, false));
    }

    #[test]
    fn cd_indent() {
        assert!(InsertKind::Indent.is_content_dependent(false, false));
    }

    #[test]
    fn cd_outdent() {
        assert!(InsertKind::Outdent.is_content_dependent(false, false));
    }

    #[test]
    fn cd_outdent_temporary() {
        assert!(InsertKind::OutdentTemporary.is_content_dependent(false, false));
    }

    #[test]
    fn cd_outdent_clear() {
        assert!(InsertKind::OutdentClear.is_content_dependent(false, false));
    }

    #[test]
    fn cd_copy_char_below() {
        assert!(InsertKind::CopyCharBelow.is_content_dependent(false, false));
    }

    #[test]
    fn cd_copy_char_above() {
        assert!(InsertKind::CopyCharAbove.is_content_dependent(false, false));
    }

    #[test]
    fn cd_expression_result() {
        assert!(InsertKind::ExpressionResult {
            expression: String::new(),
        }
        .is_content_dependent(false, false));
    }

    // ── Char: newline is always content-dependent ────────────────────────

    #[test]
    fn cd_char_newline_no_settings() {
        assert!(InsertKind::Char { char: '\n' }.is_content_dependent(false, false));
    }

    #[test]
    fn cd_char_newline_with_expandtab() {
        assert!(InsertKind::Char { char: '\n' }.is_content_dependent(true, false));
    }

    #[test]
    fn cd_char_newline_with_auto_pairs() {
        assert!(InsertKind::Char { char: '\n' }.is_content_dependent(false, true));
    }

    // ── Char: tab depends on expandtab ───────────────────────────────────

    #[test]
    fn cd_char_tab_expandtab_true() {
        assert!(InsertKind::Char { char: '\t' }.is_content_dependent(true, false));
    }

    #[test]
    fn cd_char_tab_expandtab_false() {
        assert!(!InsertKind::Char { char: '\t' }.is_content_dependent(false, false));
    }

    #[test]
    fn cd_char_tab_both_settings() {
        // expandtab takes priority for tab
        assert!(InsertKind::Char { char: '\t' }.is_content_dependent(true, true));
    }

    // ── Char: regular chars depend on auto_pairs ─────────────────────────

    #[test]
    fn cd_char_regular_auto_pairs_true() {
        assert!(InsertKind::Char { char: '(' }.is_content_dependent(false, true));
    }

    #[test]
    fn cd_char_regular_auto_pairs_false() {
        assert!(!InsertKind::Char { char: '(' }.is_content_dependent(false, false));
    }

    #[test]
    fn cd_char_plain_no_settings() {
        assert!(!InsertKind::Char { char: 'a' }.is_content_dependent(false, false));
    }

    #[test]
    fn cd_char_plain_auto_pairs_true() {
        assert!(InsertKind::Char { char: 'a' }.is_content_dependent(false, true));
    }

    // ── Always position-independent ──────────────────────────────────────

    #[test]
    fn pi_literal_char() {
        assert!(!InsertKind::LiteralChar { char: '\t' }.is_content_dependent(true, true));
    }

    #[test]
    fn pi_register() {
        let reg = RegisterName::new_unchecked('a');
        assert!(!InsertKind::Register { register: reg }.is_content_dependent(true, true));
    }

    #[test]
    fn pi_last_inserted() {
        assert!(!InsertKind::LastInserted.is_content_dependent(true, true));
    }

    #[test]
    fn pi_last_inserted_and_exit() {
        assert!(!InsertKind::LastInsertedAndExit.is_content_dependent(true, true));
    }

    #[test]
    fn pi_paste() {
        assert!(!InsertKind::Paste.is_content_dependent(true, true));
    }

    #[test]
    fn pi_one_shot() {
        assert!(!InsertKind::OneShot.is_content_dependent(true, true));
    }

    #[test]
    fn pi_break_undo_sequence() {
        assert!(!InsertKind::BreakUndoSequence.is_content_dependent(true, true));
    }

    #[test]
    fn pi_dont_sync_undo() {
        assert!(!InsertKind::DontSyncUndo.is_content_dependent(true, true));
    }

    #[test]
    fn pi_nop() {
        assert!(!InsertKind::Nop.is_content_dependent(true, true));
    }

    #[test]
    fn pi_request_completion() {
        assert!(!InsertKind::RequestCompletion {
            kind: CompletionKind::Line,
        }
        .is_content_dependent(true, true));
    }

    #[test]
    fn pi_toggle_replace() {
        assert!(!InsertKind::ToggleReplace.is_content_dependent(true, true));
    }

    #[test]
    fn pi_host_inserted() {
        assert!(!InsertKind::HostInserted.is_content_dependent(true, true));
    }

    #[test]
    fn pi_digraph() {
        assert!(!InsertKind::Digraph { c1: 'a', c2: 'e' }.is_content_dependent(true, true));
    }

    #[test]
    fn pi_toggle_langmap() {
        assert!(!InsertKind::ToggleLangmap.is_content_dependent(true, true));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::count
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn command_motion_count() {
        let cmd = Command::Motion {
            count: NonZeroU32::new(5).unwrap(),
            motion: Motion::Down,
            explicit_count: true,
        };
        assert_eq!(cmd.count().get(), 5);
    }

    #[test]
    fn command_insert_count_is_one() {
        let cmd = Command::Insert(InsertKind::Char { char: 'a' });
        assert_eq!(cmd.count().get(), 1);
    }

    #[test]
    fn command_insert_exit_count_is_one() {
        assert_eq!(Command::InsertExit.count().get(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::is_mutating
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn motion_is_not_mutating() {
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        assert!(!cmd.is_mutating());
    }

    #[test]
    fn operator_delete_motion_is_mutating() {
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Delete,
            motion: Motion::WordForward,
            force_type: None,
        };
        assert!(cmd.is_mutating());
    }

    #[test]
    fn operator_yank_motion_is_not_mutating() {
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Yank,
            motion: Motion::WordForward,
            force_type: None,
        };
        assert!(!cmd.is_mutating());
    }

    #[test]
    fn insert_entry_is_mutating() {
        let cmd = Command::InsertEntry {
            count: NonZeroU32::MIN,
            entry_type: InsertEntryType::BeforeCursor,
            register: None,
        };
        assert!(cmd.is_mutating());
    }

    #[test]
    fn macro_stop_is_not_mutating() {
        let cmd = Command::Macro(MacroKind::Stop);
        assert!(!cmd.is_mutating());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::is_content_dependent
    // ═══════════════════════════════════════════════════════════════════════

    // ── Position-independent variants ───────────────────────────────────

    #[test]
    fn cd_motion_is_content_dependent() {
        // Standalone motions are CD so each cursor in a multi-cursor session
        // independently computes its target against its own text geometry.
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_mode_switch_is_not_content_dependent() {
        let cmd = Command::ModeSwitch { mode: Mode::Normal };
        assert!(!cmd.is_content_dependent());
    }

    #[test]
    fn cd_mark_is_not_content_dependent() {
        let cmd = Command::Mark {
            count: NonZeroU32::MIN,
            mark_type: super::super::types::MarkType::Set,
            mark: crate::primitives::MarkName::new('a').unwrap(),
        };
        assert!(!cmd.is_content_dependent());
    }

    #[test]
    fn cd_visual_is_not_content_dependent() {
        assert!(!Command::Visual(VisualKind::Exit).is_content_dependent());
    }

    #[test]
    fn cd_insert_is_not_content_dependent() {
        assert!(!Command::Insert(InsertKind::Char { char: 'a' }).is_content_dependent());
    }

    #[test]
    fn cd_insert_exit_is_not_content_dependent() {
        assert!(!Command::InsertExit.is_content_dependent());
    }

    #[test]
    fn cd_macro_is_not_content_dependent() {
        assert!(!Command::Macro(MacroKind::Stop).is_content_dependent());
    }

    #[test]
    fn cd_select_enter_is_not_content_dependent() {
        let cmd = Command::SelectEnter {
            visual_type: VisualType::Char,
        };
        assert!(!cmd.is_content_dependent());
    }

    #[test]
    fn cd_prefix_is_not_content_dependent() {
        let cmd = Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::ScrollCenter,
        };
        assert!(!cmd.is_content_dependent());
    }

    #[test]
    fn cd_visual_text_object_is_not_content_dependent() {
        let cmd = Command::VisualTextObject {
            count: NonZeroU32::MIN,
            textobject: TextObject {
                scope: super::super::types::TextObjectScope::Inner,
                kind: super::super::types::TextObjectKind::Word,
                seek: None,
            },
            register: None,
        };
        assert!(!cmd.is_content_dependent());
    }

    // ── OperatorMotion composition ──────────────────────────────────────

    #[test]
    fn cd_operator_motion_cd_motion() {
        // Non-reading operator + CD motion → true
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Delete,
            motion: Motion::WordForward,
            force_type: None,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_operator_motion_reading_operator() {
        // Content-reading operator + PI motion → true
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::ToggleCase,
            motion: Motion::Down,
            force_type: None,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_operator_motion_delete_down() {
        // Delete (mutating) + PI motion → true (range width is content-dependent)
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Delete,
            motion: Motion::Down,
            force_type: None,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_operator_motion_yank_pi() {
        // Yank (non-reading) + PI motion → false
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Yank,
            motion: Motion::Down,
            force_type: None,
        };
        assert!(!cmd.is_content_dependent());
    }

    #[test]
    fn cd_operator_motion_indent_pi() {
        // Indent (content-reading) + PI motion → true
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Indent,
            motion: Motion::Down,
            force_type: None,
        };
        assert!(cmd.is_content_dependent());
    }

    // ── OperatorTextObject: always CD ───────────────────────────────────

    #[test]
    fn cd_operator_text_object_always_true() {
        let cmd = Command::OperatorTextObject {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Delete,
            textobject: TextObject {
                scope: super::super::types::TextObjectScope::Inner,
                kind: super::super::types::TextObjectKind::Word,
                seek: None,
            },
        };
        assert!(cmd.is_content_dependent());
    }

    // ── OperatorLine: depends on operator ───────────────────────────────

    #[test]
    fn cd_operator_line_reading() {
        let cmd = Command::OperatorLine {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::ToggleCase,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_operator_line_delete() {
        // Delete (mutating) linewise → true (line byte widths differ)
        let cmd = Command::OperatorLine {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Delete,
        };
        assert!(cmd.is_content_dependent());
    }

    // ── OperatorMark: depends on operator ───────────────────────────────

    #[test]
    fn cd_operator_mark_reading() {
        let cmd = Command::OperatorMark {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Uppercase,
            mark: crate::primitives::MarkName::new('a').unwrap(),
            mark_type: super::super::types::MarkType::JumpLine,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_operator_mark_non_reading() {
        let cmd = Command::OperatorMark {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Yank,
            mark: crate::primitives::MarkName::new('a').unwrap(),
            mark_type: super::super::types::MarkType::JumpLine,
        };
        assert!(!cmd.is_content_dependent());
    }

    // ── OperatorSelection: depends on operator ──────────────────────────

    #[test]
    fn cd_operator_selection_reading() {
        let cmd = Command::OperatorSelection {
            register: None,
            operator: Operator::Lowercase,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_operator_selection_change() {
        // Change (mutating) on visual selection → true (selection widths differ)
        let cmd = Command::OperatorSelection {
            register: None,
            operator: Operator::Change,
        };
        assert!(cmd.is_content_dependent());
    }

    // ── CharCommand composition ─────────────────────────────────────────

    #[test]
    fn cd_char_command_with_operator() {
        let cmd = Command::CharCommand {
            count: NonZeroU32::MIN,
            register: None,
            operator: Some(Operator::Delete),
            command: CharCommand::FindForward,
            target: CompactString::from("x"),
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_char_command_replace_no_operator() {
        let cmd = Command::CharCommand {
            count: NonZeroU32::MIN,
            register: None,
            operator: None,
            command: CharCommand::Replace,
            target: CompactString::from("x"),
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_char_command_find_no_operator() {
        let cmd = Command::CharCommand {
            count: NonZeroU32::MIN,
            register: None,
            operator: None,
            command: CharCommand::FindForward,
            target: CompactString::from("x"),
        };
        // Standalone find is CD: target char appears at different positions
        // per cursor line — each cursor must search independently.
        assert!(cmd.is_content_dependent());
    }

    // ── Sneak composition ───────────────────────────────────────────────

    #[test]
    fn cd_sneak_with_operator() {
        let cmd = Command::Sneak {
            count: NonZeroU32::MIN,
            register: None,
            operator: Some(Operator::Delete),
            c1: 'a',
            c2: 'b',
            forward: true,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_sneak_no_operator() {
        let cmd = Command::Sneak {
            count: NonZeroU32::MIN,
            register: None,
            operator: None,
            c1: 'a',
            c2: 'b',
            forward: true,
        };
        assert!(!cmd.is_content_dependent());
    }

    // ── Surround: always CD ─────────────────────────────────────────────

    #[test]
    fn cd_surround_add() {
        let cmd = Command::SurroundAdd {
            count: NonZeroU32::MIN,
            motion: Some(Motion::WordForward),
            textobject: None,
            char: '(',
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_surround_delete() {
        let cmd = Command::SurroundDelete { char: '(' };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_surround_change() {
        let cmd = Command::SurroundChange {
            old_char: '(',
            new_char: '[',
        };
        assert!(cmd.is_content_dependent());
    }

    // ── Action: delegates ───────────────────────────────────────────────

    #[test]
    fn cd_action_swap_case() {
        let cmd = Command::Action {
            count: NonZeroU32::MIN,
            register: None,
            action: Action::SwapCase,
        };
        assert!(cmd.is_content_dependent());
    }

    #[test]
    fn cd_action_undo_not_cd() {
        let cmd = Command::Action {
            count: NonZeroU32::MIN,
            register: None,
            action: Action::Undo,
        };
        assert!(!cmd.is_content_dependent());
    }

    // ── InsertEntry: always CD ──────────────────────────────────────────

    #[test]
    fn pi_insert_entry() {
        let cmd = Command::InsertEntry {
            count: NonZeroU32::MIN,
            entry_type: InsertEntryType::BeforeCursor,
            register: None,
        };
        assert!(!cmd.is_content_dependent());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::discriminant uniqueness
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn discriminants_are_unique() {
        // Build one representative of each variant and verify no duplicates
        let commands: Vec<Command> = vec![
            Command::Motion {
                count: NonZeroU32::MIN,
                motion: Motion::Down,
                explicit_count: false,
            },
            Command::OperatorMotion {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Delete,
                motion: Motion::Down,
                force_type: None,
            },
            Command::OperatorTextObject {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Delete,
                textobject: TextObject {
                    scope: super::super::types::TextObjectScope::Inner,
                    kind: super::super::types::TextObjectKind::Word,
                    seek: None,
                },
            },
            Command::OperatorLine {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Delete,
            },
            Command::OperatorMark {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Delete,
                mark: crate::primitives::MarkName::new('a').unwrap(),
                mark_type: super::super::types::MarkType::JumpLine,
            },
            Command::Action {
                count: NonZeroU32::MIN,
                register: None,
                action: Action::Undo,
            },
            Command::CharCommand {
                count: NonZeroU32::MIN,
                register: None,
                operator: None,
                command: CharCommand::FindForward,
                target: CompactString::from("x"),
            },
            Command::Mark {
                count: NonZeroU32::MIN,
                mark_type: super::super::types::MarkType::Set,
                mark: crate::primitives::MarkName::new('a').unwrap(),
            },
            Command::ModeSwitch { mode: Mode::Normal },
            Command::Prefix {
                count: NonZeroU32::MIN,
                register: None,
                command: PrefixCommand::ScrollCenter,
            },
            Command::Insert(InsertKind::Char { char: 'a' }),
            Command::InsertEntry {
                count: NonZeroU32::MIN,
                entry_type: InsertEntryType::BeforeCursor,
                register: None,
            },
            Command::InsertExit,
            Command::Visual(VisualKind::Exit),
            Command::OperatorSelection {
                register: None,
                operator: Operator::Delete,
            },
            Command::VisualTextObject {
                count: NonZeroU32::MIN,
                textobject: TextObject {
                    scope: super::super::types::TextObjectScope::Inner,
                    kind: super::super::types::TextObjectKind::Word,
                    seek: None,
                },
                register: None,
            },
            Command::Macro(MacroKind::Stop),
            Command::SelectEnter {
                visual_type: VisualType::Char,
            },
        ];

        let discs: Vec<u16> = commands.iter().map(|c| c.discriminant()).collect();
        let mut sorted = discs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            discs.len(),
            sorted.len(),
            "discriminants are not unique: {discs:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::with_count
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn with_count_replaces_motion_count() {
        let cmd = Command::Motion {
            count: NonZeroU32::new(1).unwrap(),
            motion: Motion::Down,
            explicit_count: false,
        };
        let new_cmd = cmd.with_count(NonZeroU32::new(10).unwrap());
        assert_eq!(new_cmd.count().get(), 10);
    }

    #[test]
    fn with_count_preserves_motion() {
        let cmd = Command::Motion {
            count: NonZeroU32::new(1).unwrap(),
            motion: Motion::Up,
            explicit_count: true,
        };
        let new_cmd = cmd.with_count(NonZeroU32::new(5).unwrap());
        if let Command::Motion { motion, .. } = new_cmd {
            assert_eq!(motion, Motion::Up);
        } else {
            panic!("expected Motion variant");
        }
    }

    #[test]
    fn with_count_on_insert_exit_clones_as_is() {
        let cmd = Command::InsertExit;
        let new_cmd = cmd.with_count(NonZeroU32::new(5).unwrap());
        assert_eq!(new_cmd.count().get(), 1); // InsertExit has no count field
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::with_register
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn with_register_replaces_operator_register() {
        let cmd = Command::OperatorLine {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Delete,
        };
        let reg = RegisterName::new_unchecked('a');
        let new_cmd = cmd.with_register(Some(reg));
        assert_eq!(new_cmd.register(), Some(reg));
    }

    #[test]
    fn with_register_on_motion_clones_as_is() {
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        let reg = RegisterName::new_unchecked('a');
        let new_cmd = cmd.with_register(Some(reg));
        // Motions have no register — should clone unchanged
        assert_eq!(new_cmd.register(), None);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::properties
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn motion_properties_skip_repeat_keep_visual() {
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        let props = cmd.properties();
        assert_eq!(props.repeat, RepeatBehavior::Skip);
        assert!(props.keep_visual);
        assert!(props.idempotent);
    }

    #[test]
    fn delete_operator_records_for_repeat() {
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Delete,
            motion: Motion::WordForward,
            force_type: None,
        };
        let props = cmd.properties();
        assert_eq!(props.repeat, RepeatBehavior::Record);
    }

    #[test]
    fn yank_operator_skips_repeat() {
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Yank,
            motion: Motion::WordForward,
            force_type: None,
        };
        let props = cmd.properties();
        assert_eq!(props.repeat, RepeatBehavior::Skip);
    }

    #[test]
    fn insert_entry_records_for_repeat() {
        let cmd = Command::InsertEntry {
            count: NonZeroU32::MIN,
            entry_type: InsertEntryType::BeforeCursor,
            register: None,
        };
        assert_eq!(cmd.properties().repeat, RepeatBehavior::Record);
    }

    #[test]
    fn macro_skips_repeat() {
        let cmd = Command::Macro(MacroKind::Stop);
        assert_eq!(cmd.properties().repeat, RepeatBehavior::Skip);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PrefixCommand::is_mutating
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn join_no_space_is_mutating() {
        assert!(PrefixCommand::JoinNoSpace.is_mutating());
    }

    #[test]
    fn scroll_center_is_not_mutating() {
        assert!(!PrefixCommand::ScrollCenter.is_mutating());
    }

    #[test]
    fn insert_blank_below_is_mutating() {
        assert!(PrefixCommand::InsertBlankBelow.is_mutating());
    }

    #[test]
    fn fold_close_is_not_mutating() {
        assert!(!PrefixCommand::FoldClose.is_mutating());
    }

    #[test]
    fn window_split_is_not_mutating() {
        assert!(!PrefixCommand::WindowSplit.is_mutating());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::is_insert_subcommand
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn insert_char_is_subcommand() {
        let cmd = Command::Insert(InsertKind::Char { char: 'a' });
        assert!(cmd.is_insert_subcommand());
    }

    #[test]
    fn insert_exit_is_subcommand() {
        assert!(Command::InsertExit.is_insert_subcommand());
    }

    #[test]
    fn motion_is_not_subcommand() {
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        assert!(!cmd.is_insert_subcommand());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::Display
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn display_motion_shows_motion_name() {
        let cmd = Command::Motion {
            count: NonZeroU32::MIN,
            motion: Motion::Down,
            explicit_count: false,
        };
        let display = format!("{cmd}");
        assert!(!display.is_empty());
    }

    #[test]
    fn display_insert_exit() {
        assert_eq!(format!("{}", Command::InsertExit), "InsertExit");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Action::is_mutating
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn action_undo_is_not_mutating() {
        assert!(!Action::Undo.is_mutating());
    }

    #[test]
    fn action_redo_is_not_mutating() {
        assert!(!Action::Redo.is_mutating());
    }

    #[test]
    fn action_delete_char_is_mutating() {
        assert!(Action::DeleteChar.is_mutating());
    }

    #[test]
    fn action_put_is_mutating() {
        assert!(Action::Put.is_mutating());
    }

    #[test]
    fn action_yank_line_is_not_mutating() {
        assert!(!Action::YankLine.is_mutating());
    }

    #[test]
    fn action_intent_repeat_is_not_mutating() {
        assert!(!Action::IntentRepeat.is_mutating());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Command::may_produce_operator_filter
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn filter_operator_may_produce_operator_filter() {
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Filter,
            motion: Motion::Down,
            force_type: None,
        };
        assert!(cmd.may_produce_operator_filter());
    }

    #[test]
    fn delete_operator_does_not_produce_operator_filter() {
        let cmd = Command::OperatorMotion {
            count: NonZeroU32::MIN,
            register: None,
            operator: Operator::Delete,
            motion: Motion::Down,
            force_type: None,
        };
        assert!(!cmd.may_produce_operator_filter());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Multi-cursor classification integration tests
    //
    // These verify that real Command values produce the correct
    // content-dependent (CD) vs position-independent (PI) classification
    // for per-cursor re-execution decisions.
    // ═══════════════════════════════════════════════════════════════════════

    mod multi_cursor_classification {
        use super::*;
        use crate::grammar::types::{TextObjectKind, TextObjectScope};

        // ── 1. dw — OperatorMotion { Delete, WordForward } → CD ─────────
        #[test]
        fn dw_is_content_dependent() {
            let cmd = Command::OperatorMotion {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Delete,
                motion: Motion::WordForward,
                force_type: None,
            };
            // WordForward is content-dependent (scans word boundaries),
            // so operator+motion inherits CD even though Delete itself
            // is not content-reading.
            assert!(
                cmd.is_content_dependent(),
                "dw should be CD: WordForward scans text"
            );
        }

        // ── 2. dd — OperatorLine { Delete } → CD ───────────────────────
        #[test]
        fn dd_is_content_dependent() {
            let cmd = Command::OperatorLine {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Delete,
            };
            // Delete is mutating — line byte widths differ per cursor.
            assert!(
                cmd.is_content_dependent(),
                "dd should be CD: line byte widths differ per cursor"
            );
        }

        // ── 3. gUiw — OperatorTextObject { Uppercase, InnerWord } → CD ─
        #[test]
        fn g_upper_iw_is_content_dependent() {
            let cmd = Command::OperatorTextObject {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Uppercase,
                textobject: TextObject {
                    scope: TextObjectScope::Inner,
                    kind: TextObjectKind::Word,
                    seek: None,
                },
            };
            // All operator+textobject commands are CD — text objects
            // scan content to find boundaries.
            assert!(
                cmd.is_content_dependent(),
                "gUiw should be CD: text objects scan content"
            );
        }

        // ── 4. yy — OperatorLine { Yank } → PI ─────────────────────────
        #[test]
        fn yy_is_position_independent() {
            let cmd = Command::OperatorLine {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Yank,
            };
            // Yank is not content-reading.
            assert!(
                !cmd.is_content_dependent(),
                "yy should be PI: Yank is not content-reading"
            );
        }

        // ── 5. >> — OperatorLine { Indent } → CD ───────────────────────
        #[test]
        fn indent_line_is_content_dependent() {
            let cmd = Command::OperatorLine {
                count: NonZeroU32::MIN,
                register: None,
                operator: Operator::Indent,
            };
            // Indent IS content-reading: inserts at line_start which
            // doesn't shift by cursor delta for different column positions.
            assert!(
                cmd.is_content_dependent(),
                ">> should be CD: Indent is content-reading"
            );
        }

        // ── 6. ~ — Action { SwapCase } → CD ────────────────────────────
        #[test]
        fn swap_case_is_content_dependent() {
            let cmd = Command::Action {
                count: NonZeroU32::MIN,
                register: None,
                action: Action::SwapCase,
            };
            // SwapCase reads the character under cursor to flip its case.
            assert!(
                cmd.is_content_dependent(),
                "~ should be CD: reads char under cursor"
            );
        }

        // ── 7. p — Action { Put } → PI ─────────────────────────────────
        #[test]
        fn put_is_position_independent() {
            let cmd = Command::Action {
                count: NonZeroU32::MIN,
                register: None,
                action: Action::Put,
            };
            // Put pastes from a register — doesn't read document text.
            assert!(
                !cmd.is_content_dependent(),
                "p should be PI: pastes from register"
            );
        }

        // ── 8. rx — CharCommand { Replace, no operator } → CD ──────────
        #[test]
        fn replace_char_is_content_dependent() {
            let cmd = Command::CharCommand {
                count: NonZeroU32::MIN,
                register: None,
                operator: None,
                command: CharCommand::Replace,
                target: CompactString::new_inline("x"),
            };
            // Replace reads grapheme width at cursor position.
            assert!(
                cmd.is_content_dependent(),
                "rx should be CD: Replace reads grapheme width"
            );
        }

        // ── 9. fa — CharCommand { FindForward, no operator } → CD ──────
        #[test]
        fn find_forward_is_content_dependent_for_mc() {
            let cmd = Command::CharCommand {
                count: NonZeroU32::MIN,
                register: None,
                operator: None,
                command: CharCommand::FindForward,
                target: CompactString::new_inline("a"),
            };
            // Standalone find is CD: target char appears at different
            // positions per cursor line — each cursor must search independently.
            assert!(
                cmd.is_content_dependent(),
                "fa should be CD: per-cursor find for MC"
            );
        }

        // ── 10. dfa — CharCommand { FindForward, Some(Delete) } → CD ───
        #[test]
        fn delete_find_forward_is_content_dependent() {
            let cmd = Command::CharCommand {
                count: NonZeroU32::MIN,
                register: None,
                operator: Some(Operator::Delete),
                command: CharCommand::FindForward,
                target: CompactString::new_inline("a"),
            };
            // Operator present → CD: find distance varies by content.
            assert!(
                cmd.is_content_dependent(),
                "dfa should be CD: operator + char command"
            );
        }

        // ── 11. csab — SurroundChange → CD ──────────────────────────────
        #[test]
        fn surround_change_is_content_dependent() {
            let cmd = Command::SurroundChange {
                old_char: 'a',
                new_char: 'b',
            };
            // Surround operations scan content to find/modify delimiters.
            assert!(
                cmd.is_content_dependent(),
                "csab should be CD: scans for delimiters"
            );
        }

        // ── 12. Insert Char('x') with expandtab=false, auto_pairs=false → PI ──
        #[test]
        fn insert_char_no_expandtab_no_autopairs_is_position_independent() {
            let kind = InsertKind::Char { char: 'x' };
            // With both settings off, plain char insert is PI.
            assert!(
                !kind.is_content_dependent(false, false),
                "Char('x') with expandtab=false, auto_pairs=false should be PI"
            );
        }

        // ── 13. Insert Char('\n') → CD ──────────────────────────────────
        #[test]
        fn insert_newline_is_content_dependent() {
            let kind = InsertKind::Char { char: '\n' };
            // Newline triggers autoindent → reads surrounding indentation.
            assert!(
                kind.is_content_dependent(false, false),
                "Char('\\n') should be CD: triggers autoindent"
            );
        }

        // ── 14. Insert Backspace → CD ───────────────────────────────────
        #[test]
        fn insert_backspace_is_content_dependent() {
            let kind = InsertKind::Backspace;
            // Backspace reads surrounding text to determine deletion behavior.
            assert!(
                kind.is_content_dependent(false, false),
                "Backspace should be CD: reads surrounding text"
            );
        }

        // ── 15. JumpOlder → global-only ─────────────────────────────────
        #[test]
        fn jump_older_is_global_only() {
            let cmd = Command::Action {
                count: NonZeroU32::MIN,
                register: None,
                action: Action::JumpOlder,
            };
            assert!(cmd.is_global_only(), "Ctrl-O should be global-only");
        }

        // ── 16. Regular motion → NOT global-only ────────────────────────
        #[test]
        fn regular_motion_is_not_global_only() {
            let cmd = Command::Motion {
                count: NonZeroU32::MIN,
                motion: Motion::Down,
                explicit_count: false,
            };
            assert!(!cmd.is_global_only(), "j should NOT be global-only");
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Comprehensive system test: real Vim keystrokes and their
    // content-dependency classification
    //
    // Table-driven test that builds actual Command values for realistic
    // Vim keystrokes and asserts their is_content_dependent() result.
    // Each entry pins one classification: a command whose effect depends on
    // document content, or a design spec classification from Section 5.
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn real_command_content_dependency_comprehensive() {
        use crate::grammar::types::{TextObjectKind, TextObjectScope};

        let one = NonZeroU32::MIN;

        // (description, command, expected_cd)
        let cases: &[(&str, Command, bool)] = &[
            // ── Content-dependent: operator reads content ───────────────
            (
                "g~j (toggle case down)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::ToggleCase,
                    motion: Motion::Down,
                    force_type: None,
                },
                true,
            ),
            (
                "gUj (uppercase down)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Uppercase,
                    motion: Motion::Down,
                    force_type: None,
                },
                true,
            ),
            (
                "guj (lowercase down)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Lowercase,
                    motion: Motion::Down,
                    force_type: None,
                },
                true,
            ),
            (
                "g?w (rot13 word)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Rot13,
                    motion: Motion::WordForward,
                    force_type: None,
                },
                true,
            ),
            (
                "gqip (format paragraph)",
                Command::OperatorTextObject {
                    count: one,
                    register: None,
                    operator: Operator::Format,
                    textobject: TextObject {
                        scope: TextObjectScope::Inner,
                        kind: TextObjectKind::Paragraph,
                        seek: None,
                    },
                },
                true,
            ),
            // ── Content-dependent: motion reads content ─────────────────
            (
                "dw (delete word)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Delete,
                    motion: Motion::WordForward,
                    force_type: None,
                },
                true,
            ),
            (
                "d$ (delete to end of line)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Delete,
                    motion: Motion::LineEnd,
                    force_type: None,
                },
                true,
            ),
            (
                "ciw (change inner word)",
                Command::OperatorTextObject {
                    count: one,
                    register: None,
                    operator: Operator::Change,
                    textobject: TextObject {
                        scope: TextObjectScope::Inner,
                        kind: TextObjectKind::Word,
                        seek: None,
                    },
                },
                true,
            ),
            (
                "gUiw (uppercase inner word -- both sides CD)",
                Command::OperatorTextObject {
                    count: one,
                    register: None,
                    operator: Operator::Uppercase,
                    textobject: TextObject {
                        scope: TextObjectScope::Inner,
                        kind: TextObjectKind::Word,
                        seek: None,
                    },
                },
                true,
            ),
            // ── Content-dependent actions ───────────────────────────────
            (
                "~ (swap case)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::SwapCase,
                },
                true,
            ),
            (
                "Ctrl-A (increment number)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::IncrementNumber,
                },
                true,
            ),
            (
                "Ctrl-X (decrement number)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::DecrementNumber,
                },
                true,
            ),
            (
                "x (delete char)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::DeleteChar,
                },
                true,
            ),
            (
                "X (delete char back)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::DeleteCharBack,
                },
                true,
            ),
            (
                "J (join lines)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::Join,
                },
                true,
            ),
            (
                "s (substitute)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::Substitute,
                },
                true,
            ),
            (
                "D (delete to end)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::DeleteToEnd,
                },
                true,
            ),
            (
                "C (change to end)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::ChangeToEnd,
                },
                true,
            ),
            // ── Surround ────────────────────────────────────────────────
            (
                "ys( (surround add)",
                Command::SurroundAdd {
                    count: one,
                    motion: Some(Motion::WordForward),
                    textobject: None,
                    char: '(',
                },
                true,
            ),
            (
                "ds( (surround delete)",
                Command::SurroundDelete { char: '(' },
                true,
            ),
            (
                "cs([ (surround change)",
                Command::SurroundChange {
                    old_char: '(',
                    new_char: '[',
                },
                true,
            ),
            // ── CharCommand + operator: CD ──────────────────────────────
            (
                "dfa (delete to find a)",
                Command::CharCommand {
                    count: one,
                    register: None,
                    operator: Some(Operator::Delete),
                    command: CharCommand::FindForward,
                    target: CompactString::new_inline("a"),
                },
                true,
            ),
            (
                "rx (replace char)",
                Command::CharCommand {
                    count: one,
                    register: None,
                    operator: None,
                    command: CharCommand::Replace,
                    target: CompactString::new_inline("x"),
                },
                true,
            ),
            // ── Sneak + operator: CD ────────────────────────────────────
            (
                "dsab (delete sneak)",
                Command::Sneak {
                    count: one,
                    register: None,
                    operator: Some(Operator::Delete),
                    c1: 'a',
                    c2: 'b',
                    forward: true,
                },
                true,
            ),
            // ── Linewise: depends on operator ───────────────────────────
            (
                ">> (indent line)",
                Command::OperatorLine {
                    count: one,
                    register: None,
                    operator: Operator::Indent,
                },
                true,
            ),
            (
                "<< (outdent line)",
                Command::OperatorLine {
                    count: one,
                    register: None,
                    operator: Operator::Outdent,
                },
                true,
            ),
            (
                "g~~ (toggle case line)",
                Command::OperatorLine {
                    count: one,
                    register: None,
                    operator: Operator::ToggleCase,
                },
                true,
            ),
            // ── Composed operator ───────────────────────────────────────
            (
                "y>w (yank + indent word)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Composed(crate::primitives::ComposedPair::new(
                        Operator::Yank,
                        Operator::Indent,
                    )),
                    motion: Motion::WordForward,
                    force_type: None,
                },
                true,
            ),
            // ── Visual operator: CD if operator reads content ───────────
            (
                "visual gU",
                Command::OperatorSelection {
                    register: None,
                    operator: Operator::Uppercase,
                },
                true,
            ),
            // ── InsertEntry: PI (mode transition, must use algebraic rebase) ──
            (
                "i (enter insert)",
                Command::InsertEntry {
                    count: one,
                    entry_type: InsertEntryType::BeforeCursor,
                    register: None,
                },
                false,
            ),
            // ── Position-independent commands ───────────────────────────
            (
                "dj (delete down)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Delete,
                    motion: Motion::Down,
                    force_type: None,
                },
                true, // Delete is mutating — line byte widths differ per cursor
            ),
            (
                "yj (yank down)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Yank,
                    motion: Motion::Down,
                    force_type: None,
                },
                false,
            ),
            (
                "dd (delete line)",
                Command::OperatorLine {
                    count: one,
                    register: None,
                    operator: Operator::Delete,
                },
                true, // Delete is mutating — line byte widths differ per cursor
            ),
            (
                "yy (yank line)",
                Command::OperatorLine {
                    count: one,
                    register: None,
                    operator: Operator::Yank,
                },
                false,
            ),
            (
                "p (put)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::Put,
                },
                false,
            ),
            (
                "u (undo)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::Undo,
                },
                false,
            ),
            (
                "Ctrl-R (redo)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::Redo,
                },
                false,
            ),
            (
                "fa standalone (CD: per-cursor find for MC)",
                Command::CharCommand {
                    count: one,
                    register: None,
                    operator: None,
                    command: CharCommand::FindForward,
                    target: CompactString::new_inline("a"),
                },
                true,
            ),
            (
                "sab standalone sneak",
                Command::Sneak {
                    count: one,
                    register: None,
                    operator: None,
                    c1: 'a',
                    c2: 'b',
                    forward: true,
                },
                false,
            ),
            (
                "j (motion — per-cursor for MC)",
                Command::Motion {
                    count: one,
                    motion: Motion::Down,
                    explicit_count: false,
                },
                true,
            ),
            (
                "v (visual enter)",
                Command::Visual(VisualKind::Enter {
                    visual_type: VisualType::Char,
                    count: None,
                }),
                false,
            ),
            ("Esc (insert exit)", Command::InsertExit, false),
            (
                ": (mode switch)",
                Command::ModeSwitch {
                    mode: Mode::CommandLine,
                },
                false,
            ),
            (
                "ma (set mark)",
                Command::Mark {
                    count: one,
                    mark_type: super::super::types::MarkType::Set,
                    mark: crate::primitives::MarkName::new('a').unwrap(),
                },
                false,
            ),
            (
                "zz (scroll center)",
                Command::Prefix {
                    count: one,
                    register: None,
                    command: PrefixCommand::ScrollCenter,
                },
                false,
            ),
            ("q (macro stop)", Command::Macro(MacroKind::Stop), false),
            (
                "@a (macro play)",
                Command::Macro(MacroKind::Play {
                    register: RegisterName::new_unchecked('a'),
                    count: one,
                }),
                false,
            ),
            (
                "gh (select enter)",
                Command::SelectEnter {
                    visual_type: VisualType::Char,
                },
                false,
            ),
            (
                "visual d (delete selection)",
                Command::OperatorSelection {
                    register: None,
                    operator: Operator::Delete,
                },
                true, // Delete is mutating — selection widths differ per cursor
            ),
            (
                "=j (reindent -- host-delegated per spec 7.8)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Reindent,
                    motion: Motion::Down,
                    force_type: None,
                },
                false,
            ),
            (
                "!j (filter -- host-delegated per spec 7.8)",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Filter,
                    motion: Motion::Down,
                    force_type: None,
                },
                false,
            ),
            (
                "Insert(Char 'a') at Command level",
                Command::Insert(InsertKind::Char { char: 'a' }),
                false,
            ),
        ];

        for (desc, cmd, expected) in cases {
            assert_eq!(
                cmd.is_content_dependent(),
                *expected,
                "{desc}: expected is_content_dependent()={expected}"
            );
        }
    }

    /// Verify is_global_only() for jump list, changelist, and undo commands.
    #[test]
    fn jump_changelist_and_undo_are_global_only() {
        let one = NonZeroU32::MIN;

        // Global-only: jump list and changelist navigation
        let global_only: &[(&str, Command)] = &[
            (
                "Ctrl-O (jump older)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::JumpOlder,
                },
            ),
            (
                "Ctrl-I (jump newer)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::JumpNewer,
                },
            ),
            (
                "g; (changelist older)",
                Command::Motion {
                    count: one,
                    motion: Motion::ChangelistOlder,
                    explicit_count: false,
                },
            ),
            (
                "g, (changelist newer)",
                Command::Motion {
                    count: one,
                    motion: Motion::ChangelistNewer,
                    explicit_count: false,
                },
            ),
            (
                "u (undo)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::Undo,
                },
            ),
            (
                "Ctrl-R (redo)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::Redo,
                },
            ),
            (
                "U (undo line)",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::UndoLine,
                },
            ),
        ];
        for (desc, cmd) in global_only {
            assert!(cmd.is_global_only(), "{desc}: must be global-only");
        }

        // NOT global-only
        let not_global: &[(&str, Command)] = &[
            (
                "j",
                Command::Motion {
                    count: one,
                    motion: Motion::Down,
                    explicit_count: false,
                },
            ),
            (
                "dw",
                Command::OperatorMotion {
                    count: one,
                    register: None,
                    operator: Operator::Delete,
                    motion: Motion::WordForward,
                    force_type: None,
                },
            ),
            (
                "~",
                Command::Action {
                    count: one,
                    register: None,
                    action: Action::SwapCase,
                },
            ),
        ];
        for (desc, cmd) in not_global {
            assert!(!cmd.is_global_only(), "{desc}: must NOT be global-only");
        }
    }
}
