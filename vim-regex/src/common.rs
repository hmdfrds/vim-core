//! Shared utility functions for the Vim regex engines.
//!
//! Contains helpers used by both the Pike VM and the bounded backtracker,
//! extracted here to eliminate duplication.

use smallvec::SmallVec;

use super::VimMatch;
use crate::engines::lazy_dfa::DfaSearchResult;

/// Maximum number of capturing groups supported by Vim regex (`\1` through `\9`).
pub(crate) const MAX_CAPTURE_GROUPS: usize = 9;

/// Maximum nesting depth for `\(` groups, to prevent stack overflow on
/// adversarial input like `\(\(\(\(...`.
pub(crate) const MAX_GROUP_NESTING: u32 = 200;

/// Maximum nesting depth for `[...]` collection constructs.
///
/// Independent of the group nesting limit (`MAX_GROUP_NESTING = 200`)
/// and the lookaround depth limit (32). Prevents stack overflow on
/// adversarial patterns like `[[[[[[[[[...`.
pub(crate) const MAX_COLLECTION_DEPTH: u32 = 8;

/// Safety limit on the byte window to scan backward from the cursor when
/// searching in reverse. Both the Pike VM and backtracker use this limit
/// for their `find_backward` implementations. The engine's per-match step
/// limit bounds actual work, so this is just a ceiling on how far back we
/// look.
pub(super) const BACKWARD_SCAN_WINDOW: usize = 1024 * 1024; // 1 MiB

// ═══════════════════════════════════════════════════════════════════════════════
// MEMORY CONFIG
// ═══════════════════════════════════════════════════════════════════════════════

/// Configurable memory budgets for the regex engine.
///
/// All values have sane defaults. Override individual fields for
/// memory-constrained environments (WASM, embedded) or for testing.
///
/// Threading this struct through the entire crate is future work (see the
/// elevation v3 design spec, section 8.6). For now it defines the central
/// source of truth for all budget constants, and its `Default` impl
/// matches the existing hard-coded values throughout the crate.
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// DFA state cache memory budget in bytes.
    ///
    /// When exceeded, the DFA resets its cache. After multiple resets
    /// (thrashing), the DFA gives up and falls back to Pike VM.
    ///
    /// Default: 4 MiB.
    pub dfa_budget: usize,

    /// Backtracker visited-set capacity in bytes.
    ///
    /// The visited set deduplicates `(state, position)` pairs.
    /// Larger values allow longer matches before stack-depth fallback.
    ///
    /// Default: 256 KiB.
    pub visited_capacity: usize,

    /// Maximum backtracker stack depth (in frames).
    ///
    /// Derived from `stack_byte_budget / frame_size`. Exceeding this depth
    /// aborts the current backtracking search and returns `None` (no match
    /// found within the budget) — see `break 'search None` in `backtracker.rs`.
    /// This is distinct from VisitedSet *capacity* exhaustion, which returns
    /// `SearchResult::Declined` and is mapped by the terminal dispatch to
    /// `Err(HaystackTooLarge)`; neither path falls through to Pike VM.
    ///
    /// Default: 10,922 (256 KiB / 24 bytes per frame).
    pub backtracker_max_depth: usize,

    /// Maximum NFA states per pattern.
    ///
    /// Patterns exceeding this budget return `PatternTooComplex`.
    ///
    /// Default: 100,000.
    pub nfa_state_budget: usize,

    /// Backward scan window for reverse Pike VM, in bytes.
    ///
    /// The reverse engine scans at most this many bytes backward
    /// from the match end to find the match start.
    ///
    /// Default: 1 MiB.
    pub backward_scan_window: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            dfa_budget: 4 * 1024 * 1024,   // 4 MiB
            visited_capacity: 256 * 1024,  // 256 KiB
            backtracker_max_depth: 10_922, // 256 KiB / 24 bytes per frame
            nfa_state_budget: 100_000,
            backward_scan_window: 1024 * 1024, // 1 MiB
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of an engine search attempt.
///
/// Used by both the strategy cascade and individual engines.
/// Engines that cannot handle a pattern return `Declined`,
/// allowing the meta-engine to fall through to the next strategy.
#[allow(
    clippy::large_enum_variant,
    reason = "VimMatch is the hot path; boxing adds indirection"
)]
pub(crate) enum SearchResult {
    /// A match was found.
    Match(VimMatch),
    /// No match exists in the searched region.
    NoMatch,
    /// The engine declined (e.g., input too large, pattern not eligible).
    /// Caller should try the next engine/strategy.
    Declined,
    /// The terminal backtracker exhausted its memoization capacity for this
    /// haystack (over the memory cap). Distinct from `Declined` (which means
    /// "strategy not applicable, try next"): this MUST surface as
    /// `Err(HaystackTooLarge)`, never fall through to another engine.
    CapacityExceeded,
}

impl From<DfaSearchResult> for SearchResult {
    fn from(dfa: DfaSearchResult) -> Self {
        match dfa {
            DfaSearchResult::Match { start, end } => {
                SearchResult::Match(VimMatch::new(start..end, start..end, SmallVec::new()))
            }
            DfaSearchResult::NoMatch => SearchResult::NoMatch,
            DfaSearchResult::Quit => SearchResult::Declined,
        }
    }
}

