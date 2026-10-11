//! Insert-mode indentation commands (Ctrl-T, Ctrl-D, ^^D, 0^D).
//!
//! Ctrl-T and Ctrl-D move the indent as Vim's `change_indent()` does, through
//! the same [`shift_indent`] as `>>` and `<<`.

use crate::commands::actions::indent::{indent_effect, shift_indent};
use crate::commands::helpers::{byte_to_vcol, line_end_for_offset, line_start_for_offset};
use crate::commands::operators::ShiftDirection;
use crate::commands::CommandResult;
use crate::effects::{Effect, Effects};
use crate::primitives::{Offset, Range};

/// Indent current line during insert mode (Ctrl-T).
#[inline]
pub fn indent_at_cursor(
    text: &str,
    cursor: Offset,
    shift_width: usize,
    tabstop: usize,
    expandtab: bool,
) -> CommandResult {
    change_indent(
        text,
        cursor,
        ShiftDirection::Right,
        shift_width,
        tabstop,
        expandtab,
    )
}

/// Outdent current line during insert mode (Ctrl-D).
#[inline]
pub fn outdent_at_cursor(
    text: &str,
    cursor: Offset,
    shift_width: usize,
    tabstop: usize,
    expandtab: bool,
) -> CommandResult {
    change_indent(
        text,
        cursor,
        ShiftDirection::Left,
        shift_width,
        tabstop,
        expandtab,
    )
}

/// Move the cursor line's indent to the next multiple of `shift_width`, as
/// Vim's `change_indent()` does for Ctrl-T and Ctrl-D.
///
/// A cursor at or after the first non-blank stays on its character. One
/// inside the indent keeps its distance in columns from the first non-blank;
/// when that column falls inside a tab, Vim pads with spaces to reach it.
fn change_indent(
    text: &str,
    cursor: Offset,
    direction: ShiftDirection,
    shift_width: usize,
    tabstop: usize,
    expandtab: bool,
) -> CommandResult {
    let cur = cursor.get();
    let ls = line_start_for_offset(text, cur);
    let line = &text[ls..line_end_for_offset(text, ls)];
    let Some((old_len, indent)) =
        shift_indent(line, direction, true, shift_width, tabstop, expandtab)
    else {
        return CommandResult::empty(cursor);
    };

    let mut open = Effects::new().begin_undo();
    open.push(indent_effect(ls, &line[..old_len], &indent));
    let new_cursor = if cur >= ls + old_len {
        cur - old_len + indent.len()
    } else {
        let ts = tabstop.max(1);
        let old_width = byte_to_vcol(&line[..old_len], old_len, ts);
        let left_of_text = old_width - byte_to_vcol(line, cur - ls, ts);
        let target = byte_to_vcol(&indent, indent.len(), ts).saturating_sub(left_of_text);
        let (mut vcol, mut bytes) = (0, 0);
        for b in indent.bytes() {
            let next = if b == b'\t' {
                (vcol / ts + 1) * ts
            } else {
                vcol + 1
            };
            if next > target {
                break;
            }
            vcol = next;
            bytes += 1;
        }
        if vcol < target {
            open.push(Effect::insert(
                Offset::new(ls + bytes),
                " ".repeat(target - vcol),
            ));
            bytes += target - vcol;
        }
        ls + bytes
    };
    let mut effects = open.end_undo();
    effects.push(Effect::set_cursor(Offset::new(new_cursor)));
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
        let result = indent_at_cursor("hello", Offset::new(0), 4, 4, true);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_indent_at_cursor_tab_under_noexpandtab() {
        // Ctrl-T on a tab-indented line adds a tab and moves the cursor right
        // by one byte (the inserted tab), not by shift_width bytes.
        let result = indent_at_cursor("\thello", Offset::new(1), 4, 4, false);
        let last = result.effects.iter().last().unwrap();
        assert!(
            matches!(last, Effect::SetCursor { offset } if offset.get() == 2),
            "cursor should advance by one tab byte, got {last:?}"
        );
    }

    #[test]
    fn test_indent_at_cursor_keeps_cursor_when_spaces_become_tabs() {
        // "    hello" under noexpandtab, ts=4, sw=4 becomes "\t\thello"; the
        // cursor on 'h' moves left with it.
        let result = indent_at_cursor("    hello", Offset::new(4), 4, 4, false);
        let last = result.effects.iter().last().unwrap();
        assert!(
            matches!(last, Effect::SetCursor { offset } if offset.get() == 2),
            "cursor should stay on 'h', got {last:?}"
        );
    }

    #[test]
    fn test_outdent_at_cursor_no_indent() {
        let result = outdent_at_cursor("hello", Offset::new(0), 4, 4, true);
        // No leading whitespace to remove — result has cursor but no edit effects
        assert!(result.effects.is_empty());
    }

    #[test]
    fn test_outdent_at_cursor_with_indent() {
        let result = outdent_at_cursor("    hello", Offset::new(4), 4, 4, true);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_outdent_at_cursor_tab_indent() {
        // Ctrl-D on a tab-indented line removes one tab and moves the cursor
        // left by exactly one byte (the removed tab), not to line start.
        let result = outdent_at_cursor("\t\thello", Offset::new(2), 4, 4, false);
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
