//! Tests for the upgraded Pike VM with `SparseSet`, `SlotTable`, and `Cache`.
//!
//! Verifies that the Pike VM produces correct results for a comprehensive
//! set of patterns covering literals, quantifiers, character classes,
//! anchors, lookaround, \zs/\ze, and backward search.

use crate::cache::Cache;
use crate::engines::pike_vm;
use crate::hir;
use crate::matchers::MatchContext;
use crate::nfa::builder::NfaBuilder;
use crate::parser::parse_pattern;
use crate::test_builder::regex;

use super::total_slots;

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS (used by non-converted tests)
// ═══════════════════════════════════════════════════════════════════════════════

/// Build NFA from pattern, returning also num_cap and total slot count.
fn build_nfa(pattern: &str) -> (crate::nfa::Nfa, usize, usize) {
    let parsed = parse_pattern(pattern).expect("valid pattern");
    let (lowered, _) = hir::lower(&parsed.node);
    let nfa = NfaBuilder::build(&lowered).unwrap();
    let num_cap = nfa.slot_count();
    let total = total_slots(num_cap);
    (nfa, num_cap, total)
}

/// Run the Pike VM forward from `start` and return the match.
fn assert_forward(pattern: &str, text: &str, start: usize) -> Option<crate::VimMatch> {
    let (nfa, _, total) = build_nfa(pattern);
    let ctx = MatchContext::simple(text);
    let mut cache = Cache::new(nfa.state_count() as u32, total, false);
    pike_vm::search(&nfa, &mut cache, &ctx, start, false, None, None)
}

/// Run the Pike VM anchored at `pos` and return the match.
fn assert_anchored(pattern: &str, text: &str, pos: usize) -> Option<crate::VimMatch> {
    let (nfa, _, total) = build_nfa(pattern);
    let ctx = MatchContext::simple(text);
    let mut cache = Cache::new(nfa.state_count() as u32, total, false);
    pike_vm::match_anchored(&nfa, &mut cache, &ctx, pos)
}

/// Run the Pike VM backward and return the match.
fn assert_backward(pattern: &str, text: &str, cursor: usize) -> Option<crate::VimMatch> {
    let (nfa, _, total) = build_nfa(pattern);
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
    let mut cache = Cache::new(nfa.state_count() as u32, total, false);
    pike_vm::search_backward(&nfa, &mut cache, &ctx, None)
}

/// Run the Pike VM find_all and return all matches.
fn assert_find_all(pattern: &str, text: &str) -> Vec<crate::VimMatch> {
    let (nfa, _, total) = build_nfa(pattern);
    let ctx = MatchContext::simple(text);
    let mut cache = Cache::new(nfa.state_count() as u32, total, false);
    pike_vm::search_all(&nfa, &mut cache, &ctx, None)
}

