use std::ops::Range;
use std::sync::Arc;

use super::builder::build_tree;
use super::cursor::Cursor;
use super::node::{InternalNode, Node, DEFAULT_B};
use super::rebalance::{rebalance_child, split_internal, InsertResult};
use super::traits::{Dimension, Item, Summary};

/// Persistent B+ tree with monoidal summaries and Arc-based COW.
/// Clone is O(1) (Arc refcount bump).
#[derive(Clone)]
pub(crate) struct SumTree<T: Item, const B: usize = DEFAULT_B> {
    root: Arc<Node<T, B>>,
}

impl<T: Item, const B: usize> SumTree<T, B> {
    /// Create a tree containing a single leaf.
    pub fn from_item(item: T) -> Self {
        SumTree {
            root: Arc::new(Node::leaf(item)),
        }
    }

    /// Create a tree from multiple items using the O(n) bottom-up builder.
    ///
    /// # Panics
    /// Panics if `items` is empty.
    pub fn from_items(items: Vec<T>) -> Self {
        SumTree {
            root: build_tree::<T, B>(items),
        }
    }

    /// Borrow the root node.
    pub fn root(&self) -> &Arc<Node<T, B>> {
        &self.root
    }

    /// Returns the aggregate summary of the entire tree.
    pub fn summary(&self) -> T::Summary {
        self.root.summary()
    }

    /// Returns the total length measured in the given dimension.
    pub fn len<D: Dimension<T::Summary>>(&self) -> D {
        D::from_summary(&self.summary())
    }

    /// Returns true if the tree contains no content (single empty leaf).
    ///
    /// `VimText` answers emptiness from its own summary, so this is only
    /// reached by the tree's own tests.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        match self.root.as_ref() {
            Node::Leaf { item, .. } => item.len() == 0,
            Node::Internal(_) => false,
        }
    }

    /// O(1) snapshot via Arc clone.
    ///
    /// Callers outside the tree clone `VimText` itself, which is the same Arc
    /// bump; the named alias survives for the persistence tests.
    #[cfg(test)]
    pub fn snapshot(&self) -> Self {
        self.clone()
    }

    /// Create a cursor for traversal/seeking in the given dimension.
    pub fn cursor<D: Dimension<T::Summary>>(&self) -> Cursor<'_, T, D, B> {
        Cursor::new(&self.root)
    }

    /// Insert `new_item` at `offset` (measured in base units). O(log n).
    ///
    /// After insertion, the content at offsets `[0..offset)` is unchanged,
    /// `new_item` occupies `[offset..offset+new_item.len())`, and the former
    /// content at `[offset..)` follows.
    pub fn insert(&mut self, offset: usize, new_item: T) {
        let result = insert_recursive(Arc::make_mut(&mut self.root), offset, new_item);

        match result {
            InsertResult::Absorbed => {}
            InsertResult::Split(new_sibling) => {
                self.grow_root_with_siblings(vec![new_sibling]);
            }
            InsertResult::SplitTwo(sib1, sib2) => {
                self.grow_root_with_siblings(vec![sib1, sib2]);
            }
        }
    }

    /// Create a new internal root containing the current root (as left child)
    /// plus the given siblings (as right children), increasing tree height by 1.
    fn grow_root_with_siblings(&mut self, siblings: Vec<Arc<Node<T, B>>>) {
        let new_height = self.root.height() + 1;
        // Take the current root Arc. We replace self.root temporarily; it will
        // be overwritten at the end of this function.
        let old_root = Arc::clone(&self.root);
        let mut new_internal = InternalNode::new(new_height);
        new_internal.push_child(old_root);
        for sib in siblings {
            new_internal.push_child(sib);
        }
        self.root = Arc::new(Node::Internal(new_internal));
    }

    /// Delete the content in `range` (measured in base units). O(log n).
    ///
    /// After deletion, content at `[0..range.start)` and `[range.end..)` are
    /// concatenated.
    pub fn delete(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        // Salvage a canonical empty item from the leftmost leaf BEFORE the
        // destructive delete, in case it drains the whole document. Once
        // `delete_recursive` removes every child of an Internal root, the root
        // carries no leaf to empty, and `Item` has no zero-arg constructor — so
        // the leftmost leaf is the only generic source of a value of `T` we can
        // empty via `split_at(0)`. O(log n) (leftmost spine), negligible vs the
        // delete itself.
        let empty_item = leftmost_empty_item::<T, B>(&self.root);

        let result = delete_recursive(Arc::make_mut(&mut self.root), range.start, range.end);
        match result {
            DeleteResult::Empty => {
                // Entire tree content deleted (e.g. `:%s` replacing every line, or
                // `ggVGd`). Collapse to a canonical empty leaf regardless of whether
                // the drained root was a Leaf or an Internal node — an empty buffer
                // is a valid state. (Previously the Internal case hit `unreachable!`.)
                self.root = Arc::new(Node::leaf(empty_item));
            }
            DeleteResult::Ok => {
                self.collapse_root();
            }
        }
    }

    /// Replace content in `range` with `new_item`. O(log n).
    ///
    /// Equivalent to `delete(range); insert(range.start, new_item)`.
    ///
    /// Edits arrive as explicit delete/insert pairs from `ChangeSet`, so the
    /// combined form is exercised only by the tree's own tests.
    #[cfg(test)]
    pub fn replace(&mut self, range: Range<usize>, new_item: T) {
        self.delete(range.clone());
        self.insert(range.start, new_item);
    }

    /// If the root is an internal node with a single child, replace the root
    /// with that child. Repeats until the root is stable.
    fn collapse_root(&mut self) {
        loop {
            let should_collapse = match self.root.as_ref() {
                Node::Internal(internal) => internal.children.len() == 1,
                Node::Leaf { .. } => false,
            };
            if !should_collapse {
                break;
            }
            let child = match Arc::make_mut(&mut self.root) {
                Node::Internal(internal) => internal.children.remove(0),
                _ => unreachable!(),
            };
            self.root = child;
        }
    }
}

