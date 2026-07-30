//! Insert mode delete commands.
//!
//! Handles deletion operations in insert mode (Backspace, Ctrl-W, Ctrl-U, Delete).
//!
//! # Commands
//!
//! | Key | Function | Description |
//! |-----|----------|-------------|
//! | `BS` | `backspace` | Delete one char backward |
//! | `C-W` | `delete_word` | Delete word backward |
//! | `C-U` | `delete_to_start` | Delete to start of line |
//! | `Del` | `delete_under` | Delete char under cursor |

use super::types::InsertContext;
use crate::commands::helpers::{
    byte_to_vcol, line_of, line_start, line_start_for_offset, next_char_boundary,
    prev_char_boundary, vcol_to_byte, CharClass,
};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{Offset, Range, WordEraseStyle, WordKind};

/// Delete backward in insert mode (Backspace).
///
/// Implements Neovim's smarttab behavior (on by default): when the cursor
/// is in leading whitespace, BS deletes back to the previous shiftwidth-
/// aligned column instead of deleting a single character.  At BOL, BS
/// joins with the previous line (backspace=eol, the default).
///
/// Alignment is measured in **display columns** (`tabstop`-expanded), not
/// raw bytes, so a tab-indented line deletes one indent level rather than
/// the whole indent — a single tab byte can span several columns.
#[inline]
pub fn backspace(ctx: &InsertContext<'_>) -> CommandResult {
    let cursor = ctx.cursor_usize();
    if cursor == 0 {
        return CommandResult::none();
    }

    let ls = line_start_for_offset(ctx.text, cursor);

    if cursor > ls {
        let leading = &ctx.text[ls..cursor];
        let in_leading_ws = leading.bytes().all(|b| b == b' ' || b == b'\t');

        if in_leading_ws {
            let tabstop = ctx.tabstop.max(1);
            let sw = ctx.shift_width.max(1);

            // Cursor's display column, then the previous shiftwidth-aligned
            // column. `col >= 1` here since `cursor > ls`.
            let col = byte_to_vcol(leading, leading.len(), tabstop);
            let target_col = if col.is_multiple_of(sw) {
                col.saturating_sub(sw)
            } else {
                (col / sw) * sw
            };

            // Map the target column back to a byte. `vcol_to_byte` rounds
            // *up*, so when the target lands inside a wide tab it returns the
            // byte after that tab; step back one grapheme to delete the whole
            // straddling tab (Neovim parity for the delete step).
            let mut target_byte = vcol_to_byte(leading, target_col, tabstop);
            if target_byte >= leading.len() && target_byte > 0 {
                target_byte = prev_char_boundary(leading, target_byte);
            }
            let delete_start = ls + target_byte;

            if delete_start < cursor {
                return CommandResult::effects_only(
                    Effects::new()
                        .delete(Range::new(Offset::new(delete_start), Offset::new(cursor)))
                        .set_cursor(Offset::new(delete_start)),
                );
            }
        }
    }

    let prev_pos = prev_char_boundary(ctx.text, cursor);
    CommandResult::effects_only(
        Effects::new()
            .delete(Range::new(Offset::new(prev_pos), Offset::new(cursor)))
            .set_cursor(Offset::new(prev_pos)),
    )
}

/// Delete word backward (Ctrl-W).
///
/// Dispatches to one of three algorithms based on `ctx.word_erase_style`:
///
/// - **Vi** (default): skip whitespace, then delete one contiguous char-class group.
/// - **AltWerase**: same as Vi but additionally treats `/` as a word boundary.
/// - **TtyWerase**: skip whitespace, then delete backward to the previous whitespace
///   (ignores keyword/punctuation classification entirely).
///
/// All three styles respect Vim's line-boundary rule: reaching a `\n` during
/// the whitespace-skip phase deletes only that newline (joins lines) and stops.
#[inline]
pub fn delete_word(ctx: &InsertContext<'_>) -> CommandResult {
    let cursor = ctx.cursor_usize();
    if cursor == 0 {
        return CommandResult::none();
    }

    let text = ctx.text;
    let boundary = ctx.entry_offset.map_or(0, Offset::get);

    match ctx.word_erase_style {
        WordEraseStyle::Vi => delete_word_vi(text, cursor, boundary, ctx),
        WordEraseStyle::AltWerase => delete_word_alt(text, cursor, boundary, ctx),
        WordEraseStyle::TtyWerase => delete_word_tty(text, cursor, boundary),
    }
}

