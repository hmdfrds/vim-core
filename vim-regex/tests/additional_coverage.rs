//! Additional test coverage for edge cases (Section 6.6).
//!
//! Covers: all 9 capture groups, MAX_GROUP_NESTING stress, CacheWithGuard
//! lifecycle, round-trip parse/display, error paths, DFA thrashing.

use vim_regex::{
    LineResolver, MagicMode, MatchContext, SearchConfig, SingleLineResolver, VimRegex,
    VimRegexErrorKind,
};

// ═══════════════════════════════════════════════════════════════════════════════
// ALL 9 CAPTURE GROUPS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn all_nine_captures_simultaneously() {
    // Pattern with 9 capturing groups, each capturing one character.
    let pattern = "\\(a\\)\\(b\\)\\(c\\)\\(d\\)\\(e\\)\\(f\\)\\(g\\)\\(h\\)\\(i\\)";
    let regex = VimRegex::new(pattern).unwrap();
    let ctx = MatchContext::simple("abcdefghi");
    let m = regex.find(&ctx).unwrap().unwrap();

    assert_eq!(m.range, 0..9);
    assert_eq!(m.capture_count(), 9);
    assert_eq!(m.capture(1), Some(&(0..1))); // \1 = "a"
    assert_eq!(m.capture(2), Some(&(1..2))); // \2 = "b"
    assert_eq!(m.capture(3), Some(&(2..3))); // \3 = "c"
    assert_eq!(m.capture(4), Some(&(3..4))); // \4 = "d"
    assert_eq!(m.capture(5), Some(&(4..5))); // \5 = "e"
    assert_eq!(m.capture(6), Some(&(5..6))); // \6 = "f"
    assert_eq!(m.capture(7), Some(&(6..7))); // \7 = "g"
    assert_eq!(m.capture(8), Some(&(7..8))); // \8 = "h"
    assert_eq!(m.capture(9), Some(&(8..9))); // \9 = "i"
}

#[test]
fn nine_captures_with_backrefs() {
    // Each group captures one char, then backrefs verify.
    let pattern = "\\(a\\)\\(b\\)\\(c\\)\\1\\2\\3";
    let regex = VimRegex::new(pattern).unwrap();
    let ctx = MatchContext::simple("abcabc");
    let m = regex.find(&ctx).unwrap().unwrap();

    assert_eq!(m.range, 0..6);
    assert_eq!(m.capture(1), Some(&(0..1)));
    assert_eq!(m.capture(2), Some(&(1..2)));
    assert_eq!(m.capture(3), Some(&(2..3)));
}

