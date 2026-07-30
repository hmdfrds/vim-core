//! Character motions: h, l, 0, $, ^, g_
//!
//! Basic horizontal cursor movement within a line.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods — no dynamic dispatch.
//! Each motion is a standalone function called directly from dispatch.

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{
    first_non_blank_in_line, last_non_blank_in_line, line_content, line_count, line_end, line_of,
    line_start, move_left, move_left_on_line, move_right_on_line,
};
use crate::primitives::byte_delta;
use crate::primitives::Offset;

// ─────────────────────────────────────────────────────────────────────────────
// Character Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `h` - Move left by `[count]` characters.
///
/// If `whichwrap` contains `'h'`, wraps to the end of the previous line when
/// the cursor is already at column 0. Handles multi-line wrapping for large counts.
pub fn h(ctx: &MotionContext<'_>) -> MotionResult {
    let text = ctx.text;
    let can_wrap = ctx.options.whichwrap().contains('h');
    let mut pos = ctx.cursor.get();
    let mut remaining = ctx.count_usize();

    while remaining > 0 {
        let line = line_of(text, pos);
        let ls = line_start(text, line).unwrap_or(0);

        // Count graphemes (characters) available to move left on this line.
        let chars_available = text[ls..pos].chars().count();

        if chars_available >= remaining {
            // All remaining steps fit on the current line.
            pos = move_left_on_line(text, pos, byte_delta::to_u32(remaining));
            remaining = 0;
        } else {
            // Consume all available on this line, move to line start.
            pos = ls;
            remaining -= chars_available;

            if can_wrap && line > 0 {
                // Wrap to end of previous line. line_end returns the offset of the
                // '\n' character (or text.len() on the last line). For the previous
                // line there is always a '\n', so line_end points at it. The last
                // *content* character is one grapheme before that '\n'.
                let prev_line_end = line_end(text, line - 1).unwrap_or(0);
                let prev_ls = line_start(text, line - 1).unwrap_or(0);
                pos = if prev_ls < prev_line_end {
                    // Move one grapheme left from the '\n' to land on the last char.
                    move_left(text, prev_line_end, 1).max(prev_ls)
                } else {
                    // Previous line is empty — stay at its line start.
                    prev_ls
                };
                // The '\n' itself was crossed: consume one step of remaining.
                remaining = remaining.saturating_sub(1);
            } else {
                // Can't wrap — stop at column 0 of the current line.
                break;
            }
        }
    }

    MotionResult::Position(Offset::new(pos))
}

/// `l` - Move right by `[count]` characters.
///
/// If `whichwrap` contains `'l'`, wraps to the start of the next line when
/// the cursor is already at the end of the line. Handles multi-line wrapping
/// for large counts. Preserves the original inclusive-end clamping for
/// visual/operator mode.
pub fn l(ctx: &MotionContext<'_>) -> MotionResult {
    right_impl(ctx, 'l')
}

/// `<Space>` - Move right by `[count]` characters, wrapping when `whichwrap` contains `'s'`.
///
/// Identical to `l` except it checks for `'s'` in `whichwrap` instead of `'l'`.
pub fn space(ctx: &MotionContext<'_>) -> MotionResult {
    right_impl(ctx, 's')
}

