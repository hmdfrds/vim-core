//! Type definitions for the shadow execution subsystem.
//!
//! These types are the data structures used by the shadow execution loop
//! and engine integration. `ShadowResult` is the return type of the
//! orchestration function — it is not an `Effect` or `Response`, but a
//! structured result that the engine integration converts into `Response`
//! effects.
//!
//! # Design decisions
//!
//! - `ShadowContext` does **not** own the `OwnedDocument` or original text.
//!   Those are kept as separate locals so the borrow checker allows
//!   `&shadow_doc` for `InputContext` and `&mut shadow_doc` for applying
//!   effects simultaneously.
//! - `ShadowStatus` is built at the end from the loop exit condition, not
//!   stored on `ShadowContext` during execution.
//! - Undo groups (`BeginUndoGroup`/`EndUndoGroup`) are classified as
//!   `PassThrough`, accumulate in `deferred_effects`, and survive
//!   coalescing — no separate tracking needed.

use crate::document::Document;
use crate::effects::Effect;
use crate::primitives::SelectionRange;

// ═══════════════════════════════════════════════════════════════════════════════
// ShadowAbortReason
// ═══════════════════════════════════════════════════════════════════════════════

/// Why shadow execution was aborted before processing all keys.
///
/// Stored inside [`ShadowStatus::Aborted`] to distinguish recoverable
/// host-interaction aborts from engine errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::execution::engine) enum ShadowAbortReason {
    /// An effect classified as `HostRequired` was encountered.
    ///
    /// The macro replay must fall back to per-key host round-trips for
    /// the remaining unprocessed keys.
    HostInteractionRequired,

    /// `VimEngine::process()` returned an error.
    ///
    /// The shadow state may be inconsistent; discard all shadow results
    /// and fall back to normal replay.
    #[allow(dead_code)] // Reserved for future error detection in process()
    EngineError,

    /// The host set the external cancellation flag.
    ///
    /// The macro replay was interrupted because the cancel-check closure
    /// returned `true`. The shadow state up to this point is discarded
    /// and the engine falls back to normal replay.
    Cancelled,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ShadowStatus
// ═══════════════════════════════════════════════════════════════════════════════

/// Outcome status of a shadow execution run.
///
/// Used in [`ShadowResult`] to tell the caller whether the shadow run
/// completed fully or was cut short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::execution::engine) enum ShadowStatus {
    /// Shadow execution is still in progress.
    ///
    /// This is the initial state during the loop; a finished
    /// `ShadowResult` should never carry this variant.
    #[allow(dead_code)] // Semantic sentinel; used in tests for completeness
    Active,

    /// Shadow execution was aborted before processing all keys.
    ///
    /// `reason` explains why, and `keys_processed` records how many
    /// keys were successfully applied before the abort.
    Aborted {
        /// Why shadow execution could not continue.
        reason: ShadowAbortReason,
        /// Number of keys successfully processed before the abort.
        keys_processed: usize,
    },

    /// All keys were processed successfully in shadow mode.
    Completed,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ShadowResult
// ═══════════════════════════════════════════════════════════════════════════════

/// Structured result of a shadow execution run.
///
/// This is the return type of the shadow orchestration function. The engine
/// integration layer converts it into `Response` effects for the host.
///
/// # Fields
///
/// - `text_effects`: diff-based, minimal (0 or 1 effects). Computed by
///   [`compute_diff`](super::shadow_diff::compute_diff) comparing the
///   original text to the `OwnedDocument`'s final state.
/// - `other_effects`: coalesced UI/state effects from the deferred buffer.
/// - `final_cursor`: final cursor byte offset after all keys, if known.
/// - `status`: whether the run completed, aborted, or is still active.
#[derive(Debug, Clone)]
pub(in crate::execution::engine) struct ShadowResult {
    /// Diff-based text effect (0 or 1 entries).
    ///
    /// Computed by comparing original text to the `OwnedDocument`'s
    /// final state. Empty if the text was not modified.
    pub(in crate::execution::engine) text_effects: Vec<Effect>,

