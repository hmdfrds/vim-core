//! State diffing for reactive UIs.
//!
//! Provides [`StateDiff`] and [`StateSnapshot`] types that enable reactive
//! host UIs to perform minimal updates — only re-rendering components
//! whose underlying state actually changed.
//!
//! # Usage
//!
//! ```ignore
//! // Before processing a keystroke
//! let snapshot = StateSnapshot::capture(engine.state());
//!
//! // Process keystroke
//! let response = engine.process(key, ctx);
//!
//! // Compute what changed
//! let diff = snapshot.diff(engine.state());
//!
//! // Only re-render changed components
//! if diff.mode {
//!     ui.update_mode_indicator(engine.state().mode());
//! }
//! if diff.registers {
//!     ui.update_register_display(engine.state().registers());
//! }
//! ```
//!
//! # Architecture
//!
//! By tracking which state domains changed between two snapshots, hosts
//! avoid expensive full-state comparisons and unnecessary re-renders.
//!
//! # Layering
//!
//! State holds pure data containers with no execution logic. Imports
//! `primitives`, `std` and sibling `state` modules; must not import
//! `commands`, `effects`, `execution` or `dispatch`.

use compact_str::CompactString;

use super::VimState;
use crate::primitives::Mode;

/// Describes which parts of [`VimState`] changed between two snapshots.
///
/// Enables reactive UIs to perform minimal updates — only re-render
/// components whose underlying state actually changed.
///
/// Each field is `true` if the corresponding state domain changed.
/// All fields default to `false` (no change).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(
    clippy::struct_excessive_bools,
    reason = "intentional: each bool is a distinct state domain flag"
)]
pub struct StateDiff {
    /// Mode changed.
    pub mode: bool,
    /// Register contents changed.
    pub registers: bool,
    /// Mark positions changed.
    pub marks: bool,
    /// Search state changed (pattern, direction, etc.).
    pub search: bool,
    /// Command-line state changed.
    pub command_line: bool,
    /// Visual selection state changed.
    pub visual: bool,
    /// Jumplist changed.
    pub jumplist: bool,
    /// Changelist changed.
    pub changelist: bool,
    /// Recording state changed.
    pub recording: bool,
}

impl StateDiff {
    /// Returns `true` if nothing changed.
    ///
    /// A no-op keystroke (e.g., pressing an unmapped key in normal mode)
    /// typically produces an empty diff.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.mode
            && !self.registers
            && !self.marks
            && !self.search
            && !self.command_line
            && !self.visual
            && !self.jumplist
            && !self.changelist
            && !self.recording
    }

    /// Returns `true` if anything changed.
    ///
    /// Convenience inverse of [`is_empty`](Self::is_empty).
    #[inline]
    #[must_use]
    pub const fn has_any(&self) -> bool {
        !self.is_empty()
    }
}

/// Lightweight snapshot of diffable [`VimState`] fields.
///
/// Created via [`StateSnapshot::capture()`] before processing a key,
/// then compared with the post-key state via [`diff()`](Self::diff).
///
/// For complex types that lack `PartialEq` (registers, marks), we
/// snapshot their monotonic version counters. For simpler types, we
/// capture representative scalar values.
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    /// Editing mode at snapshot time.
    mode: Mode,
    /// Register version counter at snapshot time.
    register_version: u64,
    /// Marks version counter at snapshot time.
    marks_version: u64,
    /// Search pattern at snapshot time.
    search_pattern: Option<CompactString>,
    /// Search direction at snapshot time.
    search_direction: crate::primitives::SearchDirection,
    /// Command-line input text at snapshot time.
    command_line_text: CompactString,
    /// Whether visual mode was active at snapshot time.
    visual_active: bool,
    /// Jumplist length at snapshot time.
    jumplist_len: usize,
    /// Jumplist navigation position at snapshot time.
    jumplist_position: usize,
    /// Changelist length at snapshot time.
    changelist_len: usize,
    /// Changelist navigation position at snapshot time.
    changelist_position: usize,
    /// Whether macro recording was active at snapshot time.
    recording: bool,
}

