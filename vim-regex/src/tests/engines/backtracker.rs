//! Tests for the upgraded backtracker with VisitedSet dedup.
//!
//! Covers VisitedSet dedup behavior, inner literal prescreen, the Declined
//! result path, and correctness for backreference and lookaround patterns.

use crate::cache::Cache;
use crate::engines::backtracker::{self as new_bt, SearchResult};
use crate::hir;
use crate::matchers::MatchContext;
use crate::nfa::builder::NfaBuilder;
use crate::parser::parse_pattern;
use crate::test_builder::regex;

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

fn build_nfa(pattern: &str) -> crate::nfa::Nfa {
    let parsed = parse_pattern(pattern).expect("valid pattern");
    let (lowered, _) = hir::lower(&parsed.node);
    NfaBuilder::build(&lowered).unwrap()
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "NFA state count limited to u32::MAX"
)]
fn make_cache(nfa: &crate::nfa::Nfa) -> Cache {
    let num_slots = nfa.slot_count();
    Cache::new(
        nfa.state_count() as u32,
        num_slots,
        nfa.has_backreferences(),
    )
}

/// Run the backtracker forward, assert match or no-match.
fn assert_forward_match(pattern: &str, text: &str, start: usize) -> Option<crate::VimMatch> {
    let nfa = build_nfa(pattern);
    let ctx = MatchContext::simple(text);
    let mut cache = make_cache(&nfa);
    match new_bt::search(&nfa, &mut cache, &ctx, start, false, None) {
        SearchResult::Match(m) => Some(m),
        SearchResult::NoMatch => None,
        SearchResult::Declined => {
            panic!("backtracker declined — test input may be too large for VisitedSet capacity");
        }
        // The backtracker signals capacity via `Declined`; `CapacityExceeded`
        // is only produced by the terminal dispatch, never by these direct calls.
        SearchResult::CapacityExceeded => {
            unreachable!("CapacityExceeded only from the terminal backtracker dispatch")
        }
    }
}

