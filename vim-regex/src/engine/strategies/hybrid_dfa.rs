//! Hybrid DFA fast path.
//!
//! Uses the lazy DFA for DFA-eligible patterns. If the DFA finds a match
//! and captures/overrides are needed, confirms with Pike VM anchored match.
//! If DFA Quits (thrashing), declines to the next strategy.

use smallvec::SmallVec;

use crate::accel::Prefilter;
use crate::cache::Cache;
use crate::common::SearchResult;
use crate::engine::strategy::SearchMode;
use crate::engine::VimRegex;
use crate::engines;
use crate::engines::lazy_dfa;
use crate::matchers::MatchContext;
use crate::VimMatch;

/// Hybrid DFA acceleration.
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    debug_assert!(
        crate::engine::strategy::is_dfa_eligible(&regex.properties),
        "HybridDfa strategy should not be in cascade for non-DFA-eligible patterns"
    );
    // Dynamic check: composing mode changes per MatchContext.
    if ctx.ignore_composing {
        return SearchResult::Declined;
    }

    // Ensure DFA cache case sensitivity matches.
    if cache
        .dfa
        .as_ref()
        .is_some_and(|d| d.case_sensitive() != ctx.case_sensitive)
    {
        cache.dfa = None;
    }

    // Skip prefilter when the tracker has disabled it due to low hit rate.
    let prefilter: Option<&dyn Prefilter> = if cache.prefilter_tracker.is_disabled() {
        None
    } else if ctx.case_sensitive {
        regex.prefilter.as_deref()
    } else {
        regex.ci_prefilter.as_deref()
    };

    let dfa_result = {
        let case_sensitive = ctx.case_sensitive;
        let dfa_cache = cache.dfa.get_or_insert_with(|| {
            lazy_dfa::DfaCache::new(
                &regex.nfa,
                case_sensitive,
                regex.properties.has_look_ahead_assertions(),
            )
        });
        lazy_dfa::dfa_search(&regex.nfa, dfa_cache, ctx, start, prefilter)
    };

    // Transfer newline checkpoints from the DFA cache to the
    // search_checkpoints store for edit-incremental search.
    if let Some(ref dfa_cache) = cache.dfa {
        if !dfa_cache.newline_checkpoints.is_empty() {
            for &(offset, ordinal) in &dfa_cache.newline_checkpoints {
                cache.search_checkpoints.record(offset, ordinal);
            }
        }
    }

    match dfa_result {
        lazy_dfa::DfaSearchResult::Match {
            start: fwd_start,
            end,
        } => {
            cache.prefilter_tracker.record_candidate();
            // Determine whether captures/overrides/alternation need Pike VM.
            let needs_captures = regex.properties.capture_count() > 0
                || regex.properties.has_match_override()
                || regex.properties.has_alternation();

            if needs_captures {
                // Captures needed -- use forward DFA's bumpalong start (the
                // correct leftmost match start). The reverse DFA cannot
                // replace Pike VM for capture resolution; it only provides
                // group 0 bounds.
                //
                // Try one-pass DFA first (DFA speed with captures).
                // Skip one-pass when case-insensitive: the classifier is built
                // with exact char values and has no equivalence-class folding.
                if ctx.case_sensitive
                    && crate::engines::onepass::is_onepass_eligible(&regex.properties)
                {
                    let onepass = cache.onepass_state(&regex.nfa);
                    if let Some(m) = onepass.search_anchored(ctx.text, fwd_start) {
                        return SearchResult::Match(m);
                    }
                    // One-pass couldn't match (not built or pattern not one-pass).
                    // Fall through to Pike VM.
                }
                // Fall back to Pike VM for capture resolution.
                match engines::pike_vm::match_anchored(&regex.nfa, cache, ctx, fwd_start) {
                    Some(m) => SearchResult::Match(m),
                    None => SearchResult::Declined, // Shouldn't happen, but safe.
                }
            } else {
                // No captures needed -- we can use forward DFA end + reverse
                // DFA start to compute group 0 bounds entirely at DFA speed.
                let match_start = if let Some(rev_nfa) =
                    regex.reverse_nfa.as_ref().filter(|_| !ctx.ignore_composing)
                {
                    let case_sensitive = ctx.case_sensitive;
                    let rev_cache = cache.reverse_dfa.get_or_insert_with(|| {
                        lazy_dfa::DfaCache::new(rev_nfa, case_sensitive, false)
                    });
                    // Ensure case sensitivity matches.
                    if rev_cache.case_sensitive() != case_sensitive {
                        *rev_cache = lazy_dfa::DfaCache::new(rev_nfa, case_sensitive, false);
                    }
                    match lazy_dfa::dfa_search_reverse(rev_nfa, rev_cache, ctx, end) {
                        lazy_dfa::DfaSearchResult::Match {
                            start: rev_start, ..
                        } => rev_start,
                        _ => fwd_start, // Reverse DFA quit or no match.
                    }
                } else {
                    fwd_start
                };
                SearchResult::Match(VimMatch::new(
                    match_start..end,
                    match_start..end,
                    SmallVec::new(),
                ))
            }
        }
        lazy_dfa::DfaSearchResult::NoMatch => SearchResult::NoMatch,
        lazy_dfa::DfaSearchResult::Quit => SearchResult::Declined,
    }
}

