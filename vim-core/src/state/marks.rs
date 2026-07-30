//! Vim marks storage.
//!
//! # Layering
//!
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`. State modules are pure data containers with
//! no execution logic.
//!
//! Implements all Vim mark types.
//!
//! # Mark Types
//!
//! | Mark | Scope | Description |
//! |------|-------|-------------|
//! | `a-z` | Local | Per-buffer marks |
//! | `A-Z` | Global | Cross-buffer (shell manages file) |
//! | `'` / `` ` `` | Auto | Position before last jump |
//! | `.` | Auto | Position of last change |
//! | `^` | Auto | Where insert mode stopped |
//! | `[` / `]` | Auto | Start/end of last change |
//! | `<` / `>` | Auto | Start/end of last visual |

use crate::primitives::byte_delta;
use crate::primitives::{BufferId, Mark, MarkName, Offset};
use ahash::AHashMap;
use compact_str::CompactString;

/// Opaque serialized mark data for host persistence.
///
/// Wraps a `Vec<u8>` containing JSON-encoded mark state. Hosts save this
/// blob to disk/database and restore it on next session via
/// [`VimEngine::import_marks`](crate::execution::VimEngine::import_marks) /
/// [`VimEngine::import_global_marks`](crate::execution::VimEngine::import_global_marks).
///
/// The format is intentionally opaque — hosts should not parse or depend
/// on the internal structure.
#[cfg(feature = "serde")]
#[derive(Debug, Clone)]
pub struct SerializedMarks(pub(crate) Vec<u8>);

#[cfg(feature = "serde")]
impl SerializedMarks {
    /// Access the raw bytes (for persistence by the host).
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Construct from raw bytes (loaded from host persistence).
    #[inline]
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Returns `true` if the serialized data is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Per-buffer mark state extracted from the shared [`Marks`] struct.
///
/// Contains local named marks (a-z) and all special marks (. ^ [ ] < > ' ` ").
/// Global named marks (A-Z) remain in the shared `Marks` struct and are never
/// included here — they are session-global by Vim semantics.
#[derive(Debug, Clone, Default)]
pub struct BufferMarks {
    /// Local named marks: a-z. Keyed by `MarkName` where `is_local() == true`.
    pub(crate) local: AHashMap<MarkName, Mark>,
    /// Special marks: . ^ [ ] < > ' ` ". Keyed by `MarkName` where `is_special() == true`.
    pub(crate) special: AHashMap<MarkName, Mark>,
}

/// A global mark (A-Z) with its buffer association.
///
/// Global marks are cross-buffer: jumping to a global mark may require the host
/// to switch to a different buffer first. This struct pairs the position
/// ([`Mark`]) with the [`BufferId`] of the buffer the mark belongs to.
///
/// The `Mark` primitive remains `Copy` (24 bytes); the buffer association lives
/// here at the container level, not on the `Mark` itself.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlobalMarkEntry {
    /// The saved position within the buffer.
    pub mark: Mark,
    /// The buffer this mark belongs to.
    pub buffer_id: BufferId,
    /// Optional file path for marks in unloaded buffers.
    ///
    /// When set, hosts can use this to open the correct file when jumping to a
    /// global mark whose buffer is not currently loaded.
    #[cfg_attr(feature = "serde", serde(default))]
    pub path: Option<CompactString>,
}

impl GlobalMarkEntry {
    /// Attach a file path to this entry so hosts can open unloaded buffers.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<CompactString>) -> Self {
        self.path = Some(path.into());
        self
    }
}

/// All marks for the current buffer.
///
/// See the module docs for the table of mark names and their scopes.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Marks {
    /// Local named marks (a-z): fixed array indexed by `(char - b'a')`.
    /// Exactly 26 possible keys, so a hash map is unnecessary overhead.
    local_az: [Option<Mark>; 26],
    /// Legacy global marks (A-Z) written through the [`set`] path.
    ///
    /// After the global-mark split, new global marks written via
    /// [`set_with_buffer_id`] go into [`globals`]. This map retains A-Z
    /// entries written through the legacy [`set`] path for backward compat.
    named_globals: AHashMap<MarkName, Mark>,
    /// Special/automatic marks (' ` . ^ [ ] < >).
    special: AHashMap<MarkName, Mark>,
    /// Global marks (A-Z) with buffer association.
    ///
    /// Preferred storage for cross-buffer marks. [`get`] checks this map
    /// first for global mark names, falling back to [`named_globals`] for legacy
    /// entries.
    #[cfg_attr(feature = "serde", serde(default))]
    globals: AHashMap<MarkName, GlobalMarkEntry>,
    /// Monotonically increasing version counter, bumped on every mutation.
    ///
    /// Used by [`crate::state::diff::StateSnapshot`] to detect mark changes
    /// without requiring `PartialEq` on the full mark set.
    #[cfg_attr(feature = "serde", serde(default))]
    version: u64,
}

impl Default for Marks {
    fn default() -> Self {
        let mut special = AHashMap::default();
        // Neovim initializes the `.` (last change) mark to offset 0 for new
        // buffers. Match this so that commands that don't modify text still
        // report the same `.` mark position as Neovim.
        special.insert(
            MarkName::LAST_CHANGE,
            Mark::new(crate::primitives::Offset::new(0)),
        );
        Self {
            local_az: [None; 26],
            named_globals: AHashMap::default(),
            special,
            globals: AHashMap::default(),
            version: 0,
        }
    }
}

/// Configuration for the consolidated [`Marks::remap_named`] method.
///
/// Controls which marks are adjusted and how the same-line skip behaves.
#[derive(Clone, Copy)]
struct RemapConfig {
    /// Edit position (byte offset where the edit occurred).
    pos: usize,
    /// Bytes deleted at `pos`.
    old_len: usize,
    /// Bytes inserted at `pos`.
    new_len: usize,
    /// When `Some(threshold)`, marks with offset `<= threshold` are skipped
    /// (cross-line semantics: only marks past the edit line are shifted).
    /// When `None`, all marks at/after `pos` are adjusted.
    skip_threshold: Option<usize>,
    /// When `Some(bid)`, only adjusts buffer-associated global marks in the
    /// `globals` map matching that buffer id (skipping local and legacy globals).
    /// When `None`, adjusts local marks (a-z) and legacy globals (A-Z).
    buffer_filter: Option<BufferId>,
}

impl Marks {
    /// Create empty marks (with Neovim-compatible initial `.` mark at offset 0).
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Monotonically increasing version counter.
    ///
    /// Bumped on every mutation (set, delete, adjust). Used by state diffing
    /// to detect changes without comparing all mark positions.
    #[inline]
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Get a mark by name.
    ///
    /// For global marks (A-Z), checks the `globals` map first (with buffer
    /// association), falling back to `named_globals` for legacy entries set via
    /// [`Self::set`].
    ///
    /// # Complexity
    ///
    /// Time: O(1) amortized — dispatches on mark type, then performs O(1)
    /// amortized hash map lookups (at most two for global marks).
    ///
    /// Space: O(1)
    #[must_use]
    pub fn get(&self, name: MarkName) -> Option<Mark> {
        if name.is_global() {
            // Prefer globals map (has buffer association), fall back to named_globals.
            self.globals
                .get(&name)
                .map(|e| e.mark)
                .or_else(|| self.named_globals.get(&name).copied())
        } else if name.is_local() {
            let idx = (name.char() as u8 - b'a') as usize;
            self.local_az.get(idx).copied().flatten()
        } else if name.is_special() {
            self.special.get(&name).copied()
        } else {
            // Numbered marks (0-9) — not stored locally (shell/shada manages)
            None
        }
    }

    /// Get a global mark entry with its buffer association.
    ///
    /// Returns `None` if `name` is not a global mark (A-Z) or if no entry
    /// exists in the `globals` map. Legacy entries in `named_globals` (set via
    /// [`Self::set`] without a buffer id) are not returned here.
    #[must_use]
    pub fn get_global(&self, name: MarkName) -> Option<&GlobalMarkEntry> {
        if name.is_global() {
            self.globals.get(&name)
        } else {
            None
        }
    }

    /// Set a mark.
    ///
    /// # Complexity
    ///
    /// Time: O(1) amortized — hash map insert.
    ///
    /// Space: O(1) amortized
    pub fn set(&mut self, name: MarkName, mark: Mark) {
        self.version += 1;
        if name.is_local() {
            let idx = (name.char() as u8 - b'a') as usize;
            if let Some(slot) = self.local_az.get_mut(idx) {
                *slot = Some(mark);
            }
        } else if name.is_global() {
            self.named_globals.insert(name, mark);
        } else if name.is_special() {
            self.special.insert(name, mark);
        }
        // Numbered marks silently ignored (shell manages)
    }

