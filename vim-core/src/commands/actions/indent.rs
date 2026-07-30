//! Indent/outdent actions (`>>`, `<<`).
//!
//! Shifts lines left or right by shiftwidth.
//! Plain functions, not trait methods.

use super::types::ActionContext;
use crate::commands::helpers::{line_end_for_offset, line_of, line_start, line_start_for_offset};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{Offset, Range};

/// Indent lines by adding spaces at the start (`>>`).
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
    )
}

/// Raw indent — takes primitive parameters, no ActionContext coupling.
///
/// Both `commands::actions` (via `>>`) and `commands::insert` (via Ctrl-T)
/// call this function. Single source of truth for indent logic.
pub fn indent_lines_raw(
    text: &str,
    cursor: usize,
    count: usize,
    shift_width: usize,
) -> CommandResult {
    let indent_str = " ".repeat(shift_width);
    let current_line = line_of(text, cursor);
    let num_lines = count;

    let mut open = Effects::new().begin_undo();

    // Process lines bottom-to-top to preserve byte offsets
    for i in (0..num_lines).rev() {
        let target_line = current_line + i;
        if let Some(ls) = line_start(text, target_line) {
            open = open.insert(Offset::new(ls), indent_str.as_str());
        }
    }

    let effects = open.end_undo();

    // Cursor at first non-blank of current line after indent
    let line_start_pos = line_start_for_offset(text, cursor);
    CommandResult::new(effects, Offset::new(line_start_pos + shift_width))
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
}
