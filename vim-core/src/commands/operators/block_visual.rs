//! Block visual operator commands (Ctrl-V + d/y/c/~/>/< etc).
//!
//! Pure command logic for operators applied to block visual selections.
//! Works on specific grapheme columns across multiple lines, not contiguous bytes.
//!
//! # Behavior
//!
//! - `Ctrl-V` + `d` — Delete block column
//! - `Ctrl-V` + `y` — Yank block column
//! - `Ctrl-V` + `c` — Change block column (enter insert)
//! - `Ctrl-V` + `~`/`u`/`U` — Case change block column
//! - `Ctrl-V` + `>`/`<` — Indent/outdent lines in block
//!
//! Plain functions, not trait methods.

use crate::commands::helpers::{
    byte_to_vcol, compute_block_geometry, line_end, line_start, vcol_to_byte,
};
use crate::commands::operators::registers;
use crate::commands::operators::types::OperatorContext;
use crate::effects::{Effect, Effects};
use crate::grammar::types::Operator;
use crate::primitives::byte_delta;
use crate::primitives::{InsertEntryType, MotionType, Offset, Range, SelectionRange};
use crate::primitives::{LastVisualInfo, Mode, VisualType};
/// Result type follows operator conventions.
pub type BlockCommandResult = crate::commands::CommandResult;

