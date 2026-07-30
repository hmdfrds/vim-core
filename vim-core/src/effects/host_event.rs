//! Host-visible event enum — direct projection of non-internal
//! [`Effect`](crate::effects::Effect) variants.
//!
//! # Invariant
//!
//! `HostEvent` is a 1:1 projection of every [`Effect`](crate::effects::Effect)
//! variant whose [`EffectTier`](crate::effects::EffectTier) is not `Internal`,
//! minus those that become [`HostRequest`](crate::execution::HostRequest) variants.
//!
//! Each variant carries only the fields the host needs — internal engine
//! bookkeeping (cursor strategy, syntax selection stacks, etc.) is stripped.
//!
//! Adding a new non-Internal `Effect` variant that is not a `HostRequest`
//! requires adding a corresponding `HostEvent` variant here, and updating
//! the `From<Effect>` impl (see `host_event_convert.rs`).

use compact_str::CompactString;
use smallvec::SmallVec;

use crate::effects::{HighlightStyle, InfoMessage, SelectionTag};
use crate::errors::VimError;
use crate::primitives::{
    CommandLineEdit, CursorStyle, Diagnostic, Direction, InsertEntryType, LineNumber, LineRange,
    MarkName, Mode, ModeAppearance, MotionType, Offset, Operator, Range, RegisterName,
    SelectionRange, SelectionShape, SubstitutePreviewMatch, UndoCursorStrategy, UndoNavStep,
    UndoTreeSnapshot, VimEvent, VirtualTextPosition,
};

/// Host-visible event produced by the Vim engine.
///
/// This is the canonical type that integration layers (
/// godot-vim) consume for state notifications. It mirrors every non-Internal
/// [`Effect`](crate::effects::Effect) variant that is not a host request,
/// carrying only the fields the host needs.
///
/// # Non-exhaustive
///
/// New variants may be added in minor releases. Hosts must include a
/// wildcard arm (`_ => { /* ignore */ }`) when matching.
#[derive(Debug, Clone, PartialEq, strum::EnumCount)]
#[non_exhaustive]
pub enum HostEvent {
    // ── Text Mutations ───────────────────────────────────────────────────
    /// Insert text at the specified byte offset.
    Insert {
        /// Byte offset where text should be inserted.
        offset: Offset,
        /// The text content to insert.
        text: CompactString,
    },
    /// Delete a range of text.
    Delete {
        /// The byte range to delete.
        range: Range,
    },
    /// Replace a range with new text.
    Replace {
        /// The byte range to replace.
        range: Range,
        /// The replacement text.
        text: CompactString,
    },

    // ── Cursor + Selection ───────────────────────────────────────────────
    /// Move cursor to byte offset.
    SetCursor {
        /// Target byte offset for the cursor.
        offset: Offset,
    },
    /// Set the selection for visual mode.
    SetSelection {
        /// Fixed starting point of selection.
        anchor: Offset,
        /// Moving end of selection (cursor position).
        head: Offset,
        /// How the selection must be rendered.
        shape: SelectionShape,
    },
    /// Clear selection (exit visual mode).
    ClearSelection,
    /// Set block visual insert context on the current insert session.
    SetBlockInsert {
        /// Number of additional lines below the primary line.
        lines_below: usize,
        /// Grapheme column where text should be inserted on each line.
        grapheme_col: usize,
        /// Byte offset where cursor should return after exiting block insert.
        cursor_return_offset: Offset,
    },

    // ── Mode ─────────────────────────────────────────────────────────────
    /// Change the editing mode.
    SetMode {
        /// The new mode to switch to.
        mode: Mode,
        /// Theme color suggestion for this mode.
        appearance: ModeAppearance,
    },
    /// Begin insert mode with entry type and count.
    BeginInsert {
        /// How insert mode was entered.
        entry_type: InsertEntryType,
        /// Count for repeating inserted text on exit.
        count: u32,
        /// Length of auto-indentation inserted by o/O.
        auto_indent_len: usize,
        /// Byte offset where cursor lands at insert entry.
        entry_offset: Offset,
    },
    /// Edit the command line buffer.
    CommandLineEdit(CommandLineEdit),

