//! Per-cursor re-execution for content-dependent multi-cursor commands.
//!
//! When a command's effects depend on the buffer content at the cursor position
//! (content-dependent), the algebraic rebase approach produces incorrect results
//! because it copies the primary cursor's replacement text verbatim to all
//! secondary cursors. This module implements the alternative: executing the
//! command independently at each cursor against the original (T0) document text.
//!
//! # Architecture: hybrid approach
//!
//! The dispatch in `execute_effect_plan` uses three paths:
//!
//! 1. **Per-cursor re-execution** (this module): for content-dependent commands.
//! 2. **Algebraic rebase** (`multi_cursor.rs`): for position-independent commands.
//! 3. **Single cursor / global-only**: no replication needed.
//!
//! # Correctness: snapshot semantics
//!
//! All cursors execute against the original (unmodified) document text (T0).
//! Cursors are processed in descending offset order so that each cursor's
//! edits affect only higher offsets, which were already processed. Non-overlapping
//! selections (enforced by `Selections::normalize()`) guarantee correctness.

use super::VimEngine;
use crate::document::Document;
use crate::effects::undo_intent::UndoIntent;
use crate::effects::{Effect, Effects};
use crate::execution::{ExecutionContext, InputContext, Validated};
use crate::grammar::Command;
use crate::primitives::byte_delta;
use crate::primitives::{MotionType, Offset, RegisterName, SelectionRange, Selections};
use compact_str::CompactString;
use smallvec::SmallVec;

/// Result of per-cursor re-execution.
pub(super) struct PerCursorResult {
    /// Merged effects from all cursors. Undo markers are stripped;
    /// the caller uses `undo_intent` to materialise them at the
    /// single wrapping point.
    pub effects: Effects,
    /// Per-cursor `(selection_index, raw_set_cursor_offset, net_byte_delta)` for selection update.
    pub cursor_deltas: Vec<(usize, Offset, i64)>,
    /// Consolidated multi-entry register to apply after `process_effects`.
    pub register_override: Option<PerCursorRegisterOverride>,
    /// Undo lifecycle intent from the primary cursor's command.
    pub undo_intent: UndoIntent,
}

/// Consolidated register data from per-cursor re-execution.
pub(super) struct PerCursorRegisterOverride {
    /// Per-register consolidated content: `(register_name, multi_entry_content)`.
    pub pairs: Vec<(RegisterName, crate::primitives::RegisterContent)>,
}

