//! DFA state checkpoints for edit-incremental matching.
//!
//! Stores DFA state snapshots at line boundaries during search so that
//! buffer edits only require re-scanning from the nearest upstream checkpoint.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// DFA state checkpoints stored at line boundaries during search.
///
/// Each checkpoint records the DFA state at a byte offset (typically a
/// line boundary), so that after an edit we can resume DFA search from
/// the nearest checkpoint upstream of the edit rather than re-scanning
/// from the beginning.
#[derive(Debug)]
pub struct SearchCheckpoints {
    /// Sorted by byte_offset. Each entry: (byte_offset, dfa_state_ordinal).
    line_states: Vec<(usize, u32)>,
    /// Hash of the pattern string -- used to invalidate checkpoints when the
    /// pattern changes.
    pattern_hash: u64,
    /// Total text length when checkpoints were recorded -- used to detect
    /// whether the buffer has been entirely replaced.
    text_len: usize,
}

impl SearchCheckpoints {
    /// Create empty checkpoints for a given pattern.
    pub fn new(pattern: &str, text_len: usize) -> Self {
        Self {
            line_states: Vec::new(),
            pattern_hash: Self::hash_pattern(pattern),
            text_len,
        }
    }

    /// Record a checkpoint at a byte offset with a DFA state ordinal.
    pub fn record(&mut self, byte_offset: usize, dfa_state_ordinal: u32) {
        // Maintain sorted order; replace if offset already exists.
        match self
            .line_states
            .binary_search_by_key(&byte_offset, |&(o, _)| o)
        {
            Ok(idx) => self.line_states[idx].1 = dfa_state_ordinal,
            Err(idx) => self
                .line_states
                .insert(idx, (byte_offset, dfa_state_ordinal)),
        }
    }

    /// Find the nearest checkpoint at or before `byte_offset`.
    ///
    /// Returns `(checkpoint_offset, dfa_state_ordinal)` or `None` if no
    /// checkpoint exists before the given offset.
    pub fn nearest_before(&self, byte_offset: usize) -> Option<(usize, u32)> {
        let idx = self.line_states.partition_point(|&(o, _)| o <= byte_offset);
        if idx == 0 {
            None
        } else {
            Some(self.line_states[idx - 1])
        }
    }

    /// Invalidate all checkpoints at or after `start_offset`.
    ///
    /// Called when an edit touches `start_offset..`, meaning all checkpoints
    /// from that point onward may have stale DFA state.
    pub fn invalidate_from(&mut self, start_offset: usize) {
        let keep = self.line_states.partition_point(|&(o, _)| o < start_offset);
        self.line_states.truncate(keep);
    }

    /// Adjust checkpoint offsets after an edit at `edit_start..edit_end`
    /// that changed the text length by `delta` bytes.
    ///
    /// - Checkpoints before `edit_start` are kept as-is.
    /// - Checkpoints within `edit_start..edit_end` are removed.
    /// - Checkpoints after `edit_end` are shifted by `delta`.
    pub fn notify_edit(&mut self, edit_start: usize, edit_end: usize, new_len: usize) {
        let old_len = edit_end - edit_start;
        let delta = new_len as isize - old_len as isize;

        // Remove checkpoints in the edited range and shift those after.
        self.line_states.retain_mut(|&mut (ref mut offset, _)| {
            if *offset >= edit_start && *offset < edit_end {
                false // Within the edited range -- remove.
            } else if *offset >= edit_end {
                // After the edit -- shift.
                *offset = (*offset as isize + delta) as usize;
                true
            } else {
                true // Before the edit -- keep.
            }
        });

        // Update text_len.
        self.text_len = (self.text_len as isize + delta) as usize;
    }

    /// Check if these checkpoints are valid for the given pattern.
    pub fn is_valid_for(&self, pattern: &str) -> bool {
        Self::hash_pattern(pattern) == self.pattern_hash
    }

    /// Number of stored checkpoints.
    pub fn len(&self) -> usize {
        self.line_states.len()
    }

    /// Whether there are no stored checkpoints.
    pub fn is_empty(&self) -> bool {
        self.line_states.is_empty()
    }

    /// Clear all checkpoints.
    pub fn clear(&mut self) {
        self.line_states.clear();
    }

    /// Returns the text length recorded when checkpoints were created.
    pub fn text_len(&self) -> usize {
        self.text_len
    }

