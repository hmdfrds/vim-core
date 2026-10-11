//! Indent/outdent actions (`>>`, `<<`).
//!
//! Shifts lines left or right by shiftwidth.
//! Plain functions, not trait methods.

use super::types::ActionContext;
use crate::commands::helpers::{
    byte_to_vcol, line_end_for_offset, line_of, line_start, line_start_for_offset,
};
use crate::commands::insert::smartindent::build_indent_string;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{Offset, Range};

/// The leading blanks of `line` after adding `shift_width` columns, laid out
/// as tabs then spaces unless `expandtab`, so a tab indent stays tabs.
/// Returns the byte length of the old blanks and the new blanks.
pub(crate) fn reindent(
    line: &str,
    shift_width: usize,
    tabstop: usize,
    expandtab: bool,
) -> (usize, String) {
    let ts = tabstop.max(1);
    let ws_bytes = line
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    let old_width = byte_to_vcol(&line[..ws_bytes], ws_bytes, ts);
    (
        ws_bytes,
        build_indent_string(old_width + shift_width, !expandtab, ts),
    )
}

/// Indent lines (`>>`).
///
/// # Arguments
/// * `ctx` - Indent context with text, cursor, count, shift width
///
/// # Returns
/// * `CommandResult` with effects to apply
pub fn execute_indent_lines(ctx: &ActionContext<'_>) -> CommandResult {
    indent_lines_raw(
        ctx.text,
        ctx.cursor.get(),
        ctx.count_usize(),
        ctx.shift_width,
        ctx.tabstop,
        ctx.expandtab,
    )
}

/// Raw indent — takes primitive parameters, no ActionContext coupling.
///
/// Insert-mode Ctrl-T calls this. Normal-mode `>>` runs through
/// `commands::operators::indent` instead.
///
/// Adds one `shift_width` of indentation to each line and lays the leading
/// blanks out again per `expandtab`/`tabstop`, so a tab indent gains a tab.
pub fn indent_lines_raw(
    text: &str,
    cursor: usize,
    count: usize,
    shift_width: usize,
    tabstop: usize,
    expandtab: bool,
) -> CommandResult {
    let current_line = line_of(text, cursor);
    let num_lines = count;

    let mut open = Effects::new().begin_undo();

    // Process lines bottom-to-top to preserve byte offsets
    for i in (0..num_lines).rev() {
        let target_line = current_line + i;
        if let Some(ls) = line_start(text, target_line) {
            let lend = line_end_for_offset(text, ls);
            let (ws_bytes, new_indent) = reindent(&text[ls..lend], shift_width, tabstop, expandtab);
            if ws_bytes == 0 {
                open = open.insert(Offset::new(ls), new_indent.as_str());
            } else {
                open = open.replace(Range::from_raw(ls, ls + ws_bytes), new_indent.as_str());
            }
        }
    }

    let effects = open.end_undo();

    // Cursor at first non-blank of the current line after indent.
    let ls_cur = line_start_for_offset(text, cursor);
    let lend_cur = line_end_for_offset(text, ls_cur);
    let (_, new_indent_cur) = reindent(&text[ls_cur..lend_cur], shift_width, tabstop, expandtab);
    CommandResult::new(effects, Offset::new(ls_cur + new_indent_cur.len()))
}

/// Outdent lines by removing spaces at the start (`<<`).
///
/// # Arguments
/// * `ctx` - Indent context with text, cursor, count, shift width
///
/// # Returns
/// * `CommandResult` with effects to apply
pub fn execute_outdent_lines(ctx: &ActionContext<'_>) -> CommandResult {
    outdent_lines_raw(
        ctx.text,
        ctx.cursor.get(),
        ctx.count_usize(),
        ctx.shift_width,
        ctx.tabstop,
    )
}

