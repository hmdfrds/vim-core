//! Insert mode entry commands.
//!
//! Handles entering insert mode from various contexts (i, a, I, A, o, O, s, S, c).
//!
//! # Commands
//!
//! | Key | Function | Description |
//! |-----|----------|-------------|
//! | `i` | `before_cursor` | Insert before cursor |
//! | `a` | `after_cursor` | Insert after cursor |
//! | `I` | `first_non_blank` | Insert at first non-blank |
//! | `A` | `end_of_line` | Insert at end of line |
//! | `o` | `new_line_below` | Open line below |
//! | `O` | `new_line_above` | Open line above |
//! | `s` | `substitute_char` | Substitute character |
//! | `S` | `substitute_line` | Substitute line |

use super::autoindent::expand_tabs_to_spaces;
use crate::commands::helpers::{
    first_non_blank_in_line, line_content, line_end, line_of, line_start,
};
use crate::commands::CommandResult;
use crate::document::{IndentProvider, IndentResult};
use crate::effects::Effects;
use crate::primitives::InsertEntryType;
use crate::primitives::{LineNumber, MotionType, Offset, Range, RegisterName};

/// Execute insert mode entry based on entry type.
///
/// Returns effects for setting mode and positioning cursor.
/// The count parameter controls how many times text is repeated on exit.
#[inline]
#[expect(
    clippy::too_many_arguments,
    reason = "every parameter is independently sourced (document, cursor, entry kind, count, register, indent context); a struct wrapper would just shift the burden to the call site"
)]
pub fn execute(
    text: &str,
    cursor: usize,
    entry_type: InsertEntryType,
    count: u32,
    register: Option<RegisterName>,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn IndentProvider>,
) -> CommandResult {
    // For substitute commands (s/S), count controls deletion, not insert repetition.
    // For R (ReplaceMode), count controls repeat-on-exit (e.g., 3Rx<Esc> → "xxx" overwriting).
    // For other entries (i/a/I/A/o/O), count controls insert repetition on exit (e.g., 3iX<Esc> → "XXX").
    let insert_count = match entry_type {
        InsertEntryType::SubstituteChar | InsertEntryType::SubstituteLine => 1,
        _ => count,
    };

    // Pre-compute auto-indent length for o/O. This is needed so handle_insert_exit
    // can strip trailing whitespace when no text is typed before <Esc>.
    // Uses expanded indent length (tabs → spaces) for expandtab support.
    let auto_indent_len = match entry_type {
        InsertEntryType::NewLineBelow | InsertEntryType::NewLineAbove => {
            let line = line_of(text, cursor);
            get_line_indent(text, line, tabstop, autoindent, indent_provider).cursor_indent_len()
        }
        _ => 0,
    };

    // Compute the byte offset where the cursor will be when typing begins.
    // This is the boundary for <C-u>/<C-w> — they won't delete past this.
    let entry_offset = compute_entry_offset(
        text,
        cursor,
        entry_type,
        tabstop,
        autoindent,
        indent_provider,
    );

    // For o/O/S, undo should restore cursor to pre-command position,
    // not to the structural edit offset. S deletes after indent (offset 4
    // on "    hello") but cursor may be at col 0 — FirstEdit would place
    // undo cursor at 4, not 0.
    let force_entry_cursor = matches!(
        entry_type,
        InsertEntryType::NewLineBelow
            | InsertEntryType::NewLineAbove
            | InsertEntryType::SubstituteLine
    );

    // Base effects: BeginUndoGroup + BeginInsert (with entry_type, insert_count, auto_indent_len)
    let begin_undo = if force_entry_cursor {
        Effects::new().begin_undo_force_entry()
    } else {
        Effects::new().begin_undo()
    };
    let mut effects = begin_undo.begin_insert(
        entry_type,
        insert_count,
        auto_indent_len,
        Offset::new(entry_offset),
    );

    // Dispatch to per-entry-type handler — returns ONLY structural effects
    // (inserts, deletions, register writes). Cursor position comes from
    // compute_entry_offset() above — single source of truth.
    let structural = match entry_type {
        // i / c<motion> / a / I / A / gI / R: cursor-only, no structural effects needed
        InsertEntryType::BeforeCursor
        | InsertEntryType::ChangeOperator
        | InsertEntryType::AfterCursor
        | InsertEntryType::FirstNonBlank
        | InsertEntryType::EndOfLine
        | InsertEntryType::Column0
        | InsertEntryType::ReplaceMode => Effects::new(),

        // o / O: insert newline + indent (structural)
        InsertEntryType::NewLineBelow => {
            structural_new_line_below(text, cursor, tabstop, autoindent, indent_provider)
        }
        InsertEntryType::NewLineAbove => {
            structural_new_line_above(text, cursor, tabstop, autoindent, indent_provider)
        }

        // s: delete characters under cursor (structural)
        InsertEntryType::SubstituteChar => {
            structural_substitute_char(text, cursor, count, register)
        }

        // S: delete line content (with count: delete count lines, structural)
        InsertEntryType::SubstituteLine => {
            structural_substitute_line(text, cursor, count, register)
        }
    };
    effects.extend(structural);

    // Set cursor to the position computed by compute_entry_offset()
    effects = effects.set_cursor(Offset::new(entry_offset));

    CommandResult::effects_only(effects.into_raw_closed())
}

