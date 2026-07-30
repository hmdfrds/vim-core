//! Undo tree snapshot types for visualization.
//!
//! These types carry the complete tree structure so a host can render an
//! interactive undo tree browser. They live in `primitives/` so that both
//! `effects/` and `state/` can reference them without creating a cycle.
//!
//! [`NodeId`], [`UndoTreeNodeView`], and [`UndoTreeSnapshot`] are the public
//! surface used by `Effect::UndoTreeSnapshot`. The undo tree engine
//! (`state::undo_tree`) re-exports these and uses `NodeId` for its arena.

use super::Offset;

/// Unique node identifier within an [`UndoTree`](crate::state::UndoTree).
///
/// The root node always has ID 0 ([`NodeId::ROOT`]). IDs are monotonically
/// increasing and correspond to arena indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeId(u32);

impl NodeId {
    /// The root node (initial document state before any edits).
    pub const ROOT: Self = Self(0);

    /// Create from a raw index.
    #[inline]
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Raw index value.
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl core::fmt::Display for NodeId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A view of a single node in the undo tree, for visualization.
///
/// Contains the metadata needed by a host to render an interactive undo tree
/// browser. Produced by [`UndoTree::snapshot()`](crate::state::UndoTree::snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UndoTreeNodeView {
    /// Node identifier.
    pub id: NodeId,
    /// Parent node (None for root).
    pub parent: Option<NodeId>,
    /// Child node identifiers.
    pub children: Vec<NodeId>,
    /// Monotonic sequence number.
    pub sequence: u64,
    /// Timestamp when committed.
    pub timestamp: u64,
    /// Cursor position before this change group.
    pub cursor_before: Offset,
    /// Whether this is the current position in the tree.
    pub is_current: bool,
}

/// Complete snapshot of the undo tree for visualization.
///
/// Carries the full tree structure so the host can render a visual undo tree
/// browser. Produced by [`UndoTree::snapshot()`](crate::state::UndoTree::snapshot),
/// emitted via [`Effect::UndoTreeSnapshot`](crate::effects::Effect::UndoTreeSnapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UndoTreeSnapshot {
    /// All nodes in the tree (index 0 = root).
    pub nodes: Vec<UndoTreeNodeView>,
    /// The currently active node.
    pub current: NodeId,
    /// Total number of change groups (excludes root).
    pub change_count: u32,
}
