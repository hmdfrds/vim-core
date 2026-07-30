//! Trait-based position remapping through a [`ChangeSet`].
//!
//! # Layering
//!
//! Imports `primitives`, `std` and sibling state modules; must not import
//! `commands`, `effects`, `execution` or `dispatch`. State modules are pure
//! data containers with no execution logic.
//!
//! [`RemapPositions`] is a uniform interface for adjusting stored byte offsets
//! after a text edit described by a [`ChangeSet`]. Each position-tracking
//! subsystem (marks, jumplist, changelist) implements this trait.
//!
//! Named marks (a-z, A-Z) are NOT remapped through this trait; they use the
//! separate `adjust_named_offsets` path with cross-line semantics.

use crate::primitives::changeset::ChangeSet;

/// Adjust all stored byte offsets through a [`ChangeSet`].
///
/// Implementors map every internally-stored position from input-document
/// coordinates to output-document coordinates using
/// [`ChangeSet::map_pos`] / [`ChangeSet::map_offset`].
pub(crate) trait RemapPositions {
    /// Remap all stored positions through `changeset`.
    fn remap(&mut self, changeset: &ChangeSet);
}

#[cfg(test)]
mod tests {
    use super::RemapPositions;
    use crate::primitives::changeset::ChangeSet;
    use crate::primitives::{Mark, MarkName, Offset};

    fn mn(c: char) -> MarkName {
        MarkName::new(c).unwrap()
    }

    // ── Marks ────────────────────────────────────────────────────────

    #[test]
    fn marks_remap_insert_shifts_special_marks() {
        use crate::state::Marks;

        let mut marks = Marks::new();
        marks.set_previous_position(Offset::new(10));
        marks.set_last_change(Offset::new(50));
        marks.set_change_region(Offset::new(100), Offset::new(110));

        let cs = ChangeSet::from_insert(200, 30, "xxxxx");
        marks.remap(&cs);

        // ' and ` at 10 (before edit) — unchanged
        assert_eq!(marks.get(mn('\'')).unwrap().offset().get(), 10);
        assert_eq!(marks.get(mn('`')).unwrap().offset().get(), 10);
        // . at 50 → 55
        assert_eq!(marks.get(mn('.')).unwrap().offset().get(), 55);
        // [ at 100 → 105, ] at 110 → 115
        assert_eq!(marks.get(mn('[')).unwrap().offset().get(), 105);
        assert_eq!(marks.get(mn(']')).unwrap().offset().get(), 115);
    }

    #[test]
    fn marks_remap_delete_collapses_special_marks() {
        use crate::state::Marks;

        let mut marks = Marks::new();
        marks.set_previous_position(Offset::new(10));
        marks.set_last_change(Offset::new(50));
        marks.set_change_region(Offset::new(100), Offset::new(110));

        // Delete bytes 30..60
        let cs = ChangeSet::from_delete(200, 30, 60);
        marks.remap(&cs);

        // ' and ` at 10 — unchanged (before edit)
        assert_eq!(marks.get(mn('\'')).unwrap().offset().get(), 10);
        assert_eq!(marks.get(mn('`')).unwrap().offset().get(), 10);
        // . at 50 — inside deleted region [30, 60) → clamped to 30
        assert_eq!(marks.get(mn('.')).unwrap().offset().get(), 30);
        // [ at 100 → 70, ] at 110 → 80 (shifted back by 30)
        assert_eq!(marks.get(mn('[')).unwrap().offset().get(), 70);
        assert_eq!(marks.get(mn(']')).unwrap().offset().get(), 80);
    }

    #[test]
    fn marks_remap_special_marks() {
        use crate::state::Marks;

        let mut marks = Marks::new();
        marks.set_previous_position(Offset::new(50));
        marks.set_last_change(Offset::new(80));

        let cs = ChangeSet::from_insert(200, 30, "0123456789");
        marks.remap(&cs);

        assert_eq!(marks.get(mn('\'')).unwrap().offset().get(), 60);
        assert_eq!(marks.get(mn('`')).unwrap().offset().get(), 60);
        assert_eq!(marks.get(mn('.')).unwrap().offset().get(), 90);
    }

