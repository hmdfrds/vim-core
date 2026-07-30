//! External-edit reconciliation and effect-to-host promotion helpers.

use smallvec::SmallVec;

use super::{Response, ResponseKind, VimEngine};
use crate::document::Document;
use crate::effects::Effect;
use crate::execution::{ExternalEdit, HostRequest};
use crate::primitives::byte_delta;
use crate::primitives::Offset;
use crate::state::mark_snapshot::MarkSnapshot;

/// Clamp a range to `doc_text` bounds and extract the corresponding slice.
fn clamp_and_extract<'a>(doc_text: &'a str, range: &crate::primitives::Range) -> &'a str {
    let doc_end = Offset::new(doc_text.len());
    let start = range.start().min(doc_end).get();
    let end = range.end().min(doc_end).get();
    if start < end {
        &doc_text[start..end]
    } else {
        ""
    }
}

impl VimEngine {
    /// Reconcile an external host-owned edit back into core state.
    ///
    /// Performs full position remapping, undo entry creation, and shadow
    /// document update so that engine state remains consistent after
    /// host-initiated text changes (LSP refactors, collaborator edits,
    /// autocomplete acceptance, etc.).
    pub fn apply_external_edit(&mut self, edit: ExternalEdit) -> Response {
        debug!(target: "vim::engine::external", "applying external edit (kind={:?}, deleted={}, inserted={})", edit.kind, edit.deleted.len(), edit.inserted.len());
        self.state.clear_transient();

        let ExternalEdit {
            deleted,
            inserted: inserted_text,
            caret_after,
            kind,
        } = edit;
        let offset = deleted.start().get();
        let raw_old_len = deleted.len();
        let new_len = inserted_text.len();

        // ① Build ChangeSet from the edit
        debug_assert!(
            self.shadow.is_some(),
            "apply_external_edit: shadow should be initialized for correct position remapping"
        );
        if self.shadow.is_none() {
            warn!("apply_external_edit called without shadow — position remapping uses fallback doc_len");
        }
        // Capture mark snapshot BEFORE remapping so undo restores pre-edit marks.
        // (Mirrors effect_processor.rs line 574-575: snapshot at BeginUndoGroup.)
        let marks_snapshot = MarkSnapshot::capture(self.state.marks());

        let doc_len = self.shadow.as_ref().map_or_else(
            // Fallback: use a generous upper-bound so marks beyond the edit aren't
            // clamped to output_len. The ChangeSet fast-path returns output_len
            // for positions >= input_len, so we need input_len > any stored position.
            // Uses saturating_add to prevent overflow with adversarial inputs.
            || offset.saturating_add(raw_old_len).saturating_add(1 << 20),
            |s| s.text().len(),
        );
        // Clamp old_len so the deleted range never extends past the document.
        let old_len = raw_old_len.min(doc_len.saturating_sub(offset));
        let changeset = crate::primitives::changeset::ChangeSet::from_edit_len(
            doc_len, offset, old_len, new_len,
        );

        // ② Remap all byte-offset state (special marks except visual/insert-stop,
        //    jumplist, changelist)
        self.state.remap_all_positions(&changeset);

        // ③ Remap visual marks and insert-stop mark (^)
        self.state.marks_mut().remap_visual_marks(&changeset);
        self.state.marks_mut().remap_insert_stop(&changeset);

        // ④ Adjust named marks with cross-line semantics
        let shadow_text: Option<&str> = self.shadow.as_ref().map(Document::text);
        let crosses_line = inserted_text.contains('\n')
            || shadow_text.is_some_and(|t| {
                let end = (offset + old_len).min(t.len());
                offset < end && t.get(offset..end).is_some_and(|s| s.contains('\n'))
            });
        let edit_line_end = shadow_text
            .and_then(|t| {
                let p = offset.min(t.len());
                match t.get(p..)?.find('\n') {
                    Some(i) => Some(p + i),
                    None if crosses_line => Some(t.len()),
                    None => None,
                }
            })
            .unwrap_or(if crosses_line { 0 } else { usize::MAX });
        let skip_same_line = !(old_len == 0 && crosses_line);
        self.state.marks_mut().adjust_named_offsets_ext(
            offset,
            old_len,
            new_len,
            edit_line_end,
            skip_same_line,
        );

        // ④¼ Remap multi-cursor selections through the changeset
        {
            let remapped = self
                .state
                .multi_cursor()
                .selections()
                .map_through_batched_directed(&changeset);
            self.state.multi_cursor_mut().set_selections(remapped);
        }

        // ④½ Invalidate syntax selection history (byte offsets are now stale)
        self.state.syntax_selection_mut().clear();

        // ⑤ Insert-mode position adjustment
        if let Some(insert_state) = self.state.insert_state_mut() {
            let entry = insert_state.entry_offset().map_or(0, Offset::get);
            let cursor = entry + insert_state.accumulated_text().len();

            if offset + old_len <= entry {
                // Edit BEFORE insert region: shift entry_offset
                let delta = byte_delta::delta(new_len, old_len);
                let new_entry = entry.saturating_add_signed(delta);
                insert_state.set_entry_offset(Offset::new(new_entry));
                // Also shift min_change_start if it's set
                if let Some(mcs) = insert_state.min_change_start() {
                    let new_mcs = mcs.get().saturating_add_signed(delta);
                    insert_state.set_min_change_start(Offset::new(new_mcs));
                }
                // Shift block_insert cursor_return_offset if present
                if let Some(block_ctx) = insert_state.block_insert_mut() {
                    block_ctx.shift_cursor_return_offset(delta);
                }
            } else if offset == cursor && old_len == 0 {
                // Pure insertion AT cursor end: extends the insert session.
                // Only record for dot-repeat if the kind says so (Completion,
                // Snippet). Other kinds (FormatOnType, AutoPair, HostDrift, etc.)
                // should not pollute accumulated_text.
                if kind.recorded_for_repeat() {
                    insert_state.push_str(&inserted_text);
                }
            } else if offset >= cursor {
                // Edit AFTER insert region: no-op for insert state
            } else if offset < entry {
                // Straddling edit: starts BEFORE the insert region and extends
                // into it. Split into pre-entry and within-region portions.
                let pre_entry_deleted = entry - offset;
                let within_deleted = old_len.saturating_sub(pre_entry_deleted);
                let overlap = within_deleted.min(cursor - entry);

                // Truncate from the HEAD of accumulated_text (the overlap is
                // at the beginning of the insert region, not the end).
                if let Some(is) = self.state.insert_state_mut() {
                    if overlap > 0 {
                        is.truncate_head_bytes(overlap);
                    }
                    // Shift entry_offset: the pre-entry bytes were replaced,
                    // so entry moves to offset + new_len.
                    is.set_entry_offset(Offset::new(offset + new_len));
                    is.clear_replaced_chars();
                    is.clear_mark_dot_override();
                }
            } else {
                // Edit purely WITHIN insert region (offset >= entry, offset < cursor).
                super::insert::reconcile_external_edit_mutations(
                    &mut self.state,
                    old_len,
                    inserted_text.as_str(),
                );
                if let Some(is) = self.state.insert_state_mut() {
                    is.clear_replaced_chars();
                    is.clear_mark_dot_override();
                }
            }
        }

        // ⑥ Create undo entry (marks_snapshot captured before remapping above)
        //
        // Merging kinds (Insert, Replace, Completion, Snippet, PasteOrIme,
        // AutoPair, HostNotified, CaretOnly) merge into an existing open undo
        // group when one is pending — so `u` undoes the entire insert session
        // (including completions/snippets) in one step.
        //
        // Non-merging kinds (FormatOnType, Refactor, HostDrift) always get
        // their own separate undo entry, regardless of pending group state.
        if kind.merges_undo_group() && self.state.undo_tree().has_pending_group() {
            // Merge into the existing open group: just mark edit extents.
            if old_len > 0 {
                self.state
                    .undo_tree_mut()
                    .mark_edit_range(Offset::new(offset), old_len);
            } else {
                self.state.undo_tree_mut().mark_edit_at(Offset::new(offset));
            }
            if new_len > 0 {
                self.state
                    .undo_tree_mut()
                    .mark_insert_extent(Offset::new(offset), new_len);
            }
        } else {
            // Create a new separate undo group for this edit.
            let cursor_before = self.state.undo_cursor_hint();
            let t0_len = self.shadow.as_ref().map(|s| s.text().len());
            let timestamp = self.keystroke_seq;
            let was_in_insert = self.state.mode().is_insert();

            let (broke_merge, force_committed) = self.state.undo_tree_mut().begin_external_group(
                cursor_before,
                marks_snapshot,
                t0_len,
                timestamp,
            );
            self.last_force_committed_node = force_committed;

            if old_len > 0 {
                self.state
                    .undo_tree_mut()
                    .mark_edit_range(Offset::new(offset), old_len);
            } else {
                self.state.undo_tree_mut().mark_edit_at(Offset::new(offset));
            }
            if new_len > 0 {
                self.state
                    .undo_tree_mut()
                    .mark_insert_extent(Offset::new(offset), new_len);
            }
            let ext_node_id = self
                .state
                .undo_tree_mut()
                .end_group(caret_after, timestamp, None);
            self.last_external_edit_node = ext_node_id;

            if broke_merge {
                self.state.undo_tree_mut().begin_merge();
            }

            // If we force-committed an INSERT pending group, the INSERT session
            // is still active but has no open undo group. Open a new one so text
            // typed after the external edit gets its own undo entry.
            if was_in_insert && force_committed.is_some() {
                let new_t0 = self.shadow.as_ref().map(|s| s.text().len());
                let last_visual = self.state.last_visual();
                self.state.undo_tree_mut().begin_group(
                    caret_after,
                    crate::primitives::UndoCursorStrategy::FirstEdit,
                    MarkSnapshot::new(),
                    new_t0,
                    crate::primitives::Mode::Insert,
                    last_visual,
                    false,
                );
            }
        }

        // ⑦ Update shadow document
        if let Some(shadow) = &mut self.shadow {
            if old_len > 0 && new_len > 0 {
                shadow.apply_replace(offset, offset + old_len, &inserted_text);
            } else if old_len > 0 {
                shadow.apply_delete(offset, offset + old_len);
            } else if new_len > 0 {
                shadow.apply_insert(offset, &inserted_text);
            }
        }

        // ⑧ Process pending effects
        let mut response = Response::ignored();
        crate::execution::effect_processor::process_effects(
            &mut self.state,
            &mut self.parser,
            false,
            &mut response,
        );
        self.register_pending_host_requests(response)
    }