/// `o`: Insert newline + indent below current line (structural effects only).
fn structural_new_line_below(
    text: &str,
    cursor: usize,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn IndentProvider>,
) -> Effects {
    let line = line_of(text, cursor);
    let eol = line_end(text, line).unwrap_or(text.len());
    let result = get_line_indent(text, line, tabstop, autoindent, indent_provider);
    let text_to_insert = match &result {
        IndentResult::Simple { indent, append } => {
            let append_str = append.as_deref().unwrap_or("");
            compact_str::format_compact!("\n{indent}{append_str}")
        }
        IndentResult::IndentOutdent {
            indent,
            append,
            closing_indent,
        } => {
            let append_str = append.as_deref().unwrap_or("");
            compact_str::format_compact!("\n{indent}{append_str}\n{closing_indent}")
        }
    };
    // Neovim sets mark.. and mark.[ to the start of the new line (past the
    // newline char), not the position where the newline was inserted.
    let new_line_start = Offset::new(eol + 1);
    Effects::new()
        .insert(Offset::new(eol), text_to_insert)
        .set_mark(
            crate::primitives::MarkName::LAST_CHANGE,
            new_line_start,
            None,
        )
        .set_mark(
            crate::primitives::MarkName::CHANGE_START,
            new_line_start,
            None,
        )
}

/// `O`: Insert newline + indent above current line (structural effects only).
fn structural_new_line_above(
    text: &str,
    cursor: usize,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn IndentProvider>,
) -> Effects {
    let line = line_of(text, cursor);
    let line_start_offset = line_start(text, line).unwrap_or(0);
    let result = get_line_indent(text, line, tabstop, autoindent, indent_provider);
    let text_to_insert = match &result {
        IndentResult::Simple { indent, append } => {
            let append_str = append.as_deref().unwrap_or("");
            compact_str::format_compact!("{indent}{append_str}\n")
        }
        IndentResult::IndentOutdent {
            indent,
            append,
            closing_indent,
        } => {
            let append_str = append.as_deref().unwrap_or("");
            compact_str::format_compact!("{indent}{append_str}\n{closing_indent}\n")
        }
    };
    // For O, Neovim sets mark.. and mark.[ to the start of the new line
    // (which is at line_start_offset after the insert shifts things down).
    Effects::new()
        .insert(Offset::new(line_start_offset), text_to_insert)
        .set_mark(
            crate::primitives::MarkName::LAST_CHANGE,
            Offset::new(line_start_offset),
            None,
        )
        .set_mark(
            crate::primitives::MarkName::CHANGE_START,
            Offset::new(line_start_offset),
            None,
        )
}

