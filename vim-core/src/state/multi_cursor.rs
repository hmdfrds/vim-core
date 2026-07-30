//! Multi-cursor state management.
//!
//! Contains the state needed for multi-cursor editing: the selection set
//! and commands that manipulate cursors.
//!
//! # Architecture
//!
//! This is a pure data container in the `state` layer. It holds `Selections`
//! (from `primitives`) and the command enum for cursor manipulation.
//! No execution logic lives here.

use compact_str::CompactString;

use crate::primitives::{Direction, Offset, Selections};

/// State for multi-cursor editing.
///
/// Wraps the `Selections` type with multi-cursor-specific tracking.
/// Stored on `VimState` when the `multi-cursor` feature is enabled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultiCursorState {
    /// The current set of cursor selections.
    selections: Selections,

    /// Active `gb`/`gB` match session state.
    /// Set on first `gb` press, cleared on `ClearSecondary` (Escape).
    #[cfg_attr(feature = "serde", serde(default))]
    match_search: Option<MatchSearchState>,
}

impl MultiCursorState {
    /// Create a new multi-cursor state with the given selections.
    #[inline]
    #[must_use]
    pub const fn new(selections: Selections) -> Self {
        Self {
            selections,
            match_search: None,
        }
    }

    /// Get the current selections (read-only).
    #[inline]
    #[must_use]
    pub const fn selections(&self) -> &Selections {
        &self.selections
    }

    /// Get the current selections (mutable).
    #[inline]
    pub const fn selections_mut(&mut self) -> &mut Selections {
        &mut self.selections
    }

    /// Replace the selections entirely.
    #[inline]
    pub fn set_selections(&mut self, selections: Selections) {
        self.selections = selections;
    }

    /// Whether multi-cursor mode is active (more than one cursor).
    #[inline]
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.selections.len() > 1
    }

    /// Get the current match search state (read-only).
    #[inline]
    #[must_use]
    pub const fn match_search(&self) -> Option<&MatchSearchState> {
        self.match_search.as_ref()
    }

    /// Set the match search state.
    #[inline]
    pub fn set_match_search(&mut self, state: MatchSearchState) {
        self.match_search = Some(state);
    }

    /// Clear the match search state.
    #[inline]
    pub fn clear_match_search(&mut self) {
        self.match_search = None;
    }
}

/// Tracks the state of an active `gb`/`gB` match session.
/// Set on first `gb` press, cleared on `ClearSecondary` (Escape).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MatchSearchState {
    /// The locked search pattern (e.g., word or last search pattern).
    pub pattern: CompactString,
    /// Byte offset of the last match that was added or skipped.
    pub last_match_offset: usize,
    /// Whether the pattern requires whole-word matching (`\<...\>` boundaries).
    /// `true` when the pattern originated from word-under-cursor,
    /// `false` when it came from the search register (user's `/pattern`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub whole_word: bool,
}

/// Command vocabulary for multi-cursor operations.
///
/// Pure selection commands (`AddCursor`, `RemoveCursor`, `ClearSecondary`,
/// `RotatePrimary`) are dispatched by the executor directly.
///
/// Context-dependent commands (`AddCursorVertical`, `AddCursorsAtMatches`,
/// `SelectAllOccurrences`, `AddNextMatch`) require document/search context
/// and are dispatched through the host-assisted execution path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MultiCursorCommand {
    /// Add a cursor at a specific byte offset.
    AddCursor(Offset),
    /// Add cursors at all matches of the current search pattern.
    AddCursorsAtMatches,
    /// Add a cursor on the line above or below.
    AddCursorVertical(Direction),
    /// Remove the cursor at the given byte offset.
    RemoveCursor(Offset),
    /// Remove all cursors except the primary.
    ClearSecondary,
    /// Rotate the primary cursor designation (next or previous).
    RotatePrimary(Direction),
    /// Select all occurrences of the word under the primary cursor.
    SelectAllOccurrences,
    /// Find the next/previous match and either add a cursor there or skip it.
    /// Used by `gb` (add next), `gB` (add prev), `gs` (skip current + advance).
    AddNextMatch {
        /// Search direction: Forward for gb/gs, Backward for gB.
        direction: Direction,
        /// If true, skip the match without adding a cursor (gs behavior).
        skip: bool,
    },
    /// Convert a visual-block selection into individual cursors (one per line).
    CursorSplit,
    /// Create sub-selections at regex matches within each selection.
    SelectOnMatches {
        /// Regex pattern to match within each selection.
        pattern: String,
    },
    /// Split each selection at regex match boundaries.
    SplitOnMatches {
        /// Regex pattern whose matches define split points.
        pattern: String,
    },
    /// Keep only selections whose content matches regex.
    KeepMatching {
        /// Regex pattern that selections must match to be kept.
        pattern: String,
    },
    /// Remove selections whose content matches regex.
    RemoveMatching {
        /// Regex pattern that selections must match to be removed.
        pattern: String,
    },
    /// Trim leading/trailing whitespace from each selection.
    TrimSelections,
    /// Collapse all selections to cursor positions (zero-width).
    CollapseSelections,
    /// Flip anchor/head on all selections.
    FlipSelections,
    /// Force all selections to face forward (anchor <= head).
    EnsureForward,
    /// Merge selections that are exactly touching.
    MergeConsecutive,
    /// Rotate text content between selections.
    RotateContents(Direction),
    /// Align selection cursors by inserting padding spaces.
    AlignSelections,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Offset, SelectionRange, Selections};

    #[test]
    fn default_is_single_cursor() {
        let state = MultiCursorState::default();
        assert!(!state.is_active());
        assert_eq!(state.selections().len(), 1);
    }

    #[test]
    fn multi_cursor_is_active() {
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
            ],
            0,
        );
        let state = MultiCursorState::new(sels);
        assert!(state.is_active());
        assert_eq!(state.selections().len(), 2);
    }

    #[test]
    fn set_selections() {
        let mut state = MultiCursorState::default();
        assert!(!state.is_active());

        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(15)),
                SelectionRange::insert_cursor(Offset::new(25)),
            ],
            1,
        );
        state.set_selections(sels);
        assert!(state.is_active());
        assert_eq!(state.selections().len(), 3);
        assert_eq!(state.selections().primary().head(), Offset::new(15));
    }

    #[test]
    fn selections_mut_access() {
        let mut state = MultiCursorState::default();
        state
            .selections_mut()
            .push(SelectionRange::insert_cursor(Offset::new(42)));
        assert!(state.is_active());
    }
}