    // ── Undo ─────────────────────────────────────────────────────────────
    /// Begin an undo group.
    BeginUndoGroup {
        /// Strategy for cursor placement after undoing this group.
        cursor_strategy: UndoCursorStrategy,
    },
    /// End an undo group.
    EndUndoGroup {
        /// Engine-assigned node ID for this undo group.
        /// `Some` = node created, `None` = empty group (no edits).
        node_id: Option<u32>,
    },
    /// Request undo.
    Undo {
        /// Number of changes to undo.
        count: u32,
        /// Navigation steps describing which nodes to visit.
        steps: Vec<UndoNavStep>,
    },
    /// Request line-local undo (`U`).
    UndoLine {
        /// Number of line-local undo steps to apply.
        count: u32,
    },
    /// Request redo.
    Redo {
        /// Number of changes to redo.
        count: u32,
        /// Navigation steps describing which nodes to visit.
        steps: Vec<UndoNavStep>,
    },
    /// Apply operator from current cursor to mark position.
    OperatorToMark {
        /// The operator to apply (yank, delete, change, etc.)
        operator: Operator,
        /// Mark name to use as motion target.
        mark: MarkName,
        /// If true, motion is linewise (`'a`), else charwise (`` `a ``).
        linewise: bool,
        /// Target register for the operation.
        register: Option<RegisterName>,
        /// Start position (cursor when command was issued).
        cursor: Offset,
    },

    // ── Scroll ───────────────────────────────────────────────────────────
    /// Scroll to bring offset into view.
    ScrollTo {
        /// Byte offset that must be visible.
        offset: Offset,
    },
    /// Center cursor in viewport (`zz`).
    CenterCursor,
    /// Scroll cursor to top of viewport (`zt`).
    CursorToTop,
    /// Scroll cursor to bottom of viewport (`zb`).
    CursorToBottom,
    /// Scroll viewport left by count columns (`zh`).
    ScrollLeft {
        /// Number of columns to scroll.
        count: u32,
    },
    /// Scroll viewport right by count columns (`zl`).
    ScrollRight {
        /// Number of columns to scroll.
        count: u32,
    },
    /// Scroll viewport left by half screen width (`zH`).
    ScrollHalfScreenLeft {
        /// Number of half-screens to scroll.
        count: u32,
    },
    /// Scroll viewport right by half screen width (`zL`).
    ScrollHalfScreenRight {
        /// Number of half-screens to scroll.
        count: u32,
    },
    /// Scroll so cursor is at left edge of viewport (`zs`).
    ScrollCursorToLeftEdge,
    /// Scroll so cursor is at right edge of viewport (`ze`).
    ScrollCursorToRightEdge,

    // ── Search + Highlights ──────────────────────────────────────────────
    /// Set the search pattern and direction.
    SetSearchPattern {
        /// The regex pattern to search for.
        pattern: CompactString,
        /// Search direction (forward `/` or backward `?`).
        direction: Direction,
    },
    /// Highlight search matches.
    HighlightMatches {
        /// Ranges to highlight.
        ranges: Vec<Range>,
    },
    /// Clear search highlights.
    ClearHighlights,
    /// Report search match position for "Match N of M" display.
    SearchMatchInfo {
        /// 1-based index of the current match.
        current: u32,
        /// Total number of matches in the document.
        total: u32,
        /// Whether the count is complete (false if timed out or exceeded maxcount).
        complete: bool,
    },
    // ── Messages ─────────────────────────────────────────────────────────
    /// Audible or visual bell.
    Bell,
    /// Show a structured informational message.
    ShowInfo {
        /// The structured info message.
        info: InfoMessage,
    },
    /// Show a warning message.
    ShowWarning {
        /// Warning message text.
        text: CompactString,
    },
    /// Show an error message.
    ShowError {
        /// Typed error with Vim error code.
        error: VimError,
    },
    /// Clear the message area.
    ClearMessage,

