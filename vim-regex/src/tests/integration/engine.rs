//! Tests for the public VimRegex API.

use crate::engine::VimRegex;
use crate::ir::{VimPatternNode, VimRegexErrorKind};
use crate::matchers::MatchContext;
use crate::test_builder::regex;
use crate::MagicMode;

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC API — simple find / no-match via regex_suite!
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(basic_api {
    new_and_find:             "abc",     "xabcy" => (1, 4);
    is_match_true:            "abc",     "xabcy" => equiv;
    is_match_false:           "abc",     "xyz"   => ();
});

#[test]
fn is_match_no_captures_mode() {
    // Tests multiple patterns; keep as manual test.
    let re = VimRegex::new("\\(foo\\)\\(bar\\)").unwrap();
    assert!(re.is_match(&MatchContext::simple("foobar")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("baz")).unwrap());
    // Also test with backreference pattern (uses backtracker)
    let re = VimRegex::new("\\(a\\)\\1").unwrap();
    assert!(re.is_match(&MatchContext::simple("aa")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("ab")).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════════
// ENGINE SELECTION — uses features(), keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn backref_uses_backtracker() {
    let re = VimRegex::new("\\(ab\\)\\1").unwrap();
    assert!(re.features().has_backreferences);
    regex("\\(ab\\)\\1").text("abab").expect_match(0..4).run();
}

#[test]
fn no_backref_uses_pike_vm() {
    let re = VimRegex::new("\\d\\+").unwrap();
    assert!(!re.features().has_backreferences);
}

// ═══════════════════════════════════════════════════════════════════════════════
// FIND ALL
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(find_all_suite {
    digits:                 r"\d\+",   "a12b34c5"  => all[(1, 3), (4, 6), (7, 8)];
});

#[test]
fn find_all_empty_pattern_advances() {
    let re = VimRegex::new("a*").unwrap();
    let ctx = MatchContext::simple("bc");
    let matches = re.find_all(&ctx).unwrap();
    // Each position yields a zero-length match for "a*".
    assert_eq!(matches.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAGIC MODES
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn very_magic_mode() {
    regex("(a|b)+")
        .magic(MagicMode::VeryMagic)
        .text("xxabba")
        .expect_match(2..6)
        .run();
}

#[test]
fn verymagic_backslash_plus_is_literal() {
    regex(r"\v(foo)\+")
        .magic(MagicMode::VeryMagic)
        .text("foo+")
        .expect_match(0..4)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// BRACE QUANTIFIER EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(brace_quantifier_suite {
    min_greater_than_max_swapped: r"a\{5,2}", "aaa"  => equiv;
    backslash_close:              r"a\{2,5\}", "aaa" => equiv;
});

// ═══════════════════════════════════════════════════════════════════════════════
// CASE SENSITIVITY
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(case_sensitivity_suite {
    case_insensitive_modifier: r"\cabc", "ABC" => (0, 3);
    case_sensitive_modifier:   r"\Cabc", "ABC" => ();
});

#[test]
fn case_default_follows_context() {
    // Uses raw MatchContext with case_sensitive: false; keep manual.
    let re = VimRegex::new("abc").unwrap();
    let ctx = MatchContext {
        text: "ABC",
        cursor: None,
        visual_range: None,
        case_sensitive: false,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERROR CASES — uses error kind matching, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn empty_pattern_error() {
    let result = VimRegex::new("");
    assert!(matches!(
        result,
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::EmptyPattern)
    ));
}

#[test]
fn trailing_backslash_error() {
    let result = VimRegex::new("abc\\");
    assert!(matches!(
        result,
        Err(ref e) if matches!(e.kind, VimRegexErrorKind::TrailingBackslash { .. })
    ));
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCESSORS — uses features()/ir(), keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ir_accessor() {
    let re = VimRegex::new("abc").unwrap();
    let _ = format!("{:?}", re.ir());
}

#[test]
fn features_accessor() {
    let re = VimRegex::new("\\(a\\)\\1").unwrap();
    assert!(re.features().has_backreferences);
    assert_eq!(re.features().capture_count, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKWARD SEARCH VIA PUBLIC API
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn find_backward_public_api() {
    regex("abc")
        .text("abcXXXabcXX")
        .cursor(10)
        .expect_backward_match(6..9)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// ATOMIC GROUPS VIA PUBLIC API
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn atomic_group_via_public_api() {
    let re = VimRegex::new("\\(foo\\)\\@>bar").unwrap();
    assert!(re.features().has_atomic);
    regex("\\(foo\\)\\@>bar")
        .text("foobar")
        .expect_match(0..6)
        .run();
}

crate::test_harness::regex_suite!(atomic_group_suite {
    no_match: r"\(foo\)\@>baz", "foobar" => ();
});

// ═══════════════════════════════════════════════════════════════════════════════
// [\n] EXPLICIT NEWLINE MATCHING VIA PUBLIC API
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(explicit_newline_suite {
    in_collection:            "[\\n]",  "a\nb" => (1, 2);
    with_other_chars:         "[\\na]", "b\na" => (1, 2);
});

// ═══════════════════════════════════════════════════════════════════════════════
// CASE-INSENSITIVE COLLECTIONS VIA PUBLIC API
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(case_insensitive_collections_suite {
    range:              r"\c[A-Z]", "abc" => (0, 1);
    single:             r"\c[a]",   "ABC" => (0, 1);
    no_fold:            "[A-Z]",    "abc" => ();
});

#[test]
fn collection_escaped_range_start() {
    // Tests multiple texts on same pattern; keep manual.
    let re = VimRegex::new(r"[\t-z]").unwrap();
    let ctx_tab = MatchContext::simple("\t");
    assert!(
        re.find(&ctx_tab).unwrap().is_some(),
        r"[\t-z] should match tab"
    );
    let ctx_a = MatchContext::simple("a");
    assert!(
        re.find(&ctx_a).unwrap().is_some(),
        r"[\t-z] should match 'a'"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// CASE-INSENSITIVE PREFILTER
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(ci_prefilter_suite {
    finds_uppercase:   r"\cfoo", "hello FOO world" => (6, 9);
    finds_mixed_case:  r"\cfoo", "hello FoO world" => (6, 9);
});

#[test]
fn case_insensitive_backward_search() {
    regex("\\cfoo")
        .text("FOO and foo")
        .cursor(11)
        .expect_backward_match(8..11)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// PURE LITERAL BYPASS
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(pure_literal_suite {
    bypass_match:      "hello", "say hello world" => (4, 9);
    bypass_no_match:   "hello", "no match here"   => ();
});

// ═══════════════════════════════════════════════════════════════════════════════
// ANCHORED START-OF-FILE FAST PATH
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(anchored_start_suite {
    matches_at_start:           r"\%^hello", "hello world" => equiv;
    no_match_not_at_start:      r"\%^hello", "say hello"   => ();
    start_of_line_all_lines:    "^world",    "hello\nworld" => (6, 11);
});

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH HARDENING IN find_all
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn find_all_zero_width_no_infinite_loop() {
    // Checks specific count constraint; keep manual.
    let re = VimRegex::new("x*").unwrap();
    let ctx = MatchContext::simple("ab");
    let matches = re.find_all(&ctx).unwrap();
    assert!(matches.len() <= 4);
    assert!(!matches.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMPILATION CACHE (8-ENTRY LRU) — uses cache API, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn compile_cache_hit() {
    let rc1 = VimRegex::cached("hello").unwrap();
    let rc2 = VimRegex::cached("hello").unwrap();
    assert!(std::rc::Rc::ptr_eq(&rc1, &rc2));
}

#[test]
fn compile_cache_different_magic() {
    let rc_magic = VimRegex::cached_with_magic("(a|b)+", MagicMode::Magic).unwrap();
    let rc_very = VimRegex::cached_with_magic("(a|b)+", MagicMode::VeryMagic).unwrap();
    assert!(!std::rc::Rc::ptr_eq(&rc_magic, &rc_very));
}

#[test]
fn compile_cache_basic_search() {
    let re = VimRegex::cached("\\d\\+").unwrap();
    let ctx = MatchContext::simple("abc123def");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..6);
}

#[test]
fn compile_cache_eviction() {
    let patterns: Vec<String> = (0..10).map(|i| format!("pat{}", i)).collect();
    for pat in &patterns {
        let _ = VimRegex::cached(pat).unwrap();
    }
    let rc_a = VimRegex::cached("pat9").unwrap();
    let rc_b = VimRegex::cached("pat9").unwrap();
    assert!(std::rc::Rc::ptr_eq(&rc_a, &rc_b));
}

// ═══════════════════════════════════════════════════════════════════════════════
// LAZY VISITED SET (N5) — uses cache API, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn pike_vm_search_leaves_backtracker_none() {
    let re = VimRegex::new("\\d\\+").unwrap();
    assert!(!re.features().has_backreferences);
    let mut cache = re.create_cache();
    assert!(
        cache.backtracker.is_none(),
        "backtracker should be None before search"
    );
    let ctx = MatchContext::simple("abc123def");
    let _ = re.find_with_cache(&mut cache, &ctx).unwrap();
    assert!(
        cache.backtracker.is_none(),
        "Pike VM search should not allocate BacktrackerScratch"
    );
}

#[test]
fn backtracker_search_allocates_backtracker_scratch() {
    let re = VimRegex::new("\\(ab\\)\\1").unwrap();
    assert!(re.features().has_backreferences);
    let mut cache = re.create_cache();
    assert!(
        cache.backtracker.is_none(),
        "backtracker should be None before search"
    );
    let ctx = MatchContext::simple("abab");
    let m = re.find_with_cache(&mut cache, &ctx).unwrap();
    assert!(m.is_some(), "should find a match");
    assert!(
        cache.backtracker.is_some(),
        "backtracker search should allocate BacktrackerScratch"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// BRANCH-AND (\&) INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(branch_and_suite {
    basic_matching:                r"\w\+\&foo",       "hello foo bar"  => (6, 9);
    both_must_match_no_match:      r"\d\+\&foo",       "foo 123"        => ();
    last_branch_determines_extent: r"....\&fo",        "foobar"         => (0, 2);
    no_match_first_branch_fails:   r"xyz\&foo",        "foo xyz"        => ();
    three_branches_all_match:      r"...\&\w\+\&foo",  "hello foo bar"  => (6, 9);
    at_different_positions:        r"\w\+\&bar",        "123 bar baz"    => (4, 7);
});

#[test]
fn branch_and_not_dfa_eligible() {
    let re = VimRegex::new("a\\&b").unwrap();
    assert!(!re.is_dfa_eligible());
}

#[test]
fn branch_and_with_alternation() {
    // Tests which branch matches — keep manual to check substring.
    let re = VimRegex::new("\\(cat\\|dog\\)\\&\\w\\{3}").unwrap();
    let ctx = MatchContext::simple("the cat and dog");
    let m = re.find(&ctx).unwrap().unwrap();
    let matched = &ctx.text[m.range.clone()];
    assert!(matched == "cat" || matched == "dog");
}

#[test]
fn branch_and_feature_flag_propagated() {
    let re = VimRegex::new("a\\&b").unwrap();
    assert!(re.features().has_branch_and);
}

#[test]
fn no_branch_and_feature_flag_without_operator() {
    let re = VimRegex::new("abc").unwrap();
    assert!(!re.features().has_branch_and);
}

#[test]
fn branch_and_single_char_class_no_match() {
    let re = VimRegex::new("\\d\\&f").unwrap();
    let ctx = MatchContext::simple("f123");
    assert!(
        matches!(re.ir(), VimPatternNode::BranchAnd(_)),
        "IR should be BranchAnd, got {:?}",
        re.ir()
    );
    assert!(re.find(&ctx).unwrap().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// ANYWHERE ANCHORS (\_^ / \_$) INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(anywhere_anchors_suite {
    sol_no_match_without_nl_consumption:  r"foo\_^bar",             "foo\nbar"   => ();
    sol_at_start_of_text:                 r"\_^foo",                "foobar"     => (0, 3);
    sol_matches_second_line:              r"\_^bar",                "foo\nbar"   => (4, 7);
    sol_no_match_mid_line:                r"\_^bar",                "foobar"     => ();
    eol_matches_before_newline:           r"foo\_$",                "foo\nbar"   => (0, 3);
    eol_matches_at_end_of_text:           r"foo\_$",                "foo"        => (0, 3);
    eol_no_match_mid_line:                r"foo\_$",                "foobar"     => ();
    eol_followed_by_nl_then_sol:          "foo\\_$\\n\\_^bar",     "foo\nbar"   => (0, 7);
    sol_inside_group:                     r"\(\_^foo\)",            "hello\nfoo" => (6, 9);
    sol_always_anchor_mid_pattern:        "x\\n\\_^y",             "x\ny"       => (0, 3);
    eol_with_any_char_nl:                 "foo\\_$\\_.*\\_^bar",   "foo\nbar"   => (0, 7);
    sol_after_quantifier_no_match:        "\\d\\+\\_^foo",         "123\nfoo"   => ();
    eol_mid_pattern:                      "a\\_$\\n\\_^b",         "a\nb"       => (0, 3);
});

#[test]
fn anywhere_anchors_in_alternation() {
    // Tests which branch matched — keep manual to verify correct one.
    let re = VimRegex::new("\\_^foo\\|\\_^bar").unwrap();
    let ctx = MatchContext::simple("hello\nbar\nfoo");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.clone()], "bar");
}

#[test]
fn anywhere_sol_differs_from_bare_caret() {
    // Tests two distinct patterns on same text — keep manual.
    let re_bare = VimRegex::new("foo^bar").unwrap();
    let ctx = MatchContext::simple("foo^bar");
    let m = re_bare.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.clone()], "foo^bar");

    let re_anywhere = VimRegex::new("foo\\_^bar").unwrap();
    assert!(re_anywhere.find(&ctx).unwrap().is_none());
}

#[test]
fn anywhere_eol_differs_from_bare_dollar() {
    // Tests two distinct patterns on same text — keep manual.
    let re_bare = VimRegex::new("foo$bar").unwrap();
    let ctx = MatchContext::simple("foo$bar");
    let m = re_bare.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.clone()], "foo$bar");

    let re_anywhere = VimRegex::new("foo\\_$bar").unwrap();
    assert!(re_anywhere.find(&ctx).unwrap().is_none());
}

#[test]
fn anywhere_sol_with_multiline_find_all() {
    regex("\\_^\\w\\+")
        .text("foo\nbar\nbaz")
        .expect_all_matches(&[0..3, 4..7, 8..11])
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// \Z — IGNORE COMPOSING CHARACTERS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ignore_composing_matches_base_char_with_combining_accent() {
    regex("\\Ze").text("e\u{0301}").expect_match(0..3).run();
}

#[test]
fn ignore_composing_matches_word_with_combined_chars() {
    regex("\\Zcafe")
        .text("caf\u{0065}\u{0301}")
        .expect_match(0..6)
        .run();
}

#[test]
fn respect_composing_does_not_skip_marks() {
    // Tests match length, not just range — keep manual for clarity.
    let re = VimRegex::new("e").unwrap();
    let text = "e\u{0301}x";
    let ctx = MatchContext::simple(text);
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range.end - m.range.start, 1);
}

crate::test_harness::regex_suite!(ignore_composing_suite {
    plain_text_no_marks:    r"\Zabc", "xabcy" => (1, 4);
    wrong_base_no_match:    r"\Za",   "e\u{0301}" => ();
});

#[test]
fn ignore_composing_multiple_combining_marks() {
    regex("\\Ze")
        .text("e\u{0301}\u{0327}")
        .expect_match(0..5)
        .run();
}

#[test]
fn ignore_composing_matches_at_offset() {
    regex("\\Ze").text("xe\u{0301}y").expect_match(1..4).run();
}

#[test]
fn ignore_composing_with_dot_matches_base_plus_marks() {
    regex("\\Z.").text("e\u{0301}x").expect_match(0..3).run();
}

#[test]
fn ignore_composing_find_all() {
    regex("\\Ze")
        .text("e\u{0301} e\u{0301}")
        .expect_all_matches(&[0..3, 4..7])
        .run();
}

#[test]
fn ignore_composing_is_match() {
    // is_match is auto-checked by invariant 1 via expect_match; use builder.
    regex("\\Ze").text("e\u{0301}").expect_match(0..3).run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// CACHE COMPATIBILITY — uses cache API, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn cache_compatibility_undersized_cache_resizes_in_release() {
    let simple = VimRegex::new("a").unwrap();
    let complex = VimRegex::new("\\(foo\\|bar\\|baz\\)\\+").unwrap();
    let mut cache = simple.create_cache();
    let ctx = MatchContext::simple("foobarbaz");
    let complex_states = complex.create_cache().state_count();
    cache.ensure_capacity(complex_states, 20);
    let m = complex
        .find_at_with_cache(&mut cache, &ctx, 0)
        .unwrap()
        .unwrap();
    assert_eq!(m.range, 0..9);
}

#[test]
fn cache_is_compatible_after_create_cache() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let cache = re.create_cache();
    assert!(cache.is_compatible_with_state_count(re.features().capture_count as usize));
}

// ═══════════════════════════════════════════════════════════════════════════════
// FIND_ITER — LAZY ITERATOR — uses cache API, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn find_iter_digits() {
    let re = VimRegex::new("\\d\\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("abc 123 def 456");
    let matches: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].range, 4..7);
    assert_eq!(matches[1].range, 12..15);
}

