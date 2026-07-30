//! Tests for `ZeroWidthMatcher` — zero-width assertions and buffer positions.

use crate::ir::{ColumnSpec, LineSpec, MarkRel};
use crate::matchers::{MatchContext, MockLineResolver, MockMarkResolver, ZeroWidthMatcher};

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — START/END OF LINE
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn sol_at_position_zero() {
    let m = ZeroWidthMatcher::StartOfLine;
    let ctx = MatchContext::simple("hello");
    assert_eq!(m.matches("hello", 0, &ctx), Some(0));
}

#[test]
fn sol_after_newline() {
    let m = ZeroWidthMatcher::StartOfLine;
    let ctx = MatchContext::simple("a\nb");
    assert_eq!(m.matches("a\nb", 2, &ctx), Some(0));
}

#[test]
fn sol_in_middle_of_line() {
    let m = ZeroWidthMatcher::StartOfLine;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 1, &ctx), None);
}

#[test]
fn eol_before_newline() {
    let m = ZeroWidthMatcher::EndOfLine;
    let ctx = MatchContext::simple("a\n");
    assert_eq!(m.matches("a\n", 1, &ctx), Some(0));
}

#[test]
fn eol_at_end_of_text() {
    let m = ZeroWidthMatcher::EndOfLine;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 3, &ctx), Some(0));
}

#[test]
fn eol_in_middle_of_line() {
    let m = ZeroWidthMatcher::EndOfLine;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 1, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — START/END OF FILE
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn sof_at_zero() {
    let m = ZeroWidthMatcher::StartOfFile;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 0, &ctx), Some(0));
}

#[test]
fn sof_not_at_zero() {
    let m = ZeroWidthMatcher::StartOfFile;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 1, &ctx), None);
}

#[test]
fn eof_at_end() {
    let m = ZeroWidthMatcher::EndOfFile;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 3, &ctx), Some(0));
}

