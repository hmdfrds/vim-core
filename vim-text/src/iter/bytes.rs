use std::iter::FusedIterator;

use super::chunks::Chunks;

/// Iterator over individual bytes of a [`VimText`](crate::VimText).
///
/// Wraps a [`Chunks`] iterator for efficient access: bytes are yielded from
/// the current chunk's byte slice without per-byte tree traversal.
///
/// Implements [`DoubleEndedIterator`] and [`FusedIterator`].
///
/// # Internal design
///
/// Maintains two byte buffers: one filled by forward chunk iteration, one by
/// backward. When both directions deplete the `Chunks` source, they share
/// a single buffer (the forward one) and converge within it.
pub struct Bytes<'a> {
    chunks: Chunks<'a>,

    // Forward state: bytes from the latest `chunks.next()` call.
    // `fwd_lo..fwd_hi` are the unconsumed bytes within `current_fwd`.
    current_fwd: &'a [u8],
    fwd_lo: usize,
    fwd_hi: usize,

    // Backward state: bytes from the latest `chunks.next_back()` call.
    // `bwd_lo..bwd_hi` are the unconsumed bytes within `current_bwd`.
    current_bwd: &'a [u8],
    bwd_lo: usize,
    bwd_hi: usize,
}

impl<'a> Bytes<'a> {
    /// Create a new `Bytes` iterator wrapping the given `Chunks`.
    pub(crate) fn new(chunks: Chunks<'a>) -> Self {
        Bytes {
            chunks,
            current_fwd: &[],
            fwd_lo: 0,
            fwd_hi: 0,
            current_bwd: &[],
            bwd_lo: 0,
            bwd_hi: 0,
        }
    }
}

impl<'a> Iterator for Bytes<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        // 1. Try the forward buffer
        if self.fwd_lo < self.fwd_hi {
            let byte = self.current_fwd[self.fwd_lo];
            self.fwd_lo += 1;
            return Some(byte);
        }

        // 2. Try to get a new forward chunk
        if let Some(chunk_str) = self.chunks.next() {
            self.current_fwd = chunk_str.as_bytes();
            self.fwd_lo = 1;
            self.fwd_hi = self.current_fwd.len();
            return Some(self.current_fwd[0]);
        }

        // 3. Chunks exhausted from front — try the backward buffer
        // (this handles the convergence case where next_back loaded
        // a chunk that still has unconsumed bytes from the front)
        if self.bwd_lo < self.bwd_hi {
            let byte = self.current_bwd[self.bwd_lo];
            self.bwd_lo += 1;
            return Some(byte);
        }

        None
    }
}

impl DoubleEndedIterator for Bytes<'_> {
    fn next_back(&mut self) -> Option<u8> {
        // 1. Try the backward buffer
        if self.bwd_lo < self.bwd_hi {
            self.bwd_hi -= 1;
            return Some(self.current_bwd[self.bwd_hi]);
        }

        // 2. Try to get a new backward chunk
        if let Some(chunk_str) = self.chunks.next_back() {
            self.current_bwd = chunk_str.as_bytes();
            self.bwd_lo = 0;
            self.bwd_hi = self.current_bwd.len().saturating_sub(1);
            if self.current_bwd.is_empty() {
                return None;
            }
            return Some(self.current_bwd[self.bwd_hi]);
        }

        // 3. Chunks exhausted from back — try the forward buffer
        // (convergence: the forward buffer may have bytes that
        // next_back should yield from the end)
        if self.fwd_lo < self.fwd_hi {
            self.fwd_hi -= 1;
            return Some(self.current_fwd[self.fwd_hi]);
        }

        None
    }
}

impl FusedIterator for Bytes<'_> {}

#[cfg(test)]
mod tests {
    use crate::VimText;

    #[test]
    fn forward_bytes() {
        let text = "hello world";
        let vt = VimText::from_str(text);
        let collected: Vec<u8> = vt.bytes().collect();
        assert_eq!(collected, text.as_bytes());
    }

