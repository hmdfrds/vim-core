use crate::changeset::{ChangeSet, Op};
use crate::chunk::chunk_text;
use crate::iter::{Bytes, Chars, Chunks, Lines};
use crate::rope_slice::RopeSlice;
use crate::summary::{ByteOffset, CharOffset, LineOffset, Utf16Offset};
use crate::tree::Bias;
use crate::VimText;

/// Position in a document (0-indexed line + byte column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl VimText {
    // === Metrics ===

    pub fn byte_len(&self) -> usize {
        let ByteOffset(n) = self.tree.len::<ByteOffset>();
        n as usize
    }

    pub fn char_count(&self) -> usize {
        let CharOffset(n) = self.tree.len::<CharOffset>();
        n as usize
    }

    /// Number of lines in the document. Empty doc has 1 line.
    /// Equal to (number of newlines) + 1.
    pub fn line_count(&self) -> usize {
        let LineOffset(n) = self.tree.len::<LineOffset>();
        (n as usize) + 1
    }

    pub fn is_empty(&self) -> bool {
        self.byte_len() == 0
    }

    // === Line Access ===

    /// Byte offset of the start of line `n` (0-indexed).
    /// Returns None if `n >= line_count`.
    pub fn line_start(&self, line: usize) -> Option<usize> {
        if line == 0 {
            return Some(0);
        }
        if line >= self.line_count() {
            return None;
        }
        // We need the byte position right after the `line`-th newline.
        // Use Bias::Left so the cursor lands on the chunk whose end LineOffset
        // >= target, even when target equals the total tree dimension.
        let mut cursor = self.tree.cursor::<LineOffset>();
        cursor.seek(&LineOffset(line as u32), Bias::Left);

        if let Some(chunk) = cursor.item() {
            let byte_before = cursor.start::<ByteOffset>().0 as usize;
            let lines_before = cursor.start::<LineOffset>().0 as usize;

            // How many newlines within this chunk do we need to skip?
            let remaining = line - lines_before;

            if remaining == 0 {
                // The accumulated newlines before this chunk already equal `line`.
                // The start of this chunk IS the start of line `line`.
                return Some(byte_before);
            }

            // Scan within the chunk to find the `remaining`-th newline.
            let text = chunk.as_str();
            let mut count = 0;
            for (i, b) in text.as_bytes().iter().enumerate() {
                if *b == b'\n' {
                    count += 1;
                    if count == remaining {
                        return Some(byte_before + i + 1);
                    }
                }
            }
        }

        // at_end: the `line`-th newline count was reached at the very end.
        // This means line_start == byte_len (empty trailing line after final \n).
        Some(self.byte_len())
    }

    /// Byte offset of the end of line `n` (before the \n, or at byte_len for last line).
    /// Returns None if `n >= line_count`.
    pub fn line_end(&self, line: usize) -> Option<usize> {
        if line >= self.line_count() {
            return None;
        }
        let next_start = self.line_start(line + 1).unwrap_or(self.byte_len());
        let start = self.line_start(line)?;
        // If the line has a trailing \n, line_end is before it.
        // line_start(line+1) exists => there IS a \n at next_start - 1.
        if next_start > start && line + 1 < self.line_count() {
            Some(next_start - 1)
        } else {
            // Last line (no trailing \n), or same position.
            Some(next_start)
        }
    }

    /// Line number (0-indexed) containing the given byte offset.
    ///
    /// Returns the last line for offsets past the end (clamped).
    /// For an `Option`-returning variant, see [`try_line_of_offset`](Self::try_line_of_offset).
    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.try_line_of_offset(offset)
            .unwrap_or(self.line_count().saturating_sub(1))
    }

    /// Line number (0-indexed) containing the given byte offset.
    /// Returns None if offset > byte_len.
    pub fn try_line_of_offset(&self, offset: usize) -> Option<usize> {
        if offset > self.byte_len() {
            return None;
        }
        if self.is_empty() {
            return Some(0);
        }
        // Use Bias::Left so that when offset == byte_len, we still land on the
        // last chunk rather than going to at_end.
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(offset as u32), Bias::Left);

        if let Some(chunk) = cursor.item() {
            let lines_before = cursor.start::<LineOffset>().0 as usize;
            let byte_before = cursor.start::<ByteOffset>().0 as usize;

            // Count newlines within this chunk, up to the local offset.
            let local_offset = offset - byte_before;
            let text = chunk.as_str();
            let local_newlines = text.as_bytes()[..local_offset]
                .iter()
                .filter(|&&b| b == b'\n')
                .count();
            Some(lines_before + local_newlines)
        } else {
            // at_end: should only happen for empty tree, handled above.
            Some(self.line_count() - 1)
        }
    }

    /// Length of line `n` in bytes (excluding the newline).
    pub fn line_len(&self, line: usize) -> Option<usize> {
        let start = self.line_start(line)?;
        let end = self.line_end(line)?;
        Some(end - start)
    }

    // === Position Conversion ===

    pub fn offset_to_pos(&self, offset: usize) -> Option<Position> {
        if offset > self.byte_len() {
            return None;
        }
        let line = self.try_line_of_offset(offset)?;
        let line_start = self.line_start(line)?;
        Some(Position {
            line,
            col: offset - line_start,
        })
    }

    pub fn pos_to_offset(&self, pos: Position) -> Option<usize> {
        let start = self.line_start(pos.line)?;
        let end = self.line_end(pos.line)?;
        let offset = start + pos.col;
        if offset > end {
            return None;
        }
        Some(offset)
    }

    // === UTF-16 Conversion (for LSP) ===

    pub fn byte_to_utf16(&self, byte_offset: usize) -> Option<usize> {
        if byte_offset > self.byte_len() {
            return None;
        }
        if self.is_empty() {
            return Some(0);
        }
        // Use Bias::Left so offset == byte_len still lands on the last chunk.
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(byte_offset as u32), Bias::Left);

        if let Some(chunk) = cursor.item() {
            let utf16_before = cursor.start::<Utf16Offset>().0 as usize;
            let byte_before = cursor.start::<ByteOffset>().0 as usize;
            let local_offset = byte_offset - byte_before;
            let text = &chunk.as_str()[..local_offset];
            let local_utf16: usize = text.chars().map(|c| c.len_utf16()).sum();
            Some(utf16_before + local_utf16)
        } else {
            // Empty tree handled above.
            let total = self.tree.len::<Utf16Offset>();
            Some(total.0 as usize)
        }
    }

    pub fn utf16_to_byte(&self, utf16_offset: usize) -> Option<usize> {
        let total_utf16 = self.tree.len::<Utf16Offset>();
        if utf16_offset > total_utf16.0 as usize {
            return None;
        }
        if self.is_empty() {
            return Some(0);
        }
        // Use Bias::Left so offset == total_utf16 lands on the last chunk.
        let mut cursor = self.tree.cursor::<Utf16Offset>();
        cursor.seek(&Utf16Offset(utf16_offset as u32), Bias::Left);

        if let Some(chunk) = cursor.item() {
            let byte_before = cursor.start::<ByteOffset>().0 as usize;
            let utf16_before = cursor.start::<Utf16Offset>().0 as usize;
            let remaining = utf16_offset - utf16_before;
            let text = chunk.as_str();
            let mut utf16_count = 0;
            let mut byte_count = 0;
            for c in text.chars() {
                if utf16_count >= remaining {
                    break;
                }
                utf16_count += c.len_utf16();
                byte_count += c.len_utf8();
            }
            Some(byte_before + byte_count)
        } else {
            Some(self.byte_len())
        }
    }

    // === Single-element Access ===

    /// Get the byte at the given offset. O(log n).
    pub fn byte_at(&self, offset: usize) -> Option<u8> {
        if offset >= self.byte_len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(offset as u32), Bias::Right);
        let chunk = cursor.item()?;
        let chunk_start = cursor.start::<ByteOffset>().0 as usize;
        let local = offset - chunk_start;
        chunk.as_bytes().get(local).copied()
    }

    /// Get the character at the given byte offset. O(log n).
    /// Returns None if offset is out of bounds or not a char boundary.
    pub fn char_at(&self, offset: usize) -> Option<char> {
        if offset >= self.byte_len() {
            return None;
        }
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(offset as u32), Bias::Right);
        let chunk = cursor.item()?;
        let chunk_start = cursor.start::<ByteOffset>().0 as usize;
        let local = offset - chunk_start;
        chunk.as_str().get(local..)?.chars().next()
    }

    // === Mutation ===

    /// Apply a ChangeSet to this buffer.
    ///
    /// # Panics
    /// Panics if `changes.src_len() != self.byte_len()`.
    pub fn apply(&mut self, changes: &ChangeSet) {
        assert_eq!(
            changes.src_len(),
            self.byte_len(),
            "ChangeSet src_len ({}) != buffer byte_len ({})",
            changes.src_len(),
            self.byte_len()
        );

        // Walk ops, applying inserts and deletes to the tree.
        // After a Retain, offset advances (content is unchanged).
        // After a Delete, the tree shrinks (offset does NOT advance since
        // subsequent content shifted left to current offset).
        // After an Insert, the tree grows and offset advances by inserted length.
        let mut offset: usize = 0;
        for op in changes.ops() {
            match op {
                Op::Retain(n) => {
                    offset += *n as usize;
                }
                Op::Delete(n) => {
                    let n = *n as usize;
                    self.tree.delete(offset..offset + n);
                    // offset stays the same — content after the deleted region
                    // is now at `offset`.
                }
                Op::Insert(text) => {
                    let chunks = chunk_text(text);
                    // Insert chunks in reverse so they end up in the correct order.
                    // Each chunk is inserted at the same offset, pushing previous
                    // insertions to the right.
                    for chunk in chunks.into_iter().rev() {
                        self.tree.insert(offset, chunk);
                    }
                    offset += text.len();
                }
            }
        }
    }

    // === Snapshot ===

    /// O(1) snapshot via Arc clone.
    pub fn snapshot(&self) -> Self {
        self.clone()
    }

    // === Slicing ===

    /// Get a zero-copy borrowed view into a byte range.
    /// Returns None for out-of-bounds or inverted ranges.
    pub fn slice(&self, range: std::ops::Range<usize>) -> Option<RopeSlice<'_>> {
        if range.start > range.end || range.end > self.byte_len() {
            return None;
        }
        Some(RopeSlice::new(&self.tree, range.start, range.end))
    }

    // === Regex Cursor ===

    /// Create a regex-cursor-compatible cursor spanning the entire document.
    pub fn rope_cursor(&self) -> crate::regex_adapter::VimTextCursor<'_> {
        crate::regex_adapter::VimTextCursor::new(self)
    }

    /// Create a regex-cursor-compatible cursor spanning a byte range.
    pub fn rope_cursor_range(
        &self,
        start: usize,
        end: usize,
    ) -> crate::regex_adapter::VimTextCursor<'_> {
        crate::regex_adapter::VimTextCursor::new_range(self, start, end)
    }

    // === Iterators ===

    /// Iterate over `&str` chunks from tree leaves.
    ///
    /// Each chunk is a zero-copy slice of a tree leaf. For small texts this
    /// yields a single chunk; for large texts it yields multiple chunks of up
    /// to `CHUNK_MAX_BYTES` each.
    ///
    /// Implements `DoubleEndedIterator` and `FusedIterator`.
    pub fn chunks(&self) -> Chunks<'_> {
        Chunks::new(self.tree.root())
    }

    /// Iterate over individual bytes of the buffer.
    ///
    /// Wraps [`chunks()`](Self::chunks) for efficient per-byte access.
    /// Implements `DoubleEndedIterator` and `FusedIterator`.
    pub fn bytes(&self) -> Bytes<'_> {
        Bytes::new(self.chunks())
    }

    /// Iterate over lines of the buffer.
    ///
    /// Yields `Cow<str>` for each line (without trailing `\n`).
    /// Single-chunk lines are zero-copy borrows; cross-chunk lines are allocated.
    /// Implements `DoubleEndedIterator`, `ExactSizeIterator`, and `FusedIterator`.
    pub fn lines(&self) -> Lines<'_> {
        Lines::new(self)
    }

    /// Iterate over individual `char`s of the buffer.
    ///
    /// Wraps [`chunks()`](Self::chunks) for efficient per-char access.
    /// Correctly handles multi-byte UTF-8 characters.
    /// Implements `DoubleEndedIterator` and `FusedIterator`.
    pub fn chars(&self) -> Chars<'_> {
        Chars::new(self.chunks())
    }

    // === Materialization ===

    // to_string() is provided by the Display impl on VimText (see lib.rs).
}

