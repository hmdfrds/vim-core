//! Jump list for Ctrl-O/Ctrl-I navigation.
//!
//! # Layering
//!
//! State holds pure data containers with no execution logic. Imports
//! `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`.
//!
//! Maintains a history of cursor positions for jump commands.
//!
//! # Behavior
//!
//! Jump list entries are added when:
//! - G, gg (goto line)
//! - /, ?, n, N (search)
//! - %, (, ) (sentence/paragraph)
//! - ', ` (mark jumps)
//!
//! Commands:
//! - Ctrl-O: Go to older position
//! - Ctrl-I: Go to newer position

use crate::primitives::{BufferId, Offset};
use std::collections::VecDeque;

/// Maximum number of jump list entries.
const MAX_JUMPS: usize = 100;

/// Jump list entry.
///
/// Each entry records a cursor position, optionally the buffer it belongs to,
/// and optionally a relative topline offset for viewport restoration.
/// When `buffer_id` is `Some`, the entry refers to a position in a specific buffer,
/// enabling cross-buffer Ctrl-O / Ctrl-I navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JumpEntry {
    /// Position in the document.
    offset: Offset,
    /// Buffer this entry belongs to, if known.
    ///
    /// `None` for entries created before cross-buffer support was available,
    /// or when the host doesn't supply buffer IDs.
    buffer_id: Option<BufferId>,
    /// Relative topline offset: `cursor_line - viewport_first_line` at jump time.
    ///
    /// Used by Ctrl-O/I to restore the viewport position. Resolved at jump:
    /// `topline_line = cursor_line - topline_offset`.
    #[cfg_attr(feature = "serde", serde(default))]
    topline_offset: Option<i32>,
}

impl JumpEntry {
    /// Create a new jump entry for the current buffer (no buffer ID).
    #[inline]
    #[must_use]
    pub const fn new(offset: Offset) -> Self {
        Self {
            offset,
            buffer_id: None,
            topline_offset: None,
        }
    }

    /// Create a jump entry with a buffer identifier for cross-buffer navigation.
    #[inline]
    #[must_use]
    pub const fn with_buffer(offset: Offset, buffer_id: Option<BufferId>) -> Self {
        Self {
            offset,
            buffer_id,
            topline_offset: None,
        }
    }

    /// Create a jump entry with buffer and topline offset.
    #[inline]
    #[must_use]
    pub const fn with_topline(
        offset: Offset,
        buffer_id: Option<BufferId>,
        topline_offset: Option<i32>,
    ) -> Self {
        Self {
            offset,
            buffer_id,
            topline_offset,
        }
    }

    /// Get the offset.
    #[inline]
    #[must_use]
    pub const fn offset(self) -> Offset {
        self.offset
    }

    /// Get the buffer identifier, if known.
    #[inline]
    #[must_use]
    pub const fn buffer_id(self) -> Option<BufferId> {
        self.buffer_id
    }

    /// Get the relative topline offset for viewport restoration.
    #[inline]
    #[must_use]
    pub const fn topline_offset(self) -> Option<i32> {
        self.topline_offset
    }
}

/// Jump list for Ctrl-O/Ctrl-I navigation.
///
/// Uses `VecDeque` for O(1) eviction of the oldest entry when capacity
/// is exceeded.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JumpList {
    /// List of jump positions.
    entries: VecDeque<JumpEntry>,
    /// Current position in the list (points to "current" entry).
    /// When at `len()`, we're at the present position (not in history).
    current: usize,
    /// When true, Ctrl-O then a new jump truncates forward entries
    /// (browser-style navigation). Corresponds to Vim's `jumpoptions=stack`.
    #[cfg_attr(feature = "serde", serde(default))]
    stack_mode: bool,
}

