//! Mark commands.
//!
//! Setting and jumping to marks.
//!
//! # Commands
//!
//! | Command | Action |
//! |---------|--------|
//! | `m{a-z}` | Set local mark |
//! | `m{A-Z}` | Set global mark |

use super::types::MarkContext;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::byte_delta;

/// Set a mark at the current position.
///
/// Per Vim spec:
/// - `m{a-z}` sets a local mark
/// - `m{A-Z}` sets a global mark
/// - Non-alphabetic marks (e.g., numbered) cannot be set by user.
///
/// The `topline` from `MarkContext` is forwarded into the `SetMark` effect
/// so the effect processor can store viewport context alongside the mark.
pub fn execute_set_mark(ctx: &MarkContext) -> CommandResult {
    if !ctx.mark.is_settable() {
        return CommandResult::effects_only(Effects::new());
    }

    CommandResult::effects_only(Effects::new().set_mark(ctx.mark, ctx.cursor, ctx.topline_offset))
}

/// Jump to a mark.
///
/// This pushes current position to jump list and emits SetCursor effect.
/// When the mark has a stored topline and the cursor is jumping to a
/// different line, a `ScrollTo` effect is emitted AFTER `SetCursor` to
/// restore the viewport position that was active when the mark was set.
///
/// # Arguments
/// * `ctx` - Mark context with mark name and cursor position
/// * `target_mark` - The resolved `Mark` (offset + optional topline) from State
/// * `text` - The document text
/// * `to_line_start` - If true, jump to first non-blank of mark's line (`'a`)
pub fn execute_jump_to_mark(
    ctx: &MarkContext,
    target_mark: Option<crate::primitives::Mark>,
    text: &str,
    to_line_start: bool,
) -> CommandResult {
    let Some(mark) = target_mark else {
        return CommandResult::effects_only(Effects::new());
    };

    let target = mark.offset();

    // Clamp mark offset to valid range (mark may point past text end,
    // e.g., `^` mark after insert at EOB in Neovim's model).
    // Use prev_char_boundary to land on a valid character boundary,
    // not inside a multi-byte character.
    let offset = if target.get() >= text.len() && !text.is_empty() {
        crate::commands::helpers::prev_char_boundary(text, text.len())
    } else {
        target.get()
    };

    let current_line = crate::commands::helpers::line_of(text, ctx.cursor.get());
    let mark_line = crate::commands::helpers::line_of(text, offset);

    let mut effects = Effects::new();
    if current_line != mark_line {
        effects = effects.push_jump_list(ctx.cursor);
    }

    let final_offset = if to_line_start {
        let line_text = crate::commands::helpers::current_line(text, offset);
        let offset_in_line = crate::commands::helpers::first_non_blank_in_line(line_text);
        let line_start = crate::commands::helpers::line_start_for_offset(text, offset);
        line_start + offset_in_line
    } else {
        offset
    };

    // SetCursor first
    effects = effects.set_cursor(crate::primitives::Offset::new(final_offset));

    // ScrollTo AFTER SetCursor: restore viewport position when the mark has
    // a stored topline_offset and the cursor is moving to a different line.
    // Resolve relative offset: topline_line = mark_line - topline_offset.
    if let Some(tl_off) = mark.topline_offset() {
        if current_line != mark_line {
            let topline_line = byte_delta::shift(mark_line, -i64::from(tl_off));
            if let Some(topline_byte) = crate::commands::helpers::line_start(text, topline_line) {
                effects = effects.scroll_to(crate::primitives::Offset::new(topline_byte));
            }
        }
    }

    CommandResult::effects_only(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{MarkName, Offset};

    #[test]
    fn test_set_local_mark() {
        let ctx = MarkContext::new(MarkName::new('a').unwrap(), Offset::new(100));
        let result = execute_set_mark(&ctx);

        assert_eq!(result.effects.len(), 1);
        let effect = result.effects.iter().next().unwrap();
        assert!(matches!(effect, Effect::SetMark { name, .. } if name.char() == 'a'));
    }

    #[test]
    fn test_set_global_mark() {
        let ctx = MarkContext::new(MarkName::new('A').unwrap(), Offset::new(200));
        let result = execute_set_mark(&ctx);

        assert_eq!(result.effects.len(), 1);
        let effect = result.effects.iter().next().unwrap();
        assert!(matches!(effect, Effect::SetMark { name, .. } if name.char() == 'A'));
    }

    #[test]
    fn test_non_settable_mark() {
        // Numbered marks cannot be set by user
        let ctx = MarkContext::new(MarkName::new('1').unwrap(), Offset::new(100));
        let result = execute_set_mark(&ctx);
        assert!(result.effects.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Viewport Topline Tests (relative encoding)
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn set_mark_captures_topline_offset() {
        let topline_offset = Some(5i32);
        let ctx = MarkContext::with_topline_offset(
            MarkName::new('a').unwrap(),
            Offset::new(100),
            topline_offset,
        );
        let result = execute_set_mark(&ctx);

        assert_eq!(result.effects.len(), 1);
        let effect = result.effects.iter().next().unwrap();
        match effect {
            Effect::SetMark {
                name,
                offset,
                topline_offset: tl,
            } => {
                assert_eq!(name.char(), 'a');
                assert_eq!(offset.get(), 100);
                assert_eq!(*tl, Some(5));
            }
            other => panic!("expected SetMark, got {:?}", other),
        }
    }

    #[test]
    fn set_mark_without_topline_offset() {
        let ctx = MarkContext::new(MarkName::new('b').unwrap(), Offset::new(42));
        let result = execute_set_mark(&ctx);

        let effect = result.effects.iter().next().unwrap();
        match effect {
            Effect::SetMark { topline_offset, .. } => {
                assert_eq!(
                    *topline_offset, None,
                    "auto-mark should have no topline_offset"
                );
            }
            other => panic!("expected SetMark, got {:?}", other),
        }
    }

    #[test]
    fn jump_emits_scroll_to_when_topline_offset_present_and_cross_line() {
        // Cursor at line 0 (offset 0), mark at line 1 (offset 7)
        // topline_offset = 1 (mark_line=1 - viewport_first_line=0)
        let ctx = MarkContext::new(MarkName::new('a').unwrap(), Offset::new(0));
        let mark = crate::primitives::Mark::with_topline_offset(Offset::new(7), Some(1));
        let result = execute_jump_to_mark(&ctx, Some(mark), "line 1\nline 2", false);

        // Should have: PushJumpList, SetCursor, ScrollTo
        let effects: Vec<_> = result.effects.iter().collect();
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::PushJumpList { .. })),
            "cross-line jump should emit PushJumpList"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SetCursor { offset } if offset.get() == 7)),
            "should emit SetCursor to mark offset"
        );
        // topline_line = mark_line(1) - topline_offset(1) = 0, line_start(0) = 0
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::ScrollTo { offset } if offset.get() == 0)),
            "should emit ScrollTo with resolved topline"
        );

        // Verify ScrollTo comes after SetCursor
        let set_cursor_idx = effects
            .iter()
            .position(|e| matches!(e, Effect::SetCursor { .. }))
            .unwrap();
        let scroll_to_idx = effects
            .iter()
            .position(|e| matches!(e, Effect::ScrollTo { .. }))
            .unwrap();
        assert!(
            scroll_to_idx > set_cursor_idx,
            "ScrollTo must come after SetCursor"
        );
    }

    #[test]
    fn jump_no_scroll_to_when_topline_offset_none() {
        // Mark without topline_offset (auto-mark) should not emit ScrollTo
        let ctx = MarkContext::new(MarkName::new('a').unwrap(), Offset::new(0));
        let mark = crate::primitives::Mark::new(Offset::new(7));
        let result = execute_jump_to_mark(&ctx, Some(mark), "line 1\nline 2", false);

        let has_scroll_to = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ScrollTo { .. }));
        assert!(
            !has_scroll_to,
            "mark without topline_offset should not emit ScrollTo"
        );
    }

    #[test]
    fn jump_no_scroll_to_when_same_line() {
        // Mark on the same line as cursor — no ScrollTo even with topline_offset
        let ctx = MarkContext::new(MarkName::new('a').unwrap(), Offset::new(0));
        let mark = crate::primitives::Mark::with_topline_offset(Offset::new(3), Some(0));
        let result = execute_jump_to_mark(&ctx, Some(mark), "line 1\nline 2", false);

        let has_scroll_to = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ScrollTo { .. }));
        assert!(!has_scroll_to, "same-line jump should not emit ScrollTo");
    }
}
