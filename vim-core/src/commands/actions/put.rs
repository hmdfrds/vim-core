//! Put command (p/P).
//!
//! Pastes register content into the document.
//!
//! # Behavior
//!
//! | Command | Action |
//! |---------|--------|
//! | `p` | Put after cursor |
//! | `P` | Put before cursor |
//! | `gp` | Put after, cursor after inserted text |
//! | `gP` | Put before, cursor after inserted text |
//!
//! # Paste Behavior by Motion Type
//!
//! - **`CharWise`**: Insert inline at cursor position
//! - **`LineWise`**: Insert as new line(s) above/below
//! - **`BlockWise`**: Insert as block (TODO)

use std::borrow::Cow;

use super::types::ActionContext;
use crate::commands::helpers::{
    first_non_blank_in_line, line_end, line_of, line_start, prev_char_boundary,
};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::errors::VimError;
use crate::primitives::RegisterContent;
use crate::primitives::{MotionType, Offset, RegisterName};

/// Put after cursor (p command).
///
/// Per Vim spec:
/// - `CharWise`: insert after cursor
/// - `LineWise`: insert on new line below
pub fn put_after(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(content) = ctx.register_content else {
        // Neovim sets change marks to cursor position even with empty register
        let effects = Effects::new()
            .set_mark(crate::primitives::MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(crate::primitives::MarkName::CHANGE_END, ctx.cursor, None);
        return CommandResult::new(effects, ctx.cursor);
    };
    let text = repeat_text(content.text(), ctx.count);
    let text_len = ctx.text.len();

    match content.motion_type() {
        MotionType::CharWise => {
            // Insert after cursor char (need to skip full char, not just +1 byte)
            // On an empty line (cursor on \n), don't advance past the newline —
            // Vim pastes ON the empty line, not on the next line.
            let cursor_pos = ctx.cursor.get();
            let on_newline = ctx.text.as_bytes().get(cursor_pos) == Some(&b'\n');
            let next_char = if cursor_pos < ctx.text.len() && !on_newline {
                // Advance past the current character
                let ch = ctx.text[cursor_pos..].chars().next();
                cursor_pos + ch.map_or(1, char::len_utf8)
            } else {
                cursor_pos
            };
            let insert_at = Offset::new(next_char.min(text_len));
            // Cursor positioning per Vim behavior:
            // - Single-line paste: cursor on last char of pasted text
            // - Multi-line paste: cursor on first char of pasted text
            let new_cursor = if text.is_empty() {
                insert_at
            } else if text.contains('\n') {
                // Multi-line charwise paste: cursor at start of pasted text
                insert_at
            } else {
                // Single-line: cursor at last char of pasted text
                insert_at.saturating_add_raw(prev_char_boundary(&text, text.len()))
            };

            // Neovim sets mark.] to the first byte of the last pasted character.
            // For ASCII this equals `text.len() - 1`; for multi-byte chars we
            // must subtract the full character width. Empty text (no last
            // character) leaves the mark at the insertion point.
            let mark_end = text.chars().next_back().map_or(insert_at, |last| {
                insert_at.saturating_add_raw(text.len() - last.len_utf8())
            });
            let effects = Effects::new()
                .begin_undo_force_entry()
                .insert(insert_at, &*text)
                .set_mark(crate::primitives::MarkName::CHANGE_END, mark_end, None)
                .set_cursor(new_cursor)
                .end_undo();

            CommandResult::new(effects, new_cursor)
        }
        MotionType::LineWise => {
            // Handle insert position and text based on whether we're on the last line
            let (insert_at, text, line_start_in_new_doc) =
                if let Some(next_start) = ctx.next_line_start {
                    // Not on last line - insert at start of next line
                    // Ensure text ends with newline
                    (next_start, ensure_trailing_newline(&text), next_start.get())
                } else {
                    // On last line - insert at end of document
                    // Prepend newline, strip trailing newline since we're at doc end
                    let insert_pos = ctx.line_end;
                    let mut final_text = String::from("\n");
                    final_text.push_str(text.trim_end_matches('\n'));
                    let line_start = insert_pos.next().get(); // After the prepended newline
                    (insert_pos, Cow::Owned(final_text), line_start)
                };

            // Cursor goes to first non-blank of first inserted line (Vim behavior)
            // Extract just the first line of inserted text to find first non-blank
            let first_line = (*text).split('\n').next().unwrap_or("");
            // Skip leading newline if present (for EOF case)
            let first_line = first_line.trim_start_matches('\n');
            let first_non_blank_offset =
                first_non_blank_in_line(if ctx.next_line_start.is_some() {
                    first_line
                } else {
                    // For EOF case, the text starts with \n, so get content after that
                    text.strip_prefix('\n')
                        .unwrap_or(&text)
                        .split('\n')
                        .next()
                        .unwrap_or("")
                });
            let new_cursor = Offset::new(line_start_in_new_doc + first_non_blank_offset);

            // For the EOF case the inserted text is prepended with '\n', so
            // mark.[ and mark.. must point past that separator to the first
            // content character, not to the '\n' itself.
            let mark_start = Offset::new(line_start_in_new_doc);

            let mark_end_offset = linewise_mark_end(insert_at, &text);
            let effects = Effects::new()
                .begin_undo_force_entry()
                .insert(insert_at, &*text)
                .set_mark(crate::primitives::MarkName::CHANGE_START, mark_start, None)
                .set_mark(
                    crate::primitives::MarkName::CHANGE_END,
                    mark_end_offset,
                    None,
                )
                .set_mark(crate::primitives::MarkName::LAST_CHANGE, mark_start, None)
                .set_cursor(new_cursor)
                .end_undo();

            CommandResult::new(effects, new_cursor)
        }
        MotionType::BlockWise => block_put(ctx, content, false),
    }
}

/// Put before cursor (P command).
///
/// Per Vim spec:
/// - `CharWise`: insert at cursor
/// - `LineWise`: insert on new line above
pub fn put_before(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(content) = ctx.register_content else {
        // Neovim sets change marks to cursor position even with empty register
        let effects = Effects::new()
            .set_mark(crate::primitives::MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(crate::primitives::MarkName::CHANGE_END, ctx.cursor, None);
        return CommandResult::new(effects, ctx.cursor);
    };
    let text = repeat_text(content.text(), ctx.count);

    match content.motion_type() {
        MotionType::CharWise => {
            // Insert at cursor position
            let insert_at = ctx.cursor;
            // Cursor positioning per Vim behavior:
            // - Single-line paste: cursor on last char of pasted text
            // - Multi-line paste: cursor on first char of pasted text
            let new_cursor = if text.is_empty() || text.contains('\n') {
                insert_at
            } else {
                insert_at.saturating_add_raw(prev_char_boundary(&text, text.len()))
            };

            // Neovim sets mark.] to the first byte of the last pasted character.
            // Empty text (no last character) leaves the mark at the insertion
            // point.
            let mark_end = text.chars().next_back().map_or(insert_at, |last| {
                insert_at.saturating_add_raw(text.len() - last.len_utf8())
            });
            let effects = Effects::new()
                .begin_undo_force_entry()
                .insert(insert_at, &*text)
                .set_mark(crate::primitives::MarkName::CHANGE_END, mark_end, None)
                .set_cursor(new_cursor)
                .end_undo();

            CommandResult::new(effects, new_cursor)
        }
        MotionType::LineWise => {
            // Insert on new line above current line
            let insert_at = ctx.line_start;

            // Ensure text ends with newline for linewise paste
            let text = ensure_trailing_newline(&text);

            // Cursor goes to first non-blank of first inserted line (Vim behavior)
            let first_line = text.split('\n').next().unwrap_or("");
            let fnb = crate::commands::helpers::first_non_blank_in_line(first_line);
            let new_cursor = insert_at.saturating_add_raw(fnb);

            let mark_end_offset = linewise_mark_end(insert_at, &text);
            let effects = Effects::new()
                .begin_undo_force_entry()
                .insert(insert_at, &*text)
                .set_mark(
                    crate::primitives::MarkName::CHANGE_END,
                    mark_end_offset,
                    None,
                )
                .set_cursor(new_cursor)
                .end_undo();

            CommandResult::new(effects, new_cursor)
        }
        MotionType::BlockWise => block_put(ctx, content, true),
    }
}

/// Repeat text `count` times. Borrows for count <= 1, allocates otherwise.
fn repeat_text(text: &str, count: u32) -> Cow<'_, str> {
    if count <= 1 {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(text.repeat(count as usize))
    }
}

/// Ensure text ends with a newline (for linewise paste).
///
/// Returns `Cow::Borrowed` when text already ends with `\n` (zero allocation).
fn ensure_trailing_newline(text: &str) -> Cow<'_, str> {
    if text.ends_with('\n') {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(format!("{text}\n"))
    }
}

