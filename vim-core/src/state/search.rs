//! Search state.
//!
//! Tracks the current search pattern, direction, offset, and history
//! for `/`/`?`/`n`/`N` motions.
//!
//! # Layering
//!
//! State is a low-mid layer: pure data containers, no execution logic.
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`.
//!
//! ## Design
//!
//! Search is shell-delegated for pattern input (`/`, `?`), but search
//! state is tracked in the core so that `n`/`N` can be computed.
//!
//! The shell handles:
//! - Command-line input for `/pattern` and `?pattern`
//! - Regex compilation
//! - Setting the search state via `set_pattern()`
//!
//! The core handles:
//! - `n` (next match) and `N` (previous match) motions
//! - `*` and `#` (word under cursor) by building patterns
//! - Search offsets (`/pat/e`, `/pat/+2`)
//! - Search history ring (Up/Down in `/`/`?` prompt)
//!
//! ```text
//! Shell: /{pattern}/{offset}<Enter>
//!    └── parses pattern + offset, calls state.search_mut().set_pattern(...)
//!
//! Core: n motion
//!    └── reads state.search().pattern() → finds next match → applies offset
//! ```

use compact_str::CompactString;
use std::collections::VecDeque;

use crate::primitives::SubFlags;

use crate::primitives::SearchDirection;

/// Maximum number of search history entries.
const SEARCH_HISTORY_CAP: usize = 50;

/// Tracks which pattern store was most recently used (Neovim's `RE_LAST` pointer).
///
/// Neovim maintains two separate pattern registers:
/// - `RE_SEARCH`: the pattern from `/`/`?`/`*`/`#` searches
/// - `RE_SUBST`: the pattern from `:s/pattern/replacement/`
///
/// `RE_LAST` points to whichever was set most recently. When `:s` is invoked
/// with an empty pattern (`:%s//replacement/`), it resolves the pattern via
/// `RE_LAST`: if the last pattern came from `:s`, reuse the substitute pattern;
/// if it came from `/`, reuse the search pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum LastPatternKind {
    /// The most recent pattern came from `/`, `?`, `*`, `#`, or `n`/`N`.
    #[default]
    Search,
    /// The most recent pattern came from `:s/pattern/`.
    Substitute,
}

/// Post-match cursor offset for search commands.
///
/// Vim supports offsets like `/pattern/e` (end of match), `/pattern/+3`
/// (3 lines below match), etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SearchOffset {
    /// `/pat/e` or `/pat/e+N` — cursor at end of match, optionally shifted.
    End(i32),
    /// `/pat/s+N` or `/pat/b+N` — cursor at start of match + offset chars.
    Start(i32),
    /// `/pat/+N` or `/pat/-N` — cursor N lines below/above the match line.
    Lines(i32),
}

impl SearchOffset {
    /// The default "no offset" value (start of match, 0 shift).
    pub const NONE: Self = Self::Start(0);

    /// Check if this is the default (no offset).
    #[inline]
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::Start(0))
    }
}

impl Default for SearchOffset {
    fn default() -> Self {
        Self::NONE
    }
}

/// Search state.
///
/// Tracks the current search pattern, direction, offset, and history.
///
/// ## Two-Pattern System (Neovim `RE_LAST` semantics)
///
/// Neovim keeps two separate pattern stores:
/// - **Search pattern** (`RE_SEARCH`): set by `/`, `?`, `*`, `#`
/// - **Substitute pattern** (`RE_SUBST`): set by `:s/pattern/`
///
/// The `last_used` field tracks which was set most recently (`RE_LAST`).
/// When `:s` is invoked with an empty pattern, `resolve_substitute_pattern()`
/// follows the `RE_LAST` pointer to pick the right pattern.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchState {
    /// Current search pattern (raw string, shell compiles to regex).
    pattern: Option<CompactString>,
    /// Current search direction.
    direction: SearchDirection,
    /// Post-match cursor offset (`/pat/e`, `/pat/+3`, etc.).
    offset: SearchOffset,
    /// Search history ring (most recent first).
    history: VecDeque<CompactString>,
    /// Current position in history navigation (0 = latest, None = not navigating).
    history_pos: Option<usize>,
    /// Last substitute replacement string (for `~` in patterns and replacements).
    last_substitute_string: Option<CompactString>,
    /// Flags from the most recent `:s` command (for `:&&` repeat-with-flags).
    ///
    /// `None` when no substitute command has been run yet.  Stores the full
    /// [`SubFlags`] struct, preserving all fields including `use_last_search`
    /// and `reuse_flags` that the old bitmask encoding silently dropped.
    last_substitute_flags: Option<SubFlags>,
    /// Last substitute PATTERN from `:s/pattern/replacement/` (Neovim's `RE_SUBST`).
    ///
    /// Distinct from `last_substitute_string` which stores the REPLACEMENT text
    /// (used for `~` expansion). This stores the search PATTERN used by `:s`.
    substitute_pattern: Option<CompactString>,
    /// Which pattern store was used most recently (Neovim's `RE_LAST` pointer).
    last_used: LastPatternKind,
}

