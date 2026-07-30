//! Ex command types.
//!
//! Context and result types for Ex command execution.

use crate::commands::helpers;
use crate::effects::Effects;
use crate::primitives::{MarkName, Offset, SubFlags};

/// Mark resolver callback used by Ex range resolution.
pub type MarkResolver<'text> = dyn Fn(MarkName) -> Option<usize> + 'text;

/// Context for executing Ex commands.
///
/// Provides read-only access to document state needed for Ex commands.
#[derive(Clone, Copy)]
pub struct ExContext<'text> {
    /// Buffer text.
    pub text: &'text str,
    /// Current cursor line (0-indexed).
    pub cursor_line: usize,
    /// Total number of lines in buffer.
    pub total_lines: usize,
    /// Optional callback to resolve mark names to byte offsets.
    pub mark_resolver: Option<&'text MarkResolver<'text>>,
    /// Last substitute replacement string (for `~` in patterns and replacements).
    pub last_substitute: Option<&'text str>,
    /// Whether `:s` defaults to global replacement (from `gdefault` option).
    pub gdefault: bool,
    /// Whether `g` and `c` flags on `:s` are sticky and toggle (from `edcompatible` option).
    pub edcompatible: bool,
    /// Flags from the most recent `:s` command (for `:&&` repeat-with-flags).
    ///
    /// `None` when no substitute command has been run yet.
    pub last_substitute_flags: Option<SubFlags>,
    /// The last substitute PATTERN from `:s/pattern/` (Neovim's `RE_SUBST`).
    ///
    /// Unlike `resolved_substitute_pattern` (which follows RE_LAST and may return
    /// the search pattern), this field always reflects the last `:s` pattern
    /// specifically.  Used by `:&` / `:&&` to repeat the exact last substitute.
    pub last_substitute_pattern: Option<&'text str>,
    /// Resolved substitute pattern for empty-pattern `:s` (Neovim `RE_LAST` resolution).
    ///
    /// When `:s//rep/` is used, this provides the pattern resolved by the
    /// two-pattern system: if the last pattern came from `:s`, reuse the
    /// substitute pattern; if from `/`, reuse the search pattern.
    pub resolved_substitute_pattern: Option<&'text str>,
    /// The last search pattern from `/`/`?` (Neovim's `RE_SEARCH`).
    ///
    /// Used by the `:s///r` flag to override the substitute pattern with the
    /// most recent `/` search pattern.
    pub last_search_pattern: Option<&'text str>,
    /// VimText tree for O(log n) queries via B+ tree summaries.
    pub tree: Option<&'text vim_text::VimText>,
}

impl<'text> ExContext<'text> {
    /// Create a new ExContext.
    #[inline]
    #[must_use]
    pub fn new(text: &'text str, cursor_line: usize) -> Self {
        let total_lines = helpers::line_count(text).max(1);
        Self {
            text,
            cursor_line,
            total_lines,
            mark_resolver: None,
            last_substitute: None,
            gdefault: false,
            edcompatible: false,
            last_substitute_flags: None,
            resolved_substitute_pattern: None,
            last_substitute_pattern: None,
            last_search_pattern: None,
            tree: None,
        }
    }

    /// Attach a mark resolver callback.
    #[inline]
    #[must_use]
    pub fn with_mark_resolver(mut self, resolver: &'text MarkResolver<'text>) -> Self {
        self.mark_resolver = Some(resolver);
        self
    }

    /// Attach the last substitute replacement string.
    #[inline]
    #[must_use]
    pub const fn with_last_substitute(mut self, last_sub: Option<&'text str>) -> Self {
        self.last_substitute = last_sub;
        self
    }

    /// Set the `gdefault` option value.
    #[inline]
    #[must_use]
    pub const fn with_gdefault(mut self, gdefault: bool) -> Self {
        self.gdefault = gdefault;
        self
    }

    /// Set the `edcompatible` option value.
    #[inline]
    #[must_use]
    pub const fn with_edcompatible(mut self, edcompatible: bool) -> Self {
        self.edcompatible = edcompatible;
        self
    }

    /// Set the last substitute flags (for `:&&`).
    #[inline]
    #[must_use]
    pub const fn with_last_substitute_flags(mut self, flags: Option<SubFlags>) -> Self {
        self.last_substitute_flags = flags;
        self
    }

    /// Set the last substitute pattern (for `:&` / `:&&`).
    ///
    /// This is the raw substitute pattern from `RE_SUBST`, not the RE_LAST-resolved
    /// version that `resolved_substitute_pattern` provides.
    #[inline]
    #[must_use]
    pub const fn with_last_substitute_pattern(mut self, pattern: Option<&'text str>) -> Self {
        self.last_substitute_pattern = pattern;
        self
    }

