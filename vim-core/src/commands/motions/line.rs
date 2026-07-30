//! Line motions: j, k, +, -, gj, gk
//!
//! Vertical cursor movement with column preservation.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{
    curswant_of, first_non_blank_in_line, line_content, line_count, line_end, line_of, line_start,
};
use crate::primitives::Offset;
use unicode_segmentation::UnicodeSegmentation;

// ─────────────────────────────────────────────────────────────────────────────
// Line Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `j` - Move down by `[count]` lines, preserving column.
///
/// When a `FoldProvider` is present, skips folded lines per Vim behavior:
/// if line 3-7 are folded and cursor is on line 2, `j` lands on line 8
/// (the first visible line after the fold). Each visible line counts as
/// one step toward the count.
pub fn j(ctx: &MotionContext<'_>) -> MotionResult {
    let cursor = ctx.cursor.get();
    let current_line = line_of(ctx.text, cursor);
    let total_lines = line_count(ctx.text);

    if total_lines == 0 {
        return MotionResult::Position(Offset::new(0));
    }

    let target_line = next_visible_line_down(
        current_line,
        ctx.count_usize(),
        total_lines,
        ctx.providers.fold,
    );

    if target_line == current_line {
        // j on the last line is an error — beeps in Vim and aborts operators/macros.
        return MotionResult::Error;
    }

    position_at_column(ctx, target_line)
}

/// `k` - Move up by `[count]` lines, preserving column.
///
/// When a `FoldProvider` is present, skips folded lines backward.
pub fn k(ctx: &MotionContext<'_>) -> MotionResult {
    let cursor = ctx.cursor.get();
    let current_line = line_of(ctx.text, cursor);

    if current_line == 0 {
        return MotionResult::Error;
    }

    let target_line = next_visible_line_up(current_line, ctx.count_usize(), ctx.providers.fold);

    position_at_column(ctx, target_line)
}

/// Walk down `count` visible lines from `current`, skipping folded regions.
///
/// Without a fold provider, this is plain `(current + count).min(max - 1)`.
/// With a provider, each step lands on the next visible line, and folded
/// blocks are jumped in one step.
fn next_visible_line_down(
    current: usize,
    count: usize,
    total_lines: usize,
    fold: Option<&dyn crate::document::FoldProvider>,
) -> usize {
    let Some(fold) = fold else {
        return (current + count).min(total_lines - 1);
    };

    let mut line = current;
    for _ in 0..count {
        let next = line + 1;
        if next >= total_lines {
            break;
        }
        // Skip to first visible line at or after `next`
        line = fold
            .next_visible_line(
                crate::primitives::LineNumber::new(next),
                crate::primitives::Direction::Forward,
            )
            .get();
        if line >= total_lines {
            line = total_lines - 1;
            break;
        }
    }
    line
}

/// Walk up `count` visible lines from `current`, skipping folded regions.
fn next_visible_line_up(
    current: usize,
    count: usize,
    fold: Option<&dyn crate::document::FoldProvider>,
) -> usize {
    let Some(fold) = fold else {
        return current.saturating_sub(count);
    };

    let mut line = current;
    for _ in 0..count {
        if line == 0 {
            break;
        }
        let prev = line - 1;
        // Skip to first visible line at or before `prev`
        line = fold
            .next_visible_line(
                crate::primitives::LineNumber::new(prev),
                crate::primitives::Direction::Backward,
            )
            .get();
    }
    line
}

