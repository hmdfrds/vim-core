//! Position mapping through a changeset.
//!
//! Given a byte position in the **input** document, `map_pos` computes the
//! corresponding position in the **output** document. The [`Assoc`] enum
//! controls cursor stickiness at insertion boundaries.
//!
//! `map_positions` maps a sorted batch of positions in O(N+M) time,
//! where N = number of ops and M = number of positions.

use crate::primitives::Offset;

use super::change_set::ChangeSet;
use super::text_op::TextOp;

// Re-export the canonical is_word_char from text_util for cursor-affinity decisions.
pub use crate::primitives::text_util::is_word_char;

/// Cursor affinity when mapping a position that falls at an insertion point.
///
/// When text is inserted at position P:
/// - `Before`: cursor stays at P (before the new text)
/// - `After`: cursor moves to P + inserted_len (after the new text)
/// - `BeforeSticky` / `AfterSticky`: preserve cursor offset within a
///   replacement (Insert+Delete sequence). Falls back to
///   `Before` / `After` for pure insertions.
/// - `AfterWord` / `BeforeWord`: decide based on whether the inserted text
///   starts/ends with a word character. Useful for auto-pair insertion.
///
/// `#[non_exhaustive]` allows future variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Assoc {
    /// Cursor stays before inserted text.
    #[default]
    Before,
    /// Cursor moves after inserted text.
    After,
    /// Like `Before` for pure insertions. In a replacement (Insert+Delete),
    /// preserves the cursor's byte offset within the deleted region, clamped
    /// to the insertion length.
    BeforeSticky,
    /// Like `After` for pure insertions. In a replacement (Insert+Delete),
    /// preserves the cursor's byte offset within the deleted region, clamped
    /// to the insertion length.
    AfterSticky,
    /// If the first byte of the inserted text is a word character, behave
    /// like `After`; otherwise behave like `Before`. Useful for auto-pair
    /// scenarios where typing `(` inserts `()` and the cursor should stay
    /// between the parens.
    AfterWord,
    /// If the last byte of the inserted text is a word character, behave
    /// like `Before`; otherwise behave like `After`. The mirror of
    /// `AfterWord`.
    BeforeWord,
}

