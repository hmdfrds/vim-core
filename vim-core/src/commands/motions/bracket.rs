//! Bracket motions: `%`, `[X`, `]X`
//!
//! Navigate between matching brackets and block boundaries.
//!
//! # Philosophy
//!
//! Plain functions, not trait methods — no dynamic dispatch.
//! Each motion is a standalone function called directly from dispatch.

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::next_char_boundary;
use crate::primitives::{Direction, Offset, MAX_BRACKET_TRAVEL};
use smallvec::SmallVec;

// ─────────────────────────────────────────────────────────────────────────────
// Bracket Motions
// ─────────────────────────────────────────────────────────────────────────────

/// All bracket pairs recognized by the `%` motion and bracket text objects.
///
/// Ordered: ASCII first (most common), then Unicode.
///
/// `<>` is deliberately excluded — in real Vim, `matchpairs` defaults to
/// `(:),{:},[:]`. Angle brackets cause false positives with comparison
/// operators and generics. Hosts wanting `<>` should use the
/// SyntaxProvider-aware path.
pub(crate) const BRACKET_PAIRS: [(char, char); 14] = [
    // ASCII
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    // Smart quotes
    ('\u{2018}', '\u{2019}'), // '' smart single quotes
    ('\u{201C}', '\u{201D}'), // "" smart double quotes
    // European
    ('\u{00AB}', '\u{00BB}'), // «» guillemets
    // CJK
    ('\u{300C}', '\u{300D}'), // 「」 corner brackets
    ('\u{300E}', '\u{300F}'), // 『』 white corner brackets
    ('\u{3008}', '\u{3009}'), // 〈〉 angle brackets
    ('\u{300A}', '\u{300B}'), // 《》 double angle brackets
    ('\u{3010}', '\u{3011}'), // 【】 lenticular brackets
    ('\u{3014}', '\u{3015}'), // 〔〕 tortoise shell brackets
    // Fullwidth
    ('\u{FF08}', '\u{FF09}'), // （） fullwidth parentheses
    // Mathematical
    ('\u{27E8}', '\u{27E9}'), // ⟨⟩ mathematical angle brackets
];

/// Returns bracket pair info for a given character.
///
/// Returns `(open_bracket, close_bracket, is_forward)` for a bracket character,
/// or `None` if the character is not a bracket.
pub(crate) fn bracket_info(c: char) -> Option<(char, char, bool)> {
    BRACKET_PAIRS.iter().find_map(|&(open, close)| {
        if c == open {
            Some((open, close, true))
        } else if c == close {
            Some((open, close, false))
        } else {
            None
        }
    })
}

/// `%` - Go to matching bracket/parenthesis.
///
/// If cursor is not on a bracket, scans right on current line to find the first one.
pub fn matching_bracket(ctx: &MotionContext<'_>) -> MotionResult {
    let text = ctx.text;
    let cursor = ctx.cursor.get();

    // Find bracket at or after cursor on current line
    // First check if cursor is on a bracket
    let cursor_char = text[cursor..].chars().next();
    let (bracket_pos, open, close, forward) = if let Some(info) = cursor_char.and_then(bracket_info)
    {
        (cursor, info.0, info.1, info.2)
    } else {
        // Scan right on current line to find first bracket
        let line_end = text[cursor..].find('\n').map_or(text.len(), |i| cursor + i);

        let mut found = None;
        for (i, c) in text[cursor..line_end].char_indices() {
            if let Some(info) = bracket_info(c) {
                found = Some((cursor + i, info.0, info.1, info.2));
                break;
            }
        }

        match found {
            Some(f) => f,
            None => return MotionResult::Error,
        }
    };

    let mut depth = 1;

    if forward {
        let after_open = bracket_pos + open.len_utf8();
        let window_end = (after_open + MAX_BRACKET_TRAVEL).min(text.len());
        let ranges = CommentStringRanges::scan(text, after_open, window_end);

        let mut traveled = 0;
        for (i, c) in text[after_open..].char_indices() {
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                return MotionResult::Error; // No match found within travel limit
            }
            let absolute_pos = after_open + i;
            // Skip brackets inside string literals (Neovim `inquote` behavior).
            // Do NOT skip brackets inside comments — Neovim's `%` counts them.
            if ranges.in_string(absolute_pos) {
                continue;
            }
            if c == close {
                depth -= 1;
                if depth == 0 {
                    return MotionResult::Position(Offset::new(absolute_pos));
                }
            } else if c == open {
                depth += 1;
            }
        }
    } else {
        let before = &text[..bracket_pos];
        let window_start = bracket_pos.saturating_sub(MAX_BRACKET_TRAVEL);
        let ranges = CommentStringRanges::scan(text, window_start, bracket_pos);

        let mut traveled = 0;
        for (i, c) in before.char_indices().rev() {
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                return MotionResult::Error;
            }
            if ranges.in_string(i) {
                continue;
            }
            if c == open {
                depth -= 1;
                if depth == 0 {
                    return MotionResult::Position(Offset::new(i));
                }
            } else if c == close {
                depth += 1;
            }
        }
    }

    MotionResult::Error
}