/// Execute an operator on a block visual selection.
///
/// Block visual operations work on SPECIFIC COLUMNS across MULTIPLE LINES:
/// 1. Compute grapheme column boundaries from anchor/head
/// 2. For each line, compute byte range for those columns
/// 3. Extract per-line text for register, delete bottom-to-top
///
/// # Arguments
/// * `operator` - The operator variant from the dispatch/executor match
/// * `ctx` - Standard operator context (text, range, register)
/// * `selection` - Block visual selection (anchor/head positions)
pub fn execute_block_operator(
    operator: Operator,
    ctx: &OperatorContext<'_>,
    selection: &SelectionRange,
) -> BlockCommandResult {
    let text = ctx.text;
    let geo = compute_block_geometry(text, selection.anchor(), selection.head(), ctx.tabstop);

    // Collect per-line ranges and register text
    let (line_ranges, register_text, pad_info) = collect_line_ranges(
        text,
        geo.top_line,
        geo.bot_line,
        geo.left_vcol,
        geo.right_vcol,
        ctx.virtualedit_block,
        ctx.tabstop,
    );

    // Compute cursor position (top-left corner of the block)
    let cursor_pos = compute_cursor_pos(text, geo.top_line, geo.left_vcol, ctx.tabstop);

    // Pre-set visual marks BEFORE operator effects so that text-modifying
    // operators (delete, change) trigger adjust_offsets and shift marks correctly.
    // This matches the non-block operator path in dispatch_operator_selection.
    let pre_effects = build_pre_exit_effects(
        selection,
        geo.top_line,
        geo.bot_line,
        geo.left_gcol,
        geo.right_gcol,
    );

    let op_effects = match operator {
        Operator::Delete => {
            let mut effects = Effects::new().begin_undo();
            effects = route_block_delete_registers(effects, &register_text, ctx);
            effects = delete_block_ranges(effects, &line_ranges);
            effects.end_undo()
        }
        Operator::Change => {
            // Undo group left OPEN — exit_finalize() closes it so the delete
            // and typed text form a single undoable atom (matching change.rs).
            let mut effects = Effects::new().begin_undo();
            effects = route_block_delete_registers(effects, &register_text, ctx);
            delete_block_ranges(effects, &line_ranges).into_raw_closed()
        }
        Operator::Yank => {
            let mut effects = route_block_yank_registers(Effects::new(), &register_text, ctx);

            // Emit yank highlight for the block region.
            // Use the first line-range start and last line-range end to form
            // the bounding-box byte range; the shape=Block tells the host
            // to render equal-width rectangles on every line.
            if !line_ranges.is_empty() {
                let first_start = line_ranges.first().map_or(0, |r| r.0);
                let last_end = line_ranges.last().map_or(0, |r| r.1);
                effects.push(Effect::SetHighlightRange {
                    owner: compact_str::CompactString::from(crate::effects::HIGHLIGHT_OWNER_YANK),
                    range: Range::from_raw(first_start, last_end),
                    group: compact_str::CompactString::new_inline("yank"),
                    shape: crate::primitives::SelectionShape::Block,
                });
            }

            effects
        }
        Operator::ToggleCase | Operator::Uppercase | Operator::Lowercase | Operator::Rot13 => {
            apply_case_change(text, operator, &line_ranges)
        }
        Operator::Indent => apply_indent(
            text,
            geo.top_line,
            geo.bot_line,
            geo.left_vcol,
            ctx.shiftwidth,
            ctx.tabstop,
        ),
        Operator::Outdent => apply_outdent(
            text,
            geo.top_line,
            geo.bot_line,
            geo.left_vcol,
            ctx.shiftwidth,
            ctx.tabstop,
        ),
        _ => {
            // Unsupported operators: register + exit only
            route_block_yank_registers(Effects::new(), &register_text, ctx)
        }
    };

    // Build: pre_effects (marks) → pad (if ve=block) → op_effects → post_effects
    let mut effects = pre_effects;

    // Emit virtualedit=block padding bottom-to-top (higher offsets first) so
    // each insert doesn't shift the offsets of subsequent inserts.
    if !pad_info.is_empty() {
        for pad in pad_info.iter().rev() {
            effects.push(Effect::Insert {
                offset: Offset::new(pad.insert_at),
                text: compact_str::CompactString::from(" ".repeat(pad.pad_count)),
            });
        }
    }

    effects.extend(op_effects);
    effects.extend(build_post_exit_effects(
        operator,
        cursor_pos,
        geo.top_line,
        geo.bot_line,
        geo.left_gcol,
    ));

    // Neovim sets mark '.' to the top-left corner of a block visual operation.
    // sync_change_marks records the start of the FIRST edit (bottom line for
    // reverse-order deletions), which is wrong. Override with cursor_pos for
    // text-mutating operators. Change enters insert mode, so mark '.' will be
    // set later during insert exit; skip it here to avoid clobbering.
    if operator != Operator::Change && operator != Operator::Yank {
        effects = effects.set_mark(crate::primitives::MarkName::LAST_CHANGE, cursor_pos, None);
    }

    // ── Change marks '[' and ']' for block visual operations ───────────
    //
    // sync_change_marks processes bottom-to-top deletions and ends up with
    // `]` pointing at the topmost edit (lowest offset). Neovim expects:
    //   Delete/Change: `]` = start of bottom-most affected line (post-edit)
    //   Yank:          `]` = bottom-right corner of block (pre-edit)
    //   `[` = cursor_pos (top-left) in all cases
    use crate::primitives::MarkName;
    match operator {
        Operator::Delete => {
            if !line_ranges.is_empty() {
                // Post-edit bottom line start = original bottom start minus
                // total bytes deleted on all lines ABOVE the bottom one.
                // Neovim's op_delete for block mode sets b_op_end.col =
                // oap->start.col (the left column of the block). Add the
                // column byte offset so mark `]` points to the correct
                // character on the bottom line.
                let original_bot_start = line_ranges
                    .last()
                    .map_or(0, |r| line_start(text, geo.bot_line).unwrap_or(r.0));
                let bytes_deleted_above: usize = line_ranges
                    .iter()
                    .take(line_ranges.len().saturating_sub(1))
                    .map(|&(s, e)| e.saturating_sub(s))
                    .sum();
                let post_edit_bot_line_start =
                    original_bot_start.saturating_sub(bytes_deleted_above);
                // Compute the byte offset of left_vcol on the bottom line
                // in post-edit coordinates. The bottom line text starts at
                // post_edit_bot_line_start in the post-edit document.
                // Since the column is the same as on the top line (block),
                // use vcol_to_byte for tab-aware conversion.
                let col_byte_offset = {
                    let bot_line_start = line_start(text, geo.bot_line).unwrap_or(0);
                    let bot_line_end = line_end(text, geo.bot_line).unwrap_or(text.len());
                    let bot_line_text = &text[bot_line_start..bot_line_end];
                    vcol_to_byte(bot_line_text, geo.left_vcol, ctx.tabstop)
                };
                let mark_end = Offset::new(post_edit_bot_line_start + col_byte_offset);
                effects = effects.set_mark(MarkName::CHANGE_END, mark_end, None);
                effects = effects.set_mark(MarkName::CHANGE_START, cursor_pos, None);
            }
        }
        Operator::Yank => {
            // `]` = bottom-right of the block in pre-edit coordinates.
            if let Some(&(_start, end)) = line_ranges.last() {
                let mark_end = Offset::new(end.saturating_sub(1));
                effects = effects.set_mark(MarkName::CHANGE_END, mark_end, None);
                effects = effects.set_mark(MarkName::CHANGE_START, cursor_pos, None);
            }
        }
        Operator::Indent | Operator::Outdent => {
            // Neovim's op_shift() sets b_op_end to last byte of last affected line
            // in the POST-edit text. Compute the cumulative byte delta from all
            // per-line shifts, then apply to the bottom line's position.
            let sw = ctx.shiftwidth;
            let bot_line_start = line_start(text, geo.bot_line).unwrap_or(0);
            let bot_line_end = line_end(text, geo.bot_line).unwrap_or(text.len());
            let bot_line_text = &text[bot_line_start..bot_line_end];
            let mut total_delta: isize = 0;
            for l in geo.top_line..=geo.bot_line {
                let ls = line_start(text, l).unwrap_or(0);
                let le = line_end(text, l).unwrap_or(text.len());
                let line_text = &text[ls..le];
                if line_text.is_empty() {
                    continue;
                }
                let col_byte = vcol_to_byte(line_text, geo.left_vcol, ctx.tabstop);
                if col_byte >= line_text.len() && geo.left_vcol > 0 {
                    continue;
                }
                if operator == Operator::Indent {
                    total_delta += byte_delta::to_isize(sw);
                } else {
                    let removable = line_text[col_byte..]
                        .bytes()
                        .take(sw)
                        .take_while(|&b| b == b' ')
                        .count();
                    total_delta -= byte_delta::to_isize(removable);
                }
            }
            // Bottom line's new length: original len + its own delta
            let bot_col_byte = vcol_to_byte(bot_line_text, geo.left_vcol, ctx.tabstop);
            let bot_reachable = !bot_line_text.is_empty()
                && (bot_col_byte < bot_line_text.len() || geo.left_vcol == 0);
            let bot_own_delta: isize = if !bot_reachable {
                0
            } else if operator == Operator::Indent {
                byte_delta::to_isize(sw)
            } else {
                let removable = bot_line_text[bot_col_byte..]
                    .bytes()
                    .take(sw)
                    .take_while(|&b| b == b' ')
                    .count();
                -byte_delta::to_isize(removable)
            };
            // Delta from lines ABOVE the bottom line
            let delta_above = total_delta - bot_own_delta;
            let new_bot_start = bot_line_start.saturating_add_signed(delta_above);
            let new_bot_len = bot_line_text.len().saturating_add_signed(bot_own_delta);
            let mark_end_val = if new_bot_len > 0 {
                new_bot_start + new_bot_len - 1
            } else {
                new_bot_start
            };
            effects = effects.set_mark(MarkName::CHANGE_END, Offset::new(mark_end_val), None);
            effects = effects.set_mark(MarkName::CHANGE_START, cursor_pos, None);
        }
        _ => {}
    }

    BlockCommandResult::effects_only(effects)
}

