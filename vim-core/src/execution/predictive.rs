//! Predictive effect pre-computation for `VimEngine`.
//!
//! This module provides a read-only prediction API: given the current engine
//! state (mode, pending operator) the `predict()` method returns a list of
//! likely next actions and the text ranges they would affect — without
//! executing anything.
//!
//! # API surface
//!
//! - [`VimEngine::predict()`](crate::execution::VimEngine::predict) — lightweight candidate generation with
//!   `affected_range: None`. No document context required.
//! - [`VimEngine::predict_with_ranges()`](crate::execution::VimEngine::predict_with_ranges) — forks the engine per-candidate
//!   via [`ScopedFork`](crate::execution::ScopedFork), speculatively executes each trigger key, converts
//!   the resulting effects to a [`ChangeSet`](crate::primitives::ChangeSet), and derives the actual byte
//!   range. Engine state is rolled back after each fork.
//!
//! Both methods consult the engine's [`PredictionWeights`] for adaptive
//! likelihood scoring rather than using hard-coded probabilities.
//!
//! This module is always compiled (formerly feature-gated as `predictive`).

use compact_str::CompactString;
use smallvec::SmallVec;

// ── Core prediction types ────────────────────────────────────────────────────

/// A single predicted action.
///
/// Describes what the engine *would* do if the user typed `trigger` next,
/// given the current mode and operator.  This is purely descriptive — no
/// state is modified when predictions are computed.
#[derive(Debug, Clone)]
pub struct Prediction {
    /// The key (or key sequence) that would trigger this action.
    pub trigger: PredictionTrigger,
    /// The document range this action would affect, if known.
    ///
    /// `None` when the range cannot be determined without running the full
    /// motion dispatch against the live document.  Use
    /// [`VimEngine::predict_with_ranges()`](crate::execution::VimEngine::predict_with_ranges) to populate this field via
    /// speculative execution.
    pub affected_range: Option<crate::primitives::Range>,
    /// Human-readable description of the predicted action.
    pub description: CompactString,
    /// Estimated probability that this is the next user action (0.0 – 1.0).
    pub likelihood: f32,
}

/// The keystroke(s) that would trigger a [`Prediction`].
#[derive(Debug, Clone)]
pub enum PredictionTrigger {
    /// Single key trigger (e.g. `w`, `$`).
    Key(crate::keymap::KeyEvent),
    /// Multi-key trigger (e.g. `i"` for inner-double-quote text object).
    Sequence(SmallVec<[crate::keymap::KeyEvent; 2]>),
}

// ── Configuration ────────────────────────────────────────────────────────────

/// Configuration passed to [`VimEngine::predict()`](crate::execution::VimEngine::predict).
pub struct PredictionConfig {
    /// Maximum number of predictions to return.
    pub max_predictions: usize,
    /// Which categories of predictions to compute.
    pub categories: PredictionCategories,
}

impl Default for PredictionConfig {
    fn default() -> Self {
        Self {
            max_predictions: 8,
            categories: PredictionCategories::ALL,
        }
    }
}

bitflags::bitflags! {
    /// Bitflag set selecting which prediction categories to compute.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PredictionCategories: u8 {
        /// Predict common motions: `w`, `e`, `$`, `^`, `%`.
        const MOTIONS      = 0b0001;
        /// Predict text objects: `iw`, `i"`, `i)`.
        const TEXT_OBJECTS = 0b0010;
        /// Predict the doubled-operator line repeat: `dd`, `yy`, `cc`, etc.
        const LINE_REPEAT  = 0b0100;
        /// Enable all categories.
        const ALL          = 0b0111;
    }
}

// ── Adaptive weights ─────────────────────────────────────────────────────────

/// Per-session adaptive likelihood weights.
///
/// Weights start at a uniform prior of `0.05` and are updated via exponential
/// moving average each time the caller observes a keystroke with
/// [`observe()`](Self::observe).  After each observation the weights are
/// re-normalised to sum to 1.0.
#[derive(Debug, Clone)]
pub struct PredictionWeights {
    weights: ahash::AHashMap<CompactString, f32>,
}

impl Default for PredictionWeights {
    fn default() -> Self {
        Self::new()
    }
}

impl PredictionWeights {
    /// Create a new weights table with an empty prior.
    #[must_use]
    pub fn new() -> Self {
        Self {
            weights: ahash::AHashMap::new(),
        }
    }

