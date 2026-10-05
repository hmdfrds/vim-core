//! Paragraph motions: {, }
//!
//! Navigate between paragraphs (blocks separated by empty lines).
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
use crate::commands::helpers::line_count;
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

/// The text of line `line`, without its newline. A '\r' stays: a line
/// that holds only one is not empty, as in a Vim buffer with 'fileformat'
/// unix.
#[inline]
fn line_text<'t>(text: &'t str, idx: &LineIndex, line: usize) -> &'t str {
    let start = idx.line_start(line).unwrap_or(0);
    let end = idx.line_end(line, text.len());
    text.get(start..end).unwrap_or_default()
}

/// Vim's `startPS(lnum, NUL, FALSE)`: a line `{` and `}` stop at. That is
/// an empty line, a line starting with a form feed, or an nroff paragraph
/// or section macro. A line of blanks belongs to the paragraph.
fn starts_paragraph(text: &str, idx: &LineIndex, line: usize) -> bool {
    crate::commands::helpers::starts_paragraph_or_section(line_text(text, idx, line))
}

// ─────────────────────────────────────────────────────────────────────────────
// Paragraph Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `{` - Previous paragraph.
///
/// A port of Vim's `findpar()` backward: move up past the empty lines the
/// cursor is on, then past the paragraph, and stop at the line before it
/// that starts a paragraph (Vim's `startPS()`), or at line 0.
///
/// When a fold provider is present, closed fold boundaries act as implicit
/// paragraph breaks. The motion stops at fold boundaries.
pub fn open_brace(ctx: &MotionContext<'_>) -> MotionResult {
    let mut local_index = None;
    let idx = get_or_build_index(ctx, &mut local_index);

    let mut line = idx.line_of(ctx.cursor.get());
    for _ in 0..ctx.count_usize() {
        let mut did_skip = false;
        let mut first = true;
        while line > 0 {
            if !line_text(ctx.text, idx, line).is_empty() {
                did_skip = true;
            }
            if !first && did_skip && starts_paragraph(ctx.text, idx, line) {
                break;
            }
            // Fold-aware: if the previous line is inside a fold, treat the
            // fold start as a paragraph boundary and stop before it.
            if did_skip {
                if let Some(fold) = ctx.providers.fold {
                    if fold.is_folded(crate::primitives::LineNumber::new(line - 1)) {
                        break;
                    }
                }
            }
            line -= 1;
            first = false;
        }
    }

    MotionResult::Position(Offset::new(idx.line_start(line).unwrap_or(0)))
}

/// `}` - Next paragraph.
///
/// A port of Vim's `findpar()` forward: move down past the empty lines the
/// cursor is on, then past the paragraph, and stop at the line after it
/// that starts a paragraph (Vim's `startPS()`). On the last line the
/// cursor goes to its last character; an operator then includes that
/// character, as Vim makes the motion inclusive there.
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

    let last_line = total_lines - 1;
    let mut line = idx.line_of(ctx.cursor.get());
    for _ in 0..ctx.count_usize() {
        let mut did_skip = false;
        let mut first = true;
        while line < last_line {
            if !line_text(ctx.text, idx, line).is_empty() {
                did_skip = true;
            }
            if !first && did_skip && starts_paragraph(ctx.text, idx, line) {
                break;
            }
            // Fold-aware: if the next line is inside a fold, treat the
            // fold boundary as a paragraph break and stop at it.
            if did_skip {
                if let Some(fold) = ctx.providers.fold {
                    if fold.is_folded(crate::primitives::LineNumber::new(line + 1)) {
                        line += 1;
                        break;
                    }
                }
            }
            line += 1;
            first = false;
        }
    }

    // On the last line, go to its last character.
    if line == last_line && !line_text(ctx.text, idx, line).is_empty() {
        let last_char = crate::primitives::text_util::prev_char_boundary(ctx.text, ctx.text.len());
        return MotionResult::Position(Offset::new(last_char));
    }
    MotionResult::Position(Offset::new(idx.line_start(line).unwrap_or(0)))
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

    /// `}` does not stop at a line of blanks: Vim's findpar() only stops
    /// at empty lines (and form feeds and nroff macros).
    #[test]
    fn close_brace_passes_whitespace_only_line() {
        let text = "para1\n   \npara2\n\npara3";
        let o = opts();
        let ctx = make_ctx(text, 0, &o);
        let result = close_brace(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(16)));
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

    /// `{` does not stop at a line of blanks either.
    #[test]
    fn open_brace_passes_whitespace_only_line() {
        let text = "para0\n\npara1\n   \npara2\n";
        let o = opts();
        let ctx = make_ctx(text, 17, &o);
        let result = open_brace(&ctx);
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

    /// A line that holds only "\r" is not empty, so `{` and `}` go past
    /// it, as in Vim.
    #[test]
    fn braces_pass_lines_holding_a_carriage_return() {
        let o = opts();
        let text = "a\r\n\r\nb\r\nc";
        assert_eq!(
            close_brace(&make_ctx(text, 0, &o)),
            MotionResult::Position(Offset::new(text.len() - 1))
        );
        assert_eq!(
            open_brace(&make_ctx(text, 8, &o)),
            MotionResult::Position(Offset::new(0))
        );
    }

    /// A line starting with a form feed or an nroff macro stops `}`.
    #[test]
    fn close_brace_stops_at_form_feed_and_macro_lines() {
        let o = opts();
        let text = "para1\n\x0cpage\npara2";
        assert_eq!(
            close_brace(&make_ctx(text, 0, &o)),
            MotionResult::Position(Offset::new(6))
        );
        let text = "para1\n.PP\npara2";
        assert_eq!(
            close_brace(&make_ctx(text, 0, &o)),
            MotionResult::Position(Offset::new(6))
        );
    }

    /// On a non-empty last line `}` goes to its last character.
    #[test]
    fn close_brace_on_last_line_goes_to_last_char() {
        let o = opts();
        let text = "aa bb\ncc";
        assert_eq!(
            close_brace(&make_ctx(text, 0, &o)),
            MotionResult::Position(Offset::new(7))
        );
        let text = "aa bb\ncc\n";
        assert_eq!(
            close_brace(&make_ctx(text, 0, &o)),
            MotionResult::Position(Offset::new(9))
        );
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
