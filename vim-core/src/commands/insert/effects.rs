//! Insert mode effect builders.
//!
//! Pure functions that build `Effects` from computed data.
//! The execution layer computes inputs (text, cursor, etc.)
//! and calls these to get the effects — never constructing
//! `Effect::*` variants directly.

use crate::effects::Effects;
use crate::primitives::Mode;
use crate::primitives::{MarkName, Offset, Range, ReplacedChar};
use compact_str::CompactString;

/// Build effects for inserting text at a position and moving cursor.
///
/// Used for: normal char insert, Ctrl-A (last inserted), Ctrl-R (register),
/// tab expansion, block insert replication.
pub fn insert_text_and_advance(offset: usize, text: CompactString, cursor_after: usize) -> Effects {
    Effects::new()
        .insert(Offset::new(offset), text)
        .set_cursor(Offset::new(cursor_after))
}

/// Build effects for newline with autoindent.
///
/// Optionally strips trailing whitespace after cursor and leading
/// autoindent whitespace before cursor (Neovim's `trunc_line`).
pub fn newline_with_indent(
    cursor: usize,
    trailing_strip_len: usize,
    leading_strip_len: usize,
    insert_text: &str,
    cursor_after: usize,
) -> Effects {
    let mut effects = Effects::new();
    // Strip autoindent-only whitespace before cursor (trunc_line).
    // Delete BEFORE trailing strip so offsets are consistent: both
    // deletions reference the original text, not the post-first-edit text.
    // The effect processor applies them in order, so leading strip must
    // come first (it's at a lower offset).
    if leading_strip_len > 0 {
        let strip_start = cursor.saturating_sub(leading_strip_len);
        effects = effects.delete(Range::new(Offset::new(strip_start), Offset::new(cursor)));
    }
    // The cursor has shifted left by leading_strip_len after the leading strip.
    let adj_cursor = cursor.saturating_sub(leading_strip_len);
    if trailing_strip_len > 0 {
        effects = effects.delete(Range::new(
            Offset::new(adj_cursor),
            Offset::new(adj_cursor + trailing_strip_len),
        ));
    }
    effects
        .insert(Offset::new(adj_cursor), CompactString::from(insert_text))
        .set_cursor(Offset::new(cursor_after))
}

/// Build effects for repeating insert text (counted inserts like `3iX<Esc>`).
///
/// When `is_replace` is true (Replace mode, entry_type ReplaceMode),
/// each repetition deletes existing characters before inserting, matching
/// Vim's `3Rx<Esc>` behavior where chars are overwritten, not pushed right.
///
/// `text_len` is the total document length, needed to clamp delete ranges
/// for replace mode when near end-of-line.
///
/// Returns the new insert_offset after all repetitions.
pub fn repeat_text(
    insert_offset: usize,
    repeat_text: &CompactString,
    count: u32,
    is_replace: bool,
    text: &str,
) -> (Effects, usize) {
    let mut effects = Effects::new();
    let mut offset = insert_offset;
    for _ in 1..count {
        if is_replace {
            // Delete up to repeat_text.len() chars, but stop at newline or end of text
            let remaining = &text[offset.min(text.len())..];
            let mut del_len = 0;
            for ch in remaining.chars() {
                if ch == '\n' || del_len >= repeat_text.len() {
                    break;
                }
                del_len += ch.len_utf8();
            }
            if del_len > 0 {
                effects = effects.delete(Range::new(
                    Offset::new(offset),
                    Offset::new(offset + del_len),
                ));
            }
        }
        effects = effects.insert(Offset::new(offset), repeat_text.clone());
        offset += repeat_text.len();
    }
    (effects, offset)
}

/// Build effects for stripping auto-indentation (o/O with no text typed).
pub fn strip_indent(insert_offset: usize, auto_indent_len: usize) -> (Effects, usize) {
    let indent_start = insert_offset.saturating_sub(auto_indent_len);
    let effects = Effects::new().delete(Range::from_raw(indent_start, insert_offset));
    (effects, indent_start)
}

