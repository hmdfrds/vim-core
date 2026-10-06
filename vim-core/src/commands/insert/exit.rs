//! Insert mode exit computations.
//!
//! Pure functions extracted from `execution/insert_handler.rs`.
//! No state mutations, no Effects — just context → computed values.

use super::types::{InsertExitContext, InsertExitParams};
use crate::primitives::Offset;
use compact_str::CompactString;
use unicode_segmentation::UnicodeSegmentation;

/// Compute the exit cursor position after leaving insert mode.
///
/// Pure: `(&InsertExitContext) -> cursor_offset`.
///
/// Handles:
/// - Grapheme-aware cursor-back (move left one grapheme on exit)
/// - Newline boundary clamping
/// - Block insert cursor return override
pub fn compute_exit_cursor(ctx: &InsertExitContext<'_>) -> usize {
    let insert_offset = ctx.insert_offset.get();
    let last_char_is_newline = ctx.accumulated_text.ends_with('\n');

    let grapheme_len = if last_char_is_newline || ctx.is_open_line_no_text {
        0 // Don't move back past newline
    } else if !ctx.accumulated_text.is_empty() {
        ctx.accumulated_text
            .graphemes(true)
            .next_back()
            .map_or(1, str::len)
    } else {
        // Use grapheme length from document text for multibyte chars
        let before_cursor = &ctx.text[..insert_offset.min(ctx.text.len())];
        before_cursor
            .graphemes(true)
            .next_back()
            .map_or(1, str::len)
    };

    // Clamp cursor-back to line start
    let line_start = crate::commands::helpers::line_start_for_offset(ctx.text, insert_offset);
    let new_cursor = insert_offset.saturating_sub(grapheme_len).max(line_start);

    // Clamp to valid normal-mode position, but only when insert_offset
    // is within the original text. Count-repeat (e.g. 3iword<Esc>) extends
    // insert_offset beyond ctx.text via queued Insert effects; clamping
    // against the pre-repeat text length would be wrong in that case.
    let new_cursor = if insert_offset <= ctx.text.len() && !ctx.text.is_empty() {
        let max_cursor = if ctx.text.ends_with('\n') {
            ctx.text.len()
        } else {
            // Last char may be multi-byte; walk back to its start.
            crate::primitives::text_util::prev_char_boundary(ctx.text, ctx.text.len())
        };
        new_cursor.min(max_cursor)
    } else {
        new_cursor
    };

    // For block insert/append, override cursor to original block's top-left corner
    if let Some(block_ctx) = ctx.block_insert {
        block_ctx.cursor_return_offset().get()
    } else {
        new_cursor
    }
}

/// Compute the repeat text for counted insert (e.g., `3iX<Esc>` produces "XXX").
///
/// Pure: `(accumulated_text, entry_type) -> repeat_text`.
/// For o/O entry, prepends a newline to each repetition.
#[must_use]
pub fn compute_repeat_text(
    accumulated_text: &CompactString,
    entry_type: crate::primitives::InsertEntryType,
) -> CompactString {
    match entry_type {
        crate::primitives::InsertEntryType::NewLineBelow
        | crate::primitives::InsertEntryType::NewLineAbove => {
            CompactString::from(format!("\n{accumulated_text}"))
        }
        _ => accumulated_text.clone(),
    }
}

/// Compute byte offsets for block visual insert replication.
///
/// Pure: `(&InsertExitContext) -> Vec<(offset, text)>`.
///
/// Computes where to insert the accumulated text on each secondary line
/// (below the primary insert line). Returns offsets in reverse order so
/// callers can insert bottom-to-top without invalidating byte offsets.
#[must_use]
pub fn compute_block_insert_offsets(ctx: &InsertExitContext<'_>) -> Vec<(usize, CompactString)> {
    use crate::commands::helpers::{line_end, line_of, line_start};

    let Some(block_ctx) = ctx.block_insert else {
        return Vec::new();
    };

    let insert_offset = ctx.insert_offset.get();
    let accumulated = CompactString::from(ctx.accumulated_text);
    if accumulated.is_empty() || block_ctx.lines_below() == 0 {
        return Vec::new();
    }

    let primary_line = line_of(ctx.text, insert_offset.min(ctx.text.len()));
    let mut offsets = Vec::with_capacity(block_ctx.lines_below());

    // Iterate from bottom to top so byte offsets stay valid
    for line_idx in (primary_line + 1..=primary_line + block_ctx.lines_below()).rev() {
        if let (Some(ls), Some(le)) = (line_start(ctx.text, line_idx), line_end(ctx.text, line_idx))
        {
            let line_text = &ctx.text[ls..le];
            let graphemes: Vec<&str> = line_text.graphemes(true).collect();
            let total_graphemes = graphemes.len();
            let col = block_ctx.grapheme_col().min(total_graphemes);
            let byte_offset: usize = graphemes
                .get(..col)
                .unwrap_or(&[])
                .iter()
                .map(|g| g.len())
                .sum::<usize>()
                + ls;
            offsets.push((byte_offset, accumulated.clone()));
        }
    }

    offsets
}

