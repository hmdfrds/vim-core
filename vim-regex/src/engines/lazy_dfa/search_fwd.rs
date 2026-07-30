//! Forward DFA search with byte-level scanning.
//!
//! The search operates on `&[u8]` directly. Position always advances by 1 byte.
//! Multi-byte UTF-8 characters cause multiple state transitions automatically.

use super::cache::DfaCache;
use super::DfaSearchResult;
use super::TaggedStateId;
use crate::accel::Prefilter;
use crate::common::advance_one_codepoint;
use crate::matchers::MatchContext;
use crate::nfa::Nfa;

/// Maximum number of cache resets before the DFA gives up.
///
/// Each reset saves/restores the current DFA state, so the search continues
/// from where it left off. Only after this many resets AND the efficiency
/// metric fails (`should_bail()`) does the DFA QUIT to a fallback engine.
const MAX_RESETS: u32 = 8;

// ═══════════════════════════════════════════════════════════════════════════════
// TOP-LEVEL ENTRY POINT
// ═══════════════════════════════════════════════════════════════════════════════

/// Top-level DFA search entry point.
///
/// Searches for the leftmost-longest match in `ctx.text` starting from `start`.
/// Uses an anchored-loop approach: tries the anchored DFA at each position,
/// optionally skipping ahead with a prefilter.
pub(crate) fn dfa_search(
    nfa: &Nfa,
    cache: &mut DfaCache,
    ctx: &MatchContext<'_>,
    start: usize,
    prefilter: Option<&dyn Prefilter>,
) -> DfaSearchResult {
    // If byte class construction overflowed, immediately decline.
    if cache.must_quit() {
        return DfaSearchResult::Quit;
    }

    // Clear newline checkpoints from previous search.
    cache.newline_checkpoints.clear();

    let text = ctx.text;
    let mut pos = start;
    let fast_prefilter = prefilter.filter(|p| p.is_fast());

    loop {
        // Prefilter acceleration: skip to the next candidate position.
        if let Some(pf) = fast_prefilter {
            if let Some(candidate) = pf.find_next(text, pos) {
                pos = candidate;
            } else {
                // Prefilter says no more candidates.
                return DfaSearchResult::NoMatch;
            }
        }

        if pos > text.len() {
            return DfaSearchResult::NoMatch;
        }

        match try_anchored_at(nfa, cache, text, pos) {
            AnchResult::Match(s, e) => return DfaSearchResult::Match { start: s, end: e },
            AnchResult::Quit => return DfaSearchResult::Quit,
            AnchResult::Dead => {
                // Advance past this codepoint and try the next position.
                pos = advance_one_codepoint(text, pos);
                if pos > text.len() {
                    return DfaSearchResult::NoMatch;
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ANCHORED RESULT (internal)
// ═══════════════════════════════════════════════════════════════════════════════

/// Cold path: DFA quit due to thrashing or budget.
#[cold]
#[inline(never)]
fn quit_result() -> AnchResult {
    AnchResult::Quit
}

/// Result of an anchored search attempt at a single position.
enum AnchResult {
    /// Found a match: (match_start, match_end).
    Match(usize, usize),
    /// No match starting at this position (dead state reached).
    Dead,
    /// DFA cannot continue (thrashing / budget exceeded).
    Quit,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ANCHORED SEARCH AT A POSITION
// ═══════════════════════════════════════════════════════════════════════════════

/// Try an anchored DFA search starting at byte position `at`.
///
/// Returns the leftmost-longest match anchored at `at`, or `Dead` if no match
/// is possible from this position, or `Quit` if the DFA cannot continue.
///
/// Uses save/restore across cache resets: before a budget-exceeded reset, the
/// current DFA state's NFA state set is saved. After the reset, the state is
/// restored into the fresh cache and the search continues from where it left off.
fn try_anchored_at(nfa: &Nfa, cache: &mut DfaCache, text: &str, at: usize) -> AnchResult {
    let bytes = text.as_bytes();

    // 1. Compute start state from look-behind context.
    let (prev_newline, prev_word) = compute_look_context(text, at);
    let mut sid = cache.start_state(prev_newline, prev_word);

    // 2. Check for zero-width match at start (e.g. empty pattern).
    let mut last_match: Option<usize> = if sid.is_match() { Some(at) } else { None };
    if sid.is_dead() {
        return match last_match {
            Some(end) => AnchResult::Match(at, end),
            None => AnchResult::Dead,
        };
    }

    // 3. Byte loop with save/restore across cache resets.
    let mut pos = at;
    let mut reset_count: u32 = 0;

    'outer: loop {
        // ── 4x UNROLLED FAST PATH ──────────────────────────────────────
        // Process 4 bytes per iteration when the state is non-tagged.
        // Non-tagged means: not UNKNOWN, DEAD, MATCH, START, or ACCEL.
        // All of those require slow-path handling. The vast majority of
        // bytes flow through non-tagged states, so this eliminates 75%
        // of loop overhead for the common case.
        //
        // SAFETY: The unrolled loop has two invariants:
        // 1. `pos + 3 < bytes.len()` — all four `bytes[pos]` accesses
        //    are in bounds.
        // 2. `!sid.is_tagged()` — the state is a valid, computed DFA
        //    state. `table.next(sid, class)` accesses
        //    `table[sid.index() + class]`, where sid.index() is a valid
        //    premultiplied offset (created by alloc_state()) and class
        //    < num_classes. The premultiplied offset guarantees the
        //    table row exists.
        while pos + 3 < bytes.len() && !sid.is_tagged() {
            sid = next_fast(cache, sid, bytes, pos);
            pos += 1;
            if sid.is_tagged() {
                break;
            }

            sid = next_fast(cache, sid, bytes, pos);
            pos += 1;
            if sid.is_tagged() {
                break;
            }

            sid = next_fast(cache, sid, bytes, pos);
            pos += 1;
            if sid.is_tagged() {
                break;
            }

            sid = next_fast(cache, sid, bytes, pos);
            pos += 1;
            // Loop condition checks is_tagged() on next iteration.
        }

        // Check if unrolled loop landed on MATCH.
        if sid.is_match() {
            last_match = Some(pos);
        }

        // ── SLOW PATH ──────────────────────────────────────────────────
        // Handles: tagged states (ACCEL, MATCH, DEAD, UNKNOWN, START,
        // QUIT), final <4 bytes, assertion handling, and budget resets.
        while pos < bytes.len() {
            // STATE ACCELERATION: single bit test on the current state.
            if sid.is_accel() {
                let ordinal = cache.table.premul_to_ordinal(sid.index() as u32);
                if let Some(ref accel) = cache.accel[ordinal] {
                    let skip_to = super::state_accel::accel_skip(accel, bytes, pos);
                    if skip_to >= bytes.len() {
                        pos = bytes.len();
                        break;
                    }
                    pos = skip_to;
                }
            }

            let byte = bytes[pos];
            let class = cache.classes.classify(byte);

            match cache.transition(sid, class, nfa) {
                Some(next) => {
                    // QUIT sentinel: DFA cannot handle this pattern feature.
                    if next.is_quit() {
                        return AnchResult::Quit;
                    }

                    // Check for assertion match BEFORE consuming.
                    if cache.has_assertion_match(sid, class) {
                        last_match = Some(pos);
                    }

                    // Consume the byte.
                    sid = next;
                    pos += 1;

                    // AFTER consuming: check for match.
                    if sid.is_match() {
                        last_match = Some(pos);
                    }
                    if sid.is_dead() {
                        break;
                    }

                    // Record checkpoint at newline boundaries for
                    // edit-incremental search. The state ordinal is
                    // derived from the premultiplied TaggedStateId.
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "DFA state ordinal fits in u32"
                    )]
                    if byte == b'\n' && !sid.is_tagged() {
                        let ordinal = cache.table.premul_to_ordinal(sid.index() as u32) as u32;
                        cache.newline_checkpoints.push((pos, ordinal));
                    }
                }
                None => {
                    // Budget exceeded. Save state, reset, restore, continue.
                    reset_count += 1;

                    // Save current state before reset.
                    let saver = cache.save_current_state(sid, pos, last_match);

                    // Reset the cache.
                    cache.reset(nfa);

                    // Check if we should give up.
                    if reset_count >= MAX_RESETS && cache.should_bail() {
                        return quit_result();
                    }

                    // Restore saved state into the fresh cache.
                    let (restored_sid, restored_pos, restored_match) =
                        cache.restore_saved_state(&saver, nfa);

                    if restored_sid.is_unknown() || restored_sid.is_dead() {
                        // Could not restore (e.g., saver was Empty or alloc
                        // failed on freshly-reset cache). Fall back to
                        // restarting from the beginning of this anchored
                        // search position.
                        sid = cache.start_state(prev_newline, prev_word);
                        pos = at;
                        last_match = if sid.is_match() { Some(at) } else { None };
                    } else {
                        sid = restored_sid;
                        pos = restored_pos;
                        last_match = restored_match;
                    }

                    // Continue the outer loop (re-enter fast path).
                    continue 'outer;
                }
            }
        }

        // Byte loop finished (pos >= bytes.len()).
        break;
    }

    // Batch-record all bytes consumed.
    cache.record_bytes((pos - at) as u64);

    // 4. End-of-text assertion handling.
    if cache.check_eot_assertions(sid, nfa) {
        last_match = Some(pos);
    }

    // 5. Return result.
    match last_match {
        Some(end) => AnchResult::Match(at, end),
        None => AnchResult::Dead,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STOP-AT SEARCH (anchored with dead-position reporting)
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of an anchored search with stop-position reporting.
#[derive(Debug)]
pub(crate) enum StopAtResult {
    /// Found a match: (start, end).
    Match { start: usize, end: usize },
    /// No match; `stop_pos` is where the DFA died.
    Dead { stop_pos: usize },
    /// DFA cannot continue.
    Quit,
}

/// Anchored DFA search that reports the stop position on non-match.
///
/// Unlike `dfa_search` which tries every position in a bumpalong loop,
/// this function performs a single anchored attempt at `at` and reports
/// where the DFA died if no match is found.
pub(crate) fn dfa_search_anchored_stopat(
    nfa: &Nfa,
    cache: &mut DfaCache,
    ctx: &MatchContext<'_>,
    at: usize,
) -> StopAtResult {
    if cache.must_quit() {
        return StopAtResult::Quit;
    }

    let text = ctx.text;
    let bytes = text.as_bytes();

    let (prev_newline, prev_word) = compute_look_context(text, at);
    let mut sid = cache.start_state(prev_newline, prev_word);

    let mut last_match: Option<usize> = if sid.is_match() { Some(at) } else { None };
    if sid.is_dead() {
        return match last_match {
            Some(end) => StopAtResult::Match { start: at, end },
            None => StopAtResult::Dead { stop_pos: at },
        };
    }

    let mut pos = at;

    while pos < bytes.len() {
        let byte = bytes[pos];
        let class = cache.classes.classify(byte);

        match cache.transition(sid, class, nfa) {
            Some(next) => {
                if next.is_quit() {
                    return StopAtResult::Quit;
                }

                if cache.has_assertion_match(sid, class) {
                    last_match = Some(pos);
                }

                sid = next;
                pos += 1;

                if sid.is_match() {
                    last_match = Some(pos);
                }
                if sid.is_dead() {
                    break;
                }
            }
            None => {
                // Budget exceeded. Report as Quit.
                return StopAtResult::Quit;
            }
        }
    }

    cache.record_bytes((pos - at) as u64);

    if cache.check_eot_assertions(sid, nfa) {
        last_match = Some(pos);
    }

    match last_match {
        Some(end) => StopAtResult::Match { start: at, end },
        None => StopAtResult::Dead { stop_pos: pos },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Fast-path transition for the unrolled loop.
///
/// Reads the cached transition directly from the table without going through
/// the lazy `transition()` method. This is only safe when `sid` is not tagged
/// (guaranteed by the caller's loop condition), which means the state row was
/// fully allocated by `alloc_state()`.
///
/// If the cached entry is UNKNOWN, the returned state will have the UNKNOWN bit
/// set, which is tagged. The unrolled loop's `is_tagged()` check will then
/// break out to the slow path, where `transition()` will lazily compute it.
#[inline(always)]
fn next_fast(cache: &DfaCache, sid: TaggedStateId, bytes: &[u8], pos: usize) -> TaggedStateId {
    debug_assert!(!sid.is_tagged(), "next_fast called on tagged state");
    debug_assert!(pos < bytes.len(), "next_fast called with pos out of bounds");
    let byte = bytes[pos];
    let class = cache.classes.classify(byte);
    cache.table.next(sid, class)
}

/// Compute look-behind context for start state selection.
///
/// At position 0: prev_newline=true (beginning-of-text acts like after newline),
/// prev_word=false.
#[inline]
fn compute_look_context(text: &str, pos: usize) -> (bool, bool) {
    if pos == 0 {
        return (true, false);
    }
    let prev_byte = text.as_bytes()[pos - 1];
    let prev_newline = prev_byte == b'\n';
    let prev_word = prev_byte.is_ascii_alphanumeric() || prev_byte == b'_';
    (prev_newline, prev_word)
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::VimRegex;
    use crate::matchers::MatchContext;

    #[test]
    fn dfa_search_basic_match() {
        let regex = VimRegex::new("hello").unwrap();
        let mut dfa_cache = DfaCache::new(&regex.nfa, true, false);
        let ctx = MatchContext::simple("say hello world");
        let result = dfa_search(&regex.nfa, &mut dfa_cache, &ctx, 0, None);
        match result {
            DfaSearchResult::Match { start, end } => {
                assert_eq!(start, 4);
                assert_eq!(end, 9);
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn dfa_search_no_match() {
        let regex = VimRegex::new("xyz").unwrap();
        let mut dfa_cache = DfaCache::new(&regex.nfa, true, false);
        let ctx = MatchContext::simple("hello world");
        let result = dfa_search(&regex.nfa, &mut dfa_cache, &ctx, 0, None);
        assert!(matches!(result, DfaSearchResult::NoMatch));
    }

    #[test]
    fn dfa_search_survives_cache_clear() {
        // Use a pattern that creates many DFA states but still matches.
        // The DFA should survive cache clears via StateSaver.
        let regex = VimRegex::new(r"[a-z]\+").unwrap();
        let mut cache = regex.create_cache();
        let text = "a".repeat(1000);
        let ctx = MatchContext::simple(&text);
        let result = dfa_search(
            &regex.nfa,
            cache
                .dfa
                .get_or_insert_with(|| DfaCache::new(&regex.nfa, true, false)),
            &ctx,
            0,
            None,
        );
        assert!(
            matches!(result, DfaSearchResult::Match { .. }),
            "expected Match, got {result:?}"
        );
    }
}
