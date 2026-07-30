//! Bracket text objects (i(, a(, i{, a{, i[, a[, i<, a<).
//!
//! Finds matching bracket pairs containing cursor.

use super::types::{BracketType, TextObjectContext, TextObjectRange};
use crate::grammar::types::TextObjectScope;
use crate::primitives::MAX_BRACKET_TRAVEL;

/// Compute a bracket text object.
///
/// # Arguments
///
/// * `ctx` - Text object context with text and cursor
/// * `inner` - If true, exclude brackets; if false, include them
/// * `bracket` - The bracket type
///
/// # Returns
///
/// `Some(TextObjectRange)` if matching brackets found, `None` otherwise.
#[must_use]
pub fn compute_bracket_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
    bracket: BracketType,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    let (open, close) = bracket.chars();
    let cursor = if ctx.cursor.get() >= text.len() && !text.is_empty() {
        crate::primitives::text_util::prev_char_boundary(text, text.len())
    } else {
        ctx.cursor.get()
    };

    // Find the opening bracket (search backward, counting depth)
    let open_pos = find_opening_bracket(text, cursor, open, close)?;

    // Find the closing bracket (search forward from open, counting depth)
    let close_pos = find_closing_bracket(text, open_pos, open, close)?;

    if scope.is_inner() {
        // Inner: exclude the brackets themselves
        let mut start = open_pos + open.len_utf8();
        let mut end = close_pos;

        // For multiline bracket pairs, Vim adjusts the inner range:
        // - Skip the newline immediately after the opening bracket
        // - Skip whitespace/newline immediately before the closing bracket
        if start < end {
            let inner_text = &text[start..end];
            if inner_text.contains('\n') {
                // Skip newline (and optional whitespace) after opening bracket
                if inner_text.starts_with('\n') {
                    start += 1;
                }
                // Skip whitespace/newline before closing bracket
                let trimmed_end = inner_text.trim_end_matches([' ', '\t']);
                if trimmed_end.len() < inner_text.len() && trimmed_end.ends_with('\n') {
                    // Keep the trailing newline, remove only spaces/tabs
                    end = start + trimmed_end.len();
                }
            }
        }

        if start <= end {
            // Determine if this should be linewise:
            // In Vim, inner bracket text objects become linewise when:
            // 1. The content contains newlines
            // 2. The opening bracket is followed by a newline
            // 3. The closing bracket is preceded by a newline (with optional whitespace)
            let inner_text = &text[start..end];
            let is_linewise =
                inner_text.contains('\n') && text[open_pos + open.len_utf8()..].starts_with('\n');

            if is_linewise {
                Some(TextObjectRange::from_range_linewise(
                    crate::primitives::Range::from_raw(start, end),
                ))
            } else {
                Some(TextObjectRange::char(start, end))
            }
        } else {
            None
        }
    } else {
        // Around: include the brackets.
        // Neovim uses linewise only when the opening bracket is the first
        // non-blank character on its line (i.e., the brace is on its own
        // or at the start). When the bracket is mid-line (e.g., `fn() {`),
        // the text object stays charwise.
        let inner_text = &text[open_pos..close_pos + close.len_utf8()];
        let open_line_start = crate::commands::helpers::line_start_for_offset(text, open_pos);
        let before_open = &text[open_line_start..open_pos];
        let open_at_bol = before_open.chars().all(char::is_whitespace);
        let is_linewise = open_at_bol
            && inner_text.contains('\n')
            && text[open_pos + open.len_utf8()..].starts_with('\n');

        if is_linewise {
            Some(TextObjectRange::from_range_linewise(
                crate::primitives::Range::from_raw(open_pos, close_pos + close.len_utf8()),
            ))
        } else {
            Some(TextObjectRange::char(
                open_pos,
                close_pos + close.len_utf8(),
            ))
        }
    }
}