    #[test]
    fn marks_remap_preserves_topline_on_special() {
        use crate::state::Marks;

        let mut marks = Marks::new();
        // Use change-start mark with topline_offset (special mark that remap touches)
        marks.set(mn('['), Mark::with_topline_offset(Offset::new(50), Some(5)));

        let cs = ChangeSet::from_insert(200, 30, "xxxxx");
        marks.remap(&cs);

        let mark = marks.get(mn('[')).unwrap();
        assert_eq!(mark.offset().get(), 55);
        // topline_offset is relative — unchanged after remap
        assert_eq!(mark.topline_offset().unwrap(), 5);
    }

    #[test]
    fn marks_remap_identity_is_noop() {
        use crate::state::Marks;

        let mut marks = Marks::new();
        marks.set_last_change(Offset::new(42));

        let cs = ChangeSet::identity(200);
        marks.remap(&cs);

        assert_eq!(marks.get(mn('.')).unwrap().offset().get(), 42);
    }

    #[test]
    fn marks_remap_skips_local_and_global() {
        use crate::primitives::BufferId;
        use crate::state::Marks;

        let mut marks = Marks::new();
        let bid = BufferId::new(1);

        marks.set(mn('a'), Mark::from_raw(50));
        marks.set(mn('A'), Mark::from_raw(50));
        marks.set_with_buffer_id(mn('B'), Mark::from_raw(80), Some(bid));

        let cs = ChangeSet::from_insert(200, 30, "xxxxx");
        marks.remap(&cs);

        // Local and global marks are NOT touched by remap
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 50);
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 50);
        assert_eq!(marks.get(mn('B')).unwrap().offset().get(), 80);
    }

    #[test]
    fn marks_remap_skips_visual_and_insert_stop() {
        use crate::state::Marks;

        let mut marks = Marks::new();
        marks.set_visual_region(Offset::new(50), Offset::new(60));
        marks.set_insert_stop(Offset::new(70));

        let cs = ChangeSet::from_insert(200, 30, "xxxxx");
        marks.remap(&cs);

        // Visual and insert-stop marks are skipped (same as adjust_map)
        assert_eq!(marks.get(mn('<')).unwrap().offset().get(), 50);
        assert_eq!(marks.get(mn('>')).unwrap().offset().get(), 60);
        assert_eq!(marks.get(mn('^')).unwrap().offset().get(), 70);
    }

    // ── JumpList ─────────────────────────────────────────────────────

    #[test]
    fn jumplist_remap_insert_shifts() {
        use crate::state::JumpList;

        let mut jl = JumpList::new();
        jl.push(Offset::new(10), None);
        jl.push(Offset::new(50), None);
        jl.push(Offset::new(100), None);

        let cs = ChangeSet::from_insert(200, 30, "xxxxx");
        jl.remap(&cs);

        assert_eq!(jl.older().unwrap().get(), 105);
        assert_eq!(jl.older().unwrap().get(), 55);
        assert_eq!(jl.older().unwrap().get(), 10);
    }

    #[test]
    fn jumplist_remap_delete_collapses() {
        use crate::state::JumpList;

        let mut jl = JumpList::new();
        jl.push(Offset::new(10), None);
        jl.push(Offset::new(50), None);
        jl.push(Offset::new(100), None);

        let cs = ChangeSet::from_delete(200, 30, 60);
        jl.remap(&cs);

        assert_eq!(jl.older().unwrap().get(), 70);
        assert_eq!(jl.older().unwrap().get(), 30);
        assert_eq!(jl.older().unwrap().get(), 10);
    }

    #[test]
    fn jumplist_remap_identity_noop() {
        use crate::state::JumpList;

        let mut jl = JumpList::new();
        jl.push(Offset::new(42), None);

        let cs = ChangeSet::identity(200);
        jl.remap(&cs);

        assert_eq!(jl.older().unwrap().get(), 42);
    }

    #[test]
    fn jumplist_remap_empty_noop() {
        use crate::state::JumpList;

        let mut jl = JumpList::new();
        let cs = ChangeSet::from_insert(200, 30, "xxxxx");
        jl.remap(&cs);
        assert!(jl.is_empty());
    }

    // ── ChangeList ───────────────────────────────────────────────────

    #[test]
    fn changelist_remap_insert_with_before_assoc() {
        use crate::state::ChangeList;

        let mut cl = ChangeList::new();
        cl.push(Offset::new(10));
        cl.push(Offset::new(50));
        cl.push(Offset::new(100));

        // Insert 5 bytes at position 50 (exactly at second entry)
        let cs = ChangeSet::from_insert(200, 50, "xxxxx");
        cl.remap(&cs);

        // Reset cursor to end
        let _ = cl.older();
        let _ = cl.newer();

        assert_eq!(cl.older()[0].get(), 105);
        // 50 at insertion point with Assoc::Before → stays at 50
        assert_eq!(cl.older()[0].get(), 50);
        assert_eq!(cl.older()[0].get(), 10);
    }

    #[test]
    fn changelist_remap_delete() {
        use crate::state::ChangeList;

        let mut cl = ChangeList::new();
        cl.push(Offset::new(10));
        cl.push(Offset::new(50));
        cl.push(Offset::new(100));

        let cs = ChangeSet::from_delete(200, 30, 60);
        cl.remap(&cs);

        assert_eq!(cl.older()[0].get(), 70);
        assert_eq!(cl.older()[0].get(), 30);
        assert_eq!(cl.older()[0].get(), 10);
    }

    #[test]
    fn changelist_remap_empty_noop() {
        use crate::state::ChangeList;

        let mut cl = ChangeList::new();
        let cs = ChangeSet::from_insert(200, 30, "xxxxx");
        cl.remap(&cs);
        assert!(cl.is_empty());
    }

    // ── VimState::remap_all_positions ────────────────────────────────

    #[test]
    fn vim_state_remap_all_positions() {
        use crate::state::VimState;

        let mut state = VimState::new();

        // Local mark — should NOT be remapped
        state.marks_mut().set(mn('a'), Mark::from_raw(50));
        // Special mark — should be remapped
        state.marks_mut().set_previous_position(Offset::new(80));
        state.jump_list_mut().push(Offset::new(60), None);
        state.changelist_mut().push(Offset::new(70));

        let cs = ChangeSet::from_insert(200, 30, "0123456789");
        state.remap_all_positions(&cs);

        // Local mark 'a' unchanged (remap only handles special marks)
        assert_eq!(state.marks().get(mn('a')).unwrap().offset().get(), 50);
        // Special mark ' shifted from 80 to 90
        assert_eq!(state.marks().get(mn('\'')).unwrap().offset().get(), 90);
        assert_eq!(state.jump_list_mut().older().unwrap().get(), 70);
        assert_eq!(state.changelist_mut().older()[0].get(), 80);
    }

    #[test]
    fn vim_state_remap_all_no_exchange() {
        use crate::state::VimState;

        let mut state = VimState::new();
        // Use a special mark (not local) for remap testing
        state.marks_mut().set_last_change(Offset::new(50));

        let cs = ChangeSet::from_insert(200, 30, "xxxxx");
        state.remap_all_positions(&cs);

        assert_eq!(state.marks().get(mn('.')).unwrap().offset().get(), 55);
    }

    // ── Multi-change ─────────────────────────────────────────────────

    #[test]
    fn marks_remap_multi_change_special() {
        use crate::state::Marks;

        let mut marks = Marks::new();
        marks.set_previous_position(Offset::new(5));
        marks.set_last_change(Offset::new(15));
        marks.set_change_region(Offset::new(50), Offset::new(60));

        let cs = ChangeSet::from_changes(
            100,
            [
                (10, 10, Some("XX")), // insert 2 bytes at 10
                (20, 30, None),       // delete 10 bytes at 20..30
            ],
        );
        marks.remap(&cs);

        // ' at 5 — before first edit, unchanged
        assert_eq!(marks.get(mn('\'')).unwrap().offset().get(), 5);
        // . at 15 — between edits, shifted by insert: 15 → 17
        assert_eq!(marks.get(mn('.')).unwrap().offset().get(), 17);
        // [ at 50 → 42, ] at 60 → 52 (shifted by +2 insert, -10 delete = -8)
        assert_eq!(marks.get(mn('[')).unwrap().offset().get(), 42);
        assert_eq!(marks.get(mn(']')).unwrap().offset().get(), 52);
    }

    #[test]
    fn jumplist_remap_replace() {
        use crate::state::JumpList;

        let mut jl = JumpList::new();
        jl.push(Offset::new(5), None);
        jl.push(Offset::new(15), None);
        jl.push(Offset::new(50), None);

        // Replace [10..20] with "abc" (net -7)
        let cs = ChangeSet::from_replace(100, 10, 20, "abc");
        jl.remap(&cs);

        assert_eq!(jl.older().unwrap().get(), 43);
        // 15 inside deleted region → collapses to 13 (after Insert("abc"))
        assert_eq!(jl.older().unwrap().get(), 13);
        assert_eq!(jl.older().unwrap().get(), 5);
    }
}
