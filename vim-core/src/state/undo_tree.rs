//! Branch-aware undo tree.
//!
//! Tracks undo/redo history as a tree rather than a linear stack. When edits
//! are made after undoing, a new branch is forked instead of discarding the
//! old redo history.
//!
//! This module stores **metadata only** — actual text undo/redo is delegated
//! to the host via effects. The tree enables:
//!
//! - Branch-aware navigation (redo follows the last-visited branch)
//! - Time-based navigation (`:earlier` / `:later`)
//! - Tree visualization (`:undolist`)
//! - State queries (`can_undo`, `can_redo`, `depth`, `branch_count`)
//!
//! Node access uses arena indexing with [`NodeId`] values. Pruned slots
//! are reused via a free list, so `NodeId` values may be recycled.
use super::mark_snapshot::MarkSnapshot;
use super::Marks;
use crate::primitives::byte_delta;
use crate::primitives::{LastVisualInfo, Mode};
use smallvec::SmallVec;

// Re-export visualization types from primitives (canonical definitions live
// there so that `effects/` can reference them without a state→effects cycle).
pub use crate::primitives::{NodeId, Offset, UndoTreeNodeView, UndoTreeSnapshot};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Read-only snapshot of a node's metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeInfo {
    /// Node identifier.
    id: NodeId,
    /// Parent node (`None` for root).
    parent: Option<NodeId>,
    /// Number of child branches.
    child_count: usize,
    /// Monotonic sequence number (chronological ordering across the tree).
    sequence: u64,
    /// Cursor position before this change group.
    cursor_before: Offset,
    /// Cursor position after this change group.
    cursor_after: Offset,
    /// Byte offset of the first text edit in this group.
    first_edit_offset: Option<Offset>,
    /// Timestamp when committed (caller-defined time unit, typically seconds).
    timestamp: u64,
    /// Depth from root (root = 0).
    depth: u32,
}

impl NodeInfo {
    /// Node identifier.
    #[inline]
    #[must_use]
    pub const fn id(&self) -> NodeId {
        self.id
    }

    /// Parent node (`None` for root).
    #[inline]
    #[must_use]
    pub const fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// Number of child branches.
    #[inline]
    #[must_use]
    pub const fn child_count(&self) -> usize {
        self.child_count
    }

    /// Monotonic sequence number (chronological ordering across the tree).
    #[inline]
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Cursor position before this change group.
    #[inline]
    #[must_use]
    pub const fn cursor_before(&self) -> Offset {
        self.cursor_before
    }

    /// Cursor position after this change group.
    #[inline]
    #[must_use]
    pub const fn cursor_after(&self) -> Offset {
        self.cursor_after
    }

    /// Byte offset of the first text edit in this group.
    #[inline]
    #[must_use]
    pub const fn first_edit_offset(&self) -> Option<Offset> {
        self.first_edit_offset
    }

    /// Timestamp when committed (caller-defined time unit, typically seconds).
    #[inline]
    #[must_use]
    pub const fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Depth from root (root = 0).
    #[inline]
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }
}

/// Result of an undo or redo navigation step.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UndoStep {
    /// Node we navigated to.
    node: NodeId,
    /// Suggested cursor position(s) after navigation.
    ///
    /// Single-element for normal undo; multiple elements when `multi-cursor`
    /// feature is enabled and the undo group was created with multiple cursors.
    cursors: SmallVec<[Offset; 1]>,
    /// Line-start offset for `'[` mark after this undo/redo step.
    ///
    /// Set to the start of the first changed line. `None` if the undo
    /// group had no edits with a known offset.
    change_mark_start: Option<Offset>,
    /// Line-start offset for `']` mark after this undo/redo step.
    ///
    /// Set to the start of the last changed line. `None` if the undo
    /// group had no edits with a known offset.
    change_mark_end: Option<Offset>,
    /// Raw byte offset of the first edit in the undo group.
    ///
    /// Used for mark `'.'` after undo/redo. Unlike `change_mark_start`
    /// (line-start), this is the exact byte position, matching Neovim's
    /// behavior where mark `'.'` points to the edit position itself.
    first_edit_offset: Option<Offset>,
    /// Line-start offset for mark `'.'` after undo/redo.
    ///
    /// Precomputed using `logical_edit_line_start` which skips a leading
    /// `\n` byte. This differs from `change_mark_start` when a linewise
    /// delete consumed the preceding newline separator.
    mark_dot: Option<Offset>,
    /// End of the last edit in the original (T0) text.
    ///
    /// Used together with `first_edit_offset` and `edit_delta` to construct
    /// a `ChangeSet` for `remap_all_positions` after undo/redo.
    last_edit_end: Option<Offset>,
    /// Net byte change of this undo group (new_len - old_len, summed).
    ///
    /// Used to construct a `ChangeSet` for `remap_all_positions` after
    /// undo/redo so that jumplist byte offsets track text changes.
    edit_delta: i32,
    /// Mode that was active when this undo group was created.
    ///
    /// Allows the caller to differentiate cursor/selection restoration
    /// between visual-line, visual-block, visual-char, etc.
    mode: Mode,
    /// Visual info from the undo node being navigated to/from.
    /// Swapped with the engine's `last_visual` so `gv` restores the
    /// visual selection active at the time of the undone/redone edit.
    last_visual: Option<LastVisualInfo>,
    /// True when the node we navigated to is a save point (or the root).
    /// The caller can use this to clear the buffer modified indicator.
    is_at_save_point: bool,
}

impl UndoStep {
    /// Node we navigated to.
    #[inline]
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Suggested cursor position after navigation (first/primary cursor).
    ///
    /// For backward compatibility, returns the first cursor position (index 0).
    /// Use [`cursors()`](Self::cursors) for multi-cursor access.
    #[inline]
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "cursors is always non-empty — constructed from &[cursor_before] or SmallVec::from_slice with at least one element"
    )]
    pub fn cursor(&self) -> Offset {
        self.cursors[0]
    }

    /// All cursor positions after navigation.
    ///
    /// Returns a single-element slice for normal undo. When the `multi-cursor`
    /// feature is enabled and the undo group was created with multiple cursors,
    /// returns all stored positions.
    #[inline]
    #[must_use]
    pub fn cursors(&self) -> &[Offset] {
        &self.cursors
    }

    /// Line-start offset for `'[` mark after this undo/redo step.
    #[inline]
    #[must_use]
    pub const fn change_mark_start(&self) -> Option<Offset> {
        self.change_mark_start
    }

    /// Line-start offset for `']` mark after this undo/redo step.
    #[inline]
    #[must_use]
    pub const fn change_mark_end(&self) -> Option<Offset> {
        self.change_mark_end
    }

    /// Raw byte offset of the first edit for mark `'.'`.
    #[inline]
    #[must_use]
    pub const fn first_edit_offset(&self) -> Option<Offset> {
        self.first_edit_offset
    }

    /// Precomputed line-start for mark `'.'` after undo/redo.
    ///
    /// Differs from `change_mark_start` when a linewise delete consumed
    /// the preceding `\n` separator.
    #[inline]
    #[must_use]
    pub const fn mark_dot(&self) -> Option<Offset> {
        self.mark_dot
    }

    /// End of the last edit in the original (T0) text.
    #[inline]
    #[must_use]
    pub const fn last_edit_end(&self) -> Option<Offset> {
        self.last_edit_end
    }

    /// Net byte change of this undo group.
    #[inline]
    #[must_use]
    pub const fn edit_delta(&self) -> i32 {
        self.edit_delta
    }

    /// Mode that was active when this undo group was created.
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Visual info from the undo node for `gv` restoration.
    #[inline]
    #[must_use]
    pub const fn last_visual(&self) -> Option<LastVisualInfo> {
        self.last_visual
    }

    /// Whether the node we navigated to is a save point.
    #[inline]
    #[must_use]
    pub const fn is_at_save_point(&self) -> bool {
        self.is_at_save_point
    }
}

/// Leaf node summary for `:undolist` display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LeafInfo {
    /// Leaf node identifier.
    node: NodeId,
    /// Sequence number.
    sequence: u64,
    /// Timestamp when committed.
    timestamp: u64,
    /// Depth from root (number of changes to reach this leaf).
    depth: u32,
}