/// `[{` - Previous unmatched `{`.
pub fn prev_unmatched_brace(ctx: &MotionContext<'_>) -> MotionResult {
    find_unmatched(ctx, '{')
}

/// `]}` - Next unmatched `}`.
pub fn next_unmatched_brace(ctx: &MotionContext<'_>) -> MotionResult {
    find_unmatched(ctx, '}')
}

/// `[(` - Previous unmatched `(`.
pub fn prev_unmatched_paren(ctx: &MotionContext<'_>) -> MotionResult {
    find_unmatched(ctx, '(')
}

/// `])` - Next unmatched `)`.
pub fn next_unmatched_paren(ctx: &MotionContext<'_>) -> MotionResult {
    find_unmatched(ctx, ')')
}

// ─────────────────────────────────────────────────────────────────────────────
// Method boundary motions ([m, ]m, [M, ]M)
// ─────────────────────────────────────────────────────────────────────────────

/// `[m` — previous method/function start ('{' at brace depth 0).
pub fn prev_method_start(ctx: &MotionContext<'_>) -> MotionResult {
    find_depth_zero_brace(ctx, '{', Direction::Backward)
}

/// `]m` — next method/function start ('{' at brace depth 0).
pub fn next_method_start(ctx: &MotionContext<'_>) -> MotionResult {
    find_depth_zero_brace(ctx, '{', Direction::Forward)
}

/// `[M` — previous method/function end ('}' at brace depth 0).
pub fn prev_method_end(ctx: &MotionContext<'_>) -> MotionResult {
    find_depth_zero_brace(ctx, '}', Direction::Backward)
}

/// `]M` — next method/function end ('}' at brace depth 0).
pub fn next_method_end(ctx: &MotionContext<'_>) -> MotionResult {
    find_depth_zero_brace(ctx, '}', Direction::Forward)
}

/// Find the matching brace using unmatched-brace search, iterated for count.
///
/// Used by `[m`/`]m`/`[M`/`]M`. In Neovim, these motions use `findmatchlimit()`
/// which finds the nearest unmatched brace in the given direction. Count is
/// handled by iterating: each step finds the next unmatched brace from the
/// previous result. If a step fails, the last successful position is returned.
fn find_depth_zero_brace(
    ctx: &MotionContext<'_>,
    target: char,
    direction: Direction,
) -> MotionResult {
    let text = ctx.text;
    let count = ctx.count_usize();

    let (open, close) = ('{', '}');
    let mut pos = ctx.cursor.get();
    let mut last_found: Option<usize> = None;

    for _ in 0..count {
        let result = find_unmatched_brace_single(text, pos, target, open, close, direction);
        match result {
            Some(new_pos) => {
                last_found = Some(new_pos);
                pos = new_pos;
            }
            None => break,
        }
    }

    match last_found {
        Some(offset) => MotionResult::Position(Offset::new(offset)),
        None => MotionResult::Error,
    }
}

