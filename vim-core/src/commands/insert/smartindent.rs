//! Smartindent adjustments for insert mode.
//!
//! Implements Neovim's `smartindent` option behavior for `{`, `}`, and `#`.
//! Pure computations: `(text, cursor, shiftwidth) → SmartIndentAction`.
//!
//! Neovim reference: `indent.c:ins_try_si()` (char-level) and
//! `change.c:open_line()` (newline-level `did_si` flag).
//!
//! Conditions for activation (matching `may_do_si()`):
//! - `smartindent` is enabled
//! - `cindent` is NOT enabled (cin takes priority)
//! - `indentexpr` is empty (external provider takes priority)
//! - Not in paste mode

use crate::commands::helpers::line_start_for_offset;

/// Result of smartindent analysis for a character being inserted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartIndentAction {
    /// No smartindent adjustment needed.
    None,
    /// `}` in indent region: set indent to the indent of the line containing
    /// the matching `{`.
    SetIndent {
        /// Target indent column count (in spaces).
        new_indent: usize,
    },
    /// `#` in indent region: strip all leading whitespace (move to column 0).
    StripAllIndent,
    /// `{` after `o`/`O` or after a newline: add one shiftwidth of indent.
    /// Only when `did_si` is set (previous line ended with `{` or cinword).
    AddShiftWidth,
}

/// Check whether the cursor is in the indent region of its line.
///
/// "In indent region" means the cursor is at or before the first non-whitespace
/// character on the line. Neovim's `inindent()`.
fn in_indent(text: &str, cursor: usize) -> bool {
    let line_start = line_start_for_offset(text, cursor);
    let before_cursor = &text[line_start..cursor];
    before_cursor.bytes().all(|b| b == b' ' || b == b'\t')
}

