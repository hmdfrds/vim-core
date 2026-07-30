//! In-memory mutable `Document` for shadow execution.
//!
//! `OwnedDocument` wraps a `String` + `LineIndex` and implements the
//! `Document` trait. Unlike `SimpleDocument` (which is test-only and
//! recomputes line info on every query), `OwnedDocument` maintains a
//! persistent `LineIndex` for O(1) `line_count` and O(log n) `line_of`.
//!
//! Mutation methods (`apply_insert`, `apply_delete`, `apply_replace`)
//! update both the `String` and the `LineIndex` incrementally.
//!
//! # Usage
//!
//! The shadow execution loop creates an `OwnedDocument` from the host's
//! current text, then replays macro keys against it without host
//! round-trips. Position translation uses the same `commands::helpers`
//! functions as `SimpleDocument`.

use crate::commands::helpers;
use crate::document::Document;
use crate::primitives::{LineNumber, Offset, Position};

use memchr::memchr_iter;

// ═══════════════════════════════════════════════════════════════════════════════
// LineIndex
// ═══════════════════════════════════════════════════════════════════════════════

/// Byte-offset index of line starts within a document.
///
/// `line_starts[i]` is the byte offset of line `i`'s first character.
/// `line_starts[0]` is always 0.
///
/// Provides O(1) `line_count`, O(log n) `line_of` (via binary search),
/// and O(1) `line_start`. Mutations splice/shift entries incrementally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a `LineIndex` by scanning `text` for newlines.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for offset in memchr_iter(b'\n', text.as_bytes()) {
            line_starts.push(offset + 1);
        }
        Self { line_starts }
    }

    /// Number of lines in the index.
    #[must_use]
    pub const fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Find which line contains `offset` (0-indexed).
    ///
    /// Uses `partition_point` (binary search) for O(log n) lookup.
    #[must_use]
    pub fn line_of(&self, offset: usize) -> usize {
        // partition_point returns the first index where line_starts[i] > offset.
        // Subtracting 1 gives the line that contains `offset`.
        self.line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
    }

    /// Byte offset of the start of `line` (0-indexed). O(1).
    #[must_use]
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line).copied()
    }

    /// Update the index after inserting `text` at `offset`.
    ///
    /// Scans the inserted text for newlines, splices new entries into
    /// `line_starts`, and shifts all subsequent entries by the byte delta.
    pub fn update_insert(&mut self, offset: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let byte_delta = text.len();
        let line = self.line_of(offset);

        // Collect byte offsets of new line starts introduced by inserted newlines.
        let new_starts: Vec<usize> = memchr_iter(b'\n', text.as_bytes())
            .map(|nl_pos| offset + nl_pos + 1)
            .collect();

        let insert_at = line + 1;

        // Splice new line-start entries into the vec.
        self.line_starts
            .splice(insert_at..insert_at, new_starts.iter().copied());

        // Shift all entries after the insertion region by byte_delta.
        let shift_from = insert_at + new_starts.len();
        if let Some(tail) = self.line_starts.get_mut(shift_from..) {
            for start in tail {
                *start += byte_delta;
            }
        }
    }

    /// Update the index after deleting bytes in `start..end`.
    ///
    /// Removes entries for lines whose starts fall within the deleted range,
    /// then shifts all subsequent entries by the negative byte delta.
    pub fn update_delete(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        let byte_delta = end - start;

        // Remove line-start entries in `(start, end]`. The line containing
        // `start` survives (its entry is `<= start`). Any entry `> start`
        // and `<= end` corresponds to a line that was merged or destroyed
        // by the deletion.
        let remove_from = self.line_starts.partition_point(|&s| s <= start);
        // Entries at s == end are also removed: if a line starts at exactly
        // the exclusive end boundary, the newline preceding it (at end-1) was
        // within the deleted range, so this line merges into the line containing
        // `start`.
        let remove_to = self.line_starts.partition_point(|&s| s <= end);

        self.line_starts.drain(remove_from..remove_to);

        // Shift all entries after the deletion point by -byte_delta.
        if let Some(tail) = self.line_starts.get_mut(remove_from..) {
            for s in tail {
                *s -= byte_delta;
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OwnedDocument
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory mutable document backed by `String` + `LineIndex`.
///
/// Implements the `Document` trait for read access and provides
/// mutation methods for the shadow execution loop.
#[derive(Debug, Clone)]
pub struct OwnedDocument {
    text: String,
    line_index: LineIndex,
}

/// Backward-compatibility alias. External consumers (e.g. vim-wasm) may still
/// reference `ShadowDocument`; this alias keeps them compiling without changes.
pub type ShadowDocument = OwnedDocument;

impl OwnedDocument {
    /// Create an `OwnedDocument` from text.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let line_index = LineIndex::new(&text);
        Self { text, line_index }
    }

    /// Snap a byte offset to the nearest char boundary within our text.
    fn snap(&self, offset: usize) -> usize {
        crate::primitives::text_util::snap_to_char_boundary(&self.text, offset)
    }

    /// Insert `text` at byte `offset`.
    ///
    /// Clamps and snaps `offset` so stale or mid-char offsets from the
    /// engine never cause a panic during shadow execution.
    pub fn apply_insert(&mut self, offset: usize, text: &str) {
        let offset = self.snap(offset.min(self.text.len()));
        self.text.insert_str(offset, text);
        self.line_index.update_insert(offset, text);
    }

    /// Delete bytes in `start..end`.
    ///
    /// Clamps and snaps both bounds so out-of-range or mid-char values
    /// degrade to a no-op rather than panicking during shadow execution.
    pub fn apply_delete(&mut self, start: usize, end: usize) {
        let len = self.text.len();
        let start = self.snap(start.min(len));
        let end = self.snap(end.min(len));
        if start >= end {
            return;
        }
        self.text.drain(start..end);
        self.line_index.update_delete(start, end);
    }

    /// Replace bytes in `start..end` with `text`.
    ///
    /// Clamps and snaps both bounds so stale or mid-char offsets from the
    /// engine never cause a panic during shadow execution.
    pub fn apply_replace(&mut self, start: usize, end: usize, text: &str) {
        let len = self.text.len();
        let start = self.snap(start.min(len));
        let end = self.snap(end.min(len));
        if start < end {
            self.text.drain(start..end);
            self.line_index.update_delete(start, end);
        }
        self.text.insert_str(start, text);
        self.line_index.update_insert(start, text);
    }

    /// Access the underlying `LineIndex` for direct queries.
    #[must_use]
    pub const fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    /// Get line content without trailing newline.
    #[must_use]
    pub fn line(&self, n: LineNumber) -> Option<&str> {
        let idx = n.get();
        let start = self.line_index.line_start(idx)?;
        let end = self
            .line_index
            .line_start(idx + 1)
            .map_or(self.text.len(), |s| s.saturating_sub(1));
        if start > self.text.len() || end > self.text.len() || start > end {
            return None;
        }
        Some(&self.text[start..end])
    }

    /// Find which line contains `offset`. Delegates to `LineIndex::line_of`.
    #[must_use]
    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.line_index.line_of(offset)
    }

    /// Byte offset of the start of line `n`. Returns typed `Offset`.
    #[must_use]
    pub fn line_start_offset(&self, n: LineNumber) -> Option<Offset> {
        self.line_index.line_start(n.get()).map(Offset::new)
    }

    /// Replace the entire document text, rebuilding the line index.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.line_index = LineIndex::new(&self.text);
    }
}