fn right_impl(ctx: &MotionContext<'_>, ww_char: char) -> MotionResult {
    let text = ctx.text;
    let can_wrap = ctx.options.whichwrap().contains(ww_char);

    // Fast path: no wrapping — use the original single-line logic.
    if !can_wrap {
        let cursor = ctx.cursor.get();
        let line = line_of(text, cursor);
        let line_end_pos = line_end(text, line).unwrap_or(text.len());

        let new_offset = move_right_on_line(text, cursor, ctx.count);

        // In visual/operator mode (inclusive_end), cursor can reach the newline position.
        if ctx.inclusive_end {
            return MotionResult::Position(Offset::new(new_offset.min(line_end_pos)));
        }

        // Clamp to one before line end (last char, not past it) in normal mode.
        let clamped = if new_offset > 0 && new_offset >= line_end_pos && line_end_pos > 0 {
            let line_start_pos = line_start(text, line).unwrap_or(0);
            if line_start_pos < line_end_pos {
                move_left(text, line_end_pos, 1).max(line_start_pos)
            } else {
                line_start_pos
            }
        } else {
            new_offset
        };

        return MotionResult::Position(Offset::new(clamped));
    }

    // Wrapping path: step through lines as needed.
    let total_lines = line_count(text);
    let mut pos = ctx.cursor.get();
    let mut remaining = ctx.count_usize();

    while remaining > 0 {
        let line = line_of(text, pos);
        let line_end_pos = line_end(text, line).unwrap_or(text.len());

        // In normal mode the last reachable position on a line is one char before
        // the '\n'. In inclusive_end mode the '\n' itself is reachable.
        let last_reachable = if ctx.inclusive_end || line_end_pos == text.len() {
            line_end_pos
        } else if line_end_pos > 0 {
            let ls = line_start(text, line).unwrap_or(0);
            move_left(text, line_end_pos, 1).max(ls)
        } else {
            0
        };

        // How many graphemes can we move right before hitting last_reachable?
        let chars_available = text[pos..last_reachable].chars().count();

        if chars_available >= remaining {
            // All remaining steps fit on the current line.
            let new_pos = move_right_on_line(text, pos, byte_delta::to_u32(remaining));
            // Apply normal-mode clamping.
            let clamped =
                if !ctx.inclusive_end && new_pos > 0 && new_pos >= line_end_pos && line_end_pos > 0
                {
                    let ls = line_start(text, line).unwrap_or(0);
                    if ls < line_end_pos {
                        move_left(text, line_end_pos, 1).max(ls)
                    } else {
                        ls
                    }
                } else {
                    new_pos.min(line_end_pos)
                };
            pos = clamped;
            remaining = 0;
        } else {
            // Consume all available on this line.
            remaining -= chars_available;
            pos = last_reachable;

            // Try to wrap to the next line.
            let next_line = line + 1;
            if next_line < total_lines {
                // The '\n' at line_end_pos is the wrap boundary.
                // Moving past it costs one step.
                if remaining > 0 {
                    remaining -= 1;
                } else {
                    break;
                }
                pos = line_start(text, next_line).unwrap_or(pos);
            } else {
                // No next line — stop here.
                break;
            }
        }
    }

    MotionResult::Position(Offset::new(pos))
}

/// `0` - First column of line (exclusive motion).
pub fn zero(ctx: &MotionContext<'_>) -> MotionResult {
    let line = line_of(ctx.text, ctx.cursor.get());
    MotionResult::Position(Offset::new(line_start(ctx.text, line).unwrap_or(0)))
}

/// `$` - End of line (inclusive motion).
/// With count, `{count}$` goes to end of count-1 lines below.
pub fn dollar(ctx: &MotionContext<'_>) -> MotionResult {
    let current_line = line_of(ctx.text, ctx.cursor.get());
    let total_lines = line_count(ctx.text);

    // {count}$ goes count-1 lines down, then to end of that line
    let target_line = if total_lines == 0 {
        0
    } else {
        (current_line + ctx.count_usize() - 1).min(total_lines - 1)
    };

    let end = line_end(ctx.text, target_line).unwrap_or(ctx.text.len());

    // In visual/operator mode (inclusive_end), cursor can reach the newline position
    if ctx.inclusive_end {
        return MotionResult::Position(Offset::new(end));
    }

    // For $ motion in normal mode, go to last character, not past it
    if end > 0 {
        let line_start_pos = line_start(ctx.text, target_line).unwrap_or(0);
        if line_start_pos < end {
            let last_char = crate::commands::helpers::move_left(ctx.text, end, 1);
            return MotionResult::Position(Offset::new(last_char.max(line_start_pos)));
        }
    }

    MotionResult::Position(Offset::new(end))
}

/// `^` - First non-blank character of line (exclusive motion).
pub fn caret(ctx: &MotionContext<'_>) -> MotionResult {
    let line = line_of(ctx.text, ctx.cursor.get());
    let content = match line_content(ctx.text, line) {
        Some(c) => c,
        None => return MotionResult::Error,
    };

    let relative_offset = first_non_blank_in_line(content);
    let line_start_pos = line_start(ctx.text, line).unwrap_or(0);

    MotionResult::Position(Offset::new(line_start_pos + relative_offset))
}

/// `g_` - Last non-blank character of line (inclusive motion).
pub fn g_underscore(ctx: &MotionContext<'_>) -> MotionResult {
    let current_line = line_of(ctx.text, ctx.cursor.get());
    let total = line_count(ctx.text);
    let line = (current_line + ctx.count_usize() - 1).min(total.saturating_sub(1));
    let content = match line_content(ctx.text, line) {
        Some(c) => c,
        None => return MotionResult::Error,
    };

    if content.is_empty() {
        let line_start_pos = line_start(ctx.text, line).unwrap_or(0);
        return MotionResult::Position(Offset::new(line_start_pos));
    }

    let relative_offset = last_non_blank_in_line(content);
    let line_start_pos = line_start(ctx.text, line).unwrap_or(0);

    MotionResult::Position(Offset::new(line_start_pos + relative_offset))
}

#[cfg(test)]
#[path = "char_tests.rs"]
mod tests;