    /// Apply an external edit and reconcile macro recording.
    ///
    /// Like `apply_external_edit`, but also appends the "net-new" text to the
    /// macro recording buffer using the text block encoding so that macro replay
    /// can faithfully reproduce the insertion as an atomic block with correct
    /// cursor positioning.
    ///
    /// For prefix-match completions (the common case), the inserted text starts
    /// with `replaced_text`, so only the suffix beyond the prefix is recorded —
    /// matching Vim's behavior of recording completion results rather than
    /// individual keystrokes.
    pub fn apply_external_edit_with_recording(
        &mut self,
        edit: ExternalEdit,
        replaced_text: &str,
    ) -> Response {
        let inserted = edit.inserted.clone();
        let caret_after = edit.caret_after();
        let deleted_range_start = edit.deleted().start().get();
        let record_macro = edit.kind.recorded_for_macro();
        let response = self.apply_external_edit(edit);

        // Only record to the macro buffer if the kind permits it.
        // Kinds like FormatOnType, Refactor, AutoPair, and HostDrift should
        // NOT be recorded — replaying them would produce incorrect results
        // since the host-side trigger won't fire on replay.
        if record_macro {
            // Record the net-new text as a text block: if the inserted text extends
            // the replaced prefix, only record the extension. Otherwise record all
            // of the inserted text.
            if let Some(net_new) = inserted.strip_prefix(replaced_text) {
                if !net_new.is_empty() {
                    // Net-new text starts at deleted_range_start + replaced_text.len()
                    // in the post-edit document. Cursor offset is relative to net_new.
                    let net_new_start = deleted_range_start + replaced_text.len();
                    let cursor_offset = caret_after.get().saturating_sub(net_new_start);
                    self.append_text_block_to_recording(net_new, cursor_offset);
                }
            } else {
                // Full replacement (case-insensitive or fuzzy match): the inserted
                // text does NOT start with replaced_text. During replay, the preceding
                // keystrokes already typed `replaced_text` at the cursor. Record
                // backspaces to erase those stale characters before the TextBlock
                // (matching Vim's ins_compl_fixRedoBufForLeader backspace logic).
                if !replaced_text.is_empty() {
                    let bs_count = replaced_text.chars().count();
                    self.append_backspaces_to_recording(bs_count);
                }
                let cursor_offset = caret_after.get().saturating_sub(deleted_range_start);
                self.append_text_block_to_recording(&inserted, cursor_offset);
            }
        }

        response
    }

