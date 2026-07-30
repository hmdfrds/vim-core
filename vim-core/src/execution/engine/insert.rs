//! Insert-mode pipeline helpers.
//!
//! Free functions for the insert-mode processing pipeline.
//! These live outside `VimEngine` to avoid `&mut self` borrow conflicts
//! — they only need `&VimState` / `&mut VimState`, not the full engine.
//!
//! # Architecture
//!
//! The insert pipeline has three phases:
//! 1. **Pre-compute** (`precompute_insert`) — read-only state access
//! 2. **Mutate** (`apply_insert_mutations`) — state tracking updates
//! 3. **Dispatch** (`dispatch_insert`) — effect construction (in dispatch layer)
//!
//! **ALLOWED imports**: `crate::state`, `crate::dispatch`
//! **FORBIDDEN imports**: `super::VimEngine`, `super::Response` (avoids circular deps)

use crate::commands::insert::autoindent::compute_newline_insert_with_ai;
use crate::commands::textobjects::helpers::prev_char_boundary;
use crate::dispatch::{compute_tab_spaces, InsertPrecomputed, ReplaceRestoreAction};
use crate::grammar::{Command, InsertKind};

// ─────────────────────────────────────────────────────────────────────────────
// Pre-computation (read-only state access)
// ─────────────────────────────────────────────────────────────────────────────

