use std::iter::FusedIterator;
use std::sync::Arc;

use crate::chunk::TextChunk;
use crate::summary::ByteOffset;
use crate::tree::{Bias, Cursor, Node, DEFAULT_B};

/// Iterator over `&str` chunks of a [`VimText`](crate::VimText).
///
/// Yields zero-copy string slices from tree leaves. Boundary chunks (first and
/// last in a sub-range) are clipped to the requested range.
///
/// Implements [`DoubleEndedIterator`] using two independent cursors that
/// converge from opposite ends. When the forward position meets or passes the
/// backward position, iteration is exhausted from both directions.
///
/// Also implements [`FusedIterator`]: once `None` is returned, all subsequent
/// calls return `None`.
pub struct Chunks<'a> {
    // Forward cursor state
    fwd_cursor: Cursor<'a, TextChunk, ByteOffset, DEFAULT_B>,
    fwd_pos: usize, // byte position of the start of the next forward chunk

    // Backward cursor state
    bwd_cursor: Cursor<'a, TextChunk, ByteOffset, DEFAULT_B>,
    bwd_pos: usize, // byte position just past the end of the next backward chunk

    // Content bounds (for sub-range iteration)
    start: usize,
    end: usize,
}

impl<'a> Chunks<'a> {
    /// Create a `Chunks` iterator over the full content of a tree.
    pub(crate) fn new(root: &'a Arc<Node<TextChunk, DEFAULT_B>>) -> Self {
        let total_bytes = {
            let s = root.summary();
            s.metrics.bytes as usize
        };
        Self::new_range(root, 0, total_bytes)
    }

    /// Create a `Chunks` iterator over a byte sub-range `[start, end)`.
    ///
    /// # Panics
    /// Panics if `start > end`.
    pub(crate) fn new_range(
        root: &'a Arc<Node<TextChunk, DEFAULT_B>>,
        start: usize,
        end: usize,
    ) -> Self {
        assert!(
            start <= end,
            "Chunks::new_range: start ({start}) > end ({end})"
        );

        // Forward cursor: seek to `start` with Bias::Left so that when start
        // falls on a chunk boundary we land on the chunk whose content covers
        // that position.
        let mut fwd_cursor = Cursor::new(root);
        if start > 0 {
            fwd_cursor.seek(&ByteOffset(start as u32), Bias::Left);
        }
        // else: Cursor::new already positions at first leaf

        // Backward cursor: seek to (end - 1) to land on the chunk containing
        // the last byte. If end == 0, the range is empty and we skip.
        let mut bwd_cursor = Cursor::new(root);
        if end > 0 {
            // Seek to end with Bias::Left to land on the chunk whose content
            // includes the byte just before `end`.
            bwd_cursor.seek(&ByteOffset(end as u32), Bias::Left);
        }

        Chunks {
            fwd_cursor,
            fwd_pos: start,
            bwd_cursor,
            bwd_pos: end,
            start,
            end,
        }
    }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.fwd_pos >= self.bwd_pos {
            return None;
        }

        let chunk = self.fwd_cursor.item()?;
        let chunk_str = chunk.as_str();
        let chunk_start = self.fwd_cursor.start::<ByteOffset>().0 as usize;
        let chunk_end = chunk_start + chunk_str.len();

        // Clip to range [start, end) and to the backward boundary
        let clip_start = self.fwd_pos.max(self.start) - chunk_start;
        let clip_end = chunk_end.min(self.bwd_pos).min(self.end) - chunk_start;

        if clip_start >= clip_end {
            // This chunk contributes nothing (empty or past the boundary)
            return None;
        }

        let result = &chunk_str[clip_start..clip_end];

        // Advance forward cursor
        let new_pos = chunk_start + clip_end;
        if !self.fwd_cursor.next() {
            // No more leaves; set fwd_pos to signal exhaustion
            self.fwd_pos = self.bwd_pos;
        } else {
            self.fwd_pos = new_pos;
        }

        Some(result)
    }
}

impl<'a> DoubleEndedIterator for Chunks<'a> {
    fn next_back(&mut self) -> Option<&'a str> {
        if self.bwd_pos <= self.fwd_pos {
            return None;
        }

        let chunk = self.bwd_cursor.item()?;
        let chunk_str = chunk.as_str();
        let chunk_start = self.bwd_cursor.start::<ByteOffset>().0 as usize;
        let chunk_end = chunk_start + chunk_str.len();

        // Clip to range [start, end) and to the forward boundary
        let clip_start = chunk_start.max(self.fwd_pos).max(self.start) - chunk_start;
        let clip_end = self.bwd_pos.min(chunk_end).min(self.end) - chunk_start;

        if clip_start >= clip_end {
            return None;
        }

        let result = &chunk_str[clip_start..clip_end];

        // Retreat backward cursor
        let new_pos = chunk_start + clip_start;
        if !self.bwd_cursor.prev() {
            // No more leaves backward; set bwd_pos to signal exhaustion
            self.bwd_pos = self.fwd_pos;
        } else {
            self.bwd_pos = new_pos;
        }

