// ═══════════════════════════════════════════════════════════════════════════════
// RESOLVER TRAITS
// ═══════════════════════════════════════════════════════════════════════════════

/// Resolves byte offsets to line/column positions.
///
/// Implementations are provided by the host editor. All line/column
/// values are 1-indexed to match Vim conventions.
pub trait LineResolver {
    /// Returns the 1-indexed line number for a byte offset.
    fn byte_to_line(&self, offset: usize) -> u32;
    /// Returns the 1-indexed byte column for a byte offset.
    fn byte_to_col(&self, offset: usize) -> u32;
    /// Returns the 1-indexed virtual column (tabs expanded) for a byte offset.
    fn byte_to_vcol(&self, offset: usize) -> u32;
    /// Returns the cursor's current line number (for `\%.l`).
    fn cursor_line(&self) -> u32;
    /// Returns the byte range `(start, end)` of the given 1-indexed line.
    fn line_byte_range(&self, line: u32) -> Option<(usize, usize)>;
}

/// Resolves mark characters to byte positions.
pub trait MarkResolver {
    /// Returns the byte position of the given mark, or `None` if unset.
    fn mark_position(&self, mark: char) -> Option<usize>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// SINGLE LINE RESOLVER
// ═══════════════════════════════════════════════════════════════════════════════

/// A `LineResolver` for per-line contexts (`:s`, `:g`).
///
/// Maps any byte offset within the single-line text to a known absolute
/// line number (1-indexed). Column is computed as `offset + 1` (byte column,
/// matching Vim's `\%c` semantics).
///
/// This enables `\%l`, `\%c`, and `\%v` atoms to work correctly in per-line
/// substitute/global operations where the regex engine only sees one line at
/// a time but buffer-position atoms reference absolute document positions.
///
/// # Example
///
/// ```ignore
/// use vim_regex::{MatchContext, SingleLineResolver};
///
/// let resolver = SingleLineResolver::new(3, 11); // line 3, text length 11
/// let ctx = MatchContext::builder("hello world")
///     .line_resolver(&resolver)
///     .build();
/// ```
pub struct SingleLineResolver {
    /// The 1-indexed absolute line number this text belongs to.
    line: u32,
    /// Length of the text (for `line_byte_range` reporting).
    text_len: usize,
}

impl SingleLineResolver {
    /// Create a new resolver for a specific absolute line number.
    ///
    /// - `line`: 1-indexed absolute line number in the buffer.
    /// - `text_len`: byte length of the line's text (for `line_byte_range`).
    #[must_use]
    pub const fn new(line: u32, text_len: usize) -> Self {
        Self { line, text_len }
    }
}

impl LineResolver for SingleLineResolver {
    /// Any offset within this single-line text maps to the stored absolute line.
    #[inline]
    fn byte_to_line(&self, _offset: usize) -> u32 {
        self.line
    }

    /// Returns 1-indexed byte column: `offset + 1`.
    #[inline]
    fn byte_to_col(&self, offset: usize) -> u32 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "line text length << u32::MAX"
        )]
        {
            (offset + 1) as u32
        }
    }

    /// Returns 1-indexed virtual column (same as byte column for single lines
    /// without tab expansion).
    #[inline]
    fn byte_to_vcol(&self, offset: usize) -> u32 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "line text length << u32::MAX"
        )]
        {
            (offset + 1) as u32
        }
    }

    /// Returns the stored line number as the cursor's line.
    #[inline]
    fn cursor_line(&self) -> u32 {
        self.line
    }

    /// Returns the byte range `(0, text_len)` if `line` matches the stored
    /// line number, `None` otherwise.
    #[inline]
    fn line_byte_range(&self, line: u32) -> Option<(usize, usize)> {
        if line == self.line {
            Some((0, self.text_len))
        } else {
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MATCH CONTEXT
// ═══════════════════════════════════════════════════════════════════════════════

/// Runtime context provided to matchers during regex execution.
///
/// Carries the text being searched, cursor state, and optional resolvers
/// for buffer-position atoms (`\%l`, `\%c`, `\%v`, `\%'m`).
///
/// Construct using [`MatchContext::simple`], [`MatchContext::with_cursor`],
/// or [`MatchContext::builder`].
#[non_exhaustive]
pub struct MatchContext<'a> {
    /// The full text being searched.
    pub(crate) text: &'a str,
    /// Current cursor byte offset (for `\%#`).
    /// `None` means no cursor was set — `\%#` will never match.
    pub(crate) cursor: Option<usize>,
    /// Visual selection range as `(start, end)` byte offsets (for `\%V`).
    pub(crate) visual_range: Option<(usize, usize)>,
    /// Whether matching is case-sensitive.
    pub(crate) case_sensitive: bool,
    /// Whether to ignore composing (combining) characters during matching (`\Z`).
    pub(crate) ignore_composing: bool,
    /// Optional line/column resolver for buffer-position atoms.
    pub(crate) line_resolver: Option<&'a dyn LineResolver>,
    /// Optional mark resolver for `\%'m` atoms.
    pub(crate) mark_resolver: Option<&'a dyn MarkResolver>,
    /// Last substitute string (for `~`).
    pub(crate) last_substitute: Option<&'a str>,
}

impl std::fmt::Debug for MatchContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatchContext")
            .field("text_len", &self.text.len())
            .field("cursor", &self.cursor)
            .field("visual_range", &self.visual_range)
            .field("case_sensitive", &self.case_sensitive)
            .field("ignore_composing", &self.ignore_composing)
            .field("has_line_resolver", &self.line_resolver.is_some())
            .field("has_mark_resolver", &self.mark_resolver.is_some())
            .field("has_last_substitute", &self.last_substitute.is_some())
            .finish_non_exhaustive()
    }
}

