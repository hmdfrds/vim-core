//! Repeat state for `g.` (re-indent) intent tracking.
//!
//! Stores the semantic intent of the last repeatable command so that `g.`
//! can replay the appropriate action. Real dot-repeat (`.`) uses
//! `Parser.last_command` directly; this module is solely for intent storage.
//!
//! # Layering
//!
//! State is a low-mid layer: pure data containers, no execution logic.
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution`, `dispatch` or `grammar`.

use crate::state::CommandIntent;

/// State for tracking the semantic intent of the last repeatable command.
///
/// Used exclusively by `g.` to replay an intent-based action. The `.`
/// dot-repeat command itself uses `Parser.last_command`, not this struct.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RepeatState {
    /// The semantic intent of the last repeatable command.
    last_intent: Option<CommandIntent>,
}

impl RepeatState {
    /// Create new empty repeat state.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the semantic intent of the last repeatable command, if any.
    #[inline]
    #[must_use]
    pub const fn last_intent(&self) -> Option<&CommandIntent> {
        self.last_intent.as_ref()
    }

    /// Save the semantic intent of a repeatable command.
    #[inline]
    pub fn save_intent(&mut self, intent: CommandIntent) {
        self.last_intent = Some(intent);
    }

    /// Clear the repeat state.
    #[inline]
    pub fn clear(&mut self) {
        self.last_intent = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::types::{Action, Motion, Operator};
    use crate::state::CommandIntent;
    use std::num::NonZeroU32;

    fn sample_intent() -> CommandIntent {
        CommandIntent::OperatorMotion {
            operator: Operator::Delete,
            motion: Motion::WordForward,
            count: None,
            inserted_text: None,
            register: None,
        }
    }

    #[test]
    fn test_new_has_no_intent() {
        let state = RepeatState::new();
        assert!(state.last_intent().is_none());
    }

    #[test]
    fn test_save_and_retrieve_intent() {
        let mut state = RepeatState::new();
        state.save_intent(sample_intent());
        assert!(state.last_intent().is_some());
    }

    #[test]
    fn test_clear_resets_intent() {
        let mut state = RepeatState::new();
        state.save_intent(sample_intent());
        state.clear();
        assert!(state.last_intent().is_none());
    }

    #[test]
    fn test_save_overwrites_previous_intent() {
        let mut state = RepeatState::new();
        state.save_intent(sample_intent());

        let second = CommandIntent::Action {
            action: Action::DeleteChar,
            count: Some(NonZeroU32::new(3).unwrap()),
        };
        state.save_intent(second.clone());
        assert_eq!(state.last_intent(), Some(&second));
    }
}
