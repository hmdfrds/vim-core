//! Linewise text with guaranteed trailing newline.
//!
//! A newtype wrapper that encodes its invariant at the type level, so callers
//! cannot construct a value that violates it.
//!
//! # Invariant
//!
//! `LinewiseText` ALWAYS ends with `\n`. This is enforced at construction.
//! Any text stored in a linewise register has this property.
//!
//! # Why This Exists
//!
//! When deleting last line at EOF, we delete the PRECEDING newline but the
//! register should still contain `line\n` (trailing newline for linewise paste).
//! Without this type, we'd need string surgery at every usage point.

use derive_more::{AsRef, Display};
use std::borrow::Cow;

/// Text guaranteed to have linewise semantics (trailing newline).
///
/// # Invariant
///
/// The contained string ALWAYS ends with `\n`. This is enforced at construction
/// and cannot be violated.
///
/// # Examples
///
/// ```rust
/// use vim_core::primitives::LinewiseText;
///
/// // Basic construction - adds trailing newline if missing
/// let lt = LinewiseText::new("hello");
/// assert_eq!(lt.as_str(), "hello\n");
///
/// // Preserves existing trailing newline
/// let lt = LinewiseText::new("hello\n");
/// assert_eq!(lt.as_str(), "hello\n");
///
/// // EOF edge case: leading newline becomes trailing
/// let lt = LinewiseText::new("\nhello");
/// assert_eq!(lt.as_str(), "hello\n");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef)]
#[display(fmt = "{_0}")]
#[as_ref(forward)]
pub struct LinewiseText(String);

impl LinewiseText {
    /// Create from text, normalizing to guarantee trailing newline.
    ///
    /// # Normalization Rules
    ///
    /// 1. Leading `\n` is stripped (EOF edge case: we deleted preceding newline)
    /// 2. Trailing `\n` is added if missing
    /// 3. Empty input becomes `\n`
    ///
    /// # Examples
    ///
    /// | Input | Output |
    /// |-------|--------|
    /// | `"hello"` | `"hello\n"` |
    /// | `"hello\n"` | `"hello\n"` |
    /// | `"\nhello"` | `"hello\n"` |
    /// | `"\nhello\n"` | `"hello\n"` |
    /// | `""` | `"\n"` |
    #[inline]
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let mut s = text.into();

        // Strip leading newline (EOF edge case)
        if s.starts_with('\n') {
            s.drain(..1);
        }

        // Ensure trailing newline
        if !s.ends_with('\n') {
            s.push('\n');
        }

        Self(s)
    }

    /// Create from text that is KNOWN to already have trailing newline.
    ///
    /// # Precondition
    ///
    /// Callers MUST ensure the input ends with `\n`.
    /// Debug builds will panic if this invariant is violated.
    ///
    /// Use `new()` instead unless you have profiled and this is a hot path.
    #[inline]
    #[must_use]
    pub fn from_normalized(text: String) -> Self {
        debug_assert!(
            text.ends_with('\n'),
            "LinewiseText::from_normalized called with text not ending in newline: {text:?}"
        );
        // Release-mode safety: enforce invariant even if debug_assert didn't fire
        let mut text = text;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        Self(text)
    }

    /// The guaranteed-normalized content.
    ///
    /// Always ends with `\n`.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to owned String.
    #[inline]
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    /// Length in bytes (always >= 1 due to trailing newline).
    #[allow(
        clippy::len_without_is_empty,
        reason = "LinewiseText is never empty — invariant guarantees trailing newline"
    )]
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Content without trailing newline.
    ///
    /// Useful for display or comparison with non-linewise text.
    #[inline]
    #[must_use]
    pub fn content(&self) -> &str {
        &self.0[..self.0.len() - 1]
    }

    /// Returns whether this is an empty line (just `\n`).
    #[inline]
    #[must_use]
    pub const fn is_empty_line(&self) -> bool {
        self.0.len() == 1
    }
}

impl From<&str> for LinewiseText {
    #[inline]
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for LinewiseText {
    #[inline]
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<Cow<'_, str>> for LinewiseText {
    #[inline]
    fn from(s: Cow<'_, str>) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adds_trailing_newline() {
        let lt = LinewiseText::new("hello");
        assert_eq!(lt.as_str(), "hello\n");
    }

    #[test]
    fn test_preserves_trailing_newline() {
        let lt = LinewiseText::new("hello\n");
        assert_eq!(lt.as_str(), "hello\n");
    }

    #[test]
    fn test_strips_leading_newline() {
        let lt = LinewiseText::new("\nhello");
        assert_eq!(lt.as_str(), "hello\n");
    }

    #[test]
    fn test_both_leading_and_trailing() {
        let lt = LinewiseText::new("\nhello\n");
        assert_eq!(lt.as_str(), "hello\n");
    }

    #[test]
    fn test_empty_becomes_newline() {
        let lt = LinewiseText::new("");
        assert_eq!(lt.as_str(), "\n");
        assert!(lt.is_empty_line());
    }

    #[test]
    fn test_content_without_newline() {
        let lt = LinewiseText::new("hello");
        assert_eq!(lt.content(), "hello");
    }

    #[test]
    fn test_from_string() {
        let lt: LinewiseText = "test".into();
        assert_eq!(lt.as_str(), "test\n");
    }

    #[test]
    fn test_multiline() {
        let lt = LinewiseText::new("line1\nline2");
        assert_eq!(lt.as_str(), "line1\nline2\n");
    }

    #[test]
    fn test_only_newline() {
        let lt = LinewiseText::new("\n");
        assert_eq!(lt.as_str(), "\n");
    }

    #[test]
    #[should_panic(expected = "not ending in newline")]
    fn test_from_normalized_panics_on_invalid() {
        let _ = LinewiseText::from_normalized("no newline".to_string());
    }

    // ─────────────────────────────────────────────────────────────────────
    // Mutation-resistant tests: each covers a mutant that survived
    // cargo-mutants, so a silently-wrong change here fails a test.
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn test_len_returns_correct_length() {
        // Catches: replace len() -> 0 and len() -> 1
        let lt = LinewiseText::new("hello");
        assert_eq!(lt.len(), 6); // "hello\n" = 6 bytes
        assert!(
            lt.len() > 1,
            "len must be greater than 1 for non-empty content"
        );

        let lt2 = LinewiseText::new("");
        assert_eq!(lt2.len(), 1); // just "\n" = 1 byte
    }

    #[test]
    fn test_is_empty_line_accurate() {
        // Catches: replace is_empty_line() -> true
        let lt = LinewiseText::new("hello");
        assert!(!lt.is_empty_line(), "'hello\\n' is not an empty line");

        let lt2 = LinewiseText::new("");
        assert!(lt2.is_empty_line(), "'\\n' is an empty line");

        let lt3 = LinewiseText::new("a");
        assert!(!lt3.is_empty_line(), "'a\\n' is not an empty line");
    }
}
