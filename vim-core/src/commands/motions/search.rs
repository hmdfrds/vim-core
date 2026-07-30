//! Search motions: /, ?, n, N, *, #, g*, g#
//!
//! Pattern-based navigation through the document.
//!
//! ## Motions
//!
//! | Keys | Description |
//! |------|-------------|
//! | `/{pattern}` | Search forward for pattern |
//! | `?{pattern}` | Search backward for pattern |
//! | `n` | Next match (same direction) |
//! | `N` | Next match (opposite direction) |
//! | `*` | Search forward for word under cursor |
//! | `#` | Search backward for word under cursor |
//! | `g*` | Partial match forward |
//! | `g#` | Partial match backward |
//!
//! ## Design
//!
//! Matching is tried in three tiers. If the embedder installed a
//! `SearchProvider`, that wins — it lets the editor reuse its own engine.
//! Otherwise the pattern is compiled by the bundled regex engine, which
//! understands Vim syntax (`\<`, `\>`, `\c`, `\v`, …). If it will not
//! compile, a plain substring scan is the last resort so that a
//! malformed pattern still behaves like a literal search.

use super::types::{MotionContext, MotionResult, SearchMotion};
pub(super) use super::word_boundary::strip_word_boundaries;
use crate::commands::helpers::next_char_boundary;
use crate::primitives::{Direction, MotionInclusivity, Offset, SearchFlags};
use crate::state::SearchOffset;
use compact_str::CompactString;
impl SearchMotion {
    /// Compute the target offset (requires search state in context).
    ///
    /// When a `SearchProvider` is available (via `ctx.providers.search`),
    /// delegates to it for full regex matching. Otherwise the pattern is
    /// compiled by the built-in `VimRegex` engine, and only a pattern that
    /// fails to compile falls back to substring matching.
    pub fn compute(&self, ctx: &MotionContext<'_>) -> MotionResult {
        let cursor = ctx.cursor.get();

        // Fold-aware search: snap cursor to fold boundary in search direction
        // so search starts from outside the fold.
        let search_cursor = if let Some(fold) = ctx.providers.fold {
            let dir = match self {
                Self::NextMatch
                | Self::WordUnderCursor
                | Self::PartialWord
                | Self::SearchForward => ctx.search_direction,
                Self::PrevMatch
                | Self::WordUnderCursorBack
                | Self::PartialWordBack
                | Self::SearchBackward => ctx.search_direction.reverse(),
            };
            crate::commands::helpers::fold_snap(ctx.text, cursor, dir, fold)
        } else {
            cursor
        };

        match self {
            Self::NextMatch | Self::PrevMatch => {
                let pattern = match ctx.search_pattern {
                    Some(p) if !p.is_empty() => p,
                    _ => return MotionResult::Error,
                };

                let direction = match self {
                    Self::NextMatch => ctx.search_direction,
                    Self::PrevMatch => ctx.search_direction.reverse(),
                    // Outer match constrains to NextMatch | PrevMatch only
                    _ => return MotionResult::Error,
                };

                let result = find_nth_match_at(ctx, pattern, direction, ctx.count, search_cursor);
                let result = apply_search_offset(result, ctx.text, pattern, &ctx.search_offset);

                // Retry if stuck on same position (search offset moved cursor back to start)
                if let MotionResult::Position(pos) = &result {
                    if pos.get() == search_cursor {
                        let retry = find_nth_match_at(
                            ctx,
                            pattern,
                            direction,
                            ctx.count + 1,
                            search_cursor,
                        );
                        return apply_search_offset(retry, ctx.text, pattern, &ctx.search_offset);
                    }
                }

                result
            }

            Self::WordUnderCursor | Self::WordUnderCursorBack => {
                let word_chars = ctx.options.word_char_set();
                let (word, effective_cursor) =
                    super::word_boundary::word_under_cursor_or_next(ctx.text, cursor, word_chars);
                if word.is_empty() {
                    return MotionResult::Error;
                }

                // Wrap in \<...\> for whole-word matching (Vim * and #).
                // Use CompactString SSO (24 bytes inline) to avoid heap
                // allocation for typical word lengths (<=20 chars).
                let mut bounded = CompactString::with_capacity(word.len() + 4);
                bounded.push_str("\\<");
                bounded.push_str(word);
                bounded.push_str("\\>");

                let direction = if matches!(self, Self::WordUnderCursor) {
                    Direction::Forward
                } else {
                    Direction::Backward
                };

                // Vim's * and # search from the word boundary, not from cursor position.
                // For * (forward), search from word end to skip current match.
                // For # (backward), search from word start to skip current match.
                let word_start = ctx.text[..effective_cursor]
                    .rfind(|c: char| !word_chars.contains(c))
                    .map_or(0, |i| i + 1);
                let word_end = word_start + word.len();
                let search_cursor = if direction.is_forward() {
                    word_end.min(ctx.text.len())
                } else {
                    word_start
                };

                find_nth_match_at(ctx, &bounded, direction, ctx.count, search_cursor)
            }

            Self::PartialWord | Self::PartialWordBack => {
                let word_chars = ctx.options.word_char_set();
                let (word, _) =
                    super::word_boundary::word_under_cursor_or_next(ctx.text, cursor, word_chars);
                if word.is_empty() {
                    return MotionResult::Error;
                }

                let direction = if matches!(self, Self::PartialWord) {
                    Direction::Forward
                } else {
                    Direction::Backward
                };
                find_nth_match_at(ctx, word, direction, ctx.count, search_cursor)
            }

            Self::SearchForward | Self::SearchBackward => {
                // These require command-line input - emit effect instead
                MotionResult::NoMotion
            }
        }
    }