    /// Set a mark with an optional buffer association.
    ///
    /// For global marks (A-Z), if `buffer_id` is `Some`, the mark is stored
    /// in the `globals` map with its buffer association (and any legacy
    /// entry in `named_globals` is cleaned up). If `buffer_id` is `None`, the mark
    /// falls back to the legacy `named_globals` map.
    ///
    /// For local and special marks, `buffer_id` is ignored and this behaves
    /// identically to [`Self::set`].
    ///
    /// # Complexity
    ///
    /// Time: O(1) amortized — hash map insert/remove.
    ///
    /// Space: O(1) amortized
    pub fn set_with_buffer_id(&mut self, name: MarkName, mark: Mark, buffer_id: Option<BufferId>) {
        self.version += 1;
        if name.is_global() {
            if let Some(bid) = buffer_id {
                self.globals.insert(
                    name,
                    GlobalMarkEntry {
                        mark,
                        buffer_id: bid,
                        path: None,
                    },
                );
                // Clean up legacy entry if it exists.
                self.named_globals.remove(&name);
            } else {
                self.named_globals.insert(name, mark);
            }
        } else if name.is_local() {
            let idx = (name.char() as u8 - b'a') as usize;
            if let Some(slot) = self.local_az.get_mut(idx) {
                *slot = Some(mark);
            }
        } else if name.is_special() {
            self.special.insert(name, mark);
        }
        // Numbered marks silently ignored (shell manages)
    }

    /// Delete a mark.
    ///
    /// For global marks (A-Z), removes from both `globals` and `named_globals`
    /// (returns the first found).
    ///
    /// # Complexity
    ///
    /// Time: O(1) amortized — hash map remove (at most two lookups for
    /// global marks).
    ///
    /// Space: O(1)
    pub fn delete(&mut self, name: MarkName) -> Option<Mark> {
        let removed = if name.is_global() {
            // Remove from both maps; prefer globals entry if both exist.
            let from_globals = self.globals.remove(&name).map(|e| e.mark);
            let from_named = self.named_globals.remove(&name);
            from_globals.or(from_named)
        } else if name.is_local() {
            let idx = (name.char() as u8 - b'a') as usize;
            self.local_az.get_mut(idx).and_then(Option::take)
        } else if name.is_special() {
            self.special.remove(&name)
        } else {
            None
        };
        if removed.is_some() {
            self.version += 1;
        }
        removed
    }

    /// Set the previous position mark (for jump commands).
    ///
    /// Updates both `'` and `` ` `` marks.
    pub fn set_previous_position(&mut self, offset: Offset) {
        self.version += 1;
        let mark = Mark::new(offset);
        self.special.insert(MarkName::PREV_JUMP, mark);
        self.special.insert(MarkName::PREV_JUMP_EXACT, mark);
    }

    /// Set the last change position.
    pub fn set_last_change(&mut self, offset: Offset) {
        self.version += 1;
        self.special
            .insert(MarkName::LAST_CHANGE, Mark::new(offset));
    }

    /// Set the insert stop position.
    pub fn set_insert_stop(&mut self, offset: Offset) {
        self.version += 1;
        self.special
            .insert(MarkName::INSERT_STOP, Mark::new(offset));
    }

    /// Clear the insert stop mark (^) when the line containing it is deleted.
    pub fn clear_insert_stop(&mut self) {
        if self.special.remove(&MarkName::INSERT_STOP).is_some() {
            self.version += 1;
        }
    }

    /// Set the change/yank region marks.
    pub fn set_change_region(&mut self, start: Offset, end: Offset) {
        self.version += 1;
        self.special
            .insert(MarkName::CHANGE_START, Mark::new(start));
        self.special.insert(MarkName::CHANGE_END, Mark::new(end));
    }

    /// Set the visual region marks.
    pub fn set_visual_region(&mut self, start: Offset, end: Offset) {
        self.version += 1;
        self.special
            .insert(MarkName::VISUAL_START, Mark::new(start));
        self.special.insert(MarkName::VISUAL_END, Mark::new(end));
    }

    /// Adjust the insert-stop mark (`^`) for a specific edit using simple
    /// byte-offset arithmetic (shift/clamp).
    ///
    /// This is a lower-level helper; the effect processor now uses
    /// `adjust_insert_stop_line_col` which preserves the (line, col) model.
    /// Retained for unit tests and potential fallback use.
    #[cfg(test)]
    pub fn adjust_insert_stop(&mut self, pos: usize, old_len: usize, new_len: usize) {
        let delta = byte_delta::delta(new_len, old_len);
        if delta == 0 && old_len == 0 {
            return;
        }
        if let Some(mark) = self.special.get_mut(&MarkName::INSERT_STOP) {
            let val = mark.offset().get();
            let adjusted = super::offset_adjust::adjust_offset(val, pos, old_len, delta);
            if adjusted != val {
                self.version += 1;
                *mark = Mark::with_topline_offset(Offset::new(adjusted), mark.topline_offset());
            }
        }
    }

    /// Adjust named mark offsets for edits that cross line boundaries.
    ///
    /// In Neovim, named marks store (line, col). Intra-line edits don't
    /// change the mark position, but edits that add/remove lines shift
    /// the mark's line number — which in our byte-offset model means
    /// adjusting the byte offset.
    ///
    /// The caller determines `crosses_line` by checking whether the
    /// deleted/inserted text contains a newline.
    ///
    /// # Complexity
    ///
    /// Time: O(M) where M = number of named marks stored (up to 52 for
    /// a-z + A-Z). Short-circuits to O(1) when `crosses_line` is false.
    ///
    /// Space: O(1)
    pub fn adjust_named_offsets(
        &mut self,
        pos: usize,
        old_len: usize,
        new_len: usize,
        edit_line_end: usize,
    ) {
        self.adjust_named_offsets_ext(pos, old_len, new_len, edit_line_end, true);
    }

    /// Like [`Self::adjust_named_offsets`] but with an explicit `skip_same_line`
    /// flag. When `skip_same_line` is false, ALL marks at or after `pos`
    /// are adjusted (the `edit_line_end` skip is suppressed). This is
    /// needed for pure cross-line inserts (e.g. linewise paste) where
    /// Neovim shifts ALL marks at and below the insertion line.
    pub fn adjust_named_offsets_ext(
        &mut self,
        pos: usize,
        old_len: usize,
        new_len: usize,
        edit_line_end: usize,
        skip_same_line: bool,
    ) {
        self.remap_named(RemapConfig {
            pos,
            old_len,
            new_len,
            skip_threshold: if skip_same_line {
                Some(edit_line_end)
            } else {
                None
            },
            buffer_filter: None,
        });
    }

    /// Adjust named marks on the same line as an edit using a `ChangeSet`.
    ///
    /// When an edit occurs on the same line as a named mark (a-z, A-Z) and
    /// the mark's column is at/after the edit start, the mark is shifted
    /// Remove named marks whose offset falls within a deleted byte range.
    ///
    /// In Neovim, when the line containing a mark is deleted, the mark
    /// becomes invalid (E20: Mark not set). Since we store byte offsets,
    /// we invalidate any named mark whose offset is in `[start, end)`.
    ///
    /// Also invalidates global marks in the `globals` map whose offset
    /// falls in the range.
    ///
    /// # Complexity
    ///
    /// Time: O(M + G) where M = number of named marks and G = number of
    /// global mark entries. Each mark's offset is compared against the range
    /// bounds.
    ///
    /// Space: O(1)
    pub fn invalidate_named_in_range(&mut self, start: usize, end: usize) {
        let mut changed = false;
        // Invalidate local marks (a-z) in the fixed array
        for slot in &mut self.local_az {
            if let Some(mark) = slot {
                let val = mark.offset().get();
                if val >= start && val < end {
                    *slot = None;
                    changed = true;
                }
            }
        }
        // Invalidate legacy global marks (A-Z) in the hash map
        let before_globals_legacy = self.named_globals.len();
        self.named_globals.retain(|_, mark| {
            let val = mark.offset().get();
            val < start || val >= end
        });
        if self.named_globals.len() != before_globals_legacy {
            changed = true;
        }
        // Invalidate global marks with buffer association
        let before_globals = self.globals.len();
        self.globals.retain(|_, entry| {
            let val = entry.mark.offset().get();
            val < start || val >= end
        });
        if self.globals.len() != before_globals {
            changed = true;
        }
        if changed {
            self.version += 1;
        }
    }

