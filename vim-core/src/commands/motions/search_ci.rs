//! Case-insensitive built-in substring search.
//!
//! When `ignorecase` is on (or `\c` modifier is present), the engine
//! needs to do case-folded matching. This module handles the Unicode
//! case-folding, byte-position mapping, and search orchestration.
//!
//! # Performance
//!
//! An ASCII fast path avoids allocating a case-folded copy of the entire
//! document. When both the document text and pattern are ASCII, matching
//! is done byte-by-byte with `eq_ignore_ascii_case`, yielding zero heap
//! allocation per search call. The Unicode fallback path is retained for
//! non-ASCII text.

use super::search::{is_at_word_end, is_at_word_start};
use super::types::MotionResult;
use crate::commands::helpers::next_char_boundary;
use crate::primitives::{Direction, Offset};

/// Case-insensitive variant of the built-in substring search.
///
/// Dispatches to an ASCII fast path (zero allocation) when both text and
/// pattern are pure ASCII, or falls back to the Unicode case-folding path.
pub(super) fn search_pattern_ci(
    text: &str,
    cursor: usize,
    pattern: &str,
    word_boundary: bool,
    direction: Direction,
    count: u32,
    wrap: bool,
) -> MotionResult {
    if pattern.is_empty() {
        return MotionResult::Error;
    }

    let need = count as usize;

    // Fast path: pure ASCII — no allocation needed.
    if text.is_ascii() && pattern.is_ascii() {
        return search_ascii_ci(text, cursor, pattern, word_boundary, direction, need, wrap);
    }

    // Slow path: Unicode case-folding with position map.
    search_unicode_ci(text, cursor, pattern, word_boundary, direction, need, wrap)
}

// ═══════════════════════════════════════════════════════════════════════════
// ASCII FAST PATH — zero allocation
// ═══════════════════════════════════════════════════════════════════════════

/// Perform case-insensitive search on pure-ASCII text without any heap allocation.
fn search_ascii_ci(
    text: &str,
    cursor: usize,
    pattern: &str,
    word_boundary: bool,
    direction: Direction,
    need: usize,
    wrap: bool,
) -> MotionResult {
    let pat_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();

    if direction.is_forward() {
        ascii_ci_forward(
            text,
            text_bytes,
            pat_bytes,
            cursor,
            word_boundary,
            pattern,
            need,
            wrap,
        )
    } else {
        ascii_ci_backward(
            text,
            text_bytes,
            pat_bytes,
            cursor,
            word_boundary,
            pattern,
            need,
            wrap,
        )
    }
}

/// Forward ASCII case-insensitive search with wrap support.
#[allow(
    clippy::too_many_arguments,
    reason = "search parameters are all orthogonal"
)]
fn ascii_ci_forward(
    text: &str,
    text_bytes: &[u8],
    pat_bytes: &[u8],
    cursor: usize,
    word_boundary: bool,
    pattern: &str,
    need: usize,
    wrap: bool,
) -> MotionResult {
    let start = (cursor + 1).min(text_bytes.len());
    let mut remaining = need;
    let mut last_pos = None;

    // Phase 1: search forward from after cursor
    let pat_len = pat_bytes.len();
    let text_len = text_bytes.len();
    let last_valid_fwd = if pat_len <= text_len {
        text_len - pat_len + 1
    } else {
        0
    };
    for i in start.min(last_valid_fwd)..last_valid_fwd {
        #[allow(
            clippy::indexing_slicing,
            reason = "i + pat_len <= text_len guaranteed by last_valid_fwd bound"
        )]
        if !text_bytes[i..i + pat_len].eq_ignore_ascii_case(pat_bytes) {
            continue;
        }
        if word_boundary && !check_word_boundary(text, i, pattern) {
            continue;
        }
        last_pos = Some(i);
        remaining -= 1;
        if remaining == 0 {
            return MotionResult::Position(Offset::new(i));
        }
    }

    // Phase 2: wrap around from beginning to cursor
    if wrap && remaining > 0 {
        let end = cursor.min(text_len);
        let last_valid_wrap = if pat_len <= end { end - pat_len + 1 } else { 0 };
        for i in 0..last_valid_wrap {
            #[allow(
                clippy::indexing_slicing,
                reason = "i + pat_len <= end guaranteed by last_valid_wrap bound"
            )]
            if !text_bytes[i..i + pat_len].eq_ignore_ascii_case(pat_bytes) {
                continue;
            }
            if word_boundary && !check_word_boundary(text, i, pattern) {
                continue;
            }
            last_pos = Some(i);
            remaining -= 1;
            if remaining == 0 {
                return MotionResult::Position(Offset::new(i));
            }
        }
    }

    match last_pos {
        Some(pos) => MotionResult::Position(Offset::new(pos)),
        None => MotionResult::Error,
    }
}

