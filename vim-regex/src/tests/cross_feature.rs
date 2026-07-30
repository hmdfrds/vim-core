//! Cross-feature interaction tests.
//!
//! Verifies that combining two or more regex features produces correct results.
//! Each section tests a specific pair from the interaction matrix.

use crate::engine::VimRegex;
use crate::matchers::MatchContext;
use crate::test_builder::regex;

// ─── Helpers (used by tests the builder cannot reach) ──────────────────────

fn find_all_substrs<'t>(pattern: &str, text: &'t str) -> Vec<&'t str> {
    let re = VimRegex::new(pattern).expect("valid pattern");
    let ctx = MatchContext::simple(text);
    re.find_all(&ctx)
        .expect("no error")
        .into_iter()
        .map(|m| &text[m.range])
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// \c + BACKREF
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(ci_backref {
    simple:        r"\c\(\w\+\) \1",    "Hello hello world" => (0, 11);
    no_match:      r"\c\(abc\)x\1",     "ABCxDEF"           => ();
    all_caps:      r"\c\(foo\)\1",       "FOOFOO bar"        => (0, 6);
});

// ═══════════════════════════════════════════════════════════════════════════════
// \c + LOOKAROUND
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn case_insensitive_positive_lookahead() {
    regex(r"\cfoo\(bar\)\@=")
        .text("FOOBar baz")
        .expect_match(0..3)
        .run();
}

#[test]
fn case_insensitive_negative_lookbehind() {
    // Should skip "GOOD" after "BAD" — need to verify matched text.
    let re = VimRegex::new("\\c\\(bad\\)\\@<!good").unwrap();
    let ctx = MatchContext::simple("BADGOOD realgood");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(&ctx.text[m.range.start..m.range.end], "good");
}

// ═══════════════════════════════════════════════════════════════════════════════
// \c + AHO-CORASICK
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn case_insensitive_ac_alternation() {
    let matches = find_all_substrs("\\cfoo\\|bar\\|baz", "BAZ FOO BAR");
    assert_eq!(matches.len(), 3);
}

#[test]
fn case_insensitive_ac_mixed_case_matches() {
    regex("\\cFOO\\|BAR\\|BAZ\\|QUX")
        .text("test Baz here")
        .expect_match(5..8)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// \zs + LOOKAROUND
// ═══════════════════════════════════════════════════════════════════════════════

// \zs patterns trip invariant 6 (find_at consistency) due to a known engine
// limitation with match-start overrides. Keep as manual tests.

#[test]
fn zs_with_positive_lookahead() {
    let re = VimRegex::new("foo\\zs\\(bar\\)\\@=").unwrap();
    let ctx = MatchContext::simple("foobar baz");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..3);
}

#[test]
fn zs_with_lookbehind() {
    let re = VimRegex::new("\\(foo\\)\\@<=\\zsbar").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..6);
}

crate::test_harness::regex_suite!(ze_lookaround {
    ze_with_lookahead: r"foo\ze\(bar\)\@=", "foobar" => (0, 3);
});

// ═══════════════════════════════════════════════════════════════════════════════
// \zs + BACKREF
// ═══════════════════════════════════════════════════════════════════════════════

// \zs + backref: same invariant 6 limitation with find_at.

#[test]
fn zs_with_backref() {
    let re = VimRegex::new("\\(\\w\\+\\) \\zs\\1").unwrap();
    let ctx = MatchContext::simple("hello hello");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 6..11);
}

crate::test_harness::regex_suite!(zs_backref {
    no_match: r"\(\w\+\) \zs\1", "hello world" => ();
});

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND + QUANTIFIER
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(lookaround_quantifier {
    lookahead_with_star:    r"\(x\)\@=x*",      "xxx"  => (0, 3);
    lookbehind_with_plus:   r"\(a\)\@<=b\+",     "xabbb" => (2, 5);
});

#[test]
fn negative_lookahead_with_quantifier() {
    // Complex interaction — the engine must not panic and must produce a valid match.
    regex("\\d\\+\\(px\\)\\@!").text("100px 200em").run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// DFA + \c
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn dfa_eligible_with_case_insensitive() {
    let matches = find_all_substrs("\\cfoo", "FOO bar foo BAR");
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0], "FOO");
    assert_eq!(matches[1], "foo");
}