impl VimEngine {
    /// Per-cursor re-execution for content-dependent commands.
    ///
    /// Instead of executing once at the primary cursor and algebraically rebasing,
    /// this function executes the command independently at each cursor against the
    /// original (T0) document text.
    ///
    /// # Algorithm
    ///
    /// 1. Sort cursors descending by offset (same order as algebraic rebase).
    /// 2. For each cursor: build an `ExecutionContext` with that cursor's offset
    ///    and selection, then call `executor::execute(command.clone(), &ctx_i)`.
    /// 3. Separate positional from global effects; strip inner undo markers.
    /// 4. Global effects (except SetRegister) captured from primary cursor only
    ///    (lowest offset, last in descending walk).
    /// 5. SetRegister effects collected from ALL cursors for register consolidation.
    /// 6. Track each cursor's SetCursor offset and net byte delta for selection
    ///    update after processing.
    /// 7. Return `UndoIntent` from the primary cursor (wrapping happens at the
    ///    single convergence point in mode_dispatch).
    /// 8. Emit SetRegister effects for all register groups so process_effects
    ///    handles state sync (clipboard routing, numbered cascade, etc.).
    /// 9. Build consolidated multi-entry RegisterContent for post-processing
    ///    override.
    pub(super) fn execute_per_cursor<D: Document>(
        &self,
        command: &Command,
        ctx: &InputContext<'_, D, Validated>,
        selections: &Selections,
    ) -> PerCursorResult {
        let is_positional = super::super::multi_cursor::is_positional_effect;

        let mut all_effects: Vec<Effect> = Vec::new();
        let mut cursor_deltas: Vec<(usize, Offset, i64)> = Vec::new();
        // Register entries collected from ALL cursors in descending offset order.
        // Will be reversed to ascending (selection order) for consolidation.
        let mut register_entries: Vec<(RegisterName, CompactString, MotionType)> = Vec::new();

        // Collect cursors sorted descending by head offset.
        let mut cursors_desc: Vec<(usize, &SelectionRange)> =
            selections.iter().enumerate().collect();
        cursors_desc.sort_by_key(|&(_, sel)| std::cmp::Reverse(sel.head().get()));

        let last_cursor_idx = cursors_desc.len().saturating_sub(1);
        let original_primary_idx = selections.primary_index();
        let mut captured_undo_intent: Option<UndoIntent> = None;
        let mut first_error_emitted = false;

        // Only pass selection in visual/select mode — in normal mode,
        // multi-cursor positions are just cursor offsets, NOT visual selections.
        // Commands like Ctrl-A check `ctx.selection` to decide between normal
        // and visual code paths; passing an insert cursor (anchor == head)
        // incorrectly triggers the visual path.
        let pass_selection = self.state.mode().is_visual() || self.state.mode().is_select();

        // Per-cursor execution loop.
        for (i, &(sel_idx, sel_range)) in cursors_desc.iter().enumerate() {
            // Build a fresh InputContext for this cursor's offset/selection.
            let mut ctx_i = InputContext::new(ctx.doc(), sel_range.head().get())
                .validate_clamped()
                .with_providers(*ctx.providers());
            if pass_selection {
                ctx_i = ctx_i.with_selection(*sel_range);
            }
            if let Some(vp) = ctx.viewport() {
                ctx_i = ctx_i.with_viewport(vp);
            }
            if let Some(bid) = ctx.buffer_id() {
                ctx_i = ctx_i.with_buffer_id(bid);
            }
            let exec_ctx_i = ExecutionContext::new(ctx_i, &self.state, &self.resolved_options);
            let output_i = super::super::executor::execute(command.clone(), &exec_ctx_i);
            if sel_idx == original_primary_idx {
                captured_undo_intent = Some(output_i.undo_intent);
            }
            let effects_i = output_i.effects;

            // Partition this cursor's effects.
            let mut set_cursor_offset: Option<Offset> = None;
            let mut net_delta: i64 = 0;

            for effect in effects_i.as_slice() {
                // Track SetCursor and net delta for selection update.
                match effect {
                    Effect::SetCursor { offset } => {
                        set_cursor_offset = Some(*offset);
                    }
                    Effect::Insert { text, .. } => {
                        net_delta += byte_delta::to_i64(text.len());
                    }
                    Effect::Delete { range } => {
                        net_delta -= byte_delta::delta_i64(range.end().get(), range.start().get());
                    }
                    Effect::Replace { range, text } => {
                        net_delta += byte_delta::to_i64(text.len())
                            - byte_delta::delta_i64(range.end().get(), range.start().get());
                    }
                    _ => {}
                }

                // Skip undo markers — intent is captured from ExecutorOutput,
                // wrapping happens at the single convergence point in mode_dispatch.

                if matches!(
                    effect,
                    Effect::BeginUndoGroup { .. } | Effect::EndUndoGroup { .. }
                ) {
                    continue;
                }

                // Intercept SetRegister from ALL cursors for consolidation.
                if let Effect::SetRegister {
                    name,
                    text,
                    motion_type,
                } = effect
                {
                    register_entries.push((*name, text.clone(), *motion_type));
                    continue;
                }

                // Positional effects collected from all cursors.
                if is_positional(effect) {
                    all_effects.push(effect.clone());
                } else if !first_error_emitted
                    && matches!(
                        effect,
                        Effect::ShowError { .. } | Effect::ShowWarning { .. } | Effect::Bell
                    )
                {
                    // Emit error/warning from the FIRST cursor that produces one,
                    // regardless of which cursor is primary.
                    all_effects.push(effect.clone());
                    first_error_emitted = true;
                } else if i == last_cursor_idx {
                    // Other global effects from the lowest-offset cursor (last
                    // in descending walk). Mode-entry effects like BeginInsert
                    // carry this cursor's offset.
                    all_effects.push(effect.clone());
                }
            }

            // Record cursor delta for selection update.
            // Fall back to original position if no SetCursor produced
            // (e.g., command produced only global effects or errored).
            let final_offset = set_cursor_offset.unwrap_or(sel_range.head());
            cursor_deltas.push((sel_idx, final_offset, net_delta));
        }

        // Deduplicate overlapping text mutations from different cursors.
        // Non-overlapping cursors can produce overlapping effects (e.g., two
        // cursors on the same line both doing J target the same \n). Remove
        // duplicate mutations to prevent double-application by the host.
        let delta_before_dedup: i64 = cursor_deltas.iter().map(|(_, _, d)| d).sum();
        dedup_overlapping_effects(&mut all_effects);
        let delta_after_dedup = crate::effects::algebra::compute_length_change(&all_effects);

        // If dedup dropped mutations, some cursors' net_delta is over-counted.
        // Recompute by distributing the actual document delta proportionally.
        if delta_before_dedup != delta_after_dedup && delta_before_dedup != 0 {
            for (_, _, delta) in &mut cursor_deltas {
                // Scale each cursor's delta to match the actual surviving mutations.
                // For the common case (all cursors had same delta, one dropped),
                // this correctly zeroes the dropped cursor's contribution.
                //
                // Done in `i128` rather than `f64`: the product of two
                // document-sized deltas cannot overflow 128 bits, and integer
                // division truncates toward zero exactly like the `as i64` that
                // used to follow the float division — without the precision
                // loss that made the float version lint.
                let scaled = i128::from(*delta) * i128::from(delta_after_dedup)
                    / i128::from(delta_before_dedup);
                *delta = i64::try_from(scaled).unwrap_or_else(|_| {
                    if scaled.is_negative() {
                        i64::MIN
                    } else {
                        i64::MAX
                    }
                });
            }
        }

        // Undo markers stripped from inner effects. Intent carried as metadata.
        // materialize_undo_markers() is called ONCE in mode_dispatch.rs,
        // AFTER per-cursor returns.

        // Build consolidated register.
        // Register entries are in descending offset order; reverse to ascending
        // (selection order) so entry(0) = primary cursor (lowest offset).
        //
        // Each cursor may write to multiple registers (e.g., unnamed + "0 for yank,
        // unnamed + "1 for delete). Group by register name and consolidate each.
        let register_override = consolidate_registers(&mut all_effects, &mut register_entries);

        let undo_intent = captured_undo_intent.unwrap_or(UndoIntent::None);

        PerCursorResult {
            effects: all_effects.into_iter().collect(),
            cursor_deltas,
            register_override,
            undo_intent,
        }
    }
}

