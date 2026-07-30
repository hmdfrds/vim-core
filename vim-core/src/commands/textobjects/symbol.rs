//! Symbol text objects (im, am).
//!
//! A "symbol" is a contiguous run of programming symbol characters:
//! alphanumerics, underscore, dot, colon, dollar, at, hash, and dash
//! (as used in `->` and similar constructs).
//!
//! This is similar to Evil mode's symbol text object.  Useful for selecting
//! qualified identifiers like `foo.bar.baz` or `std::collections::HashMap`
//! as one unit.
//!
//! - `im` selects only the symbol characters (inner).
//! - `am` includes one side of surrounding whitespace (trailing preferred, with
//!   leading as fallback when no trailing whitespace exists).

use super::helpers::{next_char_boundary, prev_char_boundary};
use super::types::{TextObjectContext, TextObjectRange};
use crate::grammar::types::TextObjectScope;

/// Returns `true` if `c` is a symbol character.
///
/// Symbol characters are: `[a-zA-Z0-9_.$@#:-]`
///
/// Note: `-` is included because it appears in common sigils (`->`, `--`).
/// It is NOT treated as a minus operator here; context-free scanning is used.
#[inline]
#[must_use]
const fn is_symbol_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '$' | '@' | '#' | '-')
}

/// Find the start (byte offset) of the symbol run containing `pos`.
///
/// Walks backward while each preceding character is a symbol character.
fn symbol_start(text: &str, pos: usize) -> usize {
    let mut start = pos;
    loop {
        if start == 0 {
            break;
        }
        let prev = prev_char_boundary(text, start);
        if let Some(c) = text[prev..].chars().next() {
            if is_symbol_char(c) {
                start = prev;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    start
}

/// Find the end (exclusive byte offset) of the symbol run containing `pos`.
///
/// Walks forward while each character at or after `pos` is a symbol character.
fn symbol_end(text: &str, pos: usize) -> usize {
    let mut end = pos;
    while end < text.len() {
        if let Some(c) = text[end..].chars().next() {
            if is_symbol_char(c) {
                end = next_char_boundary(text, end);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    end
}

/// Compute the symbol text object.
///
/// # Arguments
///
/// * `ctx`   – Text object context (text + cursor byte offset).
/// * `scope` – `Inner` (`im`) or `Around` (`am`).
///
/// # Returns
///
/// `None` when the buffer is empty or the cursor is not on a symbol character.
#[must_use]
pub fn compute_symbol_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    if text.is_empty() {
        return None;
    }

    // Clamp cursor to last valid char boundary if past end.
    let cursor = if ctx.cursor.get() >= text.len() {
        crate::primitives::text_util::prev_char_boundary(text, text.len())
    } else {
        ctx.cursor.get()
    };

    // The character under the cursor must be a symbol character.
    let cursor_char = text[cursor..].chars().next()?;
    if !is_symbol_char(cursor_char) {
        return None;
    }

    let sym_start = symbol_start(text, cursor);
    let sym_end = symbol_end(text, cursor);

    if sym_start >= sym_end {
        return None;
    }

    if scope.is_inner() {
        return Some(TextObjectRange::char(sym_start, sym_end));
    }

    // Around: prefer trailing whitespace, fall back to leading whitespace.
    let mut end = sym_end;
    let trailing_end = skip_whitespace_forward(text, end);

    if trailing_end > end {
        // Found trailing whitespace — absorb it.
        end = trailing_end;
        return Some(TextObjectRange::char(sym_start, end));
    }

    // No trailing whitespace — try leading whitespace.
    let leading_start = skip_whitespace_backward(text, sym_start);
    Some(TextObjectRange::char(leading_start, sym_end))
}

/// Advance `pos` forward over ASCII horizontal whitespace (space, tab).
///
/// Stops at newlines so we never cross a line boundary.
fn skip_whitespace_forward(text: &str, mut pos: usize) -> usize {
    while pos < text.len() {
        if let Some(c) = text[pos..].chars().next() {
            if c == ' ' || c == '\t' {
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

/// Retreat `pos` backward over ASCII horizontal whitespace (space, tab).
///
/// Stops at newlines so we never cross a line boundary.
fn skip_whitespace_backward(text: &str, mut pos: usize) -> usize {
    loop {
        if pos == 0 {
            break;
        }
        let prev = prev_char_boundary(text, pos);
        if let Some(c) = text[prev..].chars().next() {
            if c == ' ' || c == '\t' {
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
        let result = compute_symbol_object(&ctx, scope);
        match (result, expected) {
            (Some(r), Some((slice, s, e))) => {
                assert_eq!(r.start(), s, "start mismatch for {:?} at {}", text, cursor);
                assert_eq!(r.end(), e, "end mismatch for {:?} at {}", text, cursor);
                assert_eq!(&text[r.start()..r.end()], slice, "slice mismatch");
            }
            (None, None) => {}
            (got, exp) => panic!("got {:?}, expected {:?}", got, exp),
        }
    }

    // ── is_symbol_char ────────────────────────────────────────────────────────

    #[test]
    fn symbol_char_alphanumeric() {
        assert!(is_symbol_char('a'));
        assert!(is_symbol_char('Z'));
        assert!(is_symbol_char('0'));
        assert!(is_symbol_char('9'));
    }

    #[test]
    fn symbol_char_special() {
        for c in ['_', '.', ':', '$', '@', '#', '-'] {
            assert!(is_symbol_char(c), "{c:?} should be a symbol char");
        }
    }

    #[test]
    fn symbol_char_non_symbol() {
        for c in [
            ' ', '\t', '\n', '(', ')', '[', ']', '{', '}', '/', '\\', '"', '\'',
        ] {
            assert!(!is_symbol_char(c), "{c:?} should NOT be a symbol char");
        }
    }

    // ── inner (im) ────────────────────────────────────────────────────────────

    #[test]
    fn im_simple_word() {
        check("hello", 0, true, Some(("hello", 0, 5)));
        check("hello", 2, true, Some(("hello", 0, 5)));
        check("hello", 4, true, Some(("hello", 0, 5)));
    }

    #[test]
    fn im_dotted_path() {
        // foo.bar.baz — cursor anywhere inside should select whole symbol
        check("foo.bar.baz", 0, true, Some(("foo.bar.baz", 0, 11)));
        check("foo.bar.baz", 4, true, Some(("foo.bar.baz", 0, 11)));
        check("foo.bar.baz", 10, true, Some(("foo.bar.baz", 0, 11)));
    }

    #[test]
    fn im_double_colon_path() {
        let text = "std::collections::HashMap";
        check(text, 5, true, Some((text, 0, text.len())));
    }

    #[test]
    fn im_symbol_in_sentence() {
        // "call foo.bar()" — cursor on 'o' in foo.bar
        let text = "call foo.bar()";
        check(text, 7, true, Some(("foo.bar", 5, 12)));
    }

    #[test]
    fn im_cursor_on_non_symbol_char() {
        // cursor on '(' — not a symbol char, returns None
        check("foo(bar)", 3, true, None);
    }

    #[test]
    fn im_empty_text() {
        check("", 0, true, None);
    }

    #[test]
    fn im_single_char() {
        check("x", 0, true, Some(("x", 0, 1)));
    }

    #[test]
    fn im_cursor_past_end() {
        // Cursor clamped to last char
        check("hello", 100, true, Some(("hello", 0, 5)));
    }

    // ── around (am) ───────────────────────────────────────────────────────────

    #[test]
    fn am_trailing_space() {
        // "foo.bar baz" — cursor on symbol, around absorbs trailing space
        let text = "foo.bar baz";
        check(text, 2, false, Some(("foo.bar ", 0, 8)));
    }

    #[test]
    fn am_trailing_tab() {
        let text = "foo.bar\tbaz";
        check(text, 2, false, Some(("foo.bar\t", 0, 8)));
    }

    #[test]
    fn am_no_trailing_whitespace_uses_leading() {
        // "baz foo.bar" — cursor on foo.bar (no trailing ws), absorbs leading space
        let text = "baz foo.bar";
        check(text, 5, false, Some((" foo.bar", 3, 11)));
    }

    #[test]
    fn am_no_surrounding_whitespace() {
        // Only a symbol, nothing around it
        check("foo.bar", 2, false, Some(("foo.bar", 0, 7)));
    }

    #[test]
    fn am_cursor_on_non_symbol_char() {
        check("foo bar", 3, false, None);
    }

    // ── edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn im_does_not_cross_newline() {
        // "foo\nbar" — cursor on 'f', inner should only select "foo"
        let text = "foo\nbar";
        check(text, 0, true, Some(("foo", 0, 3)));
        // cursor on 'b'
        check(text, 4, true, Some(("bar", 4, 7)));
    }

    #[test]
    fn am_does_not_absorb_newline_as_whitespace() {
        // "foo\nbar" — around foo, no trailing space (newline is not absorbed)
        let text = "foo\nbar";
        // No trailing *horizontal* whitespace, no leading whitespace either
        check(text, 0, false, Some(("foo", 0, 3)));
    }

    #[test]
    fn im_underscore_identifier() {
        let text = "my_var_name";
        check(text, 3, true, Some(("my_var_name", 0, 11)));
    }

    #[test]
    fn im_sigil_dollar() {
        let text = "$HOME";
        check(text, 0, true, Some(("$HOME", 0, 5)));
    }
}