impl ChangeSet {
    /// Map a single byte position through this changeset.
    ///
    /// `pos` is a byte offset in the **input** document.
    /// Returns the corresponding offset in the **output** document.
    ///
    /// `assoc` controls where the position lands when it falls exactly
    /// at an insertion point.
    ///
    /// Positions beyond `input_len` are clamped.
    ///
    /// # Complexity
    ///
    /// O(|ops|) — single pass.
    #[must_use]
    pub fn map_pos(&self, pos: usize, assoc: Assoc) -> usize {
        let mut old_pos = 0; // position in input document
        let mut new_pos = 0; // position in output document

        // Fast path: if pos is beyond the document, return output_len
        if pos >= self.input_len() {
            return self.output_len();
        }

        // For sticky variants we track two things:
        //
        // 1. `pending_sticky` — set when pos == old_pos at an Insert with
        //    sticky assoc. Defers the return until we see whether a Delete
        //    follows (replacement) or a Retain (pure insert).
        //
        // 2. `prev_insert_len` — length of the immediately preceding Insert.
        //    Used in the Delete branch for positions *inside* the deleted
        //    region of a replacement (Insert+Delete).
        let mut pending_sticky: Option<usize> = None;
        let mut prev_insert_len: Option<usize> = None;

        for op in self.ops() {
            match op {
                TextOp::Retain(n) => {
                    // Pending sticky + Retain = pure insert, no replacement.
                    if let Some(insert_len) = pending_sticky.take() {
                        return match assoc {
                            Assoc::BeforeSticky => new_pos - insert_len,
                            Assoc::AfterSticky => new_pos,
                            Assoc::Before | Assoc::After | Assoc::AfterWord | Assoc::BeforeWord => {
                                new_pos
                            }
                        };
                    }

                    prev_insert_len = None;

                    if pos < old_pos + n {
                        return new_pos + (pos - old_pos);
                    }
                    old_pos += n;
                    new_pos += n;
                }
                TextOp::Insert(t) => {
                    // Pending sticky + another Insert = unexpected; fall back.
                    if let Some(insert_len) = pending_sticky.take() {
                        return match assoc {
                            Assoc::BeforeSticky => new_pos - insert_len,
                            Assoc::AfterSticky => new_pos,
                            Assoc::Before | Assoc::After | Assoc::AfterWord | Assoc::BeforeWord => {
                                new_pos
                            }
                        };
                    }

                    // Position is exactly at the insertion point
                    if old_pos == pos {
                        match assoc {
                            Assoc::Before => return new_pos,
                            Assoc::After => {
                                prev_insert_len = Some(t.len());
                                new_pos += t.len();
                                continue;
                            }
                            Assoc::AfterWord => {
                                if t.chars().next().is_some_and(is_word_char) {
                                    prev_insert_len = Some(t.len());
                                    new_pos += t.len();
                                    continue;
                                }
                                return new_pos;
                            }
                            Assoc::BeforeWord => {
                                if t.chars().last().is_some_and(is_word_char) {
                                    return new_pos;
                                }
                                prev_insert_len = Some(t.len());
                                new_pos += t.len();
                                continue;
                            }
                            Assoc::BeforeSticky | Assoc::AfterSticky => {
                                pending_sticky = Some(t.len());
                                prev_insert_len = Some(t.len());
                                new_pos += t.len();
                                continue;
                            }
                        }
                    }
                    prev_insert_len = Some(t.len());
                    new_pos += t.len();
                }
                TextOp::Delete(n) => {
                    // Resolve pending sticky: pos == old_pos was at the
                    // Insert boundary, now a Delete follows (replacement).
                    // offset = 0 → map to start of inserted text.
                    if let Some(insert_len) = pending_sticky.take() {
                        if pos < old_pos + n {
                            // pos is within the deleted region (offset 0)
                            return new_pos - insert_len;
                        }
                        // pos is beyond this Delete — continue.
                        prev_insert_len = None;
                        old_pos += n;
                        continue;
                    }

                    if pos < old_pos + n {
                        // Position falls within deleted region.
                        // For sticky + preceding Insert → replacement.
                        if let Some(insert_len) = prev_insert_len {
                            if matches!(assoc, Assoc::BeforeSticky | Assoc::AfterSticky) {
                                let offset_in_deleted = pos - old_pos;
                                let clamped = offset_in_deleted.min(insert_len);
                                return (new_pos - insert_len) + clamped;
                            }
                        }
                        return new_pos;
                    }
                    prev_insert_len = None;
                    old_pos += n;
                }
            }
        }

        // End of ops with pending sticky — fall back (pure insert at end).
        if let Some(insert_len) = pending_sticky {
            return match assoc {
                Assoc::BeforeSticky => new_pos - insert_len,
                Assoc::AfterSticky => new_pos,
                Assoc::Before | Assoc::After | Assoc::AfterWord | Assoc::BeforeWord => new_pos,
            };
        }

        new_pos
    }

    /// Map an `Offset` through this changeset.
    ///
    /// Convenience wrapper around [`map_pos`](Self::map_pos).
    #[must_use]
    pub fn map_offset(&self, offset: Offset, assoc: Assoc) -> Offset {
        Offset::new(self.map_pos(offset.get(), assoc))
    }

