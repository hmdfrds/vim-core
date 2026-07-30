//! Tests for the range transformation system.

use super::*;

// ─── Helper ──────────────────────────────────────────────────────────────

fn mr(start: usize, end: usize, incl: MotionInclusivity) -> MotionRange {
    MotionRange::new(Range::from_raw(start, end), incl)
}

// ─── MotionRange constructors ────────────────────────────────────────────

#[test]
fn exclusive_constructor() {
    let r = MotionRange::exclusive(Range::from_raw(0, 5));
    assert_eq!(r.inclusivity(), MotionInclusivity::Exclusive);
    assert_eq!(r.range(), Range::from_raw(0, 5));
}

#[test]
fn inclusive_constructor() {
    let r = MotionRange::inclusive(Range::from_raw(0, 5));
    assert_eq!(r.inclusivity(), MotionInclusivity::Inclusive);
}

#[test]
fn linewise_constructor() {
    let r = MotionRange::linewise(Range::from_raw(0, 5));
    assert_eq!(r.inclusivity(), MotionInclusivity::Linewise);
}

// ─── NormalizedRange accessors ───────────────────────────────────────────

#[test]
fn normalized_range_accessors() {
    let nr = NormalizedRange::new(Range::from_raw(3, 7), MotionType::CharWise);
    assert_eq!(nr.start(), Offset::new(3));
    assert_eq!(nr.end(), Offset::new(7));
    assert_eq!(nr.len(), 4);
    assert!(!nr.is_empty());
    assert_eq!(nr.motion_type(), MotionType::CharWise);
}

#[test]
fn normalized_range_empty() {
    let nr = NormalizedRange::new(Range::from_raw(5, 5), MotionType::CharWise);
    assert!(nr.is_empty());
    assert_eq!(nr.len(), 0);
}

// ─── normalize: exclusive ────────────────────────────────────────────────

#[test]
fn normalize_exclusive_passthrough() {
    let text = "hello world";
    let result = mr(0, 5, MotionInclusivity::Exclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
    assert_eq!(result.motion_type(), MotionType::CharWise);
}

#[test]
fn normalize_exclusive_clamps_to_text_len() {
    let text = "hi";
    let result = mr(0, 100, MotionInclusivity::Exclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 2));
}

#[test]
fn normalize_exclusive_empty_text() {
    let text = "";
    let result = mr(0, 0, MotionInclusivity::Exclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 0));
    assert!(result.is_empty());
}

// ─── normalize: inclusive ────────────────────────────────────────────────

#[test]
fn normalize_inclusive_extends_end_by_one() {
    // "hello" -- inclusive range [0, 4] -> normalized [0, 5)
    let text = "hello";
    let result = mr(0, 4, MotionInclusivity::Inclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
    assert_eq!(result.motion_type(), MotionType::CharWise);
}

#[test]
fn normalize_inclusive_at_end_of_text() {
    // "abc" -- inclusive end at last char [0, 2] -> [0, 3)
    let text = "abc";
    let result = mr(0, 2, MotionInclusivity::Inclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 3));
}

#[test]
fn normalize_inclusive_past_text_end() {
    let text = "abc";
    let result = mr(0, 10, MotionInclusivity::Inclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 3));
}

#[test]
fn normalize_inclusive_multibyte() {
    let text = "caf\u{00e9}"; // U+00E9 = 2-byte UTF-8
    let result = mr(0, 3, MotionInclusivity::Inclusive).normalize(text);
    // '\u{00e9}' is 2 bytes, so advancing past it goes from 3 to 5
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

#[test]
fn normalize_inclusive_single_char() {
    let text = "x";
    let result = mr(0, 0, MotionInclusivity::Inclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 1));
}

// ─── normalize: linewise ─────────────────────────────────────────────────

#[test]
fn normalize_linewise_single_line() {
    let text = "hello world\n";
    let result = mr(2, 5, MotionInclusivity::Linewise).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 12));
    assert_eq!(result.motion_type(), MotionType::LineWise);
}

#[test]
fn normalize_linewise_multi_line() {
    let text = "line1\nline2\nline3\n";
    let result = mr(2, 8, MotionInclusivity::Linewise).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 12)); // "line1\nline2\n"
}

#[test]
fn normalize_linewise_no_trailing_newline() {
    let text = "hello";
    let result = mr(0, 3, MotionInclusivity::Linewise).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

#[test]
fn normalize_linewise_last_line_no_newline() {
    let text = "line1\nline2";
    let result = mr(6, 9, MotionInclusivity::Linewise).normalize(text);
    assert_eq!(result.range(), Range::from_raw(6, 11));
}

#[test]
fn normalize_linewise_empty_text() {
    let text = "";
    let result = mr(0, 0, MotionInclusivity::Linewise).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 0));
    assert_eq!(result.motion_type(), MotionType::LineWise);
}