    /// Motion type.
    #[must_use]
    pub const fn motion_type(&self) -> MotionInclusivity {
        MotionInclusivity::Exclusive
    }
}

/// Find the nth match using the best available strategy, starting from `cursor`.
///
/// Computes [`SearchFlags`] from `VimOptions` + per-pattern modifiers
/// (`\c`/`\C`/`\v`/`\V`), strips those modifiers from the pattern,
/// then delegates to either the host's `SearchProvider` or the built-in
/// substring matcher.
fn find_nth_match_at(
    ctx: &MotionContext<'_>,
    pattern: &str,
    direction: Direction,
    count: u32,
    cursor: usize,
) -> MotionResult {
    // Strip \c/\C/\v/\V modifiers from the pattern and compute flags.
    let (stripped, modifiers) = parse_search_modifiers(pattern);
    let flags = compute_search_flags(ctx.options, &stripped, &modifiers);

    if let Some(search) = ctx.providers.search {
        return search_via_provider(
            search,
            &stripped,
            cursor,
            direction,
            count,
            &flags,
            ctx.text.len(),
        );
    }

    if let Some(result) = try_bloom_regex_search(ctx, &stripped, cursor, direction, count, &flags) {
        return result;
    }

    search_pattern(ctx.text, cursor, &stripped, direction, count, &flags)
}

/// Apply a post-match cursor offset (`/pat/e`, `/pat/+3`, etc.).
///
/// Adjusts the match position returned by `find_nth_match` according to the
/// active [`SearchOffset`]. For `End(n)`, the cursor moves to the last byte
/// of the match ± n chars. For `Start(n)`, it shifts from match start.
/// For `Lines(n)`, it jumps to the start of a line n below/above the match.
fn apply_search_offset(
    result: MotionResult,
    text: &str,
    pattern: &str,
    offset: &SearchOffset,
) -> MotionResult {
    if offset.is_none() {
        return result;
    }
    let match_start = match &result {
        MotionResult::Position(pos) => pos.get(),
        MotionResult::Range { start, .. } => start.get(),
        _ => return result,
    };

    let new_pos = match *offset {
        SearchOffset::End(n) => {
            let match_end = refind_match_end(text, pattern, match_start);
            let end_pos = if match_end > match_start {
                crate::primitives::text_util::prev_char_boundary(text, match_end)
            } else {
                match_start
            };
            shift_by_chars(text, end_pos, n)
        }
        SearchOffset::Start(n) => shift_by_chars(text, match_start, n),
        SearchOffset::Lines(n) => shift_by_lines(text, match_start, n),
    };
    MotionResult::Position(Offset::new(clamp_to_text(text, new_pos)))
}

/// Re-run a single regex find at `match_start` to obtain the actual match end.
///
/// This avoids propagating the match range through the entire call chain.
/// Falls back to pattern byte-length if the regex fails to compile or match.
fn refind_match_end(text: &str, pattern: &str, match_start: usize) -> usize {
    let (stripped, modifiers) = parse_search_modifiers(pattern);
    let magic = modifiers
        .magic_override
        .unwrap_or(crate::primitives::MagicMode::Magic);
    if let Ok(re) = crate::regex::VimRegex::cached_with_magic(&stripped, magic) {
        let ctx = crate::regex::MatchContext::builder(text)
            .cursor(match_start)
            .case_sensitive(true)
            .build();
        if let Ok(Some(m)) = re.find_at(&ctx, match_start) {
            if m.range.start == match_start {
                return m.range.end;
            }
        }
    }
    // Fallback: use stripped pattern byte-length as approximation.
    let (actual_pat, _) = strip_word_boundaries(&stripped);
    match_start + actual_pat.len()
}

