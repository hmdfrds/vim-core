use std::iter::FusedIterator;

use super::chunks::Chunks;

/// Iterator over individual `char`s of a [`VimText`](crate::VimText).
///
/// Wraps a [`Chunks`] iterator for efficient access: characters are yielded
/// from the current chunk's char iterator without per-char tree traversal.
///
/// Implements [`DoubleEndedIterator`] and [`FusedIterator`].
///
/// # Internal design
///
/// Maintains two chunk buffers (forward and backward), each with a sub-slice
/// of remaining characters. When both directions deplete the `Chunks` source,
/// they share a single buffer and converge within it.
pub struct Chars<'a> {
    chunks: Chunks<'a>,

    // Forward state: the current chunk string and byte offsets of the
    // unconsumed region within it.
    current_fwd: &'a str,
    fwd_lo: usize, // byte offset into current_fwd
    fwd_hi: usize, // byte offset of end of unconsumed region

    // Backward state: analogous to forward but populated by next_back.
    current_bwd: &'a str,
    bwd_lo: usize,
    bwd_hi: usize,
}

impl<'a> Chars<'a> {
    /// Create a new `Chars` iterator wrapping the given `Chunks`.
    pub(crate) fn new(chunks: Chunks<'a>) -> Self {
        Chars {
            chunks,
            current_fwd: "",
            fwd_lo: 0,
            fwd_hi: 0,
            current_bwd: "",
            bwd_lo: 0,
            bwd_hi: 0,
        }
    }
}

impl<'a> Iterator for Chars<'a> {
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

        // 3. Chunks exhausted from front -- try the backward buffer
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

impl DoubleEndedIterator for Chars<'_> {
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

        // 3. Chunks exhausted from back -- try the forward buffer
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

impl FusedIterator for Chars<'_> {}

#[cfg(test)]
mod tests {
    use crate::VimText;

    #[test]
    fn chars_ascii() {
        let vt = VimText::from_str("hello");
        let chars: Vec<char> = vt.chars().collect();
        assert_eq!(chars, vec!['h', 'e', 'l', 'l', 'o']);
    }

    #[test]
    fn chars_unicode() {
        let vt = VimText::from_str("h\u{00E9}llo");
        let chars: Vec<char> = vt.chars().collect();
        assert_eq!(chars, vec!['h', '\u{00E9}', 'l', 'l', 'o']);
    }

    #[test]
    fn chars_emoji() {
        let vt = VimText::from_str("\u{1F389}");
        let chars: Vec<char> = vt.chars().collect();
        assert_eq!(chars, vec!['\u{1F389}']);
    }

    #[test]
    fn chars_mixed_utf8_widths() {
        // 1-byte, 2-byte, 3-byte, 4-byte
        let text = "a\u{00E9}\u{4E16}\u{1F600}b";
        let vt = VimText::from_str(text);
        let chars: Vec<char> = vt.chars().collect();
        let expected: Vec<char> = text.chars().collect();
        assert_eq!(chars, expected);
    }

    #[test]
    fn chars_backward() {
        let vt = VimText::from_str("hello");
        let rev: Vec<char> = vt.chars().rev().collect();
        assert_eq!(rev, vec!['o', 'l', 'l', 'e', 'h']);
    }

    #[test]
    fn chars_backward_unicode() {
        let text = "a\u{00E9}\u{4E16}\u{1F600}b";
        let vt = VimText::from_str(text);
        let rev: Vec<char> = vt.chars().rev().collect();
        let expected: Vec<char> = text.chars().rev().collect();
        assert_eq!(rev, expected);
    }

    #[test]
    fn chars_empty() {
        let vt = VimText::new();
        let chars: Vec<char> = vt.chars().collect();
        assert!(chars.is_empty());

        let rev: Vec<char> = vt.chars().rev().collect();
        assert!(rev.is_empty());
    }

    #[test]
    fn chars_double_ended() {
        let vt = VimText::from_str("abcdef");
        let mut iter = vt.chars();

        assert_eq!(iter.next(), Some('a'));
        assert_eq!(iter.next_back(), Some('f'));
        assert_eq!(iter.next(), Some('b'));
        assert_eq!(iter.next_back(), Some('e'));
        assert_eq!(iter.next(), Some('c'));
        assert_eq!(iter.next_back(), Some('d'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn chars_double_ended_unicode() {
        let text = "a\u{00E9}\u{4E16}\u{1F600}b";
        let vt = VimText::from_str(text);
        let mut iter = vt.chars();
        let expected: Vec<char> = text.chars().collect();

        assert_eq!(iter.next(), Some(expected[0])); // 'a'
        assert_eq!(iter.next_back(), Some(expected[4])); // 'b'
        assert_eq!(iter.next(), Some(expected[1])); // e-acute
        assert_eq!(iter.next_back(), Some(expected[3])); // emoji
        assert_eq!(iter.next(), Some(expected[2])); // CJK
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn chars_fused() {
        let vt = VimText::from_str("ab");
        let mut iter = vt.chars();
        assert_eq!(iter.next(), Some('a'));
        assert_eq!(iter.next(), Some('b'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn chars_large_text() {
        let text: String = "abcdefghij\n".repeat(200);
        let vt = VimText::from_str(&text);

        let fwd: Vec<char> = vt.chars().collect();
        let expected: Vec<char> = text.chars().collect();
        assert_eq!(fwd, expected);

        let rev: Vec<char> = vt.chars().rev().collect();
        let expected_rev: Vec<char> = text.chars().rev().collect();
        assert_eq!(rev, expected_rev);
    }

    #[test]
    fn chars_alternating_large() {
        let text: String = "abcdefghij\n".repeat(200);
        let vt = VimText::from_str(&text);
        let expected: Vec<char> = text.chars().collect();
        let mut iter = vt.chars();

        let mut lo = 0usize;
        let mut hi = expected.len();
        let mut toggle = true;

        loop {
            if toggle {
                match iter.next() {
                    Some(c) => {
                        assert_eq!(c, expected[lo], "mismatch at forward index {lo}");
                        lo += 1;
                    }
                    None => break,
                }
            } else {
                match iter.next_back() {
                    Some(c) => {
                        hi -= 1;
                        assert_eq!(c, expected[hi], "mismatch at backward index {hi}");
                    }
                    None => break,
                }
            }
            toggle = !toggle;
        }
        assert_eq!(lo, hi, "forward and backward should meet");
    }

    #[test]
    fn chars_single() {
        let vt = VimText::from_str("x");
        let mut iter = vt.chars();
        assert_eq!(iter.next(), Some('x'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn chars_single_back_first() {
        let vt = VimText::from_str("x");
        let mut iter = vt.chars();
        assert_eq!(iter.next_back(), Some('x'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }
}