    /// Adjust global mark offsets for edits in a specific buffer.
    ///
    /// Only adjusts global marks whose [`buffer_id`](GlobalMarkEntry::buffer_id)
    /// matches `current_buffer`. Uses the same cross-line semantics as
    /// [`Self::adjust_named_offsets`]: marks on the same line as the edit (at or
    /// before `edit_line_end`) are left alone.
    ///
    /// # Complexity
    ///
    /// Time: O(G) where G = number of global mark entries. Each entry is
    /// checked for buffer id match and offset comparison.
    ///
    /// Space: O(1)
    pub fn adjust_global_offsets(
        &mut self,
        current_buffer: BufferId,
        pos: usize,
        old_len: usize,
        new_len: usize,
        edit_line_end: usize,
    ) {
        self.remap_named(RemapConfig {
            pos,
            old_len,
            new_len,
            skip_threshold: Some(edit_line_end),
            buffer_filter: Some(current_buffer),
        });
    }

    /// Consolidated named-mark adjustment: the single implementation behind
    /// [`adjust_named_offsets`], [`adjust_named_offsets_ext`], and
    /// [`adjust_global_offsets`].
    ///
    /// # Config
    ///
    /// - `skip_threshold`: when `Some(t)`, marks with offset `<= t` are
    ///   skipped (cross-line semantics). When `None`, all marks at/after
    ///   `pos` are adjusted.
    /// - `buffer_filter`: when `Some(bid)`, only adjusts buffer-associated
    ///   global marks in the [`globals`] map matching that buffer id, and
    ///   skips local marks and legacy globals. When `None`, adjusts local
    ///   marks (a-z) and legacy globals (A-Z in `named_globals`).
    fn remap_named(&mut self, config: RemapConfig) {
        let delta = byte_delta::delta(config.new_len, config.old_len);
        if delta == 0 && config.old_len == 0 {
            return;
        }

        if config.buffer_filter.is_some() {
            // Global marks with buffer association only.
            let mut changed = false;
            for entry in self.globals.values_mut() {
                if Some(entry.buffer_id) != config.buffer_filter {
                    continue;
                }
                let val = entry.mark.offset().get();
                if let Some(threshold) = config.skip_threshold {
                    if val <= threshold {
                        continue;
                    }
                }
                let adjusted = super::offset_adjust::adjust_offset_named(
                    val,
                    config.pos,
                    config.old_len,
                    delta,
                );
                if adjusted != val {
                    // topline_offset is relative (line count) — stable across edits.
                    entry.mark = Mark::with_topline_offset(
                        Offset::new(adjusted),
                        entry.mark.topline_offset(),
                    );
                    changed = true;
                }
            }
            if changed {
                self.version += 1;
            }
        } else {
            // Local marks (a-z) and legacy global marks (A-Z).
            self.version += 1;
            Self::adjust_marks_array(
                &mut self.local_az,
                config.pos,
                config.old_len,
                delta,
                config.skip_threshold,
            );
            Self::adjust_marks_map(
                &mut self.named_globals,
                config.pos,
                config.old_len,
                delta,
                config.skip_threshold,
            );
        }
    }

    /// Adjust marks in the fixed local array, optionally skipping those
    /// at or below `skip_threshold`.
    fn adjust_marks_array(
        array: &mut [Option<Mark>; 26],
        pos: usize,
        old_len: usize,
        delta: isize,
        skip_threshold: Option<usize>,
    ) {
        for mark in array.iter_mut().flatten() {
            let val = mark.offset().get();
            if let Some(threshold) = skip_threshold {
                if val <= threshold {
                    continue;
                }
            }
            let adjusted = super::offset_adjust::adjust_offset_named(val, pos, old_len, delta);
            if adjusted != val {
                // topline_offset is relative (line count) — stable across edits.
                *mark = Mark::with_topline_offset(Offset::new(adjusted), mark.topline_offset());
            }
        }
    }

    /// Adjust marks in a hash map, optionally skipping those at or below
    /// `skip_threshold`.
    fn adjust_marks_map(
        map: &mut AHashMap<MarkName, Mark>,
        pos: usize,
        old_len: usize,
        delta: isize,
        skip_threshold: Option<usize>,
    ) {
        for mark in map.values_mut() {
            let val = mark.offset().get();
            if let Some(threshold) = skip_threshold {
                if val <= threshold {
                    continue;
                }
            }
            let adjusted = super::offset_adjust::adjust_offset_named(val, pos, old_len, delta);
            if adjusted != val {
                // topline_offset is relative (line count) — stable across edits.
                *mark = Mark::with_topline_offset(Offset::new(adjusted), mark.topline_offset());
            }
        }
    }

    /// Iterate over all global mark entries.
    ///
    /// Yields `(MarkName, &GlobalMarkEntry)` pairs for marks stored in the
    /// `globals` map. Legacy global marks in `named_globals` (set via [`Self::set`]
    /// without a buffer id) are *not* included.
    pub fn globals(&self) -> impl Iterator<Item = (MarkName, &GlobalMarkEntry)> + '_ {
        self.globals.iter().map(|(k, v)| (*k, v))
    }

    /// Clear all local marks.
    ///
    /// # Complexity
    ///
    /// Time: O(M) where M = number of named marks stored. The `retain()`
    /// call scans all entries, keeping only global marks (A-Z).
    ///
    /// Space: O(1)
    pub const fn clear_local(&mut self) {
        self.version += 1;
        self.local_az = [None; 26];
    }

    /// Extract all per-buffer marks, leaving only global marks (A-Z).
    ///
    /// Drains local named marks (a-z) from `named` and takes the entire
    /// `special` map. Global marks (A-Z) in both `named` (legacy) and
    /// `globals` are untouched.
    ///
    /// Used by `VimEngine::on_buffer_leave` to save per-buffer mark state.
    pub fn take_buffer_marks(&mut self) -> BufferMarks {
        self.version += 1;
        // Extract local marks from the fixed array into a hash map for BufferMarks
        let mut local = AHashMap::default();
        for (letter, slot) in ('a'..='z').zip(self.local_az.iter_mut()) {
            if let Some(mark) = slot.take() {
                if let Some(name) = MarkName::new(letter) {
                    local.insert(name, mark);
                }
            }
        }
        let special = std::mem::take(&mut self.special);
        BufferMarks { local, special }
    }

    /// Restore per-buffer marks from a previous `take_buffer_marks` call.
    ///
    /// Removes all existing local marks (a-z) from `named` and replaces
    /// `special` with the saved state. Global marks (A-Z) are untouched.
    ///
    /// Used by `VimEngine::on_buffer_enter` to restore per-buffer mark state.
    pub fn set_buffer_marks(&mut self, marks: BufferMarks) {
        self.version += 1;
        // Clear local marks and restore from BufferMarks
        self.local_az = [None; 26];
        for (name, mark) in marks.local {
            if name.is_local() {
                let idx = (name.char() as u8 - b'a') as usize;
                if let Some(slot) = self.local_az.get_mut(idx) {
                    *slot = Some(mark);
                }
            }
        }
        self.special = marks.special;
    }

    /// Remap visual marks `<` and `>` through a ChangeSet.
    ///
    /// Unlike the main `remap()` method (which skips visual marks because
    /// normal effect processing sets them explicitly), this method remaps
    /// ONLY visual marks. Used by `apply_external_edit()` to keep visual
    /// selection bounds consistent with external text changes.
    pub(crate) fn remap_visual_marks(
        &mut self,
        changeset: &crate::primitives::changeset::ChangeSet,
    ) {
        use crate::primitives::changeset::Assoc;

        let mut changed = false;
        for (name, mark) in &mut self.special {
            if !name.is_visual() {
                continue;
            }
            let new_offset = changeset.map_offset(mark.offset(), Assoc::Before);
            if new_offset != mark.offset() {
                *mark = Mark::with_topline_offset(new_offset, mark.topline_offset());
                changed = true;
            }
        }
        if changed {
            self.version += 1;
        }
    }

    /// Remap the insert-stop mark (`^`) through a ChangeSet.
    ///
    /// The main `remap()` method (via `RemapPositions`) skips `is_insert_stop()`
    /// because the effect processor handles it with line/col semantics. For
    /// external edits, a simple ChangeSet remap is sufficient.
    pub(crate) fn remap_insert_stop(
        &mut self,
        changeset: &crate::primitives::changeset::ChangeSet,
    ) {
        use crate::primitives::changeset::Assoc;

        if let Some(mark) = self.special.get_mut(&MarkName::INSERT_STOP) {
            let new_offset = changeset.map_offset(mark.offset(), Assoc::Before);
            if new_offset != mark.offset() {
                *mark = Mark::with_topline_offset(new_offset, mark.topline_offset());
                self.version += 1;
            }
        }
    }
}

