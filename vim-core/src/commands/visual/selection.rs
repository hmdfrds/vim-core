//! Selection helpers for motion result handling.
//!
//! Provides helper functions for building effects from motion results,
//! particularly for visual mode selection extension and jump list handling.

use crate::effects::Effects;
use crate::primitives::{Offset, SelectionRange, SelectionShape};

/// Build effects for a motion result in visual mode.
///
/// Extends the selection by moving the head to the new position.
///
/// # Arguments
/// * `selection` - The current selection
/// * `new_offset` - The new cursor/head position
/// * `shape` - The render shape that must be preserved
///
/// # Returns
/// Effects to extend selection and move cursor
#[inline]
pub fn extend_selection(
    selection: &SelectionRange,
    new_offset: Offset,
    shape: SelectionShape,
) -> Effects {
    Effects::new().set_visual_selection(selection.anchor(), new_offset, shape)
}

/// Build effects for a motion result in normal mode.
///
/// Handles jump list push for jump motions.
///
/// # Arguments
/// * `current_offset` - Current cursor position (for jump list)
/// * `new_offset` - The new cursor position
/// * `is_jump_motion` - Whether this is a jump motion (gg, G, %, etc.)
///
/// # Returns
/// Effects to move cursor (and optionally push to jump list)
#[inline]
pub fn move_cursor(current_offset: Offset, new_offset: Offset, is_jump_motion: bool) -> Effects {
    if is_jump_motion {
        Effects::new()
            .push_jump_list(current_offset)
            .set_mark(crate::primitives::MarkName::PREV_JUMP, current_offset, None)
            .set_mark(
                crate::primitives::MarkName::PREV_JUMP_EXACT,
                current_offset,
                None,
            )
            .set_cursor(new_offset)
    } else {
        Effects::new().set_cursor(new_offset)
    }
}

/// Build effects for exiting visual mode after an operator.
///
/// This is the standard Vim pattern: exit visual mode, clear selection,
/// and position cursor at start of selection.
///
/// # Arguments
/// * `start_offset` - Start of the selection (cursor position after exit)
///
/// # Returns
/// Effects to clear selection, exit visual mode, and set cursor
#[inline]
pub fn visual_exit_effects(start_offset: Offset) -> Effects {
    Effects::new()
        .clear_selection()
        .set_mode(crate::primitives::Mode::Normal)
        .set_cursor(start_offset)
}

/// Compute `LastVisualInfo` from selection bounds and visual type.
///
/// Shared logic extracted from `visual_commands.rs` and `operator_selection.rs`.
/// Both files previously computed this identically.
///
/// The `columns` field stores **virtual columns** (tab-expanded screen columns),
/// matching Neovim's `resel_VIsual_vcol` behavior. This ensures dot-repeat
/// of visual operators reconstructs the correct range even when the text
/// contains tabs or multi-byte characters.
///
/// - Single-line charwise: stores the virtual column **width** of the selection
/// - Multi-line charwise: stores the virtual column of `hi` on its line
/// - Linewise / block: columns are informational (linewise ignores them)
#[inline]
#[must_use]
pub fn compute_last_visual_info(
    text: &str,
    visual_type: crate::primitives::VisualType,
    sel_start: usize,
    sel_end: usize,
    tabstop: usize,
) -> crate::primitives::LastVisualInfo {
    use crate::commands::helpers::{byte_to_vcol, line_start_for_offset};

    let sel_start = sel_start.min(text.len());
    let sel_end = sel_end.min(text.len());
    let (lo, hi) = if sel_start <= sel_end {
        (sel_start, sel_end)
    } else {
        (sel_end, sel_start)
    };
    let lines = text.get(lo..hi).map_or(1, |s| s.matches('\n').count() + 1);

    let columns = if lines == 1 {
        // Single-line: store virtual column width (vcol(hi) - vcol(lo)).
        let line_start = line_start_for_offset(text, lo);
        let lo_vcol = byte_to_vcol(&text[line_start..], lo - line_start, tabstop);
        let hi_vcol = byte_to_vcol(&text[line_start..], hi - line_start, tabstop);
        hi_vcol.saturating_sub(lo_vcol)
    } else {
        // Multi-line: store absolute virtual column of hi on the last line.
        let last_line_start = line_start_for_offset(text, hi);
        byte_to_vcol(&text[last_line_start..], hi - last_line_start, tabstop)
    };

    crate::primitives::LastVisualInfo::new(visual_type, lines, columns)
}

