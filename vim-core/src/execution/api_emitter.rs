//! [`EffectEmitter`] — append-only effect collector for universal API operations.
//!
//! Uses `Cell<Vec<Effect>>` for zero-cost interior mutability: the caller
//! takes the vec, pushes, then puts it back.  No `RefCell` overhead.

use std::cell::Cell;

use compact_str::CompactString;

use crate::effects::Effect;
use crate::effects::InfoMessage;
use crate::errors::VimError;
use crate::execution::api_error::ApiError;
use crate::primitives::{
    CapabilityTier, MarkName, Mode, ModeAppearance, MotionType, Offset, Range, RegisterName,
    SelectionShape, UndoCursorStrategy, VarScope, VimValue,
};

/// Append-only effect collector for universal API operations.
///
/// `EffectEmitter` borrows a shared `Cell<Vec<Effect>>` and appends effects
/// produced by API calls.  Tier-gated methods return [`ApiError::InsufficientTier`]
/// when the caller's [`CapabilityTier`] is too low.
pub struct EffectEmitter<'a> {
    /// Shared effect accumulator.
    pub(crate) effects: &'a Cell<Vec<Effect>>,
    /// The caller's capability tier.
    pub(crate) tier: CapabilityTier,
}

impl EffectEmitter<'_> {
    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Return `Err` if the caller is not at least `Mutating`.
    fn require_mutating(&self) -> Result<(), ApiError> {
        if self.tier == CapabilityTier::ReadOnly {
            return Err(ApiError::InsufficientTier {
                required: CapabilityTier::Mutating,
                have: CapabilityTier::ReadOnly,
            });
        }
        Ok(())
    }

    /// Append a single effect to the shared accumulator.
    fn push(&self, effect: Effect) {
        let mut v = self.effects.take();
        v.push(effect);
        self.effects.set(v);
    }

    // ------------------------------------------------------------------
    // Mutating-tier methods
    // ------------------------------------------------------------------

    /// Emit an [`Effect::Insert`] at `offset`.
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`]; this is the only failure mode.
    pub fn insert(&self, offset: usize, text: &str) -> Result<(), ApiError> {
        self.require_mutating()?;
        self.push(Effect::Insert {
            offset: Offset::new(offset),
            text: CompactString::from(text),
        });
        Ok(())
    }

    /// Emit an [`Effect::Delete`] covering `start..end`.
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`]; this is the only failure mode.
    pub fn delete(&self, start: usize, end: usize) -> Result<(), ApiError> {
        self.require_mutating()?;
        self.push(Effect::Delete {
            range: Range::new(Offset::new(start), Offset::new(end)),
        });
        Ok(())
    }

    /// Emit an [`Effect::Replace`] covering `start..end` with `text`.
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`]; this is the only failure mode.
    pub fn replace(&self, start: usize, end: usize, text: &str) -> Result<(), ApiError> {
        self.require_mutating()?;
        self.push(Effect::Replace {
            range: Range::new(Offset::new(start), Offset::new(end)),
            text: CompactString::from(text),
        });
        Ok(())
    }

    /// Emit an [`Effect::SetMode`] with
    /// [`ModeAppearance::default()`](crate::primitives::ModeAppearance).
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`]; this is the only failure mode.
    pub fn set_mode(&self, mode: Mode) -> Result<(), ApiError> {
        self.require_mutating()?;
        self.push(Effect::SetMode {
            mode,
            appearance: ModeAppearance::for_mode(mode),
        });
        Ok(())
    }

    /// Emit an [`Effect::BeginUndoGroup`] with the default cursor strategy.
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`]; this is the only failure mode.
    pub fn begin_undo_group(&self) -> Result<(), ApiError> {
        self.require_mutating()?;
        self.push(Effect::BeginUndoGroup {
            cursor_strategy: UndoCursorStrategy::FirstEdit,
        });
        Ok(())
    }

    /// Emit an [`Effect::EndUndoGroup`] with `node_id: None`.
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`]; this is the only failure mode.
    pub fn end_undo_group(&self) -> Result<(), ApiError> {
        self.require_mutating()?;
        self.push(Effect::EndUndoGroup { node_id: None });
        Ok(())
    }

    /// Emit an [`Effect::SetRegister`].
    ///
    /// Returns [`ApiError::RegisterNotFound`] if `name` is not a valid register
    /// character.  Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`].
    ///
    /// Returns [`ApiError::RegisterNotFound`] carrying `name` if `name` is not a
    /// register character Vim recognises (see [`RegisterName::new`]) — for example
    /// `'!'`, or a non-ASCII character.
    pub fn set_register(
        &self,
        name: char,
        text: &str,
        motion_type: MotionType,
    ) -> Result<(), ApiError> {
        self.require_mutating()?;
        let reg = RegisterName::new(name).ok_or(ApiError::RegisterNotFound(name))?;
        self.push(Effect::SetRegister {
            name: reg,
            text: CompactString::from(text),
            motion_type,
        });
        Ok(())
    }

    /// Emit an [`Effect::SetMark`] with `topline_offset: None`.
    ///
    /// Returns [`ApiError::MarkNotFound`] if `name` is not a valid mark
    /// character.  Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`].
    ///
    /// Returns [`ApiError::MarkNotFound`] carrying `name` if `name` is not a mark
    /// character Vim recognises (see [`MarkName::new`]) — for example `'!'`, or a
    /// non-ASCII character.
    pub fn set_mark(&self, name: char, offset: usize) -> Result<(), ApiError> {
        self.require_mutating()?;
        let mark = MarkName::new(name).ok_or(ApiError::MarkNotFound(name))?;
        self.push(Effect::SetMark {
            name: mark,
            offset: Offset::new(offset),
            topline_offset: None,
        });
        Ok(())
    }

    // ------------------------------------------------------------------
    // ReadOnly-tier methods (always allowed)
    // ------------------------------------------------------------------

    /// Emit an [`Effect::SetCursor`].
    ///
    /// Allowed at [`CapabilityTier::ReadOnly`].
    ///
    /// # Errors
    ///
    /// Never returns an error. The `Result` exists so every emitter method has the
    /// same signature; cursor movement is permitted at every tier.
    pub fn set_cursor(&self, offset: usize) -> Result<(), ApiError> {
        self.push(Effect::SetCursor {
            offset: Offset::new(offset),
        });
        Ok(())
    }

    /// Emit an [`Effect::SetSelection`].
    ///
    /// Allowed at [`CapabilityTier::ReadOnly`].
    ///
    /// # Errors
    ///
    /// Never returns an error. The `Result` exists so every emitter method has the
    /// same signature; changing the selection is permitted at every tier.
    pub fn set_selection(
        &self,
        anchor: usize,
        head: usize,
        shape: SelectionShape,
    ) -> Result<(), ApiError> {
        self.push(Effect::SetSelection {
            anchor: Offset::new(anchor),
            head: Offset::new(head),
            shape,
        });
        Ok(())
    }

    /// Emit an [`Effect::ShowInfo`] with [`InfoMessage::Text`].
    ///
    /// Allowed at [`CapabilityTier::ReadOnly`].
    ///
    /// # Errors
    ///
    /// Never returns an error. The `Result` exists so every emitter method has the
    /// same signature; showing a message is permitted at every tier.
    pub fn message(&self, text: &str) -> Result<(), ApiError> {
        self.push(Effect::ShowInfo {
            info: InfoMessage::Text(text.into()),
        });
        Ok(())
    }

    /// Emit an [`Effect::ShowError`] with [`VimError::HostFailure`].
    ///
    /// Allowed at [`CapabilityTier::ReadOnly`].
    ///
    /// # Errors
    ///
    /// Never returns an error. The `Result` exists so every emitter method has the
    /// same signature; showing an error is permitted at every tier.
    pub fn error(&self, text: &str) -> Result<(), ApiError> {
        self.push(Effect::ShowError {
            error: VimError::HostFailure(text.into()),
            source: None,
        });
        Ok(())
    }

    // ------------------------------------------------------------------
    // Highlight range methods (ReadOnly tier)
    // ------------------------------------------------------------------

    /// Emit an [`Effect::SetHighlightRange`] for the given owner.
    ///
    /// Sets a persistent highlight over `start..end` in `group`.  Multiple
    /// groups can coexist within the same owner.
    ///
    /// Allowed at [`CapabilityTier::ReadOnly`].
    ///
    /// # Errors
    ///
    /// Never returns an error. The `Result` exists so every emitter method has the
    /// same signature; highlighting is permitted at every tier. `start`, `end` and
    /// `group` are not validated here — an out-of-range or inverted range is
    /// resolved when the effect is applied.
    pub fn set_highlight(&self, start: usize, end: usize, group: &str) -> Result<(), ApiError> {
        let owner = CompactString::from("__internal__");
        self.push(Effect::SetHighlightRange {
            owner,
            range: Range::new(Offset::new(start), Offset::new(end)),
            group: CompactString::from(group),
            shape: crate::primitives::SelectionShape::Char,
        });
        Ok(())
    }

    /// Emit an [`Effect::ClearHighlightRange`] for the given owner.
    ///
    /// Clears all highlight groups belonging to the caller.
    ///
    /// Allowed at [`CapabilityTier::ReadOnly`].
    ///
    /// # Errors
    ///
    /// Never returns an error. The `Result` exists so every emitter method has the
    /// same signature; clearing highlights is permitted at every tier, and
    /// clearing when nothing is highlighted is a no-op.
    pub fn clear_highlights(&self) -> Result<(), ApiError> {
        let owner = CompactString::from("__internal__");
        self.push(Effect::ClearHighlightRange { owner, group: None });
        Ok(())
    }

    // ------------------------------------------------------------------
    // Host action method (Mutating tier)
    // ------------------------------------------------------------------

    /// Emit an [`Effect::HostAction`] with the given `name`.
    ///
    /// Instructs the host to dispatch a named editor action (IdeaVim-style
    /// `<Action>()` bridge).
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`]; this is the only failure mode.
    pub fn host_action(&self, name: &str) -> Result<(), ApiError> {
        self.require_mutating()?;
        self.push(Effect::HostAction {
            name: CompactString::from(name),
        });
        Ok(())
    }

    // ------------------------------------------------------------------
    // Cross-buffer edit method (Mutating tier)
    // ------------------------------------------------------------------

    /// Emit a [`Effect::CrossBufferEdit`] targeting a specific buffer.
    ///
    /// The host is responsible for routing this effect to the target buffer
    /// and applying the edits atomically.
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`]. `target` is not checked here: a
    /// [`ApiError::BufferNotFound`]-style failure for an unknown buffer, if any,
    /// surfaces from the host when the effect is routed.
    pub fn cross_buffer_edit(
        &self,
        target: crate::primitives::BufferId,
        edits: &[crate::primitives::TextEdit],
    ) -> Result<(), ApiError> {
        self.require_mutating()?;
        self.push(Effect::CrossBufferEdit {
            target,
            edits: edits.iter().cloned().collect(),
        });
        Ok(())
    }

    // ------------------------------------------------------------------
    // Variable store methods (Mutating tier)
    // ------------------------------------------------------------------

    /// Emit an [`Effect::SetVariable`] in the given scope.
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`].
    ///
    /// Returns [`ApiError::VariableNotFound`] with the given `scope` and an empty
    /// name if `name` is empty; the empty string is not a legal variable name.
    pub fn set_variable(
        &self,
        scope: VarScope,
        name: &str,
        value: VimValue,
    ) -> Result<(), ApiError> {
        self.require_mutating()?;
        if name.is_empty() {
            return Err(ApiError::VariableNotFound {
                scope,
                name: CompactString::new_inline(""),
            });
        }
        self.push(Effect::SetVariable {
            scope,
            name: CompactString::from(name),
            value,
        });
        Ok(())
    }

    /// Emit an [`Effect::DeleteVariable`] in the given scope.
    ///
    /// Requires [`CapabilityTier::Mutating`].
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InsufficientTier`] if the caller's tier is
    /// [`CapabilityTier::ReadOnly`].
    ///
    /// Returns [`ApiError::VariableNotFound`] with the given `scope` and an empty
    /// name if `name` is empty; the empty string is not a legal variable name.
    /// Deleting a variable that does not exist is *not* an error.
    pub fn delete_variable(&self, scope: VarScope, name: &str) -> Result<(), ApiError> {
        self.require_mutating()?;
        if name.is_empty() {
            return Err(ApiError::VariableNotFound {
                scope,
                name: CompactString::new_inline(""),
            });
        }
        self.push(Effect::DeleteVariable {
            scope,
            name: CompactString::from(name),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a `Cell<Vec<Effect>>` and an emitter with the given tier.
    fn make_emitter(tier: CapabilityTier) -> (Cell<Vec<Effect>>, CapabilityTier) {
        (Cell::new(Vec::new()), tier)
    }

    #[test]
    fn emitter_push_effects() {
        let (cell, tier) = make_emitter(CapabilityTier::Mutating);
        let em = EffectEmitter {
            effects: &cell,
            tier,
        };

        em.insert(0, "hello").unwrap();
        em.set_cursor(5).unwrap();

        let v = cell.take();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn emitter_tier_enforcement() {
        let (cell, tier) = make_emitter(CapabilityTier::ReadOnly);
        let em = EffectEmitter {
            effects: &cell,
            tier,
        };

        // Mutating operation should fail.
        let err = em.insert(0, "x").unwrap_err();
        assert_eq!(
            err,
            ApiError::InsufficientTier {
                required: CapabilityTier::Mutating,
                have: CapabilityTier::ReadOnly,
            }
        );

        // ReadOnly operation should succeed.
        em.set_cursor(0).unwrap();
        let v = cell.take();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn emitter_message_readonly() {
        let (cell, tier) = make_emitter(CapabilityTier::ReadOnly);
        let em = EffectEmitter {
            effects: &cell,
            tier,
        };

        em.message("hello").unwrap();
        em.error("oops").unwrap();

        let v = cell.take();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn emitter_register_validation() {
        let (cell, tier) = make_emitter(CapabilityTier::Mutating);
        let em = EffectEmitter {
            effects: &cell,
            tier,
        };

        // Invalid register character.
        let err = em
            .set_register('\x00', "text", MotionType::CharWise)
            .unwrap_err();
        assert_eq!(err, ApiError::RegisterNotFound('\x00'));
    }
}
