//! Word boundary utilities for search and command-line.
//!
//! Extracted from `search.rs` — contains word-under-cursor extraction,
//! word character classification, and pattern boundary stripping.

use crate::primitives::WordCharSet;

/// Get the word under cursor using the given word character set.
#[must_use]
pub fn word_under_cursor<'a>(text: &'a str, cursor: usize, word_chars: &WordCharSet) -> &'a str {
    if cursor >= text.len() || !text.is_char_boundary(cursor) {
        return "";
    }

    // Check if cursor is on a word char
    let cursor_char = match text[cursor..].chars().next() {
        Some(c) if word_chars.contains(c) => c,
        _ => return "",
    };

    // Find start of word (walk backward by chars)
    let mut start = cursor;
    for (idx, c) in text[..cursor].char_indices().rev() {
        if !word_chars.contains(c) {
            start = idx + c.len_utf8();
            break;
        }
        start = idx;
    }

    // Find end of word (walk forward by chars)
    let after_cursor = cursor + cursor_char.len_utf8();
    let mut end = after_cursor;
    for (idx, c) in text[after_cursor..].char_indices() {
        if !word_chars.contains(c) {
            end = after_cursor + idx;
            break;
        }
        end = after_cursor + idx + c.len_utf8();
    }

    &text[start..end]
}

/// Get the word under or after the cursor (for `*`, `#`, `g*`, `g#`).
///
/// If the cursor is on a non-word character, advances to the next word on the
/// same line. Returns `("", cursor)` if no word is found.
#[must_use]
pub fn word_under_cursor_or_next<'a>(
    text: &'a str,
    cursor: usize,
    word_chars: &WordCharSet,
) -> (&'a str, usize) {
    if cursor >= text.len() || !text.is_char_boundary(cursor) {
        return ("", cursor);
    }

    // If cursor is on a word char, use it directly
    if let Some(c) = text[cursor..].chars().next() {
        if word_chars.contains(c) {
            return (word_under_cursor(text, cursor, word_chars), cursor);
        }
    }

    // Advance past non-word, non-newline chars to find next word on line
    for (idx, ch) in text[cursor..].char_indices() {
        if ch == '\n' {
            return ("", cursor);
        }
        if word_chars.contains(ch) {
            let effective = cursor + idx;
            return (word_under_cursor(text, effective, word_chars), effective);
        }
    }

    ("", cursor)
}

/// Strip Vim word boundary markers `\<` and `\>` from a pattern.
/// Returns (actual_pattern, has_word_boundaries).
pub(super) fn strip_word_boundaries(pattern: &str) -> (&str, bool) {
    if pattern.starts_with("\\<") && pattern.ends_with("\\>") && pattern.len() > 4 {
        (&pattern[2..pattern.len() - 2], true)
    } else {
        (pattern, false)
    }
}
