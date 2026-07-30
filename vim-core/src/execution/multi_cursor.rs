//! Algebraic effect replication for multiple cursors.
//!
//! Given effects computed for the primary cursor, this module produces effects
//! for ALL cursors by algebraically rebasing — no re-execution of commands.
//!
//! # Algorithm
//!
//! 1. Fast path: if only one cursor, return effects unchanged.
//! 2. Collect all cursors, sorted by offset DESCENDING (bottom to top).
//! 3. For each cursor, compute the signed delta from the primary cursor offset.
//! 4. Rebase primary effects by that delta using `rebase_effects_by_delta`.
//! 5. Wrap everything in `BeginUndoGroup` / `EndUndoGroup` for atomic undo.
//!
//! # Why bottom-to-top?
//!
//! Processing cursors from the highest offset to the lowest means each cursor's
//! edits are emitted in descending position order. When the host applies these
//! effects sequentially, higher-offset edits don't invalidate positions of
//! lower-offset cursors. This eliminates the need for cumulative shift tracking.

use crate::effects::algebra::rebase_effects_into;
use crate::effects::undo_intent::UndoIntent;
use crate::effects::{Effect, Effects};
use crate::primitives::byte_delta;
use crate::primitives::Selections;

/// Returns `true` for effects that are position-dependent and must be
/// replicated (rebased) for each cursor. Returns `false` for global effects
/// that should appear only once in the output.
pub(crate) const fn is_positional_effect(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::Insert { .. }
            | Effect::Delete { .. }
            | Effect::Replace { .. }
            | Effect::SetCursor { .. }
            | Effect::SetMark { .. }
            | Effect::SetSelection { .. }
            | Effect::ScrollTo { .. }
            | Effect::PushJumpList { .. }
    )
}

