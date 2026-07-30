//! Pure helper functions for operator range computation.
//!
//! Extracted from `range.rs` — contains functions for range extension,
//! promotion, EOF adjustment, and motion type determination.

use crate::commands::helpers::line_start_for_offset;
use crate::grammar::types::Motion;
use crate::primitives::{MotionType, Offset, Range};

/// Extend a range to include full lines at both boundaries.
///
/// For linewise motions (j, k, gg, G), we need the range to cover
/// complete lines from start-of-line to end-of-line (including newline).
pub(crate) fn extend_to_full_lines(text: &str, start: usize, end: usize) -> (usize, usize) {
    // Find start of first line
    let line_start = line_start_for_offset(text, start);

    // Find end of last line (including newline if present)
    let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i + 1); // To end of buffer if no newline

    // EOF case: when deleting the last line(s) of the file (line_end == text.len())
    // and the range doesn't start at the beginning of the file (line_start > 0),
    // include the preceding newline so the remaining text doesn't have a trailing \n.
    if line_end == text.len() && line_start > 0 {
        (line_start - 1, line_end)
    } else {
        (line_start, line_end)
    }
}

/// Determine motion type from the Motion enum.
///
/// Derives the answer from the authoritative `motion_inclusivity()` in
/// `inclusivity.rs`, which is exhaustive (covers every `Motion` variant).
/// This eliminates the catch-all `_ => CharWise` that previously missed
/// linewise motions like `+`, `-`, `_`, `gj`, `gk`, `H`, `M`, `L`.
///
/// Paragraph motions are a special case: their range computation is
/// exclusive (charwise), but Vim stores paragraph yanks/deletes with
/// linewise register type. We keep the explicit `LineWise` override here
/// so that `RangeResult.motion_type` flows correctly to register storage.
pub(super) const fn determine_motion_type(motion: &Motion) -> MotionType {
    // Some motions are exclusive for range computation but linewise for register storage.
    // The exclusive_linewise_motion flag in compute_motion_range skips
    // extend_to_full_lines so the range stays charwise; only motion_type
    // becomes LineWise, for register storage.
    //
    // - Paragraph motions ({/}): exclusive range (don't include blank line) but 'V' register
    // - Display-line motions (gj/gk): exclusive range (exact display line) but 'V' register
    if matches!(
        motion,
        Motion::ParagraphForward
            | Motion::ParagraphBackward
            | Motion::DisplayDown
            | Motion::DisplayUp
    ) {
        return MotionType::LineWise;
    }

    // For everything else, derive from the exhaustive inclusivity classification.
    if super::inclusivity::motion_inclusivity(*motion).is_linewise() {
        MotionType::LineWise
    } else {
        MotionType::CharWise
    }
}

/// Check whether a motion is a forward-direction exclusive motion.
///
/// Used to scope the "cursor == target -> inclusive" promotion rule.
/// Only forward-seeking exclusive motions should promote when clamped
/// (e.g., `dl` at end of line -> deletes under cursor).
/// Backward motions at boundary (e.g., `d0` at col 0) should NOT promote.
pub(super) const fn is_forward_exclusive_motion(motion: &Motion) -> bool {
    // Search motions (n, *, etc.) are intentionally excluded: when the only
    // match wraps back to the cursor position, the operator should be a no-op
    // (empty exclusive range), not delete the character under the cursor.
    // The "cursor didn't move → make inclusive" rule only applies to simple
    // movement motions like l, Space, w, W, ), }.
    matches!(
        motion,
        Motion::Right
            | Motion::Space
            | Motion::WordForward
            | Motion::WORDForward
            | Motion::SentenceForward
            | Motion::ParagraphForward
    )
}

