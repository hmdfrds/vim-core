//! Adversarial patterns: inputs chosen to break the engine rather than to
//! exercise ordinary use.
//!
//! Each test targets a specific attack vector designed to break the
//! DFA / two-phase / AC / CI / find_all implementation.
//!
//! Results key:
//!   - Tests that pass: the implementation survived the attack.
//!   - Tests marked with PRE_EXISTING: behavior diverges from Vim but is
//!     a known parser-level issue, not a regression in these strategies.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS (only retained for non-suite tests)
// ═══════════════════════════════════════════════════════════════════════════════

fn find_range(pattern: &str, text: &str) -> Option<std::ops::Range<usize>> {
    let re = VimRegex::new(pattern).expect("valid pattern");
    let ctx = MatchContext::simple(text);
    re.find(&ctx).expect("no engine error").map(|m| m.range)
}

fn find_str<'t>(pattern: &str, text: &'t str) -> Option<&'t str> {
    find_range(pattern, text).map(|r| &text[r])
}

fn find_all_ranges(pattern: &str, text: &str) -> Vec<std::ops::Range<usize>> {
    let re = VimRegex::new(pattern).expect("valid pattern");
    let ctx = MatchContext::simple(text);
    re.find_all(&ctx)
        .expect("no error")
        .into_iter()
        .map(|m| m.range)
        .collect()
}

fn is_dfa_eligible(pattern: &str) -> bool {
    let re = VimRegex::new(pattern).expect("valid pattern");
    re.is_dfa_eligible()
}

// ═══════════════════════════════════════════════════════════════════════════════
// TABULAR TESTS — simple find / find_all assertions
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(alternation_priority {
    // Attack 1: Alternation priority
    nested_prefix_overlap:   "\\(a\\|ab\\)\\(c\\|cd\\)",  "abcd"  => (0, 3);
    four_branch_ac:          "a\\|ab\\|abc\\|abcd",        "abcd"  => (0, 1);
    four_branch_find_all:    "a\\|ab\\|abc\\|abcd",        "abcd"  => all[(0, 1)];
    ci_prefix_overlap:       "\\ca\\|\\cab",               "AB"    => (0, 1);
});

crate::test_harness::regex_suite!(ci_dfa {
    // Attack 2: CI DFA
    ci_collection:           "\\c[a-z]",           "A"   => (0, 1);
    ci_mixed_seq:            "\\c[a-z][A-Z]",      "Ab"  => (0, 2);
    force_case_sensitive:    "\\C[a-z]",           "A"   => ();
    ci_greek_not_folded:     "\\c[a-z]",           "\u{03B1}" => ();
});

crate::test_harness::regex_suite!(start_of_line {
    // Attack 3: StartOfLine
    bare_caret:              "^",                  "abc"       => (0, 0);
    caret_in_alternation:    "^\\|foo",            "bar\nfoo"  => (0, 0);
    double_caret:            "^^",                 "^hello"    => (0, 1);
    caret_find_all:          "^",                  "ab\ncd\nef" => all[(0, 0), (3, 3), (6, 6)];
});

crate::test_harness::regex_suite!(find_all_attacks {
    // Attack 4: find_all edge cases
    zero_width_star:         "a*",       "bab"    => all[(0, 0), (1, 2), (2, 2), (3, 3)];
    ci_greedy:               "\\ca\\+",  "AaAa"   => all[(0, 4)];
    empty_text_no_match:     "\\d\\+",   ""        => all[];
});

crate::test_harness::regex_suite!(is_match_attacks {
    // Attack 5 (subset) & Attack 9: is_match edge cases
    // Invariant 1 ensures is_match agrees with find
    empty_text_no:           "\\d\\+",   ""        => ();
});

crate::test_harness::regex_suite!(two_phase {
    // Attack 8: Two-phase correctness
    alternation_priority:    "a\\|ab",   "ab"      => (0, 1);
});

crate::test_harness::regex_suite!(edge_cases {
    // Attack 9: Edge cases
    single_newline:          "^",          "\n"              => all[(0, 0), (1, 1)];
    unicode_boundary:        "\\w\\+",     "caf\u{00E9}"     => (0, 5);
    trailing_newline:        "^\\w\\+",    "hello\nworld\n"  => all[(0, 5), (6, 11)];
});

crate::test_harness::regex_suite!(ac_matcher {
    // Attack 10: AC matcher
    leftmost_first:          "abc\\|abcd\\|xy\\|xyz",                "abcd xyz"     => (0, 3);
    ci_match:                "\\cfoo\\|\\cbar\\|\\cbaz\\|\\cqux",    "FOOBAR"       => (0, 3);
    no_false_positive:       "abcd\\|efgh\\|ijkl\\|mnop",            "abc efg ijk mno" => ();
    find_all_multiple:       "foo\\|bar\\|baz\\|qux",                "foo bar baz qux" => all[(0, 3), (4, 7), (8, 11), (12, 15)];
});

