//! Compilation pipeline: pattern string -> VimRegex.
//!
//! Phases: parse -> HIR lower -> auto-possessify -> NFA build ->
//! prefilter extraction -> reverse NFA (optional) -> AC prefilter (optional).

use compact_str::CompactString;
use smallvec::SmallVec;

use crate::accel;
use crate::hir::possessify::auto_possessify;
use crate::hir::{self, LoweredNode, PatternProperties};
use crate::ir::VimRegexError;
use crate::nfa::builder::NfaBuilder;
use crate::parser;
use crate::MagicMode;

use super::strategy::{is_dfa_eligible, select_engine, EngineKind, Strategy};
use super::VimRegex;

/// Compile a Vim regex pattern with an explicit magic mode.
///
/// This is the core compilation pipeline:
/// 1. Parse pattern text into IR (`VimPatternNode`)
/// 2. Lower IR to HIR (`LoweredNode` + `PatternProperties`)
/// 3. Auto-possessify eligible greedy quantifiers
/// 4. Build NFA from HIR
/// 5. Extract prefilters (memchr, substring, AC)
/// 6. Build reverse NFA if eligible (for suffix/inner-guided search)
/// 7. Compute AC prefilters for literal alternations
/// 8. Build strategy cascade
pub(crate) fn compile_pattern(pattern: &str, magic: MagicMode) -> Result<VimRegex, VimRegexError> {
    compile_pattern_with_config(pattern, magic, &super::SearchConfig::default())
}