        Some(result)
    }
}

impl FusedIterator for Chunks<'_> {}

#[cfg(test)]
mod tests {
    use crate::VimText;

    #[test]
    fn forward_iteration_all_chunks() {
        let text = "hello world, this is a test of chunk iteration!";
        let vt = VimText::from_str(text);
        let collected: String = vt.chunks().collect();
        assert_eq!(collected, text);
    }

    #[test]
    fn backward_iteration_all_chunks() {
        let text = "hello world, this is a test of chunk iteration!";
        let vt = VimText::from_str(text);
        // Collect chunks in reverse order, then reverse to check all content is covered
        let rev_chunks: Vec<&str> = vt.chunks().rev().collect();
        let mut reversed = String::new();
        for chunk in rev_chunks.into_iter().rev() {
            reversed.push_str(chunk);
        }
        assert_eq!(reversed, text);
    }

    #[test]
    fn double_ended_meet_in_middle() {
        let text = "hello world";
        let vt = VimText::from_str(text);
        let mut iter = vt.chunks();

        // Alternate front and back
        let mut front_chunks = Vec::new();
        let mut back_chunks = Vec::new();
        let mut toggle = true;

        loop {
            if toggle {
                match iter.next() {
                    Some(c) => front_chunks.push(c.to_string()),
                    None => break,
                }
            } else {
                match iter.next_back() {
                    Some(c) => back_chunks.push(c.to_string()),
                    None => break,
                }
            }
            toggle = !toggle;
        }

        // All content is covered: front_chunks + reversed back_chunks = full text
        let mut full = String::new();
        for c in &front_chunks {
            full.push_str(c);
        }
        for c in back_chunks.iter().rev() {
            full.push_str(c);
        }
        assert_eq!(full, text);
    }

    #[test]
    fn fused_after_exhaustion() {
        let vt = VimText::from_str("hello");
        let mut iter = vt.chunks();

        // Exhaust forward
        while iter.next().is_some() {}

        // Further calls must return None
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
        assert!(iter.next_back().is_none());
        assert!(iter.next_back().is_none());
    }

    #[test]
    fn empty_text() {
        let vt = VimText::new();
        let collected: String = vt.chunks().collect();
        assert_eq!(collected, "");

        // Reverse also empty
        let rev: Vec<&str> = vt.chunks().rev().collect();
        assert!(rev.is_empty() || rev == vec![""]);
    }

    #[test]
    fn single_chunk_text() {
        let vt = VimText::from_str("hello");
        let chunks: Vec<&str> = vt.chunks().collect();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "hello");

        let rev_chunks: Vec<&str> = vt.chunks().rev().collect();
        assert_eq!(rev_chunks.len(), 1);
        assert_eq!(rev_chunks[0], "hello");
    }

    #[test]
    fn large_text_multiple_chunks() {
        // Create text larger than CHUNK_MAX_BYTES (1024) to force multiple tree leaves
        let text: String = "abcdefghij\n".repeat(200); // 2200 bytes, > 1 chunk
        let vt = VimText::from_str(&text);

        // Forward: join all chunks and compare
        let collected: String = vt.chunks().collect();
        assert_eq!(collected, text);

        // Count chunks: must be > 1 for large text
        let chunk_count = vt.chunks().count();
        assert!(
            chunk_count > 1,
            "expected multiple chunks for {} bytes, got {}",
            text.len(),
            chunk_count
        );

        // Backward: collect reversed chunks, un-reverse, compare
        let rev_chunks: Vec<&str> = vt.chunks().rev().collect();
        let mut reversed = String::new();
        for chunk in rev_chunks.into_iter().rev() {
            reversed.push_str(chunk);
        }
        assert_eq!(reversed, text);
    }

    #[test]
    fn forward_then_backward_exhausts_all() {
        // Use large text to get multiple chunks
        let text: String = "line\n".repeat(300); // 1500 bytes
        let vt = VimText::from_str(&text);
        let mut iter = vt.chunks();

        // Take one from front
        let first = iter.next().unwrap();
        // Take one from back
        let last = iter.next_back().unwrap();

        // Collect remaining
        let remaining: String = iter.collect();

        // first + remaining + last == full text
        let mut full = String::new();
        full.push_str(first);
        full.push_str(&remaining);
        full.push_str(last);
        assert_eq!(full, text);
    }

    #[test]
    fn unicode_chunks() {
        // Mix of multi-byte characters
        let text = "\u{1F600}\u{1F601}\u{1F602}\u{1F603}hello\nworld\u{4E16}\u{754C}";
        let vt = VimText::from_str(text);
        let collected: String = vt.chunks().collect();
        assert_eq!(collected, text);

        let rev_collected: String = {
            let rev: Vec<&str> = vt.chunks().rev().collect();
            let mut s = String::new();
            for c in rev.into_iter().rev() {
                s.push_str(c);
            }
            s
        };
        assert_eq!(rev_collected, text);
    }
}
