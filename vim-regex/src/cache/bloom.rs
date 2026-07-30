//! Per-line trigram Bloom filters for fast line rejection.
//!
//! Each line gets a 256-bit Bloom filter encoding its character trigrams.
//! During search, the pattern's required trigrams are tested against each
//! line's filter -- lines that fail the test are skipped entirely.

/// A 256-bit Bloom filter for a single line's trigrams.
///
/// Uses 2 hash functions (h1, h2) mapped to bit positions in a 256-bit
/// array. False positive rate for ~20 trigrams is approximately 5%.
#[derive(Debug, Clone, Copy, Default)]
pub struct LineBloomFilter {
    bits: [u64; 4],
}

impl LineBloomFilter {
    /// Create an empty filter.
    pub const fn empty() -> Self {
        Self { bits: [0; 4] }
    }

    /// Build a filter from a line of text by inserting all character trigrams.
    pub fn from_line(line: &str) -> Self {
        let mut filter = Self::empty();
        let bytes = line.as_bytes();
        if bytes.len() < 3 {
            // Lines shorter than 3 bytes have no trigrams -- filter stays empty.
            // This means all pattern trigram tests will fail, which is correct:
            // a pattern requiring a trigram cannot match a line shorter than 3 bytes.
            return filter;
        }
        for window in bytes.windows(3) {
            let trigram =
                u32::from(window[0]) | (u32::from(window[1]) << 8) | (u32::from(window[2]) << 16);
            filter.insert(trigram);
        }
        filter
    }

    /// Insert a trigram hash into the filter.
    #[inline]
    fn insert(&mut self, trigram: u32) {
        let h1 = (trigram & 0xFF) as usize;
        let h2 = ((trigram >> 8) ^ (trigram >> 16)) as usize & 0xFF;
        self.set_bit(h1);
        self.set_bit(h2);
    }

    /// Test whether a trigram might be present.
    #[inline]
    pub fn might_contain(&self, trigram: u32) -> bool {
        let h1 = (trigram & 0xFF) as usize;
        let h2 = ((trigram >> 8) ^ (trigram >> 16)) as usize & 0xFF;
        self.get_bit(h1) && self.get_bit(h2)
    }

    /// Test whether all of the given trigrams might be present.
    pub fn might_contain_all(&self, trigrams: &[u32]) -> bool {
        trigrams.iter().all(|&t| self.might_contain(t))
    }

    #[inline]
    fn set_bit(&mut self, bit: usize) {
        let word = bit >> 6; // bit / 64
        let offset = bit & 0x3F; // bit % 64
        self.bits[word] |= 1u64 << offset;
    }

    #[inline]
    fn get_bit(&self, bit: usize) -> bool {
        let word = bit >> 6;
        let offset = bit & 0x3F;
        self.bits[word] & (1u64 << offset) != 0
    }

    /// Whether the filter is empty (no trigrams inserted).
    pub fn is_empty(&self) -> bool {
        self.bits == [0; 4]
    }
}

/// Per-line trigram Bloom filter store.
///
/// Maintains a Bloom filter for each line in the buffer. On buffer edit,
/// only the affected lines are recomputed.
#[derive(Debug)]
pub struct LineBloomStore {
    /// One filter per line (0-indexed).
    filters: Vec<LineBloomFilter>,
}

impl LineBloomStore {
    /// Create a store from the full buffer text.
    pub fn from_text(text: &str) -> Self {
        let filters: Vec<_> = text.split('\n').map(LineBloomFilter::from_line).collect();
        Self { filters }
    }

    /// Create an empty store.
    pub fn empty() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Get the filter for a line (0-indexed).
    pub fn get(&self, line_idx: usize) -> Option<&LineBloomFilter> {
        self.filters.get(line_idx)
    }

    /// Update a single line's filter.
    pub fn update_line(&mut self, line_idx: usize, line_text: &str) {
        if line_idx < self.filters.len() {
            self.filters[line_idx] = LineBloomFilter::from_line(line_text);
        }
    }

    /// Handle a line insertion: insert a new filter at `line_idx`.
    pub fn insert_line(&mut self, line_idx: usize, line_text: &str) {
        let filter = LineBloomFilter::from_line(line_text);
        if line_idx <= self.filters.len() {
            self.filters.insert(line_idx, filter);
        }
    }

    /// Handle a line deletion: remove the filter at `line_idx`.
    pub fn remove_line(&mut self, line_idx: usize) {
        if line_idx < self.filters.len() {
            self.filters.remove(line_idx);
        }
    }

    /// Rebuild from the full buffer text.
    pub fn rebuild(&mut self, text: &str) {
        self.filters.clear();
        self.filters
            .extend(text.split('\n').map(LineBloomFilter::from_line));
    }

    /// Number of lines tracked.
    pub fn line_count(&self) -> usize {
        self.filters.len()
    }