impl SearchState {
    /// Create new empty search state.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there's an active search pattern.
    #[inline]
    #[must_use]
    pub const fn has_pattern(&self) -> bool {
        self.pattern.is_some()
    }

    /// Get the current search pattern.
    #[inline]
    #[must_use]
    pub fn pattern(&self) -> Option<&str> {
        self.pattern.as_deref()
    }

    /// Get the current search direction.
    #[inline]
    #[must_use]
    pub const fn direction(&self) -> SearchDirection {
        self.direction
    }

    /// Get the current search offset.
    #[inline]
    #[must_use]
    pub const fn offset(&self) -> SearchOffset {
        self.offset
    }

    /// Set the search pattern, direction, and offset.
    ///
    /// Called by shell after `/pattern/offset` is entered.
    /// Pushes the pattern to history automatically.
    /// Sets `last_used` to `Search` (Neovim's `RE_LAST = RE_SEARCH`).
    pub fn set_pattern_with_offset(
        &mut self,
        pattern: impl Into<CompactString>,
        direction: SearchDirection,
        offset: SearchOffset,
    ) {
        let pat = pattern.into();
        self.push_history(&pat);
        self.pattern = Some(pat);
        self.direction = direction;
        self.offset = offset;
        self.history_pos = None;
        self.last_used = LastPatternKind::Search;
    }

    /// Set the search pattern and direction (no offset).
    ///
    /// Convenience for `*`/`#` and other non-command-line searches.
    #[inline]
    pub fn set_pattern(&mut self, pattern: impl Into<CompactString>, direction: SearchDirection) {
        self.set_pattern_with_offset(pattern, direction, SearchOffset::NONE);
    }

    /// Clear the search pattern.
    #[inline]
    pub fn clear(&mut self) {
        self.pattern = None;
        self.offset = SearchOffset::NONE;
    }

    /// Get effective direction for `N` motion (opposite of current).
    #[inline]
    #[must_use]
    pub const fn prev_direction(&self) -> SearchDirection {
        self.direction.opposite()
    }

    // ── History ──────────────────────────────────────────────────────────

    /// Push a pattern to search history.
    ///
    /// Deduplicates: if pattern is already the most recent entry, skip.
    fn push_history(&mut self, pattern: &str) {
        if pattern.is_empty() {
            return;
        }
        // Remove duplicate if it exists anywhere in history.
        self.history.retain(|p| p.as_str() != pattern);
        self.history.push_front(CompactString::new(pattern));
        if self.history.len() > SEARCH_HISTORY_CAP {
            self.history.pop_back();
        }
    }

    /// Navigate to an older history entry (Up arrow in search prompt).
    ///
    /// Returns the pattern at the new position, or `None` if at the end.
    pub fn history_older(&mut self) -> Option<&str> {
        if self.history.is_empty() {
            return None;
        }
        let pos = match self.history_pos {
            Some(p) => (p + 1).min(self.history.len() - 1),
            None => 0,
        };
        self.history_pos = Some(pos);
        self.history.get(pos).map(CompactString::as_str)
    }

    /// Navigate to a newer history entry (Down arrow in search prompt).
    ///
    /// Returns the pattern at the new position, or `None` if past newest.
    pub fn history_newer(&mut self) -> Option<&str> {
        match self.history_pos {
            Some(0) | None => {
                self.history_pos = None;
                None
            }
            Some(p) => {
                let new_pos = p - 1;
                self.history_pos = Some(new_pos);
                self.history.get(new_pos).map(CompactString::as_str)
            }
        }
    }

    /// Reset history navigation position (when search prompt opens).
    #[inline]
    pub const fn reset_history_navigation(&mut self) {
        self.history_pos = None;
    }