impl JumpList {
    /// Create a new empty jump list.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a position to the jump list.
    ///
    /// This is called before making a jump. The current position
    /// is saved so we can return to it with Ctrl-O.
    ///
    /// `buffer_id` identifies which buffer this position belongs to, enabling
    /// cross-buffer jump navigation. Pass `None` when the host doesn't track
    /// buffer identifiers.
    /// Push a new entry to the jump list.
    ///
    /// Matches Neovim's `setpcmark()`: only consecutive-duplicate suppression.
    /// Non-adjacent duplicates are left in place and cleaned up lazily by
    /// `cleanup()` at navigation time (matching Neovim's `cleanup_jumplist`
    /// called inside `get_jumplist`).
    pub fn push(&mut self, offset: Offset, buffer_id: Option<BufferId>) {
        self.push_with_topline(offset, buffer_id, None);
    }

    /// Push a new entry with optional topline offset for viewport restoration.
    pub fn push_with_topline(
        &mut self,
        offset: Offset,
        buffer_id: Option<BufferId>,
        topline_offset: Option<i32>,
    ) {
        // Don't add duplicate of last entry (same offset AND same buffer)
        if let Some(last) = self.entries.back() {
            if last.offset == offset && last.buffer_id == buffer_id {
                return;
            }
        }

        // jumpoptions=stack: truncate forward entries when pushing from middle.
        if self.stack_mode && self.current < self.entries.len() {
            self.entries.truncate(self.current);
        }

        // Append the new entry at the end.
        self.entries
            .push_back(JumpEntry::with_topline(offset, buffer_id, topline_offset));

        // Enforce max size — evict oldest entry via O(1) pop_front
        if self.entries.len() > MAX_JUMPS {
            self.entries.pop_front();
            // Removal shifts all indices down by one
            self.current = self.current.saturating_sub(1);
        }

        // Current is now at the end (present position)
        self.current = self.entries.len();
    }

    /// Push with `checkpcmark()`-style dedup.
    ///
    /// Like [`Self::push`], but also suppresses the entry when the previous entry
    /// is on the same line or within 1 line (same buffer), matching Vim's
    /// `checkpcmark()` in mark.c. The `line_of` callback maps an `Offset`
    /// to its 0-based line number.
    pub fn push_checked(
        &mut self,
        offset: Offset,
        buffer_id: Option<BufferId>,
        line_of: impl Fn(Offset) -> usize,
    ) {
        if let Some(last) = self.entries.back() {
            if last.buffer_id == buffer_id {
                if last.offset == offset {
                    return;
                }
                let last_line = line_of(last.offset);
                let new_line = line_of(offset);
                let diff = new_line.abs_diff(last_line);
                if diff <= 1 {
                    return;
                }
            }
        }
        self.push_with_topline(offset, buffer_id, None);
    }