/// Shift a byte position by `n` characters (positive = forward, negative = backward).
fn shift_by_chars(text: &str, pos: usize, n: i32) -> usize {
    if n == 0 {
        return pos;
    }
    let clamped = pos.min(text.len());
    let abs_n = usize::try_from(n.unsigned_abs()).unwrap_or(usize::MAX);
    if n > 0 {
        // Fallback to last char start for multi-byte safety.
        let last = crate::primitives::text_util::prev_char_boundary(text, text.len());
        text.get(clamped..)
            .and_then(|s| s.char_indices().nth(abs_n))
            .map_or(last, |(idx, _)| clamped + idx)
    } else {
        text.get(..clamped)
            .and_then(|s| s.char_indices().rev().nth(abs_n.saturating_sub(1)))
            .map_or(0, |(idx, _)| idx)
    }
}

/// Shift a byte position by `n` lines, returning the start of the target line.
fn shift_by_lines(text: &str, pos: usize, n: i32) -> usize {
    let current_line = text
        .get(..pos.min(text.len()))
        .map_or(0, |s| s.bytes().filter(|&b| b == b'\n').count());
    let magnitude = usize::try_from(n.unsigned_abs()).unwrap_or(usize::MAX);
    let target_line = if n >= 0 {
        current_line.saturating_add(magnitude)
    } else {
        current_line.saturating_sub(magnitude)
    };
    nth_line_start(text, target_line)
}

/// Find the byte offset of the start of the Nth line (0-indexed).
fn nth_line_start(text: &str, target: usize) -> usize {
    if target == 0 {
        return 0;
    }
    let mut line = 0;
    for (i, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            line += 1;
            if line == target {
                return (i + 1).min(text.len());
            }
        }
    }
    // Target exceeds line count — return start of last line.
    crate::commands::helpers::line_start_for_offset(text, text.len())
}

/// Clamp a byte position to a valid text offset.
#[inline]
const fn clamp_to_text(text: &str, pos: usize) -> usize {
    if text.is_empty() {
        return 0;
    }
    if pos >= text.len() {
        crate::primitives::text_util::prev_char_boundary(text, text.len())
    } else {
        pos
    }
}

/// Use the shell's `SearchProvider` for regex-powered search.
///
/// Steps forward/backward through matches, one at a time, to honor count.
/// The provider handles wrapping internally based on `flags.wrap`.
fn search_via_provider(
    provider: &dyn crate::document::SearchProvider,
    pattern: &str,
    cursor: usize,
    direction: Direction,
    count: u32,
    flags: &SearchFlags,
    text_len: usize,
) -> MotionResult {
    let count = count as usize;
    let mut from = cursor;

    for i in 0..count {
        // Always advance past current position to avoid re-finding the same match.
        let search_from = if direction.is_forward() {
            from + 1
        } else {
            from.saturating_sub(1)
        };

        match provider.find_match(pattern, search_from, direction, flags) {
            Some(range) => from = range.start().get().min(text_len),
            None => {
                return if i > 0 {
                    // Got some matches but not enough — return last found
                    MotionResult::Position(Offset::new(from))
                } else {
                    MotionResult::Error
                };
            }
        }
    }

    MotionResult::Position(Offset::new(from))
}

/// Find the nth occurrence of pattern in text using VimRegex.
///
/// The pattern is compiled with `VimRegex::cached_with_magic()`, which
/// handles `\<`, `\>`, `\c`, `\C`, `\v`, `\V`, `\m`, `\M` natively.
/// Falls back to substring matching if the pattern is not a valid regex.
///
/// Respects `flags.case_sensitive` and `flags.wrap`.
///
/// Uses VimRegex's 8-entry LRU compilation cache to avoid recompiling
/// the regex when the pattern and magic mode have not changed (common
/// with `n`/`N` repeat and incsearch).
fn search_pattern(
    text: &str,
    cursor: usize,
    pattern: &str,
    direction: Direction,
    count: u32,
    flags: &SearchFlags,
) -> MotionResult {
    if pattern.is_empty() || text.is_empty() {
        return MotionResult::Error;
    }

    let magic = flags.magic();

    match crate::regex::VimRegex::cached_with_magic(pattern, magic) {
        Ok(re) => super::search_regex::search_with_regex(
            &re,
            text,
            cursor,
            direction,
            count,
            flags,
            #[cfg(not(target_arch = "wasm32"))]
            Some(std::time::Instant::now() + std::time::Duration::from_secs(5)),
        ),
        Err(_) => search_pattern_substring(text, cursor, pattern, direction, count, flags),
    }
}

