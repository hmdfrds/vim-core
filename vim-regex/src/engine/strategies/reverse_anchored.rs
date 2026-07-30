//! Reverse anchored-end strategy.
//!
//! When the pattern ends with \%$, run the reverse NFA backward from EOT
//! to find the match start, then confirm with a forward anchored match.

use crate::cache::Cache;
use crate::common::SearchResult;
use crate::engine::strategy::{EngineKind, SearchMode};
use crate::engine::VimRegex;
use crate::engines;
use crate::matchers::MatchContext;

/// Reverse anchored end-of-file search.
pub(crate) fn try_search(
    regex: &VimRegex,
    cache: &mut Cache,
    start: usize,
    ctx: &MatchContext<'_>,
    _mode: SearchMode,
) -> SearchResult {
    debug_assert!(
        regex.engine_kind == EngineKind::PikeVm && regex.properties.is_anchored_end_of_file(),
        "ReverseAnchored strategy should only be in cascade for PikeVm anchored-end patterns"
    );
    let rev_nfa = match regex.reverse_nfa.as_ref() {
        Some(n) => n,
        None => return SearchResult::Declined,
    };
    // Dynamic check: composing mode changes per MatchContext.
    if ctx.ignore_composing {
        return SearchResult::Declined;
    }

    let text_end = ctx.text.len();
    let scan_lower = start.max(text_end.saturating_sub(regex.max_reverse_distance));

    if let Some(match_start) =
        engines::pike_vm::try_match_at_reverse(rev_nfa, cache, ctx, text_end, scan_lower)
    {
        if match_start >= start {
            if let Some(m) = engines::pike_vm::match_anchored(&regex.nfa, cache, ctx, match_start) {
                return SearchResult::Match(m);
            }
        }
    }

    // Reverse NFA couldn't find a start — not authoritative, decline.
    SearchResult::Declined
}

#[cfg(test)]
mod tests {
    use crate::VimRegex;

    #[test]
    fn reverse_anchored_not_in_cascade_for_non_anchored_end() {
        // Non-anchored-end patterns should not have ReverseAnchored in their
        // cascade (verified at compile time in build_strategies).
        let regex = VimRegex::new("hello").unwrap();
        assert!(
            !regex
                .strategies
                .as_slice()
                .contains(&crate::engine::strategy::Strategy::ReverseAnchored),
            "ReverseAnchored should not be in the cascade for non-anchored-end patterns"
        );
    }
}