#[test]
fn find_iter_zero_width_caret() {
    let re = VimRegex::new("^").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("a\nb\nc");
    let matches: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    assert!(
        !matches.is_empty(),
        "zero-width ^ should yield at least one match"
    );
    assert_eq!(matches[0].range, 0..0);
    for m in &matches {
        assert!(m.range.is_empty(), "^ match should be zero-width");
    }
}

#[test]
fn find_iter_empty_text() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("");
    let matches: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    assert!(matches.is_empty());
}

#[test]
fn find_iter_equals_find_all_words() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("hello world foo bar");
    let iter_results: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    let all_results = re.find_all(&ctx).unwrap();
    assert_eq!(iter_results.len(), all_results.len());
    for (a, b) in iter_results.iter().zip(all_results.iter()) {
        assert_eq!(a.range, b.range);
    }
}

#[test]
fn find_iter_equals_find_all_alternation() {
    let re = VimRegex::new("foo\\|bar\\|baz").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("foo and bar and baz");
    let iter_results: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    let all_results = re.find_all(&ctx).unwrap();
    assert_eq!(iter_results.len(), all_results.len());
    for (a, b) in iter_results.iter().zip(all_results.iter()) {
        assert_eq!(a.range, b.range);
    }
}

#[test]
fn find_iter_equals_find_all_single_char() {
    let re = VimRegex::new("a").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("abacada");
    let iter_results: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    let all_results = re.find_all(&ctx).unwrap();
    assert_eq!(iter_results.len(), all_results.len());
    for (a, b) in iter_results.iter().zip(all_results.iter()) {
        assert_eq!(a.range, b.range);
    }
}

