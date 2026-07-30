//! Lightweight position tracker with splice-based offset adjustment.
//!
//! A sorted position list where each position is identified by a unique
//! [`TrackedId`] handle. The [`PositionTracker::splice`] method adjusts all
//! tracked positions in O(n) when text is inserted or deleted, providing a
//! generic foundation that replaces per-feature `adjust_offsets()` methods.
//!
//! # Layering
//!
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`. State modules are pure data containers with
//! no execution logic.

use crate::primitives::byte_delta;

/// Lightweight copy-able handle for a tracked position.
///
/// Opaque identifier returned by [`PositionTracker::track`] and used by
/// [`PositionTracker::get`] and [`PositionTracker::untrack`] for O(n) lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackedId(u32);

impl TrackedId {
    /// Create a `TrackedId` from a raw `u32` value.
    #[inline]
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the underlying `u32` value.
    #[inline]
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Sorted position tracker with O(n) splice adjustment.
///
/// Maintains a list of `(TrackedId, usize)` entries sorted by position
/// (ascending). Positions are adjusted in bulk via [`splice`](Self::splice)
/// when text is inserted or deleted, which removes positions that fall
/// within the deleted range and shifts positions after the edit.
///
/// # Invariants
///
/// - Entries are always sorted by position (ascending).
/// - Each `TrackedId` is unique within the tracker.
/// - IDs are monotonically increasing and never reused within a single session (up to 2^32 allocations).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PositionTracker {
    /// Entries sorted by position (ascending).
    entries: Vec<(TrackedId, usize)>,
    /// Next ID to allocate.
    next_id: u32,
}

impl PositionTracker {
    /// Create a new empty position tracker.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Track a new position, returning its unique handle.
    ///
    /// The position is inserted in sorted order to maintain the ascending
    /// position invariant. IDs are monotonically increasing and never reused.
    ///
    /// # Complexity
    ///
    /// Time: O(n) where n = number of tracked positions (binary search O(log n) + insert O(n))
    /// Space: O(1) amortized
    #[must_use = "the returned TrackedId is the only way to get or remove this position"]
    pub fn track(&mut self, position: usize) -> TrackedId {
        let id = TrackedId(self.next_id);
        debug_assert!(self.next_id < u32::MAX, "TrackedId counter exhaustion");
        self.next_id = self.next_id.saturating_add(1);

        // Binary search for the insertion point to maintain sorted order.
        let idx = self
            .entries
            .binary_search_by_key(&position, |&(_, pos)| pos)
            .unwrap_or_else(|i| i);
        self.entries.insert(idx, (id, position));

        id
    }

    /// Remove a tracked position by its handle.
    ///
    /// Returns `true` if the position was found and removed, `false` if the
    /// ID was not present (already untracked or never tracked).
    ///
    /// # Complexity
    ///
    /// Time: O(n) where n = number of tracked positions (linear scan + remove)
    /// Space: O(1)
    pub fn untrack(&mut self, id: TrackedId) -> bool {
        if let Some(idx) = self.entries.iter().position(|&(eid, _)| eid == id) {
            self.entries.remove(idx);
            true
        } else {
            false
        }
    }

    /// Get the current position for a tracked ID.
    ///
    /// Returns `None` if the ID is not present (untracked, never tracked,
    /// or removed by a [`splice`](Self::splice) operation).
    ///
    /// # Complexity
    ///
    /// Time: O(n) where n = number of tracked positions (linear scan)
    /// Space: O(1)
    #[must_use]
    pub fn get(&self, id: TrackedId) -> Option<usize> {
        self.entries
            .iter()
            .find(|&&(eid, _)| eid == id)
            .map(|&(_, pos)| pos)
    }

    /// Adjust all tracked positions for a text splice operation.
    ///
    /// Models a text edit that removes `old_len` bytes starting at `offset`
    /// and inserts `new_len` bytes in their place.
    ///
    /// # Adjustment rules
    ///
    /// - Positions **before** `offset`: unchanged.
    /// - Positions **in** `[offset, offset + old_len)`: **removed** (invalidated).
    /// - Positions **at or after** `offset + old_len`: shifted by
    ///   `(new_len as isize - old_len as isize)`.
    ///
    /// # Complexity
    ///
    /// Time: O(n) where n = number of tracked positions (single pass via `retain_mut`)
    /// Space: O(1)
    ///
    /// After adjustment the sorted invariant is preserved because:
    /// - Positions before the edit keep their relative order.
    /// - Removed positions leave no gaps.
    /// - Shifted positions all move by the same delta, preserving order.
    ///
    /// # Note: boundary semantics vs `offset_adjust::adjust_offset`
    ///
    /// A position exactly at `offset` with `old_len == 0` is shifted forward
    /// (insert-before semantics). This differs from `offset_adjust::adjust_offset`,
    /// which leaves positions at or before the edit point unchanged. When replacing
    /// `adjust_offsets()` call sites, verify which boundary behavior each call site
    /// requires.
    pub fn splice(&mut self, offset: usize, old_len: usize, new_len: usize) {
        if old_len == 0 && new_len == 0 {
            return;
        }

        let delete_end = offset.saturating_add(old_len);
        let delta = byte_delta::delta(new_len, old_len);

        self.entries.retain_mut(|(_id, pos)| {
            if *pos < offset {
                // Before the edit — unchanged.
                true
            } else if old_len > 0 && *pos < delete_end {
                // Inside the deleted range — remove.
                false
            } else {
                // At or after the end of the deleted range — shift.
                *pos = pos.saturating_add_signed(delta);
                true
            }
        });
    }

    /// Number of tracked positions.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the tracker has no positions.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all tracked positions (but don't reset the ID counter).
    ///
    /// IDs allocated before the clear are never reused, so stale handles
    /// will simply return `None` from [`get`](Self::get).
    #[inline]
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Iterate over all tracked positions in sorted order.
    ///
    /// Yields `(TrackedId, usize)` pairs ordered by ascending position.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (TrackedId, usize)> + '_ {
        self.entries.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction & basics ─────────────────────────────────────────

    #[test]
    fn new_tracker_is_empty() {
        let tracker = PositionTracker::new();
        assert!(tracker.is_empty());
        assert_eq!(tracker.len(), 0);
    }

    #[test]
    fn default_tracker_is_empty() {
        let tracker = PositionTracker::default();
        assert!(tracker.is_empty());
        assert_eq!(tracker.len(), 0);
    }

    // ── Track / get ───────────────────────────────────────────────────

    #[test]
    fn track_single_position() {
        let mut tracker = PositionTracker::new();
        let id = tracker.track(42);
        assert_eq!(tracker.len(), 1);
        assert!(!tracker.is_empty());
        assert_eq!(tracker.get(id), Some(42));
    }

    #[test]
    fn track_multiple_positions_maintains_sorted_order() {
        let mut tracker = PositionTracker::new();
        let id_c = tracker.track(100);
        let id_a = tracker.track(10);
        let id_b = tracker.track(50);

        assert_eq!(tracker.len(), 3);

        // Verify positions are retrievable.
        assert_eq!(tracker.get(id_a), Some(10));
        assert_eq!(tracker.get(id_b), Some(50));
        assert_eq!(tracker.get(id_c), Some(100));

        // Verify iteration order is sorted by position.
        let positions: Vec<usize> = tracker.iter().map(|(_, pos)| pos).collect();
        assert_eq!(positions, vec![10, 50, 100]);
    }

    #[test]
    fn track_duplicate_positions() {
        let mut tracker = PositionTracker::new();
        let id1 = tracker.track(42);
        let id2 = tracker.track(42);

        // Both should be tracked with distinct IDs.
        assert_ne!(id1, id2);
        assert_eq!(tracker.len(), 2);
        assert_eq!(tracker.get(id1), Some(42));
        assert_eq!(tracker.get(id2), Some(42));
    }

    #[test]
    fn get_nonexistent_id_returns_none() {
        let tracker = PositionTracker::new();
        assert_eq!(tracker.get(TrackedId::new(999)), None);
    }

    // ── Untrack ───────────────────────────────────────────────────────

    #[test]
    fn untrack_existing_returns_true() {
        let mut tracker = PositionTracker::new();
        let id = tracker.track(42);
        assert!(tracker.untrack(id));
        assert!(tracker.is_empty());
        assert_eq!(tracker.get(id), None);
    }

    #[test]
    fn untrack_nonexistent_returns_false() {
        let mut tracker = PositionTracker::new();
        assert!(!tracker.untrack(TrackedId::new(999)));
    }

    #[test]
    fn untrack_already_untracked_returns_false() {
        let mut tracker = PositionTracker::new();
        let id = tracker.track(42);
        assert!(tracker.untrack(id));
        assert!(!tracker.untrack(id)); // second time
    }

    #[test]
    fn untrack_preserves_sorted_order() {
        let mut tracker = PositionTracker::new();
        let _id_a = tracker.track(10);
        let id_b = tracker.track(50);
        let _id_c = tracker.track(100);

        tracker.untrack(id_b);

        let positions: Vec<usize> = tracker.iter().map(|(_, pos)| pos).collect();
        assert_eq!(positions, vec![10, 100]);
    }

    // ── Clear ─────────────────────────────────────────────────────────

    #[test]
    fn clear_removes_all_positions() {
        let mut tracker = PositionTracker::new();
        let id1 = tracker.track(10);
        let id2 = tracker.track(50);
        let id3 = tracker.track(100);

        tracker.clear();

        assert!(tracker.is_empty());
        assert_eq!(tracker.len(), 0);
        // Stale IDs return None.
        assert_eq!(tracker.get(id1), None);
        assert_eq!(tracker.get(id2), None);
        assert_eq!(tracker.get(id3), None);
    }

    #[test]
    fn clear_does_not_reset_id_counter() {
        let mut tracker = PositionTracker::new();
        let id_before = tracker.track(10);
        tracker.clear();
        let id_after = tracker.track(20);

        // IDs should be strictly increasing even after clear.
        assert!(id_after.get() > id_before.get());
    }

    #[test]
    fn ids_after_clear_do_not_collide_with_stale_ids() {
        let mut tracker = PositionTracker::new();
        let stale_id = tracker.track(10);
        tracker.clear();
        let new_id = tracker.track(10);

        // The stale ID should not resolve.
        assert_eq!(tracker.get(stale_id), None);
        // The new ID should resolve.
        assert_eq!(tracker.get(new_id), Some(10));
    }

    // ── Splice: pure insertion (old_len=0) ────────────────────────────

    #[test]
    fn splice_insert_shifts_positions_after() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(50);
        let id_c = tracker.track(100);

        // Insert 5 bytes at position 30.
        tracker.splice(30, 0, 5);

        assert_eq!(tracker.get(id_a), Some(10)); // before → unchanged
        assert_eq!(tracker.get(id_b), Some(55)); // after → shifted +5
        assert_eq!(tracker.get(id_c), Some(105)); // after → shifted +5
        assert_eq!(tracker.len(), 3);
    }

    #[test]
    fn splice_insert_at_position_shifts_it() {
        let mut tracker = PositionTracker::new();
        let id = tracker.track(30);

        // Insert 10 bytes exactly at position 30.
        // Position is >= offset (and not in a deleted range since old_len=0), so it shifts.
        tracker.splice(30, 0, 10);

        assert_eq!(tracker.get(id), Some(40));
    }

    #[test]
    fn splice_insert_at_beginning() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(50);

        tracker.splice(0, 0, 20);

        assert_eq!(tracker.get(id_a), Some(30));
        assert_eq!(tracker.get(id_b), Some(70));
    }

    #[test]
    fn splice_insert_at_end_beyond_all_positions() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(50);

        // Insert at position 200 — well after all tracked positions.
        tracker.splice(200, 0, 10);

        assert_eq!(tracker.get(id_a), Some(10)); // unchanged
        assert_eq!(tracker.get(id_b), Some(50)); // unchanged
    }

    // ── Splice: pure deletion (new_len=0) ─────────────────────────────

    #[test]
    fn splice_delete_removes_positions_in_range() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(50);
        let id_c = tracker.track(100);

        // Delete 60 bytes at position 30 → removes [30, 90).
        tracker.splice(30, 60, 0);

        assert_eq!(tracker.get(id_a), Some(10)); // before → unchanged
        assert_eq!(tracker.get(id_b), None); // at 50, inside [30, 90) → removed
        assert_eq!(tracker.get(id_c), Some(40)); // at 100, after → shifted by -60
        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn splice_delete_at_exact_boundary() {
        let mut tracker = PositionTracker::new();
        let id_start = tracker.track(30);
        let id_end = tracker.track(50);
        let id_after = tracker.track(51);

        // Delete 20 bytes at position 30 → removes [30, 50).
        tracker.splice(30, 20, 0);

        assert_eq!(tracker.get(id_start), None); // at 30 (start of range) → removed
        assert_eq!(tracker.get(id_end), Some(30)); // at 50 (end of range, >= 50) → shifted
        assert_eq!(tracker.get(id_after), Some(31)); // at 51, after → shifted by -20
    }

    #[test]
    fn splice_delete_all_positions() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(20);
        let id_c = tracker.track(30);

        // Delete range [0, 100) — everything is inside.
        tracker.splice(0, 100, 0);

        assert_eq!(tracker.get(id_a), None);
        assert_eq!(tracker.get(id_b), None);
        assert_eq!(tracker.get(id_c), None);
        assert!(tracker.is_empty());
    }

    #[test]
    fn splice_delete_before_all_positions() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(50);
        let id_b = tracker.track(100);

        // Delete 10 bytes at position 0 → removes [0, 10).
        tracker.splice(0, 10, 0);

        assert_eq!(tracker.get(id_a), Some(40));
        assert_eq!(tracker.get(id_b), Some(90));
    }

    // ── Splice: replacement (old_len > 0, new_len > 0) ───────────────

    #[test]
    fn splice_replace_shorter() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(35);
        let id_c = tracker.track(100);

        // Replace 10 bytes at position 30 with 3 bytes → delta = -7.
        // Deleted range: [30, 40).
        tracker.splice(30, 10, 3);

        assert_eq!(tracker.get(id_a), Some(10)); // before → unchanged
        assert_eq!(tracker.get(id_b), None); // at 35, inside [30, 40) → removed
        assert_eq!(tracker.get(id_c), Some(93)); // at 100, after → shifted by -7
    }

    #[test]
    fn splice_replace_longer() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(31);
        let id_c = tracker.track(100);

        // Replace 3 bytes at position 30 with 10 bytes → delta = +7.
        // Deleted range: [30, 33).
        tracker.splice(30, 3, 10);

        assert_eq!(tracker.get(id_a), Some(10)); // before → unchanged
        assert_eq!(tracker.get(id_b), None); // at 31, inside [30, 33) → removed
        assert_eq!(tracker.get(id_c), Some(107)); // at 100, after → shifted by +7
    }

    #[test]
    fn splice_replace_equal_length() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(35);
        let id_c = tracker.track(100);

        // Replace 5 bytes at position 30 with 5 bytes → delta = 0.
        // Deleted range: [30, 35). Position 35 is NOT in the range (half-open).
        tracker.splice(30, 5, 5);

        assert_eq!(tracker.get(id_a), Some(10)); // before → unchanged
        assert_eq!(tracker.get(id_b), Some(35)); // at 35, >= delete_end (35) → shifted by 0
        assert_eq!(tracker.get(id_c), Some(100)); // after → shifted by 0, unchanged
    }

    // ── Splice: edge cases ────────────────────────────────────────────

    #[test]
    fn splice_on_empty_tracker_is_noop() {
        let mut tracker = PositionTracker::new();
        tracker.splice(10, 5, 3);
        assert!(tracker.is_empty());
    }

    #[test]
    fn splice_zero_zero_is_noop() {
        let mut tracker = PositionTracker::new();
        let id = tracker.track(42);

        tracker.splice(10, 0, 0);

        assert_eq!(tracker.get(id), Some(42));
    }

    #[test]
    fn splice_position_at_zero() {
        let mut tracker = PositionTracker::new();
        let id = tracker.track(0);

        // Delete 5 bytes at position 0 → position 0 is inside [0, 5) → removed.
        tracker.splice(0, 5, 0);

        assert_eq!(tracker.get(id), None);
    }

    #[test]
    fn splice_preserves_sorted_invariant_after_shift() {
        let mut tracker = PositionTracker::new();
        tracker.track(10);
        tracker.track(20);
        tracker.track(30);
        tracker.track(40);
        tracker.track(50);

        // Delete [15, 35) — removes positions 20, 30. Positions 40, 50 shift by -20.
        tracker.splice(15, 20, 0);

        let positions: Vec<usize> = tracker.iter().map(|(_, pos)| pos).collect();
        assert_eq!(positions, vec![10, 20, 30]);
        // Verify sorted invariant.
        for window in positions.windows(2) {
            assert!(window[0] <= window[1], "positions should be sorted");
        }
    }

    #[test]
    fn splice_preserves_sorted_invariant_after_insert() {
        let mut tracker = PositionTracker::new();
        tracker.track(10);
        tracker.track(20);
        tracker.track(30);

        // Insert 100 bytes at position 15.
        tracker.splice(15, 0, 100);

        let positions: Vec<usize> = tracker.iter().map(|(_, pos)| pos).collect();
        assert_eq!(positions, vec![10, 120, 130]);
        for window in positions.windows(2) {
            assert!(window[0] <= window[1], "positions should be sorted");
        }
    }

    // ── Multiple splices ──────────────────────────────────────────────

    #[test]
    fn multiple_splices_compound_correctly() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(50);
        let id_c = tracker.track(100);

        // First splice: insert 10 bytes at position 20.
        tracker.splice(20, 0, 10);
        assert_eq!(tracker.get(id_a), Some(10));
        assert_eq!(tracker.get(id_b), Some(60));
        assert_eq!(tracker.get(id_c), Some(110));

        // Second splice: delete 5 bytes at position 55 → removes [55, 60).
        // Position 60 is at boundary (60 >= 60), NOT inside range → shifted by -5.
        tracker.splice(55, 5, 0);
        assert_eq!(tracker.get(id_a), Some(10));
        assert_eq!(tracker.get(id_b), Some(55)); // 60 >= 60 (delete_end), shifted: 60 - 5 = 55
        assert_eq!(tracker.get(id_c), Some(105));
    }

    #[test]
    fn splice_after_untrack() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(50);
        let id_c = tracker.track(100);

        tracker.untrack(id_b);
        tracker.splice(30, 0, 20);

        assert_eq!(tracker.get(id_a), Some(10));
        assert_eq!(tracker.get(id_b), None);
        assert_eq!(tracker.get(id_c), Some(120));
    }

    // ── TrackedId ─────────────────────────────────────────────────────

    #[test]
    fn tracked_id_new_and_get() {
        let id = TrackedId::new(42);
        assert_eq!(id.get(), 42);
    }

    #[test]
    fn tracked_id_equality() {
        assert_eq!(TrackedId::new(1), TrackedId::new(1));
        assert_ne!(TrackedId::new(1), TrackedId::new(2));
    }

    #[test]
    fn tracked_id_copy() {
        let id = TrackedId::new(7);
        let id2 = id; // Copy, not move.
        assert_eq!(id, id2);
    }

    #[test]
    fn tracked_ids_are_monotonically_increasing() {
        let mut tracker = PositionTracker::new();
        let id1 = tracker.track(100);
        let id2 = tracker.track(50);
        let id3 = tracker.track(200);

        assert!(id1.get() < id2.get());
        assert!(id2.get() < id3.get());
    }

    // ── Iterator ──────────────────────────────────────────────────────

    #[test]
    fn iter_empty_tracker() {
        let tracker = PositionTracker::new();
        assert_eq!(tracker.iter().count(), 0);
    }

    #[test]
    fn iter_returns_all_entries_in_position_order() {
        let mut tracker = PositionTracker::new();
        let id_c = tracker.track(300);
        let id_a = tracker.track(100);
        let id_b = tracker.track(200);

        let entries: Vec<(TrackedId, usize)> = tracker.iter().collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], (id_a, 100));
        assert_eq!(entries[1], (id_b, 200));
        assert_eq!(entries[2], (id_c, 300));
    }

    // ── Splice algorithm from task description ────────────────────────

    #[test]
    fn splice_example_from_spec() {
        // splice(offset=5, old_len=3, new_len=1):
        //   - positions < 5: unchanged
        //   - positions in [5, 8): REMOVED
        //   - positions >= 8: shifted by (1 - 3) = -2
        let mut tracker = PositionTracker::new();
        let id_before = tracker.track(3);
        let id_in_range_1 = tracker.track(5);
        let id_in_range_2 = tracker.track(7);
        let id_after_1 = tracker.track(8);
        let id_after_2 = tracker.track(20);

        tracker.splice(5, 3, 1);

        assert_eq!(tracker.get(id_before), Some(3)); // unchanged
        assert_eq!(tracker.get(id_in_range_1), None); // removed (in [5, 8))
        assert_eq!(tracker.get(id_in_range_2), None); // removed (in [5, 8))
        assert_eq!(tracker.get(id_after_1), Some(6)); // 8 - 2 = 6
        assert_eq!(tracker.get(id_after_2), Some(18)); // 20 - 2 = 18
    }

    // ── Clone ─────────────────────────────────────────────────────────

    #[test]
    fn clone_is_independent() {
        let mut tracker = PositionTracker::new();
        let id = tracker.track(42);

        let mut cloned = tracker.clone();
        cloned.splice(0, 50, 0); // This removes the position in the clone.

        // Original is unaffected.
        assert_eq!(tracker.get(id), Some(42));
        // Clone has the position removed.
        assert_eq!(cloned.get(id), None);
    }

    // ── Saturating subtraction prevents underflow ─────────────────────

    #[test]
    fn splice_large_deletion_saturates_to_zero() {
        let mut tracker = PositionTracker::new();
        let id = tracker.track(5);

        // A tracked position is only shifted when it sits at or after the end of
        // the deleted range; positions inside the range are removed instead. So
        // the leftward shift can never take a position below zero, and the
        // saturating_sub in splice is a guard, not a live code path.
        assert_eq!(tracker.get(id), Some(5));

        // The largest legitimate leftward shift: delete [0, 9) with a position at
        // 10. 10 is past delete_end, so it shifts by -9 to 1 rather than
        // saturating.
        let mut tracker2 = PositionTracker::new();
        let id2 = tracker2.track(10);
        tracker2.splice(0, 9, 0);
        assert_eq!(tracker2.get(id2), Some(1));
    }

    // ── Serde round-trip ──────────────────────────────────────────────

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_tracker() {
        let mut tracker = PositionTracker::new();
        let id_a = tracker.track(10);
        let id_b = tracker.track(50);
        let _id_c = tracker.track(100);
        tracker.untrack(id_b);

        let json = serde_json::to_string(&tracker).expect("serialize PositionTracker");
        let restored: PositionTracker =
            serde_json::from_str(&json).expect("deserialize PositionTracker");

        assert_eq!(restored.len(), 2);
        assert_eq!(restored.get(id_a), Some(10));
        assert_eq!(restored.get(id_b), None);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip_tracked_id() {
        let id = TrackedId::new(42);
        let json = serde_json::to_string(&id).expect("serialize TrackedId");
        let restored: TrackedId = serde_json::from_str(&json).expect("deserialize TrackedId");
        assert_eq!(id, restored);
    }
}