    #[test]
    fn backward_bytes() {
        let text = "hello world";
        let vt = VimText::from_str(text);
        let rev_bytes: Vec<u8> = vt.bytes().rev().collect();
        let expected: Vec<u8> = text.as_bytes().iter().copied().rev().collect();
        assert_eq!(rev_bytes, expected);
    }

    #[test]
    fn empty_text_bytes() {
        let vt = VimText::new();
        let collected: Vec<u8> = vt.bytes().collect();
        assert!(collected.is_empty());

        let rev: Vec<u8> = vt.bytes().rev().collect();
        assert!(rev.is_empty());
    }

    #[test]
    fn large_text_bytes_match() {
        let text: String = "abcdefghij\n".repeat(200); // >1024 bytes
        let vt = VimText::from_str(&text);

        let fwd: Vec<u8> = vt.bytes().collect();
        assert_eq!(fwd, text.as_bytes());

        let rev: Vec<u8> = vt.bytes().rev().collect();
        let expected_rev: Vec<u8> = text.as_bytes().iter().copied().rev().collect();
        assert_eq!(rev, expected_rev);
    }

    #[test]
    fn unicode_bytes() {
        let text = "a\u{00E9}\u{4E16}\u{1F600}b"; // mixed 1,2,3,4-byte chars
        let vt = VimText::from_str(text);

        let fwd: Vec<u8> = vt.bytes().collect();
        assert_eq!(fwd, text.as_bytes());

        let rev: Vec<u8> = vt.bytes().rev().collect();
        let expected: Vec<u8> = text.as_bytes().iter().copied().rev().collect();
        assert_eq!(rev, expected);
    }

    #[test]
    fn alternating_front_back_bytes() {
        let text = "abcdef";
        let vt = VimText::from_str(text);
        let mut iter = vt.bytes();

        assert_eq!(iter.next(), Some(b'a'));
        assert_eq!(iter.next_back(), Some(b'f'));
        assert_eq!(iter.next(), Some(b'b'));
        assert_eq!(iter.next_back(), Some(b'e'));
        assert_eq!(iter.next(), Some(b'c'));
        assert_eq!(iter.next_back(), Some(b'd'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn alternating_front_back_large() {
        // Multiple chunks: ensure convergence works across chunk boundaries
        let text: String = "abcdefghij\n".repeat(200);
        let vt = VimText::from_str(&text);
        let bytes = text.as_bytes();
        let mut iter = vt.bytes();

        let mut lo = 0usize;
        let mut hi = bytes.len();
        let mut toggle = true;

        loop {
            if toggle {
                match iter.next() {
                    Some(b) => {
                        assert_eq!(b, bytes[lo], "mismatch at forward index {lo}");
                        lo += 1;
                    }
                    None => break,
                }
            } else {
                match iter.next_back() {
                    Some(b) => {
                        hi -= 1;
                        assert_eq!(b, bytes[hi], "mismatch at backward index {hi}");
                    }
                    None => break,
                }
            }
            toggle = !toggle;
        }
        assert_eq!(lo, hi, "forward and backward should meet");
    }

    #[test]
    fn fused_after_exhaustion() {
        let vt = VimText::from_str("ab");
        let mut iter = vt.bytes();
        assert_eq!(iter.next(), Some(b'a'));
        assert_eq!(iter.next(), Some(b'b'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn single_byte() {
        let vt = VimText::from_str("x");
        let mut iter = vt.bytes();
        assert_eq!(iter.next(), Some(b'x'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn single_byte_back_first() {
        let vt = VimText::from_str("x");
        let mut iter = vt.bytes();
        assert_eq!(iter.next_back(), Some(b'x'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn two_bytes_front_then_back() {
        let vt = VimText::from_str("ab");
        let mut iter = vt.bytes();
        assert_eq!(iter.next(), Some(b'a'));
        assert_eq!(iter.next_back(), Some(b'b'));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }
}
