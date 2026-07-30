use std::borrow::Cow;
use std::iter::FusedIterator;

use crate::chunk::TextChunk;
use crate::summary::{ByteOffset, LineOffset};
use crate::tree::{Bias, Cursor, SumTree};

/// Zero-copy borrowed view into a byte range of a VimText.
/// Supports reading operations without allocation (until `to_string` is called).
///
/// Borrows the underlying `SumTree<TextChunk>` directly, decoupled from VimText.
pub struct RopeSlice<'a> {
    tree: &'a SumTree<TextChunk>,
    start: usize, // byte offset (inclusive)
    end: usize,   // byte offset (exclusive)
}

impl<'a> RopeSlice<'a> {
    pub(crate) fn new(tree: &'a SumTree<TextChunk>, start: usize, end: usize) -> Self {
        assert!(start <= end, "RopeSlice: start > end");
        assert!(
            end <= tree.len::<ByteOffset>().0 as usize,
            "RopeSlice: end > byte_len"
        );
        Self { tree, start, end }
    }

    pub fn byte_len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Count newlines in the slice to determine line count.
    /// An empty slice or a slice with no newlines has 1 line.
    ///
    /// Uses the tree's `LineOffset` dimension for O(log n) computation.
    /// Two cursor seeks + two boundary chunk scans = O(log n + CHUNK_MAX).
    pub fn line_count(&self) -> usize {
        if self.start >= self.end {
            return 1;
        }
        let start_newlines = self.count_newlines_to(self.start);
        let end_newlines = self.count_newlines_to(self.end);
        (end_newlines - start_newlines) + 1
    }

    /// Count the total number of newlines in `[0, byte_offset)`.
    ///
    /// Uses `LineOffset` prefix sums from the tree (O(log n) for the seek)
    /// plus a linear scan of the boundary chunk fragment.
    fn count_newlines_to(&self, byte_offset: usize) -> usize {
        if byte_offset == 0 {
            return 0;
        }
        // Seek with Bias::Left to land on the chunk containing byte_offset - 1.
        // This avoids the at_end case when byte_offset == total tree length.
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(byte_offset as u32), Bias::Left);
        // Newlines from tree summaries up to (but not including) the current chunk
        let prefix_newlines = cursor.start::<LineOffset>().0 as usize;
        // Newlines within the boundary chunk up to byte_offset
        let boundary_newlines = if let Some(chunk) = cursor.item() {
            let chunk_start = cursor.start::<ByteOffset>().0 as usize;
            let local_end = byte_offset.saturating_sub(chunk_start);
            memchr::memchr_iter(b'\n', &chunk.as_bytes()[..local_end]).count()
        } else {
            0
        };
        prefix_newlines + boundary_newlines
    }

    /// Materialize the slice as a `Cow<str>`.
    ///
    /// If the slice falls within a single chunk, returns a zero-copy borrow.
    /// Otherwise allocates a String.
    pub fn to_cow(&self) -> Cow<'a, str> {
        if self.is_empty() {
            return Cow::Borrowed("");
        }

        // Try to get a zero-copy borrow if the slice is within a single chunk.
        // Use Bias::Right so that at chunk boundaries we land on the chunk
        // whose start == self.start, not the previous chunk whose end == self.start.
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(self.start as u32), Bias::Right);

        if let Some(chunk) = cursor.item() {
            let chunk_start_byte = cursor.start::<ByteOffset>().0 as usize;
            let chunk_end_byte = cursor.end::<ByteOffset>().0 as usize;

            // If the entire slice is within this one chunk, borrow directly.
            if self.start >= chunk_start_byte && self.end <= chunk_end_byte {
                let local_start = self.start - chunk_start_byte;
                let local_end = self.end - chunk_start_byte;
                return Cow::Borrowed(&chunk.as_str()[local_start..local_end]);
            }
        }

        // Otherwise, allocate via Display.
        Cow::Owned(std::fmt::format(format_args!("{}", self)))
    }

    /// Iterate over `&str` chunks within this slice range.
    ///
    /// Each yielded item is a sub-slice of a tree leaf, clipped to the slice
    /// boundaries. Implements `DoubleEndedIterator` and `FusedIterator`.
    ///
    /// Stores two cursors (forward and backward) for O(1) amortized iteration.
    pub fn chunks(&self) -> SliceChunks<'a> {
        let mut fwd_cursor = self.tree.cursor::<ByteOffset>();
        let mut bwd_cursor = self.tree.cursor::<ByteOffset>();

        if self.start < self.end {
            fwd_cursor.seek(&ByteOffset(self.start as u32), Bias::Right);
            // Seek bwd_cursor to the chunk containing the last byte (end - 1).
            bwd_cursor.seek(&ByteOffset((self.end - 1) as u32), Bias::Left);
        }
        // If empty (start == end), cursors stay at initial position; fwd_pos >= bwd_pos
        // will cause immediate None from both next() and next_back().

        SliceChunks {
            fwd_cursor,
            bwd_cursor,
            fwd_pos: self.start,
            bwd_pos: self.end,
            start: self.start,
            end: self.end,
        }
    }

    /// Iterate over individual `char`s within this slice range.
    ///
    /// Implements `DoubleEndedIterator` and `FusedIterator`.
    pub fn chars(&self) -> SliceChars<'a> {
        SliceChars {
            chunks: self.chunks(),
            current_fwd: "",
            fwd_lo: 0,
            fwd_hi: 0,
            current_bwd: "",
            bwd_lo: 0,
            bwd_hi: 0,
        }
    }

    /// Iterate over lines within this slice range.
    ///
    /// Each yielded item is a `Cow<str>`: single-chunk lines are zero-copy
    /// borrows, cross-chunk lines are allocated. Lines are yielded without
    /// trailing `\n`.
    ///
    /// Implements `DoubleEndedIterator`, `ExactSizeIterator`, and `FusedIterator`.
    pub fn lines(&self) -> SliceLines<'a> {
        let total_lines = self.line_count();
        let start_line = if self.start == 0 {
            0u32
        } else {
            self.count_newlines_to(self.start) as u32
        };
        SliceLines {
            tree: self.tree,
            start: self.start,
            end: self.end,
            start_line,
            fwd_line: 0,
            bwd_line: total_lines,
            total_lines,
        }
    }
}

