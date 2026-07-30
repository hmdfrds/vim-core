//! Per-pattern execution statistics for adaptive strategy promotion.
//!
//! Tracks usage metrics for each pattern and recommends promotions
//! when patterns exceed thresholds.

/// Execution statistics for a single compiled pattern.
///
/// Accumulated across searches. When thresholds are exceeded, the
/// pattern may be promoted to a faster strategy (e.g., Pike VM
/// pattern that is DFA-eligible gets promoted to lazy DFA).
#[derive(Debug, Clone)]
pub struct PatternStats {
    /// Number of times this pattern has been executed.
    pub execution_count: u32,
    /// Total bytes searched across all executions.
    pub total_bytes_searched: u64,
    /// Total time spent in nanoseconds (if timing is enabled).
    pub total_time_ns: u64,
    /// The strategy used in the most recent search.
    pub last_strategy_used: Option<StrategyKind>,
}

/// Simplified strategy kind for stats tracking.
///
/// We don't store the full `Strategy` enum here because stats tracking
/// operates at a higher level (which engine was used, not which cascade
/// phase succeeded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyKind {
    /// Pure literal bypass.
    Literal,
    /// Aho-Corasick full match.
    AcFullMatch,
    /// Hybrid DFA acceleration.
    HybridDfa,
    /// One-pass DFA.
    OnePassDfa,
    /// Pike VM (NFA simulation).
    PikeVm,
    /// Bounded backtracker.
    Backtracker,
}

/// Promotion recommendation for a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotion {
    /// No promotion needed -- current strategy is optimal.
    None,
    /// Promote to lazy DFA (pattern is DFA-eligible but was using Pike VM).
    ForceDfa,
    /// Promote to one-pass DFA (anchored pattern with captures).
    ForceOnePass,
    /// Pattern uses a literal prefilter but the literal can serve as a
    /// full memchr replacement.
    ForceMemchr,
}

/// Thresholds for promotion decisions.
const EXECUTION_THRESHOLD: u32 = 10;
const BYTES_THRESHOLD: u64 = 100 * 1024; // 100 KB

impl PatternStats {
    /// Create empty stats.
    pub fn new() -> Self {
        Self {
            execution_count: 0,
            total_bytes_searched: 0,
            total_time_ns: 0,
            last_strategy_used: None,
        }
    }

    /// Record a search execution.
    pub fn record(&mut self, bytes_searched: usize, time_ns: u64, strategy: StrategyKind) {
        self.execution_count += 1;
        self.total_bytes_searched += bytes_searched as u64;
        self.total_time_ns += time_ns;
        self.last_strategy_used = Some(strategy);
    }

    /// Check if this pattern has exceeded promotion thresholds.
    pub fn exceeds_threshold(&self) -> bool {
        self.execution_count >= EXECUTION_THRESHOLD && self.total_bytes_searched >= BYTES_THRESHOLD
    }

    /// Recommend a promotion based on accumulated statistics.
    ///
    /// The caller provides `is_dfa_eligible` and `is_onepass_eligible`
    /// from the compiled pattern's properties.
    pub fn recommend_promotion(
        &self,
        is_dfa_eligible: bool,
        is_onepass_eligible: bool,
        is_literal: bool,
    ) -> Promotion {
        if !self.exceeds_threshold() {
            return Promotion::None;
        }

        match self.last_strategy_used {
            Some(StrategyKind::PikeVm) if is_dfa_eligible => Promotion::ForceDfa,
            Some(StrategyKind::PikeVm) if is_onepass_eligible => Promotion::ForceOnePass,
            Some(StrategyKind::Literal) if is_literal => Promotion::ForceMemchr,
            _ => Promotion::None,
        }
    }

    /// Average bytes per nanosecond (throughput metric).
    pub fn throughput_bytes_per_ns(&self) -> f64 {
        if self.total_time_ns == 0 {
            return 0.0;
        }
        self.total_bytes_searched as f64 / self.total_time_ns as f64
    }
}

impl Default for PatternStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stats_below_threshold() {
        let stats = PatternStats::new();
        assert!(!stats.exceeds_threshold());
        assert_eq!(
            stats.recommend_promotion(true, false, false),
            Promotion::None
        );
    }

    #[test]
    fn stats_exceed_threshold_after_many_executions() {
        let mut stats = PatternStats::new();
        for _ in 0..15 {
            stats.record(10 * 1024, 1000, StrategyKind::PikeVm);
        }
        assert!(stats.exceeds_threshold());
    }

    #[test]
    fn promote_pike_vm_to_dfa() {
        let mut stats = PatternStats::new();
        for _ in 0..15 {
            stats.record(10 * 1024, 1000, StrategyKind::PikeVm);
        }
        assert_eq!(
            stats.recommend_promotion(true, false, false),
            Promotion::ForceDfa,
        );
    }

    #[test]
    fn promote_pike_vm_to_onepass() {
        let mut stats = PatternStats::new();
        for _ in 0..15 {
            stats.record(10 * 1024, 1000, StrategyKind::PikeVm);
        }
        // Not DFA eligible but onepass eligible.
        assert_eq!(
            stats.recommend_promotion(false, true, false),
            Promotion::ForceOnePass,
        );
    }

    #[test]
    fn no_promotion_for_already_optimal() {
        let mut stats = PatternStats::new();
        for _ in 0..15 {
            stats.record(10 * 1024, 1000, StrategyKind::HybridDfa);
        }
        // Already using DFA -- no promotion needed.
        assert_eq!(
            stats.recommend_promotion(true, false, false),
            Promotion::None,
        );
    }

    #[test]
    fn no_promotion_below_bytes_threshold() {
        let mut stats = PatternStats::new();
        for _ in 0..15 {
            stats.record(100, 1000, StrategyKind::PikeVm); // 100 bytes each
        }
        // execution_count=15 exceeds 10, but total_bytes=1500 < 100KB.
        assert!(!stats.exceeds_threshold());
    }

    #[test]
    fn throughput_metric() {
        let mut stats = PatternStats::new();
        stats.record(1000, 500, StrategyKind::PikeVm);
        stats.record(2000, 500, StrategyKind::PikeVm);
        // total_bytes=3000, total_time=1000ns -> 3.0 bytes/ns.
        let throughput = stats.throughput_bytes_per_ns();
        assert!((throughput - 3.0).abs() < 0.001);
    }

    #[test]
    fn throughput_zero_time() {
        let stats = PatternStats::new();
        assert_eq!(stats.throughput_bytes_per_ns(), 0.0);
    }

    #[test]
    fn promote_literal_to_memchr() {
        let mut stats = PatternStats::new();
        for _ in 0..15 {
            stats.record(10 * 1024, 1000, StrategyKind::Literal);
        }
        assert_eq!(
            stats.recommend_promotion(false, false, true),
            Promotion::ForceMemchr,
        );
    }

    #[test]
    fn default_is_new() {
        let stats = PatternStats::default();
        assert_eq!(stats.execution_count, 0);
        assert_eq!(stats.total_bytes_searched, 0);
        assert_eq!(stats.total_time_ns, 0);
        assert!(stats.last_strategy_used.is_none());
    }
}
