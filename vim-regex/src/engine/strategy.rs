//! Strategy selection logic for the Vim regex engine.
//!
//! Determines which simulation engine (Pike VM or backtracker) and which
//! acceleration strategies (DFA, prefilter, reverse) to apply for a pattern.

use crate::cache::Cache;
use crate::hir::PatternProperties;
use crate::matchers::MatchContext;

// ═══════════════════════════════════════════════════════════════════════════════
// ENGINE KIND
// ═══════════════════════════════════════════════════════════════════════════════

/// Which simulation engine to use for a given pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    /// Pike VM — parallel NFA simulation (no backreference support).
    PikeVm,
    /// Bounded backtracker — stack-based DFS (handles backreferences).
    Backtracker,
}

/// Select the best engine for a pattern based on its properties.
pub(crate) fn select_engine(properties: &PatternProperties) -> EngineKind {
    if properties.features.needs_backtracker() {
        EngineKind::Backtracker
    } else {
        EngineKind::PikeVm
    }
}

/// Feature flag for look-ahead assertion DFA eligibility.
/// Set to false to route all look-ahead patterns to the Pike VM instead.
pub(crate) const DFA_LOOK_AHEAD_ENABLED: bool = true;

/// Whether a pattern can use the lazy DFA for acceleration.
///
/// The DFA handles patterns expressible as pure regular languages.
/// Patterns with context-sensitive features fall through to Pike VM.
pub(crate) fn is_dfa_eligible(properties: &PatternProperties) -> bool {
    if properties.features.disqualifies_dfa() {
        return false;
    }
    // Look-ahead assertions are allowed when DFA_LOOK_AHEAD_ENABLED is true.
    DFA_LOOK_AHEAD_ENABLED || !properties.features.has_look_ahead_assertions
}

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH MODE — OPTIMIZATION HINT
// ═══════════════════════════════════════════════════════════════════════════════

/// Controls the level of detail strategies must produce.
///
/// Threaded through the cascade to allow early-exit optimizations:
/// - `Full`: Match range + captures (default for `find`/`find_at`).
/// - `HalfEnd`: Match end position only, no start or captures (for
///   reverse-confirmed strategies).
/// - `Existence`: Boolean match/no-match only (for `is_match`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "HalfEnd is never constructed: search_internal is only ever called with Full (find/find_at) or Existence (is_match). The variant names the third contract level so strategies can already discriminate on it"
)]
pub(crate) enum SearchMode {
    /// Full match with captures.
    Full,
    /// Match end position only (no start, no captures).
    HalfEnd,
    /// Boolean: does any match exist? No position needed.
    Existence,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PREFILTER TRACKER
// ═══════════════════════════════════════════════════════════════════════════════

/// Tracks prefilter effectiveness and disables it when false positive rate is too high.
///
/// Uses exponential backoff: checks at 64, 128, 256, 512, ... candidates.
/// This reduces overhead for prefilters that start bad but improve, and
/// avoids the fixed-interval overhead of checking every 64 candidates.
///
/// Thresholds are haystack-proportional (set via `set_haystack_len`):
/// - Short texts (< 1KB): 10% false positive tolerance
/// - Medium texts (1KB-100KB): 1% threshold
/// - Large texts (> 100KB): 0.5% threshold
#[derive(Debug, Clone)]
pub(crate) struct PrefilterTracker {
    /// Total prefilter candidate positions reported.
    candidates: u32,
    /// Candidates that led to confirmed matches.
    confirmations: u32,
    /// Whether the prefilter has been disabled due to low hit rate.
    disabled: bool,
    /// Next candidate count at which to check effectiveness.
    /// Doubles after each check: 64, 128, 256, 512, ...
    next_check: u32,
    /// Confirmation rate threshold (multiplied by 10000 for integer math).
    /// 100 = 1%, 1000 = 10%, 50 = 0.5%.
    threshold_bps: u32,
}

impl PrefilterTracker {
    pub(crate) const fn new() -> Self {
        Self {
            candidates: 0,
            confirmations: 0,
            disabled: false,
            next_check: 64,
            threshold_bps: 100, // 1% default
        }
    }

