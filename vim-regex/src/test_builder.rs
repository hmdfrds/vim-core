//! Fluent test builder for vim-regex — the `regex()` equivalent of vim-test's `vim()`.
//!
//! Every `.run()` call automatically checks 19 invariants covering API consistency,
//! range validity, capture containment, engine equivalence, start-filter
//! soundness, and compilation determinism. No opt-in required.
//!
//! # Usage
//!
//! ```ignore
//! regex(r"\d\+").text("abc123").expect_match(3..6).run();
//! regex(r"\d").text("abc").expect_no_match().run();
//! regex("").expect_compile_error().run();
//! ```

use std::ops::Range;

use crate::engine::{SearchConfig, VimRegex};
use crate::matchers::MatchContext;
use crate::MagicMode;
use crate::VimMatch;

// ═══════════════════════════════════════════════════════════════════════════════
// EXPECTED — what the user asserts
// ═══════════════════════════════════════════════════════════════════════════════

/// Tracks user-specified expectations for a regex test.
#[derive(Debug, Default)]
struct Expected {
    /// Expected forward match range, if any.
    forward_match: Option<ExpectedMatch>,
    /// Expected find_all ranges, if any.
    all_matches: Option<Vec<Range<usize>>>,
    /// Expected capture groups (1-indexed group number -> range).
    captures: Vec<(usize, Range<usize>)>,
    /// Expected non-participating capture groups (1-indexed group numbers).
    no_captures: Vec<usize>,
    /// Expected backward match range, if any.
    backward_match: Option<ExpectedMatch>,
    /// Expected find_at(start) result, if any.
    match_from: Option<(usize, Range<usize>)>,
    /// Expected full_range (before \zs/\ze narrowing).
    full_range: Option<Range<usize>>,
    /// Expected matched text (convenience for `&input[match.range]`).
    matched_text: Option<String>,
    /// Whether compilation should fail.
    compile_error: bool,
    /// Expected compile error kind (Debug format substring match).
    compile_error_kind: Option<String>,
}

/// Represents either "expect a match at this range" or "expect no match".
#[derive(Debug, Clone)]
enum ExpectedMatch {
    Match(Range<usize>),
    NoMatch,
}

// ═══════════════════════════════════════════════════════════════════════════════
// BUILDER
// ═══════════════════════════════════════════════════════════════════════════════

/// Fluent builder for regex tests. Created via [`regex()`].
pub struct RegexTestBuilder {
    pattern: String,
    text: Option<String>,
    cursor: Option<usize>,
    case_sensitive: Option<bool>,
    magic_mode: Option<MagicMode>,
    expected: Expected,
    /// Skip invariant 5 (find_backward presence agreement).
    skip_backward: bool,
    /// Skip invariant 6 (find_at at every char boundary).
    skip_find_at: bool,
}

/// Entry point: create a regex test for the given pattern.
///
/// # Example
/// ```ignore
/// regex(r"\d\+").text("abc123").expect_match(3..6).run();
/// ```
#[must_use]
pub fn regex(pattern: &str) -> RegexTestBuilder {
    RegexTestBuilder {
        pattern: pattern.to_string(),
        text: None,
        cursor: None,
        case_sensitive: None,
        magic_mode: None,
        expected: Expected::default(),
        skip_backward: false,
        skip_find_at: false,
    }
}

#[allow(
    dead_code,
    reason = "public API for test modules — not all methods exercised by self-tests"
)]
impl RegexTestBuilder {
    // ─── Configuration ────────────────────────────────────────────────────

    /// Set the search text.
    #[must_use]
    pub fn text(mut self, text: &str) -> Self {
        self.text = Some(text.to_string());
        self
    }

    /// Set the cursor position (byte offset).
    #[must_use]
    pub fn cursor(mut self, pos: usize) -> Self {
        self.cursor = Some(pos);
        self
    }

    /// Set case-insensitive matching.
    #[must_use]
    pub fn case_insensitive(mut self) -> Self {
        self.case_sensitive = Some(false);
        self
    }