    /// Get the full search history (most recent first).
    #[must_use]
    pub const fn history(&self) -> &VecDeque<CompactString> {
        &self.history
    }

    // ── Substitute pattern (RE_SUBST / RE_LAST) ──────────────────────────

    /// Get the last substitute pattern (from `:s/pattern/`).
    #[inline]
    #[must_use]
    pub fn substitute_pattern(&self) -> Option<&str> {
        self.substitute_pattern.as_deref()
    }

    /// Set the substitute pattern and mark `last_used = Substitute`.
    ///
    /// Called when `:s/pattern/replacement/` uses an explicit pattern.
    /// Sets Neovim's `RE_LAST = RE_SUBST`.
    pub fn set_substitute_pattern(&mut self, pattern: impl Into<CompactString>) {
        self.substitute_pattern = Some(pattern.into());
        self.last_used = LastPatternKind::Substitute;
    }

    /// Which pattern store was used most recently (Neovim's `RE_LAST`).
    #[inline]
    #[must_use]
    pub const fn last_used(&self) -> LastPatternKind {
        self.last_used
    }

    /// Resolve the pattern to use when `:s` is invoked with an empty pattern.
    ///
    /// Follows Neovim's `RE_LAST` pointer semantics:
    /// - If `last_used == Substitute`, return the substitute pattern
    /// - If `last_used == Search`, return the search pattern
    /// - If neither exists, return `None`
    #[must_use]
    pub fn resolve_substitute_pattern(&self) -> Option<&str> {
        match self.last_used {
            LastPatternKind::Substitute => self
                .substitute_pattern
                .as_deref()
                .or(self.pattern.as_deref()),
            LastPatternKind::Search => self
                .pattern
                .as_deref()
                .or(self.substitute_pattern.as_deref()),
        }
    }

    // ── Last substitute string ──────────────────────────────────────────

    /// Get the last substitute replacement string (for `~`).
    #[inline]
    #[must_use]
    pub fn last_substitute(&self) -> Option<&str> {
        self.last_substitute_string.as_deref()
    }

    /// Set the last substitute replacement string (for `~`).
    #[inline]
    pub fn set_last_substitute(&mut self, s: &str) {
        self.last_substitute_string = Some(CompactString::new(s));
    }

    /// Get the flags from the most recent `:s` command (for `:&&`).
    ///
    /// Returns `None` if no substitute command has been run yet.
    #[inline]
    #[must_use]
    pub const fn last_substitute_flags(&self) -> Option<SubFlags> {
        self.last_substitute_flags
    }

