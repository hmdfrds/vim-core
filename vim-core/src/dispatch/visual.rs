//! Visual mode command dispatcher.
//!
//! Maps visual mode Commands to `commands::visual` implementations.
//! This is the ONLY place to update when adding visual commands.
//!
//! # Design
//!
//! Grammar layer has visual Command variants for parsing.
//! Commands layer has organized implementations (mode.rs).
//! This dispatcher bridges them via exhaustive match.
//!
//! # Adding New Visual Commands
//!
//! 1. Add variant to `grammar::Command`
//! 2. Create implementation in `commands/visual/`
//! 3. Add match arm HERE in `dispatch_visual()`

use crate::commands::visual::mode;
pub use crate::commands::visual::selection::expand_selection_to_lines;
pub use crate::commands::visual::selection::save_last_visual_effects;
pub use crate::commands::visual::selection::{
    reconstruct_from_last_visual, resolve_live_selection,
};
pub use crate::commands::visual::VisualContext;
use crate::commands::CommandResult;
use crate::grammar::{Command, VisualKind};

/// Dispatch a visual mode Command to the appropriate implementation.
///
/// This handles all visual-mode specific commands.
///
/// No dyn traits in the hot path: exhaustive match dispatch, which is
/// inlinable and allocation-free.
///
/// # Arguments
/// * `cmd` - The visual command from Grammar
/// * `ctx` - Visual context with cursor position and selection
///
/// # Returns
/// * `CommandResult` with effects to apply
#[inline]
pub fn dispatch_visual(cmd: &Command, ctx: &VisualContext<'_>) -> CommandResult {
    match cmd {
        Command::Visual(VisualKind::Enter { visual_type, count }) => {
            mode::enter(ctx, *visual_type, *count)
        }
        Command::Visual(VisualKind::Exit) => mode::exit(ctx),
        Command::Visual(VisualKind::Switch { visual_type }) => mode::switch(ctx, *visual_type),
        Command::Visual(VisualKind::SwapEnds) => mode::swap_ends(ctx),
        Command::Visual(VisualKind::SwapCorner) => mode::swap_corner(ctx),
        Command::Visual(VisualKind::Reselect) => mode::reselect(ctx),

        // ToggleSelect (Ctrl-G) is intercepted by the executor before reaching dispatch,
        // because it needs the full Mode (Visual vs Select) to determine toggle direction.
        // This arm satisfies exhaustiveness and guards against incorrect routing.
        Command::Visual(VisualKind::ToggleSelect) => {
            debug_assert!(
                false,
                "ToggleSelect should be intercepted by executor before reaching dispatch_visual"
            );
            CommandResult::none()
        }

        // Not visual commands - should not reach here
        other => {
            debug_assert!(
                false,
                "Non-visual command reached dispatch_visual: {other:?}"
            );
            CommandResult::effects_only(crate::effects::Effects::new().show_error(
                crate::errors::VimError::InternalError(
                    format!("Non-visual command in visual mode: {other:?}").into(),
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;
    use crate::primitives::VisualType;

    #[test]
    fn test_dispatch_visual_enter() {
        let ctx = VisualContext::new("", Offset::new(5), None);
        let cmd = Command::Visual(VisualKind::Enter {
            visual_type: VisualType::Char,
            count: None,
        });
        let result = dispatch_visual(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_visual_exit() {
        let ctx = VisualContext::new("", Offset::new(5), None);
        let cmd = Command::Visual(VisualKind::Exit);
        let result = dispatch_visual(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    // ── Fold-aware visual entry tests ────────────────────────────────

    mod fold_aware_visual {
        use super::*;
        use crate::document::FoldProvider;
        use crate::primitives::{Direction, LineNumber};

        /// Fold: lines 1-2 are folded.
        struct FoldLines1To2;
        impl FoldProvider for FoldLines1To2 {
            fn next_visible_line(&self, line: LineNumber, dir: Direction) -> LineNumber {
                if (1..=2).contains(&line.get()) {
                    match dir {
                        Direction::Forward => LineNumber::new(3),
                        Direction::Backward => LineNumber::new(0),
                    }
                } else {
                    line
                }
            }
            fn is_folded(&self, line: LineNumber) -> bool {
                (1..=2).contains(&line.get())
            }
        }

        #[test]
        fn visual_enter_inside_fold_expands_selection() {
            // "aaa\nbbb\nccc\nddd\n"
            //  L0   L1   L2   L3
            //  0    4    8    12
            // Lines 1-2 folded. Cursor at offset 5 (inside fold, line 1).
            let text = "aaa\nbbb\nccc\nddd\n";
            let fold = FoldLines1To2;
            let mut ctx = VisualContext::new(text, Offset::new(5), None);
            ctx.fold_provider = Some(&fold);

            let cmd = Command::Visual(VisualKind::Enter {
                visual_type: VisualType::Char,
                count: None,
            });
            let result = dispatch_visual(&cmd, &ctx);

            // The selection should be expanded to cover the fold.
            // Check that SetVisualSelection effect has anchor != head.
            let has_expanded = result.effects.iter().any(|e| {
                if let crate::effects::Effect::SetSelection { anchor, head, .. } = e {
                    // anchor should be at fold start (line 1 start = 4)
                    // head should be at fold end (line 2 end = 11)
                    anchor.get() <= 4 && head.get() >= 11
                } else {
                    false
                }
            });
            assert!(
                has_expanded,
                "Visual entry inside fold should expand selection to cover fold"
            );
        }

        #[test]
        fn visual_enter_outside_fold_collapsed() {
            // Cursor at offset 0 (line 0, not folded). Selection should be collapsed.
            let text = "aaa\nbbb\nccc\nddd\n";
            let fold = FoldLines1To2;
            let mut ctx = VisualContext::new(text, Offset::new(0), None);
            ctx.fold_provider = Some(&fold);

            let cmd = Command::Visual(VisualKind::Enter {
                visual_type: VisualType::Char,
                count: None,
            });
            let result = dispatch_visual(&cmd, &ctx);

            let has_collapsed = result.effects.iter().any(|e| {
                if let crate::effects::Effect::SetSelection { anchor, head, .. } = e {
                    anchor == head
                } else {
                    false
                }
            });
            assert!(
                has_collapsed,
                "Visual entry outside fold should have collapsed selection"
            );
        }
    }
}