impl std::fmt::Display for RopeSlice<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return Ok(());
        }
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(self.start as u32), Bias::Left);
        let mut pos = self.start;
        while let Some(chunk) = cursor.item() {
            let chunk_str = chunk.as_str();
            let chunk_start = cursor.start::<ByteOffset>().0 as usize;
            let chunk_end = chunk_start + chunk_str.len();
            let local_start = pos.saturating_sub(chunk_start);
            let local_end = (self.end - chunk_start).min(chunk_str.len());
            if local_start < local_end {
                f.write_str(&chunk_str[local_start..local_end])?;
            }
            pos = chunk_end;
            if pos >= self.end {
                break;
            }
            if !cursor.next() {
                break;
            }
        }
        Ok(())
    }
}

// =========================================================================
// SliceChunks — DoubleEndedIterator over &str chunks
// =========================================================================

/// Iterator over `&str` chunks within a [`RopeSlice`].
///
/// Each yielded item is a sub-slice of a tree leaf, clipped to the slice
/// boundaries. Implements [`DoubleEndedIterator`] via dual cursor tracking:
/// `fwd_cursor` and `bwd_cursor` converge from opposite ends.
///
/// Stores two cursors for O(1) amortized iteration: `next()` reads from
/// `fwd_cursor` then calls `fwd_cursor.next()`, and `next_back()` reads
/// from `bwd_cursor` then calls `bwd_cursor.prev()`.
pub struct SliceChunks<'a> {
    fwd_cursor: Cursor<'a, TextChunk, ByteOffset>,
    bwd_cursor: Cursor<'a, TextChunk, ByteOffset>,
    fwd_pos: usize,
    bwd_pos: usize,
    start: usize,
    end: usize,
}

impl<'a> Iterator for SliceChunks<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.fwd_pos >= self.bwd_pos {
            return None;
        }

        let chunk = self.fwd_cursor.item()?;
        let chunk_str = chunk.as_str();
        let chunk_start = self.fwd_cursor.start::<ByteOffset>().0 as usize;
        let chunk_end = chunk_start + chunk_str.len();

        // Clip to [start, end) and the backward boundary
        let clip_start = self.fwd_pos.max(self.start) - chunk_start;
        let clip_end = chunk_end.min(self.bwd_pos).min(self.end) - chunk_start;

        if clip_start >= clip_end {
            // This chunk contributes nothing
            self.fwd_pos = self.bwd_pos;
            return None;
        }

        let result = &chunk_str[clip_start..clip_end];

        // Advance forward position past the yielded region
        self.fwd_pos = chunk_start + clip_end;

        // Advance the cursor to the next chunk for the subsequent call
        self.fwd_cursor.next();

        Some(result)
    }
}