/// Compute mark `']` offset for linewise paste.
///
/// In Neovim, `b_op_end` is set to `(last_inserted_line, max(strlen-1, 0))`.
/// After `ml_append` the cursor is at (last_line, strlen(last_line)).
/// Then `if (col > 0) col--`.
fn linewise_mark_end(insert_at: Offset, text: &str) -> Offset {
    if text.ends_with('\n') && text.len() >= 2 {
        let before_nl = &text[..text.len() - 1];
        // Find the last line in before_nl (after the last '\n')
        let last_line_start = before_nl.rfind('\n').map_or(0, |p| p + 1);
        let last_line = &before_nl[last_line_start..];
        if last_line.is_empty() {
            // Blank last line: col = 0
            insert_at.saturating_add_raw(last_line_start)
        } else {
            // Non-blank last line: col = strlen - 1 (first byte of last char)
            let last_char_len = last_line.chars().next_back().map_or(1, char::len_utf8);
            insert_at.saturating_add_raw(last_line_start + last_line.len() - last_char_len)
        }
    } else if !text.is_empty() {
        let last_char_len = text.chars().next_back().map_or(1, char::len_utf8);
        insert_at.saturating_add_raw(text.len() - last_char_len)
    } else {
        insert_at
    }
}

/// Block paste implementation shared by `p` and `P`.
///
/// Splits register text into lines, then inserts each fragment at the same
/// column on successive document lines. If `before` is true, insert at the
/// cursor column (P); otherwise insert after the cursor column (p).
///
/// Insertions are done bottom-to-top to preserve byte offsets for preceding lines.
fn block_put(ctx: &ActionContext<'_>, content: &RegisterContent, before: bool) -> CommandResult {
    use unicode_segmentation::UnicodeSegmentation;

    let text = ctx.text;
    let cursor_pos = ctx.cursor.get();
    let cursor_line = line_of(text, cursor_pos);
    let cursor_ls = line_start(text, cursor_line).unwrap_or(0);

    // Grapheme column of cursor
    let cursor_gcol = text[cursor_ls..cursor_pos].graphemes(true).count();
    // For `p`, insert after cursor column (only if there's a char at cursor);
    // for `P`, insert at cursor column.
    let cursor_line_end = line_end(text, cursor_line).unwrap_or(text.len());
    let cursor_line_gcols = text[cursor_ls..cursor_line_end].graphemes(true).count();
    let insert_gcol = if before || cursor_gcol >= cursor_line_gcols {
        cursor_gcol
    } else {
        cursor_gcol + 1
    };

    // Split register content into block lines
    let block_lines_vec: Vec<&str> = content.text().split('\n').collect();
    // Trim trailing empty element from split (if register text ends with \n)
    let block_lines = if block_lines_vec.last() == Some(&"") && block_lines_vec.len() > 1 {
        block_lines_vec
            .get(..block_lines_vec.len() - 1)
            .unwrap_or(&block_lines_vec)
    } else {
        &block_lines_vec[..]
    };

    let total_doc_lines = text.matches('\n').count() + 1;
    let mut effects = Effects::new().begin_undo_force_entry();

    // We need to figure out how many new lines to append at the end
    let lines_needed = cursor_line + block_lines.len();
    let lines_to_add = lines_needed.saturating_sub(total_doc_lines);

    // Append blank lines at end of document if needed
    if lines_to_add > 0 {
        let newlines = "\n".repeat(lines_to_add);
        effects = effects.insert(Offset::new(text.len()), &newlines);
    }

    // Insert block fragments bottom-to-top to preserve byte offsets
    // We need to compute per-line insert positions on the ORIGINAL text
    // (before our blank-line additions), then apply bottom-to-top
    let mut inserts: Vec<(usize, String)> = Vec::new();

    for (i, block_text) in block_lines.iter().enumerate() {
        let doc_line = cursor_line + i;
        if doc_line >= total_doc_lines {
            // This line was just appended — it's empty, so just insert at its start
            // The start of newly added lines: original text.len() + newlines accumulated
            let new_line_offset = text.len() + (doc_line - total_doc_lines + 1);
            // Pad with spaces to reach insert_gcol, then insert block text
            let padding = " ".repeat(insert_gcol);
            inserts.push((new_line_offset, format!("{padding}{block_text}")));
        } else {
            let ls = line_start(text, doc_line).unwrap_or(0);
            let le = line_end(text, doc_line).unwrap_or(text.len());
            let line_text = &text[ls..le];
            let graphemes: Vec<&str> = line_text.graphemes(true).collect();
            let line_gcol_count = graphemes.len();

            if insert_gcol <= line_gcol_count {
                // Column exists — compute byte offset
                let byte_offset: usize = graphemes
                    .get(..insert_gcol)
                    .unwrap_or(&[])
                    .iter()
                    .map(|g| g.len())
                    .sum::<usize>()
                    + ls;
                inserts.push((byte_offset, block_text.to_string()));
            } else {
                // Line too short — pad with spaces
                let padding = " ".repeat(insert_gcol - line_gcol_count);
                inserts.push((le, format!("{padding}{block_text}")));
            }
        }
    }

    // Apply bottom-to-top
    for (offset, insert_text) in inserts.iter().rev() {
        effects = effects.insert(Offset::new(*offset), insert_text);
    }

    // Cursor goes to the first inserted position (top of block)
    let new_cursor = if let Some((first_offset, _)) = inserts.first() {
        Offset::new(*first_offset)
    } else {
        ctx.cursor
    };

    // Explicit change marks for block paste. sync_change_marks processes
    // inserts bottom-to-top which inverts `[` and `]`. Compute correct T1
    // positions: `[` = first inserted char (top), `]` = last inserted char (bottom).
    if !inserts.is_empty() {
        // `[` is the T0 offset of the first (top) insert — stable because
        // all subsequent inserts are at higher offsets.
        let mark_start = Offset::new(inserts[0].0);

        // `]` needs T1 coordinates for the last (bottom) insert. Each insert
        // before the last one (in top-to-bottom order) shifts the last one's
        // position by the insert text length.
        let cumulative_shift: usize = inserts[..inserts.len() - 1]
            .iter()
            .map(|(_, t)| t.len())
            .sum();
        let (last_offset, last_text) = &inserts[inserts.len() - 1];
        let last_t1_offset = last_offset + cumulative_shift;
        let mark_end_offset = if last_text.is_empty() {
            last_t1_offset
        } else {
            let last_char_len = last_text.chars().next_back().unwrap().len_utf8();
            last_t1_offset + last_text.len() - last_char_len
        };
        effects = effects
            .set_mark(crate::primitives::MarkName::CHANGE_START, mark_start, None)
            .set_mark(
                crate::primitives::MarkName::CHANGE_END,
                Offset::new(mark_end_offset),
                None,
            )
            .set_mark(crate::primitives::MarkName::LAST_CHANGE, mark_start, None);
    }

    let effects = effects.set_cursor(new_cursor).end_undo();
    CommandResult::new(effects, new_cursor)
}