// =========================================================================
// Backward-compatibility shims for vim-core
// =========================================================================

impl VimText {
    // --- Legacy mutation methods (wrap ChangeSet) ---

    /// Legacy compatibility: insert text at offset.
    pub fn apply_insert(&mut self, offset: usize, text: &str) {
        let offset = offset.min(self.byte_len());
        let cs = crate::changeset::ChangeSet::from_changes(
            self.byte_len(),
            std::iter::once(crate::changeset::Change {
                start: offset,
                end: offset,
                text: text.into(),
            }),
        );
        self.apply(&cs);
    }

    /// Legacy compatibility: delete byte range [start, end).
    pub fn apply_delete(&mut self, start: usize, end: usize) {
        let len = self.byte_len();
        let start = start.min(len);
        let end = end.min(len).max(start);
        if start == end {
            return;
        }
        let cs = crate::changeset::ChangeSet::from_changes(
            len,
            std::iter::once(crate::changeset::Change {
                start,
                end,
                text: "".into(),
            }),
        );
        self.apply(&cs);
    }

    /// Legacy compatibility: replace byte range [start, end) with text.
    pub fn apply_replace(&mut self, start: usize, end: usize, text: &str) {
        let len = self.byte_len();
        let start = start.min(len);
        let end = end.min(len).max(start);
        if start == end && text.is_empty() {
            return;
        }
        let cs = crate::changeset::ChangeSet::from_changes(
            len,
            std::iter::once(crate::changeset::Change {
                start,
                end,
                text: text.into(),
            }),
        );
        self.apply(&cs);
    }

