use std::sync::Arc;

use arrayvec::ArrayVec;

use super::node::{InternalNode, Node, MAX_HEIGHT};
use super::traits::{Bias, Dimension, Item, Summary};

/// A stack frame recording the cursor's position within an internal node.
struct StackFrame<'a, T: Item, D: Dimension<T::Summary>, const B: usize> {
    node: &'a InternalNode<T, B>,
    index: usize,
    position: D, // accumulated dimension at entry to this frame
}

/// A dimension-typed cursor for O(log n) traversal and seeking within a SumTree.
///
/// Generic over `D: Dimension<T::Summary>` — the primary tracking dimension.
/// The cursor maintains a stack of internal node references for efficient
/// up/down traversal without heap allocation (uses ArrayVec).
///
/// The position tracks the accumulated dimension up to (not including) the
/// current item.
pub(crate) struct Cursor<'a, T: Item, D: Dimension<T::Summary>, const B: usize = 8> {
    root: &'a Arc<Node<T, B>>,
    stack: ArrayVec<StackFrame<'a, T, D, B>, MAX_HEIGHT>,
    position: D,
    item: Option<&'a T>,
    item_summary: Option<&'a T::Summary>,
    at_end: bool,
}

impl<'a, T: Item, D: Dimension<T::Summary>, const B: usize> Cursor<'a, T, D, B> {
    /// Create a new cursor positioned at the first leaf item.
    pub fn new(root: &'a Arc<Node<T, B>>) -> Self {
        let mut cursor = Cursor {
            root,
            stack: ArrayVec::new(),
            position: D::default(),
            item: None,
            item_summary: None,
            at_end: false,
        };
        cursor.reset_to_first();
        cursor
    }

    /// Returns the current position (accumulated dimension up to but not including the current item).
    ///
    /// Only the cursor's own tests read the raw position; production callers go
    /// through `start`/`end`, so this is compiled out of non-test builds.
    #[cfg(test)]
    pub fn pos(&self) -> D {
        self.position
    }