/// Return an EMPTY clone of the leftmost leaf's item.
///
/// Every tree holds at least one leaf, so descending the leftmost spine always
/// reaches a `Leaf`. We clone its item and empty it via `split_at(0)` (which
/// leaves `self` empty and returns the original), yielding a zero-length `T`
/// without requiring an `Item::empty()` constructor the trait does not provide.
/// Used by `delete()` to install a canonical empty leaf when a whole-document
/// delete drains the root.
fn leftmost_empty_item<T: Item, const B: usize>(node: &Node<T, B>) -> T {
    let mut cur = node;
    loop {
        match cur {
            Node::Leaf { item, .. } => {
                let mut empty = item.clone();
                let _original = empty.split_at(0);
                return empty;
            }
            Node::Internal(internal) => {
                cur = internal.children[0].as_ref();
            }
        }
    }
}

// --- Insert ---

/// Recursively insert `new_item` at `offset` within `node`.
/// Returns `InsertResult::Absorbed` if the node absorbed it, or
/// `InsertResult::Split(sibling)` if the node split.
fn insert_recursive<T: Item, const B: usize>(
    node: &mut Node<T, B>,
    offset: usize,
    new_item: T,
) -> InsertResult<T, B> {
    match node {
        Node::Leaf { item, summary } => {
            let item_len = item.len();

            if offset == 0 {
                // Insert before: try merge new_item + existing
                let mut merged = new_item.clone();
                if merged.try_merge(item) {
                    *item = merged;
                    *summary = item.summary();
                    return InsertResult::Absorbed;
                }
                // Can't merge: new_item becomes this leaf, old item becomes sibling
                let old_item = std::mem::replace(item, new_item);
                *summary = item.summary();
                InsertResult::Split(Arc::new(Node::leaf(old_item)))
            } else if offset >= item_len {
                // Insert after: try merge existing + new_item
                if item.try_merge(&new_item) {
                    *summary = item.summary();
                    return InsertResult::Absorbed;
                }
                // Can't merge: return new_item as sibling
                InsertResult::Split(Arc::new(Node::leaf(new_item)))
            } else {
                // Insert in middle: split leaf at offset, then fit pieces together.
                let right_part = item.split_at(offset);
                // Now: item = [0..offset], right_part = [offset..item_len]
                // Goal: this_leaf=[0..offset]+new_item (or part), sibling=remainder+right_part

                // Try merge item + new_item
                if item.try_merge(&new_item) {
                    // item now = [0..offset] + new_item
                    // Try merge with right_part
                    if item.try_merge(&right_part) {
                        // All fits in one leaf
                        *summary = item.summary();
                        InsertResult::Absorbed
                    } else {
                        // Two pieces: item (left+new) stays, right_part is sibling
                        *summary = item.summary();
                        InsertResult::Split(Arc::new(Node::leaf(right_part)))
                    }
                } else {
                    // item stays as [0..offset]. new_item didn't merge with left.
                    // Try merge new_item + right_part into a single sibling.
                    let mut new_plus_right = new_item;
                    if new_plus_right.try_merge(&right_part) {
                        // Sibling = new_item + right_part
                        *summary = item.summary();
                        InsertResult::Split(Arc::new(Node::leaf(new_plus_right)))
                    } else {
                        // Three pieces that can't merge pairwise:
                        //   item=[0..offset], new_plus_right=new_item, right_part=[offset..item_len]
                        // This happens when new_item is large enough that neither adjacent
                        // pair fits in MAX_LEN. Keep item as this leaf; return both
                        // new_item and right_part as siblings via SplitTwo.
                        *summary = item.summary();
                        InsertResult::SplitTwo(
                            Arc::new(Node::leaf(new_plus_right)),
                            Arc::new(Node::leaf(right_part)),
                        )
                    }
                }
            }
        }
        Node::Internal(internal) => {
            // Find the child containing the offset
            let (child_idx, local_offset) = find_child_for_offset::<T, B>(internal, offset);

            // Recurse into that child with COW
            let child = Arc::make_mut(&mut internal.children[child_idx]);
            let result = insert_recursive(child, local_offset, new_item);

            // Update summary for the modified child
            internal.update_summary(child_idx);

            match result {
                InsertResult::Absorbed => InsertResult::Absorbed,
                InsertResult::Split(new_sibling) => {
                    insert_sibling_at(internal, child_idx + 1, new_sibling)
                }
                InsertResult::SplitTwo(sibling1, sibling2) => {
                    // Insert both siblings after child_idx
                    let summary1 = sibling1.summary();
                    let summary2 = sibling2.summary();
                    internal.children.insert(child_idx + 1, sibling1);
                    internal.summaries.insert(child_idx + 1, summary1);
                    internal.children.insert(child_idx + 2, sibling2);
                    internal.summaries.insert(child_idx + 2, summary2);

                    if internal.children.len() > B {
                        // split_internal recomputes aggregate on both halves.
                        let right_half = split_internal(internal);
                        InsertResult::Split(Arc::new(Node::Internal(right_half)))
                    } else {
                        internal.recompute_aggregate();
                        InsertResult::Absorbed
                    }
                }
            }
        }
    }
}

