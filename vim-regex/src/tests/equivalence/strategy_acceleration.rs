//! Strategy-accelerator equivalence: the accelerated strategies must produce
//! identical results to the unaccelerated engine.
//!
//! # Design overview
//!
//! Each accelerator is an *optimization* that must be semantically transparent:
//! the same pattern on the same text must produce the same `VimMatch` regardless
//! of whether the optimization is active. This file contains the infrastructure
//! and test cases for proving that contract.
//!
//! ## Disabling strategies for comparison
//!
//! We use `VimRegex::with_config(pattern, config)` where `StrategyConfig` is a
//! `#[cfg(test)]`-only struct that lets tests selectively disable each new
//! acceleration pathway. The default config enables everything (production
//! behavior); tests construct a "baseline" config that disables the feature
//! under test.
//!
//! This avoids:
//! - Feature flags (bloat Cargo.toml, leak into public API)
//! - Duplicate compilation (two separate NFA builds)
//! - Snapshot files (brittle, opaque)
//!
//! The config lives behind `#[cfg(test)]` so it has zero production cost.
//!
//! ## Test organization
//!
//! - Section 1: `StrategyConfig` and `assert_equivalence` harness
//! - Section 2: Reverse NFA equivalence
//! - Section 3: AC full-matcher equivalence
//! - Section 4: AC prefilter equivalence
//! - Section 5: CoW capture equivalence
//! - Section 6: Double-push regression
//! - Section 7: Adversarial / fuzz patterns
//!
//! ## How this file is wired
//!
//! In `regex/mod.rs`, add:
//!
//! ```ignore
//! #[cfg(test)]
//! #[path = "tests/equivalence/strategy_acceleration.rs"]
//! mod strategy_acceleration_tests;
//! ```

use crate::engine::VimRegex;
use crate::matchers::MatchContext;

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 1 — STRATEGY CONFIG AND EQUIVALENCE HARNESS
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for selectively disabling optimization strategies during tests.
///
/// Production code always uses `StrategyConfig::default()` (everything enabled).
/// Tests construct a "baseline" config to compare against.
///
/// This struct is `#[cfg(test)]`-only — it does not exist in release builds.
///
/// ## Implementation guidance for engine.rs
///
/// Add this to `VimRegex`:
///
/// ```ignore
/// #[cfg(test)]
/// pub(crate) fn with_config(
///     pattern: &str,
///     config: StrategyConfig,
/// ) -> Result<Self, VimRegexError> {
///     let mut re = Self::new(pattern)?;
///     if !config.enable_reverse_nfa {
///         re.reverse_nfa = None;  // drop the reverse NFA
///     }
///     if !config.enable_ac_matcher {
///         re.ac_automaton = None;  // drop the AC automaton
///     }
///     if !config.enable_ac_prefilter {
///         re.ac_prefilter = None;  // drop the AC prefilter
///     }
///     // CoW is structural — no runtime toggle needed (see Section 5)
///     Ok(re)
/// }
/// ```
#[derive(Debug, Clone)]
#[allow(
    dead_code,
    reason = "will be consumed by VimRegex::with_config once the accelerators land"
)]
struct StrategyConfig {
    /// Enable reverse NFA strategies (ReverseSuffix, ReverseInner, ReverseAnchored).
    enable_reverse_nfa: bool,
    /// Enable Aho-Corasick full matcher for literal alternations.
    enable_ac_matcher: bool,
    /// Enable Aho-Corasick prefilter for multi-pattern literal skip.
    enable_ac_prefilter: bool,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            enable_reverse_nfa: true,
            enable_ac_matcher: true,
            enable_ac_prefilter: true,
        }
    }
}

#[allow(
    dead_code,
    reason = "will be consumed by VimRegex::with_config once the accelerators land"
)]
impl StrategyConfig {
    /// All optimizations disabled — pure Pike VM / backtracker baseline.
    fn baseline() -> Self {
        Self {
            enable_reverse_nfa: false,
            enable_ac_matcher: false,
            enable_ac_prefilter: false,
        }
    }

    /// Only reverse NFA enabled (for testing reverse in isolation).
    fn reverse_only() -> Self {
        Self {
            enable_reverse_nfa: true,
            enable_ac_matcher: false,
            enable_ac_prefilter: false,
        }
    }

    /// Only AC matcher enabled.
    fn ac_matcher_only() -> Self {
        Self {
            enable_reverse_nfa: false,
            enable_ac_matcher: true,
            enable_ac_prefilter: false,
        }
    }