impl super::remap::RemapPositions for Marks {
    /// Remap special marks through a ChangeSet.
    ///
    /// Only special marks (. ^ [ ] < > ' `) are remapped here.
    /// Visual marks and insert-stop are skipped. Named marks (a-z, A-Z)
    /// are handled separately by `adjust_named_offsets` which uses
    /// different (cross-line) semantics.
    fn remap(&mut self, changeset: &crate::primitives::changeset::ChangeSet) {
        use crate::primitives::changeset::Assoc;

        self.version += 1;

        for (name, mark) in &mut self.special {
            // Skip visual and insert-stop marks — they have custom adjustment
            // logic in handle_text_mutation (effect_processor).
            if name.is_visual() || name.is_insert_stop() {
                continue;
            }
            let new_offset = changeset.map_offset(mark.offset(), Assoc::Before);
            if new_offset != mark.offset() {
                *mark = Mark::with_topline_offset(new_offset, mark.topline_offset());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test MarkName.
    fn mn(c: char) -> MarkName {
        MarkName::new(c).unwrap()
    }

    #[test]
    fn test_local_mark() {
        let mut marks = Marks::new();

        marks.set(mn('a'), Mark::from_raw(100));
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 100);
    }

    #[test]
    fn test_global_mark() {
        let mut marks = Marks::new();

        marks.set(mn('A'), Mark::from_raw(200));
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 200);
    }

    #[test]
    fn test_previous_position() {
        let mut marks = Marks::new();

        marks.set_previous_position(Offset::new(50));

        // Both ' and ` should be set
        assert_eq!(marks.get(mn('\'')).unwrap().offset().get(), 50);
        assert_eq!(marks.get(mn('`')).unwrap().offset().get(), 50);
    }

    #[test]
    fn test_change_region() {
        let mut marks = Marks::new();

        marks.set_change_region(Offset::new(10), Offset::new(20));

        assert_eq!(marks.get(mn('[')).unwrap().offset().get(), 10);
        assert_eq!(marks.get(mn(']')).unwrap().offset().get(), 20);
    }

    #[test]
    fn test_visual_region() {
        let mut marks = Marks::new();

        marks.set_visual_region(Offset::new(5), Offset::new(15));

        assert_eq!(marks.get(mn('<')).unwrap().offset().get(), 5);
        assert_eq!(marks.get(mn('>')).unwrap().offset().get(), 15);
    }

    #[test]
    fn test_delete_mark() {
        let mut marks = Marks::new();

        marks.set(mn('a'), Mark::from_raw(100));
        assert!(marks.get(mn('a')).is_some());

        marks.delete(mn('a'));
        assert!(marks.get(mn('a')).is_none());
    }

    #[test]
    fn test_mark_validation() {
        assert!(MarkName::new('a').is_some());
        assert!(MarkName::new('Z').is_some());
        assert!(MarkName::new('\'').is_some());
        assert!(MarkName::new('.').is_some());
        assert!(MarkName::new('!').is_none());
    }

    // ========== FIDELITY TESTS ==========

    /// Test all 26 lowercase local marks a-z
    #[test]
    fn test_all_lowercase_marks() {
        let mut marks = Marks::new();

        for c in 'a'..='z' {
            marks.set(mn(c), Mark::from_raw(c as usize));
        }

        for c in 'a'..='z' {
            assert_eq!(marks.get(mn(c)).unwrap().offset().get(), c as usize);
        }
    }

    /// Test all 26 uppercase global marks A-Z
    #[test]
    fn test_all_uppercase_marks() {
        let mut marks = Marks::new();

        for c in 'A'..='Z' {
            marks.set(mn(c), Mark::from_raw(c as usize));
        }

        for c in 'A'..='Z' {
            assert_eq!(marks.get(mn(c)).unwrap().offset().get(), c as usize);
        }
    }

    /// Test that local marks are cleared but global marks remain on clear_local
    #[test]
    fn test_clear_local_preserves_global() {
        let mut marks = Marks::new();

        marks.set(mn('a'), Mark::from_raw(100));
        marks.set(mn('A'), Mark::from_raw(200));

        marks.clear_local();

        assert!(marks.get(mn('a')).is_none());
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 200);
    }

    /// Test overwriting a mark replaces its position
    #[test]
    fn test_mark_overwrite() {
        let mut marks = Marks::new();

        marks.set(mn('a'), Mark::from_raw(100));
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 100);