    /// Set the magic mode.
    #[must_use]
    pub fn magic(mut self, mode: MagicMode) -> Self {
        self.magic_mode = Some(mode);
        self
    }

    // ─── Expectations ─────────────────────────────────────────────────────

    /// Expect a forward match at the given range.
    #[must_use]
    pub fn expect_match(mut self, range: Range<usize>) -> Self {
        self.expected.forward_match = Some(ExpectedMatch::Match(range));
        self
    }

    /// Expect no forward match.
    #[must_use]
    pub fn expect_no_match(mut self) -> Self {
        self.expected.forward_match = Some(ExpectedMatch::NoMatch);
        self
    }

    /// Expect find_all to return exactly these ranges.
    #[must_use]
    pub fn expect_all_matches(mut self, ranges: &[Range<usize>]) -> Self {
        self.expected.all_matches = Some(ranges.to_vec());
        self
    }

    /// Expect a capture group (1-indexed) at the given range.
    #[must_use]
    pub fn expect_capture(mut self, group: usize, range: Range<usize>) -> Self {
        assert!(
            group >= 1,
            "capture groups are 1-indexed; group 0 is the match range itself"
        );
        self.expected.captures.push((group, range));
        self
    }

    /// Expect a backward match at the given range.
    #[must_use]
    pub fn expect_backward_match(mut self, range: Range<usize>) -> Self {
        self.expected.backward_match = Some(ExpectedMatch::Match(range));
        self
    }

    /// Expect no backward match.
    #[must_use]
    pub fn expect_no_backward_match(mut self) -> Self {
        self.expected.backward_match = Some(ExpectedMatch::NoMatch);
        self
    }

    /// Expect `find_at(start)` to return a match at the given range.
    #[must_use]
    pub fn expect_match_from(mut self, start: usize, range: Range<usize>) -> Self {
        self.expected.match_from = Some((start, range));
        self
    }

    /// Expect compilation to fail.
    #[must_use]
    pub fn expect_compile_error(mut self) -> Self {
        self.expected.compile_error = true;
        self
    }

    /// Expect compilation to fail with a specific error kind (Debug format substring).
    #[must_use]
    pub fn expect_compile_error_kind(mut self, kind_str: &str) -> Self {
        self.expected.compile_error = true;
        self.expected.compile_error_kind = Some(kind_str.to_string());
        self
    }

    /// Assert `VimMatch.full_range` equals the given range.
    /// Needed for `\zs`/`\ze` tests where `range` is narrowed but `full_range`
    /// is the full match extent.
    #[must_use]
    pub fn expect_full_range(mut self, range: Range<usize>) -> Self {
        self.expected.full_range = Some(range);
        self
    }

    /// Assert that capture group N did NOT participate (is `None`).
    #[must_use]
    pub fn expect_no_capture(mut self, group: usize) -> Self {
        assert!(
            group >= 1,
            "capture groups are 1-indexed; group 0 is the match range itself"
        );
        self.expected.no_captures.push(group);
        self
    }

    /// Convenience: assert `&input_text[match.range]` equals the expected string.
    #[must_use]
    pub fn expect_matched_text(mut self, text: &str) -> Self {
        self.expected.matched_text = Some(text.to_string());
        self
    }

    /// Skip invariant 5 (find_backward presence agreement).
    ///
    /// Some zero-width assertion-only patterns (like `^` on empty text, `$` on
    /// "hello") legitimately have `find()` match but `find_backward()` return
    /// `None`.
    #[must_use]
    pub fn skip_backward_check(mut self) -> Self {
        self.skip_backward = true;
        self
    }

    /// Skip invariant 6 (find_at at every char boundary).
    ///
    /// Patterns with `\zs` have a pre-existing limitation where
    /// `find_at(offset)` returns `None` for offsets before the effective match
    /// start.
    #[must_use]
    pub fn skip_find_at_check(mut self) -> Self {
        self.skip_find_at = true;
        self
    }

    // ─── Execution ────────────────────────────────────────────────────────

