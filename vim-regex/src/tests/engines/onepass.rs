//! Integration tests for the one-pass DFA engine.
//!
//! Tests verify:
//! - Eligibility pre-check (which patterns qualify)
//! - Build correctness (one-pass vs not-one-pass detection)
//! - Search correctness (captures, match ranges)
//! - Equivalence with Pike VM (results must match)

use crate::engine::SearchConfig;
use crate::matchers::MatchContext;
use crate::VimRegex;

// ═══════════════════════════════════════════════════════════════════════════════
// ELIGIBILITY TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn eligible_simple_literal() {
    let re = VimRegex::new("hello").unwrap();
    assert!(re.is_onepass_eligible());
}

#[test]
fn eligible_with_captures() {
    let re = VimRegex::new(r"\(\w\+\):\(\d\+\)").unwrap();
    assert!(re.is_onepass_eligible());
}

#[test]
fn eligible_digit_pattern() {
    let re = VimRegex::new(r"\d\+").unwrap();
    assert!(re.is_onepass_eligible());
}

#[test]
fn eligible_date_pattern() {
    let re = VimRegex::new(r"\(\d\{4}\)-\(\d\{2}\)-\(\d\{2}\)").unwrap();
    assert!(re.is_onepass_eligible());
}

#[test]
fn ineligible_backref() {
    let re = VimRegex::new(r"\(a\)\1").unwrap();
    assert!(!re.is_onepass_eligible());
}

#[test]
fn ineligible_lookaround() {
    let re = VimRegex::new(r"foo\@<=bar").unwrap();
    assert!(!re.is_onepass_eligible());
}

#[test]
fn ineligible_atomic() {
    let re = VimRegex::new(r"foo\@>bar").unwrap();
    assert!(!re.is_onepass_eligible());
}

#[test]
fn ineligible_last_substitute() {
    let re = VimRegex::new("~").unwrap();
    assert!(!re.is_onepass_eligible());
}

// ═══════════════════════════════════════════════════════════════════════════════
// BUILD TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn build_succeeds_for_simple_literal() {
    let re = VimRegex::new("abc").unwrap();
    let mut cache = re.create_cache();
    let state = cache.onepass_state(&re.nfa);
    assert!(
        state.is_available(),
        "simple literal 'abc' should build one-pass DFA"
    );
}

#[test]
fn build_succeeds_for_digit_plus() {
    let re = VimRegex::new(r"\d\+").unwrap();
    let mut cache = re.create_cache();
    let state = cache.onepass_state(&re.nfa);
    // \d\+ is one-pass: each digit has only one valid continuation.
    assert!(state.is_available(), r"\d\+ should build one-pass DFA");
}

#[test]
fn build_fails_for_ambiguous_star_followed_by_same() {
    // a*a is not one-pass: at any 'a', ambiguous whether to stay in a* or advance.
    let re = VimRegex::new(r"a*a").unwrap();
    let mut cache = re.create_cache();
    let state = cache.onepass_state(&re.nfa);
    assert!(!state.is_available(), "a*a should NOT be one-pass");
}

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH CORRECTNESS TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn search_simple_literal() {
    let re = VimRegex::new("hello").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("hello world");
    let m = re.find_with_cache(&mut cache, &ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..5);
}

#[test]
fn search_digit_plus() {
    let re = VimRegex::new(r"\d\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("abc 123 def");
    let m = re.find_with_cache(&mut cache, &ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.clone()], "123");
}

#[test]
fn search_with_captures() {
    let re = VimRegex::new(r"\(\d\+\)-\(\d\+\)").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("123-456");
    let m = re.find_with_cache(&mut cache, &ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..7);
    assert_eq!(m.capture(1), Some(&(0..3)));
    assert_eq!(m.capture(2), Some(&(4..7)));
}

#[test]
fn search_non_overlapping_alternation() {
    let re = VimRegex::new(r"\(abc\|def\)").unwrap();
    let mut cache = re.create_cache();

    let ctx1 = MatchContext::simple("abc");
    let m1 = re.find_with_cache(&mut cache, &ctx1).unwrap().unwrap();
    assert_eq!(&ctx1.text[m1.range.clone()], "abc");

    let ctx2 = MatchContext::simple("def");
    let m2 = re.find_with_cache(&mut cache, &ctx2).unwrap().unwrap();
    assert_eq!(&ctx2.text[m2.range.clone()], "def");
}

// ═══════════════════════════════════════════════════════════════════════════════
// EQUIVALENCE TESTS — One-Pass DFA vs Pike VM
// ═══════════════════════════════════════════════════════════════════════════════

/// For each pattern, compare the normal search result (which may use one-pass DFA)
/// vs a baseline Pike VM result (all acceleration disabled).
fn assert_equivalence(pattern: &str, text: &str) {
    let re = VimRegex::new(pattern).unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple(text);

    // Normal search (may use one-pass DFA via HybridDfa capture resolver).
    let normal = re.find_with_cache(&mut cache, &ctx).unwrap();

    // Baseline: all acceleration disabled (Pike VM only).
    let re_baseline = VimRegex::with_config(pattern, &SearchConfig::baseline()).unwrap();
    let mut baseline_cache = re_baseline.create_cache();
    let baseline = re_baseline
        .find_with_cache(&mut baseline_cache, &ctx)
        .unwrap();

    assert_eq!(normal, baseline, "divergence for /{pattern}/ on {text:?}");
}

#[test]
fn equivalence_simple_literal() {
    assert_equivalence("hello", "hello world");
    assert_equivalence("hello", "no match here");
}

#[test]
fn equivalence_digit_plus() {
    assert_equivalence(r"\d\+", "abc 123 def");
    assert_equivalence(r"\d\+", "no digits");
}

#[test]
fn equivalence_captures() {
    assert_equivalence(r"\(\d\+\)-\(\d\+\)", "123-456");
    assert_equivalence(r"\(\d\+\)-\(\d\+\)", "abc-def");
}

#[test]
fn equivalence_word_plus() {
    assert_equivalence(r"\w\+", "hello_world 123");
    assert_equivalence(r"\w\+", "   ");
}

#[test]
fn equivalence_alternation() {
    assert_equivalence(r"\(abc\|def\)", "abcdef");
    assert_equivalence(r"\(abc\|def\)", "defabc");
    assert_equivalence(r"\(abc\|def\)", "xyz");
}

#[test]
fn equivalence_date_pattern() {
    assert_equivalence(r"\(\d\{4}\)-\(\d\{2}\)-\(\d\{2}\)", "2024-01-15");
    assert_equivalence(r"\(\d\{4}\)-\(\d\{2}\)-\(\d\{2}\)", "no date");
}

#[test]
fn equivalence_mixed_literal_and_class() {
    assert_equivalence(r"foo\d\+bar", "foo123bar");
    assert_equivalence(r"foo\d\+bar", "foobar");
}

#[test]
fn equivalence_nested_captures() {
    assert_equivalence(r"\(\(\d\+\)\.\(\d\+\)\)", "3.14");
    assert_equivalence(r"\(\(\d\+\)\.\(\d\+\)\)", "abc");
}
