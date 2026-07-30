//! SmallWrite fast path.
//!
//! For inputs under 256 bytes, skip all acceleration and dispatch
//! directly to the engine. The prefilter/DFA/reverse overhead exceeds
//! benefit on tiny inputs.

use crate::accel::Prefilter;
use crate::cache::Cache;
use crate::common::SearchResult;
use crate::engine::strategy::SearchMode;
use crate::engine::VimRegex;
use crate::matchers::MatchContext;

/// Input size threshold below which we skip acceleration.
const SMALL_WRITE_THRESHOLD: usize = 256;

/// SmallWrite fast path.
///
/// For tiny inputs, the overhead of prefilter setup, DFA cache lookup,
/// reverse scan, and strategy cascade iteration exceeds the benefit.
/// Direct engine dispatch is faster.
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    if ctx.text.len() >= SMALL_WRITE_THRESHOLD {
        return SearchResult::Declined;
    }

    // Bypass all acceleration — run the engine directly. SmallWrite has no
    // precomputed start bitmap, so it passes `None`. The shared terminal
    // dispatch maps backtracker capacity exhaustion to `CapacityExceeded`
    // (never PikeVM) — see `engine_dispatch::run_terminal_engine`.
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

    super::engine_dispatch::run_terminal_engine(
        regex,
        cache,
        ctx,
        start,
        prefilter,
        None,
        inner_literal,
    )
}

#[cfg(test)]
mod tests {
    use crate::matchers::MatchContext;
    use crate::VimRegex;

    #[test]
    fn small_write_handles_tiny_input() {
        let re = VimRegex::new(r"\d\+").unwrap();
        let mut cache = re.create_cache();
        let ctx = MatchContext::simple("abc 42 def");
        let m = re.find_with_cache(&mut cache, &ctx).unwrap().unwrap();
        assert_eq!(&ctx.text[m.range.clone()], "42");
    }

    #[test]
    fn small_write_declines_large_input() {
        let re = VimRegex::new("a").unwrap();
        let mut cache = re.create_cache();
        let text = "b".repeat(300) + "a";
        let ctx = MatchContext::simple(&text);
        // SmallWrite declines (text > 256 bytes), but cascade finds match anyway
        let m = re.find_with_cache(&mut cache, &ctx).unwrap().unwrap();
        assert_eq!(m.range, 300..301);
    }
}
