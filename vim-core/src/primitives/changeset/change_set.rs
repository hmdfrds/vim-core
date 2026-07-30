//! The `ChangeSet` struct — a composable, invertible text transformation.
//!
//! A `ChangeSet` describes how to transform a document of `input_len` bytes
//! into a document of `output_len` bytes via a sequence of `TextOp`s.
//!
//! # Construction
//!
//! Use the factory methods (`identity`, `from_insert`, `from_delete`,
//! `from_replace`, `from_changes`) rather than building ops directly.
//!
//! # Core operations
//!
//! - [`apply`](ChangeSet::apply) — apply to text
//! - [`compose`](ChangeSet::compose) — chain two changesets
//! - [`invert`](ChangeSet::invert) — compute the inverse
//! - [`map_pos`](ChangeSet::map_pos) — map a position through the changeset
//!
//! Each operation lives in its own file for budget compliance.

use compact_str::CompactString;

use super::builder::ChangeSetBuilder;
use super::text_op::TextOp;

/// A composable, invertible description of a text transformation.
///
/// Internally this is a sequence of `Retain` / `Insert` / `Delete`
/// operations in canonical order (Insert before Delete at any position).
///
/// The `input_len` and `output_len` fields enable O(1) length-mismatch
/// checks before any algorithm runs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChangeSet {
    ops: Vec<TextOp>,
    input_len: usize,
    output_len: usize,
}

impl ChangeSet {
    // ── Construction ────────────────────────────────────────────────────

    /// Build a `ChangeSet` directly from pre-validated components.
    ///
    /// This is `pub(super)` — external construction goes through factories.
    ///
    /// In debug builds, validates that `input_len` and `output_len` are
    /// consistent with the ops sequence.
    #[inline]
    pub(super) fn from_parts(ops: Vec<TextOp>, input_len: usize, output_len: usize) -> Self {
        #[cfg(debug_assertions)]
        {
            let mut retain_sum: usize = 0;
            let mut delete_sum: usize = 0;
            let mut insert_sum: usize = 0;
            for op in &ops {
                match op {
                    TextOp::Retain(n) => retain_sum += n,
                    TextOp::Delete(n) => delete_sum += n,
                    TextOp::Insert(t) => insert_sum += t.len(),
                }
            }
            let computed_input = retain_sum + delete_sum;
            let computed_output = retain_sum + insert_sum;
            assert!(
                computed_input == input_len && computed_output == output_len,
                "from_parts: ops inconsistent with lengths — input_len={input_len}, output_len={output_len}, ops={ops:?}"
            );
        }
        Self {
            ops,
            input_len,
            output_len,
        }
    }

    /// Identity (no-op) changeset for a document of `len` bytes.
    ///
    /// `apply(identity(n), text)` returns `text` unchanged.
    #[must_use]
    pub fn identity(len: usize) -> Self {
        let ops = if len > 0 {
            vec![TextOp::Retain(len)]
        } else {
            Vec::new()
        };
        Self {
            ops,
            input_len: len,
            output_len: len,
        }
    }

    /// Changeset for a single insertion.
    ///
    /// `offset` is the byte position in the **original** document.
    #[must_use]
    pub fn from_insert(doc_len: usize, offset: usize, text: &str) -> Self {
        debug_assert!(
            offset <= doc_len,
            "insert offset {offset} exceeds doc_len {doc_len}"
        );
        let offset = offset.min(doc_len);

        if text.is_empty() {
            return Self::identity(doc_len);
        }

        let mut b = ChangeSetBuilder::with_capacity(3);
        b.retain(offset);
        b.insert(CompactString::from(text));
        b.retain(doc_len.saturating_sub(offset));

        Self {
            ops: b.finish(),
            input_len: doc_len,
            output_len: doc_len + text.len(),
        }
    }

    /// Changeset for a single deletion.
    ///
    /// `start..end` is a half-open byte range in the **original** document.
    #[must_use]
    pub fn from_delete(doc_len: usize, start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "delete start {start} > end {end}");
        debug_assert!(end <= doc_len, "delete end {end} exceeds doc_len {doc_len}");
        let start = start.min(doc_len);
        let end = end.min(doc_len);
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        let del_len = end - start;
        if del_len == 0 {
            return Self::identity(doc_len);
        }

        let mut b = ChangeSetBuilder::with_capacity(3);
        b.retain(start);
        b.delete(del_len);
        b.retain(doc_len.saturating_sub(end));