/// Compute the leading whitespace length (in bytes) of a line.
fn leading_ws_len(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// Compute the indent (in spaces) of a line, expanding tabs.
fn indent_of_line(line: &str, tabstop: usize) -> usize {
    let mut col = 0;
    for b in line.bytes() {
        match b {
            b' ' => col += 1,
            b'\t' => col = col / tabstop * tabstop + tabstop,
            _ => break,
        }
    }
    col
}

/// Find the matching `{` for a `}` at the given cursor position.
///
/// Simple brace-counting scanner that walks backward through the text.
/// Skips string/character literals and comments for basic correctness.
/// Returns the byte offset of the matching `{`, or `None` if unmatched.
fn find_matching_open_brace(text: &str, cursor: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let bytes = text.as_bytes();
    let mut i = cursor;

    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'}' => depth += 1,
            b'{' => {
                depth -= 1;
                if depth < 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Compute the smartindent action for a character typed in insert mode.
///
/// This is called from `precompute_insert` when smartindent is active
/// and no indent provider / cindent is present.
///
/// Returns `SmartIndentAction` describing what indent adjustment to make.
#[must_use]
pub fn compute_smartindent_action(
    text: &str,
    cursor: usize,
    ch: char,
    _shiftwidth: usize,
    tabstop: usize,
) -> SmartIndentAction {
    match ch {
        '#' => {
            if cursor > 0 && in_indent(text, cursor) {
                SmartIndentAction::StripAllIndent
            } else {
                SmartIndentAction::None
            }
        }
        '}' => {
            if in_indent(text, cursor) {
                if let Some(match_pos) = find_matching_open_brace(text, cursor) {
                    let match_line_start = line_start_for_offset(text, match_pos);
                    let match_line_end = text[match_line_start..]
                        .find('\n')
                        .map_or(text.len(), |n| match_line_start + n);
                    let match_line = &text[match_line_start..match_line_end];
                    let new_indent = indent_of_line(match_line, tabstop);
                    SmartIndentAction::SetIndent { new_indent }
                } else {
                    SmartIndentAction::None
                }
            } else {
                SmartIndentAction::None
            }
        }
        '{' => {
            if cursor > 0 && in_indent(text, cursor) {
                SmartIndentAction::AddShiftWidth
            } else {
                SmartIndentAction::None
            }
        }
        _ => SmartIndentAction::None,
    }
}

/// Build the indent string for a given column count.
#[must_use]
pub fn build_indent_string(indent_cols: usize, use_tabs: bool, tabstop: usize) -> String {
    if use_tabs && tabstop > 0 {
        let tabs = indent_cols / tabstop;
        let spaces = indent_cols % tabstop;
        "\t".repeat(tabs) + &" ".repeat(spaces)
    } else {
        " ".repeat(indent_cols)
    }
}

/// Compute the byte range of leading whitespace on the cursor's line,
/// and the number of spaces to replace it with based on the action.
///
/// Returns `(strip_start, strip_end, new_indent_string)` where:
/// - `strip_start..strip_end` is the byte range to delete
/// - `new_indent_string` is the replacement indent
#[must_use]
pub fn compute_indent_replacement(
    text: &str,
    cursor: usize,
    action: &SmartIndentAction,
    shiftwidth: usize,
    tabstop: usize,
    expandtab: bool,
) -> Option<(usize, usize, String)> {
    let line_start = line_start_for_offset(text, cursor);
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |n| line_start + n);
    let line = &text[line_start..line_end];
    let ws_len = leading_ws_len(line);
    let current_indent = indent_of_line(line, tabstop);

    match action {
        SmartIndentAction::StripAllIndent => {
            if ws_len > 0 {
                Some((line_start, line_start + ws_len, String::new()))
            } else {
                None
            }
        }
        SmartIndentAction::SetIndent { new_indent } => {
            let new_str = build_indent_string(*new_indent, !expandtab, tabstop);
            if ws_len > 0 || !new_str.is_empty() {
                Some((line_start, line_start + ws_len, new_str))
            } else {
                None
            }
        }
        SmartIndentAction::AddShiftWidth => {
            let new_indent = current_indent + shiftwidth;
            let new_str = build_indent_string(new_indent, !expandtab, tabstop);
            Some((line_start, line_start + ws_len, new_str))
        }
        SmartIndentAction::None => None,
    }
}

/// Compute smartindent extra for newline: whether the line before cursor
/// ends with `{` (after stripping trailing whitespace).
///
/// When true, the newline should add `shiftwidth` extra indent.
/// Neovim's `did_si` flag in `open_line()`.
#[must_use]
pub fn should_add_indent_after_newline(text: &str, cursor: usize) -> bool {
    let line_start = line_start_for_offset(text, cursor);
    let before_cursor = &text[line_start..cursor];
    let trimmed = before_cursor.trim_end();
    trimmed.ends_with('{')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_at_indent_strips() {
        let action = compute_smartindent_action("    ", 4, '#', 4, 4);
        assert_eq!(action, SmartIndentAction::StripAllIndent);
    }

    #[test]
    fn hash_not_in_indent_noop() {
        let action = compute_smartindent_action("int x;", 6, '#', 4, 4);
        assert_eq!(action, SmartIndentAction::None);
    }

    #[test]
    fn closing_brace_finds_match() {
        let text = "    if (true) {\n        x = 1;\n        ";
        let cursor = text.len();
        let action = compute_smartindent_action(text, cursor, '}', 4, 4);
        assert_eq!(action, SmartIndentAction::SetIndent { new_indent: 4 });
    }

    #[test]
    fn closing_brace_no_match() {
        let text = "        ";
        let cursor = text.len();
        let action = compute_smartindent_action(text, cursor, '}', 4, 4);
        assert_eq!(action, SmartIndentAction::None);
    }

    #[test]
    fn opening_brace_in_indent() {
        let action = compute_smartindent_action("    ", 4, '{', 4, 4);
        assert_eq!(action, SmartIndentAction::AddShiftWidth);
    }

    #[test]
    fn opening_brace_not_in_indent() {
        let action = compute_smartindent_action("int x", 5, '{', 4, 4);
        assert_eq!(action, SmartIndentAction::None);
    }

    #[test]
    fn newline_after_brace() {
        assert!(should_add_indent_after_newline("if (true) {", 11));
        assert!(should_add_indent_after_newline("if (true) {  ", 13));
    }

    #[test]
    fn newline_no_brace() {
        assert!(!should_add_indent_after_newline("int x = 5;", 10));
        assert!(!should_add_indent_after_newline("}", 1));
    }

    #[test]
    fn indent_replacement_strip_all() {
        let text = "    #include";
        let (start, end, new) =
            compute_indent_replacement(text, 4, &SmartIndentAction::StripAllIndent, 4, 4, true)
                .unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 4);
        assert_eq!(new, "");
    }

    #[test]
    fn indent_replacement_set() {
        let text = "        }";
        let (start, end, new) = compute_indent_replacement(
            text,
            8,
            &SmartIndentAction::SetIndent { new_indent: 4 },
            4,
            4,
            true,
        )
        .unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 8);
        assert_eq!(new, "    ");
    }

    #[test]
    fn indent_replacement_add_sw() {
        let text = "    {";
        let (start, end, new) =
            compute_indent_replacement(text, 4, &SmartIndentAction::AddShiftWidth, 4, 4, true)
                .unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 4);
        assert_eq!(new, "        ");
    }

    #[test]
    fn in_indent_empty_line() {
        assert!(in_indent("", 0));
    }

    #[test]
    fn in_indent_with_spaces() {
        assert!(in_indent("    hello", 2));
        assert!(in_indent("    hello", 4));
        assert!(!in_indent("    hello", 5));
    }

    #[test]
    fn indent_of_line_spaces() {
        assert_eq!(indent_of_line("    hello", 4), 4);
        assert_eq!(indent_of_line("        hello", 4), 8);
        assert_eq!(indent_of_line("hello", 4), 0);
    }

    #[test]
    fn indent_of_line_tabs() {
        assert_eq!(indent_of_line("\thello", 4), 4);
        assert_eq!(indent_of_line("\t\thello", 4), 8);
        assert_eq!(indent_of_line("  \thello", 4), 4);
    }

    #[test]
    fn build_indent_string_spaces() {
        assert_eq!(build_indent_string(4, false, 4), "    ");
        assert_eq!(build_indent_string(8, false, 4), "        ");
    }

    #[test]
    fn build_indent_string_tabs() {
        assert_eq!(build_indent_string(4, true, 4), "\t");
        assert_eq!(build_indent_string(6, true, 4), "\t  ");
        assert_eq!(build_indent_string(8, true, 4), "\t\t");
    }
}