/// Compute `LastVisualInfo` and wrap it in effects for saving.
///
/// Keeps effect construction in the commands layer, avoiding the execution
/// layer needing to import `Effects`.
#[inline]
pub fn save_last_visual_effects(
    text: &str,
    visual_type: crate::primitives::VisualType,
    sel_start: usize,
    sel_end: usize,
    tabstop: usize,
    cursor_at_start: bool,
) -> crate::effects::Effects {
    let info = compute_last_visual_info(text, visual_type, sel_start, sel_end, tabstop)
        .with_cursor_at_start(cursor_at_start);
    crate::effects::Effects::new().save_last_visual(info)
}

/// Build complete visual exit effects: save `LastVisualInfo`, set `<`/`>` marks,
/// and clear selection.
///
/// This is the canonical visual exit sequence for operator commands.
/// Combines `compute_last_visual_info` + mark setting + selection clearing.
#[inline]
pub fn visual_exit_with_marks(
    text: &str,
    visual_type: crate::primitives::VisualType,
    selection: &SelectionRange,
    tabstop: usize,
) -> Effects {
    let (sel_start, sel_end) = (selection.start(), selection.end());
    let cursor_at_start = !selection.is_forward();
    let info = compute_last_visual_info(text, visual_type, sel_start.get(), sel_end.get(), tabstop)
        .with_cursor_at_start(cursor_at_start);

    let (mark_start, mark_end) = {
        let (raw_start, raw_end) = if selection.is_forward() {
            (selection.anchor(), selection.head())
        } else {
            (selection.head(), selection.anchor())
        };
        // For linewise visual, Neovim sets mark.< to the start of the first
        // selected line and mark.> to a large sentinel (v:maxcol) column on
        // the last line. We approximate by using line_start for `<`.
        if visual_type == crate::primitives::VisualType::Line {
            let line_start = crate::commands::helpers::line_start_for_offset(text, raw_start.get());
            (crate::primitives::Offset::new(line_start), raw_end)
        } else {
            (raw_start, raw_end)
        }
    };

    Effects::new()
        .save_last_visual(info)
        .set_mark(crate::primitives::MarkName::VISUAL_START, mark_start, None)
        .set_mark(crate::primitives::MarkName::VISUAL_END, mark_end, None)
        .clear_selection()
}

/// Convert a selection to operator range.
///
/// Handles the anchor > head case correctly.
/// In Vim, char-wise visual selection is **inclusive** by default —
/// the character under cursor is included in the selection.
///
/// # Gap indexing semantics
///
/// With gap indexing, `selection.start()` and `selection.end()` are already
/// half-open `[start, end)`. For the **inclusive** case (default), the range
/// is simply `[start, end)` — no `.next()` adjustment needed.
///
/// For the **exclusive** case, we shrink by removing the last character from
/// the head side: `end = prev_char_boundary(text, end)`. If shrinking would
/// make the range empty (single-char selection), we fall through to inclusive.
///
/// # Arguments
/// * `text` - Document text (needed for `prev_char_boundary` in exclusive mode)
/// * `selection` - The current selection
/// * `exclusive` - When `true`, the character at the cursor position is NOT
///   included in the range (`selection=exclusive` Vim option). If the
///   selection is single-char, it is always treated as inclusive — single-
///   character selections are never empty (Neovim behavior).
///
/// # Returns
/// Tuple of (`start_offset`, range) where the range respects `exclusive`
#[inline]
#[must_use]
pub const fn selection_to_operator_range(
    text: &str,
    selection: &SelectionRange,
    exclusive: bool,
) -> (Offset, crate::primitives::Range) {
    use crate::primitives::text_util::prev_char_boundary;

    let start = selection.start();
    let end = selection.end();

    if exclusive {
        // Shrink by one character from the end (head side).
        let shrunk_end = prev_char_boundary(text, end.get());
        if shrunk_end > start.get() {
            // Exclusive: range [start, shrunk_end)
            return (
                start,
                crate::primitives::Range::new(start, Offset::new(shrunk_end)),
            );
        }
        // Single-char guard: shrinking would make range empty — fall through to inclusive.
    }

    // Inclusive (default), or exclusive single-char fallback:
    // Visual selection head is the cursor position (ON a character). To include
    // that character in the operator range, expand end to the next char boundary.
    // This matches Neovim's inclusive visual selection semantics.
    let inclusive_end = crate::primitives::text_util::next_char_boundary(text, end.get());
    (
        start,
        crate::primitives::Range::new(start, Offset::new(inclusive_end)),
    )
}

