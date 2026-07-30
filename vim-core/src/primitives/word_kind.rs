//! Word kind type for vim-core.
//!
//! Distinguishes Vim's `word` (alphanumeric + underscore, respects punctuation)
//! from `WORD` (any non-whitespace sequence).

/// Word classification kind for motions and text objects.
///
/// Replaces `big_word: bool` / `big: bool` throughout the codebase with
/// a self-documenting type.
///
/// - `Word` — vim's `word`: alphanumeric + `_`, with punctuation as boundaries
/// - `WORD` — vim's `WORD`: any non-whitespace sequence
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum WordKind {
    /// Vim `word`: alphanumeric + underscore. Punctuation creates boundaries.
    Word,
    /// Vim `WORD`: any non-whitespace. Only whitespace creates boundaries.
    #[allow(non_camel_case_types, reason = "Matches Vim terminology")]
    WORD,
}

impl WordKind {
    /// Whether this is the WORD (big word) variant.
    #[inline]
    #[must_use]
    pub const fn is_big(self) -> bool {
        matches!(self, Self::WORD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_big() {
        assert!(!WordKind::Word.is_big());
        assert!(WordKind::WORD.is_big());
    }
}
