//! Bidirectional conversion between the [`Effect`] system and
//! [`ChangeSet`](crate::primitives::ChangeSet) algebra.
//!
//! The bridge provides two functions:
//!
//! - [`effects_to_changeset`] — collapse a sequence of text-mutating effects
//!   into a single composed [`ChangeSet`](crate::primitives::ChangeSet). Non-text
//!   effects are silently skipped.
//! - [`changeset_to_effects`] — decompose a [`ChangeSet`](crate::primitives::ChangeSet) into minimal
//!   [`Effect::Insert`] and [`Effect::Delete`] operations.
//!
//! Together these enable round-tripping between the effect stream (what the
//! engine produces) and the algebraic representation (what OT/CRDT composition,
//! inversion, and position-mapping require).
//!
//! # Import constraints
//!
//! This module only depends on:
//! - `primitives` — `ChangeSet`, `Offset`, `Range`
//! - sibling `effect` module — `Effect`
//! - `compact_str` — `CompactString` (same as `effect.rs`)
//! - `smallvec` — `SmallVec` (crate-wide dependency)

use compact_str::CompactString;
use smallvec::SmallVec;

use crate::effects::effect::Effect;
use crate::primitives::{ChangeSet, Offset, Range};

/// Convert a slice of [`Effect`]s into a single composed [`ChangeSet`].
///
/// Only text-mutating effects ([`Effect::Insert`], [`Effect::Delete`],
/// [`Effect::Replace`]) contribute to the result. All other effects are
/// silently ignored and produce no change.
///
/// # Coordinate model
///
/// Effects use **post-mutation coordinates**: each effect's offsets are
/// relative to the document *after* all preceding effects have been applied.
/// This function builds a micro-[`ChangeSet`] for each text effect using
/// the running output length, then composes incrementally.
///
/// # Arguments
///
/// - `effects` — ordered sequence of effects (as produced by the engine).
/// - `doc_len` — byte length of the document *before* any effects are applied.
///
/// # Returns
///
/// A single [`ChangeSet`] whose `input_len` is `doc_len` and whose
/// `output_len` reflects the cumulative result of all text mutations.
/// If no text-mutating effects are present, returns `ChangeSet::identity(doc_len)`.
#[must_use]
pub fn effects_to_changeset(effects: &[Effect], doc_len: usize) -> ChangeSet {
    let mut composed = ChangeSet::identity(doc_len);

    for effect in effects {
        let current_len = composed.output_len();

        let micro = match effect {
            Effect::Insert { offset, text } => {
                let pos = offset.get().min(current_len);
                ChangeSet::from_insert(current_len, pos, text)
            }
            Effect::Delete { range } => {
                let start = range.start().get().min(current_len);
                let end = range.end().get().min(current_len);
                ChangeSet::from_delete(current_len, start, end)
            }
            Effect::Replace { range, text } => {
                let start = range.start().get().min(current_len);
                let end = range.end().get().min(current_len);
                ChangeSet::from_replace(current_len, start, end, text)
            }
            _ => continue,
        };

        // Sequential effects on the same logical document always compose.
        // The micro-changeset's input_len equals composed.output_len by
        // construction, so this should never fail.
        composed = if let Ok(cs) = composed.compose(&micro) {
            cs
        } else {
            // Defensive: if composition fails due to an unexpected
            // mismatch, return what we have so far rather than hiding
            // the problem. In debug builds the assertion in compose()
            // will already have fired.
            debug_assert!(false, "sequential effect composition failed unexpectedly");
            return composed;
        };
    }

    composed
}