// ─── expand ──────────────────────────────────────────────────────────────

#[test]
fn expand_includes_trailing_whitespace() {
    let text = "hello   world";
    let result = mr(0, 5, MotionInclusivity::Exclusive).expand(text);
    assert_eq!(result.range(), Range::from_raw(0, 8)); // "hello   "
}

#[test]
fn expand_includes_trailing_tabs() {
    let text = "hello\t\tworld";
    let result = mr(0, 5, MotionInclusivity::Exclusive).expand(text);
    assert_eq!(result.range(), Range::from_raw(0, 7)); // "hello\t\t"
}

#[test]
fn expand_falls_back_to_leading_whitespace() {
    let text = "   hello";
    let result = mr(3, 8, MotionInclusivity::Exclusive).expand(text);
    assert_eq!(result.range(), Range::from_raw(0, 8)); // "   hello"
}

#[test]
fn expand_no_surrounding_whitespace() {
    let text = "helloworld";
    let result = mr(0, 5, MotionInclusivity::Exclusive).expand(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

#[test]
fn expand_at_end_of_text() {
    let text = "hello";
    let result = mr(0, 5, MotionInclusivity::Exclusive).expand(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

#[test]
fn expand_prefers_trailing_over_leading() {
    let text = "  hello  world";
    let result = mr(2, 7, MotionInclusivity::Exclusive).expand(text);
    assert_eq!(result.range(), Range::from_raw(2, 9)); // "hello  "
}

#[test]
fn expand_does_not_cross_newlines() {
    let text = "hello\nworld";
    let result = mr(0, 5, MotionInclusivity::Exclusive).expand(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

// ─── contract ────────────────────────────────────────────────────────────

#[test]
fn contract_trims_leading_whitespace() {
    let text = "   hello   ";
    let result = mr(0, 11, MotionInclusivity::Exclusive).contract(text);
    assert_eq!(result.range(), Range::from_raw(3, 8)); // "hello"
}

#[test]
fn contract_trims_trailing_whitespace() {
    let text = "hello   ";
    let result = mr(0, 8, MotionInclusivity::Exclusive).contract(text);
    assert_eq!(result.range(), Range::from_raw(0, 5)); // "hello"
}

#[test]
fn contract_trims_both_sides() {
    let text = "\t\thello\t\t";
    let result = mr(0, 9, MotionInclusivity::Exclusive).contract(text);
    assert_eq!(result.range(), Range::from_raw(2, 7)); // "hello"
}

#[test]
fn contract_no_whitespace() {
    let text = "hello";
    let result = mr(0, 5, MotionInclusivity::Exclusive).contract(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

#[test]
fn contract_all_whitespace_preserves_range() {
    let text = "     ";
    let result = mr(0, 5, MotionInclusivity::Exclusive).contract(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

#[test]
fn contract_partial_range() {
    let text = "hello   world";
    let result = mr(5, 13, MotionInclusivity::Exclusive).contract(text);
    assert_eq!(result.range(), Range::from_raw(8, 13)); // "world"
}

// ─── Inclusive + expand/contract ─────────────────────────────────────────

#[test]
fn expand_with_inclusive_motion() {
    let text = "hello   world";
    let result = mr(0, 4, MotionInclusivity::Inclusive).expand(text);
    assert_eq!(result.range(), Range::from_raw(0, 8)); // "hello   "
}

#[test]
fn contract_with_inclusive_motion() {
    let text = "   hello   ";
    let result = mr(0, 10, MotionInclusivity::Inclusive).contract(text);
    assert_eq!(result.range(), Range::from_raw(3, 8)); // "hello"
}

// ─── Linewise expand/contract ────────────────────────────────────────────

#[test]
fn expand_linewise_includes_trailing_whitespace() {
    let text = "  hello  \n  world  \n";
    let result = mr(2, 5, MotionInclusivity::Linewise).expand(text);
    // After linewise normalize: [0, 10) = "  hello  \n"
    // Trailing from 10: "  world  \n" -- spaces extend to offset 12
    assert_eq!(result.range(), Range::from_raw(0, 12));
}

#[test]
fn contract_linewise_trims_whitespace() {
    let text = "  hello  \n";
    let result = mr(0, 5, MotionInclusivity::Linewise).contract(text);
    // Normalize linewise: [0, 10) = "  hello  \n"
    // Leading: skip "  " -> start=2
    // Trailing: '\n' is not space/tab -> stops at 10
    assert_eq!(result.range(), Range::from_raw(2, 10));
}

// ─── Helper function tests ───────────────────────────────────────────────

#[test]
fn advance_past_ascii_char() {
    assert_eq!(advance_past_char("hello", 0), 1);
    assert_eq!(advance_past_char("hello", 4), 5);
}

#[test]
fn advance_past_multibyte_char() {
    let text = "\u{00e9}bc"; // e-acute (2 bytes) + "bc"
    assert_eq!(advance_past_char(text, 0), 2);
}

#[test]
fn advance_past_char_at_end() {
    assert_eq!(advance_past_char("abc", 3), 3);
    assert_eq!(advance_past_char("abc", 100), 3);
}

#[test]
fn advance_past_char_empty_text() {
    assert_eq!(advance_past_char("", 0), 0);
}

#[test]
fn expand_line_boundaries_single_line() {
    let text = "hello world\n";
    assert_eq!(expand_to_line_boundaries(text, 3, 7), (0, 12));
}

#[test]
fn expand_line_boundaries_multi_line() {
    let text = "aaa\nbbb\nccc\n";
    assert_eq!(expand_to_line_boundaries(text, 5, 6), (4, 8));
    assert_eq!(expand_to_line_boundaries(text, 1, 9), (0, 12));
}

#[test]
fn expand_line_boundaries_no_trailing_newline() {
    let text = "hello";
    assert_eq!(expand_to_line_boundaries(text, 1, 3), (0, 5));
}

#[test]
fn skip_whitespace_forward_basic() {
    assert_eq!(skip_whitespace_forward("   hello", 0), 3);
    assert_eq!(skip_whitespace_forward("hello", 0), 0);
    assert_eq!(skip_whitespace_forward("\t\thello", 0), 2);
}

#[test]
fn skip_whitespace_forward_stops_at_newline() {
    assert_eq!(skip_whitespace_forward("hello\n  world", 5), 5);
}

#[test]
fn skip_whitespace_forward_at_end() {
    assert_eq!(skip_whitespace_forward("hello", 5), 5);
    assert_eq!(skip_whitespace_forward("hello", 100), 5);
}

#[test]
fn skip_whitespace_backward_basic() {
    assert_eq!(skip_whitespace_backward("hello   ", 8), 5);
    assert_eq!(skip_whitespace_backward("hello", 5), 5);
    assert_eq!(skip_whitespace_backward("hello\t\t", 7), 5);
}

#[test]
fn skip_whitespace_backward_stops_at_newline() {
    assert_eq!(skip_whitespace_backward("hello\n   ", 9), 6);
}

#[test]
fn skip_whitespace_backward_at_start() {
    assert_eq!(skip_whitespace_backward("hello", 0), 0);
}

// ─── Edge cases ──────────────────────────────────────────────────────────

#[test]
fn normalize_zero_length_exclusive() {
    let text = "hello";
    let result = mr(3, 3, MotionInclusivity::Exclusive).normalize(text);
    assert!(result.is_empty());
    assert_eq!(result.range(), Range::from_raw(3, 3));
}

#[test]
fn normalize_zero_length_inclusive() {
    let text = "hello";
    let result = mr(3, 3, MotionInclusivity::Inclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(3, 4));
}

#[test]
fn normalize_full_text_exclusive() {
    let text = "hello";
    let result = mr(0, 5, MotionInclusivity::Exclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

#[test]
fn normalize_full_text_inclusive() {
    let text = "hello";
    let result = mr(0, 4, MotionInclusivity::Inclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 5));
}

#[test]
fn normalize_linewise_full_document() {
    let text = "line1\nline2\n";
    let result = mr(0, 11, MotionInclusivity::Linewise).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 12));
}

#[test]
fn expand_empty_range() {
    let text = "  hello  ";
    let result = mr(2, 2, MotionInclusivity::Exclusive).expand(text);
    // 'h' at offset 2 is not whitespace, no trailing. Leading "  " -> start=0
    assert_eq!(result.range(), Range::from_raw(0, 2));
}

#[test]
fn contract_empty_range_preserves() {
    let text = "hello";
    let result = mr(2, 2, MotionInclusivity::Exclusive).contract(text);
    assert_eq!(result.range(), Range::from_raw(2, 2));
}

#[test]
fn normalize_multibyte_emoji() {
    let text = "ab\u{1F600}cd";
    // 'a'=0, 'b'=1, emoji=2..6, 'c'=6, 'd'=7
    let result = mr(0, 2, MotionInclusivity::Inclusive).normalize(text);
    assert_eq!(result.range(), Range::from_raw(0, 6));
}
