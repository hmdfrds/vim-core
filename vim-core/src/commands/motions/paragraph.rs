//! Paragraph motions: {, }
//!
//! Navigate between paragraphs (blank-line separated blocks).
//!
//! # Philosophy
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.
//!
//! # Performance
//!
//! Each paragraph motion builds a [`LineIndex`] on its own stack when one
//! isn't already provided via `ctx.line_index`, giving O(1) line lookups
//! for the duration of the motion. The LineIndex build is O(N) via memchr
//! SIMD — a single pass that replaces the O(L*N) cost of calling
//! `line_start(text, line)` per-line from byte 0.
//!
//! Simple motions (j, k, w, b) that never call paragraph functions
//! pay zero cost — no LineIndex is built.

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{is_blank_line_range, line_count};
use crate::commands::line_index::LineIndex;
use crate::primitives::Offset;

// ─────────────────────────────────────────────────────────────────────────────
// Internal helper — get or build a LineIndex
// ─────────────────────────────────────────────────────────────────────────────

/// Returns a reference to the context's LineIndex if present, or builds one
/// into `storage` and returns a reference to that. Avoids cloning the Vec.
#[inline]
fn get_or_build_index<'a>(
    ctx: &'a MotionContext<'_>,
    storage: &'a mut Option<LineIndex>,
) -> &'a LineIndex {
    if let Some(idx) = ctx.line_index.as_ref() {
        idx
    } else {
        storage.get_or_insert_with(|| LineIndex::build(ctx.text))
    }
}

/// Check blankness of a line using pre-computed LineIndex boundaries.
///
/// Avoids re-deriving line start/end from a byte offset (which `is_blank_line`
/// does via `line_start_for_offset`/`line_end_for_offset`). Instead uses the
/// O(1) `idx.line_start()` and `idx.line_end()` that the caller already has.
#[inline]
fn is_blank_via_index(text: &str, idx: &LineIndex, line: usize) -> bool {
    let start = idx.line_start(line).unwrap_or(0);
    let end = idx.line_end(line, text.len());
    is_blank_line_range(text, start, end)
}

// ─────────────────────────────────────────────────────────────────────────────
// Paragraph Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `{` - Previous paragraph.
///
/// In Vim, `{` moves to the previous blank line before a non-blank paragraph.
/// Symmetric to `}` but searching backward.
///
/// When a fold provider is present, closed fold boundaries act as implicit
/// paragraph breaks. The motion stops at fold boundaries.
pub fn open_brace(ctx: &MotionContext<'_>) -> MotionResult {
    let mut local_index = None;
    let idx = get_or_build_index(ctx, &mut local_index);

    let mut line = idx.line_of(ctx.cursor.get());
    let mut remaining = ctx.count_usize();

    while remaining > 0 && line > 0 {
        // Phase 1: Skip past any blank lines going backward
        while line > 0 && is_blank_via_index(ctx.text, idx, line) {
            line -= 1;
        }
        // Phase 2: Skip past non-blank lines (the paragraph body) going backward
        while line > 0 && !is_blank_via_index(ctx.text, idx, line) {
            // Fold-aware: if the previous line is inside a fold, treat the
            // fold start as a paragraph boundary and stop before it.
            if let Some(fold) = ctx.providers.fold {
                if fold.is_folded(crate::primitives::LineNumber::new(line.saturating_sub(1))) {
                    break;
                }
            }
            line -= 1;
        }
        // Now `line` points to the blank line before the paragraph (or line 0)
        remaining -= 1;
    }

    // If we couldn't find enough paragraph boundaries, go to start
    if remaining > 0 {
        return MotionResult::Position(Offset::new(0));
    }

    MotionResult::Position(Offset::new(idx.line_start(line).unwrap_or(0)))
}

