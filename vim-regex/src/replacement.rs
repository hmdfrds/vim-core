//! Vim replacement string processor.
//!
//! Implements Vim's `:substitute` replacement syntax, including
//! back-references (`\1`..`\9`), whole match (`&`/`\0`), case
//! transforms (`\u`, `\U`, `\l`, `\L`, `\e`, `\E`), and special
//! escape sequences (`\r`, `\n`, `\t`).

use super::ir::{VimRegexError, VimRegexErrorKind};
use super::VimMatch;

// ═══════════════════════════════════════════════════════════════════════════════
// CASE TRANSFORM STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Tracks the active case transformation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaseTransform {
    /// No active transformation.
    None,
    /// `\u` — uppercase the next character only.
    NextUpper,
    /// `\l` — lowercase the next character only.
    NextLower,
    /// `\U` — uppercase all characters until `\e`/`\E`.
    AllUpper,
    /// `\L` — lowercase all characters until `\e`/`\E`.
    AllLower,
}

/// Combined case state: the active transform plus the enclosing block
/// transform to restore to after a `NextUpper`/`NextLower` fires.
#[derive(Debug, Clone, Copy)]
struct CaseState {
    /// Current active transform.
    active: CaseTransform,
    /// Enclosing block transform (`AllUpper`/`AllLower`/`None`) to restore
    /// after `NextUpper`/`NextLower` fires on one character.
    restore_to: CaseTransform,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PUBLIC API
// ═══════════════════════════════════════════════════════════════════════════════

/// Apply a Vim replacement string to a match result.
///
/// Processes the replacement string character-by-character, expanding
/// back-references, special sequences, and case transforms.
///
/// # Arguments
///
/// * `text` — the full text that was searched
/// * `m` — the match result from `VimRegex`
/// * `replacement` — the Vim replacement string (e.g., `\U&\e`)
/// * `previous_replacement` — the previous substitute string (for `~`)
///
/// # Errors
///
/// Returns `VimRegexErrorKind::ExpressionReplacementNotSupported.into()` if `\=` is encountered.
pub fn apply_replacement(
    text: &str,
    m: &VimMatch,
    replacement: &str,
    previous_replacement: Option<&str>,
) -> Result<String, VimRegexError> {
    let mut result = String::with_capacity(replacement.len() + 16);
    let mut cs = CaseState {
        active: CaseTransform::None,
        restore_to: CaseTransform::None,
    };
    let mut chars = replacement.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '&' => {
                push_with_case(&mut result, &mut cs, match_text(text, &m.range));
            }
            '~' => {
                if let Some(prev) = previous_replacement {
                    push_with_case(&mut result, &mut cs, prev);
                }
            }
            '\\' => {
                process_backslash_escape(
                    &mut chars,
                    &mut result,
                    &mut cs,
                    text,
                    m,
                    previous_replacement,
                )?;
            }
            _ => push_char_with_case(&mut result, &mut cs, ch),
        }
    }

    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// EXPRESSION REPLACEMENT EVALUATOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for evaluating Vim expressions in `\=expr` replacement strings.
///
/// Implemented by vim-core's expression engine to evaluate arbitrary
/// Vim script expressions during substitute replacement.
pub trait ReplacementEvaluator {
    /// Evaluate the expression and return the replacement string.
    ///
    /// # Arguments
    /// * `expr` - The expression text after `\=` (e.g., `submatch(0)`)
    /// * `match_result` - The current match being replaced
    /// * `text` - The full text being searched
    fn evaluate_expression(&self, expr: &str, match_result: &VimMatch, text: &str) -> String;
}

