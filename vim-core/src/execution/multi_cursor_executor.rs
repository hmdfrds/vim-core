//! Multi-cursor command executor.
//!
//! Dispatches [`MultiCursorCommand`] variants, modifying the engine's
//! [`MultiCursorState`] stored on [`VimState`].
//!
//! # Design
//!
//! Commands are split into two categories:
//!
//! **Pure selection commands** (no document context needed):
//! - `AddCursor` — push a new cursor at a byte offset
//! - `RemoveCursor` — remove the cursor nearest to a byte offset
//! - `ClearSecondary` — collapse to the primary cursor only
//! - `RotatePrimary` — cycle the primary designation forward/backward
//!
//! **Context-dependent commands** (require document text and/or search state):
//! - `AddCursorVertical` — needs line structure to compute above/below
//! - `AddCursorsAtMatches` — needs current search pattern + document text
//! - `SelectAllOccurrences` — needs word-under-cursor + document text
//! - `AddNextMatch` — needs current match state + document text
//!
//! Context-dependent commands receive a [`MultiCursorContext`] that provides
//! document text, search pattern, and line count.

use crate::commands::helpers::{self, CharClass};
use crate::commands::selections::{manipulation, refinement};
use crate::effects::Effect;
use crate::errors::VimError;
use crate::primitives::{Direction, Offset, SelectionRange, Selections, WordCharSet, WordKind};
use crate::regex::VimRegex;
use crate::state::MatchSearchState;
use crate::state::MultiCursorCommand;
use crate::state::VimState;

/// Context required by multi-cursor commands that depend on document content.
///
/// Pure selection commands (`AddCursor`, `RemoveCursor`, `ClearSecondary`,
/// `RotatePrimary`) ignore this context. Context-dependent commands
/// (`AddCursorVertical`, `AddCursorsAtMatches`, `SelectAllOccurrences`,
/// `AddNextMatch`) require it to resolve positions within the document.
pub struct MultiCursorContext<'a> {
    /// The full document text.
    pub text: &'a str,
    /// The current search pattern (if any).
    pub search_pattern: Option<&'a str>,
    /// Number of lines in the document.
    pub line_count: usize,
}

