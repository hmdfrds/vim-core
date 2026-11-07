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

/// The first and last line of the closed fold that holds `line`.
fn closed_fold(ctx: &MotionContext<'_>, line: usize) -> Option<(usize, usize)> {
    let fold = ctx.providers.fold?;
    let (start, end) = fold.enclosing_fold(crate::primitives::LineNumber::new(line))?;
    Some((start.get(), end.get()))
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
/// A closed fold ends a paragraph, as an empty line does, and `{` stops on
/// its first line. A closed fold the cursor starts in is passed first, as
/// `findpar()` does.
pub fn open_brace(ctx: &MotionContext<'_>) -> MotionResult {
    let mut local_index = None;
    let idx = get_or_build_index(ctx, &mut local_index);

    let mut line = idx.line_of(ctx.cursor.get());
    for _ in 0..ctx.count_usize() {
        let mut did_skip = false;
        let mut first = true;
        while line > 0 {
            if let Some((start, _)) = closed_fold(ctx, line) {
                if !first && did_skip {
                    line = start;
                    break;
                }
                if first && !line_text(ctx.text, idx, line).is_empty() {
                    did_skip = true;
                }
                line = start;
                if line == 0 {
                    break;
                }
            } else {
                if !line_text(ctx.text, idx, line).is_empty() {
                    did_skip = true;
                }
                if !first && did_skip && starts_paragraph(ctx.text, idx, line) {
                    break;
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
/// A closed fold ends a paragraph, as an empty line does, and `}` stops on
/// its first line. A closed fold the cursor starts in is passed first, as
/// `findpar()` does.
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
            if let Some((start, end)) = closed_fold(ctx, line) {
                if !first && did_skip {
                    line = start;
                    break;
                }
                if first && !line_text(ctx.text, idx, line).is_empty() {
                    did_skip = true;
                }
                if end >= last_line {
                    line = last_line;
                    break;
                }
                line = end;
            } else {
                if !line_text(ctx.text, idx, line).is_empty() {
                    did_skip = true;
                }
                if !first && did_skip && starts_paragraph(ctx.text, idx, line) {
                    break;
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

        fn with_fold(text: &str, cursor: usize, count: u32, o: &VimOptions) -> (usize, usize) {
            let fold = FoldLines2To3;
            let providers = Providers::new().with_fold(&fold);
            let ctx =
                MotionContext::new(text, Offset::new(cursor), count, o).with_providers(providers);
            let pos = |r: MotionResult| match r {
                MotionResult::Position(p) => p.get(),
                other => panic!("expected a position, got {other:?}"),
            };
            (pos(open_brace(&ctx)), pos(close_brace(&ctx)))
        }

        /// A closed fold ends a paragraph, as an empty line does, and both
        /// motions stop on its first line. Vim has no such rule; it is
        /// vim-core's own.
        #[test]
        fn closed_fold_ends_a_paragraph() {
            // Lines: aaa, bbb, [ccc, ddd folded], eee, and the empty last line.
            let text = "aaa\nbbb\nccc\nddd\neee\n";
            let o = opts();
            assert_eq!(with_fold(text, 0, 1, &o).1, 8);
            assert_eq!(with_fold(text, 16, 1, &o).0, 8);
        }

        /// From inside a closed fold, `{` and `}` first pass the fold, as
        /// Vim's `findpar()` (textobject.c) does, and the cursor's line
        /// counts as text: an empty line right after the fold stops them.
        #[test]
        fn braces_from_inside_a_closed_fold_pass_it() {
            // Lines: aaa, bbb, [ccc, ddd folded], "", eee.
            let text = "aaa\nbbb\nccc\nddd\n\neee";
            let o = opts();
            assert_eq!(with_fold(text, 8, 1, &o).1, 16);
            assert_eq!(with_fold(text, 12, 1, &o).1, 16);
            // Lines: aaa, "", [bbb, ccc folded], ddd, eee.
            let text = "aaa\n\nbbb\nccc\nddd\neee";
            assert_eq!(with_fold(text, 9, 1, &o).0, 4);
            assert_eq!(with_fold(text, 5, 1, &o).0, 4);
        }

        /// A count stops at one fold per step: the second `}` or `{` passes
        /// the fold the first one stopped on.
        #[test]
        fn counted_braces_pass_the_fold_they_stopped_on() {
            let text = "aaa\nbbb\nccc\nddd\neee\n";
            let o = opts();
            assert_eq!(with_fold(text, 0, 2, &o).1, 20);
            assert_eq!(with_fold(text, 20, 2, &o).0, 0);
        }

        /// Like empty lines, a closed fold that comes before any text is
        /// passed: `}` from an empty line above a fold goes on to the next
        /// empty line after the paragraph below it.
        #[test]
        fn close_brace_from_an_empty_line_passes_the_fold_below() {
            // Lines: aaa, "", [ccc, ddd folded], eee, "", fff.
            let text = "aaa\n\nccc\nddd\neee\n\nfff";
            let o = opts();
            assert_eq!(with_fold(text, 4, 1, &o).1, 17);
        }
    }
}
