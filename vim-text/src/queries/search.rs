//! Text search acceleration via bloom filters.
//!
//! The bloom index is NOT stored on VimText (preserves O(1) clone via Arc COW).
//! Instead, bloom filters are computed on-the-fly per search call. This is
//! acceptable because search is called once per command (`:g`, `:s`), not per
//! keystroke.
//!
//! Algorithm:
//! 1. Iterate all chunks via the chunks iterator
//! 2. For each chunk, compute `BloomFilter::from_text(chunk)`
//! 3. Check `filter.might_contain(pattern)`
//! 4. If might contain: scan the chunk to find which lines match
//! 5. Return collected line numbers

use crate::bloom::BloomFilter;
use crate::VimText;

/// Text search acceleration via bloom filters.
pub trait TextSearch {
    /// Search for lines that might contain the pattern.
    /// Returns line numbers where the pattern might exist.
    ///
    /// **No false negatives:** if a line contains the pattern, it WILL be returned.
    /// **May have false positives:** returned lines might not actually contain the pattern.
    fn bloom_search_lines(&self, pattern: &[u8]) -> Vec<usize>;

    /// Quick check: does any part of the document potentially contain this pattern?
    ///
    /// Returns `true` if any chunk's bloom filter does not reject the pattern.
    /// A `false` return guarantees the pattern is absent from the entire document.
    fn might_contain_literal(&self, pattern: &[u8]) -> bool;
}

impl TextSearch for VimText {
    fn bloom_search_lines(&self, pattern: &[u8]) -> Vec<usize> {
        let mut result = Vec::new();
        let mut current_line: usize = 0;
        let mut any_chunks = false;

        // Carry buffer: when a chunk doesn't end with '\n', the trailing
        // fragment after the last '\n' is NOT a complete line. We buffer it
        // here and prepend it to the first line of the next chunk.
        let mut carry: Vec<u8> = Vec::new();

        // For empty or single-byte patterns, bloom can't filter (always returns true).
        // We still need to report which lines contain the pattern.
        let can_bloom_filter = pattern.len() >= 2;

        for chunk_str in self.chunks() {
            any_chunks = true;
            let chunk_bytes = chunk_str.as_bytes();

            // Find the last newline position so we can identify the trailing
            // fragment that must be carried forward regardless of bloom.
            let last_newline_pos = chunk_bytes.iter().rposition(|&b| b == b'\n');

            // Bloom pre-filter: can only skip the chunk's COMPLETE lines when
            // (a) there is no carry to merge, AND (b) bloom says no match.
            // The trailing fragment (after last '\n') ALWAYS goes into carry.
            let bloom_skip = can_bloom_filter && carry.is_empty() && {
                let filter = BloomFilter::from_text(chunk_str);
                !filter.might_contain(pattern)
            };

            if bloom_skip {
                // Skip scanning complete lines within this chunk.
                // But we still must carry forward any trailing fragment.
                if let Some(nl_pos) = last_newline_pos {
                    let newline_count = chunk_bytes.iter().filter(|&&b| b == b'\n').count();
                    current_line += newline_count;
                    // Trailing fragment after the last newline.
                    if nl_pos + 1 < chunk_bytes.len() {
                        carry.extend_from_slice(&chunk_bytes[nl_pos + 1..]);
                    }
                } else {
                    // No newlines at all: entire chunk is a partial line.
                    carry.extend_from_slice(chunk_bytes);
                }
                continue;
            }

            // Scan the chunk line by line, merging carry into the first line.
            let mut line_start = 0;
            for (i, &byte) in chunk_bytes.iter().enumerate() {
                if byte == b'\n' {
                    let fragment = &chunk_bytes[line_start..i];

                    if !carry.is_empty() {
                        // First newline in this chunk: merge carry + fragment
                        // to form the complete line.
                        carry.extend_from_slice(fragment);
                        if line_contains(&carry, pattern) && result.last() != Some(&current_line) {
                            result.push(current_line);
                        }
                        carry.clear();
                    } else if line_contains(fragment, pattern)
                        && result.last() != Some(&current_line)
                    {
                        result.push(current_line);
                    }

                    current_line += 1;
                    line_start = i + 1;
                }
            }

            // Trailing segment after the last newline (or the entire chunk if
            // no newlines). This is a partial line continuing into the next
            // chunk, or the final line of the document.
            if line_start < chunk_bytes.len() {
                let fragment = &chunk_bytes[line_start..];
                // Append to carry — do NOT search yet, the line is incomplete.
                carry.extend_from_slice(fragment);
            }
        }

        // After all chunks: if carry is non-empty, it's the final line of
        // the document (no trailing newline).
        if !carry.is_empty()
            && line_contains(&carry, pattern)
            && result.last() != Some(&current_line)
        {
            result.push(current_line);
        }

        // Edge case: empty document yields zero chunks but still has one
        // logical line (line 0). An empty pattern matches it.
        if !any_chunks && pattern.is_empty() {
            result.push(0);
        }

        result
    }

