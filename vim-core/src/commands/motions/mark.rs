//! Mark motions: ', `
//!
//! Jump to marked positions in the document.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.

use arrayvec::ArrayVec;

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{first_non_blank_in_line, line_content, line_of, line_start};
use crate::primitives::Offset;

// ─────────────────────────────────────────────────────────────────────────────
// Mark Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `'{mark}` - Jump to mark line (first non-blank character).
///
/// The mark offset must be resolved and set in `ctx.mark_offset` before calling.
pub fn apostrophe(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(mark_offset) = ctx.mark_offset else {
        return MotionResult::Error;
    };

    let mark_offset_raw = mark_offset.get();
    if mark_offset_raw > ctx.text.len() {
        return MotionResult::Error;
    }

    let line = line_of(ctx.text, mark_offset_raw);
    let line_start_offset = line_start(ctx.text, line).unwrap_or(0);

    if let Some(content) = line_content(ctx.text, line) {
        let relative_offset = first_non_blank_in_line(content);
        MotionResult::Position(Offset::new(line_start_offset + relative_offset))
    } else {
        MotionResult::Position(Offset::new(line_start_offset))
    }
}

/// `` `{mark} `` - Jump to exact mark position.
///
/// The mark offset must be resolved and set in `ctx.mark_offset` before calling.
pub const fn backtick(ctx: &MotionContext<'_>) -> MotionResult {
    let Some(mark_offset) = ctx.mark_offset else {
        return MotionResult::Error;
    };

    if mark_offset.get() > ctx.text.len() {
        return MotionResult::Error;
    }

    MotionResult::Position(mark_offset)
}

/// `]'` — Jump to the next lowercase mark (by buffer position).
///
/// Iterates all set local marks (a-z), sorts by offset, then returns the
/// `count`-th mark whose offset is strictly greater than the current cursor.
/// Returns `Error` if no such mark exists.
pub fn next_mark(ctx: &MotionContext<'_>) -> MotionResult {
    let cursor = ctx.cursor.get();
    let count = ctx.count_usize();

    // Collect set marks with offset > cursor, sorted ascending.
    // At most 26 local marks (a-z), so ArrayVec avoids heap allocation.
    let mut candidates: ArrayVec<Offset, 26> = ctx
        .local_marks
        .iter()
        .filter_map(|opt| *opt)
        .filter(|off| off.get() > cursor)
        .collect();
    candidates.sort_unstable_by_key(|o| o.get());

    // Return the count-th candidate (1-indexed).
    if let Some(&target) = candidates.get(count.saturating_sub(1)) {
        // Land on the first non-blank of the target line (same as `'`).
        let mark_offset_raw = target.get();
        let line = line_of(ctx.text, mark_offset_raw);
        let line_start_offset = line_start(ctx.text, line).unwrap_or(0);
        if let Some(content) = line_content(ctx.text, line) {
            let relative_offset = first_non_blank_in_line(content);
            MotionResult::Position(Offset::new(line_start_offset + relative_offset))
        } else {
            MotionResult::Position(Offset::new(line_start_offset))
        }
    } else {
        MotionResult::Error
    }
}

