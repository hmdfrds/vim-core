//! NodeId-keyed undo store for host sessions.
//!
//! Stores ChangeSet pairs (forward + inverse) keyed by the engine's
//! undo-tree NodeId.  The engine directs all navigation -- the host never
//! decides which node to visit.
//!
//! # Checkpoint-based memory optimization
//!
//! Instead of storing full document text snapshots for every undo node
//! (O(H * 2N) memory), only every `CHECKPOINT_INTERVAL`-th node stores a
//! full `checkpoint_text_after` snapshot.  The changeset-based `apply()`
//! path handles the majority of undo/redo operations.  When the changeset
//! path fails (external edit desync), the fallback uses the checkpoint
//! snapshot if available; otherwise the step returns `None` (graceful
//! failure the caller already handles).
//!
//! For 10K edits on a 1MB document:
//! - Before: O(H * 2N) = ~20 GB (10K * 2 * 1 MB)
//! - After:  O(H/64 * N + H * E) ~ ~158 MB (157 checkpoints * 1 MB + changeset overhead)

use crate::primitives::changeset::{ChangeSet, TextOp};
use crate::primitives::{NodeId, Offset};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How often to store a full document text snapshot for desync fallback.
///
/// Every `CHECKPOINT_INTERVAL`-th committed node stores `checkpoint_text_after`.
/// Trade-off: lower = more memory, higher = more desync-fallback failures.
/// 64 provides ~126x memory reduction while keeping desync recovery available
/// for recent nodes.
const CHECKPOINT_INTERVAL: u64 = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of an undo or redo step, carrying the new text, cursor position,
/// and the [`TextOp`] sequence that was applied.
///
/// Consumers use `ops` to derive byte-level [`EditOp`]s for host-side
/// document synchronisation (via `changeset_to_edit_ops`).
#[derive(Debug)]
pub struct UndoResult {
    /// The document text after applying the undo/redo step.
    pub text: String,
    /// The cursor byte offset to restore.
    pub cursor: Offset,
    /// The [`TextOp`] sequence that was applied (inverse for undo, forward
    /// for redo).
    pub ops: Vec<TextOp>,
}

/// Pending snapshot captured at `begin_group`.
#[derive(Debug)]
struct PendingSnapshot {
    text_before: String,
    cursor_before: Offset,
}

/// Committed snapshot: forward + inverse ChangeSet for one undo group.
///
/// Stores both changesets (for efficient apply when the document matches)
/// and an optional full text snapshot at checkpoint intervals (for fallback
/// when external edits have desynchronised the document length from the
/// changeset's `input_len`).
#[derive(Debug)]
struct UndoSnapshot {
    forward: ChangeSet,
    inverse: ChangeSet,
    /// Full document text after the mutation, stored only at checkpoint
    /// intervals (every `CHECKPOINT_INTERVAL`-th node). Used as desync
    /// fallback for redo.  For undo desync fallback, the inverse changeset
    /// is applied to this checkpoint text.
    checkpoint_text_after: Option<String>,
    /// Monotonic sequence number assigned at commit time.
    #[allow(
        dead_code,
        reason = "assigned from next_sequence at commit; the checkpoint-interval test uses the local counter before the snapshot is built, so the stored copy is only ever seen in Debug output"
    )]
    sequence: u64,
    cursor_before: Offset,
    cursor_after: Offset,
}

// ---------------------------------------------------------------------------
// UndoStore
// ---------------------------------------------------------------------------

/// NodeId-keyed undo store.
///
/// `begin_group` captures text+cursor before mutation.
/// `end_group` commits the snapshot under the engine-assigned NodeId.
/// `undo_step`/`redo_step` look up snapshots by NodeId.
#[derive(Debug)]
pub struct UndoStore {
    groups: HashMap<NodeId, UndoSnapshot>,
    pending: Option<PendingSnapshot>,
    /// Monotonic sequence counter, incremented on each committed group.
    /// Used to determine checkpoint intervals.
    next_sequence: u64,
    /// Number of times `undo_step` used the checkpoint fallback path.
    /// Non-zero during tests indicates a structural undo bug being masked.
    #[cfg(feature = "testing")]
    pub(crate) checkpoint_fallback_count: u32,
}

