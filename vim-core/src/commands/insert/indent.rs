//! Insert-mode indentation commands (Ctrl-T, Ctrl-D, ^^D, 0^D).
//!
//! Calls raw indent/outdent functions with primitive params.
//! Zero type coupling — no `ActionContext` import needed.

use crate::commands::actions::indent::{indent_lines_raw, outdent_lines_raw};
use crate::commands::helpers::line_start_for_offset;
use crate::commands::CommandResult;
use crate::effects::{Effect, Effects};
use crate::primitives::{Offset, Range};

/// Indent current line during insert mode (Ctrl-T).
///
/// Calls the shared raw indent function, then adjusts cursor position
/// by the shift width so typing continues at the new indent.
#[inline]
pub fn indent_at_cursor(text: &str, cursor: Offset, shift_width: usize) -> CommandResult {
    let result = indent_lines_raw(text, cursor.get(), 1, shift_width);
    let new_cursor = cursor.saturating_add_raw(shift_width);
    let mut effects = result.effects;
    effects.push(Effect::set_cursor(new_cursor));
    CommandResult::effects_only(effects)
}

/// Outdent current line during insert mode (Ctrl-D).
///
/// Calls the shared raw outdent function, then adjusts cursor position
/// by the removed whitespace amount.
#[inline]
pub fn outdent_at_cursor(
    text: &str,
    cursor: Offset,
    shift_width: usize,
    tabstop: usize,
) -> CommandResult {
    let result = outdent_lines_raw(text, cursor.get(), 1, shift_width, tabstop);
    if result.effects.is_empty() {
        // No whitespace to remove — pass through empty result
        return result;
    }
    // Move the cursor left by the bytes actually removed. Deriving this from
    // the delete range (rather than assuming the whole indent was removed)
    // keeps the cursor correct for partial outdents and tab indents.
    let removed = result
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::Delete { range } => Some(range.end().get() - range.start().get()),
            _ => None,
        })
        .unwrap_or(0);
    let new_cursor = cursor.saturating_sub_raw(removed);
    let mut effects = result.effects;
    effects.push(Effect::set_cursor(new_cursor));
    CommandResult::effects_only(effects)
}

