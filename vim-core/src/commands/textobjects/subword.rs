//! Subword text objects (iS, aS).
//!
//! Selects camelCase/snake_case sub-parts using the same boundary logic as
//! subword motions (`commands/motions/subword.rs`).
//!
//! - `iS` selects the subword under cursor (between boundaries).
//! - `aS` includes trailing separator/whitespace (or leading if at end).

use super::helpers::{next_char_boundary, prev_char_boundary};
use super::types::{TextObjectContext, TextObjectRange};
use crate::commands::motions::subword::{classify_subword, is_subword_start, SubwordClass};
use crate::grammar::types::TextObjectScope;
use crate::primitives::SubwordConfig;

/// Compute a subword text object.
///
/// # Arguments
///
/// * `ctx`   - Text object context (text + cursor + subword config).
/// * `scope` - `Inner` (`iS`) or `Around` (`aS`).
///
/// # Returns
///
/// `None` when the buffer is empty or the cursor is on whitespace/non-word.
#[must_use]
pub fn compute_subword_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    if text.is_empty() {
        return None;
    }

    let config = ctx.subword_config;

    // Clamp cursor to last valid char boundary if past end.
    let cursor = if ctx.cursor.get() >= text.len() {
        crate::primitives::text_util::prev_char_boundary(text, text.len())
    } else {
        ctx.cursor.get()
    };

    // Get the character under cursor and classify it.
    let cursor_char = text[cursor..].chars().next()?;
    let cursor_class = classify_subword(cursor_char, config);

    // If cursor is on whitespace (NonWord), no subword to select.
    if cursor_class == SubwordClass::NonWord {
        return None;
    }

    // If cursor is on a separator, select just the separator for inner.
    if cursor_class == SubwordClass::Separator {
        let sep_end = next_char_boundary(text, cursor);
        if scope.is_inner() {
            return Some(TextObjectRange::char(cursor, sep_end));
        }
        // For around on separator: include trailing word chars or leading word chars.
        return Some(compute_around_separator(text, cursor, sep_end, config));
    }

    // Find the start of the subword containing the cursor.
    let start = find_subword_start_at(text, cursor, config);

    // Find the end of the subword containing the cursor (exclusive).
    let end = find_subword_end_at(text, cursor, config);

    if start >= end {
        return None;
    }

    if scope.is_inner() {
        Some(TextObjectRange::char(start, end))
    } else {
        Some(compute_around_subword(text, start, end, config))
    }
}

/// Find the start of the subword that contains `pos`.
///
/// Walks backward from `pos` until we hit a subword boundary or the text start.
fn find_subword_start_at(text: &str, pos: usize, config: &SubwordConfig) -> usize {
    // If pos is itself a subword start, that's our answer.
    if is_subword_start(text, pos, config) {
        return pos;
    }

    // Walk backward to find the start.
    let mut p = pos;
    loop {
        if p == 0 {
            return 0;
        }
        p = prev_char_boundary(text, p);
        if is_subword_start(text, p, config) {
            return p;
        }
        // Stop if we hit a non-word character (shouldn't be part of any subword).
        if let Some(c) = text[p..].chars().next() {
            let class = classify_subword(c, config);
            if matches!(class, SubwordClass::NonWord | SubwordClass::Separator) {
                // The subword starts at the char after this non-word/separator.
                return next_char_boundary(text, p);
            }
        }
    }
}

/// Find the exclusive end of the subword that contains `pos`.
///
/// Walks forward from `pos` until we hit the next subword boundary, a separator,
/// or non-word character.
fn find_subword_end_at(text: &str, pos: usize, config: &SubwordConfig) -> usize {
    let mut p = next_char_boundary(text, pos);
    while p < text.len() {
        if let Some(c) = text[p..].chars().next() {
            let class = classify_subword(c, config);
            if matches!(class, SubwordClass::NonWord | SubwordClass::Separator) {
                return p;
            }
            if is_subword_start(text, p, config) {
                return p;
            }
        }
        p = next_char_boundary(text, p);
    }
    text.len()
}

