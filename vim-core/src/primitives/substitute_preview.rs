//! Substitute preview match type for live `:s` preview (inccommand).
//!
//! Represents what one regex match in a `:s` command would produce:
//! the range of the matched text and what it would be replaced with.

use crate::primitives::Offset;
use compact_str::CompactString;

/// A single match in a substitute preview.
///
/// Represents what one regex match in a `:s` command would produce:
/// the range of the matched text and what it would be replaced with.
///
/// Used by [`Effect::SubstitutePreview`](crate::effects::Effect::SubstitutePreview)
/// to communicate preview data to the host for live highlighting.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SubstitutePreviewMatch {
    /// Start of the matched text (document gap offset).
    match_start: Offset,
    /// End of the matched text (document gap offset).
    match_end: Offset,
    /// The replacement text that would be produced.
    replacement: CompactString,
    /// Original line text before substitution (for split-preview diff).
    ///
    /// Populated when `inccommand=split` so the host can render a before/after
    /// diff in a preview window. `None` when `inccommand=nosplit`.
    original_line: Option<CompactString>,
}

impl SubstitutePreviewMatch {
    /// Create a new substitute preview match.
    #[inline]
    #[must_use]
    pub const fn new(match_start: Offset, match_end: Offset, replacement: CompactString) -> Self {
        Self {
            match_start,
            match_end,
            replacement,
            original_line: None,
        }
    }

    /// Create a new substitute preview match with original line text.
    #[inline]
    #[must_use]
    pub const fn with_original_line(
        match_start: Offset,
        match_end: Offset,
        replacement: CompactString,
        original_line: CompactString,
    ) -> Self {
        Self {
            match_start,
            match_end,
            replacement,
            original_line: Some(original_line),
        }
    }

    /// Start of the matched text (document gap offset).
    #[inline]
    #[must_use]
    pub const fn match_start(&self) -> Offset {
        self.match_start
    }

    /// End of the matched text (document gap offset).
    #[inline]
    #[must_use]
    pub const fn match_end(&self) -> Offset {
        self.match_end
    }

    /// The replacement text that would be produced.
    #[inline]
    #[must_use]
    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    /// Original line text before substitution (for split-preview diff).
    ///
    /// Returns `Some` when the preview was generated with `inccommand=split`,
    /// `None` for `inccommand=nosplit`.
    #[inline]
    #[must_use]
    pub fn original_line(&self) -> Option<&str> {
        self.original_line.as_deref()
    }
}