/// Remove ALL leading whitespace on current line (^^D / 0^D common logic).
///
/// Also deletes the preceding trigger character (`^` or `0`) from the
/// document text. The trigger was typed as a normal insert character and
/// needs to be erased as part of the command.
///
/// Returns effects that:
/// 1. Delete the trigger character preceding the cursor
/// 2. Delete all leading whitespace on the current line
/// 3. Set cursor to line start
#[inline]
pub fn outdent_all_at_cursor(text: &str, cursor: Offset) -> CommandResult {
    let cur = cursor.get();
    let line_start = line_start_for_offset(text, cur);

    // Count all leading whitespace on this line.
    let leading_ws = text[line_start..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(char::len_utf8)
        .sum::<usize>();

    // The trigger char (^ or 0) sits at cursor-1 in the document.
    // It was inserted as a normal character, so we delete it.
    let trigger_start = cur.saturating_sub(1);
    let trigger_len = cur.saturating_sub(trigger_start);

    if leading_ws == 0 && trigger_len == 0 {
        return CommandResult::empty(cursor);
    }

    let mut open = Effects::new().begin_undo();

    // Delete trigger character first (it's after the whitespace region).
    if trigger_len > 0 {
        open = open.delete(Range::from_raw(trigger_start, cur));
    }

    // Delete leading whitespace. After deleting the trigger, offsets shift
    // only if the trigger was within the whitespace region (unlikely since
    // ^/0 is typed at cursor which is after the indent). The trigger is
    // at `cursor - 1` which is >= line_start + leading_ws (cursor is after
    // the indent), so whitespace range is unaffected by trigger deletion.
    if leading_ws > 0 {
        open = open.delete(Range::from_raw(line_start, line_start + leading_ws));
    }

    let mut effects = open.end_undo();

    // Cursor lands at line start (all indent + trigger removed).
    let new_cursor = Offset::new(line_start);
    effects.push(Effect::set_cursor(new_cursor));
    CommandResult::effects_only(effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indent_at_cursor() {
        let result = indent_at_cursor("hello", Offset::new(0), 4);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_outdent_at_cursor_no_indent() {
        let result = outdent_at_cursor("hello", Offset::new(0), 4, 4);
        // No leading whitespace to remove — result has cursor but no edit effects
        assert!(result.effects.is_empty());
    }

    #[test]
    fn test_outdent_at_cursor_with_indent() {
        let result = outdent_at_cursor("    hello", Offset::new(4), 4, 4);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_outdent_at_cursor_tab_indent() {
        // Ctrl-D on a tab-indented line removes one tab and moves the cursor
        // left by exactly one byte (the removed tab), not to line start.
        let result = outdent_at_cursor("\t\thello", Offset::new(2), 4, 4);
        assert!(!result.is_empty());
        let last = result.effects.iter().last().unwrap();
        assert!(
            matches!(last, Effect::SetCursor { offset } if offset.get() == 1),
            "cursor should move left by one removed tab, got {last:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // outdent_all_at_cursor (^^D / 0^D)
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn outdent_all_removes_all_whitespace_and_trigger() {
        // "        ^" — 8 spaces of indent, cursor after the trigger char '^'
        // Cursor is at position 9 (after the '^').
        let text = "        ^hello";
        let cursor = Offset::new(9); // after the '^'
        let result = outdent_all_at_cursor(text, cursor);
        assert!(!result.is_empty());
        // Should have: begin_undo, delete trigger, delete whitespace, end_undo, set_cursor
        assert!(result.effects.len() >= 4);
        // Verify cursor lands at line start (offset 0)
        let last_effect = result.effects.iter().last().unwrap();
        assert!(
            matches!(last_effect, Effect::SetCursor { offset } if offset.get() == 0),
            "cursor should land at line start, got {last_effect:?}"
        );
    }

    #[test]
    fn outdent_all_no_indent_still_deletes_trigger() {
        // "^hello" — no indent, just the trigger at position 0
        // Cursor is at position 1 (after the '^').
        let text = "^hello";
        let cursor = Offset::new(1);
        let result = outdent_all_at_cursor(text, cursor);
        assert!(!result.is_empty());
        // Should have: begin_undo, delete trigger, end_undo, set_cursor
        assert!(result.effects.len() >= 3);
    }

    #[test]
    fn outdent_all_with_tabs() {
        // "\t\t^hello" — 2 tabs of indent, trigger '^' at position 2
        let text = "\t\t^hello";
        let cursor = Offset::new(3); // after the '^'
        let result = outdent_all_at_cursor(text, cursor);
        assert!(!result.is_empty());
        // Should delete trigger + both tabs
        let last_effect = result.effects.iter().last().unwrap();
        assert!(
            matches!(last_effect, Effect::SetCursor { offset } if offset.get() == 0),
            "cursor should land at line start"
        );
    }

    #[test]
    fn outdent_all_second_line() {
        // First line + second line with indent and trigger
        let text = "first\n    0hello";
        // Cursor on second line, after "    0" = offset 6+5 = 11
        let cursor = Offset::new(11);
        let result = outdent_all_at_cursor(text, cursor);
        assert!(!result.is_empty());
        // Cursor should land at start of second line (offset 6)
        let last_effect = result.effects.iter().last().unwrap();
        assert!(
            matches!(last_effect, Effect::SetCursor { offset } if offset.get() == 6),
            "cursor should land at second line start, got {last_effect:?}"
        );
    }

    #[test]
    fn outdent_all_cursor_at_zero_is_noop() {
        // Edge case: cursor at position 0, no trigger to delete
        let text = "hello";
        let cursor = Offset::new(0);
        let result = outdent_all_at_cursor(text, cursor);
        assert!(result.effects.is_empty());
    }
}
