//! Effect types for vim-core.
//!
//! Effects are pure instructions that tell the shell what to do.
//! The engine produces Effects, the shell applies them.

use crate::errors::VimError;
use crate::primitives::SubFlags;
use smallvec::SmallVec;

use super::info_message::InfoMessage;

/// Owner identifier for yank highlight ranges.
///
/// Used in [`Effect::SetHighlightRange`] and [`Effect::ClearHighlightRange`]
/// to tag highlights that represent the brief flash on yanked text.
pub const HIGHLIGHT_OWNER_YANK: &str = "yank";

/// Classification tier for [`EffectKind`] variants.
///
/// Integrators should prioritise handling effects by tier:
///
/// - **Core** — text mutations, cursor, selection, mode, undo.  Every host
///   *must* handle these for correct editing.
/// - **Standard** — scrolling, search highlights, registers, folds, messages,
///   recording, cursor style.  Expected in any production integration.
/// - **Advanced** — windowing, LSP navigation, host actions, previews,
///   virtual text, diagnostics, macros, jump-to-buffer, etc.  Can be
///   stubbed or deferred.
/// - **Internal** — book-keeping consumed by the engine's own effect
///   processor (exchange state, syntax selection, norm commands, operator
///   helpers, jump/changelist, marks, sticky column, noop).  Hosts
///   typically ignore these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectTier {
    /// MUST handle for correct editing. Text mutations, cursor, selection,
    /// mode, undo grouping.
    Core,
    /// Needed for good UX. Scrolling, messages, search highlights,
    /// registers, clipboard, folds, command line.
    Standard,
    /// Full vim experience. Window management, LSP navigation,
    /// virtual text, diagnostics, substitute preview.
    Advanced,
    /// Engine-internal. Consumed by the effect processor before reaching
    /// the host. Hosts should never see these.
    Internal,
}

/// Style for row highlighting during command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HighlightStyle {
    /// A pending operation highlight (e.g., operator pending range).
    Pending,
    /// An active/confirmed highlight (e.g., current line during `:norm`).
    Active,
    /// Clear any existing row highlight.
    Clear,
}

/// Tag for saving/restoring host selection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SelectionTag {
    /// Search-related selections (e.g., `gn` visual selection).
    Search,
    /// Preview-related selections (e.g., inccommand preview).
    Preview,
}

/// Context for where an error originated (for `:source` script errors).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SourceContext {
    /// The file path being sourced (e.g. `"init.vim"`).
    pub file: CompactString,
    /// The line number within the file where the error occurred.
    pub line: u32,
}

use crate::primitives::{
    BufferId, CommandLineEdit as CmdLineEdit, Diagnostic, Direction, InsertEntryType, LineNumber,
    LineRange, MarkName, Mode, ModeAppearance, MotionType, NodeId, Offset, Range, RegisterName,
    SelectionRange, SelectionShape, SubstitutePreviewMatch, UndoCursorStrategy, UndoNavStep,
    VirtualTextPosition,
};
use compact_str::CompactString;

/// Stable discriminator for [`Effect`] variants.
///
/// This allows host adapters to implement exhaustive translation coverage
/// checks without pattern-matching directly on a non-exhaustive enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EffectKind {
    /// [`Effect::Insert`]
    Insert,
    /// [`Effect::Delete`]
    Delete,
    /// [`Effect::Replace`]
    Replace,
    /// [`Effect::SetCursor`]
    SetCursor,
    /// [`Effect::SetSelection`]
    SetSelection,
    /// [`Effect::ClearSelection`]
    ClearSelection,
    /// [`Effect::SaveLastVisual`]
    SaveLastVisual,
    /// [`Effect::SetMode`]
    SetMode,
    /// [`Effect::CommandLineEdit`]
    CommandLineEdit,
    /// [`Effect::BeginInsert`]
    BeginInsert,
    /// [`Effect::SetBlockInsert`]
    SetBlockInsert,
    /// [`Effect::SetRegister`]
    SetRegister,
    /// [`Effect::SetMark`]
    SetMark,
    /// [`Effect::OperatorToMark`]
    OperatorToMark,
    /// [`Effect::PushJumpList`]
    PushJumpList,
    /// [`Effect::JumpOlder`]
    JumpOlder,
    /// [`Effect::JumpNewer`]
    JumpNewer,
    /// [`Effect::JumpToBuffer`]
    JumpToBuffer,
    /// [`Effect::ChangelistOlder`]
    ChangelistOlder,
    /// [`Effect::ChangelistNewer`]
    ChangelistNewer,
    /// [`Effect::BeginUndoGroup`]
    BeginUndoGroup,
    /// [`Effect::EndUndoGroup`]
    EndUndoGroup,
    /// [`Effect::Undo`]
    Undo,
    /// [`Effect::UndoLine`]
    UndoLine,
    /// [`Effect::Redo`]
    Redo,
    /// [`Effect::SetSearchPattern`]
    SetSearchPattern,
    /// [`Effect::SetLastSubstitute`]
    SetLastSubstitute,
    /// [`Effect::SetLastSubstituteFlags`]
    SetLastSubstituteFlags,
    /// [`Effect::HighlightMatches`]
    HighlightMatches,
    /// [`Effect::ClearHighlights`]
    ClearHighlights,
    /// [`Effect::SetLastFind`]
    SetLastFind,
    /// [`Effect::NormCommand`]
    NormCommand,
    /// [`Effect::OperatorFilter`]
    OperatorFilter,
    /// [`Effect::OperatorReindent`]
    OperatorReindent,
    /// [`Effect::Bell`]
    Bell,
    /// [`Effect::ShowInfo`]
    ShowInfo,
    /// [`Effect::ShowWarning`]
    ShowWarning,
    /// [`Effect::ShowError`]
    ShowError,
    /// [`Effect::ClearMessage`]
    ClearMessage,
    /// [`Effect::ScrollTo`]
    ScrollTo,
    /// [`Effect::CenterCursor`]
    CenterCursor,
    /// [`Effect::CursorToTop`]
    CursorToTop,
    /// [`Effect::CursorToBottom`]
    CursorToBottom,
    /// [`Effect::ScrollLeft`]
    ScrollLeft,
    /// [`Effect::ScrollRight`]
    ScrollRight,
    /// [`Effect::ScrollHalfScreenLeft`]
    ScrollHalfScreenLeft,
    /// [`Effect::ScrollHalfScreenRight`]
    ScrollHalfScreenRight,
    /// [`Effect::ScrollCursorToLeftEdge`]
    ScrollCursorToLeftEdge,
    /// [`Effect::ScrollCursorToRightEdge`]
    ScrollCursorToRightEdge,
    /// [`Effect::StartRecording`]
    StartRecording,
    /// [`Effect::StopRecording`]
    StopRecording,
    /// [`Effect::PlayMacro`]
    PlayMacro,
    /// [`Effect::CopyToClipboard`]
    CopyToClipboard,
    /// [`Effect::SearchMatchInfo`]
    SearchMatchInfo,
    /// [`Effect::SetScrollHalfCount`]
    SetScrollHalfCount,
    /// [`Effect::SetStickyColumn`]
    SetStickyColumn,
    /// [`Effect::FoldLine`]
    FoldLine,
    /// [`Effect::UnfoldLine`]
    UnfoldLine,
    /// [`Effect::ToggleFold`]
    ToggleFold,
    /// [`Effect::ToggleFoldRecursive`]
    ToggleFoldRecursive,
    /// [`Effect::FoldAll`]
    FoldAll,
    /// [`Effect::UnfoldAll`]
    UnfoldAll,
    /// [`Effect::OpenCommandWindow`]
    OpenCommandWindow,
    /// [`Effect::CallOperatorFunc`]
    CallOperatorFunc,
    /// [`Effect::Event`]
    Event,
    // Window effects
    /// [`Effect::WindowSplit`]
    WindowSplit,
    /// [`Effect::WindowNew`]
    WindowNew,
    /// [`Effect::WindowVSplit`]
    WindowVSplit,
    /// [`Effect::WindowClose`]
    WindowClose,
    /// [`Effect::WindowOnly`]
    WindowOnly,
    /// [`Effect::WindowNext`]
    WindowNext,
    /// [`Effect::WindowPrev`]
    WindowPrev,
    /// [`Effect::WindowMoveLeft`]
    WindowMoveLeft,
    /// [`Effect::WindowMoveRight`]
    WindowMoveRight,
    /// [`Effect::WindowMoveUp`]
    WindowMoveUp,
    /// [`Effect::WindowMoveDown`]
    WindowMoveDown,
    /// [`Effect::WindowEqualSize`]
    WindowEqualSize,
    /// [`Effect::WindowIncreaseHeight`]
    WindowIncreaseHeight,
    /// [`Effect::WindowDecreaseHeight`]
    WindowDecreaseHeight,
    /// [`Effect::WindowIncreaseWidth`]
    WindowIncreaseWidth,
    /// [`Effect::WindowDecreaseWidth`]
    WindowDecreaseWidth,
    /// [`Effect::WindowRotateDown`]
    WindowRotateDown,
    /// [`Effect::WindowRotateUp`]
    WindowRotateUp,
    // Recursive fold effects
    /// [`Effect::FoldLineRecursive`]
    FoldLineRecursive,
    /// [`Effect::UnfoldLineRecursive`]
    UnfoldLineRecursive,
    /// [`Effect::DeleteFold`]
    DeleteFold,
    /// [`Effect::DeleteFoldRecursive`]
    DeleteFoldRecursive,
    /// [`Effect::EliminateAllFolds`]
    EliminateAllFolds,
    /// [`Effect::ToggleFoldEnable`]
    ToggleFoldEnable,
    /// [`Effect::SetFoldEnable`]
    SetFoldEnable,
    // LSP Navigation effects
    /// [`Effect::GotoDefinition`]
    GotoDefinition,
    /// [`Effect::ShowDocumentation`]
    ShowDocumentation,
    /// [`Effect::HostAction`]
    HostAction,
    // Extension state/highlight effects
    /// [`Effect::SetExtState`]
    SetExtState,
    /// [`Effect::ClearExtState`]
    ClearExtState,
    /// [`Effect::SetHighlightRange`]
    SetHighlightRange,
    /// [`Effect::ClearHighlightRange`]
    ClearHighlightRange,
    /// [`Effect::SubstitutePreview`]
    SubstitutePreview,
    /// [`Effect::ClearSubstitutePreview`]
    ClearSubstitutePreview,
    // Syntax selection internal state (consumed by effect processor)
    /// [`Effect::SyntaxSelectionPush`]
    SyntaxSelectionPush,
    /// [`Effect::SyntaxSelectionPop`]
    SyntaxSelectionPop,
    /// [`Effect::SyntaxHistoryClear`]
    SyntaxHistoryClear,
    /// [`Effect::SetSyntaxSelections`]
    SetSyntaxSelections,
    // Virtual text / decoration effects
    /// [`Effect::SetVirtualText`]
    SetVirtualText,
    /// [`Effect::ClearVirtualText`]
    ClearVirtualText,
    /// [`Effect::SetDiagnostics`]
    SetDiagnostics,
    /// [`Effect::SyncFoldRanges`]
    SyncFoldRanges,
    /// [`Effect::UndoTreeSnapshot`]
    UndoTreeSnapshot,
    /// [`Effect::SetSubstitutePattern`]
    SetSubstitutePattern,
    /// [`Effect::SetCursorStyle`]
    SetCursorStyle,
    /// [`Effect::Noop`]
    Noop,
    /// [`Effect::ClearNamedRegister`]
    ClearNamedRegister,
    /// [`Effect::ClearMark`]
    ClearMark,
    /// [`Effect::SetVariable`]
    SetVariable,
    /// [`Effect::DeleteVariable`]
    DeleteVariable,
    // Multi-cursor & syntax selection effects
    /// [`Effect::HighlightRows`]
    HighlightRows,
    /// [`Effect::SetBlockSelections`]
    SetBlockSelections,
    /// [`Effect::SaveSelections`]
    SaveSelections,
    /// [`Effect::RestoreSelections`]
    RestoreSelections,
    /// [`Effect::SelectNextMatch`]
    SelectNextMatch,
    /// [`Effect::SelectPreviousMatch`]
    SelectPreviousMatch,
    /// [`Effect::SubstituteConfirmShow`]
    SubstituteConfirmShow,
    /// [`Effect::SubstituteConfirmEnd`]
    SubstituteConfirmEnd,
    /// [`Effect::SetSubstituteConfirmState`]
    SetSubstituteConfirmState,
    /// [`Effect::ClearSubstituteConfirmState`]
    ClearSubstituteConfirmState,
    /// [`Effect::CursorShapeHint`]
    CursorShapeHint,
    /// [`Effect::ShowMatch`]
    ShowMatch,
    /// [`Effect::CrossBufferEdit`]
    CrossBufferEdit,
    /// [`Effect::ModeTransition`]
    ModeTransition,
    /// [`Effect::RequestTimer`]
    RequestTimer,
}

