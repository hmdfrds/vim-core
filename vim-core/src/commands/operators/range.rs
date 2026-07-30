//! Range computation for operators.
//!
//! Computes the text range for operators based on motion or text object.
//!
//! # Algorithm
//!
//! 1. Compute target position from motion
//! 2. Create range from cursor to target
//! 3. Optionally promote charwise to linewise
//! 4. Return range and motion type

use crate::commands::motions::{MotionContext, MotionResult};
use crate::grammar::types::Motion;
use crate::primitives::{MotionType, Range, WordKind};

use super::range_helpers::{
    determine_motion_type, extend_to_full_lines, is_forward_exclusive_motion,
    maybe_promote_to_linewise,
};

/// Result of range computation.
#[derive(Debug, Clone)]
pub struct RangeResult {
    /// The computed range.
    pub range: Range,
    /// How the range should be treated.
    pub motion_type: MotionType,
    /// Whether this was promoted from charwise to linewise.
    pub was_promoted: bool,
    /// For linewise ranges: whether this is at end of file (no trailing newline).
    /// Used by delete operator to handle preceding newline deletion.
    pub at_eof: bool,
    /// For linewise ranges: whether there is a preceding newline.
    /// Used by delete operator to handle last-line deletion.
    pub has_preceding_newline: bool,
    /// The motion target byte offset (before linewise expansion).
    /// Used by yank/case operators to compute cursor position.
    pub motion_target: usize,
}

impl RangeResult {
    /// Create a new range result.
    #[inline]
    #[must_use]
    pub const fn new(range: Range, motion_type: MotionType) -> Self {
        Self {
            range,
            motion_type,
            was_promoted: false,
            at_eof: false,
            has_preceding_newline: false,
            motion_target: 0,
        }
    }

    /// Create with promotion flag.
    #[inline]
    #[must_use]
    pub const fn promoted(range: Range, motion_type: MotionType) -> Self {
        Self {
            range,
            motion_type,
            was_promoted: true,
            at_eof: false,
            has_preceding_newline: false,
            motion_target: 0,
        }
    }

    /// Set the motion target offset.
    #[inline]
    #[must_use]
    pub const fn with_motion_target(mut self, target: usize) -> Self {
        self.motion_target = target;
        self
    }

    /// Create for linewise range with EOF flags.
    #[inline]
    #[must_use]
    pub const fn new_linewise(
        range: Range,
        motion_type: MotionType,
        has_preceding_newline: bool,
        at_eof: bool,
    ) -> Self {
        Self {
            range,
            motion_type,
            was_promoted: false,
            at_eof,
            has_preceding_newline,
            motion_target: 0,
        }
    }
}