/// Place cursor at the sticky column on `target_line`, preserving column.
///
/// Uses virtual columns (curswant / `coladvance` semantics) to match
/// Neovim's behavior. This correctly handles tab characters: a tab at
/// byte 0 with tabstop=4 occupies vcols 0-3, and `coladvance(3)` lands
/// on that tab character (byte 0).
///
/// Public within the motions module so `G`/`gg` can reuse it.
pub(super) fn position_at_column(ctx: &MotionContext<'_>, target_line: usize) -> MotionResult {
    let tabstop = ctx.options.tabstop();
    let vcol = ctx.sticky_column.unwrap_or_else(|| {
        crate::primitives::VirtualColumn::new(curswant_of(ctx.text, ctx.cursor.get(), tabstop))
    });

    let target_line_start = line_start(ctx.text, target_line).unwrap_or(0);
    let target_line_end = line_end(ctx.text, target_line).unwrap_or(ctx.text.len());
    let target_line_text = &ctx.text[target_line_start..target_line_end];

    // For END_OF_LINE ($ motion), clamp to last character on line.
    // In visual/operator mode (inclusive_end), land past the last character
    // (at the newline position) so block selections extend to EOL.
    if vcol.is_end_of_line() {
        let target_graphemes = grapheme_count_in_line(target_line_text);
        let target_col = if ctx.inclusive_end {
            target_graphemes
        } else {
            target_graphemes.saturating_sub(1)
        };
        let new_offset = byte_offset_for_column(ctx.text, target_line_start, target_col);
        return MotionResult::Position(Offset::new(new_offset));
    }

    // Use vcol_to_byte to find the byte position for the target virtual column.
    // vcol_to_byte returns the byte offset of the first char whose vcol >= target.
    // For coladvance semantics on a tab: if target_vcol falls within a tab's span,
    // we want to land ON the tab character (its byte offset), not past it.
    let byte_in_line = coladvance_byte(target_line_text, vcol.get(), tabstop);
    let new_offset = target_line_start + byte_in_line.min(target_line_text.len().saturating_sub(1));

    MotionResult::Position(Offset::new(new_offset))
}

/// `+` or `<Enter>` - Move down `[count]` lines to first non-blank.
pub fn plus(ctx: &MotionContext<'_>) -> MotionResult {
    let cursor = ctx.cursor.get();
    let current_line = line_of(ctx.text, cursor);
    let total_lines = line_count(ctx.text);

    if total_lines == 0 {
        return MotionResult::Position(Offset::new(0));
    }

    let target_line = (current_line + ctx.count_usize()).min(total_lines - 1);
    let line_start_pos = line_start(ctx.text, target_line).unwrap_or(0);

    if let Some(content) = line_content(ctx.text, target_line) {
        let non_blank_offset = first_non_blank_in_line(content);
        MotionResult::Position(Offset::new(line_start_pos + non_blank_offset))
    } else {
        MotionResult::Position(Offset::new(line_start_pos))
    }
}

/// `-` - Move up `[count]` lines to first non-blank.
pub fn minus(ctx: &MotionContext<'_>) -> MotionResult {
    let cursor = ctx.cursor.get();
    let current_line = line_of(ctx.text, cursor);
    let target_line = current_line.saturating_sub(ctx.count_usize());
    let line_start_pos = line_start(ctx.text, target_line).unwrap_or(0);

    if let Some(content) = line_content(ctx.text, target_line) {
        let non_blank_offset = first_non_blank_in_line(content);
        MotionResult::Position(Offset::new(line_start_pos + non_blank_offset))
    } else {
        MotionResult::Position(Offset::new(line_start_pos))
    }
}

/// `_` - First non-blank of [count-1] lines down (1_ = current line).
pub fn underscore(ctx: &MotionContext<'_>) -> MotionResult {
    let cursor = ctx.cursor.get();
    let current_line = line_of(ctx.text, cursor);
    let total_lines = line_count(ctx.text);

    // _ with count N goes to line current + (N-1)
    let lines_down = (ctx.count_usize()).saturating_sub(1);
    let target_line = if total_lines == 0 {
        0
    } else {
        (current_line + lines_down).min(total_lines - 1)
    };

    let line_start_pos = line_start(ctx.text, target_line).unwrap_or(0);

    if let Some(content) = line_content(ctx.text, target_line) {
        let non_blank_offset = first_non_blank_in_line(content);
        MotionResult::Position(Offset::new(line_start_pos + non_blank_offset))
    } else {
        MotionResult::Position(Offset::new(line_start_pos))
    }
}

