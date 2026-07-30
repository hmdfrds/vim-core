//! One-Pass DFA strategy tier.
//!
//! For anchored-start patterns with captures that are one-pass eligible,
//! resolves captures at DFA speed without Pike VM thread management.
//! Only activates for `SearchMode::Full` (its value is captures).

use crate::cache::Cache;
use crate::common::SearchResult;
use crate::engine::strategy::SearchMode;
use crate::engine::VimRegex;
use crate::engines::onepass;
use crate::matchers::MatchContext;

pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    mode: SearchMode,
) -> SearchResult {
    // The one-pass DFA classifier is built at compile time with exact char
    // values. Case-insensitive search would need equivalence-class folding
    // that the classifier doesn't support, so decline to a lower tier.
    if !ctx.case_sensitive {
        return SearchResult::Declined;
    }

    // One-pass DFA only provides value for Full mode (captures needed).
    // For HalfEnd/Existence, the regular DFA or SmallWrite suffices.
    if mode != SearchMode::Full {
        return SearchResult::Declined;
    }

    // Only useful for patterns that actually need capture resolution.
    if regex.properties.capture_count() == 0
        && !regex.properties.has_match_override()
        && !regex.properties.has_alternation()
    {
        return SearchResult::Declined;
    }

    // Must be one-pass eligible.
    if !onepass::is_onepass_eligible(&regex.properties) {
        return SearchResult::Declined;
    }

    // Only handles anchored-start patterns as a standalone strategy.
    // For non-anchored patterns, the HybridDfa strategy finds the position
    // first and then invokes one-pass as a capture-resolver internally.
    if !regex.properties.is_anchored_start_of_file() {
        return SearchResult::Declined;
    }

    // Build or retrieve the one-pass DFA (lazy, cached in Cache).
    let onepass_state = cache.onepass_state(&regex.nfa);
    match onepass_state.search_anchored(ctx.text, start) {
        Some(m) => SearchResult::Match(m),
        None => SearchResult::NoMatch,
    }
}