/// Build effects for block insert replication (visual block I/A).
pub fn block_replicate(offsets: Vec<(usize, CompactString)>) -> Effects {
    let mut effects = Effects::new();
    for (byte_offset, text) in offsets {
        effects = effects.insert(Offset::new(byte_offset), text);
    }
    effects
}

/// Neovim's `ISSPECIAL` macro for the `insertchar()` batching logic.
///
/// ```
/// // Neovim edit.c:
/// // #define ISSPECIAL(c) ((c) < ' ' || (c) >= DEL || (c) == '0' || (c) == '^')
/// use vim_core::commands::insert::effects::is_special_for_batching;
/// assert!(is_special_for_batching(b'\n'));
/// assert!(is_special_for_batching(b'\x1b'));
/// assert!(is_special_for_batching(b'0'));
/// assert!(is_special_for_batching(b'^'));
/// assert!(is_special_for_batching(0x7F));
/// assert!(is_special_for_batching(0x80));
/// assert!(!is_special_for_batching(b'a'));
/// assert!(!is_special_for_batching(b' '));
/// assert!(!is_special_for_batching(b'9'));
/// assert!(!is_special_for_batching(b','));
/// ```
#[must_use]
pub const fn is_special_for_batching(c: u8) -> bool {
    c < b' ' || c >= 0x7F || c == b'0' || c == b'^'
}

