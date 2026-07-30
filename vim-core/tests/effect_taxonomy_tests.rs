//! Tests for [`EffectKind::tier`] and [`EffectKind::is_text_mutation`].

use vim_core::effects::{EffectKind, EffectTier};

#[test]
fn every_effect_kind_has_a_tier() {
    // The exhaustive match in `tier()` guarantees this at compile time,
    // but this test documents intent: every variant in ALL is classified.
    for kind in EffectKind::ALL {
        let _ = kind.tier(); // must not panic
    }
}

#[test]
fn core_tier_has_16_variants() {
    let count = EffectKind::ALL
        .iter()
        .filter(|k| k.tier() == EffectTier::Core)
        .count();
    assert_eq!(count, 16, "Core tier should have exactly 16 variants");
}

#[test]
fn is_text_mutation_returns_true_for_exactly_7_variants() {
    let mutations: Vec<EffectKind> = EffectKind::ALL
        .iter()
        .copied()
        .filter(|k| k.is_text_mutation())
        .collect();
    assert_eq!(
        mutations.len(),
        7,
        "Expected exactly 7 text mutation variants, got {mutations:?}"
    );
    let expected = [
        EffectKind::Insert,
        EffectKind::Delete,
        EffectKind::Replace,
        EffectKind::OperatorToMark,
        EffectKind::Undo,
        EffectKind::UndoLine,
        EffectKind::Redo,
    ];
    for e in &expected {
        assert!(mutations.contains(e), "{e:?} should be a text mutation");
    }
}

#[test]
fn text_mutations_are_all_core_tier() {
    for kind in EffectKind::ALL {
        if kind.is_text_mutation() {
            assert_eq!(
                kind.tier(),
                EffectTier::Core,
                "{kind:?} is a text mutation but not Core tier"
            );
        }
    }
}
