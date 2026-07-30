//! Combined Vim state.
//!
//! The complete state of the Vim engine, including mode, registers, marks, etc.
//!
//! # Layering
//!
//! State is a low-mid layer: pure data containers, no execution logic.
//! Imports `primitives`, `std` and sibling state modules; must not import
//! `commands`, `effects`, `execution` or `dispatch`.

use super::MultiCursorState;
use super::{
    ChangeList, CommandLineState, InsertState, JumpList, MacroState, Marks, MessageHistory,
    Registers, RepeatState, ScrollHint, SearchState, StatusMessage, SubstituteConfirmState,
    SyntaxSelectionHistory, UndoStep, UndoTree, VariableStore,
};
use crate::primitives::{BufferId, LastFind, LastVisualInfo, Mode, ReturnTo};
use smart_default::SmartDefault;

/// Cached search count result. Avoids O(n) rescan on repeated n/N.
///
/// When the pattern and document haven't changed, `n`/`N` can adjust `current`
/// arithmetically instead of re-scanning the entire document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchCountCache {
    /// Hash of the search pattern.
    pub pattern_hash: u64,
    /// 1-based index of the current match.
    pub current: u32,
    /// Total matches found.
    pub total: u32,
    /// Whether the count is complete (false if timed out or exceeded maxcount).
    pub complete: bool,
}

impl SearchCountCache {
    /// Try to advance the cached current index by `count` steps in the given
    /// direction. Returns an updated `SearchCountCache` with the new `current`
    /// if the cache is usable (complete, total > 0), or `None` if a full
    /// recount is needed.
    ///
    /// `forward` is the *effective* search direction after resolving `n` vs `N`.
    #[must_use]
    pub const fn try_advance(&self, count: u32, forward: bool) -> Option<Self> {
        // Incomplete counts have unreliable totals — must rescan.
        if !self.complete || self.total == 0 {
            return None;
        }

        // Arithmetic wrapping: (current - 1 ± count) mod total + 1
        let idx0 = self.current.wrapping_sub(1);
        let new_idx0 = if forward {
            (idx0 + count) % self.total
        } else {
            // (idx0 - count) mod total, avoiding underflow
            (idx0 + self.total - (count % self.total)) % self.total
        };

        Some(Self {
            pattern_hash: self.pattern_hash,
            current: new_idx0 + 1,
            total: self.total,
            complete: self.complete,
        })
    }
}