    // ── Registers ────────────────────────────────────────────────────────
    /// Set a register's content.
    SetRegister {
        /// Register name.
        name: RegisterName,
        /// The text content to store.
        text: CompactString,
        /// Whether the content is characterwise or linewise.
        motion_type: MotionType,
    },
    /// Clear (erase) a named register.
    ClearNamedRegister {
        /// The register to clear.
        register: RegisterName,
    },
    /// Copy text to system clipboard.
    CopyToClipboard {
        /// Text to copy to clipboard.
        text: CompactString,
        /// Target register (`*` or `+`).
        register: RegisterName,
    },

    // ── Folds ────────────────────────────────────────────────────────────
    /// Fold (close) the region starting at the given line (`zc`).
    FoldLine {
        /// The line to fold (0-indexed).
        line: LineNumber,
    },
    /// Unfold (open) the fold at or containing the given line (`zo`).
    UnfoldLine {
        /// The line to unfold (0-indexed).
        line: LineNumber,
    },
    /// Toggle the fold at the given line (`za`).
    ToggleFold {
        /// The line to toggle (0-indexed).
        line: LineNumber,
    },
    /// Recursively toggle folds at the given line (`zA`).
    ToggleFoldRecursive {
        /// The line to toggle (0-indexed).
        line: LineNumber,
    },
    /// Fold all foldable regions in the document (`zM`).
    FoldAll,
    /// Unfold all folds in the document (`zR`).
    UnfoldAll,
    /// Recursively close all folds at cursor line (`zC`).
    FoldLineRecursive {
        /// The line to fold recursively (0-indexed).
        line: LineNumber,
    },
    /// Recursively open all folds at cursor line (`zO`).
    UnfoldLineRecursive {
        /// The line to unfold recursively (0-indexed).
        line: LineNumber,
    },
    /// Delete fold at cursor (`zd`).
    DeleteFold {
        /// The line whose fold to delete (0-indexed).
        line: LineNumber,
    },
    /// Recursively delete all folds at cursor (`zD`).
    DeleteFoldRecursive {
        /// The line whose folds to delete recursively (0-indexed).
        line: LineNumber,
    },
    /// Eliminate all folds in document (`zE`).
    EliminateAllFolds,
    /// Set foldenable to specific value (`zn` = false, `zN` = true).
    SetFoldEnable {
        /// Whether folding is enabled.
        enabled: bool,
    },
    /// Toggle foldenable option (`zi`).
    ToggleFoldEnable,

    // ── Visual Feedback ──────────────────────────────────────────────────
    /// Preview of what a `:s` command would do (inccommand).
    SubstitutePreview {
        /// The list of matches and their replacements.
        matches: Vec<SubstitutePreviewMatch>,
    },
    /// Clear the substitute preview.
    ClearSubstitutePreview,
    /// Show the next match for interactive `:s///c` confirmation.
    SubstituteConfirmShow {
        /// Document byte range of the current match to highlight.
        match_range: Range,
        /// The replacement string that would be applied.
        replacement: CompactString,
        /// 1-based index of the current match.
        match_index: u32,
        /// Total number of matches found.
        total_matches: u32,
    },
    /// End the interactive substitute confirm session.
    SubstituteConfirmEnd,
    /// Place virtual text at a specific position in the buffer.
    SetVirtualText {
        /// Namespace identifier (allows independent clear-by-namespace).
        namespace: u32,
        /// 0-indexed line number.
        line: LineNumber,
        /// Byte offset within the line where the virtual text is anchored.
        col: Offset,
        /// The virtual text content to display.
        text: CompactString,
        /// Where to render relative to the anchor position.
        position: VirtualTextPosition,
    },
    /// Clear all virtual text in the given namespace.
    ClearVirtualText {
        /// Namespace whose virtual text entries should be removed.
        namespace: u32,
    },
    /// Set the full list of diagnostics for a namespace.
    SetDiagnostics {
        /// Namespace identifier (allows independent diagnostic sources).
        namespace: u32,
        /// The complete list of diagnostics (replaces previous set).
        diagnostics: Vec<Diagnostic>,
    },
    /// Recommended cursor shape and blink style for the current mode.
    SetCursorStyle {
        /// The cursor style that matches the new mode.
        style: CursorStyle,
    },
    /// Hint about desired cursor shape based on operator-pending state.
    ///
    /// Emitted when entering operator-pending mode with `Some(operator)`,
    /// and when leaving with `None`. Hosts may ignore this.
    CursorShapeHint {
        /// Which operator is pending, or `None` when returning to normal.
        pending_operator: Option<Operator>,
    },

