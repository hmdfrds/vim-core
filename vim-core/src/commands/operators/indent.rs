//! Indent operators (> and <).
//!
//! Shifts lines left or right by shiftwidth.
//!
//! # Behavior
//!
//! - `>>` - Indent current line
//! - `>j` - Indent 2 lines
//! - `<<` - Outdent current line
//! - `<3j` - Outdent 4 lines
//!
//! # Per-line effects
//!
//! Neovim's `op_shift()` processes each line individually with per-line
//! inserts/deletes.  Cursor and mark adjustments behave differently for
//! Replace vs Insert effects:
//!
//! - **Replace**: cursor/marks inside the replaced region get clamped
//! - **Per-line Insert**: cursor/marks shift naturally as spaces are
//!   inserted at each line start
//!
//! We emit per-line Insert (indent-right) or Delete/Replace (indent-left)
//! effects, processed top-to-bottom with cumulative offset tracking so that
//! the effect processor's automatic mark computation gives correct `[` and
//! `]` marks.

use super::types::{OperatorContext, ShiftDirection};
use crate::commands::helpers::{
    curswant_of, expand_tabs_into, line_start_for_offset, vcol_to_byte,
};
use crate::commands::CommandResult;
use crate::effects::{Effect, Effects};
use crate::primitives::{MarkName, Offset};
use compact_str::CompactString;

/// Apply a signed delta to a usize offset, clamping to 0 instead of wrapping
/// to a huge value when the result would be negative.
#[inline]
fn offset_by_delta(base: usize, delta: isize) -> usize {
    base.checked_add_signed(delta).unwrap_or(0)
}

/// Execute indent operator (>).
///
/// No traits, just functions + enum dispatch.
pub fn execute_indent(ctx: &OperatorContext<'_>) -> CommandResult {
    execute_shift(ctx, ShiftDirection::Right)
}

/// Execute outdent operator (<).
///
/// No traits, just functions + enum dispatch.
pub fn execute_outdent(ctx: &OperatorContext<'_>) -> CommandResult {
    execute_shift(ctx, ShiftDirection::Left)
}