    /// Set the haystack length to adjust the effectiveness threshold.
    ///
    /// Short texts tolerate more false positives; large texts are stricter.
    #[inline]
    pub(crate) fn set_haystack_len(&mut self, len: usize) {
        self.threshold_bps = if len < 1024 {
            1000 // 10% for short texts
        } else if len <= 100 * 1024 {
            100 // 1% for medium texts
        } else {
            50 // 0.5% for large texts
        };
    }

    /// Record a prefilter candidate. Returns `true` if prefilter is still active.
    #[inline]
    pub(crate) fn record_candidate(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        self.candidates += 1;
        // Check at exponential intervals: 64, 128, 256, 512, ...
        if self.candidates == self.next_check {
            // Disable if confirmation rate is below threshold.
            // confirmations * 10000 < candidates * threshold_bps
            if (self.confirmations as u64) * 10000
                < (self.candidates as u64) * (self.threshold_bps as u64)
            {
                self.disabled = true;
                return false;
            }
            // Double the interval for next check (saturating to avoid overflow).
            self.next_check = self.next_check.saturating_mul(2);
        }
        true
    }

    /// Record a confirmed match.
    #[inline]
    pub(crate) fn record_confirmation(&mut self) {
        self.confirmations += 1;
    }

    /// Whether the prefilter has been disabled.
    #[inline]
    pub(crate) const fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Reset for a new search.
    #[inline]
    pub(crate) fn reset(&mut self) {
        self.candidates = 0;
        self.confirmations = 0;
        self.disabled = false;
        self.next_check = 64;
        // threshold_bps is preserved -- set once per search via set_haystack_len.
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// REVERSE STRATEGY CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum reverse scan attempts before declining.
///
/// Secondary safety net behind the min_start progress tracking.
/// If the reverse scan makes no progress after this many attempts,
/// the strategy declines to the next tier.
pub(crate) const MAX_REVERSE_ATTEMPTS: usize = 32;

// ═══════════════════════════════════════════════════════════════════════════════
// STRATEGY ENUM — ZERO-COST DISPATCH
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) use crate::common::SearchResult;

/// A strategy in the search cascade.
///
/// Each variant is one tier of the cascade; a pattern's cascade preserves
/// the declaration order below, from cheapest to most general. Dispatch is
/// via `match` on the enum — no vtable, no heap allocation.
///
/// The cascade is stored as `SmallVec<[Strategy; 5]>` (1 byte per entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strategy {
    /// Skip prefilter/DFA overhead for tiny inputs (<256 bytes).
    SmallWrite,
    /// Pure literal bypass via prefilter (no NFA needed).
    LiteralBypass,
    /// Aho-Corasick full matcher bypass (literal alternations).
    AcFullMatch,
    /// Anchored start-of-file fast path (\%^).
    AnchoredStart,
    /// Reverse anchored end-of-file search.
    ReverseAnchored,
    /// Reverse suffix-guided search.
    ReverseSuffix,
    /// Reverse inner-literal-guided search.
    ReverseInner,
    /// Hybrid DFA acceleration.
    HybridDfa,
    /// One-pass DFA for anchored patterns with captures.
    OnePassDfa,
    /// Full engine dispatch (never declines).
    EngineDispatch,
}

impl Strategy {
    /// Execute this strategy, returning Match/NoMatch/Declined.
    ///
    /// `mode` is an optimization hint: strategies may use it to skip
    /// unnecessary work (e.g., skip captures for `Existence` mode).
    /// Currently all strategies ignore `mode` — they always produce
    /// full results; the DFA and one-pass DFA strategies will use it.
    pub(crate) fn try_search(
        self,
        regex: &super::VimRegex,
        cache: &mut Cache,
        start: usize,
        ctx: &MatchContext<'_>,
        mode: SearchMode,
    ) -> SearchResult {
        use super::strategies;
        match self {
            Self::SmallWrite => strategies::small_write::try_search(regex, cache, start, ctx, mode),
            Self::LiteralBypass => {
                strategies::literal_bypass::try_search(regex, cache, start, ctx, mode)
            }
            Self::AcFullMatch => {
                strategies::ac_full_match::try_search(regex, cache, start, ctx, mode)
            }
            Self::AnchoredStart => {
                strategies::anchored_start::try_search(regex, cache, start, ctx, mode)
            }
            Self::ReverseAnchored => {
                strategies::reverse_anchored::try_search(regex, cache, start, ctx, mode)
            }
            Self::ReverseSuffix => {
                strategies::reverse_suffix::try_search(regex, cache, start, ctx, mode)
            }
            Self::ReverseInner => {
                strategies::reverse_inner::try_search(regex, cache, start, ctx, mode)
            }
            Self::HybridDfa => strategies::hybrid_dfa::try_search(regex, cache, start, ctx, mode),
            Self::OnePassDfa => strategies::onepass_dfa::try_search(regex, cache, start, ctx, mode),
            Self::EngineDispatch => {
                strategies::engine_dispatch::try_search(regex, cache, start, ctx, mode)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_enum_is_copy_and_small() {
        assert_eq!(std::mem::size_of::<Strategy>(), 1);
        let s = Strategy::EngineDispatch;
        let s2 = s; // Copy
        assert_eq!(s, s2);
    }

    #[test]
    fn search_mode_is_copy_and_small() {
        assert_eq!(std::mem::size_of::<SearchMode>(), 1);
        let m = SearchMode::Full;
        let m2 = m; // Copy
        assert_eq!(m, m2);
        assert_ne!(SearchMode::Full, SearchMode::Existence);
        assert_ne!(SearchMode::HalfEnd, SearchMode::Existence);
    }

    #[test]
    fn prefilter_tracker_disables_after_many_misses() {
        let mut tracker = PrefilterTracker::new();
        // First 63 candidates: no check triggered yet.
        for _ in 0..63 {
            assert!(tracker.record_candidate());
        }
        assert!(!tracker.is_disabled());
        // 64th candidate: triggers check.
        // With 0 confirmations, rate < 1% => disabled, returns false.
        assert!(!tracker.record_candidate());
        assert!(tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_stays_active_with_good_hit_rate() {
        let mut tracker = PrefilterTracker::new();
        for i in 0u32..64 {
            tracker.record_candidate();
            if i % 5 == 0 {
                tracker.record_confirmation(); // ~20% hit rate
            }
        }
        // 64 candidates triggered a check; 13 confirmations (>1%), still active
        assert!(tracker.record_candidate());
        assert!(!tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_reset_clears_state() {
        let mut tracker = PrefilterTracker::new();
        for _ in 0..64 {
            tracker.record_candidate();
        }
        assert!(tracker.is_disabled());
        tracker.reset();
        assert!(!tracker.is_disabled());
        assert!(tracker.record_candidate());
    }

    #[test]
    fn prefilter_tracker_exponential_backoff_first_check_at_64() {
        let mut tracker = PrefilterTracker::new();
        // Feed 63 candidates -- no check triggered.
        for _ in 0..63 {
            assert!(tracker.record_candidate());
        }
        assert!(!tracker.is_disabled());
        // 64th candidate triggers check (first interval). 0 confirmations -> disabled.
        assert!(!tracker.record_candidate());
        assert!(tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_exponential_second_check_at_128() {
        let mut tracker = PrefilterTracker::new();
        // First 64: 2 confirmations (>1%) -- survives first check.
        for i in 0u32..64 {
            tracker.record_candidate();
            if i == 0 || i == 32 {
                tracker.record_confirmation();
            }
        }
        assert!(!tracker.is_disabled());
        // Next 64 (65-128): 0 additional confirmations.
        // At candidate 128, second check triggers. Total: 2/128 = 1.5% > 1% -> survives.
        for _ in 64..128 {
            tracker.record_candidate();
        }
        assert!(!tracker.is_disabled());
        // Next 128 (129-256): 0 additional confirmations.
        // At candidate 256, third check triggers. Total: 2/256 = 0.78% < 1% -> disabled.
        for _ in 128..256 {
            tracker.record_candidate();
        }
        assert!(tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_exponential_intervals_grow() {
        // Verify the check intervals are 64, 128, 256, 512...
        let mut tracker = PrefilterTracker::new();

        // Enough confirmations to never get disabled
        for i in 0u32..2048 {
            if !tracker.record_candidate() {
                panic!("tracker disabled at candidate {i} with good hit rate");
            }
            // Keep a healthy confirmation rate
            if i % 10 == 0 {
                tracker.record_confirmation();
            }
        }

        // With 10% hit rate, tracker should never disable.
        // Verify it's still active at 2048 candidates.
        assert!(!tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_short_haystack_tolerant() {
        let mut tracker = PrefilterTracker::new();
        tracker.set_haystack_len(500); // < 1KB -> 10% threshold

        // 64 candidates, 6 confirmations (9.4%) -- below 10% -> disabled.
        for i in 0u32..63 {
            tracker.record_candidate();
            if i % 11 == 0 {
                tracker.record_confirmation(); // 6 confirmations in 63 candidates
            }
        }
        // 64th: triggers check. 6/64 = 9.375% < 10% -> disabled.
        assert!(!tracker.record_candidate());
        assert!(tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_short_haystack_survives_with_high_rate() {
        let mut tracker = PrefilterTracker::new();
        tracker.set_haystack_len(500); // < 1KB -> 10% threshold

        // 64 candidates, 7 confirmations (10.9%) -- above 10% -> survives.
        for i in 0u32..63 {
            tracker.record_candidate();
            if i % 9 == 0 {
                tracker.record_confirmation(); // 7 confirmations
            }
        }
        // 64th: 7/64 = 10.9% > 10% -> survives.
        assert!(tracker.record_candidate());
        assert!(!tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_large_haystack_strict() {
        let mut tracker = PrefilterTracker::new();
        tracker.set_haystack_len(200_000); // > 100KB -> 0.5% threshold

        // 64 candidates, 0 confirmations -> 0% < 0.5% -> disabled.
        for _ in 0..63 {
            tracker.record_candidate();
        }
        assert!(!tracker.record_candidate());
        assert!(tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_large_haystack_survives_with_marginal_rate() {
        let mut tracker = PrefilterTracker::new();
        tracker.set_haystack_len(200_000); // > 100KB -> 0.5% threshold

        // 64 candidates, 1 confirmation = 1.56% > 0.5% -> survives.
        for _ in 0..32 {
            tracker.record_candidate();
        }
        tracker.record_confirmation();
        for _ in 32..63 {
            tracker.record_candidate();
        }
        assert!(tracker.record_candidate()); // check at 64: 1/64 = 1.56% > 0.5%
        assert!(!tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_medium_haystack_uses_1_percent() {
        let mut tracker = PrefilterTracker::new();
        tracker.set_haystack_len(50_000); // 1KB-100KB -> 1% threshold

        // 64 candidates, 0 confirmations -> 0% < 1% -> disabled.
        for _ in 0..63 {
            tracker.record_candidate();
        }
        assert!(!tracker.record_candidate());
        assert!(tracker.is_disabled());
    }

    #[test]
    fn prefilter_tracker_full_lifecycle() {
        let mut tracker = PrefilterTracker::new();
        tracker.set_haystack_len(50_000); // medium text, 1% threshold

        // Simulate a search with decent hit rate.
        for i in 0u32..200 {
            if !tracker.record_candidate() {
                panic!("tracker disabled at candidate {i} with good hit rate");
            }
            if i % 50 == 0 {
                tracker.record_confirmation();
            }
        }
        assert!(!tracker.is_disabled());

        // Reset and simulate a bad search.
        tracker.reset();
        tracker.set_haystack_len(50_000);
        for _ in 0..64 {
            tracker.record_candidate();
        }
        // 0 confirmations at check -> disabled.
        assert!(tracker.is_disabled());
    }
}
