//! Host-side fold provider for FFI/WASM hosts.
//!
//! Hosts reached across an ABI boundary cannot perform synchronous callbacks across the
//! WASM boundary. Instead they push fold state before each `processKey` call
//! using [`VimEngine::set_fold_state`].
//!
//! [`HostFoldProvider`] implements [`FoldProvider`] on top of a
//! pre-pushed `Vec<(LineNumber, LineNumber)>` of hidden-line ranges.
//! Each entry is a `(start, end)` pair (both inclusive) representing a
//! contiguous block of folded/hidden lines.

use crate::primitives::{Direction, LineNumber};

use super::FoldProvider;

/// Fold provider backed by a pre-pushed list of hidden-line ranges.
///
/// The host populates this before each `processKey` call via
/// [`VimEngine::set_fold_state`](crate::execution::VimEngine::set_fold_state). Each `(start, end)` entry is an
/// inclusive range of hidden lines. Ranges must be non-overlapping and
/// sorted in ascending order for the binary search in `find_range` to
/// work correctly.
///
/// # Example
///
/// ```ignore
/// // Lines 3–5 and 10–12 are folded.
/// engine.set_fold_state(vec![
///     (LineNumber::new(3), LineNumber::new(5)),
///     (LineNumber::new(10), LineNumber::new(12)),
/// ]);
/// ```
pub struct HostFoldProvider {
    hidden: Vec<(LineNumber, LineNumber)>,
}

impl HostFoldProvider {
    /// Create a new provider from the given hidden-line ranges.
    ///
    /// `hidden` is a list of `(start, end)` inclusive ranges. Ranges must be
    /// non-overlapping and sorted in ascending order.
    #[inline]
    #[must_use]
    pub const fn new(hidden: Vec<(LineNumber, LineNumber)>) -> Self {
        Self { hidden }
    }

    /// Find the range that contains `line`, if any.
    ///
    /// Uses `partition_point` (binary search) for O(log n) lookup.
    /// Returns the `(start, end)` pair when `line` falls inside a range.
    fn find_range(&self, line: LineNumber) -> Option<(LineNumber, LineNumber)> {
        // Find the last range whose start <= line.
        let idx = self.hidden.partition_point(|(start, _end)| *start <= line);
        if idx == 0 {
            return None;
        }
        let (start, end) = self.hidden[idx - 1];
        if line <= end {
            Some((start, end))
        } else {
            None
        }
    }
}

impl FoldProvider for HostFoldProvider {
    fn is_folded(&self, line: LineNumber) -> bool {
        self.find_range(line).is_some()
    }

    fn next_visible_line(&self, line: LineNumber, direction: Direction) -> LineNumber {
        match self.find_range(line) {
            None => line,
            Some((start, end)) => match direction {
                Direction::Forward => end.next(),
                Direction::Backward => start.prev(),
            },
        }
    }

