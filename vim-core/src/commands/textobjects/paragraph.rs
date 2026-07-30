//! Paragraph text objects (ip, ap).
//!
//! Paragraph = text separated by blank lines.

use super::helpers::{is_blank_line, line_end_for_offset, line_start_for_offset};
use super::types::{TextObjectContext, TextObjectRange};
use crate::grammar::types::TextObjectScope;

/// Compute a paragraph text object.
///
/// Uses an iterator-based approach that scans outward from the cursor line
/// rather than collecting all line boundaries upfront.
#[must_use]
pub fn compute_paragraph_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    if text.is_empty() {
        return Some(TextObjectRange::line(0, 0));
    }

    let cursor = if ctx.cursor.get() >= text.len() && !text.is_empty() {
        crate::primitives::text_util::prev_char_boundary(text, text.len())
    } else {
        ctx.cursor.get()
    };
    let cursor_line_start = line_start_for_offset(text, cursor);
    let cursor_line_end = line_end_for_offset(text, cursor);
    let on_blank = is_blank_line(text, cursor);

    // Find paragraph start: scan backward from cursor line
    let mut para_start = scan_backward(text, cursor_line_start, on_blank);

    // Find paragraph end: scan forward from cursor line
    let mut para_end = line_end_inclusive(text, cursor_line_end);
    let para_end_scan = scan_forward_core(text, cursor_line_end, on_blank, &mut para_end);

    if !scope.is_inner() {
        let pre_extend_end = para_end;
        extend_around(
            text,
            on_blank,
            para_end_scan,
            &mut para_start,
            &mut para_end,
        );
        // If cursor is on a blank line and "around" failed to include any
        // non-blank content (extend_around didn't grow the range), the text
        // object is invalid — Neovim treats `dap` on all-blank text as a no-op.
        if on_blank && para_end == pre_extend_end {
            return None;
        }
    }

    Some(TextObjectRange::line(para_start, para_end))
}

/// Scan backward from `start` to find the paragraph start byte offset.
fn scan_backward(text: &str, start: usize, on_blank: bool) -> usize {
    let mut para_start = start;
    let mut scan = start;
    while scan > 0 {
        let prev_start = line_start_for_offset(text, scan - 1);
        let prev_blank =
            is_line_blank_range(text, prev_start, line_end_for_offset(text, prev_start));
        if on_blank != prev_blank {
            break;
        }
        para_start = prev_start;
        scan = prev_start;
    }
    para_start
}

/// Scan forward from the cursor line end, updating `para_end`.
/// Returns the "scan position" (the line_end before the inclusive newline).
fn scan_forward_core(
    text: &str,
    cursor_line_end: usize,
    on_blank: bool,
    para_end: &mut usize,
) -> usize {
    let mut scan_end = cursor_line_end;
    while let Some(next_start) = next_line_start(text, scan_end) {
        if next_start >= text.len() {
            break;
        }
        let next_end = line_end_for_offset(text, next_start);
        let next_blank = is_line_blank_range(text, next_start, next_end);
        if on_blank != next_blank {
            break;
        }
        *para_end = line_end_inclusive(text, next_end);
        scan_end = next_end;
    }
    // Recover the line_end of the last paragraph line
    if *para_end > 0
        && *para_end <= text.len()
        && text.as_bytes().get(*para_end - 1) == Some(&b'\n')
    {
        *para_end - 1
    } else {
        *para_end
    }
}

/// Extend the paragraph range for "around" scope.
fn extend_around(
    text: &str,
    on_blank: bool,
    para_end_scan: usize,
    para_start: &mut usize,
    para_end: &mut usize,
) {
    if on_blank {
        // Around on blank line: also include the following non-blank paragraph
        scan_forward_while(text, para_end_scan, para_end, |blank| !blank);
    } else {
        // Around on content: include trailing blank lines
        let saved = *para_end;
        scan_forward_while(text, para_end_scan, para_end, |blank| blank);
        if *para_end == saved {
            // No trailing blank lines: include preceding blank lines instead
            let mut scan = *para_start;
            while scan > 0 {
                let prev_start = line_start_for_offset(text, scan - 1);
                let prev_blank =
                    is_line_blank_range(text, prev_start, line_end_for_offset(text, prev_start));
                if !prev_blank {
                    break;
                }
                *para_start = prev_start;
                scan = prev_start;
            }
        }
    }
}