/// Insert a new sibling node at `insert_pos` in the internal node.
/// If overfull (>B children), split the internal node.
fn insert_sibling_at<T: Item, const B: usize>(
    internal: &mut InternalNode<T, B>,
    insert_pos: usize,
    new_sibling: Arc<Node<T, B>>,
) -> InsertResult<T, B> {
    let sibling_summary = new_sibling.summary();
    internal.children.insert(insert_pos, new_sibling);
    internal.summaries.insert(insert_pos, sibling_summary);

    if internal.children.len() > B {
        // split_internal recomputes aggregate on both halves.
        let right_half = split_internal(internal);
        InsertResult::Split(Arc::new(Node::Internal(right_half)))
    } else {
        internal.recompute_aggregate();
        InsertResult::Absorbed
    }
}

/// Find which child contains the given offset, returning (child_index, local_offset).
/// If offset equals the total length, returns the last child with local_offset = that child's length.
fn find_child_for_offset<T: Item, const B: usize>(
    internal: &InternalNode<T, B>,
    offset: usize,
) -> (usize, usize) {
    let mut accumulated = 0usize;
    for (i, summary) in internal.summaries.iter().enumerate() {
        let child_len = summary.base_len();
        if offset < accumulated + child_len {
            return (i, offset - accumulated);
        }
        accumulated += child_len;
    }
    // Offset is at or past the end: target the last child
    let last = internal.children.len() - 1;
    let last_len = internal.summaries[last].base_len();
    (last, last_len)
}

// --- Delete ---

/// Result of a delete operation on a node.
enum DeleteResult {
    /// Node still has content (possibly undersized).
    Ok,
    /// Node is now completely empty and should be removed by parent.
    Empty,
}

/// Recursively delete content in `[start, end)` from `node`.
fn delete_recursive<T: Item, const B: usize>(
    node: &mut Node<T, B>,
    start: usize,
    end: usize,
) -> DeleteResult {
    match node {
        Node::Leaf { item, summary } => {
            let item_len = item.len();
            assert!(
                end <= item_len,
                "delete range end ({end}) exceeds leaf length ({item_len})"
            );

            if start == 0 && end >= item_len {
                // Entire leaf deleted
                return DeleteResult::Empty;
            }

            if start == 0 {
                // Delete prefix [0..end): keep [end..item_len)
                let right = item.split_at(end);
                *item = right;
            } else if end >= item_len {
                // Delete suffix [start..item_len): keep [0..start)
                let _right = item.split_at(start);
                // item is now [0..start), _right is discarded
            } else {
                // Delete middle [start..end): keep [0..start) + [end..item_len)
                let mut right = item.split_at(start);
                // item = [0..start), right = [start..item_len)
                // From right, discard [0..end-start), keep [end-start..)
                let right_suffix = right.split_at(end - start);
                // right = [start..end) (discarded), right_suffix = [end..item_len)
                let merged = item.try_merge(&right_suffix);
                assert!(
                    merged,
                    "Middle-delete merge must succeed: left.len()={} + right.len()={} \
                     = {} <= MAX_LEN={} (original item was valid)",
                    item.len(),
                    right_suffix.len(),
                    item.len() + right_suffix.len(),
                    T::MAX_LEN
                );
            }

            *summary = item.summary();

            if item.len() == 0 {
                DeleteResult::Empty
            } else {
                DeleteResult::Ok
            }
        }
        Node::Internal(internal) => {
            delete_from_internal(internal, start, end);
            if internal.children.is_empty() {
                DeleteResult::Empty
            } else {
                DeleteResult::Ok
            }
        }
    }
}