impl<'a> DoubleEndedIterator for SliceChunks<'a> {
    fn next_back(&mut self) -> Option<&'a str> {
        if self.bwd_pos <= self.fwd_pos {
            return None;
        }

        let chunk = self.bwd_cursor.item()?;
        let chunk_str = chunk.as_str();
        let chunk_start = self.bwd_cursor.start::<ByteOffset>().0 as usize;
        let chunk_end = chunk_start + chunk_str.len();

        // Clip to [start, end) and the forward boundary
        let clip_start = chunk_start.max(self.fwd_pos).max(self.start) - chunk_start;
        let clip_end = self.bwd_pos.min(chunk_end).min(self.end) - chunk_start;

        if clip_start >= clip_end {
            self.bwd_pos = self.fwd_pos;
            return None;
        }

        let result = &chunk_str[clip_start..clip_end];

        // Retreat backward position to the start of the clipped region
        self.bwd_pos = chunk_start + clip_start;

        // Move the cursor to the previous chunk for the subsequent call
        self.bwd_cursor.prev();

        Some(result)
    }
}

impl FusedIterator for SliceChunks<'_> {}

// =========================================================================
// SliceChars — DoubleEndedIterator over chars
// =========================================================================

/// Iterator over `char`s within a [`RopeSlice`].
///
/// Wraps [`SliceChunks`] for efficient per-char access. Maintains separate
/// forward and backward chunk buffers with byte-offset tracking.
///
/// When chunks are exhausted from one direction, the iterator converges into
/// the other direction's buffer — the same pattern used by the full-doc
/// [`Chars`](crate::iter::Chars) iterator.
///
/// Implements [`DoubleEndedIterator`] and [`FusedIterator`].
pub struct SliceChars<'a> {
    chunks: SliceChunks<'a>,

    // Forward state: current chunk and byte offsets of unconsumed region
    current_fwd: &'a str,
    fwd_lo: usize,
    fwd_hi: usize,

    // Backward state: analogous to forward but populated by next_back
    current_bwd: &'a str,
    bwd_lo: usize,
    bwd_hi: usize,
}

impl<'a> Iterator for SliceChars<'a> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        // 1. Try the forward buffer
        if self.fwd_lo < self.fwd_hi {
            let remaining = &self.current_fwd[self.fwd_lo..self.fwd_hi];
            let ch = remaining.chars().next().unwrap();
            self.fwd_lo += ch.len_utf8();
            return Some(ch);
        }

        // 2. Try to get a new forward chunk
        if let Some(chunk_str) = self.chunks.next() {
            self.current_fwd = chunk_str;
            self.fwd_lo = 0;
            self.fwd_hi = chunk_str.len();
            if !chunk_str.is_empty() {
                let ch = chunk_str.chars().next().unwrap();
                self.fwd_lo = ch.len_utf8();
                return Some(ch);
            }
        }

        // 3. Chunks exhausted from front — try the backward buffer
        // (convergence: next_back loaded a chunk that still has unconsumed chars)
        if self.bwd_lo < self.bwd_hi {
            let remaining = &self.current_bwd[self.bwd_lo..self.bwd_hi];
            let ch = remaining.chars().next().unwrap();
            self.bwd_lo += ch.len_utf8();
            return Some(ch);
        }

        None
    }
}

