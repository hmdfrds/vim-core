//! Insert mode command dispatcher.
//!
//! Maps insert mode Commands to `commands::insert` implementations.
//! This is the ONLY place to update when adding insert commands.
//!
//! # Design
//!
//! Every insert-mode-specific command flows through this dispatcher.
//! Normal-mode entry commands (`InsertEntry`) are handled directly by the
//! executor → `entry::execute()`, NOT through this dispatcher.
//! The engine populates `InsertContext.precomputed` with all derived data
//! so this layer stays state-free — zero VimState access, zero recomputation.
//!
//! # Adding New Insert Commands
//!
//! 1. Add variant to `grammar::Command`
//! 2. Create implementation in `commands/insert/`
//! 3. Add match arm HERE in `dispatch_insert()`

use crate::commands::insert::auto_pairs;
use crate::commands::insert::delete;
use crate::commands::insert::effects as insert_effects;
use crate::commands::insert::entry;
use crate::commands::insert::indent as insert_indent;
pub use crate::commands::insert::InsertContext;
pub use crate::commands::insert::InsertPrecomputed;
pub use crate::commands::insert::ReplaceRestoreAction;
pub use crate::commands::insert::{
    build_insert_exit_effects, compute_newline_insert, compute_tab_spaces, enter_insert_at,
    InsertExitParams,
};
use crate::commands::CommandResult;
use crate::grammar::{Command, InsertKind};
use crate::primitives::InsertEntryType;
use compact_str::CompactString;

/// Dispatch an `InsertEntry` command to `entry::execute`.
///
/// This is the dispatch-layer path for normal-mode insert entry commands
/// (i, a, I, A, o, O, s, S). The executor calls this, NOT `dispatch_insert`.
#[inline]
#[expect(
    clippy::too_many_arguments,
    reason = "thin pass-through to insert::entry::execute which has the same arity for the same architectural reason; bundling here would force a struct shuffle on both ends"
)]
pub fn dispatch_insert_entry(
    text: &str,
    cursor: usize,
    entry_type: InsertEntryType,
    count: u32,
    register: Option<crate::primitives::RegisterName>,
    tabstop: usize,
    autoindent: bool,
    indent_provider: Option<&dyn crate::document::IndentProvider>,
) -> CommandResult {
    entry::execute(
        text,
        cursor,
        entry_type,
        count,
        register,
        tabstop,
        autoindent,
        indent_provider,
    )
}

