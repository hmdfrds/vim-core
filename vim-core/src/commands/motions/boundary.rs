//! Composable boundary scanning primitives for word motions.
//!
//! The four word-motion algorithms (w, b, e, ge) all share a common
//! structure: advance one character, then perform two phases of scanning
//! (skip-class and skip-whitespace) in a direction-dependent order.
//!
//! This module extracts those phases into reusable building blocks:
//!
//! - [`skip_class_forward`] / [`skip_class_backward`] — skip chars of a given class
//! - [`skip_whitespace_forward`] / [`skip_whitespace_backward`] — skip whitespace, respecting empty-line stops
//! - [`step_forward`] / [`step_backward`] — advance/retreat one character
//!
//! Each word motion composes these primitives in the appropriate order.

use crate::commands::helpers::{char_at, next_char_boundary, prev_char_boundary, CharClass};
use crate::primitives::{WordCharSet, WordKind};

/// Advance one character forward. Returns the new position.
#[inline]
#[must_use]
pub const fn step_forward(text: &str, pos: usize) -> usize {
    next_char_boundary(text, pos)
}

/// Retreat one character backward. Returns the new position.
#[inline]
#[must_use]
pub const fn step_backward(text: &str, pos: usize) -> usize {
    prev_char_boundary(text, pos)
}

/// Skip forward past characters of the given class.
///
/// Stops when a character of a different class is encountered or end-of-text.
/// Returns the position of the first character that differs from `class`.
#[must_use]
pub fn skip_class_forward(
    text: &str,
    mut pos: usize,
    class: CharClass,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    while pos < text.len() {
        if let Some(c) = char_at(text, pos) {
            if CharClass::classify(c, kind, word_chars) != class {
                break;
            }
            pos = next_char_boundary(text, pos);
        } else {
            break;
        }
    }
    pos
}

/// Skip backward past characters of the given class.
///
/// Stops when a character of a different class is encountered or start-of-text.
/// Returns the position of the first character (scanning backward) that still
/// belongs to `class` — i.e., the start of the class run.
#[must_use]
pub fn skip_class_backward(
    text: &str,
    mut pos: usize,
    class: CharClass,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    while pos > 0 {
        let prev = prev_char_boundary(text, pos);
        if let Some(prev_c) = char_at(text, prev) {
            if CharClass::classify(prev_c, kind, word_chars) != class {
                break;
            }
            pos = prev;
        } else {
            break;
        }
    }
    pos
}

/// Skip backward while the character AT `pos` matches `class`.
///
/// Unlike [`skip_class_backward`] which peeks at the character before `pos`,
/// this checks the character at the current position and moves backward.
/// Used by `ge`/`gE` which need to skip past same-class characters they're
/// standing on.
///
/// Returns the position of the first character (scanning backward) that
/// differs from `class`, or 0 if the entire prefix matches.
#[must_use]
pub fn skip_while_class_backward(
    text: &str,
    mut pos: usize,
    class: CharClass,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    while pos > 0 {
        if let Some(c) = char_at(text, pos) {
            if CharClass::classify(c, kind, word_chars) != class {
                break;
            }
            pos = prev_char_boundary(text, pos);
        } else {
            break;
        }
    }
    pos
}

/// Skip forward past whitespace, stopping at empty lines (Vim behavior).
///
/// An empty line is a `\n` that starts a blank line — detected by:
/// - The next char is `\n` (forward empty line detection), or
/// - The previous char is `\n` or we're at text start (this IS an empty line)
///
/// The `start` parameter is the original motion start position, used to avoid
/// infinite loops when the cursor is already on an empty line.
#[must_use]
pub fn skip_whitespace_forward(
    text: &str,
    mut pos: usize,
    start: usize,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    while pos < text.len() {
        if let Some(c) = char_at(text, pos) {
            if !CharClass::classify(c, kind, word_chars).is_whitespace() {
                break;
            }
            if c == '\n' {
                let next = next_char_boundary(text, pos);
                let next_is_nl = next < text.len() && char_at(text, next) == Some('\n');
                let prev_is_nl = pos == 0
                    || (pos > 0 && char_at(text, prev_char_boundary(text, pos)) == Some('\n'));

                if next_is_nl {
                    // Next line is empty — stop at its start
                    pos = next;
                    break;
                }
                if prev_is_nl {
                    // Current position is an empty line — stop here
                    // but only if we've actually moved (avoid infinite loop at start)
                    if pos > start {
                        break;
                    }
                }
            }
            pos = next_char_boundary(text, pos);
        } else {
            break;
        }
    }
    pos.min(text.len())
}

