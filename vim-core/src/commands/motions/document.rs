//! Document motions: gg, G, %, H, M, L
//!
//! Motions that move to specific locations in the document or screen.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{
    first_non_blank_in_line, line_content, line_count, line_of, line_start,
};
use crate::primitives::Offset;

// ─────────────────────────────────────────────────────────────────────────────
// Document Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `gg` - Go to first line (or Nth line with count).
///
/// Preserves cursor column (curswant), matching Neovim behavior.
/// When the target line is the same as the current line, cursor stays put.
pub fn gg(ctx: &MotionContext<'_>) -> MotionResult {
    let total_lines = line_count(ctx.text);
    if total_lines == 0 {
        return MotionResult::Position(Offset::new(0));
    }

    // gg with count goes to line N (1-indexed)
    let target_line = (ctx.count_usize()).saturating_sub(1).min(total_lines - 1);
    let current_line = line_of(ctx.text, ctx.cursor.get());

    if target_line == current_line {
        return MotionResult::Position(ctx.cursor);
    }

    super::line::position_at_column(ctx, target_line)
}

/// `G` - Go to last line (or Nth line with count).
///
/// Preserves cursor column (curswant), matching Neovim behavior.
/// When the target line is the same as the current line, cursor stays put.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn G(ctx: &MotionContext<'_>) -> MotionResult {
    let total_lines = line_count(ctx.text);
    if total_lines == 0 {
        return MotionResult::Position(Offset::new(0));
    }

    // G without count = last line, with explicit count = line N (1-indexed)
    let target_line = if ctx.explicit_count {
        // Explicit count: go to line N
        (ctx.count_usize()).saturating_sub(1).min(total_lines - 1)
    } else {
        // No count: go to last line
        total_lines - 1
    };

    let current_line = line_of(ctx.text, ctx.cursor.get());
    if target_line == current_line {
        return MotionResult::Position(ctx.cursor);
    }

    super::line::position_at_column(ctx, target_line)
}

/// `%` with count - Go to percentage of file.
pub fn percent(ctx: &MotionContext<'_>) -> MotionResult {
    let total_lines = line_count(ctx.text);
    if total_lines == 0 {
        return MotionResult::Position(Offset::new(0));
    }

    let percent = ctx.count.min(100) as usize;
    // Vim formula: (total_lines * percent + 99) / 100 gives 1-indexed line
    let target_line_1indexed = (total_lines * percent).div_ceil(100);
    let target_line = target_line_1indexed.saturating_sub(1).min(total_lines - 1);
    goto_line_first_non_blank(ctx.text, target_line)
}

/// `H` - Top of visible screen.
///
/// With `nostartofline` (Neovim default), preserves cursor column.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn H(ctx: &MotionContext<'_>) -> MotionResult {
    match ctx.viewport {
        Some(viewport) => {
            // H goes to first visible line + (count - 1)
            let offset = (ctx.count_usize()).saturating_sub(1);
            let target_line = viewport.first_line + offset;
            let total = line_count(ctx.text);
            let clamped = target_line.min(total.saturating_sub(1));
            goto_line_preserve_column(ctx.text, clamped, ctx.cursor.get())
        }
        None => MotionResult::NeedsViewport,
    }
}

/// `M` - Middle of visible screen.
///
/// With `nostartofline` (Neovim default), preserves cursor column.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn M(ctx: &MotionContext<'_>) -> MotionResult {
    match ctx.viewport {
        Some(viewport) => {
            // M goes to middle of visible area, clamped to actual content
            let total = line_count(ctx.text);
            let last_visible = (viewport.first_line + viewport.height.saturating_sub(1))
                .min(total.saturating_sub(1));
            let middle = usize::midpoint(viewport.first_line, last_visible);
            goto_line_preserve_column(ctx.text, middle, ctx.cursor.get())
        }
        None => MotionResult::NeedsViewport,
    }
}