impl EffectKind {
    /// Ordered list of all currently defined effect kinds.
    pub const ALL: [Self; 130] = [
        Self::Insert,
        Self::Delete,
        Self::Replace,
        Self::SetCursor,
        Self::SetSelection,
        Self::ClearSelection,
        Self::SaveLastVisual,
        Self::SetMode,
        Self::CommandLineEdit,
        Self::BeginInsert,
        Self::SetBlockInsert,
        Self::SetRegister,
        Self::SetMark,
        Self::OperatorToMark,
        Self::PushJumpList,
        Self::JumpOlder,
        Self::JumpNewer,
        Self::JumpToBuffer,
        Self::ChangelistOlder,
        Self::ChangelistNewer,
        Self::BeginUndoGroup,
        Self::EndUndoGroup,
        Self::Undo,
        Self::UndoLine,
        Self::Redo,
        Self::SetSearchPattern,
        Self::SetLastSubstitute,
        Self::SetLastSubstituteFlags,
        Self::HighlightMatches,
        Self::ClearHighlights,
        Self::SetLastFind,
        Self::NormCommand,
        Self::OperatorFilter,
        Self::OperatorReindent,
        Self::Bell,
        Self::ShowInfo,
        Self::ShowWarning,
        Self::ShowError,
        Self::ClearMessage,
        Self::ScrollTo,
        Self::CenterCursor,
        Self::CursorToTop,
        Self::CursorToBottom,
        Self::ScrollLeft,
        Self::ScrollRight,
        Self::ScrollHalfScreenLeft,
        Self::ScrollHalfScreenRight,
        Self::ScrollCursorToLeftEdge,
        Self::ScrollCursorToRightEdge,
        Self::StartRecording,
        Self::StopRecording,
        Self::PlayMacro,
        Self::CopyToClipboard,
        Self::SearchMatchInfo,
        Self::SetScrollHalfCount,
        Self::SetStickyColumn,
        Self::FoldLine,
        Self::UnfoldLine,
        Self::ToggleFold,
        Self::ToggleFoldRecursive,
        Self::FoldAll,
        Self::UnfoldAll,
        Self::OpenCommandWindow,
        Self::CallOperatorFunc,
        Self::Event,
        // Window effects
        Self::WindowSplit,
        Self::WindowNew,
        Self::WindowVSplit,
        Self::WindowClose,
        Self::WindowOnly,
        Self::WindowNext,
        Self::WindowPrev,
        Self::WindowMoveLeft,
        Self::WindowMoveRight,
        Self::WindowMoveUp,
        Self::WindowMoveDown,
        Self::WindowEqualSize,
        Self::WindowIncreaseHeight,
        Self::WindowDecreaseHeight,
        Self::WindowIncreaseWidth,
        Self::WindowDecreaseWidth,
        Self::WindowRotateDown,
        Self::WindowRotateUp,
        // Recursive fold effects
        Self::FoldLineRecursive,
        Self::UnfoldLineRecursive,
        Self::DeleteFold,
        Self::DeleteFoldRecursive,
        Self::EliminateAllFolds,
        Self::ToggleFoldEnable,
        Self::SetFoldEnable,
        // LSP Navigation
        Self::GotoDefinition,
        Self::ShowDocumentation,
        // Host action bridge
        Self::HostAction,
        // Extension state/highlight
        Self::SetExtState,
        Self::ClearExtState,
        Self::SetHighlightRange,
        Self::ClearHighlightRange,
        // Substitute preview
        Self::SubstitutePreview,
        Self::ClearSubstitutePreview,
        // Syntax selection internal state (consumed by effect processor)
        Self::SyntaxSelectionPush,
        Self::SyntaxSelectionPop,
        Self::SyntaxHistoryClear,
        Self::SetSyntaxSelections,
        // Virtual text / decoration effects
        Self::SetVirtualText,
        Self::ClearVirtualText,
        Self::SetDiagnostics,
        // Fold range sync
        Self::SyncFoldRanges,
        // Undo tree visualization
        Self::UndoTreeSnapshot,
        // Substitute pattern (two-pattern system)
        Self::SetSubstitutePattern,
        // Cursor style (emitted alongside SetMode)
        Self::SetCursorStyle,
        // No-op / register and mark clearing
        Self::Noop,
        Self::ClearNamedRegister,
        Self::ClearMark,
        // Variable store
        Self::SetVariable,
        Self::DeleteVariable,
        // Multi-cursor & syntax selection
        Self::HighlightRows,
        Self::SetBlockSelections,
        Self::SaveSelections,
        Self::RestoreSelections,
        Self::SelectNextMatch,
        Self::SelectPreviousMatch,
        // Substitute confirm
        Self::SubstituteConfirmShow,
        Self::SubstituteConfirmEnd,
        Self::SetSubstituteConfirmState,
        Self::ClearSubstituteConfirmState,
        // Operator-pending cursor shape hint
        Self::CursorShapeHint,
        // Insert-mode bracket matching flash
        Self::ShowMatch,
        // Cross-buffer edit
        Self::CrossBufferEdit,
        // Atomic mode transition
        Self::ModeTransition,
        // Timer
        Self::RequestTimer,
    ];

    /// Return the [`EffectTier`] for this variant.
    ///
    /// The match is exhaustive — adding a new `EffectKind` variant without
    /// classifying it here will cause a compile error.
    #[must_use]
    pub const fn tier(self) -> EffectTier {
        match self {
            // Core — MUST handle for correct editing
            Self::Insert
            | Self::Delete
            | Self::Replace
            | Self::SetCursor
            | Self::SetSelection
            | Self::ClearSelection
            | Self::SetMode
            | Self::BeginUndoGroup
            | Self::EndUndoGroup
            | Self::Undo
            | Self::UndoLine
            | Self::Redo
            | Self::BeginInsert
            | Self::SetBlockInsert
            | Self::OperatorToMark
            | Self::ModeTransition => EffectTier::Core,

            // Standard — needed for good UX
            Self::ScrollTo
            | Self::CenterCursor
            | Self::CursorToTop
            | Self::CursorToBottom
            | Self::ScrollLeft
            | Self::ScrollRight
            | Self::ScrollHalfScreenLeft
            | Self::ScrollHalfScreenRight
            | Self::ScrollCursorToLeftEdge
            | Self::ScrollCursorToRightEdge
            | Self::SetSearchPattern
            | Self::HighlightMatches
            | Self::ClearHighlights
            | Self::SearchMatchInfo
            | Self::Bell
            | Self::ShowInfo
            | Self::ShowWarning
            | Self::ShowError
            | Self::ClearMessage
            | Self::SetRegister
            | Self::ClearNamedRegister
            | Self::CopyToClipboard
            | Self::CommandLineEdit
            | Self::FoldLine
            | Self::FoldLineRecursive
            | Self::UnfoldLine
            | Self::UnfoldLineRecursive
            | Self::ToggleFold
            | Self::ToggleFoldRecursive
            | Self::FoldAll
            | Self::UnfoldAll
            | Self::DeleteFold
            | Self::DeleteFoldRecursive
            | Self::EliminateAllFolds
            | Self::SetFoldEnable
            | Self::ToggleFoldEnable
            | Self::SyncFoldRanges
            | Self::SetHighlightRange
            | Self::ClearHighlightRange
            | Self::SetCursorStyle
            | Self::CursorShapeHint
            | Self::SetScrollHalfCount
            | Self::SetBlockSelections
            | Self::SaveSelections
            | Self::RestoreSelections
            | Self::SubstituteConfirmShow
            | Self::SubstituteConfirmEnd
            | Self::ShowMatch => EffectTier::Standard,

            // Internal — consumed by effect processor before reaching host.
            // Some also pass through for host state replication (marked below).
            Self::StartRecording
            | Self::StopRecording
            | Self::SetExtState
            | Self::ClearExtState
            | Self::SyntaxSelectionPush
            | Self::SyntaxSelectionPop
            | Self::SyntaxHistoryClear
            | Self::SetSyntaxSelections
            // Internal+Passthrough: sync engine state AND pass through to host
            // for state replication / UI updates.
            | Self::SaveLastVisual
            | Self::SetLastFind
            | Self::SetLastSubstitute
            | Self::SetLastSubstituteFlags
            | Self::SetSubstitutePattern
            | Self::PushJumpList
            | Self::JumpOlder
            | Self::JumpNewer
            | Self::ChangelistOlder
            | Self::ChangelistNewer
            | Self::SetMark
            | Self::ClearMark
            | Self::SetStickyColumn
            | Self::SetSubstituteConfirmState
            | Self::ClearSubstituteConfirmState
            | Self::SetVariable
            | Self::DeleteVariable
            | Self::Noop => EffectTier::Internal,

            // Advanced — full vim experience
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
            | Self::WindowRotateUp
            | Self::GotoDefinition
            | Self::ShowDocumentation
            | Self::HostAction
            | Self::SubstitutePreview
            | Self::ClearSubstitutePreview
            | Self::SetVirtualText
            | Self::ClearVirtualText
            | Self::SetDiagnostics
            | Self::Event
            | Self::UndoTreeSnapshot
            | Self::PlayMacro
            | Self::JumpToBuffer
            | Self::OpenCommandWindow
            | Self::CallOperatorFunc
            | Self::NormCommand
            | Self::OperatorFilter
            | Self::OperatorReindent
            | Self::HighlightRows
            | Self::SelectNextMatch
            | Self::SelectPreviousMatch
            | Self::CrossBufferEdit
            | Self::RequestTimer => EffectTier::Advanced,
        }
    }

    /// Returns `true` if this kind represents a text-mutating effect.
    ///
    /// Includes: `Insert`, `Delete`, `Replace`, `Undo`, `UndoLine`, `Redo`
    /// (direct buffer mutations) and `OperatorToMark` (host-executed mutation
    /// that applies an operator to a mark position).
    #[inline]
    #[must_use]
    pub const fn is_text_mutation(self) -> bool {
        matches!(
            self,
            Self::Insert
                | Self::Delete
                | Self::Replace
                | Self::Undo
                | Self::UndoLine
                | Self::Redo
                | Self::OperatorToMark
        )
    }
}

