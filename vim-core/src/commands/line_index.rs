//! Cached line-offset table for O(1) line_start and O(log N) line_of.
//!
//! Built once per process() call on first use, via memchr SIMD scan.
//! Provides O(1) line_start, O(log N) line_of, O(1) line_end.

use memchr::memchr_iter;

/// Cached line-offset table.
#[derive(Debug)]
pub struct LineIndex {
    /// Sorted byte offsets of each line start. offsets[0] = 0 always.
    offsets: Vec<usize>,
}

impl LineIndex {
    /// Build from text. O(N) one-time scan via memchr SIMD.
    #[must_use]
    pub fn build(text: &str) -> Self {
        let mut offsets = Vec::with_capacity(text.len() / 40 + 1);
        offsets.push(0);
        for pos in memchr_iter(b'\n', text.as_bytes()) {
            offsets.push(pos + 1);
        }
        Self { offsets }
    }

    /// O(1) — direct index lookup.
    #[inline]
    #[must_use]
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.offsets.get(line).copied()
    }

    /// O(log N) — binary search over sorted offsets.
    #[inline]
    #[must_use]
    pub fn line_of(&self, offset: usize) -> usize {
        self.offsets
            .partition_point(|&o| o <= offset)
            .saturating_sub(1)
    }

    /// O(1) — end of line = start of next line - 1, or text_len for last line.
    #[inline]
    #[must_use]
    pub fn line_end(&self, line: usize, text_len: usize) -> usize {
        self.offsets
            .get(line + 1)
            .map_or(text_len, |&o| o.saturating_sub(1))
    }

    /// O(1) — number of lines.
    #[inline]
    #[must_use]
    pub const fn line_count(&self) -> usize {
        self.offsets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text() {
        let idx = LineIndex::build("");
        assert_eq!(idx.line_count(), 1);
        assert_eq!(idx.line_start(0), Some(0));
        assert_eq!(idx.line_of(0), 0);
    }

    #[test]
    fn single_line_no_newline() {
        let idx = LineIndex::build("hello");
        assert_eq!(idx.line_count(), 1);
        assert_eq!(idx.line_of(3), 0);
        assert_eq!(idx.line_end(0, 5), 5);
    }

    #[test]
    fn multi_line() {
        let idx = LineIndex::build("abc\ndef\nghi");
        assert_eq!(idx.line_count(), 3);
        assert_eq!(idx.line_start(0), Some(0));
        assert_eq!(idx.line_start(1), Some(4));
        assert_eq!(idx.line_start(2), Some(8));
        assert_eq!(idx.line_of(0), 0);
        assert_eq!(idx.line_of(3), 0); // newline belongs to line 0
        assert_eq!(idx.line_of(4), 1);
        assert_eq!(idx.line_of(5), 1);
        assert_eq!(idx.line_of(8), 2);
        assert_eq!(idx.line_of(9), 2);
        assert_eq!(idx.line_end(0, 11), 3);
        assert_eq!(idx.line_end(1, 11), 7);
        assert_eq!(idx.line_end(2, 11), 11);
    }

    #[test]
    fn trailing_newline() {
        let idx = LineIndex::build("abc\n");
        assert_eq!(idx.line_count(), 2);
        assert_eq!(idx.line_start(1), Some(4));
    }

    #[test]
    fn line_start_out_of_bounds() {
        let idx = LineIndex::build("abc\ndef");
        assert_eq!(idx.line_start(5), None);
    }
}
