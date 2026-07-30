//! Premultiplied DFA transition table.
//!
//! State IDs are premultiplied by stride: `TaggedStateId::index()` is already
//! `ordinal * stride`. Transition lookup is one addition + one array index.
//! No division needed to recover ordinal.

use super::TaggedStateId;

/// Flat transition table indexed by premultiplied state ID + byte class.
///
/// Layout: `table[sid.index() + class] = target_sid`
///
/// All entries are initialized to `TaggedStateId::UNKNOWN` and filled lazily.
#[allow(
    dead_code,
    reason = "num_classes used by acceleration analysis (Section K)"
)]
#[derive(Debug, Clone)]
pub(super) struct StateTable {
    /// Flat storage: `table[premul_index + class_id] = target`.
    table: Vec<TaggedStateId>,
    /// Number of equivalence classes (logical alphabet size).
    num_classes: usize,
    /// Stride (power-of-2 >= num_classes). Used for premultiplication.
    stride: usize,
    /// log2(stride) for shift-based ordinal<->premul conversion.
    stride_shift: u32,
}

#[allow(
    dead_code,
    reason = "covers stride_shift() and num_classes(): callers convert indices with ordinal_to_premul/premul_to_ordinal rather than shifting themselves, and read the class count from ByteClasses, so those two accessors have no users outside this file's tests"
)]
impl StateTable {
    /// Create a new empty state table with the given alphabet size.
    pub(super) fn new(num_classes: usize) -> Self {
        let stride = num_classes.next_power_of_two();
        let stride_shift = stride.trailing_zeros();
        Self {
            table: Vec::new(),
            num_classes,
            stride,
            stride_shift,
        }
    }

    /// Look up a transition: `table[sid.index() + class]`.
    /// The caller guarantees `sid.index() + class < table.len()`.
    #[inline(always)]
    #[allow(unsafe_code)]
    pub(super) fn next(&self, sid: TaggedStateId, class: u8) -> TaggedStateId {
        debug_assert!((sid.index() + class as usize) < self.table.len());
        // SAFETY: The caller (DFA search loop) guarantees that `sid` was returned
        // by `alloc_state()` and `class < num_classes`. Both invariants together
        // ensure `sid.index() + class < table.len()`. The debug_assert above
        // verifies this in debug builds.
        unsafe { *self.table.get_unchecked(sid.index() + class as usize) }
    }

    /// Set a transition slot.
    #[inline]
    pub(super) fn set(&mut self, sid: TaggedStateId, class: u8, target: TaggedStateId) {
        let idx = sid.index() + class as usize;
        debug_assert!(idx < self.table.len());
        self.table[idx] = target;
    }

    /// Allocate one new state row, initialized to UNKNOWN.
    /// Returns the premultiplied index for the new state, or `None` if the
    /// premultiplied index would overflow the 27-bit `TaggedStateId` index space.
    pub(super) fn alloc_state(&mut self) -> Option<u32> {
        let premul = self.table.len();
        debug_assert!(
            premul <= TaggedStateId::MAX_INDEX as usize,
            "DFA state table overflow: premul={premul} > MAX_INDEX={}",
            TaggedStateId::MAX_INDEX
        );
        if premul > TaggedStateId::MAX_INDEX as usize {
            return None;
        }
        self.table
            .resize(self.table.len() + self.stride, TaggedStateId::UNKNOWN);
        Some(premul as u32)
    }

    /// Allocate one new state row, initialized to a specific value.
    /// Returns the premultiplied index for the new state, or `None` if the
    /// premultiplied index would overflow the 27-bit `TaggedStateId` index space.
    pub(super) fn alloc_state_filled(&mut self, fill: TaggedStateId) -> Option<u32> {
        let premul = self.table.len();
        debug_assert!(
            premul <= TaggedStateId::MAX_INDEX as usize,
            "DFA state table overflow: premul={premul} > MAX_INDEX={}",
            TaggedStateId::MAX_INDEX
        );
        if premul > TaggedStateId::MAX_INDEX as usize {
            return None;
        }
        self.table.resize(self.table.len() + self.stride, fill);
        Some(premul as u32)
    }