        Self {
            ops: b.finish(),
            input_len: doc_len,
            output_len: doc_len - del_len,
        }
    }

    /// Changeset for a single replacement.
    ///
    /// Replaces `start..end` with `text` in the **original** document.
    #[must_use]
    pub fn from_replace(doc_len: usize, start: usize, end: usize, text: &str) -> Self {
        debug_assert!(start <= end, "replace start {start} > end {end}");
        debug_assert!(
            end <= doc_len,
            "replace end {end} exceeds doc_len {doc_len}"
        );
        let start = start.min(doc_len);
        let end = end.min(doc_len);
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        let del_len = end - start;
        if del_len == 0 && text.is_empty() {
            return Self::identity(doc_len);
        }

        let mut b = ChangeSetBuilder::with_capacity(4);
        b.retain(start);
        b.insert(CompactString::from(text));
        b.delete(del_len);
        b.retain(doc_len.saturating_sub(end));

        Self {
            ops: b.finish(),
            input_len: doc_len,
            output_len: doc_len - del_len + text.len(),
        }
    }

    /// Build a changeset from sorted `(from, to, Option<text>)` triples
    /// in **original** document coordinates.
    ///
    /// Each triple represents:
    /// - `(pos, pos, Some(text))` — pure insertion at `pos`
    /// - `(from, to, None)` — deletion of `from..to`
    /// - `(from, to, Some(text))` — replacement of `from..to` with `text`
    ///
    /// Triples **must** be sorted by `from` and non-overlapping.
    /// In debug builds, this is asserted.
    #[must_use]
    pub fn from_changes<'a, I>(doc_len: usize, changes: I) -> Self
    where
        I: IntoIterator<Item = (usize, usize, Option<&'a str>)>,
    {
        let mut b = ChangeSetBuilder::new();
        let mut pos = 0;
        let mut output_len = doc_len;

        for (from, to, text) in changes {
            debug_assert!(from >= pos, "changes must be sorted and non-overlapping");
            debug_assert!(from <= to, "change range must be from <= to");
            debug_assert!(to <= doc_len, "change range exceeds document length");

            let from = from.min(doc_len);
            let to = to.max(from).min(doc_len);

            // Retain gap between previous position and this change
            b.retain(from.saturating_sub(pos));

            let del_len = to - from;
            if let Some(ins_text) = text {
                if !ins_text.is_empty() {
                    b.insert(CompactString::from(ins_text));
                    output_len += ins_text.len();
                }
            }
            if del_len > 0 {
                b.delete(del_len);
                output_len -= del_len;
            }

            pos = to;
        }

        // Retain remaining text after last change
        b.retain(doc_len.saturating_sub(pos));

        Self {
            ops: b.finish(),
            input_len: doc_len,
            output_len,
        }
    }

    /// Create a mapping-only ChangeSet from edit parameters.
    ///
    /// `doc_len` is the pre-edit document length (or a generous upper bound).
    /// The ChangeSet retains `offset` bytes, then deletes `old_len` and inserts
    /// `new_len`, then retains the remainder.  The inserted content is a dummy
    /// string (this ChangeSet is only used for `map_pos`, not text transformation).
    #[must_use]
    pub fn from_edit_len(doc_len: usize, offset: usize, old_len: usize, new_len: usize) -> Self {
        use compact_str::CompactString;

        let mut b = super::builder::ChangeSetBuilder::with_capacity(4);
        b.retain(offset);
        if old_len > 0 {
            b.delete(old_len);
        }
        if new_len > 0 {
            // Build a dummy string of the correct byte length for Insert.
            // We only use this ChangeSet for map_pos, never for apply().
            let dummy = CompactString::from("x".repeat(new_len));
            b.insert(dummy);
        }
        let tail = doc_len.saturating_sub(offset + old_len);
        b.retain(tail);

        Self {
            ops: b.finish(),
            input_len: doc_len,
            output_len: doc_len - old_len + new_len,
        }
    }

    // ── Queries ─────────────────────────────────────────────────────────

    /// Expected input document length in bytes.
    #[inline]
    #[must_use]
    pub const fn input_len(&self) -> usize {
        self.input_len
    }

    /// Output document length in bytes after applying this changeset.
    #[inline]
    #[must_use]
    pub const fn output_len(&self) -> usize {
        self.output_len
    }

    /// Is this the identity transformation (no changes)?
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.input_len == self.output_len && self.ops.iter().all(TextOp::is_retain)
    }

    /// Does this changeset contain any insertions or deletions?
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.is_identity()
    }

    /// The operation sequence.
    #[inline]
    #[must_use]
    pub fn ops(&self) -> &[TextOp] {
        &self.ops
    }

    /// Number of operations in the sequence.
    #[inline]
    #[must_use]
    pub const fn op_count(&self) -> usize {
        self.ops.len()
    }
}