/// An effect produced by the Vim engine.
///
/// Effects describe what the shell should do. They are:
/// - Pure: no side effects in the engine
/// - Invertible: can be undone
/// - Serializable: can be logged and replayed
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Effect {
    // === Document Effects ===
    /// Insert text at the specified position.
    ///
    /// The shell should insert `text` at `offset`, shifting subsequent
    /// content to the right. This is the primitive for all insertions.
    Insert {
        /// Byte offset where text should be inserted.
        offset: Offset,
        /// The text content to insert.
        text: CompactString,
    },
    /// Delete a range of text.
    ///
    /// The shell should remove all content in `range` (start inclusive,
    /// end exclusive). Subsequent content shifts left.
    Delete {
        /// The byte range to delete.
        range: Range,
    },
    /// Replace a range with new text.
    ///
    /// Equivalent to Delete + Insert, but atomic for undo purposes.
    Replace {
        /// The byte range to replace.
        range: Range,
        /// The replacement text.
        text: CompactString,
    },

    // === Cursor Effects ===
    /// Move cursor to byte offset.
    ///
    /// The shell should reposition the cursor to this exact offset.
    SetCursor {
        /// Target gap offset for the cursor.
        offset: Offset,
    },
    /// Set the selection for visual mode.
    ///
    /// Creates or updates a selection from anchor to head. The anchor
    /// is the fixed point, head moves with the cursor.
    SetSelection {
        /// Fixed starting point of selection.
        anchor: Offset,
        /// Moving end of selection (cursor position).
        head: Offset,
        /// How the selection must be rendered by the shell.
        shape: SelectionShape,
    },
    /// Clear selection (exit visual mode).
    ClearSelection,
    /// Save visual selection dimensions for dot repeat.
    ///
    /// Used by `execute_operator_selection` to record the visual selection
    /// geometry (type, line count, column span) so that `.` can reconstruct
    /// the same selection from the new cursor position.
    ///
    /// Mirrors Neovim's `resel_VIsual_mode`, `resel_VIsual_line_count`,
    /// and `resel_VIsual_vcol` internal variables.
    SaveLastVisual {
        /// Visual selection info (type, lines, columns).
        info: crate::primitives::LastVisualInfo,
    },

    // === Mode Effects ===
    /// Change the editing mode.
    ///
    /// The shell should update its mode display and behavior.
    SetMode {
        /// The new mode to switch to.
        mode: Mode,
        /// Theme color suggestion for this mode.
        appearance: ModeAppearance,
    },
    /// Edit the command line buffer.
    ///
    /// The shell should apply the pure text edit to the active command line prompt.
    CommandLineEdit(CmdLineEdit),
    /// Begin insert mode with entry type and count.
    ///
    /// Used instead of SetMode for insert entry to track:
    /// - Entry type (i, a, I, A, o, O, s, S)
    /// - Count for repeating text on exit (e.g., `3iX<Esc>` → "XXX")
    BeginInsert {
        /// How insert mode was entered.
        entry_type: InsertEntryType,
        /// Count for repeating inserted text on exit.
        count: u32,
        /// Length of auto-indentation inserted by o/O.
        /// Used to strip trailing whitespace when no text typed before Esc.
        auto_indent_len: usize,
        /// Byte offset where cursor lands at insert entry.
        /// Used as the boundary for `<C-u>` and `<C-w>` — they won't
        /// delete past this position.
        entry_offset: Offset,
    },
    /// Set block visual insert context on the current insert session.
    ///
    /// When insert mode is entered from a block visual operation (change, I, A),
    /// this effect carries the context needed to replicate typed text to all
    /// other lines in the block upon insert exit.
    SetBlockInsert {
        /// Number of additional lines below the primary line.
        lines_below: usize,
        /// Grapheme column where text should be inserted on each additional line.
        grapheme_col: usize,
        /// Byte offset where cursor should return after exiting block insert.
        cursor_return_offset: Offset,
    },

    // === Register Effects ===
    /// Set a register's content.
    ///
    /// Replaces the register's content entirely.
    SetRegister {
        /// Register name (validated at grammar boundary).
        name: RegisterName,
        /// The text content to store.
        text: CompactString,
        /// Whether the content is characterwise or linewise.
        motion_type: MotionType,
    },

    // === Mark & Jump Effects ===
    /// Set a mark position.
    ///
    /// Associates a single character with a buffer position and optional
    /// relative topline offset for viewport restoration.
    SetMark {
        /// Mark name (a-z for local, A-Z for global, 0-9 for numbered).
        name: MarkName,
        /// Byte offset to mark.
        offset: Offset,
        /// Relative line offset: `mark_line - viewport_first_line` at set time.
        /// `Some` for user-set marks (`m{a-z}`), `None` for auto-marks.
        topline_offset: Option<i32>,
    },

    /// Apply operator from current cursor to mark position.
    ///
    /// Shell resolves mark position and applies the operator to the range.
    /// Examples: y'a (yank to mark a), d`b (delete to mark b)
    OperatorToMark {
        /// The operator to apply (yank, delete, change, etc.)
        operator: crate::primitives::Operator,
        /// Mark name to use as motion target.
        mark: MarkName,
        /// If true, motion is linewise (`'a`), else charwise (`` `a ``).
        linewise: bool,
        /// Target register for the operation.
        register: Option<RegisterName>,
        /// Start position (cursor when command was issued).
        cursor: Offset,
    },
    /// Push position to jump list (before jump motions).
    ///
    /// Called before gg, G, /, ?, etc. to enable Ctrl-O/Ctrl-I navigation.
    PushJumpList {
        /// The position to push.
        offset: Offset,
    },
    /// Jump to older position in jump list (Ctrl-O).
    ///
    /// Navigate backward in the jump list.
    JumpOlder {
        /// Number of positions to go back.
        count: u32,
    },
    /// Jump to newer position in jump list (Ctrl-I).
    ///
    /// Navigate forward in the jump list.
    JumpNewer {
        /// Number of positions to go forward.
        count: u32,
    },
    /// Navigate to a position in a different buffer via jump list.
    ///
    /// Emitted by the executor when Ctrl-O or Ctrl-I targets a jump list
    /// entry whose `buffer_id` differs from the current buffer. The shell
    /// should switch to the target buffer and place the cursor at `offset`.
    JumpToBuffer {
        /// Target buffer to navigate to.
        buffer_id: BufferId,
        /// Cursor offset within the target buffer.
        offset: Offset,
    },
    /// Jump to older change position in changelist (g;).
    ///
    /// Navigate backward in the changelist.
    ChangelistOlder {
        /// Number of positions to go back.
        count: u32,
    },
    /// Jump to newer change position in changelist (g,).
    ///
    /// Navigate forward in the changelist.
    ChangelistNewer {
        /// Number of positions to go forward.
        count: u32,
    },

    // === Undo Effects ===
    /// Begin an undo group.
    ///
    /// All subsequent effects until `EndUndoGroup` are undone together.
    /// The `cursor_strategy` controls where the cursor lands after undo:
    /// `FirstEdit` uses the first text-edit offset, `EntryPosition` uses
    /// the pre-command cursor position (for `o`/`O`, `p`/`P`).
    BeginUndoGroup {
        /// Strategy for cursor placement after undoing this group.
        cursor_strategy: UndoCursorStrategy,
    },
    /// End an undo group.
    ///
    /// Closes the group started by `BeginUndoGroup`.
    EndUndoGroup {
        /// Engine-assigned NodeId for this undo group.
        /// `Some(id)` = node created, host stores ops under this id.
        /// `None` = empty group (no edits), host discards pending.
        node_id: Option<NodeId>,
    },
    /// Request undo.
    ///
    /// Undo the last `count` changes.
    Undo {
        /// Number of changes to undo.
        count: u32,
        /// Navigation steps filled by effect_processor. Empty before processing.
        steps: Vec<UndoNavStep>,
    },
    /// Request line-local undo (`U`).
    ///
    /// Restores changes made on the current line.
    UndoLine {
        /// Number of line-local undo steps to apply.
        count: u32,
    },
    /// Request redo.
    ///
    /// Redo the last `count` undone changes.
    Redo {
        /// Number of changes to redo.
        count: u32,
        /// Navigation steps filled by effect_processor. Empty before processing.
        steps: Vec<UndoNavStep>,
    },

    // === Search Effects ===
    /// Set the search pattern.
    ///
    /// Updates the last search pattern and direction.
    SetSearchPattern {
        /// The regex pattern to search for.
        pattern: CompactString,
        /// Search direction (forward `/` or backward `?`).
        direction: Direction,
    },
    /// Set the last substitute replacement string (for `~` in patterns and replacements).
    SetLastSubstitute {
        /// The replacement string from the most recent `:s` command.
        replacement: CompactString,
    },
    /// Store the flags used by the last `:s` command (for `:&&` repeat-with-flags).
    ///
    /// Carries the full [`SubFlags`] struct, preserving all fields including
    /// `use_last_search` and `reuse_flags` that a bitmask encoding would drop.
    SetLastSubstituteFlags {
        /// The substitute flags from the most recent `:s` command.
        flags: SubFlags,
    },
    /// Set the substitute pattern (Neovim's `RE_SUBST`).
    ///
    /// Updates the substitute pattern store and sets `RE_LAST = RE_SUBST`.
    /// Emitted by `:s/pattern/replacement/` when an explicit pattern is given.
    /// The search pattern (`RE_SEARCH`) is updated separately via `SetSearchPattern`
    /// for hlsearch purposes.
    SetSubstitutePattern {
        /// The regex pattern used by the substitute command.
        pattern: CompactString,
    },
    /// Highlight search matches.
    ///
    /// The shell should visually highlight all provided ranges.
    HighlightMatches {
        /// Ranges to highlight.
        ranges: Vec<Range>,
    },
    /// Clear search highlights.
    ClearHighlights,
    /// Set the last find command state for `;` and `,` repeat.
    ///
    /// Tracks f/F/t/T motion so it can be repeated with `;` or reversed with `,`.
    SetLastFind {
        /// The find direction (f/F/t/T/sneak).
        direction: crate::primitives::FindDirection,
        /// The character that was searched for (first char for sneak).
        target_char: char,
        /// Second character for sneak motions. `None` for f/F/t/T.
        sneak_c2: Option<char>,
        /// Resolved `ignorecase` flag at the time of the original find.
        resolved_ignorecase: bool,
        /// Resolved `smartcase` flag at the time of the original find.
        resolved_smartcase: bool,
    },

    // === Compound Effects (handled by orchestrator) ===
    /// Execute normal-mode keystrokes on each line in range.
    ///
    /// Emitted by `:norm` command. The orchestrator feeds each key through
    /// the engine with cursor positioned at the start of each line.
    NormCommand {
        /// Start line (0-indexed).
        start_line: LineNumber,
        /// End line (0-indexed, inclusive).
        end_line: LineNumber,
        /// Normal-mode keystrokes to execute.
        keys: CompactString,
        /// Whether mappings should be expanded while replaying keys.
        remap: bool,
    },
    /// Request host-driven filter for an operator range (`!{motion}`).
    ///
    /// The host should prompt for/filter through an external command and apply
    /// the transformed text to `range`.
    OperatorFilter {
        /// Target range to filter.
        range: Range,
        /// Motion type used to derive the range.
        motion_type: MotionType,
        /// Explicit register (for Vim-compatible side effects), if any.
        register: Option<RegisterName>,
    },
    /// Request host-driven reindentation for an operator range (`={motion}`).
    ///
    /// The host should auto-indent the text in `range` and return the
    /// reindented text via `HostResult::FilteredRange`.
    OperatorReindent {
        /// Target range to reindent.
        range: Range,
        /// Motion type used to derive the range.
        motion_type: MotionType,
        /// Column of `oap->start` (byte col within the start line).
        /// Used to set mark `[` in the post-reindent text.
        /// See Neovim indent.c:1054: `b_op_start = oap->start`.
        start_col: usize,
        /// Column of `oap->end` (byte col within the end line).
        /// Used to set mark `]` in the post-reindent text.
        /// See Neovim indent.c:1055: `b_op_end = oap->end`.
        end_col: usize,
        /// Number of newlines between `range.start()` and `oap->end` in the
        /// original text.  Used to locate the correct line in the replacement
        /// text for the mark `]` computation.
        end_line_in_range: usize,
        /// Byte offset of `oap->start` in the original text.
        ///
        /// When the linewise range is extended to include a preceding newline
        /// (EOF case in `extend_to_full_lines`), `range.start()` differs from
        /// the actual `oap->start` position. This field carries the true
        /// content position so that `reindent_completion` can compute mark `[`
        /// correctly.
        start_byte_offset: usize,
    },

    // === UI Effects ===
    /// Audible or visual bell (e.g., invalid key in normal mode).
    Bell,
    /// Show a structured informational message.
    ///
    /// Replaces the old `ShowMessage` variant with a richer [`InfoMessage`]
    /// payload that supports free text, line-modification reports, and
    /// verbose-only messages.
    ShowInfo {
        /// The structured info message.
        info: InfoMessage,
    },
    /// Show a warning message.
    ///
    /// For non-fatal notifications that deserve more attention than info,
    /// like "search hit TOP" or "W10: Changing a readonly file".
    ShowWarning {
        /// Warning message text.
        text: CompactString,
    },
    /// Show an error message.
    ///
    /// Carries a typed `VimError` with Vim error codes.
    /// The host calls `error.to_string()` for the display message.
    /// When `source` is `Some`, the error originated from a `:source` script
    /// and the host can display e.g. "init.vim line 42: E486: Pattern not found".
    ShowError {
        /// Typed error with Vim error code.
        error: VimError,
        /// Optional source file/line context (set when error comes from `:source`).
        source: Option<SourceContext>,
    },
    /// Clear the message area.
    ClearMessage,
    /// Scroll to bring offset into view.
    ///
    /// Ensures the cursor is visible without changing cursor position.
    ScrollTo {
        /// Byte offset that must be visible.
        offset: Offset,
    },
    /// Center cursor in viewport (`zz` command).
    CenterCursor,
    /// Scroll cursor to top of viewport (`zt` command).
    CursorToTop,
    /// Scroll cursor to bottom of viewport (`zb` command).
    CursorToBottom,
    /// Scroll viewport left by count columns (`zh` command).
    ScrollLeft {
        /// Number of columns to scroll.
        count: u32,
    },
    /// Scroll viewport right by count columns (`zl` command).
    ScrollRight {
        /// Number of columns to scroll.
        count: u32,
    },
    /// Scroll viewport left by half screen width (`zH` command).
    ScrollHalfScreenLeft {
        /// Number of half-screens to scroll.
        count: u32,
    },
    /// Scroll viewport right by half screen width (`zL` command).
    ScrollHalfScreenRight {
        /// Number of half-screens to scroll.
        count: u32,
    },
    /// Scroll so cursor is at left edge of viewport (`zs` command).
    ScrollCursorToLeftEdge,
    /// Scroll so cursor is at right edge of viewport (`ze` command).
    ScrollCursorToRightEdge,

    // === Macro Effects ===
    /// Start recording a macro into a register.
    ///
    /// The shell should begin capturing keystrokes.
    StartRecording {
        /// Register to record into (validated at grammar boundary).
        register: RegisterName,
    },
    /// Stop recording.
    ///
    /// Finishes macro recording started by `StartRecording`.
    StopRecording,
    /// Play a macro from a register.
    ///
    /// Execute the recorded keystrokes `count` times.
    PlayMacro {
        /// Register containing the macro (validated at grammar boundary).
        register: RegisterName,
        /// Number of times to replay.
        count: u32,
    },

    // === Clipboard Effects ===
    /// Copy text to system clipboard.
    ///
    /// For `"+y` and similar commands that write to system clipboard.
    /// The `register` field indicates the target: `*` (primary selection)
    /// for `clipboard=unnamed`, `+` (system clipboard) for `clipboard=unnamedplus`.
    CopyToClipboard {
        /// Text to copy to clipboard.
        text: CompactString,
        /// Target register (`*` or `+`).
        register: RegisterName,
    },

    // === Search Info Effects ===
    /// Report search match position for "Match N of M" display.
    ///
    /// Emitted after a successful search motion (`n`/`N`/`*`/`#`).
    /// The host can display this info in the status line.
    SearchMatchInfo {
        /// 1-based index of the current match.
        current: u32,
        /// Total number of matches in the document.
        total: u32,
        /// Whether the count is complete (false if timed out or exceeded maxcount).
        complete: bool,
    },

    // === Scroll State Effects ===
    /// Set the sticky half-page scroll count for `Ctrl-D`/`Ctrl-U`.
    ///
    /// When the user provides an explicit count to `Ctrl-D`/`Ctrl-U`,
    /// that count becomes the new default for subsequent half-page scrolls.
    SetScrollHalfCount {
        /// The count to persist (from explicit user input).
        count: u32,
    },

    /// Set the sticky column for vertical motions (Vim's `curswant`).
    ///
    /// Horizontal motions emit this with `Some(VirtualColumn::new(col))`.
    /// `$` emits `Some(VirtualColumn::END_OF_LINE)` for end-of-line stickiness.
    /// Vertical motions do NOT emit this (preserving the existing value).
    SetStickyColumn {
        /// The column to persist, or `None` to clear.
        column: Option<crate::primitives::VirtualColumn>,
    },

    // === Fold Effects ===
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

    // === Window Effects ===
    /// Split window horizontally (`Ctrl-W s`, `:split`).
    WindowSplit,
    /// Request host to open a new empty buffer in a split window (`Ctrl-W n`, `:new`).
    WindowNew,
    /// Split window vertically (`Ctrl-W v`, `:vsplit`).
    WindowVSplit,
    /// Close current window (`Ctrl-W c`, `:close`).
    WindowClose,
    /// Close all windows except current (`Ctrl-W o`, `:only`).
    WindowOnly,
    /// Move to next window (`Ctrl-W w`).
    WindowNext,
    /// Move to previous window (`Ctrl-W W`).
    WindowPrev,
    /// Move cursor to left window (`Ctrl-W h`).
    WindowMoveLeft,
    /// Move cursor to right window (`Ctrl-W l`).
    WindowMoveRight,
    /// Move cursor to window above (`Ctrl-W k`).
    WindowMoveUp,
    /// Move cursor to window below (`Ctrl-W j`).
    WindowMoveDown,
    /// Equalize all window sizes (`Ctrl-W =`).
    WindowEqualSize,
    /// Increase window height by count (`Ctrl-W +`).
    WindowIncreaseHeight {
        /// Number of rows to increase by.
        count: u32,
    },
    /// Decrease window height by count (`Ctrl-W -`).
    WindowDecreaseHeight {
        /// Number of rows to decrease by.
        count: u32,
    },
    /// Increase window width by count (`Ctrl-W >`).
    WindowIncreaseWidth {
        /// Number of columns to increase by.
        count: u32,
    },
    /// Decrease window width by count (`Ctrl-W <`).
    WindowDecreaseWidth {
        /// Number of columns to decrease by.
        count: u32,
    },
    /// Rotate windows downward/rightward (`Ctrl-W r`).
    WindowRotateDown,
    /// Rotate windows upward/leftward (`Ctrl-W R`).
    WindowRotateUp,

    // === Recursive Fold Effects ===
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
    /// Toggle foldenable option (`zi`).
    ToggleFoldEnable,
    /// Set foldenable to specific value (`zn` = false, `zN` = true).
    SetFoldEnable {
        /// Whether folding is enabled.
        enabled: bool,
    },

    // === LSP Navigation Effects ===
    /// Request the host to navigate to the definition of the symbol under
    /// the cursor (`gd`). The resolution mechanism is host-defined (e.g.,
    /// LSP, Godot's symbol lookup, or a custom provider).
    GotoDefinition,
    /// Request the host to display documentation for the symbol under the
    /// cursor (`K`). The display mechanism is host-defined (e.g., hover
    /// tooltip, documentation panel).
    ShowDocumentation,

    /// Open a command-line history window (`q:`, `q/`, `q?`, or `Ctrl-F` from cmdline).
    ///
    /// The host should display an editable history buffer populated with `history`
    /// entries (oldest first). If `prefill` is `Some`, the current partial input
    /// is appended as the last line (from `Ctrl-F` while in command-line mode).
    OpenCommandWindow {
        /// Which command-line history to show.
        prompt: crate::primitives::CommandLinePrompt,
        /// History entries for this prompt kind (oldest first, newest last).
        history: Vec<CompactString>,
        /// Partial input from an active command-line session (`Ctrl-F`).
        /// `None` when opened via `q:` / `q/` / `q?` from normal mode.
        prefill: Option<CompactString>,
    },

    // === Extension Effects ===
    /// Call the host's operatorfunc on a range (`g@{motion}`).
    ///
    /// The host should invoke the registered `operatorfunc` with the given
    /// range and motion type. This is the runtime extension gateway — hosts
    /// can implement custom operators without forking the core.
    CallOperatorFunc {
        /// The range the operator applies to.
        range: Range,
        /// Whether the motion was charwise or linewise.
        motion_type: MotionType,
    },

    // === Host Action Bridge ===
    /// Invoke a host-registered action by name (IdeaVim-style `<Action>()` bridge).
    ///
    /// When a key mapping expands to `<Action>(name)`, the engine resolves
    /// the name from the keymap's action registry and emits this effect.
    /// The host should dispatch to the corresponding editor command.
    HostAction {
        /// The action name (e.g., "ReformatCode", "FindUsages").
        name: CompactString,
    },

    // === Extension State Effects ===
    /// Store opaque state for a named owner (extension/feature).
    ///
    /// Engine-internal: consumed by the effect processor, which stores
    /// the state blob keyed by owner name. Not passed to the host.
    SetExtState {
        /// Owner identifier (e.g., "exchange", "surround").
        owner: CompactString,
        /// Opaque state blob (owner-defined encoding).
        state: Vec<u8>,
    },
    /// Clear state for a named owner (extension/feature).
    ///
    /// Engine-internal: consumed by the effect processor, which removes
    /// the state entry for the owner. Not passed to the host.
    ClearExtState {
        /// Owner identifier whose state to clear.
        owner: CompactString,
    },

    // === Highlight Range Effects ===
    //
    // See [`HIGHLIGHT_OWNER_YANK`] for the well-known owner used by yank.
    /// Set a highlight region for a named owner and highlight group.
    ///
    /// The host should display a visual highlight over `range` using the
    /// style associated with `group`. Multiple highlights can coexist
    /// for different groups within the same owner.
    SetHighlightRange {
        /// Owner identifier (e.g., "exchange", "surround").
        owner: CompactString,
        /// The byte range to highlight.
        range: Range,
        /// Highlight group name (e.g., "pending", "target").
        group: CompactString,
        /// Render shape: char (trapezoid), line (full-width), or block (rectangular).
        shape: crate::primitives::SelectionShape,
    },
    /// Clear highlights for a named owner.
    ///
    /// If `group` is `Some`, only highlights for that group are cleared.
    /// If `group` is `None`, all highlights for the owner are cleared.
    ClearHighlightRange {
        /// Owner identifier whose highlights to clear.
        owner: CompactString,
        /// Optional group to clear. `None` clears all groups.
        group: Option<CompactString>,
    },

    // === Substitute Preview Effects ===
    /// Preview of what a `:s` command would do (inccommand).
    ///
    /// Emitted during command-line editing when `inccommand` is set.
    /// The host should display highlights over the matched ranges and
    /// show the replacement text in the preview area.
    SubstitutePreview {
        /// The list of matches and their replacements.
        matches: Vec<SubstitutePreviewMatch>,
    },
    /// Clear the substitute preview.
    ///
    /// Emitted when the command-line is dismissed or the pattern becomes
    /// empty. The host should remove all substitute preview highlights.
    ClearSubstitutePreview,

    // === Substitute Confirm Effects ===
    /// Show the next match for interactive `:s///c` confirmation.
    ///
    /// Emitted when a `:s/pat/rep/gc` command starts or when the user
    /// presses `n` (skip) or `y` (accept) to advance to the next match.
    /// The host should highlight `match_range` and display a prompt like
    /// "replace with rep (y/n/a/q/l/^E/^Y)?".
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
    ///
    /// Emitted when the user presses `q`, `Esc`, or `l` (last), or when
    /// all matches have been processed. The host should remove the confirm
    /// highlight and prompt.
    SubstituteConfirmEnd,

    // === Substitute Confirm Internal State ===
    // These effects are consumed by the effect processor (never reach the host).
    // They manage the interactive `:s///c` confirm session state.
    /// Store substitute confirm state for an interactive `:s///c` session.
    ///
    /// Engine-internal: consumed by the effect processor, which calls
    /// `state.set_substitute_confirm()`. Not passed to the host.
    SetSubstituteConfirmState {
        /// Initialization data for the confirm session.
        payload: Box<crate::primitives::SubstituteConfirmPayload>,
    },
    /// Clear substitute confirm state (session ended).
    ///
    /// Engine-internal: consumed by the effect processor, which calls
    /// `state.clear_substitute_confirm()`. Not passed to the host.
    ClearSubstituteConfirmState,

    // === Syntax Selection Internal State ===
    // These effects are consumed by the effect processor (never reach the host).
    // They manage the syntax selection history stack.
    /// Push a selections snapshot onto the syntax selection history stack.
    ///
    /// Engine-internal: consumed by the effect processor, which calls
    /// `state.syntax_selection_mut().push()`. Not passed to the host.
    SyntaxSelectionPush {
        /// Snapshot of all cursor selections before expansion.
        snapshot: crate::primitives::Selections,
    },

    /// Pop the top entry from the syntax selection history stack.
    ///
    /// Engine-internal: consumed by the effect processor, which calls
    /// `state.syntax_selection_mut().pop()`. Not passed to the host.
    SyntaxSelectionPop,

    /// Clear the entire syntax selection history stack.
    ///
    /// Engine-internal: consumed by the effect processor, which calls
    /// `state.syntax_selection_mut().clear()`. Not passed to the host.
    SyntaxHistoryClear,

    /// Set the host's cursor selections to match the syntax tree result.
    ///
    /// Engine-internal: consumed by the effect processor. The host reads
    /// the resulting cursor state via the response.
    SetSyntaxSelections {
        /// The new selections to apply.
        selections: crate::primitives::Selections,
    },

    // === Virtual Text / Decoration Effects ===
    /// Place virtual text at a specific position in the buffer.
    ///
    /// Virtual text is rendered by the host alongside the buffer text but
    /// does not modify the document. Common uses: inlay hints, ghost text,
    /// inline annotations. Effects within the same `namespace` can be
    /// cleared together via [`ClearVirtualText`](Self::ClearVirtualText).
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
    ///
    /// Removes every virtual text entry previously set with the matching
    /// `namespace` value. Does not affect other namespaces.
    ClearVirtualText {
        /// Namespace whose virtual text entries should be removed.
        namespace: u32,
    },
    /// Set the full list of diagnostics for a namespace.
    ///
    /// Replaces any previously set diagnostics in the same namespace.
    /// The host renders these as inline markers, gutter icons, or
    /// underlines depending on the UI framework.
    SetDiagnostics {
        /// Namespace identifier (allows independent diagnostic sources).
        namespace: u32,
        /// The complete list of diagnostics (replaces previous set).
        diagnostics: Vec<Diagnostic>,
    },

    // === Fold Range Sync ===
    /// Synchronize syntax-based fold ranges with the host.
    ///
    /// Emitted when the engine computes or receives updated fold ranges
    /// from the syntax provider. The host should replace its fold range
    /// set with the provided list.
    SyncFoldRanges {
        /// The fold ranges as `(start_line, end_line)` pairs (0-indexed).
        ranges: Vec<(LineNumber, LineNumber)>,
    },

    // === Undo Tree Visualization ===
    /// Full undo tree snapshot for visualization.
    ///
    /// Emitted by `:undotree` to provide the host with the complete tree
    /// structure for rendering an interactive undo tree browser.
    /// vim-core is the first embeddable Vim engine with first-class
    /// undo tree visualization as an effect.
    UndoTreeSnapshot {
        /// The complete tree snapshot.
        snapshot: crate::primitives::UndoTreeSnapshot,
    },

    // === Event Effects ===
    /// Typed Vim event for host notification.
    ///
    /// Events are deterministic (ordered in the effect stream), typed
    /// (Rust enums, not strings), and zero-overhead when ignored.
    /// No other embeddable vim engine provides this.
    Event {
        /// The event that occurred.
        kind: crate::primitives::VimEvent,
    },

    // === Cursor Style Effects ===
    /// Recommended cursor shape and blink style for the current mode.
    ///
    /// Emitted by the effect processor immediately after every `SetMode` effect
    /// (when the mode actually changes) so that host adapters do not need their
    /// own mode-to-cursor mapping.
    ///
    /// The host can ignore this effect if it already manages cursor style.
    SetCursorStyle {
        /// The cursor style that matches the new mode.
        style: crate::primitives::CursorStyle,
    },

    // === Operator-Pending Cursor Shape Hint ===
    /// Hint to the host about desired cursor shape based on operator-pending state.
    ///
    /// Emitted when entering operator-pending mode (e.g., `d` pressed) with
    /// `Some(operator)`, and when leaving (motion received, Escape, error)
    /// with `None`. Hosts may ignore this effect — it is purely advisory.
    ///
    /// Host usage:
    /// - `None` → restore normal cursor (block)
    /// - `Some(Delete)` → e.g., red-tinted cursor or half-block
    /// - `Some(Yank)` → e.g., green-tinted cursor
    /// - `Some(Change)` → e.g., bar cursor (since change enters insert)
    CursorShapeHint {
        /// Which operator is pending, or `None` when returning to normal.
        pending_operator: Option<crate::primitives::Operator>,
    },

    // === Insert-Mode Bracket Match Flash ===
    /// Signal the host to briefly highlight the matching bracket.
    ///
    /// Emitted when a closing bracket (`)`, `]`, `}`) is typed in insert mode
    /// with `'showmatch'` enabled. The host should flash the cursor to the
    /// matching opening bracket for `matchtime` tenths of a second.
    ShowMatch {
        /// Byte offset of the matching opening bracket to highlight.
        position: Offset,
    },

    // === Multi-Cursor & Syntax Selection Effects ===
    /// Highlight rows during command execution (e.g., `:norm` line range).
    ///
    /// The host should visually highlight the specified line range with the
    /// given style. `Clear` removes any existing row highlight.
    HighlightRows {
        /// The line range to highlight.
        lines: LineRange,
        /// The highlight style to apply.
        style: HighlightStyle,
    },
    /// Set block selections for multi-cursor visual block mode.
    ///
    /// Provides a set of selection ranges for visual block operations that
    /// affect multiple cursor positions simultaneously.
    SetBlockSelections {
        /// The selection ranges (up to 4 inline, heap-allocated beyond).
        selections: SmallVec<[SelectionRange; 4]>,
    },
    /// Save the host's current selection state under a tag.
    ///
    /// The host should snapshot its selection state so it can be restored
    /// later with [`RestoreSelections`](Self::RestoreSelections).
    SaveSelections {
        /// Tag identifying the selection snapshot.
        tag: SelectionTag,
    },
    /// Restore a previously saved selection state.
    ///
    /// The host should restore the selection state saved under the given tag.
    RestoreSelections {
        /// Tag identifying which snapshot to restore.
        tag: SelectionTag,
    },
    /// Add a selection at the next occurrence of a pattern.
    ///
    /// Multi-cursor command: finds the next match of `pattern` (or the
    /// current word if `None`) and adds a new cursor/selection there.
    SelectNextMatch {
        /// Pattern to search for, or `None` for current word.
        pattern: Option<String>,
        /// If true, skip the match under the current cursor.
        skip_current: bool,
    },
    /// Add a selection at the previous occurrence of a pattern.
    ///
    /// Multi-cursor command: finds the previous match of `pattern` (or the
    /// current word if `None`) and adds a new cursor/selection there.
    SelectPreviousMatch {
        /// Pattern to search for, or `None` for current word.
        pattern: Option<String>,
        /// If true, skip the match under the current cursor.
        skip_current: bool,
    },
    // === No-op / Placeholder ===
    /// No-operation effect.
    ///
    /// Carries no data and produces no visible change. Useful as a
    /// placeholder when a command path must emit at least one effect
    /// but has nothing meaningful to communicate.
    Noop,

    // === Register / Mark Clearing ===
    /// Clear (erase) a named register.
    ///
    /// After this effect the target register is empty. Useful for commands
    /// that need to reset a specific named register without assigning new
    /// content (e.g., clearing a macro register).
    ClearNamedRegister {
        /// The register to clear.
        register: RegisterName,
    },
    /// Delete a named mark.
    ///
    /// After this effect the target mark no longer exists. Mirrors Vim's
    /// `:delmarks {mark}` behaviour for single marks.
    ClearMark {
        /// The mark to delete.
        mark: MarkName,
    },

    // === Variable Store Effects ===
    /// Set a variable in the given scope.
    ///
    /// Stores `value` under `name` in the specified scope (global or buffer).
    /// Overwrites any existing variable with the same name in that scope.
    SetVariable {
        /// Which scope to store in.
        scope: crate::primitives::VarScope,
        /// Variable name.
        name: CompactString,
        /// Value to set.
        value: crate::primitives::VimValue,
    },
    /// Delete a variable from the given scope.
    ///
    /// Removes the variable with the given name from the specified scope.
    /// No-op if the variable does not exist.
    DeleteVariable {
        /// Which scope to delete from.
        scope: crate::primitives::VarScope,
        /// Variable name.
        name: CompactString,
    },

    // === Cross-Buffer Effects ===
    /// Apply edits to a buffer other than the current one.
    ///
    /// The host is responsible for routing this effect to the target buffer
    /// and applying the edits atomically. Requires [`HostCapability::MultiBuffer`](crate::execution::host_api::HostCapability::MultiBuffer).
    CrossBufferEdit {
        /// The target buffer to apply edits to.
        target: BufferId,
        /// The edits to apply (byte-range replacements).
        edits: SmallVec<[crate::primitives::TextEdit; 1]>,
    },

    // === Atomic Mode Transition ===
    /// Atomic mode change bundling mode, cursor style, and appearance.
    ///
    /// Emitted at mode change sites instead of separate `SetMode` + `SetCursorStyle`
    /// effects. Hosts that handle `ModeTransition` get atomic state updates without
    /// transient inconsistency. Middleware may decompose this into individual
    /// `SetMode` + `SetCursorStyle` effects for hosts that don't handle it directly.
    ModeTransition {
        /// The new editing mode.
        mode: Mode,
        /// The cursor style for the new mode.
        cursor_style: crate::primitives::CursorStyle,
        /// Theme color suggestion for the new mode.
        appearance: ModeAppearance,
    },

    // === Timer Effects ===
    /// Request the host to start a timer with the given ID and delay.
    ///
    /// The engine emits this when it needs a deferred callback (e.g., for
    /// CursorHold after `updatetime` ms of inactivity). The host starts the
    /// timer and sends `HostNotification::TimerFired { id }` when it expires.
    /// This preserves the pull-based architecture — the engine never owns a clock.
    ///
    /// If a timer with the same ID is already running, the host should cancel
    /// the old timer and start a new one (debounce semantics).
    RequestTimer {
        /// Opaque timer identifier. The host passes this back in `TimerFired`.
        id: u32,
        /// Delay in milliseconds before the timer fires.
        delay_ms: u32,
    },
}

