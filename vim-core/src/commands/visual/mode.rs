//! Visual mode commands.
//!
//! Handles entering, exiting, and manipulating visual mode.
//!
//! # Commands
//!
//! | Key | Function | Description |
//! |-----|----------|-------------|
//! | `v` | `enter` | Enter character-wise visual |
//! | `V` | `enter` | Enter line-wise visual |
//! | `C-v` | `enter` | Enter block-wise visual |
//! | `Esc` | `exit` | Exit visual mode |
//! | `v/V/C-v` | `switch` | Switch visual type |
//! | `o` | `swap_ends` | Swap anchor/head |
//! | `O` | `swap_corner` | Swap corners (block) |
//! | `gv` | `reselect` | Reselect last visual |

use super::types::VisualContext;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{Mode, VisualType};
use crate::primitives::{Offset, SelectionShape};

/// Enter visual mode from normal.
///
/// Sets mode and creates selection at cursor. When the cursor is inside a
/// closed fold, the selection is expanded to cover the entire fold.
#[inline]
pub fn enter(
    ctx: &VisualContext<'_>,
    visual_type: VisualType,
    count: Option<u32>,
) -> CommandResult {
    // When count is provided and a previous visual selection exists,
    // scale the selection dimensions by count (Neovim `3v` behavior).
    if let (Some(c), Some(last)) = (count, ctx.last_visual_info.as_ref()) {
        if c > 0 {
            let scaled = crate::primitives::LastVisualInfo::new(
                last.visual_type(),
                last.lines().saturating_mul(c as usize),
                last.columns().saturating_mul(c as usize),
            );
            let (range, _motion_type, _) =
                crate::commands::visual::selection::reconstruct_from_last_visual(
                    ctx.text,
                    ctx.cursor.get(),
                    &scaled,
                    ctx.tabstop,
                );
            let shape = SelectionShape::from(visual_type);
            return CommandResult::effects_only(
                Effects::new()
                    .set_mode(Mode::Visual(visual_type))
                    .set_visual_selection(range.start(), range.end(), shape),
            );
        }
    }

    let (anchor, head) = if let Some(fold) = ctx.fold_provider {
        use crate::commands::helpers::fold_snap;
        use crate::primitives::Direction;
        let start = fold_snap(ctx.text, ctx.cursor.get(), Direction::Backward, fold);
        let end = fold_snap(ctx.text, ctx.cursor.get(), Direction::Forward, fold);
        (Offset::new(start), Offset::new(end))
    } else {
        (ctx.cursor, ctx.cursor)
    };
    CommandResult::effects_only(
        Effects::new()
            .set_mode(Mode::Visual(visual_type))
            .set_visual_selection(anchor, head, SelectionShape::from(visual_type)),
    )
}

/// Exit visual mode.
///
/// Clears selection and returns to normal.
/// Emits SetMark for '<' and '>' to remember last visual selection.
/// Saves LastVisualInfo for `gv` restoration.
#[inline]
pub fn exit(ctx: &VisualContext<'_>) -> CommandResult {
    let mut effects = Effects::new();

    // Save LastVisualInfo for gv restoration (before clearing selection)
    if let (Some(visual_type), Some(selection)) = (ctx.current_visual_type, &ctx.selection) {
        let (sel_start, sel_end) = (selection.start().get(), selection.end().get());
        let cursor_at_start = !selection.is_forward();
        let save_effects = super::selection::save_last_visual_effects(
            ctx.text,
            visual_type,
            sel_start,
            sel_end,
            ctx.tabstop,
            cursor_at_start,
        );
        effects.extend(save_effects);
    }

    // Set '<' and '>' marks to remember last visual selection (per Vim behavior)
    if let Some(selection) = &ctx.selection {
        let (sel_start, sel_end) = if selection.is_forward() {
            (selection.anchor(), selection.head())
        } else {
            (selection.head(), selection.anchor())
        };
        effects = effects
            .set_mark(crate::primitives::MarkName::VISUAL_START, sel_start, None)
            .set_mark(crate::primitives::MarkName::VISUAL_END, sel_end, None);
    }

    effects = effects.clear_selection().set_mode(Mode::Normal);

    CommandResult::effects_only(effects)
}

