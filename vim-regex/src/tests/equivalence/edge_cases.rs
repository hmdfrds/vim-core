//! Edge-case coverage for deferred lookbehind, the DFA, ReverseInner,
//! cross-feature interactions, and Unicode.
//!
//! Each test targets one specific edge case and verifies the implementation
//! handles it through the public `VimRegex` API.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

// ═══════════════════════════════════════════════════════════════════════════════
// TABULAR TESTS — simple find assertions
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(pim_lookbehind {
    // PIM: positive/negative lookbehind edge cases
    fastpath_wrong_lookbehind:      "\\(WRONG\\)\\@<=\\(bar\\)",  "xyzbar"   => ();
    positive_with_capture:          "\\(foo\\)\\@<=\\(bar\\)",    "foobar"   => (3, 6);
    multiple_lookbehinds:           "\\(x\\)\\@<=\\(y\\)\\@<=z", "xyz"      => ();
    negative_match:                 "\\(foo\\)\\@<!\\(bar\\)",    "xyzbar"   => (3, 6);
    negative_fails:                 "\\(foo\\)\\@<!\\(bar\\)",    "foobar"   => ();
    variable_length:                "\\(fo*\\)\\@<=bar",          "fooobar"  => (4, 7);
    at_position_zero:               "\\(abc\\)\\@<=def",          "def"      => ();
    lookbehind_greedy_simple:       "\\(x\\)\\@<=\\w\\+",        "xhello"   => (1, 6);
    lookbehind_greedy_with_suffix:  "\\(foo\\)\\@<=\\w*end",     "foomiddleend" => (3, 12);
});

crate::test_harness::regex_suite!(dfa_simple {
    // DFA edge cases: simple patterns
    digit_plus:           "\\d\\+",      "abc123def"    => (3, 6);
    ci_digits:            "\\c\\d\\+",   "ABC123"       => (3, 6);
    leftmost_longest:     "\\w\\+",      "hello world"  => (0, 5);
    empty_text:           "\\d\\+",      ""             => ();
    alt_leftmost_str:     "foo\\|bar",   "xbarfoo"      => (1, 4);
});

crate::test_harness::regex_suite!(reverse_inner {
    // ReverseInner edge cases
    word_at_word:         "\\w\\+@\\w\\+",       "user@host"    => (0, 9);
    suffix_falls_to_4b:  "\\w\\+world",          "helloworld"   => (0, 10);
});

crate::test_harness::regex_suite!(cross_feature {
    // Cross-feature interaction
    inner_plus_lookbehind: "\\(start\\)\\@<=_hello\\w\\+", "start_helloworld" => (5, 16);
    dfa_plus_inner:        "\\d\\+hello\\w\\+",             "42helloworld"     => (0, 12);
});

crate::test_harness::regex_suite!(unicode {
    // Unicode edge cases
    dfa_combining:        "\\d\\+",        "caf\u{00e9}123"  => (5, 8);
    multibyte_inner:      ".\u{2605}",     "x\u{2605}y"      => (0, 4);
});

crate::test_harness::regex_suite!(pim_find_all {
    // PIM lookbehind with find_all
    lookbehind_find_all:  "\\(a\\)\\@<=b", "ab xb ab"  => all[(1, 2), (7, 8)];
});

// ═══════════════════════════════════════════════════════════════════════════════
// NON-TABULAR TESTS — require captures, features(), \zs range, etc.
// ═══════════════════════════════════════════════════════════════════════════════

/// Test 8: Lookbehind + `\zs` — `\(foo\)\@<=\zsbar` on "foobar".
/// The lookbehind succeeds at position 3, `\zs` sets match start to 3,
/// then "bar" matches at 3..6. The narrowed range should be 3..6.
#[test]
fn pim_08_lookbehind_with_zs() {
    let re = VimRegex::new("\\(foo\\)\\@<=\\zsbar").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..6);
}

/// Test 9: Lookbehind + `\ze` — `\(foo\)\@<=b\zear` on "foobar".
/// Lookbehind "foo" succeeds at position 3. Then "b" matches (3..4).
/// `\ze` sets match end to position 4. Then "ar" matches (4..6).
/// The narrowed range should be 3..4 ("b" only, due to `\ze`).
#[test]
fn pim_09_lookbehind_with_ze() {
    let re = VimRegex::new("\\(foo\\)\\@<=b\\zear").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..4);
}

/// Test 12: DFA with captures — two-phase: DFA finds boundaries, Pike VM
/// extracts captures. `\(\d\+\)` on "abc123" should match with capture.
#[test]
fn dfa_12_with_captures_two_phase() {
    let re = VimRegex::new("\\(\\d\\+\\)").unwrap();
    let ctx = MatchContext::simple("abc123");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.clone()], "123");
    let cap1 = m.captures.first().unwrap().as_ref().unwrap();
    assert_eq!(&ctx.text[cap1.clone()], "123");
}

/// Test 13: DFA ineligible (lookaround) — `\(foo\)\@<=bar` should NOT
/// use DFA, but must still produce correct results via PikeVM+PIM.
#[test]
fn dfa_13_ineligible_lookaround_still_works() {
    let re = VimRegex::new("\\(foo\\)\\@<=bar").unwrap();
    assert!(re.features().has_lookaround);
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range], "bar");
}

/// Test 14: DFA ineligible (backref) — `\(\w\+\)_\1` on "abc_abc"
/// must match via the backtracker engine.
#[test]
fn dfa_14_ineligible_backref_uses_backtracker() {
    let re = VimRegex::new("\\(\\w\\+\\)_\\1").unwrap();
    assert!(re.features().has_backreferences);
    let ctx = MatchContext::simple("abc_abc");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range], "abc_abc");
}

/// Test 21: Case-insensitive inner (falls through) — `\c.foo` on "xFOO".
/// Case-insensitive mode makes inner literal extraction unreliable,
/// so this falls through to the standard PikeVM path.
#[test]
fn ri_21_case_insensitive_inner() {
    let re = VimRegex::new("\\c.foo").unwrap();
    let ctx = MatchContext::simple("xFOO");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range], "xFOO");
}

/// Test 24: Backtracker pattern — atomic group `\(a\+\)\@>b` on "aaab".
/// The atomic group `\(a\+\)\@>` greedily consumes all "a"s and does not
/// backtrack. After consuming "aaa", "b" must match at position 3.
#[test]
fn cross_24_atomic_group() {
    let re = VimRegex::new("\\(a\\+\\)\\@>b").unwrap();
    assert!(re.features().has_atomic);
    let ctx = MatchContext::simple("aaab");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range], "aaab");
}
