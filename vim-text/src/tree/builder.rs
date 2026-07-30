use std::sync::Arc;

use super::node::{InternalNode, Node};
use super::traits::Item;

/// Build a balanced tree from a vec of items. O(n).
/// Panics if items is empty.
pub(crate) fn build_tree<T: Item, const B: usize>(items: Vec<T>) -> Arc<Node<T, B>> {
    assert!(!items.is_empty(), "cannot build tree from empty items");

    // Convert items to leaf nodes
    let mut level: Vec<Arc<Node<T, B>>> = items
        .into_iter()
        .map(|item| Arc::new(Node::leaf(item)))
        .collect();

    let mut height: u8 = 0;

    // Bottom-up: group leaves into internal nodes, repeat until single root
    while level.len() > 1 {
        height += 1;
        let mut next_level = Vec::new();
        let mut i = 0;

        while i < level.len() {
            let remaining = level.len() - i;
            // Runt avoidance: if remaining is between B and 2*B, split evenly
            let chunk_size = if remaining > B && remaining < 2 * B {
                remaining.div_ceil(2)
            } else {
                remaining.min(B)
            };

            let children: Vec<Arc<Node<T, B>>> = level[i..i + chunk_size].to_vec();
            let internal = InternalNode::from_children(children, height);
            next_level.push(Arc::new(Node::Internal(internal)));
            i += chunk_size;
        }

        level = next_level;
    }

    level.into_iter().next().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_helpers::test_items::*;

    #[test]
    fn build_single_item() {
        let root = build_tree::<NumItem, 4>(vec![NumItem(42)]);
        assert!(root.is_leaf());
        assert_eq!(root.item().unwrap().0, 42);
    }

    #[test]
    fn build_multiple_produces_internal_root() {
        let items: Vec<NumItem> = (1..=10).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);
        assert!(!root.is_leaf());
        let summary = root.summary();
        assert_eq!(summary.sum, (1..=10).sum::<u32>());
        assert_eq!(summary.count, 10);
    }

    #[test]
    fn build_summary_correct_for_large_tree() {
        let items: Vec<NumItem> = (1..=100).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);
        let summary = root.summary();
        assert_eq!(summary.sum, 5050);
        assert_eq!(summary.count, 100);
    }

    #[test]
    fn build_respects_max_children() {
        let items: Vec<NumItem> = (1..=100).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);
        // Verify no internal node has more than B=4 children
        fn check_invariants<const B: usize>(node: &Node<NumItem, B>) {
            if let Node::Internal(internal) = node {
                assert!(
                    internal.children.len() <= B,
                    "node has {} children, max is {B}",
                    internal.children.len()
                );
                assert!(!internal.children.is_empty());
                // All children should be at the same height
                let expected_height = internal.height - 1;
                for child in &internal.children {
                    assert_eq!(child.height(), expected_height);
                    check_invariants(child);
                }
            }
        }
        check_invariants::<4>(&root);
    }

    #[test]
    fn build_all_leaves_at_same_depth() {
        let items: Vec<NumItem> = (1..=50).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);
        fn leaf_depths<const B: usize>(
            node: &Node<NumItem, B>,
            depth: usize,
            depths: &mut Vec<usize>,
        ) {
            match node {
                Node::Leaf { .. } => depths.push(depth),
                Node::Internal(internal) => {
                    for child in &internal.children {
                        leaf_depths(child, depth + 1, depths);
                    }
                }
            }
        }
        let mut depths = Vec::new();
        leaf_depths::<4>(&root, 0, &mut depths);
        let first = depths[0];
        assert!(
            depths.iter().all(|&d| d == first),
            "leaves at different depths: {:?}",
            depths
        );
    }

    #[test]
    fn build_with_b8_default() {
        let items: Vec<NumItem> = (1..=64).map(NumItem).collect();
        let root = build_tree::<NumItem, 8>(items);
        assert_eq!(root.summary().count, 64);
        assert!(root.height() >= 2); // 64 items with B=8 needs at least height 2
    }

    #[test]
    #[should_panic(expected = "cannot build tree from empty items")]
    fn build_empty_panics() {
        let items: Vec<NumItem> = vec![];
        build_tree::<NumItem, 4>(items);
    }

    #[test]
    fn build_exactly_b_items() {
        // n=4, B=4: should be single internal node with 4 leaf children
        let items: Vec<NumItem> = (1..=4).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);
        assert!(!root.is_leaf());
        if let Node::Internal(internal) = root.as_ref() {
            assert_eq!(internal.children.len(), 4);
            assert!(internal.children.iter().all(|c| c.is_leaf()));
        } else {
            panic!("expected internal node");
        }
    }

    #[test]
    fn build_b_plus_one_runt_avoidance() {
        // n=5, B=4: runt avoidance should produce [3, 2] not [4, 1]
        let items: Vec<NumItem> = (1..=5).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);
        if let Node::Internal(internal) = root.as_ref() {
            assert_eq!(internal.children.len(), 2);
            // At height 1: 5 leaves grouped into [3, 2] internal nodes
            // Level 0: 5 leaves. remaining=5 > 4 && < 8. chunk_size = ceil(5/2) = 3.
            // Groups: [3, 2]. Two internal nodes.
            // Level 1: 2 nodes. 2 <= B=4, done. Root is internal with 2 children.
            assert_eq!(internal.children.len(), 2);
            // Verify no child has only 1 leaf (that would be a "runt")
            for child in &internal.children {
                if let Node::Internal(inner) = child.as_ref() {
                    assert!(
                        inner.children.len() >= 2,
                        "runt detected: child has only {} children",
                        inner.children.len()
                    );
                }
            }
        } else {
            panic!("expected internal root");
        }
    }

    #[test]
    fn build_two_b_minus_one() {
        // n=7, B=4: runt avoidance triggers, should produce [4, 3]
        let items: Vec<NumItem> = (1..=7).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);
        if let Node::Internal(internal) = root.as_ref() {
            assert_eq!(internal.children.len(), 2);
        } else {
            panic!("expected internal root");
        }
    }

    #[test]
    fn build_two_b_items() {
        // n=8, B=4: exactly 2*B, produces [4, 4]
        let items: Vec<NumItem> = (1..=8).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);
        if let Node::Internal(internal) = root.as_ref() {
            assert_eq!(internal.children.len(), 2);
            for child in &internal.children {
                if let Node::Internal(inner) = child.as_ref() {
                    assert_eq!(inner.children.len(), 4);
                }
            }
        } else {
            panic!("expected internal root");
        }
    }

    #[test]
    fn build_no_undersized_non_root() {
        // For various sizes, verify no non-root internal node is undersized
        for n in [5, 9, 15, 20, 33, 50, 100] {
            let items: Vec<NumItem> = (1..=n).map(|i| NumItem(i as u32)).collect();
            let root = build_tree::<NumItem, 4>(items);
            fn check_fill<const B: usize>(node: &Node<NumItem, B>, is_root: bool) {
                if let Node::Internal(internal) = node {
                    if !is_root {
                        assert!(
                            internal.children.len() >= B / 2,
                            "non-root node has {} children, min is {}",
                            internal.children.len(),
                            B / 2
                        );
                    }
                    for child in &internal.children {
                        check_fill(child, false);
                    }
                }
            }
            check_fill::<4>(&root, true);
        }
    }

    #[test]
    fn build_preserves_item_order() {
        let items: Vec<NumItem> = (1..=20).map(NumItem).collect();
        let root = build_tree::<NumItem, 4>(items);

        // Collect all leaf items left-to-right
        fn collect_items<const B: usize>(node: &Node<NumItem, B>, out: &mut Vec<u32>) {
            match node {
                Node::Leaf { item, .. } => out.push(item.0),
                Node::Internal(internal) => {
                    for child in &internal.children {
                        collect_items(child, out);
                    }
                }
            }
        }

        let mut collected = Vec::new();
        collect_items::<4>(&root, &mut collected);
        let expected: Vec<u32> = (1..=20).collect();
        assert_eq!(collected, expected);
    }
}
