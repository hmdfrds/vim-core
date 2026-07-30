//! Miscellaneous motions
//!
//! Less common motions that don't fit other categories.
//!
//! ## Motions
//!
//! | Keys | Description |
//! |------|-------------|
//! | `\|` | Go to column N |
//! | `gm` | Go to middle of screen line |
//! | `gM` | Go to middle of text line |
//! | `go` | Go to byte N in file |

use super::types::{MiscMotion, MotionContext, MotionResult};
use crate::commands::helpers::{
    line_end, line_of, line_start, next_char_boundary, prev_char_boundary,
};
use crate::primitives::{MotionInclusivity, Offset};
use unicode_segmentation::UnicodeSegmentation;

impl MiscMotion {
    /// Compute the target offset.
    pub fn compute(&self, ctx: &MotionContext<'_>) -> MotionResult {
        match self {
            Self::MiddleOfScreenLine => middle_of_screen_line(ctx),
            Self::MiddleOfTextLine => middle_of_text_line(ctx),
            Self::GotoByte => goto_byte(ctx),
        }
    }

    /// Motion type.
    #[must_use]
    pub const fn motion_type(&self) -> MotionInclusivity {
        MotionInclusivity::Exclusive
    }
}

/// `gm` — go to middle of screen line width (grapheme-aware).
///
/// Viewport width is in display columns, so we walk graphemes to find the
/// byte offset at the target column. Clamps to last grapheme if the line
/// is shorter than half the viewport.
pub fn middle_of_screen_line(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(viewport) = ctx.viewport else {
        return MotionResult::NeedsViewport;
    };

    let line = line_of(ctx.text, ctx.cursor.get());
    let line_start_pos = line_start(ctx.text, line).unwrap_or(0);
    let line_end_pos = line_end(ctx.text, line).unwrap_or(ctx.text.len());
    let line_text = ctx.text.get(line_start_pos..line_end_pos).unwrap_or("");

    // Walk graphemes to find byte offset of column `mid_col`.
    // Each grapheme = 1 column (simplified; real Vim uses display width).
    let mid_col = viewport.width / 2;
    let mut byte_offset = 0;
    let mut prev_byte_offset = 0;
    for (col, grapheme) in line_text.graphemes(true).filter(|g| *g != "\n").enumerate() {
        if col == mid_col {
            return MotionResult::Position(Offset::new(line_start_pos + byte_offset));
        }
        prev_byte_offset = byte_offset;
        byte_offset += grapheme.len();
    }
    // Line shorter than mid_col — clamp to last grapheme start
    MotionResult::Position(Offset::new(line_start_pos + prev_byte_offset))
}

/// `gM` — go to middle of text on current line (grapheme-aware).
pub fn middle_of_text_line(ctx: &MotionContext<'_>) -> MotionResult {
    let line = line_of(ctx.text, ctx.cursor.get());
    let line_start_pos = line_start(ctx.text, line).unwrap_or(0);
    let line_end_pos = line_end(ctx.text, line).unwrap_or(ctx.text.len());
    let line_text = ctx.text.get(line_start_pos..line_end_pos).unwrap_or("");

    // Count graphemes (not bytes) to find the midpoint on a char boundary
    let grapheme_count = line_text.graphemes(true).filter(|g| *g != "\n").count();
    let middle_grapheme = grapheme_count / 2;
    let middle_byte: usize = line_text
        .graphemes(true)
        .take(middle_grapheme)
        .map(str::len)
        .sum();
    MotionResult::Position(Offset::new(line_start_pos + middle_byte))
}

/// `go` — go to byte N in file (1-based count).
///
/// Snaps to the next char boundary if the target byte falls mid-character.
/// Clamps to the last valid character position (never past-end).
pub fn goto_byte(ctx: &MotionContext<'_>) -> MotionResult {
    let target = (ctx.count_usize()).saturating_sub(1);
    // Clamp to last valid byte (not past-end), then snap to char boundary
    let max_pos = ctx.text.len().saturating_sub(1);
    let clamped = target.min(max_pos);
    let safe = next_char_boundary(ctx.text, clamped);
    // If snapping forward pushed us past-end, back up to last char start
    let final_pos = if safe >= ctx.text.len() && !ctx.text.is_empty() {
        prev_char_boundary(ctx.text, ctx.text.len().saturating_sub(1))
    } else {
        safe
    };
    MotionResult::Position(Offset::new(final_pos))
}