/// Apply Neovim's exclusive-to-linewise promotion rule (`:h exclusive-linewise`).
///
/// When an exclusive, character-wise motion's end lands at column 0 (first
/// column of a line), Neovim adjusts the range:
///
/// 1. If the start position is at or before the first non-blank character
///    on its line → the motion becomes **linewise** (extended to full lines).
/// 2. Otherwise → the end is moved back to the end of the previous line
///    and the motion becomes **inclusive** (but not linewise).
///
/// This must be called BEFORE the range is finalized, operating on the raw
/// `end` position that the exclusive motion produced.
///
/// # Arguments
/// * `text` - The document text
/// * `range` - The computed range (exclusive end)
/// * `motion_type` - Current motion type
/// * `is_exclusive` - Whether the motion that produced this range is exclusive
///
/// # Returns
/// Tuple of (adjusted range, final motion type, `was_promoted_to_linewise`)
#[must_use]
pub fn maybe_promote_to_linewise(
    text: &str,
    range: Range,
    motion_type: MotionType,
    is_exclusive: bool,
) -> (Range, MotionType, bool) {
    // Already linewise — nothing to do.
    if motion_type.is_line_wise() {
        return (range, motion_type, false);
    }

    // Only applies to exclusive motions.
    if !is_exclusive {
        return (range, motion_type, false);
    }

    let clamped = range.clamp_end(Offset::new(text.len()));
    let start = clamped.start().get();
    let end = clamped.end().get();

    // Rule trigger: end must be at column 0, i.e., the byte before `end` is '\n'
    // (or end == 0, but that would mean empty range so nothing to do).
    if end == 0 || end <= start {
        return (range, motion_type, false);
    }

    let end_at_col0 = text.as_bytes().get(end - 1) == Some(&b'\n');
    if !end_at_col0 {
        return (range, motion_type, false);
    }

    // End is at column 0 of some line. Now check the exception:
    // If start is at or before the first non-blank on its line → linewise.

    // Find start of the line containing `start`.
    let start_line_begin = line_start_for_offset(text, start);

    // Find the first non-blank character on the start's line.
    let start_line_text_end = text[start_line_begin..]
        .find('\n')
        .map_or(text.len(), |i| start_line_begin + i);
    let start_line_text = &text[start_line_begin..start_line_text_end];
    let first_non_blank_offset =
        start_line_begin + crate::commands::helpers::first_non_blank_in_line(start_line_text);

    if start <= first_non_blank_offset {
        // Exception: start is at or before first non-blank → promote to linewise.
        // First, back up end to the previous line (the line before col-0 position),
        // then extend to full line boundaries. This ensures we don't include the
        // line that end was pointing to.
        let backed_up_end = end - 1; // Points at the '\n' of the previous line
        let (ls, le) = extend_to_full_lines(text, start, backed_up_end);
        (Range::from_raw(ls, le), MotionType::LineWise, true)
    } else {
        // Normal case: back up end to end of previous line (make inclusive).
        // The newline at end-1 is excluded; end becomes the position of the \n.
        // This applies regardless of whether the previous line is empty —
        // Vim's coladvance(MAXCOL) on an empty line stays at col 0, but the
        // range still excludes the line that `end` was pointing at.
        let newline_pos = end - 1;
        (Range::from_raw(start, newline_pos), motion_type, false)
    }
}

/// Adjust range for linewise text objects at EOF with delete/change operators.
///
/// Extends range backward to include preceding newline so `"l1\n\nl2"` with `dip` on l2
/// deletes `"\nl2"` leaving `"l1\n"` instead of `"l1\n\n"`.
///
/// This is pure: `(text, operator, range) -> Range`.
#[must_use]
pub fn adjust_eof_range(
    text: &str,
    operator: crate::grammar::types::Operator,
    text_obj_range: &crate::commands::textobjects::TextObjectRange,
) -> Range {
    if text_obj_range.linewise
        && text_obj_range.range.end().get() >= text.len()
        && !text.ends_with('\n')
        && text_obj_range.range.start().get() > 0
        && text
            .as_bytes()
            .get(text_obj_range.range.start().prev().get())
            == Some(&b'\n')
        && matches!(
            operator,
            crate::grammar::types::Operator::Delete | crate::grammar::types::Operator::Change
        )
    {
        Range::new(
            text_obj_range.range.start().prev(),
            text_obj_range.range.end(),
        )
    } else {
        text_obj_range.range
    }
}

/// Handle empty text object range (e.g., `di(` on `()`).
///
/// Returns effects for:
/// - Empty buffer with linewise range (dip on empty buffer): routes to register system
/// - Change on empty range (ci" on ""): enters insert mode at the range start
/// - Otherwise: just sets cursor to range start
pub fn empty_textobject_result(
    text: &str,
    operator: crate::grammar::types::Operator,
    register: Option<crate::primitives::RegisterName>,
    text_obj_range: &crate::commands::textobjects::TextObjectRange,
) -> crate::commands::CommandResult {
    use crate::commands::CommandResult;
    use crate::effects::Effects;
    use crate::grammar::types::Operator;

    // Special case: linewise empty range on empty buffer (dip on empty buffer)
    if text_obj_range.linewise
        && text.is_empty()
        && matches!(operator, Operator::Delete | Operator::Yank)
    {
        use crate::commands::operators::registers::route_empty_buffer_registers;
        return CommandResult::effects_only(route_empty_buffer_registers(operator, register));
    }

    let cursor_at_start = Offset::new(text_obj_range.range.start().get());

    // For Change on empty range (e.g., ci" on ""), enter insert mode
    if matches!(operator, Operator::Change) {
        use crate::primitives::InsertEntryType;
        let effects = Effects::new()
            .set_cursor(cursor_at_start)
            .begin_undo()
            .begin_insert(InsertEntryType::ChangeOperator, 1, 0, cursor_at_start);
        return CommandResult::effects_only(effects);
    }

    CommandResult::effects_only(Effects::new().set_cursor(cursor_at_start))
}