/// The complete state of the Vim engine.
///
/// Contains all mutable state that persists across commands.
#[derive(Debug, Clone, SmartDefault)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct VimState {
    /// Schema version for serde forward-compatibility.
    /// Bumped on breaking state layout changes.
    #[default(1)]
    schema_version: u32,

    /// Current editing mode.
    #[default(Mode::Normal)]
    mode: Mode,

    /// Last find character (for ; and ,).
    /// Stored directly — `LastFind::default()` represents "no find yet"
    /// (both `direction` and `target_char` are `None`).
    last_find: LastFind,

    /// Register storage.
    registers: Registers,

    /// Mark storage.
    marks: Marks,

    /// Jump list for Ctrl-O/Ctrl-I.
    jump_list: JumpList,

    /// Changelist for g;/g, navigation.
    changelist: ChangeList,

    /// Active insert session state.
    /// None when not in insert mode.
    insert_state: Option<InsertState>,

    /// Last inserted text from previous insert session.
    /// Used for Ctrl-A (insert last inserted) and `.` repeat.
    /// Stored when exiting insert mode.
    last_inserted_text: compact_str::CompactString,

    /// Entry type of the last completed insert session.
    /// Used by dot-repeat to know whether the original command was Replace
    /// mode (`ReplaceMode`) so it can use replace (delete+insert) semantics
    /// instead of pure insert semantics.
    last_insert_entry_type: crate::primitives::InsertEntryType,

    /// Per-newline auto-indent byte lengths from the last completed insert session.
    /// Used by dot-repeat to strip original indent and recompute for the replay context.
    last_insert_indent_lens: Vec<usize>,

    /// Staged text for `InsertKind::HostInserted`.
    ///
    /// The host calls `VimEngine::stage_host_insert(text)` to store text here,
    /// then sends `InsertKind::HostInserted`. Precomputation reads and clears
    /// this field. This indirection keeps `InsertKind` trivially-destructible
    /// so `Command` can be used in `const fn` contexts.
    pending_host_insert: compact_str::CompactString,

    /// Last expression text evaluated via `<C-r>=` or `"=`.
    ///
    /// Stored when an expression register evaluation succeeds so that
    /// dot-repeat can re-evaluate the expression instead of replaying
    /// the stale cached result.
    #[cfg_attr(feature = "serde", serde(default))]
    last_expression_text: Option<compact_str::CompactString>,

    /// Last visual mode info.
    /// Stored when exiting visual mode for `gv` and `.` repeat.
    last_visual: Option<LastVisualInfo>,

    /// Repeat state for dot command.
    repeat: RepeatState,

    /// Last text-mutating ex command for dot-repeat (`:s`, `:d`, etc.).
    ///
    /// Set by `command_line_exec` after a text-mutating ex command succeeds.
    /// The `.` command checks this before `last_command` so that ex edits
    /// can be replayed.
    #[cfg_attr(feature = "serde", serde(skip))]
    last_ex_for_dot: Option<compact_str::CompactString>,

    /// Cached search count result. Keyed by pattern hash — invalidated when
    /// search pattern changes or text is edited. Avoids O(n) rescan on every n/N.
    #[cfg_attr(feature = "serde", serde(skip))]
    search_count_cache: Option<SearchCountCache>,

    /// Macro recording state.
    macros: MacroState,

    /// Search state.
    search: SearchState,

    /// Active `:s///c` interactive confirm session.
    ///
    /// `Some` when the user is being prompted to confirm each match.
    /// `None` otherwise.
    #[cfg_attr(feature = "serde", serde(skip))]
    substitute_confirm: Option<SubstituteConfirmState>,

    // ── Transient Processing State (not persisted) ────────────────
    // These fields are used during effect processing and undo tracking.
    // They are skipped by serde and reset on buffer switches or engine
    // resets. Logically they form a "TransientProcessingState" group.
    /// Timestamp (ms) of the last text-mutating edit under an auto-opened undo group.
    #[cfg_attr(feature = "serde", serde(skip))]
    undo_auto_group_last_edit_ms: u64,

    /// Whether the current undo group was auto-opened by the time-based fallback.
    #[cfg_attr(feature = "serde", serde(skip))]
    undo_auto_group_active: bool,

    /// Incremental syntax selection history for expand/shrink navigation.
    ///
    /// Cleared on any text mutation to prevent stale ranges.
    #[cfg_attr(feature = "serde", serde(skip))]
    syntax_selection: SyntaxSelectionHistory,

    /// Command-line state.
    command_line: CommandLineState,

    /// Message history ring buffer for `:messages`.
    ///
    /// Bounded ring of the last 200 messages emitted via `ShowMessage` /
    /// `ShowError`. Transient — not persisted in serde snapshots, matching
    /// Neovim's behaviour.
    #[cfg_attr(feature = "serde", serde(skip))]
    message_history: MessageHistory,

    // ── Transient per-keystroke output ────────────────────────────
    // Set by effects during process(), consumed by host afterwards,
    // cleared at start of next process() via clear_transient().
    /// Status message to display (set by ShowMessage/ShowError/ClearMessage).
    #[cfg_attr(feature = "serde", serde(skip))]
    message: Option<StatusMessage>,

    /// Scroll intent for the host (set by ScrollTo/CenterCursor/etc.).
    #[cfg_attr(feature = "serde", serde(skip))]
    scroll_hint: Option<ScrollHint>,

    /// Mode to return to after a Ctrl-O one-shot normal-mode command.
    ///
    /// Set when entering one-shot normal mode (e.g. Ctrl-O from Insert),
    /// consumed after the command executes to restore the original mode.
    /// Generalises the old `InsertState::restart_edit` boolean.
    #[cfg_attr(feature = "serde", serde(skip))]
    return_to: ReturnTo,

    // ── Transient Processing State (continued) ────────────────────
    // Per-undo-group tracking flags, also not persisted.
    /// Whether the `.` (last change) mark has been set in the current undo group.
    /// Reset on `BeginUndoGroup`, prevents per-keystroke overwrites during insert.
    #[cfg_attr(feature = "serde", serde(skip))]
    #[default(false)]
    last_change_mark_set: bool,

    /// Whether the `[` (change start) mark has been set in the current undo group.
    /// Reset on `BeginUndoGroup`, prevents per-keystroke overwrites during insert.
    #[cfg_attr(feature = "serde", serde(skip))]
    #[default(false)]
    change_start_mark_set: bool,

    /// Neovim's `b_new_change` flag.
    ///
    /// Set to `true` when a new undo group begins (`BeginUndoGroup`).
    /// `changed_common` (our `changelist_push`) checks this to decide whether
    /// to create a **new** changelist entry or merely update the last one.
    /// After the first mutation in a new undo group creates a new entry, this
    /// flag is reset to `false` so subsequent mutations in the same group only
    /// update (not grow) the changelist.
    #[cfg_attr(feature = "serde", serde(skip))]
    #[default(true)]
    changelist_new_change: bool,

    /// Sticky count for Ctrl-D/Ctrl-U half-page scroll.
    ///
    /// When a count is explicitly given to Ctrl-D or Ctrl-U, that count
    /// becomes the new default for subsequent half-page scrolls.
    scroll_half_count: Option<u32>,

    /// Sticky column for vertical motions (Vim's `curswant`).
    ///
    /// Preserves the desired cursor column across vertical motions (j, k, G,
    /// gg, H, M, L, Ctrl-D/U/F/B) through lines shorter than the target
    /// column. Horizontal motions update this to the new column; `$` sets it
    /// to [`VirtualColumn::END_OF_LINE`] for end-of-line stickiness.
    sticky_column: Option<crate::primitives::VirtualColumn>,

    // ── Undo tree ────────────────────────────────────────────────
    /// Branch-aware undo history tree.
    ///
    /// Tracks undo group metadata for branch navigation and
    /// time-based undo (`:earlier`/`:later`). Skipped in serde
    /// because it must match the host's undo stack state.
    #[cfg_attr(feature = "serde", serde(skip))]
    undo_tree: UndoTree,

    /// Cursor hint for undo tree tracking.
    ///
    /// Set by the engine to the input cursor offset before effect processing;
    /// updated by `SetCursor` effects during processing. Used as
    /// `cursor_before` at `BeginUndoGroup` and `cursor_after` at `EndUndoGroup`.
    #[cfg_attr(feature = "serde", serde(skip))]
    #[default(crate::primitives::Offset::ZERO)]
    undo_cursor_hint: crate::primitives::Offset,

    /// Timestamp hint for undo tree tracking (seconds since engine start).
    ///
    /// Set by the engine before effect processing. Used when committing
    /// undo groups for time-based navigation (`:earlier Ns`).
    #[cfg_attr(feature = "serde", serde(skip))]
    #[default(0)]
    undo_timestamp_hint: u64,

    /// Variable store for g: (global) and b: (buffer-local) variables.
    ///
    /// Buffer-local variables are saved/restored during buffer switches.
    variable_store: VariableStore,

    /// Current buffer identifier, set from `InputContext` at each `process()` call.
    ///
    /// Used by the effect processor to tag jump list entries with the buffer they
    /// belong to, enabling cross-buffer Ctrl-O / Ctrl-I navigation. `None` when
    /// the host doesn't supply buffer identifiers.
    #[cfg_attr(feature = "serde", serde(skip))]
    current_buffer_id: Option<BufferId>,

    /// Multi-cursor state (selections and cursor mode).
    ///
    /// Only present when the `multi-cursor` feature is enabled.
    /// Tracks all cursor positions for algebraic effect replication.
    multi_cursor: MultiCursorState,

    // ── Bell rate limiting ──────────────────────────────────────────
    /// Consecutive bell count for rate limiting.
    ///
    /// Incremented each time an `Effect::Bell` is kept; reset to 0 when a
    /// `process_effects` call produces no bells. When this exceeds
    /// [`Self::BELL_RATE_LIMIT`], further bells are suppressed.
    #[cfg_attr(feature = "serde", serde(skip))]
    #[default(0)]
    bell_count: u8,

    /// Cached `belloff` option value.
    ///
    /// Synced from `VimOptions::belloff()` by the engine before effect
    /// processing. When `true`, all `Effect::Bell` emissions are suppressed.
    #[cfg_attr(feature = "serde", serde(skip))]
    #[default(false)]
    belloff: bool,
}

impl VimState {
    /// Current schema version constant.
    ///
    /// Increment this whenever the persistent serde layout of `VimState` changes
    /// in a breaking way (field removed, type changed, semantics altered).
    /// Forward-compatible additions (new fields with `#[serde(default)]`) do
    /// **not** require a bump.
    pub const SCHEMA_VERSION: u32 = 1;

    /// Create new `VimState` in Normal mode.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the schema version embedded in this state snapshot.
    ///
    /// Callers that load a snapshot from persistent storage should compare
    /// this value against [`Self::SCHEMA_VERSION`] and either migrate or
    /// reject snapshots with incompatible versions.
    #[inline]
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Get current mode.
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Set current mode.
    #[inline]
    pub const fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Get last find command, or `None` if no find has been recorded.
    #[inline]
    #[must_use]
    pub fn last_find(&self) -> Option<LastFind> {
        self.last_find
            .direction()
            .is_some()
            .then_some(self.last_find)
    }