/// Delete range [start, end) from an internal node's children.
fn delete_from_internal<T: Item, const B: usize>(
    internal: &mut InternalNode<T, B>,
    start: usize,
    end: usize,
) {
    // Find first and last children that overlap with [start, end)
    let (first_idx, first_local_start) = find_child_for_delete_start::<T, B>(internal, start);
    let (last_idx, last_local_end) = find_child_for_delete_end::<T, B>(internal, end);

    if first_idx == last_idx {
        // Range is entirely within one child
        let child = Arc::make_mut(&mut internal.children[first_idx]);
        let result = delete_recursive(child, first_local_start, last_local_end);
        match result {
            DeleteResult::Empty => {
                internal.children.remove(first_idx);
                internal.summaries.remove(first_idx);
            }
            DeleteResult::Ok => {
                internal.summaries[first_idx] = internal.children[first_idx].summary();
                if internal.children[first_idx].is_undersized() && internal.children.len() > 1 {
                    rebalance_child(internal, first_idx);
                }
            }
        }
    } else {
        // Range spans multiple children. Process from right to left to maintain indices.

        // Phase 1: Truncate or remove the last child
        let last_child_len = internal.summaries[last_idx].base_len();
        if last_local_end >= last_child_len {
            // Entire last child is within delete range
            internal.children.remove(last_idx);
            internal.summaries.remove(last_idx);
        } else if last_local_end > 0 {
            // Partial delete from start of last child
            let child = Arc::make_mut(&mut internal.children[last_idx]);
            let result = delete_recursive(child, 0, last_local_end);
            match result {
                DeleteResult::Empty => {
                    internal.children.remove(last_idx);
                    internal.summaries.remove(last_idx);
                }
                DeleteResult::Ok => {
                    internal.summaries[last_idx] = internal.children[last_idx].summary();
                }
            }
        }

        // Phase 2: Remove all fully-contained middle children (between first and last)
        let middle_count = last_idx - first_idx - 1;
        for _ in 0..middle_count {
            // Always remove at first_idx+1 since previous removals shift everything
            internal.children.remove(first_idx + 1);
            internal.summaries.remove(first_idx + 1);
        }

        // Phase 3: Truncate or remove the first child
        let first_child_len = internal.summaries[first_idx].base_len();
        if first_local_start == 0 {
            // Entire first child is within delete range
            internal.children.remove(first_idx);
            internal.summaries.remove(first_idx);
        } else if first_local_start < first_child_len {
            // Partial delete from end of first child: delete [first_local_start..first_child_len)
            let child = Arc::make_mut(&mut internal.children[first_idx]);
            let result = delete_recursive(child, first_local_start, first_child_len);
            match result {
                DeleteResult::Empty => {
                    internal.children.remove(first_idx);
                    internal.summaries.remove(first_idx);
                }
                DeleteResult::Ok => {
                    internal.summaries[first_idx] = internal.children[first_idx].summary();
                }
            }
        }

        // Phase 4: Rebalance undersized children
        let mut i = internal.children.len();
        while i > 0 && internal.children.len() > 1 {
            i -= 1;
            if internal.children[i].is_undersized() {
                rebalance_child(internal, i);
                if i >= internal.children.len() {
                    i = internal.children.len();
                }
            }
        }
    }

    // Recompute the aggregate after all mutations (removes, rebalances, etc.).
    if !internal.children.is_empty() {
        internal.recompute_aggregate();
    }
}

/// Find the child index and local start offset for a delete start position.
fn find_child_for_delete_start<T: Item, const B: usize>(
    internal: &InternalNode<T, B>,
    start: usize,
) -> (usize, usize) {
    let mut accumulated = 0usize;
    for (i, summary) in internal.summaries.iter().enumerate() {
        let child_len = summary.base_len();
        if start < accumulated + child_len {
            return (i, start - accumulated);
        }
        accumulated += child_len;
    }
    let last = internal.children.len() - 1;
    let last_len = internal.summaries[last].base_len();
    (last, last_len)
}

