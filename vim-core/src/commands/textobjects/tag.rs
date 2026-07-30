//! Tag text objects (it, at).
//!
//! HTML/XML tag matching.

use super::types::{TextObjectContext, TextObjectRange};
use crate::grammar::types::TextObjectScope;

/// Compute a tag text object.
///
/// # Arguments
///
/// * `ctx` - Text object context with text and cursor
/// * `inner` - If true, content between tags; if false, including tags
///
/// # Returns
///
/// `Some(TextObjectRange)` if matching tags found, `None` otherwise.
#[must_use]
pub fn compute_tag_object(
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

    // Find the opening tag containing cursor
    let (open_start, open_end, tag_name) = find_opening_tag(text, cursor)?;

    // Find the matching closing tag
    let (close_start, close_end) = find_closing_tag(text, open_end, &tag_name)?;

    if scope.is_inner() {
        // Content between tags
        if open_end <= close_start {
            Some(TextObjectRange::char(open_end, close_start))
        } else {
            None
        }
    } else {
        // Including tags — check if range covers full lines (linewise)
        // Must also contain a newline to be truly multi-line
        let contains_newline = text[open_start..close_end].contains('\n');
        let is_linewise = contains_newline
            && is_at_line_start(text, open_start)
            && is_at_line_end(text, close_end);
        if is_linewise {
            Some(TextObjectRange::line(open_start, close_end))
        } else {
            Some(TextObjectRange::char(open_start, close_end))
        }
    }
}

/// Check if offset is at the start of a line (position 0 or after '\n').
fn is_at_line_start(text: &str, offset: usize) -> bool {
    offset == 0 || text.as_bytes().get(offset.wrapping_sub(1)) == Some(&b'\n')
}

/// Check if offset is at the end of a line (at text end or at/before '\n').
fn is_at_line_end(text: &str, offset: usize) -> bool {
    offset >= text.len() || text.as_bytes().get(offset) == Some(&b'\n')
}

/// Find the opening tag containing cursor.
/// Returns (`tag_start`, `tag_end`, `tag_name`).
fn find_opening_tag(text: &str, cursor: usize) -> Option<(usize, usize, String)> {
    let search_end = crate::primitives::text_util::next_char_boundary(text, cursor.min(text.len()));
    let mut search_before = search_end;

    loop {
        let mut pos = text[..search_before].rfind('<')?;

        if text.as_bytes().get(pos + 1) == Some(&b'/') {
            let mut depth: usize = 1;
            if pos == 0 {
                return None;
            }
            loop {
                match text[..pos].rfind('<') {
                    Some(p) => {
                        if text.as_bytes().get(p + 1) == Some(&b'/') {
                            depth += 1;
                        } else {
                            depth -= 1;
                            if depth == 0 {
                                pos = p;
                                break;
                            }
                        }
                        if p == 0 {
                            return None;
                        }
                        pos = p;
                    }
                    None => return None,
                }
            }
        }

        let tag_start = pos;
        let tag_end = text[tag_start..].find('>')?.checked_add(tag_start)? + 1;
        let tag_content = &text[tag_start + 1..tag_end - 1];

        // Skip self-closing tags (e.g., <br/>, <img src="..." />)
        if tag_content.trim_end().ends_with('/') {
            if tag_start == 0 {
                return None;
            }
            search_before = tag_start;
            continue;
        }

        let tag_name = tag_content
            .split(|c: char| c.is_whitespace() || c == '>')
            .next()?
            .to_owned();

        if tag_name.is_empty() || tag_name.starts_with('/') {
            return None;
        }

        return Some((tag_start, tag_end, tag_name));
    }
}

