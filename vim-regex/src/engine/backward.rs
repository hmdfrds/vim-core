//! Backward-search logic for the Vim regex engine.
//!
//! Finds the last match before the cursor position by first trying the
//! reverse NFA fast path, then falling through to the engine-specific
//! backward search.

use crate::cache::Cache;
use crate::engines;
use crate::ir::{VimRegexError, VimRegexErrorKind};
use crate::matchers::MatchContext;
use crate::VimMatch;

use super::dispatch::offset_match;
use super::strategy::EngineKind;
use super::VimRegex;

/// Find the last match before the cursor position, reusing `cache`.
pub(crate) fn find_backward_with_cache(
    regex: &VimRegex,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
) -> Result<Option<VimMatch>, VimRegexError> {
    let resolved = regex.resolve_context(ctx);

    // Narrow the haystack first when the pattern pins buffer positions.
    let (resolved, offset) = regex.apply_range_narrowing(&resolved);

    let prefilter = regex.effective_prefilter(resolved.case_sensitive);
    let inner_literal = regex.effective_inner_literal(resolved.case_sensitive);

    // Reverse NFA fast path for backward search.
    // Skip when ignore_composing is active — reverse NFA cannot account for combining marks.
    if let (Some(ref rev_nfa), Some(cursor)) = (&regex.reverse_nfa, resolved.cursor) {
        if regex.engine_kind == EngineKind::PikeVm && !resolved.ignore_composing {
            if let Some(start) =
                engines::pike_vm::try_match_at_reverse(rev_nfa, cache, &resolved, cursor, 0)
            {
                if let Some(m) =
                    engines::pike_vm::match_anchored(&regex.nfa, cache, &resolved, start)
                {
                    if m.range.start < cursor {
                        return Ok(Some(offset_match(m, offset)));
                    }
                }
            }
        }
    }

    let result = match regex.engine_kind {
        EngineKind::PikeVm => {
            engines::pike_vm::search_backward(&regex.nfa, cache, &resolved, prefilter)
        }
        EngineKind::Backtracker => {
            match engines::backtracker::search_backward(&regex.nfa, cache, &resolved, inner_literal)
            {
                super::strategy::SearchResult::Match(m) => Some(m),
                super::strategy::SearchResult::NoMatch => None,
                // Capacity exhaustion — NEVER PikeVM (which cannot handle
                // backref/atomic/last-substitute). Surface a typed error.
                super::strategy::SearchResult::Declined
                | super::strategy::SearchResult::CapacityExceeded => {
                    return Err(VimRegexError::new(VimRegexErrorKind::HaystackTooLarge {
                        needed: cache.visited_mut().required_bytes(resolved.text.len()),
                        limit: crate::cache::MAX_VISITED_BYTES,
                    }));
                }
            }
        }
    };
    Ok(result.map(|m| offset_match(m, offset)))
}
