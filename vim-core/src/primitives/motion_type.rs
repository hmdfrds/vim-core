//! Motion type for vim-core.
//!
//! Describes how a range should be treated by operators.

/// How a text range should be handled.
///
/// This affects how operators like delete and yank process the selected text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, strum::EnumCount, strum::EnumIter)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MotionType {
    /// Character-wise motion (default).
    /// Only the exact characters are affected.
    #[default]
    CharWise,
    /// Line-wise motion.
    /// Entire lines are affected, including trailing newline.
    LineWise,
    /// Block-wise motion.
    /// A rectangular block of text is affected.
    BlockWise,
}

impl MotionType {
    /// Check if character-wise.
    #[inline]
    #[must_use]
    pub const fn is_char_wise(self) -> bool {
        matches!(self, Self::CharWise)
    }

    /// Check if line-wise.
    #[inline]
    #[must_use]
    pub const fn is_line_wise(self) -> bool {
        matches!(self, Self::LineWise)
    }

    /// Check if block-wise.
    #[inline]
    #[must_use]
    pub const fn is_block_wise(self) -> bool {
        matches!(self, Self::BlockWise)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_charwise() {
        assert_eq!(MotionType::default(), MotionType::CharWise);
    }

    #[test]
    fn is_char_wise() {
        assert!(MotionType::CharWise.is_char_wise());
        assert!(!MotionType::LineWise.is_char_wise());
        assert!(!MotionType::BlockWise.is_char_wise());
    }

    #[test]
    fn is_line_wise() {
        assert!(MotionType::LineWise.is_line_wise());
        assert!(!MotionType::CharWise.is_line_wise());
        assert!(!MotionType::BlockWise.is_line_wise());
    }

    #[test]
    fn is_block_wise() {
        assert!(MotionType::BlockWise.is_block_wise());
        assert!(!MotionType::CharWise.is_block_wise());
        assert!(!MotionType::LineWise.is_block_wise());
    }

    #[test]
    fn each_variant_matches_exactly_one_predicate() {
        for mt in [
            MotionType::CharWise,
            MotionType::LineWise,
            MotionType::BlockWise,
        ] {
            let count = [mt.is_char_wise(), mt.is_line_wise(), mt.is_block_wise()]
                .iter()
                .filter(|&&b| b)
                .count();
            assert_eq!(count, 1, "{:?} should match exactly one predicate", mt);
        }
    }
}
