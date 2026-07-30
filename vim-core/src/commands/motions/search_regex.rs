//! Regex-based search using the VimRegex engine.
//!
//! Provides `search_with_regex` as the primary search strategy when
//! no `SearchProvider` is available. Handles forward/backward search
//! with count and wrapping, using the compiled `VimRegex`.

use std::collections::VecDeque;

use super::types::MotionResult;
use crate::commands::helpers::next_char_boundary;
use crate::primitives::{Direction, Offset, SearchFlags};
use crate::regex::{Cache, MatchContext, VimRegex};

/// How often (in loop iterations) to check the deadline for timeout.
///
/// Amortises the `Instant::now()` syscall to keep the hot loop fast.
const DEADLINE_CHECK_INTERVAL: u64 = 64;

/// Search using the VimRegex engine.
///
/// Builds a `MatchContext` from the text and flags, then delegates to
/// forward or backward search depending on direction.
///
/// ## Timeout
///
/// On native targets, an optional `deadline` prevents catastrophic
/// backtracking from blocking the main thread. The check is amortised
/// to every [`DEADLINE_CHECK_INTERVAL`] iterations. On timeout the
/// function returns `MotionResult::NoMotion` (search timed out).
pub(super) fn search_with_regex(
    re: &VimRegex,
    text: &str,
    cursor: usize,
    direction: Direction,
    count: u32,
    flags: &SearchFlags,
    #[cfg(not(target_arch = "wasm32"))] deadline: Option<std::time::Instant>,
) -> MotionResult {
    let ctx = MatchContext::builder(text)
        .cursor(cursor)
        .case_sensitive(flags.case_sensitive())
        .build();

    let count = count as usize;
    let start = next_char_boundary(text, cursor);

    // One Cache for the entire search operation: it is reused by every
    // match call below so the DFA state allocation happens once, not per call.
    let mut cache = re.create_cache();

    if direction.is_forward() {
        regex_search_forward(
            re,
            &mut cache,
            &ctx,
            start,
            count,
            flags.wrap(),
            #[cfg(not(target_arch = "wasm32"))]
            deadline,
        )
    } else {
        regex_search_backward(
            re,
            &mut cache,
            &ctx,
            cursor,
            count,
            flags.wrap(),
            #[cfg(not(target_arch = "wasm32"))]
            deadline,
        )
    }
}

/// Forward regex search with count and optional wrapping.
fn regex_search_forward(
    re: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
    count: usize,
    wrap: bool,
    #[cfg(not(target_arch = "wasm32"))] deadline: Option<std::time::Instant>,
) -> MotionResult {
    let mut from = start;
    let mut found = 0;
    let mut last_pos = None;
    #[cfg(not(target_arch = "wasm32"))]
    let mut iter_count: u64 = 0;

    // Scan forward from cursor to end
    while let Ok(Some(m)) = re.find_at_with_cache(cache, ctx, from) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            iter_count += 1;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if iter_count.is_multiple_of(DEADLINE_CHECK_INTERVAL) {
            if let Some(dl) = deadline {
                if std::time::Instant::now() >= dl {
                    return MotionResult::NoMotion;
                }
            }
        }

        found += 1;
        last_pos = Some(m.range.start);
        if found == count {
            return MotionResult::Position(Offset::new(m.range.start));
        }
        from = if m.range.end == from {
            advance_one_cp(ctx.text(), from)
        } else {
            m.range.end
        };
    }

    // Wrap: search from beginning up to cursor
    if wrap && found < count {
        from = 0;
        while let Ok(Some(m)) = re.find_at_with_cache(cache, ctx, from) {
            if m.range.start >= start {
                break;
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                iter_count += 1;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if iter_count.is_multiple_of(DEADLINE_CHECK_INTERVAL) {
                if let Some(dl) = deadline {
                    if std::time::Instant::now() >= dl {
                        return MotionResult::NoMotion;
                    }
                }
            }

            found += 1;
            last_pos = Some(m.range.start);
            if found == count {
                return MotionResult::Position(Offset::new(m.range.start));
            }
            from = if m.range.end == from {
                advance_one_cp(ctx.text(), from)
            } else {
                m.range.end
            };
        }
    }

    last_pos.map_or(MotionResult::Error, |p| {
        MotionResult::Position(Offset::new(p))
    })
}

