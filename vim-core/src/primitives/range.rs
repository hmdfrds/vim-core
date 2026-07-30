//! Range types for vim-core.
//!
//! Ranges represent spans of text with inclusive start and exclusive end.

use crate::primitives::{LineNumber, Offset};
use derive_more::Display;

/// A byte range in a document.
///
/// Uses exclusive end: `start..end` where `end` is not included.
/// Invariant: `start <= end` (enforced at construction).
///
/// Both `start` and `end` are [`Offset`] — gap-indexed positions that point
/// *between* characters, making half-open `[start, end)` semantics natural.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Display)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[display(fmt = "{}..{}", "start.get()", "end.get()")]
pub struct Range {
    /// Start gap offset (inclusive).
    start: Offset,
    /// End gap offset (exclusive).
    end: Offset,
}

impl Range {
    /// Create a new range.
    ///
    /// In debug builds, asserts `start <= end` to catch logic bugs.
    /// In release builds, normalizes by swapping — recovers gracefully
    /// instead of panicking.
    #[inline]
    #[must_use]
    pub const fn new(start: Offset, end: Offset) -> Self {
        debug_assert!(
            start.get() <= end.get(),
            "Range::new called with start > end"
        );
        if start.get() <= end.get() {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    /// Create from raw values.
    #[inline]
    #[must_use]
    pub const fn from_raw(start: usize, end: usize) -> Self {
        Self::new(Offset::new(start), Offset::new(end))
    }

    /// Empty range at offset 0.
    pub const EMPTY: Self = Self {
        start: Offset::ZERO,
        end: Offset::ZERO,
    };

    /// Empty range at gap offset.
    #[inline]
    #[must_use]
    pub const fn empty_at(offset: Offset) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// Start gap offset (inclusive).
    #[inline]
    #[must_use]
    pub const fn start(self) -> Offset {
        self.start
    }

    /// End gap offset (exclusive).
    #[inline]
    #[must_use]
    pub const fn end(self) -> Offset {
        self.end
    }

    /// Length in bytes.
    #[inline]
    #[must_use]
    pub const fn len(self) -> usize {
        debug_assert!(
            self.start.get() <= self.end.get(),
            "Range invariant violated: start > end"
        );
        self.end.get().saturating_sub(self.start.get())
    }

    /// Check if empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.get() == self.end.get()
    }

    /// Check if gap offset is within range.
    #[inline]
    #[must_use]
    pub const fn contains(self, offset: Offset) -> bool {
        self.start.get() <= offset.get() && offset.get() < self.end.get()
    }

    /// Check if ranges overlap.
    ///
    /// An empty range cannot overlap with any range (including itself).
    #[inline]
    #[must_use]
    #[allow(
        clippy::suspicious_operation_groupings,
        reason = "half-open interval overlap: self.start < other.end && other.start < self.end \
                  is the standard algorithm; the asymmetry is intentional"
    )]
    pub const fn overlaps(self, other: Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.start.get() < other.end.get()
            && other.start.get() < self.end.get()
    }

    /// Returns `true` if the given offset falls within this half-open range [start, end).
    ///
    /// An empty range contains nothing.
    #[must_use]
    pub const fn contains_offset(self, offset: Offset) -> bool {
        self.start.get() <= offset.get() && offset.get() < self.end.get()
    }

    /// Slice a string using this range.
    ///
    /// Clamps to text length and adjusts to char boundaries for safety.
    #[inline]
    #[must_use]
    pub fn slice(self, text: &str) -> &str {
        let mut start = self.start.get().min(text.len());
        let mut end = self.end.get().min(text.len());
        // Adjust to valid char boundaries
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        // After boundary adjustment, start may have advanced past end
        if start > end {
            return "";
        }
        &text[start..end]
    }

    /// Return a new range with a different end offset.
    ///
    /// Normalizes by swapping if the new end is before the current start,
    /// matching `Range::new` behavior.
    #[inline]
    #[must_use]
    pub const fn with_end(self, end: Offset) -> Self {
        debug_assert!(
            self.start.get() <= end.get(),
            "Range::with_end would create start > end"
        );
        if end.get() < self.start.get() {
            Self {
                start: end,
                end: self.start,
            }
        } else {
            Self {
                start: self.start,
                end,
            }
        }
    }

    /// Return a new range with a different start offset.
    ///
    /// Normalizes by swapping if the new start is after the current end,
    /// matching `Range::new` behavior.
    #[inline]
    #[must_use]
    pub const fn with_start(self, start: Offset) -> Self {
        debug_assert!(
            start.get() <= self.end.get(),
            "Range::with_start would create start > end"
        );
        if start.get() > self.end.get() {
            Self {
                start: self.end,
                end: start,
            }
        } else {
            Self {
                start,
                end: self.end,
            }
        }
    }

    /// Clamp the range end to a maximum value.
    ///
    /// Returns a new range where `end = min(self.end, max)` and
    /// `start` is also clamped to not exceed the new end.
    #[inline]
    #[must_use]
    pub const fn clamp_end(self, max: Offset) -> Self {
        let new_end = self.end.min(max);
        let new_start = self.start.min(new_end);
        Self {
            start: new_start,
            end: new_end,
        }
    }

    /// Convert to `std::ops::Range`.
    #[inline]
    #[must_use]
    pub const fn to_std_range(self) -> std::ops::Range<usize> {
        self.start.get()..self.end.get()
    }
}

