//! Quote text objects (i", a", i', a', i`, a`).
//!
//! Per Neovim: Quote search is SAME LINE ONLY.

use super::helpers::{line_end, line_start};
use super::types::{TextObjectContext, TextObjectRange};
use crate::grammar::types::TextObjectScope;

/// Compute a quote text object.
///
/// # Arguments
///
/// * `ctx` - Text object context with text and cursor
/// * `scope` - Inner excludes quotes; Around includes them
/// * `quote` - The quote character (", ', `)
///
/// # Returns
///
/// `Some(TextObjectRange)` if matching quotes found on same line, `None` otherwise.
#[must_use]
pub fn compute_quote_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
    quote: char,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    if text.is_empty() {
        return None;
    }

    let cursor = if ctx.cursor.get() >= text.len() && !text.is_empty() {
        crate::primitives::text_util::prev_char_boundary(text, text.len())
    } else {
        ctx.cursor.get()
    };

    // Get the current line bounds
    let line_s = line_start(text, cursor);
    let line_e = line_end(text, cursor);
    let line = text.get(line_s..line_e)?;
    let cursor_in_line = cursor - line_s;

    // Neovim's current_quote() algorithm (simplified for non-visual):
    //
    // 1. If cursor is ON a quote character, scan from line start to
    //    determine pairing, then find the pair containing the cursor.
    // 2. Otherwise, search BACKWARD for the nearest quote (col_start),
    //    then search FORWARD from col_start+1 for the next quote (col_end).
    //    If backward search fails, search forward from cursor instead.
    let cursor_char = line.as_bytes().get(cursor_in_line).copied();
    let escape_chars = ctx.quoteescape;

    let (col_start, col_end) = if cursor_char == Some(quote as u8) {
        // Cursor is on a quote — scan from line start to determine pairing.
        let quote_positions = find_quote_positions(line, quote, escape_chars);
        let mut found = None;
        for pair in quote_positions.chunks_exact(2) {
            let (&open, &close) = (pair.first()?, pair.get(1)?);
            if open <= cursor_in_line && cursor_in_line <= close {
                found = Some((open, close));
                break;
            }
        }
        found?
    } else {
        // Cursor is NOT on a quote — Neovim's else branch:
        // Search backward for nearest quote, then forward for the pair.
        let backward = find_prev_quote(line, cursor_in_line, quote, escape_chars);
        if let Some(col_s) = backward {
            // Found a quote before cursor — search forward for its pair
            let col_e = find_next_quote(line, col_s + quote.len_utf8(), quote, escape_chars)?;
            (col_s, col_e)
        } else {
            // No quote before cursor — search forward for the next pair
            let col_s = find_next_quote(line, cursor_in_line, quote, escape_chars)?;
            let col_e = find_next_quote(line, col_s + quote.len_utf8(), quote, escape_chars)?;
            (col_s, col_e)
        }
    };

    let abs_open = line_s + col_start;
    let abs_close = line_s + col_end;

    if scope.is_inner() {
        let start = abs_open + quote.len_utf8();
        let end = abs_close;
        if start <= end {
            Some(TextObjectRange::char(start, end))
        } else {
            None
        }
    } else {
        let mut start = abs_open;
        let mut end = abs_close + quote.len_utf8();
        // For around-quote, include trailing whitespace (preferred)
        // or leading whitespace (fallback), like Vim's a" behavior
        let after = text.get(end..line_e).unwrap_or_default();
        let trailing_ws = after.len() - after.trim_start_matches([' ', '\t']).len();
        if trailing_ws > 0 {
            end += trailing_ws;
        } else {
            let before = text.get(line_s..start).unwrap_or_default();
            let leading_ws = before.len() - before.trim_end_matches([' ', '\t']).len();
            start -= leading_ws;
        }
        Some(TextObjectRange::char(start, end))
    }
}

/// Find all unescaped quote positions on a line.
///
/// Tracks escape state properly: `\\` is an escaped escape-char (resets escape),
/// so `\\"` is escaped-backslash + unescaped-quote (detected), while `\"` is
/// a single escaped quote (skipped).
///
/// `escape_chars` is the set of characters that escape the quote (Vim's
/// `quoteescape` option). The default is `"\\"` (single backslash).
fn find_quote_positions(line: &str, quote: char, escape_chars: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut escaped = false;

    for (pos, c) in line.char_indices() {
        if escaped {
            // Previous char was an unescaped escape char — this char is escaped.
            escaped = false;
        } else if escape_chars.contains(c) {
            escaped = true;
        } else if c == quote {
            positions.push(pos);
        }
    }

    positions
}

