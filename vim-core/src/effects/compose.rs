//! Effect algebra — composition and simplification of effect streams.
//!
//! Two complementary optimizations for reducing effect stream size:
//!
//! - **Composition** ([`compose_effects`]) — fuses *adjacent* compatible pairs
//!   into a single effect (local optimization).
//! - **Simplification** ([`simplify_effects`]) — removes *non-adjacent* redundant
//!   effects and empty structural patterns (global optimization).
//!
//! # Composition rules (local, adjacent pairs)
//!
//! | Pattern                                                              | Result                                          | Condition                                           |
//! |----------------------------------------------------------------------|-------------------------------------------------|-----------------------------------------------------|
//! | `Insert(a, text_a) + Insert(b, text_b)`                             | `Insert(a, text_a + text_b)`                    | `b == a + text_a.len()` (forward-adjacent)          |
//! | `Delete(range_a) + Delete(range_b)`                                 | `Delete(a.start .. a.start + a.len + b.len)`    | `b.start == a.start` (same start after first delete)|
//! | `Insert(at, text) + Delete(range)`                                   | `Noop`                                          | `range.start == at && range.len == text.len`        |
//! | `SetCursor(a) + SetCursor(b)`                                       | `SetCursor(b)`                                  | Always (last wins)                                  |
//! | `SetMode(a) + SetMode(b)`                                           | `SetMode(b)`                                    | Always (last wins)                                  |
//!
//! # Simplification rules (global, non-adjacent)
//!
//! | Rule                       | Behavior                                                              |
//! |----------------------------|-----------------------------------------------------------------------|
//! | Last `SetCursor` wins      | Only the final `SetCursor` in the stream is kept                      |
//! | Last `SetMode` wins        | Only the final `SetMode` in the stream is kept                        |
//! | Empty undo groups removed  | `BeginUndoGroup`…`EndUndoGroup` with no text mutations → both removed |
//!
//! # Import constraints
//!
//! This module only depends on:
//! - `primitives` — `Offset`, `Range`, `Mode`
//! - sibling `effect` module — `Effect`
//! - `compact_str` — `CompactString` (same as `effect.rs`)
//!
//! It does NOT import `state`, `commands`, `execution`, or any other layer.

use compact_str::CompactString;
use smallvec::SmallVec;

use crate::effects::effect::Effect;
use crate::primitives::{Offset, Range};

// ═══════════════════════════════════════════════════════════════════════════════
// PAIRWISE COMPOSITION
// ═══════════════════════════════════════════════════════════════════════════════

