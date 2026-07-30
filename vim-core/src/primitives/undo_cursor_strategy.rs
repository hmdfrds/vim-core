//! Undo cursor strategy — controls where the cursor lands after undo.
//!
//! Replaces the `force_entry_cursor: bool` field on `BeginUndoGroup`.

/// Strategy for cursor placement after undoing an edit group.
///
/// Determines whether undo restores the cursor to the position of the
/// first text edit in the group, or to the pre-command entry position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum UndoCursorStrategy {
    /// Restore cursor to the position of the first text edit in the group.
    ///
    /// This is the default Vim behaviour: undo places the cursor at the
    /// byte offset of the earliest mutation within the undo group.
    FirstEdit,
    /// Restore cursor to the pre-command entry position.
    ///
    /// Used by commands like `o`/`O` and `p`/`P` where the structural
    /// newline insert or paste position shouldn't determine the undo cursor.
    EntryPosition,
}
