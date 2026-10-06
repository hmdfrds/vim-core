//! Insert mode exit orchestration — extracted from `engine.rs`.
//!
//! Contains `handle_insert_exit` which orchestrates:
//! - Count repeat (e.g., `3iX<Esc>`)
//! - Auto-indent stripping for o/O with no text
//! - Block visual insert replication
//! - Exit cursor computation
//!
//! Note: All other insert-mode handlers have been unified into the
//! standard pipeline (dispatch_insert). This file only retains
//! handle_insert_exit because its orchestration complexity (multiple
//! sequential effect builders that depend on intermediate state)
//! doesn't map cleanly to the single-command dispatch model.

use crate::dispatch::{build_insert_exit_effects, InsertExitParams};
use crate::execution::response::Response;
use crate::state::VimState;
use compact_str::CompactString;

/// Information about the insert session needed for post-exit mark correction.
pub(crate) struct InsertExitInfo {
    /// Whether text was actually typed (accumulated_text is non-empty).
    pub had_accumulated_text: bool,
    /// Whether the insert session was a block visual insert/append.
    pub is_block_insert: bool,
    /// The cursor position at exit (before backup) — Neovim's end_insert_pos.
    pub end_insert_pos: usize,
}

/// Handle InsertExit command (Escape/Ctrl-[/Ctrl-C).
///
/// If insert was entered with a count (e.g., 3iX), repeats accumulated
/// text count-1 times.
///
/// Returns `(Response, InsertExitInfo)` — the caller uses `InsertExitInfo`
/// to correct marks `[` and `]` AFTER effect processing (matching Neovim's
/// `stop_insert()` which sets `b_op_start = Insstart; b_op_end = *end_insert_pos`
/// after all text mutations are finalized).
pub(crate) fn handle_insert_exit(
    state: &mut VimState,
    cursor: usize,
    text: &str,
    format: Option<&crate::commands::insert::wrap::FormatPolicy<'_>>,
) -> (Response, InsertExitInfo) {
    use crate::primitives::InsertEntryType;

    // Get insert session data
    let (
        count,
        accumulated_text,
        entry_type,
        auto_indent_len,
        block_insert,
        entry_offset,
        had_text_mutation,
        mark_dot_override_pos,
        min_change_start,
        arrow_used,
        newline_indent_lens,
        insert_start,
    ) = if let Some(insert_state) = state.insert_state() {
        (
            insert_state.count().get(),
            CompactString::from(insert_state.accumulated_text()),
            insert_state.entry_type(),
            insert_state.auto_indent_len(),
            insert_state.block_insert().cloned(),
            insert_state.entry_offset(),
            insert_state.had_text_mutation(),
            insert_state.mark_dot_override_pos(),
            insert_state.min_change_start(),
            insert_state.arrow_used(),
            insert_state.newline_indent_lens().to_vec(),
            insert_state.insert_start(),
        )
    } else {
        (
            1,
            CompactString::default(),
            InsertEntryType::BeforeCursor,
            0,
            None,
            None,
            false,
            None,
            None,
            false,
            Vec::new(),
            None,
        )
    };

    let (all_effects, final_insert_offset) = build_insert_exit_effects(&InsertExitParams {
        text,
        accumulated_text: &accumulated_text,
        cursor: crate::primitives::Offset::new(cursor),
        count,
        entry_type,
        auto_indent_len,
        block_insert: block_insert.as_ref(),
        mark_dot_override_pos,
        entry_offset,
        format,
        insert_start,
    });

    state.store_last_inserted_text(&accumulated_text);
    state.store_last_insert_entry_type(entry_type);
    state.store_last_insert_indent_lens(&newline_indent_lens);
    // INSERT_STOP mark (^) is set via Effect in exit_finalize — no direct mutation needed.

    // Changelist is now maintained in real-time during insert mode by
    // handle_text_mutation → changelist_push (mirroring Neovim's changed_common).
    // No explicit push needed at insert exit.

    state.take_insert();

    if accumulated_text.is_empty() {
        // Neovim's ins_esc: when arrow_used is true AND the C-o command
        // mutated text, stop_insert is NOT called and marks `[`/`]` from
        // the one-shot command are preserved.
        //
        // When arrow_used is true but no text was mutated (e.g., i<C-o><Esc>),
        // marks must still be set: Neovim's start_arrow_common called
        // stop_insert at the C-o moment, setting b_op_start=b_op_end=Insstart.
        let skip_mark_override = arrow_used && had_text_mutation;
        if !skip_mark_override {
            let mark_start = entry_offset.unwrap_or_else(|| crate::primitives::Offset::new(cursor));
            if had_text_mutation {
                // Deletion-only session (BS/C-w/C-u/C-d with no typed text):
                // `[` = entry_offset, `]` = current cursor position.
                //
                // Exception: C-d/C-t (indent/outdent) track their edit start
                // via min_change_start. When set and before entry_offset,
                // use it instead so mark `[` brackets the actual indent edit.
                let final_mark_start = match min_change_start {
                    Some(mcs) if mcs < mark_start => mcs,
                    _ => mark_start,
                };
                let mark_end = crate::primitives::Offset::new(cursor);
                state.marks_mut().set(
                    crate::primitives::MarkName::CHANGE_START,
                    crate::primitives::Mark::new(final_mark_start),
                );
                state.marks_mut().set(
                    crate::primitives::MarkName::CHANGE_END,
                    crate::primitives::Mark::new(mark_end),
                );
            } else {
                // Arrow-only session (no text mutations at all):
                // Both `[` and `]` = entry_offset. Arrow keys move the cursor
                // but don't affect change marks.
                state.marks_mut().set(
                    crate::primitives::MarkName::CHANGE_START,
                    crate::primitives::Mark::new(mark_start),
                );
                state.marks_mut().set(
                    crate::primitives::MarkName::CHANGE_END,
                    crate::primitives::Mark::new(mark_start),
                );
            }
        }
    }

    let info = InsertExitInfo {
        had_accumulated_text: !accumulated_text.is_empty(),
        is_block_insert: block_insert.is_some(),
        end_insert_pos: final_insert_offset,
    };

    (Response::with_effects(all_effects), info)
}