/// Convert a [`ChangeSet`] into minimal [`Effect::Insert`] and [`Effect::Delete`]
/// operations.
///
/// The returned effects use **post-mutation coordinates** — each effect's
/// offsets are relative to the document *after* all preceding effects in
/// the returned list have been applied.
///
/// # Decomposition rules
///
/// The [`ChangeSet::changes`] iterator yields `(from, to, Option<&str>)`
/// triples in original-document coordinates. These are converted as follows:
///
/// - `(from, to, None)` where `from < to` — pure deletion.
/// - `(from, from, Some(text))` — pure insertion.
/// - `(from, to, Some(text))` where `from < to` — replacement, decomposed
///   into a [`Effect::Delete`] followed by an [`Effect::Insert`] at the same
///   position.
///
/// Because the `ChangeIter` yields triples in *original* coordinates but
/// effects use *post-mutation* coordinates, we track a running delta to
/// adjust offsets for each subsequent effect.
///
/// # Returns
///
/// A `SmallVec<[Effect; 4]>` — typically small for single edits, but grows
/// for multi-site changes. Returns an empty vec for identity changesets.
#[must_use]
pub fn changeset_to_effects(cs: &ChangeSet) -> SmallVec<[Effect; 4]> {
    let mut result: SmallVec<[Effect; 4]> = SmallVec::new();

    // Track the cumulative offset shift caused by preceding effects.
    // Insertions increase document length (+text.len()), deletions
    // decrease it (-(to - from)). Each subsequent effect's position
    // must be adjusted by this delta.
    let mut delta: isize = 0;

    for (from, to, text) in cs.changes() {
        let del_len = to - from;

        match (del_len > 0, text) {
            // Pure deletion
            (true, None) => {
                let adjusted_from = adjust_offset(from, delta);
                let adjusted_to = adjusted_from + del_len;
                result.push(Effect::Delete {
                    range: Range::new(Offset::new(adjusted_from), Offset::new(adjusted_to)),
                });
                delta -= del_len.cast_signed();
            }
            // Pure insertion
            (false, Some(ins_text)) => {
                let adjusted_pos = adjust_offset(from, delta);
                result.push(Effect::Insert {
                    offset: Offset::new(adjusted_pos),
                    text: CompactString::from(ins_text),
                });
                delta += ins_text.len().cast_signed();
            }
            // Replacement: decompose into Delete + Insert at same position
            (true, Some(ins_text)) => {
                let adjusted_from = adjust_offset(from, delta);
                let adjusted_to = adjusted_from + del_len;

                result.push(Effect::Delete {
                    range: Range::new(Offset::new(adjusted_from), Offset::new(adjusted_to)),
                });
                delta -= del_len.cast_signed();

                // After the delete, insert at the same position
                result.push(Effect::Insert {
                    offset: Offset::new(adjusted_from),
                    text: CompactString::from(ins_text),
                });
                delta += ins_text.len().cast_signed();
            }
            // No deletion and no insertion — nothing to emit
            (false, None) => {}
        }
    }

    result
}

