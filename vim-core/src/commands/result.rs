//! Unified command result type.
//!
//! All command modules (actions, operators, insert, visual, ex) converge on
//! `CommandResult` to eliminate the proliferation of near-identical result structs.
//!
//! # Design
//!
//! ```text
//! CommandResult { effects: Effects, cursor: Option<Offset> }
//! ```
//!
//! - `cursor: Some(offset)` — actions and operators that compute a new cursor position
//! - `cursor: None` — insert/visual/ex commands where cursor is set via `Effect::SetCursor`

use crate::effects::undo_state::EffectState;
use crate::effects::{Effect, Effects};
use crate::primitives::Offset;
use smallvec::SmallVec;

/// Unified result for all commands that produce effects.
///
/// Unified result type for all command operations.
#[derive(Debug, Clone, Default)]
#[must_use = "CommandResult contains effects that must be applied"]
pub struct CommandResult {
    /// Effects to apply.
    pub effects: Effects,

    /// New cursor position (if the command sets it via return value).
    ///
    /// - `Some(offset)` — actions, operators (cursor computed by command)
    /// - `None` — insert, visual, ex (cursor set via `Effect::SetCursor`)
    pub cursor: Option<Offset>,
}

impl CommandResult {
    /// Create a result with effects and a cursor position.
    ///
    /// Accepts `Effects<S>` for any undo state — the typestate is erased
    /// at this boundary since the effect processor handles raw effects.
    #[inline]
    pub fn new<S: EffectState>(effects: Effects<S>, cursor: Offset) -> Self {
        Self {
            effects: effects.into_raw_closed(),
            cursor: Some(cursor),
        }
    }

    /// Create a result with effects only (cursor handled via effects).
    ///
    /// Accepts `Effects<S>` for any undo state — the typestate is erased
    /// at this boundary since the effect processor handles raw effects.
    #[inline]
    pub fn effects_only<S: EffectState>(effects: Effects<S>) -> Self {
        Self {
            effects: effects.into_raw_closed(),
            cursor: None,
        }
    }

    /// Create an empty result with just a cursor position (no effects).
    #[inline]
    pub fn empty(cursor: Offset) -> Self {
        Self {
            effects: Effects::new(),
            cursor: Some(cursor),
        }
    }

    /// Create a completely empty result (no effects, no cursor).
    #[inline]
    pub fn none() -> Self {
        Self::default()
    }

    /// Create from a `SmallVec` of effects (convenience for insert/visual commands).
    ///
    /// Equivalent to `effects_only(effects.into())`.
    #[inline]
    pub fn from_effects_vec(effects: SmallVec<[Effect; 8]>) -> Self {
        Self {
            effects: Effects::from(effects),
            cursor: None,
        }
    }

    /// Check if this result is a no-op (no effects).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}
