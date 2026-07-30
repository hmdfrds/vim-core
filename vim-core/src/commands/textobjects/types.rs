//! Text object result types.
//!
//! Type-safe representation of text object computation results.

use crate::primitives::{Offset, Range, SubwordConfig, VimOptions, WordCharSet};

// ═══════════════════════════════════════════════════════════════════════════════
// Context (consistent with MotionContext, OperatorContext)
// ═══════════════════════════════════════════════════════════════════════════════

/// Context for text object computation.
///
/// Contains all information a text object might need.
/// Follows the same pattern as `MotionContext` for dispatch consistency.
///
/// # Design
///
/// All dispatchers take a context struct:
/// - `dispatch_motion(motion, &MotionContext)`
/// - `dispatch_operator(op, &OperatorContext)`
/// - `dispatch_textobject(object, &TextObjectContext)`
#[derive(Debug, Clone)]
pub struct TextObjectContext<'text> {
    /// Full document text.
    pub text: &'text str,
    /// Current cursor byte offset.
    pub cursor: Offset,
    /// Shell providers (custom text objects, etc.).
    pub providers: crate::document::Providers<'text>,
    /// Word character set for word boundary classification.
    pub word_chars: &'text WordCharSet,
    /// Subword boundary detection configuration.
    pub subword_config: &'text SubwordConfig,
    /// VimText tree for O(log n) queries via B+ tree summaries.
    pub tree: Option<&'text vim_text::VimText>,
    /// Characters used to escape the quote character in quote text objects.
    /// Defaults to `"\\"` (single backslash). Set from `VimOptions::quoteescape()`.
    pub quoteescape: &'text str,
}

/// Static default word char set for contexts constructed without options.
static DEFAULT_WORD_CHAR_SET: std::sync::LazyLock<WordCharSet> =
    std::sync::LazyLock::new(WordCharSet::default_vim);

/// Static default subword config for contexts constructed without options.
static DEFAULT_SUBWORD_CONFIG: std::sync::LazyLock<SubwordConfig> =
    std::sync::LazyLock::new(SubwordConfig::default);

impl<'text> TextObjectContext<'text> {
    /// Create a new text object context with default word character set.
    #[inline]
    #[must_use]
    pub fn new(text: &'text str, cursor: usize) -> Self {
        Self {
            text,
            cursor: Offset::new(cursor),
            providers: crate::document::Providers::new(),
            word_chars: &DEFAULT_WORD_CHAR_SET,
            subword_config: &DEFAULT_SUBWORD_CONFIG,
            tree: None,
            quoteescape: "\\",
        }
    }

    /// Set capability providers (custom text objects, etc.).
    #[inline]
    #[must_use]
    pub const fn with_providers(mut self, providers: crate::document::Providers<'text>) -> Self {
        self.providers = providers;
        self
    }

    /// Set the word character set, subword config, and quoteescape from VimOptions.
    #[inline]
    #[must_use]
    pub fn with_options(mut self, options: &'text VimOptions) -> Self {
        self.word_chars = options.word_char_set();
        self.subword_config = options.subword_config();
        self.quoteescape = options.quoteescape();
        self
    }

    /// Set VimText tree for O(log n) summary queries.
    #[must_use]
    pub const fn with_tree(mut self, tree: &'text vim_text::VimText) -> Self {
        self.tree = Some(tree);
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Result Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of computing a text object.
///
/// # Invariants
///
/// - `range.start() <= range.end()`
/// - If `linewise`, the range should span complete lines
///
/// # Example
///
/// ```ignore
/// let result = compute_word_object(text, cursor, inner, big);
/// match result {
///     Some(TextObjectRange { range, linewise: false }) => {
///         // Characterwise operation
///     }
///     Some(TextObjectRange { range, linewise: true }) => {
///         // Linewise operation (e.g., paragraph)
///     }
///     None => {
///         // No valid text object found
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextObjectRange {
    /// The range of text selected by this text object.
    pub range: Range,
    /// Whether this text object is linewise (affects paste behavior).
    /// Paragraphs are linewise, words are not.
    pub linewise: bool,
}

impl TextObjectRange {
    /// Create a characterwise text object range.
    #[inline]
    #[must_use]
    pub const fn char(start: usize, end: usize) -> Self {
        Self {
            range: Range::from_raw(start, end),
            linewise: false,
        }
    }

    /// Create a linewise text object range.
    #[inline]
    #[must_use]
    pub const fn line(start: usize, end: usize) -> Self {
        Self {
            range: Range::from_raw(start, end),
            linewise: true,
        }
    }

    /// Create from an existing Range (characterwise).
    #[inline]
    #[must_use]
    pub const fn from_range(range: Range) -> Self {
        Self {
            range,
            linewise: false,
        }
    }

    /// Create from an existing Range (linewise).
    #[inline]
    #[must_use]
    pub const fn from_range_linewise(range: Range) -> Self {
        Self {
            range,
            linewise: true,
        }
    }

    /// Get the start offset.
    #[inline]
    #[must_use]
    pub const fn start(&self) -> usize {
        self.range.start().get()
    }

    /// Get the end offset.
    #[inline]
    #[must_use]
    pub const fn end(&self) -> usize {
        self.range.end().get()
    }

    /// Check if the range is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.range.is_empty()
    }

    /// Get the length of the range.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.range.len()
    }
}
/// Types of bracket pairs supported by text objects.
///
/// These correspond to Vim's `i(`, `a{`, `i[`, `i<` text objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BracketType {
    /// Parentheses `()` - used for `i(`, `a(`, `i)`, `a)`.
    Paren,
    /// Curly braces `{}` - used for `i{`, `a{`, `i}`, `a}`, `iB`, `aB`.
    Brace,
    /// Square brackets `[]` - used for `i[`, `a[`, `i]`, `a]`.
    Bracket,
    /// Angle brackets `<>` - used for `i<`, `a<`, `i>`, `a>`.
    Angle,
}

impl BracketType {
    /// Get the open and close characters.
    #[must_use]
    pub const fn chars(self) -> (char, char) {
        match self {
            Self::Paren => ('(', ')'),
            Self::Brace => ('{', '}'),
            Self::Bracket => ('[', ']'),
            Self::Angle => ('<', '>'),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_range() {
        let r = TextObjectRange::char(0, 5);
        assert_eq!(r.start(), 0);
        assert_eq!(r.end(), 5);
        assert!(!r.linewise);
        assert_eq!(r.len(), 5);
    }

    #[test]
    fn test_line_range() {
        let r = TextObjectRange::line(0, 10);
        assert!(r.linewise);
    }

    #[test]
    fn test_empty() {
        let r = TextObjectRange::char(5, 5);
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn test_from_range() {
        let range = Range::from_raw(3, 7);
        let r = TextObjectRange::from_range(range);
        assert_eq!(r.start(), 3);
        assert_eq!(r.end(), 7);
        assert!(!r.linewise);
    }
}
