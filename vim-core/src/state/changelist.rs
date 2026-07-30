//! Changelist for g;/g, navigation.
//!
//! # Layering
//!
//! State holds pure data containers with no execution logic. Imports
//! `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`.
//!
//! # Behavior
//!
//! The changelist records the cursor position(s) at each text edit.
//! Each entry stores one offset per cursor (multi-cursor support).
//! Primary cursor is always at index 0 for backward compat.
//!
//! Commands:
//! - `g;` — go to older change position(s)
//! - `g,` — go to newer change position(s)

use crate::primitives::Offset;
use smallvec::SmallVec;
use std::collections::VecDeque;

/// Maximum number of changelist entries (same as Neovim).
const MAX_CHANGES: usize = 100;

/// Changelist — tracks positions where edits occurred.
///
/// Models the same data structure as Vim's internal changelist.
/// Uses `VecDeque` for O(1) eviction of the oldest entry when capacity
/// is exceeded.
///
/// Each entry stores one or more cursor positions (multi-cursor support).
/// Primary cursor is always at index 0. Single-cursor edits use a
/// `SmallVec<[Offset; 1]>` which avoids heap allocation.
///
/// The effect_processor gates pushes by mode: only normal/visual edits
/// create entries (matching Neovim's one-entry-per-undoable-change behavior).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChangeList {
    /// Edit positions (byte offsets). Each entry may have multiple offsets
    /// (one per cursor in multi-cursor mode).
    entries: VecDeque<SmallVec<[Offset; 1]>>,
    /// Cursor into the list. When equal to `len()`, we're at the present.
    current: usize,
}

impl ChangeList {
    /// Create a new empty changelist.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an edit position (single cursor convenience).
    ///
    /// Callers are responsible for gating by mode (effect_processor skips
    /// pushes during insert/replace mode).
    pub fn push(&mut self, offset: Offset) {
        self.push_multi(&[offset]);
    }

    /// Record edit positions for multiple cursors.
    ///
    /// Primary cursor offset should be at index 0.
    /// Callers are responsible for gating by mode (effect_processor skips
    /// pushes during insert/replace mode).
    pub fn push_multi(&mut self, offsets: &[Offset]) {
        if offsets.is_empty() {
            return;
        }

        // If navigated back, truncate forward history
        if self.current < self.entries.len() {
            self.entries.truncate(self.current);
        }

        // Deduplicate: don't store same positions twice in a row
        if let Some(last) = self.entries.back() {
            if last.as_slice() == offsets {
                self.current = self.entries.len();
                return;
            }
        }

        self.entries.push_back(SmallVec::from_slice(offsets));

        // Enforce max size
        if self.entries.len() > MAX_CHANGES {
            self.entries.pop_front();
        }

        self.current = self.entries.len();
    }

    /// Push with same-line dedup: if the previous entry's primary offset is
    /// on the same line as `offset`, update it in place instead of adding a
    /// new entry. Matches Neovim's `changed_common()` behavior.
    ///
    /// `line_of` maps a byte offset to its 0-based line number.
    pub fn push_same_line_dedup(&mut self, offset: Offset, line_of: impl Fn(Offset) -> usize) {
        if let Some(last) = self.entries.back() {
            if let Some(&last_primary) = last.first() {
                if line_of(last_primary) == line_of(offset) {
                    // Same line: update in place instead of pushing
                    self.update_last(offset);
                    return;
                }
            }
        }
        self.push(offset);
    }

    /// Update the most recent entry's primary offset in-place (same-response merge).
    ///
    /// Used by effect_processor when multiple edits in a single response
    /// should produce only one changelist entry. Updates index 0 (primary cursor).
    pub fn update_last(&mut self, offset: Offset) {
        if let Some(last) = self.entries.back_mut() {
            if let Some(first) = last.first_mut() {
                *first = offset;
            } else {
                last.push(offset);
            }
        }
    }