impl DoubleEndedIterator for SliceChars<'_> {
    fn next_back(&mut self) -> Option<char> {
        // 1. Try the backward buffer
        if self.bwd_lo < self.bwd_hi {
            let remaining = &self.current_bwd[self.bwd_lo..self.bwd_hi];
            let ch = remaining.chars().next_back().unwrap();
            self.bwd_hi -= ch.len_utf8();
            return Some(ch);
        }

        // 2. Try to get a new backward chunk
        if let Some(chunk_str) = self.chunks.next_back() {
            self.current_bwd = chunk_str;
            self.bwd_lo = 0;
            self.bwd_hi = chunk_str.len();
            if !chunk_str.is_empty() {
                let ch = chunk_str.chars().next_back().unwrap();
                self.bwd_hi -= ch.len_utf8();
                return Some(ch);
            }
        }

        // 3. Chunks exhausted from back — try the forward buffer
        // (convergence: the forward buffer may have chars that next_back should yield)
        if self.fwd_lo < self.fwd_hi {
            let remaining = &self.current_fwd[self.fwd_lo..self.fwd_hi];
            let ch = remaining.chars().next_back().unwrap();
            self.fwd_hi -= ch.len_utf8();
            return Some(ch);
        }

        None
    }
}

impl FusedIterator for SliceChars<'_> {}

// =========================================================================
// SliceLines — DoubleEndedIterator + ExactSizeIterator over lines
// =========================================================================

/// Iterator over lines within a [`RopeSlice`].
///
/// Yields `Cow<'a, str>` for each line (without trailing `\n`):
/// - Single-chunk lines yield `Cow::Borrowed` (zero-copy)
/// - Cross-chunk lines yield `Cow::Owned` (allocated)
///
/// Uses index-based approach: tracks forward/backward line indices and
/// computes byte ranges for each line on demand.
///
/// Implements [`DoubleEndedIterator`], [`ExactSizeIterator`], and [`FusedIterator`].
pub struct SliceLines<'a> {
    tree: &'a SumTree<TextChunk>,
    start: usize,
    end: usize,
    start_line: u32, // absolute line index: newline count in [0, self.start)
    fwd_line: usize,
    bwd_line: usize, // exclusive upper bound: next_back yields bwd_line - 1
    total_lines: usize,
}

impl<'a> SliceLines<'a> {
    /// Compute the byte offset of the start of `line_idx` within the slice.
    /// `line_idx` is 0-indexed relative to the slice.
    ///
    /// Uses the tree's `LineOffset` dimension for O(log n) seeking instead of
    /// linear newline scanning. The absolute line we want is `start_line + line_idx`,
    /// and we seek by `LineOffset` to find the chunk containing that newline.
    fn line_start_byte(&self, line_idx: usize) -> usize {
        if line_idx == 0 {
            return self.start;
        }

        // The absolute line we want is start_line + line_idx
        let abs_line = self.start_line + line_idx as u32;

        // Seek to the chunk containing the abs_line-th newline
        let mut cursor = self.tree.cursor::<LineOffset>();
        cursor.seek(&LineOffset(abs_line), Bias::Left);

        // cursor.start::<LineOffset>() = number of newlines before this chunk
        // cursor.start::<ByteOffset>() = byte offset of this chunk's start
        let chunk_byte_start = cursor.start::<ByteOffset>().0 as usize;
        let prefix_newlines = cursor.start::<LineOffset>().0 as usize;
        let remaining = abs_line as usize - prefix_newlines;

        if remaining == 0 {
            return chunk_byte_start;
        }

        // Scan within the chunk for the remaining newlines
        if let Some(chunk) = cursor.item() {
            let chunk_bytes = chunk.as_bytes();
            let mut nl_count = 0;
            for (i, &b) in chunk_bytes.iter().enumerate() {
                if b == b'\n' {
                    nl_count += 1;
                    if nl_count == remaining {
                        return chunk_byte_start + i + 1;
                    }
                }
            }
        }

        self.end
    }

    /// Compute the byte offset of the end of `line_idx` within the slice
    /// (before the `\n`, or at slice end for the last line).
    fn line_end_byte(&self, line_idx: usize) -> usize {
        if line_idx + 1 >= self.total_lines {
            // Last line: goes to the end of the slice
            return self.end;
        }

        // The end of this line is one byte before the start of the next line
        // (the \n character)
        let next_start = self.line_start_byte(line_idx + 1);
        // next_start points to the byte after \n, so the \n is at next_start - 1
        next_start - 1
    }

    /// Extract line content as `Cow<'a, str>`.
    fn get_line(&self, line_idx: usize) -> Cow<'a, str> {
        let line_start = self.line_start_byte(line_idx);
        let line_end = self.line_end_byte(line_idx);

        if line_start >= line_end {
            return Cow::Borrowed("");
        }