/// Single-step unmatched brace search (no count).
fn find_unmatched_brace_single(
    text: &str,
    cursor: usize,
    target: char,
    open: char,
    close: char,
    direction: Direction,
) -> Option<usize> {
    let mut depth: i32 = 0;

    if direction.is_forward() {
        let start = next_char_boundary(text, cursor);
        let window_end = (start + MAX_BRACKET_TRAVEL).min(text.len());
        let ranges = CommentStringRanges::scan(text, start, window_end);

        let mut traveled = 0;
        for (i, c) in text.get(start..).unwrap_or("").char_indices() {
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                break;
            }
            let absolute_pos = start + i;
            if ranges.contains(absolute_pos) {
                continue;
            }
            if c == target {
                depth += 1;
                if depth == 1 {
                    return Some(absolute_pos);
                }
            } else if c == if target == close { open } else { close } {
                depth -= 1;
                if depth < 0 {
                    return Some(absolute_pos);
                }
            }
        }
    } else {
        let before = text.get(..cursor).unwrap_or("");
        let window_start = cursor.saturating_sub(MAX_BRACKET_TRAVEL);
        let ranges = CommentStringRanges::scan(text, window_start, cursor);

        let mut traveled = 0;
        for (i, c) in before.char_indices().rev() {
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                break;
            }
            if ranges.contains(i) {
                continue;
            }
            if c == target {
                depth += 1;
                if depth == 1 {
                    return Some(i);
                }
            } else if c == if target == open { close } else { open } {
                depth -= 1;
                // When scanning backward and encountering an unmatched opposite
                // brace (depth < 0), return it only if the cursor is NOT sitting
                // on the target brace. If the cursor IS on the target brace, the
                // opposite brace is actually matched by the cursor's brace, so
                // we should skip it. This matches Neovim's findmatchlimit
                // behavior for [m/[M/]m/]M section motions.
                if depth < 0 {
                    let cursor_char = text.as_bytes().get(cursor).copied();
                    if cursor_char != Some(target as u8) {
                        return Some(i);
                    }
                    // Cursor's brace matches this opposite brace — don't
                    // return and don't reset depth; subsequent target braces
                    // will cancel the depth back towards 0 naturally.
                }
            }
        }
    }

    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Comment navigation ([/, ]/)
// ─────────────────────────────────────────────────────────────────────────────

/// `[/` — go to previous start of C comment (`/*`).
pub fn prev_comment_start(ctx: &MotionContext<'_>) -> MotionResult {
    find_comment_marker(ctx, "/*", Direction::Backward)
}

/// `]/` — go to next end of C comment (`*/`).
pub fn next_comment_end(ctx: &MotionContext<'_>) -> MotionResult {
    find_comment_marker(ctx, "*/", Direction::Forward)
}

/// Find the Nth occurrence of a 2-char comment marker (`/*` or `*/`).
fn find_comment_marker(
    ctx: &MotionContext<'_>,
    marker: &str,
    direction: Direction,
) -> MotionResult {
    let text = ctx.text;
    let cursor = ctx.cursor.get();
    let count = ctx.count_usize();
    let mut found = 0;

    if direction.is_forward() {
        let start = (cursor + 1).min(text.len());
        let search = text.get(start..).unwrap_or("");
        let mut traveled = 0;
        for (pos, _) in search.match_indices(marker) {
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                break;
            }
            found += 1;
            if found == count {
                return MotionResult::Position(Offset::new(start + pos));
            }
        }
    } else {
        let search = text.get(..cursor).unwrap_or("");
        let mut traveled = 0;
        for (pos, _) in search.rmatch_indices(marker) {
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                break;
            }
            found += 1;
            if found == count {
                return MotionResult::Position(Offset::new(pos));
            }
        }
    }
    MotionResult::Error
}

// ─────────────────────────────────────────────────────────────────────────────
// Bracket/quote pair navigation: ]b, [b, ]q, [q
// ─────────────────────────────────────────────────────────────────────────────

/// Opening brackets for `]b`/`[b` navigation.
const OPENING_BRACKETS: &[char] = &['(', '[', '{', '<'];

/// Quote characters for `]q`/`[q` navigation.
const QUOTE_CHARS: &[char] = &['"', '\'', '`'];