/// Compute "around" subword: include trailing separator/whitespace, or leading if at end.
fn compute_around_subword(
    text: &str,
    start: usize,
    end: usize,
    config: &SubwordConfig,
) -> TextObjectRange {
    // Try trailing: consume separators and whitespace after the subword.
    let trailing_end = skip_separators_and_whitespace_forward(text, end, config);
    if trailing_end > end {
        return TextObjectRange::char(start, trailing_end);
    }

    // No trailing — try leading: consume separators and whitespace before the subword.
    let leading_start = skip_separators_and_whitespace_backward(text, start, config);
    TextObjectRange::char(leading_start, end)
}

/// Compute "around" for when cursor is on a separator.
fn compute_around_separator(
    text: &str,
    sep_start: usize,
    sep_end: usize,
    config: &SubwordConfig,
) -> TextObjectRange {
    // Try including the next subword after the separator.
    if sep_end < text.len() {
        if let Some(c) = text[sep_end..].chars().next() {
            let class = classify_subword(c, config);
            if !matches!(class, SubwordClass::NonWord | SubwordClass::Separator) {
                let word_end = find_subword_end_at(text, sep_end, config);
                return TextObjectRange::char(sep_start, word_end);
            }
        }
    }
    // No following word — try including the previous subword.
    if sep_start > 0 {
        let prev = prev_char_boundary(text, sep_start);
        if let Some(c) = text[prev..].chars().next() {
            let class = classify_subword(c, config);
            if !matches!(class, SubwordClass::NonWord | SubwordClass::Separator) {
                let word_start = find_subword_start_at(text, prev, config);
                return TextObjectRange::char(word_start, sep_end);
            }
        }
    }
    // Isolated separator — just select it.
    TextObjectRange::char(sep_start, sep_end)
}

