//! Machine-verified effect commutativity classification.
//!
//! Groups the 117 [`EffectKind`] variants into semantic categories and provides
//! a complete commutativity table for all category pairs. This enables:
//!
//! - **Optimisation passes**: reorder commutative effects for batching.
//! - **Validation**: detect non-commutative reorderings that would change semantics.
//! - **Documentation**: machine-checked specification of effect independence.
//!
//! # Categories
//!
//! | Category         | Representative effects                                |
//! |------------------|-------------------------------------------------------|
//! | `TextMutation`   | Insert, Delete, Replace, Undo, UndoLine, Redo         |
//! | `CursorMove`     | SetCursor, ScrollTo, CenterCursor, CursorToTop/Bottom |
//! | `ModeChange`     | SetMode, BeginInsert, SetBlockInsert, CommandLineEdit  |
//! | `RegisterOp`     | SetRegister, ClearNamedRegister, CopyToClipboard       |
//! | `MarkOp`         | SetMark, ClearMark, PushJumpList, Jump*, Changelist*   |
//! | `UndoBracket`    | BeginUndoGroup, EndUndoGroup                          |
//! | `Selection`      | SetSelection, ClearSelection, SaveLastVisual           |
//! | `ViewState`      | SetCursorStyle, ScrollLeft/Right, SetScrollHalfCount   |
//! | `SearchState`    | SetSearchPattern, HighlightMatches, ClearHighlights, etc. |
//! | `FoldOp`         | FoldLine, UnfoldLine, FoldAll, etc.                   |
//! | `WindowOp`       | WindowSplit, WindowClose, WindowNext, etc.            |
//! | `Notification`   | Bell, ShowInfo, ShowWarning, ShowError, ClearMessage  |
//! | `Internal`       | Noop, SetExtState, etc.                               |
//!
//! # Import constraints
//!
//! This module only depends on:
//! - sibling `effect` module -- `EffectKind`

use crate::effects::effect::EffectKind;

// =============================================================================
// EFFECT CATEGORY
// =============================================================================

/// Semantic grouping of [`EffectKind`] variants for commutativity analysis.
///
/// Each category represents a class of effects that operate on the same logical
/// state domain. Effects in different categories are often commutative because
/// they touch independent state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectCategory {
    /// Text mutations: Insert, Delete, Replace, Undo, UndoLine, Redo.
    /// These modify the document buffer and are offset-sensitive.
    TextMutation,

    /// Cursor positioning: SetCursor, ScrollTo, CenterCursor, CursorToTop,
    /// CursorToBottom. These set the viewport/cursor position.
    CursorMove,

    /// Mode transitions: SetMode, BeginInsert, SetBlockInsert, CommandLineEdit.
    /// These change the editing mode state machine.
    ModeChange,

    /// Register operations: SetRegister, ClearNamedRegister, CopyToClipboard.
    /// These modify the register file (independent of document content).
    RegisterOp,

    /// Mark and jump list operations: SetMark, ClearMark, PushJumpList,
    /// JumpOlder, JumpNewer, JumpToBuffer, ChangelistOlder, ChangelistNewer.
    MarkOp,

    /// Undo group brackets: BeginUndoGroup, EndUndoGroup.
    /// These are structural delimiters that must maintain strict ordering.
    UndoBracket,

    /// Selection state: SetSelection, ClearSelection, SaveLastVisual.
    /// These control the visual selection overlay.
    Selection,

    /// View/display state: SetCursorStyle, ScrollLeft, ScrollRight,
    /// SetScrollHalfCount, SetStickyColumn,
    /// SetHighlightRange, ClearHighlightRange, HighlightRows.
    ViewState,

    /// Search state: SetSearchPattern, SetSubstitutePattern, HighlightMatches,
    /// ClearHighlights, SetLastFind, SetLastSubstitute, SetLastSubstituteFlags,
    /// SearchMatchInfo.
    SearchState,

    /// Fold operations: FoldLine, UnfoldLine, ToggleFold, FoldAll, etc.
    /// These modify the fold tree which is independent of document text.
    FoldOp,

    /// Window management: WindowSplit, WindowClose, WindowNext, etc.
    /// These control the window layout.
    WindowOp,

    /// User notifications: Bell, ShowInfo, ShowWarning, ShowError, ClearMessage.
    /// Pure display feedback with no state mutation.
    Notification,

    /// Engine-internal effects: Noop,
    /// SetExtState, ClearExtState, SyntaxSelectionPush/Pop, NormCommand,
    /// OperatorFilter, OperatorReindent, PlayMacro, StartRecording,
    /// StopRecording, Event, GotoDefinition, ShowDocumentation, HostAction,
    /// OpenCommandWindow, CallOperatorFunc, SubstitutePreview,
    /// ClearSubstitutePreview, SetVirtualText, ClearVirtualText,
    /// SetDiagnostics, UndoTreeSnapshot.
    Internal,
}

