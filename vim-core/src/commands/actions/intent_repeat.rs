//! Intent-aware repeat (`g.`).
//!
//! Replays the semantic intent of the last repeatable command rather than
//! the exact keystroke sequence. This allows the operator to be re-applied
//! with motions/text-objects resolved at the *current* cursor position.
//!
//! # Current Status
//!
//! This is a partial (stub) implementation. The full version requires wiring
//! through the executor to re-resolve motions and apply operators, which
//! depends on having document and cursor context at replay time.
//!
//! The stub reads the stored `CommandIntent` and produces a `StatusMessage`
//! describing what it would do. The full implementation will follow the
//! same pattern as dot-repeat but read from `last_intent` instead of
//! `last_command`.

use crate::effects::Effects;
use crate::state::CommandIntent;

/// Execute intent-aware repeat (`g.`).
///
/// Reads the last `CommandIntent` from repeat state. If present, produces
/// a status message describing the intent. If absent, produces an error.
///
/// # Future
///
/// The full implementation will re-resolve the intent's motion/text-object
/// at the current cursor position and re-apply the operator through the
/// executor pipeline.
pub fn execute_intent_repeat(last_intent: Option<&CommandIntent>) -> Effects {
    match last_intent {
        Some(_intent) => {
            Effects::new().show_message("E10: g. replay not yet implemented".to_owned())
        }
        None => Effects::new().show_message("E10: No intent recorded"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::grammar::types::{Motion, Operator};
    use std::num::NonZeroU32;

    #[test]
    fn intent_repeat_with_no_intent() {
        let effects = execute_intent_repeat(None);
        let msgs: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::ShowInfo {
                    info: crate::effects::InfoMessage::Text(text),
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(msgs.iter().any(|m| m.contains("No intent")));
    }

    #[test]
    fn intent_repeat_with_operator_motion() {
        let intent = CommandIntent::OperatorMotion {
            operator: Operator::Delete,
            motion: Motion::WordForward,
            count: Some(NonZeroU32::MIN),
            inserted_text: None,
            register: None,
        };
        let effects = execute_intent_repeat(Some(&intent));
        let msgs: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::ShowInfo {
                    info: crate::effects::InfoMessage::Text(text),
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(msgs.iter().any(|m| m.contains("E10")));
    }

    #[test]
    fn intent_repeat_with_action() {
        let intent = CommandIntent::Action {
            action: crate::grammar::types::Action::DeleteChar,
            count: Some(NonZeroU32::MIN),
        };
        let effects = execute_intent_repeat(Some(&intent));
        let has_msg = effects.iter().any(|e| matches!(e, Effect::ShowInfo { .. }));
        assert!(has_msg);
    }
}
