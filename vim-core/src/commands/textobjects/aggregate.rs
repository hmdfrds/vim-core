//! Aggregate text objects (ib/ab, iq/aq).
//!
//! `AnyBracket` tries all four bracket types and picks the tightest enclosing
//! pair (or the nearest pair if the cursor is not inside any).
//!
//! `AnyQuote` tries all three quote characters and applies the same heuristic.
//!
//! # Algorithm
//!
//! 1. **Pass 1 — Tightest enclosing:** Among candidates whose range contains
//!    the cursor, pick the one with the smallest span.
//! 2. **Pass 2 — Nearest fallback:** If no candidate contains the cursor,
//!    pick the one closest to it. Ties are broken by earlier start position.
//!
//! # Architecture
//!
//! ALLOWED imports: `primitives`, sibling modules (`brackets`, `quotes`, `types`)
//! FORBIDDEN imports: `grammar`, `mode/`, `execution/`, `dispatch/`

use super::brackets;
use super::quotes;
use super::types::{BracketType, TextObjectContext, TextObjectRange};
use crate::grammar::types::TextObjectScope;

/// Compute the `AnyBracket` text object.
///
/// Tries all four bracket types (`()`, `{}`, `[]`, `<>`) and picks the
/// tightest enclosing pair around the cursor. If the cursor is not inside
/// any bracket pair, falls back to the nearest pair.
#[must_use]
pub fn compute_any_bracket_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let bracket_types = [
        BracketType::Paren,
        BracketType::Brace,
        BracketType::Bracket,
        BracketType::Angle,
    ];

    let candidates: Vec<TextObjectRange> = bracket_types
        .iter()
        .filter_map(|&bt| brackets::compute_bracket_object(ctx, scope, bt))
        .collect();

    pick_tightest_or_nearest(&candidates, ctx.cursor.get())
}

/// Compute the `AnyQuote` text object.
///
/// Tries all three quote characters (`"`, `'`, `` ` ``) and picks the
/// tightest enclosing pair around the cursor. If the cursor is not inside
/// any quote pair, falls back to the nearest pair.
#[must_use]
pub fn compute_any_quote_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let quote_chars = ['"', '\'', '`'];

    let candidates: Vec<TextObjectRange> = quote_chars
        .iter()
        .filter_map(|&q| quotes::compute_quote_object(ctx, scope, q))
        .collect();

    pick_tightest_or_nearest(&candidates, ctx.cursor.get())
}

/// Pick the tightest enclosing candidate, or fall back to the nearest one.
///
/// **Pass 1 — Tightest enclosing:**
/// Filter candidates where the range contains the cursor
/// (`start <= cursor && cursor < end`, using `cursor <= end` for empty ranges
/// where `start == end`). Among those, pick the one with the smallest span.
///
/// **Pass 2 — Nearest fallback (only if pass 1 found nothing):**
/// For each candidate, compute the distance from the cursor to the range.
/// Pick the minimum distance. On tie, prefer the candidate starting earlier.
fn pick_tightest_or_nearest(
    candidates: &[TextObjectRange],
    cursor: usize,
) -> Option<TextObjectRange> {
    if candidates.is_empty() {
        return None;
    }

    // Pass 1: tightest enclosing
    let enclosing = candidates.iter().filter(|c| {
        let start = c.start();
        let end = c.end();
        if start == end {
            // Empty range: cursor must be at exactly that position
            cursor <= end && cursor >= start
        } else {
            start <= cursor && cursor < end
        }
    });

    let tightest = enclosing.min_by_key(|c| c.end() - c.start());
    if let Some(&best) = tightest {
        return Some(best);
    }

    // Pass 2: nearest fallback
    let nearest = candidates.iter().min_by(|a, b| {
        let dist_a = distance_to_range(cursor, a.start(), a.end());
        let dist_b = distance_to_range(cursor, b.start(), b.end());
        dist_a.cmp(&dist_b).then_with(|| a.start().cmp(&b.start()))
    });

    nearest.copied()
}

