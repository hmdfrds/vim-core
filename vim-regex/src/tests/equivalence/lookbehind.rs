//! Deferred lookbehind: correctness, and equivalence with the
//! unaccelerated engine.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

// ═══════════════════════════════════════════════════════════════════════════════
// TABULAR TESTS — simple find assertions
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(lookbehind_basic {
    // Basic lookbehind correctness
    positive_match:           "\\(foo\\)\\@<=bar",     "foobar"   => (3, 6);
    positive_no_match:        "\\(foo\\)\\@<=bar",     "xyzbar"   => ();
    negative_match:           "\\(foo\\)\\@<!bar",     "xyzbar"   => (3, 6);
    negative_fails:           "\\(foo\\)\\@<!bar",     "foobar"   => ();
});

crate::test_harness::regex_suite!(pim_scenarios {
    // PIM-specific scenarios
    with_long_rest:           "\\(foo\\)\\@<=bar",       "foobar"     => (3, 6);
    negative_deferred:        "\\(error\\)\\@<!ok",      "foobarok"   => (6, 8);
});

crate::test_harness::regex_suite!(alternation_convergence {
    // Alternation convergence (SparseSet dedup)
    first_fails:              "\\(foo\\)\\@<=x\\|baz",   "barbaz"  => (3, 6);
    first_succeeds:           "\\(foo\\)\\@<=x\\|baz",   "foox"    => (3, 4);
});

crate::test_harness::regex_suite!(lookbehind_edge_cases {
    // Edge cases
    at_text_start:            "\\(foo\\)\\@<=bar",        "bar"     => ();
    empty_text:               "\\(foo\\)\\@<=bar",        ""        => ();
    multiple_in_text:         "\\(x\\)\\@<=y",            "ay xy by xy" => (4, 5);
    with_word_rest:           "\\(start\\)\\@<=\\w\\+end", "startmiddleend" => (5, 14);
    // Lookbehind at pattern end: zero-width match at position 3
    at_pattern_end:           "\\(foo\\)\\@<=",           "foobar"  => (3, 3);
});

crate::test_harness::regex_suite!(fastpath_regression {
    // Fast-path regression (epsilon chain PIM gate)
    neg_lb_with_capture:      "\\(WRONG\\)\\@<=\\(bar\\)",  "xyzbar"      => ();
    pos_lb_with_capture:      "\\(foo\\)\\@<=\\(bar\\)",    "foobar"      => (3, 6);
    neg_lb_no_capture:        "\\(WRONG\\)\\@<=bar",         "xyzbar"      => ();
    neg_lb_with_group_rest:   "\\(NOPE\\)\\@<=\\(test\\)\\(ing\\)", "footesting" => ();
});

crate::test_harness::regex_suite!(deferral_heuristic {
    // Deferral heuristic boundaries
    short_lb_match:           "\\(ab\\)\\@<=cd",  "abcd"  => (2, 4);
    short_lb_no_match:        "\\(ab\\)\\@<=cd",  "xxcd"  => ();
});

crate::test_harness::regex_suite!(lookbehind_find_all {
    // find_all with lookbehind
    find_all_ab:              "\\(a\\)\\@<=b",     "ab cb ab"  => all[(1, 2), (7, 8)];
});

// ═══════════════════════════════════════════════════════════════════════════════
// NON-TABULAR TESTS — require captures, \zs range, backward cursor, etc.
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn variable_length_lookbehind() {
    let text = format!("{}END", "MARKER".repeat(50));
    crate::test_builder::regex("\\(MARKER.*\\)\\@<=END")
        .text(&text)
        .expect_match((text.len() - 3)..text.len())
        .run();
}

#[test]
fn pim_lookbehind_with_captures() {
    // nvim (magic, -u NONE -i NONE): matchlist("foobar", '\(foo\)\@<=\(bar\)')
    //   -> ['bar', 'foo', 'bar', ...]; matchstrpos -> ['bar', 3, 6].
    // Match is "bar" (3..6); Vim numbers \(foo\) inside the lookbehind as group 1.
    let re = VimRegex::new("\\(foo\\)\\@<=\\(bar\\)").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.clone()], "bar");
    let cap1 = m.captures.first().unwrap().as_ref().unwrap();
    assert_eq!(&ctx.text[cap1.clone()], "foo");
}

#[test]
fn pim_lookbehind_with_zs() {
    let re = VimRegex::new("\\(foo\\)\\@<=\\zsbar").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..6);
}

// ═══════════════════════════════════════════════════════════════════════════════
// PikeVM lookaround bugs (RED: prove bugs; ranges verified against nvim)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn c5_pim_swallow_lower_priority_branch_matches() {
    // nvim: matchstrpos("xb", '\(\(a\)\@<=b\)\|b') -> ['b', 1, 2].
    // The higher-priority lookbehind branch fails on "xb"; the plain "b" branch
    // must still match. PikeVM currently drops it (PIM-swallow).
    crate::test_builder::regex("\\(\\(a\\)\\@<=b\\)\\|b")
        .text("xb")
        .expect_match(1..2)
        .run();
}

#[test]
fn c5_ordered_alt_shorter_higher_priority_wins() {
    // nvim: matchstrpos("abc", '\(a\)\@<=b\|\(a\)\@<=bc') -> ['b', 1, 2].
    // Vim alternation is ORDERED: the first matching alternative wins, NOT the
    // longest. The higher-priority `\(a\)\@<=b` branch accepts "b" at pos 2; the
    // lower-priority `\(a\)\@<=bc` branch must NOT clobber it with "bc" at pos 3.
    crate::test_builder::regex("\\(a\\)\\@<=b\\|\\(a\\)\\@<=bc")
        .text("abc")
        .expect_match(1..2)
        .run();
}

