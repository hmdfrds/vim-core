//! Compact snapshot of local marks (a-z) for undo/redo restoration.
//!
//! # Layering
//!
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`. State modules are pure data containers with
//! no execution logic.

use super::marks::Marks;
use crate::primitives::{Mark, MarkName};

/// Number of local marks (a-z).
const LOCAL_MARK_COUNT: usize = 26;

/// Snapshot of local marks (a-z) for undo/redo restoration.
///
/// Follows Neovim's `uh_namedm[NMARKS]` approach: captures the 26
/// lowercase marks at undo-group creation time. On undo/redo, the
/// snapshot is swapped with live marks.
///
/// Only local marks (a-z) are captured — global marks (A-Z) and special
/// marks are not part of undo history in Neovim.
///
/// Stores full `Mark` values (offset + topline) so viewport context
/// participates in undo/redo.
#[derive(Debug, Clone)]
pub(crate) struct MarkSnapshot {
    /// Marks a-z indexed by `(c - b'a')`. `None` = mark was not set.
    marks: [Option<Mark>; LOCAL_MARK_COUNT],
    /// Visual selection marks `<` and `>`.
    /// Neovim's uh_namedm includes these; they must survive undo/redo
    /// because text edits during visual operations adjust them.
    visual_start: Option<Mark>,
    visual_end: Option<Mark>,
}

impl MarkSnapshot {
    /// Create an empty snapshot (all marks `None`).
    ///
    /// Used for the root undo node where no marks have been set yet.
    #[inline]
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            marks: [None; LOCAL_MARK_COUNT],
            visual_start: None,
            visual_end: None,
        }
    }

    /// Capture a snapshot of all local marks from the live `Marks` store.
    ///
    /// Iterates a-z and reads each full `Mark` (offset + topline) via `marks.get()`.
    ///
    /// # Complexity
    ///
    /// Time: O(26) = O(1) — iterates the 26 local mark slots, each
    /// requiring an O(1) amortized hash map lookup.
    ///
    /// Space: O(1) — fixed 26-element array.
    #[must_use]
    pub(crate) fn capture(marks: &Marks) -> Self {
        let mut snapshot = [None; LOCAL_MARK_COUNT];
        for (i, slot) in snapshot.iter_mut().enumerate() {
            let name = MarkName::from_local_index(i);
            *slot = marks.get(name);
        }
        Self {
            marks: snapshot,
            visual_start: marks.get(MarkName::VISUAL_START),
            visual_end: marks.get(MarkName::VISUAL_END),
        }
    }

    /// Swap this snapshot's marks with the live `Marks` store.
    ///
    /// Implements Neovim's swap pattern for undo/redo:
    /// - For each of the 26 local mark slots:
    ///   1. Save the current live value (full `Mark` with topline).
    ///   2. If the snapshot has `Some(mark)`, overwrite the live mark.
    ///   3. Store the saved live value into the snapshot slot.
    ///
    /// If the snapshot slot is `None`, the live mark is **not** cleared —
    /// this matches Neovim's behavior where undo does not remove marks
    /// that didn't exist at snapshot time.
    ///
    /// # Complexity
    ///
    /// Time: O(26) = O(1) — iterates the 26 local mark slots. Each slot
    /// performs an O(1) amortized `marks.get()` and optionally an O(1)
    /// amortized `marks.set()`.
    ///
    /// Space: O(1)
    pub(crate) fn swap_with(&mut self, marks: &mut Marks) {
        for (i, slot) in self.marks.iter_mut().enumerate() {
            let name = MarkName::from_local_index(i);
            let live = marks.get(name);

            if let Some(mark) = *slot {
                marks.set(name, mark);
            }

            *slot = live;
        }

        // Swap visual marks `<` and `>`.
        for (slot, name) in [
            (&mut self.visual_start, MarkName::VISUAL_START),
            (&mut self.visual_end, MarkName::VISUAL_END),
        ] {
            let live = marks.get(name);
            if let Some(mark) = *slot {
                marks.set(name, mark);
            }
            *slot = live;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;

    /// Helper to create a `MarkName` from a char.
    fn mn(c: char) -> MarkName {
        MarkName::new(c).unwrap()
    }

    /// Capture/swap round trip: set marks a,b -> capture -> change marks -> swap -> verify restored.
    #[test]
    fn capture_swap_round_trip() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::new(Offset::new(100)));
        marks.set(mn('b'), Mark::new(Offset::new(200)));

        // Capture the current state
        let mut snapshot = MarkSnapshot::capture(&marks);

        // Change marks to different values
        marks.set(mn('a'), Mark::new(Offset::new(999)));
        marks.set(mn('b'), Mark::new(Offset::new(888)));

        // Swap — should restore original values
        snapshot.swap_with(&mut marks);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 100);
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 200);

        // Snapshot should now contain the changed values
        // Swap again to verify
        snapshot.swap_with(&mut marks);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 999);
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 888);
    }

    /// Swap is self-inverting: swap twice returns to original state.
    #[test]
    fn swap_is_self_inverting() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::new(Offset::new(10)));
        marks.set(mn('c'), Mark::new(Offset::new(30)));
        marks.set(mn('z'), Mark::new(Offset::new(260)));

        let mut snapshot = MarkSnapshot::capture(&marks);

        // Change everything
        marks.set(mn('a'), Mark::new(Offset::new(11)));
        marks.set(mn('c'), Mark::new(Offset::new(33)));
        marks.set(mn('z'), Mark::new(Offset::new(266)));

        // First swap: restores snapshot, snapshot gets live values
        snapshot.swap_with(&mut marks);
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10);
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 30);
        assert_eq!(marks.get(mn('z')).unwrap().offset().get(), 260);

        // Second swap: restores the changed values
        snapshot.swap_with(&mut marks);
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 11);
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 33);
        assert_eq!(marks.get(mn('z')).unwrap().offset().get(), 266);
    }

    /// None slots don't clear live marks.
    #[test]
    fn none_slots_do_not_clear_live_marks() {
        let mut marks = Marks::new();

        // Create an empty snapshot (all None)
        let mut snapshot = MarkSnapshot::new();

        // Set some live marks
        marks.set(mn('a'), Mark::new(Offset::new(100)));
        marks.set(mn('m'), Mark::new(Offset::new(500)));

        // Swap with empty snapshot — live marks should NOT be cleared
        snapshot.swap_with(&mut marks);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 100);
        assert_eq!(marks.get(mn('m')).unwrap().offset().get(), 500);

        // But the snapshot should have captured the live values
        // Verify by swapping again into fresh marks
        let mut fresh_marks = Marks::new();
        snapshot.swap_with(&mut fresh_marks);

        assert_eq!(fresh_marks.get(mn('a')).unwrap().offset().get(), 100);
        assert_eq!(fresh_marks.get(mn('m')).unwrap().offset().get(), 500);
    }

    /// Empty snapshot (no marks set) leaves live marks unchanged.
    #[test]
    fn empty_snapshot_leaves_live_marks_unchanged() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::new(Offset::new(42)));
        marks.set(mn('b'), Mark::new(Offset::new(84)));
        marks.set(mn('z'), Mark::new(Offset::new(999)));

        let mut snapshot = MarkSnapshot::new();
        snapshot.swap_with(&mut marks);

        // All live marks should still be there
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 42);
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 84);
        assert_eq!(marks.get(mn('z')).unwrap().offset().get(), 999);
    }

    /// Capture only captures local marks, not global or special.
    #[test]
    fn capture_ignores_global_and_special_marks() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::new(Offset::new(100)));
        marks.set(mn('A'), Mark::new(Offset::new(200))); // global
        marks.set_last_change(Offset::new(300)); // special

        let mut snapshot = MarkSnapshot::capture(&marks);

        // Clear live mark 'a' and swap — only 'a' should be restored
        marks.delete(mn('a'));
        snapshot.swap_with(&mut marks);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 100);
        // Global and special marks are untouched by snapshot swap
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 200);
    }

    /// All 26 local marks round-trip correctly.
    #[test]
    fn all_26_marks_round_trip() {
        let mut marks = Marks::new();
        for i in 0..26 {
            let name = MarkName::from_local_index(i);
            marks.set(name, Mark::new(Offset::new(i * 100)));
        }

        let mut snapshot = MarkSnapshot::capture(&marks);

        // Overwrite all
        for i in 0..26 {
            let name = MarkName::from_local_index(i);
            marks.set(name, Mark::new(Offset::new(9999)));
        }

        // Swap restores originals
        snapshot.swap_with(&mut marks);

        for i in 0..26 {
            let name = MarkName::from_local_index(i);
            assert_eq!(
                marks.get(name).unwrap().offset().get(),
                i * 100,
                "mark {} should be restored",
                name.char()
            );
        }
    }

    /// `from_local_index` produces correct mark names.
    #[test]
    fn from_local_index_produces_correct_names() {
        for i in 0..26 {
            let name = MarkName::from_local_index(i);
            let expected = char::from(b'a' + u8::try_from(i).unwrap());
            assert_eq!(name.char(), expected);
            assert!(name.is_local());
        }
    }

    /// `from_local_index` panics on out-of-range index.
    #[test]
    #[should_panic(expected = "local mark index must be 0..26")]
    fn from_local_index_panics_on_out_of_range() {
        let _ = MarkName::from_local_index(26);
    }

    // ========== TOPLINE_OFFSET PRESERVATION TESTS ==========

    /// Snapshot captures full Mark (offset + topline_offset) and restores both on swap.
    #[test]
    fn snapshot_preserves_topline_offset_through_undo() {
        let mut marks = Marks::new();
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(100), Some(5)),
        );
        marks.set(
            mn('b'),
            Mark::with_topline_offset(Offset::new(200), Some(8)),
        );

        // Capture snapshot (stores full Mark with topline_offset)
        let mut snapshot = MarkSnapshot::capture(&marks);

        // Change marks to different values (no topline_offset)
        marks.set(mn('a'), Mark::new(Offset::new(999)));
        marks.set(mn('b'), Mark::new(Offset::new(888)));

        // Swap — should restore original marks with topline_offset
        snapshot.swap_with(&mut marks);

        let a = marks.get(mn('a')).unwrap();
        assert_eq!(a.offset().get(), 100);
        assert_eq!(
            a.topline_offset().unwrap(),
            5,
            "topline_offset should be restored by undo"
        );

        let b = marks.get(mn('b')).unwrap();
        assert_eq!(b.offset().get(), 200);
        assert_eq!(
            b.topline_offset().unwrap(),
            8,
            "topline_offset should be restored by undo"
        );
    }

    /// Snapshot captures marks without topline_offset and preserves that on swap.
    #[test]
    fn snapshot_preserves_none_topline_offset() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::new(Offset::new(100)));

        let mut snapshot = MarkSnapshot::capture(&marks);

        // Change to mark with topline_offset
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(200), Some(5)),
        );

        // Swap restores original (no topline_offset)
        snapshot.swap_with(&mut marks);

        let a = marks.get(mn('a')).unwrap();
        assert_eq!(a.offset().get(), 100);
        assert!(
            a.topline_offset().is_none(),
            "should restore None topline_offset"
        );
    }

    /// Double swap is self-inverting (including topline_offset).
    #[test]
    fn snapshot_double_swap_topline_offset_round_trip() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::with_topline_offset(Offset::new(10), Some(0)));

        let mut snapshot = MarkSnapshot::capture(&marks);

        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(20), Some(10)),
        );

        // First swap: restore snapshot values
        snapshot.swap_with(&mut marks);
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10);
        assert_eq!(marks.get(mn('a')).unwrap().topline_offset().unwrap(), 0);

        // Second swap: restore live values
        snapshot.swap_with(&mut marks);
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 20);
        assert_eq!(marks.get(mn('a')).unwrap().topline_offset().unwrap(), 10);
    }
}
