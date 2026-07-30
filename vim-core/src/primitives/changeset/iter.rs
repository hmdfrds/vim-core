//! `ChangeIter` — convert ops to `(from, to, Option<&str>)` triples.
//!
//! This iterator provides a higher-level view of a changeset,
//! yielding the concrete changes in original-document coordinates:
//!
//! - `(from, to, None)` — deletion of `from..to`
//! - `(pos, pos, Some(text))` — pure insertion at `pos`
//! - `(from, to, Some(text))` — replacement of `from..to` with `text`
//!
//! Retains are skipped (they don't represent changes).

use super::change_set::ChangeSet;
use super::text_op::TextOp;

/// Iterator over changes as `(from, to, Option<&str>)` triples.
///
/// Yields only actual changes (insertions, deletions, replacements).
/// `Retain` ops are consumed internally to track position but not yielded.
pub struct ChangeIter<'a> {
    ops: std::slice::Iter<'a, TextOp>,
    /// Peeked next op (used to combine Insert+Delete into replacement).
    peeked: Option<&'a TextOp>,
    pos: usize,
}

impl ChangeSet {
    /// Iterate over changes as `(from, to, Option<&str>)` triples.
    ///
    /// - `(from, to, None)` — deletion of bytes `from..to`
    /// - `(pos, pos, Some(text))` — insertion of `text` at `pos`
    /// - `(from, to, Some(text))` — replacement of `from..to` with `text`
    ///
    /// Triples are in document order, suitable for `from_changes`.
    #[must_use]
    pub fn changes(&self) -> ChangeIter<'_> {
        ChangeIter {
            ops: self.ops().iter(),
            peeked: None,
            pos: 0,
        }
    }
}

impl<'a> ChangeIter<'a> {
    /// Advance to the next op, returning from peek buffer first.
    fn next_op(&mut self) -> Option<&'a TextOp> {
        self.peeked.take().or_else(|| self.ops.next())
    }

    /// Peek at the next op without consuming it.
    fn peek_op(&mut self) -> Option<&'a TextOp> {
        if self.peeked.is_none() {
            self.peeked = self.ops.next();
        }
        self.peeked
    }
}

impl<'a> Iterator for ChangeIter<'a> {
    type Item = (usize, usize, Option<&'a str>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let op = self.next_op()?;

            match op {
                TextOp::Retain(n) => {
                    self.pos += n;
                    // Skip retains — loop to next op
                }
                TextOp::Insert(t) => {
                    let from = self.pos;

                    // Peek: if next op is Delete, combine into a replacement
                    if let Some(TextOp::Delete(n)) = self.peek_op() {
                        let n = *n;
                        let to = from + n;
                        self.pos = to;
                        self.peeked = None; // consume the Delete
                        return Some((from, to, Some(t.as_str())));
                    }

                    // Pure insertion
                    return Some((from, from, Some(t.as_str())));
                }
                TextOp::Delete(n) => {
                    let from = self.pos;
                    let to = from + n;
                    self.pos = to;
                    return Some((from, to, None));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_identity() {
        let cs = ChangeSet::identity(10);
        let changes: Vec<_> = cs.changes().collect();
        assert!(changes.is_empty());
    }

    #[test]
    fn iter_single_insert() {
        let cs = ChangeSet::from_insert(5, 2, "XX");
        let changes: Vec<_> = cs.changes().collect();
        assert_eq!(changes, vec![(2, 2, Some("XX"))]);
    }

    #[test]
    fn iter_single_delete() {
        let cs = ChangeSet::from_delete(10, 3, 7);
        let changes: Vec<_> = cs.changes().collect();
        assert_eq!(changes, vec![(3, 7, None)]);
    }

    #[test]
    fn iter_replace() {
        // Replace is Insert+Delete in canonical order
        let cs = ChangeSet::from_replace(10, 3, 6, "XY");
        let changes: Vec<_> = cs.changes().collect();
        // Should yield a single replacement triple
        assert_eq!(changes, vec![(3, 6, Some("XY"))]);
    }

    #[test]
    fn iter_multiple_changes() {
        let cs = ChangeSet::from_changes(
            10,
            [
                (1, 3, None),       // delete
                (5, 5, Some("X")),  // insert
                (7, 9, Some("YZ")), // replace
            ],
        );
        let changes: Vec<_> = cs.changes().collect();
        assert_eq!(
            changes,
            vec![(1, 3, None), (5, 5, Some("X")), (7, 9, Some("YZ")),]
        );
    }

    #[test]
    fn iter_empty_changeset() {
        let cs = ChangeSet::identity(0);
        let changes: Vec<_> = cs.changes().collect();
        assert!(changes.is_empty());
    }

    #[test]
    fn iter_round_trip_from_changes() {
        // Create from changes, iterate back to changes, create again — should be equivalent
        let original_changes: Vec<(usize, usize, Option<&str>)> =
            vec![(2, 4, Some("XY")), (7, 7, Some("!"))];
        let cs1 = ChangeSet::from_changes(10, original_changes.iter().copied());
        let triples: Vec<_> = cs1.changes().collect();
        let cs2 = ChangeSet::from_changes(10, triples.into_iter());
        assert_eq!(cs1, cs2);
    }

    #[test]
    fn iter_delete_at_beginning() {
        let cs = ChangeSet::from_delete(10, 0, 5);
        let changes: Vec<_> = cs.changes().collect();
        assert_eq!(changes, vec![(0, 5, None)]);
    }

    #[test]
    fn iter_insert_at_end() {
        let cs = ChangeSet::from_insert(5, 5, "end");
        let changes: Vec<_> = cs.changes().collect();
        assert_eq!(changes, vec![(5, 5, Some("end"))]);
    }
}