/// Backward ASCII case-insensitive search with wrap support.
#[allow(
    clippy::too_many_arguments,
    reason = "search parameters are all orthogonal"
)]
fn ascii_ci_backward(
    text: &str,
    text_bytes: &[u8],
    pat_bytes: &[u8],
    cursor: usize,
    word_boundary: bool,
    pattern: &str,
    need: usize,
    wrap: bool,
) -> MotionResult {
    let mut remaining = need;
    let mut last_pos = None;
    let pat_len = pat_bytes.len();
    let text_len = text_bytes.len();

    // Phase 1: search backward from cursor
    let search_end = cursor.min(text_len);
    if pat_len <= search_end {
        for i in (0..=search_end - pat_len).rev() {
            #[allow(
                clippy::indexing_slicing,
                reason = "i + pat_len <= search_end guaranteed by loop bound"
            )]
            if !text_bytes[i..i + pat_len].eq_ignore_ascii_case(pat_bytes) {
                continue;
            }
            if word_boundary && !check_word_boundary(text, i, pattern) {
                continue;
            }
            last_pos = Some(i);
            remaining -= 1;
            if remaining == 0 {
                return MotionResult::Position(Offset::new(i));
            }
        }
    }

    // Phase 2: wrap around from end to cursor
    if wrap && remaining > 0 {
        let after = (cursor + 1).min(text_len);
        if pat_len <= text_len && after <= text_len - pat_len {
            for i in (after..=text_len - pat_len).rev() {
                #[allow(
                    clippy::indexing_slicing,
                    reason = "i + pat_len <= text_len guaranteed by loop condition"
                )]
                if !text_bytes[i..i + pat_len].eq_ignore_ascii_case(pat_bytes) {
                    continue;
                }
                if word_boundary && !check_word_boundary(text, i, pattern) {
                    continue;
                }
                last_pos = Some(i);
                remaining -= 1;
                if remaining == 0 {
                    return MotionResult::Position(Offset::new(i));
                }
            }
        }
    }

    match last_pos {
        Some(pos) => MotionResult::Position(Offset::new(pos)),
        None => MotionResult::Error,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// UNICODE FALLBACK — allocates case-folded copy
// ═══════════════════════════════════════════════════════════════════════════

/// Unicode case-insensitive search using full case-fold + position map.
fn search_unicode_ci(
    text: &str,
    cursor: usize,
    pattern: &str,
    word_boundary: bool,
    direction: Direction,
    need: usize,
    wrap: bool,
) -> MotionResult {
    let (folded_text, pos_map) = case_fold_with_map(text);
    let folded_pattern: String = pattern.chars().flat_map(char::to_lowercase).collect();

    if folded_pattern.is_empty() {
        return MotionResult::Error;
    }

    let folded_cursor = original_to_folded_offset(cursor, text, &folded_text);

    let params = CiSearchParams {
        folded_text: &folded_text,
        folded_pattern: &folded_pattern,
        pos_map: &pos_map,
        text,
        pattern,
        word_boundary,
    };

    if direction.is_forward() {
        find_nth_forward(&params, folded_cursor, need, wrap)
    } else {
        find_nth_backward(&params, folded_cursor, need, wrap)
    }
}

/// Grouped parameters for Unicode case-insensitive search functions.
struct CiSearchParams<'a> {
    folded_text: &'a str,
    folded_pattern: &'a str,
    pos_map: &'a [usize],
    text: &'a str,
    pattern: &'a str,
    word_boundary: bool,
}

/// Find the Nth forward match in Unicode-folded text.
fn find_nth_forward(
    params: &CiSearchParams<'_>,
    folded_cursor: usize,
    need: usize,
    wrap: bool,
) -> MotionResult {
    let start = next_char_boundary(params.folded_text, folded_cursor);
    let mut remaining = need;
    let mut last_orig = None;

    for (folded_pos, _) in params
        .folded_text
        .get(start..)
        .unwrap_or("")
        .match_indices(params.folded_pattern)
    {
        let abs_folded = start + folded_pos;
        let orig_pos = folded_to_original_offset(abs_folded, params.pos_map);
        if params.word_boundary && !check_word_boundary(params.text, orig_pos, params.pattern) {
            continue;
        }
        last_orig = Some(orig_pos);
        remaining -= 1;
        if remaining == 0 {
            return MotionResult::Position(Offset::new(orig_pos));
        }
    }

    if wrap && remaining > 0 {
        for (folded_pos, _) in params
            .folded_text
            .get(..folded_cursor)
            .unwrap_or("")
            .match_indices(params.folded_pattern)
        {
            let orig_pos = folded_to_original_offset(folded_pos, params.pos_map);
            if params.word_boundary && !check_word_boundary(params.text, orig_pos, params.pattern) {
                continue;
            }
            last_orig = Some(orig_pos);
            remaining -= 1;
            if remaining == 0 {
                return MotionResult::Position(Offset::new(orig_pos));
            }
        }
    }

    match last_orig {
        Some(pos) => MotionResult::Position(Offset::new(pos)),
        None => MotionResult::Error,
    }
}

