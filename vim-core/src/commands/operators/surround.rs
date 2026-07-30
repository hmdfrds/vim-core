//! Surround operations (ys/ds/cs).
//!
//! Pure text transformations for adding, deleting, and changing surrounds.
//!
//! # Layering
//!
//! Imports `primitives` and `effects`; must not import `grammar`, `mode`,
//! `execution` or `dispatch`. Surround operations produce `Effects` and know
//! nothing about modes or grammar.

use crate::effects::Effects;
use crate::primitives::{Offset, Range};

/// A surround pair (open and close delimiters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurroundPair {
    /// Opening delimiter string.
    pub open: &'static str,
    /// Closing delimiter string.
    pub close: &'static str,
}

/// Resolve a character to its surround pair and whether it's the opening variant.
///
/// Opening chars (e.g., `(`, `{`, `[`, `<`) produce spaces around content.
/// Closing chars (e.g., `)`, `}`, `]`, `>`) produce no spaces.
/// Symmetric pairs (`'`, `"`, `` ` ``) never add spaces.
/// Unknown chars produce a symmetric pair of themselves.
#[must_use]
pub const fn resolve_pair(ch: char) -> (SurroundPair, bool) {
    match ch {
        '(' | ')' => (
            SurroundPair {
                open: "(",
                close: ")",
            },
            ch == '(',
        ),
        '{' | '}' => (
            SurroundPair {
                open: "{",
                close: "}",
            },
            ch == '{',
        ),
        '[' | ']' => (
            SurroundPair {
                open: "[",
                close: "]",
            },
            ch == '[',
        ),
        '<' | '>' => (
            SurroundPair {
                open: "<",
                close: ">",
            },
            ch == '<',
        ),
        '\'' => (
            SurroundPair {
                open: "'",
                close: "'",
            },
            false,
        ),
        '"' => (
            SurroundPair {
                open: "\"",
                close: "\"",
            },
            false,
        ),
        '`' => (
            SurroundPair {
                open: "`",
                close: "`",
            },
            false,
        ),
        _ => {
            // For unknown chars, we can't return a &'static str easily.
            // We'll handle this at the call site by using the char directly.
            // Return a placeholder pair — callers check for this case.
            (
                SurroundPair {
                    open: "",
                    close: "",
                },
                false,
            )
        }
    }
}

/// Check if the character is an "unknown" surround (not a recognized pair).
#[must_use]
pub const fn is_unknown_pair(ch: char) -> bool {
    !matches!(
        ch,
        '(' | ')' | '{' | '}' | '[' | ']' | '<' | '>' | '\'' | '"' | '`'
    )
}

/// Add surrounds around a text range.
///
/// Returns effects that insert the opening delimiter before the range start
/// and the closing delimiter after the range end.
///
/// When `opening` is true, spaces are added between delimiters and content:
/// `( content )` vs `(content)`.
pub fn add_surround(text: &str, range: Range, ch: char) -> Effects {
    let (pair, opening) = resolve_pair(ch);
    let start = range.start().get();
    let end = range.end().get().min(text.len());

    // Build the opening and closing strings
    let (open_str, close_str) = if is_unknown_pair(ch) {
        // Unknown char: use the char itself as symmetric pair, no spaces
        let mut open = String::with_capacity(1);
        open.push(ch);
        let close = open.clone();
        (open, close)
    } else if opening {
        // Opening variant: add spaces
        let mut open = String::from(pair.open);
        open.push(' ');
        let mut close = String::from(" ");
        close.push_str(pair.close);
        (open, close)
    } else {
        // Closing variant or symmetric: no spaces
        (pair.open.to_owned(), pair.close.to_owned())
    };

    // Insert close first (higher offset), then open (lower offset).
    // This avoids offset shifting issues — inserting at a later position
    // first means the earlier position is still valid.
    let effects = Effects::new()
        .insert(Offset::new(end), &close_str)
        .insert(Offset::new(start), &open_str);

    // Cursor goes to the start of the opening delimiter (standard vim-surround behavior)
    effects.set_cursor(Offset::new(start))
}