/// Dispatch an insert mode Command to the appropriate implementation.
///
/// Uses `InsertContext.precomputed` for all derived data — zero recomputation.
/// The engine computes once, this function reads the result.
///
/// No dyn traits in the hot path: exhaustive match dispatch, which is
/// inlinable and allocation-free.
#[inline]
pub fn dispatch_insert(cmd: &Command, ctx: &InsertContext<'_>) -> CommandResult {
    match cmd {
        // ── Character insertion — reads pre-computed data ───────────────
        Command::Insert(InsertKind::Char { char }) => {
            let cursor = ctx.cursor_usize();
            match &ctx.precomputed {
                InsertPrecomputed::Newline {
                    insert_text,
                    trailing_strip_len,
                    leading_strip_len,
                    cursor_advance,
                } => {
                    let effects = insert_effects::newline_with_indent(
                        cursor,
                        *trailing_strip_len,
                        *leading_strip_len,
                        insert_text,
                        *cursor_advance,
                    );
                    CommandResult::effects_only(effects)
                }
                InsertPrecomputed::Tab { spaces } => {
                    const MAX_TAB: usize = 32;
                    const SPACES: &str = "                                "; // 32 spaces
                    let n = (*spaces).min(MAX_TAB);
                    let effects = insert_effects::insert_text_and_advance(
                        cursor,
                        CompactString::from(&SPACES[..n]),
                        cursor + n,
                    );
                    CommandResult::effects_only(effects)
                }
                InsertPrecomputed::ReplaceTab {
                    spaces, delete_len, ..
                } => {
                    // Tab in replace mode: delete char under cursor, insert expanded spaces
                    const MAX_TAB: usize = 32;
                    const SPACES: &str = "                                "; // 32 spaces
                    let n = (*spaces).min(MAX_TAB);
                    let mut effects = crate::effects::Effects::new();
                    if let Some(del_len) = delete_len {
                        effects = effects.delete(crate::primitives::Range::new(
                            crate::primitives::Offset::new(cursor),
                            crate::primitives::Offset::new(cursor + del_len),
                        ));
                    }
                    effects = effects
                        .insert(
                            crate::primitives::Offset::new(cursor),
                            CompactString::from(&SPACES[..n]),
                        )
                        .set_cursor(crate::primitives::Offset::new(cursor + n));
                    CommandResult::effects_only(effects)
                }
                InsertPrecomputed::ReplaceChar {
                    ch: _, delete_len, ..
                } => {
                    let effects = insert_effects::replace_char_at(cursor, *char, *delete_len);
                    CommandResult::effects_only(effects)
                }
                InsertPrecomputed::Char(ch) => {
                    // Check auto-pairs before normal insertion
                    if let Some(pairs) = ctx.auto_pairs {
                        if let Some(result) =
                            auto_pairs::auto_pair_hook(ctx.text, ctx.cursor, *ch, pairs)
                        {
                            return result;
                        }
                    }
                    let effects = insert_effects::insert_text_and_advance(
                        cursor,
                        CompactString::from(&*ch.encode_utf8(&mut [0u8; 4])),
                        cursor + ch.len_utf8(),
                    );
                    CommandResult::effects_only(effects)
                }
                InsertPrecomputed::CharWithIndentAdjust {
                    ch,
                    strip_start,
                    strip_end,
                    new_indent,
                } => {
                    let mut effects = crate::effects::Effects::new();
                    if *strip_start < *strip_end {
                        effects = effects.delete(crate::primitives::Range::new(
                            crate::primitives::Offset::new(*strip_start),
                            crate::primitives::Offset::new(*strip_end),
                        ));
                    }
                    let insert_pos = *strip_start + new_indent.len();
                    if !new_indent.is_empty() {
                        effects = effects.insert(
                            crate::primitives::Offset::new(*strip_start),
                            new_indent.clone(),
                        );
                    }
                    effects = effects.insert(
                        crate::primitives::Offset::new(insert_pos),
                        CompactString::from(&*ch.encode_utf8(&mut [0u8; 4])),
                    );
                    effects = effects
                        .set_cursor(crate::primitives::Offset::new(insert_pos + ch.len_utf8()));
                    CommandResult::effects_only(effects)
                }
                other => {
                    debug_assert!(false, "InsertChar with unexpected precomputed: {other:?}");
                    CommandResult::effects_only(crate::effects::Effects::new().show_error(
                        crate::errors::VimError::InternalError(
                            format!("InsertChar with unexpected precomputed: {other:?}").into(),
                        ),
                    ))
                }
            }
        }

        // ── Literal character (Ctrl-V): bypass expandtab/auto-pairs ──
        Command::Insert(InsertKind::LiteralChar { char }) => {
            let cursor = ctx.cursor_usize();
            let effects = insert_effects::insert_text_and_advance(
                cursor,
                CompactString::from(&*char.encode_utf8(&mut [0u8; 4])),
                cursor + char.len_utf8(),
            );
            CommandResult::effects_only(effects)
        }

        // ── Deletion commands ──────────────────────────────────────────
        Command::Insert(InsertKind::Backspace) => {
            if let InsertPrecomputed::ReplaceBackspace { action, prev_pos } = &ctx.precomputed {
                match action {
                    ReplaceRestoreAction::Restore(original_char) => {
                        if ctx.cursor_usize() == 0 {
                            return CommandResult::none();
                        }
                        let effects = insert_effects::replace_backspace(
                            *prev_pos,
                            ctx.cursor_usize(),
                            *original_char,
                        );
                        CommandResult::effects_only(effects)
                    }
                    ReplaceRestoreAction::JoinLine {
                        prev_line_end,
                        delete_len,
                    } => {
                        let effects = insert_effects::replace_backspace_join_line(
                            *prev_line_end,
                            *delete_len,
                        );
                        CommandResult::effects_only(effects)
                    }
                    ReplaceRestoreAction::StackEmpty => {
                        // Stack empty: move cursor left without modifying text.
                        // In Vim, R-mode backspace at BOL moves to previous line's end.
                        if *prev_pos < ctx.cursor_usize() {
                            let effects = crate::effects::Effects::new()
                                .set_cursor(crate::primitives::Offset::new(*prev_pos));
                            CommandResult::effects_only(effects)
                        } else {
                            CommandResult::none()
                        }
                    }
                }
            } else {
                // Check auto-pair backspace before normal backspace
                if let Some(pairs) = ctx.auto_pairs {
                    if let Some(result) =
                        auto_pairs::auto_pair_backspace(ctx.text, ctx.cursor, pairs)
                    {
                        return result;
                    }
                }
                delete::backspace(ctx)
            }
        }
        Command::Insert(InsertKind::DeleteWord) => delete::delete_word(ctx),
        Command::Insert(InsertKind::DeleteToStart) => delete::delete_to_start(ctx),
        Command::Insert(InsertKind::DeleteUnder) => delete::delete_under(ctx),

        // ── Indentation ────────────────────────────────────────────────
        Command::Insert(InsertKind::Indent) => {
            insert_indent::indent_at_cursor(ctx.text, ctx.cursor, ctx.shift_width)
        }
        Command::Insert(InsertKind::Outdent) => {
            insert_indent::outdent_at_cursor(ctx.text, ctx.cursor, ctx.shift_width, ctx.tabstop)
        }
        Command::Insert(InsertKind::OutdentTemporary) => {
            // ^^D: indent saving is handled in the engine layer (mode_dispatch)
            // before dispatch. This arm removes all indent.
            insert_indent::outdent_all_at_cursor(ctx.text, ctx.cursor)
        }
        Command::Insert(InsertKind::OutdentClear) => {
            // 0^D: permanent indent removal, no saving.
            insert_indent::outdent_all_at_cursor(ctx.text, ctx.cursor)
        }

        // ── Special insert commands ────────────────────────────────────
        Command::Insert(InsertKind::OneShot) => {
            // Engine handles set_return_to() on VimState.
            // We ONLY return the SetMode effect — single owner.
            CommandResult::effects_only(insert_effects::one_shot_normal())
        }

        // ── Paste request (async via host) ────────────────────────────
        Command::Insert(InsertKind::Paste) => CommandResult::none(),

        // ── Copy char from adjacent line ───────────────────────────────
        Command::Insert(InsertKind::CopyCharBelow | InsertKind::CopyCharAbove) => {
            match &ctx.precomputed {
                InsertPrecomputed::Char(ch) => {
                    let cursor = ctx.cursor_usize();
                    let effects = insert_effects::insert_text_and_advance(
                        cursor,
                        CompactString::from(&*ch.encode_utf8(&mut [0u8; 4])),
                        cursor + ch.len_utf8(),
                    );
                    CommandResult::effects_only(effects)
                }
                _ => CommandResult::none(), // no char on adjacent line at this column
            }
        }

        // ── Ctrl-G sub-commands ─────────────────────────────────────────
        Command::Insert(InsertKind::BreakUndoSequence) => {
            use crate::effects::{undo_state, Effects};
            // In insert mode, an undo group is already open. Close it and
            // immediately re-open to start a new undo sequence.
            CommandResult::effects_only(
                Effects::<undo_state::UndoOpen>::resume_open()
                    .end_undo()
                    .begin_undo()
                    .into_raw_closed(),
            )
        }

        // Ctrl-G U: sets dont_sync_undo flag on InsertState (handled in engine),
        // no effects needed here.
        Command::Insert(InsertKind::DontSyncUndo) => CommandResult::none(),

        // Nop: no-op from cancelled expression input
        Command::Insert(InsertKind::Nop) => CommandResult::none(),

        Command::Insert(
            InsertKind::LastInserted
            | InsertKind::LastInsertedAndExit
            | InsertKind::Register { .. }
            | InsertKind::ExpressionResult { .. }
            | InsertKind::HostInserted
            | InsertKind::InsertWordUnderCursor
            | InsertKind::InsertWORDUnderCursor
            | InsertKind::InsertCurrentLine,
        ) => {
            match &ctx.precomputed {
                InsertPrecomputed::Text(text) => {
                    let cursor = ctx.cursor_usize();
                    let new_cursor = cursor + text.len();
                    let effects =
                        insert_effects::insert_text_and_advance(cursor, text.clone(), new_cursor);
                    CommandResult::effects_only(effects)
                }
                InsertPrecomputed::Error(err) => {
                    let effects = crate::effects::Effects::new().show_error(err.clone());
                    CommandResult::effects_only(effects)
                }
                _ => CommandResult::none(), // no text to insert
            }
        }

        // ── Completion request (async via host) ──────────────────────
        // Safety arm: the engine intercepts RequestCompletion before it
        // reaches dispatch. If this arm is reached it indicates a bug;
        // return empty effects to avoid panic in production.
        Command::Insert(InsertKind::RequestCompletion { .. }) => {
            debug_assert!(
                false,
                "RequestCompletion should be intercepted by the engine"
            );
            CommandResult::none()
        }

        // Unreachable in correct pipeline:
        // - InsertEntry is a normal-mode command (executor → entry::execute directly)
        // - InsertExit is intercepted by mode handler (ModeAction::InsertExit)
        // - Non-insert commands (Motion, Action) are guarded by is_insert_specific()
        other => {
            debug_assert!(false, "Invalid command reached dispatch_insert: {other:?}");
            CommandResult::effects_only(crate::effects::Effects::new().show_error(
                crate::errors::VimError::InternalError(
                    format!("Invalid command in insert mode: {other:?}").into(),
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;

    #[test]
    fn test_dispatch_backspace() {
        let ctx = InsertContext::new("hello", Offset::new(2));
        let cmd = Command::Insert(InsertKind::Backspace);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_delete_word() {
        let ctx = InsertContext::new("hello world", Offset::new(11));
        let cmd = Command::Insert(InsertKind::DeleteWord);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_insert_char_normal() {
        let ctx = InsertContext::new("hello", Offset::new(5))
            .with_precomputed(InsertPrecomputed::Char('x'));
        let cmd = Command::Insert(InsertKind::Char { char: 'x' });
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_insert_char_newline() {
        let ctx = InsertContext::new("hello", Offset::new(5)).with_precomputed(
            InsertPrecomputed::Newline {
                insert_text: CompactString::from("\nhello"),
                trailing_strip_len: 0,
                leading_strip_len: 0,
                cursor_advance: 11,
            },
        );
        let cmd = Command::Insert(InsertKind::Char { char: '\n' });
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_insert_char_tab() {
        let ctx = InsertContext::new("hello", Offset::new(5))
            .with_precomputed(InsertPrecomputed::Tab { spaces: 4 });
        let cmd = Command::Insert(InsertKind::Char { char: '\t' });
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_replace_char() {
        use crate::primitives::ReplacedChar;
        let ctx = InsertContext::new("hello", Offset::new(2)).with_precomputed(
            InsertPrecomputed::ReplaceChar {
                ch: 'X',
                original_char: ReplacedChar::Replaced('l'),
                delete_len: Some(1),
            },
        );
        let cmd = Command::Insert(InsertKind::Char { char: 'X' });
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_replace_backspace_restores() {
        use crate::primitives::ReplacedChar;
        let ctx = InsertContext::new("hello", Offset::new(2)).with_precomputed(
            InsertPrecomputed::ReplaceBackspace {
                action: ReplaceRestoreAction::Restore(ReplacedChar::Replaced('e')),
                prev_pos: 1,
            },
        );
        let cmd = Command::Insert(InsertKind::Backspace);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_replace_backspace_empty_stack() {
        let ctx = InsertContext::new("hello", Offset::new(2)).with_precomputed(
            InsertPrecomputed::ReplaceBackspace {
                action: ReplaceRestoreAction::StackEmpty,
                prev_pos: 1,
            },
        );
        let cmd = Command::Insert(InsertKind::Backspace);
        let result = dispatch_insert(&cmd, &ctx);
        // Stack empty: cursor moves left without modifying text (Vim behavior)
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_replace_backspace_join_line() {
        // "hello\n    world" — cursor at start of "world" (offset 10),
        // backspace should join by deleting "\n    " (5 bytes), cursor to 5.
        let ctx = InsertContext::new("hello\n    world", Offset::new(10)).with_precomputed(
            InsertPrecomputed::ReplaceBackspace {
                action: ReplaceRestoreAction::JoinLine {
                    prev_line_end: 5,
                    delete_len: 5, // \n + 4 spaces
                },
                prev_pos: 5,
            },
        );
        let cmd = Command::Insert(InsertKind::Backspace);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
        // Should have a Delete effect covering the newline + indent
        assert!(result.effects.iter().any(|e| matches!(
            e,
            crate::effects::Effect::Delete { range }
            if range.start().get() == 5 && range.end().get() == 10
        )));
        // Cursor should be set to end of previous line
        assert!(result.effects.iter().any(
            |e| matches!(e, crate::effects::Effect::SetCursor { offset } if offset.get() == 5)
        ));
    }

    #[test]
    fn test_dispatch_last_inserted_with_text() {
        let ctx = InsertContext::new("hello", Offset::new(5))
            .with_precomputed(InsertPrecomputed::Text(CompactString::from("world")));
        let cmd = Command::Insert(InsertKind::LastInserted);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_last_inserted_no_text() {
        let ctx = InsertContext::new("hello", Offset::new(5));
        let cmd = Command::Insert(InsertKind::LastInserted);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(result.is_empty());
    }

    #[test]
    fn test_dispatch_register_with_text() {
        let ctx = InsertContext::new("hello", Offset::new(5))
            .with_precomputed(InsertPrecomputed::Text(CompactString::from("reg_content")));
        let cmd = Command::Insert(InsertKind::Register {
            register: crate::primitives::RegisterName::UNNAMED,
        });
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_dispatch_one_shot_returns_set_mode() {
        let ctx = InsertContext::new("hello", Offset::new(3));
        let cmd = Command::Insert(InsertKind::OneShot);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
        // Should contain SetMode(Normal) effect
        assert!(result.effects.iter().any(|e|
            matches!(e, crate::effects::Effect::SetMode { mode, .. } if *mode == crate::primitives::Mode::Normal)
        ));
    }

    #[test]
    fn test_dispatch_host_inserted_with_text() {
        let ctx = InsertContext::new("hello", Offset::new(5))
            .with_precomputed(InsertPrecomputed::Text(CompactString::from("completion")));
        let cmd = Command::Insert(InsertKind::HostInserted);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
        // Should produce Insert effect with the text
        assert!(result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Insert { .. })));
    }

    #[test]
    fn test_dispatch_host_inserted_empty() {
        let ctx =
            InsertContext::new("hello", Offset::new(5)).with_precomputed(InsertPrecomputed::None);
        let cmd = Command::Insert(InsertKind::HostInserted);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(result.is_empty());
    }

    // ── OutdentTemporary / OutdentClear ────────────────────────────────

    #[test]
    fn test_dispatch_outdent_temporary_removes_all_indent() {
        // "        ^hello" — 8 spaces + trigger '^', cursor after trigger
        let ctx = InsertContext::new("        ^hello", Offset::new(9));
        let cmd = Command::Insert(InsertKind::OutdentTemporary);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
        // Should contain Delete effects and SetCursor
        assert!(result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Delete { .. })));
        assert!(result.effects.iter().any(
            |e| matches!(e, crate::effects::Effect::SetCursor { offset } if offset.get() == 0)
        ));
    }

    #[test]
    fn test_dispatch_outdent_clear_removes_all_indent() {
        // "    0hello" — 4 spaces + trigger '0', cursor after trigger
        let ctx = InsertContext::new("    0hello", Offset::new(5));
        let cmd = Command::Insert(InsertKind::OutdentClear);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
        assert!(result
            .effects
            .iter()
            .any(|e| matches!(e, crate::effects::Effect::Delete { .. })));
        assert!(result.effects.iter().any(
            |e| matches!(e, crate::effects::Effect::SetCursor { offset } if offset.get() == 0)
        ));
    }

    #[test]
    fn test_dispatch_outdent_temporary_no_indent() {
        // "^hello" — no indent, just trigger
        let ctx = InsertContext::new("^hello", Offset::new(1));
        let cmd = Command::Insert(InsertKind::OutdentTemporary);
        let result = dispatch_insert(&cmd, &ctx);
        assert!(!result.is_empty());
    }
}