/// `]b` — jump to the Nth next opening bracket.
///
/// From cursor+1, scans forward for any char in `([{<`.
pub fn find_next_bracket_pair(ctx: &MotionContext<'_>) -> MotionResult {
    find_next_char_in_set(ctx, OPENING_BRACKETS)
}

/// `[b` — jump to the Nth previous opening bracket.
///
/// From cursor-1, scans backward for any char in `([{<`.
pub fn find_prev_bracket_pair(ctx: &MotionContext<'_>) -> MotionResult {
    find_prev_char_in_set(ctx, OPENING_BRACKETS)
}

/// `]q` — jump to the Nth next quote character.
///
/// From cursor+1, scans forward for any char in `"'\``.
pub fn find_next_quote(ctx: &MotionContext<'_>) -> MotionResult {
    find_next_char_in_set(ctx, QUOTE_CHARS)
}

/// `[q` — jump to the Nth previous quote character.
///
/// From cursor-1, scans backward for any char in `"'\``.
pub fn find_prev_quote(ctx: &MotionContext<'_>) -> MotionResult {
    find_prev_char_in_set(ctx, QUOTE_CHARS)
}

/// Scan forward from cursor+1 for the Nth occurrence of any char in `targets`.
fn find_next_char_in_set(ctx: &MotionContext<'_>, targets: &[char]) -> MotionResult {
    let text = ctx.text;
    let cursor = ctx.cursor.get();
    let count = ctx.count_usize();
    let mut found = 0;

    let start = next_char_boundary(text, cursor);
    let mut traveled = 0;
    for (i, c) in text.get(start..).unwrap_or("").char_indices() {
        traveled += 1;
        if traveled >= MAX_BRACKET_TRAVEL {
            break;
        }
        if targets.contains(&c) {
            found += 1;
            if found == count {
                return MotionResult::Position(Offset::new(start + i));
            }
        }
    }
    MotionResult::Error
}

/// Scan backward from cursor-1 for the Nth occurrence of any char in `targets`.
fn find_prev_char_in_set(ctx: &MotionContext<'_>, targets: &[char]) -> MotionResult {
    let text = ctx.text;
    let cursor = ctx.cursor.get();
    let count = ctx.count_usize();
    let mut found = 0;

    let mut traveled = 0;
    for (i, c) in text.get(..cursor).unwrap_or("").char_indices().rev() {
        traveled += 1;
        if traveled >= MAX_BRACKET_TRAVEL {
            break;
        }
        if targets.contains(&c) {
            found += 1;
            if found == count {
                return MotionResult::Position(Offset::new(i));
            }
        }
    }
    MotionResult::Error
}

// ─────────────────────────────────────────────────────────────────────────────
// String/Comment Detection
// ─────────────────────────────────────────────────────────────────────────────

/// Lexical state at a given position in the text.
///
/// Determined by a single forward scan from byte 0 to `pos`, tracking all
/// state transitions: string delimiters (with escape handling), block comments
/// (`/* ... */`), and line comments (`//` to end-of-line only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexicalState {
    Normal,
    InBlockComment,
    InDoubleQuote,
    InSingleQuote,
}

/// Maximum number of lines to scan backward for lexical context.
///
/// 256 lines covers virtually all real-world string/comment spans.
/// Matches Vim/Neovim's bounded lexical scanning behavior.
const MAX_LEXICAL_SCAN_LINES: usize = 256;