/// Skip backward past whitespace, stopping at empty lines (Vim behavior).
///
/// An empty line (`\n` preceded by `\n` or at text start) acts as a word
/// boundary — Vim treats it as its own "word".
#[must_use]
pub fn skip_whitespace_backward(
    text: &str,
    mut pos: usize,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    while pos > 0 {
        if let Some(c) = char_at(text, pos) {
            if !CharClass::classify(c, kind, word_chars).is_whitespace() {
                break;
            }
            if c == '\n' {
                let prev = prev_char_boundary(text, pos);
                if prev == 0 || char_at(text, prev) == Some('\n') {
                    return pos;
                }
            }
            pos = prev_char_boundary(text, pos);
        } else {
            break;
        }
    }
    pos
}

/// Skip backward past whitespace for `ge`/`gE`, stopping at empty lines.
///
/// The `ge` motion has subtly different empty-line detection from `b`:
/// when at the very start of text and the first char is `\n`, it returns
/// that position. The `b` motion handles this via `prev == 0 || ...`
/// which covers that case, but `ge` checks `prev == 0 && char_at(0) == '\n'`
/// separately from `char_at(prev) == '\n'`.
#[must_use]
pub fn skip_whitespace_backward_ge(
    text: &str,
    mut pos: usize,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    while pos > 0 {
        if let Some(c) = char_at(text, pos) {
            if !CharClass::classify(c, kind, word_chars).is_whitespace() {
                break;
            }
            if c == '\n' {
                let prev = prev_char_boundary(text, pos);
                if prev == 0 && char_at(text, 0) == Some('\n') {
                    return pos;
                }
                if char_at(text, prev) == Some('\n') {
                    return pos;
                }
            }
            pos = prev_char_boundary(text, pos);
        } else {
            break;
        }
    }
    pos
}

/// Find the end of the current word class run, scanning forward.
///
/// Like [`skip_class_forward`] but returns the position of the **last**
/// character in the run rather than the first character after it.
/// Used by `e`/`E` which land on the last character of a word.
#[must_use]
pub fn find_class_end_forward(
    text: &str,
    mut pos: usize,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    let word_class = char_at(text, pos).map(|c| CharClass::classify(c, kind, word_chars));
    let mut last_pos = pos;

    while pos < text.len() {
        if let Some(c) = char_at(text, pos) {
            if Some(CharClass::classify(c, kind, word_chars)) != word_class {
                break;
            }
            last_pos = pos;
            pos = next_char_boundary(text, pos);
        } else {
            break;
        }
    }

    last_pos
}

