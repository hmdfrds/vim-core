//! Sentence text objects (is, as).
//!
//! Sentence = text ending with `.`, `!`, `?` followed by whitespace.

use super::types::{TextObjectContext, TextObjectRange};
use crate::commands::helpers::prev_char_boundary;
use crate::grammar::types::TextObjectScope;

/// Compute a sentence text object.
///
/// # Arguments
///
/// * `ctx` - Text object context with text and cursor
/// * `inner` - If true, exclude trailing whitespace; if false, include it
///
/// # Returns
///
/// `Some(TextObjectRange)` if a sentence boundary found, `None` otherwise.
#[must_use]
pub fn compute_sentence_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
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

    // Find sentence start (search backward for sentence-ending punctuation + whitespace)
    let sent_start = find_sentence_start(text, cursor);

    // Find sentence end (search forward for sentence-ending punctuation)
    let (sent_end, trailing_ws_end) = find_sentence_end(text, cursor);

    let (start, end) = if scope.is_inner() {
        // Vim's `is` selects the sentence without trailing whitespace.
        // However, when the sentence is followed by a newline (multi-line),
        // the newline IS included as part of the sentence boundary.
        let mut is_end = sent_end;
        // Include trailing newlines (but not spaces/tabs)
        while is_end < text.len() {
            match text.as_bytes().get(is_end) {
                Some(&b'\n') => is_end += 1,
                _ => break,
            }
        }
        // When the cursor is on whitespace before the sentence text,
        // include that leading whitespace. In Vim, `is` from inter-sentence
        // whitespace selects the whitespace + the following sentence.
        let is_start = if cursor < sent_start {
            // Walk backward from cursor to find start of whitespace run
            let mut ws_start = cursor;
            while ws_start > 0 {
                let prev = prev_char_boundary(text, ws_start);
                match text[prev..].chars().next() {
                    Some(c) if c.is_whitespace() => ws_start = prev,
                    _ => break,
                }
            }
            ws_start
        } else {
            sent_start
        };
        (is_start, is_end)
    } else {
        // Vim's `as` includes trailing whitespace. If there is NO trailing
        // whitespace (last sentence / end of text), include leading whitespace
        // instead — just like `aw` on the last word.
        if trailing_ws_end == sent_end && sent_start > 0 {
            // No trailing whitespace; extend backward to include leading whitespace
            let mut lead = sent_start;
            while lead > 0 {
                let prev = prev_char_boundary(text, lead);
                match text[prev..].chars().next() {
                    Some(c) if c == ' ' || c == '\t' => lead = prev,
                    _ => break,
                }
            }
            (lead, sent_end)
        } else {
            (sent_start, trailing_ws_end)
        }
    };

    // Sentence text objects promote to linewise when the range covers complete
    // lines (column 0 to newline). Verified: das_multiline/dis_multiline golden
    // files show regtype "V". Cross-line ranges NOT ending at \n stay charwise.
    let at_line_start = start == 0 || text.as_bytes().get(start.wrapping_sub(1)) == Some(&b'\n');
    let at_line_end = end > start && text.as_bytes().get(end.wrapping_sub(1)) == Some(&b'\n');
    if at_line_start && at_line_end {
        Some(TextObjectRange::line(start, end))
    } else {
        Some(TextObjectRange::char(start, end))
    }
}

/// Find the start of the current sentence.
fn find_sentence_start(text: &str, cursor: usize) -> usize {
    let mut pos = cursor;

    // Skip back past any whitespace (char-boundary safe)
    while pos > 0 {
        let prev = prev_char_boundary(text, pos);
        match text[prev..].chars().next() {
            Some(c) if c.is_whitespace() => pos = prev,
            _ => break,
        }
    }

    // Search backward for sentence-ending punctuation followed by whitespace
    while pos > 0 {
        let prev = prev_char_boundary(text, pos);
        match text[prev..].chars().next() {
            Some(c) if is_sentence_end_char(c) => break,
            Some(_) => pos = prev,
            None => break,
        }
    }

    // Skip forward past any leading whitespace (advance by char width)
    while pos < text.len() {
        match text[pos..].chars().next() {
            Some(c) if c.is_whitespace() => pos += c.len_utf8(),
            _ => break,
        }
    }

    pos
}

/// Find the end of the current sentence.
/// Returns (`sentence_end`, `trailing_whitespace_end`).
fn find_sentence_end(text: &str, cursor: usize) -> (usize, usize) {
    let mut pos = cursor;

    // Search forward for sentence-ending punctuation
    while pos < text.len() {
        let c = text[pos..].chars().next();
        if let Some(ch) = c {
            if is_sentence_end_char(ch) {
                let sent_end = pos + ch.len_utf8();

                // Skip trailing closing brackets/quotes
                let mut trailing_end = sent_end;
                while trailing_end < text.len() {
                    let next = text[trailing_end..].chars().next();
                    if let Some(nc) = next {
                        if nc == ')' || nc == ']' || nc == '"' || nc == '\'' {
                            trailing_end += nc.len_utf8();
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                // Skip trailing whitespace
                let mut ws_end = trailing_end;
                while ws_end < text.len() {
                    let next = text[ws_end..].chars().next();
                    if let Some(nc) = next {
                        if nc.is_whitespace() {
                            ws_end += nc.len_utf8();
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                return (trailing_end, ws_end);
            }
            pos += ch.len_utf8();
        } else {
            break;
        }
    }

    // No sentence end found, return end of text
    (text.len(), text.len())
}

const fn is_sentence_end_char(c: char) -> bool {
    c == '.' || c == '!' || c == '?'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_sentence() {
        let text = "Hello world.";
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_sentence_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 12);
    }

    #[test]
    fn test_two_sentences_inner() {
        let text = "First. Second.";
        // Cursor in "First"
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_sentence_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 6); // "First."
    }

    #[test]
    fn test_around_includes_trailing_space() {
        let text = "First. Second.";
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_sentence_object(&ctx, TextObjectScope::Around).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 7); // "First. "
    }

    #[test]
    fn test_exclamation() {
        let text = "Hello!";
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_sentence_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.end(), 6);
    }

    #[test]
    fn test_question() {
        let text = "Really?";
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_sentence_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.end(), 7);
    }
}
