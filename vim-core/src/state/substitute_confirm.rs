//! Substitute confirm state for interactive `:s///c`.
//!
//! Pure data container tracking the pending confirm session.
//! The engine stores this in `VimState` and consults it when
//! processing y/n/a/q/l keystrokes during confirm mode.

use crate::primitives::byte_delta;
use crate::primitives::{ConfirmMatchPayload, Offset, Range, SubFlags, SubstituteConfirmPayload};
use compact_str::CompactString;

/// A single match in the confirm queue.
#[derive(Debug, Clone, PartialEq)]
pub struct SubstituteConfirmMatch {
    /// Document byte range of the match (pre-replacement).
    pub range: Range,
    /// Line index (0-indexed) this match is on.
    pub line_idx: usize,
    /// Byte offset of the line start in the document.
    pub line_start: Offset,
    /// The full line text (pre-replacement).
    pub line_text: CompactString,
}

/// State for an active `:s///c` interactive confirm session.
///
/// Created when `:s/pat/rep/gc` is executed. The engine stores this
/// in `VimState` and advances through matches as the user presses
/// y/n/a/q/l keys.
#[derive(Debug, Clone, PartialEq)]
pub struct SubstituteConfirmState {
    /// All match ranges (document byte offsets, pre any replacements).
    matches: Vec<SubstituteConfirmMatch>,
    /// The replacement string.
    replacement: CompactString,
    /// The compiled pattern (string form for re-matching).
    pattern: CompactString,
    /// Flags from the original `:s` command.
    flags: SubFlags,
    /// Index of the current match being shown.
    current_index: usize,
    /// Cumulative byte offset shift from accepted replacements.
    /// Tracks how much the document has grown/shrunk so we can adjust
    /// subsequent match ranges.
    offset_shift: isize,
    /// Number of replacements accepted so far.
    accepted_count: usize,
    /// Number of distinct lines that had at least one replacement.
    lines_changed: usize,
    /// Whether `gdefault` was active when the session started.
    gdefault: bool,
}

impl SubstituteConfirmState {
    /// Create a new confirm session.
    #[must_use]
    pub const fn new(
        matches: Vec<SubstituteConfirmMatch>,
        replacement: CompactString,
        pattern: CompactString,
        flags: SubFlags,
        gdefault: bool,
    ) -> Self {
        Self {
            matches,
            replacement,
            pattern,
            flags,
            current_index: 0,
            offset_shift: 0,
            accepted_count: 0,
            lines_changed: 0,
            gdefault,
        }
    }

    /// Convert the current state back to a payload for re-emission as an effect.
    #[must_use]
    pub fn to_payload(&self) -> SubstituteConfirmPayload {
        SubstituteConfirmPayload {
            matches: self
                .matches
                .iter()
                .map(|m| ConfirmMatchPayload {
                    range: m.range,
                    line_idx: m.line_idx,
                    line_start: m.line_start,
                    line_text: m.line_text.clone(),
                })
                .collect(),
            replacement: self.replacement.clone(),
            pattern: self.pattern.clone(),
            flags: self.flags,
            gdefault: self.gdefault,
        }
    }

    /// The replacement string.
    #[inline]
    #[must_use]
    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    /// The pattern string.
    #[inline]
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The flags from the original command.
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> SubFlags {
        self.flags
    }

    /// Current match index (0-based).
    #[inline]
    #[must_use]
    pub const fn current_index(&self) -> usize {
        self.current_index
    }

    /// Total number of matches.
    #[inline]
    #[must_use]
    pub const fn total_matches(&self) -> usize {
        self.matches.len()
    }