/// Compute the lexical state at byte offset `pos` via a single forward scan.
///
/// Handles:
/// - `//` line comments: skip to next newline (NOT past it — the newline itself
///   resets to `Normal`). This fixes the old bug where `//` detection bled
///   across line boundaries.
/// - `/* ... */` block comments: properly nested inside the state machine, so
///   `/*` inside a string literal is NOT treated as a comment opener (fixes the
///   old `is_in_block_comment` rfind bug).
/// - Escaped quotes (`\"`, `\'`): skipped inside their respective string states.
/// - Quotes inside block comments are ignored (NOT treated as string openers).
// All byte accesses are bounded by `i < end` and `i + 1 < end` guards above each
// indexing operation; `end = pos.min(bytes.len())` is an upper bound on `i`.
#[expect(
    clippy::indexing_slicing,
    reason = "byte accesses guarded by `i < end` and `i + 1 < end` checks; `end <= bytes.len()`"
)]
fn lexical_state_at(text: &str, pos: usize) -> LexicalState {
    let bytes = text.as_bytes();
    let end = pos.min(bytes.len());

    // Bound the scan: instead of scanning from byte 0, scan from at most
    // MAX_LEXICAL_SCAN_LINES lines before `pos`. Build a local LineIndex
    // to find the scan start efficiently.
    let scan_start = if end > 8192 {
        // Only bother with bounded scan for positions deep in the file
        let idx = crate::commands::line_index::LineIndex::build(text.get(..end).unwrap_or(text));
        let current_line = idx.line_of(end.saturating_sub(1));
        let start_line = current_line.saturating_sub(MAX_LEXICAL_SCAN_LINES);
        idx.line_start(start_line).unwrap_or(0)
    } else {
        0 // Small files: scan from beginning (fast anyway)
    };

    let mut state = LexicalState::Normal;
    let mut i = scan_start;

    while i < end {
        match state {
            LexicalState::Normal => {
                if bytes[i] == b'"' {
                    state = LexicalState::InDoubleQuote;
                } else if bytes[i] == b'\'' {
                    state = LexicalState::InSingleQuote;
                } else if i + 1 < end && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                    // Line comment: skip to end of line (newline resets to Normal).
                    i += 2;
                    while i < end && bytes[i] != b'\n' {
                        i += 1;
                    }
                    // After the loop, `i` is either at the newline or at `end`.
                    // If at a newline, the outer `i += 1` at the bottom will
                    // advance past it, returning to Normal on the next line.
                    continue;
                } else if i + 1 < end && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    state = LexicalState::InBlockComment;
                    i += 2; // skip past /*
                    continue;
                }
            }
            LexicalState::InDoubleQuote => {
                if bytes[i] == b'\\' && i + 1 < end {
                    i += 2; // skip escaped char
                    continue;
                } else if bytes[i] == b'"' {
                    state = LexicalState::Normal;
                }
            }
            LexicalState::InSingleQuote => {
                if bytes[i] == b'\\' && i + 1 < end {
                    i += 2; // skip escaped char
                    continue;
                } else if bytes[i] == b'\'' {
                    state = LexicalState::Normal;
                }
            }
            LexicalState::InBlockComment => {
                if i + 1 < end && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    state = LexicalState::Normal;
                    i += 2; // skip past */
                    continue;
                }
            }
        }
        i += 1;
    }

    state
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Find Nth unmatched bracket.
fn find_unmatched(ctx: &MotionContext<'_>, target: char) -> MotionResult {
    let (open, close, forward) = match target {
        '{' => ('{', '}', false), // [{ goes backward to find unmatched opener
        '}' => ('{', '}', true),  // ]} goes forward to find unmatched closer
        '(' => ('(', ')', false), // [( goes backward
        ')' => ('(', ')', true),  // ]) goes forward
        _ => return MotionResult::Error,
    };

    let text = ctx.text;
    let cursor = ctx.cursor.get();
    let count = i32::try_from(ctx.count).unwrap_or(i32::MAX);
    let mut depth: i32 = 0;

    if forward {
        // Search forward for unmatched closer
        let search_start = next_char_boundary(text, cursor);
        // Compute window for forward scan and pre-scan comment/string ranges
        let window_end = (search_start + MAX_BRACKET_TRAVEL).min(text.len());
        let ranges = CommentStringRanges::scan(text, search_start, window_end);

        let mut traveled = 0;
        for (i, c) in text.get(search_start..).unwrap_or("").char_indices() {
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                break; // Exceeded travel limit
            }
            let absolute_pos = search_start + i;
            if ranges.contains(absolute_pos) {
                continue;
            }
            if c == close {
                depth += 1;
                if depth == count {
                    return MotionResult::Position(Offset::new(absolute_pos));
                }
            } else if c == open {
                depth -= 1;
            }
        }
    } else {
        // Search backward for unmatched opener
        let before = text.get(..cursor).unwrap_or("");
        // Compute window for backward scan and pre-scan comment/string ranges
        let window_start = cursor.saturating_sub(MAX_BRACKET_TRAVEL);
        let ranges = CommentStringRanges::scan(text, window_start, cursor);

        let mut traveled = 0;
        for (i, c) in before.char_indices().rev() {
            traveled += 1;
            if traveled >= MAX_BRACKET_TRAVEL {
                break; // Exceeded travel limit
            }
            if ranges.contains(i) {
                continue;
            }
            if c == open {
                depth += 1;
                if depth == count {
                    return MotionResult::Position(Offset::new(i));
                }
            } else if c == close {
                depth -= 1;
            }
        }
    }

    MotionResult::Error
}

