//! Effect inspection — fluent query API over captured effects.

use vim_core::effects::{Effect, EffectKind, EffectTier};

/// Captured effects grouped by keystroke step.
pub struct EffectLog {
    /// Effects per keystroke step.
    steps: Vec<Vec<Effect>>,
}

impl EffectLog {
    /// Create an empty effect log.
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// Record effects from one keystroke.
    pub fn record_step(&mut self, effects: Vec<Effect>) {
        self.steps.push(effects);
    }

    /// Effects from the most recent step.
    pub fn last(&self) -> &[Effect] {
        self.steps.last().map_or(&[], |v| v.as_slice())
    }

    /// Effects grouped by step.
    pub fn steps(&self) -> &[Vec<Effect>] {
        &self.steps
    }

    /// Effects from a specific step index.
    pub fn step(&self, n: usize) -> &[Effect] {
        self.steps.get(n).map_or(&[], |v| v.as_slice())
    }

    /// All effects flattened.
    pub fn all_flat(&self) -> Vec<&Effect> {
        self.steps.iter().flat_map(|v| v.iter()).collect()
    }

    /// Total number of steps recorded.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Total number of effects across all steps.
    pub fn total_count(&self) -> usize {
        self.steps.iter().map(|v| v.len()).sum()
    }

    /// Create an `EffectInspector` over the most recent step's effects.
    pub fn inspector(&self) -> EffectInspector<'_> {
        EffectInspector::new(self.last())
    }

    /// Create an `EffectInspector` over all effects.
    pub fn inspector_all(&self) -> EffectInspector<'_> {
        let effects: Vec<&Effect> = self.all_flat();
        EffectInspector { effects }
    }
}

/// Fluent inspector over a slice of captured effects.
///
/// ```ignore
/// let inspector = EffectInspector::new(session.last_effects());
/// inspector
///     .of_kind(EffectKind::Delete).expect_count(1)
///     .expect_no_kind(EffectKind::Insert);
/// ```
pub struct EffectInspector<'a> {
    effects: Vec<&'a Effect>,
}

impl<'a> EffectInspector<'a> {
    /// Create an inspector over a slice of effects.
    pub fn new(effects: &'a [Effect]) -> Self {
        Self {
            effects: effects.iter().collect(),
        }
    }

    /// Filter to only effects of a specific kind.
    #[must_use]
    pub fn of_kind(&self, kind: EffectKind) -> Self {
        Self {
            effects: self
                .effects
                .iter()
                .filter(|e| e.kind() == kind)
                .copied()
                .collect(),
        }
    }

    /// Filter to multiple kinds.
    #[must_use]
    pub fn of_kinds(&self, kinds: &[EffectKind]) -> Self {
        Self {
            effects: self
                .effects
                .iter()
                .filter(|e| kinds.contains(&e.kind()))
                .copied()
                .collect(),
        }
    }

    /// Exclude effects of a specific kind.
    #[must_use]
    pub fn excluding(&self, kind: EffectKind) -> Self {
        Self {
            effects: self
                .effects
                .iter()
                .filter(|e| e.kind() != kind)
                .copied()
                .collect(),
        }
    }

    /// Only text mutation effects (Insert, Delete, Replace).
    #[must_use]
    pub fn text_mutations(&self) -> Self {
        self.of_kinds(&[EffectKind::Insert, EffectKind::Delete, EffectKind::Replace])
    }

    /// Exclude Internal-tier effects.
    #[must_use]
    pub fn excluding_internal(&self) -> Self {
        Self {
            effects: self
                .effects
                .iter()
                .filter(|e| e.kind().tier() != EffectTier::Internal)
                .copied()
                .collect(),
        }
    }

    /// Assert exactly `n` effects in the working set.
    #[track_caller]
    pub fn expect_count(&self, n: usize) -> &Self {
        let actual = self.effects.len();
        assert!(
            actual == n,
            "EFFECT ASSERTION FAILED: expected {n} effects, got {actual}\n\
             \x20 working set: {:?}",
            self.kind_list()
        );
        self
    }