/// Find the child index and local end offset for a delete end position.
/// End is exclusive, so if end falls exactly on a boundary, it belongs to the left child.
fn find_child_for_delete_end<T: Item, const B: usize>(
    internal: &InternalNode<T, B>,
    end: usize,
) -> (usize, usize) {
    let mut accumulated = 0usize;
    for (i, summary) in internal.summaries.iter().enumerate() {
        let child_len = summary.base_len();
        if end <= accumulated + child_len {
            return (i, end - accumulated);
        }
        accumulated += child_len;
    }
    let last = internal.children.len() - 1;
    let last_len = internal.summaries[last].base_len();
    (last, last_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Splittable test item: Chunk (Vec<u8> based) ---

    #[derive(Clone, Debug, PartialEq)]
    struct Chunk(Vec<u8>);

    #[derive(Clone, Debug, Default, PartialEq)]
    struct ChunkSummary {
        len: u32,
    }

    impl Summary for ChunkSummary {
        fn compose(&mut self, other: &Self) {
            self.len += other.len;
        }

        fn base_len(&self) -> usize {
            self.len as usize
        }
    }

    impl Item for Chunk {
        type Summary = ChunkSummary;
        const MIN_LEN: usize = 2;
        const MAX_LEN: usize = 8;

        fn summary(&self) -> ChunkSummary {
            ChunkSummary {
                len: self.0.len() as u32,
            }
        }

        fn len(&self) -> usize {
            self.0.len()
        }

        fn split_at(&mut self, offset: usize) -> Self {
            let right = Chunk(self.0[offset..].to_vec());
            self.0.truncate(offset);
            right
        }

        fn try_merge(&mut self, other: &Self) -> bool {
            if self.0.len() + other.0.len() > Self::MAX_LEN {
                return false;
            }
            self.0.extend_from_slice(&other.0);
            true
        }
    }

    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
    struct Len(u32);

    impl Dimension<ChunkSummary> for Len {
        fn from_summary(s: &ChunkSummary) -> Self {
            Len(s.len)
        }
        fn add_summary(&mut self, s: &ChunkSummary) {
            self.0 += s.len;
        }
    }

    /// Helper to collect all bytes from a tree by in-order traversal.
    fn collect_bytes<const B: usize>(tree: &SumTree<Chunk, B>) -> Vec<u8> {
        fn collect<const B: usize>(node: &Node<Chunk, B>, out: &mut Vec<u8>) {
            match node {
                Node::Leaf { item, .. } => out.extend_from_slice(&item.0),
                Node::Internal(internal) => {
                    for child in &internal.children {
                        collect(child, out);
                    }
                }
            }
        }
        let mut result = Vec::new();
        collect(tree.root.as_ref(), &mut result);
        result
    }

    // --- Construction tests ---

    #[test]
    fn from_item_creates_leaf() {
        let tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3]));
        assert!(tree.root.is_leaf());
        assert_eq!(tree.root.item().unwrap().0, vec![1, 2, 3]);
        assert_eq!(tree.len::<Len>(), Len(3));
    }

    #[test]
    fn from_items_creates_tree() {
        let items = vec![
            Chunk(vec![1, 2, 3]),
            Chunk(vec![4, 5, 6]),
            Chunk(vec![7, 8]),
        ];
        let tree = SumTree::<Chunk, 4>::from_items(items);
        assert_eq!(collect_bytes(&tree), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(tree.len::<Len>(), Len(8));
    }

    #[test]
    fn is_empty_for_empty_chunk() {
        let tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![]));
        assert!(tree.is_empty());
    }

    #[test]
    fn is_empty_false_for_nonempty() {
        let tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1]));
        assert!(!tree.is_empty());
    }

    // --- Insert tests ---

    #[test]
    fn insert_at_beginning() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3]));
        tree.insert(0, Chunk(vec![10, 20]));
        assert_eq!(collect_bytes(&tree), vec![10, 20, 1, 2, 3]);
    }

    #[test]
    fn insert_at_end() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3]));
        tree.insert(3, Chunk(vec![10, 20]));
        assert_eq!(collect_bytes(&tree), vec![1, 2, 3, 10, 20]);
    }

    #[test]
    fn insert_in_middle() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4]));
        tree.insert(2, Chunk(vec![10, 20]));
        assert_eq!(collect_bytes(&tree), vec![1, 2, 10, 20, 3, 4]);
    }

    #[test]
    fn insert_causes_split() {
        // Start with a full chunk (8 bytes = MAX_LEN)
        let items = vec![Chunk(vec![1, 2, 3, 4, 5, 6, 7, 8])];
        let mut tree = SumTree::<Chunk, 4>::from_items(items);
        // Insert 5 bytes in the middle — won't fit, must split
        tree.insert(4, Chunk(vec![10, 20, 30, 40, 50]));
        let bytes = collect_bytes(&tree);
        assert_eq!(bytes, vec![1, 2, 3, 4, 10, 20, 30, 40, 50, 5, 6, 7, 8]);
    }

    #[test]
    fn insert_merges_when_possible() {
        // Small chunk that can absorb an insert
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2]));
        tree.insert(2, Chunk(vec![3, 4]));
        // Should merge: [1,2] + [3,4] = [1,2,3,4] (4 <= MAX_LEN=8)
        assert_eq!(collect_bytes(&tree), vec![1, 2, 3, 4]);
        // Should still be a single leaf
        assert!(tree.root.is_leaf());
    }

    #[test]
    fn insert_at_beginning_merge() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![3, 4, 5]));
        tree.insert(0, Chunk(vec![1, 2]));
        // [1,2] + [3,4,5] = [1,2,3,4,5] (5 <= MAX_LEN=8)
        assert_eq!(collect_bytes(&tree), vec![1, 2, 3, 4, 5]);
        assert!(tree.root.is_leaf());
    }

    #[test]
    fn insert_into_multi_leaf_tree() {
        let items = vec![
            Chunk(vec![1, 2, 3, 4]),
            Chunk(vec![5, 6, 7, 8]),
            Chunk(vec![9, 10, 11, 12]),
        ];
        let mut tree = SumTree::<Chunk, 4>::from_items(items);
        tree.insert(6, Chunk(vec![20, 21]));
        assert_eq!(
            collect_bytes(&tree),
            vec![1, 2, 3, 4, 5, 6, 20, 21, 7, 8, 9, 10, 11, 12]
        );
    }

    // --- Delete tests ---

    #[test]
    fn delete_prefix() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.delete(0..2);
        assert_eq!(collect_bytes(&tree), vec![3, 4, 5]);
    }

    #[test]
    fn delete_suffix() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.delete(3..5);
        assert_eq!(collect_bytes(&tree), vec![1, 2, 3]);
    }

    #[test]
    fn delete_middle() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.delete(1..4);
        assert_eq!(collect_bytes(&tree), vec![1, 5]);
    }

    #[test]
    fn delete_entire_content() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3]));
        tree.delete(0..3);
        assert_eq!(collect_bytes(&tree), vec![]);
        assert!(tree.is_empty());
    }

    #[test]
    fn delete_spanning_multiple_leaves() {
        let items = vec![
            Chunk(vec![1, 2, 3, 4]),
            Chunk(vec![5, 6, 7, 8]),
            Chunk(vec![9, 10, 11, 12]),
        ];
        let mut tree = SumTree::<Chunk, 4>::from_items(items);
        tree.delete(2..10);
        assert_eq!(collect_bytes(&tree), vec![1, 2, 11, 12]);
    }

    #[test]
    fn delete_to_empty_collapses_internal_root() {
        // Build a tree large enough that the root is an Internal node, then
        // delete the entire content. This reproduces the `:%s`/`ggVGd` whole-
        // document delete that previously hit `unreachable!()` at the empty
        // Internal-root collapse. An empty buffer is a valid state.
        let items: Vec<Chunk> = (0..64u8).map(|i| Chunk(vec![i, i, i, i])).collect();
        let total: usize = items.iter().map(|c| c.0.len()).sum();
        let mut tree = SumTree::<Chunk, 4>::from_items(items);
        // Confirm the precondition: the root really is Internal (multi-level).
        assert!(
            !tree.root.is_leaf(),
            "test precondition: root must be Internal to exercise the collapse"
        );
        tree.delete(0..total); // delete EVERYTHING — must not panic
        assert_eq!(collect_bytes(&tree), Vec::<u8>::new());
        assert_eq!(tree.len::<Len>(), Len(0));
        assert!(tree.is_empty());
        assert!(
            tree.root.is_leaf(),
            "after whole-document delete the root must collapse to an empty leaf"
        );
        // The empty tree must remain usable: a subsequent insert works.
        tree.insert(0, Chunk(vec![9, 9]));
        assert_eq!(collect_bytes(&tree), vec![9, 9]);
    }

    #[test]
    fn delete_empty_range_is_noop() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3]));
        tree.delete(1..1);
        assert_eq!(collect_bytes(&tree), vec![1, 2, 3]);
    }

    #[test]
    fn delete_single_byte() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.delete(2..3);
        assert_eq!(collect_bytes(&tree), vec![1, 2, 4, 5]);
    }

    // --- Replace tests ---

    #[test]
    fn replace_in_middle() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.replace(1..4, Chunk(vec![10, 20]));
        assert_eq!(collect_bytes(&tree), vec![1, 10, 20, 5]);
    }

    #[test]
    fn replace_with_larger() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.replace(2..3, Chunk(vec![10, 20, 30, 40]));
        assert_eq!(collect_bytes(&tree), vec![1, 2, 10, 20, 30, 40, 4, 5]);
    }

    #[test]
    fn replace_with_smaller() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.replace(1..4, Chunk(vec![10]));
        assert_eq!(collect_bytes(&tree), vec![1, 10, 5]);
    }

    // --- COW / snapshot tests ---

    #[test]
    fn cow_snapshot_independence() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        let snapshot = tree.snapshot();
        tree.insert(2, Chunk(vec![10, 20]));
        assert_eq!(collect_bytes(&snapshot), vec![1, 2, 3, 4, 5]);
        assert_eq!(collect_bytes(&tree), vec![1, 2, 10, 20, 3, 4, 5]);
    }

    #[test]
    fn cow_snapshot_after_delete() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        let snapshot = tree.snapshot();
        tree.delete(1..4);
        assert_eq!(collect_bytes(&snapshot), vec![1, 2, 3, 4, 5]);
        assert_eq!(collect_bytes(&tree), vec![1, 5]);
    }

    #[test]
    fn multiple_snapshots_independent() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        let snap1 = tree.snapshot();
        tree.insert(5, Chunk(vec![6, 7]));
        let snap2 = tree.snapshot();
        tree.delete(0..2);

        assert_eq!(collect_bytes(&snap1), vec![1, 2, 3, 4, 5]);
        assert_eq!(collect_bytes(&snap2), vec![1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(collect_bytes(&tree), vec![3, 4, 5, 6, 7]);
    }

    // --- Summary correctness tests ---

    #[test]
    fn summary_stays_correct_after_edits() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        assert_eq!(tree.len::<Len>(), Len(5));
        tree.insert(2, Chunk(vec![10, 20, 30]));
        assert_eq!(tree.len::<Len>(), Len(8));
        tree.delete(1..4);
        assert_eq!(tree.len::<Len>(), Len(5));
    }

    #[test]
    fn summary_correct_after_replace() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.replace(1..4, Chunk(vec![10, 20]));
        // Started with 5, removed 3 (positions 1..4), added 2 = 4
        assert_eq!(tree.len::<Len>(), Len(4));
    }

    // --- Stress / many-operation tests ---

    #[test]
    fn many_inserts_maintain_invariants() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![0]));
        for i in 1..50u8 {
            tree.insert(i as usize, Chunk(vec![i]));
        }
        let bytes = collect_bytes(&tree);
        assert_eq!(bytes.len(), 50);
        assert_eq!(tree.len::<Len>(), Len(50));
    }

    #[test]
    fn alternating_insert_delete() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5, 6, 7, 8]));

        tree.insert(4, Chunk(vec![10, 20]));
        assert_eq!(tree.len::<Len>(), Len(10));

        tree.delete(2..6);
        assert_eq!(tree.len::<Len>(), Len(6));

        tree.insert(3, Chunk(vec![30, 40, 50]));
        assert_eq!(tree.len::<Len>(), Len(9));

        tree.delete(0..2);
        assert_eq!(tree.len::<Len>(), Len(7));

        let bytes = collect_bytes(&tree);
        assert_eq!(bytes.len(), 7);
    }

    #[test]
    fn large_tree_operations() {
        let items: Vec<Chunk> = (0..20)
            .map(|i| Chunk(vec![i * 4, i * 4 + 1, i * 4 + 2, i * 4 + 3]))
            .collect();
        let mut tree = SumTree::<Chunk, 4>::from_items(items);
        assert_eq!(tree.len::<Len>(), Len(80));

        tree.delete(30..50);
        assert_eq!(tree.len::<Len>(), Len(60));

        tree.insert(20, Chunk(vec![100, 101, 102, 103, 104]));
        assert_eq!(tree.len::<Len>(), Len(65));

        verify_tree_invariants(&tree);
    }

    #[test]
    fn delete_all_then_insert() {
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5]));
        tree.delete(0..5);
        assert!(tree.is_empty());
        tree.insert(0, Chunk(vec![10, 20, 30]));
        assert_eq!(collect_bytes(&tree), vec![10, 20, 30]);
        assert_eq!(tree.len::<Len>(), Len(3));
    }

    #[test]
    fn insert_three_way_split() {
        // Force a three-way split: MAX_LEN=8, insert 6 bytes into middle of a 6-byte chunk.
        // offset=3, so left=[3 bytes], new=[6 bytes], right=[3 bytes].
        // left+new = 9 > 8 (fail), new+right = 9 > 8 (fail).
        // This triggers SplitTwo.
        let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(vec![1, 2, 3, 4, 5, 6]));
        tree.insert(3, Chunk(vec![10, 20, 30, 40, 50, 60]));
        assert_eq!(
            collect_bytes(&tree),
            vec![1, 2, 3, 10, 20, 30, 40, 50, 60, 4, 5, 6]
        );
        assert_eq!(tree.len::<Len>(), Len(12));
    }

    // --- Invariant verification ---

    fn verify_tree_invariants<const B: usize>(tree: &SumTree<Chunk, B>) {
        fn verify_node<const B: usize>(node: &Node<Chunk, B>, is_root: bool) {
            match node {
                Node::Leaf { item, summary } => {
                    assert_eq!(summary.len, item.0.len() as u32);
                }
                Node::Internal(internal) => {
                    if !is_root {
                        assert!(
                            internal.children.len() >= B / 2,
                            "non-root internal has {} children, min is {}",
                            internal.children.len(),
                            B / 2
                        );
                    }
                    assert!(
                        internal.children.len() <= B,
                        "internal has {} children, max is {}",
                        internal.children.len(),
                        B
                    );
                    let mut expected_aggregate = ChunkSummary::default();
                    for (i, child) in internal.children.iter().enumerate() {
                        assert_eq!(
                            child.height(),
                            internal.height - 1,
                            "child {} at wrong height",
                            i
                        );
                        let actual = child.summary();
                        assert_eq!(
                            internal.summaries[i], actual,
                            "cached per-child summary mismatch at child {}",
                            i
                        );
                        expected_aggregate.compose(&internal.summaries[i]);
                        verify_node(child, false);
                    }
                    // Verify cached aggregate matches fold of per-child summaries.
                    assert_eq!(
                        internal.summary, expected_aggregate,
                        "cached aggregate summary mismatch: expected {:?}, got {:?}",
                        expected_aggregate, internal.summary
                    );
                }
            }
        }
        verify_node(tree.root.as_ref(), true);
    }

    // --- Property tests (oracle + boundary) ---

    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        /// Random operation on a SumTree, parameterized by fractional offsets
        /// so proptest can shrink meaningfully.
        #[derive(Clone, Debug)]
        enum Op {
            Insert {
                offset_frac: f64,
                data: Vec<u8>,
            },
            Delete {
                start_frac: f64,
                end_frac: f64,
            },
            Replace {
                start_frac: f64,
                end_frac: f64,
                data: Vec<u8>,
            },
        }

        fn op_strategy() -> impl Strategy<Value = Op> {
            prop_oneof![
                (0.0..=1.0f64, prop::collection::vec(0..255u8, 1..6))
                    .prop_map(|(offset_frac, data)| Op::Insert { offset_frac, data }),
                (0.0..=1.0f64, 0.0..=1.0f64).prop_map(|(a, b)| {
                    let (start_frac, end_frac) = if a <= b { (a, b) } else { (b, a) };
                    Op::Delete {
                        start_frac,
                        end_frac,
                    }
                }),
                (
                    0.0..=1.0f64,
                    0.0..=1.0f64,
                    prop::collection::vec(0..255u8, 0..4)
                )
                    .prop_map(|(a, b, data)| {
                        let (start_frac, end_frac) = if a <= b { (a, b) } else { (b, a) };
                        Op::Replace {
                            start_frac,
                            end_frac,
                            data,
                        }
                    }),
            ]
        }

        fn apply_op(tree: &mut SumTree<Chunk, 4>, oracle: &mut Vec<u8>, op: &Op) {
            let len = oracle.len();
            match op {
                Op::Insert { offset_frac, data } => {
                    let offset = (*offset_frac * len as f64).min(len as f64) as usize;
                    let offset = offset.min(len);
                    tree.insert(offset, Chunk(data.clone()));
                    oracle.splice(offset..offset, data.iter().copied());
                }
                Op::Delete {
                    start_frac,
                    end_frac,
                } => {
                    if len == 0 {
                        return;
                    }
                    let start = (*start_frac * len as f64) as usize;
                    let end = (*end_frac * len as f64) as usize;
                    let start = start.min(len);
                    let end = end.min(len);
                    if start >= end {
                        return;
                    }
                    tree.delete(start..end);
                    oracle.drain(start..end);
                }
                Op::Replace {
                    start_frac,
                    end_frac,
                    data,
                } => {
                    if len == 0 && data.is_empty() {
                        return;
                    }
                    let start = (*start_frac * len as f64) as usize;
                    let end = (*end_frac * len as f64) as usize;
                    let start = start.min(len);
                    let end = end.min(len);
                    if start > end {
                        return;
                    }
                    if start == end && data.is_empty() {
                        return;
                    }
                    tree.replace(start..end, Chunk(data.clone()));
                    oracle.splice(start..end, data.iter().copied());
                }
            }
        }

        proptest! {
            #[test]
            fn random_ops_match_oracle(ops in prop::collection::vec(op_strategy(), 1..50)) {
                let initial = vec![1u8, 2, 3, 4, 5];
                let mut tree = SumTree::<Chunk, 4>::from_item(Chunk(initial.clone()));
                let mut oracle = initial;

                for op in &ops {
                    apply_op(&mut tree, &mut oracle, op);
                    // Verify content matches oracle
                    let tree_bytes = collect_bytes(&tree);
                    prop_assert_eq!(&tree_bytes, &oracle,
                        "content mismatch after op {:?}", op);
                    // Verify summary length
                    prop_assert_eq!(tree.len::<Len>().0, oracle.len() as u32,
                        "length mismatch after op {:?}", op);
                    // Verify structural invariants
                    verify_tree_invariants(&tree);
                }
            }
        }

        // --- Boundary-exact operation tests ---

        #[test]
        fn insert_at_exact_chunk_boundaries() {
            let items = vec![
                Chunk(vec![1, 2, 3, 4]),
                Chunk(vec![5, 6, 7, 8]),
                Chunk(vec![9, 10, 11, 12]),
            ];
            let mut tree = SumTree::<Chunk, 4>::from_items(items);

            // Insert at boundary between chunk 1 and chunk 2 (offset 4)
            tree.insert(4, Chunk(vec![20, 21]));
            assert_eq!(
                collect_bytes(&tree),
                vec![1, 2, 3, 4, 20, 21, 5, 6, 7, 8, 9, 10, 11, 12]
            );
            verify_tree_invariants(&tree);

            // Insert at boundary between chunk 2 and chunk 3 (now offset 10)
            tree.insert(10, Chunk(vec![30, 31]));
            assert_eq!(
                collect_bytes(&tree),
                vec![1, 2, 3, 4, 20, 21, 5, 6, 7, 8, 30, 31, 9, 10, 11, 12]
            );
            verify_tree_invariants(&tree);
        }

        #[test]
        fn delete_at_exact_chunk_boundaries() {
            let items = vec![
                Chunk(vec![1, 2, 3, 4]),
                Chunk(vec![5, 6, 7, 8]),
                Chunk(vec![9, 10, 11, 12]),
            ];
            let mut tree = SumTree::<Chunk, 4>::from_items(items);

            // Delete exactly the second chunk (offset 4..8)
            tree.delete(4..8);
            assert_eq!(collect_bytes(&tree), vec![1, 2, 3, 4, 9, 10, 11, 12]);
            verify_tree_invariants(&tree);
        }

        #[test]
        fn delete_starting_at_chunk_boundary() {
            let items = vec![
                Chunk(vec![1, 2, 3, 4]),
                Chunk(vec![5, 6, 7, 8]),
                Chunk(vec![9, 10, 11, 12]),
            ];
            let mut tree = SumTree::<Chunk, 4>::from_items(items);

            // Delete from start of chunk 2 into middle of chunk 3
            tree.delete(4..10);
            assert_eq!(collect_bytes(&tree), vec![1, 2, 3, 4, 11, 12]);
            verify_tree_invariants(&tree);
        }
    }
}
