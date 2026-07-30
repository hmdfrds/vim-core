//! Auto-pair insertion and deletion for bracket/quote characters.
//!
//! Pure functions that compute auto-pair effects without touching state.
//! Called from the insert mode dispatch path when `VimOptions::auto_pairs`
//! is `Some`.

use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{AutoPairs, Offset, Pair, Range};
use compact_str::CompactString;

/// Called when a character is typed in insert mode.
///
/// Returns `Some(CommandResult)` if auto-pairing should handle this keystroke,
/// or `None` if normal insertion should proceed.
///
/// # Rules
///
/// - **Opening bracket typed**: If the next character is whitespace, EOL,
///   a closing bracket, or EOF, insert open+close and place cursor between.
///   If next char is alphanumeric, return `None` (no auto-pair).
/// - **Closing bracket typed**: If the next char IS the closer, overtype
///   (move cursor right, no insert). Otherwise return `None`.
/// - **Same-char pair (quotes)**: Don't open if the previous char is
///   alphanumeric. If the next char is the same quote, overtype.
#[must_use]
pub fn auto_pair_hook(
    text: &str,
    cursor: Offset,
    typed: char,
    pairs: &AutoPairs,
) -> Option<CommandResult> {
    let pos = cursor.get();
    let next_char = text[pos..].chars().next();
    let prev_char = if pos > 0 {
        let mut i = pos - 1;
        while i > 0 && !text.is_char_boundary(i) {
            i -= 1;
        }
        text[i..].chars().next()
    } else {
        None
    };

    // Check if typed char is an opener
    if let Some(pair) = find_pair_by_open(typed, pairs) {
        if pair.is_same_char() {
            return handle_same_char_pair(pos, pair, next_char, prev_char);
        }
        return handle_open_bracket(pos, pair, next_char, pairs);
    }

    // Check if typed char is a closer (and not same-char, which was handled above)
    if let Some(pair) = find_pair_by_close(typed, pairs) {
        if !pair.is_same_char() {
            return handle_close_bracket(pos, pair, next_char);
        }
    }

    None
}

/// Called on backspace in insert mode.
///
/// If the character before the cursor is an opener AND the character after
/// the cursor is the matching closer, delete both characters.
/// Returns `None` if no auto-pair deletion should occur.
#[must_use]
pub fn auto_pair_backspace(text: &str, cursor: Offset, pairs: &AutoPairs) -> Option<CommandResult> {
    let pos = cursor.get();
    if pos == 0 || pos >= text.len() {
        return None;
    }

    let mut prev_start = pos - 1;
    while prev_start > 0 && !text.is_char_boundary(prev_start) {
        prev_start -= 1;
    }
    let prev_char = text[prev_start..].chars().next()?;
    let next_char = text[pos..].chars().next()?;

    for pair in &pairs.pairs {
        if pair.open == prev_char && pair.close == next_char {
            let next_end = pos + next_char.len_utf8();
            return Some(CommandResult::effects_only(
                Effects::new()
                    .delete(Range::new(Offset::new(prev_start), Offset::new(next_end)))
                    .set_cursor(Offset::new(prev_start)),
            ));
        }
    }

    None
}

/// Find a pair whose opener matches the typed character.
fn find_pair_by_open(ch: char, pairs: &AutoPairs) -> Option<Pair> {
    pairs.pairs.iter().find(|p| p.open == ch).copied()
}

/// Find a pair whose closer matches the typed character.
fn find_pair_by_close(ch: char, pairs: &AutoPairs) -> Option<Pair> {
    pairs.pairs.iter().find(|p| p.close == ch).copied()
}

/// Check if a character is any closer in the pairs config.
fn is_closing_bracket(ch: char, pairs: &AutoPairs) -> bool {
    pairs
        .pairs
        .iter()
        .any(|p: &Pair| !p.is_same_char() && p.close == ch)
}

