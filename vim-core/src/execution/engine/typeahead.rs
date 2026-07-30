//! Unified typeahead buffer for key event sequencing.
//!
//! Centralizes all key sources — user input, macro replay, mapping RHS,
//! feedkeys — into a single FIFO with per-key metadata flags. This is
//! the data-structure foundation for the unified typeahead refactor,
//! inspired by Neovim's `typebuf` + `tb_noremap[]` architecture.
//!
//! # Architecture
//!
//! ```text
//!  User input ──┐
//!  Macro replay ─┼─→ TypeaheadBuffer (VecDeque<TypeaheadEntry>) ──→ pop_front()
//!  Mapping RHS ──┤                                                      │
//!  feedkeys() ───┘                                                      ▼
//!                                                              Engine dispatch
//! ```
//!
//! Each entry carries [`TypeaheadFlags`] indicating its origin and
//! remapping eligibility, allowing the engine to make correct decisions
//! about recording, mapping expansion, and recursion control.

use arrayvec::ArrayVec;
use bitflags::bitflags;
use std::collections::VecDeque;

#[cfg(test)]
use crate::keymap::MappingFlags;
use crate::keymap::{Key, KeyEvent, Keymap, MappingKind, MappingMode, TrieLookup};
use crate::primitives::Mode;
use compact_str::CompactString;

/// Maximum LHS key sequence length we buffer for mapping prefix matching.
///
/// 8 keys is generous for any practical multi-key mapping LHS.
const MAX_PENDING: usize = 8;

/// Maximum recursion depth for recursive mapping expansion.
///
/// Prevents infinite loops from circular mappings like
/// `:map a b` + `:map b a`.
const MAX_RECURSION_DEPTH: u8 = 100;

/// Maximum number of entries in the typeahead buffer.
///
/// Derived from existing bounds: macro depth (1000) × average keys per
/// macro (10) = 10,000. Prevents unbounded memory growth from
/// pathological mapping/macro expansion without being stricter than the
/// recursion limits already in place.
const MAX_TYPEAHEAD_LEN: usize = 10_000;

// ═══════════════════════════════════════════════════════════════════════
// ResolveResult
// ═══════════════════════════════════════════════════════════════════════

/// Result of resolving a key through the mapping expansion pipeline.
///
/// Returned by [`TypeaheadBuffer::resolve_key()`] and
/// [`TypeaheadBuffer::force_resolve()`] to tell the engine what to do next.
#[derive(Debug)]
pub(in crate::execution::engine) enum ResolveResult {
    /// Key resolved — dispatch it to the parser. Carries the resolved key
    /// and the flags indicating its origin/remapping eligibility.
    Dispatch(KeyEvent, TypeaheadFlags),
    /// Multi-key LHS prefix detected — wait for more input (or timeout).
    /// The engine should return `Response::pending_response()` to the shell,
    /// which starts the mapping timeout timer.
    Pending,
    /// Mapping recursion overflow (depth > `MAX_RECURSION_DEPTH`).
    /// Indicates a circular mapping like `:map a b` + `:map b a`.
    /// The engine should abort expansion and report an error.
    RecursionOverflow,
    /// An `<expr>` mapping was triggered. The engine should emit a
    /// `HostRequest::EvaluateMapping` and pause until the host returns
    /// the evaluated key sequence.
    ExprMapping {
        /// The expression text (the mapping's RHS) to be evaluated by the host.
        expression: CompactString,
        /// The mapping kind (recursive vs non-recursive).
        kind: MappingKind,
        /// The mapping mode in which the mapping was triggered.
        mode: MappingMode,
        /// Whether the mapping was defined with `<silent>`.
        silent: bool,
    },
}

// ═══════════════════════════════════════════════════════════════════════
// TypeaheadFlags
// ═══════════════════════════════════════════════════════════════════════

bitflags! {
    /// Per-key metadata flags for typeahead entries.
    ///
    /// Inspired by Neovim's `tb_noremap[]` array, these flags track each
    /// key's origin and mapping eligibility. The engine uses them to decide:
    ///
    /// - **Mapping expansion**: `REMAPPABLE` keys go through the trie;
    ///   `NOREMAP` keys bypass it.
    /// - **Recording**: only `TYPED` keys are recorded into macro registers.
    /// - **Live insert detection**: `TYPED` distinguishes real user input
    ///   from replayed/expanded keys.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub(in crate::execution::engine) struct TypeaheadFlags: u8 {
        /// Key is eligible for mapping expansion (Neovim RM_YES).
        const REMAPPABLE = 0b0001;
        /// Key came from user typing (controls recording, `is_live_insert_input`).
        const TYPED      = 0b0010;
        /// Key skips mapping expansion (Neovim RM_NONE, from noremap RHS).
        const NOREMAP    = 0b0100;
        /// Key came from a `<silent>` mapping — `ShowMessage` effects are
        /// suppressed. Propagates through recursive mapping expansion.
        const SILENT     = 0b1000;
    }
}

impl TypeaheadFlags {
    /// Standard user keystroke: remappable and typed.
    ///
    /// This is the default for keys arriving from the host's key event handler.
    /// They go through mapping expansion and are recorded into macro registers.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) const fn user_typed() -> Self {
        Self::REMAPPABLE.union(Self::TYPED)
    }

    /// Macro replay key: remappable but not typed.
    ///
    /// Macro keys go through mapping expansion (`:map` applies during replay)
    /// but are not recorded (avoids recording keys that are themselves replays).
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) const fn macro_key() -> Self {
        Self::REMAPPABLE
    }

    /// Non-recursive mapping RHS key: noremap, not typed.
    ///
    /// Keys from `:noremap` / `:nnoremap` RHS bypass mapping expansion
    /// entirely, preventing infinite recursion.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) const fn noremap_rhs() -> Self {
        Self::NOREMAP
    }

    /// Recursive mapping RHS key: remappable, not typed.
    ///
    /// Keys from `:map` / `:nmap` RHS go through mapping expansion again,
    /// enabling chains like `<Plug>(name)` expansion.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) const fn recursive_rhs() -> Self {
        Self::REMAPPABLE
    }

    /// Feedkeys key: remappable or noremap based on the `remap` flag.
    ///
    /// Mirrors Vim's `feedkeys({string}, {mode})` where the `m` flag
    /// controls whether the injected keys are remappable.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) const fn feedkeys(remap: bool) -> Self {
        if remap {
            Self::REMAPPABLE
        } else {
            Self::NOREMAP
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// TypeaheadEntry
// ═══════════════════════════════════════════════════════════════════════

/// A single entry in the typeahead buffer: a key event with metadata flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::execution::engine) struct TypeaheadEntry {
    /// The key event.
    pub key: KeyEvent,
    /// Per-key metadata flags (remappable, typed, noremap).
    pub flags: TypeaheadFlags,
}

impl TypeaheadEntry {
    /// Create a new typeahead entry.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) const fn new(key: KeyEvent, flags: TypeaheadFlags) -> Self {
        Self { key, flags }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// TypeaheadBuffer
// ═══════════════════════════════════════════════════════════════════════

/// Unified typeahead key buffer.
///
/// Centralizes all key sources into a single FIFO queue with per-key
/// metadata. The engine pops entries from the front and uses the flags
/// to determine recording, mapping expansion, and other per-key behavior.
///
/// # Buffer layout
///
/// ```text
/// front ←── pop direction ──── back
/// [entry_0, entry_1, ..., entry_n]
///
/// inject_front: inserts at front (mapping RHS, macro keys)
/// inject_back:  appends at back (feedkeys append mode)
/// pop_front:    consumes from front (engine dispatch)
/// ```
///
/// # Pending sub-buffer
///
/// The `pending` field accumulates a multi-key prefix during mapping LHS
/// matching. When a timeout fires or no match is found, pending keys are
/// flushed back to the main buffer front with `NOREMAP` flags to prevent
/// re-matching.
#[derive(Clone)]
pub(in crate::execution::engine) struct TypeaheadBuffer {
    /// Main buffer. Front = next key to consume.
    buf: VecDeque<TypeaheadEntry>,
    /// Multi-key LHS prefix accumulator for mapping expansion.
    pending: ArrayVec<KeyEvent, MAX_PENDING>,
    /// Recursion depth for mapping expansion (max `MAX_RECURSION_DEPTH`).
    mapping_recursion_depth: u8,
    /// Flags of the last key returned by `pop_front()`.
    ///
    /// Used by the engine to determine recording/remap behavior for the
    /// current `process()` call without passing flags through the entire
    /// dispatch chain.
    last_popped_flags: TypeaheadFlags,
}

impl TypeaheadBuffer {
    // ─── Construction ──────────────────────────────────────────────────

    /// Create a new empty typeahead buffer.
    #[must_use]
    pub(in crate::execution::engine) fn new() -> Self {
        Self {
            buf: VecDeque::new(),
            pending: ArrayVec::new(),
            mapping_recursion_depth: 0,
            last_popped_flags: TypeaheadFlags::empty(),
        }
    }

    // ─── Core buffer operations ────────────────────────────────────────

    /// Prepend entries at the front of the buffer.
    ///
    /// Used for mapping RHS injection and macro replay keys. The entries
    /// maintain their relative order: the first item in `keys` becomes
    /// the next key to be popped.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Buffer: [C, D]
    /// // inject_front([A, B])
    /// // Buffer: [A, B, C, D]
    /// ```
    /// Inject a single key at the front with NOREMAP flag.
    ///
    /// Used for re-processing keys that were consumed but should be
    /// dispatched normally (like Neovim's `vungetc()`).
    pub(in crate::execution::engine) fn inject_front_noremap(&mut self, key: KeyEvent) {
        self.inject_front(std::iter::once(TypeaheadEntry::new(
            key,
            TypeaheadFlags::NOREMAP,
        )));
    }

    pub(in crate::execution::engine) fn inject_front(
        &mut self,
        keys: impl IntoIterator<Item = TypeaheadEntry>,
    ) {
        let entries: Vec<TypeaheadEntry> = keys.into_iter().collect();
        let entry_count = entries.len();
        let available = MAX_TYPEAHEAD_LEN.saturating_sub(self.buf.len());
        let to_inject = available.min(entry_count);
        self.buf.reserve(to_inject);
        for entry in entries.into_iter().take(to_inject).rev() {
            self.buf.push_front(entry);
        }
        if to_inject < entry_count {
            #[allow(clippy::panic, reason = "debug-only overflow detection")]
            {
                debug_assert!(
                    false,
                    "typeahead overflow: {} + {} > {MAX_TYPEAHEAD_LEN}",
                    self.buf.len(),
                    entry_count
                );
            }
        }
    }

    /// Append entries at the back of the buffer.
    ///
    /// Used for `feedkeys()` append mode where injected keys should be
    /// processed after all currently buffered keys.
    pub(in crate::execution::engine) fn inject_back(
        &mut self,
        keys: impl IntoIterator<Item = TypeaheadEntry>,
    ) {
        let available = MAX_TYPEAHEAD_LEN.saturating_sub(self.buf.len());
        let mut overflow = false;
        for (i, entry) in keys.into_iter().enumerate() {
            if i >= available {
                overflow = true;
                break;
            }
            self.buf.push_back(entry);
        }
        if overflow {
            #[allow(clippy::panic, reason = "debug-only overflow detection")]
            {
                debug_assert!(
                    false,
                    "typeahead overflow: truncated at {MAX_TYPEAHEAD_LEN}"
                );
            }
        }
    }

    /// Consume the next entry from the front of the buffer.
    ///
    /// Stores the popped entry's flags in `last_popped_flags` for the
    /// engine to query after dispatch without threading flags through
    /// every function call.
    ///
    /// Returns `None` if the buffer is empty.
    pub(in crate::execution::engine) fn pop_front(&mut self) -> Option<TypeaheadEntry> {
        let entry = self.buf.pop_front()?;
        self.last_popped_flags = entry.flags;
        Some(entry)
    }

    /// Whether the main buffer is empty.
    ///
    /// Note: this does NOT consider the pending sub-buffer. Use
    /// `has_pending()` to check for pending mapping prefix keys.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Number of entries in the main buffer.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(in crate::execution::engine) fn len(&self) -> usize {
        self.buf.len()
    }

    /// Clear all state: main buffer, pending sub-buffer, and recursion depth.
    ///
    /// Used on mode transitions, error recovery, and macro abort.
    pub(in crate::execution::engine) fn clear(&mut self) {
        self.buf.clear();
        self.pending.clear();
        self.mapping_recursion_depth = 0;
        self.last_popped_flags = TypeaheadFlags::empty();
    }

