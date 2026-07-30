use std::fmt::Debug;

/// Monoidal aggregate cached at each tree node.
/// Laws:
///   - compose is associative: compose(compose(a,b),c) == compose(a,compose(b,c))
///   - Default is identity: compose(default(), x) == x, compose(x, default()) == x
pub trait Summary: Clone + Debug + Default {
    fn compose(&mut self, other: &Self);

    /// Returns the base-unit length represented by this summary.
    /// This is the "natural offset dimension" used by insert/delete operations.
    /// For text, this is bytes. For sequences, this is item count.
    fn base_len(&self) -> usize;
}

/// Extension for summaries where subtraction is exact.
/// Law: if c = compose(a, b), then subtract(c, b) == a.
///
/// Nothing in the tree is generic over this bound — the law is asserted by
/// tests against the concrete summaries, so the trait is test-only.
#[cfg(test)]
pub trait InvertibleSummary: Summary {
    fn subtract(&mut self, other: &Self);
}

/// Leaf content stored in the tree.
pub trait Item: Clone {
    type Summary: Summary;

    /// Minimum content length before a leaf is "undersized" (triggers merge).
    const MIN_LEN: usize;

    /// Maximum content length before a leaf splits.
    const MAX_LEN: usize;

    /// Compute the summary for this item's content.
    fn summary(&self) -> Self::Summary;

    /// Length in base units (bytes for text, count for sequences).
    fn len(&self) -> usize;

    /// Split at offset, returning the right portion. Self becomes the left.
    fn split_at(&mut self, offset: usize) -> Self;

    /// Attempt to merge other into self. Returns true if successful.
    fn try_merge(&mut self, other: &Self) -> bool;
}

/// A monotonic projection from Summary enabling O(log n) seeking.
/// Must be Ord to enable binary search on prefix sums within internal nodes.
pub trait Dimension<S: Summary>: Copy + Debug + Default + Ord {
    fn from_summary(summary: &S) -> Self;
    fn add_summary(&mut self, summary: &S);
}

/// Bias for cursor seek: when the target falls on a boundary between
/// two items, Left returns the end of the left item, Right returns the start of the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    Left,
    Right,
}