/// Compute the mark `'.'` byte offset after insert mode, matching Neovim's
/// batching behaviour precisely.
///
/// Neovim's `insertchar()` (edit.c) batches consecutive ASCII characters
/// into a single `ins_str()` call under specific conditions.  Each
/// `ins_str()` / `ins_char_bytes()` / `open_line()` call triggers
/// `changed_bytes(lnum, col)`, and the LAST such call determines mark `'.'`.
///
/// The batching rules (from Neovim's `insertchar` and its inner loop):
///
///  1. **Newlines** (`\n`/`\r`): handled by `ins_eol()` → `open_line()`,
///     which calls `changed_lines(new_lnum, 0, ...)`.  Always a batch boundary.
///
///  2. **Multi-byte characters** (>= 0x80 leading byte): inserted
///     individually via `ins_char_bytes()`.  Each one is its own batch.
///
///  3. **ISSPECIAL characters**: control chars (`< ' '`), DEL+ (`>= 0x7F`),
///     literal `'0'` (0x30), literal `'^'` (0x5E).  These skip the
///     batching `if` entirely and go through `ins_char_bytes()`.
///
///  4. **Abbreviation boundary**: would break batches, but only when
///     `no_abbr == false`.  Neovim defaults `no_abbr = true` (globals.h:605)
///     and only sets it to false when abbreviations are loaded.  Our engine
///     does not support abbreviations, so this check never fires.
///
///  5. **Normal ASCII**: consecutive ASCII chars that don't hit any of
///     the above boundaries batch into a single `ins_str()` call.
///     The `changed_bytes` col is the cursor column at batch start.
///
/// `accumulated_text` is all text typed during the insert session.
/// `insert_start` is the document byte offset where insert began.
#[must_use]
pub fn compute_mark_dot_for_insert(accumulated_text: &str, insert_start: usize) -> Option<usize> {
    if accumulated_text.is_empty() {
        return None;
    }

    // We walk backward through the accumulated text to find the byte
    // position where the LAST `changed_bytes()` call would fire.
    //
    // Starting from the end:
    //   - If the last byte is a multibyte continuation/lead (>= 0x80),
    //     walk back to the start of that character.
    //   - If the last byte is ISSPECIAL ('0', '^', control, DEL+),
    //     the batch is just that one byte.
    //   - Otherwise, walk backward over consecutive batchable ASCII
    //     bytes (not multibyte, not ISSPECIAL, not newline).
    //     Stop at any batch boundary.
    let bytes = accumulated_text.as_bytes();
    let mut pos = bytes.len();

    let last = bytes[pos - 1];
    if last >= 0x80 {
        // Multi-byte character: walk back to the leading byte.
        pos -= 1;
        while pos > 0 && bytes[pos] & 0xC0 == 0x80 {
            pos -= 1;
        }
    } else if is_special_for_batching(last) {
        // ISSPECIAL: this char alone is its own changed_bytes call.
        pos -= 1;
    } else {
        // Normal batchable ASCII.  Walk backward over consecutive
        // batchable ASCII, stopping at newlines, ISSPECIAL, multibyte,
        // or the start of the text.
        // Note: no abbreviation check (no_abbr = true by default).
        while pos > 0 {
            let prev = bytes[pos - 1];
            if prev >= 0x80 || prev == b'\n' || is_special_for_batching(prev) {
                break;
            }
            pos -= 1;
        }
    }

    // Autoindent correction: Neovim's `open_line()` inserts `\n` + indent
    // as one atomic operation.  The `changed_bytes/changed_lines` call from
    // `open_line()` uses `col = indent_width`, not col=0.  Then user-typed
    // characters start a new batch from that cursor position (at the indent
    // end).  Our accumulated_text includes the autoindent whitespace after
    // `\n`, which makes the backward walk include it in the batch.  We
    // correct by skipping leading whitespace after the closest `\n` before
    // `pos` — that whitespace is autoindent, not user-typed batching input.
    //
    // Exception: if the autoindent skip reaches the END of the accumulated
    // text, that means ONLY autoindent (no user content) follows the `\n`.
    // In this case, the last `changed_bytes/changed_lines` was from the
    // `open_line()` call for the `\n` itself, which fires at the newline
    // position.  `open_line()` calls `changed_lines(lnum, col, ...)` where
    // col is the cursor column where the split happened.  In accumulated_text
    // coordinates, that's the position just before the `\n`.  To match
    // Neovim, we need the byte offset of the newline, which is `pos - 1`.
    if pos > 0 && bytes[pos - 1] == b'\n' {
        let newline_pos = pos - 1; // save position of the '\n'
                                   // Skip any spaces/tabs (= autoindent).
        while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        // If we consumed all remaining bytes, no user text follows —
        // the last change was the open_line() at the newline position.
        if pos == bytes.len() {
            pos = newline_pos;
        }
    }

    Some(insert_start + pos)
}