#[allow(
    clippy::elidable_lifetime_names,
    reason = "explicit lifetime binds text to context"
)]
impl<'a> MatchContext<'a> {
    /// Creates a minimal context for simple text matching.
    #[must_use]
    pub fn simple(text: &'a str) -> Self {
        Self {
            text,
            cursor: None,
            visual_range: None,
            case_sensitive: true,
            ignore_composing: false,
            line_resolver: None,
            mark_resolver: None,
            last_substitute: None,
        }
    }

    /// Creates a context with cursor position set.
    #[must_use]
    pub fn with_cursor(text: &'a str, cursor: usize) -> Self {
        Self {
            text,
            cursor: Some(cursor),
            visual_range: None,
            case_sensitive: true,
            ignore_composing: false,
            line_resolver: None,
            mark_resolver: None,
            last_substitute: None,
        }
    }

    /// Creates a `MatchContextBuilder` for fluent construction.
    #[must_use]
    pub fn builder(text: &'a str) -> MatchContextBuilder<'a> {
        MatchContextBuilder {
            text,
            cursor: None,
            line_resolver: None,
            mark_resolver: None,
            visual_range: None,
            case_sensitive: true,
            ignore_composing: false,
            last_substitute: None,
        }
    }

    /// Returns the full text being searched.
    #[must_use]
    #[inline]
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// Returns the cursor byte offset, if set.
    #[must_use]
    #[inline]
    pub const fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// Returns the visual selection range, if set.
    #[must_use]
    #[inline]
    pub const fn visual_range(&self) -> Option<(usize, usize)> {
        self.visual_range
    }

    /// Returns whether matching is case-sensitive.
    #[must_use]
    #[inline]
    pub const fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Returns whether composing characters are ignored.
    #[must_use]
    #[inline]
    pub const fn ignore_composing(&self) -> bool {
        self.ignore_composing
    }

    /// Returns the line/column resolver, if set.
    #[must_use]
    #[inline]
    pub fn line_resolver(&self) -> Option<&'a dyn LineResolver> {
        self.line_resolver
    }

    /// Returns the mark resolver, if set.
    #[must_use]
    #[inline]
    pub fn mark_resolver(&self) -> Option<&'a dyn MarkResolver> {
        self.mark_resolver
    }

    /// Returns the last substitute string, if set.
    #[must_use]
    #[inline]
    pub fn last_substitute(&self) -> Option<&'a str> {
        self.last_substitute
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MATCH CONTEXT BUILDER
// ═══════════════════════════════════════════════════════════════════════════════

/// Builder for constructing `MatchContext` with optional fields.
///
/// Created via `MatchContext::builder(text)`. Provides a fluent API for
/// setting cursor, resolvers, visual range, and case override.
pub struct MatchContextBuilder<'a> {
    text: &'a str,
    cursor: Option<usize>,
    line_resolver: Option<&'a dyn LineResolver>,
    mark_resolver: Option<&'a dyn MarkResolver>,
    visual_range: Option<(usize, usize)>,
    case_sensitive: bool,
    ignore_composing: bool,
    last_substitute: Option<&'a str>,
}

impl<'a> MatchContextBuilder<'a> {
    /// Set the cursor byte offset (for `\%#`).
    #[must_use]
    pub fn cursor(mut self, pos: usize) -> Self {
        self.cursor = Some(pos);
        self
    }

    /// Set the line/column resolver for buffer-position atoms.
    #[must_use]
    pub fn line_resolver(mut self, r: &'a dyn LineResolver) -> Self {
        self.line_resolver = Some(r);
        self
    }

    /// Set the mark resolver for `\%'m` atoms.
    #[must_use]
    pub fn mark_resolver(mut self, r: &'a dyn MarkResolver) -> Self {
        self.mark_resolver = Some(r);
        self
    }

    /// Set the visual selection range as `(start, end)` byte offsets (for `\%V`).
    #[must_use]
    pub fn visual_range(mut self, start: usize, end: usize) -> Self {
        self.visual_range = Some((start, end));
        self
    }

    /// Set case sensitivity (overrides the default `true`).
    #[must_use]
    pub fn case_sensitive(mut self, sensitive: bool) -> Self {
        self.case_sensitive = sensitive;
        self
    }

    /// Set the last substitute string (for `~`).
    #[must_use]
    pub fn last_substitute(mut self, sub: &'a str) -> Self {
        self.last_substitute = Some(sub);
        self
    }

    /// Set whether to ignore composing (combining) characters (`\Z`).
    #[must_use]
    pub fn ignore_composing(mut self, ignore: bool) -> Self {
        self.ignore_composing = ignore;
        self
    }

    /// Build the `MatchContext`.
    #[must_use]
    pub fn build(self) -> MatchContext<'a> {
        MatchContext {
            text: self.text,
            cursor: self.cursor,
            visual_range: self.visual_range,
            case_sensitive: self.case_sensitive,
            ignore_composing: self.ignore_composing,
            line_resolver: self.line_resolver,
            mark_resolver: self.mark_resolver,
            last_substitute: self.last_substitute,
        }
    }
}