/// Combines the domain calculations into a single sequence of `Effects`.
///
/// This encapsulates the entire sequence of count-repeats, auto-indent stripping,
/// block replication, and cursor formatting. The Orchestrator simply
/// requests these effects securely and applies them sequentially.
/// Returns `(effects, final_insert_offset)` where `final_insert_offset` is
/// the cursor position before the exit backup — matching Neovim's `end_insert_pos`.
pub fn build_insert_exit_effects(
    params: &InsertExitParams<'_>,
) -> (crate::effects::Effects, usize) {
    let mut all_effects = crate::effects::Effects::new();
    let mut insert_offset = params.cursor.get();
    // The buffer once the formatted repeats are in, when formatting broke
    // their lines: the exit cursor is found in it.
    let mut formatted_text: Option<String> = None;

    // 1. Repeat accumulated text count-1 more times
    if params.count > 1 {
        let repeat_text = compute_repeat_text(
            &CompactString::from(params.accumulated_text),
            params.entry_type,
        );
        if !repeat_text.is_empty() {
            let is_replace = params.entry_type == crate::primitives::InsertEntryType::ReplaceMode;
            let (mut repeat_fx, mut new_offset) = super::effects::repeat_text(
                insert_offset,
                &repeat_text,
                params.count,
                is_replace,
                params.text,
            );
            // Vim types the repeats, so they break lines like the first
            // round did. In Replace mode only the text past the end of the
            // line formats.
            if let Some(policy) = params.format {
                let overwritten = super::wrap::overwritten_chars(
                    repeat_fx.as_slice(),
                    params.text,
                    insert_offset,
                );
                if let Some(cursor) = super::wrap::format_inserted_text(
                    &mut repeat_fx,
                    params.text,
                    &[],
                    insert_offset..new_offset,
                    policy,
                    params.insert_start,
                    overwritten,
                ) {
                    new_offset = cursor;
                    formatted_text =
                        super::wrap::apply_text_effects(params.text, repeat_fx.as_slice());
                }
            }
            all_effects.extend(repeat_fx);
            // For counted inserts (3iX<Esc>), mark '.' should point to
            // the last character of the repeated text. sync_change_marks
            // only records the first edit's start, so we override here.
            if new_offset > 0 {
                all_effects = all_effects.set_mark(
                    crate::primitives::MarkName::LAST_CHANGE,
                    Offset::new(new_offset.saturating_sub(1)),
                    None,
                );
            }
            insert_offset = new_offset;
        }
    }

    // 2. Strip auto-indentation for o/O with no text typed
    let is_open_line_no_text = params.accumulated_text.is_empty()
        && matches!(
            params.entry_type,
            crate::primitives::InsertEntryType::NewLineBelow
                | crate::primitives::InsertEntryType::NewLineAbove
        );

    if is_open_line_no_text && params.auto_indent_len > 0 {
        let before_cursor = &params.text[..insert_offset.min(params.text.len())];
        let actual_ws = before_cursor
            .bytes()
            .rev()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        let actual_strip = actual_ws.min(params.auto_indent_len);
        if actual_strip > 0 {
            let (strip_fx, new_offset) = super::effects::strip_indent(insert_offset, actual_strip);
            all_effects.extend(strip_fx);
            insert_offset = new_offset;
        }
    }

    // Build shared context for exit computations
    let exit_ctx = InsertExitContext {
        text: params.text,
        accumulated_text: params.accumulated_text,
        is_open_line_no_text,
        insert_offset: Offset::new(insert_offset),
        block_insert: params.block_insert,
    };

    // 3. Replicate text for block visual insert
    let offsets = compute_block_insert_offsets(&exit_ctx);
    // Save top_offset before offsets is consumed by block_replicate.
    // Used in step 6 for mark '.' correction.
    let block_top_offset = offsets.last().map(|(off, _)| *off);
    // offsets[0] = bottommost (highest byte offset); offsets[N-1] = topmost.
    // `Some` exactly when `offsets` is non-empty, i.e. when this is a block
    // visual insert.
    let block_bottom_offset = offsets.first().map(|(off, _)| *off);
    if let Some(bottom_offset) = block_bottom_offset {
        let text_len = params.accumulated_text.len();
        let n_inserts = offsets.len();

        all_effects.extend(super::effects::block_replicate(offsets));

        // ── Mark ']' correction for block insert ───────────────────
        //
        // Block change (c) vs block insert/append (I/A) differ:
        //
        // - Block change: Neovim sets mark `]` to the end of the
        //   **primary** line's change (where the user typed).
        // - Block insert/append: Neovim sets mark `]` to the end of
        //   the **bottommost** insertion in post-all-edits coordinates.
        let is_block_change =
            params.entry_type == crate::primitives::InsertEntryType::ChangeOperator;
        if is_block_change {
            all_effects = all_effects.set_mark(
                crate::primitives::MarkName::CHANGE_END,
                Offset::new(insert_offset),
                None,
            );
        } else {
            // Block insert/append: bottom_offset + N * text_len
            let final_bracket_end = bottom_offset + n_inserts * text_len;
            all_effects = all_effects.set_mark(
                crate::primitives::MarkName::CHANGE_END,
                Offset::new(final_bracket_end),
                None,
            );
        }

        // Mark '.' correction is deferred to after exit_finalize (step 5)
        // because exit_finalize also sets LAST_CHANGE and would override us.
    }

    // 4. Compute exit cursor position
    let final_cursor = match formatted_text.as_deref() {
        Some(text) => compute_exit_cursor(&InsertExitContext { text, ..exit_ctx }),
        None => compute_exit_cursor(&exit_ctx),
    };

    // 5. Finalize exit (delegated to commands)
    let is_replace = params.entry_type == crate::primitives::InsertEntryType::ReplaceMode;
    all_effects.extend(super::effects::exit_finalize(
        insert_offset,
        final_cursor,
        params.text,
        params.accumulated_text,
        is_replace,
        params.mark_dot_override_pos,
    ));

    // 6. Mark '.' correction for block insert (MUST come after exit_finalize
    //    which also sets LAST_CHANGE based on accumulated text).
    //
    // Neovim calls `changed_lines(start.lnum + 1, 0, ...)` after block
    // replication, setting mark '.' to the LINE START of the first
    // secondary line (col 0). This overrides the per-character mark '.'
    // that exit_finalize computes from the primary insert.
    if let Some(block_top_offset) = block_top_offset {
        use crate::commands::helpers::line_start_for_offset;
        let top_line_start = line_start_for_offset(params.text, block_top_offset);
        all_effects = all_effects.set_mark(
            crate::primitives::MarkName::LAST_CHANGE,
            Offset::new(top_line_start),
            None,
        );
    }

    (all_effects, insert_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_cursor_basic() {
        let ctx = InsertExitContext {
            text: "hello",
            accumulated_text: "lo",
            is_open_line_no_text: false,
            insert_offset: Offset::new(5),
            block_insert: None,
        };
        let result = compute_exit_cursor(&ctx);
        assert_eq!(result, 4);
    }

    #[test]
    fn test_exit_cursor_at_line_start() {
        let ctx = InsertExitContext {
            text: "ab\ncd",
            accumulated_text: "",
            is_open_line_no_text: false,
            insert_offset: Offset::new(3),
            block_insert: None,
        };
        let result = compute_exit_cursor(&ctx);
        assert_eq!(result, 3);
    }

    #[test]
    fn test_exit_cursor_after_newline_insert() {
        let ctx = InsertExitContext {
            text: "hello\n",
            accumulated_text: "\n",
            is_open_line_no_text: false,
            insert_offset: Offset::new(6),
            block_insert: None,
        };
        let result = compute_exit_cursor(&ctx);
        assert_eq!(result, 6);
    }

    #[test]
    fn test_repeat_text_for_o() {
        let text = CompactString::from("abc");
        let result = compute_repeat_text(&text, crate::primitives::InsertEntryType::NewLineBelow);
        assert_eq!(result.as_str(), "\nabc");
    }

    #[test]
    fn test_repeat_text_for_normal() {
        let text = CompactString::from("abc");
        let result = compute_repeat_text(&text, crate::primitives::InsertEntryType::BeforeCursor);
        assert_eq!(result.as_str(), "abc");
    }
}
