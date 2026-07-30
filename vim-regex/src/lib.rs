#![deny(unsafe_code)]
//! # vim-regex
//!
//! A Vim-compatible regular expression engine, implementing the full Vim regex
//! dialect as specified in `:help pattern`. This crate is the regex backend for
//! the vim-core editor engine.
//!
//! ## Compilation Pipeline
//!
//! ```text
//! Pattern string
//!   -> Parser (magic-mode-aware tokenizer)
//!   -> IR (VimPatternNode tree -- the AST)
//!   -> HIR lowering (normalize quantifiers, expand classes, compute properties)
//!   -> NFA construction (Thompson's construction with capture slots)
//!   -> Prefilter extraction (memchr, Aho-Corasick for literal prefixes/suffixes)
//! ```
//!
//! ## Strategy Cascade
//!
//! At search time, the meta-engine applies an ordered cascade of strategies,
//! using the fastest applicable approach for each pattern:
//!
//! 1. **Pure literal bypass** -- patterns with no metacharacters use memchr/memmem.
//! 2. **Aho-Corasick** -- literal alternations (`foo\|bar\|baz`) use multi-pattern matching.
//! 3. **Anchored fast path** -- `\%^`-anchored patterns only try position 0.
//! 4. **Reverse NFA** -- suffix-guided or inner-literal-guided backward scan.
//! 5. **Lazy DFA** -- byte-level DFA with on-demand state construction.
//! 6. **Pike VM** -- parallel NFA simulation (guaranteed linear time).
//! 7. **Backtracker** -- bounded DFS for backreference / atomic / last-substitute
//!    patterns. Its memoization grows to fit the haystack (bounded by a 32 MiB
//!    cap); over the cap a search surfaces `HaystackTooLarge` rather than falling
//!    back to the backref-incapable Pike VM.
//!
//! ## Usage
//!
//! ```rust
//! use vim_regex::{VimRegex, MatchContext, Cache};
//!
//! // Compile a pattern
//! let regex = VimRegex::new(r"\<\w\+\>").unwrap();
//!
//! // Create a reusable cache (avoids per-call allocation)
//! let mut cache = regex.create_cache();
//!
//! // Search
//! let ctx = MatchContext::simple("hello world");
//! let m = regex.find_at_with_cache(&mut cache, &ctx, 0).unwrap().unwrap();
//! assert_eq!(m.range, 0..5);
//!
//! // Iterate all matches
//! for m in regex.find_iter(&mut cache, &ctx) {
//!     println!("match at {:?}", m.range);
//! }
//! ```
//!
//! ## Supported Vim Features
//!
//! - All four magic modes: `\m` (magic), `\M` (nomagic), `\v` (very magic), `\V` (very nomagic)
//! - Character classes: `\d`, `\w`, `\s`, `\a`, `\l`, `\u`, `\x`, `\h`, `\f`, `\k`, `\i`, `\p`, `\o` (+ uppercase negations)
//! - POSIX classes: `[:alpha:]`, `[:digit:]`, etc.
//! - Collections: `[abc]`, `[^abc]`, `[a-z]`, `\_[...]`
//! - Quantifiers: `*`, `\+`, `\?`, `\{n,m}`, `\{-}` (non-greedy)
//! - Groups: `\(...\)` (capturing), `\%(...\)` (non-capturing)
//! - Alternation: `\|`
//! - Anchors: `^`, `$`, `\<`, `\>`, `\%^`, `\%$`
//! - Match narrowing: `\zs`, `\ze`
//! - Lookarounds: `\@=`, `\@!`, `\@<=`, `\@<!`
//! - Atomic groups: `\@>`
//! - Backreferences: `\1` through `\9`
//! - Case modifiers: `\c` (insensitive), `\C` (sensitive)
//! - Buffer positions: `\%l`, `\%c`, `\%v`, `\%#`, `\%V`, `\%'m`
//! - Character codes: `\%d`, `\%x`, `\%u`, `\%U`
//! - Optional sequence: `\%[atoms]`
//! - Last substitute: `~`
//!
//! ## Choosing the Right Search Method
//!
//! | Method | Allocates cache? | Use case |
//! |--------|-----------------|----------|
//! | `find`, `find_at`, `find_backward`, `find_all`, `is_match` | Yes (fresh per call) | One-shot searches |
//! | `find_with_cache`, `find_at_with_cache`, etc. | No (caller provides) | Repeated searches in a loop |
//! | `find_backward_in_range`, `find_backward_in_range_with_cache` | See name | Backward within a sub-range |
//!
//! For hot loops, create a cache once with [`VimRegex::create_cache()`] and
//! pass it to the `_with_cache` variants to amortise allocation cost.

mod magic_mode;
pub use magic_mode::MagicMode;

mod accel;
mod cache;
mod common;
mod diagnostics;
mod engine;
mod engines;
mod hir;
mod ir;
mod matchers;
mod nfa;
mod parser;
/// Vim replacement string processor.
pub mod replacement;