/// Switch visual mode type (e.g., v -> V or V -> v).
///
/// Changes mode and re-emits the current live selection with the new shape.
#[inline]
pub fn switch(ctx: &VisualContext<'_>, visual_type: VisualType) -> CommandResult {
    let mut effects = Effects::new().set_mode(Mode::Visual(visual_type));
    if let Some(selection) = &ctx.selection {
        effects = effects.set_visual_selection(
            selection.anchor(),
            selection.head(),
            SelectionShape::from(visual_type),
        );
    }
    CommandResult::effects_only(effects)
}

/// Swap selection anchor and head (o command).
///
/// Flips the selection direction.
#[inline]
pub fn swap_ends(ctx: &VisualContext<'_>) -> CommandResult {
    if let Some(selection) = &ctx.selection {
        CommandResult::effects_only(
            Effects::new().set_visual_selection(
                selection.head(),
                selection.anchor(),
                ctx.current_visual_type
                    .map_or(SelectionShape::Char, SelectionShape::from),
            ),
        )
    } else {
        CommandResult::none()
    }
}

/// Swap block corners (O command in block mode).
///
/// In block mode, swaps the horizontal column positions of anchor and head
/// so each gets the other's grapheme column on their respective lines.
/// This is the full grapheme-aware implementation.
pub fn swap_corner(ctx: &VisualContext<'_>) -> CommandResult {
    use crate::commands::helpers::{gcol_to_byte, line_of, line_start};
    use unicode_segmentation::UnicodeSegmentation;

    let Some(selection) = &ctx.selection else {
        return CommandResult::none();
    };

    let text = ctx.text;
    let anchor = selection.anchor().get();
    let head = selection.head().get();
    let anchor_line = line_of(text, anchor);
    let head_line = line_of(text, head);
    let anchor_ls = line_start(text, anchor_line).unwrap_or(0);
    let head_ls = line_start(text, head_line).unwrap_or(0);

    let anchor_gcol = text[anchor_ls..anchor].graphemes(true).count();
    let head_gcol = text[head_ls..head].graphemes(true).count();

    // Swap: anchor gets head's column, head gets anchor's column
    let new_anchor_byte = gcol_to_byte(text, anchor_line, head_gcol);
    let new_head_byte = gcol_to_byte(text, head_line, anchor_gcol);

    CommandResult::effects_only(Effects::new().set_visual_selection(
        Offset::new(new_anchor_byte),
        Offset::new(new_head_byte),
        SelectionShape::Block,
    ))
}

