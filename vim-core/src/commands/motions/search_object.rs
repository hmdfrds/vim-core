//! Search object motions: gn, gN
//!
//! These motions find the next/previous search match and select the match
//! as a **range** (text-object-like behavior).
//!
//! ## Behavior
//!
//! | Keys | Normal mode | Operator-pending (cgn) |
//! |------|-------------|------------------------|
//! | `gn` | Move to end of next/current match | Select entire match range |
//! | `gN` | Move to start of prev/current match | Select entire match range |
//!
//! ## Special Cases
//!
//! - If cursor is already inside a match, uses that match
//! - If no search pattern is set, returns `Failed`
//! - Wraps around the document like `n`/`N`
//!
//! ## Design
//!
//! Returns `MotionResult::Range { start, end }` so the operator system
//! can act on the entire match. The dispatch layer interprets `Range`
//! for normal mode (cursor → end) and operator-pending (full range).
//!
//! ## Regex
//!
//! Uses `VimRegex` for pattern matching, supporting `\d\+`, `[a-z]`,
//! `\v\w+`, `\<`/`\>` word boundaries, and all Vim regex features.
//! Falls back to literal substring matching if the pattern fails to
//! compile as a regex.

use super::search_modifiers::{compute_search_flags, parse_search_modifiers};
use super::types::{MotionContext, MotionResult};
use crate::primitives::Offset;

/// `gn` — search object forward: select current/next search match.
///
/// Returns `Range { start, end }` for the match.
/// In normal mode, the dispatch layer moves cursor to `end - 1` (last byte of match).
/// In operator-pending mode (`cgn`, `dgn`), the operator acts on the full range.
pub fn gn(ctx: &MotionContext<'_>) -> MotionResult {
    let pattern = match ctx.search_pattern {
        Some(p) if !p.is_empty() => p,
        _ => return MotionResult::Error,
    };

    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Error;
    }

    let cursor = ctx.cursor.get();
    let count = ctx.count as usize;

    // Compile pattern as regex. VimRegex handles \<, \>, \c, \C, \v, \V natively.
    let all_matches = match collect_all_matches(text, pattern, ctx.options) {
        Some(v) if !v.is_empty() => v,
        _ => return MotionResult::Error,
    };

    // 1. Check if cursor is inside a match
    if let Some((idx, &(ms, me))) = find_match_containing(cursor, &all_matches) {
        if count == 1 {
            return MotionResult::Range {
                start: Offset::new(ms),
                end: Offset::new(me),
            };
        }
        // count > 1: skip this match, find (count-1) more forward from idx
        return nth_match_forward_from(&all_matches, idx + 1, count - 1, cursor);
    }

    // 2. Not inside a match — find next match forward (wrapping)
    nth_match_forward(&all_matches, cursor, count)
}

/// `gN` — search object backward: select current/previous search match.
///
/// Returns `Range { start, end }` for the match.
/// In normal mode, the dispatch layer moves cursor to `start`.
/// In operator-pending mode (`cgN`, `dgN`), the operator acts on the full range.
#[allow(non_snake_case, reason = "Vim motion name")]
pub fn gN(ctx: &MotionContext<'_>) -> MotionResult {
    let pattern = match ctx.search_pattern {
        Some(p) if !p.is_empty() => p,
        _ => return MotionResult::Error,
    };

    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Error;
    }

    let cursor = ctx.cursor.get();
    let count = ctx.count as usize;

    // Compile pattern as regex. VimRegex handles \<, \>, \c, \C, \v, \V natively.
    let all_matches = match collect_all_matches(text, pattern, ctx.options) {
        Some(v) if !v.is_empty() => v,
        _ => return MotionResult::Error,
    };

    // 1. Check if cursor is inside a match
    if let Some((idx, &(ms, me))) = find_match_containing(cursor, &all_matches) {
        if count == 1 {
            return MotionResult::Range {
                start: Offset::new(ms),
                end: Offset::new(me),
            };
        }
        // count > 1: skip this match, find (count-1) more backward from idx
        return nth_match_backward_from(&all_matches, idx, count - 1, cursor);
    }

    // 2. Not inside a match — find previous match backward (wrapping)
    nth_match_backward(&all_matches, cursor, count)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Internal helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Collect all non-overlapping regex matches in `text`.