    // ── Multi-Cursor / Selection ─────────────────────────────────────────
    /// Set block selections for multi-cursor visual block mode.
    SetBlockSelections {
        /// The selection ranges (up to 4 inline, heap-allocated beyond).
        selections: SmallVec<[SelectionRange; 4]>,
    },
    /// Save the host's current selection state under a tag.
    SaveSelections {
        /// Tag identifying the selection snapshot.
        tag: SelectionTag,
    },
    /// Restore a previously saved selection state.
    RestoreSelections {
        /// Tag identifying which snapshot to restore.
        tag: SelectionTag,
    },
    /// Add a selection at the next occurrence of a pattern.
    SelectNextMatch {
        /// Pattern to search for, or `None` for current word.
        pattern: Option<String>,
        /// If true, skip the match under the current cursor.
        skip_current: bool,
    },
    /// Add a selection at the previous occurrence of a pattern.
    SelectPreviousMatch {
        /// Pattern to search for, or `None` for current word.
        pattern: Option<String>,
        /// If true, skip the match under the current cursor.
        skip_current: bool,
    },
    /// Highlight rows during command execution.
    HighlightRows {
        /// The line range to highlight.
        lines: LineRange,
        /// The highlight style to apply.
        style: HighlightStyle,
    },
    /// Set a highlight region for a named owner and highlight group.
    SetHighlightRange {
        /// Owner identifier (e.g., "exchange", "surround").
        owner: CompactString,
        /// The byte range to highlight.
        range: Range,
        /// Highlight group name (e.g., "pending", "target").
        group: CompactString,
        /// Render shape: char (trapezoid), line (full-width), or block (rectangular).
        shape: SelectionShape,
    },
    /// Clear highlights for a named owner.
    ClearHighlightRange {
        /// Owner identifier whose highlights to clear.
        owner: CompactString,
        /// Optional group to clear. `None` clears all groups.
        group: Option<CompactString>,
    },

    // ── Misc ─────────────────────────────────────────────────────────────
    /// Synchronize syntax-based fold ranges with the host.
    SyncFoldRanges {
        /// The fold ranges as `(start_line, end_line)` pairs (0-indexed).
        ranges: Vec<(LineNumber, LineNumber)>,
    },
    /// Full undo tree snapshot for visualization.
    UndoTreeSnapshot {
        /// The complete tree snapshot.
        snapshot: UndoTreeSnapshot,
    },
    /// Typed Vim event for host notification.
    Event {
        /// The event that occurred.
        kind: VimEvent,
    },
    /// Set the sticky half-page scroll count for `Ctrl-D`/`Ctrl-U`.
    SetScrollHalfCount {
        /// The count to persist (from explicit user input).
        count: u32,
    },
    /// Signal the host to briefly highlight the matching bracket (`showmatch`).
    ///
    /// Emitted when a closing bracket is typed in insert mode with `'showmatch'`
    /// enabled. The host should flash the cursor to the matching opening bracket.
    ShowMatch {
        /// Byte offset of the matching opening bracket to highlight.
        position: Offset,
    },
    /// Atomic mode change bundling mode, cursor style, and appearance.
    ///
    /// Hosts that handle this variant get a consistent state transition
    /// without seeing transient inconsistencies from separate `SetMode` +
    /// `SetCursorStyle`. Hosts that do not handle it will never receive it
    /// (middleware decomposes it into individual events).
    ModeTransition {
        /// The new editing mode.
        mode: Mode,
        /// The cursor style for the new mode.
        cursor_style: CursorStyle,
        /// Theme color suggestion for the new mode.
        appearance: ModeAppearance,
    },
    /// Request the host to start a timer.
    ///
    /// See [`Effect::RequestTimer`](crate::effects::Effect::RequestTimer) for
    /// full semantics. The host starts a timer and sends
    /// `HostNotification::TimerFired { id }` when it expires.
    RequestTimer {
        /// Opaque timer identifier.
        id: u32,
        /// Delay in milliseconds.
        delay_ms: u32,
    },
}
