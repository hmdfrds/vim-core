// Regex cursor adapter: a VimTextCursor that traverses tree leaves yielding
// byte slices, compatible with the regex-cursor crate's Cursor interface.
//
// When regex-cursor is added as a dependency, implement their trait via
// blanket impl on RopeCursor.

use crate::chunk::TextChunk;
use crate::summary::ByteOffset;
use crate::tree::traits::Item;
use crate::tree::{Bias, Cursor};
use crate::VimText;

/// Trait matching the regex-cursor crate's Cursor interface.
/// When regex-cursor is added as a dependency, implement their trait via blanket impl.
pub trait RopeCursor {
    /// Returns the byte slice of the current chunk (clipped to range boundaries).
    fn chunk(&self) -> &[u8];

    /// Whether the underlying data is valid UTF-8.
    fn utf8_aware(&self) -> bool;

    /// Advance to the next chunk. Returns false when exhausted.
    fn advance(&mut self) -> bool;

    /// Retreat to the previous chunk. Returns false when at start.
    fn backtrack(&mut self) -> bool;

    /// Total byte count in the traversal range.
    fn total_bytes(&self) -> Option<usize>;

    /// Current byte offset relative to range_start.
    fn offset(&self) -> usize;
}

/// Cursor for traversing VimText chunks, compatible with regex-cursor.
///
/// Yields byte slices from tree leaves, clipped to a byte range.
/// Initial seek is O(log n) via the tree cursor; advance/backtrack are O(1) amortized.
pub struct VimTextCursor<'a> {
    cursor: Cursor<'a, TextChunk, ByteOffset>,
    range_start: usize,
    range_end: usize,
    current_start: usize,
    exhausted: bool,
}

impl<'a> VimTextCursor<'a> {
    /// Create a cursor spanning the entire document.
    pub fn new(tree: &'a VimText) -> Self {
        let total = tree.byte_len();
        Self::new_range(tree, 0, total)
    }

    /// Create a cursor spanning the byte range `[start, end)`.
    pub fn new_range(tree: &'a VimText, start: usize, end: usize) -> Self {
        let mut cursor = tree.tree.cursor::<ByteOffset>();
        if start < end {
            cursor.seek(&ByteOffset(start as u32), Bias::Right);
        }
        let current_start = if cursor.item().is_some() {
            cursor.start::<ByteOffset>().0 as usize
        } else {
            start
        };
        Self {
            cursor,
            range_start: start,
            range_end: end,
            current_start,
            exhausted: start >= end,
        }
    }
}

impl<'a> RopeCursor for VimTextCursor<'a> {
    fn chunk(&self) -> &[u8] {
        if self.exhausted {
            return &[];
        }
        if let Some(item) = self.cursor.item() {
            let chunk_bytes = item.as_bytes();
            let chunk_start = self.current_start;
            // Clip to range boundaries within this chunk.
            let local_start = self.range_start.saturating_sub(chunk_start);
            let local_end = (self.range_end - chunk_start).min(chunk_bytes.len());
            if local_start < local_end {
                &chunk_bytes[local_start..local_end]
            } else {
                &[]
            }
        } else {
            &[]
        }
    }

    fn utf8_aware(&self) -> bool {
        true
    }

    fn advance(&mut self) -> bool {
        if self.exhausted {
            return false;
        }
        if let Some(item) = self.cursor.item() {
            self.current_start += item.len();
        }
        if self.current_start >= self.range_end {
            self.exhausted = true;
            return false;
        }
        if !self.cursor.next() {
            self.exhausted = true;
            return false;
        }
        if self.cursor.item().is_some() {
            self.current_start = self.cursor.start::<ByteOffset>().0 as usize;
            true
        } else {
            self.exhausted = true;
            false
        }
    }

    fn backtrack(&mut self) -> bool {
        if self.current_start <= self.range_start {
            return false;
        }
        // When exhaustion was caused by a range check (current_start >= range_end),
        // the tree cursor still points at the last valid chunk -- it was never moved.
        // Un-exhaust and stay on the current chunk without calling prev().
        if self.exhausted && self.cursor.item().is_some() {
            self.exhausted = false;
            self.current_start = self.cursor.start::<ByteOffset>().0 as usize;
            return true;
        }
        if !self.cursor.prev() {
            return false;
        }
        self.exhausted = false;
        if self.cursor.item().is_some() {
            self.current_start = self.cursor.start::<ByteOffset>().0 as usize;
            true
        } else {
            false
        }
    }

    fn total_bytes(&self) -> Option<usize> {
        Some(self.range_end - self.range_start)
    }

