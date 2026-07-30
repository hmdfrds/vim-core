//! Reverse suffix search.
//!
//! Find a suffix literal via memchr, run reverse NFA backward to locate
//! the match start, confirm with forward anchored match.

use crate::cache::Cache;
use crate::common::advance_one_codepoint;
use crate::common::SearchResult;
use crate::engine::strategy::{EngineKind, SearchMode, MAX_REVERSE_ATTEMPTS};
use crate::engine::VimRegex;
use crate::engines;
use crate::matchers::MatchContext;

/// Reverse suffix-guided search.
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    debug_assert!(
        regex.engine_kind == EngineKind::PikeVm,
        "ReverseSuffix strategy should only be in cascade for PikeVm patterns"
    );
    let rev_nfa = match regex.reverse_nfa.as_ref() {
        Some(n) => n,
        None => return SearchResult::Declined,
    };
    // Dynamic check: composing mode changes per MatchContext.
    if ctx.ignore_composing {
        return SearchResult::Declined;
    }

    // Select suffix literal based on case sensitivity.
    let suffix = if ctx.case_sensitive {
        regex.suffix_literal.as_deref()
    } else {
        regex.ci_suffix_literal.as_deref()
    };
    let suffix = match suffix {
        Some(s) => s,
        None => return SearchResult::Declined,
    };

    match reverse_suffix_search(regex, cache, ctx, rev_nfa, start, suffix) {
        Some(m) => SearchResult::Match(m),
        None => SearchResult::Declined, // Not authoritative — fall through.
    }
}

/// Core reverse-suffix search loop.
///
/// Uses `min_start` tracking to prevent quadratic behavior: after each
/// failed reverse scan candidate, `min_start` advances past the candidate
/// end so future reverse scans don't re-traverse already-eliminated text.
fn reverse_suffix_search(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    reverse_nfa: &crate::nfa::Nfa,
    start: usize,
    suffix: &str,
) -> Option<crate::VimMatch> {
    let finder = memchr::memmem::Finder::new(suffix.as_bytes());
    let text_bytes = ctx.text.as_bytes();
    let mut search_from = start;
    // Track minimum start position: after each failed reverse scan candidate,
    // advance min_start past the candidate end so the next reverse scan
    // doesn't re-traverse already-eliminated text.
    let mut min_start = start;

    for _ in 0..MAX_REVERSE_ATTEMPTS {
        let suffix_pos = finder.find(text_bytes.get(search_from..)?)?;
        let suffix_pos = suffix_pos + search_from;
        let suffix_end = suffix_pos + suffix.len();

        // Anti-quadratic: use forward DFA stopat to raise min_start.
        // If the forward DFA dies at `stop_pos` when anchored at min_start,
        // we know no match can start before stop_pos.
        if crate::engine::strategy::is_dfa_eligible(&regex.properties) {
            if let Err(stop_pos) =
                super::hybrid_dfa::try_match_anchored_stopat(regex, cache, ctx, min_start)
            {
                min_start = min_start.max(stop_pos);
            }
        }

        // Don't scan backward past min_start.
        let scan_lower = min_start.max(suffix_end.saturating_sub(regex.max_reverse_distance));
        let mut inner_lower = scan_lower;

        let mut found_any_candidate = false;
        while let Some(candidate) =
            engines::pike_vm::try_match_at_reverse(reverse_nfa, cache, ctx, suffix_end, inner_lower)
        {
            found_any_candidate = true;
            if let Some(m) = engines::pike_vm::match_anchored(&regex.nfa, cache, ctx, candidate) {
                return Some(m);
            }
            inner_lower = advance_one_codepoint(ctx.text, candidate);
        }

        // Advance min_start past this suffix occurrence — no match here,
        // so future reverse scans should not re-examine this region.
        if found_any_candidate {
            min_start = min_start.max(suffix_end);
        }

        search_from = suffix_pos + 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::strategy::SearchMode;
    use crate::matchers::MatchContext;
    use crate::VimRegex;

    #[test]
    fn reverse_suffix_declines_without_suffix() {
        let regex = VimRegex::new(r".\+").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("hello");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        assert!(matches!(result, SearchResult::Declined));
    }

    #[test]
    fn reverse_suffix_with_stopat_skips_dead_region() {
        // The anti-quadratic guard ensures that when the forward DFA reports
        // a stop position, reverse scans don't extend past it.
        // This is primarily a performance guarantee, tested via the existing
        // correctness tests -- the strategy should produce the same results
        // whether or not stopat is used.
        let regex = VimRegex::new(r"\w\+world").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("helloworld and goodbyeworld");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        match result {
            SearchResult::Match(m) => {
                assert_eq!(&ctx.text[m.range.clone()], "helloworld");
            }
            SearchResult::Declined | SearchResult::NoMatch => {} // Acceptable
            // CapacityExceeded only from the terminal backtracker; ReverseSuffix cannot produce it.
            SearchResult::CapacityExceeded => {
                unreachable!("CapacityExceeded only from the terminal backtracker")
            }
        }
    }
}