// =============================================================================
// Geometry
// =============================================================================

/// Padding needed for a short line in virtualedit=block mode.
#[derive(Debug, Clone)]
struct LinePad {
    /// Byte offset where to insert padding (end of line content).
    insert_at: usize,
    /// Number of spaces to insert.
    pad_count: usize,
}

/// Collect per-line byte ranges and register text for the block.
///
/// Uses virtual columns (`left_vcol`/`right_vcol`) with tab-aware and
/// wide-char-aware `vcol_to_byte()` conversion to compute per-line byte
/// ranges. This correctly handles lines containing tabs and wide characters
/// where grapheme column counts diverge from display column counts.
///
/// When `virtualedit_block` is true, short lines that don't reach the
/// block's column range are padded with spaces (persisted) so the block
/// selection can extend past line ends. The padding info is returned
/// separately so the caller can emit Insert effects bottom-to-top
/// before the operation effects.
fn collect_line_ranges(
    text: &str,
    top_line: usize,
    bottom_line: usize,
    left_vcol: usize,
    right_vcol: usize,
    virtualedit_block: bool,
    tabstop: usize,
) -> (Vec<(usize, usize)>, String, Vec<LinePad>) {
    let mut line_ranges: Vec<(usize, usize)> = Vec::new();
    let mut register_parts: Vec<String> = Vec::new();
    let mut pad_info: Vec<LinePad> = Vec::new();

    for line_idx in top_line..=bottom_line {
        let ls = line_start(text, line_idx).unwrap_or(0);
        let le = line_end(text, line_idx).unwrap_or(text.len());
        let line_text = &text[ls..le];

        // Compute the total display width of the line.
        let line_width = byte_to_vcol(line_text, line_text.len(), tabstop);

        // Byte offsets within the line for the left and right vcol boundaries.
        // `vcol_to_byte` returns the byte offset of the grapheme at or past
        // the target vcol. For the right boundary, we need the byte PAST the
        // last character in the block, so we target right_vcol + 1.
        let left_byte_in_line = vcol_to_byte(line_text, left_vcol, tabstop);
        let right_byte_in_line = vcol_to_byte(line_text, right_vcol + 1, tabstop);

        if left_byte_in_line >= right_byte_in_line || left_byte_in_line >= line_text.len() {
            // Line is too short or empty for this block column range.
            if virtualedit_block && line_width <= right_vcol {
                // Short line: needs padding to reach the block columns.
                let needed = (right_vcol + 1).saturating_sub(line_width);
                pad_info.push(LinePad {
                    insert_at: le,
                    pad_count: needed,
                });
                // Register gets spaces for the padded region within the block.
                let padded_left = left_vcol.saturating_sub(line_width);
                let padded_width = (right_vcol + 1).saturating_sub(left_vcol);
                let visible = padded_width.saturating_sub(padded_left);
                register_parts.push(" ".repeat(visible));
            } else {
                register_parts.push(String::new());
            }
            continue;
        }

        let left_byte = left_byte_in_line + ls;
        let right_byte = right_byte_in_line + ls;

        // Even if the line reaches the left column, it might be shorter
        // than the right column. Pad the remainder.
        if virtualedit_block && line_width <= right_vcol {
            let extra = (right_vcol + 1).saturating_sub(line_width);
            pad_info.push(LinePad {
                insert_at: le,
                pad_count: extra,
            });
        }

        line_ranges.push((left_byte, right_byte));
        register_parts.push(text[left_byte..right_byte].to_string());
    }

    (line_ranges, register_parts.join("\n"), pad_info)
}

