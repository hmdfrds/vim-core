//! Composable, invertible document transformation.
//!
//! A [`ChangeSet`] represents the delta between two document versions as a
//! sequence of [`Op::Retain`], [`Op::Delete`], and [`Op::Insert`] operations.

use compact_str::CompactString;

/// A single operation in a changeset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Keep `n` bytes unchanged.
    Retain(u32),
    /// Delete `n` bytes from the source.
    Delete(u32),
    /// Insert new text.
    Insert(CompactString),
}

/// Association direction for position mapping through a changeset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assoc {
    /// Map to the position before an insertion.
    Before,
    /// Map to the position after an insertion.
    After,
}

/// A composable, invertible document transformation.
///
/// Represents the delta from one document version to another as a sequence of
/// Retain/Delete/Insert operations. The invariant is:
/// - sum of Retain + Delete = src_len
/// - sum of Retain + Insert.len() = dst_len
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet {
    ops: Vec<Op>,
    src_len: u32,
    dst_len: u32,
}

/// A single change (used to build ChangeSets from sparse edits).
#[derive(Clone, Debug)]
pub struct Change {
    /// Byte offset where the change starts (in the source document).
    pub start: usize,
    /// Byte offset where the change ends (in the source document).
    pub end: usize,
    /// Replacement text (empty for pure deletions).
    pub text: CompactString,
}

/// Error when edits overlap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlapError {
    pub first: usize,
    pub second: usize,
}

impl ChangeSet {
    /// Create an identity (no-op) changeset for a document of the given length.
    pub fn identity(len: usize) -> Self {
        let len32 = len as u32;
        let ops = if len > 0 {
            vec![Op::Retain(len32)]
        } else {
            Vec::new()
        };
        Self {
            ops,
            src_len: len32,
            dst_len: len32,
        }
    }

    /// Build a changeset from sorted, non-overlapping changes.
    ///
    /// Changes must be sorted by `start` and must not overlap (i.e., each
    /// change's `start` must be >= the previous change's `end`).
    pub fn from_changes(src_len: usize, changes: impl IntoIterator<Item = Change>) -> Self {
        let mut ops = Vec::new();
        let mut current_pos: usize = 0;
        let mut dst_len: usize = 0;

        for change in changes {
            assert!(
                change.start >= current_pos,
                "changes must be sorted and non-overlapping"
            );
            assert!(change.end <= src_len, "change exceeds source length");

            // Retain bytes between the previous change and this one.
            if change.start > current_pos {
                let retain = (change.start - current_pos) as u32;
                push_op(&mut ops, Op::Retain(retain));
                dst_len += retain as usize;
            }

            // Delete bytes from the source.
            if change.end > change.start {
                let delete = (change.end - change.start) as u32;
                push_op(&mut ops, Op::Delete(delete));
            }

            // Insert replacement text.
            if !change.text.is_empty() {
                dst_len += change.text.len();
                push_op(&mut ops, Op::Insert(change.text));
            }

            current_pos = change.end;
        }

        // Trailing retain for remaining bytes.
        if current_pos < src_len {
            let retain = (src_len - current_pos) as u32;
            push_op(&mut ops, Op::Retain(retain));
            dst_len += retain as usize;
        }

        Self {
            ops,
            src_len: src_len as u32,
            dst_len: dst_len as u32,
        }
    }

    /// Map a single position through this changeset.
    ///
    /// `assoc` determines behavior at insertion boundaries:
    /// - `Assoc::Before`: position stays before the inserted text
    /// - `Assoc::After`: position moves after the inserted text
    pub fn map_pos(&self, pos: usize, assoc: Assoc) -> usize {
        let mut old_pos: usize = 0;
        let mut new_pos: usize = 0;

        for op in &self.ops {
            match op {
                Op::Retain(n) => {
                    let n = *n as usize;
                    if pos < old_pos + n {
                        return new_pos + (pos - old_pos);
                    }
                    old_pos += n;
                    new_pos += n;
                }
                Op::Delete(n) => {
                    let n = *n as usize;
                    if pos < old_pos + n {
                        // Position is inside deleted range — collapse to boundary.
                        return new_pos;
                    }
                    old_pos += n;
                }
                Op::Insert(text) => {
                    let len = text.len();
                    if pos == old_pos {
                        return match assoc {
                            Assoc::Before => new_pos,
                            Assoc::After => new_pos + len,
                        };
                    }
                    new_pos += len;
                }
            }
        }

        new_pos
    }