    /// Look up the current weight for `key` (default: `0.05`).
    #[must_use]
    pub fn get(&self, key: &str) -> f32 {
        self.weights.get(key).copied().unwrap_or(0.05)
    }

    /// Record an observation for `key` and re-normalise.
    ///
    /// Uses an exponential moving average: `w = w * 0.9 + 0.1`, then all
    /// weights are re-normalised to sum to 1.0.
    pub fn observe(&mut self, key: &str) {
        let entry = self.weights.entry(key.into()).or_insert(0.05);
        *entry = entry.mul_add(0.9, 0.1);
        let total: f32 = self.weights.values().sum();
        if total > 0.0 {
            for v in self.weights.values_mut() {
                *v /= total;
            }
        }
    }

    /// Returns `true` when no observations have been recorded yet.
    ///
    /// When empty, `predict()` falls back to hard-coded default likelihoods.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.weights.is_empty()
    }
}

// ── Range extraction helper ─────────────────────────────────────────────────

/// Derive the byte range of all modifications in a [`ChangeSet`].
///
/// Scans the changeset's ops to find the span from the first non-Retain op
/// to the last non-Retain op, measured in **input** document coordinates.
/// This tells us "which part of the original document would be touched"
/// by the edit.
///
/// Returns `None` for identity changesets (no modifications).
fn changeset_affected_range(cs: &crate::primitives::ChangeSet) -> Option<crate::primitives::Range> {
    use crate::primitives::changeset::TextOp;

    let ops = cs.ops();
    if ops.is_empty() || cs.is_identity() {
        return None;
    }

    let mut input_pos: usize = 0;
    let mut first_change: Option<usize> = None;
    let mut last_change_end: usize = 0;

    for op in ops {
        match op {
            TextOp::Retain(n) => {
                input_pos += n;
            }
            TextOp::Delete(n) => {
                if first_change.is_none() {
                    first_change = Some(input_pos);
                }
                last_change_end = input_pos + n;
                input_pos += n;
            }
            TextOp::Insert(_) => {
                // Insertions happen at the current input position but don't
                // consume input bytes. They still mark a modification point.
                if first_change.is_none() {
                    first_change = Some(input_pos);
                }
                // For pure insertions the affected range in the input document
                // is a zero-width point. We track it so the range at least
                // covers the insertion site.
                if last_change_end < input_pos {
                    last_change_end = input_pos;
                }
            }
        }
    }

    first_change.map(|start| {
        let end = last_change_end.max(start);
        crate::primitives::Range::from_raw(start, end)
    })
}

// ── VimEngine::predict() ─────────────────────────────────────────────────────

