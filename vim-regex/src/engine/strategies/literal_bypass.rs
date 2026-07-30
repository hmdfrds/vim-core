//! Pure literal bypass.
//!
//! When the pattern is a pure literal with no captures and no composing
//! mode, the prefilter alone is authoritative — no NFA needed.

use smallvec::SmallVec;

use crate::accel::Prefilter;
use crate::cache::Cache;
use crate::common::SearchResult;
use crate::engine::strategy::SearchMode;
use crate::engine::VimRegex;
use crate::matchers::MatchContext;
use crate::VimMatch;

/// Pure literal bypass via prefilter.
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    debug_assert!(
        regex.properties.is_literal() && regex.properties.capture_count() == 0,
        "LiteralBypass strategy should only be in cascade for literal patterns with no captures"
    );
    // Dynamic check: composing mode changes per MatchContext.
    if ctx.ignore_composing {
        return SearchResult::Declined;
    }

    // CI prefilter only covers the first 4 bytes. For literals longer than 4
    // bytes in CI mode, the prefilter may find false positives that the full
    // engine would reject. Decline to the full engine path.
    if !ctx.case_sensitive && regex.properties.minimum_match_len() > 4 {
        return SearchResult::Declined;
    }

    let prefilter: Option<&dyn Prefilter> = if ctx.case_sensitive {
        regex.prefilter.as_deref()
    } else {
        regex.ci_prefilter.as_deref()
    };

    match prefilter {
        Some(pf) => {
            if let Some(pos) = pf.find_next(ctx.text, start) {
                cache.prefilter_tracker.record_candidate();
                let len = regex.properties.minimum_match_len();
                SearchResult::Match(VimMatch::new(
                    pos..pos + len,
                    pos..pos + len,
                    SmallVec::new(),
                ))
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
    fn literal_bypass_finds_simple_literal() {
        let regex = VimRegex::new("hello").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("say hello world");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        match result {
            SearchResult::Match(m) => {
                assert_eq!(m.range, 4..9);
            }
            _ => panic!("expected Match"),
        }
    }

    #[test]
    fn literal_bypass_not_in_cascade_for_captures() {
        // Patterns with captures are not literal-eligible, so LiteralBypass
        // should never be in their cascade (verified at compile time in build_strategies).
        let regex = VimRegex::new(r"\(hello\)").unwrap();
        assert!(
            !regex
                .strategies
                .as_slice()
                .contains(&crate::engine::strategy::Strategy::LiteralBypass),
            "LiteralBypass should not be in the cascade for patterns with captures"
        );
    }

    #[test]
    fn literal_bypass_ci_long_literal_declines() {
        use crate::VimRegex;
        // Case-insensitive search for "hello" (5 bytes) should NOT return false positive
        let re = VimRegex::new(r"\chello").unwrap();
        let ctx = MatchContext::simple("HELLx");
        let m = re.find(&ctx).unwrap();
        assert!(
            m.is_none(),
            r"\chello should NOT match 'HELLx' (only first 4 bytes match)"
        );
    }
}