/// Reselect previous visual selection (gv).
///
/// Uses marks `<` and `>` to restore last visual region.
///
/// For linewise mode, uses `<` mark + stored line count from `LastVisualInfo`
/// to correctly cover the same lines even after text edits that change line
/// lengths (e.g., indent). This mirrors Neovim's behavior where `>` stores
/// (line, INT_MAX) for linewise selections.
///
/// When called from within an active visual selection (Neovim behavior),
/// exchanges the current visual area with the previously stored one:
/// the current selection is saved as the new "previous", and the old
/// previous is restored as the active selection.
#[inline]
pub fn reselect(ctx: &VisualContext<'_>) -> CommandResult {
    if let (Some(visual_type), Some((start, end))) = (ctx.last_visual_type, ctx.last_visual_marks) {
        let doc_len = ctx.text.len();
        let doc_end = Offset::new(doc_len);

        // Preserve unclamped mark offsets for curswant computation.
        // Visual marks are not adjusted through text edits (Neovim behavior),
        // so the original column may exceed the current line length.
        let raw_start = start;
        let raw_end = end;

        let start = start.min(doc_end);

        // For linewise: use `<` mark + line count to find the correct end position.
        // The `>` mark as a byte offset can't reliably track line identity through
        // edits that change line lengths (indent/dedent), so we use the stored
        // line count from LastVisualInfo instead.
        let end = if visual_type.is_line() {
            if let Some(lines) = ctx.last_visual_lines {
                let end_clamped = raw_end.min(doc_end);
                let mark_lines = {
                    let s = start.get();
                    let e = end_clamped.get();
                    if s >= ctx.text.len() || e <= s {
                        1
                    } else {
                        ctx.text[s..e.min(ctx.text.len())]
                            .bytes()
                            .filter(|&b| b == b'\n')
                            .count()
                            + 1
                    }
                };
                compute_linewise_end(ctx.text, start.get(), lines.min(mark_lines))
            } else {
                raw_end.min(doc_end)
            }
        } else {
            raw_end.min(doc_end)
        };

        let mut effects = Effects::new();

        // Neovim behavior: if currently in visual mode, save the active selection
        // as the new "previous" before restoring the old one (exchange semantics).
        if let (Some(cur_vtype), Some(cur_sel)) = (ctx.current_visual_type, &ctx.selection) {
            let (sel_start, sel_end) = (cur_sel.start().get(), cur_sel.end().get());
            let cur_cursor_at_start = !cur_sel.is_forward();
            // Save current selection as LastVisualInfo
            let save_fx = super::selection::save_last_visual_effects(
                ctx.text,
                cur_vtype,
                sel_start,
                sel_end,
                ctx.tabstop,
                cur_cursor_at_start,
            );
            effects.extend(save_fx);
            // Update `<` and `>` marks to current selection
            let (mark_start, mark_end) = if cur_sel.is_forward() {
                (cur_sel.anchor(), cur_sel.head())
            } else {
                (cur_sel.head(), cur_sel.anchor())
            };
            effects = effects
                .set_mark(crate::primitives::MarkName::VISUAL_START, mark_start, None)
                .set_mark(crate::primitives::MarkName::VISUAL_END, mark_end, None);
        }

        // Now restore the previous visual selection.
        // Place cursor at the same end it was on when the selection was saved.
        let cursor_at_start = ctx.last_visual_cursor_at_start.unwrap_or(false);
        let cursor_pos = if cursor_at_start { start } else { end };
        effects = effects
            .set_mode(Mode::Visual(visual_type))
            .set_selection(start, end, SelectionShape::from(visual_type))
            .set_cursor(cursor_pos);

        // Emit curswant from the *unclamped* mark position.
        // Visual marks are not adjusted through text edits (Neovim preserves
        // the original column), so the mark's byte offset may exceed the
        // current line length. We compute the vcol allowing for past-end offsets.
        let mark_for_curswant = if cursor_at_start { raw_start } else { raw_end };
        let curswant = compute_unclamped_curswant(ctx.text, mark_for_curswant.get(), ctx.tabstop);
        effects.push(crate::effects::Effect::SetStickyColumn {
            column: Some(crate::primitives::VirtualColumn::new(curswant)),
        });

        CommandResult::effects_only(effects)
    } else {
        CommandResult::none()
    }
}

