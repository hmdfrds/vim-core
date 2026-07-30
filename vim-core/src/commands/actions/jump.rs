//! Jump list navigation commands.
//!
//! Handles Ctrl-O (older) and Ctrl-I (newer) jump navigation.
//!
//! # Commands
//!
//! | Command | Action |
//! |---------|--------|
//! | `Ctrl-O` | Jump to older position in jump list |
//! | `Ctrl-I` | Jump to newer position in jump list |

use super::types::ActionContext;
use crate::commands::CommandResult;
use crate::effects::Effects;

/// Execute jump older action (Ctrl-O from action dispatch).
///
/// Emits a `JumpOlder` effect that the effect processor resolves
/// against the actual `JumpList` in state. This keeps the action
/// layer free from `&mut JumpList` references.
#[inline]
pub fn execute_jump_older(ctx: &ActionContext<'_>) -> CommandResult {
    let effects = Effects::new().jump_older(ctx.count);
    CommandResult::new(effects, ctx.cursor)
}

/// Execute jump newer action (Ctrl-I from action dispatch).
///
/// Emits a `JumpNewer` effect that the effect processor resolves
/// against the actual `JumpList` in state.
#[inline]
pub fn execute_jump_newer(ctx: &ActionContext<'_>) -> CommandResult {
    let effects = Effects::new().jump_newer(ctx.count);
    CommandResult::new(effects, ctx.cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::Offset;
    use std::num::NonZeroU32;

    #[test]
    fn test_execute_jump_older_emits_effect() {
        let ctx = ActionContext::from_text_and_cursor("hello", Offset::new(0), NonZeroU32::MIN);
        let result = execute_jump_older(&ctx);
        assert_eq!(result.cursor.unwrap().get(), 0);
        let has_jump = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::JumpOlder { .. }));
        assert!(has_jump, "Should emit JumpOlder effect");
    }

    #[test]
    fn test_execute_jump_newer_emits_effect() {
        let ctx = ActionContext::from_text_and_cursor("hello", Offset::new(0), NonZeroU32::MIN);
        let result = execute_jump_newer(&ctx);
        assert_eq!(result.cursor.unwrap().get(), 0);
        let has_jump = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::JumpNewer { .. }));
        assert!(has_jump, "Should emit JumpNewer effect");
    }
}