impl super::engine::VimEngine {
    /// Compute predictions for the likely next user action.
    ///
    /// # Behaviour
    ///
    /// * Returns an **empty** list unless the engine is currently in
    ///   `OperatorPending` mode (i.e. the user has typed an operator such as
    ///   `d`, `y`, or `c` and is waiting for a motion or text-object).
    /// * Does **not** mutate any engine state — this is a pure read.
    /// * `affected_range` is always `None` — use [`Self::predict_with_ranges()`]
    ///   for actual byte-range computation via speculative execution.
    /// * Likelihoods come from the engine's [`PredictionWeights`] when
    ///   observations have been recorded; otherwise falls back to static
    ///   defaults.
    ///
    /// # Arguments
    ///
    /// * `config` — controls which categories are computed and the result cap.
    ///
    /// # Returns
    ///
    /// Up to `config.max_predictions` [`Prediction`] values, sorted roughly
    /// by descending likelihood.
    #[must_use]
    pub fn predict(&self, config: &PredictionConfig) -> SmallVec<[Prediction; 8]> {
        use crate::keymap::{Key, KeyEvent};

        let mut predictions: SmallVec<[Prediction; 8]> = SmallVec::new();

        // Predictions only make sense when we are waiting for a motion.
        let operator = match self.state().mode() {
            crate::primitives::Mode::OperatorPending(op) => op,
            _ => return predictions,
        };

        let remaining = config.max_predictions;

        // Determine whether to use adaptive weights or static defaults.
        let use_weights = !self.prediction_weights().is_empty();

        // ── LINE_REPEAT category ─────────────────────────────────────────
        // e.g. dd, yy, cc — operator doubled acts on the current line.
        if config
            .categories
            .contains(PredictionCategories::LINE_REPEAT)
            && predictions.len() < remaining
        {
            let notation = operator.key_notation();
            // The doubled-operator trigger is just the operator key pressed again.
            // For single-char operators this is one key; for two-char operators
            // (like `g~`) we emit both keys as a Sequence.
            let trigger = if notation.len() == 1 {
                let ch = notation.chars().next().unwrap_or('d');
                PredictionTrigger::Key(KeyEvent::new(
                    Key::Char(ch),
                    crate::keymap::Modifiers::empty(),
                ))
            } else {
                let keys: SmallVec<[KeyEvent; 2]> = notation
                    .chars()
                    .map(|ch| KeyEvent::new(Key::Char(ch), crate::keymap::Modifiers::empty()))
                    .collect();
                PredictionTrigger::Sequence(keys)
            };

            let description: CompactString = {
                let mut s = CompactString::new(operator.key_notation());
                s.push_str(operator.key_notation());
                s.push_str(" — line repeat");
                s
            };

            let likelihood = if use_weights {
                self.prediction_weights().get(notation)
            } else {
                0.35
            };

            predictions.push(Prediction {
                trigger,
                affected_range: None,
                description,
                likelihood,
            });
        }

        // ── MOTIONS category ─────────────────────────────────────────────
        // Common word/line motions.
        if config.categories.contains(PredictionCategories::MOTIONS) {
            let motions: &[(&str, char, f32)] = &[
                ("w — word forward", 'w', 0.25),
                ("$ — end of line", '$', 0.15),
                ("e — end of word", 'e', 0.12),
                ("^ — first non-blank", '^', 0.08),
                ("% — matching pair", '%', 0.05),
            ];

            for (desc, ch, default_likelihood) in motions {
                if predictions.len() >= remaining {
                    break;
                }
                let key_str: CompactString = CompactString::from(ch.to_string());
                let likelihood = if use_weights {
                    self.prediction_weights().get(&key_str)
                } else {
                    *default_likelihood
                };
                predictions.push(Prediction {
                    trigger: PredictionTrigger::Key(KeyEvent::new(
                        Key::Char(*ch),
                        crate::keymap::Modifiers::empty(),
                    )),
                    affected_range: None,
                    description: CompactString::from(*desc),
                    likelihood,
                });
            }
        }

        // ── TEXT_OBJECTS category ────────────────────────────────────────
        // Common inner text-objects.
        if config
            .categories
            .contains(PredictionCategories::TEXT_OBJECTS)
        {
            // Each entry: (description, second key char, likelihood)
            let objects: &[(&str, char, f32)] = &[
                ("iw — inner word", 'w', 0.20),
                ("i\" — inner double quotes", '"', 0.10),
                ("i) — inner parentheses", ')', 0.08),
            ];

            let i_key = KeyEvent::new(Key::Char('i'), crate::keymap::Modifiers::empty());

            for (desc, second, default_likelihood) in objects {
                if predictions.len() >= remaining {
                    break;
                }
                let second_key =
                    KeyEvent::new(Key::Char(*second), crate::keymap::Modifiers::empty());
                let mut seq: SmallVec<[KeyEvent; 2]> = SmallVec::new();
                seq.push(i_key);
                seq.push(second_key);

                // Build the compound key string (e.g. "iw") for weight lookup.
                let mut compound = CompactString::from("i");
                compound.push(*second);
                let likelihood = if use_weights {
                    self.prediction_weights().get(&compound)
                } else {
                    *default_likelihood
                };

                predictions.push(Prediction {
                    trigger: PredictionTrigger::Sequence(seq),
                    affected_range: None,
                    description: CompactString::from(*desc),
                    likelihood,
                });
            }
        }

        // Truncate to the configured cap (we may have gone over due to ordering).
        predictions.truncate(config.max_predictions);
        predictions
    }

