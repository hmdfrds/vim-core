//! Composition: `compose(A→B, B→C) → A→C`.
//!
//! The two-pointer merge algorithm walks both op sequences simultaneously,
//! consuming matching byte counts and emitting composed ops into a builder.
//!
//! # Algorithm overview
//!
//! Two "heads" (current ops from A and B) are compared:
//!
//! | A op | B op | Result |
//! |------|------|--------|
//! | `Delete(i)` | any | Pass `Delete` through (deleted in A, doesn't exist in B) |
//! | any | `Insert(s)` | Pass `Insert` through (new in B, not in A's output) |
//! | `Retain(i)` | `Retain(j)` | `Retain(min)`, push back remainder |
//! | `Retain(i)` | `Delete(j)` | `Delete(min)`, push back remainder |
//! | `Insert(s)` | `Retain(j)` | `Insert(s[..min])`, push back remainder |
//! | `Insert(s)` | `Delete(j)` | Cancellation — skip min bytes from both |

use compact_str::CompactString;

use super::builder::ChangeSetBuilder;
use super::change_set::ChangeSet;
use super::error::ChangeSetError;
use super::text_op::TextOp;

impl ChangeSet {
    /// Compose this changeset (A→B) with `other` (B→C) to produce A→C.
    ///
    /// Requires `self.output_len() == other.input_len()`.
    ///
    /// # Errors
    ///
    /// Returns `ComposeMismatch` if `self.output_len() != other.input_len()`.
    ///
    /// # Complexity
    ///
    /// O(|ops_a| + |ops_b|) — each op is consumed at most once.
    pub fn compose(&self, other: &Self) -> Result<Self, ChangeSetError> {
        if self.output_len() != other.input_len() {
            return Err(ChangeSetError::ComposeMismatch {
                a_output: self.output_len(),
                b_input: other.input_len(),
            });
        }

        // Fast paths
        if self.ops().is_empty() {
            return Ok(other.clone());
        }
        if other.ops().is_empty() {
            return Ok(self.clone());
        }

        let mut iter_a = self.ops().iter().cloned();
        let mut iter_b = other.ops().iter().cloned();

        let mut head_a: Option<TextOp> = iter_a.next();
        let mut head_b: Option<TextOp> = iter_b.next();

        let mut b = ChangeSetBuilder::with_capacity(self.op_count().max(other.op_count()));

        loop {
            match (head_a.take(), head_b.take()) {
                // Done
                (None, None) => break,

                // A:Delete passes through — this text was deleted before B sees it
                (Some(TextOp::Delete(i)), b_op) => {
                    b.delete(i);
                    head_a = iter_a.next();
                    head_b = b_op;
                }

                // B:Insert passes through — new text not in A's output
                (a_op, Some(TextOp::Insert(s))) => {
                    b.insert(s);
                    head_a = a_op;
                    head_b = iter_b.next();
                }

                // Both exhausted one side but not the other — length mismatch
                // should have been caught by the guard above, but surface the
                // error instead of silently dropping ops in release builds.
                (None, Some(_remaining)) => {
                    return Err(ChangeSetError::ComposeMismatch {
                        a_output: self.output_len(),
                        b_input: other.input_len(),
                    });
                }
                (Some(_remaining), None) => {
                    return Err(ChangeSetError::ComposeMismatch {
                        a_output: self.output_len(),
                        b_input: other.input_len(),
                    });
                }

                // Retain + Retain → Retain(min)
                (Some(TextOp::Retain(i)), Some(TextOp::Retain(j))) => {
                    let min = i.min(j);
                    b.retain(min);
                    if i > min {
                        head_a = Some(TextOp::Retain(i - min));
                    } else {
                        head_a = iter_a.next();
                    }
                    if j > min {
                        head_b = Some(TextOp::Retain(j - min));
                    } else {
                        head_b = iter_b.next();
                    }
                }

                // Retain + Delete → Delete(min)
                (Some(TextOp::Retain(i)), Some(TextOp::Delete(j))) => {
                    let min = i.min(j);
                    b.delete(min);
                    if i > min {
                        head_a = Some(TextOp::Retain(i - min));
                    } else {
                        head_a = iter_a.next();
                    }
                    if j > min {
                        head_b = Some(TextOp::Delete(j - min));
                    } else {
                        head_b = iter_b.next();
                    }
                }

                // Insert + Retain → Insert(text[..min])
                (Some(TextOp::Insert(s)), Some(TextOp::Retain(j))) => {
                    let s_len = s.len();
                    let min = s_len.min(j);

                    debug_assert!(
                        s.is_char_boundary(min),
                        "compose: splitting Insert at non-char-boundary {min} in {s:?}"
                    );

                    if s_len <= j {
                        // Entire insert is retained
                        b.insert(s);
                        head_a = iter_a.next();
                        if j > s_len {
                            head_b = Some(TextOp::Retain(j - s_len));
                        } else {
                            head_b = iter_b.next();
                        }
                    } else {
                        // Split the insert
                        let boundary = find_char_boundary(&s, j);
                        b.insert(CompactString::from(&s[..boundary]));
                        head_a = Some(TextOp::Insert(CompactString::from(&s[boundary..])));
                        head_b = iter_b.next();
                    }
                }

                // Insert + Delete → cancellation (insert then immediately delete)
                (Some(TextOp::Insert(s)), Some(TextOp::Delete(j))) => {
                    let s_len = s.len();

                    if s_len <= j {
                        // Entire insert is deleted — perfect cancellation
                        head_a = iter_a.next();
                        if j > s_len {
                            head_b = Some(TextOp::Delete(j - s_len));
                        } else {
                            head_b = iter_b.next();
                        }
                    } else {
                        // Delete consumes part of the insert
                        let boundary = find_char_boundary(&s, j);
                        head_a = Some(TextOp::Insert(CompactString::from(&s[boundary..])));
                        head_b = iter_b.next();
                    }
                }
            }
        }

        Ok(Self::from_parts(
            b.finish(),
            self.input_len(),
            other.output_len(),
        ))
    }
}