    /// Batch-map positions through this changeset in O(N+M) time.
    ///
    /// `positions` must be sorted by the `usize` component. After the call,
    /// each `usize` is replaced with its mapped value.
    pub fn map_positions(&self, positions: &mut [(usize, Assoc)]) {
        if positions.is_empty() {
            return;
        }

        let mut old_pos: usize = 0;
        let mut new_pos: usize = 0;
        let mut pi = 0; // index into positions

        for op in &self.ops {
            if pi >= positions.len() {
                break;
            }

            match op {
                Op::Retain(n) => {
                    let n = *n as usize;
                    while pi < positions.len() && positions[pi].0 < old_pos + n {
                        positions[pi].0 = new_pos + (positions[pi].0 - old_pos);
                        pi += 1;
                    }
                    old_pos += n;
                    new_pos += n;
                }
                Op::Delete(n) => {
                    let n = *n as usize;
                    while pi < positions.len() && positions[pi].0 < old_pos + n {
                        positions[pi].0 = new_pos;
                        pi += 1;
                    }
                    old_pos += n;
                }
                Op::Insert(text) => {
                    let len = text.len();
                    while pi < positions.len() && positions[pi].0 == old_pos {
                        positions[pi].0 = match positions[pi].1 {
                            Assoc::Before => new_pos,
                            Assoc::After => new_pos + len,
                        };
                        pi += 1;
                    }
                    new_pos += len;
                }
            }
        }

        // Any remaining positions are at or past the end.
        while pi < positions.len() {
            positions[pi].0 = new_pos + (positions[pi].0 - old_pos);
            pi += 1;
        }
    }

    /// Returns true if this changeset is a no-op (all operations are Retain).
    pub fn is_empty(&self) -> bool {
        self.ops.iter().all(|op| matches!(op, Op::Retain(_)))
    }

    /// The length of the source document this changeset applies to.
    pub fn src_len(&self) -> usize {
        self.src_len as usize
    }

    /// The length of the resulting document after applying this changeset.
    pub fn dst_len(&self) -> usize {
        self.dst_len as usize
    }

    /// The operations in this changeset.
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// Compose two changesets: `self` maps A→B, `other` maps B→C, result maps A→C.
    ///
    /// Panics if `self.dst_len != other.src_len` (the intermediate document
    /// lengths must match).
    pub fn compose(self, other: Self) -> Self {
        assert_eq!(
            self.dst_len, other.src_len,
            "compose: self.dst_len ({}) != other.src_len ({})",
            self.dst_len, other.src_len
        );

        let src_len = self.src_len;
        let dst_len = other.dst_len;

        let mut a_ops = self.ops.into_iter().peekable();
        let mut b_ops = other.ops.into_iter().peekable();

        // Partially consumed state for current ops.
        let mut a_cur: Option<Op> = None;
        let mut b_cur: Option<Op> = None;

        let mut result: Vec<Op> = Vec::new();

        loop {
            // Refill current ops from iterators if needed.
            if a_cur.is_none() {
                a_cur = a_ops.next();
            }
            if b_cur.is_none() {
                b_cur = b_ops.next();
            }

            // If both are exhausted, we're done.
            if a_cur.is_none() && b_cur.is_none() {
                break;
            }

            // Priority 1: b inserts (doesn't consume from B, produces into C).
            if let Some(Op::Insert(ref text)) = b_cur {
                let text = text.clone();
                push_op(&mut result, Op::Insert(text));
                b_cur = None;
                continue;
            }

            // Priority 2: a deletes (doesn't produce into B, consumes from A).
            if let Some(Op::Delete(n)) = a_cur {
                push_op(&mut result, Op::Delete(n));
                a_cur = None;
                continue;
            }

            // Both consume/produce B content — take the smaller chunk.
            match (a_cur.take(), b_cur.take()) {
                (Some(Op::Retain(a_n)), Some(Op::Retain(b_n))) => {
                    let take = a_n.min(b_n);
                    push_op(&mut result, Op::Retain(take));
                    if a_n > take {
                        a_cur = Some(Op::Retain(a_n - take));
                    }
                    if b_n > take {
                        b_cur = Some(Op::Retain(b_n - take));
                    }
                }
                (Some(Op::Retain(a_n)), Some(Op::Delete(b_n))) => {
                    let take = a_n.min(b_n);
                    push_op(&mut result, Op::Delete(take));
                    if a_n > take {
                        a_cur = Some(Op::Retain(a_n - take));
                    }
                    if b_n > take {
                        b_cur = Some(Op::Delete(b_n - take));
                    }
                }
                (Some(Op::Insert(a_text)), Some(Op::Retain(b_n))) => {
                    let a_len = a_text.len() as u32;
                    let take = a_len.min(b_n);
                    let take_usize = take as usize;
                    push_op(
                        &mut result,
                        Op::Insert(CompactString::from(&a_text[..take_usize])),
                    );
                    if a_len > take {
                        a_cur = Some(Op::Insert(CompactString::from(&a_text[take_usize..])));
                    }
                    if b_n > take {
                        b_cur = Some(Op::Retain(b_n - take));
                    }
                }
                (Some(Op::Insert(a_text)), Some(Op::Delete(b_n))) => {
                    // Insert then delete cancel out — emit nothing.
                    let a_len = a_text.len() as u32;
                    let take = a_len.min(b_n);
                    let take_usize = take as usize;
                    if a_len > take {
                        a_cur = Some(Op::Insert(CompactString::from(&a_text[take_usize..])));
                    }
                    if b_n > take {
                        b_cur = Some(Op::Delete(b_n - take));
                    }
                }
                (None, Some(op)) => {
                    // Shouldn't happen if lengths are correct, but handle gracefully.
                    push_op(&mut result, op);
                }
                (Some(op), None) => {
                    push_op(&mut result, op);
                }
                (None, None) => break,
                // These cases are handled above (Insert/Delete priorities).
                _ => unreachable!("compose: unhandled op combination"),
            }
        }

        ChangeSet {
            ops: result,
            src_len,
            dst_len,
        }
    }