/// Compute the end offset for linewise `gv` reselection.
///
/// Starting from `start_offset`, finds the start of the Nth line (where N = `lines`).
/// Returns column 0 of the last selected line -- Neovim positions the cursor there
/// for linewise selections, and linewise expansion handles the rest.
fn compute_linewise_end(text: &str, start_offset: usize, lines: usize) -> Offset {
    use crate::commands::helpers::line_of;

    let clamped = if start_offset >= text.len() && !text.is_empty() {
        crate::commands::helpers::prev_char_boundary(text, text.len())
    } else {
        start_offset
    };
    let start_line = line_of(text, clamped);
    let target_line = start_line + lines.saturating_sub(1);

    let mut current_line = 0;
    let mut line_start = 0;
    for (i, c) in text.char_indices() {
        if current_line == target_line {
            return Offset::new(line_start);
        }
        if c == '\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    // Past end of text -- return last line start
    if current_line == target_line {
        return Offset::new(line_start);
    }
    Offset::new(line_start)
}

/// Compute curswant (virtual column) for a byte offset that may exceed the
/// current line length.
///
/// Visual marks `<`/`>` are not adjusted through text edits (Neovim behavior),
/// so after a change that shortens a line, the mark's byte offset can be past
/// the current line end. We compute:
///   - `byte_to_vcol(line, line_len, tabstop)` for the in-bounds portion
///   - `+ (col_byte - line_len)` for the extra bytes past the line end
///
/// When the offset is within the line, this is equivalent to `curswant_of`.
fn compute_unclamped_curswant(text: &str, offset: usize, tabstop: usize) -> usize {
    use crate::commands::helpers::{byte_to_vcol, line_start_for_offset};

    let clamped_for_line = offset.min(text.len());
    let ls = line_start_for_offset(text, clamped_for_line);
    let col_byte = offset.saturating_sub(ls);

    let line = text
        .get(ls..)
        .and_then(|s| s.split('\n').next())
        .unwrap_or("");
    let line_len = line.len();

    if col_byte <= line_len {
        if line.as_bytes().get(col_byte) == Some(&b'\t') {
            byte_to_vcol(line, col_byte + 1, tabstop).saturating_sub(1)
        } else {
            byte_to_vcol(line, col_byte, tabstop)
        }
    } else {
        // Offset is past line end -- vcol of line end + extra bytes.
        byte_to_vcol(line, line_len, tabstop) + (col_byte - line_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Offset, SelectionRange};

    #[test]
    fn test_enter_visual() {
        let ctx = VisualContext::new("", Offset::new(5), None);
        let result = enter(&ctx, VisualType::Char, None);
        assert_eq!(result.effects.len(), 3); // SetMode + SetSelection + SetCursor
    }

    #[test]
    fn test_exit_visual() {
        // Without selection: just ClearSelection + SetMode
        let ctx = VisualContext::new("", Offset::new(5), None);
        let result = exit(&ctx);
        assert_eq!(result.effects.len(), 2);

        // With selection: SetMark('<') + SetMark('>') + ClearSelection + SetMode
        let selection = SelectionRange::new(Offset::new(5), Offset::new(10));
        let ctx = VisualContext::new("", Offset::new(10), Some(selection));
        let result = exit(&ctx);
        assert_eq!(result.effects.len(), 4);
    }

    #[test]
    fn test_swap_ends() {
        let selection = SelectionRange::new(Offset::new(5), Offset::new(10));
        let ctx = VisualContext::new("", Offset::new(10), Some(selection));
        let result = swap_ends(&ctx);
        assert_eq!(result.effects.len(), 2);
    }

    #[test]
    fn test_reselect() {
        let ctx = VisualContext::new("", Offset::new(0), None).with_last_visual(
            VisualType::Line,
            Offset::new(5),
            Offset::new(15),
        );
        let result = reselect(&ctx);
        // SetMode + SetSelection + SetCursor + SetStickyColumn
        assert_eq!(result.effects.len(), 4);
    }

    #[test]
    fn test_unclamped_curswant_within_line() {
        // Offset within line: should behave like normal curswant_of
        assert_eq!(compute_unclamped_curswant("hello", 3, 8), 3);
        assert_eq!(compute_unclamped_curswant("hello", 0, 8), 0);
        assert_eq!(compute_unclamped_curswant("hello", 5, 8), 5);
    }

    #[test]
    fn test_unclamped_curswant_past_line_end() {
        // Offset past line end: should return vcol(line_end) + extra bytes
        assert_eq!(compute_unclamped_curswant("Xorld", 6, 8), 6);
        assert_eq!(compute_unclamped_curswant("orld", 6, 8), 6);
        assert_eq!(compute_unclamped_curswant("ab", 5, 8), 5);
    }
}
