use std::sync::Arc;

use super::node::{InternalNode, Node};
use super::traits::Item;

/// Result of inserting into a node.
pub(crate) enum InsertResult<T: Item, const B: usize> {
    /// Node absorbed the insert without splitting.
    Absorbed,
    /// Node split. Returns the new right sibling that the parent must absorb.
    Split(Arc<Node<T, B>>),
    /// Node produced two new siblings (three-way split at leaf level).
    /// This occurs when inserting into the middle of a leaf produces three pieces
    /// that cannot be merged pairwise (e.g., all near MAX_LEN).
    SplitTwo(Arc<Node<T, B>>, Arc<Node<T, B>>),
}

/// Attempt to merge the right node into the left node.
///
/// - For leaves: delegates to `Item::try_merge`. If successful, updates the leaf summary.
/// - For internals: merges if combined children count <= B.
///
/// Returns true if the merge succeeded. On success, `left` contains both nodes' data.
///
/// # Panics
/// Panics if node types don't match (leaf + internal).
pub(crate) fn try_merge_nodes<T: Item, const B: usize>(
    left: &mut Node<T, B>,
    right: &Node<T, B>,
) -> bool {
    match (left, right) {
        (
            Node::Leaf {
                item: left_item,
                summary: left_summary,
            },
            Node::Leaf {
                item: right_item, ..
            },
        ) => {
            if left_item.try_merge(right_item) {
                *left_summary = left_item.summary();
                true
            } else {
                false
            }
        }
        (Node::Internal(left_internal), Node::Internal(right_internal)) => {
            if left_internal.children.len() + right_internal.children.len() > B {
                return false;
            }
            // Absorb all right's children into left.
            for i in 0..right_internal.children.len() {
                left_internal
                    .children
                    .push(Arc::clone(&right_internal.children[i]));
                left_internal
                    .summaries
                    .push(right_internal.summaries[i].clone());
            }
            left_internal.recompute_aggregate();
            true
        }
        _ => {
            panic!("try_merge_nodes: cannot merge nodes of different types (leaf + internal)");
        }
    }
}

/// Split an overfull internal node at the midpoint.
///
/// The left half remains in `node`, the right half is returned.
/// Both halves preserve the original height.
///
/// # Panics
/// Panics if the node has fewer than 2 children (nothing to split).
pub(crate) fn split_internal<T: Item, const B: usize>(
    node: &mut InternalNode<T, B>,
) -> InternalNode<T, B> {
    assert!(
        node.children.len() >= 2,
        "split_internal: node must have at least 2 children to split"
    );

    let total = node.children.len();
    let mid = total / 2;
    let height = node.height;

    // Build the right half by draining children [mid..total] from the left.
    // ArrayVec doesn't have split_off, so drain is the idiomatic approach.
    let mut right = InternalNode::new(height);
    for child in node.children.drain(mid..) {
        right.children.push(child);
    }
    for summary in node.summaries.drain(mid..) {
        right.summaries.push(summary);
    }

    // Recompute cached aggregates for both halves.
    node.recompute_aggregate();
    right.recompute_aggregate();

    right
}