impl Effect {
    /// Return the stable [`EffectKind`] discriminator for this value.
    #[must_use]
    pub const fn kind(&self) -> EffectKind {
        match self {
            Self::Insert { .. } => EffectKind::Insert,
            Self::Delete { .. } => EffectKind::Delete,
            Self::Replace { .. } => EffectKind::Replace,
            Self::SetCursor { .. } => EffectKind::SetCursor,
            Self::SetSelection { .. } => EffectKind::SetSelection,
            Self::ClearSelection => EffectKind::ClearSelection,
            Self::SaveLastVisual { .. } => EffectKind::SaveLastVisual,
            Self::SetMode { .. } => EffectKind::SetMode,
            Self::CommandLineEdit(_) => EffectKind::CommandLineEdit,
            Self::BeginInsert { .. } => EffectKind::BeginInsert,
            Self::SetBlockInsert { .. } => EffectKind::SetBlockInsert,
            Self::SetRegister { .. } => EffectKind::SetRegister,
            Self::SetMark { .. } => EffectKind::SetMark,
            Self::OperatorToMark { .. } => EffectKind::OperatorToMark,
            Self::PushJumpList { .. } => EffectKind::PushJumpList,
            Self::JumpOlder { .. } => EffectKind::JumpOlder,
            Self::JumpNewer { .. } => EffectKind::JumpNewer,
            Self::JumpToBuffer { .. } => EffectKind::JumpToBuffer,
            Self::ChangelistOlder { .. } => EffectKind::ChangelistOlder,
            Self::ChangelistNewer { .. } => EffectKind::ChangelistNewer,
            Self::BeginUndoGroup { .. } => EffectKind::BeginUndoGroup,
            Self::EndUndoGroup { .. } => EffectKind::EndUndoGroup,
            Self::Undo { .. } => EffectKind::Undo,
            Self::UndoLine { .. } => EffectKind::UndoLine,
            Self::Redo { .. } => EffectKind::Redo,
            Self::SetSearchPattern { .. } => EffectKind::SetSearchPattern,
            Self::SetLastSubstitute { .. } => EffectKind::SetLastSubstitute,
            Self::SetLastSubstituteFlags { .. } => EffectKind::SetLastSubstituteFlags,
            Self::SetSubstitutePattern { .. } => EffectKind::SetSubstitutePattern,
            Self::HighlightMatches { .. } => EffectKind::HighlightMatches,
            Self::ClearHighlights => EffectKind::ClearHighlights,
            Self::SetLastFind { .. } => EffectKind::SetLastFind,
            Self::NormCommand { .. } => EffectKind::NormCommand,
            Self::OperatorFilter { .. } => EffectKind::OperatorFilter,
            Self::OperatorReindent { .. } => EffectKind::OperatorReindent,
            Self::Bell => EffectKind::Bell,
            Self::ShowInfo { .. } => EffectKind::ShowInfo,
            Self::ShowWarning { .. } => EffectKind::ShowWarning,
            Self::ShowError { .. } => EffectKind::ShowError,
            Self::ClearMessage => EffectKind::ClearMessage,
            Self::ScrollTo { .. } => EffectKind::ScrollTo,
            Self::CenterCursor => EffectKind::CenterCursor,
            Self::CursorToTop => EffectKind::CursorToTop,
            Self::CursorToBottom => EffectKind::CursorToBottom,
            Self::ScrollLeft { .. } => EffectKind::ScrollLeft,
            Self::ScrollRight { .. } => EffectKind::ScrollRight,
            Self::ScrollHalfScreenLeft { .. } => EffectKind::ScrollHalfScreenLeft,
            Self::ScrollHalfScreenRight { .. } => EffectKind::ScrollHalfScreenRight,
            Self::ScrollCursorToLeftEdge => EffectKind::ScrollCursorToLeftEdge,
            Self::ScrollCursorToRightEdge => EffectKind::ScrollCursorToRightEdge,
            Self::StartRecording { .. } => EffectKind::StartRecording,
            Self::StopRecording => EffectKind::StopRecording,
            Self::PlayMacro { .. } => EffectKind::PlayMacro,
            Self::CopyToClipboard { .. } => EffectKind::CopyToClipboard,
            Self::SearchMatchInfo { .. } => EffectKind::SearchMatchInfo,
            Self::SetScrollHalfCount { .. } => EffectKind::SetScrollHalfCount,
            Self::SetStickyColumn { .. } => EffectKind::SetStickyColumn,
            Self::FoldLine { .. } => EffectKind::FoldLine,
            Self::UnfoldLine { .. } => EffectKind::UnfoldLine,
            Self::ToggleFold { .. } => EffectKind::ToggleFold,
            Self::ToggleFoldRecursive { .. } => EffectKind::ToggleFoldRecursive,
            Self::FoldAll => EffectKind::FoldAll,
            Self::UnfoldAll => EffectKind::UnfoldAll,
            Self::WindowSplit => EffectKind::WindowSplit,
            Self::WindowNew => EffectKind::WindowNew,
            Self::WindowVSplit => EffectKind::WindowVSplit,
            Self::WindowClose => EffectKind::WindowClose,
            Self::WindowOnly => EffectKind::WindowOnly,
            Self::WindowNext => EffectKind::WindowNext,
            Self::WindowPrev => EffectKind::WindowPrev,
            Self::WindowMoveLeft => EffectKind::WindowMoveLeft,
            Self::WindowMoveRight => EffectKind::WindowMoveRight,
            Self::WindowMoveUp => EffectKind::WindowMoveUp,
            Self::WindowMoveDown => EffectKind::WindowMoveDown,
            Self::WindowEqualSize => EffectKind::WindowEqualSize,
            Self::WindowIncreaseHeight { .. } => EffectKind::WindowIncreaseHeight,
            Self::WindowDecreaseHeight { .. } => EffectKind::WindowDecreaseHeight,
            Self::WindowIncreaseWidth { .. } => EffectKind::WindowIncreaseWidth,
            Self::WindowDecreaseWidth { .. } => EffectKind::WindowDecreaseWidth,
            Self::WindowRotateDown => EffectKind::WindowRotateDown,
            Self::WindowRotateUp => EffectKind::WindowRotateUp,
            Self::FoldLineRecursive { .. } => EffectKind::FoldLineRecursive,
            Self::UnfoldLineRecursive { .. } => EffectKind::UnfoldLineRecursive,
            Self::DeleteFold { .. } => EffectKind::DeleteFold,
            Self::DeleteFoldRecursive { .. } => EffectKind::DeleteFoldRecursive,
            Self::EliminateAllFolds => EffectKind::EliminateAllFolds,
            Self::ToggleFoldEnable => EffectKind::ToggleFoldEnable,
            Self::SetFoldEnable { .. } => EffectKind::SetFoldEnable,
            Self::GotoDefinition => EffectKind::GotoDefinition,
            Self::ShowDocumentation => EffectKind::ShowDocumentation,
            Self::OpenCommandWindow { .. } => EffectKind::OpenCommandWindow,
            Self::CallOperatorFunc { .. } => EffectKind::CallOperatorFunc,
            Self::HostAction { .. } => EffectKind::HostAction,
            Self::SetExtState { .. } => EffectKind::SetExtState,
            Self::ClearExtState { .. } => EffectKind::ClearExtState,
            Self::SetHighlightRange { .. } => EffectKind::SetHighlightRange,
            Self::ClearHighlightRange { .. } => EffectKind::ClearHighlightRange,
            Self::SubstitutePreview { .. } => EffectKind::SubstitutePreview,
            Self::ClearSubstitutePreview => EffectKind::ClearSubstitutePreview,
            Self::SubstituteConfirmShow { .. } => EffectKind::SubstituteConfirmShow,
            Self::SubstituteConfirmEnd => EffectKind::SubstituteConfirmEnd,
            Self::SetSubstituteConfirmState { .. } => EffectKind::SetSubstituteConfirmState,
            Self::ClearSubstituteConfirmState => EffectKind::ClearSubstituteConfirmState,
            Self::SyntaxSelectionPush { .. } => EffectKind::SyntaxSelectionPush,
            Self::SyntaxSelectionPop => EffectKind::SyntaxSelectionPop,
            Self::SyntaxHistoryClear => EffectKind::SyntaxHistoryClear,
            Self::SetSyntaxSelections { .. } => EffectKind::SetSyntaxSelections,
            Self::SetVirtualText { .. } => EffectKind::SetVirtualText,
            Self::ClearVirtualText { .. } => EffectKind::ClearVirtualText,
            Self::SetDiagnostics { .. } => EffectKind::SetDiagnostics,
            Self::SyncFoldRanges { .. } => EffectKind::SyncFoldRanges,
            Self::UndoTreeSnapshot { .. } => EffectKind::UndoTreeSnapshot,
            Self::Event { .. } => EffectKind::Event,
            Self::SetCursorStyle { .. } => EffectKind::SetCursorStyle,
            Self::CursorShapeHint { .. } => EffectKind::CursorShapeHint,
            Self::HighlightRows { .. } => EffectKind::HighlightRows,
            Self::SetBlockSelections { .. } => EffectKind::SetBlockSelections,
            Self::SaveSelections { .. } => EffectKind::SaveSelections,
            Self::RestoreSelections { .. } => EffectKind::RestoreSelections,
            Self::SelectNextMatch { .. } => EffectKind::SelectNextMatch,
            Self::SelectPreviousMatch { .. } => EffectKind::SelectPreviousMatch,
            Self::ShowMatch { .. } => EffectKind::ShowMatch,
            Self::Noop => EffectKind::Noop,
            Self::ClearNamedRegister { .. } => EffectKind::ClearNamedRegister,
            Self::ClearMark { .. } => EffectKind::ClearMark,
            Self::SetVariable { .. } => EffectKind::SetVariable,
            Self::DeleteVariable { .. } => EffectKind::DeleteVariable,
            Self::CrossBufferEdit { .. } => EffectKind::CrossBufferEdit,
            Self::ModeTransition { .. } => EffectKind::ModeTransition,
            Self::RequestTimer { .. } => EffectKind::RequestTimer,
        }
    }