    /// Coalesced UI/state effects accumulated during shadow execution.
    ///
    /// These are `PassThrough`-classified effects that survived
    /// deduplication via [`coalesce_effects`](super::shadow_effects::coalesce_effects).
    pub(in crate::execution::engine) other_effects: Vec<Effect>,

    /// Final cursor byte offset after all keys, if known.
    ///
    /// `None` if no `SetCursor` effect was seen during shadow execution.
    pub(in crate::execution::engine) final_cursor: Option<usize>,

    /// Outcome status of the shadow execution run.
    #[allow(dead_code)]
    // Currently only read by tests; production abort/complete handling not yet implemented
    pub(in crate::execution::engine) status: ShadowStatus,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ShadowContext
// ═══════════════════════════════════════════════════════════════════════════════

/// Mutable context carried through the shadow execution loop.
///
/// Tracks the evolving cursor, selection, deferred effects, and progress
/// counter. Does **not** own the `OwnedDocument` or original text — those
/// are kept as separate locals to avoid borrow-checker conflicts when
/// building `InputContext` (needs `&doc`) while also mutating the document
/// (needs `&mut doc`).
#[derive(Debug, Clone)]
pub(in crate::execution::engine) struct ShadowContext {
    /// Current cursor byte offset in the shadow document.
    cursor: usize,

    /// Current visual selection, if any.
    ///
    /// `None` when not in visual mode. Updated by `SetSelection` /
    /// `ClearSelection` effects.
    selection: Option<SelectionRange>,

    /// Accumulated `PassThrough` effects awaiting coalescing.
    ///
    /// These are delivered to the host after shadow execution completes.
    /// Includes undo group markers (`BeginUndoGroup`/`EndUndoGroup`).
    deferred_effects: Vec<Effect>,

    /// Number of keys successfully processed so far.
    ///
    /// Used for abort reporting in [`ShadowStatus::Aborted`].
    keys_processed: usize,
}

impl ShadowContext {
    /// Create a new `ShadowContext` with the given initial cursor position.
    ///
    /// Starts with no selection, an empty deferred-effects buffer, and
    /// zero keys processed.
    pub(in crate::execution::engine) const fn new(cursor: usize) -> Self {
        Self {
            cursor,
            selection: None,
            deferred_effects: Vec::new(),
            keys_processed: 0,
        }
    }

    /// Get the current cursor byte offset.
    pub(in crate::execution::engine) const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Update the cursor byte offset.
    pub(in crate::execution::engine) const fn set_cursor(&mut self, offset: usize) {
        self.cursor = offset;
    }

    /// Get the current visual selection, if any.
    pub(in crate::execution::engine) const fn selection(&self) -> Option<SelectionRange> {
        self.selection
    }

    /// Set the visual selection.
    pub(in crate::execution::engine) const fn set_selection(&mut self, selection: SelectionRange) {
        self.selection = Some(selection);
    }

    /// Clear the visual selection.
    pub(in crate::execution::engine) const fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Accumulate a `PassThrough` effect into the deferred buffer.
    pub(in crate::execution::engine) fn push_deferred_effect(&mut self, effect: Effect) {
        self.deferred_effects.push(effect);
    }