/// Check if the byte at `pos` terminates a tag name (is `>`, `/`, whitespace, or end-of-text).
fn is_tag_name_boundary(text: &str, pos: usize) -> bool {
    matches!(
        text.as_bytes().get(pos),
        None | Some(b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r')
    )
}

/// Find the closing tag matching `tag_name`, starting from position.
fn find_closing_tag(text: &str, start: usize, tag_name: &str) -> Option<(usize, usize)> {
    let close_pattern = format!("</{tag_name}");
    let open_pattern = format!("<{tag_name}");
    let mut depth = 1;
    let mut pos = start;

    while pos < text.len() {
        // Look for either opening or closing tag
        if let Some(found) = text[pos..].find('<') {
            let abs_pos = pos + found;

            // Check if closing tag — must be followed by a tag-name terminator
            // to avoid matching e.g. `</divider>` when searching for `</div>`.
            if text[abs_pos..].starts_with(&close_pattern)
                && is_tag_name_boundary(text, abs_pos + close_pattern.len())
            {
                depth -= 1;
                if depth == 0 {
                    // Find the end of this closing tag
                    let close_end = text[abs_pos..].find('>')?.checked_add(abs_pos)? + 1;
                    return Some((abs_pos, close_end));
                }
            } else if text[abs_pos..].starts_with(&open_pattern)
                && is_tag_name_boundary(text, abs_pos + open_pattern.len())
            {
                // Opening tag of same name
                depth += 1;
            }
            pos = abs_pos + 1;
        } else {
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, cursor: usize, inner: bool, expected: Option<(usize, usize)>) {
        let ctx = TextObjectContext::new(text, cursor);
        let result = compute_tag_object(&ctx, TextObjectScope::from_inner_flag(inner));
        match (result, expected) {
            (Some(r), Some((s, e))) => {
                assert_eq!(r.start(), s, "start mismatch for '{}' at {}", text, cursor);
                assert_eq!(r.end(), e, "end mismatch for '{}' at {}", text, cursor);
            }
            (None, None) => {}
            _ => panic!(
                "result {:?} != expected {:?} for '{}' at {}",
                result, expected, text, cursor
            ),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // BASIC TAG TESTS
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_simple_tag() {
        check("<div>content</div>", 6, true, Some((5, 12)));
        check("<div>content</div>", 6, false, Some((0, 18)));
    }

    #[test]
    fn test_around_tag() {
        check("<div>content</div>", 6, false, Some((0, 18)));
    }

    #[test]
    fn test_empty_tag() {
        check("<div></div>", 5, true, Some((5, 5)));
        check("<div></div>", 5, false, Some((0, 11)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // NESTED TAGS
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_nested_tags() {
        let text = "<outer><inner>text</inner></outer>";
        // Cursor in "text"
        check(text, 14, true, Some((14, 18)));
    }

    #[test]
    fn test_nested_tags_outer() {
        let text = "<outer><inner>text</inner></outer>";
        // Should get content of outer tag when cursor is on 'outer'
        check(text, 1, true, Some((7, 26)));
    }

    #[test]
    fn test_deeply_nested() {
        let text = "<a><b><c>x</c></b></a>";
        check(text, 9, true, Some((9, 10))); // innermost <c>
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TAGS WITH ATTRIBUTES
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_tag_with_attributes() {
        check("<div class=\"foo\">bar</div>", 17, true, Some((17, 20)));
        check("<div class=\"foo\">bar</div>", 17, false, Some((0, 26)));
    }

    #[test]
    fn test_tag_with_multiple_attrs() {
        check(
            "<div id=\"one\" class=\"two\">x</div>",
            26,
            true,
            Some((26, 27)),
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // MULTILINE TAGS
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_multiline_tag() {
        let text = "<div>\n  content\n</div>";
        // <div> is 5 chars, then \n, content is at 6-15, close tag starts at 16
        check(text, 8, true, Some((5, 16)));
    }

    #[test]
    fn test_multiline_around() {
        let text = "<div>\n  content\n</div>";
        check(text, 8, false, Some((0, 22)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // EDGE CASES
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_no_tag() {
        check("just text", 0, true, None);
    }

    #[test]
    fn test_unclosed_tag() {
        check("<div>content", 6, true, None);
    }

    #[test]
    fn test_cursor_on_opening_tag() {
        check("<div>x</div>", 1, true, Some((5, 6)));
    }

    #[test]
    fn test_cursor_on_closing_tag() {
        check("<div>x</div>", 7, true, Some((5, 6)));
    }

    #[test]
    fn test_different_tag_names() {
        check("<span>x</span>", 6, true, Some((6, 7)));
        check("<p>x</p>", 3, true, Some((3, 4)));
    }
}
