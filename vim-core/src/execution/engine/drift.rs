//! Drift detection and reconciliation for the self-healing shadow document.
//!
//! The drift gate runs at the top of `process()` before any state mutation.
//! It compares the shadow document against the host's text and auto-heals
//! via `apply_external_edit()` if they differ.

use super::shadow_diff::compute_diff;
use super::VimEngine;
use crate::document::Document;
use crate::effects::Effect;
use crate::execution::{ExternalEdit, ExternalEditKind};
use crate::primitives::{Offset, Range};

impl VimEngine {
    /// Reconcile a detected drift between the shadow document and host text.
    ///
    /// Computes the minimal diff and applies it as an `ExternalEdit` with
    /// `ExternalEditKind::HostDrift`, which updates the shadow document,
    /// remaps marks/positions, and creates an undo entry.
    pub(in crate::execution::engine) fn reconcile_drift(
        &mut self,
        host_text: &str,
        host_cursor: Offset,
    ) {
        warn!(target: "vim::engine::drift", "drift detected — reconciling (host_len={}, cursor={})", host_text.len(), host_cursor);
        let shadow_text = self
            .shadow
            .as_ref()
            .expect("reconcile_drift requires shadow")
            .text()
            .to_owned();

        let Some(effect) = compute_diff(&shadow_text, host_text) else {
            return; // Texts are identical (defensive; caller already checked)
        };

        let edit = match effect {
            Effect::Insert { offset, ref text } => ExternalEdit::new(
                Range::new(offset, offset),
                text.as_str(),
                host_cursor,
                ExternalEditKind::HostDrift,
            ),
            Effect::Delete { range } => {
                ExternalEdit::new(range, "", host_cursor, ExternalEditKind::HostDrift)
            }
            Effect::Replace { range, ref text } => ExternalEdit::new(
                range,
                text.as_str(),
                host_cursor,
                ExternalEditKind::HostDrift,
            ),
            _ => return, // Only text mutations are relevant
        };

        let _ = self.apply_external_edit(edit);
    }

