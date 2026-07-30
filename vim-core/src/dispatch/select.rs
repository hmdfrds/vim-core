//! Select mode command dispatcher.
//!
//! Maps select mode entry commands to `commands::visual::mode` implementations.
//! Select mode entry parallels visual mode entry: sets selection anchor at
//! the current cursor position and switches to Select mode.
//!
//! # Design
//!
//! Select mode uses the same selection infrastructure as visual mode,
//! with the mode being `Select(vt)` instead of `Visual(vt)`.

use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{Mode, VisualType};
use crate::primitives::{Offset, SelectionShape};

/// Dispatch select mode entry.
///
/// Creates a collapsed selection at the cursor and switches to Select mode.
/// Parallels `commands::visual::mode::enter()` but targets Select mode.
///
/// # Arguments
/// * `cursor` - Current cursor position (becomes both anchor and head)
/// * `visual_type` - The selection type (Char, Line, Block)
///
/// # Returns
/// * `CommandResult` with SetSelection + SetMode(Select) effects
#[inline]
pub fn dispatch_select_enter(cursor: Offset, visual_type: VisualType) -> CommandResult {
    CommandResult::effects_only(
        Effects::new()
            .set_mode(Mode::Select(visual_type))
            .set_visual_selection(cursor, cursor, SelectionShape::from(visual_type)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    #[test]
    fn select_enter_produces_set_mode_and_set_selection() {
        let cursor = Offset::new(5);
        let result = dispatch_select_enter(cursor, VisualType::Char);
        assert_eq!(result.effects.len(), 3);

        let effects: Vec<_> = result.effects.iter().collect();
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::SetMode {
                    mode: Mode::Select(VisualType::Char),
                    ..
                }
            )),
            "Expected SetMode(Select(Char)) effect"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SetSelection { .. })),
            "Expected SetSelection effect"
        );
    }

    #[test]
    fn select_enter_linewise() {
        let cursor = Offset::new(10);
        let result = dispatch_select_enter(cursor, VisualType::Line);
        assert_eq!(result.effects.len(), 3);

        let effects: Vec<_> = result.effects.iter().collect();
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::SetMode {
                    mode: Mode::Select(VisualType::Line),
                    ..
                }
            )),
            "Expected SetMode(Select(Line)) effect"
        );
    }

    #[test]
    fn select_enter_blockwise() {
        let cursor = Offset::new(0);
        let result = dispatch_select_enter(cursor, VisualType::Block);
        assert_eq!(result.effects.len(), 3);

        let effects: Vec<_> = result.effects.iter().collect();
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::SetMode {
                    mode: Mode::Select(VisualType::Block),
                    ..
                }
            )),
            "Expected SetMode(Select(Block)) effect"
        );
    }

    #[test]
    fn select_enter_selection_is_collapsed_at_cursor() {
        let cursor = Offset::new(7);
        let result = dispatch_select_enter(cursor, VisualType::Char);

        let effects: Vec<_> = result.effects.iter().collect();
        let sel = effects.iter().find_map(|e| {
            if let Effect::SetSelection { anchor, head, .. } = e {
                Some((*anchor, *head))
            } else {
                None
            }
        });
        assert_eq!(sel, Some((cursor, cursor)));
    }
}