impl LeafInfo {
    /// Leaf node identifier.
    #[inline]
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Sequence number.
    #[inline]
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Timestamp when committed.
    #[inline]
    #[must_use]
    pub const fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Depth from root (number of changes to reach this leaf).
    #[inline]
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct UndoNode {
    parent: Option<NodeId>,
    children: SmallVec<[NodeId; 2]>,
    /// Monotonic sequence number (1-based; root is 0).
    sequence: u64,
    /// Cursor position(s) before this change group.
    ///
    /// `SmallVec<[Offset; 1]>` stores one cursor inline (zero heap) for the
    /// common single-cursor case. Multiple entries when `multi-cursor` is enabled.
    cursors_before: SmallVec<[Offset; 1]>,
    cursor_after: Offset,
    /// Minimum edit offset within the group (for undo cursor placement).
    first_edit_offset: Option<Offset>,
    /// Maximum end-of-edit offset within the group (for undo `']` mark).
    ///
    /// For Delete/Replace: `offset + old_len`. For Insert: `offset`.
    /// Represents the furthest extent of the change in the *original* text.
    /// Consumed at `end_group()` time to compute `undo_mark_end`.
    #[allow(dead_code, reason = "consumed at end_group() to compute undo_mark_end")]
    last_edit_end: Option<Offset>,
    /// Net byte change of this undo group (new_len - old_len, summed).
    ///
    /// Used to construct a `ChangeSet` for `remap_all_positions` after
    /// undo/redo so that jumplist byte offsets are adjusted through text
    /// changes that undo/redo applies.
    edit_delta: i32,
    /// Caller-provided timestamp.
    timestamp: u64,
    /// Depth from root (root = 0, first change = 1, ...).
    depth: u32,
    /// Strategy for cursor placement after undoing this group.
    cursor_strategy: crate::primitives::UndoCursorStrategy,
    /// Index into `children` for the preferred redo direction.
    /// Updated by `undo()` to remember the branch we came from.
    preferred_child_idx: Option<u16>,
    /// Snapshot of local marks (a-z) at the time this undo group was created.
    /// Swapped with live marks on undo/redo (Neovim `uh_namedm` pattern).
    marks: MarkSnapshot,
    /// Mode that was active when this undo group was created.
    mode: Mode,
    /// Visual info saved when this undo group was created.
    /// Swapped with engine's `last_visual` on undo/redo so `gv` restores
    /// the visual selection that was active at the time of the edit.
    last_visual: Option<LastVisualInfo>,
    /// Whether the buffer was modified (had unsaved changes) at group creation.
    /// Used to detect undo-to-save-point for clearing the modified indicator.
    #[allow(
        dead_code,
        reason = "copied from the pending group when the node is sealed, but never read back: undo/redo derive UndoStep::is_at_save_point from save_nr instead. Kept so a node records the modified state it was created under"
    )]
    was_changed: bool,
    /// Tombstone flag: node has been pruned due to `undolevels` limit.
    ///
    /// Pruned nodes are marked dead and skipped during traversal.
    /// Their slots are added to the free list for reuse by future nodes.
    /// The root node (index 0) is NEVER pruned.
    pruned: bool,
    /// File-save counter at the time this node was marked as a save point.
    ///
    /// `0` means this node has not been associated with a file save.
    /// Non-zero values are assigned by `mark_save()` and correspond to
    /// `UndoTree::save_counter` at the time of the write.
    save_nr: u32,
    /// Line-start offset of the first changed line in the *original* text
    /// (T0, before the undo group's edits). Used for `'[` mark after undo.
    undo_mark_start: Option<Offset>,
    /// Line-start offset of the last changed line in the *original* text
    /// (T0). Used for `']` mark after undo. Also used as the best
    /// approximation for `']` after redo (T0-based, since T1 text is
    /// not available at end_group time for single-response undo groups).
    undo_mark_end: Option<Offset>,
    /// Line-start offset for mark `'.'` after undo/redo.
    ///
    /// Differs from `undo_mark_start` when a linewise delete consumes
    /// the preceding `\n` separator: `undo_mark_start` gives the line
    /// containing the `\n` (the line above), while `undo_mark_dot` skips
    /// the `\n` to point to the deleted line itself. Matches Neovim's
    /// `changed_lines(first_lnum)` which uses the deleted line number.
    undo_mark_dot: Option<Offset>,
    /// Override for mark `'.'` after redo, when it differs from `undo_mark_dot`.
    ///
    /// For block visual insert, redo `'.'` should be the start of the first
    /// secondary line in the post-edit (T1) text, while undo `'.'` is the
    /// start of the first changed line in the original (T0) text.
    redo_mark_dot: Option<Offset>,
    /// Override for mark `']'` after redo, when it differs from `undo_mark_end`.
    ///
    /// For block visual insert, redo `']'` should be the last changed line
    /// start in the post-edit (T1) text.
    redo_mark_end: Option<Offset>,
}

#[derive(Debug, Clone)]
struct PendingGroup {
    cursors_before: SmallVec<[Offset; 1]>,
    has_edits: bool,
    first_edit_offset: Option<Offset>,
    /// Maximum end-of-edit offset within the group (for undo `']` mark).
    last_edit_end: Option<Offset>,
    /// Maximum end-of-insert offset (offset + inserted_bytes) in the T1
    /// (post-edit) text. Used to compute `undo_mark_end` when only inserts
    /// occurred (no deletes/replaces, so `last_edit_end` is `None`).
    last_insert_end_t1: Option<Offset>,
    /// Precomputed line-start offset for `first_edit_offset` in T0 text.
    /// Set by `mark_edit_range_with_text`/`mark_edit_at_with_text` when
    /// the T0 text is available. Avoids incorrect computation when
    /// `end_group` receives T1 text.
    first_edit_line_start: Option<Offset>,
    /// Precomputed line-start offset for `last_edit_end` in T0 text.
    last_edit_end_line_start: Option<Offset>,
    /// Length of the T0 (pre-edit) document text at `begin_group` time.
    /// Used to clamp `last_insert_end_t1` when the `text` parameter at
    /// `end_group` time may be T1 (post-edit) rather than T0.
    t0_text_len: Option<usize>,
    /// True if any Insert or Replace effect was recorded (not pure delete).
    /// Used to determine whether `logical_edit_line_start` should skip a
    /// leading `\n` for undo_mark_dot: only pure deletes get the skip.
    has_insert_or_replace: bool,
    cursor_strategy: crate::primitives::UndoCursorStrategy,
    /// Snapshot of local marks captured at group creation time.
    marks: MarkSnapshot,
    /// Mode that was active when this undo group was created.
    mode: Mode,
    /// Visual info at group creation time for `gv` restoration on undo/redo.
    last_visual: Option<LastVisualInfo>,
    /// Whether the buffer was modified (had unsaved changes) at group creation.
    was_changed: bool,
    /// Net byte change across all edits in this group.
    ///
    /// Accumulated by `accumulate_delta()`: positive for net insertions,
    /// negative for net deletions. Used after undo/redo to construct a
    /// `ChangeSet` for `remap_all_positions` so that jumplist entries
    /// are adjusted through undo text changes.
    edit_delta: i32,
    /// Externally precomputed undo mark end (line-start in T0 text).
    ///
    /// When set, `end_group()` uses this directly for `undo_mark_end`
    /// instead of computing from `last_edit_end` / `last_insert_end_t1`.
    /// Used for block visual insert groups where the T0 line-start cannot
    /// be correctly computed at `end_group()` time because the `text`
    /// parameter is T1 (post-primary-insert) rather than true T0.
    precomputed_undo_mark_end: Option<Offset>,
    /// Externally precomputed redo mark dot (line-start in T1 text).
    ///
    /// For block insert redo, `'.'` should point to the start of the first
    /// secondary line in the post-edit text, which differs from the T0-based
    /// `undo_mark_dot`. When set, `redo()` uses this instead of `undo_mark_dot`.
    precomputed_redo_mark_dot: Option<Offset>,
    /// Externally precomputed redo mark end (line-start in T1 text).
    ///
    /// For block insert redo, `']'` should point to the last changed line
    /// in the post-edit text, which differs from the T0-based `undo_mark_end`.
    /// When set, `redo()` uses this instead of `undo_mark_end`.
    precomputed_redo_mark_end: Option<Offset>,
}

// ---------------------------------------------------------------------------
// UndoTree
// ---------------------------------------------------------------------------

/// Branch-aware undo tree.
///
/// Starts with a single root node representing the initial document state.
/// Each committed undo group creates a new child node. When edits are made
/// after undoing (current node is not a leaf on the preferred path), a new
/// branch is forked from the current position.
///
/// # Navigation model
///
/// - **Undo** moves to the parent node and sets the parent's preferred child
///   to the node we came from (so redo returns to it).
/// - **Redo** moves to the preferred child.
/// - **`:earlier N`** undoes N changes along the ancestor path.
/// - **`:earlier Ns`** undoes until finding a node at or before T−N seconds.
/// - **`:later`** is the dual of `:earlier`, following the preferred path.
///
/// # Metadata only
///
/// The tree does **not** store effect data or document snapshots. The host
/// is responsible for executing the actual undo/redo operations. The tree
/// tells the engine *how many* undo/redo steps to emit.
///
/// # Pruning (undolevels)
///
/// When `undolevels` is set, the oldest leaf branches are tombstoned via
/// [`prune`](Self::prune) after each committed group. Pruned nodes are marked
/// with `pruned: bool` and are invisible to callers. Pruned slots are added
/// to a free list and reused by subsequent [`end_group`](Self::end_group)
/// calls, so the arena converges to O(undolevels) rather than growing
/// monotonically.
#[derive(Debug, Clone)]
pub struct UndoTree {
    /// Arena of all nodes. Index 0 is always the root.
    nodes: Vec<UndoNode>,
    /// Currently active node.
    current: NodeId,
    /// Next sequence number to assign.
    next_sequence: u64,
    /// In-flight undo group (between begin/end).
    pending: Option<PendingGroup>,
    /// Count of live (non-pruned, non-root) nodes.
    ///
    /// Incremented by `end_group()` when a node is committed.
    /// Decremented by `prune()` when nodes are tombstoned.
    /// `change_count()` returns this value.
    live_count: usize,
    /// Monotonic counter incremented by each `mark_save()` call.
    ///
    /// Used for `:earlier Nf` / `:later Nf` navigation by file-save count.
    save_counter: u32,
    /// NodeIds pruned during prune() calls, drained by `VimSession` to sync UndoStore.
    pruned_since_last_drain: Vec<NodeId>,
    /// Indices of pruned slots available for reuse.
    /// The generational-handle pattern: an index paired with a generation
    /// counter, so a stale handle is detected rather than silently aliasing
    /// whatever now occupies its slot.
    free_list: Vec<u32>,
    /// Whether undo group merging is active (macro replay).
    ///
    /// When true, only the first `begin_group` creates a pending group.
    /// Subsequent `begin_group`/`end_group` pairs are tracked via
    /// `merge_depth` but don't create or commit groups. The merged
    /// group is committed when `end_merge()` is called.
    merging: bool,
    /// Nesting depth of begin/end pairs within a merge region.
    ///
    /// Starts at 0. Incremented by `begin_group`, decremented by `end_group`.
    /// Only the transition from 0→1 creates a real group; only the
    /// transition to 0 after `end_merge()` commits it.
    merge_depth: u32,
    /// Set after undo/redo, cleared on begin_group. Used by `:undojoin`
    /// to reject E790 ("undojoin is not allowed after undo").
    last_was_undo: bool,
}

