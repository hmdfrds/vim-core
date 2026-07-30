//! Indent-block text objects (ii, ai).
//!
//! Inspired by vim-indent-object:
//! - `ii` selects contiguous lines at the same or deeper indent level as the
//!   cursor line (skipping blank lines within the block).
//! - `ai` includes one surrounding less-indented line above and below (if they
//!   exist), similar to how `a(` includes the parentheses themselves.

use super::types::{TextObjectContext, TextObjectRange};
use crate::commands::helpers;
use crate::grammar::types::TextObjectScope;

/// Compute an indent-block text object.
///
/// The algorithm:
/// 1. Determine the indent level of the cursor's line.
/// 2. Expand upward/downward while lines have indent >= cursor indent
///    (blank lines are skipped — they don't break the block).
/// 3. For `ai`, include one less-indented line above and below the block.
///
/// Returns `None` if the buffer is empty.
#[must_use]
pub fn compute_indent_object(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    if text.is_empty() {
        return None;
    }

    let total_lines = helpers::line_count(text);
    if total_lines == 0 {
        return None;
    }

    let cursor = if ctx.cursor.get() >= text.len() && !text.is_empty() {
        crate::primitives::text_util::prev_char_boundary(text, text.len())
    } else {
        ctx.cursor.get()
    };
    let cursor_line = helpers::line_of(text, cursor);
    let cursor_line = cursor_line.min(total_lines.saturating_sub(1));

    let cursor_line_str = helpers::line_content(text, cursor_line)?;
    let base_indent = line_indent_level(cursor_line_str);

    // If cursor is on a blank line, find the nearest non-blank line's indent.
    let base_indent = if cursor_line_str.trim().is_empty() {
        find_nearest_indent(text, total_lines, cursor_line)
    } else {
        base_indent
    };

    let (top, bot) = expand_indent_block(text, total_lines, cursor_line, base_indent);

    // Trim leading/trailing blank lines from the inner block.
    let (top, bot) = trim_blank_edges(text, top, bot);

    // For `ai`, extend to include one surrounding less-indented line.
    let (top, bot) = if scope.is_inner() {
        (top, bot)
    } else {
        extend_around(total_lines, top, bot, true)
    };

    // Convert line indices to byte offsets.
    let start = helpers::line_start(text, top).unwrap_or(0);
    let end = helpers::line_start(text, bot + 1).unwrap_or(text.len());

    Some(TextObjectRange::line(start, end))
}

/// Compute an indent-block text object without extending below (`aI`).
///
/// Identical to [`compute_indent_object`] except that for `Around` scope,
/// only the header line above is included — the line below is NOT included.
/// This matches the vim-indent-object `aI` behavior for braceless languages.
#[must_use]
pub fn compute_indent_object_no_below(
    ctx: &TextObjectContext<'_>,
    scope: TextObjectScope,
) -> Option<TextObjectRange> {
    let text = ctx.text;
    if text.is_empty() {
        return None;
    }

    let total_lines = helpers::line_count(text);
    if total_lines == 0 {
        return None;
    }

    let cursor = if ctx.cursor.get() >= text.len() && !text.is_empty() {
        crate::primitives::text_util::prev_char_boundary(text, text.len())
    } else {
        ctx.cursor.get()
    };
    let cursor_line = helpers::line_of(text, cursor);
    let cursor_line = cursor_line.min(total_lines.saturating_sub(1));

    let cursor_line_str = helpers::line_content(text, cursor_line)?;
    let base_indent = line_indent_level(cursor_line_str);

    // If cursor is on a blank line, find the nearest non-blank line's indent.
    let base_indent = if cursor_line_str.trim().is_empty() {
        find_nearest_indent(text, total_lines, cursor_line)
    } else {
        base_indent
    };

    let (top, bot) = expand_indent_block(text, total_lines, cursor_line, base_indent);

    // Trim leading/trailing blank lines from the inner block.
    let (top, bot) = trim_blank_edges(text, top, bot);

    // For `aI`, extend above only (include_below = false).
    let (top, bot) = if scope.is_inner() {
        (top, bot)
    } else {
        extend_around(total_lines, top, bot, false)
    };

    // Convert line indices to byte offsets.
    let start = helpers::line_start(text, top).unwrap_or(0);
    let end = helpers::line_start(text, bot + 1).unwrap_or(text.len());

    Some(TextObjectRange::line(start, end))
}

