//! Mapping API methods for `VimEngine`.
//!
//! This module contains the shell-facing API for user key mappings:
//! mapping CRUD, leader key management, timeoutlen configuration,
//! buffer-local mapping lifecycle, and mapping expansion draining.
//!
//! Extracted from `engine.rs` for single-responsibility decomposition.

use super::VimEngine;
use crate::keymap::{
    BufferMappings, KeyEvent, MappingEntry, MappingFlags, MappingKind, MappingMode, TrieLookup,
};

impl VimEngine {
    // ═══════════════════════════════════════════════════════════════════════
    // Mapping expansion
    // ═══════════════════════════════════════════════════════════════════════

    /// Whether the mapping expander has buffered keys waiting for more input.
    ///
    /// When this returns `true` after `process()`, the shell should start a
    /// timer of [`Self::timeoutlen()`] ms. If no further key arrives before the
    /// timer fires, the shell calls [`Self::resolve_timeout()`].
    ///
    /// Note: this checks the **pending mapping prefix** sub-buffer (multi-key
    /// LHS awaiting timeout resolution), NOT the main typeahead buffer. For
    /// checking whether there are keys to drain, use [`Self::has_pending_keys()`].
    #[inline]
    #[must_use]
    pub const fn has_pending_mapping(&self) -> bool {
        self.typeahead.buffer.has_pending()
    }

    /// Format the pending mapping keys as a Vim notation string for display.
    ///
    /// Returns an empty string if no mapping keys are pending.
    #[must_use]
    pub fn pending_mapping_display(&self) -> compact_str::CompactString {
        self.typeahead.buffer.pending_display()
    }

    /// Returns the effective timeout in milliseconds for the current pending
    /// mapping prefix, or `None` if there is no pending input.
    ///
    /// When the sequence starts with Escape (typical for terminal escape codes
    /// such as arrow keys), `ttimeoutlen` is used instead of `timeoutlen` so
    /// that pressing bare `<Esc>` is recognised quickly:
    ///
    /// - `ttimeoutlen == -1` (default): fall back to `timeoutlen`.
    /// - `ttimeoutlen >= 0`: use that value directly (clamped to 0 if negative
    ///   due to the `i32` storage; -1 is the only sentinel).
    ///
    /// For non-Escape prefixes the full `timeoutlen` is always used.
    #[must_use]
    pub fn pending_timeout_ms(&self) -> Option<u64> {
        if !self.has_pending_mapping() {
            return None;
        }
        let starts_with_esc = self.typeahead.buffer.pending_starts_with_escape();
        let timeout = if starts_with_esc {
            match self.options.ttimeoutlen_ms() {
                -1 => u64::from(self.options.timeoutlen_ms()),
                n => u64::try_from(n).unwrap_or(0),
            }
        } else {
            u64::from(self.options.timeoutlen_ms())
        };
        Some(timeout)
    }

    /// Whether the given key could start a mapping in the current mode.
    ///
    /// Returns `true` if the keymap has any mapping (exact or prefix) whose
    /// LHS begins with `key` in the current mode. Used by the shell's
    /// passthrough system: mappings take priority over passthrough, so if
    /// a key could trigger a mapping, it must not be passed through to Godot.
    #[must_use]
    pub fn could_start_mapping(&self, key: KeyEvent) -> bool {
        let mode = self.state.mode();
        MappingMode::from_mode(mode)
            .is_some_and(|mm| !matches!(self.keymap.lookup(mm, &[key]), TrieLookup::NoMatch))
    }