    /// Set last find command.
    #[inline]
    pub const fn set_last_find(&mut self, find: LastFind) {
        self.last_find = find;
    }

    // === Register Access ===

    /// Get registers (read-only).
    #[inline]
    #[must_use]
    pub const fn registers(&self) -> &Registers {
        &self.registers
    }

    /// Get registers (mutable).
    #[inline]
    pub const fn registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    // === Mark Access ===

    /// Get marks (read-only).
    #[inline]
    #[must_use]
    pub const fn marks(&self) -> &Marks {
        &self.marks
    }

    /// Get marks (mutable).
    #[inline]
    pub const fn marks_mut(&mut self) -> &mut Marks {
        &mut self.marks
    }

    // === Jump List Access ===

    /// Get jump list (read-only).
    #[inline]
    #[must_use]
    pub const fn jump_list(&self) -> &JumpList {
        &self.jump_list
    }

    /// Get jump list (mutable).
    #[inline]
    pub const fn jump_list_mut(&mut self) -> &mut JumpList {
        &mut self.jump_list
    }

    // === Change List Access ===

    /// Get changelist (read-only).
    #[inline]
    #[must_use]
    pub const fn changelist(&self) -> &ChangeList {
        &self.changelist
    }

    /// Get changelist (mutable).
    #[inline]
    pub const fn changelist_mut(&mut self) -> &mut ChangeList {
        &mut self.changelist
    }

    // === Insert State Access ===

    /// Get insert state if in insert mode.
    #[inline]
    #[must_use]
    pub const fn insert_state(&self) -> Option<&InsertState> {
        self.insert_state.as_ref()
    }

    /// Get insert state (mutable) if in insert mode.
    #[inline]
    pub const fn insert_state_mut(&mut self) -> Option<&mut InsertState> {
        self.insert_state.as_mut()
    }

    /// Start an insert session.
    #[inline]
    pub fn start_insert(&mut self, state: InsertState) {
        self.insert_state = Some(state);
    }

    /// Take the insert session state, ending the session.
    ///
    /// Pure data operation — just removes and returns the `InsertState`.
    /// The caller is responsible for storing accumulated text via
    /// [`Self::store_last_inserted_text`] if needed.
    #[inline]
    pub const fn take_insert(&mut self) -> Option<InsertState> {
        self.insert_state.take()
    }

    /// Store text from a completed insert session for Ctrl-A and dot-repeat.
    #[inline]
    pub fn store_last_inserted_text(&mut self, text: &str) {
        self.last_inserted_text = compact_str::CompactString::from(text);
    }

    /// Get the last inserted text (for Ctrl-A).
    #[inline]
    #[must_use]
    pub fn last_inserted_text(&self) -> &str {
        &self.last_inserted_text
    }

    /// Store the entry type from a completed insert session for dot-repeat.
    ///
    /// This tells dot-repeat whether to use replace (delete+insert) or pure
    /// insert semantics when replaying the saved text.
    #[inline]
    pub const fn store_last_insert_entry_type(
        &mut self,
        entry_type: crate::primitives::InsertEntryType,
    ) {
        self.last_insert_entry_type = entry_type;
    }

    /// Get the entry type of the last completed insert session.
    #[inline]
    #[must_use]
    pub const fn last_insert_entry_type(&self) -> crate::primitives::InsertEntryType {
        self.last_insert_entry_type
    }

    /// Store per-newline auto-indent byte lengths from a completed insert session.
    ///
    /// Used by dot-repeat to strip original indent and recompute for the replay context.
    #[inline]
    pub fn store_last_insert_indent_lens(&mut self, lens: &[usize]) {
        self.last_insert_indent_lens.clear();
        self.last_insert_indent_lens.extend_from_slice(lens);
    }

    /// Get per-newline auto-indent byte lengths from the last completed insert session.
    #[inline]
    #[must_use]
    pub fn last_insert_indent_lens(&self) -> &[usize] {
        &self.last_insert_indent_lens
    }

    // === Host Insert Staging ===

    /// Stage text for `InsertKind::HostInserted`.
    ///
    /// Call this before sending the `HostInserted` command through the pipeline.
    #[inline]
    pub fn stage_host_insert(&mut self, text: &str) {
        self.pending_host_insert = compact_str::CompactString::from(text);
    }

    /// Read the staged host-insert text (without clearing).
    #[inline]
    #[must_use]
    pub(crate) fn pending_host_insert(&self) -> &str {
        &self.pending_host_insert
    }

    /// Take the staged host-insert text (returns and clears it).
    #[inline]
    pub(crate) fn take_pending_host_insert(&mut self) -> compact_str::CompactString {
        std::mem::take(&mut self.pending_host_insert)
    }

    // === Expression Register Re-evaluation ===

    /// Store the expression text from the most recent `<C-r>=` or `"=` evaluation.
    ///
    /// Called when an expression is successfully evaluated so that
    /// dot-repeat can re-evaluate the expression instead of replaying
    /// the stale cached result.
    #[inline]
    pub fn store_last_expression_text(&mut self, text: impl Into<compact_str::CompactString>) {
        self.last_expression_text = Some(text.into());
    }

    /// Get the last expression text for re-evaluation during dot-repeat.
    #[inline]
    #[must_use]
    pub fn last_expression_text(&self) -> Option<&str> {
        self.last_expression_text.as_deref()
    }

    // === Visual State Access ===

    /// Get last visual info (for gv and . repeat).
    #[inline]
    #[must_use]
    pub const fn last_visual(&self) -> Option<LastVisualInfo> {
        self.last_visual
    }

    /// Set last visual info (called on visual exit).
    #[inline]
    pub const fn set_last_visual(&mut self, info: LastVisualInfo) {
        self.last_visual = Some(info);
    }

    /// Take the last visual info, returning it and clearing the field.
    ///
    /// Used by `on_buffer_leave` to extract per-buffer visual state.
    #[inline]
    pub const fn take_last_visual(&mut self) -> Option<LastVisualInfo> {
        self.last_visual.take()
    }

    /// Set last visual info from an `Option` (restore path).
    ///
    /// Unlike `set_last_visual` which takes a non-optional value,
    /// this accepts `Option` for restoring state that may be `None`
    /// (buffer never entered visual mode).
    #[inline]
    pub const fn set_last_visual_opt(&mut self, info: Option<LastVisualInfo>) {
        self.last_visual = info;
    }

    // === Repeat State Access ===

    /// Get repeat state (read-only).
    #[inline]
    #[must_use]
    pub const fn repeat_state(&self) -> &RepeatState {
        &self.repeat
    }