crate::test_harness::regex_suite!(dfa_pike_consistency {
    // Attack 11: DFA + Pike VM consistency
    greedy_star:             ".*",     "abc"  => (0, 3);
    greedy_plus_no_match:    "a\\+",   "bbb"  => ();
    greedy_star_zero_width:  "a*",     "bbb"  => (0, 0);
});

crate::test_harness::regex_suite!(regression_vectors {
    // Attack 12: Regression vectors
    bounded_exact:           "a\\{3}",       "aaaa"  => (0, 3);
    bounded_range:           "a\\{2,4}",     "aaa"   => (0, 3);
    optional_zero_width:     "a\\?",          "b"     => (0, 0);
    dot_no_nl:               ".",             "\n"    => ();
    dot_nl:                  "\\_.",          "\n"    => (0, 1);
    lazy_quantifier:         "a\\{-1,}b",    "aaab"  => (0, 4);
});

crate::test_harness::regex_suite!(ci_collection_dfa {
    // Attack 13: CI collection DFA correctness
    hex_range:               "\\c[a-f]",      "F"      => (0, 1);
    negated_ci:              "\\c[^a-z]",     "A"      => ();
    negated_ci_digit:        "\\c[^a-z]",     "1"      => (0, 1);
    uppercase_range_lower:   "\\c[A-Z]\\+",  "hELLo"  => (0, 5);
});

crate::test_harness::regex_suite!(sol_dfa {
    // Attack 14: Start-of-line DFA
    mid_line_no_match:       "^foo",   "bar foo"   => ();
    second_line:             "^foo",   "bar\nfoo"   => (4, 7);
});

// ═══════════════════════════════════════════════════════════════════════════════
// NON-TABULAR TESTS — require is_dfa_eligible, cache API, captures, etc.
// ═══════════════════════════════════════════════════════════════════════════════

// ── is_match on empty text with zero-width anchor ───────────────────────

/// is_match on empty text with ^ — true (zero-width SOL at pos 0).
/// Kept non-tabular because backward search on empty text legitimately
/// returns None, which would trip the builder's invariant 5.
#[test]
fn attack9c_empty_text_zero_width() {
    let re = VimRegex::new("^").unwrap();
    let ctx = MatchContext::simple("");
    assert!(re.is_match(&ctx).expect("no engine error"));
}

// ── CI cross-contamination ──────────────────────────────────────────────

/// Two separate regex objects: \c vs \C. Verify no cross-contamination.
#[test]
fn attack2d_ci_then_cs_separate_objects() {
    let re_ci = VimRegex::new("\\c[a-z]").unwrap();
    let ctx_a = MatchContext::simple("A");
    assert!(re_ci.find(&ctx_a).unwrap().is_some(), "CI should match 'A'");

    let re_cs = VimRegex::new("\\C[a-z]").unwrap();
    assert!(
        re_cs.find(&ctx_a).unwrap().is_none(),
        "CS should NOT match 'A'"
    );
}

// ── DFA eligibility ─────────────────────────────────────────────────────

/// ^$ — DFA-eligible: both ^ and $ are handled by the DFA.
#[test]
fn attack3c_caret_dollar_dfa_eligible() {
    assert!(is_dfa_eligible("^$"));
    let result = find_range("^$", "hello\n\nworld");
    assert_eq!(result, Some(6..6));
}

/// ^ is DFA-eligible.
#[test]
fn attack3e_caret_dfa_eligible() {
    assert!(is_dfa_eligible("^"));
}

// ── Large text find_all ─────────────────────────────────────────────────

/// 10000-char text with \w\+ — no truncation.
#[test]
fn attack4c_large_text_word_find_all() {
    let text = "word ".repeat(2000);
    let ranges = find_all_ranges("\\w\\+", &text);
    assert_eq!(ranges.len(), 2000);
    assert_eq!(ranges[0], 0..4);
    assert_eq!(ranges[1999], 9995..9999);
}

/// Empty pattern is rejected by the parser (EmptyPattern error), before any
/// strategy is chosen.
#[test]
fn attack4d_empty_pattern_rejected() {
    let result = VimRegex::new("");
    assert!(
        result.is_err(),
        "empty pattern should be rejected by parser"
    );
}

// ── is_match consistency ────────────────────────────────────────────────

