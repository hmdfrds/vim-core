/// 1024-bit bloom filter using byte-bigram hashing.
///
/// Used for accelerated text search: each leaf chunk produces a bloom filter
/// from its byte bigrams, enabling quick rejection of chunks that definitely
/// do not contain a search pattern.
///
/// **Key property:** No false negatives. If a chunk contains the pattern,
/// `might_contain` will return `true`. False positives are possible.
#[derive(Clone, Debug)]
pub struct BloomFilter {
    bits: [u64; 16],
}

impl BloomFilter {
    pub const EMPTY: Self = Self { bits: [0; 16] };

    /// Single good hash split into two approximately independent 10-bit indices
    /// (adjacent bit ranges of a single multiply — correlated but sufficient
    /// for pre-filtering).
    ///
    /// Uses a multiplicative hash on the 16-bit bigram value, then extracts
    /// h1 from bits [0..10) and h2 from bits [10..20).
    fn hash_bigram(b0: u8, b1: u8) -> (u16, u16) {
        let h = ((b0 as u32) | ((b1 as u32) << 8)).wrapping_mul(0x517cc1b7);
        ((h & 0x3FF) as u16, ((h >> 10) & 0x3FF) as u16)
    }

    /// Build a bloom filter from the byte bigrams in `text`.
    pub fn from_text(text: impl AsRef<[u8]>) -> Self {
        let bytes = text.as_ref();
        let mut filter = Self::EMPTY;
        if bytes.len() < 2 {
            return filter;
        }
        for window in bytes.windows(2) {
            let (h1, h2) = Self::hash_bigram(window[0], window[1]);
            filter.set_bit(h1);
            filter.set_bit(h2);
        }
        filter
    }

    /// Check if this filter might contain all bigrams of `pattern`.
    ///
    /// Returns `true` (conservatively) for patterns shorter than 2 bytes,
    /// since single-byte patterns cannot be filtered by bigram hashing.
    pub fn might_contain(&self, pattern: &[u8]) -> bool {
        if pattern.len() < 2 {
            return true; // can't filter single bytes
        }
        for window in pattern.windows(2) {
            let (h1, h2) = Self::hash_bigram(window[0], window[1]);
            if !self.get_bit(h1) || !self.get_bit(h2) {
                return false;
            }
        }
        true
    }

    /// Check if all bits set in `other` are also set in `self`.
    ///
    /// This is the bloom filter equivalent of "self might contain the
    /// pattern that produced `other`". No false negatives.
    pub fn contains_filter(&self, other: &Self) -> bool {
        for i in 0..16 {
            if (other.bits[i] & !self.bits[i]) != 0 {
                return false;
            }
        }
        true
    }

    fn set_bit(&mut self, idx: u16) {
        let word = (idx / 64) as usize;
        let bit = idx % 64;
        self.bits[word] |= 1u64 << bit;
    }

    fn get_bit(&self, idx: u16) -> bool {
        let word = (idx / 64) as usize;
        let bit = idx % 64;
        (self.bits[word] >> bit) & 1 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_contains_nothing() {
        let f = BloomFilter::EMPTY;
        assert!(!f.might_contain(b"ab"));
        assert!(!f.might_contain(b"hello"));
    }

    #[test]
    fn single_byte_always_passes() {
        let f = BloomFilter::EMPTY;
        assert!(f.might_contain(b"a"));
        assert!(f.might_contain(b""));
    }

    #[test]
    fn from_text_contains_own_bigrams() {
        let f = BloomFilter::from_text("hello world");
        // Every bigram from "hello world" must be found
        assert!(f.might_contain(b"he"));
        assert!(f.might_contain(b"el"));
        assert!(f.might_contain(b"ll"));
        assert!(f.might_contain(b"lo"));
        assert!(f.might_contain(b"o "));
        assert!(f.might_contain(b" w"));
        assert!(f.might_contain(b"wo"));
        assert!(f.might_contain(b"or"));
        assert!(f.might_contain(b"rl"));
        assert!(f.might_contain(b"ld"));
    }

    #[test]
    fn from_text_rejects_absent_pattern() {
        let f = BloomFilter::from_text("hello world");
        // "xyz" bigrams ("xy", "yz") are very unlikely to be set
        // This could theoretically be a false positive, but for this specific
        // input it should reject.
        assert!(!f.might_contain(b"xyz"));
    }

    #[test]
    fn hash_produces_independent_bits() {
        // Verify h1 and h2 differ for typical inputs
        let (h1, h2) = BloomFilter::hash_bigram(b'a', b'b');
        assert_ne!(h1, h2, "h1 and h2 should differ for 'ab'");
    }

    #[test]
    fn short_text_returns_empty_filter() {
        let f = BloomFilter::from_text("a");
        // Single char text has no bigrams, so filter should be empty
        assert_eq!(f.bits, [0; 16]);
    }

    #[test]
    fn empty_text_returns_empty_filter() {
        let f = BloomFilter::from_text("");
        assert_eq!(f.bits, [0; 16]);
    }

    #[test]
    fn no_false_negatives_on_substrings() {
        let text = "the quick brown fox jumps over the lazy dog";
        let f = BloomFilter::from_text(text);
        // Every substring of length >= 2 must pass might_contain
        for window in text.as_bytes().windows(3) {
            assert!(
                f.might_contain(window),
                "false negative for {:?}",
                std::str::from_utf8(window)
            );
        }
    }
}