    /// Get repeat state (mutable).
    #[inline]
    pub const fn repeat_state_mut(&mut self) -> &mut RepeatState {
        &mut self.repeat
    }

    /// Get the last text-mutating ex command for dot-repeat.
    #[inline]
    #[must_use]
    pub fn last_ex_for_dot(&self) -> Option<&str> {
        self.last_ex_for_dot.as_deref()
    }

    /// Set the last text-mutating ex command for dot-repeat.
    #[inline]
    pub fn set_last_ex_for_dot(&mut self, cmd: Option<compact_str::CompactString>) {
        self.last_ex_for_dot = cmd;
    }

    /// Get the cached search count result if the pattern matches.
    #[inline]
    #[must_use]
    pub fn search_count_cache(&self, pattern_hash: u64) -> Option<&SearchCountCache> {
        self.search_count_cache
            .as_ref()
            .filter(|c| c.pattern_hash == pattern_hash)
    }

    /// Store a search count cache entry.
    #[inline]
    pub const fn set_search_count_cache(&mut self, cache: SearchCountCache) {
        self.search_count_cache = Some(cache);
    }

    /// Invalidate the search count cache (called on text mutation).
    #[inline]
    pub const fn invalidate_search_count_cache(&mut self) {
        self.search_count_cache = None;
    }

    // === Macro State Access ===

    /// Get macro state (read-only).
    #[inline]
    #[must_use]
    pub const fn macros(&self) -> &MacroState {
        &self.macros
    }

    /// Get macro state (mutable).
    #[inline]
    pub const fn macros_mut(&mut self) -> &mut MacroState {
        &mut self.macros
    }

    // === Search State Access ===

    /// Get search state (read-only).
    #[inline]
    #[must_use]
    pub const fn search(&self) -> &SearchState {
        &self.search
    }

    /// Get search state (mutable).
    #[inline]
    pub const fn search_mut(&mut self) -> &mut SearchState {
        &mut self.search
    }

    // === Substitute Confirm State Access ===

    /// Get substitute confirm state (read-only).
    ///
    /// Returns `Some` when an interactive `:s///c` session is active.
    #[inline]
    #[must_use]
    pub const fn substitute_confirm(&self) -> Option<&SubstituteConfirmState> {
        self.substitute_confirm.as_ref()
    }

    /// Get substitute confirm state (mutable).
    #[inline]
    pub const fn substitute_confirm_mut(&mut self) -> Option<&mut SubstituteConfirmState> {
        self.substitute_confirm.as_mut()
    }

    /// Start an interactive substitute confirm session.
    #[inline]
    pub fn set_substitute_confirm(&mut self, state: SubstituteConfirmState) {
        self.substitute_confirm = Some(state);
    }

    /// Take the substitute confirm state, returning it and clearing the field.
    #[inline]
    pub const fn take_substitute_confirm(&mut self) -> Option<SubstituteConfirmState> {
        self.substitute_confirm.take()
    }

    /// Clear the substitute confirm state.
    #[inline]
    pub fn clear_substitute_confirm(&mut self) {
        self.substitute_confirm = None;
    }

    // === Undo auto-grouping state ===

    /// Timestamp (ms) of the last edit under an auto-opened undo group.
    #[must_use]
    pub const fn undo_auto_group_last_edit_ms(&self) -> u64 {
        self.undo_auto_group_last_edit_ms
    }

    /// Set the timestamp of the last edit under an auto-opened undo group.
    pub const fn set_undo_auto_group_last_edit_ms(&mut self, ms: u64) {
        self.undo_auto_group_last_edit_ms = ms;
    }

    /// Whether the current undo group was auto-opened by the time-based fallback.
    #[must_use]
    pub const fn undo_auto_group_active(&self) -> bool {
        self.undo_auto_group_active
    }

    /// Set whether the current undo group was auto-opened.
    pub const fn set_undo_auto_group_active(&mut self, active: bool) {
        self.undo_auto_group_active = active;
    }

    // === ChangeSet-based position remapping ===

    /// Remap all position-tracking subsystems through a [`ChangeSet`].
    ///
    /// Subsystems remapped: marks (special only), jump list, change list.
    pub(crate) fn remap_all_positions(
        &mut self,
        changeset: &crate::primitives::changeset::ChangeSet,
    ) {
        use super::remap::RemapPositions;

        self.marks.remap(changeset);
        self.jump_list.remap(changeset);
        self.changelist.remap(changeset);
    }

    // === Syntax Selection History Access ===

    /// Get syntax selection history (read-only).
    #[inline]
    #[must_use]
    pub const fn syntax_selection(&self) -> &SyntaxSelectionHistory {
        &self.syntax_selection
    }

    /// Get syntax selection history (mutable).
    #[inline]
    pub const fn syntax_selection_mut(&mut self) -> &mut SyntaxSelectionHistory {
        &mut self.syntax_selection
    }

    // === Command-Line State Access ===

    /// Get command-line state (read-only).
    #[inline]
    #[must_use]
    pub const fn command_line(&self) -> &CommandLineState {
        &self.command_line
    }

    /// Get command-line state (mutable).
    #[inline]
    pub const fn command_line_mut(&mut self) -> &mut CommandLineState {
        &mut self.command_line
    }

    // === Message History Access ===

    /// Get message history (read-only).
    #[inline]
    #[must_use]
    pub const fn message_history(&self) -> &MessageHistory {
        &self.message_history
    }

    /// Get message history (mutable).
    #[inline]
    pub const fn message_history_mut(&mut self) -> &mut MessageHistory {
        &mut self.message_history
    }

    // === Multi-Cursor Access ===

    /// Get multi-cursor state (read-only).
    #[inline]
    #[must_use]
    pub const fn multi_cursor(&self) -> &MultiCursorState {
        &self.multi_cursor
    }

    /// Get multi-cursor state (mutable).
    #[inline]
    pub const fn multi_cursor_mut(&mut self) -> &mut MultiCursorState {
        &mut self.multi_cursor
    }

    // === Bell Rate Limiting ===

    /// Maximum consecutive bells before throttling suppresses further emissions.
    pub const BELL_RATE_LIMIT: u8 = 3;

    /// Current consecutive bell count.
    #[inline]
    #[must_use]
    pub const fn bell_count(&self) -> u8 {
        self.bell_count
    }

    /// Increment the bell counter. Returns the new count.
    #[inline]
    pub const fn increment_bell_count(&mut self) -> u8 {
        self.bell_count = self.bell_count.saturating_add(1);
        self.bell_count
    }

    /// Reset the bell counter to zero.
    #[inline]
    pub const fn reset_bell_count(&mut self) {
        self.bell_count = 0;
    }