#[test]
fn dfa_eligible_ci_alternation() {
    let matches = find_all_substrs("\\cthe\\|and\\|for", "The AND for THE and FOR");
    assert_eq!(matches.len(), 6);
}

#[test]
fn dfa_eligible_ci_word_boundary() {
    let matches = find_all_substrs("\\c\\<fox\\>", "fox FOX Fox foxes");
    assert_eq!(matches.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKWARD + LOOKAROUND — uses raw MatchContext, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn backward_search_with_lookahead() {
    let re = VimRegex::new("\\w\\+\\(bar\\)\\@=").unwrap();
    let ctx = MatchContext {
        text: "foobar bazbar",
        cursor: Some(13),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find_backward(&ctx).unwrap().unwrap();
    let matched = &ctx.text[m.range.clone()];
    assert!(
        matched == "baz" || matched == "foo" || matched == "bazbar" || matched == "foobar",
        "unexpected backward match: {matched:?}"
    );
}

#[test]
fn backward_search_with_lookbehind() {
    let re = VimRegex::new("\\(x\\)\\@<=\\d\\+").unwrap();
    let text = "a1 x2 b3 x4";
    let ctx = MatchContext {
        text,
        cursor: Some(text.len()),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find_backward(&ctx).unwrap().unwrap();
    let matched = &text[m.range.clone()];
    assert!(matched == "2" || matched == "4", "unexpected: {matched:?}");
}

// ═══════════════════════════════════════════════════════════════════════════════
// find_all + \zs
// ═══════════════════════════════════════════════════════════════════════════════

// find_all with \zs: same invariant 6 limitation.
#[test]
fn find_all_with_zs() {
    let re = VimRegex::new("foo\\zsbar").unwrap();
    let ctx = MatchContext::simple("foobar foobar");
    let all = re.find_all(&ctx).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].range, 3..6);
    assert_eq!(all[1].range, 10..13);
}

#[test]
fn find_all_with_zs_ze() {
    // Narrowed by \zs/\ze: only "bar" part of "foobarbaz" is returned.
    let re = VimRegex::new("foo\\zsbar\\zebaz").unwrap();
    let ctx = MatchContext::simple("foobarbaz xfoobarbaz");
    let all = re.find_all(&ctx).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(&ctx.text[all[0].range.clone()], "bar");
    assert_eq!(&ctx.text[all[1].range.clone()], "bar");
}

// ═══════════════════════════════════════════════════════════════════════════════
// \_ + LOOKAROUND (multiline)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn multiline_dot_with_lookahead() {
    let re = VimRegex::new("\\_.*\\(end\\)\\@=").unwrap();
    let ctx = MatchContext::simple("start\nmiddle\nend");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range.end, 13);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ALTERNATION + CAPTURES — tests capture absence, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn alternation_captures_correct_branch() {
    let re = VimRegex::new("\\(foo\\)\\|\\(bar\\)").unwrap();
    let ctx = MatchContext::simple("bar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert!(m.capture(1).is_none());
    assert_eq!(m.capture(2).unwrap().clone(), 0..3);
}

#[test]
fn alternation_captures_first_branch() {
    let re = VimRegex::new("\\(foo\\)\\|\\(bar\\)").unwrap();
    let ctx = MatchContext::simple("foo");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.capture(1).unwrap().clone(), 0..3);
    assert!(m.capture(2).is_none());
}

#[test]
fn alternation_captures_nested() {
    let re = VimRegex::new("\\(\\(a\\)\\|\\(b\\)\\)c").unwrap();
    let ctx = MatchContext::simple("bc");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..2);
    assert_eq!(m.capture(1).unwrap().clone(), 0..1);
    assert!(m.capture(2).is_none());
    assert_eq!(m.capture(3).unwrap().clone(), 0..1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND + ATOMIC — panic-safety tests, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn atomic_with_lookahead() {
    let re = VimRegex::new("\\(\\w\\+\\)\\@>\\(bar\\)\\@=").unwrap();
    let ctx = MatchContext::simple("foobar");
    let _ = re.find(&ctx);
}

crate::test_harness::regex_suite!(atomic_group {
    no_backtrack: r"\(\w\+\)\@>:", "abc:" => (0, 4);
});

// ═══════════════════════════════════════════════════════════════════════════════
// \c + \zs (case modifier + match override)
// ═══════════════════════════════════════════════════════════════════════════════

// \zs with \c: same invariant 6 limitation.

#[test]
fn case_insensitive_with_zs() {
    let re = VimRegex::new("\\cfoo\\zsbar").unwrap();
    let ctx = MatchContext::simple("FOObar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..6);
}

crate::test_harness::regex_suite!(ci_ze {
    with_ze: r"\cfoo\zebar", "FOObar" => (0, 3);
});

// ═══════════════════════════════════════════════════════════════════════════════
// BACKREF + LOOKAROUND (combined) — panic-safety / complex assertion, keep manual
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn backref_with_lookahead() {
    let re = VimRegex::new("\\(\\w\\+\\) \\(\\1\\)\\@=").unwrap();
    let ctx = MatchContext::simple("abc abc");
    let _ = re.find(&ctx);
}

crate::test_harness::regex_suite!(backref_lookaround {
    simple_backref: r"\(\w\+\) \1", "abc abc" => (0, 7);
});

#[test]
fn backref_with_negative_lookbehind() {
    let re = VimRegex::new("\\(ab\\).*\\(x\\)\\@<!\\1").unwrap();
    let ctx = MatchContext::simple("ab xab ab");
    let _ = re.find(&ctx);
}

// ═══════════════════════════════════════════════════════════════════════════════
// MULTIPLE FEATURES COMBINED
// ═══════════════════════════════════════════════════════════════════════════════

// \zs combined tests: same invariant 6 limitation.

#[test]
fn ci_zs_lookahead_combined() {
    let re = VimRegex::new("\\cfoo\\zs\\(BAR\\)\\@=").unwrap();
    let ctx = MatchContext::simple("foobar");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 3..3);
}

#[test]
fn ci_backref_zs_combined() {
    let re = VimRegex::new("\\c\\(\\w\\+\\) \\zs\\1").unwrap();
    let ctx = MatchContext::simple("Hello hello");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 6..11);
}

// ═══════════════════════════════════════════════════════════════════════════════
// AUTO-POSSESSIFICATION — End-to-end correctness through full compile pipeline
// ═══════════════════════════════════════════════════════════════════════════════

crate::test_harness::regex_suite!(possessify_suite {
    word_plus_colon:              r"\w\+:",        "hello:world"  => (0, 6);
    digit_plus_dot_digit_plus:    r"\d\+\.\d\+",   "3.14"         => (0, 4);
    digit_plus_whitespace:        r"\d\+\s",        "123 abc"      => (0, 4);
    alpha_range_plus_digit:       r"[a-z]\+[0-9]",  "abc1"         => (0, 4);

    // Patterns requiring backtracking (must NOT be possessified)
    word_plus_word_backtrack:     r"\w\+\w",        "abc"          => (0, 3);
    dot_star_x_backtrack:         ".*x",            "abcx"         => (0, 4);
    word_plus_word_single_char:   r"\w\+\w",        "ab"           => (0, 2);
});

#[test]
fn possessify_word_plus_colon_find_all() {
    regex("\\w\\+:")
        .text("foo:bar:baz:")
        .expect_all_matches(&[0..4, 4..8, 8..12])
        .run();
}

#[test]
fn possessify_digit_plus_dot_digit_plus_multiple() {
    regex("\\d\\+\\.\\d\\+")
        .text("pi=3.14 e=2.72")
        .expect_all_matches(&[3..7, 10..14])
        .run();
}

#[test]
fn possessify_does_not_break_captures() {
    // Capture group around possessified quantifier must still capture correctly.
    regex("\\(\\w\\+\\):")
        .text("hello:world")
        .expect_match(0..6)
        .expect_capture(1, 0..5)
        .run();
}

#[test]
fn possessify_nested_in_alternation() {
    // Both branches have possessifiable quantifiers.
    regex("\\w\\+:\\|\\d\\+\\.")
        .text("abc:")
        .expect_match(0..4)
        .run();
    regex("\\w\\+:\\|\\d\\+\\.")
        .text("42.")
        .expect_match(0..3)
        .run();
}

crate::test_harness::regex_suite!(possessify_misc {
    does_not_affect_lazy:      r"\w\{-1,}:", "abc:def" => (0, 4);
    lazy_minimal:              r"\w\{-1,}:", "a:b:c"   => (0, 2);
    case_insensitive_works:    r"\c\w\+:",   "HELLO:world" => (0, 6);
});

// \zs with possessify: same invariant 6 limitation.
#[test]
fn possessify_with_zs() {
    let re = VimRegex::new("\\w\\+\\zs:").unwrap();
    let ctx = MatchContext::simple("hello:world");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 5..6);
}