/// If the child at `index` is undersized, rebalance it by merging with or
/// redistributing from a sibling.
///
/// Strategy:
/// 1. If child is NOT undersized, return immediately.
/// 2. Try merging with the right sibling (if one exists).
/// 3. If no right sibling or merge fails, try merging with the left sibling.
/// 4. If all merges fail, redistribute with a sibling (internals only).
///
/// Uses `Arc::make_mut` for COW semantics.
pub(crate) fn rebalance_child<T: Item, const B: usize>(
    parent: &mut InternalNode<T, B>,
    index: usize,
) {
    // If child is not undersized, nothing to do.
    if !parent.children[index].is_undersized() {
        return;
    }

    let num_children = parent.children.len();

    // Try merge with right sibling first.
    if index + 1 < num_children {
        let right_idx = index + 1;

        // Use split_at_mut to get simultaneous mutable access to both children.
        let (left_slice, right_slice) = parent.children.split_at_mut(right_idx);
        let left_node = Arc::make_mut(&mut left_slice[index]);
        let right_node = &*right_slice[0];

        if try_merge_nodes(left_node, right_node) {
            // Merge succeeded: remove right child from parent.
            parent.children.remove(right_idx);
            parent.summaries.remove(right_idx);
            // Update left child's summary.
            parent.update_summary(index);
            return;
        }
    }

    // Try merge with left sibling.
    if index > 0 {
        let left_idx = index - 1;

        // Use split_at_mut to get simultaneous mutable access.
        let (left_slice, right_slice) = parent.children.split_at_mut(index);
        let left_node = Arc::make_mut(&mut left_slice[left_idx]);
        let right_node = &*right_slice[0];

        if try_merge_nodes(left_node, right_node) {
            // Merge succeeded: remove the undersized child (now at `index`).
            parent.children.remove(index);
            parent.summaries.remove(index);
            // Update left sibling's summary.
            parent.update_summary(left_idx);
            return;
        }
    }

    // Merges failed — redistribute (only meaningful for internal nodes).
    // Pick the sibling with more children: prefer right, fallback left.
    if index + 1 < num_children {
        // Redistribute with right sibling.
        let (left_slice, right_slice) = parent.children.split_at_mut(index + 1);
        let left_node = Arc::make_mut(left_slice.last_mut().unwrap());
        let right_node = Arc::make_mut(right_slice.first_mut().unwrap());

        if let (Node::Internal(left_int), Node::Internal(right_int)) = (left_node, right_node) {
            redistribute_internal(left_int, right_int);
        }

        parent.update_summary(index);
        parent.update_summary(index + 1);
    } else if index > 0 {
        // Redistribute with left sibling.
        let (left_slice, right_slice) = parent.children.split_at_mut(index);
        let left_node = Arc::make_mut(left_slice.last_mut().unwrap());
        let right_node = Arc::make_mut(right_slice.first_mut().unwrap());

        if let (Node::Internal(left_int), Node::Internal(right_int)) = (left_node, right_node) {
            redistribute_internal(left_int, right_int);
        }

        parent.update_summary(index - 1);
        parent.update_summary(index);
    }
}

