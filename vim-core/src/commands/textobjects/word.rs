//! Word text objects (iw, aw, iW, aW).
//!
//! Per Neovim:
//! - `word` = keyword characters (alphanumeric + underscore)
//! - `WORD` = non-whitespace characters
//!
//! Inner excludes surrounding whitespace.
//! Around includes trailing whitespace (or leading if at end).

use super::helpers::{class_at, next_char_boundary, prev_char_boundary, CharClass};
use super::types::{TextObjectContext, TextObjectRange};
use crate::grammar::types::TextObjectScope;
use crate::primitives::{WordCharSet, WordKind};

/// Compute a word text object.
///
/// # Arguments
///
/// * `ctx` - Text object context with text and cursor
/// * `inner` - If true, compute inner word (iw/iW), else around word (aw/aW)
/// * `kind` - `WordKind::Word` for iw/aw, `WordKind::WORD` for iW/aW
///
/// # Returns
///
/// `Some(TextObjectRange)` if a word was found, `None` if the text is empty.
///
/// # Edge Cases
///
/// - Cursor on whitespace: selects the whitespace region
/// - Cursor on punctuation: selects punctuation as its own "word"
/// - End of line: handles EOL properly
#[must_use]
pub fn compute_word_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
    kind: WordKind,
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

    // Get the class of the character under cursor
    let cursor_char = text[cursor..].chars().next()?;
    let cursor_class = class_at(text, cursor, kind, ctx.word_chars)?;

    // Special case: cursor on a newline character (empty line).
    // In Vim, each newline is its own boundary — iw/aw on an empty line
    // should not extend across line boundaries into adjacent whitespace.
    let word_chars = ctx.word_chars;
    if cursor_char == '\n' {
        // An empty line is when the newline is at position 0 or follows another newline.
        let is_empty_line =
            cursor == 0 || text.as_bytes().get(cursor.wrapping_sub(1)) == Some(&b'\n');
        if scope.is_inner() {
            // iw on a newline: in Vim, this is a no-op on empty lines.
            if is_empty_line {
                return None;
            }
            // Otherwise cursor is at end of a line with content — select just the newline
            return Some(TextObjectRange::char(cursor, cursor + 1));
        } else {
            // aw on empty line: select the newline plus the next line's content.
            // In Neovim, aw on an empty line between content selects the empty
            // line and the adjacent content line as linewise.
            let newline_end = cursor + 1;
            if let Some(next_class) = class_at(text, newline_end, kind, word_chars) {
                if !next_class.is_whitespace() {
                    let word_end = find_word_end(text, newline_end, next_class, kind, word_chars);
                    // Check if this results in full-line coverage (linewise)
                    if is_empty_line {
                        return Some(TextObjectRange::line(cursor, word_end));
                    }
                    return Some(TextObjectRange::char(cursor, word_end));
                }
            }
            // No following word found — select just the newline.
            // On an empty line (at buffer start or after another newline),
            // Neovim treats this as linewise.
            if is_empty_line {
                return Some(TextObjectRange::line(cursor, newline_end));
            }
            return Some(TextObjectRange::char(cursor, newline_end));
        }
    }

    // Find the start of the current word/whitespace region
    let word_start = find_word_start(text, cursor, cursor_class, kind, word_chars);

    // Find the end of the current word/whitespace region
    let word_end = find_word_end(text, cursor, cursor_class, kind, word_chars);

    if scope.is_inner() {
        // Inner word: just the word/whitespace region itself
        if word_start < word_end {
            Some(TextObjectRange::char(word_start, word_end))
        } else {
            None
        }
    } else {
        // Around word: include trailing whitespace (or leading if at end)
        compute_around_word(text, word_start, word_end, cursor_class, kind, word_chars)
    }
}

/// Find the start of the word/region containing cursor.
fn find_word_start(
    text: &str,
    cursor: usize,
    cursor_class: CharClass,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    let mut pos = cursor;

    while pos > 0 {
        let prev = prev_char_boundary(text, pos);
        if let Some(prev_class) = class_at(text, prev, kind, word_chars) {
            if prev_class != cursor_class {
                break;
            }
            pos = prev;
        } else {
            break;
        }
    }

    pos
}

/// Find the end of the word/region (exclusive) containing cursor.
fn find_word_end(
    text: &str,
    cursor: usize,
    cursor_class: CharClass,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    let mut pos = cursor;

    while pos < text.len() {
        if let Some(current_class) = class_at(text, pos, kind, word_chars) {
            if current_class != cursor_class {
                break;
            }
            pos = next_char_boundary(text, pos);
        } else {
            break;
        }
    }

    pos
}