/// Shared first phase: skip whitespace backward.
///
/// Returns `Ok(pos)` if whitespace was skipped (pos is the first non-whitespace
/// position found, i.e. next char before `pos` is non-whitespace).
/// Returns `Err(result)` if the caller should return that result immediately
/// (hit a newline, or reached the start).
fn skip_whitespace_backward(
    text: &str,
    mut pos: usize,
    cursor: usize,
) -> Result<usize, CommandResult> {
    while pos > 0 {
        let prev_pos = prev_char_boundary(text, pos);
        let c = text[prev_pos..].chars().next().unwrap_or(' ');
        if c == '\n' {
            // At a newline boundary: delete only the newline (join lines).
            return Err(CommandResult::effects_only(
                Effects::new()
                    .delete(Range::new(Offset::new(prev_pos), Offset::new(cursor)))
                    .set_cursor(Offset::new(prev_pos)),
            ));
        }
        if !c.is_whitespace() {
            break;
        }
        pos = prev_pos;
    }
    Ok(pos)
}

/// Build the final deletion result, clamping to boundary.
fn make_delete_result(pos: usize, cursor: usize, boundary: usize) -> CommandResult {
    let pos = pos.max(boundary);
    if pos >= cursor {
        return CommandResult::none();
    }
    CommandResult::effects_only(
        Effects::new()
            .delete(Range::new(Offset::new(pos), Offset::new(cursor)))
            .set_cursor(Offset::new(pos)),
    )
}

/// Vi-style: skip whitespace, delete one char-class group.
fn delete_word_vi(
    text: &str,
    cursor: usize,
    boundary: usize,
    ctx: &InsertContext<'_>,
) -> CommandResult {
    let pos = match skip_whitespace_backward(text, cursor, cursor) {
        Err(r) => return r,
        Ok(p) => p,
    };

    if pos == 0 {
        return make_delete_result(0, cursor, boundary);
    }

    // Classify the char just before pos
    let prev_pos = prev_char_boundary(text, pos);
    let c = text[prev_pos..].chars().next().unwrap_or(' ');
    let target_class = CharClass::classify(c, WordKind::Word, ctx.word_chars);

    // Walk back while same class
    let mut pos = prev_pos;
    while pos > 0 {
        let pp = prev_char_boundary(text, pos);
        let c = text[pp..].chars().next().unwrap_or(' ');
        if CharClass::classify(c, WordKind::Word, ctx.word_chars) != target_class {
            break;
        }
        pos = pp;
    }

    make_delete_result(pos, cursor, boundary)
}

/// AltWerase: same as Vi but `/` is always a word boundary.
fn delete_word_alt(
    text: &str,
    cursor: usize,
    boundary: usize,
    ctx: &InsertContext<'_>,
) -> CommandResult {
    let pos = match skip_whitespace_backward(text, cursor, cursor) {
        Err(r) => return r,
        Ok(p) => p,
    };

    if pos == 0 {
        return make_delete_result(0, cursor, boundary);
    }

    // Classify the char just before pos
    let prev_pos = prev_char_boundary(text, pos);
    let c = text[prev_pos..].chars().next().unwrap_or(' ');

    // Slash is its own boundary: delete only the slash itself.
    if c == '/' {
        return make_delete_result(prev_pos, cursor, boundary);
    }

    let target_class = CharClass::classify(c, WordKind::Word, ctx.word_chars);

    // Walk back while same class AND not hitting a slash
    let mut pos = prev_pos;
    while pos > 0 {
        let pp = prev_char_boundary(text, pos);
        let c = text[pp..].chars().next().unwrap_or(' ');
        if c == '/' || CharClass::classify(c, WordKind::Word, ctx.word_chars) != target_class {
            break;
        }
        pos = pp;
    }

    make_delete_result(pos, cursor, boundary)
}

/// TtyWerase: skip whitespace, then delete backward to the next whitespace.
fn delete_word_tty(text: &str, cursor: usize, boundary: usize) -> CommandResult {
    let pos = match skip_whitespace_backward(text, cursor, cursor) {
        Err(r) => return r,
        Ok(p) => p,
    };

    if pos == 0 {
        return make_delete_result(0, cursor, boundary);
    }

    // Walk back until we hit whitespace (or start)
    let mut pos = pos;
    while pos > 0 {
        let pp = prev_char_boundary(text, pos);
        let c = text[pp..].chars().next().unwrap_or(' ');
        if c.is_whitespace() {
            break;
        }
        pos = pp;
    }

    make_delete_result(pos, cursor, boundary)
}

/// Delete to start of line (Ctrl-U).
///
/// Respects insert start boundary: won't delete past where insert mode was entered.
#[inline]
pub fn delete_to_start(ctx: &InsertContext<'_>) -> CommandResult {
    let cursor = ctx.cursor_usize();
    let text = ctx.text;
    let line = line_of(text, cursor);
    let line_start_offset = line_start(text, line).unwrap_or(0);

    // Don't delete past insert entry point (Vim backspace=start behavior)
    let boundary = match ctx.entry_offset {
        Some(entry) => line_start_offset.max(entry.get()),
        None => line_start_offset,
    };

    if cursor > boundary {
        CommandResult::effects_only(
            Effects::new()
                .delete(Range::new(Offset::new(boundary), Offset::new(cursor)))
                .set_cursor(Offset::new(boundary)),
        )
    } else if cursor > 0 && cursor == line_start_offset {
        // At beginning of line: join with previous line (delete the newline)
        let prev = crate::primitives::text_util::prev_char_boundary(text, cursor);
        CommandResult::effects_only(
            Effects::new()
                .delete(Range::new(Offset::new(prev), Offset::new(cursor)))
                .set_cursor(Offset::new(prev)),
        )
    } else {
        CommandResult::none()
    }
}