/// Build finalization effects for insert exit.
///
/// Emits: SetCursor, EndUndoGroup, SetMark('^'), [SetMark('.')], SetMode(Normal), SetStickyColumn.
///
/// `'^` mark is set to `insert_offset` — where the cursor was when text was
/// last inserted, BEFORE the exit cursor-back. This is what `gi` uses.
///
/// Mark `'.'` is computed via [`compute_mark_dot_for_insert`] to match
/// Neovim's ASCII-batching behaviour.  For empty inserts (`A<Esc>`,
/// `i<Esc>`), mark `'.'` is left unchanged.
///
/// `SetStickyColumn` updates curswant to the exit cursor's column so that
/// subsequent vertical motions (j/k) use the correct target column.
///
/// ORDERING NOTE: SetCursor is emitted BEFORE EndUndoGroup so the effect
/// processor's undo_cursor_hint reflects the final exit cursor position.
/// This ensures the undo tree records `cursor_after` as the post-exit
/// position (after backup), matching Neovim's redo cursor behavior.
pub fn exit_finalize(
    insert_offset: usize,
    final_cursor: usize,
    text: &str,
    accumulated_text: &str,
    is_replace: bool,
    mark_dot_override_pos: Option<usize>,
) -> Effects {
    use crate::effects::undo_state;
    let column = crate::commands::helpers::column_of(text, final_cursor);
    // SetCursor BEFORE EndUndoGroup: the effect processor updates
    // undo_cursor_hint on SetCursor, and EndUndoGroup reads it to set
    // cursor_after in the undo tree. This ensures redo restores cursor
    // to the correct exit position.
    //
    // resume_open() starts in UndoOpen state because the undo group was
    // opened by insert mode entry (begin_undo in entry.rs).
    let mut effects = Effects::<undo_state::UndoOpen>::resume_open()
        .set_cursor(Offset::new(final_cursor))
        .end_undo()
        .set_mark(MarkName::INSERT_STOP, Offset::new(insert_offset), None);
    if !accumulated_text.is_empty() {
        let insert_start = insert_offset.saturating_sub(accumulated_text.len());
        if is_replace {
            // Replace mode: Neovim does NOT batch characters (REPLACE_FLAG
            // disables insertchar() lookahead). Each overwrite calls
            // changed_bytes() individually. When a mark_dot_override is set
            // (e.g., after backspace which calls changed_bytes at the restore
            // position), use it. Otherwise the last changed_bytes() was at the
            // start of the last typed character.
            //
            // Use prev_char_boundary for multibyte correctness: the last typed
            // character may be multi-byte (e.g., CJK), and its start is not
            // simply insert_offset - 1.
            let mark_dot = if let Some(override_pos) = mark_dot_override_pos {
                insert_start + override_pos
            } else {
                // Walk back to the start of the last character in accumulated_text
                let last_char_start = accumulated_text
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i);
                insert_start + last_char_start
            };
            effects = effects.set_mark(MarkName::LAST_CHANGE, Offset::new(mark_dot), None);
        } else {
            // Insert mode: apply ASCII-batching logic.
            // When a non-batching operation (Tab, Ctrl-E/Y, Ctrl-R) set an
            // override, it represents a batch boundary in the accumulated
            // text.  Run the backward walk on the SUBSTRING from that point
            // to the end (finding the last changed_bytes position within the
            // pasted-plus-typed tail), then add the base offsets.
            let mark_dot_opt = if let Some(override_pos) = mark_dot_override_pos {
                let tail = &accumulated_text[override_pos..];
                if tail.is_empty() {
                    Some(insert_start + override_pos)
                } else {
                    compute_mark_dot_for_insert(tail, insert_start + override_pos)
                }
            } else {
                compute_mark_dot_for_insert(accumulated_text, insert_start)
            };
            if let Some(mark_dot) = mark_dot_opt {
                effects = effects.set_mark(MarkName::LAST_CHANGE, Offset::new(mark_dot), None);
            }
        }

        // Note: marks '[ and '] are set by the engine's sync_change_marks()
        // during text mutations within the undo group, and further refined
        // by handle_insert_exit() after all effects are processed.  We do
        // NOT emit them here to avoid interfering with the effect processor's
        // change tracking within the undo group.
    }
    effects = effects.set_mode(Mode::Normal);
    effects.push(crate::effects::Effect::SetStickyColumn {
        column: Some(crate::primitives::VirtualColumn::new(column)),
    });
    effects
}

/// Build effects for replace mode character overwrite.
///
/// Deletes existing character (if any), inserts new character, moves cursor.
pub fn replace_char_at(cursor: usize, ch: char, delete_len: Option<usize>) -> Effects {
    let char_len = ch.len_utf8();
    let mut effects = Effects::new();
    if let Some(del_len) = delete_len {
        effects = effects.delete(Range::new(
            Offset::new(cursor),
            Offset::new(cursor + del_len),
        ));
    }
    effects
        .insert(Offset::new(cursor), {
            let mut s = CompactString::new("");
            s.push(ch);
            s
        })
        .set_cursor(Offset::new(cursor + char_len))
}