/// Find the Nth backward match in Unicode-folded text.
fn find_nth_backward(
    params: &CiSearchParams<'_>,
    folded_cursor: usize,
    need: usize,
    wrap: bool,
) -> MotionResult {
    let mut remaining = need;
    let mut last_orig = None;

    for (folded_pos, _) in params
        .folded_text
        .get(..folded_cursor)
        .unwrap_or("")
        .rmatch_indices(params.folded_pattern)
    {
        let orig_pos = folded_to_original_offset(folded_pos, params.pos_map);
        if params.word_boundary && !check_word_boundary(params.text, orig_pos, params.pattern) {
            continue;
        }
        last_orig = Some(orig_pos);
        remaining -= 1;
        if remaining == 0 {
            return MotionResult::Position(Offset::new(orig_pos));
        }
    }

    if wrap && remaining > 0 {
        let after = next_char_boundary(params.folded_text, folded_cursor);
        for (folded_pos, _) in params
            .folded_text
            .get(after..)
            .unwrap_or("")
            .rmatch_indices(params.folded_pattern)
        {
            let abs_folded = after + folded_pos;
            let orig_pos = folded_to_original_offset(abs_folded, params.pos_map);
            if params.word_boundary && !check_word_boundary(params.text, orig_pos, params.pattern) {
                continue;
            }
            last_orig = Some(orig_pos);
            remaining -= 1;
            if remaining == 0 {
                return MotionResult::Position(Offset::new(orig_pos));
            }
        }
    }

    match last_orig {
        Some(pos) => MotionResult::Position(Offset::new(pos)),
        None => MotionResult::Error,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════════════════════════

/// Check word boundary at an original-text position.
fn check_word_boundary(text: &str, orig_pos: usize, pattern: &str) -> bool {
    let wc = crate::primitives::WordCharSet::default_vim();
    is_at_word_start(text, orig_pos, &wc)
        && is_at_word_end(
            text,
            orig_pos + pattern_original_len(text, orig_pos, pattern),
            &wc,
        )
}

/// Case-fold text and build a byte-position mapping from folded -> original.
fn case_fold_with_map(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(text.len());
    let mut pos_map = Vec::with_capacity(text.len());

    for (orig_idx, ch) in text.char_indices() {
        for lower_ch in ch.to_lowercase() {
            let start = folded.len();
            folded.push(lower_ch);
            for _ in start..folded.len() {
                pos_map.push(orig_idx);
            }
        }
    }

    (folded, pos_map)
}

/// Map an original byte offset to the corresponding folded byte offset.
fn original_to_folded_offset(orig_offset: usize, original: &str, folded: &str) -> usize {
    let mut orig_pos = 0;
    let mut fold_pos = 0;

    for ch in original.chars() {
        if orig_pos >= orig_offset {
            break;
        }
        orig_pos += ch.len_utf8();
        for lower_ch in ch.to_lowercase() {
            fold_pos += lower_ch.len_utf8();
        }
    }

    fold_pos.min(folded.len())
}

/// Map a folded byte offset back to the original byte offset.
#[inline]
fn folded_to_original_offset(folded_offset: usize, pos_map: &[usize]) -> usize {
    pos_map
        .get(folded_offset)
        .or_else(|| pos_map.last())
        .copied()
        .unwrap_or(0)
}

/// Compute the original-text byte length for a pattern match at `orig_start`.
///
/// Each original character may case-fold to one *or more* characters in the
/// pattern (e.g., U+0130 LATIN CAPITAL LETTER I WITH DOT ABOVE folds to
/// `'i'` followed by `'\u{0307}'`). We walk the original text, subtracting
/// each character's folded width from the remaining pattern char count, so
/// the mapping is correct even for 1:N case folding.
fn pattern_original_len(text: &str, orig_start: usize, pattern: &str) -> usize {
    let text_slice = text.get(orig_start..).unwrap_or("");
    let mut text_chars = text_slice.chars();
    let mut consumed_bytes = 0;
    let mut remaining_pattern_chars = pattern.chars().count();

    while remaining_pattern_chars > 0 {
        let Some(text_char) = text_chars.next() else {
            break;
        };
        consumed_bytes += text_char.len_utf8();

        // Count how many folded chars this text char contributes.
        // For most chars this is 1; for U+0130 (Turkish İ) it's 2.
        let folded_count = text_char.to_lowercase().count();
        remaining_pattern_chars = remaining_pattern_chars.saturating_sub(folded_count);
    }

    consumed_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_original_len_turkish_i() {
        // U+0130 LATIN CAPITAL LETTER I WITH DOT ABOVE
        // to_lowercase produces 'i' + '\u{0307}' (2 chars)
        let text = "\u{0130}xyz";
        let pattern = "i\u{0307}"; // the folded form
        assert_eq!(pattern_original_len(text, 0, pattern), "\u{0130}".len());
    }

    #[test]
    fn pattern_original_len_ascii_unchanged() {
        let text = "Hello World";
        let pattern = "hello";
        assert_eq!(pattern_original_len(text, 0, pattern), 5);
    }
}