/// Anchored forward DFA scan with stop-position reporting on non-match.
///
/// Returns:
/// - `Ok(Some(match))` -- a match was found starting at `start`.
/// - `Ok(None)` -- DFA could not run (not eligible, quit, etc.).
/// - `Err(stop_pos)` -- no match; `stop_pos` is the position where the
///   DFA reached a dead state. Reverse strategies can use this as a lower
///   bound to avoid re-scanning text that the forward DFA already rejected.
pub(crate) fn try_match_anchored_stopat(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
) -> Result<Option<VimMatch>, usize> {
    if !crate::engine::strategy::is_dfa_eligible(&regex.properties) {
        return Ok(None);
    }
    if ctx.ignore_composing {
        return Ok(None);
    }

    // Ensure DFA cache case sensitivity matches.
    if cache
        .dfa
        .as_ref()
        .is_some_and(|d| d.case_sensitive() != ctx.case_sensitive)
    {
        cache.dfa = None;
    }

    let dfa_result = {
        let case_sensitive = ctx.case_sensitive;
        let dfa_cache = cache.dfa.get_or_insert_with(|| {
            lazy_dfa::DfaCache::new(
                &regex.nfa,
                case_sensitive,
                regex.properties.has_look_ahead_assertions(),
            )
        });

        // Use the anchored search directly (not the bumpalong loop).
        lazy_dfa::dfa_search_anchored_stopat(&regex.nfa, dfa_cache, ctx, start)
    };

    match dfa_result {
        lazy_dfa::StopAtResult::Match { start: s, end: e } => {
            if regex.properties.capture_count() > 0
                || regex.properties.has_match_override()
                || regex.properties.has_alternation()
            {
                match engines::pike_vm::match_anchored(&regex.nfa, cache, ctx, s) {
                    Some(m) => Ok(Some(m)),
                    None => Ok(None),
                }
            } else {
                Ok(Some(VimMatch::new(s..e, s..e, SmallVec::new())))
            }
        }
        lazy_dfa::StopAtResult::Dead { stop_pos } => Err(stop_pos),
        lazy_dfa::StopAtResult::Quit => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::strategy::SearchMode;
    use crate::matchers::MatchContext;
    use crate::VimRegex;

    #[test]
    fn hybrid_dfa_finds_simple_pattern() {
        let regex = VimRegex::new(r"[0-9]\+").unwrap();
        if !crate::engine::strategy::is_dfa_eligible(&regex.properties) {
            return; // Skip if DFA not eligible (shouldn't happen for this pattern).
        }
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("abc 123 def");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        match result {
            SearchResult::Match(m) => assert_eq!(&ctx.text[m.range.clone()], "123"),
            SearchResult::Declined => {} // DFA may quit; acceptable
            SearchResult::NoMatch => panic!("expected match or decline"),
            // CapacityExceeded only from the terminal backtracker; HybridDfa cannot produce it.
            SearchResult::CapacityExceeded => {
                unreachable!("CapacityExceeded only from the terminal backtracker")
            }
        }
    }

    #[test]
    fn hybrid_dfa_not_in_cascade_for_backrefs() {
        // Backref patterns are not DFA-eligible, so HybridDfa should never
        // be in their cascade (verified at compile time in build_strategies).
        let regex = VimRegex::new(r"\(a\)\1").unwrap();
        assert!(
            !regex
                .strategies
                .as_slice()
                .contains(&crate::engine::strategy::Strategy::HybridDfa),
            "HybridDfa should not be in the cascade for backref patterns"
        );
    }

    #[test]
    fn hybrid_dfa_finds_match_with_captures() {
        // Pattern with captures: forward DFA finds end position, then
        // Pike VM resolves captures anchored at the forward DFA start.
        // Test through the full engine to exercise the strategy cascade.
        let regex = VimRegex::new(r"\(\d\+\)").unwrap();
        let ctx = MatchContext::simple("abc 123 def");
        let m = regex.find(&ctx).expect("no error").expect("expected match");
        assert_eq!(&ctx.text[m.range.clone()], "123");
    }

    #[test]
    fn hybrid_dfa_reverse_finds_start_for_no_capture_pattern() {
        // No captures: forward DFA end + reverse DFA start give group 0
        // bounds at full DFA speed, bypassing Pike VM entirely.
        let regex = VimRegex::new(r"[0-9]\+").unwrap();
        if !crate::engine::strategy::is_dfa_eligible(&regex.properties) {
            return;
        }
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("abc 123 def");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        match result {
            SearchResult::Match(m) => {
                assert_eq!(&ctx.text[m.range.clone()], "123");
            }
            SearchResult::Declined => {} // DFA may quit
            SearchResult::NoMatch => panic!("expected match or decline"),
            // CapacityExceeded only from the terminal backtracker; HybridDfa cannot produce it.
            SearchResult::CapacityExceeded => {
                unreachable!("CapacityExceeded only from the terminal backtracker")
            }
        }
    }

    #[test]
    fn stopat_returns_match_for_matching_input() {
        let regex = VimRegex::new(r"[0-9]\+").unwrap();
        if !crate::engine::strategy::is_dfa_eligible(&regex.properties) {
            return;
        }
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("123abc");
        let result = try_match_anchored_stopat(&regex, &mut cache, &ctx, 0);
        match result {
            Ok(Some(m)) => {
                assert_eq!(m.range, 0..3);
            }
            Ok(None) => panic!("expected match"),
            Err(_stop) => panic!("expected match, not stopat"),
        }
    }

    #[test]
    fn stopat_returns_stop_position_for_non_match() {
        let regex = VimRegex::new(r"[0-9]\+").unwrap();
        if !crate::engine::strategy::is_dfa_eligible(&regex.properties) {
            return;
        }
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("abc123");
        // Starting at 0, the pattern requires digits, so "abc" won't match.
        let result = try_match_anchored_stopat(&regex, &mut cache, &ctx, 0);
        match result {
            Ok(Some(_)) => panic!("pattern shouldn't match at pos 0"),
            Ok(None) => panic!("expected Err(stop_pos)"),
            Err(stop_pos) => {
                // Stop position should be 0 or 1 (DFA died immediately on 'a').
                assert!(stop_pos <= 1, "stop_pos={stop_pos} too far");
            }
        }
    }

    /// Case-insensitive search with captures must not route through the
    /// one-pass DFA (whose classifier has no case-folding support).
    /// The full engine should still produce a correct match via Pike VM.
    #[test]
    fn ci_search_still_works_with_captures() {
        let re = VimRegex::new(r"\c\(\d\+\)-\(\d\+\)").unwrap();
        let ctx = MatchContext::simple("123-456");
        let m = re.find(&ctx).unwrap().unwrap();
        assert_eq!(m.range, 0..7);
    }
}
