//! Inversion: given a changeset and the original document, produce
//! the inverse changeset that undoes the transformation.
//!
//! The inverse of `cs` applied to `cs.apply(original)` yields `original`.
//!
//! # Algorithm
//!
//! Walk the ops, tracking position in the original document:
//! - `Retain(n)` → `Retain(n)` (unchanged text stays unchanged)
//! - `Insert(t)` → `Delete(t.len())` (undo insertion by deleting)
//! - `Delete(n)` → `Insert(original[pos..pos+n])` (undo deletion by re-inserting)

use compact_str::CompactString;

use super::builder::ChangeSetBuilder;
use super::change_set::ChangeSet;
use super::error::ChangeSetError;
use super::text_op::TextOp;

impl ChangeSet {
    /// Compute the inverse of this changeset.
    ///
    /// The inverse, when applied to `self.apply(original)`, yields `original`.
    ///
    /// Requires the **original** document text (before this changeset was
    /// applied) because deleted text must be reconstructed.
    ///
    /// # Errors
    ///
    /// Returns `LengthMismatch` if `original.len() != self.input_len()`.
    ///
    /// # Complexity
    ///
    /// O(input_len) — single pass over the ops and original text.
    pub fn invert(&self, original: &str) -> Result<Self, ChangeSetError> {
        if original.len() != self.input_len() {
            return Err(ChangeSetError::LengthMismatch {
                expected: self.input_len(),
                actual: original.len(),
            });
        }

        let mut b = ChangeSetBuilder::with_capacity(self.op_count());
        let mut pos = 0;

        for op in self.ops() {
            match op {
                TextOp::Retain(n) => {
                    b.retain(*n);
                    pos += n;
                }
                TextOp::Insert(t) => {
                    // Undo an insertion by deleting the same number of bytes
                    b.delete(t.len());
                }
                TextOp::Delete(n) => {
                    // Undo a deletion by re-inserting the original text
                    let end = (pos + n).min(original.len());
                    debug_assert!(
                        end <= original.len(),
                        "invert: Delete({n}) at pos {pos} exceeds original length {}",
                        original.len()
                    );
                    b.insert(CompactString::from(&original[pos..end]));
                    pos = end;
                }
            }
        }

        // The inverse's input is this changeset's output, and vice versa
        Ok(Self::from_parts(
            b.finish(),
            self.output_len(),
            self.input_len(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fundamental property: apply(invert(cs, orig), apply(cs, orig)) == orig
    fn verify_round_trip(cs: &ChangeSet, original: &str) {
        let transformed = cs.apply(original).unwrap();
        let inverse = cs.invert(original).unwrap();
        let restored = inverse.apply(&transformed).unwrap();
        assert_eq!(
            restored, original,
            "round-trip failed:\n  cs: {cs}\n  original: {original:?}\n  \
             transformed: {transformed:?}\n  inverse: {inverse}\n  \
             restored: {restored:?}"
        );
    }

    #[test]
    fn invert_identity() {
        let cs = ChangeSet::identity(5);
        let inv = cs.invert("hello").unwrap();
        assert!(inv.is_identity());
    }

    #[test]
    fn invert_insert() {
        let cs = ChangeSet::from_insert(5, 2, "XX");
        verify_round_trip(&cs, "hello");
    }

    #[test]
    fn invert_delete() {
        let cs = ChangeSet::from_delete(5, 1, 4);
        verify_round_trip(&cs, "hello");
    }

    #[test]
    fn invert_replace() {
        let cs = ChangeSet::from_replace(5, 1, 4, "XY");
        verify_round_trip(&cs, "hello");
    }

    #[test]
    fn invert_insert_beginning() {
        let cs = ChangeSet::from_insert(5, 0, "ABC");
        verify_round_trip(&cs, "hello");
    }

    #[test]
    fn invert_delete_all() {
        let cs = ChangeSet::from_delete(5, 0, 5);
        verify_round_trip(&cs, "hello");
    }

    #[test]
    fn invert_insert_into_empty() {
        let cs = ChangeSet::from_insert(0, 0, "hello");
        verify_round_trip(&cs, "");
    }

    #[test]
    fn invert_multiple_changes() {
        let cs = ChangeSet::from_changes(10, [(1, 3, Some("X")), (5, 7, None)]);
        verify_round_trip(&cs, "abcdefghij");
    }

    #[test]
    fn invert_utf8() {
        // "héllo" is 6 bytes: h(1) + é(2) + l(1) + l(1) + o(1)
        let cs = ChangeSet::from_delete(6, 1, 3); // delete "é"
        verify_round_trip(&cs, "héllo");
    }

    #[test]
    fn invert_length_mismatch() {
        let cs = ChangeSet::identity(5);
        let err = cs.invert("abc").unwrap_err();
        assert_eq!(
            err,
            ChangeSetError::LengthMismatch {
                expected: 5,
                actual: 3,
            }
        );
    }

    #[test]
    fn invert_double_invert_is_original() {
        let cs = ChangeSet::from_replace(10, 3, 7, "hi");
        let original = "abcdefghij";
        let inv = cs.invert(original).unwrap();
        let transformed = cs.apply(original).unwrap();
        let inv_inv = inv.invert(&transformed).unwrap();

        // Double inversion should produce something equivalent to original cs
        let result = inv_inv.apply(original).unwrap();
        assert_eq!(result, cs.apply(original).unwrap());
    }

    #[test]
    fn invert_compose_round_trip() {
        // compose(cs, invert(cs)) should be identity
        let cs = ChangeSet::from_insert(5, 2, "XY");
        let original = "hello";
        let inv = cs.invert(original).unwrap();
        let composed = cs.compose(&inv).unwrap();
        assert_eq!(composed.input_len(), 5);
        assert_eq!(composed.output_len(), 5);
        assert_eq!(composed.apply(original).unwrap(), original);
    }
}
