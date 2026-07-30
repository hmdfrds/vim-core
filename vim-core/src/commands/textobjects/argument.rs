//! Pure-text argument text object (ia/aa).
//!
//! Finds comma-delimited arguments within enclosing bracket pairs.
//! Works without any syntax provider — pure text analysis.
//!
//! # Algorithm
//!
//! 1. Find the tightest enclosing bracket pair containing the cursor.
//! 2. Scan for commas at depth 0 within that pair (tracking nested brackets).
//! 3. Select the comma-delimited segment containing the cursor.
//! 4. **Inner** scope: trimmed argument text (no surrounding whitespace).
//! 5. **Around** scope: include trailing comma+whitespace, or leading if last arg.

use super::types::{TextObjectContext, TextObjectRange};
use crate::grammar::types::TextObjectScope;

/// Compute a pure-text argument text object.
///
/// Returns `None` when:
/// - No enclosing bracket pair is found
/// - The bracket pair is empty
/// - The cursor is not inside any argument
#[must_use]
pub fn compute_argument_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    let cursor = ctx.cursor.get();

    // 1. Find the tightest enclosing bracket pair.
    let (inner_start, inner_end) = find_tightest_bracket_inner(text, cursor)?;

    // Empty bracket pair — no arguments.
    if inner_start >= inner_end {
        return None;
    }

    // 2. Find comma positions at depth 0 within the bracket inner content.
    let commas = find_depth0_commas(text, inner_start, inner_end);

    // 3. Determine which argument segment contains the cursor.
    let (arg_start, arg_end, arg_index, total_args) =
        find_argument_segment(inner_start, inner_end, &commas, cursor)?;

    // 4/5. Apply scope (inner vs around).
    match scope {
        TextObjectScope::Inner => {
            // Trim leading/trailing whitespace from the argument.
            let trimmed_start = trim_leading_whitespace(text, arg_start, arg_end);
            let trimmed_end = trim_trailing_whitespace(text, trimmed_start, arg_end);
            if trimmed_start >= trimmed_end {
                return None;
            }
            Some(TextObjectRange::char(trimmed_start, trimmed_end))
        }
        TextObjectScope::Around => {
            if total_args == 1 {
                // Single argument — same as inner (no comma to include).
                let trimmed_start = trim_leading_whitespace(text, arg_start, arg_end);
                let trimmed_end = trim_trailing_whitespace(text, trimmed_start, arg_end);
                if trimmed_start >= trimmed_end {
                    return None;
                }
                Some(TextObjectRange::char(trimmed_start, trimmed_end))
            } else if arg_index < total_args - 1 {
                // Not the last argument — include trailing comma + whitespace.
                let trimmed_start = trim_leading_whitespace(text, arg_start, arg_end);
                // The comma after this argument is at commas[arg_index].
                let comma_pos = commas[arg_index];
                // Include the comma and any whitespace after it.
                let after_comma = skip_whitespace_forward(text, comma_pos + 1, inner_end);
                Some(TextObjectRange::char(trimmed_start, after_comma))
            } else {
                // Last argument — include leading comma + whitespace.
                let trimmed_end = trim_trailing_whitespace(text, arg_start, arg_end);
                // The comma before this argument is at commas[arg_index - 1].
                let comma_pos = commas[arg_index - 1];
                Some(TextObjectRange::char(comma_pos, trimmed_end))
            }
        }
    }
}

/// Find the tightest enclosing bracket pair's inner range (excluding brackets).
///
/// Tries all four bracket types and picks the tightest one containing the cursor.
fn find_tightest_bracket_inner(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let pairs: [(&str, &str); 4] = [("(", ")"), ("[", "]"), ("{", "}"), ("<", ">")];
    let mut best: Option<(usize, usize)> = None;
    let mut best_span = usize::MAX;

    for (open, close) in &pairs {
        if let Some((_, open_end, close_start, _)) =
            crate::document::text_pairs::find_surrounding_pair(text, cursor, open, close)
        {
            let span = close_start - open_end;
            if span < best_span {
                best = Some((open_end, close_start));
                best_span = span;
            }
        }
    }

    best
}