/// Compute cursor position at the top-left corner of the block.
fn compute_cursor_pos(text: &str, top_line: usize, left_vcol: usize, tabstop: usize) -> Offset {
    let cursor_line_start = line_start(text, top_line).unwrap_or(0);
    let cursor_line_end = line_end(text, top_line).unwrap_or(text.len());
    let cursor_line_text = &text[cursor_line_start..cursor_line_end];
    let byte_offset = vcol_to_byte(cursor_line_text, left_vcol, tabstop) + cursor_line_start;
    Offset::new(byte_offset)
}

// =============================================================================
// Register Routing (delegates to registers.rs)
// =============================================================================

/// Route registers for block delete/change using the centralized routing.
fn route_block_delete_registers<S: crate::effects::undo_state::EffectState>(
    effects: Effects<S>,
    register_text: &str,
    ctx: &OperatorContext<'_>,
) -> Effects<S> {
    let is_multiline = register_text.contains('\n');
    registers::route_delete_registers(
        effects,
        register_text,
        MotionType::BlockWise,
        ctx.register,
        is_multiline,
        false,
    )
}

/// Route registers for block yank using the centralized routing.
fn route_block_yank_registers<S: crate::effects::undo_state::EffectState>(
    effects: Effects<S>,
    register_text: &str,
    ctx: &OperatorContext<'_>,
) -> Effects<S> {
    registers::route_yank_registers(effects, register_text, MotionType::BlockWise, ctx.register)
}

// =============================================================================
// Operator Implementations
// =============================================================================

