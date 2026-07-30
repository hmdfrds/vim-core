//! Hybrid codepoint DFA: correctness, and equivalence with the
//! unaccelerated engine.
//!
//! These tests exercise the DFA through the PUBLIC `VimRegex` API.
//! The DFA fires transparently for eligible patterns via the `HybridDfa`
//! rung of the strategy cascade. Tests use the `regex()` builder and
//! `regex_suite!` macro for concise tabular tests.
//!
//! Tests that verify DFA internals (eligibility flags, cache behaviour,
//! features) remain as manual `#[test]` functions below.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;
use crate::test_builder::regex;

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS (only for non-convertible tests that access internals)
// ═══════════════════════════════════════════════════════════════════════════════

/// Returns true if a pattern is DFA-eligible.
fn is_dfa_eligible(pattern: &str) -> bool {
    let re = VimRegex::new(pattern).expect("valid pattern");
    re.is_dfa_eligible()
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 1: DFA Eligibility Verification (non-convertible — internal API)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn eligibility_word_plus_is_dfa_eligible() {
    assert!(is_dfa_eligible("\\w\\+"));
}

#[test]
fn eligibility_digit_plus_is_dfa_eligible() {
    assert!(is_dfa_eligible("\\d\\+"));
}

#[test]
fn eligibility_collection_is_dfa_eligible() {
    assert!(is_dfa_eligible("[a-z]\\+"));
}

#[test]
fn eligibility_literal_is_dfa_eligible() {
    assert!(is_dfa_eligible("foo"));
}

#[test]
fn eligibility_lookaround_not_dfa_eligible() {
    let re = VimRegex::new("\\(foo\\)\\@<=bar").unwrap();
    assert!(re.features().has_lookaround);
}

#[test]
fn eligibility_backref_not_dfa_eligible() {
    let re = VimRegex::new("\\(a\\)\\1").unwrap();
    assert!(re.features().has_backreferences);
}

#[test]
fn eligibility_buffer_position_not_dfa_eligible() {
    let re = VimRegex::new("\\%23lfoo").unwrap();
    assert!(re.features().has_buffer_position);
}

#[test]
fn eligibility_alternation_is_dfa_eligible() {
    assert!(is_dfa_eligible("foo\\|bar\\|baz"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 2: Basic Correctness (DFA-eligible patterns)
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(basic_correctness {
    digit_plus:              r"\d\+",     "abc123def"    => (3, 6);
    lowercase_collection:    r"[a-z]\+",  "123hello456"  => (3, 8);
    dot_star:                ".*",        "hello"        => (0, 5);
    alternation:             r"cat\|dog", "I have a dog" => (9, 12);
    quantifier_bounded:      r"a\{2,4}",  "aaaaa"        => (0, 4);
    any_char_nl:             r"\_.",      "a\nb"         => (0, 1);
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 3: Equivalence (DFA result = Pike VM result)
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(equivalence {
    digit_plus:           r"\d\+",      "abc123def456"  => (3, 6);
    word_plus:            r"\w\+",      "  hello  world" => (2, 7);
    alpha_class:          r"\a\+",      "123abc456"     => (3, 6);
    negated_collection:   r"[^0-9]\+",  "123abc456"     => (3, 6);
    alternation_leftmost: r"foo\|bar",  "barfoo"        => (0, 3);
    greedy_star:          "a*",         "aaa"           => (0, 3);
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 4: Two-Phase (captures)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn twophase_captures_extracted() {
    regex(r"\(\w\+\)@\(\w\+\)")
        .text("user@host more text")
        .expect_match(0..9)
        .expect_capture(1, 0..4)
        .expect_capture(2, 5..9)
        .run();
}

#[test]
fn twophase_captures_nested() {
    regex(r"\(\(\d\+\)\.\(\d\+\)\)")
        .text("version 12.34 end")
        .expect_match(8..13)
        .expect_capture(1, 8..13)
        .expect_capture(2, 8..10)
        .expect_capture(3, 11..13)
        .run();
}

#[test]
fn twophase_single_capture() {
    regex(r"\(\d\+\)")
        .text("abc42xyz")
        .expect_match(3..5)
        .expect_capture(1, 3..5)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 5: find_all
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(find_all {
    digit_groups:  r"\d\+",      "a12b34c567d8"      => all[(1,3), (4,6), (7,10), (11,12)];
    word_tokens:   r"\w\+",      "one two three"      => all[(0,3), (4,7), (8,13)];
    alternation:   r"foo\|bar",  "foo and bar and foo" => all[(0,3), (8,11), (16,19)];
    no_matches:    r"\d\+",      "no digits here"     => ();
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 6: Edge Cases
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(edge_cases {
    match_at_position_zero:          r"\d\+",     "42abc"     => (0, 2);
    match_at_end_of_text:            r"\d\+",     "abc99"     => (3, 5);
    overlapping_candidates_leftmost: r"[a-z]\+",  "123abcdef" => (3, 9);
    // cafe\u{0301} = 4 + 2 bytes for combining accent, then space, "42" at byte 7..9.
    unicode_text_ascii_pattern:      r"\d\+",     "cafe\u{0301} 42 blah" => (7, 9);
    // \u{1F600} = 4 bytes, "hello" = 5, "42" at byte 9..11.
    unicode_multibyte_boundary:      r"\d\+",     "\u{1F600}hello42world" => (9, 11);
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 7: DFA fallback and ineligibility edge cases
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(fallback {
    complex_pattern:   r"[a-z]\+[0-9]\+[a-z]\+[0-9]\+[a-z]\+", "abc123def456ghi rest" => (0, 15);
    lazy_quantifier:   r"a\{-1,}b", "aaab" => (0, 4);
    ci_bypasses_dfa:         r"\c[a-z]\+",  "HELLO"       => (0, 5);
    ci_bypasses_dfa_mid:     r"\c[a-z]\+",  "123HELLO456" => (3, 8);
    zero_width_caret:        "^foo",        "foo bar"     => (0, 3);
    zero_width_word_bound:   r"\<word\>",   "a word b"    => (2, 6);
});

#[test]
fn eligibility_atomic_not_dfa() {
    let re = VimRegex::new("\\(foo\\)\\@>bar").unwrap();
    let f = re.features();
    assert!(f.has_atomic, "atomic group should set flag");
}

#[test]
fn twophase_nested_captures_correct() {
    regex(r"\(\(a\)b\(c\)\)")
        .text("abc")
        .expect_match(0..3)
        .expect_capture(1, 0..3)
        .expect_capture(2, 0..1)
        .expect_capture(3, 2..3)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 7b: DfaCache case_sensitive stored (non-convertible — internal API)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn dfa_cache_case_sensitive_stored() {
    use crate::engines::lazy_dfa::DfaCache;
    use crate::hir::lower;
    use crate::ir::VimPatternNode;
    use crate::nfa::builder::NfaBuilder;

    let node = VimPatternNode::Literal('a');
    let (lowered, _) = lower(&node);
    let nfa = NfaBuilder::build(&lowered).unwrap();

    let cache_cs = DfaCache::new(&nfa, true, false);
    assert!(
        cache_cs.case_sensitive(),
        "DfaCache built with case_sensitive=true must report true"
    );

    let cache_ci = DfaCache::new(&nfa, false, false);
    assert!(
        !cache_ci.case_sensitive(),
        "DfaCache built with case_sensitive=false must report false"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 8: Alternation Priority (leftmost-first, not leftmost-longest)
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(alternation_priority {
    a_or_ab:           r"a\|ab",        "ab"     => (0, 1);
    http_or_https:     r"http\|https",  "https"  => (0, 4);
    foo_or_foobar:     r"foo\|foobar",  "foobar" => (0, 3);
    reversed:          r"ab\|a",        "ab"     => (0, 2);
    no_conflict:       r"\w\+",         "abc"    => (0, 3);
    // The builder automatically checks invariant 3: find() == find_all()[0],
    // which is exactly what this test was verifying manually.
    find_vs_find_all:  r"a\|ab",        "ab"     => equiv;
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 9: CI Collection Matching in DFA (B4)
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(ci_collection {
    range_uppercase:    r"\c[a-z]\+",   "HELLO" => (0, 5);
    range_lowercase:    r"\c[A-Z]\+",   "hello" => (0, 5);
    single_vowel:       r"\c[aeiou]",   "E"     => (0, 1);
    negated_digit:      r"\c[^0-9]\+",  "abc"   => (0, 3);
    negated_letter:     r"\c[^a-z]",    "A"     => ();
    literal_still_works: r"\chello",    "HELLO" => (0, 5);
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 10: is_match DFA shortcut
// ═══════════════════════════════════════════════════════════════════════════════
//
// The `regex()` builder checks invariant 1: find().is_some() == is_match().
// So a `=> (s, e)` entry implicitly verifies is_match() == true,
// and a `=> ()` entry verifies is_match() == false.

crate::test_harness::regex_suite!(is_match_shortcut {
    dfa_true:             r"\w\+",               "hello"  => (0, 5);
    dfa_false:            r"\d\+",               "hello"  => ();
    dfa_alternation:      r"a\|ab",              "ab"     => (0, 1);
    ci:                   r"\chello",            "HELLO"  => (0, 5);
    ineligible_fallback:  r"\(foo\)\@<=bar",     "foobar" => (3, 6);
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 11: StartOfLine (^) DFA eligibility via context-sensitive start states
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(start_of_line {
    basic:              r"^\w\+",  "hello\nworld"    => (0, 5);
    after_newline:      r"^\w\+",  "\nhello"         => (1, 6);
    find_all:           r"^\w\+",  "hello\nworld\nfoo" => all[(0,5), (6,11), (12,15)];
    position_zero:      r"^\w\+",  "abc"             => (0, 3);
    empty_line:         "^$",      "hello\n\nworld"  => (6, 6);
    literal_hello:      "^hello",  "hello\nworld"    => (0, 5);
    literal_world:      "^world",  "hello\nworld"    => (6, 11);
    literal_nope:       "^nope",   "hello\nworld"    => ();
});

#[test]
fn start_of_line_mid_line_no_match() {
    // The builder doesn't support `expect_no_match_from`, so we use find_at directly.
    let re = VimRegex::new("^\\w\\+").unwrap();
    let ctx = MatchContext::simple("hello world");
    let result = re.find_at(&ctx, 6).expect("no engine error");
    assert!(
        result.is_none(),
        "^ should not match mid-line at position 6"
    );
}

#[test]
fn end_of_line_dfa_eligible_correctness() {
    regex("foo$").text("foo\nbar").expect_match(0..3).run();
}

// Non-convertible: checks is_dfa_eligible() internal API.
#[test]
fn start_of_line_dfa_eligible() {
    assert!(is_dfa_eligible("^\\w\\+"));
    assert!(is_dfa_eligible("^foo"));
    assert!(is_dfa_eligible("^[a-z]\\+"));
}

#[test]
fn end_of_line_dfa_eligible() {
    assert!(is_dfa_eligible("foo$"));
    assert!(is_dfa_eligible("^$"));
}

#[test]
fn word_boundary_dfa_eligible() {
    assert!(is_dfa_eligible("\\<word\\>"));
    assert!(is_dfa_eligible("\\<foo"));
}

#[test]
fn start_of_line_multiline_find_from() {
    regex("^\\w\\+")
        .text("hello\nworld\nfoo")
        .expect_match_from(6, 6..11)
        .run();
}

crate::test_harness::regex_suite!(start_of_line_anycharnl {
    // Regression: ^x\_.  — 'x' followed by any char, but only at start of line.
    // After consuming 'a' on line "ax", the DFA must NOT think we're at
    // start-of-line when it sees 'x' next.
    no_false_prev_newline: r"^x\_.",  "ax\nhello"  => ();
    matches_at_sol:        r"^x\_.",  "x\nhello"   => (0, 2);
    after_newline:         r"^x\_.",  "\nxyhello"   => (1, 3);
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 12: DFA-accelerated find_all
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(dfa_find_all {
    basic:                r"\w\+",      "hello world"   => all[(0,5), (6,11)];
    single_char:          "a",          "aaa"           => all[(0,1), (1,2), (2,3)];
    greedy:               r"a\+",       "aabaa"         => all[(0,2), (3,5)];
    alternation_priority: r"a\|ab",     "aab"           => all[(0,1), (1,2)];
    ci:                   r"\c[a-z]\+", "Hello WORLD"   => all[(0,5), (6,11)];
    no_match:             "xyz",        "abc"           => ();
    start_of_line:        r"^\w\+",     "hello\nworld"  => all[(0,5), (6,11)];
});

// The builder checks invariant 3 (find() == find_all()[0]) automatically,
// so these equivalence checks are covered by `=> equiv`.
crate::test_harness::regex_suite!(dfa_find_all_equivalence {
    word_tokens:   r"\w\+",      "one two three"       => equiv;
    digit_groups:  r"\d\+",      "a1b23c456"           => equiv;
    lower_alpha:   r"[a-z]\+",   "123abc456def"        => equiv;
    greedy_a:      r"a\+",       "aabaa"               => equiv;
    alternation:   r"foo\|bar",  "foo bar foo"         => equiv;
    sol_words:     r"^\w\+",     "hello\nworld\nfoo"   => equiv;
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 13: prev_word derivation — uppercase coverage (B1)
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(prev_word {
    uppercase_in_pattern:    r"\w\+",  "  HELLO  world"  => (2, 7);
    all_uppercase_word:      r"\w\+",  "!!!ABCDEF!!!"    => (3, 9);
    mixed_case_find_all:     r"\w\+",  "Hello WORLD foo_BAR baz123" => all[(0,5), (6,11), (12,19), (20,26)];
    uppercase_start_of_line: r"^\w\+", "HELLO\nWORLD\nFoo" => all[(0,5), (6,11), (12,15)];
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION: DFA State Map Hash (non-convertible — uses create_cache/find_with_cache)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn state_map_hash_lookup_no_collision() {
    let re = VimRegex::new(r"hello\|world").unwrap();
    let ctx = MatchContext::simple("hello world");
    let mut cache = re.create_cache();
    let m = re.find_with_cache(&mut cache, &ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..5);
    assert_eq!(&ctx.text[m.range], "hello");
}

#[test]
fn state_map_hash_reuse_across_searches() {
    let re = VimRegex::new(r"\d\+").unwrap();
    let ctx1 = MatchContext::simple("abc 123 def");
    let ctx2 = MatchContext::simple("xyz 456 uvw");
    let mut cache = re.create_cache();
    let m1 = re.find_with_cache(&mut cache, &ctx1).unwrap().unwrap();
    let m2 = re.find_with_cache(&mut cache, &ctx2).unwrap().unwrap();
    assert_eq!(&ctx1.text[m1.range], "123");
    assert_eq!(&ctx2.text[m2.range], "456");
}

#[test]
fn state_map_hash_with_word_boundaries() {
    let re = VimRegex::new(r"\<\w\+\>").unwrap();
    let ctx = MatchContext::simple("hello world test");
    let mut cache = re.create_cache();
    let m1 = re.find_at_with_cache(&mut cache, &ctx, 0).unwrap().unwrap();
    assert_eq!(&ctx.text[m1.range.clone()], "hello");
    let m2 = re
        .find_at_with_cache(&mut cache, &ctx, m1.range.end)
        .unwrap()
        .unwrap();
    assert_eq!(&ctx.text[m2.range.clone()], "world");
    let m3 = re
        .find_at_with_cache(&mut cache, &ctx, m2.range.end)
        .unwrap()
        .unwrap();
    assert_eq!(&ctx.text[m3.range.clone()], "test");
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 14: UTF-8 Multi-Byte DFA Bridging
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(utf8_cjk {
    // U+4E2D = 3 bytes; "hello" = 5 bytes => CJK at 5..8
    literal:       "\u{4E2D}",                    "hello\u{4E2D}world"          => (5, 8);
    collection:    "[\u{4E00}-\u{9FFF}]\\+",      "abc\u{4E2D}\u{6587}def"     => (3, 9);
    alternation:   "hello\\|\u{4E16}\u{754C}",    "\u{4E16}\u{754C}!"           => (0, 6);
    // "text" = 4 bytes, each CJK = 3 bytes => sequence at 4..13
    sequence:      "\u{65E5}\u{672C}\u{8A9E}",    "text\u{65E5}\u{672C}\u{8A9E}end" => (4, 13);
});

crate::test_harness::regex_suite!(utf8_emoji {
    // "hi" = 2 bytes, emoji = 4 bytes => at 2..6
    literal:   "\u{1F600}",                     "hi\u{1F600}bye"              => (2, 6);
    // "text" = 4 bytes, each emoji = 4 bytes => at 4..12
    range:     "[\u{1F600}-\u{1F64F}]\\+",      "text\u{1F601}\u{1F602}end"  => (4, 12);
    // \u{1F600} = 4 bytes, "hello" = 5 bytes => 0..9
    dot_star:  ".*",                            "\u{1F600}hello"              => (0, 9);
});

crate::test_harness::regex_suite!(utf8_mixed {
    // "I like " = 7 bytes, "caf" = 3, e-acute = 2 => match at 7..12
    ascii_unicode_literal:  "caf\u{00E9}",  "I like caf\u{00E9}s"  => (7, 12);
    // "caf" = 3 bytes, e-acute at 3..5
    two_byte_literal:       "\u{00E9}",     "caf\u{00E9}"           => (3, 5);
    // e-acute = 2 bytes, "l" = 1, e-grave = 2, "ve " = 3 => "42" at byte 8..10
    ascii_pattern_unicode:  r"\d\+",        "\u{00E9}l\u{00E8}ve 42 \u{00E9}cole" => (8, 10);
    // "r" + e-acute + "sum" + e-acute = 1+2+3+2 = 8 bytes => 0..8
    mixed_collection:       "[a-z\u{00E0}-\u{00FF}]\\+",  "r\u{00E9}sum\u{00E9}" => (0, 8);
});

crate::test_harness::regex_suite!(utf8_overlapping_prefix {
    // e-grave(2) then e-acute(2) => e-acute at 2..4
    two_byte:       "\u{00E9}",             "\u{00E8}\u{00E9}"      => (2, 4);
    // U+4E00(3) then U+4E2D(3) => U+4E2D at 3..6
    three_byte:     "\u{4E2D}",             "\u{4E00}\u{4E2D}"      => (3, 6);
    // U+4E2D(3) then U+6587(3) => U+4E2D at 0..3
    alternation_shared_prefix: "\u{4E2D}\\|\u{4E00}", "\u{4E2D}\u{6587}" => (0, 3);
});

crate::test_harness::regex_suite!(utf8_no_false_match {
    partial_decode:             "\u{00E9}",  "\u{00E8}"  => ();
    interleaved_continuation:   "\u{00E9}",  "\u{0429}"  => ();
    partial_3byte:              "\u{4E2D}",  "\u{4E00}"  => ();
});

// Equivalence corpus: each case checked via the builder's 18 invariants.
crate::test_harness::regex_suite!(utf8_equivalence {
    // "caf" = 3, e-acute(2) x2 = 4, " end" => match at 3..7
    e_acute_plus:       "\u{00E9}\\+",                "caf\u{00E9}\u{00E9} end"  => (3, 7);
    // emoji at byte 0..4
    emoji_literal:      "\u{1F600}",                  "\u{1F600}middle\u{1F601}" => (0, 4);
    // CJK = 3 bytes => 0..3
    any_char_nl_cjk:    "\\_.",                       "\u{4E2D}\n"               => (0, 3);
    // "r" + e-acute + "sum" + e-acute = 8 bytes
    mixed_collection:   "[a-z\u{00E0}-\u{00FF}]\\+",  "r\u{00E9}sum\u{00E9}"    => (0, 8);
});

#[test]
fn utf8_find_all_multibyte() {
    // "caf" = 3, e-acute(2) at 3..5, " r" = 2, e-acute(2) at 7..9,
    // "sum" = 3, e-acute(2) at 12..14
    regex("\u{00E9}")
        .text("caf\u{00E9} r\u{00E9}sum\u{00E9}")
        .expect_all_matches(&[3..5, 7..9, 12..14])
        .run();
}

crate::test_harness::regex_suite!(utf8_dot {
    // U+4E2D is 3 bytes => . matches 0..3
    matches_multibyte_char:  ".",     "\u{4E2D}x"  => (0, 3);
    // "a"(1) + e-acute(2) + "b"(1) + CJK(3) + "c"(1) = 8 bytes => 0..8
    plus_across_multibyte:   ".\\+",  "a\u{00E9}b\u{4E2D}c" => (0, 8);
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 15: QUIT Sentinel — DFA declines to Pike VM on unsupported features
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(dfa_quit_sentinel {
    backref_match:      r"\(foo\)\1",  "foofoo" => (0, 6);
    backref_no_match:   r"\(foo\)\1",  "foobar" => ();
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 16: AnyChar Matcher Multi-Byte Correctness
// ═══════════════════════════════════════════════════════════════════════════════

// Non-convertible: \zs patterns cause full_range != range, which trips
// the builder's invariant 6 (find_at at match.start returns None because
// the match actually starts consuming earlier). Keep as manual tests.

#[test]
fn dfa_anychar_matcher_3byte() {
    let re = VimRegex::new(r"\_.\zsfoo").unwrap();
    let input = "\u{4E2D}foo";
    let ctx = MatchContext::simple(input);
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&input[m.range], "foo");
}

#[test]
fn dfa_anychar_matcher_4byte() {
    let re = VimRegex::new(r"\_.\zsbar").unwrap();
    let input = "\u{1F600}bar";
    let ctx = MatchContext::simple(input);
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&input[m.range], "bar");
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 17: Negated Collection Non-ASCII -> QUIT
// ═══════════════════════════════════════════════════════════════════════════════

// Non-convertible: explicitly listed as non-convertible (DFA QUIT test).
#[test]
fn dfa_quits_on_negated_non_ascii_collection() {
    let re = VimRegex::new(r"[^\u{00E9}]").unwrap();
    let ctx = MatchContext::simple("hello");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..1);
}

crate::test_harness::regex_suite!(negated_ascii_collection {
    // [^abc] on "axyz" — skips 'a', matches 'x' at 1..2
    basic:           r"[^abc]",  "axyz"         => (1, 2);
    // [^abc] on e-acute + "abc" — e-acute (2 bytes) matches at 0..2
    multibyte_input: "[^abc]",   "\u{00E9}abc"  => (0, 2);
});

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 18: Scratch Buffer Pooling (non-convertible — uses create_cache)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn scratch_buffers_survive_multiple_transitions() {
    let re = VimRegex::new(r"\w\+@\w\+\.\w\+").unwrap();
    let mut cache = re.create_cache();
    for input in ["a@b.c", "user@host.com", "no-match-here", "x@y.z"] {
        let ctx = MatchContext::simple(input);
        let _ = re.find_with_cache(&mut cache, &ctx);
    }
}

#[test]
fn scratch_buffers_with_multibyte_patterns() {
    let re = VimRegex::new("[\u{00E0}-\u{00FF}]\\+").unwrap();
    let mut cache = re.create_cache();
    for input in [
        "r\u{00E9}sum\u{00E9}",
        "caf\u{00E9}",
        "no match",
        "\u{00E8}\u{00E9}\u{00EA}",
    ] {
        let ctx = MatchContext::simple(input);
        let _ = re.find_with_cache(&mut cache, &ctx);
    }
}
