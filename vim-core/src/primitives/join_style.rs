//! Join style for line-joining operations.

/// How to join lines — with or without spaces.
///
/// `J` joins with spaces (trimming leading whitespace from the next line).
/// `gJ` joins without spaces (raw concatenation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum JoinStyle {
    /// Join with a space between lines (`J` command).
    WithSpace,
    /// Join without adding space (`gJ` command).
    NoSpace,
}

impl JoinStyle {
    /// Whether this style adds spaces between joined lines.
    #[inline]
    #[must_use]
    pub const fn adds_space(self) -> bool {
        matches!(self, Self::WithSpace)
    }
}