    /// Whether `belloff=all` is active (all bells suppressed).
    #[inline]
    #[must_use]
    pub const fn belloff(&self) -> bool {
        self.belloff
    }

    /// Set the cached belloff value (synced from VimOptions).
    #[inline]
    pub const fn set_belloff(&mut self, value: bool) {
        self.belloff = value;
    }

    // === Transient Output ===

    /// Get the current status message (if any).
    #[inline]
    #[must_use]
    pub const fn message(&self) -> Option<&StatusMessage> {
        self.message.as_ref()
    }

    /// Set the status message.
    #[inline]
    pub fn set_message(&mut self, msg: StatusMessage) {
        self.message = Some(msg);
    }

    /// Clear the status message.
    #[inline]
    pub fn clear_message(&mut self) {
        self.message = None;
    }

    /// Get the current scroll hint (if any).
    #[inline]
    #[must_use]
    pub const fn scroll_hint(&self) -> Option<ScrollHint> {
        self.scroll_hint
    }

    /// Set a scroll hint.
    #[inline]
    pub const fn set_scroll_hint(&mut self, hint: ScrollHint) {
        self.scroll_hint = Some(hint);
    }

    /// Clear transient per-keystroke output.
    ///
    /// Called at the start of each `process()` call so the host
    /// sees only the output from the current keystroke.
    #[inline]
    pub fn clear_transient(&mut self) {
        self.message = None;
        self.scroll_hint = None;
    }

    // === Return-To (Ctrl-O one-shot) ===

    /// Get the mode to return to after a one-shot normal command.
    #[inline]
    #[must_use]
    pub const fn return_to(&self) -> ReturnTo {
        self.return_to
    }

    /// Set the mode to return to after a one-shot normal command.
    #[inline]
    pub const fn set_return_to(&mut self, return_to: ReturnTo) {
        self.return_to = return_to;
    }

    /// Take the return-to value, resetting it to `ReturnTo::None`.
    ///
    /// Used by the engine after executing a one-shot command to consume
    /// the pending return and restore the original mode.
    #[inline]
    pub const fn take_return_to(&mut self) -> ReturnTo {
        let ret = self.return_to;
        self.return_to = ReturnTo::None;
        ret
    }

    /// Whether the `.` mark has been set in the current undo group.
    #[inline]
    #[must_use]
    pub const fn last_change_mark_set(&self) -> bool {
        self.last_change_mark_set
    }

    /// Record that the `.` mark was set (called by effect processor).
    #[inline]
    pub const fn set_last_change_mark_set(&mut self, val: bool) {
        self.last_change_mark_set = val;
    }

    /// Whether the `[` (change start) mark has been set in the current undo group.
    #[inline]
    #[must_use]
    pub const fn change_start_mark_set(&self) -> bool {
        self.change_start_mark_set
    }

    /// Record that the `[` mark was set (called by effect processor).
    #[inline]
    pub const fn set_change_start_mark_set(&mut self, val: bool) {
        self.change_start_mark_set = val;
    }

    /// Whether a new undo-able change has started (Neovim's `b_new_change`).
    ///
    /// When `true`, the next text mutation creates a **new** changelist entry.
    /// When `false`, text mutations only update the last entry.
    #[inline]
    #[must_use]
    pub const fn changelist_new_change(&self) -> bool {
        self.changelist_new_change
    }

    /// Set the new-change flag (called by `BeginUndoGroup` processing).
    #[inline]
    pub const fn set_changelist_new_change(&mut self, val: bool) {
        self.changelist_new_change = val;
    }

    /// Sticky count for Ctrl-D/Ctrl-U.
    #[inline]
    #[must_use]
    pub const fn scroll_half_count(&self) -> Option<u32> {
        self.scroll_half_count
    }

    /// Set sticky count for Ctrl-D/Ctrl-U.
    #[inline]
    pub const fn set_scroll_half_count(&mut self, count: u32) {
        self.scroll_half_count = Some(count);
    }

    /// Set or clear sticky count for Ctrl-D/Ctrl-U.
    /// Used by `on_buffer_enter`/`on_buffer_leave` for per-buffer save/restore.
    #[inline]
    pub const fn set_scroll_half_count_opt(&mut self, count: Option<u32>) {
        self.scroll_half_count = count;
    }

    /// Sticky column for vertical motions (curswant).
    #[inline]
    #[must_use]
    pub const fn sticky_column(&self) -> Option<crate::primitives::VirtualColumn> {
        self.sticky_column
    }

    /// Set sticky column for vertical motions.
    #[inline]
    pub const fn set_sticky_column(&mut self, col: Option<crate::primitives::VirtualColumn>) {
        self.sticky_column = col;
    }

    // === Undo Tree Access ===

    /// Get undo tree (read-only).
    #[inline]
    #[must_use]
    pub const fn undo_tree(&self) -> &UndoTree {
        &self.undo_tree
    }

    /// Get undo tree (mutable).
    #[inline]
    pub const fn undo_tree_mut(&mut self) -> &mut UndoTree {
        &mut self.undo_tree
    }

    /// Undo one step, swapping mark snapshots atomically.
    ///
    /// Splits the borrow between `undo_tree` and `marks` so the tree's
    /// `undo()` can restore mark state in the same `&mut self` call.
    /// Returns `None` if already at the root (nothing to undo).
    #[inline]
    pub fn undo_with_marks(&mut self) -> Option<UndoStep> {
        self.undo_tree.undo(&mut self.marks, &mut self.last_visual)
    }

    /// Redo one step, swapping mark snapshots atomically.
    ///
    /// Splits the borrow between `undo_tree` and `marks` so the tree's
    /// `redo()` can restore mark state in the same `&mut self` call.
    /// Returns `None` if already at the leaf (nothing to redo).
    #[inline]
    pub fn redo_with_marks(&mut self) -> Option<UndoStep> {
        self.undo_tree.redo(&mut self.marks, &mut self.last_visual)
    }

    /// Get cursor hint for undo tree tracking.
    #[inline]
    #[must_use]
    pub const fn undo_cursor_hint(&self) -> crate::primitives::Offset {
        self.undo_cursor_hint
    }

    /// Set cursor hint for undo tree tracking.
    #[inline]
    pub const fn set_undo_cursor_hint(&mut self, offset: crate::primitives::Offset) {
        self.undo_cursor_hint = offset;
    }

    /// Get timestamp hint for undo tree tracking.
    #[inline]
    #[must_use]
    pub const fn undo_timestamp_hint(&self) -> u64 {
        self.undo_timestamp_hint
    }

    /// Set timestamp hint for undo tree tracking.
    #[inline]
    pub const fn set_undo_timestamp_hint(&mut self, secs: u64) {
        self.undo_timestamp_hint = secs;
    }

    // === Variable Store Access ===