impl StateSnapshot {
    /// Capture a snapshot of the current [`VimState`].
    ///
    /// This is a lightweight operation — it reads scalar values and
    /// version counters, never cloning large data structures.
    #[must_use]
    pub fn capture(state: &VimState) -> Self {
        Self {
            mode: state.mode(),
            register_version: state.registers().version(),
            marks_version: state.marks().version(),
            search_pattern: state.search().pattern().map(CompactString::from),
            search_direction: state.search().direction(),
            command_line_text: CompactString::from(state.command_line().input()),
            visual_active: state.mode().is_visual(),
            jumplist_len: state.jump_list().len(),
            jumplist_position: state.jump_list().position(),
            changelist_len: state.changelist().len(),
            changelist_position: state.changelist().position(),
            recording: state.macros().is_recording(),
        }
    }

    /// Compare this snapshot with the current [`VimState`] to produce a diff.
    ///
    /// Each field in the returned [`StateDiff`] is `true` if the corresponding
    /// state domain differs between the snapshot and the current state.
    #[must_use]
    pub fn diff(&self, current: &VimState) -> StateDiff {
        let current_visual_active = current.mode().is_visual();
        let current_search_pattern = current.search().pattern();

        // Search pattern comparison: both None → equal, otherwise compare strings.
        let search_pattern_changed = match (&self.search_pattern, current_search_pattern) {
            (None, None) => false,
            (Some(old), Some(new)) => old.as_str() != new,
            _ => true,
        };

        StateDiff {
            mode: self.mode != current.mode(),
            registers: self.register_version != current.registers().version(),
            marks: self.marks_version != current.marks().version(),
            search: search_pattern_changed || self.search_direction != current.search().direction(),
            command_line: self.command_line_text.as_str() != current.command_line().input(),
            visual: self.visual_active != current_visual_active,
            jumplist: self.jumplist_len != current.jump_list().len()
                || self.jumplist_position != current.jump_list().position(),
            changelist: self.changelist_len != current.changelist().len()
                || self.changelist_position != current.changelist().position(),
            recording: self.recording != current.macros().is_recording(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StateDiff, StateSnapshot};
    use crate::primitives::{MarkName, Offset, RegisterContent, RegisterName};
    use crate::primitives::{Mode, SearchDirection, VisualType};
    use crate::state::VimState;

    // ── Helper ──────────────────────────────────────────────────────────

    /// Snapshot, mutate, diff — single round-trip helper.
    fn capture_mutate_diff(state: &mut VimState, mutate: impl FnOnce(&mut VimState)) -> StateDiff {
        let snap = StateSnapshot::capture(state);
        mutate(state);
        snap.diff(state)
    }

    // ── Tests ───────────────────────────────────────────────────────────

    /// Same state produces an empty diff — no false positives.
    #[test]
    fn test_empty_diff_when_nothing_changed() {
        let state = VimState::new();
        let snap = StateSnapshot::capture(&state);
        let diff = snap.diff(&state);
        assert!(diff.is_empty());
        assert!(!diff.has_any());
    }

    /// Mode field change produces diff with mode=true.
    #[test]
    fn test_mode_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.set_mode(Mode::Insert);
        });
        assert!(diff.mode);
        assert!(diff.has_any());
        // Other fields unchanged.
        assert!(!diff.registers);
        assert!(!diff.marks);
        assert!(!diff.search);
        assert!(!diff.command_line);
        assert!(!diff.jumplist);
        assert!(!diff.changelist);
        assert!(!diff.recording);
    }

    /// Register mutation detected via version counter.
    #[test]
    fn test_register_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.registers_mut().set(
                RegisterName::new_unchecked('a'),
                RegisterContent::char_wise("test"),
            );
        });
        assert!(diff.registers);
        assert!(!diff.mode);
    }

    /// Mark mutation detected via version counter.
    #[test]
    fn test_marks_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.marks_mut().set(
                MarkName::new('a').unwrap(),
                crate::primitives::Mark::from_raw(42),
            );
        });
        assert!(diff.marks);
        assert!(!diff.mode);
    }

    /// Search pattern change detected.
    #[test]
    fn test_search_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.search_mut().set_pattern("foo", SearchDirection::Forward);
        });
        assert!(diff.search);
        assert!(!diff.mode);
    }

    /// Search direction change (same pattern) detected.
    #[test]
    fn test_search_direction_change_detected() {
        let mut state = VimState::new();
        state
            .search_mut()
            .set_pattern("foo", SearchDirection::Forward);

        let diff = capture_mutate_diff(&mut state, |s| {
            s.search_mut().set_pattern("foo", SearchDirection::Backward);
        });
        assert!(diff.search);
    }

    /// Command-line text change detected.
    #[test]
    fn test_command_line_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.command_line_mut().insert_char('w');
        });
        assert!(diff.command_line);
        assert!(!diff.mode);
    }

    /// Visual mode entry detected.
    #[test]
    fn test_visual_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.set_mode(Mode::Visual(VisualType::Char));
        });
        assert!(diff.visual);
        assert!(diff.mode);
    }

    /// Jumplist change detected.
    #[test]
    fn test_jumplist_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.jump_list_mut().push(Offset::new(100), None);
        });
        assert!(diff.jumplist);
        assert!(!diff.mode);
    }

    /// Changelist change detected.
    #[test]
    fn test_changelist_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.changelist_mut().push(Offset::new(50));
        });
        assert!(diff.changelist);
        assert!(!diff.mode);
    }

    /// Recording state change detected.
    #[test]
    fn test_recording_change_detected() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.macros_mut()
                .start_recording(RegisterName::new_unchecked('q'));
        });
        assert!(diff.recording);
        assert!(!diff.mode);
    }

    /// Multiple simultaneous changes all detected correctly.
    #[test]
    fn test_multiple_changes() {
        let mut state = VimState::new();
        let diff = capture_mutate_diff(&mut state, |s| {
            s.set_mode(Mode::Insert);
            s.registers_mut().set(
                RegisterName::new_unchecked('a'),
                RegisterContent::char_wise("y"),
            );
            s.search_mut().set_pattern("bar", SearchDirection::Backward);
        });
        assert!(diff.mode);
        assert!(diff.registers);
        assert!(diff.search);
        // Unchanged domains:
        assert!(!diff.marks);
        assert!(!diff.jumplist);
        assert!(!diff.changelist);
        assert!(!diff.recording);
    }

    /// Full round-trip: capture, modify, diff, verify.
    #[test]
    fn test_snapshot_and_diff() {
        let mut state = VimState::new();

        // Set up initial state.
        state
            .search_mut()
            .set_pattern("initial", SearchDirection::Forward);
        state.jump_list_mut().push(Offset::new(10), None);

        // Capture snapshot of populated state.
        let snap = StateSnapshot::capture(&state);

        // Modify several domains.
        state.set_mode(Mode::Visual(VisualType::Line));
        state.marks_mut().set_last_change(Offset::new(99));
        state
            .search_mut()
            .set_pattern("changed", SearchDirection::Forward);
        state.changelist_mut().push(Offset::new(200));
        state
            .macros_mut()
            .start_recording(RegisterName::new_unchecked('a'));

        let diff = snap.diff(&state);

        // All modified domains detected.
        assert!(diff.mode);
        assert!(diff.marks);
        assert!(diff.search);
        assert!(diff.changelist);
        assert!(diff.recording);
        assert!(diff.visual); // mode went from Normal to Visual

        // Unmodified domains.
        assert!(!diff.registers); // untouched
        assert!(!diff.jumplist); // same len and position as snapshot
        assert!(!diff.command_line); // untouched
    }

    /// Entering and exiting visual mode both detected.
    #[test]
    fn test_visual_exit_detected() {
        let mut state = VimState::new();
        state.set_mode(Mode::Visual(VisualType::Char));

        let diff = capture_mutate_diff(&mut state, |s| {
            s.set_mode(Mode::Normal);
        });
        assert!(diff.visual);
        assert!(diff.mode);
    }

    /// Jumplist navigation (position change, same length) detected.
    #[test]
    fn test_jumplist_navigation_detected() {
        let mut state = VimState::new();
        state.jump_list_mut().push(Offset::new(10), None);
        state.jump_list_mut().push(Offset::new(20), None);

        let diff = capture_mutate_diff(&mut state, |s| {
            s.jump_list_mut().older();
        });
        assert!(diff.jumplist);
    }

    /// Clearing search pattern detected.
    #[test]
    fn test_search_clear_detected() {
        let mut state = VimState::new();
        state
            .search_mut()
            .set_pattern("foo", SearchDirection::Forward);

        let diff = capture_mutate_diff(&mut state, |s| {
            s.search_mut().clear();
        });
        assert!(diff.search);
    }

    /// is_empty and has_any are consistent inverses.
    #[test]
    fn test_is_empty_has_any_consistency() {
        let empty = StateDiff::default();
        assert!(empty.is_empty());
        assert!(!empty.has_any());

        let non_empty = StateDiff {
            mode: true,
            ..StateDiff::default()
        };
        assert!(!non_empty.is_empty());
        assert!(non_empty.has_any());
    }
}