/// Raw outdent — takes primitive parameters, no ActionContext coupling.
///
/// Both `commands::actions` (via `<<`) and `commands::insert` (via Ctrl-D)
/// call this function. Single source of truth for outdent logic.
///
/// Removes up to `shift_width` **display columns** of leading whitespace,
/// counting a tab as the number of columns to the next `tabstop`. A tab is
/// only removed when it fits wholly within the remaining budget, matching
/// Neovim (it never splits a tab into spaces here). This handles tab-indented
/// lines, where the old space-only scan removed nothing.
pub fn outdent_lines_raw(
    text: &str,
    cursor: usize,
    count: usize,
    shift_width: usize,
    tabstop: usize,
) -> CommandResult {
    let current_line = line_of(text, cursor);
    let num_lines = count;
    let tabstop = tabstop.max(1);

    let mut open = Effects::new().begin_undo();
    let mut any_removed = false;

    // Process lines bottom-to-top to preserve byte offsets
    for i in (0..num_lines).rev() {
        let target_line = current_line + i;
        if let Some(ls) = line_start(text, target_line) {
            let lend = line_end_for_offset(text, ls);
            let line = &text[ls..lend];
            let mut vcols_removed = 0usize;
            let mut bytes_to_remove = 0usize;
            for ch in line.chars() {
                if vcols_removed >= shift_width {
                    break;
                }
                match ch {
                    ' ' => {
                        vcols_removed += 1;
                        bytes_to_remove += 1;
                    }
                    '\t' => {
                        let tab_cols = tabstop - (vcols_removed % tabstop);
                        if vcols_removed + tab_cols <= shift_width {
                            vcols_removed += tab_cols;
                            bytes_to_remove += 1;
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            if bytes_to_remove > 0 {
                open = open.delete(Range::from_raw(ls, ls + bytes_to_remove));
                any_removed = true;
            }
        }
    }

    if !any_removed {
        return CommandResult::empty(Offset::new(cursor));
    }

    let effects = open.end_undo();
    let lstart = line_start_for_offset(text, cursor);
    CommandResult::new(effects, Offset::new(lstart))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn make_ctx(text: &str, cursor: usize, count: u32) -> ActionContext<'_> {
        ActionContext::from_text_and_cursor(
            text,
            Offset::new(cursor),
            NonZeroU32::new(count).unwrap_or(NonZeroU32::MIN),
        )
    }

    /// Byte range of the first Delete effect.
    fn deleted_range(result: &CommandResult) -> (usize, usize) {
        for e in result.effects.iter() {
            if let crate::effects::Effect::Delete { range } = e {
                return (range.start().get(), range.end().get());
            }
        }
        panic!("No Delete effect found in result");
    }

    /// Text of the first Insert or Replace effect.
    fn emitted_text(result: &CommandResult) -> String {
        for e in result.effects.iter() {
            match e {
                crate::effects::Effect::Insert { text, .. }
                | crate::effects::Effect::Replace { text, .. } => return text.to_string(),
                _ => {}
            }
        }
        panic!("No Insert/Replace effect found in result");
    }

    #[test]
    fn test_indent_line() {
        let ctx = make_ctx("hello", 0, 1);
        let result = execute_indent_lines(&ctx);
        // begin_undo + insert + end_undo = 3 effects
        assert_eq!(result.effects.len(), 3);
        // "hello" -> "    hello"
    }

    #[test]
    fn test_outdent_line() {
        let ctx = make_ctx("    hello", 0, 1);
        let result = execute_outdent_lines(&ctx);
        // begin_undo + delete + end_undo = 3 effects
        assert_eq!(result.effects.len(), 3);
        // "    hello" -> "hello"
    }

    #[test]
    fn test_outdent_partial() {
        let ctx = make_ctx("  hello", 0, 1);
        let result = execute_outdent_lines(&ctx);
        // begin_undo + delete + end_undo = 3 effects
        assert_eq!(result.effects.len(), 3);
        // "  hello" -> "hello" (removes 2 spaces, not 4)
    }

    #[test]
    fn test_outdent_no_spaces() {
        let ctx = make_ctx("hello", 0, 1);
        let result = execute_outdent_lines(&ctx);
        assert!(result.effects.is_empty());
    }

    // ── tab-aware outdent (sibling of hmdfrds/godot-vim#50) ───────────────────

    #[test]
    fn test_outdent_single_tab() {
        // "\thello" with tabstop=4, shiftwidth=4: `<<` removes one tab.
        // The space-only implementation removed nothing.
        let ctx = make_ctx("\thello", 0, 1);
        let result = execute_outdent_lines(&ctx);
        assert_eq!(deleted_range(&result), (0, 1));
    }

    #[test]
    fn test_outdent_two_tabs_removes_one_level() {
        // "\t\thello", tabstop=4, shiftwidth=4: remove one tab (one level).
        let ctx = make_ctx("\t\thello", 0, 1);
        let result = execute_outdent_lines(&ctx);
        assert_eq!(deleted_range(&result), (0, 1));
    }

    #[test]
    fn test_outdent_tab_wider_than_shiftwidth() {
        // tabstop=8, shiftwidth=4: a single tab spans 8 display columns,
        // which is more than one shiftwidth, so it is NOT removed (removing
        // it would over-outdent). Neovim leaves the tab in place.
        let mut ctx = make_ctx("\thello", 0, 1);
        ctx = ctx.with_shift_width(4).with_tab_options(8, false);
        let result = execute_outdent_lines(&ctx);
        assert!(result.effects.is_empty());
    }

    #[test]
    fn test_outdent_tabs_still_removes_spaces() {
        // Regression guard: pure-space indent is unchanged.
        let ctx = make_ctx("        hello", 0, 1);
        let result = execute_outdent_lines(&ctx);
        assert_eq!(deleted_range(&result), (0, 4));
    }

    // ── tab-aware indent `>>` (sibling of hmdfrds/godot-vim#50) ───────────────

    #[test]
    fn test_indent_unindented_line_uses_tab_under_noexpandtab() {
        // noexpandtab, ts=4, sw=4: `>>` adds one indent level == one tab,
        // not four spaces.
        let ctx = make_ctx("hello", 0, 1)
            .with_shift_width(4)
            .with_tab_options(4, false);
        assert_eq!(emitted_text(&execute_indent_lines(&ctx)), "\t");
    }

    #[test]
    fn test_indent_tab_line_adds_a_tab() {
        // "\thello" (width 4) + sw 4 -> width 8 -> two tabs.
        let ctx = make_ctx("\thello", 0, 1)
            .with_shift_width(4)
            .with_tab_options(4, false);
        assert_eq!(emitted_text(&execute_indent_lines(&ctx)), "\t\t");
    }

    #[test]
    fn test_indent_renders_partial_tab() {
        // sw=2, ts=4, noexpandtab, existing tab (width 4) -> width 6 ->
        // one tab + two spaces.
        let ctx = make_ctx("\thello", 0, 1)
            .with_shift_width(2)
            .with_tab_options(4, false);
        assert_eq!(emitted_text(&execute_indent_lines(&ctx)), "\t  ");
    }

    #[test]
    fn test_indent_expandtab_still_spaces() {
        // Regression guard: under expandtab, `>>` still inserts spaces.
        let ctx = make_ctx("hello", 0, 1)
            .with_shift_width(4)
            .with_tab_options(4, true);
        assert_eq!(emitted_text(&execute_indent_lines(&ctx)), "    ");
    }
}