impl Default for UndoTree {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoTree {
    /// Create an empty undo tree with only the root node.
    #[must_use]
    pub fn new() -> Self {
        let root = UndoNode {
            parent: None,
            children: SmallVec::new(),
            sequence: 0,
            cursors_before: smallvec::smallvec![Offset::ZERO],
            cursor_after: Offset::ZERO,
            first_edit_offset: None,
            last_edit_end: None,
            edit_delta: 0,
            timestamp: 0,
            depth: 0,
            cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            preferred_child_idx: None,
            marks: MarkSnapshot::new(),
            mode: Mode::Normal,
            last_visual: None,
            was_changed: false,
            pruned: false,
            save_nr: 0,
            undo_mark_start: None,
            undo_mark_end: None,
            undo_mark_dot: None,
            redo_mark_dot: None,
            redo_mark_end: None,
        };
        Self {
            nodes: vec![root],
            current: NodeId::ROOT,
            next_sequence: 1,
            pending: None,
            live_count: 0,
            save_counter: 0,
            pruned_since_last_drain: Vec::new(),
            free_list: Vec::new(),
            merging: false,
            merge_depth: 0,
            last_was_undo: false,
        }
    }

    /// Reset the undo tree to a single root node.
    ///
    /// Called after `syncText` replaces the document wholesale. All existing
    /// undo entries reference pre-sync text and would produce garbage if
    /// replayed. Fencing prevents undo from crossing the sync boundary.
    pub fn fence(&mut self) {
        *self = Self::new();
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Group merge (macro replay)
    // ═══════════════════════════════════════════════════════════════════════

    /// Enable undo group merging.
    ///
    /// While merging is active, the first `begin_group` creates a pending
    /// group as normal. All subsequent `begin_group`/`end_group` pairs
    /// are suppressed — the pending group stays open across all of them.
    ///
    /// In Vim, `N@a` (macro replay with count) produces a single undo entry
    /// for the entire replay. This method enables that behavior.
    /// Whether undo group merging is currently active.
    #[inline]
    #[must_use]
    pub const fn is_merging(&self) -> bool {
        self.merging
    }

    /// Whether the last operation was undo/redo. Used by `:undojoin` to
    /// reject E790. Cleared on `begin_group` (next text mutation).
    #[inline]
    #[must_use]
    pub const fn last_was_undo(&self) -> bool {
        self.last_was_undo
    }

    pub(crate) const fn begin_merge(&mut self) {
        self.merging = true;
        self.merge_depth = 0;
    }

    /// Disable undo group merging and commit the merged pending group.
    ///
    /// Called when macro replay ends. Commits the pending group that was
    /// kept open during the merge, producing a single undo entry for
    /// the entire macro replay.
    pub(crate) fn end_merge(
        &mut self,
        cursor_after: Offset,
        timestamp: u64,
        text: Option<&str>,
    ) -> Option<NodeId> {
        let was_merging = self.merging;
        self.merging = false;
        self.merge_depth = 0;
        // Only commit the pending group if we were actually merging.
        // Normal (non-macro) calls to end_merge are no-ops.
        if was_merging && self.pending.is_some() {
            self.end_group(cursor_after, timestamp, text)
        } else {
            None
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // External edit undo isolation
    // ═══════════════════════════════════════════════════════════════════════

    /// Begin an undo group for an external edit, force-breaking any active merge.
    ///
    /// During macro replay, all undo groups are merged into one via
    /// `begin_merge()`/`end_merge()`. External edits (e.g. LSP refactors,
    /// collaborator changes) should NOT be absorbed into the macro's merged
    /// group — the user must be able to undo them independently.
    ///
    /// Returns `true` if a merge was active and was broken. The caller should
    /// call `begin_merge()` again after `end_group()` to resume merging for
    /// remaining macro keys.
    /// Returns `(broke_merge, force_committed_node)`.
    ///
    /// `force_committed_node` is `Some(NodeId)` when an existing pending group
    /// (e.g. from an active INSERT session) was committed to make room for the
    /// external edit. The caller MUST propagate this to the `UndoStore` so the
    /// two systems stay in sync.
    pub(crate) fn begin_external_group(
        &mut self,
        cursor_before: Offset,
        marks: MarkSnapshot,
        t0_text_len: Option<usize>,
        timestamp: u64,
    ) -> (bool, Option<NodeId>) {
        if self.merging {
            #[allow(
                clippy::indexing_slicing,
                reason = "cursors_before is always non-empty"
            )]
            let commit_cursor = self
                .pending
                .as_ref()
                .map_or(cursor_before, |p| p.cursors_before[0]);
            self.end_merge(commit_cursor, timestamp, None);

            self.begin_group(
                cursor_before,
                crate::primitives::UndoCursorStrategy::FirstEdit,
                marks,
                t0_text_len,
                Mode::Normal,
                None,
                false,
            );
            (true, None)
        } else {
            let force_committed = if self.pending.is_some() {
                self.end_group(cursor_before, timestamp, None)
            } else {
                None
            };
            self.begin_group(
                cursor_before,
                crate::primitives::UndoCursorStrategy::FirstEdit,
                marks,
                t0_text_len,
                Mode::Normal,
                None,
                false,
            );
            (false, force_committed)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Group lifecycle
    // ═══════════════════════════════════════════════════════════════════════

    /// Begin a new undo group.
    ///
    /// Call when processing `BeginUndoGroup`. The group is not committed
    /// until [`end_group`](Self::end_group) is called. If a group is
    /// already pending, it is silently replaced (defensive).
    ///
    /// `cursor_strategy`: controls where the cursor lands after undoing
    /// this group — `FirstEdit` for the default behaviour, `EntryPosition`
    /// to always restore `cursor_before` (for `o`/`O`, `p`/`P`).
    ///
    /// `marks`: snapshot of local marks (a-z) at group creation time,
    /// stored in the undo node for swap on undo/redo.
    ///
    /// # Complexity
    ///
    /// Time: O(1)
    ///
    /// Space: O(1) — stores a fixed-size `PendingGroup` (includes a
    /// 26-element mark snapshot array).
    pub(crate) fn begin_group(
        &mut self,
        cursor_before: Offset,
        cursor_strategy: crate::primitives::UndoCursorStrategy,
        marks: MarkSnapshot,
        t0_text_len: Option<usize>,
        mode: Mode,
        last_visual: Option<LastVisualInfo>,
        was_changed: bool,
    ) {
        self.begin_group_multi(
            &[cursor_before],
            cursor_strategy,
            marks,
            t0_text_len,
            mode,
            last_visual,
            was_changed,
        );
    }

    /// Begin a new undo group with multiple cursor positions.
    ///
    /// Like [`begin_group`](Self::begin_group) but stores all cursor positions
    /// so that multi-cursor undo can restore them. When the `multi-cursor`
    /// feature is disabled, callers should pass a single-element slice (the
    /// `SmallVec` inline storage avoids heap allocation for that case).
    pub(crate) fn begin_group_multi(
        &mut self,
        cursors_before: &[Offset],
        cursor_strategy: crate::primitives::UndoCursorStrategy,
        marks: MarkSnapshot,
        t0_text_len: Option<usize>,
        mode: Mode,
        last_visual: Option<LastVisualInfo>,
        was_changed: bool,
    ) {
        self.last_was_undo = false;

        if self.merging {
            self.merge_depth += 1;
            if self.merge_depth > 1 {
                return;
            }
        }
        self.pending = Some(PendingGroup {
            cursors_before: SmallVec::from_slice(cursors_before),
            has_edits: false,
            first_edit_offset: None,
            last_edit_end: None,
            last_insert_end_t1: None,
            first_edit_line_start: None,
            last_edit_end_line_start: None,
            t0_text_len,
            has_insert_or_replace: false,
            cursor_strategy,
            marks,
            mode,
            last_visual,
            was_changed,
            edit_delta: 0,
            precomputed_undo_mark_end: None,
            precomputed_redo_mark_dot: None,
            precomputed_redo_mark_end: None,
        });
    }

    /// Record that a text edit occurred at `offset` within the current group.
    ///
    /// Tracks the minimum offset for undo cursor placement. Also marks
    /// the group as non-empty so it will be committed.
    ///
    /// Only updates `first_edit_offset` (minimum). For `last_edit_end`
    /// tracking (needed for undo `']` mark), use [`Self::mark_edit_range`]
    /// which accounts for the byte extent of delete/replace operations.
    /// Insert operations (zero `old_len`) only contribute to `first_edit_offset`.
    pub const fn mark_edit_at(&mut self, offset: Offset) {
        if let Some(ref mut p) = self.pending {
            p.has_edits = true;
            p.first_edit_offset = Some(match p.first_edit_offset {
                None => offset,
                Some(existing) => existing.min(offset),
            });
        }
    }

    /// Like [`Self::mark_edit_at`] but also precomputes the T0 line-start offset.
    pub fn mark_edit_at_with_text(&mut self, offset: Offset, t0_text: &str) {
        self.mark_edit_at(offset);
        if let Some(ref mut p) = self.pending {
            let ls = Offset::new(line_start_for_offset(Some(t0_text), offset.get()));
            p.first_edit_line_start = Some(match p.first_edit_line_start {
                None => ls,
                Some(existing) => existing.min(ls),
            });
        }
    }

    /// Record an insert operation's extent in the T1 (post-edit) text.
    ///
    /// `offset`: the byte position where text was inserted.
    /// `insert_len`: the number of bytes inserted.
    ///
    /// Tracks the maximum `offset + insert_len` so that `end_group` can
    /// compute `undo_mark_end` for insert-only undo groups. For
    /// delete/replace groups, `last_edit_end` (from `mark_edit_range`)
    /// takes precedence.
    pub const fn mark_insert_extent(&mut self, offset: Offset, insert_len: usize) {
        if let Some(ref mut p) = self.pending {
            let end = offset.saturating_add_raw(insert_len);
            p.last_insert_end_t1 = Some(match p.last_insert_end_t1 {
                None => end,
                Some(existing) => existing.max(end),
            });
        }
    }

    /// Set a hint for the undo `']` mark end offset, computed externally.
    ///
    /// Called from the effect processor when it has the insert text
    /// and can count newlines to compute the exact line offset in T0.
    /// Only updates if the new value is larger (max semantics).
    pub const fn set_undo_mark_end_hint(&mut self, offset: Offset) {
        if let Some(ref mut p) = self.pending {
            p.last_insert_end_t1 = Some(match p.last_insert_end_t1 {
                None => offset,
                Some(existing) => existing.max(offset),
            });
        }
    }

    /// Set a precomputed undo mark `']'` line-start offset in T0 text.
    ///
    /// When set, `end_group()` uses this directly for `undo_mark_end`,
    /// bypassing the normal computation from `last_edit_end` /
    /// `last_insert_end_t1`. This is necessary for block visual insert
    /// groups where the T0 line-start cannot be correctly derived at
    /// `end_group()` time (the `text` parameter may be T1 text with
    /// different byte offsets than T0).
    pub const fn set_precomputed_undo_mark_end(&mut self, offset: Offset) {
        if let Some(ref mut p) = self.pending {
            p.precomputed_undo_mark_end = Some(offset);
        }
    }

    /// Set precomputed redo mark overrides for block insert groups.
    ///
    /// For block visual insert, redo marks differ from undo marks because
    /// redo restores the T1 (post-edit) text where byte offsets differ from
    /// T0 (pre-edit) due to multi-line insertions.
    ///
    /// `mark_dot`: the `'.'` mark after redo (start of first secondary line
    /// in T1). `mark_end`: the `']'` mark after redo (start of last affected
    /// line in T1).
    pub const fn set_precomputed_redo_marks(
        &mut self,
        mark_dot: Option<Offset>,
        mark_end: Option<Offset>,
    ) {
        if let Some(ref mut p) = self.pending {
            p.precomputed_redo_mark_dot = mark_dot;
            p.precomputed_redo_mark_end = mark_end;
        }
    }

    /// Update the auto-computed redo `']` mark for insert effects.
    ///
    /// Called from the effect processor for each Insert effect. Sets the
    /// redo mark to the line-start of the last inserted line in T1, using
    /// max semantics on `insert_end_t1` to track the furthest insert.
    ///
    /// Values set here are overridden by [`Self::set_precomputed_redo_marks`]
    /// for block insert groups (which call that method after individual
    /// inserts are processed).
    ///
    /// `line_start`: the line-start offset in T1 for this insert's last line.
    /// `insert_end_t1`: the end offset of this insert in T1 (for ordering).
    pub fn update_redo_mark_end_if_larger(&mut self, line_start: Offset, insert_end_t1: Offset) {
        if let Some(ref mut p) = self.pending {
            let dominated = p
                .last_insert_end_t1
                .is_none_or(|prev| insert_end_t1 >= prev);
            if dominated {
                p.precomputed_redo_mark_end = Some(line_start);
            }
        }
    }

    /// Record that a text edit occurred at `offset` with `old_len` bytes
    /// affected in the original text.
    ///
    /// Tracks both minimum offset (for cursor placement) and maximum
    /// end-of-edit (for undo `']` mark computation). `old_len` is the
    /// number of bytes in the original text affected by this edit:
    /// - Delete: the length of the deleted region
    /// - Replace: the length of the replaced region
    /// - Insert: 0
    pub const fn mark_edit_range(&mut self, offset: Offset, old_len: usize) {
        if let Some(ref mut p) = self.pending {
            p.has_edits = true;
            p.first_edit_offset = Some(match p.first_edit_offset {
                None => offset,
                Some(existing) => existing.min(offset),
            });
            let end = offset.saturating_add_raw(old_len);
            p.last_edit_end = Some(match p.last_edit_end {
                None => end,
                Some(existing) => existing.max(end),
            });
        }
    }

    /// Like [`Self::mark_edit_range`] but also computes and stores the T0 line-start
    /// offset for the edit end. This pre-computation ensures correct undo mark
    /// `']` even when `end_group` is called with T1 text (multi-keystroke groups).
    pub fn mark_edit_range_with_text(&mut self, offset: Offset, old_len: usize, t0_text: &str) {
        self.mark_edit_range(offset, old_len);
        if let Some(ref mut p) = self.pending {
            // Precompute first_edit line start in T0
            let first_ls = Offset::new(line_start_for_offset(Some(t0_text), offset.get()));
            p.first_edit_line_start = Some(match p.first_edit_line_start {
                None => first_ls,
                Some(existing) => existing.min(first_ls),
            });
            // Precompute last_edit_end line start in T0
            let end = offset.saturating_add_raw(old_len);
            let last_affected = end.get().saturating_sub(1);
            let last_ls = Offset::new(line_start_for_offset(Some(t0_text), last_affected));
            p.last_edit_end_line_start = Some(match p.last_edit_end_line_start {
                None => last_ls,
                Some(existing) => existing.max(last_ls),
            });
        }
    }

    /// Record that a text edit occurred (without a specific offset).
    ///
    /// Marks the group as non-empty. Prefer [`mark_edit_at`](Self::mark_edit_at)
    /// when the offset is known.
    pub const fn mark_edit(&mut self) {
        if let Some(ref mut p) = self.pending {
            p.has_edits = true;
        }
    }

    /// Mark that the current group contains an insert or replace (not a pure delete).
    pub const fn mark_has_insert_or_replace(&mut self) {
        if let Some(ref mut p) = self.pending {
            p.has_insert_or_replace = true;
        }
    }

    /// Accumulate the byte delta of a text edit within the current group.
    ///
    /// `old_len`: bytes consumed from the original text (delete/replace extent).
    /// `new_len`: bytes written as replacement (insert/replace extent).
    ///
    /// The net delta `new_len - old_len` is accumulated. After the group is
    /// committed, the stored `edit_delta` enables construction of a `ChangeSet`
    /// for `remap_all_positions` after undo/redo.
    pub fn accumulate_delta(&mut self, old_len: usize, new_len: usize) {
        if let Some(ref mut p) = self.pending {
            p.edit_delta = p
                .edit_delta
                .saturating_add(byte_delta::delta_i32(new_len, old_len));
        }
    }

    /// The first edit of the open group and the start of its line, as
    /// `mark_edit_at_with_text` and `mark_edit_range_with_text` record them,
    /// or `None` without an open group.
    #[must_use]
    pub const fn pending_first_edit(&self) -> Option<(Option<Offset>, Option<Offset>)> {
        match &self.pending {
            Some(p) => Some((p.first_edit_offset, p.first_edit_line_start)),
            None => None,
        }
    }

    /// Set the first edit of the open group and the start of its line.
    ///
    /// Formatting while typing breaks the line before where the insert
    /// started, and Vim still puts the cursor back where it started on
    /// undo, so the break must not move the first edit.
    pub const fn set_pending_first_edit(
        &mut self,
        offset: Option<Offset>,
        line_start: Option<Offset>,
    ) {
        if let Some(ref mut p) = self.pending {
            p.first_edit_offset = offset;
            p.first_edit_line_start = line_start;
        }
    }

    /// Whether a group is currently open.
    #[must_use]
    pub const fn has_pending_group(&self) -> bool {
        self.pending.is_some()
    }

    /// Discard any in-flight undo group without committing it.
    ///
    /// Used by [`crate::execution::VimEngine::emergency_reset()`] to clean up orphaned metadata
    /// after a panic. The next `begin_group()` will start fresh.
    pub fn abandon_pending(&mut self) {
        self.pending = None;
    }

    /// Commit the current undo group.
    ///
    /// If no edits were recorded ([`Self::mark_edit`] / [`Self::mark_edit_at`] never
    /// called), the group is discarded and `None` returned. Otherwise a
    /// new node is created as a child of `current` and becomes the new
    /// `current`.
    ///
    /// `text` is the document text BEFORE the undo group's edits were applied
    /// (the "original" or T0 text). It is used to compute line-start offsets
    /// for the `'[`, `']`, `'.` marks that Neovim sets after undo.
    ///
    /// # Complexity
    ///
    /// Time: O(1) amortized — arena `Vec::push` is O(1) amortized,
    /// parent's child `SmallVec::push` is O(1) (inline capacity = 2).
    ///
    /// Space: O(1) amortized — one `UndoNode` added to the arena.
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn end_group(
        &mut self,
        cursor_after: Offset,
        timestamp: u64,
        text: Option<&str>,
    ) -> Option<NodeId> {
        // While merge is active, only the outermost end_group commits.
        if self.merging {
            if self.merge_depth > 1 {
                self.merge_depth -= 1;
                return None;
            }
            // merge_depth == 1: this is the outermost pair's end_group,
            // but merge is still active (more iterations may follow).
            // Don't commit — keep the pending group open.
            if self.merge_depth == 1 {
                self.merge_depth = 0;
                return None;
            }
            // merge_depth == 0 shouldn't happen, but handle defensively.
        }
        // Not merging, or merging just ended: commit normally.
        let pending = self.pending.take()?;
        // Neovim records ALL undo groups, even those without text edits
        // (e.g. guu on already-lowercase text, ~ on non-alpha). Discarding
        // empty groups breaks undo ordering: 'u' would skip the no-op and
        // undo a different operation. Always record the group.

        // Compute undo change marks: line-start offsets in the original text
        // (T0) for `'[` and `']` after undo. These match Neovim's behavior
        // where undo/redo sets these marks to the beginning of the first/last
        // changed line.
        //
        // The `text` parameter at end_group time may be T0 (pre-edit) for
        // single-response undo groups, or T1 (post-edit) for multi-keystroke
        // groups. Use `t0_text_len` (captured at begin_group time) to detect
        // and safely clamp T0-space offsets against the T0 text boundary.
        // Determine if `text` is T0 or T1. When T1 (text.len() != t0_len),
        // line-start computation against this text would produce wrong offsets.
        // Fall back to None so line_start_for_offset returns the raw offset.
        // When text at end_group is T1, we fall back to precomputed line
        // starts from mark_edit_range_with_text / mark_edit_at_with_text.
        // The t0_text variable is only used as a fallback when precomputed
        // values are unavailable.
        let t0_text: Option<&str> = match (text, pending.t0_text_len) {
            (Some(t), Some(t0_len)) if t.len() == t0_len => Some(t),
            (Some(t), Some(t0_len)) if t0_len <= t.len() => {
                // text is T1 (grew); use T0-length prefix for line-start.
                // Floor to char boundary to avoid panics with multi-byte text.
                let safe_len = t.floor_char_boundary(t0_len);
                Some(&t[..safe_len])
            }
            (Some(_), Some(_)) => {
                // text is T1 but shorter than T0 (delete-heavy operation).
                // Can't reliably compute T0 line starts from T1 text.
                // Precomputed values from mark_edit_*_with_text will be used.
                None
            }
            _ => text,
        };
        let undo_mark_start = pending.first_edit_line_start.or_else(|| {
            pending
                .first_edit_offset
                .map(|off| Offset::new(line_start_for_offset(t0_text, off.get())))
        });
        let undo_mark_end = pending
            .precomputed_undo_mark_end
            .or(pending.last_edit_end_line_start)
            .or_else(|| {
                pending.last_edit_end.map(|end| {
                    // last_edit_end points one past the last affected byte.
                    // For line-start computation, we want the line containing
                    // the last affected byte (end - 1), not the byte after it.
                    let last_affected = end.get().saturating_sub(1);
                    Offset::new(line_start_for_offset(t0_text, last_affected))
                })
            })
            .or_else(|| {
                // Pure insert (no delete/replace): last_edit_end is None.
                // Use last_insert_end_t1 (the end offset in post-edit text)
                // to compute the undo `']` mark. Clamp to T0 length.
                pending.last_insert_end_t1.map(|end_t1| {
                    let t0_len = pending
                        .t0_text_len
                        .unwrap_or_else(|| text.map_or(end_t1.get(), str::len));
                    let clamped = end_t1.get().min(t0_len);
                    Offset::new(line_start_for_offset(t0_text, clamped))
                })
            })
            .or(undo_mark_start); // single point edit: start == end

        // Compute undo_mark_dot — like undo_mark_start but skips a leading
        // '\n'. Used for mark '.' after undo/redo. Falls back to undo_mark_start.
        //
        // Only use `logical_edit_line_start` (which skips a leading '\n') for
        // pure-delete groups. For replace/insert groups, the '\n' at the edit
        // position IS part of the content being changed (e.g. J replaces '\n'
        // with ' '), so the mark should be on the line CONTAINING the '\n'.
        let undo_mark_dot = if !pending.has_insert_or_replace {
            pending
                .first_edit_offset
                .map(|off| Offset::new(logical_edit_line_start(t0_text, off.get())))
                .or(undo_mark_start)
        } else {
            undo_mark_start
        };

        let parent_depth = self.nodes[self.current.index()].depth;
        let seq = self.next_sequence;
        self.next_sequence += 1;

        let node = UndoNode {
            parent: Some(self.current),
            children: SmallVec::new(),
            sequence: seq,
            cursors_before: pending.cursors_before,
            cursor_after,
            first_edit_offset: pending.first_edit_offset,
            last_edit_end: pending.last_edit_end,
            edit_delta: pending.edit_delta,
            timestamp,
            depth: parent_depth.saturating_add(1),
            cursor_strategy: pending.cursor_strategy,
            preferred_child_idx: None,
            marks: pending.marks,
            mode: pending.mode,
            last_visual: pending.last_visual,
            was_changed: pending.was_changed,
            pruned: false,
            save_nr: 0,
            undo_mark_start,
            undo_mark_end,
            undo_mark_dot,
            redo_mark_dot: pending.precomputed_redo_mark_dot,
            redo_mark_end: pending.precomputed_redo_mark_end,
        };

        let id = if let Some(reuse_idx) = self.free_list.pop() {
            // Remove from pruned_since_last_drain to prevent
            // gc_pruned_undo_snapshots from deleting the reused slot's
            // live data from UndoStore (TOCTOU fix).
            self.pruned_since_last_drain
                .retain(|id| id.index() != reuse_idx as usize);
            let idx = reuse_idx as usize;
            self.nodes[idx] = node;
            NodeId::new(reuse_idx)
        } else {
            debug_assert!(
                self.nodes.len() < u32::MAX as usize,
                "undo tree arena overflow"
            );
            let idx = byte_delta::to_u32(self.nodes.len());
            self.nodes.push(node);
            NodeId::new(idx)
        };
        self.live_count += 1;

        // Register as child of current node and set as preferred redo target.
        let parent = &mut self.nodes[self.current.index()];
        let child_idx = parent.children.len();
        parent.children.push(id);
        parent.preferred_child_idx = Some(u16::try_from(child_idx).unwrap_or(u16::MAX));

        self.current = id;
        Some(id)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Navigation
    // ═══════════════════════════════════════════════════════════════════════

    /// Undo one step (move to parent node).
    ///
    /// Updates the parent's preferred child to point back to the node we
    /// came from so that subsequent redo returns to it. Swaps the current
    /// node's mark snapshot with the live marks before navigating.
    ///
    /// Returns `None` if already at root.
    ///
    /// # Complexity
    ///
    /// Time: O(C) where C = number of children of the parent node
    /// (for the linear `position()` search to update `preferred_child_idx`).
    /// C is typically 1-2 (SmallVec inline capacity = 2). The mark snapshot
    /// swap is O(26) = O(1). Arena index lookups are O(1).
    ///
    /// Space: O(1)
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn undo(
        &mut self,
        marks: &mut Marks,
        live_last_visual: &mut Option<LastVisualInfo>,
    ) -> Option<UndoStep> {
        let cur_idx = self.current.index();
        let parent_id = self.nodes[cur_idx].parent?;

        // Swap mark snapshot with live marks before moving current.
        self.nodes[cur_idx].marks.swap_with(marks);
        // Swap last_visual with engine's live value.
        std::mem::swap(&mut self.nodes[cur_idx].last_visual, live_last_visual);

        // Update parent's preferred child to remember where we came from.
        let child_pos = self.nodes[parent_id.index()]
            .children
            .iter()
            .position(|&c| c == self.current);
        if let Some(pos) = child_pos {
            self.nodes[parent_id.index()].preferred_child_idx =
                Some(u16::try_from(pos).unwrap_or(u16::MAX));
        }

        // Compute undo cursors: EntryPosition overrides first_edit_offset.
        // For multi-cursor, each stored cursor_before is returned as-is when
        // EntryPosition is active; otherwise the primary cursor uses
        // first_edit_offset and secondary cursors use their stored positions.
        let node = &self.nodes[cur_idx];
        let cursors =
            if node.cursor_strategy == crate::primitives::UndoCursorStrategy::EntryPosition {
                node.cursors_before.clone()
            } else {
                let primary = node.first_edit_offset.unwrap_or(node.cursors_before[0]);
                let mut result = SmallVec::with_capacity(node.cursors_before.len());
                result.push(primary);
                if node.cursors_before.len() > 1 {
                    result.extend_from_slice(&node.cursors_before[1..]);
                }
                result
            };

        // Undo change marks: use the node's precomputed line-start offsets.
        // These were computed at end_group() time from the original (T0) text.
        let change_mark_start = node.undo_mark_start;
        let change_mark_end = node.undo_mark_end;

        let first_edit_offset = node.first_edit_offset;
        let mark_dot = node.undo_mark_dot;
        let last_edit_end = node.last_edit_end;
        let edit_delta = node.edit_delta;
        let mode = node.mode;
        let last_visual = node.last_visual;

        self.current = parent_id;

        // Check if we landed on a save point.
        let is_at_save_point =
            self.nodes[parent_id.index()].save_nr > 0 || parent_id == NodeId::ROOT;

        self.last_was_undo = true;

        Some(UndoStep {
            node: parent_id,
            cursors,
            change_mark_start,
            change_mark_end,
            first_edit_offset,
            mark_dot,
            last_edit_end,
            edit_delta,
            mode,
            last_visual,
            is_at_save_point,
        })
    }

    /// Redo one step (move to preferred child node).
    ///
    /// Skips pruned children: if the preferred child is pruned, searches
    /// through remaining children for the first non-pruned one. If all
    /// children are pruned (or there are no children), returns `None`.
    ///
    /// Swaps the target child node's mark snapshot with the live marks
    /// after navigating to it.
    ///
    /// Returns `None` if at a leaf or no live preferred child exists.
    ///
    /// # Complexity
    ///
    /// Time: O(C) where C = number of children of the current node
    /// (for `resolve_redo_child` scanning live children when the preferred
    /// child is pruned). C is typically 1-2. Mark snapshot swap is O(26) = O(1).
    ///
    /// Space: O(1)
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn redo(
        &mut self,
        marks: &mut Marks,
        live_last_visual: &mut Option<LastVisualInfo>,
    ) -> Option<UndoStep> {
        let child_id = self.resolve_redo_child(self.current)?;

        self.current = child_id;

        // Swap mark snapshot with live marks after moving to child.
        self.nodes[child_id.index()].marks.swap_with(marks);
        // Swap last_visual with engine's live value.
        std::mem::swap(
            &mut self.nodes[child_id.index()].last_visual,
            live_last_visual,
        );

        let node = &self.nodes[child_id.index()];
        // Neovim uses the same uh_cursor for both undo and redo — the cursor
        // saved when the undo header was created (at the start of the change).
        // Match this by using cursors_before (with first_edit_offset override)
        // for redo, just like undo does.
        let cursors =
            if node.cursor_strategy == crate::primitives::UndoCursorStrategy::EntryPosition {
                node.cursors_before.clone()
            } else {
                let primary = node.first_edit_offset.unwrap_or(node.cursors_before[0]);
                let mut result = SmallVec::with_capacity(node.cursors_before.len());
                result.push(primary);
                if node.cursors_before.len() > 1 {
                    result.extend_from_slice(&node.cursors_before[1..]);
                }
                result
            };
        // Redo change marks: use redo-specific overrides when available
        // (block insert groups have different T1 line-start offsets than T0).
        // Fall back to the undo values for non-block-insert groups where
        // T0 and T1 line starts are typically identical.
        let change_mark_start = node.undo_mark_start;
        let change_mark_end = node.redo_mark_end.or(node.undo_mark_end);

        let first_edit_offset = node.first_edit_offset;
        let mark_dot = node.redo_mark_dot.or(node.undo_mark_dot);
        let last_edit_end = node.last_edit_end;
        let edit_delta = node.edit_delta;
        let mode = node.mode;
        let last_visual = node.last_visual;

        // Check if we landed on a save point.
        let is_at_save_point = node.save_nr > 0;

        self.last_was_undo = true;

        Some(UndoStep {
            node: child_id,
            cursors,
            change_mark_start,
            change_mark_end,
            first_edit_offset,
            mark_dot,
            last_edit_end,
            edit_delta,
            mode,
            last_visual,
            is_at_save_point,
        })
    }

    /// Whether undo is possible (not at root).
    #[must_use]
    pub const fn can_undo(&self) -> bool {
        !matches!(self.current, NodeId::ROOT)
    }

    /// Whether redo is possible (at least one non-pruned child exists).
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.resolve_redo_child(self.current).is_some()
    }

    /// Resolve the next redo child for `node_id`, skipping pruned nodes.
    ///
    /// Returns the preferred child if live, otherwise the first live child.
    /// Returns `None` if no live children exist or no preferred index is set.
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    fn resolve_redo_child(&self, node_id: NodeId) -> Option<NodeId> {
        let node = &self.nodes[node_id.index()];
        let pref_idx = node.preferred_child_idx? as usize;
        let candidate = *node.children.get(pref_idx)?;
        if self.nodes[candidate.index()].pruned {
            node.children
                .iter()
                .copied()
                .find(|&c| !self.nodes[c.index()].pruned)
        } else {
            Some(candidate)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Queries
    // ═══════════════════════════════════════════════════════════════════════

    /// Current position in the tree.
    #[must_use]
    pub const fn current(&self) -> NodeId {
        self.current
    }

    /// Total number of nodes in the arena (including root and pruned nodes).
    ///
    /// This is the raw arena size, useful for debugging and memory analysis.
    /// For the count of visible changes, use [`change_count`](Self::change_count).
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of live (non-pruned) changes recorded (excludes root).
    ///
    /// After pruning, this reflects the capped history size rather than
    /// the total arena size.
    #[must_use]
    pub const fn change_count(&self) -> usize {
        self.live_count
    }

    /// The next sequence number that will be assigned to a new undo group.
    ///
    /// Compare two calls to detect whether the undo tree changed between them.
    /// Monotonically increasing — never decreases even after undo/redo.
    #[inline]
    #[must_use]
    pub const fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Metadata for a specific node.
    ///
    /// Returns `None` if the node does not exist or has been pruned.
    /// Pruned nodes are invisible — they are treated as if they never existed.
    ///
    /// # Complexity
    ///
    /// Time: O(C) where C = number of children of the node (to count
    /// live children). C is typically 1-2.
    ///
    /// Space: O(1)
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "child NodeIds are valid arena indices by construction"
    )]
    pub fn node_info(&self, id: NodeId) -> Option<NodeInfo> {
        let node = self.nodes.get(id.index())?;
        if node.pruned {
            return None;
        }
        let live_child_count = node
            .children
            .iter()
            .filter(|&&c| !self.nodes[c.index()].pruned)
            .count();
        Some(NodeInfo {
            id,
            parent: node.parent,
            child_count: live_child_count,
            sequence: node.sequence,
            cursor_before: node.cursors_before[0],
            cursor_after: node.cursor_after,
            first_edit_offset: node.first_edit_offset,
            timestamp: node.timestamp,
            depth: node.depth,
        })
    }

    /// Metadata for the current node.
    ///
    /// Falls back to a root-like default if the arena is somehow corrupted.
    #[must_use]
    pub fn current_info(&self) -> NodeInfo {
        self.node_info(self.current).unwrap_or(NodeInfo {
            id: self.current,
            parent: None,
            child_count: 0,
            sequence: 0,
            cursor_before: Offset::ZERO,
            cursor_after: Offset::ZERO,
            first_edit_offset: None,
            timestamp: 0,
            depth: 0,
        })
    }

    /// Number of live (non-pruned) child branches at the current node.
    ///
    /// # Complexity
    ///
    /// Time: O(C) where C = number of children of the current node.
    /// Typically O(1) since C is 1-2.
    ///
    /// Space: O(1)
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "child NodeIds are valid arena indices by construction"
    )]
    pub fn branch_count(&self) -> usize {
        self.nodes.get(self.current.index()).map_or(0, |n| {
            n.children
                .iter()
                .filter(|&&c| !self.nodes[c.index()].pruned)
                .count()
        })
    }

    /// Depth of the current node from root.
    #[must_use]
    pub fn depth(&self) -> u32 {
        self.nodes.get(self.current.index()).map_or(0, |n| n.depth)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pruning (undolevels)
    // ═══════════════════════════════════════════════════════════════════════

    /// Prune the oldest branches until `live_count <= max_levels`.
    ///
    /// Uses a tombstone approach: nodes are marked `pruned: true` rather than
    /// freed (arena indices must remain stable). Pruning cascades upward:
    /// if a parent has all its children pruned AND the parent is not on the
    /// path from root to `current`, the parent is also pruned.
    ///
    /// **Invariants preserved:**
    /// - The root node (index 0) is never pruned.
    /// - No node on the path from root to `current` is ever pruned.
    /// - After this call, `live_count <= max_levels`.
    ///
    /// If `live_count <= max_levels` already, this is a no-op.
    ///
    /// # Complexity
    ///
    /// Time: O(D * N) where D = `live_count - max_levels` (nodes to prune)
    /// and N = total arena size. Each prune iteration scans the full arena
    /// to find the oldest live leaf. The ancestor set construction is O(depth)
    /// with O(depth) HashSet insertions. Cascading up after tombstoning each
    /// leaf is O(C) per level where C = children count.
    ///
    /// Space: O(depth) for the ancestor `HashSet`.
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn prune(&mut self, max_levels: usize) {
        if self.live_count <= max_levels {
            return;
        }

        // Build the ancestor set: all nodes on the path from root to current.
        // These must NEVER be pruned.
        let mut ancestor_set = std::collections::HashSet::new();
        let mut probe = self.current;
        loop {
            ancestor_set.insert(probe);
            match self.nodes[probe.index()].parent {
                Some(p) => probe = p,
                None => break,
            }
        }

        // Repeatedly prune the oldest live leaf that is not an ancestor
        // until live_count <= max_levels.
        while self.live_count > max_levels {
            // Find the live leaf with the smallest sequence number that is
            // not on the ancestor path. A "live leaf" is a non-pruned node
            // with no non-pruned children.
            let candidate = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(i, node)| {
                    if node.pruned || *i == 0 {
                        return false; // skip pruned + root
                    }
                    if ancestor_set.contains(&NodeId::new(byte_delta::to_u32(*i))) {
                        return false; // never prune ancestors of current
                    }
                    // Must be a live leaf (no live children).
                    !node.children.iter().any(|&c| !self.nodes[c.index()].pruned)
                })
                .min_by_key(|(_, node)| node.sequence)
                .map(|(i, _)| NodeId::new(byte_delta::to_u32(i)));

            let Some(target) = candidate else {
                // No pruneable leaf found — cannot prune further.
                break;
            };

            // Tombstone the target and cascade up to parent if needed.
            let mut to_prune = target;
            loop {
                self.nodes[to_prune.index()].pruned = true;
                self.live_count -= 1;
                self.pruned_since_last_drain.push(to_prune);

                // Remove from parent's children list so that slot reuse
                // does not create phantom children on the old parent.
                if let Some(parent_id) = self.nodes[to_prune.index()].parent {
                    let parent = &self.nodes[parent_id.index()];
                    let removed_pos = parent.children.iter().position(|&c| c == to_prune);
                    if let Some(pos) = removed_pos {
                        let old_pref = parent.preferred_child_idx.map(|p| p as usize);
                        self.nodes[parent_id.index()].children.remove(pos);
                        // Adjust preferred_child_idx after removal.
                        let new_len = self.nodes[parent_id.index()].children.len();
                        if new_len == 0 {
                            self.nodes[parent_id.index()].preferred_child_idx = None;
                        } else if let Some(pref) = old_pref {
                            if pref == pos {
                                // Preferred child was the one removed; clamp to last.
                                let clamped = pref.min(new_len - 1);
                                self.nodes[parent_id.index()].preferred_child_idx =
                                    Some(u16::try_from(clamped).unwrap_or(u16::MAX));
                            } else if pref > pos {
                                // Preferred was after the removed position; shift down.
                                self.nodes[parent_id.index()].preferred_child_idx =
                                    Some(u16::try_from(pref - 1).unwrap_or(u16::MAX));
                            }
                            // If pref < pos, no adjustment needed.
                        }
                    }
                }

                // Clear heavy fields to shrink memory footprint of dead slot.
                self.nodes[to_prune.index()].children.clear();
                self.nodes[to_prune.index()].marks = MarkSnapshot::new();
                // Make slot available for reuse.
                self.free_list.push(byte_delta::to_u32(to_prune.index()));

                // Cascade: if the parent has no live children remaining and is
                // not on the ancestor path (and is not root), prune it too.
                let parent_id = match self.nodes[to_prune.index()].parent {
                    Some(p) => p,
                    None => break,
                };
                if parent_id == NodeId::ROOT {
                    break;
                }
                if ancestor_set.contains(&parent_id) {
                    break;
                }
                let parent = &self.nodes[parent_id.index()];
                if parent.pruned {
                    break;
                }
                // After removing the pruned child, check if parent has any
                // children left (all pruned children have been removed).
                if parent.children.is_empty() {
                    to_prune = parent_id;
                } else {
                    break;
                }
            }
        }
    }

    /// Takes accumulated pruned NodeIds since the last drain.
    /// Called by `VimSession` to synchronize UndoStore.
    pub fn take_pruned_ids(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.pruned_since_last_drain)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Snapshot (for visualization)
    // ═══════════════════════════════════════════════════════════════════════

    /// Produce a snapshot of the live (non-pruned) undo tree for visualization.
    ///
    /// Iterates every non-pruned node in the arena and builds a
    /// [`UndoTreeSnapshot`] containing [`UndoTreeNodeView`] entries. The
    /// `is_current` flag is set on the node matching [`current()`](Self::current).
    ///
    /// Pruned nodes are excluded — they are invisible to callers.
    ///
    /// # Complexity
    ///
    /// Time: O(N * C_max) where N = total arena size (including pruned nodes)
    /// and C_max = maximum children count per node. Each live node filters
    /// its children for pruned status. In practice C_max is typically 1-2.
    ///
    /// Space: O(L) where L = number of live (non-pruned) nodes — each is
    /// cloned into the snapshot.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "child NodeIds are valid arena indices by construction"
    )]
    pub fn snapshot(&self) -> UndoTreeSnapshot {
        let nodes = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| !node.pruned)
            .map(|(i, node)| {
                let id = NodeId::new(byte_delta::to_u32(i));
                UndoTreeNodeView {
                    id,
                    // Safe: parent of a live node is always live (prune cascades
                    // only when ALL children are pruned, so a live child keeps
                    // its parent alive).
                    parent: node.parent,
                    children: node
                        .children
                        .iter()
                        .copied()
                        .filter(|&c| !self.nodes[c.index()].pruned)
                        .collect(),
                    sequence: node.sequence,
                    timestamp: node.timestamp,
                    cursor_before: node.cursors_before[0],
                    is_current: id == self.current,
                }
            })
            .collect();
        UndoTreeSnapshot {
            nodes,
            current: self.current,
            change_count: byte_delta::to_u32(self.change_count()),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Time-based navigation
    // ═══════════════════════════════════════════════════════════════════════

    /// Peek backward by `count` changes along the ancestor path.
    ///
    /// Does **not** move `current` — the caller is responsible for issuing
    /// `undo()` calls to actually navigate. Returns `(actual_undo_count, cursor)`
    /// or `None` if already at root.
    ///
    /// # Complexity
    ///
    /// Time: O(count) — walks up the ancestor chain, one arena lookup per step.
    ///
    /// Space: O(1)
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn earlier_by_count(&self, count: u32) -> Option<(u32, Offset)> {
        let mut probe = self.current;
        let mut actual = 0u32;
        let mut last_cursor = None;
        for _ in 0..count {
            let node = &self.nodes[probe.index()];
            let parent_id = match node.parent {
                Some(p) => p,
                None => break,
            };
            actual += 1;
            let cursor =
                if node.cursor_strategy == crate::primitives::UndoCursorStrategy::EntryPosition {
                    node.cursors_before[0]
                } else {
                    node.first_edit_offset.unwrap_or(node.cursors_before[0])
                };
            last_cursor = Some(cursor);
            probe = parent_id;
        }
        last_cursor.map(|cursor| (actual, cursor))
    }

    /// Peek forward by `count` changes along the preferred path.
    ///
    /// Does **not** move `current` — the caller is responsible for issuing
    /// `redo()` calls to actually navigate. Returns `(actual_redo_count, cursor)`
    /// or `None` if already at leaf.
    ///
    /// # Complexity
    ///
    /// Time: O(count * C_max) where C_max = maximum children count per
    /// node (for `resolve_redo_child` at each step). Typically O(count)
    /// since C_max is 1-2.
    ///
    /// Space: O(1)
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn later_by_count(&self, count: u32) -> Option<(u32, Offset)> {
        let mut probe = self.current;
        let mut actual = 0u32;
        let mut last_cursor = None;
        for _ in 0..count {
            let child_id = match self.resolve_redo_child(probe) {
                Some(id) => id,
                None => break,
            };
            actual += 1;
            let node = &self.nodes[child_id.index()];
            // Use first_edit_offset for redo cursor (matching Neovim's behavior
            // of placing cursor at the start of the redone change), falling back
            // to cursor_after if no edit offset is recorded.
            let cursor = node.first_edit_offset.unwrap_or(node.cursor_after);
            last_cursor = Some(cursor);
            probe = child_id;
        }
        last_cursor.map(|cursor| (actual, cursor))
    }

    /// Peek backward to a state from `seconds` ago.
    ///
    /// Walks up the ancestor path until finding a node whose timestamp is
    /// at or before `current_time - seconds`. Does **not** move `current` —
    /// the caller is responsible for issuing `undo()` calls to actually
    /// navigate. Returns `(undo_count, cursor)` or `None` if already at
    /// or before the target time.
    ///
    /// # Complexity
    ///
    /// Time: O(depth) — walks up the full ancestor chain in the worst case,
    /// one arena lookup per step.
    ///
    /// Space: O(1)
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn earlier_by_time(&self, seconds: u64, current_time: u64) -> Option<(u32, Offset)> {
        let target_time = current_time.saturating_sub(seconds);

        // Probe without mutating to count needed undos and find the cursor.
        let mut probe = self.current;
        let mut count = 0u32;
        let mut last_cursor = None;
        loop {
            let node = &self.nodes[probe.index()];
            if node.timestamp <= target_time {
                break;
            }
            match node.parent {
                Some(parent) => {
                    count += 1;
                    let cursor = if node.cursor_strategy
                        == crate::primitives::UndoCursorStrategy::EntryPosition
                    {
                        node.cursors_before[0]
                    } else {
                        node.first_edit_offset.unwrap_or(node.cursors_before[0])
                    };
                    last_cursor = Some(cursor);
                    probe = parent;
                }
                None => break,
            }
        }

        if count == 0 {
            return None;
        }

        last_cursor.map(|cursor| (count, cursor))
    }

    /// Peek forward by `seconds` along the preferred descendant path.
    ///
    /// Does **not** move `current` — the caller is responsible for issuing
    /// `redo()` calls to actually navigate. Returns `(redo_count, cursor)`
    /// or `None` if already at or past the target time.
    ///
    /// # Complexity
    ///
    /// Time: O(depth * C_max) where depth = maximum descendant chain length
    /// and C_max = maximum children count per node (for `resolve_redo_child`).
    /// Typically O(depth) since C_max is 1-2.
    ///
    /// Space: O(1)
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn later_by_time(&self, seconds: u64) -> Option<(u32, Offset)> {
        // Use the current node's timestamp as reference, not wall-clock time.
        // Wall-clock is always greater than all node timestamps, so
        // `current_time + seconds` would overshoot past all history.
        let current_node_time = self.nodes[self.current.index()].timestamp;
        let target_time = current_node_time.saturating_add(seconds);

        // Probe along preferred (live) children without mutating.
        let mut probe = self.current;
        let mut count = 0u32;
        let mut last_cursor = None;
        while let Some(child_id) = self.resolve_redo_child(probe) {
            let child = &self.nodes[child_id.index()];
            if child.timestamp > target_time {
                break;
            }
            probe = child_id;
            count += 1;
            last_cursor = Some(child.first_edit_offset.unwrap_or(child.cursor_after));
        }

        if count == 0 {
            return None;
        }

        last_cursor.map(|cursor| (count, cursor))
    }

    // ═══════════════════════════════════════════════════════════════════════
    // File-save navigation (:earlier Nf / :later Nf)
    // ═══════════════════════════════════════════════════════════════════════

    /// Mark the current node as a file-save point.
    ///
    /// Increments the tree's `save_counter` and records it on the current
    /// node. Call this whenever the buffer is written to disk (`:w`, `:wq`,
    /// auto-save). Used by `:earlier Nf` / `:later Nf` to navigate by
    /// number of file saves.
    ///
    /// # Complexity
    ///
    /// Time: O(1)
    ///
    /// Space: O(1)
    #[allow(
        clippy::indexing_slicing,
        reason = "current is always a valid arena index"
    )]
    pub fn mark_save(&mut self) {
        self.save_counter += 1;
        self.nodes[self.current.index()].save_nr = self.save_counter;
    }

    /// Peek backward by `count` file saves along the ancestor path.
    ///
    /// Walks up the ancestor chain counting nodes where `save_nr > 0`.
    /// Does **not** move `current` — the caller is responsible for issuing
    /// `undo()` calls to actually navigate. Returns `(undo_step_count, cursor)`
    /// or `None` if there are fewer than `count` saves in the undo history.
    ///
    /// # Complexity
    ///
    /// Time: O(depth)
    ///
    /// Space: O(1)
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn earlier_by_saves(&self, count: usize) -> Option<(u32, Offset)> {
        if count == 0 {
            return None;
        }
        let mut saves_found = 0usize;
        let mut steps = 0u32;
        let mut node_id = self.current;
        while let Some(parent_id) = self.nodes[node_id.index()].parent {
            steps += 1;
            // Read cursor from the CHILD node (node_id) being undone,
            // matching earlier_by_count's pattern.
            let child = &self.nodes[node_id.index()];
            let cursor =
                if child.cursor_strategy == crate::primitives::UndoCursorStrategy::EntryPosition {
                    child.cursors_before[0]
                } else {
                    child.first_edit_offset.unwrap_or(child.cursors_before[0])
                };
            if self.nodes[parent_id.index()].save_nr > 0 {
                saves_found += 1;
                if saves_found >= count {
                    return Some((steps, cursor));
                }
            }
            node_id = parent_id;
        }
        None
    }

    /// Peek forward by `count` file saves along the preferred descendant path.
    ///
    /// Walks the preferred-child chain counting nodes where `save_nr > 0`.
    /// Does **not** move `current` — the caller is responsible for issuing
    /// `redo()` calls to actually navigate. Returns `(redo_step_count, cursor)`
    /// or `None` if there are fewer than `count` saves ahead.
    ///
    /// # Complexity
    ///
    /// Time: O(depth * C_max)
    ///
    /// Space: O(1)
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    pub fn later_by_saves(&self, count: usize) -> Option<(u32, Offset)> {
        if count == 0 {
            return None;
        }
        let mut saves_found = 0usize;
        let mut steps = 0u32;
        let mut probe = self.current;
        while let Some(child_id) = self.resolve_redo_child(probe) {
            steps += 1;
            probe = child_id;
            if self.nodes[child_id.index()].save_nr > 0 {
                saves_found += 1;
                if saves_found >= count {
                    let node = &self.nodes[child_id.index()];
                    return Some((steps, node.first_edit_offset.unwrap_or(node.cursor_after)));
                }
            }
        }
        None
    }

    /// Navigate to the undo node with the given sequence number.
    ///
    /// Computes the undo/redo path from the current node to the target and
    /// returns `(undo_count, redo_count, cursor)` or `None` if no node has
    /// that sequence number. The caller issues the corresponding `undo()` /
    /// `redo()` calls to perform the actual navigation.
    ///
    /// If `undo_count > 0` and `redo_count > 0` the caller must undo first,
    /// then redo — this handles branch switches.
    ///
    /// # Complexity
    ///
    /// Time: O(depth) for LCA computation.
    ///
    /// Space: O(depth) for the ancestor path.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "arena NodeId values are valid by construction"
    )]
    /// # Panics
    ///
    /// Panics in debug builds if a non-root node on the redo path is observed
    /// without a parent. This indicates a corruption of the undo tree's
    /// internal arena and should be unreachable for trees built through the
    /// public API.
    pub fn goto_sequence(&mut self, seq: u64) -> Option<(u32, u32, Offset)> {
        // Find the target node by sequence number.
        let target_id = self.nodes.iter().enumerate().find_map(|(i, n)| {
            if !n.pruned && n.sequence == seq {
                let id = NodeId::new(byte_delta::to_u32(i));
                Some(id)
            } else {
                None
            }
        })?;

        if target_id == self.current {
            let cursor = self.nodes[target_id.index()].cursor_after;
            return Some((0, 0, cursor));
        }

        // Build ancestor path from current to root.
        let mut current_ancestors: Vec<NodeId> = Vec::new();
        {
            let mut probe = self.current;
            loop {
                current_ancestors.push(probe);
                match self.nodes[probe.index()].parent {
                    Some(p) => probe = p,
                    None => break,
                }
            }
        }

        // Build ancestor path from target to root.
        let mut target_ancestors: Vec<NodeId> = Vec::new();
        {
            let mut probe = target_id;
            loop {
                target_ancestors.push(probe);
                match self.nodes[probe.index()].parent {
                    Some(p) => probe = p,
                    None => break,
                }
            }
        }

        // Find LCA (lowest common ancestor) as the last common node in both
        // ancestor paths (both paths end at the root).
        let current_set: std::collections::HashSet<usize> =
            current_ancestors.iter().map(|n| n.index()).collect();
        let lca = target_ancestors
            .iter()
            .find(|n| current_set.contains(&n.index()))
            .copied()
            .unwrap_or(NodeId::ROOT);

        // undo_count = distance from current to LCA.
        let undo_count = byte_delta::to_u32(
            current_ancestors
                .iter()
                .position(|&n| n == lca)
                .unwrap_or(0),
        );

        // redo_count = distance from LCA to target (excluding LCA itself).
        let lca_pos = target_ancestors.iter().position(|&n| n == lca).unwrap_or(0);
        let redo_count = byte_delta::to_u32(lca_pos);

        // Set preferred_child_idx along the redo path (LCA → target) so that
        // subsequent redo() calls follow the correct branch instead of the
        // source branch that undo() just stamped.
        // target_ancestors is [target, ..., LCA, ...root]. The redo path from
        // LCA to target is target_ancestors[0..lca_pos] in reverse order.
        // Each pair (parent, child) needs preferred_child_idx set on parent.
        for i in (0..lca_pos).rev() {
            let child_id = target_ancestors[i];
            #[allow(
                clippy::expect_used,
                reason = "non-root nodes always have a parent by tree construction — arena indices are monotonically allocated and never removed"
            )]
            let parent_id = self.nodes[child_id.index()]
                .parent
                .expect("non-root node on target path must have parent");
            if let Some(pos) = self.nodes[parent_id.index()]
                .children
                .iter()
                .position(|&c| c == child_id)
            {
                // Branch arity is small; saturate the rare overflow rather than
                // silently truncating with `as u16`.
                let idx = u16::try_from(pos).unwrap_or(u16::MAX);
                self.nodes[parent_id.index()].preferred_child_idx = Some(idx);
            }
        }

        let cursor = self.nodes[target_id.index()].cursor_after;
        Some((undo_count, redo_count, cursor))
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Leaf enumeration
    // ═══════════════════════════════════════════════════════════════════════

    /// Collect all live leaf nodes for `:undolist` display.
    ///
    /// A live leaf is a non-pruned node whose children are all pruned (or has
    /// no children). The root is included only if it is the sole live node.
    /// Results are sorted by sequence number.
    ///
    /// # Complexity
    ///
    /// Time: O(N * C_max + L log L) where N = total arena size (scanned for
    /// live leaves), C_max = max children per node, and L = number of leaves
    /// found (sorted by sequence).
    ///
    /// Space: O(L) for the collected leaf info vector.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "child NodeIds are valid arena indices by construction"
    )]
    pub fn leaves(&self) -> Vec<LeafInfo> {
        let mut result: Vec<LeafInfo> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, node)| {
                if node.pruned {
                    return false;
                }
                // A live leaf has no live children.
                let has_live_children =
                    node.children.iter().any(|&c| !self.nodes[c.index()].pruned);
                // Include root only if it's the sole live node (live_count == 0 means no changes).
                !has_live_children && (*i != 0 || self.live_count == 0)
            })
            .map(|(i, node)| LeafInfo {
                node: NodeId::new(byte_delta::to_u32(i)),
                sequence: node.sequence,
                timestamp: node.timestamp,
                depth: node.depth,
            })
            .collect();
        result.sort_by_key(|l| l.sequence);
        result
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Find the byte offset of the start of the line containing `offset`.
///
/// Scans backward from `offset` for the last `\n` and returns the byte
/// after it. Returns `0` if `offset` is on the first line.
///
/// When `text` is `None`, returns `offset` unchanged (no snapping).
fn line_start_for_offset(text: Option<&str>, offset: usize) -> usize {
    let Some(t) = text else {
        return offset;
    };
    let clamped = offset.min(t.len());
    // Use byte-level search so this never panics on non-char-boundary offsets.
    // `\n` is always a single ASCII byte, so byte scanning is safe.
    t.as_bytes()[..clamped]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |newline_pos| newline_pos + 1)
}

/// Compute the logical edit line-start, skipping a leading `\n`.
///
/// When `dd` deletes the last line, the delete range starts at the preceding
/// `\n` (e.g. offset 9 in "keep this\naccident"). But the LOGICAL first
/// affected line is line 2 (offset 10), not line 1 (offset 0). Neovim's
/// `changed_lines(first_lnum)` uses the line number of the deleted line,
/// not the line of the preceding separator.
///
/// This function checks if `text[offset] == '\n'` and, if so, uses
/// `offset + 1` for the line-start computation.
fn logical_edit_line_start(text: Option<&str>, offset: usize) -> usize {
    let Some(t) = text else {
        return offset;
    };
    let effective = if t.as_bytes().get(offset) == Some(&b'\n') {
        offset + 1
    } else {
        offset
    };
    line_start_for_offset(Some(t), effective)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "undo_tree_tests.rs"]
mod tests;