/// Handle typing an opening bracket.
fn handle_open_bracket(
    pos: usize,
    pair: Pair,
    next_char: Option<char>,
    pairs: &AutoPairs,
) -> Option<CommandResult> {
    match next_char {
        None | Some('\n' | '\r') => {}
        Some(c) if c.is_whitespace() => {}
        Some(c) if is_closing_bracket(c, pairs) => {}
        _ => return None,
    }

    let insert_str = format!("{}{}", pair.open, pair.close);
    let cursor_after = pos + pair.open.len_utf8();

    Some(CommandResult::effects_only(
        Effects::new()
            .insert(Offset::new(pos), CompactString::from(insert_str))
            .set_cursor(Offset::new(cursor_after)),
    ))
}

/// Handle typing a closing bracket.
fn handle_close_bracket(pos: usize, pair: Pair, next_char: Option<char>) -> Option<CommandResult> {
    if next_char == Some(pair.close) {
        let cursor_after = pos + pair.close.len_utf8();
        Some(CommandResult::effects_only(
            Effects::new().set_cursor(Offset::new(cursor_after)),
        ))
    } else {
        None
    }
}

/// Handle typing a same-char pair (quotes).
fn handle_same_char_pair(
    pos: usize,
    pair: Pair,
    next_char: Option<char>,
    prev_char: Option<char>,
) -> Option<CommandResult> {
    // Overtype if next char is the same quote
    if next_char == Some(pair.open) {
        let cursor_after = pos + pair.open.len_utf8();
        return Some(CommandResult::effects_only(
            Effects::new().set_cursor(Offset::new(cursor_after)),
        ));
    }

    // Don't auto-pair if previous char is alphanumeric
    if let Some(c) = prev_char {
        if c.is_alphanumeric() {
            return None;
        }
    }

    // Don't auto-pair if next char is alphanumeric
    if let Some(c) = next_char {
        if c.is_alphanumeric() {
            return None;
        }
    }

    let insert_str = format!("{}{}", pair.open, pair.close);
    let cursor_after = pos + pair.open.len_utf8();

    Some(CommandResult::effects_only(
        Effects::new()
            .insert(Offset::new(pos), CompactString::from(insert_str))
            .set_cursor(Offset::new(cursor_after)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_pairs() -> AutoPairs {
        AutoPairs::default()
    }

    #[test]
    fn test_open_paren_at_eof() {
        let pairs = default_pairs();
        let result = auto_pair_hook("hello", Offset::new(5), '(', &pairs);
        assert!(result.is_some(), "should auto-pair at EOF");
        let cr = result.unwrap();
        assert!(!cr.is_empty());
    }

    #[test]
    fn test_open_paren_before_space() {
        let pairs = default_pairs();
        let result = auto_pair_hook("hello world", Offset::new(5), '(', &pairs);
        assert!(result.is_some(), "should auto-pair before whitespace");
    }

    #[test]
    fn test_open_paren_before_newline() {
        let pairs = default_pairs();
        let result = auto_pair_hook("hello\nworld", Offset::new(5), '(', &pairs);
        assert!(result.is_some(), "should auto-pair before newline");
    }

    #[test]
    fn test_open_paren_before_closing_bracket() {
        let pairs = default_pairs();
        let result = auto_pair_hook("()", Offset::new(1), '(', &pairs);
        assert!(result.is_some(), "should auto-pair before closing bracket");
    }

    #[test]
    fn test_open_paren_before_alpha_no_pair() {
        let pairs = default_pairs();
        let result = auto_pair_hook("helloworld", Offset::new(5), '(', &pairs);
        assert!(result.is_none(), "should NOT auto-pair before alphanumeric");
    }

    #[test]
    fn test_close_paren_overtype() {
        let pairs = default_pairs();
        let result = auto_pair_hook("()", Offset::new(1), ')', &pairs);
        assert!(result.is_some(), "should overtype closing paren");
    }

    #[test]
    fn test_close_paren_no_overtype() {
        let pairs = default_pairs();
        let result = auto_pair_hook("(a", Offset::new(1), ')', &pairs);
        assert!(
            result.is_none(),
            "should NOT overtype when next char is not closer"
        );
    }

    #[test]
    fn test_quote_at_start() {
        let pairs = default_pairs();
        // Position 0 of an empty string: no previous char, no next char.
        let result = auto_pair_hook("", Offset::new(0), '"', &pairs);
        assert!(result.is_some(), "should auto-pair quote in empty string");
    }

    #[test]
    fn test_quote_overtype() {
        let pairs = default_pairs();
        let result = auto_pair_hook("\"\"", Offset::new(1), '"', &pairs);
        assert!(result.is_some(), "should overtype closing quote");
    }

    #[test]
    fn test_quote_after_word_no_pair() {
        let pairs = default_pairs();
        let result = auto_pair_hook("hello", Offset::new(5), '"', &pairs);
        assert!(
            result.is_none(),
            "should NOT auto-pair quote after alphanumeric"
        );
    }

    #[test]
    fn test_quote_after_space() {
        let pairs = default_pairs();
        let result = auto_pair_hook("hello ", Offset::new(6), '"', &pairs);
        assert!(result.is_some(), "should auto-pair quote after space");
    }

    #[test]
    fn test_quote_before_alpha_no_pair() {
        let pairs = default_pairs();
        let result = auto_pair_hook("hello", Offset::new(0), '\'', &pairs);
        assert!(
            result.is_none(),
            "should NOT auto-pair quote before alphanumeric"
        );
    }

    #[test]
    fn test_open_bracket_types() {
        let pairs = default_pairs();
        for open in ['[', '{'] {
            let result = auto_pair_hook("", Offset::new(0), open, &pairs);
            assert!(result.is_some(), "should auto-pair {open}");
        }
    }

    #[test]
    fn test_backtick_pair() {
        let pairs = default_pairs();
        let result = auto_pair_hook("hello ", Offset::new(6), '`', &pairs);
        assert!(result.is_some(), "should auto-pair backtick");
    }

    #[test]
    fn test_unrelated_char_no_pair() {
        let pairs = default_pairs();
        let result = auto_pair_hook("hello", Offset::new(5), 'x', &pairs);
        assert!(result.is_none(), "should return None for non-pair char");
    }

    #[test]
    fn test_backspace_deletes_pair() {
        let pairs = default_pairs();
        let result = auto_pair_backspace("()", Offset::new(1), &pairs);
        assert!(result.is_some(), "should delete both chars of pair");
    }

    #[test]
    fn test_backspace_deletes_quote_pair() {
        let pairs = default_pairs();
        let result = auto_pair_backspace("\"\"", Offset::new(1), &pairs);
        assert!(result.is_some(), "should delete both chars of quote pair");
    }

    #[test]
    fn test_backspace_no_pair() {
        let pairs = default_pairs();
        let result = auto_pair_backspace("ab", Offset::new(1), &pairs);
        assert!(result.is_none(), "should NOT delete non-pair chars");
    }

    #[test]
    fn test_backspace_at_start() {
        let pairs = default_pairs();
        let result = auto_pair_backspace("()", Offset::new(0), &pairs);
        assert!(result.is_none(), "should return None at start of document");
    }

    #[test]
    fn test_backspace_at_end() {
        let pairs = default_pairs();
        let result = auto_pair_backspace("()", Offset::new(2), &pairs);
        assert!(result.is_none(), "should return None at end of document");
    }

    #[test]
    fn test_backspace_mismatched_pair() {
        let pairs = default_pairs();
        let result = auto_pair_backspace("(]", Offset::new(1), &pairs);
        assert!(result.is_none(), "should NOT delete mismatched pair");
    }

    #[test]
    fn test_backspace_all_bracket_types() {
        let pairs = default_pairs();
        for pair_str in ["[]", "{}", "''", "``"] {
            let open_len = pair_str.chars().next().map_or(1, |c| c.len_utf8());
            let result = auto_pair_backspace(pair_str, Offset::new(open_len), &pairs);
            assert!(result.is_some(), "should delete pair: {pair_str}");
        }
    }

    #[test]
    fn test_pair_is_same_char() {
        assert!(Pair {
            open: '"',
            close: '"'
        }
        .is_same_char());
        assert!(Pair {
            open: '\'',
            close: '\''
        }
        .is_same_char());
        assert!(!Pair {
            open: '(',
            close: ')'
        }
        .is_same_char());
    }
}