    /// Take ownership of the accumulated deferred effects.
    ///
    /// Returns the deferred-effects buffer, leaving the context with an
    /// empty vec. Used when building the final [`ShadowResult`].
    pub(in crate::execution::engine) fn take_deferred_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.deferred_effects)
    }

    /// Get the number of deferred effects accumulated so far.
    #[cfg(test)]
    pub(in crate::execution::engine) const fn deferred_effect_count(&self) -> usize {
        self.deferred_effects.len()
    }

    /// Get the number of keys processed so far.
    pub(in crate::execution::engine) const fn keys_processed(&self) -> usize {
        self.keys_processed
    }

    /// Increment the keys-processed counter by one.
    pub(in crate::execution::engine) const fn increment_keys_processed(&mut self) {
        self.keys_processed += 1;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shadow Execution Loop
// ═══════════════════════════════════════════════════════════════════════════════

use super::shadow_diff::compute_diff;
use super::shadow_document::OwnedDocument;
use super::shadow_effects::{classify_effect, coalesce_effects, EffectCategory};
use crate::execution::InputContext;

impl super::VimEngine {
    /// Execute a shadow replay of pending macro keys against an in-memory document.
    ///
    /// Uses [`ScopedFork`](super::fork::ScopedFork) to gate the engine into
    /// speculative mode (`fork_active = true`), which suppresses recording,
    /// pipeline, federation, and session recording inside `process()`.
    /// `ScopedFork`'s `Drop` impl guarantees `fork_active` is cleared even
    /// on panic, replacing the old `catch_unwind` + manual `shadow_active`
    /// flag pattern.
    ///
    /// Drains all pending macro keys from the stack, processing each through
    /// `fork.engine_mut().process()` while routing effects to an
    /// `OwnedDocument` instead of the host. Text mutations are applied
    /// directly; cursor/selection updates are tracked; passthrough effects
    /// are accumulated for later delivery.
    ///
    /// Returns a [`ShadowResult`] containing:
    /// - A single diff-based text effect (if text changed)
    /// - Coalesced UI/state effects
    /// - The final cursor position
    /// - Whether the replay completed or was aborted
    pub(in crate::execution::engine) fn execute_shadow_replay(
        &mut self,
        initial_text: String,
        cursor: usize,
        selection: Option<SelectionRange>,
        cancel_check: Option<&dyn Fn() -> bool>,
    ) -> ShadowResult {
        // Acquire a ScopedFork — sets fork_active = true.
        // If forking fails (AlreadyForked), return an empty completed result.
        // This shouldn't happen in normal operation since shadow replay is
        // only triggered from the top-level process() which is not itself
        // inside a fork, but we handle it gracefully.
        let mut fork = match self.fork() {
            Ok(f) => f,
            Err(_) => {
                return ShadowResult {
                    text_effects: Vec::new(),
                    other_effects: Vec::new(),
                    final_cursor: None,
                    status: ShadowStatus::Completed,
                };
            }
        };

        let mut shadow_doc = OwnedDocument::new(initial_text);
        let original_text = shadow_doc.text().to_owned();
        let mut ctx = ShadowContext::new(cursor);
        if let Some(sel) = selection {
            ctx.set_selection(sel);
        }

        let mut aborted = false;
        let mut abort_reason = ShadowAbortReason::HostInteractionRequired;

        let engine = fork.engine_mut();
        while engine.has_pending_keys() {
            // Check external cancellation flag before processing each key.
            if cancel_check.is_some_and(|f| f()) {
                aborted = true;
                abort_reason = ShadowAbortReason::Cancelled;
                break;
            }

            let Some(output) = engine.drain_next_key() else {
                break;
            };

            match output {
                super::macro_replay::MacroOutput::Key(key) => {
                    let input_ctx = engine.build_shadow_input(&shadow_doc, &ctx);
                    let mut key_response = engine.process(key, input_ctx);

                    if !key_response.host_requests().is_empty() {
                        aborted = true;
                        break;
                    }

                    let effects = key_response.take_effects();
                    if Self::apply_shadow_effects(effects, &mut shadow_doc, &mut ctx) {
                        aborted = true;
                        abort_reason = ShadowAbortReason::HostInteractionRequired;
                        break;
                    }
                }
                super::macro_replay::MacroOutput::TextBlock {
                    text,
                    cursor_offset,
                } => {
                    // Apply text block directly to shadow document at current cursor.
                    let insert_pos = ctx.cursor();
                    shadow_doc.apply_insert(insert_pos, &text);
                    ctx.set_cursor(insert_pos + cursor_offset);
                }
            }
            ctx.increment_keys_processed();
        }

        let result =
            Self::build_shadow_result(&original_text, &shadow_doc, ctx, aborted, abort_reason);

        // Commit the fork — keeps the engine state mutations (e.g., register
        // updates, mode transitions) from the shadow replay and clears
        // fork_active. If the shadow was aborted, we still commit because
        // the keys that WERE processed should take effect (the engine already
        // consumed them from the typeahead/macro stack).
        fork.commit();

        result
    }

    /// Build a validated `InputContext` for the shadow document.
    ///
    /// Uses `validate_clamped()` to handle cursors that drift past the
    /// document end after text edits.
    fn build_shadow_input<'doc>(
        &self,
        shadow_doc: &'doc OwnedDocument,
        ctx: &ShadowContext,
    ) -> InputContext<'doc, OwnedDocument, crate::execution::Validated> {
        let input_ctx = InputContext::new(shadow_doc, ctx.cursor()).validate_clamped();
        if let Some(sel) = ctx.selection() {
            input_ctx.with_selection(sel)
        } else {
            input_ctx
        }
    }

    /// Route a batch of effects through shadow classification.
    ///
    /// Returns `true` if the loop should abort (a `HostRequired` effect
    /// was encountered).
    fn apply_shadow_effects(
        effects: Vec<Effect>,
        shadow_doc: &mut OwnedDocument,
        ctx: &mut ShadowContext,
    ) -> bool {
        for effect in effects {
            match classify_effect(&effect) {
                EffectCategory::ShadowApply => {
                    Self::apply_shadow_text_effect(&effect, shadow_doc);
                }
                EffectCategory::CursorUpdate => {
                    Self::apply_shadow_cursor_effect(&effect, ctx);
                }
                EffectCategory::PassThrough => {
                    ctx.push_deferred_effect(effect);
                }
                EffectCategory::Intercepted => {
                    // Already consumed by process_effects inside process().
                }
                EffectCategory::HostRequired => {
                    return true;
                }
            }
        }
        false
    }

    /// Apply a text-mutation effect to the shadow document.
    fn apply_shadow_text_effect(effect: &Effect, shadow_doc: &mut OwnedDocument) {
        match effect {
            Effect::Insert { offset, text } => {
                shadow_doc.apply_insert(offset.get(), text);
            }
            Effect::Delete { range } => {
                shadow_doc.apply_delete(range.start().get(), range.end().get());
            }
            Effect::Replace { range, text } => {
                shadow_doc.apply_replace(range.start().get(), range.end().get(), text);
            }
            _ => {}
        }
    }

    /// Apply a cursor/selection effect to the shadow context.
    const fn apply_shadow_cursor_effect(effect: &Effect, ctx: &mut ShadowContext) {
        match effect {
            Effect::SetCursor { offset } => {
                ctx.set_cursor(offset.get());
            }
            Effect::SetSelection { anchor, head, .. } => {
                ctx.set_selection(SelectionRange::new(*anchor, *head));
            }
            Effect::ClearSelection => {
                ctx.clear_selection();
            }
            _ => {}
        }
    }

    /// Assemble the final `ShadowResult` from loop state.
    fn build_shadow_result(
        original_text: &str,
        shadow_doc: &OwnedDocument,
        mut ctx: ShadowContext,
        aborted: bool,
        abort_reason: ShadowAbortReason,
    ) -> ShadowResult {
        let text_effects: Vec<Effect> = compute_diff(original_text, shadow_doc.text())
            .into_iter()
            .collect();
        let other_effects = coalesce_effects(ctx.take_deferred_effects());
        // Only emit final_cursor when shadow actually produced observable effects.
        // When all pending keys went Pending (mapping prefix), shadow is a no-op
        // and its cursor is the stale pre-processing value — emitting it would
        // undo the real key's cursor movement.
        let final_cursor = if text_effects.is_empty() && other_effects.is_empty() {
            None
        } else {
            Some(ctx.cursor())
        };

        let status = if aborted {
            ShadowStatus::Aborted {
                reason: abort_reason,
                keys_processed: ctx.keys_processed(),
            }
        } else {
            ShadowStatus::Completed
        };

        ShadowResult {
            text_effects,
            other_effects,
            final_cursor,
            status,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "shadow_tests.rs"]
mod tests;
