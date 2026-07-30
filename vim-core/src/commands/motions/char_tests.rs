use super::*;
use crate::primitives::VimOptions;

fn opts() -> VimOptions {
    VimOptions::default()
}

fn opts_with_whichwrap(ww: &str) -> VimOptions {
    let mut o = VimOptions::default();
    o.set_whichwrap(ww);
    o
}

fn make_ctx<'a>(
    text: &'a str,
    cursor: usize,
    count: u32,
    options: &'a VimOptions,
) -> MotionContext<'a> {
    MotionContext::new(text, Offset::new(cursor), count, options)
}

// h tests

#[test]
fn h_basic_left_movement() {
    let o = opts();
    let c = make_ctx("hello", 3, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(2)));
}

#[test]
fn h_with_count() {
    let o = opts();
    let c = make_ctx("hello", 4, 3, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(1)));
}

#[test]
fn h_clamps_at_line_start() {
    let o = opts();
    let c = make_ctx("hello", 0, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn h_clamps_at_line_start_large_count() {
    let o = opts();
    let c = make_ctx("hello", 2, 100, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn h_second_line_clamps_at_line_start() {
    let o = opts();
    let c = make_ctx("hello\nworld", 6, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn h_second_line_moves_within_line() {
    let o = opts();
    let c = make_ctx("hello\nworld", 8, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(7)));
}

#[test]
fn h_unicode_multibyte() {
    let o = opts();
    let c = make_ctx("a\u{00e9}b", 3, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(1)));
}

#[test]
fn h_unicode_multibyte_with_count() {
    let o = opts();
    let c = make_ctx("a\u{00e9}b", 3, 2, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn h_empty_text() {
    let o = opts();
    let c = make_ctx("", 0, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(0)));
}

// l tests

#[test]
fn l_basic_right_movement() {
    let o = opts();
    let c = make_ctx("hello", 0, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(1)));
}

#[test]
fn l_with_count() {
    let o = opts();
    let c = make_ctx("hello", 0, 3, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(3)));
}

#[test]
fn l_clamps_at_line_end_normal_mode() {
    let o = opts();
    let c = make_ctx("hello", 4, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn l_clamps_at_line_end_large_count() {
    let o = opts();
    let c = make_ctx("hello", 0, 100, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn l_inclusive_end_reaches_newline() {
    let o = opts();
    let c = make_ctx("hello\nworld", 4, 1, &o).with_inclusive_end(true);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(5)));
}

#[test]
fn l_second_line_moves_within_line() {
    let o = opts();
    let c = make_ctx("hello\nworld", 6, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(7)));
}

#[test]
fn l_unicode_multibyte() {
    let o = opts();
    let c = make_ctx("a\u{00e9}b", 0, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(1)));
}

#[test]
fn l_unicode_skip_multibyte() {
    let o = opts();
    let c = make_ctx("a\u{00e9}b", 1, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(3)));
}

#[test]
fn l_empty_text() {
    let o = opts();
    let c = make_ctx("", 0, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(0)));
}

// zero (0) tests

#[test]
fn zero_moves_to_line_start() {
    let o = opts();
    let c = make_ctx("hello", 3, 1, &o);
    assert_eq!(zero(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn zero_at_line_start_stays() {
    let o = opts();
    let c = make_ctx("hello", 0, 1, &o);
    assert_eq!(zero(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn zero_second_line() {
    let o = opts();
    let c = make_ctx("hello\nworld", 8, 1, &o);
    assert_eq!(zero(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn zero_empty_text() {
    let o = opts();
    let c = make_ctx("", 0, 1, &o);
    assert_eq!(zero(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn zero_with_leading_whitespace() {
    let o = opts();
    let c = make_ctx("  hello", 4, 1, &o);
    assert_eq!(zero(&c), MotionResult::Position(Offset::new(0)));
}

// dollar ($) tests

#[test]
fn dollar_moves_to_last_char() {
    let o = opts();
    let c = make_ctx("hello", 0, 1, &o);
    assert_eq!(dollar(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn dollar_already_at_end() {
    let o = opts();
    let c = make_ctx("hello", 4, 1, &o);
    assert_eq!(dollar(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn dollar_second_line() {
    let o = opts();
    let c = make_ctx("hello\nworld", 6, 1, &o);
    assert_eq!(dollar(&c), MotionResult::Position(Offset::new(10)));
}

#[test]
fn dollar_with_count_goes_down() {
    let o = opts();
    let c = make_ctx("hello\nworld", 0, 2, &o);
    assert_eq!(dollar(&c), MotionResult::Position(Offset::new(10)));
}

#[test]
fn dollar_inclusive_end() {
    let o = opts();
    let c = make_ctx("hello\nworld", 0, 1, &o).with_inclusive_end(true);
    assert_eq!(dollar(&c), MotionResult::Position(Offset::new(5)));
}

#[test]
fn dollar_empty_line() {
    let o = opts();
    let c = make_ctx("hello\n\nworld", 6, 1, &o);
    assert_eq!(dollar(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn dollar_unicode() {
    let o = opts();
    let c = make_ctx("h\u{00e9}llo", 0, 1, &o);
    assert_eq!(dollar(&c), MotionResult::Position(Offset::new(5)));
}

// caret (^) tests

#[test]
fn caret_skips_whitespace() {
    let o = opts();
    let c = make_ctx("  hello", 0, 1, &o);
    assert_eq!(caret(&c), MotionResult::Position(Offset::new(2)));
}

#[test]
fn caret_no_leading_whitespace() {
    let o = opts();
    let c = make_ctx("hello", 3, 1, &o);
    assert_eq!(caret(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn caret_all_whitespace() {
    let o = opts();
    let c = make_ctx("   ", 0, 1, &o);
    assert_eq!(caret(&c), MotionResult::Position(Offset::new(2)));
}

#[test]
fn caret_tabs() {
    let o = opts();
    let c = make_ctx("\thello", 0, 1, &o);
    assert_eq!(caret(&c), MotionResult::Position(Offset::new(1)));
}

#[test]
fn caret_second_line() {
    let o = opts();
    let c = make_ctx("hello\n  world", 10, 1, &o);
    assert_eq!(caret(&c), MotionResult::Position(Offset::new(8)));
}

#[test]
fn caret_empty_text() {
    let o = opts();
    let c = make_ctx("", 0, 1, &o);
    // Empty string has line_content returning Some(""), first_non_blank returns 0
    assert_eq!(caret(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn caret_unicode_whitespace() {
    let o = opts();
    let c = make_ctx("\t caf\u{00e9}", 0, 1, &o);
    assert_eq!(caret(&c), MotionResult::Position(Offset::new(2)));
}

// g_ tests

#[test]
fn g_underscore_basic() {
    let o = opts();
    let c = make_ctx("hello  ", 0, 1, &o);
    assert_eq!(g_underscore(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn g_underscore_no_trailing_whitespace() {
    let o = opts();
    let c = make_ctx("hello", 0, 1, &o);
    assert_eq!(g_underscore(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn g_underscore_all_whitespace() {
    let o = opts();
    let c = make_ctx("   ", 0, 1, &o);
    assert_eq!(g_underscore(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn g_underscore_second_line() {
    let o = opts();
    let c = make_ctx("hello\nworld  ", 6, 1, &o);
    assert_eq!(g_underscore(&c), MotionResult::Position(Offset::new(10)));
}

#[test]
fn g_underscore_empty_line_content() {
    let o = opts();
    let c = make_ctx("hello\n\nworld", 6, 1, &o);
    assert_eq!(g_underscore(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn g_underscore_empty_text() {
    let o = opts();
    let c = make_ctx("", 0, 1, &o);
    // Empty string has line_content returning Some(""), which is empty, so returns line start
    assert_eq!(g_underscore(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn g_underscore_unicode_trailing() {
    let o = opts();
    let c = make_ctx("h\u{00e9}llo  ", 0, 1, &o);
    assert_eq!(g_underscore(&c), MotionResult::Position(Offset::new(5)));
}

// Unicode edge cases

#[test]
fn h_with_cjk_characters() {
    let o = opts();
    let c = make_ctx("\u{4f60}\u{597d}\u{4e16}", 6, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(3)));
}

#[test]
fn l_with_cjk_characters() {
    let o = opts();
    let c = make_ctx("\u{4f60}\u{597d}\u{4e16}", 0, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(3)));
}

#[test]
fn caret_with_cjk_after_spaces() {
    let o = opts();
    let c = make_ctx("  \u{4f60}\u{597d}", 0, 1, &o);
    assert_eq!(caret(&c), MotionResult::Position(Offset::new(2)));
}

#[test]
fn g_underscore_with_cjk_trailing_spaces() {
    let o = opts();
    let c = make_ctx("\u{4f60}\u{597d}  ", 0, 1, &o);
    assert_eq!(g_underscore(&c), MotionResult::Position(Offset::new(3)));
}

#[test]
fn dollar_with_emoji() {
    let o = opts();
    let c = make_ctx("a\u{1f600}b", 0, 1, &o);
    assert_eq!(dollar(&c), MotionResult::Position(Offset::new(5)));
}

#[test]
fn zero_with_multibyte_line() {
    let o = opts();
    let c = make_ctx("a\u{00e9}\n\u{4f60}\u{597d}", 7, 1, &o);
    assert_eq!(zero(&c), MotionResult::Position(Offset::new(4)));
}

// ─── whichwrap h tests ─────────────────────────────────────────────────────

#[test]
fn h_whichwrap_wraps_to_prev_line_end() {
    // "hello\nworld", cursor at 'w' (offset 6), h should wrap to 'o' (offset 4).
    let o = opts_with_whichwrap("h");
    let c = make_ctx("hello\nworld", 6, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn h_whichwrap_wraps_from_first_col_with_count() {
    // "hello\nworld", cursor at 'w' (offset 6), count=2:
    // step 1: wrap to 'o' (offset 4), step 2: move to 'l' (offset 3).
    let o = opts_with_whichwrap("h");
    let c = make_ctx("hello\nworld", 6, 2, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(3)));
}

#[test]
fn h_whichwrap_does_not_wrap_on_first_line() {
    // At start of first line — cannot wrap further back, stays at 0.
    let o = opts_with_whichwrap("h");
    let c = make_ctx("hello\nworld", 0, 1, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(0)));
}

#[test]
fn h_no_whichwrap_clamps_at_line_start() {
    // Default whichwrap="b,s" — 'h' not in it, so clamps normally.
    let o = opts();
    let c = make_ctx("hello\nworld", 6, 5, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn h_whichwrap_wraps_over_empty_line() {
    // "ab\n\ncd", cursor at 'c' (offset 4), count=1: wraps to empty line (offset 3).
    // The empty line's only position is offset 3 (line start == line end for '\n' line).
    let o = opts_with_whichwrap("h");
    let c = make_ctx("ab\n\ncd", 4, 1, &o);
    // line 2 starts at offset 4; prev line (line 1) is empty: line_start=3, line_end=3.
    // wrap to empty prev line => pos = line_start(prev) = 3.
    assert_eq!(h(&c), MotionResult::Position(Offset::new(3)));
}

#[test]
fn h_whichwrap_wraps_multiple_lines() {
    // "ab\ncd", cursor at 'c' (offset 3), count=4:
    // 'c' is at col 0, wrap to 'b' (offset 1) — 1 step to cross \n.
    // Then 'b'->offset 1 has 1 char left, then 'a'->offset 0.
    // Total: wrap (1 step) + 'b' (1) + 'a' (1) = 3 steps from count=4: pos=0.
    let o = opts_with_whichwrap("h");
    let c = make_ctx("ab\ncd", 3, 4, &o);
    assert_eq!(h(&c), MotionResult::Position(Offset::new(0)));
}

// ─── whichwrap l tests ─────────────────────────────────────────────────────

#[test]
fn l_whichwrap_wraps_to_next_line_start() {
    // "hello\nworld", cursor at 'o' (offset 4), l should wrap to 'w' (offset 6).
    let o = opts_with_whichwrap("l");
    let c = make_ctx("hello\nworld", 4, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(6)));
}

#[test]
fn l_whichwrap_wraps_with_count() {
    // "hello\nworld", cursor at 'o' (offset 4), count=2:
    // step 1: wrap to 'w' (offset 6), step 2: move to 'o' (offset 7).
    let o = opts_with_whichwrap("l");
    let c = make_ctx("hello\nworld", 4, 2, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(7)));
}

#[test]
fn l_whichwrap_does_not_wrap_past_last_line() {
    // At last char of last line — stays put.
    let o = opts_with_whichwrap("l");
    let c = make_ctx("hello\nworld", 10, 1, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(10)));
}

#[test]
fn l_no_whichwrap_clamps_at_line_end() {
    // Default whichwrap="b,s" — 'l' not in it, so clamps at last char of line.
    let o = opts();
    let c = make_ctx("hello\nworld", 4, 5, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn l_whichwrap_wraps_over_empty_line() {
    // "ab\n\ncd", cursor at 'b' (offset 1), count=2:
    // step 1: wrap from end of line 0 ('\n' at offset 2) to line 1 start (offset 3).
    // line 1 is empty, so last_reachable=3. chars_available=0 on empty line.
    // step 2: wrap from line 1 to line 2 start (offset 4) => 'c'.
    let o = opts_with_whichwrap("l");
    let c = make_ctx("ab\n\ncd", 1, 2, &o);
    assert_eq!(l(&c), MotionResult::Position(Offset::new(4)));
}

#[test]
fn l_whichwrap_inclusive_end_wraps() {
    // In inclusive_end mode, cursor can be at '\n'; wrapping still works.
    let o = opts_with_whichwrap("l");
    let c = make_ctx("hello\nworld", 4, 1, &o).with_inclusive_end(true);
    // In inclusive_end, last_reachable = line_end_pos = 5, so
    // chars_available = text[4..5].chars().count() = 1 >= remaining = 1.
    // One step right from 'o' at 4 lands on the '\n' at 5 without wrapping.
    assert_eq!(l(&c), MotionResult::Position(Offset::new(5)));
}