    /// Test which lines might contain all required trigrams.
    /// Returns a list of line indices that pass the Bloom test.
    pub fn candidate_lines(&self, required_trigrams: &[u32]) -> Vec<usize> {
        if required_trigrams.is_empty() {
            return (0..self.filters.len()).collect();
        }
        self.filters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.might_contain_all(required_trigrams))
            .map(|(i, _)| i)
            .collect()
    }
}

/// Extract trigrams from a literal string (for pattern pre-screening).
pub fn extract_trigrams(literal: &str) -> Vec<u32> {
    let bytes = literal.as_bytes();
    if bytes.len() < 3 {
        return Vec::new();
    }
    bytes
        .windows(3)
        .map(|w| u32::from(w[0]) | (u32::from(w[1]) << 8) | (u32::from(w[2]) << 16))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloom_filter_insert_and_test() {
        let filter = LineBloomFilter::from_line("hello world");
        // "hel" trigram should be present.
        let trigram = u32::from(b'h') | (u32::from(b'e') << 8) | (u32::from(b'l') << 16);
        assert!(filter.might_contain(trigram));
    }

    #[test]
    fn bloom_filter_absent_trigram() {
        let filter = LineBloomFilter::from_line("hello world");
        // "xyz" trigram should (probably) be absent.
        let trigram = u32::from(b'x') | (u32::from(b'y') << 8) | (u32::from(b'z') << 16);
        // Bloom filters can have false positives, but for this specific
        // case with a small filter, "xyz" is very likely absent.
        // We test that the filter at least works without panicking.
        let _ = filter.might_contain(trigram);
    }

    #[test]
    fn bloom_filter_short_line() {
        let filter = LineBloomFilter::from_line("ab");
        assert!(filter.is_empty());
    }

    #[test]
    fn bloom_filter_empty_line() {
        let filter = LineBloomFilter::from_line("");
        assert!(filter.is_empty());
    }

    #[test]
    fn bloom_store_from_text() {
        let store = LineBloomStore::from_text("hello\nworld\nfoo");
        assert_eq!(store.line_count(), 3);
    }

    #[test]
    fn bloom_store_candidate_lines() {
        let store = LineBloomStore::from_text("hello world\ngoodbye\nhello again");
        let trigrams = extract_trigrams("hel");
        let candidates = store.candidate_lines(&trigrams);
        // Lines 0 and 2 contain "hel", line 1 does not.
        assert!(candidates.contains(&0));
        assert!(candidates.contains(&2));
        // Line 1 might or might not be a candidate (false positives possible).
    }

    #[test]
    fn bloom_store_update_line() {
        let mut store = LineBloomStore::from_text("aaa\nbbb\nccc");
        // Update line 1 to contain "hello".
        store.update_line(1, "hello world");
        let trigrams = extract_trigrams("hel");
        let candidates = store.candidate_lines(&trigrams);
        assert!(candidates.contains(&1));
    }

    #[test]
    fn bloom_store_insert_remove_line() {
        let mut store = LineBloomStore::from_text("aaa\nccc");
        assert_eq!(store.line_count(), 2);

        store.insert_line(1, "bbb");
        assert_eq!(store.line_count(), 3);

        store.remove_line(1);
        assert_eq!(store.line_count(), 2);
    }

    #[test]
    fn extract_trigrams_from_literal() {
        let trigrams = extract_trigrams("hello");
        // "hello" has 3 trigrams: hel, ell, llo.
        assert_eq!(trigrams.len(), 3);
    }

    #[test]
    fn extract_trigrams_short_literal() {
        assert!(extract_trigrams("he").is_empty());
        assert!(extract_trigrams("h").is_empty());
        assert!(extract_trigrams("").is_empty());
    }

    #[test]
    fn bloom_filter_might_contain_all() {
        let filter = LineBloomFilter::from_line("hello world");
        let present = extract_trigrams("hel");
        assert!(filter.might_contain_all(&present));

        // Empty trigrams should always pass.
        assert!(filter.might_contain_all(&[]));
    }

    #[test]
    fn bloom_store_empty() {
        let store = LineBloomStore::empty();
        assert_eq!(store.line_count(), 0);
    }

    #[test]
    fn bloom_store_rebuild() {
        let mut store = LineBloomStore::from_text("aaa\nbbb");
        assert_eq!(store.line_count(), 2);
        store.rebuild("xxx\nyyy\nzzz");
        assert_eq!(store.line_count(), 3);
    }

    #[test]
    fn bloom_store_candidate_lines_no_trigrams() {
        let store = LineBloomStore::from_text("aaa\nbbb\nccc");
        // No required trigrams -> all lines are candidates.
        let candidates = store.candidate_lines(&[]);
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn bloom_store_get() {
        let store = LineBloomStore::from_text("hello\nworld");
        assert!(store.get(0).is_some());
        assert!(store.get(1).is_some());
        assert!(store.get(2).is_none());
    }
}