    /// Return the [`EffectTier`] classification for this effect.
    ///
    /// Delegates to [`EffectKind::tier`].
    #[inline]
    #[must_use]
    pub const fn tier(&self) -> EffectTier {
        self.kind().tier()
    }

    /// Returns `true` if this effect mutates the document text.
    ///
    /// Used by the shell's cycle-level text cache to know when to invalidate.
    /// Includes direct buffer mutations (`Insert`, `Delete`, `Replace`, `Undo`,
    /// `UndoLine`, `Redo`) and `OperatorToMark` (a host-executed mutation that
    /// applies an operator to a mark position).
    #[inline]
    #[must_use]
    pub const fn is_text_mutation(&self) -> bool {
        matches!(
            self,
            Self::Insert { .. }
                | Self::Delete { .. }
                | Self::Replace { .. }
                | Self::Undo { .. }
                | Self::UndoLine { .. }
                | Self::Redo { .. }
                | Self::OperatorToMark { .. }
        )
    }

    // === Convenience Constructors ===

    /// Create an `Insert` effect.
    #[inline]
    #[must_use]
    pub fn insert(offset: Offset, text: impl Into<CompactString>) -> Self {
        Self::Insert {
            offset,
            text: text.into(),
        }
    }

    /// Create a `Delete` effect.
    #[inline]
    #[must_use]
    pub const fn delete(range: Range) -> Self {
        Self::Delete { range }
    }