        marks.set(mn('a'), Mark::from_raw(200));
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 200);
    }

    /// Test numbered marks 0-9 are valid MarkNames but not stored locally
    #[test]
    fn test_digit_marks_valid() {
        for c in '0'..='9' {
            assert!(MarkName::new(c).is_some(), "Mark {} should be valid", c);
        }
    }

    /// Test special automatic marks (', ', [, ], <, >, .)
    #[test]
    fn test_automatic_marks_valid() {
        for c in ['\'', '`', '[', ']', '<', '>', '.', '^'] {
            assert!(MarkName::new(c).is_some(), "Mark {} should be valid", c);
        }
    }

    /// Test insert-position mark ^ is valid
    #[test]
    fn test_insert_position_mark() {
        let mut marks = Marks::new();

        marks.set(mn('^'), Mark::from_raw(50));
        assert_eq!(marks.get(mn('^')).unwrap().offset().get(), 50);
    }

    #[test]
    fn test_invalid_mark_returns_none() {
        // Invalid chars can't even construct a MarkName
        assert!(MarkName::new('!').is_none());
        assert!(MarkName::new('@').is_none());
        assert!(MarkName::new(' ').is_none());
    }

    #[test]
    fn test_delete_nonexistent_returns_none() {
        let mut marks = Marks::new();
        assert!(marks.delete(mn('a')).is_none());
        assert!(marks.delete(mn('Z')).is_none());
    }

    #[test]
    fn test_clear_local_preserves_global_and_special() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(1));
        marks.set(mn('z'), Mark::from_raw(2));
        marks.set(mn('A'), Mark::from_raw(3));
        marks.set_previous_position(Offset::new(4));

        marks.clear_local();

        // Local marks gone
        assert!(marks.get(mn('a')).is_none());
        assert!(marks.get(mn('z')).is_none());
        // Global and special preserved
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 3);
        assert_eq!(marks.get(mn('\'')).unwrap().offset().get(), 4);
    }

    // ========== MARK INVALIDATION TESTS ==========

    #[test]
    fn test_invalidate_named_in_deleted_range() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(50));

        marks.invalidate_named_in_range(30, 60);

        // Mark at 50 is in [30, 60) → invalidated
        assert!(marks.get(mn('a')).is_none());
    }

    #[test]
    fn test_invalidate_preserves_marks_outside_range() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(10));
        marks.set(mn('b'), Mark::from_raw(50));
        marks.set(mn('c'), Mark::from_raw(80));

        marks.invalidate_named_in_range(30, 60);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10); // before range
        assert!(marks.get(mn('b')).is_none()); // inside range
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 80); // after range
    }

    #[test]
    fn test_invalidate_boundary_exclusive_end() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(6));

        // Delete range [0, 6) — mark at 6 is at exclusive end, survives
        marks.invalidate_named_in_range(0, 6);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 6);
    }

    // ========== TOPLINE_OFFSET (VIEWPORT CONTEXT) TESTS ==========

    #[test]
    fn mark_with_topline_offset() {
        let mark = Mark::with_topline_offset(Offset::new(100), Some(5));
        assert_eq!(mark.offset().get(), 100);
        assert_eq!(mark.topline_offset().unwrap(), 5);
    }

    #[test]
    fn mark_without_topline_offset() {
        let mark = Mark::new(Offset::new(100));
        assert_eq!(mark.offset().get(), 100);
        assert!(mark.topline_offset().is_none());
    }

    #[test]
    fn mark_from_raw_has_no_topline_offset() {
        let mark = Mark::from_raw(42);
        assert!(mark.topline_offset().is_none());
    }

    #[test]
    fn mark_topline_offset_preserved_in_marks_store() {
        let mut marks = Marks::new();
        let mark = Mark::with_topline_offset(Offset::new(100), Some(5));
        marks.set(mn('a'), mark);

        let retrieved = marks.get(mn('a')).unwrap();
        assert_eq!(retrieved.offset().get(), 100);
        assert_eq!(retrieved.topline_offset().unwrap(), 5);
    }

    #[test]
    fn mark_topline_offset_overwritten_on_re_set() {
        let mut marks = Marks::new();
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(100), Some(5)),
        );
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(200), Some(8)),
        );

        let retrieved = marks.get(mn('a')).unwrap();
        assert_eq!(retrieved.offset().get(), 200);
        assert_eq!(retrieved.topline_offset().unwrap(), 8);
    }

    #[test]
    fn auto_mark_has_no_topline_offset() {
        let mut marks = Marks::new();
        marks.set_previous_position(Offset::new(42));

        let retrieved = marks.get(mn('\'')).unwrap();
        assert!(
            retrieved.topline_offset().is_none(),
            "auto-marks should have no topline_offset"
        );
    }

    // ========== TOPLINE_OFFSET STABILITY TESTS ==========
    //
    // With relative encoding, topline_offset is inherently stable across
    // edits — only the byte offset changes, the relative line count stays.

    #[test]
    fn topline_offset_stable_on_insert_before_mark() {
        let mut marks = Marks::new();
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(100), Some(5)),
        );

        // Insert 30 bytes at pos 10, edit_line_end = 20
        marks.adjust_named_offsets(10, 0, 30, 20);

        let mark = marks.get(mn('a')).unwrap();
        assert_eq!(mark.offset().get(), 130);
        // topline_offset is relative — unchanged
        assert_eq!(mark.topline_offset().unwrap(), 5);
    }

    #[test]
    fn topline_offset_stable_on_delete_before_mark() {
        let mut marks = Marks::new();
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(100), Some(5)),
        );

        // Delete 20 bytes at pos 10, edit_line_end = 30
        marks.adjust_named_offsets(10, 20, 0, 30);

        let mark = marks.get(mn('a')).unwrap();
        assert_eq!(mark.offset().get(), 80);
        // topline_offset is relative — unchanged
        assert_eq!(mark.topline_offset().unwrap(), 5);
    }

    #[test]
    fn topline_offset_none_stays_none_after_adjustment() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::new(Offset::new(100)));

        marks.adjust_named_offsets(10, 0, 30, 20);

        let mark = marks.get(mn('a')).unwrap();
        assert_eq!(mark.offset().get(), 130);
        assert!(mark.topline_offset().is_none());
    }

    #[test]
    fn topline_offset_stable_on_large_delete() {
        let mut marks = Marks::new();
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(100), Some(3)),
        );

        // Delete 80 bytes at pos 5, edit_line_end = 6
        marks.adjust_named_offsets(5, 80, 0, 6);

        let mark = marks.get(mn('a')).unwrap();
        assert_eq!(mark.offset().get(), 20);
        // topline_offset is relative — unchanged even on large deletes
        assert_eq!(mark.topline_offset().unwrap(), 3);
    }

    // ========== BUFFER MARKS TAKE/SET TESTS ==========

    #[test]
    fn take_buffer_marks_extracts_local_and_special() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(10));
        marks.set(mn('m'), Mark::from_raw(50));
        marks.set(mn('z'), Mark::from_raw(90));
        marks.set(mn('A'), Mark::from_raw(200));
        marks.set(mn('Z'), Mark::from_raw(300));
        marks.set_visual_region(Offset::new(5), Offset::new(15));
        marks.set_last_change(Offset::new(42));

        let buffer_marks = marks.take_buffer_marks();

        // Global marks remain
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 200);
        assert_eq!(marks.get(mn('Z')).unwrap().offset().get(), 300);

        // Local marks gone from Marks
        assert!(marks.get(mn('a')).is_none());
        assert!(marks.get(mn('m')).is_none());
        assert!(marks.get(mn('z')).is_none());

        // Special marks gone from Marks
        assert!(marks.get(mn('<')).is_none());
        assert!(marks.get(mn('>')).is_none());
        assert!(marks.get(mn('.')).is_none());

        // Verify buffer_marks content
        assert_eq!(buffer_marks.local.len(), 3);
        assert_eq!(buffer_marks.local[&mn('a')].offset().get(), 10);
        assert_eq!(buffer_marks.local[&mn('m')].offset().get(), 50);
        assert_eq!(buffer_marks.local[&mn('z')].offset().get(), 90);
        assert!(buffer_marks.special.contains_key(&mn('<')));
        assert!(buffer_marks.special.contains_key(&mn('>')));
        assert!(buffer_marks.special.contains_key(&mn('.')));
    }

    #[test]
    fn set_buffer_marks_restores_and_preserves_global() {
        let mut marks = Marks::new();
        marks.set(mn('A'), Mark::from_raw(200));
        marks.set(mn('x'), Mark::from_raw(999)); // stale local from previous buffer

        let mut buffer_marks = BufferMarks::default();
        buffer_marks.local.insert(mn('a'), Mark::from_raw(10));
        buffer_marks.local.insert(mn('b'), Mark::from_raw(20));
        buffer_marks
            .special
            .insert(MarkName::LAST_CHANGE, Mark::from_raw(42));

        marks.set_buffer_marks(buffer_marks);

        // Restored local marks
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10);
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 20);
        // Stale local mark removed
        assert!(marks.get(mn('x')).is_none());
        // Global mark preserved
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 200);
        // Special mark restored
        assert_eq!(marks.get(mn('.')).unwrap().offset().get(), 42);
    }

    #[test]
    fn buffer_marks_round_trip() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(10));
        marks.set(mn('z'), Mark::from_raw(90));
        marks.set(mn('A'), Mark::from_raw(200));
        marks.set_visual_region(Offset::new(5), Offset::new(15));

        let saved = marks.take_buffer_marks();
        marks.set_buffer_marks(saved);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10);
        assert_eq!(marks.get(mn('z')).unwrap().offset().get(), 90);
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 200);
        assert_eq!(marks.get(mn('<')).unwrap().offset().get(), 5);
        assert_eq!(marks.get(mn('>')).unwrap().offset().get(), 15);
    }

    #[test]
    fn buffer_marks_default_is_empty() {
        let bm = BufferMarks::default();
        assert!(bm.local.is_empty());
        assert!(bm.special.is_empty());
    }

    // ========== GLOBAL MARK ENTRY TESTS ==========

    #[test]
    fn set_with_buffer_id_stores_in_globals() {
        let mut marks = Marks::new();
        let bid = BufferId::new(42);

        marks.set_with_buffer_id(mn('A'), Mark::from_raw(100), Some(bid));

        // Retrievable via get()
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 100);

        // Retrievable via get_global() with buffer association
        let entry = marks.get_global(mn('A')).unwrap();
        assert_eq!(entry.mark.offset().get(), 100);
        assert_eq!(entry.buffer_id, bid);
    }

    #[test]
    fn set_with_buffer_id_none_falls_back_to_named() {
        let mut marks = Marks::new();

        marks.set_with_buffer_id(mn('B'), Mark::from_raw(200), None);

        // Retrievable via get() (from named map)
        assert_eq!(marks.get(mn('B')).unwrap().offset().get(), 200);

        // NOT in globals — get_global returns None
        assert!(marks.get_global(mn('B')).is_none());
    }

    #[test]
    fn set_with_buffer_id_cleans_up_legacy() {
        let mut marks = Marks::new();
        let bid = BufferId::new(1);

        // Set via legacy path first
        marks.set(mn('C'), Mark::from_raw(50));
        assert!(marks.get_global(mn('C')).is_none());

        // Upgrade to globals path
        marks.set_with_buffer_id(mn('C'), Mark::from_raw(75), Some(bid));

        // Should be in globals now
        let entry = marks.get_global(mn('C')).unwrap();
        assert_eq!(entry.mark.offset().get(), 75);
        assert_eq!(entry.buffer_id, bid);

        // get() returns the globals entry
        assert_eq!(marks.get(mn('C')).unwrap().offset().get(), 75);
    }

    #[test]
    fn get_global_returns_none_for_local_marks() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(10));

        assert!(marks.get_global(mn('a')).is_none());
    }

    #[test]
    fn local_marks_unaffected_by_global_split() {
        let mut marks = Marks::new();
        let bid = BufferId::new(1);

        marks.set(mn('a'), Mark::from_raw(10));
        marks.set_with_buffer_id(mn('A'), Mark::from_raw(100), Some(bid));

        // Local mark works normally
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10);
        assert!(marks.get_global(mn('a')).is_none());

        // Global mark works
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 100);
        assert!(marks.get_global(mn('A')).is_some());
    }

    #[test]
    fn globals_survive_take_buffer_marks() {
        let mut marks = Marks::new();
        let bid = BufferId::new(7);

        marks.set(mn('a'), Mark::from_raw(10));
        marks.set_with_buffer_id(mn('A'), Mark::from_raw(100), Some(bid));
        marks.set_last_change(Offset::new(42));

        let buffer_marks = marks.take_buffer_marks();

        // Local marks extracted
        assert!(marks.get(mn('a')).is_none());
        assert_eq!(buffer_marks.local.len(), 1);
        assert_eq!(buffer_marks.local[&mn('a')].offset().get(), 10);

        // Global mark survives in globals
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 100);
        let entry = marks.get_global(mn('A')).unwrap();
        assert_eq!(entry.buffer_id, bid);
    }

    #[test]
    fn delete_global_mark_from_globals() {
        let mut marks = Marks::new();
        let bid = BufferId::new(1);

        marks.set_with_buffer_id(mn('D'), Mark::from_raw(300), Some(bid));
        assert!(marks.get(mn('D')).is_some());

        let removed = marks.delete(mn('D'));
        assert_eq!(removed.unwrap().offset().get(), 300);
        assert!(marks.get(mn('D')).is_none());
        assert!(marks.get_global(mn('D')).is_none());
    }

    #[test]
    fn delete_global_mark_from_both_maps() {
        let mut marks = Marks::new();
        let bid = BufferId::new(1);

        // Set in both maps (legacy and globals)
        marks.set(mn('E'), Mark::from_raw(100));
        marks.globals.insert(
            mn('E'),
            GlobalMarkEntry {
                mark: Mark::from_raw(200),
                buffer_id: bid,
                path: None,
            },
        );

        // Delete should remove from both, prefer globals value
        let removed = marks.delete(mn('E'));
        assert_eq!(removed.unwrap().offset().get(), 200);
        assert!(marks.get(mn('E')).is_none());
    }

    #[test]
    fn adjust_global_offsets_only_adjusts_matching_buffer() {
        let mut marks = Marks::new();
        let buf1 = BufferId::new(1);
        let buf2 = BufferId::new(2);

        // Mark in buffer 1 at offset 100 (past the edit line)
        marks.set_with_buffer_id(mn('A'), Mark::from_raw(100), Some(buf1));
        // Mark in buffer 2 at offset 100
        marks.set_with_buffer_id(mn('B'), Mark::from_raw(100), Some(buf2));

        // Edit in buffer 1: insert 10 bytes at position 30, edit_line_end = 35
        marks.adjust_global_offsets(buf1, 30, 0, 10, 35);

        // Buffer 1 mark should be adjusted (100 → 110)
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 110);

        // Buffer 2 mark should NOT be adjusted
        assert_eq!(marks.get(mn('B')).unwrap().offset().get(), 100);
    }

    #[test]
    fn adjust_global_offsets_skips_marks_on_edit_line() {
        let mut marks = Marks::new();
        let buf = BufferId::new(1);

        // Mark at offset 30, which is on the edit line (edit_line_end = 50)
        marks.set_with_buffer_id(mn('A'), Mark::from_raw(30), Some(buf));
        // Mark at offset 100, past the edit line
        marks.set_with_buffer_id(mn('B'), Mark::from_raw(100), Some(buf));

        // Insert 10 bytes at position 20, edit_line_end = 50
        marks.adjust_global_offsets(buf, 20, 0, 10, 50);

        // Mark on edit line is skipped (val 30 <= edit_line_end 50)
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 30);

        // Mark past edit line is adjusted (100 → 110)
        assert_eq!(marks.get(mn('B')).unwrap().offset().get(), 110);
    }

    #[test]
    fn globals_iterator() {
        let mut marks = Marks::new();
        let bid = BufferId::new(5);

        marks.set_with_buffer_id(mn('A'), Mark::from_raw(10), Some(bid));
        marks.set_with_buffer_id(mn('B'), Mark::from_raw(20), Some(bid));
        // Legacy global — NOT in globals iterator
        marks.set(mn('C'), Mark::from_raw(30));

        let entries: Vec<_> = marks.globals().collect();
        assert_eq!(entries.len(), 2);

        // Verify both entries are present (order not guaranteed with AHashMap)
        let has_a = entries
            .iter()
            .any(|(name, e)| name.char() == 'A' && e.mark.offset().get() == 10);
        let has_b = entries
            .iter()
            .any(|(name, e)| name.char() == 'B' && e.mark.offset().get() == 20);
        assert!(has_a, "globals() should yield mark A");
        assert!(has_b, "globals() should yield mark B");
    }

    #[test]
    fn invalidate_named_in_range_also_invalidates_globals() {
        let mut marks = Marks::new();
        let bid = BufferId::new(1);

        marks.set(mn('a'), Mark::from_raw(50)); // local, in range
        marks.set_with_buffer_id(mn('A'), Mark::from_raw(55), Some(bid)); // global, in range
        marks.set_with_buffer_id(mn('B'), Mark::from_raw(10), Some(bid)); // global, out of range

        marks.invalidate_named_in_range(40, 60);

        assert!(
            marks.get(mn('a')).is_none(),
            "local mark in range should be invalidated"
        );
        assert!(
            marks.get(mn('A')).is_none(),
            "global mark in range should be invalidated"
        );
        assert_eq!(
            marks.get(mn('B')).unwrap().offset().get(),
            10,
            "global mark out of range should survive"
        );
    }

    #[test]
    fn set_with_buffer_id_local_ignores_buffer_id() {
        let mut marks = Marks::new();
        let bid = BufferId::new(99);

        // Setting a local mark with buffer_id should work like normal set()
        marks.set_with_buffer_id(mn('a'), Mark::from_raw(42), Some(bid));

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 42);
        // Local marks never go to globals
        assert!(marks.get_global(mn('a')).is_none());
    }

    #[test]
    fn clear_local_preserves_globals_map() {
        let mut marks = Marks::new();
        let bid = BufferId::new(1);

        marks.set(mn('a'), Mark::from_raw(10));
        marks.set_with_buffer_id(mn('A'), Mark::from_raw(100), Some(bid));
        marks.set(mn('B'), Mark::from_raw(200)); // legacy global in named

        marks.clear_local();

        // Local marks gone
        assert!(marks.get(mn('a')).is_none());
        // Both global paths preserved
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 100);
        assert_eq!(marks.get(mn('B')).unwrap().offset().get(), 200);
    }

    // ========== GLOBAL MARK PATH TESTS ==========

    #[test]
    fn global_mark_with_path() {
        let bid = BufferId::new(1);
        let entry = GlobalMarkEntry {
            mark: Mark::from_raw(100),
            buffer_id: bid,
            path: None,
        }
        .with_path("src/main.rs");

        assert_eq!(entry.path.as_deref(), Some("src/main.rs"));
        assert_eq!(entry.mark.offset().get(), 100);
        assert_eq!(entry.buffer_id, bid);
    }

    #[test]
    fn global_mark_default_no_path() {
        let bid = BufferId::new(2);
        let entry = GlobalMarkEntry {
            mark: Mark::from_raw(50),
            buffer_id: bid,
            path: None,
        };

        assert!(entry.path.is_none());
    }

    // ========== REMAP VISUAL MARKS TESTS ==========

    #[test]
    fn remap_visual_marks_shifts_on_insert_before() {
        use crate::primitives::changeset::ChangeSet;

        let mut marks = Marks::new();
        marks.set_visual_region(Offset::new(5), Offset::new(10));

        // Insert 3 bytes at offset 2 (before both visual marks).
        // Doc len must cover both marks: use 20.
        let changeset = ChangeSet::from_edit_len(20, 2, 0, 3);
        marks.remap_visual_marks(&changeset);

        assert_eq!(marks.get(mn('<')).unwrap().offset().get(), 8);
        assert_eq!(marks.get(mn('>')).unwrap().offset().get(), 13);
    }

    // ========== REFACTOR CHARACTERIZATION TESTS ==========
    //
    // These tests capture the exact behavior of each adjustment function
    // before the `remap_all` consolidation. They must continue to pass
    // identically after the refactor.

    // --- adjust_named_offsets (skip_same_line = true) ---

    #[test]
    fn refactor_adjust_named_cross_line_insert_after_edit_line() {
        let mut marks = Marks::new();
        // Marks at various positions
        marks.set(mn('a'), Mark::from_raw(10)); // before edit
        marks.set(mn('b'), Mark::from_raw(50)); // on edit line (edit_line_end = 60)
        marks.set(mn('c'), Mark::from_raw(100)); // past edit line
        marks.set(mn('A'), Mark::from_raw(100)); // global, past edit line

        // Insert 10 bytes at position 30, edit_line_end = 60
        marks.adjust_named_offsets(30, 0, 10, 60);

        // 'a' at 10 < pos (30) → unchanged
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10);
        // 'b' at 50 <= edit_line_end (60) → skipped (cross-line semantics)
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 50);
        // 'c' at 100 > edit_line_end → shifted by delta (+10)
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 110);
        // 'A' at 100 > edit_line_end → shifted by delta (+10)
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 110);
    }

    #[test]
    fn refactor_adjust_named_cross_line_delete_after_edit_line() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(10)); // before edit
        marks.set(mn('b'), Mark::from_raw(50)); // on edit line
        marks.set(mn('c'), Mark::from_raw(100)); // past edit line

        // Delete 20 bytes at position 30, edit_line_end = 60
        marks.adjust_named_offsets(30, 20, 0, 60);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10);
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 50); // skipped
                                                                    // 'c' at 100: > edit_line_end → adjust_offset_named(100, 30, 20, -20) → 80
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 80);
    }

    #[test]
    fn refactor_adjust_named_cross_line_mark_at_edit_line_end_boundary() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(60)); // exactly at edit_line_end
        marks.set(mn('b'), Mark::from_raw(61)); // just past edit_line_end

        // Insert 5 bytes at position 30, edit_line_end = 60
        marks.adjust_named_offsets(30, 0, 5, 60);

        // val == edit_line_end → skipped (val <= edit_line_end)
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 60);
        // val > edit_line_end → adjusted
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 66);
    }

    #[test]
    fn refactor_adjust_named_cross_line_topline_offset_stable() {
        let mut marks = Marks::new();
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(100), Some(5)),
        );

        marks.adjust_named_offsets(30, 0, 10, 60);

        let mark = marks.get(mn('a')).unwrap();
        assert_eq!(mark.offset().get(), 110);
        // topline_offset is relative — unchanged
        assert_eq!(mark.topline_offset().unwrap(), 5);
    }

    #[test]
    fn refactor_adjust_named_cross_line_zero_delta_zero_old_len_noop() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(100));
        let v_before = marks.version();

        // pos=30, old_len=0, new_len=0 → delta=0 && old_len=0 → early return
        marks.adjust_named_offsets(30, 0, 0, 60);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 100);
        // Version should NOT be bumped on no-op
        assert_eq!(marks.version(), v_before);
    }

    // --- adjust_named_offsets_ext (skip_same_line = false) ---

    #[test]
    fn refactor_adjust_named_no_skip_insert() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(10)); // before edit
        marks.set(mn('b'), Mark::from_raw(30)); // at edit pos
        marks.set(mn('c'), Mark::from_raw(50)); // on edit line
        marks.set(mn('d'), Mark::from_raw(100)); // past edit line
        marks.set(mn('A'), Mark::from_raw(50)); // global, on edit line

        // Insert 10 bytes at position 30, edit_line_end = 60, skip_same_line = false
        marks.adjust_named_offsets_ext(30, 0, 10, 60, false);

        // 'a' at 10 < pos → unchanged
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 10);
        // 'b' at 30 == pos → adjust_offset_named uses strict < → shifted
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 40);
        // 'c' at 50 → shifted (no skip)
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 60);
        // 'd' at 100 → shifted
        assert_eq!(marks.get(mn('d')).unwrap().offset().get(), 110);
        // 'A' at 50 → shifted (no skip)
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 60);
    }

    #[test]
    fn refactor_adjust_named_no_skip_delete() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(35)); // inside deleted region
        marks.set(mn('b'), Mark::from_raw(80)); // after deleted region

        // Delete 20 bytes at position 30, skip_same_line = false
        marks.adjust_named_offsets_ext(30, 20, 0, 60, false);

        // 'a' at 35: inside [30, 50) → clamped to pos (30)
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 30);
        // 'b' at 80: after edit → shifted by -20
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 60);
    }

    // --- adjust_global_offsets ---

    #[test]
    fn refactor_adjust_global_offsets_insert_shifts_matching_buffer() {
        let mut marks = Marks::new();
        let buf1 = BufferId::new(1);
        let buf2 = BufferId::new(2);

        marks.set_with_buffer_id(mn('A'), Mark::from_raw(30), Some(buf1)); // on edit line
        marks.set_with_buffer_id(mn('B'), Mark::from_raw(100), Some(buf1)); // past
        marks.set_with_buffer_id(mn('C'), Mark::from_raw(100), Some(buf2)); // wrong buffer
        marks.set_with_buffer_id(
            mn('D'),
            Mark::with_topline_offset(Offset::new(100), Some(3)),
            Some(buf1),
        );

        marks.adjust_global_offsets(buf1, 30, 0, 10, 50);

        // 'A' at 30 <= edit_line_end (50) → skipped
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 30);
        // 'B' at 100 > edit_line_end, buf1 → shifted
        assert_eq!(marks.get(mn('B')).unwrap().offset().get(), 110);
        // 'C' at 100, buf2 → not touched
        assert_eq!(marks.get(mn('C')).unwrap().offset().get(), 100);
        // 'D' at 100 → shifted, topline_offset is relative — unchanged
        let d = marks.get(mn('D')).unwrap();
        assert_eq!(d.offset().get(), 110);
        assert_eq!(d.topline_offset().unwrap(), 3);
    }

    #[test]
    fn refactor_adjust_global_offsets_zero_delta_noop() {
        let mut marks = Marks::new();
        let buf = BufferId::new(1);
        marks.set_with_buffer_id(mn('A'), Mark::from_raw(100), Some(buf));
        let v_before = marks.version();

        // old_len=0, new_len=0 → delta=0 && old_len=0 → early return
        marks.adjust_global_offsets(buf, 30, 0, 0, 50);

        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 100);
        assert_eq!(marks.version(), v_before);
    }

    // --- invalidate_named_in_range ---

    #[test]
    fn refactor_invalidate_locals_globals_and_globals_map() {
        let mut marks = Marks::new();
        let bid = BufferId::new(1);

        marks.set(mn('a'), Mark::from_raw(5)); // local, below range
        marks.set(mn('b'), Mark::from_raw(15)); // local, in range
        marks.set(mn('c'), Mark::from_raw(25)); // local, above range
        marks.set(mn('A'), Mark::from_raw(15)); // legacy global, in range
        marks.set(mn('B'), Mark::from_raw(5)); // legacy global, below
        marks.set_with_buffer_id(mn('C'), Mark::from_raw(15), Some(bid)); // globals map, in range
        marks.set_with_buffer_id(mn('D'), Mark::from_raw(25), Some(bid)); // globals map, above

        marks.invalidate_named_in_range(10, 20);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 5);
        assert!(marks.get(mn('b')).is_none()); // invalidated
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 25);
        assert!(marks.get(mn('A')).is_none()); // invalidated
        assert_eq!(marks.get(mn('B')).unwrap().offset().get(), 5);
        assert!(marks.get(mn('C')).is_none()); // invalidated
        assert_eq!(marks.get(mn('D')).unwrap().offset().get(), 25);
    }

    #[test]
    fn refactor_invalidate_range_start_inclusive_end_exclusive() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(10)); // at start (inclusive)
        marks.set(mn('b'), Mark::from_raw(19)); // just before end
        marks.set(mn('c'), Mark::from_raw(20)); // at end (exclusive, survives)

        marks.invalidate_named_in_range(10, 20);

        assert!(marks.get(mn('a')).is_none());
        assert!(marks.get(mn('b')).is_none());
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 20);
    }

    #[test]
    fn refactor_invalidate_empty_range_noop() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(15));
        let v_before = marks.version();

        marks.invalidate_named_in_range(15, 15); // empty range

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 15);
        assert_eq!(marks.version(), v_before);
    }

    // --- Combined scenario: insert that crosses lines ---

    #[test]
    fn refactor_cross_line_insert_with_skip() {
        // Simulates: insert "hello\nworld" at offset 10, edit_line_end = 20
        // Only marks PAST edit_line_end should shift.
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(15)); // on edit line
        marks.set(mn('b'), Mark::from_raw(20)); // at edit_line_end
        marks.set(mn('c'), Mark::from_raw(21)); // past edit_line_end
        marks.set(mn('d'), Mark::from_raw(5)); // before edit

        // "hello\nworld" = 11 bytes inserted at pos 10
        marks.adjust_named_offsets(10, 0, 11, 20);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 15); // skipped
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 20); // skipped
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 32); // 21 + 11
        assert_eq!(marks.get(mn('d')).unwrap().offset().get(), 5); // before
    }

    #[test]
    fn refactor_cross_line_insert_without_skip() {
        // skip_same_line = false: ALL marks at/after pos shift
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(10)); // at edit pos
        marks.set(mn('b'), Mark::from_raw(15)); // on edit line
        marks.set(mn('c'), Mark::from_raw(21)); // past edit_line_end
        marks.set(mn('d'), Mark::from_raw(5)); // before edit

        marks.adjust_named_offsets_ext(10, 0, 11, 20, false);

        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 21); // 10+11
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 26); // 15+11
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 32); // 21+11
        assert_eq!(marks.get(mn('d')).unwrap().offset().get(), 5); // before
    }

    // --- Replace scenario ---

    #[test]
    fn refactor_adjust_named_replace_shrinks() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(35)); // inside deleted region
        marks.set(mn('b'), Mark::from_raw(80)); // after edit

        // Replace 20 bytes at pos 30 with 5 bytes (delta = -15), edit_line_end = 60
        marks.adjust_named_offsets(30, 20, 5, 60);

        // 'a' at 35 <= edit_line_end (60) → skipped
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 35);
        // 'b' at 80 > edit_line_end → adjust_offset_named(80, 30, 20, -15) → 65
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 65);
    }

    #[test]
    fn refactor_adjust_named_no_skip_replace_clamps_inside_delete() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(35)); // inside deleted [30, 50)
        marks.set(mn('b'), Mark::from_raw(80)); // after

        // Replace 20 bytes at pos 30 with 5 bytes, no skip
        marks.adjust_named_offsets_ext(30, 20, 5, 60, false);

        // 'a' at 35: inside [30, 50) → clamped to 30
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 30);
        // 'b' at 80: after → shifted by -15
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 65);
    }

    // --- Version bumping ---

    #[test]
    fn refactor_version_bumped_on_adjust() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(100));
        let v_before = marks.version();

        marks.adjust_named_offsets(30, 0, 10, 60);

        // Version should bump even if no mark actually moved (conservative)
        assert!(marks.version() > v_before);
    }

    #[test]
    fn refactor_version_bumped_on_invalidate() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(50));
        let v_before = marks.version();

        marks.invalidate_named_in_range(40, 60);

        assert!(marks.version() > v_before);
    }

    #[test]
    fn refactor_version_not_bumped_if_no_invalidation() {
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(5));
        let v_before = marks.version();

        marks.invalidate_named_in_range(40, 60);

        // No mark in range → no change
        assert_eq!(marks.version(), v_before);
    }

    // ========== REMAP CONSOLIDATION REFACTOR VALIDATION TESTS ==========

    #[test]
    fn refactor_multiple_marks_at_same_offset() {
        // Multiple marks at the same offset should all be adjusted identically.
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(100));
        marks.set(mn('b'), Mark::from_raw(100));
        marks.set(mn('c'), Mark::from_raw(100));
        marks.set(mn('A'), Mark::from_raw(100)); // legacy global at same offset

        // Insert 15 bytes at position 30, edit_line_end = 60
        marks.adjust_named_offsets(30, 0, 15, 60);

        // All marks at 100 (> edit_line_end 60) should shift by +15
        assert_eq!(marks.get(mn('a')).unwrap().offset().get(), 115);
        assert_eq!(marks.get(mn('b')).unwrap().offset().get(), 115);
        assert_eq!(marks.get(mn('c')).unwrap().offset().get(), 115);
        assert_eq!(marks.get(mn('A')).unwrap().offset().get(), 115);
    }

    #[test]
    fn refactor_mark_exactly_at_edit_boundary() {
        // A mark whose offset equals the edit position (pos) with skip_same_line
        // active should be skipped when within edit_line_end, but adjusted when
        // skip_same_line is false.
        let mut marks = Marks::new();
        marks.set(mn('a'), Mark::from_raw(30)); // exactly at edit pos

        // With skip: mark at 30 <= edit_line_end (60) → skipped
        marks.adjust_named_offsets(30, 0, 10, 60);
        assert_eq!(
            marks.get(mn('a')).unwrap().offset().get(),
            30,
            "mark at edit pos should be skipped when <= edit_line_end"
        );

        // Without skip: mark at 30 == pos → adjusted
        let mut marks2 = Marks::new();
        marks2.set(mn('a'), Mark::from_raw(30));
        marks2.adjust_named_offsets_ext(30, 0, 10, 60, false);
        assert_eq!(
            marks2.get(mn('a')).unwrap().offset().get(),
            40,
            "mark at edit pos should shift when skip_same_line=false"
        );
    }

    #[test]
    fn refactor_global_mark_wrong_buffer_id_skipped() {
        // Global marks belonging to a different buffer_id must not be touched
        // by adjust_global_offsets.
        let mut marks = Marks::new();
        let target_buf = BufferId::new(10);
        let other_buf = BufferId::new(99);

        marks.set_with_buffer_id(mn('A'), Mark::from_raw(100), Some(other_buf));
        marks.set_with_buffer_id(mn('B'), Mark::from_raw(200), Some(other_buf));
        marks.set_with_buffer_id(mn('C'), Mark::from_raw(100), Some(target_buf));

        // Edit in target_buf: insert 20 bytes at pos 30, edit_line_end = 50
        marks.adjust_global_offsets(target_buf, 30, 0, 20, 50);

        // Marks in other_buf must remain unchanged
        assert_eq!(
            marks.get(mn('A')).unwrap().offset().get(),
            100,
            "global mark in wrong buffer should not be adjusted"
        );
        assert_eq!(
            marks.get(mn('B')).unwrap().offset().get(),
            200,
            "global mark in wrong buffer should not be adjusted"
        );
        // Mark in target_buf past edit_line_end should shift
        assert_eq!(
            marks.get(mn('C')).unwrap().offset().get(),
            120,
            "global mark in correct buffer past edit_line_end should shift"
        );
    }

    // ========== RELATIVE TOPLINE ENCODING TEST ==========

    #[test]
    fn task_5_2_relative_topline_stable_across_edits() {
        // Set mark with viewport at line 10, mark at line 15 → offset=5.
        // Insert 100 lines above. The topline_offset stays 5.
        let mut marks = Marks::new();
        marks.set(
            mn('a'),
            Mark::with_topline_offset(Offset::new(150), Some(5)),
        );

        // Simulate inserting 100 lines (say 1000 bytes) before the mark
        marks.adjust_named_offsets(0, 0, 1000, 0);

        let mark = marks.get(mn('a')).unwrap();
        // Byte offset shifted
        assert_eq!(mark.offset().get(), 1150);
        // Relative topline unchanged
        assert_eq!(mark.topline_offset().unwrap(), 5);
    }

    // ========== SERIALIZED MARKS TESTS (serde feature) ==========

    #[cfg(feature = "serde")]
    mod serde_tests {
        use super::*;

        #[test]
        fn serialized_marks_from_bytes_round_trip() {
            let data = vec![1, 2, 3, 4];
            let sm = super::super::SerializedMarks::from_bytes(data.clone());
            assert_eq!(sm.as_bytes(), &data[..]);
        }

        #[test]
        fn serialized_marks_empty() {
            let sm = super::super::SerializedMarks::from_bytes(vec![]);
            assert!(sm.is_empty());
        }

        #[test]
        fn serialized_marks_not_empty() {
            let sm = super::super::SerializedMarks::from_bytes(vec![42]);
            assert!(!sm.is_empty());
        }

        #[test]
        fn marks_serde_round_trip() {
            let mut marks = Marks::new();
            marks.set(mn('a'), Mark::from_raw(100));
            marks.set(mn('z'), Mark::from_raw(999));
            marks.set(mn('A'), Mark::from_raw(200));
            marks.set_previous_position(Offset::new(50));
            marks.set_last_change(Offset::new(42));

            let json = serde_json::to_vec(&marks).unwrap();
            let restored: Marks = serde_json::from_slice(&json).unwrap();

            assert_eq!(restored.get(mn('a')).unwrap().offset().get(), 100);
            assert_eq!(restored.get(mn('z')).unwrap().offset().get(), 999);
            assert_eq!(restored.get(mn('A')).unwrap().offset().get(), 200);
            assert_eq!(restored.get(mn('\'')).unwrap().offset().get(), 50);
            assert_eq!(restored.get(mn('.')).unwrap().offset().get(), 42);
        }

        #[test]
        fn global_mark_entry_serde_round_trip() {
            let bid = BufferId::new(7);
            let entry = GlobalMarkEntry {
                mark: Mark::from_raw(123),
                buffer_id: bid,
                path: Some(CompactString::from("src/lib.rs")),
            };

            let json = serde_json::to_vec(&entry).unwrap();
            let restored: GlobalMarkEntry = serde_json::from_slice(&json).unwrap();

            assert_eq!(restored.mark.offset().get(), 123);
            assert_eq!(restored.buffer_id, bid);
            assert_eq!(restored.path.as_deref(), Some("src/lib.rs"));
        }

        #[test]
        fn marks_export_import_via_serialized_marks() {
            let mut marks = Marks::new();
            marks.set(mn('a'), Mark::from_raw(100));
            marks.set(mn('m'), Mark::from_raw(500));
            marks.set_last_change(Offset::new(42));

            // Export
            let bytes = serde_json::to_vec(&marks).unwrap();
            let serialized = super::super::SerializedMarks::from_bytes(bytes);

            // Import into fresh marks
            let restored: Marks = serde_json::from_slice(serialized.as_bytes()).unwrap();

            assert_eq!(restored.get(mn('a')).unwrap().offset().get(), 100);
            assert_eq!(restored.get(mn('m')).unwrap().offset().get(), 500);
            assert_eq!(restored.get(mn('.')).unwrap().offset().get(), 42);
        }

        #[test]
        fn import_corrupted_data_fails_gracefully() {
            let corrupted = super::super::SerializedMarks::from_bytes(vec![0xFF, 0xFE, 0xFD]);
            let result = serde_json::from_slice::<Marks>(corrupted.as_bytes());
            assert!(result.is_err());
        }
    }
}