/// Attempt to fuse two adjacent effects into one.
///
/// Returns `Some(fused)` if the effects are composable, `None` otherwise.
/// The caller must ensure that `a` appears immediately before `b` in the
/// effect stream — this function does not reorder effects.
///
/// # Complexity
///
/// Time: O(t) where t = combined text length (for Insert fusion string concatenation);
///       O(1) for all other rules
/// Space: O(t) for Insert fusion; O(1) otherwise
///
/// # Composition rules
///
/// - **Adjacent inserts:** `Insert(offset_a, text_a)` followed by
///   `Insert(offset_b, text_b)` where `offset_b == offset_a + text_a.len()`
///   fuses to `Insert(offset_a, text_a + text_b)`.
///
/// - **Adjacent deletes:** After `Delete(range_a)` executes, all text shifts
///   left by `range_a.len()`. A subsequent `Delete(range_b)` that starts at
///   `range_a.start()` (i.e., "keep deleting forward from the same position")
///   fuses to `Delete(range_a.start() .. range_a.start() + range_a.len() + range_b.len())`.
///
/// - **Insert+Delete annihilation:** `Insert(at, text)` followed by
///   `Delete(range)` where `range.start() == at` and `range.len() == text.len()`
///   annihilates to `Noop` (the delete undoes the insert exactly).
///
/// - **SetCursor last-wins:** `SetCursor(a)` followed by `SetCursor(b)`
///   fuses to `SetCursor(b)`. No adjacency check needed.
///
/// - **SetMode last-wins:** `SetMode(a)` followed by `SetMode(b)` fuses to
///   `SetMode(b)`. No adjacency check needed.
#[must_use]
pub fn try_compose(a: &Effect, b: &Effect) -> Option<Effect> {
    match (a, b) {
        // ── Adjacent inserts ─────────────────────────────────────────────
        //
        // Insert(offset_a, text_a) + Insert(offset_b, text_b)
        // where offset_b == offset_a + text_a.len()
        // → Insert(offset_a, text_a + text_b)
        (
            Effect::Insert {
                offset: offset_a,
                text: text_a,
            },
            Effect::Insert {
                offset: offset_b,
                text: text_b,
            },
        ) => {
            let expected_b = offset_a.get() + text_a.len();
            if offset_b.get() == expected_b {
                let mut fused_text = CompactString::with_capacity(text_a.len() + text_b.len());
                fused_text.push_str(text_a);
                fused_text.push_str(text_b);
                Some(Effect::Insert {
                    offset: *offset_a,
                    text: fused_text,
                })
            } else {
                None
            }
        }

        // ── Adjacent deletes ─────────────────────────────────────────────
        //
        // After Delete(range_a), text shifts left by range_a.len().
        // If the next delete starts at range_a.start() (the "keep deleting
        // forward" pattern), the combined range spans both.
        (Effect::Delete { range: range_a }, Effect::Delete { range: range_b }) => {
            if range_b.start().get() == range_a.start().get() {
                let combined_len = range_a.len() + range_b.len();
                let combined_end = Offset::new(range_a.start().get() + combined_len);
                Some(Effect::Delete {
                    range: Range::new(range_a.start(), combined_end),
                })
            } else {
                None
            }
        }

        // ── SetCursor last-wins ──────────────────────────────────────────
        (Effect::SetCursor { .. }, Effect::SetCursor { offset }) => {
            Some(Effect::SetCursor { offset: *offset })
        }

        // ── SetMode last-wins ────────────────────────────────────────────
        (Effect::SetMode { .. }, Effect::SetMode { mode, .. }) => Some(Effect::set_mode(*mode)),

        // ── Insert + Delete annihilation ──────────────────────────────────
        //
        // Insert(at, text) followed by Delete(range) where range starts at
        // the same offset and spans the same byte length: the delete undoes
        // the insert exactly, producing Noop.
        (Effect::Insert { offset: at, text }, Effect::Delete { range })
            if range.start() == *at && range.len() == text.len() =>
        {
            Some(Effect::Noop)
        }

        // ── Everything else: not composable ──────────────────────────────
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BATCH COMPOSITION
// ═══════════════════════════════════════════════════════════════════════════════

/// Run a single forward pass over the effects list, fusing adjacent composable pairs.
///
/// Operates in-place. After one pass, no two adjacent effects in the result
/// can be further composed (because each successful fusion feeds the fused
/// effect forward as the new "current" candidate). This is therefore a
/// fixed-point with respect to contiguous runs of composable effects.
///
/// Non-composable effects are left in their original relative order.
///
/// # Complexity
///
/// Time: O(n) where n = number of effects (single forward pass)
/// Space: O(1) (in-place with write cursor)
#[allow(
    clippy::indexing_slicing,
    reason = "write < read <= len is maintained by loop invariant"
)]
pub fn compose_effects(effects: &mut SmallVec<[Effect; 4]>) {
    if effects.len() < 2 {
        return;
    }

    // We build the compacted result in-place using a write cursor.
    // `write` tracks where the next "kept" element goes; `read` scans forward.
    //
    // Invariant: `write < read` at all times. `write` starts at 0 and only
    // advances by 1 when fusion fails (at which point `read` has already
    // advanced past it). `read` ranges from 1..len. Therefore:
    //   - `write` is always in [0, len-1]
    //   - `read` is always in [1, len-1]
    //   - `write < read` (write increments at most once per iteration)
    //
    // This guarantees all indexing operations are in bounds.

    let mut write = 0;
    for read in 1..effects.len() {
        // Split to get disjoint borrows: left = [0..read], right = [read..].
        let (left, right) = effects.split_at(read);
        let candidate = &left[write];
        let next = &right[0];

        if let Some(fused) = try_compose(candidate, next) {
            // Fusion succeeded — replace the accumulator in-place.
            effects[write] = fused;
        } else {
            // No fusion — advance write cursor and move the element.
            write += 1;
            if write != read {
                effects.swap(write, read);
            }
        }
    }

    // Truncate to keep only [0..=write].
    effects.truncate(write + 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// NON-LOCAL SIMPLIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Perform non-local simplification of an effect stream, removing redundant effects.
///
/// Unlike [`compose_effects`] which only fuses *adjacent* same-type pairs,
/// `simplify_effects` performs broader, non-local optimizations:
///
/// 1. **Last `SetCursor` wins** — if multiple `SetCursor` effects appear anywhere
///    in the stream, only the last one is kept. Earlier `SetCursor` effects are
///    removed because each effect carries explicit offsets; text mutations do not
///    depend on cursor position.
///
/// 2. **Last `SetMode` wins** — same logic. Modes are "set and forget" from
///    the host's perspective, so only the final mode matters.
///
/// 3. **Empty undo groups removed** — a `BeginUndoGroup` followed by an
///    `EndUndoGroup` with no [`Effect::is_text_mutation`] effects between them
///    is removed entirely (both the Begin and End). Non-mutation effects
///    (e.g., `SetCursor`, `SetMode`) between them do NOT prevent removal.
///
/// The function modifies the vector in-place.
///
/// # Complexity
///
/// Time: O(n * k) where n = number of effects, k = number of empty undo group
///       removal iterations (typically 1). Passes 1-2 are each O(n). Pass 3 may
///       iterate multiple times for nested empty groups but is O(n) per iteration.
/// Space: O(n) for the index-removal bitmap in pass 3
pub fn simplify_effects(effects: &mut SmallVec<[Effect; 4]>) {
    // ── Pass 1: Last SetCursor wins ────────────────────────────────────
    retain_only_last(effects, |e| matches!(e, Effect::SetCursor { .. }));

    // ── Pass 2: Last SetMode wins ──────────────────────────────────────
    retain_only_last(effects, |e| matches!(e, Effect::SetMode { .. }));

    // ── Pass 3: Remove empty undo groups ───────────────────────────────
    remove_empty_undo_groups(effects);
}

/// Remove all effects matching `predicate` except the last one.
///
/// If there is zero or one matching effect, the vector is unchanged.
fn retain_only_last(effects: &mut SmallVec<[Effect; 4]>, predicate: fn(&Effect) -> bool) {
    let Some(last_idx) = effects.iter().rposition(predicate) else {
        return;
    };
    let has_earlier = effects.iter().take(last_idx).any(predicate);
    if !has_earlier {
        return;
    }
    let mut i = 0;
    effects.retain(|e| {
        let idx = i;
        i += 1;
        !predicate(e) || idx == last_idx
    });
}

/// Inner helper for the empty-undo-group removal pass.
///
/// Iterates until a fixed point: each pass finds `BeginUndoGroup` / `EndUndoGroup`
/// pairs with no text mutations between them and removes both bookends.
/// Non-mutation effects between the bookends are retained.
#[allow(
    clippy::indexing_slicing,
    reason = "i < len, j iterates over a sub-slice obtained by .get()"
)]
fn remove_empty_undo_groups(effects: &mut SmallVec<[Effect; 4]>) {
    loop {
        let mut indices_to_remove: Vec<usize> = Vec::new();

        let mut i = 0;
        while i < effects.len() {
            let is_begin = effects
                .get(i)
                .is_some_and(|e| matches!(e, Effect::BeginUndoGroup { .. }));

            if is_begin {
                // Look forward for the matching EndUndoGroup.
                // Track nesting depth so we handle nested groups correctly.
                let mut depth: u32 = 1;
                let mut has_mutation = false;
                let mut end_idx = None;

                // Iterate over the tail slice starting after position `i`.
                if let Some(tail) = effects.get((i + 1)..) {
                    for (offset, effect) in tail.iter().enumerate() {
                        if matches!(effect, Effect::BeginUndoGroup { .. }) {
                            depth += 1;
                        } else if matches!(effect, Effect::EndUndoGroup { .. }) {
                            depth -= 1;
                            if depth == 0 {
                                end_idx = Some(i + 1 + offset);
                                break;
                            }
                        } else if effect.is_text_mutation() {
                            has_mutation = true;
                        }
                    }
                }

                if let Some(end) = end_idx {
                    if !has_mutation {
                        indices_to_remove.push(i);
                        indices_to_remove.push(end);
                        // Skip past this group — we've handled it.
                        i = end + 1;
                        continue;
                    }
                }
            }
            i += 1;
        }

        if indices_to_remove.is_empty() {
            break;
        }

        // Remove all marked indices in a single O(n) retain pass.
        indices_to_remove.sort_unstable();
        indices_to_remove.dedup();
        let mut remove_iter = indices_to_remove.iter().peekable();
        let mut current_idx = 0usize;
        effects.retain(|_| {
            let keep = remove_iter.peek() != Some(&&current_idx);
            if !keep {
                remove_iter.next();
            }
            current_idx += 1;
            keep
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Mode, Offset, Range};
    use compact_str::CompactString;
    use smallvec::smallvec;

    // ── try_compose: Adjacent inserts ────────────────────────────────────

    #[test]
    fn compose_adjacent_inserts() {
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("he"),
        };
        let b = Effect::Insert {
            offset: Offset::new(7),
            text: CompactString::new("llo"),
        };
        let fused = try_compose(&a, &b).expect("adjacent inserts should compose");
        assert_eq!(
            fused,
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::new("hello"),
            }
        );
    }

    #[test]
    fn compose_adjacent_inserts_at_zero() {
        let a = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("abc"),
        };
        let b = Effect::Insert {
            offset: Offset::new(3),
            text: CompactString::new("def"),
        };
        let fused = try_compose(&a, &b).expect("should compose");
        assert_eq!(
            fused,
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("abcdef"),
            }
        );
    }

    #[test]
    fn non_adjacent_inserts_do_not_compose() {
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("he"),
        };
        let b = Effect::Insert {
            offset: Offset::new(10), // gap between 7 and 10
            text: CompactString::new("llo"),
        };
        assert!(try_compose(&a, &b).is_none());
    }

    #[test]
    fn backward_inserts_do_not_compose() {
        // b starts before the end of a's text
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        let b = Effect::Insert {
            offset: Offset::new(3),
            text: CompactString::new("world"),
        };
        assert!(try_compose(&a, &b).is_none());
    }

    #[test]
    fn compose_insert_with_empty_first_text() {
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new(""),
        };
        let b = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        let fused = try_compose(&a, &b).expect("empty + text should compose (offset matches)");
        assert_eq!(
            fused,
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::new("hello"),
            }
        );
    }

    #[test]
    fn compose_insert_with_empty_second_text() {
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        let b = Effect::Insert {
            offset: Offset::new(10),
            text: CompactString::new(""),
        };
        let fused = try_compose(&a, &b).expect("text + empty should compose");
        assert_eq!(
            fused,
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::new("hello"),
            }
        );
    }

    #[test]
    fn compose_two_empty_inserts_same_offset() {
        let a = Effect::Insert {
            offset: Offset::new(3),
            text: CompactString::new(""),
        };
        let b = Effect::Insert {
            offset: Offset::new(3),
            text: CompactString::new(""),
        };
        let fused = try_compose(&a, &b).expect("two empty inserts at same offset compose");
        assert_eq!(
            fused,
            Effect::Insert {
                offset: Offset::new(3),
                text: CompactString::new(""),
            }
        );
    }

    #[test]
    fn compose_insert_single_chars() {
        // Simulates typing "abc" one character at a time.
        let a = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("a"),
        };
        let b = Effect::Insert {
            offset: Offset::new(1),
            text: CompactString::new("b"),
        };
        let ab = try_compose(&a, &b).unwrap();
        assert_eq!(
            ab,
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("ab"),
            }
        );
        let c = Effect::Insert {
            offset: Offset::new(2),
            text: CompactString::new("c"),
        };
        let abc = try_compose(&ab, &c).unwrap();
        assert_eq!(
            abc,
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("abc"),
            }
        );
    }

    // ── try_compose: Adjacent deletes ────────────────────────────────────

    #[test]
    fn compose_adjacent_deletes_same_start() {
        // Delete(5..8) then Delete(5..7): both start at 5, which is the
        // "keep deleting forward" pattern.
        let a = Effect::Delete {
            range: Range::from_raw(5, 8),
        };
        let b = Effect::Delete {
            range: Range::from_raw(5, 7),
        };
        let fused = try_compose(&a, &b).expect("same-start deletes should compose");
        // Combined: Delete(5..10) because 3 + 2 = 5 bytes total.
        assert_eq!(
            fused,
            Effect::Delete {
                range: Range::from_raw(5, 10),
            }
        );
    }

    #[test]
    fn compose_deletes_at_offset_zero() {
        let a = Effect::Delete {
            range: Range::from_raw(0, 3),
        };
        let b = Effect::Delete {
            range: Range::from_raw(0, 4),
        };
        let fused = try_compose(&a, &b).unwrap();
        assert_eq!(
            fused,
            Effect::Delete {
                range: Range::from_raw(0, 7),
            }
        );
    }

    #[test]
    fn non_adjacent_deletes_do_not_compose() {
        // Second delete starts at a different position.
        let a = Effect::Delete {
            range: Range::from_raw(5, 8),
        };
        let b = Effect::Delete {
            range: Range::from_raw(10, 15),
        };
        assert!(try_compose(&a, &b).is_none());
    }

    #[test]
    fn compose_delete_with_zero_length_first() {
        let a = Effect::Delete {
            range: Range::from_raw(5, 5), // empty
        };
        let b = Effect::Delete {
            range: Range::from_raw(5, 8),
        };
        let fused = try_compose(&a, &b).expect("empty + non-empty delete should compose");
        assert_eq!(
            fused,
            Effect::Delete {
                range: Range::from_raw(5, 8),
            }
        );
    }

    #[test]
    fn compose_delete_with_zero_length_second() {
        let a = Effect::Delete {
            range: Range::from_raw(5, 8),
        };
        let b = Effect::Delete {
            range: Range::from_raw(5, 5), // empty
        };
        let fused = try_compose(&a, &b).expect("non-empty + empty delete should compose");
        // Combined length is 3 + 0 = 3.
        assert_eq!(
            fused,
            Effect::Delete {
                range: Range::from_raw(5, 8),
            }
        );
    }

    #[test]
    fn compose_two_empty_deletes_same_start() {
        let a = Effect::Delete {
            range: Range::from_raw(5, 5),
        };
        let b = Effect::Delete {
            range: Range::from_raw(5, 5),
        };
        let fused = try_compose(&a, &b).expect("two empty deletes same start should compose");
        assert_eq!(
            fused,
            Effect::Delete {
                range: Range::from_raw(5, 5),
            }
        );
    }

    #[test]
    fn deletes_different_start_do_not_compose() {
        // Even if they're "close" — different start means different pattern.
        let a = Effect::Delete {
            range: Range::from_raw(5, 8),
        };
        let b = Effect::Delete {
            range: Range::from_raw(6, 9),
        };
        assert!(try_compose(&a, &b).is_none());
    }

    // ── try_compose: SetCursor last-wins ─────────────────────────────────

    #[test]
    fn compose_set_cursor_last_wins() {
        let a = Effect::SetCursor {
            offset: Offset::new(5),
        };
        let b = Effect::SetCursor {
            offset: Offset::new(10),
        };
        let fused = try_compose(&a, &b).expect("SetCursor should always compose");
        assert_eq!(
            fused,
            Effect::SetCursor {
                offset: Offset::new(10),
            }
        );
    }

    #[test]
    fn compose_set_cursor_same_position() {
        let a = Effect::SetCursor {
            offset: Offset::new(5),
        };
        let b = Effect::SetCursor {
            offset: Offset::new(5),
        };
        let fused = try_compose(&a, &b).expect("same position should compose");
        assert_eq!(
            fused,
            Effect::SetCursor {
                offset: Offset::new(5),
            }
        );
    }

    #[test]
    fn compose_set_cursor_zero() {
        let a = Effect::SetCursor {
            offset: Offset::new(100),
        };
        let b = Effect::SetCursor {
            offset: Offset::new(0),
        };
        let fused = try_compose(&a, &b).unwrap();
        assert_eq!(
            fused,
            Effect::SetCursor {
                offset: Offset::new(0),
            }
        );
    }

    // ── try_compose: SetMode last-wins ───────────────────────────────────

    #[test]
    fn compose_set_mode_last_wins() {
        let a = Effect::set_mode(Mode::Normal);
        let b = Effect::set_mode(Mode::Insert);
        let fused = try_compose(&a, &b).expect("SetMode should always compose");
        assert_eq!(fused, Effect::set_mode(Mode::Insert));
    }

    #[test]
    fn compose_set_mode_same_mode() {
        let a = Effect::set_mode(Mode::Normal);
        let b = Effect::set_mode(Mode::Normal);
        let fused = try_compose(&a, &b).expect("same mode should compose");
        assert_eq!(fused, Effect::set_mode(Mode::Normal));
    }

    // ── try_compose: Non-composable pairs ────────────────────────────────

    #[test]
    fn insert_then_delete_same_range_annihilates_to_noop() {
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        let b = Effect::Delete {
            range: Range::from_raw(5, 10),
        };
        assert_eq!(
            try_compose(&a, &b),
            Some(Effect::Noop),
            "Insert followed by Delete of same offset+length should annihilate to Noop"
        );
    }

    #[test]
    fn insert_then_delete_different_offset_does_not_compose() {
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        let b = Effect::Delete {
            range: Range::from_raw(10, 15),
        };
        assert!(
            try_compose(&a, &b).is_none(),
            "Insert at 5 + Delete at 10 should not compose (different offsets)"
        );
    }

    #[test]
    fn insert_then_delete_different_length_does_not_compose() {
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        let b = Effect::Delete {
            range: Range::from_raw(5, 8),
        };
        assert!(
            try_compose(&a, &b).is_none(),
            "Insert 5 bytes + Delete 3 bytes should not compose (different lengths)"
        );
    }

    #[test]
    fn compose_effects_batch_annihilates_insert_delete() {
        let mut effects = smallvec![
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::new("ab"),
            },
            Effect::Delete {
                range: Range::from_raw(5, 7),
            },
        ];
        compose_effects(&mut effects);
        assert_eq!(
            effects.as_slice(),
            &[Effect::Noop],
            "Insert+Delete of same range should annihilate to [Noop] in batch"
        );
    }

    #[test]
    fn delete_and_insert_do_not_compose() {
        let a = Effect::Delete {
            range: Range::from_raw(5, 10),
        };
        let b = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        assert!(try_compose(&a, &b).is_none());
    }

    #[test]
    fn set_cursor_and_set_mode_do_not_compose() {
        let a = Effect::SetCursor {
            offset: Offset::new(5),
        };
        let b = Effect::set_mode(Mode::Insert);
        assert!(try_compose(&a, &b).is_none());
    }

    #[test]
    fn clear_message_does_not_compose() {
        let a = Effect::ClearMessage;
        let b = Effect::ClearMessage;
        assert!(try_compose(&a, &b).is_none());
    }

    #[test]
    fn replace_effects_do_not_compose() {
        let a = Effect::Replace {
            range: Range::from_raw(0, 3),
            text: CompactString::new("xyz"),
        };
        let b = Effect::Replace {
            range: Range::from_raw(3, 6),
            text: CompactString::new("abc"),
        };
        assert!(try_compose(&a, &b).is_none());
    }

    #[test]
    fn insert_and_set_cursor_do_not_compose() {
        let a = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("hello"),
        };
        let b = Effect::SetCursor {
            offset: Offset::new(10),
        };
        assert!(try_compose(&a, &b).is_none());
    }

    // ── compose_effects: batch composition ───────────────────────────────

    #[test]
    fn compose_effects_empty_vec() {
        let mut effects: SmallVec<[Effect; 4]> = smallvec![];
        compose_effects(&mut effects);
        assert!(effects.is_empty());
    }

    #[test]
    fn compose_effects_single_effect() {
        let mut effects = smallvec![Effect::set_cursor(Offset::new(5))];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(5)));
    }

    #[test]
    fn compose_effects_two_non_composable() {
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_mode(Mode::Insert),
        ];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn compose_effects_three_cursors_fuse_to_one() {
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(1)),
            Effect::set_cursor(Offset::new(2)),
            Effect::set_cursor(Offset::new(3)),
        ];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(3)));
    }

    #[test]
    fn compose_effects_three_modes_fuse_to_one() {
        let mut effects = smallvec![
            Effect::set_mode(Mode::Normal),
            Effect::set_mode(Mode::Insert),
            Effect::set_mode(Mode::Replace),
        ];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_mode(Mode::Replace));
    }

    #[test]
    fn compose_effects_chain_of_adjacent_inserts() {
        let mut effects = smallvec![
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("a"),
            },
            Effect::Insert {
                offset: Offset::new(1),
                text: CompactString::new("b"),
            },
            Effect::Insert {
                offset: Offset::new(2),
                text: CompactString::new("c"),
            },
            Effect::Insert {
                offset: Offset::new(3),
                text: CompactString::new("d"),
            },
        ];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0],
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("abcd"),
            }
        );
    }

    #[test]
    fn compose_effects_chain_of_adjacent_deletes() {
        // Three consecutive `dw` at position 5 — each deletes 3 bytes.
        let mut effects = smallvec![
            Effect::Delete {
                range: Range::from_raw(5, 8),
            },
            Effect::Delete {
                range: Range::from_raw(5, 8),
            },
            Effect::Delete {
                range: Range::from_raw(5, 8),
            },
        ];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0],
            Effect::Delete {
                range: Range::from_raw(5, 14),
            }
        );
    }

    #[test]
    fn compose_effects_mixed_composable_and_non_composable() {
        // Cursor, Cursor (compose), Insert, Mode, Mode (compose)
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(1)),
            Effect::set_cursor(Offset::new(2)),
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("x"),
            },
            Effect::set_mode(Mode::Normal),
            Effect::set_mode(Mode::Insert),
        ];
        compose_effects(&mut effects);
        // Expect: SetCursor(2), Insert(0, "x"), SetMode(Insert)
        assert_eq!(effects.len(), 3);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(2)));
        assert_eq!(
            effects[1],
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("x"),
            }
        );
        assert_eq!(effects[2], Effect::set_mode(Mode::Insert));
    }

    #[test]
    fn compose_effects_inserts_with_gap_do_not_fuse() {
        // Two inserts at non-adjacent positions should stay separate.
        let mut effects = smallvec![
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("abc"),
            },
            Effect::Insert {
                offset: Offset::new(10),
                text: CompactString::new("def"),
            },
        ];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn compose_effects_interleaved_inserts_and_cursors() {
        // Insert, Cursor, Insert — the cursor breaks the insert chain.
        let mut effects = smallvec![
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("a"),
            },
            Effect::set_cursor(Offset::new(1)),
            Effect::Insert {
                offset: Offset::new(1),
                text: CompactString::new("b"),
            },
        ];
        compose_effects(&mut effects);
        // No composition possible: Insert/Cursor and Cursor/Insert are different types.
        assert_eq!(effects.len(), 3);
    }

    #[test]
    fn compose_effects_preserves_non_composable_order() {
        let clear = Effect::ClearMessage;
        let show = Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text(CompactString::new("hello")),
        };
        let scroll = Effect::ScrollTo {
            offset: Offset::new(0),
        };
        let mut effects: SmallVec<[Effect; 4]> =
            smallvec![clear.clone(), show.clone(), scroll.clone()];
        compose_effects(&mut effects);
        assert_eq!(effects.as_slice(), &[clear, show, scroll]);
    }

    #[test]
    fn compose_effects_only_adjacent_pairs_fuse() {
        // Insert(0,"a"), Delete(5..8), Insert(1,"b") — the delete breaks the chain.
        let mut effects = smallvec![
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("a"),
            },
            Effect::Delete {
                range: Range::from_raw(5, 8),
            },
            Effect::Insert {
                offset: Offset::new(1),
                text: CompactString::new("b"),
            },
        ];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 3);
    }

    // ── Unicode and multi-byte inserts ───────────────────────────────────

    #[test]
    fn compose_unicode_adjacent_inserts() {
        let a = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("\u{00E9}"), // e-acute: 2 bytes
        };
        let b = Effect::Insert {
            offset: Offset::new(2), // byte offset, not char offset
            text: CompactString::new("lite"),
        };
        let fused = try_compose(&a, &b).expect("unicode inserts should compose by byte offset");
        assert_eq!(
            fused,
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("\u{00E9}lite"),
            }
        );
    }

    #[test]
    fn compose_emoji_inserts() {
        let a = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("\u{1F600}"), // 4 bytes
        };
        let b = Effect::Insert {
            offset: Offset::new(4),
            text: CompactString::new("!"),
        };
        let fused = try_compose(&a, &b).unwrap();
        assert_eq!(
            fused,
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("\u{1F600}!"),
            }
        );
    }

    // ── Regression / stress ──────────────────────────────────────────────

    #[test]
    fn compose_effects_large_chain() {
        // 100 single-char inserts at consecutive positions.
        let mut effects: SmallVec<[Effect; 4]> = (0..100)
            .map(|i| Effect::Insert {
                offset: Offset::new(i),
                text: CompactString::new("x"),
            })
            .collect();
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0],
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new(&"x".repeat(100)),
            }
        );
    }

    #[test]
    fn compose_effects_alternating_types_no_fusion() {
        // Alternating SetCursor and SetMode — no adjacent same-type pairs.
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(1)),
            Effect::set_mode(Mode::Normal),
            Effect::set_cursor(Offset::new(2)),
            Effect::set_mode(Mode::Insert),
        ];
        compose_effects(&mut effects);
        assert_eq!(effects.len(), 4);
    }

    // ═════════════════════════════════════════════════════════════════════
    // simplify_effects — non-local simplification
    // ═════════════════════════════════════════════════════════════════════

    // ── Empty input ──────────────────────────────────────────────────────

    #[test]
    fn simplify_empty_vec() {
        let mut effects: SmallVec<[Effect; 4]> = smallvec![];
        simplify_effects(&mut effects);
        assert!(effects.is_empty());
    }

    // ── Single effect passthrough ────────────────────────────────────────

    #[test]
    fn simplify_single_set_cursor_kept() {
        let mut effects = smallvec![Effect::set_cursor(Offset::new(5))];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(5)));
    }

    #[test]
    fn simplify_single_set_mode_kept() {
        let mut effects = smallvec![Effect::set_mode(Mode::Insert)];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_mode(Mode::Insert));
    }

    #[test]
    fn simplify_single_non_cursor_effect_kept() {
        let mut effects = smallvec![Effect::ClearMessage];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::ClearMessage);
    }

    // ── SetCursor: last wins (non-local) ─────────────────────────────────

    #[test]
    fn simplify_two_adjacent_cursors_last_wins() {
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(5)),
            Effect::set_cursor(Offset::new(10)),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(10)));
    }

    #[test]
    fn simplify_cursors_with_intervening_effects() {
        // SetCursor(5), Insert(...), SetCursor(10) → the first SetCursor is removed.
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(5)),
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("hello"),
            },
            Effect::set_cursor(Offset::new(10)),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 2);
        assert_eq!(
            effects[0],
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("hello"),
            }
        );
        assert_eq!(effects[1], Effect::set_cursor(Offset::new(10)));
    }

    #[test]
    fn simplify_three_cursors_non_adjacent_last_wins() {
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(1)),
            Effect::ClearMessage,
            Effect::set_cursor(Offset::new(2)),
            Effect::set_mode(Mode::Normal),
            Effect::set_cursor(Offset::new(3)),
        ];
        simplify_effects(&mut effects);
        // Only the last SetCursor (offset=3) should remain; other effects stay.
        assert_eq!(effects.len(), 3);
        assert_eq!(effects[0], Effect::ClearMessage);
        assert_eq!(effects[1], Effect::set_mode(Mode::Normal));
        assert_eq!(effects[2], Effect::set_cursor(Offset::new(3)));
    }

    #[test]
    fn simplify_cursors_with_text_mutations_between() {
        // Even with text mutations, only the last SetCursor is kept.
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(5)),
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("a"),
            },
            Effect::Delete {
                range: Range::from_raw(3, 5),
            },
            Effect::set_cursor(Offset::new(10)),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 3);
        assert!(matches!(effects[0], Effect::Insert { .. }));
        assert!(matches!(effects[1], Effect::Delete { .. }));
        assert_eq!(effects[2], Effect::set_cursor(Offset::new(10)));
    }

    // ── SetMode: last wins (non-local) ───────────────────────────────────

    #[test]
    fn simplify_two_adjacent_modes_last_wins() {
        let mut effects = smallvec![
            Effect::set_mode(Mode::Insert),
            Effect::set_mode(Mode::Normal),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_mode(Mode::Normal));
    }

    #[test]
    fn simplify_modes_with_intervening_effects() {
        let mut effects = smallvec![
            Effect::set_mode(Mode::Insert),
            Effect::set_cursor(Offset::new(5)),
            Effect::set_mode(Mode::Normal),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(5)));
        assert_eq!(effects[1], Effect::set_mode(Mode::Normal));
    }

    #[test]
    fn simplify_three_modes_non_adjacent_last_wins() {
        let mut effects = smallvec![
            Effect::set_mode(Mode::Normal),
            Effect::ClearMessage,
            Effect::set_mode(Mode::Insert),
            Effect::set_cursor(Offset::new(5)),
            Effect::set_mode(Mode::Replace),
        ];
        simplify_effects(&mut effects);
        // Only the last SetMode (Replace) survives. SetCursor is only one, kept.
        assert_eq!(effects.len(), 3);
        assert_eq!(effects[0], Effect::ClearMessage);
        assert_eq!(effects[1], Effect::set_cursor(Offset::new(5)));
        assert_eq!(effects[2], Effect::set_mode(Mode::Replace));
    }

    // ── Empty undo groups ────────────────────────────────────────────────

    #[test]
    fn simplify_empty_undo_group_adjacent() {
        // BeginUndoGroup immediately followed by EndUndoGroup → both removed.
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        assert!(effects.is_empty());
    }

    #[test]
    fn simplify_empty_undo_group_with_non_mutations_between() {
        // BeginUndoGroup, SetCursor, SetMode, EndUndoGroup → all 4 removed
        // (SetCursor and SetMode are NOT text mutations).
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::set_cursor(Offset::new(5)),
            Effect::set_mode(Mode::Normal),
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        // The undo group is empty (no mutations). The Begin and End are removed.
        // But the inner SetCursor and SetMode remain.
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(5)));
        assert_eq!(effects[1], Effect::set_mode(Mode::Normal));
    }

    #[test]
    fn simplify_undo_group_with_mutation_not_removed() {
        // BeginUndoGroup, Insert, EndUndoGroup → kept (has mutation).
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("hello"),
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 3);
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(effects[1], Effect::Insert { .. }));
        assert!(matches!(effects[2], Effect::EndUndoGroup { .. }));
    }

    #[test]
    fn simplify_undo_group_with_delete_not_removed() {
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::EntryPosition,
            },
            Effect::Delete {
                range: Range::from_raw(0, 5),
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 3);
    }

    #[test]
    fn simplify_undo_group_with_replace_not_removed() {
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Replace {
                range: Range::from_raw(0, 3),
                text: CompactString::new("xyz"),
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 3);
    }

    #[test]
    fn simplify_multiple_empty_undo_groups() {
        // Two consecutive empty undo groups → both removed.
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::EndUndoGroup { node_id: None },
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        assert!(effects.is_empty());
    }

    #[test]
    fn simplify_mixed_empty_and_non_empty_undo_groups() {
        // First group is empty → removed. Second has a mutation → kept.
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::EndUndoGroup { node_id: None },
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("x"),
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 3);
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(effects[1], Effect::Insert { .. }));
        assert!(matches!(effects[2], Effect::EndUndoGroup { .. }));
    }

    #[test]
    fn simplify_undo_group_entry_position_strategy_preserved() {
        // The cursor_strategy on the BeginUndoGroup shouldn't affect
        // whether we remove it — we still remove if empty.
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::EntryPosition,
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        assert!(effects.is_empty());
    }

    // ── Mixed simplifications ────────────────────────────────────────────

    #[test]
    fn simplify_cursor_mode_and_empty_undo_group_all_at_once() {
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(1)),
            Effect::set_mode(Mode::Insert),
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::EndUndoGroup { node_id: None },
            Effect::set_cursor(Offset::new(10)),
            Effect::set_mode(Mode::Normal),
        ];
        simplify_effects(&mut effects);
        // Expected: earlier SetCursor(1) removed, earlier SetMode(Insert) removed,
        // empty undo group removed. Left: SetCursor(10), SetMode(Normal).
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(10)));
        assert_eq!(effects[1], Effect::set_mode(Mode::Normal));
    }

    #[test]
    fn simplify_complex_realistic_stream() {
        // Simulates: mode change, cursor move, empty undo group, insert with undo,
        // another cursor move, mode change back.
        let mut effects = smallvec![
            Effect::set_mode(Mode::Insert),     // 0 — will be removed (not last)
            Effect::set_cursor(Offset::new(5)), // 1 — will be removed (not last)
            Effect::BeginUndoGroup {
                // 2 — empty group, removed
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::set_cursor(Offset::new(6)), // 3 — will be removed (not last)
            Effect::EndUndoGroup { node_id: None }, // 4 — empty group end, removed
            Effect::BeginUndoGroup {
                // 5 — has mutation, kept
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("hello"),
            },
            Effect::EndUndoGroup { node_id: None }, // 7 — kept
            Effect::set_cursor(Offset::new(20)),    // 8 — LAST cursor, kept
            Effect::set_mode(Mode::Normal),         // 9 — LAST mode, kept
        ];
        simplify_effects(&mut effects);
        // Expected remaining: BeginUndoGroup, Insert, EndUndoGroup, SetCursor(20), SetMode(Normal)
        assert_eq!(effects.len(), 5);
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(effects[1], Effect::Insert { .. }));
        assert!(matches!(effects[2], Effect::EndUndoGroup { .. }));
        assert_eq!(effects[3], Effect::set_cursor(Offset::new(20)));
        assert_eq!(effects[4], Effect::set_mode(Mode::Normal));
    }

    #[test]
    fn simplify_no_cursors_or_modes_leaves_effects_intact() {
        let mut effects = smallvec![
            Effect::ClearMessage,
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("hi"),
            },
            Effect::ScrollTo {
                offset: Offset::new(10),
            },
        ];
        let original = effects.clone();
        simplify_effects(&mut effects);
        assert_eq!(effects, original);
    }

    #[test]
    fn simplify_only_set_cursors_reduces_to_one() {
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(1)),
            Effect::set_cursor(Offset::new(2)),
            Effect::set_cursor(Offset::new(3)),
            Effect::set_cursor(Offset::new(4)),
            Effect::set_cursor(Offset::new(5)),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_cursor(Offset::new(5)));
    }

    #[test]
    fn simplify_only_set_modes_reduces_to_one() {
        let mut effects = smallvec![
            Effect::set_mode(Mode::Normal),
            Effect::set_mode(Mode::Insert),
            Effect::set_mode(Mode::Replace),
            Effect::set_mode(Mode::Normal),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0], Effect::set_mode(Mode::Normal));
    }

    #[test]
    fn simplify_preserves_relative_order_of_non_removed_effects() {
        // Ensure that after removing cursors/modes, the remaining effects
        // maintain their original relative ordering.
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(1)),
            Effect::ClearMessage,
            Effect::set_cursor(Offset::new(2)),
            Effect::show_message("hello"),
            Effect::set_cursor(Offset::new(3)),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 3);
        assert_eq!(effects[0], Effect::ClearMessage);
        assert_eq!(effects[1], Effect::show_message("hello"));
        assert_eq!(effects[2], Effect::set_cursor(Offset::new(3)));
    }

    #[test]
    fn simplify_undo_group_unpaired_begin_left_alone() {
        // BeginUndoGroup with no matching EndUndoGroup — don't remove it.
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::set_cursor(Offset::new(5)),
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert_eq!(effects[1], Effect::set_cursor(Offset::new(5)));
    }

    #[test]
    fn simplify_undo_group_unpaired_end_left_alone() {
        // EndUndoGroup with no preceding BeginUndoGroup — don't remove it.
        let mut effects = smallvec![
            Effect::set_cursor(Offset::new(5)),
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn simplify_nested_undo_groups_outer_empty() {
        // Outer group has no mutations (inner group is also empty).
        // Both groups should be removed.
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::EndUndoGroup { node_id: None },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        // The inner Begin/End pair is removed first (no mutations between them),
        // then the outer Begin/End pair becomes adjacent and empty too.
        assert!(effects.is_empty());
    }

    #[test]
    fn simplify_nested_undo_groups_inner_has_mutation() {
        // Outer group has a mutation (through the inner group).
        // The inner group has a mutation → kept. The outer group has the inner
        // group which contains a mutation, so it also counts as having a mutation.
        let mut effects = smallvec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("x"),
            },
            Effect::EndUndoGroup { node_id: None },
            Effect::EndUndoGroup { node_id: None },
        ];
        simplify_effects(&mut effects);
        // The inner group has a mutation, so it stays. The outer group
        // also has a mutation (the Insert between its Begin and End),
        // so it stays too.
        assert_eq!(effects.len(), 5);
    }
}