///
/// Returns `None` if the regex fails to compile; returns `Some(vec)` on success
/// (the vec may be empty if there are no matches).
///
/// Each match is `(start, end)` with exclusive end — zero-width matches are
/// extended by one character to avoid degenerate selections.
fn collect_all_matches(
    text: &str,
    pattern: &str,
    options: &crate::primitives::VimOptions,
) -> Option<Vec<(usize, usize)>> {
    let (stripped, modifiers) = parse_search_modifiers(pattern);
    let flags = compute_search_flags(options, &stripped, &modifiers);

    let re = crate::regex::VimRegex::cached_with_magic(&stripped, flags.magic()).ok()?;

    // No cursor, per evolve's search_object: text objects scan the whole buffer
    // and have no cursor-relative anchor.
    let match_ctx = crate::regex::MatchContext::builder(text)
        .case_sensitive(flags.case_sensitive())
        .build();

    let vim_matches = re.find_all(&match_ctx).ok()?;

    Some(
        vim_matches
            .into_iter()
            .map(|m| {
                let start = m.range.start;
                let mut end = m.range.end;
                // Guard against zero-width matches: extend by one char.
                if end == start {
                    end = text[start..]
                        .chars()
                        .next()
                        .map_or(start, |c| start + c.len_utf8());
                }
                (start, end)
            })
            .collect(),
    )
}

/// Find the index and value of the match that contains `cursor` (start <= cursor < end).
fn find_match_containing(
    cursor: usize,
    matches: &[(usize, usize)],
) -> Option<(usize, &(usize, usize))> {
    matches
        .iter()
        .enumerate()
        .find(|(_, &(start, end))| start <= cursor && cursor < end)
}

/// Find the nth match forward from cursor, wrapping around.
///
/// `matches` is sorted by position (output of `find_all`).
fn nth_match_forward(matches: &[(usize, usize)], cursor: usize, count: usize) -> MotionResult {
    // Matches after cursor
    let after = matches.iter().filter(|&&(start, _)| start > cursor);
    // Matches at or before cursor (wrap portion)
    let before = matches.iter().filter(|&&(start, _)| start <= cursor);

    let mut found = 0;
    for &(ms, me) in after.chain(before) {
        found += 1;
        if found == count {
            return MotionResult::Range {
                start: Offset::new(ms),
                end: Offset::new(me),
            };
        }
    }
    MotionResult::Error
}

/// Find the nth match forward starting from a specific index in the match list.
/// Used when cursor is inside a match and count > 1 (skip current, find more forward).
fn nth_match_forward_from(
    matches: &[(usize, usize)],
    from_idx: usize,
    count: usize,
    cursor: usize,
) -> MotionResult {
    // Iterate from from_idx to end, then wrap from 0 to from_idx
    let after = matches.iter().skip(from_idx);
    let before = matches.iter().take(from_idx);

    let mut found = 0;
    for &(ms, me) in after.chain(before) {
        // When wrapping, skip the match the cursor is in
        if ms <= cursor && cursor < me {
            continue;
        }
        found += 1;
        if found == count {
            return MotionResult::Range {
                start: Offset::new(ms),
                end: Offset::new(me),
            };
        }
    }
    MotionResult::Error
}

/// Find the nth match backward from cursor, wrapping around.
///
/// `matches` is sorted by position (output of `find_all`).
fn nth_match_backward(matches: &[(usize, usize)], cursor: usize, count: usize) -> MotionResult {
    // Matches before cursor (in reverse)
    let before = matches.iter().rev().filter(|&&(start, _)| start < cursor);
    // Matches at or after cursor (wrap portion, in reverse)
    let after = matches.iter().rev().filter(|&&(start, _)| start >= cursor);

    let mut found = 0;
    for &(ms, me) in before.chain(after) {
        found += 1;
        if found == count {
            return MotionResult::Range {
                start: Offset::new(ms),
                end: Offset::new(me),
            };
        }
    }
    MotionResult::Error
}