/// Advance forward over separators and horizontal whitespace.
/// Stops at newlines so we never cross a line boundary.
fn skip_separators_and_whitespace_forward(
    text: &str,
    mut pos: usize,
    config: &SubwordConfig,
) -> usize {
    while pos < text.len() {
        if let Some(c) = text[pos..].chars().next() {
            if c == ' ' || c == '\t' || config.is_separator(c) {
                pos = next_char_boundary(text, pos);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    pos
}

/// Retreat backward over separators (NOT whitespace).
///
/// For leading absorption in `aS`, we only absorb configured separators
/// (like `_`, `.`, `-`) because those are legitimate subword delimiters.
/// Whitespace before a subword typically belongs to word separation, not
/// subword separation (e.g., in `"foo camelCase"`, the space belongs to
/// the word boundary, not to the "Case" subword).
fn skip_separators_and_whitespace_backward(
    text: &str,
    mut pos: usize,
    config: &SubwordConfig,
) -> usize {
    loop {
        if pos == 0 {
            break;
        }
        let prev = prev_char_boundary(text, pos);
        if let Some(c) = text[prev..].chars().next() {
            if config.is_separator(c) {
                pos = prev;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, cursor: usize, inner: bool, expected: Option<(&str, usize, usize)>) {
        let ctx = TextObjectContext::new(text, cursor);
        let scope = TextObjectScope::from_inner_flag(inner);
        let result = compute_subword_object(&ctx, scope);
        match (result, expected) {
            (Some(r), Some((slice, s, e))) => {
                assert_eq!(r.start(), s, "start mismatch for {:?} at {}", text, cursor);
                assert_eq!(r.end(), e, "end mismatch for {:?} at {}", text, cursor);
                assert_eq!(
                    &text[r.start()..r.end()],
                    slice,
                    "slice mismatch for {:?} at {}",
                    text,
                    cursor,
                );
            }
            (None, None) => {}
            (got, exp) => panic!(
                "text={:?}, cursor={}, inner={}: got {:?}, expected {:?}",
                text, cursor, inner, got, exp,
            ),
        }
    }

    // ─── Inner (iS) — camelCase ──────────────────────────────────────────────

    #[test]
    fn is_camel_case_first_subword() {
        // "camelCaseWord" — cursor on 'a' (pos 1), should select "camel" (0..5)
        check("camelCaseWord", 1, true, Some(("camel", 0, 5)));
    }

    #[test]
    fn is_camel_case_second_subword() {
        // "camelCaseWord" — cursor on 'C' (pos 5), should select "Case" (5..9)
        check("camelCaseWord", 5, true, Some(("Case", 5, 9)));
    }

    #[test]
    fn is_camel_case_third_subword() {
        // "camelCaseWord" — cursor on 'W' (pos 9), should select "Word" (9..13)
        check("camelCaseWord", 9, true, Some(("Word", 9, 13)));
    }

    #[test]
    fn is_camel_case_middle_of_subword() {
        // "camelCaseWord" — cursor on 'a' (pos 6), should select "Case" (5..9)
        check("camelCaseWord", 6, true, Some(("Case", 5, 9)));
    }

    // ─── Inner (iS) — snake_case ─────────────────────────────────────────────

    #[test]
    fn is_snake_case_first_subword() {
        // "snake_case_word" — cursor on 's' (pos 0), should select "snake" (0..5)
        check("snake_case_word", 0, true, Some(("snake", 0, 5)));
    }

    #[test]
    fn is_snake_case_second_subword() {
        // "snake_case_word" — cursor on 'c' (pos 6), should select "case" (6..10)
        check("snake_case_word", 6, true, Some(("case", 6, 10)));
    }

    #[test]
    fn is_snake_case_third_subword() {
        // "snake_case_word" — cursor on 'w' (pos 11), should select "word" (11..15)
        check("snake_case_word", 11, true, Some(("word", 11, 15)));
    }

    // ─── Inner (iS) — cursor on separator ────────────────────────────────────

    #[test]
    fn is_cursor_on_underscore_separator() {
        // "snake_case" — cursor on '_' (pos 5), selects just the separator
        check("snake_case", 5, true, Some(("_", 5, 6)));
    }

    #[test]
    fn is_cursor_on_dot_separator() {
        // "foo.bar" — cursor on '.' (pos 3), selects just the dot
        check("foo.bar", 3, true, Some((".", 3, 4)));
    }

    // ─── Inner (iS) — acronyms ───────────────────────────────────────────────

    #[test]
    fn is_acronym_xml() {
        // "XMLParser" — cursor on 'X' (pos 0), should select "XML" (0..3)
        check("XMLParser", 0, true, Some(("XML", 0, 3)));
    }

    #[test]
    fn is_acronym_parser_part() {
        // "XMLParser" — cursor on 'P' (pos 3), should select "Parser" (3..9)
        check("XMLParser", 3, true, Some(("Parser", 3, 9)));
    }

    #[test]
    fn is_all_uppercase() {
        // "HTTP" — entire word is one subword (no boundary within consecutive uppercase)
        check("HTTP", 0, true, Some(("HTTP", 0, 4)));
        check("HTTP", 2, true, Some(("HTTP", 0, 4)));
    }

    // ─── Inner (iS) — digit transitions ──────────────────────────────────────

    #[test]
    fn is_digit_alpha_transition() {
        // "var123name" — boundaries at 0, 3, 6
        check("var123name", 0, true, Some(("var", 0, 3)));
        check("var123name", 3, true, Some(("123", 3, 6)));
        check("var123name", 6, true, Some(("name", 6, 10)));
    }

    // ─── Inner (iS) — single char subword ────────────────────────────────────

    #[test]
    fn is_single_char_subword() {
        // "getIDs" — 'I' at pos 3 is a single-char subword before 'D'
        // Boundaries: get(0), I(3), Ds(4)
        // Actually: g-e-t are Lower, I is Upper, D is Upper, s is Lower
        // Lower→Upper at I(3): boundary. Upper→Upper+Lower at D(4)→s: acronym boundary at D.
        // So subwords: "get"(0..3), "I"(3..4), "Ds"(4..6)
        check("getIDs", 3, true, Some(("I", 3, 4)));
    }

    // ─── Inner (iS) — edge cases ─────────────────────────────────────────────

    #[test]
    fn is_empty_text() {
        check("", 0, true, None);
    }

    #[test]
    fn is_cursor_on_whitespace() {
        check("foo bar", 3, true, None);
    }

    #[test]
    fn is_single_word() {
        check("hello", 2, true, Some(("hello", 0, 5)));
    }

    #[test]
    fn is_cursor_past_end() {
        check("hello", 100, true, Some(("hello", 0, 5)));
    }

    #[test]
    fn is_at_line_boundary() {
        // Subword never crosses lines
        check("foo\nbar", 0, true, Some(("foo", 0, 3)));
        check("foo\nbar", 4, true, Some(("bar", 4, 7)));
    }

    // ─── Around (aS) — camelCase ─────────────────────────────────────────────

    #[test]
    fn as_camel_case_first_subword_no_leading() {
        // "camelCaseWord" — cursor on 'a' (pos 1)
        // aS on first subword: no leading ws/sep, but there IS no trailing sep/ws
        // between subwords in camelCase. The subwords are directly adjacent.
        // So "around" just returns the inner.
        check("camelCaseWord", 1, false, Some(("camel", 0, 5)));
    }

    #[test]
    fn as_snake_case_includes_trailing_separator() {
        // "snake_case_word" — cursor on 's' (pos 0)
        // aS should include trailing separator: "snake_"
        check("snake_case_word", 0, false, Some(("snake_", 0, 6)));
    }

    #[test]
    fn as_snake_case_last_subword_includes_leading_separator() {
        // "snake_case_word" — cursor on 'w' (pos 11)
        // No trailing separator, so include leading: "_word"
        check("snake_case_word", 11, false, Some(("_word", 10, 15)));
    }

    #[test]
    fn as_snake_case_middle_subword() {
        // "snake_case_word" — cursor on 'c' (pos 6)
        // Trailing separator exists: "case_"
        check("snake_case_word", 6, false, Some(("case_", 6, 11)));
    }

    // ─── Around (aS) — with whitespace ───────────────────────────────────────

    #[test]
    fn as_with_trailing_whitespace() {
        // "camelCase word" — cursor on 'C' (pos 5), subword is "Case"(5..9)
        // After "Case" there's a space — trailing whitespace included
        check("camelCase word", 5, false, Some(("Case ", 5, 10)));
    }

    #[test]
    fn as_last_subword_no_trailing() {
        // "foo camelCase" — cursor on 'C' (pos 9)
        // "Case" is 9..13, no trailing ws. camelCase subwords are adjacent,
        // no leading separator either. So just inner.
        check("foo camelCase", 9, false, Some(("Case", 9, 13)));
    }

    // ─── Around (aS) — cursor on separator ───────────────────────────────────

    #[test]
    fn as_cursor_on_separator_includes_next_word() {
        // "snake_case" — cursor on '_' (pos 5)
        // Around separator: include the next subword → "_case"
        check("snake_case", 5, false, Some(("_case", 5, 10)));
    }

    #[test]
    fn as_cursor_on_trailing_separator() {
        // "word_" — cursor on '_' (pos 4), no following word
        // Include previous subword: "word_"
        check("word_", 4, false, Some(("word_", 0, 5)));
    }

    // ─── kebab-case ──────────────────────────────────────────────────────────

    #[test]
    fn is_kebab_case() {
        check("kebab-case-word", 0, true, Some(("kebab", 0, 5)));
        check("kebab-case-word", 6, true, Some(("case", 6, 10)));
        check("kebab-case-word", 11, true, Some(("word", 11, 15)));
    }

    #[test]
    fn as_kebab_case_includes_trailing_separator() {
        check("kebab-case-word", 0, false, Some(("kebab-", 0, 6)));
    }

    // ─── dot.separated ───────────────────────────────────────────────────────

    #[test]
    fn is_dot_separated() {
        check("foo.bar.baz", 0, true, Some(("foo", 0, 3)));
        check("foo.bar.baz", 4, true, Some(("bar", 4, 7)));
        check("foo.bar.baz", 8, true, Some(("baz", 8, 11)));
    }

    #[test]
    fn as_dot_separated() {
        check("foo.bar.baz", 0, false, Some(("foo.", 0, 4)));
        check("foo.bar.baz", 8, false, Some((".baz", 7, 11)));
    }
}