/// Delete surrounds identified by `ch` around the cursor position.
///
/// Finds the matching pair containing the cursor and removes both delimiters.
/// Returns empty effects if no matching pair is found.
pub fn delete_surround(text: &str, cursor: usize, ch: char) -> Effects {
    let Some((open_start, open_end, close_start, close_end)) = find_surrounding(text, cursor, ch)
    else {
        return Effects::new();
    };

    // Delete close first (higher offset), then open (lower offset).
    // This avoids offset shifting issues.
    let effects = Effects::new()
        .delete(Range::from_raw(close_start, close_end))
        .delete(Range::from_raw(open_start, open_end));

    // Cursor goes to where the opening delimiter was
    effects.set_cursor(Offset::new(open_start))
}

/// Change surrounds from `old_char` to `new_char` around the cursor.
///
/// Finds the pair identified by `old_char`, then replaces both delimiters
/// with the pair identified by `new_char`.
pub fn change_surround(text: &str, cursor: usize, old_char: char, new_char: char) -> Effects {
    let Some((open_start, open_end, close_start, close_end)) =
        find_surrounding(text, cursor, old_char)
    else {
        return Effects::new();
    };

    let (new_pair, new_opening) = resolve_pair(new_char);

    // Build the new opening and closing strings
    let (new_open, new_close) = if is_unknown_pair(new_char) {
        let mut s = String::with_capacity(1);
        s.push(new_char);
        (s.clone(), s)
    } else if new_opening {
        let mut open = String::from(new_pair.open);
        open.push(' ');
        let mut close = String::from(" ");
        close.push_str(new_pair.close);
        (open, close)
    } else {
        (new_pair.open.to_owned(), new_pair.close.to_owned())
    };

    // Replace close first (higher offset), then open (lower offset).
    let effects = Effects::new()
        .replace(Range::from_raw(close_start, close_end), &new_close)
        .replace(Range::from_raw(open_start, open_end), &new_open);

    effects.set_cursor(Offset::new(open_start))
}

/// Find the surrounding pair delimiters around the cursor position.
///
/// Returns `(open_start, open_end, close_start, close_end)` byte offsets,
/// or `None` if no matching pair is found.
///
/// For bracket-like pairs (parens, braces, brackets, angles), does nested matching.
/// For symmetric pairs (quotes), finds the enclosing pair on the same line.
fn find_surrounding(text: &str, cursor: usize, ch: char) -> Option<(usize, usize, usize, usize)> {
    let (pair, _opening) = resolve_pair(ch);

    if is_unknown_pair(ch) {
        // Unknown pair: treat as symmetric single-char delimiter
        return find_symmetric_surround(text, cursor, ch);
    }

    if pair.open == pair.close {
        // Symmetric pair (quotes)
        find_symmetric_surround(text, cursor, ch)
    } else {
        // Asymmetric pair (brackets) — nested matching
        find_bracket_surround(text, cursor, pair)
    }
}

/// Find a symmetric surround (quotes, backticks, or any single repeated char).
///
/// Searches the current line for the pair of delimiters enclosing the cursor.
fn find_symmetric_surround(
    text: &str,
    cursor: usize,
    ch: char,
) -> Option<(usize, usize, usize, usize)> {
    // Find line boundaries
    let line_start = text[..cursor].rfind('\n').map_or(0, |pos| pos + 1);
    let line_end = text[cursor..]
        .find('\n')
        .map_or(text.len(), |pos| cursor + pos);

    let line = &text[line_start..line_end];
    let cursor_in_line = cursor - line_start;

    // Find all occurrences of the char in this line
    let positions: Vec<usize> = line
        .char_indices()
        .filter(|(_, c)| *c == ch)
        .map(|(i, _)| i)
        .collect();

    // Find the pair that encloses the cursor
    // Try each consecutive pair of occurrences
    let mut i = 0;
    while i + 1 < positions.len() {
        let open = positions[i];
        let close = positions[i + 1];
        let open_end = open + ch.len_utf8();

        if open < cursor_in_line && close >= cursor_in_line {
            // This pair encloses the cursor (or cursor is on close)
            return Some((
                line_start + open,
                line_start + open_end,
                line_start + close,
                line_start + close + ch.len_utf8(),
            ));
        }
        // If cursor is ON the opening delimiter, also match
        if open == cursor_in_line {
            return Some((
                line_start + open,
                line_start + open_end,
                line_start + close,
                line_start + close + ch.len_utf8(),
            ));
        }
        i += 2; // Skip by pairs
    }

    None
}

