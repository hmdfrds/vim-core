//! Anchored start-of-file fast path.
//!
//! When the pattern is anchored to start-of-file (\%^) and we're searching
//! from position 0, try a single anchored match (no scanning loop).

use crate::cache::Cache;
use crate::common::SearchResult;
use crate::engine::strategy::SearchMode;
use crate::engine::VimRegex;
use crate::engines;
use crate::matchers::MatchContext;

/// Tries one anchored match at position 0; declines anywhere else.
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    debug_assert!(
        regex.properties.is_anchored_start_of_file(),
        "AnchoredStart strategy should only be in cascade for anchored-start patterns"
    );
    // Dynamic check: start position changes per search call.
    if start != 0 {
        return SearchResult::Declined;
    }

    match engines::pike_vm::match_anchored(&regex.nfa, cache, ctx, 0) {
        Some(m) => SearchResult::Match(m),
        None => SearchResult::NoMatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::strategy::SearchMode;
    use crate::matchers::MatchContext;
    use crate::VimRegex;

    #[test]
    fn anchored_start_matches_at_position_zero() {
        let regex = VimRegex::new(r"\%^hello").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("hello world");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        assert!(matches!(result, SearchResult::Match(_)));
    }

    #[test]
    fn anchored_start_declines_at_nonzero_position() {
        let regex = VimRegex::new(r"\%^hello").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("hello world");
        let result = try_search(&regex, &mut cache, 5, &ctx, SearchMode::Full);
        assert!(matches!(result, SearchResult::Declined));
    }

    #[test]
    fn anchored_start_no_match() {
        let regex = VimRegex::new(r"\%^xyz").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("hello world");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        assert!(matches!(result, SearchResult::NoMatch));
    }

    #[test]
    fn anchored_possessified_pattern_still_matches() {
        // \%^\w\+: auto-possessifies (word chars disjoint from :), so engine_kind=Backtracker
        // AnchoredStart should NOT be in cascade; EngineDispatch handles it correctly
        let regex = VimRegex::new(r"\%^\w\+:").unwrap();
        let mut cache = regex.create_cache();
        let text = "hello:world";
        let ctx = MatchContext::simple(text);
        let m = regex.find_with_cache(&mut cache, &ctx).unwrap();
        assert!(m.is_some(), r"\%^\w\+: should match 'hello:world'");
        assert_eq!(m.unwrap().range, 0..6);
    }
}