/// Build effects for replace mode backspace across a line boundary (join lines).
///
/// When backspace in Replace mode encounters a `LineBoundary` on the replace
/// stack, this joins the current line back to the previous line. Deletes the
/// newline character and any autoindent whitespace on the current line, then
/// moves the cursor to the end of the previous line.
///
/// `prev_line_end` is the byte offset of the `\n` character.
/// `delete_len` is the total bytes to delete (`\n` + indent whitespace).
pub fn replace_backspace_join_line(prev_line_end: usize, delete_len: usize) -> Effects {
    Effects::new()
        .delete(Range::new(
            Offset::new(prev_line_end),
            Offset::new(prev_line_end + delete_len),
        ))
        .set_cursor(Offset::new(prev_line_end))
}

/// Build effects for replace mode backspace (undo overwrite).
///
/// Deletes the replacement character, optionally restores the original,
/// moves cursor back.
pub fn replace_backspace(prev_pos: usize, cursor: usize, original_char: ReplacedChar) -> Effects {
    let mut effects = Effects::new();
    effects = effects.delete(Range::new(Offset::new(prev_pos), Offset::new(cursor)));
    if let ReplacedChar::Replaced(ch) = original_char {
        effects = effects.insert(Offset::new(prev_pos), {
            let mut s = CompactString::new("");
            s.push(ch);
            s
        });
    }
    effects.set_cursor(Offset::new(prev_pos))
}

/// Analyze the character under the cursor for Replace mode.
///
/// Pure computation: examines the text at cursor position and returns:
/// - `original_char`: the character to push onto the replace stack
///   (`Some(ch)` if overwriting, `None` if at newline or past end)
/// - `delete_len`: byte length of the character being overwritten
///   (`None` if no character to overwrite)
#[must_use]
pub fn analyze_replace_target(text: &str, cursor: usize) -> (Option<char>, Option<usize>) {
    if cursor >= text.len() {
        // Past end of document — pure insertion
        return (None, None);
    }
    let existing = &text[cursor..];
    match existing.chars().next() {
        Some(ch) if ch != '\n' => (Some(ch), Some(ch.len_utf8())),
        _ => (None, None), // At newline or empty — pure insertion
    }
}

/// Build effects for one-shot normal mode (Ctrl-O).
pub fn one_shot_normal() -> Effects {
    Effects::new().set_mode(Mode::Normal)
}

/// Build effects for dot-repeat text injection.
///
/// Inserts the repeated text at the given position and places the cursor
/// one grapheme before the end (matching Vim's behavior).
pub fn repeat_insert(insert_pos: usize, text: CompactString, last_grapheme_len: usize) -> Effects {
    let text_len = text.len();
    Effects::new()
        .insert(Offset::new(insert_pos), text)
        .set_cursor(Offset::new(
            insert_pos + text_len.saturating_sub(last_grapheme_len),
        ))
}

/// Build effects for Replace mode dot-repeat text injection.
///
/// Like `repeat_insert`, but deletes existing characters before inserting,
/// stopping at newlines (matching Vim's Replace mode behavior where
/// characters past EOL are appended, not replacing the newline).
pub fn repeat_replace(
    insert_pos: usize,
    text: CompactString,
    last_grapheme_len: usize,
    doc_text: &str,
) -> Effects {
    let text_len = text.len();
    let mut effects = Effects::new();

    // Delete existing characters at the insertion point, stopping at newline
    let remaining = &doc_text[insert_pos.min(doc_text.len())..];
    let mut del_len = 0;
    for ch in remaining.chars() {
        if ch == '\n' || del_len >= text_len {
            break;
        }
        del_len += ch.len_utf8();
    }
    if del_len > 0 {
        effects = effects.delete(Range::new(
            Offset::new(insert_pos),
            Offset::new(insert_pos + del_len),
        ));
    }

    effects
        .insert(Offset::new(insert_pos), text)
        .set_cursor(Offset::new(
            insert_pos + text_len.saturating_sub(last_grapheme_len),
        ))
}