    /// Run the test, checking all 19 invariants plus user expectations.
    ///
    /// Panics on any invariant or expectation violation, with a descriptive
    /// error message showing pattern, text, cursor, and expected vs actual.
    #[track_caller]
    pub fn run(self) {
        let label = self.make_label();
        let magic = self.magic_mode.unwrap_or(MagicMode::Magic);

        // ── Compile ──────────────────────────────────────────────────

        let compile_result = VimRegex::with_magic(&self.pattern, magic);

        if self.expected.compile_error {
            match compile_result {
                Err(ref e) => {
                    // Invariant 17: compilation doesn't panic (already handled — we're here).
                    if let Some(ref expected_kind) = self.expected.compile_error_kind {
                        let debug = format!("{:?}", e.kind);
                        assert!(
                            debug.contains(expected_kind),
                            "{label}\nexpected compile error kind containing \"{expected_kind}\"\n\
                             got: {debug}"
                        );
                    }
                    return; // Error path done — no further checks.
                }
                Ok(_) => {
                    panic!(
                        "{label}\nexpected compilation error, but pattern compiled successfully"
                    );
                }
            }
        }

        let re =
            compile_result.unwrap_or_else(|e| panic!("{label}\nunexpected compile error: {e}"));

        // If no text was provided, only compilation was tested.
        let text = match self.text {
            Some(ref t) => t.as_str(),
            None => return,
        };

        // ── Build context ────────────────────────────────────────────

        let ctx = self.build_context(text);
        let backward_cursor = self.cursor.unwrap_or(text.len());
        let backward_ctx = MatchContext::with_cursor(text, backward_cursor);

        // ── Eagerly run ALL search methods ───────────────────────────

        let find_result = re
            .find(&ctx)
            .unwrap_or_else(|e| panic!("{label}\nfind() engine error: {e}"));

        let find_at_0_result = re
            .find_at(&ctx, 0)
            .unwrap_or_else(|e| panic!("{label}\nfind_at(0) engine error: {e}"));

        let find_all_result = re
            .find_all(&ctx)
            .unwrap_or_else(|e| panic!("{label}\nfind_all() engine error: {e}"));

        let find_backward_result = re
            .find_backward(&backward_ctx)
            .unwrap_or_else(|e| panic!("{label}\nfind_backward() engine error: {e}"));

        let is_match_result = re
            .is_match(&ctx)
            .unwrap_or_else(|e| panic!("{label}\nis_match() engine error: {e}"));

        // ══════════════════════════════════════════════════════════════
        // 19 AUTOMATIC INVARIANTS
        // ══════════════════════════════════════════════════════════════

        // ── API Consistency (invariants 1-6) ──────────────────────────

        // 1. find().is_some() == is_match()
        assert_eq!(
            find_result.is_some(),
            is_match_result,
            "{label}\ninvariant 1: find().is_some() ({}) != is_match() ({is_match_result})",
            find_result.is_some()
        );

        // 2. find() == find_at(0)
        assert_eq!(
            find_result, find_at_0_result,
            "{label}\ninvariant 2: find() != find_at(0)\n\
             find():      {find_result:?}\n\
             find_at(0):  {find_at_0_result:?}"
        );

        // 3. find_all()[0].range == find().range (when both exist)
        if let Some(ref find_m) = find_result {
            assert!(
                !find_all_result.is_empty(),
                "{label}\ninvariant 3: find() returned a match but find_all() is empty"
            );
            assert_eq!(
                find_m.range, find_all_result[0].range,
                "{label}\ninvariant 3: find().range ({:?}) != find_all()[0].range ({:?})",
                find_m.range, find_all_result[0].range
            );
        }

        // 4. find_all() results are non-overlapping and monotonically increasing
        for window in find_all_result.windows(2) {
            assert!(
                window[0].range.end <= window[1].range.start,
                "{label}\ninvariant 4: find_all() overlap or non-monotonic: \
                 [{:?}] then [{:?}]",
                window[0].range,
                window[1].range
            );
        }

        // 5. find_backward() presence agrees with find()
        //    (if find() found something, backward should too, and vice versa)
        if !self.skip_backward {
            if find_result.is_some() {
                assert!(
                    find_backward_result.is_some(),
                    "{label}\ninvariant 5: find() found a match but find_backward() did not"
                );
            }
            // Note: we don't assert the reverse because backward search uses a different
            // cursor position and may legitimately not find anything if cursor is at 0.
        }

        // 6. find_at() at every char boundary <= match.start (gated: text <= 200 bytes)
        if !self.skip_find_at {
            if text.len() <= 200 {
                if let Some(ref find_m) = find_result {
                    for offset in char_boundary_offsets(text) {
                        if offset > find_m.range.start {
                            break;
                        }
                        let at_result = re.find_at(&ctx, offset).unwrap_or_else(|e| {
                            panic!("{label}\ninvariant 6: find_at({offset}) error: {e}")
                        });
                        assert_eq!(
                            at_result.as_ref().map(|m| &m.range),
                            Some(&find_m.range),
                            "{label}\ninvariant 6: find_at({offset}).range != find().range\n\
                             find_at({offset}): {at_result:?}\n\
                             find():           {find_result:?}"
                        );
                    }
                }
            }
        }

        // ── Range Validity (invariants 7-10) ─────────────────────────

        // Lookaround patterns can export captures from inside a lookahead/
        // lookbehind that lie OUTSIDE the match range (Vim semantics: a lookbehind
        // group is behind the cursor, a lookahead group ahead of it). Invariant 13
        // (capture containment in full_range) is therefore relaxed for these
        // patterns; it still holds in full for every non-lookaround pattern.
        let has_lookaround = re.properties.has_lookaround();

        // Check all matches from all methods.
        if let Some(ref m) = find_result {
            assert_match_valid(m, text, "find()", &label, has_lookaround);
        }
        if let Some(ref m) = find_at_0_result {
            assert_match_valid(m, text, "find_at(0)", &label, has_lookaround);
        }
        for (i, m) in find_all_result.iter().enumerate() {
            assert_match_valid(m, text, &format!("find_all()[{i}]"), &label, has_lookaround);
        }
        if let Some(ref m) = find_backward_result {
            assert_match_valid(m, text, "find_backward()", &label, has_lookaround);
        }

        // ── Engine Equivalence (invariants 14-15) ────────────────────

        let baseline_config = SearchConfig::baseline();
        let baseline_result =
            VimRegex::with_magic_and_config(&self.pattern, magic, &baseline_config);

        if let Ok(baseline_re) = baseline_result {
            // 14. Optimized vs baseline find/is_match/find_all
            let baseline_find = baseline_re
                .find(&ctx)
                .unwrap_or_else(|e| panic!("{label}\ninvariant 14: baseline find() error: {e}"));
            let baseline_is_match = baseline_re.is_match(&ctx).unwrap_or_else(|e| {
                panic!("{label}\ninvariant 14: baseline is_match() error: {e}")
            });
            let baseline_find_all = baseline_re.find_all(&ctx).unwrap_or_else(|e| {
                panic!("{label}\ninvariant 14: baseline find_all() error: {e}")
            });

            assert_eq!(
                find_result.as_ref().map(|m| &m.range),
                baseline_find.as_ref().map(|m| &m.range),
                "{label}\ninvariant 14: optimized find().range != baseline find().range\n\
                 optimized: {find_result:?}\n\
                 baseline:  {baseline_find:?}"
            );
            assert_eq!(
                is_match_result, baseline_is_match,
                "{label}\ninvariant 14: optimized is_match ({is_match_result}) \
                 != baseline is_match ({baseline_is_match})"
            );

            let opt_ranges: Vec<_> = find_all_result.iter().map(|m| &m.range).collect();
            let base_ranges: Vec<_> = baseline_find_all.iter().map(|m| &m.range).collect();
            assert_eq!(
                opt_ranges, base_ranges,
                "{label}\ninvariant 14: optimized find_all ranges != baseline find_all ranges\n\
                 optimized: {opt_ranges:?}\n\
                 baseline:  {base_ranges:?}"
            );

            // 15. Backward presence agreement
            let baseline_backward = baseline_re
                .find_backward(&backward_ctx)
                .unwrap_or_else(|e| {
                    panic!("{label}\ninvariant 15: baseline find_backward() error: {e}")
                });
            assert_eq!(
                find_backward_result.is_some(),
                baseline_backward.is_some(),
                "{label}\ninvariant 15: optimized find_backward().is_some() ({}) \
                 != baseline ({}).\n\
                 optimized: {find_backward_result:?}\n\
                 baseline:  {baseline_backward:?}",
                find_backward_result.is_some(),
                baseline_backward.is_some()
            );

            // 18. Optimized vs baseline CAPTURES.
            // Closes CORRECT-02: engine equivalence previously checked ranges only,
            // which is exactly why the capture-renumbering bug shipped.
            // Checked for find() and for every find_all() match present in both
            // engines, not just the first match.
            if let (Some(opt_m), Some(base_m)) = (find_result.as_ref(), baseline_find.as_ref()) {
                let opt_caps = captures_vec(opt_m);
                let base_caps = captures_vec(base_m);
                assert_eq!(
                    opt_caps, base_caps,
                    "{label}\ninvariant 18: optimized captures != baseline captures (find())\n\
                     optimized: {opt_caps:?}\n baseline:  {base_caps:?}"
                );
            }
            for (mi, (opt_m, base_m)) in find_all_result
                .iter()
                .zip(baseline_find_all.iter())
                .enumerate()
            {
                let opt_caps = captures_vec(opt_m);
                let base_caps = captures_vec(base_m);
                assert_eq!(
                    opt_caps, base_caps,
                    "{label}\ninvariant 18: optimized captures != baseline captures \
                     (find_all() match {mi})\n\
                     optimized: {opt_caps:?}\n baseline:  {base_caps:?}"
                );
            }
        }
        // If baseline compilation fails (unsupported feature), skip equivalence.

        // ── Start-filter Soundness (invariant 19) ────────────────────

        // 19. Start-filter SOUNDNESS: the start bitmap / prefilter must never
        // exclude a position where an unfiltered scan finds a match. We compile
        // the SAME pattern with every start-position / fast-reject accelerator
        // disabled (and only the terminal engine-dispatch strategy active), then
        // require the filtered `find()` to agree with that unfiltered ground
        // truth. A divergence means the start filter pruned a real match.
        {
            let unfiltered_cfg = SearchConfig::without_prefilters();
            if let Ok(unfiltered) =
                VimRegex::with_magic_and_config(&self.pattern, magic, &unfiltered_cfg)
            {
                let unfiltered_find = unfiltered.find(&ctx).unwrap_or_else(|e| {
                    panic!("{label}\ninvariant 19: unfiltered find() error: {e}")
                });
                assert_eq!(
                    find_result.as_ref().map(|m| &m.range),
                    unfiltered_find.as_ref().map(|m| &m.range),
                    "{label}\ninvariant 19: filtered find() != unfiltered find() — start filter excluded a real match\n\
                     filtered:   {find_result:?}\n unfiltered: {unfiltered_find:?}"
                );
            }
        }

        // ── Compilation (invariants 16-17) ───────────────────────────

        // 16. Pattern string roundtrip: re.as_str() -> recompile -> same find
        {
            let roundtrip_pattern = re.as_str();
            let roundtrip_re = VimRegex::with_magic(roundtrip_pattern, magic).unwrap_or_else(|e| {
                panic!(
                    "{label}\ninvariant 16: roundtrip recompile failed for {:?}: {e}",
                    roundtrip_pattern
                )
            });
            let roundtrip_find = roundtrip_re
                .find(&ctx)
                .unwrap_or_else(|e| panic!("{label}\ninvariant 16: roundtrip find() error: {e}"));
            assert_eq!(
                find_result.as_ref().map(|m| &m.range),
                roundtrip_find.as_ref().map(|m| &m.range),
                "{label}\ninvariant 16: original find().range != roundtrip find().range\n\
                 original:  {find_result:?}\n\
                 roundtrip: {roundtrip_find:?}"
            );
        }

        // 17. Compilation does not panic — already handled by reaching this point.

        // ══════════════════════════════════════════════════════════════
        // USER EXPECTATIONS
        // ══════════════════════════════════════════════════════════════

        // Forward match
        if let Some(ref expected) = self.expected.forward_match {
            match expected {
                ExpectedMatch::Match(expected_range) => {
                    let m = find_result.as_ref().unwrap_or_else(|| {
                        panic!(
                            "{label}\nexpected match at {expected_range:?}, but find() returned None"
                        )
                    });
                    assert_eq!(
                        &m.range, expected_range,
                        "{label}\nexpected match at {expected_range:?}, got {:?}",
                        m.range
                    );
                }
                ExpectedMatch::NoMatch => {
                    assert!(
                        find_result.is_none(),
                        "{label}\nexpected no match, but find() returned {:?}",
                        find_result.as_ref().map(|m| &m.range)
                    );
                }
            }
        }

        // All matches
        if let Some(ref expected_ranges) = self.expected.all_matches {
            let actual_ranges: Vec<_> = find_all_result.iter().map(|m| m.range.clone()).collect();
            assert_eq!(
                &actual_ranges, expected_ranges,
                "{label}\nexpected find_all ranges: {expected_ranges:?}\n\
                 got: {actual_ranges:?}"
            );
        }

        // Captures
        for &(group, ref expected_range) in &self.expected.captures {
            let m = find_result.as_ref().unwrap_or_else(|| {
                panic!(
                    "{label}\nexpected capture({group}) at {expected_range:?}, \
                     but find() returned no match"
                )
            });
            let actual = m.capture(group).unwrap_or_else(|| {
                panic!(
                    "{label}\nexpected capture({group}) at {expected_range:?}, \
                     but capture({group}) returned None"
                )
            });
            assert_eq!(
                actual, expected_range,
                "{label}\ncapture({group}): expected {expected_range:?}, got {actual:?}"
            );
        }

        // Backward match
        if let Some(ref expected) = self.expected.backward_match {
            match expected {
                ExpectedMatch::Match(expected_range) => {
                    let m = find_backward_result.as_ref().unwrap_or_else(|| {
                        panic!(
                            "{label}\nexpected backward match at {expected_range:?}, \
                             but find_backward() returned None"
                        )
                    });
                    assert_eq!(
                        &m.range, expected_range,
                        "{label}\nexpected backward match at {expected_range:?}, got {:?}",
                        m.range
                    );
                }
                ExpectedMatch::NoMatch => {
                    assert!(
                        find_backward_result.is_none(),
                        "{label}\nexpected no backward match, but find_backward() returned {:?}",
                        find_backward_result.as_ref().map(|m| &m.range)
                    );
                }
            }
        }

        // Match from specific position
        if let Some((start, ref expected_range)) = self.expected.match_from {
            let at_result = re
                .find_at(&ctx, start)
                .unwrap_or_else(|e| panic!("{label}\nfind_at({start}) engine error: {e}"));
            let m = at_result.as_ref().unwrap_or_else(|| {
                panic!(
                    "{label}\nexpected find_at({start}) match at {expected_range:?}, \
                     but returned None"
                )
            });
            assert_eq!(
                &m.range, expected_range,
                "{label}\nfind_at({start}): expected {expected_range:?}, got {:?}",
                m.range
            );
        }

        // Full range
        if let Some(ref expected_full) = self.expected.full_range {
            if let Some(ref m) = find_result {
                assert_eq!(
                    &m.full_range, expected_full,
                    "{label}\nexpected full_range {expected_full:?}, got {:?}",
                    m.full_range
                );
            } else {
                panic!(
                    "{label}\nexpected full_range {expected_full:?}, \
                     but find() returned no match"
                );
            }
        }

        // Non-participating captures
        for &group in &self.expected.no_captures {
            if let Some(ref m) = find_result {
                assert!(
                    m.capture(group).is_none(),
                    "{label}\nexpected capture({group}) to be None, \
                     but got {:?}",
                    m.capture(group)
                );
            } else {
                panic!(
                    "{label}\nexpected capture({group}) to be None, \
                     but find() returned no match"
                );
            }
        }

        // Matched text
        if let Some(ref expected_text) = self.expected.matched_text {
            if let Some(ref m) = find_result {
                let actual = &text[m.range.clone()];
                assert_eq!(
                    actual,
                    expected_text.as_str(),
                    "{label}\nexpected matched text {:?}, got {:?}",
                    expected_text,
                    actual
                );
            } else {
                panic!(
                    "{label}\nexpected matched text {:?}, \
                     but find() returned no match",
                    expected_text
                );
            }
        }
    }