/// Compute "around" word - includes trailing or leading whitespace.
fn compute_around_word(
    text: &str,
    word_start: usize,
    word_end: usize,
    cursor_class: CharClass,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> Option<TextObjectRange> {
    let mut start = word_start;
    let mut end = word_end;

    if cursor_class.is_whitespace() {
        // Cursor is on whitespace: include the following word
        if let Some(next_class) = class_at(text, end, kind, word_chars) {
            if !next_class.is_whitespace() {
                end = find_word_end(text, end, next_class, kind, word_chars);
            }
        }
    } else {
        // Cursor is on a word: try to include trailing whitespace first
        let mut trailing_end = end;
        while trailing_end < text.len() {
            if let Some(class) = class_at(text, trailing_end, kind, word_chars) {
                if class.is_whitespace() {
                    trailing_end = next_char_boundary(text, trailing_end);
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if trailing_end > end {
            // Found trailing whitespace
            end = trailing_end;
        } else {
            // No trailing whitespace, try leading whitespace
            let mut leading_start = start;
            while leading_start > 0 {
                let prev = prev_char_boundary(text, leading_start);
                if let Some(class) = class_at(text, prev, kind, word_chars) {
                    if class.is_whitespace() {
                        leading_start = prev;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            start = leading_start;
        }
    }

    if start < end {
        Some(TextObjectRange::char(start, end))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to run tests
    fn check(
        text: &str,
        cursor: usize,
        inner: bool,
        kind: WordKind,
        expected: Option<(usize, usize)>,
    ) {
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_word_object(&ctx, TextObjectScope::from_inner_flag(inner), kind);
        match (result, expected) {
            (Some(r), Some((s, e))) => {
                assert_eq!(r.start(), s, "start mismatch for '{}' at {}", text, cursor);
                assert_eq!(r.end(), e, "end mismatch for '{}' at {}", text, cursor);
            }
            (None, None) => {}
            _ => panic!("result {:?} != expected {:?}", result, expected),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // INNER WORD (iw)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_iw_single_word() {
        check("hello", 0, true, WordKind::Word, Some((0, 5)));
        check("hello", 2, true, WordKind::Word, Some((0, 5)));
        check("hello", 4, true, WordKind::Word, Some((0, 5)));
    }

    #[test]
    fn test_iw_two_words() {
        check("hello world", 0, true, WordKind::Word, Some((0, 5)));
        check("hello world", 6, true, WordKind::Word, Some((6, 11)));
    }

    #[test]
    fn test_iw_cursor_on_space() {
        check("hello world", 5, true, WordKind::Word, Some((5, 6)));
    }

    #[test]
    fn test_iw_with_punctuation() {
        check("hello, world", 5, true, WordKind::Word, Some((5, 6))); // comma
        check("hello, world", 7, true, WordKind::Word, Some((7, 12))); // world
    }

    #[test]
    fn test_iw_all_punctuation() {
        check("...", 1, true, WordKind::Word, Some((0, 3)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // AROUND WORD (aw)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_aw_includes_trailing_space() {
        check("hello world", 0, false, WordKind::Word, Some((0, 6))); // "hello "
    }

    #[test]
    fn test_aw_last_word_includes_leading_space() {
        check("hello world", 6, false, WordKind::Word, Some((5, 11))); // " world"
    }

    #[test]
    fn test_aw_cursor_on_space() {
        check("hello world", 5, false, WordKind::Word, Some((5, 11))); // " world"
    }

    // ─────────────────────────────────────────────────────────────────────────
    // INNER WORD (iW) - BIG WORD
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_iW_treats_punctuation_as_word() {
        check("hello,world foo", 0, true, WordKind::WORD, Some((0, 11))); // "hello,world"
        check("hello,world foo", 5, true, WordKind::WORD, Some((0, 11))); // comma is part of word
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EDGE CASES
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_empty_text() {
        check("", 0, true, WordKind::Word, None);
    }

    #[test]
    fn test_single_char() {
        check("a", 0, true, WordKind::Word, Some((0, 1)));
    }

    #[test]
    fn test_whitespace_only() {
        check("   ", 1, true, WordKind::Word, Some((0, 3)));
    }

    #[test]
    fn test_cursor_past_end() {
        check("hello", 100, true, WordKind::Word, Some((0, 5))); // Clamps to last char
    }
}
