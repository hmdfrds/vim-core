//! Tabular test harness for vim-regex.
//!
//! Provides the [`regex_suite!`] macro for writing concise tabular tests that
//! delegate to the [`regex()`](crate::test_builder::regex) builder. Every test
//! entry becomes a standalone `#[test]` function, individually runnable via
//! `cargo test suite_name::test_name`.
//!
//! All invariant checking (18 automatic invariants) is handled by the builder's
//! `.run()` method — the runner functions here are intentionally thin wrappers.

// ═══════════════════════════════════════════════════════════════════════════════
// RUNNER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Assert that `pattern` matches `text` with the first match at `start..end`.
#[track_caller]
pub fn assert_match(pattern: &str, text: &str, start: usize, end: usize) {
    crate::test_builder::regex(pattern)
        .text(text)
        .expect_match(start..end)
        .run();
}

/// Assert that `pattern` does not match anywhere in `text`.
#[track_caller]
pub fn assert_no_match(pattern: &str, text: &str) {
    crate::test_builder::regex(pattern)
        .text(text)
        .expect_no_match()
        .run();
}

/// Assert that `pattern` finds all matches in `text` at exactly the given ranges.
#[track_caller]
pub fn assert_find_all(pattern: &str, text: &str, ranges: &[(usize, usize)]) {
    let ranges: Vec<_> = ranges.iter().map(|&(s, e)| s..e).collect();
    crate::test_builder::regex(pattern)
        .text(text)
        .expect_all_matches(&ranges)
        .run();
}

/// Assert that `pattern` compiles and all 18 invariants hold for `text`,
/// without asserting a specific match range. Useful for pure equivalence checks.
#[track_caller]
pub fn assert_equivalence(pattern: &str, text: &str) {
    crate::test_builder::regex(pattern).text(text).run();
}

/// Assert that `pattern` fails to compile.
#[track_caller]
pub fn assert_compile_error(pattern: &str) {
    crate::test_builder::regex(pattern)
        .expect_compile_error()
        .run();
}

// ═══════════════════════════════════════════════════════════════════════════════
// MACRO
// ═══════════════════════════════════════════════════════════════════════════════

/// Tabular test macro for vim-regex. Each entry becomes a separate `#[test]`
/// function inside a generated `mod`, individually runnable.
///
/// # Syntax
///
/// ```ignore
/// regex_suite!(suite_name {
///     // Match: first match at start..end
///     test_name: "pattern", "text" => (start, end);
///
///     // No match
///     test_name: "pattern", "text" => ();
///
///     // Find all: exact set of match ranges
///     test_name: "pattern", "text" => all[(0, 3), (4, 7)];
///
///     // Equivalence only: compile + run 18 invariants, no range assertion
///     test_name: "pattern", "text" => equiv;
///
///     // Compile error: pattern must fail to compile
///     test_name: "pattern", _ => error;
/// });
/// ```
///
/// # Example
///
/// ```ignore
/// regex_suite!(basics {
///     literal:  "abc", "xabcy" => (1, 4);
///     no_hit:   "xyz", "hello" => ();
///     digits:   r"\d\+", "abc123" => (3, 6);
/// });
///
/// // Run individually:
/// //   cargo test basics::literal
/// //   cargo test basics::no_hit
/// ```
macro_rules! regex_suite {
    // ── Entry: mod name + body ───────────────────────────────────────────
    ($suite:ident { $($body:tt)* }) => {
        mod $suite {
            crate::test_harness::regex_suite!(@parse $($body)*);
        }
    };

    // ── Terminal ─────────────────────────────────────────────────────────
    (@parse) => {};

    // ── Match: name: PATTERN, TEXT => (start, end); ─────────────────────
    (@parse $name:ident : $pat:expr, $text:expr => ($s:expr, $e:expr) ; $($rest:tt)*) => {
        #[test]
        fn $name() {
            crate::test_harness::assert_match($pat, $text, $s, $e);
        }
        crate::test_harness::regex_suite!(@parse $($rest)*);
    };

    // ── No match: name: PATTERN, TEXT => (); ────────────────────────────
    (@parse $name:ident : $pat:expr, $text:expr => () ; $($rest:tt)*) => {
        #[test]
        fn $name() {
            crate::test_harness::assert_no_match($pat, $text);
        }
        crate::test_harness::regex_suite!(@parse $($rest)*);
    };

    // ── Find all: name: PATTERN, TEXT => all[(s, e), ...]; ──────────────
    (@parse $name:ident : $pat:expr, $text:expr => all[ $( ($s:expr, $e:expr) ),* $(,)? ] ; $($rest:tt)*) => {
        #[test]
        fn $name() {
            crate::test_harness::assert_find_all($pat, $text, &[ $( ($s, $e) ),* ]);
        }
        crate::test_harness::regex_suite!(@parse $($rest)*);
    };

    // ── Equivalence only: name: PATTERN, TEXT => equiv; ─────────────────
    (@parse $name:ident : $pat:expr, $text:expr => equiv ; $($rest:tt)*) => {
        #[test]
        fn $name() {
            crate::test_harness::assert_equivalence($pat, $text);
        }
        crate::test_harness::regex_suite!(@parse $($rest)*);
    };

    // ── Compile error: name: PATTERN, _ => error; ───────────────────────
    (@parse $name:ident : $pat:expr, _ => error ; $($rest:tt)*) => {
        #[test]
        fn $name() {
            crate::test_harness::assert_compile_error($pat);
        }
        crate::test_harness::regex_suite!(@parse $($rest)*);
    };
}

// Re-export so other modules can use `crate::test_harness::regex_suite!`.
pub(crate) use regex_suite;

// ═══════════════════════════════════════════════════════════════════════════════
// SMOKE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod smoke {
    regex_suite!(basic_matching {
        literal:      "abc",     "xabcy"       => (1, 4);
        no_match:     "xyz",     "hello"       => ();
        digit:        r"\d\+",   "abc123"      => (3, 6);
        word_multi:   r"\w\+",   "a b c"       => all[(0, 1), (2, 3), (4, 5)];
        equiv_check:  r".*foo",  "prefix foo"  => equiv;
        empty_err:    "",        _             => error;
    });
}
