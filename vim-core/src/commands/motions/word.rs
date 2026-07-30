//! Word motions: w, b, e, ge, W, B, E, gE
//!
//! Word-based cursor movement with proper word/WORD boundary detection.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods. Each motion is a standalone
//! function called directly from dispatch.

use super::types::{MotionContext, MotionResult};
use crate::primitives::{Offset, WordCharSet, WordKind};

// ─────────────────────────────────────────────────────────────────────────────
// Word Motions (lowercase variants)
// ─────────────────────────────────────────────────────────────────────────────

/// `w` - Move to start of next word.
pub fn w(ctx: &MotionContext<'_>) -> MotionResult {
    compute_word_forward(ctx, WordKind::Word)
}

/// `e` - Move to end of current/next word.
pub fn e(ctx: &MotionContext<'_>) -> MotionResult {
    compute_word_end(ctx, WordKind::Word)
}

/// `b` - Move to start of previous word.
pub fn b(ctx: &MotionContext<'_>) -> MotionResult {
    compute_word_backward(ctx, WordKind::Word)
}

/// `ge` - Move to end of previous word.
pub fn ge(ctx: &MotionContext<'_>) -> MotionResult {
    compute_word_end_backward(ctx, WordKind::Word)
}

// ─────────────────────────────────────────────────────────────────────────────
// WORD Motions (uppercase/WORD variants)
// ─────────────────────────────────────────────────────────────────────────────

/// `W` - Move to start of next WORD.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn W(ctx: &MotionContext<'_>) -> MotionResult {
    compute_word_forward(ctx, WordKind::WORD)
}

/// `E` - Move to end of current/next WORD.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn E(ctx: &MotionContext<'_>) -> MotionResult {
    compute_word_end(ctx, WordKind::WORD)
}

/// `B` - Move to start of previous WORD.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn B(ctx: &MotionContext<'_>) -> MotionResult {
    compute_word_backward(ctx, WordKind::WORD)
}

/// `gE` - Move to end of previous WORD.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn gE(ctx: &MotionContext<'_>) -> MotionResult {
    compute_word_end_backward(ctx, WordKind::WORD)
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal Implementations
// ─────────────────────────────────────────────────────────────────────────────

fn compute_word_forward(ctx: &MotionContext<'_>, kind: WordKind) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Position(Offset::new(0));
    }

    let wc_at = |pos| ctx.word_char_set_at(pos);
    let mut pos = ctx.cursor.get();
    for _ in 0..ctx.count {
        pos = find_next_word_start(text, pos, kind, &wc_at);
        if pos >= text.len() {
            // In visual/operator mode (inclusive_end), allow cursor at text.len()
            // as an exclusive endpoint. In normal mode, clamp to last valid pos.
            if ctx.inclusive_end {
                pos = text.len();
            } else {
                pos = max_normal_cursor(text);
            }
            break;
        }
    }

    let max_pos = if ctx.inclusive_end {
        text.len()
    } else {
        max_normal_cursor(text)
    };
    let mut result = pos.min(max_pos);
    // Fold-aware: if result lands inside a closed fold, snap forward.
    if let Some(fold) = ctx.providers.fold {
        result = crate::commands::helpers::fold_snap(
            text,
            result,
            crate::primitives::Direction::Forward,
            fold,
        );
        result = result.min(max_pos);
    }
    MotionResult::Position(Offset::new(result))
}

fn compute_word_end(ctx: &MotionContext<'_>, kind: WordKind) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Position(Offset::new(0));
    }

    let wc_at = |pos| ctx.word_char_set_at(pos);
    let mut pos = ctx.cursor.get();
    for _ in 0..ctx.count {
        let new_pos = find_word_end(text, pos, kind, &wc_at);
        if new_pos == pos && new_pos < max_normal_cursor(text) {
            // `e` couldn't find a word end (only whitespace/newlines ahead).
            // Vim still advances to the end of the buffer in this case.
            pos = max_normal_cursor(text);
            break;
        }
        pos = new_pos;
        if pos >= text.len() {
            break;
        }
    }

    let mut result = pos.min(max_normal_cursor(text));
    // Fold-aware: if result lands inside a closed fold, snap forward.
    if let Some(fold) = ctx.providers.fold {
        result = crate::commands::helpers::fold_snap(
            text,
            result,
            crate::primitives::Direction::Forward,
            fold,
        );
        result = result.min(max_normal_cursor(text));
    }
    MotionResult::Position(Offset::new(result))
}

fn compute_word_backward(ctx: &MotionContext<'_>, kind: WordKind) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Error;
    }

    let wc_at = |pos| ctx.word_char_set_at(pos);
    let start = ctx.cursor.get();
    let mut pos = start;
    for _ in 0..ctx.count {
        pos = find_prev_word_start(text, pos, kind, &wc_at);
    }

    // In Neovim, `b` at the start of the buffer is an error (beep).
    // Returning Error when the motion cannot move ensures that operator-
    // pending commands like `cb` are cancelled instead of acting on an
    // empty range.
    if pos == start {
        return MotionResult::Error;
    }

    // Fold-aware: if result lands inside a closed fold, snap backward.
    if let Some(fold) = ctx.providers.fold {
        pos = crate::commands::helpers::fold_snap(
            text,
            pos,
            crate::primitives::Direction::Backward,
            fold,
        );
    }
    MotionResult::Position(Offset::new(pos))
}