/// Expand upward and downward from `cursor_line` while lines have indent >= `base_indent`.
///
/// Accesses lines on demand via `helpers::line_content` instead of materializing
/// the entire document into a Vec.
fn expand_indent_block(
    text: &str,
    total_lines: usize,
    cursor_line: usize,
    base_indent: usize,
) -> (usize, usize) {
    let mut top = cursor_line;
    while top > 0 {
        let candidate = top - 1;
        let Some(line) = helpers::line_content(text, candidate) else {
            break;
        };
        if line.trim().is_empty() || line_indent_level(line) >= base_indent {
            top = candidate;
        } else {
            break;
        }
    }
    let mut bot = cursor_line;
    while bot + 1 < total_lines {
        let candidate = bot + 1;
        let Some(line) = helpers::line_content(text, candidate) else {
            break;
        };
        if line.trim().is_empty() || line_indent_level(line) >= base_indent {
            bot = candidate;
        } else {
            break;
        }
    }
    (top, bot)
}

/// Trim blank lines from the top and bottom edges of the range.
fn trim_blank_edges(text: &str, mut top: usize, mut bot: usize) -> (usize, usize) {
    while top < bot && helpers::line_content(text, top).is_some_and(|l| l.trim().is_empty()) {
        top += 1;
    }
    while bot > top && helpers::line_content(text, bot).is_some_and(|l| l.trim().is_empty()) {
        bot -= 1;
    }
    (top, bot)
}

/// For `ai`/`aI`, extend to include surrounding less-indented lines.
///
/// When `include_below` is `true` (standard `ai`), extends both above and below.
/// When `include_below` is `false` (`aI`), extends only above (header line).
const fn extend_around(
    total_lines: usize,
    top: usize,
    bot: usize,
    include_below: bool,
) -> (usize, usize) {
    let new_top = if top > 0 { top - 1 } else { top };
    let new_bot = if include_below && bot + 1 < total_lines {
        bot + 1
    } else {
        bot
    };
    (new_top, new_bot)
}

/// Find the nearest non-blank line's indent level (searching up then down).
fn find_nearest_indent(text: &str, total_lines: usize, from: usize) -> usize {
    // Search upward first.
    for i in (0..from).rev() {
        if let Some(line) = helpers::line_content(text, i) {
            if !line.trim().is_empty() {
                return line_indent_level(line);
            }
        }
    }
    // Then downward.
    for i in (from + 1)..total_lines {
        if let Some(line) = helpers::line_content(text, i) {
            if !line.trim().is_empty() {
                return line_indent_level(line);
            }
        }
    }
    0
}