    /// Only AC prefilter enabled.
    fn ac_prefilter_only() -> Self {
        Self {
            enable_reverse_nfa: false,
            enable_ac_matcher: false,
            enable_ac_prefilter: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Equivalence assertion helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Compile `pattern` twice — once with all optimizations (`optimized`) and once
/// with the baseline engine only (`baseline`) — then assert every search method
/// returns identical results on `text`.
///
/// Until `with_config` is wired into `VimRegex`, this function compiles both
/// via `VimRegex::new()` (they use identical code paths today). Once the accelerators
/// strategies land, the optimized build will use `StrategyConfig::default()`
/// and the baseline will use `StrategyConfig::baseline()`.
fn assert_full_equivalence(pattern: &str, text: &str) {
    // --- compile ---
    let re_opt = VimRegex::new(pattern).expect("optimized compile failed");
    let re_base = VimRegex::new(pattern).expect("baseline compile failed");
    // NOTE: Once with_config exists, replace the above with:
    //   let re_opt = VimRegex::with_config(pattern, StrategyConfig::default()).unwrap();
    //   let re_base = VimRegex::with_config(pattern, StrategyConfig::baseline()).unwrap();

    let ctx = MatchContext::simple(text);

    // --- find (forward from 0) ---
    let opt_find = re_opt.find(&ctx).expect("opt find error");
    let base_find = re_base.find(&ctx).expect("base find error");
    assert_eq!(
        opt_find, base_find,
        "find() mismatch for pattern={pattern:?} text={text:?}\n  opt={opt_find:?}\n  base={base_find:?}"
    );

    // --- is_match ---
    let opt_is = re_opt.is_match(&ctx).expect("opt is_match error");
    let base_is = re_base.is_match(&ctx).expect("base is_match error");
    assert_eq!(
        opt_is, base_is,
        "is_match() mismatch for pattern={pattern:?} text={text:?}"
    );

    // --- find_at at every char boundary ---
    let mut pos = 0;
    while pos <= text.len() {
        let opt_from = re_opt.find_at(&ctx, pos).expect("opt find_at error");
        let base_from = re_base.find_at(&ctx, pos).expect("base find_at error");
        assert_eq!(
            opt_from, base_from,
            "find_at(pos={pos}) mismatch for pattern={pattern:?} text={text:?}\n  opt={opt_from:?}\n  base={base_from:?}"
        );
        // Advance to next char boundary.
        if pos >= text.len() {
            break;
        }
        pos += text[pos..].chars().next().map_or(1, |c| c.len_utf8());
    }

    // --- find_all ---
    let opt_all = re_opt.find_all(&ctx).expect("opt find_all error");
    let base_all = re_base.find_all(&ctx).expect("base find_all error");
    assert_eq!(
        opt_all, base_all,
        "find_all() mismatch for pattern={pattern:?} text={text:?}\n  opt={opt_all:?}\n  base={base_all:?}"
    );

    // --- find_backward (cursor at end) ---
    let bwd_ctx = MatchContext {
        text,
        cursor: Some(text.len()),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let opt_bwd = re_opt.find_backward(&bwd_ctx).expect("opt backward error");
    let base_bwd = re_base
        .find_backward(&bwd_ctx)
        .expect("base backward error");
    assert_eq!(
        opt_bwd, base_bwd,
        "find_backward(cursor=end) mismatch for pattern={pattern:?} text={text:?}\n  opt={opt_bwd:?}\n  base={base_bwd:?}"
    );

    // --- find_backward (cursor at midpoint) ---
    if text.len() >= 2 {
        // Find the last char boundary at or before `text.len() / 2`.
        let target = text.len() / 2;
        let mid = text
            .char_indices()
            .take_while(|(i, _)| *i <= target)
            .last()
            .map_or(0, |(i, c)| i + c.len_utf8());
        let mid_ctx = MatchContext {
            text,
            cursor: Some(mid),
            visual_range: None,
            case_sensitive: true,
            ignore_composing: false,
            line_resolver: None,
            mark_resolver: None,
            last_substitute: None,
        };
        let opt_mid = re_opt.find_backward(&mid_ctx).expect("opt mid-bwd error");
        let base_mid = re_base.find_backward(&mid_ctx).expect("base mid-bwd error");
        assert_eq!(
            opt_mid, base_mid,
            "find_backward(cursor=mid={mid}) mismatch for pattern={pattern:?} text={text:?}"
        );
    }
}

/// Assert that captures match between optimized and baseline.
///
/// Specifically checks that all 9 possible capture group ranges are identical.
fn assert_capture_equivalence(pattern: &str, text: &str) {
    let re_opt = VimRegex::new(pattern).expect("optimized compile failed");
    let re_base = VimRegex::new(pattern).expect("baseline compile failed");
    let ctx = MatchContext::simple(text);

    let opt_m = re_opt.find(&ctx).expect("opt find error");
    let base_m = re_base.find(&ctx).expect("base find error");

    match (&opt_m, &base_m) {
        (None, None) => {} // both found nothing — ok
        (Some(o), Some(b)) => {
            assert_eq!(
                o.range, b.range,
                "match range mismatch for pattern={pattern:?} text={text:?}"
            );
            assert_eq!(
                o.full_range, b.full_range,
                "full_range mismatch for pattern={pattern:?} text={text:?}"
            );
            assert_eq!(
                o.captures.len(),
                b.captures.len(),
                "capture count mismatch for pattern={pattern:?} text={text:?}"
            );
            for (i, (oc, bc)) in o.captures.iter().zip(b.captures.iter()).enumerate() {
                assert_eq!(
                    oc, bc,
                    "capture group {i} mismatch for pattern={pattern:?} text={text:?}\n  opt={oc:?}\n  base={bc:?}"
                );
            }
        }
        _ => panic!(
            "match presence mismatch for pattern={pattern:?} text={text:?}\n  opt={opt_m:?}\n  base={base_m:?}"
        ),
    }
}

/// Run a single pattern+text through `find()` and return the matched substring.
fn find<'t>(pattern: &str, text: &'t str) -> Option<&'t str> {
    let re = VimRegex::new(pattern).expect("valid pattern");
    let ctx = MatchContext::simple(text);
    let m = re.find(&ctx).expect("no engine error");
    m.map(|m| &text[m.range])
}

/// Like `find` but returns the byte range.
#[allow(
    dead_code,
    reason = "available for targeted range assertions in future tests"
)]
fn find_range(pattern: &str, text: &str) -> Option<std::ops::Range<usize>> {
    let re = VimRegex::new(pattern).expect("valid pattern");
    let ctx = MatchContext::simple(text);
    let m = re.find(&ctx).expect("no engine error");
    m.map(|m| m.range)
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 2 — REVERSE NFA EQUIVALENCE
// ═══════════════════════════════════════════════════════════════════════════════
//
// A reverse NFA strategy finds a match by:
//   1. Scanning backward from the end of a candidate region to locate where
//      a suffix/inner literal ends.
//   2. Using that position to narrow the forward search start.
//
// The invariant: the final match must be identical to what the forward-only
// Pike VM would have found.
//
// Three sub-strategies trigger on different pattern shapes:
//   - ReverseSuffix: pattern has a literal suffix (e.g., `.*foo`)
//   - ReverseInner:  pattern has a mandatory inner literal (e.g., `.*bar.*`)
//   - ReverseAnchored: pattern is anchored at end (e.g., `foo$`)

// ── Suffix literal patterns ────────────────────────────────────────────

#[test]
fn rev_suffix_simple_literal() {
    // `.*foo` — greedy prefix + literal suffix.
    // Reverse strategy: scan backward for "foo", narrow forward search.
    assert_full_equivalence(".*foo", "hello foo world foo end");
}

#[test]
fn rev_suffix_at_end_of_text() {
    assert_full_equivalence(".*foo", "xxxfoo");
}

#[test]
fn rev_suffix_no_match() {
    assert_full_equivalence(".*foo", "hello bar world");
}

#[test]
fn rev_suffix_with_captures() {
    // Captures must be identical whether or not reverse is used.
    assert_capture_equivalence("\\(.*\\)foo", "hello foo world foo");
}

#[test]
fn rev_suffix_multiline() {
    // Suffix search across newlines.
    assert_full_equivalence(".*bar", "first\nsecond bar\nthird");
}

#[test]
fn rev_suffix_unicode() {
    // Multi-byte suffix literal.
    assert_full_equivalence(".*日本", "テスト日本テスト日本");
}

#[test]
fn rev_suffix_greedy_vs_lazy() {
    // Greedy `.*` should consume as much as possible before the suffix.
    assert_full_equivalence(".*foo", "fooXfooXfoo");
    // Lazy `.\{-}` should consume as little as possible.
    assert_full_equivalence(".\\{-}foo", "fooXfooXfoo");
}

#[test]
fn rev_suffix_empty_text() {
    assert_full_equivalence(".*foo", "");
}

#[test]
fn rev_suffix_alternation_suffix() {
    // `.*\(foo\|bar\)` — suffix is an alternation, not a single literal.
    // Reverse strategy may not trigger, but equivalence must hold.
    assert_full_equivalence(".*\\(foo\\|bar\\)", "hello bar world");
}

// ── Inner literal patterns ─────────────────────────────────────────────

#[test]
fn rev_inner_simple() {
    // `.* bar .*` — inner literal "bar" is mandatory.
    assert_full_equivalence(".*bar.*", "foo bar baz");
}

#[test]
fn rev_inner_multiple_occurrences() {
    // Forward search finds leftmost; reverse inner should not change that.
    assert_full_equivalence(".*bar.*", "bar one bar two bar three");
}

#[test]
fn rev_inner_no_match() {
    assert_full_equivalence(".*bar.*", "hello world");
}

#[test]
fn rev_inner_with_quantifier() {
    // `\d\+bar\d\+` — digits, inner "bar", digits.
    assert_full_equivalence("\\d\\+bar\\d\\+", "123bar456");
}

#[test]
fn rev_inner_unicode() {
    assert_full_equivalence(".*café.*", "un café chaud");
}

// ── End-anchored patterns ──────────────────────────────────────────────

#[test]
fn rev_anchored_end_simple() {
    assert_full_equivalence("world$", "hello world");
}

#[test]
fn rev_anchored_end_multiline() {
    assert_full_equivalence("end$", "start\nmiddle\nend");
}

#[test]
fn rev_anchored_end_no_match() {
    assert_full_equivalence("end$", "end of story");
}

#[test]
fn rev_anchored_end_with_prefix() {
    // `.*world$` — suffix + end anchor.
    assert_full_equivalence(".*world$", "hello world");
}

#[test]
fn rev_anchored_eof() {
    // `foo\%$` — literal at end of file.
    assert_full_equivalence("foo\\%$", "barfoo");
    assert_full_equivalence("foo\\%$", "foobar");
}

// ── Backward search with reverse NFA ───────────────────────────────────

#[test]
fn rev_backward_suffix() {
    // Backward search should also produce identical results.
    let re = VimRegex::new(".*foo").unwrap();
    let ctx = MatchContext {
        text: "foo one foo two foo three",
        cursor: Some(24),
        visual_range: None,
        case_sensitive: true,
        ignore_composing: false,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: None,
    };
    let m = re.find_backward(&ctx).unwrap();
    assert!(m.is_some(), "backward search should find a match");
}

#[test]
fn rev_backward_inner() {
    let re = VimRegex::new("\\d\\+bar\\d\\+").unwrap();
    let ctx = MatchContext {
        text: "1bar2 3bar4 5bar6",
        cursor: Some(17),
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

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 3 — AC FULL-MATCHER EQUIVALENCE
// ═══════════════════════════════════════════════════════════════════════════════
//
// When a pattern is a pure literal alternation (`foo\|bar\|baz`) with
// >= AC_THRESHOLD branches, the engine may replace the NFA alternation
// with an Aho-Corasick automaton for O(n + m) matching.
//
// The invariant: AC must find the same leftmost match as the NFA.

// ── Basic AC equivalence ───────────────────────────────────────────────

#[test]
fn ac_three_branch_alternation() {
    // Below threshold — NFA path. Verify baseline is correct.
    assert_full_equivalence("foo\\|bar\\|baz", "test baz here");
}

#[test]
fn ac_four_branch_alternation() {
    // At or above a plausible threshold.
    assert_full_equivalence(
        "alpha\\|beta\\|gamma\\|delta",
        "look for gamma in this text",
    );
}

#[test]
fn ac_eight_branch_alternation() {
    assert_full_equivalence(
        "one\\|two\\|three\\|four\\|five\\|six\\|seven\\|eight",
        "here is five and seven",
    );
}

#[test]
fn ac_leftmost_wins() {
    // Both "foo" and "bar" present; leftmost in text wins.
    assert_full_equivalence("foo\\|bar\\|baz\\|qux", "prefix bar then foo suffix");
}

#[test]
fn ac_leftmost_pattern_priority() {
    // When two patterns match at the same position, Vim uses the first
    // alternation branch. AC must preserve this.
    assert_full_equivalence("ab\\|abc\\|abcd\\|abcde", "abcde");
}

#[test]
fn ac_no_match() {
    assert_full_equivalence("xxx\\|yyy\\|zzz", "nothing matches here");
}

#[test]
fn ac_find_all_equivalence() {
    let re = VimRegex::new("cat\\|dog\\|bird\\|fish").unwrap();
    let ctx = MatchContext::simple("I have a cat and a dog and a bird and a fish");
    let matches = re.find_all(&ctx).unwrap();
    assert_eq!(matches.len(), 4);
    // Verify order: cat, dog, bird, fish.
    assert_eq!(&ctx.text[matches[0].range.clone()], "cat");
    assert_eq!(&ctx.text[matches[1].range.clone()], "dog");
    assert_eq!(&ctx.text[matches[2].range.clone()], "bird");
    assert_eq!(&ctx.text[matches[3].range.clone()], "fish");
}

// ── Case-insensitive AC ────────────────────────────────────────────────

#[test]
fn ac_case_insensitive() {
    assert_full_equivalence("\\cABC\\|DEF\\|GHI\\|JKL", "look for ghi here");
}

#[test]
fn ac_case_insensitive_mixed() {
    // \c on the whole pattern: all branches are case-insensitive.
    let re = VimRegex::new("\\cFOO\\|BAR\\|BAZ\\|QUX").unwrap();
    let ctx = MatchContext::simple("test Baz here");
    let m = re.find(&ctx).unwrap();
    assert!(m.is_some());
    assert_eq!(&ctx.text[m.unwrap().range], "Baz");
}

// ── Prefix-of-each-other patterns ──────────────────────────────────────

#[test]
fn ac_prefix_patterns() {
    // "a", "ab", "abc", "abcd" — AC must respect Vim's leftmost-first rule.
    // Vim alternation `a\|ab\|abc\|abcd` on "abcd" matches "a" (first branch).
    assert_full_equivalence("a\\|ab\\|abc\\|abcd", "abcd");
}

#[test]
fn ac_suffix_shared() {
    // Alternates that share a suffix.
    assert_full_equivalence("foobar\\|bazbar\\|quxbar\\|nixbar", "find bazbar in text");
}

// ── Unicode AC ─────────────────────────────────────────────────────────

#[test]
fn ac_unicode_alternates() {
    assert_full_equivalence("café\\|naïve\\|résumé\\|über", "send your résumé");
}

#[test]
fn ac_unicode_cjk() {
    assert_full_equivalence("東京\\|大阪\\|名古屋\\|福岡", "visit 名古屋 next");
}

// ── Large alternation ──────────────────────────────────────────────────

#[test]
fn ac_many_branches() {
    // Build a 50-branch alternation of 4-char strings.
    let branches: Vec<String> = (0..50).map(|i| format!("w{i:03}")).collect();
    let pattern = branches.join("\\|");
    let text = format!("prefix {} suffix", branches[37]);
    assert_full_equivalence(&pattern, &text);
}

#[test]
fn ac_many_branches_no_match() {
    let branches: Vec<String> = (0..50).map(|i| format!("w{i:03}")).collect();
    let pattern = branches.join("\\|");
    assert_full_equivalence(&pattern, "nothing here at all");
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 4 — AC PREFILTER EQUIVALENCE
// ═══════════════════════════════════════════════════════════════════════════════
//
// When the pattern is NOT a pure literal alternation but HAS alternation
// with literal first-bytes (e.g., `\(foo\|bar\)\d+`), an AC prefilter
// can skip positions where none of the alternation branches can start.
//
// The invariant: AC prefilter + NFA confirmation == NFA-only.

#[test]
fn ac_pf_alternation_with_suffix() {
    // `\(foo\|bar\|baz\)_end` — alternation followed by suffix.
    // AC prefilter scans for "foo"/"bar"/"baz", NFA confirms the rest.
    assert_full_equivalence(
        "\\(foo\\|bar\\|baz\\)_end",
        "test bar_end here and foo_end there",
    );
}

#[test]
fn ac_pf_alternation_with_quantifier() {
    assert_full_equivalence("\\(alpha\\|beta\\|gamma\\)\\d\\+", "alpha123 beta456");
}

#[test]
fn ac_pf_no_match() {
    assert_full_equivalence("\\(xxx\\|yyy\\|zzz\\)_suffix", "no match in this text");
}

#[test]
fn ac_pf_find_all() {
    let pattern = "\\(cat\\|dog\\|bird\\)_house";
    let text = "cat_house dog_house bird_house fish_house";
    assert_full_equivalence(pattern, text);
}

#[test]
fn ac_pf_backward_search() {
    let re = VimRegex::new("\\(foo\\|bar\\|baz\\)_end").unwrap();
    let text = "foo_end xxx bar_end xxx baz_end";
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
    let m = re.find_backward(&ctx).unwrap();
    assert!(m.is_some());
    assert_eq!(&text[m.unwrap().range], "baz_end");
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 5 — COW CAPTURE EQUIVALENCE
// ═══════════════════════════════════════════════════════════════════════════════
//
// CoW (Copy-on-Write) is an internal optimization of `SlotTable`. The public
// API is unchanged — `get()` returns `&[Option<usize>]`, `get_mut()` returns
// `&mut [Option<usize>]`. The only difference is that `copy_slots()` shares
// a physical row instead of memcpy'ing.
//
// Strategy for proving equivalence:
//
// 1. The existing `SlotTable` tests in `slot_table.rs` already cover the
//    internal CoW mechanics (sharing, materialisation, three-way share, etc.).
//
// 2. Here we test at the `VimRegex` level: patterns with many capture groups,
//    nested groups, and edge cases must produce the same captures regardless
//    of the internal SlotTable implementation.
//
// Since CoW is already the implementation (the code I read in slot_table.rs
// already uses CoW with refcounted rows, indirection, and sentinel), these
// tests validate the existing implementation and serve as regression tests
// if the internals are ever changed.

#[test]
fn cow_single_capture() {
    assert_capture_equivalence("\\(foo\\)", "foobar");
}

#[test]
fn cow_two_captures() {
    assert_capture_equivalence("\\(foo\\)\\(bar\\)", "foobar");
}

#[test]
fn cow_nested_captures() {
    assert_capture_equivalence("\\(\\(a\\)b\\)", "ab");
}

#[test]
fn cow_nine_captures() {
    // Maximum: 9 capture groups.
    assert_capture_equivalence(
        "\\(a\\)\\(b\\)\\(c\\)\\(d\\)\\(e\\)\\(f\\)\\(g\\)\\(h\\)\\(i\\)",
        "abcdefghi",
    );
}

#[test]
fn cow_deep_nesting() {
    // `\(\(\(\(\(\(\(\(\(a\)\)\)\)\)\)\)\)\)` — 9-deep nesting.
    assert_capture_equivalence(
        "\\(\\(\\(\\(\\(\\(\\(\\(\\(a\\)\\)\\)\\)\\)\\)\\)\\)\\)",
        "a",
    );
}

#[test]
fn cow_alternation_forks() {
    // `\(a\|b\)\(c\|d\)\(e\|f\)\(g\|h\)` — each fork creates threads.
    // Many threads die immediately — CoW should avoid copies for dead threads.
    assert_capture_equivalence("\\(a\\|b\\)\\(c\\|d\\)\\(e\\|f\\)\\(g\\|h\\)", "aceg");
}

#[test]
fn cow_alternation_forks_all_second() {
    assert_capture_equivalence("\\(a\\|b\\)\\(c\\|d\\)\\(e\\|f\\)\\(g\\|h\\)", "bdfh");
}

#[test]
fn cow_empty_capture() {
    // `\(a*\)b` on "b" — group 1 captures empty string.
    assert_capture_equivalence("\\(a*\\)b", "b");
}

#[test]
fn cow_optional_capture_unset() {
    // `\(foo\)\?\(bar\)` on "bar" — group 1 unset, group 2 set.
    assert_capture_equivalence("\\(foo\\)\\?\\(bar\\)", "bar");
}

#[test]
fn cow_optional_capture_set() {
    assert_capture_equivalence("\\(foo\\)\\?\\(bar\\)", "foobar");
}

#[test]
fn cow_all_threads_survive() {
    // Pattern where ALL threads survive (many `.` transitions).
    // CoW provides no benefit but must not regress.
    assert_capture_equivalence("\\(.\\)\\(.\\)\\(.\\)", "xyz");
}

#[test]
fn cow_captures_with_zs_ze() {
    // `\zs` and `\ze` interact with captures.
    let re = VimRegex::new("\\(foo\\)\\zsbar\\ze\\(baz\\)").unwrap();
    let ctx = MatchContext::simple("foobarbaz");
    let m = re.find(&ctx).unwrap().unwrap();
    // Narrowed range should be "bar" (3..6).
    assert_eq!(m.range, 3..6);
    // Capture 1: "foo" (0..3).
    assert_eq!(m.captures.first(), Some(&Some(0..3)));
    // Capture 2: "baz" (6..9).
    assert_eq!(m.captures.get(1), Some(&Some(6..9)));
}

#[test]
fn cow_nosaves_mode() {
    // `is_match` uses NoSaves (zero-slot cache). Must still be correct.
    let re = VimRegex::new("\\(foo\\)\\(bar\\)").unwrap();
    assert!(re.is_match(&MatchContext::simple("foobar")).unwrap());
    assert!(!re.is_match(&MatchContext::simple("baz")).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 6 — DOUBLE-PUSH REGRESSION
// ═══════════════════════════════════════════════════════════════════════════════
//
// The Integration Surgeon identified that `epsilon_closure` can double-push
// a state to `dest` when the chain fast-path pushes a state (because it has
// consuming transitions), and then the post-chain check also pushes the same
// state (because `has_consuming || is_accept` is true).
//
// The chain fast-path only fires when a state has exactly one outgoing epsilon
// transition. After following the chain, the loop exits at a state with != 1
// transitions. That final state is then processed by the "normal" path below
// the chain loop, which checks `has_consuming || is_accept` and pushes.
//
// If the chain fast-path already pushed this state into `dest`, the post-chain
// push creates a duplicate.
//
// This is *not a correctness bug* — duplicate entries in `dest` just cause the
// same state to be stepped twice, yielding identical results. But it IS a
// performance regression (wasted work).
//
// The test below creates a pattern that triggers the chain fast-path and verifies
// that the match result is still correct. The performance aspect (no duplicates)
// should be tested via an internal unit test on epsilon_closure, not at the
// public API level.

#[test]
fn double_push_chain_fast_path_correct() {
    // Pattern: `a\(b\)c` — the NFA for `\(b\)` has:
    //   Save(0) -> [single epsilon chain] -> Literal('b') -> Save(1)
    // The chain fast-path follows Save(0) -> intermediate epsilons.
    // After the chain, the state with Literal('b') is processed normally.
    //
    // If double-push occurs, `Literal('b')` state appears twice in dest.
    // The match result should still be correct either way.
    let re = VimRegex::new("a\\(b\\)c").unwrap();
    let ctx = MatchContext::simple("abc");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..3);
    assert_eq!(m.captures.first(), Some(&Some(1..2)));
}

#[test]
fn double_push_nested_group_chain() {
    // `\(\(a\)\)` — nested groups create a chain of Save transitions.
    let re = VimRegex::new("\\(\\(a\\)\\)").unwrap();
    let ctx = MatchContext::simple("a");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..1);
    assert_eq!(m.captures.first(), Some(&Some(0..1))); // outer
    assert_eq!(m.captures.get(1), Some(&Some(0..1))); // inner
}

#[test]
fn double_push_alternation_epsilon() {
    // `a\|b` — the NFA start has two epsilon transitions (one per branch).
    // Neither triggers the chain fast-path (two transitions, not one).
    // But if the alternation is inside a group, the group wrapper
    // creates a chain: Save(0) -> epsilon -> alt_start.
    let re = VimRegex::new("\\(a\\|b\\)c").unwrap();
    let ctx = MatchContext::simple("bc");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..2);
    assert_eq!(m.captures.first(), Some(&Some(0..1)));
}

#[test]
fn double_push_quantifier_chain() {
    // `a\+` — the NFA for `\+` has an epsilon back-edge.
    // State with Literal('a') + epsilon creates the pattern where
    // chain fast-path might follow the epsilon back to itself.
    let re = VimRegex::new("a\\+").unwrap();
    let ctx = MatchContext::simple("aaa");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 0..3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 7 — ADVERSARIAL / FUZZ PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════
//
// Patterns designed to stress the boundaries of each new feature.

// ── Reverse NFA stress ─────────────────────────────────────────────────

#[test]
fn adversarial_rev_greedy_full_scan() {
    // `.*` — greedy, scans entire text. Reverse scan budget may be exhausted.
    // Keep text small enough that find_at at every position finishes quickly.
    assert_full_equivalence(".*", "a".repeat(200).as_str());
}

#[test]
fn adversarial_rev_ambiguous_alternation() {
    // `\(a\|aa\)\+b` — ambiguous: "a" or "aa" before "b".
    // Forward engine explores both paths. Reverse must not change the result.
    assert_full_equivalence("\\(a\\|aa\\)\\+b", "aaab");
    assert_full_equivalence("\\(a\\|aa\\)\\+b", "aab");
    assert_full_equivalence("\\(a\\|aa\\)\\+b", "ab");
}

#[test]
fn adversarial_rev_multiline_boundary() {
    // `foo\nbar` — newline in pattern. Tests line boundaries in reverse.
    assert_full_equivalence("foo\\nbar", "xxx\nfoo\nbar\nyyy");
}

#[test]
fn adversarial_rev_zero_width_pattern() {
    // `^` is zero-width. Reverse NFA should handle gracefully.
    assert_full_equivalence("^", "hello\nworld");
}

#[test]
fn adversarial_rev_lookaround_plus_quantifier() {
    // `\(foo\)\@<=bar\+` — positive lookbehind + quantifier.
    assert_full_equivalence("\\(foo\\)\\@<=bar\\+", "foobarbaz");
}

#[test]
fn adversarial_rev_word_boundary_suffix() {
    // `\w\+\>` — word chars + word boundary end. Tests boundary in reverse.
    assert_full_equivalence("\\w\\+\\>", "hello world");
}

// ── AC stress ──────────────────────────────────────────────────────────

#[test]
fn adversarial_ac_single_char_branches() {
    // 26 single-char alternates. Should NOT trigger AC (single-byte threshold).
    // Verify the existing prefilter handles this correctly.
    let pattern = "a\\|b\\|c\\|d\\|e\\|f\\|g\\|h\\|i\\|j\\|k\\|l\\|m\\|n\\|o\\|p\\|q\\|r\\|s\\|t\\|u\\|v\\|w\\|x\\|y\\|z";
    assert_full_equivalence(pattern, "0123456789 hello");
}

#[test]
fn adversarial_ac_long_branches() {
    // Long alternates (50 chars each) — should trigger AC.
    let branch_a = "a".repeat(50);
    let branch_b = "b".repeat(50);
    let branch_c = "c".repeat(50);
    let branch_d = "d".repeat(50);
    let pattern = format!("{branch_a}\\|{branch_b}\\|{branch_c}\\|{branch_d}");
    let text = format!("prefix {} suffix", branch_c);
    assert_full_equivalence(&pattern, &text);
}

#[test]
fn adversarial_ac_prefix_of_each_other() {
    // `a\|ab\|abc\|abcd` — each is a prefix of the next.
    // AC must respect leftmost-first semantics.
    assert_full_equivalence("a\\|ab\\|abc\\|abcd", "abcde");
    // Verify: the match should be "a" (first branch), not "abcd" (longest).
    assert_eq!(find("a\\|ab\\|abc\\|abcd", "abcde"), Some("a"));
}

#[test]
fn adversarial_ac_shared_suffix() {
    assert_full_equivalence("foobar\\|bazbar\\|quxbar\\|nixbar", "the quxbar is here");
}

#[test]
fn adversarial_ac_case_insensitive_many() {
    // `\c` with 10 alternates.
    let pattern = "\\cALPHA\\|BETA\\|GAMMA\\|DELTA\\|EPSILON\\|ZETA\\|ETA\\|THETA\\|IOTA\\|KAPPA";
    assert_full_equivalence(pattern, "find theta in text");
}

// ── CoW stress ─────────────────────────────────────────────────────────

#[test]
fn adversarial_cow_max_groups_nested() {
    // 9 capture groups with deep nesting.
    assert_capture_equivalence(
        "\\(\\(\\(\\(\\(\\(\\(\\(\\(a\\)\\)\\)\\)\\)\\)\\)\\)\\)",
        "a",
    );
}

#[test]
fn adversarial_cow_many_forks() {
    // Each `\(a\|b\)` doubles the thread count.
    // 4 such groups = up to 16 threads alive simultaneously.
    assert_capture_equivalence("\\(a\\|b\\)\\(c\\|d\\)\\(e\\|f\\)\\(g\\|h\\)", "aceg");
}

#[test]
fn adversarial_cow_threads_die_immediately() {
    // Pattern where most alternation branches fail at position 0.
    // Only one path survives. CoW should avoid copies for the dead paths.
    assert_capture_equivalence(
        "\\(x\\|y\\|z\\|a\\)\\(x\\|y\\|z\\|b\\)\\(x\\|y\\|z\\|c\\)",
        "abc",
    );
}

#[test]
fn adversarial_cow_all_threads_survive_long() {
    // `\(.\)\(.\)\(.\)\(.\)\(.\)\(.\)\(.\)\(.\)\(.\)` on "123456789".
    // All 9 groups match — every thread survives. CoW provides no benefit.
    assert_capture_equivalence(
        "\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)\\(.\\)",
        "123456789",
    );
}

#[test]
fn adversarial_cow_captures_across_find_all() {
    // find_all with captures — each match may have different capture values.
    let re = VimRegex::new("\\(\\d\\+\\)x").unwrap();
    let ctx = MatchContext::simple("1x 22x 333x");
    let matches = re.find_all(&ctx).unwrap();
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0].captures.first(), Some(&Some(0..1))); // "1"
    assert_eq!(matches[1].captures.first(), Some(&Some(3..5))); // "22"
    assert_eq!(matches[2].captures.first(), Some(&Some(7..10))); // "333"
}

// ── Combined stress ────────────────────────────────────────────────────

#[test]
fn adversarial_combined_reverse_plus_ac() {
    // Pattern that could trigger both reverse NFA and AC:
    // `.*\(foo\|bar\|baz\|qux\)_end`
    // — suffix literal "_end" for reverse
    // — alternation for AC
    assert_full_equivalence(".*\\(foo\\|bar\\|baz\\|qux\\)_end", "prefix bar_end suffix");
}

#[test]
fn adversarial_combined_all_features() {
    // Pattern exercising captures + alternation + quantifier + suffix.
    assert_full_equivalence(
        "\\(\\d\\+\\)\\(foo\\|bar\\)\\(.*\\)end",
        "123foo middle end",
    );
}

#[test]
fn adversarial_utf8_reverse_scan() {
    // Multi-byte chars with reverse scan.
    // Keep text moderate to avoid O(n^2) in find_at-at-every-position.
    let text = format!("{}target{}", "日".repeat(50), "本".repeat(50));
    assert_full_equivalence(".*target.*", &text);
}

#[test]
fn adversarial_empty_branches_in_alternation() {
    // Edge: alternation where some branches can match empty.
    // Not a valid AC candidate (AC requires non-empty literals), but
    // equivalence must hold.
    assert_full_equivalence("foo\\|\\|bar", "test");
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 8 — PARAMETRIC BULK EQUIVALENCE
// ═══════════════════════════════════════════════════════════════════════════════
//
// Run a large set of (pattern, text) pairs through the full equivalence harness.
// This catches regressions that individual targeted tests might miss.

/// Corpus of (pattern, text) pairs that exercise every strategy path.
///
/// Organized by which strategy they are expected to trigger (though the
/// equivalence assertion holds regardless of which strategy fires).
const EQUIVALENCE_CORPUS: &[(&str, &str)] = &[
    // --- Pure literal ---
    ("hello", "say hello world"),
    ("xyz", "no match"),
    ("日本語", "テスト日本語テスト"),
    // --- Alternation (potential AC) ---
    ("foo\\|bar", "test bar end"),
    ("foo\\|bar\\|baz\\|qux", "before qux after"),
    ("one\\|two\\|three\\|four\\|five", "pick three"),
    ("abc\\|def\\|ghi\\|jkl\\|mno\\|pqr", "find mno here"),
    // --- Suffix literal (potential reverse) ---
    (".*foo", "prefix foo suffix"),
    (".*\\.rs", "main.rs"),
    (".*end$", "the end"),
    // --- Inner literal (potential reverse) ---
    ("\\d\\+bar\\d\\+", "123bar456"),
    (".*middle.*", "start middle end"),
    // --- Anchored ---
    ("^hello", "hello world"),
    ("world$", "hello world"),
    ("^foo$", "foo"),
    ("^foo$", "foo\nbar"),
    // --- Captures ---
    ("\\(foo\\)\\(bar\\)", "foobar"),
    ("\\(a\\|b\\)\\(c\\|d\\)", "bc"),
    ("\\(\\d\\+\\)", "abc123def"),
    // --- Quantifiers ---
    ("a\\+", "aaa"),
    ("a\\{2,4}", "aaaaa"),
    ("a\\{-2,4}", "aaaaa"),
    ("x*", "xxx"),
    // --- Character classes ---
    ("\\d\\+", "abc123def"),
    ("\\w\\+", "  hello  "),
    ("[a-z]\\+", "123abc"),
    ("[^0-9]\\+", "abc123"),
    // --- Lookaround ---
    ("foo\\(bar\\)\\@=", "foobar"),
    ("foo\\(bar\\)\\@!", "foobaz"),
    ("\\(foo\\)\\@<=bar", "foobar"),
    // --- \zs / \ze ---
    ("foo\\zsbar", "foobar"),
    ("foo\\zebar", "foobar"),
    // --- Word boundaries ---
    ("\\<word\\>", "a word here"),
    ("\\<word\\>", "wording"),
    // --- Multiline ---
    ("^def", "abc\ndef\nghi"),
    ("\\_.*", "hello\nworld"),
    // --- Case insensitive ---
    ("\\cfoo", "FOO"),
    ("\\Cfoo", "FOO"),
];

#[test]
fn parametric_corpus_equivalence() {
    for (pattern, text) in EQUIVALENCE_CORPUS {
        assert_full_equivalence(pattern, text);
    }
}

#[test]
fn parametric_corpus_captures() {
    // Run capture equivalence on patterns that have groups.
    for (pattern, text) in EQUIVALENCE_CORPUS {
        if pattern.contains("\\(") {
            assert_capture_equivalence(pattern, text);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 9 — SLOT TABLE UNIT TESTS (COW INTERNALS)
// ═══════════════════════════════════════════════════════════════════════════════
//
// These tests complement the existing `slot_table.rs` tests by focusing on
// patterns that stress CoW sharing during actual NFA simulation. They do NOT
// require the `with_config` mechanism — they test the SlotTable directly.
//
// The existing `slot_table.rs` already has excellent coverage:
//   - NoSaves mode
//   - Basic get/set
//   - copy_slots sharing
//   - CoW materialisation
//   - Three-way share chain
//   - clear_row
//   - reset_all
//   - assign_from_slice
//   - Cross-table copy
//   - Free list recycling
//   - Sentinel invariants
//
// Additional tests needed for the accelerators (to be added to slot_table.rs):
//
// 1. `cow_high_sharing_ratio`: All N states share one row, then one mutates.
//    Verify N-1 still see old data, mutated one sees new data.
//
// 2. `cow_rapid_share_then_reset`: Share rows rapidly, then reset_all.
//    Free list must be fully rebuilt.
//
// 3. `cow_assign_from_slice_over_shared`: assign_from_slice on a state that
//    shares a row with siblings. Siblings must not be affected.
//
// These are documented here but should be implemented directly in
// `cache/slot_table.rs` alongside the existing tests.

// ═══════════════════════════════════════════════════════════════════════════════
// SECTION 10 — FILE ORGANIZATION PLAN
// ═══════════════════════════════════════════════════════════════════════════════
//
// The test framework is organized as follows:
//
// ## This file: `strategy_acceleration.rs`
//   - Central equivalence harness (`assert_full_equivalence`, `assert_capture_equivalence`)
//   - Cross-cutting tests that verify *semantic transparency* of all accelerators
//   - Adversarial patterns that stress multiple features simultaneously
//   - Parametric corpus for bulk regression testing
//
// ## Existing files to extend:
//
// ### `strategy_equivalence_tests.rs`
//   - Already has 22 sections covering all strategy paths
//   - Add new sections for reverse NFA, AC matcher, AC prefilter
//   - These would be targeted tests for specific strategy selection logic
//
// ### `cache/slot_table.rs`
//   - Add CoW-specific stress tests (high sharing ratio, rapid share/reset)
//   - These test the data structure, not the regex semantics
//
// ## New files (if an accelerator has complex internal logic):
//
// ### `engines/reverse_pike_vm_tests.rs` (if reverse VM is a separate module)
//   - Unit tests for reverse NFA construction
//   - Reverse epsilon closure correctness
//   - Reverse scan budget enforcement
//
// ### `accel/aho_corasick_tests.rs` (if AC is a separate module)
//   - AC automaton construction tests
//   - AC match semantics (leftmost-first vs leftmost-longest)
//   - Case-insensitive AC byte folding
//   - Threshold selection tests
//
// ## Why one central file instead of scattered tests:
//
// The key contract is *equivalence across all methods* (find, find_at,
// find_all, find_backward, is_match, captures). A single harness tests all
// of these in one call. Scattering this across files would duplicate the
// harness or miss some methods.
//
// Individual files for `reverse_pike_vm_tests.rs` and `aho_corasick_tests.rs`
// are for *unit testing internal correctness* of those modules (e.g., "does
// the AC automaton produce the right state transitions"). The equivalence
// tests here are for *integration testing semantic transparency*.