/// Case-insensitive forward search.
fn assert_forward_ci(pattern: &str, text: &str, start: usize) -> Option<crate::VimMatch> {
    let (nfa, _, total) = build_nfa(pattern);
    let ctx = MatchContext {
        text,
        cursor: None,
        visual_range: None,
        case_sensitive: false,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let mut cache = Cache::new(nfa.state_count() as u32, total, false);
    pike_vm::search(&nfa, &mut cache, &ctx, start, false, None, None)
}

// ═══════════════════════════════════════════════════════════════════════════════
// SIMPLE FORWARD MATCH TESTS — regex_suite!
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(literals {
    literal_from_offset:    "def",      "abcdef"    => (3, 6);
});

crate::test_harness::regex_suite!(dot_and_classes {
    dot_match:         "a.c",          "axc"         => (0, 3);
    digit_class:       r"\d\+",        "abc123def"   => (3, 6);
    word_class:        r"\w\+",        "hello world" => (0, 5);
    collection_range:  "[a-z]\\+",     "Hello"       => (1, 5);
});

crate::test_harness::regex_suite!(quantifiers {
    star_greedy:          "a*",           "aaa"     => (0, 3);
    plus_match:           "a\\+",         "aaa"     => (0, 3);
    plus_no_match:        "a\\+",         ""        => ();
    question_mark:        "a\\?",         "a"       => (0, 1);
    bounded_greedy:       "a\\{2,4}",     "aaaa"    => (0, 4);
    bounded_lazy:         "a\\{-2,4}",    "aaaa"    => (0, 2);
});

crate::test_harness::regex_suite!(alternation {
    first:   "a\\|b",     "a"      => (0, 1);
    second:  "a\\|b",     "b"      => (0, 1);
    longer:  "foo\\|bar",  "foobar" => (0, 3);
});

crate::test_harness::regex_suite!(capture_groups {
    two_groups:    "\\(a\\)\\(b\\)",     "ab" => (0, 2);
    nested_groups: "\\(\\(a\\)b\\)",     "ab" => (0, 2);
});

crate::test_harness::regex_suite!(anchors {
    caret_anchor:   "^abc",  "abc\ndef" => (0, 3);
    caret_no_match: "^abc",  "xabc"     => ();
    dollar_anchor:  "abc$",  "abc"      => (0, 3);
});

crate::test_harness::regex_suite!(lookahead {
    positive:       "foo\\(bar\\)\\@=",  "foobar" => (0, 3);
    negative:       "foo\\(bar\\)\\@!",  "foobar" => ();
    negative_pass:  "foo\\(bar\\)\\@!",  "foobaz" => (0, 3);
});

crate::test_harness::regex_suite!(lookbehind {
    positive:       "\\(foo\\)\\@<=bar",  "foobar" => (3, 6);
    negative:       "\\(foo\\)\\@<!bar",  "foobar" => ();
    negative_pass:  "\\(foo\\)\\@<!bar",  "xxxbar" => (3, 6);
});

crate::test_harness::regex_suite!(zs_ze {
    ze_match:  "foo\\zebar",  "foobar" => (0, 3);
});

crate::test_harness::regex_suite!(multiline {
    any_char_newline: "\\_.", "a\nb" => (0, 1);
});

crate::test_harness::regex_suite!(utf8_safety {
    utf8_literal: "café",  "le café est bon" => (3, 8);
    utf8_dot:     "c.f",   "café"            => (0, 3);
});

crate::test_harness::regex_suite!(find_all_suite {
    find_all_literal:  "ab",  "ababab" => all[(0, 2), (2, 4), (4, 6)];
});

// ═══════════════════════════════════════════════════════════════════════════════
// CAPTURE GROUP — regex() builder (needs capture assertion)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn single_group() {
    regex("\\(ab\\)")
        .text("ab")
        .expect_match(0..2)
        .expect_capture(1, 0..2)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// \zs — kept as-is (invariant 6 incompatible with \zs range shifting)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn zs_match() {
    let m = assert_forward("foo\\zsbar", "foobar", 0).unwrap();
    assert_eq!(m.range, 3..6);
}

#[test]
fn zs_ze_combined() {
    let m = assert_forward("foo\\zsbar\\zebaz", "foobarbaz", 0).unwrap();
    assert_eq!(m.range, 3..6);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-LENGTH MATCHES — kept as-is (invariant 5 incompatible with empty text)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn star_empty() {
    let m = assert_forward("a*", "", 0);
    assert!(m.is_some()); // matches empty
}

#[test]
fn question_mark_empty() {
    let m = assert_forward("a\\?", "", 0);
    assert!(m.is_some()); // matches empty
}

// ═══════════════════════════════════════════════════════════════════════════════
// CASE INSENSITIVE — kept as-is (context-level case sensitivity)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn case_insensitive_literal() {
    let m = assert_forward_ci("foo", "FOO", 0);
    assert_eq!(m.unwrap().range, 0..3);
}

