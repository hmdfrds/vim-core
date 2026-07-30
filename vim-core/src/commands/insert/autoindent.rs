//! Autoindent and tab expansion for insert mode.
//!
//! Pure text computations extracted from `execution/insert_handler.rs`.
//! No state mutations, no Effects — just `(text, cursor) → computed values`.

use super::types::NewlineInsert;
use crate::commands::helpers::{expand_tabs, line_of, line_start_for_offset};
use crate::document::{IndentProvider, IndentResult};
use crate::primitives::LineNumber;

/// Compute the newline+indent text to insert and the trailing whitespace to strip.
///
/// Pure: `(text, cursor, tabstop, autoindent, indent_provider, did_ai) → NewlineInsert`.
///
/// If an `IndentProvider` is supplied, delegates to it for language-aware
/// indentation. Otherwise falls back to copying the current line's leading
/// whitespace (basic autoindent) — but only when `autoindent` is true.
/// When `autoindent` is false and no provider is set, no indent is added.
///
/// - Expands tabs to spaces (expandtab) using `tabstop` in fallback mode
/// - Reports trailing whitespace after cursor that Vim strips on Enter
/// - Reports leading whitespace before cursor to strip when `did_ai` is set
///   and the line up to cursor is only whitespace (Neovim's `trunc_line`)
#[must_use]
pub fn compute_newline_insert(
    text: &str,
    cursor: usize,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn IndentProvider>,
) -> NewlineInsert {
    compute_newline_insert_with_ai(text, cursor, tabstop, autoindent, indent_provider, false)
}

/// Like [`compute_newline_insert`] but accepts a `did_ai` flag for
/// Neovim's `trunc_line` (autoindent-only line stripping on Enter).
#[must_use]
pub fn compute_newline_insert_with_ai(
    text: &str,
    cursor: usize,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn IndentProvider>,
    did_ai: bool,
) -> NewlineInsert {
    let indent_result: IndentResult = if let Some(provider) = indent_provider {
        let line = line_of(text, cursor);
        provider.indent_for_new_line(LineNumber::from(line))
    } else if autoindent {
        // Fallback: copy current line's leading whitespace
        let line_start = line_start_for_offset(text, cursor);
        let line_content = &text[line_start..];
        let indent_len = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
        let raw_indent = &text[line_start..line_start + indent_len];
        IndentResult::Simple {
            indent: compact_str::CompactString::from(expand_tabs_to_spaces(raw_indent, tabstop)),
            append: None,
        }
    } else {
        IndentResult::Simple {
            indent: compact_str::CompactString::default(),
            append: None,
        }
    };

    let (insert_text, insert_len) = match &indent_result {
        IndentResult::Simple { indent, append } => {
            let append_str = append.as_deref().unwrap_or("");
            let text = compact_str::format_compact!("\n{indent}{append_str}");
            let len = text.len();
            (text, len)
        }
        IndentResult::IndentOutdent {
            indent,
            append,
            closing_indent,
        } => {
            let append_str = append.as_deref().unwrap_or("");
            let text = compact_str::format_compact!("\n{indent}{append_str}\n{closing_indent}");
            // Cursor lands on the first new line (after \n + indent + append),
            // not at the end of the full text.
            let cursor_len = 1 + indent.len() + append_str.len();
            (text, cursor_len)
        }
    };

    // Strip leading whitespace after break point (Vim strips spaces/tabs after cursor on Enter)
    let after_cursor = &text[cursor..];
    let stripped_len = after_cursor.len() - after_cursor.trim_start_matches([' ', '\t']).len();

    // Neovim's trunc_line: when did_ai is set and the line from line_start to
    // cursor is entirely whitespace, strip that leading whitespace.
    let leading_strip_len = if did_ai {
        let line_start = line_start_for_offset(text, cursor);
        let before_cursor = &text[line_start..cursor];
        if !before_cursor.is_empty() && before_cursor.bytes().all(|b| b == b' ' || b == b'\t') {
            before_cursor.len()
        } else {
            0
        }
    } else {
        0
    };

    NewlineInsert {
        insert_text,
        insert_len,
        trailing_strip_len: stripped_len,
        leading_strip_len,
    }
}

/// Expand tab characters in a raw indent string to spaces.
///
/// Pure: `(raw_indent, tabstop) → String`.
///
/// Delegates to [`crate::commands::helpers::expand_tabs`] — single source of truth.
#[must_use]
pub fn expand_tabs_to_spaces(raw_indent: &str, tabstop: usize) -> String {
    expand_tabs(raw_indent, tabstop)
}