/// IR types for pattern introspection and analysis.
///
/// These types are re-exported from the root for backward compatibility,
/// but new code should prefer importing from `vim_regex::ast`.
pub mod ast {
    pub use crate::ir::{
        AuxiliarySpan, CaseMode, CharClass, CollectionItem, ColumnSpec, ComposingMode, EscapeKind,
        LineSpec, LookaroundKind, MarkRel, Span, VimPatternNode,
    };
}

pub use cache::bloom::{extract_trigrams, LineBloomFilter, LineBloomStore};
pub use cache::checkpoints::SearchCheckpoints;
pub use cache::Cache;
pub use common::MemoryConfig;
pub use diagnostics::DiagnosticRenderer;
pub use engine::pattern_stats::{PatternStats, Promotion, StrategyKind};
#[doc(hidden)]
pub use engine::SearchConfig;
pub use engine::{CacheWithGuard, Matches, VimRegex};
pub use ir::{
    AuxiliarySpan, CaseMode, CharClass, CollectionItem, ColumnSpec, ComposingMode, EscapeKind,
    LineSpec, LookaroundKind, MarkRel, PatternFeatures, PercentEscapeContext, Span, VimPatternNode,
    VimRegexError, VimRegexErrorKind,
};
pub use matchers::{
    LineResolver, MarkResolver, MatchContext, MatchContextBuilder, SingleLineResolver,
};
pub use replacement::{apply_replacement, apply_replacement_with_evaluator, ReplacementEvaluator};

use smallvec::SmallVec;
use std::ops::Range;

use common::MAX_CAPTURE_GROUPS;

// ═══════════════════════════════════════════════════════════════════════════════
// SEND/SYNC COMPILE-TIME ASSERTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Static assertions that core public types implement the expected
/// auto-traits. These assertions are checked at compile time -- they
/// have zero runtime cost.
///
/// - `VimRegex`: `Send + Sync` (can be shared across threads).
/// - `Cache`: `Send` (can be moved between threads, but not shared).
/// - `VimMatch`: `Send + Sync` (plain data).
/// - `VimRegexError`: `Send + Sync` (errors must be sendable).
///
/// `CacheWithGuard` is intentionally `!Send` because it holds an
/// `Rc<VimRegex>` for the thread-local compile cache return-on-drop.
const _: () = {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    // VimRegex is Send+Sync: compiled patterns can be shared across threads.
    assert_send::<VimRegex>();
    assert_sync::<VimRegex>();

    // Cache is Send: can be moved between threads (but not Sync -- mutable state).
    assert_send::<Cache>();

    // VimMatch is Send+Sync: match results are plain data.
    assert_send::<VimMatch>();
    assert_sync::<VimMatch>();

    // VimRegexError is Send+Sync: errors propagate across thread boundaries.
    assert_send::<ir::VimRegexError>();
    assert_sync::<ir::VimRegexError>();
};

#[cfg(test)]
#[path = "tests/equivalence/strategy.rs"]
mod strategy_equivalence_tests;

#[cfg(test)]
#[path = "tests/equivalence/strategy_acceleration.rs"]
mod strategy_acceleration_tests;

#[cfg(test)]
#[path = "tests/equivalence/reverse_inner.rs"]
mod reverse_inner_tests;

#[cfg(test)]
#[path = "tests/equivalence/lookbehind.rs"]
mod lookbehind_tests;

#[cfg(test)]
#[path = "tests/engines/dfa.rs"]
mod dfa_engine_tests;

#[cfg(test)]
#[path = "tests/equivalence/edge_cases.rs"]
mod edge_case_tests;

#[cfg(test)]
#[path = "tests/equivalence/adversarial_patterns.rs"]
mod adversarial_pattern_tests;

#[cfg(test)]
#[path = "tests/equivalence/lookahead_assertions.rs"]
mod lookahead_assertion_tests;

#[cfg(test)]
#[path = "tests/engines/onepass.rs"]
mod onepass_engine_tests;

#[cfg(test)]
#[path = "tests/cross_feature.rs"]
mod cross_feature_tests;

#[cfg(test)]
#[path = "tests/integration/exhaustive_small.rs"]
mod exhaustive_small_tests;

#[cfg(test)]
#[path = "tests/integration/regressions.rs"]
mod regression_tests;

#[cfg(test)]
#[path = "tests/integration/adversarial_edge_cases.rs"]
mod adversarial_edge_case_tests;

#[cfg(test)]
mod test_builder;

#[cfg(test)]
#[path = "tests/harness.rs"]
mod test_harness;

// ═══════════════════════════════════════════════════════════════════════════════
// VIM MATCH — result of a successful regex match
// ═══════════════════════════════════════════════════════════════════════════════

/// The result of a successful regex match.
///
/// Contains the matched range (respecting `\zs`/`\ze` overrides),
/// the full extent of the match before narrowing, and any captured
/// sub-group ranges.
///
/// Access captures via [`capture`](Self::capture),
/// [`capture_count`](Self::capture_count), or
/// [`captures_iter`](Self::captures_iter).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[must_use]
pub struct VimMatch {
    /// Match range respecting `\zs`/`\ze` overrides.
    pub range: Range<usize>,
    /// Full matched extent before `\zs`/`\ze` narrowing.
    pub full_range: Range<usize>,
    /// Captured sub-group ranges (groups 1..9).
    #[cfg_attr(
        feature = "serde",
        serde(
            serialize_with = "serde_captures::serialize",
            deserialize_with = "serde_captures::deserialize"
        )
    )]
    pub(crate) captures: SmallVec<[Option<Range<usize>>; MAX_CAPTURE_GROUPS]>,
}