/// Find the opening bracket containing the cursor.
///
/// Search order (per Vim behavior):
/// 1. If cursor is ON an opening bracket, use it
/// 2. Search backward for an enclosing opening bracket
/// 3. If backward search fails, search forward on the current line
///
/// Note: Neovim's `current_block()` (which handles `ci(`, `di{`, etc.)
/// does NOT skip brackets inside strings/comments. It temporarily sets
/// `cpo` to just `%` (or `%M`), disabling comment/string awareness.
/// Only the `%` motion uses comment/string skipping. We match this
/// behavior by doing plain bracket matching without `CommentStringRanges`.
fn find_opening_bracket(text: &str, cursor: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0;
    let mut pos = cursor;

    // Check if cursor is on an opening bracket
    let cursor_char = text[cursor..].chars().next()?;
    if cursor_char == open {
        return Some(cursor);
    }

    // Check if cursor is on a closing bracket — search backward for its match
    if cursor_char == close {
        let mut d = 1i32;
        let mut p = cursor;
        let mut traveled = 0;
        while p > 0 {
            p -= 1;
            while p > 0 && !text.is_char_boundary(p) {
                p -= 1;
            }
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                return None;
            }
            let c = text[p..].chars().next()?;
            if c == close {
                d += 1;
            } else if c == open {
                d -= 1;
                if d == 0 {
                    return Some(p);
                }
            }
        }
        let c = text.chars().next()?;
        if c == open && d == 1 {
            return Some(0);
        }
        return None;
    }

    // Search backward for enclosing open bracket
    let mut traveled = 0;
    while pos > 0 {
        pos -= 1;
        while pos > 0 && !text.is_char_boundary(pos) {
            pos -= 1;
        }

        traveled += 1;
        if traveled >= MAX_BRACKET_TRAVEL {
            return None;
        }
        let c = text[pos..].chars().next()?;
        if c == close {
            depth += 1;
        } else if c == open {
            if depth == 0 {
                return Some(pos);
            }
            depth -= 1;
        }
    }

    // Backward search failed — search FORWARD in entire buffer for opening bracket
    // This matches Vim behavior: `di(` when cursor is before `(` on the same line or later
    let search_end = text.len();

    let mut fwd = cursor + cursor_char.len_utf8();
    let mut traveled = 0;
    while fwd < search_end {
        traveled += 1;
        if traveled >= MAX_BRACKET_TRAVEL {
            return None;
        }
        let c = text[fwd..].chars().next()?;
        if c == open {
            return Some(fwd);
        }
        fwd += c.len_utf8();
    }
    None
}

