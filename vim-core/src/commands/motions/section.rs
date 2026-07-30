//! Section motions: `[[`, `]]`, `][`, `[]`
//!
//! Navigate between section boundaries — lines whose first character is `{` or `}`.
//!
//! In Vim, `]]` / `[[` jump to the next/previous line that starts with `{`,
//! and `][` / `[]` jump to the next/previous line that starts with `}`.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{line_content, line_count, line_of, line_start};
use crate::primitives::Offset;

// ─────────────────────────────────────────────────────────────────────────────
// Section Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `]]` — Forward to next line starting with `{`.
///
/// Respects count: `2]]` skips to the 2nd such boundary.
pub fn section_forward_start(ctx: &MotionContext<'_>) -> MotionResult {
    section_scan(ctx, ScanDirection::Forward, '{')
}

/// `[[` — Backward to previous line starting with `{`.
///
/// Respects count: `2[[` skips to the 2nd such boundary.
pub fn section_backward_start(ctx: &MotionContext<'_>) -> MotionResult {
    section_scan(ctx, ScanDirection::Backward, '{')
}

/// `][` — Forward to next line starting with `}`.
///
/// Respects count: `2][` skips to the 2nd such boundary.
pub fn section_forward_end(ctx: &MotionContext<'_>) -> MotionResult {
    section_scan(ctx, ScanDirection::Forward, '}')
}

/// `[]` — Backward to previous line starting with `}`.
///
/// Respects count: `2[]` skips to the 2nd such boundary.
pub fn section_backward_end(ctx: &MotionContext<'_>) -> MotionResult {
    section_scan(ctx, ScanDirection::Backward, '}')
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared implementation
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum ScanDirection {
    Forward,
    Backward,
}

/// Core section-scanning logic.
///
/// Scans lines in `dir` searching for lines that contain `target_char`.
/// In Neovim, `[[`/`]]`/`[]`/`][` find `{`/`}` even when not at column 0.
/// Repeats `count` times, then returns the offset OF the brace character
/// (not the line start).
fn section_scan(ctx: &MotionContext<'_>, dir: ScanDirection, target_char: char) -> MotionResult {
    let total_lines = line_count(ctx.text);
    if total_lines == 0 {
        return MotionResult::Position(Offset::new(0));
    }

    let start_line = line_of(ctx.text, ctx.cursor.get());
    let mut remaining = ctx.count_usize();
    let mut found_offset: Option<usize> = None;

    match dir {
        ScanDirection::Forward => {
            // Start scanning from the line *after* cursor (skip current line)
            let mut scan = start_line.saturating_add(1);
            while scan < total_lines && remaining > 0 {
                if let Some(off) = find_char_on_line(ctx.text, scan, target_char) {
                    found_offset = Some(off);
                    remaining -= 1;
                }
                if remaining > 0 {
                    scan += 1;
                }
            }
            if remaining > 0 {
                return MotionResult::NoMotion;
            }
        }
        ScanDirection::Backward => {
            if start_line == 0 {
                return MotionResult::NoMotion;
            }
            let mut scan = start_line - 1;
            loop {
                if let Some(off) = find_char_on_line(ctx.text, scan, target_char) {
                    found_offset = Some(off);
                    remaining -= 1;
                    if remaining == 0 {
                        break;
                    }
                }
                if scan == 0 {
                    break;
                }
                scan -= 1;
            }
            if remaining > 0 {
                return MotionResult::NoMotion;
            }
        }
    }

    match found_offset {
        Some(offset) => MotionResult::Position(Offset::new(offset)),
        None => MotionResult::NoMotion,
    }
}

/// Find `target_char` on a given line and return its absolute byte offset.
///
/// Returns the position of the first occurrence of `target_char` on the line,
/// or `None` if it's not found.
fn find_char_on_line(text: &str, line_idx: usize, target_char: char) -> Option<usize> {
    let ls = line_start(text, line_idx)?;
    let content = line_content(text, line_idx)?;
    for (i, c) in content.char_indices() {
        if c == target_char {
            return Some(ls + i);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Offset, VimOptions};

    // ── ]] section_forward_start ────────────────────────────────────────────

    #[test]
    fn section_forward_start_finds_next_open_brace_line() {
        // Line 0: "hello"    offset 0
        // Line 1: "{world"   offset 6
        // Line 2: "end"      offset 13
        let text = "hello\n{world\nend";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = section_forward_start(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    #[test]
    fn section_forward_start_skips_current_line() {
        // Cursor is already on a `{` line — ]] should find the *next* one
        let text = "{first\n{second\nother";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = section_forward_start(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(7)));
    }

    #[test]
    fn section_forward_start_no_match_returns_no_motion() {
        let text = "hello\nworld\n";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = section_forward_start(&ctx);
        assert_eq!(result, MotionResult::NoMotion);
    }

    #[test]
    fn section_forward_start_respects_count() {
        // Line 0: "a"      offset 0
        // Line 1: "{one"   offset 2
        // Line 2: "{two"   offset 7
        // Line 3: "end"    offset 12
        let text = "a\n{one\n{two\nend";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 2, &opts);
        let result = section_forward_start(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(7)));
    }

    // ── [[ section_backward_start ───────────────────────────────────────────

    #[test]
    fn section_backward_start_finds_prev_open_brace_line() {
        // Line 0: "{first"   offset 0
        // Line 1: "middle"   offset 7
        // Line 2: "end"      offset 14
        let text = "{first\nmiddle\nend";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(14), 1, &opts);
        let result = section_backward_start(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn section_backward_start_skips_current_line() {
        // Cursor on a `{` line — [[ should go to the previous one
        let text = "{first\n{second\nother";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(7), 1, &opts);
        let result = section_backward_start(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn section_backward_start_no_match_returns_no_motion() {
        let text = "hello\nworld";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(6), 1, &opts);
        let result = section_backward_start(&ctx);
        assert_eq!(result, MotionResult::NoMotion);
    }

    #[test]
    fn section_backward_start_respects_count() {
        // Line 0: "{a"    offset 0
        // Line 1: "{b"    offset 3
        // Line 2: "end"   offset 6
        let text = "{a\n{b\nend";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(6), 2, &opts);
        let result = section_backward_start(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    // ── ][ section_forward_end ──────────────────────────────────────────────

    #[test]
    fn section_forward_end_finds_next_close_brace_line() {
        // Line 0: "hello"   offset 0
        // Line 1: "}world"  offset 6
        let text = "hello\n}world";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = section_forward_end(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    #[test]
    fn section_forward_end_no_match_returns_no_motion() {
        let text = "hello\nworld";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = section_forward_end(&ctx);
        assert_eq!(result, MotionResult::NoMotion);
    }

    // ── [] section_backward_end ─────────────────────────────────────────────

    #[test]
    fn section_backward_end_finds_prev_close_brace_line() {
        // Line 0: "}first"   offset 0
        // Line 1: "middle"   offset 7
        let text = "}first\nmiddle";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(7), 1, &opts);
        let result = section_backward_end(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn section_backward_end_no_match_returns_no_motion() {
        let text = "hello\nworld";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(6), 1, &opts);
        let result = section_backward_end(&ctx);
        assert_eq!(result, MotionResult::NoMotion);
    }

    #[test]
    fn section_backward_end_at_first_line_returns_no_motion() {
        // Cursor on first line — can't go further back
        let text = "}first\nmiddle";
        let opts = VimOptions::default();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = section_backward_end(&ctx);
        assert_eq!(result, MotionResult::NoMotion);
    }
}
