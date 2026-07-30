//! Mark command dispatcher.
//!
//! Maps `MarkType` (Set, `JumpLine`, `JumpExact`) to mark command implementations.
//! This is the ONLY place to update when adding mark commands.
//!
//! # Design
//!
//! Grammar layer has `MarkType` enum for parsing.
//! Commands layer has organized implementations in mark.rs.
//! This dispatcher bridges them via exhaustive match.
//!
//! # Adding New Mark Commands
//!
//! 1. Add variant to `grammar::MarkType` enum
//! 2. Create implementation in `commands/actions/mark.rs`
//! 3. Add match arm HERE in `dispatch_mark()`

pub use crate::commands::actions::MarkContext;
use crate::commands::CommandResult;
use crate::grammar::types::MarkType;

/// Dispatch a mark command to the appropriate implementation.
///
/// This is the **exhaustive match** for all mark operations.
/// Adding a new `MarkType` variant will cause a compile error here.
///
/// No dyn traits in the hot path: exhaustive match dispatch, which is
/// inlinable and allocation-free.
///
/// # Arguments
/// * `mark_type` - The mark operation type from Grammar
/// * `ctx` - Mark context with mark char, cursor position, and optional topline
/// * `target_mark` - Optional resolved `Mark` (offset + topline) for the mark (via State)
/// * `text` - Document text buffer
///
/// # Returns
/// * `CommandResult` with effects to apply
#[inline]
pub fn dispatch_mark(
    mark_type: MarkType,
    ctx: &MarkContext,
    target_mark: Option<crate::primitives::Mark>,
    text: &str,
) -> CommandResult {
    use crate::commands::actions::mark::{execute_jump_to_mark, execute_set_mark};

    match mark_type {
        MarkType::Set => execute_set_mark(ctx),
        MarkType::JumpLine => execute_jump_to_mark(ctx, target_mark, text, true),
        MarkType::JumpExact => execute_jump_to_mark(ctx, target_mark, text, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::MarkName;
    use crate::primitives::Offset;

    #[test]
    fn test_dispatch_mark_set() {
        let mark_name = MarkName::new('a').unwrap();
        let ctx = MarkContext::new(mark_name, Offset::new(42));
        let result = dispatch_mark(MarkType::Set, &ctx, None, "");
        assert!(!result.is_empty());
        assert!(matches!(
            result.effects.iter().next().unwrap(),
            Effect::SetMark { .. }
        ));
    }

    #[test]
    fn test_dispatch_mark_jump() {
        let mark_name = MarkName::new('a').unwrap();
        let ctx = MarkContext::new(mark_name, Offset::new(0));
        let result = dispatch_mark(
            MarkType::JumpLine,
            &ctx,
            Some(crate::primitives::Mark::from_raw(4)),
            "    hello",
        );
        assert!(!result.is_empty());
    }
}