impl EffectCategory {
    /// All category variants in definition order.
    pub const ALL: [Self; 13] = [
        Self::TextMutation,
        Self::CursorMove,
        Self::ModeChange,
        Self::RegisterOp,
        Self::MarkOp,
        Self::UndoBracket,
        Self::Selection,
        Self::ViewState,
        Self::SearchState,
        Self::FoldOp,
        Self::WindowOp,
        Self::Notification,
        Self::Internal,
    ];
}

// =============================================================================
// COMMUTATIVITY CLASSIFICATION
// =============================================================================

/// Classification of how two effects interact when reordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Commutativity {
    /// Order does not matter: `apply(a, b)` and `apply(b, a)` produce
    /// identical final state. Safe to reorder freely.
    Commutative,

    /// Order matters: `apply(a, b)` and `apply(b, a)` may produce different
    /// results. Must preserve original ordering.
    NonCommutative,

    /// Only the last effect of this type matters. Earlier effects of the same
    /// category are superseded. Safe to drop all but the last.
    LastWins,
}

// =============================================================================
// CATEGORIZATION
// =============================================================================

/// Map an [`EffectKind`] to its semantic [`EffectCategory`].
///
/// This match is exhaustive -- adding a new `EffectKind` variant without
/// categorizing it here will cause a compile error.
#[must_use]
pub const fn categorize(kind: EffectKind) -> EffectCategory {
    match kind {
        // TextMutation: document buffer modifications (direct and host-executed)
        EffectKind::Insert
        | EffectKind::Delete
        | EffectKind::Replace
        | EffectKind::Undo
        | EffectKind::UndoLine
        | EffectKind::Redo
        | EffectKind::OperatorToMark => EffectCategory::TextMutation,

        // CursorMove: cursor/viewport positioning
        EffectKind::SetCursor
        | EffectKind::ScrollTo
        | EffectKind::CenterCursor
        | EffectKind::CursorToTop
        | EffectKind::CursorToBottom => EffectCategory::CursorMove,

        // ModeChange: editing mode transitions
        EffectKind::SetMode
        | EffectKind::BeginInsert
        | EffectKind::SetBlockInsert
        | EffectKind::CommandLineEdit => EffectCategory::ModeChange,

        // RegisterOp: register file modifications
        EffectKind::SetRegister | EffectKind::ClearNamedRegister | EffectKind::CopyToClipboard => {
            EffectCategory::RegisterOp
        }

        // MarkOp: marks and jump list
        EffectKind::SetMark
        | EffectKind::ClearMark
        | EffectKind::PushJumpList
        | EffectKind::JumpOlder
        | EffectKind::JumpNewer
        | EffectKind::JumpToBuffer
        | EffectKind::ChangelistOlder
        | EffectKind::ChangelistNewer => EffectCategory::MarkOp,

        // UndoBracket: undo group delimiters
        EffectKind::BeginUndoGroup | EffectKind::EndUndoGroup => EffectCategory::UndoBracket,

        // Selection: visual selection state
        EffectKind::SetSelection
        | EffectKind::ClearSelection
        | EffectKind::SaveLastVisual
        | EffectKind::SetBlockSelections
        | EffectKind::SaveSelections
        | EffectKind::RestoreSelections
        | EffectKind::SelectNextMatch
        | EffectKind::SelectPreviousMatch => EffectCategory::Selection,

        // ViewState: display/decoration state
        EffectKind::SetCursorStyle
        | EffectKind::CursorShapeHint
        | EffectKind::ShowMatch
        | EffectKind::ScrollLeft
        | EffectKind::ScrollRight
        | EffectKind::ScrollHalfScreenLeft
        | EffectKind::ScrollHalfScreenRight
        | EffectKind::ScrollCursorToLeftEdge
        | EffectKind::ScrollCursorToRightEdge
        | EffectKind::SetScrollHalfCount
        | EffectKind::SetStickyColumn
        | EffectKind::SetHighlightRange
        | EffectKind::ClearHighlightRange
        | EffectKind::HighlightRows => EffectCategory::ViewState,

        // SearchState: search/substitute pattern state
        EffectKind::SetSearchPattern
        | EffectKind::SetSubstitutePattern
        | EffectKind::HighlightMatches
        | EffectKind::ClearHighlights
        | EffectKind::SetLastFind
        | EffectKind::SetLastSubstitute
        | EffectKind::SetLastSubstituteFlags
        | EffectKind::SearchMatchInfo => EffectCategory::SearchState,

        // FoldOp: fold tree modifications
        EffectKind::FoldLine
        | EffectKind::UnfoldLine
        | EffectKind::ToggleFold
        | EffectKind::ToggleFoldRecursive
        | EffectKind::FoldAll
        | EffectKind::UnfoldAll
        | EffectKind::FoldLineRecursive
        | EffectKind::UnfoldLineRecursive
        | EffectKind::DeleteFold
        | EffectKind::DeleteFoldRecursive
        | EffectKind::EliminateAllFolds
        | EffectKind::ToggleFoldEnable
        | EffectKind::SetFoldEnable
        | EffectKind::SyncFoldRanges => EffectCategory::FoldOp,

        // WindowOp: window layout management
        EffectKind::WindowSplit
        | EffectKind::WindowNew
        | EffectKind::WindowVSplit
        | EffectKind::WindowClose
        | EffectKind::WindowOnly
        | EffectKind::WindowNext
        | EffectKind::WindowPrev
        | EffectKind::WindowMoveLeft
        | EffectKind::WindowMoveRight
        | EffectKind::WindowMoveUp
        | EffectKind::WindowMoveDown
        | EffectKind::WindowEqualSize
        | EffectKind::WindowIncreaseHeight
        | EffectKind::WindowDecreaseHeight
        | EffectKind::WindowIncreaseWidth
        | EffectKind::WindowDecreaseWidth
        | EffectKind::WindowRotateDown
        | EffectKind::WindowRotateUp => EffectCategory::WindowOp,

        // Notification: user-facing messages
        EffectKind::Bell
        | EffectKind::ShowInfo
        | EffectKind::ShowWarning
        | EffectKind::ShowError
        | EffectKind::ClearMessage => EffectCategory::Notification,

        // Internal: engine-internal bookkeeping and host bridge
        EffectKind::Noop
        | EffectKind::SetExtState
        | EffectKind::ClearExtState
        | EffectKind::SyntaxSelectionPush
        | EffectKind::SyntaxSelectionPop
        | EffectKind::SyntaxHistoryClear
        | EffectKind::SetSyntaxSelections
        | EffectKind::NormCommand
        | EffectKind::OperatorFilter
        | EffectKind::OperatorReindent
        | EffectKind::PlayMacro
        | EffectKind::StartRecording
        | EffectKind::StopRecording
        | EffectKind::Event
        | EffectKind::GotoDefinition
        | EffectKind::ShowDocumentation
        | EffectKind::HostAction
        | EffectKind::OpenCommandWindow
        | EffectKind::CallOperatorFunc
        | EffectKind::SubstitutePreview
        | EffectKind::ClearSubstitutePreview
        | EffectKind::SetVirtualText
        | EffectKind::ClearVirtualText
        | EffectKind::SetDiagnostics
        | EffectKind::UndoTreeSnapshot
        | EffectKind::SubstituteConfirmShow
        | EffectKind::SubstituteConfirmEnd
        | EffectKind::SetSubstituteConfirmState
        | EffectKind::ClearSubstituteConfirmState
        | EffectKind::SetVariable
        | EffectKind::DeleteVariable
        | EffectKind::CrossBufferEdit
        | EffectKind::ModeTransition
        | EffectKind::RequestTimer => EffectCategory::Internal,
    }
}