    /// Apply multiple external edits atomically with a single ChangeSet for
    /// position remapping and one undo group.
    ///
    /// This is the batch counterpart to [`Self::apply_external_edit_with_recording`].
    /// When a host delivers multiple non-overlapping edits from a single
    /// transaction (e.g. multi-site LSP rename, batch formatting), applying
    /// them through a single ChangeSet avoids O(n) changeset composition
    /// and ensures all position remapping is done in one consistent pass.
    ///
    /// # Preconditions
    ///
    /// Edits **must** be sorted by `deleted.start()` ascending and
    /// non-overlapping. In debug builds this is asserted.
    ///
    /// `deleted_texts` must be parallel to `edits`: `deleted_texts[i]` is the
    /// text that was replaced by `edits[i]`. This is used for macro recording
    /// (the `replaced_text` parameter to `apply_external_edit_with_recording`).
    ///
    /// # Panics
    ///
    /// Only in builds with `debug_assertions` enabled, and only when the
    /// preconditions above are violated:
    ///
    /// - two consecutive entries of `edits` overlap or are out of order, i.e.
    ///   `edits[i].deleted.end() > edits[i + 1].deleted.start()`;
    /// - the engine has no shadow document, so there is no authoritative
    ///   pre-edit text to remap positions against.
    ///
    /// Release builds do not panic: a missing shadow is logged as a warning and
    /// the document length falls back to the largest edit end plus a 1 MiB
    /// margin, and unsorted edits simply produce a wrong `ChangeSet`.
    pub fn apply_external_edits_batch(
        &mut self,
        edits: Vec<ExternalEdit>,
        deleted_texts: &[&str],
    ) -> Response {
        // ── Trivial cases ────────────────────────────────────────────────
        if edits.is_empty() {
            return Response::ignored();
        }
        if edits.len() == 1 {
            let replaced = deleted_texts.first().copied().unwrap_or("");
            let edit = edits.into_iter().next().unwrap();
            return self.apply_external_edit_with_recording(edit, replaced);
        }

        debug!(target: "vim::engine::external", "applying external edits batch (count={})", edits.len());

        // ── Precondition: sorted and non-overlapping ─────────────────────
        #[cfg(debug_assertions)]
        {
            for pair in edits.windows(2) {
                debug_assert!(
                    pair[0].deleted.end() <= pair[1].deleted.start(),
                    "batch edits must be sorted by start and non-overlapping: {:?} overlaps {:?}",
                    pair[0].deleted,
                    pair[1].deleted,
                );
            }
        }

        self.state.clear_transient();

        // ── Shadow document length ───────────────────────────────────────
        debug_assert!(
            self.shadow.is_some(),
            "apply_external_edits_batch: shadow should be initialized"
        );
        if self.shadow.is_none() {
            warn!("apply_external_edits_batch called without shadow — position remapping uses fallback doc_len");
        }

        let shadow_len = self.shadow.as_ref().map_or_else(
            || {
                // Fallback: generous upper-bound (mirrors single-edit path)
                edits
                    .iter()
                    .map(|e| e.deleted.end().get())
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1 << 20)
            },
            |s| s.text().len(),
        );

        // ── Capture mark snapshot BEFORE remapping ───────────────────────
        let marks_snapshot = MarkSnapshot::capture(self.state.marks());

        // ── Build single ChangeSet from all edits ────────────────────────
        let changes = edits.iter().map(|e| {
            let start = e.deleted.start().get().min(shadow_len);
            let end = e.deleted.end().get().min(shadow_len).max(start);
            let ins: Option<&str> = if e.inserted.is_empty() {
                None
            } else {
                Some(e.inserted.as_str())
            };
            (start, end, ins)
        });
        let changeset = crate::primitives::changeset::ChangeSet::from_changes(shadow_len, changes);

        // ── Position remapping (one pass each) ───────────────────────────

        // ② Remap special marks, jumplist, changelist
        self.state.remap_all_positions(&changeset);

        // ③ Remap visual marks and insert-stop mark (^)
        self.state.marks_mut().remap_visual_marks(&changeset);
        self.state.marks_mut().remap_insert_stop(&changeset);

        // ④ Named marks in REVERSE order (original-document coordinates)
        let shadow_text: Option<&str> = self.shadow.as_ref().map(Document::text);
        for edit in edits.iter().rev() {
            let offset = edit.deleted.start().get();
            let old_len = edit.deleted.len().min(shadow_len.saturating_sub(offset));
            let new_len = edit.inserted.len();

            let crosses_line = edit.inserted.contains('\n')
                || shadow_text.is_some_and(|t| {
                    let end = (offset + old_len).min(t.len());
                    offset < end && t.get(offset..end).is_some_and(|s| s.contains('\n'))
                });
            let edit_line_end = shadow_text
                .and_then(|t| {
                    let p = offset.min(t.len());
                    match t.get(p..)?.find('\n') {
                        Some(i) => Some(p + i),
                        None if crosses_line => Some(t.len()),
                        None => None,
                    }
                })
                .unwrap_or(if crosses_line { 0 } else { usize::MAX });
            let skip_same_line = !(old_len == 0 && crosses_line);
            self.state.marks_mut().adjust_named_offsets_ext(
                offset,
                old_len,
                new_len,
                edit_line_end,
                skip_same_line,
            );
        }

        // ④¼ Remap multi-cursor selections through the changeset
        {
            let remapped = self
                .state
                .multi_cursor()
                .selections()
                .map_through_batched_directed(&changeset);
            self.state.multi_cursor_mut().set_selections(remapped);
        }

        // ④½ Invalidate syntax selection history (byte offsets are now stale)
        self.state.syntax_selection_mut().clear();

        // ⑤ Insert-mode: if in insert mode with a pending undo group,
        //    force-commit so the batch gets its own clean undo entry.
        //    Batch edits are inherently non-merging (multi-site rename, etc.).
        if self.state.insert_state().is_some() && self.state.undo_tree().has_pending_group() {
            let cursor_before = self.state.undo_cursor_hint();
            let t0_len = self.shadow.as_ref().map(|s| s.text().len());
            let timestamp = self.keystroke_seq;
            let (_broke_merge, force_committed) = self.state.undo_tree_mut().begin_external_group(
                cursor_before,
                marks_snapshot,
                t0_len,
                timestamp,
            );
            self.last_force_committed_node = force_committed;

            // Mark all edit ranges
            for edit in &edits {
                let offset = edit.deleted.start().get();
                let old_len = edit.deleted.len();
                let new_len = edit.inserted.len();
                if old_len > 0 {
                    self.state
                        .undo_tree_mut()
                        .mark_edit_range(Offset::new(offset), old_len);
                } else {
                    self.state.undo_tree_mut().mark_edit_at(Offset::new(offset));
                }
                if new_len > 0 {
                    self.state
                        .undo_tree_mut()
                        .mark_insert_extent(Offset::new(offset), new_len);
                }
            }

            let caret_after = edits.last().map_or(Offset::ZERO, |e| e.caret_after);
            let ext_node_id = self
                .state
                .undo_tree_mut()
                .end_group(caret_after, timestamp, None);
            self.last_external_edit_node = ext_node_id;

            // Re-open a continuation group for the ongoing INSERT session
            let new_t0 = None; // will be set after shadow update
            let last_visual = self.state.last_visual();
            self.state.undo_tree_mut().begin_group(
                caret_after,
                crate::primitives::UndoCursorStrategy::FirstEdit,
                MarkSnapshot::new(),
                new_t0,
                crate::primitives::Mode::Insert,
                last_visual,
                false,
            );
        } else {
            // ⑥ Create one undo group for the entire batch
            let cursor_before = self.state.undo_cursor_hint();
            let t0_len = self.shadow.as_ref().map(|s| s.text().len());
            let timestamp = self.keystroke_seq;

            let (broke_merge, force_committed) = self.state.undo_tree_mut().begin_external_group(
                cursor_before,
                marks_snapshot,
                t0_len,
                timestamp,
            );
            self.last_force_committed_node = force_committed;

            // Mark all edit ranges in the undo tree
            for edit in &edits {
                let offset = edit.deleted.start().get();
                let old_len = edit.deleted.len();
                let new_len = edit.inserted.len();
                if old_len > 0 {
                    self.state
                        .undo_tree_mut()
                        .mark_edit_range(Offset::new(offset), old_len);
                } else {
                    self.state.undo_tree_mut().mark_edit_at(Offset::new(offset));
                }
                if new_len > 0 {
                    self.state
                        .undo_tree_mut()
                        .mark_insert_extent(Offset::new(offset), new_len);
                }
            }

            let caret_after = edits.last().map_or(Offset::ZERO, |e| e.caret_after);
            let ext_node_id = self
                .state
                .undo_tree_mut()
                .end_group(caret_after, timestamp, None);
            self.last_external_edit_node = ext_node_id;

            if broke_merge {
                self.state.undo_tree_mut().begin_merge();
            }
        }

