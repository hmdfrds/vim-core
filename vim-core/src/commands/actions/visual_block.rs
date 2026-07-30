//! Block visual insert/append actions (I/A in block visual mode).
//!
//! Moved from `executor.rs` inline block insert/append short-circuit
//! to keep action logic in the commands layer.

use super::types::ActionContext;
use super::types::BlockGeometry;
use crate::commands::helpers::{compute_block_geometry, line_end, line_start};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::Offset;
use crate::primitives::{LastVisualInfo, VisualType};
use unicode_segmentation::UnicodeSegmentation;

/// Execute block insert (I in block visual mode).
pub fn execute_block_insert(ctx: &ActionContext<'_>) -> CommandResult {
    execute_block_entry(ctx, false)
}

/// Execute block append (A in block visual mode).
pub fn execute_block_append(ctx: &ActionContext<'_>) -> CommandResult {
    execute_block_entry(ctx, true)
}

/// Common logic for block insert/append.
fn execute_block_entry(ctx: &ActionContext<'_>, is_append: bool) -> CommandResult {
    let Some(selection) = ctx.selection else {
        return CommandResult::empty(ctx.cursor);
    };
    let text = ctx.text;
    let geo = compute_block_geometry(text, selection.anchor(), selection.head(), ctx.tabstop);

    // Compute cursor column: I → left, A → right + 1
    // In dollar mode ($A), append at end of each line (usize::MAX clamps
    // to line length in replication code).
    let target_gcol = if is_append && ctx.dollar_mode {
        usize::MAX
    } else if is_append {
        geo.right_gcol + 1
    } else {
        geo.left_gcol
    };

    let top_line_start = line_start(text, geo.top_line).unwrap_or(0);
    let top_line_end = line_end(text, geo.top_line).unwrap_or(text.len());
    let top_graphemes: Vec<&str> = text[top_line_start..top_line_end].graphemes(true).collect();
    let clamped = target_gcol.min(top_graphemes.len());
    let cursor_byte: usize = top_graphemes
        .get(..clamped)
        .unwrap_or(&[])
        .iter()
        .map(|g| g.len())
        .sum::<usize>()
        + top_line_start;

    // Use proper InsertEntryType: I → BeforeCursor, A → AfterCursor
    let entry_type = if is_append {
        crate::primitives::InsertEntryType::AfterCursor
    } else {
        crate::primitives::InsertEntryType::BeforeCursor
    };

    // Build effects: preamble → undo group → cursor → begin_insert → optional block insert
    // begin_undo() opens the undo group that exit_finalize() will close with end_undo(),
    // so that typing + block replication are all undone as one atomic operation.
    let mut effects = build_block_preamble(&geo)
        .begin_undo()
        .set_cursor(Offset::new(cursor_byte))
        .begin_insert(entry_type, 1, 0, Offset::new(cursor_byte));

    // Set block insert context for replication across lines
    let lines_below = geo.bot_line - geo.top_line;
    if lines_below > 0 {
        let return_byte: usize = top_graphemes
            .get(..geo.left_gcol.min(top_graphemes.len()))
            .unwrap_or(&[])
            .iter()
            .map(|g| g.len())
            .sum::<usize>()
            + top_line_start;
        effects = effects.set_block_insert(lines_below, target_gcol, Offset::new(return_byte));
    }

    CommandResult::new(effects, Offset::new(cursor_byte))
}

/// Build the common preamble effects: SaveLastVisual, SetMarks, ClearSelection.
fn build_block_preamble(geo: &BlockGeometry) -> Effects {
    let info = LastVisualInfo::new(
        VisualType::Block,
        geo.bot_line - geo.top_line + 1,
        geo.right_gcol - geo.left_gcol,
    );

    let (mark_start, mark_end) = if geo.anchor <= geo.head {
        (geo.anchor, geo.head)
    } else {
        (geo.head, geo.anchor)
    };

    Effects::new()
        .save_last_visual(info)
        .set_mark(crate::primitives::MarkName::VISUAL_START, mark_start, None)
        .set_mark(crate::primitives::MarkName::VISUAL_END, mark_end, None)
        .clear_selection()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{Offset, SelectionRange};
    use std::num::NonZeroU32;

    fn make_block_ctx<'text>(text: &'text str, anchor: usize, head: usize) -> ActionContext<'text> {
        let sel = SelectionRange::new(Offset::new(anchor), Offset::new(head));
        ActionContext::from_text_and_cursor(text, Offset::new(anchor), NonZeroU32::MIN)
            .with_selection(sel)
    }

    #[test]
    fn test_block_insert_no_selection_returns_empty() {
        let ctx =
            ActionContext::from_text_and_cursor("hello\nworld\n", Offset::new(0), NonZeroU32::MIN);
        let result = execute_block_insert(&ctx);
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn test_block_insert_single_line() {
        // Select "hello" on a single line — no lines_below, no SetBlockInsert
        let ctx = make_block_ctx("hello\nworld\n", 0, 4);
        let result = execute_block_insert(&ctx);
        let has_begin_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginInsert { .. }));
        assert!(has_begin_insert, "Should enter insert mode via BeginInsert");
    }

    #[test]
    fn test_block_insert_multi_line_sets_block_context() {
        // Select across 2 lines: anchor at (0,0), head at (1,0)
        let text = "hello\nworld\n";
        let ctx = make_block_ctx(text, 0, 6);
        let result = execute_block_insert(&ctx);
        let has_block_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetBlockInsert { lines_below, .. } if *lines_below == 1));
        assert!(
            has_block_insert,
            "Multi-line block insert should emit SetBlockInsert with lines_below=1"
        );
    }

    #[test]
    fn test_block_append_cursor_after_right_edge() {
        let text = "hello\nworld\n";
        let ctx = make_block_ctx(text, 0, 6);
        let result = execute_block_append(&ctx);
        let has_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginInsert { .. }));
        assert!(
            has_insert,
            "Block append should enter insert mode via BeginInsert"
        );
    }

    #[test]
    fn test_block_preamble_saves_visual_info() {
        let text = "hello\nworld\n";
        let ctx = make_block_ctx(text, 0, 6);
        let result = execute_block_insert(&ctx);
        let has_save = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SaveLastVisual { .. }));
        let has_marks = result
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::SetMark { .. }))
            .count();
        let has_clear = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSelection));
        assert!(has_save, "Should save last visual info");
        assert_eq!(has_marks, 2, "Should set < and > marks");
        assert!(has_clear, "Should clear selection");
    }
}