/// Execute a multi-cursor command, modifying the engine's `MultiCursorState`.
///
/// Pure selection commands are handled inline and ignore `ctx`.
/// Context-dependent commands use `ctx` to resolve positions within the
/// document text.
///
/// # Errors
///
/// Returns `VimError` for:
/// - Attempting to remove the last remaining cursor
/// - No search pattern when one is required
/// - No word under cursor
/// - Pattern not found in document
pub(crate) fn execute_multi_cursor_command(
    state: &mut VimState,
    cmd: &MultiCursorCommand,
    ctx: &MultiCursorContext<'_>,
) -> Result<Vec<Effect>, VimError> {
    match cmd {
        MultiCursorCommand::AddCursor(offset) => {
            add_cursor(state, *offset);
            Ok(vec![])
        }
        MultiCursorCommand::RemoveCursor(offset) => remove_cursor(state, *offset).map(|()| vec![]),
        MultiCursorCommand::ClearSecondary => {
            clear_secondary(state);
            state.multi_cursor_mut().clear_match_search();
            Ok(vec![])
        }
        MultiCursorCommand::RotatePrimary(direction) => {
            rotate_primary(state, *direction);
            Ok(vec![])
        }
        MultiCursorCommand::AddCursorVertical(direction) => {
            add_cursor_vertical(state, *direction, ctx);
            Ok(vec![])
        }
        MultiCursorCommand::AddCursorsAtMatches => {
            add_cursors_at_matches(state, ctx).map(|()| vec![])
        }
        MultiCursorCommand::SelectAllOccurrences => {
            select_all_occurrences(state, ctx).map(|()| vec![])
        }
        MultiCursorCommand::AddNextMatch { direction, skip } => {
            add_next_match(state, ctx, *direction, *skip).map(|()| vec![])
        }
        MultiCursorCommand::CursorSplit => cursor_split(state, ctx).map(|()| vec![]),

        // ── Selection toolkit: regex-bearing ─────────────────────────
        MultiCursorCommand::SelectOnMatches { pattern } => {
            let regex = compile_pattern(pattern)?;
            let sels = state.multi_cursor().selections();
            let new = refinement::select_on_matches(sels, ctx.text, &regex)
                .ok_or(VimError::PatternNotFound(pattern.clone().into()))?;
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }
        MultiCursorCommand::SplitOnMatches { pattern } => {
            let regex = compile_pattern(pattern)?;
            let sels = state.multi_cursor().selections();
            let new = refinement::split_on_matches(sels, ctx.text, &regex);
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }
        MultiCursorCommand::KeepMatching { pattern } => {
            let regex = compile_pattern(pattern)?;
            let sels = state.multi_cursor().selections();
            let new = refinement::keep_matching(sels, ctx.text, &regex)
                .ok_or(VimError::NoSelectionsRemaining)?;
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }
        MultiCursorCommand::RemoveMatching { pattern } => {
            let regex = compile_pattern(pattern)?;
            let sels = state.multi_cursor().selections();
            let new = refinement::remove_matching(sels, ctx.text, &regex)
                .ok_or(VimError::NoSelectionsRemaining)?;
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }

        // ── Selection toolkit: pure transforms ───────────────────────
        MultiCursorCommand::TrimSelections => {
            let sels = state.multi_cursor().selections();
            let new = refinement::trim_whitespace(sels, ctx.text)
                .ok_or(VimError::NoSelectionsRemaining)?;
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }
        MultiCursorCommand::CollapseSelections => {
            let sels = state.multi_cursor().selections();
            let new = refinement::collapse(sels);
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }
        MultiCursorCommand::FlipSelections => {
            let sels = state.multi_cursor().selections();
            let new = refinement::flip(sels);
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }
        MultiCursorCommand::EnsureForward => {
            let sels = state.multi_cursor().selections();
            let new = refinement::ensure_forward(sels);
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }
        MultiCursorCommand::MergeConsecutive => {
            let sels = state.multi_cursor().selections().clone();
            let new = sels.merge_consecutive();
            state.multi_cursor_mut().set_selections(new);
            Ok(vec![])
        }

        // ── Selection toolkit: text-mutating ─────────────────────────
        MultiCursorCommand::RotateContents(direction) => {
            let sels = state.multi_cursor().selections();
            let effects = manipulation::rotate_contents(sels, ctx.text, *direction, 1);
            Ok(effects)
        }
        MultiCursorCommand::AlignSelections => {
            let sels = state.multi_cursor().selections();
            manipulation::align_selections(sels, ctx.text)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Pure Selection Commands
// ═══════════════════════════════════════════════════════════════════════════

/// Add a cursor at the given byte offset.
///
/// Pushes a new zero-width (insert-mode) cursor into the selection set,
/// then normalizes to merge any overlapping ranges. If the offset already
/// has a cursor, normalization deduplicates it.
fn add_cursor(state: &mut VimState, offset: Offset) {
    let mc = state.multi_cursor_mut();
    let sels = mc.selections_mut();
    sels.push(SelectionRange::insert_cursor(offset));

    // Replace with the normalized form to merge overlapping/duplicate cursors.
    let normalized = mc.selections().clone().normalize();
    mc.set_selections(normalized);
}

/// Remove the cursor nearest to the given byte offset.
///
/// Finds the selection whose head is closest to `offset` and removes it.
/// Returns an error if there is only one cursor remaining (invariant:
/// `Selections` must always contain at least one range).
fn remove_cursor(state: &mut VimState, offset: Offset) -> Result<(), VimError> {
    let mc = state.multi_cursor_mut();
    let sels = mc.selections_mut();

    if sels.len() <= 1 {
        return Err(VimError::InternalError(
            "cannot remove the last cursor".into(),
        ));
    }

    let nearest = sels.nearest_index(offset);
    // `remove` handles primary_index adjustment internally.
    sels.remove(nearest);

    Ok(())
}

/// Keep only the primary cursor, discarding all secondary cursors.
///
/// Replaces the entire selection set with a single zero-width cursor
/// at the primary selection's head position.
fn clear_secondary(state: &mut VimState) {
    let mc = state.multi_cursor_mut();
    let primary_head = mc.selections().primary().head();
    mc.set_selections(Selections::cursor(primary_head));
}

/// Rotate the primary cursor designation forward or backward.
///
/// Wraps around: advancing past the last cursor wraps to index 0;
/// retreating past index 0 wraps to the last cursor.
fn rotate_primary(state: &mut VimState, direction: Direction) {
    let mc = state.multi_cursor_mut();
    let sels = mc.selections_mut();
    let len = sels.len();

    if len <= 1 {
        // Single cursor: rotation is a no-op.
        return;
    }

    let current = sels.primary_index();
    let next = match direction {
        Direction::Forward => {
            if current + 1 >= len {
                0
            } else {
                current + 1
            }
        }
        Direction::Backward => {
            if current == 0 {
                len - 1
            } else {
                current - 1
            }
        }
    };

    sels.set_primary_index(next);
}

// ═══════════════════════════════════════════════════════════════════════════
// Context-Dependent Commands
// ═══════════════════════════════════════════════════════════════════════════

/// Add a cursor on the line above or below the primary cursor.
///
/// Computes the target line from the primary cursor's current line,
/// clamped to `[0, line_count - 1]`. The new cursor is placed at the
/// same column (byte offset within line), clamped to the target line's
/// length. If the target line is the same as the current line (already
/// at the top/bottom boundary), this is a no-op.
fn add_cursor_vertical(state: &mut VimState, direction: Direction, ctx: &MultiCursorContext<'_>) {
    if ctx.line_count == 0 {
        return;
    }

    let selections = state.multi_cursor().selections();
    let offset = match direction {
        Direction::Forward => selections.iter().map(|s| s.head().get()).max().unwrap_or(0),
        Direction::Backward => selections.iter().map(|s| s.head().get()).min().unwrap_or(0),
    };
    let current_line = helpers::line_of(ctx.text, offset);
    let column = helpers::column_of(ctx.text, offset);

    // Compute target line, clamped to valid range.
    let max_line = ctx.line_count.saturating_sub(1);
    let target_line = match direction {
        Direction::Forward => {
            if current_line >= max_line {
                // Already at the last line; no-op.
                return;
            }
            current_line + 1
        }
        Direction::Backward => {
            if current_line == 0 {
                // Already at the first line; no-op.
                return;
            }
            current_line - 1
        }
    };

    // Find the start and end of the target line.
    let target_start = match helpers::line_start(ctx.text, target_line) {
        Some(s) => s,
        None => return, // target line doesn't exist
    };
    let target_end = helpers::line_end(ctx.text, target_line).unwrap_or(ctx.text.len());

    // Clamp column to target line length.
    let target_line_len = target_end.saturating_sub(target_start);
    let clamped_column = column.min(target_line_len);
    let target_offset = Offset::new(target_start + clamped_column);

    // Add the cursor and normalize.
    let mc = state.multi_cursor_mut();
    mc.selections_mut()
        .push(SelectionRange::insert_cursor(target_offset));
    let normalized = mc.selections().clone().normalize();
    mc.set_selections(normalized);
}

/// Add cursors at all matches of the current search pattern.
///
/// The pattern is resolved from `ctx.search_pattern` first, falling back
/// to `state.search().pattern()`. If no pattern is available, returns
/// `VimError::NoPreviousPattern`. Uses literal substring matching via
/// `str::match_indices`.
///
/// Existing cursors are preserved; new cursors are added at each match
/// start offset. After adding, the selection set is normalized to
/// deduplicate any overlaps with existing cursors.
fn add_cursors_at_matches(
    state: &mut VimState,
    ctx: &MultiCursorContext<'_>,
) -> Result<(), VimError> {
    let pattern = resolve_pattern(state, ctx)?;

    let matches: Vec<usize> = ctx
        .text
        .match_indices(pattern.as_str())
        .map(|(idx, _)| idx)
        .collect();

    if matches.is_empty() {
        return Err(VimError::PatternNotFound(pattern.into()));
    }

    // Add a cursor at each match start.
    let mc = state.multi_cursor_mut();
    for &match_start in &matches {
        mc.selections_mut()
            .push(SelectionRange::insert_cursor(Offset::new(match_start)));
    }

    let normalized = mc.selections().clone().normalize();
    mc.set_selections(normalized);

    Ok(())
}

/// Select all occurrences of the word under the primary cursor.
///
/// Finds word boundaries around the primary cursor's head using character
/// classification (alphanumeric + underscore = word character). Then
/// searches the entire document for all occurrences of that word.
///
/// **Replaces** all existing selections with cursors at each occurrence
/// start. The primary is set to the occurrence closest to the original
/// cursor position.
fn select_all_occurrences(
    state: &mut VimState,
    ctx: &MultiCursorContext<'_>,
) -> Result<(), VimError> {
    let primary_head = state.multi_cursor().selections().primary().head();
    let offset = primary_head.get();

    let word = extract_word_under_cursor(ctx.text, offset)?;

    // Find all occurrences.
    let matches: Vec<usize> = ctx.text.match_indices(word).map(|(idx, _)| idx).collect();

    if matches.is_empty() {
        return Err(VimError::PatternNotFound(word.into()));
    }

    // Build new selections: one cursor at each match start.
    let ranges: Vec<SelectionRange> = matches
        .iter()
        .map(|&idx| SelectionRange::insert_cursor(Offset::new(idx)))
        .collect();

    // Find the occurrence closest to the original cursor position for primary.
    let primary_idx = matches
        .iter()
        .enumerate()
        .min_by_key(|(_, &match_start)| offset.abs_diff(match_start))
        .map_or(0, |(i, _)| i);

    let new_sels = Selections::from_vec(ranges, primary_idx).normalize();
    state.multi_cursor_mut().set_selections(new_sels);

    Ok(())
}

/// Find the next/previous match and either add a cursor there or skip it.
///
/// Behavior depends on the `skip` flag:
/// - `skip: false` (gb/gB): keep ALL existing cursors, add a new cursor at the
///   next match position.
/// - `skip: true` (gs): remove the primary cursor, then add a new cursor at the
///   next match position.
///
/// The pattern is resolved in priority order:
/// 1. Locked pattern from `MatchSearchState` (set on a previous gb press)
/// 2. Word under the primary cursor (`whole_word = true`)
/// 3. Search register / `ctx.search_pattern` (`whole_word = false`)
///
/// Supports wrap-around in both directions and skips positions that already
/// have a cursor.
fn add_next_match(
    state: &mut VimState,
    ctx: &MultiCursorContext<'_>,
    direction: Direction,
    skip: bool,
) -> Result<(), VimError> {
    // 1. Resolve pattern with whole-word flag.
    //    Priority: MatchSearchState > word-under-cursor > search register.
    let (pattern, whole_word) = if let Some(ms) = state.multi_cursor().match_search() {
        (ms.pattern.clone().to_string(), ms.whole_word)
    } else {
        let primary_head = state.multi_cursor().selections().primary().head().get();
        if let Ok(word) = extract_word_under_cursor(ctx.text, primary_head) {
            (word.to_owned(), true)
        } else {
            let pat = resolve_pattern(state, ctx)?;
            (pat, false)
        }
    };

    // 2. Determine search start position.
    let search_start = if let Some(ms) = state.multi_cursor().match_search() {
        match direction {
            Direction::Forward => ms.last_match_offset + pattern.len(),
            Direction::Backward => ms.last_match_offset.saturating_sub(1),
        }
    } else {
        let primary_head = state.multi_cursor().selections().primary().head().get();
        match direction {
            Direction::Forward => primary_head + 1,
            Direction::Backward => primary_head.saturating_sub(1),
        }
    };

    // 3. Collect existing cursor positions for overlap check.
    let existing: std::collections::HashSet<usize> = state
        .multi_cursor()
        .selections()
        .iter()
        .map(|s| s.head().get())
        .collect();

    // 4. Find next match (with wrap-around, skipping existing cursors).
    let match_offset = find_next_match_skipping(
        ctx.text,
        &pattern,
        search_start,
        direction,
        &existing,
        whole_word,
    )
    .ok_or_else(|| VimError::PatternNotFound(pattern.clone().into()))?;

    // 5. Add or skip.
    let mc = state.multi_cursor_mut();
    let sels = mc.selections_mut();

    if skip && sels.len() > 1 {
        // gs with multiple cursors: remove primary.
        sels.remove(sels.primary_index());
    }

    sels.push(SelectionRange::insert_cursor(Offset::new(match_offset)));
    let normalized = sels.clone().normalize();

    let mc = state.multi_cursor_mut();
    mc.set_selections(normalized);

    // 6. Update match search state.
    mc.set_match_search(MatchSearchState {
        pattern: pattern.into(),
        last_match_offset: match_offset,
        whole_word,
    });

    Ok(())
}

/// Convert a visual-block selection into individual cursors (one per line).
///
/// Reads `last_visual()` to check the visual type and the `'<'`/`'>'` marks
/// for the selection bounds. For each line spanned by the block, creates a
/// cursor at the block's starting column (clamped to line length).
///
/// This function is invoked via `:cursorsplit` from the command line, which
/// resets mode to Normal before dispatching. Therefore it cannot check the
/// current mode — it must use the saved visual state instead.
fn cursor_split(state: &mut VimState, ctx: &MultiCursorContext<'_>) -> Result<(), VimError> {
    use crate::primitives::{MarkName, VisualType};

    // Only meaningful when last visual was Block mode.
    let last_visual = match state.last_visual() {
        Some(info) if info.visual_type() == VisualType::Block => info,
        _ => return Ok(()),
    };
    let _ = last_visual; // used only for the type check above

    let text = ctx.text;
    if text.is_empty() {
        return Ok(());
    }

    // Read '< and '> marks for the visual selection bounds.
    let start_mark = match state.marks().get(MarkName::VISUAL_START) {
        Some(m) => m,
        None => return Ok(()),
    };
    let end_mark = match state.marks().get(MarkName::VISUAL_END) {
        Some(m) => m,
        None => return Ok(()),
    };

    let anchor_offset = start_mark.offset().get();
    let head_offset = end_mark.offset().get();

    // Determine line range and column range for the block.
    let anchor_line = helpers::line_of(text, anchor_offset);
    let head_line = helpers::line_of(text, head_offset);
    let anchor_col = helpers::column_of(text, anchor_offset);
    let head_col = helpers::column_of(text, head_offset);

    let start_line = anchor_line.min(head_line);
    let end_line = anchor_line.max(head_line);
    let start_col = anchor_col.min(head_col);

    // Create one cursor per line at the block's left column.
    let mut ranges = Vec::new();
    for line in start_line..=end_line {
        let line_start = match helpers::line_start(text, line) {
            Some(s) => s,
            None => continue,
        };
        let line_end = helpers::line_end(text, line).unwrap_or(text.len());
        let line_len = line_end.saturating_sub(line_start);
        let clamped_col = start_col.min(line_len);
        let target_offset = line_start + clamped_col;
        ranges.push(SelectionRange::insert_cursor(Offset::new(target_offset)));
    }

    if ranges.is_empty() {
        return Ok(());
    }

    let new_sels = Selections::from_vec(ranges, 0).normalize();
    state.multi_cursor_mut().set_selections(new_sels);

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Internal Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Compile a regex pattern, mapping errors to `VimError::PatternNotFound`.
fn compile_pattern(pattern: &str) -> Result<VimRegex, VimError> {
    VimRegex::new(pattern).map_err(|e| VimError::PatternNotFound(format!("{e}").into()))
}

/// Resolve the search pattern from context or engine state.
///
/// Prefers `ctx.search_pattern`, falls back to `state.search().pattern()`.
/// Returns an owned `String` to avoid borrow conflicts when the caller
/// needs subsequent mutable access to `state`.
///
/// Returns `VimError::NoPreviousPattern` if neither is available.
fn resolve_pattern(state: &VimState, ctx: &MultiCursorContext<'_>) -> Result<String, VimError> {
    if let Some(pat) = ctx.search_pattern {
        if !pat.is_empty() {
            return Ok(pat.to_owned());
        }
    }
    state
        .search()
        .pattern()
        .map(str::to_owned)
        .ok_or(VimError::NoPreviousPattern)
}

/// Check whether a substring match at `match_start` with length `match_len`
/// in `text` is a whole-word match — i.e. the characters immediately before
/// and after the match are not word characters.
fn is_whole_word_match(text: &str, match_start: usize, match_len: usize) -> bool {
    let word_chars = WordCharSet::default_vim();

    // Check the character immediately before the match.
    if match_start > 0 {
        if let Some(prev_char) = text[..match_start].chars().next_back() {
            if CharClass::classify(prev_char, WordKind::Word, &word_chars) == CharClass::Word {
                return false;
            }
        }
    }

    // Check the character immediately after the match.
    let match_end = match_start + match_len;
    if match_end < text.len() {
        if let Some(next_char) = text[match_end..].chars().next() {
            if CharClass::classify(next_char, WordKind::Word, &word_chars) == CharClass::Word {
                return false;
            }
        }
    }

    true
}

/// Returns `true` if the match at `abs_pos` should be accepted, considering
/// skip-offsets and the optional whole-word constraint.
fn is_acceptable_match(
    text: &str,
    abs_pos: usize,
    pattern_len: usize,
    skip_offsets: &std::collections::HashSet<usize>,
    whole_word: bool,
) -> bool {
    if skip_offsets.contains(&abs_pos) {
        return false;
    }
    if whole_word && !is_whole_word_match(text, abs_pos, pattern_len) {
        return false;
    }
    true
}

/// Find the next match of `pattern` in `text`, searching in `direction` from
/// `start_offset`. Skips positions in `skip_offsets`. When `whole_word` is
/// true, only matches surrounded by non-word characters are accepted.
/// Wraps around to cover the entire document exactly once.
fn find_next_match_skipping(
    text: &str,
    pattern: &str,
    start_offset: usize,
    direction: Direction,
    skip_offsets: &std::collections::HashSet<usize>,
    whole_word: bool,
) -> Option<usize> {
    let pat_len = pattern.len();

    match direction {
        Direction::Forward => {
            // Phase 1: search from start_offset to end of document.
            let clamped = start_offset.min(text.len());
            if let Some(search_slice) = text.get(clamped..) {
                for (pos, _) in search_slice.match_indices(pattern) {
                    let abs_pos = clamped + pos;
                    if is_acceptable_match(text, abs_pos, pat_len, skip_offsets, whole_word) {
                        return Some(abs_pos);
                    }
                }
            }
            // Phase 2: wrap around from start of document to start_offset.
            for (pos, _) in text.match_indices(pattern) {
                if pos >= clamped {
                    break; // already searched this range
                }
                if is_acceptable_match(text, pos, pat_len, skip_offsets, whole_word) {
                    return Some(pos);
                }
            }
            None
        }
        Direction::Backward => {
            // Phase 1: search backward from start_offset to beginning.
            let clamped = start_offset.min(text.len());
            if let Some(search_slice) = text.get(..clamped) {
                // Find all matches in the slice, take the last valid one.
                let mut last_valid = None;
                for (pos, _) in search_slice.match_indices(pattern) {
                    if is_acceptable_match(text, pos, pat_len, skip_offsets, whole_word) {
                        last_valid = Some(pos);
                    }
                }
                if last_valid.is_some() {
                    return last_valid;
                }
            }
            // Phase 2: wrap around from end of document back to start_offset.
            let mut last_valid = None;
            for (pos, _) in text.match_indices(pattern) {
                if pos >= clamped
                    && is_acceptable_match(text, pos, pat_len, skip_offsets, whole_word)
                {
                    last_valid = Some(pos);
                }
            }
            last_valid
        }
    }
}

/// Extract the word under the cursor using Vim's word boundary rules.
///
/// Scans backward from `offset` to find the word start, then forward to
/// find the word end. Word characters are determined by [`CharClass::classify`]
/// with `WordKind::Word` (alphanumeric + underscore).
///
/// Returns the word slice, or `VimError::NoStringUnderCursor` if the cursor
/// is not on a word character.
fn extract_word_under_cursor(text: &str, offset: usize) -> Result<&str, VimError> {
    let clamped = offset.min(text.len().saturating_sub(1));
    let word_chars = WordCharSet::default_vim();

    // Check that the character at cursor is a word character.
    let cursor_char = helpers::char_at(text, clamped).ok_or(VimError::NoStringUnderCursor)?;
    let cursor_class = CharClass::classify(cursor_char, WordKind::Word, &word_chars);
    if cursor_class != CharClass::Word {
        return Err(VimError::NoStringUnderCursor);
    }

    // Scan backward to find word start.
    let mut word_start = clamped;
    while word_start > 0 {
        let prev = helpers::prev_char_boundary(text, word_start);
        match helpers::char_at(text, prev) {
            Some(c) if CharClass::classify(c, WordKind::Word, &word_chars) == CharClass::Word => {
                word_start = prev;
            }
            _ => break,
        }
    }

    // Scan forward to find word end (exclusive).
    let mut word_end = helpers::next_char_boundary(text, clamped);
    while word_end < text.len() {
        match helpers::char_at(text, word_end) {
            Some(c) if CharClass::classify(c, WordKind::Word, &word_chars) == CharClass::Word => {
                word_end = helpers::next_char_boundary(text, word_end);
            }
            _ => break,
        }
    }

    text.get(word_start..word_end)
        .ok_or(VimError::NoStringUnderCursor)
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Offset, SelectionRange, Selections};

    /// Helper: create a `VimState` with the given selections pre-loaded.
    fn state_with_selections(sels: Selections) -> VimState {
        let mut state = VimState::default();
        state.multi_cursor_mut().set_selections(sels);
        state
    }

    /// Helper: create a default `MultiCursorContext` for pure-command tests.
    fn empty_ctx() -> MultiCursorContext<'static> {
        MultiCursorContext {
            text: "",
            search_pattern: None,
            line_count: 0,
        }
    }

    /// Helper: create a `MultiCursorContext` for the given text.
    fn test_ctx(text: &str) -> MultiCursorContext<'_> {
        MultiCursorContext {
            text,
            search_pattern: None,
            line_count: helpers::line_count(text).max(1),
        }
    }

    /// Helper: create a `MultiCursorContext` with a search pattern.
    fn test_ctx_with_pattern<'a>(text: &'a str, pattern: &'a str) -> MultiCursorContext<'a> {
        MultiCursorContext {
            text,
            search_pattern: Some(pattern),
            line_count: helpers::line_count(text).max(1),
        }
    }

    // ── AddCursor ────────────────────────────────────────────────────────

    #[test]
    fn add_cursor_increases_count() {
        let mut state = VimState::default();
        let ctx = empty_ctx();
        assert_eq!(state.multi_cursor().selections().len(), 1);

        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursor(Offset::new(10)),
            &ctx,
        );
        assert!(result.is_ok());
        assert_eq!(state.multi_cursor().selections().len(), 2);
    }

    #[test]
    fn add_cursor_at_existing_position_deduplicates() {
        // Start with a cursor at offset 0 (the default).
        let mut state = VimState::default();
        let ctx = empty_ctx();

        // Add a cursor at the same position.
        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursor(Offset::ZERO),
            &ctx,
        );
        assert!(result.is_ok());
        // After normalize, overlapping zero-width cursors merge into one.
        assert_eq!(state.multi_cursor().selections().len(), 1);
    }

    #[test]
    fn add_cursor_at_distinct_offsets() {
        let mut state = VimState::default();
        let ctx = empty_ctx();

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursor(Offset::new(5)),
            &ctx,
        )
        .expect("add cursor at 5");
        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursor(Offset::new(15)),
            &ctx,
        )
        .expect("add cursor at 15");

        assert_eq!(state.multi_cursor().selections().len(), 3);
    }

    // ── RemoveCursor ─────────────────────────────────────────────────────

    #[test]
    fn remove_cursor_decreases_count() {
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            0,
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RemoveCursor(Offset::new(10)),
            &ctx,
        );
        assert!(result.is_ok());
        assert_eq!(state.multi_cursor().selections().len(), 2);

        // The remaining cursors should be at offsets 0 and 20.
        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads, vec![0, 20]);
    }

    #[test]
    fn remove_cursor_last_returns_error() {
        let mut state = VimState::default();
        let ctx = empty_ctx();
        // Only one cursor — should fail.
        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RemoveCursor(Offset::ZERO),
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn remove_cursor_nearest_match() {
        // Cursors at 0, 10, 20. Remove nearest to offset 12 -> should remove offset 10.
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            0,
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RemoveCursor(Offset::new(12)),
            &ctx,
        )
        .expect("remove cursor near 12");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads, vec![0, 20]);
    }

    #[test]
    fn remove_cursor_adjusts_primary_when_before() {
        // Primary is at index 2 (offset 20). Remove index 0 (offset 0).
        // Primary should shift from index 2 to index 1.
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            2,
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RemoveCursor(Offset::ZERO),
            &ctx,
        )
        .expect("remove cursor at 0");

        assert_eq!(state.multi_cursor().selections().primary_index(), 1);
        assert_eq!(
            state.multi_cursor().selections().primary().head(),
            Offset::new(20)
        );
    }

    #[test]
    fn remove_cursor_primary_itself_shifts() {
        // Primary is at index 1 (offset 10). Remove the primary.
        // After removal, primary should be at index 1 (offset 20) or clamp.
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            1,
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RemoveCursor(Offset::new(10)),
            &ctx,
        )
        .expect("remove cursor at 10");

        // After removing index 1, the remaining is [0, 20] with primary_index
        // clamped to at most len-1. The `remove` method keeps primary_index=1
        // which now points to offset 20.
        let sels = state.multi_cursor().selections();
        assert_eq!(sels.len(), 2);
        assert!(sels.primary_index() < sels.len());
    }

    // ── ClearSecondary ───────────────────────────────────────────────────

    #[test]
    fn clear_secondary_to_single_cursor() {
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            1, // primary at offset 10
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        let result =
            execute_multi_cursor_command(&mut state, &MultiCursorCommand::ClearSecondary, &ctx);
        assert!(result.is_ok());

        let sels = state.multi_cursor().selections();
        assert_eq!(sels.len(), 1);
        assert_eq!(sels.primary().head(), Offset::new(10));
    }

    #[test]
    fn clear_secondary_single_cursor_is_noop() {
        let mut state = VimState::default();
        let ctx = empty_ctx();
        let result =
            execute_multi_cursor_command(&mut state, &MultiCursorCommand::ClearSecondary, &ctx);
        assert!(result.is_ok());
        assert_eq!(state.multi_cursor().selections().len(), 1);
    }

    // ── RotatePrimary ────────────────────────────────────────────────────

    #[test]
    fn rotate_primary_forward() {
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            0,
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RotatePrimary(Direction::Forward),
            &ctx,
        )
        .expect("rotate forward 1");
        assert_eq!(state.multi_cursor().selections().primary_index(), 1);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RotatePrimary(Direction::Forward),
            &ctx,
        )
        .expect("rotate forward 2");
        assert_eq!(state.multi_cursor().selections().primary_index(), 2);
    }

    #[test]
    fn rotate_primary_forward_wraps() {
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            2, // start at last
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RotatePrimary(Direction::Forward),
            &ctx,
        )
        .expect("rotate forward wraps");
        assert_eq!(state.multi_cursor().selections().primary_index(), 0);
    }

    #[test]
    fn rotate_primary_backward() {
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            2,
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RotatePrimary(Direction::Backward),
            &ctx,
        )
        .expect("rotate backward");
        assert_eq!(state.multi_cursor().selections().primary_index(), 1);
    }

    #[test]
    fn rotate_primary_backward_wraps() {
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            0, // start at first
        );
        let mut state = state_with_selections(sels);
        let ctx = empty_ctx();

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RotatePrimary(Direction::Backward),
            &ctx,
        )
        .expect("rotate backward wraps");
        assert_eq!(state.multi_cursor().selections().primary_index(), 2);
    }

    #[test]
    fn rotate_primary_single_cursor_is_noop() {
        let mut state = VimState::default();
        let ctx = empty_ctx();
        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::RotatePrimary(Direction::Forward),
            &ctx,
        )
        .expect("rotate single noop");
        assert_eq!(state.multi_cursor().selections().primary_index(), 0);
    }

    // ── AddCursorVertical ────────────────────────────────────────────────

    #[test]
    fn add_cursor_vertical_down() {
        // 3-line document: "abc\ndef\nghi"
        //                   012 3 456 7 890
        // Cursor at offset 1 (line 0, col 1).
        // Add vertical down -> new cursor on line 1, col 1 -> offset 5.
        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(1));
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursorVertical(Direction::Forward),
            &ctx,
        )
        .expect("add cursor down");

        assert_eq!(state.multi_cursor().selections().len(), 2);
        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert!(heads.contains(&1));
        assert!(heads.contains(&5));
    }

    #[test]
    fn add_cursor_vertical_up() {
        // 3-line document: "abc\ndef\nghi"
        // Cursor at offset 5 (line 1, col 1).
        // Add vertical up -> new cursor on line 0, col 1 -> offset 1.
        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(5));
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursorVertical(Direction::Backward),
            &ctx,
        )
        .expect("add cursor up");

        assert_eq!(state.multi_cursor().selections().len(), 2);
        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert!(heads.contains(&5));
        assert!(heads.contains(&1));
    }

    #[test]
    fn add_cursor_vertical_clamp_at_bottom() {
        // Cursor on last line, add vertical down -> no new cursor.
        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(9)); // line 2, col 1
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursorVertical(Direction::Forward),
            &ctx,
        )
        .expect("clamp at bottom is ok");

        // Should still be 1 cursor (no-op).
        assert_eq!(state.multi_cursor().selections().len(), 1);
    }

    #[test]
    fn add_cursor_vertical_clamp_at_top() {
        // Cursor on first line, add vertical up -> no new cursor.
        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(1)); // line 0, col 1
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursorVertical(Direction::Backward),
            &ctx,
        )
        .expect("clamp at top is ok");

        assert_eq!(state.multi_cursor().selections().len(), 1);
    }

    #[test]
    fn add_cursor_vertical_column_clamp_short_line() {
        // Line 0: "abcde" (5 chars), line 1: "fg" (2 chars).
        // Cursor at offset 4 (line 0, col 4). Add down: target line has
        // only 2 chars, so column clamps to 2 -> offset = 6 + 2 = 8.
        let text = "abcde\nfg\nhij";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(4)); // line 0, col 4
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursorVertical(Direction::Forward),
            &ctx,
        )
        .expect("column clamp on short line");

        assert_eq!(state.multi_cursor().selections().len(), 2);
        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert!(heads.contains(&4)); // original
        assert!(heads.contains(&8)); // clamped to end of "fg"
    }

    #[test]
    fn add_cursor_vertical_consecutive_down() {
        let text = "aaa\nbbb\nccc\nddd\neee";
        let ctx = test_ctx(text);
        let sels = Selections::cursor(Offset::new(0));
        let mut state = state_with_selections(sels);

        add_cursor_vertical(&mut state, Direction::Forward, &ctx);
        assert_eq!(state.multi_cursor().selections().len(), 2);

        add_cursor_vertical(&mut state, Direction::Forward, &ctx);
        assert_eq!(state.multi_cursor().selections().len(), 3);

        add_cursor_vertical(&mut state, Direction::Forward, &ctx);
        assert_eq!(state.multi_cursor().selections().len(), 4);
    }

    #[test]
    fn add_cursor_vertical_consecutive_up() {
        let text = "aaa\nbbb\nccc\nddd\neee";
        let ctx = test_ctx(text);
        let last_line_start = text.rfind('\n').unwrap() + 1;
        let sels = Selections::cursor(Offset::new(last_line_start));
        let mut state = state_with_selections(sels);

        add_cursor_vertical(&mut state, Direction::Backward, &ctx);
        assert_eq!(state.multi_cursor().selections().len(), 2);

        add_cursor_vertical(&mut state, Direction::Backward, &ctx);
        assert_eq!(state.multi_cursor().selections().len(), 3);

        add_cursor_vertical(&mut state, Direction::Backward, &ctx);
        assert_eq!(state.multi_cursor().selections().len(), 4);
    }

    // ── AddCursorsAtMatches ──────────────────────────────────────────────

    #[test]
    fn add_cursors_at_matches() {
        let text = "foo bar foo baz foo";
        let ctx = test_ctx_with_pattern(text, "foo");

        let mut state = VimState::default();
        execute_multi_cursor_command(&mut state, &MultiCursorCommand::AddCursorsAtMatches, &ctx)
            .expect("add cursors at matches");

        // 3 matches: offsets 0, 8, 16.
        // Plus the original cursor at 0, but normalize deduplicates.
        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert!(heads.contains(&0));
        assert!(heads.contains(&8));
        assert!(heads.contains(&16));
    }

    #[test]
    fn add_cursors_at_matches_no_pattern() {
        let text = "foo bar foo baz foo";
        let ctx = test_ctx(text); // no search pattern

        let mut state = VimState::default();
        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursorsAtMatches,
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn add_cursors_at_matches_pattern_not_found() {
        let text = "foo bar foo baz foo";
        let ctx = test_ctx_with_pattern(text, "xyz");

        let mut state = VimState::default();
        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddCursorsAtMatches,
            &ctx,
        );
        assert!(result.is_err());
    }

    // ── SelectAllOccurrences ─────────────────────────────────────────────

    #[test]
    fn select_all_occurrences() {
        // "hello world hello"
        //  01234567890123456
        // Cursor at offset 0 -> word "hello" -> matches at 0 and 12.
        let text = "hello world hello";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(0));
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::SelectAllOccurrences, &ctx)
            .expect("select all occurrences");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&0));
        assert!(heads.contains(&12));
    }

    #[test]
    fn select_all_occurrences_cursor_in_middle_of_word() {
        // Cursor at offset 2 (inside "hello") should still find the word.
        let text = "hello world hello";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(2));
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::SelectAllOccurrences, &ctx)
            .expect("select all from middle");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&0));
        assert!(heads.contains(&12));
    }

    #[test]
    fn select_all_occurrences_on_non_word_returns_error() {
        // Cursor on a space character.
        let text = "hello world";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(5)); // the space
        let mut state = state_with_selections(sels);

        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::SelectAllOccurrences,
            &ctx,
        );
        assert!(result.is_err());
    }

    // ── AddNextMatch ─────────────────────────────────────────────────────

    #[test]
    fn add_next_match_skip() {
        // "foo bar foo baz foo"
        //  0123456789012345678
        // Cursors at 0 and 8 (two "foo" matches). Primary at 0.
        // Skip primary (remove cursor at 0), add next "foo" after 0 -> offset 8.
        // But 8 already exists, so next after 0+1=1 finds 8 (already there).
        // Then next unique: result depends on normalization.
        let text = "foo bar foo baz foo";
        let ctx = test_ctx_with_pattern(text, "foo");

        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(8)),
            ],
            0, // primary at offset 0
        );
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: true,
            },
            &ctx,
        )
        .expect("add next match skip");

        // After skip: the primary (index 0 = offset 0) is removed, the search
        // runs from 0+1=1 and finds "foo" at 8, pushing a cursor at 8. A cursor
        // already existed at 8, so normalization dedupes to a single cursor at
        // 8 — this case loses a cursor. The single-cursor behaviour (keep
        // current + add next) is covered by `add_next_match_single_cursor`.
        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        // The cursor at 0 was removed, cursor at 8 remains, and new match at 8
        // deduplicates. So we end up with just cursor at 8.
        assert!(heads.contains(&8));
    }

    #[test]
    fn add_next_match_single_cursor() {
        // Single cursor: keeps current, adds next match.
        let text = "foo bar foo baz foo";
        let ctx = test_ctx_with_pattern(text, "foo");

        let sels = Selections::cursor(Offset::new(0)); // at first "foo"
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: true,
            },
            &ctx,
        )
        .expect("add next match single cursor");

        // Should now have cursors at 0 (kept) and 8 (next match).
        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&0));
        assert!(heads.contains(&8));
    }

    #[test]
    fn add_next_match_no_pattern_uses_word_under_cursor() {
        // No search pattern: falls back to word under cursor ("foo").
        let text = "foo bar foo";
        let ctx = test_ctx(text); // no search pattern

        let mut state = VimState::default(); // cursor at offset 0
        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        )
        .expect("should use word under cursor");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&0));
        assert!(heads.contains(&8));
    }

    #[test]
    fn add_next_match_no_pattern_on_space_returns_error() {
        // Cursor on a space character: no word under cursor and no search
        // pattern, so should error.
        let text = "foo bar foo";
        let ctx = test_ctx(text); // no search pattern

        let sels = Selections::cursor(Offset::new(3)); // the space
        let mut state = state_with_selections(sels);

        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn add_next_match_skip_wraps_around() {
        // Cursors at 0 and 16, primary at 16. skip=true removes primary (16),
        // searches forward from 17, wraps around, skips 0 (has cursor),
        // finds "foo" at 8.
        let text = "foo bar foo baz foo";
        let ctx = test_ctx_with_pattern(text, "foo");

        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(16)),
            ],
            1, // primary at offset 16
        );
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: true,
            },
            &ctx,
        )
        .expect("skip should wrap around and find match at 8");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&0));
        assert!(heads.contains(&8));
    }

    // ── AddNextMatch: gb (skip=false) ─────────────────────────────────────

    #[test]
    fn add_next_match_gb_basic() {
        // gb: keep all cursors, add next match.
        let text = "foo bar foo baz foo";
        let ctx = test_ctx_with_pattern(text, "foo");
        let sels = Selections::cursor(Offset::new(0));
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        )
        .expect("gb basic");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&0));
        assert!(heads.contains(&8));
    }

    #[test]
    fn add_next_match_gb_second_press() {
        // Second gb: should add THIRD cursor.
        let text = "foo bar foo baz foo";
        let ctx = test_ctx_with_pattern(text, "foo");
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(8)),
            ],
            0,
        );
        let mut state = state_with_selections(sels);

        // Set MatchSearchState to simulate previous gb press.
        state.multi_cursor_mut().set_match_search(MatchSearchState {
            pattern: "foo".into(),
            last_match_offset: 8,
            whole_word: true,
        });

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        )
        .expect("gb second press");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 3);
        assert!(heads.contains(&0));
        assert!(heads.contains(&8));
        assert!(heads.contains(&16));
    }

    #[test]
    fn add_next_match_gb_wrap_around() {
        // gb wraps around from end to start.
        let text = "foo bar foo";
        let ctx = test_ctx_with_pattern(text, "foo");
        let sels = Selections::cursor(Offset::new(8)); // at second "foo"
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        )
        .expect("gb wrap around");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&0)); // wrapped to first "foo"
        assert!(heads.contains(&8));
    }

    #[test]
    fn add_next_match_gb_all_exhausted() {
        // All matches have cursors — error.
        let text = "foo bar foo";
        let ctx = test_ctx_with_pattern(text, "foo");
        let sels = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(8)),
            ],
            0,
        );
        let mut state = state_with_selections(sels);

        // Set match state so search advances past both.
        state.multi_cursor_mut().set_match_search(MatchSearchState {
            pattern: "foo".into(),
            last_match_offset: 8,
            whole_word: true,
        });

        let result = execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        );
        assert!(result.is_err()); // all matches have cursors
    }

    #[test]
    fn add_next_match_gb_backward() {
        // gB: search backward.
        let text = "foo bar foo baz foo";
        let ctx = test_ctx_with_pattern(text, "foo");
        let sels = Selections::cursor(Offset::new(16)); // at last "foo"
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Backward,
                skip: false,
            },
            &ctx,
        )
        .expect("gB backward");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&8)); // found second "foo" searching backward
        assert!(heads.contains(&16));
    }

    #[test]
    fn add_next_match_word_under_cursor() {
        // No search pattern — use word under cursor.
        let text = "foo bar foo";
        let ctx = test_ctx(text); // no search_pattern
        let sels = Selections::cursor(Offset::new(0)); // on "foo"
        let mut state = state_with_selections(sels);

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        )
        .expect("word under cursor");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 2);
        assert!(heads.contains(&0));
        assert!(heads.contains(&8));
    }

    #[test]
    fn add_next_match_sets_match_search_state() {
        // After gb, MatchSearchState should be set.
        let text = "foo bar foo";
        let ctx = test_ctx_with_pattern(text, "foo");
        let sels = Selections::cursor(Offset::new(0));
        let mut state = state_with_selections(sels);

        assert!(state.multi_cursor().match_search().is_none());

        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        )
        .expect("gb sets state");

        let ms = state.multi_cursor().match_search().expect("should be set");
        assert_eq!(ms.pattern.as_str(), "foo");
        assert_eq!(ms.last_match_offset, 8);
    }

    #[test]
    fn clear_secondary_clears_match_search() {
        // Escape (ClearSecondary) should clear MatchSearchState.
        let text = "foo bar foo";
        let ctx = test_ctx_with_pattern(text, "foo");
        let sels = Selections::cursor(Offset::new(0));
        let mut state = state_with_selections(sels);

        // Do a gb to set the state.
        execute_multi_cursor_command(
            &mut state,
            &MultiCursorCommand::AddNextMatch {
                direction: Direction::Forward,
                skip: false,
            },
            &ctx,
        )
        .expect("gb");
        assert!(state.multi_cursor().match_search().is_some());

        // Now clear.
        execute_multi_cursor_command(&mut state, &MultiCursorCommand::ClearSecondary, &ctx)
            .expect("clear secondary");
        assert!(state.multi_cursor().match_search().is_none());
    }

    // ── extract_word_under_cursor ────────────────────────────────────────

    #[test]
    fn extract_word_basic() {
        let word = extract_word_under_cursor("hello world", 2).expect("should find word");
        assert_eq!(word, "hello");
    }

    #[test]
    fn extract_word_at_start() {
        let word = extract_word_under_cursor("hello world", 0).expect("should find word");
        assert_eq!(word, "hello");
    }

    #[test]
    fn extract_word_second_word() {
        let word = extract_word_under_cursor("hello world", 7).expect("should find word");
        assert_eq!(word, "world");
    }

    #[test]
    fn extract_word_with_underscore() {
        let word = extract_word_under_cursor("foo_bar baz", 3).expect("should find word");
        assert_eq!(word, "foo_bar");
    }

    #[test]
    fn extract_word_on_space_fails() {
        let result = extract_word_under_cursor("hello world", 5);
        assert!(result.is_err());
    }

    #[test]
    fn extract_word_on_punctuation_fails() {
        let result = extract_word_under_cursor("foo.bar", 3);
        assert!(result.is_err());
    }

    // ── CursorSplit ─────────────────────────────────────────────────

    #[test]
    fn cursor_split_creates_cursors_from_visual_block() {
        // 3-line document: "abc\ndef\nghi"
        //                   012 3 456 7 890
        // Visual block: anchor at offset 0 (line 0, col 0),
        //               head at offset 8 (line 2, col 0).
        // Should create 3 cursors: at offsets 0, 4, 8 (col 0 on each line).
        use crate::primitives::{LastVisualInfo, Mark, MarkName};

        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(8)));
        let mut state = state_with_selections(sels);
        // Simulate having just exited Visual Block mode.
        state.set_last_visual(LastVisualInfo::block_wise(3, 0));
        state
            .marks_mut()
            .set(MarkName::VISUAL_START, Mark::new(Offset::new(0)));
        state
            .marks_mut()
            .set(MarkName::VISUAL_END, Mark::new(Offset::new(8)));

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::CursorSplit, &ctx)
            .expect("cursor_split should succeed");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 3, "should have 3 cursors, got: {heads:?}");
        assert!(heads.contains(&0));
        assert!(heads.contains(&4));
        assert!(heads.contains(&8));
    }

    #[test]
    fn cursor_split_with_nonzero_column() {
        // 3-line document: "abc\ndef\nghi"
        // Visual block from offset 1 (line 0, col 1) to offset 9 (line 2, col 1).
        // Block column range: min col = 1. Should place cursors at col 1 on each line.
        // Line 0: col 1 -> offset 1
        // Line 1: col 1 -> offset 5
        // Line 2: col 1 -> offset 9
        use crate::primitives::{LastVisualInfo, Mark, MarkName};

        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::single(SelectionRange::new(Offset::new(1), Offset::new(9)));
        let mut state = state_with_selections(sels);
        state.set_last_visual(LastVisualInfo::block_wise(3, 1));
        state
            .marks_mut()
            .set(MarkName::VISUAL_START, Mark::new(Offset::new(1)));
        state
            .marks_mut()
            .set(MarkName::VISUAL_END, Mark::new(Offset::new(9)));

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::CursorSplit, &ctx)
            .expect("cursor_split with nonzero column");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 3, "should have 3 cursors, got: {heads:?}");
        assert!(heads.contains(&1));
        assert!(heads.contains(&5));
        assert!(heads.contains(&9));
    }

    #[test]
    fn cursor_split_clamps_column_on_short_line() {
        // Line 0: "abcde" (5 chars), Line 1: "fg" (2 chars), Line 2: "hij" (3 chars).
        // anchor at offset 4 (line 0, col 4), head at offset 12 (line 2, col 3).
        // start_col = min(4, 3) = 3.
        // Line 0: col 3 -> offset 3
        // Line 1: "fg" has len 2, col clamped to 2 -> offset 6+2=8
        // Line 2: col 3 -> offset 9+3=12
        use crate::primitives::{LastVisualInfo, Mark, MarkName};

        let text = "abcde\nfg\nhij";
        let ctx = test_ctx(text);

        let sels = Selections::single(SelectionRange::new(Offset::new(4), Offset::new(12)));
        let mut state = state_with_selections(sels);
        state.set_last_visual(LastVisualInfo::block_wise(3, 2));
        state
            .marks_mut()
            .set(MarkName::VISUAL_START, Mark::new(Offset::new(4)));
        state
            .marks_mut()
            .set(MarkName::VISUAL_END, Mark::new(Offset::new(12)));

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::CursorSplit, &ctx)
            .expect("cursor_split clamps column");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 3, "should have 3 cursors, got: {heads:?}");
        // col 4 (anchor) and col 3 (head) → start_col = 3
        // Line 0: offset 3
        assert!(heads.contains(&3), "line 0 col 3, heads: {heads:?}");
        // Line 1: "fg" len=2, clamped to 2, offset = 6+2 = 8
        assert!(heads.contains(&8), "line 1 clamped, heads: {heads:?}");
        // Line 2: col 3, offset = 9+3 = 12
        assert!(heads.contains(&12), "line 2 col 3, heads: {heads:?}");
    }

    #[test]
    fn cursor_split_noop_when_last_visual_is_none() {
        // cursor_split should be a no-op when last_visual is None.
        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::cursor(Offset::new(0));
        let mut state = state_with_selections(sels);
        // last_visual is None by default.

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::CursorSplit, &ctx)
            .expect("cursor_split no-op when last_visual is None");

        assert_eq!(state.multi_cursor().selections().len(), 1);
    }

    #[test]
    fn cursor_split_noop_when_last_visual_is_char() {
        // cursor_split should be a no-op when last visual was Char mode.
        use crate::primitives::LastVisualInfo;

        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(8)));
        let mut state = state_with_selections(sels);
        state.set_last_visual(LastVisualInfo::char_wise(3));

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::CursorSplit, &ctx)
            .expect("cursor_split no-op when last_visual is Char");

        // Selection should be unchanged (still 1 range).
        assert_eq!(state.multi_cursor().selections().len(), 1);
    }

    #[test]
    fn cursor_split_single_line_block() {
        // Block on a single line: anchor and head on the same line.
        // Should create exactly 1 cursor.
        use crate::primitives::{LastVisualInfo, Mark, MarkName};

        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::single(SelectionRange::new(Offset::new(1), Offset::new(2)));
        let mut state = state_with_selections(sels);
        state.set_last_visual(LastVisualInfo::block_wise(1, 1));
        state
            .marks_mut()
            .set(MarkName::VISUAL_START, Mark::new(Offset::new(1)));
        state
            .marks_mut()
            .set(MarkName::VISUAL_END, Mark::new(Offset::new(2)));

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::CursorSplit, &ctx)
            .expect("single line block");

        assert_eq!(
            state.multi_cursor().selections().len(),
            1,
            "single line block should produce 1 cursor"
        );
    }

    #[test]
    fn cursor_split_reversed_anchor_head() {
        // Visual block where head is BEFORE anchor (selection drawn upward).
        // '< at offset 0 (low end), '> at offset 8 (high end).
        // Marks are always ordered: '< <= '>.
        use crate::primitives::{LastVisualInfo, Mark, MarkName};

        let text = "abc\ndef\nghi";
        let ctx = test_ctx(text);

        let sels = Selections::single(SelectionRange::new(Offset::new(8), Offset::new(0)));
        let mut state = state_with_selections(sels);
        state.set_last_visual(LastVisualInfo::block_wise(3, 0));
        state
            .marks_mut()
            .set(MarkName::VISUAL_START, Mark::new(Offset::new(0)));
        state
            .marks_mut()
            .set(MarkName::VISUAL_END, Mark::new(Offset::new(8)));

        execute_multi_cursor_command(&mut state, &MultiCursorCommand::CursorSplit, &ctx)
            .expect("reversed anchor/head");

        let heads: Vec<usize> = state
            .multi_cursor()
            .selections()
            .iter()
            .map(|r| r.head().get())
            .collect();
        assert_eq!(heads.len(), 3, "should still have 3 cursors");
        assert!(heads.contains(&0));
        assert!(heads.contains(&4));
        assert!(heads.contains(&8));
    }
}