#[test]
fn eof_not_at_end() {
    let m = ZeroWidthMatcher::EndOfFile;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — WORD BOUNDARIES
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn word_boundary_start_at_word_beginning() {
    let m = ZeroWidthMatcher::WordBoundaryStart;
    // "hello world" — word starts at 0 and 6
    let ctx = MatchContext::simple("hello world");
    assert_eq!(m.matches("hello world", 0, &ctx), Some(0));
    assert_eq!(m.matches("hello world", 6, &ctx), Some(0));
}

#[test]
fn word_boundary_start_in_middle_of_word() {
    let m = ZeroWidthMatcher::WordBoundaryStart;
    let ctx = MatchContext::simple("hello");
    assert_eq!(m.matches("hello", 2, &ctx), None);
}

#[test]
fn word_boundary_start_at_non_word() {
    let m = ZeroWidthMatcher::WordBoundaryStart;
    let ctx = MatchContext::simple(" hello");
    assert_eq!(m.matches(" hello", 0, &ctx), None);
}

#[test]
fn word_boundary_end_at_word_ending() {
    let m = ZeroWidthMatcher::WordBoundaryEnd;
    // "hello world" — word ends at 5 and 11
    let ctx = MatchContext::simple("hello world");
    assert_eq!(m.matches("hello world", 5, &ctx), Some(0));
    assert_eq!(m.matches("hello world", 11, &ctx), Some(0));
}

#[test]
fn word_boundary_end_in_middle_of_word() {
    let m = ZeroWidthMatcher::WordBoundaryEnd;
    let ctx = MatchContext::simple("hello");
    assert_eq!(m.matches("hello", 2, &ctx), None);
}

#[test]
fn word_boundary_end_at_start() {
    let m = ZeroWidthMatcher::WordBoundaryEnd;
    let ctx = MatchContext::simple("hello");
    // No previous character, so can't be end of word
    assert_eq!(m.matches("hello", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — SET MATCH START/END (always succeed)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn set_match_start_always_matches() {
    let m = ZeroWidthMatcher::SetMatchStart;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 0, &ctx), Some(0));
    assert_eq!(m.matches("abc", 2, &ctx), Some(0));
}

#[test]
fn set_match_end_always_matches() {
    let m = ZeroWidthMatcher::SetMatchEnd;
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 0, &ctx), Some(0));
    assert_eq!(m.matches("abc", 3, &ctx), Some(0));
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — CURSOR POSITION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn cursor_position_at_cursor() {
    let m = ZeroWidthMatcher::CursorPosition;
    let mut ctx = MatchContext::simple("hello");
    ctx.cursor = Some(3);
    assert_eq!(m.matches("hello", 3, &ctx), Some(0));
}

#[test]
fn cursor_position_not_at_cursor() {
    let m = ZeroWidthMatcher::CursorPosition;
    let mut ctx = MatchContext::simple("hello");
    ctx.cursor = Some(3);
    assert_eq!(m.matches("hello", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — VISUAL AREA
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn visual_area_inside_selection() {
    let m = ZeroWidthMatcher::VisualArea;
    let mut ctx = MatchContext::simple("hello world");
    ctx.visual_range = Some((2, 7));
    assert_eq!(m.matches("hello world", 3, &ctx), Some(0));
    assert_eq!(m.matches("hello world", 2, &ctx), Some(0));
    assert_eq!(m.matches("hello world", 7, &ctx), Some(0));
}

#[test]
fn visual_area_outside_selection() {
    let m = ZeroWidthMatcher::VisualArea;
    let mut ctx = MatchContext::simple("hello world");
    ctx.visual_range = Some((2, 7));
    assert_eq!(m.matches("hello world", 0, &ctx), None);
    assert_eq!(m.matches("hello world", 8, &ctx), None);
}

#[test]
fn visual_area_no_selection() {
    let m = ZeroWidthMatcher::VisualArea;
    let ctx = MatchContext::simple("hello world");
    assert_eq!(m.matches("hello world", 3, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — AT LINE
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_line_exact() {
    let m = ZeroWidthMatcher::AtLine(LineSpec::Exact(2));
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (5, 2, 1, 1), (10, 3, 1, 1)], 1);
    let mut ctx = MatchContext::simple("line1\nline2\nline3");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("line1\nline2\nline3", 5, &ctx), Some(0));
    assert_eq!(m.matches("line1\nline2\nline3", 0, &ctx), None);
}

#[test]
fn at_line_before() {
    let m = ZeroWidthMatcher::AtLine(LineSpec::Before(3));
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (5, 2, 1, 1), (10, 3, 1, 1)], 1);
    let mut ctx = MatchContext::simple("line1\nline2\nline3");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("line1\nline2\nline3", 0, &ctx), Some(0));
    assert_eq!(m.matches("line1\nline2\nline3", 5, &ctx), Some(0));
    assert_eq!(m.matches("line1\nline2\nline3", 10, &ctx), None);
}

#[test]
fn at_line_after() {
    let m = ZeroWidthMatcher::AtLine(LineSpec::After(1));
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (5, 2, 1, 1)], 1);
    let mut ctx = MatchContext::simple("line1\nline2");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("line1\nline2", 5, &ctx), Some(0));
    assert_eq!(m.matches("line1\nline2", 0, &ctx), None);
}

#[test]
fn at_line_current() {
    let m = ZeroWidthMatcher::AtLine(LineSpec::Current);
    let resolver = MockLineResolver::new(
        vec![(0, 1, 1, 1), (5, 2, 1, 1)],
        2, // cursor is on line 2
    );
    let mut ctx = MatchContext::simple("line1\nline2");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("line1\nline2", 5, &ctx), Some(0));
    assert_eq!(m.matches("line1\nline2", 0, &ctx), None);
}

#[test]
fn at_line_no_resolver() {
    let m = ZeroWidthMatcher::AtLine(LineSpec::Exact(1));
    let ctx = MatchContext::simple("hello");
    assert_eq!(m.matches("hello", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — AT COLUMN
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_column_exact() {
    let m = ZeroWidthMatcher::AtColumn(ColumnSpec::Exact(3));
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 2), (2, 1, 3, 3)], 1);
    let mut ctx = MatchContext::simple("abc");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("abc", 2, &ctx), Some(0));
    assert_eq!(m.matches("abc", 0, &ctx), None);
}