    /// Create a `Replace` effect.
    #[inline]
    #[must_use]
    pub fn replace(range: Range, text: impl Into<CompactString>) -> Self {
        Self::Replace {
            range,
            text: text.into(),
        }
    }

    /// Create a `SetCursor` effect.
    #[inline]
    #[must_use]
    pub const fn set_cursor(offset: Offset) -> Self {
        Self::SetCursor { offset }
    }

    /// Create a `SetMode` effect.
    #[inline]
    #[must_use]
    pub const fn set_mode(mode: Mode) -> Self {
        Self::SetMode {
            mode,
            appearance: ModeAppearance::for_mode(mode),
        }
    }

    /// Create a `CommandLineEdit` effect.
    #[inline]
    #[must_use]
    pub const fn command_line_edit(edit: CmdLineEdit) -> Self {
        Self::CommandLineEdit(edit)
    }

    /// Create a `ShowInfo` effect with free text (convenience wrapper).
    #[inline]
    #[must_use]
    pub fn show_message(text: impl Into<CompactString>) -> Self {
        Self::ShowInfo {
            info: InfoMessage::Text(text.into()),
        }
    }

    /// Create a `ShowWarning` effect.
    #[inline]
    #[must_use]
    pub fn show_warning(text: impl Into<CompactString>) -> Self {
        Self::ShowWarning { text: text.into() }
    }