    fn offset(&self) -> usize {
        if self.exhausted {
            return self.range_end - self.range_start;
        }
        self.current_start.saturating_sub(self.range_start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_forward_traversal() {
        let t = VimText::from_str("hello world");
        let cursor = VimTextCursor::new(&t);
        assert!(!cursor.chunk().is_empty());
        assert_eq!(cursor.total_bytes(), Some(11));
        assert_eq!(cursor.offset(), 0);
    }

    #[test]
    fn advance_through_chunks() {
        let text: String = "x".repeat(2000); // spans multiple chunks
        let t = VimText::from_str(&text);
        let mut cursor = VimTextCursor::new(&t);
        let mut total = cursor.chunk().len();
        while cursor.advance() {
            total += cursor.chunk().len();
        }
        assert_eq!(total, 2000);
    }

    #[test]
    fn backtrack_works() {
        let text: String = "x".repeat(2000);
        let t = VimText::from_str(&text);
        let mut cursor = VimTextCursor::new(&t);
        cursor.advance(); // move to second chunk
        assert!(cursor.backtrack()); // back to first
        assert_eq!(cursor.offset(), 0);
    }

    #[test]
    fn range_cursor() {
        let t = VimText::from_str("hello world foo");
        let cursor = VimTextCursor::new_range(&t, 6, 11);
        assert_eq!(cursor.total_bytes(), Some(5));
        let chunk = std::str::from_utf8(cursor.chunk()).unwrap();
        assert!(chunk.contains("world"));
    }

    #[test]
    fn utf8_aware_is_true() {
        let t = VimText::from_str("hello");
        let cursor = VimTextCursor::new(&t);
        assert!(cursor.utf8_aware());
    }

    #[test]
    fn empty_document() {
        let t = VimText::new();
        let cursor = VimTextCursor::new(&t);
        assert_eq!(cursor.chunk(), &[] as &[u8]);
        assert_eq!(cursor.total_bytes(), Some(0));
    }

    #[test]
    fn advance_past_end_returns_false() {
        let t = VimText::from_str("hi");
        let mut cursor = VimTextCursor::new(&t);
        while cursor.advance() {}
        assert!(!cursor.advance());
        assert!(!cursor.advance()); // fused
    }

    #[test]
    fn backtrack_at_start_returns_false() {
        let t = VimText::from_str("hello");
        let mut cursor = VimTextCursor::new(&t);
        assert!(!cursor.backtrack());
    }

    #[test]
    fn full_content_via_chunks() {
        let t = VimText::from_str("hello world");
        let mut cursor = VimTextCursor::new(&t);
        let mut content = Vec::new();
        loop {
            content.extend_from_slice(cursor.chunk());
            if !cursor.advance() {
                break;
            }
        }
        assert_eq!(std::str::from_utf8(&content).unwrap(), "hello world");
    }

    #[test]
    fn full_content_large_via_chunks() {
        let text: String = "abcdefghij".repeat(300); // 3000 bytes, multiple chunks
        let t = VimText::from_str(&text);
        let mut cursor = VimTextCursor::new(&t);
        let mut content = Vec::new();
        loop {
            content.extend_from_slice(cursor.chunk());
            if !cursor.advance() {
                break;
            }
        }
        assert_eq!(std::str::from_utf8(&content).unwrap(), text);
    }

    #[test]
    fn range_cursor_content() {
        let t = VimText::from_str("hello world foo");
        let mut cursor = VimTextCursor::new_range(&t, 6, 11);
        let mut content = Vec::new();
        loop {
            content.extend_from_slice(cursor.chunk());
            if !cursor.advance() {
                break;
            }
        }
        assert_eq!(std::str::from_utf8(&content).unwrap(), "world");
    }

    #[test]
    fn range_cursor_large_text() {
        let text: String = "x".repeat(3000);
        let t = VimText::from_str(&text);
        // Take a range that spans chunk boundaries.
        let mut cursor = VimTextCursor::new_range(&t, 500, 2500);
        let mut total = 0;
        loop {
            total += cursor.chunk().len();
            if !cursor.advance() {
                break;
            }
        }
        assert_eq!(total, 2000);
    }

    #[test]
    fn offset_tracks_position() {
        let text: String = "x".repeat(2000);
        let t = VimText::from_str(&text);
        let mut cursor = VimTextCursor::new(&t);
        let first_offset = cursor.offset();
        assert_eq!(first_offset, 0);
        let first_len = cursor.chunk().len();
        cursor.advance();
        assert_eq!(cursor.offset(), first_len);
    }

    #[test]
    fn offset_at_exhaustion_equals_total() {
        let t = VimText::from_str("hello world");
        let mut cursor = VimTextCursor::new(&t);
        while cursor.advance() {}
        assert_eq!(cursor.offset(), cursor.total_bytes().unwrap());
    }

    #[test]
    fn empty_range_yields_empty() {
        let t = VimText::from_str("hello");
        let cursor = VimTextCursor::new_range(&t, 3, 3);
        assert_eq!(cursor.chunk(), &[] as &[u8]);
        assert_eq!(cursor.total_bytes(), Some(0));
    }

    #[test]
    fn unicode_content_preserved() {
        let text = "hello \u{1F600} world \u{4E16}\u{754C}";
        let t = VimText::from_str(text);
        let mut cursor = VimTextCursor::new(&t);
        let mut content = Vec::new();
        loop {
            content.extend_from_slice(cursor.chunk());
            if !cursor.advance() {
                break;
            }
        }
        assert_eq!(std::str::from_utf8(&content).unwrap(), text);
    }

    #[test]
    fn backtrack_after_full_traversal() {
        let text: String = "x".repeat(2000);
        let t = VimText::from_str(&text);
        let mut cursor = VimTextCursor::new(&t);
        // Advance to the end.
        while cursor.advance() {}
        // Backtrack should work.
        assert!(cursor.backtrack());
        assert!(!cursor.chunk().is_empty());
    }

    #[test]
    fn backtrack_returns_last_chunk_after_exhaustion() {
        let text: String = "x".repeat(2000);
        let t = VimText::from_str(&text);
        let mut cursor = VimTextCursor::new(&t);

        // Record the last chunk before exhaustion
        let mut last_chunk;
        loop {
            last_chunk = cursor.chunk().to_vec();
            if !cursor.advance() {
                break;
            }
        }

        // Backtrack should return to the LAST chunk
        assert!(cursor.backtrack());
        assert_eq!(cursor.chunk(), &last_chunk[..]);
    }
}
