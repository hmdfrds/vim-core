//! Replace character commands (r{char}) — normal, visual, and block visual.
//!
//! Handles all variants of the `r` command:
//! - Normal mode: replace `count` chars starting at cursor
//! - Visual charwise/linewise: replace all non-newline chars in selection
//! - Visual block: replace chars within block columns across lines
//!
//! Implemented as plain functions, not trait methods — no dynamic dispatch.

use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::Mode;
use crate::primitives::{MarkName, MotionType, Offset, Range, RegisterName, SelectionRange};

use super::types::ReplaceCharContext;

/// Execute `r{char}` in the appropriate mode based on context fields.
///
/// Dispatches to counted, visual, or block-visual replace
/// based on the `selection` and `visual_type` fields.
pub fn execute_replace_char(ctx: &ReplaceCharContext<'_>) -> CommandResult {
    use crate::primitives::VisualType;

    match (ctx.selection, ctx.visual_type) {
        (Some(sel), Some(VisualType::Block)) => execute_block_replace(ctx.text, sel, ctx.ch),
        (Some(sel), Some(VisualType::Line)) => execute_visual_replace(ctx.text, sel, ctx.ch, true),
        (Some(sel), _) => execute_visual_replace(ctx.text, sel, ctx.ch, false),
        (None, _) => execute_counted_replace(ctx.text, ctx.cursor, ctx.ch, ctx.count),
    }
}

/// Replace `count` characters at cursor with the replacement char.
///
/// `r{char}` in normal mode with count support.
/// Aborts (returns empty) if any character in the range is a newline,
/// or if there aren't enough characters remaining — matching Vim behavior.
fn execute_counted_replace(text: &str, cursor: Offset, ch: char, count: usize) -> CommandResult {
    let start = cursor.get();
    let mut end = start;
    for _ in 0..count {
        if let Some(c) = text[end..].chars().next() {
            if c == '\n' {
                // Can't replace across newlines — abort like Vim
                return CommandResult::none();
            }
            end += c.len_utf8();
        } else {
            // Not enough characters — abort like Vim
            return CommandResult::none();
        }
    }

    let range = Range::from_raw(start, end);
    let replacement: String = std::iter::repeat_n(ch, count).collect();
    // For newline replacement (r<CR>), cursor goes to start of the next line
    // (first byte after the last newline). For other chars, cursor goes to
    // the last replaced character position.
    let new_cursor = if ch == '\n' {
        Offset::new(start + replacement.len())
    } else {
        Offset::new(start + replacement.len().saturating_sub(ch.len_utf8()))
    };

    // Neovim sets the `.` register to the replacement char(s) after `r`.
    let dot_text: String = std::iter::repeat_n(ch, count).collect();

    // Mark `.` (LAST_CHANGE) should point to the LAST modified character,
    // not the start. sync_change_marks records start; override with the
    // inclusive end position (start + replaced_length - 1).
    let last_modified = Offset::new(start + replacement.len().saturating_sub(ch.len_utf8()));

    let mut open = Effects::new()
        .begin_undo()
        .replace(range, replacement)
        .set_mark(MarkName::LAST_CHANGE, last_modified, None)
        .set_register(RegisterName::LAST_INSERT, dot_text, MotionType::CharWise)
        .set_cursor(new_cursor);
    // For r<CR>, mark ']' should point to the start of the new line (cursor position),
    // not the newline byte itself. Neovim's nv_replace sets b_op_end to the new cursor.
    // Also set mark '^' (INSERT_STOP): Neovim briefly enters insert for the newline,
    // which sets mark '^' to the cursor position after the newline.
    if ch == '\n' {
        open = open
            .set_mark(MarkName::CHANGE_END, new_cursor, None)
            .set_mark(MarkName::INSERT_STOP, new_cursor, None);
    }
    let effects = open.end_undo();
    CommandResult::effects_only(effects)
}

