//! Register routing for operator commands.
//!
//! Centralizes the Vim register routing logic used by delete, change, and yank
//! operators. Eliminates duplication across operator files.
//!
//! # Delete/Change Register Rules
//!
//! - Always: unnamed `"` gets the text
//! - Explicit register (not `"`): only that register
//! - No explicit + linewise/multiline: numbered `1`
//! - No explicit + force_numbered: small-delete `-` AND numbered `1`
//! - No explicit + small delete: small-delete `-` only
//!
//! # Yank Register Rules
//!
//! - Always: unnamed `"` gets the text
//! - Explicit register (not `"`): only that register
//! - No explicit: yank register `0`

use crate::effects::undo_state::EffectState;
use crate::effects::Effects;
use crate::primitives::{MotionType, RegisterName};

/// Numbered register 1 (first delete slot).
const REG_1: RegisterName = RegisterName::NUMBERED_1;

/// Route deleted/changed text to registers per Vim spec.
///
/// Sets the unnamed register `"` and then routes to the appropriate
/// special register based on the context.
///
/// # Arguments
/// * `effects` - Effects builder to append register effects to
/// * `text` - The deleted/changed text to store
/// * `motion_type` - CharWise/LineWise/BlockWise
/// * `register` - The explicit register (UNNAMED means no explicit register)
/// * `is_multiline` - Whether the operation spans multiple lines or is linewise
/// * `force_numbered` - Whether to force numbered register (for search/jump motions)
pub fn route_delete_registers<S: EffectState>(
    effects: Effects<S>,
    text: &str,
    motion_type: MotionType,
    register: RegisterName,
    is_multiline: bool,
    force_numbered: bool,
) -> Effects<S> {
    // Black hole register: suppress ALL register updates
    if register.is_blackhole() {
        return effects;
    }

    // Always update the unnamed register
    let effects = effects.set_register(RegisterName::UNNAMED, text, motion_type);

    // Route to special registers.
    if register != RegisterName::UNNAMED {
        // Explicit register specified: set that register.
        // Neovim also rotates numbered registers 1-9 for linewise/multiline
        // deletes even with explicit register (`:h registers`).
        let effects = effects.set_register(register, text, motion_type);
        if is_multiline {
            effects.set_register(REG_1, text, motion_type)
        } else {
            effects
        }
    } else if is_multiline {
        // Linewise or multi-line: shift numbered registers 1-9
        effects.set_register(REG_1, text, motion_type)
    } else if force_numbered {
        // Special motion (search/jump): sub-line delete goes to both '-' and '1'
        effects
            .set_register(RegisterName::SMALL_DELETE, text, motion_type)
            .set_register(REG_1, text, motion_type)
    } else {
        // Small delete (< 1 line): update small delete register only
        effects.set_register(RegisterName::SMALL_DELETE, text, motion_type)
    }
}

/// Route yanked text to registers per Vim spec.
///
/// Sets the unnamed register `"` and then routes to the appropriate
/// special register based on the context.
///
/// # Arguments
/// * `effects` - Effects builder to append register effects to
/// * `text` - The yanked text to store
/// * `motion_type` - CharWise/LineWise/BlockWise
/// * `register` - The explicit register (UNNAMED means no explicit register)
pub fn route_yank_registers<S: EffectState>(
    effects: Effects<S>,
    text: &str,
    motion_type: MotionType,
    register: RegisterName,
) -> Effects<S> {
    // Black hole register: suppress ALL register updates
    if register.is_blackhole() {
        return effects;
    }

    // Always update the unnamed register
    let effects = effects.set_register(RegisterName::UNNAMED, text, motion_type);

    if register == RegisterName::UNNAMED {
        // No explicit register: also update yank register 0
        effects.set_register(RegisterName::LAST_YANK, text, motion_type)
    } else {
        // Explicit register specified: only that register
        effects.set_register(register, text, motion_type)
    }
}