/// Find the nearest char boundary at or after `pos` in `s`.
///
/// If `pos` is already a char boundary, returns `pos`.
/// Otherwise advances to the next boundary.
const fn find_char_boundary(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    let mut p = pos;
    while p < s.len() && !s.is_char_boundary(p) {
        p += 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: verify compose by applying both individually vs composed
    fn verify_compose(a: &ChangeSet, b: &ChangeSet, original: &str) {
        let mid = a.apply(original).unwrap();
        let final_individual = b.apply(&mid).unwrap();
        let composed = a.compose(b).unwrap();
        let final_composed = composed.apply(original).unwrap();
        assert_eq!(
            final_individual, final_composed,
            "compose mismatch:\n  A: {a}\n  B: {b}\n  composed: {composed}\n  \
             original: {original:?}\n  via individual: {final_individual:?}\n  \
             via composed: {final_composed:?}"
        );
    }

    #[test]
    fn compose_identity_left() {
        let cs = ChangeSet::from_insert(5, 2, "X");
        let id = ChangeSet::identity(5);
        let composed = id.compose(&cs).unwrap();
        assert_eq!(composed.apply("hello").unwrap(), cs.apply("hello").unwrap());
    }

    #[test]
    fn compose_identity_right() {
        let cs = ChangeSet::from_insert(5, 2, "X");
        let id = ChangeSet::identity(cs.output_len());
        let composed = cs.compose(&id).unwrap();
        assert_eq!(composed.apply("hello").unwrap(), cs.apply("hello").unwrap());
    }

    #[test]
    fn compose_two_inserts() {
        let a = ChangeSet::from_insert(5, 0, "A"); // "hello" -> "Ahello" (6)
        let b = ChangeSet::from_insert(6, 6, "B"); // "Ahello" -> "AhelloB" (7)
        verify_compose(&a, &b, "hello");
    }

    #[test]
    fn compose_insert_then_delete() {
        let a = ChangeSet::from_insert(5, 2, "XX"); // "hello" -> "heXXllo" (7)
        let b = ChangeSet::from_delete(7, 2, 4);
        // Composed: inserting "XX" then immediately deleting it yields the original text.
        verify_compose(&a, &b, "hello");
    }

    #[test]
    fn compose_delete_then_insert() {
        let a = ChangeSet::from_delete(5, 1, 3); // "hello" -> "hlo" (3)
        let b = ChangeSet::from_insert(3, 1, "EL"); // "hlo" -> "hELlo" (5)
        verify_compose(&a, &b, "hello");
    }

    #[test]
    fn compose_insert_then_delete_cancellation() {
        // Insert "XY" then immediately delete it — should be identity
        let a = ChangeSet::from_insert(5, 2, "XY"); // "hello" -> "heXYllo" (7)
        let b = ChangeSet::from_delete(7, 2, 4); // "heXYllo" -> "hello" (5)
        let composed = a.compose(&b).unwrap();
        assert!(composed.is_identity(), "should be identity: {composed}");
    }

    #[test]
    fn compose_mismatch_error() {
        let a = ChangeSet::identity(5);
        let b = ChangeSet::identity(10);
        let err = a.compose(&b).unwrap_err();
        assert_eq!(
            err,
            ChangeSetError::ComposeMismatch {
                a_output: 5,
                b_input: 10,
            }
        );
    }

    #[test]
    fn compose_two_deletes() {
        let a = ChangeSet::from_delete(10, 0, 3); // 10 -> 7
        let b = ChangeSet::from_delete(7, 4, 7); // 7 -> 4
        verify_compose(&a, &b, "abcdefghij");
    }

    #[test]
    fn compose_two_replaces() {
        let a = ChangeSet::from_replace(10, 2, 5, "XY"); // 10 -> 9
        let b = ChangeSet::from_replace(9, 0, 3, "AB"); // 9 -> 8
        verify_compose(&a, &b, "abcdefghij");
    }

    #[test]
    fn compose_associativity() {
        // (A∘B)∘C == A∘(B∘C)
        let a = ChangeSet::from_insert(5, 2, "X");
        let b = ChangeSet::from_delete(6, 0, 1);
        let c = ChangeSet::from_insert(5, 5, "!");

        let ab = a.compose(&b).unwrap();
        let ab_c = ab.compose(&c).unwrap();

        let bc = b.compose(&c).unwrap();
        let a_bc = a.compose(&bc).unwrap();

        let result1 = ab_c.apply("hello").unwrap();
        let result2 = a_bc.apply("hello").unwrap();
        assert_eq!(result1, result2);
    }

    #[test]
    fn compose_empty_changesets() {
        let a = ChangeSet::identity(0);
        let b = ChangeSet::identity(0);
        let composed = a.compose(&b).unwrap();
        assert!(composed.is_identity());
        assert_eq!(composed.input_len(), 0);
    }

    #[test]
    fn compose_from_changes_complex() {
        // Multiple changes in A, single change in B
        let a = ChangeSet::from_changes(
            10,
            [
                (1, 3, Some("X")), // replace "bc" with "X"
                (5, 7, None),      // delete "fg"
            ],
        );
        // "abcdefghij": replace [1..3] "bc" with "X" and delete [5..7] "fg".
        // from_changes uses original coordinates, so A is:
        // retain 1, insert "X", delete 2, retain 2, delete 2, retain 3
        // = "a" + "X" + skip "bc" + "de" + skip "fg" + "hij"
        // = "aXdehij" (7 bytes)
        let mid = a.apply("abcdefghij").unwrap();
        assert_eq!(mid, "aXdehij");

        let b = ChangeSet::from_insert(7, 3, "YY");
        verify_compose(&a, &b, "abcdefghij");
    }

    #[test]
    fn compose_utf8_insert_split() {
        // Insert a multibyte string, then retain part of it
        // "ab" (2 bytes) -> insert "héllo" at 1 -> "ahéllob" (8 bytes)
        let a = ChangeSet::from_insert(2, 1, "héllo");
        // Retain first 4 bytes of "ahéllob" (= "ahél"), delete rest
        let b = ChangeSet::from_delete(8, 4, 8);
        verify_compose(&a, &b, "ab");
    }
}