/// Replace all non-newline characters in a visual selection.
///
/// `r{char}` in charwise/linewise visual mode.
/// Newline characters are preserved — only visible characters are replaced.
fn execute_visual_replace(
    text: &str,
    selection: &SelectionRange,
    ch: char,
    is_line_visual: bool,
) -> CommandResult {
    let (start, end) = if is_line_visual {
        // Linewise: expand selection to full line boundaries
        let expanded = crate::commands::visual::expand_selection_to_lines(text, selection);
        (expanded.start().get(), expanded.end().get().min(text.len()))
    } else {
        // Charwise: include the character at the head position (gap indexing → inclusive end)
        let sel_end = selection.end().get().min(text.len());
        let inclusive_end =
            crate::primitives::text_util::next_char_boundary(text, sel_end).min(text.len());
        (selection.start().get(), inclusive_end)
    };

    let original = &text[start..end];
    let replaced: String = original
        .chars()
        .map(|c| if c == '\n' { '\n' } else { ch })
        .collect();

    let mut open = Effects::new().begin_undo();
    if replaced != original {
        open = open.replace(Range::from_raw(start, end), replaced);
    }
    let effects = open
        .clear_selection()
        .set_mode(Mode::Normal)
        .set_cursor(Offset::new(selection.start().get()))
        .end_undo();

    CommandResult::effects_only(effects)
}