/// Group register entries by name, build multi-entry `RegisterContent` for each,
/// emit `SetRegister` effects into `all_effects`, and return the override data.
fn consolidate_registers(
    all_effects: &mut Vec<Effect>,
    register_entries: &mut Vec<(RegisterName, CompactString, MotionType)>,
) -> Option<PerCursorRegisterOverride> {
    if register_entries.is_empty() {
        return None;
    }

    register_entries.reverse(); // now ascending by cursor offset

    // Group entries by register name, preserving per-cursor order.
    let mut groups: Vec<(RegisterName, SmallVec<[CompactString; 1]>, MotionType)> = Vec::new();
    for (name, text, mt) in register_entries.iter() {
        if let Some(group) = groups.iter_mut().find(|(n, _, _)| *n == *name) {
            group.1.push(text.clone());
        } else {
            let mut entries = SmallVec::new();
            entries.push(text.clone());
            groups.push((*name, entries, *mt));
        }
    }

    // Build register overrides and emit SetRegister effects for ALL register
    // groups. process_effects needs to see the same SetRegister effects it would
    // see from single-cursor execution (UNNAMED for clipboard routing, LAST_YANK
    // for yank register, NUMBERED_1 for delete cascading, etc.).
    let mut override_pairs: Vec<(RegisterName, crate::primitives::RegisterContent)> = Vec::new();

    for (name, entries, motion_type) in groups {
        let content = crate::primitives::RegisterContent::from_entries(entries, motion_type);
        // Emit primary cursor's register text so process_effects handles state
        // sync (register routing, clipboard detection, numbered cascade) the same
        // way it would for single-cursor execution.
        all_effects.push(Effect::SetRegister {
            name,
            text: CompactString::from(content.entry(0)),
            motion_type,
        });
        override_pairs.push((name, content));
    }

    if override_pairs.is_empty() {
        None
    } else {
        Some(PerCursorRegisterOverride {
            pairs: override_pairs,
        })
    }
}