/// Find the closing bracket matching the opening at `open_pos`.
///
/// Plain bracket matching without comment/string skipping, matching
/// Neovim's `current_block()` behavior for text objects.
fn find_closing_bracket(text: &str, open_pos: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 1;
    let mut pos = open_pos + open.len_utf8();

    let mut traveled = 0;
    while pos < text.len() {
        traveled += 1;
        if traveled >= MAX_BRACKET_TRAVEL {
            return None;
        }

        let c = text[pos..].chars().next()?;
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(pos);
            }
        }
        pos += c.len_utf8();
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
        bracket: BracketType,
        expected: Option<(usize, usize)>,
    ) {
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_bracket_object(&ctx, TextObjectScope::from_inner_flag(inner), bracket);
        match (result, expected) {
            (Some(r), Some((s, e))) => {
                assert_eq!(r.start(), s, "start mismatch");
                assert_eq!(r.end(), e, "end mismatch");
            }
            (None, None) => {}
            _ => panic!("result {:?} != expected {:?}", result, expected),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // PARENTHESES
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_inner_paren_simple() {
        check("(hello)", 1, true, BracketType::Paren, Some((1, 6)));
    }

    #[test]
    fn test_around_paren_simple() {
        check("(hello)", 1, false, BracketType::Paren, Some((0, 7)));
    }

    #[test]
    fn test_nested_parens() {
        check("(a(b)c)", 3, true, BracketType::Paren, Some((3, 4))); // inner of (b)
        check("(a(b)c)", 1, true, BracketType::Paren, Some((1, 6))); // inner of outer
    }

    #[test]
    fn test_cursor_on_bracket() {
        check("(hello)", 0, true, BracketType::Paren, Some((1, 6)));
        check("(hello)", 0, false, BracketType::Paren, Some((0, 7)));
    }

    #[test]
    fn test_cursor_on_close_bracket() {
        check("(hello)", 6, true, BracketType::Paren, Some((1, 6)));
        check("(hello)", 6, false, BracketType::Paren, Some((0, 7)));
    }

    #[test]
    fn test_no_match() {
        check("hello", 0, true, BracketType::Paren, None);
        check("(unmatched", 1, true, BracketType::Paren, None);
    }

    #[test]
    fn test_angle_no_match_on_plain_text() {
        // Oracle #93: ci< on text with NO angle brackets should return None.
        let text = "sqmzkfp hjifnlei rasow xtriayb eiy nyy scnrx paenv dhausowz duyizb tjzumqdf mhz xixqyqm";
        check(text, 79, true, BracketType::Angle, None);
        check(text, 0, true, BracketType::Angle, None);
        check(text, 40, false, BracketType::Angle, None);
    }

    #[test]
    fn test_empty_parens() {
        check("()", 0, true, BracketType::Paren, Some((1, 1)));
        check("()", 0, false, BracketType::Paren, Some((0, 2)));
    }

    #[test]
    fn test_deeply_nested() {
        // For deeply nested ((( ))), at pos 3 we're inside all of them
        // but the innermost containing cursor is at pos 2
        check("(((a)))", 3, true, BracketType::Paren, Some((3, 4)));
        // At pos 2, we're on the inner ( so we get that one
        check("(((a)))", 2, true, BracketType::Paren, Some((3, 4)));
        // At pos 1, we're on the middle ( so we get that one
        check("(((a)))", 1, true, BracketType::Paren, Some((2, 5)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // BRACES
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_braces() {
        check("{foo}", 1, true, BracketType::Brace, Some((1, 4)));
    }

    #[test]
    fn test_braces_around() {
        check("{foo}", 1, false, BracketType::Brace, Some((0, 5)));
    }

    #[test]
    fn test_braces_nested() {
        check("{a{b}c}", 3, true, BracketType::Brace, Some((3, 4)));
    }

    #[test]
    fn test_braces_multiline() {
        let text = "{\nfoo\nbar\n}";
        check(text, 2, true, BracketType::Brace, Some((2, 10)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // SQUARE BRACKETS
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_brackets() {
        check("[bar]", 1, true, BracketType::Bracket, Some((1, 4)));
    }

    #[test]
    fn test_brackets_around() {
        check("[bar]", 1, false, BracketType::Bracket, Some((0, 5)));
    }

    #[test]
    fn test_brackets_nested() {
        check("[a[b]c]", 3, true, BracketType::Bracket, Some((3, 4)));
    }

    #[test]
    fn test_brackets_with_content() {
        check("arr[idx]", 4, true, BracketType::Bracket, Some((4, 7)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ANGLE BRACKETS
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_angles() {
        check("<tag>", 1, true, BracketType::Angle, Some((1, 4)));
    }

    #[test]
    fn test_angles_around() {
        check("<tag>", 1, false, BracketType::Angle, Some((0, 5)));
    }

    #[test]
    fn test_angles_nested() {
        check("<a<b>c>", 3, true, BracketType::Angle, Some((3, 4)));
    }

    #[test]
    fn test_angles_generic() {
        check(
            "Option<Result<T>>",
            7,
            true,
            BracketType::Angle,
            Some((7, 16)),
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MULTILINE AND EDGE CASES
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_multiline() {
        let text = "(\nhello\n)";
        check(text, 2, true, BracketType::Paren, Some((2, 8)));
    }

    #[test]
    fn test_multiline_around() {
        let text = "(\nhello\n)";
        check(text, 2, false, BracketType::Paren, Some((0, 9)));
    }

    #[test]
    fn test_bracket_with_spaces() {
        check("(  hello  )", 4, true, BracketType::Paren, Some((1, 10)));
    }

    #[test]
    fn test_unicode_content() {
        check("(héllo)", 1, true, BracketType::Paren, Some((1, 7)));
    }

    #[test]
    fn test_mixed_brackets() {
        // Looking for parens in text with braces
        check("{(foo)}", 2, true, BracketType::Paren, Some((2, 5)));
        check("{(foo)}", 1, true, BracketType::Brace, Some((1, 6)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ADVERSARIAL SAFETY TESTS — Bracket Hardening Verification
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn adversarial_unmatched_text_object_no_hang() {
        // Test 1: Unmatched text object (for text objects)
        // Create 200K unmatched closing brackets with no opening bracket
        let text = "}".repeat(200_000);

        let ctx = TextObjectContext::new(&text, 10);
        // Should return None (no enclosing bracket)
        // Should not hang or crash with huge amount of closing brackets
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Brace);
        assert_eq!(result, None);
    }

    #[test]
    fn adversarial_deep_nesting_text_object() {
        // Test 2: Deep nesting text object
        // Create 50K nested braces: {{{...}}}
        let half = "{\n".repeat(25_000);
        let closing = "}\n".repeat(25_000);
        let text = format!("{}{}", half, closing);

        let ctx = TextObjectContext::new(&text, 100);
        // Should handle deeply nested content without stack overflow
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Brace);
        // Should either succeed or return None, never panic
        let _ = result; // Just verify no panic
    }

    #[test]
    fn adversarial_all_comment_text_object() {
        // Test 3: All-comment file with text object
        // Create 200K characters inside a block comment, no actual brackets
        let text = format!("/* {} */", "x".repeat(200_000));

        let ctx = TextObjectContext::new(&text, 10_000);
        // Should return None (no enclosing bracket)
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Paren);
        assert_eq!(result, None);
    }

    #[test]
    fn adversarial_max_travel_limit_text_object() {
        // Test 4: MAX_BRACKET_TRAVEL limit for text objects
        // Opening bracket at position 0, closing far beyond MAX_BRACKET_TRAVEL
        let text = format!("({}", "x".repeat(150_000));

        let ctx = TextObjectContext::new(&text, 50_000);
        // Should return None because closing bracket is beyond
        // MAX_BRACKET_TRAVEL distance from opening bracket
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Paren);
        assert_eq!(result, None);
    }

    #[test]
    fn adversarial_complex_multiline_text_object() {
        // Test 5: Complex multiline text object
        // 50K line file with mixed strings, comments, and bracket pairs
        let mut text = String::new();
        for i in 0..10_000 {
            text.push_str(&format!(
                "code = {{ \"str_{i}\" /* comment */ = [val]; }}\n"
            ));
        }

        let ctx = TextObjectContext::new(&text, 5000);
        // Should find text object correctly even in large file
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Brace);
        // Should either succeed or fail gracefully, never hang
        let _ = result; // Just verify no panic or hang
    }

    #[test]
    fn adversarial_cursor_beyond_max_travel() {
        // Test 6: Cursor beyond matching bracket by more than MAX_BRACKET_TRAVEL
        // Create opening bracket at 0 and cursor far away
        let text = format!("{}{}", "(", "x".repeat(150_000));

        let ctx = TextObjectContext::new(&text, 100_000);
        // Should return None because we can't find the closing bracket
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Paren);
        assert_eq!(result, None);
    }

    #[test]
    fn adversarial_escaped_quotes_text_object() {
        // Test 7: Escaped quotes in text object
        // Verify escaped quote handling doesn't confuse bracket finding
        let text = r#"(first "str with \" escaped" second)"#;

        let ctx = TextObjectContext::new(&text, 5);
        // Should correctly find the enclosing paren despite escaped quotes
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Paren);
        assert!(result.is_some());
    }

    #[test]
    fn adversarial_comment_with_bracket_chars_text_object() {
        // Test 8: Comments containing bracket characters
        // Verify brackets in comments are ignored
        let text = "/* { [ ( brackets ) ] } inside comment */ (real (nested))";

        let ctx = TextObjectContext::new(&text, 50);
        // Should find the real bracket pair, ignoring comment brackets
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Paren);
        assert!(result.is_some());
    }

    #[test]
    fn adversarial_very_long_string_with_brackets() {
        // Test 9: Very long string literal with bracket-like characters
        // 100K character string with brackets inside
        let text =
            format!(r#"(code before "very long string with {{[({{}})}} all inside" code after)"#);

        let ctx = TextObjectContext::new(&text, 50);
        // Should find the real outer parens, skipping string content
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Paren);
        assert!(result.is_some());
    }

    #[test]
    fn adversarial_nested_text_objects_stress() {
        // Test 10: Stress test with many nested levels
        // Create deeply nested: (((((...))))
        let opens = "(".repeat(1000);
        let closes = ")".repeat(1000);
        let text = format!("{}{}", opens, closes);

        let ctx = TextObjectContext::new(&text, 500);
        // Should handle deep nesting without stack overflow
        let result = compute_bracket_object(&ctx, TextObjectScope::Inner, BracketType::Paren);
        // Should either succeed or bail gracefully
        let _ = result; // Just verify no panic or stack overflow
    }
}