    /// Map a sorted batch of positions in O(N+M) time.
    ///
    /// `positions` must be sorted in ascending order. Each position is
    /// updated in-place to its mapped value. The mapped positions will
    /// remain sorted (monotonicity).
    ///
    /// # Complexity
    ///
    /// O(|ops| + |positions|) — both are walked once.
    #[allow(
        clippy::indexing_slicing,
        reason = "O(N+M) two-pointer algorithm — idx < positions.len() and \
                  op_idx < ops.len() are loop invariants, checked in while conditions"
    )]
    pub fn map_positions(&self, positions: &mut [usize], assoc: Assoc) {
        if positions.is_empty() {
            return;
        }

        let mut old_pos: usize = 0;
        let mut new_pos: usize = 0;
        let mut idx = 0;
        let ops = self.ops();
        let mut op_idx = 0;

        // For sticky variants: track the range of positions that were at an
        // Insert boundary and need deferred resolution. `sticky_start..idx`
        // is the range of positions encoded as sentinels.
        let mut sticky_range: Option<(usize, usize, usize)> = None; // (start_idx, end_idx, insert_len)

        while idx < positions.len() && op_idx < ops.len() {
            match &ops[op_idx] {
                TextOp::Retain(n) => {
                    let span_end = old_pos + n;

                    // Sticky + Retain = pure insert, fall back.
                    if let Some((start, end, ins_len)) = sticky_range.take() {
                        Self::resolve_sticky_range(
                            positions, start, end, new_pos, ins_len, assoc, false,
                        );
                    }

                    // Map all positions that fall within this retain
                    while idx < positions.len() && positions[idx] < span_end {
                        positions[idx] = new_pos + (positions[idx] - old_pos);
                        idx += 1;
                    }
                    old_pos = span_end;
                    new_pos += n;
                    op_idx += 1;
                }
                TextOp::Insert(t) => {
                    let t_len = t.len();

                    // Sticky + Insert = unexpected, fall back.
                    if let Some((start, end, ins_len)) = sticky_range.take() {
                        Self::resolve_sticky_range(
                            positions, start, end, new_pos, ins_len, assoc, false,
                        );
                    }

                    // Handle positions exactly at the insertion point
                    if idx < positions.len() && positions[idx] == old_pos {
                        match assoc {
                            Assoc::Before => {
                                while idx < positions.len() && positions[idx] == old_pos {
                                    positions[idx] = new_pos;
                                    idx += 1;
                                }
                            }
                            Assoc::After => {
                                while idx < positions.len() && positions[idx] == old_pos {
                                    positions[idx] = new_pos + t_len;
                                    idx += 1;
                                }
                            }
                            Assoc::AfterWord => {
                                if t.chars().next().is_some_and(is_word_char) {
                                    while idx < positions.len() && positions[idx] == old_pos {
                                        positions[idx] = new_pos + t_len;
                                        idx += 1;
                                    }
                                } else {
                                    while idx < positions.len() && positions[idx] == old_pos {
                                        positions[idx] = new_pos;
                                        idx += 1;
                                    }
                                }
                            }
                            Assoc::BeforeWord => {
                                if t.chars().last().is_some_and(is_word_char) {
                                    while idx < positions.len() && positions[idx] == old_pos {
                                        positions[idx] = new_pos;
                                        idx += 1;
                                    }
                                } else {
                                    while idx < positions.len() && positions[idx] == old_pos {
                                        positions[idx] = new_pos + t_len;
                                        idx += 1;
                                    }
                                }
                            }
                            Assoc::BeforeSticky | Assoc::AfterSticky => {
                                // Record the range of positions at the insert
                                // boundary. Leave their values unchanged for
                                // now — they'll be resolved in Delete or
                                // fallen back in Retain.
                                let start = idx;
                                while idx < positions.len() && positions[idx] == old_pos {
                                    idx += 1;
                                }
                                sticky_range = Some((start, idx, t_len));
                            }
                        }
                    }
                    new_pos += t_len;
                    op_idx += 1;
                }
                TextOp::Delete(n) => {
                    let span_end = old_pos + n;

                    // Resolve sticky from preceding Insert — this IS a
                    // replacement. Offset = 0 → start of inserted text.
                    if let Some((start, end, ins_len)) = sticky_range.take() {
                        Self::resolve_sticky_range(
                            positions, start, end, new_pos, ins_len, assoc, true,
                        );
                    }

                    match assoc {
                        Assoc::BeforeSticky | Assoc::AfterSticky => {
                            // Positions within Delete after Insert (replacement):
                            // map using offset into the preceding insert.
                            // We need to know if there was actually a preceding
                            // insert. Check if op_idx > 0 and previous op was Insert.
                            let prev_is_insert =
                                op_idx > 0 && matches!(&ops[op_idx - 1], TextOp::Insert(_));
                            if prev_is_insert {
                                // Recover insert_len from new_pos computation:
                                // new_pos was advanced by insert_len during Insert.
                                // We can get insert_len from the previous op.
                                let insert_len = match &ops[op_idx - 1] {
                                    TextOp::Insert(t) => t.len(),
                                    _ => 0,
                                };
                                while idx < positions.len() && positions[idx] < span_end {
                                    let offset_in_deleted = positions[idx] - old_pos;
                                    let clamped = offset_in_deleted.min(insert_len);
                                    positions[idx] = (new_pos - insert_len) + clamped;
                                    idx += 1;
                                }
                            } else {
                                // No preceding insert — collapse like normal.
                                while idx < positions.len() && positions[idx] < span_end {
                                    positions[idx] = new_pos;
                                    idx += 1;
                                }
                            }
                        }
                        Assoc::Before | Assoc::After | Assoc::AfterWord | Assoc::BeforeWord => {
                            while idx < positions.len() && positions[idx] < span_end {
                                positions[idx] = new_pos;
                                idx += 1;
                            }
                        }
                    }
                    old_pos = span_end;
                    op_idx += 1;
                }
            }
        }

        // Resolve any remaining sticky range
        if let Some((start, end, ins_len)) = sticky_range.take() {
            Self::resolve_sticky_range(positions, start, end, new_pos, ins_len, assoc, false);
        }

        // Any remaining positions beyond the last op → map to output_len
        while idx < positions.len() {
            positions[idx] = self.output_len();
            idx += 1;
        }
    }

    /// Map a sorted batch of positions with per-position [`Assoc`] in O(N+M) time.
    ///
    /// Each element is `(position, assoc)`. Positions must be sorted ascending
    /// by the `position` field. Each position is updated in-place using its own
    /// `Assoc` for insertion-boundary decisions.
    ///
    /// # Complexity
    ///
    /// O(|ops| + |positions|) — identical to [`map_positions`](Self::map_positions).
    #[allow(
        clippy::indexing_slicing,
        reason = "O(N+M) two-pointer algorithm — idx < positions.len() and \
                  op_idx < ops.len() are loop invariants, checked in while conditions"
    )]
    pub fn map_positions_with_assoc(&self, positions: &mut [(usize, Assoc)]) {
        if positions.is_empty() {
            return;
        }

        let mut old_pos: usize = 0;
        let mut new_pos: usize = 0;
        let mut idx = 0;
        let ops = self.ops();
        let mut op_idx = 0;

        while idx < positions.len() && op_idx < ops.len() {
            match &ops[op_idx] {
                TextOp::Retain(n) => {
                    let span_end = old_pos + n;
                    while idx < positions.len() && positions[idx].0 < span_end {
                        positions[idx].0 = new_pos + (positions[idx].0 - old_pos);
                        idx += 1;
                    }
                    old_pos = span_end;
                    new_pos += n;
                    op_idx += 1;
                }
                TextOp::Insert(t) => {
                    let t_len = t.len();
                    while idx < positions.len() && positions[idx].0 == old_pos {
                        let assoc = positions[idx].1;
                        positions[idx].0 = match assoc {
                            Assoc::Before => new_pos,
                            Assoc::After => new_pos + t_len,
                            Assoc::AfterWord => {
                                if t.chars().next().is_some_and(is_word_char) {
                                    new_pos + t_len
                                } else {
                                    new_pos
                                }
                            }
                            Assoc::BeforeWord => {
                                if t.chars().last().is_some_and(is_word_char) {
                                    new_pos
                                } else {
                                    new_pos + t_len
                                }
                            }
                            Assoc::BeforeSticky | Assoc::AfterSticky => {
                                let next_op = ops.get(op_idx + 1);
                                if let Some(TextOp::Delete(_)) = next_op {
                                    new_pos
                                } else {
                                    match assoc {
                                        Assoc::BeforeSticky => new_pos,
                                        _ => new_pos + t_len,
                                    }
                                }
                            }
                        };
                        idx += 1;
                    }
                    new_pos += t_len;
                    op_idx += 1;
                }
                TextOp::Delete(n) => {
                    let span_end = old_pos + n;
                    let prev_is_insert =
                        op_idx > 0 && matches!(&ops[op_idx - 1], TextOp::Insert(_));
                    let insert_len = if prev_is_insert {
                        match &ops[op_idx - 1] {
                            TextOp::Insert(t) => t.len(),
                            _ => 0,
                        }
                    } else {
                        0
                    };

                    while idx < positions.len() && positions[idx].0 < span_end {
                        let assoc = positions[idx].1;
                        if prev_is_insert
                            && matches!(assoc, Assoc::BeforeSticky | Assoc::AfterSticky)
                        {
                            let offset_in_deleted = positions[idx].0 - old_pos;
                            let clamped = offset_in_deleted.min(insert_len);
                            positions[idx].0 = (new_pos - insert_len) + clamped;
                        } else {
                            positions[idx].0 = new_pos;
                        }
                        idx += 1;
                    }
                    old_pos = span_end;
                    op_idx += 1;
                }
            }
        }

        while idx < positions.len() {
            positions[idx].0 = self.output_len();
            idx += 1;
        }
    }

    /// Resolve a range of sticky positions that were at an Insert boundary.
    ///
    /// Positions `start..end` were at the Insert's `old_pos` and need
    /// resolution. If `is_replacement` is true, a Delete followed the Insert
    /// (replacement pattern), so map to offset 0 within the insertion.
    /// Otherwise, fall back to Before/After behavior.
    #[allow(
        clippy::indexing_slicing,
        reason = "start..end range was validated during Insert processing"
    )]
    fn resolve_sticky_range(
        positions: &mut [usize],
        start: usize,
        end: usize,
        new_pos: usize,
        insert_len: usize,
        assoc: Assoc,
        is_replacement: bool,
    ) {
        let target = if is_replacement {
            // Offset 0 → start of inserted text
            new_pos - insert_len
        } else {
            // Pure insert — fall back to Before/After
            match assoc {
                Assoc::BeforeSticky => new_pos - insert_len,
                Assoc::AfterSticky
                | Assoc::Before
                | Assoc::After
                | Assoc::AfterWord
                | Assoc::BeforeWord => new_pos,
            }
        };
        for pos in &mut positions[start..end] {
            *pos = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_pos_identity() {
        let cs = ChangeSet::identity(10);
        for i in 0..=10 {
            assert_eq!(cs.map_pos(i, Assoc::Before), i);
            assert_eq!(cs.map_pos(i, Assoc::After), i);
        }
    }

    #[test]
    fn map_pos_insert_before() {
        // "hello" -> "heXXllo" (insert "XX" at 2)
        let cs = ChangeSet::from_insert(5, 2, "XX");
        assert_eq!(cs.map_pos(0, Assoc::Before), 0);
        assert_eq!(cs.map_pos(1, Assoc::Before), 1);
        assert_eq!(cs.map_pos(2, Assoc::Before), 2); // Before insertion
        assert_eq!(cs.map_pos(3, Assoc::Before), 5); // After the insert ops
        assert_eq!(cs.map_pos(4, Assoc::Before), 6);
    }

    #[test]
    fn map_pos_insert_after() {
        let cs = ChangeSet::from_insert(5, 2, "XX");
        assert_eq!(cs.map_pos(0, Assoc::After), 0);
        assert_eq!(cs.map_pos(1, Assoc::After), 1);
        assert_eq!(cs.map_pos(2, Assoc::After), 4); // After insertion
        assert_eq!(cs.map_pos(3, Assoc::After), 5);
    }

    #[test]
    fn map_pos_delete() {
        // "hello" -> "ho" (delete bytes 1..4)
        let cs = ChangeSet::from_delete(5, 1, 4);
        assert_eq!(cs.map_pos(0, Assoc::Before), 0);
        assert_eq!(cs.map_pos(1, Assoc::Before), 1); // Collapsed to deletion start
        assert_eq!(cs.map_pos(2, Assoc::Before), 1); // Within deleted region
        assert_eq!(cs.map_pos(3, Assoc::Before), 1); // Within deleted region
        assert_eq!(cs.map_pos(4, Assoc::Before), 1); // After deletion
                                                     // Position 4 in the input is 'o'. After deleting [1..4],
                                                     // "hello" becomes "h" + "o" = "ho", so:
                                                     // pos 0 -> 0 (h)
                                                     // pos 1,2,3 -> 1 (deleted region collapses)
                                                     // pos 4 -> 1 (o maps to index 1 in "ho")
        assert_eq!(cs.map_pos(4, Assoc::Before), 1);
    }

    #[test]
    fn map_pos_replace() {
        // "hello" -> "hXYo" (replace [1..4] with "XY")
        let cs = ChangeSet::from_replace(5, 1, 4, "XY");
        assert_eq!(cs.map_pos(0, Assoc::Before), 0); // h
                                                     // Position 1 is at the insert+delete boundary:
                                                     // ops: Retain(1), Insert("XY"), Delete(3), Retain(1)
                                                     // At pos 1: Insert is at old_pos=1, so assoc matters
        assert_eq!(cs.map_pos(1, Assoc::Before), 1);
        assert_eq!(cs.map_pos(1, Assoc::After), 3); // After "XY"
        assert_eq!(cs.map_pos(2, Assoc::Before), 3); // Within deleted region -> 3 (after insert)
        assert_eq!(cs.map_pos(3, Assoc::Before), 3);
        assert_eq!(cs.map_pos(4, Assoc::Before), 3); // 'o' -> index 3 in "hXYo"
    }

    #[test]
    fn map_pos_beyond_input() {
        let cs = ChangeSet::from_insert(5, 2, "XX");
        assert_eq!(cs.map_pos(100, Assoc::Before), cs.output_len());
    }

    #[test]
    fn map_offset() {
        let cs = ChangeSet::from_insert(5, 2, "XX");
        let mapped = cs.map_offset(Offset::new(3), Assoc::Before);
        assert_eq!(mapped, Offset::new(5));
    }

    #[test]
    fn map_positions_batch() {
        // "hello" -> "heXXllo" (insert "XX" at 2)
        let cs = ChangeSet::from_insert(5, 2, "XX");
        let mut positions = vec![0, 1, 2, 3, 4, 5];
        cs.map_positions(&mut positions, Assoc::Before);
        assert_eq!(positions, vec![0, 1, 2, 5, 6, 7]);
    }

    #[test]
    fn map_positions_batch_after() {
        let cs = ChangeSet::from_insert(5, 2, "XX");
        let mut positions = vec![0, 1, 2, 3, 4, 5];
        cs.map_positions(&mut positions, Assoc::After);
        assert_eq!(positions, vec![0, 1, 4, 5, 6, 7]);
    }

    #[test]
    fn map_positions_empty() {
        let cs = ChangeSet::from_insert(5, 2, "XX");
        let mut positions: Vec<usize> = vec![];
        cs.map_positions(&mut positions, Assoc::Before);
        assert!(positions.is_empty());
    }

    #[test]
    fn map_positions_monotonicity() {
        // After mapping, positions should still be sorted
        let cs = ChangeSet::from_changes(20, [(3, 7, Some("X")), (10, 12, None)]);
        let mut positions: Vec<usize> = (0..=20).collect();
        cs.map_positions(&mut positions, Assoc::Before);
        for i in 1..positions.len() {
            assert!(
                positions[i] >= positions[i - 1],
                "monotonicity violated at index {i}: {} < {}",
                positions[i],
                positions[i - 1]
            );
        }
    }

    #[test]
    fn map_positions_delete() {
        // "abcdefghij" -> delete [2..5] -> "abfghij" (7 bytes)
        let cs = ChangeSet::from_delete(10, 2, 5);
        let mut positions = vec![0, 1, 2, 3, 4, 5, 6, 9];
        cs.map_positions(&mut positions, Assoc::Before);
        // 0->0, 1->1, 2->2 (collapsed), 3->2 (deleted), 4->2 (deleted),
        // 5->2, 6->3, 9->6
        assert_eq!(positions, vec![0, 1, 2, 2, 2, 2, 3, 6]);
    }

    // --- is_word_char ---

    #[test]
    fn test_is_word_char() {
        assert!(is_word_char('a'));
        assert!(is_word_char('z'));
        assert!(is_word_char('A'));
        assert!(is_word_char('Z'));
        assert!(is_word_char('0'));
        assert!(is_word_char('9'));
        assert!(is_word_char('_'));
        assert!(!is_word_char('('));
        assert!(!is_word_char(')'));
        assert!(!is_word_char(' '));
        assert!(!is_word_char('.'));
        // Unicode word characters
        assert!(is_word_char('é'));
        assert!(is_word_char('ü'));
        assert!(is_word_char('字'));
        assert!(!is_word_char('•'));
        assert!(!is_word_char('—'));
    }

    // --- AfterWord / BeforeWord tests ---

    #[test]
    fn after_word_insert_parens() {
        // Insert "()" — first char '(' is not a word char → Before behavior
        let cs = ChangeSet::from_insert(5, 2, "()");
        assert_eq!(cs.map_pos(2, Assoc::AfterWord), 2); // stays before
    }

    #[test]
    fn after_word_insert_word() {
        // Insert "foo" — first char 'f' is a word char → After behavior
        let cs = ChangeSet::from_insert(5, 2, "foo");
        assert_eq!(cs.map_pos(2, Assoc::AfterWord), 5); // moves after "foo"
    }

    #[test]
    fn before_word_insert_parens() {
        // Insert "()" — last char ')' is not a word char → After behavior
        let cs = ChangeSet::from_insert(5, 2, "()");
        assert_eq!(cs.map_pos(2, Assoc::BeforeWord), 4); // moves after "()"
    }

    #[test]
    fn before_word_insert_word() {
        // Insert "foo" — last char 'o' is a word char → Before behavior
        let cs = ChangeSet::from_insert(5, 2, "foo");
        assert_eq!(cs.map_pos(2, Assoc::BeforeWord), 2); // stays before
    }

    // --- BeforeSticky / AfterSticky tests ---

    #[test]
    fn sticky_replace_preserves_offset() {
        // "abcde" -> replace [1..4] with "xyz" → "axyze"
        // ops: Retain(1), Insert("xyz"), Delete(3), Retain(1)
        // Cursor at position 2 (offset 1 into deleted region)
        // → should map to offset 1 into "xyz" = position 2
        let cs = ChangeSet::from_replace(5, 1, 4, "xyz");
        assert_eq!(cs.map_pos(2, Assoc::BeforeSticky), 2);
        assert_eq!(cs.map_pos(2, Assoc::AfterSticky), 2);
    }

    #[test]
    fn sticky_replace_clamped_offset() {
        // "abcde" -> replace [1..4] with "xy" → "axye"
        // ops: Retain(1), Insert("xy"), Delete(3), Retain(1)
        // Cursor at position 3 (offset 2 into deleted region, insert is 2 bytes)
        // → clamped to min(2, 2) = 2 → position 1 + 2 = 3
        let cs = ChangeSet::from_replace(5, 1, 4, "xy");
        assert_eq!(cs.map_pos(3, Assoc::BeforeSticky), 3);
    }

    #[test]
    fn sticky_delete_no_insert_falls_back() {
        // Pure delete — falls back; positions inside deleted region collapse.
        // "abcde" -> delete [1..4] → "ae"
        // ops: Retain(1), Delete(3), Retain(1)
        let cs = ChangeSet::from_delete(5, 1, 4);
        assert_eq!(cs.map_pos(2, Assoc::BeforeSticky), 1);
        assert_eq!(cs.map_pos(2, Assoc::AfterSticky), 1);
    }

    #[test]
    fn sticky_pure_insert_before() {
        // Pure insert (no delete) — BeforeSticky acts like Before
        let cs = ChangeSet::from_insert(5, 2, "XX");
        assert_eq!(cs.map_pos(2, Assoc::BeforeSticky), 2);
    }

    #[test]
    fn sticky_pure_insert_after() {
        // Pure insert (no delete) — AfterSticky acts like After
        let cs = ChangeSet::from_insert(5, 2, "XX");
        assert_eq!(cs.map_pos(2, Assoc::AfterSticky), 4);
    }

    // --- Batch tests for new variants ---

    #[test]
    fn map_positions_after_word() {
        let cs = ChangeSet::from_insert(5, 2, "()");
        let mut positions = vec![0, 2, 3];
        cs.map_positions(&mut positions, Assoc::AfterWord);
        // pos 0 → 0, pos 2 → 2 (Before, '(' not word), pos 3 → 5
        assert_eq!(positions, vec![0, 2, 5]);
    }

    #[test]
    fn map_positions_before_word() {
        let cs = ChangeSet::from_insert(5, 2, "()");
        let mut positions = vec![0, 2, 3];
        cs.map_positions(&mut positions, Assoc::BeforeWord);
        // pos 0 → 0, pos 2 → 4 (After, ')' not word), pos 3 → 5
        assert_eq!(positions, vec![0, 4, 5]);
    }

    #[test]
    fn map_positions_sticky_replace() {
        // "abcde" -> replace [1..4] with "xyz" → "axyze"
        let cs = ChangeSet::from_replace(5, 1, 4, "xyz");
        let mut positions = vec![0, 2, 4];
        cs.map_positions(&mut positions, Assoc::BeforeSticky);
        // pos 0 → 0, pos 2 → 2 (offset 1 in "xyz"), pos 4 → 4 ('e')
        assert_eq!(positions, vec![0, 2, 4]);
    }

    #[test]
    fn map_positions_sticky_boundary() {
        // "abcde" -> replace [1..4] with "xyz" → "axyze"
        // Position 1 is at the insert boundary (offset 0)
        let cs = ChangeSet::from_replace(5, 1, 4, "xyz");
        let mut positions = vec![1];
        cs.map_positions(&mut positions, Assoc::BeforeSticky);
        assert_eq!(positions, vec![1]);
    }

    #[test]
    fn map_positions_monotonicity_all_variants() {
        let cs = ChangeSet::from_changes(20, [(3, 7, Some("X")), (10, 12, None)]);
        for assoc in [
            Assoc::Before,
            Assoc::After,
            Assoc::BeforeSticky,
            Assoc::AfterSticky,
            Assoc::AfterWord,
            Assoc::BeforeWord,
        ] {
            let mut positions: Vec<usize> = (0..=20).collect();
            cs.map_positions(&mut positions, assoc);
            for i in 1..positions.len() {
                assert!(
                    positions[i] >= positions[i - 1],
                    "monotonicity violated for {assoc:?} at index {i}: {} < {}",
                    positions[i],
                    positions[i - 1]
                );
            }
        }
    }

    #[test]
    fn map_positions_batch_matches_single() {
        // Verify batch results match single-position mapping for all variants
        let cs = ChangeSet::from_replace(10, 3, 7, "XY");
        for assoc in [
            Assoc::Before,
            Assoc::After,
            Assoc::BeforeSticky,
            Assoc::AfterSticky,
            Assoc::AfterWord,
            Assoc::BeforeWord,
        ] {
            let expected: Vec<usize> = (0..=10).map(|p| cs.map_pos(p, assoc)).collect();
            let mut batch: Vec<usize> = (0..=10).collect();
            cs.map_positions(&mut batch, assoc);
            assert_eq!(batch, expected, "batch vs single mismatch for {assoc:?}");
        }
    }

    #[test]
    fn map_positions_with_assoc_uniform_matches_shared() {
        let cs = ChangeSet::from_replace(10, 3, 7, "XY");
        for assoc in [
            Assoc::Before,
            Assoc::After,
            Assoc::BeforeSticky,
            Assoc::AfterSticky,
            Assoc::AfterWord,
            Assoc::BeforeWord,
        ] {
            let mut shared: Vec<usize> = (0..=10).collect();
            cs.map_positions(&mut shared, assoc);

            let mut per_pos: Vec<(usize, Assoc)> = (0..=10).map(|p| (p, assoc)).collect();
            cs.map_positions_with_assoc(&mut per_pos);

            let per_pos_vals: Vec<usize> = per_pos.iter().map(|&(p, _)| p).collect();
            assert_eq!(
                per_pos_vals, shared,
                "per-position vs shared mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_positions_with_assoc_mixed_at_insert() {
        let cs = ChangeSet::from_insert(10, 5, "XX");
        let mut positions = vec![(5, Assoc::Before), (5, Assoc::After)];
        cs.map_positions_with_assoc(&mut positions);
        assert_eq!(positions[0].0, 5);
        assert_eq!(positions[1].0, 7);
    }

    #[test]
    fn map_positions_with_assoc_empty() {
        let cs = ChangeSet::from_insert(5, 2, "XX");
        let mut positions: Vec<(usize, Assoc)> = vec![];
        cs.map_positions_with_assoc(&mut positions);
        assert!(positions.is_empty());
    }

    #[test]
    fn map_positions_with_assoc_monotonicity() {
        let cs = ChangeSet::from_changes(20, [(3, 7, Some("X")), (10, 12, None)]);
        for assoc in [Assoc::Before, Assoc::After] {
            let mut positions: Vec<(usize, Assoc)> = (0..=20).map(|p| (p, assoc)).collect();
            cs.map_positions_with_assoc(&mut positions);
            for i in 1..positions.len() {
                assert!(
                    positions[i].0 >= positions[i - 1].0,
                    "monotonicity violated for {assoc:?} at index {i}"
                );
            }
        }
    }

    #[test]
    fn map_positions_with_assoc_brute_force_equivalence() {
        let changesets = [
            ChangeSet::identity(20),
            ChangeSet::from_insert(20, 0, "AB"),
            ChangeSet::from_insert(20, 10, "XYZ"),
            ChangeSet::from_insert(20, 20, "END"),
            ChangeSet::from_delete(20, 5, 10),
            ChangeSet::from_delete(20, 0, 20),
            ChangeSet::from_replace(20, 3, 7, "XY"),
            ChangeSet::from_replace(20, 0, 5, "ABCDEF"),
            ChangeSet::from_changes(20, [(2, 5, Some("X")), (10, 12, None)]),
            ChangeSet::from_changes(20, [(0, 3, None), (5, 5, Some("YY")), (15, 18, Some("Z"))]),
        ];
        let assocs = [
            Assoc::Before,
            Assoc::After,
            Assoc::BeforeSticky,
            Assoc::AfterSticky,
            Assoc::AfterWord,
            Assoc::BeforeWord,
        ];

        for cs in &changesets {
            let max_pos = cs.input_len();
            for &assoc in &assocs {
                let mut shared: Vec<usize> = (0..=max_pos).collect();
                cs.map_positions(&mut shared, assoc);

                let mut per_pos: Vec<(usize, Assoc)> = (0..=max_pos).map(|p| (p, assoc)).collect();
                cs.map_positions_with_assoc(&mut per_pos);

                let per_pos_vals: Vec<usize> = per_pos.iter().map(|&(p, _)| p).collect();
                assert_eq!(
                    per_pos_vals, shared,
                    "mismatch for cs={cs}, assoc={assoc:?}"
                );
            }
        }
    }
}
