//! Ex and command-line effect builders.
//!
//! Pure functions that build `Effects` for ex commands, command-line mode
//! transitions, search operations, and host-completion results.
//!
//! The execution layer calls these instead of constructing `Effect::*`
//! variants directly.

use crate::effects::Effects;
use crate::errors::VimError;
use crate::primitives::Mode;
use crate::primitives::{Direction, MarkName, MotionType, Offset, Range, RegisterName};
use compact_str::CompactString;

// ═══════════════════════════════════════════════════════════════════════
// Command-line mode transitions
// ═══════════════════════════════════════════════════════════════════════

/// Enter command-line mode.
pub fn enter_command_line() -> Effects {
    Effects::new().set_mode(Mode::CommandLine)
}

/// Cancel command-line mode (return to Normal).
pub fn cancel_command_line() -> Effects {
    Effects::new().set_mode(Mode::Normal)
}

// ═══════════════════════════════════════════════════════════════════════
// Search operations
// ═══════════════════════════════════════════════════════════════════════

/// Set search pattern and direction.
pub fn set_search_pattern(pattern: &str, direction: Direction) -> Effects {
    Effects::new()
        .set_search_pattern(CompactString::from(pattern), direction)
        .set_register(
            RegisterName::SEARCH,
            pattern.to_owned(),
            MotionType::CharWise,
        )
}

/// Clear active search highlights.
pub fn clear_highlights() -> Effects {
    Effects::new().clear_highlights()
}

/// Highlight exact search match ranges.
pub fn highlight_matches(ranges: Vec<Range>) -> Effects {
    Effects::new().highlight_matches(ranges)
}

/// Search pattern not found error.
///
/// **Side effect**: resets the mode to `Mode::Normal`. This ensures the
/// editor leaves command-line / search mode when a pattern yields no
/// matches, matching Vim's behaviour of returning to Normal on search
/// failure.
pub fn search_not_found(pattern: &str) -> Effects {
    Effects::new()
        .show_error(VimError::PatternNotFound(pattern.into()))
        .set_mode(Mode::Normal)
}

/// Search jump marks (push jump list, set `''` and `` ` ``).
pub fn search_jump_marks(cursor_offset: usize) -> Effects {
    let offset = Offset::new(cursor_offset);
    Effects::new()
        .push_jump_list(offset)
        .set_mark(MarkName::PREV_JUMP, offset, None)
        .set_mark(MarkName::PREV_JUMP_EXACT, offset, None)
}

/// No previous regular expression error.
pub fn missing_pattern() -> Effects {
    Effects::new()
        .show_error(VimError::NoPreviousPattern)
        .set_mode(Mode::Normal)
}

// ═══════════════════════════════════════════════════════════════════════
// Ex command effects
// ═══════════════════════════════════════════════════════════════════════

/// Save ex command to the `:` register.
pub fn register_ex_command(command: CompactString) -> Effects {
    Effects::new().set_register(RegisterName::LAST_COMMAND, command, MotionType::CharWise)
}

/// Visual exit marks for command-line (set `'<`, `'>`, clear selection).
pub fn visual_exit_marks(selection: &crate::primitives::SelectionRange) -> Effects {
    Effects::new()
        .set_mark(
            MarkName::VISUAL_START,
            Offset::new(selection.start().get()),
            None,
        )
        .set_mark(
            MarkName::VISUAL_END,
            Offset::new(selection.end().get()),
            None,
        )
        .clear_selection()
}

/// `:normal` command effects.
pub fn norm_command(
    start_line: usize,
    end_line: usize,
    keys: CompactString,
    remap: bool,
) -> Effects {
    use crate::primitives::LineNumber;
    Effects::new().norm_command(
        LineNumber::new(start_line),
        LineNumber::new(end_line),
        keys,
        remap,
    )
}

/// Goto-line (`:3`, `:$`, etc.).
pub fn goto_line(offset: usize) -> Effects {
    Effects::new().set_cursor(Offset::new(offset))
}

/// `:join` result (replace text, set cursor, show line count).
pub fn join_result(
    range: Range,
    joined_text: CompactString,
    cursor: Offset,
    line_count: usize,
) -> Effects {
    Effects::new()
        .replace(range, joined_text)
        .set_cursor(cursor)
        .show_message(CompactString::from(format!("{line_count} lines joined")))
}

/// Read-file completion (insert text, set cursor).
pub fn read_file_completion(offset: usize, data: CompactString) -> Effects {
    let insert_offset = Offset::new(offset);
    Effects::new()
        .insert(insert_offset, data)
        .set_cursor(insert_offset)
}

/// Filter completion (replace range, optionally set cursor).
pub fn filter_completion(
    range: Range,
    replacement: CompactString,
    cursor_offset: Option<usize>,
) -> Effects {
    let mut effects = Effects::new().begin_undo().replace(range, replacement);
    if let Some(offset) = cursor_offset {
        effects = effects.set_cursor(Offset::new(offset));
    } else {
        effects = effects.set_cursor(Offset::new(range.start().get()));
    }
    effects.end_undo()
}

// ═══════════════════════════════════════════════════════════════════════
// Inccommand preview
// ═══════════════════════════════════════════════════════════════════════

/// Compute substitute preview matches and wrap them in effects.
///
/// This is the effect-builder entry point for inccommand live preview.
/// Returns `SubstitutePreview` with computed matches, or
/// `ClearSubstitutePreview` if the pattern is empty, invalid, or has no
/// matches.
///
/// The caller is responsible for emitting `SetSearchPattern` and
/// `HighlightMatches` separately (reusing the incsearch infrastructure).
pub fn substitute_preview(
    text: &str,
    start_line: usize,
    end_line: usize,
    pattern: &str,
    replacement: &str,
    global: bool,
    max_matches: usize,
    include_original_lines: bool,
) -> Effects {
    let matches = super::substitute::compute_preview_matches(
        text,
        start_line,
        end_line,
        pattern,
        replacement,
        global,
        max_matches,
        include_original_lines,
    );
    if matches.is_empty() {
        Effects::new().clear_substitute_preview()
    } else {
        Effects::new().substitute_preview(matches)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Generic messages and errors
// ═══════════════════════════════════════════════════════════════════════

/// Show an error message.
pub fn show_error(error: VimError) -> Effects {
    Effects::new().show_error(error)
}

/// Show an informational message.
pub fn show_message(text: CompactString) -> Effects {
    Effects::new().show_message(text)
}
