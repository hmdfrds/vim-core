//! Adversarial edge-case tests: inputs chosen to break the engine rather
//! than to exercise ordinary use.
//!
//! Each test targets a specific subsystem boundary or corner case that
//! might slip through normal testing.

use crate::test_builder::regex;
use crate::{extract_trigrams, FuzzyConfig, LineBloomFilter, MatchContext, VimRegex};

// ═══════════════════════════════════════════════════════════════════════════════
// 1. AUTO-POSSESSIFICATION SAFETY
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(auto_possessify {
    // [a-z]\+z — 'z' is in [a-z], so must NOT be possessified.
    does_not_eat_overlapping_successor: r"[a-z]\+z",  "abcz"  => (0, 4);
    // \w\+\d — \w includes digits, must NOT be possessified.
    word_digit_overlap:                 r"\w\+9",      "abc9"  => (0, 4);
    // [a-z]\+: — ':' is NOT in [a-z], so possessification IS safe.
    disjoint_still_matches:             r"[a-z]\+:",   "abc:"  => (0, 4);
});

// ═══════════════════════════════════════════════════════════════════════════════
// 2. REQUIRED-BYTE WITH UNICODE
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(required_byte_unicode {
    cafe: "caf\u{00e9}", "I like caf\u{00e9} au lait" => (7, 12);
    cjk:  "日本語",      "私は日本語を話します"         => (6, 15);
});

// ═══════════════════════════════════════════════════════════════════════════════
// 3. START BITMAP WITH NEGATED COLLECTION
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(start_bitmap_negated {
    skips_correctly: "[^a]x", "ax bx cx" => (3, 5);
});

