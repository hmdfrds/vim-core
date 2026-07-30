// TextChunk: fixed-capacity leaf content for the B+ tree text buffer.
//
// Uses ArrayString<1024> for allocator-friendly inline storage matching
// the conventions of ropey/crop/xi-editor.

use arrayvec::ArrayString;

use crate::summary::TextSummary;
use crate::tree::Item;

/// Maximum byte capacity per chunk (matches ropey/crop/xi conventions).
pub const CHUNK_MAX_BYTES: usize = 1024;

/// Minimum byte length before a chunk is considered "undersized" (triggers merge).
/// Set to MAX / 4, following xi-editor precedent.
pub const CHUNK_MIN_BYTES: usize = 256;

/// A fixed-capacity text leaf stored in the B+ tree.
///
/// Holds up to `CHUNK_MAX_BYTES` bytes of valid UTF-8 in a stack-allocated
/// `ArrayString`. All operations maintain the invariant that contents are
/// valid UTF-8 and respect char boundaries.
#[derive(Clone, Debug)]
pub struct TextChunk {
    text: ArrayString<CHUNK_MAX_BYTES>,
}

impl TextChunk {
    /// Create a new chunk from a string slice.
    ///
    /// # Panics
    /// Panics if `s.len() > CHUNK_MAX_BYTES`.
    pub fn new(s: &str) -> Self {
        assert!(
            s.len() <= CHUNK_MAX_BYTES,
            "TextChunk::new: input exceeds {} bytes (got {})",
            CHUNK_MAX_BYTES,
            s.len()
        );
        let mut text = ArrayString::new();
        text.push_str(s);
        Self { text }
    }

    /// View the chunk contents as a string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }

    /// View the chunk contents as a byte slice.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.text.as_bytes()
    }

    /// Byte length of the chunk contents.
    #[inline]
    pub fn byte_len(&self) -> usize {
        self.text.len()
    }
}

impl Item for TextChunk {
    type Summary = TextSummary;
    const MIN_LEN: usize = CHUNK_MIN_BYTES;
    const MAX_LEN: usize = CHUNK_MAX_BYTES;

    fn summary(&self) -> TextSummary {
        TextSummary::from_str(self.text.as_str())
    }

    fn len(&self) -> usize {
        self.text.len()
    }

    fn split_at(&mut self, offset: usize) -> Self {
        assert!(
            self.text.as_str().is_char_boundary(offset),
            "TextChunk::split_at: offset {} is not a char boundary",
            offset
        );
        let right_str = &self.text.as_str()[offset..];
        let right = TextChunk::new(right_str);
        // Truncate self to [0..offset]
        let mut left_text = ArrayString::new();
        left_text.push_str(&self.text.as_str()[..offset]);
        self.text = left_text;
        right
    }

    fn try_merge(&mut self, other: &Self) -> bool {
        if self.text.len() + other.text.len() > CHUNK_MAX_BYTES {
            return false;
        }
        self.text.push_str(other.text.as_str());
        true
    }
}