    fn enclosing_fold(&self, line: LineNumber) -> Option<(LineNumber, LineNumber)> {
        self.find_range(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Direction, LineNumber};

    fn ln(n: usize) -> LineNumber {
        LineNumber::new(n)
    }

    #[test]
    fn empty_ranges() {
        let p = HostFoldProvider::new(vec![]);
        assert!(!p.is_folded(ln(0)));
        assert!(!p.is_folded(ln(5)));
        assert_eq!(p.next_visible_line(ln(3), Direction::Forward), ln(3));
        assert_eq!(p.next_visible_line(ln(3), Direction::Backward), ln(3));
    }

    #[test]
    fn single_fold_header_visible() {
        // Lines 2–4 are folded. Line 1 (the fold header) is visible.
        let p = HostFoldProvider::new(vec![(ln(2), ln(4))]);
        assert!(!p.is_folded(ln(1)));
        assert!(p.is_folded(ln(2)));
        assert!(p.is_folded(ln(3)));
        assert!(p.is_folded(ln(4)));
        assert!(!p.is_folded(ln(5)));
    }

    #[test]
    fn next_visible_forward() {
        // Lines 2–4 folded. Moving forward from line 2 should jump to line 5.
        let p = HostFoldProvider::new(vec![(ln(2), ln(4))]);
        assert_eq!(p.next_visible_line(ln(2), Direction::Forward), ln(5));
        assert_eq!(p.next_visible_line(ln(3), Direction::Forward), ln(5));
        assert_eq!(p.next_visible_line(ln(4), Direction::Forward), ln(5));
    }

    #[test]
    fn next_visible_backward() {
        // Lines 2–4 folded. Moving backward from inside the fold should jump to line 1.
        let p = HostFoldProvider::new(vec![(ln(2), ln(4))]);
        assert_eq!(p.next_visible_line(ln(2), Direction::Backward), ln(1));
        assert_eq!(p.next_visible_line(ln(3), Direction::Backward), ln(1));
        assert_eq!(p.next_visible_line(ln(4), Direction::Backward), ln(1));
    }

    #[test]
    fn visible_unchanged() {
        // Lines 2–4 folded. Visible lines are returned unchanged.
        let p = HostFoldProvider::new(vec![(ln(2), ln(4))]);
        assert_eq!(p.next_visible_line(ln(0), Direction::Forward), ln(0));
        assert_eq!(p.next_visible_line(ln(1), Direction::Forward), ln(1));
        assert_eq!(p.next_visible_line(ln(5), Direction::Backward), ln(5));
    }

    #[test]
    fn multiple_folds() {
        // Two separate folds: lines 2–3 and lines 7–9.
        let p = HostFoldProvider::new(vec![(ln(2), ln(3)), (ln(7), ln(9))]);
        assert!(!p.is_folded(ln(1)));
        assert!(p.is_folded(ln(2)));
        assert!(p.is_folded(ln(3)));
        assert!(!p.is_folded(ln(4)));
        assert!(p.is_folded(ln(7)));
        assert!(p.is_folded(ln(9)));
        assert!(!p.is_folded(ln(10)));
        assert_eq!(p.next_visible_line(ln(2), Direction::Forward), ln(4));
        assert_eq!(p.next_visible_line(ln(8), Direction::Forward), ln(10));
        assert_eq!(p.next_visible_line(ln(9), Direction::Backward), ln(6));
    }

    #[test]
    fn single_line_fold() {
        // A fold that covers exactly one line.
        let p = HostFoldProvider::new(vec![(ln(5), ln(5))]);
        assert!(p.is_folded(ln(5)));
        assert!(!p.is_folded(ln(4)));
        assert!(!p.is_folded(ln(6)));
        assert_eq!(p.next_visible_line(ln(5), Direction::Forward), ln(6));
        assert_eq!(p.next_visible_line(ln(5), Direction::Backward), ln(4));
    }

    #[test]
    fn backward_saturate() {
        // Fold starts at line 0. Moving backward should saturate at 0 (not underflow).
        let p = HostFoldProvider::new(vec![(ln(0), ln(2))]);
        assert_eq!(p.next_visible_line(ln(0), Direction::Backward), ln(0));
        assert_eq!(p.next_visible_line(ln(1), Direction::Backward), ln(0));
    }

    #[test]
    fn enclosing_fold_returns_range() {
        let p = HostFoldProvider::new(vec![(ln(2), ln(4))]);
        assert_eq!(p.enclosing_fold(ln(3)), Some((ln(2), ln(4))));
        assert_eq!(p.enclosing_fold(ln(2)), Some((ln(2), ln(4))));
        assert_eq!(p.enclosing_fold(ln(4)), Some((ln(2), ln(4))));
        assert_eq!(p.enclosing_fold(ln(1)), None);
        assert_eq!(p.enclosing_fold(ln(5)), None);
    }

    #[test]
    fn enclosing_fold_at_line_zero() {
        let p = HostFoldProvider::new(vec![(ln(0), ln(2))]);
        assert_eq!(p.enclosing_fold(ln(0)), Some((ln(0), ln(2))));
        assert_eq!(p.enclosing_fold(ln(1)), Some((ln(0), ln(2))));
    }
}