    /// Get variable store (read-only).
    #[inline]
    #[must_use]
    pub const fn variable_store(&self) -> &VariableStore {
        &self.variable_store
    }

    /// Get variable store (mutable).
    #[inline]
    pub const fn variable_store_mut(&mut self) -> &mut VariableStore {
        &mut self.variable_store
    }

    // === Buffer Identity ===

    /// Get the current buffer identifier.
    ///
    /// Returns `None` if the host hasn't supplied a buffer ID.
    #[inline]
    #[must_use]
    pub const fn current_buffer_id(&self) -> Option<BufferId> {
        self.current_buffer_id
    }

    /// Set the current buffer identifier.
    ///
    /// Called by the engine at the start of each `process()` call, syncing
    /// from the `InputContext` provided by the host.
    #[inline]
    pub const fn set_current_buffer_id(&mut self, id: Option<BufferId>) {
        self.current_buffer_id = id;
    }

    /// Reset transient state (mode, insert, message, scroll, command line).
    /// All other fields (registers, marks, lists, search, macros, undo, etc.) are preserved.
    pub fn reset(&mut self) {
        self.mode = Mode::Normal;
        self.insert_state = None;
        self.substitute_confirm = None;
        self.message = None;
        self.scroll_hint = None;
        self.return_to = ReturnTo::None;
        self.command_line.clear();
    }

    /// Full reset including registers and marks.
    pub fn reset_all(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::InsertEntryType;

    #[test]
    fn test_default_is_normal() {
        let state = VimState::new();
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn test_take_insert_and_store_text() {
        let mut state = VimState::new();
        let mut insert = InsertState::new(InsertEntryType::BeforeCursor);
        insert.push_str("hello");
        state.start_insert(insert);

        let taken = state.take_insert().unwrap();
        state.store_last_inserted_text(taken.accumulated_text());
        assert_eq!(state.last_inserted_text(), "hello");
        assert!(state.insert_state().is_none());
    }

    #[test]
    fn test_store_empty_clears_previous() {
        let mut state = VimState::new();

        // First insert with text
        let mut insert1 = InsertState::new(InsertEntryType::BeforeCursor);
        insert1.push_str("first");
        state.start_insert(insert1);
        let taken1 = state.take_insert().unwrap();
        state.store_last_inserted_text(taken1.accumulated_text());

        // Second insert with no text
        let insert2 = InsertState::new(InsertEntryType::BeforeCursor);
        state.start_insert(insert2);
        let taken2 = state.take_insert().unwrap();
        state.store_last_inserted_text(taken2.accumulated_text());

        // Empty inserts should now clear the register
        assert_eq!(state.last_inserted_text(), "");
    }

    #[test]
    fn test_reset_preserves_registers_marks() {
        let mut state = VimState::new();
        state.set_mode(Mode::Insert);
        state.registers_mut().set(
            crate::primitives::RegisterName::new_unchecked('a'),
            crate::primitives::RegisterContent::char_wise("test"),
        );

        state.reset();

        assert_eq!(state.mode(), Mode::Normal);
        assert!(
            state
                .registers()
                .get(crate::primitives::RegisterName::new_unchecked('a'))
                .is_some(),
            "registers should persist"
        );
    }

    #[test]
    fn test_reset_all_clears_everything() {
        let mut state = VimState::new();
        state.registers_mut().set(
            crate::primitives::RegisterName::new_unchecked('a'),
            crate::primitives::RegisterContent::char_wise("test"),
        );

        state.reset_all();

        assert!(
            state
                .registers()
                .get(crate::primitives::RegisterName::new_unchecked('a'))
                .is_none(),
            "registers should be cleared"
        );
    }

    #[test]
    fn test_mode_roundtrip() {
        let mut state = VimState::new();
        state.set_mode(Mode::Insert);
        assert_eq!(state.mode(), Mode::Insert);

        state.set_mode(Mode::Visual(crate::primitives::VisualType::Line));
        assert!(matches!(state.mode(), Mode::Visual(_)));
    }

    #[test]
    fn test_mutable_accessors() {
        let mut state = VimState::new();

        // Search
        state
            .search_mut()
            .set_pattern("foo", crate::primitives::SearchDirection::Forward);
        assert_eq!(state.search().pattern(), Some("foo"));

        // Marks
        state.marks_mut().set(
            crate::primitives::MarkName::new('a').unwrap(),
            crate::primitives::Mark::from_raw(42),
        );
        assert_eq!(
            state
                .marks()
                .get(crate::primitives::MarkName::new('a').unwrap())
                .unwrap()
                .offset()
                .get(),
            42
        );

        // Jump list
        state
            .jump_list_mut()
            .push(crate::primitives::Offset::new(100), None);
        assert!(!state.jump_list().is_empty());
    }

    #[test]
    fn test_last_find_storage() {
        let mut state = VimState::new();
        assert!(state.last_find().is_none());

        let mut lf = LastFind::new();
        lf.record(crate::primitives::FindDirection::FindForward, 'x');
        state.set_last_find(lf);
        let got = state.last_find().unwrap();
        assert_eq!(
            got.direction(),
            Some(crate::primitives::FindDirection::FindForward)
        );
        assert_eq!(got.target_char(), Some('x'));
    }

    /// Serde round-trip: serialize VimState to JSON, deserialize back, verify fields match.
    ///
    /// Transient fields (message, scroll_hint) are skipped — they should be None after deser.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_roundtrip() {
        let state = populated_state_for_serde();
        let json = serde_json::to_string(&state).expect("serialize VimState");
        let restored: VimState = serde_json::from_str(&json).expect("deserialize VimState");
        assert_roundtripped_state(&restored);
    }

    #[cfg(feature = "serde")]
    fn populated_state_for_serde() -> VimState {
        let mut state = VimState::new();
        populate_persistent_fields(&mut state);
        populate_transient_fields(&mut state);
        state
    }

    #[cfg(feature = "serde")]
    fn populate_persistent_fields(state: &mut VimState) {
        state.set_mode(Mode::Insert);
        state
            .search_mut()
            .set_pattern("hello", crate::primitives::SearchDirection::Forward);
        state.registers_mut().set(
            crate::primitives::RegisterName::new_unchecked('a'),
            crate::primitives::RegisterContent::char_wise("yanked text"),
        );
        state.marks_mut().set(
            crate::primitives::MarkName::new('b').unwrap(),
            crate::primitives::Mark::from_raw(99),
        );
        state
            .jump_list_mut()
            .push(crate::primitives::Offset::new(42), None);
        state.store_last_inserted_text("previously typed");
        state.set_last_visual(LastVisualInfo::char_wise(3));

        let mut lf = LastFind::new();
        lf.record(crate::primitives::FindDirection::FindForward, 'z');
        state.set_last_find(lf);
    }

    #[cfg(feature = "serde")]
    fn populate_transient_fields(state: &mut VimState) {
        state.set_message(StatusMessage::Info("transient msg".into()));
        state.set_scroll_hint(ScrollHint::CenterCursor);
    }

    #[cfg(feature = "serde")]
    fn assert_roundtripped_state(restored: &VimState) {
        assert_eq!(restored.mode(), Mode::Insert);
        assert_eq!(restored.search().pattern(), Some("hello"));
        assert!(restored
            .registers()
            .get(crate::primitives::RegisterName::new_unchecked('a'))
            .is_some());
        assert_eq!(
            restored
                .marks()
                .get(crate::primitives::MarkName::new('b').unwrap())
                .unwrap()
                .offset()
                .get(),
            99
        );
        assert!(!restored.jump_list().is_empty());
        assert_eq!(restored.last_inserted_text(), "previously typed");
        assert_eq!(
            restored.last_visual().unwrap().visual_type(),
            crate::primitives::VisualType::Char
        );

        let got_find = restored.last_find().unwrap();
        assert_eq!(
            got_find.direction(),
            Some(crate::primitives::FindDirection::FindForward)
        );
        assert_eq!(got_find.target_char(), Some('z'));
        assert!(restored.message().is_none());
        assert!(restored.scroll_hint().is_none());
    }

    /// Round-trip verifies that `schema_version` survives serialize → deserialize.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_schema_version_roundtrip() {
        let state = VimState::new();
        assert_eq!(state.schema_version(), VimState::SCHEMA_VERSION);

        let json = serde_json::to_string(&state).expect("serialize VimState");
        let restored: VimState = serde_json::from_str(&json).expect("deserialize VimState");

        assert_eq!(restored.schema_version(), VimState::SCHEMA_VERSION);
        assert_eq!(restored.schema_version(), 1);
    }

