//! Cursor-gravity search: bidirectional expanding wavefront.
//!
//! Interleaves forward/backward search in exponentially growing chunks
//! from the cursor position. The first match found is the nearest.

use crate::cache::Cache;
use crate::engine::dispatch;
use crate::engine::strategy::SearchMode;
use crate::engine::VimRegex;
use crate::ir::VimRegexError;
use crate::matchers::MatchContext;
use crate::VimMatch;

/// Initial chunk size for the expanding wavefront (bytes).
const INITIAL_CHUNK: usize = 256;

/// Growth factor for each expansion round (4x).
const GROWTH_FACTOR: usize = 4;

/// Maximum chunk size to prevent degenerate single-chunk scans.
const MAX_CHUNK: usize = 256 * 1024; // 256 KB

/// Find the match nearest to the cursor position.
pub(crate) fn find_nearest(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
) -> Result<Option<VimMatch>, VimRegexError> {
    let cursor = ctx.cursor.unwrap_or(0);
    let text = ctx.text;
    let text_len = text.len();

    if text_len == 0 {
        return Ok(None);
    }

    let cursor = cursor.min(text_len);

    let mut chunk_size = INITIAL_CHUNK;
    let mut fwd_pos = cursor;
    let mut bwd_end = cursor;

    loop {
        let fwd_exhausted = fwd_pos >= text_len;
        let bwd_exhausted = bwd_end == 0;

        if fwd_exhausted && bwd_exhausted {
            return Ok(None);
        }

        // Forward chunk: [fwd_pos, fwd_pos + chunk_size)
        let fwd_match = if !fwd_exhausted {
            let chunk_end = (fwd_pos + chunk_size).min(text_len);
            search_chunk_forward(regex, cache, ctx, fwd_pos, chunk_end)?
        } else {
            None
        };

        // Backward chunk: [bwd_end - chunk_size, bwd_end)
        let bwd_match = if !bwd_exhausted {
            let chunk_start = bwd_end.saturating_sub(chunk_size);
            search_chunk_backward(regex, cache, ctx, chunk_start, bwd_end)?
        } else {
            None
        };

        // Pick the nearest match.
        match (fwd_match, bwd_match) {
            (Some(f), Some(b)) => {
                let fwd_dist = f.range.start.abs_diff(cursor);
                let bwd_dist = b.range.start.abs_diff(cursor);
                return Ok(Some(if fwd_dist <= bwd_dist { f } else { b }));
            }
            (Some(f), None) => return Ok(Some(f)),
            (None, Some(b)) => return Ok(Some(b)),
            (None, None) => {
                // Expand the wavefront.
                if !fwd_exhausted {
                    fwd_pos = (fwd_pos + chunk_size).min(text_len);
                }
                if !bwd_exhausted {
                    bwd_end = bwd_end.saturating_sub(chunk_size);
                }
                chunk_size = (chunk_size * GROWTH_FACTOR).min(MAX_CHUNK);
            }
        }
    }
}

/// Search forward in [start, end) for the first match.
///
/// Delegates to the full strategy cascade starting at `start`, then checks
/// that the match starts within the `[start, end)` chunk boundary. If the
/// match falls past `end`, returns `None` so the wavefront expands and
/// retries with a larger chunk.
fn search_chunk_forward(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
    end: usize,
) -> Result<Option<VimMatch>, VimRegexError> {
    match dispatch::search_internal(regex, cache, ctx, start, SearchMode::Full)? {
        Some(m) if m.range.start < end => Ok(Some(m)),
        _ => Ok(None),
    }
}

/// Search backward in [start, end) for the last match.
fn search_chunk_backward(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
    end: usize,
) -> Result<Option<VimMatch>, VimRegexError> {
    regex.find_backward_in_range_with_cache(cache, ctx, start..end)
}

#[cfg(test)]
mod tests {
    use crate::engine::VimRegex;
    use crate::matchers::MatchContext;

    #[test]
    fn nearest_match_forward() {
        let re = VimRegex::new("world").unwrap();
        let ctx = MatchContext::with_cursor("hello world", 0);
        let m = re.find_nearest_simple(&ctx).unwrap().unwrap();
        assert_eq!(m.range, 6..11);
    }

    #[test]
    fn nearest_match_backward() {
        let re = VimRegex::new("hello").unwrap();
        let ctx = MatchContext::with_cursor("hello world", 11);
        let m = re.find_nearest_simple(&ctx).unwrap().unwrap();
        assert_eq!(m.range, 0..5);
    }

    #[test]
    fn nearest_match_at_cursor() {
        let re = VimRegex::new("world").unwrap();
        let ctx = MatchContext::with_cursor("hello world", 6);
        let m = re.find_nearest_simple(&ctx).unwrap().unwrap();
        assert_eq!(m.range, 6..11);
    }

    #[test]
    fn nearest_prefers_closer() {
        let re = VimRegex::new("x").unwrap();
        // Cursor at position 10. 'x' at 8 and 15 -> nearest is 8.
        let ctx = MatchContext::with_cursor("01234567x9012345x", 10);
        let m = re.find_nearest_simple(&ctx).unwrap().unwrap();
        assert_eq!(m.range.start, 8);
    }

    #[test]
    fn nearest_no_match() {
        let re = VimRegex::new("zzz").unwrap();
        let ctx = MatchContext::with_cursor("hello world", 5);
        assert!(re.find_nearest_simple(&ctx).unwrap().is_none());
    }

    #[test]
    fn nearest_empty_text() {
        let re = VimRegex::new("a").unwrap();
        let ctx = MatchContext::with_cursor("", 0);
        assert!(re.find_nearest_simple(&ctx).unwrap().is_none());
    }

    #[test]
    fn nearest_cursor_at_end() {
        let re = VimRegex::new("hello").unwrap();
        let ctx = MatchContext::with_cursor("hello", 5);
        let m = re.find_nearest_simple(&ctx).unwrap().unwrap();
        assert_eq!(m.range, 0..5);
    }

    #[test]
    fn nearest_with_regex_pattern() {
        // Use Vim magic \d\+ for one-or-more digits.
        let re = VimRegex::new(r"\d\+").unwrap();
        let ctx = MatchContext::with_cursor("abc 123 def 456 ghi", 10);
        let m = re.find_nearest_simple(&ctx).unwrap().unwrap();
        // Nearest digits to cursor=10 are "456" at offset 12 (dist=2)
        // vs "123" at offset 4 (dist=6).
        assert_eq!(m.range, 12..15);
    }
}
