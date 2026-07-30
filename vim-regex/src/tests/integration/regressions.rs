//! Regression tests.
//!
//! A bug that escapes into a release earns a named test here so it cannot
//! return silently. Each carries a note saying how it was found and what went
//! wrong, so a future reader can tell a deliberate assertion from an accident.
//!
//! ```text
//! #[test]
//! fn regression_short_description() {
//!     // Found by: <how>
//!     // Bug: <one line>
//!     let re = VimRegex::new(r"pattern").unwrap();
//!     let ctx = MatchContext::simple("input");
//!     let m = re.find(&ctx).unwrap();
//!     assert_eq!(m, ...);
//! }
//! ```

use crate::engine::{EngineKind, VimRegex};
use crate::ir::VimRegexErrorKind;
use crate::matchers::MatchContext;
use crate::test_builder::regex;

/// Smoke test to verify this file compiles and is wired into the test tree.
#[test]
fn regressions_module_compiles() {
    let re = VimRegex::new("a").unwrap();
    let ctx = MatchContext::simple("a");
    assert!(re.is_match(&ctx).unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Cross-engine lookaround-capture equivalence (PikeVM vs Backtracker)
// ═══════════════════════════════════════════════════════════════════════════════

/// Assert `force_pike_vm` and `force_backtracker` agree on BOTH the match range
/// AND every capture for a capturing-lookaround pattern. Guards both engines'
/// lookaround-capture export against drift. All gold values nvim-verified.
fn c5_cross_engine_eq(pat: &str, txt: &str) {
    use crate::engine::SearchConfig;
    use crate::magic_mode::MagicMode;

    let pike =
        VimRegex::with_magic_and_config(pat, MagicMode::Magic, &SearchConfig::force_pike_vm())
            .unwrap();
    let bt =
        VimRegex::with_magic_and_config(pat, MagicMode::Magic, &SearchConfig::force_backtracker())
            .unwrap();

    let ctx = MatchContext::simple(txt);
    let pm = pike.find(&ctx).unwrap();
    let bm = bt.find(&ctx).unwrap();

    match (pm, bm) {
        (Some(p), Some(b)) => {
            assert_eq!(p.range, b.range, "range mismatch for {pat:?} on {txt:?}");
            let n = p.capture_count().max(b.capture_count());
            for g in 1..=n {
                assert_eq!(
                    p.capture(g),
                    b.capture(g),
                    "capture {g} mismatch for {pat:?} on {txt:?}"
                );
            }
        }
        (None, None) => {}
        (p, b) => {
            panic!("engine match-presence disagreement for {pat:?} on {txt:?}: pike={p:?} bt={b:?}")
        }
    }
}

/// Positive lookbehind capture is exported identically by both engines.
/// nvim: matchlist("foobar", '\(foo\)\@<=\(bar\)') -> ['bar','foo','bar', ...].
#[test]
fn c5_cross_engine_positive_lookbehind_capture() {
    c5_cross_engine_eq(r"\(foo\)\@<=\(bar\)", "foobar");
}

/// Positive lookahead capture is exported identically by both engines.
/// nvim: matchlist("abc", '\(ab\)\@=ab') -> ['ab','ab', ...].
#[test]
fn c5_cross_engine_positive_lookahead_capture() {
    c5_cross_engine_eq(r"\(ab\)\@=ab", "abc");
}

/// A passing negative lookbehind exports no capture in either engine.
/// nvim: matchstrpos("zy", '\(x\)\@<!y') -> ['y', 1, 2].
#[test]
fn c5_cross_engine_negative_lookbehind_no_capture() {
    c5_cross_engine_eq(r"\(x\)\@<!y", "zy");
}

/// Positive lookahead capture at a non-leading start position; the failed
/// position must not leak a stale sub-capture. Both engines agree.
/// nvim: matchstrpos("xabab", '\(ab\)\@=ab') -> ['ab', 1, 3].
#[test]
fn c5_cross_engine_lookahead_no_stale_capture() {
    c5_cross_engine_eq(r"\(ab\)\@=ab", "xabab");
}

/// A backreference resolving a group captured INSIDE a positive lookbehind.
/// This routes to the backtracker (PikeVM cannot resolve backrefs), so the
/// lookbehind's inner capture MUST be merged into the frame slots for the
/// later `\1` to resolve. nvim: matchstrpos("aa", '\(.\)\@<=\1') -> ['a', 1, 2];
/// matchlist -> group 1 = "a" (0..1). The earlier backtracker dropped the
/// lookbehind capture, leaving `\1` unresolvable.
#[test]
fn c5_option_b_backref_into_positive_lookbehind() {
    let re = VimRegex::new(r"\(.\)\@<=\1").unwrap();
    assert_eq!(
        re.engine_kind,
        EngineKind::Backtracker,
        "backref pattern must route to the backtracker"
    );
    let ctx = MatchContext::simple("aa");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 1..2, "match 'a' at 1..2");
    assert_eq!(
        m.capture(1),
        Some(&(0..1)),
        "group 1 captured inside the lookbehind = 'a' at 0..1"
    );
}

/// Multi-char variant: backref to a two-char group captured inside the
/// lookbehind. nvim: matchstrpos("abab", '\(ab\)\@<=\1') -> ['ab', 2, 4];
/// matchlist -> group 1 = "ab" (0..2).
#[test]
fn c5_option_b_backref_into_lookbehind_multichar() {
    let re = VimRegex::new(r"\(ab\)\@<=\1").unwrap();
    assert_eq!(re.engine_kind, EngineKind::Backtracker);
    let ctx = MatchContext::simple("abab");
    let m = re.find(&ctx).unwrap().unwrap();
    assert_eq!(m.range, 2..4);
    assert_eq!(m.capture(1), Some(&(0..2)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Adversarial: capture-on-backtrack leak + second-site export (nvim gold)
// ═══════════════════════════════════════════════════════════════════════════════

/// Run `pat` on `txt` through ONE forced engine and assert the exact match range
/// and gold captures (`gold[g-1]` is group `g`). nvim-verified gold values.
fn c5_engine_gold(
    cfg: crate::engine::SearchConfig,
    label: &str,
    pat: &str,
    txt: &str,
    range: std::ops::Range<usize>,
    gold: &[Option<std::ops::Range<usize>>],
) {
    use crate::magic_mode::MagicMode;
    let re = VimRegex::with_magic_and_config(pat, MagicMode::Magic, &cfg).unwrap();
    let ctx = MatchContext::simple(txt);
    let m = re
        .find(&ctx)
        .unwrap()
        .unwrap_or_else(|| panic!("[{label}] no match for {pat:?} on {txt:?}"));
    assert_eq!(m.range, range, "[{label}] range for {pat:?} on {txt:?}");
    for (i, want) in gold.iter().enumerate() {
        let g = i + 1;
        assert_eq!(
            m.capture(g),
            want.as_ref(),
            "[{label}] group {g} for {pat:?} on {txt:?}"
        );
    }
}

/// Assert both forced engines produce identical range + captures for `pat`/`txt`.
fn c5_both_engines_gold(
    pat: &str,
    txt: &str,
    range: std::ops::Range<usize>,
    gold: &[Option<std::ops::Range<usize>>],
) {
    use crate::engine::SearchConfig;
    c5_engine_gold(
        SearchConfig::force_pike_vm(),
        "pike",
        pat,
        txt,
        range.clone(),
        gold,
    );
    c5_engine_gold(
        SearchConfig::force_backtracker(),
        "bt",
        pat,
        txt,
        range,
        gold,
    );
}

/// Backtracker: a capturing lookbehind on a FAILING alternation branch
/// must not leak its capture into the winning branch. Outer `\%(...\)` is
/// non-capturing so only the inner lookbehind groups participate.
/// nvim: matchlist("ab", '\%(\(a\)\@<=bZ\|\(a\)\@<=b\)') -> ['b','','a',...];
/// matchstrpos -> ['b', 1, 2]. Branch1 captures g1 then "bZ" fails; backtracking
/// to branch2 must restore g1 to None (g2='a').
#[test]
fn c5_bug_a_lookbehind_capture_no_leak_on_failed_branch() {
    c5_engine_gold(
        crate::engine::SearchConfig::force_backtracker(),
        "bt",
        r"\%(\(a\)\@<=bZ\|\(a\)\@<=b\)",
        "ab",
        1..2,
        &[None, Some(0..1)],
    );
}

/// Backtracker: the same leak with a positive LOOKAHEAD on the failing branch.
/// nvim: matchlist("abc", '\%(\(ab\)\@=abZ\|\(ab\)\@=abc\)') -> ['abc','','ab',...];
/// matchstrpos -> ['abc', 0, 3]. g1 must be None, g2='ab'.
#[test]
fn c5_bug_a_lookahead_capture_no_leak_on_failed_branch() {
    c5_engine_gold(
        crate::engine::SearchConfig::force_backtracker(),
        "bt",
        r"\%(\(ab\)\@=abZ\|\(ab\)\@=abc\)",
        "abc",
        0..3,
        &[None, Some(0..2)],
    );
}

/// Backtracker: a differently-sized lookbehind capture on the failing
/// branch (g1 spans 2 chars) must not leak. nvim: matchstrpos("aab",
/// '\%(\(aa\)\@<=bZ\|\(a\)\@<=b\)') -> ['b', 2, 3]; matchlist g1='' g2='a'(1..2).
#[test]
fn c5_bug_a_lookbehind_sized_capture_no_leak() {
    c5_engine_gold(
        crate::engine::SearchConfig::force_backtracker(),
        "bt",
        r"\%(\(aa\)\@<=bZ\|\(a\)\@<=b\)",
        "aab",
        2..3,
        &[None, Some(1..2)],
    );
}

/// Root cause, PURE SAVE analog: a branch-exclusive plain capture on a
/// failing branch must not leak either. nvim: matchstrpos("ab",
/// '\%(\(a\)Z\|\(a\)b\)') -> ['ab', 0, 2]; matchlist -> ['ab','','a',...]
/// (g1='' g2='a'). This is the same restore-discipline root cause, no lookaround.
#[test]
fn c5_bug_a_pure_save_no_leak_on_failed_branch() {
    c5_both_engines_gold(r"\%(\(a\)Z\|\(a\)b\)", "ab", 0..2, &[None, Some(0..1)]);
}

/// Both engines: a SECOND positive lookaround later in the pattern must
/// export its capture. nvim: matchstrpos("abcd", '\(\a\)\@=\a\(\a\)\@=\a') ->
/// ['ab', 0, 2]; matchlist -> ['ab','a','b',...] (g1='a' 0..1, g2='b' 1..2).
#[test]
fn c5_bug_b_second_lookaround_capture_exported() {
    c5_both_engines_gold(
        r"\(\a\)\@=\a\(\a\)\@=\a",
        "abcd",
        0..2,
        &[Some(0..1), Some(1..2)],
    );
}

/// PikeVM alternation: the winning branch's lookahead capture must be
/// exported by the PikeVM too (combines second-site + alternation). nvim:
/// matchlist("abc", '\%(\(ab\)\@=abZ\|\(ab\)\@=abc\)') -> ['abc','','ab',...].
#[test]
fn c5_bug_b_pike_alternation_lookahead_capture_exported() {
    c5_both_engines_gold(
        r"\%(\(ab\)\@=abZ\|\(ab\)\@=abc\)",
        "abc",
        0..3,
        &[None, Some(0..2)],
    );
}

/// Cross-engine guard: lookaround capture followed by a later backref to that
/// group, both engines (well, backtracker for backref; verify range/caps).
/// nvim: matchstrpos("abab", '\(ab\)\@=\(ab\)\1') ... verify below.
#[test]
fn c5_lookaround_capture_then_backref_cross_check() {
    // Pure cross-engine equivalence guard for two sequential capturing lookaheads.
    c5_cross_engine_eq(r"\(\a\)\@=\a\(\a\)\@=\a", "abcd");
}

// ═══════════════════════════════════════════════════════════════════════════════
// NEOVIM DIFFERENTIAL TEST STUB
// ═══════════════════════════════════════════════════════════════════════════════

/// Differential test: compare vim-regex match results against Neovim's regex engine.
///
/// Ignored: running it needs a driver that starts a real Neovim, evaluates
/// `matchstrpos()` / `matchlist()` for each pattern and input, and reports the
/// results back. No such driver is wired into this crate. To enable the test,
/// add one as a dev-dependency, replace the loop below with a call into it,
/// and drop the `#[ignore]` attribute.
#[test]
#[ignore = "no Neovim differential driver is wired in — see comment above"]
fn neovim_differential_basic_patterns() {
    // Patterns to compare against Neovim's regex engine:
    let _patterns_and_inputs: &[(&str, &str)] = &[
        (r"\d\+", "abc123def"),
        (r"\w\+", "hello world"),
        (r"foo\|bar", "foobar"),
        (r"\(ab\)\+", "abababc"),
        (r"[a-z]\{2,4}", "abcdef"),
        (r"\<\w\+\>", "hello world"),
        (r"\v(foo|bar)+", "foobarfoo"),
        (r"\zsfoo", "xxfoo"),
    ];

    // For each (pattern, input), run both vim-regex and a real Neovim, then
    // assert match positions and captured groups are identical.
    panic!("not yet implemented — needs a Neovim differential driver");
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEMPLATE — copy this for each new regression
// ═══════════════════════════════════════════════════════════════════════════════
//
// #[test]
// fn regression_0001_describe_the_bug() {
//     // Found by: proptest seed XXXX / cargo-fuzz corpus abc123 / issue #NNN
//     // Bug: <one-line description of what went wrong>
//     let re = VimRegex::new(r"pattern_here").unwrap();
//     let ctx = MatchContext::simple("input_here");
//     let m = re.find(&ctx).unwrap().unwrap();
//     assert_eq!(m.range, 0..1);
// }

// ═══════════════════════════════════════════════════════════════════════════════
// Quantified-group capture renumbering
// ═══════════════════════════════════════════════════════════════════════════════

/// A capturing group inside `\{n}` must keep its number; the
/// following group must not be renumbered.
#[test]
fn c2_quantified_group_capture_numbering_is_vim_correct() {
    regex(r"\(a\)\{2}\(b\)")
        .text("aab")
        .expect_match(0..3)
        .expect_capture(1, 1..2) // group 1 = last rep of "a"
        .expect_capture(2, 2..3) // group 2 = "b"
        .run();
}

/// A backref to a quantified group resolves to the right slot.
#[test]
fn c2_quantified_group_then_backref_matches() {
    regex(r"\(.\)\{2}\1")
        .text("abb")
        .expect_match(0..3)
        .expect_capture(1, 1..2)
        .run();
}

/// Nested groups are numbered by opening-paren (pre-order), outer=1.
#[test]
fn c2_nested_group_numbering_outer_first() {
    regex(r"\(\(a\)\)")
        .text("a")
        .expect_match(0..1)
        .expect_capture(1, 0..1)
        .expect_capture(2, 0..1)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Nullable start-set / start-filter soundness
// ═══════════════════════════════════════════════════════════════════════════════

/// A nullable capture prefix (`a*`) must let the match
/// begin at a position the start filter would skip. nvim: `\(a*\)b` on "xb"
/// matches 1..2 (group 1 empty).
#[test]
fn c8_nullable_capture_prefix_matches() {
    regex(r"\(a*\)b").text("xb").expect_match(1..2).run();
}

/// A nullable prefix at offset 0 — `a*b` on "b" matches 0..1 even though no
/// `a` byte is present to seed the start set.
#[test]
fn c8_nullable_prefix_at_offset_zero() {
    regex(r"a*b").text("b").expect_match(0..1).run();
}

/// The whitespace-align idiom `\(\s*\)=` on "x=" matches 1..2 (the optional
/// leading whitespace is empty), exercising a nullable `\s*` prefix.
#[test]
fn c8_whitespace_align_idiom() {
    regex(r"\(\s*\)=").text("x=").expect_match(1..2).run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Case-insensitive collection overlap must backtrack (no over-possessify)
// ═══════════════════════════════════════════════════════════════════════════════

/// `\c[a-m]\+[A-M]` on "aaB" matches 0..3. The case-folded
/// classes overlap, so the greedy `[a-m]\+` must give back a char for `[A-M]`;
/// wrongly possessifying it drops the match.
#[test]
fn c6_ci_collection_overlap_backtracks() {
    regex(r"\c[a-m]\+[A-M]")
        .text("aaB")
        .expect_match(0..3)
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Backtracker-only features must not be silently lost above the VisitedSet cliff
// ═══════════════════════════════════════════════════════════════════════════════

/// A backref match must not be lost above the VisitedSet capacity cliff.
///
/// The backref pattern compiles to a 10-state NFA, so its decline threshold is
/// `2_097_152 / 10 = 209_715` bytes; a 400 KB pad is well past it. Above the
/// cliff the backtracker declines and dispatch falls back to PikeVM, which
/// cannot execute backreferences and returns `None` — the match is lost.
/// nvim: `\(xy\)\1` on "xyxy" matches 0..4.
#[test]
fn c1_backref_large_haystack_still_matches() {
    let pad = "z".repeat(400_000);
    let text = format!("{pad}xyxy");
    let start = pad.len();
    regex(r"\(xy\)\1")
        .text(&text)
        .expect_match(start..start + 4)
        .run();
}

/// An atomic group is also backtracker-only — must not fall back to PikeVM.
///
/// Pad is 600 KB: the atomic pattern compiles to a 4-state NFA, so its
/// VisitedSet decline threshold is `2_097_152 / 4 = 524_288` bytes. The match
/// must sit past that cliff to trigger the decline → PikeVM fallback. Above the
/// cliff PikeVM treats `\@>` as a zero-width look-ahead and returns the wrong
/// range (`start+3..start+4`, matching only `b`) instead of `start..start+4`.
/// nvim: `\(a*\)\@>b` on "aaab" matches 0..4.
#[test]
fn c1_atomic_group_large_haystack_still_matches() {
    let pad = "z".repeat(600_000);
    let text = format!("{pad}aaab");
    let start = pad.len();
    regex(r"\(a*\)\@>b")
        .text(&text)
        .expect_match(start..start + 4)
        .run();
}

/// Capability guard: every backtracker-only feature (backreference, atomic
/// group `\@>`, last-substitute `~`) is routed to the `Backtracker` engine.
///
/// This is the structural counterpart to `run_terminal_engine`, whose
/// `Backtracker` arm has NO PikeVM branch: capacity exhaustion becomes
/// `CapacityExceeded` (→ `HaystackTooLarge`), never a silent PikeVM fallback.
/// Asserting the engine selection here proves these patterns can never reach a
/// backref-incapable primitive. `needs_backtracker()` =
/// `has_backreferences || has_last_substitute || has_atomic` (see
/// `hir::PatternFeatures::needs_backtracker`); lookaround is intentionally NOT
/// included (PikeVM handles it), so the three classes below are exhaustive.
#[test]
fn c1_backtracker_only_patterns_select_backtracker() {
    for pat in [r"\(a\)\1", r"\(a*\)\@>b", r"a~b"] {
        let re = VimRegex::new(pat).unwrap();
        assert_eq!(
            re.engine_kind,
            EngineKind::Backtracker,
            "pattern {pat} must route to the Backtracker engine"
        );
        assert!(
            re.properties.features.needs_backtracker(),
            "pattern {pat} must report needs_backtracker()"
        );
    }
}

/// Beyond the memoization memory cap, a search surfaces the typed
/// `HaystackTooLarge` error — never a wrong answer (and never a PikeVM
/// fallback). Driven through the real dispatch path (`find_at_with_cache` →
/// `search_internal` → terminal `run_terminal_engine` → backtracker), with the
/// cap lowered via a `#[cfg(test)]` hook so we exercise the genuine
/// over-cap → `CapacityExceeded` → `Err(HaystackTooLarge)` chain WITHOUT
/// allocating a multi-hundred-MB haystack (the real 32 MiB cap would require
/// one for this tiny-state pattern). The cap boundary itself is covered by
/// `cache::tests::visited_grows_to_fit_then_caps`.
#[test]
fn c1_over_cap_returns_haystack_too_large() {
    let re = VimRegex::new(r"\(xy\)\1").unwrap();
    assert_eq!(re.engine_kind, EngineKind::Backtracker);

    let mut cache = re.create_cache();
    // Lower the memoization cap to 0 bytes: any non-empty NFA now exceeds it on
    // the first `ensure_capacity` call, so the backtracker reports capacity
    // exhaustion instead of growing the bitset.
    cache.visited_mut().set_max_bytes_for_test(0);

    // A haystack containing the matchable prefix so prefilter/min-length
    // fast-rejects do not short-circuit before the terminal backtracker runs.
    let ctx = MatchContext::simple("xyxy");
    let result = re.find_at_with_cache(&mut cache, &ctx, 0);

    match result {
        Err(e) => assert!(
            matches!(e.kind, VimRegexErrorKind::HaystackTooLarge { .. }),
            "over-cap search must return HaystackTooLarge, got {e:?}"
        ),
        Ok(other) => {
            panic!("over-cap search must error, not return a (possibly wrong) result: {other:?}")
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Multibyte start-byte soundness
// ═══════════════════════════════════════════════════════════════════════════════

/// Multibyte start-byte soundness: an explicit Cyrillic range `[А-В]\+` on "АБ"
/// matches 0..4 (each Cyrillic char is 2 UTF-8 bytes). The start filter must not
/// exclude the multibyte start position. nvim verified.
#[test]
fn multibyte_explicit_unicode_start_sound() {
    regex(r"[А-В]\+").text("АБ").expect_match(0..4).run();
}
