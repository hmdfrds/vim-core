//! Reverse DFA search for finding match start positions.
//!
//! Given a known match end position from the forward DFA, scans backward
//! through the text using the reverse NFA's lazy DFA to find the match
//! start. The reverse NFA is pre-built during compilation for eligible
//! patterns.

use super::cache::DfaCache;
use super::DfaSearchResult;
use crate::matchers::MatchContext;
use crate::nfa::Nfa;

/// Reverse DFA search: find the start of a match ending at `end_pos`.
///
/// Scans backward from `end_pos` toward the beginning of the text.
/// The reverse NFA was constructed by `NfaBuilder::build_reverse()` so
/// its "forward" direction corresponds to reading the text backward.
///
/// Returns `Match { start, end }` where `start` is the leftmost position
/// from which the match can begin, and `end` equals `end_pos`.
pub(crate) fn dfa_search_reverse(
    reverse_nfa: &Nfa,
    cache: &mut DfaCache,
    ctx: &MatchContext<'_>,
    end_pos: usize,
) -> DfaSearchResult {
    if cache.must_quit() {
        return DfaSearchResult::Quit;
    }

    let text = ctx.text;
    let bytes = text.as_bytes();

    if end_pos > bytes.len() {
        return DfaSearchResult::NoMatch;
    }

    // The reverse NFA's start state corresponds to the end of the forward
    // match. Compute look-behind context at end_pos (looking at the character
    // at end_pos in the forward direction, which is the "previous" character
    // in the reverse scan direction).
    let (prev_newline, prev_word) = compute_reverse_look_context(text, end_pos);
    let mut sid = cache.start_state(prev_newline, prev_word);

    // Check for zero-width match at end_pos.
    let mut last_match: Option<usize> = if sid.is_match() { Some(end_pos) } else { None };

    if sid.is_dead() {
        return match last_match {
            Some(start) => DfaSearchResult::Match {
                start,
                end: end_pos,
            },
            None => DfaSearchResult::NoMatch,
        };
    }

    // Scan backward byte by byte.
    let mut pos = end_pos;

    loop {
        if pos == 0 {
            break;
        }

        pos -= 1;
        let byte = bytes[pos];
        let class = cache.classes.classify(byte);

        match cache.transition(sid, class, reverse_nfa) {
            Some(next) => {
                if next.is_quit() {
                    return DfaSearchResult::Quit;
                }

                // Check for assertion match BEFORE consuming (reverse direction).
                if cache.has_assertion_match(sid, class) {
                    last_match = Some(pos);
                }

                sid = next;

                if sid.is_match() {
                    // In the reverse direction, the "match" position is
                    // the start of the forward match.
                    last_match = Some(pos);
                }

                if sid.is_dead() {
                    break;
                }
            }
            None => {
                // Budget exceeded. For reverse search (which is typically
                // bounded to a small region), simply QUIT and let the
                // Pike VM handle it.
                return DfaSearchResult::Quit;
            }
        }
    }

    // Record bytes consumed.
    let bytes_consumed = end_pos.saturating_sub(pos);
    cache.record_bytes(bytes_consumed as u64);

    // Check EOT assertions at position 0 (start of text in reverse = end
    // of reverse scan).
    if pos == 0 && cache.check_eot_assertions(sid, reverse_nfa) {
        last_match = Some(0);
    }

    match last_match {
        Some(start) => DfaSearchResult::Match {
            start,
            end: end_pos,
        },
        None => DfaSearchResult::NoMatch,
    }
}

/// Compute look-behind context for reverse start state selection.
///
/// In the reverse direction, "look-behind" means looking at the character
/// at `pos` (the character just past the end of what we're about to scan).
#[inline]
fn compute_reverse_look_context(text: &str, pos: usize) -> (bool, bool) {
    if pos >= text.len() {
        // At end of text: treat as after-newline for reverse $ matching.
        return (true, false);
    }
    let byte = text.as_bytes()[pos];
    let prev_newline = byte == b'\n';
    let prev_word = byte.is_ascii_alphanumeric() || byte == b'_';
    (prev_newline, prev_word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::VimRegex;
    use crate::engines::lazy_dfa::DfaCache;
    use crate::matchers::MatchContext;

    #[test]
    fn reverse_dfa_finds_start_of_literal() {
        let regex = VimRegex::new("hello").unwrap();
        let rev_nfa = match regex.reverse_nfa.as_ref() {
            Some(n) => n,
            None => return,
        };
        let mut rev_cache = DfaCache::new(rev_nfa, true, false);
        let ctx = MatchContext::simple("say hello world");
        let result = dfa_search_reverse(rev_nfa, &mut rev_cache, &ctx, 9);
        match result {
            DfaSearchResult::Match { start, .. } => {
                assert_eq!(start, 4, "reverse DFA should find start=4");
            }
            DfaSearchResult::NoMatch => panic!("expected reverse match"),
            DfaSearchResult::Quit => {} // Acceptable
        }
    }

    #[test]
    fn reverse_dfa_no_match_in_range() {
        let regex = VimRegex::new("xyz").unwrap();
        let rev_nfa = match regex.reverse_nfa.as_ref() {
            Some(n) => n,
            None => return,
        };
        let mut rev_cache = DfaCache::new(rev_nfa, true, false);
        let ctx = MatchContext::simple("hello world");
        let result = dfa_search_reverse(rev_nfa, &mut rev_cache, &ctx, 5);
        assert!(matches!(
            result,
            DfaSearchResult::NoMatch | DfaSearchResult::Quit
        ));
    }

    #[test]
    fn reverse_dfa_at_start_of_text() {
        let regex = VimRegex::new("he").unwrap();
        let rev_nfa = match regex.reverse_nfa.as_ref() {
            Some(n) => n,
            None => return,
        };
        let mut rev_cache = DfaCache::new(rev_nfa, true, false);
        let ctx = MatchContext::simple("hello");
        let result = dfa_search_reverse(rev_nfa, &mut rev_cache, &ctx, 2);
        match result {
            DfaSearchResult::Match { start, end } => {
                assert_eq!(start, 0);
                assert_eq!(end, 2);
            }
            DfaSearchResult::NoMatch => panic!("expected match at start"),
            DfaSearchResult::Quit => {} // Acceptable
        }
    }

    #[test]
    fn reverse_dfa_empty_text() {
        let regex = VimRegex::new("a").unwrap();
        let rev_nfa = match regex.reverse_nfa.as_ref() {
            Some(n) => n,
            None => return,
        };
        let mut rev_cache = DfaCache::new(rev_nfa, true, false);
        let ctx = MatchContext::simple("");
        let result = dfa_search_reverse(rev_nfa, &mut rev_cache, &ctx, 0);
        assert!(matches!(
            result,
            DfaSearchResult::NoMatch | DfaSearchResult::Quit
        ));
    }
}