/// Check if a Vim regex pattern might match across line boundaries.
///
/// Detects `\n` (newline) and `\_` (multi-line character class prefix),
/// accounting for `\\` (escaped backslash). When true, bloom per-line
/// search cannot be used — fall back to full-text search.
pub(crate) fn pattern_might_span_lines(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'n' | b'_' => return true,
                _ => {}
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// Try bloom-accelerated regex search when a VimText tree is available.
///
/// Returns `Some(result)` when bloom can produce an answer, `None` when
/// bloom can't help (no tree, case-insensitive, no extractable literal,
/// or multi-line pattern).
/// Uses VimRegex's 8-entry LRU compilation cache.
fn try_bloom_regex_search(
    ctx: &MotionContext<'_>,
    pattern: &str,
    cursor: usize,
    direction: Direction,
    count: u32,
    flags: &SearchFlags,
) -> Option<MotionResult> {
    let tree = ctx.tree?;

    if !flags.case_sensitive() || pattern.is_empty() || ctx.text.is_empty() {
        return None;
    }

    if pattern_might_span_lines(pattern) {
        return None;
    }

    let magic = flags.magic();
    let re = crate::regex::VimRegex::cached_with_magic(pattern, magic).ok()?;

    super::search_regex::bloom_search_with_regex(
        &re,
        tree,
        cursor,
        direction,
        count,
        flags,
        #[cfg(not(target_arch = "wasm32"))]
        Some(std::time::Instant::now() + std::time::Duration::from_secs(5)),
    )
}

/// Substring-based fallback when pattern is not a valid VimRegex.
fn search_pattern_substring(
    text: &str,
    cursor: usize,
    pattern: &str,
    direction: Direction,
    count: u32,
    flags: &SearchFlags,
) -> MotionResult {
    let (actual_pattern, word_boundary) = strip_word_boundaries(pattern);
    let case_insensitive = !flags.case_sensitive();

    if case_insensitive {
        return search_pattern_ci(
            text,
            cursor,
            actual_pattern,
            word_boundary,
            direction,
            count,
            flags.wrap(),
        );
    }

    let count = count as usize;

    if direction.is_forward() {
        substring_forward(
            text,
            cursor,
            actual_pattern,
            word_boundary,
            count,
            flags.wrap(),
        )
    } else {
        substring_backward(
            text,
            cursor,
            actual_pattern,
            word_boundary,
            count,
            flags.wrap(),
        )
    }
}

/// Forward substring search with count and optional wrapping.
fn substring_forward(
    text: &str,
    cursor: usize,
    pattern: &str,
    word_boundary: bool,
    count: usize,
    wrap: bool,
) -> MotionResult {
    let start = next_char_boundary(text, cursor);
    let mut found_count = 0;

    for pos in find_matches_forward(&text[start..], pattern, word_boundary, text, start)
        .map(|pos| start + pos)
    {
        found_count += 1;
        if found_count == count {
            return MotionResult::Position(Offset::new(pos));
        }
    }

    if wrap {
        for pos in find_matches_forward(&text[..cursor], pattern, word_boundary, text, 0) {
            found_count += 1;
            if found_count == count {
                return MotionResult::Position(Offset::new(pos));
            }
        }
    }

    if found_count > 0 {
        find_matches_forward(text, pattern, word_boundary, text, 0)
            .last()
            .map_or(MotionResult::Error, |p| {
                MotionResult::Position(Offset::new(p))
            })
    } else {
        MotionResult::Error
    }
}

/// Backward substring search with count and optional wrapping.
fn substring_backward(
    text: &str,
    cursor: usize,
    pattern: &str,
    word_boundary: bool,
    count: usize,
    wrap: bool,
) -> MotionResult {
    let mut found_count = 0;

    for pos in find_matches_backward(&text[..cursor], pattern, word_boundary, text, 0) {
        found_count += 1;
        if found_count == count {
            return MotionResult::Position(Offset::new(pos));
        }
    }

    if wrap {
        let base = next_char_boundary(text, cursor);
        for pos in find_matches_backward(&text[base..], pattern, word_boundary, text, base)
            .map(|p| base + p)
        {
            found_count += 1;
            if found_count == count {
                return MotionResult::Position(Offset::new(pos));
            }
        }
    }

    if found_count > 0 {
        find_matches_backward(&text[..cursor], pattern, word_boundary, text, 0)
            .next()
            .map_or(MotionResult::Error, |p| {
                MotionResult::Position(Offset::new(p))
            })
    } else {
        MotionResult::Error
    }
}

/// Check if position in full text is at a word boundary (start).
#[inline]
pub(super) fn is_at_word_start(
    full_text: &str,
    abs_pos: usize,
    word_chars: &crate::primitives::WordCharSet,
) -> bool {
    if abs_pos == 0 {
        return true;
    }
    let prev_char = full_text[..abs_pos].chars().next_back();
    prev_char.is_none_or(|c| !word_chars.contains(c))
}

/// Check if position after a match in full text is at a word boundary (end).
#[inline]
pub(super) fn is_at_word_end(
    full_text: &str,
    abs_end: usize,
    word_chars: &crate::primitives::WordCharSet,
) -> bool {
    if abs_end >= full_text.len() {
        return true;
    }
    let next_char = full_text[abs_end..].chars().next();
    next_char.is_none_or(|c| !word_chars.contains(c))
}

/// Iterator over forward matches of pattern in text, optionally checking word boundaries.
pub(super) fn find_matches_forward<'a>(
    text: &'a str,
    pattern: &'a str,
    word_boundary: bool,
    full_text: &'a str,
    base_offset: usize,
) -> impl Iterator<Item = usize> + 'a {
    let wc = crate::primitives::WordCharSet::default_vim();
    text.match_indices(pattern)
        .map(|(pos, _)| pos)
        .filter(move |&pos| {
            if !word_boundary {
                return true;
            }
            let abs_pos = base_offset + pos;
            is_at_word_start(full_text, abs_pos, &wc)
                && is_at_word_end(full_text, abs_pos + pattern.len(), &wc)
        })
}

