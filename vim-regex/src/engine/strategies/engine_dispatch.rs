//! Engine dispatch — the terminal strategy of the search cascade.
//!
//! Runs the full Pike VM or backtracker with prefilter acceleration.
//! This strategy NEVER declines — it is always the last in the cascade.

use crate::accel::Prefilter;
use crate::cache::Cache;
use crate::common::SearchResult;
use crate::engine::strategy::{EngineKind, SearchMode};
use crate::engine::VimRegex;
use crate::engines;
use crate::matchers::MatchContext;

/// The single terminal-engine dispatch for forward search. A `Backtracker`
/// (backref/atomic/last-substitute) pattern has NO PikeVM branch: capacity
/// exhaustion becomes `CapacityExceeded`, which the cascade turns into
/// `Err(HaystackTooLarge)`. Routing every terminal dispatch through this one
/// function is what makes the wrong fallback unrepresentable.
pub(crate) fn run_terminal_engine(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
    prefilter: Option<&dyn Prefilter>,
    start_bitmap: Option<&[u32; 8]>,
    inner_literal: Option<&str>,
) -> SearchResult {
    match regex.engine_kind {
        EngineKind::PikeVm => {
            match engines::pike_vm::search(
                &regex.nfa,
                cache,
                ctx,
                start,
                false,
                prefilter,
                start_bitmap,
            ) {
                Some(m) => SearchResult::Match(m),
                None => SearchResult::NoMatch,
            }
        }
        EngineKind::Backtracker => {
            match engines::backtracker::search(&regex.nfa, cache, ctx, start, false, inner_literal)
            {
                SearchResult::Match(m) => SearchResult::Match(m),
                SearchResult::NoMatch => SearchResult::NoMatch,
                // Capacity exhaustion — NEVER PikeVM. Surfaced as HaystackTooLarge.
                SearchResult::Declined | SearchResult::CapacityExceeded => {
                    SearchResult::CapacityExceeded
                }
            }
        }
    }
}

/// Full engine dispatch (never declines as "try next" — capacity exhaustion
/// surfaces as `CapacityExceeded`).
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    let prefilter: Option<&dyn Prefilter> = if ctx.case_sensitive {
        regex.prefilter.as_deref()
    } else {
        regex.ci_prefilter.as_deref()
    };
    let inner_literal = if ctx.case_sensitive {
        regex.inner_literal.as_deref()
    } else {
        None
    };
    let start_bitmap = regex.start_bitmap.as_ref();

    run_terminal_engine(
        regex,
        cache,
        ctx,
        start,
        prefilter,
        start_bitmap,
        inner_literal,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::strategy::SearchMode;
    use crate::matchers::MatchContext;
    use crate::VimRegex;

    #[test]
    fn engine_dispatch_never_declines() {
        // The terminal never returns `Declined` ("try next"). For over-cap
        // inputs it now returns `CapacityExceeded` instead; this small input
        // yields a real `Match`. Either way, `Declined` must never appear.
        let regex = VimRegex::new(r"\(a\)\1").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("aa");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        assert!(!matches!(result, SearchResult::Declined));
    }

    #[test]
    fn engine_dispatch_pike_vm_finds_match() {
        let regex = VimRegex::new(r"wor\w\+").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("hello world");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        match result {
            SearchResult::Match(m) => assert_eq!(&ctx.text[m.range.clone()], "world"),
            _ => panic!("expected Match"),
        }
    }

    #[test]
    fn engine_dispatch_no_match() {
        let regex = VimRegex::new("xyz").unwrap();
        let mut cache = regex.create_cache();
        let ctx = MatchContext::simple("hello world");
        let result = try_search(&regex, &mut cache, 0, &ctx, SearchMode::Full);
        assert!(matches!(result, SearchResult::NoMatch));
    }
}