    /// Create a `ShowError` effect (no source context).
    #[inline]
    #[must_use]
    pub const fn show_error(error: VimError) -> Self {
        Self::ShowError {
            error,
            source: None,
        }
    }

    /// Create a `SetRegister` effect.
    ///
    /// # Linewise invariant
    ///
    /// When `motion_type` is `LineWise`, a trailing `\n` is appended if the
    /// text does not already end with one, matching Neovim's `op_yank`.
    #[must_use]
    pub fn set_register(
        name: RegisterName,
        text: impl Into<CompactString>,
        motion_type: MotionType,
    ) -> Self {
        let mut text = text.into();
        if motion_type == MotionType::LineWise && !text.ends_with('\n') {
            text.push('\n');
        }
        Self::SetRegister {
            name,
            text,
            motion_type,
        }
    }

    /// Create a `SetMark` effect.
    #[inline]
    #[must_use]
    pub const fn set_mark(name: MarkName, offset: Offset, topline_offset: Option<i32>) -> Self {
        Self::SetMark {
            name,
            offset,
            topline_offset,
        }
    }

    /// Create an `OperatorFilter` effect.
    #[inline]
    #[must_use]
    pub const fn operator_filter(
        range: Range,
        motion_type: MotionType,
        register: Option<RegisterName>,
    ) -> Self {
        Self::OperatorFilter {
            range,
            motion_type,
            register,
        }
    }

    /// Create an `OperatorReindent` effect.
    #[inline]
    #[must_use]
    pub const fn operator_reindent(
        range: Range,
        motion_type: MotionType,
        start_col: usize,
        end_col: usize,
        end_line_in_range: usize,
        start_byte_offset: usize,
    ) -> Self {
        Self::OperatorReindent {
            range,
            motion_type,
            start_col,
            end_col,
            end_line_in_range,
            start_byte_offset,
        }
    }

    /// Create a `ModeTransition` effect.
    ///
    /// Bundles mode, cursor style, and appearance atomically. Hosts that
    /// handle this effect get consistent state in a single effect rather
    /// than separate `SetMode` + `SetCursorStyle`.
    #[inline]
    #[must_use]
    pub const fn mode_transition(mode: Mode) -> Self {
        Self::ModeTransition {
            mode,
            cursor_style: crate::primitives::CursorStyle::for_mode(mode),
            appearance: ModeAppearance::for_mode(mode),
        }
    }