/// Compute operator range from a motion.
///
/// # Arguments
/// * `text` - The document text
/// * `cursor` - Current cursor offset
/// * `motion` - The motion to compute
/// * `count` - The count for the motion
/// * `resolve_motion` - Closure that dispatches a motion to get a result.
///   Injected by the caller (execution layer) to avoid `commands → dispatch` dependency.
///
/// # Returns
/// * `Some(RangeResult)` if the motion succeeded
/// * `None` if the motion failed
#[allow(
    clippy::too_many_arguments,
    reason = "motion resolution requires all context pieces — splitting would obscure the dispatch contract"
)]
pub fn compute_motion_range<'text>(
    text: &'text str,
    cursor: usize,
    motion: Motion,
    count: u32,
    search: Option<(&'text str, crate::primitives::Direction)>,
    last_find: Option<crate::primitives::LastFind>,
    options: &'text crate::primitives::VimOptions,
    resolve_motion: impl Fn(Motion, &MotionContext<'_>) -> MotionResult,
    viewport: Option<crate::commands::motions::types::ViewportInfo>,
) -> Option<RangeResult> {
    compute_motion_range_with_sticky(
        text,
        cursor,
        motion,
        count,
        search,
        last_find,
        options,
        resolve_motion,
        viewport,
        None,
    )
}

/// Like [`compute_motion_range`] but accepts an optional `VirtualColumn`
/// (curswant) for vertical motions. Used by operator+motion dispatch
/// to propagate `$`-set MAXCOL through `gUgg`, `dG`, etc.
#[expect(
    clippy::too_many_arguments,
    reason = "this is the canonical motion-range entry point; all parameters are derived from independent context (text, cursor, motion, count, search, last_find, options, resolve, viewport, sticky) and bundling them would cost more than the call-site clarity it would buy"
)]
pub fn compute_motion_range_with_sticky<'text>(
    text: &'text str,
    cursor: usize,
    motion: Motion,
    count: u32,
    search: Option<(&'text str, crate::primitives::Direction)>,
    last_find: Option<crate::primitives::LastFind>,
    options: &'text crate::primitives::VimOptions,
    resolve_motion: impl Fn(Motion, &MotionContext<'_>) -> MotionResult,
    viewport: Option<crate::commands::motions::types::ViewportInfo>,
    sticky_column: Option<crate::primitives::VirtualColumn>,
) -> Option<RangeResult> {
    // Extract last_find direction before moving into context (needed for RepeatFind inclusivity)
    let last_find_direction = last_find.as_ref().and_then(|lf| lf.direction());

    // Create motion context.
    // For G/gg/%, explicit_count distinguishes d2G (line 2) from dG (last line).
    let explicit_count = count > 1
        && matches!(
            motion,
            Motion::GotoLine | Motion::GotoFirstLine | Motion::MatchingPair
        );
    let mut motion_ctx =
        MotionContext::new(text, crate::primitives::Offset::new(cursor), count, options)
            .with_explicit_count(explicit_count);
    if let Some(col) = sticky_column {
        motion_ctx = motion_ctx.with_sticky_column(col);
    }
    if let Some((pattern, forward)) = search {
        motion_ctx = motion_ctx.with_search(pattern, forward);
    }
    if let Some(last_find) = last_find {
        motion_ctx = motion_ctx.with_last_find(last_find);
    }
    if let Some(vp) = viewport {
        motion_ctx = motion_ctx.with_viewport(vp);
    }

    // Dispatch motion to get target position
    let result = resolve_motion(motion, &motion_ctx);

    match result {
        MotionResult::Position(target_offset) => {
            let mut target = target_offset.get();
            // For exclusive forward motions like `l` with count, the target may be
            // clamped to the last char on line. For operator purposes (e.g., `c5l`),
            // we need the unclamped position so the exclusive range covers all chars.
            // Re-dispatch with inclusive_end to get the unclamped position.
            if matches!(motion, Motion::Right | Motion::Space) && target < text.len() {
                let ctx2 = MotionContext::new(
                    text,
                    crate::primitives::Offset::new(cursor),
                    count,
                    options,
                )
                .with_inclusive_end(true);
                if let MotionResult::Position(t2) = resolve_motion(motion, &ctx2) {
                    target = t2.get();
                }
            }
            // Create range from cursor to target
            let (start, end) = if cursor <= target {
                (cursor, target)
            } else {
                (target, cursor)
            };

            // Determine motion type before inclusivity adjustment
            let motion_type = determine_motion_type(&motion);

            // Some motions are classified as LineWise (for register storage) but their
            // range computation is exclusive/charwise. Skip extend_to_full_lines for these
            // so the range stays exact, while the LineWise motion_type flows to registers.
            let exclusive_linewise_motion = matches!(
                motion,
                Motion::ParagraphForward
                    | Motion::ParagraphBackward
                    | Motion::DisplayDown
                    | Motion::DisplayUp
            );

            // For linewise motions (except paragraph), extend to full lines
            let (start, end) = if motion_type.is_line_wise() && !exclusive_linewise_motion {
                extend_to_full_lines(text, start, end)
            } else {
                // Handle inclusivity for charwise motions (and paragraph motions)
                // Pass last_find direction for RepeatFind/RepeatFindReverse
                let mut end = if cursor == target
                    && cursor < text.len()
                    && is_forward_exclusive_motion(&motion)
                {
                    // Vim rule: Exclusive forward motions become inclusive when cursor doesn't move.
                    // This happens for motions like `l` at end of line → delete under cursor.
                    // Backward motions at a boundary stay empty.
                    text[cursor..]
                        .chars()
                        .next()
                        .map_or(cursor, |c| cursor + c.len_utf8())
                } else {
                    adjust_for_inclusivity_with_find(text, end, &motion, last_find_direction)
                };

                // Special case: WordForward/WORDForward at end of buffer
                if matches!(motion, Motion::WordForward | Motion::WORDForward) {
                    end = adjust_word_forward_at_eof(
                        text,
                        start,
                        end,
                        target,
                        &motion,
                        count,
                        options.word_char_set(),
                    );
                }

                (start, end)
            };

            let range = Range::from_raw(start, end);

            // Apply Neovim's exclusive-to-linewise promotion rule (:h exclusive-linewise).
            // When an exclusive motion's end is at column 0:
            //   - If start is at/before first non-blank → promote to linewise
            //   - Otherwise → back up end to end of previous line (inclusive)
            //
            // Skip for paragraph/sentence/display motions — they have their own
            // semantics (paragraph/display use charwise range but linewise register;
            // sentence is exclusive and should not promote).
            let skip_promotion = exclusive_linewise_motion
                || matches!(motion, Motion::SentenceForward | Motion::SentenceBackward);

            let is_exclusive = {
                use super::inclusivity::motion_inclusivity_with_find;
                let incl = motion_inclusivity_with_find(motion, last_find_direction);
                incl.is_exclusive()
            };

            let (final_range, final_type, promoted) = if skip_promotion {
                (range, motion_type, false)
            } else {
                maybe_promote_to_linewise(text, range, motion_type, is_exclusive)
            };

            if promoted {
                Some(RangeResult::promoted(final_range, final_type).with_motion_target(target))
            } else {
                Some(RangeResult::new(final_range, final_type).with_motion_target(target))
            }
        }
        MotionResult::PositionWithType {
            offset,
            inclusivity,
        } => {
            // Custom motion with explicit type info.
            let target = offset.get();
            let (start, end) = if cursor <= target {
                (cursor, target)
            } else {
                (target, cursor)
            };

            if inclusivity.is_linewise() {
                let (ls, le) = extend_to_full_lines(text, start, end);
                Some(
                    RangeResult::new(Range::from_raw(ls, le), MotionType::LineWise)
                        .with_motion_target(target),
                )
            } else {
                // For inclusive motions, extend end by one char (like Vim inclusive motions)
                let end = if inclusivity.is_inclusive() && end < text.len() {
                    text[end..]
                        .chars()
                        .next()
                        .map_or(end, |c| end + c.len_utf8())
                } else {
                    end
                };
                Some(
                    RangeResult::new(Range::from_raw(start, end), MotionType::CharWise)
                        .with_motion_target(target),
                )
            }
        }
        MotionResult::Range { start, end } => {
            // Text-object-like range (gn/gN): bypass all inclusivity adjustments.
            // The range is exact — operator acts on [start, end) directly.
            let range = Range::from_raw(start.get(), end.get());
            let motion_type = MotionType::CharWise;
            Some(RangeResult::new(range, motion_type))
        }
        MotionResult::NeedsViewport | MotionResult::Error | MotionResult::NoMotion => None,
    }
}

/// Compute operator range for linewise operation (dd, yy, cc, etc).
///
/// # Arguments
/// * `text` - The document text
/// * `cursor` - Current cursor offset
/// * `count` - Number of lines
///
/// # Returns
/// The range covering the specified lines.
#[must_use]
pub fn compute_linewise_range(text: &str, cursor: usize, count: u32) -> RangeResult {
    // Find start of current line
    let line_start = crate::commands::helpers::line_start_for_offset(text, cursor);

    // Find end of last line
    let mut line_end = line_start;
    let mut lines_remaining = count as usize;

    for (i, c) in text[line_start..].char_indices() {
        if c == '\n' {
            lines_remaining -= 1;
            if lines_remaining == 0 {
                line_end = line_start + i + 1; // Include the newline
                break;
            }
        }
        line_end = line_start + i + c.len_utf8();
    }

    // If we hit end of file, include to the end
    if lines_remaining > 0 {
        line_end = text.len();
    }

    let at_eof = line_end >= text.len();
    let has_preceding_newline = line_start > 0;

    // Range is always [line_start, line_end) — pure line content.
    // Operators that need to handle EOF preceding newline (e.g., delete)
    // should use the at_eof and has_preceding_newline metadata.
    RangeResult::new_linewise(
        Range::from_raw(line_start, line_end),
        MotionType::LineWise,
        has_preceding_newline,
        at_eof,
    )
}

/// Compute operator range from a find motion (f/F/t/T).
///
/// Find motions are always charwise and inclusive of the target character.
/// This function handles the inclusive range computation for operators.
///
/// # Arguments
/// * `text` - The document text  
/// * `cursor` - Current cursor offset
/// * `target` - Target position from find motion
///
/// # Returns
/// * `RangeResult` with the inclusive range
#[must_use]
pub fn compute_find_range(text: &str, cursor: usize, target: usize) -> RangeResult {
    if cursor <= target {
        // Forward find: range [cursor, target+1) — inclusive of target char
        let final_end = if target < text.len() {
            text[target..]
                .chars()
                .next()
                .map_or(target, |c| target + c.len_utf8())
        } else {
            target
        };
        RangeResult::new(Range::from_raw(cursor, final_end), MotionType::CharWise)
    } else {
        // Backward find: range [target, cursor) — exclusive of cursor char
        RangeResult::new(Range::from_raw(target, cursor), MotionType::CharWise)
    }
}

/// Apply Vim's cw→ce special case to a range.
///
/// Per Vim spec: `cw` on a non-blank character behaves like `ce`,
/// stripping trailing whitespace from the range. This is Vim's historical
/// quirk that became standard behavior.
///
/// # Arguments
/// * `text` - The document text
/// * `range` - The current range to adjust
/// * `cursor` - Current cursor offset
///
/// # Returns
/// The adjusted range (or unchanged if not applicable)
#[must_use]
pub fn adjust_change_word_range(text: &str, range: Range, cursor: usize) -> Range {
    let cursor_char = text[cursor..].chars().next().unwrap_or(' ');
    if cursor_char.is_whitespace() {
        return range; // Don't strip if on whitespace
    }

    let range_text = range.slice(text);
    let trimmed = range_text.trim_end();

    if trimmed.is_empty() {
        range
    } else {
        range.with_end(range.start().saturating_add_raw(trimmed.len()))
    }
}

/// Apply Vim's `yw` cross-line trimming special case.
///
/// When `yw` word motion crosses a newline, yank should NOT include the trailing
/// whitespace/newline. Only trims if the range actually contains a newline
/// (meaning the motion wrapped to the next line).
///
/// This is pure: `(text, range) → Range`.
#[must_use]
pub fn adjust_yank_word_range(text: &str, range: Range) -> Range {
    let range_text = range.slice(text);
    if let Some(nl_pos) = range_text.find('\n') {
        let trimmed = range_text[..nl_pos].trim_end();
        // Don't trim to empty — yw on an empty line should still yank "\n"
        if trimmed.is_empty() && nl_pos == 0 {
            return range;
        }
        range.with_end(range.start().saturating_add_raw(trimmed.len()))
    } else {
        range
    }
}

/// Result of mark range computation.
#[derive(Debug, Clone)]
pub struct MarkRangeResult {
    /// The computed operator range.
    pub range: Range,
    /// Whether the range is linewise or charwise.
    pub motion_type: MotionType,
}

/// Compute operator range from a mark position.
///
/// Pure computation: orders cursor/mark offsets and optionally expands to
/// full line boundaries for linewise marks (e.g., `y'a` vs `` y`a ``).
#[must_use]
pub fn compute_mark_range(
    text: &str,
    cursor: usize,
    mark_offset: usize,
    linewise: bool,
) -> MarkRangeResult {
    use crate::commands::helpers::{line_of, line_start};

    let (start, end) = if cursor <= mark_offset {
        (cursor, mark_offset)
    } else {
        (mark_offset, cursor)
    };

    if linewise {
        // Expand to full line boundaries
        let start_line = line_of(text, start);
        let end_line = line_of(text, end);
        let ls = line_start(text, start_line).unwrap_or(0);
        // Use line_start of next line to get end including newline
        let le = line_start(text, end_line + 1).unwrap_or(text.len());
        MarkRangeResult {
            range: Range::from_raw(ls, le),
            motion_type: MotionType::LineWise,
        }
    } else {
        // Backtick marks are exclusive motions. Apply the same
        // exclusive-to-linewise promotion as regular motions: if the range
        // end is at column 0 and start is at or before the first non-blank,
        // promote to linewise (see :h exclusive-linewise).
        let (final_range, final_type, _promoted) = super::range_helpers::maybe_promote_to_linewise(
            text,
            Range::from_raw(start, end),
            MotionType::CharWise,
            true, // backtick marks are exclusive
        );
        MarkRangeResult {
            range: final_range,
            motion_type: final_type,
        }
    }
}

/// Adjust end for `WordForward`/`WORDForward` motions when the target is
/// at or near the end of the buffer.
///
/// When the motion lands at the last char because it was **clamped**
/// (cursor was already in/on the last word and `w` couldn't advance),
/// extend `end` to `text.len()` to include the entire last word.
///
/// Does **not** extend when:
/// - The motion naturally landed on the last word start from an earlier
///   position (e.g., `3dw` from `"d e f g"` naturally lands on `"g"`).
/// - The target is a different non-whitespace word class (e.g., `"old;"`
///   where `w` lands on `";"`, a valid word boundary).
#[must_use]
fn adjust_word_forward_at_eof(
    text: &str,
    start: usize,
    end: usize,
    target: usize,
    motion: &Motion,
    _count: u32,
    word_chars: &crate::primitives::WordCharSet,
) -> usize {
    use crate::commands::helpers::CharClass;

    // Find the byte offset of the last character (char-boundary safe).
    let last_char = if text.is_empty() {
        return end;
    } else {
        crate::commands::helpers::prev_char_boundary(text, text.len())
    };
    if target < last_char {
        return end;
    }

    // The `w` motion hit the last character, meaning no further word start
    // exists.  Vim's rule: when the `w` motion used in an operator cannot
    // find a next word start (it ends up on the last char), extend the
    // deletion to end-of-buffer — unless the target char starts a new word
    // class different from the cursor's class (e.g., `dw` on `abc{` where
    // target lands on `{`).

    let word_kind = if matches!(motion, Motion::WORDForward) {
        WordKind::WORD
    } else {
        WordKind::Word
    };

    let target_char = text[target..].chars().next();
    let target_is_different_word_class = target_char.is_some_and(|tc| {
        let tc_class = CharClass::classify(tc, word_kind, word_chars);
        tc_class != CharClass::Whitespace && {
            let cursor_class = text[start..]
                .chars()
                .next()
                .map(|c| CharClass::classify(c, word_kind, word_chars));
            Some(tc_class) != cursor_class
        }
    });

    // When target is on a different word class, the motion legitimately
    // stopped at a word boundary — don't extend.
    if target_is_different_word_class {
        return end;
    }

    // Check if the `w` motion actually found a valid next word start at
    // the target position, rather than just clamping to the last char.
    // If target is at a valid word start (preceded by whitespace or a class
    // change), the motion succeeded and we should NOT extend.
    // This handles `dw` on "d" in "d e" (target='e' is a valid word start)
    // and `3dw.` on "d e f g" (target='g' is a valid word start after 3 jumps).
    if target > 0 {
        let prev_pos = crate::commands::helpers::prev_char_boundary(text, target);
        let prev_class = text[prev_pos..]
            .chars()
            .next()
            .map(|c| CharClass::classify(c, word_kind, word_chars));
        let target_class_val = target_char.map(|c| CharClass::classify(c, word_kind, word_chars));
        let target_is_word_start = prev_class
            .is_some_and(|pc| target_class_val.is_some_and(|tc| tc != pc || pc.is_whitespace()));
        if target_is_word_start && target_class_val.is_some_and(|tc| tc != CharClass::Whitespace) {
            return end;
        }
    }

    // Check if the operation stays within a single line.  When the cursor
    // is on the last line (no \n after cursor), Vim extends `dw` to the
    // end of the buffer.  When the cursor is NOT on the last line, `dw`
    // only extends within the line.
    let after_cursor = &text[start..];
    let on_last_line = !after_cursor.contains('\n');

    if on_last_line {
        text.len()
    } else {
        // Multi-line case: extend to end of current line (including
        // the newline), but only if the region between cursor and
        // target doesn't cross a line boundary.
        let region = &text[start..=target.min(text.len().saturating_sub(1))];
        if region.contains('\n') {
            end
        } else {
            // Same line — extend to end of line
            text[start..]
                .find('\n')
                .map_or(text.len(), |nl| start + nl + 1)
        }
    }
}

/// Adjust end position for motion inclusivity.
///
/// Vim motions are either:
/// - **Exclusive**: End position is NOT included (w, b, $, 0, h, l)
/// - **Inclusive**: End position IS included (e, ge)
///
/// For operators, exclusive motions already point to the first char NOT deleted,
/// so we don't add. Inclusive motions point to the last char TO delete, so we
/// must add the character width.
fn adjust_for_inclusivity_with_find(
    text: &str,
    end: usize,
    motion: &Motion,
    last_find_direction: Option<crate::primitives::FindDirection>,
) -> usize {
    use super::inclusivity::motion_inclusivity_with_find;

    let inclusivity = motion_inclusivity_with_find(*motion, last_find_direction);

    // Linewise motions are handled separately by extend_to_full_lines
    // For charwise: inclusive extends by one char, exclusive doesn't
    let is_inclusive = inclusivity.is_inclusive();

    if is_inclusive && end < text.len() {
        // Move end past the current char (exclusive end)
        let next_char_boundary = text[end..]
            .chars()
            .next()
            .map_or(end, |c| end + c.len_utf8());
        next_char_boundary
    } else {
        end
    }
}

#[cfg(test)]
#[path = "range_tests.rs"]
mod tests;
