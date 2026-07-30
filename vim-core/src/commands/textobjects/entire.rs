//! Entire-buffer text objects (ie, ae).
//!
//! Inspired by vim-textobj-entire:
//! - `ae` selects the entire buffer (all text, linewise).
//! - `ie` selects the entire buffer excluding leading and trailing blank lines.

use super::types::{TextObjectContext, TextObjectRange};
use crate::commands::helpers;
use crate::grammar::types::TextObjectScope;

/// Compute the entire-buffer text object.
///
/// - `ae` (around): selects the full buffer as a linewise range.
/// - `ie` (inner): trims leading and trailing blank lines, returning
///   the content in between (still linewise).
///
/// Returns `None` only if the buffer is empty.
#[must_use]
pub fn compute_entire_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    if text.is_empty() {
        return None;
    }

    if !scope.is_inner() {
        // ae: the entire buffer, linewise.
        return Some(TextObjectRange::line(0, text.len()));
    }

    // ie: trim leading and trailing blank lines.
    let total_lines = helpers::line_count(text);

    // Find first non-blank line (scan forward).
    let first_non_blank = (0..total_lines)
        .find(|&i| helpers::line_content(text, i).is_some_and(|l| !l.trim().is_empty()));
    // Find last non-blank line (scan backward).
    let last_non_blank = (0..total_lines)
        .rev()
        .find(|&i| helpers::line_content(text, i).is_some_and(|l| !l.trim().is_empty()));

    let (first, last) = match (first_non_blank, last_non_blank) {
        (Some(f), Some(l)) => (f, l),
        // All lines are blank — return the whole buffer.
        _ => return Some(TextObjectRange::line(0, text.len())),
    };

    // Compute byte offset for the start of `first` line.
    let start = helpers::line_start(text, first).unwrap_or(0);
    // Compute byte offset for the end of `last` line (include its trailing newline if present).
    let end = helpers::line_start(text, last + 1).unwrap_or(text.len());

    Some(TextObjectRange::line(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, inner: bool, expected: Option<(usize, usize)>) {
        let ctx = TextObjectContext::new(text, 0);
        let scope = TextObjectScope::from_inner_flag(inner);
        let result = compute_entire_object(&ctx, scope);
        match (result, expected) {
            (Some(r), Some((s, e))) => {
                assert_eq!(r.start(), s, "start mismatch for {:?}", text);
                assert_eq!(r.end(), e, "end mismatch for {:?}", text);
                assert!(r.linewise, "entire object should be linewise");
            }
            (None, None) => {}
            _ => panic!("result {:?} != expected {:?}", result, expected),
        }
    }

    // ── ae (around entire) ───────────────────────────────────────────────

    #[test]
    fn ae_single_line() {
        check("hello", false, Some((0, 5)));
    }

    #[test]
    fn ae_multi_line() {
        check("hello\nworld\n", false, Some((0, 12)));
    }

    #[test]
    fn ae_empty() {
        check("", false, None);
    }

    #[test]
    fn ae_blank_lines_only() {
        check("\n\n\n", false, Some((0, 3)));
    }

    #[test]
    fn ae_with_leading_trailing_blanks() {
        let text = "\n\nhello\nworld\n\n";
        check(text, false, Some((0, text.len())));
    }

    // ── ie (inner entire) ────────────────────────────────────────────────

    #[test]
    fn ie_no_blank_lines() {
        check("hello\nworld", true, Some((0, 11)));
    }

    #[test]
    fn ie_leading_blank_lines() {
        let text = "\n\nhello\nworld";
        // First non-blank line is "hello" at byte 2.
        check(text, true, Some((2, text.len())));
    }

    #[test]
    fn ie_trailing_blank_lines() {
        let text = "hello\nworld\n\n\n";
        // Last non-blank line is "world", which ends at the \n after "world" (byte 12).
        check(text, true, Some((0, 12)));
    }

    #[test]
    fn ie_leading_and_trailing_blank_lines() {
        let text = "\n\nhello\nworld\n\n";
        check(text, true, Some((2, 14)));
    }

    #[test]
    fn ie_all_blank() {
        // All blank => ie returns the whole buffer.
        check("\n\n\n", true, Some((0, 3)));
    }

    #[test]
    fn ie_single_content_line_with_blanks() {
        let text = "\nhello\n";
        check(text, true, Some((1, 7)));
    }

    // ── cursor position is irrelevant for entire-buffer objects ──────────

    #[test]
    fn ae_cursor_middle() {
        let text = "hello\nworld\nfoo";
        let ctx = TextObjectContext::new(text, 6); // cursor on 'w'
        let result = compute_entire_object(&ctx, TextObjectScope::Around).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), text.len());
    }

    #[test]
    fn ie_cursor_end() {
        let text = "\nhello\n";
        let ctx = TextObjectContext::new(text, 6); // cursor at end
        let result = compute_entire_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.start(), 1);
        assert_eq!(result.end(), 7);
    }
}
