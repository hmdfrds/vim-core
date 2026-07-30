//! Text range for vim-core.
//!
//! A range with motion type information for operators.

use crate::primitives::{MotionType, Offset, Range};
use smart_default::SmartDefault;

/// Whether a text range end is inclusive or exclusive.
///
/// Replaces bare `bool` to make call sites self-documenting:
/// `TextRange::char_wise(range, RangeEnd::Inclusive)` instead of
/// `TextRange::char_wise(range, true)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum RangeEnd {
    /// End offset is included in the range.
    Inclusive,
    /// End offset is excluded from the range.
    #[default]
    Exclusive,
}

impl RangeEnd {
    /// Returns `true` if inclusive.
    #[inline]
    #[must_use]
    pub const fn is_inclusive(self) -> bool {
        matches!(self, Self::Inclusive)
    }
}

/// A text range with motion type information.
///
/// This is used by operators to know how to process the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SmartDefault)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextRange {
    /// Byte range.
    #[default(Range::default())]
    range: Range,
    /// How to treat this range.
    #[default(MotionType::CharWise)]
    motion_type: MotionType,
    /// Whether the end is inclusive or exclusive.
    #[default(RangeEnd::Exclusive)]
    range_end: RangeEnd,
}

impl TextRange {
    /// Create a new text range.
    #[inline]
    #[must_use]
    pub const fn new(range: Range, motion_type: MotionType, range_end: RangeEnd) -> Self {
        Self {
            range,
            motion_type,
            range_end,
        }
    }

    /// Create a character-wise range.
    #[inline]
    #[must_use]
    pub const fn char_wise(range: Range, range_end: RangeEnd) -> Self {
        Self::new(range, MotionType::CharWise, range_end)
    }

    /// Create a line-wise range.
    #[inline]
    #[must_use]
    pub const fn line_wise(range: Range) -> Self {
        Self::new(range, MotionType::LineWise, RangeEnd::Inclusive)
    }

    /// Create a block-wise range.
    #[inline]
    #[must_use]
    pub const fn block_wise(range: Range) -> Self {
        Self::new(range, MotionType::BlockWise, RangeEnd::Inclusive)
    }

    /// Get the byte range.
    #[inline]
    #[must_use]
    pub const fn range(self) -> Range {
        self.range
    }

    /// Get the motion type.
    #[inline]
    #[must_use]
    pub const fn motion_type(self) -> MotionType {
        self.motion_type
    }

    /// Check if the end is inclusive.
    #[inline]
    #[must_use]
    pub const fn inclusive(self) -> bool {
        self.range_end.is_inclusive()
    }

    /// Get the range end kind.
    #[inline]
    #[must_use]
    pub const fn range_end(self) -> RangeEnd {
        self.range_end
    }

    /// Return a copy with a different range end.
    #[inline]
    #[must_use]
    pub const fn with_range_end(self, range_end: RangeEnd) -> Self {
        Self { range_end, ..self }
    }

    /// Start gap offset.
    #[inline]
    #[must_use]
    pub const fn start(self) -> Offset {
        self.range.start()
    }

    /// End gap offset.
    #[inline]
    #[must_use]
    pub const fn end(self) -> Offset {
        self.range.end()
    }

    /// Length in bytes.
    #[inline]
    #[must_use]
    pub const fn len(self) -> usize {
        self.range.len()
    }

    /// Check if empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.range.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range_5_10() -> Range {
        Range::from_raw(5, 10)
    }

    #[test]
    fn char_wise_constructor() {
        let tr = TextRange::char_wise(range_5_10(), RangeEnd::Exclusive);
        assert_eq!(tr.motion_type(), MotionType::CharWise);
        assert!(!tr.inclusive());
    }

    #[test]
    fn line_wise_constructor() {
        let tr = TextRange::line_wise(range_5_10());
        assert_eq!(tr.motion_type(), MotionType::LineWise);
        assert!(tr.inclusive());
    }

    #[test]
    fn block_wise_constructor() {
        let tr = TextRange::block_wise(range_5_10());
        assert_eq!(tr.motion_type(), MotionType::BlockWise);
        assert!(tr.inclusive());
    }

    #[test]
    fn start_end_len() {
        let tr = TextRange::char_wise(range_5_10(), RangeEnd::Inclusive);
        assert_eq!(tr.start(), Offset::new(5));
        assert_eq!(tr.end(), Offset::new(10));
        assert_eq!(tr.len(), 5);
    }

    #[test]
    fn empty_range() {
        let tr = TextRange::char_wise(Range::empty_at(Offset::new(3)), RangeEnd::Exclusive);
        assert!(tr.is_empty());
        assert_eq!(tr.len(), 0);
    }

    #[test]
    fn default_is_empty_charwise() {
        let d = TextRange::default();
        assert!(d.is_empty());
        assert_eq!(d.motion_type(), MotionType::CharWise);
    }

    #[test]
    fn with_range_end() {
        let tr = TextRange::char_wise(range_5_10(), RangeEnd::Exclusive);
        assert!(!tr.inclusive());
        let tr = tr.with_range_end(RangeEnd::Inclusive);
        assert!(tr.inclusive());
    }
}