#[test]
fn find_iter_equals_find_all_capturing() {
    let re = VimRegex::new("\\(\\w\\+\\)=\\(\\w\\+\\)").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("a=b c=d e=f");
    let iter_results: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    let all_results = re.find_all(&ctx).unwrap();
    assert_eq!(iter_results.len(), all_results.len());
    for (a, b) in iter_results.iter().zip(all_results.iter()) {
        assert_eq!(a.range, b.range);
        assert_eq!(a.captures, b.captures);
    }
}

#[test]
fn find_iter_equals_find_all_dot_star() {
    let re = VimRegex::new("x").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("axbxcxd");
    let iter_results: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    let all_results = re.find_all(&ctx).unwrap();
    assert_eq!(iter_results.len(), all_results.len());
    for (a, b) in iter_results.iter().zip(all_results.iter()) {
        assert_eq!(a.range, b.range);
    }
}

#[test]
fn percent_c_matches_combining_mark() {
    // U+0301 is COMBINING ACUTE ACCENT (category Mn).
    let re = VimRegex::new(r"\%C").unwrap();
    let text = "e\u{0301}x"; // 'e' + combining accent + 'x'
    let m = re.find(&MatchContext::simple(text)).unwrap();
    assert!(m.is_some(), r"\%C should match the combining accent");
    let m = m.unwrap();
    // The combining accent starts at byte offset 1 (after 'e').
    assert_eq!(m.range.start, 1);
}