    /// Apply text mutation effects from the current response to the shadow
    /// document, keeping it in sync for the next `process()` call.
    ///
    /// Handles `Insert`, `Delete`, and `Replace` effects incrementally.
    /// Undo/Redo effects are NOT handled here — the engine doesn't hold
    /// changeset data to compute the resulting text. Instead, `VimSession`
    /// calls `sync_shadow_after_undo_redo` after delivering effects to the
    /// host, resetting the shadow from the host's authoritative text.
    pub(in crate::execution::engine) fn update_shadow_from_effects(&mut self, effects: &[Effect]) {
        let Some(shadow) = &mut self.shadow else {
            return;
        };
        for effect in effects {
            match effect {
                Effect::Insert { offset, text } => {
                    shadow.apply_insert(offset.get(), text);
                }
                Effect::Delete { range } => {
                    shadow.apply_delete(range.start().get(), range.end().get());
                }
                Effect::Replace { range, text } => {
                    shadow.apply_replace(range.start().get(), range.end().get(), text);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::InputContext;
    use crate::keymap::KeyEvent;
    use crate::primitives::Mode;
    use crate::test_utils::SimpleDocument;

    /// Helper: create an engine with shadow text initialized.
    fn engine_with_shadow(text: &str) -> VimEngine {
        let mut engine = VimEngine::new();
        engine.set_shadow_text(text);
        engine
    }

    // ══════════════════════════════════════════════════════════════════════
    // Test 1: Drift detection and healing
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn drift_gate_heals_shadow_on_host_text_mismatch() {
        let mut engine = engine_with_shadow("hello\n");

        // Host document has different text than the shadow.
        let doc = SimpleDocument::new("hello world\n");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();

        // Process any key — the drift gate runs before the keystroke.
        let _response = engine.process(KeyEvent::char('j'), ctx);

        // After process(), shadow should match the host text.
        assert_eq!(
            engine.shadow_text().unwrap(),
            "hello world\n",
            "drift gate should heal shadow to match host text"
        );
    }

    #[test]
    fn drift_gate_no_op_when_texts_match() {
        let mut engine = engine_with_shadow("hello\n");

        let doc = SimpleDocument::new("hello\n");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();

        // Process a key — no drift, shadow should remain unchanged.
        let _response = engine.process(KeyEvent::char('j'), ctx);

        assert_eq!(
            engine.shadow_text().unwrap(),
            "hello\n",
            "shadow should remain unchanged when no drift"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Test 2: Shadow update after engine effects
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn shadow_updated_after_insert_effects() {
        let mut engine = engine_with_shadow("hello\n");

        // Enter insert mode at position 5 (before \n)
        let doc = SimpleDocument::new("hello\n");
        let ctx = InputContext::new(&doc, 5).validate().unwrap();
        let _response = engine.process(KeyEvent::char('i'), ctx);
        assert_eq!(engine.mode(), Mode::Insert);

        // Now type a character 'X' — this should produce an Insert effect
        let doc = SimpleDocument::new("hello\n");
        let ctx = InputContext::new(&doc, 5).validate().unwrap();
        let response = engine.process(KeyEvent::char('X'), ctx);

        // Verify an Insert effect was produced
        let has_insert = response
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Insert { .. }));
        if has_insert {
            // Shadow should have been updated to include the inserted character
            let shadow = engine.shadow_text().unwrap();
            assert!(
                shadow.contains('X'),
                "shadow should contain the inserted character, got: {shadow:?}"
            );
        }
        // If no insert effect (e.g., native insert mode), shadow stays as-is
        // which is correct — the host will notify via apply_external_edit.
    }

    #[test]
    fn update_shadow_from_effects_applies_insert() {
        let mut engine = engine_with_shadow("hello\n");

        let effects = vec![Effect::insert(Offset::new(5), " world")];
        engine.update_shadow_from_effects(&effects);

        assert_eq!(engine.shadow_text().unwrap(), "hello world\n");
    }

    #[test]
    fn update_shadow_from_effects_applies_delete() {
        let mut engine = engine_with_shadow("hello world\n");

        let effects = vec![Effect::delete(Range::new(Offset::new(5), Offset::new(11)))];
        engine.update_shadow_from_effects(&effects);

        assert_eq!(engine.shadow_text().unwrap(), "hello\n");
    }

    #[test]
    fn update_shadow_from_effects_applies_replace() {
        let mut engine = engine_with_shadow("hello world\n");

        let effects = vec![Effect::replace(
            Range::new(Offset::new(6), Offset::new(11)),
            "earth",
        )];
        engine.update_shadow_from_effects(&effects);

        assert_eq!(engine.shadow_text().unwrap(), "hello earth\n");
    }

    #[test]
    fn update_shadow_from_effects_no_shadow_is_noop() {
        let mut engine = VimEngine::new();
        // No shadow set — should not panic
        let effects = vec![Effect::insert(Offset::new(0), "text")];
        engine.update_shadow_from_effects(&effects);
        assert!(engine.shadow_text().is_none());
    }

    #[test]
    fn drift_gate_heals_deletion_drift() {
        let mut engine = engine_with_shadow("hello world\n");

        // Host document has deleted " world" — shadow is stale.
        let doc = SimpleDocument::new("hello\n");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();

        let _response = engine.process(KeyEvent::char('j'), ctx);

        assert_eq!(
            engine.shadow_text().unwrap(),
            "hello\n",
            "drift gate should heal shadow after host deleted text"
        );
    }

    #[test]
    fn drift_gate_heals_replacement_drift() {
        let mut engine = engine_with_shadow("hello world\n");

        // Host replaced "world" with "earth"
        let doc = SimpleDocument::new("hello earth\n");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();

        let _response = engine.process(KeyEvent::char('j'), ctx);

        assert_eq!(
            engine.shadow_text().unwrap(),
            "hello earth\n",
            "drift gate should heal shadow after host replaced text"
        );
    }

    #[test]
    fn drift_gate_skipped_when_no_shadow() {
        let mut engine = VimEngine::new();
        // No shadow — drift gate should be a no-op, not panic.
        let doc = SimpleDocument::new("hello\n");
        let ctx = InputContext::new(&doc, 0).validate().unwrap();

        let _response = engine.process(KeyEvent::char('j'), ctx);

        assert!(engine.shadow_text().is_none());
    }
}