/// Apply a signed delta to an unsigned offset.
///
/// Saturates at zero on underflow (should not happen with well-formed
/// changesets, but defensive coding).
const fn adjust_offset(base: usize, delta: isize) -> usize {
    base.saturating_add_signed(delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Mode, Offset, Range};

    // ── Helper: apply effects to a string to get the result ──────────

    fn apply_effects(text: &str, effects: &[Effect]) -> String {
        let mut result = String::from(text);
        for effect in effects {
            match effect {
                Effect::Insert { offset, text } => {
                    let pos = offset.get().min(result.len());
                    result.insert_str(pos, text);
                }
                Effect::Delete { range } => {
                    let start = range.start().get().min(result.len());
                    let end = range.end().get().min(result.len());
                    result.replace_range(start..end, "");
                }
                Effect::Replace { range, text } => {
                    let start = range.start().get().min(result.len());
                    let end = range.end().get().min(result.len());
                    result.replace_range(start..end, text);
                }
                _ => {}
            }
        }
        result
    }

    // ── 1. Single Insert ─────────────────────────────────────────────

    #[test]
    fn single_insert_to_changeset() {
        let effects = [Effect::Insert {
            offset: Offset::new(3),
            text: CompactString::from("XY"),
        }];
        let cs = effects_to_changeset(&effects, 10);

        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 12);
        assert!(cs.has_changes());

        // Verify by applying to a concrete string
        let original = "abcdefghij";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "abcXYdefghij");
    }

    // ── 2. Single Delete ─────────────────────────────────────────────

    #[test]
    fn single_delete_to_changeset() {
        let effects = [Effect::Delete {
            range: Range::from_raw(2, 5),
        }];
        let cs = effects_to_changeset(&effects, 10);

        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 7);
        assert!(cs.has_changes());

        let original = "abcdefghij";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "abfghij");
    }

    // ── 3. Single Replace ────────────────────────────────────────────

    #[test]
    fn single_replace_to_changeset() {
        let effects = [Effect::Replace {
            range: Range::from_raw(1, 4),
            text: CompactString::from("HELLO"),
        }];
        let cs = effects_to_changeset(&effects, 10);

        assert_eq!(cs.input_len(), 10);
        // 10 - 3 (deleted) + 5 (inserted) = 12
        assert_eq!(cs.output_len(), 12);

        let original = "abcdefghij";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "aHELLOefghij");
    }

    // ── 4. Sequential Insert then Delete (post-mutation coords) ──────

    #[test]
    fn sequential_insert_then_delete() {
        // Start: "abcdefghij" (10 bytes)
        // Effect 1: Insert "XY" at offset 3 -> "abcXYdefghij" (12 bytes)
        // Effect 2: Delete range [5, 8) in the 12-byte doc -> "abcXYghij" (9 bytes)
        //   (deletes "def" — positions 5,6,7 in the post-insertion document)
        let effects = [
            Effect::Insert {
                offset: Offset::new(3),
                text: CompactString::from("XY"),
            },
            Effect::Delete {
                range: Range::from_raw(5, 8),
            },
        ];
        let cs = effects_to_changeset(&effects, 10);

        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 9); // 10 + 2 (insert) - 3 (delete) = 9

        let original = "abcdefghij";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "abcXYghij");
    }

    // ── 5. Non-text effects are ignored ──────────────────────────────

    #[test]
    fn non_text_effects_are_identity() {
        let effects = [
            Effect::set_mode(Mode::Normal),
            Effect::SetCursor {
                offset: Offset::new(5),
            },
            Effect::ClearSelection,
            Effect::Noop,
        ];
        let cs = effects_to_changeset(&effects, 42);
        assert!(cs.is_identity());
        assert_eq!(cs.input_len(), 42);
        assert_eq!(cs.output_len(), 42);
    }

    // ── 6. Empty effects list ────────────────────────────────────────

    #[test]
    fn empty_effects_list() {
        let cs = effects_to_changeset(&[], 100);
        assert!(cs.is_identity());
        assert_eq!(cs.input_len(), 100);
        assert_eq!(cs.output_len(), 100);
    }

    // ── 7. Round-trip: effects -> changeset -> effects -> same result ─

    #[test]
    fn round_trip_single_insert() {
        let original = "hello world";
        let doc_len = original.len();
        let effects = [Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::from(" beautiful"),
        }];

        // Forward: effects -> changeset
        let cs = effects_to_changeset(&effects, doc_len);
        // Reverse: changeset -> effects
        let round_tripped = changeset_to_effects(&cs);

        // Both should produce the same result
        let via_original = apply_effects(original, &effects);
        let via_round_trip = apply_effects(original, &round_tripped);
        assert_eq!(via_original, via_round_trip);
    }

    #[test]
    fn round_trip_single_delete() {
        let original = "hello world";
        let doc_len = original.len();
        let effects = [Effect::Delete {
            range: Range::from_raw(5, 11),
        }];

        let cs = effects_to_changeset(&effects, doc_len);
        let round_tripped = changeset_to_effects(&cs);

        let via_original = apply_effects(original, &effects);
        let via_round_trip = apply_effects(original, &round_tripped);
        assert_eq!(via_original, via_round_trip);
    }

    #[test]
    fn round_trip_replace() {
        let original = "hello world";
        let doc_len = original.len();
        let effects = [Effect::Replace {
            range: Range::from_raw(0, 5),
            text: CompactString::from("goodbye"),
        }];

        let cs = effects_to_changeset(&effects, doc_len);
        let round_tripped = changeset_to_effects(&cs);

        let via_original = apply_effects(original, &effects);
        let via_round_trip = apply_effects(original, &round_tripped);
        assert_eq!(via_original, via_round_trip);
    }

    #[test]
    fn round_trip_multiple_sequential_effects() {
        let original = "abcdefghij";
        let doc_len = original.len();

        // Insert then delete (post-mutation coordinates)
        let effects = [
            Effect::Insert {
                offset: Offset::new(2),
                text: CompactString::from("XX"),
            },
            Effect::Delete {
                range: Range::from_raw(6, 9),
            },
        ];

        let cs = effects_to_changeset(&effects, doc_len);
        let round_tripped = changeset_to_effects(&cs);

        let via_original = apply_effects(original, &effects);
        let via_round_trip = apply_effects(original, &round_tripped);
        assert_eq!(via_original, via_round_trip);
    }

    // ── 8. changeset_to_effects on identity ──────────────────────────

    #[test]
    fn changeset_to_effects_identity_is_empty() {
        let cs = ChangeSet::identity(42);
        let effects = changeset_to_effects(&cs);
        assert!(effects.is_empty());
    }

    #[test]
    fn changeset_to_effects_identity_empty_doc() {
        let cs = ChangeSet::identity(0);
        let effects = changeset_to_effects(&cs);
        assert!(effects.is_empty());
    }

    // ── Additional edge cases ────────────────────────────────────────

    #[test]
    fn insert_at_beginning() {
        let effects = [Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::from("prefix"),
        }];
        let cs = effects_to_changeset(&effects, 5);

        let original = "world";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "prefixworld");
    }

    #[test]
    fn insert_at_end() {
        let effects = [Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::from("suffix"),
        }];
        let cs = effects_to_changeset(&effects, 5);

        let original = "hello";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "hellosuffix");
    }

    #[test]
    fn delete_entire_document() {
        let effects = [Effect::Delete {
            range: Range::from_raw(0, 10),
        }];
        let cs = effects_to_changeset(&effects, 10);

        assert_eq!(cs.output_len(), 0);
        let original = "abcdefghij";
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_cs, "");
    }

    #[test]
    fn insert_into_empty_document() {
        let effects = [Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::from("hello"),
        }];
        let cs = effects_to_changeset(&effects, 0);

        assert_eq!(cs.input_len(), 0);
        assert_eq!(cs.output_len(), 5);
        let via_cs = cs.apply("").unwrap();
        assert_eq!(via_cs, "hello");
    }

    #[test]
    fn mixed_text_and_non_text_effects() {
        let effects: Vec<Effect> = vec![
            Effect::set_mode(Mode::Insert),
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::from("X"),
            },
            Effect::SetCursor {
                offset: Offset::new(1),
            },
            Effect::Delete {
                range: Range::from_raw(1, 3),
            },
            Effect::ClearMessage,
        ];
        let cs = effects_to_changeset(&effects, 5);

        // Only Insert and Delete matter: insert "X" at 0 (len=6), delete [1,3) (len=4)
        let original = "abcde";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
    }

    #[test]
    fn changeset_to_effects_single_insert() {
        let cs = ChangeSet::from_insert(10, 3, "XY");
        let effects = changeset_to_effects(&cs);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            Effect::Insert { offset, text } => {
                assert_eq!(offset.get(), 3);
                assert_eq!(text.as_str(), "XY");
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    #[test]
    fn changeset_to_effects_single_delete() {
        let cs = ChangeSet::from_delete(10, 2, 6);
        let effects = changeset_to_effects(&cs);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            Effect::Delete { range } => {
                assert_eq!(range.start().get(), 2);
                assert_eq!(range.end().get(), 6);
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn changeset_to_effects_replace_decomposes() {
        let cs = ChangeSet::from_replace(10, 2, 5, "HELLO");
        let effects = changeset_to_effects(&cs);
        // Replace decomposes into Delete + Insert
        assert_eq!(effects.len(), 2);
        match &effects[0] {
            Effect::Delete { range } => {
                assert_eq!(range.start().get(), 2);
                assert_eq!(range.end().get(), 5);
            }
            other => panic!("expected Delete, got {other:?}"),
        }
        match &effects[1] {
            Effect::Insert { offset, text } => {
                assert_eq!(offset.get(), 2);
                assert_eq!(text.as_str(), "HELLO");
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    #[test]
    fn changeset_to_effects_multi_change() {
        // Multiple changes in original coordinates
        let cs = ChangeSet::from_changes(
            10,
            [
                (1, 3, None),       // delete
                (5, 5, Some("X")),  // insert
                (7, 9, Some("YZ")), // replace
            ],
        );
        let effects = changeset_to_effects(&cs);

        // Should produce: Delete, Insert, Delete+Insert
        assert_eq!(effects.len(), 4);

        // First: delete [1,3) -> post-mutation delta = -2
        match &effects[0] {
            Effect::Delete { range } => {
                assert_eq!(range.start().get(), 1);
                assert_eq!(range.end().get(), 3);
            }
            other => panic!("expected Delete at index 0, got {other:?}"),
        }

        // Second: insert at 5, but adjusted by delta=-2 -> pos 3
        match &effects[1] {
            Effect::Insert { offset, text } => {
                assert_eq!(offset.get(), 3);
                assert_eq!(text.as_str(), "X");
            }
            other => panic!("expected Insert at index 1, got {other:?}"),
        }

        // Third: delete [7,9) adjusted by delta=-2+1=-1 -> [6,8)
        match &effects[2] {
            Effect::Delete { range } => {
                assert_eq!(range.start().get(), 6);
                assert_eq!(range.end().get(), 8);
            }
            other => panic!("expected Delete at index 2, got {other:?}"),
        }

        // Fourth: insert at 7, adjusted by delta=-1-2=-3 -> pos 6
        match &effects[3] {
            Effect::Insert { offset, text } => {
                assert_eq!(offset.get(), 6);
                assert_eq!(text.as_str(), "YZ");
            }
            other => panic!("expected Insert at index 3, got {other:?}"),
        }

        // Verify the round-trip produces the same text
        let original = "abcdefghij";
        let via_cs = cs.apply(original).unwrap();
        let via_effects = apply_effects(original, &effects);
        assert_eq!(via_cs, via_effects);
    }

    #[test]
    fn round_trip_complex_changeset() {
        // Build a ChangeSet from changes, convert to effects, apply both, compare
        let original = "the quick brown fox jumps";
        let cs = ChangeSet::from_changes(
            original.len(),
            [
                (0, 3, Some("a")),         // "the" -> "a"
                (10, 15, None),            // delete "brown"
                (20, 20, Some(" lazily")), // insert " lazily" before "jumps"
            ],
        );

        let via_cs = cs.apply(original).unwrap();
        let effects = changeset_to_effects(&cs);
        let via_effects = apply_effects(original, &effects);
        assert_eq!(via_cs, via_effects);
    }

    #[test]
    fn utf8_multibyte_insert() {
        // "hello" (5 bytes) -> insert "世界" (6 bytes) at position 2
        let effects = [Effect::Insert {
            offset: Offset::new(2),
            text: CompactString::from("世界"),
        }];
        let cs = effects_to_changeset(&effects, 5);

        assert_eq!(cs.input_len(), 5);
        assert_eq!(cs.output_len(), 11); // 5 + 6

        let original = "hello";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "he世界llo");
    }

    #[test]
    fn three_sequential_inserts() {
        // All in post-mutation coordinates
        let effects = [
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::from("A"),
            },
            Effect::Insert {
                offset: Offset::new(1),
                text: CompactString::from("B"),
            },
            Effect::Insert {
                offset: Offset::new(2),
                text: CompactString::from("C"),
            },
        ];
        let cs = effects_to_changeset(&effects, 3);

        let original = "xyz";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "ABCxyz");
    }

    #[test]
    fn replace_with_shorter_text() {
        let effects = [Effect::Replace {
            range: Range::from_raw(0, 10),
            text: CompactString::from("hi"),
        }];
        let cs = effects_to_changeset(&effects, 10);

        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 2);

        let original = "abcdefghij";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "hi");
    }

    #[test]
    fn replace_with_same_length_text() {
        let effects = [Effect::Replace {
            range: Range::from_raw(2, 5),
            text: CompactString::from("XYZ"),
        }];
        let cs = effects_to_changeset(&effects, 10);

        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 10);
        assert!(cs.has_changes()); // not identity despite same length

        let original = "abcdefghij";
        let via_effects = apply_effects(original, &effects);
        let via_cs = cs.apply(original).unwrap();
        assert_eq!(via_effects, via_cs);
        assert_eq!(via_cs, "abXYZfghij");
    }
}