/// Custom serde for `SmallVec` captures -- serializes as a plain `Vec`
/// sequence for stable cross-version format, decoupled from SmallVec internals.
#[cfg(feature = "serde")]
mod serde_captures {
    use super::*;

    pub(super) fn serialize<S>(
        captures: &SmallVec<[Option<Range<usize>>; MAX_CAPTURE_GROUPS]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(captures.len()))?;
        for cap in captures {
            seq.serialize_element(cap)?;
        }
        seq.end()
    }

    pub(super) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<SmallVec<[Option<Range<usize>>; MAX_CAPTURE_GROUPS]>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let vec: Vec<Option<Range<usize>>> = serde::Deserialize::deserialize(deserializer)?;
        Ok(SmallVec::from_vec(vec))
    }
}

impl VimMatch {
    /// Returns the captured range for the given group (1-indexed).
    ///
    /// Group 0 returns the match range itself. Returns `None` if the group
    /// did not participate in the match or the group index is out of bounds.
    #[must_use]
    #[inline]
    pub fn capture(&self, group: usize) -> Option<&Range<usize>> {
        if group == 0 {
            return Some(&self.range);
        }
        self.captures.get(group - 1).and_then(|opt| opt.as_ref())
    }

    /// Returns the number of capture groups in this match (excluding group 0).
    #[must_use]
    #[inline]
    pub fn capture_count(&self) -> usize {
        self.captures.len()
    }

    /// Returns an iterator over capture group ranges (1-indexed groups).
    ///
    /// Each element is `Option<&Range<usize>>` -- `None` if that group
    /// did not participate in the match.
    pub fn captures_iter(&self) -> impl Iterator<Item = Option<&Range<usize>>> {
        self.captures.iter().map(|opt| opt.as_ref())
    }

    /// Creates a new `VimMatch`. Only available within the crate.
    pub(crate) fn new(
        range: Range<usize>,
        full_range: Range<usize>,
        captures: SmallVec<[Option<Range<usize>>; MAX_CAPTURE_GROUPS]>,
    ) -> Self {
        Self {
            range,
            full_range,
            captures,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// FUZZY / APPROXIMATE MATCH CONFIG
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for approximate (fuzzy) matching.
///
/// Controls the edit distance budget and per-operation costs for
/// insertion, deletion, and substitution errors. The backtracker
/// explores all paths within the cost budget and returns matches
/// ranked by total cost (lower = closer match).
///
/// # Vim Use Cases
///
/// - Fuzzy `:find` / Ctrl-P style matching
/// - Typo-tolerant `/search` with user-specified tolerance
/// - Fuzzy command-line completion
///
/// # Example
///
/// ```
/// use vim_regex::FuzzyConfig;
///
/// let config = FuzzyConfig {
///     max_cost: 2,
///     cost_insert: 1,
///     cost_delete: 1,
///     cost_substitute: 1,
///     max_errors: None,
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FuzzyConfig {
    /// Maximum total cost budget for the approximate match.
    /// A match is accepted only if its accumulated cost <= `max_cost`.
    pub max_cost: u16,
    /// Cost charged per insertion (input character consumed without NFA advance).
    pub cost_insert: u16,
    /// Cost charged per deletion (NFA advances without consuming input).
    pub cost_delete: u16,
    /// Cost charged per substitution (character mismatch tolerated).
    pub cost_substitute: u16,
    /// Optional limit on the total number of edit operations (regardless of cost).
    /// When `Some(n)`, the match is rejected if more than `n` total edits are used.
    pub max_errors: Option<u16>,
}

impl Default for FuzzyConfig {
    fn default() -> Self {
        Self {
            max_cost: 0,
            cost_insert: 1,
            cost_delete: 1,
            cost_substitute: 1,
            max_errors: None,
        }
    }
}

impl FuzzyConfig {
    /// Create a config allowing up to `max_edits` errors with uniform cost.
    #[must_use]
    pub fn with_max_edits(max_edits: u16) -> Self {
        Self {
            max_cost: max_edits,
            cost_insert: 1,
            cost_delete: 1,
            cost_substitute: 1,
            max_errors: Some(max_edits),
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_vim_match_tests {
    use super::*;

    #[test]
    fn vim_match_round_trip() {
        let m = VimMatch::new(
            5..10,
            3..12,
            SmallVec::from_vec(vec![Some(5..7), None, Some(8..10)]),
        );
        let json = serde_json::to_string(&m).expect("serialize");
        let back: VimMatch = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }

    #[test]
    fn vim_match_empty_captures_round_trip() {
        let m = VimMatch::new(0..3, 0..3, SmallVec::new());
        let json = serde_json::to_string(&m).expect("serialize");
        let back: VimMatch = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, back);
    }
}