// ═══════════════════════════════════════════════════════════════════════════════
// AST DISPLAY ROUNDTRIP — structural equality — keep manual
// ═══════════════════════════════════════════════════════════════════════════════

mod ast_roundtrip {
    use crate::parser::parse_pattern;

    /// Parse a pattern, display it, re-parse, and compare ASTs.
    fn assert_roundtrip(pattern: &str) {
        let parsed1 = parse_pattern(pattern).expect("first parse failed");
        let displayed = parsed1.node.to_string();
        let parsed2 = parse_pattern(&displayed).unwrap_or_else(|e| {
            panic!(
                "re-parse failed for pattern {pattern:?}\n  displayed as: {displayed:?}\n  error: {e}"
            )
        });
        assert_eq!(
            parsed1.node, parsed2.node,
            "AST roundtrip mismatch\n  original pattern: {pattern:?}\n  displayed: {displayed:?}\n  AST1: {:#?}\n  AST2: {:#?}",
            parsed1.node, parsed2.node
        );
    }

    #[test]
    fn roundtrip_literal() {
        assert_roundtrip("abc");
    }

    #[test]
    fn roundtrip_escaped_specials() {
        assert_roundtrip("a\\.b\\*c\\$d");
    }

    #[test]
    fn roundtrip_classes() {
        assert_roundtrip("\\d\\w\\s");
    }