/// Route register effects for empty-buffer linewise operations (dd/yy on empty buffer).
///
/// Neovim always writes `"\n"` linewise to registers even on empty buffers.
/// This centralizes the routing logic that was duplicated across
/// `operator_line.rs` and `operator_textobject.rs`.
///
/// # Register Rules
///
/// - Always: unnamed `"` gets `"\n"` linewise
/// - Explicit register (not `"`): also that register
/// - No explicit + delete: numbered `"1`
/// - No explicit + yank: yank register `"0`
pub fn route_empty_buffer_registers(
    operator: crate::grammar::types::Operator,
    register: Option<RegisterName>,
) -> Effects {
    let reg_name = register.unwrap_or(RegisterName::UNNAMED);
    let mut effects =
        Effects::new().set_register(RegisterName::UNNAMED, "\n", MotionType::LineWise);
    if reg_name != RegisterName::UNNAMED {
        effects = effects.set_register(reg_name, "\n", MotionType::LineWise);
    } else if matches!(operator, crate::grammar::types::Operator::Delete) {
        effects = effects.set_register(REG_1, "\n", MotionType::LineWise);
    } else {
        effects = effects.set_register(RegisterName::LAST_YANK, "\n", MotionType::LineWise);
    }
    // Neovim sets [, ] marks even on empty buffer for dd/yy.
    // '[ = 0, '] = 0 (both point to start of empty buffer).
    effects = effects
        .set_mark(
            crate::primitives::MarkName::CHANGE_START,
            crate::primitives::Offset::new(0),
            None,
        )
        .set_mark(
            crate::primitives::MarkName::CHANGE_END,
            crate::primitives::Offset::new(0),
            None,
        );
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    #[test]
    fn test_delete_routing_default() {
        let effects = route_delete_registers(
            Effects::new(),
            "hello",
            MotionType::CharWise,
            RegisterName::UNNAMED,
            false,
            false,
        );

        let has_unnamed = effects.iter().any(
            |e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::UNNAMED),
        );
        let has_small = effects.iter().any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::SMALL_DELETE));
        assert!(has_unnamed, "Should set unnamed register");
        assert!(has_small, "Small delete should set register -");
    }

    #[test]
    fn test_delete_routing_multiline() {
        let effects = route_delete_registers(
            Effects::new(),
            "line1\nline2\n",
            MotionType::LineWise,
            RegisterName::UNNAMED,
            true,
            false,
        );

        let has_numbered = effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == REG_1));
        let has_small = effects.iter().any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::SMALL_DELETE));
        assert!(has_numbered, "Multiline should set register 1");
        assert!(!has_small, "Multiline should not set register -");
    }

    #[test]
    fn test_delete_routing_explicit_register() {
        let reg_a = RegisterName::new_unchecked('a');
        let effects = route_delete_registers(
            Effects::new(),
            "hello",
            MotionType::CharWise,
            reg_a,
            false,
            false,
        );

        let has_reg_a = effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == reg_a));
        let has_numbered = effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == REG_1));
        let has_small = effects.iter().any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::SMALL_DELETE));
        assert!(has_reg_a, "Should set explicit register a");
        assert!(!has_numbered, "Explicit register should not set register 1");
        assert!(!has_small, "Explicit register should not set register -");
    }

    #[test]
    fn test_delete_routing_force_numbered() {
        let effects = route_delete_registers(
            Effects::new(),
            "hello",
            MotionType::CharWise,
            RegisterName::UNNAMED,
            false,
            true,
        );

        let has_numbered = effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == REG_1));
        let has_small = effects.iter().any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::SMALL_DELETE));
        assert!(has_numbered, "Force numbered should set register 1");
        assert!(has_small, "Force numbered should also set register -");
    }

    #[test]
    fn test_yank_routing_default() {
        let effects = route_yank_registers(
            Effects::new(),
            "hello",
            MotionType::CharWise,
            RegisterName::UNNAMED,
        );

        let has_unnamed = effects.iter().any(
            |e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::UNNAMED),
        );
        let has_yank = effects.iter().any(
            |e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::LAST_YANK),
        );
        assert!(has_unnamed, "Should set unnamed register");
        assert!(has_yank, "Default yank should set register 0");
    }

    #[test]
    fn test_yank_routing_explicit_register() {
        let reg_a = RegisterName::new_unchecked('a');
        let effects = route_yank_registers(Effects::new(), "hello", MotionType::CharWise, reg_a);

        let has_reg_a = effects
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { name, .. } if *name == reg_a));
        let has_yank = effects.iter().any(
            |e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::LAST_YANK),
        );
        assert!(has_reg_a, "Should set explicit register a");
        assert!(
            !has_yank,
            "Explicit register yank should not set register 0"
        );
    }
}