fn compute_word_end_backward(ctx: &MotionContext<'_>, kind: WordKind) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Position(Offset::new(0));
    }

    let wc_at = |pos| ctx.word_char_set_at(pos);
    let mut pos = ctx.cursor.get();
    for _ in 0..ctx.count {
        let new_pos = find_prev_word_end(text, pos, kind, &wc_at);
        if new_pos == pos {
            break;
        }
        pos = new_pos;
    }

    // Fold-aware: if result lands inside a closed fold, snap backward.
    if let Some(fold) = ctx.providers.fold {
        pos = crate::commands::helpers::fold_snap(
            text,
            pos,
            crate::primitives::Direction::Backward,
            fold,
        );
    }
    MotionResult::Position(Offset::new(pos))
}

// Character Classification — uses canonical CharClass from textobjects::helpers
// ─────────────────────────────────────────────────────────────────────────────

use crate::commands::helpers::{char_at, next_char_boundary, prev_char_boundary, CharClass};

/// Maximum valid cursor position in normal mode.
///
/// When text ends with `\n`, `text.len()` is valid — it represents the start
/// of the trailing empty line. Otherwise the last character position is used.
fn max_normal_cursor(text: &str) -> usize {
    if text.ends_with('\n') {
        text.len()
    } else {
        prev_char_boundary(text, text.len())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Word Motion Core Algorithms
// ─────────────────────────────────────────────────────────────────────────────

/// Find the start of the next word.
///
/// `word_chars_at` returns the `WordCharSet` for a given byte offset,
/// enabling position-dependent classification.
fn find_next_word_start<'a>(
    text: &str,
    start: usize,
    kind: WordKind,
    word_chars_at: &'a dyn Fn(usize) -> &'a WordCharSet,
) -> usize {
    if start >= text.len() {
        return text.len();
    }

    let starting_class =
        char_at(text, start).map(|c| CharClass::classify(c, kind, word_chars_at(start)));
    let mut pos = start;

    // Step 1: Move forward one character
    pos = next_char_boundary(text, pos);

    // Step 2: Skip characters of the same class
    if let Some(class) = starting_class {
        if class != CharClass::Whitespace {
            while pos < text.len() {
                if let Some(c) = char_at(text, pos) {
                    if CharClass::classify(c, kind, word_chars_at(pos)) != class {
                        break;
                    }
                    pos = next_char_boundary(text, pos);
                } else {
                    break;
                }
            }
        }
    }

    // Step 3: Skip whitespace, but stop at empty lines (Vim behavior).
    // Uses CharClass for whitespace detection so that Neovim-blank characters
    // like ZWSP (U+200B) are treated as whitespace consistently.
    // An empty line is a \n that starts a blank line — detected by:
    //   (a) current \n is followed by another \n (next line is empty), or
    //   (b) current \n is preceded by another \n or is at text start (this line is empty)
    while pos < text.len() {
        if let Some(c) = char_at(text, pos) {
            if CharClass::classify(c, kind, word_chars_at(pos)) != CharClass::Whitespace {
                break;
            }
            if c == '\n' {
                // Check if this \n represents an empty line:
                // Either the next char is \n (forward empty line detection)
                // or the previous char is \n / we're at start (this IS an empty line)
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

/// Find the end of the current/next word.
fn find_word_end<'a>(
    text: &str,
    start: usize,
    kind: WordKind,
    word_chars_at: &'a dyn Fn(usize) -> &'a WordCharSet,
) -> usize {
    if start >= text.len() {
        return start;
    }

    let mut pos = start;

    // Step 1: Move forward one character
    if pos < text.len() {
        pos = next_char_boundary(text, pos);
    }

    // Step 2: Skip whitespace (using CharClass for Neovim-consistent classification)
    while pos < text.len() {
        if let Some(c) = char_at(text, pos) {
            if CharClass::classify(c, kind, word_chars_at(pos)) != CharClass::Whitespace {
                break;
            }
            pos = next_char_boundary(text, pos);
        } else {
            break;
        }
    }

    if pos >= text.len() {
        return prev_char_boundary(text, text.len());
    }

    // Step 3: Get current word class and skip until it changes
    let word_class = char_at(text, pos).map(|c| CharClass::classify(c, kind, word_chars_at(pos)));
    let mut last_pos = pos;

    while pos < text.len() {
        if let Some(c) = char_at(text, pos) {
            if Some(CharClass::classify(c, kind, word_chars_at(pos))) != word_class {
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

/// Find the start of the previous word.
fn find_prev_word_start<'a>(
    text: &str,
    start: usize,
    kind: WordKind,
    word_chars_at: &'a dyn Fn(usize) -> &'a WordCharSet,
) -> usize {
    if start == 0 || text.is_empty() {
        return 0;
    }

    let mut pos = start;

    // Step 1: Move backward one character
    pos = prev_char_boundary(text, pos);

    // Step 2: Skip whitespace backward, but stop at empty lines.
    // Uses CharClass for whitespace detection (Neovim-consistent).
    // An empty line (a \n whose preceding char is also \n, or \n at pos 0)
    // acts as a word boundary for `b` — Vim treats it as its own "word".
    while pos > 0 {
        if let Some(c) = char_at(text, pos) {
            if CharClass::classify(c, kind, word_chars_at(pos)) != CharClass::Whitespace {
                break;
            }
            // Check for empty line: if current char is \n and previous is also \n
            // (or we are at the start of text), this is an empty line boundary.
            if c == '\n' {
                let prev = prev_char_boundary(text, pos);
                if prev == 0 || char_at(text, prev) == Some('\n') {
                    // Stop here — the empty line is the "word" target.
                    return pos;
                }
            }
            pos = prev_char_boundary(text, pos);
        } else {
            break;
        }
    }

    // Handle edge case: all whitespace before cursor
    if let Some(c) = char_at(text, pos) {
        if CharClass::classify(c, kind, word_chars_at(pos)) == CharClass::Whitespace {
            return 0;
        }

        // Step 3: Get current word class and skip backward
        let word_class = CharClass::classify(c, kind, word_chars_at(pos));
        while pos > 0 {
            let prev = prev_char_boundary(text, pos);
            if let Some(prev_c) = char_at(text, prev) {
                if CharClass::classify(prev_c, kind, word_chars_at(prev)) != word_class {
                    break;
                }
                pos = prev;
            } else {
                break;
            }
        }
    }

    pos
}

/// Find the end of the previous word (ge/gE motion).
fn find_prev_word_end<'a>(
    text: &str,
    start: usize,
    kind: WordKind,
    word_chars_at: &'a dyn Fn(usize) -> &'a WordCharSet,
) -> usize {
    if start == 0 || text.is_empty() {
        return 0;
    }

    let mut pos = start;
    let starting_class =
        char_at(text, pos).map(|c| CharClass::classify(c, kind, word_chars_at(pos)));

    // Step 1: Move backward one character
    pos = prev_char_boundary(text, pos);

    // Step 2: If we started on a word, skip backward past same class
    if let Some(class) = starting_class {
        if class != CharClass::Whitespace {
            while pos > 0 {
                if let Some(c) = char_at(text, pos) {
                    if CharClass::classify(c, kind, word_chars_at(pos)) != class {
                        break;
                    }
                    pos = prev_char_boundary(text, pos);
                } else {
                    break;
                }
            }
        }
    }

    // Step 3: Skip whitespace backward, stopping at empty lines.
    // Uses CharClass for whitespace detection (Neovim-consistent).
    // An empty line (\n preceded by \n or at text start) acts as a word boundary
    // for ge — Vim treats it as its own "word end".
    while pos > 0 {
        if let Some(c) = char_at(text, pos) {
            if CharClass::classify(c, kind, word_chars_at(pos)) != CharClass::Whitespace {
                break;
            }
            // Check for empty line: if current char is \n and previous is also \n
            // (or we are at the start of text), this is an empty line boundary.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn default_wc() -> WordCharSet {
        WordCharSet::default_vim()
    }

    #[test]
    fn test_find_next_word_start() {
        let wc = default_wc();
        let wc_at = |_| &wc;
        assert_eq!(
            find_next_word_start("hello world", 0, WordKind::Word, &wc_at),
            6
        );
        assert_eq!(
            find_next_word_start("hello world", 5, WordKind::Word, &wc_at),
            6
        );
        assert_eq!(
            find_next_word_start("hello  world", 0, WordKind::Word, &wc_at),
            7
        );
    }

    #[test]
    fn test_find_word_end() {
        let wc = default_wc();
        let wc_at = |_| &wc;
        assert_eq!(find_word_end("hello world", 0, WordKind::Word, &wc_at), 4);
        assert_eq!(find_word_end("hello world", 4, WordKind::Word, &wc_at), 10);
    }

    #[test]
    fn test_find_prev_word_start() {
        let wc = default_wc();
        let wc_at = |_| &wc;
        assert_eq!(
            find_prev_word_start("hello world", 6, WordKind::Word, &wc_at),
            0
        );
        assert_eq!(
            find_prev_word_start("hello world", 10, WordKind::Word, &wc_at),
            6
        );
    }

    #[test]
    fn test_find_prev_word_end() {
        let wc = default_wc();
        let wc_at = |_| &wc;
        assert_eq!(
            find_prev_word_end("hello world foo", 12, WordKind::Word, &wc_at),
            10
        );
        assert_eq!(
            find_prev_word_end("hello world foo", 6, WordKind::Word, &wc_at),
            4
        );
    }

    #[test]
    fn test_classify() {
        let wc = default_wc();
        assert_eq!(
            CharClass::classify('a', WordKind::Word, &wc),
            CharClass::Word
        );
        assert_eq!(
            CharClass::classify('_', WordKind::Word, &wc),
            CharClass::Word
        );
        assert_eq!(
            CharClass::classify('.', WordKind::Word, &wc),
            CharClass::Punctuation
        );
        assert_eq!(
            CharClass::classify(' ', WordKind::Word, &wc),
            CharClass::Whitespace
        );
        assert_eq!(
            CharClass::classify('.', WordKind::WORD, &wc),
            CharClass::Word
        ); // WORD mode
           // CJK classification
        assert_eq!(
            CharClass::classify('日', WordKind::Word, &wc),
            CharClass::CjkIdeograph
        );
        assert_eq!(
            CharClass::classify('テ', WordKind::Word, &wc),
            CharClass::Katakana
        );
        assert_eq!(
            CharClass::classify('한', WordKind::Word, &wc),
            CharClass::Hangul
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Integration tests: custom iskeyword changes word motion behavior
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn custom_iskeyword_dash_is_word_char() {
        // With default iskeyword: dash is punctuation, so w stops at it
        let default_wc = default_wc();
        let default_at = |_| &default_wc;
        assert_eq!(
            find_next_word_start("foo-bar baz", 0, WordKind::Word, &default_at),
            3 // stops at '-'
        );

        // With iskeyword including dash (45): dash is a word char
        let dash_wc = WordCharSet::from_iskeyword("@,48-57,_,45");
        let dash_at = |_| &dash_wc;
        assert_eq!(
            find_next_word_start("foo-bar baz", 0, WordKind::Word, &dash_at),
            8 // skips past "foo-bar" to "baz"
        );
    }

    #[test]
    fn custom_iskeyword_dash_word_end() {
        let default_wc = default_wc();
        let default_at = |_| &default_wc;
        // Default: 'e' on "foo-bar" from 0 lands on 'o' (pos 2)
        assert_eq!(find_word_end("foo-bar", 0, WordKind::Word, &default_at), 2);

        // With dash as word char: 'e' on "foo-bar" from 0 lands on 'r' (pos 6)
        let dash_wc = WordCharSet::from_iskeyword("@,48-57,_,45");
        let dash_at = |_| &dash_wc;
        assert_eq!(find_word_end("foo-bar", 0, WordKind::Word, &dash_at), 6);
    }

    #[test]
    fn custom_iskeyword_dash_word_backward() {
        let default_wc = default_wc();
        let default_at = |_| &default_wc;
        // Default: 'b' from end of "foo-bar" (pos 7) goes to 'b' (pos 4)
        assert_eq!(
            find_prev_word_start("foo-bar", 7, WordKind::Word, &default_at),
            4
        );

        // With dash as word char: 'b' from end goes to 'f' (pos 0)
        let dash_wc = WordCharSet::from_iskeyword("@,48-57,_,45");
        let dash_at = |_| &dash_wc;
        assert_eq!(
            find_prev_word_start("foo-bar", 7, WordKind::Word, &dash_at),
            0
        );
    }

    #[test]
    fn custom_iskeyword_dollar_sign() {
        // In Lisp/shell: $ is a word char
        let dollar_wc = WordCharSet::from_iskeyword("@,48-57,_,36");
        assert_eq!(
            CharClass::classify('$', WordKind::Word, &dollar_wc),
            CharClass::Word
        );
        // "let $foo = bar" — w from 4 ($) should skip past "$foo" to " "
        let dollar_at = |_| &dollar_wc;
        assert_eq!(
            find_next_word_start("let $foo = bar", 4, WordKind::Word, &dollar_at),
            9 // lands at "="
        );
    }

    #[test]
    fn classify_respects_iskeyword_for_ascii() {
        // Default: '#' is punctuation
        let default_wc = default_wc();
        assert_eq!(
            CharClass::classify('#', WordKind::Word, &default_wc),
            CharClass::Punctuation
        );

        // With '#' (35) in iskeyword: it becomes Word
        let hash_wc = WordCharSet::from_iskeyword("@,48-57,_,35");
        assert_eq!(
            CharClass::classify('#', WordKind::Word, &hash_wc),
            CharClass::Word
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Position-dependent word character classification (word_char_fn)
    // ─────────────────────────────────────────────────────────────────────────

    use super::super::types::WordCharProvider;
    use crate::primitives::VimOptions;

    /// Test provider: dash is a word char at offsets 0..=10, punctuation after.
    struct SplitDashProvider {
        /// WordCharSet where dash IS a word char.
        dash_wc: WordCharSet,
        /// Default WordCharSet (dash is punctuation).
        default_wc: WordCharSet,
    }

    impl SplitDashProvider {
        fn new() -> Self {
            Self {
                dash_wc: WordCharSet::from_iskeyword("@,48-57,_,45"),
                default_wc: WordCharSet::default_vim(),
            }
        }
    }

    impl WordCharProvider for SplitDashProvider {
        fn word_char_set_at(&self, offset: usize) -> &WordCharSet {
            if offset <= 10 {
                &self.dash_wc
            } else {
                &self.default_wc
            }
        }
    }

    #[test]
    fn word_char_fn_w_motion_dash_is_word_in_first_scope() {
        // Text: "back-ground back-ground"
        //        0123456789012345678901
        //        ^--- offset <=10 ---^^--- offset >10 ---^
        // At offset 0 (within scope), dash is a word char, so `w` from 0
        // should skip "back-ground" entirely and land at 12 (space -> "back").
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground back-ground";
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(12)));
    }

    #[test]
    fn word_char_fn_w_motion_dash_is_punct_in_second_scope() {
        // At offset 16 (within "back-ground" starting at 12), dash is
        // punctuation, so `w` from 12 should stop at the dash (offset 16).
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground back-ground";
        let ctx = MotionContext::new(text, Offset::new(12), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(16)));
    }

    #[test]
    fn word_char_fn_b_motion_position_dependent() {
        // `b` from offset 11 ('d' at end of first "back-ground"):
        // Since offset 11 is in the dash-as-word scope (<=10 boundary),
        // `b` should treat "back-ground" as one word and go to 0.
        //
        // `b` from offset 22 ('d' at end of second "back-ground"):
        // Since offset 22 is in the default scope (>10), dash is punctuation,
        // so `b` should go to the start of "ground" at 17.
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground back-ground";

        let ctx = MotionContext::new(text, Offset::new(11), 1, &opts).with_word_char_fn(&provider);
        let result = b(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));

        let ctx = MotionContext::new(text, Offset::new(22), 1, &opts).with_word_char_fn(&provider);
        let result = b(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(17)));
    }

    #[test]
    fn word_char_fn_e_motion_position_dependent() {
        // `e` from offset 0: dash is word char in this scope, so
        // e should land on the last char of "back-ground" at offset 10.
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground back-ground";

        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(10)));

        // `e` from offset 12: dash is punctuation in this scope,
        // so e should land on the 'k' of "back" at offset 15.
        let ctx = MotionContext::new(text, Offset::new(12), 1, &opts).with_word_char_fn(&provider);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(15)));
    }

    #[test]
    fn word_char_fn_ge_motion_position_dependent() {
        // `ge` from offset 12 (space before second "back-ground"):
        // The previous word end is at offset 10 ('d' of first "back-ground").
        // At offset 10, dash is a word char (<=10), so ge should traverse
        // back through the whole "back-ground" and land at 10.
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground back-ground";

        let ctx = MotionContext::new(text, Offset::new(12), 1, &opts).with_word_char_fn(&provider);
        let result = ge(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(10)));
    }

    #[test]
    fn word_char_fn_none_uses_global() {
        // Without word_char_fn, behavior matches global options (dash = punct).
        let opts = VimOptions::default();
        let text = "back-ground";

        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        assert!(ctx.word_char_fn.is_none());
        let result = w(&ctx);
        // Default: dash is punctuation, so `w` stops at '-' (offset 4).
        assert_eq!(result, MotionResult::Position(Offset::new(4)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // WORD motions (W/B/E/gE) ignore WordCharProvider — whitespace-only
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn word_char_fn_big_w_ignores_provider() {
        // WORD motions treat all non-whitespace as one class regardless of
        // the provider. With provider active, W from 0 on "back-ground next"
        // should still skip "back-ground" (all non-whitespace) and land at 12.
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground next";
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = W(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(12)));
    }

    #[test]
    fn word_char_fn_big_b_ignores_provider() {
        // B from offset 15 ("ext") should skip back over "next" whitespace
        // to "back-ground" start at 0, treating dash as non-whitespace.
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground next";
        let ctx = MotionContext::new(text, Offset::new(15), 1, &opts).with_word_char_fn(&provider);
        let result = B(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(12)));

        // B from 12 should land at 0 (start of "back-ground" as one WORD).
        let ctx = MotionContext::new(text, Offset::new(12), 1, &opts).with_word_char_fn(&provider);
        let result = B(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn word_char_fn_big_e_ignores_provider() {
        // E from 0 on "back-ground next" should land on 'd' at offset 10
        // (end of the WORD "back-ground"), treating dash as non-whitespace.
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground next";
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = E(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(10)));
    }

    #[test]
    fn word_char_fn_big_ge_ignores_provider() {
        // gE from 12 ("next") should land on 'd' at offset 10
        // (end of previous WORD "back-ground").
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "back-ground next";
        let ctx = MotionContext::new(text, Offset::new(12), 1, &opts).with_word_char_fn(&provider);
        let result = gE(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(10)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Word motions with provider at end of line
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn word_char_fn_w_at_end_of_line() {
        // w from the last word char before newline should cross to next line.
        // "foo-bar\nbaz" with dash-as-word in scope <=10:
        //  0123456 7 890
        // w from 0 should skip "foo-bar" (one word) and land at 8 ("baz").
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "foo-bar\nbaz";
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(8)));
    }

    #[test]
    fn word_char_fn_b_at_start_of_line() {
        // b from start of second line back to first line.
        // "foo-bar\nbaz" with dash-as-word in scope <=10:
        // b from 8 ("baz") should land at 0 ("foo-bar" is one word).
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "foo-bar\nbaz";
        let ctx = MotionContext::new(text, Offset::new(8), 1, &opts).with_word_char_fn(&provider);
        let result = b(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn word_char_fn_e_at_end_of_line() {
        // e from 0 on "foo-bar\nbaz" with dash-as-word in scope <=10:
        // Should land on 'r' at offset 6 (end of "foo-bar" as one word).
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "foo-bar\nbaz";
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Provider returning different sets for different lines
    // ─────────────────────────────────────────────────────────────────────────

    /// Provider that switches at a line boundary: line 1 has dash-as-word,
    /// line 2 uses default (dash = punctuation).
    struct PerLineProvider {
        /// Byte offset where line 2 starts.
        line2_start: usize,
        dash_wc: WordCharSet,
        default_wc: WordCharSet,
    }

    impl PerLineProvider {
        fn new(line2_start: usize) -> Self {
            Self {
                line2_start,
                dash_wc: WordCharSet::from_iskeyword("@,48-57,_,45"),
                default_wc: WordCharSet::default_vim(),
            }
        }
    }

    impl WordCharProvider for PerLineProvider {
        fn word_char_set_at(&self, offset: usize) -> &WordCharSet {
            if offset < self.line2_start {
                &self.dash_wc
            } else {
                &self.default_wc
            }
        }
    }

    #[test]
    fn per_line_provider_w_different_behavior_per_line() {
        // Line 1: "foo-bar " (8 bytes, dash is word char)
        // Line 2: "foo-bar"  (7 bytes, dash is punctuation)
        let text = "foo-bar\nfoo-bar";
        //          01234567 89...14
        let provider = PerLineProvider::new(8); // line 2 starts at offset 8

        let opts = VimOptions::default();

        // w from 0 (line 1): dash is word, so skip "foo-bar" -> land at 8.
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(8)));

        // w from 8 (line 2): dash is punctuation, so stop at dash at offset 11.
        let ctx = MotionContext::new(text, Offset::new(8), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(11)));
    }

    #[test]
    fn per_line_provider_b_different_behavior_per_line() {
        let text = "foo-bar\nfoo-bar";
        let provider = PerLineProvider::new(8);
        let opts = VimOptions::default();

        // b from 14 (end of line 2): dash is punct on line 2,
        // so b lands at start of "bar" at offset 12.
        let ctx = MotionContext::new(text, Offset::new(14), 1, &opts).with_word_char_fn(&provider);
        let result = b(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(12)));

        // b from 6 (end of line 1): dash is word on line 1,
        // so b lands at start of "foo-bar" at offset 0.
        let ctx = MotionContext::new(text, Offset::new(6), 1, &opts).with_word_char_fn(&provider);
        let result = b(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn per_line_provider_e_different_behavior_per_line() {
        let text = "foo-bar\nfoo-bar";
        let provider = PerLineProvider::new(8);
        let opts = VimOptions::default();

        // e from 0 (line 1): dash is word, so land on 'r' at offset 6.
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));

        // e from 8 (line 2): dash is punct, so land on 'o' at offset 10.
        let ctx = MotionContext::new(text, Offset::new(8), 1, &opts).with_word_char_fn(&provider);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(10)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CSS/JS mixed-language scenario: position-dependent mid-motion
    // ─────────────────────────────────────────────────────────────────────────

    /// Provider simulating a mixed CSS/JS file where CSS scope has
    /// dash as a word char and JS scope has dash as punctuation.
    ///
    /// Layout: CSS scope is byte offsets [0..css_end), JS scope is [css_end..).
    struct CssJsProvider {
        /// WordCharSet for CSS scope: dash IS a word char.
        css_wc: WordCharSet,
        /// WordCharSet for JS scope: dash is punctuation (default).
        js_wc: WordCharSet,
        /// Byte offset where CSS scope ends and JS scope begins.
        css_end: usize,
    }

    impl CssJsProvider {
        fn new(css_end: usize) -> Self {
            Self {
                css_wc: WordCharSet::from_iskeyword("@,48-57,_,45"),
                js_wc: WordCharSet::default_vim(),
                css_end,
            }
        }
    }

    impl WordCharProvider for CssJsProvider {
        fn word_char_set_at(&self, offset: usize) -> &WordCharSet {
            if offset < self.css_end {
                &self.css_wc
            } else {
                &self.js_wc
            }
        }
    }

    #[test]
    fn css_scope_w_treats_dashed_property_as_one_word() {
        // CSS: "background-color" is one word (dash is word char).
        // `w` from 0 should skip past the entire property.
        let text = "background-color: red;";
        let opts = VimOptions::default();
        let provider = CssJsProvider::new(text.len());
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        // Skips "background-color", lands on ':' (offset 16).
        assert_eq!(result, MotionResult::Position(Offset::new(16)));
    }

    #[test]
    fn js_scope_w_treats_dash_as_punctuation() {
        // JS: "my-variable" — dash is punctuation, so `w` stops at '-'.
        let text = "my-variable = 42;";
        let opts = VimOptions::default();
        let provider = CssJsProvider::new(0); // entire text is JS scope
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        // Stops at '-' (offset 2).
        assert_eq!(result, MotionResult::Position(Offset::new(2)));
    }

    #[test]
    fn css_then_js_w_motion_crosses_scope_boundary() {
        // Mixed file: CSS then JS separated by whitespace.
        // CSS scope: "background-color " (0..18), JS scope: (18..)
        let text = "background-color  my-var";
        //           0         1         2
        //           0123456789012345678901234
        // CSS scope ends at 18. "background-color" is one word.
        let opts = VimOptions::default();
        let provider = CssJsProvider::new(18);

        // `w` from 0 in CSS scope: "background-color" is one word, lands at 18.
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(18)));

        // `w` from 18 in JS scope: "my" is the word (dash=punct), lands at 20.
        let ctx = MotionContext::new(text, Offset::new(18), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(20)));
    }

    #[test]
    fn css_e_lands_on_last_char_of_dashed_word() {
        // `e` from 0 on "background-color" in CSS scope lands on 'r' (offset 15).
        let text = "background-color";
        let opts = VimOptions::default();
        let provider = CssJsProvider::new(text.len());
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(15)));
    }

    #[test]
    fn js_e_stops_before_dash() {
        // `e` from 0 on "my-variable" in JS scope lands on 'y' (offset 1).
        let text = "my-variable";
        let opts = VimOptions::default();
        let provider = CssJsProvider::new(0);
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(1)));
    }

    #[test]
    fn css_b_traverses_entire_dashed_word() {
        // `b` from end of "background-color" in CSS scope goes to 0.
        let text = "background-color";
        let opts = VimOptions::default();
        let provider = CssJsProvider::new(text.len());
        let ctx = MotionContext::new(text, Offset::new(15), 1, &opts).with_word_char_fn(&provider);
        let result = b(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(0)));
    }

    #[test]
    fn js_b_stops_at_dash_boundary() {
        // `b` from end of "my-variable" in JS scope: goes to "variable" start.
        let text = "my-variable";
        let opts = VimOptions::default();
        let provider = CssJsProvider::new(0);
        let ctx = MotionContext::new(text, Offset::new(10), 1, &opts).with_word_char_fn(&provider);
        let result = b(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(3)));
    }

    #[test]
    fn provider_called_per_character_not_per_motion() {
        // This test proves the provider is called at each character position
        // during a single motion, not cached once at the start.
        //
        // Text: "abc-def ghi"
        //        01234567890
        // Scope boundary at offset 4 (between '-' and 'd').
        // Offsets 0..4: dash is word char (CSS-like).
        // Offsets 4..:  dash is punctuation (JS-like).
        //
        // The dash at offset 3 is in CSS scope, so it is Word class.
        // `w` from 0 walks: a(0)=Word, b(1)=Word, c(2)=Word, -(3)=Word [CSS].
        // At d(4) scope switches to JS, but 'd' is alphanumeric, still Word.
        // The entire "abc-def" is one word.
        let text = "abc-def ghi";
        let opts = VimOptions::default();
        let provider = CssJsProvider::new(4);
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(8)));

        // Now move the boundary to offset 3 so the dash itself falls in JS scope.
        let provider2 = CssJsProvider::new(3);
        let ctx2 = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider2);
        let result2 = w(&ctx2);
        // The dash at offset 3 is now JS scope (offset 3 >= 3): punctuation.
        // `w` stops at the dash.
        assert_eq!(result2, MotionResult::Position(Offset::new(3)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Operator+motion composition: `dw` with provider
    //
    // `dispatch_operator_with_motion` builds its own MotionContext via
    // `compute_motion_range_with_sticky`, which currently does NOT propagate
    // word_char_fn. This means operator+motion commands like `dw` always
    // use the global WordCharSet, ignoring any position-dependent provider.
    //
    // These tests document the current behavior (global fallback) so that
    // a future integration can verify the fix without silently regressing.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn dw_uses_global_wordcharset_not_provider() {
        // The `dispatch_operator_with_motion` path builds MotionContext
        // without word_char_fn. Verify that `w` in that code path uses
        // the global options, not the provider.
        //
        // Text: "foo-bar baz"
        // With provider: dash is word char at all offsets.
        // Expected with provider: w from 0 skips "foo-bar" -> offset 8.
        // Expected without provider (global): w from 0 stops at '-' -> offset 3.
        //
        // Since `compute_motion_range_with_sticky` omits word_char_fn, the
        // operator path should behave like the global (offset 3).

        // Direct motion WITH provider (ground truth):
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "foo-bar baz";
        let ctx_with =
            MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result_with = w(&ctx_with);
        // Provider active: dash is word char at offset 0..=10, so w skips "foo-bar".
        assert_eq!(result_with, MotionResult::Position(Offset::new(8)));

        // Direct motion WITHOUT provider (simulating operator path):
        let ctx_without = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result_without = w(&ctx_without);
        // No provider: dash is punctuation (global default), so w stops at '-'.
        assert_eq!(result_without, MotionResult::Position(Offset::new(3)));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Text object: `diw` does NOT use position-dependent provider
    //
    // TextObjectContext holds a single &WordCharSet (not a provider fn),
    // so `diw` uses the same classification for all offsets. This is
    // consistent with Vim (iskeyword is buffer-global, not position-local).
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn diw_uses_global_wordcharset() {
        use crate::commands::textobjects::word::compute_word_object;
        use crate::commands::textobjects::TextObjectContext;
        use crate::grammar::types::TextObjectScope;

        // With default WordCharSet, dash is punctuation.
        // "foo-bar" at cursor 0: iw selects "foo" (0..3).
        let text = "foo-bar";
        let ctx = TextObjectContext::new(text, 0);
        let result = compute_word_object(&ctx, TextObjectScope::Inner, WordKind::Word);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(range.start(), 0);
        assert_eq!(range.end(), 3); // "foo" only, dash is punctuation
    }

    #[test]
    fn diw_with_custom_iskeyword_dash_selects_full_word() {
        use crate::commands::textobjects::word::compute_word_object;
        use crate::commands::textobjects::TextObjectContext;
        use crate::grammar::types::TextObjectScope;

        // With dash in iskeyword, "foo-bar" is one word.
        let text = "foo-bar baz";
        let mut opts = VimOptions::default();
        opts.set_iskeyword("@,48-57,_,45");
        let ctx = TextObjectContext::new(text, 0).with_options(&opts);
        let result = compute_word_object(&ctx, TextObjectScope::Inner, WordKind::Word);
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(range.start(), 0);
        assert_eq!(range.end(), 7); // "foo-bar" as one word
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Provider robustness: inconsistent return values
    //
    // A provider that returns different WordCharSet references for the SAME
    // offset on successive calls could cause word boundary detection to be
    // inconsistent within a single motion. The engine does not guard against
    // this — it trusts the provider to be deterministic. These tests verify
    // that the engine does not panic or loop infinitely even when the
    // provider is non-deterministic.
    // ─────────────────────────────────────────────────────────────────────────

    /// Provider that alternates between two WordCharSets on each call,
    /// simulating a non-deterministic host implementation.
    struct FlipFlopProvider {
        call_count: std::cell::Cell<usize>,
        dash_wc: WordCharSet,
        default_wc: WordCharSet,
    }

    impl FlipFlopProvider {
        fn new() -> Self {
            Self {
                call_count: std::cell::Cell::new(0),
                dash_wc: WordCharSet::from_iskeyword("@,48-57,_,45"),
                default_wc: WordCharSet::default_vim(),
            }
        }
    }

    impl WordCharProvider for FlipFlopProvider {
        fn word_char_set_at(&self, _offset: usize) -> &WordCharSet {
            let count = self.call_count.get();
            self.call_count.set(count + 1);
            if count % 2 == 0 {
                &self.dash_wc
            } else {
                &self.default_wc
            }
        }
    }

    #[test]
    fn inconsistent_provider_does_not_panic_or_loop() {
        // A provider that alternates its classification should not cause
        // the word motion to panic or loop infinitely. The result may be
        // "wrong" (undefined behavior from the provider contract), but
        // the engine must remain safe.
        let opts = VimOptions::default();
        let provider = FlipFlopProvider::new();
        let text = "foo-bar baz-qux";

        // w motion: should terminate without panic regardless of flip-flop.
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = w(&ctx);
        // We don't assert the exact position — just that it terminates and
        // returns a valid offset within bounds.
        match result {
            MotionResult::Position(off) => {
                assert!(off.get() <= text.len());
            }
            _ => {} // Error/NoMotion are also acceptable
        }

        // b motion: same safety check.
        let provider2 = FlipFlopProvider::new();
        let ctx = MotionContext::new(text, Offset::new(10), 1, &opts).with_word_char_fn(&provider2);
        let result = b(&ctx);
        match result {
            MotionResult::Position(off) => {
                assert!(off.get() <= text.len());
            }
            _ => {}
        }

        // e motion: same safety check.
        let provider3 = FlipFlopProvider::new();
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider3);
        let result = e(&ctx);
        match result {
            MotionResult::Position(off) => {
                assert!(off.get() <= text.len());
            }
            _ => {}
        }

        // ge motion: same safety check.
        let provider4 = FlipFlopProvider::new();
        let ctx = MotionContext::new(text, Offset::new(10), 1, &opts).with_word_char_fn(&provider4);
        let result = ge(&ctx);
        match result {
            MotionResult::Position(off) => {
                assert!(off.get() <= text.len());
            }
            _ => {}
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // cw with provider: `cw` has a special Vim behavior (cw -> ce)
    //
    // In Vim, `cw` is equivalent to `ce` (change to end of word, not
    // to start of next word). The special case is in
    // `dispatch_operator_with_motion` via `adjust_change_word_range`.
    // Since operator dispatch doesn't propagate word_char_fn, the
    // adjustment uses global classification. Verify this at the motion
    // level by comparing `e` with and without provider.
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn cw_motion_e_with_provider_dash_is_word() {
        // Simulate the `cw -> ce` behavior: cw from 0 on "foo-bar baz"
        // with dash-as-word provider should land at end of "foo-bar" (offset 6).
        // Without provider (global), cw from 0 lands at end of "foo" (offset 2).
        let opts = VimOptions::default();
        let provider = SplitDashProvider::new();
        let text = "foo-bar baz";

        // With provider: `e` from 0 -> offset 6 (end of "foo-bar")
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts).with_word_char_fn(&provider);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(6)));

        // Without provider: `e` from 0 -> offset 2 (end of "foo")
        let ctx = MotionContext::new(text, Offset::new(0), 1, &opts);
        let result = e(&ctx);
        assert_eq!(result, MotionResult::Position(Offset::new(2)));
    }

    // ── Fold-aware word motion tests ─────────────────────────────────

    mod fold_aware {
        use super::*;
        use crate::document::{FoldProvider, Providers};
        use crate::primitives::{Direction, LineNumber};

        /// Fold: lines 2-3 are folded.
        struct FoldLines2To3;
        impl FoldProvider for FoldLines2To3 {
            fn next_visible_line(&self, line: LineNumber, dir: Direction) -> LineNumber {
                if (2..=3).contains(&line.get()) {
                    match dir {
                        Direction::Forward => LineNumber::new(4),
                        Direction::Backward => LineNumber::new(1),
                    }
                } else {
                    line
                }
            }
            fn is_folded(&self, line: LineNumber) -> bool {
                (2..=3).contains(&line.get())
            }
        }

        #[test]
        fn w_snaps_forward_past_fold() {
            // "aaa\nbbb\nccc\nddd\neee\n"
            //  0    4    8    12   16
            // Lines 2-3 are folded. `w` from line 1 offset 4 ("bbb")
            // would normally land on line 2 offset 8, but fold snaps forward
            // to end of line 3 (offset 11, end of "ddd").
            let text = "aaa\nbbb\nccc\nddd\neee\n";
            let opts = VimOptions::default();
            let fold = FoldLines2To3;
            let providers = Providers::new().with_fold(&fold);
            let ctx = MotionContext::new(text, Offset::new(4), 1, &opts).with_providers(providers);
            let result = w(&ctx);
            // Word motion from "bbb" lands on "ccc" which is folded.
            // fold_snap(Forward) snaps to end of fold line 3 = line_end(3).
            if let MotionResult::Position(pos) = result {
                let line = crate::commands::helpers::line_of(text, pos.get());
                // Result should be past the fold (line 4 or at end of fold)
                assert!(
                    line >= 3,
                    "w should snap forward past fold, got line {line} offset {}",
                    pos.get()
                );
            } else {
                panic!("Expected Position, got {result:?}");
            }
        }

        #[test]
        fn b_snaps_backward_before_fold() {
            // "aaa\nbbb\nccc\nddd\neee\n"
            //  0    4    8    12   16
            // Lines 2-3 folded. `b` from "eee" (line 4, offset 16)
            // would normally land in "ddd" (line 3, folded).
            // fold_snap(Backward) snaps to start of fold (line 2, offset 8).
            let text = "aaa\nbbb\nccc\nddd\neee\n";
            let opts = VimOptions::default();
            let fold = FoldLines2To3;
            let providers = Providers::new().with_fold(&fold);
            let ctx = MotionContext::new(text, Offset::new(16), 1, &opts).with_providers(providers);
            let result = b(&ctx);
            if let MotionResult::Position(pos) = result {
                // Should snap backward to start of fold (line 2)
                assert!(
                    pos.get() <= 8,
                    "b should snap backward before fold, got offset {}",
                    pos.get()
                );
            } else {
                panic!("Expected Position, got {result:?}");
            }
        }
    }
}