/// Compile a Vim regex pattern with a search config controlling strategy selection.
pub(crate) fn compile_pattern_with_config(
    pattern: &str,
    magic: MagicMode,
    config: &super::SearchConfig,
) -> Result<VimRegex, VimRegexError> {
    let parsed = parser::parse_with_magic(pattern, magic).map_err(|e| e.with_pattern(pattern))?;
    let (mut lowered, mut properties) = hir::lower(&parsed.node);
    if config.possessify_enabled && auto_possessify(&mut lowered, parsed.case_mode) {
        // The tree now contains Atomic nodes that were not in the original
        // pattern.  Update the feature flag so engine selection routes to
        // the Backtracker, which correctly handles Atomic (position-
        // advancing) groups.  The PikeVM treats Lookaround as zero-width
        // and cannot execute atomic groups.
        properties.features.has_atomic = true;
    }
    let nfa = NfaBuilder::build(&lowered).map_err(|e| e.with_pattern(pattern))?;
    let prefilter = accel::build_prefilter(&properties, &lowered, parsed.case_mode);
    let ci_prefilter = accel::build_case_insensitive_prefilter(&properties);
    let inner_literal = accel::extract_inner_literal(&lowered);
    let engine_kind = config
        .force_engine
        .unwrap_or_else(|| select_engine(&properties));

    let reverse_nfa = if config.reverse_enabled
        && !properties.has_backreferences()
        && !properties.has_last_substitute()
        && !properties.has_atomic()
    {
        Some(NfaBuilder::build_reverse(&lowered).map_err(|e| e.with_pattern(pattern))?)
    } else {
        None
    };

    let suffix_literal = properties.accel_hints.literal_suffix.clone();

    let inner_literal_pos = accel::extract_inner_literal_with_position(&lowered);

    let prefix_reverse_nfa =
        if config.reverse_enabled && reverse_nfa.is_some() && suffix_literal.is_none() {
            match inner_literal_pos.as_ref() {
                Some(info) if info.split_index >= 1 => {
                    if let LoweredNode::Sequence(children) = &lowered {
                        let prefix_node = if info.split_index == 1 {
                            children[0].clone()
                        } else {
                            LoweredNode::Sequence(children[..info.split_index].to_vec())
                        };
                        let nfa = NfaBuilder::build_reverse(&prefix_node)
                            .map_err(|e| e.with_pattern(pattern))?;
                        if nfa.state_count() > 2 {
                            Some(nfa)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };

    let ac_literals = if config.ac_enabled {
        accel::aho_corasick::extract_ac_literals(&lowered)
            .filter(|lits| accel::aho_corasick::should_use_ac(lits))
    } else {
        None
    };
    let ac_prefilter = ac_literals
        .as_ref()
        .and_then(|lits| accel::aho_corasick::AcPrefilter::new(lits, false));
    let ac_prefilter_ci = ac_literals
        .as_ref()
        .and_then(|lits| accel::aho_corasick::AcPrefilter::new(lits, true));
    let ac_is_full_match = (ac_prefilter.is_some() || ac_prefilter_ci.is_some())
        && properties.capture_count() == 0
        && !properties.has_match_override()
        && !properties.has_lookaround()
        && !properties.has_backreferences();

    let max_reverse_distance = match properties.maximum_match_len() {
        Some(len) => len.saturating_add(16),
        None => usize::MAX,
    };

    let ci_suffix_literal = suffix_literal.as_ref().and_then(|s| {
        let lower: String = s
            .chars()
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect();
        if lower.len() == s.len() {
            Some(CompactString::from(lower))
        } else {
            None
        }
    });

    let required_byte = accel::extract_required_byte(&lowered);

    let suffix_literal_extracted = accel::extract_suffix_literal(&lowered);

    let prefilter_tree = accel::build_prefilter_tree(&lowered);

    let start_bitmap = accel::compute_start_bitmap(&lowered, parsed.case_mode);

    let min_match_length = properties.minimum_match_len();

    // When start-position / fast-reject acceleration is disabled, strip every
    // accelerator that could prune a candidate start position so the terminal
    // engine scan visits every position. This is the soundness ground truth
    // used by test_builder invariant 19. `inner_literal` is included because it
    // is the primary source for `bloom_literal()`'s line fast-reject; we ALSO
    // clear `accel_hints.literal_prefix` below — `bloom_literal()` falls back to
    // it when `inner_literal` is None, so without clearing it the dispatch-level
    // bloom line-scan (dispatch.rs) could still prune positions for large
    // newline-containing inputs, making invariant 19 a false-negative there.
    let (
        prefilter,
        ci_prefilter,
        inner_literal,
        required_byte,
        suffix_literal_extracted,
        prefilter_tree,
        start_bitmap,
    ) = if config.prefilters_enabled {
        (
            prefilter,
            ci_prefilter,
            inner_literal,
            required_byte,
            suffix_literal_extracted,
            prefilter_tree,
            start_bitmap,
        )
    } else {
        // Neutralize the bloom fallback needle so `bloom_literal()` returns None
        // and the dispatch bloom reject is skipped entirely. Strictly gated to
        // this `!prefilters_enabled` (test-only) branch, so the production
        // default path is byte-identical.
        properties.accel_hints.literal_prefix = None;
        (None, None, None, None, None, None, None)
    };

    // None = no prefilter, Some(true) = fast, Some(false) = slow.
    let prefilter_speed: Option<bool> = prefilter.as_ref().map(|pf| pf.is_fast());

    let strategies = build_strategies(
        &properties,
        engine_kind,
        reverse_nfa.is_some(),
        suffix_literal.is_some() || ci_suffix_literal.is_some(),
        prefix_reverse_nfa.is_some(),
        config,
        prefilter_speed,
    );

    Ok(VimRegex {
        pattern: CompactString::new(pattern),
        ir: parsed.node,
        nfa,
        properties,
        case_mode: parsed.case_mode,
        composing_mode: parsed.composing_mode,
        prefilter,
        ci_prefilter,
        inner_literal,
        engine_kind,
        reverse_nfa,
        suffix_literal,
        prefix_reverse_nfa,
        ac_prefilter,
        ac_prefilter_ci,
        ac_is_full_match,
        max_reverse_distance,
        ci_suffix_literal,
        required_byte,
        suffix_literal_extracted,
        prefilter_tree,
        start_bitmap,
        min_match_length,
        strategies,
        all_errors: parsed.additional_errors,
    })
}

/// Build the ordered strategy cascade based on pattern properties.
///
/// `prefilter_speed` indicates the prefilter state:
/// - `None` = no prefilter exists
/// - `Some(true)` = fast (SIMD-accelerated) prefilter
/// - `Some(false)` = slow (linear scan) prefilter
fn build_strategies(
    properties: &PatternProperties,
    engine_kind: EngineKind,
    has_reverse_nfa: bool,
    has_suffix_literal: bool,
    has_prefix_reverse_nfa: bool,
    config: &super::SearchConfig,
    prefilter_speed: Option<bool>,
) -> SmallVec<[Strategy; 5]> {
    let mut strategies: SmallVec<[Strategy; 5]> = SmallVec::new();

    // SmallWrite fast path is always first: it is the cheapest check.
    strategies.push(Strategy::SmallWrite);

    if config.literal_bypass_enabled && properties.is_literal() && properties.capture_count() == 0 {
        strategies.push(Strategy::LiteralBypass);
    }

    if config.ac_enabled {
        strategies.push(Strategy::AcFullMatch);
    }

    if properties.is_anchored_start_of_file() && engine_kind == EngineKind::PikeVm {
        strategies.push(Strategy::AnchoredStart);
    }

    if config.reverse_enabled && has_reverse_nfa && engine_kind == EngineKind::PikeVm {
        if properties.is_anchored_end_of_file() {
            strategies.push(Strategy::ReverseAnchored);
        }
        // Reverse suffix/inner strategies use prune-on-accept which finds
        // the CLOSEST match start to the suffix, not the LEFTMOST. This is
        // incorrect for patterns with lazy quantifiers where the leftmost
        // match starts before the closest reverse-confirmed position.
        // Skip these strategies for lazy patterns; EngineDispatch handles
        // them correctly via position-by-position scanning.
        //
        // Additionally, skip reverse strategies when the prefilter exists
        // but is slow (is_fast = false). A slow prefilter (e.g. ByteSet
        // linear scan) negates the benefit of reverse-confirmed search --
        // the forward confirmation scan becomes the bottleneck. If no
        // prefilter exists at all, reverse strategies are still beneficial
        // since they use suffix/inner literals directly.
        //
        // - prefilter_speed = None      -> no prefilter; reverse is OK
        // - prefilter_speed = Some(true)  -> fast prefilter; reverse is OK
        // - prefilter_speed = Some(false) -> slow prefilter; skip reverse
        let has_slow_prefilter = prefilter_speed == Some(false);
        let reverse_eligible = !properties.has_lazy_quantifier() && !has_slow_prefilter;
        if has_suffix_literal && reverse_eligible {
            strategies.push(Strategy::ReverseSuffix);
        }
        if has_prefix_reverse_nfa && reverse_eligible {
            strategies.push(Strategy::ReverseInner);
        }
    }

    if config.dfa_enabled && is_dfa_eligible(properties) {
        strategies.push(Strategy::HybridDfa);
    }

    // One-pass DFA for anchored patterns with captures (between DFA and engine dispatch).
    // Declines for non-anchored, non-capture, or non-eligible patterns.
    strategies.push(Strategy::OnePassDfa);

    // Engine dispatch is ALWAYS last -- never declines.
    strategies.push(Strategy::EngineDispatch);

    strategies
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_strategies_excluded_with_slow_prefilter() {
        let properties = PatternProperties::default();
        let config = super::super::SearchConfig::default();

        // Slow prefilter: reverse strategies should be excluded.
        let strategies_slow = build_strategies(
            &properties,
            EngineKind::PikeVm,
            true, // has_reverse_nfa
            true, // has_suffix_literal
            true, // has_prefix_reverse_nfa
            &config,
            Some(false), // slow prefilter
        );
        assert!(
            !strategies_slow
                .iter()
                .any(|s| matches!(s, Strategy::ReverseSuffix)),
            "ReverseSuffix should be excluded with slow prefilter"
        );
        assert!(
            !strategies_slow
                .iter()
                .any(|s| matches!(s, Strategy::ReverseInner)),
            "ReverseInner should be excluded with slow prefilter"
        );

        // Fast prefilter: reverse strategies should be included.
        let strategies_fast = build_strategies(
            &properties,
            EngineKind::PikeVm,
            true,
            true,
            true,
            &config,
            Some(true), // fast prefilter
        );
        assert!(
            strategies_fast
                .iter()
                .any(|s| matches!(s, Strategy::ReverseSuffix)),
            "ReverseSuffix should be included with fast prefilter"
        );
        assert!(
            strategies_fast
                .iter()
                .any(|s| matches!(s, Strategy::ReverseInner)),
            "ReverseInner should be included with fast prefilter"
        );

        // No prefilter: reverse strategies should still be included.
        let strategies_none = build_strategies(
            &properties,
            EngineKind::PikeVm,
            true,
            true,
            true,
            &config,
            None, // no prefilter
        );
        assert!(
            strategies_none
                .iter()
                .any(|s| matches!(s, Strategy::ReverseSuffix)),
            "ReverseSuffix should be included with no prefilter"
        );
        assert!(
            strategies_none
                .iter()
                .any(|s| matches!(s, Strategy::ReverseInner)),
            "ReverseInner should be included with no prefilter"
        );
    }

    #[test]
    fn strategies_always_end_with_engine_dispatch() {
        let properties = PatternProperties::default();
        let config = super::super::SearchConfig::default();

        let strategies = build_strategies(
            &properties,
            EngineKind::PikeVm,
            false,
            false,
            false,
            &config,
            None,
        );
        assert_eq!(strategies.last().copied(), Some(Strategy::EngineDispatch));
    }

    #[test]
    fn reverse_anchored_not_gated_by_prefilter_speed() {
        // ReverseAnchored is gated only by is_anchored_end_of_file,
        // NOT by prefilter speed.
        let mut properties = PatternProperties::default();
        properties.accel_hints.is_anchored_end_of_file = true;
        let config = super::super::SearchConfig::default();

        let strategies = build_strategies(
            &properties,
            EngineKind::PikeVm,
            true, // has_reverse_nfa
            false,
            false,
            &config,
            Some(false), // slow prefilter
        );
        assert!(
            strategies
                .iter()
                .any(|s| matches!(s, Strategy::ReverseAnchored)),
            "ReverseAnchored should not be affected by prefilter speed"
        );
    }
}
