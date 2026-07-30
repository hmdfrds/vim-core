//! Search provider trait for shell integration.
//!
//! Shells implement this trait to provide regex-powered search to the engine.
//! The engine's built-in search uses simple substring matching; this trait
//! allows the shell to upgrade search to full Vim-compatible regex patterns.

use crate::primitives::{Direction, Range, SearchFlags};

/// Provider for regex-powered search.
///
/// Shells implement this to expose their search capabilities.
/// The engine uses this for `n`/`N`/`gn`/`gN` search motions when
/// a `SearchProvider` is available.
///
/// If no `SearchProvider` is supplied, the engine falls back to
/// built-in substring matching via `str::match_indices()`.
pub trait SearchProvider {
    /// Find the next match for `pattern` starting from byte offset `from`.
    ///
    /// - `pattern`: the search pattern (may contain Vim regex syntax,
    ///   already stripped of `\c`/`\C`/`\v`/`\V` modifiers)
    /// - `from`: byte offset to start searching from
    /// - `direction`: search direction (`Forward` = toward end, `Backward` = toward start)
    /// - `flags`: search behavior flags (case sensitivity, wrapping, magic mode)
    ///
    /// Returns a [`Range`] with `start..end` byte offsets, or `None` if no match.
    fn find_match(
        &self,
        pattern: &str,
        from: usize,
        direction: Direction,
        flags: &SearchFlags,
    ) -> Option<Range>;

    /// Collect all non-overlapping matches for `pattern` across the document.
    ///
    /// Shells with a faster native API may override this directly. The default
    /// implementation repeatedly calls `find_match` without wrapping.
    fn find_matches(&self, pattern: &str, text_len: usize) -> Vec<Range> {
        let no_wrap = SearchFlags::new().with_wrap(false);
        let mut matches = Vec::new();
        let mut from = 0usize;

        while from <= text_len {
            let Some(range) = self.find_match(pattern, from, Direction::Forward, &no_wrap) else {
                break;
            };
            let start = range.start().get();
            let end = range.end().get();
            if start >= text_len || end < start || start < from {
                break;
            }

            let clamped_end = end.min(text_len);
            matches.push(Range::from_raw(start, clamped_end));
            from = if clamped_end > start {
                clamped_end
            } else {
                start.saturating_add(1)
            };
        }

        matches
    }
}