/// Execute shift operation using per-line Insert/Delete/Replace effects.
///
/// Effects are emitted top-to-bottom with cumulative offset tracking so the
/// effect processor's automatic `[`/`]` mark computation works correctly:
/// `[` = start of first line, `]` = end of last line's edit.
fn execute_shift(ctx: &OperatorContext<'_>, direction: ShiftDirection) -> CommandResult {
    if ctx.text.is_empty() {
        // Neovim always sets `[` and `]` marks, even when the range is empty
        // (e.g. `>>` on an empty buffer). See op_shift() in ops.c.
        use crate::primitives::MarkName;
        let effects = Effects::new()
            .set_mark(MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(MarkName::CHANGE_END, ctx.cursor, None)
            .set_cursor(ctx.cursor);
        return CommandResult::new(effects, ctx.cursor);
    }

    let text = ctx.text;
    let range = ctx.range;

    let shiftwidth = ctx.shiftwidth;
    let expandtab = ctx.expandtab;
    let tabstop = ctx.tabstop;
    // Count is used for line range selection, NOT indent multiplier.
    // `2>>` indents 2 lines by shiftwidth, not 1 line by 2*shiftwidth.
    let total_shift = shiftwidth;

    // Find line boundaries within range
    let clamped = range.clamp_end(Offset::new(text.len()));
    let mut start_offset = clamped.start().get();
    let mut end_offset = clamped.end().get();

    // When extend_to_full_lines does EOF adjustment, start may point at the
    // preceding '\n' (e.g., offset 5 = '\n' in "line1\n  line2\nline3").
    // Advance past that newline so we don't accidentally include the prior line.
    // But don't skip if this '\n' IS the start of an empty line (preceded by
    // another '\n' or at the very beginning of the buffer).
    if text.as_bytes().get(start_offset) == Some(&b'\n')
        && start_offset > 0
        && text.as_bytes().get(start_offset - 1) != Some(&b'\n')
    {
        start_offset += 1;
    }
    if end_offset <= start_offset && !text.is_empty() {
        end_offset = text[start_offset..]
            .find('\n')
            .map_or(text.len(), |p| start_offset + p + 1);
    }
    let first_line_start = line_start_for_offset(text, start_offset);

    // Collect per-line effects, processing top-to-bottom.
    // `delta` tracks the cumulative byte offset shift from prior effects.
    //
    // Build the indent string based on expandtab: spaces or minimal tabs+spaces.
    let indent_str = if expandtab {
        " ".repeat(total_shift)
    } else {
        let tabs = total_shift / tabstop;
        let spaces = total_shift % tabstop;
        let mut s = "\t".repeat(tabs);
        s.push_str(&" ".repeat(spaces));
        s
    };
    let mut effects = Effects::new().begin_undo();
    let mut delta: isize = 0;
    let mut expanded_buf = String::new();
    let mut current_offset = first_line_start;

    // Track the first line's new content length for cursor placement
    let mut first_line_new_len: Option<usize> = None;
    // Track every processed line's adjusted start + new length for mark.] computation.
    let mut last_line_adjusted_start: usize = first_line_start;
    let mut last_line_new_len: usize = 0;

    for line in text[first_line_start..].lines() {
        if current_offset >= end_offset {
            break;
        }
        let line_start = current_offset;
        let line_end = line_start + line.len(); // exclusive, before '\n'

        match direction {
            ShiftDirection::Right => {
                // Vim doesn't indent empty lines
                if line.is_empty() {
                    // Empty line: no effect, but track first line length
                    if first_line_new_len.is_none() {
                        first_line_new_len = Some(0);
                    }
                    last_line_adjusted_start = offset_by_delta(line_start, delta);
                    last_line_new_len = 0;
                } else {
                    let has_tabs = line.contains('\t');
                    if has_tabs && expandtab {
                        // Line has tabs AND expandtab is on: Replace entire
                        // line content with expanded (tabs -> spaces) + indent.
                        expanded_buf.clear();
                        expand_tabs_into(line, shiftwidth, &mut expanded_buf);
                        let new_content = format!("{indent_str}{expanded_buf}");
                        let new_len = new_content.len();
                        let adjusted_start = offset_by_delta(line_start, delta);
                        let adjusted_end = offset_by_delta(line_end, delta);
                        let replace_range =
                            crate::primitives::Range::from_raw(adjusted_start, adjusted_end);
                        effects.push(Effect::replace(
                            replace_range,
                            CompactString::new(&new_content),
                        ));
                        if first_line_new_len.is_none() {
                            first_line_new_len = Some(new_len);
                        }
                        last_line_adjusted_start = adjusted_start;
                        last_line_new_len = new_len;
                        delta += new_len.cast_signed() - line.len().cast_signed();
                    } else {
                        // No tab expansion needed: insert indent at line start.
                        let adjusted_start = offset_by_delta(line_start, delta);
                        let indent_len = indent_str.len();
                        effects.push(Effect::insert(
                            Offset::new(adjusted_start),
                            CompactString::new(&indent_str),
                        ));
                        let new_len = line.len() + indent_len;
                        if first_line_new_len.is_none() {
                            first_line_new_len = Some(new_len);
                        }
                        last_line_adjusted_start = adjusted_start;
                        last_line_new_len = new_len;
                        delta += indent_len.cast_signed();
                    }
                }
            }
            ShiftDirection::Left => {
                // Compute how many leading bytes to remove. Expand tabs to
                // virtual columns to figure out how many characters cover
                // `total_shift` columns of leading whitespace.
                expanded_buf.clear();
                expand_tabs_into(line, tabstop, &mut expanded_buf);
                let spaces_to_remove = expanded_buf
                    .bytes()
                    .take(total_shift)
                    .take_while(|&b| b == b' ')
                    .count();

                let has_tabs = line.contains('\t');
                if has_tabs && expandtab {
                    // Line has tabs AND expandtab: Replace entire line with expanded + outdented
                    let new_content = &expanded_buf[spaces_to_remove..];
                    let new_len = new_content.len();
                    let adjusted_start = offset_by_delta(line_start, delta);
                    let adjusted_end = offset_by_delta(line_end, delta);
                    let replace_range =
                        crate::primitives::Range::from_raw(adjusted_start, adjusted_end);
                    effects.push(Effect::replace(
                        replace_range,
                        CompactString::new(new_content),
                    ));
                    if first_line_new_len.is_none() {
                        first_line_new_len = Some(new_len);
                    }
                    last_line_adjusted_start = adjusted_start;
                    last_line_new_len = new_len;
                    delta += new_len.cast_signed() - line.len().cast_signed();
                } else if has_tabs {
                    // Line has tabs, noexpandtab: remove leading whitespace
                    // bytes covering `total_shift` virtual columns, preserving
                    // remaining tab characters.
                    let mut vcol: usize = 0;
                    let mut bytes_to_remove: usize = 0;
                    for ch in line.chars() {
                        if vcol >= total_shift {
                            break;
                        }
                        match ch {
                            '\t' => {
                                let next = (vcol / tabstop + 1) * tabstop;
                                if next <= total_shift {
                                    vcol = next;
                                    bytes_to_remove += 1;
                                } else {
                                    break;
                                }
                            }
                            ' ' => {
                                vcol += 1;
                                bytes_to_remove += 1;
                            }
                            _ => break,
                        }
                    }
                    if bytes_to_remove > 0 {
                        let adjusted_start = offset_by_delta(line_start, delta);
                        let delete_range = crate::primitives::Range::from_raw(
                            adjusted_start,
                            adjusted_start + bytes_to_remove,
                        );
                        effects.push(Effect::delete(delete_range));
                        let new_len = line.len().saturating_sub(bytes_to_remove);
                        if first_line_new_len.is_none() {
                            first_line_new_len = Some(new_len);
                        }
                        last_line_adjusted_start = adjusted_start;
                        last_line_new_len = new_len;
                        delta -= bytes_to_remove.cast_signed();
                    } else {
                        if first_line_new_len.is_none() {
                            first_line_new_len = Some(line.len());
                        }
                        last_line_adjusted_start = offset_by_delta(line_start, delta);
                        last_line_new_len = line.len();
                    }
                } else if spaces_to_remove > 0 {
                    // No tabs, just spaces to remove: Delete leading spaces
                    let adjusted_start = offset_by_delta(line_start, delta);
                    let delete_range = crate::primitives::Range::from_raw(
                        adjusted_start,
                        adjusted_start + spaces_to_remove,
                    );
                    effects.push(Effect::delete(delete_range));
                    let new_len = line.len().saturating_sub(spaces_to_remove);
                    if first_line_new_len.is_none() {
                        first_line_new_len = Some(new_len);
                    }
                    last_line_adjusted_start = adjusted_start;
                    last_line_new_len = new_len;
                    delta -= spaces_to_remove.cast_signed();
                } else {
                    // Nothing to remove
                    if first_line_new_len.is_none() {
                        first_line_new_len = Some(expanded_buf.len());
                    }
                    last_line_adjusted_start = offset_by_delta(line_start, delta);
                    last_line_new_len = expanded_buf.len();
                }
            }
        }
        current_offset = line_end + 1; // +1 for newline
    }

    // Neovim's op_shift places cursor on the FIRST line of the operated
    // range (oap->start.lnum) via `beginline(BL_SOL | BL_FIX)`. With
    // nostartofline (default), beginline resolves to coladvance(curswant),
    // restoring the pre-operator virtual column on that first line.
    //
    // For upward motions (>k, >gg), the first affected line is ABOVE the
    // cursor, so we must use first_line_start, not the cursor's line.
    let pre_curswant = curswant_of(text, ctx.cursor.get(), ctx.tabstop);
    let first_line_len = first_line_new_len.unwrap_or(0);
    // Reconstruct the first affected line's content to compute vcol→byte.
    let orig_line = text
        .get(first_line_start..)
        .and_then(|s| s.split('\n').next())
        .unwrap_or("");
    let new_first_line = match direction {
        ShiftDirection::Right => {
            if orig_line.is_empty() {
                String::new()
            } else if orig_line.contains('\t') && ctx.expandtab {
                // Tab expansion + indent: full replace was done
                let mut buf = String::new();
                expand_tabs_into(orig_line, ctx.shiftwidth, &mut buf);
                format!("{indent_str}{buf}")
            } else {
                format!("{indent_str}{orig_line}")
            }
        }
        ShiftDirection::Left => {
            if orig_line.contains('\t') && ctx.expandtab {
                // Full expansion was done; the expanded+outdented content
                let mut buf = String::new();
                expand_tabs_into(orig_line, ctx.tabstop, &mut buf);
                let spaces_to_remove = buf
                    .bytes()
                    .take(total_shift)
                    .take_while(|&b| b == b' ')
                    .count();
                buf[spaces_to_remove..].to_string()
            } else if orig_line.contains('\t') {
                // noexpandtab: remove leading whitespace bytes
                let mut vcol: usize = 0;
                let mut bytes_to_remove: usize = 0;
                for ch in orig_line.chars() {
                    if vcol >= total_shift {
                        break;
                    }
                    match ch {
                        '\t' => {
                            let next = (vcol / ctx.tabstop + 1) * ctx.tabstop;
                            if next <= total_shift {
                                vcol = next;
                                bytes_to_remove += 1;
                            } else {
                                break;
                            }
                        }
                        ' ' => {
                            vcol += 1;
                            bytes_to_remove += 1;
                        }
                        _ => break,
                    }
                }
                orig_line[bytes_to_remove..].to_string()
            } else {
                // Spaces only
                let mut buf = String::new();
                expand_tabs_into(orig_line, ctx.tabstop, &mut buf);
                let spaces_to_remove = buf
                    .bytes()
                    .take(total_shift)
                    .take_while(|&b| b == b' ')
                    .count();
                if spaces_to_remove <= orig_line.len() {
                    orig_line[spaces_to_remove..].to_string()
                } else {
                    orig_line.to_owned()
                }
            }
        }
    };
    // coladvance(curswant) in the new line: find byte offset for the vcol.
    // BL_FIX means don't land on NUL (i.e., clamp to last char of line).
    let byte_in_new = vcol_to_byte(&new_first_line, pre_curswant, ctx.tabstop);
    let clamped_byte = if new_first_line.is_empty() {
        0
    } else {
        byte_in_new.min(first_line_len.saturating_sub(1))
    };
    let new_cursor = Offset::new(first_line_start + clamped_byte);

    // Neovim's op_shift() explicitly sets mark.[ and mark.] after all per-line
    // shifts (ops.c:240-248).
    // mark.[ = oap->start — preserves cursor column.
    // In Neovim, oap->start = min(cursor, motion_target) after the potential
    // swap in do_pending_operator. This preserves the column of whichever
    // position is earlier in the buffer.
    // mark.] = last byte of last affected line after shifting (inclusive)
    //          = ml_get_len(oap->end.lnum) - 1
    let mark_start = Offset::new(ctx.cursor.get().min(ctx.motion_target.get()));
    // Neovim's `.` mark for shift operations uses the line-start of the
    // first affected line (oap->start with col=0), NOT the cursor column.
    let dot_mark = Offset::new(first_line_start);
    let mark_end_val = if last_line_new_len > 0 {
        last_line_adjusted_start + last_line_new_len - 1
    } else {
        last_line_adjusted_start
    };
    let effects = effects
        .set_mark(MarkName::CHANGE_START, mark_start, None)
        .set_mark(MarkName::CHANGE_END, Offset::new(mark_end_val), None)
        .set_mark(MarkName::LAST_CHANGE, dot_mark, None)
        .set_cursor(new_cursor)
        .end_undo();

    CommandResult::new(effects, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{MotionType, Range};

    #[test]
    fn test_indent_single_line() {
        let ctx = OperatorContext::new(
            "hello",
            Range::from_raw(0, 5),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_indent(&ctx);

        // Should have undo groups and insert effect (per-line)
        let effects: Vec<_> = result.effects.iter().collect();
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));

        let has_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Insert { .. }));
        assert!(
            has_insert,
            "Indent should produce insert effect for line without tabs"
        );
    }

    #[test]
    fn test_outdent_removes_spaces() {
        let ctx = OperatorContext::new(
            "    hello",
            Range::from_raw(0, 9),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_outdent(&ctx);

        let has_delete = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Delete { .. }));
        assert!(
            has_delete,
            "Outdent should produce delete effect for spaces-only indent"
        );
    }

    #[test]
    fn test_indent_empty_line_skipped() {
        // "hello\n\nworld" - empty line in middle should not be indented
        let text = "hello\n\nworld";
        let ctx = OperatorContext::new(
            text,
            Range::from_raw(0, text.len()),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_indent(&ctx);

        // Should have exactly 2 insert effects (line 1 and line 3, not line 2)
        let insert_count = result
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::Insert { .. }))
            .count();
        assert_eq!(
            insert_count, 2,
            "Should insert indent for 2 non-empty lines"
        );
    }

    #[test]
    fn test_indent_with_tabs_produces_replace() {
        let text = "\thello";
        let ctx = OperatorContext::new(
            text,
            Range::from_raw(0, text.len()),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_indent(&ctx);

        let has_replace = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Replace { .. }));
        assert!(
            has_replace,
            "Indent on line with tabs should produce replace effect"
        );
    }

    #[test]
    fn test_outdent_with_tabs_produces_replace() {
        let text = "\thello";
        let ctx = OperatorContext::new(
            text,
            Range::from_raw(0, text.len()),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_outdent(&ctx);

        let has_replace = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Replace { .. }));
        assert!(
            has_replace,
            "Outdent on line with tabs should produce replace effect"
        );
    }

    #[test]
    fn test_multi_line_indent_produces_per_line_effects() {
        let text = "aaa\nbbb\nccc";
        let ctx = OperatorContext::new(
            text,
            Range::from_raw(0, text.len()),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_indent(&ctx);

        // Should have 3 insert effects, one per line
        let insert_count = result
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::Insert { .. }))
            .count();
        assert_eq!(
            insert_count, 3,
            "Should produce one insert per non-empty line"
        );
    }
}
