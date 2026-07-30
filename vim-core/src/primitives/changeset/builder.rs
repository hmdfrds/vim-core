//! Internal builder for constructing well-formed `ChangeSet` op sequences.
//!
//! The builder enforces two invariants:
//!
//! 1. **Canonical ordering**: At any position, `Insert` always comes before
//!    `Delete`. If a `Delete` is the last op and an `Insert` is pushed, the
//!    builder swaps them. This matches the operational-transform literature.
//!
//! 2. **Coalescing**: Adjacent ops of the same kind are merged. Two consecutive
//!    `Retain(a)` + `Retain(b)` become `Retain(a+b)`, etc.
//!
//! The builder is `pub(super)` — only changeset internals use it.

use compact_str::CompactString;

use super::text_op::TextOp;

/// Builder that accumulates `TextOp`s into a well-formed sequence.
///
/// All changeset construction goes through this builder to guarantee
/// canonical ordering and coalesced ops.
pub(super) struct ChangeSetBuilder {
    ops: Vec<TextOp>,
}

impl ChangeSetBuilder {
    /// Create an empty builder.
    #[inline]
    pub(super) const fn new() -> Self {
        Self { ops: Vec::new() }
    }

    /// Create a builder with pre-allocated capacity.
    #[inline]
    pub(super) fn with_capacity(cap: usize) -> Self {
        Self {
            ops: Vec::with_capacity(cap),
        }
    }

    /// Push a `Retain(n)`. No-op if `n == 0`.
    pub(super) fn retain(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        // Coalesce with previous Retain
        if let Some(TextOp::Retain(prev)) = self.ops.last_mut() {
            *prev += n;
            return;
        }
        self.ops.push(TextOp::Retain(n));
    }

    /// Push an `Insert(text)`. No-op if text is empty.
    ///
    /// Enforces canonical ordering: if the last op is a `Delete`, swap so
    /// `Insert` comes first. This is critical for compose correctness.
    pub(super) fn insert(&mut self, text: CompactString) {
        if text.is_empty() {
            return;
        }
        // Canonical ordering: Insert before Delete.
        // If last op is Delete, we need to insert BEFORE it.
        // But first check if the op before the Delete is also an Insert — coalesce.
        if let Some(TextOp::Delete(_)) = self.ops.last() {
            let delete = self.ops.pop();
            // Now try to coalesce with what's now the last op
            if let Some(TextOp::Insert(prev)) = self.ops.last_mut() {
                prev.push_str(&text);
            } else {
                self.ops.push(TextOp::Insert(text));
            }
            // Put the Delete back after the Insert
            if let Some(del) = delete {
                self.ops.push(del);
            }
            return;
        }
        // Coalesce with previous Insert
        if let Some(TextOp::Insert(prev)) = self.ops.last_mut() {
            prev.push_str(&text);
            return;
        }
        self.ops.push(TextOp::Insert(text));
    }

    /// Push a `Delete(n)`. No-op if `n == 0`.
    pub(super) fn delete(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        // Coalesce with previous Delete
        if let Some(TextOp::Delete(prev)) = self.ops.last_mut() {
            *prev += n;
            return;
        }
        self.ops.push(TextOp::Delete(n));
    }

    /// Consume the builder, returning the op sequence.
    #[inline]
    pub(super) fn finish(self) -> Vec<TextOp> {
        self.ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder() {
        let b = ChangeSetBuilder::new();
        assert!(b.finish().is_empty());
    }

    #[test]
    fn retain_coalesces() {
        let mut b = ChangeSetBuilder::new();
        b.retain(3);
        b.retain(5);
        let ops = b.finish();
        assert_eq!(ops, vec![TextOp::Retain(8)]);
    }

    #[test]
    fn delete_coalesces() {
        let mut b = ChangeSetBuilder::new();
        b.delete(2);
        b.delete(4);
        let ops = b.finish();
        assert_eq!(ops, vec![TextOp::Delete(6)]);
    }

    #[test]
    fn insert_coalesces() {
        let mut b = ChangeSetBuilder::new();
        b.insert(CompactString::from("he"));
        b.insert(CompactString::from("llo"));
        let ops = b.finish();
        assert_eq!(ops, vec![TextOp::Insert(CompactString::from("hello"))]);
    }

    #[test]
    fn canonical_ordering_insert_before_delete() {
        let mut b = ChangeSetBuilder::new();
        b.delete(3);
        b.insert(CompactString::from("abc"));
        let ops = b.finish();
        // Insert must come before Delete
        assert_eq!(
            ops,
            vec![
                TextOp::Insert(CompactString::from("abc")),
                TextOp::Delete(3),
            ]
        );
    }

    #[test]
    fn canonical_ordering_coalesces_insert_through_delete() {
        let mut b = ChangeSetBuilder::new();
        b.insert(CompactString::from("ab"));
        b.delete(3);
        b.insert(CompactString::from("cd"));
        let ops = b.finish();
        // Second insert should coalesce with first (before the delete)
        assert_eq!(
            ops,
            vec![
                TextOp::Insert(CompactString::from("abcd")),
                TextOp::Delete(3),
            ]
        );
    }

    #[test]
    fn zero_length_ops_ignored() {
        let mut b = ChangeSetBuilder::new();
        b.retain(0);
        b.delete(0);
        b.insert(CompactString::from(""));
        b.retain(5);
        let ops = b.finish();
        assert_eq!(ops, vec![TextOp::Retain(5)]);
    }

    #[test]
    fn mixed_sequence() {
        let mut b = ChangeSetBuilder::new();
        b.retain(5);
        b.delete(3);
        b.insert(CompactString::from("hello"));
        b.retain(10);
        let ops = b.finish();
        assert_eq!(
            ops,
            vec![
                TextOp::Retain(5),
                TextOp::Insert(CompactString::from("hello")),
                TextOp::Delete(3),
                TextOp::Retain(10),
            ]
        );
    }

    #[test]
    fn retain_does_not_coalesce_across_other_ops() {
        let mut b = ChangeSetBuilder::new();
        b.retain(3);
        b.delete(1);
        b.retain(4);
        let ops = b.finish();
        assert_eq!(
            ops,
            vec![TextOp::Retain(3), TextOp::Delete(1), TextOp::Retain(4),]
        );
    }

    #[test]
    fn with_capacity_preallocates() {
        let b = ChangeSetBuilder::with_capacity(16);
        assert!(b.finish().is_empty());
    }
}
