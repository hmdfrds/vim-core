use std::borrow::Cow;
use std::iter::FusedIterator;

use crate::summary::ByteOffset;
use crate::tree::Bias;
use crate::VimText;

/// Iterator over lines of a [`VimText`].
///
/// Yields `Cow<'a, str>` for each line (without trailing `\n`):
/// - Single-chunk lines yield `Cow::Borrowed` (zero-copy)
/// - Cross-chunk lines yield `Cow::Owned` (allocated)
///
/// Implements [`DoubleEndedIterator`], [`ExactSizeIterator`], and [`FusedIterator`].
///
/// # Line semantics
///
/// - An empty document yields one empty-string line.
/// - A trailing `\n` produces an empty trailing line (e.g. `"hello\n"` -> `["hello", ""]`).
/// - Line content never includes the `\n` delimiter.
pub struct Lines<'a> {
    tree: &'a VimText,
    fwd_line: usize,
    bwd_line: usize, // exclusive upper bound: next_back yields bwd_line - 1
}

impl<'a> Lines<'a> {
    pub(crate) fn new(tree: &'a VimText) -> Self {
        let total_lines = tree.line_count();
        Lines {
            tree,
            fwd_line: 0,
            bwd_line: total_lines,
        }
    }

    /// Extract line content as `Cow<'a, str>`.
    ///
    /// Uses the tree cursor to determine if the line fits within a single chunk
    /// (yielding a zero-copy borrow) or spans multiple chunks (yielding an owned string).
    fn get_line(&self, line: usize) -> Cow<'a, str> {
        let start = self.tree.line_start(line).unwrap_or(self.tree.byte_len());
        let end = self.tree.line_end(line).unwrap_or(self.tree.byte_len());

        if start == end {
            return Cow::Borrowed("");
        }

        // Seek to the chunk containing `start`
        let mut cursor = self.tree.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(start as u32), Bias::Left);

        if let Some(chunk) = cursor.item() {
            let chunk_str = chunk.as_str();
            let chunk_start = cursor.start::<ByteOffset>().0 as usize;
            let chunk_end = chunk_start + chunk_str.len();

            // If the entire line [start, end) fits in this single chunk, borrow
            if end <= chunk_end {
                let local_start = start - chunk_start;
                let local_end = end - chunk_start;
                return Cow::Borrowed(&chunk_str[local_start..local_end]);
            }

            // Line spans multiple chunks: collect into an owned String
            let mut result = String::with_capacity(end - start);
            let local_start = start - chunk_start;
            result.push_str(&chunk_str[local_start..]);

            while cursor.next() {
                if let Some(next_chunk) = cursor.item() {
                    let next_chunk_start = cursor.start::<ByteOffset>().0 as usize;
                    let next_chunk_str = next_chunk.as_str();
                    let next_chunk_end = next_chunk_start + next_chunk_str.len();

                    if end <= next_chunk_end {
                        // Final chunk: take only what we need
                        let local_end = end - next_chunk_start;
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
            // Should not happen for valid line indices, but handle gracefully
            Cow::Borrowed("")
        }
    }
}

impl<'a> Iterator for Lines<'a> {
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

impl<'a> DoubleEndedIterator for Lines<'a> {
    fn next_back(&mut self) -> Option<Cow<'a, str>> {
        if self.bwd_line <= self.fwd_line {
            return None;
        }
        self.bwd_line -= 1;
        Some(self.get_line(self.bwd_line))
    }
}

impl ExactSizeIterator for Lines<'_> {
    fn len(&self) -> usize {
        self.bwd_line.saturating_sub(self.fwd_line)
    }
}

impl FusedIterator for Lines<'_> {}

#[cfg(test)]
mod tests {
    use crate::VimText;
    use std::borrow::Cow;

    #[test]
    fn lines_basic() {
        let vt = VimText::from_str("hello\nworld");
        let lines: Vec<_> = vt.lines().collect();
        assert_eq!(lines, vec!["hello", "world"]);
    }

    #[test]
    fn lines_trailing_newline() {
        let vt = VimText::from_str("hello\n");
        let lines: Vec<_> = vt.lines().collect();
        assert_eq!(lines, vec!["hello", ""]);
    }

    #[test]
    fn lines_empty() {
        let vt = VimText::new();
        let lines: Vec<_> = vt.lines().collect();
        assert_eq!(lines, vec![""]);
    }

    #[test]
    fn lines_only_newlines() {
        let vt = VimText::from_str("\n\n");
        let lines: Vec<_> = vt.lines().collect();
        assert_eq!(lines, vec!["", "", ""]);
    }

    #[test]
    fn lines_backward() {
        let vt = VimText::from_str("hello\nworld\nfoo");
        let rev_lines: Vec<_> = vt.lines().rev().collect();
        assert_eq!(rev_lines, vec!["foo", "world", "hello"]);
    }

    #[test]
    fn lines_exact_size() {
        let vt = VimText::from_str("a\nb\nc\nd");
        let iter = vt.lines();
        assert_eq!(iter.len(), 4);
        assert_eq!(iter.len(), vt.line_count());
    }

    #[test]
    fn lines_double_ended_meet() {
        let vt = VimText::from_str("a\nb\nc\nd");
        let mut iter = vt.lines();

        assert_eq!(iter.next().as_deref(), Some("a"));
        assert_eq!(iter.next_back().as_deref(), Some("d"));
        assert_eq!(iter.next().as_deref(), Some("b"));
        assert_eq!(iter.next_back().as_deref(), Some("c"));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn lines_large_text() {
        // Create text spanning multiple chunks
        let text: String = (0..300)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let vt = VimText::from_str(&text);
        let lines: Vec<_> = vt.lines().collect();
        let expected: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), expected.len());
        for (got, want) in lines.iter().zip(expected.iter()) {
            assert_eq!(got.as_ref(), *want);
        }
    }

    #[test]
    fn lines_fused() {
        let vt = VimText::from_str("a\nb");
        let mut iter = vt.lines();
        assert_eq!(iter.next().as_deref(), Some("a"));
        assert_eq!(iter.next().as_deref(), Some("b"));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn lines_single_chunk_borrows() {
        // A small text fits in one chunk, so all lines should be Borrowed
        let vt = VimText::from_str("hello\nworld");
        for line in vt.lines() {
            assert!(
                matches!(line, Cow::Borrowed(_)),
                "expected Borrowed, got Owned for {:?}",
                line
            );
        }
    }

    #[test]
    fn lines_size_hint_accurate() {
        let vt = VimText::from_str("a\nb\nc");
        let mut iter = vt.lines();
        assert_eq!(iter.size_hint(), (3, Some(3)));
        iter.next();
        assert_eq!(iter.size_hint(), (2, Some(2)));
        iter.next_back();
        assert_eq!(iter.size_hint(), (1, Some(1)));
        iter.next();
        assert_eq!(iter.size_hint(), (0, Some(0)));
    }
}
