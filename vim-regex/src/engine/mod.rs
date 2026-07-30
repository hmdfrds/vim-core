//! Meta-engine and public API for the Vim regex engine.
//!
//! Selects between the Pike VM (for NFA-safe patterns) and the bounded
//! backtracker (for patterns with backreferences), and provides the
//! user-facing `VimRegex` struct.

mod backward;
mod compile;
mod compile_cache;
mod dispatch;
mod nearest;
pub mod pattern_stats;
pub(crate) mod strategies;
pub(crate) mod strategy;

pub use compile_cache::CacheWithGuard;
pub use strategy::EngineKind;

use std::rc::Rc;

use compact_str::CompactString;
use smallvec::SmallVec;

use crate::MagicMode;

use super::accel::{self, Prefilter};
use super::cache::Cache;
use super::hir::PatternProperties;
use super::ir::{CaseMode, ComposingMode, PatternFeatures, VimPatternNode, VimRegexError};
use super::matchers::MatchContext;
use super::nfa::Nfa;
use super::VimMatch;

use strategy::Strategy;

use compile_cache::{take_dfa_cache, COMPILE_CACHE};

// ═══════════════════════════════════════════════════════════════════════════════
// VIM REGEX — PUBLIC STRUCT
// ═══════════════════════════════════════════════════════════════════════════════

/// A compiled Vim-compatible regular expression.
pub struct VimRegex {
    /// The original pattern string used to compile this regex.
    pub(crate) pattern: CompactString,
    /// The parsed intermediate representation (for debug/display, future API).
    #[allow(
        dead_code,
        reason = "retained for ir() getter used in tests and future introspection API"
    )]
    ir: VimPatternNode,
    /// The compiled NFA.
    pub(crate) nfa: Nfa,
    /// HIR pattern properties (acceleration hints + feature flags).
    pub(crate) properties: PatternProperties,
    /// Case sensitivity mode from pattern modifiers (`\c`, `\C`).
    pub(crate) case_mode: CaseMode,
    /// Composing character mode from pattern modifier (`\Z`).
    pub(crate) composing_mode: ComposingMode,
    /// Prefilter for fast position skipping (memchr, substring, etc.).
    pub(crate) prefilter: Option<Box<dyn Prefilter>>,
    /// Case-insensitive prefilter (built once, reused across CI searches).
    pub(crate) ci_prefilter: Option<Box<dyn Prefilter>>,
    /// Inner literal for backtracker prescreen.
    pub(crate) inner_literal: Option<CompactString>,
    /// Which engine to use for this pattern.
    pub(crate) engine_kind: EngineKind,
    /// Reverse NFA for suffix-guided backward scan (PikeVm-eligible only).
    pub(crate) reverse_nfa: Option<Nfa>,
    /// Extracted literal suffix for reverse search guidance.
    pub(crate) suffix_literal: Option<CompactString>,
    /// Prefix-reverse NFA for inner-literal-guided backward scan.
    pub(crate) prefix_reverse_nfa: Option<Nfa>,
    /// Pre-built AC prefilter for multi-pattern literal matching.
    pub(crate) ac_prefilter: Option<accel::aho_corasick::AcPrefilter>,
    /// Pre-built AC prefilter for case-insensitive matching.
    pub(crate) ac_prefilter_ci: Option<accel::aho_corasick::AcPrefilter>,
    /// Whether AC can serve as a full matcher (no captures, no overrides).
    pub(crate) ac_is_full_match: bool,
    /// Upper bound on reverse scan distance (match len + 16, or MAX).
    pub(crate) max_reverse_distance: usize,
    /// Case-insensitive suffix literal (lowercased) for reverse search.
    pub(crate) ci_suffix_literal: Option<CompactString>,
    /// Last fixed literal byte that must appear in any match (PCRE2-style).
    /// Used for fast-rejection: memchr for this byte; if absent, no match possible.
    pub(crate) required_byte: Option<(u8, bool)>,
    /// Extracted literal suffix (from HIR tree walk, distinct from
    /// `suffix_literal` which comes from `PatternProperties` for reverse strategies).
    pub(crate) suffix_literal_extracted: Option<CompactString>,
    /// Prefilter tree (AND/OR of required literals) for fast rejection.
    /// Used before the full match attempt to check if all required literals
    /// are present in the text.
    pub(crate) prefilter_tree: Option<accel::PrefilterNode>,
    /// 256-bit bitmap of possible first bytes of any match.
    /// None if universal (any byte can start a match).
    /// Used in the bumpalong loop: `bitmap[byte >> 5] & (1 << (byte & 31)) != 0`.
    pub(crate) start_bitmap: Option<[u32; 8]>,
    /// Minimum number of bytes any match must consume.
    /// Used for early-exit: if remaining text is shorter than this, no match possible.
    pub(crate) min_match_length: usize,
    /// Ordered strategy cascade. Each strategy is tried in sequence.
    /// The last entry (EngineDispatch) never declines.
    pub(crate) strategies: SmallVec<[Strategy; 5]>,
    /// All parser errors collected during multi-error recovery.
    ///
    /// Empty for patterns that parsed successfully or in single-error mode.
    /// The primary error is returned as `Err` from `VimRegex::new()`.
    /// Secondary errors are stored here for diagnostic display.
    all_errors: Vec<VimRegexError>,
}

