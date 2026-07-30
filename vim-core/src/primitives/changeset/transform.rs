//! OT transform: given two concurrent edits A and B from the same base
//! document, produce A' and B' such that `A.compose(B') == B.compose(A')`
//! (the diamond property).
//!
//! # Algorithm
//!
//! Standard two-cursor walk over both op sequences. At each step, compare
//! the current ("head") op from each side:
//!
//! | A head | B head | A' gets | B' gets | Consumed |
//! |--------|--------|---------|---------|----------|
//! | `Retain(n)` | `Retain(m)` | `Retain(min)` | `Retain(min)` | min from both |
//! | `Insert(s)` | any | `Insert(s)` | `Retain(s.len())` | A only |
//! | any | `Insert(s)` | `Retain(s.len())` | `Insert(s)` | B only |
//! | `Delete(n)` | `Delete(m)` | nothing | nothing | min from both (cancel) |
//! | `Delete(n)` | `Retain(m)` | `Delete(min)` | nothing | min from both |
//! | `Retain(n)` | `Delete(m)` | nothing | `Delete(min)` | min from both |
//!
//! **Tie-breaking (left-bias):** when both heads are `Insert`, A goes first.

use super::builder::ChangeSetBuilder;
use super::change_set::ChangeSet;
use super::error::ChangeSetError;
use super::text_op::TextOp;