impl std::fmt::Display for ChangeSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChangeSet({} → {} | ", self.input_len, self.output_len)?;
        for (i, op) in self.ops.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{op}")?;
        }
        write!(f, ")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_empty_doc() {
        let cs = ChangeSet::identity(0);
        assert_eq!(cs.input_len(), 0);
        assert_eq!(cs.output_len(), 0);
        assert!(cs.is_identity());
        assert!(!cs.has_changes());
        assert_eq!(cs.op_count(), 0);
    }

    #[test]
    fn identity_nonempty_doc() {
        let cs = ChangeSet::identity(42);
        assert_eq!(cs.input_len(), 42);
        assert_eq!(cs.output_len(), 42);
        assert!(cs.is_identity());
        assert_eq!(cs.op_count(), 1);
        assert_eq!(cs.ops(), &[TextOp::Retain(42)]);
    }

    #[test]
    fn from_insert_at_beginning() {
        let cs = ChangeSet::from_insert(10, 0, "abc");
        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 13);
        assert!(cs.has_changes());
        assert_eq!(
            cs.ops(),
            &[
                TextOp::Insert(CompactString::from("abc")),
                TextOp::Retain(10),
            ]
        );
    }

    #[test]
    fn from_insert_at_end() {
        let cs = ChangeSet::from_insert(10, 10, "xyz");
        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 13);
        assert_eq!(
            cs.ops(),
            &[
                TextOp::Retain(10),
                TextOp::Insert(CompactString::from("xyz")),
            ]
        );
    }

    #[test]
    fn from_insert_middle() {
        let cs = ChangeSet::from_insert(10, 5, "hi");
        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 12);
        assert_eq!(
            cs.ops(),
            &[
                TextOp::Retain(5),
                TextOp::Insert(CompactString::from("hi")),
                TextOp::Retain(5),
            ]
        );
    }

    #[test]
    fn from_insert_empty_text_is_identity() {
        let cs = ChangeSet::from_insert(10, 5, "");
        assert!(cs.is_identity());
    }

    #[test]
    fn from_delete_beginning() {
        let cs = ChangeSet::from_delete(10, 0, 3);
        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 7);
        assert_eq!(cs.ops(), &[TextOp::Delete(3), TextOp::Retain(7)]);
    }

    #[test]
    fn from_delete_end() {
        let cs = ChangeSet::from_delete(10, 7, 10);
        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 7);
        assert_eq!(cs.ops(), &[TextOp::Retain(7), TextOp::Delete(3)]);
    }

    #[test]
    fn from_delete_empty_is_identity() {
        let cs = ChangeSet::from_delete(10, 5, 5);
        assert!(cs.is_identity());
    }

    #[test]
    fn from_replace() {
        let cs = ChangeSet::from_replace(10, 3, 6, "hello");
        assert_eq!(cs.input_len(), 10);
        // 10 - 3 (deleted) + 5 (inserted) = 12
        assert_eq!(cs.output_len(), 12);
        assert_eq!(
            cs.ops(),
            &[
                TextOp::Retain(3),
                TextOp::Insert(CompactString::from("hello")),
                TextOp::Delete(3),
                TextOp::Retain(4),
            ]
        );
    }

    #[test]
    fn from_changes_multiple() {
        // Original: "hello world" (11 bytes)
        // Change 1: delete "ello" (1..5)
        // Change 2: insert "!" at position 6 (after space)
        let cs = ChangeSet::from_changes(
            11,
            [
                (1, 5, None),      // delete "ello"
                (6, 6, Some("!")), // insert "!" after space
            ],
        );
        // 11 - 4 (deleted) + 1 (inserted) = 8
        assert_eq!(cs.input_len(), 11);
        assert_eq!(cs.output_len(), 8);
    }

    #[test]
    fn from_changes_empty_is_identity() {
        let cs = ChangeSet::from_changes(10, std::iter::empty());
        assert!(cs.is_identity());
    }

    #[test]
    fn from_changes_replacement() {
        // "abcdef" (6 bytes) -> replace "cd" with "XY"
        let cs = ChangeSet::from_changes(6, [(2, 4, Some("XY"))]);
        assert_eq!(cs.input_len(), 6);
        assert_eq!(cs.output_len(), 6); // same length replacement
    }

    #[test]
    fn display_format() {
        let cs = ChangeSet::from_replace(10, 3, 6, "hi");
        let s = cs.to_string();
        assert!(s.contains("ChangeSet(10 → 9"));
    }

    #[test]
    fn insert_into_empty_doc() {
        let cs = ChangeSet::from_insert(0, 0, "hello");
        assert_eq!(cs.input_len(), 0);
        assert_eq!(cs.output_len(), 5);
        assert_eq!(cs.ops(), &[TextOp::Insert(CompactString::from("hello"))]);
    }

    #[test]
    fn delete_entire_doc() {
        let cs = ChangeSet::from_delete(10, 0, 10);
        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 0);
        assert_eq!(cs.ops(), &[TextOp::Delete(10)]);
    }
}