impl std::fmt::Debug for VimRegex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VimRegex")
            .field("pattern", &self.pattern.as_str())
            .field("engine", &self.engine_kind)
            .field("case_mode", &self.case_mode)
            .field("has_prefilter", &self.prefilter.is_some())
            .field("has_ci_prefilter", &self.ci_prefilter.is_some())
            .field("has_inner_literal", &self.inner_literal.is_some())
            .field("required_byte", &self.required_byte)
            .field("strategies", &self.strategies.as_slice())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for VimRegex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.pattern)
    }
}

impl std::str::FromStr for VimRegex {
    type Err = super::ir::VimRegexError;

    /// Compile a regex from a string using the default `Magic` mode.
    ///
    /// This is equivalent to [`VimRegex::new`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-ERROR RECOVERY ACCESSORS
// ═══════════════════════════════════════════════════════════════════════════════

impl VimRegex {
    /// Returns all errors collected during pattern compilation.
    ///
    /// In single-error mode (current default), this returns an empty slice.
    /// When multi-error recovery is enabled, this may contain secondary
    /// errors found after the parser recovered from the first error.
    #[must_use]
    pub fn all_errors(&self) -> &[VimRegexError] {
        &self.all_errors
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PREFILTER SELECTION HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl VimRegex {
    /// Select the effective prefilter for the given case sensitivity.
    #[inline]
    pub(crate) fn effective_prefilter(&self, case_sensitive: bool) -> Option<&dyn Prefilter> {
        if case_sensitive {
            self.prefilter.as_deref()
        } else {
            self.ci_prefilter.as_deref()
        }
    }

    /// Select the effective inner literal for the given case sensitivity.
    #[inline]
    pub(crate) fn effective_inner_literal(&self, case_sensitive: bool) -> Option<&str> {
        if case_sensitive {
            self.inner_literal.as_deref()
        } else {
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH CONFIG — Strategy subtractive configuration (regex-automata pattern)
// ═══════════════════════════════════════════════════════════════════════════════

/// Configuration for controlling which search strategies are used.
///
/// Primarily for testing — lets you disable strategies to verify fallback
/// correctness. All flags default to `true` (all strategies enabled).
///
/// Follows the "subtractive configuration" pattern from `regex-automata`:
/// start with everything enabled, then selectively disable to test
/// strategy isolation and fallback paths.
#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct SearchConfig {
    /// Enable hybrid DFA acceleration.
    pub dfa_enabled: bool,
    /// Enable pure literal bypass strategy.
    pub literal_bypass_enabled: bool,
    /// Enable Aho-Corasick full matcher.
    pub ac_enabled: bool,
    /// Enable reverse NFA strategies (suffix, inner, anchored).
    pub reverse_enabled: bool,
    /// Enable auto-possessification of greedy quantifiers.
    pub possessify_enabled: bool,
    /// Enable start-position / fast-reject accelerators (start bitmap, prefilter,
    /// prefilter tree, required-byte memchr, suffix-literal reject). When `false`,
    /// the compiled regex omits ALL of these so every candidate start position is
    /// visited by the terminal engine scan. Used to verify (invariant 19) that the
    /// start filter never excludes a position where an unfiltered scan matches.
    pub prefilters_enabled: bool,
    /// Force a specific engine kind, overriding the pattern-based selection.
    /// `None` means automatic (default). `Some(EngineKind::PikeVm)` forces
    /// Pike VM even for backreference patterns; `Some(EngineKind::Backtracker)`
    /// forces the backtracker even for simple patterns.
    pub force_engine: Option<strategy::EngineKind>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            dfa_enabled: true,
            literal_bypass_enabled: true,
            ac_enabled: true,
            reverse_enabled: true,
            possessify_enabled: true,
            prefilters_enabled: true,
            force_engine: None,
        }
    }
}

impl SearchConfig {
    /// All acceleration disabled — pure engine dispatch only.
    #[must_use]
    #[doc(hidden)]
    pub fn baseline() -> Self {
        Self {
            dfa_enabled: false,
            literal_bypass_enabled: false,
            ac_enabled: false,
            reverse_enabled: false,
            possessify_enabled: false,
            prefilters_enabled: true,
            force_engine: None,
        }
    }

    /// Force the terminal engine scan with NO start-position / fast-reject
    /// accelerators: start bitmap, prefilter, prefilter tree, required-byte
    /// memchr, and suffix-literal reject are all stripped, AND every
    /// acceleration strategy (DFA, literal bypass, AC, reverse) is disabled so
    /// the only active strategy is the position-by-position engine dispatch.
    /// Every candidate start position is therefore visited by the engine
    /// itself, making the result the start-filter soundness ground truth that
    /// invariant 19 compares the filtered search against.
    ///
    /// Possessification is left ENABLED: it is a semantic transform on the NFA,
    /// not a start-position filter, so disabling it would conflate the
    /// (separately tested) possessify behavior with start-filter soundness.
    #[must_use]
    #[doc(hidden)]
    pub fn without_prefilters() -> Self {
        Self {
            possessify_enabled: true,
            ..Self::baseline() // baseline() leaves prefilters_enabled = true; override below.
        }
        .with_prefilters_disabled()
    }

    /// Internal: return `self` with start-position / fast-reject accelerators
    /// stripped. Kept separate so `without_prefilters` can compose it onto a
    /// `baseline()` (all-strategies-off) base.
    #[must_use]
    fn with_prefilters_disabled(mut self) -> Self {
        self.prefilters_enabled = false;
        self
    }

    /// Force Pike VM engine regardless of pattern features.
    #[must_use]
    #[doc(hidden)]
    pub fn force_pike_vm() -> Self {
        Self {
            force_engine: Some(strategy::EngineKind::PikeVm),
            ..Self::baseline()
        }
    }

    /// Force Backtracker engine regardless of pattern features.
    #[must_use]
    #[doc(hidden)]
    pub fn force_backtracker() -> Self {
        Self {
            force_engine: Some(strategy::EngineKind::Backtracker),
            ..Self::baseline()
        }
    }
}

// Public API methods stay here (thin facade delegating to submodules).
impl VimRegex {
    /// Compile a Vim regex pattern using the default `Magic` mode.
    pub fn new(pattern: &str) -> Result<Self, VimRegexError> {
        Self::with_magic(pattern, MagicMode::Magic)
    }

    /// Compile a Vim regex pattern with an explicit magic mode.
    pub fn with_magic(pattern: &str, magic: MagicMode) -> Result<Self, VimRegexError> {
        compile::compile_pattern(pattern, magic)
    }

    /// Compile a Vim regex pattern with a `SearchConfig` that controls
    /// which acceleration strategies are active.
    ///
    /// This is the subtractive-config entry point: the compiled regex
    /// omits strategies whose config flag is `false`, falling through
    /// to the terminal engine dispatch (Pike VM / backtracker).
    ///
    /// Useful for verifying that disabled strategies produce identical
    /// results to the full cascade (strategy isolation testing).
    #[doc(hidden)]
    pub fn with_config(pattern: &str, config: &SearchConfig) -> Result<Self, VimRegexError> {
        Self::with_magic_and_config(pattern, MagicMode::Magic, config)
    }

    /// Compile with explicit magic mode and search config.
    #[doc(hidden)]
    pub fn with_magic_and_config(
        pattern: &str,
        magic: MagicMode,
        config: &SearchConfig,
    ) -> Result<Self, VimRegexError> {
        compile::compile_pattern_with_config(pattern, magic, config)
    }

    /// Compile (or retrieve from cache) using default `Magic` mode.
    pub fn cached(pattern: &str) -> Result<Rc<Self>, VimRegexError> {
        Self::cached_with_magic(pattern, MagicMode::Magic)
    }

    /// Compile (or retrieve from cache) with explicit magic mode.
    pub fn cached_with_magic(pattern: &str, magic: MagicMode) -> Result<Rc<Self>, VimRegexError> {
        let hit = COMPILE_CACHE.with(|cache| cache.borrow_mut().get(pattern, magic));
        if let Some(rc) = hit {
            return Ok(rc);
        }
        let regex = Self::with_magic(pattern, magic)?;
        let rc = COMPILE_CACHE.with(|cache| cache.borrow_mut().insert(pattern, magic, regex));
        Ok(rc)
    }

    // ─── Convenience methods (allocate a fresh Cache per call) ─────────

    /// Find the first match in the text, starting from byte offset 0.
    ///
    /// Allocates a fresh [`Cache`] per call. For repeated searches with the
    /// same compiled regex, prefer [`find_with_cache`](Self::find_with_cache).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if the pattern is too complex for the
    /// chosen engine (e.g., exceeds backtracker budget).
    pub fn find(&self, ctx: &MatchContext<'_>) -> Result<Option<VimMatch>, VimRegexError> {
        self.find_at(ctx, 0)
    }

    /// Find the first match starting at or after `start` byte offset.
    ///
    /// Allocates a fresh [`Cache`] per call. For repeated searches,
    /// prefer [`find_at_with_cache`](Self::find_at_with_cache).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_at(
        &self,
        ctx: &MatchContext<'_>,
        start: usize,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        let mut cache = self.create_cache();
        self.find_at_with_cache(&mut cache, ctx, start)
    }

    /// Find the first match starting at or after `start` byte offset.
    ///
    /// Allocates a fresh [`Cache`] per call. For repeated searches,
    /// prefer [`find_at_with_cache`](Self::find_at_with_cache).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    #[deprecated(since = "0.2.0", note = "renamed to `find_at`")]
    pub fn find_from(
        &self,
        ctx: &MatchContext<'_>,
        start: usize,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        self.find_at(ctx, start)
    }

    /// Find the last (rightmost) match in the text (backward search from end).
    ///
    /// Allocates a fresh [`Cache`] per call. For repeated searches,
    /// prefer [`find_backward_with_cache`](Self::find_backward_with_cache).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_backward(&self, ctx: &MatchContext<'_>) -> Result<Option<VimMatch>, VimRegexError> {
        let mut cache = self.create_cache();
        self.find_backward_with_cache(&mut cache, ctx)
    }