/// Pre-compute operation data for an insert command. Read-only state access.
///
/// Returns `InsertPrecomputed` that both `apply_insert_mutations` and
/// `dispatch_insert` will consume — single computation, zero duplication.
#[expect(
    clippy::too_many_arguments,
    reason = "internal helper: every parameter is read-only context derived once at the call site; bundling them in a struct would require allocating a transient holder for no readability gain"
)]
pub(super) fn precompute_insert(
    state: &crate::state::VimState,
    command: &Command,
    insert_mode: crate::mode::InsertMode,
    text: &str,
    cursor: usize,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn crate::document::IndentProvider>,
    options: &crate::primitives::VimOptions,
) -> InsertPrecomputed {
    let is_replace = matches!(
        insert_mode,
        crate::mode::InsertMode::Replace | crate::mode::InsertMode::VirtualReplace
    );
    match command {
        Command::Insert(InsertKind::Char { char }) => {
            if is_replace && *char != '\n' {
                // Replace mode: overwrite character under cursor.
                // Newline uses insert semantics even in replace mode
                // (Vim behavior: Enter in R mode inserts a newline without
                // deleting the character under cursor).
                let (original_char_opt, delete_len) =
                    crate::commands::insert::effects::analyze_replace_target(text, cursor);
                if *char == '\t' && options.expandtab() {
                    let sts = options.effective_tab_columns();
                    let spaces = compute_tab_spaces(text, cursor, sts);
                    InsertPrecomputed::ReplaceTab {
                        spaces,
                        original_char: crate::primitives::ReplacedChar::from_option(original_char_opt),
                        delete_len,
                    }
                } else {
                    InsertPrecomputed::ReplaceChar {
                        ch: *char,
                        original_char: crate::primitives::ReplacedChar::from_option(original_char_opt),
                        delete_len,
                    }
                }
            } else if *char == '\n' {
                // ^^D saved indent: if OutdentTemporary saved an indent, the
                // next newline restores it instead of using autoindent.
                let saved_indent = state
                    .insert_state()
                    .and_then(|is| is.saved_indent().cloned());
                if let Some(ref saved) = saved_indent {
                    // Build newline with the saved indent, bypassing autoindent.
                    // Still strip trailing whitespace after cursor (Vim behavior).
                    let after_cursor = &text[cursor..];
                    let trailing_strip_len = after_cursor.len()
                        - after_cursor.trim_start_matches([' ', '\t']).len();
                    let insert_text = compact_str::format_compact!("\n{saved}");
                    let insert_len = insert_text.len();
                    InsertPrecomputed::Newline {
                        insert_text,
                        trailing_strip_len,
                        leading_strip_len: 0,
                        cursor_advance: cursor + insert_len,
                    }
                } else {
                // Check did_ai: autoindent was applied if auto_indent_len > 0.
                // This drives Neovim's trunc_line behavior (strip autoindent-only
                // whitespace on the old line when pressing Enter).
                let did_ai = state
                    .insert_state()
                    .is_some_and(|is| is.auto_indent_len() > 0);
                let mut nl = compute_newline_insert_with_ai(
                    text, cursor, tabstop, autoindent, indent_provider, did_ai,
                );
                // Smartindent: add extra shiftwidth when line before cursor ends with `{`
                if options.smartindent()
                    && indent_provider.is_none()
                    && crate::commands::insert::smartindent::should_add_indent_after_newline(
                        text, cursor,
                    )
                {
                    let sw = options.shiftwidth();
                    let extra = if options.expandtab() {
                        " ".repeat(sw)
                    } else {
                        crate::commands::insert::smartindent::build_indent_string(
                            sw, true, tabstop,
                        )
                    };
                    let base = nl.insert_text.as_str();
                    nl.insert_text = compact_str::format_compact!("{base}{extra}");
                    nl.insert_len += extra.len();
                }
                InsertPrecomputed::Newline {
                    insert_text: nl.insert_text.clone(),
                    trailing_strip_len: nl.trailing_strip_len,
                    leading_strip_len: nl.leading_strip_len,
                    cursor_advance: cursor + nl.insert_len - nl.leading_strip_len,
                }
                }
            } else if *char == '\t' && options.expandtab() {
                let sts = options.effective_tab_columns();
                let spaces = compute_tab_spaces(text, cursor, sts);
                InsertPrecomputed::Tab { spaces }
            } else if options.smartindent()
                && indent_provider.is_none()
                && matches!(*char, '{' | '}' | '#')
            {
                use crate::commands::insert::smartindent::{
                    compute_indent_replacement, compute_smartindent_action,
                };
                let action = compute_smartindent_action(
                    text,
                    cursor,
                    *char,
                    options.shiftwidth(),
                    tabstop,
                );
                if let Some((strip_start, strip_end, new_indent)) = compute_indent_replacement(
                    text,
                    cursor,
                    &action,
                    options.shiftwidth(),
                    tabstop,
                    options.expandtab(),
                ) {
                    InsertPrecomputed::CharWithIndentAdjust {
                        ch: *char,
                        strip_start,
                        strip_end,
                        new_indent: compact_str::CompactString::from(new_indent),
                    }
                } else {
                    InsertPrecomputed::Char(*char)
                }
            } else {
                InsertPrecomputed::Char(*char)
            }
        }
        Command::Insert(InsertKind::Backspace) if is_replace => {
            if let Some(is) = state.insert_state() {
                if let Some(replaced) = is.peek_replaced() {
                    if matches!(replaced, crate::primitives::ReplacedChar::LineBoundary) {
                        // LineBoundary: join current line back to previous line.
                        // Find the end of the previous line (the \n position)
                        // and the start of the current line's content (after any
                        // autoindent whitespace).
                        let line_start = crate::commands::helpers::line_start_for_offset(text, cursor);
                        if line_start == 0 {
                            // Already on the first line — shouldn't happen, but
                            // treat as stack empty.
                            InsertPrecomputed::ReplaceBackspace {
                                action: ReplaceRestoreAction::StackEmpty,
                                prev_pos: cursor,
                            }
                        } else {
                            // The \n is at line_start - 1. Delete from there
                            // through any autoindent whitespace on the current line.
                            let newline_pos = line_start - 1;
                            // Count leading whitespace on current line (autoindent)
                            let current_line_content = &text[line_start..];
                            let indent_len = current_line_content.len()
                                - current_line_content.trim_start_matches([' ', '\t']).len();
                            let delete_len = 1 + indent_len; // \n + indent
                            InsertPrecomputed::ReplaceBackspace {
                                action: ReplaceRestoreAction::JoinLine {
                                    prev_line_end: newline_pos,
                                    delete_len,
                                },
                                prev_pos: newline_pos,
                            }
                        }
                    } else {
                        let prev_pos = prev_char_boundary(text, cursor);
                        InsertPrecomputed::ReplaceBackspace {
                            action: ReplaceRestoreAction::Restore(replaced),
                            prev_pos,
                        }
                    }
                } else {
                    // Stack is empty — cursor is at or before the entry point.
                    // In Vim, backspace still moves cursor left (across lines)
                    // even when at the entry point.
                    let prev_pos = if cursor > 0 {
                        prev_char_boundary(text, cursor)
                    } else {
                        cursor
                    };
                    InsertPrecomputed::ReplaceBackspace {
                        action: ReplaceRestoreAction::StackEmpty,
                        prev_pos,
                    }
                }
            } else {
                InsertPrecomputed::None
            }
        }
        Command::Insert(InsertKind::LastInserted | InsertKind::LastInsertedAndExit) => {
            let last = state.last_inserted_text();
            if last.is_empty() {
                InsertPrecomputed::None
            } else {
                InsertPrecomputed::Text(compact_str::CompactString::from(last))
            }
        }
        Command::Insert(InsertKind::Register { register }) => {
            // "." register is special: contains last inserted text (not in register store)
            if *register == crate::primitives::RegisterName::LAST_INSERT {
                let last = state.last_inserted_text();
                if last.is_empty() {
                    InsertPrecomputed::None
                } else {
                    InsertPrecomputed::Text(compact_str::CompactString::from(last))
                }
            } else if *register == crate::primitives::RegisterName::FILENAME {
                // '%' register: current filename. If not set, Vim shows E32.
                if let Some(content) = state.registers().get(*register) {
                    let reg_text = content.text();
                    if reg_text.is_empty() {
                        InsertPrecomputed::Error(crate::errors::VimError::HostFailure("E32: No file name".into()))
                    } else {
                        InsertPrecomputed::Text(compact_str::CompactString::from(reg_text))
                    }
                } else {
                    InsertPrecomputed::Error(crate::errors::VimError::HostFailure("E32: No file name".into()))
                }
            } else if let Some(content) = state.registers().get_aliased(*register, options) {
                let reg_text = content.text();
                if reg_text.is_empty() {
                    InsertPrecomputed::None
                } else {
                    InsertPrecomputed::Text(compact_str::CompactString::from(reg_text))
                }
            } else {
                InsertPrecomputed::None
            }
        }
        // Copy char from adjacent line (Ctrl-E below, Ctrl-Y above)
        Command::Insert(InsertKind::CopyCharBelow) => {
            copy_char_from_adjacent(text, cursor, true)
        }
        Command::Insert(InsertKind::CopyCharAbove) => {
            copy_char_from_adjacent(text, cursor, false)
        }
        // Host-injected text — read staged text from state
        Command::Insert(InsertKind::HostInserted) => {
            let text = state.pending_host_insert();
            if text.is_empty() {
                InsertPrecomputed::None
            } else {
                InsertPrecomputed::Text(compact_str::CompactString::from(text))
            }
        }
        // Insert-specific commands that need no precomputation:
        // these are handled entirely by dispatch_insert / apply_insert_mutations.
        // LiteralChar from Ctrl-V: no precomputation needed (bypasses expandtab).
        Command::Insert(InsertKind::LiteralChar { char }) => InsertPrecomputed::Char(*char),
        // Expression register: evaluate expression and insert result text.
        // For simple numeric/string literals, the expression text IS the result.
        Command::Insert(InsertKind::ExpressionResult { expression }) => {
            // Simple expression evaluation: try to parse as a number, otherwise
            // insert the expression string as-is (matching Neovim for simple cases).
            let result = evaluate_simple_expression(expression);
            if result.is_empty() {
                InsertPrecomputed::None
            } else {
                InsertPrecomputed::Text(compact_str::CompactString::from(result))
            }
        }
        Command::Insert(InsertKind::InsertWordUnderCursor) => {
            let word_chars = options.word_char_set();
            let word = crate::commands::motions::word_boundary::word_under_cursor(
                text, cursor, word_chars,
            );
            if word.is_empty() {
                InsertPrecomputed::None
            } else {
                InsertPrecomputed::Text(compact_str::CompactString::from(word))
            }
        }
        Command::Insert(InsertKind::InsertWORDUnderCursor) => {
            // WORD = non-whitespace sequence under cursor
            let start = text[..cursor]
                .bytes()
                .rposition(|b| b.is_ascii_whitespace())
                .map_or(0, |i| i + 1);
            let end = text[cursor..]
                .bytes()
                .position(|b| b.is_ascii_whitespace())
                .map_or(text.len(), |i| cursor + i);
            let word = &text[start..end];
            if word.is_empty() {
                InsertPrecomputed::None
            } else {
                InsertPrecomputed::Text(compact_str::CompactString::from(word))
            }
        }
        Command::Insert(InsertKind::InsertCurrentLine) => {
            let line = crate::commands::helpers::current_line(text, cursor);
            if line.is_empty() {
                InsertPrecomputed::None
            } else {
                InsertPrecomputed::Text(compact_str::CompactString::from(line))
            }
        }
        Command::Insert(
            InsertKind::Backspace       // normal-mode backspace (replace handled above)
            | InsertKind::DeleteWord
            | InsertKind::DeleteToStart
            | InsertKind::DeleteUnder
            | InsertKind::Indent
            | InsertKind::Outdent
            | InsertKind::OutdentTemporary
            | InsertKind::OutdentClear
            | InsertKind::OneShot
            | InsertKind::Paste
            | InsertKind::BreakUndoSequence
            | InsertKind::DontSyncUndo
            | InsertKind::Nop
            | InsertKind::RequestCompletion { .. }
            | InsertKind::ToggleLangmap,
        )
        | Command::InsertExit
        | Command::InsertEntry { .. } => InsertPrecomputed::None,

        // Non-insert commands should never reach precompute_insert.
        // This is called exclusively from execute_insert_command which only
        // receives insert-specific commands (is_insert_specific() == true).
        other => {
            debug_assert!(false, "non-insert command {other:?} reached precompute_insert");
            InsertPrecomputed::None
        }
    }
}