/// Find all comma positions at depth 0 within `text[start..end]`.
///
/// Tracks nested brackets: `()`, `[]`, `{}`, `<>`.
fn find_depth0_commas(text: &str, start: usize, end: usize) -> Vec<usize> {
    let mut commas = Vec::new();
    let mut depth = 0i32;

    for (i, ch) in text[start..end].char_indices() {
        match ch {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => commas.push(start + i),
            _ => {}
        }
    }

    commas
}

/// Find the argument segment containing the cursor.
///
/// Returns `(arg_start, arg_end, arg_index, total_args)`.
fn find_argument_segment(
    inner_start: usize,
    inner_end: usize,
    commas: &[usize],
    cursor: usize,
) -> Option<(usize, usize, usize, usize)> {
    let total_args = commas.len() + 1;

    // Build segment boundaries.
    let mut seg_start = inner_start;
    for (i, &comma_pos) in commas.iter().enumerate() {
        if cursor < comma_pos {
            return Some((seg_start, comma_pos, i, total_args));
        }
        seg_start = comma_pos + 1;
    }

    // Cursor is in the last (or only) segment.
    Some((seg_start, inner_end, commas.len(), total_args))
}

/// Skip leading whitespace, returning the first non-whitespace byte offset.
fn trim_leading_whitespace(text: &str, start: usize, end: usize) -> usize {
    let slice = &text[start..end];
    let trimmed = slice.trim_start_matches(|c: char| c.is_ascii_whitespace());
    end - trimmed.len()
}

/// Skip trailing whitespace, returning the last non-whitespace byte offset + 1.
fn trim_trailing_whitespace(text: &str, start: usize, end: usize) -> usize {
    let slice = &text[start..end];
    let trimmed = slice.trim_end_matches(|c: char| c.is_ascii_whitespace());
    start + trimmed.len()
}