#[test]
fn case_insensitive_no_match() {
    assert!(assert_forward_ci("foo", "bar", 0).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// FIND ALL EDGE CASES — kept as-is (count-only assertions)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn find_all_caret() {
    let matches = assert_find_all("^", "abc\ndef");
    assert!(!matches.is_empty());
}

#[test]
fn find_all_zero_length() {
    let matches = assert_find_all("a*", "bc");
    // Each position yields a zero-length match for "a*".
    assert_eq!(matches.len(), 3);
}

#[test]
fn find_all_star_in_middle() {
    let matches = assert_find_all("a*", "bab");
    assert!(matches.len() >= 3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKWARD SEARCH — regex() builder (needs cursor + backward expectation)
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
fn backward_first_only() {
    regex("abc")
        .text("abcXXX")
        .cursor(6)
        .expect_backward_match(0..3)
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

#[test]
fn backward_cursor_at_match() {
    regex("abc")
        .text("abcdef")
        .cursor(3)
        .expect_backward_match(0..3)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKWARD SEARCH EDGE CASES — kept as-is (cursor=0 violates invariant 5)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn backward_cursor_at_zero() {
    let m = assert_backward("abc", "abcdef", 0);
    assert!(m.is_none(), "no match should exist before cursor=0");
}

#[test]
fn backward_returns_last_match_before_cursor() {
    // Multiple matches — backward should return the one closest to cursor
    let m = assert_backward("ab", "ab cd ab cd ab", 10);
    // Matches at 0, 6, 12. Last before cursor=10 is at position 6.
    assert!(m.is_some());
    assert_eq!(m.unwrap().range.start, 6);
}

// ═══════════════════════════════════════════════════════════════════════════════
// UNBOUNDED LOOKBEHIND — kept as-is (dynamic text, is_some assertion)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn unbounded_lookbehind() {
    let padding = "x".repeat(300);
    let text = format!("MARKER{padding}END");
    let m = assert_forward("\\(MARKER.*\\)\\@<=END", &text, 0);
    assert!(m.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════════
// ANCHORED — kept as-is (uses pike_vm::match_anchored directly)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchored_match() {
    let m = assert_anchored("foo", "foobar", 0);
    assert_eq!(m.unwrap().range, 0..3);
}

#[test]
fn anchored_no_match_at_pos() {
    assert!(assert_anchored("foo", "barfoo", 0).is_none());
}

#[test]
fn anchored_match_at_offset() {
    let m = assert_anchored("foo", "barfoo", 3);
    assert_eq!(m.unwrap().range, 3..6);
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMPREHENSIVE BATCH — kept as-is
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn comprehensive() {
    let cases: Vec<(&str, &str, usize, bool)> = vec![
        ("a", "a", 0, true),
        ("a", "b", 0, false),
        ("a", "ba", 0, true),
        ("a*b", "aaab", 0, true),
        ("a\\+b", "aaab", 0, true),
        ("a\\?b", "ab", 0, true),
        ("a\\?b", "b", 0, true),
        ("\\<word\\>", "a word here", 0, true),
        ("\\<word\\>", "wording", 0, false),
        ("\\(ab\\)*c", "abababc", 0, true),
        ("a.*b", "aXXXb", 0, true),
        ("a.*b", "a", 0, false),
    ];

    for (pat, text, start, should_match) in cases {
        let m = assert_forward(pat, text, start);
        assert_eq!(
            m.is_some(),
            should_match,
            "pattern={pat:?} text={text:?} start={start}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// REVERSE PIKE VM TESTS — kept as-is (use build_reverse, custom reverse search)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn reverse_match_simple_literal() {
    use crate::hir;
    use crate::nfa::builder::NfaBuilder;

    let pattern = "abc";
    let parsed = crate::parser::parse_with_magic(pattern, crate::MagicMode::Magic).unwrap();
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let ctx = MatchContext::simple("xxabcyy");
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let result = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, 5, 0);
    assert_eq!(
        result,
        Some(2),
        "reverse match of 'abc' in 'xxabcyy' from pos=5"
    );
}

#[test]
fn reverse_match_no_match() {
    use crate::hir;
    use crate::nfa::builder::NfaBuilder;

    let parsed = crate::parser::parse_with_magic("xyz", crate::MagicMode::Magic).unwrap();
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let ctx = MatchContext::simple("abcdef");
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let result = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, 6, 0);
    assert_eq!(result, None);
}

#[test]
fn reverse_match_respects_lower_bound() {
    use crate::hir;
    use crate::nfa::builder::NfaBuilder;

    let parsed = crate::parser::parse_with_magic("abc", crate::MagicMode::Magic).unwrap();
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let ctx = MatchContext::simple("xxabcyy");
    // Lower bound 3 means the reverse scan can't reach position 2
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let result = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, 5, 3);
    assert_eq!(result, None);
}

#[test]
fn reverse_match_zero_width_pattern() {
    use crate::hir;
    use crate::nfa::builder::NfaBuilder;

    // a* can match zero chars (zero-width match at any position)
    let parsed = crate::parser::parse_with_magic("a*", crate::MagicMode::Magic).unwrap();
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let ctx = MatchContext::simple("bbb");
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let result = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, 2, 0);
    // a* reversed is still a*, accepts immediately (zero 'a's consumed)
    assert_eq!(result, Some(2));
}

#[test]
fn reverse_match_single_char() {
    use crate::hir;
    use crate::nfa::builder::NfaBuilder;

    let parsed = crate::parser::parse_with_magic("a", crate::MagicMode::Magic).unwrap();
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let ctx = MatchContext::simple("a");
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let result = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, 1, 0);
    assert_eq!(result, Some(0));
}

#[test]
fn reverse_match_utf8_multibyte() {
    use crate::hir;
    use crate::nfa::builder::NfaBuilder;

    let parsed = crate::parser::parse_with_magic("caf\u{00E9}", crate::MagicMode::Magic).unwrap();
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let text = "xxcaf\u{00E9}yy";
    let cafe_end = "xxcaf\u{00E9}".len(); // 7 bytes
    let ctx = MatchContext::simple(text);
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let result = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, cafe_end, 0);
    assert_eq!(result, Some(2));
}

// ═══════════════════════════════════════════════════════════════════════════════
// DIFFERENTIAL: REVERSE + FORWARD CONFIRMATION vs PURE FORWARD
// ═══════════════════════════════════════════════════════════════════════════════

/// Differential test: reverse NFA + forward confirmation produces the
/// same match as pure forward search.
#[test]
fn reverse_matches_forward_for_suffix_pattern() {
    // Pattern with suffix literal "bar" — triggers ReverseSuffix
    let pattern = "\\d\\+bar";
    let text = "xx99barxx";

    // Forward-only result
    let fwd = assert_forward(pattern, text, 0);

    // Reverse + forward confirmation
    let parsed = parse_pattern(pattern).expect("valid");
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let ctx = MatchContext::simple(text);

    let suffix = "bar";
    let suffix_pos = text.find(suffix).unwrap();
    let suffix_end = suffix_pos + suffix.len();

    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let rev_start = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, suffix_end, 0);
    let rev_confirmed = rev_start.and_then(|start| {
        let (nfa, _, total) = build_nfa(pattern);
        let mut cache = Cache::new(nfa.state_count() as u32, total, false);
        super::match_anchored(&nfa, &mut cache, &ctx, start)
    });

    assert_eq!(
        fwd, rev_confirmed,
        "reverse+confirm must match forward for pattern={pattern:?} text={text:?}"
    );
}

#[test]
fn reverse_matches_forward_for_anchored_eof() {
    let pattern = "foo\\%$";
    let text = "xxfoo";
    let fwd = assert_forward(pattern, text, 0);

    let parsed = parse_pattern(pattern).expect("valid");
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let ctx = MatchContext::simple(text);

    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let rev_start = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, text.len(), 0);
    let rev_confirmed = rev_start.and_then(|start| {
        let (nfa, _, total) = build_nfa(pattern);
        let mut cache = Cache::new(nfa.state_count() as u32, total, false);
        super::match_anchored(&nfa, &mut cache, &ctx, start)
    });

    assert_eq!(fwd, rev_confirmed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// SUFFIX PRESENT BUT FULL PATTERN FAILS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn reverse_suffix_present_but_pattern_fails() {
    // Pattern: \d\+bar — requires digits before "bar"
    // Text: "xyzbar" — "bar" present but no digits before it
    let pattern = "\\d\\+bar";
    let text = "xyzbar";

    let parsed = parse_pattern(pattern).expect("valid");
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let ctx = MatchContext::simple(text);

    // Reverse NFA finds "bar" suffix at end, scans backward
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let _rev_start = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, 6, 0);

    // Even if reverse NFA finds a candidate, forward confirmation should fail
    // because there are no digits before "bar"
    let fwd = assert_forward(pattern, text, 0);
    assert!(
        fwd.is_none(),
        "pattern should not match text without digits"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// CJK AND EMOJI REVERSE PIKE VM — kept as-is (reverse NFA tests)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn reverse_match_cjk() {
    let pattern = "\u{65E5}\u{672C}"; // 日本 (two 3-byte chars)
    let parsed = crate::parser::parse_with_magic(pattern, crate::MagicMode::Magic).unwrap();
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let text = "xx\u{65E5}\u{672C}yy"; // xx日本yy
    let match_end = "xx\u{65E5}\u{672C}".len(); // 8 bytes
    let ctx = MatchContext::simple(text);
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let result = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, match_end, 0);
    assert_eq!(
        result,
        Some(2),
        "reverse match of 日本 should start at byte 2"
    );
}