/// `['` — Jump to the previous lowercase mark (by buffer position).
///
/// Iterates all set local marks (a-z), sorts by offset descending, then
/// returns the `count`-th mark whose offset is strictly less than the current
/// cursor. Returns `Error` if no such mark exists.
pub fn previous_mark(ctx: &MotionContext<'_>) -> MotionResult {
    let cursor = ctx.cursor.get();
    let count = ctx.count_usize();

    // Collect set marks with offset < cursor, sorted descending.
    // At most 26 local marks (a-z), so ArrayVec avoids heap allocation.
    let mut candidates: ArrayVec<Offset, 26> = ctx
        .local_marks
        .iter()
        .filter_map(|opt| *opt)
        .filter(|off| off.get() < cursor)
        .collect();
    candidates.sort_unstable_by_key(|o| std::cmp::Reverse(o.get()));

    // Return the count-th candidate (1-indexed).
    if let Some(&target) = candidates.get(count.saturating_sub(1)) {
        let mark_offset_raw = target.get();
        let line = line_of(ctx.text, mark_offset_raw);
        let line_start_offset = line_start(ctx.text, line).unwrap_or(0);
        if let Some(content) = line_content(ctx.text, line) {
            let relative_offset = first_non_blank_in_line(content);
            MotionResult::Position(Offset::new(line_start_offset + relative_offset))
        } else {
            MotionResult::Position(Offset::new(line_start_offset))
        }
    } else {
        MotionResult::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backtick() {
        let opts = crate::primitives::VimOptions::default();
        let text = "hello world\nfoo bar";
        let ctx =
            MotionContext::new(text, Offset::new(0), 1, &opts).with_mark_offset(Offset::new(12)); // 'f' in "foo"

        let result = backtick(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(12)));
    }

    #[test]
    fn test_apostrophe() {
        let opts = crate::primitives::VimOptions::default();
        let text = "hello world\n  foo bar";
        let ctx =
            MotionContext::new(text, Offset::new(0), 1, &opts).with_mark_offset(Offset::new(14)); // 'f' in "foo"

        let result = apostrophe(&ctx);
        // Should go to first non-blank on line 1, which is 'f' at position 14
        assert_eq!(result, MotionResult::Position(Offset::new(14)));
    }

    #[test]
    fn test_apostrophe_with_leading_whitespace() {
        let opts = crate::primitives::VimOptions::default();
        let text = "hello\n    bar";
        let ctx =
            MotionContext::new(text, Offset::new(0), 1, &opts).with_mark_offset(Offset::new(10)); // 'b' in "bar"

        let result = apostrophe(&ctx);
        // Should go to first non-blank 'b' at position 10
        assert_eq!(result, MotionResult::Position(Offset::new(10)));
    }

    #[test]
    fn test_no_mark_offset_fails() {
        let opts = crate::primitives::VimOptions::default();
        let text = "hello world";
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts); // No mark_offset set

        let result = backtick(&ctx);
        assert_eq!(result, MotionResult::Error);
    }

    #[test]
    fn test_out_of_bounds_mark_fails() {
        let opts = crate::primitives::VimOptions::default();
        let text = "hello";
        let ctx =
            MotionContext::new(text, Offset::new(0), 1, &opts).with_mark_offset(Offset::new(1000));

        let result = backtick(&ctx);
        assert_eq!(result, MotionResult::Error);
    }

    // ── ]' / [' mark navigation tests ────────────────────────────────────────

    /// Build local_marks with specific (index, offset) entries.
    fn build_marks(entries: &[(usize, usize)]) -> [Option<Offset>; 26] {
        let mut marks = [None; 26];
        for &(i, off) in entries {
            marks[i] = Some(Offset::new(off));
        }
        marks
    }

    #[test]
    fn next_mark_jumps_to_next_after_cursor() {
        // Text: "line0\nline1\nline2\n"
        //        0     6     12    18
        // Marks: a=0, b=6, c=12
        let opts = crate::primitives::VimOptions::default();
        let text = "line0\nline1\nline2\n";
        let marks = build_marks(&[(0, 0), (1, 6), (2, 12)]); // a, b, c
                                                             // cursor at 6 (line1), next mark should be c at 12 (line2)
        let ctx = MotionContext::new(text, Offset::new(6), 1, &opts).with_local_marks(marks);
        let result = next_mark(&ctx);
        // Lands on first non-blank of line2 (offset 12)
        assert_eq!(result, MotionResult::Position(Offset::new(12)));
    }

    #[test]
    fn next_mark_with_count_skips_marks() {
        let opts = crate::primitives::VimOptions::default();
        let text = "a\nb\nc\nd\n";
        //          01 23 45 67
        let marks = build_marks(&[(0, 2), (1, 4), (2, 6)]); // a=2, b=4, c=6
                                                            // cursor at 0, count=2 → 2nd next mark = offset 4 (b)
        let ctx = MotionContext::new(text, Offset::new(0), 2, &opts).with_local_marks(marks);
        let result = next_mark(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(4)));
    }

    #[test]
    fn next_mark_no_marks_ahead_returns_error() {
        let opts = crate::primitives::VimOptions::default();
        let text = "hello\nworld\n";
        let marks = build_marks(&[(0, 0)]); // a=0, cursor is past it
        let ctx = MotionContext::new(text, Offset::new(6), 1, &opts).with_local_marks(marks);
        let result = next_mark(&ctx);
        assert_eq!(result, MotionResult::Error);
    }

    #[test]
    fn next_mark_no_marks_at_all_returns_error() {
        let opts = crate::primitives::VimOptions::default();
        let text = "hello";
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = next_mark(&ctx);
        assert_eq!(result, MotionResult::Error);
    }

    #[test]
    fn previous_mark_jumps_to_prev_before_cursor() {
        let opts = crate::primitives::VimOptions::default();
        let text = "line0\nline1\nline2\n";
        let marks = build_marks(&[(0, 0), (1, 6), (2, 12)]);
        // cursor at 12 (line2), previous mark = b at 6 (line1)
        let ctx = MotionContext::new(text, Offset::new(12), 1, &opts).with_local_marks(marks);
        let result = previous_mark(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    #[test]
    fn previous_mark_with_count_skips_marks() {
        let opts = crate::primitives::VimOptions::default();
        let text = "a\nb\nc\nd\n";
        let marks = build_marks(&[(0, 2), (1, 4), (2, 6)]); // a=2, b=4, c=6
                                                            // cursor at 8, count=2 → 2nd previous = b at 4
        let ctx = MotionContext::new(text, Offset::new(8), 2, &opts).with_local_marks(marks);
        let result = previous_mark(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(4)));
    }

    #[test]
    fn previous_mark_no_marks_before_returns_error() {
        let opts = crate::primitives::VimOptions::default();
        let text = "hello\nworld\n";
        let marks = build_marks(&[(0, 6)]); // a=6, cursor is before it
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_local_marks(marks);
        let result = previous_mark(&ctx);
        assert_eq!(result, MotionResult::Error);
    }
}