/// Skip whitespace forward from `pos`, returning the next non-whitespace offset.
/// Bounded by `limit`.
fn skip_whitespace_forward(text: &str, pos: usize, limit: usize) -> usize {
    let slice = &text[pos..limit];
    let trimmed = slice.trim_start_matches(|c: char| c.is_ascii_whitespace());
    limit - trimmed.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, cursor: usize, scope: TextObjectScope, expected: Option<&str>) {
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_argument_object(&ctx, scope);
        match (result, expected) {
            (Some(r), Some(exp)) => {
                let selected = &text[r.start()..r.end()];
                assert_eq!(
                    selected, exp,
                    "cursor={cursor}, scope={scope:?}: got {selected:?}, expected {exp:?}"
                );
            }
            (None, None) => {}
            (Some(r), None) => {
                let selected = &text[r.start()..r.end()];
                panic!(
                    "cursor={cursor}, scope={scope:?}: expected None, got Some({selected:?}) [{},{})",
                    r.start(),
                    r.end()
                );
            }
            (None, Some(exp)) => {
                panic!("cursor={cursor}, scope={scope:?}: expected Some({exp:?}), got None");
            }
        }
    }

    // ── Basic cases ────────────────────────────────────────────────────

    #[test]
    fn ia_middle_arg() {
        // foo(bar, baz, qux) — cursor on 'b' of 'baz'
        check("foo(bar, baz, qux)", 9, TextObjectScope::Inner, Some("baz"));
    }

    #[test]
    fn aa_middle_arg() {
        // foo(bar, baz, qux) — cursor on 'b' of 'baz'
        check(
            "foo(bar, baz, qux)",
            9,
            TextObjectScope::Around,
            Some("baz, "),
        );
    }

    #[test]
    fn ia_first_arg() {
        // foo(bar, baz, qux) — cursor on 'b' of 'bar'
        check("foo(bar, baz, qux)", 4, TextObjectScope::Inner, Some("bar"));
    }

    #[test]
    fn aa_first_arg() {
        // foo(bar, baz, qux) — cursor on 'b' of 'bar'
        check(
            "foo(bar, baz, qux)",
            4,
            TextObjectScope::Around,
            Some("bar, "),
        );
    }

    #[test]
    fn ia_last_arg() {
        // foo(bar, baz, qux) — cursor on 'q' of 'qux'
        check(
            "foo(bar, baz, qux)",
            14,
            TextObjectScope::Inner,
            Some("qux"),
        );
    }

    #[test]
    fn aa_last_arg() {
        // foo(bar, baz, qux) — cursor on 'q' of 'qux'
        check(
            "foo(bar, baz, qux)",
            14,
            TextObjectScope::Around,
            Some(", qux"),
        );
    }

    // ── Nested brackets ────────────────────────────────────────────────

    #[test]
    fn ia_nested_inner_bracket() {
        // foo(bar(1, 2), baz) — cursor on '1': inner bracket pair is (1, 2)
        check("foo(bar(1, 2), baz)", 8, TextObjectScope::Inner, Some("1"));
    }

    #[test]
    fn ia_nested_outer_level() {
        // foo(bar(1, 2), baz) — cursor on 'b' of 'baz'
        check(
            "foo(bar(1, 2), baz)",
            15,
            TextObjectScope::Inner,
            Some("baz"),
        );
    }

    #[test]
    fn aa_nested_first_arg_with_nested_call() {
        // foo(bar(1, 2), baz) — cursor on 'b' of 'bar'
        check(
            "foo(bar(1, 2), baz)",
            4,
            TextObjectScope::Around,
            Some("bar(1, 2), "),
        );
    }

    // ── Single argument ────────────────────────────────────────────────

    #[test]
    fn ia_single_arg() {
        check("foo(bar)", 4, TextObjectScope::Inner, Some("bar"));
    }

    #[test]
    fn aa_single_arg() {
        // Single arg: around == inner (no comma).
        check("foo(bar)", 4, TextObjectScope::Around, Some("bar"));
    }

    // ── Edge cases ─────────────────────────────────────────────────────

    #[test]
    fn empty_parens() {
        check("foo()", 4, TextObjectScope::Inner, None);
    }

    #[test]
    fn no_brackets() {
        check("bare text", 3, TextObjectScope::Inner, None);
    }

    #[test]
    fn square_brackets() {
        check("[a, b, c]", 4, TextObjectScope::Inner, Some("b"));
    }

    #[test]
    fn curly_braces() {
        check("{x, y}", 1, TextObjectScope::Inner, Some("x"));
    }

    #[test]
    fn angle_brackets() {
        check("<A, B>", 1, TextObjectScope::Inner, Some("A"));
    }

    #[test]
    fn whitespace_trimming() {
        // Spaces around argument should be trimmed for inner.
        check(
            "foo(  bar  ,  baz  )",
            6,
            TextObjectScope::Inner,
            Some("bar"),
        );
    }

    #[test]
    fn not_linewise() {
        let ctx = TextObjectContext::new("foo(bar, baz)", 4);
        let result = compute_argument_object(&ctx, TextObjectScope::Inner);
        assert!(result.is_some());
        assert!(!result.unwrap().linewise);
    }

    // ── Cursor-on-comma and whitespace ────────────────────────────────

    #[test]
    fn ia_cursor_on_comma() {
        // foo(bar, baz) — cursor on ',' (byte 7).
        // Comma position equals cursor, so cursor falls into the NEXT
        // segment (the arg after the comma).
        check("foo(bar, baz)", 7, TextObjectScope::Inner, Some("baz"));
    }

    #[test]
    fn aa_cursor_on_comma() {
        // foo(bar, baz) — cursor on ',' (byte 7).
        // Around the next segment (baz is last) → include leading comma.
        check("foo(bar, baz)", 7, TextObjectScope::Around, Some(", baz"));
    }

    #[test]
    fn ia_cursor_on_whitespace_between_args() {
        // foo(bar, baz) — cursor on ' ' (byte 8) between comma and 'baz'.
        // Falls in the second segment; inner trims whitespace → "baz".
        check("foo(bar, baz)", 8, TextObjectScope::Inner, Some("baz"));
    }

    #[test]
    fn aa_cursor_on_whitespace_between_args() {
        // foo(bar, baz) — cursor on ' ' (byte 8).
        // baz is the last arg → around includes leading comma.
        check("foo(bar, baz)", 8, TextObjectScope::Around, Some(", baz"));
    }

    // ── Single arg with trailing comma ─────────────────────────────────

    #[test]
    fn ia_single_arg_trailing_comma() {
        // foo(bar,) — trailing comma creates two segments:
        // segment 0 = "bar", segment 1 = "" (empty).
        // Cursor on 'b' (byte 4) → inner selects "bar".
        check("foo(bar,)", 4, TextObjectScope::Inner, Some("bar"));
    }

    #[test]
    fn aa_single_arg_trailing_comma() {
        // foo(bar,) — cursor on 'b' (byte 4).
        // Segment 0 is not the last (trailing comma makes 2 segments),
        // so around includes trailing comma (no whitespace after it).
        check("foo(bar,)", 4, TextObjectScope::Around, Some("bar,"));
    }

    #[test]
    fn ia_trailing_comma_empty_segment() {
        // foo(bar,) — cursor on ')' is outside; cursor on ',' (byte 7)
        // lands in the empty second segment → inner returns None (empty trim).
        check("foo(bar,)", 7, TextObjectScope::Inner, None);
    }

    // ── Deeply nested arguments ───────────────────────────────────────

    #[test]
    fn ia_deeply_nested_cursor_on_middle_arg() {
        // f(g(h(1,2),3),4) — cursor on '3' (byte 11):
        // Tightest pair is g(h(1,2),3), inner = "h(1,2),3".
        // Depth-0 comma at byte 10. Cursor 11 > 10 → last segment = "3".
        check("f(g(h(1,2),3),4)", 11, TextObjectScope::Inner, Some("3"));
    }

    #[test]
    fn aa_deeply_nested_cursor_on_middle_arg() {
        // f(g(h(1,2),3),4) — cursor on '3' (byte 11):
        // Last arg of g() → around includes leading comma.
        check("f(g(h(1,2),3),4)", 11, TextObjectScope::Around, Some(",3"));
    }

    #[test]
    fn ia_deeply_nested_innermost_first_arg() {
        // f(g(h(1,2),3),4) — cursor on '1' (byte 6):
        // Tightest pair is h(1,2), inner = "1,2". First arg = "1".
        check("f(g(h(1,2),3),4)", 6, TextObjectScope::Inner, Some("1"));
    }

    #[test]
    fn ia_deeply_nested_outermost_arg() {
        // f(g(h(1,2),3),4) — cursor on '4' (byte 14):
        // Tightest pair is f(), last segment = "4".
        check("f(g(h(1,2),3),4)", 14, TextObjectScope::Inner, Some("4"));
    }

    #[test]
    fn aa_deeply_nested_outermost_last_arg() {
        // f(g(h(1,2),3),4) — cursor on '4': last arg of f() → leading comma.
        check("f(g(h(1,2),3),4)", 14, TextObjectScope::Around, Some(",4"));
    }

    #[test]
    fn ia_deeply_nested_first_arg_with_nested_call() {
        // f(g(h(1,2),3),4) — cursor on 'g' (byte 2):
        // Tightest pair is f(). First arg = "g(h(1,2),3)".
        // Nested commas inside g() and h() are at depth > 0.
        check(
            "f(g(h(1,2),3),4)",
            2,
            TextObjectScope::Inner,
            Some("g(h(1,2),3)"),
        );
    }

    // ── Multiline arguments ───────────────────────────────────────────

    #[test]
    fn ia_multiline_first_arg() {
        // Whitespace trimming strips all ASCII whitespace (incl. \n).
        // First segment "\n  bar" → trimmed to "bar".
        let text = "foo(\n  bar,\n  baz,\n  qux\n)";
        check(text, 7, TextObjectScope::Inner, Some("bar"));
    }

    #[test]
    fn ia_multiline_middle_arg() {
        // Second segment "\n  baz" → trimmed to "baz".
        let text = "foo(\n  bar,\n  baz,\n  qux\n)";
        check(text, 14, TextObjectScope::Inner, Some("baz"));
    }

    #[test]
    fn ia_multiline_last_arg() {
        // Last segment "\n  qux\n" → trimmed to "qux".
        let text = "foo(\n  bar,\n  baz,\n  qux\n)";
        check(text, 21, TextObjectScope::Inner, Some("qux"));
    }

    #[test]
    fn aa_multiline_last_arg() {
        // Last arg → around: from comma (byte 17) to trimmed end.
        // trim_trailing on "\n  qux\n" strips trailing \n → end at "qux".
        let text = "foo(\n  bar,\n  baz,\n  qux\n)";
        check(text, 21, TextObjectScope::Around, Some(",\n  qux"));
    }

    #[test]
    fn ia_multiline_same_line_args() {
        // First segment "\n  bar" → "bar". Second segment " baz\n" → "baz".
        let text = "foo(\n  bar, baz\n)";
        check(text, 7, TextObjectScope::Inner, Some("bar"));
        check(text, 12, TextObjectScope::Inner, Some("baz"));
    }

    // ── Generics (angle brackets shield nested commas) ────────────────

    #[test]
    fn ia_arg_with_generics_first() {
        // foo(Vec<i32>, bar) — '<' and '>' increase/decrease depth,
        // so the comma between > and bar is at depth 0. First arg = "Vec<i32>".
        check(
            "foo(Vec<i32>, bar)",
            4,
            TextObjectScope::Inner,
            Some("Vec<i32>"),
        );
    }

    #[test]
    fn ia_arg_with_generics_second() {
        check(
            "foo(Vec<i32>, bar)",
            14,
            TextObjectScope::Inner,
            Some("bar"),
        );
    }

    #[test]
    fn aa_arg_with_generics_first() {
        // Not the last arg → around includes trailing comma + whitespace.
        check(
            "foo(Vec<i32>, bar)",
            4,
            TextObjectScope::Around,
            Some("Vec<i32>, "),
        );
    }

    #[test]
    fn ia_arg_with_nested_generics() {
        // HashMap<String, Vec<i32>> has two levels of <>.
        // Inner: "HashMap<String, Vec<i32>>, bar"
        // Depth tracking: H=0.. <=1 .. ,=1(skip) .. <=2 .. >=1 .. >=0 ,=0(found) ..
        check(
            "foo(HashMap<String, Vec<i32>>, bar)",
            4,
            TextObjectScope::Inner,
            Some("HashMap<String, Vec<i32>>"),
        );
    }

    #[test]
    fn ia_arg_with_nested_generics_second() {
        check(
            "foo(HashMap<String, Vec<i32>>, bar)",
            30,
            TextObjectScope::Inner,
            Some("bar"),
        );
    }

    // ── String literals containing commas (pure-text limitation) ──────
    //
    // The pure-text fallback does NOT parse string literals, so commas
    // inside quoted strings are treated as argument separators. These
    // tests document the actual behavior. A syntax-aware provider
    // would produce better results and is tried first in the dispatch
    // chain.

    #[test]
    fn ia_string_with_comma_last_segment() {
        // foo("a,b", c) — pure-text sees three depth-0 commas:
        //   inner = `"a,b", c`
        //   commas at bytes 6 (inside "a,b") and 9 (between " and c).
        // Cursor on 'c' (byte 11): last segment → selects "c".
        check(r#"foo("a,b", c)"#, 11, TextObjectScope::Inner, Some("c"));
    }

    #[test]
    fn ia_string_with_comma_first_segment() {
        // foo("a,b", c) — cursor on '"' (byte 4).
        // First segment is from inner_start(4) to first comma(6): `"a`
        check(r#"foo("a,b", c)"#, 4, TextObjectScope::Inner, Some("\"a"));
    }
}
