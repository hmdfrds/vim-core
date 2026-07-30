//! History stack for incremental syntax selection.
//!
//! Tracks previous selection snapshots so that "shrink to child" (g])
//! retraces the exact expansion path of "expand to parent" (g[).
//!
//! Stores full `Selections` snapshots to preserve direction, primary
//! index, and multi-cursor state. Invalidated eagerly on text edits
//! (by the effect processor) and lazily via containment validation on pop.

use crate::primitives::Selections;

/// History stack for incremental syntax selection.
///
/// Each entry is a full `Selections` snapshot captured before an
/// expand or fan-out operation. Shrink (g]) pops and restores.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SyntaxSelectionHistory {
    stack: Vec<Selections>,
}

impl SyntaxSelectionHistory {
    /// Push a selections snapshot onto the history stack.
    pub fn push(&mut self, snapshot: Selections) {
        self.stack.push(snapshot);
    }

    /// Pop the most recent snapshot from the history stack.
    pub fn pop(&mut self) -> Option<Selections> {
        self.stack.pop()
    }

    /// Peek at the most recent snapshot without removing it.
    #[must_use]
    pub fn peek(&self) -> Option<&Selections> {
        self.stack.last()
    }

    /// Clear the entire history stack.
    pub fn clear(&mut self) {
        self.stack.clear();
    }

    /// Returns true if the history stack is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Returns the number of entries in the history stack.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.stack.len()
    }
}

/// Returns true if every range in `subset` is fully contained
/// by some range in `superset`.
///
/// Both inputs must be normalized (sorted by start, non-overlapping).
/// Runs in O(n + m) time via a single linear scan.
#[must_use]
pub fn selections_contained_by(subset: &Selections, superset: &Selections) -> bool {
    let subset_ranges = subset.ranges();
    let superset_ranges = superset.ranges();
    let mut si = 0;

    for sub in subset_ranges {
        let sub_start = sub.start();
        let sub_end = sub.end();
        while si < superset_ranges.len() && superset_ranges[si].end() < sub_start {
            si += 1;
        }
        if si >= superset_ranges.len() {
            return false;
        }
        if !(superset_ranges[si].start() <= sub_start && sub_end <= superset_ranges[si].end()) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Offset, SelectionRange};

    fn sel(anchor: usize, head: usize) -> SelectionRange {
        SelectionRange::new(Offset::new(anchor), Offset::new(head))
    }

    fn single(anchor: usize, head: usize) -> Selections {
        Selections::single(sel(anchor, head))
    }

    #[test]
    fn push_pop_lifo_order() {
        let mut h = SyntaxSelectionHistory::default();
        h.push(single(0, 10));
        h.push(single(5, 15));
        assert_eq!(h.len(), 2);
        let popped = h.pop().unwrap();
        assert_eq!(popped.primary().start(), Offset::new(5));
        let popped = h.pop().unwrap();
        assert_eq!(popped.primary().start(), Offset::new(0));
        assert!(h.pop().is_none());
    }

    #[test]
    fn clear_empties_stack() {
        let mut h = SyntaxSelectionHistory::default();
        h.push(single(0, 10));
        h.push(single(5, 15));
        h.clear();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert!(h.pop().is_none());
    }

    #[test]
    fn peek_returns_last_without_removing() {
        let mut h = SyntaxSelectionHistory::default();
        h.push(single(0, 10));
        assert_eq!(h.peek().unwrap().primary().start(), Offset::new(0));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn default_is_empty() {
        let h = SyntaxSelectionHistory::default();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert!(h.peek().is_none());
    }

    #[test]
    fn containment_single_range_contained() {
        let inner = single(5, 10);
        let outer = single(0, 20);
        assert!(selections_contained_by(&inner, &outer));
    }

    #[test]
    fn containment_single_range_not_contained() {
        let inner = single(5, 25);
        let outer = single(0, 20);
        assert!(!selections_contained_by(&inner, &outer));
    }

    #[test]
    fn containment_equal_ranges() {
        let a = single(5, 10);
        let b = single(5, 10);
        assert!(selections_contained_by(&a, &b));
    }

    #[test]
    fn containment_empty_subset() {
        let inner = single(5, 5);
        let outer = single(0, 20);
        assert!(selections_contained_by(&inner, &outer));
    }

    #[test]
    fn containment_subset_outside_superset() {
        let inner = single(25, 30);
        let outer = single(0, 20);
        assert!(!selections_contained_by(&inner, &outer));
    }

    #[test]
    fn containment_multiple_subset_in_one_superset() {
        let inner = Selections::from_vec(vec![sel(2, 5), sel(8, 12)], 0);
        let outer = single(0, 20);
        assert!(selections_contained_by(&inner, &outer));
    }

    #[test]
    fn containment_subset_spans_two_superset_ranges() {
        let inner = single(5, 15);
        let outer = Selections::from_vec(vec![sel(0, 10), sel(12, 20)], 0);
        assert!(!selections_contained_by(&inner, &outer));
    }
}