/// Split a string into chunk-sized pieces respecting char boundaries and CRLF.
///
/// Prefers splitting at newline boundaries in the last 25% of the chunk when
/// possible. Never splits CRLF pairs across chunks.
///
/// Returns at least one chunk (an empty chunk for empty input).
pub fn chunk_text(text: &str) -> Vec<TextChunk> {
    if text.is_empty() {
        return vec![TextChunk::new("")];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= CHUNK_MAX_BYTES {
            chunks.push(TextChunk::new(remaining));
            break;
        }

        let mut split = CHUNK_MAX_BYTES;

        // Back up to a char boundary.
        while split > 0 && !remaining.is_char_boundary(split) {
            split -= 1;
        }

        // Don't split CRLF pairs: if split lands between \r and \n, back up.
        if split > 0
            && split < remaining.len()
            && remaining.as_bytes()[split - 1] == b'\r'
            && remaining.as_bytes()[split] == b'\n'
        {
            split -= 1;
        }

        // Prefer splitting at a newline in the last 25% of the chunk.
        let search_start = split * 3 / 4;
        if let Some(nl_pos) = remaining[search_start..split].rfind('\n') {
            split = search_start + nl_pos + 1; // split AFTER the newline
        }

        // Safety: if split is 0 (shouldn't happen with valid UTF-8), take what we can.
        if split == 0 {
            split = remaining.len().min(CHUNK_MAX_BYTES);
            while split < remaining.len() && !remaining.is_char_boundary(split) {
                split += 1;
            }
        }

        chunks.push(TextChunk::new(&remaining[..split]));
        remaining = &remaining[split..];
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Item;

    #[test]
    fn new_empty() {
        let chunk = TextChunk::new("");
        assert_eq!(chunk.as_str(), "");
        assert_eq!(chunk.byte_len(), 0);
    }

    #[test]
    fn new_ascii() {
        let chunk = TextChunk::new("hello world");
        assert_eq!(chunk.as_str(), "hello world");
        assert_eq!(chunk.byte_len(), 11);
        assert_eq!(chunk.len(), 11);
    }

    #[test]
    fn split_at_ascii() {
        let mut chunk = TextChunk::new("hello world");
        let right = chunk.split_at(5);
        assert_eq!(chunk.as_str(), "hello");
        assert_eq!(right.as_str(), " world");
    }

    #[test]
    fn split_at_unicode_boundary() {
        let mut chunk = TextChunk::new("h\u{00E9}llo"); // "héllo", é is 2 bytes
        let right = chunk.split_at(3); // after 'é'
        assert_eq!(chunk.as_str(), "h\u{00E9}");
        assert_eq!(right.as_str(), "llo");
    }

    #[test]
    #[should_panic(expected = "not a char boundary")]
    fn split_at_invalid_boundary_panics() {
        let mut chunk = TextChunk::new("h\u{00E9}llo");
        chunk.split_at(2); // middle of 'é'
    }

    #[test]
    fn try_merge_success() {
        let mut a = TextChunk::new("hello");
        let b = TextChunk::new(" world");
        assert!(a.try_merge(&b));
        assert_eq!(a.as_str(), "hello world");
    }

    #[test]
    fn try_merge_failure_over_capacity() {
        let s: String = "x".repeat(600);
        let mut a = TextChunk::new(&s);
        let t: String = "y".repeat(500);
        let b = TextChunk::new(&t);
        assert!(!a.try_merge(&b)); // 600 + 500 > 1024
        assert_eq!(a.as_str(), &s); // unchanged
    }

    #[test]
    fn summary_correct() {
        let chunk = TextChunk::new("hello\nworld");
        let summary = chunk.summary();
        assert_eq!(summary.metrics.bytes, 11);
        assert_eq!(summary.metrics.newlines, 1);
        assert_eq!(summary.metrics.chars, 11);
    }

    // ===================================================================
    // chunk_text tests
    // ===================================================================

    #[test]
    fn chunk_text_empty() {
        let chunks = chunk_text("");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].as_str(), "");
    }

    #[test]
    fn chunk_text_small_fits_one_chunk() {
        let chunks = chunk_text("hello world");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].as_str(), "hello world");
    }

    #[test]
    fn chunk_text_large_splits_correctly() {
        let text: String = "a".repeat(2000);
        let chunks = chunk_text(&text);
        assert!(chunks.len() >= 2);
        // Verify all chunks are within MAX
        for chunk in &chunks {
            assert!(chunk.byte_len() <= CHUNK_MAX_BYTES);
        }
        // Verify concatenation equals original
        let reconstructed: String = chunks.iter().map(|c| c.as_str()).collect();
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn chunk_text_respects_char_boundaries() {
        // String with 4-byte emoji repeated many times
        let text: String = "\u{1F389}".repeat(300); // 300 * 4 = 1200 bytes
        let chunks = chunk_text(&text);
        for chunk in &chunks {
            // Each chunk must be valid UTF-8 (enforced by TextChunk::new)
            assert!(chunk.as_str().is_char_boundary(0));
            assert!(chunk.byte_len() <= CHUNK_MAX_BYTES);
        }
        let reconstructed: String = chunks.iter().map(|c| c.as_str()).collect();
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn chunk_text_never_splits_crlf() {
        // Create text with CRLF near the split boundary
        let mut text = String::new();
        text.push_str(&"x".repeat(1023)); // 1023 bytes
        text.push_str("\r\n"); // bytes 1023-1024: \r at 1023, \n at 1024
        text.push_str("after");
        let chunks = chunk_text(&text);
        // The \r\n should not be split across chunks
        for chunk in &chunks {
            let bytes = chunk.as_bytes();
            if let Some(&last) = bytes.last() {
                if last == b'\r' {
                    // If chunk ends with \r, next chunk must NOT start with \n
                    // (CRLF should be kept together)
                    panic!("chunk ends with \\r, CRLF was split");
                }
            }
        }
    }

    #[test]
    fn chunk_text_prefers_newline_split() {
        // Text with a newline in the last 25% of the max chunk size
        let mut text = String::new();
        text.push_str(&"x".repeat(800)); // 800 bytes
        text.push('\n'); // byte 800
        text.push_str(&"y".repeat(500)); // bytes 801-1300
        let chunks = chunk_text(&text);
        // First chunk should split at the newline (byte 801)
        assert_eq!(chunks[0].byte_len(), 801); // includes the \n
    }

    #[test]
    fn chunk_text_roundtrip_multibyte() {
        // Mix of 1, 2, 3, and 4-byte chars
        let text: String = "a\u{00E9}\u{4E16}\u{1F600}".repeat(100); // 10 bytes * 100 = 1000 bytes
        let chunks = chunk_text(&text);
        let reconstructed: String = chunks.iter().map(|c| c.as_str()).collect();
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn chunk_text_exactly_max_bytes() {
        let text: String = "a".repeat(CHUNK_MAX_BYTES);
        let chunks = chunk_text(&text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].byte_len(), CHUNK_MAX_BYTES);
    }

    #[test]
    fn chunk_text_one_over_max() {
        let text: String = "a".repeat(CHUNK_MAX_BYTES + 1);
        let chunks = chunk_text(&text);
        assert_eq!(chunks.len(), 2);
        let reconstructed: String = chunks.iter().map(|c| c.as_str()).collect();
        assert_eq!(reconstructed, text);
    }
}
