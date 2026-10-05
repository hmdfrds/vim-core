//! Public API surface for [`VimEngine`].
//!
//! Getters, setters, configuration methods, provider registration,
//! pipeline/session/shadow APIs, and trait impls.

use super::coordinators::{HostCoordinator, RecordingState, TypeaheadCoordinator};
use super::providers::EngineProviders;
use super::VimEngine;
use crate::effects::{Effect, EffectPipeline, Effects};
use crate::execution::replay::SessionRecorder;
use crate::execution::response::Response;
use crate::execution::trace::trace_event;
#[cfg(feature = "engine-tracing")]
use crate::execution::trace::TraceEvent;
use crate::grammar::Parser;
use crate::keymap::{Keymap, MappingEntry, MappingKind, MappingMode, MappingOwner};
use crate::mode::ModeDispatcher;
use crate::primitives::Mode;
use crate::primitives::{
    KeyHint, KeyHintsInfo, LineNumber, OptionId, OptionOverrides, OptionValue, RegisterName,
    StickyTarget, VimOptions, VisualType,
};
use crate::state::VimState;

// ── Host Extension Registration ───────────────────────────────────────────────

/// A single key mapping contributed by a host extension.
///
/// Host extensions (e.g., LSP features, IDE features) use this struct to
/// register key mappings through [`VimEngine::register_host_mappings`].
/// All mappings contributed by the same extension are identified by the
/// extension name string so they can be atomically removed via
/// [`VimEngine::unregister_host_mappings`].
///
/// # Example
///
/// ```ignore
/// use vim_core::execution::HostMapping;
/// use vim_core::keymap::MappingMode;
/// use compact_str::CompactString;
///
/// let mapping = HostMapping {
///     modes: smallvec::smallvec![MappingMode::Normal],
///     lhs: CompactString::from("gd"),
///     rhs: CompactString::from("<Action>(gotoDefinition)"),
///     recursive: false,
///     silent: true,
///     description: Some(CompactString::from("Go to definition")),
/// };
/// engine.register_host_mappings("my-lsp", &[mapping]);
/// ```
pub struct HostMapping {
    /// The modes in which this mapping is active.
    ///
    /// Use `smallvec::smallvec![MappingMode::Normal]` for a single-mode
    /// mapping, or list multiple modes to register the same binding in all.
    pub modes: smallvec::SmallVec<[MappingMode; 4]>,
    /// The left-hand side key sequence in Vim key notation (e.g., `"gd"`, `"<C-]>"`).
    pub lhs: compact_str::CompactString,
    /// The right-hand side key sequence in Vim key notation (e.g., `"<Action>(gotoDefinition)"`).
    pub rhs: compact_str::CompactString,
    /// Whether the mapping is recursive (`true` → `:map`, `false` → `:noremap`).
    ///
    /// Host mappings that invoke `<Action>(...)` keys should be non-recursive
    /// (set `recursive: false`) to prevent accidental re-expansion.
    pub recursive: bool,
    /// Whether `<silent>` is set — suppresses `ShowMessage` effects during expansion.
    pub silent: bool,
    /// Optional human-readable description for which-key display.
    pub description: Option<compact_str::CompactString>,
}
impl VimEngine {
    /// Get current mode.
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.state.mode()
    }

    /// Snapshot of all recommended editor settings for the current state.
    ///
    /// Returns cursor style, line-mode flag, EOL clipping, input routing,
    /// relative line numbers, and mode appearance in a single call so the
    /// host does not need to derive each value independently.
    ///
    /// Uses the resolved (global + buffer + window override) value of
    /// `relativenumber`.
    #[inline]
    #[must_use]
    pub const fn host_settings(&self) -> crate::primitives::HostSettings {
        crate::primitives::HostSettings::from_mode(
            self.state.mode(),
            self.resolved_options.relativenumber(),
        )
    }

    /// Extended mode string combining `Mode::vim_string()` with parser state.
    ///
    /// Returns a richer sub-mode context string matching Vim's `mode(1)`:
    /// - `"ic"` / `"ix"` when in Ctrl-X completion sub-modes
    /// - `"niI"` when in Ctrl-O temporary normal mode (return-to Insert)
    /// - `"niR"` when in Ctrl-O temporary normal mode (return-to Replace)
    /// - `"niV"` when in Ctrl-O temporary normal mode (return-to VirtualReplace)
    /// - `"nov"` / `"noV"` / `"no\x16"` when operator-pending has forced motion
    /// - Base `Mode::vim_string()` otherwise
    #[must_use]
    pub fn mode_string(&self) -> &'static str {
        use crate::grammar::InputState;
        use crate::primitives::ReturnTo;

        let mode = self.state.mode();
        let parser_state = self.parser.state();
        let return_to = self.state.return_to();

        // Ctrl-O temporary normal mode: normal mode with a return-to target
        if mode.is_normal() && return_to.is_some() {
            return match return_to {
                ReturnTo::Insert => "niI",
                ReturnTo::Replace => "niR",
                ReturnTo::VirtualReplace => "niV",
                ReturnTo::Select(_) => "niS",
                ReturnTo::None => unreachable!(),
            };
        }

        // Insert mode with Ctrl-X completion sub-mode
        if mode.is_insert() {
            if matches!(parser_state, InputState::AwaitingInsertCtrlX) {
                return "ix";
            }
            // After Ctrl-X, the host may have entered a completion state.
            // The base "ic" case: if we had a completion-active flag we'd
            // return "ic" here, but for now "ix" covers the parser state.
        }

        // Operator-pending with forced motion type
        // (Parser tracks operator-pending via InputState, not Mode enum)
        if let InputState::Operator {
            force_type: Some(ft),
            ..
        } = parser_state
        {
            return match ft {
                crate::primitives::MotionType::CharWise => "nov",
                crate::primitives::MotionType::LineWise => "noV",
                crate::primitives::MotionType::BlockWise => "no\x16",
            };
        }

        mode.vim_string()
    }

    /// Cheap snapshot of the engine's current semantic state.
    ///
    /// Hosts call this before key processing to make routing decisions
    /// (e.g., whether to pass a key to the engine or handle it natively)
    /// without needing mutable access to the engine.
    ///
    /// All fields are derived from existing engine state — construction is O(P)
    /// where P is the pending command display length (typically 0-5 chars).
    #[must_use]
    pub fn vim_context(&self) -> crate::primitives::VimContext {
        crate::primitives::VimContext {
            mode: self.state.mode(),
            pending_operator: self.parser.state().pending_operator(),
            is_recording: self.recording.buffer.is_some(),
            is_repeating: self.is_repeating,
            has_pending_keys: self.has_pending_keys(),
            pending_display: self.pending_command_display(),
            active_register: self.parser.state().register(),
        }
    }

    /// Process a host notification about external state changes.
    ///
    /// Returns a [`Response`] that may contain effects the host should apply.
    /// Notifications are lightweight and typically produce zero or few effects.
    ///
    /// # Semantics
    ///
    /// - `FocusLost` in insert mode: breaks the current undo group.
    /// - `ClipboardChanged`: updates the clipboard generation counter.
    /// - `FocusGained`, `SelectionChangedExternally`, `ConfigReloaded`: acknowledged,
    ///   no effects produced currently.
    pub fn process_notification(
        &mut self,
        notification: crate::execution::HostNotification,
    ) -> Response {
        use crate::execution::HostNotification;

        match notification {
            HostNotification::FocusGained => Response::consumed_empty(),
            HostNotification::FocusLost => {
                // In insert mode, break the undo group so focus loss creates
                // a natural undo boundary.
                if self.state.mode().is_insert() {
                    let cursor = self.state.undo_cursor_hint();
                    let timestamp = self.state.undo_timestamp_hint();
                    self.state
                        .undo_tree_mut()
                        .end_group(cursor, timestamp, None);
                }
                Response::consumed_empty()
            }
            HostNotification::SelectionChangedExternally { .. } => {
                // Currently acknowledged without internal state changes.
                // Future: may update internal selection tracking.
                Response::consumed_empty()
            }
            HostNotification::ClipboardChanged { generation } => {
                self.state
                    .registers_mut()
                    .set_clipboard_generation(generation);
                Response::consumed_empty()
            }
            HostNotification::ConfigReloaded => {
                // Currently acknowledged without effects.
                // Future: may invalidate caches.
                Response::consumed_empty()
            }
            HostNotification::TimerFired { id: _ } => {
                // Timer fired — emit CursorHold event.
                // Future: dispatch based on timer ID for different timer types.
                let effects =
                    crate::effects::Effects::new().event(crate::primitives::VimEvent::CursorHold);
                Response::with_effects(effects)
            }
            HostNotification::ViewportChanged { first_line, height } => {
                // Store viewport info in engine state for H/M/L motions.
                self.viewport_first_line = first_line;
                self.viewport_height = height;
                Response::consumed_empty()
            }
            HostNotification::FileTypeChanged { filetype } => {
                // Update filetype for filetype-specific mappings.
                self.set_filetype(Some(&filetype));
                Response::consumed_empty()
            }
            HostNotification::DiagnosticsUpdated { count } => {
                // Store diagnostic count for variable/statusline access.
                self.diagnostics_count = count;
                Response::consumed_empty()
            }
            HostNotification::CompletionDone { item: _ } => {
                // Acknowledged. Future: emit VimEvent::CompletionDone.
                Response::consumed_empty()
            }
            HostNotification::WindowResized { cols, rows } => {
                // Store terminal dimensions.
                self.terminal_cols = cols;
                self.terminal_rows = rows;
                Response::consumed_empty()
            }
            HostNotification::AsyncEvent { .. } => {
                // Async events are queued for draining at the start of
                // the next process_key() cycle. Currently a no-op placeholder.
                Response::consumed_empty()
            }
        }
    }

    /// Whether the engine is currently executing a dot-repeat (`.`) replay.
    ///
    /// Used by the shell to gate insert-mode features (e.g. auto-brace
    /// completion) that should only fire for live user input, not replays.
    #[inline]
    #[must_use]
    pub const fn is_repeating(&self) -> bool {
        self.is_repeating
    }

    /// Whether the engine is processing live user input in insert mode.
    ///
    /// Returns `false` during dot-repeat, macro replay, mapping expansion,
    /// or when the typeahead buffer has entries (from mapping RHS or feedkeys).
    /// Used by the shell to gate host-specific insert features (auto-brace
    /// completion, etc.) that should only fire for interactive user keystrokes.
    #[inline]
    #[must_use]
    pub fn is_live_insert_input(&self) -> bool {
        self.state.mode().is_insert()
            && !self.is_repeating
            && self.typeahead.macro_stack.is_empty()
            && self.typeahead.buffer.is_empty()
            && !self.typeahead.buffer.has_pending()
    }

    /// Set current mode.
    ///
    /// Used by shell when it processes effects from delegated operations
    /// (e.g., OperatorToMark) that need to update the engine's mode.
    #[inline]
    pub const fn set_mode(&mut self, mode: Mode) {
        self.state.set_mode(mode);
    }

    /// Set the sticky column for vertical motions (curswant).
    ///
    /// Called by the shell for external cursor changes (e.g., mouse clicks)
    /// that should update the desired column for subsequent j/k motions.
    #[inline]
    pub const fn set_sticky_column(&mut self, col: usize) {
        self.state
            .set_sticky_column(Some(crate::primitives::VirtualColumn::new(col)));
    }

    /// Set the search pattern and direction.
    ///
    /// Called by shell after `/pattern<CR>` or `?pattern<CR>` is entered.
    /// This allows `n`/`N` motions and `/{pattern}` operators to work.
    #[inline]
    pub fn set_search_pattern(
        &mut self,
        pattern: impl Into<compact_str::CompactString>,
        direction: crate::primitives::SearchDirection,
    ) {
        self.state.search_mut().set_pattern(pattern, direction);
    }

    /// Apply an effect to the engine's internal state.
    ///
    /// Used by shell when it processes effects from delegated operations
    /// (e.g., OperatorToMark) that need to sync state like registers.
    /// Delegates to the shared `sync_effect` in `effect_processor`.
    pub fn apply_effect(&mut self, effect: &Effect) {
        super::super::effect_processor::sync_effect(&mut self.state, &mut self.parser, effect);
    }

    /// Record text typed during insert mode for dot-repeat and macro recording.
    ///
    /// Called by an integration layer when the host's insert-mode fast path
    /// bypasses the engine for character-by-character typing. Appends to the
    /// active `InsertState.accumulated_text` so that `handle_insert_exit` picks up
    /// the full text on Escape. Does NOT touch `last_inserted_text` — that is
    /// authoritatively set from `accumulated_text` during `handle_insert_exit`.
    pub fn record_insert_text(&mut self, text: &str) {
        if let Some(insert_state) = self.state.insert_state_mut() {
            insert_state.push_str(text);
            insert_state.set_had_text_mutation();
        }
        self.append_to_recording(text);
    }

    /// Get current state (read-only).
    #[inline]
    #[must_use]
    pub const fn state(&self) -> &VimState {
        &self.state
    }

    /// Capture a lightweight snapshot of the current state for diffing.
    ///
    /// Call this before processing a keystroke, then call
    /// [`StateSnapshot::diff()`](crate::state::StateSnapshot::diff) after
    /// processing to discover which state domains changed.
    ///
    /// # Complexity
    ///
    /// Time: O(1) — captures mode enum, version counters, and a few scalar
    /// fields. No deep copies of registers, marks, or undo tree.
    ///
    /// Space: O(1) — the snapshot is a small fixed-size struct.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let snapshot = engine.snapshot_state();
    /// let response = engine.process(key, ctx);
    /// let diff = snapshot.diff(engine.state());
    /// if diff.mode {
    ///     ui.update_mode_indicator(engine.state().mode());
    /// }
    /// ```
    #[must_use]
    pub fn snapshot_state(&self) -> crate::state::StateSnapshot {
        crate::state::StateSnapshot::capture(&self.state)
    }

    /// Mutable access to the jump list (for shell-level navigation).
    ///
    /// Shells call this to push positions before jumps (e.g., mouse clicks,
    /// buffer switches) so that `Ctrl-O` / `Ctrl-I` work correctly.
    #[inline]
    pub const fn jump_list_mut(&mut self) -> &mut crate::state::JumpList {
        self.state.jump_list_mut()
    }

    /// Mutable access to registers (for shell-level clipboard sync).
    ///
    /// Shells call this to write clipboard contents into the unnamed register
    /// after an async clipboard read completes.
    #[inline]
    pub const fn registers_mut(&mut self) -> &mut crate::state::Registers {
        self.state.registers_mut()
    }

    /// Mutable access to marks (for shell-level mark management).
    ///
    /// Shells call this to set buffer-local marks when switching editors,
    /// or to update automatic marks (e.g., last-change position).
    #[inline]
    pub const fn marks_mut(&mut self) -> &mut crate::state::Marks {
        self.state.marks_mut()
    }

    /// Export all buffer-local marks (a-z and special marks) as an opaque blob.
    ///
    /// Hosts call this when saving buffer state (e.g., on buffer close or
    /// session save). The returned [`SerializedMarks`](crate::state::SerializedMarks)
    /// can be persisted to
    /// disk and later restored via [`Self::import_marks`].
    #[cfg(feature = "serde")]
    pub fn export_marks(&self) -> crate::state::SerializedMarks {
        let marks = self.state.marks();
        let bytes = serde_json::to_vec(marks).unwrap_or_default();
        crate::state::SerializedMarks(bytes)
    }

    /// Import buffer-local marks from a previously exported blob.
    ///
    /// Replaces the current marks state with the deserialized data.
    /// If deserialization fails (e.g., corrupted data), the marks state
    /// is left unchanged.
    #[cfg(feature = "serde")]
    pub fn import_marks(&mut self, data: crate::state::SerializedMarks) {
        if let Ok(marks) = serde_json::from_slice::<crate::state::Marks>(&data.0) {
            *self.state.marks_mut() = marks;
        }
    }

    /// Export only global marks (A-Z) as an opaque blob.
    ///
    /// Global marks are cross-buffer and should be persisted at the session
    /// level (not per-buffer). Hosts call this on session save.
    #[cfg(feature = "serde")]
    pub fn export_global_marks(&self) -> crate::state::SerializedMarks {
        use crate::primitives::MarkName;

        // Collect global mark entries into a serializable structure.
        let marks = self.state.marks();
        let globals: ahash::AHashMap<MarkName, &crate::state::GlobalMarkEntry> =
            marks.globals().collect();
        let bytes = serde_json::to_vec(&globals).unwrap_or_default();
        crate::state::SerializedMarks(bytes)
    }

    /// Import global marks (A-Z) from a previously exported blob.
    ///
    /// Merges the deserialized global marks into the current state.
    /// Existing global marks with the same name are overwritten.
    /// If deserialization fails, the state is left unchanged.
    #[cfg(feature = "serde")]
    pub fn import_global_marks(&mut self, data: crate::state::SerializedMarks) {
        use crate::primitives::MarkName;

        if let Ok(globals) = serde_json::from_slice::<
            ahash::AHashMap<MarkName, crate::state::GlobalMarkEntry>,
        >(&data.0)
        {
            let marks = self.state.marks_mut();
            for (name, entry) in globals {
                marks.set_with_buffer_id(name, entry.mark, Some(entry.buffer_id));
            }
        }
    }

    /// Mutable access to the changelist (for shell-level change tracking).
    #[inline]
    pub const fn changelist_mut(&mut self) -> &mut crate::state::ChangeList {
        self.state.changelist_mut()
    }

    /// Get undo tree (read-only).
    ///
    /// The undo tree tracks branch-aware undo history for navigation
    /// (`:earlier`, `:later`, `:undolist`) and state queries
    /// (`can_undo`, `can_redo`, `depth`, `branch_count`).
    #[inline]
    #[must_use]
    pub const fn undo_tree(&self) -> &crate::state::UndoTree {
        self.state.undo_tree()
    }

    /// Mutable access to the undo tree.
    #[inline]
    pub const fn undo_tree_mut(&mut self) -> &mut crate::state::UndoTree {
        self.state.undo_tree_mut()
    }

    /// Set the timestamp hint for undo tree group commits.
    ///
    /// Hosts should call this with a monotonic seconds value before
    /// each `process()` call to enable time-based undo navigation
    /// (`:earlier Ns` / `:later Ns`).
    #[inline]
    pub const fn set_undo_timestamp(&mut self, secs: u64) {
        self.state.set_undo_timestamp_hint(secs);
    }

    /// Close a stale auto-opened undo group if the time window has expired.
    ///
    /// Returns `true` if an auto-group was closed.
    pub fn tick_undo_auto_group(&mut self, now_ms: u64) -> bool {
        let window = match self.options.undo_auto_group_ms() {
            Some(ms) => u64::from(ms),
            None => return false,
        };
        if !self.state.undo_auto_group_active() {
            return false;
        }
        let last = self.state.undo_auto_group_last_edit_ms();
        if now_ms.saturating_sub(last) <= window {
            return false;
        }
        let cursor = self.state.undo_cursor_hint();
        let timestamp = self.state.undo_timestamp_hint();
        self.state
            .undo_tree_mut()
            .end_group(cursor, timestamp, None);
        self.state.set_undo_auto_group_active(false);
        self.state.set_undo_auto_group_last_edit_ms(0);
        true
    }

    /// Set the expression register (`=`) result after host evaluation.
    ///
    /// When the engine emits `HostRequest::EvaluateExpression`, the host
    /// evaluates the expression and calls this method with the result text.
    /// The text is then available via `"=p` and other register-reading
    /// commands.
    #[inline]
    pub fn set_expression_result(&mut self, text: impl Into<compact_str::CompactString>) {
        self.state
            .registers_mut()
            .set_expression_result(crate::primitives::RegisterContent::char_wise(text));
    }

    /// Mutable access to search state (for shell-level search integration).
    ///
    /// Prefer [`Self::set_search_pattern`] for simple pattern updates; use this
    /// only when you need full control over [`SearchState`](crate::state::SearchState).
    #[inline]
    pub const fn search_mut(&mut self) -> &mut crate::state::SearchState {
        self.state.search_mut()
    }

    /// Mutable access to repeat state (for dot-repeat and intent tracking).
    ///
    /// Allows direct manipulation of the last command, intent, and inserted text
    /// for dot-repeat (`.`) and intent-aware repeat (`g.`).
    #[inline]
    pub const fn repeat_state_mut(&mut self) -> &mut crate::state::RepeatState {
        self.state.repeat_state_mut()
    }

    /// Get the keymap (read-only).
    #[inline]
    #[must_use]
    pub const fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Mutable access to the keymap for registering mappings, `<Plug>`, `<Action>`, etc.
    #[inline]
    pub const fn keymap_mut(&mut self) -> &mut Keymap {
        self.key_interest_dirty = true;
        &mut self.keymap
    }

    /// Set the current buffer's file type for filetype-specific mappings.
    ///
    /// Pass `None` to clear the filetype.
    pub fn set_filetype(&mut self, filetype: Option<&str>) {
        self.keymap
            .set_filetype(filetype.map(compact_str::CompactString::from));
        self.key_interest_dirty = true;
    }

    /// Get the current file type.
    #[must_use]
    pub fn filetype(&self) -> Option<&str> {
        self.keymap.active_filetype()
    }

    /// First visible line as reported by the host via `ViewportChanged`.
    #[inline]
    #[must_use]
    pub const fn viewport_first_line(&self) -> usize {
        self.viewport_first_line
    }

    /// Visible line count as reported by the host via `ViewportChanged`.
    #[inline]
    #[must_use]
    pub const fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    /// Terminal width in columns as reported by `WindowResized`.
    #[inline]
    #[must_use]
    pub const fn terminal_cols(&self) -> usize {
        self.terminal_cols
    }

    /// Terminal height in rows as reported by `WindowResized`.
    #[inline]
    #[must_use]
    pub const fn terminal_rows(&self) -> usize {
        self.terminal_rows
    }

    /// Active diagnostic count as reported by `DiagnosticsUpdated`.
    #[inline]
    #[must_use]
    pub const fn diagnostics_count(&self) -> usize {
        self.diagnostics_count
    }

    /// Get the handler map (read-only).
    ///
    /// The handler map controls per-key, per-mode delegation between
    /// the Vim engine and the host editor (`:sethandler`).
    #[inline]
    #[must_use]
    pub const fn handler_map(&self) -> &crate::keymap::HandlerMap {
        &self.handler_map
    }

    /// Mutable access to the handler map for configuring key delegation.
    ///
    /// # Example
    /// ```ignore
    /// // Let the host handle Ctrl-A in insert mode
    /// engine.handler_map_mut().set(
    ///     KeyEvent::ctrl('a'),
    ///     MappingMode::Insert,
    ///     Handler::Host,
    /// );
    /// ```
    #[inline]
    pub const fn handler_map_mut(&mut self) -> &mut crate::keymap::HandlerMap {
        self.key_interest_dirty = true;
        &mut self.handler_map
    }

    /// Get the current Vim options (read-only).
    #[inline]
    #[must_use]
    pub const fn options(&self) -> &VimOptions {
        &self.options
    }

    /// Replace all Vim options at once.
    ///
    /// This writes the global layer only, like `:setglobal`: a local value
    /// that `:set` or `:setlocal` gave the current buffer or window keeps
    /// winning there. To change an option the way `:set` does, use
    /// [`set_option`](Self::set_option).
    ///
    /// Rebuilds the resolved-options cache immediately.
    #[inline]
    pub fn set_options(&mut self, options: VimOptions) {
        self.options = options;
        self.rebuild_resolved_cache();
        self.rebuild_langmap_if_needed();
    }

    /// Mutable reference to the engine's Vim options (global layer only).
    ///
    /// Allows updating individual options (e.g., `commentstring`) without
    /// replacing the entire options struct. Like `:setglobal`, a write here
    /// does not change a local value that `:set` or `:setlocal` gave the
    /// current buffer or window; see [`set_option`](Self::set_option).
    ///
    /// # Cache invalidation
    ///
    /// Because this returns a `&mut VimOptions`, the engine cannot detect when
    /// the caller stops mutating. The resolved-options cache is marked dirty and
    /// rebuilt lazily at the start of the next `process()` call.
    ///
    /// If you need the cache to be current immediately (e.g., before calling
    /// `effective_option()`), call `invalidate_option_cache()` afterwards.
    #[inline]
    pub const fn options_mut(&mut self) -> &mut VimOptions {
        self.options_dirty = true;
        &mut self.options
    }

    /// Set one option the way `:set` does: the global value, and the value
    /// the current buffer or window sees. Any local value the option had
    /// there, from an earlier `:set` or `:setlocal`, is dropped, so this
    /// value takes effect at once and the last change wins.
    ///
    /// Use this for a value the host means to apply now, such as an editor
    /// setting the user just changed. Local values saved for other buffers
    /// (see [`on_buffer_leave`](Self::on_buffer_leave)) are not touched, as
    /// with `:set`. For the setting to win in those buffers too, also call
    /// [`BufferLocalState::clear_local_option`](crate::execution::BufferLocalState::clear_local_option)
    /// on every saved state.
    ///
    /// Rebuilds the resolved-options cache immediately.
    pub fn set_option(&mut self, id: OptionId, value: &OptionValue) {
        self.options.set_option(id, value);
        self.buffer_overrides.remove(id);
        self.window_overrides.remove(id);
        self.rebuild_resolved_cache();
        self.rebuild_langmap_if_needed();
    }

    /// Force an immediate rebuild of the resolved-options cache.
    ///
    /// Required when you modify options via `options_mut()` and then read
    /// `effective_option()` before the next `process()` call.
    #[inline]
    pub fn invalidate_option_cache(&mut self) {
        self.rebuild_resolved_cache();
        self.rebuild_langmap_if_needed();
    }

    // ─── Digraph Registry API ─────────────────────────────────────────

    /// Get the runtime digraph registry (read-only).
    #[inline]
    #[must_use]
    pub const fn digraph_registry(&self) -> &crate::primitives::DigraphRegistry {
        &self.digraph_registry
    }

    /// Get the runtime digraph registry (mutable).
    ///
    /// Use this to populate user-defined digraphs from host configuration.
    #[inline]
    pub const fn digraph_registry_mut(&mut self) -> &mut crate::primitives::DigraphRegistry {
        &mut self.digraph_registry
    }

    // ─── Multi-Cursor Command API ───────────────────────────────────────

    /// Execute a multi-cursor command.
    ///
    /// Dispatches the command to the multi-cursor executor, which modifies
    /// the engine's `MultiCursorState` stored on `VimState`.
    ///
    /// Pure selection commands (`AddCursor`, `RemoveCursor`, `ClearSecondary`,
    /// `RotatePrimary`) are handled inline and ignore the context.
    /// Context-dependent commands (`AddCursorVertical`, `AddCursorsAtMatches`,
    /// `SelectAllOccurrences`, `AddNextMatch`) use `ctx` to resolve
    /// positions within the document.
    ///
    /// # Errors
    ///
    /// Returns `VimError` for invalid operations (e.g., removing the last cursor),
    /// missing search patterns, or pattern-not-found conditions.
    pub fn execute_multi_cursor(
        &mut self,
        cmd: &crate::state::MultiCursorCommand,
        ctx: &super::super::multi_cursor_executor::MultiCursorContext<'_>,
    ) -> Result<Vec<crate::effects::Effect>, crate::errors::VimError> {
        super::super::multi_cursor_executor::execute_multi_cursor_command(&mut self.state, cmd, ctx)
    }

    /// Update the primary cursor position in the multi-cursor state to match
    /// the host's actual cursor. Called by hosts that move their own carets
    /// directly (e.g. a host-level Ctrl+D keybinding) where the engine's
    /// primary cursor is stale because that path bypasses `process_key`.
    pub fn sync_primary_cursor(&mut self, offset: usize) {
        let primary = self.state.multi_cursor_mut().selections_mut().primary_mut();
        *primary = crate::primitives::SelectionRange::insert_cursor(
            crate::primitives::Offset::new(offset),
        );
    }

    // ─── Option Override API ──────────────────────────────────────────

    /// Replace the current buffer's local option overrides.
    ///
    /// Rebuilds the resolved-options cache immediately.
    #[inline]
    pub fn set_buffer_overrides(&mut self, overrides: OptionOverrides) {
        self.buffer_overrides = overrides;
        self.rebuild_resolved_cache();
    }

    /// Take the current buffer's local option overrides, replacing with an empty set.
    ///
    /// Rebuilds the resolved-options cache immediately.
    /// Returns the previous overrides so the caller can persist them.
    #[inline]
    pub fn take_buffer_overrides(&mut self) -> OptionOverrides {
        let old = std::mem::take(&mut self.buffer_overrides);
        self.rebuild_resolved_cache();
        old
    }

    /// Save all per-buffer state and return it for the caller to persist.
    ///
    /// Sets the `'"` (last-position) mark at `cursor_offset`, then extracts
    /// all per-buffer state from the engine. After this call, the engine
    /// holds only session-global state (registers, global marks A-Z, search,
    /// jumplist, mode, macros, etc.).
    ///
    /// The caller should store the returned
    /// [`BufferLocalState`](crate::execution::BufferLocalState) keyed by
    /// buffer identity (e.g., Godot `InstanceId`) and pass it back to
    /// [`on_buffer_enter`](Self::on_buffer_enter) when returning to this buffer.
    ///
    /// # Call order
    ///
    /// In the host's buffer-detach path: resolve pending mapping keys, send a
    /// synthetic Escape through the engine pipeline if not in Normal mode,
    /// THEN call this method. The pipeline exit handles marks, changelist, macro
    /// recording, and undo groups correctly — identical to the user pressing
    /// Esc.
    pub fn on_buffer_leave(
        &mut self,
        cursor_offset: usize,
    ) -> super::buffer_state::BufferLocalState {
        use crate::primitives::{Mark, MarkName, Offset};

        // Set the last-position mark (existing behavior, preserved).
        self.state.marks_mut().set(
            MarkName::LAST_POSITION,
            Mark::new(Offset::new(cursor_offset)),
        );

        let sticky_column = self.state.sticky_column();
        self.state.set_sticky_column(None);

        let scroll_half_count = self.state.scroll_half_count();
        self.state.set_scroll_half_count_opt(None);

        // Exhaustive construction — adding a field to BufferLocalState
        // causes a compile error here until handled.
        let result = super::buffer_state::BufferLocalState {
            marks: self.state.marks_mut().take_buffer_marks(),
            changelist: std::mem::take(self.state.changelist_mut()),
            last_visual: self.state.take_last_visual(),
            sticky_column,
            buffer_overrides: self.take_buffer_overrides(),
            buffer_mappings: self.take_buffer_mappings(),
            scroll_half_count,
            undo_tree: std::mem::take(self.state.undo_tree_mut()),
            buffer_variables: self.state.variable_store_mut().take_buffer_vars(),
        };

        // Clear syntax selection history — byte offsets are buffer-specific
        // and meaningless for the next buffer.
        self.state.syntax_selection_mut().clear();

        result
    }

    /// Restore per-buffer state from a previous [`on_buffer_leave`](Self::on_buffer_leave) call.
    ///
    /// For first-visit buffers, pass
    /// [`BufferLocalState::default()`](crate::execution::BufferLocalState) to
    /// initialize with empty per-buffer state.
    pub fn on_buffer_enter(&mut self, state: super::buffer_state::BufferLocalState) {
        // Exhaustive destructure — adding a field to BufferLocalState
        // causes a compile error here until handled.
        let super::buffer_state::BufferLocalState {
            marks,
            changelist,
            last_visual,
            sticky_column,
            buffer_overrides,
            buffer_mappings,
            scroll_half_count,
            undo_tree,
            buffer_variables,
        } = state;

        self.state.marks_mut().set_buffer_marks(marks);
        *self.state.changelist_mut() = changelist;
        self.state.set_last_visual_opt(last_visual);
        self.state.set_sticky_column(sticky_column);
        self.set_buffer_overrides(buffer_overrides);
        self.set_buffer_mappings(buffer_mappings);
        self.state.set_scroll_half_count_opt(scroll_half_count);
        *self.state.undo_tree_mut() = undo_tree;
        self.state
            .variable_store_mut()
            .restore_buffer_vars(buffer_variables);
    }

    /// Replace the current window's local option overrides.
    ///
    /// Rebuilds the resolved-options cache immediately.
    #[inline]
    pub fn set_window_overrides(&mut self, overrides: OptionOverrides) {
        self.window_overrides = overrides;
        self.rebuild_resolved_cache();
    }

    /// Take the current window's local option overrides, replacing with an empty set.
    ///
    /// Rebuilds the resolved-options cache immediately.
    /// Returns the previous overrides so the caller can persist them.
    #[inline]
    pub fn take_window_overrides(&mut self) -> OptionOverrides {
        let old = std::mem::take(&mut self.window_overrides);
        self.rebuild_resolved_cache();
        old
    }

    /// Return the effective value of an option (global + local overrides resolved).
    ///
    /// Uses the pre-resolved cache; always reflects the current buffer and window
    /// overrides. For the raw global value, use `options().get_option(id)`.
    #[inline]
    #[must_use]
    pub fn effective_option(&self, id: OptionId) -> OptionValue {
        self.resolved_options.get_option(id)
    }

    // ─── Persistent Provider API ──────────────────────────────────────

    /// Register a persistent custom motion provider on the engine.
    ///
    /// This provider is automatically merged with per-call providers
    /// from `InputContext`. Per-call providers take precedence.
    ///
    /// Replaces any previously registered custom motion provider.
    #[inline]
    pub fn register_motion_provider(
        &mut self,
        provider: impl crate::document::CustomMotionProvider + 'static,
    ) {
        self.engine_providers.custom_motions = Some(Box::new(provider));
    }

    /// Register a persistent custom text object provider on the engine.
    ///
    /// This provider is automatically merged with per-call providers
    /// from `InputContext`. Per-call providers take precedence.
    ///
    /// Replaces any previously registered custom text object provider.
    #[inline]
    pub fn register_textobject_provider(
        &mut self,
        provider: impl crate::document::CustomTextObjectProvider + 'static,
    ) {
        self.engine_providers.custom_textobjects = Some(Box::new(provider));
    }

    /// Register a persistent custom operator provider on the engine.
    ///
    /// This provider is automatically merged with per-call providers
    /// from `InputContext`. Per-call providers take precedence.
    ///
    /// Replaces any previously registered custom operator provider.
    #[inline]
    pub fn register_operator_provider(
        &mut self,
        provider: impl crate::document::CustomOperatorProvider + 'static,
    ) {
        self.engine_providers.custom_operators = Some(Box::new(provider));
    }

    /// Register a persistent syntax provider (tree-sitter gateway) on the engine.
    ///
    /// This provider is automatically merged with per-call providers
    /// from `InputContext`. Per-call providers take precedence.
    ///
    /// Replaces any previously registered syntax provider.
    #[inline]
    pub fn register_syntax_provider(
        &mut self,
        provider: impl crate::document::SyntaxProvider + 'static,
    ) {
        self.engine_providers.syntax = Some(Box::new(provider));
    }

    /// Register a persistent semantic text object provider on the engine.
    ///
    /// This provider is automatically merged with per-call providers
    /// from `InputContext`. Per-call providers take precedence.
    ///
    /// Replaces any previously registered semantic text object provider.
    #[inline]
    pub fn register_semantic_textobject_provider(
        &mut self,
        provider: Box<dyn crate::document::SemanticTextObjectProvider>,
    ) {
        self.engine_providers.semantic_textobjects = Some(provider);
    }

    /// Set a cached indent hint for the next `o`/`O`/Enter command.
    ///
    /// The host calls this before each `processKey` to push a pre-computed
    /// indent string from its language intelligence (e.g., on-enter indent
    /// rules, tree-sitter). When the engine processes `o`/`O` or
    /// Enter in insert mode, it retrieves this cached hint instead of
    /// falling back to basic autoindent.
    ///
    /// The hint is a [`CachedIndentHint`](crate::document::CachedIndentHint)
    /// which implements `IndentProvider`.
    #[inline]
    pub fn set_indent_hint(&mut self, indent: &str) {
        self.engine_providers.indent =
            Some(Box::new(crate::document::CachedIndentHint::new(indent)));
    }

    /// Clear the cached indent hint.
    ///
    /// After clearing, the engine falls back to basic autoindent (copying
    /// the previous line's leading whitespace).
    #[inline]
    pub fn clear_indent_hint(&mut self) {
        self.engine_providers.indent = None;
    }

    /// Push fold state for hosts that cannot answer synchronous callbacks.
    ///
    /// A host that reaches the engine across a foreign-function boundary
    /// generally cannot be called back into synchronously. Instead it calls
    /// this before each key is processed, pushing the current fold state as a
    /// list of hidden-line ranges.
    ///
    /// Each `(start, end)` entry is an inclusive range of hidden lines.
    /// Ranges must be non-overlapping and sorted in ascending order.
    ///
    /// When the engine processes `j`/`k` motions it will skip folded lines
    /// using this data.
    #[inline]
    pub fn set_fold_state(&mut self, hidden: Vec<(LineNumber, LineNumber)>) {
        self.engine_providers.fold = Some(Box::new(crate::document::HostFoldProvider::new(hidden)));
    }

    /// Clear the pushed fold state.
    ///
    /// After clearing, the engine treats all lines as visible (no fold
    /// provider). Call this when the document has no folds.
    #[inline]
    pub fn clear_fold_state(&mut self) {
        self.engine_providers.fold = None;
    }

    /// Push display line state for hosts that cannot answer synchronous
    /// callbacks.
    ///
    /// The host calls this before each key is processed to push the wrap
    /// column and tab size. The engine creates a `HostDisplayLineProvider`
    /// that computes break positions from these parameters + line text.
    ///
    /// When `gj`/`gk`/`g0`/`g$`/`g^` motions execute, they pass the
    /// current line text to the provider which computes sub-line breaks.
    #[inline]
    pub fn set_display_line_state(&mut self, wrap_column: usize, tab_size: usize) {
        self.engine_providers.display_lines = Some(Box::new(
            crate::document::HostDisplayLineProvider::new(wrap_column, tab_size),
        ));
    }

    /// Clear the display line state.
    ///
    /// After clearing, display-line motions (gj/gk/g0/g$/g^) fall back
    /// to their physical-line equivalents.
    #[inline]
    pub fn clear_display_line_state(&mut self) {
        self.engine_providers.display_lines = None;
    }

    /// Set a simple indent action (indent + optional append text).
    ///
    /// More expressive than [`set_indent_hint`](Self::set_indent_hint) --
    /// supports an optional `append` string (e.g. `* ` for block-comment
    /// continuations).
    #[inline]
    pub fn set_indent_action_simple(&mut self, indent: &str, append: Option<&str>) {
        self.engine_providers.indent = Some(Box::new(crate::document::CachedIndentAction::simple(
            indent, append,
        )));
    }

    /// Set an indent-outdent action (two newlines for bracket pairs).
    ///
    /// Used when the host detects a bracket pair (e.g. Enter between `{}`).
    /// The engine will insert two newlines: one for the cursor line (with
    /// `indent` + optional `append`) and one for the closing bracket line
    /// (with `closing_indent`).
    #[inline]
    pub fn set_indent_action_outdent(
        &mut self,
        indent: &str,
        append: Option<&str>,
        closing_indent: &str,
    ) {
        self.engine_providers.indent = Some(Box::new(
            crate::document::CachedIndentAction::indent_outdent(indent, append, closing_indent),
        ));
    }

    /// Check whether any engine-level providers are registered.
    #[inline]
    #[must_use]
    pub fn has_engine_providers(&self) -> bool {
        self.engine_providers.has_any()
    }

    // ─── Effect Pipeline API ─────────────────────────────────────────

    /// Set the effect middleware pipeline.
    ///
    /// All effects produced by [`process()`](Self::process) and
    /// [`process_click()`](Self::process_click) will pass through
    /// this pipeline before being returned to the caller.
    ///
    /// Pass `None` to remove the pipeline.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use vim_core::effects::{EffectPipeline, DeduplicateMiddleware, LoggingMiddleware};
    ///
    /// let mut pipeline = EffectPipeline::new();
    /// pipeline.push(DeduplicateMiddleware);
    /// pipeline.push(LoggingMiddleware::new());
    ///
    /// engine.set_pipeline(Some(pipeline));
    /// ```
    #[inline]
    pub fn set_pipeline(&mut self, pipeline: Option<EffectPipeline>) {
        self.cold.pipeline = pipeline;
    }

    /// Read-only access to the effect pipeline.
    #[inline]
    #[must_use]
    pub const fn pipeline(&self) -> Option<&EffectPipeline> {
        self.cold.pipeline.as_ref()
    }

    /// Mutable access to the effect pipeline.
    ///
    /// Returns `None` if no pipeline is set. Use [`set_pipeline()`](Self::set_pipeline)
    /// to install one first.
    #[inline]
    pub const fn pipeline_mut(&mut self) -> Option<&mut EffectPipeline> {
        self.cold.pipeline.as_mut()
    }

    // ─── Hook API ──────────────────────────────────────────────────────

    /// Register a hook handler for the given lifecycle point.
    ///
    /// The handler will be invoked whenever the engine fires the specified
    /// [`HookPoint`](crate::execution::HookPoint). Handlers fire in registration order (FIFO). Hook
    /// firing is suppressed during speculative execution (`fork_active`).
    ///
    /// Returns a [`HookId`](crate::execution::HookId) that can be passed to [`remove_hook()`](Self::remove_hook)
    /// to unregister the handler.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use vim_core::execution::{HookAction, HookContext, HookPoint, VimEngine};
    ///
    /// let mut engine = VimEngine::new();
    /// let id = engine.add_hook(HookPoint::PostCommand, Box::new(|ctx: &HookContext<'_>| {
    ///     // Inspect ctx.effects, ctx.mode, ctx.state...
    ///     HookAction::Continue
    /// }));
    /// ```
    pub fn add_hook(
        &mut self,
        point: super::hooks::HookPoint,
        handler: Box<dyn super::hooks::HookHandler>,
    ) -> super::hooks::HookId {
        self.cold.hooks.add(point, handler)
    }

    /// Remove a previously registered hook handler.
    ///
    /// Returns `true` if the handler was found and removed, `false` if the
    /// [`HookId`](crate::execution::HookId) was not present (already removed or never registered).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let id = engine.add_hook(HookPoint::PostCommand, Box::new(my_handler));
    /// assert!(engine.remove_hook(id));
    /// assert!(!engine.remove_hook(id)); // already removed
    /// ```
    pub fn remove_hook(&mut self, id: super::hooks::HookId) -> bool {
        self.cold.hooks.remove(id)
    }

    /// Notify the engine that a buffer write (`:w`) is about to happen.
    ///
    /// This is a **host-integration hook**: the host calls this immediately before
    /// it performs the actual file write so that registered handlers can inspect
    /// state and, if necessary, veto the write.
    ///
    /// - Fires [`HookPoint::BufWritePre`](crate::execution::HookPoint::BufWritePre) handlers in registration order (FIFO).
    /// - Returns [`HookAction::Cancel`](crate::execution::HookAction::Cancel) if any handler returns `Cancel`; the host
    ///   **must** abort the write when it receives `Cancel`.
    /// - Returns [`HookAction::Continue`](crate::execution::HookAction::Continue) when no handlers are registered or all
    ///   handlers allow the write to proceed.
    ///
    /// This method is **not** gated on `fork_active` because it is called
    /// directly by the host, never from within [`process()`](Self::process).
    /// The host would not call buffer lifecycle methods during speculative
    /// execution.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use vim_core::execution::engine::hooks::HookAction;
    ///
    /// if engine.notify_buf_write_pre() == HookAction::Cancel {
    ///     // A hook handler vetoed the write — do not save the file.
    ///     return;
    /// }
    /// // Proceed with the actual file write.
    /// ```
    pub fn notify_buf_write_pre(&mut self) -> super::hooks::HookAction {
        let hook_ctx = super::hooks::HookContext {
            point: super::hooks::HookPoint::BufWritePre,
            mode: self.state.mode(),
            effects: &[],
            state: &self.state,
        };
        self.cold
            .hooks
            .fire(super::hooks::HookPoint::BufWritePre, &hook_ctx)
    }

    /// Notify the engine that the host is entering a new buffer.
    ///
    /// This is a **host-integration hook**: the host calls this after it has
    /// switched the active buffer (and after calling
    /// [`on_buffer_enter`](Self::on_buffer_enter) to restore per-buffer state)
    /// so that registered handlers can react to the buffer switch.
    ///
    /// - Fires [`HookPoint::BufEnter`](crate::execution::HookPoint::BufEnter) handlers in registration order (FIFO).
    /// - Returns [`HookAction::Cancel`](crate::execution::HookAction::Cancel) if any handler returns `Cancel`.
    ///   For informational lifecycle points like `BufEnter`, `Cancel` stops
    ///   handler iteration but does **not** undo the buffer switch — the host
    ///   has already completed the switch before calling this method.
    /// - Returns [`HookAction::Continue`](crate::execution::HookAction::Continue) when no handlers are registered or all
    ///   handlers return `Continue`.
    ///
    /// This method is **not** gated on `fork_active` because it is called
    /// directly by the host.
    ///
    /// # Example
    ///
    /// ```ignore
    /// engine.on_buffer_enter(saved_state);
    /// engine.notify_buf_enter(); // inform hooks about the switch
    /// ```
    pub fn notify_buf_enter(&mut self) -> super::hooks::HookAction {
        let hook_ctx = super::hooks::HookContext {
            point: super::hooks::HookPoint::BufEnter,
            mode: self.state.mode(),
            effects: &[],
            state: &self.state,
        };
        self.cold
            .hooks
            .fire(super::hooks::HookPoint::BufEnter, &hook_ctx)
    }

    /// Notify the engine that the host is leaving the current buffer.
    ///
    /// This is a **host-integration hook**: the host calls this before it
    /// switches away from the current buffer (and before calling
    /// [`on_buffer_leave`](Self::on_buffer_leave) to save per-buffer state)
    /// so that registered handlers can react or perform cleanup.
    ///
    /// - Fires [`HookPoint::BufLeave`](crate::execution::HookPoint::BufLeave) handlers in registration order (FIFO).
    /// - Returns [`HookAction::Cancel`](crate::execution::HookAction::Cancel) if any handler returns `Cancel`.
    ///   For informational lifecycle points like `BufLeave`, `Cancel` stops
    ///   handler iteration but does **not** prevent the buffer switch — the
    ///   host decides whether to honour the cancellation.
    /// - Returns [`HookAction::Continue`](crate::execution::HookAction::Continue) when no handlers are registered or all
    ///   handlers return `Continue`.
    ///
    /// This method is **not** gated on `fork_active` because it is called
    /// directly by the host.
    ///
    /// # Example
    ///
    /// ```ignore
    /// engine.notify_buf_leave(); // inform hooks before the switch
    /// let saved = engine.on_buffer_leave(cursor_offset);
    /// ```
    pub fn notify_buf_leave(&mut self) -> super::hooks::HookAction {
        let hook_ctx = super::hooks::HookContext {
            point: super::hooks::HookPoint::BufLeave,
            mode: self.state.mode(),
            effects: &[],
            state: &self.state,
        };
        self.cold
            .hooks
            .fire(super::hooks::HookPoint::BufLeave, &hook_ctx)
    }

    // ─── Autocmd Registry API ──────────────────────────────────────────

    /// Read-only access to the autocmd registry.
    ///
    /// Use this to inspect registered autocmds, list subscriptions for a
    /// given event, or check the registry size.
    #[inline]
    #[must_use]
    pub fn event_registry(&self) -> &crate::execution::event_registry::EventRegistry {
        &self.cold.event_registry
    }

    /// Mutable access to the event registry.
    ///
    /// Use this to subscribe, unsubscribe, or remove autocmds.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use vim_core::primitives::VimEvent;
    /// use vim_core::execution::{EventRegistry, StoredAutocmdHandler};
    ///
    /// let id = engine.event_registry_mut().subscribe(
    ///     &VimEvent::InsertEnter,
    ///     None,  // no filter
    ///     0,     // default priority
    ///     false, // not once
    ///     StoredAutocmdHandler::Rust { label: "my_hook".into() },
    /// );
    /// ```
    #[inline]
    pub fn event_registry_mut(&mut self) -> &mut crate::execution::event_registry::EventRegistry {
        &mut self.cold.event_registry
    }

    /// Returns a reference to the grammar parser.
    ///
    /// Exposes parser state for testing and binding table management.
    #[inline]
    #[must_use]
    pub const fn parser(&self) -> &crate::grammar::Parser {
        &self.parser
    }

    /// Returns a mutable reference to the grammar parser.
    ///
    /// Used to rebuild the binding table after host mapping changes.
    #[inline]
    pub const fn parser_mut(&mut self) -> &mut crate::grammar::Parser {
        &mut self.parser
    }

    // ─── External Selection Notification API ───────────────────────────

    /// Notify the engine that the host has created or cleared a selection
    /// via mouse drag, host action, or other non-keyboard means.
    ///
    /// When `has_selection` is `true` and the engine is in Normal mode, the
    /// engine transitions to Visual mode (character-wise or block-wise based
    /// on `is_block`). When `has_selection` is `false` and the engine is in
    /// Visual mode, it transitions back to Normal mode.
    ///
    /// For all other mode/selection combinations (e.g., Insert mode with a
    /// selection, or Normal mode without a selection), the method returns an
    /// empty consumed response — no mode change is needed.
    ///
    /// The host is responsible for managing the actual selection range; this
    /// method only synchronizes the engine's internal mode state so that
    /// subsequent keystrokes are dispatched correctly.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Host detected a mouse drag — tell the engine about the selection.
    /// let response = engine.notify_external_selection(true, false);
    /// shell.apply_effects(response.effects());
    ///
    /// // Host cleared the selection (e.g., user clicked without dragging).
    /// let response = engine.notify_external_selection(false, false);
    /// shell.apply_effects(response.effects());
    /// ```
    pub fn notify_external_selection(&mut self, has_selection: bool, is_block: bool) -> Response {
        let current_mode = self.state.mode();
        match (has_selection, current_mode) {
            (true, Mode::Normal) => {
                let visual_type = if is_block {
                    VisualType::Block
                } else {
                    VisualType::Char
                };
                let effects = Effects::new().set_mode(Mode::Visual(visual_type));
                let mut response = Response::with_effects(effects);
                super::super::effect_processor::process_effects(
                    &mut self.state,
                    &mut self.parser,
                    false,
                    &mut response,
                );
                response
            }
            (false, Mode::Visual(_)) => {
                let effects = Effects::new().set_mode(Mode::Normal);
                let mut response = Response::with_effects(effects);
                super::super::effect_processor::process_effects(
                    &mut self.state,
                    &mut self.parser,
                    false,
                    &mut response,
                );
                response
            }
            _ => Response::consumed_empty(),
        }
    }

    // ─── Unified Drain API ─────────────────────────────────────────────

    /// Drain the next key from the unified typeahead.
    ///
    /// Priority: typeahead buffer first (mapping RHS, feedkeys), then macro frame stack.
    /// Returns `None` when both are empty (shell should stop draining).
    ///
    /// Replaces the old two-drain pattern: `drain_mapping_key()` then `drain_macro_key()`.
    ///
    /// # Complexity
    ///
    /// Time: O(1) amortized — VecDeque pop_front is O(1) amortized, macro
    /// stack pop is O(1).
    ///
    /// Space: O(1)
    ///
    /// # Shell Usage
    ///
    /// ```ignore
    /// let response = engine.process(key, ctx);
    /// shell.apply_effects(&response.effects);
    /// while let Some(next) = engine.drain_next_key() {
    ///     let ctx = build_fresh_context();
    ///     let r = engine.process(next, ctx);
    ///     shell.apply_effects(&r.effects);
    /// }
    /// ```
    pub fn drain_next_key(&mut self) -> Option<super::macro_replay::MacroOutput> {
        use super::macro_replay::MacroOutput;

        // Priority 1: Typeahead buffer (mapping expansion RHS, feedkeys)
        if let Some(entry) = self.typeahead.buffer.pop_front() {
            self.typeahead.last_drained_flags = entry.flags;
            return Some(MacroOutput::Key(entry.key));
        }
        // Priority 2: Macro frame stack
        if let Some(output) = self.pump_macro_key() {
            self.typeahead.macro_effect_counter += 1;
            let limit = self.options.max_macro_effects();
            if self.typeahead.macro_effect_counter > limit {
                self.pending_macro_error =
                    Some(crate::errors::VimError::MacroEffectLimitExceeded { limit });
                self.abort_replay();
                return None;
            }
            // Macro entries are REMAPPABLE but not TYPED — never recorded.
            self.typeahead.last_drained_flags = super::typeahead::TypeaheadFlags::macro_key();
            return Some(output);
        }
        self.typeahead.macro_effect_counter = 0;
        // Macro replay finished — commit the merged undo group so the
        // entire N@a produces a single undo entry (Vim behavior).
        let cursor = self.state.undo_cursor_hint();
        let timestamp = self.state.undo_timestamp_hint();
        self.state
            .undo_tree_mut()
            .end_merge(cursor, timestamp, None);
        trace_event!(self, TraceEvent::UndoMergeEnd);
        None
    }

    /// Whether there are keys waiting to be processed.
    ///
    /// Checks both the typeahead buffer and the macro frame stack.
    /// When this returns `false`, the shell can stop draining and wait
    /// for the next user keystroke.
    ///
    /// Note: this does **not** consider the pending mapping prefix sub-buffer
    /// (multi-key LHS awaiting timeout). Use [`Self::has_pending_mapping()`] for that.
    #[inline]
    #[must_use]
    pub fn has_pending_keys(&self) -> bool {
        !self.typeahead.buffer.is_empty() || !self.typeahead.macro_stack.is_empty()
    }

    /// Abort all pending replay (macros + mapping expansion + feedkeys).
    ///
    /// Clears the typeahead buffer and macro stack. Resets macro replay
    /// depth in `MacroState` without affecting recording state or the
    /// last-played register.
    ///
    /// Used by shells when an error occurs during draining (e.g., motion
    /// past end-of-file) and all pending keystrokes should be discarded.
    ///
    /// # Complexity
    ///
    /// Time: O(T + F) where T = typeahead buffer size and F = macro frame
    /// stack depth. Both are cleared (VecDeque/Vec drop).
    ///
    /// Space: O(1)
    pub fn abort_replay(&mut self) {
        self.typeahead.buffer.clear();
        self.typeahead.macro_stack.clear();
        self.typeahead.macro_effect_counter = 0;
        self.state.macros_mut().reset_replay_depth();
        self.cmd_buffer = None;
        // Commit merged undo group if macro replay was active.
        let cursor = self.state.undo_cursor_hint();
        let timestamp = self.state.undo_timestamp_hint();
        self.state
            .undo_tree_mut()
            .end_merge(cursor, timestamp, None);
    }

    // ─── Host Request Query API ──────────────────────────────────────────

    /// Check whether a host request with the given ID is still pending.
    ///
    /// Returns `true` if the request has been issued but not yet completed.
    #[inline]
    #[must_use]
    pub fn has_pending_host_request(&self, id: crate::execution::host::HostRequestId) -> bool {
        self.host.pending.contains_key(&id)
    }

    /// Remove a pending host request by ID without completing it.
    ///
    /// Returns the request if found, `None` if the ID is not pending.
    /// Used by `VimSession::cancel_request()` to extract the request
    /// before applying its fallback.
    pub fn remove_pending_host_request(
        &mut self,
        id: crate::execution::host::HostRequestId,
    ) -> Option<crate::execution::HostRequest> {
        self.host.pending.remove(&id)
    }

    /// Number of pending (unfulfilled) async host requests.
    #[inline]
    #[must_use]
    pub fn pending_request_count(&self) -> usize {
        self.host.pending.len()
    }

    // ─── Key Injection API ─────────────────────────────────────────────

    /// Inject keys into the typeahead buffer for processing.
    ///
    /// This is the vim-core equivalent of Neovim's `feedkeys()` API.
    /// Injected keys will be processed on subsequent `drain_next_key()` calls.
    ///
    /// # Complexity
    ///
    /// Time: O(n) where n = number of characters in `keys`. The key string
    /// is parsed character-by-character to resolve Vim notation (e.g.,
    /// `<C-w>` → single key event). Each parsed key is appended to the
    /// typeahead VecDeque in O(1) amortized.
    ///
    /// Space: O(n) for the parsed key events added to the typeahead buffer.
    ///
    /// # Arguments
    /// * `keys` - Key string in Vim notation (e.g., `"jkdd"`, `"<Esc>"`, `"<C-w>h"`)
    /// * `remap` - Whether injected keys should go through mapping expansion
    ///
    /// # Example
    /// ```ignore
    /// engine.feed_keys("dd", true);  // will be remapped
    /// engine.feed_keys("<Esc>", false);  // noremap
    /// ```
    pub fn feed_keys(&mut self, keys: &str, remap: bool) {
        use super::macro_replay::parse_keys_from_string;
        use super::typeahead::{TypeaheadEntry, TypeaheadFlags};

        let parsed = parse_keys_from_string(keys);
        let flags = TypeaheadFlags::feedkeys(remap);

        self.typeahead.buffer.inject_back(
            parsed
                .into_iter()
                .map(|key| TypeaheadEntry::new(key, flags)),
        );
    }

    // ─── Mapping API ──────────────────────────────────────────────────
    // See engine/mapping.rs for: has_pending_mapping, resolve_timeout,
    // timeoutlen, set_timeoutlen, leader, set_leader, map_buffer,
    // unmap_buffer, clear_buffer_mappings, clear_buffer_mappings_for,
    // take_buffer_mappings, set_buffer_mappings

    // ─── Host Extension Registration API ─────────────────────────────

    /// Register a batch of key mappings contributed by a host extension.
    ///
    /// Each mapping in `mappings` is parsed and inserted into the global
    /// user mapping layer for every mode listed in `mapping.modes`.
    /// All inserted entries are tagged with [`MappingOwner::Host(name)`](MappingOwner::Host)
    /// so that [`unregister_host_mappings`](Self::unregister_host_mappings)
    /// can remove them atomically without touching user-defined mappings.
    ///
    /// Calling this method again with the same `name` does not clear
    /// previously registered mappings — use `unregister_host_mappings`
    /// first if you want a clean slate.
    ///
    /// # Arguments
    ///
    /// * `name` — A stable identifier for the extension (e.g., `"lsp"`,
    ///   `"my-ext"`). Used as the owner tag for lifecycle management.
    /// * `mappings` — A slice of [`HostMapping`] descriptors. LHS and RHS
    ///   are parsed via the Vim key notation parser; `<Action>(name)` and
    ///   `<Plug>(name)` references are registered in the keymap's name
    ///   registries automatically.
    pub fn register_host_mappings(&mut self, name: &str, mappings: &[HostMapping]) {
        let owner = MappingOwner::Host(compact_str::CompactString::from(name));
        for mapping in mappings {
            let lhs = super::super::key_notation::parse_key_notation_sequence(
                &mapping.lhs,
                Some(&mut self.keymap),
            );
            let rhs = super::super::key_notation::parse_key_notation_sequence(
                &mapping.rhs,
                Some(&mut self.keymap),
            );
            if lhs.is_empty() {
                continue;
            }
            let kind = MappingKind::from_recursive_flag(mapping.recursive);
            let flags = crate::keymap::MappingFlags {
                silent: mapping.silent,
                ..crate::keymap::MappingFlags::default()
            };
            let entry = MappingEntry::with_flags(rhs, kind, flags, None)
                .with_owner(owner.clone())
                .with_description(mapping.description.clone());
            for &mm in &mapping.modes {
                self.keymap.map_entry(mm, &lhs, entry.clone());
            }
        }
        self.key_interest_dirty = true;
    }

    /// Remove all key mappings previously registered by a host extension.
    ///
    /// Walks every mapping layer (global user, buffer-local, filetype-specific)
    /// and removes every entry whose owner matches
    /// [`MappingOwner::Host(name)`](MappingOwner::Host). User-defined mappings (owner
    /// [`MappingOwner::User`]) are never touched.
    ///
    /// No-ops if no mappings were registered under `name`.
    ///
    /// # Arguments
    ///
    /// * `name` — The extension identifier passed to
    ///   [`register_host_mappings`](Self::register_host_mappings).
    pub fn unregister_host_mappings(&mut self, name: &str) {
        let owner = MappingOwner::Host(compact_str::CompactString::from(name));
        self.keymap.remove_mappings_by_owner(&owner);
        self.key_interest_dirty = true;
    }

    // ─── Macro API ───────────────────────────────────────────────────
    // See engine/macro_replay.rs for: pump_macro_key (internal),
    // record_key, flush_recording

    /// The register currently being recorded into, if any.
    ///
    /// Returns `Some(register)` when `q{reg}` is active, `None` otherwise.
    #[inline]
    #[must_use]
    pub fn recording_register(&self) -> Option<RegisterName> {
        self.recording.buffer.as_ref().map(|(reg, _)| *reg)
    }

    /// Forcefully abort macro recording without flushing to a register.
    ///
    /// Used by emergency recovery paths (panic recovery, editor freed) where
    /// the recording context may be corrupted. Normal buffer switches should
    /// NOT call this — recording is a session-level concept that survives
    /// `:edit` / tab navigation, matching Vim's behavior.
    ///
    /// The in-progress recording is discarded — the register is NOT updated.
    ///
    /// Idempotent: no-op if not currently recording.
    pub fn abort_recording(&mut self) {
        self.recording.buffer = None;
        self.state.macros_mut().stop_recording();
        self.parser.set_recording(None);
    }

    // ─── Session Replay API ─────────────────────────────────────────

    /// Start recording a debugging session.
    ///
    /// While active, every call to [`process()`](Self::process) records
    /// the keystroke and its resulting effects into a [`SessionRecorder`].
    /// Call [`stop_recording_session()`](Self::stop_recording_session) to
    /// retrieve the recorder and create a [`SessionReplayer`](crate::execution::SessionReplayer) for
    /// time-travel debugging.
    ///
    /// If a recording session is already active, it is replaced with a
    /// fresh recorder (the previous recording is discarded).
    #[inline]
    pub fn start_recording_session(&mut self) {
        self.cold.recorder = Some(SessionRecorder::new());
    }

    /// Stop the active recording session and return the recorder.
    ///
    /// Returns `None` if no session was active. The returned
    /// [`SessionRecorder`] can be consumed to create a
    /// [`SessionReplayer`](crate::execution::SessionReplayer).
    #[inline]
    pub const fn stop_recording_session(&mut self) -> Option<SessionRecorder> {
        self.cold.recorder.take()
    }

    /// Read-only access to the active session recorder.
    ///
    /// Returns `None` if no session recording is active.
    #[inline]
    #[must_use]
    pub const fn session_recorder(&self) -> Option<&SessionRecorder> {
        self.cold.recorder.as_ref()
    }

    // ─── Shadow Execution API ────────────────────────────────────────

    /// Enable or disable shadow execution for macro replays.
    ///
    /// When enabled, executing a macro (`@q`) processes all pending keys
    /// against an in-memory document clone, producing a single batched
    /// diff instead of per-key host round-trips. This is transparent to
    /// the host — the `Response` contains the same semantic effects, just
    /// delivered all at once.
    ///
    /// Default is `false` (traditional per-key drain via the host loop).
    #[inline]
    pub const fn set_shadow_execution(&mut self, enabled: bool) {
        self.shadow_enabled = enabled;
    }

    /// Query whether shadow execution is currently enabled.
    ///
    /// Returns `true` if [`set_shadow_execution(true)`](Self::set_shadow_execution)
    /// has been called and not yet toggled back off.
    #[inline]
    #[must_use]
    pub const fn shadow_execution_enabled(&self) -> bool {
        self.shadow_enabled
    }

    /// Set (or replace) the self-healing shadow document text.
    ///
    /// The host should call this once after engine construction with the
    /// initial document content, and optionally again on full reloads.
    /// Subsequent edits are tracked incrementally via `apply_external_edit()`.
    /// Resets `shadow_generation` to `None` because the generation counter
    /// for new text is unknown.
    #[inline]
    pub fn set_shadow_text(&mut self, text: impl Into<String>) {
        self.shadow = Some(super::shadow_document::OwnedDocument::new(text));
        self.shadow_generation = None;
    }

    /// Clear the shadow document and generation counter.
    #[inline]
    pub fn clear_shadow(&mut self) {
        self.shadow = None;
        self.shadow_generation = None;
    }

    /// Get a reference to the shadow document text, if initialized.
    #[inline]
    #[must_use]
    pub fn shadow_text(&self) -> Option<&str> {
        self.shadow.as_ref().map(|d| {
            use crate::document::Document;
            d.text()
        })
    }

    // ─── External Edit Undo Tracking ─────────────────────────────────

    /// Take the NodeId of the most recently created external-edit undo node.
    ///
    /// Returns `Some(node_id)` if `apply_external_edit()` created an undo node
    /// since the last call to this method. Consuming (resetting to `None`) so
    /// the same node is not recorded twice.
    #[inline]
    pub const fn take_last_external_edit_node(&mut self) -> Option<crate::primitives::NodeId> {
        self.last_external_edit_node.take()
    }

    /// Returns `Some(node_id)` if `apply_external_edit()` force-committed a
    /// pending group (e.g. an active INSERT undo group) to make room for a
    /// non-merging external edit. The caller must create an `UndoStore` entry
    /// for this node.
    #[inline]
    pub const fn take_last_force_committed_node(&mut self) -> Option<crate::primitives::NodeId> {
        self.last_force_committed_node.take()
    }

    // ─── Native Insert API ────────────────────────────────────────────

    /// Whether the host handles insert-mode printable chars natively.
    ///
    /// When `true`, `would_handle_key` returns `false` for printable
    /// characters and Enter in insert mode (host passthrough path).
    /// When `false` (default), the engine handles all insert-mode keys.
    #[inline]
    #[must_use]
    pub const fn native_insert(&self) -> bool {
        self.native_insert
    }

    /// Set the native insert mode.
    ///
    /// Called by `VimSession` at construction based on
    /// `HostCapability::NativeInsert`. Should not be changed at runtime.
    #[inline]
    pub const fn set_native_insert(&mut self, enabled: bool) {
        self.native_insert = enabled;
    }

    // ─── Engine Tracing API ──────────────────────────────────────────

    /// Enable or disable structured trace event collection.
    ///
    /// When enabled, engine methods push [`TraceEvent`]s into an internal
    /// collector. Drain them with [`drain_trace_events()`](Self::drain_trace_events).
    #[cfg(feature = "engine-tracing")]
    pub fn set_tracing_enabled(&mut self, enabled: bool) {
        self.cold.trace.set_enabled(enabled);
    }

    /// Query whether trace event collection is currently enabled.
    #[cfg(feature = "engine-tracing")]
    #[must_use]
    pub fn tracing_enabled(&self) -> bool {
        self.cold.trace.is_enabled()
    }

    /// Push a trace event into the collector (if tracing is enabled).
    ///
    /// This is the public equivalent of the `trace_event!` macro, used
    /// by code outside the `engine` module (e.g., `VimSession`).
    #[cfg(feature = "engine-tracing")]
    #[inline]
    pub fn push_trace_event(&mut self, event: crate::execution::trace::TraceEvent) {
        if self.cold.trace.enabled {
            self.cold.trace.push(event);
        }
    }

    /// Drain all collected trace events, returning them.
    ///
    /// The internal buffer is left empty. Subsequent calls return an
    /// empty `Vec` until new events are collected.
    #[cfg(feature = "engine-tracing")]
    pub fn drain_trace_events(&mut self) -> Vec<crate::execution::trace::TraceEvent> {
        self.cold.trace.drain()
    }

    /// Build an [`InspectSnapshot`](crate::execution::trace::InspectSnapshot) of the current engine state.
    ///
    /// This captures mode, parser, undo tree, and configuration state.
    /// Host-level fields (`document_len`, `cursor_offset`) are defaulted
    /// to zero; use [`VimSession::inspect()`](crate::execution::HostSession) for a complete snapshot.
    #[cfg(feature = "engine-tracing")]
    #[must_use]
    pub fn inspect(&self) -> crate::execution::trace::InspectSnapshot {
        crate::execution::trace::InspectSnapshot {
            mode: compact_str::CompactString::from(self.state.mode().display_name()),
            keystroke_seq: self.keystroke_seq,
            has_pending_keys: self.has_pending_keys(),
            is_recording: self.recording.buffer.is_some(),
            is_merging: self.state.undo_tree().is_merging(),
            undo_node_count: self.state.undo_tree().node_count(),
            undo_current_node: self.state.undo_tree().current().index() as u64,
            pending_command: self.pending_command_display(),
            shadow_enabled: self.shadow_enabled,
            document_len: 0,
            cursor_offset: 0,
        }
    }

    /// Reset engine state.
    ///
    /// # Complexity
    ///
    /// Time: O(T + F + P) where T = typeahead buffer size, F = macro frame
    /// stack depth, P = pending host requests count. All are cleared.
    ///
    /// Space: O(1)
    pub fn reset(&mut self) {
        self.parser.reset();
        self.state.reset();
        self.typeahead.macro_stack.clear();
        self.typeahead.buffer.clear();
        self.host.pending.clear();
        self.typeahead.last_drained_flags = super::typeahead::TypeaheadFlags::empty();
        // Apply configurable default mode (state.reset() sets Normal; override if needed)
        let default_mode = self.resolved_options.default_mode();
        if !default_mode.is_normal() {
            self.state.set_mode(default_mode);
        }
    }

    /// Emergency reset for panic recovery.
    ///
    /// Superset of [`reset()`](Self::reset): clears everything `reset()` does
    /// (parser, transient state, typeahead, host pending), **plus** state that
    /// `reset()` deliberately preserves for normal use but becomes
    /// untrustworthy after a panic:
    ///
    /// - `is_repeating` — dot-repeat execution flag
    /// - `recording` — discard corrupted macro recording
    /// - `command_line_session` — stale session metadata
    /// - `cmd_buffer` — partial `<Cmd>...<CR>` accumulator
    /// - changelist — entries may reference phantom edit positions
    /// - macro replay depth — stale `is_replaying()` flag
    /// - macro recording register — phantom recording indicator
    /// - parser recording flag — stale `q` behavior
    /// - undo tree pending group — orphaned metadata
    /// - `fork_active` — stale speculative fork flag
    /// - syntax selection — stale incremental selection history
    /// - `sticky_column` — stale cursor column preference (Vim's `curswant`)
    ///
    /// **Preserves:** registers, marks, jumplist, search, options, keymap,
    /// mappings, providers — long-lived user state unlikely to be
    /// corrupted by a single panic.
    ///
    /// Idempotent: calling on an already-clean engine is a no-op.
    pub fn emergency_reset(&mut self) {
        self.reset();
        self.is_repeating = false;
        self.recording.buffer = None;
        self.command_line_session = None;
        self.sticky_session = None;
        self.cmd_buffer = None;
        self.state.changelist_mut().clear();
        self.state.macros_mut().reset_replay_depth();
        self.state.macros_mut().stop_recording();
        self.parser.set_recording(None);
        self.state.undo_tree_mut().abandon_pending();
        self.fork_active = false;
        self.shadow_generation = None;
        self.state.syntax_selection_mut().clear();
        self.state.set_sticky_column(None);
        // Apply configurable default mode (reset() sets Normal; override if needed)
        let default_mode = self.resolved_options.default_mode();
        if !default_mode.is_normal() {
            self.state.set_mode(default_mode);
        }
    }

    /// Get the pending command display string (showcmd).
    ///
    /// Returns a Vim-notation string showing the partially-typed command
    /// (e.g., `"3d"`, `"\"ad2f"`, `"ci"`) while the user builds a multi-key
    /// command. Returns empty string when no command is in progress.
    ///
    /// This combines the parser's intermediate state (count, register, operator,
    /// sub-state) with any pending mapping expansion keys.
    ///
    /// # Complexity
    ///
    /// Time: O(P) where P = number of pending keys in the mapping buffer
    /// (typically 0-5). Parser display is O(1) (fixed-size state).
    ///
    /// Space: O(P) for the display string.
    #[must_use]
    pub fn pending_command_display(&self) -> compact_str::CompactString {
        let mut display = self.parser.state().pending_display();

        // When a sticky session is active and the parser is idle (Ready),
        // prepend the prefix indicator with "+" to show sticky mode.
        // When the parser is mid-command (e.g. AwaitingWindowCommand),
        // its own pending_display already shows the prefix — no extra needed.
        if let Some(ref session) = self.sticky_session {
            if self.parser.state().is_ready() {
                let mut sticky_display =
                    compact_str::CompactString::new(session.target().prefix_display());
                sticky_display.push('+');
                sticky_display.push_str(&display);
                display = sticky_display;
            }
        }

        let mapping_pending = self.typeahead.buffer.pending_display();
        if !mapping_pending.is_empty() {
            display.push_str(&mapping_pending);
        }
        display
    }

    /// Returns which-key hint data if the engine is currently awaiting
    /// a prefix continuation (grammar prefix or mapping prefix).
    /// Returns `None` if no prefix is pending.
    ///
    /// Grammar prefixes (`g`, `z`, `Z`, `[`, `]`, `Ctrl-W`) are merged
    /// with user mapping continuations from the keymap trie. User mappings
    /// shadow built-in hints when the key display string matches.
    ///
    /// Mapping prefixes (multi-key LHS awaiting timeout) query the keymap
    /// trie directly.
    #[must_use]
    pub fn key_hints(&self, keymap: &Keymap) -> Option<KeyHintsInfo> {
        if let Some(info) = self.grammar_prefix_hints(keymap) {
            return Some(info);
        }
        if self.has_pending_mapping() {
            return self.mapping_prefix_hints(keymap);
        }
        None
    }

    /// Build hints for a grammar-level prefix (parser in `AwaitingPrefix`
    /// or `AwaitingWindowCommand` state).
    fn grammar_prefix_hints(&self, keymap: &Keymap) -> Option<KeyHintsInfo> {
        use crate::grammar::hints;
        use crate::grammar::input_state::InputState;
        use crate::keymap::KeyEvent;

        let state = self.parser.state();

        let (title, static_hints, has_operator, prefix_key) = match state {
            InputState::AwaitingPrefix {
                prefix, operator, ..
            } => {
                let (title, table) = hints::builtin_prefix_hints(*prefix)?;
                (title, table, operator.is_some(), KeyEvent::char(*prefix))
            }
            InputState::AwaitingWindowCommand { .. } => {
                let (title, table) = hints::window_prefix_hints();
                (title, table, false, KeyEvent::ctrl('w'))
            }
            _ => return None,
        };

        // Collect into a map keyed by display string; user mappings shadow built-ins.
        let mut hint_map: ahash::AHashMap<compact_str::CompactString, compact_str::CompactString> =
            ahash::AHashMap::new();

        // Insert static (built-in) hints, filtering out g-prefix operators when
        // an operator is already pending (e.g., after `d` we don't show `gu`, `gU`).
        for &(key, desc) in static_hints {
            if has_operator
                && prefix_key == KeyEvent::char('g')
                && hints::is_g_prefix_operator_key(key)
            {
                continue;
            }
            hint_map.insert(
                compact_str::CompactString::from(key),
                compact_str::CompactString::from(desc),
            );
        }

        // Overlay user mapping continuations (shadow built-ins by key).
        if let Some(mm) = MappingMode::from_mode(self.mode()) {
            for (continuation_key, entry) in keymap.list_continuations(mm, &[prefix_key]) {
                let key_display =
                    compact_str::CompactString::from(continuation_key.to_vim_notation().as_ref());
                let desc = if let Some(d) = entry.description() {
                    compact_str::CompactString::from(d)
                } else {
                    let mut rhs = compact_str::CompactString::default();
                    for k in entry.sequence() {
                        rhs.push_str(&k.to_string());
                    }
                    rhs
                };
                hint_map.insert(key_display, desc);
            }
        }

        // Sort by key for deterministic ordering.
        let mut hints: Vec<KeyHint> = hint_map
            .into_iter()
            .map(|(key, description)| KeyHint { key, description })
            .collect();
        hints.sort_by(|a, b| a.key.cmp(&b.key));

        Some(KeyHintsInfo {
            title: compact_str::CompactString::from(title),
            hints,
        })
    }

    /// Build hints for a mapping-level prefix (typeahead buffer has pending keys
    /// awaiting timeout resolution).
    fn mapping_prefix_hints(&self, keymap: &Keymap) -> Option<KeyHintsInfo> {
        let pending = self.typeahead.buffer.pending_keys();
        if pending.is_empty() {
            return None;
        }

        let mm = MappingMode::from_mode(self.mode())?;
        let continuations = keymap.list_continuations(mm, pending);
        if continuations.is_empty() {
            return None;
        }

        // Build a title from the pending prefix display.
        let title = {
            let mut t = compact_str::CompactString::from("Mapping (");
            t.push_str(&self.typeahead.buffer.pending_display());
            t.push(')');
            t
        };

        let mut hints: Vec<KeyHint> = continuations
            .into_iter()
            .map(|(key, entry)| {
                let key_display = compact_str::CompactString::from(key.to_vim_notation().as_ref());
                let desc = if let Some(d) = entry.description() {
                    compact_str::CompactString::from(d)
                } else {
                    let mut rhs = compact_str::CompactString::default();
                    for k in entry.sequence() {
                        rhs.push_str(&k.to_string());
                    }
                    rhs
                };
                KeyHint {
                    key: key_display,
                    description: desc,
                }
            })
            .collect();
        hints.sort_by(|a, b| a.key.cmp(&b.key));

        Some(KeyHintsInfo { title, hints })
    }

    /// Get the pending operator info if the parser is in operator-pending state.
    /// Returns (operator, count, register) if pending.
    #[must_use]
    pub const fn pending_operator(
        &self,
    ) -> Option<(crate::grammar::types::Operator, u32, Option<RegisterName>)> {
        use crate::grammar::input_state::InputState;
        match self.parser.state() {
            InputState::Operator {
                operator,
                count,
                count2,
                register,
                ..
            } => {
                let total = crate::grammar::command::compute_count(*count, *count2);
                Some((*operator, total.get(), *register))
            }
            _ => None,
        }
    }

    /// Reset just the parser state (used after handling operator-search).
    pub fn reset_parser(&mut self) {
        self.parser.reset();
    }

    // ─── Property Overlay API ────────────────────────────────────────

    /// Override the behavioral properties of a `Command` variant at runtime.
    ///
    /// Use `cmd.discriminant()` to obtain the discriminant for a given variant.
    /// The override applies to all commands with that discriminant, regardless
    /// of their field contents.
    ///
    /// # Example
    /// ```ignore
    /// use vim_core::grammar::Command;
    /// use vim_core::primitives::{CommandProperties, RepeatBehavior};
    /// use vim_core::grammar::types::Motion;
    ///
    /// let disc = Command::Motion { count: 1, motion: Motion::Down, explicit_count: false }
    ///     .discriminant();
    /// engine.set_command_properties(disc, CommandProperties {
    ///     repeat: RepeatBehavior::Record,
    ///     ..Default::default()
    /// });
    /// ```
    #[inline]
    pub fn set_command_properties(
        &mut self,
        discriminant: u16,
        props: crate::primitives::CommandProperties,
    ) {
        self.cold.property_overlay.set(discriminant, props);
    }

    /// Read-only access to the property overlay.
    #[inline]
    #[must_use]
    pub const fn property_overlay(&self) -> &crate::execution::PropertyOverlay {
        &self.cold.property_overlay
    }

    /// Mutable access to the property overlay.
    #[inline]
    pub const fn property_overlay_mut(&mut self) -> &mut crate::execution::PropertyOverlay {
        &mut self.cold.property_overlay
    }

    // ─── Federation API ──────────────────────────────────────────────

    /// Register a cross-instance state persistence backend.
    ///
    /// When set, the engine will use this backend for [`Self::load_persisted_state()`]
    /// and [`Self::persist_state()`] calls. Pass `None` to remove a registered backend.
    ///
    /// # Example
    /// ```ignore
    /// engine.set_state_backend(Some(Box::new(MyBackend::new())));
    /// engine.load_persisted_state(); // restore registers, marks, history
    /// ```
    #[inline]
    pub fn set_state_backend(
        &mut self,
        backend: Option<Box<dyn crate::state::federation::StateBackend>>,
    ) {
        self.cold.backend = backend;
    }

    /// Apply an inbound federated state event from another editor instance.
    ///
    /// Updates the engine's internal state according to the event. Events with
    /// [`EventSource::Local`](crate::state::federation::EventSource::Local) are rejected (a loop-guard: we only accept events
    /// from remote peers or the persistence backend).
    ///
    /// Currently handled events:
    /// - `RegisterChanged` — writes the register into the local register table.
    /// - `GlobalMarkChanged` — sets the corresponding global mark (offset only).
    /// - `SearchPatternChanged` — updates the search pattern and direction.
    pub fn apply_federated_event(&mut self, event: &crate::state::federation::StateEvent) {
        use crate::primitives::Mark;
        use crate::state::federation::events::{EventSource, StateEvent};

        match event {
            StateEvent::RegisterChanged { source, .. } if *source == EventSource::Local => {
                // Reject local-origin events to prevent echo loops.
            }
            StateEvent::RegisterChanged { name, content, .. } => {
                self.state.registers_mut().set(*name, content.clone());
            }
            StateEvent::GlobalMarkChanged { source, .. } if *source == EventSource::Local => {}
            StateEvent::GlobalMarkChanged { name, mark, .. } => {
                let m = Mark::from_raw(mark.offset);
                self.state.marks_mut().set(*name, m);
            }
            StateEvent::SearchPatternChanged { source, .. } if *source == EventSource::Local => {}
            StateEvent::SearchPatternChanged {
                pattern, direction, ..
            } => {
                self.state
                    .search_mut()
                    .set_pattern(pattern.clone(), *direction);
            }
            // Other event variants are observed but not applied to local state.
            _ => {}
        }
    }

    /// Load persisted state from the registered backend.
    ///
    /// Restores registers, global marks, search history, command history,
    /// and macro registers from the backend store. No-ops if no backend is set.
    ///
    /// Call this once after construction to restore a previous session.
    pub fn load_persisted_state(&mut self) {
        use crate::primitives::{Mark, MarkName, RegisterContent, RegisterName};

        let Some(ref backend) = self.cold.backend else {
            return;
        };

        // Restore named registers (a-z).
        for ch in 'a'..='z' {
            let name = RegisterName::new_unchecked(ch);
            if let Some(content) = backend.load_register(name) {
                self.state.registers_mut().set(name, content);
            }
        }

        // Restore global marks (A-Z).
        for ch in 'A'..='Z' {
            let name = MarkName::new_unchecked(ch);
            if let Some(gm) = backend.load_global_mark(name) {
                let mark = Mark::from_raw(gm.offset);
                self.state.marks_mut().set(name, mark);
            }
        }

        // Restore search history.
        let search_history = backend.load_search_history();
        if !search_history.is_empty() {
            let search = self.state.search_mut();
            for entry in search_history {
                search.set_pattern(entry, crate::primitives::SearchDirection::Forward);
            }
        }

        // Restore macro registers (a-z).
        for ch in 'a'..='z' {
            let name = RegisterName::new_unchecked(ch);
            if let Some(content) = backend.load_macro(name) {
                self.state
                    .registers_mut()
                    .set(name, RegisterContent::char_wise(content));
            }
        }
    }

    /// Flush the current state to the registered backend.
    ///
    /// Persists registers, global marks, search history, command history,
    /// and macro registers to the backend store. No-ops if no backend is set.
    ///
    /// Call this after processing keystrokes to keep the backend in sync.
    pub fn persist_state(&self) {
        use crate::primitives::{MarkName, RegisterName};

        let Some(ref backend) = self.cold.backend else {
            return;
        };

        // Persist named registers (a-z).
        for ch in 'a'..='z' {
            let name = RegisterName::new_unchecked(ch);
            if let Some(content) = self.state.registers().get(name) {
                backend.store_register(name, content);
            }
        }

        // Persist global marks (A-Z).
        for ch in 'A'..='Z' {
            let name = MarkName::new_unchecked(ch);
            if let Some(mark) = self.state.marks().get(name) {
                let gm = crate::state::federation::GlobalMark {
                    file_path: compact_str::CompactString::new(""),
                    offset: mark.offset().get(),
                    line: 0,
                    column: 0,
                    timestamp: 0,
                };
                backend.store_global_mark(name, &gm);
            }
        }

        // Persist search history.
        let history: Vec<compact_str::CompactString> =
            self.state.search().history().iter().cloned().collect();
        if !history.is_empty() {
            backend.store_search_history(&history);
        }

        // Persist macro registers (a-z)
        for ch in 'a'..='z' {
            let name = RegisterName::new_unchecked(ch);
            if let Some(content) = self.state.registers().get(name) {
                backend.store_macro(name, content.text());
            }
        }
    }

    // ─── Mode Profile Override API ────────────────────────────────────

    /// Register a custom [`ModeProfile`](crate::mode::ModeProfile) for the given mode.
    ///
    /// After this call, [`mode_profile()`](Self::mode_profile) returns the
    /// overridden profile instead of `mode.default_profile()` for keys
    /// processed in that mode.
    ///
    /// Replaces any previously registered override for `mode`.
    ///
    /// # Concerns
    ///
    /// Mode handlers currently do not have direct access to the engine.
    /// The override is stored here but will only take effect once mode
    /// handlers are wired to call `engine.mode_profile()` via `ModeContext`.
    /// That wiring is not implemented yet.
    #[inline]
    pub fn set_mode_profile(
        &mut self,
        mode: crate::primitives::Mode,
        profile: crate::mode::capabilities::ModeProfile,
    ) {
        let disc = mode_discriminant(mode);
        self.cold.profile_overrides.insert(disc, profile);
    }

    /// Resolve the effective [`ModeProfile`](crate::mode::ModeProfile) for the given mode.
    ///
    /// Returns the host-registered override if one is present, otherwise
    /// falls back to `mode.default_profile()`.
    #[inline]
    #[must_use]
    pub fn mode_profile(
        &self,
        mode: &crate::primitives::Mode,
    ) -> &crate::mode::capabilities::ModeProfile {
        let disc = mode_discriminant(*mode);
        if let Some(profile) = self.cold.profile_overrides.get(&disc) {
            profile
        } else {
            mode.default_profile()
        }
    }

    /// Remove any host-registered profile override for the given mode.
    ///
    /// After this call, [`mode_profile()`](Self::mode_profile) falls back
    /// to `mode.default_profile()` for that mode.
    #[inline]
    pub fn clear_mode_profile(&mut self, mode: &crate::primitives::Mode) {
        let disc = mode_discriminant(*mode);
        self.cold.profile_overrides.remove(&disc);
    }

    // ── Sticky Sub-Mode Configuration ────────────────────────────────────

    /// Set which prefix groups auto-activate sticky sub-mode.
    ///
    /// When a prefix group is marked sticky, completing a command in that
    /// group automatically re-enters the prefix state, allowing the user
    /// to chain commands without re-pressing the leader key.
    ///
    /// Pass an empty slice to disable all sticky prefixes.
    pub fn set_sticky_prefixes(&mut self, targets: &[StickyTarget]) {
        self.sticky_prefixes = 0;
        for &t in targets {
            self.sticky_prefixes |= sticky_target_bit(t);
        }
    }

    /// Clear all sticky prefix configuration and end any active session.
    pub const fn clear_sticky_prefixes(&mut self) {
        self.sticky_prefixes = 0;
        self.sticky_session = None;
    }

    /// Whether the given prefix group is configured for sticky mode.
    pub(crate) const fn is_prefix_sticky(&self, target: StickyTarget) -> bool {
        self.sticky_prefixes & sticky_target_bit(target) != 0
    }
}