// ─────────────────────────────────────────────────────────────────────────────
// CommentStringRanges: Pre-scanned ranges for bracket safety
// ─────────────────────────────────────────────────────────────────────────────

/// Pre-scanned comment and string literal ranges within a text window.
///
/// Built once before a bracket scan loop, then queried per-character
/// via binary search. Converts O(n²) per-character scanning to
/// O(n) pre-scan + O(log k) per query.
pub(crate) struct CommentStringRanges {
    /// Sorted, non-overlapping (start, end) byte ranges that are
    /// inside comments or string literals. Ranges are [start, end),
    /// i.e., the end byte is exclusive.
    ranges: SmallVec<[(usize, usize); 16]>,
    /// Subset of `ranges` that are string literals only (not comments).
    string_ranges: SmallVec<[(usize, usize); 8]>,
}

impl CommentStringRanges {
    /// Scan text[window_start..window_end] and return ranges of comments/strings.
    ///
    /// # Algorithm
    ///
    /// Single forward pass over the window, tracking a state machine:
    /// - Normal: waiting for quote, comment start, or escape
    /// - InDoubleQuote: inside `"..."`, skip on `\"`, exit on unescaped `"`
    /// - InSingleQuote: inside `'...'`, skip on `\'`, exit on unescaped `'`
    /// - InBlockComment: from `/*` to `*/`
    ///
    /// Line comments (`//` to end of line) are handled inline in the `Normal`
    /// state rather than via a dedicated state variant.
    ///
    /// For initial block comment state at window_start: calls `is_in_block_comment(text, window_start)` once.
    pub(crate) fn scan(text: &str, window_start: usize, window_end: usize) -> Self {
        let mut ranges = SmallVec::new();
        let mut string_ranges = SmallVec::new();

        // Single call to determine the lexical state at window_start.
        // This replaces three separate (and buggy) functions.
        let initial_state = lexical_state_at(text, window_start);

        let mut state = initial_state;

        let window = text.get(window_start..window_end).unwrap_or("");
        let bytes = window.as_bytes();
        let mut i = 0;
        let mut range_start: Option<usize> = match state {
            LexicalState::Normal => None,
            LexicalState::InBlockComment
            | LexicalState::InDoubleQuote
            | LexicalState::InSingleQuote => Some(window_start),
        };

        while let Some(&byte) = bytes.get(i) {
            let absolute_pos = window_start + i;
            // `None` past the end of the window; every two-byte digraph below
            // therefore fails to match at the last byte, exactly as the old
            // `i + 1 < bytes.len()` guards did.
            let next_byte = bytes.get(i + 1).copied();

            match state {
                LexicalState::Normal => {
                    if byte == b'"' {
                        state = LexicalState::InDoubleQuote;
                        range_start = Some(absolute_pos);
                    } else if byte == b'\'' {
                        state = LexicalState::InSingleQuote;
                        range_start = Some(absolute_pos);
                    } else if byte == b'/' && next_byte == Some(b'/') {
                        // Line comment extends to end of line or end of window
                        let line_end = window
                            .get(i..)
                            .and_then(|rest| rest.find('\n'))
                            .map_or(bytes.len(), |offset| i + offset);
                        ranges.push((absolute_pos, window_start + line_end));
                        i = line_end;
                        continue;
                    } else if byte == b'/' && next_byte == Some(b'*') {
                        state = LexicalState::InBlockComment;
                        range_start = Some(absolute_pos);
                        i += 1; // skip *
                    }
                }
                LexicalState::InDoubleQuote => {
                    if byte == b'\\' && next_byte.is_some() {
                        i += 1; // skip escaped char
                    } else if byte == b'"' {
                        if let Some(start) = range_start {
                            let range = (start, absolute_pos + 1);
                            ranges.push(range);
                            string_ranges.push(range);
                            range_start = None;
                        }
                        state = LexicalState::Normal;
                    }
                }
                LexicalState::InSingleQuote => {
                    if byte == b'\\' && next_byte.is_some() {
                        i += 1; // skip escaped char
                    } else if byte == b'\'' {
                        if let Some(start) = range_start {
                            let range = (start, absolute_pos + 1);
                            ranges.push(range);
                            string_ranges.push(range);
                            range_start = None;
                        }
                        state = LexicalState::Normal;
                    }
                }
                LexicalState::InBlockComment => {
                    if byte == b'*' && next_byte == Some(b'/') {
                        if let Some(start) = range_start {
                            ranges.push((start, absolute_pos + 2));
                            range_start = None;
                        }
                        state = LexicalState::Normal;
                        i += 1; // skip /
                    }
                }
            }
            i += 1;
        }

        // If we end in a string or comment that extends to window_end
        if let Some(start) = range_start {
            let range = (start, window_end);
            ranges.push(range);
            if matches!(
                state,
                LexicalState::InDoubleQuote | LexicalState::InSingleQuote
            ) {
                string_ranges.push(range);
            }
        }

        Self {
            ranges,
            string_ranges,
        }
    }