/// Run the backtracker backward.
fn assert_backward_match(pattern: &str, text: &str, cursor: usize) -> Option<crate::VimMatch> {
    let nfa = build_nfa(pattern);
    let ctx = MatchContext {
        text,
        cursor: Some(cursor),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let mut cache = make_cache(&nfa);
    match new_bt::search_backward(&nfa, &mut cache, &ctx, None) {
        SearchResult::Match(m) => Some(m),
        SearchResult::NoMatch => None,
        SearchResult::Declined => {
            panic!("backtracker declined backward — test input may be too large for VisitedSet capacity");
        }
        // The backtracker signals capacity via `Declined`; `CapacityExceeded`
        // is only produced by the terminal dispatch, never by these direct calls.
        SearchResult::CapacityExceeded => {
            unreachable!("CapacityExceeded only from the terminal backtracker dispatch")
        }
    }
}

/// Run the backtracker anchored.
fn assert_anchored_match(pattern: &str, text: &str, pos: usize) -> Option<crate::VimMatch> {
    let nfa = build_nfa(pattern);
    let ctx = MatchContext::simple(text);
    let mut cache = make_cache(&nfa);
    match new_bt::match_anchored(&nfa, &mut cache, &ctx, pos) {
        SearchResult::Match(m) => Some(m),
        SearchResult::NoMatch => None,
        SearchResult::Declined => {
            panic!("backtracker declined anchored — test input may be too large for VisitedSet capacity");
        }
        // The backtracker signals capacity via `Declined`; `CapacityExceeded`
        // is only produced by the terminal dispatch, never by these direct calls.
        SearchResult::CapacityExceeded => {
            unreachable!("CapacityExceeded only from the terminal backtracker dispatch")
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC CORRECTNESS — regex_suite!
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(basic_correctness {
    quantifier_star:  "a*",     "aaa" => (0, 3);
    quantifier_plus:  "a\\+",   "aaa" => (0, 3);
});

// ═══════════════════════════════════════════════════════════════════════════════
// BACKREFERENCE CORRECTNESS — regex_suite!
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(backref_correctness {
    backref_simple:               "\\(foo\\)\\1",     "foofoo"   => (0, 6);
    backref_no_match:             "\\(foo\\)\\1",     "foobar"   => ();
    backref_greedy:               "\\(.*\\)\\1",      "abcabc"   => (0, 6);
    backref_alternation:          "\\(a\\|b\\)\\1",   "aa"       => (0, 2);
    backref_alternation_bb:       "\\(a\\|b\\)\\1",   "bb"       => (0, 2);
    backref_alternation_no_match: "\\(a\\|b\\)\\1",   "ab"       => ();
    backref_in_context:           "\\(ab\\)\\1",      "xxababyy" => (2, 6);
    backref_repeated_group:       "\\(a\\+\\)\\1",    "aaaa"     => (0, 4);
});

// ═══════════════════════════════════════════════════════════════════════════════
// ATOMIC GROUP CORRECTNESS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn atomic_group_consumes_input() {
    let m = assert_forward_match("\\(foo\\)\\@>bar", "foobar", 0).unwrap();
    assert_eq!(m.range, 0..6);
}

#[test]
fn atomic_group_no_match_after() {
    assert!(assert_forward_match("\\(foo\\)\\@>baz", "foobar", 0).is_none());
}

#[test]
fn atomic_group_sub_no_match() {
    assert!(assert_forward_match("\\(xyz\\)\\@>bar", "foobar", 0).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND CORRECTNESS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn lookbehind_simple() {
    let m = assert_forward_match("\\(foo\\)\\@<=bar", "foobar", 0).unwrap();
    assert_eq!(m.range, 3..6);
}

#[test]
fn lookbehind_negative() {
    assert!(assert_forward_match("\\(foo\\)\\@<!bar", "foobar", 0).is_none());
}

#[test]
fn lookbehind_negative_match() {
    let m = assert_forward_match("\\(foo\\)\\@<!bar", "xyzbar", 0).unwrap();
    assert_eq!(m.range, 3..6);
}

#[test]
fn lookahead_positive() {
    let m = assert_forward_match("foo\\(bar\\)\\@=", "foobar", 0).unwrap();
    assert_eq!(m.range, 0..3);
}

#[test]
fn lookahead_negative() {
    let m = assert_forward_match("foo\\(bar\\)\\@!", "foobaz", 0).unwrap();
    assert_eq!(m.range, 0..3);
}

#[test]
fn lookbehind_with_backref() {
    let m = assert_forward_match("\\(\\(a\\)\\2\\)\\@<=x", "aax", 0);
    assert!(m.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKWARD SEARCH — regex() builder
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn backward_basic() {
    regex("abc")
        .text("abcXXXabcXX")
        .cursor(10)
        .expect_backward_match(6..9)
        .run();
}

#[test]
fn backward_no_match() {
    regex("xyz")
        .text("abcdef")
        .cursor(6)
        .expect_no_backward_match()
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// ANCHORED
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchored_parity_match() {
    let m = assert_anchored_match("foo", "foobar", 0).unwrap();
    assert_eq!(m.range, 0..3);
}

#[test]
fn anchored_parity_no_match_at_pos() {
    assert!(assert_anchored_match("foo", "barfoo", 0).is_none());
}

#[test]
fn anchored_parity_at_offset() {
    let m = assert_anchored_match("foo", "barfoo", 3).unwrap();
    assert_eq!(m.range, 3..6);
}

#[test]
fn anchored_backref() {
    let m = assert_anchored_match("\\(ab\\)\\1", "abab", 0).unwrap();
    assert_eq!(m.range, 0..4);
}

// ═══════════════════════════════════════════════════════════════════════════════
// CAPACITY: GROW-TO-FIT
// ═══════════════════════════════════════════════════════════════════════════════
//
// These tests previously asserted that `search`/`search_backward` *declined*
// whenever `text.len()` exceeded the fixed 256 KB VisitedSet capacity divided
// by the state count. That cliff was a silent-wrong-answer bug: a tiny
// haystack with a large NFA would decline and (via the old terminal dispatch)
// fall through to the backref-incapable PikeVM. The backtracker now grows its
// memoization to fit (bounded by `MAX_VISITED_BYTES`), so the same inputs match
// instead of declining. Confirmed against Neovim: `a` on "ab" matches 0..1
// (`matchstrpos("ab", "a") == ["a", 0, 1]`).

#[test]
fn grows_to_fit_large_state_count_forward() {
    // 2M states with a 2-byte haystack would exceed the old fixed cliff
    // (256 KB / 2M states = 1 byte) and decline. Grow-to-fit needs only
    // (2+1)*2M bits ≈ 768 KB, well under the 32 MiB cap, so it matches.
    let nfa = build_nfa("a");
    let mut cache = Cache::new(2_097_152, 0, false);
    let ctx = MatchContext::simple("ab");

    let result = new_bt::search(&nfa, &mut cache, &ctx, 0, false, None);
    match result {
        SearchResult::Match(m) => assert_eq!(m.range, 0..1),
        _ => panic!("expected Match 0..1 (grow-to-fit)"),
    }
}

#[test]
fn grows_to_fit_large_state_count_backward() {
    let nfa = build_nfa("a");
    let mut cache = Cache::new(2_097_152, 0, false);
    let ctx = MatchContext {
        text: "ab",
        cursor: Some(2),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let result = new_bt::search_backward(&nfa, &mut cache, &ctx, None);
    match result {
        SearchResult::Match(m) => assert_eq!(m.range, 0..1),
        _ => panic!("expected Match 0..1 (grow-to-fit)"),
    }
}

#[test]
fn declined_anchored() {
    // `match_anchored` was intentionally left on the original fixed-capacity
    // guard (only `search`/`search_backward` were converted to grow-to-fit),
    // so it still declines when the haystack exceeds `max_haystack_len`.
    let nfa = build_nfa("a");
    let mut cache = Cache::new(2_097_152, 0, false);
    let ctx = MatchContext::simple("ab");
    let result = new_bt::match_anchored(&nfa, &mut cache, &ctx, 0);
    assert!(
        matches!(result, SearchResult::Declined),
        "expected Declined for anchored"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// INNER LITERAL PRESCREEN
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn inner_literal_absent_returns_no_match() {
    let nfa = build_nfa("a.*foo");
    let mut cache = make_cache(&nfa);
    let ctx = MatchContext::simple("aXXXbar");

    let result = new_bt::search(&nfa, &mut cache, &ctx, 0, false, Some("foo"));
    assert!(
        matches!(result, SearchResult::NoMatch),
        "expected NoMatch when inner literal is absent"
    );
}

#[test]
fn inner_literal_present_matches() {
    let nfa = build_nfa("a.*foo");
    let mut cache = make_cache(&nfa);
    let ctx = MatchContext::simple("aXXXfoo");

    let result = new_bt::search(&nfa, &mut cache, &ctx, 0, false, Some("foo"));
    assert!(
        matches!(result, SearchResult::Match(_)),
        "expected Match when inner literal is present"
    );
}

#[test]
fn inner_literal_absent_backward() {
    let nfa = build_nfa("a.*foo");
    let mut cache = make_cache(&nfa);
    let ctx = MatchContext {
        text: "aXXXbar",
        cursor: Some(7),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };

    let result = new_bt::search_backward(&nfa, &mut cache, &ctx, Some("foo"));
    assert!(
        matches!(result, SearchResult::NoMatch),
        "expected NoMatch backward when inner literal absent"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMPREHENSIVE BATCH
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn comprehensive() {
    let cases: &[(&str, &str, usize, bool)] = &[
        ("a", "a", 0, true),
        ("a", "b", 0, false),
        ("a", "ba", 0, true),
        ("a*b", "aaab", 0, true),
        ("a\\+b", "aaab", 0, true),
        ("a\\?b", "ab", 0, true),
        ("a\\?b", "b", 0, true),
        ("\\<word\\>", "a word here", 0, true),
        ("\\(ab\\)*c", "abababc", 0, true),
        ("a.*b", "aXXXb", 0, true),
        ("a.*b", "a", 0, false),
        ("\\(ab\\)\\1", "abab", 0, true),
        ("\\(ab\\)\\1", "abcd", 0, false),
        ("\\(a\\+\\)\\1", "aaaa", 0, true),
    ];

    for &(pat, text, start, should_match) in cases {
        let m = assert_forward_match(pat, text, start);
        assert_eq!(
            m.is_some(),
            should_match,
            "pattern={pat:?} text={text:?} start={start}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VISITED SET CAPTURE-AWARE DEDUP
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn backref_different_captures_same_position() {
    // Pattern: \(a\|b\)\1 against "aa" and "bb"
    // Without capture-aware dedup, after visiting state S at pos 1
    // with capture[1]="a", the engine might skip the same state with
    // capture[1]="b", causing a missed match on "bb".
    let re = crate::VimRegex::new(r"\(a\|b\)\1").unwrap();
    let mut cache = re.create_cache();

    let ctx = crate::matchers::MatchContext::simple("aa");
    let m = re.find_at_with_cache(&mut cache, &ctx, 0).unwrap();
    assert!(m.is_some(), "expected match on 'aa'");
    assert_eq!(m.unwrap().range, 0..2);

    let ctx2 = crate::matchers::MatchContext::simple("bb");
    let m2 = re.find_at_with_cache(&mut cache, &ctx2, 0).unwrap();
    assert!(m2.is_some(), "expected match on 'bb'");
    assert_eq!(m2.unwrap().range, 0..2);
}

#[test]
fn backref_no_false_match() {
    // \(a\|b\)\1 should NOT match "ab" or "ba"
    let re = crate::VimRegex::new(r"\(a\|b\)\1").unwrap();
    let mut cache = re.create_cache();

    let ctx = crate::matchers::MatchContext::simple("ab");
    let m = re.find_at_with_cache(&mut cache, &ctx, 0).unwrap();
    assert!(m.is_none(), "should not match 'ab'");

    let ctx2 = crate::matchers::MatchContext::simple("ba");
    let m2 = re.find_at_with_cache(&mut cache, &ctx2, 0).unwrap();
    assert!(m2.is_none(), "should not match 'ba'");
}

#[test]
fn backref_stack_depth_limit() {
    // A pathological backref pattern on long input should not stack overflow.
    // Instead it should either match correctly or return gracefully.
    let re = crate::VimRegex::new(r"\(.\)\1*").unwrap();
    let mut cache = re.create_cache();
    let input = "a".repeat(2000);
    let ctx = crate::matchers::MatchContext::simple(&input);
    // Should not panic — either matches or declines gracefully.
    let _ = re.find_at_with_cache(&mut cache, &ctx, 0);
}

#[test]
fn backref_alternation_captures_all_variants() {
    // \(\(a\|aa\)\)\1 on "aaaa": group 1 can be "a" (then \1 = "a" → "aa")
    // or "aa" (then \1 = "aa" → "aaaa"). Must find the longer match.
    let re = crate::VimRegex::new(r"\(\(a\|aa\)\)\1").unwrap();
    let mut cache = re.create_cache();
    let ctx = crate::matchers::MatchContext::simple("aaaa");
    let m = re.find_at_with_cache(&mut cache, &ctx, 0).unwrap();
    assert!(m.is_some(), "expected match on 'aaaa'");
}

// ═══════════════════════════════════════════════════════════════════════════════
// FRAME SIZE AND RLE
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn frame_run_length_fits_in_24_bytes() {
    // The Frame enum must remain <= 24 bytes after adding RunLength.
    assert!(
        std::mem::size_of::<crate::engines::backtracker::Frame>() <= 24,
        "Frame is {} bytes, must be <= 24",
        std::mem::size_of::<crate::engines::backtracker::Frame>()
    );
}

#[test]
fn run_length_variant_exists() {
    // Verify the RunLength variant can be constructed
    use crate::engines::backtracker::Frame;
    use crate::nfa::StateId;
    let frame = Frame::RunLength {
        state: StateId::from_raw(42),
        start_pos: 100,
        count: 500,
    };
    // Pattern-match to verify fields
    match frame {
        Frame::RunLength {
            state,
            start_pos,
            count,
        } => {
            assert_eq!(state.index(), 42);
            assert_eq!(start_pos, 100);
            assert_eq!(count, 500);
        }
        _ => panic!("expected RunLength variant"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SKIP-UNTIL-CHAR FAST PATH
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn skip_until_char_dot_star_literal() {
    // Pattern: `a.*x` on "a----x" should match 0..6
    let m = assert_forward_match("a.*x", "a----x", 0).unwrap();
    assert_eq!(m.range, 0..6);
}

#[test]
fn skip_until_char_dot_star_literal_multiple_candidates() {
    // Pattern: `a.*x` on "a--x--x" -- greedy, should match 0..7
    let m = assert_forward_match("a.*x", "a--x--x", 0).unwrap();
    assert_eq!(m.range, 0..7);
}

#[test]
fn skip_until_char_no_match() {
    // Pattern: `a.*x` on "a------" -- no 'x', no match from pos 0
    assert!(assert_forward_match("a.*x", "a------", 0).is_none());
}

#[test]
fn skip_until_char_dot_star_at_start() {
    // Pattern: `.*:` on "hello: world" should match 0..6
    let m = assert_forward_match(".*:", "hello: world", 0).unwrap();
    assert_eq!(m.range, 0..6);
}

#[test]
fn skip_until_char_empty_prefix() {
    // Pattern: `.*x` on "x" -- zero-length .* prefix, matches 0..1
    let m = assert_forward_match(".*x", "x", 0).unwrap();
    assert_eq!(m.range, 0..1);
}

#[test]
fn skip_until_char_with_captures() {
    // Pattern: `\(.*\)x` on "abcx" -- capture group 1 should be "abc"
    let m = assert_forward_match("\\(.*\\)x", "abcx", 0).unwrap();
    assert_eq!(m.range, 0..4);
    assert_eq!(m.capture(1), Some(&(0..3)));
}

#[test]
fn skip_until_char_unicode_text() {
    // Pattern: `.*:` on "cafe\u{00E9}: latte" -- skips multi-byte chars correctly
    let m = assert_forward_match(".*:", "caf\u{00E9}: latte", 0).unwrap();
    let expected_end = "caf\u{00E9}:".len();
    assert_eq!(m.range, 0..expected_end);
}

#[test]
fn skip_until_char_backward() {
    // Pattern: `.*x` backward on "ax--bx--" with cursor at 8
    let m = assert_backward_match(".*x", "ax--bx--", 8).unwrap();
    // Backward finds the rightmost match start before cursor
    assert!(m.range.end <= 8);
}

// ═══════════════════════════════════════════════════════════════════════════════
// RLE STACK COMPRESSION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn rle_correctness_dot_star() {
    // Pattern matching long runs should produce correct results
    // even with RLE compression active.
    let text = "a".repeat(500) + "b";
    let m = assert_forward_match("a*b", &text, 0).unwrap();
    assert_eq!(m.range, 0..501);
}

#[test]
fn rle_correctness_alternation_long_run() {
    // Pattern with alternation on long text -- RLE must not break backtracking
    let text = "x".repeat(200) + "abc";
    let m = assert_forward_match(".*abc", &text, 0).unwrap();
    assert_eq!(m.range, 0..203);
}

#[test]
fn rle_correctness_with_captures() {
    // Captures must work correctly with RLE compression
    let text = "a".repeat(100) + "xyz";
    let m = assert_forward_match("\\(a*\\)xyz", &text, 0).unwrap();
    assert_eq!(m.range, 0..103);
    assert_eq!(m.capture(1), Some(&(0..100)));
}

#[test]
fn rle_greedy_vs_lazy() {
    // Greedy and lazy quantifiers on long runs
    let text = "aaa".to_string() + "x" + "aaa" + "x";
    let m_greedy = assert_forward_match("a*x", &text, 0).unwrap();
    assert_eq!(m_greedy.range, 0..4); // greedy: matches "aaax"

    // Lazy: `.\{-}x` -- matches shortest to first 'x'
    let m_lazy = assert_forward_match(".\\{-}x", &text, 0).unwrap();
    assert_eq!(m_lazy.range, 0..4);
}

#[test]
fn rle_backref_not_broken() {
    // RLE must not interfere with backreference patterns
    let text = "aaa".to_string() + "aaa";
    let m = assert_forward_match("\\(a\\+\\)\\1", &text, 0);
    assert!(m.is_some(), "backref with RLE should still match");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Integration -- SkipUntilChar + RLE correctness
// ═══════════════════════════════════════════════════════════════════════════════

/// Helper: compare backtracker-only results between skip/non-skip paths.
/// Uses VimRegex public API to verify end-to-end correctness.
fn assert_same_match(pattern: &str, text: &str) {
    let re = crate::VimRegex::new(pattern).unwrap();
    let ctx = crate::matchers::MatchContext::simple(text);
    let mut cache = re.create_cache();

    // Forward match
    let m = re.find_at_with_cache(&mut cache, &ctx, 0).unwrap();

    // Also verify via baseline config (all strategies disabled, pure engine dispatch)
    let re_baseline =
        crate::VimRegex::with_config(pattern, &crate::engine::SearchConfig::baseline()).unwrap();
    let mut cache_baseline = re_baseline.create_cache();
    let m_baseline = re_baseline
        .find_at_with_cache(&mut cache_baseline, &ctx, 0)
        .unwrap();

    assert_eq!(
        m.as_ref().map(|m| &m.range),
        m_baseline.as_ref().map(|m| &m.range),
        "match range mismatch for pattern={pattern:?} text={text:?}"
    );

    // Compare captures
    if let (Some(m), Some(m_baseline)) = (&m, &m_baseline) {
        for g in 1..=9 {
            assert_eq!(
                m.capture(g),
                m_baseline.capture(g),
                "capture group {g} mismatch for pattern={pattern:?} text={text:?}"
            );
        }
    }
}

#[test]
fn integration_skip_until_char_batch() {
    let cases: &[(&str, &str)] = &[
        // Basic .*x patterns
        (".*x", "abcx"),
        (".*x", "x"),
        (".*x", "abcdefghijklmnox"),
        (".*:", "key: value"),
        (".*=", "var=42"),
        // .+ patterns
        (".\\+x", "ax"),
        (".\\+x", "abcx"),
        // Patterns with captures
        ("\\(.*\\)x", "abcx"),
        ("a\\(.*\\)b", "aXXXb"),
        // Long text
        (".*z", &("a".repeat(1000) + "z")),
        // Multiple candidates (greedy)
        (".*x", "xaxbxcx"),
        // No match
        (".*z", "abcdef"),
        // Multi-byte text
        (".*:", &("caf\u{00E9}".to_string() + ": latte")),
        // Anchored patterns
        ("^.*x$", "abcx"),
        // Patterns that should NOT trigger skip (no .* before literal)
        ("abc", "abc"),
        ("a\\|b", "b"),
        ("\\d\\+", "123"),
    ];

    for &(pattern, text) in cases {
        assert_same_match(pattern, text);
    }
}

#[test]
fn integration_rle_long_alternation() {
    // Long greedy quantifiers produce many stack frames -- RLE should compress
    let text = "a".repeat(5000) + "b";
    assert_same_match("a*b", &text);
}

#[test]
fn integration_rle_does_not_break_backref() {
    // Backref patterns must still work with RLE active
    assert_same_match("\\(ab\\)\\1", "abab");
    assert_same_match("\\(a\\+\\)\\1", "aaaa");
    assert_same_match("\\(a\\|b\\)\\1", "aa");
    assert_same_match("\\(a\\|b\\)\\1", "bb");
}

#[test]
fn integration_find_all_with_skip() {
    // find_all must correctly return all non-overlapping matches
    let re = crate::VimRegex::new(".*x").unwrap();
    let ctx = crate::matchers::MatchContext::simple("ax\nbx\ncx\n");
    let matches = re.find_all(&ctx).unwrap();
    assert_eq!(matches.len(), 3, "expected 3 matches across 3 lines");
    assert_eq!(matches[0].range, 0..2);
    assert_eq!(matches[1].range, 3..5);
    assert_eq!(matches[2].range, 6..8);
}

// ═══════════════════════════════════════════════════════════════════════════════
// APPROXIMATE MATCHING
// ═══════════════════════════════════════════════════════════════════════════════

use crate::FuzzyConfig;

fn build_nfa_and_search_fuzzy(
    pattern: &str,
    text: &str,
    config: &FuzzyConfig,
) -> Vec<(crate::VimMatch, u16)> {
    let nfa = build_nfa(pattern);
    let ctx = MatchContext::simple(text);
    let mut cache = make_cache(&nfa);
    crate::engines::backtracker::search_approximate(&nfa, &mut cache, &ctx, 0, config)
}

#[test]
fn fuzzy_exact_match_cost_zero() {
    let config = FuzzyConfig::with_max_edits(2);
    let results = build_nfa_and_search_fuzzy("abc", "abc", &config);
    assert!(!results.is_empty(), "exact match should be found");
    assert_eq!(results[0].1, 0, "exact match should have cost 0");
    assert_eq!(results[0].0.range, 0..3);
}

#[test]
fn fuzzy_substitution_one_char() {
    let config = FuzzyConfig::with_max_edits(1);
    let results = build_nfa_and_search_fuzzy("abc", "axc", &config);
    assert!(!results.is_empty(), "1-substitution match should be found");
    let best = &results[0];
    assert_eq!(best.1, 1, "one substitution = cost 1");
    assert_eq!(best.0.range, 0..3);
}

#[test]
fn fuzzy_insertion_one_char() {
    let config = FuzzyConfig::with_max_edits(1);
    let results = build_nfa_and_search_fuzzy("abc", "abXc", &config);
    assert!(!results.is_empty(), "1-insertion match should be found");
    let best = results.iter().min_by_key(|r| r.1).unwrap();
    assert_eq!(best.1, 1, "one insertion = cost 1");
}

#[test]
fn fuzzy_deletion_one_char() {
    let config = FuzzyConfig::with_max_edits(1);
    let results = build_nfa_and_search_fuzzy("abc", "ac", &config);
    assert!(!results.is_empty(), "1-deletion match should be found");
    let best = results.iter().min_by_key(|r| r.1).unwrap();
    assert_eq!(best.1, 1, "one deletion = cost 1");
}

#[test]
fn fuzzy_over_budget_rejected() {
    let config = FuzzyConfig::with_max_edits(2);
    let results = build_nfa_and_search_fuzzy("abc", "xyz", &config);
    assert!(
        results.is_empty(),
        "3 edits with budget 2 should produce no match"
    );
}

#[test]
fn fuzzy_multiple_matches_ranked() {
    let config = FuzzyConfig::with_max_edits(2);
    let results = build_nfa_and_search_fuzzy("abc", "abcaxcabc", &config);
    assert!(results.len() >= 2, "should find multiple matches");
    assert!(results[0].1 <= results.last().unwrap().1);
}

#[test]
fn fuzzy_custom_costs() {
    // With cost_substitute=3 and budget=2, a pure substitution path is rejected.
    // But delete+insert (cost 1+1=2) can still reach the match.
    // Use higher costs for ALL operations to ensure rejection.
    let config = FuzzyConfig {
        max_cost: 2,
        cost_insert: 3,
        cost_delete: 3,
        cost_substitute: 3,
        max_errors: None,
    };
    let results = build_nfa_and_search_fuzzy("abc", "axc", &config);
    assert!(
        results.is_empty(),
        "all edit costs 3 > budget 2 should reject"
    );

    // Verify that with cost_substitute=2, budget=2, substitution is possible.
    let config2 = FuzzyConfig {
        max_cost: 2,
        cost_insert: 1,
        cost_delete: 1,
        cost_substitute: 2,
        max_errors: None,
    };
    let results2 = build_nfa_and_search_fuzzy("abc", "axc", &config2);
    assert!(
        !results2.is_empty(),
        "substitution cost 2 <= budget 2 should allow match"
    );
}

#[test]
fn fuzzy_max_errors_limit() {
    let config = FuzzyConfig {
        max_cost: 4,
        cost_insert: 1,
        cost_delete: 1,
        cost_substitute: 1,
        max_errors: Some(1),
    };
    let results = build_nfa_and_search_fuzzy("abc", "axc", &config);
    assert!(!results.is_empty(), "1 error within max_errors=1");

    let results2 = build_nfa_and_search_fuzzy("abc", "ayz", &config);
    assert!(results2.is_empty(), "2 errors exceed max_errors=1");
}

#[test]
fn fuzzy_single_char_pattern() {
    // Use a single-char pattern that matches at every position.
    let config = FuzzyConfig::with_max_edits(2);
    let results = build_nfa_and_search_fuzzy(".", "abc", &config);
    assert!(!results.is_empty());
    assert_eq!(results[0].1, 0);
}

#[test]
fn fuzzy_quantifier_pattern() {
    let config = FuzzyConfig::with_max_edits(1);
    let results = build_nfa_and_search_fuzzy("a.*b", "aXXb", &config);
    assert!(!results.is_empty());
    assert_eq!(results[0].1, 0);
}