impl From<std::ops::Range<usize>> for Range {
    fn from(range: std::ops::Range<usize>) -> Self {
        Self::from_raw(range.start, range.end)
    }
}

impl From<Range> for std::ops::Range<usize> {
    fn from(range: Range) -> Self {
        range.start.get()..range.end.get()
    }
}

/// A line range in a document.
///
/// Uses inclusive end for line ranges.
/// Invariant: `start <= end` (enforced at construction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineRange {
    /// Start line (inclusive).
    start: LineNumber,
    /// End line (inclusive).
    end: LineNumber,
}

impl LineRange {
    /// Create a new line range.
    ///
    /// Normalizes by swapping if `start > end` (matches `Range::new` behavior).
    #[inline]
    #[must_use]
    pub const fn new(start: LineNumber, end: LineNumber) -> Self {
        debug_assert!(start.get() <= end.get(), "LineRange start must be <= end");
        if start.get() <= end.get() {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    /// Single line range.
    #[inline]
    #[must_use]
    pub const fn single(line: LineNumber) -> Self {
        Self {
            start: line,
            end: line,
        }
    }

    /// Start line (inclusive).
    #[inline]
    #[must_use]
    pub const fn start(self) -> LineNumber {
        self.start
    }

    /// End line (inclusive).
    #[inline]
    #[must_use]
    pub const fn end(self) -> LineNumber {
        self.end
    }

    /// Number of lines (always >= 1).
    #[inline]
    #[allow(
        clippy::len_without_is_empty,
        reason = "LineRange is never empty — invariant guarantees start <= end"
    )]
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.get() - self.start.get() + 1
    }

    /// Check if single line.
    #[inline]
    #[must_use]
    pub const fn is_single(self) -> bool {
        self.start.get() == self.end.get()
    }

    /// Check if line is within range.
    #[inline]
    #[must_use]
    pub const fn contains(self, line: LineNumber) -> bool {
        self.start.get() <= line.get() && line.get() <= self.end.get()
    }
}