    /// Remove non-adjacent duplicate entries, keeping the *later* occurrence.
    ///
    /// Mirrors Neovim's `cleanup_jumplist()`. Deduplicates by **(line, buffer_id)**
    /// rather than exact byte offset, matching Neovim's `(fnum, lnum)` key.
    ///
    /// `line_of` maps a byte offset to its 0-indexed line number. Callers
    /// typically pass `|off| text[..off.get()].bytes().filter(|&b| b == b'\n').count()`
    /// or `|off| doc.line_of_offset(off.get())`.
    ///
    /// When the current index equals `len()` (i.e. at "present"), it is remapped
    /// to the new length. Otherwise it tracks through the compaction so it still
    /// points to the same logical entry.
    ///
    /// Additionally, if `cursor_offset` is `Some` and `current == len`,
    /// remove the last entry when it matches the cursor **line** (Neovim's
    /// "phantom jump" removal: avoids useless entries when the cursor is
    /// already at the top of the list).
    pub fn cleanup(&mut self, cursor_offset: Option<Offset>, line_of: impl Fn(Offset) -> usize) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }

        // Step 1: reverse-iterate with AHashSet — keep the LATEST occurrence of
        // each (line, buffer_id) pair and drop all earlier duplicates.
        // O(n) with MAX_JUMPS=100, replacing the prior O(n²) nested loop.
        let mut seen = ahash::AHashSet::with_capacity(len);
        let mut keep = vec![true; len];
        for (entry, keep_slot) in self.entries.iter().zip(keep.iter_mut()).rev() {
            let key = (line_of(entry.offset), entry.buffer_id);
            if !seen.insert(key) {
                *keep_slot = false;
            }
        }

        // Step 2: compact in-place, tracking `current`.
        let at_present = self.current == len;
        let mut to = 0usize;
        for (from, &keep_this) in keep.iter().enumerate().take(len) {
            if !at_present && self.current == from {
                self.current = to;
            }
            if keep_this {
                if to != from {
                    // `to <= from` always, and everything past `to` is dropped
                    // by the `truncate` below, so swapping is equivalent to the
                    // overwrite it replaces.
                    self.entries.swap(to, from);
                }
                to += 1;
            }
        }
        self.entries.truncate(to);
        if at_present {
            self.current = to;
        }

        // Step 3: phantom-jump removal (line-based comparison).
        if let Some(cursor_off) = cursor_offset {
            if self.current == self.entries.len() {
                if let Some(last) = self.entries.back() {
                    if line_of(last.offset) == line_of(cursor_off) {
                        self.entries.pop_back();
                        self.current = self.current.saturating_sub(1);
                    }
                }
            }
        }
    }

    /// Go to older position (Ctrl-O).
    ///
    /// Returns the position to jump to, or None if at start.
    pub fn older(&mut self) -> Option<Offset> {
        if self.current > 0 {
            self.current -= 1;
            self.entries.get(self.current).map(|e| e.offset)
        } else {
            None
        }
    }

    /// Go to newer position (Ctrl-I).
    ///
    /// Returns the position to jump to, or None if at end.
    pub fn newer(&mut self) -> Option<Offset> {
        if self.current < self.entries.len() {
            self.current += 1;
            self.entries.get(self.current).map(|e| e.offset)
        } else {
            None
        }
    }

    /// Get the current position in the jump list.
    #[inline]
    #[must_use]
    pub const fn position(&self) -> usize {
        self.current
    }

    /// Get the number of entries.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Peek at the entry after navigating `count` steps older, without mutating.
    ///
    /// Used by the executor (read-only state) to compute `SetCursor` target
    /// and detect cross-buffer jumps. Returns the full `JumpEntry` (offset +
    /// buffer_id) if any movement occurred, `None` otherwise.
    ///
    /// Handles partial navigation: if only 2 of 3 requested steps are possible,
    /// returns the entry after 2 steps.
    #[must_use]
    pub fn peek_older(&self, count: u32) -> Option<JumpEntry> {
        let mut pos = self.current;
        for _ in 0..count {
            if pos > 0 {
                pos -= 1;
            } else {
                break;
            }
        }
        if pos < self.current {
            self.entries.get(pos).copied()
        } else {
            None
        }
    }

    /// Peek at the entry after navigating `count` steps newer, without mutating.
    ///
    /// Used by the executor (read-only state) to compute `SetCursor` target
    /// and detect cross-buffer jumps. Returns the full `JumpEntry` (offset +
    /// buffer_id) if any movement occurred, `None` otherwise.
    ///
    /// Mirrors `newer()` semantics: returns `None` when advancing past all entries.
    #[must_use]
    pub fn peek_newer(&self, count: u32) -> Option<JumpEntry> {
        let mut pos = self.current;
        let mut last_valid = None;
        for _ in 0..count {
            if pos < self.entries.len() {
                pos += 1;
                if let Some(entry) = self.entries.get(pos) {
                    last_valid = Some(*entry);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        last_valid
    }

    /// Read-only access to all entries for display (`:jumps`).
    #[inline]
    #[must_use]
    pub const fn entries(&self) -> &VecDeque<JumpEntry> {
        &self.entries
    }

    /// Whether `jumpoptions=stack` is active.
    #[inline]
    #[must_use]
    pub const fn stack_mode(&self) -> bool {
        self.stack_mode
    }

    /// Set `jumpoptions=stack` mode.
    #[inline]
    pub const fn set_stack_mode(&mut self, enabled: bool) {
        self.stack_mode = enabled;
    }

    /// Clear the jump list.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = 0;
    }

    /// Remove all entries associated with the given buffer.
    ///
    /// Adjusts the `current` navigation index: entries removed before
    /// `current` shift it left; if `current` was at the present position
    /// (`== old len`), it stays at the new `len()`.
    pub fn remove_buffer(&mut self, buffer_id: BufferId) {
        let was_at_present = self.current == self.entries.len();
        let removed_before = if was_at_present {
            0
        } else {
            self.entries
                .iter()
                .take(self.current)
                .filter(|e| e.buffer_id() == Some(buffer_id))
                .count()
        };

        self.entries.retain(|e| e.buffer_id() != Some(buffer_id));

        if was_at_present {
            self.current = self.entries.len();
        } else {
            self.current = self.current.saturating_sub(removed_before);
            self.current = self.current.min(self.entries.len());
        }
    }
}

impl super::remap::RemapPositions for JumpList {
    fn remap(&mut self, changeset: &crate::primitives::changeset::ChangeSet) {
        use crate::primitives::changeset::Assoc;

        for entry in &mut self.entries {
            let new_offset = changeset.map_offset(entry.offset, Assoc::After);
            if new_offset != entry.offset {
                entry.offset = new_offset;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_older() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(0), None);
        jl.push(Offset::new(100), None);
        jl.push(Offset::new(200), None);

        // Go back through history
        assert_eq!(jl.older().unwrap().get(), 200);
        assert_eq!(jl.older().unwrap().get(), 100);
        assert_eq!(jl.older().unwrap().get(), 0);
        assert!(jl.older().is_none()); // At start
    }

    #[test]
    fn test_newer() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(0), None);
        jl.push(Offset::new(100), None);
        jl.push(Offset::new(200), None);

        // Go back
        jl.older();
        jl.older();

        // Go forward
        assert_eq!(jl.newer().unwrap().get(), 200);
        // Advancing to "present" — returns None (no entry at present position)
        assert!(jl.newer().is_none());
        // At present now; further newer() is no-op
        assert!(jl.newer().is_none());
    }

    #[test]
    fn test_push_preserves_forward_history() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(0), None);
        jl.push(Offset::new(100), None);
        jl.push(Offset::new(200), None);

        // Go back twice (current goes from 3 → 2 → 1)
        jl.older(); // returns 200, current=2
        jl.older(); // returns 100, current=1

        // Push new position — real Vim never truncates forward history.
        // Entries: [0, 100, 200, 50], current=4 (present)
        jl.push(Offset::new(50), None);

        assert_eq!(jl.len(), 4);

        // Going back should give 50, 200, 100, 0
        assert_eq!(jl.older().unwrap().get(), 50);
        assert_eq!(jl.older().unwrap().get(), 200);
        assert_eq!(jl.older().unwrap().get(), 100);
        assert_eq!(jl.older().unwrap().get(), 0);
        assert!(jl.older().is_none());
    }

    #[test]
    fn test_no_duplicate_entries() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(100), None);
        jl.push(Offset::new(100), None); // Duplicate

        assert_eq!(jl.len(), 1);
    }

    #[test]
    fn test_max_entries() {
        let mut jl = JumpList::new();

        // Push more than max
        for i in 0..=MAX_JUMPS + 10 {
            jl.push(Offset::new(i), None);
        }

        assert_eq!(jl.len(), MAX_JUMPS);
    }

    // ========== FIDELITY TESTS ==========

    /// Test empty jumplist behavior
    #[test]
    fn test_empty_jumplist() {
        let mut jl = JumpList::new();

        assert!(jl.is_empty());
        assert!(jl.older().is_none());
        assert!(jl.newer().is_none());
        assert_eq!(jl.position(), 0);
    }

    /// Test single entry behavior
    #[test]
    fn test_single_entry() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(42), None);

        assert_eq!(jl.len(), 1);
        assert_eq!(jl.older().unwrap().get(), 42);
        assert!(jl.older().is_none()); // Can't go further back
    }

    /// Test position tracking through navigation
    #[test]
    fn test_position_tracking() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(0), None);
        jl.push(Offset::new(100), None);
        jl.push(Offset::new(200), None);

        assert_eq!(jl.position(), 3); // After last push

        jl.older();
        assert_eq!(jl.position(), 2);

        jl.older();
        assert_eq!(jl.position(), 1);

        jl.newer();
        assert_eq!(jl.position(), 2);
    }

    /// Test clear resets everything
    #[test]
    fn test_clear_resets() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(0), None);
        jl.push(Offset::new(100), None);
        jl.older();

        jl.clear();

        assert!(jl.is_empty());
        assert_eq!(jl.position(), 0);
        assert!(jl.older().is_none());
    }

    /// Test consecutive pushes without navigation
    #[test]
    fn test_consecutive_pushes() {
        let mut jl = JumpList::new();

        for i in 0..10 {
            jl.push(Offset::new(i * 100), None);
        }

        assert_eq!(jl.len(), 10);
        assert_eq!(jl.position(), 10);

        // Should be able to navigate back through all 10
        for _ in 0..10 {
            assert!(jl.older().is_some());
        }
        assert!(jl.older().is_none());
    }

    /// Test that newer at present position returns None
    #[test]
    fn test_newer_at_end() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(100), None);
        // Position is at end (present) — newer is no-op
        assert!(jl.newer().is_none());
        // Position didn't change (already at len())
        assert_eq!(jl.position(), 1);
    }

    /// Test zigzag navigation
    #[test]
    fn test_zigzag_navigation() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(0), None);
        jl.push(Offset::new(100), None);
        jl.push(Offset::new(200), None);

        // Position starts at 3 (after all pushes)
        // older() decrements and returns entry at new position
        assert_eq!(jl.older().unwrap().get(), 200); // position 2
        assert_eq!(jl.older().unwrap().get(), 100); // position 1

        // newer() increments and returns entry at new position
        assert_eq!(jl.newer().unwrap().get(), 200); // position 2

        // Advance to "present" (position 3) — returns None (no entry)
        assert!(jl.newer().is_none()); // position 3

        // KEY: Ctrl-O from present goes to most recent entry (200), not 100
        assert_eq!(jl.older().unwrap().get(), 200); // position 2
        assert_eq!(jl.older().unwrap().get(), 100); // position 1
        assert_eq!(jl.older().unwrap().get(), 0); // position 0
    }

    /// Test push from middle position preserves all entries
    #[test]
    fn test_push_from_middle() {
        let mut jl = JumpList::new();

        jl.push(Offset::new(0), None);
        jl.push(Offset::new(100), None);
        jl.push(Offset::new(200), None);
        jl.push(Offset::new(300), None);

        // Go back to position 2
        jl.older(); // at 3 - returns 300
        jl.older(); // at 2 - returns 200

        // Push new entry — appends without truncating forward history
        // entries: [0, 100, 200, 300, 999], current=5
        jl.push(Offset::new(999), None);

        assert_eq!(jl.len(), 5);

        // Navigate back through all entries
        assert_eq!(jl.older().unwrap().get(), 999);
        assert_eq!(jl.older().unwrap().get(), 300);
        assert_eq!(jl.older().unwrap().get(), 200);
        assert_eq!(jl.older().unwrap().get(), 100);
        assert_eq!(jl.older().unwrap().get(), 0);
        assert!(jl.older().is_none());
    }

    /// Test exact MAX_JUMPS entries
    #[test]
    fn test_exact_max_entries() {
        let mut jl = JumpList::new();

        for i in 0..MAX_JUMPS {
            jl.push(Offset::new(i), None);
        }

        assert_eq!(jl.len(), MAX_JUMPS);

        // Push one more - should evict oldest
        jl.push(Offset::new(9999), None);
        assert_eq!(jl.len(), MAX_JUMPS);

        // Navigate to oldest - should not be 0
        for _ in 0..MAX_JUMPS {
            jl.older();
        }
        // First entry (0) should have been evicted
    }

    // ========== CROSS-BUFFER TESTS ==========

    #[test]
    fn cross_buffer_entries_stored() {
        let mut jl = JumpList::new();
        let buf_a = BufferId::new(1);
        let buf_b = BufferId::new(2);

        jl.push(Offset::new(10), Some(buf_a));
        jl.push(Offset::new(20), Some(buf_b));

        let entry = jl.peek_older(1).unwrap();
        assert_eq!(entry.offset().get(), 20);
        assert_eq!(entry.buffer_id(), Some(buf_b));

        let entry = jl.peek_older(2).unwrap();
        assert_eq!(entry.offset().get(), 10);
        assert_eq!(entry.buffer_id(), Some(buf_a));
    }

    #[test]
    fn cross_buffer_dedup_same_buffer_same_offset() {
        let mut jl = JumpList::new();
        let buf = BufferId::new(1);

        jl.push(Offset::new(50), Some(buf));
        jl.push(Offset::new(50), Some(buf)); // Duplicate

        assert_eq!(jl.len(), 1);
    }

    #[test]
    fn cross_buffer_no_dedup_different_buffers_same_offset() {
        let mut jl = JumpList::new();
        let buf_a = BufferId::new(1);
        let buf_b = BufferId::new(2);

        jl.push(Offset::new(50), Some(buf_a));
        jl.push(Offset::new(50), Some(buf_b)); // Different buffer — not a duplicate

        assert_eq!(jl.len(), 2);
    }

    #[test]
    fn peek_returns_full_entry() {
        let mut jl = JumpList::new();
        let buf = BufferId::new(42);

        jl.push(Offset::new(10), Some(buf));
        jl.push(Offset::new(20), None);
        jl.push(Offset::new(30), Some(buf));

        // peek_older returns full JumpEntry
        let entry = jl.peek_older(1).unwrap();
        assert_eq!(entry.offset().get(), 30);
        assert_eq!(entry.buffer_id(), Some(buf));

        let entry = jl.peek_older(2).unwrap();
        assert_eq!(entry.offset().get(), 20);
        assert_eq!(entry.buffer_id(), None);

        // peek_newer after navigating
        jl.older(); // go to position 2
        jl.older(); // go to position 1
        let entry = jl.peek_newer(1).unwrap();
        assert_eq!(entry.offset().get(), 30);
        assert_eq!(entry.buffer_id(), Some(buf));
    }

    #[test]
    fn mixed_buffer_navigation() {
        let mut jl = JumpList::new();
        let buf_a = BufferId::new(1);
        let buf_b = BufferId::new(2);

        jl.push(Offset::new(10), Some(buf_a));
        jl.push(Offset::new(100), Some(buf_b));
        jl.push(Offset::new(20), Some(buf_a));

        // Navigate back: should see buf_a:20, buf_b:100, buf_a:10
        assert_eq!(jl.older().unwrap().get(), 20);
        assert_eq!(jl.older().unwrap().get(), 100);
        assert_eq!(jl.older().unwrap().get(), 10);
        assert!(jl.older().is_none());
    }

    // ========== REMOVE BUFFER TESTS ==========

    #[test]
    fn remove_buffer_basic() {
        let mut jl = JumpList::new();
        let buf_a = BufferId::new(1);
        let buf_b = BufferId::new(2);
        jl.push(Offset::new(10), Some(buf_a));
        jl.push(Offset::new(20), Some(buf_b));
        jl.push(Offset::new(30), Some(buf_a));

        jl.remove_buffer(buf_a);

        assert_eq!(jl.len(), 1);
        assert_eq!(jl.entries()[0].offset(), Offset::new(20));
        assert_eq!(jl.entries()[0].buffer_id(), Some(buf_b));
    }

    #[test]
    fn remove_buffer_adjusts_current_before() {
        let mut jl = JumpList::new();
        let buf_a = BufferId::new(1);
        let buf_b = BufferId::new(2);
        jl.push(Offset::new(10), Some(buf_a));
        jl.push(Offset::new(20), Some(buf_a));
        jl.push(Offset::new(30), Some(buf_b));

        jl.older();
        jl.older();

        jl.remove_buffer(buf_a);

        assert_eq!(jl.len(), 1);
        assert_eq!(jl.position(), 0);
    }

    #[test]
    fn remove_buffer_not_found_is_noop() {
        let mut jl = JumpList::new();
        jl.push(Offset::new(10), Some(BufferId::new(1)));
        jl.push(Offset::new(20), Some(BufferId::new(1)));

        let before_len = jl.len();
        jl.remove_buffer(BufferId::new(99));

        assert_eq!(jl.len(), before_len);
    }

    #[test]
    fn remove_buffer_all_entries() {
        let mut jl = JumpList::new();
        let buf = BufferId::new(1);
        jl.push(Offset::new(10), Some(buf));
        jl.push(Offset::new(20), Some(buf));

        jl.remove_buffer(buf);

        assert!(jl.is_empty());
        assert_eq!(jl.position(), 0);
        assert!(jl.older().is_none());
        assert!(jl.newer().is_none());
    }

    #[test]
    fn remove_buffer_current_at_present() {
        let mut jl = JumpList::new();
        let buf_a = BufferId::new(1);
        let buf_b = BufferId::new(2);
        jl.push(Offset::new(10), Some(buf_a));
        jl.push(Offset::new(20), Some(buf_b));

        jl.remove_buffer(buf_a);

        assert_eq!(jl.len(), 1);
        assert_eq!(jl.position(), jl.len());
    }

    #[test]
    fn remove_buffer_navigate_after() {
        let mut jl = JumpList::new();
        let buf_a = BufferId::new(1);
        let buf_b = BufferId::new(2);
        jl.push(Offset::new(10), Some(buf_b));
        jl.push(Offset::new(20), Some(buf_a));
        jl.push(Offset::new(30), Some(buf_b));

        jl.remove_buffer(buf_a);

        assert_eq!(jl.len(), 2);
        let older = jl.older();
        assert!(older.is_some());
    }

    // ========== JUMPLIST TOPLINE OFFSET TESTS ==========

    #[test]
    fn task_5_5_push_with_topline_stores_offset() {
        let mut jl = JumpList::new();
        jl.push_with_topline(Offset::new(100), None, Some(5));

        let entry = jl.peek_older(1).unwrap();
        assert_eq!(entry.offset().get(), 100);
        assert_eq!(entry.topline_offset(), Some(5));
    }

    #[test]
    fn task_5_5_push_without_topline_has_none() {
        let mut jl = JumpList::new();
        jl.push(Offset::new(100), None);

        let entry = jl.peek_older(1).unwrap();
        assert_eq!(entry.topline_offset(), None);
    }

    #[test]
    fn task_5_5_topline_preserved_through_navigation() {
        let mut jl = JumpList::new();
        jl.push_with_topline(Offset::new(10), None, Some(2));
        jl.push_with_topline(Offset::new(20), None, Some(7));
        jl.push_with_topline(Offset::new(30), None, Some(3));

        // Navigate back and check topline is preserved.
        let entry = jl.peek_older(1).unwrap();
        assert_eq!(entry.topline_offset(), Some(3));

        let entry = jl.peek_older(2).unwrap();
        assert_eq!(entry.topline_offset(), Some(7));

        let entry = jl.peek_older(3).unwrap();
        assert_eq!(entry.topline_offset(), Some(2));
    }

    // ========== JUMPOPTIONS=STACK TESTS ==========

    #[test]
    fn task_5_6_stack_mode_truncates_forward() {
        // Jump A→B→C, Ctrl-O to B, jump to D → forward entry C gone.
        let mut jl = JumpList::new();
        jl.set_stack_mode(true);

        jl.push(Offset::new(10), None); // A
        jl.push(Offset::new(20), None); // B
        jl.push(Offset::new(30), None); // C

        // Ctrl-O to B (position moves from 3 → 2)
        jl.older(); // returns C

        // Jump to D — should truncate C from forward history.
        jl.push(Offset::new(40), None); // D

        // Forward entry C should be gone. Only A, B, D remain.
        assert_eq!(jl.len(), 3);
        assert_eq!(jl.older().unwrap().get(), 40); // D
        assert_eq!(jl.older().unwrap().get(), 20); // B
        assert_eq!(jl.older().unwrap().get(), 10); // A
        assert!(jl.older().is_none());
    }

    #[test]
    fn task_5_6_non_stack_mode_preserves_forward() {
        // Without stack mode, forward entries are preserved (Vim default).
        let mut jl = JumpList::new();
        // stack_mode is false by default.
        assert!(!jl.stack_mode());

        jl.push(Offset::new(10), None); // A
        jl.push(Offset::new(20), None); // B
        jl.push(Offset::new(30), None); // C

        jl.older(); // Ctrl-O to C
        jl.older(); // Ctrl-O to B

        // Push new — all entries preserved (Vim behavior).
        jl.push(Offset::new(40), None); // D

        assert_eq!(jl.len(), 4); // A, B, C, D
    }

    #[test]
    fn task_5_6_stack_mode_at_end_no_truncation() {
        // When already at present (current == len), no truncation occurs.
        let mut jl = JumpList::new();
        jl.set_stack_mode(true);

        jl.push(Offset::new(10), None);
        jl.push(Offset::new(20), None);
        jl.push(Offset::new(30), None);

        assert_eq!(jl.len(), 3);
    }

    // ========== checkpcmark() TESTS ==========

    #[test]
    fn task_5_7_push_checked_same_offset_dedup() {
        let mut jl = JumpList::new();
        // Same offset => suppressed
        jl.push_checked(Offset::new(100), None, |_| 5);
        jl.push_checked(Offset::new(100), None, |_| 5);
        assert_eq!(jl.len(), 1);
    }

    #[test]
    fn task_5_7_push_checked_same_line_dedup() {
        let mut jl = JumpList::new();
        // Offsets differ but same line => suppressed
        jl.push_checked(
            Offset::new(10),
            None,
            |off| {
                if off.get() <= 20 {
                    3
                } else {
                    99
                }
            },
        );
        jl.push_checked(
            Offset::new(15),
            None,
            |off| {
                if off.get() <= 20 {
                    3
                } else {
                    99
                }
            },
        );
        assert_eq!(jl.len(), 1);
    }

    #[test]
    fn task_5_7_push_checked_adjacent_line_dedup() {
        let mut jl = JumpList::new();
        // Line 5 and line 6 => within 1 line => suppressed
        jl.push_checked(
            Offset::new(50),
            None,
            |off| {
                if off.get() == 50 {
                    5
                } else {
                    6
                }
            },
        );
        jl.push_checked(
            Offset::new(70),
            None,
            |off| {
                if off.get() == 50 {
                    5
                } else {
                    6
                }
            },
        );
        assert_eq!(jl.len(), 1);
    }

    #[test]
    fn task_5_7_push_checked_two_lines_apart_not_dedup() {
        let mut jl = JumpList::new();
        // Line 5 and line 7 => diff=2 > 1 => NOT suppressed
        jl.push_checked(
            Offset::new(50),
            None,
            |off| {
                if off.get() == 50 {
                    5
                } else {
                    7
                }
            },
        );
        jl.push_checked(
            Offset::new(100),
            None,
            |off| {
                if off.get() == 50 {
                    5
                } else {
                    7
                }
            },
        );
        assert_eq!(jl.len(), 2);
    }

    #[test]
    fn task_5_7_push_checked_different_buffer_no_dedup() {
        let mut jl = JumpList::new();
        let buf_a = BufferId::new(1);
        let buf_b = BufferId::new(2);
        // Same line but different buffer => NOT suppressed
        jl.push_checked(Offset::new(10), Some(buf_a), |_| 3);
        jl.push_checked(Offset::new(15), Some(buf_b), |_| 3);
        assert_eq!(jl.len(), 2);
    }

    #[test]
    fn task_5_7_multiple_jumps_same_location_one_entry() {
        let mut jl = JumpList::new();
        // Push same location 5 times => only one entry
        for _ in 0..5 {
            jl.push_checked(Offset::new(42), None, |_| 10);
        }
        assert_eq!(jl.len(), 1);
    }
}