    /// Whether all matches have been processed.
    #[inline]
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.current_index >= self.matches.len()
    }

    /// Get the current match (if any remain).
    #[must_use]
    pub fn current_match(&self) -> Option<&SubstituteConfirmMatch> {
        self.matches.get(self.current_index)
    }

    /// Get the adjusted range for the current match, accounting for
    /// cumulative offset shift from prior accepted replacements.
    #[must_use]
    pub fn current_adjusted_range(&self) -> Option<Range> {
        let m = self.matches.get(self.current_index)?;
        Some(adjust_range(m.range, self.offset_shift))
    }

    /// Advance to the next match without doing a replacement.
    pub const fn skip(&mut self) {
        self.current_index += 1;
    }

    /// Accept the current replacement and advance.
    ///
    /// Returns the adjusted range and replacement text to apply,
    /// or `None` if no matches remain.
    pub fn accept(&mut self) -> Option<(Range, CompactString)> {
        let m = self.matches.get(self.current_index)?;
        let adjusted = adjust_range(m.range, self.offset_shift);

        // Track line changes
        let is_new_line = if self.accepted_count == 0 {
            true
        } else {
            // Check if this match is on a different line than the previous accepted one
            self.current_index == 0
                || self
                    .matches
                    .get(self.current_index.wrapping_sub(1))
                    .is_none_or(|prev| prev.line_idx != m.line_idx)
        };
        if is_new_line {
            self.lines_changed += 1;
        }

        let match_len = m.range.end().get().saturating_sub(m.range.start().get());
        self.offset_shift += byte_delta::delta(self.replacement.len(), match_len);
        self.accepted_count += 1;
        self.current_index += 1;

        Some((adjusted, self.replacement.clone()))
    }

    /// Get all remaining matches from current_index onward,
    /// each adjusted for cumulative offset shift. Used by 'a' (all remaining).
    pub fn accept_all_remaining(&mut self) -> Vec<(Range, CompactString)> {
        let mut results = Vec::new();
        while !self.is_done() {
            if let Some(pair) = self.accept() {
                results.push(pair);
            }
        }
        results
    }

    /// Number of accepted replacements.
    #[inline]
    #[must_use]
    pub const fn accepted_count(&self) -> usize {
        self.accepted_count
    }

    /// Number of lines that had at least one replacement.
    #[inline]
    #[must_use]
    pub const fn lines_changed(&self) -> usize {
        self.lines_changed
    }
}

/// Adjust a range by a signed byte offset.
const fn adjust_range(range: Range, shift: isize) -> Range {
    let start = range.start().get().saturating_add_signed(shift);
    let end = range.end().get().saturating_add_signed(shift);
    Range::new(Offset::new(start), Offset::new(end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_match(start: usize, end: usize, line_idx: usize) -> SubstituteConfirmMatch {
        SubstituteConfirmMatch {
            range: Range::new(Offset::new(start), Offset::new(end)),
            line_idx,
            line_start: Offset::new(0),
            line_text: CompactString::new(""),
        }
    }

    #[test]
    fn test_basic_session() {
        let matches = vec![
            make_match(0, 3, 0),
            make_match(10, 13, 0),
            make_match(20, 23, 1),
        ];
        let state = SubstituteConfirmState::new(
            matches,
            "bar".into(),
            "foo".into(),
            SubFlags::default(),
            false,
        );

        assert_eq!(state.total_matches(), 3);
        assert_eq!(state.current_index(), 0);
        assert!(!state.is_done());
    }

    #[test]
    fn test_skip() {
        let matches = vec![make_match(0, 3, 0), make_match(10, 13, 0)];
        let mut state = SubstituteConfirmState::new(
            matches,
            "bar".into(),
            "foo".into(),
            SubFlags::default(),
            false,
        );

        state.skip();
        assert_eq!(state.current_index(), 1);
        assert_eq!(state.accepted_count(), 0);
    }

    #[test]
    fn test_accept_adjusts_offset() {
        let matches = vec![
            make_match(0, 3, 0),   // "foo" -> "ba" (shrink by 1)
            make_match(10, 13, 0), // should become (9, 12) after shift
        ];
        let mut state = SubstituteConfirmState::new(
            matches,
            "ba".into(),
            "foo".into(),
            SubFlags::default(),
            false,
        );

        // Accept first: range (0,3), replacement "ba" (len 2), match len 3, shift = -1
        let (range, rep) = state.accept().unwrap();
        assert_eq!(range.start().get(), 0);
        assert_eq!(range.end().get(), 3);
        assert_eq!(rep.as_str(), "ba");

        // Second match should be shifted by -1
        let (range2, _) = state.accept().unwrap();
        assert_eq!(range2.start().get(), 9);
        assert_eq!(range2.end().get(), 12);
    }

    #[test]
    fn test_accept_all_remaining() {
        let matches = vec![
            make_match(0, 3, 0),
            make_match(10, 13, 0),
            make_match(20, 23, 1),
        ];
        let mut state = SubstituteConfirmState::new(
            matches,
            "bar".into(),
            "foo".into(),
            SubFlags::default(),
            false,
        );

        state.skip(); // skip first
        let remaining = state.accept_all_remaining();
        assert_eq!(remaining.len(), 2);
        assert!(state.is_done());
        assert_eq!(state.accepted_count(), 2);
    }

    #[test]
    fn test_done_when_all_processed() {
        let matches = vec![make_match(0, 3, 0)];
        let mut state = SubstituteConfirmState::new(
            matches,
            "bar".into(),
            "foo".into(),
            SubFlags::default(),
            false,
        );

        state.skip();
        assert!(state.is_done());
    }
}