// ============================================================================
// Dispatcher Interface Functions
// ============================================================================

/// Execute put after (p command) from dispatcher.
///
/// This is the interface called by dispatch/action.rs.
#[inline]
pub fn execute_put(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(_content) = ctx.register_content else {
        // Neovim sets change marks to cursor position even with empty register
        let reg_char = ctx.register_name.unwrap_or(RegisterName::UNNAMED);
        let effects = Effects::new()
            .show_error(VimError::NothingInRegister(reg_char.char()))
            .set_mark(crate::primitives::MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(crate::primitives::MarkName::CHANGE_END, ctx.cursor, None);
        return CommandResult::new(effects, ctx.cursor);
    };

    let result = put_after(ctx);
    CommandResult::new(result.effects, result.cursor.unwrap_or(ctx.cursor))
}

/// Execute put before (P command) from dispatcher.
///
/// This is the interface called by dispatch/action.rs.
#[inline]
pub fn execute_put_before(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(_content) = ctx.register_content else {
        // Neovim sets change marks to cursor position even with empty register
        let reg_char = ctx.register_name.unwrap_or(RegisterName::UNNAMED);
        let effects = Effects::new()
            .show_error(VimError::NothingInRegister(reg_char.char()))
            .set_mark(crate::primitives::MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(crate::primitives::MarkName::CHANGE_END, ctx.cursor, None);
        return CommandResult::new(effects, ctx.cursor);
    };

    let result = put_before(ctx);
    CommandResult::new(result.effects, result.cursor.unwrap_or(ctx.cursor))
}

/// Execute ]p (put after with indent adjustment) from dispatcher.
///
/// Pastes linewise register content after the current line, reindenting
/// each pasted line to match the indentation of the current line.
/// For charwise/blockwise content, falls back to regular put.
pub fn execute_put_indent_after(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(content) = ctx.register_content else {
        let reg_char = ctx.register_name.unwrap_or(RegisterName::UNNAMED);
        let effects = Effects::new().show_error(VimError::NothingInRegister(reg_char.char()));
        return CommandResult::new(effects, ctx.cursor);
    };

    if content.motion_type() != MotionType::LineWise {
        // For charwise/blockwise, behave like regular put
        return execute_put(ctx);
    }

    let target_cols = line_indent_columns(ctx.text, ctx.line_start.get(), ctx.tabstop);
    let adjusted = reindent_text(content.text(), target_cols, ctx.expandtab, ctx.tabstop);
    let text = repeat_text(&adjusted, ctx.count);
    put_linewise_after_text(ctx, &text)
}

/// Execute [p (put before with indent adjustment) from dispatcher.
///
/// Pastes linewise register content before the current line, reindenting
/// each pasted line to match the indentation of the current line.
/// For charwise/blockwise content, falls back to regular put before.
pub fn execute_put_indent_before(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(content) = ctx.register_content else {
        let reg_char = ctx.register_name.unwrap_or(RegisterName::UNNAMED);
        let effects = Effects::new().show_error(VimError::NothingInRegister(reg_char.char()));
        return CommandResult::new(effects, ctx.cursor);
    };

    if content.motion_type() != MotionType::LineWise {
        // For charwise/blockwise, behave like regular put before
        return execute_put_before(ctx);
    }

    let target_cols = line_indent_columns(ctx.text, ctx.line_start.get(), ctx.tabstop);
    let adjusted = reindent_text(content.text(), target_cols, ctx.expandtab, ctx.tabstop);
    let text = repeat_text(&adjusted, ctx.count);
    put_linewise_before_text(ctx, &text)
}

/// Perform a linewise put-after using caller-supplied (already-repeated) text.
///
/// Mirrors the `MotionType::LineWise` branch of `put_after`, but takes the
/// text as a parameter instead of reading it from `ctx.register_content`.
/// Used by ]p (indent-adjusted paste).
fn put_linewise_after_text(ctx: &ActionContext<'_>, text: &str) -> CommandResult {
    let (insert_at, owned_text, line_start_in_new_doc) =
        if let Some(next_start) = ctx.next_line_start {
            (next_start, ensure_trailing_newline(text), next_start.get())
        } else {
            let insert_pos = ctx.line_end;
            let mut final_text = String::from("\n");
            final_text.push_str(text.trim_end_matches('\n'));
            let ls = insert_pos.next().get();
            (insert_pos, Cow::Owned(final_text), ls)
        };

    let first_non_blank_offset = {
        let scan = if ctx.next_line_start.is_some() {
            (*owned_text).split('\n').next().unwrap_or("")
        } else {
            owned_text
                .strip_prefix('\n')
                .unwrap_or(&owned_text)
                .split('\n')
                .next()
                .unwrap_or("")
        };
        first_non_blank_in_line(scan)
    };
    let new_cursor = Offset::new(line_start_in_new_doc + first_non_blank_offset);

    // For the EOF case the inserted text is prepended with '\n', so
    // mark.[ and mark.. must point past that separator to the content start.
    let mark_start = Offset::new(line_start_in_new_doc);

    let mark_end_offset = linewise_mark_end(insert_at, &owned_text);

    let effects = Effects::new()
        .begin_undo_force_entry()
        .insert(insert_at, &*owned_text)
        .set_mark(crate::primitives::MarkName::CHANGE_START, mark_start, None)
        .set_mark(
            crate::primitives::MarkName::CHANGE_END,
            mark_end_offset,
            None,
        )
        .set_mark(crate::primitives::MarkName::LAST_CHANGE, mark_start, None)
        .set_cursor(new_cursor)
        .end_undo();
    CommandResult::new(effects, new_cursor)
}

/// Perform a linewise put-before using caller-supplied (already-repeated) text.
///
/// Mirrors the `MotionType::LineWise` branch of `put_before`, but takes the
/// text as a parameter instead of reading it from `ctx.register_content`.
/// Used by [p (indent-adjusted paste).
fn put_linewise_before_text(ctx: &ActionContext<'_>, text: &str) -> CommandResult {
    let insert_at = ctx.line_start;
    let text = ensure_trailing_newline(text);

    let fnb = first_non_blank_in_line((*text).split('\n').next().unwrap_or(""));
    let new_cursor = insert_at.saturating_add_raw(fnb);

    let effects = Effects::new()
        .begin_undo_force_entry()
        .insert(insert_at, &*text)
        .set_cursor(new_cursor)
        .end_undo();
    CommandResult::new(effects, new_cursor)
}

/// Measure the column width of a leading whitespace string, accounting for tabs.
fn indent_column_width(indent: &str, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    let mut col = 0;
    for c in indent.chars() {
        match c {
            '\t' => col = col / tabstop * tabstop + tabstop,
            ' ' => col += 1,
            _ => break,
        }
    }
    col
}

/// Measure the column width of leading whitespace in a line.
fn line_indent_columns(text: &str, line_start_offset: usize, tabstop: usize) -> usize {
    let slice = &text[line_start_offset..];
    let indent: String = slice
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    indent_column_width(&indent, tabstop)
}

/// Construct an indent string for a given column width using expandtab/tabstop settings.
fn make_indent(columns: usize, expandtab: bool, tabstop: usize) -> String {
    let tabstop = tabstop.max(1);
    if expandtab {
        " ".repeat(columns)
    } else {
        let tabs = columns / tabstop;
        let spaces = columns % tabstop;
        let mut s = "\t".repeat(tabs);
        s.push_str(&" ".repeat(spaces));
        s
    }
}

/// Reindent all lines in `text` to match a target indent column width.
///
/// - The first non-empty line's indent is used as the "source indent" baseline.
/// - Each subsequent line has any extra indent (beyond the baseline) preserved.
/// - Blank/whitespace-only lines are left empty (no indent added).
/// - Tabs are properly measured and expanded/contracted based on settings.
fn reindent_text(text: &str, target_columns: usize, expandtab: bool, tabstop: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();

    // Find the source indent column width from the first non-empty line
    let source_indent_cols = lines.iter().find(|l| !l.trim().is_empty()).map_or(0, |l| {
        let indent: String = l.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        indent_column_width(&indent, tabstop)
    });

    let mut result = String::with_capacity(text.len() + lines.len() * target_columns);
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        if line.trim().is_empty() {
            // Leave blank/whitespace-only lines empty
        } else {
            let this_indent: String = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let this_indent_cols = indent_column_width(&this_indent, tabstop);
            let this_indent_bytes: usize = this_indent.len();
            // Extra columns on this line beyond the source baseline
            let extra_cols = this_indent_cols.saturating_sub(source_indent_cols);
            let total_cols = target_columns + extra_cols;
            result.push_str(&make_indent(total_cols, expandtab, tabstop));
            result.push_str(&line[this_indent_bytes..]);
        }
    }
    result
}

/// Execute gp (put after, cursor after pasted text) from dispatcher.
#[inline]
pub fn execute_put_after_cursor_after(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(content) = ctx.register_content else {
        let reg_char = ctx.register_name.unwrap_or(RegisterName::UNNAMED);
        // Neovim sets change marks to cursor even for empty registers.
        let effects = Effects::new()
            .show_error(VimError::NothingInRegister(reg_char.char()))
            .set_mark(crate::primitives::MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(crate::primitives::MarkName::CHANGE_END, ctx.cursor, None);
        return CommandResult::new(effects, ctx.cursor);
    };
    let pasted = repeat_text(content.text(), ctx.count);
    let mut result = put_after(ctx);
    let cursor = gp_cursor_after(ctx, &pasted, content.motion_type(), true);
    // Override the cursor in the effects — append SetCursor after the existing ones.
    result
        .effects
        .push(crate::effects::Effect::set_cursor(cursor));
    CommandResult::new(result.effects, cursor)
}

/// Execute gP (put before, cursor after pasted text) from dispatcher.
#[inline]
pub fn execute_put_before_cursor_after(ctx: &ActionContext<'_>) -> CommandResult {
    let Some(content) = ctx.register_content else {
        let reg_char = ctx.register_name.unwrap_or(RegisterName::UNNAMED);
        // Neovim sets change marks to cursor even for empty registers.
        let effects = Effects::new()
            .show_error(VimError::NothingInRegister(reg_char.char()))
            .set_mark(crate::primitives::MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(crate::primitives::MarkName::CHANGE_END, ctx.cursor, None);
        return CommandResult::new(effects, ctx.cursor);
    };
    let pasted = repeat_text(content.text(), ctx.count);
    let mut result = put_before(ctx);
    let cursor = gp_cursor_after(ctx, &pasted, content.motion_type(), false);
    // Override the cursor in the effects.
    result
        .effects
        .push(crate::effects::Effect::set_cursor(cursor));
    CommandResult::new(result.effects, cursor)
}

/// Compute cursor position for gp/gP: right after the pasted text.
///
/// For charwise: byte position after the last pasted byte, clamped to last char.
/// For linewise: first non-blank of the line after the last pasted line;
///               if at EOF, first non-blank of the last pasted line.
fn gp_cursor_after(
    ctx: &ActionContext<'_>,
    pasted: &str,
    motion_type: MotionType,
    after: bool,
) -> Offset {
    match motion_type {
        MotionType::CharWise => {
            let insert_at = if after {
                let cursor_pos = ctx.cursor.get();
                let on_newline = ctx.text.as_bytes().get(cursor_pos) == Some(&b'\n');
                if cursor_pos < ctx.text.len() && !on_newline {
                    let ch = ctx.text[cursor_pos..].chars().next();
                    (cursor_pos + ch.map_or(1, char::len_utf8)).min(ctx.text.len())
                } else {
                    cursor_pos
                }
            } else {
                ctx.cursor.get()
            };
            let after_pos = insert_at + pasted.len();
            let new_doc_len = ctx.text.len() + pasted.len();
            // Clamp: cursor can't be past last char of new document
            if after_pos >= new_doc_len {
                Offset::new(new_doc_len.saturating_sub(1))
            } else {
                Offset::new(after_pos)
            }
        }
        MotionType::LineWise => {
            if after {
                linewise_gp_after(ctx, pasted)
            } else {
                linewise_gp_before(ctx, pasted)
            }
        }
        MotionType::BlockWise => {
            // Block mode: just use the regular put cursor
            ctx.cursor
        }
    }
}

/// Linewise gp (put after): cursor goes to line after last pasted line.
fn linewise_gp_after(ctx: &ActionContext<'_>, pasted: &str) -> Offset {
    if let Some(next_start) = ctx.next_line_start {
        // Non-EOF: text inserted at next_line_start with trailing \n.
        let inserted_len = if pasted.ends_with('\n') {
            pasted.len()
        } else {
            pasted.len() + 1
        };
        Offset::new(next_start.get() + inserted_len)
    } else {
        // EOF: text inserted as "\n" + pasted.trim_end('\n').
        // Cursor goes to first non-blank of the last pasted line.
        let stripped = pasted.trim_end_matches('\n');
        let last_line = stripped.rsplit('\n').next().unwrap_or(stripped);
        let fnb = first_non_blank_in_line(last_line);
        // Last pasted line starts at: line_end + 1 + (stripped.len() - last_line.len())
        let last_line_offset = ctx.line_end.get() + 1 + (stripped.len() - last_line.len());
        Offset::new(last_line_offset + fnb)
    }
}

/// Linewise gP (put before): cursor goes to line after last pasted line.
fn linewise_gp_before(ctx: &ActionContext<'_>, pasted: &str) -> Offset {
    let insert_at = ctx.line_start.get();
    let inserted_len = if pasted.ends_with('\n') {
        pasted.len()
    } else {
        pasted.len() + 1
    };
    Offset::new(insert_at + inserted_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn make_content(text: &str, motion_type: MotionType) -> RegisterContent {
        RegisterContent::new(text, motion_type)
    }

    fn make_put_ctx<'text>(
        text: &'text str,
        cursor: usize,
        content: &'text RegisterContent,
    ) -> ActionContext<'text> {
        ActionContext::from_text_and_cursor(text, Offset::new(cursor), NonZeroU32::MIN)
            .with_register(content)
    }

    #[test]
    fn test_put_after_charwise() {
        let content = make_content("xyz", MotionType::CharWise);
        let ctx = make_put_ctx("hello", 2, &content);

        let result = put_after(&ctx);

        // Cursor lands on last inserted char (Vim p behavior)
        assert_eq!(result.cursor.unwrap().get(), 5);
        assert!(!result.effects.is_empty());
    }

    #[test]
    fn test_put_before_charwise() {
        let content = make_content("xyz", MotionType::CharWise);
        let ctx = make_put_ctx("hello", 2, &content);

        let result = put_before(&ctx);

        // Cursor lands on last inserted char (Vim P behavior)
        assert_eq!(result.cursor.unwrap().get(), 4);
        assert!(!result.effects.is_empty());
    }

    #[test]
    fn test_put_after_linewise() {
        let content = make_content("new line\n", MotionType::LineWise);
        let ctx = ActionContext::new(
            "hello line\nnext line",
            Offset::new(5),
            Offset::new(0),
            Offset::new(10),
            Some(Offset::new(11)),
            1,
        )
        .with_register(&content);

        let result = put_after(&ctx);

        // Should insert at next line start
        assert_eq!(result.cursor.unwrap().get(), 11);
    }

    #[test]
    fn test_put_before_linewise() {
        let content = make_content("new line\n", MotionType::LineWise);
        let ctx = ActionContext::new(
            "hello line\nnext line",
            Offset::new(5),
            Offset::new(0),
            Offset::new(10),
            Some(Offset::new(11)),
            1,
        )
        .with_register(&content);

        let result = put_before(&ctx);

        // Should insert at line start
        assert_eq!(result.cursor.unwrap().get(), 0);
    }

    #[test]
    fn test_put_with_count() {
        let content = make_content("ab", MotionType::CharWise);
        let mut ctx = make_put_ctx("hello", 0, &content);
        ctx.count = 3;

        let result = put_after(&ctx);

        // Effect should exist
        assert!(!result.effects.is_empty());
    }

    // ── ]p / [p indent-adjusted paste tests ──────────────────────────────

    fn make_ctx_linewise<'text>(
        text: &'text str,
        cursor_pos: usize,
        content: &'text RegisterContent,
    ) -> ActionContext<'text> {
        ActionContext::from_text_and_cursor(text, Offset::new(cursor_pos), NonZeroU32::MIN)
            .with_register(content)
    }

    #[test]
    fn test_reindent_text_no_indent() {
        // Target line has no indent; pasted text's first line also has none.
        // Second line has 4-space relative indent — preserved.
        let result = reindent_text("fn main() {\n    hello();\n}", 0, true, 4);
        assert_eq!(result, "fn main() {\n    hello();\n}");
    }

    #[test]
    fn test_reindent_text_adds_indent() {
        // Target line has 4-space indent; pasted text has none
        let result = reindent_text("fn foo() {}\nfn bar() {}", 4, true, 4);
        assert_eq!(result, "    fn foo() {}\n    fn bar() {}");
    }

    #[test]
    fn test_reindent_text_removes_indent() {
        // Target line has no indent; pasted text has 4-space indent
        let result = reindent_text("    fn foo() {}\n    fn bar() {}", 0, true, 4);
        assert_eq!(result, "fn foo() {}\nfn bar() {}");
    }

    #[test]
    fn test_reindent_text_preserves_relative_indent() {
        // First pasted line has 4 spaces; second has 8 (4 extra).
        // Target is 2 spaces → first becomes 2, second becomes 6.
        let result = reindent_text("    x()\n        y()", 2, true, 4);
        assert_eq!(result, "  x()\n      y()");
    }

    #[test]
    fn test_reindent_text_blank_lines_stay_empty() {
        let result = reindent_text("a\n\nb", 4, true, 4);
        assert_eq!(result, "    a\n\n    b");
    }

    #[test]
    fn test_line_indent_columns_spaces() {
        let text = "hello\n    world";
        // Line 1 starts at byte 6
        assert_eq!(line_indent_columns(text, 6, 4), 4);
    }

    #[test]
    fn test_line_indent_columns_no_indent() {
        assert_eq!(line_indent_columns("hello world", 0, 4), 0);
    }

    #[test]
    fn test_put_indent_after_linewise() {
        // Current line "hello" has no indent.
        // Register has "    fn foo() {}" (4-space indent).
        // After ]p the pasted line should lose its indent.
        let content = make_content("    fn foo() {}\n", MotionType::LineWise);
        let ctx = make_ctx_linewise("hello\nworld", 0, &content);
        let result = execute_put_indent_after(&ctx);
        assert!(!result.effects.is_empty());
    }

    #[test]
    fn test_put_indent_before_linewise() {
        // Cursor on "    world" (4-space indent).
        // Register has "fn foo() {}" (no indent).
        // After [p the pasted line should gain 4-space indent.
        let content = make_content("fn foo() {}\n", MotionType::LineWise);
        let ctx = make_ctx_linewise("hello\n    world", 6, &content);
        let result = execute_put_indent_before(&ctx);
        assert!(!result.effects.is_empty());
    }

    #[test]
    fn test_put_indent_after_charwise_fallback() {
        // Charwise register should fall back to regular put (no indent adjustment)
        let content = make_content("xyz", MotionType::CharWise);
        let ctx = ActionContext::from_text_and_cursor("hello", Offset::new(2), NonZeroU32::MIN)
            .with_register(&content);
        let result = execute_put_indent_after(&ctx);
        assert!(!result.effects.is_empty());
    }

    #[test]
    fn test_put_indent_before_charwise_fallback() {
        // Charwise register should fall back to regular put before
        let content = make_content("xyz", MotionType::CharWise);
        let ctx = ActionContext::from_text_and_cursor("hello", Offset::new(2), NonZeroU32::MIN)
            .with_register(&content);
        let result = execute_put_indent_before(&ctx);
        assert!(!result.effects.is_empty());
    }
}