    /// Find all non-overlapping matches in the text.
    ///
    /// Returns matches in forward (left-to-right) order. Allocates a
    /// fresh [`Cache`] per call. For repeated searches, prefer
    /// [`find_all_with_cache`](Self::find_all_with_cache).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_all(&self, ctx: &MatchContext<'_>) -> Result<Vec<VimMatch>, VimRegexError> {
        let mut cache = self.create_cache();
        self.find_all_with_cache(&mut cache, ctx)
    }

    /// Check whether the pattern matches anywhere in the text.
    ///
    /// Uses a no-captures cache for efficiency. Allocates a fresh
    /// [`Cache`] per call. For repeated checks, prefer
    /// [`is_match_with_cache`](Self::is_match_with_cache).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn is_match(&self, ctx: &MatchContext<'_>) -> Result<bool, VimRegexError> {
        let mut cache = self.create_cache_no_captures();
        self.is_match_with_cache(&mut cache, ctx)
    }

    // ─── Cursor-gravity search ─────────────────────────────────────────────

    /// Find the match nearest to the cursor position.
    ///
    /// Performs a bidirectional expanding wavefront search: interleaves
    /// forward and backward searches in exponentially growing chunks
    /// (256 bytes, 1KB, 4KB, ...) centered on `ctx.cursor`. Returns
    /// the first match found, which is guaranteed to be the nearest
    /// to the cursor.
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_nearest(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        self.assert_cache_compatible(cache);
        nearest::find_nearest(self, cache, ctx)
    }

    /// Find the match nearest to the cursor position, using a fresh cache.
    ///
    /// Convenience wrapper that allocates a new [`Cache`] per call.
    /// For repeated searches, prefer [`find_nearest`](Self::find_nearest).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_nearest_simple(
        &self,
        ctx: &MatchContext<'_>,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        let mut cache = self.create_cache();
        self.find_nearest(&mut cache, ctx)
    }

    // ─── Backward search with explicit range ──────────────────────────────

    /// Find the last match within the given byte range (backward search),
    /// using a pre-allocated cache.
    ///
    /// Searches backward from `range.end` toward `range.start`, returning
    /// the rightmost match whose start is >= `range.start` and whose end
    /// is <= `range.end`.
    ///
    /// This is the cache-reusing variant of [`find_backward_in_range`](Self::find_backward_in_range).
    ///
    /// # Panics
    ///
    /// Debug-asserts that `range.start <= range.end` and `range.end <= ctx.text.len()`.
    ///
    /// # Errors
    ///
    /// Returns `VimRegexError` if an internal engine error occurs.
    pub fn find_backward_in_range_with_cache(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
        range: std::ops::Range<usize>,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        self.assert_cache_compatible(cache);

        debug_assert!(
            range.start <= range.end,
            "find_backward_in_range_with_cache: range.start ({}) > range.end ({})",
            range.start,
            range.end
        );
        debug_assert!(
            range.end <= ctx.text.len(),
            "find_backward_in_range_with_cache: range.end ({}) > text.len() ({})",
            range.end,
            ctx.text.len()
        );

        if range.is_empty() {
            return Ok(None);
        }

        let slice = &ctx.text[range.start..range.end];
        let narrowed = MatchContext {
            text: slice,
            cursor: Some(slice.len()),
            visual_range: ctx.visual_range.map(|(s, e)| {
                (
                    s.saturating_sub(range.start),
                    e.saturating_sub(range.start).min(slice.len()),
                )
            }),
            case_sensitive: ctx.case_sensitive,
            ignore_composing: ctx.ignore_composing,
            line_resolver: None,
            mark_resolver: None,
            last_substitute: ctx.last_substitute,
        };

        let result = self.find_backward_with_cache(cache, &narrowed)?;
        Ok(result.map(|m| dispatch::offset_match(m, range.start)))
    }

    /// Find the last match within the given byte range (backward search).
    ///
    /// Allocates a fresh cache per call. For repeated use, prefer
    /// [`find_backward_in_range_with_cache`](Self::find_backward_in_range_with_cache)
    /// with a pre-allocated cache.
    ///
    /// # Errors
    ///
    /// Returns `VimRegexError` if an internal engine error occurs.
    pub fn find_backward_in_range(
        &self,
        ctx: &MatchContext<'_>,
        range: std::ops::Range<usize>,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        let mut cache = self.create_cache();
        self.find_backward_in_range_with_cache(&mut cache, ctx, range)
    }

    // ─── Cache compatibility ─────────────────────────────────────────────

    /// Assert cache compatibility and gracefully resize in release mode.
    #[inline]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "NFA state count limited to u32::MAX"
    )]
    fn assert_cache_compatible(&self, cache: &mut Cache) {
        let nfa_states = self.nfa.state_count();
        debug_assert!(
            cache.is_compatible_with_state_count(nfa_states),
            "Cache created for {} states, but regex has {} states. \
             Create a cache with `regex.create_cache()` or a larger state count.",
            cache.state_count(),
            nfa_states,
        );
        cache.ensure_capacity(self.nfa.state_count() as u32, self.nfa.slot_count() + 2);
    }

    // ─── _with_cache methods (delegate to dispatch/backward) ───────────

    /// Find the first match starting at `start`, using a pre-allocated cache.
    ///
    /// This is the cache-reusing variant of [`find_at`](Self::find_at).
    /// The cache must have been created by [`create_cache`](Self::create_cache)
    /// on a regex with the same or larger NFA state count.
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_at_with_cache(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
        start: usize,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        self.assert_cache_compatible(cache);
        dispatch::search_internal(self, cache, ctx, start, strategy::SearchMode::Full)
    }

    /// Find the first match starting at `start`, using a pre-allocated cache.
    ///
    /// This is the cache-reusing variant of [`find_at`](Self::find_at).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    #[deprecated(since = "0.2.0", note = "renamed to `find_at_with_cache`")]
    pub fn find_from_with_cache(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
        start: usize,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        self.find_at_with_cache(cache, ctx, start)
    }

    /// Find the last match in the text, using a pre-allocated cache.
    ///
    /// This is the cache-reusing variant of [`find_backward`](Self::find_backward).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_backward_with_cache(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        self.assert_cache_compatible(cache);
        backward::find_backward_with_cache(self, cache, ctx)
    }

    /// Find all non-overlapping matches, using a pre-allocated cache.
    ///
    /// This is the cache-reusing variant of [`find_all`](Self::find_all).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_all_with_cache(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
    ) -> Result<Vec<VimMatch>, VimRegexError> {
        self.assert_cache_compatible(cache);
        dispatch::find_all_with_cache(self, cache, ctx)
    }

    /// Check whether the pattern matches, using a pre-allocated cache.
    ///
    /// This is the cache-reusing variant of [`is_match`](Self::is_match).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn is_match_with_cache(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
    ) -> Result<bool, VimRegexError> {
        self.assert_cache_compatible(cache);
        dispatch::is_match_with_cache(self, cache, ctx)
    }

    /// Find the first match starting at offset 0, using a pre-allocated cache.
    ///
    /// This is the cache-reusing variant of [`find`](Self::find).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_with_cache(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
    ) -> Result<Option<VimMatch>, VimRegexError> {
        self.assert_cache_compatible(cache);
        dispatch::search_internal(self, cache, ctx, 0, strategy::SearchMode::Full)
    }

    // ─── Accessors ─────────────────────────────────────────────────────

    /// Returns the original pattern string used to compile this regex.
    ///
    /// This is the exact string passed to [`new`](Self::new),
    /// [`with_magic`](Self::with_magic), or similar constructors.
    #[must_use]
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.pattern
    }

    /// Returns the parsed intermediate representation (AST) of this pattern.
    ///
    /// Useful for pattern introspection, debug display, or re-compilation.
    #[must_use]
    pub const fn ir(&self) -> &VimPatternNode {
        &self.ir
    }

    /// Returns the feature flags detected during compilation.
    ///
    /// Indicates whether the pattern uses backreferences, lookaround,
    /// atomic groups, buffer positions, match overrides, etc.
    #[must_use]
    pub fn features(&self) -> PatternFeatures {
        PatternFeatures {
            has_backreferences: self.properties.features.has_backreferences,
            has_lookaround: self.properties.features.has_lookaround,
            has_atomic: self.properties.features.has_atomic,
            has_buffer_position: self.properties.features.has_buffer_position,
            has_match_override: self.properties.features.has_match_override,
            has_last_substitute: self.properties.features.has_last_substitute,
            has_multiline: self.properties.features.has_multiline,
            has_branch_and: self.properties.features.has_branch_and,
            capture_count: self.properties.features.capture_count,
        }
    }

    /// Returns a literal string suitable for Bloom filter pre-screening.
    ///
    /// Returns `None` if the pattern is case-insensitive or has no literal
    /// of length >= 2.
    #[must_use]
    pub fn bloom_literal(&self) -> Option<&str> {
        if matches!(self.case_mode, CaseMode::Insensitive) {
            return None;
        }
        self.inner_literal
            .as_deref()
            .or(self.properties.accel_hints.literal_prefix.as_deref())
            .filter(|lit| lit.len() >= 2)
    }

    /// Returns the approximate heap memory used by this compiled regex, in bytes.
    ///
    /// This includes the NFA states, reverse NFAs, strategy cascade, and
    /// string fields. It does NOT include stack size
    /// (`std::mem::size_of::<VimRegex>()`), which the caller can compute
    /// separately.
    ///
    /// This is useful for monitoring and debugging memory usage in long-lived
    /// editor sessions with many cached patterns.
    #[must_use]
    pub fn memory_usage(&self) -> usize {
        use std::mem::size_of;

        let mut total = 0;

        // NFA states (approximate: per-state vec of transitions).
        total += self.nfa.state_count() * size_of::<crate::nfa::NfaState>();

        // Reverse NFA (if present).
        if let Some(ref rev) = self.reverse_nfa {
            total += rev.state_count() * size_of::<crate::nfa::NfaState>();
        }

        // Prefix-reverse NFA (if present).
        if let Some(ref prev) = self.prefix_reverse_nfa {
            total += prev.state_count() * size_of::<crate::nfa::NfaState>();
        }

        // Strategy cascade.
        total += self.strategies.len() * size_of::<Strategy>();

        // CompactString heap portions (inline storage <= 24 bytes is free).
        let compact_str_inline = 24;
        if self.pattern.len() > compact_str_inline {
            total += self.pattern.len();
        }
        if let Some(ref s) = self.inner_literal {
            if s.len() > compact_str_inline {
                total += s.len();
            }
        }
        if let Some(ref s) = self.suffix_literal {
            if s.len() > compact_str_inline {
                total += s.len();
            }
        }
        if let Some(ref s) = self.ci_suffix_literal {
            if s.len() > compact_str_inline {
                total += s.len();
            }
        }
        if let Some(ref s) = self.suffix_literal_extracted {
            if s.len() > compact_str_inline {
                total += s.len();
            }
        }

        total
    }

    #[cfg(test)]
    pub(crate) fn has_prefix_reverse_nfa(&self) -> bool {
        self.prefix_reverse_nfa.is_some()
    }

    #[cfg(test)]
    pub(crate) fn is_dfa_eligible(&self) -> bool {
        strategy::is_dfa_eligible(&self.properties)
    }

    /// Returns whether this pattern is eligible for the one-pass DFA engine.
    ///
    /// Eligible patterns have no backreferences, lookaround, atomic groups,
    /// branch-and, buffer positions, or look-ahead assertions, and have
    /// at most 16 capture groups.
    #[cfg(test)]
    pub(crate) fn is_onepass_eligible(&self) -> bool {
        crate::engines::onepass::is_onepass_eligible(&self.properties)
    }

    // ─── Cache creation ────────────────────────────────────────────────

    /// Create a fresh cache sized for this regex, with capture slot tracking.
    ///
    /// The returned [`Cache`] is compatible with all `_with_cache` methods
    /// on this regex instance.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "NFA state count limited to u32::MAX"
    )]
    pub fn create_cache(&self) -> Cache {
        let slot_count = self.nfa.slot_count();
        Cache::new(
            self.nfa.state_count() as u32,
            slot_count + 2,
            self.nfa.has_backreferences(),
        )
    }

    /// Create a cache without capture slot tracking (for `is_match` use).
    ///
    /// More memory-efficient than [`create_cache`](Self::create_cache) when
    /// only match/no-match is needed and capture groups are not required.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "NFA state count limited to u32::MAX"
    )]
    pub fn create_cache_no_captures(&self) -> Cache {
        Cache::new_no_captures(self.nfa.state_count() as u32)
    }

    /// Create a cache pre-seeded with DFA state tables from the compile cache.
    ///
    /// Returns a [`CacheWithGuard`] that returns the DFA cache to the global
    /// pool on drop. Useful for long-lived regex instances used in editor loops.
    #[must_use]
    pub fn create_cache_seeded(self: &Rc<Self>) -> CacheWithGuard {
        let mut cache = self.create_cache();
        cache.dfa = take_dfa_cache(self);
        CacheWithGuard {
            cache,
            regex: Rc::clone(self),
        }
    }

    /// Check whether a cache is compatible with this regex.
    ///
    /// A cache is compatible if its internal sparse sets are large enough
    /// to hold this regex's NFA state count. Use this to verify before
    /// passing a shared cache to `_with_cache` methods.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let re = VimRegex::new(r"\d+")?;
    /// let cache = re.create_cache();
    /// assert!(re.is_cache_compatible(&cache));
    /// ```
    #[must_use]
    #[inline]
    pub fn is_cache_compatible(&self, cache: &Cache) -> bool {
        cache.is_compatible_with_state_count(self.nfa.state_count())
    }

    // ─── Case resolution (used by dispatch + backward) ─────────────────

    pub(crate) fn resolve_context<'a>(&self, ctx: &MatchContext<'a>) -> MatchContext<'a> {
        let &MatchContext {
            text,
            cursor,
            visual_range,
            case_sensitive: ctx_case_sensitive,
            ignore_composing: _,
            line_resolver,
            mark_resolver,
            last_substitute,
        } = ctx;

        let case_sensitive = match self.case_mode {
            CaseMode::Sensitive => true,
            CaseMode::Insensitive => false,
            CaseMode::Default => ctx_case_sensitive,
        };

        let ignore_composing = self.composing_mode == ComposingMode::Ignore;

        MatchContext {
            text,
            cursor,
            visual_range,
            case_sensitive,
            ignore_composing,
            line_resolver,
            mark_resolver,
            last_substitute,
        }
    }

    // ─── Range narrowing (used by dispatch + backward) ─────────────────

    pub(crate) fn apply_range_narrowing<'a>(
        &self,
        ctx: &MatchContext<'a>,
    ) -> (MatchContext<'a>, usize) {
        let mut range = 0..ctx.text.len();
        accel::narrow_search_range(
            &mut range,
            &self.properties,
            ctx.line_resolver,
            ctx.visual_range,
        );

        if range.start == 0 && range.end == ctx.text.len() {
            return (
                MatchContext {
                    text: ctx.text,
                    cursor: ctx.cursor,
                    visual_range: ctx.visual_range,
                    case_sensitive: ctx.case_sensitive,
                    ignore_composing: ctx.ignore_composing,
                    line_resolver: ctx.line_resolver,
                    mark_resolver: ctx.mark_resolver,
                    last_substitute: ctx.last_substitute,
                },
                0,
            );
        }

        use super::common::align_to_char_boundary;
        let start = align_to_char_boundary(ctx.text, range.start);
        let end = if range.end >= ctx.text.len() {
            ctx.text.len()
        } else {
            align_to_char_boundary(ctx.text, range.end)
        };

        let narrowed_text = &ctx.text[start..end];
        let narrowed = MatchContext {
            text: narrowed_text,
            cursor: ctx.cursor.map(|c| c.saturating_sub(start)),
            visual_range: ctx.visual_range.map(|(s, e)| {
                (
                    s.saturating_sub(start),
                    e.saturating_sub(start).min(narrowed_text.len()),
                )
            }),
            case_sensitive: ctx.case_sensitive,
            ignore_composing: ctx.ignore_composing,
            line_resolver: None,
            mark_resolver: None,
            last_substitute: ctx.last_substitute,
        };
        (narrowed, start)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LAZY ITERATOR — Matches
// ═══════════════════════════════════════════════════════════════════════════════

/// A lazy iterator over all non-overlapping matches of a regex in a text.
///
/// Created by [`VimRegex::find_iter`]. Yields `VimMatch` values in
/// left-to-right order, advancing past each match (or one codepoint
/// for zero-width matches) before searching for the next.
///
/// The iterator borrows the `VimRegex` and a `Cache` for the lifetime of
/// iteration. No heap allocation occurs per iteration step.
pub struct Matches<'r, 'c, 't> {
    regex: &'r VimRegex,
    cache: &'c mut Cache,
    ctx: MatchContext<'t>,
    pos: usize,
    /// Byte offset added back after range narrowing.
    offset: usize,
    done: bool,
    /// Error that terminated iteration, if any.
    error: Option<VimRegexError>,
}