        // Seek to the chunk containing `line_start`.
        // Use Bias::Right so that at chunk boundaries we land on the chunk
        // whose start == line_start, not the previous chunk whose end == line_start.
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(line_start as u32), Bias::Right);

        if let Some(chunk) = cursor.item() {
            let chunk_str = chunk.as_str();
            let chunk_start = cursor.start::<ByteOffset>().0 as usize;
            let chunk_end = chunk_start + chunk_str.len();

            // If the entire line fits in this single chunk, borrow
            if line_end <= chunk_end {
                let local_start = line_start - chunk_start;
                let local_end = line_end - chunk_start;
                return Cow::Borrowed(&chunk_str[local_start..local_end]);
            }

            // Line spans multiple chunks: collect into an owned String
            let mut result = String::with_capacity(line_end - line_start);
            let local_start = line_start - chunk_start;
            result.push_str(&chunk_str[local_start..]);

            while cursor.next() {
                if let Some(next_chunk) = cursor.item() {
                    let next_chunk_start = cursor.start::<ByteOffset>().0 as usize;
                    let next_chunk_str = next_chunk.as_str();
                    let next_chunk_end = next_chunk_start + next_chunk_str.len();

                    if line_end <= next_chunk_end {
                        // Final chunk: take only what we need
                        let local_end = line_end - next_chunk_start;
                        result.push_str(&next_chunk_str[..local_end]);
                        break;
                    } else {
                        // Entire chunk is part of the line
                        result.push_str(next_chunk_str);
                    }
                } else {
                    break;
                }
            }

            Cow::Owned(result)
        } else {
            Cow::Borrowed("")
        }
    }
}

impl<'a> Iterator for SliceLines<'a> {
    type Item = Cow<'a, str>;

    fn next(&mut self) -> Option<Cow<'a, str>> {
        if self.fwd_line >= self.bwd_line {
            return None;
        }
        let line = self.fwd_line;
        self.fwd_line += 1;
        Some(self.get_line(line))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.bwd_line.saturating_sub(self.fwd_line);
        (remaining, Some(remaining))
    }
}

impl<'a> DoubleEndedIterator for SliceLines<'a> {
    fn next_back(&mut self) -> Option<Cow<'a, str>> {
        if self.bwd_line <= self.fwd_line {
            return None;
        }
        self.bwd_line -= 1;
        Some(self.get_line(self.bwd_line))
    }
}

impl ExactSizeIterator for SliceLines<'_> {
    fn len(&self) -> usize {
        self.bwd_line.saturating_sub(self.fwd_line)
    }
}

impl FusedIterator for SliceLines<'_> {}

#[cfg(test)]
mod tests {
    use crate::VimText;
    use std::borrow::Cow;

    #[test]
    fn empty_slice() {
        let t = VimText::from_str("hello");
        let s = t.slice(2..2).unwrap();
        assert_eq!(s.byte_len(), 0);
        assert!(s.is_empty());
        assert_eq!(s.to_string(), "");
        assert_eq!(s.line_count(), 1);
    }

    #[test]
    fn full_slice() {
        let t = VimText::from_str("hello\nworld");
        let s = t.slice(0..11).unwrap();
        assert_eq!(s.byte_len(), 11);
        assert_eq!(s.to_string(), "hello\nworld");
        assert_eq!(s.line_count(), 2);
    }

    #[test]
    fn partial_slice() {
        let t = VimText::from_str("hello\nworld\nfoo");
        let s = t.slice(6..11).unwrap();
        assert_eq!(s.to_string(), "world");
        assert_eq!(s.line_count(), 1);
    }

    #[test]
    fn slice_across_chunks() {
        // Create a large text that spans multiple chunks
        let text: String = "abcdefghij\n".repeat(200); // 2200 bytes, ~2 chunks
        let t = VimText::from_str(&text);
        let s = t.slice(500..1500).unwrap();
        assert_eq!(s.byte_len(), 1000);
        assert_eq!(s.to_string(), &text[500..1500]);
    }

    #[test]
    fn slice_out_of_bounds() {
        let t = VimText::from_str("hello");
        assert!(t.slice(0..6).is_none());
        // Inverted range (intentionally reversed for testing bounds checking).
        #[allow(clippy::reversed_empty_ranges)]
        let inverted = 3..2;
        assert!(t.slice(inverted).is_none());
    }