/// Find the nth match backward starting from a specific index in the match list.
/// Used when cursor is inside a match and count > 1 (skip current, find more backward).
fn nth_match_backward_from(
    matches: &[(usize, usize)],
    from_idx: usize,
    count: usize,
    cursor: usize,
) -> MotionResult {
    // Iterate from from_idx-1 down to 0, then wrap from end down to from_idx
    let before = matches.iter().take(from_idx).rev();
    let after = matches.iter().skip(from_idx).rev();

    let mut found = 0;
    for &(ms, me) in before.chain(after) {
        // When wrapping, skip the match the cursor is in
        if ms <= cursor && cursor < me {
            continue;
        }
        found += 1;
        if found == count {
            return MotionResult::Range {
                start: Offset::new(ms),
                end: Offset::new(me),
            };
        }
    }
    MotionResult::Error
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_search<'text>(
        text: &'text str,
        cursor: usize,
        pattern: &'text str,
        options: &'text crate::primitives::VimOptions,
    ) -> MotionContext<'text> {
        MotionContext::new(text, Offset::new(cursor), 1, options)
            .with_search(pattern, crate::primitives::Direction::Forward)
    }

    // Helper to extract Range start
    fn range_start(result: &MotionResult) -> usize {
        match result {
            MotionResult::Range { start, .. } => start.get(),
            other => panic!("expected Range, got {:?}", other),
        }
    }

    // Helper to extract Range end
    fn range_end(result: &MotionResult) -> usize {
        match result {
            MotionResult::Range { end, .. } => end.get(),
            other => panic!("expected Range, got {:?}", other),
        }
    }

    // ─── Literal pattern tests ──────────────────────────────────────────

    #[test]
    fn gn_finds_next_match() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("foo bar foo baz", 0, "foo", &opts);
        let result = gn(&ctx);
        // Cursor is at start of first "foo" (inside match), returns full range
        assert_eq!(range_start(&result), 0);
        assert_eq!(range_end(&result), 3);
    }

    #[test]
    fn gn_from_outside_match() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("foo bar foo baz", 4, "foo", &opts);
        let result = gn(&ctx);
        // Cursor at 'b' in "bar", next "foo" at 8..11
        assert_eq!(range_start(&result), 8);
        assert_eq!(range_end(&result), 11);
    }

    #[test]
    fn gn_wraps_around() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("foo bar baz", 8, "foo", &opts);
        let result = gn(&ctx);
        // Only "foo" is at 0..3, cursor at 8, wraps
        assert_eq!(range_start(&result), 0);
        assert_eq!(range_end(&result), 3);
    }

    #[test]
    fn gn_no_pattern_fails() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("foo bar", Offset::new(0), 1, &opts);
        assert_eq!(gn(&ctx), MotionResult::Error);
    }

    #[test]
    fn gn_no_match_fails() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("hello world", 0, "xyz", &opts);
        assert_eq!(gn(&ctx), MotionResult::Error);
    }

    #[test]
    fn gn_cursor_inside_match() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("hello world", 1, "hello", &opts);
        let result = gn(&ctx);
        // Cursor at 'e' inside "hello" → full match range 0..5
        assert_eq!(range_start(&result), 0);
        assert_eq!(range_end(&result), 5);
    }

    #[test]
    fn gN_finds_previous_match() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("foo bar foo baz", 12, "foo", &opts);
        let result = gN(&ctx);
        // Cursor at 12, previous "foo" at 8..11
        assert_eq!(range_start(&result), 8);
        assert_eq!(range_end(&result), 11);
    }

    #[test]
    fn gN_wraps_around() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("bar foo baz", 0, "foo", &opts);
        let result = gN(&ctx);
        // Only "foo" at 4..7, cursor at 0, wraps backward
        assert_eq!(range_start(&result), 4);
        assert_eq!(range_end(&result), 7);
    }

    #[test]
    fn gN_cursor_inside_match() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("hello world", 2, "hello", &opts);
        let result = gN(&ctx);
        // Cursor at 'l' inside "hello" → full match range 0..5
        assert_eq!(range_start(&result), 0);
        assert_eq!(range_end(&result), 5);
    }

    #[test]
    fn gn_with_count() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("foo bar foo baz foo", Offset::new(0), 2, &opts)
            .with_search("foo", crate::primitives::Direction::Forward);
        let result = gn(&ctx);
        // Cursor inside first "foo", count=2 → skip current, find 1 more → 8..11
        assert_eq!(range_start(&result), 8);
        assert_eq!(range_end(&result), 11);
    }

    #[test]
    fn gn_empty_text_fails() {
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("", 0, "foo", &opts);
        assert_eq!(gn(&ctx), MotionResult::Error);
    }

    #[test]
    fn cgn_range_is_exact_match() {
        let opts = crate::primitives::VimOptions::default();
        // This verifies that `gn` returns the full match range for operator use
        let ctx = ctx_with_search("hello world hello", 6, "hello", &opts);
        let result = gn(&ctx);
        // From "world" (pos 6), next "hello" is at 12..17
        assert_eq!(
            result,
            MotionResult::Range {
                start: Offset::new(12),
                end: Offset::new(17)
            }
        );
    }

    // ─── Regex pattern tests ────────────────────────────────────────────

    #[test]
    fn gn_regex_digit_sequence() {
        // \d\+ matches one or more digits (magic mode default)
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("abc 123 def 456", 0, "\\d\\+", &opts);
        let result = gn(&ctx);
        // First digit sequence "123" at 4..7
        assert_eq!(range_start(&result), 4);
        assert_eq!(range_end(&result), 7);
    }

    #[test]
    fn gn_regex_digit_cursor_inside() {
        let opts = crate::primitives::VimOptions::default();
        // Cursor at 5 → inside "123" (4..7)
        let ctx = ctx_with_search("abc 123 def 456", 5, "\\d\\+", &opts);
        let result = gn(&ctx);
        assert_eq!(range_start(&result), 4);
        assert_eq!(range_end(&result), 7);
    }

    #[test]
    fn gn_regex_char_class() {
        // [a-z]\+ matches one or more lowercase letters
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("123 abc 456 def", 0, "[a-z]\\+", &opts);
        let result = gn(&ctx);
        // First lowercase run "abc" at 4..7
        assert_eq!(range_start(&result), 4);
        assert_eq!(range_end(&result), 7);
    }

    #[test]
    fn gn_regex_very_magic() {
        // \v\w+ in very-magic mode
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("  hello  world  ", 0, "\\v\\w+", &opts);
        let result = gn(&ctx);
        // First word "hello" at 2..7
        assert_eq!(range_start(&result), 2);
        assert_eq!(range_end(&result), 7);
    }

    #[test]
    fn gn_regex_with_count() {
        // \d\+ with count=2 → skip first digit match, return second
        let opts = crate::primitives::VimOptions::default();
        let ctx = MotionContext::new("abc 123 def 456 ghi", Offset::new(0), 2, &opts)
            .with_search("\\d\\+", crate::primitives::Direction::Forward);
        let result = gn(&ctx);
        // Second digit sequence "456" at 12..15
        assert_eq!(range_start(&result), 12);
        assert_eq!(range_end(&result), 15);
    }

    #[test]
    fn gN_regex_backward() {
        let opts = crate::primitives::VimOptions::default();
        // Cursor at 16 (past "456"), search backward for \d\+
        let ctx = ctx_with_search("abc 123 def 456 ghi", 16, "\\d\\+", &opts);
        let result = gN(&ctx);
        // Previous digit match "456" at 12..15
        assert_eq!(range_start(&result), 12);
        assert_eq!(range_end(&result), 15);
    }

    #[test]
    fn gn_regex_wraps_around() {
        let opts = crate::primitives::VimOptions::default();
        // Cursor past all digit matches, wraps to first
        let ctx = ctx_with_search("123 abc 456", 11, "[0-9]\\+", &opts);
        let result = gn(&ctx);
        // Cursor at 11 (past "456"), no forward match → wraps to "123" at 0..3
        assert_eq!(range_start(&result), 0);
        assert_eq!(range_end(&result), 3);
    }

    #[test]
    fn gn_word_boundary_via_regex() {
        // \<foo\> with VimRegex — word boundaries handled natively
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("foobar foo baz", 0, "\\<foo\\>", &opts);
        let result = gn(&ctx);
        // "foobar" should NOT match \<foo\>, only standalone "foo" at 7..10
        assert_eq!(range_start(&result), 7);
        assert_eq!(range_end(&result), 10);
    }

    #[test]
    fn cgn_regex_selects_exact_match() {
        // Operator-pending: cgn with regex pattern selects the full match range
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("x = 42; y = 99;", 0, "\\d\\+", &opts);
        let result = gn(&ctx);
        // First number "42" at 4..6
        assert_eq!(
            result,
            MotionResult::Range {
                start: Offset::new(4),
                end: Offset::new(6)
            }
        );
    }

    #[test]
    fn gn_regex_alternation() {
        // foo\|bar matches "foo" or "bar"
        let opts = crate::primitives::VimOptions::default();
        let ctx = ctx_with_search("baz bar foo", 0, "foo\\|bar", &opts);
        let result = gn(&ctx);
        // First match: "bar" at 4..7
        assert_eq!(range_start(&result), 4);
        assert_eq!(range_end(&result), 7);
    }
}