/// Find the character at the same column on an adjacent line.
///
/// `below == true` means line below (Ctrl-E), `false` means above (Ctrl-Y).
fn copy_char_from_adjacent(text: &str, cursor: usize, below: bool) -> InsertPrecomputed {
    use crate::commands::helpers::{line_content, line_of, line_start};
    use unicode_segmentation::UnicodeSegmentation;

    let current_line = line_of(text, cursor);
    let target_line = if below {
        current_line + 1
    } else {
        if current_line == 0 {
            return InsertPrecomputed::None;
        }
        current_line - 1
    };

    let current_start = line_start(text, current_line).unwrap_or(0);

    // Convert byte column to grapheme column (handles multibyte correctly)
    let current_content = text.get(current_start..cursor).unwrap_or("");
    let grapheme_col = current_content.graphemes(true).count();

    let Some(target_content) = line_content(text, target_line) else {
        return InsertPrecomputed::None;
    };

    // Find the grapheme at the same grapheme column in the target line
    if let Some(grapheme) = target_content
        .graphemes(true)
        .filter(|g| *g != "\n")
        .nth(grapheme_col)
    {
        // Return the first char of the grapheme
        if let Some(ch) = grapheme.chars().next() {
            return InsertPrecomputed::Char(ch);
        }
    }
    // If byte_col approach would work for ASCII-only case, keep as fallback
    // but the grapheme approach above handles all cases correctly
    InsertPrecomputed::None
}

