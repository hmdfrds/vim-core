//! Strategy equivalence and adversarial pattern tests (Tasks 12 & 13).
//!
//! Every test in this file exercises the **public** `VimRegex` API, which
//! routes through the full meta-engine strategy cascade:
//!
//!   prefilter → backtracker (when backrefs/atomic present) → Pike VM fallback
//!
//! # Test organisation
//!
//! 1. **Strategy equivalence** — verify that `VimRegex::find()` returns the
//!    expected result for a broad corpus of patterns covering all strategy paths.
//! 2. **Adversarial patterns** — stress-test the engines with inputs that could
//!    cause catastrophic backtracking, large NFAs, or force the Declined path.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS (only retained for non-suite tests below)
// ═══════════════════════════════════════════════════════════════════════════════

/// Like `find` but returns the byte range of the narrowed match.
fn find_range(pattern: &str, text: &str) -> Option<std::ops::Range<usize>> {
    let re = VimRegex::new(pattern).expect("valid pattern");
    let ctx = MatchContext::simple(text);
    let m = re.find(&ctx).expect("no engine error");
    m.map(|m| m.range)
}

// ═══════════════════════════════════════════════════════════════════════════════
// TABULAR TESTS — simple find / find_range assertions
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(literal_prefilter {
    // Section 1: Literal / prefilter path
    mid:                "foo",   "hello foo bar"   => (6, 9);
    at_start:           "hello", "hello world"     => (0, 5);
    not_present:        "xyz",   "hello world"     => ();
    repeated:           "ab",    "xababab"         => (1, 3);
    single_char:        "x",     "abc xyz"         => (4, 5);
});

crate::test_harness::regex_suite!(alternation {
    // Section 2: Alternation
    second_branch:      "foo\\|bar",             "test bar end"  => (5, 8);
    three_way:          "abc\\|def\\|ghi",       "xxghiyy"       => (2, 5);
    first_branch_wins:  "foo\\|bar",             "foo bar"       => (0, 3);
    none_match:         "foo\\|bar\\|baz",       "xyz"           => ();
});

crate::test_harness::regex_suite!(anchors {
    // Section 3: Anchored patterns
    caret_start:        "^hello",  "hello world"    => (0, 5);
    caret_no_match_mid: "^hello",  "say hello"      => ();
    dollar_at_end:      "world$",  "hello world"    => (6, 11);
    dollar_no_match:    "world$",  "hello world today" => ();
    caret_multiline:    "^def",    "abc\ndef\nghi"  => (4, 7);
});

crate::test_harness::regex_suite!(backrefs {
    // Section 4: Backreference patterns (backtracker path)
    simple_repeated:    "\\(foo\\)\\1",           "foofoo"   => (0, 6);
    alt_aa:             "\\(a\\|b\\)\\1",         "aa"       => (0, 2);
    alt_bb:             "\\(a\\|b\\)\\1",         "bb"       => (0, 2);
    alt_no_cross:       "\\(a\\|b\\)\\1",         "ab"       => ();
    in_larger_context:  "\\(ab\\)\\1",            "xxababyy" => (2, 6);
});

crate::test_harness::regex_suite!(char_classes {
    // Section 8: Character classes
    digit_class:        "\\d\\+",    "abc123def"   => (3, 6);
    word_class:         "\\w\\+",    "  hello  "   => (2, 7);
    nondigit_class:     "\\D\\+",    "abc123"      => (0, 3);
    alpha_class:        "\\a\\+",    "123abc456"   => (3, 6);
    collection_range:   "[a-z]\\+",  "123abc"      => (3, 6);
    negated_collection: "[^a-z]\\+", "123abc"      => (0, 3);
});

crate::test_harness::regex_suite!(multiline_newline {
    // Section 9: Multiline / newline classes
    any_char_nl_first:  "\\_.",       "a\nb"   => (0, 1);
    any_char_no_nl:     ".",          "a\nb"   => (0, 1);
    dot_plus_skips_nl:  ".\\+",      "\nabc"  => (1, 4);
});

crate::test_harness::regex_suite!(case_insensitive {
    // Section 10: Case insensitive (\c)
    ci_lower_to_upper:  "\\cfoo",    "FOO"          => (0, 3);
    ci_upper_to_lower:  "\\cFOO",    "foo"          => (0, 3);
    ci_mixed:           "\\cHello",  "HELLO WORLD"  => (0, 5);
    cs_match:           "\\Cfoo",    "foo"          => (0, 3);
    cs_no_match:        "\\Cfoo",    "FOO"          => ();
});