/// Iterator over backward matches (reverse order), optionally checking word boundaries.
pub(super) fn find_matches_backward<'a>(
    text: &'a str,
    pattern: &'a str,
    word_boundary: bool,
    full_text: &'a str,
    base_offset: usize,
) -> impl Iterator<Item = usize> + 'a {
    let wc = crate::primitives::WordCharSet::default_vim();
    text.rmatch_indices(pattern)
        .map(|(pos, _)| pos)
        .filter(move |&pos| {
            if !word_boundary {
                return true;
            }
            let abs_pos = base_offset + pos;
            is_at_word_start(full_text, abs_pos, &wc)
                && is_at_word_end(full_text, abs_pos + pattern.len(), &wc)
        })
}

// Delegate to sub-modules for case-insensitive search and modifier parsing.
use super::search_ci::search_pattern_ci;
use super::search_modifiers::{compute_search_flags, parse_search_modifiers};

/// Result of counting search matches, including completeness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchCountResult {
    /// 1-based index of the current match.
    pub current: u32,
    /// Total number of matches found (may be partial if timed out or capped).
    pub total: u32,
    /// Whether the count covers all matches in the document.
    /// `false` when timed out or when total exceeded `maxcount`.
    pub complete: bool,
}

/// Default maxcount for search match counting (matches Vim's default).
const SEARCH_COUNT_MAXCOUNT: u32 = 99;

/// How often (in matches) to check the deadline for timeout.
const TIMEOUT_CHECK_INTERVAL: u32 = 1000;

/// Count all matches of pattern in text and find the 1-based index of the
/// match at `landed_pos`. Returns [`SearchCountResult`] or `None`.
///
/// ## Timeout
///
/// If `deadline` is `Some`, the function checks elapsed time every
/// ~`TIMEOUT_CHECK_INTERVAL` matches. On timeout, returns a partial
/// result with `complete: false`.
///
/// ## Maxcount
///
/// When total exceeds `SEARCH_COUNT_MAXCOUNT`, returns early with
/// `complete: false`.
///
/// Used by the dispatch layer to emit `SearchMatchInfo` effects.
#[must_use]
pub fn count_search_matches(
    text: &str,
    pattern: &str,
    landed_pos: usize,
    #[cfg(not(target_arch = "wasm32"))] deadline: Option<std::time::Instant>,
) -> Option<SearchCountResult> {
    let (stripped, modifiers) = parse_search_modifiers(pattern);
    let magic = modifiers
        .magic_override
        .unwrap_or(crate::primitives::MagicMode::Magic);
    let (actual_pat, word_boundary) = strip_word_boundaries(&stripped);
    if actual_pat.is_empty() {
        return None;
    }

    // Try regex first (uses the full pattern with \<\> boundaries intact,
    // matching what the actual search uses), fall back to substring.
    let regex_matches: Option<Vec<usize>> =
        crate::regex::VimRegex::cached_with_magic(&stripped, magic)
            .ok()
            .and_then(|re| {
                let ctx = crate::regex::MatchContext::builder(text)
                    .cursor(landed_pos)
                    .case_sensitive(true)
                    .build();
                re.find_all(&ctx)
                    .ok()
                    .map(|matches| matches.into_iter().map(|m| m.range.start).collect())
            });

    let mut total: u32 = 0;
    let mut current: u32 = 0;
    let mut complete = true;
    let mut nearest_dist: usize = usize::MAX;

    let iter: Box<dyn Iterator<Item = usize>> = match &regex_matches {
        Some(positions) => Box::new(positions.iter().copied()),
        None => Box::new(find_matches_forward(
            text,
            actual_pat,
            word_boundary,
            text,
            0,
        )),
    };

    for pos in iter {
        total = total.saturating_add(1);
        if pos == landed_pos {
            current = total;
        } else {
            // Track nearest match for when exact position not found
            let dist = pos.abs_diff(landed_pos);
            if dist < nearest_dist {
                nearest_dist = dist;
                if current == 0 {
                    current = total;
                }
            }
        }

        if total > SEARCH_COUNT_MAXCOUNT {
            complete = false;
            break;
        }

        #[cfg(not(target_arch = "wasm32"))]
        if total.is_multiple_of(TIMEOUT_CHECK_INTERVAL) {
            if let Some(dl) = deadline {
                if std::time::Instant::now() >= dl {
                    complete = false;
                    break;
                }
            }
        }
    }
    if total == 0 {
        return None;
    }
    if current == 0 {
        current = 1; // absolute fallback if no match found near landed_pos
    }
    Some(SearchCountResult {
        current,
        total,
        complete,
    })
}

