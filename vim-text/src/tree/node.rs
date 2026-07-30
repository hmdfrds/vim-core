use std::sync::Arc;

use arrayvec::ArrayVec;

use super::traits::{Item, Summary};

/// Maximum tree height. With B=8, height 16 supports 8^16 = 2^48 leaves.
pub(crate) const MAX_HEIGHT: usize = 16;

/// Default branching factor. 8 children fits in 6 cache lines per internal node.
pub(crate) const DEFAULT_B: usize = 8;

/// Maximum capacity for `InternalNode`'s ArrayVec-backed children/summaries.
/// Set to `DEFAULT_B + 2` to accommodate the temporary overflow during
/// `SplitTwo` (up to B+2 children) before the split propagates.
pub(crate) const INTERNAL_CAP: usize = DEFAULT_B + 2;

/// A node in the persistent B+ tree. Either a leaf holding content
/// or an internal node holding child pointers with Arc-based COW.
#[derive(Clone)]
pub(crate) enum Node<T: Item, const B: usize = DEFAULT_B> {
    Leaf { item: T, summary: T::Summary },
    Internal(InternalNode<T, B>),
}

/// Internal node: stores child pointers (Arc for COW sharing), cached
/// per-child summaries, a cached aggregate summary, and height.
///
/// The `summary` field caches the fold of all per-child summaries, making
/// `Node::summary()` O(1) instead of O(B). It is recomputed by
/// `recompute_aggregate()` after any mutation that changes children.
///
/// Uses `ArrayVec<_, INTERNAL_CAP>` to eliminate heap allocations. Capacity
/// is `DEFAULT_B + 2 = 10`, which accommodates temporary overflow during
/// insertions (SplitTwo can produce up to B+2 children before the split
/// propagates). After any public operation completes, `children.len() <= B`.
///
/// For tests using smaller `B` (e.g. `B = 4`), the extra capacity is unused
/// stack space -- a few dozen bytes, acceptable for test simplicity.
#[derive(Clone)]
pub(crate) struct InternalNode<T: Item, const B: usize = DEFAULT_B> {
    pub children: ArrayVec<Arc<Node<T, B>>, INTERNAL_CAP>,
    pub summaries: ArrayVec<T::Summary, INTERNAL_CAP>,
    pub summary: T::Summary,
    pub height: u8,
}

// --- Node methods ---

impl<T: Item, const B: usize> Node<T, B> {
    /// Construct a leaf node, computing its summary from the item.
    pub fn leaf(item: T) -> Self {
        let summary = item.summary();
        Node::Leaf { item, summary }
    }

    /// Returns the summary for this node.
    /// For leaves, returns the cached leaf summary.
    /// For internals, returns the cached aggregate summary (O(1)).
    pub fn summary(&self) -> T::Summary {
        match self {
            Node::Leaf { summary, .. } => summary.clone(),
            Node::Internal(internal) => internal.summary.clone(),
        }
    }

    /// Returns the height of this node (0 for leaves, stored value for internals).
    pub fn height(&self) -> u8 {
        match self {
            Node::Leaf { .. } => 0,
            Node::Internal(internal) => internal.height,
        }
    }

    /// Returns true if this is a leaf node.
    ///
    /// Production code always matches on `Node` directly; the predicate exists
    /// for the tree's structural assertions, so it is test-only.
    #[cfg(test)]
    pub fn is_leaf(&self) -> bool {
        matches!(self, Node::Leaf { .. })
    }

    /// Returns true if this node is undersized and should trigger a merge.
    /// Leaf: item.len() < MIN_LEN. Internal: children.len() < B/2.
    pub fn is_undersized(&self) -> bool {
        match self {
            Node::Leaf { item, .. } => item.len() < T::MIN_LEN,
            Node::Internal(internal) => internal.children.len() < B / 2,
        }
    }

    /// Returns a reference to the item if this is a leaf, None for internals.
    ///
    /// Production code destructures `Node::Leaf` in place; this accessor is
    /// used by the tree's structural assertions, so it is test-only.
    #[cfg(test)]
    pub fn item(&self) -> Option<&T> {
        match self {
            Node::Leaf { item, .. } => Some(item),
            Node::Internal(_) => None,
        }
    }
}

// --- InternalNode methods ---

impl<T: Item, const B: usize> InternalNode<T, B> {
    /// Create an empty internal node at the given height.
    pub fn new(height: u8) -> Self {
        InternalNode {
            children: ArrayVec::new(),
            summaries: ArrayVec::new(),
            summary: T::Summary::default(),
            height,
        }
    }

    /// Construct an internal node from an iterator of children at the given height.
    /// Computes per-child summaries from the children.
    ///
    /// # Panics
    /// Panics if `children` is empty or exceeds capacity B.
    pub fn from_children(children: impl IntoIterator<Item = Arc<Node<T, B>>>, height: u8) -> Self {
        let mut node = Self::new(height);
        for child in children {
            let child_summary = child.summary();
            node.summary.compose(&child_summary);
            node.summaries.push(child_summary);
            node.children.push(child);
        }

        assert!(
            !node.children.is_empty(),
            "InternalNode requires at least one child"
        );
        assert!(
            node.children.len() <= B,
            "InternalNode exceeds branching factor B={}",
            B
        );

        node
    }

    /// Returns the cached aggregate summary. O(1).
    ///
    /// Production code reaches the aggregate through `Node::summary`, which
    /// reads the same cached field; only the rebalance tests call it directly.
    #[cfg(test)]
    pub fn summary(&self) -> T::Summary {
        self.summary.clone()
    }

    /// Append a child and its summary, updating the cached aggregate.
    pub fn push_child(&mut self, child: Arc<Node<T, B>>) {
        let child_summary = child.summary();
        self.summary.compose(&child_summary);
        self.children.push(child);
        self.summaries.push(child_summary);
    }

    /// Recompute the cached per-child summary for the child at the given index,
    /// then recompute the aggregate summary.
    pub fn update_summary(&mut self, index: usize) {
        self.summaries[index] = self.children[index].summary();
        self.recompute_aggregate();
    }

    /// Recompute `self.summary` by folding compose over all per-child summaries.
    /// Called after any mutation that changes children or per-child summaries.
    ///
    /// In debug builds, asserts that children and summaries ArrayVecs have the same length.
    pub fn recompute_aggregate(&mut self) {
        debug_assert_eq!(
            self.children.len(),
            self.summaries.len(),
            "children/summaries length mismatch"
        );

        let mut agg = T::Summary::default();
        for s in &self.summaries {
            agg.compose(s);
        }
        self.summary = agg;
    }
}