/// Expand a selection to full line boundaries (for visual line mode).
///
/// Given a character-level selection (anchor/head), expands both ends to
/// cover complete lines. The anchor moves to the start of its line, and
/// the head moves to include the trailing newline of its line (or end of text).
///
/// # Arguments
/// * `text` - The document text
/// * `selection` - The charwise selection to expand
///
/// # Returns
/// A new `SelectionRange` covering full lines
#[inline]
#[must_use]
pub fn expand_selection_to_lines(text: &str, selection: &SelectionRange) -> SelectionRange {
    use crate::commands::helpers::{line_end, line_of, line_start};

    let start = selection.start().get();
    let end = selection.end().get();
    let start_line = line_of(text, start);
    let end_line = line_of(text, end);
    let line_start_off = line_start(text, start_line).unwrap_or(0);
    let line_end_off = line_end(text, end_line).unwrap_or(text.len());
    let expanded_end = if line_end_off < text.len() {
        line_end_off + 1 // include trailing newline
    } else {
        line_end_off
    };
    // With gap indexing, selection end IS exclusive — use the line end directly.
    // No .saturating_sub(1) needed.
    SelectionRange::new(Offset::new(line_start_off), Offset::new(expanded_end))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Operator Range Resolution (extracted from execution/operator_selection.rs)
// ═══════════════════════════════════════════════════════════════════════════════

/// Resolve a live visual selection to operator range, motion type, and cursor position.
///
/// Handles linewise expansion when mode is `Visual(Line)`.
///
/// # Arguments
/// * `text` - Document text
/// * `selection` - The current visual selection
/// * `mode` - Current editor mode (used to detect linewise visual)
/// * `selection_exclusive` - Whether the `selection` option is `"exclusive"`.
///   Pass `options.selection_is_exclusive()`. Has no effect on linewise
///   selections (those are always fully inclusive of their lines).
///
/// This is pure: `(text, selection, mode, exclusive) → (Range, MotionType, Offset)`.
#[must_use]
pub fn resolve_live_selection(
    text: &str,
    selection: &SelectionRange,
    mode: crate::primitives::Mode,
    selection_exclusive: bool,
) -> (
    crate::primitives::Range,
    crate::primitives::MotionType,
    Offset,
) {
    use crate::primitives::{MotionType, Range};

    // Linewise selections always use inclusive range — the exclusive flag only
    // affects charwise selections. This prevents the exclusive adjustment from
    // shifting `last_selected` and corrupting the line-boundary scan.
    let is_linewise = matches!(mode, crate::primitives::Mode::Visual(vt) if vt.is_line());
    let effective_exclusive = selection_exclusive && !is_linewise;
    let (start, mut range) = selection_to_operator_range(text, selection, effective_exclusive);

    // Clamp range end to text length
    if range.end().get() > text.len() {
        range = range.with_end(Offset::new(text.len()));
    }

    // Track the actual first-line start (before EOF backward extension).
    let mut linewise_first_line_start = 0usize;

    let motion_type = match mode {
        crate::primitives::Mode::Visual(vt) if vt.is_line() => {
            let line_start =
                crate::commands::helpers::line_start_for_offset(text, range.start().get());
            linewise_first_line_start = line_start;
            let last_selected = if range.end().get() > 0 {
                crate::primitives::text_util::prev_char_boundary(text, range.end().get())
            } else {
                0
            };
            let ls_line = crate::commands::helpers::line_of(text, last_selected);
            let ls_eol = crate::commands::helpers::line_end(text, ls_line).unwrap_or(text.len());
            let line_end = if ls_eol < text.len() {
                ls_eol + 1
            } else {
                ls_eol
            };

            // When the selection covers the last line (no trailing newline),
            // extend backward to include the preceding \n — same as
            // compute_linewise_range's at_eof logic. Without this, deleting
            // the last line would leave a trailing newline in the text.
            let final_start = if line_end >= text.len() && line_start > 0 {
                line_start - 1
            } else {
                line_start
            };

            range = Range::from_raw(final_start, line_end);
            MotionType::LineWise
        }
        _ => MotionType::CharWise,
    };

    // Neovim places cursor at col 0 of the first selected line, never on the
    // preceding newline that EOF-adjustment may include in the range.
    let cursor_pos = if motion_type.is_line_wise() {
        Offset::new(linewise_first_line_start)
    } else {
        start
    };

    (range, motion_type, cursor_pos)
}

/// Reconstruct operator range from `LastVisualInfo` for dot-repeat.
///
/// When an operator applied in visual mode is repeated with `.` in normal mode,
/// this reconstructs the equivalent range from stored visual dimensions.
///
/// The `columns` field in `LastVisualInfo` stores **virtual columns** (tab-expanded),
/// so this function uses `vcol_to_byte` and `byte_to_vcol` to convert between
/// screen columns and byte offsets.
///
/// This is pure: `(text, cursor, last_visual, tabstop) → (Range, MotionType, Offset)`.
#[must_use]
pub fn reconstruct_from_last_visual(
    text: &str,
    cursor: usize,
    last_visual: &crate::primitives::LastVisualInfo,
    tabstop: usize,
) -> (
    crate::primitives::Range,
    crate::primitives::MotionType,
    Offset,
) {
    use crate::commands::helpers::{byte_to_vcol, line_start_for_offset, vcol_to_byte};
    use crate::primitives::VisualType;
    use crate::primitives::{MotionType, Range};

    match last_visual.visual_type() {
        VisualType::Line => {
            // Linewise: expand from cursor's line for `lines` count
            let line_start = line_start_for_offset(text, cursor);
            let mut line_end = line_start;
            for _ in 0..last_visual.lines() {
                if let Some(nl) = text[line_end..].find('\n') {
                    line_end = line_end + nl + 1;
                } else {
                    line_end = text.len();
                    break;
                }
            }
            let range = Range::from_raw(line_start, line_end);
            (range, MotionType::LineWise, Offset::new(line_start))
        }
        VisualType::Char => {
            // Charwise: reconstruct from cursor + virtual column dimensions
            let cursor_line_start = line_start_for_offset(text, cursor);

            if last_visual.lines() <= 1 {
                // Single-line: columns stores vcol width of the selection.
                // Compute cursor's vcol, add width, convert back to byte offset.
                let cursor_vcol = byte_to_vcol(
                    &text[cursor_line_start..],
                    cursor - cursor_line_start,
                    tabstop,
                );
                let end_vcol = cursor_vcol + last_visual.columns() + 1;
                let line_end = text[cursor_line_start..]
                    .find('\n')
                    .map_or(text.len(), |i| cursor_line_start + i);
                let end_byte_in_line =
                    vcol_to_byte(&text[cursor_line_start..line_end], end_vcol, tabstop);
                let end = (cursor_line_start + end_byte_in_line).min(text.len());
                let range = Range::from_raw(cursor, end);
                (range, MotionType::CharWise, Offset::new(cursor))
            } else {
                // Multi-line: advance lines, then columns stores absolute vcol
                // of the end position on the last line.
                let mut pos = cursor;
                for _ in 1..last_visual.lines() {
                    if let Some(nl) = text[pos..].find('\n') {
                        pos = pos + nl + 1;
                    } else {
                        pos = text.len();
                        break;
                    }
                }
                // pos is now the start of the last line
                let last_line_start = pos;
                let last_line_end = text[last_line_start..]
                    .find('\n')
                    .map_or(text.len(), |i| last_line_start + i);
                let end_byte_in_line = vcol_to_byte(
                    &text[last_line_start..last_line_end],
                    last_visual.columns() + 1,
                    tabstop,
                );
                let end = (last_line_start + end_byte_in_line).min(text.len());
                let range = Range::from_raw(cursor, end);
                (range, MotionType::CharWise, Offset::new(cursor))
            }
        }
        VisualType::Block => {
            // Block mode: treat as charwise for now, using vcol conversion
            let cursor_line_start = line_start_for_offset(text, cursor);
            let cursor_vcol = byte_to_vcol(
                &text[cursor_line_start..],
                cursor - cursor_line_start,
                tabstop,
            );
            let end_vcol = cursor_vcol + last_visual.columns() + 1;
            let line_end = text[cursor_line_start..]
                .find('\n')
                .map_or(text.len(), |i| cursor_line_start + i);
            let end_byte_in_line =
                vcol_to_byte(&text[cursor_line_start..line_end], end_vcol, tabstop);
            let end = (cursor_line_start + end_byte_in_line).min(text.len());
            let range = Range::from_raw(cursor, end);
            (range, MotionType::CharWise, Offset::new(cursor))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extend_selection() {
        let selection = SelectionRange::new(Offset::new(0), Offset::new(5));
        let effects = extend_selection(&selection, Offset::new(10), SelectionShape::Char);
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn test_move_cursor_normal() {
        let effects = move_cursor(Offset::new(0), Offset::new(10), false);
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn test_move_cursor_jump() {
        let effects = move_cursor(Offset::new(0), Offset::new(10), true);
        assert_eq!(effects.len(), 4);
    }

    // --- expand_selection_to_lines edge cases ---

    #[test]
    fn test_expand_single_line_doc() {
        // "hello" — no newlines, selection covers entire doc
        // With gap indexing: anchor=gap1, head=gap3
        let sel = SelectionRange::new(Offset::new(1), Offset::new(3));
        let expanded = expand_selection_to_lines("hello", &sel);
        assert_eq!(expanded.anchor().get(), 0, "anchor should be start of line");
        // With gap indexing, head is exclusive end — no trailing newline, so len=5
        assert_eq!(
            expanded.head().get(),
            5,
            "head should be end of text (exclusive)"
        );
    }

    #[test]
    fn test_expand_multi_line() {
        // "abc\ndef\nghi\n" — select from mid-first to mid-second
        let sel = SelectionRange::new(Offset::new(1), Offset::new(5));
        let expanded = expand_selection_to_lines("abc\ndef\nghi\n", &sel);
        assert_eq!(expanded.anchor().get(), 0, "anchor at start of first line");
        // With gap indexing, head is exclusive end — covers through newline of second line
        // line_end of line 1 = 7 (byte of '\n'), expanded_end = 7+1 = 8
        assert_eq!(
            expanded.head().get(),
            8,
            "head at end of second line (exclusive, past newline)"
        );
    }

    #[test]
    fn test_expand_last_line_no_newline() {
        // "abc\ndef" — last line has no trailing newline
        let sel = SelectionRange::new(Offset::new(4), Offset::new(6));
        let expanded = expand_selection_to_lines("abc\ndef", &sel);
        assert_eq!(expanded.anchor().get(), 4, "anchor at start of second line");
        // With gap indexing, head is exclusive end = text.len() = 7
        assert_eq!(expanded.head().get(), 7, "head at end of text (exclusive)");
    }

    #[test]
    fn test_expand_reversed_anchor_head() {
        // Selection with head < anchor (backward selection)
        let sel = SelectionRange::new(Offset::new(5), Offset::new(1));
        let expanded = expand_selection_to_lines("abc\ndef\n", &sel);
        // Should still expand correctly — uses min/max internally
        assert_eq!(expanded.anchor().get(), 0, "reversed: anchor at line start");
        // "abc\ndef\n" — line_end of line 1 = 7 ('\n'), expanded_end = 7+1 = 8
        assert_eq!(
            expanded.head().get(),
            8,
            "reversed: head at end of second line (exclusive)"
        );
    }

    #[test]
    fn test_expand_single_char_text() {
        // "x" — zero-width selection at gap 0
        let sel = SelectionRange::new(Offset::new(0), Offset::new(0));
        let expanded = expand_selection_to_lines("x", &sel);
        assert_eq!(expanded.anchor().get(), 0);
        // With gap indexing, no trailing newline: expanded_end = text.len() = 1
        assert_eq!(
            expanded.head().get(),
            1,
            "single char: head at end of text (exclusive)"
        );
    }

    // --- selection_to_operator_range: inclusive vs exclusive ---

    #[test]
    fn test_selection_to_operator_range_inclusive() {
        // "hello" — selection [1, 3): anchor=1, head=3 (on 'l')
        // Inclusive: expand end to include char at head → next_char_boundary(3) = 4
        // Operator range [1, 4) covers "ell"
        let sel = SelectionRange::new(Offset::new(1), Offset::new(3));
        let (start, range) = selection_to_operator_range("hello", &sel, false);
        assert_eq!(start.get(), 1);
        assert_eq!(range.start().get(), 1);
        assert_eq!(range.end().get(), 4, "inclusive: end includes char at head");
    }

    #[test]
    fn test_selection_to_operator_range_exclusive_multi_char() {
        // "hello" — gap selection [1, 3) covers "el"
        // Exclusive: shrink by one char from end → prev_char_boundary("hello", 3) = 2
        // Range [1, 2) — only 'e' included
        let sel = SelectionRange::new(Offset::new(1), Offset::new(3));
        let (start, range) = selection_to_operator_range("hello", &sel, true);
        assert_eq!(start.get(), 1);
        assert_eq!(range.start().get(), 1);
        assert_eq!(
            range.end().get(),
            2,
            "exclusive: end = prev_char_boundary(3) = 2"
        );
    }

    #[test]
    fn test_selection_to_operator_range_exclusive_single_char_is_inclusive() {
        // "hello" — selection [2, 3): head=3 (on second 'l')
        // Exclusive: shrink would produce [2, 2) which is empty → fall back to inclusive
        // Inclusive fallback: expand end to include char at head → 4
        let sel = SelectionRange::new(Offset::new(2), Offset::new(3));
        let (start, range) = selection_to_operator_range("hello", &sel, true);
        assert_eq!(start.get(), 2);
        assert_eq!(range.start().get(), 2);
        assert_eq!(
            range.end().get(),
            4,
            "exclusive single-char falls back to inclusive: includes char at head"
        );
    }

    #[test]
    fn test_selection_to_operator_range_inclusive_single_char() {
        // "hello" — selection [2, 3): head=3 (on second 'l')
        // Inclusive: expand end to include char at head → 4
        let sel = SelectionRange::new(Offset::new(2), Offset::new(3));
        let (_start, range) = selection_to_operator_range("hello", &sel, false);
        assert_eq!(
            range.end().get(),
            4,
            "inclusive single-char includes char at head"
        );
    }

    #[test]
    fn test_selection_to_operator_range_exclusive_two_chars() {
        // "hello" — gap selection [0, 2) covers "he"
        // Exclusive: shrink → prev_char_boundary("hello", 2) = 1
        // Range [0, 1) — only 'h' included
        let sel = SelectionRange::new(Offset::new(0), Offset::new(2));
        let (_start, range) = selection_to_operator_range("hello", &sel, true);
        assert_eq!(range.start().get(), 0);
        assert_eq!(
            range.end().get(),
            1,
            "exclusive two-char: end = prev_char_boundary(2) = 1"
        );
    }

    // --- resolve_live_selection: exclusive parameter ---

    #[test]
    fn test_resolve_live_selection_inclusive_charwise() {
        // "hello world" — selection [1, 5): head=5 (on space ' ')
        // Inclusive: expand end to include char at head → 6
        // Operator range [1, 6) covers "ello "
        let sel = SelectionRange::new(Offset::new(1), Offset::new(5));
        let (range, motion_type, cursor_pos) =
            resolve_live_selection("hello world", &sel, crate::primitives::Mode::Normal, false);
        assert_eq!(range.start().get(), 1);
        assert_eq!(range.end().get(), 6, "inclusive: end includes char at head");
        assert!(matches!(
            motion_type,
            crate::primitives::MotionType::CharWise
        ));
        assert_eq!(cursor_pos.get(), 1);
    }

    #[test]
    fn test_resolve_live_selection_exclusive_charwise() {
        // "hello world" — gap selection [1, 5) covers "ello"
        // Exclusive: shrink → prev_char_boundary(5) = 4, range [1, 4)
        let sel = SelectionRange::new(Offset::new(1), Offset::new(5));
        let (range, motion_type, cursor_pos) =
            resolve_live_selection("hello world", &sel, crate::primitives::Mode::Normal, true);
        assert_eq!(range.start().get(), 1);
        assert_eq!(
            range.end().get(),
            4,
            "exclusive: end = prev_char_boundary(5) = 4"
        );
        assert!(matches!(
            motion_type,
            crate::primitives::MotionType::CharWise
        ));
        assert_eq!(cursor_pos.get(), 1);
    }

    #[test]
    fn test_resolve_live_selection_exclusive_single_char_is_inclusive() {
        // "hello" — selection [2, 3): head=3 (on second 'l')
        // Exclusive single-char falls back to inclusive → expand end to 4
        let sel = SelectionRange::new(Offset::new(2), Offset::new(3));
        let (range, _motion_type, _cursor_pos) =
            resolve_live_selection("hello", &sel, crate::primitives::Mode::Normal, true);
        assert_eq!(range.start().get(), 2);
        assert_eq!(
            range.end().get(),
            4,
            "exclusive single-char falls back to inclusive"
        );
    }

    #[test]
    fn test_resolve_live_selection_linewise_unaffected_by_exclusive() {
        use crate::primitives::{Mode, VisualType};

        // Visual Line with exclusive=true should still cover both lines,
        // not be shrunk by the exclusive adjustment.
        let text = "hello\nworld";
        // gap selection [0, 7) covers "hello\nw"
        let sel = SelectionRange::new(Offset::new(0), Offset::new(7));
        let (range, motion_type, _cursor_pos) =
            resolve_live_selection(text, &sel, Mode::Visual(VisualType::Line), true);
        assert!(motion_type.is_line_wise());
        // Should cover both lines: "hello\nworld" = [0, 11)
        assert_eq!(range.start().get(), 0);
        assert_eq!(
            range.end().get(),
            text.len(),
            "linewise + exclusive should cover both lines"
        );
    }

    // --- compute_last_visual_info: virtual columns ---

    #[test]
    fn last_visual_info_single_line_ascii() {
        // "hello" select bytes 1..3 ("el"), tabstop=4
        // vcol width = vcol(3) - vcol(1) = 3 - 1 = 2
        let info = compute_last_visual_info("hello", crate::primitives::VisualType::Char, 1, 3, 4);
        assert_eq!(info.lines(), 1);
        assert_eq!(info.columns(), 2, "single-line ASCII: columns = vcol width");
    }

    #[test]
    fn last_visual_info_single_line_with_tab() {
        // "\thello" select bytes 0..2 (tab + 'h'), tabstop=8
        // vcol(0) = 0, vcol(2) = 9 (tab=8 cols, 'h'=1 col), width = 9
        let info =
            compute_last_visual_info("\thello", crate::primitives::VisualType::Char, 0, 2, 8);
        assert_eq!(info.lines(), 1);
        assert_eq!(
            info.columns(),
            9,
            "single-line with tab: vcol width accounts for tab expansion"
        );
    }

    #[test]
    fn last_visual_info_multi_line_ascii() {
        // "abc\ndef" select bytes 1..5 ('b' to 'e'), tabstop=4
        // 2 lines. End at byte 5 on second line: last_line_start=4, offset_in_line=1
        // vcol of 'e' on "def" = vcol(1) = 1
        let info =
            compute_last_visual_info("abc\ndef", crate::primitives::VisualType::Char, 1, 5, 4);
        assert_eq!(info.lines(), 2);
        assert_eq!(
            info.columns(),
            1,
            "multi-line ASCII: columns = vcol of end on last line"
        );
    }

    #[test]
    fn last_visual_info_multi_line_with_tab_on_last_line() {
        // "abc\n\tdef" select bytes 1..6 ('b' to 'd'), tabstop=4
        // 2 lines. last_line_start=4, end byte=6, offset_in_line=2
        // Byte offsets in the text: \t=4, d=5, e=6, f=7
        // line text = "\tdef", offset_in_line = 6-4 = 2
        // byte_to_vcol("\tdef", 2, 4) = tab(4 cols) + 'd'(1 col) = 5
        let info =
            compute_last_visual_info("abc\n\tdef", crate::primitives::VisualType::Char, 1, 6, 4);
        assert_eq!(info.lines(), 2);
        assert_eq!(
            info.columns(),
            5,
            "multi-line with tab: vcol accounts for tab on last line"
        );
    }

    // --- reconstruct_from_last_visual: virtual columns ---

    #[test]
    fn reconstruct_single_line_ascii() {
        // Reconstruct from cursor=0, 1 line, 3 vcol width, tabstop=4
        // "hello" -> end = vcol_to_byte("hello", 0+3+1=4, 4) = 4
        let info =
            crate::primitives::LastVisualInfo::new(crate::primitives::VisualType::Char, 1, 3);
        let (range, mt, cursor) = reconstruct_from_last_visual("hello", 0, &info, 4);
        assert_eq!(range.start().get(), 0);
        assert_eq!(range.end().get(), 4);
        assert!(mt.is_char_wise());
        assert_eq!(cursor.get(), 0);
    }

    #[test]
    fn reconstruct_single_line_with_tab() {
        // "\thello" cursor at byte 1 ('h'), vcol of cursor=8
        // columns=2 (vcol width), so end_vcol = 8+2+1 = 11
        // vcol_to_byte("\thello", 11, 8) = byte 4 ('l')
        let info =
            crate::primitives::LastVisualInfo::new(crate::primitives::VisualType::Char, 1, 2);
        let (range, _, _) = reconstruct_from_last_visual("\thello", 1, &info, 8);
        assert_eq!(range.start().get(), 1);
        assert_eq!(range.end().get(), 4);
    }

    #[test]
    fn reconstruct_multi_line_ascii() {
        // "abc\ndef" cursor at 0, 2 lines, columns=2 (vcol on last line)
        // Advance 1 line -> pos=4 (start of "def"), end vcol=2+1=3
        // vcol_to_byte("def", 3, 4) = 3 -> end = 4+3 = 7
        let info =
            crate::primitives::LastVisualInfo::new(crate::primitives::VisualType::Char, 2, 2);
        let (range, _, _) = reconstruct_from_last_visual("abc\ndef", 0, &info, 4);
        assert_eq!(range.start().get(), 0);
        assert_eq!(range.end().get(), 7);
    }

    #[test]
    fn reconstruct_multi_line_with_tab_on_last_line() {
        // "abc\n\tdef" cursor at 0, 2 lines, columns=5 (vcol of 'd' after tab, tabstop=4)
        // Advance 1 line -> pos=4 (start of "\tdef")
        // vcol_to_byte("\tdef", 5+1=6, 4) tests `vcol >= target_vcol` at the top
        // of the loop, so it returns the byte index of the first char at or past
        // the target vcol. char_indices: (0, '\t'), (1, 'd'), (2, 'e'), (3, 'f')
        // start: vcol=0
        // (0, '\t'): vcol(0) < 6, tab: vcol=4
        // (1, 'd'): vcol(4) < 6, vcol=5
        // (2, 'e'): vcol(5) < 6, vcol=6
        // (3, 'f'): vcol(6) >= 6, return 3
        // So end = 4+3 = 7
        let info =
            crate::primitives::LastVisualInfo::new(crate::primitives::VisualType::Char, 2, 5);
        let (range, _, _) = reconstruct_from_last_visual("abc\n\tdef", 0, &info, 4);
        assert_eq!(range.start().get(), 0);
        assert_eq!(range.end().get(), 7);
    }

    #[test]
    fn reconstruct_linewise_ignores_columns() {
        // Linewise reconstruction doesn't use columns
        let info =
            crate::primitives::LastVisualInfo::new(crate::primitives::VisualType::Line, 2, 999);
        let (range, mt, _) = reconstruct_from_last_visual("abc\ndef\nghi", 0, &info, 4);
        assert!(mt.is_line_wise());
        // Should cover 2 lines from cursor: "abc\ndef\n"
        assert_eq!(range.start().get(), 0);
        assert_eq!(range.end().get(), 8);
    }

    #[test]
    fn compute_and_reconstruct_roundtrip_with_tabs() {
        // Verify that computing LastVisualInfo then reconstructing produces
        // the same range for tab-containing text
        let text = "ab\tcd\n\tefgh";
        let tabstop = 4;
        // Select from byte 1 ('b') to byte 8 ('f' on second line)
        let info =
            compute_last_visual_info(text, crate::primitives::VisualType::Char, 1, 8, tabstop);
        assert_eq!(info.lines(), 2);
        // vcol of byte 8 on "\tefgh": offset_in_line = 8-6 = 2
        // byte_to_vcol("\tefgh", 2, 4) = 4+1 = 5
        assert_eq!(info.columns(), 5);

        // Reconstruct from cursor=1 (same as original start)
        let (range, _, _) = reconstruct_from_last_visual(text, 1, &info, tabstop);
        assert_eq!(range.start().get(), 1);
        // end: advance 1 line from byte 1 -> find '\n' at byte 5 -> pos=6
        // vcol_to_byte("\tefgh", 5+1=6, 4):
        // (0,'\t'): vcol=4; (1,'e'): vcol=5; (2,'f'): vcol=6 >= 6 → byte 2
        // end = 6+2 = 8
        // end=9 because ranges are exclusive: sel_end=8 includes byte 8 ('f'),
        // so the exclusive range end is 9 (past 'f').
        assert_eq!(
            range.end().get(),
            9,
            "roundtrip should reproduce original selection (inclusive)"
        );
    }
}