/// WASM overload — same regex-first logic, no deadline parameter.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn count_search_matches(
    text: &str,
    pattern: &str,
    landed_pos: usize,
) -> Option<SearchCountResult> {
    let (stripped, modifiers) = parse_search_modifiers(pattern);
    let magic = modifiers
        .magic_override
        .unwrap_or(crate::primitives::MagicMode::Magic);
    let (actual_pat, word_boundary) = strip_word_boundaries(&stripped);
    if actual_pat.is_empty() {
        return None;
    }

    let regex_matches: Option<Vec<usize>> =
        crate::regex::VimRegex::cached_with_magic(&stripped, magic)
            .ok()
            .and_then(|re| {
                let ctx = crate::regex::MatchContext::builder(text)
                    .cursor(landed_pos)
                    .case_sensitive(true)
                    .build();
                re.find_all(&ctx)
                    .ok()
                    .map(|matches| matches.into_iter().map(|m| m.range.start).collect())
            });

    let mut total: u32 = 0;
    let mut current: u32 = 0;
    let mut complete = true;
    let mut nearest_dist: usize = usize::MAX;

    let iter: Box<dyn Iterator<Item = usize>> = match &regex_matches {
        Some(positions) => Box::new(positions.iter().copied()),
        None => Box::new(find_matches_forward(
            text,
            actual_pat,
            word_boundary,
            text,
            0,
        )),
    };

    for pos in iter {
        total = total.saturating_add(1);
        if pos == landed_pos {
            current = total;
        } else {
            let dist = if pos > landed_pos {
                pos - landed_pos
            } else {
                landed_pos - pos
            };
            if dist < nearest_dist {
                nearest_dist = dist;
                if current == 0 {
                    current = total;
                }
            }
        }
        if total > SEARCH_COUNT_MAXCOUNT {
            complete = false;
            break;
        }
    }
    if total == 0 {
        return None;
    }
    if current == 0 {
        current = 1;
    }
    Some(SearchCountResult {
        current,
        total,
        complete,
    })
}

#[cfg(test)]
mod tests {
    use super::super::word_boundary::word_under_cursor;
    use super::*;
    use crate::primitives::Direction;