#[test]
fn percent_c_does_not_match_ascii() {
    let re = VimRegex::new(r"\%C").unwrap();
    let m = re.find(&MatchContext::simple("abc")).unwrap();
    assert!(m.is_none(), r"\%C should not match ASCII characters");
}

#[test]
fn matches_iterator_basic() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("hello world");
    let matches: Vec<_> = re.find_iter(&mut cache, &ctx).collect();
    assert_eq!(matches.len(), 2);
}

#[test]
fn matches_iterator_no_error_on_success() {
    let re = VimRegex::new("\\d\\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("abc 123 def 456");
    let mut iter = re.find_iter(&mut cache, &ctx);
    while iter.next().is_some() {}
    assert!(
        iter.error().is_none(),
        "successful iteration should not produce an error"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// FIND_BACKWARD_IN — EXPLICIT RANGE BACKWARD SEARCH — uses cache API, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn find_backward_in_middle_range() {
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
fn find_backward_in_first_range() {
    let re = VimRegex::new(".").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::with_cursor("foo bar baz", 11);
    let m = re
        .find_backward_in_range_with_cache(&mut cache, &ctx, 0..3)
        .unwrap()
        .unwrap();
    assert_eq!(m.range, 2..3);
}

#[test]
fn find_backward_in_empty_range_returns_none() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::with_cursor("hello", 5);
    let result = re
        .find_backward_in_range_with_cache(&mut cache, &ctx, 3..3)
        .unwrap();
    assert!(result.is_none());
}

#[test]
fn find_backward_in_full_range_equals_find_backward() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let text = "hello world";
    let ctx = MatchContext::with_cursor(text, text.len());
    let full_backward = re.find_backward(&ctx).unwrap();
    let mut cache = re.create_cache();
    let range_backward = re
        .find_backward_in_range_with_cache(&mut cache, &ctx, 0..text.len())
        .unwrap();
    assert_eq!(full_backward, range_backward);
}

#[test]
fn find_backward_in_range_convenience() {
    let re = VimRegex::new("\\w\\+").unwrap();
    let ctx = MatchContext::with_cursor("aaa bbb ccc", 11);
    let m = re.find_backward_in_range(&ctx, 4..7).unwrap().unwrap();
    assert_eq!(m.range, 4..7);
}

// ═══════════════════════════════════════════════════════════════════════════════
// NFA STATE BUDGET — tests error conditions, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn nfa_state_budget_rejects_pathological_pattern() {
    let pattern = r"a\{0,200000}";
    let result = VimRegex::new(pattern);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        format!("{err}").contains("too complex") || format!("{err}").contains("budget"),
        "Expected PatternTooComplex, got: {err}"
    );
}