// =============================================================================
// COMMUTATIVITY TABLE
// =============================================================================

/// Classify the commutativity of two effect categories.
///
/// The classification captures how the final state depends on ordering:
///
/// | A \ B            | TextMut | Cursor  | Mode    | Reg     | Mark    | Undo    | Sel     | View    | Search  | Fold    | Window  | Notif   | Internal |
/// |------------------|---------|---------|---------|---------|---------|---------|---------|---------|---------|---------|---------|---------|----------|
/// | **TextMutation** | NonComm | NonComm | Comm    | Comm    | NonComm | NonComm | NonComm | Comm    | Comm    | NonComm | Comm    | Comm    | NonComm  |
/// | **CursorMove**   | NonComm | LastW   | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm     |
/// | **ModeChange**   | Comm    | Comm    | LastW   | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm     |
/// | **RegisterOp**   | Comm    | Comm    | Comm    | NonComm | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm     |
/// | **MarkOp**       | NonComm | Comm    | Comm    | Comm    | NonComm | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm     |
/// | **UndoBracket**  | NonComm | Comm    | Comm    | Comm    | Comm    | NonComm | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm     |
/// | **Selection**    | NonComm | Comm    | Comm    | Comm    | Comm    | Comm    | LastW   | Comm    | Comm    | Comm    | Comm    | Comm    | Comm     |
/// | **ViewState**    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | NonComm | Comm    | Comm    | Comm    | Comm    | Comm     |
/// | **SearchState**  | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | NonComm | Comm    | Comm    | Comm    | Comm     |
/// | **FoldOp**       | NonComm | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | NonComm | Comm    | Comm    | Comm     |
/// | **WindowOp**     | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | NonComm | Comm    | Comm     |
/// | **Notification** | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | LastW   | Comm     |
/// | **Internal**     | NonComm | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | Comm    | NonComm  |
///
/// # Design rationale
///
/// - **TextMutation + TextMutation -> NonCommutative**: Insert at offset 5 then
///   Insert at offset 3 differs from Insert at 3 then Insert at 5 (the first
///   insert shifts subsequent offsets).
///
/// - **CursorMove + CursorMove -> LastWins**: Two SetCursor effects -- only the
///   final cursor position matters.
///
/// - **ModeChange + ModeChange -> LastWins**: Two SetMode effects -- only the
///   final mode matters.
///
/// - **TextMutation + CursorMove -> NonCommutative**: An Insert shifts the byte
///   offsets that a subsequent SetCursor refers to.
///
/// - **TextMutation + ModeChange -> Commutative**: Inserting text and changing
///   mode affect independent state domains.
///
/// - **TextMutation + MarkOp -> NonCommutative**: Text mutations shift mark
///   offsets.
///
/// - **TextMutation + UndoBracket -> NonCommutative**: Undo brackets must wrap
///   their text mutations; reordering breaks undo semantics.
///
/// - **TextMutation + Selection -> NonCommutative**: Text mutations shift
///   selection anchor/head offsets.
///
/// - **TextMutation + FoldOp -> NonCommutative**: Text mutations (inserts,
///   deletes) shift line numbers, which invalidates fold ranges stored as
///   line-number pairs. A fold created before a text mutation may point to
///   the wrong lines after the mutation.
///
/// - **TextMutation + Internal -> NonCommutative**: Some internals
///   (NormCommand, OperatorFilter) carry ranges affected by text mutations.
///
/// - **Selection + Selection -> LastWins**: Only the final selection state
///   matters.
///
/// - **Notification + Notification -> LastWins**: Only the last message
///   displayed is visible to the user.
///
/// - **Same-category generally -> NonCommutative**: Two effects in the same
///   domain often interact (except cursor, mode, selection, notification
///   which are last-wins).
#[must_use]
pub const fn classify_commutativity(a: EffectCategory, b: EffectCategory) -> Commutativity {
    use Commutativity as R;
    use EffectCategory as C;

    match (a, b) {
        // ── Same-category pairs ──────────────────────────────────────────
        (C::TextMutation, C::TextMutation) => R::NonCommutative,
        (C::CursorMove, C::CursorMove) => R::LastWins,
        (C::ModeChange, C::ModeChange) => R::LastWins,
        (C::RegisterOp, C::RegisterOp) => R::NonCommutative,
        (C::MarkOp, C::MarkOp) => R::NonCommutative,
        (C::UndoBracket, C::UndoBracket) => R::NonCommutative,
        (C::Selection, C::Selection) => R::LastWins,
        (C::ViewState, C::ViewState) => R::NonCommutative,
        (C::SearchState, C::SearchState) => R::NonCommutative,
        (C::FoldOp, C::FoldOp) => R::NonCommutative,
        (C::WindowOp, C::WindowOp) => R::NonCommutative,
        (C::Notification, C::Notification) => R::LastWins,
        (C::Internal, C::Internal) => R::NonCommutative,

        // ── TextMutation cross-category ──────────────────────────────────
        // Text mutations shift offsets, so cursor/mark/selection/undo/internal
        // are non-commutative. Mode/register/view/search/fold/window/notif
        // are independent state domains.
        (C::TextMutation, C::CursorMove) | (C::CursorMove, C::TextMutation) => R::NonCommutative,
        (C::TextMutation, C::ModeChange) | (C::ModeChange, C::TextMutation) => R::Commutative,
        (C::TextMutation, C::RegisterOp) | (C::RegisterOp, C::TextMutation) => R::Commutative,
        (C::TextMutation, C::MarkOp) | (C::MarkOp, C::TextMutation) => R::NonCommutative,
        (C::TextMutation, C::UndoBracket) | (C::UndoBracket, C::TextMutation) => R::NonCommutative,
        (C::TextMutation, C::Selection) | (C::Selection, C::TextMutation) => R::NonCommutative,
        (C::TextMutation, C::ViewState) | (C::ViewState, C::TextMutation) => R::Commutative,
        (C::TextMutation, C::SearchState) | (C::SearchState, C::TextMutation) => R::Commutative,
        (C::TextMutation, C::FoldOp) | (C::FoldOp, C::TextMutation) => R::NonCommutative,
        (C::TextMutation, C::WindowOp) | (C::WindowOp, C::TextMutation) => R::Commutative,
        (C::TextMutation, C::Notification) | (C::Notification, C::TextMutation) => R::Commutative,
        (C::TextMutation, C::Internal) | (C::Internal, C::TextMutation) => R::NonCommutative,

        // ── CursorMove cross-category ────────────────────────────────────
        // Cursor is independent of mode, registers, marks, undo brackets,
        // selection, view state, search, folds, windows, notifications, internals.
        (C::CursorMove, C::ModeChange) | (C::ModeChange, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::RegisterOp) | (C::RegisterOp, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::MarkOp) | (C::MarkOp, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::UndoBracket) | (C::UndoBracket, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::Selection) | (C::Selection, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::ViewState) | (C::ViewState, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::SearchState) | (C::SearchState, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::FoldOp) | (C::FoldOp, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::WindowOp) | (C::WindowOp, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::Notification) | (C::Notification, C::CursorMove) => R::Commutative,
        (C::CursorMove, C::Internal) | (C::Internal, C::CursorMove) => R::Commutative,

        // ── ModeChange cross-category ────────────────────────────────────
        // Mode is independent of everything except itself (last-wins above)
        // and TextMutation (commutative above).
        (C::ModeChange, C::RegisterOp) | (C::RegisterOp, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::MarkOp) | (C::MarkOp, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::UndoBracket) | (C::UndoBracket, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::Selection) | (C::Selection, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::ViewState) | (C::ViewState, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::SearchState) | (C::SearchState, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::FoldOp) | (C::FoldOp, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::WindowOp) | (C::WindowOp, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::Notification) | (C::Notification, C::ModeChange) => R::Commutative,
        (C::ModeChange, C::Internal) | (C::Internal, C::ModeChange) => R::Commutative,

        // ── RegisterOp cross-category ────────────────────────────────────
        // Registers are independent of marks, undo, selection, view, search,
        // folds, windows, notifications, internals.
        (C::RegisterOp, C::MarkOp) | (C::MarkOp, C::RegisterOp) => R::Commutative,
        (C::RegisterOp, C::UndoBracket) | (C::UndoBracket, C::RegisterOp) => R::Commutative,
        (C::RegisterOp, C::Selection) | (C::Selection, C::RegisterOp) => R::Commutative,
        (C::RegisterOp, C::ViewState) | (C::ViewState, C::RegisterOp) => R::Commutative,
        (C::RegisterOp, C::SearchState) | (C::SearchState, C::RegisterOp) => R::Commutative,
        (C::RegisterOp, C::FoldOp) | (C::FoldOp, C::RegisterOp) => R::Commutative,
        (C::RegisterOp, C::WindowOp) | (C::WindowOp, C::RegisterOp) => R::Commutative,
        (C::RegisterOp, C::Notification) | (C::Notification, C::RegisterOp) => R::Commutative,
        (C::RegisterOp, C::Internal) | (C::Internal, C::RegisterOp) => R::Commutative,

        // ── MarkOp cross-category ────────────────────────────────────────
        // Marks are independent of undo brackets, selection, view, search,
        // folds, windows, notifications, internals.
        (C::MarkOp, C::UndoBracket) | (C::UndoBracket, C::MarkOp) => R::Commutative,
        (C::MarkOp, C::Selection) | (C::Selection, C::MarkOp) => R::Commutative,
        (C::MarkOp, C::ViewState) | (C::ViewState, C::MarkOp) => R::Commutative,
        (C::MarkOp, C::SearchState) | (C::SearchState, C::MarkOp) => R::Commutative,
        (C::MarkOp, C::FoldOp) | (C::FoldOp, C::MarkOp) => R::Commutative,
        (C::MarkOp, C::WindowOp) | (C::WindowOp, C::MarkOp) => R::Commutative,
        (C::MarkOp, C::Notification) | (C::Notification, C::MarkOp) => R::Commutative,
        (C::MarkOp, C::Internal) | (C::Internal, C::MarkOp) => R::Commutative,

        // ── UndoBracket cross-category ───────────────────────────────────
        // Undo brackets are independent of selection, view, search, folds,
        // windows, notifications, internals.
        (C::UndoBracket, C::Selection) | (C::Selection, C::UndoBracket) => R::Commutative,
        (C::UndoBracket, C::ViewState) | (C::ViewState, C::UndoBracket) => R::Commutative,
        (C::UndoBracket, C::SearchState) | (C::SearchState, C::UndoBracket) => R::Commutative,
        (C::UndoBracket, C::FoldOp) | (C::FoldOp, C::UndoBracket) => R::Commutative,
        (C::UndoBracket, C::WindowOp) | (C::WindowOp, C::UndoBracket) => R::Commutative,
        (C::UndoBracket, C::Notification) | (C::Notification, C::UndoBracket) => R::Commutative,
        (C::UndoBracket, C::Internal) | (C::Internal, C::UndoBracket) => R::Commutative,

        // ── Selection cross-category ─────────────────────────────────────
        // Selection is independent of view, search, folds, windows,
        // notifications, internals.
        (C::Selection, C::ViewState) | (C::ViewState, C::Selection) => R::Commutative,
        (C::Selection, C::SearchState) | (C::SearchState, C::Selection) => R::Commutative,
        (C::Selection, C::FoldOp) | (C::FoldOp, C::Selection) => R::Commutative,
        (C::Selection, C::WindowOp) | (C::WindowOp, C::Selection) => R::Commutative,
        (C::Selection, C::Notification) | (C::Notification, C::Selection) => R::Commutative,
        (C::Selection, C::Internal) | (C::Internal, C::Selection) => R::Commutative,

        // ── ViewState cross-category ─────────────────────────────────────
        // View state is independent of search, folds, windows,
        // notifications, internals.
        (C::ViewState, C::SearchState) | (C::SearchState, C::ViewState) => R::Commutative,
        (C::ViewState, C::FoldOp) | (C::FoldOp, C::ViewState) => R::Commutative,
        (C::ViewState, C::WindowOp) | (C::WindowOp, C::ViewState) => R::Commutative,
        (C::ViewState, C::Notification) | (C::Notification, C::ViewState) => R::Commutative,
        (C::ViewState, C::Internal) | (C::Internal, C::ViewState) => R::Commutative,

        // ── SearchState cross-category ───────────────────────────────────
        // Search is independent of folds, windows, notifications, internals.
        (C::SearchState, C::FoldOp) | (C::FoldOp, C::SearchState) => R::Commutative,
        (C::SearchState, C::WindowOp) | (C::WindowOp, C::SearchState) => R::Commutative,
        (C::SearchState, C::Notification) | (C::Notification, C::SearchState) => R::Commutative,
        (C::SearchState, C::Internal) | (C::Internal, C::SearchState) => R::Commutative,

        // ── FoldOp cross-category ────────────────────────────────────────
        // Folds are independent of windows, notifications, internals.
        (C::FoldOp, C::WindowOp) | (C::WindowOp, C::FoldOp) => R::Commutative,
        (C::FoldOp, C::Notification) | (C::Notification, C::FoldOp) => R::Commutative,
        (C::FoldOp, C::Internal) | (C::Internal, C::FoldOp) => R::Commutative,

        // ── WindowOp cross-category ──────────────────────────────────────
        // Windows are independent of notifications and internals.
        (C::WindowOp, C::Notification) | (C::Notification, C::WindowOp) => R::Commutative,
        (C::WindowOp, C::Internal) | (C::Internal, C::WindowOp) => R::Commutative,

        // ── Notification cross-category ──────────────────────────────────
        // Notifications are independent of internals.
        (C::Notification, C::Internal) | (C::Internal, C::Notification) => R::Commutative,
    }
}