/// Delete block ranges bottom-to-top to preserve byte offsets.
///
/// # INVARIANT: Reverse iteration order is critical.
///
/// Deleting from the bottom up ensures that byte offsets of earlier (higher)
/// ranges remain valid after each deletion. If we deleted top-to-bottom,
/// each deletion would shift all subsequent offsets, requiring recalculation.
/// The Effects builder queues deletions without applying them, but the shell
/// processes them in order — so emission order determines correctness.
fn delete_block_ranges<S: crate::effects::undo_state::EffectState>(
    mut effects: Effects<S>,
    line_ranges: &[(usize, usize)],
) -> Effects<S> {
    for &(start, end) in line_ranges.iter().rev() {
        effects.push(Effect::Delete {
            range: Range::from_raw(start, end),
        });
    }
    effects
}

/// Apply case changes to block columns.
fn apply_case_change(text: &str, operator: Operator, line_ranges: &[(usize, usize)]) -> Effects {
    let mut effects = Effects::new().begin_undo();

    for &(start, end) in line_ranges.iter().rev() {
        let original = &text[start..end];
        let mut replaced = String::with_capacity(original.len());
        for c in original.chars() {
            match operator {
                Operator::ToggleCase => {
                    if c.is_uppercase() {
                        replaced.extend(c.to_lowercase());
                    } else {
                        replaced.extend(c.to_uppercase());
                    }
                }
                Operator::Uppercase => replaced.extend(c.to_uppercase()),
                Operator::Lowercase => replaced.extend(c.to_lowercase()),
                Operator::Rot13 => {
                    let rotated = match c {
                        'a'..='z' => (b'a' + (c as u8 - b'a' + 13) % 26) as char,
                        'A'..='Z' => (b'A' + (c as u8 - b'A' + 13) % 26) as char,
                        _ => c,
                    };
                    replaced.push(rotated);
                }
                _ => replaced.push(c),
            }
        }
        if replaced != original {
            effects = effects.replace(Range::from_raw(start, end), replaced);
        }
    }

    effects.end_undo()
}

/// Apply indent to all lines in the block at the block's left column.
///
/// Neovim's `shift_block` for right-shift inserts `shiftwidth` spaces at
/// the block's start virtual column, not at column 0. This matches that
/// behavior: for each line, compute the byte offset of `left_vcol` and
/// insert spaces there. Lines shorter than the block column are skipped
/// (matching Neovim's `bd.is_short` early return).
fn apply_indent(
    text: &str,
    top_line: usize,
    bottom_line: usize,
    left_vcol: usize,
    shiftwidth: usize,
    tabstop: usize,
) -> Effects {
    let indent = " ".repeat(shiftwidth);
    let mut effects = Effects::new().begin_undo();

    for line_idx in (top_line..=bottom_line).rev() {
        let ls = line_start(text, line_idx).unwrap_or(0);
        let le = line_end(text, line_idx).unwrap_or(text.len());
        let line_text = &text[ls..le];

        // Skip empty lines (matches Neovim: empty line → cursor.col = 0, no shift)
        if line_text.is_empty() {
            continue;
        }

        // Compute the byte offset within the line for left_vcol.
        // If the line is shorter than the block column, vcol_to_byte returns
        // line_text.len() — skip these short lines (Neovim's bd.is_short).
        let insert_byte = vcol_to_byte(line_text, left_vcol, tabstop);
        if insert_byte >= line_text.len() && left_vcol > 0 {
            continue;
        }

        effects = effects.insert(Offset::new(ls + insert_byte), &indent);
    }

    effects.end_undo()
}

/// Apply outdent to all lines in the block at the block's left column.
///
/// Neovim's `shift_block` for left-shift removes whitespace starting from
/// the block's start virtual column, not from column 0. This matches that
/// behavior: for each line, find whitespace at and after `left_vcol`, then
/// remove up to `shiftwidth` columns worth of spaces. Lines shorter than
/// the block column are skipped.
fn apply_outdent(
    text: &str,
    top_line: usize,
    bottom_line: usize,
    left_vcol: usize,
    shiftwidth: usize,
    tabstop: usize,
) -> Effects {
    let mut effects = Effects::new().begin_undo();

    for line_idx in (top_line..=bottom_line).rev() {
        let ls = line_start(text, line_idx).unwrap_or(0);
        let le = line_end(text, line_idx).unwrap_or(text.len());
        let line_text = &text[ls..le];

        if line_text.is_empty() {
            continue;
        }

        // Find the byte offset of the block's left column on this line.
        let start_byte = vcol_to_byte(line_text, left_vcol, tabstop);
        if start_byte >= line_text.len() && left_vcol > 0 {
            continue;
        }

        // Count whitespace characters from the block column onwards,
        // up to shiftwidth spaces to remove.
        let spaces_to_remove = line_text[start_byte..]
            .chars()
            .take_while(|c| *c == ' ')
            .take(shiftwidth)
            .count();
        if spaces_to_remove > 0 {
            effects = effects.delete(Range::from_raw(
                ls + start_byte,
                ls + start_byte + spaces_to_remove,
            ));
        }
    }

    effects.end_undo()
}