impl UndoStore {
    /// Create an empty undo store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            pending: None,
            next_sequence: 0,
            #[cfg(feature = "testing")]
            checkpoint_fallback_count: 0,
        }
    }

    /// Begin an undo group, capturing current state.
    /// If a group is already open, this is a no-op.
    pub fn begin_group(&mut self, text: &str, cursor: Offset) {
        if self.pending.is_none() {
            self.pending = Some(PendingSnapshot {
                text_before: text.to_owned(),
                cursor_before: cursor,
            });
        }
    }

    /// Commit the pending undo group under the given NodeId.
    /// `None` means the engine deemed the group empty -- discard pending.
    ///
    /// Only stores a full `checkpoint_text_after` snapshot every
    /// `CHECKPOINT_INTERVAL`-th commit, reducing memory from O(H * 2N)
    /// to O(H/64 * N + H * E).
    ///
    /// # Panics
    ///
    /// Panics if `ChangeSet::invert` fails, which should never happen since
    /// the forward changeset was just built from `from_diff` with matching
    /// input lengths.
    pub fn end_group(&mut self, node_id: Option<NodeId>, text_after: &str, cursor_after: Offset) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let Some(node_id) = node_id else {
            return;
        };
        let forward = ChangeSet::from_diff(&pending.text_before, text_after);
        #[allow(
            clippy::expect_used,
            reason = "ChangeSet::from_diff produces a changeset whose input_len == original.len(), so invert() cannot fail"
        )]
        let inverse = forward
            .invert(&pending.text_before)
            .expect("invert of from_diff: lengths guaranteed to match");

        let sequence = self.next_sequence;
        self.next_sequence += 1;

        // Only store full text snapshot at checkpoint intervals.
        let checkpoint_text_after = if sequence.is_multiple_of(CHECKPOINT_INTERVAL) {
            Some(text_after.to_owned())
        } else {
            None
        };

        self.groups.insert(
            node_id,
            UndoSnapshot {
                forward,
                inverse,
                checkpoint_text_after,
                sequence,
                cursor_before: pending.cursor_before,
                cursor_after,
            },
        );
    }

    /// Apply the inverse ChangeSet to recover the before-state for undoing a node.
    ///
    /// Returns an `UndoResult` containing the restored text, cursor position,
    /// and the inverse [`TextOp`] sequence that was applied.
    ///
    /// When the document has been modified by external edits (changing its
    /// length), the changeset's `apply()` will fail with a length mismatch.
    /// In that case, if this node has a checkpoint snapshot, we apply the
    /// inverse changeset to the checkpoint text to recover the before-state.
    /// If no checkpoint is available, returns `None` (graceful failure --
    /// the caller already handles `None`).
    pub fn undo_step(&mut self, node_id: NodeId, current_text: &str) -> Option<UndoResult> {
        let snap = self.groups.get(&node_id)?;
        if let Ok(text) = snap.inverse.apply(current_text) {
            let ops = snap.inverse.ops().to_vec();
            Some(UndoResult {
                text,
                cursor: snap.cursor_before,
                ops,
            })
        } else {
            // External edit desynchronised the document -- try checkpoint fallback.
            // If this node has a checkpoint, apply the inverse to it to derive
            // the before-state. Otherwise return None (undo fails gracefully).
            let checkpoint = snap.checkpoint_text_after.as_ref()?;
            if let Ok(text_before) = snap.inverse.apply(checkpoint) {
                #[cfg(feature = "testing")]
                {
                    self.checkpoint_fallback_count += 1;
                }
                let fallback = ChangeSet::from_diff(current_text, &text_before);
                let ops = fallback.ops().to_vec();
                Some(UndoResult {
                    text: text_before,
                    cursor: snap.cursor_before,
                    ops,
                })
            } else {
                // Inverse apply to checkpoint also failed -- should not happen
                // since the checkpoint was captured at commit time with matching
                // lengths, but handle gracefully.
                None
            }
        }
    }

    /// Number of times `undo_step` used the checkpoint fallback path.
    #[cfg(feature = "testing")]
    #[must_use]
    pub const fn checkpoint_fallback_count(&self) -> u32 {
        self.checkpoint_fallback_count
    }

    /// Removes undo snapshots for pruned tree nodes, freeing their document text.
    ///
    /// Called after `UndoTree::prune()` to keep the store synchronized with the
    /// tree. Calls `shrink_to_fit` after bulk removal to release HashMap memory.
    pub fn remove_pruned(&mut self, pruned_ids: &[NodeId]) {
        for &id in pruned_ids {
            self.groups.remove(&id);
        }
        if pruned_ids.len() > 16 {
            self.groups.shrink_to_fit();
        }
    }

    /// Apply the forward ChangeSet to recover the after-state for redoing a node.
    ///
    /// Returns an `UndoResult` containing the restored text, cursor position,
    /// and the forward [`TextOp`] sequence that was applied.
    ///
    /// Falls back to the stored `checkpoint_text_after` snapshot when the
    /// document has been modified by external edits (same rationale as
    /// [`Self::undo_step`]). Returns `None` if no checkpoint is available.
    #[must_use]
    pub fn redo_step(&self, node_id: NodeId, current_text: &str) -> Option<UndoResult> {
        let snap = self.groups.get(&node_id)?;
        if let Ok(text) = snap.forward.apply(current_text) {
            let ops = snap.forward.ops().to_vec();
            Some(UndoResult {
                text,
                cursor: snap.cursor_after,
                ops,
            })
        } else {
            // External edit desynchronised the document -- try checkpoint fallback.
            // If this node has a checkpoint, use it directly as the after-state.
            // Otherwise return None (redo fails gracefully).
            let checkpoint = snap.checkpoint_text_after.as_ref()?;
            let fallback = ChangeSet::from_diff(current_text, checkpoint);
            let ops = fallback.ops().to_vec();
            Some(UndoResult {
                text: checkpoint.clone(),
                cursor: snap.cursor_after,
                ops,
            })
        }
    }
}

impl Default for UndoStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "undo_stack_tests.rs"]
mod tests;