/// Skip forward past simple whitespace (no empty-line logic).
///
/// Used by `e`/`E` which don't stop at empty lines during their
/// whitespace-skip phase.
#[must_use]
pub fn skip_whitespace_forward_simple(
    text: &str,
    mut pos: usize,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> usize {
    while pos < text.len() {
        if let Some(c) = char_at(text, pos) {
            if !CharClass::classify(c, kind, word_chars).is_whitespace() {
                break;
            }
            pos = next_char_boundary(text, pos);
        } else {
            break;
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_wc() -> WordCharSet {
        WordCharSet::default_vim()
    }

    // ─── step_forward / step_backward ────────────────────────────────────

    #[test]
    fn step_forward_ascii() {
        assert_eq!(step_forward("hello", 0), 1);
        assert_eq!(step_forward("hello", 4), 5);
    }

    #[test]
    fn step_forward_multibyte() {
        // 'ü' is 2 bytes in UTF-8
        assert_eq!(step_forward("über", 0), 2);
    }

    #[test]
    fn step_forward_at_end() {
        assert_eq!(step_forward("hi", 2), 2);
    }

    #[test]
    fn step_backward_ascii() {
        assert_eq!(step_backward("hello", 5), 4);
        assert_eq!(step_backward("hello", 1), 0);
    }

    #[test]
    fn step_backward_at_start() {
        assert_eq!(step_backward("hello", 0), 0);
    }

    // ─── skip_class_forward ──────────────────────────────────────────────

    #[test]
    fn skip_class_forward_words() {
        let wc = default_wc();
        // "hello world" — skip word chars from 0
        let pos = skip_class_forward("hello world", 0, CharClass::Word, WordKind::Word, &wc);
        assert_eq!(pos, 5); // stops at space
    }

    #[test]
    fn skip_class_forward_at_boundary() {
        let wc = default_wc();
        // Already at a different class
        let pos = skip_class_forward("hello world", 5, CharClass::Word, WordKind::Word, &wc);
        assert_eq!(pos, 5); // space is not Word, stops immediately
    }

    #[test]
    fn skip_class_forward_to_end() {
        let wc = default_wc();
        let pos = skip_class_forward("hello", 0, CharClass::Word, WordKind::Word, &wc);
        assert_eq!(pos, 5); // runs to end
    }

    // ─── skip_class_backward ─────────────────────────────────────────────

    #[test]
    fn skip_class_backward_words() {
        let wc = default_wc();
        // "hello world" — from pos 10 ('d'), skip backward through "world"
        let pos = skip_class_backward("hello world", 10, CharClass::Word, WordKind::Word, &wc);
        assert_eq!(pos, 6); // stops at start of "world"
    }

    #[test]
    fn skip_class_backward_to_start() {
        let wc = default_wc();
        let pos = skip_class_backward("hello", 4, CharClass::Word, WordKind::Word, &wc);
        assert_eq!(pos, 0);
    }

    // ─── skip_whitespace_forward ─────────────────────────────────────────

    #[test]
    fn skip_ws_forward_basic() {
        let wc = default_wc();
        let pos = skip_whitespace_forward("  hello", 0, 0, WordKind::Word, &wc);
        assert_eq!(pos, 2);
    }

    #[test]
    fn skip_ws_forward_empty_line_stop() {
        let wc = default_wc();
        // "hello\n\nworld" — whitespace skip from 5 (\n) should stop at 6 (empty line)
        let pos = skip_whitespace_forward("hello\n\nworld", 5, 0, WordKind::Word, &wc);
        assert_eq!(pos, 6);
    }

    // ─── skip_whitespace_backward ────────────────────────────────────────

    #[test]
    fn skip_ws_backward_basic() {
        let wc = default_wc();
        // "hello   world" — backward from 7 (space before 'w')
        let pos = skip_whitespace_backward("hello   world", 7, WordKind::Word, &wc);
        // Should stop at non-whitespace
        assert_eq!(pos, 4); // 'o' in "hello"
    }

    #[test]
    fn skip_ws_backward_empty_line() {
        let wc = default_wc();
        // "hello\n\nworld" — backward from 6 (\n at empty line)
        let pos = skip_whitespace_backward("hello\n\nworld", 6, WordKind::Word, &wc);
        assert_eq!(pos, 6); // stops at the empty line \n
    }

    // ─── skip_whitespace_forward_simple ──────────────────────────────────

    #[test]
    fn skip_ws_simple_forward() {
        let wc = default_wc();
        let pos = skip_whitespace_forward_simple("  hello", 0, WordKind::Word, &wc);
        assert_eq!(pos, 2);
    }

    #[test]
    fn skip_ws_simple_no_empty_line_stop() {
        let wc = default_wc();
        // Unlike skip_whitespace_forward, this does NOT stop at empty lines
        let pos = skip_whitespace_forward_simple("\n\nhello", 0, WordKind::Word, &wc);
        assert_eq!(pos, 2); // skips past both newlines
    }

    // ─── find_class_end_forward ──────────────────────────────────────────

    #[test]
    fn find_class_end_basic() {
        let wc = default_wc();
        // "hello world" — from 0, find end of "hello"
        let pos = find_class_end_forward("hello world", 0, WordKind::Word, &wc);
        assert_eq!(pos, 4); // last char of "hello"
    }

    #[test]
    fn find_class_end_single_char() {
        let wc = default_wc();
        let pos = find_class_end_forward("h world", 0, WordKind::Word, &wc);
        assert_eq!(pos, 0); // single-char word, stays at 0
    }

    // ─── composition: simulating w motion ────────────────────────────────

    #[test]
    fn compose_w_motion() {
        let wc = default_wc();
        let text = "hello world";
        let start = 0;
        // w = step forward, skip same class (if non-ws), skip whitespace
        let mut pos = step_forward(text, start);
        let starting_class =
            char_at(text, start).map(|c| CharClass::classify(c, WordKind::Word, &wc));
        if let Some(class) = starting_class {
            if class != CharClass::Whitespace {
                pos = skip_class_forward(text, pos, class, WordKind::Word, &wc);
            }
        }
        pos = skip_whitespace_forward(text, pos, start, WordKind::Word, &wc);
        assert_eq!(pos, 6); // start of "world"
    }

    // ─── composition: simulating e motion ────────────────────────────────

    #[test]
    fn compose_e_motion() {
        let wc = default_wc();
        let text = "hello world";
        let start = 0;
        // e = step forward, skip whitespace (simple), find class end
        let mut pos = step_forward(text, start);
        pos = skip_whitespace_forward_simple(text, pos, WordKind::Word, &wc);
        if pos < text.len() {
            pos = find_class_end_forward(text, pos, WordKind::Word, &wc);
        }
        assert_eq!(pos, 4); // end of "hello"
    }

    // ─── composition: simulating b motion ────────────────────────────────

    #[test]
    fn compose_b_motion() {
        let wc = default_wc();
        let text = "hello world";
        let start = 6;
        // b = step backward, skip whitespace backward, skip class backward
        let mut pos = step_backward(text, start);
        pos = skip_whitespace_backward(text, pos, WordKind::Word, &wc);
        if let Some(c) = char_at(text, pos) {
            if !CharClass::classify(c, WordKind::Word, &wc).is_whitespace() {
                let class = CharClass::classify(c, WordKind::Word, &wc);
                pos = skip_class_backward(text, pos, class, WordKind::Word, &wc);
            }
        }
        assert_eq!(pos, 0); // start of "hello"
    }

    // ─── U+200B consistency ─────────────────────────────────────────────

    #[test]
    fn zwsp_treated_as_whitespace_consistently() {
        // U+200B (Zero-Width Space) is whitespace in Neovim's utf_class_tab
        // but NOT in Rust's char::is_whitespace(). CharClass::classify()
        // handles this via is_neovim_blank(). Boundary helpers must agree.
        let wc = default_wc();
        let zwsp = '\u{200B}';

        // CharClass says whitespace
        assert_eq!(
            CharClass::classify(zwsp, WordKind::Word, &wc),
            CharClass::Whitespace,
        );

        // skip_whitespace_forward skips over ZWSP
        let text = "\u{200B}\u{200B}hello";
        let pos = skip_whitespace_forward(text, 0, 0, WordKind::Word, &wc);
        // ZWSP is 3 bytes in UTF-8, two of them = 6 bytes
        assert_eq!(pos, 6);

        // skip_whitespace_forward_simple also skips ZWSP
        let pos = skip_whitespace_forward_simple(text, 0, WordKind::Word, &wc);
        assert_eq!(pos, 6);

        // skip_whitespace_backward skips over ZWSP
        let text = "hello\u{200B}\u{200B}";
        // pos 10 = second ZWSP (byte 8..11), start there
        let pos = skip_whitespace_backward(text, 8, WordKind::Word, &wc);
        assert_eq!(pos, 4); // 'o' in "hello"
    }
}