    /// Store the flags from a `:s` command.
    #[inline]
    pub const fn set_last_substitute_flags(&mut self, flags: SubFlags) {
        self.last_substitute_flags = Some(flags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_search_state_has_no_pattern() {
        let state = SearchState::new();
        assert!(!state.has_pattern());
        assert_eq!(state.pattern(), None);
    }

    #[test]
    fn set_pattern_works() {
        let mut state = SearchState::new();
        state.set_pattern("foo", SearchDirection::Forward);
        assert!(state.has_pattern());
        assert_eq!(state.pattern(), Some("foo"));
        assert_eq!(state.direction(), SearchDirection::Forward);
    }

    #[test]
    fn direction_opposite() {
        assert_eq!(
            SearchDirection::Forward.opposite(),
            SearchDirection::Backward
        );
        assert_eq!(
            SearchDirection::Backward.opposite(),
            SearchDirection::Forward
        );
    }

    #[test]
    fn direction_and_prev_direction() {
        let mut state = SearchState::new();
        state.set_pattern("test", SearchDirection::Forward);
        assert_eq!(state.direction(), SearchDirection::Forward);
        assert_eq!(state.prev_direction(), SearchDirection::Backward);

        state.set_pattern("test", SearchDirection::Backward);
        assert_eq!(state.direction(), SearchDirection::Backward);
        assert_eq!(state.prev_direction(), SearchDirection::Forward);
    }

    #[test]
    fn clear_removes_pattern() {
        let mut state = SearchState::new();
        state.set_pattern("foo", SearchDirection::Forward);
        assert!(state.has_pattern());

        state.clear();
        assert!(!state.has_pattern());
        assert_eq!(state.pattern(), None);
    }

    #[test]
    fn clear_preserves_direction() {
        let mut state = SearchState::new();
        state.set_pattern("foo", SearchDirection::Backward);

        state.clear();

        // Direction is preserved — `n` after clearing should still search backward
        // if the pattern is re-set without specifying direction
        assert_eq!(state.direction(), SearchDirection::Backward);
    }

    #[test]
    fn set_pattern_overwrites_previous() {
        let mut state = SearchState::new();
        state.set_pattern("first", SearchDirection::Forward);
        state.set_pattern("second", SearchDirection::Backward);

        assert_eq!(state.pattern(), Some("second"));
        assert_eq!(state.direction(), SearchDirection::Backward);
    }

    #[test]
    fn default_direction_is_forward() {
        assert_eq!(SearchDirection::default(), SearchDirection::Forward);
    }

    // ── Search offset tests ──────────────────────────────────────────

    #[test]
    fn default_offset_is_none() {
        let state = SearchState::new();
        assert!(state.offset().is_none());
    }

    #[test]
    fn set_pattern_with_offset() {
        let mut state = SearchState::new();
        state.set_pattern_with_offset("foo", SearchDirection::Forward, SearchOffset::End(0));
        assert_eq!(state.offset(), SearchOffset::End(0));
    }

    #[test]
    fn set_pattern_without_offset_clears_offset() {
        let mut state = SearchState::new();
        state.set_pattern_with_offset("foo", SearchDirection::Forward, SearchOffset::End(0));
        state.set_pattern("bar", SearchDirection::Forward);
        assert!(state.offset().is_none());
    }

    #[test]
    fn clear_resets_offset() {
        let mut state = SearchState::new();
        state.set_pattern_with_offset("foo", SearchDirection::Forward, SearchOffset::Lines(3));
        state.clear();
        assert!(state.offset().is_none());
    }

    // ── Search history tests ─────────────────────────────────────────

    #[test]
    fn history_starts_empty() {
        let state = SearchState::new();
        assert!(state.history().is_empty());
    }

    #[test]
    fn set_pattern_pushes_to_history() {
        let mut state = SearchState::new();
        state.set_pattern("foo", SearchDirection::Forward);
        state.set_pattern("bar", SearchDirection::Forward);
        assert_eq!(state.history().len(), 2);
        assert_eq!(
            state.history().front().map(CompactString::as_str),
            Some("bar")
        );
    }

    #[test]
    fn history_deduplicates() {
        let mut state = SearchState::new();
        state.set_pattern("foo", SearchDirection::Forward);
        state.set_pattern("bar", SearchDirection::Forward);
        state.set_pattern("foo", SearchDirection::Forward);
        // "foo" should only appear once (most recent position)
        assert_eq!(state.history().len(), 2);
        assert_eq!(
            state.history().front().map(CompactString::as_str),
            Some("foo")
        );
    }

    #[test]
    fn history_older_navigates_back() {
        let mut state = SearchState::new();
        state.set_pattern("first", SearchDirection::Forward);
        state.set_pattern("second", SearchDirection::Forward);
        state.set_pattern("third", SearchDirection::Forward);

        state.reset_history_navigation();
        assert_eq!(state.history_older(), Some("third"));
        assert_eq!(state.history_older(), Some("second"));
        assert_eq!(state.history_older(), Some("first"));
        // At end — stays at last entry
        assert_eq!(state.history_older(), Some("first"));
    }

    #[test]
    fn history_newer_navigates_forward() {
        let mut state = SearchState::new();
        state.set_pattern("first", SearchDirection::Forward);
        state.set_pattern("second", SearchDirection::Forward);

        state.reset_history_navigation();
        state.history_older(); // → "second"
        state.history_older(); // → "first"
        assert_eq!(state.history_newer(), Some("second"));
        assert_eq!(state.history_newer(), None); // past newest → None
    }

    #[test]
    fn empty_pattern_not_pushed_to_history() {
        let mut state = SearchState::new();
        state.set_pattern("", SearchDirection::Forward);
        assert!(state.history().is_empty());
    }

    #[test]
    fn history_cap_enforced() {
        let mut state = SearchState::new();
        for i in 0..60 {
            state.set_pattern(format!("pat{i}"), SearchDirection::Forward);
        }
        assert!(state.history().len() <= SEARCH_HISTORY_CAP);
    }

    // ── Two-pattern system (RE_LAST) tests ───────────────────────────

    #[test]
    fn default_last_used_is_search() {
        let state = SearchState::new();
        assert_eq!(state.last_used(), LastPatternKind::Search);
    }

    #[test]
    fn set_pattern_sets_last_used_to_search() {
        let mut state = SearchState::new();
        // First set substitute to change last_used
        state.set_substitute_pattern("sub_pat");
        assert_eq!(state.last_used(), LastPatternKind::Substitute);

        // Now set search pattern — should flip back to Search
        state.set_pattern("search_pat", SearchDirection::Forward);
        assert_eq!(state.last_used(), LastPatternKind::Search);
    }

    #[test]
    fn set_substitute_pattern_sets_last_used_to_substitute() {
        let mut state = SearchState::new();
        state.set_substitute_pattern("sub_pat");
        assert_eq!(state.last_used(), LastPatternKind::Substitute);
        assert_eq!(state.substitute_pattern(), Some("sub_pat"));
    }

    #[test]
    fn substitute_pattern_starts_empty() {
        let state = SearchState::new();
        assert_eq!(state.substitute_pattern(), None);
    }

    #[test]
    fn resolve_substitute_pattern_uses_search_when_last_used_is_search() {
        let mut state = SearchState::new();
        state.set_pattern("search_pat", SearchDirection::Forward);
        // last_used is Search, no substitute pattern → returns search pattern
        assert_eq!(state.resolve_substitute_pattern(), Some("search_pat"));
    }

    #[test]
    fn resolve_substitute_pattern_uses_substitute_when_last_used_is_substitute() {
        let mut state = SearchState::new();
        state.set_pattern("search_pat", SearchDirection::Forward);
        state.set_substitute_pattern("sub_pat");
        // last_used is Substitute → returns substitute pattern
        assert_eq!(state.resolve_substitute_pattern(), Some("sub_pat"));
    }

    #[test]
    fn resolve_substitute_pattern_falls_back_to_search_when_no_substitute() {
        let mut state = SearchState::new();
        state.set_pattern("search_pat", SearchDirection::Forward);
        // last_used is Search (default), no substitute pattern
        // Should return search pattern
        assert_eq!(state.resolve_substitute_pattern(), Some("search_pat"));
    }

    #[test]
    fn resolve_substitute_pattern_returns_none_when_empty() {
        let state = SearchState::new();
        assert_eq!(state.resolve_substitute_pattern(), None);
    }

    #[test]
    fn re_last_pointer_switches_correctly() {
        let mut state = SearchState::new();

        // 1. Search for "alpha"
        state.set_pattern("alpha", SearchDirection::Forward);
        assert_eq!(state.last_used(), LastPatternKind::Search);
        assert_eq!(state.resolve_substitute_pattern(), Some("alpha"));

        // 2. Substitute with "beta"
        state.set_substitute_pattern("beta");
        assert_eq!(state.last_used(), LastPatternKind::Substitute);
        assert_eq!(state.resolve_substitute_pattern(), Some("beta"));

        // 3. Search for "gamma" — RE_LAST flips back to Search
        state.set_pattern("gamma", SearchDirection::Forward);
        assert_eq!(state.last_used(), LastPatternKind::Search);
        assert_eq!(state.resolve_substitute_pattern(), Some("gamma"));

        // 4. Substitute pattern still preserved as "beta"
        assert_eq!(state.substitute_pattern(), Some("beta"));
    }

    #[test]
    fn search_does_not_update_substitute_pattern() {
        let mut state = SearchState::new();
        state.set_substitute_pattern("sub_original");
        state.set_pattern("new_search", SearchDirection::Forward);

        // Search should not modify substitute pattern
        assert_eq!(state.substitute_pattern(), Some("sub_original"));
        // But it should change last_used
        assert_eq!(state.last_used(), LastPatternKind::Search);
    }

    #[test]
    fn resolve_substitute_fallback_when_substitute_is_last_but_none() {
        let mut state = SearchState::new();
        // Set search pattern
        state.set_pattern("search_pat", SearchDirection::Forward);
        // Manually set last_used to Substitute without setting a substitute pattern.
        // This is a defensive edge case — in practice set_substitute_pattern() always
        // sets both. We test the fallback logic anyway.
        state.last_used = LastPatternKind::Substitute;
        // Should fall back to search pattern when substitute is None
        assert_eq!(state.resolve_substitute_pattern(), Some("search_pat"));
    }
}