    /// Check if position is inside a comment or string literal using binary search.
    ///
    /// # Complexity
    ///
    /// O(log k) where k = number of ranges.
    pub(crate) fn contains(&self, pos: usize) -> bool {
        Self::pos_in_ranges(&self.ranges, pos)
    }

    /// Check if position is inside a string literal (not a comment).
    ///
    /// Used by `%` (matching bracket) which skips brackets inside strings
    /// but NOT inside comments, matching Neovim's `findmatchlimit` behavior.
    pub(crate) fn in_string(&self, pos: usize) -> bool {
        Self::pos_in_ranges(&self.string_ranges, pos)
    }

    fn pos_in_ranges(ranges: &[(usize, usize)], pos: usize) -> bool {
        ranges
            .binary_search_by(|&(start, end)| {
                if pos < start {
                    std::cmp::Ordering::Greater
                } else if pos >= end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ShowMatch support: find matching opener for a just-inserted closing bracket
// ─────────────────────────────────────────────────────────────────────────────

/// Find the matching opening bracket for a closing bracket at `close_pos`.
///
/// Used by the insert-mode `showmatch` feature: when a closing bracket is
/// typed, the engine calls this to find the matching opener's position so it
/// can emit `Effect::ShowMatch`.
///
/// `close_pos` is the byte offset where the closing bracket will be (or has
/// just been) inserted. The text before `close_pos` is scanned backward
/// using the same string-literal-aware logic as `matching_bracket`.
///
/// Returns `Some(Offset)` of the matching opener, or `None` if no match is
/// found within `MAX_BRACKET_TRAVEL`.
pub(crate) fn find_matching_open_bracket(
    text: &str,
    close_pos: usize,
    close_char: char,
) -> Option<Offset> {
    let (open, close, is_forward) = bracket_info(close_char)?;
    // Only match closing brackets (is_forward == false means the char is a closer).
    if is_forward {
        return None;
    }

    let before = text.get(..close_pos)?;
    let window_start = close_pos.saturating_sub(MAX_BRACKET_TRAVEL);
    let ranges = CommentStringRanges::scan(text, window_start, close_pos);

    let mut depth: usize = 1;
    let mut traveled = 0;
    for (i, c) in before.char_indices().rev() {
        traveled += 1;
        if traveled >= MAX_BRACKET_TRAVEL {
            return None;
        }
        if ranges.in_string(i) {
            continue;
        }
        if c == open {
            depth -= 1;
            if depth == 0 {
                return Some(Offset::new(i));
            }
        } else if c == close {
            depth += 1;
        }
    }

    None
}

#[cfg(test)]
#[path = "bracket_tests.rs"]
mod tests;