    #[test]
    fn slice_display() {
        let t = VimText::from_str("hello world");
        let s = t.slice(0..5).unwrap();
        assert_eq!(format!("{}", s), "hello");
    }

    #[test]
    fn slice_with_newlines() {
        let t = VimText::from_str("line1\nline2\nline3");
        let s = t.slice(0..11).unwrap(); // "line1\nline2"
        assert_eq!(s.line_count(), 2);
        assert_eq!(s.to_string(), "line1\nline2");
    }

    // --- SliceChunks DoubleEndedIterator tests ---

    #[test]
    fn slice_chunks_forward() {
        let text: String = "abcdefghij\n".repeat(200);
        let t = VimText::from_str(&text);
        let s = t.slice(500..1500).unwrap();
        let collected: String = s.chunks().collect();
        assert_eq!(collected, &text[500..1500]);
    }

    #[test]
    fn slice_chunks_backward() {
        let text: String = "abcdefghij\n".repeat(200);
        let t = VimText::from_str(&text);
        let s = t.slice(500..1500).unwrap();
        let rev_chunks: Vec<&str> = s.chunks().rev().collect();
        let mut reversed = String::new();
        for chunk in rev_chunks.into_iter().rev() {
            reversed.push_str(chunk);
        }
        assert_eq!(reversed, &text[500..1500]);
    }

    #[test]
    fn slice_chunks_double_ended_meet() {
        let t = VimText::from_str("hello world");
        let s = t.slice(0..11).unwrap();
        let mut iter = s.chunks();

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

        let mut full = String::new();
        for c in &front_chunks {
            full.push_str(c);
        }
        for c in back_chunks.iter().rev() {
            full.push_str(c);
        }
        assert_eq!(full, "hello world");
    }

    #[test]
    fn slice_chunks_fused() {
        let t = VimText::from_str("hello");
        let s = t.slice(0..5).unwrap();
        let mut iter = s.chunks();
        while iter.next().is_some() {}
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
        assert!(iter.next_back().is_none());
    }

    // --- SliceChars DoubleEndedIterator tests ---