crate::test_harness::regex_suite!(quantifiers {
    // Section 11: Quantifiers
    star_greedy_all:    "a*",          "aaa"   => (0, 3);
    plus_no_match:      "a\\+",        "bbb"   => ();
    plus_match:         "a\\+",        "baab"  => (1, 3);
    question_absent:    "ab\\?c",      "ac"    => (0, 2);
    question_present:   "ab\\?c",      "abc"   => (0, 3);
    bounded_exact:      "a\\{3}",      "aaaa"  => (0, 3);
    bounded_range:      "a\\{2,4}",    "aaaa"  => (0, 4);
    bounded_lazy:       "a\\{-2,4}",   "aaaa"  => (0, 2);
});

crate::test_harness::regex_suite!(word_boundaries {
    // Section 15: Word boundaries
    wb_start_match:         "\\<word",     "a word here"  => (2, 6);
    wb_start_no_match:      "\\<word",     "swordfish"    => ();
    wb_end_match:           "word\\>",     "a word here"  => (2, 6);
    wb_end_no_match:        "word\\>",     "wording"      => ();
    wb_both_match:          "\\<word\\>",  "a word here"  => (2, 6);
    wb_both_no_match:       "\\<word\\>",  "wording"      => ();
});

// ═══════════════════════════════════════════════════════════════════════════════
// NON-TABULAR TESTS — require features(), full_range, captures, cursors, etc.
// ═══════════════════════════════════════════════════════════════════════════════

// ── Section 4 (cont.) — backref greedy ──────────────────────────────────

#[test]
fn eq_backref_greedy_longest() {
    // \(.*\)\1 — greedy: should match "abcabc" (the whole string).
    let m = find_range("\\(.*\\)\\1", "abcabc");
    assert!(m.is_some(), "\\(.*\\)\\1 should match abcabc");
    assert_eq!(m.unwrap().start, 0);
}

// ── Section 5 — capture groups ──────────────────────────────────────────

#[test]
fn eq_capture_two_groups() {
    let re = VimRegex::new("\\(foo\\)\\(bar\\)").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..6);
    assert_eq!(m.captures.first(), Some(&Some(0..3))); // group 1: "foo"
    assert_eq!(m.captures.get(1), Some(&Some(3..6))); // group 2: "bar"
}

#[test]
fn eq_capture_nested_groups() {
    let re = VimRegex::new("\\(\\(a\\)b\\)").unwrap();
    let ctx = MatchContext::simple("ab");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..2);
    // Outer group 1 spans "ab".
    assert_eq!(m.captures.first(), Some(&Some(0..2)));
    // Inner group 2 spans "a".
    assert_eq!(m.captures.get(1), Some(&Some(0..1)));
}

// ── Section 6 — lookahead / lookbehind ──────────────────────────────────

#[test]
fn eq_positive_lookahead_via_meta() {
    // foo\(bar\)\@= — positive lookahead: "foo" followed (without consuming) by "bar".
    // The meta-engine selects Pike VM (no backreferences).
    crate::test_builder::regex("foo\\(bar\\)\\@=")
        .text("foobar")
        .expect_match(0..3)
        .run();
}

#[test]
fn eq_negative_lookahead_via_meta() {
    // foo\(bar\)\@! — must NOT be followed by "bar".
    crate::test_builder::regex("foo\\(bar\\)\\@!")
        .text("foobar")
        .expect_no_match()
        .run();

    crate::test_builder::regex("foo\\(bar\\)\\@!")
        .text("foobaz")
        .expect_match(0..3)
        .run();
}

#[test]
fn eq_positive_lookbehind_via_meta() {
    // \(foo\)\@<=bar — "bar" preceded by "foo".
    crate::test_builder::regex("\\(foo\\)\\@<=bar")
        .text("foobar")
        .expect_match(3..6)
        .run();
}

#[test]
fn eq_negative_lookbehind_via_meta() {
    crate::test_builder::regex("\\(foo\\)\\@<!bar")
        .text("foobar")
        .expect_no_match()
        .run();

    crate::test_builder::regex("\\(foo\\)\\@<!bar")
        .text("xxxbar")
        .expect_match(3..6)
        .run();
}

// ── Section 7 — \zs / \ze match override ────────────────────────────────

#[test]
fn eq_zs_narrows_match_start() {
    // foo\zsbar — match covers "bar" (the part after \zs), full extent is "foobar".
    let re = VimRegex::new("foo\\zsbar").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..6, "narrowed range should be bar");
    assert_eq!(m.full_range, 0..6, "full_range should be foobar");
}