/// `gj` - Move down by `[count]` display lines.
///
/// When a `DisplayLineProvider` is present, navigates soft-wrapped sub-lines.
/// A single long physical line that wraps into 3 display lines can be traversed
/// one display line at a time. Without a provider, falls back to physical `j`.
///
/// Column tracking uses the display sub-line column (not the physical line
/// column), matching Vim's behavior where gj/gk preserve column within the
/// visible screen width.
pub fn gj(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(display) = ctx.providers.display_lines else {
        return j(ctx);
    };

    let current_line = line_of(ctx.text, ctx.cursor.get());
    let ls = line_start(ctx.text, current_line).unwrap_or(0);
    let line_text = line_content(ctx.text, current_line).unwrap_or("");
    let cursor_in_line = ctx.cursor.get() - ls;

    let Some((sub_line, display_col)) = display.byte_to_display_col(line_text, cursor_in_line)
    else {
        return j(ctx);
    };

    let target_col = ctx
        .sticky_column
        .map_or(display_col, crate::primitives::VirtualColumn::get);
    let total_lines = line_count(ctx.text);

    let mut line = current_line;
    let mut sub = sub_line;
    let mut remaining = ctx.count_usize();
    let mut cur_line_text = line_text;

    while remaining > 0 {
        let sub_count = display.display_line_count(cur_line_text);
        if sub + 1 < sub_count {
            sub += 1;
        } else {
            let next_line = line + 1;
            if next_line >= total_lines {
                break;
            }
            line = next_line;
            sub = 0;
            cur_line_text = line_content(ctx.text, line).unwrap_or("");
        }
        remaining -= 1;
    }

    let target_ls = line_start(ctx.text, line).unwrap_or(0);
    let target_text = if line == current_line {
        line_text
    } else {
        line_content(ctx.text, line).unwrap_or("")
    };
    display
        .display_col_to_byte(target_text, sub, target_col)
        .map(|b| Offset::new((target_ls + b).min(ctx.text.len())))
        .map_or_else(|| position_at_column(ctx, line), MotionResult::Position)
}

/// `gk` - Move up by `[count]` display lines.
///
/// When a `DisplayLineProvider` is present, navigates soft-wrapped sub-lines.
/// Without a provider, falls back to physical `k`.
pub fn gk(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(display) = ctx.providers.display_lines else {
        return k(ctx);
    };

    let current_line = line_of(ctx.text, ctx.cursor.get());
    let ls = line_start(ctx.text, current_line).unwrap_or(0);
    let line_text = line_content(ctx.text, current_line).unwrap_or("");
    let cursor_in_line = ctx.cursor.get() - ls;

    let Some((sub_line, display_col)) = display.byte_to_display_col(line_text, cursor_in_line)
    else {
        return k(ctx);
    };

    let target_col = ctx
        .sticky_column
        .map_or(display_col, crate::primitives::VirtualColumn::get);

    let mut line = current_line;
    let mut sub = sub_line;
    let mut remaining = ctx.count_usize();

    while remaining > 0 {
        if sub > 0 {
            sub -= 1;
        } else if line > 0 {
            line -= 1;
            let prev_line_text = line_content(ctx.text, line).unwrap_or("");
            sub = display.display_line_count(prev_line_text).saturating_sub(1);
        } else {
            break;
        }
        remaining -= 1;
    }

    let target_ls = line_start(ctx.text, line).unwrap_or(0);
    let target_text = if line == current_line {
        line_text
    } else {
        line_content(ctx.text, line).unwrap_or("")
    };
    display
        .display_col_to_byte(target_text, sub, target_col)
        .map(|b| Offset::new((target_ls + b).min(ctx.text.len())))
        .map_or_else(|| position_at_column(ctx, line), MotionResult::Position)
}

/// `g0` - Move to the start of the current display sub-line.
pub fn g0(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(display) = ctx.providers.display_lines else {
        return crate::commands::motions::char::zero(ctx);
    };
    let line = line_of(ctx.text, ctx.cursor.get());
    let ls = line_start(ctx.text, line).unwrap_or(0);
    let line_text = line_content(ctx.text, line).unwrap_or("");
    let cursor_in_line = ctx.cursor.get() - ls;
    let Some((sub_line, _)) = display.byte_to_display_col(line_text, cursor_in_line) else {
        return crate::commands::motions::char::zero(ctx);
    };
    display
        .display_col_to_byte(line_text, sub_line, 0)
        .map_or_else(
            || crate::commands::motions::char::zero(ctx),
            |b| MotionResult::Position(Offset::new(ls + b)),
        )
}

