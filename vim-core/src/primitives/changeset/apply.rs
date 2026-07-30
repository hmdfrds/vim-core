//! Apply a `ChangeSet` to produce a new document.
//!
//! `apply` walks the ops left-to-right, copying retained bytes,
//! inserting new text, and skipping deleted bytes.

use super::change_set::ChangeSet;
use super::error::ChangeSetError;
use super::text_op::TextOp;

impl ChangeSet {
    /// Apply this changeset to `text`, producing a new `String`.
    ///
    /// # Errors
    ///
    /// Returns `Err(LengthMismatch)` if `text.len() != self.input_len()`.
    ///
    /// # Complexity
    ///
    /// O(input_len + output_len) — single pass, no backtracking.
    pub fn apply(&self, text: &str) -> Result<String, ChangeSetError> {
        if text.len() != self.input_len() {
            return Err(ChangeSetError::LengthMismatch {
                expected: self.input_len(),
                actual: text.len(),
            });
        }

        let mut result = String::with_capacity(self.output_len());
        let mut pos = 0;

        for op in self.ops() {
            match op {
                TextOp::Retain(n) => {
                    let end = pos + n;
                    debug_assert!(
                        end <= text.len(),
                        "Retain({n}) at pos {pos} exceeds text length {}",
                        text.len()
                    );
                    let end = end.min(text.len());
                    // Safety: pos..end are within bounds; char boundary
                    // correctness is guaranteed by changeset construction.
                    result.push_str(&text[pos..end]);
                    pos = end;
                }
                TextOp::Insert(t) => {
                    result.push_str(t);
                }
                TextOp::Delete(n) => {
                    let end = pos + n;
                    debug_assert!(
                        end <= text.len(),
                        "Delete({n}) at pos {pos} exceeds text length {}",
                        text.len()
                    );
                    pos = end.min(text.len());
                }
            }
        }

        debug_assert_eq!(pos, text.len(), "not all input consumed");
        debug_assert_eq!(
            result.len(),
            self.output_len(),
            "output length mismatch: produced {} expected {}",
            result.len(),
            self.output_len()
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_identity() {
        let cs = ChangeSet::identity(5);
        assert_eq!(cs.apply("hello").unwrap(), "hello");
    }

    #[test]
    fn apply_identity_empty() {
        let cs = ChangeSet::identity(0);
        assert_eq!(cs.apply("").unwrap(), "");
    }

    #[test]
    fn apply_insert_beginning() {
        let cs = ChangeSet::from_insert(5, 0, "XXX");
        assert_eq!(cs.apply("hello").unwrap(), "XXXhello");
    }

    #[test]
    fn apply_insert_end() {
        let cs = ChangeSet::from_insert(5, 5, "!!!");
        assert_eq!(cs.apply("hello").unwrap(), "hello!!!");
    }

    #[test]
    fn apply_insert_middle() {
        let cs = ChangeSet::from_insert(5, 2, "--");
        assert_eq!(cs.apply("hello").unwrap(), "he--llo");
    }

    #[test]
    fn apply_delete_beginning() {
        let cs = ChangeSet::from_delete(5, 0, 2);
        assert_eq!(cs.apply("hello").unwrap(), "llo");
    }

    #[test]
    fn apply_delete_end() {
        let cs = ChangeSet::from_delete(5, 3, 5);
        assert_eq!(cs.apply("hello").unwrap(), "hel");
    }

    #[test]
    fn apply_delete_middle() {
        let cs = ChangeSet::from_delete(5, 1, 4);
        assert_eq!(cs.apply("hello").unwrap(), "ho");
    }

    #[test]
    fn apply_replace() {
        let cs = ChangeSet::from_replace(5, 1, 4, "XY");
        assert_eq!(cs.apply("hello").unwrap(), "hXYo");
    }

    #[test]
    fn apply_delete_all() {
        let cs = ChangeSet::from_delete(5, 0, 5);
        assert_eq!(cs.apply("hello").unwrap(), "");
    }

    #[test]
    fn apply_insert_into_empty() {
        let cs = ChangeSet::from_insert(0, 0, "new");
        assert_eq!(cs.apply("").unwrap(), "new");
    }

    #[test]
    fn apply_length_mismatch() {
        let cs = ChangeSet::identity(5);
        let err = cs.apply("abc").unwrap_err();
        assert_eq!(
            err,
            ChangeSetError::LengthMismatch {
                expected: 5,
                actual: 3,
            }
        );
    }

    #[test]
    fn apply_utf8_multibyte() {
        // "héllo" = h(1) + é(2) + l(1) + l(1) + o(1) = 6 bytes
        let text = "héllo";
        assert_eq!(text.len(), 6);
        // Delete "é" (bytes 1..3)
        let cs = ChangeSet::from_delete(6, 1, 3);
        assert_eq!(cs.apply(text).unwrap(), "hllo");
    }

    #[test]
    fn apply_from_changes_multiple() {
        // "abcdefghij" (10 bytes)
        // Delete "bcd" (1..4), insert "X" at 6
        let cs = ChangeSet::from_changes(
            10,
            [
                (1, 4, None),      // delete "bcd"
                (6, 6, Some("X")), // insert "X" before 'g'
            ],
        );
        assert_eq!(cs.apply("abcdefghij").unwrap(), "aefXghij");
    }

    #[test]
    fn apply_replace_with_longer() {
        let cs = ChangeSet::from_replace(5, 1, 2, "XXXX");
        // "hello" -> "h" + "XXXX" + "llo" = "hXXXXllo"
        assert_eq!(cs.apply("hello").unwrap(), "hXXXXllo");
    }

    #[test]
    fn apply_replace_with_shorter() {
        let cs = ChangeSet::from_replace(5, 0, 5, "hi");
        assert_eq!(cs.apply("hello").unwrap(), "hi");
    }
}