#[test]
fn at_column_before() {
    let m = ZeroWidthMatcher::AtColumn(ColumnSpec::Before(3));
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 2), (2, 1, 3, 3)], 1);
    let mut ctx = MatchContext::simple("abc");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("abc", 0, &ctx), Some(0));
    assert_eq!(m.matches("abc", 1, &ctx), Some(0));
    assert_eq!(m.matches("abc", 2, &ctx), None);
}

#[test]
fn at_column_after() {
    let m = ZeroWidthMatcher::AtColumn(ColumnSpec::After(1));
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 2)], 1);
    let mut ctx = MatchContext::simple("ab");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("ab", 1, &ctx), Some(0));
    assert_eq!(m.matches("ab", 0, &ctx), None);
}

#[test]
fn at_column_no_resolver() {
    let m = ZeroWidthMatcher::AtColumn(ColumnSpec::Exact(1));
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — AT VIRTUAL COLUMN
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_vcol_exact() {
    let m = ZeroWidthMatcher::AtVirtualColumn(ColumnSpec::Exact(9));
    // Tab at col 1 expands to vcol 8, then next char is at vcol 9
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 9)], 1);
    let mut ctx = MatchContext::simple("\tx");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("\tx", 1, &ctx), Some(0));
    assert_eq!(m.matches("\tx", 0, &ctx), None);
}

#[test]
fn at_vcol_before() {
    let m = ZeroWidthMatcher::AtVirtualColumn(ColumnSpec::Before(5));
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 2), (2, 1, 3, 5)], 1);
    let mut ctx = MatchContext::simple("abc");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("abc", 0, &ctx), Some(0));
    assert_eq!(m.matches("abc", 1, &ctx), Some(0));
    assert_eq!(m.matches("abc", 2, &ctx), None);
}