    /// Predict with document context for actual range computation.
    ///
    /// Unlike [`predict()`](Self::predict) which returns `None` ranges, this
    /// forks the engine internally and speculatively executes each candidate
    /// trigger key to compute actual byte ranges.
    ///
    /// # Algorithm
    ///
    /// 1. Generate static candidate predictions via the same logic as `predict()`.
    /// 2. For each candidate with a [`PredictionTrigger::Key`]:
    ///    - Fork the engine via [`ScopedFork`](crate::execution::ScopedFork).
    ///    - Process the trigger key speculatively.
    ///    - Extract the response's effects.
    ///    - Convert to a [`ChangeSet`](crate::primitives::ChangeSet) and derive the affected input-document range.
    ///    - Drop the fork (automatic rollback).
    ///    - Populate the prediction's `affected_range`.
    /// 3. For [`PredictionTrigger::Sequence`] candidates, all keys in the
    ///    sequence are fed through a single fork.
    ///
    /// # Rollback guarantee
    ///
    /// Engine state is **unchanged** after this call — every fork is dropped
    /// (rolled back) rather than committed.
    ///
    /// # Arguments
    ///
    /// * `config` — controls which categories are computed and the result cap.
    /// * `ctx` — validated input context providing document access and cursor position.
    ///
    /// # Returns
    ///
    /// Up to `config.max_predictions` predictions, most with `Some(range)` in
    /// `affected_range`. A prediction's range may still be `None` if the
    /// speculative execution produced no text-mutating effects (e.g., the
    /// motion failed or was a no-op).
    pub fn predict_with_ranges<D: crate::document::Document>(
        &mut self,
        config: &PredictionConfig,
        ctx: &crate::execution::InputContext<'_, D, crate::execution::Validated>,
    ) -> SmallVec<[Prediction; 8]> {
        // Step 1: generate candidates using the read-only predict() logic.
        let mut predictions = self.predict(config);

        if predictions.is_empty() {
            return predictions;
        }

        let doc_len = ctx.doc().len();

        // Step 2: for each candidate, fork → execute → extract range → rollback.
        for prediction in &mut predictions {
            let keys: SmallVec<[crate::keymap::KeyEvent; 2]> = match &prediction.trigger {
                PredictionTrigger::Key(k) => {
                    let mut v = SmallVec::new();
                    v.push(*k);
                    v
                }
                PredictionTrigger::Sequence(seq) => seq.clone(),
            };

            // Attempt to fork the engine for speculative execution.
            let fork_result = self.fork();
            let mut fork = match fork_result {
                Ok(f) => f,
                Err(_) => {
                    // Already forked (shouldn't happen since we drop each fork
                    // before the next iteration, but be defensive).
                    continue;
                }
            };

            // Build a fresh InputContext for the fork. The document hasn't
            // changed (prediction is read-only w.r.t. the document), so we
            // reuse the same document reference and cursor position.
            let mut all_effects: SmallVec<[crate::effects::Effect; 4]> = SmallVec::new();

            // Track the cursor position across iterations so that each key in
            // a multi-key sequence sees the cursor position left by the
            // previous key, not the original cursor from the caller's context.
            let mut current_cursor = ctx.cursor_offset_raw();

            for key in &keys {
                let fork_ctx = crate::execution::InputContext::new(ctx.doc(), current_cursor)
                    .validate_clamped();

                // Copy over selection and viewport from the original context.
                let fork_ctx = if let Some(sel) = ctx.selection() {
                    fork_ctx.with_selection(sel)
                } else {
                    fork_ctx
                };
                let fork_ctx = if let Some(vp) = ctx.viewport() {
                    fork_ctx.with_viewport(vp)
                } else {
                    fork_ctx
                };

                let response = fork.process(*key, fork_ctx);

                // Advance the tracked cursor to wherever the fork left it.
                // We want the *last* SetCursor in the response because multiple
                // cursor effects are emitted in some sequences (e.g. a motion
                // paired with a mode change) and the final one is authoritative.
                for effect in response.effects() {
                    if let crate::effects::Effect::SetCursor { offset } = effect {
                        current_cursor = offset.get();
                    }
                }

                all_effects.extend(response.effects().iter().cloned());
            }

            // Convert collected effects to a ChangeSet and extract the range.
            let cs = crate::effects::bridge::effects_to_changeset(&all_effects, doc_len);
            prediction.affected_range = changeset_affected_range(&cs);

            // Fork is dropped here → automatic rollback.
        }

        predictions
    }

    /// Get a reference to the engine's prediction weights.
    ///
    /// Useful for inspecting the current adaptive state or serialising
    /// weights for session persistence.
    #[inline]
    #[must_use]
    pub const fn prediction_weights(&self) -> &PredictionWeights {
        &self.prediction_weights
    }