/// Build a `VimMatch` from raw capture data.
///
/// Shared by both the Pike VM and the backtracker to eliminate duplication
/// in match construction. Both engines track the same data: capture slots,
/// `\zs`/`\ze` overrides, and the position where the accept state was reached.
///
/// # Arguments
///
/// * `captures` — capture slot values (even = open, odd = close)
/// * `match_start` — `\zs` override position, if set
/// * `match_end` — `\ze` override position, if set
/// * `search_start` — the text position where scanning began for this match attempt
/// * `accepted_at` — the text position where the accept state was reached
pub(super) fn build_vim_match_from_captures(
    captures: &[Option<usize>],
    match_start: Option<usize>,
    match_end: Option<usize>,
    search_start: usize,
    accepted_at: usize,
) -> VimMatch {
    let full_end = match_end.unwrap_or(accepted_at);
    let range_start = match_start.unwrap_or(search_start);

    let mut result_captures: SmallVec<[Option<std::ops::Range<usize>>; MAX_CAPTURE_GROUPS]> =
        SmallVec::new();
    let max_group = captures.len() / 2;
    for g in 0..max_group.min(MAX_CAPTURE_GROUPS) {
        let open = captures.get(g * 2).copied().flatten();
        let close = captures.get(g * 2 + 1).copied().flatten();
        match (open, close) {
            (Some(s), Some(e)) => result_captures.push(Some(s..e)),
            _ => result_captures.push(None),
        }
    }

    VimMatch::new(
        range_start..full_end,
        search_start..full_end,
        result_captures,
    )
}

/// Advance to the next UTF-8 character boundary after `pos`.
#[inline]
#[must_use]
#[allow(
    clippy::indexing_slicing,
    reason = "loop bound guarantees next < bytes.len()"
)]
pub(super) const fn advance_one_codepoint(text: &str, pos: usize) -> usize {
    if pos >= text.len() {
        return pos.saturating_add(1);
    }
    let bytes = text.as_bytes();
    let mut next = pos + 1;
    while next < bytes.len() {
        if bytes[next] & 0xC0 != 0x80 {
            break;
        }
        next += 1;
    }
    next
}

/// Find the first char boundary at or after `pos`.
#[inline]
#[must_use]
#[allow(
    clippy::indexing_slicing,
    reason = "loop bound guarantees p < bytes.len()"
)]
pub(super) const fn align_to_char_boundary(text: &str, pos: usize) -> usize {
    let bytes = text.as_bytes();
    let mut p = pos;
    while p < bytes.len() {
        if bytes[p] & 0xC0 != 0x80 {
            break;
        }
        p += 1;
    }
    p
}

/// Retreat to the start of the previous UTF-8 codepoint.
///
/// Given a byte position `pos` in valid UTF-8 `text`, returns the byte
/// position of the start of the codepoint immediately before `pos`.
/// If `pos == 0`, returns 0 (clamped).
#[inline]
#[must_use]
#[allow(
    clippy::indexing_slicing,
    reason = "loop bound guarantees i < bytes.len() via pos <= bytes.len()"
)]
pub(super) const fn retreat_one_codepoint(text: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    debug_assert!(
        pos <= text.len(),
        "retreat_one_codepoint: pos out of bounds"
    );
    let bytes = text.as_bytes();
    let mut i = pos - 1;
    while i > 0 && bytes[i] & 0xC0 == 0x80 {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retreat_ascii() {
        assert_eq!(retreat_one_codepoint("hello", 3), 2);
        assert_eq!(retreat_one_codepoint("hello", 1), 0);
        assert_eq!(retreat_one_codepoint("hello", 0), 0);
    }

    #[test]
    fn retreat_two_byte_utf8() {
        let text = "h\u{00E9}llo"; // héllo
        assert_eq!(retreat_one_codepoint(text, 3), 1);
        assert_eq!(retreat_one_codepoint(text, 1), 0);
    }

    #[test]
    fn retreat_three_byte_utf8() {
        let text = "\u{65E5}\u{672C}\u{8A9E}"; // 日本語, 3 bytes each
        assert_eq!(retreat_one_codepoint(text, 6), 3);
        assert_eq!(retreat_one_codepoint(text, 3), 0);
    }

    #[test]
    fn retreat_four_byte_utf8() {
        let text = "a\u{1F600}b"; // a + 😀(4 bytes) + b
        assert_eq!(retreat_one_codepoint(text, 5), 1);
        assert_eq!(retreat_one_codepoint(text, 1), 0);
    }

    #[test]
    fn retreat_from_end() {
        assert_eq!(retreat_one_codepoint("hello", 5), 4);
        assert_eq!(retreat_one_codepoint("h\u{00E9}", 3), 1);
    }

    #[test]
    fn search_result_from_dfa_match() {
        use crate::engines::lazy_dfa::DfaSearchResult;
        let dfa = DfaSearchResult::Match { start: 5, end: 10 };
        let sr: SearchResult = dfa.into();
        match sr {
            SearchResult::Match(m) => {
                assert_eq!(m.range, 5..10);
                assert!(m.captures.is_empty());
            }
            _ => panic!("expected Match"),
        }
    }

    #[test]
    fn search_result_from_dfa_no_match() {
        use crate::engines::lazy_dfa::DfaSearchResult;
        let sr: SearchResult = DfaSearchResult::NoMatch.into();
        assert!(matches!(sr, SearchResult::NoMatch));
    }

    #[test]
    fn search_result_from_dfa_quit() {
        use crate::engines::lazy_dfa::DfaSearchResult;
        let sr: SearchResult = DfaSearchResult::Quit.into();
        assert!(matches!(sr, SearchResult::Declined));
    }
}