/// Scan forward from `scan_end`, including lines where `predicate(is_blank)` is true.
fn scan_forward_while(
    text: &str,
    mut scan_end: usize,
    para_end: &mut usize,
    predicate: impl Fn(bool) -> bool,
) {
    while let Some(next_start) = next_line_start(text, scan_end) {
        if next_start >= text.len() {
            break;
        }
        let next_end = line_end_for_offset(text, next_start);
        let next_blank = is_line_blank_range(text, next_start, next_end);
        if !predicate(next_blank) {
            break;
        }
        *para_end = line_end_inclusive(text, next_end);
        scan_end = next_end;
    }
}

/// Get the start of the next line after `line_end`, or `None` if at end.
fn next_line_start(text: &str, line_end: usize) -> Option<usize> {
    if line_end < text.len() && text.as_bytes().get(line_end) == Some(&b'\n') {
        Some(line_end + 1)
    } else {
        None
    }
}

/// Include the newline after `line_end` if present, otherwise return `line_end`.
fn line_end_inclusive(text: &str, line_end: usize) -> usize {
    if line_end < text.len() && text.as_bytes().get(line_end) == Some(&b'\n') {
        line_end + 1
    } else {
        line_end
    }
}

/// Check if the text in `[start..end)` is blank (empty or whitespace only).
fn is_line_blank_range(text: &str, start: usize, end: usize) -> bool {
    text.get(start..end)
        .is_none_or(|s| s.chars().all(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_paragraph() {
        let text = "hello\nworld";
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_paragraph_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 11);
        assert!(result.linewise);
    }

    #[test]
    fn test_two_paragraphs() {
        let text = "para1\n\npara2";
        // Cursor in first paragraph
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_paragraph_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 6); // "para1\n"
    }

    #[test]
    fn test_around_includes_trailing_blank() {
        let text = "para1\n\npara2";
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_paragraph_object(&ctx, TextObjectScope::Around).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 7); // "para1\n\n"
    }

    #[test]
    fn test_cursor_on_blank_line() {
        let text = "para1\n\npara2";
        let ctx = TextObjectContext::new(text, 6);
        let result = compute_paragraph_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.start(), 6);
        assert_eq!(result.end(), 7); // Just the blank line
    }

    /// Whitespace-only lines ARE paragraph separators (matching Neovim).
    /// `ip` with cursor on "para1" should NOT include the whitespace-only line.
    #[test]
    fn test_ip_whitespace_only_line_is_boundary() {
        // "para1\n   \npara2" — the "   " line IS a separator
        let text = "para1\n   \npara2";
        let ctx = TextObjectContext::new(text, 0); // cursor on "para1"
        let result = compute_paragraph_object(&ctx, TextObjectScope::Inner).unwrap();
        // Only "para1\n" is the inner paragraph
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 6);
    }

    /// Truly empty lines ARE paragraph separators.
    #[test]
    fn test_ip_empty_line_is_boundary() {
        let text = "para1\n\npara2";
        let ctx = TextObjectContext::new(text, 0); // cursor on "para1"
        let result = compute_paragraph_object(&ctx, TextObjectScope::Inner).unwrap();
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 6); // "para1\n" — stops before empty line
    }

    /// `ap` treats whitespace-only lines as boundaries (matching Neovim).
    #[test]
    fn test_ap_whitespace_only_line_is_boundary() {
        // "para1\n   \npara2" — the "   " line IS a separator
        let text = "para1\n   \npara2";
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_paragraph_object(&ctx, TextObjectScope::Around).unwrap();
        // "around" includes trailing blank: "para1\n" + "   \n" = offsets 0..10
        assert_eq!(result.start(), 0);
        assert_eq!(result.end(), 10);
    }
}
