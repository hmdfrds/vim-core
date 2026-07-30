//! Lookaround memo tables for backtracker optimization.
//!
//! Caches lookaround evaluation results by (position, lookaround_id)
//! to avoid redundant re-evaluation during alternation backtracking.

use ahash::AHashMap;

/// Caches lookaround results by `(position, lookaround_id)`.
///
/// During backtracker execution, the same lookaround assertion may be
/// evaluated at the same position multiple times when alternation
/// branches are explored. Caching prevents redundant NFA sub-searches.
///
/// The memo table is cleared per top-level search call.
#[derive(Debug)]
pub(crate) struct LookaroundMemo {
    /// Map from (byte_position, lookaround_id) to the evaluation result.
    results: AHashMap<(usize, u16), bool>,
}

impl LookaroundMemo {
    /// Create an empty memo table.
    pub(crate) fn new() -> Self {
        Self {
            results: AHashMap::new(),
        }
    }

    /// Look up a cached lookaround result.
    ///
    /// Returns `Some(true)` if the lookaround matched at this position,
    /// `Some(false)` if it did not, or `None` if not yet evaluated.
    #[inline]
    pub(crate) fn get(&self, position: usize, lookaround_id: u16) -> Option<bool> {
        self.results.get(&(position, lookaround_id)).copied()
    }

    /// Store a lookaround evaluation result.
    #[inline]
    pub(crate) fn insert(&mut self, position: usize, lookaround_id: u16, result: bool) {
        self.results.insert((position, lookaround_id), result);
    }

    /// Clear all cached results. Called at the start of each top-level search.
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.results.clear();
    }

    /// Number of cached entries.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.results.len()
    }

    /// Approximate heap memory usage in bytes.
    pub(crate) fn memory_usage(&self) -> usize {
        use std::mem::size_of;
        self.results.capacity() * (size_of::<(usize, u16)>() + size_of::<bool>() + size_of::<u64>())
    }
}

impl Default for LookaroundMemo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_retrieve() {
        let mut memo = LookaroundMemo::new();
        memo.insert(10, 0, true);
        memo.insert(10, 1, false);
        memo.insert(20, 0, false);

        assert_eq!(memo.get(10, 0), Some(true));
        assert_eq!(memo.get(10, 1), Some(false));
        assert_eq!(memo.get(20, 0), Some(false));
        assert_eq!(memo.get(20, 1), None);
    }

    #[test]
    fn clear_empties_table() {
        let mut memo = LookaroundMemo::new();
        memo.insert(0, 0, true);
        memo.insert(5, 1, false);
        assert_eq!(memo.len(), 2);

        memo.clear();
        assert_eq!(memo.len(), 0);
        assert_eq!(memo.get(0, 0), None);
    }

    #[test]
    fn overwrite_existing() {
        let mut memo = LookaroundMemo::new();
        memo.insert(10, 0, true);
        assert_eq!(memo.get(10, 0), Some(true));

        memo.insert(10, 0, false);
        assert_eq!(memo.get(10, 0), Some(false));
    }

    #[test]
    fn memory_usage_grows() {
        let mut memo = LookaroundMemo::new();
        let initial = memo.memory_usage();
        for i in 0..100 {
            memo.insert(i, 0, i % 2 == 0);
        }
        assert!(memo.memory_usage() >= initial);
    }

    #[test]
    fn default_is_empty() {
        let memo = LookaroundMemo::default();
        assert_eq!(memo.len(), 0);
        assert_eq!(memo.get(0, 0), None);
    }
}