#[test]
fn reverse_match_emoji() {
    let pattern = "a\u{1F600}"; // a😀 (1 + 4 bytes)
    let parsed = crate::parser::parse_with_magic(pattern, crate::MagicMode::Magic).unwrap();
    let (lowered, _) = hir::lower(&parsed.node);
    let rev_nfa = NfaBuilder::build_reverse(&lowered).unwrap();
    let text = "xxa\u{1F600}yy";
    let match_end = "xxa\u{1F600}".len(); // 7 bytes
    let ctx = MatchContext::simple(text);
    let mut cache = crate::cache::Cache::new_no_captures(rev_nfa.state_count() as u32);
    let result = super::try_match_at_reverse(&rev_nfa, &mut cache, &ctx, match_end, 0);
    assert_eq!(result, Some(2));
}

// ═══════════════════════════════════════════════════════════════════════════════
// ENSURE_CACHE — DFA CACHE PRESERVATION (B2) — kept as-is
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ensure_cache_preserves_dfa_cache() {
    // Build a small NFA so we can create a DfaCache from it.
    let (nfa, _num_cap, total) = build_nfa("abc");

    // Start with a small cache (capacity 2) that will need rebuilding.
    let mut cache = Cache::new(2, total, false);
    assert!(cache.dfa.is_none());

    // Inject a DfaCache.
    let dfa = crate::engines::lazy_dfa::DfaCache::new(&nfa, true, false);
    cache.dfa = Some(dfa);
    assert!(cache.dfa.is_some());

    // ensure_cache must rebuild (nfa.state_count() > 2) but preserve dfa_cache.
    let state_count = nfa.state_count();
    assert!(
        state_count > 2,
        "NFA must exceed initial capacity to trigger rebuild"
    );
    super::ensure_cache(&mut cache, state_count, total);

    assert!(
        cache.dfa.is_some(),
        "DfaCache must survive ensure_cache rebuild"
    );
    assert!(
        cache.nfa.curr.capacity() >= state_count,
        "cache must be resized to fit NFA"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SUB-CACHE POOLING — LOOKAROUND REUSE (A3) — kept as-is
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn sub_cache_pooled_after_lookaround() {
    // A lookbehind pattern: after matching, the parent cache should have
    // a pooled sub_cache ready for reuse.
    let (nfa, _, total) = build_nfa("\\(foo\\)\\@<=bar");
    let ctx = MatchContext::simple("foobar");
    let mut cache = Cache::new(nfa.state_count() as u32, total, false);

    assert!(
        cache.lookaround.sub_cache.is_none(),
        "sub_cache starts as None"
    );

    let m = pike_vm::search(&nfa, &mut cache, &ctx, 0, false, None, None);
    assert_eq!(m.unwrap().range, 3..6);

    assert!(
        cache.lookaround.sub_cache.is_some(),
        "sub_cache must be pooled after lookaround check"
    );
}

#[test]
fn sub_cache_reused_across_multiple_matches() {
    // Run two searches with the same cache. The second search should
    // reuse the sub_cache pooled by the first, producing correct results.
    let (nfa, _, total) = build_nfa("\\(foo\\)\\@<=\\w\\+");
    let text = "foobar foobaz";
    let ctx = MatchContext::simple(text);
    let mut cache = Cache::new(nfa.state_count() as u32, total, false);

    let m1 = pike_vm::search(&nfa, &mut cache, &ctx, 0, false, None, None);
    assert_eq!(m1.unwrap().range, 3..6);
    assert!(
        cache.lookaround.sub_cache.is_some(),
        "sub_cache pooled after first search"
    );

    // Second search reuses the pooled sub_cache.
    let m2 = pike_vm::search(&nfa, &mut cache, &ctx, 7, false, None, None);
    assert_eq!(m2.unwrap().range, 10..13);
    assert!(
        cache.lookaround.sub_cache.is_some(),
        "sub_cache still pooled after second search"
    );
}

#[test]
fn sub_cache_lookahead_correctness() {
    // Verify lookahead also pools and reuses correctly.
    let (nfa, _, total) = build_nfa("foo\\(bar\\)\\@=");
    let ctx = MatchContext::simple("foobar");
    let mut cache = Cache::new(nfa.state_count() as u32, total, false);

    let m = pike_vm::search(&nfa, &mut cache, &ctx, 0, false, None, None);
    assert_eq!(m.unwrap().range, 0..3);
    assert!(
        cache.lookaround.sub_cache.is_some(),
        "sub_cache pooled after lookahead"
    );
}

#[test]
fn ensure_cache_preserves_sub_cache() {
    // Verify that ensure_cache preserves the pooled sub_cache across rebuild.
    let (nfa, _num_cap, total) = build_nfa("abc");

    let mut cache = Cache::new(2, total, false);
    // Inject a sub_cache.
    cache.lookaround.sub_cache = Some(Box::new(Cache::new(4, 0, false)));
    assert!(cache.lookaround.sub_cache.is_some());

    let state_count = nfa.state_count();
    assert!(state_count > 2, "must trigger rebuild");
    super::ensure_cache(&mut cache, state_count, total);

    assert!(
        cache.lookaround.sub_cache.is_some(),
        "sub_cache must survive ensure_cache rebuild"
    );
}