#[test]
fn nine_captures_partial_match() {
    // Only some groups match (groups 2-9 have optional content).
    let pattern =
        "\\(\\w\\+\\)\\(\\d*\\)\\(\\s*\\)\\(x\\?\\)\\(y\\?\\)\\(z\\?\\)\\(a\\?\\)\\(b\\?\\)\\(c\\?\\)";
    let regex = VimRegex::new(pattern).unwrap();
    let ctx = MatchContext::simple("hello");
    let m = regex.find(&ctx).unwrap().unwrap();

    assert_eq!(m.capture(1), Some(&(0..5))); // "hello"
                                             // Groups 2-9 match empty strings at position 5.
    for i in 2..=9 {
        assert!(m.capture(i).is_some(), "capture group {} should be Some", i);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAX_GROUP_NESTING (200) STRESS TEST
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn max_group_nesting_at_limit() {
    // Build a pattern with exactly 200 levels of nesting.
    // Only first 9 are capturing — the rest are non-capturing.
    let mut pattern = String::new();
    for i in 0..200 {
        if i < 9 {
            pattern.push_str("\\(");
        } else {
            pattern.push_str("\\%(");
        }
    }
    pattern.push('x');
    for _ in 0..200 {
        pattern.push_str("\\)");
    }

    let result = VimRegex::new(&pattern);
    // Should compile successfully at the limit.
    assert!(
        result.is_ok(),
        "Pattern with 200 nesting levels should compile: {:?}",
        result.err()
    );

    let regex = result.unwrap();
    let ctx = MatchContext::simple("x");
    let m = regex.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..1);
}

#[test]
fn max_group_nesting_exceeds_limit() {
    // Build a pattern with 201 levels of nesting — should error.
    let mut pattern = String::new();
    for i in 0..201 {
        if i < 9 {
            pattern.push_str("\\(");
        } else {
            pattern.push_str("\\%(");
        }
    }
    pattern.push('x');
    for _ in 0..201 {
        pattern.push_str("\\)");
    }

    let result = VimRegex::new(&pattern);
    assert!(
        result.is_err(),
        "Pattern with 201 nesting levels should fail"
    );
    match result.unwrap_err().kind {
        VimRegexErrorKind::PatternTooComplex { .. } => {}
        ref other => panic!("Expected PatternTooComplex, got: {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CACHE WITH GUARD LIFECYCLE
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn cache_with_guard_basic_lifecycle() {
    let regex = VimRegex::cached("\\w\\+").unwrap();
    let mut guard = regex.create_cache_seeded();

    let ctx = MatchContext::simple("hello world");
    let m = regex
        .find_at_with_cache(&mut guard, &ctx, 0)
        .unwrap()
        .unwrap();
    assert_eq!(m.range, 0..5);

    // Second search reuses the same cache.
    let m2 = regex
        .find_at_with_cache(&mut guard, &ctx, 6)
        .unwrap()
        .unwrap();
    assert_eq!(m2.range, 6..11);

    // Drop the guard — DFA cache should be returned to compile cache.
    drop(guard);

    // Create a new guard — should retrieve the persisted DFA cache.
    let mut guard2 = regex.create_cache_seeded();
    let m3 = regex
        .find_at_with_cache(&mut guard2, &ctx, 0)
        .unwrap()
        .unwrap();
    assert_eq!(m3.range, 0..5);
}

#[test]
fn cache_with_guard_multiple_patterns() {
    let re1 = VimRegex::cached("\\d\\+").unwrap();
    let re2 = VimRegex::cached("\\a\\+").unwrap();

    let mut g1 = re1.create_cache_seeded();
    let mut g2 = re2.create_cache_seeded();

    let ctx = MatchContext::simple("abc123");
    let m1 = re1.find_at_with_cache(&mut g1, &ctx, 0).unwrap().unwrap();
    let m2 = re2.find_at_with_cache(&mut g2, &ctx, 0).unwrap().unwrap();

    assert_eq!(m1.range, 3..6);
    assert_eq!(m2.range, 0..3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ROUND-TRIP: PARSE -> IR -> DEBUG FORMAT
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn round_trip_ir_stability() {
    // Verify that parsing the same pattern twice yields identical IR.
    let patterns = &[
        "\\<\\w\\+\\>",
        "\\(foo\\|bar\\)\\+",
        "\\d\\{2,4}",
        "^hello$",
        "[a-zA-Z_][a-zA-Z0-9_]*",
        "\\(\\w\\+\\)\\s\\+\\1",
    ];

    for pattern in patterns {
        let r1 = VimRegex::new(pattern).unwrap();
        let r2 = VimRegex::new(pattern).unwrap();
        let ir1 = format!("{:?}", r1.ir());
        let ir2 = format!("{:?}", r2.ir());
        assert_eq!(ir1, ir2, "IR instability for pattern {:?}", pattern);
    }
}

#[test]
fn round_trip_all_magic_modes_same_semantics() {
    // A simple pattern compiled in different magic modes should match
    // the same input when the pattern is adjusted for the mode.
    let input = "hello world";

    // Magic: \w\+  NoMagic: \w\+ (same syntax for \-prefixed)
    let re_magic = VimRegex::with_magic("\\w\\+", MagicMode::Magic).unwrap();
    let re_nomagic = VimRegex::with_magic("\\w\\+", MagicMode::NoMagic).unwrap();

    let ctx = MatchContext::simple(input);
    let m1 = re_magic.find(&ctx).unwrap().unwrap();
    let m2 = re_nomagic.find(&ctx).unwrap().unwrap();
    assert_eq!(m1.range, m2.range);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERROR PATHS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn error_invalid_char_code() {
    // \%d followed by non-digits should error.
    let result = VimRegex::new("\\%dXYZ");
    assert!(result.is_err());
    match result.unwrap_err().kind {
        VimRegexErrorKind::InvalidCharCode { .. } => {}
        ref other => panic!("Expected InvalidCharCode, got: {:?}", other),
    }
}

#[test]
fn error_pattern_too_complex_deep_nesting() {
    // Already tested above, but verify the error variant.
    let mut pattern = String::new();
    for _ in 0..250 {
        pattern.push_str("\\%(");
    }
    pattern.push('x');
    for _ in 0..250 {
        pattern.push_str("\\)");
    }
    let result = VimRegex::new(&pattern);
    assert!(matches!(
        result,
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::PatternTooComplex { .. })
    ));
}

#[test]
fn error_empty_pattern() {
    assert!(matches!(
        VimRegex::new(""),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::EmptyPattern)
    ));
}

#[test]
fn error_trailing_backslash() {
    assert!(matches!(
        VimRegex::new("abc\\"),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::TrailingBackslash { .. })
    ));
}

#[test]
fn error_unmatched_group_open() {
    assert!(matches!(
        VimRegex::new("\\(abc"),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::UnmatchedGroup { .. })
    ));
}

#[test]
fn error_unmatched_group_close() {
    assert!(matches!(
        VimRegex::new("abc\\)"),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::UnmatchedGroup { .. })
    ));
}

#[test]
fn error_invalid_collection() {
    assert!(matches!(
        VimRegex::new("[abc"),
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::UnterminatedCollection { .. })
    ));
}

// ═══════════════════════════════════════════════════════════════════════════════
// DFA CACHE THRASHING
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn dfa_thrashing_preserves_correctness() {
    // Use a pattern that is DFA-eligible but with many distinct Unicode chars
    // to stress the DFA state cache and potentially trigger eviction.
    let regex = VimRegex::new("\\w\\+").unwrap();

    // Create input with many distinct characters to stress state cache.
    let mut input = String::new();
    for i in 0u32..500 {
        // Mix ASCII words with varied characters to force state transitions.
        input.push_str(&format!("word{i} "));
        if i % 50 == 0 {
            // Add some unusual chars.
            input.push('\u{00E9}'); // e-acute
            input.push('\u{00F1}'); // n-tilde
            input.push(' ');
        }
    }

    let ctx = MatchContext::simple(&input);
    let matches = regex.find_all(&ctx).unwrap();

    // Verify all matches are valid words.
    assert!(!matches.is_empty());
    for m in &matches {
        let slice = &input[m.range.clone()];
        assert!(
            slice.chars().all(|c| c.is_alphanumeric() || c == '_'),
            "Non-word char in match: {:?}",
            slice
        );
    }
}

#[test]
fn dfa_correctness_under_cache_pressure() {
    // Multiple DFA-eligible patterns in sequence, each evicting the other's
    // cache from the compile cache (only 8 entries).
    let patterns: Vec<String> = (0..12).map(|i| format!("word{i}")).collect();

    let input = "word0 word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11";

    for (i, pat) in patterns.iter().enumerate() {
        let regex = VimRegex::new(pat).unwrap();
        let ctx = MatchContext::simple(input);
        let result = regex.find(&ctx).unwrap();
        assert!(result.is_some(), "Pattern {:?} should match in input", pat);
        let m = result.unwrap();
        let slice = &input[m.range.clone()];
        assert_eq!(slice, pat.as_str(), "Wrong match for pattern[{i}]");
    }
}

#[test]
fn dfa_seeded_cache_survives_thrashing() {
    // Use CacheWithGuard through multiple searches that would thrash a
    // non-persisted DFA cache.
    let regex = VimRegex::cached("\\<\\w\\{4,8}\\>").unwrap();
    let mut guard = regex.create_cache_seeded();

    let inputs = &[
        "hello world programming test",
        "alpha beta gamma delta epsilon",
        "the quick brown fox jumps over",
        "rust cargo build release target",
    ];

    for input in inputs {
        let ctx = MatchContext::simple(input);
        let matches = regex.find_all_with_cache(&mut guard, &ctx).unwrap();
        // All should find at least one 4-8 char word.
        assert!(!matches.is_empty(), "No matches for {:?}", input);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// API HARDENING — ACCESSOR METHOD VERIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn match_context_accessor_methods() {
    let ctx = MatchContext::builder("hello world")
        .cursor(5)
        .case_sensitive(false)
        .ignore_composing(true)
        .build();

    assert_eq!(ctx.text(), "hello world");
    assert_eq!(ctx.cursor(), Some(5));
    assert!(!ctx.case_sensitive());
    assert!(ctx.ignore_composing());
    assert!(ctx.visual_range().is_none());
    assert!(ctx.line_resolver().is_none());
    assert!(ctx.mark_resolver().is_none());
    assert!(ctx.last_substitute().is_none());
}

#[test]
fn match_context_builder_last_substitute() {
    let ctx = MatchContext::builder("test").last_substitute("foo").build();

    assert_eq!(ctx.last_substitute(), Some("foo"));
}

#[test]
fn vim_match_capture_accessors() {
    let regex = VimRegex::new("\\(\\w\\+\\) \\(\\w\\+\\)").unwrap();
    let ctx = MatchContext::simple("hello world");
    let m = regex.find(&ctx).unwrap().unwrap();

    // Group 0 = full match range
    assert_eq!(m.capture(0), Some(&(0..11)));
    // Group 1 = "hello"
    assert_eq!(m.capture(1), Some(&(0..5)));
    // Group 2 = "world"
    assert_eq!(m.capture(2), Some(&(6..11)));
    // Group 3 = out of bounds
    assert_eq!(m.capture(3), None);

    assert_eq!(m.capture_count(), 2);

    let caps: Vec<_> = m.captures_iter().collect();
    assert_eq!(caps.len(), 2);
    assert_eq!(caps[0], Some(&(0..5)));
    assert_eq!(caps[1], Some(&(6..11)));
}

#[test]
fn vim_match_capture_out_of_bounds() {
    let regex = VimRegex::new("hello").unwrap();
    let ctx = MatchContext::simple("hello");
    let m = regex.find(&ctx).unwrap().unwrap();

    assert_eq!(m.capture_count(), 0);
    assert_eq!(m.capture(1), None);
    assert_eq!(m.capture(10), None);
    assert_eq!(m.captures_iter().count(), 0);
}

#[test]
fn is_cache_compatible_check() {
    let regex = VimRegex::new("\\w\\+").unwrap();
    let cache = regex.create_cache();
    assert!(regex.is_cache_compatible(&cache));

    let no_cap_cache = regex.create_cache_no_captures();
    assert!(regex.is_cache_compatible(&no_cap_cache));
}

#[test]
fn find_backward_in_range_with_cache_renamed() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::with_cursor("aaa bbb ccc", 11);
    let m = re
        .find_backward_in_range_with_cache(&mut cache, &ctx, 4..7)
        .unwrap()
        .unwrap();
    assert_eq!(m.range, 4..7);
}

#[test]
fn apply_replacement_re_exported_at_crate_root() {
    // Verify apply_replacement is accessible via crate root
    let regex = VimRegex::new("hello").unwrap();
    let ctx = MatchContext::simple("hello world");
    let m = regex.find(&ctx).unwrap().unwrap();
    let result = vim_regex::apply_replacement("hello world", &m, "goodbye", None).unwrap();
    assert_eq!(result, "goodbye");
}

// ═══════════════════════════════════════════════════════════════════════════════
// GAP 2: ^*ptr INTEGRATION TEST
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn caret_star_ptr_matches_correctly() {
    // In Vim: "^*ptr" means ^ (start of line) followed by literal * then "ptr".
    // The * after ^ is NOT a quantifier — it's a literal because * can't
    // quantify ^. So the pattern matches "*ptr" only at the start of a line.
    let re = VimRegex::new("^*ptr").unwrap();

    // "  *ptr here" does NOT start with "*ptr" (has leading spaces)
    let ctx = MatchContext::simple("  *ptr here");
    let m = re.find(&ctx).unwrap();
    assert!(
        m.is_none(),
        "^*ptr should not match '  *ptr here' (not at BOL)"
    );

    // "*ptr here" DOES start with "*ptr"
    let ctx2 = MatchContext::simple("*ptr here");
    let m2 = re.find(&ctx2).unwrap();
    assert!(m2.is_some(), "^*ptr should match '*ptr here' at BOL");
    let matched = m2.unwrap();
    assert_eq!(matched.range, 0..4, "^*ptr should match '*ptr' (4 bytes)");

    // Verify on a multi-line input: "*ptr" on line 2
    let ctx3 = MatchContext::simple("hello\n*ptr world");
    let m3 = re.find(&ctx3).unwrap();
    assert!(m3.is_some(), "^*ptr should match at start of line 2");
    assert_eq!(m3.unwrap().range, 6..10);

    // No match if "*ptr" is mid-line
    let ctx4 = MatchContext::simple("hello\n  *ptr world");
    let m4 = re.find(&ctx4).unwrap();
    assert!(
        m4.is_none(),
        "^*ptr should not match '*ptr' in the middle of a line"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// GAP 3: STRATEGY SUBTRACTIVE CONFIG TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn search_config_dfa_disabled_same_results() {
    // Compile with DFA disabled — should still produce identical results
    // via Pike VM fallback.
    let config = SearchConfig {
        dfa_enabled: false,
        ..SearchConfig::default()
    };
    let re_full = VimRegex::new("\\w\\+").unwrap();
    let re_no_dfa = VimRegex::with_config("\\w\\+", &config).unwrap();

    let text = "hello world 123 foo_bar";
    let ctx = MatchContext::simple(text);

    let full_result = re_full.find(&ctx).unwrap();
    let no_dfa_result = re_no_dfa.find(&ctx).unwrap();
    assert_eq!(
        full_result, no_dfa_result,
        "DFA disabled should match default"
    );

    let full_all = re_full.find_all(&ctx).unwrap();
    let no_dfa_all = re_no_dfa.find_all(&ctx).unwrap();
    assert_eq!(
        full_all, no_dfa_all,
        "find_all should match with DFA disabled"
    );
}

#[test]
fn search_config_literal_bypass_disabled_same_results() {
    // Compile with literal bypass disabled — should still match correctly
    // via engine dispatch.
    let config = SearchConfig {
        literal_bypass_enabled: false,
        ..SearchConfig::default()
    };
    let re_full = VimRegex::new("hello").unwrap();
    let re_no_lit = VimRegex::with_config("hello", &config).unwrap();

    let text = "say hello world";
    let ctx = MatchContext::simple(text);

    let full_result = re_full.find(&ctx).unwrap();
    let no_lit_result = re_no_lit.find(&ctx).unwrap();
    assert_eq!(
        full_result, no_lit_result,
        "literal bypass disabled should match default"
    );
}

#[test]
fn search_config_ac_disabled_same_results() {
    // Compile with AC disabled — literal alternation still works via engine dispatch.
    let config = SearchConfig {
        ac_enabled: false,
        ..SearchConfig::default()
    };
    let re_full = VimRegex::new("foo\\|bar\\|baz").unwrap();
    let re_no_ac = VimRegex::with_config("foo\\|bar\\|baz", &config).unwrap();

    let text = "the baz is here and foo is there";
    let ctx = MatchContext::simple(text);

    let full_result = re_full.find(&ctx).unwrap();
    let no_ac_result = re_no_ac.find(&ctx).unwrap();
    assert_eq!(
        full_result, no_ac_result,
        "AC disabled should match default"
    );

    let full_all = re_full.find_all(&ctx).unwrap();
    let no_ac_all = re_no_ac.find_all(&ctx).unwrap();
    assert_eq!(
        full_all, no_ac_all,
        "find_all should match with AC disabled"
    );
}

#[test]
fn search_config_reverse_disabled_same_results() {
    // Compile with reverse strategies disabled.
    let config = SearchConfig {
        reverse_enabled: false,
        ..SearchConfig::default()
    };
    let re_full = VimRegex::new("\\w\\+ing").unwrap();
    let re_no_rev = VimRegex::with_config("\\w\\+ing", &config).unwrap();

    let text = "running jumping swimming resting";
    let ctx = MatchContext::simple(text);

    let full_result = re_full.find(&ctx).unwrap();
    let no_rev_result = re_no_rev.find(&ctx).unwrap();
    assert_eq!(
        full_result, no_rev_result,
        "reverse disabled should match default"
    );

    let full_all = re_full.find_all(&ctx).unwrap();
    let no_rev_all = re_no_rev.find_all(&ctx).unwrap();
    assert_eq!(
        full_all, no_rev_all,
        "find_all should match with reverse disabled"
    );
}

#[test]
fn search_config_all_disabled_same_results() {
    // All acceleration disabled — pure engine dispatch only.
    let config = SearchConfig::baseline();

    // Test several pattern types
    let cases: &[(&str, &str)] = &[
        ("\\w\\+", "hello world 123"),
        ("hello", "say hello world"),
        ("foo\\|bar\\|baz", "the baz is here"),
        ("\\d\\{2,4}", "a 12 b 3456 c"),
        ("\\<\\w\\+\\>", "one two three"),
    ];

    for &(pattern, text) in cases {
        let re_full = VimRegex::new(pattern).unwrap();
        let re_base = VimRegex::with_config(pattern, &config).unwrap();
        let ctx = MatchContext::simple(text);

        let full = re_full.find_all(&ctx).unwrap();
        let base = re_base.find_all(&ctx).unwrap();
        assert_eq!(
            full, base,
            "baseline config mismatch for pattern={pattern:?} text={text:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GAP 4: RESOLVER END-TO-END THROUGH VimRegex::find()
// ═══════════════════════════════════════════════════════════════════════════════

/// A test resolver that reports a fixed line number for any offset.
struct TestLineResolver {
    line: u32,
}

impl LineResolver for TestLineResolver {
    fn byte_to_line(&self, _offset: usize) -> u32 {
        self.line
    }
    fn byte_to_col(&self, offset: usize) -> u32 {
        #[allow(clippy::cast_possible_truncation)]
        {
            (offset + 1) as u32
        }
    }
    fn byte_to_vcol(&self, offset: usize) -> u32 {
        #[allow(clippy::cast_possible_truncation)]
        {
            (offset + 1) as u32
        }
    }
    fn cursor_line(&self) -> u32 {
        self.line
    }
    fn line_byte_range(&self, line: u32) -> Option<(usize, usize)> {
        if line == self.line {
            Some((0, 100))
        } else {
            None
        }
    }
}

#[test]
fn percent_line_with_resolver_matches_on_correct_line() {
    // \%3l\w\+ — match one or more word characters, but ONLY on line 3.
    let re = VimRegex::new("\\%3l\\w\\+").unwrap();

    // On line 3: should match
    let resolver3 = TestLineResolver { line: 3 };
    let ctx3 = MatchContext::builder("hello world")
        .line_resolver(&resolver3)
        .build();
    let result3 = re.find(&ctx3).unwrap();
    assert!(
        result3.is_some(),
        "\\%3l\\w\\+ should match when resolver reports line 3"
    );
    assert_eq!(result3.unwrap().range, 0..5, "should match 'hello'");

    // On line 5: should NOT match
    let resolver5 = TestLineResolver { line: 5 };
    let ctx5 = MatchContext::builder("hello world")
        .line_resolver(&resolver5)
        .build();
    let result5 = re.find(&ctx5).unwrap();
    assert!(
        result5.is_none(),
        "\\%3l\\w\\+ should NOT match when resolver reports line 5"
    );
}

#[test]
fn percent_line_with_single_line_resolver() {
    // Verify the public SingleLineResolver works end-to-end.
    let re = VimRegex::new("\\%3l\\w\\+").unwrap();

    // SingleLineResolver reporting line 3
    let resolver = SingleLineResolver::new(3, 11);
    let ctx = MatchContext::builder("hello world")
        .line_resolver(&resolver)
        .build();
    let result = re.find(&ctx).unwrap();
    assert!(
        result.is_some(),
        "SingleLineResolver(3) should enable \\%3l match"
    );
    assert_eq!(result.unwrap().range, 0..5);

    // SingleLineResolver reporting line 7 — no match
    let resolver7 = SingleLineResolver::new(7, 11);
    let ctx7 = MatchContext::builder("hello world")
        .line_resolver(&resolver7)
        .build();
    let result7 = re.find(&ctx7).unwrap();
    assert!(
        result7.is_none(),
        "SingleLineResolver(7) should prevent \\%3l match"
    );
}

#[test]
fn percent_column_with_resolver() {
    // \%5c\w\+ — match word chars starting at byte column 5 (1-indexed).
    let re = VimRegex::new("\\%5c\\w\\+").unwrap();

    let resolver = TestLineResolver { line: 1 };
    let ctx = MatchContext::builder("    hello world")
        .line_resolver(&resolver)
        .build();
    let result = re.find(&ctx).unwrap();
    // Column 5 means byte offset 4 (1-indexed → 0-indexed). At offset 4 we have 'h'.
    assert!(result.is_some(), "\\%5c should match at byte column 5");
    assert_eq!(
        result.unwrap().range,
        4..9,
        "should match 'hello' starting at offset 4"
    );
}

#[test]
fn percent_line_before_with_resolver() {
    // \%<3l\w\+ — match word chars on lines BEFORE line 3 (i.e., lines 1, 2).
    let re = VimRegex::new("\\%<3l\\w\\+").unwrap();

    // On line 2: should match (2 < 3)
    let resolver2 = TestLineResolver { line: 2 };
    let ctx2 = MatchContext::builder("test")
        .line_resolver(&resolver2)
        .build();
    let result2 = re.find(&ctx2).unwrap();
    assert!(
        result2.is_some(),
        "\\%<3l should match on line 2 (before line 3)"
    );

    // On line 3: should NOT match (3 is not < 3)
    let resolver3 = TestLineResolver { line: 3 };
    let ctx3 = MatchContext::builder("test")
        .line_resolver(&resolver3)
        .build();
    let result3 = re.find(&ctx3).unwrap();
    assert!(result3.is_none(), "\\%<3l should NOT match on line 3");

    // On line 4: should NOT match (4 is not < 3)
    let resolver4 = TestLineResolver { line: 4 };
    let ctx4 = MatchContext::builder("test")
        .line_resolver(&resolver4)
        .build();
    let result4 = re.find(&ctx4).unwrap();
    assert!(result4.is_none(), "\\%<3l should NOT match on line 4");
}