    /// Navigate to older change position(s) (`g;`).
    ///
    /// Returns all cursor offsets for the entry (primary at index 0),
    /// or an empty slice if at start.
    pub fn older(&mut self) -> &[Offset] {
        if self.current > 0 {
            self.current -= 1;
            match self.entries.get(self.current) {
                Some(entry) => entry.as_slice(),
                None => &[],
            }
        } else {
            &[]
        }
    }

    /// Navigate to newer change position(s) (`g,`).
    ///
    /// Returns all cursor offsets for the entry (primary at index 0),
    /// or an empty slice if at end (Vim: E663).
    pub fn newer(&mut self) -> &[Offset] {
        if self.current < self.entries.len() {
            self.current += 1;
            match self.entries.get(self.current) {
                Some(entry) => entry.as_slice(),
                None => &[],
            }
        } else {
            &[]
        }
    }

    /// Peek at the most recent entry's primary offset without mutating.
    #[inline]
    #[must_use]
    pub fn peek_last(&self) -> Option<Offset> {
        self.entries.back().and_then(|e| e.first().copied())
    }

    /// Peek at the primary offset after navigating `count` steps older, without mutating.
    ///
    /// Used by the executor (read-only state) to compute SetCursor target.
    #[must_use]
    pub fn peek_older(&self, count: u32) -> Option<Offset> {
        let mut pos = self.current;
        for _ in 0..count {
            if pos > 0 {
                pos -= 1;
            } else {
                return None;
            }
        }
        self.entries.get(pos).and_then(|e| e.first().copied())
    }

    /// Peek at the primary offset after navigating `count` steps newer, without mutating.
    ///
    /// Used by the executor (read-only state) to compute SetCursor target.
    /// Returns `None` when at the end of the changelist (Vim: E663).
    #[must_use]
    pub fn peek_newer(&self, count: u32) -> Option<Offset> {
        let mut pos = self.current;
        for _ in 0..count {
            if pos < self.entries.len() {
                pos += 1;
            } else {
                return None;
            }
        }
        self.entries.get(pos).and_then(|e| e.first().copied())
    }

    /// Get current position in the changelist.
    #[inline]
    #[must_use]
    pub const fn position(&self) -> usize {
        self.current
    }

    /// Get the number of entries.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Read-only access to all entries for display (`:changes`).
    #[inline]
    #[must_use]
    pub const fn entries(&self) -> &VecDeque<SmallVec<[Offset; 1]>> {
        &self.entries
    }

    /// Clear the changelist.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = 0;
    }
}