    /// Assert zero effects in the working set.
    #[track_caller]
    pub fn expect_none(&self) -> &Self {
        self.expect_count(0)
    }

    /// Assert at least one effect in the working set.
    #[track_caller]
    pub fn expect_some(&self) -> &Self {
        assert!(
            !self.effects.is_empty(),
            "EFFECT ASSERTION FAILED: expected at least one effect, got none"
        );
        self
    }

    /// Assert the exact sequence of effect kinds matches.
    #[track_caller]
    pub fn expect_sequence(&self, expected: &[EffectKind]) -> &Self {
        let actual: Vec<EffectKind> = self.kind_list();
        assert!(
            actual == expected,
            "EFFECT SEQUENCE MISMATCH\n\
             \x20 expected: {expected:?}\n\
             \x20 actual:   {actual:?}"
        );
        self
    }

    /// Assert that the expected kinds appear as a subsequence (in order, possibly with gaps).
    #[track_caller]
    pub fn expect_sequence_contains(&self, expected: &[EffectKind]) -> &Self {
        let actual = self.kind_list();
        let mut ai = 0;
        for &exp in expected {
            while ai < actual.len() && actual[ai] != exp {
                ai += 1;
            }
            assert!(
                ai < actual.len(),
                "EFFECT SUBSEQUENCE MISMATCH: {exp:?} not found after position {ai}\n\
                 \x20 expected subsequence: {expected:?}\n\
                 \x20 actual sequence:      {actual:?}"
            );
            ai += 1;
        }
        self
    }

    /// Assert no effects of a specific kind exist.
    #[track_caller]
    pub fn expect_no_kind(&self, kind: EffectKind) -> &Self {
        let count = self.effects.iter().filter(|e| e.kind() == kind).count();
        assert!(
            count == 0,
            "EFFECT ASSERTION FAILED: expected no {kind:?} effects, found {count}"
        );
        self
    }

    /// Assert that `before` kind appears before `after` kind.
    #[track_caller]
    pub fn expect_before(&self, before: EffectKind, after: EffectKind) -> &Self {
        let before_pos = self.effects.iter().position(|e| e.kind() == before);
        let after_pos = self.effects.iter().position(|e| e.kind() == after);
        match (before_pos, after_pos) {
            (Some(b), Some(a)) => assert!(
                b < a,
                "EFFECT ORDERING: expected {before:?} before {after:?}, \
                 but {before:?} at index {b}, {after:?} at index {a}"
            ),
            (None, _) => panic!("EFFECT ORDERING: {before:?} not found in effects"),
            (_, None) => panic!("EFFECT ORDERING: {after:?} not found in effects"),
        }
        self
    }

    /// Assert all effects in the working set match a predicate.
    #[track_caller]
    pub fn expect_all(&self, predicate: impl Fn(&Effect) -> bool) -> &Self {
        for (i, effect) in self.effects.iter().enumerate() {
            assert!(
                predicate(effect),
                "EFFECT PREDICATE FAILED at index {i}: {effect:?}"
            );
        }
        self
    }

    /// Find the first effect of a specific kind.
    #[must_use]
    pub fn find_first(&self, kind: EffectKind) -> Option<&'a Effect> {
        self.effects.iter().find(|e| e.kind() == kind).copied()
    }

    /// Get the Nth effect in the working set.
    #[must_use]
    pub fn nth(&self, n: usize) -> Option<&'a Effect> {
        self.effects.get(n).copied()
    }

    /// Number of effects in the working set.
    #[must_use]
    pub fn count(&self) -> usize {
        self.effects.len()
    }

    fn kind_list(&self) -> Vec<EffectKind> {
        self.effects.iter().map(|e| e.kind()).collect()
    }
}