    // ─── Helpers ──────────────────────────────────────────────────────

    /// Build a `MatchContext` from the builder's configuration.
    fn build_context<'a>(&self, text: &'a str) -> MatchContext<'a> {
        let mut builder = MatchContext::builder(text);
        if let Some(cursor) = self.cursor {
            builder = builder.cursor(cursor);
        }
        if let Some(case_sensitive) = self.case_sensitive {
            builder = builder.case_sensitive(case_sensitive);
        }
        builder.build()
    }

    /// Build a human-readable label for error messages.
    fn make_label(&self) -> String {
        let mut parts = vec![format!("pattern: {:?}", self.pattern)];
        if let Some(ref text) = self.text {
            parts.push(format!("text:    {:?}", text));
        }
        if let Some(cursor) = self.cursor {
            parts.push(format!("cursor:  {cursor}"));
        }
        if let Some(case_sensitive) = self.case_sensitive {
            parts.push(format!(
                "case:    {}",
                if case_sensitive {
                    "sensitive"
                } else {
                    "insensitive"
                }
            ));
        }
        if let Some(ref mode) = self.magic_mode {
            parts.push(format!("magic:   {mode:?}"));
        }
        format!(
            "\n--- regex test failure ---\n{}\n--------------------------",
            parts.join("\n")
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Collect a match's capture groups (1-indexed) as owned ranges, preserving
/// non-participation (`None`). Used by invariant 18 to compare optimized vs
/// baseline captures for equality.
fn captures_vec(m: &VimMatch) -> Vec<Option<Range<usize>>> {
    m.captures_iter().map(|c| c.cloned()).collect()
}

/// Check range validity and capture containment for a single `VimMatch`.
///
/// Covers invariants 7-13:
///   7. range.start <= range.end <= text.len()
///   8. range char boundaries
///   9. full_range contains range
///  10. full_range char boundaries
///  11. capture range bounds
///  12. capture char boundaries
///  13. capture containment within full_range (skipped for lookaround patterns,
///      whose exported lookaround captures may legitimately lie outside the match)
#[track_caller]
fn assert_match_valid(m: &VimMatch, text: &str, source: &str, label: &str, has_lookaround: bool) {
    // 7. range.start <= range.end <= text.len()
    assert!(
        m.range.start <= m.range.end && m.range.end <= text.len(),
        "{label}\ninvariant 7 [{source}]: invalid range {:?} for text of length {}",
        m.range,
        text.len()
    );

    // 8. range char boundaries
    assert!(
        text.is_char_boundary(m.range.start) && text.is_char_boundary(m.range.end),
        "{label}\ninvariant 8 [{source}]: range {:?} not on char boundaries",
        m.range
    );

    // 9. full_range contains range
    assert!(
        m.full_range.start <= m.range.start && m.range.end <= m.full_range.end,
        "{label}\ninvariant 9 [{source}]: full_range {:?} does not contain range {:?}",
        m.full_range,
        m.range
    );

    // 10. full_range char boundaries
    assert!(
        text.is_char_boundary(m.full_range.start) && text.is_char_boundary(m.full_range.end),
        "{label}\ninvariant 10 [{source}]: full_range {:?} not on char boundaries",
        m.full_range
    );

    // Captures: invariants 11-13
    for (i, cap) in m.captures_iter().enumerate() {
        if let Some(cap_range) = cap {
            let group = i + 1;

            // 11. capture range bounds
            assert!(
                cap_range.start <= cap_range.end && cap_range.end <= text.len(),
                "{label}\ninvariant 11 [{source}]: capture({group}) range {cap_range:?} \
                 invalid for text of length {}",
                text.len()
            );

            // 12. capture char boundaries
            assert!(
                text.is_char_boundary(cap_range.start) && text.is_char_boundary(cap_range.end),
                "{label}\ninvariant 12 [{source}]: capture({group}) range {cap_range:?} \
                 not on char boundaries"
            );

            // 13. capture containment within full_range.
            //
            // Relaxed for lookaround patterns: a positive lookaround exports its
            // sub-captures into the parent match by global index, and Vim places
            // those captures OUTSIDE the match range (a lookbehind group is behind
            // the cursor, e.g. `\(foo\)\@<=bar` on "foobar" → match "bar" 3..6 with
            // group 1 = "foo" 0..3; a lookahead group can extend past the match).
            // Invariants 11/12 (valid, char-boundary range) still hold above.
            if !has_lookaround {
                assert!(
                    m.full_range.start <= cap_range.start && cap_range.end <= m.full_range.end,
                    "{label}\ninvariant 13 [{source}]: capture({group}) range {cap_range:?} \
                     not contained in full_range {:?}",
                    m.full_range
                );
            }
        }
    }
}

/// Returns all valid char boundary byte offsets in `text`, including 0 and text.len().
fn char_boundary_offsets(text: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(text.len() + 1);
    for i in 0..=text.len() {
        if text.is_char_boundary(i) {
            offsets.push(i);
        }
    }
    offsets
}

// ═══════════════════════════════════════════════════════════════════════════════
// SELF-TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::regex;

    #[test]
    fn basic_match() {
        regex(r"\d\+").text("abc123").expect_match(3..6).run();
    }

    #[test]
    fn no_match() {
        regex(r"\d").text("abc").expect_no_match().run();
    }

    #[test]
    fn compile_error() {
        regex("").expect_compile_error().run();
    }

    #[test]
    fn find_all() {
        regex(r"\d\+")
            .text("a1b23c456")
            .expect_all_matches(&[1..2, 3..5, 6..9])
            .run();
    }

    #[test]
    fn captures() {
        regex(r"\(\w\+\)=\(\d\+\)")
            .text("x=42")
            .expect_match(0..4)
            .expect_capture(1, 0..1)
            .expect_capture(2, 2..4)
            .run();
    }

    #[test]
    fn case_insensitive_modifier() {
        regex(r"\cfoo").text("FOO").expect_match(0..3).run();
    }

    #[test]
    fn backward_search() {
        regex("abc")
            .text("abc xxx abc")
            .expect_backward_match(8..11)
            .run();
    }

    #[test]
    fn unicode_text() {
        // \w matches non-ASCII alphanumeric characters like é (2 bytes: 0xc3 0xa9).
        // So \w\+ matches "café" (0..5), including the accented character.
        regex(r"\w\+").text("caf\u{00e9}").expect_match(0..5).run();
    }

    #[test]
    fn multiline() {
        regex(r"^\w\+")
            .text("hello\nworld")
            .expect_match(0..5)
            .run();
    }

    #[test]
    fn very_magic() {
        regex("(a|b)+")
            .magic(crate::MagicMode::VeryMagic)
            .text("xxabba")
            .expect_match(2..6)
            .run();
    }

    #[test]
    fn expect_full_range_with_zs() {
        regex(r"foo\zsbar")
            .text("foobar")
            .expect_match(3..6)
            .expect_full_range(0..6)
            .skip_find_at_check()
            .run();
    }

    #[test]
    fn expect_no_capture() {
        // alternation: first branch has no group 1
        regex(r"abc\|\(\d\+\)")
            .text("abc")
            .expect_match(0..3)
            .expect_no_capture(1)
            .run();
    }

    #[test]
    fn expect_matched_text() {
        regex(r"\w\+")
            .text("hello world")
            .expect_matched_text("hello")
            .run();
    }

    #[test]
    fn skip_backward_for_zero_width() {
        regex("^")
            .text("")
            .expect_match(0..0)
            .skip_backward_check()
            .run();
    }

    #[test]
    fn skip_find_at_for_zs() {
        regex(r"foo\zsbar")
            .text("foobar")
            .expect_match(3..6)
            .skip_find_at_check()
            .run();
    }
}
