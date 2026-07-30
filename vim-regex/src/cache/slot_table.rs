// Copy-on-write slot table for NFA simulation capture groups.
//
// # Architecture
//
// The `SlotTable` uses an indirection layer between state indices and physical
// data rows. Multiple state indices can share a single physical row via
// refcounting. Mutation (via `get_mut`) triggers copy-on-write: if the row's
// refcount > 1, a fresh physical row is allocated from the free list, the data
// is copied, and the old row's refcount is decremented.
//
// ## Layout
//
//   State index 0  ->  indirection[0] = RowId(2)  ->  arena[2*sps .. 3*sps]
//   State index 1  ->  indirection[1] = RowId(2)  ->  (same physical row -- shared)
//   State index 2  ->  indirection[2] = SENTINEL   ->  (returns SENTINEL_ROW: all None)
//
// ## Sentinel row
//
// `RowId(0)` is the sentinel. Its physical data is always all-`None`. Its
// refcount is permanently `u32::MAX` so it is never decremented to zero or
// placed on the free list. All state indices start mapped to the sentinel.
//
// ## Free list
//
// Physical rows that are not in use are chained through `refcounts[]`:
// `refcounts[free_row]` stores the index of the next free row (or `u32::MAX`
// for end-of-list). `free_head` points to the first free row. On allocation
// we pop from the head; on deallocation we push to the head.
//
// ## `reset_all()`
//
// Called once per swap (when the table becomes the "next" table for a new BFS
// step). Rebuilds the free list to contain ALL non-sentinel rows and resets
// all indirections to the sentinel. This is O(state_capacity) and runs once
// per BFS step -- the same cost as `SparseSet::clear()`.
//
// ## NoSaves optimisation
//
// When `slots_per_state == 0`, the arena and all auxiliary structures are
// empty. Every method is a no-op or returns an empty slice. The indirection
// table and refcounts are still allocated (they're just Vecs of length 0 or
// state_capacity with zero per-element cost) but the arena -- which is the
// expensive part -- is zero-sized.
//
// ## Lifecycle invariants
//
// - `copy_slots(parent, child)` is called only when the child state has been
//   confirmed inserted into the SparseSet, OR is about to be pushed onto the
//   work stack for dedup checking. In the latter case, if the child fails
//   dedup, the refcount increment is a harmless phantom cleared by
//   `reset_all()` at the next swap boundary.
//
// - Between `prune_after_accept` truncating the thread list and the next
//   `cache.swap()`, the table with phantom refcounts is only read from (for
//   `copy_curr_to_next_row`). Its free list is never consulted for new
//   allocations. At the next swap, `reset_all()` clears all phantoms.
//
// - `reset_all()` is the ONLY bulk cleanup mechanism. No per-state cleanup
//   is needed during thread truncation or SparseSet clearing.

/// Index into the physical arena. `RowId(0)` is the sentinel (all-None row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowId(u32);

impl RowId {
    const SENTINEL: Self = Self(0);
}

const FREE_LIST_END: u32 = u32::MAX;

#[derive(Debug)]
pub(crate) struct SlotTable {
    /// Physical data arena. Length = `row_capacity * slots_per_state`.
    /// Row 0 is the sentinel (always all-None).
    arena: Vec<Option<usize>>,

    /// Maps state index -> physical `RowId`.
    /// Length = `state_capacity`.
    indirection: Vec<RowId>,

    /// Refcount (or free-list next pointer) for each physical row.
    /// Length = `row_capacity`.
    ///
    /// - For an in-use row: the number of state indices pointing at it.
    /// - For a free row: the index of the next free row (or `FREE_LIST_END`).
    /// - For the sentinel (index 0): permanently `u32::MAX`.
    refcounts: Vec<u32>,

    /// Head of the free list (index into `refcounts`), or `FREE_LIST_END`.
    free_head: u32,

    /// Number of slots per state row.
    slots_per_state: usize,

