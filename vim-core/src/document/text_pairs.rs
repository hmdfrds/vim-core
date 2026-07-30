//! Text pair finding utilities.
//!
//! General-purpose algorithms for finding matching delimiters, quotes,
//! brackets, tags, and function calls in text. Available to any layer
//! above `document`.

/// Find the innermost matching pair of delimiters around `cursor`.
///
/// Returns `(open_start, open_end, close_start, close_end)` — byte offsets
/// where `open_start..open_end` is the opening delimiter and
/// `close_start..close_end` is the closing delimiter.
pub fn find_surrounding_pair(
    text: &str,
    cursor: usize,
    open: &str,
    close: &str,
) -> Option<(usize, usize, usize, usize)> {
    if open == close {
        let (open_pos, close_pos) = find_quote_pair(text, cursor, open)?;
        Some((
            open_pos,
            open_pos + open.len(),
            close_pos,
            close_pos + close.len(),
        ))
    } else {
        find_bracket_pair(text, cursor, open, close)
    }
}

/// Find matching quote pair containing cursor (same open/close delimiter).
///
/// Implements Vim's `current_quote` algorithm: line-scoped search with
/// asymmetric escape handling — opening quotes found without escape,
/// closing quotes use forward-skip. Zero allocation.
///
/// Returns `(open_pos, close_pos)` — byte offsets of the two quote characters.
pub fn find_quote_pair(text: &str, cursor: usize, quote: &str) -> Option<(usize, usize)> {
    let qb = quote.as_bytes().first().copied()?;
    if text.is_empty() || cursor > text.len() {
        return None;
    }

    let bytes = text.as_bytes();

    let line_start = bytes
        .get(..cursor)?
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let line_end = bytes
        .get(cursor..)?
        .iter()
        .position(|&b| b == b'\n')
        .map_or(text.len(), |p| cursor + p);

    let line = bytes.get(line_start..line_end)?;
    let col = cursor - line_start;

    let mut pos = 0;
    while let Some(open) = find_raw_quote(line, pos, qb) {
        if open > col {
            break;
        }
        let close = match find_escaped_quote(line, open + 1, qb) {
            Some(c) => c,
            None => break,
        };
        if col <= close {
            return Some((line_start + open, line_start + close));
        }
        pos = close + 1;
    }

    if let Some(open) = find_raw_quote(line, col, qb) {
        if let Some(close) = find_escaped_quote(line, open + 1, qb) {
            return Some((line_start + open, line_start + close));
        }
    }

    None
}

#[inline]
fn find_raw_quote(line: &[u8], start: usize, qb: u8) -> Option<usize> {
    line.get(start..)?
        .iter()
        .position(|&b| b == qb)
        .map(|p| start + p)
}