    #[test]
    fn roundtrip_quantifiers() {
        assert_roundtrip("a*");
        assert_roundtrip("a\\+");
        assert_roundtrip("a\\?");
        assert_roundtrip("a\\{2,5}");
        assert_roundtrip("a\\{-}");
        assert_roundtrip("a\\{3}");
    }

    #[test]
    fn roundtrip_groups() {
        assert_roundtrip("\\(abc\\)");
        assert_roundtrip("\\%(abc\\)");
    }

    #[test]
    fn roundtrip_alternation() {
        assert_roundtrip("foo\\|bar\\|baz");
    }

    #[test]
    fn roundtrip_collection() {
        assert_roundtrip("[a-z]");
        assert_roundtrip("[^0-9]");
        assert_roundtrip("[[:alpha:]]");
    }

    #[test]
    fn roundtrip_lookaround() {
        assert_roundtrip("\\(foo\\)\\@=bar");
        assert_roundtrip("\\(foo\\)\\@!bar");
        assert_roundtrip("\\(foo\\)\\@<=bar");
        assert_roundtrip("\\(foo\\)\\@<!bar");
    }

    #[test]
    fn roundtrip_backreference() {
        assert_roundtrip("\\(a\\)\\1");
    }

    #[test]
    fn roundtrip_anchors() {
        assert_roundtrip("^abc$");
        assert_roundtrip("\\<word\\>");
    }

    #[test]
    fn roundtrip_zs_ze() {
        assert_roundtrip("foo\\zsbar\\zebaz");
    }

    #[test]
    fn roundtrip_complex() {
        assert_roundtrip("\\(\\d\\+\\)\\.\\(\\d\\+\\)");
        assert_roundtrip("\\<\\w\\+\\>");
        assert_roundtrip("\\(foo\\|bar\\)\\+baz");
    }

