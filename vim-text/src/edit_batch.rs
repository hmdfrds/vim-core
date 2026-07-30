//! Convenience builder for collecting multiple edits and converting to a ChangeSet.

use std::ops::Range;

use compact_str::CompactString;

use crate::changeset::{Change, ChangeSet, OverlapError};

/// Convenience builder for collecting multiple edits and converting to a ChangeSet.
/// Edits can be added in any order — they are sorted and validated on conversion.
pub struct EditBatch {
    edits: Vec<RawEdit>,
}

struct RawEdit {
    range: Range<usize>,
    text: CompactString,
    index: usize, // original insertion order (for overlap error reporting)
}

impl EditBatch {
    pub fn new() -> Self {
        Self { edits: Vec::new() }
    }

    /// Insert text at offset (zero-width range).
    pub fn insert(&mut self, offset: usize, text: &str) {
        let index = self.edits.len();
        self.edits.push(RawEdit {
            range: offset..offset,
            text: CompactString::from(text),
            index,
        });
    }

    /// Delete byte range [start, end).
    pub fn delete(&mut self, range: Range<usize>) {
        let index = self.edits.len();
        self.edits.push(RawEdit {
            range,
            text: CompactString::default(),
            index,
        });
    }

    /// Replace byte range with new text.
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let index = self.edits.len();
        self.edits.push(RawEdit {
            range,
            text: CompactString::from(text),
            index,
        });
    }

    /// Validate edits (no overlaps) and convert to a ChangeSet.
    /// Returns Err(OverlapError) if any two edits have overlapping ranges.
    /// Zero-width inserts at the same position are allowed (they don't overlap).
    pub fn into_changeset(self, src_len: usize) -> Result<ChangeSet, OverlapError> {
        let mut indexed: Vec<RawEdit> = self.edits;
        indexed.sort_by_key(|e| (e.range.start, e.range.end));

        // Check for overlaps (adjacent non-overlapping edits are fine)
        for window in indexed.windows(2) {
            let a = &window[0];
            let b = &window[1];
            if a.range.end > b.range.start {
                return Err(OverlapError {
                    first: a.index,
                    second: b.index,
                });
            }
        }

        // Convert to Changes for ChangeSet::from_changes
        let changes = indexed.into_iter().map(|edit| Change {
            start: edit.range.start,
            end: edit.range.end,
            text: edit.text,
        });

        Ok(ChangeSet::from_changes(src_len, changes))
    }
}

impl Default for EditBatch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::Assoc;

    #[test]
    fn empty_batch() {
        let batch = EditBatch::new();
        let cs = batch.into_changeset(10).unwrap();
        assert!(cs.is_empty());
        assert_eq!(cs.src_len(), 10);
        assert_eq!(cs.dst_len(), 10);
    }

    #[test]
    fn single_insert() {
        let mut batch = EditBatch::new();
        batch.insert(5, "hello");
        let cs = batch.into_changeset(10).unwrap();
        assert_eq!(cs.src_len(), 10);
        assert_eq!(cs.dst_len(), 15);
        assert_eq!(cs.apply_to_string("0123456789"), "01234hello56789");
    }

    #[test]
    fn single_delete() {
        let mut batch = EditBatch::new();
        batch.delete(3..7);
        let cs = batch.into_changeset(10).unwrap();
        assert_eq!(cs.apply_to_string("0123456789"), "012789");
    }

    #[test]
    fn single_replace() {
        let mut batch = EditBatch::new();
        batch.replace(3..7, "XY");
        let cs = batch.into_changeset(10).unwrap();
        assert_eq!(cs.apply_to_string("0123456789"), "012XY789");
    }

    #[test]
    fn multiple_non_overlapping() {
        let mut batch = EditBatch::new();
        batch.insert(8, "B");
        batch.insert(2, "A"); // added out of order — should be sorted
        let cs = batch.into_changeset(10).unwrap();
        assert_eq!(cs.apply_to_string("0123456789"), "01A234567B89");
    }

    #[test]
    fn overlapping_returns_error() {
        let mut batch = EditBatch::new();
        batch.delete(3..7);
        batch.delete(5..9); // overlaps with first
        let err = batch.into_changeset(10).unwrap_err();
        assert_eq!(err.first, 0); // first edit (index 0)
        assert_eq!(err.second, 1); // second edit (index 1)
    }

    #[test]
    fn adjacent_non_overlapping_ok() {
        let mut batch = EditBatch::new();
        batch.delete(0..5);
        batch.delete(5..10);
        let cs = batch.into_changeset(10).unwrap();
        assert_eq!(cs.apply_to_string("0123456789"), "");
    }

    #[test]
    fn zero_width_inserts_at_same_position() {
        let mut batch = EditBatch::new();
        batch.insert(5, "A");
        batch.insert(5, "B");
        // Both are 5..5, end(5) > start(5) is false, so no overlap
        let cs = batch.into_changeset(10).unwrap();
        // Order depends on sort stability (both have same key)
        let result = cs.apply_to_string("0123456789");
        assert!(result == "01234AB56789" || result == "01234BA56789");
    }

    #[test]
    fn multi_cursor_scenario() {
        // Simulates 3 cursors each inserting "x" at positions 5, 15, 25
        let mut batch = EditBatch::new();
        batch.insert(25, "x");
        batch.insert(5, "x");
        batch.insert(15, "x");
        let cs = batch.into_changeset(30).unwrap();
        assert_eq!(cs.src_len(), 30);
        assert_eq!(cs.dst_len(), 33);
    }

    #[test]
    fn map_pos_after_batch() {
        let mut batch = EditBatch::new();
        batch.insert(5, "xx");
        let cs = batch.into_changeset(10).unwrap();
        assert_eq!(cs.map_pos(3, Assoc::Before), 3);
        assert_eq!(cs.map_pos(5, Assoc::After), 7);
        assert_eq!(cs.map_pos(7, Assoc::Before), 9);
    }
}