    /// Number of states currently allocated.
    #[inline]
    pub(super) fn state_count(&self) -> usize {
        // stride is always >= 1 (power of 2), so division is safe.
        self.table.len() / self.stride
    }

    /// Total stride (power of 2).
    #[inline]
    pub(super) const fn stride(&self) -> usize {
        self.stride
    }

    /// log2(stride).
    #[inline]
    pub(super) const fn stride_shift(&self) -> u32 {
        self.stride_shift
    }

    /// Convert a state ordinal to its premultiplied index.
    #[inline]
    pub(super) const fn ordinal_to_premul(&self, ordinal: usize) -> u32 {
        (ordinal << self.stride_shift) as u32
    }

    /// Convert a premultiplied index back to ordinal.
    #[inline]
    pub(super) const fn premul_to_ordinal(&self, premul: u32) -> usize {
        (premul as usize) >> self.stride_shift
    }

    /// Approximate memory usage in bytes.
    pub(super) fn memory_usage(&self) -> usize {
        self.table.len() * core::mem::size_of::<TaggedStateId>()
    }

    /// Clear all states (for cache eviction).
    pub(super) fn clear(&mut self) {
        self.table.clear();
    }

    /// Number of equivalence classes.
    #[inline]
    pub(super) const fn num_classes(&self) -> usize {
        self.num_classes
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_table_alloc_and_lookup() {
        let mut table = StateTable::new(4);
        assert_eq!(table.stride(), 4);
        assert_eq!(table.state_count(), 0);

        let s0 = table.alloc_state().unwrap();
        assert_eq!(s0, 0);
        assert_eq!(table.state_count(), 1);

        // All transitions should be UNKNOWN initially.
        let sid = TaggedStateId::normal(s0);
        assert!(table.next(sid, 0).is_unknown());
        assert!(table.next(sid, 1).is_unknown());

        // Set a transition and verify.
        let target = TaggedStateId::normal(4); // premul for ordinal 1
        table.set(sid, 2, target);
        assert_eq!(table.next(sid, 2), target);
    }

    #[test]
    fn state_table_premul_conversion() {
        let table = StateTable::new(8); // stride = 8, shift = 3
        assert_eq!(table.stride_shift(), 3);
        assert_eq!(table.ordinal_to_premul(0), 0);
        assert_eq!(table.ordinal_to_premul(1), 8);
        assert_eq!(table.ordinal_to_premul(5), 40);
        assert_eq!(table.premul_to_ordinal(0), 0);
        assert_eq!(table.premul_to_ordinal(8), 1);
        assert_eq!(table.premul_to_ordinal(40), 5);
    }

    #[test]
    fn state_table_filled_alloc() {
        let mut table = StateTable::new(4);
        let dead = TaggedStateId::DEAD;
        let s0 = table.alloc_state_filled(dead).unwrap();
        let sid = TaggedStateId::normal(s0);
        // All transitions should be DEAD.
        for class in 0..4u8 {
            assert!(table.next(sid, class).is_dead());
        }
    }

    #[test]
    fn state_table_stride_power_of_two() {
        for n in [1, 2, 3, 5, 7, 8, 15, 16, 17, 30, 64, 100, 200, 255] {
            let table = StateTable::new(n);
            assert!(table.stride().is_power_of_two());
            assert!(table.stride() >= n);
        }
    }

    #[test]
    fn alloc_state_returns_some_within_bounds() {
        let mut table = StateTable::new(4);
        for i in 0..100 {
            let s = table.alloc_state();
            assert!(s.is_some());
            assert_eq!(s.unwrap(), (i * 4) as u32);
        }
    }

    #[test]
    fn alloc_state_first_allocation() {
        let mut table = StateTable::new(128);
        let s0 = table.alloc_state();
        assert!(s0.is_some());
        assert_eq!(s0.unwrap(), 0);
    }
}