    /// Decompose a `ModeTransition` into individual `SetMode` + `SetCursorStyle` effects.
    ///
    /// Returns `None` if `self` is not a `ModeTransition`. Middleware uses this
    /// to split atomic transitions for hosts that handle individual effects.
    #[must_use]
    pub fn decompose_mode_transition(&self) -> Option<[Self; 2]> {
        match self {
            Self::ModeTransition {
                mode,
                cursor_style,
                appearance,
            } => Some([
                Self::SetMode {
                    mode: *mode,
                    appearance: appearance.clone(),
                },
                Self::SetCursorStyle {
                    style: *cursor_style,
                },
            ]),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Compile-time guard: ensure ALL array stays in sync with kind() match.
// If a new EffectKind variant is added without updating ALL, the tests below
// will fail because the kind() exhaustive match is the source of truth.
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod effect_kind_guard_tests {
    use super::*;
    use std::collections::HashSet;

    /// Verify that `EffectKind::ALL` has no duplicates and that its length
    /// matches the expected variant count. If a new variant is added to
    /// `EffectKind` and `kind()` but not to `ALL`, this test will catch it
    /// because the unique count from `ALL` won't match the length.
    #[test]
    fn all_has_no_duplicates() {
        let unique: HashSet<EffectKind> = EffectKind::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            EffectKind::ALL.len(),
            "EffectKind::ALL contains duplicate entries"
        );
    }

    /// Verify that every `Effect` variant's `kind()` is present in `EffectKind::ALL`.
    ///
    /// We construct one representative instance of each `Effect` variant,
    /// call `.kind()`, and assert it appears in `ALL`. If a new `Effect`
    /// variant is added with a new `EffectKind` that isn't in `ALL`, this
    /// test will fail.
    #[test]
    fn all_covers_every_effect_variant() {
        use crate::primitives::{
            BufferId, Direction, FindDirection, InsertEntryType, LineNumber, MarkName, Mode,
            MotionType, Offset, Range, RegisterName, SelectionShape,
        };
        use compact_str::CompactString;

        let all_set: HashSet<EffectKind> = EffectKind::ALL.iter().copied().collect();

        let representative_effects: Vec<Effect> = vec![
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new(""),
            },
            Effect::Delete {
                range: Range::from_raw(0, 1),
            },
            Effect::Replace {
                range: Range::from_raw(0, 1),
                text: CompactString::new(""),
            },
            Effect::SetCursor {
                offset: Offset::new(0),
            },
            Effect::SetSelection {
                anchor: Offset::new(0),
                head: Offset::new(1),
                shape: SelectionShape::Char,
            },
            Effect::ClearSelection,
            Effect::SaveLastVisual {
                info: crate::primitives::LastVisualInfo::new(
                    crate::primitives::VisualType::Char,
                    1,
                    1,
                ),
            },
            Effect::set_mode(Mode::Normal),
            Effect::CommandLineEdit(crate::primitives::CommandLineEdit::Backspace),
            Effect::BeginInsert {
                entry_type: InsertEntryType::BeforeCursor,
                count: 1,
                auto_indent_len: 0,
                entry_offset: Offset::new(0),
            },
            Effect::SetBlockInsert {
                lines_below: 0,
                grapheme_col: 0,
                cursor_return_offset: Offset::new(0),
            },
            Effect::SetRegister {
                name: RegisterName::UNNAMED,
                text: CompactString::new(""),
                motion_type: MotionType::CharWise,
            },
            Effect::SetMark {
                name: MarkName::new_unchecked('a'),
                offset: Offset::new(0),
                topline_offset: None,
            },
            Effect::OperatorToMark {
                operator: crate::primitives::Operator::Delete,
                mark: MarkName::new_unchecked('a'),
                linewise: false,
                register: None,
                cursor: Offset::new(0),
            },
            Effect::PushJumpList {
                offset: Offset::new(0),
            },
            Effect::JumpOlder { count: 1 },
            Effect::JumpNewer { count: 1 },
            Effect::JumpToBuffer {
                buffer_id: BufferId::new(1),
                offset: Offset::new(0),
            },
            Effect::ChangelistOlder { count: 1 },
            Effect::ChangelistNewer { count: 1 },
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::FirstEdit,
            },
            Effect::EndUndoGroup { node_id: None },
            Effect::Undo {
                count: 1,
                steps: vec![],
            },
            Effect::UndoLine { count: 1 },
            Effect::Redo {
                count: 1,
                steps: vec![],
            },
            Effect::SetSearchPattern {
                pattern: CompactString::new(""),
                direction: Direction::Forward,
            },
            Effect::SetLastSubstitute {
                replacement: CompactString::new(""),
            },
            Effect::SetLastSubstituteFlags {
                flags: SubFlags::default(),
            },
            Effect::SetSubstitutePattern {
                pattern: CompactString::new(""),
            },
            Effect::HighlightMatches { ranges: vec![] },
            Effect::ClearHighlights,
            Effect::SetLastFind {
                direction: FindDirection::FindForward,
                target_char: 'a',
                sneak_c2: None,
                resolved_ignorecase: false,
                resolved_smartcase: false,
            },
            Effect::NormCommand {
                start_line: LineNumber::new(0),
                end_line: LineNumber::new(0),
                keys: CompactString::new(""),
                remap: false,
            },
            Effect::OperatorFilter {
                range: Range::from_raw(0, 1),
                motion_type: MotionType::CharWise,
                register: None,
            },
            Effect::OperatorReindent {
                range: Range::from_raw(0, 1),
                motion_type: MotionType::LineWise,
                start_col: 0,
                end_col: 0,
                end_line_in_range: 0,
                start_byte_offset: 0,
            },
            Effect::Bell,
            Effect::ShowInfo {
                info: InfoMessage::Text(CompactString::new("")),
            },
            Effect::ShowWarning {
                text: CompactString::new("test warning"),
            },
            Effect::ShowError {
                error: crate::errors::VimError::NotEditorCommand(CompactString::new("")),
                source: None,
            },
            Effect::ClearMessage,
            Effect::ScrollTo {
                offset: Offset::new(0),
            },
            Effect::CenterCursor,
            Effect::CursorToTop,
            Effect::CursorToBottom,
            Effect::ScrollLeft { count: 1 },
            Effect::ScrollRight { count: 1 },
            Effect::ScrollHalfScreenLeft { count: 1 },
            Effect::ScrollHalfScreenRight { count: 1 },
            Effect::ScrollCursorToLeftEdge,
            Effect::ScrollCursorToRightEdge,
            Effect::StartRecording {
                register: RegisterName::UNNAMED,
            },
            Effect::StopRecording,
            Effect::PlayMacro {
                register: RegisterName::UNNAMED,
                count: 1,
            },
            Effect::CopyToClipboard {
                text: CompactString::new(""),
                register: RegisterName::UNNAMED,
            },
            Effect::SearchMatchInfo {
                current: 1,
                total: 1,
                complete: true,
            },
            Effect::SetScrollHalfCount { count: 10 },
            Effect::SetStickyColumn {
                column: Some(crate::primitives::VirtualColumn::new(5)),
            },
            Effect::FoldLine {
                line: LineNumber::new(0),
            },
            Effect::UnfoldLine {
                line: LineNumber::new(0),
            },
            Effect::ToggleFold {
                line: LineNumber::new(0),
            },
            Effect::ToggleFoldRecursive {
                line: LineNumber::new(0),
            },
            Effect::FoldAll,
            Effect::UnfoldAll,
            Effect::OpenCommandWindow {
                prompt: crate::primitives::CommandLinePrompt::Ex,
                history: vec![],
                prefill: None,
            },
            Effect::CallOperatorFunc {
                range: Range::from_raw(0, 1),
                motion_type: MotionType::CharWise,
            },
            Effect::Event {
                kind: crate::primitives::VimEvent::CursorMoved,
            },
            // Window effects
            Effect::WindowSplit,
            Effect::WindowNew,
            Effect::WindowVSplit,
            Effect::WindowClose,
            Effect::WindowOnly,
            Effect::WindowNext,
            Effect::WindowPrev,
            Effect::WindowMoveLeft,
            Effect::WindowMoveRight,
            Effect::WindowMoveUp,
            Effect::WindowMoveDown,
            Effect::WindowEqualSize,
            Effect::WindowIncreaseHeight { count: 1 },
            Effect::WindowDecreaseHeight { count: 1 },
            Effect::WindowIncreaseWidth { count: 1 },
            Effect::WindowDecreaseWidth { count: 1 },
            Effect::WindowRotateDown,
            Effect::WindowRotateUp,
            // Recursive fold effects
            Effect::FoldLineRecursive {
                line: LineNumber::new(0),
            },
            Effect::UnfoldLineRecursive {
                line: LineNumber::new(0),
            },
            Effect::DeleteFold {
                line: LineNumber::new(0),
            },
            Effect::DeleteFoldRecursive {
                line: LineNumber::new(0),
            },
            Effect::EliminateAllFolds,
            Effect::ToggleFoldEnable,
            Effect::SetFoldEnable { enabled: true },
            // LSP Navigation effects
            Effect::GotoDefinition,
            Effect::ShowDocumentation,
            // Host action bridge
            Effect::HostAction {
                name: CompactString::new("Test"),
            },
            // Extension state (engine-internal)
            Effect::SetExtState {
                owner: CompactString::new("test"),
                state: vec![1, 2, 3],
            },
            Effect::ClearExtState {
                owner: CompactString::new("test"),
            },
            // Highlight range effects
            Effect::SetHighlightRange {
                owner: CompactString::new("test"),
                range: Range::from_raw(0, 5),
                group: CompactString::new("pending"),
                shape: crate::primitives::SelectionShape::Char,
            },
            Effect::ClearHighlightRange {
                owner: CompactString::new("test"),
                group: None,
            },
            // Substitute preview
            Effect::SubstitutePreview { matches: vec![] },
            Effect::ClearSubstitutePreview,
            // Syntax selection history (engine-internal)
            Effect::SyntaxSelectionPush {
                snapshot: crate::primitives::Selections::single(
                    crate::primitives::SelectionRange::new(Offset::new(0), Offset::new(10)),
                ),
            },
            Effect::SyntaxSelectionPop,
            Effect::SyntaxHistoryClear,
            Effect::SetSyntaxSelections {
                selections: crate::primitives::Selections::single(
                    crate::primitives::SelectionRange::new(Offset::new(0), Offset::new(10)),
                ),
            },
            // Virtual text / decoration effects
            Effect::SetVirtualText {
                namespace: 0,
                line: LineNumber::new(0),
                col: Offset::new(0),
                text: CompactString::new(""),
                position: crate::primitives::VirtualTextPosition::Eol,
            },
            Effect::ClearVirtualText { namespace: 0 },
            Effect::SetDiagnostics {
                namespace: 0,
                diagnostics: vec![],
            },
            // Fold range sync
            Effect::SyncFoldRanges { ranges: vec![] },
            // Undo tree visualization
            Effect::UndoTreeSnapshot {
                snapshot: crate::primitives::UndoTreeSnapshot {
                    nodes: vec![],
                    current: crate::primitives::NodeId::ROOT,
                    change_count: 0,
                },
            },
            // Cursor style
            Effect::SetCursorStyle {
                style: crate::primitives::CursorStyle {
                    shape: crate::primitives::CursorShape::Block,
                    blink: false,
                },
            },
            // Operator-pending cursor shape hint
            Effect::CursorShapeHint {
                pending_operator: Some(crate::primitives::Operator::Delete),
            },
            // Insert-mode bracket match flash
            Effect::ShowMatch {
                position: Offset::new(0),
            },
            // Multi-cursor & syntax selection
            Effect::HighlightRows {
                lines: crate::primitives::LineRange::new(LineNumber::new(0), LineNumber::new(5)),
                style: HighlightStyle::Active,
            },
            Effect::SetBlockSelections {
                selections: SmallVec::new(),
            },
            Effect::SaveSelections {
                tag: SelectionTag::Search,
            },
            Effect::RestoreSelections {
                tag: SelectionTag::Search,
            },
            Effect::SelectNextMatch {
                pattern: None,
                skip_current: false,
            },
            Effect::SelectPreviousMatch {
                pattern: None,
                skip_current: false,
            },
            // Substitute confirm
            Effect::SubstituteConfirmShow {
                match_range: Range::from_raw(0, 3),
                replacement: CompactString::new("bar"),
                match_index: 1,
                total_matches: 3,
            },
            Effect::SubstituteConfirmEnd,
            // Substitute confirm state (engine-internal)
            Effect::SetSubstituteConfirmState {
                payload: Box::new(crate::primitives::SubstituteConfirmPayload {
                    matches: vec![],
                    replacement: CompactString::new(""),
                    pattern: CompactString::new(""),
                    flags: SubFlags::default(),
                    gdefault: false,
                }),
            },
            Effect::ClearSubstituteConfirmState,
            // No-op / register and mark clearing
            Effect::Noop,
            Effect::ClearNamedRegister {
                register: RegisterName::UNNAMED,
            },
            Effect::ClearMark {
                mark: MarkName::new_unchecked('a'),
            },
            Effect::SetVariable {
                scope: crate::primitives::VarScope::Global,
                name: CompactString::new("x"),
                value: crate::primitives::VimValue::Nil,
            },
            Effect::DeleteVariable {
                scope: crate::primitives::VarScope::Buffer,
                name: CompactString::new("y"),
            },
            Effect::CrossBufferEdit {
                target: BufferId::new(1),
                edits: SmallVec::new(),
            },
            Effect::mode_transition(Mode::Normal),
            Effect::RequestTimer {
                id: 1,
                delay_ms: 4000,
            },
        ];

        // Each representative effect's kind must be in ALL
        for effect in &representative_effects {
            let kind = effect.kind();
            assert!(
                all_set.contains(&kind),
                "EffectKind::{kind:?} is returned by Effect::kind() but missing from EffectKind::ALL"
            );
        }

        // ALL must not contain kinds that no Effect variant produces
        let produced_kinds: HashSet<EffectKind> =
            representative_effects.iter().map(Effect::kind).collect();
        for kind in &EffectKind::ALL {
            assert!(
                produced_kinds.contains(kind),
                "EffectKind::{kind:?} is in ALL but no Effect variant produces it"
            );
        }

        // Final count check
        assert_eq!(
            EffectKind::ALL.len(),
            representative_effects.len(),
            "EffectKind::ALL length ({}) doesn't match the number of Effect variants ({}). \
             A new variant was likely added without updating ALL.",
            EffectKind::ALL.len(),
            representative_effects.len(),
        );
    }
}

#[cfg(test)]
mod effect_tier_tests {
    use super::*;

    #[test]
    fn every_effect_kind_has_a_tier() {
        // Calling tier() on every variant exercises the exhaustive match.
        // If a variant were missing, compilation would fail.
        for kind in EffectKind::ALL {
            let _tier = kind.tier();
        }
    }

    #[test]
    fn core_tier_count_is_16() {
        let core_count = EffectKind::ALL
            .iter()
            .filter(|k| k.tier() == EffectTier::Core)
            .count();
        assert_eq!(
            core_count, 16,
            "Expected 16 Core-tier effects, got {core_count}"
        );
    }

    #[test]
    fn is_text_mutation_returns_true_for_exactly_7() {
        // 6 direct buffer mutations + OperatorToMark (host-executed mutation).
        let mutation_count = EffectKind::ALL
            .iter()
            .filter(|k| k.is_text_mutation())
            .count();
        assert_eq!(
            mutation_count, 7,
            "Expected 7 text-mutation effects, got {mutation_count}"
        );
    }

    #[test]
    fn is_text_mutation_specific_variants() {
        let expected = [
            EffectKind::Insert,
            EffectKind::Delete,
            EffectKind::Replace,
            EffectKind::Undo,
            EffectKind::UndoLine,
            EffectKind::Redo,
            EffectKind::OperatorToMark,
        ];
        for kind in &expected {
            assert!(
                kind.is_text_mutation(),
                "{kind:?} should be a text mutation"
            );
        }
    }

    #[test]
    fn all_variants_classified_no_gaps() {
        // Every variant in ALL must map to one of the four tiers.
        for kind in EffectKind::ALL {
            let tier = kind.tier();
            assert!(
                matches!(
                    tier,
                    EffectTier::Core
                        | EffectTier::Standard
                        | EffectTier::Advanced
                        | EffectTier::Internal
                ),
                "{kind:?} returned an unexpected tier: {tier:?}"
            );
        }
    }

    #[test]
    fn tier_counts_sum_to_all() {
        let total: usize = [
            EffectTier::Core,
            EffectTier::Standard,
            EffectTier::Advanced,
            EffectTier::Internal,
        ]
        .iter()
        .map(|tier| EffectKind::ALL.iter().filter(|k| k.tier() == *tier).count())
        .sum();
        assert_eq!(
            total,
            EffectKind::ALL.len(),
            "Tier counts should sum to {}, got {total}",
            EffectKind::ALL.len()
        );
    }

    #[test]
    fn request_timer_is_advanced_tier() {
        assert_eq!(EffectKind::RequestTimer.tier(), EffectTier::Advanced);
    }

    #[test]
    fn request_timer_effect_round_trips_kind() {
        let effect = Effect::RequestTimer {
            id: 42,
            delay_ms: 4000,
        };
        assert_eq!(effect.kind(), EffectKind::RequestTimer);
    }

    #[test]
    fn request_timer_is_not_text_mutation() {
        assert!(!EffectKind::RequestTimer.is_text_mutation());
    }
}