/// Backward regex search with count and optional wrapping.
///
/// Uses a bounded ring buffer of size `count` instead of collecting all
/// matches into a `Vec`. For the common case of `count=1`, the ring is
/// a single-element buffer that tracks only the last match position.
fn regex_search_backward(
    re: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    cursor: usize,
    count: usize,
    wrap: bool,
    #[cfg(not(target_arch = "wasm32"))] deadline: Option<std::time::Instant>,
) -> MotionResult {
    // Phase 1: scan [0, cursor) keeping the last `count` matches in a ring.
    let mut ring = VecDeque::with_capacity(count);
    let timed_out = scan_matches_into_ring(
        re,
        cache,
        ctx,
        0,
        Some(cursor),
        count,
        &mut ring,
        #[cfg(not(target_arch = "wasm32"))]
        deadline,
    );
    #[cfg(not(target_arch = "wasm32"))]
    if timed_out {
        return MotionResult::NoMotion;
    }

    if ring.len() >= count {
        // We have enough matches before cursor. The front of the ring
        // is the count-th from the end of the forward scan.
        if let Some(&pos) = ring.front() {
            return MotionResult::Position(Offset::new(pos));
        }
    }

    // Not enough matches before cursor.
    let found_before = ring.len();

    if wrap {
        // Phase 2 (wrap): scan [cursor, text.len()) keeping last `count` matches.
        let mut after_ring = VecDeque::with_capacity(count);
        let timed_out = scan_matches_into_ring(
            re,
            cache,
            ctx,
            cursor,
            None,
            count,
            &mut after_ring,
            #[cfg(not(target_arch = "wasm32"))]
            deadline,
        );
        #[cfg(not(target_arch = "wasm32"))]
        if timed_out {
            return MotionResult::NoMotion;
        }

        // We need (count - found_before) from the END of the after-cursor region.
        let remaining = count - found_before;
        if after_ring.len() >= remaining {
            // The count-th from end of after-region is at index
            // (after_ring.len() - remaining).
            if let Some(&pos) = after_ring.get(after_ring.len() - remaining) {
                return MotionResult::Position(Offset::new(pos));
            }
        }

        // Not enough total matches — return the last match we found anywhere.
        if let Some(&last) = after_ring.back().or_else(|| ring.back()) {
            return MotionResult::Position(Offset::new(last));
        }
    } else if let Some(&last) = ring.back() {
        // No wrap, return the closest match before cursor.
        return MotionResult::Position(Offset::new(last));
    }

    MotionResult::Error
}