impl Default for VimEngine {
    fn default() -> Self {
        Self::new()
    }
}

// NOTE: Creates an engine with a **default (empty) keymap**. If you need
// the engine's existing user mappings, modify state in-place via the
// targeted accessors rather than constructing a new engine from state alone.
impl From<VimState> for VimEngine {
    /// Build an engine from saved state.
    ///
    /// The keymap, parser, and macro stack are freshly initialized.
    /// User-defined mappings are **not** carried over — call
    /// [`Keymap`] methods after construction to restore them.
    fn from(state: VimState) -> Self {
        Self {
            parser: Parser::new(),
            keymap: Keymap::default(),
            state,
            dispatcher: ModeDispatcher::new(),
            is_repeating: false,
            command_line_session: None,
            sticky_session: None,
            sticky_prefixes: 0,
            options: VimOptions::default(),
            buffer_overrides: OptionOverrides::default(),
            window_overrides: OptionOverrides::default(),
            resolved_options: VimOptions::default(),
            options_dirty: false,
            digraph_registry: crate::primitives::DigraphRegistry::new(),
            abbrev_table: crate::primitives::AbbrevTable::default(),
            langmap_table: crate::keymap::LangmapTable::new(),
            host: HostCoordinator::new(),
            typeahead: TypeaheadCoordinator::new(),
            recording: RecordingState::new(),
            engine_providers: EngineProviders::new(),
            handler_map: crate::keymap::HandlerMap::new(),
            keystroke_seq: 0,
            prediction_weights: crate::execution::predictive::PredictionWeights::new(),
            fork_active: false,
            shadow_enabled: false,
            shadow: None,
            shadow_generation: None,
            last_external_edit_node: None,
            last_force_committed_node: None,
            native_insert: false,
            cmd_buffer: None,
            pending_macro_error: None,
            key_interest_dirty: true,
            viewport_first_line: 0,
            viewport_height: 24,
            terminal_cols: 80,
            terminal_rows: 24,
            diagnostics_count: 0,
            cold: Box::new(super::EngineColdState {
                pipeline: None,
                recorder: None,
                property_overlay: crate::execution::PropertyOverlay::new(),
                profile_overrides: ahash::AHashMap::new(),
                hooks: super::hooks::HookBus::new(),
                event_registry: crate::execution::event_registry::EventRegistry::new(),
                backend: None,
                #[cfg(feature = "engine-tracing")]
                trace: crate::execution::trace::TraceCollector::new(),
            }),
        }
    }
}