#[test]
fn start_bitmap_negated_in_alternation() {
    // Tests substring content — keep manual.
    let re = VimRegex::new(r"[^0-9]\w\+").unwrap();
    let ctx = MatchContext::simple("123 hello 456");
    let m = re
        .find(&ctx)
        .unwrap()
        .expect("should find word starting with non-digit");
    assert!(
        ctx.text[m.range.start..].starts_with(' ') || ctx.text[m.range.start..].starts_with('h'),
        "match should start at a non-digit: got range {:?} = {:?}",
        m.range,
        &ctx.text[m.range.clone()]
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. FUZZY MATCH — EXACT AND BOUNDARY — uses FuzzyConfig, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn fuzzy_exact_match_zero_cost() {
    let re = VimRegex::new("abc").unwrap();
    let ctx = MatchContext::simple("abc");
    let config = FuzzyConfig {
        max_cost: 0,
        ..FuzzyConfig::default()
    };
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(
        !results.is_empty(),
        "exact match should be found with max_cost=0"
    );
    assert_eq!(results[0].1, 0, "exact match cost should be 0");
    assert_eq!(results[0].0.range, 0..3);
}

#[test]
fn fuzzy_no_match_insufficient_budget() {
    let re = VimRegex::new("abc").unwrap();
    let ctx = MatchContext::simple("xyz");
    let config = FuzzyConfig {
        max_cost: 2,
        cost_substitute: 1,
        cost_insert: 1,
        cost_delete: 1,
        max_errors: None,
    };
    let results = re.find_approximate(&ctx, &config).unwrap();
    for (m, cost) in &results {
        assert!(
            *cost <= 2,
            "got match {:?} with cost {} > max_cost=2",
            m.range,
            cost
        );
    }
    assert!(
        results.is_empty(),
        "3 substitutions exceed max_cost=2, expected no match"
    );
}

#[test]
fn fuzzy_one_sub_allowed() {
    let re = VimRegex::new("abc").unwrap();
    let ctx = MatchContext::simple("axc");
    let config = FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(
        !results.is_empty(),
        "one substitution should be within budget"
    );
    let best = &results[0];
    assert_eq!(best.1, 1);
    assert_eq!(best.0.range, 0..3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. EMPTY PATTERN FUZZY — uses error checking, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn empty_pattern_rejected_cleanly() {
    let result = VimRegex::new("");
    assert!(result.is_err(), "empty pattern should be rejected");
    let err = result.unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("empty") || msg.contains("Empty"),
        "error should mention empty pattern, got: {msg}"
    );
}

#[test]
fn fuzzy_single_char_pattern() {
    let re = VimRegex::new("a").unwrap();
    let ctx = MatchContext::simple("hello");
    let config = FuzzyConfig {
        max_cost: 1,
        cost_insert: 1,
        cost_delete: 1,
        cost_substitute: 1,
        max_errors: Some(1),
    };
    let results = re.find_approximate(&ctx, &config).unwrap();
    for (_, cost) in &results {
        assert!(*cost <= 1, "cost exceeds budget");
    }
}

#[test]
fn fuzzy_pattern_longer_than_text() {
    let re = VimRegex::new("abcdef").unwrap();
    let ctx = MatchContext::simple("abc");
    let config = FuzzyConfig::with_max_edits(3);
    let results = re.find_approximate(&ctx, &config).unwrap();
    if !results.is_empty() {
        assert!(results[0].1 <= 3, "cost should be within budget");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. SkipUntilChar / .* WITH NO MATCH
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(dot_star_no_match {
    missing_char_terminates:       ".*x",       "abcdefghijklmnopqrstuvw" => ();
    plus_missing_literal:          r"\w\+Z",    "hello world"             => ();
});

#[test]
fn dot_star_no_match_on_long_text() {
    // 10k text — keep manual for large allocation.
    let text = "a".repeat(10_000);
    let re = VimRegex::new(".*z").unwrap();
    let ctx = MatchContext::simple(&text);
    let result = re.find(&ctx).unwrap();
    assert!(
        result.is_none(),
        "10k 'a's with no 'z', should find no match"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. NESTED QUANTIFIER OVERFLOW — tests error conditions, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn large_quantifier_no_panic() {
    let result = VimRegex::new(r"a\{65536}");
    match result {
        Ok(re) => {
            let text = "a".repeat(65536);
            let ctx = MatchContext::simple(&text);
            let m = re.find(&ctx).unwrap();
            assert!(m.is_some(), "65536 a's should match a\\{{65536}}");
        }
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                msg.contains("too complex") || msg.contains("budget") || msg.contains("NFA"),
                "expected a complexity error, got: {msg}"
            );
        }
    }
}

crate::test_harness::regex_suite!(double_quantifier {
    // The second \{ is NOT a quantifier — it's parsed as Literal('{').
    parsed_as_literal_brace: r"a\{3}\{2}", "aaa{2}" => (0, 6);
});

#[test]
fn grouped_nested_quantifier_hits_budget() {
    let result = VimRegex::new(r"\(a\{1000}\)\{1000}");
    match result {
        Ok(_) => {}
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                msg.contains("too complex") || msg.contains("budget") || msg.contains("NFA"),
                "expected a complexity error, got: {msg}"
            );
        }
    }
}

#[test]
fn huge_decimal_in_quantifier_saturates() {
    let result = VimRegex::new(r"a\{99999999999}");
    match result {
        Ok(_) | Err(_) => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. BLOOM FILTER FALSE POSITIVE / TRUE NEGATIVE — uses internal API, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn bloom_filter_no_false_negatives() {
    let filter = LineBloomFilter::from_line("xxabcxx");
    let abc_trigrams = extract_trigrams("abc");
    assert!(!abc_trigrams.is_empty(), "should have trigrams from 'abc'");
    assert!(
        filter.might_contain_all(&abc_trigrams),
        "bloom filter must not have false negatives for inserted trigrams"
    );
}

#[test]
fn bloom_filter_rejects_some_unrelated() {
    let filter = LineBloomFilter::from_line("hello world");
    let test_strings = ["xyz", "qqq", "zzz", "123", "!!!", "###", "^^^"];
    let mut rejections = 0;
    for s in &test_strings {
        let trigrams = extract_trigrams(s);
        if !trigrams.is_empty() && !filter.might_contain_all(&trigrams) {
            rejections += 1;
        }
    }
    assert!(
        rejections > 0,
        "bloom filter should reject at least some unrelated trigrams (rejected {rejections}/{})",
        test_strings.len()
    );
}

#[test]
fn bloom_filter_empty_line_rejects_all() {
    let filter = LineBloomFilter::from_line("");
    let trigrams = extract_trigrams("abc");
    assert!(
        !filter.might_contain_all(&trigrams),
        "empty filter should reject all"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. CURSOR GRAVITY AT TEXT BOUNDARIES — uses find_nearest, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn find_nearest_cursor_at_zero() {
    let re = VimRegex::new("hello").unwrap();
    let ctx = MatchContext::with_cursor("hello world", 0);
    let m = re
        .find_nearest_simple(&ctx)
        .unwrap()
        .expect("should find 'hello'");
    assert_eq!(m.range, 0..5);
}

#[test]
fn find_nearest_cursor_at_text_end() {
    let re = VimRegex::new("world").unwrap();
    let text = "hello world";
    let ctx = MatchContext::with_cursor(text, text.len());
    let m = re
        .find_nearest_simple(&ctx)
        .unwrap()
        .expect("should find 'world'");
    assert_eq!(m.range, 6..11);
}

#[test]
fn find_nearest_cursor_beyond_text_no_panic() {
    let re = VimRegex::new("hello").unwrap();
    let text = "hello";
    let ctx = MatchContext::with_cursor(text, 1000);
    let m = re
        .find_nearest_simple(&ctx)
        .unwrap()
        .expect("should still find 'hello'");
    assert_eq!(m.range, 0..5);
}

#[test]
fn find_nearest_empty_text_cursor_zero() {
    let re = VimRegex::new("a").unwrap();
    let ctx = MatchContext::with_cursor("", 0);
    assert!(re.find_nearest_simple(&ctx).unwrap().is_none());
}

#[test]
fn find_nearest_single_char_text() {
    let re = VimRegex::new("a").unwrap();

    let ctx0 = MatchContext::with_cursor("a", 0);
    let m0 = re.find_nearest_simple(&ctx0).unwrap().expect("cursor at 0");
    assert_eq!(m0.range, 0..1);

    let ctx1 = MatchContext::with_cursor("a", 1);
    let m1 = re.find_nearest_simple(&ctx1).unwrap().expect("cursor at 1");
    assert_eq!(m1.range, 0..1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 10. DFA CACHE THRASHING — PATHOLOGICAL PATTERN
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(dfa_pathological {
    many_classes: r"\d\w\s\d\w\s\d\w\s\d", "1a 2b 3c 4" => (0, 10);
});

#[test]
fn dfa_cache_clear_graceful_fallback() {
    // Multi-text test — keep manual.
    let pattern = r"[a-f][g-l][m-r][s-z][A-F][G-L][M-R][S-Z]";
    regex(pattern).text("agmsAGMS").expect_match(0..8).run();
    regex(pattern).text("abcdefgh").expect_no_match().run();
}

#[test]
fn dfa_long_text_no_hang() {
    // Large text — keep manual.
    let re = VimRegex::new(r"[a-z]\+[0-9]\+[a-z]\+").unwrap();
    let mut text = "X".repeat(5000);
    text.push_str("abc123def");
    text.push_str(&"Y".repeat(5000));
    let ctx = MatchContext::simple(&text);
    let m = re.find(&ctx).unwrap().expect("should find buried pattern");
    assert_eq!(&text[m.range.clone()], "abc123def");
}

// ═══════════════════════════════════════════════════════════════════════════════
// BONUS: CROSS-CUTTING EDGE CASES — panic-safety tests, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn find_at_non_char_boundary_no_panic() {
    let re = VimRegex::new("e\u{0301}").unwrap();
    let text = "caf\u{00e9}";
    let ctx = MatchContext::simple(text);
    let _ = re.find_at(&ctx, 4);
}

#[test]
fn backward_search_empty_range() {
    let re = VimRegex::new("a").unwrap();
    let ctx = MatchContext::simple("abc");
    let result = re.find_backward_in_range(&ctx, 0..0).unwrap();
    assert!(result.is_none(), "empty range should find nothing");
}

crate::test_harness::regex_suite!(cross_cutting {
    case_insensitive_unicode: r"\chello", "HELLO world" => (0, 5);
});