/// Scan regex matches in `[from, end)` and keep the last `capacity` positions
/// in a bounded ring buffer. When `end` is `None`, scan to end of text.
///
/// Returns `true` if the scan was aborted due to a deadline timeout.
///
/// This replaces the old `collect_matches_before`/`collect_matches_from` which
/// allocated a `Vec<usize>` of ALL match positions. The ring buffer uses O(count)
/// memory instead of O(matches), which is a significant improvement for documents
/// with many matches.
#[allow(clippy::too_many_arguments)]
fn scan_matches_into_ring(
    re: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    from: usize,
    end: Option<usize>,
    capacity: usize,
    ring: &mut VecDeque<usize>,
    #[cfg(not(target_arch = "wasm32"))] deadline: Option<std::time::Instant>,
) -> bool {
    let mut pos = from;
    #[cfg(not(target_arch = "wasm32"))]
    let mut iter_count: u64 = 0;
    while let Ok(Some(m)) = re.find_at_with_cache(cache, ctx, pos) {
        if let Some(end) = end {
            if m.range.start >= end {
                break;
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            iter_count += 1;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if iter_count.is_multiple_of(DEADLINE_CHECK_INTERVAL) {
            if let Some(dl) = deadline {
                if std::time::Instant::now() >= dl {
                    return true;
                }
            }
        }
        ring.push_back(m.range.start);
        if ring.len() > capacity {
            ring.pop_front();
        }
        pos = if m.range.end == pos {
            advance_one_cp(ctx.text(), pos)
        } else {
            m.range.end
        };
    }
    false
}

/// Advance past one codepoint (prevent infinite loops on zero-width matches).
fn advance_one_cp(text: &str, pos: usize) -> usize {
    text.get(pos..)
        .and_then(|s| s.chars().next())
        .map_or(pos + 1, |ch| pos + ch.len_utf8())
}

// ═══════════════════════════════════════════════════════════════════════════════
// BLOOM-ACCELERATED SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

struct BloomLineCtx {
    case_sensitive: bool,
    line_start: usize,
}

/// Bloom-accelerated regex search using VimText's per-leaf bloom filters.
///
/// Builds a bloom query from the regex's extractable literal and uses
/// `find_matching_lines()` to skip leaves that definitely don't contain
/// the pattern. Falls back to `None` when bloom can't help.
///
/// On native targets, an optional `deadline` prevents catastrophic
/// backtracking from blocking the main thread (same semantics as
/// [`search_with_regex`]).
pub(super) fn bloom_search_with_regex(
    re: &VimRegex,
    tree: &vim_text::VimText,
    cursor: usize,
    direction: Direction,
    count: u32,
    flags: &SearchFlags,
    #[cfg(not(target_arch = "wasm32"))] deadline: Option<std::time::Instant>,
) -> Option<MotionResult> {
    if re.features().has_buffer_position {
        return None;
    }
    let literal = re.bloom_literal()?;
    let candidates = tree.find_matching_lines(literal, 0..tree.line_count());
    if candidates.is_empty() {
        return Some(MotionResult::Error);
    }

    let count = count as usize;
    let case_sensitive = flags.case_sensitive();

    // One Cache for the entire bloom search operation: reused across every
    // candidate line so the DFA state allocation happens once, not per line.
    let mut cache = re.create_cache();

    let scan = BloomScan {
        re,
        tree,
        candidates: &candidates,
        cursor,
        count,
        wrap: flags.wrap(),
        case_sensitive,
    };

    let result = if direction.is_forward() {
        bloom_forward(
            &scan,
            &mut cache,
            #[cfg(not(target_arch = "wasm32"))]
            deadline,
        )
    } else {
        bloom_backward(
            &scan,
            &mut cache,
            #[cfg(not(target_arch = "wasm32"))]
            deadline,
        )
    };
    Some(result)
}

/// The inputs a bloom-accelerated search shares between its forward and
/// backward passes: the compiled pattern, the buffer, the candidate lines the
/// bloom filter selected, and the search parameters. `bloom_forward` and
/// `bloom_backward` take exactly this record plus the scratch `Cache`.
struct BloomScan<'a> {
    /// Compiled pattern being searched for.
    re: &'a VimRegex,
    /// Buffer being searched.
    tree: &'a vim_text::VimText,
    /// Line numbers whose bloom filter matched the pattern's literal.
    candidates: &'a [usize],
    /// Byte offset the search starts from.
    cursor: usize,
    /// How many matches to advance past.
    count: usize,
    /// Whether the search wraps around the end of the buffer.
    wrap: bool,
    /// Whether the match is case sensitive.
    case_sensitive: bool,
}

fn bloom_forward(
    scan: &BloomScan<'_>,
    cache: &mut Cache,
    #[cfg(not(target_arch = "wasm32"))] deadline: Option<std::time::Instant>,
) -> MotionResult {
    let &BloomScan {
        re,
        tree,
        candidates,
        cursor,
        count,
        wrap,
        case_sensitive,
    } = scan;
    let cursor_line = tree.line_of_offset(cursor);
    let cursor_col = cursor.saturating_sub(tree.line_start(cursor_line).unwrap_or(0));
    let mut remaining = count;
    let mut last_pos = None;
    #[cfg(not(target_arch = "wasm32"))]
    let mut iter_count: u64 = 0;

    let fwd_start = candidates.partition_point(|&n| n < cursor_line);
    for &line_n in &candidates[fwd_start..] {
        #[cfg(not(target_arch = "wasm32"))]
        {
            iter_count += 1;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if iter_count.is_multiple_of(DEADLINE_CHECK_INTERVAL) {
            if let Some(dl) = deadline {
                if std::time::Instant::now() >= dl {
                    return MotionResult::NoMotion;
                }
            }
        }
        let (line_start, line_text) = match bloom_resolve_line(tree, line_n) {
            Some(pair) => pair,
            None => continue,
        };
        let from = if line_n == cursor_line {
            advance_one_cp(&line_text, cursor_col)
        } else {
            0
        };
        let lctx = BloomLineCtx {
            case_sensitive,
            line_start,
        };
        if let Some(pos) = bloom_scan_line(
            re,
            cache,
            &line_text,
            from,
            None,
            &lctx,
            &mut remaining,
            &mut last_pos,
        ) {
            return MotionResult::Position(Offset::new(pos));
        }
    }

    if wrap {
        let wrap_end = candidates.partition_point(|&n| n <= cursor_line);
        for &line_n in &candidates[..wrap_end] {
            #[cfg(not(target_arch = "wasm32"))]
            {
                iter_count += 1;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if iter_count.is_multiple_of(DEADLINE_CHECK_INTERVAL) {
                if let Some(dl) = deadline {
                    if std::time::Instant::now() >= dl {
                        return MotionResult::NoMotion;
                    }
                }
            }
            let (line_start, line_text) = match bloom_resolve_line(tree, line_n) {
                Some(pair) => pair,
                None => continue,
            };
            let end = if line_n == cursor_line {
                Some(advance_one_cp(&line_text, cursor_col))
            } else {
                None
            };
            let lctx = BloomLineCtx {
                case_sensitive,
                line_start,
            };
            if let Some(pos) = bloom_scan_line(
                re,
                cache,
                &line_text,
                0,
                end,
                &lctx,
                &mut remaining,
                &mut last_pos,
            ) {
                return MotionResult::Position(Offset::new(pos));
            }
        }
    }

    last_pos.map_or(MotionResult::Error, |p| {
        MotionResult::Position(Offset::new(p))
    })
}

fn bloom_backward(
    scan: &BloomScan<'_>,
    cache: &mut Cache,
    #[cfg(not(target_arch = "wasm32"))] deadline: Option<std::time::Instant>,
) -> MotionResult {
    let &BloomScan {
        re,
        tree,
        candidates,
        cursor,
        count,
        wrap,
        case_sensitive,
    } = scan;
    let cursor_line = tree.line_of_offset(cursor);
    let cursor_col = cursor.saturating_sub(tree.line_start(cursor_line).unwrap_or(0));
    let mut ring = VecDeque::with_capacity(count);
    #[cfg(not(target_arch = "wasm32"))]
    let mut iter_count: u64 = 0;

    let bwd_end = candidates.partition_point(|&n| n <= cursor_line);
    for &line_n in &candidates[..bwd_end] {
        #[cfg(not(target_arch = "wasm32"))]
        {
            iter_count += 1;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if iter_count.is_multiple_of(DEADLINE_CHECK_INTERVAL) {
            if let Some(dl) = deadline {
                if std::time::Instant::now() >= dl {
                    return MotionResult::NoMotion;
                }
            }
        }
        let (line_start, line_text) = match bloom_resolve_line(tree, line_n) {
            Some(pair) => pair,
            None => continue,
        };
        let end = if line_n == cursor_line {
            Some(cursor_col)
        } else {
            None
        };
        let lctx = BloomLineCtx {
            case_sensitive,
            line_start,
        };
        bloom_scan_ring(re, cache, &line_text, 0, end, &lctx, count, &mut ring);
    }

    if ring.len() >= count {
        if let Some(&pos) = ring.front() {
            return MotionResult::Position(Offset::new(pos));
        }
    }
    let found_before = ring.len();

    if wrap {
        let mut after_ring = VecDeque::with_capacity(count);
        let wrap_start = candidates.partition_point(|&n| n < cursor_line);
        for &line_n in &candidates[wrap_start..] {
            #[cfg(not(target_arch = "wasm32"))]
            {
                iter_count += 1;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if iter_count.is_multiple_of(DEADLINE_CHECK_INTERVAL) {
                if let Some(dl) = deadline {
                    if std::time::Instant::now() >= dl {
                        return MotionResult::NoMotion;
                    }
                }
            }
            let (line_start, line_text) = match bloom_resolve_line(tree, line_n) {
                Some(pair) => pair,
                None => continue,
            };
            let from = if line_n == cursor_line { cursor_col } else { 0 };
            let lctx = BloomLineCtx {
                case_sensitive,
                line_start,
            };
            bloom_scan_ring(
                re,
                cache,
                &line_text,
                from,
                None,
                &lctx,
                count,
                &mut after_ring,
            );
        }

        let remaining = count - found_before;
        if after_ring.len() >= remaining {
            if let Some(&pos) = after_ring.get(after_ring.len() - remaining) {
                return MotionResult::Position(Offset::new(pos));
            }
        }
        if let Some(&last) = after_ring.back().or_else(|| ring.back()) {
            return MotionResult::Position(Offset::new(last));
        }
    } else if let Some(&last) = ring.back() {
        return MotionResult::Position(Offset::new(last));
    }

    MotionResult::Error
}

fn bloom_resolve_line(
    tree: &vim_text::VimText,
    line_n: usize,
) -> Option<(usize, std::borrow::Cow<'_, str>)> {
    let start = tree.line_start(line_n)?;
    let text = tree.line(line_n)?;
    Some((start, text))
}

/// Scan a single line for regex matches, counting down `remaining`.
/// Returns the doc-absolute position when the target count is reached.
fn bloom_scan_line(
    re: &VimRegex,
    cache: &mut Cache,
    line_text: &str,
    from: usize,
    end: Option<usize>,
    lctx: &BloomLineCtx,
    remaining: &mut usize,
    last_pos: &mut Option<usize>,
) -> Option<usize> {
    // No `.cursor(...)`: there is no cursor inside an arbitrary scanned line,
    // so the setter is deliberately left uncalled rather than passing a
    // meaningless zero.
    let ctx = MatchContext::builder(line_text)
        .case_sensitive(lctx.case_sensitive)
        .build();
    let mut pos = from;
    while let Ok(Some(m)) = re.find_at_with_cache(cache, &ctx, pos) {
        if let Some(end) = end {
            if m.range.start >= end {
                break;
            }
        }
        let doc_pos = lctx.line_start + m.range.start;
        *last_pos = Some(doc_pos);
        *remaining = remaining.saturating_sub(1);
        if *remaining == 0 {
            return Some(doc_pos);
        }
        pos = if m.range.end == pos {
            advance_one_cp(line_text, pos)
        } else {
            m.range.end
        };
    }
    None
}

/// Scan a single line for regex matches, collecting into a bounded ring.
fn bloom_scan_ring(
    re: &VimRegex,
    cache: &mut Cache,
    line_text: &str,
    from: usize,
    end: Option<usize>,
    lctx: &BloomLineCtx,
    capacity: usize,
    ring: &mut VecDeque<usize>,
) {
    // No `.cursor(...)`: there is no cursor inside an arbitrary scanned line,
    // so the setter is deliberately left uncalled rather than passing a
    // meaningless zero.
    let ctx = MatchContext::builder(line_text)
        .case_sensitive(lctx.case_sensitive)
        .build();
    let mut pos = from;
    while let Ok(Some(m)) = re.find_at_with_cache(cache, &ctx, pos) {
        if let Some(end) = end {
            if m.range.start >= end {
                break;
            }
        }
        ring.push_back(lctx.line_start + m.range.start);
        if ring.len() > capacity {
            ring.pop_front();
        }
        pos = if m.range.end == pos {
            advance_one_cp(line_text, pos)
        } else {
            m.range.end
        };
    }
}