/// Find a bracket-like surround with nesting support.
fn find_bracket_surround(
    text: &str,
    cursor: usize,
    pair: SurroundPair,
) -> Option<(usize, usize, usize, usize)> {
    let open_char = pair.open.chars().next()?;
    let close_char = pair.close.chars().next()?;
    let open_len = open_char.len_utf8();
    let close_len = close_char.len_utf8();

    // Search backward from cursor for the opening bracket
    let mut depth: i32 = 0;
    let mut open_pos = None;

    for (i, c) in text[..=cursor.min(text.len().saturating_sub(1))]
        .char_indices()
        .rev()
    {
        if c == close_char {
            depth += 1;
        } else if c == open_char {
            if depth == 0 {
                open_pos = Some(i);
                break;
            }
            depth -= 1;
        }
    }

    let open_start = open_pos?;

    // Search forward from cursor for the closing bracket
    depth = 0;
    let search_start = if cursor > open_start + open_len {
        cursor
    } else {
        open_start + open_len
    };

    let mut close_pos = None;
    for (i, c) in text[search_start..].char_indices() {
        if c == open_char {
            depth += 1;
        } else if c == close_char {
            if depth == 0 {
                close_pos = Some(search_start + i);
                break;
            }
            depth -= 1;
        }
    }

    let close_start = close_pos?;

    Some((
        open_start,
        open_start + open_len,
        close_start,
        close_start + close_len,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    // ═══════════════════════════════════════════════════════════════════
    // resolve_pair tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn resolve_pair_paren_opening() {
        let (pair, opening) = resolve_pair('(');
        assert_eq!(pair.open, "(");
        assert_eq!(pair.close, ")");
        assert!(opening);
    }

    #[test]
    fn resolve_pair_paren_closing() {
        let (pair, opening) = resolve_pair(')');
        assert_eq!(pair.open, "(");
        assert_eq!(pair.close, ")");
        assert!(!opening);
    }

    #[test]
    fn resolve_pair_brace_opening() {
        let (pair, opening) = resolve_pair('{');
        assert_eq!(pair.open, "{");
        assert_eq!(pair.close, "}");
        assert!(opening);
    }

    #[test]
    fn resolve_pair_double_quote() {
        let (pair, opening) = resolve_pair('"');
        assert_eq!(pair.open, "\"");
        assert_eq!(pair.close, "\"");
        assert!(!opening);
    }

    #[test]
    fn resolve_pair_unknown_char() {
        let (pair, opening) = resolve_pair('*');
        assert_eq!(pair.open, "");
        assert_eq!(pair.close, "");
        assert!(!opening);
        assert!(is_unknown_pair('*'));
    }

    // ═══════════════════════════════════════════════════════════════════
    // add_surround tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn add_surround_double_quote() {
        let text = "hello";
        let range = Range::from_raw(0, 5);
        let effects = add_surround(text, range, '"');

        // Should have Insert effects for both delimiters + SetCursor
        let inserts: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::Insert { .. }))
            .collect();
        assert_eq!(inserts.len(), 2);
    }

    #[test]
    fn add_surround_closing_brace() {
        let text = "hello";
        let range = Range::from_raw(0, 5);
        let effects = add_surround(text, range, '}');

        // Check that we get inserts without spaces (closing variant)
        let inserts: Vec<_> = effects.iter().collect();
        // Should find Insert at offset 5 with "}" and Insert at offset 0 with "{"
        let has_open = inserts.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "{"));
        let has_close = inserts.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 5 && text.as_str() == "}"));
        assert!(has_open, "should insert opening brace at 0");
        assert!(has_close, "should insert closing brace at 5");
    }

    #[test]
    fn add_surround_opening_brace_with_spaces() {
        let text = "hello";
        let range = Range::from_raw(0, 5);
        let effects = add_surround(text, range, '{');

        // Opening variant adds spaces: "{ " and " }"
        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "{ "));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 5 && text.as_str() == " }"));
        assert!(has_open, "should insert '{{ ' at 0");
        assert!(has_close, "should insert ' }}' at 5");
    }

    #[test]
    fn add_surround_unknown_char() {
        let text = "hello";
        let range = Range::from_raw(0, 5);
        let effects = add_surround(text, range, '*');

        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "*"));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 5 && text.as_str() == "*"));
        assert!(has_open, "should insert '*' at 0");
        assert!(has_close, "should insert '*' at 5");
    }

    // ═══════════════════════════════════════════════════════════════════
    // delete_surround tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn delete_surround_double_quote() {
        let text = "\"hello\"";
        let cursor = 3; // inside "hello"
        let effects = delete_surround(text, cursor, '"');

        // Should produce Delete effects removing both quotes
        let deletes: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .collect();
        assert_eq!(deletes.len(), 2, "should delete both quote chars");
    }

    #[test]
    fn delete_surround_parens() {
        let text = "(hello)";
        let cursor = 3; // inside
        let effects = delete_surround(text, cursor, ')');

        let deletes: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .collect();
        assert_eq!(deletes.len(), 2, "should delete both parens");
    }

    #[test]
    fn delete_surround_not_found() {
        let text = "hello";
        let cursor = 2;
        let effects = delete_surround(text, cursor, '"');
        assert!(
            effects.is_empty(),
            "should produce no effects when not found"
        );
    }

    #[test]
    fn delete_surround_nested_parens() {
        let text = "(a(b)c)";
        let cursor = 3; // on 'b' inside inner parens
        let effects = delete_surround(text, cursor, ')');

        // Should find the inner pair (a(b)c) → positions 2..5
        let deletes: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Delete { range } => Some((range.start().get(), range.end().get())),
                _ => None,
            })
            .collect();
        assert_eq!(deletes.len(), 2);
        // Close paren at position 4, open paren at position 2
        assert!(
            deletes.contains(&(4, 5)),
            "should delete close paren at 4..5"
        );
        assert!(
            deletes.contains(&(2, 3)),
            "should delete open paren at 2..3"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // change_surround tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn change_surround_quotes_to_single() {
        let text = "\"hello\"";
        let cursor = 3;
        let effects = change_surround(text, cursor, '"', '\'');

        let replaces: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::Replace { .. }))
            .collect();
        assert_eq!(replaces.len(), 2, "should replace both delimiters");
    }

    #[test]
    fn change_surround_not_found() {
        let text = "hello";
        let cursor = 2;
        let effects = change_surround(text, cursor, '"', '\'');
        assert!(
            effects.is_empty(),
            "should produce no effects when not found"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // find_surrounding tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn find_surrounding_quotes_basic() {
        let text = "\"hello\"";
        let result = find_surrounding(text, 3, '"');
        assert_eq!(result, Some((0, 1, 6, 7)));
    }

    #[test]
    fn find_surrounding_parens_basic() {
        let text = "(hello)";
        let result = find_surrounding(text, 3, ')');
        assert_eq!(result, Some((0, 1, 6, 7)));
    }

    #[test]
    fn find_surrounding_nested_outer() {
        let text = "((hello))";
        // cursor on 'h' (position 2), outer pair is 0..8
        let result = find_surrounding(text, 2, ')');
        // Should find inner first: open at 1, close at 7
        assert_eq!(result, Some((1, 2, 7, 8)));
    }

    #[test]
    fn find_surrounding_cursor_on_opening() {
        let text = "\"hello\"";
        let result = find_surrounding(text, 0, '"');
        assert_eq!(result, Some((0, 1, 6, 7)));
    }

    // ═══════════════════════════════════════════════════════════════════
    // Edge cases: unknown char surround
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn delete_surround_unknown_char() {
        // `*hello*` with cursor inside — delete the surrounding `*`s
        let text = "*hello*";
        let cursor = 3; // inside "hello"
        let effects = delete_surround(text, cursor, '*');

        let deletes: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::Delete { .. }))
            .collect();
        assert_eq!(deletes.len(), 2, "should delete both * chars");
    }

    #[test]
    fn change_surround_unknown_to_known() {
        // `*hello*` → `(hello)` via cs*}
        let text = "*hello*";
        let cursor = 3;
        let effects = change_surround(text, cursor, '*', ')');

        let replaces: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::Replace { .. }))
            .collect();
        assert_eq!(replaces.len(), 2, "should replace both * with parens");
    }

    #[test]
    fn change_surround_known_to_unknown() {
        // `"hello"` → `*hello*` via cs"*
        let text = "\"hello\"";
        let cursor = 3;
        let effects = change_surround(text, cursor, '"', '*');

        let replaces: Vec<_> = effects
            .iter()
            .filter(|e| matches!(e, Effect::Replace { .. }))
            .collect();
        assert_eq!(replaces.len(), 2, "should replace both quotes with *");

        // Verify the replacement text is "*"
        let replace_texts: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Replace { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            replace_texts.iter().all(|t| *t == "*"),
            "all replacements should be '*', got {:?}",
            replace_texts
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Edge cases: empty selection / zero-length range
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn add_surround_empty_range() {
        // Surround an empty range — should insert open+close at same point
        let text = "hello";
        let range = Range::from_raw(2, 2); // empty range at offset 2
        let effects = add_surround(text, range, '"');

        let inserts: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Insert { offset, text } => Some((offset.get(), text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(inserts.len(), 2, "should still produce two inserts");
        // Both inserts at offset 2: close first, then open
        assert!(
            inserts.iter().all(|(off, _)| *off == 2),
            "both inserts should be at offset 2, got {:?}",
            inserts
        );
    }

    #[test]
    fn add_surround_empty_text() {
        // Surround the entirety of an empty string
        let text = "";
        let range = Range::from_raw(0, 0);
        let effects = add_surround(text, range, ')');

        let inserts: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Insert { offset, text } => Some((offset.get(), text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(inserts.len(), 2);
        // Should produce "(" at 0 and ")" at 0
        assert!(inserts.contains(&(0, "(")), "should have open paren insert");
        assert!(
            inserts.contains(&(0, ")")),
            "should have close paren insert"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Edge cases: delete/change surround that doesn't exist (no-op)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn delete_surround_no_brackets_is_noop() {
        let text = "hello world";
        let cursor = 5;
        let effects = delete_surround(text, cursor, ')');
        assert!(effects.is_empty(), "no parens exist — should be no-op");
    }

    #[test]
    fn delete_surround_no_quotes_is_noop() {
        let text = "hello world";
        let cursor = 3;
        let effects = delete_surround(text, cursor, '\'');
        assert!(effects.is_empty(), "no quotes exist — should be no-op");
    }

    #[test]
    fn delete_surround_unknown_char_not_found_is_noop() {
        let text = "hello world";
        let cursor = 5;
        let effects = delete_surround(text, cursor, '*');
        assert!(effects.is_empty(), "no * surrounds — should be no-op");
    }

    #[test]
    fn change_surround_not_found_is_noop() {
        let text = "hello world";
        let cursor = 5;
        let effects = change_surround(text, cursor, ')', ']');
        assert!(effects.is_empty(), "no parens exist — should be no-op");
    }

    #[test]
    fn change_surround_unknown_not_found_is_noop() {
        let text = "hello world";
        let cursor = 3;
        let effects = change_surround(text, cursor, '*', '"');
        assert!(effects.is_empty(), "no * surrounds — should be no-op");
    }

    // ═══════════════════════════════════════════════════════════════════
    // Edge cases: angle brackets (<>)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn resolve_pair_angle_opening() {
        let (pair, opening) = resolve_pair('<');
        assert_eq!(pair.open, "<");
        assert_eq!(pair.close, ">");
        assert!(opening);
    }

    #[test]
    fn resolve_pair_angle_closing() {
        let (pair, opening) = resolve_pair('>');
        assert_eq!(pair.open, "<");
        assert_eq!(pair.close, ">");
        assert!(!opening);
    }

    #[test]
    fn add_surround_angle_opening_with_spaces() {
        let text = "hello";
        let range = Range::from_raw(0, 5);
        let effects = add_surround(text, range, '<');

        // Opening angle bracket adds spaces: "< " and " >"
        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "< "));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 5 && text.as_str() == " >"));
        assert!(has_open, "should insert '< ' at 0");
        assert!(has_close, "should insert ' >' at 5");
    }

    #[test]
    fn add_surround_angle_closing_no_spaces() {
        let text = "hello";
        let range = Range::from_raw(0, 5);
        let effects = add_surround(text, range, '>');

        // Closing angle bracket: no spaces
        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "<"));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 5 && text.as_str() == ">"));
        assert!(has_open, "should insert '<' at 0");
        assert!(has_close, "should insert '>' at 5");
    }

    #[test]
    fn delete_surround_angle_brackets() {
        let text = "<hello>";
        let cursor = 3;
        let effects = delete_surround(text, cursor, '>');

        let deletes: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Delete { range } => Some((range.start().get(), range.end().get())),
                _ => None,
            })
            .collect();
        assert_eq!(deletes.len(), 2);
        assert!(deletes.contains(&(0, 1)), "should delete < at 0..1");
        assert!(deletes.contains(&(6, 7)), "should delete > at 6..7");
    }

    #[test]
    fn change_surround_angle_to_parens() {
        let text = "<hello>";
        let cursor = 3;
        let effects = change_surround(text, cursor, '>', ')');

        let replaces: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Replace { range, text } => {
                    Some((range.start().get(), range.end().get(), text.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(replaces.len(), 2);
        // Should replace < with ( and > with )
        assert!(
            replaces
                .iter()
                .any(|(s, e, t)| *s == 0 && *e == 1 && *t == "("),
            "should replace < with ("
        );
        assert!(
            replaces
                .iter()
                .any(|(s, e, t)| *s == 6 && *e == 7 && *t == ")"),
            "should replace > with )"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Edge cases: nested surround
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn delete_surround_nested_outer_parens() {
        // ((text)) — cursor on 't' at position 2
        // From position 2, searching backward the first unmatched '(' is at 1.
        // The inner pair is the one found.
        let text = "((text))";
        let cursor = 2; // on 't'
        let effects = delete_surround(text, cursor, ')');

        let deletes: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Delete { range } => Some((range.start().get(), range.end().get())),
                _ => None,
            })
            .collect();
        assert_eq!(deletes.len(), 2, "should delete a pair");
        // The inner pair: open at 1, close at 6
        assert!(
            deletes.contains(&(1, 2)),
            "should delete inner open paren at 1..2, got {:?}",
            deletes
        );
        assert!(
            deletes.contains(&(6, 7)),
            "should delete inner close paren at 6..7, got {:?}",
            deletes
        );
    }

    #[test]
    fn find_surrounding_deeply_nested() {
        // (((x))) — cursor on 'x' at position 3
        let text = "(((x)))";
        let cursor = 3; // on 'x'
        let result = find_surrounding(text, cursor, ')');
        // Should find innermost pair: open at 2, close at 4
        assert_eq!(result, Some((2, 3, 4, 5)));
    }

    #[test]
    fn delete_surround_nested_braces() {
        // { { inner } } — cursor inside inner
        let text = "{ { inner } }";
        let cursor = 6; // on 'i'
        let effects = delete_surround(text, cursor, '}');

        let deletes: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Delete { range } => Some((range.start().get(), range.end().get())),
                _ => None,
            })
            .collect();
        assert_eq!(deletes.len(), 2, "should delete inner brace pair");
        // Inner braces: open at 2, close at 10
        assert!(
            deletes.contains(&(2, 3)),
            "should delete inner open brace, got {:?}",
            deletes
        );
        assert!(
            deletes.contains(&(10, 11)),
            "should delete inner close brace, got {:?}",
            deletes
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Opening vs closing verification (space insertion logic)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn add_surround_opening_paren_has_spaces() {
        let text = "word";
        let range = Range::from_raw(0, 4);
        let effects = add_surround(text, range, '(');

        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "( "));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 4 && text.as_str() == " )"));
        assert!(has_open, "opening '(' should produce '( '");
        assert!(has_close, "opening '(' should produce ' )'");
    }

    #[test]
    fn add_surround_closing_paren_no_spaces() {
        let text = "word";
        let range = Range::from_raw(0, 4);
        let effects = add_surround(text, range, ')');

        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "("));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 4 && text.as_str() == ")"));
        assert!(has_open, "closing ')' should produce '(' without space");
        assert!(has_close, "closing ')' should produce ')' without space");
    }

    #[test]
    fn add_surround_opening_bracket_has_spaces() {
        let text = "data";
        let range = Range::from_raw(0, 4);
        let effects = add_surround(text, range, '[');

        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "[ "));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 4 && text.as_str() == " ]"));
        assert!(has_open, "opening '[' should produce '[ '");
        assert!(has_close, "opening '[' should produce ' ]'");
    }

    #[test]
    fn add_surround_closing_bracket_no_spaces() {
        let text = "data";
        let range = Range::from_raw(0, 4);
        let effects = add_surround(text, range, ']');

        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "["));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 4 && text.as_str() == "]"));
        assert!(has_open, "closing ']' should produce '[' without space");
        assert!(has_close, "closing ']' should produce ']' without space");
    }

    #[test]
    fn add_surround_quote_never_has_spaces() {
        let text = "word";
        let range = Range::from_raw(0, 4);
        let effects = add_surround(text, range, '"');

        // Quotes are symmetric — never add spaces
        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "\""));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 4 && text.as_str() == "\""));
        assert!(has_open, "quote should produce '\"' without space");
        assert!(has_close, "quote should produce '\"' without space");
    }

    #[test]
    fn add_surround_backtick_never_has_spaces() {
        let text = "code";
        let range = Range::from_raw(0, 4);
        let effects = add_surround(text, range, '`');

        let has_open = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 0 && text.as_str() == "`"));
        let has_close = effects.iter().any(|e| matches!(e, Effect::Insert { offset, text } if offset.get() == 4 && text.as_str() == "`"));
        assert!(has_open, "backtick should produce '`' without space");
        assert!(has_close, "backtick should produce '`' without space");
    }

    // ═══════════════════════════════════════════════════════════════════
    // Change surround: opening/closing space behavior
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn change_surround_to_opening_adds_spaces() {
        // "hello" → cs"{ → should become { hello }
        let text = "\"hello\"";
        let cursor = 3;
        let effects = change_surround(text, cursor, '"', '{');

        let replaces: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Replace { range, text } => {
                    Some((range.start().get(), range.end().get(), text.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(replaces.len(), 2);
        // Opening brace replacement includes space
        assert!(
            replaces
                .iter()
                .any(|(s, e, t)| *s == 0 && *e == 1 && *t == "{ "),
            "should replace opening \" with '{{ ', got {:?}",
            replaces
        );
        assert!(
            replaces
                .iter()
                .any(|(s, e, t)| *s == 6 && *e == 7 && *t == " }"),
            "should replace closing \" with ' }}', got {:?}",
            replaces
        );
    }

    #[test]
    fn change_surround_to_closing_no_spaces() {
        // "hello" → cs"} → should become {hello}
        let text = "\"hello\"";
        let cursor = 3;
        let effects = change_surround(text, cursor, '"', '}');

        let replaces: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Replace { range, text } => {
                    Some((range.start().get(), range.end().get(), text.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(replaces.len(), 2);
        assert!(
            replaces
                .iter()
                .any(|(s, e, t)| *s == 0 && *e == 1 && *t == "{"),
            "should replace opening \" with '{{', got {:?}",
            replaces
        );
        assert!(
            replaces
                .iter()
                .any(|(s, e, t)| *s == 6 && *e == 7 && *t == "}"),
            "should replace closing \" with '}}', got {:?}",
            replaces
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Cursor positioning after surround operations
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn add_surround_cursor_at_range_start() {
        let text = "hello";
        let range = Range::from_raw(0, 5);
        let effects = add_surround(text, range, '"');

        let cursor_effect = effects
            .iter()
            .find(|e| matches!(e, Effect::SetCursor { .. }));
        assert!(cursor_effect.is_some(), "should set cursor");
        if let Some(Effect::SetCursor { offset }) = cursor_effect {
            assert_eq!(
                offset.get(),
                0,
                "cursor should go to start of opening delimiter"
            );
        }
    }

    #[test]
    fn delete_surround_cursor_at_open_position() {
        let text = "\"hello\"";
        let cursor = 3;
        let effects = delete_surround(text, cursor, '"');

        let cursor_effect = effects
            .iter()
            .find(|e| matches!(e, Effect::SetCursor { .. }));
        assert!(cursor_effect.is_some(), "should set cursor");
        if let Some(Effect::SetCursor { offset }) = cursor_effect {
            assert_eq!(
                offset.get(),
                0,
                "cursor should go to where opening delimiter was"
            );
        }
    }

    #[test]
    fn change_surround_cursor_at_open_position() {
        let text = "(hello)";
        let cursor = 3;
        let effects = change_surround(text, cursor, ')', '"');

        let cursor_effect = effects
            .iter()
            .find(|e| matches!(e, Effect::SetCursor { .. }));
        assert!(cursor_effect.is_some(), "should set cursor");
        if let Some(Effect::SetCursor { offset }) = cursor_effect {
            assert_eq!(
                offset.get(),
                0,
                "cursor should go to where opening delimiter was"
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Multi-line surround
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn find_surrounding_brackets_multiline() {
        let text = "(\nhello\n)";
        let cursor = 3; // on 'h' in second line
        let result = find_surrounding(text, cursor, ')');
        assert_eq!(result, Some((0, 1, 8, 9)));
    }

    #[test]
    fn delete_surround_multiline_brackets() {
        let text = "{\n  code\n}";
        let cursor = 5; // inside "code"
        let effects = delete_surround(text, cursor, '}');

        let deletes: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Delete { range } => Some((range.start().get(), range.end().get())),
                _ => None,
            })
            .collect();
        assert_eq!(deletes.len(), 2);
        assert!(deletes.contains(&(0, 1)), "should delete {{ at 0..1");
        assert!(deletes.contains(&(9, 10)), "should delete }} at 9..10");
    }

    #[test]
    fn find_symmetric_surround_same_line_only() {
        // Symmetric surround (quotes) only searches the current line
        let text = "\"line1\"\n\"line2\"";
        let cursor = 3; // inside "line1" on first line
        let result = find_surrounding(text, cursor, '"');
        // Should find the pair on line 1 only: positions 0 and 6
        assert_eq!(result, Some((0, 1, 6, 7)));
    }
}