/// `s`: Delete count characters under cursor (structural effects only).
fn structural_substitute_char(
    text: &str,
    cursor: usize,
    count: u32,
    register: Option<RegisterName>,
) -> Effects {
    // Walk character boundaries — count is CHARACTER count, not byte count.
    // Stops at newlines: `s` stays within current line.
    let remaining = &text[cursor..];
    let mut end = cursor;
    for c in remaining.chars().take(count as usize) {
        if c == '\n' {
            break;
        }
        end += c.len_utf8();
    }
    if cursor < end {
        let deleted = &text[cursor..end];
        let mut effects = Effects::new();
        if !register.is_some_and(RegisterName::is_blackhole) {
            effects = effects
                .set_register(RegisterName::UNNAMED, deleted, MotionType::CharWise)
                .set_register(RegisterName::SMALL_DELETE, deleted, MotionType::CharWise);
        }
        effects.delete(Range::new(Offset::new(cursor), Offset::new(end)))
    } else {
        Effects::new()
    }
}

/// `S`: Delete line content past indentation (structural effects only).
///
/// Preserves the leading whitespace of the first affected line (autoindent
/// behavior matching Vim).  The full line content (including whitespace) is
/// still stored in the register for linewise paste fidelity.
fn structural_substitute_line(
    text: &str,
    cursor: usize,
    count: u32,
    register: Option<RegisterName>,
) -> Effects {
    let line = line_of(text, cursor);
    let line_start_offset = line_start(text, line).unwrap_or(0);

    // With count > 1, S deletes count lines (current + count-1 below).
    let last_line = line_of(text, text.len().saturating_sub(1));
    let end_line = (line + count as usize - 1).min(last_line);
    let end_eol = line_end(text, end_line).unwrap_or(text.len());

    // Register content: all affected lines (linewise, with trailing \n)
    let mut effects = Effects::new();
    if !register.is_some_and(RegisterName::is_blackhole) {
        // Include content from line_start to the newline after the last line
        let reg_end = if end_eol < text.len() {
            end_eol + 1
        } else {
            end_eol
        };
        let deleted = &text[line_start_offset..reg_end];
        let register_text = if deleted.ends_with('\n') {
            deleted.to_owned()
        } else {
            format!("{deleted}\n")
        };
        effects = effects
            .set_register(
                RegisterName::UNNAMED,
                register_text.as_str(),
                MotionType::LineWise,
            )
            .set_register(
                RegisterName::NUMBERED_1,
                register_text.as_str(),
                MotionType::LineWise,
            );
    }

    // Compute leading whitespace of the first line — this is preserved
    // as autoindent (delete starts after whitespace, not at line start).
    let first_line_content = &text[line_start_offset..line_end(text, line).unwrap_or(text.len())];
    let ws_len = first_line_content.len() - first_line_content.trim_start().len();
    let delete_start = line_start_offset + ws_len;

    // Delete from after whitespace to end of last line content (keep
    // the trailing newline so remaining lines stay separate).
    if delete_start < end_eol {
        effects = effects.delete(Range::new(Offset::new(delete_start), Offset::new(end_eol)));
    }
    effects
}

/// Where `i`, `a`, `I`, `A`, `gI`, `R` or `s` from `cursor` puts the cursor
/// to type, without the text changes the command makes first.
#[must_use]
pub(crate) fn entry_offset(text: &str, cursor: usize, entry_type: InsertEntryType) -> usize {
    compute_entry_offset(text, cursor, entry_type, 8, false, None)
}