    /// Set the resolved substitute pattern (from Neovim `RE_LAST` resolution).
    #[inline]
    #[must_use]
    pub const fn with_resolved_substitute_pattern(mut self, pattern: Option<&'text str>) -> Self {
        self.resolved_substitute_pattern = pattern;
        self
    }

    /// Set the last search pattern (from `/`/`?`, Neovim's `RE_SEARCH`).
    ///
    /// Used by the `:s///r` flag.
    #[inline]
    #[must_use]
    pub const fn with_last_search_pattern(mut self, pattern: Option<&'text str>) -> Self {
        self.last_search_pattern = pattern;
        self
    }

    /// Set VimText tree for O(log n) summary queries.
    #[must_use]
    pub const fn with_tree(mut self, tree: &'text vim_text::VimText) -> Self {
        self.tree = Some(tree);
        self
    }

    /// Get byte offset for start of a line (0-indexed).
    pub fn line_start_offset(&self, line: usize) -> Option<Offset> {
        if line >= self.total_lines {
            return None;
        }
        helpers::line_start(self.text, line).map(Offset::new)
    }

    /// Get byte offset range for a line (0-indexed).
    /// Returns (start, end) where end is after the newline if present.
    #[must_use]
    pub fn line_range(&self, line: usize) -> Option<(Offset, Offset)> {
        let start = helpers::line_start(self.text, line)?;
        let end = helpers::line_end(self.text, line)?;

        // Include newline if not the last line
        let end_with_newline = if line + 1 < self.total_lines {
            end + 1
        } else {
            end
        };

        Some((Offset::new(start), Offset::new(end_with_newline)))
    }

    /// Get byte offset range for multiple lines (0-indexed, inclusive).
    #[must_use]
    pub fn lines_range(&self, start_line: usize, end_line: usize) -> Option<(Offset, Offset)> {
        let (start, _) = self.line_range(start_line)?;
        let (_, end) = self.line_range(end_line)?;
        Some((start, end))
    }

    /// Get text for a line (0-indexed).
    #[must_use]
    pub fn line_text(&self, line: usize) -> Option<&str> {
        helpers::line_content(self.text, line)
    }
}

/// Result type for Ex commands.
///
/// Returns a collection of effects to apply, using the `Effects` builder pattern
/// consistent with all other command layers.
pub type ExResult = Result<Effects, crate::errors::VimError>;

/// Resolved line range (0-indexed, inclusive).
///
/// The invariant `start <= end` is enforced by the normalizing constructor:
/// if `start > end`, the values are swapped. This guarantees that
/// `line_count()` can never underflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRange {
    /// Start line (0-indexed).
    start: usize,
    /// End line (0-indexed, inclusive).
    end: usize,
}

impl ResolvedRange {
    /// Create a new resolved range, normalizing so `start <= end`.
    ///
    /// If `start > end`, the values are swapped to maintain the invariant.
    #[inline]
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    /// Start line (0-indexed).
    #[inline]
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// End line (0-indexed, inclusive).
    #[inline]
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// Return a new range with `end` extended by `n` lines.
    #[inline]
    #[must_use]
    pub const fn extend_end(self, n: usize) -> Self {
        Self {
            start: self.start,
            end: self.end + n,
        }
    }

    /// Number of lines in range.
    #[inline]
    #[must_use]
    pub const fn line_count(&self) -> usize {
        // Invariant: start <= end, enforced by constructor.
        self.end - self.start + 1
    }

    /// Check if range is a single line.
    #[inline]
    #[must_use]
    pub const fn is_single_line(&self) -> bool {
        self.start == self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_range() {
        let ctx = ExContext::new("line1\nline2\nline3", 1);
        assert_eq!(ctx.total_lines, 3);

        let (start, end) = ctx.line_range(0).unwrap();
        assert_eq!(start, Offset::new(0));
        assert_eq!(end, Offset::new(6)); // "line1\n"

        let (start, end) = ctx.line_range(1).unwrap();
        assert_eq!(start, Offset::new(6));
        assert_eq!(end, Offset::new(12)); // "line2\n"
    }

    #[test]
    fn test_lines_range() {
        let ctx = ExContext::new("a\nb\nc", 0);
        let (start, end) = ctx.lines_range(0, 1).unwrap();
        assert_eq!(start, Offset::new(0));
        assert_eq!(end, Offset::new(4)); // "a\nb\n"
    }
}