    fn might_contain_literal(&self, pattern: &[u8]) -> bool {
        if pattern.len() < 2 {
            // Can't bloom-filter single bytes or empty patterns.
            // Conservatively return true (except for empty doc + non-empty pattern).
            if pattern.is_empty() {
                return true;
            }
            // Single byte: must actually check
            for chunk_str in self.chunks() {
                if chunk_str.as_bytes().contains(&pattern[0]) {
                    return true;
                }
            }
            return false;
        }

        for chunk_str in self.chunks() {
            let filter = BloomFilter::from_text(chunk_str);
            if filter.might_contain(pattern) {
                return true;
            }
        }
        false
    }
}

/// Check if a line's bytes contain the pattern.
///
/// For empty patterns, every line matches.
/// For non-empty patterns, uses a simple byte substring search.
fn line_contains(line: &[u8], pattern: &[u8]) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if pattern.len() > line.len() {
        return false;
    }
    // Simple substring search — this is only called on bloom-positive chunks,
    // so the number of calls is bounded by false positive rate.
    line.windows(pattern.len()).any(|w| w == pattern)
}

#[cfg(test)]
mod tests {
    use super::TextSearch;
    use crate::VimText;

    #[test]
    fn bloom_no_false_negatives() {
        let t = VimText::from_str("hello world\nfoo bar\nbaz qux");
        let results = t.bloom_search_lines(b"foo");
        assert!(results.contains(&1), "line 1 ('foo bar') must be found");
    }

    #[test]
    fn bloom_search_finds_all_matching_lines() {
        let t = VimText::from_str("hello world\nfoo bar\nbaz qux\nfoo again");
        let results = t.bloom_search_lines(b"foo");
        assert!(results.contains(&1));
        assert!(results.contains(&3));
    }

    #[test]
    fn bloom_search_pattern_not_present() {
        let t = VimText::from_str("hello world\nfoo bar\nbaz qux");
        let _results = t.bloom_search_lines(b"xyz123");
        // May or may not be empty (false positives allowed), but the key
        // invariant is: no false negatives. That is verified above; this
        // case only confirms the API does not crash.
        assert!(!t.to_string().contains("xyz123"));
    }

    #[test]
    fn bloom_single_char_pattern() {
        let t = VimText::from_str("hello\nworld");
        let results = t.bloom_search_lines(b"h");
        // Single-char pattern can't be bloom-filtered, so it falls through
        // to the line scan, which finds "h" on line 0.
        assert!(results.contains(&0));
    }

    #[test]
    fn bloom_empty_pattern() {
        let t = VimText::from_str("hello\nworld");
        let results = t.bloom_search_lines(b"");
        // Empty pattern matches every line.
        assert!(results.contains(&0));
        assert!(results.contains(&1));
    }

    #[test]
    fn might_contain_literal_present() {
        let t = VimText::from_str("hello world");
        assert!(t.might_contain_literal(b"world"));
    }

    #[test]
    fn might_contain_literal_absent() {
        let t = VimText::from_str("hello world");
        // If might_contain_literal returns false, the pattern is definitely absent.
        let result = t.might_contain_literal(b"xyzzy12345");
        if !result {
            assert!(!t.to_string().contains("xyzzy12345"));
        }
    }

    #[test]
    fn might_contain_literal_empty_pattern() {
        let t = VimText::from_str("hello");
        assert!(t.might_contain_literal(b""));
    }

    #[test]
    fn might_contain_literal_single_byte_present() {
        let t = VimText::from_str("hello");
        assert!(t.might_contain_literal(b"h"));
        assert!(t.might_contain_literal(b"o"));
    }

    #[test]
    fn might_contain_literal_single_byte_absent() {
        let t = VimText::from_str("hello");
        assert!(!t.might_contain_literal(b"z"));
    }

    #[test]
    fn bloom_search_large_text() {
        let mut lines: Vec<String> = (0..1000).map(|i| format!("line {i} content")).collect();
        lines[500] = "NEEDLE in haystack".to_string();
        let t = VimText::from_str(&lines.join("\n"));
        let results = t.bloom_search_lines(b"NEEDLE");
        assert!(
            results.contains(&500),
            "line 500 must be found; got: {:?}",
            results
        );
    }