#[test]
fn nfa_state_budget_allows_normal_patterns() {
    let patterns = [
        r"\w\+",
        r"\(foo\|bar\)\{1,10}",
        r"[a-zA-Z0-9_]\+@[a-zA-Z0-9]\+\.[a-z]\{2,4}",
        r"\v(\w+)\s*=\s*(\w+)",
    ];
    for pat in &patterns {
        assert!(VimRegex::new(pat).is_ok(), "Pattern should compile: {pat}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MEMORY CONFIG — uses MemoryConfig internals, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn memory_config_default_matches_existing_constants() {
    use crate::common::MemoryConfig;
    let config = MemoryConfig::default();
    assert_eq!(config.dfa_budget, 4 * 1024 * 1024);
    assert_eq!(config.visited_capacity, 256 * 1024);
    assert_eq!(config.backtracker_max_depth, 10_922);
    assert_eq!(config.nfa_state_budget, 100_000);
    assert_eq!(config.backward_scan_window, 1024 * 1024);
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND NESTING LIMIT — tests error conditions, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn lookaround_nesting_within_limit() {
    let re = VimRegex::new(r"\%(\%(foo\)\@=\)\@=bar");
    assert!(re.is_ok(), "3-level lookaround nesting should compile");
}

#[test]
fn lookaround_nesting_at_limit_rejects() {
    let mut pattern = String::from("foo");
    for _ in 0..33 {
        pattern = format!(r"\%({}\)\@=", pattern);
    }
    let result = VimRegex::new(&pattern);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        format!("{err}").contains("too complex") || format!("{err}").contains("nesting"),
        "Expected nesting limit error, got: {err}"
    );
}

#[test]
fn lookaround_nesting_exact_boundary() {
    let mut pattern = String::from("x");
    for _ in 0..32 {
        pattern = format!(r"\%({}\)\@=", pattern);
    }
    let result = VimRegex::new(&pattern);
    assert!(
        result.is_ok(),
        "32-level nesting should succeed (at limit), got: {}",
        result.as_ref().unwrap_err()
    );

    let pattern_33 = format!(r"\%({}\)\@=", pattern);
    let result_33 = VimRegex::new(&pattern_33);
    assert!(
        result_33.is_err(),
        "33-level nesting should fail (over limit)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// API IMPROVEMENTS — Display/FromStr, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn as_str_round_trip() {
    let re = VimRegex::new(r"\d\+").unwrap();
    assert_eq!(re.as_str(), r"\d\+");
}

#[test]
fn display_shows_pattern() {
    let re = VimRegex::new("hello").unwrap();
    assert_eq!(format!("{re}"), "hello");
}

#[test]
fn from_str_compiles() {
    let re: VimRegex = r"\w\+".parse().unwrap();
    assert_eq!(re.as_str(), r"\w\+");
}

#[test]
fn from_str_invalid_pattern() {
    let result: Result<VimRegex, _> = r"\(".parse();
    assert!(result.is_err());
}

#[test]
fn as_str_preserves_with_magic() {
    let re = VimRegex::with_magic(r"\w+", MagicMode::VeryMagic).unwrap();
    assert_eq!(re.as_str(), r"\w+");
}

// ─── find_at / find_at_with_cache — uses offset API, keep manual ────────────

#[test]
fn find_at_basic() {
    let re = VimRegex::new(r"\d\+").unwrap();
    let ctx = MatchContext::simple("abc 123 def 456");
    let m = re.find_at(&ctx, 0).unwrap().unwrap();
    assert_eq!(m.range, 4..7);

    let m2 = re.find_at(&ctx, 7).unwrap().unwrap();
    assert_eq!(m2.range, 12..15);
}

#[test]
fn find_at_with_cache_basic() {
    let re = VimRegex::new(r"\w\+").unwrap();
    let mut cache = re.create_cache();
    let ctx = MatchContext::simple("hello world");

    let m = re.find_at_with_cache(&mut cache, &ctx, 0).unwrap().unwrap();
    assert_eq!(m.range, 0..5);

    let m2 = re.find_at_with_cache(&mut cache, &ctx, 6).unwrap().unwrap();
    assert_eq!(m2.range, 6..11);
}

// ─── memory_usage — uses memory_usage(), keep manual ────────────────────────

#[test]
fn memory_usage_is_nonzero() {
    let re = VimRegex::new(r"\w\+").unwrap();
    assert!(re.memory_usage() > 0);
    let cache = re.create_cache();
    assert!(cache.memory_usage() > 0);
}

#[test]
fn complex_pattern_uses_more_memory() {
    let simple = VimRegex::new("a").unwrap();
    let complex = VimRegex::new(r"\(\w\+\)\s\+\(\d\+\)\s\+\(\w\+\)").unwrap();
    assert!(complex.memory_usage() >= simple.memory_usage());
}

// ─── has_multiline feature flag — uses features(), keep manual ──────────────

#[test]
fn multiline_feature_flag_newline() {
    let re = VimRegex::new(r"hello\nworld").unwrap();
    assert!(re.features().has_multiline);
}

#[test]
fn multiline_feature_flag_underscore_dot() {
    let re = VimRegex::new(r"hello\_.*world").unwrap();
    assert!(re.features().has_multiline);
}

#[test]
fn multiline_feature_flag_underscore_class() {
    let re = VimRegex::new(r"hello\_s\+world").unwrap();
    assert!(re.features().has_multiline);
}

#[test]
fn multiline_feature_flag_normal_pattern() {
    let re = VimRegex::new(r"hello.*world").unwrap();
    assert!(!re.features().has_multiline);
}

#[test]
fn multiline_feature_flag_literal_only() {
    let re = VimRegex::new("foobar").unwrap();
    assert!(!re.features().has_multiline);
}

// ─── Nested quantifier reduction (integration) ─────────────────────────

crate::test_harness::regex_suite!(nested_quantifier_suite {
    grouped_star_star:     r"\(a*\)*",  "aaa" => (0, 3);
    grouped_plus_plus:     r"\(a\+\)\+", "aaa" => (0, 3);
    plus_plus_no_match:    r"\(a\+\)\+", "bbb" => ();
});

#[path = "required_literal.rs"]
mod required_literal;

#[path = "start_bitmap_minlen.rs"]
mod start_bitmap_minlen;

// ═══════════════════════════════════════════════════════════════════════════════
// APPROXIMATE MATCH TYPES — uses FuzzyConfig, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn fuzzy_config_default_values() {
    let config = crate::FuzzyConfig::default();
    assert_eq!(config.max_cost, 0);
    assert_eq!(config.cost_insert, 1);
    assert_eq!(config.cost_delete, 1);
    assert_eq!(config.cost_substitute, 1);
    assert_eq!(config.max_errors, None);
}

#[test]
fn fuzzy_config_builder() {
    let config = crate::FuzzyConfig {
        max_cost: 3,
        cost_insert: 1,
        cost_delete: 1,
        cost_substitute: 2,
        max_errors: Some(2),
    };
    assert_eq!(config.max_cost, 3);
    assert_eq!(config.cost_substitute, 2);
    assert_eq!(config.max_errors, Some(2));
}

// ═══════════════════════════════════════════════════════════════════════════════
// APPROXIMATE MATCH — PUBLIC API — uses find_approximate, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn find_approximate_exact_match() {
    let re = crate::VimRegex::new("hello").unwrap();
    let ctx = crate::matchers::MatchContext::simple("hello world");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].1, 0, "exact match has cost 0");
    assert_eq!(results[0].0.range, 0..5);
}