/// Replicate effects computed for the primary cursor to all cursors.
///
/// The input `primary_effects` are the effects produced by executing a command
/// at the primary cursor. The `selections` contain all cursor positions.
///
/// Returns a new `Effects` containing effects for all cursors, wrapped in
/// a single undo group for atomic undo.
///
/// # Effect classification
///
/// Effects are split into positional (replicated per cursor with delta rebasing)
/// and global (emitted once). Global effects before the first positional effect
/// are emitted at the start; global effects after the last positional effect are
/// emitted at the end.
///
/// Undo wrapping is controlled by `undo_intent`: when the intent has an undo
/// group, `BeginUndoGroup`/`EndUndoGroup` from the primary effects are stripped
/// and fresh markers are emitted based on the intent (respecting open-ended groups).
///
/// # Fast path
///
/// If `selections.len() == 1`, returns `primary_effects` unchanged (no
/// allocation, no wrapping).
pub fn replicate_effects_precise(
    primary_effects: &Effects,
    selections: &Selections,
    undo_intent: &UndoIntent,
) -> Effects {
    // Fast path: single cursor — no replication needed.
    if selections.len() <= 1 {
        return primary_effects.clone();
    }

    let primary_offset = selections.primary().head().get();
    let primary_effects_slice = primary_effects.as_slice();

    // Undo wrapping is derived from the UndoIntent metadata (computed once
    // at the executor boundary). This replaces the 20-line scanning block
    // that detected open-ended groups by scanning for BeginUndoGroup/EndUndoGroup.
    let wrap_undo = undo_intent.has_undo_group();
    let open_ended = undo_intent.is_open_ended();
    let extracted_strategy = undo_intent
        .strategy()
        .unwrap_or(crate::primitives::UndoCursorStrategy::FirstEdit);

    // ── Split into positional and global effects ────────────────────────
    // Find the index of the first and last positional effect.
    let first_positional = primary_effects_slice.iter().position(is_positional_effect);
    let last_positional = primary_effects_slice.iter().rposition(is_positional_effect);

    // If there are NO positional effects, just emit global effects once
    // (no per-cursor replication needed).
    let (first_pos, last_pos) = match (first_positional, last_positional) {
        (Some(f), Some(l)) => (f, l),
        _ => {
            // All effects are global — emit once, optionally wrapped.
            let mut all_effects: Vec<Effect> =
                Vec::with_capacity(primary_effects_slice.len() + if wrap_undo { 2 } else { 0 });
            if wrap_undo {
                all_effects.push(Effect::BeginUndoGroup {
                    cursor_strategy: extracted_strategy,
                });
            }
            for e in primary_effects_slice {
                if wrap_undo
                    && matches!(
                        e,
                        Effect::BeginUndoGroup { .. } | Effect::EndUndoGroup { .. }
                    )
                {
                    continue;
                }
                all_effects.push(e.clone());
            }
            if wrap_undo && !open_ended {
                all_effects.push(Effect::EndUndoGroup { node_id: None });
            }
            return all_effects.into_iter().collect();
        }
    };

    // Global effects before the first positional effect.
    let global_before: Vec<&Effect> = primary_effects_slice[..first_pos]
        .iter()
        .filter(|e| !is_positional_effect(e))
        .collect();
    // Global effects after the last positional effect.
    let global_after: Vec<&Effect> = primary_effects_slice[last_pos + 1..]
        .iter()
        .filter(|e| !is_positional_effect(e))
        .collect();
    // Global effects interleaved between positional effects (emit once, after
    // per-cursor positional effects but before global_after).
    let global_middle: Vec<&Effect> = primary_effects_slice[first_pos..=last_pos]
        .iter()
        .filter(|e| !is_positional_effect(e))
        .collect();
    // The positional-only slice that gets replicated per cursor.
    let positional: Vec<&Effect> = primary_effects_slice
        .iter()
        .filter(|e| is_positional_effect(e))
        .collect();

    // Collect cursor deltas sorted by offset DESCENDING.
    // Each delta is `cursor_offset - primary_offset`.
    let mut cursor_deltas: Vec<(usize, i64)> = selections
        .iter()
        .map(|r| {
            let offset = r.head().get();
            let delta = byte_delta::delta_i64(offset, primary_offset);
            (offset, delta)
        })
        .collect();
    cursor_deltas.sort_by_key(|&(offset, _)| std::cmp::Reverse(offset)); // descending by offset

    // Build the replicated effects list.
    let positional_count = positional.len();
    let global_count = global_before.len() + global_middle.len() + global_after.len();
    let undo_overhead = if wrap_undo { 2 } else { 0 };
    let estimated_capacity = positional_count * cursor_deltas.len() + global_count + undo_overhead;
    let mut all_effects: Vec<Effect> = Vec::with_capacity(estimated_capacity);

    // Predicate: when wrap_undo is true, strip undo markers from global
    // effects (the wrapper provides its own BeginUndoGroup/EndUndoGroup).
    let should_strip = |e: &Effect| -> bool {
        wrap_undo
            && matches!(
                e,
                Effect::BeginUndoGroup { .. } | Effect::EndUndoGroup { .. }
            )
    };

    // Begin undo group for atomic undo of multi-cursor edits (normal mode).
    // Insert mode already has an open undo group, so skip wrapping.
    // Use the strategy extracted from the primary effects' BeginUndoGroup.
    if wrap_undo {
        all_effects.push(Effect::BeginUndoGroup {
            cursor_strategy: extracted_strategy,
        });
    }

    // Emit global effects that precede any positional effects.
    for e in &global_before {
        if !should_strip(e) {
            all_effects.push((*e).clone());
        }
    }

    // Replicate positional effects per cursor (descending offset order).
    // Use rebase_effects_into to push directly into all_effects, avoiding
    // N-1 temporary Vec allocations.
    let positional_slice: Vec<Effect> = positional.iter().map(|e| (*e).clone()).collect();
    for (_cursor_offset, delta) in &cursor_deltas {
        if *delta == 0 {
            // Primary cursor — use positional effects as-is.
            all_effects.extend(positional_slice.iter().cloned());
        } else {
            // Rebase the positional effects by the delta directly into output.
            rebase_effects_into(&positional_slice, *delta, &mut all_effects);
        }
    }

    // Emit global effects that were interleaved between positional effects.
    for e in &global_middle {
        if !should_strip(e) {
            all_effects.push((*e).clone());
        }
    }

    // Emit global effects that follow all positional effects.
    for e in &global_after {
        if !should_strip(e) {
            all_effects.push((*e).clone());
        }
    }

    // End undo group (skip for open-ended groups like InsertEntry).
    if wrap_undo && !open_ended {
        all_effects.push(Effect::EndUndoGroup { node_id: None });
    }

    all_effects.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{Offset, Range, SelectionRange, Selections, UndoCursorStrategy};
    use compact_str::CompactString;

    const CLOSED: UndoIntent = UndoIntent::Closed {
        strategy: UndoCursorStrategy::FirstEdit,
    };

    #[test]
    fn single_cursor_returns_unchanged() {
        let sels = Selections::cursor(Offset::new(5));
        let effects = Effects::new()
            .insert(Offset::new(5), "x")
            .set_cursor(Offset::new(6));

        let result = replicate_effects_precise(&effects, &sels, &CLOSED);
        assert_eq!(result.as_slice(), effects.as_slice());
    }

    #[test]
    fn two_cursors_insert() {
        // Two cursors at offset 5 and 15. Primary is at 5 (index 0).
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(15)),
            ],
            0,
        );

        // Primary effects: insert "x" at offset 5, cursor to 6.
        let effects = Effects::new()
            .insert(Offset::new(5), "x")
            .set_cursor(Offset::new(6));

        let result = replicate_effects_precise(&effects, &sels, &CLOSED);
        let slice = result.as_slice();

        // BeginUndoGroup + [cursor@15: insert+cursor] + [cursor@5: insert+cursor] + EndUndoGroup
        assert_eq!(slice.len(), 6);

        // First: BeginUndoGroup
        assert!(matches!(slice[0], Effect::BeginUndoGroup { .. }));

        // Cursor at offset 15 (delta=+10 from primary at 5), processed first (descending):
        assert_eq!(
            slice[1],
            Effect::Insert {
                offset: Offset::new(15),
                text: CompactString::new("x"),
            }
        );
        assert_eq!(
            slice[2],
            Effect::SetCursor {
                offset: Offset::new(16)
            }
        );

        // Cursor at offset 5 (delta=0, primary):
        assert_eq!(
            slice[3],
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::new("x"),
            }
        );
        assert_eq!(
            slice[4],
            Effect::SetCursor {
                offset: Offset::new(6)
            }
        );

        // Last: EndUndoGroup
        assert!(matches!(slice[5], Effect::EndUndoGroup { .. }));
    }

    #[test]
    fn three_cursors_delete() {
        // Three cursors at 5, 15, 25. Primary at 15 (index 1).
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(15)),
                SelectionRange::insert_cursor(Offset::new(25)),
            ],
            1,
        );

        // Primary effects: delete range [15..18] (3 bytes), cursor to 15.
        let effects = Effects::single(Effect::Delete {
            range: Range::new(Offset::new(15), Offset::new(18)),
        })
        .set_cursor(Offset::new(15));

        let result = replicate_effects_precise(&effects, &sels, &CLOSED);
        let slice = result.as_slice();

        // BeginUndoGroup + 3*(delete + cursor) + EndUndoGroup = 8
        assert_eq!(slice.len(), 8);

        // Cursor at 25 (delta=+10), processed first (highest offset):
        assert_eq!(
            slice[1],
            Effect::Delete {
                range: Range::new(Offset::new(25), Offset::new(28)),
            }
        );
        assert_eq!(
            slice[2],
            Effect::SetCursor {
                offset: Offset::new(25)
            }
        );

        // Cursor at 15 (delta=0, primary), processed second:
        assert_eq!(
            slice[3],
            Effect::Delete {
                range: Range::new(Offset::new(15), Offset::new(18)),
            }
        );
        assert_eq!(
            slice[4],
            Effect::SetCursor {
                offset: Offset::new(15)
            }
        );

        // Cursor at 5 (delta=-10), processed last (lowest offset):
        assert_eq!(
            slice[5],
            Effect::Delete {
                range: Range::new(Offset::new(5), Offset::new(8)),
            }
        );
        assert_eq!(
            slice[6],
            Effect::SetCursor {
                offset: Offset::new(5)
            }
        );
    }

    #[test]
    fn replicated_effects_have_undo_group() {
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
            ],
            0,
        );

        let effects = Effects::new()
            .insert(Offset::new(0), "a")
            .set_cursor(Offset::new(1));

        let result = replicate_effects_precise(&effects, &sels, &CLOSED);
        let slice = result.as_slice();

        // First and last effects should be undo group markers.
        assert!(matches!(slice[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(
            slice[slice.len() - 1],
            Effect::EndUndoGroup { .. }
        ));
    }

    /// Verify that replicating insert-mode character effects (which contain
    /// NO undo markers) produces exactly one balanced BeginUndoGroup/EndUndoGroup
    /// pair, and that the inner effects contain no nested undo markers.
    ///
    /// This validates that calling `replicate_effects_precise` on insert-mode
    /// typing output will not create nested undo groups inside the already-open
    /// insert-mode undo group.
    #[test]
    fn insert_char_replication_undo_balance() {
        // Simulate 2 cursors at offsets 3 and 13 (e.g., "foo|bar...\nfoo|bar...")
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(3)),
                SelectionRange::insert_cursor(Offset::new(13)),
            ],
            0,
        );

        // Insert-mode character typing produces ONLY Insert + SetCursor.
        // (See insert_text_and_advance in commands/insert/effects.rs)
        // No BeginUndoGroup/EndUndoGroup — the undo group is managed by
        // the insert-mode entry/exit lifecycle, not per-keystroke.
        let primary_effects = Effects::new()
            .insert(Offset::new(3), "x")
            .set_cursor(Offset::new(4));

        // Verify the input has zero undo markers.
        let input_begin_count = primary_effects
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::BeginUndoGroup { .. }))
            .count();
        let input_end_count = primary_effects
            .as_slice()
            .iter()
            .filter(|e| matches!(e, Effect::EndUndoGroup { .. }))
            .count();
        assert_eq!(
            input_begin_count, 0,
            "insert char effects must not contain BeginUndoGroup"
        );
        assert_eq!(
            input_end_count, 0,
            "insert char effects must not contain EndUndoGroup"
        );

        let result = replicate_effects_precise(&primary_effects, &sels, &CLOSED);
        let slice = result.as_slice();

        // Count undo markers in the replicated output.
        let begin_count = slice
            .iter()
            .filter(|e| matches!(e, Effect::BeginUndoGroup { .. }))
            .count();
        let end_count = slice
            .iter()
            .filter(|e| matches!(e, Effect::EndUndoGroup { .. }))
            .count();

        // Exactly one BeginUndoGroup and one EndUndoGroup.
        assert_eq!(begin_count, 1, "expected exactly 1 BeginUndoGroup");
        assert_eq!(end_count, 1, "expected exactly 1 EndUndoGroup");

        // They must be balanced: Begin is first, End is last.
        assert!(matches!(slice[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(
            slice[slice.len() - 1],
            Effect::EndUndoGroup { .. }
        ));

        // The inner effects (between Begin and End) must NOT contain any undo markers.
        for effect in &slice[1..slice.len() - 1] {
            assert!(
                !matches!(
                    effect,
                    Effect::BeginUndoGroup { .. } | Effect::EndUndoGroup { .. }
                ),
                "inner effects must not contain nested undo markers, found: {effect:?}"
            );
        }

        // Total: BeginUndoGroup + 2*(Insert+SetCursor) + EndUndoGroup = 6
        assert_eq!(slice.len(), 6, "expected 6 effects total");
    }
}