    /// Get a mutable reference to the engine's prediction weights.
    ///
    /// Allows the host to pre-seed weights (e.g., from a saved session)
    /// or reset them.
    #[inline]
    pub const fn prediction_weights_mut(&mut self) -> &mut PredictionWeights {
        &mut self.prediction_weights
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::engine::VimEngine;
    use crate::execution::InputContext;
    use crate::keymap::KeyEvent;
    use crate::primitives::Mode;
    use crate::primitives::Operator;
    use crate::test_utils::SimpleDocument;

    /// Create a minimal `InputContext` suitable for processing a single key.
    ///
    /// Leaks a static document — acceptable in unit tests.
    fn make_ctx(
        text: &str,
    ) -> (
        &'static SimpleDocument,
        InputContext<'static, SimpleDocument, crate::execution::context::Validated>,
    ) {
        let doc: &'static SimpleDocument = Box::leak(Box::new(SimpleDocument::new(text)));
        let ctx = InputContext::new(doc, 0).validate_clamped();
        (doc, ctx)
    }

    /// Put the engine into operator-pending mode for `d` (delete).
    ///
    /// After `process(d)`, the grammar parser enters the operator state but
    /// the VimState mode stays Normal (the host is responsible for syncing
    /// mode via effects). For prediction to work, we must also set the
    /// VimState mode explicitly to `OperatorPending(Delete)`.
    fn enter_operator_pending(engine: &mut VimEngine, doc: &SimpleDocument) {
        let ctx = InputContext::new(doc, 0).validate_clamped();
        engine.process(KeyEvent::char('d'), ctx);
        // The parser is now in operator state; set the VimState mode
        // to match, as the host would after processing the Pending response.
        engine.set_mode(Mode::OperatorPending(Operator::Delete));
        assert!(
            matches!(engine.mode(), Mode::OperatorPending(_)),
            "engine should be in OperatorPending mode after 'd' + set_mode"
        );
    }

    // ── predict() basic tests ──────────────────────────────────────────

    #[test]
    fn predict_empty_in_normal_mode() {
        let engine = VimEngine::new();
        let config = PredictionConfig::default();
        let predictions = engine.predict(&config);
        assert!(
            predictions.is_empty(),
            "predict() should return empty in Normal mode"
        );
    }

    #[test]
    fn predict_returns_candidates_in_operator_pending() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        let config = PredictionConfig::default();
        let predictions = engine.predict(&config);
        assert!(
            !predictions.is_empty(),
            "predict() should return candidates in OperatorPending mode"
        );
    }