#[test]
fn find_approximate_one_substitution() {
    let re = crate::VimRegex::new("hello").unwrap();
    let ctx = crate::matchers::MatchContext::simple("hxllo world");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(!results.is_empty());
    let best = &results[0];
    assert_eq!(best.1, 1, "one substitution = cost 1");
    assert_eq!(best.0.range, 0..5);
}

#[test]
fn find_approximate_no_match_over_budget() {
    let re = crate::VimRegex::new("abcdef").unwrap();
    let ctx = crate::matchers::MatchContext::simple("xxxxxx");
    let config = crate::FuzzyConfig::with_max_edits(2);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(
        results.is_empty(),
        "6 substitutions with budget 2 should produce no match"
    );
}

#[test]
fn find_approximate_insertion() {
    let re = crate::VimRegex::new("abc").unwrap();
    let ctx = crate::matchers::MatchContext::simple("aXbc");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(!results.is_empty(), "insertion match expected");
    let best = results.iter().min_by_key(|r| r.1).unwrap();
    assert_eq!(best.1, 1);
}

#[test]
fn find_approximate_deletion() {
    let re = crate::VimRegex::new("abc").unwrap();
    let ctx = crate::matchers::MatchContext::simple("ac");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(!results.is_empty(), "deletion match expected");
    let best = results.iter().min_by_key(|r| r.1).unwrap();
    assert_eq!(best.1, 1);
}

