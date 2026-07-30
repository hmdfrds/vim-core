//! Matcher types for the Vim regex engine.
//!
//! Contains `CharMatcher` (consuming matchers), `ZeroWidthMatcher`
//! (zero-width assertions), `LookaroundMatcher`, and the `MatchContext`
//! that provides document state for buffer-position atoms.

mod char_match;
mod context;
mod zero_width;

// Re-export all public/crate-visible types at the matchers:: level
// so existing `use crate::matchers::*` imports work unchanged.
pub(crate) use char_match::{
    class_matches, class_matches_ascii, collection_item_matches, posix_class_matches, CharMatcher,
};
pub use context::{
    LineResolver, MarkResolver, MatchContext, MatchContextBuilder, SingleLineResolver,
};
pub(crate) use zero_width::{skip_combining_marks, ZeroWidthMatcher};

use crate::ir::LookaroundKind;
use crate::nfa::SubNfaId;

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND MATCHER
// ═══════════════════════════════════════════════════════════════════════════════

/// Lookaround/atomic matcher stored in the NFA side-table.
///
/// Moved out of `TransitionKind` to shrink `Transition` from ~36 to ~12 bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LookaroundMatcher {
    /// Index of the sub-NFA in the `sub_nfas` side table.
    pub(crate) sub_nfa_id: SubNfaId,
    /// The kind of assertion.
    pub(crate) kind: LookaroundKind,
    /// Optional character limit for lookbehind.
    pub(crate) limit: Option<u32>,
    /// Minimum consuming characters from target state to accept.
    pub(crate) rest_min_len: usize,
    /// Whether to defer this lookbehind to accept time (PIM).
    pub(crate) defer_check: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// UNIFIED MATCHER (NFA SIDE TABLE)
// ═══════════════════════════════════════════════════════════════════════════════

/// Unified matcher for NFA side table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Matcher {
    /// A character-consuming matcher.
    Char(CharMatcher),
    /// A zero-width assertion matcher.
    ZeroWidth(ZeroWidthMatcher),
    /// A lookaround/atomic matcher.
    Lookaround(LookaroundMatcher),
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEST MOCK RESOLVERS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
pub(crate) struct MockLineResolver {
    positions: Vec<(usize, u32, u32, u32)>,
    cursor_line_val: u32,
}

#[cfg(test)]
impl MockLineResolver {
    pub(crate) fn new(positions: Vec<(usize, u32, u32, u32)>, cursor_line_val: u32) -> Self {
        Self {
            positions,
            cursor_line_val,
        }
    }

    fn find_entry(&self, offset: usize) -> (u32, u32, u32) {
        for &(off, line, col, vcol) in &self.positions {
            if off == offset {
                return (line, col, vcol);
            }
        }
        (1, 1, 1)
    }
}

#[cfg(test)]
impl LineResolver for MockLineResolver {
    fn byte_to_line(&self, offset: usize) -> u32 {
        self.find_entry(offset).0
    }
    fn byte_to_col(&self, offset: usize) -> u32 {
        self.find_entry(offset).1
    }
    fn byte_to_vcol(&self, offset: usize) -> u32 {
        self.find_entry(offset).2
    }
    fn cursor_line(&self) -> u32 {
        self.cursor_line_val
    }
    fn line_byte_range(&self, _line: u32) -> Option<(usize, usize)> {
        None
    }
}

#[cfg(test)]
pub(crate) struct MockMarkResolver {
    marks: Vec<(char, usize)>,
}

#[cfg(test)]
impl MockMarkResolver {
    pub(crate) fn new(marks: Vec<(char, usize)>) -> Self {
        Self { marks }
    }
}

#[cfg(test)]
impl MarkResolver for MockMarkResolver {
    fn mark_position(&self, mark: char) -> Option<usize> {
        self.marks
            .iter()
            .find(|(m, _)| *m == mark)
            .map(|(_, pos)| *pos)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "../tests/matchers/mod.rs"]
mod tests;
