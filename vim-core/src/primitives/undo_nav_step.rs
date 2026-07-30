//! Undo navigation step type for undo/redo traversal paths.

use smallvec::SmallVec;

use super::{NodeId, Offset};

/// A single step in an undo/redo navigation path.
///
/// The engine produces these by walking its undo tree.
/// Hosts use the `node_id` to look up stored operations.
///
/// For **undo**: `node_id` is the node being LEFT (current before
/// `undo()` moves to parent). The host inverts this node's edits.
///
/// For **redo**: `node_id` is the node being ENTERED (child that
/// `redo()` moves into). The host re-applies this node's edits.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UndoNavStep {
    /// Which undo node's edit operations to apply/invert.
    pub node_id: NodeId,
    /// Cursor position(s) after this step completes (engine-computed).
    ///
    /// Single-element for normal undo; multiple elements when the undo group
    /// was created with multiple cursors via `begin_group_multi`.
    pub cursors: SmallVec<[Offset; 1]>,
}

impl UndoNavStep {
    /// Backward-compatible accessor for the primary cursor position.
    ///
    /// Returns the first (primary) cursor. For multi-cursor access,
    /// read the `cursors` field directly.
    #[inline]
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "cursors is always non-empty — constructed from UndoStep::cursors() which is guaranteed non-empty"
    )]
    pub fn cursor(&self) -> Offset {
        self.cursors[0]
    }
}