    /// Returns a reference to the current leaf item, or None if at_end.
    pub fn item(&self) -> Option<&'a T> {
        self.item
    }

    /// Returns a reference to the current leaf item's summary, or None if at_end.
    pub fn item_summary(&self) -> Option<&'a T::Summary> {
        self.item_summary
    }

    /// Returns true if the cursor has moved past the last item.
    ///
    /// Production code detects exhaustion via `item()` returning `None`; only
    /// the cursor's own tests query the flag directly.
    #[cfg(test)]
    pub fn at_end(&self) -> bool {
        self.at_end
    }

    /// Read the start position of the current item in any dimension D2.
    pub fn start<D2: Dimension<T::Summary>>(&self) -> D2 {
        let mut d = D2::default();
        for frame in &self.stack {
            for i in 0..frame.index {
                d.add_summary(&frame.node.summaries[i]);
            }
        }
        d
    }

    /// Compute the aggregate summary of all items before the current cursor position.
    /// O(height * B) — walks the stack accumulating child summaries to the left of each frame's index.
    /// When `at_end`, returns the total tree summary (all items precede the cursor).
    pub fn prefix_summary(&self) -> T::Summary {
        if self.at_end {
            return self.root.summary();
        }
        let mut summary = T::Summary::default();
        for frame in &self.stack {
            for i in 0..frame.index {
                summary.compose(&frame.node.summaries[i]);
            }
        }
        summary
    }

    /// Read the end position of the current item in any dimension D2.
    pub fn end<D2: Dimension<T::Summary>>(&self) -> D2 {
        let mut d = self.start::<D2>();
        if let Some(summary) = self.item_summary {
            d.add_summary(summary);
        }
        d
    }

    /// Seek to the given target position from the root. Always O(log n).
    ///
    /// With Bias::Right: positions at the first item whose start position >= target.
    /// With Bias::Left: positions at the last item whose end position <= target.
    ///
    /// If target >= total dimension of the tree, sets at_end.
    pub fn seek(&mut self, target: &D, bias: Bias) {
        self.stack.clear();
        self.position = D::default();
        self.item = None;
        self.item_summary = None;
        self.at_end = false;

        self.seek_internal(self.root.as_ref(), target, bias);
    }

    /// Seek forward from the current position using the up-then-down algorithm.
    /// O(log d) where d is the distance to the target. O(1) amortized for sequential access.
    /// If the target is behind the current position, falls back to a full seek.
    ///
    /// No production caller does sequential seeking yet; the algorithm is kept
    /// alive by its differential tests against `seek`, so it is compiled only
    /// into test builds.
    #[cfg(test)]
    pub fn seek_forward(&mut self, target: &D, bias: Bias) {
        if self.at_end {
            self.seek(target, bias);
            return;
        }

        // If current position is already at or past target, handle accordingly
        match bias {
            Bias::Right => {
                if self.position >= *target {
                    if self.position == *target {
                        return;
                    }
                    self.seek(target, bias);
                    return;
                }
            }
            Bias::Left => {
                if self.position >= *target {
                    self.seek(target, bias);
                    return;
                }
            }
        }

        // Check if the current item already satisfies the seek target
        if let Some(summary) = self.item_summary {
            let mut end_pos = self.position;
            end_pos.add_summary(summary);

            let current_satisfies = match bias {
                Bias::Left => end_pos >= *target,
                Bias::Right => end_pos > *target,
            };

            if current_satisfies {
                return;
            }
        }

        // Up-then-down: walk up the stack checking right siblings.
        // For each frame, check siblings to the right of the current index.
        // If a sibling's cumulative end reaches the target, descend into it.
        //
        // Incremental position tracking: advance self.position past the current
        // leaf item so it marks the end of the bottommost subtree. As we pop
        // each frame, self.position is already at the end of children[frame.index]
        // (everything below has been accounted for). Scanning siblings just adds
        // their summaries — no recomputation from the root.
        if let Some(summary) = self.item_summary {
            self.position.add_summary(summary);
        }

        while let Some(frame) = self.stack.pop() {
            // self.position is at the end of children[frame.index].
            // Scan remaining siblings.
            for sibling_idx in (frame.index + 1)..frame.node.children.len() {
                let sibling_summary = &frame.node.summaries[sibling_idx];
                let mut end_after_sibling = self.position;
                end_after_sibling.add_summary(sibling_summary);

                let overshoot = match bias {
                    Bias::Left => end_after_sibling >= *target,
                    Bias::Right => end_after_sibling > *target,
                };

                if overshoot {
                    // Target is within this sibling. Re-push frame with updated index,
                    // position stored is the node entry position (from the original frame).
                    self.stack.push(StackFrame {
                        node: frame.node,
                        index: sibling_idx,
                        position: frame.position,
                    });
                    let child = frame.node.children[sibling_idx].as_ref();
                    self.seek_internal(child, target, bias);
                    return;
                }

                // Target is past this sibling — skip it entirely
                self.position = end_after_sibling;
            }

            // All siblings in this frame exhausted — continue walking up
        }

        // Entire stack exhausted without finding the target — at_end
        self.at_end = true;
        self.item = None;
        self.item_summary = None;
    }

    /// Advance to the next leaf item. Returns true if successful, false if at_end.
    pub fn next(&mut self) -> bool {
        self.search_forward(|_| true)
    }

    /// Advance to the next leaf whose summary satisfies the filter predicate.
    ///
    /// At each tree level, tests each child's summary via `filter_node`. If the
    /// filter returns `true`, descends into that child. If `false`, skips the
    /// entire subtree (adding its summary to the cursor's position).
    ///
    /// This enables O(log n) pruned search: when most subtrees fail the filter,
    /// only O(height * B) summaries are tested plus one leaf is visited.
    ///
    /// The filter receives `&T::Summary` and returns `true` to descend/visit,
    /// `false` to skip. The filter is `FnMut` so it can track state (e.g.,
    /// running bracket depth that updates when subtrees are skipped).
    ///
    /// `search_forward(|_| true)` is equivalent to `next()` — it visits every
    /// leaf sequentially.
    pub fn search_forward<F>(&mut self, mut filter_node: F) -> bool
    where
        F: FnMut(&T::Summary) -> bool,
    {
        if self.at_end {
            return false;
        }

        // Add current item's dimension contribution to position (move past it).
        if let Some(summary) = self.item_summary {
            self.position.add_summary(summary);
        }
        self.item = None;
        self.item_summary = None;

        // `descend` tracks whether we just descended into a new subtree (true)
        // or are continuing a lateral scan from a previously-visited child (false).
        // When descending, we scan from index 0; when continuing, from index+1.
        let mut descend = false;

        while !self.stack.is_empty() {
            // Borrow the top frame. Determine which children to scan.
            let entry = self.stack.last_mut().unwrap();
            let node = entry.node;

            if !descend {
                // Lateral: advance past the child we came from.
                entry.index += 1;
            }

            // Scan children from entry.index onward, testing the filter.
            while entry.index < node.children.len() {
                let child_summary = &node.summaries[entry.index];
                if filter_node(child_summary) {
                    break;
                }
                // Filter rejected: skip this subtree, advance position.
                entry.index += 1;
                self.position.add_summary(child_summary);
            }

            if entry.index < node.children.len() {
                // Found a child whose summary passed the filter. Descend into it.
                let child = node.children[entry.index].as_ref();
                match child {
                    Node::Leaf { item, summary } => {
                        // Reached a matching leaf — done.
                        self.item = Some(item);
                        self.item_summary = Some(summary);
                        return true;
                    }
                    Node::Internal(internal) => {
                        // Push frame for the internal node and descend.
                        self.stack.push(StackFrame {
                            node: internal,
                            index: 0,
                            position: self.position,
                        });
                        descend = true;
                    }
                }
            } else {
                // All children in this frame exhausted. Pop and continue upward.
                descend = false;
                self.stack.pop();
            }
        }

        // Stack exhausted — no matching leaf found.
        self.at_end = true;
        false
    }

    /// Retreat to the previous leaf item. Returns true if successful, false if at beginning.
    pub fn prev(&mut self) -> bool {
        self.search_backward(|_| true)
    }

    /// Retreat to the previous leaf whose summary satisfies the filter predicate.
    ///
    /// Mirror of `search_forward`: iterates siblings right-to-left, descends
    /// into the rightmost matching child. Subtrees whose summary fails the
    /// filter are skipped entirely.
    ///
    /// Position is recovered in O(B) from the cached position in the deepest
    /// stack frame, rather than recomputing from the full stack.
    ///
    /// `search_backward(|_| true)` is equivalent to `prev()`.
    pub fn search_backward<F>(&mut self, mut filter_node: F) -> bool
    where
        F: FnMut(&T::Summary) -> bool,
    {
        if self.at_end {
            // Position at the end of the tree, ready to scan backward from the last child.
            self.at_end = false;
            self.stack.clear();
            self.item = None;
            self.item_summary = None;

            match self.root.as_ref() {
                Node::Leaf { item, summary } => {
                    // Single-leaf tree: test the leaf directly.
                    if filter_node(summary) {
                        self.position = D::default();
                        self.item = Some(item);
                        self.item_summary = Some(summary);
                        return true;
                    }
                    // Leaf doesn't match. Reset to beginning.
                    self.reset_to_first();
                    return false;
                }
                Node::Internal(internal) => {
                    // Push a sentinel frame past the last child.
                    // The main loop uses `descend=false` so it will decrement
                    // before scanning.
                    self.stack.push(StackFrame {
                        node: internal,
                        index: internal.children.len(),
                        position: D::default(),
                    });
                }
            }
        } else {
            // Clear current item — we're moving away from it.
            self.item = None;
            self.item_summary = None;
        }

        // `descend` tracks whether we just descended into a new subtree (true)
        // or are continuing a lateral scan from a previously-visited child (false).
        // When descending, we scan from the last child; when continuing, from index-1.
        let mut descend = false;

        while !self.stack.is_empty() {
            {
                let entry = self.stack.last_mut().unwrap();

                if !descend {
                    // Lateral: move left of the child we came from.
                    if entry.index == 0 {
                        self.stack.pop();
                        continue;
                    }
                    entry.index -= 1;
                }

                // Scan children from entry.index downward (right-to-left).
                let node = entry.node;
                let mut exhausted = false;
                loop {
                    let child_summary = &node.summaries[entry.index];
                    if filter_node(child_summary) {
                        break;
                    }
                    if entry.index == 0 {
                        exhausted = true;
                        break;
                    }
                    entry.index -= 1;
                }

                if exhausted {
                    self.stack.pop();
                    descend = false;
                    continue;
                }
            }
            // entry borrow released — safe to read/push the stack.

            let entry = self.stack.last().unwrap();
            // Compute the position at entry to the child we're about to visit.
            let child_entry_pos = Self::position_from_frame(entry);
            let node = entry.node;
            let child = node.children[entry.index].as_ref();
            match child {
                Node::Leaf { item, summary } => {
                    self.item = Some(item);
                    self.item_summary = Some(summary);
                    self.position = child_entry_pos;
                    return true;
                }
                Node::Internal(internal) => {
                    let last = internal.children.len() - 1;
                    self.stack.push(StackFrame {
                        node: internal,
                        index: last,
                        position: child_entry_pos,
                    });
                    descend = true;
                }
            }
        }

        // Stack exhausted — no matching leaf found. Reset to beginning.
        self.reset_to_first();
        false
    }

    // --- Private helpers ---

    /// Compute the dimension at entry to `frame.node.children[frame.index]`.
    ///
    /// Equivalent to `frame.position + sum(frame.node.summaries[0..frame.index])`.
    fn position_from_frame(frame: &StackFrame<'a, T, D, B>) -> D {
        let mut pos = frame.position;
        for i in 0..frame.index {
            pos.add_summary(&frame.node.summaries[i]);
        }
        pos
    }

    /// Position at the first leaf of the tree.
    fn reset_to_first(&mut self) {
        self.stack.clear();
        self.position = D::default();
        self.at_end = false;
        self.descend_left(self.root.as_ref());
    }

    /// Core seek logic: descend from a node toward the target.
    fn seek_internal(&mut self, node: &'a Node<T, B>, target: &D, bias: Bias) {
        let mut current = node;
        loop {
            match current {
                Node::Leaf { item, summary } => {
                    // Check if target is past this leaf's end
                    let mut end_pos = self.position;
                    end_pos.add_summary(summary);

                    let past_end = match bias {
                        Bias::Left => end_pos < *target,
                        Bias::Right => end_pos <= *target,
                    };

                    if past_end {
                        // Target is beyond this leaf — at_end
                        self.at_end = true;
                        self.item = None;
                        self.item_summary = None;
                        return;
                    }

                    // For Bias::Left: position at the item whose end >= target.
                    // If position == target and position > 0, this means we're at
                    // the boundary — we want the previous item (whose end == target).
                    if bias == Bias::Left
                        && self.position >= *target
                        && self.position > D::default()
                    {
                        // go_prev_from_leaf sets item/item_summary via descend_right
                        self.go_prev_from_leaf();
                    } else {
                        self.item = Some(item);
                        self.item_summary = Some(summary);
                    }
                    return;
                }
                Node::Internal(internal) => {
                    if internal.children.is_empty() {
                        self.at_end = true;
                        self.item = None;
                        self.item_summary = None;
                        return;
                    }

                    // Capture position at entry to this node before pick_child
                    // advances self.position past preceding siblings.
                    let node_entry_pos = self.position;
                    let child_index = self.pick_child(internal, target, bias);

                    // Check if target is past all children (at_end case)
                    if child_index >= internal.children.len() {
                        // Move position to the end of this internal node
                        self.at_end = true;
                        self.item = None;
                        self.item_summary = None;
                        return;
                    }

                    self.stack.push(StackFrame {
                        node: internal,
                        index: child_index,
                        position: node_entry_pos,
                    });
                    current = internal.children[child_index].as_ref();
                }
            }
        }
    }

    /// Pick which child to descend into. Updates self.position to reflect
    /// the accumulated dimension before that child.
    fn pick_child(&mut self, internal: &'a InternalNode<T, B>, target: &D, bias: Bias) -> usize {
        let mut pos = self.position;

        for (i, summary) in internal.summaries.iter().enumerate() {
            let mut end_pos = pos;
            end_pos.add_summary(summary);

            let overshoot = match bias {
                Bias::Left => end_pos >= *target,
                Bias::Right => end_pos > *target,
            };

            if overshoot {
                self.position = pos;
                return i;
            }

            pos = end_pos;
        }

        // Target is at or past all children
        self.position = pos;
        internal.children.len() // signals at_end
    }

    /// Descend to the leftmost leaf from the given node.
    fn descend_left(&mut self, node: &'a Node<T, B>) {
        let mut current = node;
        loop {
            match current {
                Node::Leaf { item, summary } => {
                    self.item = Some(item);
                    self.item_summary = Some(summary);
                    return;
                }
                Node::Internal(internal) => {
                    self.stack.push(StackFrame {
                        node: internal,
                        index: 0,
                        position: self.position,
                    });
                    current = internal.children[0].as_ref();
                }
            }
        }
    }

    /// Descend to the rightmost leaf from the given node.
    fn descend_right(&mut self, node: &'a Node<T, B>) {
        let mut current = node;
        loop {
            match current {
                Node::Leaf { item, summary } => {
                    self.item = Some(item);
                    self.item_summary = Some(summary);
                    return;
                }
                Node::Internal(internal) => {
                    let last = internal.children.len() - 1;
                    self.stack.push(StackFrame {
                        node: internal,
                        index: last,
                        position: self.position,
                    });
                    // Accumulate preceding siblings so nested frames
                    // record the correct entry position for their node.
                    for i in 0..last {
                        self.position.add_summary(&internal.summaries[i]);
                    }
                    current = internal.children[last].as_ref();
                }
            }
        }
    }

    /// After landing on a leaf during seek with Bias::Left where position >= target,
    /// move back one item.
    fn go_prev_from_leaf(&mut self) {
        // Walk up until we find a frame with index > 0
        loop {
            if let Some(frame) = self.stack.last_mut() {
                if frame.index > 0 {
                    frame.index -= 1;
                    // Compute position at entry to the child we're descending into.
                    self.position = Self::position_from_frame(frame);
                    let child = &frame.node.children[frame.index];
                    self.descend_right(child.as_ref());
                    // After descend_right, recover position from the deepest frame.
                    if let Some(deepest) = self.stack.last() {
                        self.position = Self::position_from_frame(deepest);
                    } else {
                        self.position = D::default();
                    }
                    return;
                } else {
                    self.stack.pop();
                }
            } else {
                // Can't go back — stay at first item
                self.reset_to_first();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::sum_tree::SumTree;
    use crate::tree::test_helpers::test_items::*;

    /// Build a tree of items 1..=n with B=4 for testing.
    fn build_test_tree(n: u32) -> SumTree<NumItem, 4> {
        let items: Vec<NumItem> = (1..=n).map(NumItem).collect();
        SumTree::<NumItem, 4>::from_items(items)
    }

    #[test]
    fn seek_to_beginning() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);
        assert_eq!(cursor.pos(), Count(0));
        assert_eq!(cursor.item(), Some(&NumItem(1)));
    }

    #[test]
    fn seek_to_middle() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(10), Bias::Right);
        assert_eq!(cursor.pos(), Count(10));
        assert_eq!(cursor.item(), Some(&NumItem(11)));
    }

    #[test]
    fn seek_to_end() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(20), Bias::Right);
        assert!(cursor.at_end());
        assert_eq!(cursor.item(), None);
    }

    #[test]
    fn next_traverses_all_items_in_order() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        let mut items = Vec::new();
        while let Some(item) = cursor.item() {
            items.push(item.0);
            if !cursor.next() {
                break;
            }
        }
        let expected: Vec<u32> = (1..=20).collect();
        assert_eq!(items, expected);
    }

    #[test]
    fn prev_traverses_all_items_in_reverse() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        // Seek to last item
        cursor.seek(&Count(19), Bias::Right);
        assert_eq!(cursor.item(), Some(&NumItem(20)));

        let mut items = vec![cursor.item().unwrap().0];
        while cursor.prev() {
            items.push(cursor.item().unwrap().0);
        }
        let expected: Vec<u32> = (1..=20).rev().collect();
        assert_eq!(items, expected);
    }

    #[test]
    fn seek_forward_matches_seek() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(5), Bias::Right);
        assert_eq!(cursor.pos(), Count(5));
        assert_eq!(cursor.item(), Some(&NumItem(6)));

        cursor.seek_forward(&Count(10), Bias::Right);
        assert_eq!(cursor.pos(), Count(10));
        assert_eq!(cursor.item(), Some(&NumItem(11)));
    }

    #[test]
    fn bias_left_stops_before_boundary() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        // With Bias::Left, seeking Count(5) should land on the item whose end is Count(5),
        // which is NumItem(5) at position Count(4).
        cursor.seek(&Count(5), Bias::Left);
        assert_eq!(cursor.pos(), Count(4));
        assert_eq!(cursor.item(), Some(&NumItem(5)));
    }

    #[test]
    fn bias_right_passes_boundary() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        // With Bias::Right, seeking Count(5) should land on the item at start == Count(5),
        // which is NumItem(6).
        cursor.seek(&Count(5), Bias::Right);
        assert_eq!(cursor.pos(), Count(5));
        assert_eq!(cursor.item(), Some(&NumItem(6)));
    }

    #[test]
    fn start_and_end_dimensions() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(5), Bias::Right);
        assert_eq!(cursor.start::<Count>(), Count(5));
        assert_eq!(cursor.end::<Count>(), Count(6));
    }

    #[test]
    fn cross_dimension_query() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(5), Bias::Right);
        // Sum of items 1..=5 = 15
        assert_eq!(cursor.start::<Sum>(), Sum(1 + 2 + 3 + 4 + 5));
        // End includes item 6: 15 + 6 = 21
        assert_eq!(cursor.end::<Sum>(), Sum(1 + 2 + 3 + 4 + 5 + 6));
    }

    #[test]
    fn cursor_on_single_leaf_tree() {
        let tree = SumTree::<NumItem, 4>::from_item(NumItem(42));
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);
        assert_eq!(cursor.pos(), Count(0));
        assert_eq!(cursor.item(), Some(&NumItem(42)));

        // next should move past end
        assert!(!cursor.next());
        assert!(cursor.at_end());

        // seek to 1 should be at_end
        cursor.seek(&Count(1), Bias::Right);
        assert!(cursor.at_end());
    }

    #[test]
    fn seek_forward_from_beginning() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek_forward(&Count(3), Bias::Right);
        assert_eq!(cursor.pos(), Count(3));
        assert_eq!(cursor.item(), Some(&NumItem(4)));

        cursor.seek_forward(&Count(7), Bias::Right);
        assert_eq!(cursor.pos(), Count(7));
        assert_eq!(cursor.item(), Some(&NumItem(8)));

        cursor.seek_forward(&Count(19), Bias::Right);
        assert_eq!(cursor.pos(), Count(19));
        assert_eq!(cursor.item(), Some(&NumItem(20)));
    }

    #[test]
    fn interleaved_next_prev() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);
        assert_eq!(cursor.item(), Some(&NumItem(1)));

        cursor.next(); // -> item 2
        cursor.next(); // -> item 3
        cursor.next(); // -> item 4
        assert_eq!(cursor.item(), Some(&NumItem(4)));
        assert_eq!(cursor.pos(), Count(3));

        cursor.prev(); // -> item 3
        assert_eq!(cursor.item(), Some(&NumItem(3)));
        assert_eq!(cursor.pos(), Count(2));

        cursor.prev(); // -> item 2
        assert_eq!(cursor.item(), Some(&NumItem(2)));
        assert_eq!(cursor.pos(), Count(1));

        cursor.next(); // -> item 3 again
        assert_eq!(cursor.item(), Some(&NumItem(3)));
        assert_eq!(cursor.pos(), Count(2));

        cursor.next(); // -> item 4
        assert_eq!(cursor.item(), Some(&NumItem(4)));
        assert_eq!(cursor.pos(), Count(3));
    }

    #[test]
    fn seek_forward_skips_intermediate() {
        // Build a tree with 100 items (B=4) to ensure multiple internal levels.
        let tree = build_test_tree(100);
        let mut cursor = tree.cursor::<Count>();

        // Seek forward from 0 to 50 — should skip many intermediate nodes.
        cursor.seek_forward(&Count(50), Bias::Right);
        assert_eq!(cursor.pos(), Count(50));
        assert_eq!(cursor.item(), Some(&NumItem(51)));

        // Seek forward again to 52 — a short hop within the same region.
        cursor.seek_forward(&Count(52), Bias::Right);
        assert_eq!(cursor.pos(), Count(52));
        assert_eq!(cursor.item(), Some(&NumItem(53)));

        // Verify cross-dimension consistency: sum of items 1..=52 = 52*53/2 = 1378
        assert_eq!(cursor.start::<Sum>(), Sum(52 * 53 / 2));
    }

    #[test]
    fn seek_forward_matches_seek_exhaustive() {
        // For a range of (start, target) pairs, verify seek_forward produces
        // the same result as a fresh seek, for both biases.
        let tree = build_test_tree(100);

        let positions: Vec<u32> = (0..=100).step_by(7).collect();

        for &start in &positions {
            for &target in &positions {
                if target < start {
                    continue; // seek_forward only tested for forward movement
                }

                for bias in [Bias::Left, Bias::Right] {
                    // Fresh seek for reference
                    let mut ref_cursor = tree.cursor::<Count>();
                    ref_cursor.seek(&Count(target), bias);

                    // seek_forward from start
                    let mut fwd_cursor = tree.cursor::<Count>();
                    fwd_cursor.seek(&Count(start), Bias::Right);
                    fwd_cursor.seek_forward(&Count(target), bias);

                    assert_eq!(
                        fwd_cursor.item(),
                        ref_cursor.item(),
                        "item mismatch: start={start}, target={target}, bias={bias:?}"
                    );
                    assert_eq!(
                        fwd_cursor.pos(),
                        ref_cursor.pos(),
                        "pos mismatch: start={start}, target={target}, bias={bias:?}"
                    );
                    assert_eq!(
                        fwd_cursor.at_end(),
                        ref_cursor.at_end(),
                        "at_end mismatch: start={start}, target={target}, bias={bias:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn prefix_summary_matches_manual_computation() {
        let items: Vec<NumItem> = (1..=20).map(NumItem).collect();
        let tree = SumTree::<NumItem, 4>::from_items(items);

        let mut cursor = tree.cursor::<Count>();

        // Seek to position 10 and verify prefix summary
        cursor.seek(&Count(10), Bias::Right);
        let prefix = cursor.prefix_summary();
        // Sum of items 1..=10 = 55, count = 10
        assert_eq!(prefix.sum, 55);
        assert_eq!(prefix.count, 10);

        // Seek to position 0 — prefix should be identity (default)
        cursor.seek(&Count(0), Bias::Right);
        let prefix = cursor.prefix_summary();
        assert_eq!(prefix.sum, 0);
        assert_eq!(prefix.count, 0);

        // Seek to position 15
        cursor.seek(&Count(15), Bias::Right);
        let prefix = cursor.prefix_summary();
        // Sum of items 1..=15 = 120, count = 15
        assert_eq!(prefix.sum, 120);
        assert_eq!(prefix.count, 15);

        // Seek to end (past all items)
        cursor.seek(&Count(100), Bias::Right);
        let prefix = cursor.prefix_summary();
        // Sum of all items 1..=20 = 210, count = 20
        assert_eq!(prefix.sum, 210);
        assert_eq!(prefix.count, 20);
    }

    #[test]
    fn prefix_summary_after_next() {
        let items: Vec<NumItem> = (1..=10).map(NumItem).collect();
        let tree = SumTree::<NumItem, 4>::from_items(items);

        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        // Walk forward with next(), checking prefix at each step
        let mut expected_sum = 0u32;
        let mut expected_count = 0u32;
        loop {
            let prefix = cursor.prefix_summary();
            assert_eq!(prefix.sum, expected_sum, "at count {expected_count}");
            assert_eq!(prefix.count, expected_count);

            if let Some(item) = cursor.item() {
                expected_sum += item.0;
                expected_count += 1;
            }
            if !cursor.next() {
                break;
            }
        }
    }

    // -----------------------------------------------------------------------
    //  search_forward / search_backward tests
    // -----------------------------------------------------------------------

    #[test]
    fn search_forward_always_true_matches_next() {
        // search_forward(|_| true) should visit items in the same order as next().
        let tree = build_test_tree(50);

        let mut next_cursor = tree.cursor::<Count>();
        next_cursor.seek(&Count(0), Bias::Right);

        let mut search_cursor = tree.cursor::<Count>();
        search_cursor.seek(&Count(0), Bias::Right);

        // Both start on item 1. Now advance both and compare.
        while next_cursor.next() {
            let found = search_cursor.search_forward(|_| true);
            assert!(found);
            assert_eq!(
                next_cursor.item(),
                search_cursor.item(),
                "item mismatch at pos {:?}",
                next_cursor.pos()
            );
            assert_eq!(next_cursor.pos(), search_cursor.pos(), "position mismatch");
        }
        // Both should be at_end now.
        let found = search_cursor.search_forward(|_| true);
        assert!(!found);
        assert!(search_cursor.at_end());
    }

    #[test]
    fn search_backward_always_true_matches_prev() {
        // search_backward(|_| true) should visit items in the same reverse order as prev().
        let tree = build_test_tree(50);

        let mut prev_cursor = tree.cursor::<Count>();
        prev_cursor.seek(&Count(49), Bias::Right);

        let mut search_cursor = tree.cursor::<Count>();
        search_cursor.seek(&Count(49), Bias::Right);

        while prev_cursor.prev() {
            let found = search_cursor.search_backward(|_| true);
            assert!(found);
            assert_eq!(
                prev_cursor.item(),
                search_cursor.item(),
                "item mismatch at pos {:?}",
                prev_cursor.pos()
            );
            assert_eq!(prev_cursor.pos(), search_cursor.pos(), "position mismatch");
        }
        let found = search_cursor.search_backward(|_| true);
        assert!(!found);
    }

    #[test]
    fn search_forward_finds_target_by_sum() {
        // Use the Sum dimension's property: each NumItem(v) has summary.sum = v.
        // An internal node's summary.sum = total of all descendants.
        // Search for the first leaf with value > 7 by checking if the subtree's
        // max possible item value could be > 7. Since items are 1..=10, a subtree
        // containing items > 7 will have sum > 7.
        let items: Vec<NumItem> = (1..=10).map(NumItem).collect();
        let tree = SumTree::<NumItem, 4>::from_items(items);

        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        // Search for first item where the item's own sum > 7.
        // The filter sees per-child summaries. For a leaf, summary.sum == item value.
        // For an internal node, summary.sum == total of all children's values.
        // An internal node with sum <= 7 can't contain any item with value > 7 if
        // all items are positive. So: filter(summary) = summary.sum > 7.
        // This may descend into some subtrees unnecessarily (false positives) but
        // will never miss the target (no false negatives).
        let found = cursor.search_forward(|summary: &NumSummary| summary.sum > 7);
        assert!(found);
        // The first item with value > 7 is NumItem(8) at position Count(7).
        assert_eq!(cursor.item(), Some(&NumItem(8)));
        assert_eq!(cursor.pos(), Count(7));
    }

    #[test]
    fn search_forward_skips_subtrees_efficiently() {
        // With 100 items and B=4, tree height is ~3-4. If the filter always returns
        // true, search_forward should visit only ~height nodes from the current
        // position to the next leaf (the same path as next()). Counting filter
        // calls gives a measure of efficiency.
        let items: Vec<NumItem> = (1..=100).map(NumItem).collect();
        let tree = SumTree::<NumItem, 4>::from_items(items);

        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        let mut calls = 0u32;
        let found = cursor.search_forward(|_summary: &NumSummary| {
            calls += 1;
            true
        });
        assert!(found);
        // With B=4 and ~100 items, the tree has height ~3. An always-true filter
        // should descend directly — visiting at most O(height) = ~4 summaries.
        assert!(
            calls <= 10,
            "Expected <= 10 filter calls for always-true, got {}",
            calls
        );
    }

    #[test]
    fn search_forward_never_true_reaches_end() {
        // If the filter always returns false, every subtree is skipped and we reach at_end.
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        let found = cursor.search_forward(|_| false);
        assert!(!found);
        assert!(cursor.at_end());
        // Position should be advanced past all items.
        assert_eq!(cursor.pos(), Count(20));
    }

    #[test]
    fn search_backward_never_true_reaches_beginning() {
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(19), Bias::Right);

        let found = cursor.search_backward(|_| false);
        assert!(!found);
        // Should reset to beginning (first item).
        assert_eq!(cursor.item(), Some(&NumItem(1)));
        assert_eq!(cursor.pos(), Count(0));
    }

    #[test]
    fn search_forward_from_at_end_returns_false() {
        let tree = build_test_tree(10);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(10), Bias::Right);
        assert!(cursor.at_end());

        let found = cursor.search_forward(|_| true);
        assert!(!found);
        assert!(cursor.at_end());
    }

    #[test]
    fn search_backward_from_at_end() {
        // When at_end, search_backward should scan from the last element backward.
        let tree = build_test_tree(10);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(10), Bias::Right);
        assert!(cursor.at_end());

        let found = cursor.search_backward(|_| true);
        assert!(found);
        assert_eq!(cursor.item(), Some(&NumItem(10)));
        assert_eq!(cursor.pos(), Count(9));
    }

    #[test]
    fn search_forward_from_middle() {
        // Start at position 5 (item 6), then search forward for item with value > 15.
        let items: Vec<NumItem> = (1..=20).map(NumItem).collect();
        let tree = SumTree::<NumItem, 4>::from_items(items);

        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(5), Bias::Right);
        assert_eq!(cursor.item(), Some(&NumItem(6)));

        let found = cursor.search_forward(|summary: &NumSummary| summary.sum > 15);
        assert!(found);
        // First item after 6 with value > 15 is NumItem(16) at position Count(15).
        assert_eq!(cursor.item(), Some(&NumItem(16)));
        assert_eq!(cursor.pos(), Count(15));
    }

    #[test]
    fn search_backward_from_middle() {
        // Start at position 15 (item 16), search backward for any item.
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(15), Bias::Right);
        assert_eq!(cursor.item(), Some(&NumItem(16)));

        let found = cursor.search_backward(|_| true);
        assert!(found);
        assert_eq!(cursor.item(), Some(&NumItem(15)));
        assert_eq!(cursor.pos(), Count(14));
    }

    #[test]
    fn search_forward_single_leaf_tree() {
        let tree = SumTree::<NumItem, 4>::from_item(NumItem(42));
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);
        assert_eq!(cursor.item(), Some(&NumItem(42)));

        // search_forward from the only item — should hit at_end.
        let found = cursor.search_forward(|_| true);
        assert!(!found);
        assert!(cursor.at_end());
    }

    #[test]
    fn search_backward_single_leaf_tree() {
        let tree = SumTree::<NumItem, 4>::from_item(NumItem(42));
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        // search_backward from the only item — should fail (no previous item).
        let found = cursor.search_backward(|_| true);
        assert!(!found);
        // Should be back at the first item.
        assert_eq!(cursor.item(), Some(&NumItem(42)));
    }

    #[test]
    fn search_forward_position_tracking() {
        // After search_forward, the cursor's position should correctly reflect
        // all skipped subtrees' dimension contributions.
        let tree = build_test_tree(100);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        // Skip items until we find one where the subtree summary's count == 1
        // (meaning it's a leaf-level summary). This always matches, so it's
        // equivalent to next() — but we verify position stays correct.
        let mut count = 0;
        while cursor.search_forward(|_| true) {
            count += 1;
            let expected_pos = Count(count);
            assert_eq!(
                cursor.pos(),
                expected_pos,
                "position mismatch after {} calls",
                count
            );
        }
        assert_eq!(count, 99); // 100 items, started on item 1, visited 99 more
    }

    #[test]
    fn search_forward_cross_dimension_consistency() {
        // After search_forward, the Sum dimension should match the expected value.
        let tree = build_test_tree(50);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        // Move forward 10 items via search_forward
        for _ in 0..10 {
            cursor.search_forward(|_| true);
        }
        // Now at item 11, position Count(10). Sum of items 1..=10 = 55.
        assert_eq!(cursor.item(), Some(&NumItem(11)));
        assert_eq!(cursor.start::<Sum>(), Sum(55));
    }

    #[test]
    fn search_forward_and_backward_round_trip() {
        // search_forward then search_backward should return to the same item.
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(10), Bias::Right);
        assert_eq!(cursor.item(), Some(&NumItem(11)));

        cursor.search_forward(|_| true);
        assert_eq!(cursor.item(), Some(&NumItem(12)));

        cursor.search_backward(|_| true);
        assert_eq!(cursor.item(), Some(&NumItem(11)));
        assert_eq!(cursor.pos(), Count(10));
    }

    #[test]
    fn search_forward_selective_filter() {
        // Filter that only accepts items with even values.
        // For internal nodes: a subtree might contain even values if its sum
        // could possibly include an even item. Since we can't tell from sum alone,
        // use count: if count >= 2, the subtree has at least 2 items, so it likely
        // has an even value. If count == 1, check the sum (which equals the value).
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);
        assert_eq!(cursor.item(), Some(&NumItem(1)));

        let found = cursor.search_forward(|summary: &NumSummary| {
            if summary.count == 1 {
                // Leaf summary: sum == the item's value.
                summary.sum % 2 == 0
            } else {
                // Internal node: could contain even items. Always descend.
                true
            }
        });
        assert!(found);
        assert_eq!(cursor.item(), Some(&NumItem(2)));

        // Search again for the next even item
        let found = cursor.search_forward(|summary: &NumSummary| {
            if summary.count == 1 {
                summary.sum % 2 == 0
            } else {
                true
            }
        });
        assert!(found);
        assert_eq!(cursor.item(), Some(&NumItem(4)));
    }

    #[test]
    fn search_backward_selective_filter() {
        // Start at item 20, search backward for even items.
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(19), Bias::Right);
        assert_eq!(cursor.item(), Some(&NumItem(20)));

        let found = cursor.search_backward(|summary: &NumSummary| {
            if summary.count == 1 {
                summary.sum % 2 == 0
            } else {
                true
            }
        });
        assert!(found);
        assert_eq!(cursor.item(), Some(&NumItem(18)));

        let found = cursor.search_backward(|summary: &NumSummary| {
            if summary.count == 1 {
                summary.sum % 2 == 0
            } else {
                true
            }
        });
        assert!(found);
        assert_eq!(cursor.item(), Some(&NumItem(16)));
    }

    #[test]
    fn search_forward_large_tree_finds_last_item() {
        let tree = build_test_tree(200);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        // Search for item with value == 200. Filter: for a leaf, sum == 200.
        // For an internal node, we must descend if the subtree could contain 200.
        // Since items are 1..=200, a subtree with max value >= 200 will have it.
        // We can't compute max from sum alone, but sum >= 200 is necessary.
        let found = cursor.search_forward(|summary: &NumSummary| {
            if summary.count == 1 {
                summary.sum == 200
            } else {
                // The subtree's sum must be >= 200 to possibly contain item 200.
                summary.sum >= 200
            }
        });
        assert!(found);
        assert_eq!(cursor.item(), Some(&NumItem(200)));
        assert_eq!(cursor.pos(), Count(199));
    }

    #[test]
    fn search_backward_large_tree_finds_first_item() {
        let tree = build_test_tree(200);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(199), Bias::Right);

        // Search backward for item with value == 1.
        let found = cursor.search_backward(|summary: &NumSummary| {
            if summary.count == 1 {
                summary.sum == 1
            } else {
                true // always descend into multi-item subtrees
            }
        });
        assert!(found);
        assert_eq!(cursor.item(), Some(&NumItem(1)));
        assert_eq!(cursor.pos(), Count(0));
    }

    #[test]
    fn search_forward_with_stateful_filter() {
        // The filter is FnMut — it can track state. Use it to find the 5th item
        // by counting how many leaves we visit.
        let tree = build_test_tree(20);
        let mut cursor = tree.cursor::<Count>();
        cursor.seek(&Count(0), Bias::Right);

        let mut leaves_visited = 0u32;
        let target = 5;
        let found = cursor.search_forward(|summary: &NumSummary| {
            if summary.count == 1 {
                leaves_visited += 1;
                leaves_visited >= target
            } else {
                true
            }
        });
        assert!(found);
        // Started at item 1. The 5th leaf visited forward is item 6.
        assert_eq!(cursor.item(), Some(&NumItem(6)));
    }
}
