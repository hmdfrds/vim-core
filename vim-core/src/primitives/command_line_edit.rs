//! Command-line edit instruction type.
//!
//! A pure description of edits to the command-line state. This type
//! lives at the `primitives` layer because it has zero internal
//! dependencies and is consumed by state, mode, effects, and execution.

/// A pure description of an edit to the command-line state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CommandLineEdit {
    /// Insert a character at the cursor.
    InsertChar(char),
    /// Delete character before cursor (backspace).
    Backspace,
    /// Delete character at cursor.
    Delete,
    /// Delete word before cursor (Ctrl-W).
    DeleteWord,
    /// Delete from cursor to start of line (Ctrl-U).
    DeleteToStart,
    /// Delete from cursor to end of line (Ctrl-K).
    DeleteToEnd,
    /// Move cursor left.
    MoveLeft,
    /// Move cursor right.
    MoveRight,
    /// Move cursor to start.
    MoveToStart,
    /// Move cursor to end.
    MoveToEnd,
    /// Navigate to previous history entry.
    HistoryPrev,
    /// Navigate to next history entry.
    HistoryNext,
    /// Trigger next completion match (Tab).
    CompleteNext,
    /// Trigger previous completion match (Shift-Tab).
    CompletePrev,
    /// Move cursor one word left (Ctrl-Left, Shift-Left).
    MoveWordLeft,
    /// Move cursor one word right (Ctrl-Right, Shift-Right).
    MoveWordRight,
    /// List all matching completions (Ctrl-D).
    ListCompletions,
}