/// Search backward from `start` for the nearest unescaped quote.
///
/// Returns the byte position of the quote within `line`, or `None`.
fn find_prev_quote(line: &str, start: usize, quote: char, escape_chars: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let qb = quote as u8;
    // Only handles ASCII quotes (", ', `)
    if start == 0 {
        return None;
    }
    let mut pos = start - 1;
    loop {
        if bytes.get(pos).copied() == Some(qb) {
            // Check if this quote is escaped by counting consecutive escape chars
            let mut escape_count = 0;
            let mut bp = pos;
            while bp > 0 {
                bp -= 1;
                if let Some(&b) = bytes.get(bp) {
                    if escape_chars.contains(b as char) {
                        escape_count += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            if escape_count % 2 == 0 {
                return Some(pos);
            }
        }
        if pos == 0 {
            break;
        }
        pos -= 1;
    }
    None
}

/// Search forward from `start` for the nearest unescaped quote.
///
/// Returns the byte position of the quote within `line`, or `None`.
fn find_next_quote(line: &str, start: usize, quote: char, escape_chars: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let qb = quote as u8;
    let mut pos = start;
    while pos < bytes.len() {
        if bytes.get(pos).copied() == Some(qb) {
            // Check if escaped by counting consecutive escape chars before this position
            let mut escape_count = 0;
            let mut bp = pos;
            while bp > 0 {
                bp -= 1;
                if let Some(&b) = bytes.get(bp) {
                    if escape_chars.contains(b as char) {
                        escape_count += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            if escape_count % 2 == 0 {
                return Some(pos);
            }
        }
        pos += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(
        text: &str,
        cursor: usize,
        inner: bool,
        quote: char,
        expected: Option<(usize, usize)>,
    ) {
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_quote_object(&ctx, TextObjectScope::from_inner_flag(inner), quote);
        match (result, expected) {
            (Some(r), Some((s, e))) => {
                assert_eq!(r.start(), s, "start mismatch");
                assert_eq!(r.end(), e, "end mismatch");
            }
            (None, None) => {}
            _ => panic!("result {:?} != expected {:?}", result, expected),
        }
    }

    #[test]
    fn test_inner_double_quote() {
        check("\"hello\"", 1, true, '"', Some((1, 6)));
    }

    #[test]
    fn test_around_double_quote() {
        check("\"hello\"", 1, false, '"', Some((0, 7)));
    }

    #[test]
    fn test_single_quote() {
        check("'world'", 2, true, '\'', Some((1, 6)));
    }

    #[test]
    fn test_backtick() {
        check("`code`", 2, true, '`', Some((1, 5)));
    }

    #[test]
    fn test_cursor_on_quote() {
        check("\"hi\"", 0, true, '"', Some((1, 3)));
        check("\"hi\"", 0, false, '"', Some((0, 4)));
    }

    #[test]
    fn test_cursor_before_quotes() {
        check("a \"hi\" b", 0, true, '"', Some((3, 5)));
    }

    #[test]
    fn test_no_quotes() {
        check("hello", 0, true, '"', None);
    }

    #[test]
    fn test_escaped_quote() {
        // The \" should not count as a quote
        check("\"hello\\\"world\"", 1, true, '"', Some((1, 13)));
    }

    #[test]
    fn test_multiple_pairs() {
        check("\"a\" \"b\"", 5, true, '"', Some((5, 6))); // inside "b"
    }

    #[test]
    fn test_escaped_backslash_before_quote() {
        // \\\\" in source = `\\"` in string = escaped backslash then unescaped quote.
        // The quote SHOULD be detected — the backslash escapes itself, not the quote.
        check("\"hello\\\\\"", 1, true, '"', Some((1, 8)));
    }

    #[test]
    fn test_does_not_cross_lines() {
        let text = "\"start\nhello\nend\"";
        // Cursor on 'hello' line - should not find quotes
        check(text, 8, true, '"', None);
    }

    // ── Pipe (|) text object tests ──────────────────────────────────────

    #[test]
    fn test_inner_pipe_basic() {
        check("|hello|", 1, true, '|', Some((1, 6)));
    }

    #[test]
    fn test_around_pipe_basic() {
        check("|hello|", 1, false, '|', Some((0, 7)));
    }

    #[test]
    fn test_pipe_cursor_on_opening() {
        check("|hello|", 0, true, '|', Some((1, 6)));
        check("|hello|", 0, false, '|', Some((0, 7)));
    }

    #[test]
    fn test_pipe_cursor_on_closing() {
        check("|hello|", 6, true, '|', Some((1, 6)));
        check("|hello|", 6, false, '|', Some((0, 7)));
    }

    #[test]
    fn test_pipe_at_start_of_line() {
        // Pipe pair starting at column 0
        check("|x|rest", 1, true, '|', Some((1, 2)));
    }

    #[test]
    fn test_pipe_at_end_of_line() {
        // Pipe pair ending at last column
        check("rest|x|", 5, true, '|', Some((5, 6)));
    }

    #[test]
    fn test_multiple_pipe_pairs_first() {
        // |a| foo |b| — cursor inside first pair
        check("|a| foo |b|", 1, true, '|', Some((1, 2)));
    }

    #[test]
    fn test_multiple_pipe_pairs_second() {
        // |a| foo |b| — cursor inside second pair
        check("|a| foo |b|", 9, true, '|', Some((9, 10)));
    }

    #[test]
    fn test_multiple_pipe_pairs_between() {
        // |a| foo |b| — cursor in gap between pairs, searches backward
        // Backward from pos 5 finds | at pos 2, forward from 3 finds | at 8 → pair (2,8)
        check("|a| foo |b|", 5, true, '|', Some((3, 8)));
    }

    #[test]
    fn test_nested_pipes() {
        // ||x|| — four pipes on line. Positions: 0,1,3,4
        // Pairs by pairing logic: (0,1) and (3,4)
        // Cursor on 'x' at pos 2: backward finds | at 1, forward from 2 finds | at 3
        check("||x||", 2, true, '|', Some((2, 3)));
    }

    #[test]
    fn test_nested_pipes_cursor_on_first_inner() {
        // ||x|| — cursor on pos 1 (second |)
        // Pairing: positions [0,1,3,4] → pairs (0,1) and (3,4)
        // Cursor at 1 is on pair (0,1) → inner is empty (1,1)
        check("||x||", 1, true, '|', Some((1, 1)));
    }

    #[test]
    fn test_pipe_empty_content() {
        // || — empty pipe pair
        check("||", 0, true, '|', Some((1, 1)));
        check("||", 0, false, '|', Some((0, 2)));
    }

    #[test]
    fn test_pipe_markdown_table_cell() {
        // Markdown table: | cell |
        // Positions of |: 0 and 7
        check("| cell |", 3, true, '|', Some((1, 7)));
    }

    #[test]
    fn test_pipe_markdown_table_multi_cell() {
        // | a | b | — three pipes at pos 0, 4, 8
        // Pairs: (0,4) and (4,8) — but each | can only belong to one pair
        // Pairing from start: (0,4) is first pair, (4,8) is second
        // Cursor at pos 2 (inside 'a'): backward finds | at 0, forward from 1 finds | at 4
        check("| a | b |", 2, true, '|', Some((1, 4)));
    }

    #[test]
    fn test_pipe_escaped() {
        // \| is an escaped pipe — should be skipped
        // "hello\|world|end|" — unescaped pipes at 12 and 16
        check("hello\\|world|end|", 13, true, '|', Some((13, 16)));
    }

    #[test]
    fn test_pipe_escaped_not_a_pair() {
        // Only one unescaped pipe — no pair found
        check("hello\\|world|end", 13, true, '|', None);
    }

    #[test]
    fn test_pipe_no_match() {
        check("hello world", 3, true, '|', None);
    }

    #[test]
    fn test_pipe_does_not_cross_lines() {
        let text = "|start\nhello\nend|";
        // Cursor on 'hello' line — no pipes on that line
        check(text, 8, true, '|', None);
    }

    #[test]
    fn test_pipe_around_includes_trailing_whitespace() {
        // "foo |bar| baz" — around should include trailing space
        check("foo |bar| baz", 5, false, '|', Some((4, 10)));
    }

    #[test]
    fn test_pipe_around_includes_leading_whitespace_if_no_trailing() {
        // "foo |bar|" — no trailing space, so leading space is included
        check("foo |bar|", 5, false, '|', Some((3, 9)));
    }

    #[test]
    fn test_pipe_rust_closure_params() {
        // Rust closure: |a, b| — cursor inside params
        check("|a, b|", 2, true, '|', Some((1, 5)));
        check("|a, b|", 2, false, '|', Some((0, 6)));
    }

    // ── quoteescape tests ──────────────────────────────────────────────

    /// Helper that creates a context with a custom quoteescape.
    fn check_qe(
        text: &str,
        cursor: usize,
        inner: bool,
        quote: char,
        quoteescape: &str,
        expected: Option<(usize, usize)>,
    ) {
        let mut ctx = TextObjectContext::new(text, cursor);
        ctx.quoteescape = quoteescape;
        let result = compute_quote_object(&ctx, TextObjectScope::from_inner_flag(inner), quote);
        match (result, expected) {
            (Some(r), Some((s, e))) => {
                assert_eq!(r.start(), s, "start mismatch");
                assert_eq!(r.end(), e, "end mismatch");
            }
            (None, None) => {}
            _ => panic!("result {:?} != expected {:?}", result, expected),
        }
    }

    #[test]
    fn quoteescape_default_backslash() {
        // Default backslash escape: \" is escaped
        check_qe("\"hello\\\"world\"", 1, true, '"', "\\", Some((1, 13)));
    }

    #[test]
    fn quoteescape_empty_no_escape() {
        // Empty quoteescape: backslash is NOT an escape char
        // "hello\"world" with no escaping -> quote positions: 0, 7, 13
        // Pair (0,7) contains cursor 1 -> inner is (1, 7)
        check_qe("\"hello\\\"world\"", 1, true, '"', "", Some((1, 7)));
    }

    #[test]
    fn quoteescape_custom_char() {
        // Use '!' as escape character instead of backslash
        // "hello!"world" — !" is escaped, so pair is (0, 13)
        check_qe("\"hello!\"world\"", 1, true, '"', "!", Some((1, 13)));
    }

    #[test]
    fn quoteescape_multiple_chars() {
        // Both backslash and ! are escape characters
        check_qe("\"hello\\\"world\"", 1, true, '"', "\\!", Some((1, 13)));
        check_qe("\"hello!\"world\"", 1, true, '"', "\\!", Some((1, 13)));
    }
}