/// Replace characters within block visual columns across lines.
///
/// `r{char}` in block visual mode (Ctrl-V).
/// Computes grapheme columns from anchor/head, then replaces each
/// grapheme within those columns on each line with the replacement char.
fn execute_block_replace(text: &str, selection: &SelectionRange, ch: char) -> CommandResult {
    use crate::commands::helpers::{line_end, line_of, line_start};
    use unicode_segmentation::UnicodeSegmentation;

    let anchor = selection.anchor().get();
    let head = selection.head().get();

    let anchor_line = line_of(text, anchor);
    let head_line = line_of(text, head);
    let anchor_ls = line_start(text, anchor_line).unwrap_or(0);
    let head_ls = line_start(text, head_line).unwrap_or(0);

    let anchor_gcol = text[anchor_ls..anchor].graphemes(true).count();
    let head_gcol = text[head_ls..head].graphemes(true).count();

    let top_line = anchor_line.min(head_line);
    let bottom_line = anchor_line.max(head_line);
    let left_gcol = anchor_gcol.min(head_gcol);
    let right_gcol = anchor_gcol.max(head_gcol); // inclusive

    // Compute cursor position (top-left corner)
    let cursor_ls = line_start(text, top_line).unwrap_or(0);
    let cursor_line_end = line_end(text, top_line).unwrap_or(text.len());
    let cursor_line_text = &text[cursor_ls..cursor_line_end];
    let cursor_byte: usize = cursor_line_text
        .graphemes(true)
        .take(left_gcol)
        .map(str::len)
        .sum::<usize>()
        + cursor_ls;

    let mut open = Effects::new().begin_undo();

    // Track the bottom line's right-column byte offset for mark ']'
    let mut bottom_right_byte: usize = cursor_byte;

    // Replace per-line (bottom-to-top to preserve byte offsets)
    for line_idx in (top_line..=bottom_line).rev() {
        let ls = line_start(text, line_idx).unwrap_or(0);
        let le = line_end(text, line_idx).unwrap_or(text.len());
        let line_text = &text[ls..le];
        let graphemes: Vec<&str> = line_text.graphemes(true).collect();
        let total = graphemes.len();

        let eff_left = left_gcol.min(total);
        let eff_right = right_gcol.min(total.saturating_sub(1));

        if eff_left > eff_right || eff_left >= total {
            continue;
        }

        let left_byte: usize = graphemes
            .get(..eff_left)
            .unwrap_or(&[])
            .iter()
            .map(|g| g.len())
            .sum::<usize>()
            + ls;
        let right_byte: usize = graphemes
            .get(..=eff_right)
            .unwrap_or(&[])
            .iter()
            .map(|g| g.len())
            .sum::<usize>()
            + ls;

        // Track bottom line for mark ']' (bottom_line is the first iteration)
        if line_idx == bottom_line {
            // ']' points to the start of the last replaced grapheme on the bottom line
            let last_grapheme_start: usize = graphemes
                .get(..eff_right)
                .unwrap_or(&[])
                .iter()
                .map(|g| g.len())
                .sum::<usize>()
                + ls;
            bottom_right_byte = last_grapheme_start;
        }

        let original = &text[left_byte..right_byte];
        let replaced: String = original.graphemes(true).map(|_| ch).collect();
        if replaced != original {
            open = open.replace(Range::from_raw(left_byte, right_byte), replaced);
        }
    }

    // Neovim sets change marks for block visual replace:
    // '.' and '[' = top-left corner, ']' = bottom-right corner (start of last grapheme).
    // sync_change_marks records the bottom line (reverse iteration), so override all three.
    let effects = open
        .set_mark(
            crate::primitives::MarkName::LAST_CHANGE,
            Offset::new(cursor_byte),
            None,
        )
        .set_mark(
            crate::primitives::MarkName::CHANGE_START,
            Offset::new(cursor_byte),
            None,
        )
        .set_mark(
            crate::primitives::MarkName::CHANGE_END,
            Offset::new(bottom_right_byte),
            None,
        )
        .clear_selection()
        .set_mode(Mode::Normal)
        .set_cursor(Offset::new(cursor_byte))
        .end_undo();

    CommandResult::effects_only(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::Offset;

    /// Helper to build a `ReplaceCharContext` for normal mode.
    fn normal_ctx(text: &str, cursor: usize, ch: char, count: usize) -> ReplaceCharContext<'_> {
        ReplaceCharContext {
            text,
            ch,
            cursor: Offset::new(cursor),
            count,
            selection: None,
            visual_type: None,
        }
    }

    /// Helper to build a `ReplaceCharContext` for visual mode.
    fn visual_ctx<'text>(
        text: &'text str,
        ch: char,
        selection: &'text SelectionRange,
        is_block: bool,
    ) -> ReplaceCharContext<'text> {
        use crate::primitives::VisualType;
        ReplaceCharContext {
            text,
            ch,
            cursor: Offset::new(0),
            count: 1,
            selection: Some(selection),
            visual_type: if is_block {
                Some(VisualType::Block)
            } else {
                Some(VisualType::Char)
            },
        }
    }

    // --- Normal mode tests ---

    #[test]
    fn test_replace_counted_basic() {
        let ctx = normal_ctx("hello", 0, 'x', 1);
        let result = execute_replace_char(&ctx);
        let has_replace = result.effects.iter().any(|e| {
            matches!(
                e, Effect::Replace { range, text, .. }
                if range.start().get() == 0 && range.end().get() == 1 && text.as_str() == "x"
            )
        });
        assert!(has_replace, "Should replace first char with 'x'");
    }

    #[test]
    fn test_replace_counted_multiple() {
        let ctx = normal_ctx("hello", 0, 'x', 3);
        let result = execute_replace_char(&ctx);
        let has_replace = result.effects.iter().any(|e| {
            matches!(
                e, Effect::Replace { range, text, .. }
                if range.start().get() == 0 && range.end().get() == 3 && text.as_str() == "xxx"
            )
        });
        assert!(has_replace, "Should replace 3 chars with 'xxx'");
    }

    #[test]
    fn test_replace_counted_cursor_position() {
        // After replacing 3 chars with 'x', cursor should be on last replaced char
        let ctx = normal_ctx("hello", 0, 'x', 3);
        let result = execute_replace_char(&ctx);
        let has_cursor = result.effects.iter().any(|e| {
            matches!(
                e, Effect::SetCursor { offset } if offset.get() == 2
            )
        });
        assert!(
            has_cursor,
            "Cursor should be on last replaced char (offset 2)"
        );
    }

    #[test]
    fn test_replace_counted_newline_aborts() {
        let ctx = normal_ctx("he\nlo", 1, 'x', 2);
        let result = execute_replace_char(&ctx);
        assert!(result.is_empty(), "Crossing newline must abort");
    }

    #[test]
    fn test_replace_counted_past_end_aborts() {
        let ctx = normal_ctx("hi", 0, 'x', 5);
        let result = execute_replace_char(&ctx);
        assert!(result.is_empty(), "Not enough chars must abort");
    }

    // --- Visual mode tests ---

    #[test]
    fn test_visual_replace_replaces_chars() {
        let sel = SelectionRange::new(Offset::new(0), Offset::new(5));
        let ctx = visual_ctx("hello", 'x', &sel, false);
        let result = execute_replace_char(&ctx);
        let has_replace = result.effects.iter().any(|e| {
            matches!(
                e, Effect::Replace { text, .. } if text.as_str() == "xxxxx"
            )
        });
        assert!(has_replace, "Should replace all chars with 'x'");
    }

    #[test]
    fn test_visual_replace_preserves_newlines() {
        let sel = SelectionRange::new(Offset::new(0), Offset::new(9));
        let ctx = visual_ctx("hel\nworld", 'x', &sel, false);
        let result = execute_replace_char(&ctx);
        let has_replace = result.effects.iter().any(|e| {
            matches!(
                e, Effect::Replace { text, .. } if text.as_str() == "xxx\nxxxxx"
            )
        });
        assert!(has_replace, "Should replace chars but preserve newlines");
    }

    #[test]
    fn test_visual_replace_exits_visual_mode() {
        let sel = SelectionRange::new(Offset::new(0), Offset::new(5));
        let ctx = visual_ctx("hello", 'x', &sel, false);
        let result = execute_replace_char(&ctx);
        let has_clear = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSelection));
        let has_normal = result.effects.iter().any(|e| {
            matches!(
                e,
                Effect::SetMode {
                    mode: Mode::Normal,
                    ..
                }
            )
        });
        assert!(has_clear, "Must clear selection on visual replace");
        assert!(has_normal, "Must return to Normal mode");
    }

    #[test]
    fn test_visual_replace_cursor_at_start() {
        let sel = SelectionRange::new(Offset::new(2), Offset::new(5));
        let ctx = visual_ctx("hello", 'x', &sel, false);
        let result = execute_replace_char(&ctx);
        let has_cursor = result.effects.iter().any(|e| {
            matches!(
                e, Effect::SetCursor { offset } if offset.get() == 2
            )
        });
        assert!(has_cursor, "Cursor should be at start of selection");
    }

    // --- Block visual mode tests ---

    #[test]
    fn test_block_replace_exits_visual_mode() {
        let sel = SelectionRange::new(Offset::new(0), Offset::new(5));
        let ctx = visual_ctx("hello\nworld", 'x', &sel, true);
        let result = execute_replace_char(&ctx);
        let has_clear = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSelection));
        let has_normal = result.effects.iter().any(|e| {
            matches!(
                e,
                Effect::SetMode {
                    mode: Mode::Normal,
                    ..
                }
            )
        });
        assert!(has_clear, "Block replace must clear selection");
        assert!(has_normal, "Block replace must return to Normal mode");
    }

    #[test]
    fn test_block_replace_produces_replace_effects() {
        let sel = SelectionRange::new(Offset::new(0), Offset::new(6));
        let ctx = visual_ctx("hello\nworld", 'x', &sel, true);
        let result = execute_replace_char(&ctx);
        let replace_count = result
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::Replace { .. }))
            .count();
        assert!(
            replace_count > 0,
            "Block replace should produce at least one Replace effect"
        );
    }
}