impl ChangeSet {
    /// Transform two concurrent changesets against each other.
    ///
    /// Given changesets `a` and `b` that both start from the same base
    /// document (`a.input_len() == b.input_len()`), produces `(a_prime, b_prime)`
    /// satisfying the diamond property:
    ///
    /// ```text
    ///       base
    ///      /    \
    ///     A      B
    ///    /        \
    ///  docA      docB
    ///    \        /
    ///     B'    A'
    ///      \  /
    ///      final
    /// ```
    ///
    /// That is, `a.compose(&b_prime) == b.compose(&a_prime)`.
    ///
    /// Uses **left-bias**: when both changesets insert at the same position,
    /// A's insertion comes first in the final document.
    ///
    /// # Errors
    ///
    /// Returns [`ChangeSetError::TransformMismatch`] if
    /// `a.input_len() != b.input_len()`.
    ///
    /// # Complexity
    ///
    /// O(|ops_a| + |ops_b|) -- each op is consumed at most once.
    pub fn transform(a: &Self, b: &Self) -> Result<(Self, Self), ChangeSetError> {
        if a.input_len() != b.input_len() {
            return Err(ChangeSetError::TransformMismatch {
                a_input: a.input_len(),
                b_input: b.input_len(),
            });
        }

        // Fast path: both identity
        if a.ops().is_empty() && b.ops().is_empty() {
            return Ok((
                Self::identity(b.output_len()),
                Self::identity(a.output_len()),
            ));
        }

        let mut iter_a = a.ops().iter().cloned();
        let mut iter_b = b.ops().iter().cloned();

        let mut head_a: Option<TextOp> = iter_a.next();
        let mut head_b: Option<TextOp> = iter_b.next();

        // A' transforms B's output into the final document.
        // B' transforms A's output into the final document.
        let mut out_a = ChangeSetBuilder::with_capacity(a.op_count() + b.op_count());
        let mut out_b = ChangeSetBuilder::with_capacity(a.op_count() + b.op_count());

        loop {
            match (head_a.take(), head_b.take()) {
                // Both sides exhausted -- done.
                (None, None) => break,

                // A has an Insert, B has anything (or is exhausted).
                // Left-bias: A's insert goes first when both are Insert.
                (Some(TextOp::Insert(s)), b_op) => {
                    let len = s.len();
                    out_a.insert(s);
                    out_b.retain(len);
                    head_a = iter_a.next();
                    head_b = b_op; // put B's op back, it wasn't consumed
                }

                // B has an Insert, A is not Insert (Insert+Insert is caught above).
                (a_op, Some(TextOp::Insert(s))) => {
                    let len = s.len();
                    out_a.retain(len);
                    out_b.insert(s);
                    head_a = a_op; // put A's op back
                    head_b = iter_b.next();
                }

                // Retain + Retain -- consume min from both.
                (Some(TextOp::Retain(i)), Some(TextOp::Retain(j))) => {
                    let min = i.min(j);
                    out_a.retain(min);
                    out_b.retain(min);
                    head_a = if i > min {
                        Some(TextOp::Retain(i - min))
                    } else {
                        iter_a.next()
                    };
                    head_b = if j > min {
                        Some(TextOp::Retain(j - min))
                    } else {
                        iter_b.next()
                    };
                }

                // Delete + Delete -- concurrent deletes cancel out.
                (Some(TextOp::Delete(i)), Some(TextOp::Delete(j))) => {
                    let min = i.min(j);
                    // Both deleted the same region -- nothing goes to either prime.
                    head_a = if i > min {
                        Some(TextOp::Delete(i - min))
                    } else {
                        iter_a.next()
                    };
                    head_b = if j > min {
                        Some(TextOp::Delete(j - min))
                    } else {
                        iter_b.next()
                    };
                }

                // Delete(A) + Retain(B) -- A deletes what B retains.
                // A' must delete in B's output; B' emits nothing (text is gone).
                (Some(TextOp::Delete(i)), Some(TextOp::Retain(j))) => {
                    let min = i.min(j);
                    out_a.delete(min);
                    // B' emits nothing -- the text A deleted doesn't exist
                    head_a = if i > min {
                        Some(TextOp::Delete(i - min))
                    } else {
                        iter_a.next()
                    };
                    head_b = if j > min {
                        Some(TextOp::Retain(j - min))
                    } else {
                        iter_b.next()
                    };
                }

                // Retain(A) + Delete(B) -- B deletes what A retains.
                // B' must delete in A's output; A' emits nothing (text is gone).
                (Some(TextOp::Retain(i)), Some(TextOp::Delete(j))) => {
                    let min = i.min(j);
                    out_b.delete(min);
                    // A' emits nothing -- the text B deleted doesn't exist
                    head_a = if i > min {
                        Some(TextOp::Retain(i - min))
                    } else {
                        iter_a.next()
                    };
                    head_b = if j > min {
                        Some(TextOp::Delete(j - min))
                    } else {
                        iter_b.next()
                    };
                }

                // One side exhausted while the other still has Retain/Delete.
                // This should not happen if input_len matches, but we handle
                // it gracefully by surfacing the mismatch.
                (None, Some(_)) | (Some(_), None) => {
                    return Err(ChangeSetError::TransformMismatch {
                        a_input: a.input_len(),
                        b_input: b.input_len(),
                    });
                }
            }
        }

        // Compute output lengths:
        //
        // A' takes B's output as input and produces the merged document.
        //   input_len  = b.output_len()
        //   output_len = sum of A' retains + A' inserts
        //
        // B' takes A's output as input and produces the merged document.
        //   input_len  = a.output_len()
        //   output_len = sum of B' retains + B' inserts
        //
        // The builder's ops give us what we need to compute these.
        let a_prime_ops = out_a.finish();
        let b_prime_ops = out_b.finish();

        let (a_prime_input, a_prime_output) = compute_lengths(&a_prime_ops);
        let (b_prime_input, b_prime_output) = compute_lengths(&b_prime_ops);

        Ok((
            Self::from_parts(a_prime_ops, a_prime_input, a_prime_output),
            Self::from_parts(b_prime_ops, b_prime_input, b_prime_output),
        ))
    }
}