        // ⑦ Update shadow document: apply all edits in forward order with
        //    cumulative delta adjustment.
        if let Some(shadow) = &mut self.shadow {
            let mut delta: isize = 0;
            for edit in &edits {
                let raw_offset = edit.deleted.start().get();
                let raw_end = edit.deleted.end().get();
                let old_len = raw_end - raw_offset;
                let new_len = edit.inserted.len();

                let adj_offset = raw_offset.saturating_add_signed(delta);
                let adj_end = adj_offset + old_len;

                if old_len > 0 && new_len > 0 {
                    shadow.apply_replace(adj_offset, adj_end, &edit.inserted);
                } else if old_len > 0 {
                    shadow.apply_delete(adj_offset, adj_end);
                } else if new_len > 0 {
                    shadow.apply_insert(adj_offset, &edit.inserted);
                }

                delta += byte_delta::delta(new_len, old_len);
            }
        }

        // ⑧ Macro recording: record each edit if recording is active
        for (i, edit) in edits.iter().enumerate() {
            if edit.kind.recorded_for_macro() {
                let replaced = deleted_texts.get(i).copied().unwrap_or("");
                let inserted = &edit.inserted;
                let deleted_range_start = edit.deleted.start().get();
                let caret_after = edit.caret_after;

                if let Some(net_new) = inserted.strip_prefix(replaced) {
                    if !net_new.is_empty() {
                        let net_new_start = deleted_range_start + replaced.len();
                        let cursor_offset = caret_after.get().saturating_sub(net_new_start);
                        self.append_text_block_to_recording(net_new, cursor_offset);
                    }
                } else {
                    if !replaced.is_empty() {
                        let bs_count = replaced.chars().count();
                        self.append_backspaces_to_recording(bs_count);
                    }
                    let cursor_offset = caret_after.get().saturating_sub(deleted_range_start);
                    self.append_text_block_to_recording(inserted, cursor_offset);
                }
            }
        }