// ─────────────────────────────────────────────────────────────────────────────
// State mutations (write access to VimState only)
// ─────────────────────────────────────────────────────────────────────────────

/// Apply insert-mode state mutations using pre-computed data.
///
/// This is the ONLY place accumulated text tracking and replace stack
/// management happen. Uses the same `InsertPrecomputed` data that
/// `dispatch_insert` will use for effect construction — zero duplication.
pub(super) fn apply_insert_mutations(
    state: &mut crate::state::VimState,
    command: &Command,
    precomputed: &InsertPrecomputed,
) {
    // Host-injected text: take staged text before borrowing insert_state,
    // since take_pending_host_insert needs &mut state.
    if matches!(command, Command::Insert(InsertKind::HostInserted)) {
        let text = state.take_pending_host_insert();
        if let Some(insert_state) = state.insert_state_mut() {
            insert_state.push_str(&text);
        }
        return;
    }

    let is_replace_mode = matches!(
        state.mode(),
        crate::primitives::Mode::Replace | crate::primitives::Mode::VirtualReplace
    );

    let Some(insert_state) = state.insert_state_mut() else {
        return;
    };

    match (command, precomputed) {
        // Normal character — goes through Neovim's insertchar() batching.
        // Clear override if it's a "point" override (from Tab/Ctrl-E/Ctrl-Y
        // at the end of accumulated text). Keep it if it's a "batch boundary"
        // override (from Ctrl-R paste earlier in accumulated text) — the
        // normal char extends the current batch in Neovim.
        (
            Command::Insert(InsertKind::Char { .. } | InsertKind::LiteralChar { .. }),
            InsertPrecomputed::Char(ch),
        ) => {
            let is_boundary_override = insert_state
                .mark_dot_override_pos()
                .is_some_and(|pos| pos + 1 < insert_state.accumulated_text().len());
            if !is_boundary_override {
                insert_state.clear_mark_dot_override();
            }
            insert_state.push_char(*ch);
        }
        // Newline — open_line() pathway, separate changed_bytes call.
        // Clear override; backward walk handles newlines correctly.
        (
            Command::Insert(InsertKind::Char { .. }),
            InsertPrecomputed::Newline { insert_text, .. },
        ) => {
            // Record auto-indent length for indent-aware dot-repeat.
            // insert_text starts with '\n', so indent bytes = total - 1.
            let indent_len = insert_text.len().saturating_sub(1);
            insert_state.push_newline_indent_len(indent_len);

            insert_state.clear_mark_dot_override();
            for c in insert_text.chars() {
                insert_state.push_char(c);
            }
            // In Replace mode, Enter inserts a newline (doesn't overwrite).
            // Push LineBoundary sentinel so backspace can join the line back.
            if is_replace_mode {
                insert_state.push_replaced(crate::primitives::ReplacedChar::LineBoundary);
            }
            // Clear ^^D saved indent after the newline consumes it.
            // The precompute phase already used the saved indent (if any)
            // to build insert_text, so clearing it here is all that remains.
            insert_state.clear_saved_indent();
        }
        // Tab — ins_tab() pathway: each expanded space gets its own
        // changed_bytes() call (ins_char for first, ins_str for rest).
        // Set override to position of last expanded space.
        (Command::Insert(InsertKind::Char { .. }), InsertPrecomputed::Tab { spaces }) => {
            for _ in 0..*spaces {
                insert_state.push_char(' ');
            }
            // Mark '.' = position of last expanded space in accumulated text.
            let pos = insert_state.accumulated_text().len().saturating_sub(1);
            insert_state.set_mark_dot_override(pos);
        }
        // Replace mode char — track typed char + replaced original
        (
            Command::Insert(InsertKind::Char { char }),
            InsertPrecomputed::ReplaceChar { original_char, .. },
        ) => {
            insert_state.push_char(*char);
            insert_state.push_replaced(*original_char); // ReplacedChar: Copy
        }
        // Replace mode tab — track expanded spaces + replaced original
        (
            Command::Insert(InsertKind::Char { .. }),
            InsertPrecomputed::ReplaceTab {
                spaces,
                original_char,
                ..
            },
        ) => {
            for _ in 0..*spaces {
                insert_state.push_char(' ');
            }
            insert_state.push_replaced(*original_char);
        }
        // Replace mode backspace — undo tracking
        (
            Command::Insert(InsertKind::Backspace),
            InsertPrecomputed::ReplaceBackspace { action, .. },
        ) => {
            match action {
                ReplaceRestoreAction::JoinLine { .. } => {
                    // LineBoundary: undo the newline + indent from accumulated text.
                    // Pop the LineBoundary from the replace stack.
                    insert_state.pop_replaced();
                    // Pop the newline_indent_lens entry to get the indent byte count.
                    let indent_len = insert_state.pop_newline_indent_len().unwrap_or(0);
                    // Pop indent chars + newline from accumulated text.
                    for _ in 0..indent_len {
                        insert_state.pop_char();
                    }
                    insert_state.pop_char(); // pop the '\n'
                }
                _ => {
                    insert_state.pop_char();
                    insert_state.pop_replaced();
                }
            }
            // Backspace in replace mode: del_char → changed_bytes(lnum, col).
            // col = position of deleted char = current accumulated_text length.
            let pos = insert_state.accumulated_text().len();
            insert_state.set_mark_dot_override(pos);
        }
        // Normal backspace — del_char → changed_bytes(lnum, col)
        // where col = position of deleted character = current length after pop.
        (Command::Insert(InsertKind::Backspace), _) => {
            if insert_state.pop_char().is_none() && insert_state.auto_indent_len() > 0 {
                insert_state.set_auto_indent_len(insert_state.auto_indent_len() - 1);
            }
            let pos = insert_state.accumulated_text().len();
            insert_state.set_mark_dot_override(pos);
        }
        // One-shot normal mode — no state mutations here.
        // The engine sets return_to on VimState after apply_insert_mutations.
        (Command::Insert(InsertKind::OneShot), _) => {}

        // Copy char from adjacent line (Ctrl-E/Y) — goes through
        // insert_special() → insertchar() in Neovim. Each Ctrl-E/Y
        // produces its own changed_bytes() call, creating a batch boundary.
        // Set override to the position of this character.
        (
            Command::Insert(InsertKind::CopyCharBelow | InsertKind::CopyCharAbove),
            InsertPrecomputed::Char(ch),
        ) => {
            let pos = insert_state.accumulated_text().len();
            insert_state.push_char(*ch);
            insert_state.set_mark_dot_override(pos);
        }
        // Register / expression / last-inserted — goes through
        // stuffescaped() → typeahead → insertchar() in Neovim.
        // The typeahead insertion creates a batch boundary at the start
        // of the pasted content: the previous ASCII batch is flushed by
        // ins_str() before Ctrl-R is processed, and pasted characters
        // start a new batch that continues until the next special char.
        // Set override to the paste start position — Neovim's last
        // changed_bytes() fires at this offset (the batch starting
        // point), which determines mark '.'.
        (
            Command::Insert(
                InsertKind::LastInserted
                | InsertKind::LastInsertedAndExit
                | InsertKind::Register { .. }
                | InsertKind::ExpressionResult { .. }
                | InsertKind::InsertWordUnderCursor
                | InsertKind::InsertWORDUnderCursor
                | InsertKind::InsertCurrentLine,
            ),
            InsertPrecomputed::Text(text),
        ) => {
            let pos = insert_state.accumulated_text().len();
            insert_state.push_str(text);
            insert_state.set_mark_dot_override(pos);
        }
        // Everything else — no state mutations needed
        _ => {}
    }
}