    /// Total physical rows (including sentinel). Equal to `state_capacity + 1`
    /// to guarantee enough rows for all states plus the sentinel.
    row_capacity: usize,
}

// Performance-critical arena code where all indices are bounded by
// state_capacity / row_capacity invariants established in the constructor.
#[allow(clippy::indexing_slicing, clippy::cast_possible_truncation)]
impl SlotTable {
    /// Allocate a table with room for `state_capacity` states, each having
    /// `slots_per_state` slot entries.
    ///
    /// When `slots_per_state == 0` the arena allocation is zero-sized.
    pub(crate) fn new(state_capacity: usize, slots_per_state: usize) -> Self {
        // Row 0 = sentinel. Rows 1..=state_capacity are available for states.
        let row_capacity = state_capacity + 1;
        let arena_len = row_capacity.saturating_mul(slots_per_state);
        let arena = vec![None; arena_len];

        // All state indices start mapped to the sentinel.
        let indirection = vec![RowId::SENTINEL; state_capacity];

        // Build refcounts: sentinel gets MAX, all others go on the free list.
        let mut refcounts = vec![0u32; row_capacity];
        refcounts[0] = u32::MAX; // sentinel: immortal
        let free_head = build_free_list(&mut refcounts, row_capacity);

        Self {
            arena,
            indirection,
            refcounts,
            free_head,
            slots_per_state,
            row_capacity,
        }
    }

    /// Number of slots per state row.
    pub(crate) const fn slots_per_state(&self) -> usize {
        self.slots_per_state
    }

    /// Return the slot row for `state_index` as an immutable slice.
    ///
    /// Returns an empty slice when `slots_per_state == 0`.
    pub(crate) fn get(&self, state_index: usize) -> &[Option<usize>] {
        if self.slots_per_state == 0 {
            return &[];
        }
        let row = self.indirection[state_index];
        self.arena_slice(row)
    }

    /// Return the slot row for `state_index` as a mutable slice.
    ///
    /// If the row is shared (refcount > 1), performs copy-on-write:
    /// allocates a fresh physical row, copies data, decrements the old row's
    /// refcount, and updates the indirection.
    ///
    /// Returns an empty slice when `slots_per_state == 0`.
    pub(crate) fn get_mut(&mut self, state_index: usize) -> &mut [Option<usize>] {
        if self.slots_per_state == 0 {
            return &mut [];
        }
        let old_row = self.indirection[state_index];
        if self.refcounts[old_row.0 as usize] > 1 {
            // Shared row (or sentinel) -- must copy-on-write.
            let new_row = self.alloc_row();
            let sps = self.slots_per_state;
            let old_start = old_row.0 as usize * sps;
            let new_start = new_row.0 as usize * sps;
            self.arena
                .copy_within(old_start..old_start + sps, new_start);
            self.dec_ref(old_row);
            self.indirection[state_index] = new_row;
            self.arena_slice_mut(new_row)
        } else {
            // Exclusive owner (refcount == 1) -- mutate in place.
            self.arena_slice_mut(old_row)
        }
    }

    /// Make `to_index` share the same physical row as `from_index`.
    ///
    /// If `to_index` previously pointed at a different row, that row's
    /// refcount is decremented (and the row is freed if it drops to zero).
    ///
    /// No-op when `slots_per_state == 0` or `from_index == to_index`.
    #[inline]
    pub(crate) fn copy_slots(&mut self, from_index: usize, to_index: usize) {
        if self.slots_per_state == 0 || from_index == to_index {
            return;
        }
        let src_row = self.indirection[from_index];
        let dst_row = self.indirection[to_index];

        if src_row == dst_row {
            return;
        }

        self.dec_ref(dst_row);
        self.indirection[to_index] = src_row;
        self.inc_ref(src_row);
    }

