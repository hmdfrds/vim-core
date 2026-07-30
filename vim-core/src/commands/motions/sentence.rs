//! Sentence motions: (, )
//!
//! Navigate between sentences.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::prev_char_boundary;
use crate::primitives::Offset;

// ─────────────────────────────────────────────────────────────────────────────
// Sentence Motions
// ─────────────────────────────────────────────────────────────────────────────

/// `(` - Previous sentence.
///
/// In Vim, `(` moves to the beginning of the current or previous sentence.
/// A sentence ends at '.', '!', or '?' followed by whitespace, EOL, or closing chars.
/// A paragraph boundary (blank line) is also a sentence boundary.
pub fn open_paren(ctx: &MotionContext<'_>) -> MotionResult {
    let text = ctx.text;
    let cursor = ctx.cursor.get();
    if cursor == 0 || text.is_empty() {
        return MotionResult::Position(Offset::new(0));
    }

    let bytes = text.as_bytes();
    let mut remaining = ctx.count;
    let mut pos = cursor;

    while remaining > 0 && pos > 0 {
        // Move back at least one character
        pos = prev_char_boundary(text, pos);

        // Skip trailing whitespace (we might be on whitespace between sentences)
        while pos > 0 && text[pos..].chars().next().is_some_and(char::is_whitespace) {
            pos = prev_char_boundary(text, pos);
        }

        // Now scan backward to find the start of this sentence
        // We're somewhere in a sentence — find the start
        loop {
            if pos == 0 {
                // Reached beginning of text - this is a sentence start
                remaining -= 1;
                break;
            }

            // Check for blank line boundary: two consecutive newlines
            if bytes.get(pos) == Some(&b'\n') && pos > 0 && bytes.get(pos - 1) == Some(&b'\n') {
                // The sentence starts at pos + 1
                pos += 1;
                remaining -= 1;
                break;
            }

            // Check the character before our position
            let prev_pos = prev_char_boundary(text, pos);
            let prev_c = text[prev_pos..].chars().next().unwrap_or(' ');

            // Check if prev char is sentence-ending punct followed by our position being whitespace/follower
            if is_sentence_end(prev_c) {
                // Check if the sentence end is valid (followed by whitespace)
                let mut check = prev_pos + prev_c.len_utf8();
                // Skip optional closing chars like ) ] " '
                while check < text.len() {
                    let c = text[check..].chars().next().unwrap_or(' ');
                    if is_sentence_end_follower(c) {
                        check += c.len_utf8();
                    } else {
                        break;
                    }
                }
                // Must be followed by whitespace (cpo_j: require 2+ spaces)
                if check < text.len() {
                    let required_spaces: usize = if ctx.options.cpo_j() { 2 } else { 1 };
                    let ws_start = check;
                    while check < text.len() {
                        let c = text[check..].chars().next().unwrap_or(' ');
                        if c.is_whitespace() {
                            check += c.len_utf8();
                        } else {
                            break;
                        }
                    }
                    let ws_count = check - ws_start;
                    if ws_count >= required_spaces {
                        pos = check;
                        remaining -= 1;
                        break;
                    }
                } else {
                    // Sentence ends at end of text
                    pos = check;
                    remaining -= 1;
                    break;
                }
            }

            pos = prev_pos;
        }
    }

    MotionResult::Position(Offset::new(pos))
}

/// `)` - Next sentence.
pub fn close_paren(ctx: &MotionContext<'_>) -> MotionResult {
    let text = ctx.text;
    let pos = ctx.cursor.get();
    let mut sentences_found = 0;

    // Zero-allocation: use Peekable iterator instead of collecting into Vec.
    // The old code had a bug: char_indices yields (byte_offset, char), but
    // `chars.get(i + 1)` treated the byte offset as an array index.
    let mut chars = text[pos..].char_indices().peekable();

    while let Some((byte_offset, c)) = chars.next() {
        // Blank lines are sentence boundaries (see :help sentence)
        if c == '\n' {
            let abs_pos = pos + byte_offset;
            if text.as_bytes().get(abs_pos + 1) == Some(&b'\n') {
                sentences_found += 1;
                if sentences_found >= ctx.count {
                    // Skip past blank lines to start of next sentence
                    let mut next_pos = abs_pos + 1;
                    while text.as_bytes().get(next_pos) == Some(&b'\n') {
                        next_pos += 1;
                    }
                    return MotionResult::Position(Offset::new(next_pos));
                }
                continue;
            }
        }
        if is_sentence_end(c) {
            if let Some(&(_, next_c)) = chars.peek() {
                if next_c.is_whitespace() || is_sentence_end_follower(next_c) {
                    // Skip past punctuation + closing chars to count whitespace
                    let mut next_pos = pos + byte_offset + c.len_utf8();
                    while next_pos < text.len() {
                        let ch = text[next_pos..].chars().next().unwrap_or(' ');
                        if is_sentence_end_follower(ch) {
                            next_pos += ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    let ws_start = next_pos;
                    while next_pos < text.len() {
                        let ch = text[next_pos..].chars().next().unwrap_or(' ');
                        if ch.is_whitespace() {
                            next_pos += ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    let required_spaces: usize = if ctx.options.cpo_j() { 2 } else { 1 };
                    if next_pos - ws_start >= required_spaces {
                        sentences_found += 1;
                        if sentences_found >= ctx.count {
                            return MotionResult::Position(Offset::new(next_pos));
                        }
                    }
                }
            } else {
                sentences_found += 1;
                if sentences_found >= ctx.count {
                    let next_pos = pos + byte_offset + c.len_utf8();
                    return MotionResult::Position(Offset::new(
                        next_pos.min(text.len().saturating_sub(1)),
                    ));
                }
            }
        }
    }

    // End of document — clamp to last valid cursor position
    let end = if text.ends_with('\n') {
        text.len().saturating_sub(1)
    } else {
        text.char_indices().next_back().map_or(0, |(i, _)| i)
    };
    MotionResult::Position(Offset::new(end))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

const fn is_sentence_end(c: char) -> bool {
    matches!(c, '.' | '!' | '?')
}

const fn is_sentence_end_follower(c: char) -> bool {
    matches!(c, ')' | ']' | '"' | '\'')
}