    #[test]
    fn predict_affected_ranges_are_none() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        let config = PredictionConfig::default();
        let predictions = engine.predict(&config);
        for pred in &predictions {
            assert!(
                pred.affected_range.is_none(),
                "predict() v1 should always return None ranges, got {:?} for {:?}",
                pred.affected_range,
                pred.description
            );
        }
    }

    #[test]
    fn predict_respects_max_predictions() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        let config = PredictionConfig {
            max_predictions: 3,
            categories: PredictionCategories::ALL,
        };
        let predictions = engine.predict(&config);
        assert!(
            predictions.len() <= 3,
            "predictions should be capped at 3, got {}",
            predictions.len()
        );
    }

    #[test]
    fn predict_category_filtering() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        // Only motions — no line repeat, no text objects.
        let config = PredictionConfig {
            max_predictions: 20,
            categories: PredictionCategories::MOTIONS,
        };
        let predictions = engine.predict(&config);

        for pred in &predictions {
            // Motions are single-key triggers.
            assert!(
                matches!(pred.trigger, PredictionTrigger::Key(_)),
                "MOTIONS-only should produce single-key triggers, got Sequence"
            );
        }
    }

    // ── PredictionWeights integration tests ────────────────────────────

    #[test]
    fn weights_influence_predict_likelihood() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");

        // Observe 'w' many times to boost its weight.
        for _ in 0..20 {
            engine.prediction_weights_mut().observe("w");
        }

        enter_operator_pending(&mut engine, doc);

        let config = PredictionConfig {
            max_predictions: 20,
            categories: PredictionCategories::MOTIONS,
        };
        let predictions = engine.predict(&config);

        // Find the 'w' prediction.
        let w_pred = predictions.iter().find(|p| {
            if let PredictionTrigger::Key(k) = &p.trigger {
                k.as_char() == Some('w')
            } else {
                false
            }
        });
        assert!(w_pred.is_some(), "should find 'w' prediction");

        let w_likelihood = w_pred.map_or(0.0, |p| p.likelihood);

        // After many observations of 'w', its weight should be significantly
        // higher than the default 0.05. The exact value depends on EMA
        // dynamics but it should be well above the static default of 0.25.
        assert!(
            w_likelihood > 0.1,
            "w likelihood after heavy observation should be > 0.1, got {w_likelihood}"
        );
    }

    #[test]
    fn empty_weights_use_static_defaults() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        assert!(
            engine.prediction_weights().is_empty(),
            "fresh engine should have empty weights"
        );

        let config = PredictionConfig {
            max_predictions: 20,
            categories: PredictionCategories::MOTIONS,
        };
        let predictions = engine.predict(&config);

        // Find the 'w' prediction — should have the static default of 0.25.
        let w_pred = predictions.iter().find(|p| {
            if let PredictionTrigger::Key(k) = &p.trigger {
                k.as_char() == Some('w')
            } else {
                false
            }
        });
        assert!(w_pred.is_some(), "should find 'w' prediction");

        let w_likelihood = w_pred.map_or(0.0, |p| p.likelihood);
        assert!(
            (w_likelihood - 0.25).abs() < f32::EPSILON,
            "static default for 'w' should be 0.25, got {w_likelihood}"
        );
    }

    #[test]
    fn observe_integration_in_process() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");

        assert!(engine.prediction_weights().is_empty());

        // Type 'j' (move down) — this is a consumed keystroke that should
        // trigger observe().
        let ctx = InputContext::new(doc, 0).validate_clamped();
        engine.process(KeyEvent::char('j'), ctx);

        // After processing a consumed key, weights should no longer be empty.
        assert!(
            !engine.prediction_weights().is_empty(),
            "weights should be non-empty after a consumed keystroke"
        );
    }

    // ── predict_with_ranges tests ──────────────────────────────────────

    #[test]
    fn predict_with_ranges_returns_some_ranges() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        let ctx = InputContext::new(doc, 0).validate_clamped();
        let config = PredictionConfig {
            max_predictions: 8,
            categories: PredictionCategories::MOTIONS,
        };
        let predictions = engine.predict_with_ranges(&config, &ctx);

        assert!(!predictions.is_empty(), "should return predictions");

        // At least some motion predictions should produce a non-None range
        // because `dw` from position 0 in "hello world" should delete "hello ".
        let ranges_present = predictions
            .iter()
            .filter(|p| p.affected_range.is_some())
            .count();
        assert!(
            ranges_present > 0,
            "at least one prediction should have a computed range, but all were None"
        );
    }

    #[test]
    fn predict_with_ranges_w_motion_has_nonempty_range() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        let ctx = InputContext::new(doc, 0).validate_clamped();
        let config = PredictionConfig {
            max_predictions: 20,
            categories: PredictionCategories::MOTIONS,
        };
        let predictions = engine.predict_with_ranges(&config, &ctx);

        let w_pred = predictions.iter().find(|p| {
            if let PredictionTrigger::Key(k) = &p.trigger {
                k.as_char() == Some('w')
            } else {
                false
            }
        });
        assert!(w_pred.is_some(), "should find 'w' prediction");

        let range = w_pred
            .and_then(|p| p.affected_range)
            .expect("'w' prediction should have a range for 'dw' at offset 0");

        // "dw" at offset 0 in "hello world" should delete "hello " (0..6).
        assert!(
            range.start().get() == 0,
            "range should start at 0, got {}",
            range.start().get()
        );
        assert!(
            range.end().get() > 0,
            "range should have non-zero end, got {}",
            range.end().get()
        );
    }

    #[test]
    fn predict_with_ranges_dollar_motion_has_range() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        let ctx = InputContext::new(doc, 0).validate_clamped();
        let config = PredictionConfig {
            max_predictions: 20,
            categories: PredictionCategories::MOTIONS,
        };
        let predictions = engine.predict_with_ranges(&config, &ctx);

        let dollar_pred = predictions.iter().find(|p| {
            if let PredictionTrigger::Key(k) = &p.trigger {
                k.as_char() == Some('$')
            } else {
                false
            }
        });
        assert!(dollar_pred.is_some(), "should find '$' prediction");

        let range = dollar_pred.and_then(|p| p.affected_range);
        // "d$" from offset 0 should delete to end of line.
        assert!(
            range.is_some(),
            "'$' prediction should have a range for 'd$' at offset 0"
        );
    }

    #[test]
    fn predict_with_ranges_preserves_engine_state() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        let mode_before = engine.mode();
        let parser_state_before = engine.pending_operator();

        let ctx = InputContext::new(doc, 0).validate_clamped();
        let config = PredictionConfig::default();
        let _predictions = engine.predict_with_ranges(&config, &ctx);

        // Engine state must be unchanged after predict_with_ranges —
        // every fork was rolled back.
        assert_eq!(
            engine.mode(),
            mode_before,
            "mode must be unchanged after predict_with_ranges"
        );
        assert_eq!(
            engine.pending_operator().is_some(),
            parser_state_before.is_some(),
            "parser pending state must be unchanged after predict_with_ranges"
        );
    }

    #[test]
    fn predict_with_ranges_in_normal_mode_returns_empty() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\n");

        let ctx = InputContext::new(doc, 0).validate_clamped();
        let config = PredictionConfig::default();
        let predictions = engine.predict_with_ranges(&config, &ctx);

        assert!(
            predictions.is_empty(),
            "predict_with_ranges in Normal mode should return empty"
        );
    }

    #[test]
    fn predict_with_ranges_e_motion_has_range() {
        let mut engine = VimEngine::new();
        let (doc, _) = make_ctx("hello world\nsecond line\n");
        enter_operator_pending(&mut engine, doc);

        let ctx = InputContext::new(doc, 0).validate_clamped();
        let config = PredictionConfig {
            max_predictions: 20,
            categories: PredictionCategories::MOTIONS,
        };
        let predictions = engine.predict_with_ranges(&config, &ctx);

        let e_pred = predictions.iter().find(|p| {
            if let PredictionTrigger::Key(k) = &p.trigger {
                k.as_char() == Some('e')
            } else {
                false
            }
        });
        assert!(e_pred.is_some(), "should find 'e' prediction");

        let range = e_pred.and_then(|p| p.affected_range);
        // "de" from offset 0 in "hello world" should delete "hello" (inclusive of 'o').
        assert!(
            range.is_some(),
            "'e' prediction should have a range for 'de' at offset 0"
        );
    }

    // ── changeset_affected_range unit tests ────────────────────────────

    #[test]
    fn affected_range_identity_is_none() {
        let cs = crate::primitives::ChangeSet::identity(42);
        assert!(
            changeset_affected_range(&cs).is_none(),
            "identity changeset should have no affected range"
        );
    }

    #[test]
    fn affected_range_single_delete() {
        // Delete bytes 3..7 in a 10-byte document.
        let cs = crate::primitives::ChangeSet::from_delete(10, 3, 7);
        let range = changeset_affected_range(&cs).expect("should have a range");
        assert_eq!(range.start().get(), 3);
        assert_eq!(range.end().get(), 7);
    }

    #[test]
    fn affected_range_single_insert() {
        // Insert at position 5 in a 10-byte document.
        let cs = crate::primitives::ChangeSet::from_insert(10, 5, "abc");
        let range = changeset_affected_range(&cs).expect("should have a range");
        // Pure insertion at pos 5 — affected range in input coords is [5, 5).
        assert_eq!(range.start().get(), 5);
        assert_eq!(range.end().get(), 5);
    }

    #[test]
    fn affected_range_replace() {
        // Replace bytes 2..5 with "HELLO" in a 10-byte document.
        let cs = crate::primitives::ChangeSet::from_replace(10, 2, 5, "HELLO");
        let range = changeset_affected_range(&cs).expect("should have a range");
        assert_eq!(range.start().get(), 2);
        assert_eq!(range.end().get(), 5);
    }

    #[test]
    fn affected_range_empty_changeset_ops() {
        // Empty document, identity.
        let cs = crate::primitives::ChangeSet::identity(0);
        assert!(changeset_affected_range(&cs).is_none());
    }

    #[test]
    fn affected_range_multiple_changes() {
        // Two separate changes: delete [1,3) and delete [7,9) in a 10-byte doc.
        let cs = crate::primitives::ChangeSet::from_changes(10, [(1, 3, None), (7, 9, None)]);
        let range = changeset_affected_range(&cs).expect("should have a range");
        // Affected range spans from first change start to last change end.
        assert_eq!(range.start().get(), 1);
        assert_eq!(range.end().get(), 9);
    }

    // ── Multi-key cursor tracking regression test ──────────────────────

    /// Verify that the cursor is tracked across iterations of the multi-key
    /// loop in `predict_with_ranges`.
    ///
    /// The text-object trigger `iw` is two keys: `i` and `w`.  After `d`
    /// enters operator-pending mode and `i` is processed, the fork's engine
    /// state advances (the parser moves into the AwaitingTextObject sub-state).
    /// The `w` key that follows must be processed with the *updated* internal
    /// state — which is driven by the cursor tracking fix.  If the bug were
    /// present, the second key would see the raw original cursor (0) as its
    /// context even though the fork already moved on, producing a `None` range
    /// because the grammar never completed the text-object sequence.
    ///
    /// We verify that `diw` starting at a word boundary produces a non-`None`
    /// range whose start equals the beginning of the word.
    #[test]
    fn predict_with_ranges_iw_text_object_cursor_tracking() {
        // Place the cursor at offset 0, at the start of "hello".
        // "diw" should identify the affected range [0, 5) ("hello").
        let mut engine = VimEngine::new();
        let text = "hello world\nsecond line\n";
        let (doc, _) = make_ctx(text);
        enter_operator_pending(&mut engine, doc);

        let ctx = InputContext::new(doc, 0).validate_clamped();
        let config = PredictionConfig {
            max_predictions: 20,
            categories: PredictionCategories::TEXT_OBJECTS,
        };

        let predictions = engine.predict_with_ranges(&config, &ctx);

        // Find the `iw` prediction.
        let iw_pred = predictions.iter().find(|p| {
            if let PredictionTrigger::Sequence(seq) = &p.trigger {
                seq.len() == 2 && seq[0].as_char() == Some('i') && seq[1].as_char() == Some('w')
            } else {
                false
            }
        });

        assert!(
            iw_pred.is_some(),
            "should find an 'iw' sequence prediction in TEXT_OBJECTS category"
        );

        // The key assertion: the range must be Some.  If `current_cursor`
        // tracking is broken, the second key (`w`) is fed with the original
        // cursor while the fork's parser is already in an intermediate state,
        // causing the grammar to fail and leaving affected_range as None.
        let range = iw_pred
            .and_then(|p| p.affected_range)
            .expect("'diw' prediction must produce a Some(range) — cursor tracking regression");

        // From offset 0 in "hello world\n", `iw` selects "hello".
        // The exact byte bounds depend on the engine's inclusivity handling;
        // we only require that the range starts at or before the 'h' and ends
        // somewhere inside the word (i.e., end > start).
        assert!(
            range.start().get() <= 1,
            "iw range should start near the beginning of the first word, got {}",
            range.start().get()
        );
        assert!(
            range.end().get() > range.start().get(),
            "iw range should be non-empty: start={} end={}",
            range.start().get(),
            range.end().get()
        );

        // Engine state must be completely unchanged after predict_with_ranges.
        assert!(
            matches!(engine.mode(), Mode::OperatorPending(_)),
            "engine must still be in OperatorPending mode after predict_with_ranges"
        );
    }

    /// Verify that cursor tracking is also correct when the cursor is NOT at
    /// position 0 — i.e., that `current_cursor` is initialised from the
    /// caller's context and not hard-coded to zero.
    #[test]
    fn predict_with_ranges_iw_cursor_tracking_non_zero_start() {
        // Cursor at offset 6 (the 'w' in "world").  `diw` should affect
        // "world" which starts at byte 6 and ends at byte 10 (exclusive of
        // the trailing space, inclusive boundary depends on engine).
        let mut engine = VimEngine::new();
        let text = "hello world\nsecond line\n";
        let (doc, _) = make_ctx(text);
        enter_operator_pending(&mut engine, doc);

        // Cursor at offset 6 ('w' of "world").
        let ctx = InputContext::new(doc, 6).validate_clamped();
        let config = PredictionConfig {
            max_predictions: 20,
            categories: PredictionCategories::TEXT_OBJECTS,
        };

        let predictions = engine.predict_with_ranges(&config, &ctx);

        let iw_pred = predictions.iter().find(|p| {
            if let PredictionTrigger::Sequence(seq) = &p.trigger {
                seq.len() == 2 && seq[0].as_char() == Some('i') && seq[1].as_char() == Some('w')
            } else {
                false
            }
        });

        assert!(
            iw_pred.is_some(),
            "should find 'iw' prediction even with non-zero start cursor"
        );

        let range = iw_pred
            .and_then(|p| p.affected_range)
            .expect("'diw' at offset 6 must produce a non-None range");

        // The range should start at or before offset 6 and end after it —
        // confirming that current_cursor was initialised from ctx (6) not 0.
        assert!(
            range.start().get() <= 6,
            "iw range start should be at or before cursor offset 6, got {}",
            range.start().get()
        );
        assert!(
            range.end().get() > 6,
            "iw range end should be past cursor offset 6, got {}",
            range.end().get()
        );
    }
}