    /// Produce the inverse changeset. When applied to the result of `self`,
    /// it restores the original document.
    ///
    /// `original` must be the source document (byte_len == `self.src_len`).
    /// Uses `VimText::slice()` to read only deleted ranges — O(delete_size)
    /// not O(doc_size).
    pub fn invert(&self, original: &crate::VimText) -> Self {
        assert_eq!(
            original.byte_len(),
            self.src_len as usize,
            "invert: original.byte_len() ({}) != src_len ({})",
            original.byte_len(),
            self.src_len
        );

        let mut result = Vec::new();
        let mut pos: usize = 0;

        for op in &self.ops {
            match op {
                Op::Retain(n) => {
                    push_op(&mut result, Op::Retain(*n));
                    pos += *n as usize;
                }
                Op::Delete(n) => {
                    // Deleted text becomes an Insert in the inverse.
                    let n_usize = *n as usize;
                    let text = original
                        .slice(pos..pos + n_usize)
                        .map(|s| s.to_cow().into_owned())
                        .unwrap_or_default();
                    push_op(&mut result, Op::Insert(CompactString::from(text)));
                    pos += n_usize;
                }
                Op::Insert(text) => {
                    // Inserted text becomes a Delete in the inverse.
                    push_op(&mut result, Op::Delete(text.len() as u32));
                }
            }
        }

        ChangeSet {
            ops: result,
            src_len: self.dst_len,
            dst_len: self.src_len,
        }
    }

    /// Apply this changeset to a string, returning the result.
    ///
    /// Panics if `text.len() != self.src_len`.
    pub fn apply_to_string(&self, text: &str) -> String {
        assert_eq!(
            text.len(),
            self.src_len as usize,
            "apply_to_string: text length ({}) != src_len ({})",
            text.len(),
            self.src_len
        );

        let mut result = String::new();
        let mut pos: usize = 0;

        for op in &self.ops {
            match op {
                Op::Retain(n) => {
                    let n = *n as usize;
                    result.push_str(&text[pos..pos + n]);
                    pos += n;
                }
                Op::Delete(n) => {
                    pos += *n as usize;
                }
                Op::Insert(t) => {
                    result.push_str(t);
                }
            }
        }

        result
    }
}