/// Compute `(input_len, output_len)` from a raw op sequence.
///
/// - `input_len  = sum(Retain) + sum(Delete)`
/// - `output_len = sum(Retain) + sum(Insert.len())`
fn compute_lengths(ops: &[TextOp]) -> (usize, usize) {
    let mut retain_sum: usize = 0;
    let mut delete_sum: usize = 0;
    let mut insert_sum: usize = 0;

    for op in ops {
        match op {
            TextOp::Retain(n) => retain_sum += n,
            TextOp::Delete(n) => delete_sum += n,
            TextOp::Insert(t) => insert_sum += t.len(),
        }
    }

    (retain_sum + delete_sum, retain_sum + insert_sum)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ── Diamond property helper ────────────────────────────────────────

    /// Assert the diamond property: applying A then B' must yield the
    /// same document as applying B then A'.
    fn assert_diamond(a: &ChangeSet, b: &ChangeSet, base: &str) {
        let (a_prime, b_prime) = ChangeSet::transform(a, b).unwrap();

        let via_a = a.apply(base).unwrap();
        let via_a_then_b_prime = b_prime.apply(&via_a).unwrap();

        let via_b = b.apply(base).unwrap();
        let via_b_then_a_prime = a_prime.apply(&via_b).unwrap();

        assert_eq!(
            via_a_then_b_prime, via_b_then_a_prime,
            "Diamond property violated!\n\
             base: {base:?}\n\
             A: {a}\n\
             B: {b}\n\
             A': {a_prime}\n\
             B': {b_prime}\n\
             A(base): {via_a:?}\n\
             B(base): {via_b:?}\n\
             A(base) then B': {via_a_then_b_prime:?}\n\
             B(base) then A': {via_b_then_a_prime:?}"
        );

        // Also verify compose equivalence: A.compose(B') == B.compose(A').
        let composed_ab = a.compose(&b_prime).unwrap();
        let composed_ba = b.compose(&a_prime).unwrap();
        let result_ab = composed_ab.apply(base).unwrap();
        let result_ba = composed_ba.apply(base).unwrap();
        assert_eq!(
            result_ab, result_ba,
            "Compose equivalence violated!\n\
             A.compose(B').apply(base) = {result_ab:?}\n\
             B.compose(A').apply(base) = {result_ba:?}"
        );

        // All three paths must agree.
        assert_eq!(via_a_then_b_prime, result_ab);
    }

    // ── Test 1: Identity ───────────────────────────────────────────────

    #[test]
    fn transform_a_with_identity() {
        let base = "hello";
        let a = ChangeSet::from_insert(5, 2, "X");
        let id = ChangeSet::identity(5);
        assert_diamond(&a, &id, base);
    }

    #[test]
    fn transform_identity_with_b() {
        let base = "hello";
        let id = ChangeSet::identity(5);
        let b = ChangeSet::from_delete(5, 1, 3);
        assert_diamond(&id, &b, base);
    }

    #[test]
    fn transform_both_identity() {
        let base = "hello";
        let id = ChangeSet::identity(5);
        let (a_prime, b_prime) = ChangeSet::transform(&id, &id).unwrap();
        assert!(a_prime.is_identity());
        assert!(b_prime.is_identity());
        assert_diamond(&id, &id, base);
    }

    // ── Test 2: Two inserts at different positions ─────────────────────

    #[test]
    fn transform_inserts_different_positions() {
        let base = "hello";
        // A inserts "X" at byte 2: "heXllo"
        let a = ChangeSet::from_insert(5, 2, "X");
        // B inserts "Y" at byte 4: "hellYo"
        let b = ChangeSet::from_insert(5, 4, "Y");
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_inserts_beginning_and_end() {
        let base = "hello";
        let a = ChangeSet::from_insert(5, 0, "A");
        let b = ChangeSet::from_insert(5, 5, "Z");
        assert_diamond(&a, &b, base);
    }

    // ── Test 3: Two inserts at same position (left-bias) ──────────────

    #[test]
    fn transform_inserts_same_position_left_bias() {
        let base = "hello";
        // Both insert at byte 3
        let a = ChangeSet::from_insert(5, 3, "AA");
        let b = ChangeSet::from_insert(5, 3, "BB");

        let (a_prime, b_prime) = ChangeSet::transform(&a, &b).unwrap();
        assert_diamond(&a, &b, base);

        // Left-bias: A's insert goes first. After applying A then B',
        // the result should be "helAABBlo" (A first, then B).
        let via_a = a.apply(base).unwrap();
        let final_doc = b_prime.apply(&via_a).unwrap();
        assert_eq!(final_doc, "helAABBlo");

        // Verify the same via B then A'
        let via_b = b.apply(base).unwrap();
        let final_doc_2 = a_prime.apply(&via_b).unwrap();
        assert_eq!(final_doc_2, "helAABBlo");
    }

    #[test]
    fn transform_inserts_same_position_at_start() {
        let base = "abc";
        let a = ChangeSet::from_insert(3, 0, "X");
        let b = ChangeSet::from_insert(3, 0, "Y");
        assert_diamond(&a, &b, base);

        // Left-bias: A first -> "XYabc"
        let via_a = a.apply(base).unwrap();
        let (_, b_prime) = ChangeSet::transform(&a, &b).unwrap();
        let result = b_prime.apply(&via_a).unwrap();
        assert_eq!(result, "XYabc");
    }

    // ── Test 4: Two non-overlapping deletes ───────────────────────────

    #[test]
    fn transform_deletes_non_overlapping() {
        let base = "abcdefghij";
        // A deletes [1..3] ("bc")
        let a = ChangeSet::from_delete(10, 1, 3);
        // B deletes [5..7] ("fg")
        let b = ChangeSet::from_delete(10, 5, 7);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_deletes_non_overlapping_reversed() {
        let base = "abcdefghij";
        // Swap order: A deletes later region, B deletes earlier
        let a = ChangeSet::from_delete(10, 5, 7);
        let b = ChangeSet::from_delete(10, 1, 3);
        assert_diamond(&a, &b, base);
    }

    // ── Test 5: Two deletes of same region (concurrent cancel) ────────

    #[test]
    fn transform_deletes_same_region() {
        let base = "abcdefghij";
        // Both delete [2..5] ("cde")
        let a = ChangeSet::from_delete(10, 2, 5);
        let b = ChangeSet::from_delete(10, 2, 5);
        assert_diamond(&a, &b, base);

        // After transform, both primes should not delete anything extra
        // (the concurrent deletes cancel). The final document should be
        // "abfghij" regardless of path.
        let via_a = a.apply(base).unwrap();
        assert_eq!(via_a, "abfghij");
        let (_, b_prime) = ChangeSet::transform(&a, &b).unwrap();
        let result = b_prime.apply(&via_a).unwrap();
        assert_eq!(result, "abfghij");
    }

    // ── Test 6: Overlapping deletes ───────────────────────────────────

    #[test]
    fn transform_deletes_overlapping() {
        let base = "abcdefghij";
        // A deletes [1..5] ("bcde")
        let a = ChangeSet::from_delete(10, 1, 5);
        // B deletes [3..7] ("defg")
        let b = ChangeSet::from_delete(10, 3, 7);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_deletes_overlapping_reversed() {
        let base = "abcdefghij";
        // Swap: A deletes [3..7], B deletes [1..5]
        let a = ChangeSet::from_delete(10, 3, 7);
        let b = ChangeSet::from_delete(10, 1, 5);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_a_delete_contains_b_delete() {
        let base = "abcdefghij";
        // A deletes entire range, B deletes subset
        let a = ChangeSet::from_delete(10, 1, 8);
        let b = ChangeSet::from_delete(10, 3, 5);
        assert_diamond(&a, &b, base);
    }

    // ── Test 7: Insert + Delete at same position ──────────────────────

    #[test]
    fn transform_insert_and_delete_same_position() {
        let base = "abcdefghij";
        // A inserts "XY" at position 3
        let a = ChangeSet::from_insert(10, 3, "XY");
        // B deletes [3..5] ("de")
        let b = ChangeSet::from_delete(10, 3, 5);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_delete_and_insert_same_position() {
        let base = "abcdefghij";
        // A deletes [3..5]
        let a = ChangeSet::from_delete(10, 3, 5);
        // B inserts "XY" at position 3
        let b = ChangeSet::from_insert(10, 3, "XY");
        assert_diamond(&a, &b, base);
    }

    // ── Test 8: Delete + Retain (one side is identity) ────────────────

    #[test]
    fn transform_delete_with_full_retain() {
        let base = "hello";
        let a = ChangeSet::from_delete(5, 2, 4);
        let b = ChangeSet::identity(5);
        assert_diamond(&a, &b, base);

        // A' should be the original A adapted for B's (no-op) output.
        // Since B is identity, B's output = base, so A' should behave
        // like A on that same text.
        let (a_prime, _) = ChangeSet::transform(&a, &b).unwrap();
        let via_b = b.apply(base).unwrap();
        let result = a_prime.apply(&via_b).unwrap();
        assert_eq!(result, a.apply(base).unwrap());
    }

    // ── Test 9: Complex mixed operations ──────────────────────────────

    #[test]
    fn transform_complex_mixed_a() {
        let base = "abcdefghij";
        // A: insert "X" at 2, delete [5..7] ("fg")
        let a = ChangeSet::from_changes(10, [(2, 2, Some("X")), (5, 7, None)]);
        // B: insert "Y" at 8, delete [0..1] ("a")
        let b = ChangeSet::from_changes(10, [(0, 1, None), (8, 8, Some("Y"))]);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_complex_mixed_b() {
        let base = "abcdefghijklmnop";
        // A: replace [2..5] ("cde") with "XY", insert "!" at 10
        let a = ChangeSet::from_changes(16, [(2, 5, Some("XY")), (10, 10, Some("!"))]);
        // B: delete [0..2] ("ab"), replace [8..12] ("ijkl") with "Z"
        let b = ChangeSet::from_changes(16, [(0, 2, None), (8, 12, Some("Z"))]);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_complex_both_replace_overlapping_regions() {
        let base = "abcdefghij";
        // A: replace [2..6] ("cdef") with "XX"
        let a = ChangeSet::from_replace(10, 2, 6, "XX");
        // B: replace [4..8] ("efgh") with "YY"
        let b = ChangeSet::from_replace(10, 4, 8, "YY");
        assert_diamond(&a, &b, base);
    }

    // ── Test 10: Empty changesets (both identity on empty doc) ─────────

    #[test]
    fn transform_empty_doc_both_identity() {
        let base = "";
        let id = ChangeSet::identity(0);
        let (a_prime, b_prime) = ChangeSet::transform(&id, &id).unwrap();
        assert!(a_prime.is_identity());
        assert!(b_prime.is_identity());
        assert_diamond(&id, &id, base);
    }

    #[test]
    fn transform_empty_doc_both_insert() {
        let base = "";
        let a = ChangeSet::from_insert(0, 0, "hello");
        let b = ChangeSet::from_insert(0, 0, "world");
        assert_diamond(&a, &b, base);
    }

    // ── Test 11: TransformMismatch error ──────────────────────────────

    #[test]
    fn transform_mismatch_error() {
        let a = ChangeSet::identity(5);
        let b = ChangeSet::identity(10);
        let err = ChangeSet::transform(&a, &b).unwrap_err();
        assert_eq!(
            err,
            ChangeSetError::TransformMismatch {
                a_input: 5,
                b_input: 10,
            }
        );
    }

    #[test]
    fn transform_mismatch_with_actual_ops() {
        let a = ChangeSet::from_insert(5, 2, "X");
        let b = ChangeSet::from_delete(8, 1, 3);
        let err = ChangeSet::transform(&a, &b).unwrap_err();
        assert_eq!(
            err,
            ChangeSetError::TransformMismatch {
                a_input: 5,
                b_input: 8,
            }
        );
    }

    // ── Test 12: Symmetry ─────────────────────────────────────────────

    #[test]
    fn transform_symmetry_both_satisfy_diamond() {
        let base = "abcdefghij";
        let a = ChangeSet::from_insert(10, 3, "XX");
        let b = ChangeSet::from_delete(10, 5, 8);

        // transform(a, b) satisfies diamond
        assert_diamond(&a, &b, base);
        // transform(b, a) also satisfies diamond
        assert_diamond(&b, &a, base);
    }

    #[test]
    fn transform_symmetry_inserts_same_position() {
        let base = "hello";
        let a = ChangeSet::from_insert(5, 2, "XX");
        let b = ChangeSet::from_insert(5, 2, "YY");

        // Both orderings satisfy diamond
        assert_diamond(&a, &b, base);
        assert_diamond(&b, &a, base);

        // But the final document order differs: left-bias means
        // the first argument's insert comes first.
        let (_, b_prime_1) = ChangeSet::transform(&a, &b).unwrap();
        let result_1 = b_prime_1.apply(&a.apply(base).unwrap()).unwrap();

        let (_, a_prime_2) = ChangeSet::transform(&b, &a).unwrap();
        let result_2 = a_prime_2.apply(&b.apply(base).unwrap()).unwrap();

        // transform(a,b): A first -> "heXXYYllo"
        assert_eq!(result_1, "heXXYYllo");
        // transform(b,a): B first -> "heYYXXllo"
        assert_eq!(result_2, "heYYXXllo");
    }

    // ── Additional edge cases ─────────────────────────────────────────

    #[test]
    fn transform_a_insert_b_delete_entire_doc() {
        let base = "hello";
        let a = ChangeSet::from_insert(5, 2, "XY");
        let b = ChangeSet::from_delete(5, 0, 5);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_both_delete_entire_doc() {
        let base = "hello";
        let a = ChangeSet::from_delete(5, 0, 5);
        let b = ChangeSet::from_delete(5, 0, 5);
        assert_diamond(&a, &b, base);

        // Both delete everything -- final document should be empty
        let via_a = a.apply(base).unwrap();
        let (_, b_prime) = ChangeSet::transform(&a, &b).unwrap();
        let result = b_prime.apply(&via_a).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn transform_replace_vs_replace_non_overlapping() {
        let base = "abcdefghij";
        let a = ChangeSet::from_replace(10, 1, 3, "XX");
        let b = ChangeSet::from_replace(10, 6, 9, "YY");
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_insert_at_delete_boundary() {
        let base = "abcdefghij";
        // A inserts right before the region B deletes
        let a = ChangeSet::from_insert(10, 3, "!!");
        let b = ChangeSet::from_delete(10, 3, 6);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_insert_right_after_delete_region() {
        let base = "abcdefghij";
        // A inserts right after the region B deletes
        let a = ChangeSet::from_insert(10, 6, "!!");
        let b = ChangeSet::from_delete(10, 3, 6);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_multiple_inserts_vs_multiple_deletes() {
        let base = "abcdefghij";
        let a = ChangeSet::from_changes(10, [(1, 1, Some("X")), (5, 5, Some("Y"))]);
        let b = ChangeSet::from_changes(10, [(2, 4, None), (7, 9, None)]);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_single_char_doc() {
        let base = "x";
        let a = ChangeSet::from_insert(1, 0, "A");
        let b = ChangeSet::from_insert(1, 1, "B");
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_utf8_inserts() {
        // "hello" = 5 bytes
        let base = "hello";
        let a = ChangeSet::from_insert(5, 2, "\u{00e9}"); // e-acute, 2 bytes
        let b = ChangeSet::from_insert(5, 4, "\u{4e16}"); // CJK character, 3 bytes
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_large_retain_spans() {
        // Simulate edits far apart in a large document
        let base = "a]b]c]d]e]f]g]h]i]j]"; // 20 bytes
        let a = ChangeSet::from_insert(20, 2, "X");
        let b = ChangeSet::from_insert(20, 18, "Y");
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_adjacent_deletes() {
        let base = "abcdefghij";
        // A deletes [2..4] ("cd"), B deletes [4..6] ("ef") -- adjacent, not overlapping
        let a = ChangeSet::from_delete(10, 2, 4);
        let b = ChangeSet::from_delete(10, 4, 6);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_replace_vs_delete_of_same_range() {
        let base = "abcdefghij";
        // A replaces [3..6] with "XY", B deletes [3..6]
        let a = ChangeSet::from_replace(10, 3, 6, "XY");
        let b = ChangeSet::from_delete(10, 3, 6);
        assert_diamond(&a, &b, base);
    }

    #[test]
    fn transform_idempotent_identity() {
        // transform(id, id) should produce (id, id)
        let id = ChangeSet::identity(42);
        let (a_prime, b_prime) = ChangeSet::transform(&id, &id).unwrap();
        assert!(a_prime.is_identity());
        assert!(b_prime.is_identity());
    }
}