/// Delete character under cursor (Delete key).
///
/// Deletes the character at the current cursor position.
/// If at end of document, no effect.
#[inline]
pub fn delete_under(ctx: &InsertContext<'_>) -> CommandResult {
    let cursor = ctx.cursor_usize();
    let text = ctx.text;

    if cursor >= text.len() {
        // At end of document - nothing to delete
        return CommandResult::none();
    }

    let next_pos = next_char_boundary(text, cursor);

    CommandResult::effects_only(
        Effects::new().delete(Range::new(Offset::new(cursor), Offset::new(next_pos))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Extract the deleted byte range from the first Delete effect.
    fn deleted_range(result: &CommandResult) -> (usize, usize) {
        for e in result.effects.iter() {
            if let Effect::Delete { range } = e {
                return (range.start().get(), range.end().get());
            }
        }
        panic!("No Delete effect found in result");
    }

    // ── backspace ─────────────────────────────────────────────────────────────

    #[test]
    fn test_backspace_at_start() {
        let ctx = InsertContext::new("hello", Offset::new(0));
        let result = backspace(&ctx);
        assert!(result.is_empty());
    }

    #[test]
    fn test_backspace_mid_text() {
        let ctx = InsertContext::new("hello", Offset::new(2));
        let result = backspace(&ctx);
        assert!(!result.is_empty());
        assert_eq!(result.effects.len(), 2);
    }

    // ── smarttab backspace in leading whitespace (issue hmdfrds/godot-vim#50) ──

    #[test]
    fn test_backspace_two_tabs_deletes_one_level() {
        // Two leading tabs, cursor after both, sw=4 ts=4 (defaults).
        // Must delete ONE tab (one indent level), not the whole indent.
        // The byte-based math deleted both because 2 % 4 != 0.
        let ctx = InsertContext::new("\t\t", Offset::new(2));
        let result = backspace(&ctx);
        assert_eq!(deleted_range(&result), (1, 2));
    }

    #[test]
    fn test_backspace_tab_then_spaces_strips_to_tab_boundary() {
        // "\t  " (tab + 2 spaces), cursor after all three, sw=4 ts=4.
        // Display column at cursor is 6; the previous shiftwidth boundary
        // (4) sits exactly on the tab, so only the 2 sub-shiftwidth spaces
        // are removed.
        let ctx = InsertContext::new("\t  ", Offset::new(3));
        let result = backspace(&ctx);
        assert_eq!(deleted_range(&result), (1, 3));
    }

    #[test]
    fn test_backspace_mid_indent_deletes_one_tab() {
        // "\t\t\t" with cursor after the 2nd tab (a third tab follows).
        // Only text[..cursor] is considered; deletes exactly one tab.
        let ctx = InsertContext::new("\t\t\t", Offset::new(2));
        let result = backspace(&ctx);
        assert_eq!(deleted_range(&result), (1, 2));
    }

    #[test]
    fn test_backspace_wide_tab_deletes_whole_tab() {
        // ts=8, sw=4: one tab spans display columns 0..8; the shiftwidth
        // boundary (4) falls INSIDE the tab. Delete-only removes the whole
        // straddling tab (Neovim parity for the delete step; the space
        // refill that would leave 4 spaces is a separate follow-up).
        let ctx = InsertContext::new("\t", Offset::new(1))
            .with_shift_width(4)
            .with_tabstop(8);
        let result = backspace(&ctx);
        assert_eq!(deleted_range(&result), (0, 1));
    }

    #[test]
    fn test_backspace_spaces_unchanged() {
        // Regression guard: with spaces, byte offset == display column, so
        // behavior is unchanged — delete exactly one shiftwidth of spaces.
        let ctx = InsertContext::new("        ", Offset::new(8)).with_shift_width(4);
        let result = backspace(&ctx);
        assert_eq!(deleted_range(&result), (4, 8));
    }

    #[test]
    fn test_backspace_single_leading_space_to_bol() {
        // Regression guard: a lone leading space deletes back to column 0.
        let ctx = InsertContext::new(" x", Offset::new(1)).with_shift_width(4);
        let result = backspace(&ctx);
        assert_eq!(deleted_range(&result), (0, 1));
    }

    // ── Vi (default) delete_word ──────────────────────────────────────────────

    #[test]
    fn test_delete_word_vi_basic() {
        // "hello world|" → deletes "world" (bytes 6..11)
        let ctx = InsertContext::new("hello world", Offset::new(11));
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        assert_eq!(deleted_range(&result), (6, 11));
    }

    #[test]
    fn test_delete_word_vi_skips_trailing_space() {
        // "hello world  |" (trailing spaces) → skips spaces, deletes "world"
        let text = "hello world  ";
        let ctx = InsertContext::new(text, Offset::new(text.len()));
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        // "world" is at bytes 6..11; trailing spaces 11..13 are skipped
        assert_eq!(deleted_range(&result), (6, 13));
    }

    #[test]
    fn test_delete_word_vi_punctuation_separate_from_word() {
        // "hello." cursor at 6 → Vi stops at class boundary,
        // deletes only "." (punctuation class), leaving "hello"
        let ctx = InsertContext::new("hello.", Offset::new(6));
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        assert_eq!(deleted_range(&result), (5, 6));
    }

    // ── AltWerase ─────────────────────────────────────────────────────────────

    #[test]
    fn test_delete_word_alt_stops_at_slash() {
        // "/foo/bar/baz|" → AltWerase stops at '/', deletes only "baz" (9..13)
        let text = "/foo/bar/baz";
        let ctx = InsertContext::new(text, Offset::new(text.len()))
            .with_word_erase_style(WordEraseStyle::AltWerase);
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        // "baz" occupies bytes 9..12
        assert_eq!(deleted_range(&result), (9, 12));
    }

    #[test]
    fn test_delete_word_alt_slash_alone_deletes_slash() {
        // "/foo/|" → cursor right after '/', deletes the '/' itself
        let text = "/foo/";
        let ctx = InsertContext::new(text, Offset::new(text.len()))
            .with_word_erase_style(WordEraseStyle::AltWerase);
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        // '/' is at byte 4
        assert_eq!(deleted_range(&result), (4, 5));
    }

    #[test]
    fn test_delete_word_alt_no_slash_behaves_like_vi() {
        // "hello world|" → no slash, same as Vi: deletes "world" (6..11)
        let text = "hello world";
        let ctx = InsertContext::new(text, Offset::new(text.len()))
            .with_word_erase_style(WordEraseStyle::AltWerase);
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        assert_eq!(deleted_range(&result), (6, 11));
    }

    // ── TtyWerase ─────────────────────────────────────────────────────────────

    #[test]
    fn test_delete_word_tty_deletes_to_whitespace() {
        // "hello world|" → TtyWerase skips no leading space, deletes "world" (6..11)
        let text = "hello world";
        let ctx = InsertContext::new(text, Offset::new(text.len()))
            .with_word_erase_style(WordEraseStyle::TtyWerase);
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        assert_eq!(deleted_range(&result), (6, 11));
    }

    #[test]
    fn test_delete_word_tty_ignores_char_class() {
        // "hello.world|" → TtyWerase ignores class boundary: deletes "hello.world" (0..11)
        let text = "hello.world";
        let ctx = InsertContext::new(text, Offset::new(text.len()))
            .with_word_erase_style(WordEraseStyle::TtyWerase);
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        // No whitespace in text → deletes all the way to start
        assert_eq!(deleted_range(&result), (0, 11));
    }

    #[test]
    fn test_delete_word_tty_no_whitespace_deletes_to_start() {
        // "helloworld|" → no whitespace → deletes to byte 0
        let text = "helloworld";
        let ctx = InsertContext::new(text, Offset::new(text.len()))
            .with_word_erase_style(WordEraseStyle::TtyWerase);
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        assert_eq!(deleted_range(&result), (0, 10));
    }

    #[test]
    fn test_delete_word_tty_stops_at_space_before_token() {
        // "foo bar baz|" → TtyWerase skips trailing spaces (none here), deletes "baz" (8..11)
        let text = "foo bar baz";
        let ctx = InsertContext::new(text, Offset::new(text.len()))
            .with_word_erase_style(WordEraseStyle::TtyWerase);
        let result = delete_word(&ctx);
        assert!(!result.is_empty());
        assert_eq!(deleted_range(&result), (8, 11));
    }

    // ── other insert commands (unchanged) ─────────────────────────────────────

    #[test]
    fn test_delete_to_start() {
        let ctx = InsertContext::new("hello\nworld", Offset::new(8));
        let result = delete_to_start(&ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_delete_under_at_end() {
        let ctx = InsertContext::new("hello", Offset::new(5));
        let result = delete_under(&ctx);
        assert!(result.is_empty());
    }

    #[test]
    fn test_delete_under_mid_text() {
        let ctx = InsertContext::new("hello", Offset::new(2));
        let result = delete_under(&ctx);
        assert!(!result.is_empty());
    }
}