/// `g$` - Move to the end of the current display sub-line.
/// On the last sub-line, behaves like `$` (end of physical line).
pub fn g_dollar(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(display) = ctx.providers.display_lines else {
        return crate::commands::motions::char::dollar(ctx);
    };
    let line = line_of(ctx.text, ctx.cursor.get());
    let ls = line_start(ctx.text, line).unwrap_or(0);
    let line_text = line_content(ctx.text, line).unwrap_or("");
    let cursor_in_line = ctx.cursor.get() - ls;
    let Some((sub_line, _)) = display.byte_to_display_col(line_text, cursor_in_line) else {
        return crate::commands::motions::char::dollar(ctx);
    };
    let sub_count = display.display_line_count(line_text);
    if sub_line + 1 >= sub_count {
        return crate::commands::motions::char::dollar(ctx);
    }
    // Find byte offset of next sub-line start, back up one grapheme.
    let next_sub_start = display.display_col_to_byte(line_text, sub_line + 1, 0);
    match next_sub_start {
        Some(next_byte) if next_byte > 0 => {
            let prefix = &line_text[..next_byte];
            let last_grapheme_start = prefix
                .graphemes(true)
                .map(str::len)
                .scan(0usize, |acc, len| {
                    *acc += len;
                    Some(*acc - len)
                })
                .last()
                .unwrap_or(0);
            MotionResult::Position(Offset::new(ls + last_grapheme_start))
        }
        _ => crate::commands::motions::char::dollar(ctx),
    }
}

/// `g^` - Move to the first non-blank of the current display sub-line.
pub fn g_caret(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(display) = ctx.providers.display_lines else {
        return crate::commands::motions::char::caret(ctx);
    };
    let line = line_of(ctx.text, ctx.cursor.get());
    let ls = line_start(ctx.text, line).unwrap_or(0);
    let line_text = line_content(ctx.text, line).unwrap_or("");
    let cursor_in_line = ctx.cursor.get() - ls;
    let Some((sub_line, _)) = display.byte_to_display_col(line_text, cursor_in_line) else {
        return crate::commands::motions::char::caret(ctx);
    };
    let Some(sub_start) = display.display_col_to_byte(line_text, sub_line, 0) else {
        return crate::commands::motions::char::caret(ctx);
    };
    let sub_count = display.display_line_count(line_text);
    let sub_end = if sub_line + 1 < sub_count {
        display
            .display_col_to_byte(line_text, sub_line + 1, 0)
            .unwrap_or(line_text.len())
    } else {
        line_text.len()
    };
    let sub_text = &line_text[sub_start..sub_end];
    let first_non_blank = first_non_blank_in_line(sub_text);
    MotionResult::Position(Offset::new(ls + sub_start + first_non_blank))
}