impl Matches<'_, '_, '_> {
    /// Returns the error that terminated iteration, if any.
    ///
    /// When `next()` encounters an engine error, it stores the error here
    /// and returns `None`. Callers who need to distinguish "no more matches"
    /// from "iteration aborted by error" should check this after the iterator
    /// is exhausted.
    pub fn error(&self) -> Option<&VimRegexError> {
        self.error.as_ref()
    }
}

impl<'r, 'c, 't> Iterator for Matches<'r, 'c, 't> {
    type Item = VimMatch;

    fn next(&mut self) -> Option<VimMatch> {
        if self.done {
            return None;
        }

        let result = match dispatch::search_internal(
            self.regex,
            self.cache,
            &self.ctx,
            self.pos,
            strategy::SearchMode::Full,
        ) {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e);
                self.done = true;
                return None;
            }
        };

        match result {
            Some(m) => {
                // Advance past the match. For zero-width matches, advance by
                // one codepoint to avoid infinite loops.
                if m.range.is_empty() {
                    self.pos = super::common::advance_one_codepoint(self.ctx.text, m.range.end);
                } else {
                    self.pos = m.range.end;
                }
                if self.pos > self.ctx.text.len() {
                    self.done = true;
                }
                Some(dispatch::offset_match(m, self.offset))
            }
            None => {
                self.done = true;
                None
            }
        }
    }
}