    fn hash_pattern(pattern: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        pattern.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_retrieve() {
        let mut cp = SearchCheckpoints::new("test", 100);
        cp.record(0, 1);
        cp.record(20, 5);
        cp.record(50, 10);

        assert_eq!(cp.nearest_before(25), Some((20, 5)));
        assert_eq!(cp.nearest_before(50), Some((50, 10)));
        assert_eq!(cp.nearest_before(0), Some((0, 1)));
        assert_eq!(cp.nearest_before(19), Some((0, 1)));
    }

    #[test]
    fn invalidate_from() {
        let mut cp = SearchCheckpoints::new("test", 100);
        cp.record(0, 1);
        cp.record(20, 5);
        cp.record(50, 10);
        cp.record(80, 15);

        cp.invalidate_from(50);
        assert_eq!(cp.len(), 2);
        assert_eq!(cp.nearest_before(100), Some((20, 5)));
    }

    #[test]
    fn notify_edit_shifts_offsets() {
        let mut cp = SearchCheckpoints::new("test", 100);
        cp.record(0, 1);
        cp.record(20, 5);
        cp.record(50, 10);
        cp.record(80, 15);

        // Edit: replace bytes 30..40 with 5 bytes (delta = -5).
        cp.notify_edit(30, 40, 5);

        // Checkpoints at 0 and 20 are untouched.
        assert_eq!(cp.nearest_before(0), Some((0, 1)));
        assert_eq!(cp.nearest_before(20), Some((20, 5)));
        // Checkpoint at 50 shifted to 45 (50 - 5).
        assert_eq!(cp.nearest_before(46), Some((45, 10)));
        // Checkpoint at 80 shifted to 75 (80 - 5).
        assert_eq!(cp.nearest_before(76), Some((75, 15)));
    }

    #[test]
    fn notify_edit_removes_checkpoints_in_range() {
        let mut cp = SearchCheckpoints::new("test", 100);
        cp.record(10, 1);
        cp.record(25, 2);
        cp.record(35, 3);
        cp.record(60, 4);

        // Edit: replace bytes 20..40 with 10 bytes.
        cp.notify_edit(20, 40, 10);

        // Checkpoints at 25 and 35 were in the edited range -- removed.
        // Checkpoint at 60 shifted to 50 (60 - 20).
        assert_eq!(cp.len(), 2);
        assert_eq!(cp.nearest_before(15), Some((10, 1)));
        assert_eq!(cp.nearest_before(55), Some((50, 4)));
    }

    #[test]
    fn pattern_validation() {
        let cp = SearchCheckpoints::new("hello", 50);
        assert!(cp.is_valid_for("hello"));
        assert!(!cp.is_valid_for("world"));
    }

    #[test]
    fn empty_checkpoints() {
        let cp = SearchCheckpoints::new("test", 0);
        assert!(cp.is_empty());
        assert_eq!(cp.nearest_before(0), None);
        assert_eq!(cp.nearest_before(100), None);
    }

    #[test]
    fn notify_edit_insert_expands() {
        let mut cp = SearchCheckpoints::new("test", 50);
        cp.record(0, 1);
        cp.record(30, 5);

        // Insert 10 bytes at position 10 (edit_start=10, edit_end=10, new_len=10).
        cp.notify_edit(10, 10, 10);

        assert_eq!(cp.nearest_before(0), Some((0, 1)));
        // Checkpoint at 30 shifted to 40 (30 + 10).
        assert_eq!(cp.nearest_before(45), Some((40, 5)));
    }

    #[test]
    fn record_overwrites_existing_offset() {
        let mut cp = SearchCheckpoints::new("test", 100);
        cp.record(10, 1);
        cp.record(10, 2);
        assert_eq!(cp.len(), 1);
        assert_eq!(cp.nearest_before(10), Some((10, 2)));
    }

    #[test]
    fn text_len_updated_by_notify_edit() {
        let mut cp = SearchCheckpoints::new("test", 100);
        cp.notify_edit(50, 60, 5); // remove 5 bytes
        assert_eq!(cp.text_len(), 95);
    }

    #[test]
    fn clear_empties_all() {
        let mut cp = SearchCheckpoints::new("test", 100);
        cp.record(0, 1);
        cp.record(50, 2);
        cp.clear();
        assert!(cp.is_empty());
        assert_eq!(cp.len(), 0);
    }
}