    /// Legacy compatibility: replace the entire document content.
    pub fn set_text(&mut self, text: &str) {
        *self = Self::from_str(text);
    }

    // --- Legacy query methods ---

    /// Legacy compatibility: alias for `to_string()`.
    pub fn materialize(&self) -> String {
        self.to_string()
    }

    /// Get the content of line `n` (0-indexed, excluding trailing newline).
    /// Returns `None` if `n >= line_count`.
    pub fn line(&self, n: usize) -> Option<std::borrow::Cow<'_, str>> {
        if n >= self.line_count() {
            return None;
        }
        let start = self.line_start(n)?;
        let end = self.line_end(n)?;
        if start == end {
            return Some(std::borrow::Cow::Borrowed(""));
        }
        // Collect bytes from the range into a string.
        // Use the RopeSlice for efficient access.
        let slice = self.slice(start..end)?;
        Some(slice.to_cow())
    }

    /// Get the tree-level summary.
    ///
    /// Returns a compatibility wrapper that exposes `flags` for
    /// legacy `summary().flags.contains(SummaryFlags::ENDS_WITH_NEWLINE)` access.
    pub fn summary(&self) -> SummaryCompat {
        use crate::summary::IndentFlags;

        let root_summary = self.tree.summary();
        SummaryCompat {
            flags: SummaryFlagsCompat {
                ends_with_newline: root_summary
                    .indent
                    .flags
                    .contains(IndentFlags::ENDS_WITH_NEWLINE),
            },
        }
    }

    /// Find lines whose bloom filter matches the given pattern, filtered to a line range.
    ///
    /// Delegates to `bloom_search_lines` which handles cross-chunk carry buffers
    /// and dedup, then filters to the requested line range.
    pub fn find_matching_lines(
        &self,
        pattern: &str,
        line_range: std::ops::Range<usize>,
    ) -> Vec<usize> {
        self.bloom_search_lines(pattern.as_bytes())
            .into_iter()
            .filter(|&line| line >= line_range.start && line < line_range.end)
            .collect()
    }

    // --- Backward-compatible slice with two args ---

    /// Legacy compatibility: slice with (start, end) instead of Range.
    ///
    /// The old API used `slice(start, end)` returning `Cow<str>`.
    /// The new API uses `slice(start..end)` returning `Option<RopeSlice>`.
    /// This provides the old signature.
    pub fn slice_range(&self, start: usize, end: usize) -> Option<std::borrow::Cow<'_, str>> {
        if start > end || end > self.byte_len() {
            return None;
        }
        let rope_slice = self.slice(start..end)?;
        Some(rope_slice.to_cow())
    }

    // --- VimQueries trait delegation for inherent method access ---
    // These delegate to the VimQueries trait methods so that vim-core
    // callsites don't need `use vim_text::VimQueries`.

    /// Find the next blank line after `line` (exclusive).
    pub fn next_blank_line_from(&self, line: usize) -> Option<usize> {
        <Self as crate::queries::VimQueries>::next_blank_line_from(self, line)
    }

    /// Find the previous blank line before `line` (exclusive).
    pub fn prev_blank_line_from(&self, line: usize) -> Option<usize> {
        <Self as crate::queries::VimQueries>::prev_blank_line_from(self, line)
    }

    /// Find the next non-blank line after `line` (exclusive).
    pub fn next_nonblank_line_from(&self, line: usize) -> Option<usize> {
        <Self as crate::queries::VimQueries>::next_nonblank_line_from(self, line)
    }

    /// Find the previous non-blank line before `line` (exclusive).
    pub fn prev_nonblank_line_from(&self, line: usize) -> Option<usize> {
        <Self as crate::queries::VimQueries>::prev_nonblank_line_from(self, line)
    }

    /// Check if a given line is blank.
    pub fn is_line_blank(&self, line: usize) -> bool {
        <Self as crate::queries::VimQueries>::is_line_blank(self, line)
    }

    /// Check if any line in `[start_line, end_line)` is blank.
    pub fn has_blank_line_in_range(&self, start_line: usize, end_line: usize) -> bool {
        <Self as crate::queries::VimQueries>::has_blank_line_in_range(self, start_line, end_line)
    }

    /// Find minimum indent in a line range.
    pub fn min_indent_in_range(&self, start_line: usize, end_line: usize) -> u16 {
        <Self as crate::queries::VimQueries>::min_indent_in_range(self, start_line, end_line)
    }

    /// Find matching bracket at offset.
    pub fn matching_bracket(&self, offset: usize) -> Option<usize> {
        <Self as crate::queries::VimQueries>::matching_bracket(self, offset)
    }

    /// Return parenthesis nesting depth at offset.
    pub fn bracket_depth_at(&self, offset: usize) -> i16 {
        <Self as crate::queries::VimQueries>::bracket_depth_at(self, offset)
    }

    // --- TextSearch trait delegation ---

    /// Bloom-accelerated line search.
    pub fn bloom_search_lines(&self, pattern: &[u8]) -> Vec<usize> {
        <Self as crate::queries::TextSearch>::bloom_search_lines(self, pattern)
    }

    /// Quick check if document might contain a literal pattern.
    pub fn might_contain_literal(&self, pattern: &[u8]) -> bool {
        <Self as crate::queries::TextSearch>::might_contain_literal(self, pattern)
    }
}