// =============================================================================
// Visual Exit Effects
// =============================================================================

/// Build pre-operator visual exit effects: SaveLastVisual, marks, ClearSelection.
///
/// These must be emitted BEFORE operator effects (delete/change) so that
/// text-modifying operators adjust the visual marks through `adjust_offsets`.
#[allow(
    clippy::too_many_arguments,
    reason = "block visual exit requires full geometry context"
)]
fn build_pre_exit_effects(
    selection: &SelectionRange,
    top_line: usize,
    bottom_line: usize,
    left_gcol: usize,
    right_gcol: usize,
) -> Effects {
    // SaveLastVisual
    let info = LastVisualInfo::new(
        VisualType::Block,
        bottom_line - top_line + 1,
        right_gcol - left_gcol,
    );

    // Selection marks '<  and '>
    let (mark_start, mark_end) = if selection.is_forward() {
        (selection.anchor(), selection.head())
    } else {
        (selection.head(), selection.anchor())
    };

    Effects::new()
        .save_last_visual(info)
        .set_mark(crate::primitives::MarkName::VISUAL_START, mark_start, None)
        .set_mark(crate::primitives::MarkName::VISUAL_END, mark_end, None)
        .clear_selection()
}

/// Build post-operator visual exit effects: mode switch and cursor positioning.
fn build_post_exit_effects(
    operator: Operator,
    cursor_pos: Offset,
    top_line: usize,
    bottom_line: usize,
    left_gcol: usize,
) -> Effects {
    let mut effects = Effects::new();

    // Mode switch: Change enters insert via begin_insert (preserves undo
    // grouping so the delete + typed text form a single undoable atom),
    // others return to normal.
    //
    // The undo group is already open from the caller (Operator::Change arm),
    // so we push the BeginInsert effect directly rather than going through
    // the typed begin_insert() method which requires UndoOpen state.
    if operator == Operator::Change {
        effects.push(Effect::BeginInsert {
            entry_type: InsertEntryType::ChangeOperator,
            count: 1,
            auto_indent_len: 0,
            entry_offset: cursor_pos,
        });
        let lines_below = bottom_line - top_line;
        if lines_below > 0 {
            effects = effects.set_block_insert(lines_below, left_gcol, cursor_pos);
        }
    } else {
        effects = effects.set_mode(Mode::Normal);
    }

    effects.set_cursor(cursor_pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{MotionType, Offset, RegisterName};

    fn make_ctx(text: &str) -> OperatorContext<'_> {
        OperatorContext::new(
            text,
            Range::EMPTY,
            MotionType::BlockWise,
            None,
            1,
            Offset::new(0),
        )
    }

    #[test]
    fn test_block_delete_produces_delete_effects() {
        let text = "hello\nworld";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(6));
        let ctx = make_ctx(text);
        let result = execute_block_operator_with_op(Operator::Delete, &ctx, &sel);

        let has_delete = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Delete { .. }));
        assert!(has_delete, "Block delete must produce Delete effects");

        let has_undo = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginUndoGroup { .. }));
        assert!(has_undo, "Block delete must wrap in undo group");
    }

    #[test]
    fn test_block_yank_no_undo_group() {
        let text = "hello\nworld";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(6));
        let ctx = make_ctx(text);
        let result = execute_block_operator_with_op(Operator::Yank, &ctx, &sel);

        let has_undo = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginUndoGroup { .. }));
        assert!(!has_undo, "Block yank must NOT wrap in undo group");

        let has_register = result.effects.iter().any(
            |e| matches!(e, Effect::SetRegister { name, .. } if *name == RegisterName::UNNAMED),
        );
        assert!(has_register, "Block yank must set unnamed register");
    }

    #[test]
    fn test_block_change_enters_insert_mode() {
        let text = "hello\nworld";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(6));
        let ctx = make_ctx(text);
        let result = execute_block_operator_with_op(Operator::Change, &ctx, &sel);

        let has_begin_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginInsert { .. }));
        assert!(
            has_begin_insert,
            "Block change must enter Insert mode via BeginInsert"
        );
    }

    #[test]
    fn test_block_toggle_case_produces_replace() {
        let text = "Hello\nWorld";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(6));
        let ctx = make_ctx(text);
        let result = execute_block_operator_with_op(Operator::ToggleCase, &ctx, &sel);

        let has_replace = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Replace { .. }));
        assert!(
            has_replace,
            "Block toggle case must produce Replace effects"
        );
    }

    #[test]
    fn test_block_indent_inserts_spaces() {
        let text = "hello\nworld";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(6));
        let ctx = make_ctx(text);
        let result = execute_block_operator_with_op(Operator::Indent, &ctx, &sel);

        let insert_count = result
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::Insert { .. }))
            .count();
        assert_eq!(
            insert_count, 2,
            "Block indent on 2 lines must produce 2 Insert effects"
        );
    }

    #[test]
    fn test_all_block_ops_clear_selection() {
        let text = "hello\nworld";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(6));
        let ctx = make_ctx(text);

        for op in [
            Operator::Delete,
            Operator::Yank,
            Operator::Change,
            Operator::ToggleCase,
        ] {
            let result = execute_block_operator_with_op(op, &ctx, &sel);
            let has_clear = result
                .effects
                .iter()
                .any(|e| matches!(e, Effect::ClearSelection));
            assert!(has_clear, "{:?} must produce ClearSelection", op);
        }
    }

    #[test]
    fn test_all_block_ops_save_last_visual() {
        let text = "hello\nworld";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(6));
        let ctx = make_ctx(text);

        let result = execute_block_operator_with_op(Operator::Delete, &ctx, &sel);
        let has_save = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SaveLastVisual { .. }));
        assert!(has_save, "Block op must produce SaveLastVisual");
    }

    /// Test helper: calls execute_block_operator with an explicit operator.
    fn execute_block_operator_with_op(
        op: Operator,
        ctx: &OperatorContext<'_>,
        selection: &SelectionRange,
    ) -> BlockCommandResult {
        let text = ctx.text;
        let geo = compute_block_geometry(text, selection.anchor(), selection.head(), ctx.tabstop);
        let (line_ranges, register_text, _pad_info) = collect_line_ranges(
            text,
            geo.top_line,
            geo.bot_line,
            geo.left_vcol,
            geo.right_vcol,
            ctx.virtualedit_block,
            ctx.tabstop,
        );
        let cursor_pos = compute_cursor_pos(text, geo.top_line, geo.left_vcol, ctx.tabstop);

        let effects = match op {
            Operator::Delete => {
                let mut effects = Effects::new().begin_undo();
                effects = route_block_delete_registers(effects, &register_text, ctx);
                effects = delete_block_ranges(effects, &line_ranges);
                effects.end_undo()
            }
            Operator::Change => {
                // Undo group left OPEN — exit_finalize() closes it so the delete
                // and typed text form a single undoable atom (matching change.rs).
                let mut effects = Effects::new().begin_undo();
                effects = route_block_delete_registers(effects, &register_text, ctx);
                delete_block_ranges(effects, &line_ranges).into_raw_closed()
            }
            Operator::Yank => route_block_yank_registers(Effects::new(), &register_text, ctx),
            Operator::ToggleCase | Operator::Uppercase | Operator::Lowercase | Operator::Rot13 => {
                apply_case_change(text, op, &line_ranges)
            }
            Operator::Indent => apply_indent(
                text,
                geo.top_line,
                geo.bot_line,
                geo.left_vcol,
                ctx.shiftwidth,
                ctx.tabstop,
            ),
            Operator::Outdent => apply_outdent(
                text,
                geo.top_line,
                geo.bot_line,
                geo.left_vcol,
                ctx.shiftwidth,
                ctx.tabstop,
            ),
            _ => route_block_yank_registers(Effects::new(), &register_text, ctx),
        };

        let pre = build_pre_exit_effects(
            selection,
            geo.top_line,
            geo.bot_line,
            geo.left_gcol,
            geo.right_gcol,
        );
        let mut all_effects = pre;
        all_effects.extend(effects);
        all_effects.extend(build_post_exit_effects(
            op,
            cursor_pos,
            geo.top_line,
            geo.bot_line,
            geo.left_gcol,
        ));

        BlockCommandResult::effects_only(all_effects)
    }

    // ── Tab-aware collect_line_ranges tests ──────────────────────────

    #[test]
    fn test_collect_line_ranges_plain_ascii() {
        // "hello\nworld" — select cols 1..=3 on both lines ("ell" / "orl")
        let text = "hello\nworld";
        let (ranges, reg, _pad) = collect_line_ranges(text, 0, 1, 1, 3, false, 8);
        assert_eq!(ranges.len(), 2);
        // Line 0: bytes 1..4 ("ell")
        assert_eq!(ranges[0], (1, 4));
        // Line 1: bytes 7..10 ("orl")
        assert_eq!(ranges[1], (7, 10));
        assert_eq!(reg, "ell\norl");
    }

    #[test]
    fn test_collect_line_ranges_with_tabs() {
        // Line 0: "\thello" — tab expands to 8 cols (tabstop=8)
        // Line 1: "\tworld"
        // Select vcols 8..=12 (the "hello"/"world" after the tab)
        let text = "\thello\n\tworld";
        let (ranges, reg, _pad) = collect_line_ranges(text, 0, 1, 8, 12, false, 8);
        assert_eq!(ranges.len(), 2);
        // Line 0: bytes 1..6 ("hello"), line starts at 0
        assert_eq!(ranges[0], (1, 6));
        // Line 1: bytes 8..13 ("world"), line starts at 7
        assert_eq!(ranges[1], (8, 13));
        assert_eq!(reg, "hello\nworld");
    }

    #[test]
    fn test_collect_line_ranges_tab_vs_spaces_aligned() {
        // Line 0: "\tx" — tab(8) + 'x': 'x' at vcol 8
        // Line 1: "        x" — 8 spaces + 'x': 'x' at vcol 8
        // Select vcol 8..=8 (just the 'x' on each line)
        let text = "\tx\n        x";
        let (ranges, reg, _pad) = collect_line_ranges(text, 0, 1, 8, 8, false, 8);
        assert_eq!(ranges.len(), 2);
        // Line 0: 'x' at byte 1..2
        assert_eq!(ranges[0], (1, 2));
        // Line 1: 'x' at byte 11..12 (line starts at 3, 8 spaces + x)
        assert_eq!(ranges[1], (11, 12));
        assert_eq!(reg, "x\nx");
    }

    #[test]
    fn test_collect_line_ranges_tabstop_4() {
        // Line 0: "\thello" with tabstop=4: tab → 4 cols
        // Select vcols 4..=8 ("hello")
        let text = "\thello\n\tworld";
        let (ranges, reg, _pad) = collect_line_ranges(text, 0, 1, 4, 8, false, 4);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (1, 6)); // "hello"
        assert_eq!(ranges[1], (8, 13)); // "world"
        assert_eq!(reg, "hello\nworld");
    }

    #[test]
    fn test_collect_line_ranges_short_line_no_ve() {
        // Line 0: "hi" (vcol width = 2)
        // Line 1: "hello" (vcol width = 5)
        // Select vcols 3..=4 — line 0 too short, line 1 has "lo"
        let text = "hi\nhello";
        let (ranges, reg, _pad) = collect_line_ranges(text, 0, 1, 3, 4, false, 8);
        // Line 0 should be skipped (too short), line 1 has bytes 6..8 ("lo")
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], (6, 8));
        assert_eq!(reg, "\nlo");
    }

    #[test]
    fn test_compute_cursor_pos_with_tab() {
        // "\thello" — cursor at vcol 8 should be at byte 1 (after tab)
        let text = "\thello";
        let pos = compute_cursor_pos(text, 0, 8, 8);
        assert_eq!(pos.get(), 1);
    }

    #[test]
    fn test_compute_cursor_pos_plain() {
        let text = "hello\nworld";
        // vcol 2 in plain ASCII = byte 2
        let pos = compute_cursor_pos(text, 0, 2, 8);
        assert_eq!(pos.get(), 2);
    }
}