        // ⑨ Process pending effects
        let mut response = Response::ignored();
        crate::execution::effect_processor::process_effects(
            &mut self.state,
            &mut self.parser,
            false,
            &mut response,
        );
        self.register_pending_host_requests(response)
    }

    /// Stage text for a subsequent `InsertKind::HostInserted` command.
    ///
    /// Call this before feeding an `InsertKind::HostInserted` key event through
    /// the normal `process()` pipeline. The staged text is consumed during
    /// precomputation and tracked in `accumulated_text` for dot-repeat fidelity.
    ///
    /// This is the grammar-pipeline alternative to `apply_external_edit()`.
    /// Use this when the host wants completions/snippets to flow through the
    /// standard key-processing path (precompute → dispatch → mutations).
    #[inline]
    pub fn stage_host_insert(&mut self, text: &str) {
        self.state.stage_host_insert(text);
    }

    pub(super) fn promote_effects_to_host_requests(
        &mut self,
        response: &mut Response,
        doc_text: &str,
    ) {
        let mut retained: SmallVec<[Effect; 4]> = SmallVec::new();
        for effect in response.effects.drain(..) {
            match effect {
                Effect::OperatorFilter {
                    range, motion_type, ..
                } => {
                    // TODO: The `!{motion}` operator should open a command-line prompt
                    // (e.g. `:.,.+N!`) so the user can type the filter command, matching
                    // real Vim behavior. Currently we emit a `FilterDocumentRange` host
                    // request with an empty command, relying on the host to prompt for
                    // the filter command. A proper fix requires:
                    //   1. A `CommandLinePrompt::Filter` variant
                    //   2. Storing the pending filter range in engine state
                    //   3. On command-line commit, creating the `FilterDocumentRange`
                    //      host request with the user-provided command
                    let input_text = clamp_and_extract(doc_text, &range);
                    response
                        .host_requests
                        .push(HostRequest::FilterDocumentRange {
                            meta: self.host.sequencer.next_meta(),
                            range,
                            motion_type,
                            input_text: input_text.into(),
                            command: "".into(),
                        });
                    response.kind = ResponseKind::Pending;
                }
                Effect::OperatorReindent {
                    range,
                    motion_type,
                    start_col,
                    end_col,
                    end_line_in_range,
                    start_byte_offset,
                } => {
                    let input_text = clamp_and_extract(doc_text, &range);
                    response.host_requests.push(HostRequest::ReindentRange {
                        meta: self.host.sequencer.next_meta(),
                        range,
                        motion_type,
                        input_text: input_text.into(),
                        start_col,
                        end_col,
                        end_line_in_range,
                        start_byte_offset,
                    });
                    response.kind = ResponseKind::Pending;
                }
                other => retained.push(other),
            }
        }
        response.effects = retained;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{ExternalEdit, ExternalEditKind};
    use crate::primitives::{Mark, MarkName, Mode, Offset, Range};
    use crate::state::InsertState;

    /// Helper: create an engine with shadow text initialized.
    fn engine_with_shadow(text: &str) -> VimEngine {
        let mut engine = VimEngine::new();
        engine.set_shadow_text(text);
        engine
    }

    /// Helper: build an ExternalEdit for an insertion at `offset`.
    fn insert_edit(offset: usize, text: &str) -> ExternalEdit {
        ExternalEdit {
            deleted: Range::new(Offset::new(offset), Offset::new(offset)),
            inserted: text.into(),
            caret_after: Offset::new(offset + text.len()),
            kind: ExternalEditKind::HostNotified,
        }
    }

    /// Helper: build an ExternalEdit for a deletion at `start..end`.
    fn delete_edit(start: usize, end: usize) -> ExternalEdit {
        ExternalEdit {
            deleted: Range::new(Offset::new(start), Offset::new(end)),
            inserted: "".into(),
            caret_after: Offset::new(start),
            kind: ExternalEditKind::HostNotified,
        }
    }

    /// Helper: build an ExternalEdit for a replacement.
    fn replace_edit(start: usize, end: usize, text: &str) -> ExternalEdit {
        ExternalEdit {
            deleted: Range::new(Offset::new(start), Offset::new(end)),
            inserted: text.into(),
            caret_after: Offset::new(start + text.len()),
            kind: ExternalEditKind::HostNotified,
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // Test 1: external_edit_remaps_special_marks
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_remaps_special_marks() {
        // Named marks use cross-line semantics: marks on the SAME line as
        // a within-line edit are NOT shifted (matching Neovim). Use a
        // multi-line document with mark on a DIFFERENT line from the edit.
        //
        // "hello\nworld\n" — mark 'a' at offset 8 (on line 2, "r")
        // Edit: insert "XX" at offset 2 on line 1
        // Mark should shift forward by 2 because it's on a different line.
        let mut engine = engine_with_shadow("hello\nworld\n");
        engine
            .marks_mut()
            .set(MarkName::new('a').unwrap(), Mark::new(Offset::new(8)));

        // Insert "XX" at offset 2 (on line 1, before the newline at offset 5)
        engine.apply_external_edit(insert_edit(2, "XX"));

        // Mark is on line 2 (after the \n at offset 5+2=7), should shift by 2
        let mark = engine
            .state()
            .marks()
            .get(MarkName::new('a').unwrap())
            .expect("mark 'a' should still exist");
        assert_eq!(
            mark.offset().get(),
            10, // 8 + 2
            "mark 'a' on line 2 should shift forward by inserted length on line 1"
        );
    }

    #[test]
    fn external_edit_remaps_mark_after_deletion() {
        // Multi-line: "hello\nworld\n" — mark 'b' at offset 8 (line 2, "r")
        // Delete "lo" (offset 3..5) on line 1 — mark on line 2 shifts back.
        let mut engine = engine_with_shadow("hello\nworld\n");
        engine
            .marks_mut()
            .set(MarkName::new('b').unwrap(), Mark::new(Offset::new(8)));

        // Delete "lo" (offset 3..5, len 2) on line 1
        engine.apply_external_edit(delete_edit(3, 5));

        // Mark should shift back by 2
        let mark = engine
            .state()
            .marks()
            .get(MarkName::new('b').unwrap())
            .expect("mark 'b' should still exist");
        assert_eq!(
            mark.offset().get(),
            6, // 8 - 2
            "mark 'b' on line 2 should shift back by deleted length on line 1"
        );
    }

    #[test]
    fn external_edit_same_line_mark_not_shifted() {
        // Verify that marks on the SAME line as the edit are NOT shifted
        // (matching Neovim's behavior for named marks).
        let mut engine = engine_with_shadow("hello world");
        engine
            .marks_mut()
            .set(MarkName::new('a').unwrap(), Mark::new(Offset::new(6)));

        // Insert "XX" at offset 2 — same line as mark (no newline)
        engine.apply_external_edit(insert_edit(2, "XX"));

        // Mark should NOT shift (same-line skip)
        let mark = engine
            .state()
            .marks()
            .get(MarkName::new('a').unwrap())
            .expect("mark 'a' should still exist");
        assert_eq!(
            mark.offset().get(),
            6,
            "mark on same line should NOT shift for within-line edit"
        );
    }

    #[test]
    fn external_edit_remaps_special_mark_last_change() {
        // Special marks (like '.') are remapped by remap_all_positions via
        // the ChangeSet, which shifts regardless of line boundaries.
        let mut engine = engine_with_shadow("hello world");
        engine.marks_mut().set_last_change(Offset::new(6));

        // Insert "XX" at offset 2 (before the special mark)
        engine.apply_external_edit(insert_edit(2, "XX"));

        // Special marks shift unconditionally
        let mark = engine
            .state()
            .marks()
            .get(MarkName::LAST_CHANGE)
            .expect("mark '.' should still exist");
        assert_eq!(
            mark.offset().get(),
            8, // 6 + 2
            "special mark '.' should shift forward by inserted length"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Test 2: external_edit_creates_undo_node
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_creates_undo_node() {
        let mut engine = engine_with_shadow("hello world");
        let initial_count = engine.undo_tree().node_count();

        // Apply an external edit
        engine.apply_external_edit(insert_edit(5, " beautiful"));

        let new_count = engine.undo_tree().node_count();
        assert_eq!(
            new_count,
            initial_count + 1,
            "undo tree should grow by one node after external edit"
        );
    }

    #[test]
    fn external_edit_undo_node_for_replacement() {
        let mut engine = engine_with_shadow("hello world");
        let initial_count = engine.undo_tree().node_count();

        // Replace "world" with "earth"
        engine.apply_external_edit(replace_edit(6, 11, "earth"));

        assert_eq!(
            engine.undo_tree().node_count(),
            initial_count + 1,
            "replacement should create exactly one undo node"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Test 3: external_edit_insert_mode_before_shifts_entry
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_insert_mode_before_shifts_entry() {
        let mut engine = engine_with_shadow("hello world");

        // Enter insert mode with entry_offset at 6
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(6));
        is.push_str("xyz");
        engine.state.start_insert(is);

        // External edit inserts "AB" at offset 2 (BEFORE the insert region)
        engine.apply_external_edit(insert_edit(2, "AB"));

        let insert_state = engine
            .state()
            .insert_state()
            .expect("should still be in insert mode");
        assert_eq!(
            insert_state.entry_offset().unwrap().get(),
            8, // 6 + 2 (shifted by "AB".len())
            "entry_offset should shift forward when edit is before insert region"
        );
    }

    #[test]
    fn external_edit_insert_mode_after_no_shift() {
        let mut engine = engine_with_shadow("hello world");

        // Enter insert mode with entry_offset at 2, accumulated "xx"
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(2));
        is.push_str("xx");
        engine.state.start_insert(is);

        // External edit inserts "ZZ" at offset 10 (AFTER the insert region: entry=2 + accum=2 = cursor at 4)
        engine.apply_external_edit(insert_edit(10, "ZZ"));

        let insert_state = engine
            .state()
            .insert_state()
            .expect("should still be in insert mode");
        assert_eq!(
            insert_state.entry_offset().unwrap().get(),
            2,
            "entry_offset should not shift when edit is after insert region"
        );
        assert_eq!(
            insert_state.accumulated_text(),
            "xx",
            "accumulated text should not change for edit after insert region"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Additional: shadow document update
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_updates_shadow_document() {
        let mut engine = engine_with_shadow("hello world");

        engine.apply_external_edit(replace_edit(6, 11, "earth"));

        assert_eq!(
            engine.shadow_text().unwrap(),
            "hello earth",
            "shadow document should reflect the replacement"
        );
    }

    #[test]
    fn external_edit_shadow_insertion() {
        let mut engine = engine_with_shadow("ab");

        engine.apply_external_edit(insert_edit(1, "XY"));

        assert_eq!(engine.shadow_text().unwrap(), "aXYb");
    }

    #[test]
    fn external_edit_shadow_deletion() {
        let mut engine = engine_with_shadow("abcde");

        engine.apply_external_edit(delete_edit(1, 4));

        assert_eq!(engine.shadow_text().unwrap(), "ae");
    }

    // ══════════════════════════════════════════════════════════════════════
    // Insert-mode Within case
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_within_insert_clears_replaced_chars() {
        use crate::primitives::ReplacedChar;

        let mut engine = engine_with_shadow("hello world");
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::ReplaceMode);
        is.set_entry_offset(Offset::new(0));
        is.push_str("hello");
        is.push_replaced(ReplacedChar::Replaced('x'));
        is.push_replaced(ReplacedChar::Replaced('y'));
        engine.state.start_insert(is);

        // Edit WITHIN the insert region (offset 2, which is between entry=0 and cursor=5)
        engine.apply_external_edit(replace_edit(2, 4, "ZZ"));

        let insert_state = engine.state().insert_state().expect("still in insert mode");
        assert!(
            !insert_state.has_replaced(),
            "replaced_chars should be cleared after Within-region external edit"
        );
    }

    #[test]
    fn external_edit_within_insert_clears_mark_dot_override() {
        let mut engine = engine_with_shadow("hello world");
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(0));
        is.push_str("hello");
        is.set_mark_dot_override(3);
        engine.state.start_insert(is);

        // Edit WITHIN the insert region
        engine.apply_external_edit(replace_edit(2, 4, "ZZ"));

        let insert_state = engine.state().insert_state().expect("still in insert mode");
        assert_eq!(
            insert_state.mark_dot_override_pos(),
            None,
            "mark_dot_override should be cleared after Within-region external edit"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Jumplist shift via external edit
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_shifts_jumplist() {
        let mut engine = engine_with_shadow("hello\nworld\nfoo\n");
        // Add a jumplist entry at offset 8 ("r" in "world")
        engine.state.jump_list_mut().push(Offset::new(8), None);

        // Insert "XX" at offset 0 (before the jumplist entry)
        engine.apply_external_edit(insert_edit(0, "XX"));

        // The jumplist entry should shift forward by 2
        let entries = engine.state().jump_list().entries();
        assert!(!entries.is_empty(), "jumplist should not be empty");
        let last_entry = entries.back().unwrap();
        assert_eq!(
            last_entry.offset().get(),
            10, // 8 + 2
            "jumplist entry should shift forward by inserted text length"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Insert-stop mark (^) remapping
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_remaps_insert_stop_mark() {
        let mut engine = engine_with_shadow("hello world");
        engine.state.marks_mut().set_insert_stop(Offset::new(6));

        // Insert "XX" at offset 2 (before the insert-stop mark)
        engine.apply_external_edit(insert_edit(2, "XX"));

        let mark = engine
            .state()
            .marks()
            .get(MarkName::INSERT_STOP)
            .expect("insert-stop mark should exist");
        assert_eq!(
            mark.offset().get(),
            8, // 6 + 2
            "insert-stop mark should shift forward by inserted length"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Syntax selection history cleared
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_clears_syntax_selection() {
        let mut engine = engine_with_shadow("hello world");
        engine
            .state
            .syntax_selection_mut()
            .push(crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(Offset::new(0), Offset::new(5)),
            ));
        assert!(
            !engine.state().syntax_selection().is_empty(),
            "precondition: syntax selection should be non-empty"
        );

        engine.apply_external_edit(insert_edit(2, "XX"));

        assert!(
            engine.state().syntax_selection().is_empty(),
            "syntax selection should be cleared after external edit"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Block insert cursor_return_offset shift
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_before_shifts_block_insert_cursor_return() {
        use crate::state::BlockInsertContext;

        let mut engine = engine_with_shadow("hello\nworld\nfoo\n");
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(12)); // "foo" line
        is.push_str("X");
        is.set_block_insert(BlockInsertContext::new(2, 0, Offset::new(6)));
        engine.state.start_insert(is);

        // Edit BEFORE the insert region: insert "AB" at offset 0
        engine.apply_external_edit(insert_edit(0, "AB"));

        let insert_state = engine.state().insert_state().expect("still in insert mode");
        let block_ctx = insert_state
            .block_insert()
            .expect("block insert should exist");
        assert_eq!(
            block_ctx.cursor_return_offset().get(),
            8, // 6 + 2
            "block insert cursor_return_offset should shift by inserted text length"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // AT-CURSOR: pure insertion at cursor end extends accumulated_text
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn external_edit_at_cursor_extends_accumulated_text() {
        let mut engine = engine_with_shadow("hello pr\n");
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(6));
        is.push_str("pr");
        engine.state.start_insert(is);

        // Completion inserts "int" at offset 8 (cursor = 6 + 2 = 8), old_len = 0
        // Use Completion kind — only Completion/Snippet record for dot-repeat.
        let edit = ExternalEdit {
            deleted: Range::new(Offset::new(8), Offset::new(8)),
            inserted: "int".into(),
            caret_after: Offset::new(11),
            kind: ExternalEditKind::Completion,
        };
        engine.apply_external_edit(edit);

        let insert_state = engine.state().insert_state().expect("still in insert mode");
        assert_eq!(
            insert_state.accumulated_text(),
            "print",
            "Completion at cursor should extend accumulated_text for dot-repeat"
        );
        assert_eq!(
            insert_state.entry_offset().unwrap().get(),
            6,
            "entry_offset should not change for at-cursor insertion"
        );
    }

    #[test]
    fn external_edit_at_cursor_host_notified_does_not_extend_accumulated_text() {
        let mut engine = engine_with_shadow("hello pr\n");
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(6));
        is.push_str("pr");
        engine.state.start_insert(is);

        // HostNotified inserts "int" at cursor — should NOT extend accumulated_text
        // because HostNotified.recorded_for_repeat() is false.
        engine.apply_external_edit(insert_edit(8, "int"));

        let insert_state = engine.state().insert_state().expect("still in insert mode");
        assert_eq!(
            insert_state.accumulated_text(),
            "pr",
            "HostNotified at cursor should NOT extend accumulated_text"
        );
    }

    #[test]
    fn external_edit_at_cursor_with_deletion_is_after() {
        let mut engine = engine_with_shadow("hello pr world\n");
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(6));
        is.push_str("pr");
        engine.state.start_insert(is);

        // Edit replaces " world" at offset 8 (cursor = 8), old_len = 6 (NOT pure insertion)
        engine.apply_external_edit(replace_edit(8, 14, "XYZ"));

        let insert_state = engine.state().insert_state().expect("still in insert mode");
        assert_eq!(
            insert_state.accumulated_text(),
            "pr",
            "replacement starting at cursor should NOT modify accumulated_text"
        );
    }

    #[test]
    fn external_edit_strictly_after_cursor_no_change() {
        let mut engine = engine_with_shadow("hello pr world\n");
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(6));
        is.push_str("pr");
        engine.state.start_insert(is);

        // Insert at offset 12 (strictly after cursor=8)
        engine.apply_external_edit(insert_edit(12, "ZZ"));

        let insert_state = engine.state().insert_state().expect("still in insert mode");
        assert_eq!(
            insert_state.accumulated_text(),
            "pr",
            "insertion strictly after cursor should NOT modify accumulated_text"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // End-to-end undo coherence: AutoPair merges, FormatOnType separates
    // ══════════════════════════════════════════════════════════════════════

    /// Verifies that undo grouping works correctly for mixed external edits
    /// during an insert session:
    ///
    /// - AutoPair (merges_undo_group=true) merges into the open insert group
    /// - FormatOnType (merges_undo_group=false) creates a separate undo node
    ///
    /// Expected undo behavior:
    /// - Undo once → reverts FormatOnType only
    /// - Undo again → reverts the insert session (including AutoPair)
    #[test]
    fn undo_coherence_autopair_merges_format_on_type_separates() {
        use crate::primitives::UndoCursorStrategy;
        use crate::state::mark_snapshot::MarkSnapshot;

        // Start with "X\n" — we'll simulate typing "hello)" at offset 1
        // (after X), then format-on-type adds "  formatted" at offset 8.
        let initial_text = "X\n";
        let mut engine = engine_with_shadow(initial_text);

        // ── Simulate entering insert mode ──
        // The effect processor would call begin_group on BeginUndoGroup.
        engine.state.set_mode(Mode::Insert);
        let mut is = InsertState::new(crate::primitives::InsertEntryType::BeforeCursor);
        is.set_entry_offset(Offset::new(1)); // inserting after "X"
        engine.state.start_insert(is);

        // Open the undo group (mimics BeginUndoGroup effect processing)
        let marks_snapshot = MarkSnapshot::capture(engine.state().marks());
        let mode = engine.state().mode();
        engine.state.undo_tree_mut().begin_group(
            Offset::new(1), // cursor_before
            UndoCursorStrategy::FirstEdit,
            marks_snapshot,
            Some(initial_text.len()),
            mode,
            None,
            false,
        );

        let node_count_after_begin = engine.undo_tree().node_count();
        assert!(
            engine.state().undo_tree().has_pending_group(),
            "precondition: undo group should be pending after begin_group"
        );

        // ── Simulate typing "hello" ──
        // In a real session, each char would go through process() which emits
        // Insert effects. This marks the edit extent in the undo tree and
        // updates the shadow + insert state.
        engine.state.undo_tree_mut().mark_edit_at(Offset::new(1));
        engine
            .state
            .undo_tree_mut()
            .mark_insert_extent(Offset::new(1), 5); // "hello" = 5 bytes
        if let Some(is) = engine.state.insert_state_mut() {
            is.push_str("hello");
        }
        // Update shadow: "X\n" → "Xhello\n"
        if let Some(shadow) = &mut engine.shadow {
            shadow.apply_insert(1, "hello");
        }

        // Verify: still one pending group, no new nodes
        assert_eq!(
            engine.undo_tree().node_count(),
            node_count_after_begin,
            "typing should not create new undo nodes (edits merge into pending group)"
        );
        assert!(engine.state().undo_tree().has_pending_group());

        // ── Send ExternalEdit with kind=AutoPair, text=")" ──
        // AutoPair.merges_undo_group() == true, and has_pending_group() == true,
        // so this should merge into the existing group (no new node).
        let autopair_edit = ExternalEdit {
            deleted: Range::new(Offset::new(6), Offset::new(6)), // insert at offset 6 (after "hello")
            inserted: ")".into(),
            caret_after: Offset::new(7),
            kind: ExternalEditKind::AutoPair,
        };
        engine.apply_external_edit(autopair_edit);

        assert_eq!(
            engine.undo_tree().node_count(),
            node_count_after_begin,
            "AutoPair should merge into pending group — no new undo node"
        );
        assert!(
            engine.state().undo_tree().has_pending_group(),
            "pending group should still be open after AutoPair merge"
        );
        // Shadow is now "Xhello)\n"
        assert_eq!(engine.shadow_text().unwrap(), "Xhello)\n");

        // ── Send ExternalEdit with kind=FormatOnType, text="  formatted" ──
        // FormatOnType.merges_undo_group() == false, so this creates its own
        // separate undo group regardless of pending group state.
        let format_edit = ExternalEdit {
            deleted: Range::new(Offset::new(8), Offset::new(8)), // insert at end of "Xhello)\n"
            inserted: "  formatted".into(),
            caret_after: Offset::new(19),
            kind: ExternalEditKind::FormatOnType,
        };
        engine.apply_external_edit(format_edit);

        // FormatOnType commits the existing insert-mode pending group FIRST,
        // then creates and commits its own group. Total: +2 nodes.
        assert_eq!(
            engine.undo_tree().node_count(),
            node_count_after_begin + 2,
            "FormatOnType should commit insert group (+1) then its own (+1) = +2 nodes"
        );

        // After FormatOnType, a NEW pending group is opened for the
        // continuing INSERT session (text typed after format gets its own
        // undo entry).
        assert!(
            engine.state().undo_tree().has_pending_group(),
            "continuation INSERT group should be pending"
        );

        let shadow = engine.shadow_text().unwrap();
        assert_eq!(shadow, "Xhello)\n  formatted");

        // ── Simulate pressing Escape (EndUndoGroup) ──
        // The continuation group has no edits, so end_group commits an
        // empty group (which the tree may discard or keep depending on
        // whether any edit extents were marked).
        let timestamp = engine.keystroke_seq;
        let cursor_after = Offset::new(7);
        let _end_result = engine
            .state
            .undo_tree_mut()
            .end_group(cursor_after, timestamp, None);

        // ── Verify final undo tree structure ──
        // insert node (+1) + FormatOnType node (+1) + continuation node (+1) = +3
        // The continuation group is committed by end_group even though empty.
        let final_node_count = engine.undo_tree().node_count();
        assert_eq!(
            final_node_count,
            node_count_after_begin + 3,
            "insert + FormatOnType + continuation = +3 from baseline"
        );

        // ── Undo once: reverts empty continuation (no-op visually) ──
        let undo_step_0 = engine.state.undo_with_marks();
        assert!(undo_step_0.is_some(), "undo continuation group");

        // ── Undo again: reverts FormatOnType ──
        let undo_step = engine.state.undo_with_marks();
        assert!(
            undo_step.is_some(),
            "undo should succeed (reverts FormatOnType)"
        );

        // ── Undo again: reverts insert session ("hello" + ")") ──
        let undo_step_2 = engine.state.undo_with_marks();
        assert!(
            undo_step_2.is_some(),
            "undo should succeed — insert session undo group was preserved"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Batch external edit tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn batch_empty_is_noop() {
        let mut engine = engine_with_shadow("hello world");
        let initial_count = engine.undo_tree().node_count();

        let response = engine.apply_external_edits_batch(vec![], &[]);

        // No undo nodes created
        assert_eq!(
            engine.undo_tree().node_count(),
            initial_count,
            "empty batch should not create undo nodes"
        );
        // Shadow unchanged
        assert_eq!(engine.shadow_text().unwrap(), "hello world");
        // Response is ignored
        assert!(
            response.effects.is_empty(),
            "empty batch should produce no effects"
        );
    }

    #[test]
    fn batch_single_edit_delegates_to_single() {
        // Verify single-edit batch produces same result as non-batch
        let mut engine_batch = engine_with_shadow("hello world");
        let mut engine_single = engine_with_shadow("hello world");

        // Same mark on both engines
        engine_batch
            .marks_mut()
            .set(MarkName::new('a').unwrap(), Mark::new(Offset::new(8)));
        engine_single
            .marks_mut()
            .set(MarkName::new('a').unwrap(), Mark::new(Offset::new(8)));

        let edit = insert_edit(5, " beautiful");

        // Batch path (single element)
        engine_batch.apply_external_edits_batch(vec![edit.clone()], &[""]);

        // Single path
        engine_single.apply_external_edit_with_recording(edit, "");

        // Shadow documents should match
        assert_eq!(
            engine_batch.shadow_text().unwrap(),
            engine_single.shadow_text().unwrap(),
            "single-edit batch should produce same shadow as direct single"
        );

        // Undo tree should have same number of nodes
        assert_eq!(
            engine_batch.undo_tree().node_count(),
            engine_single.undo_tree().node_count(),
            "single-edit batch should create same undo structure"
        );
    }

    #[test]
    fn batch_multiple_edits_updates_shadow() {
        // "hello world foo" — apply two edits:
        //   1. Replace "hello" (0..5) with "HI"
        //   2. Replace "foo" (12..15) with "BAR"
        let mut engine = engine_with_shadow("hello world foo");

        let edits = vec![replace_edit(0, 5, "HI"), replace_edit(12, 15, "BAR")];

        engine.apply_external_edits_batch(edits, &["hello", "foo"]);

        assert_eq!(
            engine.shadow_text().unwrap(),
            "HI world BAR",
            "batch should apply all edits to shadow"
        );
    }

    #[test]
    fn batch_multiple_edits_creates_one_undo_group() {
        let mut engine = engine_with_shadow("hello world foo");
        let initial_count = engine.undo_tree().node_count();

        let edits = vec![replace_edit(0, 5, "HI"), replace_edit(12, 15, "BAR")];

        engine.apply_external_edits_batch(edits, &["hello", "foo"]);

        // Should create exactly one undo node for the batch
        assert_eq!(
            engine.undo_tree().node_count(),
            initial_count + 1,
            "batch should create exactly one undo group"
        );
    }

    #[test]
    fn batch_remaps_marks_across_edits() {
        // "aaa\nbbb\nccc\n" (14 bytes)
        // Mark 'a' at offset 8 ("c" in "ccc")
        // Edit 1: insert "XX" at offset 0 on first line
        // Edit 2: insert "YY" at offset 4 (after first \n)
        // Mark on line 3 should shift by total inserted = 4
        let mut engine = engine_with_shadow("aaa\nbbb\nccc\n");
        engine
            .marks_mut()
            .set(MarkName::new('a').unwrap(), Mark::new(Offset::new(10)));

        let edits = vec![
            insert_edit(0, "XX"),
            insert_edit(4, "YY"), // after first \n
        ];

        engine.apply_external_edits_batch(edits, &["", ""]);

        // Shadow should be "XXaaa\nYYbbb\nccc\n"
        assert_eq!(engine.shadow_text().unwrap(), "XXaaa\nYYbbb\nccc\n");

        // Special marks (last_change) should be remapped via changeset
        // Named mark 'a' at offset 10 (on line 3) should shift by 4
        let mark = engine
            .state()
            .marks()
            .get(MarkName::new('a').unwrap())
            .expect("mark 'a' should exist");
        assert_eq!(
            mark.offset().get(),
            14, // 10 + 4 (two insertions before it)
            "mark on later line should shift by total inserted bytes"
        );
    }

    #[test]
    fn batch_clears_syntax_selection() {
        let mut engine = engine_with_shadow("hello world");
        engine
            .state
            .syntax_selection_mut()
            .push(crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(Offset::new(0), Offset::new(5)),
            ));
        assert!(!engine.state().syntax_selection().is_empty());

        let edits = vec![insert_edit(0, "X"), insert_edit(6, "Y")];
        engine.apply_external_edits_batch(edits, &["", ""]);

        assert!(
            engine.state().syntax_selection().is_empty(),
            "batch should clear syntax selection history"
        );
    }

    #[test]
    fn batch_shifts_jumplist() {
        let mut engine = engine_with_shadow("hello\nworld\nfoo\n");
        engine.state.jump_list_mut().push(Offset::new(8), None);

        // Insert "XX" at offset 0 and "YY" at offset 6
        let edits = vec![insert_edit(0, "XX"), insert_edit(6, "YY")];
        engine.apply_external_edits_batch(edits, &["", ""]);

        let entries = engine.state().jump_list().entries();
        assert!(!entries.is_empty());
        let last_entry = entries.back().unwrap();
        assert_eq!(
            last_entry.offset().get(),
            12, // 8 + 2 + 2
            "jumplist entry should shift by total inserted bytes"
        );
    }

    #[test]
    fn batch_multiple_insertions_shadow_correctness() {
        // Pure insertions at multiple positions
        let mut engine = engine_with_shadow("abcdef");

        let edits = vec![
            insert_edit(1, "X"), // after 'a'
            insert_edit(3, "Y"), // after 'c' (in original coords)
            insert_edit(5, "Z"), // after 'e' (in original coords)
        ];

        engine.apply_external_edits_batch(edits, &["", "", ""]);

        assert_eq!(
            engine.shadow_text().unwrap(),
            "aXbcYdeZf",
            "multiple insertions should be applied correctly"
        );
    }

    #[test]
    fn batch_multiple_deletions_shadow_correctness() {
        // Delete at multiple positions: "aXbYcZd" -> "abcd"
        let mut engine = engine_with_shadow("aXbYcZd");

        let edits = vec![
            delete_edit(1, 2), // delete "X"
            delete_edit(3, 4), // delete "Y" (original coords)
            delete_edit(5, 6), // delete "Z" (original coords)
        ];

        engine.apply_external_edits_batch(edits, &["X", "Y", "Z"]);

        assert_eq!(
            engine.shadow_text().unwrap(),
            "abcd",
            "multiple deletions should be applied correctly"
        );
    }
}