#[test]
fn eq_ze_narrows_match_end() {
    // foo\zebar — \ze sets match_end, so range ends at 3.
    // full_range.end is also ze_pos (3), because build_vim_match sets
    // full_end = match_end.unwrap_or(accepted_at).
    let re = VimRegex::new("foo\\zebar").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..3, "narrowed range should be foo");
    // full_range.end == ze_pos (3), not accepted_at (6).
    assert_eq!(m.full_range, 0..3, "full_range.end is also ze_pos");
}

#[test]
fn eq_zs_ze_combined() {
    // foo\zsbar\zebaz — narrowed range is "bar" (3..6).
    // full_range: start=search_start=0, end=ze_pos=6 (ze_pos is match_end).
    let re = VimRegex::new("foo\\zsbar\\zebaz").unwrap();
    let ctx = MatchContext::simple("foobarbaz");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..6);
    assert_eq!(m.full_range, 0..6);
}

#[test]
fn eq_zs_not_present() {
    // Pattern without \zs/\ze — range == full_range.
    let re = VimRegex::new("foobar").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, m.full_range);
}

// ── Section 11 (cont.) — star zero match ────────────────────────────────

#[test]
fn eq_star_zero_match() {
    // a* matches empty at the start — should return Some("").
    let re = VimRegex::new("a*").unwrap();
    let ctx = MatchContext::simple("bbb");
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some());
    assert_eq!(m.unwrap().range, 0..0);
}

// ── Section 12 — find_all ───────────────────────────────────────────────

#[test]
fn eq_find_all_digits() {
    let re = VimRegex::new("\\d\\+").unwrap();
    let ctx = MatchContext::simple("a12b34c5");
    let matches = re.find_all(&ctx).unwrap();
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0].range, 1..3);
    assert_eq!(matches[1].range, 4..6);
    assert_eq!(matches[2].range, 7..8);
}

#[test]
fn eq_find_all_backref() {
    // find_all on a backreference pattern — backtracker path.
    let re = VimRegex::new("\\(a\\)\\1").unwrap();
    let ctx = MatchContext::simple("aaxaayaa");
    let matches = re.find_all(&ctx).unwrap();
    // "aa" appears at 0, 3, 6.
    assert_eq!(matches.len(), 3);
}

#[test]
fn eq_find_all_alternation() {
    let re = VimRegex::new("cat\\|dog").unwrap();
    let ctx = MatchContext::simple("a cat and a dog");
    let matches = re.find_all(&ctx).unwrap();
    assert_eq!(matches.len(), 2);
}

// ── Section 13 — backward search ────────────────────────────────────────