/// Apply a Vim replacement string with optional expression evaluation.
///
/// Like [`apply_replacement`], but when `\=` is encountered and an
/// `evaluator` is provided, calls the evaluator instead of returning an error.
///
/// # Arguments
///
/// * `text` - the full text that was searched
/// * `m` - the match result from `VimRegex`
/// * `replacement` - the Vim replacement string
/// * `previous_replacement` - the previous substitute string (for `~`)
/// * `evaluator` - optional expression evaluator for `\=`
///
/// # Errors
///
/// Returns `VimRegexErrorKind::ExpressionReplacementNotSupported.into()` if `\=` is
/// encountered and `evaluator` is `None`.
pub fn apply_replacement_with_evaluator(
    text: &str,
    m: &VimMatch,
    replacement: &str,
    previous_replacement: Option<&str>,
    evaluator: Option<&dyn ReplacementEvaluator>,
) -> Result<String, VimRegexError> {
    // Check if this is an expression replacement (\= at start of replacement).
    // In Vim, \= must appear at the very start of the replacement string.
    if let Some(expr) = replacement.strip_prefix("\\=") {
        match evaluator {
            Some(eval) => Ok(eval.evaluate_expression(expr, m, text)),
            None => Err(VimRegexErrorKind::ExpressionReplacementNotSupported.into()),
        }
    } else {
        // Delegate to the existing non-expression replacement logic
        apply_replacement(text, m, replacement, previous_replacement)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKSLASH ESCAPE PROCESSING
// ═══════════════════════════════════════════════════════════════════════════════

/// Process a backslash escape sequence in the replacement string.
fn process_backslash_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    result: &mut String,
    cs: &mut CaseState,
    text: &str,
    m: &VimMatch,
    // Reserved for future `~` expansion inside \-escapes; currently only
    // the top-level `~` in `apply_replacement` uses `previous_replacement`.
    _prev: Option<&str>,
) -> Result<(), VimRegexError> {
    let next = chars.peek().copied();
    match next {
        Some(c @ '0'..='9') => {
            chars.next();
            let group = (c as u8 - b'0') as usize;
            let s = if group == 0 {
                match_text(text, &m.range)
            } else {
                capture_group_text(text, m, group)
            };
            push_with_case(result, cs, s);
        }
        Some('r') => {
            chars.next();
            push_char_with_case(result, cs, '\n');
        }
        Some('n') => {
            chars.next();
            push_char_with_case(result, cs, '\0');
        }
        Some('t') => {
            chars.next();
            push_char_with_case(result, cs, '\t');
        }
        Some('u') => {
            chars.next();
            set_next_upper(cs);
        }
        Some('U') => {
            chars.next();
            set_block_upper(cs);
        }
        Some('l') => {
            chars.next();
            set_next_lower(cs);
        }
        Some('L') => {
            chars.next();
            set_block_lower(cs);
        }
        Some('e' | 'E') => {
            chars.next();
            reset_case(cs);
        }
        Some('\\') => {
            chars.next();
            push_char_with_case(result, cs, '\\');
        }
        Some('&') => {
            chars.next();
            push_char_with_case(result, cs, '&');
        }
        Some('~') => {
            chars.next();
            push_char_with_case(result, cs, '~');
        }
        Some('b') => {
            chars.next();
            push_char_with_case(result, cs, '\x08');
        }
        Some('=') => return Err(VimRegexErrorKind::ExpressionReplacementNotSupported.into()),
        _ => push_char_with_case(result, cs, '\\'),
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// CASE TRANSFORM HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Push a single character with the current case transform applied.
fn push_char_with_case(result: &mut String, cs: &mut CaseState, ch: char) {
    match cs.active {
        CaseTransform::None => result.push(ch),
        CaseTransform::NextUpper => {
            for c in ch.to_uppercase() {
                result.push(c);
            }
            cs.active = cs.restore_to;
        }
        CaseTransform::NextLower => {
            for c in ch.to_lowercase() {
                result.push(c);
            }
            cs.active = cs.restore_to;
        }
        CaseTransform::AllUpper => {
            for c in ch.to_uppercase() {
                result.push(c);
            }
        }
        CaseTransform::AllLower => {
            for c in ch.to_lowercase() {
                result.push(c);
            }
        }
    }
}

/// Push a string with the current case transform applied to each character.
fn push_with_case(result: &mut String, cs: &mut CaseState, s: &str) {
    for ch in s.chars() {
        push_char_with_case(result, cs, ch);
    }
}

/// `\u` — next char uppercase, then revert to enclosing block transform.
const fn set_next_upper(cs: &mut CaseState) {
    cs.restore_to = enclosing_block(cs.active, cs.restore_to);
    cs.active = CaseTransform::NextUpper;
}

/// `\l` — next char lowercase, then revert to enclosing block transform.
const fn set_next_lower(cs: &mut CaseState) {
    cs.restore_to = enclosing_block(cs.active, cs.restore_to);
    cs.active = CaseTransform::NextLower;
}

/// `\U` — begin all-uppercase block.
const fn set_block_upper(cs: &mut CaseState) {
    cs.active = CaseTransform::AllUpper;
    cs.restore_to = CaseTransform::AllUpper;
}

/// `\L` — begin all-lowercase block.
const fn set_block_lower(cs: &mut CaseState) {
    cs.active = CaseTransform::AllLower;
    cs.restore_to = CaseTransform::AllLower;
}

/// `\e`/`\E` — end case transform block.
const fn reset_case(cs: &mut CaseState) {
    cs.active = CaseTransform::None;
    cs.restore_to = CaseTransform::None;
}

/// Determine the enclosing block to restore to. If the active transform
/// is already a block (`AllUpper`/`AllLower`), that is the enclosing.
/// Otherwise, preserve the existing `restore_to`.
const fn enclosing_block(active: CaseTransform, current_restore: CaseTransform) -> CaseTransform {
    match active {
        CaseTransform::AllUpper | CaseTransform::AllLower => active,
        _ => current_restore,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT EXTRACTION HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract match text from the full text using the match range.
///
/// The regex engine guarantees ranges are in-bounds and on char boundaries.
/// `debug_assert!` validates this in debug builds; `get()` with fallback
/// prevents panics in release if the invariant is ever violated.
fn match_text<'a>(text: &'a str, range: &std::ops::Range<usize>) -> &'a str {
    debug_assert!(
        range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end),
        "match range {range:?} is out of bounds or not on a char boundary for text of len {}",
        text.len()
    );
    text.get(range.start..range.end).unwrap_or("")
}

/// Extract capture group text. Returns `""` if the group is unset or out of range.
fn capture_group_text<'a>(text: &'a str, m: &VimMatch, group: usize) -> &'a str {
    let idx = group.saturating_sub(1);
    m.captures
        .get(idx)
        .and_then(|opt| opt.as_ref())
        .and_then(|range| text.get(range.start..range.end))
        .unwrap_or("")
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matchers::MatchContext;
    use crate::VimRegex;
    use smallvec::SmallVec;

    /// Helper: create a VimMatch for the first match of `pattern` in `text`.
    fn find_match(text: &str, pattern: &str) -> VimMatch {
        let re = VimRegex::new(pattern).unwrap();
        let ctx = MatchContext::simple(text);
        re.find(&ctx)
            .expect("search should not error")
            .expect("pattern should match")
    }

    #[test]
    fn ampersand_whole_match() {
        let text = "hello world";
        let m = find_match(text, "world");
        let result = apply_replacement(text, &m, "[&]", None).unwrap();
        assert_eq!(result, "[world]");
    }

    #[test]
    fn backslash_zero_whole_match() {
        let text = "hello world";
        let m = find_match(text, "world");
        let result = apply_replacement(text, &m, "[\\0]", None).unwrap();
        assert_eq!(result, "[world]");
    }

    #[test]
    fn capture_group_one() {
        let text = "foo123bar";
        // \(\d\+\) captures one or more digits
        let re = VimRegex::new("\\(\\d\\+\\)").unwrap();
        let ctx = MatchContext::simple(text);
        let m = re.find(&ctx).unwrap().unwrap();
        let result = apply_replacement(text, &m, "num=\\1", None).unwrap();
        assert_eq!(result, "num=123");
    }

    #[test]
    fn tilde_previous_replacement() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "~!", Some("old")).unwrap();
        assert_eq!(result, "old!");
    }

    #[test]
    fn tilde_no_previous_replacement() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "~!", None).unwrap();
        assert_eq!(result, "!");
    }

    #[test]
    fn backslash_r_is_newline() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "a\\rb", None).unwrap();
        assert_eq!(result, "a\nb");
    }

    #[test]
    fn backslash_n_is_null() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "a\\nb", None).unwrap();
        assert_eq!(result, "a\0b");
    }

    #[test]
    fn backslash_t_is_tab() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "a\\tb", None).unwrap();
        assert_eq!(result, "a\tb");
    }

    #[test]
    fn backslash_u_next_char_uppercase() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "\\uhello", None).unwrap();
        assert_eq!(result, "Hello");
    }

    #[test]
    fn backslash_upper_u_block_uppercase() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "\\Uhello\\e world", None).unwrap();
        assert_eq!(result, "HELLO world");
    }

    #[test]
    fn backslash_l_next_char_lowercase() {
        let text = "HELLO";
        let m = find_match(text, "HELLO");
        let result = apply_replacement(text, &m, "\\lHELLO", None).unwrap();
        assert_eq!(result, "hELLO");
    }

    #[test]
    fn backslash_upper_l_block_lowercase() {
        let text = "HELLO";
        let m = find_match(text, "HELLO");
        let result = apply_replacement(text, &m, "\\LHELLO\\E world", None).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn escaped_backslash() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "a\\\\b", None).unwrap();
        assert_eq!(result, "a\\b");
    }

    #[test]
    fn escaped_ampersand() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "a\\&b", None).unwrap();
        assert_eq!(result, "a&b");
    }

    #[test]
    fn expression_replacement_error() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "\\=submatch(0)", None);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err().kind,
            VimRegexErrorKind::ExpressionReplacementNotSupported
        ));
    }

    #[test]
    fn case_transform_on_ampersand() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "\\U&\\e", None).unwrap();
        assert_eq!(result, "HELLO");
    }

    #[test]
    fn unset_capture_group_yields_empty() {
        // Create a VimMatch with no captures set
        let m = VimMatch::new(0..5, 0..5, SmallVec::new());
        let result = apply_replacement("hello", &m, "\\1", None).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn literal_text_passthrough() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "world", None).unwrap();
        assert_eq!(result, "world");
    }

    #[test]
    fn uppercase_e_ends_transform() {
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "\\Uab\\Ecd", None).unwrap();
        assert_eq!(result, "ABcd");
    }

    // ── Nested \u/\l within \U/\L block transforms ────────────────────

    #[test]
    fn nested_lower_in_upper_block() {
        // \U\lfoo\E → first char lowercase, rest AllUpper → "fOO"
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "\\U\\lfoo\\E", None).unwrap();
        assert_eq!(result, "fOO");
    }

    #[test]
    fn nested_upper_in_lower_block() {
        // \L\uFOO\E → first char uppercase, rest AllLower → "Foo"
        let text = "hello";
        let m = find_match(text, "hello");
        let result = apply_replacement(text, &m, "\\L\\uFOO\\E", None).unwrap();
        assert_eq!(result, "Foo");
    }

    #[test]
    fn nested_lower_mid_upper_block() {
        // \Uhello \lworld\E → "HELLO wORLD"
        let text = "x";
        let m = find_match(text, "x");
        let result = apply_replacement(text, &m, "\\Uhello \\lworld\\E", None).unwrap();
        assert_eq!(result, "HELLO wORLD");
    }

    #[test]
    fn nested_upper_mid_lower_block() {
        // \LHELLO \uWORLD\E → "hello World"
        let text = "x";
        let m = find_match(text, "x");
        let result = apply_replacement(text, &m, "\\LHELLO \\uWORLD\\E", None).unwrap();
        assert_eq!(result, "hello World");
    }

    #[test]
    fn escaped_tilde_produces_literal_tilde() {
        let m = find_match("hello world", "hello world");
        let result = apply_replacement("hello world", &m, "\\~", Some("prev")).unwrap();
        assert_eq!(result, "~");
    }

    #[test]
    fn backslash_b_produces_backspace() {
        let m = find_match("test", "test");
        let result = apply_replacement("test", &m, "a\\bb", None).unwrap();
        assert_eq!(result, "a\x08b");
    }

    #[test]
    fn replacement_newline_consumes_case_modifier() {
        let m = find_match("x", "x");
        let result = apply_replacement("x", &m, "\\u\\na", None).unwrap();
        // \u sets next-char-upper, \n produces NUL (consumes the \u), a stays lowercase
        assert_eq!(result, "\0a");
    }

    // ── ReplacementEvaluator tests ───────────────────────────────────────

    struct TestEvaluator;

    impl ReplacementEvaluator for TestEvaluator {
        fn evaluate_expression(&self, expr: &str, _match_result: &VimMatch, _text: &str) -> String {
            format!("EVAL({expr})")
        }
    }

    #[test]
    fn expression_replacement_with_evaluator() {
        let text = "hello world";
        let m = find_match(text, "hello");
        let result = apply_replacement_with_evaluator(
            text,
            &m,
            "\\=submatch(0)",
            None,
            Some(&TestEvaluator),
        );
        assert_eq!(result.unwrap(), "EVAL(submatch(0))");
    }

    #[test]
    fn expression_replacement_without_evaluator_errors() {
        let text = "hello world";
        let m = find_match(text, "hello");
        let result = apply_replacement_with_evaluator(text, &m, "\\=submatch(0)", None, None);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err().kind,
            VimRegexErrorKind::ExpressionReplacementNotSupported
        ));
    }

    #[test]
    fn non_expression_replacement_ignores_evaluator() {
        let text = "hello world";
        let m = find_match(text, "hello");
        // Normal replacement (no \=) should work regardless of evaluator
        let result =
            apply_replacement_with_evaluator(text, &m, "goodbye", None, Some(&TestEvaluator));
        assert_eq!(result.unwrap(), "goodbye");
    }
}