#[test]
fn at_vcol_after() {
    let m = ZeroWidthMatcher::AtVirtualColumn(ColumnSpec::After(2));
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 2), (2, 1, 3, 5)], 1);
    let mut ctx = MatchContext::simple("abc");
    ctx.line_resolver = Some(&resolver);
    assert_eq!(m.matches("abc", 2, &ctx), Some(0));
    assert_eq!(m.matches("abc", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — AT MARK
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_mark_at() {
    let m = ZeroWidthMatcher::AtMark {
        mark: 'a',
        rel: MarkRel::At,
    };
    let resolver = MockMarkResolver::new(vec![('a', 5)]);
    let mut ctx = MatchContext::simple("hello world");
    ctx.mark_resolver = Some(&resolver);
    assert_eq!(m.matches("hello world", 5, &ctx), Some(0));
    assert_eq!(m.matches("hello world", 3, &ctx), None);
}

#[test]
fn at_mark_before() {
    let m = ZeroWidthMatcher::AtMark {
        mark: 'b',
        rel: MarkRel::Before,
    };
    let resolver = MockMarkResolver::new(vec![('b', 5)]);
    let mut ctx = MatchContext::simple("hello world");
    ctx.mark_resolver = Some(&resolver);
    assert_eq!(m.matches("hello world", 3, &ctx), Some(0));
    assert_eq!(m.matches("hello world", 5, &ctx), None);
    assert_eq!(m.matches("hello world", 7, &ctx), None);
}

#[test]
fn at_mark_after() {
    let m = ZeroWidthMatcher::AtMark {
        mark: 'c',
        rel: MarkRel::After,
    };
    let resolver = MockMarkResolver::new(vec![('c', 5)]);
    let mut ctx = MatchContext::simple("hello world");
    ctx.mark_resolver = Some(&resolver);
    assert_eq!(m.matches("hello world", 7, &ctx), Some(0));
    assert_eq!(m.matches("hello world", 5, &ctx), None);
    assert_eq!(m.matches("hello world", 3, &ctx), None);
}

#[test]
fn at_mark_no_resolver() {
    let m = ZeroWidthMatcher::AtMark {
        mark: 'a',
        rel: MarkRel::At,
    };
    let ctx = MatchContext::simple("hello");
    assert_eq!(m.matches("hello", 0, &ctx), None);
}

#[test]
fn at_mark_unset_mark() {
    let m = ZeroWidthMatcher::AtMark {
        mark: 'x',
        rel: MarkRel::At,
    };
    let resolver = MockMarkResolver::new(vec![('a', 5)]); // 'x' not in resolver
    let mut ctx = MatchContext::simple("hello");
    ctx.mark_resolver = Some(&resolver);
    assert_eq!(m.matches("hello", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — CURRENT COLUMN (`\%.c`)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_column_current_matches_cursor_column() {
    let m = ZeroWidthMatcher::AtColumn(ColumnSpec::Current);
    // cursor at byte offset 2, which has col=3 per the resolver
    let resolver = MockLineResolver::new(
        vec![(0, 1, 1, 1), (1, 1, 2, 2), (2, 1, 3, 3), (3, 1, 4, 4)],
        1,
    );
    let mut ctx = MatchContext::simple("abcd");
    ctx.cursor = Some(2);
    ctx.line_resolver = Some(&resolver);
    // pos=2 has col=3, cursor has col=3 → match
    assert_eq!(m.matches("abcd", 2, &ctx), Some(0));
    // pos=0 has col=1, cursor has col=3 → no match
    assert_eq!(m.matches("abcd", 0, &ctx), None);
    // pos=3 has col=4, cursor has col=3 → no match
    assert_eq!(m.matches("abcd", 3, &ctx), None);
}

#[test]
fn at_column_current_no_resolver() {
    let m = ZeroWidthMatcher::AtColumn(ColumnSpec::Current);
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — CURRENT VIRTUAL COLUMN (`\%.v`)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_vcol_current_matches_cursor_vcol() {
    let m = ZeroWidthMatcher::AtVirtualColumn(ColumnSpec::Current);
    // Tab at offset 0 has vcol=1, char at offset 1 has vcol=9
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 9), (2, 1, 3, 10)], 1);
    let mut ctx = MatchContext::simple("\txy");
    ctx.cursor = Some(1); // cursor at offset 1, vcol=9
    ctx.line_resolver = Some(&resolver);
    // pos=1 has vcol=9, cursor has vcol=9 → match
    assert_eq!(m.matches("\txy", 1, &ctx), Some(0));
    // pos=0 has vcol=1, cursor has vcol=9 → no match
    assert_eq!(m.matches("\txy", 0, &ctx), None);
}

#[test]
fn at_vcol_current_no_resolver() {
    let m = ZeroWidthMatcher::AtVirtualColumn(ColumnSpec::Current);
    let ctx = MatchContext::simple("abc");
    assert_eq!(m.matches("abc", 0, &ctx), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — BEFORE/AFTER CURRENT LINE (`\%<.l`, `\%>.l`)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_line_before_current() {
    let m = ZeroWidthMatcher::AtLine(LineSpec::BeforeCurrent);
    // Cursor on line 3. Lines: offset 0=line1, 5=line2, 10=line3
    let resolver = MockLineResolver::new(
        vec![(0, 1, 1, 1), (5, 2, 1, 1), (10, 3, 1, 1)],
        3, // cursor line
    );
    let mut ctx = MatchContext::simple("line1\nline2\nline3");
    ctx.line_resolver = Some(&resolver);
    // Line 1 < cursor line 3 → match
    assert_eq!(m.matches("line1\nline2\nline3", 0, &ctx), Some(0));
    // Line 2 < cursor line 3 → match
    assert_eq!(m.matches("line1\nline2\nline3", 5, &ctx), Some(0));
    // Line 3 is NOT before cursor line 3 → no match
    assert_eq!(m.matches("line1\nline2\nline3", 10, &ctx), None);
}

#[test]
fn at_line_after_current() {
    let m = ZeroWidthMatcher::AtLine(LineSpec::AfterCurrent);
    // Cursor on line 1.
    let resolver = MockLineResolver::new(
        vec![(0, 1, 1, 1), (5, 2, 1, 1), (10, 3, 1, 1)],
        1, // cursor line
    );
    let mut ctx = MatchContext::simple("line1\nline2\nline3");
    ctx.line_resolver = Some(&resolver);
    // Line 1 is NOT after cursor line 1 → no match
    assert_eq!(m.matches("line1\nline2\nline3", 0, &ctx), None);
    // Line 2 > cursor line 1 → match
    assert_eq!(m.matches("line1\nline2\nline3", 5, &ctx), Some(0));
    // Line 3 > cursor line 1 → match
    assert_eq!(m.matches("line1\nline2\nline3", 10, &ctx), Some(0));
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — BEFORE/AFTER CURRENT COLUMN (`\%<.c`, `\%>.c`)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_column_before_current() {
    let m = ZeroWidthMatcher::AtColumn(ColumnSpec::BeforeCurrent);
    // cursor at byte offset 2, which has col=3 per the resolver
    let resolver = MockLineResolver::new(
        vec![(0, 1, 1, 1), (1, 1, 2, 2), (2, 1, 3, 3), (3, 1, 4, 4)],
        1,
    );
    let mut ctx = MatchContext::simple("abcd");
    ctx.cursor = Some(2);
    ctx.line_resolver = Some(&resolver);
    // pos=0 has col=1 < cursor col=3 → match
    assert_eq!(m.matches("abcd", 0, &ctx), Some(0));
    // pos=1 has col=2 < cursor col=3 → match
    assert_eq!(m.matches("abcd", 1, &ctx), Some(0));
    // pos=2 has col=3, NOT < cursor col=3 → no match
    assert_eq!(m.matches("abcd", 2, &ctx), None);
    // pos=3 has col=4, NOT < cursor col=3 → no match
    assert_eq!(m.matches("abcd", 3, &ctx), None);
}

#[test]
fn at_column_after_current() {
    let m = ZeroWidthMatcher::AtColumn(ColumnSpec::AfterCurrent);
    // cursor at byte offset 1, which has col=2 per the resolver
    let resolver = MockLineResolver::new(
        vec![(0, 1, 1, 1), (1, 1, 2, 2), (2, 1, 3, 3), (3, 1, 4, 4)],
        1,
    );
    let mut ctx = MatchContext::simple("abcd");
    ctx.cursor = Some(1);
    ctx.line_resolver = Some(&resolver);
    // pos=0 has col=1, NOT > cursor col=2 → no match
    assert_eq!(m.matches("abcd", 0, &ctx), None);
    // pos=1 has col=2, NOT > cursor col=2 → no match
    assert_eq!(m.matches("abcd", 1, &ctx), None);
    // pos=2 has col=3 > cursor col=2 → match
    assert_eq!(m.matches("abcd", 2, &ctx), Some(0));
    // pos=3 has col=4 > cursor col=2 → match
    assert_eq!(m.matches("abcd", 3, &ctx), Some(0));
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — BEFORE/AFTER CURRENT VIRTUAL COLUMN (`\%<.v`, `\%>.v`)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn at_vcol_before_current() {
    let m = ZeroWidthMatcher::AtVirtualColumn(ColumnSpec::BeforeCurrent);
    // cursor at offset 2 with vcol=10
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 9), (2, 1, 3, 10)], 1);
    let mut ctx = MatchContext::simple("\txy");
    ctx.cursor = Some(2); // vcol=10
    ctx.line_resolver = Some(&resolver);
    // pos=0 has vcol=1 < cursor vcol=10 → match
    assert_eq!(m.matches("\txy", 0, &ctx), Some(0));
    // pos=1 has vcol=9 < cursor vcol=10 → match
    assert_eq!(m.matches("\txy", 1, &ctx), Some(0));
    // pos=2 has vcol=10, NOT < cursor vcol=10 → no match
    assert_eq!(m.matches("\txy", 2, &ctx), None);
}

#[test]
fn at_vcol_after_current() {
    let m = ZeroWidthMatcher::AtVirtualColumn(ColumnSpec::AfterCurrent);
    // cursor at offset 0 with vcol=1
    let resolver = MockLineResolver::new(vec![(0, 1, 1, 1), (1, 1, 2, 9), (2, 1, 3, 10)], 1);
    let mut ctx = MatchContext::simple("\txy");
    ctx.cursor = Some(0); // vcol=1
    ctx.line_resolver = Some(&resolver);
    // pos=0 has vcol=1, NOT > cursor vcol=1 → no match
    assert_eq!(m.matches("\txy", 0, &ctx), None);
    // pos=1 has vcol=9 > cursor vcol=1 → match
    assert_eq!(m.matches("\txy", 1, &ctx), Some(0));
    // pos=2 has vcol=10 > cursor vcol=1 → match
    assert_eq!(m.matches("\txy", 2, &ctx), Some(0));
}