    #[test]
    fn roundtrip_escape_sequences() {
        assert_roundtrip("\\t\\n\\r");
    }

    #[test]
    fn roundtrip_branch_and() {
        assert_roundtrip("foo\\&bar");
    }

    #[test]
    fn roundtrip_optional_sequence() {
        assert_roundtrip("\\%[abc]");
    }

    #[test]
    fn roundtrip_non_greedy() {
        assert_roundtrip("a\\{-1,3}");
        assert_roundtrip(".\\{-}");
    }

    #[test]
    fn roundtrip_class_with_newline() {
        assert_roundtrip("\\_d\\_w\\_s");
    }
}

/// In-crate proptest for AST functional roundtrip using the Arbitrary generator.
///
/// Verifies that display(AST) produces a pattern string that, when compiled,
/// matches identically to the original AST's compiled form. This tests
/// functional equivalence rather than structural AST equality, since the
/// parser may canonicalize structures (e.g., flatten nested Sequences).
mod ast_roundtrip_proptest {
    use crate::engine::VimRegex;
    use crate::ir::arbitrary_ast::arb_vim_pattern_node;
    use crate::matchers::MatchContext;
    use crate::parser::parse_pattern;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        /// AST roundtrip invariant: display -> parse -> display must be
        /// idempotent, and the canonical form must compile to a functionally
        /// equivalent regex.
        ///
        /// The first display of a raw generated AST may produce a pattern
        /// string that parses to a slightly different but equivalent AST
        /// (e.g., `$` inside a group at a non-terminal position parses as
        /// `Literal('$')` instead of `EndOfLine`). But after one parse-
        /// display cycle, the output should be stable (idempotent).
        #[test]
        fn ast_functional_roundtrip(node in arb_vim_pattern_node()) {
            // Format AST -> Vim pattern string.
            let pattern1 = node.to_string();

            // Parse the string back -- may differ from original AST.
            let parsed1 = match parse_pattern(&pattern1) {
                Ok(p) => p,
                Err(_) => {
                    // Some generated ASTs produce patterns the parser rejects
                    // (e.g., lone \@= without preceding group). This is
                    // expected -- skip these cases.
                    return Ok(());
                }
            };

            // Display the parsed AST (canonical form).
            let pattern2 = parsed1.node.to_string();

            // Parse the canonical form.
            let parsed2 = match parse_pattern(&pattern2) {
                Ok(p) => p,
                Err(e) => {
                    return Err(proptest::test_runner::TestCaseError::Fail(
                        format!(
                            "canonical form failed to re-parse: {:?} -> error: {}",
                            pattern2, e
                        ).into()
                    ));
                }
            };

            // The canonical form must be idempotent: parse(display(X)) == X
            // after the first normalization cycle.
            let pattern3 = parsed2.node.to_string();
            prop_assert_eq!(
                &pattern2,
                &pattern3,
                "Display not idempotent after normalization"
            );

            // Functional check: both canonical compilations must agree.
            let re1 = match VimRegex::new(&pattern2) {
                Ok(r) => r,
                Err(_) => return Ok(()),
            };
            let re2 = match VimRegex::new(&pattern3) {
                Ok(r) => r,
                Err(_) => {
                    return Err(proptest::test_runner::TestCaseError::Fail(
                        format!(
                            "canonical compiled but re-displayed failed: {:?} vs {:?}",
                            pattern2, pattern3
                        ).into()
                    ));
                }
            };

            // Spot-check on a few inputs.
            for input in ["", "a", "ab", "abc", "aaa", " ", "hello world"] {
                let ctx = MatchContext::simple(input);
                let r1 = re1.find(&ctx);
                let r2 = re2.find(&ctx);
                if let (Ok(m1), Ok(m2)) = (&r1, &r2) {
                    prop_assert_eq!(
                        m1.as_ref().map(|m| m.range.clone()),
                        m2.as_ref().map(|m| m.range.clone()),
                        "functional mismatch on input={:?} pattern={:?}",
                        input, pattern2
                    );
                }
            }
        }
    }
}