#[test]
fn find_approximate_multiple_results_sorted() {
    let re = crate::VimRegex::new("foo").unwrap();
    let ctx = crate::matchers::MatchContext::simple("foo fxo");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(results.len() >= 2, "should find at least 2 matches");
    for window in results.windows(2) {
        assert!(
            window[0].1 <= window[1].1,
            "results should be sorted by cost"
        );
    }
}

#[test]
fn find_approximate_with_cache() {
    let re = crate::VimRegex::new("hello").unwrap();
    let mut cache = re.create_cache();
    let ctx = crate::matchers::MatchContext::simple("hxllo");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re
        .find_approximate_with_cache(&mut cache, &ctx, &config)
        .unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].1, 1);
}

#[test]
fn find_approximate_regex_with_quantifier() {
    let re = crate::VimRegex::new("a\\+b").unwrap();
    let ctx = crate::matchers::MatchContext::simple("aab");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    let best = results.iter().min_by_key(|r| r.1).unwrap();
    assert_eq!(best.1, 0, "exact match should have cost 0");
}

#[test]
fn find_approximate_unicode_text() {
    let re = crate::VimRegex::new("caf\u{00E9}").unwrap();
    let ctx = crate::matchers::MatchContext::simple("cafX");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    let best = results.iter().min_by_key(|r| r.1);
    assert!(
        best.is_some(),
        "should find fuzzy match for unicode pattern"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// EDIT DISTANCE VERIFICATION — uses levenshtein helper, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute Levenshtein distance for verification (simple reference implementation).
fn levenshtein(a: &str, b: &str) -> u16 {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();
    let mut dp = vec![vec![0u16; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i as u16;
    }
    for (j, val) in dp[0].iter_mut().enumerate().take(n + 1) {
        *val = j as u16;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}

#[test]
fn edit_distance_matches_levenshtein_for_literals() {
    let cases: &[(&str, &str)] = &[
        ("abc", "abc"),
        ("abc", "axc"),
        ("abc", "abXc"),
        ("abc", "ac"),
        ("abc", "xyz"),
        ("a", "b"),
    ];

    for &(pattern, text) in cases {
        let expected_dist = levenshtein(pattern, text);
        let max_edits = expected_dist.max(3);
        let re = crate::VimRegex::new(pattern).unwrap();
        let ctx = crate::matchers::MatchContext::simple(text);
        let config = crate::FuzzyConfig::with_max_edits(max_edits);
        let results = re.find_approximate(&ctx, &config).unwrap();

        if expected_dist <= max_edits {
            let best = results.iter().min_by_key(|r| r.1);
            assert!(
                best.is_some(),
                "expected match for pattern={pattern:?} text={text:?} dist={expected_dist}"
            );
            let best = best.unwrap();
            assert!(
                best.1 <= expected_dist,
                "best cost {} > levenshtein {} for pattern={pattern:?} text={text:?}",
                best.1,
                expected_dist,
            );
        }
    }
}

#[test]
fn approximate_match_in_longer_text() {
    let re = crate::VimRegex::new("hello").unwrap();
    let ctx = crate::matchers::MatchContext::simple("xxhelloxx");
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    let exact = results.iter().find(|r| r.1 == 0);
    assert!(exact.is_some(), "should find exact substring match");
    assert_eq!(exact.unwrap().0.range, 2..7);
}

#[test]
fn approximate_match_zero_budget_is_exact() {
    let re = crate::VimRegex::new("abc").unwrap();
    let ctx = crate::matchers::MatchContext::simple("abc");
    let config = crate::FuzzyConfig {
        max_cost: 0,
        ..Default::default()
    };
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].1, 0);

    let ctx2 = crate::matchers::MatchContext::simple("axc");
    let results2 = re.find_approximate(&ctx2, &config).unwrap();
    assert!(results2.is_empty(), "zero budget should reject mismatches");
}

#[test]
fn approximate_match_case_sensitive() {
    let re = crate::VimRegex::new("abc").unwrap();
    let ctx = crate::matchers::MatchContext {
        text: "ABC",
        cursor: None,
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let config = crate::FuzzyConfig::with_max_edits(0);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(results.is_empty(), "case-sensitive: ABC != abc");
}

#[test]
fn approximate_match_bounded_results() {
    let re = crate::VimRegex::new("a").unwrap();
    let text = "a".repeat(100);
    let ctx = crate::matchers::MatchContext::simple(&text);
    let config = crate::FuzzyConfig::with_max_edits(1);
    let results = re.find_approximate(&ctx, &config).unwrap();
    assert!(
        results.len() <= 300,
        "results should be bounded, got {}",
        results.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR POSITION — \%# false match regression tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn cursor_position_no_false_match_at_zero() {
    // Without setting cursor, \%# should NOT match at position 0.
    let re = VimRegex::new(r"\%#hello").unwrap();
    let ctx = MatchContext::simple("hello");
    assert!(
        re.find(&ctx).unwrap().is_none(),
        "\\%# should not match when no cursor is set"
    );
}

#[test]
fn cursor_position_matches_when_set() {
    let re = VimRegex::new(r"\%#hello").unwrap();
    let ctx = MatchContext::with_cursor("hello", 0);
    assert!(
        re.find(&ctx).unwrap().is_some(),
        "\\%# should match at cursor position"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// WORD BOUNDARY — Unicode letter support
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn word_boundary_unicode_letter() {
    regex(r"\<\w\+\>")
        .text("hello world")
        .expect_match(0..5)
        .run();
    // Precomposed e (U+00E9) should be treated as a word char
    regex("\\<caf\u{00e9}\\>")
        .text("hello caf\u{00e9} world")
        .expect_match(6..11)
        .run();
}