/// Update multi-cursor selections using a cumulative delta walk.
///
/// After per-cursor re-execution, each cursor has a raw `SetCursor` offset and a
/// net byte delta from its text mutations. Because effects are applied in
/// descending offset order, lower-offset cursors' positions are shifted by the
/// cumulative deltas of all higher-offset cursors that were applied before them.
///
/// This function sorts the deltas by ascending offset, walks them accumulating
/// the cumulative delta, and produces adjusted offsets for each cursor.
///
/// With `keep_anchors`, a selection that is not zero-width keeps its anchor,
/// as a visual selection does. Without it every cursor becomes a zero-width
/// insert cursor: Insert mode has no selections, and an anchor left behind
/// by the command that entered it would otherwise grow into a range that
/// swallows the cursor before it.
pub(super) fn update_selections_from_deltas(
    selections: &mut Selections,
    cursor_deltas: &[(usize, Offset, i64)],
    keep_anchors: bool,
) {
    if cursor_deltas.is_empty() {
        return;
    }

    let original_primary = selections.primary_index();

    // Sort ascending by raw offset for cumulative walk.
    let mut sorted: Vec<(usize, Offset, i64)> = cursor_deltas.to_vec();
    sorted.sort_by_key(|(_, offset, _)| offset.get());

    let mut cumulative_delta: i64 = 0;
    let mut new_ranges: Vec<SelectionRange> = Vec::with_capacity(sorted.len());
    let mut new_primary_idx: usize = 0;

    for (out_idx, (sel_idx, raw_offset, net_delta)) in sorted.iter().enumerate() {
        let adjusted_offset = Offset::new(byte_delta::shift(raw_offset.get(), cumulative_delta));
        // Preserve the original selection's anchor for visual mode.
        // insert_cursor sets anchor=head (zero-width), which destroys
        // visual selections. Instead, get the original range and update
        // only the head while preserving the anchor.
        let new_range = if let Some(orig) = selections.ranges().get(*sel_idx) {
            let anchor = orig.anchor();
            if !keep_anchors || anchor == orig.head() {
                // Non-visual (insert cursor): create zero-width at adjusted pos
                SelectionRange::insert_cursor(adjusted_offset)
            } else {
                // Visual selection: preserve anchor, update head
                SelectionRange::new(anchor, adjusted_offset)
            }
        } else {
            SelectionRange::insert_cursor(adjusted_offset)
        };
        new_ranges.push(new_range);
        if *sel_idx == original_primary {
            new_primary_idx = out_idx;
        }
        cumulative_delta += net_delta;
    }

    let new_sels = Selections::from_vec(new_ranges, new_primary_idx).normalize();
    *selections = new_sels;
}