impl VimRegex {
    /// Returns a lazy iterator over all non-overlapping matches.
    ///
    /// This is the preferred way to iterate over matches when you don't
    /// need all results upfront. The iterator reuses the provided `cache`
    /// across all search steps.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let regex = VimRegex::new(r"\d+")?;
    /// let mut cache = regex.create_cache();
    /// let ctx = MatchContext::simple("abc 123 def 456");
    /// let matches: Vec<_> = regex.find_iter(&mut cache, &ctx).collect();
    /// assert_eq!(matches.len(), 2);
    /// ```
    pub fn find_iter<'r, 'c, 't>(
        &'r self,
        cache: &'c mut Cache,
        ctx: &MatchContext<'t>,
    ) -> Matches<'r, 'c, 't> {
        self.assert_cache_compatible(cache);
        let resolved = self.resolve_context(ctx);
        let (narrowed, offset) = self.apply_range_narrowing(&resolved);
        Matches {
            regex: self,
            cache,
            ctx: narrowed,
            pos: 0,
            offset,
            done: false,
            error: None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// APPROXIMATE / FUZZY MATCHING
// ═══════════════════════════════════════════════════════════════════════════════

impl VimRegex {
    /// Find all approximate matches within the given cost budget.
    ///
    /// Returns a list of `(VimMatch, cost)` pairs sorted by cost (lowest
    /// first). The cost represents the total edit distance using the
    /// operations and costs defined in `config`.
    ///
    /// Allocates a fresh [`Cache`] per call. For repeated approximate
    /// searches, prefer [`find_approximate_with_cache`](Self::find_approximate_with_cache).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_approximate(
        &self,
        ctx: &MatchContext<'_>,
        config: &crate::FuzzyConfig,
    ) -> Result<Vec<(VimMatch, u16)>, VimRegexError> {
        let mut cache = self.create_cache();
        self.find_approximate_with_cache(&mut cache, ctx, config)
    }

    /// Find all approximate matches using a pre-allocated cache.
    ///
    /// This is the cache-reusing variant of [`find_approximate`](Self::find_approximate).
    ///
    /// # Errors
    ///
    /// Returns [`VimRegexError`] if an internal engine error occurs.
    pub fn find_approximate_with_cache(
        &self,
        cache: &mut Cache,
        ctx: &MatchContext<'_>,
        config: &crate::FuzzyConfig,
    ) -> Result<Vec<(VimMatch, u16)>, VimRegexError> {
        self.assert_cache_compatible(cache);
        let resolved = self.resolve_context(ctx);
        Ok(super::engines::backtracker::search_approximate(
            &self.nfa, cache, &resolved, 0, config,
        ))
    }
}

#[cfg(test)]
#[path = "../tests/integration/engine.rs"]
mod tests;
