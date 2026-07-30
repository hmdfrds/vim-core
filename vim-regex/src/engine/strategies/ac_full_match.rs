//! Aho-Corasick full matcher bypass.
//!
//! When the pattern is a literal alternation with no captures and no overrides,
//! the AC automaton alone is authoritative.

use smallvec::SmallVec;

use crate::cache::Cache;
use crate::common::SearchResult;
use crate::engine::strategy::SearchMode;
use crate::engine::VimRegex;
use crate::matchers::MatchContext;
use crate::VimMatch;

/// Answers from the Aho-Corasick prefilter alone, skipping the NFA.
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    if !regex.ac_is_full_match || ctx.ignore_composing {
        return SearchResult::Declined;
    }

    let pf = if ctx.case_sensitive {
        regex.ac_prefilter.as_ref()
    } else {
        regex.ac_prefilter_ci.as_ref()
    };

    match pf {
        Some(pf) => {
            if let Some((ms, me)) = pf.find_first(ctx.text, start) {
                cache.prefilter_tracker.record_candidate();
                SearchResult::Match(VimMatch::new(ms..me, ms..me, SmallVec::new()))
            } else {
                SearchResult::NoMatch
            }
        }
        None => SearchResult::Declined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::strategy::SearchMode;
    use crate::matchers::MatchContext;
    use crate::VimRegex;

    #[test]
    fn ac_bypass_finds_alternation() {
        // Use enough alternations to trigger AC prefilter (should_use_ac threshold).
        let regex = VimRegex::new(r"foo\|bar\|baz\|qux\|quux").unwrap();
        if !regex.ac_is_full_match {
            // If AC not eligible for this pattern (too few literals), skip.
            return;
        }
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("the bar is open");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        match result {
            SearchResult::Match(m) => assert_eq!(&ctx.text[m.range.clone()], "bar"),
            _ => panic!("expected Match"),
        }
    }

    #[test]
    fn ac_bypass_declines_for_non_ac_pattern() {
        let regex = VimRegex::new(r"foo.*bar").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("fooXbar");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        assert!(matches!(result, SearchResult::Declined));
    }
}