    /// Flags from the last `pop_front()` call.
    ///
    /// Returns `TypeaheadFlags::empty()` if no key has been popped yet
    /// or after `clear()`.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(in crate::execution::engine) const fn last_popped_flags(&self) -> TypeaheadFlags {
        self.last_popped_flags
    }

    // ─── Pending sub-buffer (mapping LHS prefix matching) ──────────────

    /// Add a key to the pending mapping prefix accumulator.
    ///
    /// Called when the mapping trie returns a prefix match and we need
    /// to buffer the key while waiting for more input or a timeout.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if the pending buffer is full (`MAX_PENDING`).
    /// In release mode, silently drops the key when full.
    #[cfg(test)]
    pub(in crate::execution::engine) fn push_pending(&mut self, key: KeyEvent) {
        if self.pending.is_full() {
            debug_assert!(false, "pending buffer overflow: MAX_PENDING={MAX_PENDING}");
            return;
        }
        self.pending.push(key);
    }

    /// Number of keys in the pending mapping prefix buffer.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(in crate::execution::engine) const fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Slice view of the pending mapping prefix buffer for trie lookup.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(in crate::execution::engine) fn pending_slice(&self) -> &[KeyEvent] {
        self.pending.as_slice()
    }

    /// Clear the pending mapping prefix buffer.
    #[cfg(test)]
    pub(in crate::execution::engine) fn clear_pending(&mut self) {
        self.pending.clear();
    }

    /// Flush all pending keys to the front of the main buffer as `NOREMAP` entries.
    ///
    /// Called on timeout when the pending prefix did not resolve to a mapping.
    /// The keys are marked `NOREMAP` to prevent them from being recaptured as
    /// mapping prefixes, which would create an infinite pending loop.
    ///
    /// After this call, `pending` is empty and the flushed keys are at the
    /// front of the main buffer ready for dispatch.
    #[cfg(test)]
    pub(in crate::execution::engine) fn flush_pending_to_front(&mut self) {
        // Insert in reverse order so the first pending key ends up at the front.
        for key in self.pending.drain(..).rev() {
            self.buf
                .push_front(TypeaheadEntry::new(key, TypeaheadFlags::NOREMAP));
        }
    }

    /// Whether there are keys in the pending mapping prefix buffer.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) const fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Slice view of the pending mapping prefix keys.
    ///
    /// Used by `key_hints()` to query the keymap trie for mapping
    /// continuations after the currently buffered prefix.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) fn pending_keys(&self) -> &[KeyEvent] {
        self.pending.as_slice()
    }

    /// Whether the first pending (unprocessed) key is an Escape key.
    ///
    /// Used by the engine to decide between `ttimeoutlen` and `timeoutlen`
    /// when a mapping prefix timeout fires: sequences starting with Escape
    /// typically come from terminal escape codes and should time out quickly.
    #[inline]
    #[must_use]
    pub(in crate::execution::engine) fn pending_starts_with_escape(&self) -> bool {
        self.pending.first().is_some_and(|k| k.key == Key::Escape)
    }

    // ─── Mapping recursion tracking ────────────────────────────────────

    /// Current mapping recursion depth.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(in crate::execution::engine) const fn mapping_recursion_depth(&self) -> u8 {
        self.mapping_recursion_depth
    }

    /// Increment the mapping recursion depth.
    ///
    /// Returns `Err(())` if the depth would exceed `MAX_RECURSION_DEPTH`,
    /// indicating a circular mapping. The caller should abort expansion
    /// and report an error.
    #[cfg(test)]
    pub(in crate::execution::engine) const fn increment_recursion(&mut self) -> Result<(), ()> {
        if self.mapping_recursion_depth >= MAX_RECURSION_DEPTH {
            return Err(());
        }
        self.mapping_recursion_depth += 1;
        Ok(())
    }

    /// Reset the mapping recursion depth to zero.
    ///
    /// Called when an expansion chain completes (all expanded keys consumed)
    /// or on error/abort.
    #[inline]
    #[cfg(test)]
    pub(in crate::execution::engine) const fn reset_recursion_depth(&mut self) {
        self.mapping_recursion_depth = 0;
    }

    // ─── Mapping resolution ─────────────────────────────────────────────

    /// Resolve a key through the mapping expansion pipeline.
    ///
    /// This is the unified mapping expansion entry point. It
    /// takes a key event with its flags, runs mapping expansion against the
    /// keymap trie, and returns what the engine should do next:
    ///
    /// - `Dispatch(key, flags)`: process this key through mode dispatch.
    /// - `Pending`: waiting for more keys to complete a multi-key mapping LHS.
    /// - `RecursionOverflow`: circular mapping detected; abort and report error.
    ///
    /// # Algorithm
    ///
    /// 1. **Mode check**: modes without user mappings (Replace, VirtualReplace,
    ///    CommandLine) bypass expansion entirely.
    /// 2. **NOREMAP check**: keys with the `NOREMAP` flag skip mapping lookup
    ///    (from `:noremap` RHS or timeout-flushed literals).
    /// 3. **Pending accumulation**: push key into the pending sub-buffer.
    /// 4. **Trie lookup**: query the keymap trie with the accumulated pending keys.
    ///    - `NoMatch`: flush pending keys as literals (first dispatched, rest queued).
    ///    - `ExactOnly`: unambiguous match — expand the mapping RHS.
    ///    - `Prefix`: could become a longer mapping — wait for more input or timeout.
    pub(in crate::execution::engine) fn resolve_key(
        &mut self,
        key: KeyEvent,
        flags: TypeaheadFlags,
        keymap: &Keymap,
        mode: Mode,
    ) -> ResolveResult {
        // Step 1: Modes without user mappings bypass expansion entirely.
        let Some(mm) = MappingMode::from_mode(mode) else {
            return ResolveResult::Dispatch(key, flags);
        };

        // Step 2: NOREMAP keys skip mapping lookup.
        // Exception: <Plug> keys always go through mapping expansion even in noremap context.
        if flags.contains(TypeaheadFlags::NOREMAP) && !matches!(key.key, Key::Plug(_)) {
            return ResolveResult::Dispatch(key, flags);
        }

        // Step 3: Accumulate into pending buffer.
        if self.pending.is_full() {
            // Buffer full — flush everything as literals, including the new key.
            return self.flush_pending_with_new_key(key);
        }
        self.pending.push(key);

        // Step 4: Trie lookup against the full pending buffer.
        let lookup = keymap.lookup(mm, self.pending.as_slice());

        match lookup {
            TrieLookup::NoMatch => {
                if self.pending.len() == 1 {
                    // Single key, no match — fast path: dispatch directly.
                    self.pending.clear();
                    ResolveResult::Dispatch(key, flags)
                } else {
                    // Multi-key accumulated but no match — flush as literals.
                    self.flush_pending_as_literals(flags)
                }
            }
            TrieLookup::ExactOnly(entry) => {
                // Check for <expr> mapping — delegate to host instead of expanding.
                if entry.expr() {
                    let expression = entry.expression().unwrap_or("").into();
                    let kind = entry.kind();
                    // Silent propagates: either from the entry itself or from the
                    // incoming key flags (recursive expansion chain).
                    let silent = entry.silent() || flags.contains(TypeaheadFlags::SILENT);
                    self.pending.clear();
                    return ResolveResult::ExprMapping {
                        expression,
                        kind,
                        mode: mm,
                        silent,
                    };
                }
                // Unambiguous match — expand immediately.
                let sequence = entry.sequence().to_vec();
                let kind = entry.kind();
                // Silent propagates: either from the entry itself or from the
                // incoming key flags (recursive expansion chain).
                let silent = entry.silent() || flags.contains(TypeaheadFlags::SILENT);
                // Snapshot pending keys as the LHS before clearing, for REMAP_SKIP.
                let lhs: ArrayVec<KeyEvent, MAX_PENDING> = self.pending.clone();
                self.pending.clear();
                self.inject_mapping_rhs(&sequence, &lhs, kind, silent)
            }
            TrieLookup::Prefix { .. } => {
                // Could become a longer mapping — wait for more keys or timeout.
                ResolveResult::Pending
            }
        }
    }

    /// Force-resolve any pending prefix (called on mapping timeout).
    ///
    /// If there's an exact match at the current pending prefix, expand it.
    /// Otherwise, flush all buffered keys as NOREMAP literals into the buffer
    /// front and return the first one for dispatch.
    ///
    /// Returns `None` if the pending buffer is empty (no timeout to resolve).
    pub(in crate::execution::engine) fn force_resolve(
        &mut self,
        keymap: &Keymap,
        mode: Mode,
    ) -> Option<ResolveResult> {
        if self.pending.is_empty() {
            return None;
        }

        let Some(mm) = MappingMode::from_mode(mode) else {
            // Shouldn't happen — pending should be empty in non-mapped modes.
            // Flush defensively.
            return Some(self.force_flush_all_pending());
        };

        let lookup = keymap.lookup(mm, self.pending.as_slice());
        match lookup {
            TrieLookup::Prefix { exact: Some(entry) } | TrieLookup::ExactOnly(entry) => {
                // Check for <expr> mapping — delegate to host instead of expanding.
                if entry.expr() {
                    let expression = entry.expression().unwrap_or("").into();
                    let kind = entry.kind();
                    let silent = entry.silent();
                    self.pending.clear();
                    return Some(ResolveResult::ExprMapping {
                        expression,
                        kind,
                        mode: mm,
                        silent,
                    });
                }
                // Exact match found — expand it.
                let sequence = entry.sequence().to_vec();
                let kind = entry.kind();
                let silent = entry.silent();
                // Snapshot pending keys as the LHS before clearing, for REMAP_SKIP.
                let lhs: ArrayVec<KeyEvent, MAX_PENDING> = self.pending.clone();
                self.pending.clear();
                Some(self.inject_mapping_rhs(&sequence, &lhs, kind, silent))
            }
            TrieLookup::Prefix { exact: None } | TrieLookup::NoMatch => {
                // No exact match at this prefix — flush all pending as literals.
                Some(self.force_flush_all_pending())
            }
        }
    }

    /// Format the pending keys as a Vim notation string for status bar display.
    ///
    /// Returns an empty string if no keys are pending.
    pub(in crate::execution::engine) fn pending_display(&self) -> CompactString {
        if self.pending.is_empty() {
            return CompactString::default();
        }
        let mut out = CompactString::default();
        for key in &self.pending {
            out.push_str(&key.to_vim_notation());
        }
        out
    }

    // ─── Private mapping helpers ────────────────────────────────────────

    /// Inject a mapping entry's RHS into the typeahead buffer.
    ///
    /// For recursive mappings (`:map`): increments recursion depth and injects
    /// RHS keys with `REMAPPABLE` flags so they go through mapping lookup again.
    ///
    /// For non-recursive mappings (`:noremap`): injects RHS keys with `NOREMAP`
    /// flags to bypass further mapping expansion.
    ///
    /// When `silent` is `true`, `TypeaheadFlags::SILENT` is OR'd into all
    /// injected keys' flags, causing `ShowMessage` effects to be suppressed
    /// during the mapping's execution.
    ///
    /// Returns `Dispatch(first_rhs_key, flags)` for immediate processing.
    /// If RHS is empty, dispatches an Escape (degenerate case, matches
    /// `the legacy expander::expand_entry` behavior).
    fn inject_mapping_rhs(
        &mut self,
        sequence: &[KeyEvent],
        lhs: &[KeyEvent],
        kind: MappingKind,
        silent: bool,
    ) -> ResolveResult {
        if sequence.is_empty() {
            // Degenerate: empty RHS (e.g., `:map x <Nop>`).
            // Match the legacy expander behavior: dispatch Escape.
            return ResolveResult::Dispatch(KeyEvent::escape(), TypeaheadFlags::NOREMAP);
        }

        let is_recursive = kind.is_recursive();

        let mut rhs_flags = if is_recursive {
            // Recursive: increment depth, check overflow.
            self.mapping_recursion_depth += 1;
            if self.mapping_recursion_depth > MAX_RECURSION_DEPTH {
                self.clear();
                return ResolveResult::RecursionOverflow;
            }
            TypeaheadFlags::recursive_rhs()
        } else {
            TypeaheadFlags::noremap_rhs()
        };

        // Propagate SILENT flag to all expanded keys.
        if silent {
            rhs_flags |= TypeaheadFlags::SILENT;
        }

        // REMAP_SKIP: For recursive mappings where the RHS starts with the
        // same key sequence as the LHS, mark the overlapping prefix keys as
        // NOREMAP to prevent infinite self-recursion. Example: `:map x xfoo`
        // — the leading `x` in the RHS must be dispatched as a literal,
        // otherwise it re-triggers the mapping infinitely.
        let skip_len = if is_recursive {
            Self::common_prefix_len(sequence, lhs)
        } else {
            0
        };

        // Inject remaining RHS keys (after the first) at the front of the buffer.
        // They'll be consumed by subsequent pop_front() calls.
        if sequence.len() > 1 {
            let remaining: Vec<TypeaheadEntry> = sequence
                .iter()
                .enumerate()
                .skip(1)
                .map(|(i, k)| {
                    let flags = if i < skip_len {
                        // Within the REMAP_SKIP prefix: use NOREMAP to prevent
                        // self-recursion, but preserve SILENT if set.
                        let mut f = TypeaheadFlags::noremap_rhs();
                        if silent {
                            f |= TypeaheadFlags::SILENT;
                        }
                        f
                    } else {
                        rhs_flags
                    };
                    TypeaheadEntry::new(*k, flags)
                })
                .collect();
            self.inject_front(remaining);
        }

        // Return the first RHS key for immediate dispatch.
        // Safety: we checked sequence.is_empty() above, so first() is Some.
        let first = match sequence.first().copied() {
            Some(k) => k,
            None => return ResolveResult::Dispatch(KeyEvent::escape(), TypeaheadFlags::NOREMAP),
        };

        // First key: if within REMAP_SKIP prefix, dispatch as NOREMAP.
        let first_flags = if skip_len > 0 {
            let mut f = TypeaheadFlags::noremap_rhs();
            if silent {
                f |= TypeaheadFlags::SILENT;
            }
            f
        } else {
            rhs_flags
        };
        ResolveResult::Dispatch(first, first_flags)
    }

    /// Compute the length of the common prefix between two key sequences.
    ///
    /// Used by REMAP_SKIP to determine how many leading keys of a recursive
    /// mapping's RHS match the LHS. Those keys are dispatched as NOREMAP
    /// to prevent infinite self-recursion.
    fn common_prefix_len(a: &[KeyEvent], b: &[KeyEvent]) -> usize {
        a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
    }

    /// Flush all pending keys as literals when no match is found.
    ///
    /// The first pending key is dispatched (returned). All remaining pending
    /// keys plus are injected at the buffer front with `NOREMAP` flags to
    /// prevent re-capture as mapping prefixes.
    ///
    /// # Precondition
    ///
    /// `self.pending.len() >= 2` (single-key case is handled inline by
    /// `resolve_key()`).
    fn flush_pending_as_literals(&mut self, original_flags: TypeaheadFlags) -> ResolveResult {
        debug_assert!(
            self.pending.len() >= 2,
            "flush_pending_as_literals called with {} pending keys",
            self.pending.len()
        );

        // The first pending key is the one we dispatch.
        let first = match self.pending.first().copied() {
            Some(k) => k,
            None => return ResolveResult::Pending, // defensive
        };

        // Remaining pending keys go to buffer front as NOREMAP.
        // Prevents re-entering the mapping trie (e.g., second 'g' of 'gg'
        // re-triggering pending state from 'gh'/'gd' mappings). Consistent
        // with flush_pending_to_front (timeout path). Matches Vim behavior.
        if self.pending.len() > 1 {
            let remaining: Vec<TypeaheadEntry> = self
                .pending
                .iter()
                .skip(1)
                .map(|k| TypeaheadEntry::new(*k, TypeaheadFlags::NOREMAP))
                .collect();
            self.inject_front(remaining);
        }
        self.pending.clear();

        // Dispatch the first key with NOREMAP to prevent re-capture.
        // Use NOREMAP because: if the first key was part of a failed multi-key
        // prefix, dispatching it with REMAPPABLE would just re-enter pending
        // state (e.g., 'j' re-entering prefix for 'jk').
        let _ = original_flags; // acknowledged but not used — always NOREMAP for safety
        ResolveResult::Dispatch(first, TypeaheadFlags::NOREMAP)
    }

    /// Flush pending buffer plus a new key that caused a full-buffer overflow.
    ///
    /// First pending key is dispatched with NOREMAP (prevent re-capture).
    /// Remaining pending keys + new key go to buffer front as REMAPPABLE
    /// (fresh keys eligible for their own mappings).
    fn flush_pending_with_new_key(&mut self, new_key: KeyEvent) -> ResolveResult {
        let first = if self.pending.is_empty() {
            new_key
        } else {
            // First pending key dispatched with NOREMAP (prevent re-capture).
            // Remaining + new_key use NOREMAP (prevent trie re-entry).
            let Some(first) = self.pending.first().copied() else {
                return ResolveResult::Dispatch(new_key, TypeaheadFlags::NOREMAP);
            };
            let mut remaining: Vec<TypeaheadEntry> = self
                .pending
                .iter()
                .skip(1)
                .map(|k| TypeaheadEntry::new(*k, TypeaheadFlags::NOREMAP))
                .collect();
            remaining.push(TypeaheadEntry::new(new_key, TypeaheadFlags::NOREMAP));
            self.inject_front(remaining);
            self.pending.clear();
            first
        };
        ResolveResult::Dispatch(first, TypeaheadFlags::NOREMAP)
    }

    /// Flush all pending keys for force_resolve: first dispatched, rest queued.
    ///
    /// Used by `force_resolve()` when no exact match exists. Unlike
    /// `flush_pending_as_literals` (which uses REMAPPABLE for remaining
    /// keys), force-flushed keys ALL use NOREMAP. Rationale: timeout
    /// expiry means the user stopped typing, so the accumulated keys
    /// represent a single "abandoned gesture" — re-entering mapping
    /// resolution would be surprising. In practice this rarely matters
    /// since `force_resolve` is typically called with a single pending
    /// key (the prefix that timed out).
    fn force_flush_all_pending(&mut self) -> ResolveResult {
        debug_assert!(!self.pending.is_empty());

        let first = match self.pending.first().copied() {
            Some(k) => k,
            None => return ResolveResult::Pending, // defensive
        };

        // Remaining keys go to buffer front as NOREMAP.
        if self.pending.len() > 1 {
            let remaining: Vec<TypeaheadEntry> = self
                .pending
                .iter()
                .skip(1)
                .map(|k| TypeaheadEntry::new(*k, TypeaheadFlags::NOREMAP))
                .collect();
            self.inject_front(remaining);
        }
        self.pending.clear();

        ResolveResult::Dispatch(first, TypeaheadFlags::NOREMAP)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Helper constructors ───────────────────────────────────────────

    fn entry(c: char, flags: TypeaheadFlags) -> TypeaheadEntry {
        TypeaheadEntry::new(KeyEvent::char(c), flags)
    }

    fn user_entry(c: char) -> TypeaheadEntry {
        entry(c, TypeaheadFlags::user_typed())
    }

    fn noremap_entry(c: char) -> TypeaheadEntry {
        entry(c, TypeaheadFlags::noremap_rhs())
    }

    fn macro_entry(c: char) -> TypeaheadEntry {
        entry(c, TypeaheadFlags::macro_key())
    }

    // ─── TypeaheadFlags tests ──────────────────────────────────────────

    #[test]
    fn flag_user_typed_has_remappable_and_typed() {
        let f = TypeaheadFlags::user_typed();
        assert!(f.contains(TypeaheadFlags::REMAPPABLE));
        assert!(f.contains(TypeaheadFlags::TYPED));
        assert!(!f.contains(TypeaheadFlags::NOREMAP));
    }

    #[test]
    fn flag_macro_key_has_remappable_only() {
        let f = TypeaheadFlags::macro_key();
        assert!(f.contains(TypeaheadFlags::REMAPPABLE));
        assert!(!f.contains(TypeaheadFlags::TYPED));
        assert!(!f.contains(TypeaheadFlags::NOREMAP));
    }

    #[test]
    fn flag_noremap_rhs_has_noremap_only() {
        let f = TypeaheadFlags::noremap_rhs();
        assert!(f.contains(TypeaheadFlags::NOREMAP));
        assert!(!f.contains(TypeaheadFlags::REMAPPABLE));
        assert!(!f.contains(TypeaheadFlags::TYPED));
    }

    #[test]
    fn flag_recursive_rhs_is_remappable() {
        let f = TypeaheadFlags::recursive_rhs();
        assert!(f.contains(TypeaheadFlags::REMAPPABLE));
        assert!(!f.contains(TypeaheadFlags::TYPED));
        assert!(!f.contains(TypeaheadFlags::NOREMAP));
    }

    #[test]
    fn flag_recursive_rhs_equals_macro_key() {
        // Both are just REMAPPABLE — intentionally the same bits.
        assert_eq!(TypeaheadFlags::recursive_rhs(), TypeaheadFlags::macro_key());
    }

    #[test]
    fn flag_feedkeys_remap_true_is_remappable() {
        let f = TypeaheadFlags::feedkeys(true);
        assert!(f.contains(TypeaheadFlags::REMAPPABLE));
        assert!(!f.contains(TypeaheadFlags::NOREMAP));
    }

    #[test]
    fn flag_feedkeys_remap_false_is_noremap() {
        let f = TypeaheadFlags::feedkeys(false);
        assert!(f.contains(TypeaheadFlags::NOREMAP));
        assert!(!f.contains(TypeaheadFlags::REMAPPABLE));
    }

    #[test]
    fn flag_default_is_empty() {
        let f = TypeaheadFlags::default();
        assert!(f.is_empty());
        assert!(!f.contains(TypeaheadFlags::REMAPPABLE));
        assert!(!f.contains(TypeaheadFlags::TYPED));
        assert!(!f.contains(TypeaheadFlags::NOREMAP));
    }

    #[test]
    fn flag_bitwise_operations() {
        let f = TypeaheadFlags::REMAPPABLE | TypeaheadFlags::TYPED;
        assert_eq!(f, TypeaheadFlags::user_typed());
        assert!(f.contains(TypeaheadFlags::REMAPPABLE));
        assert!(f.contains(TypeaheadFlags::TYPED));

        let f2 = f & TypeaheadFlags::REMAPPABLE;
        assert_eq!(f2, TypeaheadFlags::REMAPPABLE);
        assert!(!f2.contains(TypeaheadFlags::TYPED));
    }

    // ─── TypeaheadEntry tests ──────────────────────────────────────────

    #[test]
    fn entry_preserves_key_and_flags() {
        let e = TypeaheadEntry::new(KeyEvent::char('x'), TypeaheadFlags::user_typed());
        assert_eq!(e.key, KeyEvent::char('x'));
        assert_eq!(e.flags, TypeaheadFlags::user_typed());
    }

    #[test]
    fn entry_equality() {
        let a = user_entry('a');
        let b = user_entry('a');
        let c = noremap_entry('a');
        assert_eq!(a, b);
        assert_ne!(a, c); // same key, different flags
    }

    // ─── TypeaheadBuffer: empty state ──────────────────────────────────

    #[test]
    fn new_buffer_is_empty() {
        let buf = TypeaheadBuffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
        assert!(!buf.has_pending());
        assert_eq!(buf.pending_len(), 0);
        assert_eq!(buf.mapping_recursion_depth(), 0);
        assert_eq!(buf.last_popped_flags(), TypeaheadFlags::empty());
    }

    #[test]
    fn pop_from_empty_returns_none() {
        let mut buf = TypeaheadBuffer::new();
        assert_eq!(buf.pop_front(), None);
    }

    // ─── TypeaheadBuffer: FIFO ordering ────────────────────────────────

    #[test]
    fn fifo_basic_ordering() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('a'), user_entry('b'), user_entry('c')]);

        assert_eq!(buf.len(), 3);
        assert!(!buf.is_empty());

        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('a'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('b'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('c'));
        assert_eq!(buf.pop_front(), None);
        assert!(buf.is_empty());
    }

    #[test]
    fn fifo_preserves_flags_through_push_pop() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('a'), noremap_entry('b'), macro_entry('c')]);

        let a = buf.pop_front().unwrap();
        assert_eq!(a.flags, TypeaheadFlags::user_typed());

        let b = buf.pop_front().unwrap();
        assert_eq!(b.flags, TypeaheadFlags::noremap_rhs());

        let c = buf.pop_front().unwrap();
        assert_eq!(c.flags, TypeaheadFlags::macro_key());
    }

    // ─── TypeaheadBuffer: inject_front ─────────────────────────────────

    #[test]
    fn inject_front_prepends_in_order() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('c'), user_entry('d')]);
        buf.inject_front([user_entry('a'), user_entry('b')]);

        assert_eq!(buf.len(), 4);
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('a'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('b'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('c'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('d'));
    }

    #[test]
    fn inject_front_on_empty_buffer() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_front([noremap_entry('x'), noremap_entry('y')]);

        assert_eq!(buf.len(), 2);
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('x'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('y'));
    }

    #[test]
    fn inject_front_multiple_times_stacks_correctly() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('z')]);
        buf.inject_front([user_entry('b')]);
        buf.inject_front([user_entry('a')]);

        // a was injected last to front, so it should be first
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('a'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('b'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('z'));
    }

    // ─── TypeaheadBuffer: inject_back ──────────────────────────────────

    #[test]
    fn inject_back_appends_in_order() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('a'), user_entry('b')]);
        buf.inject_back([user_entry('c'), user_entry('d')]);

        assert_eq!(buf.len(), 4);
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('a'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('b'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('c'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('d'));
    }

    #[test]
    fn inject_back_with_empty_iterator() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back(std::iter::empty());
        assert!(buf.is_empty());
    }

    // ─── TypeaheadBuffer: clear ────────────────────────────────────────

    #[test]
    fn clear_resets_everything() {
        let mut buf = TypeaheadBuffer::new();

        // Populate main buffer
        buf.inject_back([user_entry('a'), user_entry('b')]);
        // Populate pending
        buf.push_pending(KeyEvent::char('x'));
        buf.push_pending(KeyEvent::char('y'));
        // Increment recursion
        buf.increment_recursion().unwrap();
        buf.increment_recursion().unwrap();
        // Pop something to set last_popped_flags
        buf.pop_front();

        // Verify non-empty state
        assert!(!buf.is_empty());
        assert!(buf.has_pending());
        assert_eq!(buf.mapping_recursion_depth(), 2);
        assert_ne!(buf.last_popped_flags(), TypeaheadFlags::empty());

        // Clear
        buf.clear();

        // Verify all reset
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
        assert!(!buf.has_pending());
        assert_eq!(buf.pending_len(), 0);
        assert_eq!(buf.mapping_recursion_depth(), 0);
        assert_eq!(buf.last_popped_flags(), TypeaheadFlags::empty());
    }

    // ─── TypeaheadBuffer: last_popped_flags ────────────────────────────

    #[test]
    fn last_popped_flags_tracks_each_pop() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('a'), noremap_entry('b')]);

        buf.pop_front();
        assert_eq!(buf.last_popped_flags(), TypeaheadFlags::user_typed());

        buf.pop_front();
        assert_eq!(buf.last_popped_flags(), TypeaheadFlags::noremap_rhs());
    }

    #[test]
    fn last_popped_flags_unchanged_on_empty_pop() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('a')]);
        buf.pop_front();

        // Now buffer is empty, pop returns None
        assert_eq!(buf.pop_front(), None);
        // last_popped_flags should still reflect the last successful pop
        assert_eq!(buf.last_popped_flags(), TypeaheadFlags::user_typed());
    }

    // ─── TypeaheadBuffer: pending sub-buffer ───────────────────────────

    #[test]
    fn pending_push_and_query() {
        let mut buf = TypeaheadBuffer::new();

        assert!(!buf.has_pending());
        assert_eq!(buf.pending_len(), 0);
        assert!(buf.pending_slice().is_empty());

        buf.push_pending(KeyEvent::char('j'));
        assert!(buf.has_pending());
        assert_eq!(buf.pending_len(), 1);
        assert_eq!(buf.pending_slice(), &[KeyEvent::char('j')]);

        buf.push_pending(KeyEvent::char('k'));
        assert_eq!(buf.pending_len(), 2);
        assert_eq!(
            buf.pending_slice(),
            &[KeyEvent::char('j'), KeyEvent::char('k')]
        );
    }

    #[test]
    fn pending_clear() {
        let mut buf = TypeaheadBuffer::new();
        buf.push_pending(KeyEvent::char('j'));
        buf.push_pending(KeyEvent::char('k'));

        buf.clear_pending();
        assert!(!buf.has_pending());
        assert_eq!(buf.pending_len(), 0);
        assert!(buf.pending_slice().is_empty());
    }

    #[test]
    fn pending_clear_does_not_affect_main_buffer() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('a')]);
        buf.push_pending(KeyEvent::char('x'));

        buf.clear_pending();

        assert!(!buf.is_empty());
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('a'));
    }

    #[test]
    fn flush_pending_to_front_moves_keys_with_noremap() {
        let mut buf = TypeaheadBuffer::new();

        // Existing entries in main buffer
        buf.inject_back([user_entry('c')]);

        // Add pending keys
        buf.push_pending(KeyEvent::char('a'));
        buf.push_pending(KeyEvent::char('b'));

        // Flush
        buf.flush_pending_to_front();

        // Pending should be cleared
        assert!(!buf.has_pending());
        assert_eq!(buf.pending_len(), 0);

        // Main buffer should have: [a(NOREMAP), b(NOREMAP), c(user)]
        assert_eq!(buf.len(), 3);

        let a = buf.pop_front().unwrap();
        assert_eq!(a.key, KeyEvent::char('a'));
        assert_eq!(a.flags, TypeaheadFlags::NOREMAP);

        let b = buf.pop_front().unwrap();
        assert_eq!(b.key, KeyEvent::char('b'));
        assert_eq!(b.flags, TypeaheadFlags::NOREMAP);

        let c = buf.pop_front().unwrap();
        assert_eq!(c.key, KeyEvent::char('c'));
        assert_eq!(c.flags, TypeaheadFlags::user_typed());
    }

    #[test]
    fn flush_pending_to_front_empty_pending_is_noop() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('a')]);

        buf.flush_pending_to_front();

        assert_eq!(buf.len(), 1);
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('a'));
    }

    #[test]
    fn flush_pending_to_front_on_empty_buffer() {
        let mut buf = TypeaheadBuffer::new();
        buf.push_pending(KeyEvent::char('x'));
        buf.push_pending(KeyEvent::char('y'));

        buf.flush_pending_to_front();

        assert_eq!(buf.len(), 2);
        let x = buf.pop_front().unwrap();
        assert_eq!(x.key, KeyEvent::char('x'));
        assert_eq!(x.flags, TypeaheadFlags::NOREMAP);

        let y = buf.pop_front().unwrap();
        assert_eq!(y.key, KeyEvent::char('y'));
        assert_eq!(y.flags, TypeaheadFlags::NOREMAP);
    }

    // ─── TypeaheadBuffer: recursion depth ──────────────────────────────

    #[test]
    fn recursion_starts_at_zero() {
        let buf = TypeaheadBuffer::new();
        assert_eq!(buf.mapping_recursion_depth(), 0);
    }

    #[test]
    fn increment_recursion_counts_up() {
        let mut buf = TypeaheadBuffer::new();
        assert!(buf.increment_recursion().is_ok());
        assert_eq!(buf.mapping_recursion_depth(), 1);
        assert!(buf.increment_recursion().is_ok());
        assert_eq!(buf.mapping_recursion_depth(), 2);
    }

    #[test]
    fn increment_recursion_errors_at_max_depth() {
        let mut buf = TypeaheadBuffer::new();

        // Fill to MAX_RECURSION_DEPTH
        for _ in 0..MAX_RECURSION_DEPTH {
            assert!(buf.increment_recursion().is_ok());
        }
        assert_eq!(buf.mapping_recursion_depth(), MAX_RECURSION_DEPTH);

        // Next increment should fail
        assert!(buf.increment_recursion().is_err());
        // Depth should NOT have changed
        assert_eq!(buf.mapping_recursion_depth(), MAX_RECURSION_DEPTH);
    }

    #[test]
    fn reset_recursion_depth_clears_counter() {
        let mut buf = TypeaheadBuffer::new();
        for _ in 0..50 {
            buf.increment_recursion().unwrap();
        }
        assert_eq!(buf.mapping_recursion_depth(), 50);

        buf.reset_recursion_depth();
        assert_eq!(buf.mapping_recursion_depth(), 0);
    }

    #[test]
    fn clear_also_resets_recursion_depth() {
        let mut buf = TypeaheadBuffer::new();
        buf.increment_recursion().unwrap();
        buf.increment_recursion().unwrap();

        buf.clear();
        assert_eq!(buf.mapping_recursion_depth(), 0);
    }

    // ─── TypeaheadBuffer: mixed operations ─────────────────────────────

    #[test]
    fn interleaved_inject_front_and_back() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('b')]);
        buf.inject_front([noremap_entry('a')]);
        buf.inject_back([macro_entry('c')]);

        assert_eq!(buf.len(), 3);

        let a = buf.pop_front().unwrap();
        assert_eq!(a.key, KeyEvent::char('a'));
        assert_eq!(a.flags, TypeaheadFlags::noremap_rhs());

        let b = buf.pop_front().unwrap();
        assert_eq!(b.key, KeyEvent::char('b'));
        assert_eq!(b.flags, TypeaheadFlags::user_typed());

        let c = buf.pop_front().unwrap();
        assert_eq!(c.key, KeyEvent::char('c'));
        assert_eq!(c.flags, TypeaheadFlags::macro_key());
    }

    #[test]
    fn pop_partial_then_inject_front() {
        let mut buf = TypeaheadBuffer::new();
        buf.inject_back([user_entry('a'), user_entry('b'), user_entry('c')]);

        // Pop one
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('a'));

        // Inject at front
        buf.inject_front([noremap_entry('x')]);

        // x should come before b
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('x'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('b'));
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('c'));
    }

    #[test]
    fn pending_and_main_buffer_independent() {
        let mut buf = TypeaheadBuffer::new();

        // Put entries in both
        buf.inject_back([user_entry('a')]);
        buf.push_pending(KeyEvent::char('x'));

        // Main buffer has 1, pending has 1
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.pending_len(), 1);

        // Pop from main doesn't affect pending
        buf.pop_front();
        assert_eq!(buf.pending_len(), 1);

        // Clear pending doesn't affect main (already popped)
        buf.clear_pending();
        assert!(buf.is_empty());
    }

    #[test]
    fn inject_front_with_special_keys() {
        let mut buf = TypeaheadBuffer::new();
        let esc = TypeaheadEntry::new(KeyEvent::escape(), TypeaheadFlags::noremap_rhs());
        let enter = TypeaheadEntry::new(KeyEvent::enter(), TypeaheadFlags::user_typed());
        let ctrl_w = TypeaheadEntry::new(KeyEvent::ctrl('w'), TypeaheadFlags::macro_key());

        buf.inject_front([esc, enter, ctrl_w]);

        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::escape());
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::enter());
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::ctrl('w'));
    }

    #[test]
    fn large_batch_inject_and_drain() {
        let mut buf = TypeaheadBuffer::new();
        let entries: Vec<TypeaheadEntry> = (b'a'..=b'z').map(|b| user_entry(b as char)).collect();

        buf.inject_back(entries);
        assert_eq!(buf.len(), 26);

        for expected in b'a'..=b'z' {
            let e = buf.pop_front().unwrap();
            assert_eq!(e.key, KeyEvent::char(expected as char));
            assert_eq!(e.flags, TypeaheadFlags::user_typed());
        }
        assert!(buf.is_empty());
    }

    // ═════════════════════════════════════════════════════════════════════
    // resolve_key / force_resolve tests
    // ═════════════════════════════════════════════════════════════════════

    use crate::keymap::{key_sequence, MappingKind, MappingMode};

    /// Build a keymap with the given normal-mode mappings.
    fn km_with(mappings: &[(&[KeyEvent], Vec<KeyEvent>, MappingKind)]) -> Keymap {
        let mut km = Keymap::default();
        for (from, to, kind) in mappings {
            km.map(
                MappingMode::Normal,
                from,
                to.clone(),
                *kind,
                MappingFlags::default(),
            );
        }
        km
    }

    /// Build a keymap with insert-mode mappings.
    fn km_insert_with(mappings: &[(&[KeyEvent], Vec<KeyEvent>, MappingKind)]) -> Keymap {
        let mut km = Keymap::default();
        for (from, to, kind) in mappings {
            km.map(
                MappingMode::Insert,
                from,
                to.clone(),
                *kind,
                MappingFlags::default(),
            );
        }
        km
    }

    /// Assert a ResolveResult is Dispatch with the expected key and flags.
    fn assert_dispatch(
        result: &ResolveResult,
        expected_key: KeyEvent,
        expected_flags: TypeaheadFlags,
    ) {
        match result {
            ResolveResult::Dispatch(k, f) => {
                assert_eq!(*k, expected_key, "dispatch key mismatch");
                assert_eq!(*f, expected_flags, "dispatch flags mismatch");
            }
            other => panic!(
                "expected Dispatch({:?}, {:?}), got {:?}",
                expected_key, expected_flags, other
            ),
        }
    }

    /// Assert a ResolveResult is Pending.
    fn assert_pending(result: &ResolveResult) {
        assert!(
            matches!(result, ResolveResult::Pending),
            "expected Pending, got {:?}",
            result,
        );
    }

    /// Assert a ResolveResult is RecursionOverflow.
    #[allow(dead_code, reason = "available for future recursion overflow tests")]
    fn assert_overflow(result: &ResolveResult) {
        assert!(
            matches!(result, ResolveResult::RecursionOverflow),
            "expected RecursionOverflow, got {:?}",
            result,
        );
    }

    // ─── resolve_key: no mappings (passthrough) ─────────────────────────

    #[test]
    fn resolve_no_mappings_passthrough() {
        let mut buf = TypeaheadBuffer::new();
        let km = Keymap::default();
        let key = KeyEvent::char('j');
        let flags = TypeaheadFlags::user_typed();

        let result = buf.resolve_key(key, flags, &km, Mode::Normal);
        assert_dispatch(&result, key, flags);
        assert!(!buf.has_pending());
        assert!(buf.is_empty());
    }

    // ─── resolve_key: single-key exact match ────────────────────────────

    #[test]
    fn resolve_single_key_exact_noremap() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('c'), KeyEvent::char('l')]),
            MappingKind::NonRecursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        // First RHS key dispatched with NOREMAP flags (non-recursive).
        assert_dispatch(&result, KeyEvent::char('c'), TypeaheadFlags::noremap_rhs());
        // Remaining RHS key ('l') should be in the buffer with NOREMAP.
        assert_eq!(buf.len(), 1);
        let l_entry = buf.pop_front().unwrap();
        assert_eq!(l_entry.key, KeyEvent::char('l'));
        assert_eq!(l_entry.flags, TypeaheadFlags::noremap_rhs());
        assert!(!buf.has_pending());
    }

    #[test]
    fn resolve_single_key_exact_recursive() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('a')],
            key_sequence(&[KeyEvent::char('b')]),
            MappingKind::Recursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        // Recursive: dispatched with REMAPPABLE flags.
        assert_dispatch(
            &result,
            KeyEvent::char('b'),
            TypeaheadFlags::recursive_rhs(),
        );
        assert_eq!(buf.mapping_recursion_depth(), 1);
        assert!(buf.is_empty()); // single-key RHS, nothing queued
        assert!(!buf.has_pending());
    }

    // ─── resolve_key: multi-key LHS ─────────────────────────────────────

    #[test]
    fn resolve_multi_key_pending_then_complete() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // Feed 'j' — prefix of 'jk' → Pending.
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);
        assert!(buf.has_pending());
        assert_eq!(buf.pending_len(), 1);

        // Feed 'k' — completes 'jk' → Dispatch(Escape, NOREMAP).
        let r2 = buf.resolve_key(
            KeyEvent::char('k'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r2, KeyEvent::escape(), TypeaheadFlags::noremap_rhs());
        assert!(!buf.has_pending());
        assert!(buf.is_empty()); // single-key RHS
    }

    #[test]
    fn resolve_multi_key_no_match_flush() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // Feed 'j' → Pending (prefix of 'jk').
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);

        // Feed 'x' → NoMatch for 'jx' → flush: dispatch 'j' (NOREMAP), queue 'x' (REMAPPABLE).
        // The first key 'j' is NOREMAP to prevent re-capture as prefix of 'jk'.
        // All flushed keys use NOREMAP to prevent trie re-entry.
        let r2 = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r2, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);
        assert!(!buf.has_pending());

        assert_eq!(buf.len(), 1);
        let x_entry = buf.pop_front().unwrap();
        assert_eq!(x_entry.key, KeyEvent::char('x'));
        assert_eq!(x_entry.flags, TypeaheadFlags::NOREMAP);
    }

    // ─── resolve_key: NOREMAP bypass ────────────────────────────────────

    #[test]
    fn resolve_noremap_key_skips_mapping_lookup() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )]);

        // Feed 's' with NOREMAP flag — should bypass mapping lookup entirely.
        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::NOREMAP,
            &km,
            Mode::Normal,
        );
        assert_dispatch(&result, KeyEvent::char('s'), TypeaheadFlags::NOREMAP);
        assert!(!buf.has_pending());
        assert!(buf.is_empty());
    }

    // ─── resolve_key: prefix match → Pending ────────────────────────────

    #[test]
    fn resolve_prefix_returns_pending() {
        let mut buf = TypeaheadBuffer::new();
        // Both 'j' and 'jk' are mapped — 'j' alone produces Prefix.
        let mut km = Keymap::default();
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );

        let result = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&result);
        assert!(buf.has_pending());
        assert_eq!(buf.pending_len(), 1);
    }

    // ─── resolve_key: no match with single key ──────────────────────────

    #[test]
    fn resolve_single_key_no_match_dispatches_directly() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )]);

        // Feed 'j' — no mapping for 'j' → dispatch directly with original flags.
        let flags = TypeaheadFlags::user_typed();
        let result = buf.resolve_key(KeyEvent::char('j'), flags, &km, Mode::Normal);
        assert_dispatch(&result, KeyEvent::char('j'), flags);
        assert!(!buf.has_pending());
    }

    // ─── resolve_key: mode without mappings ─────────────────────────────

    #[test]
    fn resolve_replace_mode_passthrough() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )]);

        // Replace mode → no MappingMode → bypass.
        let flags = TypeaheadFlags::user_typed();
        let result = buf.resolve_key(KeyEvent::char('s'), flags, &km, Mode::Replace);
        assert_dispatch(&result, KeyEvent::char('s'), flags);
    }

    #[test]
    fn resolve_command_line_mode_passthrough() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )]);

        let flags = TypeaheadFlags::user_typed();
        let result = buf.resolve_key(KeyEvent::char('s'), flags, &km, Mode::CommandLine);
        assert_dispatch(&result, KeyEvent::char('s'), flags);
    }

    #[test]
    fn resolve_virtual_replace_mode_passthrough() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )]);

        let flags = TypeaheadFlags::user_typed();
        let result = buf.resolve_key(KeyEvent::char('s'), flags, &km, Mode::VirtualReplace);
        assert_dispatch(&result, KeyEvent::char('s'), flags);
    }

    // ─── resolve_key: recursive mapping RHS → keys injected REMAPPABLE ──

    #[test]
    fn resolve_recursive_multi_key_rhs_injects_remappable() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[
                KeyEvent::char('d'),
                KeyEvent::char('i'),
                KeyEvent::char('w'),
            ]),
            MappingKind::Recursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        // First key 'd' dispatched with REMAPPABLE.
        assert_dispatch(
            &result,
            KeyEvent::char('d'),
            TypeaheadFlags::recursive_rhs(),
        );
        assert_eq!(buf.mapping_recursion_depth(), 1);

        // Remaining 'i', 'w' in buffer with REMAPPABLE flags.
        assert_eq!(buf.len(), 2);
        let i_entry = buf.pop_front().unwrap();
        assert_eq!(i_entry.key, KeyEvent::char('i'));
        assert_eq!(i_entry.flags, TypeaheadFlags::recursive_rhs());
        let w_entry = buf.pop_front().unwrap();
        assert_eq!(w_entry.key, KeyEvent::char('w'));
        assert_eq!(w_entry.flags, TypeaheadFlags::recursive_rhs());
    }

    // ─── resolve_key: noremap mapping RHS → keys injected NOREMAP ───────

    #[test]
    fn resolve_noremap_multi_key_rhs_injects_noremap() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[
                KeyEvent::char('d'),
                KeyEvent::char('i'),
                KeyEvent::char('w'),
            ]),
            MappingKind::NonRecursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        // First key 'd' dispatched with NOREMAP.
        assert_dispatch(&result, KeyEvent::char('d'), TypeaheadFlags::noremap_rhs());

        // Remaining 'i', 'w' in buffer with NOREMAP flags.
        assert_eq!(buf.len(), 2);
        let i_entry = buf.pop_front().unwrap();
        assert_eq!(i_entry.key, KeyEvent::char('i'));
        assert_eq!(i_entry.flags, TypeaheadFlags::noremap_rhs());
        let w_entry = buf.pop_front().unwrap();
        assert_eq!(w_entry.key, KeyEvent::char('w'));
        assert_eq!(w_entry.flags, TypeaheadFlags::noremap_rhs());
    }

    // ─── resolve_key: recursion overflow ────────────────────────────────

    #[test]
    fn resolve_recursion_overflow_at_depth_100() {
        let mut buf = TypeaheadBuffer::new();
        // map a → b (recursive), map b → a (recursive) — infinite loop.
        let mut km = Keymap::default();
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('a')],
            key_sequence(&[KeyEvent::char('b')]),
            MappingKind::Recursive,
            MappingFlags::default(),
        );
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('b')],
            key_sequence(&[KeyEvent::char('a')]),
            MappingKind::Recursive,
            MappingFlags::default(),
        );

        // Simulate recursive re-feeding until overflow.
        let mut key = KeyEvent::char('a');
        let mut overflow = false;
        for _ in 0..200 {
            let result = buf.resolve_key(key, TypeaheadFlags::recursive_rhs(), &km, Mode::Normal);
            match result {
                ResolveResult::Dispatch(k, flags) => {
                    // REMAPPABLE: continue the recursive chain.
                    if flags.contains(TypeaheadFlags::REMAPPABLE) {
                        key = k;
                    } else {
                        break; // NOREMAP — chain ends
                    }
                }
                ResolveResult::RecursionOverflow => {
                    overflow = true;
                    break;
                }
                ResolveResult::Pending => panic!("unexpected Pending"),
                ResolveResult::ExprMapping { .. } => panic!("unexpected ExprMapping"),
            }
        }
        assert!(overflow, "should hit RecursionOverflow");
        // After overflow, buffer should be cleared.
        assert_eq!(buf.mapping_recursion_depth(), 0);
        assert!(!buf.has_pending());
        assert!(buf.is_empty());
    }

    // ─── resolve_key: empty RHS (follows current the legacy expander behavior) ──

    #[test]
    fn resolve_empty_rhs_dispatches_escape() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('x')],
            key_sequence(&[]), // empty RHS
            MappingKind::NonRecursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        // Empty RHS → degenerate case: dispatch Escape (matches the legacy expander).
        assert_dispatch(&result, KeyEvent::escape(), TypeaheadFlags::NOREMAP);
    }

    // ─── resolve_key: buffer state after inject_mapping_rhs ─────────────

    #[test]
    fn resolve_buffer_state_after_multi_key_rhs() {
        let mut buf = TypeaheadBuffer::new();
        // Pre-existing entries in the buffer.
        buf.inject_back([user_entry('z')]);

        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('a'), KeyEvent::char('b')]),
            MappingKind::NonRecursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&result, KeyEvent::char('a'), TypeaheadFlags::noremap_rhs());

        // Buffer: [b(NOREMAP), z(user)] — RHS remainder injected at front.
        assert_eq!(buf.len(), 2);
        let b_entry = buf.pop_front().unwrap();
        assert_eq!(b_entry.key, KeyEvent::char('b'));
        assert_eq!(b_entry.flags, TypeaheadFlags::noremap_rhs());
        let z_entry = buf.pop_front().unwrap();
        assert_eq!(z_entry.key, KeyEvent::char('z'));
        assert_eq!(z_entry.flags, TypeaheadFlags::user_typed());
    }

    // ─── force_resolve: exact match → expands ───────────────────────────

    #[test]
    fn force_resolve_with_exact_match_expands() {
        let mut buf = TypeaheadBuffer::new();
        // Map both 'j' and 'jk' — 'j' alone produces Prefix{exact: Some}.
        let mut km = Keymap::default();
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );

        // Feed 'j' → Pending (ambiguous prefix).
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);
        assert!(buf.has_pending());

        // Force resolve → should use the exact match 'j' → 'x'.
        let r2 = buf.force_resolve(&km, Mode::Normal);
        let r2 = r2.expect("force_resolve should return Some when pending is non-empty");
        assert_dispatch(&r2, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());
        assert!(!buf.has_pending());
    }

    // ─── force_resolve: no exact match → flushes as NOREMAP ─────────────

    #[test]
    fn force_resolve_no_exact_flushes_as_noremap() {
        let mut buf = TypeaheadBuffer::new();
        // Only 'jk' mapped, no exact match for 'j' alone.
        let km = km_with(&[(
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // Feed 'j' → Pending (prefix match, no exact).
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);

        // Force resolve → no exact match → flush 'j' as NOREMAP literal.
        let r2 = buf.force_resolve(&km, Mode::Normal);
        let r2 = r2.expect("force_resolve should return Some");
        assert_dispatch(&r2, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);
        assert!(!buf.has_pending());
        assert!(buf.is_empty()); // only 1 pending key, nothing else to flush
    }

    // ─── force_resolve: empty pending → None ────────────────────────────

    #[test]
    fn force_resolve_empty_pending_returns_none() {
        let mut buf = TypeaheadBuffer::new();
        let km = Keymap::default();

        let result = buf.force_resolve(&km, Mode::Normal);
        assert!(result.is_none());
    }

    // ─── force_resolve: multi-key pending, no exact match ───────────────

    #[test]
    fn force_resolve_multi_key_pending_no_exact_flushes_all() {
        let mut buf = TypeaheadBuffer::new();
        // Map 'jkl' but not 'jk' — after feeding 'j' and 'k', pending = ['j', 'k'].
        let km = km_with(&[(
            &[
                KeyEvent::char('j'),
                KeyEvent::char('k'),
                KeyEvent::char('l'),
            ],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // Feed 'j' → Pending.
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);

        // Feed 'k' → Pending (prefix of 'jkl').
        let r2 = buf.resolve_key(
            KeyEvent::char('k'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r2);
        assert_eq!(buf.pending_len(), 2);

        // Force resolve → no exact match for 'jk' → flush both as NOREMAP.
        let r3 = buf.force_resolve(&km, Mode::Normal);
        let r3 = r3.expect("force_resolve should return Some");
        assert_dispatch(&r3, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);
        assert!(!buf.has_pending());

        // 'k' should be in buffer as NOREMAP.
        assert_eq!(buf.len(), 1);
        let k_entry = buf.pop_front().unwrap();
        assert_eq!(k_entry.key, KeyEvent::char('k'));
        assert_eq!(k_entry.flags, TypeaheadFlags::NOREMAP);
    }

    // ─── resolve_key: jj mapping (same-key multi-key mapping) ───────────

    #[test]
    fn resolve_same_key_mapping_jj_match() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_insert_with(&[(
            &[KeyEvent::char('j'), KeyEvent::char('j')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // First 'j' → Pending.
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_pending(&r1);

        // Second 'j' → ExactOnly match → Dispatch(Escape, NOREMAP).
        let r2 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_dispatch(&r2, KeyEvent::escape(), TypeaheadFlags::noremap_rhs());
        assert!(!buf.has_pending());
    }

    #[test]
    fn resolve_same_key_mapping_jj_flush() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_insert_with(&[(
            &[KeyEvent::char('j'), KeyEvent::char('j')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // 'j' → Pending.
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_pending(&r1);

        // 'p' → NoMatch for 'jp' → flush: dispatch 'j' (NOREMAP), queue 'p' (REMAPPABLE).
        // First key NOREMAP to prevent re-capture as prefix of 'jj'.
        // Remaining key 'p' is REMAPPABLE — fresh key eligible for its own mappings.
        let r2 = buf.resolve_key(
            KeyEvent::char('p'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_dispatch(&r2, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);
        assert_eq!(buf.len(), 1);
        let p_entry = buf.pop_front().unwrap();
        assert_eq!(p_entry.key, KeyEvent::char('p'));
        assert_eq!(p_entry.flags, TypeaheadFlags::NOREMAP);
    }

    #[test]
    fn resolve_same_key_timeout_flush() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_insert_with(&[(
            &[KeyEvent::char('j'), KeyEvent::char('j')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // 'j' → Pending.
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_pending(&r1);

        // Timeout → force_resolve → no exact match for 'j' → flush as literal.
        let r2 = buf.force_resolve(&km, Mode::Insert);
        let r2 = r2.expect("should resolve");
        assert_dispatch(&r2, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);
        assert!(!buf.has_pending());
        assert!(buf.is_empty());
    }

    /// Regression: timeout-flushed 'j' must not be recaptured as prefix of 'jj'.
    ///
    /// In the new typeahead model, the flushed key returns as Dispatch(j, NOREMAP).
    /// When the engine re-processes it, the NOREMAP flag prevents re-capture.
    #[test]
    fn resolve_timeout_flush_noremap_prevents_recapture() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_insert_with(&[(
            &[KeyEvent::char('j'), KeyEvent::char('j')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // Type 'j' → Pending.
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_pending(&r1);

        // Timeout fires → force_resolve flushes 'j' as NOREMAP literal.
        let r2 = buf.force_resolve(&km, Mode::Insert);
        let r2 = r2.unwrap();
        assert_dispatch(&r2, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);

        // Simulate what the engine does: re-process the dispatched key.
        // The NOREMAP flag means it bypasses mapping lookup entirely.
        let r3 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::NOREMAP, // this is the key from the Dispatch result
            &km,
            Mode::Insert,
        );
        // Should pass through as literal, NOT enter Pending.
        assert_dispatch(&r3, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);
    }

    // ─── resolve_key: recursive chain ───────────────────────────────────

    #[test]
    fn resolve_recursive_chain_two_levels() {
        let mut buf = TypeaheadBuffer::new();
        // map a → b (recursive), map b → c (recursive).
        let mut km = Keymap::default();
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('a')],
            key_sequence(&[KeyEvent::char('b')]),
            MappingKind::Recursive,
            MappingFlags::default(),
        );
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('b')],
            key_sequence(&[KeyEvent::char('c')]),
            MappingKind::Recursive,
            MappingFlags::default(),
        );

        // Feed 'a' → recursive expand to 'b' (REMAPPABLE).
        let r1 = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r1, KeyEvent::char('b'), TypeaheadFlags::recursive_rhs());
        assert_eq!(buf.mapping_recursion_depth(), 1);

        // Feed 'b' with REMAPPABLE (simulating engine re-feeding the dispatched key).
        let r2 = buf.resolve_key(
            KeyEvent::char('b'),
            TypeaheadFlags::recursive_rhs(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r2, KeyEvent::char('c'), TypeaheadFlags::recursive_rhs());
        assert_eq!(buf.mapping_recursion_depth(), 2);
    }

    // ─── resolve_key: noremap blocks recursive chain ────────────────────

    #[test]
    fn resolve_noremap_blocks_recursive_chain() {
        let mut buf = TypeaheadBuffer::new();
        // noremap a → b, map b → c (recursive).
        let mut km = Keymap::default();
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('a')],
            key_sequence(&[KeyEvent::char('b')]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('b')],
            key_sequence(&[KeyEvent::char('c')]),
            MappingKind::Recursive,
            MappingFlags::default(),
        );

        // Feed 'a' → noremap expands to 'b' with NOREMAP.
        let r1 = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r1, KeyEvent::char('b'), TypeaheadFlags::noremap_rhs());

        // Feed 'b' with NOREMAP (from noremap expansion) → should bypass mapping.
        let r2 = buf.resolve_key(
            KeyEvent::char('b'),
            TypeaheadFlags::NOREMAP,
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r2, KeyEvent::char('b'), TypeaheadFlags::NOREMAP);
    }

    // ─── resolve_key: insert mode mapping ───────────────────────────────

    #[test]
    fn resolve_insert_mode_single_key_mapping() {
        let mut buf = TypeaheadBuffer::new();
        let mut km = Keymap::default();
        km.map(
            MappingMode::Insert,
            &[KeyEvent::char('a')],
            key_sequence(&[KeyEvent::char('b'), KeyEvent::char('c')]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );

        let result = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_dispatch(&result, KeyEvent::char('b'), TypeaheadFlags::noremap_rhs());
        assert_eq!(buf.len(), 1);
        let c_entry = buf.pop_front().unwrap();
        assert_eq!(c_entry.key, KeyEvent::char('c'));
        assert_eq!(c_entry.flags, TypeaheadFlags::noremap_rhs());
    }

    // ─── force_resolve: with recursive exact match ──────────────────────

    #[test]
    fn force_resolve_exact_recursive_increments_depth() {
        let mut buf = TypeaheadBuffer::new();
        // Both 'j' and 'jk' mapped, 'j' is recursive.
        let mut km = Keymap::default();
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j')],
            key_sequence(&[KeyEvent::char('x'), KeyEvent::char('y')]),
            MappingKind::Recursive,
            MappingFlags::default(),
        );
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );

        // Feed 'j' → Pending (ambiguous prefix).
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);

        // Force resolve → exact match 'j' → recursive expand to 'xy'.
        let r2 = buf.force_resolve(&km, Mode::Normal);
        let r2 = r2.expect("should resolve");
        assert_dispatch(&r2, KeyEvent::char('x'), TypeaheadFlags::recursive_rhs());
        assert_eq!(buf.mapping_recursion_depth(), 1);

        // 'y' should be in buffer with REMAPPABLE.
        assert_eq!(buf.len(), 1);
        let y_entry = buf.pop_front().unwrap();
        assert_eq!(y_entry.key, KeyEvent::char('y'));
        assert_eq!(y_entry.flags, TypeaheadFlags::recursive_rhs());
    }

    // ─── pending_display ────────────────────────────────────────────────

    #[test]
    fn pending_display_empty() {
        let buf = TypeaheadBuffer::new();
        assert_eq!(buf.pending_display(), "");
    }

    #[test]
    fn pending_display_with_keys() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_insert_with(&[(
            &[KeyEvent::char('j'), KeyEvent::char('j')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
        )]);

        // Feed 'j' → Pending.
        buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_eq!(buf.pending_display(), "j");
    }

    // ─── resolve_key: visual mode mapping ───────────────────────────────

    #[test]
    fn resolve_visual_mode_mapping() {
        let mut buf = TypeaheadBuffer::new();
        let mut km = Keymap::default();
        km.map(
            MappingMode::Visual,
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );

        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Visual(crate::primitives::VisualType::Char),
        );
        assert_dispatch(&result, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());
    }

    // ─── resolve_key: macro key flags are remappable ────────────────────

    #[test]
    fn resolve_macro_key_goes_through_mapping() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )]);

        // Macro keys have REMAPPABLE flag → should go through mapping.
        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::macro_key(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&result, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());
    }

    // ─── resolve_key: single-key RHS noremap resets correctly ───────────

    #[test]
    fn resolve_single_key_noremap_rhs_no_lingering_state() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('a')],
            key_sequence(&[KeyEvent::char('b')]),
            MappingKind::NonRecursive,
        )]);

        // First mapping expansion.
        let r1 = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r1, KeyEvent::char('b'), TypeaheadFlags::noremap_rhs());
        assert!(buf.is_empty()); // single-key RHS, nothing queued
        assert!(!buf.has_pending());

        // Now feed a fresh user-typed key — should go through mapping normally.
        let r2 = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r2, KeyEvent::char('b'), TypeaheadFlags::noremap_rhs());
    }

    // ─── force_resolve: ExactOnly case (defensive) ──────────────────────

    #[test]
    fn force_resolve_exact_only_expands() {
        // Structurally ExactOnly shouldn't happen with pending keys (feed()
        // would have expanded it), but force_resolve handles it defensively.
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('j')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )]);

        // Manually push 'j' into pending (bypassing resolve_key).
        buf.push_pending(KeyEvent::char('j'));

        let result = buf.force_resolve(&km, Mode::Normal);
        let result = result.expect("should resolve");
        assert_dispatch(&result, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());
        assert!(!buf.has_pending());
    }

    // ─── force_resolve: non-mapped mode with pending (defensive) ────────

    #[test]
    fn force_resolve_non_mapped_mode_flushes() {
        let mut buf = TypeaheadBuffer::new();
        // Manually push a pending key (shouldn't happen in practice).
        buf.push_pending(KeyEvent::char('j'));

        let km = Keymap::default();
        let result = buf.force_resolve(&km, Mode::Replace);
        let result = result.expect("should resolve");
        assert_dispatch(&result, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);
        assert!(!buf.has_pending());
    }

    // ─── resolve_key: buffer-local mappings take priority ───────────────

    #[test]
    fn resolve_buffer_local_priority() {
        let mut buf = TypeaheadBuffer::new();
        let mut km = Keymap::default();
        // Global: q → x.
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('q')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
        // Buffer-local: q → y.
        km.map_buffer(
            MappingMode::Normal,
            &[KeyEvent::char('q')],
            key_sequence(&[KeyEvent::char('y')]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );

        let result = buf.resolve_key(
            KeyEvent::char('q'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        // Buffer-local should win.
        assert_dispatch(&result, KeyEvent::char('y'), TypeaheadFlags::noremap_rhs());
    }

    // ═════════════════════════════════════════════════════════════════════
    // <expr> mapping tests
    // ═════════════════════════════════════════════════════════════════════

    /// Build a keymap with an `<expr>` mapping in normal mode.
    fn km_with_expr(from: &[KeyEvent], expression: &str, kind: MappingKind) -> Keymap {
        let mut km = Keymap::default();
        km.map_with_expr(
            MappingMode::Normal,
            from,
            Vec::new(), // expr mappings have empty sequence
            kind,
            MappingFlags {
                expr: true,
                ..MappingFlags::default()
            },
            Some(CompactString::from(expression)),
        );
        km
    }

    /// Assert that a ResolveResult is ExprMapping with the expected fields.
    fn assert_expr_mapping(
        result: &ResolveResult,
        expected_expr: &str,
        expected_kind: MappingKind,
        expected_mode: MappingMode,
    ) {
        match result {
            ResolveResult::ExprMapping {
                expression,
                kind,
                mode,
                ..
            } => {
                assert_eq!(expression.as_str(), expected_expr, "expression mismatch");
                assert_eq!(*kind, expected_kind, "mapping kind mismatch");
                assert_eq!(*mode, expected_mode, "mapping mode mismatch");
            }
            other => panic!(
                "expected ExprMapping({:?}, {:?}, {:?}), got {:?}",
                expected_expr, expected_kind, expected_mode, other,
            ),
        }
    }

    // ─── resolve_key: single-key <expr> mapping ──────────────────────────

    #[test]
    fn resolve_expr_mapping_single_key_noremap() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with_expr(
            &[KeyEvent::char('j')],
            "v:count ? 'j' : 'gj'",
            MappingKind::NonRecursive,
        );

        let result = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_expr_mapping(
            &result,
            "v:count ? 'j' : 'gj'",
            MappingKind::NonRecursive,
            MappingMode::Normal,
        );
        // Pending buffer should be cleared after expr mapping.
        assert!(!buf.has_pending());
        // Main buffer should be empty (no RHS injected — awaiting host).
        assert!(buf.is_empty());
    }

    #[test]
    fn resolve_expr_mapping_single_key_recursive() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with_expr(&[KeyEvent::char('j')], "MyFunc()", MappingKind::Recursive);

        let result = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_expr_mapping(
            &result,
            "MyFunc()",
            MappingKind::Recursive,
            MappingMode::Normal,
        );
        assert!(!buf.has_pending());
        assert!(buf.is_empty());
    }

    // ─── resolve_key: multi-key <expr> mapping ───────────────────────────

    #[test]
    fn resolve_expr_mapping_multi_key() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with_expr(
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            "EvalJK()",
            MappingKind::NonRecursive,
        );

        // Feed 'j' → Pending (prefix of 'jk' expr mapping).
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);
        assert!(buf.has_pending());

        // Feed 'k' → completes 'jk' expr mapping → ExprMapping.
        let r2 = buf.resolve_key(
            KeyEvent::char('k'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_expr_mapping(
            &r2,
            "EvalJK()",
            MappingKind::NonRecursive,
            MappingMode::Normal,
        );
        assert!(!buf.has_pending());
        assert!(buf.is_empty());
    }

    // ─── resolve_key: <expr> mapping does NOT inject RHS ─────────────────

    #[test]
    fn resolve_expr_mapping_does_not_inject_rhs() {
        let mut buf = TypeaheadBuffer::new();
        // Pre-existing entries in buffer.
        buf.inject_back([user_entry('z')]);

        let km = km_with_expr(
            &[KeyEvent::char('j')],
            "MyExpr()",
            MappingKind::NonRecursive,
        );

        let result = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_expr_mapping(
            &result,
            "MyExpr()",
            MappingKind::NonRecursive,
            MappingMode::Normal,
        );
        // Only the pre-existing 'z' should remain — no RHS injected.
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.pop_front().unwrap().key, KeyEvent::char('z'));
    }

    // ─── resolve_key: <expr> with NOREMAP key bypasses mapping ───────────

    #[test]
    fn resolve_expr_mapping_noremap_key_bypasses() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with_expr(
            &[KeyEvent::char('j')],
            "MyExpr()",
            MappingKind::NonRecursive,
        );

        // NOREMAP key should bypass mapping lookup entirely.
        let result = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::NOREMAP,
            &km,
            Mode::Normal,
        );
        assert_dispatch(&result, KeyEvent::char('j'), TypeaheadFlags::NOREMAP);
    }

    // ─── resolve_key: <expr> mapping does NOT increment recursion depth ──

    #[test]
    fn resolve_expr_mapping_does_not_increment_recursion() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with_expr(&[KeyEvent::char('j')], "MyExpr()", MappingKind::Recursive);

        let result = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert!(matches!(result, ResolveResult::ExprMapping { .. }));
        // Expr mappings don't increment recursion depth — the injected keys
        // from the host will go through their own expansion path.
        assert_eq!(buf.mapping_recursion_depth(), 0);
    }

    // ─── force_resolve: <expr> mapping (timeout with exact match) ────────

    #[test]
    fn force_resolve_expr_mapping_with_exact_match() {
        let mut buf = TypeaheadBuffer::new();
        // Both 'j' (expr) and 'jk' mapped — 'j' is ambiguous.
        let mut km = Keymap::default();
        km.map_with_expr(
            MappingMode::Normal,
            &[KeyEvent::char('j')],
            Vec::new(),
            MappingKind::NonRecursive,
            MappingFlags {
                expr: true,
                ..MappingFlags::default()
            },
            Some(CompactString::from("TimeoutExpr()")),
        );
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );

        // Feed 'j' → Pending (ambiguous prefix).
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);

        // Force resolve (timeout) → should select the exact expr mapping.
        let r2 = buf.force_resolve(&km, Mode::Normal);
        let r2 = r2.expect("force_resolve should return Some");
        assert_expr_mapping(
            &r2,
            "TimeoutExpr()",
            MappingKind::NonRecursive,
            MappingMode::Normal,
        );
        assert!(!buf.has_pending());
    }

    // ─── force_resolve: <expr> mapping (ExactOnly in pending) ────────────

    #[test]
    fn force_resolve_expr_mapping_exact_only() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with_expr(
            &[KeyEvent::char('j')],
            "ForceExpr()",
            MappingKind::NonRecursive,
        );

        // Manually push 'j' into pending (bypassing resolve_key).
        buf.push_pending(KeyEvent::char('j'));

        let result = buf.force_resolve(&km, Mode::Normal);
        let result = result.expect("should resolve");
        assert_expr_mapping(
            &result,
            "ForceExpr()",
            MappingKind::NonRecursive,
            MappingMode::Normal,
        );
        assert!(!buf.has_pending());
    }

    // ─── resolve_key: <expr> mapping in insert mode ──────────────────────

    #[test]
    fn resolve_expr_mapping_insert_mode() {
        let mut buf = TypeaheadBuffer::new();
        let mut km = Keymap::default();
        km.map_with_expr(
            MappingMode::Insert,
            &[KeyEvent::char('j')],
            Vec::new(),
            MappingKind::NonRecursive,
            MappingFlags {
                expr: true,
                ..MappingFlags::default()
            },
            Some(CompactString::from("InsertExpr()")),
        );

        let result = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Insert,
        );
        assert_expr_mapping(
            &result,
            "InsertExpr()",
            MappingKind::NonRecursive,
            MappingMode::Insert,
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // SILENT flag propagation tests
    // ═════════════════════════════════════════════════════════════════════

    /// Build a keymap with a single silent normal-mode mapping.
    fn km_silent(from: &[KeyEvent], to: Vec<KeyEvent>, kind: MappingKind) -> Keymap {
        let mut km = Keymap::default();
        km.map(
            MappingMode::Normal,
            from,
            to,
            kind,
            MappingFlags {
                silent: true,
                ..MappingFlags::default()
            },
        );
        km
    }

    #[test]
    fn silent_noremap_sets_silent_flag_on_dispatched_key() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_silent(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        );

        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );

        // Dispatched key should have SILENT flag set.
        match result {
            ResolveResult::Dispatch(k, flags) => {
                assert_eq!(k, KeyEvent::char('x'));
                assert!(
                    flags.contains(TypeaheadFlags::SILENT),
                    "Dispatched key from silent noremap should have SILENT flag, got: {flags:?}"
                );
            }
            other => panic!("expected Dispatch, got {other:?}"),
        }
    }

    #[test]
    fn silent_recursive_map_sets_silent_flag_on_dispatched_key() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_silent(
            &[KeyEvent::char('a')],
            key_sequence(&[KeyEvent::char('b')]),
            MappingKind::Recursive,
        );

        let result = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );

        match result {
            ResolveResult::Dispatch(k, flags) => {
                assert_eq!(k, KeyEvent::char('b'));
                assert!(
                    flags.contains(TypeaheadFlags::SILENT),
                    "Dispatched key from silent recursive map should have SILENT flag"
                );
            }
            other => panic!("expected Dispatch, got {other:?}"),
        }
    }

    #[test]
    fn silent_multi_key_rhs_propagates_silent_to_buffered_keys() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_silent(
            &[KeyEvent::char('s')],
            key_sequence(&[
                KeyEvent::char('a'),
                KeyEvent::char('b'),
                KeyEvent::char('c'),
            ]),
            MappingKind::NonRecursive,
        );

        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );

        // First key dispatched with SILENT
        match result {
            ResolveResult::Dispatch(_, flags) => {
                assert!(flags.contains(TypeaheadFlags::SILENT));
            }
            other => panic!("expected Dispatch, got {other:?}"),
        }

        // Remaining buffered keys should also have SILENT
        let b_entry = buf.pop_front().unwrap();
        assert!(
            b_entry.flags.contains(TypeaheadFlags::SILENT),
            "Buffered key 'b' should have SILENT flag"
        );
        let c_entry = buf.pop_front().unwrap();
        assert!(
            c_entry.flags.contains(TypeaheadFlags::SILENT),
            "Buffered key 'c' should have SILENT flag"
        );
    }

    #[test]
    fn non_silent_mapping_does_not_set_silent_flag() {
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('s')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::NonRecursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('s'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );

        match result {
            ResolveResult::Dispatch(_, flags) => {
                assert!(
                    !flags.contains(TypeaheadFlags::SILENT),
                    "Non-silent mapping should not have SILENT flag"
                );
            }
            other => panic!("expected Dispatch, got {other:?}"),
        }
    }

    #[test]
    fn silent_flag_inherited_from_incoming_flags() {
        // When a key already has the SILENT flag (from an earlier mapping expansion),
        // and it doesn't match any further mapping, the SILENT flag passes through.
        let mut buf = TypeaheadBuffer::new();
        let km = Keymap::default(); // no mappings

        let incoming_flags = TypeaheadFlags::noremap_rhs() | TypeaheadFlags::SILENT;
        let result = buf.resolve_key(KeyEvent::char('j'), incoming_flags, &km, Mode::Normal);

        match result {
            ResolveResult::Dispatch(_, flags) => {
                assert!(
                    flags.contains(TypeaheadFlags::SILENT),
                    "SILENT flag should pass through when no mapping matches"
                );
            }
            other => panic!("expected Dispatch, got {other:?}"),
        }
    }

    #[test]
    fn inject_back_respects_max_typeahead_len() {
        let mut buf = TypeaheadBuffer::new();
        let key = KeyEvent::char('a');
        let flags = TypeaheadFlags::user_typed();

        // Fill to near the cap, leaving room for exactly 50 more.
        let fill = MAX_TYPEAHEAD_LEN - 50;
        let entries: Vec<TypeaheadEntry> =
            (0..fill).map(|_| TypeaheadEntry::new(key, flags)).collect();
        buf.inject_back(entries);
        assert_eq!(buf.len(), fill);

        // Inject 100 — only 50 should be accepted (the remaining capacity).
        let extra: Vec<TypeaheadEntry> =
            (0..100).map(|_| TypeaheadEntry::new(key, flags)).collect();
        // Use catch_unwind because debug_assert!(false) fires in debug builds.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            buf.inject_back(extra);
        }));
        assert_eq!(buf.len(), MAX_TYPEAHEAD_LEN);
    }

    #[test]
    fn inject_front_respects_max_typeahead_len() {
        let mut buf = TypeaheadBuffer::new();
        let key = KeyEvent::char('b');
        let flags = TypeaheadFlags::user_typed();

        // Fill to near the cap, leaving room for exactly 30 more.
        let fill = MAX_TYPEAHEAD_LEN - 30;
        let entries: Vec<TypeaheadEntry> =
            (0..fill).map(|_| TypeaheadEntry::new(key, flags)).collect();
        buf.inject_back(entries);
        assert_eq!(buf.len(), fill);

        // inject_front 50 — only 30 should be accepted.
        let extra: Vec<TypeaheadEntry> = (0..50).map(|_| TypeaheadEntry::new(key, flags)).collect();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            buf.inject_front(extra);
        }));
        assert_eq!(buf.len(), MAX_TYPEAHEAD_LEN);
    }

    // ═════════════════════════════════════════════════════════════════════
    // REMAP_SKIP: self-referencing recursive mapping tests
    // ═════════════════════════════════════════════════════════════════════

    // ─── common_prefix_len ──────────────────────────────────────────────

    #[test]
    fn common_prefix_len_identical() {
        let a = key_sequence(&[KeyEvent::char('x')]);
        assert_eq!(TypeaheadBuffer::common_prefix_len(&a, &a), 1);
    }

    #[test]
    fn common_prefix_len_no_overlap() {
        let a = key_sequence(&[KeyEvent::char('x')]);
        let b = key_sequence(&[KeyEvent::char('y')]);
        assert_eq!(TypeaheadBuffer::common_prefix_len(&a, &b), 0);
    }

    #[test]
    fn common_prefix_len_partial_overlap() {
        let rhs = key_sequence(&[
            KeyEvent::char('a'),
            KeyEvent::char('b'),
            KeyEvent::char('c'),
        ]);
        let lhs = key_sequence(&[KeyEvent::char('a'), KeyEvent::char('b')]);
        assert_eq!(TypeaheadBuffer::common_prefix_len(&rhs, &lhs), 2);
    }

    #[test]
    fn common_prefix_len_empty() {
        let a: Vec<KeyEvent> = vec![];
        let b = key_sequence(&[KeyEvent::char('x')]);
        assert_eq!(TypeaheadBuffer::common_prefix_len(&a, &b), 0);
        assert_eq!(TypeaheadBuffer::common_prefix_len(&b, &a), 0);
    }

    // ─── REMAP_SKIP: single-key self-ref `:map x xfoo` ─────────────────

    #[test]
    fn remap_skip_single_key_self_ref_first_key_noremap() {
        // `:map x xfoo` — RHS starts with 'x' (the LHS).
        // The first key 'x' should be NOREMAP; 'f','o','o' should be REMAPPABLE.
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('x')],
            key_sequence(&[
                KeyEvent::char('x'),
                KeyEvent::char('f'),
                KeyEvent::char('o'),
                KeyEvent::char('o'),
            ]),
            MappingKind::Recursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );

        // First RHS key 'x' dispatched with NOREMAP (REMAP_SKIP).
        assert_dispatch(&result, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());
        assert_eq!(buf.mapping_recursion_depth(), 1);

        // Remaining keys: 'f','o','o' should all be REMAPPABLE.
        assert_eq!(buf.len(), 3);
        let f_entry = buf.pop_front().unwrap();
        assert_eq!(f_entry.key, KeyEvent::char('f'));
        assert_eq!(f_entry.flags, TypeaheadFlags::recursive_rhs());
        let o1_entry = buf.pop_front().unwrap();
        assert_eq!(o1_entry.key, KeyEvent::char('o'));
        assert_eq!(o1_entry.flags, TypeaheadFlags::recursive_rhs());
        let o2_entry = buf.pop_front().unwrap();
        assert_eq!(o2_entry.key, KeyEvent::char('o'));
        assert_eq!(o2_entry.flags, TypeaheadFlags::recursive_rhs());
    }

    // ─── REMAP_SKIP: no self-ref (different prefix) ─────────────────────

    #[test]
    fn remap_skip_no_overlap_all_remappable() {
        // `:map x yfoo` — RHS does NOT start with LHS, no skip needed.
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('x')],
            key_sequence(&[
                KeyEvent::char('y'),
                KeyEvent::char('f'),
                KeyEvent::char('o'),
            ]),
            MappingKind::Recursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );

        // All keys should be REMAPPABLE (no REMAP_SKIP triggered).
        assert_dispatch(
            &result,
            KeyEvent::char('y'),
            TypeaheadFlags::recursive_rhs(),
        );
        assert_eq!(buf.len(), 2);
        let f_entry = buf.pop_front().unwrap();
        assert_eq!(f_entry.flags, TypeaheadFlags::recursive_rhs());
        let o_entry = buf.pop_front().unwrap();
        assert_eq!(o_entry.flags, TypeaheadFlags::recursive_rhs());
    }

    // ─── REMAP_SKIP: multi-key LHS overlap ──────────────────────────────

    #[test]
    fn remap_skip_multi_key_lhs_partial_overlap() {
        // `:map ab abcd` — RHS starts with 'ab' (the full LHS).
        // Keys 'a','b' should be NOREMAP; 'c','d' should be REMAPPABLE.
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('a'), KeyEvent::char('b')],
            key_sequence(&[
                KeyEvent::char('a'),
                KeyEvent::char('b'),
                KeyEvent::char('c'),
                KeyEvent::char('d'),
            ]),
            MappingKind::Recursive,
        )]);

        // Feed 'a' → Pending.
        let r1 = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);

        // Feed 'b' → completes 'ab' → expand with REMAP_SKIP.
        let r2 = buf.resolve_key(
            KeyEvent::char('b'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );

        // First RHS key 'a' dispatched with NOREMAP (REMAP_SKIP).
        assert_dispatch(&r2, KeyEvent::char('a'), TypeaheadFlags::noremap_rhs());

        // Remaining: 'b' (NOREMAP), 'c' (REMAPPABLE), 'd' (REMAPPABLE).
        assert_eq!(buf.len(), 3);
        let b_entry = buf.pop_front().unwrap();
        assert_eq!(b_entry.key, KeyEvent::char('b'));
        assert_eq!(b_entry.flags, TypeaheadFlags::noremap_rhs());
        let c_entry = buf.pop_front().unwrap();
        assert_eq!(c_entry.key, KeyEvent::char('c'));
        assert_eq!(c_entry.flags, TypeaheadFlags::recursive_rhs());
        let d_entry = buf.pop_front().unwrap();
        assert_eq!(d_entry.key, KeyEvent::char('d'));
        assert_eq!(d_entry.flags, TypeaheadFlags::recursive_rhs());
    }

    // ─── REMAP_SKIP: noremap is unaffected ──────────────────────────────

    #[test]
    fn remap_skip_noremap_unaffected() {
        // `:noremap x xfoo` — NonRecursive: ALL keys are NOREMAP regardless.
        // REMAP_SKIP only applies to recursive mappings.
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('x')],
            key_sequence(&[
                KeyEvent::char('x'),
                KeyEvent::char('f'),
                KeyEvent::char('o'),
            ]),
            MappingKind::NonRecursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&result, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());
        // All remaining are NOREMAP too (standard noremap behavior).
        let f_entry = buf.pop_front().unwrap();
        assert_eq!(f_entry.flags, TypeaheadFlags::noremap_rhs());
        let o_entry = buf.pop_front().unwrap();
        assert_eq!(o_entry.flags, TypeaheadFlags::noremap_rhs());
    }

    // ─── REMAP_SKIP: entire RHS is LHS (full overlap) ───────────────────

    #[test]
    fn remap_skip_full_overlap_all_noremap() {
        // `:map ab ab` — RHS equals LHS. Both keys NOREMAP.
        // Without REMAP_SKIP this would infinitely recurse.
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('a'), KeyEvent::char('b')],
            key_sequence(&[KeyEvent::char('a'), KeyEvent::char('b')]),
            MappingKind::Recursive,
        )]);

        // Feed 'a' → Pending.
        let r1 = buf.resolve_key(
            KeyEvent::char('a'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);

        // Feed 'b' → completes 'ab' → expand with full REMAP_SKIP.
        let r2 = buf.resolve_key(
            KeyEvent::char('b'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        // Both keys should be NOREMAP.
        assert_dispatch(&r2, KeyEvent::char('a'), TypeaheadFlags::noremap_rhs());
        let b_entry = buf.pop_front().unwrap();
        assert_eq!(b_entry.key, KeyEvent::char('b'));
        assert_eq!(b_entry.flags, TypeaheadFlags::noremap_rhs());
    }

    // ─── REMAP_SKIP: does not recurse infinitely ────────────────────────

    #[test]
    fn remap_skip_prevents_infinite_recursion() {
        // `:map x xfoo` — simulate full expansion chain.
        // 'x' should be dispatched as NOREMAP (no re-trigger).
        // 'f','o','o' should be REMAPPABLE.
        // Re-feeding the dispatched 'x' with NOREMAP should pass through.
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('x')],
            key_sequence(&[
                KeyEvent::char('x'),
                KeyEvent::char('f'),
                KeyEvent::char('o'),
                KeyEvent::char('o'),
            ]),
            MappingKind::Recursive,
        )]);

        // Initial key press.
        let r1 = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&r1, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());

        // Re-feed the dispatched 'x' with NOREMAP — should bypass mapping.
        let r2 = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::noremap_rhs(),
            &km,
            Mode::Normal,
        );
        // NOREMAP key passes through as literal.
        assert_dispatch(&r2, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());
        // No recursion overflow should have occurred.
        assert_eq!(buf.mapping_recursion_depth(), 1);
    }

    // ─── REMAP_SKIP: with SILENT flag ───────────────────────────────────

    #[test]
    fn remap_skip_preserves_silent_flag() {
        // `:map <silent> x xfoo` — REMAP_SKIP keys should still carry SILENT.
        let mut buf = TypeaheadBuffer::new();
        let km = km_silent(
            &[KeyEvent::char('x')],
            key_sequence(&[
                KeyEvent::char('x'),
                KeyEvent::char('f'),
                KeyEvent::char('o'),
            ]),
            MappingKind::Recursive,
        );

        let result = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );

        // First key 'x' should have NOREMAP + SILENT.
        match result {
            ResolveResult::Dispatch(k, flags) => {
                assert_eq!(k, KeyEvent::char('x'));
                assert!(flags.contains(TypeaheadFlags::NOREMAP));
                assert!(flags.contains(TypeaheadFlags::SILENT));
                assert!(!flags.contains(TypeaheadFlags::REMAPPABLE));
            }
            other => panic!("expected Dispatch, got {other:?}"),
        }

        // Remaining 'f','o' should have REMAPPABLE + SILENT.
        let f_entry = buf.pop_front().unwrap();
        assert_eq!(f_entry.key, KeyEvent::char('f'));
        assert!(f_entry.flags.contains(TypeaheadFlags::REMAPPABLE));
        assert!(f_entry.flags.contains(TypeaheadFlags::SILENT));
        assert!(!f_entry.flags.contains(TypeaheadFlags::NOREMAP));

        let o_entry = buf.pop_front().unwrap();
        assert_eq!(o_entry.key, KeyEvent::char('o'));
        assert!(o_entry.flags.contains(TypeaheadFlags::REMAPPABLE));
        assert!(o_entry.flags.contains(TypeaheadFlags::SILENT));
    }

    // ─── REMAP_SKIP: force_resolve path ─────────────────────────────────

    #[test]
    fn remap_skip_force_resolve_self_ref() {
        // Both 'j' and 'jk' mapped, 'j' is a self-ref recursive mapping.
        // `:map j jx` + `:map jk <Esc>`
        // Timeout on 'j' → force_resolve → expand 'j' → 'jx' with REMAP_SKIP.
        let mut buf = TypeaheadBuffer::new();
        let mut km = Keymap::default();
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j')],
            key_sequence(&[KeyEvent::char('j'), KeyEvent::char('x')]),
            MappingKind::Recursive,
            MappingFlags::default(),
        );
        km.map(
            MappingMode::Normal,
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            key_sequence(&[KeyEvent::escape()]),
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );

        // Feed 'j' → Pending (ambiguous: could be 'j' or 'jk').
        let r1 = buf.resolve_key(
            KeyEvent::char('j'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_pending(&r1);

        // Timeout → force_resolve → exact match 'j' → expand 'jx'.
        let r2 = buf.force_resolve(&km, Mode::Normal);
        let r2 = r2.expect("force_resolve should return Some");

        // First key 'j' dispatched with NOREMAP (REMAP_SKIP).
        assert_dispatch(&r2, KeyEvent::char('j'), TypeaheadFlags::noremap_rhs());
        assert_eq!(buf.mapping_recursion_depth(), 1);

        // Remaining 'x' should be REMAPPABLE.
        assert_eq!(buf.len(), 1);
        let x_entry = buf.pop_front().unwrap();
        assert_eq!(x_entry.key, KeyEvent::char('x'));
        assert_eq!(x_entry.flags, TypeaheadFlags::recursive_rhs());
    }

    // ─── REMAP_SKIP: single-key RHS equals LHS ─────────────────────────

    #[test]
    fn remap_skip_single_key_rhs_equals_lhs() {
        // `:map x x` — degenerate: RHS is exactly the LHS.
        // The single key should be NOREMAP.
        let mut buf = TypeaheadBuffer::new();
        let km = km_with(&[(
            &[KeyEvent::char('x')],
            key_sequence(&[KeyEvent::char('x')]),
            MappingKind::Recursive,
        )]);

        let result = buf.resolve_key(
            KeyEvent::char('x'),
            TypeaheadFlags::user_typed(),
            &km,
            Mode::Normal,
        );
        assert_dispatch(&result, KeyEvent::char('x'), TypeaheadFlags::noremap_rhs());
        assert!(buf.is_empty()); // single-key RHS, nothing queued
    }
}