impl Document for OwnedDocument {
    /// Get the full document text.
    fn text(&self) -> &str {
        &self.text
    }

    /// Returns the number of lines in the document.
    ///
    /// Matches Vim semantics: an empty buffer has 1 line.
    fn line_count(&self) -> usize {
        let raw = self.line_index.line_count();
        // If text ends with \n, LineIndex has one extra entry for the phantom
        // empty line. Vim treats trailing \n as a terminator, not separator.
        if self.text.ends_with('\n') && raw > 1 {
            (raw - 1).max(1)
        } else {
            raw.max(1)
        }
    }

    /// Convert byte offset to Position (line, column).
    ///
    /// Uses `LineIndex::line_of` for O(log n) line lookup and
    /// `helpers::column_of` for byte-based column computation.
    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        let off = offset.get();
        if off > self.text.len() {
            return None;
        }
        let line = self.line_index.line_of(off);
        let col = helpers::column_of(&self.text, off);
        Some(Position::from_raw(line, col))
    }

    /// Convert Position to byte offset.
    ///
    /// Uses `LineIndex::line_start` for O(1) line-start lookup, then
    /// walks graphemes manually to find the column's byte offset.
    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        let line_start = self.line_index.line_start(pos.line().get())?;
        let line_end = self
            .line_index
            .line_start(pos.line().get() + 1)
            .map_or(self.text.len(), |s| s.saturating_sub(1));
        let line_len = line_end - line_start;

        // Column is byte offset within line (matches Neovim)
        let col = pos.col().get().min(line_len);
        Some(Offset::new(line_start + col))
    }

    #[inline]
    fn line_of_offset(&self, offset: usize) -> usize {
        self.line_index.line_of(offset)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "shadow_document_tests.rs"]
mod tests;