#[inline]
fn find_escaped_quote(line: &[u8], start: usize, qb: u8) -> Option<usize> {
    let len = line.len();
    let mut i = start;
    while i < len {
        let b = *line.get(i)?;
        if b == b'\\' {
            i += 2;
            continue;
        }
        if b == qb {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find matching bracket pair containing cursor (different open/close).
///
/// Returns `(open_start, open_end, close_start, close_end)` — byte offsets
/// where `open_start..open_end` is the opening bracket and
/// `close_start..close_end` is the closing bracket.
pub fn find_bracket_pair(
    text: &str,
    cursor: usize,
    open: &str,
    close: &str,
) -> Option<(usize, usize, usize, usize)> {
    let open_char: char = open.chars().next()?;
    let close_char: char = close.chars().next()?;

    let char_positions: Vec<(usize, char)> = text.char_indices().collect();

    let start_idx = char_positions
        .iter()
        .rposition(|&(byte_off, _)| byte_off <= cursor)?;

    let mut depth = 0i32;
    let mut open_pos = None;
    for &(byte_off, c) in char_positions.get(..=start_idx)?.iter().rev() {
        if c == close_char {
            depth += 1;
        } else if c == open_char {
            depth -= 1;
            if depth < 0 {
                open_pos = Some(byte_off);
                break;
            }
        }
    }

    let open_pos = open_pos?;

    depth = 0;
    for (byte_off, c) in text.char_indices() {
        if byte_off < open_pos {
            continue;
        }
        if c == open_char {
            depth += 1;
        } else if c == close_char {
            depth -= 1;
            if depth == 0 {
                return Some((
                    open_pos,
                    open_pos + open.len(),
                    byte_off,
                    byte_off + close.len(),
                ));
            }
        }
    }

    None
}

/// Describes a matched HTML/XML tag pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagPair {
    /// Byte offset of the opening `<`.
    pub open_start: usize,
    /// Byte offset just past the closing `>` of the opening tag.
    pub open_end: usize,
    /// Byte offset of the `<` in the closing tag.
    pub close_start: usize,
    /// Byte offset just past the closing `>` of the closing tag.
    pub close_end: usize,
    /// The tag name.
    pub tag_name: String,
}

fn extract_tag_name(tag_input: &str) -> &str {
    tag_input.split_whitespace().next().unwrap_or(tag_input)
}

/// Find the innermost HTML/XML tag pair containing `cursor`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn find_tag_pair(text: &str, cursor: usize) -> Option<TagPair> {
    let bytes = text.as_bytes();
    let len = bytes.len();

    let mut search_from = cursor;
    loop {
        let tag_open = find_open_angle(bytes, search_from)?;

        let angle_close = bytes
            .iter()
            .enumerate()
            .skip(tag_open + 1)
            .find_map(|(j, &b)| if b == b'>' { Some(j) } else { None });

        let Some(angle_close) = angle_close else {
            if tag_open == 0 {
                return None;
            }
            search_from = tag_open - 1;
            continue;
        };

        if angle_close > 0 && bytes.get(angle_close - 1).copied() == Some(b'/') {
            if tag_open == 0 {
                return None;
            }
            search_from = tag_open - 1;
            continue;
        }

        let tag_content = &text[tag_open + 1..angle_close];
        let tag_name = extract_tag_name(tag_content);
        if tag_name.is_empty() {
            if tag_open == 0 {
                return None;
            }
            search_from = tag_open - 1;
            continue;
        }

        let open_end = angle_close + 1;

        let close_tag = format!("</{tag_name}>");
        let open_tag_prefix = format!("<{tag_name}");

        let mut depth = 1i32;
        let mut scan = open_end;
        let mut close_start = None;
        let mut close_end_pos = None;

        while scan < len {
            if bytes.get(scan).copied() == Some(b'<') {
                if text.get(scan..).is_some_and(|s| s.starts_with(&close_tag)) {
                    depth -= 1;
                    if depth == 0 {
                        close_start = Some(scan);
                        close_end_pos = Some(scan + close_tag.len());
                        break;
                    }
                    scan += close_tag.len();
                    continue;
                }
                if text
                    .get(scan..)
                    .is_some_and(|s| s.starts_with(&open_tag_prefix))
                {
                    let after_name = scan + open_tag_prefix.len();
                    if let Some(&b) = bytes.get(after_name) {
                        if (b == b' ' || b == b'>' || b == b'/' || b == b'\t' || b == b'\n')
                            && !is_self_closing_at(bytes, after_name, len)
                        {
                            depth += 1;
                        }
                    }
                }
            }
            scan += 1;
        }

        if let (Some(cs), Some(ce)) = (close_start, close_end_pos) {
            if cursor >= tag_open && cursor < ce {
                return Some(TagPair {
                    open_start: tag_open,
                    open_end,
                    close_start: cs,
                    close_end: ce,
                    tag_name: tag_name.to_owned(),
                });
            }
        }

        if tag_open == 0 {
            return None;
        }
        search_from = tag_open - 1;
    }
}

fn find_open_angle(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    loop {
        if bytes.get(i).copied() == Some(b'<') && bytes.get(i + 1).copied() != Some(b'/') {
            return Some(i);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

fn is_self_closing_at(bytes: &[u8], after_name: usize, len: usize) -> bool {
    for k in after_name..len {
        match bytes.get(k).copied() {
            Some(b'>') => return k > 0 && bytes.get(k - 1).copied() == Some(b'/'),
            None => return false,
            _ => {}
        }
    }
    false
}

/// Describes a matched function call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncPair {
    /// Byte offset of the first character of the function name.
    pub name_start: usize,
    /// Byte offset just past the function name (== open paren position).
    pub name_end: usize,
    /// Byte offset of the opening parenthesis.
    pub open_paren: usize,
    /// Byte offset of the closing parenthesis.
    pub close_paren: usize,
    /// The function name extracted from the text.
    pub func_name: String,
}

fn is_func_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '$' || c == ':' || c == '@' || c == '#'
}

/// Find the enclosing function call `name(...)` containing `cursor`.
#[must_use]
pub fn find_function_pair(text: &str, cursor: usize) -> Option<FuncPair> {
    let (open_paren, _, close_paren, _) = find_bracket_pair(text, cursor, "(", ")")?;

    if open_paren == 0 {
        return None;
    }

    let chars_before: Vec<(usize, char)> = text[..open_paren].char_indices().collect();

    let mut name_start = open_paren;
    for &(byte_off, c) in chars_before.iter().rev() {
        if is_func_name_char(c) {
            name_start = byte_off;
        } else {
            break;
        }
    }

    if name_start == open_paren {
        return None;
    }

    Some(FuncPair {
        name_start,
        name_end: open_paren,
        open_paren,
        close_paren,
        func_name: text[name_start..open_paren].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- find_surrounding_pair (4-value) ---

    #[test]
    fn surrounding_pair_brackets() {
        let text = "foo(bar)baz";
        let result = find_surrounding_pair(text, 4, "(", ")");
        assert_eq!(result, Some((3, 4, 7, 8)));
    }

    #[test]
    fn surrounding_pair_quotes() {
        let text = r#"say "hello" now"#;
        let result = find_surrounding_pair(text, 6, "\"", "\"");
        assert_eq!(result, Some((4, 5, 10, 11)));
    }

    #[test]
    fn surrounding_pair_none_when_outside() {
        let text = "no pairs here";
        assert_eq!(find_surrounding_pair(text, 3, "(", ")"), None);
    }

    // --- find_quote_pair ---

    #[test]
    fn quote_pair_basic() {
        let text = r#"say "hello" now"#;
        assert_eq!(find_quote_pair(text, 6, "\""), Some((4, 10)));
    }

    #[test]
    fn quote_pair_escaped_close() {
        let text = r#"say "he\"llo" now"#;
        assert_eq!(find_quote_pair(text, 6, "\""), Some((4, 12)));
    }

    #[test]
    fn quote_pair_cursor_on_open() {
        let text = r#""hello""#;
        assert_eq!(find_quote_pair(text, 0, "\""), Some((0, 6)));
    }

    #[test]
    fn quote_pair_cursor_on_close() {
        let text = r#""hello""#;
        assert_eq!(find_quote_pair(text, 6, "\""), Some((0, 6)));
    }

    #[test]
    fn quote_pair_no_match() {
        let text = "no quotes";
        assert_eq!(find_quote_pair(text, 3, "\""), None);
    }

    #[test]
    fn quote_pair_single_quotes() {
        // Apostrophe in "it's" pairs with the next quote, so cursor=7
        // falls outside that pair (2..5). No enclosing pair found.
        let text = "it's 'fine' here";
        assert_eq!(find_quote_pair(text, 7, "'"), None);
        // Without the apostrophe, quotes pair correctly:
        let text2 = "say 'fine' here";
        assert_eq!(find_quote_pair(text2, 6, "'"), Some((4, 9)));
    }

    #[test]
    fn quote_pair_line_scoped() {
        let text = "\"first\"\n\"second\"";
        // Cursor on 'e' in "second" (byte 9); close quote is at byte 15.
        assert_eq!(find_quote_pair(text, 9, "\""), Some((8, 15)));
    }

    // --- find_bracket_pair ---

    #[test]
    fn bracket_pair_basic() {
        let text = "(hello)";
        assert_eq!(find_bracket_pair(text, 3, "(", ")"), Some((0, 1, 6, 7)));
    }

    #[test]
    fn bracket_pair_nested() {
        let text = "(a(b)c)";
        // Cursor inside inner pair
        assert_eq!(find_bracket_pair(text, 3, "(", ")"), Some((2, 3, 4, 5)));
        // Cursor outside inner but inside outer
        assert_eq!(find_bracket_pair(text, 5, "(", ")"), Some((0, 1, 6, 7)));
    }

    #[test]
    fn bracket_pair_braces() {
        let text = "{ foo }";
        assert_eq!(find_bracket_pair(text, 3, "{", "}"), Some((0, 1, 6, 7)));
    }

    #[test]
    fn bracket_pair_square() {
        let text = "[a, b]";
        assert_eq!(find_bracket_pair(text, 2, "[", "]"), Some((0, 1, 5, 6)));
    }

    #[test]
    fn bracket_pair_no_match() {
        let text = "no brackets";
        assert_eq!(find_bracket_pair(text, 3, "(", ")"), None);
    }

    // --- find_tag_pair ---

    #[test]
    fn tag_pair_basic() {
        let text = "<div>hello</div>";
        let result = find_tag_pair(text, 6).unwrap();
        assert_eq!(result.open_start, 0);
        assert_eq!(result.open_end, 5);
        assert_eq!(result.close_start, 10);
        assert_eq!(result.close_end, 16);
        assert_eq!(result.tag_name, "div");
    }

    #[test]
    fn tag_pair_with_attrs() {
        let text = r#"<div class="x">hi</div>"#;
        let result = find_tag_pair(text, 16).unwrap();
        assert_eq!(result.tag_name, "div");
        assert_eq!(result.open_start, 0);
        assert_eq!(result.open_end, 15);
        assert_eq!(result.close_start, 17);
        assert_eq!(result.close_end, 23);
    }

    #[test]
    fn tag_pair_nested() {
        let text = "<div><span>hi</span></div>";
        // Cursor inside span
        let result = find_tag_pair(text, 12).unwrap();
        assert_eq!(result.tag_name, "span");
    }

    #[test]
    fn tag_pair_self_closing_skipped() {
        let text = "<div><br/>hello</div>";
        let result = find_tag_pair(text, 11).unwrap();
        assert_eq!(result.tag_name, "div");
    }

    #[test]
    fn tag_pair_no_match() {
        let text = "no tags here";
        assert!(find_tag_pair(text, 3).is_none());
    }

    // --- find_function_pair ---

    #[test]
    fn func_pair_basic() {
        let text = "foo(bar, baz)";
        let result = find_function_pair(text, 5).unwrap();
        assert_eq!(result.name_start, 0);
        assert_eq!(result.name_end, 3);
        assert_eq!(result.open_paren, 3);
        assert_eq!(result.close_paren, 12);
        assert_eq!(result.func_name, "foo");
    }

    #[test]
    fn func_pair_dotted() {
        let text = "obj.method(x)";
        let result = find_function_pair(text, 11).unwrap();
        assert_eq!(result.name_start, 0);
        assert_eq!(result.name_end, 10);
        assert_eq!(result.func_name, "obj.method");
    }

    #[test]
    fn func_pair_nested() {
        let text = "outer(inner(x))";
        // Cursor inside inner
        let result = find_function_pair(text, 12).unwrap();
        assert_eq!(result.name_start, 6);
        assert_eq!(result.name_end, 11);
        assert_eq!(result.func_name, "inner");
    }

    #[test]
    fn func_pair_no_name() {
        let text = "(just parens)";
        assert!(find_function_pair(text, 5).is_none());
    }

    #[test]
    fn func_pair_no_parens() {
        let text = "no_call";
        assert!(find_function_pair(text, 3).is_none());
    }
}