/// For DFA-eligible patterns, is_match must agree with find().is_some().
#[test]
fn attack5a_is_match_dfa_vs_pike_consistency() {
    let patterns = &[
        ("\\w\\+", "hello", true),
        ("\\d\\+", "hello", false),
        ("a\\|ab", "ab", true),
        ("^\\w\\+", "hello", true),
        ("[a-z]\\+", "123", false),
        ("a*", "bbb", true), // zero-width match at pos 0
        ("a\\+", "bbb", false),
    ];
    for &(pattern, text, expected) in patterns {
        let re = VimRegex::new(pattern).unwrap();
        let ctx = MatchContext::simple(text);
        let is_m = re.is_match(&ctx).expect("no engine error");
        let find_m = re.find(&ctx).unwrap().is_some();
        assert_eq!(
            is_m, find_m,
            "is_match and find disagree for {:?} on {:?}: is_match={}, find={}",
            pattern, text, is_m, find_m
        );
        assert_eq!(
            is_m, expected,
            "unexpected result for {:?} on {:?}: got {}, expected {}",
            pattern, text, is_m, expected
        );
    }
}

/// \zs is a match override — is_match should still return true.
#[test]
fn attack5b_is_match_with_zs() {
    let re = VimRegex::new("foo\\zsbar").unwrap();
    let ctx = MatchContext::simple("foobar");
    assert!(re.is_match(&ctx).expect("no engine error"));
}

/// Backreference pattern — not DFA-eligible, falls through to backtracker.
#[test]
fn attack5c_is_match_ineligible_pattern() {
    let re = VimRegex::new("\\(a\\)\\1").unwrap();
    let ctx = MatchContext::simple("aa");
    assert!(re.is_match(&ctx).expect("no engine error"));
}

// ── Compilation cache ───────────────────────────────────────────────────

/// 100 different patterns through the LRU (capacity 8). No panic or leak.
#[test]
fn attack6a_lru_eviction() {
    for i in 0..100 {
        let pattern = format!("pattern{}", i);
        let _ = VimRegex::cached(&pattern);
    }
    let re = VimRegex::cached("pattern0").unwrap();
    let ctx = MatchContext::simple("pattern0");
    assert!(re.find(&ctx).unwrap().is_some());
}

/// \chello vs hello — different compiled patterns, different cache entries.
#[test]
fn attack6b_ci_vs_cs_separate_cache() {
    let re_ci = VimRegex::cached("\\chello").unwrap();
    let re_cs = VimRegex::cached("hello").unwrap();
    let ctx = MatchContext::simple("HELLO");
    assert!(re_ci.find(&ctx).unwrap().is_some(), "CI should match");
    assert!(re_cs.find(&ctx).unwrap().is_none(), "CS should not match");
}

/// Same pattern returns the same Rc (cache hit).
#[test]
fn attack6c_same_pattern_cached_reused() {
    let rc1 = VimRegex::cached("hello").unwrap();
    let rc2 = VimRegex::cached("hello").unwrap();
    assert!(std::rc::Rc::ptr_eq(&rc1, &rc2));
}

// ── Performance / DFA internals ─────────────────────────────────────────

/// Large NFA: many bounded quantifiers. Tests epsilon_seen buffer resizing.
#[test]
fn attack7a_large_nfa_epsilon_buffer() {
    let pattern = "[a-z]\\{10,}[0-9]\\{10,}[a-z]\\{10,}";
    let text = "abcdefghijklmnop1234567890abcdefghijklmnop";
    assert!(find_str(pattern, text).is_some());
}

/// 20 literal branches — AC with LeftmostFirst.
/// The text "prefix word15 suffix" contains "word15" at offset 7.
/// AC finds "word1" (branch index 1) at offset 7 because "word1" is a
/// prefix of "word15" AND is listed before "word15" in the alternation.
/// This is correct leftmost-first semantics.
#[test]
fn attack7b_alternation_many_branches_ac_priority() {
    let branches: Vec<String> = (0..20).map(|i| format!("word{}", i)).collect();
    let pattern = branches.join("\\|");
    let text = "prefix word15 suffix";
    let result = find_str(&pattern, text);
    // AC LeftmostFirst: "word1" (branch 1) is a prefix of "word15" in the text.
    // Since "word1" appears before "word15" in the pattern, "word1" wins.
    assert_eq!(result, Some("word1"));
}

/// Verify that when the text EXACTLY contains a later branch with no prefix
/// match, the correct branch is found.
#[test]
fn attack7b2_alternation_exact_branch_match() {
    let branches: Vec<String> = (0..20).map(|i| format!("word{}", i)).collect();
    let pattern = branches.join("\\|");
    let text = "prefix word5 suffix";
    let result = find_str(&pattern, text);
    assert_eq!(result, Some("word5"));
}