/// Update selections after algebraic rebase.
///
/// After `replicate_effects_precise` for PI commands, the engine's internal
/// `multi_cursor().selections()` still holds pre-command positions. This
/// function extracts SetCursor offsets from the response effects and updates
/// selections using the same cumulative delta approach.
///
/// `primary_net_delta` is the net byte delta from the primary effects BEFORE
/// replication. For PI commands, all cursor groups produce the same delta.
///
/// Must be called AFTER the register override, which reads pre-edit
/// selections to compute per-cursor register text from the T0 document.
pub(super) fn update_selections_after_rebase(
    state: &mut crate::state::VimState,
    response: &crate::execution::response::Response,
    primary_net_delta: i64,
) {
    use crate::effects::Effect;

    let mc = state.multi_cursor();
    if !mc.is_active() {
        return;
    }
    let num_cursors = mc.selections().len();
    if num_cursors <= 1 {
        return;
    }

    // Extract per-cursor positions from the response effects.
    // SetSelection (visual mode) carries both anchor+head and is preferred.
    // SetCursor (normal mode) carries head only.
    // When both are present (visual mode produces paired SetSelection+SetCursor
    // per cursor), collect ONLY SetSelection to avoid double-counting which
    // would create 2N entries for N cursors and misalign the take(N) below.
    let mut set_selections: Vec<(Offset, Option<Offset>)> = Vec::new();
    let mut set_cursors: Vec<(Offset, Option<Offset>)> = Vec::new();
    for effect in response.effects.as_slice() {
        match effect {
            Effect::SetSelection { anchor, head, .. } => {
                set_selections.push((*head, Some(*anchor)));
            }
            Effect::SetCursor { offset } => {
                set_cursors.push((*offset, None));
            }
            _ => {}
        }
    }
    let cursor_positions = if set_selections.len() >= num_cursors {
        set_selections
    } else {
        set_cursors
    };

    // If we don't have enough positions for all cursors, fall back.
    if cursor_positions.len() < num_cursors {
        return;
    }

    // For the algebraic rebase path, the SetCursor offsets in the replicated
    // effects are in T0 coordinates. After descending-order application by
    // the host, each cursor position needs cumulative adjustment from
    // lower-offset cursor edits. Use the `update_selections_from_deltas`
    // with the per-cursor net delta (uniform for PI commands).
    //
    // `primary_net_delta` is the per-cursor delta directly — no division
    // by num_cursors needed.
    //
    // For pure motions (no text edits), primary_net_delta = 0 and each
    // SetCursor IS the final position. For text-editing PI commands (dd, p),
    // the SetCursor offsets are in T0 but the cumulative delta from other
    // cursor groups shifts them.
    // Build sel_idx mapping: SetCursor offsets are in descending order
    // (matching the cursor processing order). Map them back to original
    // selection indices by sorting selections descending.
    let mc_sels = state.multi_cursor().selections();
    let mut indices_desc: Vec<usize> = (0..num_cursors).collect();
    indices_desc.sort_by_key(|&idx| std::cmp::Reverse(mc_sels.ranges()[idx].head().get()));

    // Build selection ranges directly instead of going through the delta
    // walk, to correctly handle both SetCursor (normal mode) and
    // SetSelection (visual mode) effects.
    if primary_net_delta != 0 {
        // Text-editing PI commands: apply cumulative delta walk.
        let mut deltas: Vec<(usize, Offset, i64)> = Vec::with_capacity(num_cursors);
        for (i, (head, _)) in cursor_positions.iter().take(num_cursors).enumerate() {
            let sel_idx = indices_desc.get(i).copied().unwrap_or(i);
            deltas.push((sel_idx, *head, primary_net_delta));
        }
        update_selections_from_deltas(state.multi_cursor_mut().selections_mut(), &deltas, true);
    } else {
        // Pure motion (net_delta=0): update each selection's HEAD from the
        // extracted positions while PRESERVING the anchor. This is critical
        // for visual mode where the anchor marks the selection start.
        let sels = state.multi_cursor().selections();
        let existing_ranges = sels.ranges().to_vec();
        let primary_idx = sels.primary_index();

        let mut new_ranges: Vec<SelectionRange> = existing_ranges.clone();
        for (i, (head, anchor_opt)) in cursor_positions.iter().take(num_cursors).enumerate() {
            let sel_idx = indices_desc.get(i).copied().unwrap_or(i);
            if sel_idx < new_ranges.len() {
                new_ranges[sel_idx] = if let Some(anchor) = anchor_opt {
                    SelectionRange::new(*anchor, *head)
                } else {
                    // Preserve existing anchor, update head only.
                    SelectionRange::new(existing_ranges[sel_idx].anchor(), *head)
                };
            }
        }

        let new_sels = Selections::from_vec(new_ranges, primary_idx);
        *state.multi_cursor_mut().selections_mut() = new_sels;
    }
}

/// Extract the byte range targeted by a text-mutating effect.
///
/// Returns `None` for non-mutating effects (SetCursor, SetMode, etc.).
fn mutation_range(effect: &Effect) -> Option<(usize, usize)> {
    match effect {
        Effect::Insert { offset, text } if !text.is_empty() => Some((offset.get(), offset.get())),
        Effect::Delete { range } if range.start() != range.end() => {
            Some((range.start().get(), range.end().get()))
        }
        Effect::Replace { range, .. } if range.start() != range.end() => {
            Some((range.start().get(), range.end().get()))
        }
        _ => None,
    }
}

/// Remove duplicate text mutations whose target ranges overlap.
///
/// Effects are in descending offset order. Walks the list and drops any
/// text mutation whose range overlaps the most recently retained mutation.
/// Non-mutating effects (SetCursor, etc.) always pass through.
///
/// This handles the case where non-overlapping cursors produce overlapping
/// effects (e.g., two cursors on the same line both doing `J` target the
/// same `\n` character).
fn dedup_overlapping_effects(effects: &mut Vec<Effect>) {
    if effects.len() <= 1 {
        return;
    }

    let mut last_retained_range: Option<(usize, usize)> = None;

    effects.retain(|effect| {
        let Some(range) = mutation_range(effect) else {
            return true; // non-mutating: always keep
        };

        if let Some(last) = last_retained_range {
            // Overlapping ranges in descending order: current range's end
            // extends into the last retained range's start region.
            let overlaps = range.0 < last.1 && range.1 > last.0;
            if overlaps {
                return false; // drop overlapping mutation
            }
        }

        last_retained_range = Some(range);
        true
    });
}