/// Compute the indent level of a line (number of leading whitespace columns).
///
/// Tabs count as 1 unit of indent (matching vim-indent-object behavior where
/// indent level comparison is purely by leading whitespace byte length).
fn line_indent_level(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, cursor: usize, inner: bool, expected: Option<(usize, usize)>) {
        let ctx = TextObjectContext::new(text, cursor);
        let scope = TextObjectScope::from_inner_flag(inner);
        let result = compute_indent_object(&ctx, scope);
        match (result, expected) {
            (Some(r), Some((s, e))) => {
                assert_eq!(
                    r.start(),
                    s,
                    "start mismatch for {:?} cursor={}",
                    text,
                    cursor
                );
                assert_eq!(r.end(), e, "end mismatch for {:?} cursor={}", text, cursor);
                assert!(r.linewise, "indent object should be linewise");
            }
            (None, None) => {}
            _ => panic!(
                "result {:?} != expected {:?} for {:?} cursor={}",
                result, expected, text, cursor
            ),
        }
    }

    // ── ii (inner indent) ────────────────────────────────────────────────

    #[test]
    fn ii_simple_indented_block() {
        let text = "if true:\n    a\n    b\n    c\nend";
        // Cursor on "    a" (line 1), indent=4.
        // Lines 1-3 have indent >= 4.
        // ii should select lines 1-3: "    a\n    b\n    c\n"
        let cursor = 10; // start of "    a"
        check(text, cursor, true, Some((9, 27)));
    }

    #[test]
    fn ii_single_indented_line() {
        let text = "top\n    indented\nbottom";
        let cursor = 4; // start of "    indented"
        check(text, cursor, true, Some((4, 17)));
    }

    #[test]
    fn ii_no_indent() {
        // All lines at indent 0 — the whole file forms one block.
        let text = "a\nb\nc";
        let cursor = 0;
        check(text, cursor, true, Some((0, 5)));
    }

    #[test]
    fn ii_empty_text() {
        check("", 0, false, None);
    }

    #[test]
    fn ii_deeper_indent_included() {
        let text = "top\n    a\n        deeper\n    b\nbottom";
        // Cursor on "    a" (indent=4). "        deeper" has indent=8 >= 4, included.
        let cursor = 4; // "    a"
        check(text, cursor, true, Some((4, 31)));
    }

    #[test]
    fn ii_blank_lines_within_block() {
        let text = "top\n    a\n\n    b\nbottom";
        // Blank line should not break the block.
        let cursor = 4; // "    a"
        check(text, cursor, true, Some((4, 17)));
    }

    // ── ai (around indent) ───────────────────────────────────────────────

    #[test]
    fn ai_includes_surrounding_lines() {
        let text = "if true:\n    a\n    b\n    c\nend";
        let cursor = 10; // "    a"
                         // ii is lines 1-3 ("    a\n    b\n    c\n"), ai adds line 0 and line 4.
        check(text, cursor, false, Some((0, text.len())));
    }

    #[test]
    fn ai_at_top_of_file() {
        let text = "    a\n    b\nbottom";
        let cursor = 0; // "    a"
                        // ii is lines 0-1, ai extends bot to include line 2.
        check(text, cursor, false, Some((0, text.len())));
    }

    #[test]
    fn ai_at_bottom_of_file() {
        let text = "top\n    a\n    b";
        let cursor = 4; // "    a"
                        // ii is lines 1-2, ai extends top to include line 0.
        check(text, cursor, false, Some((0, text.len())));
    }

    // ── cursor on blank line ─────────────────────────────────────────────

    #[test]
    fn ii_cursor_on_blank_line_within_block() {
        let text = "top\n    a\n\n    b\nbottom";
        // Cursor on the blank line (byte 9 = '\n' on the blank line).
        let cursor = 9;
        // Nearest non-blank indent is 4 (from "    a" above). Block = lines 1-3.
        check(text, cursor, true, Some((4, 17)));
    }

    // ── indent level helpers ─────────────────────────────────────────────

    #[test]
    fn test_line_indent_level() {
        assert_eq!(line_indent_level("hello"), 0);
        assert_eq!(line_indent_level("    hello"), 4);
        assert_eq!(line_indent_level("\thello"), 1);
        assert_eq!(line_indent_level("  \thello"), 3);
        assert_eq!(line_indent_level(""), 0);
        assert_eq!(line_indent_level("   "), 3);
    }

    // ── aI (around indent, no below) ────────────────────────────────────

    fn check_no_below(text: &str, cursor: usize, inner: bool, expected: Option<(usize, usize)>) {
        let ctx = TextObjectContext::new(text, cursor);
        let scope = TextObjectScope::from_inner_flag(inner);
        let result = compute_indent_object_no_below(&ctx, scope);
        match (result, expected) {
            (Some(r), Some((s, e))) => {
                assert_eq!(
                    r.start(),
                    s,
                    "start mismatch for {:?} cursor={}",
                    text,
                    cursor
                );
                assert_eq!(r.end(), e, "end mismatch for {:?} cursor={}", text, cursor);
                assert!(r.linewise, "indent object should be linewise");
            }
            (None, None) => {}
            _ => panic!(
                "result {:?} != expected {:?} for {:?} cursor={}",
                result, expected, text, cursor
            ),
        }
    }

    #[test]
    fn ai_no_below_includes_header_but_not_below() {
        // def foo():
        //     bar
        //     baz
        // next_line
        let text = "def foo():\n    bar\n    baz\nnext_line";
        // Cursor on "    bar" (line 1)
        let cursor = 11; // start of "    bar"
                         // ai selects lines 0-3 (header + block + below)
        check(text, cursor, false, Some((0, text.len())));
        // aI selects lines 0-2 (header + block, NOT next_line)
        // Lines 0-2 end at byte 27 (start of line 3)
        check_no_below(text, cursor, false, Some((0, 27)));
    }

    #[test]
    fn ai_no_below_cursor_on_second_line() {
        let text = "def foo():\n    bar\n    baz\nnext_line";
        // Cursor on "    baz" (line 2)
        let cursor = 19; // start of "    baz"
                         // aI selects lines 0-2 (header + block, NOT next_line)
        check_no_below(text, cursor, false, Some((0, 27)));
    }

    #[test]
    fn ii_no_below_same_as_ii() {
        // iI should behave identically to ii (inner doesn't use include_below)
        let text = "def foo():\n    bar\n    baz\nnext_line";
        let cursor = 11; // "    bar"
                         // ii selects lines 1-2: "    bar\n    baz\n" → end at byte 27
        check_no_below(text, cursor, true, Some((11, 27)));
        check(text, cursor, true, Some((11, 27)));
    }

    #[test]
    fn ai_no_below_at_bottom_of_file() {
        // When the block is at the bottom, there's nothing below anyway.
        let text = "top\n    a\n    b";
        let cursor = 4; // "    a"
                        // ai extends top to include line 0, but bot is already at end.
                        // aI: same result since there's nothing below to exclude.
        check_no_below(text, cursor, false, Some((0, text.len())));
    }

    #[test]
    fn ai_no_below_at_top_of_file() {
        // Block at top: no header above, but there IS a line below.
        // aI should NOT include the line below.
        let text = "    a\n    b\nbottom";
        let cursor = 0; // "    a"
                        // ii is lines 0-1: "    a\n    b\n"
                        // ai extends top (already 0) and bot to include "bottom" → full file
                        // aI extends top (already 0) but does NOT extend below → lines 0-1
        check_no_below(text, cursor, false, Some((0, 12)));
    }

    // ── Additional edge cases for aI ────────────────────────────────────

    #[test]
    fn ai_no_below_single_line_file() {
        // File with only one line — no indent possible, but should not panic.
        let text = "only line";
        let cursor = 0;
        // The entire file is one block at indent 0.
        // aI around: top can't extend (already 0), bot can't extend (only 1 line).
        check_no_below(text, cursor, false, Some((0, text.len())));
        check_no_below(text, cursor, true, Some((0, text.len())));
    }

    #[test]
    fn ai_no_below_single_indented_line_file() {
        // Single indented line — edge case where file has one line with indent.
        let text = "    indented";
        let cursor = 0;
        check_no_below(text, cursor, false, Some((0, text.len())));
        check_no_below(text, cursor, true, Some((0, text.len())));
    }

    #[test]
    fn ii_no_below_vs_ii_consistency() {
        // iI must always produce the same result as ii (inner scope ignores include_below).
        let text = "header\n    line1\n    line2\n    line3\nfooter";
        let cursor = 7; // "    line1"
        let ctx_ii = TextObjectContext::new(text, cursor);
        let ctx_iI = TextObjectContext::new(text, cursor);
        let ii_result = compute_indent_object(&ctx_ii, TextObjectScope::Inner);
        let iI_result = compute_indent_object_no_below(&ctx_iI, TextObjectScope::Inner);
        assert_eq!(
            ii_result.map(|r| (r.start(), r.end())),
            iI_result.map(|r| (r.start(), r.end())),
            "iI (inner) must match ii (inner) exactly"
        );
    }

    #[test]
    fn ai_no_below_cursor_on_blank_line_within_block() {
        // Cursor on a blank line between indented lines — aI should still
        // include the header above but NOT the line below.
        let text = "def foo():\n    a\n\n    b\nnext";
        // Line 0: "def foo():" starts at 0, len=11 (incl \n)
        // Line 1: "    a" starts at 11, len=6 (incl \n)
        // Line 2: "" (blank) starts at 17, len=1 (just \n)
        // Line 3: "    b" starts at 18, len=6 (incl \n)
        // Line 4: "next" starts at 24, len=4
        // Cursor on blank line 2 at byte 17.
        let cursor = 17;
        // Nearest non-blank indent is 4. Block expands to lines 1-3.
        // trim_blank_edges: top=1 (not blank), bot=3 (not blank) → still 1-3.
        // aI: extend_around(5, 1, 3, false) → new_top=0, new_bot=3
        // Lines 0-3: start=0, end=start of line 4=24
        check_no_below(text, cursor, false, Some((0, 24)));
    }

    #[test]
    fn ai_no_below_mixed_tabs_and_spaces() {
        // Mixed indentation: tabs and spaces. The indent level is purely byte-length
        // of leading whitespace, so \t counts as 1 byte, spaces count as 1 each.
        let text = "header\n\tline1\n\tline2\nfooter";
        // \t = 1 byte indent. Cursor on "\tline1" at byte 7.
        let cursor = 7;
        // Block is lines 1-2 (indent >= 1). aI: header included, footer NOT.
        // Line 0: "header\n" = 7 bytes → line 1 starts at 7
        // Line 1: "\tline1\n" = 7 bytes → line 2 starts at 14
        // Line 2: "\tline2\n" = 7 bytes → line 3 starts at 21
        // aI: lines 0-2, end = start of line 3 = 21
        check_no_below(text, cursor, false, Some((0, 21)));
    }

    #[test]
    fn ai_no_below_mixed_indent_spaces_and_tab() {
        // "  " (2 spaces) and "\t" (1 tab = 1 byte) have different indent levels.
        // Cursor on "  a" (indent 2). "\tb" has indent 1 < 2, so it breaks the block.
        let text = "header\n  a\n  b\n\tc\nfooter";
        let cursor = 7; // "  a"
                        // Block: lines 1-2 (indent >= 2). "\tc" has indent 1, stops expansion.
                        // aI: extend above to line 0, do NOT extend below.
                        // Line 0: "header\n" = 7 bytes
                        // Line 1: "  a\n" = 4 bytes → starts at 7, ends at 11
                        // Line 2: "  b\n" = 4 bytes → starts at 11, ends at 15
                        // aI range: lines 0-2, start=0, end=15
        check_no_below(text, cursor, false, Some((0, 15)));
    }

    #[test]
    fn ai_no_below_extends_above_even_when_top_is_zero() {
        // Verify that include_below=false does NOT accidentally prevent extending above.
        // When top > 0, it should still extend above.
        let text = "if cond:\n    body1\n    body2\nelse:";
        let cursor = 9; // "    body1"
                        // Block: lines 1-2 (indent 4).
                        // aI: extend above to line 0 (include header), do NOT extend below to "else:".
                        // Line 0: "if cond:\n" = 9 bytes
                        // Line 1: "    body1\n" = 10 bytes → starts at 9
                        // Line 2: "    body2\n" = 10 bytes → starts at 19, ends at 29
                        // aI: lines 0-2, start=0, end=29
        check_no_below(text, cursor, false, Some((0, 29)));
        // Verify ai DOES include below:
        check(text, cursor, false, Some((0, text.len())));
    }

    #[test]
    fn ai_no_below_empty_returns_none() {
        // Empty text should return None for both variants.
        check_no_below("", 0, false, None);
        check_no_below("", 0, true, None);
    }

    #[test]
    fn ai_no_below_all_blank_lines() {
        // A file of only blank lines. find_nearest_indent returns 0.
        // The block expands to the entire file, but trim_blank_edges collapses to a single line.
        let text = "\n\n\n";
        let cursor = 0;
        // All lines are blank. find_nearest_indent returns 0 (base indent).
        // expand_indent_block: all lines have indent >= 0 (blank lines included).
        // trim_blank_edges: trims all edges since all are blank → top=bot=some middle line.
        // The result depends on how many lines collapse. With 3 newlines there
        // are 4 lines (0,1,2 are "\n" and line 3 is ""), but the exact
        // line_count is up to the helpers implementation, so this is a
        // does-not-panic test.
        let ctx = TextObjectContext::new(text, cursor);
        let scope = TextObjectScope::from_inner_flag(false);
        let _ = compute_indent_object_no_below(&ctx, scope);
        // Passing means the call did not panic.
    }

    #[test]
    fn ai_no_below_deeply_nested() {
        // Deep nesting: aI on the innermost level should include only its header.
        let text = "class:\n  method:\n    body\n  other:\n    stuff";
        // Cursor on "    body" (line 2, indent 4).
        // Line 0: "class:" starts at 0, len=7 (incl \n)
        // Line 1: "  method:" starts at 7, len=10 (incl \n)
        // Line 2: "    body" starts at 17, len=9 (incl \n)
        // Line 3: "  other:" starts at 26, len=9 (incl \n)
        // Line 4: "    stuff" starts at 35, len=9
        //
        // expand from line 2 (indent=4): up: line 1 has indent 2 < 4, stops. down: line 3 indent 2 < 4, stops.
        // Block = line 2 only.
        // aI: extend above to line 1. Do NOT extend below.
        // Range: lines 1-2, start=7, end=start of line 3=26
        let cursor = 17;
        check_no_below(text, cursor, false, Some((7, 26)));
    }

    #[test]
    fn ai_no_below_cursor_on_header_line_itself() {
        // When cursor is on the header line (the less-indented line above a block),
        // the cursor's indent level is the header's indent. The block at that indent
        // level includes ALL lines at indent >= header_indent.
        //
        // Example: cursor on "def foo():" (indent 0).
        // All lines have indent >= 0, so the block is the entire file.
        let text = "def foo():\n    bar\n    baz\nnext";
        let cursor = 0; // on "def foo():" (indent 0)
                        // Block: all lines (indent >= 0 is always true).
                        // iI: entire file
        check_no_below(text, cursor, true, Some((0, text.len())));
        // aI: top can't extend (already 0), bot can't extend (include_below=false,
        // but bot is already at last line anyway). Same as iI.
        check_no_below(text, cursor, false, Some((0, text.len())));
    }

    #[test]
    fn ai_no_below_cursor_on_header_with_content_below_block() {
        // More targeted: cursor on a mid-level header with distinct blocks below.
        // "outer\n  header:\n    a\n    b\n  sibling\nouter_end"
        // Cursor on "  header:" (indent 2). Block at indent >= 2: lines 1-4.
        // ("  header:", "    a", "    b", "  sibling" all have indent >= 2).
        // "outer_end" has indent 0 < 2, stops expansion.
        let text = "outer\n  header:\n    a\n    b\n  sibling\nouter_end";
        let cursor = 6; // "  header:" starts at byte 6
                        // Block: lines 1-4 (indent >= 2).
                        // iI: lines 1-4. Line 5 starts at byte 38.
        check_no_below(text, cursor, true, Some((6, 38)));
        // aI: extend above to line 0 ("outer"), do NOT extend below.
        check_no_below(text, cursor, false, Some((0, 38)));
    }

    // ── Python/GDScript/braceless language stress tests ──────────────────

    #[test]
    fn ai_no_below_python_multiline_function() {
        // Full Python function with multiple statements, followed by another function.
        // aI on the body should select header + body, NOT the next def.
        let text = "def greet(name):\n    msg = 'hello'\n    print(msg)\n    return msg\ndef farewell():\n    pass";
        // Cursor on "    msg = 'hello'" (line 1, byte 17)
        let cursor = 17;
        // Block: lines 1-3 (indent=4). Line 4 "def farewell():" has indent 0 < 4.
        // aI: header (line 0) + body (lines 1-3), NOT next function.
        // Line 0: "def greet(name):\n" = 17 bytes
        // Line 1: "    msg = 'hello'\n" = 18 bytes → starts at 17
        // Line 2: "    print(msg)\n" = 15 bytes → starts at 35
        // Line 3: "    return msg\n" = 15 bytes → starts at 50, ends at 65
        // aI: lines 0-3, start=0, end=65
        check_no_below(text, cursor, false, Some((0, 65)));
    }

    #[test]
    fn ai_no_below_gdscript_signal_and_function() {
        // GDScript: func body followed by another func.
        let text = "func _ready():\n\tconnect(\"sig\", self)\n\tset_process(true)\nfunc _process(dt):\n\tmove(dt)";
        // Cursor on "\tconnect..." (line 1, byte 15)
        let cursor = 15;
        // Block: lines 1-2 (indent=1, tab). Line 3 "func _process" has indent 0 < 1.
        // aI: line 0 (header) + lines 1-2, NOT anything after.
        // Line 0: "func _ready():\n" = 15 bytes
        // Line 1: "\tconnect(\"sig\", self)\n" = 22 bytes → starts at 15, ends at 37
        // Line 2: "\tset_process(true)\n" = 19 bytes → starts at 37, ends at 56
        // aI: lines 0-2, start=0, end=56
        check_no_below(text, cursor, false, Some((0, 56)));
    }

    #[test]
    fn ai_no_below_entire_file_uniform_indent() {
        // The entire file is at indent level 4 with no less-indented header.
        // aI cannot extend above (top is already 0) and does not extend below.
        let text = "    alpha\n    beta\n    gamma\n    delta";
        let cursor = 10; // "    beta" (line 1)
                         // Block = entire file (all lines indent >= 4).
                         // aI: top=0 can't go higher, bot at last line, no extend below.
                         // Result = entire file.
        check_no_below(text, cursor, false, Some((0, text.len())));
        // ii should also be the entire file.
        check_no_below(text, cursor, true, Some((0, text.len())));
    }

    #[test]
    fn ai_no_below_successive_independent_blocks() {
        // Two successive indented blocks under different headers.
        // aI on the first block should NOT include the second header.
        let text = "if a:\n    x\nif b:\n    y";
        // Cursor on "    x" (line 1, byte 6)
        let cursor = 6;
        // Block: line 1 (indent=4). Line 2 "if b:" has indent 0 < 4, stops.
        // aI: extend above to line 0, do NOT extend below.
        // Line 0: "if a:\n" = 6 bytes
        // Line 1: "    x\n" = 6 bytes → starts at 6, ends at 12
        // aI: lines 0-1, start=0, end=12
        check_no_below(text, cursor, false, Some((0, 12)));
    }

    #[test]
    fn ai_no_below_contrasted_with_ai() {
        // Directly contrast ai vs aI: same text, same cursor.
        // ai includes below, aI does not.
        let text = "while True:\n    do_stuff()\n    check()\nnot_related()";
        let cursor = 12; // "    do_stuff()" (line 1)
                         // Block: lines 1-2 (indent 4).
                         // ai: extends above (line 0) AND below (line 3) → entire file.
        check(text, cursor, false, Some((0, text.len())));
        // aI: extends above (line 0) but NOT below → lines 0-2.
        // Line 0: "while True:\n" = 12 bytes
        // Line 1: "    do_stuff()\n" = 15 bytes → starts 12, ends 27
        // Line 2: "    check()\n" = 12 bytes → starts 27, ends 39
        // aI range: start=0, end=39
        check_no_below(text, cursor, false, Some((0, 39)));
    }
}