/// Repeated DFA searches — cache reuse stability.
#[test]
fn attack7c_dfa_cache_reuse_stability() {
    let pattern = "[a-z]\\+[0-9]\\+[A-Z]\\+";
    let text = "abc123XYZ";
    for _ in 0..100 {
        assert_eq!(find_str(pattern, text), Some("abc123XYZ"));
    }
}

// ── Two-phase match override ────────────────────────────────────────────

/// foo\zsbar — match override. DFA finds "foobar", Pike VM narrows to "bar".
/// Kept non-tabular because \zs narrows the range, and find_at(1) legitimately
/// can't find "foo" starting at offset 1, which trips invariant 6.
#[test]
fn attack8c_two_phase_match_override() {
    let re = VimRegex::new("foo\\zsbar").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.clone()], "bar");
}

// ── Two-phase captures ──────────────────────────────────────────────────

/// \(a\)\|b on "b" — alternation + captures. Pike VM: branch 2 matches.
/// Capture group 1 = None.
#[test]
fn attack8b_two_phase_captures_with_alternation() {
    let re = VimRegex::new("\\(a\\)\\|b").unwrap();
    let ctx = MatchContext::simple("b");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..1);
    assert!(
        m.captures.is_empty() || m.captures[0].is_none(),
        "capture group 1 should be None when branch 2 matches"
    );
}

// ── find_at edge cases ──────────────────────────────────────────────────

/// Start search from byte offset past a multibyte char — no panic.
#[test]
fn attack9f_find_from_past_multibyte() {
    let text = "caf\u{00E9}x"; // e is 2 bytes (3..5), x at 5
    let re = VimRegex::new("x").unwrap();
    let ctx = MatchContext::simple(text);
    let m = re.find_at(&ctx, 5).unwrap();
    assert!(m.is_some());
    assert_eq!(m.unwrap().range, 5..6);
}

// ── find_all consistency ────────────────────────────────────────────────

/// For DFA-eligible patterns, find_all[0] must equal find().
#[test]
fn attack11c_find_all_consistency_with_find() {
    let cases: &[(&str, &str)] = &[
        ("\\w\\+", "  hello  world"),
        ("\\d\\+", "abc123def456"),
        ("a*", "bab"),
        ("^\\w\\+", "hello\nworld"),
        ("a\\+", "aabaa"),
        ("[a-z]\\+[0-9]\\+", "abc123 def456"),
    ];
    for &(pattern, text) in cases {
        let first = find_range(pattern, text);
        let all = find_all_ranges(pattern, text);
        if let Some(first_range) = first {
            assert!(
                !all.is_empty(),
                "find has match but find_all empty for {:?} on {:?}",
                pattern,
                text
            );
            assert_eq!(
                first_range, all[0],
                "find and find_all[0] disagree for {:?} on {:?}",
                pattern, text
            );
        }
    }
}

// ── Regression: find_all ────────────────────────────────────────────────

/// ^foo\|bar find_all on multiline text.
#[test]
fn attack12a_caret_with_alternation_find_all() {
    let text = "foo\nbar\nbaz";
    let ranges = find_all_ranges("^foo\\|bar", text);
    assert!(ranges.len() >= 2);
    assert_eq!(&text[ranges[0].clone()], "foo");
    assert_eq!(&text[ranges[1].clone()], "bar");
}

/// Back-to-back find_all calls on the same regex — no stale state.
#[test]
fn attack12g_find_all_no_stale_state() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let ctx1 = MatchContext::simple("hello world");
    let r1: Vec<_> = re
        .find_all(&ctx1)
        .unwrap()
        .into_iter()
        .map(|m| m.range)
        .collect();
    assert_eq!(r1, vec![0..5, 6..11]);

    let ctx2 = MatchContext::simple("foo");
    let r2: Vec<_> = re
        .find_all(&ctx2)
        .unwrap()
        .into_iter()
        .map(|m| m.range)
        .collect();
    assert_eq!(r2, vec![0..3]);
}

// ── SOL find_at ─────────────────────────────────────────────────────────

/// ^ with find_from at position after newline.
#[test]
fn attack14c_sol_find_from_after_newline() {
    let re = VimRegex::new("^\\w\\+").unwrap();
    let ctx = MatchContext::simple("hello\nworld\nfoo");
    let m = re.find_at(&ctx, 6).unwrap();
    assert!(m.is_some());
    assert_eq!(m.unwrap().range, 6..11);
}

/// ^ with find_from at mid-line position — no match.
#[test]
fn attack14d_sol_find_from_mid_line() {
    let re = VimRegex::new("^\\w\\+").unwrap();
    let ctx = MatchContext::simple("hello world");
    let m = re.find_at(&ctx, 3).unwrap();
    assert!(m.is_none());
}