    /// A JSON blob that omits `schema_version` must deserialize successfully and
    /// default the field to 1 (the current version) via `#[serde(default)]`.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_missing_schema_version_defaults_to_one() {
        // Minimal valid JSON for VimState — no schema_version key at all.
        let json = r#"{}"#;
        let state: VimState = serde_json::from_str(json).expect("deserialize empty object");
        assert_eq!(state.schema_version(), 1);
    }

    /// A JSON blob with an unknown field must deserialize without error.
    ///
    /// `deny_unknown_fields` is intentionally NOT set on `VimState`, so that
    /// newer snapshots (with extra keys added by future versions) can be loaded
    /// by older code gracefully.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_unknown_field_is_ignored() {
        let json = r#"{"schema_version": 1, "future_field_not_yet_known": true}"#;
        let result: Result<VimState, _> = serde_json::from_str(json);
        assert!(
            result.is_ok(),
            "unknown fields must be silently ignored — got: {result:?}"
        );
        let state = result.unwrap();
        assert_eq!(state.schema_version(), 1);
    }

    /// Deserializing a completely empty JSON object `{}` must produce a valid
    /// `VimState` with all fields at their `SmartDefault` values — in particular
    /// `Mode::Normal` and `schema_version == 1`.
    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_empty_object_produces_valid_default_state() {
        let json = r#"{}"#;
        let state: VimState = serde_json::from_str(json).expect("deserialize empty object");

        // Core invariants from SmartDefault
        assert_eq!(state.mode(), Mode::Normal, "default mode must be Normal");
        assert_eq!(
            state.schema_version(),
            1,
            "default schema_version must be 1"
        );

        // Spot-check a few other defaulted fields
        assert!(state.last_find().is_none(), "no last find by default");
        assert!(state.insert_state().is_none(), "no insert state by default");
        assert_eq!(
            state.last_inserted_text(),
            "",
            "no inserted text by default"
        );
        assert!(state.message().is_none(), "no message by default");
        assert!(state.scroll_hint().is_none(), "no scroll hint by default");
    }

    #[test]
    fn test_return_to_default_is_none() {
        let state = VimState::new();
        assert_eq!(state.return_to(), ReturnTo::None);
    }

    #[test]
    fn test_set_return_to() {
        let mut state = VimState::new();
        state.set_return_to(ReturnTo::Insert);
        assert_eq!(state.return_to(), ReturnTo::Insert);

        state.set_return_to(ReturnTo::Replace);
        assert_eq!(state.return_to(), ReturnTo::Replace);

        state.set_return_to(ReturnTo::None);
        assert_eq!(state.return_to(), ReturnTo::None);
    }

    #[test]
    fn test_take_return_to() {
        let mut state = VimState::new();

        // take from None returns None and stays None
        assert_eq!(state.take_return_to(), ReturnTo::None);
        assert_eq!(state.return_to(), ReturnTo::None);

        // set then take returns the value and resets to None
        state.set_return_to(ReturnTo::Insert);
        let taken = state.take_return_to();
        assert_eq!(taken, ReturnTo::Insert);
        assert_eq!(state.return_to(), ReturnTo::None);

        // works for all variants
        state.set_return_to(ReturnTo::VirtualReplace);
        assert_eq!(state.take_return_to(), ReturnTo::VirtualReplace);
        assert_eq!(state.return_to(), ReturnTo::None);
    }

    #[test]
    fn test_reset_clears_return_to() {
        let mut state = VimState::new();
        state.set_return_to(ReturnTo::Insert);
        state.reset();
        assert_eq!(state.return_to(), ReturnTo::None);
    }

    #[test]
    fn test_reset_all_clears_return_to() {
        let mut state = VimState::new();
        state.set_return_to(ReturnTo::Replace);
        state.reset_all();
        assert_eq!(state.return_to(), ReturnTo::None);
    }

    // === Exchange State Tests ===

    // === Syntax Selection History Tests ===

    #[test]
    fn test_syntax_selection_default_is_empty() {
        let state = VimState::new();
        assert!(state.syntax_selection().is_empty());
    }

    #[test]
    fn test_syntax_selection_push_pop_via_accessors() {
        let mut state = VimState::new();
        state
            .syntax_selection_mut()
            .push(crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(
                    crate::primitives::Offset::new(0),
                    crate::primitives::Offset::new(10),
                ),
            ));
        state
            .syntax_selection_mut()
            .push(crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(
                    crate::primitives::Offset::new(0),
                    crate::primitives::Offset::new(20),
                ),
            ));

        assert!(!state.syntax_selection().is_empty());

        let popped = state.syntax_selection_mut().pop();
        assert!(popped.is_some());
        let popped = popped.unwrap();
        assert_eq!(popped.primary().start(), crate::primitives::Offset::new(0));
        assert_eq!(popped.primary().end(), crate::primitives::Offset::new(20));
    }

    #[test]
    fn test_syntax_selection_cleared_by_reset_all() {
        let mut state = VimState::new();
        state
            .syntax_selection_mut()
            .push(crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(
                    crate::primitives::Offset::new(5),
                    crate::primitives::Offset::new(15),
                ),
            ));
        assert!(!state.syntax_selection().is_empty());

        state.reset_all();
        assert!(state.syntax_selection().is_empty());
    }

    /// Exhaustive field inventory for [`VimState`].
    ///
    /// Adding a new field causes a compile error here until it is
    /// categorized. This is the compile-time guarantee that
    /// `BufferLocalState` stays complete.
    ///
    /// Categories:
    ///   global      — session-level, never saved per-buffer
    ///   buffer      — saved/restored via `BufferLocalState`
    ///   host_buffer — per-buffer but managed by the host (not in BufferLocalState)
    ///   transient   — per-keystroke or per-undo-group, cleared by reset paths
    #[test]
    fn buffer_local_state_field_inventory() {
        #[allow(unused, unreachable_code)]
        fn check(s: VimState) {
            let VimState {
                // ── Versioning ───────────────────────────────────────
                schema_version: _, // serde forward-compatibility version stamp

                // ── Global (session-level, never saved per-buffer) ────
                mode: _,
                last_find: _,
                registers: _,
                jump_list: _,
                insert_state: _,
                last_inserted_text: _,
                last_insert_entry_type: _,
                last_insert_indent_lens: _,
                pending_host_insert: _,
                repeat: _,
                last_ex_for_dot: _, // transient: last text-mutating ex for dot-repeat
                macros: _,
                search: _,
                command_line: _,
                message_history: _,

                // ── Buffer-local (saved/restored via BufferLocalState) ──
                marks: _,              // partitioned: local+special = buffer, A-Z = global
                changelist: _,         // entirely per-buffer
                last_visual: _,        // per-buffer (visual marks are buffer-scoped)
                sticky_column: _,      // per-buffer (superior to Neovim's per-window)
                substitute_confirm: _, // transient (active confirm session)
                scroll_half_count: _,  // per-buffer (Vim :help scroll)
                undo_tree: _,          // saved/restored via BufferLocalState
                variable_store: _,     // g: global + b: buffer-local variables

                // ── Transient (per-keystroke / per-undo-group) ───────
                message: _,
                scroll_hint: _,
                return_to: _,
                last_change_mark_set: _,
                change_start_mark_set: _,
                changelist_new_change: _,
                undo_cursor_hint: _,
                undo_timestamp_hint: _,
                undo_auto_group_last_edit_ms: _,
                undo_auto_group_active: _,
                current_buffer_id: _,
                syntax_selection: _,

                // ── Feature-gated ───────────────────────────────────
                multi_cursor: _,

                // ── Bell rate limiting (transient) ──────────────────
                bell_count: _,
                belloff: _,

                // ── Search count cache (transient) ──────────────────
                search_count_cache: _,

                // ── Expression register (transient) ────────────────
                last_expression_text: _,
            } = s;
        }
    }

    // === SearchCountCache tests ===

    fn make_cache(current: u32, total: u32, complete: bool) -> SearchCountCache {
        SearchCountCache {
            pattern_hash: 42,
            current,
            total,
            complete,
        }
    }

    #[test]
    fn try_advance_forward_no_wrap() {
        let cache = make_cache(2, 5, true);
        let result = cache.try_advance(1, true).unwrap();
        assert_eq!(result.current, 3);
        assert_eq!(result.total, 5);
        assert!(result.complete);
    }

    #[test]
    fn try_advance_forward_wraps() {
        let cache = make_cache(5, 5, true);
        let result = cache.try_advance(1, true).unwrap();
        assert_eq!(result.current, 1);
        assert_eq!(result.total, 5);
    }

    #[test]
    fn try_advance_backward_no_wrap() {
        let cache = make_cache(3, 5, true);
        let result = cache.try_advance(1, false).unwrap();
        assert_eq!(result.current, 2);
        assert_eq!(result.total, 5);
    }

    #[test]
    fn try_advance_backward_wraps() {
        let cache = make_cache(1, 5, true);
        let result = cache.try_advance(1, false).unwrap();
        assert_eq!(result.current, 5);
        assert_eq!(result.total, 5);
    }

    #[test]
    fn try_advance_forward_count_3() {
        let cache = make_cache(2, 5, true);
        let result = cache.try_advance(3, true).unwrap();
        // 2 + 3 = 5
        assert_eq!(result.current, 5);
        assert_eq!(result.total, 5);
    }

    #[test]
    fn try_advance_forward_count_wraps_past_total() {
        let cache = make_cache(4, 5, true);
        let result = cache.try_advance(3, true).unwrap();
        // (4 - 1 + 3) % 5 + 1 = 6 % 5 + 1 = 2
        assert_eq!(result.current, 2);
    }

    #[test]
    fn try_advance_backward_count_wraps_past_zero() {
        let cache = make_cache(2, 5, true);
        let result = cache.try_advance(3, false).unwrap();
        // (2 - 1 + 5 - 3) % 5 + 1 = (1 + 2) % 5 + 1 = 4
        assert_eq!(result.current, 4);
    }

    #[test]
    fn try_advance_incomplete_returns_none() {
        let cache = make_cache(2, 5, false);
        assert!(cache.try_advance(1, true).is_none());
    }

    #[test]
    fn try_advance_zero_total_returns_none() {
        let cache = make_cache(0, 0, true);
        assert!(cache.try_advance(1, true).is_none());
    }

    #[test]
    fn try_advance_single_match_forward() {
        let cache = make_cache(1, 1, true);
        let result = cache.try_advance(1, true).unwrap();
        assert_eq!(result.current, 1);
        assert_eq!(result.total, 1);
    }

    #[test]
    fn try_advance_single_match_backward() {
        let cache = make_cache(1, 1, true);
        let result = cache.try_advance(1, false).unwrap();
        assert_eq!(result.current, 1);
        assert_eq!(result.total, 1);
    }

    #[test]
    fn cache_lookup_matching_hash() {
        let mut state = VimState::default();
        let cache = make_cache(3, 10, true);
        state.set_search_count_cache(cache);
        assert!(state.search_count_cache(42).is_some());
    }

    #[test]
    fn cache_lookup_mismatching_hash() {
        let mut state = VimState::default();
        let cache = make_cache(3, 10, true);
        state.set_search_count_cache(cache);
        assert!(state.search_count_cache(99).is_none());
    }

    #[test]
    fn cache_invalidation_clears() {
        let mut state = VimState::default();
        let cache = make_cache(3, 10, true);
        state.set_search_count_cache(cache);
        state.invalidate_search_count_cache();
        assert!(state.search_count_cache(42).is_none());
    }
}