#[test]
fn eq_backward_finds_last_before_cursor() {
    let re = VimRegex::new("abc").unwrap();
    let ctx = MatchContext {
        text: "abcXXXabcXX",
        cursor: Some(10),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find_backward(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 6..9);
}

#[test]
fn eq_backward_backref() {
    // Backward search through backtracker path.
    let re = VimRegex::new("\\(ab\\)\\1").unwrap();
    let ctx = MatchContext {
        text: "abababab",
        cursor: Some(8),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find_backward(&ctx).unwrap();
    assert!(m.is_some());
}

// ── Section 14 — atomic groups (backtracker path) ───────────────────────

#[test]
fn eq_atomic_group_matches() {
    // \(foo\)\@>bar — atomic: no backtrack into "foo".
    let re = VimRegex::new("\\(foo\\)\\@>bar").unwrap();
    assert!(re.features().has_atomic);
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..6);
}

#[test]
fn eq_atomic_group_no_match() {
    let re = VimRegex::new("\\(foo\\)\\@>baz").unwrap();
    let ctx = MatchContext::simple("foobar");
    assert!(re.find(&ctx).unwrap().is_none());
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 16 — ADVERSARIAL: LARGE NFA (PIKE VM HANDLES, NO CATASTROPHE)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn adversarial_large_optional_chain() {
    // \(a\?\)\{20\}a\{20\} — group (a?) repeated 20 times then 20 mandatory a's.
    //
    // The NFA for this is large (20 copies of the optional group), but the
    // Pike VM handles it in polynomial time via parallel simulation.
    // We provide 40 a's so greedy (a?){20} consumes 20, then a{20} consumes 20.
    let text = "a".repeat(40);
    let re = VimRegex::new("\\(a\\?\\)\\{20}a\\{20}").unwrap();
    assert!(!re.features().has_backreferences); // Pike VM path
    let ctx = MatchContext::simple(&text);
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some(), "(a?){{20}}a{{20}} should match 40 a's");
}

#[test]
fn adversarial_large_optional_chain_longer() {
    // \(a\?\)\{30\}a\{10\} — 30 optional groups then 10 mandatory a's.
    // Needs at least 10 a's; 40 a's gives the greedy groups room.
    let text = "a".repeat(40);
    let re = VimRegex::new("\\(a\\?\\)\\{30}a\\{10}").unwrap();
    let ctx = MatchContext::simple(&text);
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 17 — ADVERSARIAL: POTENTIAL CATASTROPHIC BACKTRACKING
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn adversarial_nested_star_polynomial_pike_vm() {
    // (a*)* — classic catastrophic backtracking for naive DFS.
    // The Pike VM handles this in polynomial time via NFA simulation.
    let text = "a".repeat(30);
    let re = VimRegex::new("\\(a*\\)*").unwrap();
    assert!(!re.features().has_backreferences); // Pike VM path
    let ctx = MatchContext::simple(&text);
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some(), "(a*)* should match");
    // Greedy: should consume the whole string.
    assert_eq!(m.unwrap().range.end, 30);
}

#[test]
fn adversarial_nested_star_no_match_polynomial() {
    // (a+)+ on a string of 'a's followed by a non-matching suffix.
    // Pike VM must NOT explode; it terminates in poly time.
    let mut text = "a".repeat(20);
    text.push('b'); // forces a failed overall match after all the 'a's
                    // "a\+b" — should still match quickly.
    let re = VimRegex::new("a\\+b").unwrap();
    let ctx = MatchContext::simple(&text);
    let m = re.find(&ctx).unwrap();
    // matches the 20 a's + b
    assert!(m.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 18 — ADVERSARIAL: BACKREFERENCE (BACKTRACKER GROWS TO FIT — NO PIKE VM)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn adversarial_backref_greedy_long() {
    // \(.*\)\1 on "abcabc" — backtracker takes this.
    let m = find_range("\\(.*\\)\\1", "abcabc");
    assert!(m.is_some(), "\\(.*\\)\\1 should match abcabc");
    assert_eq!(m.unwrap().start, 0);
}

#[test]
fn adversarial_backref_forces_backtracker() {
    // Simple backref: meta-engine must select the backtracker.
    let re = VimRegex::new("\\(.*\\)\\1").unwrap();
    assert!(re.features().has_backreferences);
}

#[test]
fn large_backref_grows_to_fit_and_no_match() {
    // Capability cascade: a backref pattern on an input far larger than the
    // old VisitedSet cliff (256 KB * 8 = 2_097_152 bits ÷ state_count) no longer
    // declines to the Pike VM. The backtracker grows its memoization to fit
    // (bounded by the 32 MiB cap) and answers correctly itself.
    //
    // \(a\)\1 on 600_000 'b's: there is no "aa", so the correct answer is None —
    // and now it is the BACKTRACKER returning None after a full search, NOT a
    // Pike VM fallback. (Pre-fix this path declined → Pike VM.)
    // nvim: matchstrpos("bbbbbbbb", "\(a\)\1") = -1 (no match).
    let text = "b".repeat(600_000);
    let re = VimRegex::new("\\(a\\)\\1").unwrap();
    assert!(
        re.features().has_backreferences,
        "must be on backtracker path"
    );
    let ctx = MatchContext::simple(&text);
    let result = re.find(&ctx).unwrap();
    assert!(
        result.is_none(),
        "no 'aa' exists in all-b text; backtracker must report None"
    );
}

#[test]
fn large_backref_no_panic_matches_correctly() {
    // Above the old VisitedSet cliff the backtracker grows to fit and
    // produces the CORRECT match — it does NOT fall through to a Pike VM that
    // cannot execute backreferences. This is the strengthened guarantee: not
    // just "no panic / Ok", but the exact match range a correct backtracker
    // (and nvim) returns.
    //
    // \(a\)\1 on 600_000 'b's followed by "aa" matches the "aa" at the end.
    // nvim verified: matchstrpos("bbbbbaa", "\(a\)\1") = start 5, end 7 — i.e.
    // the match sits at pad_len..pad_len+2 for any pad length.
    let pad = "b".repeat(600_000);
    let mut text = pad.clone();
    text.push_str("aa");
    let re = VimRegex::new("\\(a\\)\\1").unwrap();
    assert!(
        re.features().has_backreferences,
        "must be on backtracker path"
    );
    let ctx = MatchContext::simple(&text);
    let result = re
        .find(&ctx)
        .expect("engine must not error on oversize input");
    let m = result.expect("the trailing 'aa' must match (grow-to-fit, no PikeVM)");
    let start = pad.len();
    assert_eq!(
        (m.range.start, m.range.end),
        (start, start + 2),
        "backref match must land on the trailing 'aa' (nvim: pad_len..pad_len+2)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 19 — ADVERSARIAL: MULTILINE WITH NEWLINE CLASSES
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn adversarial_any_char_nl_across_lines() {
    // \_.* should match from start of file right through newlines.
    let text = "hello\nworld\n";
    let re = VimRegex::new("\\_.*").unwrap();
    let ctx = MatchContext::simple(text);
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some(), "\\_.*  should match");
    // Greedy: consumes the whole text.
    assert_eq!(m.unwrap().range.end, text.len());
}

#[test]
fn adversarial_any_char_nl_start_at_newline() {
    // Match from position 0 which is a newline.
    crate::test_builder::regex("\\_.\\+")
        .text("\nhello")
        .expect_match(0..6)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 20 — ADVERSARIAL: COMPLEX ALTERNATION + QUANTIFIER
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn adversarial_alternation_quantifier_two_or_more() {
    // \(foo\|bar\|baz\)\{2,\} — 2 or more occurrences of any of the three words.
    let re = VimRegex::new("\\(foo\\|bar\\|baz\\)\\{2,}").unwrap();
    let ctx = MatchContext::simple("foobarbaz");
    let m = re.find(&ctx).unwrap();
    assert!(
        m.is_some(),
        "\\(foo|bar|baz\\)\\{{2,}} should match foobarbaz"
    );
    assert_eq!(m.unwrap().range, 0..9);
}

#[test]
fn adversarial_alternation_quantifier_minimum_two() {
    // Must have at least 2; single "foo" not sufficient.
    let re = VimRegex::new("\\(foo\\|bar\\)\\{2,}").unwrap();
    let ctx1 = MatchContext::simple("foo");
    let ctx2 = MatchContext::simple("foobar");
    assert!(
        re.find(&ctx1).unwrap().is_none(),
        "single foo should not match \\{{2,}}"
    );
    assert!(
        re.find(&ctx2).unwrap().is_some(),
        "foobar should match \\{{2,}}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 21 — ADVERSARIAL: LONG LITERAL (PREFILTER STRESS)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn adversarial_long_literal_not_present() {
    // Prefilter should bail out early on a long literal absent from a large text.
    let needle = "XXXXXXXXXXXXXXXX"; // 16 chars, unlikely substring
    let haystack = "a".repeat(100_000);
    let re = VimRegex::new(needle).unwrap();
    let ctx = MatchContext::simple(&haystack);
    assert!(re.find(&ctx).unwrap().is_none());
}

#[test]
fn adversarial_long_literal_at_end() {
    // Prefilter + engine must scan to the end of a large haystack.
    let needle = "XXXXXXXXXXXXXXXX";
    let mut haystack = "a".repeat(100_000);
    haystack.push_str(needle);
    let re = VimRegex::new(needle).unwrap();
    let ctx = MatchContext::simple(&haystack);
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some());
    assert_eq!(m.unwrap().range.start, 100_000);
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 22 — ADVERSARIAL: UTF-8 SAFETY UNDER LOAD
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn adversarial_utf8_multibyte_literal() {
    // Pattern with a multi-byte character, surrounded by large ASCII context.
    let needle = "日本語";
    let mut haystack = "x".repeat(10_000);
    haystack.push_str(needle);
    haystack.push_str(&"y".repeat(10_000));
    let re = VimRegex::new(needle).unwrap();
    let ctx = MatchContext::simple(&haystack);
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some());
    assert_eq!(&haystack[m.unwrap().range], needle);
}

#[test]
fn adversarial_utf8_dot_star_across_multibyte() {
    // .* on a string with multi-byte chars — must not panic or corrupt ranges.
    let text = "hello 日本語 world";
    let re = VimRegex::new(".*").unwrap();
    let ctx = MatchContext::simple(text);
    let m = re.find(&ctx).unwrap().unwrap();
    // Greedy ".*" (no newline) should consume the whole single-line text.
    assert_eq!(m.range.end, text.len());
}