/// Compute the byte offset where the cursor will land when insert mode begins.
///
/// Each entry type determines a final cursor position before typing starts.
/// This offset becomes the boundary for `<C-u>` and `<C-w>`.
fn compute_entry_offset(
    text: &str,
    cursor: usize,
    entry_type: InsertEntryType,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn IndentProvider>,
) -> usize {
    match entry_type {
        // i / c<motion> / R: cursor stays where it is
        InsertEntryType::BeforeCursor
        | InsertEntryType::ChangeOperator
        | InsertEntryType::ReplaceMode => cursor,

        // a: cursor moves right by one character, but NOT past a newline
        // (on an empty line, `a` inserts before the newline, not after it)
        InsertEntryType::AfterCursor => {
            let at_newline = text.as_bytes().get(cursor) == Some(&b'\n');
            if at_newline {
                cursor
            } else {
                let char_len = text[cursor..].chars().next().map_or(1, char::len_utf8);
                (cursor + char_len).min(text.len())
            }
        }

        // gI: cursor moves to column 0 (start of line)
        InsertEntryType::Column0 => {
            let line = line_of(text, cursor);
            line_start(text, line).unwrap_or(0)
        }

        // I: cursor moves to first non-blank of current line
        InsertEntryType::FirstNonBlank => {
            let line = line_of(text, cursor);
            if let Some(content) = line_content(text, line) {
                let line_start_offset = line_start(text, line).unwrap_or(0);
                let is_all_blank = content.chars().all(char::is_whitespace);
                if is_all_blank {
                    line_start_offset + content.len()
                } else {
                    line_start_offset + first_non_blank_in_line(content)
                }
            } else {
                cursor
            }
        }

        // A: cursor moves to end of line
        InsertEntryType::EndOfLine => {
            let line = line_of(text, cursor);
            line_end(text, line).unwrap_or(cursor)
        }

        // o: cursor goes to eol + 1 (past newline) + indent + append length
        InsertEntryType::NewLineBelow => {
            let line = line_of(text, cursor);
            let eol = line_end(text, line).unwrap_or(text.len());
            let result = get_line_indent(text, line, tabstop, autoindent, indent_provider);
            eol + 1 + result.cursor_indent_len()
        }

        // O: cursor goes to line_start + indent + append length
        InsertEntryType::NewLineAbove => {
            let line = line_of(text, cursor);
            let line_start_offset = line_start(text, line).unwrap_or(0);
            let result = get_line_indent(text, line, tabstop, autoindent, indent_provider);
            line_start_offset + result.cursor_indent_len()
        }

        // s: cursor stays (deletion happens before, but cursor stays at start)
        InsertEntryType::SubstituteChar => cursor,

        // S: cursor goes to end of preserved leading whitespace (autoindent).
        // Matches Vim behavior where S preserves the line's indentation.
        InsertEntryType::SubstituteLine => {
            let line = line_of(text, cursor);
            let ls = line_start(text, line).unwrap_or(0);
            let le = line_end(text, line).unwrap_or(text.len());
            let first_line = &text[ls..le];
            let ws_len = first_line.len() - first_line.trim_start().len();
            ls + ws_len
        }
    }
}

/// Get the indent for a new line opened after `line`.
///
/// If an `IndentProvider` is supplied, delegates to it for language-aware
/// indentation (e.g., extra indent after `{`, `:`, etc.).
/// Otherwise falls back to copying the line's leading whitespace with tab
/// expansion — the basic autoindent behavior — but only when `autoindent`
/// is true.  When `autoindent` is false and no provider is set, returns
/// an empty string (no indent), matching Vim's `set noautoindent` behavior.
fn get_line_indent(
    text: &str,
    line: usize,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn IndentProvider>,
) -> IndentResult {
    if let Some(provider) = indent_provider {
        return provider.indent_for_new_line(LineNumber::from(line));
    }
    let indent = if autoindent {
        if let Some(content) = line_content(text, line) {
            let whitespace_len = content.len() - content.trim_start().len();
            compact_str::CompactString::from(expand_tabs_to_spaces(
                &content[..whitespace_len],
                tabstop,
            ))
        } else {
            compact_str::CompactString::default()
        }
    } else {
        compact_str::CompactString::default()
    };
    IndentResult::Simple {
        indent,
        append: None,
    }
}