impl std::fmt::Display for LineRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_single() {
            write!(f, "{}", self.start.get() + 1) // 1-indexed for display
        } else {
            write!(f, "{},{}", self.start.get() + 1, self.end.get() + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Range ===

    #[test]
    fn range_from_raw() {
        let r = Range::from_raw(5, 10);
        assert_eq!(r.start(), Offset::new(5));
        assert_eq!(r.end(), Offset::new(10));
    }

    #[test]
    fn range_len() {
        assert_eq!(Range::from_raw(3, 8).len(), 5);
    }

    #[test]
    fn range_empty_at() {
        let r = Range::empty_at(Offset::new(7));
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert_eq!(r.start(), r.end());
    }

    #[test]
    fn range_contains() {
        let r = Range::from_raw(5, 10);
        assert!(r.contains(Offset::new(5))); // inclusive start
        assert!(r.contains(Offset::new(9)));
        assert!(!r.contains(Offset::new(10))); // exclusive end
        assert!(!r.contains(Offset::new(4)));
    }

    #[test]
    fn range_overlaps() {
        let a = Range::from_raw(0, 5);
        let b = Range::from_raw(3, 8);
        assert!(a.overlaps(b));
        assert!(b.overlaps(a));
    }

    #[test]
    fn range_no_overlap_touching() {
        let a = Range::from_raw(0, 5);
        let b = Range::from_raw(5, 10);
        assert!(!a.overlaps(b));
    }

    #[test]
    fn range_empty_does_not_overlap_containing_range() {
        // [5,5) ∩ [3,7) = ∅ — empty ranges cannot overlap anything.
        let empty = Range::empty_at(Offset::new(5));
        let containing = Range::from_raw(3, 7);
        assert!(!empty.overlaps(containing));
        assert!(!containing.overlaps(empty));
    }

    #[test]
    fn range_empty_does_not_overlap_empty() {
        let a = Range::empty_at(Offset::new(5));
        let b = Range::empty_at(Offset::new(5));
        assert!(!a.overlaps(b));
    }

    #[test]
    fn range_contains_offset() {
        let r = Range::from_raw(5, 10);
        assert!(r.contains_offset(Offset::new(5))); // inclusive start
        assert!(r.contains_offset(Offset::new(9)));
        assert!(!r.contains_offset(Offset::new(10))); // exclusive end
        assert!(!r.contains_offset(Offset::new(4)));
    }

    #[test]
    fn range_empty_contains_offset_nothing() {
        let empty = Range::empty_at(Offset::new(5));
        // An empty range contains no offsets, not even its own position.
        assert!(!empty.contains_offset(Offset::new(5)));
        assert!(!empty.contains_offset(Offset::new(4)));
        assert!(!empty.contains_offset(Offset::new(6)));
    }

    #[test]
    fn range_to_std_range() {
        assert_eq!(Range::from_raw(3, 7).to_std_range(), 3..7);
    }

    #[test]
    fn range_from_std_range() {
        let r: Range = (3..7).into();
        assert_eq!(r, Range::from_raw(3, 7));
    }

    #[test]
    fn range_into_std_range() {
        let std_range: std::ops::Range<usize> = Range::from_raw(3, 7).into();
        assert_eq!(std_range, 3..7);
    }

    // === LineRange ===

    #[test]
    fn line_range_new() {
        let lr = LineRange::new(LineNumber::new(2), LineNumber::new(5));
        assert_eq!(lr.start(), LineNumber::new(2));
        assert_eq!(lr.end(), LineNumber::new(5));
    }

    #[test]
    fn line_range_single() {
        let lr = LineRange::single(LineNumber::new(3));
        assert!(lr.is_single());
        assert_eq!(lr.len(), 1);
    }

    #[test]
    fn line_range_len_inclusive() {
        let lr = LineRange::new(LineNumber::new(2), LineNumber::new(5));
        assert_eq!(lr.len(), 4); // inclusive: 2, 3, 4, 5
    }

    #[test]
    fn line_range_contains() {
        let lr = LineRange::new(LineNumber::new(2), LineNumber::new(5));
        assert!(lr.contains(LineNumber::new(2)));
        assert!(lr.contains(LineNumber::new(5)));
        assert!(!lr.contains(LineNumber::new(1)));
        assert!(!lr.contains(LineNumber::new(6)));
    }
}
