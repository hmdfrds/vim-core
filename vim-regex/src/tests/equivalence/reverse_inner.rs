//! ReverseInner strategy: correctness, and equivalence with the
//! unaccelerated engine.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

// ═══════════════════════════════════════════════════════════════════════════════
// TABULAR TESTS — simple find assertions
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(ri_basic {
    // Basic correctness
    word_at_word:             "\\w\\+@\\w\\+",  "user@host"  => (0, 9);
    dot_literal:              ".foo",            "xxfoo"      => (1, 5);
    first_occurrence:         ".foo",            "afoo bfoo"  => (0, 4);
    no_match:                 ".foo",            "bar baz"    => ();
    literal_at_start:         ".foo",            "xfoo"       => (0, 4);
    multiple_leftmost:        ".foo",            "afoobfoo"   => (0, 4);
});

crate::test_harness::regex_suite!(ri_equiv {
    // Equivalence with fallback path
    digit_plus_literal:       "\\d\\+world",    "abc 42world xyz"   => (4, 11);
    word_dot_literal:         "\\w.test",        "a_test and b test" => (0, 6);
    quantifier_inner:         "a\\{2,5}test",    "xaaatest"          => (1, 8);
    find_all:                 ".foo",            "afoo bfoo cfoo"    => all[(0, 4), (5, 9), (10, 14)];
});

crate::test_harness::regex_suite!(ri_edge_cases {
    // Edge cases
    unicode_inner:            ".cafe",           "\u{2615}cafe"  => (0, 7);
    inner_in_group:           ".\\(hello\\)",    "xhello"        => (0, 6);
    inner_not_at_end:         ".hello.",          "xhelloy"       => (0, 7);
    empty_text:               ".foo",             ""              => ();
    shorter_than_literal:     ".foo",             "fo"            => ();
    dot_plus_greedy:          ".\\+test",         "a test b test end" => (0, 13);
});

crate::test_harness::regex_suite!(ri_ineligible {
    // Ineligibility (still correct via fallback)
    case_insensitive:         "\\c.foo",         "xFOO"      => (0, 4);
    no_inner_literal:         ".*\\d\\+",        "abc 123"   => (0, 7);
});

// ═══════════════════════════════════════════════════════════════════════════════
// NON-TABULAR TESTS — require has_prefix_reverse_nfa, captures, cursors, etc.
// ═══════════════════════════════════════════════════════════════════════════════

// ── Section 1: Construction verification ────────────────────────────────

#[test]
fn phase4c_constructed_for_eligible_pattern() {
    // After auto-possessification, \w\+@\w\+ gets wrapped in Atomic
    // (because \w is disjoint from '@'), routing to the Backtracker.
    // The Backtracker doesn't use reverse NFA strategies.
    // Use a pattern that won't be possessified: .+@\w\+ — the dot
    // matches '@' too, so the body charset overlaps the successor and
    // possessification is skipped.
    let re = VimRegex::new(".\\+@\\w\\+").unwrap();
    assert!(
        re.has_prefix_reverse_nfa(),
        ".+@\\w+ should have prefix_reverse_nfa"
    );
}

#[test]
fn phase4c_not_constructed_with_suffix_literal() {
    let re = VimRegex::new("\\w\\+world").unwrap();
    assert!(
        !re.has_prefix_reverse_nfa(),
        "\\w+world has suffix 'world', the ReverseSuffix strategy handles it"
    );
}

#[test]
fn phase4c_not_constructed_for_pure_literal() {
    let re = VimRegex::new("hello").unwrap();
    assert!(
        !re.has_prefix_reverse_nfa(),
        "pure literal handled by the LiteralBypass strategy"
    );
}

#[test]
fn phase4c_not_constructed_for_backref() {
    let re = VimRegex::new("\\(\\w\\+\\)@\\1").unwrap();
    assert!(
        !re.has_prefix_reverse_nfa(),
        "backreference forces backtracker"
    );
}

// ── Section 3: Equivalence — capture groups ─────────────────────────────

#[test]
fn ri_equiv_group_inner_literal() {
    let re = VimRegex::new("\\(x\\).*end").expect("valid");
    let ctx = MatchContext::simple("start x middle end finish");
    let m = re.find(&ctx).expect("no error").expect("should match");
    assert_eq!(&ctx.text[m.range.clone()], "x middle end");
    assert_eq!(
        m.captures
            .first()
            .unwrap()
            .as_ref()
            .map(|r| &ctx.text[r.clone()]),
        Some("x")
    );
}

// ── Section 5: Backward search / find_at ────────────────────────────────

#[test]
fn ri_backward_search_still_works() {
    let re = VimRegex::new(".foo").unwrap();
    let ctx = MatchContext {
        text: "afoo bfoo cfoo",
        cursor: Some(10),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find_backward(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 5..9);
}

#[test]
fn ri_captures_preserved() {
    let re = VimRegex::new("\\(\\w\\+\\).*needle").unwrap();
    let ctx = MatchContext::simple("hello world needle");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.clone()], "hello world needle");
    let cap1 = m.captures.first().unwrap().as_ref().unwrap();
    assert_eq!(&ctx.text[cap1.clone()], "hello");
}

#[test]
fn ri_find_from_midway() {
    let re = VimRegex::new(".foo").unwrap();
    let ctx = MatchContext::simple("afoo bfoo cfoo");
    let m = re.find_at(&ctx, 5).unwrap().unwrap();
    assert_eq!(m.range, 5..9);
}

// ── Section 6: Budget exhaustion and degenerate prefix ──────────────────

#[test]
fn ri_budget_exhaustion_still_finds_match() {
    // Text with many "foo" occurrences where the first 33+ have no valid
    // single-char prefix. ReverseInner exhausts its budget and falls through
    // to the full engine dispatch, which finds the match.
    let mut text = "foo ".repeat(40); // 40 occurrences, none with single-char prefix right before
    text.push_str("xfoo");
    // The leftmost match is actually " foo" at position 3 (space + foo)
    // because .foo matches at the very first " foo" boundary.
    let re = VimRegex::new(".foo").unwrap();
    let ctx = MatchContext::simple(&text);
    let m = re.find(&ctx).unwrap();
    assert!(
        m.is_some(),
        "should find a match even if the ReverseInner budget is exhausted"
    );
}

#[test]
fn phase4c_degenerate_single_literal_prefix_rejected() {
    // Pattern "a\w\+" has prefix "a" — a single literal compiles to
    // NFA with state_count <= 2 (start → literal → accept for reverse).
    // ReverseInner should reject this as degenerate.
    let re = VimRegex::new("a\\w\\+").unwrap();
    assert!(
        !re.has_prefix_reverse_nfa(),
        "single-literal prefix NFA should be rejected (state_count <= 2)"
    );
}