    /// Reset every slot in the row at `state_index` to `None`.
    ///
    /// Releases the current row and remaps to the sentinel.
    ///
    /// No-op when `slots_per_state == 0`.
    pub(crate) fn clear_row(&mut self, state_index: usize) {
        if self.slots_per_state == 0 {
            return;
        }
        let old_row = self.indirection[state_index];
        if old_row == RowId::SENTINEL {
            return;
        }
        // Release old row (shared or exclusive) and remap to sentinel.
        self.dec_ref(old_row);
        self.indirection[state_index] = RowId::SENTINEL;
        self.inc_ref(RowId::SENTINEL);
    }

    /// Bulk reset: remap all state indices to the sentinel and rebuild the
    /// free list. O(state_capacity + row_capacity).
    ///
    /// Called once per swap when this table becomes the "next" table.
    pub(crate) fn reset_all(&mut self) {
        if self.slots_per_state == 0 {
            return;
        }

        // All state indices -> sentinel.
        self.indirection.fill(RowId::SENTINEL);

        // Sentinel row: restore immortal refcount and clear data.
        self.refcounts[0] = u32::MAX;
        let sps = self.slots_per_state;
        self.arena[..sps].fill(None);

        // Rebuild free list: rows 1..row_capacity.
        self.free_head = build_free_list(&mut self.refcounts, self.row_capacity);

        // NOTE: We do NOT clear the arena data for non-sentinel rows.
        // The data will be overwritten when the row is allocated and used.
        // This keeps reset_all() at O(state_capacity) rather than
        // O(state_capacity * slots_per_state).
    }

    /// Assign slot data from an external slice into `state_index`.
    ///
    /// Allocates a fresh physical row, copies `data` into it, and releases
    /// whatever row `state_index` previously pointed at. This is designed
    /// for cross-table copies (e.g., `curr_slots` -> `next_slots`).
    ///
    /// `data` must have length >= `slots_per_state` (excess is ignored).
    ///
    /// No-op when `slots_per_state == 0`.
    pub(crate) fn assign_from_slice(&mut self, state_index: usize, data: &[Option<usize>]) {
        if self.slots_per_state == 0 {
            return;
        }
        let sps = self.slots_per_state;
        let src = &data[..sps];

        // If source data is all-None, just point at sentinel.
        if src.iter().all(Option::is_none) {
            let old_row = self.indirection[state_index];
            if old_row != RowId::SENTINEL {
                self.dec_ref(old_row);
                self.indirection[state_index] = RowId::SENTINEL;
                self.inc_ref(RowId::SENTINEL);
            }
            return;
        }

        // Allocate a fresh row and copy data into it.
        let new_row = self.alloc_row();
        let start = new_row.0 as usize * sps;
        self.arena[start..start + sps].copy_from_slice(src);

        // Release whatever state_index previously pointed at.
        let old_row = self.indirection[state_index];
        self.dec_ref(old_row);
        self.indirection[state_index] = new_row;
    }

    // ─────────────────────────────────────────────────────────────────────
    // Internal helpers
    // ─────────────────────────────────────────────────────────────────────

    /// Pop a free row from the free list and set its refcount to 1.
    ///
    /// # Panics
    ///
    /// Panics if the free list is empty (should never happen in correct usage
    /// since `row_capacity == state_capacity + 1`).
    fn alloc_row(&mut self) -> RowId {
        assert!(
            self.free_head != FREE_LIST_END,
            "CowSlotTable: free list exhausted (bug: more rows allocated than states)"
        );
        let row_idx = self.free_head;
        self.free_head = self.refcounts[row_idx as usize];
        self.refcounts[row_idx as usize] = 1;
        RowId(row_idx)
    }

    /// Increment the refcount for a physical row.
    /// Sentinel (refcount = `u32::MAX`) saturates.
    fn inc_ref(&mut self, row: RowId) {
        let rc = &mut self.refcounts[row.0 as usize];
        *rc = rc.saturating_add(1);
    }

