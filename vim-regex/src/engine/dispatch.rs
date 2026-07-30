//! Search orchestration: the strategy cascade at search time.
//!
//! Implements the strategy cascade for forward searches, the find_all
//! loop, and the is_match shortcut.

use compact_str::CompactString;

use crate::cache::bloom::{extract_trigrams, LineBloomFilter};
use crate::cache::Cache;
use crate::common::advance_one_codepoint;
use crate::ir::{VimRegexError, VimRegexErrorKind};
use crate::matchers::MatchContext;
use crate::VimMatch;

use super::pattern_stats::StrategyKind;
use super::strategy::{SearchMode, SearchResult, Strategy};
use super::VimRegex;

// ═══════════════════════════════════════════════════════════════════════════════
// MATCH OFFSET HELPER
// ═══════════════════════════════════════════════════════════════════════════════

/// Offset all ranges in a `VimMatch` by `offset` bytes.
pub(crate) fn offset_match(m: VimMatch, offset: usize) -> VimMatch {
    if offset == 0 {
        return m;
    }
    let captures = m
        .captures
        .into_iter()
        .map(|c| c.map(|r| r.start + offset..r.end + offset))
        .collect();
    VimMatch::new(
        m.range.start + offset..m.range.end + offset,
        m.full_range.start + offset..m.full_range.end + offset,
        captures,
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// STRATEGY CASCADE — CORE DISPATCH
// ═══════════════════════════════════════════════════════════════════════════════

/// The core search dispatch that applies the strategy cascade.
///
/// Iterates through the pre-built strategy list. Each strategy may:
/// - Return Match(m) — search is done, return the match.
/// - Return NoMatch — search is done, no match exists.
/// - Return Declined — this strategy is not applicable, try next.
///
/// The last strategy (EngineDispatch) never declines.
///
/// `mode` is an optimization hint threaded through to strategies:
/// - `Full`: produce match range + captures.
/// - `HalfEnd`: produce match end only (for reverse-confirmed strategies).
/// - `Existence`: boolean match/no-match (for `is_match`).
pub(crate) fn search_internal(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
    mode: SearchMode,
) -> Result<Option<VimMatch>, VimRegexError> {
    cache.prefilter_tracker.reset();
    cache.prefilter_tracker.set_haystack_len(ctx.text.len());
    cache.lookaround_memo.clear();

    let resolved = regex.resolve_context(ctx);

    // Narrow the haystack first when the pattern pins buffer positions.
    let (resolved, offset) = regex.apply_range_narrowing(&resolved);
    let effective_start = start.saturating_sub(offset);

    // Min-length pruning: if the remaining text is shorter than the minimum
    // match length, no match is possible.
    if regex.min_match_length > 0 {
        let remaining = resolved.text.len().saturating_sub(effective_start);
        if remaining < regex.min_match_length {
            return Ok(None);
        }
    }

    // Fast-reject optimizations only apply to case-sensitive searches.
    // Case-insensitive searches may match different byte values than
    // the extracted literals, so we skip these checks to avoid false negatives.
    if resolved.case_sensitive {
        // Fast-reject: if a required byte must appear in every match and it's
        // absent from the remaining text, no match is possible.
        if let Some((req_byte, _ci)) = regex.required_byte {
            let haystack = resolved.text.as_bytes();
            let search_slice = if effective_start < haystack.len() {
                &haystack[effective_start..]
            } else {
                &[]
            };
            if memchr::memchr(req_byte, search_slice).is_none() {
                return Ok(None);
            }
        }

        // Fast-reject: if the prefilter tree says required literals are absent,
        // no match is possible.
        if let Some(ref tree) = regex.prefilter_tree {
            let search_text = if effective_start < resolved.text.len() {
                &resolved.text[effective_start..]
            } else {
                ""
            };
            if !tree.is_satisfied(search_text) {
                return Ok(None);
            }
        }

        // Fast-reject: for end-anchored patterns with an extracted suffix literal,
        // use backward memmem to verify the suffix exists in the text. If absent,
        // no match is possible.
        if regex.properties.is_anchored_end_of_file() {
            if let Some(ref suffix) = regex.suffix_literal_extracted {
                let haystack = resolved.text.as_bytes();
                let search_slice = if effective_start < haystack.len() {
                    &haystack[effective_start..]
                } else {
                    &[]
                };
                if memchr::memmem::rfind(search_slice, suffix.as_bytes()).is_none() {
                    return Ok(None);
                }
            }
        }
    }

    // Bloom-filter fast reject: for texts > 1KB containing newlines, extract
    // trigrams from the pattern's bloom literal and test each line. If EVERY
    // line fails the bloom test, no match is possible anywhere in the text.
    if resolved.case_sensitive {
        if let Some(bloom_lit) = regex.bloom_literal() {
            let search_text = if effective_start < resolved.text.len() {
                &resolved.text[effective_start..]
            } else {
                ""
            };
            if search_text.len() > 1024 && memchr::memchr(b'\n', search_text.as_bytes()).is_some() {
                let trigrams = extract_trigrams(bloom_lit);
                if !trigrams.is_empty() {
                    let any_line_passes = search_text
                        .split('\n')
                        .any(|line| LineBloomFilter::from_line(line).might_contain_all(&trigrams));
                    if !any_line_passes {
                        return Ok(None);
                    }
                }
            }
        }
    }

    // Execute strategy cascade.
    let text_len = resolved.text.len().saturating_sub(effective_start);
    for &strategy in &regex.strategies {
        match strategy.try_search(regex, cache, effective_start, &resolved, mode) {
            SearchResult::Match(m) => {
                cache.prefilter_tracker.record_confirmation();
                cache.record_search(text_len, 0, strategy_to_stats_kind(strategy));
                return Ok(Some(offset_match(m, offset)));
            }
            SearchResult::NoMatch => {
                cache.record_search(text_len, 0, strategy_to_stats_kind(strategy));
                return Ok(None);
            }
            SearchResult::CapacityExceeded => {
                return Err(VimRegexError::new(VimRegexErrorKind::HaystackTooLarge {
                    needed: cache.visited_mut().required_bytes(resolved.text.len()),
                    limit: crate::cache::MAX_VISITED_BYTES,
                }));
            }
            SearchResult::Declined => continue,
        }
    }

    // EngineDispatch never declines — if we reach here, the strategy list is misconfigured.
    Err(VimRegexError::new(VimRegexErrorKind::InternalError {
        detail: CompactString::from("strategy cascade exhausted: EngineDispatch must be the last strategy and never declines"),
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// STRATEGY → STATS KIND MAPPING
// ═══════════════════════════════════════════════════════════════════════════════

/// Map a `Strategy` enum to the simplified `StrategyKind` used by `PatternStats`.
fn strategy_to_stats_kind(strategy: Strategy) -> StrategyKind {
    match strategy {
        Strategy::LiteralBypass => StrategyKind::Literal,
        Strategy::AcFullMatch => StrategyKind::AcFullMatch,
        Strategy::HybridDfa => StrategyKind::HybridDfa,
        Strategy::OnePassDfa => StrategyKind::OnePassDfa,
        // All remaining strategies ultimately dispatch through the engine
        // (Pike VM or Backtracker), so we record them as PikeVm for stats.
        Strategy::SmallWrite
        | Strategy::AnchoredStart
        | Strategy::ReverseAnchored
        | Strategy::ReverseSuffix
        | Strategy::ReverseInner
        | Strategy::EngineDispatch => StrategyKind::PikeVm,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// FIND ALL
// ═══════════════════════════════════════════════════════════════════════════════

/// Find all non-overlapping matches in the text, reusing `cache`.
///
/// Delegates to `search_internal` in a loop, which runs the full strategy
/// cascade (AC, DFA, reverse, engine dispatch) for each match position.
/// This eliminates the duplicated AC/DFA/backtracker dispatch paths that
/// previously existed in this function.
///
/// Zero-width match handling: when a match has zero length (e.g., `a*` at
/// a non-'a' position), we advance by one codepoint to avoid infinite loops.
/// Since `search_internal` starts searching at `pos`, it can never find a
/// match starting before `pos`, so advancing past the current position is
/// sufficient to guarantee progress.
pub(crate) fn find_all_with_cache(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
) -> Result<Vec<VimMatch>, VimRegexError> {
    let mut results = Vec::new();
    let mut pos = 0usize;

    loop {
        match search_internal(regex, cache, ctx, pos, SearchMode::Full)? {
            Some(m) => {
                let end = m.range.end;
                let is_empty = m.range.is_empty();
                results.push(m);

                // Advance past the match end. For zero-width matches,
                // advance by one codepoint past the match position to
                // guarantee progress and prevent duplicate zero-width
                // matches at the same position.
                pos = if is_empty {
                    advance_one_codepoint(ctx.text, end)
                } else {
                    end
                };
            }
            None => break,
        }

        if pos > ctx.text.len() {
            break;
        }
    }

    Ok(results)
}

// ═══════════════════════════════════════════════════════════════════════════════
// IS MATCH
// ═══════════════════════════════════════════════════════════════════════════════

/// Check whether the pattern matches anywhere in the text, reusing `cache`.
///
/// Delegates to `search_internal` with `SearchMode::Existence`. The strategy
/// cascade (including HybridDfa) handles the DFA fast path internally.
pub(crate) fn is_match_with_cache(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
) -> Result<bool, VimRegexError> {
    Ok(search_internal(regex, cache, ctx, 0, SearchMode::Existence)?.is_some())
}
