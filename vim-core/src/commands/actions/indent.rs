//! Indent/outdent actions (`>>`, `<<`).
//!
//! Shifts lines left or right by shiftwidth.
//! Plain functions, not trait methods.

use super::types::ActionContext;
use crate::commands::helpers::{
    byte_to_vcol, line_end_for_offset, line_of, line_start, line_start_for_offset,
};
use crate::commands::insert::smartindent::build_indent_string;
use crate::commands::operators::ShiftDirection;
use crate::commands::CommandResult;
use crate::effects::{Effect, Effects};
use crate::primitives::{Offset, Range};

/// One step of Vim's `shift_line()` on `line`: the byte length of its
/// leading blanks and what they become one `shift_width` further left or
/// right, laid out as `set_indent()` does (tabs then spaces unless
/// `expandtab`). `None` when the blanks would stay as they are.
pub(crate) fn shift_indent(
    line: &str,
    direction: ShiftDirection,
    shift_width: usize,
    tabstop: usize,
    expandtab: bool,
) -> Option<(usize, String)> {
    if shift_width == 0 {
        return None;
    }
    let ts = tabstop.max(1);
    let old_len = line
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    let old_width = byte_to_vcol(&line[..old_len], old_len, ts);
    let width = match direction {
        ShiftDirection::Right => old_width + shift_width,
        ShiftDirection::Left => old_width.saturating_sub(shift_width),
    };
    let indent = build_indent_string(width, !expandtab, ts);
    (indent != line[..old_len]).then_some((old_len, indent))
}

/// The edit that turns the blanks `old` at offset `at` into `new`, keeping
/// what both end with and then what both start with. A growing indent is
/// therefore an insert at the line start and a shrinking one a delete from
/// there, as before tabs were laid out again.
pub(crate) fn indent_effect(at: usize, old: &str, new: &str) -> Effect {
    let same_end = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old, new) = (&old[..old.len() - same_end], &new[..new.len() - same_end]);
    let same_start = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let removed = Range::from_raw(at + same_start, at + old.len());
    let inserted = &new[same_start..];
    if removed.is_empty() {
        Effect::insert(removed.start(), inserted)
    } else if inserted.is_empty() {
        Effect::delete(removed)
    } else {
        Effect::replace(removed, inserted)
    }
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

    let shift =
        |line: &str| shift_indent(line, ShiftDirection::Right, shift_width, tabstop, expandtab);

    // Process lines bottom-to-top to preserve byte offsets
    for i in (0..num_lines).rev() {
        let target_line = current_line + i;
        if let Some(ls) = line_start(text, target_line) {
            let line = &text[ls..line_end_for_offset(text, ls)];
            if let Some((old_len, indent)) = shift(line) {
                open.push(indent_effect(ls, &line[..old_len], &indent));
            }
        }
    }

    let effects = open.end_undo();

    // Cursor at first non-blank of the current line after indent.
    let ls_cur = line_start_for_offset(text, cursor);
    let line_cur = &text[ls_cur..line_end_for_offset(text, ls_cur)];
    let indent_len = shift(line_cur).map_or_else(
        || line_cur.len() - line_cur.trim_start_matches([' ', '\t']).len(),
        |(_, indent)| indent.len(),
    );
    CommandResult::new(effects, Offset::new(ls_cur + indent_len))
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

    /// `text` after the edits in `result`.
    fn shifted(text: &str, result: &CommandResult) -> String {
        let effects: Vec<_> = result.effects.iter().cloned().collect();
        crate::commands::insert::wrap::apply_text_effects(text, &effects).unwrap()
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

    // ── shared shift helpers ──────────────────────────────────────────────────

    #[test]
    fn test_shift_indent_unchanged_is_none() {
        assert_eq!(
            shift_indent("hello", ShiftDirection::Left, 4, 4, false),
            None
        );
        assert_eq!(
            shift_indent("\thello", ShiftDirection::Right, 0, 4, false),
            None
        );
    }

    #[test]
    fn test_shift_indent_lays_out_the_whole_indent() {
        // Spaces worth a tab become a tab; expandtab turns tabs into spaces.
        let right = ShiftDirection::Right;
        assert_eq!(
            shift_indent("  \thello", right, 4, 4, false),
            Some((3, "\t\t".to_string()))
        );
        assert_eq!(
            shift_indent("\thello", right, 4, 8, true),
            Some((1, " ".repeat(12)))
        );
    }

    #[test]
    fn test_indent_effect_keeps_insert_and_delete_at_line_start() {
        use crate::effects::Effect;
        assert_eq!(
            indent_effect(10, "\t", "\t\t"),
            Effect::insert(Offset::new(10), "\t")
        );
        assert_eq!(
            indent_effect(10, "\t\t", "\t"),
            Effect::delete(Range::from_raw(10, 11))
        );
        assert_eq!(
            indent_effect(10, "\t", "\t  "),
            Effect::insert(Offset::new(11), "  ")
        );
        assert_eq!(
            indent_effect(10, "    ", "\t\t"),
            Effect::replace(Range::from_raw(10, 14), "\t\t")
        );
    }

    // ── tab-aware indent `>>` (sibling of hmdfrds/godot-vim#50) ───────────────

    #[test]
    fn test_indent_unindented_line_uses_tab_under_noexpandtab() {
        // noexpandtab, ts=4, sw=4: `>>` adds one indent level == one tab,
        // not four spaces.
        let ctx = make_ctx("hello", 0, 1)
            .with_shift_width(4)
            .with_tab_options(4, false);
        assert_eq!(shifted("hello", &execute_indent_lines(&ctx)), "\thello");
    }

    #[test]
    fn test_indent_tab_line_adds_a_tab() {
        // "\thello" (width 4) + sw 4 -> width 8 -> two tabs.
        let ctx = make_ctx("\thello", 0, 1)
            .with_shift_width(4)
            .with_tab_options(4, false);
        assert_eq!(shifted("\thello", &execute_indent_lines(&ctx)), "\t\thello");
    }

    #[test]
    fn test_indent_renders_partial_tab() {
        // sw=2, ts=4, noexpandtab, existing tab (width 4) -> width 6 ->
        // one tab + two spaces.
        let ctx = make_ctx("\thello", 0, 1)
            .with_shift_width(2)
            .with_tab_options(4, false);
        assert_eq!(shifted("\thello", &execute_indent_lines(&ctx)), "\t  hello");
    }

    #[test]
    fn test_indent_expandtab_still_spaces() {
        // Regression guard: under expandtab, `>>` still inserts spaces.
        let ctx = make_ctx("hello", 0, 1)
            .with_shift_width(4)
            .with_tab_options(4, true);
        assert_eq!(shifted("hello", &execute_indent_lines(&ctx)), "    hello");
    }
}