/// Enter insert mode at cursor, optionally moving to a target offset (for `gi`).
///
/// Pure function: `(cursor, target_offset, text_len) → Effects`.
/// If `target` is provided, it is clamped to `text_len` (not `text_len - 1`)
/// because insert mode allows the cursor past the last character.
pub fn enter_insert_at(
    cursor: usize,
    target: Option<usize>,
    text_len: usize,
    count: u32,
) -> CommandResult {
    let entry_offset = if let Some(offset) = target {
        offset.min(text_len)
    } else {
        cursor
    };
    let effects = Effects::new()
        .begin_undo()
        .begin_insert(
            InsertEntryType::BeforeCursor,
            count,
            0,
            Offset::new(entry_offset),
        )
        .set_cursor(Offset::new(entry_offset));
    CommandResult::effects_only(effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_before_cursor() {
        let result = execute(
            "hello",
            2,
            InsertEntryType::BeforeCursor,
            1,
            None,
            4,
            true,
            None,
        );
        // Should have BeginUndoGroup + BeginInsert + SetCursor (unified)
        assert_eq!(result.effects.len(), 3);
    }

    #[test]
    fn test_after_cursor() {
        let result = execute(
            "hello",
            2,
            InsertEntryType::AfterCursor,
            1,
            None,
            4,
            true,
            None,
        );
        // Should have BeginUndoGroup + BeginInsert + SetCursor
        assert_eq!(result.effects.len(), 3);
    }

    #[test]
    fn test_new_line_below() {
        let result = execute(
            "hello\nworld",
            2,
            InsertEntryType::NewLineBelow,
            1,
            None,
            4,
            true,
            None,
        );
        // Should have BeginUndoGroup + BeginInsert + Insert + SetMark(.) + SetMark([) + SetCursor
        assert_eq!(result.effects.len(), 6);
    }

    // ── IndentProvider tests ─────────────────────────────────────────

    struct MockIndentProvider {
        indent: compact_str::CompactString,
    }

    impl IndentProvider for MockIndentProvider {
        fn indent_for_new_line(&self, _line: LineNumber) -> crate::document::IndentResult {
            crate::document::IndentResult::Simple {
                indent: self.indent.clone(),
                append: None,
            }
        }
    }

    #[test]
    fn test_new_line_below_with_indent_provider() {
        let provider = MockIndentProvider {
            indent: compact_str::CompactString::from("        "),
        }; // 8 spaces
        let result = execute(
            "if true {",
            9,
            InsertEntryType::NewLineBelow,
            1,
            None,
            4,
            true,
            Some(&provider),
        );
        // Should have BeginUndoGroup + BeginInsert + Insert + SetMark(.) + SetMark([) + SetCursor
        assert_eq!(result.effects.len(), 6);
        // The Insert effect should contain "\n" + 8 spaces
        let has_correct_insert = result.effects.iter().any(
            |e| matches!(e, crate::effects::Effect::Insert { text, .. } if *text == "\n        "),
        );
        assert!(has_correct_insert, "Expected insert with 8-space indent");
    }

    #[test]
    fn test_new_line_above_with_indent_provider() {
        let provider = MockIndentProvider {
            indent: compact_str::CompactString::from("    "),
        }; // 4 spaces
        let result = execute(
            "hello\nworld",
            7,
            InsertEntryType::NewLineAbove,
            1,
            None,
            4,
            true,
            Some(&provider),
        );
        // Should have BeginUndoGroup + BeginInsert + Insert + SetMark(.) + SetMark([) + SetCursor
        assert_eq!(result.effects.len(), 6);
        // The Insert effect for O should contain indent + "\n"
        let has_correct_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Insert { text, .. } if *text == "    \n"));
        assert!(
            has_correct_insert,
            "Expected insert with 4-space indent above"
        );
    }

    #[test]
    fn test_new_line_below_none_provider_copies_whitespace() {
        let result = execute(
            "    hello",
            9,
            InsertEntryType::NewLineBelow,
            1,
            None,
            4,
            true,
            None,
        );
        // Fallback: copies 4 spaces from current line
        let has_correct_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Insert { text, .. } if *text == "\n    "));
        assert!(has_correct_insert, "Expected insert copying 4-space indent");
    }

    #[test]
    fn test_new_line_below_noautoindent() {
        let result = execute(
            "    hello",
            9,
            InsertEntryType::NewLineBelow,
            1,
            None,
            4,
            false,
            None,
        );
        // With autoindent=false and no provider, no indent should be added
        let has_correct_insert = result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Insert { text, .. } if *text == "\n"));
        assert!(
            has_correct_insert,
            "Expected insert with no indent when autoindent=false"
        );
    }
}