    #[test]
    fn slice_chars_forward() {
        let t = VimText::from_str("hello world");
        let s = t.slice(0..5).unwrap();
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars, vec!['h', 'e', 'l', 'l', 'o']);
    }

    #[test]
    fn slice_chars_backward() {
        let t = VimText::from_str("hello world");
        let s = t.slice(0..5).unwrap();
        let rev_chars: Vec<char> = s.chars().rev().collect();
        assert_eq!(rev_chars, vec!['o', 'l', 'l', 'e', 'h']);
    }

    #[test]
    fn slice_chars_unicode() {
        let t = VimText::from_str("a\u{1F600}b");
        let s = t.slice(0..6).unwrap(); // full text
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars, vec!['a', '\u{1F600}', 'b']);
        let rev_chars: Vec<char> = s.chars().rev().collect();
        assert_eq!(rev_chars, vec!['b', '\u{1F600}', 'a']);
    }

    #[test]
    fn slice_chars_double_ended_meet() {
        let t = VimText::from_str("abcdef");
        let s = t.slice(0..6).unwrap();
        let mut iter = s.chars();
        assert_eq!(iter.next(), Some('a'));
        assert_eq!(iter.next_back(), Some('f'));
        assert_eq!(iter.next(), Some('b'));
        assert_eq!(iter.next_back(), Some('e'));
        assert_eq!(iter.next(), Some('c'));
        assert_eq!(iter.next_back(), Some('d'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    // --- SliceLines DoubleEndedIterator + ExactSizeIterator tests ---

    #[test]
    fn slice_lines_basic() {
        let t = VimText::from_str("hello\nworld\nfoo");
        let s = t.slice(0..15).unwrap();
        let lines: Vec<Cow<str>> = s.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "hello");
        assert_eq!(lines[1], "world");
        assert_eq!(lines[2], "foo");
    }

    #[test]
    fn slice_lines_backward() {
        let t = VimText::from_str("hello\nworld\nfoo");
        let s = t.slice(0..15).unwrap();
        let rev_lines: Vec<Cow<str>> = s.lines().rev().collect();
        assert_eq!(rev_lines.len(), 3);
        assert_eq!(rev_lines[0], "foo");
        assert_eq!(rev_lines[1], "world");
        assert_eq!(rev_lines[2], "hello");
    }

    #[test]
    fn slice_lines_double_ended_meet() {
        let t = VimText::from_str("a\nb\nc\nd");
        let s = t.slice(0..7).unwrap();
        let mut iter = s.lines();
        assert_eq!(iter.next().as_deref(), Some("a"));
        assert_eq!(iter.next_back().as_deref(), Some("d"));
        assert_eq!(iter.next().as_deref(), Some("b"));
        assert_eq!(iter.next_back().as_deref(), Some("c"));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn slice_lines_exact_size() {
        let t = VimText::from_str("a\nb\nc\nd");
        let s = t.slice(0..7).unwrap();
        let iter = s.lines();
        assert_eq!(iter.len(), 4);
    }

    #[test]
    fn slice_lines_empty() {
        let t = VimText::from_str("hello");
        let s = t.slice(2..2).unwrap();
        let lines: Vec<Cow<str>> = s.lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "");
    }

    #[test]
    fn slice_lines_trailing_newline() {
        let t = VimText::from_str("hello\n");
        let s = t.slice(0..6).unwrap();
        let lines: Vec<Cow<str>> = s.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "hello");
        assert_eq!(lines[1], "");
    }

    #[test]
    fn slice_lines_partial() {
        let t = VimText::from_str("hello\nworld\nfoo");
        let s = t.slice(6..11).unwrap(); // "world"
        let lines: Vec<Cow<str>> = s.lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "world");
    }

    #[test]
    fn slice_lines_single_chunk_borrows() {
        // A small text fits in one chunk, so all lines should be Borrowed
        let t = VimText::from_str("hello\nworld");
        let s = t.slice(0..11).unwrap();
        for line in s.lines() {
            assert!(
                matches!(line, Cow::Borrowed(_)),
                "expected Borrowed, got Owned for {:?}",
                line
            );
        }
    }

    #[test]
    fn slice_lines_size_hint_accurate() {
        let t = VimText::from_str("a\nb\nc");
        let s = t.slice(0..5).unwrap();
        let mut iter = s.lines();
        assert_eq!(iter.size_hint(), (3, Some(3)));
        iter.next();
        assert_eq!(iter.size_hint(), (2, Some(2)));
        iter.next_back();
        assert_eq!(iter.size_hint(), (1, Some(1)));
        iter.next();
        assert_eq!(iter.size_hint(), (0, Some(0)));
    }

    // --- to_cow tests ---

    #[test]
    fn to_cow_empty() {
        let t = VimText::from_str("hello");
        let s = t.slice(2..2).unwrap();
        let cow = s.to_cow();
        assert!(matches!(cow, Cow::Borrowed("")));
    }

    #[test]
    fn to_cow_single_chunk() {
        let t = VimText::from_str("hello");
        let s = t.slice(1..4).unwrap();
        let cow = s.to_cow();
        assert!(matches!(cow, Cow::Borrowed("ell")));
    }

    // --- Multi-chunk DoubleEndedIterator tests ---

    #[test]
    fn slice_chunks_double_ended_multi_chunk() {
        let text: String = "abcdefghij\n".repeat(200); // ~2200 bytes, multiple chunks
        let t = VimText::from_str(&text);
        let s = t.slice(500..1500).unwrap();
        let expected: String = text[500..1500].to_string();

        // Forward
        let fwd: String = s.chunks().collect();
        assert_eq!(fwd, expected);

        // Backward: collect reversed chunks, then reverse back to original order
        let rev_chunks: Vec<&str> = s.chunks().rev().collect();
        let bwd: String = rev_chunks.into_iter().rev().collect();
        assert_eq!(bwd, expected);

        // Interleaved: alternate next()/next_back() on chars across chunk boundaries
        let expected_chars: Vec<char> = expected.chars().collect();
        let mut iter = s.chars();
        let mut lo = 0usize;
        let mut hi = expected_chars.len();
        let mut toggle = true;
        loop {
            if toggle {
                match iter.next() {
                    Some(c) => {
                        assert_eq!(c, expected_chars[lo]);
                        lo += 1;
                    }
                    None => break,
                }
            } else {
                match iter.next_back() {
                    Some(c) => {
                        hi -= 1;
                        assert_eq!(c, expected_chars[hi]);
                    }
                    None => break,
                }
            }
            toggle = !toggle;
        }
        assert_eq!(lo, hi);
    }
}