#[test]
fn c5_ordered_alt_pim_vs_plain() {
    // nvim: matchstrpos("abc", '\(a\)\@<=b\|bc') -> ['b', 1, 2].
    // Higher-priority PIM branch `\(a\)\@<=b` wins "b"; the lower-priority plain
    // `bc` branch (accepting later, at pos 3) must not overwrite it.
    crate::test_builder::regex("\\(a\\)\\@<=b\\|bc")
        .text("abc")
        .expect_match(1..2)
        .run();
}

#[test]
fn c5_ordered_alt_longer_lower_priority_loses() {
    // nvim: matchstrpos("xhello", '\(x\)\@<=hel\|\(x\)\@<=hello') -> ['hel', 1, 4].
    // The higher-priority `\(x\)\@<=hel` branch wins the shorter "hel"; the
    // lower-priority `\(x\)\@<=hello` branch must not clobber with the longer match.
    crate::test_builder::regex("\\(x\\)\\@<=hel\\|\\(x\\)\\@<=hello")
        .text("xhello")
        .expect_match(1..4)
        .run();
}

#[test]
fn c5_ordered_alt_non_lookaround_control() {
    // nvim: matchstrpos("ab", 'a\|ab') -> ['a', 0, 1].
    // CONTROL (no lookaround): ordered alternation already works; the higher-
    // priority `a` wins over the longer lower-priority `ab`. Must stay green.
    crate::test_builder::regex("a\\|ab")
        .text("ab")
        .expect_match(0..1)
        .run();
}

#[test]
fn c5_greedy_lookbehind_extends_xa() {
    // nvim: matchstrpos("xa", '\(x\)\@<=\w\+') -> ['a', 1, 2].
    // Greedy regression guard: the SAME winning thread's loop-back must still be
    // able to extend (priority truncation keeps higher-priority loop-backs alive).
    crate::test_builder::regex("\\(x\\)\\@<=\\w\\+")
        .text("xa")
        .expect_match(1..2)
        .run();
}

#[test]
fn c5_greedy_lookbehind_alt_extends() {
    // nvim: matchstrpos("xabbb", '\(x\)\@<=ab\+\|\(x\)\@<=ac') -> ['abbb', 1, 5].
    // Greedy + alternation + lookaround: the higher-priority `ab\+` branch greedily
    // extends to "abbb" (its own loop-back survives truncation).
    crate::test_builder::regex("\\(x\\)\\@<=ab\\+\\|\\(x\\)\\@<=ac")
        .text("xabbb")
        .expect_match(1..5)
        .run();
}

#[test]
fn c5_greedy_lookbehind_alt_second_branch() {
    // nvim: matchstrpos("xac", '\(x\)\@<=ab\+\|\(x\)\@<=ac') -> ['ac', 1, 3].
    // When the higher-priority `ab\+` branch fails (no 'b' after 'a'), the
    // lower-priority `ac` branch wins.
    crate::test_builder::regex("\\(x\\)\\@<=ab\\+\\|\\(x\\)\\@<=ac")
        .text("xac")
        .expect_match(1..3)
        .run();
}

#[test]
fn c5_positive_lookahead_capture_exported() {
    // nvim: matchstrpos("abc", '\(ab\)\@=ab') -> ['ab', 0, 2];
    //   matchlist -> ['ab', 'ab', ...] (group 1 = "ab" inside the lookahead).
    crate::test_builder::regex("\\(ab\\)\\@=ab")
        .text("abc")
        .expect_match(0..2)
        .expect_capture(1, 0..2)
        .run();
}

#[test]
fn c5_negative_lookbehind_no_capture() {
    // nvim: matchstrpos("zy", '\(x\)\@<!y') -> ['y', 1, 2]; no participating group.
    crate::test_builder::regex("\\(x\\)\\@<!y")
        .text("zy")
        .expect_match(1..2)
        .run();
}

#[test]
fn c5_positive_lookahead_no_stale_capture_on_failed_position() {
    // nvim: matchstrpos("xabab", '\(ab\)\@=ab') -> ['ab', 1, 3];
    //   matchlist -> ['ab', 'ab'] (group 1 = "ab" at 1..3).
    // The lookahead FAILS at pos 0 (x != a) and succeeds at pos 1. The exported
    // capture must be the one from the SUCCESSFUL position (1..3), never a stale
    // sub-capture from the failed attempt. (Merge only on success.)
    crate::test_builder::regex("\\(ab\\)\\@=ab")
        .text("xabab")
        .expect_match(1..3)
        .expect_capture(1, 1..3)
        .run();
}

#[test]
fn c5_negative_lookahead_inner_group_does_not_leak() {
    // nvim: matchstrpos("zy", '\(x\)\@!\(y\)') -> ['y', 1, 2];
    //   matchlist -> ['y', '', 'y'] — group 1 (inside the NEGATIVE lookahead) does
    //   NOT participate; group 2 (the consuming \(y\) outside the assertion) = "y".
    // A passing negative lookahead exports nothing from its inner group.
    let re = crate::engine::VimRegex::new("\\(x\\)\\@!\\(y\\)").unwrap();
    let ctx = MatchContext::simple("zy");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 1..2);
    // Group 1 (inside the negative lookahead) must not leak a capture.
    assert_eq!(
        m.capture(1),
        None,
        "negative lookahead inner group must not leak"
    );
    // Group 2 (the consuming \(y\)) is "y" at 1..2.
    assert_eq!(
        m.capture(2),
        Some(&(1..2)),
        "consuming group 2 should be \"y\""
    );
}

#[test]
fn lookbehind_backward_search() {
    let re = VimRegex::new("\\(foo\\)\\@<=bar").unwrap();
    let ctx = MatchContext {
        text: "foobar foobar",
        cursor: Some(13),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find_backward(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 10..13);
}