/// Convenience: classify commutativity for two concrete [`EffectKind`] values.
///
/// Equivalent to `classify_commutativity(categorize(a), categorize(b))`.
#[inline]
#[must_use]
pub const fn classify_effect_pair(a: EffectKind, b: EffectKind) -> Commutativity {
    classify_commutativity(categorize(a), categorize(b))
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── Categorization coverage ──────────────────────────────────────────

    #[test]
    fn every_effect_kind_is_categorized() {
        // Exercises the exhaustive match -- a missing variant would fail compilation.
        for kind in EffectKind::ALL {
            let _cat = categorize(kind);
        }
    }

    #[test]
    fn all_kinds_have_a_category() {
        let count = EffectKind::ALL.len();
        assert_eq!(count, 130, "Expected 130 EffectKind variants, got {count}");
        for kind in EffectKind::ALL {
            let cat = categorize(kind);
            assert!(
                EffectCategory::ALL.contains(&cat),
                "{kind:?} maps to {cat:?} which is not in EffectCategory::ALL"
            );
        }
    }

    #[test]
    fn text_mutation_category_matches_is_text_mutation() {
        // Every kind where is_text_mutation() is true should be TextMutation category,
        // and vice versa.
        for kind in EffectKind::ALL {
            let is_tm = kind.is_text_mutation();
            let is_cat_tm = categorize(kind) == EffectCategory::TextMutation;
            assert_eq!(
                is_tm, is_cat_tm,
                "{kind:?}: is_text_mutation()={is_tm} but category TextMutation={is_cat_tm}"
            );
        }
    }

    // ── Commutativity table symmetry ─────────────────────────────────────

    #[test]
    fn commutativity_table_is_symmetric() {
        // classify_commutativity(a, b) == classify_commutativity(b, a) for all pairs.
        for &a in &EffectCategory::ALL {
            for &b in &EffectCategory::ALL {
                let ab = classify_commutativity(a, b);
                let ba = classify_commutativity(b, a);
                assert_eq!(
                    ab, ba,
                    "Commutativity table is asymmetric: ({a:?}, {b:?}) = {ab:?} but ({b:?}, {a:?}) = {ba:?}"
                );
            }
        }
    }

    #[test]
    fn commutativity_table_covers_all_pairs() {
        // Every pair of categories must produce a valid Commutativity value.
        // (This would fail at compile time due to exhaustive match, but this
        // exercises the runtime path for completeness.)
        let mut count = 0;
        for &a in &EffectCategory::ALL {
            for &b in &EffectCategory::ALL {
                let _c = classify_commutativity(a, b);
                count += 1;
            }
        }
        // 13 categories * 13 categories = 169 pairs
        assert_eq!(count, 169, "Expected 169 pairs, got {count}");
    }

    // ── Specific commutativity classifications ───────────────────────────

    #[test]
    fn text_mutation_same_is_non_commutative() {
        assert_eq!(
            classify_commutativity(EffectCategory::TextMutation, EffectCategory::TextMutation),
            Commutativity::NonCommutative,
        );
    }

    #[test]
    fn cursor_move_same_is_last_wins() {
        assert_eq!(
            classify_commutativity(EffectCategory::CursorMove, EffectCategory::CursorMove),
            Commutativity::LastWins,
        );
    }

    #[test]
    fn mode_change_same_is_last_wins() {
        assert_eq!(
            classify_commutativity(EffectCategory::ModeChange, EffectCategory::ModeChange),
            Commutativity::LastWins,
        );
    }

    #[test]
    fn selection_same_is_last_wins() {
        assert_eq!(
            classify_commutativity(EffectCategory::Selection, EffectCategory::Selection),
            Commutativity::LastWins,
        );
    }

    #[test]
    fn notification_same_is_last_wins() {
        assert_eq!(
            classify_commutativity(EffectCategory::Notification, EffectCategory::Notification),
            Commutativity::LastWins,
        );
    }

    #[test]
    fn text_mutation_and_cursor_is_non_commutative() {
        // Insert shifts offsets, making cursor position depend on order.
        assert_eq!(
            classify_commutativity(EffectCategory::TextMutation, EffectCategory::CursorMove),
            Commutativity::NonCommutative,
        );
    }

    #[test]
    fn text_mutation_and_mode_is_commutative() {
        // Inserting text and changing mode affect independent state.
        assert_eq!(
            classify_commutativity(EffectCategory::TextMutation, EffectCategory::ModeChange),
            Commutativity::Commutative,
        );
    }

    #[test]
    fn cursor_and_mode_is_commutative() {
        assert_eq!(
            classify_commutativity(EffectCategory::CursorMove, EffectCategory::ModeChange),
            Commutativity::Commutative,
        );
    }

    #[test]
    fn text_mutation_and_mark_is_non_commutative() {
        // Text mutations shift mark offsets.
        assert_eq!(
            classify_commutativity(EffectCategory::TextMutation, EffectCategory::MarkOp),
            Commutativity::NonCommutative,
        );
    }

    #[test]
    fn text_mutation_and_undo_bracket_is_non_commutative() {
        // Undo brackets must wrap their text mutations.
        assert_eq!(
            classify_commutativity(EffectCategory::TextMutation, EffectCategory::UndoBracket),
            Commutativity::NonCommutative,
        );
    }

    #[test]
    fn text_mutation_and_selection_is_non_commutative() {
        // Text mutations shift selection offsets.
        assert_eq!(
            classify_commutativity(EffectCategory::TextMutation, EffectCategory::Selection),
            Commutativity::NonCommutative,
        );
    }

    #[test]
    fn text_mutation_and_register_is_commutative() {
        // Register contents are independent of document state.
        assert_eq!(
            classify_commutativity(EffectCategory::TextMutation, EffectCategory::RegisterOp),
            Commutativity::Commutative,
        );
    }

    #[test]
    fn text_mutation_and_notification_is_commutative() {
        assert_eq!(
            classify_commutativity(EffectCategory::TextMutation, EffectCategory::Notification),
            Commutativity::Commutative,
        );
    }

    #[test]
    fn register_and_mark_is_commutative() {
        assert_eq!(
            classify_commutativity(EffectCategory::RegisterOp, EffectCategory::MarkOp),
            Commutativity::Commutative,
        );
    }

    #[test]
    fn fold_and_window_is_commutative() {
        assert_eq!(
            classify_commutativity(EffectCategory::FoldOp, EffectCategory::WindowOp),
            Commutativity::Commutative,
        );
    }

    #[test]
    fn undo_bracket_same_is_non_commutative() {
        // Begin/End brackets are ordered delimiters.
        assert_eq!(
            classify_commutativity(EffectCategory::UndoBracket, EffectCategory::UndoBracket),
            Commutativity::NonCommutative,
        );
    }

    // ── classify_effect_pair convenience ──────────────────────────────────

    #[test]
    fn classify_effect_pair_insert_and_delete_is_non_commutative() {
        assert_eq!(
            classify_effect_pair(EffectKind::Insert, EffectKind::Delete),
            Commutativity::NonCommutative,
        );
    }

    #[test]
    fn classify_effect_pair_set_cursor_and_set_mode_is_commutative() {
        assert_eq!(
            classify_effect_pair(EffectKind::SetCursor, EffectKind::SetMode),
            Commutativity::Commutative,
        );
    }

    #[test]
    fn classify_effect_pair_two_set_cursors_is_last_wins() {
        assert_eq!(
            classify_effect_pair(EffectKind::SetCursor, EffectKind::SetCursor),
            Commutativity::LastWins,
        );
    }

    #[test]
    fn classify_effect_pair_insert_and_set_cursor_is_non_commutative() {
        assert_eq!(
            classify_effect_pair(EffectKind::Insert, EffectKind::SetCursor),
            Commutativity::NonCommutative,
        );
    }

    #[test]
    fn classify_effect_pair_noop_and_noop_is_non_commutative() {
        // Both are Internal category.
        assert_eq!(
            classify_effect_pair(EffectKind::Noop, EffectKind::Noop),
            Commutativity::NonCommutative,
        );
    }

    #[test]
    fn classify_effect_pair_show_info_and_show_error_is_last_wins() {
        // Both are Notification category.
        assert_eq!(
            classify_effect_pair(EffectKind::ShowInfo, EffectKind::ShowError),
            Commutativity::LastWins,
        );
    }

    // ── Diagonal: every category with itself ─────────────────────────────

    #[test]
    fn diagonal_classifications_are_correct() {
        use Commutativity as R;
        use EffectCategory as C;

        let expected_diagonal = [
            (C::TextMutation, R::NonCommutative),
            (C::CursorMove, R::LastWins),
            (C::ModeChange, R::LastWins),
            (C::RegisterOp, R::NonCommutative),
            (C::MarkOp, R::NonCommutative),
            (C::UndoBracket, R::NonCommutative),
            (C::Selection, R::LastWins),
            (C::ViewState, R::NonCommutative),
            (C::SearchState, R::NonCommutative),
            (C::FoldOp, R::NonCommutative),
            (C::WindowOp, R::NonCommutative),
            (C::Notification, R::LastWins),
            (C::Internal, R::NonCommutative),
        ];

        for (cat, expected) in &expected_diagonal {
            let actual = classify_commutativity(*cat, *cat);
            assert_eq!(
                actual, *expected,
                "Diagonal for {cat:?}: expected {expected:?}, got {actual:?}"
            );
        }
    }

    // ── All cross-category pairs are commutative or non-commutative ──────
    // (This test verifies the exhaustive off-diagonal coverage.)

    #[test]
    fn off_diagonal_pairs_produce_valid_classification() {
        for &a in &EffectCategory::ALL {
            for &b in &EffectCategory::ALL {
                if a != b {
                    let c = classify_commutativity(a, b);
                    // Off-diagonal should be Commutative or NonCommutative (never LastWins).
                    assert!(
                        matches!(c, Commutativity::Commutative | Commutativity::NonCommutative),
                        "Off-diagonal ({a:?}, {b:?}) = {c:?} but expected Commutative or NonCommutative"
                    );
                }
            }
        }
    }

    // ── Category population tests ────────────────────────────────────────

    #[test]
    fn text_mutation_has_7_members() {
        // 6 direct mutations (Insert/Delete/Replace/Undo/UndoLine/Redo) +
        // OperatorToMark (host-executed text mutation).
        let count = EffectKind::ALL
            .iter()
            .filter(|k| categorize(**k) == EffectCategory::TextMutation)
            .count();
        assert_eq!(count, 7, "TextMutation should have 7 members, got {count}");
    }

    #[test]
    fn cursor_move_has_5_members() {
        let count = EffectKind::ALL
            .iter()
            .filter(|k| categorize(**k) == EffectCategory::CursorMove)
            .count();
        assert_eq!(count, 5, "CursorMove should have 5 members, got {count}");
    }

    #[test]
    fn mode_change_has_4_members() {
        let count = EffectKind::ALL
            .iter()
            .filter(|k| categorize(**k) == EffectCategory::ModeChange)
            .count();
        assert_eq!(count, 4, "ModeChange should have 4 members, got {count}");
    }

    #[test]
    fn category_population_sums_to_all() {
        let total: usize = EffectCategory::ALL
            .iter()
            .map(|cat| {
                EffectKind::ALL
                    .iter()
                    .filter(|k| categorize(**k) == *cat)
                    .count()
            })
            .sum();
        assert_eq!(
            total,
            EffectKind::ALL.len(),
            "Category populations should sum to {}, got {total}",
            EffectKind::ALL.len()
        );
    }

    // ── Every EffectKind pair via classify_effect_pair ────────────────────

    #[test]
    fn classify_effect_pair_covers_all_squared_pairs() {
        let mut count = 0;
        for &a in &EffectKind::ALL {
            for &b in &EffectKind::ALL {
                let _c = classify_effect_pair(a, b);
                count += 1;
            }
        }
        let expected = EffectKind::ALL.len() * EffectKind::ALL.len();
        assert_eq!(count, expected, "Should cover all {expected} pairs");
    }
}