/// Track a character inserted by Select mode's type-to-replace.
///
/// Called from the `SelectReplace` handler in `execute_mode_action` after
/// the Change operator deletes the selection and enters Insert mode.
/// The character is appended to the insert session's accumulated text
/// so that `.` repeat and `^A` (insert previous) work correctly.
pub(super) fn track_select_replace_char(state: &mut crate::state::VimState, ch: char) {
    if let Some(insert_state) = state.insert_state_mut() {
        insert_state.push_char(ch);
    }
}

/// Reconcile host-owned external edit bookkeeping into insert state.
///
/// `deleted_len` is a byte count (from the external edit's deleted range),
/// so we truncate by bytes rather than popping Unicode characters.
pub(super) fn reconcile_external_edit_mutations(
    state: &mut crate::state::VimState,
    deleted_len: usize,
    inserted: &str,
) {
    let Some(insert_state) = state.insert_state_mut() else {
        return;
    };

    if deleted_len > 0 {
        insert_state.truncate_tail_bytes(deleted_len);
    }
    if !inserted.is_empty() {
        insert_state.push_str(inserted);
    }
}

/// Evaluate a simple VimL expression for `<C-r>=`.
///
/// Handles the most common cases:
/// - Numeric literals: `42` → `"42"`, `0xFF` → `"255"`
/// - String literals: `"hello"` → `"hello"`, `'hello'` → `"hello"`
/// - Simple arithmetic: `1+2` → `"3"`
///
/// For complex expressions, the host should override via `HostRequest::EvaluateExpression`.
pub(super) fn evaluate_simple_expression(expr: &str) -> String {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Try simple integer expression
    if let Some(val) = try_eval_int_expr(trimmed) {
        return val.to_string();
    }

    // String literal: "..." or '...'
    if ((trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
        && trimmed.len() >= 2
    {
        return trimmed
            .get(1..trimmed.len() - 1)
            .map_or_else(|| trimmed.to_owned(), str::to_owned);
    }

    // Fallback: return expression as-is
    trimmed.to_owned()
}

/// Try to evaluate a simple integer arithmetic expression.
///
/// Supports: integer literals, `+`, `-`, `*`, `/`.
fn try_eval_int_expr(s: &str) -> Option<i64> {
    // Pure integer literal
    if let Ok(v) = s.parse::<i64>() {
        return Some(v);
    }
    // Hex literal
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if let Ok(v) = i64::from_str_radix(hex, 16) {
            return Some(v);
        }
    }
    // Simple binary operation: num op num
    for op in ['+', '-', '*', '/'] {
        if let Some(pos) = s.rfind(op) {
            if pos > 0 {
                let left = s[..pos].trim();
                let right = s[pos + 1..].trim();
                if let (Some(l), Some(r)) = (try_eval_int_expr(left), try_eval_int_expr(right)) {
                    return match op {
                        '+' => Some(l.checked_add(r).unwrap_or(0)),
                        '-' => Some(l.checked_sub(r).unwrap_or(0)),
                        '*' => Some(l.checked_mul(r).unwrap_or(0)),
                        '/' if r != 0 => Some(l.checked_div(r).unwrap_or(0)),
                        _ => None,
                    };
                }
            }
        }
    }
    None
}