    #[test]
    fn search_forward_finds_next_match() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("foo bar foo baz foo", Offset::new(0), 1, &opts)
            .with_search("foo", Direction::Forward);
        let result = SearchMotion::NextMatch.compute(&ctx);
        // Should find second "foo" at position 8 (after cursor)
        assert_eq!(result, MotionResult::Position(Offset::new(8)));
    }

    #[test]
    fn search_backward_finds_previous_match() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("foo bar foo baz foo", Offset::new(15), 1, &opts)
            .with_search("foo", Direction::Backward);
        let result = SearchMotion::NextMatch.compute(&ctx);
        // Searching backward from position 15, should find "foo" at 8
        assert_eq!(result, MotionResult::Position(Offset::new(8)));
    }

    #[test]
    fn word_under_cursor_basic() {
        let wc = crate::primitives::WordCharSet::default_vim();
        assert_eq!(word_under_cursor("hello world", 0, &wc), "hello");
        assert_eq!(word_under_cursor("hello world", 6, &wc), "world");
        assert_eq!(word_under_cursor("hello world", 5, &wc), ""); // space
    }

    #[test]
    fn no_pattern_returns_failed() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("foo bar", Offset::new(0), 1, &opts);
        let result = SearchMotion::NextMatch.compute(&ctx);
        assert_eq!(result, MotionResult::Error);
    }

    #[test]
    fn search_forward_count_2_skips_first_match() {
        // "foo bar foo baz foo" — searching forward from 0 with count=2
        // should skip pos 8 and land on pos 16
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("foo bar foo baz foo", Offset::new(0), 2, &opts)
            .with_search("foo", Direction::Forward);
        let result = SearchMotion::NextMatch.compute(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(16)));
    }

    #[test]
    fn search_backward_count_2() {
        // "foo bar foo baz foo" — searching backward from 16 with count=2
        // should skip pos 8 and land on pos 0
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("foo bar foo baz foo", Offset::new(16), 2, &opts)
            .with_search("foo", Direction::Backward);
        let result = SearchMotion::NextMatch.compute(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    /// Mock SearchProvider that does simple substring matching (like the built-in).
    struct MockSearchProvider {
        text: String,
    }

    impl crate::document::SearchProvider for MockSearchProvider {
        fn find_match(
            &self,
            pattern: &str,
            from: usize,
            direction: Direction,
            flags: &crate::primitives::SearchFlags,
        ) -> Option<crate::primitives::Range> {
            let text = &self.text;
            if direction.is_forward() {
                // Search forward from `from`, wrapping if allowed
                if let Some(pos) = text[from.min(text.len())..].find(pattern) {
                    return Some(crate::primitives::Range::from_raw(
                        from + pos,
                        from + pos + pattern.len(),
                    ));
                }
                if flags.wrap() {
                    if let Some(pos) = text[..from.min(text.len())].find(pattern) {
                        return Some(crate::primitives::Range::from_raw(pos, pos + pattern.len()));
                    }
                }
            } else {
                // Search backward from `from`, wrapping if allowed
                if let Some(pos) = text[..from.min(text.len())].rfind(pattern) {
                    return Some(crate::primitives::Range::from_raw(pos, pos + pattern.len()));
                }
                if flags.wrap() {
                    if let Some(pos) = text[from.min(text.len())..].rfind(pattern) {
                        return Some(crate::primitives::Range::from_raw(
                            from + pos,
                            from + pos + pattern.len(),
                        ));
                    }
                }
            }
            None
        }
    }

    #[test]
    fn search_via_provider_count_2_forward() {
        // Proves the count>1 bug is fixed: with provider, 2n should land on second match
        let provider = MockSearchProvider {
            text: "foo bar foo baz foo".to_string(),
        };
        let opts = crate::primitives::VimOptions::default();
        let providers = crate::document::Providers::new().with_search(&provider);
        let ctx = MotionContext::new("foo bar foo baz foo", Offset::new(0), 2, &opts)
            .with_search("foo", Direction::Forward)
            .with_providers(providers);
        let result = SearchMotion::NextMatch.compute(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(16)));
    }

    #[test]
    fn search_via_provider_count_2_backward() {
        let opts = crate::primitives::VimOptions::default();
        let provider = MockSearchProvider {
            text: "foo bar foo baz foo".to_string(),
        };
        let providers = crate::document::Providers::new().with_search(&provider);
        let ctx = MotionContext::new("foo bar foo baz foo", Offset::new(16), 2, &opts)
            .with_search("foo", Direction::Backward)
            .with_providers(providers);
        let result = SearchMotion::NextMatch.compute(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn search_via_provider_count_exceeds_matches() {
        let opts = crate::primitives::VimOptions::default();
        // Only 3 "foo" in text, count=5 — should return last found match
        let provider = MockSearchProvider {
            text: "foo bar foo baz foo".to_string(),
        };
        let providers = crate::document::Providers::new().with_search(&provider);
        let ctx = MotionContext::new("foo bar foo baz foo", Offset::new(0), 5, &opts)
            .with_search("foo", Direction::Forward)
            .with_providers(providers);
        let result = SearchMotion::NextMatch.compute(&ctx);
        // Should find at least some matches, returning last found position
        assert!(matches!(result, MotionResult::Position(_)));
    }

    // ═══════════════════════════════════════════════════════════════════
    // Search match counting: timeout, caching, maxcount
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn count_search_matches_basic() {
        #[cfg(not(target_arch = "wasm32"))]
        let result = super::count_search_matches("foo bar foo baz foo", "foo", 8, None);
        #[cfg(target_arch = "wasm32")]
        let result = super::count_search_matches("foo bar foo baz foo", "foo", 8);

        let info = result.expect("should find matches");
        assert_eq!(info.current, 2, "landed_pos=8 is 2nd match");
        assert_eq!(info.total, 3, "3 total 'foo' matches");
        assert!(info.complete, "small document should be complete");
    }

    #[test]
    fn count_search_matches_first_position() {
        #[cfg(not(target_arch = "wasm32"))]
        let result = super::count_search_matches("foo bar foo", "foo", 0, None);
        #[cfg(target_arch = "wasm32")]
        let result = super::count_search_matches("foo bar foo", "foo", 0);

        let info = result.unwrap();
        assert_eq!(info.current, 1);
        assert_eq!(info.total, 2);
        assert!(info.complete);
    }

    #[test]
    fn count_search_matches_no_matches() {
        #[cfg(not(target_arch = "wasm32"))]
        let result = super::count_search_matches("hello world", "xyz", 0, None);
        #[cfg(target_arch = "wasm32")]
        let result = super::count_search_matches("hello world", "xyz", 0);

        assert!(result.is_none(), "no matches should return None");
    }

    #[test]
    fn count_search_matches_empty_pattern() {
        #[cfg(not(target_arch = "wasm32"))]
        let result = super::count_search_matches("hello", "", 0, None);
        #[cfg(target_arch = "wasm32")]
        let result = super::count_search_matches("hello", "", 0);

        assert!(result.is_none(), "empty pattern should return None");
    }

    #[test]
    fn count_search_matches_maxcount_exceeded() {
        // Create text with >99 matches (SEARCH_COUNT_MAXCOUNT = 99)
        let text = "a ".repeat(150);
        #[cfg(not(target_arch = "wasm32"))]
        let result = super::count_search_matches(&text, "a", 0, None);
        #[cfg(target_arch = "wasm32")]
        let result = super::count_search_matches(&text, "a", 0);

        let info = result.unwrap();
        assert!(info.total > 0);
        assert!(!info.complete, "exceeding maxcount should be incomplete");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn count_search_matches_with_deadline_completes_for_small_doc() {
        let deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(10));
        let result = super::count_search_matches("foo bar foo baz foo", "foo", 8, deadline);
        let info = result.unwrap();
        assert!(info.complete, "generous deadline should complete");
        assert_eq!(info.total, 3);
    }

    #[test]
    fn count_search_matches_with_word_boundary_pattern() {
        #[cfg(not(target_arch = "wasm32"))]
        let result = super::count_search_matches("foo foobar foo", "\\<foo\\>", 0, None);
        #[cfg(target_arch = "wasm32")]
        let result = super::count_search_matches("foo foobar foo", "\\<foo\\>", 0);

        let info = result.unwrap();
        assert_eq!(info.total, 2, "word boundary should skip 'foobar'");
        assert!(info.complete);
    }

    // ── Fold-aware search tests ──────────────────────────────────────

    mod fold_aware_search {
        use super::*;
        use crate::document::{FoldProvider, Providers};
        use crate::primitives::{Direction, LineNumber};

        /// Fold: lines 1-2 are folded.
        struct FoldLines1To2;
        impl FoldProvider for FoldLines1To2 {
            fn next_visible_line(&self, line: LineNumber, dir: Direction) -> LineNumber {
                if (1..=2).contains(&line.get()) {
                    match dir {
                        Direction::Forward => LineNumber::new(3),
                        Direction::Backward => LineNumber::new(0),
                    }
                } else {
                    line
                }
            }
            fn is_folded(&self, line: LineNumber) -> bool {
                (1..=2).contains(&line.get())
            }
        }

        #[test]
        fn search_from_inside_fold_snaps_forward() {
            // "aaa\nbbb\nccc\nddd\n"
            //  0    4    8    12
            // Lines 1-2 folded. Cursor at offset 5 (inside fold, line 1).
            // Searching forward for "ddd" should snap cursor to fold end first.
            let text = "aaa\nbbb\nccc\nddd\n";
            let opts = crate::primitives::VimOptions::default();
            let fold = FoldLines1To2;
            let providers = Providers::new().with_fold(&fold);
            let ctx = MotionContext::new(text, Offset::new(5), 1, &opts)
                .with_search("ddd", Direction::Forward)
                .with_providers(providers);
            let result = SearchMotion::NextMatch.compute(&ctx);
            assert_eq!(
                result,
                MotionResult::Position(Offset::new(12)),
                "Search should find 'ddd' at offset 12"
            );
        }
    }
}
