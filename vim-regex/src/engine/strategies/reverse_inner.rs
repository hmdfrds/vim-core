//! Reverse inner-literal search.
//!
//! Find an inner literal via memchr, run prefix-reverse NFA backward to
//! locate the match start, confirm with forward anchored match.

use crate::cache::Cache;
use crate::common::advance_one_codepoint;
use crate::common::SearchResult;
use crate::engine::strategy::{EngineKind, SearchMode, MAX_REVERSE_ATTEMPTS};
use crate::engine::VimRegex;
use crate::engines;
use crate::matchers::MatchContext;

/// Reverse inner-literal-guided search.
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    debug_assert!(
        regex.engine_kind == EngineKind::PikeVm,
        "ReverseInner strategy should only be in cascade for PikeVm patterns"
    );
    if regex.reverse_nfa.is_none() {
        return SearchResult::Declined;
    }
    let prefix_rev_nfa = match regex.prefix_reverse_nfa.as_ref() {
        Some(n) => n,
        None => return SearchResult::Declined,
    };
    // Dynamic check: composing mode changes per MatchContext.
    if ctx.ignore_composing {
        return SearchResult::Declined;
    }

    // Select inner literal based on case sensitivity.
    let inner_lit = if ctx.case_sensitive {
        regex.inner_literal.as_deref()
    } else {
        // No case-insensitive inner literal is extracted, so there is
        // nothing to guide the scan with.
        None
    };
    let inner_lit = match inner_lit {
        Some(s) => s,
        None => return SearchResult::Declined,
    };

    match reverse_inner_search(regex, cache, ctx, prefix_rev_nfa, start, inner_lit) {
        Some(m) => SearchResult::Match(m),
        None => SearchResult::Declined,
    }
}

/// Core reverse-inner search loop.
///
/// Uses `min_start` tracking to prevent quadratic behavior: after each
/// failed reverse scan candidate, `min_start` advances past the candidate
/// position so future reverse scans don't re-traverse already-eliminated text.
fn reverse_inner_search(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    prefix_reverse_nfa: &crate::nfa::Nfa,
    start: usize,
    inner_lit: &str,
) -> Option<crate::VimMatch> {
    let finder = memchr::memmem::Finder::new(inner_lit.as_bytes());
    let text_bytes = ctx.text.as_bytes();
    let mut search_from = start;
    let mut min_start = start;

    for _ in 0..MAX_REVERSE_ATTEMPTS {
        let lit_pos = finder.find(text_bytes.get(search_from..)?)?;
        let lit_pos = lit_pos + search_from;

        // Anti-quadratic: forward DFA stopat raises min_start.
        if crate::engine::strategy::is_dfa_eligible(&regex.properties) {
            if let Err(stop_pos) =
                super::hybrid_dfa::try_match_anchored_stopat(regex, cache, ctx, min_start)
            {
                min_start = min_start.max(stop_pos);
            }
        }

        let scan_lower = min_start.max(lit_pos.saturating_sub(regex.max_reverse_distance));
        let mut inner_lower = scan_lower;

        let mut found_any_candidate = false;
        while let Some(candidate) = engines::pike_vm::try_match_at_reverse(
            prefix_reverse_nfa,
            cache,
            ctx,
            lit_pos,
            inner_lower,
        ) {
            found_any_candidate = true;
            if let Some(m) = engines::pike_vm::match_anchored(&regex.nfa, cache, ctx, candidate) {
                return Some(m);
            }
            inner_lower = advance_one_codepoint(ctx.text, candidate);
        }

        if found_any_candidate {
            min_start = min_start.max(lit_pos);
        }

        search_from = lit_pos + 1;
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
    fn reverse_inner_declines_without_prefix_reverse_nfa() {
        let regex = VimRegex::new("hello").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("hello");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        assert!(matches!(result, SearchResult::Declined));
    }
}