/// Compatibility wrapper for summary flags access.
pub struct SummaryCompat {
    pub flags: SummaryFlagsCompat,
}

/// Compatibility wrapper that mimics the old `SummaryFlags` bitflags API.
pub struct SummaryFlagsCompat {
    ends_with_newline: bool,
}

impl SummaryFlagsCompat {
    /// Check if the `ENDS_WITH_NEWLINE` flag is set.
    /// Mimics the old `flags.contains(SummaryFlags::ENDS_WITH_NEWLINE)` pattern.
    pub fn contains(&self, flag: crate::summary::IndentFlags) -> bool {
        use crate::summary::IndentFlags;
        if flag == IndentFlags::ENDS_WITH_NEWLINE {
            self.ends_with_newline
        } else {
            false
        }
    }
}

// Provide to_string() via Display (see lib.rs).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::Change;
    use crate::VimText;

    #[test]
    fn new_is_empty() {
        let t = VimText::new();
        assert_eq!(t.byte_len(), 0);
        assert_eq!(t.line_count(), 1); // empty doc has 1 line
        assert!(t.is_empty());
    }

    #[test]
    fn from_str_basic() {
        let t = VimText::from_str("hello\nworld");
        assert_eq!(t.byte_len(), 11);
        assert_eq!(t.line_count(), 2);
        assert_eq!(t.char_count(), 11);
    }

    #[test]
    fn from_str_large() {
        let text: String = "line\n".repeat(10000);
        let t = VimText::from_str(&text);
        assert_eq!(t.byte_len(), 50000);
        assert_eq!(t.line_count(), 10001); // 10000 newlines + trailing empty line
    }

    #[test]
    fn line_start_and_end() {
        let t = VimText::from_str("hello\nworld\nfoo");
        assert_eq!(t.line_start(0), Some(0));
        assert_eq!(t.line_start(1), Some(6));
        assert_eq!(t.line_start(2), Some(12));
        assert_eq!(t.line_start(3), None);
        assert_eq!(t.line_end(0), Some(5)); // before \n
        assert_eq!(t.line_end(1), Some(11)); // before \n
        assert_eq!(t.line_end(2), Some(15)); // end of file (no trailing \n)
    }

    #[test]
    fn line_of_offset() {
        let t = VimText::from_str("hello\nworld\nfoo");
        assert_eq!(t.line_of_offset(0), 0);
        assert_eq!(t.line_of_offset(4), 0);
        assert_eq!(t.line_of_offset(5), 0); // the \n itself is on line 0
        assert_eq!(t.line_of_offset(6), 1);
        assert_eq!(t.line_of_offset(12), 2);
        assert_eq!(t.line_of_offset(15), 2);
        assert_eq!(t.line_of_offset(16), 2); // past end, clamped to last line
    }

    #[test]
    fn try_line_of_offset() {
        let t = VimText::from_str("hello\nworld\nfoo");
        assert_eq!(t.try_line_of_offset(0), Some(0));
        assert_eq!(t.try_line_of_offset(6), Some(1));
        assert_eq!(t.try_line_of_offset(15), Some(2));
        assert_eq!(t.try_line_of_offset(16), None); // past end
    }

    #[test]
    fn offset_to_pos_and_back() {
        let t = VimText::from_str("hello\nworld");
        assert_eq!(t.offset_to_pos(0), Some(Position { line: 0, col: 0 }));
        assert_eq!(t.offset_to_pos(3), Some(Position { line: 0, col: 3 }));
        assert_eq!(t.offset_to_pos(6), Some(Position { line: 1, col: 0 }));
        assert_eq!(t.offset_to_pos(9), Some(Position { line: 1, col: 3 }));

        // Round-trip
        for offset in 0..=11 {
            let pos = t.offset_to_pos(offset).unwrap();
            assert_eq!(t.pos_to_offset(pos), Some(offset));
        }
    }

    #[test]
    fn apply_changeset_insert() {
        let mut t = VimText::from_str("hello world");
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 5,
                end: 5,
                text: " beautiful".into(),
            }],
        );
        t.apply(&cs);
        assert_eq!(t.to_string(), "hello beautiful world");
    }

    #[test]
    fn apply_changeset_delete() {
        let mut t = VimText::from_str("hello beautiful world");
        let cs = ChangeSet::from_changes(
            21,
            vec![Change {
                start: 5,
                end: 15,
                text: "".into(),
            }],
        );
        t.apply(&cs);
        assert_eq!(t.to_string(), "hello world");
    }

    #[test]
    fn apply_changeset_replace() {
        let mut t = VimText::from_str("hello world");
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 6,
                end: 11,
                text: "rust".into(),
            }],
        );
        t.apply(&cs);
        assert_eq!(t.to_string(), "hello rust");
    }

    #[test]
    fn snapshot_independence() {
        let mut t = VimText::from_str("hello");
        let snap = t.snapshot();
        let cs = ChangeSet::from_changes(
            5,
            vec![Change {
                start: 5,
                end: 5,
                text: " world".into(),
            }],
        );
        t.apply(&cs);
        assert_eq!(t.to_string(), "hello world");
        assert_eq!(snap.to_string(), "hello");
    }

    #[test]
    fn display_and_to_string() {
        let t = VimText::from_str("hello\nworld");
        assert_eq!(format!("{}", t), "hello\nworld");
        assert_eq!(t.to_string(), "hello\nworld");
    }

    #[test]
    fn partial_eq() {
        let a = VimText::from_str("hello");
        let b = VimText::from_str("hello");
        let c = VimText::from_str("world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Clone shares Arc -- ptr_eq fast path
        let d = a.clone();
        assert_eq!(a, d);
    }

    #[test]
    fn from_impls() {
        let a: VimText = "hello".into();
        let b: VimText = String::from("hello").into();
        assert_eq!(a, b);
    }

    #[test]
    fn line_count_edge_cases() {
        assert_eq!(VimText::from_str("").line_count(), 1);
        assert_eq!(VimText::from_str("\n").line_count(), 2);
        assert_eq!(VimText::from_str("\n\n").line_count(), 3);
        assert_eq!(VimText::from_str("a").line_count(), 1);
        assert_eq!(VimText::from_str("a\n").line_count(), 2);
    }

    #[test]
    fn line_start_trailing_newline() {
        let t = VimText::from_str("hello\n");
        assert_eq!(t.line_count(), 2);
        assert_eq!(t.line_start(0), Some(0));
        assert_eq!(t.line_start(1), Some(6));
        assert_eq!(t.line_start(2), None);
    }

    #[test]
    fn line_end_trailing_newline() {
        let t = VimText::from_str("hello\n");
        assert_eq!(t.line_end(0), Some(5)); // before \n
        assert_eq!(t.line_end(1), Some(6)); // empty last line
    }

    #[test]
    fn line_len_basic() {
        let t = VimText::from_str("hello\nworld\nfoo");
        assert_eq!(t.line_len(0), Some(5));
        assert_eq!(t.line_len(1), Some(5));
        assert_eq!(t.line_len(2), Some(3));
        assert_eq!(t.line_len(3), None);
    }

    #[test]
    fn pos_to_offset_out_of_bounds() {
        let t = VimText::from_str("hello\nworld");
        // col past end of line
        assert_eq!(t.pos_to_offset(Position { line: 0, col: 6 }), None);
        // line past end
        assert_eq!(t.pos_to_offset(Position { line: 5, col: 0 }), None);
    }

    #[test]
    fn utf16_roundtrip_ascii() {
        let t = VimText::from_str("hello\nworld");
        // For ASCII, byte offset == utf16 offset.
        for i in 0..=11 {
            let utf16 = t.byte_to_utf16(i).unwrap();
            assert_eq!(utf16, i);
            let byte = t.utf16_to_byte(utf16).unwrap();
            assert_eq!(byte, i);
        }
    }

    #[test]
    fn utf16_with_emoji() {
        // "a\u{1F600}b" = 'a'(1B, 1 UTF-16) + emoji(4B, 2 UTF-16) + 'b'(1B, 1 UTF-16)
        let t = VimText::from_str("a\u{1F600}b");
        assert_eq!(t.byte_len(), 6);

        assert_eq!(t.byte_to_utf16(0), Some(0)); // before 'a'
        assert_eq!(t.byte_to_utf16(1), Some(1)); // after 'a', before emoji
        assert_eq!(t.byte_to_utf16(5), Some(3)); // after emoji, before 'b'
        assert_eq!(t.byte_to_utf16(6), Some(4)); // after 'b'

        assert_eq!(t.utf16_to_byte(0), Some(0));
        assert_eq!(t.utf16_to_byte(1), Some(1));
        assert_eq!(t.utf16_to_byte(3), Some(5));
        assert_eq!(t.utf16_to_byte(4), Some(6));
    }

    #[test]
    fn large_text_line_access() {
        // Create a large text with known line structure.
        let mut text = String::new();
        for i in 0..1000 {
            text.push_str(&format!("line {}\n", i));
        }
        let t = VimText::from_str(&text);
        assert_eq!(t.line_count(), 1001); // 1000 newlines + trailing empty line

        // Verify first and last accessible lines.
        assert_eq!(t.line_start(0), Some(0));
        assert_eq!(t.line_of_offset(0), 0);

        let last_line = t.line_count() - 1;
        let last_start = t.line_start(last_line).unwrap();
        assert_eq!(last_start, t.byte_len()); // empty trailing line
    }

    #[test]
    fn apply_multiple_changes() {
        let mut t = VimText::from_str("hello world");
        // Replace "hello" with "hi" and "world" with "earth"
        let cs = ChangeSet::from_changes(
            11,
            vec![
                Change {
                    start: 0,
                    end: 5,
                    text: "hi".into(),
                },
                Change {
                    start: 6,
                    end: 11,
                    text: "earth".into(),
                },
            ],
        );
        t.apply(&cs);
        assert_eq!(t.to_string(), "hi earth");
    }

    #[test]
    fn empty_doc_operations() {
        let t = VimText::new();
        assert_eq!(t.line_start(0), Some(0));
        assert_eq!(t.line_end(0), Some(0));
        assert_eq!(t.line_of_offset(0), 0);
        assert_eq!(t.line_len(0), Some(0));
        assert_eq!(t.offset_to_pos(0), Some(Position { line: 0, col: 0 }));
        assert_eq!(t.pos_to_offset(Position { line: 0, col: 0 }), Some(0));
    }

    #[test]
    fn only_newlines() {
        let t = VimText::from_str("\n\n\n");
        assert_eq!(t.line_count(), 4);
        assert_eq!(t.line_start(0), Some(0));
        assert_eq!(t.line_start(1), Some(1));
        assert_eq!(t.line_start(2), Some(2));
        assert_eq!(t.line_start(3), Some(3));
        assert_eq!(t.line_end(0), Some(0)); // \n at offset 0
        assert_eq!(t.line_end(1), Some(1)); // \n at offset 1
        assert_eq!(t.line_end(2), Some(2)); // \n at offset 2
        assert_eq!(t.line_end(3), Some(3)); // empty last line
    }

    #[test]
    fn line_of_offset_at_newlines() {
        let t = VimText::from_str("\n\n\n");
        assert_eq!(t.line_of_offset(0), 0); // the \n at offset 0 is on line 0
        assert_eq!(t.line_of_offset(1), 1); // the \n at offset 1 is on line 1
        assert_eq!(t.line_of_offset(2), 2); // the \n at offset 2 is on line 2
        assert_eq!(t.line_of_offset(3), 3); // at end = empty line 3
    }

    // --- byte_at / char_at tests ---

    #[test]
    fn byte_at_ascii() {
        let t = VimText::from_str("hello");
        assert_eq!(t.byte_at(0), Some(b'h'));
        assert_eq!(t.byte_at(1), Some(b'e'));
        assert_eq!(t.byte_at(4), Some(b'o'));
    }

    #[test]
    fn byte_at_out_of_bounds() {
        let t = VimText::from_str("hello");
        assert_eq!(t.byte_at(5), None);
        assert_eq!(t.byte_at(100), None);

        let empty = VimText::new();
        assert_eq!(empty.byte_at(0), None);
    }

    #[test]
    fn byte_at_unicode() {
        // 'ä' is U+00E4, encoded as [0xC3, 0xA4] in UTF-8.
        let t = VimText::from_str("aäb");
        assert_eq!(t.byte_at(0), Some(b'a'));
        assert_eq!(t.byte_at(1), Some(0xC3));
        assert_eq!(t.byte_at(2), Some(0xA4));
        assert_eq!(t.byte_at(3), Some(b'b'));
    }

    #[test]
    fn byte_at_emoji() {
        // U+1F600 is encoded as [0xF0, 0x9F, 0x98, 0x80] in UTF-8.
        let t = VimText::from_str("a\u{1F600}b");
        assert_eq!(t.byte_at(0), Some(b'a'));
        assert_eq!(t.byte_at(1), Some(0xF0));
        assert_eq!(t.byte_at(2), Some(0x9F));
        assert_eq!(t.byte_at(3), Some(0x98));
        assert_eq!(t.byte_at(4), Some(0x80));
        assert_eq!(t.byte_at(5), Some(b'b'));
        assert_eq!(t.byte_at(6), None);
    }

    #[test]
    fn char_at_ascii() {
        let t = VimText::from_str("hello");
        assert_eq!(t.char_at(0), Some('h'));
        assert_eq!(t.char_at(1), Some('e'));
        assert_eq!(t.char_at(4), Some('o'));
    }

    #[test]
    fn char_at_out_of_bounds() {
        let t = VimText::from_str("hello");
        assert_eq!(t.char_at(5), None);
        assert_eq!(t.char_at(100), None);

        let empty = VimText::new();
        assert_eq!(empty.char_at(0), None);
    }

    #[test]
    fn char_at_unicode() {
        let t = VimText::from_str("aäb");
        assert_eq!(t.char_at(0), Some('a'));
        assert_eq!(t.char_at(1), Some('ä')); // start of 2-byte char
        assert_eq!(t.char_at(2), None); // mid-char boundary: not a valid start
        assert_eq!(t.char_at(3), Some('b'));
    }

    #[test]
    fn char_at_emoji() {
        let t = VimText::from_str("a\u{1F600}b");
        assert_eq!(t.char_at(0), Some('a'));
        assert_eq!(t.char_at(1), Some('\u{1F600}')); // start of 4-byte emoji
        assert_eq!(t.char_at(2), None); // mid-char
        assert_eq!(t.char_at(3), None); // mid-char
        assert_eq!(t.char_at(4), None); // mid-char
        assert_eq!(t.char_at(5), Some('b'));
    }
}