    /// Force-resolve any pending mapping prefix (called when timeoutlen expires).
    ///
    /// If the current prefix has an exact match, expands it. Otherwise, flushes
    /// the buffered keys as NOREMAP literals into the typeahead buffer.
    ///
    /// The resolved first key is injected into the typeahead buffer (not returned
    /// to the caller) so the shell can drain all resolved keys uniformly via
    /// [`Self::drain_next_key()`]. The NOREMAP flag on flushed keys prevents them from
    /// being recaptured by the same mapping prefix that caused the timeout.
    pub fn resolve_timeout(&mut self) {
        use crate::execution::engine::typeahead::{ResolveResult, TypeaheadEntry};
        let mode = self.state.mode();
        debug!(target: "vim::engine::mapping", "mapping lookup resolved (mode={mode:?})");
        match self.typeahead.buffer.force_resolve(&self.keymap, mode) {
            Some(ResolveResult::Dispatch(key, flags)) => {
                // Inject back into the buffer so drain_next_key() picks it up.
                // The flags (typically NOREMAP for flushed literals, or
                // noremap_rhs for expanded mappings) are preserved, and
                // process() honors them via drained_flags to prevent
                // re-entering the pending state.
                self.typeahead
                    .buffer
                    .inject_front([TypeaheadEntry::new(key, flags)]);
            }
            Some(ResolveResult::ExprMapping {
                expression,
                kind,
                mode: mapping_mode,
                silent,
            }) => {
                // Timeout resolved to an <expr> mapping — inject the
                // evaluation request as a pending host request. The host
                // will evaluate the expression and inject the result keys.
                let response = self.handle_expr_mapping(expression, mapping_mode, kind, silent);
                // Store effects/requests for the next drain cycle.
                // (handle_expr_mapping produces a HostRequest that the
                // shell will pick up via drain_next_key → process.)
                let _ = response;
            }
            Some(ResolveResult::RecursionOverflow) => {
                self.typeahead.buffer.clear();
            }
            Some(ResolveResult::Pending) | None => {}
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Configuration
    // ═══════════════════════════════════════════════════════════════════════

    /// Get the current mapping timeout in milliseconds.
    #[inline]
    #[must_use]
    pub const fn timeoutlen(&self) -> u32 {
        self.options.timeoutlen_ms()
    }

    /// Set the mapping timeout in milliseconds.
    #[inline]
    pub const fn set_timeoutlen(&mut self, ms: u32) {
        self.options.set_timeoutlen_ms(ms);
    }

    /// Get the current leader key.
    #[inline]
    #[must_use]
    pub const fn leader(&self) -> KeyEvent {
        self.keymap.leader()
    }

    /// Set the leader key.
    ///
    /// Only affects mappings defined **after** this call.
    #[inline]
    pub const fn set_leader(&mut self, key: KeyEvent) {
        self.keymap.set_leader(key);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // User mapping CRUD
    // ═══════════════════════════════════════════════════════════════════════

    /// Add a user mapping in the global mapping layer.
    #[inline]
    pub fn map(
        &mut self,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
    ) {
        self.typeahead.buffer.clear();
        self.keymap.map(mm, from, to, kind, flags);
        self.key_interest_dirty = true;
    }

    /// Add a user mapping with full flag support (including `<expr>` text).
    ///
    /// When `flags.expr` is `true`, `expr_text` holds the expression string
    /// to evaluate at expansion time. The `to` parameter is ignored for expr
    /// mappings.
    #[inline]
    pub fn map_with_expr(
        &mut self,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
        expr_text: Option<compact_str::CompactString>,
    ) {
        self.typeahead.buffer.clear();
        self.keymap
            .map_with_expr(mm, from, to, kind, flags, expr_text);
        self.key_interest_dirty = true;
    }

    /// Remove a user mapping from the global mapping layer.
    #[inline]
    pub fn unmap(&mut self, mm: MappingMode, from: &[KeyEvent]) -> Option<MappingEntry> {
        self.typeahead.buffer.clear();
        self.key_interest_dirty = true;
        self.keymap.unmap(mm, from)
    }

    /// Clear all user mappings across all modes.
    #[inline]
    pub fn clear_mappings(&mut self) {
        self.typeahead.buffer.clear();
        self.keymap.clear_all_mappings();
        self.key_interest_dirty = true;
    }

    /// Clear user mappings for a specific mode.
    #[inline]
    pub fn clear_mappings_for(&mut self, mm: MappingMode) {
        self.typeahead.buffer.clear();
        self.keymap.clear(mm);
        self.key_interest_dirty = true;
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Buffer-local mapping CRUD
    // ═══════════════════════════════════════════════════════════════════════

    /// Add a buffer-local mapping.
    #[inline]
    pub fn map_buffer(
        &mut self,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
    ) {
        self.keymap.map_buffer(mm, from, to, kind, flags);
        self.key_interest_dirty = true;
    }

    /// Remove a buffer-local mapping.
    #[inline]
    pub fn unmap_buffer(&mut self, mm: MappingMode, from: &[KeyEvent]) -> Option<MappingEntry> {
        self.key_interest_dirty = true;
        self.keymap.unmap_buffer(mm, from)
    }

    /// Clear all buffer-local mappings across all modes.
    #[inline]
    pub fn clear_buffer_mappings(&mut self) {
        self.keymap.clear_all_buffer_mappings();
        self.key_interest_dirty = true;
    }

    /// Clear buffer-local mappings for a specific mode.
    #[inline]
    pub fn clear_buffer_mappings_for(&mut self, mm: MappingMode) {
        self.keymap.clear_buffer(mm);
        self.key_interest_dirty = true;
    }

    /// Extract buffer-local mappings for the current buffer.
    ///
    /// Call before switching away from a buffer.
    /// Also resets the typeahead buffer to flush any pending keys that
    /// were being matched against the old buffer's trie.
    #[inline]
    pub fn take_buffer_mappings(&mut self) -> BufferMappings {
        self.typeahead.buffer.clear();
        self.key_interest_dirty = true;
        self.keymap.take_buffer_mappings()
    }

    /// Install buffer-local mappings for a buffer.
    ///
    /// Call after switching to a buffer.
    /// Also resets the typeahead buffer to ensure no stale pending state
    /// from the previous buffer context leaks into the new context.
    #[inline]
    pub fn set_buffer_mappings(&mut self, bm: BufferMappings) {
        self.typeahead.buffer.clear();
        self.keymap.set_buffer_mappings(bm);
        self.key_interest_dirty = true;
    }
}