/// `|` - Go to column N (1-indexed) on the current line.
///
/// Without count, goes to column 1 (first column, offset 0).
/// With count N, goes to column N (0-indexed offset N-1).
/// Clamps to the last character if count exceeds line length.
pub fn go_to_column(ctx: &MotionContext<'_>) -> MotionResult {
    let cursor = ctx.cursor.get();
    let current_line = line_of(ctx.text, cursor);
    let line_start_pos = line_start(ctx.text, current_line).unwrap_or(0);
    let line_end_pos = line_end(ctx.text, current_line).unwrap_or(ctx.text.len());
    let line_text = &ctx.text[line_start_pos..line_end_pos];

    // Count is 1-indexed column number, convert to 0-indexed grapheme column
    let target_col = (ctx.count_usize()).saturating_sub(1);
    // Clamp to last valid grapheme on line
    let grapheme_count = line_text.graphemes(true).filter(|g| *g != "\n").count();
    let clamped_col = target_col.min(grapheme_count.saturating_sub(1));

    MotionResult::Position(Offset::new(byte_offset_for_column(
        ctx.text,
        line_start_pos,
        clamped_col,
    )))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Count graphemes in a line slice, excluding the trailing newline.
fn grapheme_count_in_line(line: &str) -> usize {
    line.graphemes(true).filter(|g| *g != "\n").count()
}

/// Get byte offset for a given column in a line (grapheme-aware).
///
/// `target_col` is a grapheme index (each grapheme = 1 column),
/// matching the output of `column_of()`.
fn byte_offset_for_column(text: &str, line_start: usize, target_col: usize) -> usize {
    let line_text = &text[line_start..];
    let mut offset = 0;

    for (col, grapheme) in line_text.graphemes(true).enumerate() {
        if grapheme == "\n" || col >= target_col {
            break;
        }
        offset += grapheme.len();
    }

    line_start + offset
}

/// Neovim-style `coladvance`: find the byte offset within a line for a target
/// virtual column.
///
/// Unlike [`vcol_to_byte`] which returns the byte of the first character whose
/// virtual column >= target, `coladvance` lands ON a character if the target
/// virtual column falls anywhere within that character's span. This matters for
/// tab characters: a tab at byte 0 with tabstop=4 spans vcols 0-3, and
/// `coladvance(3)` should land at byte 0 (the tab), not byte 1.
///
/// If `target_vcol` is beyond the line, returns the last character's byte offset
/// (clamped to line length).
fn coladvance_byte(line: &str, target_vcol: usize, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    let mut vcol: usize = 0;
    let mut consumed: usize = 0;
    let mut last_grapheme_start: usize = 0;

    // Strip trailing newline for iteration — newline is not a visible grapheme.
    let visible = line.trim_end_matches('\n');

    for grapheme in visible.graphemes(true) {
        let g_width = crate::commands::helpers::grapheme_display_width(grapheme, vcol, tabstop);
        // If target falls within this grapheme's span, return its byte offset
        if target_vcol < vcol + g_width {
            return consumed;
        }
        vcol += g_width;
        last_grapheme_start = consumed;
        consumed += grapheme.len();
    }
    // target_vcol beyond line end: return last grapheme position
    last_grapheme_start
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{FoldProvider, Providers};
    use crate::primitives::{Direction, LineNumber};

    // ── Fold-aware j/k tests ──────────────────────────────────────────

    struct FoldLines3To5;
    impl FoldProvider for FoldLines3To5 {
        fn next_visible_line(&self, line: LineNumber, dir: Direction) -> LineNumber {
            // Lines 3, 4, 5 are folded (0-indexed)
            if (3..=5).contains(&line.get()) {
                match dir {
                    Direction::Forward => LineNumber::new(6),
                    Direction::Backward => LineNumber::new(2),
                }
            } else {
                line
            }
        }
        fn is_folded(&self, line: LineNumber) -> bool {
            (3..=5).contains(&line.get())
        }
    }

    fn text_10_lines() -> &'static str {
        "line0\nline1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\n"
    }

    #[test]
    fn j_no_fold_basic() {
        let opts = crate::primitives::VimOptions::default();
        // Line 0, cursor at 0
        let ctx = MotionContext::new(text_10_lines(), Offset::new(0), 1, &opts);
        assert_eq!(j(&ctx), MotionResult::Position(Offset::new(6))); // start of line 1
    }

    #[test]
    fn j_skips_folded_lines() {
        let opts = crate::primitives::VimOptions::default();
        let fold = FoldLines3To5;
        let providers = Providers::new().with_fold(&fold);
        // Cursor on line 2 (offset 12 = "line0\nline1\n"), each line is 6 bytes
        let ctx = MotionContext::new(text_10_lines(), Offset::new(12), 1, &opts)
            .with_providers(providers);
        let result = j(&ctx);
        // Should skip lines 3-5 and land on line 6 (offset 36 = 6*6)
        assert_eq!(result, MotionResult::Position(Offset::new(36)));
    }

    #[test]
    fn k_skips_folded_lines() {
        let opts = crate::primitives::VimOptions::default();
        let fold = FoldLines3To5;
        let providers = Providers::new().with_fold(&fold);
        // Cursor on line 6 (offset 36 = 6*6)
        let ctx = MotionContext::new(text_10_lines(), Offset::new(36), 1, &opts)
            .with_providers(providers);
        let result = k(&ctx);
        // Should skip lines 5-3 and land on line 2 (offset 12)
        assert_eq!(result, MotionResult::Position(Offset::new(12)));
    }

    #[test]
    fn j_count_with_folds() {
        let opts = crate::primitives::VimOptions::default();
        let fold = FoldLines3To5;
        let providers = Providers::new().with_fold(&fold);
        // Cursor on line 1 (offset 6), count=2
        let ctx =
            MotionContext::new(text_10_lines(), Offset::new(6), 2, &opts).with_providers(providers);
        let result = j(&ctx);
        // Step 1: line 1 → line 2 (visible). Step 2: line 2 → line 6 (skip fold).
        assert_eq!(result, MotionResult::Position(Offset::new(36)));
    }

    // ── Display-line gj/gk tests ─────────────────────────────────────

    #[test]
    fn gj_falls_back_to_j_without_provider() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("line0\nline1\nline2\n", Offset::new(0), 1, &opts);
        // No display provider → falls back to j
        let gj_result = gj(&ctx);
        let j_result = j(&ctx);
        assert_eq!(gj_result, j_result);
    }
}