/// Helper: push an op, merging with the last if they're the same variant.
fn push_op(ops: &mut Vec<Op>, op: Op) {
    match (ops.last_mut(), &op) {
        (Some(Op::Retain(existing)), Op::Retain(n)) => {
            *existing += n;
        }
        (Some(Op::Delete(existing)), Op::Delete(n)) => {
            *existing += n;
        }
        // We don't merge adjacent Inserts because they're CompactStrings
        // and concatenation would require allocation. They'll still be correct.
        _ => {
            ops.push(op);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_noop() {
        let cs = ChangeSet::identity(10);
        assert_eq!(cs.src_len(), 10);
        assert_eq!(cs.dst_len(), 10);
        assert!(cs.is_empty());
    }

    #[test]
    fn identity_zero_length() {
        let cs = ChangeSet::identity(0);
        assert_eq!(cs.src_len(), 0);
        assert_eq!(cs.dst_len(), 0);
        assert!(cs.is_empty());
        assert_eq!(cs.ops(), &[]);
    }

    #[test]
    fn from_changes_single_insert() {
        let cs = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 2,
                end: 2,
                text: "xx".into(),
            }],
        );
        assert_eq!(
            cs.ops(),
            &[Op::Retain(2), Op::Insert("xx".into()), Op::Retain(3)]
        );
        assert_eq!(cs.src_len(), 5);
        assert_eq!(cs.dst_len(), 7);
    }

    #[test]
    fn from_changes_single_delete() {
        let cs = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 3,
                end: 6,
                text: "".into(),
            }],
        );
        assert_eq!(cs.ops(), &[Op::Retain(3), Op::Delete(3), Op::Retain(4)]);
        assert_eq!(cs.src_len(), 10);
        assert_eq!(cs.dst_len(), 7);
    }

    #[test]
    fn from_changes_replace() {
        let cs = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 2,
                end: 5,
                text: "abc".into(),
            }],
        );
        assert_eq!(
            cs.ops(),
            &[
                Op::Retain(2),
                Op::Delete(3),
                Op::Insert("abc".into()),
                Op::Retain(5)
            ]
        );
    }

    #[test]
    fn from_changes_multiple() {
        let cs = ChangeSet::from_changes(
            20,
            vec![
                Change {
                    start: 2,
                    end: 4,
                    text: "".into(),
                },
                Change {
                    start: 10,
                    end: 10,
                    text: "hi".into(),
                },
            ],
        );
        assert_eq!(cs.src_len(), 20);
        assert_eq!(cs.dst_len(), 20); // -2 + 2 = 0 net
    }

    #[test]
    fn map_pos_through_insert() {
        let cs = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 5,
                end: 5,
                text: "xx".into(),
            }],
        );
        assert_eq!(cs.map_pos(3, Assoc::Before), 3); // before insert: unchanged
        assert_eq!(cs.map_pos(5, Assoc::Before), 5); // at insert point: before
        assert_eq!(cs.map_pos(5, Assoc::After), 7); // at insert point: after
        assert_eq!(cs.map_pos(7, Assoc::Before), 9); // after insert: shifted by 2
    }

    #[test]
    fn map_pos_through_delete() {
        let cs = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 3,
                end: 6,
                text: "".into(),
            }],
        );
        assert_eq!(cs.map_pos(2, Assoc::Before), 2); // before delete: unchanged
        assert_eq!(cs.map_pos(4, Assoc::Before), 3); // inside delete: maps to start
        assert_eq!(cs.map_pos(4, Assoc::After), 3); // inside delete: maps to start
        assert_eq!(cs.map_pos(7, Assoc::Before), 4); // after delete: shifted by -3
    }

    #[test]
    fn map_positions_batch() {
        let cs = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 5,
                end: 5,
                text: "xx".into(),
            }],
        );
        let mut positions = vec![(3, Assoc::Before), (5, Assoc::After), (8, Assoc::Before)];
        cs.map_positions(&mut positions);
        assert_eq!(
            positions,
            vec![(3, Assoc::Before), (7, Assoc::After), (10, Assoc::Before)]
        );
    }

    #[test]
    fn empty_changeset() {
        let cs = ChangeSet::from_changes(0, std::iter::empty());
        assert!(cs.is_empty());
        assert_eq!(cs.src_len(), 0);
        assert_eq!(cs.dst_len(), 0);
    }

    #[test]
    fn map_pos_at_end_of_document() {
        let cs = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 10,
                end: 10,
                text: "end".into(),
            }],
        );
        assert_eq!(cs.map_pos(10, Assoc::Before), 10);
        assert_eq!(cs.map_pos(10, Assoc::After), 13);
    }

    #[test]
    fn map_pos_through_replace() {
        let cs = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 3,
                end: 6,
                text: "XY".into(),
            }],
        );
        // Before replace: unchanged
        assert_eq!(cs.map_pos(2, Assoc::Before), 2);
        // Inside deleted region: maps to deletion boundary
        assert_eq!(cs.map_pos(4, Assoc::Before), 3);
        assert_eq!(cs.map_pos(4, Assoc::After), 3);
        // After replace: shifted by (insert_len - delete_len) = 2 - 3 = -1
        assert_eq!(cs.map_pos(7, Assoc::Before), 6);
    }

    #[test]
    fn from_changes_at_start() {
        let cs = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 0,
                end: 0,
                text: "hi".into(),
            }],
        );
        assert_eq!(cs.ops(), &[Op::Insert("hi".into()), Op::Retain(5)]);
        assert_eq!(cs.src_len(), 5);
        assert_eq!(cs.dst_len(), 7);
    }

    #[test]
    fn from_changes_at_end() {
        let cs = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 5,
                end: 5,
                text: "!".into(),
            }],
        );
        assert_eq!(cs.ops(), &[Op::Retain(5), Op::Insert("!".into())]);
        assert_eq!(cs.src_len(), 5);
        assert_eq!(cs.dst_len(), 6);
    }

    #[test]
    fn from_changes_delete_all() {
        let cs = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 0,
                end: 5,
                text: "".into(),
            }],
        );
        assert_eq!(cs.ops(), &[Op::Delete(5)]);
        assert_eq!(cs.src_len(), 5);
        assert_eq!(cs.dst_len(), 0);
    }

    #[test]
    fn from_changes_normalizes_adjacent_ops() {
        // Two adjacent deletes should be merged by push_op.
        let cs = ChangeSet::from_changes(
            10,
            vec![
                Change {
                    start: 3,
                    end: 5,
                    text: "".into(),
                },
                Change {
                    start: 5,
                    end: 7,
                    text: "".into(),
                },
            ],
        );
        // Should be: Retain(3), Delete(4), Retain(3) — NOT Retain(3), Delete(2), Delete(2), Retain(3)
        let ops = cs.ops();
        assert_eq!(ops.len(), 3, "Adjacent deletes should be merged");
        assert_eq!(ops[0], Op::Retain(3));
        assert_eq!(ops[1], Op::Delete(4));
        assert_eq!(ops[2], Op::Retain(3));
    }

    // --- apply_to_string tests ---

    #[test]
    fn apply_to_string_basic() {
        let cs = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 2,
                end: 2,
                text: "XX".into(),
            }],
        );
        assert_eq!(cs.apply_to_string("hello"), "heXXllo");
    }

    #[test]
    fn apply_to_string_delete() {
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 5,
                end: 6,
                text: "".into(),
            }],
        );
        assert_eq!(cs.apply_to_string("hello world"), "helloworld");
    }

    #[test]
    fn apply_to_string_replace() {
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 6,
                end: 11,
                text: "rust".into(),
            }],
        );
        assert_eq!(cs.apply_to_string("hello world"), "hello rust");
    }

    // --- compose tests ---

    #[test]
    fn compose_two_inserts() {
        // A="hello", insert "X" at 2 -> "heXllo", then insert "Y" at 4 -> "heXlYlo"
        let cs1 = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 2,
                end: 2,
                text: "X".into(),
            }],
        );
        let cs2 = ChangeSet::from_changes(
            6,
            vec![Change {
                start: 4,
                end: 4,
                text: "Y".into(),
            }],
        );
        let composed = cs1.compose(cs2);
        assert_eq!(composed.src_len(), 5);
        assert_eq!(composed.dst_len(), 7);
        assert_eq!(composed.apply_to_string("hello"), "heXlYlo");
    }

    #[test]
    fn compose_insert_then_delete() {
        // A="hello", insert "XX" at 2 -> "heXXllo", then delete [2,4) -> "hello"
        let cs1 = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 2,
                end: 2,
                text: "XX".into(),
            }],
        );
        let cs2 = ChangeSet::from_changes(
            7,
            vec![Change {
                start: 2,
                end: 4,
                text: "".into(),
            }],
        );
        let composed = cs1.compose(cs2);
        assert_eq!(composed.src_len(), 5);
        assert_eq!(composed.dst_len(), 5);
        // Insert then delete of the same content = identity
        assert_eq!(composed.apply_to_string("hello"), "hello");
    }

    #[test]
    fn compose_delete_then_insert() {
        // A="hello world", delete [5,6) -> "helloworld", insert " " at 5 -> "hello world"
        let cs1 = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 5,
                end: 6,
                text: "".into(),
            }],
        );
        let cs2 = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 5,
                end: 5,
                text: " ".into(),
            }],
        );
        let composed = cs1.compose(cs2);
        assert_eq!(composed.apply_to_string("hello world"), "hello world");
    }

    #[test]
    fn compose_is_associative() {
        let a = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 2,
                end: 4,
                text: "XY".into(),
            }],
        );
        let b = ChangeSet::from_changes(
            10,
            vec![Change {
                start: 5,
                end: 5,
                text: "Z".into(),
            }],
        );
        let c = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 0,
                end: 1,
                text: "".into(),
            }],
        );

        let ab = a.clone().compose(b.clone());
        let ab_c = ab.compose(c.clone());

        let bc = b.compose(c);
        let a_bc = a.compose(bc);

        // Both should produce the same result on the source.
        let src = "0123456789";
        assert_eq!(ab_c.apply_to_string(src), a_bc.apply_to_string(src));
    }

    // --- invert tests ---

    #[test]
    fn invert_roundtrip_insert() {
        let original_str = "hello";
        let original = crate::VimText::from_str(original_str);
        let cs = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 2,
                end: 2,
                text: "XX".into(),
            }],
        );
        let result = cs.apply_to_string(original_str);
        assert_eq!(result, "heXXllo");
        let inverse = cs.invert(&original);
        let restored = inverse.apply_to_string(&result);
        assert_eq!(restored, original_str);
    }

    #[test]
    fn invert_roundtrip_delete() {
        let original_str = "hello world";
        let original = crate::VimText::from_str(original_str);
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 5,
                end: 6,
                text: "".into(),
            }],
        );
        let result = cs.apply_to_string(original_str);
        assert_eq!(result, "helloworld");
        let inverse = cs.invert(&original);
        let restored = inverse.apply_to_string(&result);
        assert_eq!(restored, original_str);
    }

    #[test]
    fn invert_roundtrip_replace() {
        let original_str = "hello world";
        let original = crate::VimText::from_str(original_str);
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 6,
                end: 11,
                text: "rust".into(),
            }],
        );
        let result = cs.apply_to_string(original_str);
        assert_eq!(result, "hello rust");
        let inverse = cs.invert(&original);
        let restored = inverse.apply_to_string(&result);
        assert_eq!(restored, original_str);
    }

    #[test]
    fn invert_roundtrip_multiple_changes() {
        let original_str = "the quick brown fox";
        let original = crate::VimText::from_str(original_str);
        let cs = ChangeSet::from_changes(
            19,
            vec![
                Change {
                    start: 4,
                    end: 9,
                    text: "slow".into(),
                },
                Change {
                    start: 16,
                    end: 19,
                    text: "dog".into(),
                },
            ],
        );
        let result = cs.apply_to_string(original_str);
        let inverse = cs.invert(&original);
        let restored = inverse.apply_to_string(&result);
        assert_eq!(restored, original_str);
    }

    #[test]
    fn invert_with_vim_text() {
        let original = crate::VimText::from_str("hello world");
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 5,
                end: 8,
                text: "XX".into(),
            }],
        );
        let inverted = cs.invert(&original);
        let applied = cs.apply_to_string("hello world");
        let restored = inverted.apply_to_string(&applied);
        assert_eq!(restored, "hello world");
    }

    // --- property tests ---

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn invert_always_roundtrips(
            text in "[a-z ]{5,30}",
            start in 0usize..30,
            del_len in 0usize..10,
            insert_text in "[A-Z]{0,5}",
        ) {
            let text_len = text.len();
            if text_len == 0 { return Ok(()); }
            let start = start % text_len;
            let end = (start + del_len).min(text_len);
            let cs = ChangeSet::from_changes(text_len, vec![
                Change { start, end, text: CompactString::from(insert_text.as_str()) }
            ]);
            let vt = crate::VimText::from_str(&text);
            let result = cs.apply_to_string(&text);
            let inverse = cs.invert(&vt);
            let restored = inverse.apply_to_string(&result);
            prop_assert_eq!(&restored, &text);
        }
    }
}