    /// Decrement the refcount. If it reaches zero, return the row to the
    /// free list. Sentinel is never decremented.
    fn dec_ref(&mut self, row: RowId) {
        let idx = row.0 as usize;
        let rc = &mut self.refcounts[idx];
        if *rc == u32::MAX {
            // Sentinel -- never free it, never decrement it.
            return;
        }
        *rc -= 1;
        if *rc == 0 {
            self.refcounts[idx] = self.free_head;
            self.free_head = idx as u32;
        }
    }

    fn arena_slice(&self, row: RowId) -> &[Option<usize>] {
        let sps = self.slots_per_state;
        let start = row.0 as usize * sps;
        &self.arena[start..start + sps]
    }

    fn arena_slice_mut(&mut self, row: RowId) -> &mut [Option<usize>] {
        let sps = self.slots_per_state;
        let start = row.0 as usize * sps;
        &mut self.arena[start..start + sps]
    }
}

/// Chain rows `1..row_capacity` into a free list through `refcounts`.
/// Returns the free list head.
#[allow(clippy::cast_possible_truncation, clippy::indexing_slicing)]
fn build_free_list(refcounts: &mut [u32], row_capacity: usize) -> u32 {
    if row_capacity <= 1 {
        return FREE_LIST_END;
    }
    for (i, rc) in refcounts
        .iter_mut()
        .enumerate()
        .take(row_capacity - 1)
        .skip(1)
    {
        *rc = (i + 1) as u32;
    }
    refcounts[row_capacity - 1] = FREE_LIST_END;
    1
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::SlotTable;

    // ── NoSaves (slots_per_state == 0) ──────────────────────────────────

    #[test]
    fn zero_slots_is_zero_sized() {
        let t = SlotTable::new(8, 0);
        assert_eq!(t.slots_per_state(), 0);
        assert!(t.get(0).is_empty());
        assert!(t.get(7).is_empty());
    }

    #[test]
    fn copy_slots_zero_width_noop() {
        let mut t = SlotTable::new(4, 0);
        t.copy_slots(0, 3);
        assert!(t.get(0).is_empty());
        assert!(t.get(3).is_empty());
    }

    #[test]
    fn reset_all_zero_width_noop() {
        let mut t = SlotTable::new(4, 0);
        t.reset_all();
        assert!(t.get(0).is_empty());
    }

    #[test]
    fn assign_from_slice_zero_width_noop() {
        let mut t = SlotTable::new(4, 0);
        t.assign_from_slice(0, &[]);
        assert!(t.get(0).is_empty());
    }

    // ── Basic get/set ───────────────────────────────────────────────────

    #[test]
    fn basic_get_set() {
        let mut t = SlotTable::new(4, 3);
        let row = t.get_mut(1);
        row[0] = Some(10);
        row[1] = Some(20);
        row[2] = None;

        assert_eq!(t.get(1), &[Some(10), Some(20), None]);
        assert_eq!(t.get(0), &[None, None, None]);
        assert_eq!(t.get(2), &[None, None, None]);
        assert_eq!(t.get(3), &[None, None, None]);
    }

    // ── copy_slots (CoW sharing) ────────────────────────────────────────

    #[test]
    fn copy_slots_shares_data() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(0)[0] = Some(5);
        t.get_mut(0)[1] = Some(9);

        t.copy_slots(0, 2);

        assert_eq!(t.get(0), &[Some(5), Some(9)]);
        assert_eq!(t.get(2), &[Some(5), Some(9)]);
        assert_eq!(t.get(1), &[None, None]);
        assert_eq!(t.get(3), &[None, None]);
    }

    #[test]
    fn copy_slots_self_is_noop() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(1)[0] = Some(42);
        t.copy_slots(1, 1);
        assert_eq!(t.get(1), &[Some(42), None]);
    }

    // ── CoW materialisation on get_mut ──────────────────────────────────

    #[test]
    fn cow_materialises_on_mutation() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(0)[0] = Some(10);
        t.get_mut(0)[1] = Some(20);

        t.copy_slots(0, 1);
        assert_eq!(t.get(0), t.get(1));

        t.get_mut(1)[0] = Some(99);

        assert_eq!(t.get(0), &[Some(10), Some(20)]);
        assert_eq!(t.get(1), &[Some(99), Some(20)]);
    }

    #[test]
    fn cow_chain_three_way_share() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(0)[0] = Some(1);
        t.get_mut(0)[1] = Some(2);

        t.copy_slots(0, 1);
        t.copy_slots(1, 2);
        assert_eq!(t.get(0), &[Some(1), Some(2)]);
        assert_eq!(t.get(1), &[Some(1), Some(2)]);
        assert_eq!(t.get(2), &[Some(1), Some(2)]);

        t.get_mut(2)[0] = Some(99);
        assert_eq!(t.get(0), &[Some(1), Some(2)]);
        assert_eq!(t.get(1), &[Some(1), Some(2)]);
        assert_eq!(t.get(2), &[Some(99), Some(2)]);

        t.get_mut(1)[1] = Some(88);
        assert_eq!(t.get(0), &[Some(1), Some(2)]);
        assert_eq!(t.get(1), &[Some(1), Some(88)]);
        assert_eq!(t.get(2), &[Some(99), Some(2)]);
    }

    // ── clear_row ───────────────────────────────────────────────────────

    #[test]
    fn clear_row_resets_to_none() {
        let mut t = SlotTable::new(3, 4);
        {
            let row = t.get_mut(1);
            row[0] = Some(1);
            row[1] = Some(2);
            row[2] = Some(3);
            row[3] = Some(4);
        }
        assert_eq!(t.get(1), &[Some(1), Some(2), Some(3), Some(4)]);

        t.clear_row(1);
        assert_eq!(t.get(1), &[None, None, None, None]);
        assert_eq!(t.get(0), &[None, None, None, None]);
        assert_eq!(t.get(2), &[None, None, None, None]);
    }

    #[test]
    fn clear_row_sentinel_is_noop() {
        let mut t = SlotTable::new(3, 2);
        t.clear_row(0);
        assert_eq!(t.get(0), &[None, None]);
    }

    #[test]
    fn clear_row_shared_does_not_affect_siblings() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(0)[0] = Some(10);
        t.get_mut(0)[1] = Some(20);
        t.copy_slots(0, 1);

        t.clear_row(1);
        assert_eq!(t.get(1), &[None, None]);
        assert_eq!(t.get(0), &[Some(10), Some(20)]);
    }

    // ── reset_all ───────────────────────────────────────────────────────

    #[test]
    fn reset_all_clears_everything() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(0)[0] = Some(10);
        t.get_mut(1)[0] = Some(20);
        t.get_mut(2)[0] = Some(30);
        t.get_mut(3)[0] = Some(40);

        t.reset_all();

        for i in 0..4 {
            assert_eq!(
                t.get(i),
                &[None, None],
                "row {i} should be None after reset"
            );
        }
    }

    #[test]
    fn reset_all_recycles_rows() {
        let mut t = SlotTable::new(4, 2);
        for i in 0..4 {
            t.get_mut(i)[0] = Some(i * 10);
        }

        t.reset_all();

        for i in 0..4 {
            t.get_mut(i)[0] = Some(i * 100);
        }
        for i in 0..4 {
            assert_eq!(t.get(i), &[Some(i * 100), None]);
        }
    }

    #[test]
    fn reset_all_handles_shared_rows() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(0)[0] = Some(10);
        t.copy_slots(0, 1);
        t.copy_slots(0, 2);
        t.copy_slots(0, 3);

        t.reset_all();

        for i in 0..4 {
            assert_eq!(t.get(i), &[None, None]);
        }

        for i in 0..4 {
            t.get_mut(i)[0] = Some(i);
        }
    }

    // ── assign_from_slice ───────────────────────────────────────────────

    #[test]
    fn assign_from_slice_basic() {
        let mut t = SlotTable::new(4, 3);
        let data = [Some(1), Some(2), Some(3)];
        t.assign_from_slice(2, &data);
        assert_eq!(t.get(2), &[Some(1), Some(2), Some(3)]);
        assert_eq!(t.get(0), &[None, None, None]);
        assert_eq!(t.get(1), &[None, None, None]);
    }

    #[test]
    fn assign_from_slice_all_none_uses_sentinel() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(1)[0] = Some(42);
        assert_eq!(t.get(1), &[Some(42), None]);

        t.assign_from_slice(1, &[None, None]);
        assert_eq!(t.get(1), &[None, None]);
    }

    #[test]
    fn assign_from_slice_replaces_existing() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(1)[0] = Some(10);
        t.get_mut(1)[1] = Some(20);

        t.assign_from_slice(1, &[Some(30), Some(40)]);
        assert_eq!(t.get(1), &[Some(30), Some(40)]);
    }

    // ── Row isolation ───────────────────────────────────────────────────

    #[test]
    fn row_isolation() {
        let mut t = SlotTable::new(3, 2);
        t.get_mut(0)[0] = Some(100);
        t.get_mut(0)[1] = Some(101);
        t.get_mut(1)[0] = Some(200);
        t.get_mut(1)[1] = Some(201);
        t.get_mut(2)[0] = Some(300);
        t.get_mut(2)[1] = Some(301);

        assert_eq!(t.get(0), &[Some(100), Some(101)]);
        assert_eq!(t.get(1), &[Some(200), Some(201)]);
        assert_eq!(t.get(2), &[Some(300), Some(301)]);
    }

    // ── Free list recycling ─────────────────────────────────────────────

    #[test]
    fn free_list_recycling() {
        let mut t = SlotTable::new(2, 2);
        t.get_mut(0)[0] = Some(1);
        t.get_mut(1)[0] = Some(2);

        t.clear_row(0);
        assert_eq!(t.get(0), &[None, None]);

        t.get_mut(0)[0] = Some(99);
        assert_eq!(t.get(0), &[Some(99), None]);
        assert_eq!(t.get(1), &[Some(2), None]);
    }

    #[test]
    fn free_list_stress() {
        let mut t = SlotTable::new(8, 2);
        for round in 0..5 {
            for i in 0..8 {
                t.get_mut(i)[0] = Some(round * 100 + i);
            }
            t.reset_all();
        }
        for i in 0..8 {
            assert_eq!(t.get(i), &[None, None]);
        }
        for i in 0..8 {
            t.get_mut(i)[0] = Some(i);
        }
        for i in 0..8 {
            assert_eq!(t.get(i), &[Some(i), None]);
        }
    }

    // ── Cross-table simulation ──────────────────────────────────────────

    #[test]
    fn cross_table_copy_pattern() {
        let mut curr = SlotTable::new(4, 3);
        let mut next = SlotTable::new(4, 3);

        curr.get_mut(2)[0] = Some(10);
        curr.get_mut(2)[1] = Some(20);
        curr.get_mut(2)[2] = Some(30);

        let src = curr.get(2);
        next.assign_from_slice(1, src);

        assert_eq!(next.get(1), &[Some(10), Some(20), Some(30)]);
        assert_eq!(curr.get(2), &[Some(10), Some(20), Some(30)]);

        next.reset_all();
        assert_eq!(next.get(1), &[None, None, None]);
    }

    // ── Sentinel invariant ──────────────────────────────────────────────

    #[test]
    fn sentinel_data_always_none() {
        let mut t = SlotTable::new(4, 2);
        t.get_mut(0)[0] = Some(1);
        t.get_mut(1)[0] = Some(2);
        assert_eq!(t.get(2), &[None, None]);
        assert_eq!(t.get(3), &[None, None]);
    }

    #[test]
    fn sentinel_survives_heavy_sharing() {
        let mut t = SlotTable::new(8, 1);
        t.copy_slots(0, 1);
        t.copy_slots(1, 2);
        t.copy_slots(2, 3);
        for i in 0..4 {
            assert_eq!(t.get(i), &[None]);
        }
    }
}