impl super::remap::RemapPositions for ChangeList {
    fn remap(&mut self, changeset: &crate::primitives::changeset::ChangeSet) {
        use crate::primitives::changeset::Assoc;

        for entry in &mut self.entries {
            for offset in entry.iter_mut() {
                let new_offset = changeset.map_offset(*offset, Assoc::Before);
                if new_offset != *offset {
                    *offset = new_offset;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_older() {
        let mut cl = ChangeList::new();

        cl.push(Offset::new(10));
        cl.push(Offset::new(50));
        cl.push(Offset::new(100));

        assert_eq!(cl.older()[0].get(), 100);
        assert_eq!(cl.older()[0].get(), 50);
        assert_eq!(cl.older()[0].get(), 10);
        assert!(cl.older().is_empty());
    }

    #[test]
    fn test_newer() {
        let mut cl = ChangeList::new();

        cl.push(Offset::new(10));
        cl.push(Offset::new(50));
        cl.push(Offset::new(100));

        cl.older(); // → pos 2, returns [100]
        cl.older(); // → pos 1, returns [50]

        assert_eq!(cl.newer()[0].get(), 100); // → pos 2
                                              // Now at pos 2, newer increments to 3 (present) — no entry there
        assert!(cl.newer().is_empty());
        // At present — newer returns empty (Vim: E663)
        assert!(cl.newer().is_empty());
        // older() from present (pos 3) goes back to pos 2
        assert_eq!(cl.older()[0].get(), 100);
    }

    #[test]
    fn test_push_truncates_forward() {
        let mut cl = ChangeList::new();

        cl.push(Offset::new(10));
        cl.push(Offset::new(50));
        cl.push(Offset::new(100));

        cl.older(); // 100
        cl.older(); // 50

        // Push new entry — truncates 100
        cl.push(Offset::new(75));

        assert_eq!(cl.len(), 2); // [10, 75]
        assert_eq!(cl.older()[0].get(), 75);
        assert_eq!(cl.older()[0].get(), 10);
        assert!(cl.older().is_empty());
    }

    #[test]
    fn test_dedup_consecutive() {
        let mut cl = ChangeList::new();

        cl.push(Offset::new(42));
        cl.push(Offset::new(42));

        assert_eq!(cl.len(), 1);
    }

    #[test]
    fn test_max_entries() {
        let mut cl = ChangeList::new();

        for i in 0..=MAX_CHANGES + 10 {
            cl.push(Offset::new(i));
        }

        assert_eq!(cl.len(), MAX_CHANGES);
    }

    #[test]
    fn test_empty_changelist() {
        let mut cl = ChangeList::new();

        assert!(cl.is_empty());
        assert!(cl.older().is_empty());
        assert!(cl.newer().is_empty());
    }

    #[test]
    fn test_clear() {
        let mut cl = ChangeList::new();
        cl.push(Offset::new(10));
        cl.push(Offset::new(50));

        cl.clear();

        assert!(cl.is_empty());
        assert_eq!(cl.position(), 0);
    }

    #[test]
    fn test_position_tracking() {
        let mut cl = ChangeList::new();

        cl.push(Offset::new(10));
        cl.push(Offset::new(50));
        cl.push(Offset::new(100));

        assert_eq!(cl.position(), 3);

        cl.older();
        assert_eq!(cl.position(), 2);

        cl.older();
        assert_eq!(cl.position(), 1);

        cl.newer();
        assert_eq!(cl.position(), 2);
    }

    #[test]
    fn test_newer_at_end() {
        let mut cl = ChangeList::new();
        cl.push(Offset::new(100));
        // At present position (current == len) — newer returns empty (Vim: E663)
        assert!(cl.newer().is_empty());
        // Navigate back, then forward again
        assert_eq!(cl.older()[0].get(), 100); // current → 0
        assert!(cl.newer().is_empty()); // current → 1 (present), no entry at index 1
    }

    // ── Multi-cursor changelist tests ─────────────────────────────────────

    #[test]
    fn test_push_multi_single_cursor() {
        let mut cl = ChangeList::new();
        cl.push_multi(&[Offset::new(42)]);

        assert_eq!(cl.len(), 1);
        let entry = cl.older();
        assert_eq!(entry.len(), 1);
        assert_eq!(entry[0].get(), 42);
    }

    #[test]
    fn test_push_multi_multiple_cursors() {
        let mut cl = ChangeList::new();
        cl.push_multi(&[Offset::new(10), Offset::new(50), Offset::new(90)]);

        assert_eq!(cl.len(), 1);
        let entry = cl.older();
        assert_eq!(entry.len(), 3);
        assert_eq!(entry[0].get(), 10); // primary at index 0
        assert_eq!(entry[1].get(), 50);
        assert_eq!(entry[2].get(), 90);
    }

    #[test]
    fn test_push_multi_empty_is_noop() {
        let mut cl = ChangeList::new();
        cl.push_multi(&[]);

        assert!(cl.is_empty());
    }

    #[test]
    fn test_push_multi_dedup() {
        let mut cl = ChangeList::new();
        cl.push_multi(&[Offset::new(10), Offset::new(50)]);
        cl.push_multi(&[Offset::new(10), Offset::new(50)]);

        assert_eq!(cl.len(), 1);
    }

    #[test]
    fn test_push_multi_no_dedup_different_offsets() {
        let mut cl = ChangeList::new();
        cl.push_multi(&[Offset::new(10), Offset::new(50)]);
        cl.push_multi(&[Offset::new(10), Offset::new(60)]);

        assert_eq!(cl.len(), 2);
    }

    #[test]
    fn test_older_newer_multi_cursor_round_trip() {
        let mut cl = ChangeList::new();
        cl.push_multi(&[Offset::new(10), Offset::new(20)]);
        cl.push_multi(&[Offset::new(30), Offset::new(40)]);

        let entry = cl.older();
        assert_eq!(entry, &[Offset::new(30), Offset::new(40)]);

        let entry = cl.older();
        assert_eq!(entry, &[Offset::new(10), Offset::new(20)]);

        let entry = cl.newer();
        assert_eq!(entry, &[Offset::new(30), Offset::new(40)]);
    }

    #[test]
    fn test_peek_last_returns_primary() {
        let mut cl = ChangeList::new();
        cl.push_multi(&[Offset::new(10), Offset::new(50), Offset::new(90)]);

        // peek_last returns primary (index 0)
        assert_eq!(cl.peek_last().unwrap().get(), 10);
    }

    #[test]
    fn test_update_last_updates_primary() {
        let mut cl = ChangeList::new();
        cl.push_multi(&[Offset::new(10), Offset::new(50)]);
        cl.update_last(Offset::new(99));

        let entry = cl.older();
        assert_eq!(entry[0].get(), 99); // primary updated
        assert_eq!(entry[1].get(), 50); // secondary unchanged
    }

    #[test]
    fn test_backward_compat_single_cursor_push_older() {
        // Ensure single-cursor push is fully compatible with older() returning
        // a slice of length 1.
        let mut cl = ChangeList::new();
        cl.push(Offset::new(100));
        cl.push(Offset::new(200));

        let entry = cl.older();
        assert_eq!(entry.len(), 1);
        assert_eq!(entry[0].get(), 200);
    }

    // ========== TASK 5.8: CHANGELIST SAME-LINE DEDUP TESTS ==========

    #[test]
    fn task_5_8_same_line_edits_update_in_place() {
        let mut cl = ChangeList::new();
        // Two edits on the same line => only one changelist entry
        cl.push_same_line_dedup(Offset::new(10), |off| {
            // All offsets 0..100 are on line 0
            if off.get() < 100 {
                0
            } else {
                1
            }
        });
        cl.push_same_line_dedup(Offset::new(20), |off| if off.get() < 100 { 0 } else { 1 });
        assert_eq!(cl.len(), 1);
        // The entry should be updated to the latest offset
        assert_eq!(cl.peek_last().unwrap().get(), 20);
    }

    #[test]
    fn task_5_8_different_line_edits_push_new_entry() {
        let mut cl = ChangeList::new();
        // Edits on different lines => two changelist entries
        cl.push_same_line_dedup(Offset::new(10), |off| if off.get() < 50 { 0 } else { 1 });
        cl.push_same_line_dedup(Offset::new(60), |off| if off.get() < 50 { 0 } else { 1 });
        assert_eq!(cl.len(), 2);
    }

    #[test]
    fn task_5_8_multiple_same_line_edits_one_entry() {
        let mut cl = ChangeList::new();
        // Five edits on the same line => one changelist entry
        for i in 0..5 {
            cl.push_same_line_dedup(Offset::new(i * 5), |_| 0);
        }
        assert_eq!(cl.len(), 1);
        // Updated to the last edit position
        assert_eq!(cl.peek_last().unwrap().get(), 20);
    }

    #[test]
    fn task_5_8_empty_changelist_push_creates_entry() {
        let mut cl = ChangeList::new();
        cl.push_same_line_dedup(Offset::new(42), |_| 3);
        assert_eq!(cl.len(), 1);
        assert_eq!(cl.peek_last().unwrap().get(), 42);
    }
}