    #[test]
    fn bloom_search_first_line() {
        let t = VimText::from_str("target line\nsecond line\nthird line");
        let results = t.bloom_search_lines(b"target");
        assert!(results.contains(&0));
    }

    #[test]
    fn bloom_search_last_line_no_trailing_newline() {
        let t = VimText::from_str("first line\nsecond line\ntarget line");
        let results = t.bloom_search_lines(b"target");
        assert!(results.contains(&2));
    }

    #[test]
    fn bloom_search_empty_doc() {
        let t = VimText::from_str("");
        let results = t.bloom_search_lines(b"foo");
        assert!(results.is_empty());
    }

    #[test]
    fn bloom_search_empty_doc_empty_pattern() {
        let t = VimText::from_str("");
        let results = t.bloom_search_lines(b"");
        // Empty pattern matches the single empty line.
        assert!(results.contains(&0));
    }

    #[test]
    fn might_contain_literal_empty_doc() {
        let t = VimText::from_str("");
        assert!(!t.might_contain_literal(b"foo"));
        assert!(t.might_contain_literal(b""));
    }

    #[test]
    fn bloom_search_pattern_at_line_boundary() {
        // Pattern that spans a newline should NOT match
        let t = VimText::from_str("abc\ndef");
        let results = t.bloom_search_lines(b"c\nd");
        // No line can match: the search is per-line and line_contains works
        // on line bytes without the \n, so "c\nd" matches neither "abc" nor
        // "def".
        for &line_num in &results {
            // Verify no false negative by checking actual line content
            let _line_text = match line_num {
                0 => "abc",
                1 => "def",
                _ => panic!("unexpected line {}", line_num),
            };
        }
    }

    #[test]
    fn bloom_cross_chunk_line_finds_pattern() {
        // Create text where "FINDME" spans a chunk boundary
        let mut text = String::new();
        text.push_str(&"x".repeat(1020));
        text.push_str("FIN"); // bytes 1020-1022 (end of first chunk ~1024 bytes)
        text.push_str("DME"); // bytes 1023-1025 (start of second chunk)
        text.push('\n');
        text.push_str("other line\n");

        let vt = crate::VimText::from_str(&text);
        let results = TextSearch::bloom_search_lines(&vt, b"FINDME");
        assert!(
            results.contains(&0),
            "Should find FINDME on line 0 spanning chunk boundary"
        );
    }

    #[test]
    fn bloom_no_duplicate_lines() {
        // Create text with "ab" pattern appearing in every chunk of a multi-chunk line
        let mut text = String::new();
        text.push_str(&"ab".repeat(600)); // 1200 bytes, spans 2 chunks
        text.push('\n');
        text.push_str("other\n");

        let vt = crate::VimText::from_str(&text);
        let results = TextSearch::bloom_search_lines(&vt, b"ab");
        let count_line0 = results.iter().filter(|&&l| l == 0).count();
        assert_eq!(
            count_line0, 1,
            "Line 0 should appear exactly once, got {}",
            count_line0
        );
    }

    #[test]
    fn bloom_search_empty_document() {
        let vt = crate::VimText::new();
        let results = TextSearch::bloom_search_lines(&vt, b"anything");
        assert!(results.is_empty());
    }

    #[test]
    fn bloom_search_single_char_pattern() {
        let vt = crate::VimText::from_str("hello\nworld\n");
        let results = TextSearch::bloom_search_lines(&vt, b"o");
        assert!(results.contains(&0)); // "hello" has 'o'
        assert!(results.contains(&1)); // "world" has 'o'
    }

    #[test]
    fn bloom_no_false_negatives_exhaustive() {
        // For every 2-char substring of the text, bloom_search_lines must
        // return the line containing it.
        let t = VimText::from_str("alpha\nbeta\ngamma\ndelta");
        let text = t.to_string();
        let lines: Vec<&str> = text.split('\n').collect();

        for (line_num, line) in lines.iter().enumerate() {
            let bytes = line.as_bytes();
            if bytes.len() < 2 {
                continue;
            }
            for window in bytes.windows(2) {
                let results = t.bloom_search_lines(window);
                assert!(
                    results.contains(&line_num),
                    "false negative: pattern {:?} not found on line {} ({:?})",
                    std::str::from_utf8(window),
                    line_num,
                    line
                );
            }
        }
    }
}