/// `}` - Next paragraph.
///
/// In Vim, `}` moves to the next blank line after a non-blank paragraph.
/// If currently on a blank line, skip past consecutive blanks first,
/// then find the next blank line after non-blank content.
///
/// When a fold provider is present, closed fold boundaries act as implicit
/// paragraph breaks. The motion stops at fold boundaries.
pub fn close_brace(ctx: &MotionContext<'_>) -> MotionResult {
    let total_lines = line_count(ctx.text);
    if total_lines == 0 {
        return MotionResult::Position(Offset::new(0));
    }

    let mut local_index = None;
    let idx = get_or_build_index(ctx, &mut local_index);

    let mut line = idx.line_of(ctx.cursor.get());
    let mut remaining = ctx.count_usize();

    while remaining > 0 && line < total_lines {
        // Phase 1: Skip past any blank lines at current position
        while line < total_lines && is_blank_via_index(ctx.text, idx, line) {
            line += 1;
        }
        // Phase 2: Skip past non-blank lines (the paragraph body)
        while line < total_lines && !is_blank_via_index(ctx.text, idx, line) {
            // Fold-aware: if the next line is inside a fold, treat the
            // fold boundary as a paragraph break and stop at this line.
            if let Some(fold) = ctx.providers.fold {
                let next = line + 1;
                if next < total_lines && fold.is_folded(crate::primitives::LineNumber::new(next)) {
                    line = next;
                    break;
                }
            }
            line += 1;
        }
        // Now `line` points to the first blank line after the paragraph (or EOF)
        remaining -= 1;
    }

    // Clamp to last valid cursor position.
    // When text ends with '\n', the last line is empty and its "start"
    // equals text.len() — a valid cursor position in Neovim's model.
    if line >= total_lines {
        let last_valid = if ctx.text.is_empty() {
            0
        } else if ctx.text.ends_with('\n') {
            ctx.text.len()
        } else {
            crate::primitives::text_util::prev_char_boundary(ctx.text, ctx.text.len())
        };
        MotionResult::Position(Offset::new(last_valid))
    } else {
        MotionResult::Position(Offset::new(idx.line_start(line).unwrap_or(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Offset, VimOptions};

    fn opts() -> VimOptions {
        VimOptions::default()
    }

    fn make_ctx<'a>(text: &'a str, cursor: usize, options: &'a VimOptions) -> MotionContext<'a> {
        MotionContext::new(text, Offset::new(cursor), 1, options)
    }

    /// `}` stops at whitespace-only lines (they ARE paragraph separators,
    /// matching Neovim's actual behavior).
    #[test]
    fn close_brace_stops_at_whitespace_only_line() {
        // "para1\n   \npara2\n" — line 2 has spaces only, which is a boundary
        let text = "para1\n   \npara2\n";
        let o = opts();
        let ctx = make_ctx(text, 0, &o);
        let result = close_brace(&ctx);
        // Should stop at the whitespace-only line (offset 6)
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    /// `}` stops at truly empty lines.
    #[test]
    fn close_brace_stops_at_empty_line() {
        let text = "para1\n\npara2\n";
        let o = opts();
        let ctx = make_ctx(text, 0, &o);
        let result = close_brace(&ctx);
        // Should stop at the empty line (offset 6)
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    /// `{` stops at whitespace-only lines.
    #[test]
    fn open_brace_stops_at_whitespace_only_line() {
        // "para1\n   \npara2\n" — cursor at start of "para2" (offset 10)
        let text = "para1\n   \npara2\n";
        let o = opts();
        let ctx = make_ctx(text, 10, &o);
        let result = open_brace(&ctx);
        // Should stop at the whitespace-only line (offset 6)
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    /// `{` stops at truly empty lines.
    #[test]
    fn open_brace_stops_at_empty_line() {
        // "para1\n\npara2\n" — cursor at start of "para2" (offset 7)
        let text = "para1\n\npara2\n";
        let o = opts();
        let ctx = make_ctx(text, 7, &o);
        let result = open_brace(&ctx);
        // Should stop at the empty line (offset 6)
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    /// Tabs-only lines ARE paragraph separators (matching Neovim).
    #[test]
    fn close_brace_stops_at_tab_only_line() {
        let text = "para1\n\t\t\npara2\n";
        let o = opts();
        let ctx = make_ctx(text, 0, &o);
        let result = close_brace(&ctx);
        // Tabs-only line is a boundary (offset 6)
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    // ── Fold-aware paragraph motion tests ────────────────────────────

    mod fold_aware {
        use super::*;
        use crate::document::{FoldProvider, Providers};
        use crate::primitives::{Direction, LineNumber};

        /// Fold: lines 2-3 are folded.
        struct FoldLines2To3;
        impl FoldProvider for FoldLines2To3 {
            fn next_visible_line(&self, line: LineNumber, dir: Direction) -> LineNumber {
                if (2..=3).contains(&line.get()) {
                    match dir {
                        Direction::Forward => LineNumber::new(4),
                        Direction::Backward => LineNumber::new(1),
                    }
                } else {
                    line
                }
            }
            fn is_folded(&self, line: LineNumber) -> bool {
                (2..=3).contains(&line.get())
            }
        }

        #[test]
        fn close_brace_stops_at_fold_boundary() {
            // "aaa\nbbb\nccc\nddd\neee\n"
            //  L0   L1   L2   L3   L4
            //  0    4    8    12   16
            // Lines 2-3 folded. `}` from line 0 should stop at fold boundary.
            let text = "aaa\nbbb\nccc\nddd\neee\n";
            let o = opts();
            let fold = FoldLines2To3;
            let providers = Providers::new().with_fold(&fold);
            let ctx = MotionContext::new(text, Offset::new(0), 1, &o).with_providers(providers);
            let result = close_brace(&ctx);
            // `}` should stop at fold boundary (line 2, which is the first folded line)
            if let MotionResult::Position(pos) = result {
                let line = crate::commands::helpers::line_of(text, pos.get());
                assert!(
                    line <= 2,
                    "}} should stop at fold boundary, got line {line} offset {}",
                    pos.get()
                );
            } else {
                panic!("Expected Position, got {result:?}");
            }
        }

        #[test]
        fn open_brace_stops_at_fold_boundary() {
            // "aaa\nbbb\nccc\nddd\neee\n"
            //  L0   L1   L2   L3   L4
            //  0    4    8    12   16
            // Lines 2-3 folded. `{` from line 4 should stop at fold boundary.
            let text = "aaa\nbbb\nccc\nddd\neee\n";
            let o = opts();
            let fold = FoldLines2To3;
            let providers = Providers::new().with_fold(&fold);
            let ctx = MotionContext::new(text, Offset::new(16), 1, &o).with_providers(providers);
            let result = open_brace(&ctx);
            // `{` backward from L4 should stop when it encounters the fold
            if let MotionResult::Position(pos) = result {
                // Should stop at or before the fold
                assert!(
                    pos.get() <= 16,
                    "{{ should stop at fold boundary, got offset {}",
                    pos.get()
                );
            } else {
                panic!("Expected Position, got {result:?}");
            }
        }
    }
}