/// Redistribute children evenly between two internal nodes.
///
/// Collects all children from both nodes, splits at total/2.
/// Left gets the first half, right gets the rest.
///
/// # Panics
/// Panics if both nodes are empty.
pub(crate) fn redistribute_internal<T: Item, const B: usize>(
    left: &mut InternalNode<T, B>,
    right: &mut InternalNode<T, B>,
) {
    let total = left.children.len() + right.children.len();
    assert!(total > 0, "redistribute_internal: both nodes are empty");

    let left_count = total / 2;

    // Collect all children and summaries from both into temporary Vecs.
    // This is a rare rebalancing code path, so the temporary heap allocation is acceptable.
    let mut all_children: Vec<Arc<Node<T, B>>> = Vec::with_capacity(total);
    let mut all_summaries: Vec<T::Summary> = Vec::with_capacity(total);

    all_children.extend(left.children.drain(..));
    all_summaries.extend(left.summaries.drain(..));
    all_children.extend(right.children.drain(..));
    all_summaries.extend(right.summaries.drain(..));

    // Distribute: left gets [0..left_count], right gets [left_count..total].
    // Use drain to move values without cloning.
    let mut drain_children = all_children.drain(..);
    let mut drain_summaries = all_summaries.drain(..);
    for _ in 0..left_count {
        left.children.push(drain_children.next().unwrap());
        left.summaries.push(drain_summaries.next().unwrap());
    }
    for child in drain_children {
        right.children.push(child);
    }
    for summary in drain_summaries {
        right.summaries.push(summary);
    }

    // Recompute cached aggregates for both nodes.
    left.recompute_aggregate();
    right.recompute_aggregate();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::builder::build_tree;
    use crate::tree::test_helpers::test_items::*;

    /// Helper: build an internal node from N leaf items with branching factor B.
    fn make_internal<const B: usize>(values: &[u32]) -> InternalNode<NumItem, B> {
        let children: Vec<Arc<Node<NumItem, B>>> = values
            .iter()
            .map(|&v| Arc::new(Node::leaf(NumItem(v))))
            .collect();

        let mut node = InternalNode::new(1);
        for child in children {
            node.push_child(child);
        }
        node
    }

    #[test]
    fn split_internal_produces_balanced_halves() {
        // Build a full node with B=8 children, split into two halves.
        let mut node = make_internal::<8>(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(node.children.len(), 8);

        let right = split_internal::<NumItem, 8>(&mut node);

        // Left gets first half (4), right gets second half (4).
        assert_eq!(node.children.len(), 4);
        assert_eq!(right.children.len(), 4);
        assert_eq!(node.height, right.height);

        // Verify content order is preserved.
        assert_eq!(node.children[0].item().unwrap().0, 1);
        assert_eq!(node.children[3].item().unwrap().0, 4);
        assert_eq!(right.children[0].item().unwrap().0, 5);
        assert_eq!(right.children[3].item().unwrap().0, 8);
    }

    #[test]
    fn split_internal_odd_count() {
        // 7 children with B=8: split into [3, 4].
        let mut node = make_internal::<8>(&[10, 20, 30, 40, 50, 60, 70]);
        let right = split_internal::<NumItem, 8>(&mut node);

        assert_eq!(node.children.len(), 3);
        assert_eq!(right.children.len(), 4);
    }

    #[test]
    fn try_merge_leaves_returns_false_for_numitem() {
        // NumItem::try_merge always returns false.
        let mut left = Node::<NumItem, 8>::leaf(NumItem(10));
        let right = Node::<NumItem, 8>::leaf(NumItem(20));

        let result = try_merge_nodes(&mut left, &right);
        assert!(!result);
    }

    #[test]
    fn try_merge_internals_within_capacity() {
        // Two internal nodes with combined children <= B should merge.
        let left_internal = make_internal::<8>(&[1, 2, 3]);
        let right_internal = make_internal::<8>(&[4, 5]);

        let mut left = Node::Internal(left_internal);
        let right = Node::Internal(right_internal);

        let result = try_merge_nodes(&mut left, &right);
        assert!(result);

        if let Node::Internal(merged) = &left {
            assert_eq!(merged.children.len(), 5);
            // Verify summaries are correct.
            let total_summary = merged.summary();
            assert_eq!(total_summary.count, 5);
            assert_eq!(total_summary.sum, 1 + 2 + 3 + 4 + 5);
        } else {
            panic!("expected internal node after merge");
        }
    }

    #[test]
    fn try_merge_internals_over_capacity_fails() {
        // Two internal nodes with combined > B should fail.
        let mut left = Node::Internal(make_internal::<4>(&[1, 2, 3]));
        let right = Node::Internal(make_internal::<4>(&[4, 5, 6]));

        // 3 + 3 = 6 > B=4
        let result = try_merge_nodes(&mut left, &right);
        assert!(!result);

        // Left should be unchanged.
        if let Node::Internal(internal) = &left {
            assert_eq!(internal.children.len(), 3);
        }
    }

    #[test]
    #[should_panic(expected = "cannot merge nodes of different types")]
    fn try_merge_type_mismatch_panics() {
        let mut left = Node::<NumItem, 8>::leaf(NumItem(1));
        let right = Node::Internal(make_internal::<8>(&[2, 3]));

        try_merge_nodes(&mut left, &right);
    }

    #[test]
    fn redistribute_internal_evens_out() {
        // Left has 2 children, right has 6. Total=8, should become [4, 4].
        let mut left = make_internal::<8>(&[1, 2]);
        let mut right = make_internal::<8>(&[3, 4, 5, 6, 7, 8]);

        redistribute_internal::<NumItem, 8>(&mut left, &mut right);

        assert_eq!(left.children.len(), 4);
        assert_eq!(right.children.len(), 4);

        // Verify order preserved.
        assert_eq!(left.children[0].item().unwrap().0, 1);
        assert_eq!(left.children[3].item().unwrap().0, 4);
        assert_eq!(right.children[0].item().unwrap().0, 5);
        assert_eq!(right.children[3].item().unwrap().0, 8);
    }

    #[test]
    fn redistribute_internal_odd_total() {
        // Left=1, Right=6. Total=7, left gets 3, right gets 4.
        let mut left = make_internal::<8>(&[10]);
        let mut right = make_internal::<8>(&[20, 30, 40, 50, 60, 70]);

        redistribute_internal::<NumItem, 8>(&mut left, &mut right);

        assert_eq!(left.children.len(), 3);
        assert_eq!(right.children.len(), 4);
    }

    #[test]
    fn rebalance_child_noop_if_not_undersized() {
        // Create parent where all children have >= B/2 children.
        // B=4, so min is 2. Give child at index 0 exactly 2 leaves.
        let child0 = Arc::new(Node::Internal(make_internal::<4>(&[1, 2])));
        let child1 = Arc::new(Node::Internal(make_internal::<4>(&[3, 4])));
        let child2 = Arc::new(Node::Internal(make_internal::<4>(&[5, 6])));

        let mut parent = InternalNode::<NumItem, 4>::new(2);
        parent.push_child(child0);
        parent.push_child(child1);
        parent.push_child(child2);

        // None are undersized (each has 2 children, B/2 = 2).
        rebalance_child(&mut parent, 0);

        // Parent unchanged.
        assert_eq!(parent.children.len(), 3);
    }

    #[test]
    fn rebalance_child_merges_undersized_with_right() {
        // B=8, min = 4. Create child with 3 children (undersized) and right sibling with 4.
        // Combined = 7 <= 8 = B, so merge should succeed.
        let undersized = Arc::new(Node::Internal(make_internal::<8>(&[1, 2, 3])));
        let adequate = Arc::new(Node::Internal(make_internal::<8>(&[4, 5, 6, 7])));
        let other = Arc::new(Node::Internal(make_internal::<8>(&[8, 9, 10, 11])));

        let mut parent = InternalNode::<NumItem, 8>::new(2);
        parent.push_child(undersized);
        parent.push_child(adequate);
        parent.push_child(other);

        rebalance_child(&mut parent, 0);

        // Merge succeeded: parent lost a child.
        assert_eq!(parent.children.len(), 2);

        // First child should now have 7 children.
        if let Node::Internal(merged) = parent.children[0].as_ref() {
            assert_eq!(merged.children.len(), 7);
        } else {
            panic!("expected internal node");
        }
    }

    #[test]
    fn rebalance_child_merges_undersized_with_left() {
        // B=8, min = 4. Last child is undersized, no right sibling.
        // Left sibling has 4, undersized has 3. Combined = 7 <= 8, merge succeeds.
        let adequate = Arc::new(Node::Internal(make_internal::<8>(&[1, 2, 3, 4])));
        let other = Arc::new(Node::Internal(make_internal::<8>(&[5, 6, 7, 8])));
        let undersized = Arc::new(Node::Internal(make_internal::<8>(&[9, 10, 11])));

        let mut parent = InternalNode::<NumItem, 8>::new(2);
        parent.push_child(adequate);
        parent.push_child(other);
        parent.push_child(undersized);

        rebalance_child(&mut parent, 2);

        // Merge with left sibling (index 1): parent lost child at index 2.
        assert_eq!(parent.children.len(), 2);

        // Second child (former "other") absorbed the undersized child.
        if let Node::Internal(merged) = parent.children[1].as_ref() {
            assert_eq!(merged.children.len(), 7);
        } else {
            panic!("expected internal node");
        }
    }

    #[test]
    fn rebalance_child_redistributes_when_merge_fails() {
        // B=4, min = 2. Undersized child has 1 child. Right sibling has 4.
        // Combined = 5 > B=4, merge fails. Should redistribute to [2, 3].
        let undersized = Arc::new(Node::Internal(make_internal::<4>(&[1])));
        let full = Arc::new(Node::Internal(make_internal::<4>(&[2, 3, 4, 5])));

        let mut parent = InternalNode::<NumItem, 4>::new(2);
        parent.push_child(undersized);
        parent.push_child(full);

        rebalance_child(&mut parent, 0);

        // Redistribute happened, parent still has 2 children.
        assert_eq!(parent.children.len(), 2);

        // Both children should now be adequately filled.
        if let Node::Internal(left) = parent.children[0].as_ref() {
            assert!(left.children.len() >= 2, "left has {}", left.children.len());
        }
        if let Node::Internal(right) = parent.children[1].as_ref() {
            assert!(
                right.children.len() >= 2,
                "right has {}",
                right.children.len()
            );
        }
    }

    #[test]
    fn rebalance_preserves_cow_semantics() {
        // Clone a parent (simulating snapshot), then rebalance. Original unchanged.
        let undersized = Arc::new(Node::Internal(make_internal::<8>(&[1, 2, 3])));
        let adequate = Arc::new(Node::Internal(make_internal::<8>(&[4, 5, 6, 7])));

        let mut parent = InternalNode::<NumItem, 8>::new(2);
        parent.push_child(undersized);
        parent.push_child(adequate);

        // Take a "snapshot" of the children.
        let snapshot_child0 = Arc::clone(&parent.children[0]);
        let snapshot_child1 = Arc::clone(&parent.children[1]);

        rebalance_child(&mut parent, 0);

        // Parent was modified (merge happened).
        assert_eq!(parent.children.len(), 1);

        // Snapshots still point to original nodes (COW).
        if let Node::Internal(snap) = snapshot_child0.as_ref() {
            assert_eq!(snap.children.len(), 3);
        }
        if let Node::Internal(snap) = snapshot_child1.as_ref() {
            assert_eq!(snap.children.len(), 4);
        }
    }

    #[test]
    fn split_internal_preserves_height() {
        let mut node: InternalNode<NumItem, 8> = InternalNode::new(5);
        for v in 1..=6u32 {
            node.push_child(Arc::new(Node::leaf(NumItem(v))));
        }

        let right = split_internal::<NumItem, 8>(&mut node);
        assert_eq!(node.height, 5);
        assert_eq!(right.height, 5);
    }

    #[test]
    fn split_internal_summaries_are_correct() {
        let mut node = make_internal::<8>(&[10, 20, 30, 40, 50, 60]);
        let right = split_internal::<NumItem, 8>(&mut node);

        // 6 children, midpoint is 3. Left: [10, 20, 30], Right: [40, 50, 60].
        let left_sum = node.summary();
        let right_sum = right.summary();

        assert_eq!(left_sum.count, 3);
        assert_eq!(left_sum.sum, 10 + 20 + 30);
        assert_eq!(right_sum.count, 3);
        assert_eq!(right_sum.sum, 40 + 50 + 60);
    }

    #[test]
    fn build_tree_and_verify_no_undersized() {
        // Integration: build a tree, verify no non-root node is undersized.
        let items: Vec<NumItem> = (1..=50).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);

        fn check_not_undersized<const B: usize>(node: &Node<NumItem, B>, is_root: bool) {
            if !is_root {
                assert!(!node.is_undersized(), "non-root node is undersized");
            }
            if let Node::Internal(internal) = node {
                for child in &internal.children {
                    check_not_undersized(child, false);
                }
            }
        }

        check_not_undersized::<4>(&root, true);
    }
}