/// `L` - Bottom of visible screen.
///
/// With `nostartofline` (Neovim default), preserves cursor column.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn L(ctx: &MotionContext<'_>) -> MotionResult {
    match ctx.viewport {
        Some(viewport) => {
            // L goes to last visible line - (count - 1), clamped to actual content
            let total = line_count(ctx.text);
            let last_visible = (viewport.first_line + viewport.height.saturating_sub(1))
                .min(total.saturating_sub(1));
            let offset = (ctx.count_usize()).saturating_sub(1);
            let target_line = last_visible.saturating_sub(offset);
            goto_line_preserve_column(ctx.text, target_line, ctx.cursor.get())
        }
        None => MotionResult::NeedsViewport,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Scroll Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `Ctrl-D` - Scroll half page down.
///
/// Moves cursor down by half the window height (or count lines if specified).
/// Cursor goes to first non-blank of the target line.
pub fn ctrl_d(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(viewport) = ctx.viewport else {
        return MotionResult::NeedsViewport;
    };
    let total = line_count(ctx.text);
    let current = line_of(ctx.text, ctx.cursor.get());
    let scroll_amount = scroll_half_amount(ctx, viewport.height);
    let target = (current + scroll_amount).min(total.saturating_sub(1));
    goto_line_first_non_blank(ctx.text, target)
}

/// `Ctrl-U` - Scroll half page up.
pub fn ctrl_u(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(viewport) = ctx.viewport else {
        return MotionResult::NeedsViewport;
    };
    let current = line_of(ctx.text, ctx.cursor.get());
    let scroll_amount = scroll_half_amount(ctx, viewport.height);
    let target = current.saturating_sub(scroll_amount);
    goto_line_first_non_blank(ctx.text, target)
}

/// Compute scroll amount for half-page scrolls.
///
/// Priority: explicit count > sticky count > half viewport height.
const fn scroll_half_amount(ctx: &MotionContext<'_>, viewport_height: usize) -> usize {
    if ctx.explicit_count {
        ctx.count_usize()
    } else if let Some(sticky) = ctx.scroll_half_count {
        sticky as usize
    } else {
        viewport_height / 2
    }
}

/// `Ctrl-F` - Scroll full page forward.
pub fn ctrl_f(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(viewport) = ctx.viewport else {
        return MotionResult::NeedsViewport;
    };
    let total = line_count(ctx.text);
    let current = line_of(ctx.text, ctx.cursor.get());
    // Full page = height - 2 (Vim keeps 2 lines of context), at least 1
    let scroll_amount = viewport
        .height
        .saturating_sub(2)
        .max(1)
        .saturating_mul(ctx.count_usize());
    let target = (current + scroll_amount).min(total.saturating_sub(1));
    goto_line_first_non_blank(ctx.text, target)
}

/// `Ctrl-B` - Scroll full page backward.
pub fn ctrl_b(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(viewport) = ctx.viewport else {
        return MotionResult::NeedsViewport;
    };
    // If viewport is already at top, cursor doesn't move
    if viewport.first_line == 0 {
        return MotionResult::Position(ctx.cursor);
    }
    let current = line_of(ctx.text, ctx.cursor.get());
    let scroll_amount = viewport
        .height
        .saturating_sub(2)
        .max(1)
        .saturating_mul(ctx.count_usize());
    let target = current.saturating_sub(scroll_amount);
    goto_line_first_non_blank(ctx.text, target)
}

/// `Ctrl-E` - Scroll viewport down one line.
///
/// Unlike Ctrl-D, this scrolls the viewport without necessarily moving the cursor.
/// However, if the cursor would go above the visible area, it moves down to stay visible.
/// In our test harness (small docs that fit on screen), it effectively moves cursor down 1 line.
pub fn ctrl_e(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(viewport) = ctx.viewport else {
        return MotionResult::NeedsViewport;
    };
    let total = line_count(ctx.text);
    let current = line_of(ctx.text, ctx.cursor.get());
    let scroll = ctx.count_usize();
    // New topline after scrolling
    let new_topline = (viewport.first_line + scroll).min(total.saturating_sub(1));
    // If cursor is above new topline, move it down
    let target = if current < new_topline {
        new_topline
    } else {
        current
    };
    goto_line_first_non_blank(ctx.text, target)
}

/// `Ctrl-Y` - Scroll viewport up one line.
pub fn ctrl_y(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(viewport) = ctx.viewport else {
        return MotionResult::NeedsViewport;
    };
    let total = line_count(ctx.text);
    let current = line_of(ctx.text, ctx.cursor.get());
    let scroll = ctx.count_usize();
    // New bottom of viewport after scrolling up
    let new_topline = viewport.first_line.saturating_sub(scroll);
    let new_botline =
        (new_topline + viewport.height.saturating_sub(1)).min(total.saturating_sub(1));
    // If cursor is below new bottom, move it up
    let target = if current > new_botline {
        new_botline
    } else {
        current
    };
    goto_line_first_non_blank(ctx.text, target)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Go to a line and preserve current cursor column (nostartofline behavior).
///
/// Keeps the same byte column as the cursor has in its current line.
/// If the target line is shorter, clamps to the last character.
fn goto_line_preserve_column(text: &str, line: usize, cursor: usize) -> MotionResult {
    let target_start = line_start(text, line).unwrap_or(0);
    let target_content_len = line_content(text, line).map_or(0, str::len);

    // Current column = cursor offset minus start of current line
    let current_line = line_of(text, cursor);
    let current_line_start = line_start(text, current_line).unwrap_or(0);
    let col = cursor - current_line_start;

    // Clamp column to target line length (last char for normal mode)
    let clamped_col = col.min(target_content_len.saturating_sub(1));
    MotionResult::Position(Offset::new(target_start + clamped_col))
}

/// Go to a line and position at first non-blank character.
fn goto_line_first_non_blank(text: &str, line: usize) -> MotionResult {
    let line_start_pos = line_start(text, line).unwrap_or(0);

    if let Some(content) = line_content(text, line) {
        let non_blank = first_non_blank_in_line(content);
        MotionResult::Position(Offset::new(line_start_pos + non_blank))
    } else {
        MotionResult::Position(Offset::new(line_start_pos))
    }
}