/// Compute the distance from a cursor position to a range.
///
/// Returns 0 if the cursor is inside the range (shouldn't happen when called
/// from pass 2, since pass 1 would have caught it).
const fn distance_to_range(cursor: usize, start: usize, end: usize) -> usize {
    if cursor < start {
        start - cursor
    } else {
        cursor.saturating_sub(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════
    // pick_tightest_or_nearest — unit tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn pick_empty_candidates_returns_none() {
        assert!(pick_tightest_or_nearest(&[], 5).is_none());
    }

    #[test]
    fn pick_single_candidate_containing_cursor() {
        let c = TextObjectRange::char(2, 8);
        let result = pick_tightest_or_nearest(&[c], 5);
        assert_eq!(result, Some(c));
    }

    #[test]
    fn pick_two_enclosing_different_sizes_picks_smaller() {
        let big = TextObjectRange::char(0, 20);
        let small = TextObjectRange::char(3, 10);
        let result = pick_tightest_or_nearest(&[big, small], 5);
        assert_eq!(result, Some(small));
    }

    #[test]
    fn pick_two_enclosing_different_sizes_picks_smaller_reverse_order() {
        // Order in the slice shouldn't matter
        let big = TextObjectRange::char(0, 20);
        let small = TextObjectRange::char(3, 10);
        let result = pick_tightest_or_nearest(&[small, big], 5);
        assert_eq!(result, Some(small));
    }

    #[test]
    fn pick_no_enclosing_picks_nearest() {
        let far = TextObjectRange::char(20, 25);
        let near = TextObjectRange::char(10, 15);
        let result = pick_tightest_or_nearest(&[far, near], 5);
        assert_eq!(result, Some(near));
    }

    #[test]
    fn pick_no_enclosing_cursor_after_ranges() {
        let a = TextObjectRange::char(2, 5);
        let b = TextObjectRange::char(7, 10);
        // cursor at 12, both ranges are behind
        // distance to a: 12 - 5 = 7
        // distance to b: 12 - 10 = 2
        let result = pick_tightest_or_nearest(&[a, b], 12);
        assert_eq!(result, Some(b));
    }

    #[test]
    fn pick_tie_distance_prefers_earlier_start() {
        // Two ranges equidistant from cursor
        let a = TextObjectRange::char(2, 4); // distance from cursor 5: 5 - 4 = 1
        let b = TextObjectRange::char(6, 8); // distance from cursor 5: 6 - 5 = 1
        let result = pick_tightest_or_nearest(&[b, a], 5);
        assert_eq!(result, Some(a));
    }

    #[test]
    fn pick_empty_range_at_cursor() {
        let empty = TextObjectRange::char(5, 5);
        let result = pick_tightest_or_nearest(&[empty], 5);
        assert_eq!(result, Some(empty));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AnyBracket — tightest enclosing
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn any_bracket_tightest_paren_inside_brace() {
        // "{( hello )}" — cursor at "hello" picks () (smaller than {})
        let text = "{( hello )}";
        let cursor = 3; // on 'h' of "hello"
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_bracket_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a bracket object");
        // Inner of () is " hello " (between parens)
        assert_eq!(r.start(), 2);
        assert_eq!(r.end(), 9);
    }

    #[test]
    fn any_bracket_only_braces() {
        // "{ hello }" — no parens, picks {}
        let text = "{ hello }";
        let cursor = 3; // on 'e'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_bracket_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a bracket object");
        assert_eq!(r.start(), 1);
        assert_eq!(r.end(), 8);
    }

    #[test]
    fn any_bracket_angle_inside_square() {
        // "[<hello>]" — cursor at "hello" picks <> (smaller than [])
        let text = "[<hello>]";
        let cursor = 3; // on 'l'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_bracket_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a bracket object");
        // Inner of <> is "hello"
        assert_eq!(r.start(), 2);
        assert_eq!(r.end(), 7);
    }

    #[test]
    fn any_bracket_all_four_types_picks_smallest() {
        // "([{<x>}])" — cursor at 'x', tightest is <>
        let text = "([{<x>}])";
        let cursor = 4; // on 'x'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_bracket_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a bracket object");
        // Inner of <> is "x"
        assert_eq!(r.start(), 4);
        assert_eq!(r.end(), 5);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AnyBracket — fallback nearest
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn any_bracket_fallback_cursor_before_brackets() {
        // "before () after" — cursor at "before" picks ()
        let text = "before () after";
        let cursor = 2; // on 'f' in "before"
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_bracket_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a bracket object");
        // The () is at positions 7..9, inner is 8..8 (empty)
        assert_eq!(r.start(), 8);
        assert_eq!(r.end(), 8);
    }

    #[test]
    fn any_bracket_no_brackets_returns_none() {
        let text = "no brackets here";
        let cursor = 5;
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_bracket_object(&ctx, TextObjectScope::Inner);
        assert!(result.is_none());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AnyBracket — scope (inner vs around)
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn any_bracket_inner_excludes_brackets() {
        // "{(hello)}" — cursor at "hello", inner of () = "hello"
        let text = "{(hello)}";
        let cursor = 3; // on 'l'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_bracket_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find inner bracket object");
        // Inner of () is "hello" = positions 2..7
        assert_eq!(r.start(), 2);
        assert_eq!(r.end(), 7);
        assert_eq!(&text[r.start()..r.end()], "hello");
    }

    #[test]
    fn any_bracket_around_includes_brackets() {
        // "{(hello)}" — cursor at "hello", around () = "(hello)"
        let text = "{(hello)}";
        let cursor = 3; // on 'l'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_bracket_object(&ctx, TextObjectScope::Around);
        let r = result.expect("should find around bracket object");
        // Around of () is "(hello)" = positions 1..8
        assert_eq!(r.start(), 1);
        assert_eq!(r.end(), 8);
        assert_eq!(&text[r.start()..r.end()], "(hello)");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AnyQuote — tightest enclosing
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn any_quote_tightest_double_inside_single() {
        // 'he said "hi"' — cursor inside "hi" picks "" (smaller than '')
        let text = r#"'he said "hi"'"#;
        let cursor = 11; // on 'i' in "hi"
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a quote object");
        // Inner of "" around "hi" is "hi"
        assert_eq!(&text[r.start()..r.end()], "hi");
    }

    #[test]
    fn any_quote_only_single_quotes() {
        // "'hello'" — only single quotes, picks ''
        let text = "'hello'";
        let cursor = 3; // on 'l'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a quote object");
        assert_eq!(&text[r.start()..r.end()], "hello");
    }

    #[test]
    fn any_quote_only_backticks() {
        let text = "`code`";
        let cursor = 3; // on 'd'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a quote object");
        assert_eq!(&text[r.start()..r.end()], "code");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AnyQuote — fallback nearest
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn any_quote_fallback_cursor_before_quotes() {
        // "before 'hi' after" — cursor at "before" picks 'hi'
        let text = "before 'hi' after";
        let cursor = 2; // on 'f' in "before"
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a quote object");
        assert_eq!(&text[r.start()..r.end()], "hi");
    }

    #[test]
    fn any_quote_no_quotes_returns_none() {
        let text = "no quotes here";
        let cursor = 5;
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        assert!(result.is_none());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AnyQuote — scope (inner vs around)
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn any_quote_inner_excludes_quotes() {
        let text = r#""hello""#;
        let cursor = 3; // on 'l'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find inner quote object");
        assert_eq!(&text[r.start()..r.end()], "hello");
    }

    #[test]
    fn any_quote_around_includes_quotes() {
        let text = r#""hello""#;
        let cursor = 3; // on 'l'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Around);
        let r = result.expect("should find around quote object");
        assert_eq!(&text[r.start()..r.end()], r#""hello""#);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AnyQuote — edge cases
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn any_quote_escaped_quotes_handled() {
        // Escaped quotes should be delegated to the underlying quote logic
        let text = r#""hello\"world""#;
        let cursor = 3; // on 'l'
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a quote object");
        // The underlying quote logic treats \" as escaped, so the pair is the outer quotes
        assert_eq!(r.start(), 1);
        assert_eq!(r.end(), 13);
    }

    #[test]
    fn any_quote_same_line_constraint() {
        // Quotes don't cross lines — cursor on a line without quotes returns None
        let text = "\"start\nhello\nend\"";
        let cursor = 8; // on 'hello' line
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        assert!(result.is_none());
    }

    #[test]
    fn any_quote_multiple_types_on_same_line() {
        // "'hello' and \"world\"" — cursor on 'world' picks ""
        let text = "'hello' and \"world\"";
        let cursor = 15; // on 'r' in "world"
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_any_quote_object(&ctx, TextObjectScope::Inner);
        let r = result.expect("should find a quote object");
        assert_eq!(&text[r.start()..r.end()], "world");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // distance_to_range — unit tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn distance_cursor_before_range() {
        assert_eq!(distance_to_range(2, 5, 10), 3);
    }

    #[test]
    fn distance_cursor_after_range() {
        assert_eq!(distance_to_range(15, 5, 10), 5);
    }

    #[test]
    fn distance_cursor_inside_range() {
        assert_eq!(distance_to_range(7, 5, 10), 0);
    }

    #[test]
    fn distance_cursor_at_start() {
        assert_eq!(distance_to_range(5, 5, 10), 0);
    }

    #[test]
    fn distance_cursor_at_end() {
        // cursor >= end, so distance = cursor - end = 0
        assert_eq!(distance_to_range(10, 5, 10), 0);
    }
}