#[cfg(test)]
#[path = "host_extension_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "public_api_emergency_reset_tests.rs"]
mod emergency_reset_tests;

#[cfg(test)]
#[path = "host_settings_tests.rs"]
mod host_settings_tests;

#[cfg(test)]
#[path = "external_selection_tests.rs"]
mod external_selection_tests;

#[cfg(test)]
#[path = "key_hints_tests.rs"]
mod key_hints_tests;

// ── Internal helpers ─────────────────────────────────────────────────────────

const fn sticky_target_bit(target: StickyTarget) -> u8 {
    match target {
        StickyTarget::Window => 1,
        StickyTarget::ZPrefix => 2,
    }
}

/// Compute a stable `u8` discriminant for a `Mode` value.
///
/// This is used as the key in `VimEngine::profile_overrides`. All variants
/// of `Mode` that share the same "shape" (e.g., all `Visual(_)` variants)
/// map to the same discriminant so that a single override covers all
/// sub-variants.
///
/// The mapping is:
///   Normal            → 0
///   Insert            → 1
///   Visual(_)         → 2
///   Select(_)         → 3
///   Replace           → 4
///   VirtualReplace    → 5
///   CommandLine       → 6
///   OperatorPending(_)→ 7
#[inline]
const fn mode_discriminant(mode: crate::primitives::Mode) -> u8 {
    use crate::primitives::Mode;
    match mode {
        Mode::Normal => 0,
        Mode::Insert => 1,
        Mode::Visual(_) => 2,
        Mode::Select(_) => 3,
        Mode::Replace => 4,
        Mode::VirtualReplace => 5,
        Mode::CommandLine => 6,
        Mode::OperatorPending(_) => 7,
    }
}
