/// Briggs/Torczon sparse-dense set.
///
/// Provides O(1) insert, O(1) contains, O(1) clear, and O(n) iteration.
///
/// The key insight: `clear` simply sets `len = 0`. Stale entries left in
/// `sparse` are harmless because `contains` does a double-check:
///   `sparse[id] < len && dense[sparse[id]] == id`
/// A stale sparse entry pointing into a region beyond `len` (or pointing
/// to a slot that now holds a different `id`) will always fail the check.
#[derive(Debug)]
pub(crate) struct SparseSet {
    dense: Vec<u32>,
    sparse: Vec<u32>,
    len: usize,
}

impl SparseSet {
    /// Create a new `SparseSet` that can hold IDs in `0..capacity`.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            dense: vec![0u32; capacity],
            sparse: vec![0u32; capacity],
            len: 0,
        }
    }

    /// Maximum ID (exclusive) this set can hold.
    pub(crate) fn capacity(&self) -> usize {
        self.dense.len()
    }

    /// Number of elements currently in the set.
    #[allow(dead_code, reason = "used in tests and future API")]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the set contains no elements.
    #[allow(dead_code, reason = "used in tests and future API")]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns `true` if `id` is a member of the set.
    ///
    /// # Panics
    /// Panics if `id as usize >= self.capacity()`.
    #[inline]
    pub(crate) fn contains(&self, id: u32) -> bool {
        let i = id as usize;
        let s = self.sparse[i] as usize;
        s < self.len && self.dense[s] == id
    }

    /// Insert `id` into the set. Returns `true` if `id` was newly inserted,
    /// `false` if it was already present.
    ///
    /// # Panics
    /// Panics if `id as usize >= self.capacity()`.
    #[inline]
    pub(crate) fn insert(&mut self, id: u32) -> bool {
        if self.contains(id) {
            return false;
        }
        let pos = self.len;
        self.dense[pos] = id;
        self.sparse[id as usize] = pos as u32;
        self.len += 1;
        true
    }

    /// Clear the set in O(1). Stale sparse entries are harmless.
    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }

    /// Iterate over the current members in insertion order.
    #[allow(dead_code, reason = "used in tests and future API")]
    pub(crate) fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.dense[..self.len].iter().copied()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::SparseSet;

    #[test]
    fn empty_set() {
        let s = SparseSet::new(16);
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
        assert_eq!(s.capacity(), 16);
        assert!(!s.contains(0));
        assert!(!s.contains(15));
        assert_eq!(s.iter().count(), 0);
    }

    #[test]
    fn insert_and_contains() {
        let mut s = SparseSet::new(8);
        assert!(s.insert(3));
        assert!(s.insert(7));
        assert!(s.insert(0));
        assert_eq!(s.len(), 3);
        assert!(!s.is_empty());
        assert!(s.contains(3));
        assert!(s.contains(7));
        assert!(s.contains(0));
        assert!(!s.contains(1));
        assert!(!s.contains(4));
    }

    #[test]
    fn double_insert_returns_false() {
        let mut s = SparseSet::new(8);
        assert!(s.insert(5));
        assert!(!s.insert(5));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn clear_is_o1_and_invalidates() {
        let mut s = SparseSet::new(8);
        s.insert(1);
        s.insert(4);
        s.insert(6);
        assert_eq!(s.len(), 3);
        s.clear();
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
        assert!(!s.contains(1));
        assert!(!s.contains(4));
        assert!(!s.contains(6));
    }

    #[test]
    fn reuse_after_clear() {
        let mut s = SparseSet::new(8);
        s.insert(2);
        s.insert(5);
        s.clear();

        // Re-inserting the same IDs must work correctly.
        assert!(s.insert(5));
        assert!(s.insert(2));
        assert_eq!(s.len(), 2);
        assert!(s.contains(5));
        assert!(s.contains(2));
        // IDs not re-inserted must still be absent.
        assert!(!s.contains(0));
        assert!(!s.contains(7));
    }

    #[test]
    fn iter_returns_current_members() {
        let mut s = SparseSet::new(16);
        s.insert(10);
        s.insert(3);
        s.insert(7);
        let mut members: Vec<u32> = s.iter().collect();
        members.sort_unstable();
        assert_eq!(members, vec![3, 7, 10]);
    }

    #[test]
    fn capacity_boundary() {
        let mut s = SparseSet::new(4);
        assert_eq!(s.capacity(), 4);
        // IDs 0, 1, 2, 3 are all valid.
        assert!(s.insert(0));
        assert!(s.insert(1));
        assert!(s.insert(2));
        assert!(s.insert(3));
        assert_eq!(s.len(), 4);
        assert!(s.contains(0));
        assert!(s.contains(3));
    }

    #[test]
    fn stale_sparse_entries_harmless() {
        // After a clear, the sparse array still holds values from the previous
        // population. Those values must NOT cause false positives.
        let mut s = SparseSet::new(8);
        s.insert(0);
        s.insert(1);
        s.insert(2);
        s.clear(); // len = 0; sparse[0..3] still have their old values

        // None of the formerly-present IDs should appear as members.
        for id in 0u32..8 {
            assert!(
                !s.contains(id),
                "id {id} incorrectly reported as present after clear"
            );
        }

        // Insert a disjoint set and confirm correctness.
        s.insert(4);
        assert!(s.contains(4));
        assert!(!s.contains(0));
        assert!(!s.contains(1));
        assert!(!s.contains(2));
    }
}