/// Compute the number of spaces to insert for a Tab key press.
///
/// Pure: `(text, cursor, tabstop) → spaces_needed`.
/// Calculates distance to next tabstop based on column position.
#[must_use]
pub fn compute_tab_spaces(text: &str, cursor: usize, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1); // guard against zero
    let line_start = line_start_for_offset(text, cursor);
    let line_slice = &text[line_start..cursor];
    let vcol = crate::commands::helpers::byte_to_vcol(line_slice, line_slice.len(), tabstop);
    tabstop - (vcol % tabstop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_newline_insert_basic() {
        let text = "    hello world";
        let cursor = 15; // End of "    hello world"
        let result = compute_newline_insert(text, cursor, 4, true, None);
        assert_eq!(result.insert_text, "\n    ");
        assert_eq!(result.trailing_strip_len, 0);
    }

    #[test]
    fn test_newline_insert_with_trailing_spaces() {
        let text = "hello   rest";
        let cursor = 5; // After "hello"
        let result = compute_newline_insert(text, cursor, 4, true, None);
        assert_eq!(result.insert_text, "\n");
        assert_eq!(result.trailing_strip_len, 3); // "   " after cursor
    }

    #[test]
    fn test_expand_tabs() {
        assert_eq!(expand_tabs_to_spaces("\t", 4), "    ");
        assert_eq!(expand_tabs_to_spaces("  \t", 4), "    "); // 2 spaces + tab fills to 4
        assert_eq!(expand_tabs_to_spaces("\t\t", 4), "        ");
    }

    #[test]
    fn test_expand_tabs_custom_tabstop() {
        assert_eq!(expand_tabs_to_spaces("\t", 8), "        ");
        assert_eq!(expand_tabs_to_spaces("\t", 2), "  ");
        assert_eq!(expand_tabs_to_spaces("  \t", 8), "        "); // 2 spaces + tab fills to 8
    }

    #[test]
    fn test_tab_spaces_at_col_0() {
        let text = "hello";
        assert_eq!(compute_tab_spaces(text, 0, 4), 4);
    }

    #[test]
    fn test_tab_spaces_at_col_2() {
        let text = "hello";
        assert_eq!(compute_tab_spaces(text, 2, 4), 2);
    }

    #[test]
    fn test_tab_spaces_at_col_4() {
        let text = "hello";
        assert_eq!(compute_tab_spaces(text, 4, 4), 4);
    }

    #[test]
    fn test_tab_spaces_custom_tabstop() {
        let text = "hello";
        assert_eq!(compute_tab_spaces(text, 0, 8), 8);
        assert_eq!(compute_tab_spaces(text, 3, 8), 5);
    }

    // ── IndentProvider tests ─────────────────────────────────────────

    struct MockIndentProvider {
        indent: compact_str::CompactString,
    }

    impl MockIndentProvider {
        fn new(indent: &str) -> Self {
            Self {
                indent: compact_str::CompactString::from(indent),
            }
        }
    }

    impl IndentProvider for MockIndentProvider {
        fn indent_for_new_line(&self, _line: LineNumber) -> crate::document::IndentResult {
            crate::document::IndentResult::Simple {
                indent: self.indent.clone(),
                append: None,
            }
        }
    }

    #[test]
    fn test_newline_insert_with_indent_provider() {
        let text = "if true {";
        let cursor = 9; // End of "if true {"
        let provider = MockIndentProvider::new("        "); // 8 spaces (extra indent)
        let result = compute_newline_insert(text, cursor, 4, true, Some(&provider));
        assert_eq!(result.insert_text, "\n        ");
        assert_eq!(result.trailing_strip_len, 0);
    }

    #[test]
    fn test_newline_insert_provider_overrides_fallback() {
        let text = "    hello";
        let cursor = 9; // End of "    hello"
                        // Provider returns different indent than what fallback would compute
        let provider = MockIndentProvider::new("            "); // 12 spaces
        let result = compute_newline_insert(text, cursor, 4, true, Some(&provider));
        assert_eq!(result.insert_text, "\n            ");
    }

    #[test]
    fn test_newline_insert_none_provider_uses_fallback() {
        let text = "    hello";
        let cursor = 9;
        let result = compute_newline_insert(text, cursor, 4, true, None);
        // Fallback: copies 4 spaces from current line
        assert_eq!(result.insert_text, "\n    ");
    }

    #[test]
    fn test_newline_insert_provider_with_trailing_whitespace_strip() {
        let text = "func() {   rest";
        let cursor = 8; // After "func() {"
        let provider = MockIndentProvider::new("    "); // 4 spaces
        let result = compute_newline_insert(text, cursor, 4, true, Some(&provider));
        assert_eq!(result.insert_text, "\n    ");
        assert_eq!(result.trailing_strip_len, 3); // "   " after cursor
    }

    #[test]
    fn test_newline_insert_noautoindent_no_provider() {
        let text = "    hello world";
        let cursor = 15;
        let result = compute_newline_insert(text, cursor, 4, false, None);
        // With autoindent=false and no provider, no indent should be added
        assert_eq!(result.insert_text, "\n");
        assert_eq!(result.trailing_strip_len, 0);
    }
}
