//! Effect processing and state synchronization — extracted from `engine.rs`.
//!
//! Contains the effect-scanning loop from `execute_command` and the
//! dot-repeat text injection logic, decomposed into ≤60-line helpers.

use crate::commands::insert::effects as insert_effects;
use crate::effects::Effect;
use crate::execution::response::Response;
use crate::grammar::parser::Parser;
use crate::primitives::byte_delta;
use crate::primitives::Mode;
use crate::primitives::{
    Offset, RegisterName, ReturnTo, SelectionRange, Selections, UndoNavStep, VimEvent,
};
use crate::state::mark_snapshot::MarkSnapshot;
use crate::state::VimState;
use smallvec::SmallVec;

/// Result of single-pass effect processing.
///
/// Carries all outputs from the unified retain loop, eliminating
/// the need for separate macro-interception and cursor-scanning passes.
pub(crate) struct ProcessResult {
    /// Dot-repeat intercept count, if intercepted.
    pub repeat_count: Option<u32>,
    /// Collected PlayMacro effects (register + count), removed from response.
    pub macro_plays: SmallVec<[(RegisterName, u32); 2]>,
    /// Last SetCursor offset seen during processing (for repeat-text injection).
    pub last_cursor_offset: Option<Offset>,
    /// Recording started for this register (engine should init buffer).
    pub recording_started: Option<RegisterName>,
    /// Recording stopped (engine should flush buffer to register).
    pub recording_stopped: bool,
    /// True if dot-repeat was active and should now stop (SetMode(Insert) intercepted).
    pub ended_repeat: bool,
    /// Text written to the unnamed register during this effect batch, if any.
    ///
    /// Set when a `SetRegister` effect caused the unnamed (`"`) register to be
    /// updated. The engine uses this to emit `CopyToClipboard` when the
    /// `clipboard=unnamed` or `clipboard=unnamedplus` option is active.
    pub unnamed_register_written: Option<compact_str::CompactString>,
    /// Last cursor from an Undo or Redo navigation step.
    ///
    /// Used by the engine to set the sticky column (curswant) after undo/redo.
    /// Neovim resets curswant to the column of the restored cursor position.
    pub last_undo_redo_cursor: Option<Offset>,
}

/// Process all effects in a **single O(n) pass**: sync internal state,
/// handle dot-repeat interception, collect PlayMacro effects, and track
/// the last SetCursor offset.
///
/// Uses `Vec::retain()` for single-pass processing. Effects that are
/// intercepted (dot-repeat, PlayMacro) are removed; all others are kept.
pub(crate) fn process_effects(
    state: &mut VimState,
    parser: &mut Parser,
    was_repeating: bool,
    response: &mut Response,
) -> ProcessResult {
    process_effects_with_text(
        state,
        parser,
        was_repeating,
        response,
        None,
        None,
        8,
        None,
        &[],
    )
}

/// Process effects with optional text for changelist line-based dedup.
///
/// `undolevels_max`: when `Some(N)`, the undo tree is pruned to at most `N`
/// live nodes after each committed undo group. `None` means unlimited.
///
/// `tabstop`: tab stop width for virtual column computation in the auto-emitted
/// `SetStickyColumn`. Neovim's curswant is a virtual column (tab-aware), not a
/// byte offset within the line, so we use `curswant_of` rather than `column_of`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_effects_with_text(
    state: &mut VimState,
    parser: &mut Parser,
    was_repeating: bool,
    response: &mut Response,
    text: Option<&str>,
    undolevels_max: Option<usize>,
    tabstop: usize,
    undo_auto_group_ms: Option<u32>,
    cursor_shape_overrides: &[Option<crate::primitives::CursorShape>],
) -> ProcessResult {
    // Undo auto-grouping pre-pass: auto-open a group if needed.
    if undo_auto_group_ms.is_some()
        && !state.undo_tree().has_pending_group()
        && !state.undo_auto_group_active()
        && response.effects.iter().any(|e| {
            matches!(
                e,
                Effect::Insert { .. } | Effect::Delete { .. } | Effect::Replace { .. }
            )
        })
    {
        let cursor_offsets: SmallVec<[Offset; 1]> = {
            let sels = state.multi_cursor().selections();
            if sels.len() <= 1 {
                smallvec::smallvec![state.undo_cursor_hint()]
            } else {
                sels.iter().map(|r| r.head()).collect()
            }
        };
        let marks_snapshot = MarkSnapshot::capture(state.marks());
        let mode = state.mode();
        let last_visual = state.last_visual();
        state.undo_tree_mut().begin_group_multi(
            &cursor_offsets,
            crate::primitives::UndoCursorStrategy::FirstEdit,
            marks_snapshot,
            text.map(str::len),
            mode,
            last_visual,
            false,
        );
        state.set_undo_auto_group_active(true);
    }

    let mut repeat_intercept_count: Option<u32> = None;
    let mut ended_repeat = false;
    let mut macro_plays: SmallVec<[(RegisterName, u32); 2]> = SmallVec::new();
    let mut last_cursor_offset: Option<Offset> = None;
    let mut recording_started: Option<RegisterName> = None;
    let mut recording_stopped = false;
    let mut unnamed_was_written = false;
    let mut cursor_moved = false;
    let mut text_mutated = false;
    let mut last_undo_redo_cursor: Option<Offset> = None;
    let mut sticky_column_emitted = false;
    let mut bell_seen = false;
    let mode_before = state.mode();

    // Capture belloff from state (synced from VimOptions by the engine).
    let belloff = state.belloff();

    response.effects.retain_mut(|effect| {
        // ── Bell filtering: belloff option and rate limiting ─────────
        if matches!(effect, Effect::Bell) {
            if belloff {
                return false; // belloff=all: suppress all bells
            }
            let count = state.increment_bell_count();
            if count > VimState::BELL_RATE_LIMIT {
                return false; // rate limit exceeded: suppress
            }
            bell_seen = true;
            return true; // keep this bell
        }

        // Track last cursor position (for repeat-text injection + undo tree).
        // Note: we do NOT snap SetCursor offsets here because `text` is the
        // pre-edit document, while SetCursor offsets are relative to the
        // post-edit state.  Char-boundary enforcement happens at the input
        // gate (`InputContext::validate`) on the next process() call.
        if let Effect::SetCursor { offset } = effect {
            last_cursor_offset = Some(*offset);
            cursor_moved = true;
            state.set_undo_cursor_hint(*offset);
        }

        // Track text mutations for TextChanged/TextChangedI events
        if matches!(
            effect,
            Effect::Insert { .. } | Effect::Delete { .. } | Effect::Replace { .. }
        ) {
            text_mutated = true;
        }

        // Track explicit SetStickyColumn so we don't auto-emit after the loop
        if matches!(effect, Effect::SetStickyColumn { .. }) {
            sticky_column_emitted = true;
        }

        // Track when the unnamed register is written (directly or via delete routing).
        // NUMBERED_1 and SMALL_DELETE route through on_delete() which always writes unnamed.
        if let Effect::SetRegister { name, .. } = effect {
            if *name == RegisterName::UNNAMED
                || *name == RegisterName::NUMBERED_1
                || *name == RegisterName::SMALL_DELETE
            {
                unnamed_was_written = true;
            }
        }

        let disposition =
            process_one_effect(effect, state, parser, was_repeating, text, undolevels_max);

        // Track undo/redo cursor from the last navigation step.
        // This is read after process_one_effect has populated the steps.
        match effect {
            Effect::Undo { steps, .. } | Effect::Redo { steps, .. } => {
                if let Some(last_step) = steps.last() {
                    last_undo_redo_cursor = Some(last_step.cursor());
                }
            }
            _ => {}
        }

        match disposition {
            EffectDisposition::Keep => true,
            EffectDisposition::Consume => false,
            EffectDisposition::InterceptRepeatSetMode => {
                repeat_intercept_count = Some(1);
                ended_repeat = true;
                false
            }
            EffectDisposition::InterceptRepeatBeginInsert(count) => {
                repeat_intercept_count = Some(count);
                ended_repeat = true;
                false
            }
            EffectDisposition::InterceptMacro(register, count) => {
                macro_plays.push((register, count));
                false
            }
            EffectDisposition::InterceptStartRecording(register) => {
                recording_started = Some(register);
                false
            }
            EffectDisposition::InterceptStopRecording => {
                recording_stopped = true;
                false
            }
        }
    });

    // Reset bell rate-limit counter when no bell was emitted in this batch.
    if !bell_seen {
        state.reset_bell_count();
    }

    // Changelist updates for undo/redo are now handled inline in the
    // Undo/Redo effect processing above, using the undo step cursor
    // (which derives from first_edit_offset) as the edit position.

    // Auto-emit SetStickyColumn when cursor moved but no explicit sticky was set.
    // This ensures curswant is always correct after any cursor-moving operation
    // (paste, undo/redo, visual exit, operators, etc.) without requiring each
    // command handler to emit SetStickyColumn individually.
    // Operations like `$` that explicitly set END_OF_LINE are unaffected because
    // they emit their own SetStickyColumn, setting sticky_column_emitted = true.
    if cursor_moved && !sticky_column_emitted {
        if let Some(cursor_offset) = last_cursor_offset {
            if let Some(doc) = text {
                let col = crate::commands::helpers::curswant_of(
                    doc,
                    cursor_offset.get().min(doc.len()),
                    tabstop,
                );
                let sticky = Effect::SetStickyColumn {
                    column: Some(crate::primitives::VirtualColumn::new(col)),
                };
                state.set_sticky_column(Some(crate::primitives::VirtualColumn::new(col)));
                response.effects.push(sticky);
            }
        }
    }

    // Emit typed events for mode transitions
    emit_mode_events(&mut response.effects, mode_before, state.mode());

    // Emit cursor style alongside mode transitions
    emit_cursor_style(
        &mut response.effects,
        mode_before,
        state.mode(),
        cursor_shape_overrides,
    );

    // Emit observation events: CursorMoved/I, TextChanged/I, RecordingEnter/Leave
    emit_observation_events(
        &mut response.effects,
        state.mode(),
        &ObservationFlags {
            cursor_moved,
            text_mutated,
            recording_started: recording_started.is_some(),
            recording_stopped,
        },
    );

    // Undo auto-grouping post-pass: update timestamp.
    if state.undo_auto_group_active() && text_mutated {
        state.set_undo_auto_group_last_edit_ms(state.undo_timestamp_hint());
    }

    // Capture unnamed register text if it was written during this batch.
    // The engine will use this to emit CopyToClipboard if clipboard option is set.
    let unnamed_register_written = if unnamed_was_written {
        state
            .registers()
            .get(RegisterName::UNNAMED)
            .map(|c| compact_str::CompactString::from(c.text()))
    } else {
        None
    };

    ProcessResult {
        repeat_count: repeat_intercept_count,
        macro_plays,
        last_cursor_offset,
        recording_started,
        recording_stopped,
        ended_repeat,
        unnamed_register_written,
        last_undo_redo_cursor,
    }
}

/// Emit typed `VimEvent` effects for mode transitions.
///
/// Compares the mode before and after effect processing and appends
/// events like `InsertEnter`, `InsertLeave`, `CmdlineEnter`, etc.
fn emit_mode_events(effects: &mut SmallVec<[Effect; 4]>, from: Mode, to: Mode) {
    use crate::primitives::VimEvent;
    if from == to {
        return;
    }
    // Specific leave events
    if from.is_insert() {
        effects.push(Effect::Event {
            kind: VimEvent::InsertLeave,
        });
    }
    if from.is_visual() {
        effects.push(Effect::Event {
            kind: VimEvent::VisualLeave,
        });
    }
    if from.is_command_line() {
        effects.push(Effect::Event {
            kind: VimEvent::CmdlineLeave,
        });
    }
    if from.is_replace() {
        effects.push(Effect::Event {
            kind: VimEvent::InsertLeave,
        });
    }
    if from.is_select() {
        effects.push(Effect::Event {
            kind: VimEvent::SelectLeave,
        });
    }
    // Specific enter events
    if to.is_insert() {
        effects.push(Effect::Event {
            kind: VimEvent::InsertEnter,
        });
    }
    if to.is_visual() {
        effects.push(Effect::Event {
            kind: VimEvent::VisualEnter,
        });
    }
    if to.is_command_line() {
        effects.push(Effect::Event {
            kind: VimEvent::CmdlineEnter,
        });
    }
    if to.is_replace() {
        effects.push(Effect::Event {
            kind: VimEvent::InsertEnter,
        });
    }
    if to.is_select() {
        effects.push(Effect::Event {
            kind: VimEvent::SelectEnter,
        });
    }
    // General mode change event
    effects.push(Effect::Event {
        kind: VimEvent::ModeChanged { from, to },
    });
}

/// Emit a `SetCursorStyle` effect when the mode changes.
///
/// Called after the retain loop and after [`emit_mode_events`]. Only emits
/// when `from != to` so that no-op mode "changes" are silent.
/// Applies per-mode cursor shape overrides from options when available.
fn emit_cursor_style(
    effects: &mut SmallVec<[Effect; 4]>,
    from: Mode,
    to: Mode,
    overrides: &[Option<crate::primitives::CursorShape>],
) {
    use crate::primitives::CursorStyle;
    if from == to {
        return;
    }
    effects.push(Effect::SetCursorStyle {
        style: CursorStyle::for_mode_with_overrides(to, overrides),
    });
}

/// Flags indicating what happened during effect processing, for event emission.
#[allow(clippy::struct_excessive_bools)] // genuinely independent flags
struct ObservationFlags {
    cursor_moved: bool,
    text_mutated: bool,
    recording_started: bool,
    recording_stopped: bool,
}

/// Emit observation events based on what happened during effect processing.
///
/// Fires `CursorMoved`/`CursorMovedI` when cursor position changed,
/// `TextChanged`/`TextChangedI` when text was mutated, and
/// `RecordingEnter`/`RecordingLeave` when macro recording state changed.
///
/// These match Neovim's autocmd events: `CursorMoved`, `CursorMovedI`,
/// `TextChanged`, `TextChangedI`, `RecordingEnter`, `RecordingLeave`.
fn emit_observation_events(
    effects: &mut SmallVec<[Effect; 4]>,
    mode_after: Mode,
    flags: &ObservationFlags,
) {
    if flags.cursor_moved {
        if mode_after.is_insert() || mode_after.is_replace() {
            effects.push(Effect::Event {
                kind: VimEvent::CursorMovedI,
            });
        } else {
            effects.push(Effect::Event {
                kind: VimEvent::CursorMoved,
            });
        }
    }

    if flags.text_mutated {
        if mode_after.is_insert() || mode_after.is_replace() {
            effects.push(Effect::Event {
                kind: VimEvent::TextChangedI,
            });
        } else {
            effects.push(Effect::Event {
                kind: VimEvent::TextChanged,
            });
        }
    }

    if flags.recording_started {
        effects.push(Effect::Event {
            kind: VimEvent::RecordingEnter,
        });
    }
    if flags.recording_stopped {
        effects.push(Effect::Event {
            kind: VimEvent::RecordingLeave,
        });
    }
}

/// What the retain loop should do with this effect.
enum EffectDisposition {
    /// Keep the effect in the response (synced if needed).
    Keep,
    /// Consume the effect (remove from response, state synced inline).
    /// Used for engine-internal effects that should not reach the host.
    Consume,
    /// Dot-repeat: intercept SetMode(Insert), remove effect.
    InterceptRepeatSetMode,
    /// Dot-repeat: intercept BeginInsert, remove effect (state already synced).
    InterceptRepeatBeginInsert(u32),
    /// Macro: intercept PlayMacro, remove effect.
    InterceptMacro(RegisterName, u32),
    /// Macro recording: intercept StartRecording (engine inits buffer).
    InterceptStartRecording(RegisterName),
    /// Macro recording: intercept StopRecording (engine flushes buffer).
    InterceptStopRecording,
}

/// Classify AND sync a single effect in one exhaustive match.
///
/// This is the single source of truth: every `Effect` variant is handled
/// exactly once. State sync happens inline for syncable effects.
fn process_one_effect(
    effect: &mut Effect,
    state: &mut VimState,
    parser: &mut Parser,
    is_repeating: bool,
    text: Option<&str>,
    undolevels_max: Option<usize>,
) -> EffectDisposition {
    match effect {
        // ── Dot-repeat interception ──────────────────────────────────────
        Effect::SetMode { mode, .. } if is_repeating && *mode == Mode::Insert => {
            EffectDisposition::InterceptRepeatSetMode
        }
        Effect::BeginInsert { count, .. } if is_repeating => {
            let c = *count;
            sync_begin_insert(state, effect);
            state.set_return_to(ReturnTo::None);
            EffectDisposition::InterceptRepeatBeginInsert(c)
        }

        // ── Effects that require internal state synchronization ──────────
        Effect::SetMode { mode, .. } => {
            handle_set_mode(state, *mode);
            EffectDisposition::Keep
        }
        Effect::BeginInsert { .. } => {
            sync_begin_insert(state, effect);
            state.set_mode(Mode::Insert);
            state.set_return_to(ReturnTo::None);
            EffectDisposition::Keep
        }
        Effect::SetBlockInsert {
            lines_below,
            grapheme_col,
            cursor_return_offset,
        } => {
            handle_set_block_insert(state, *lines_below, *grapheme_col, *cursor_return_offset);
            EffectDisposition::Keep
        }
        Effect::SetRegister {
            name,
            text,
            motion_type,
        } => {
            handle_set_register(state, *name, text, *motion_type);
            EffectDisposition::Keep
        }
        Effect::SetLastFind {
            direction,
            target_char,
            sneak_c2,
            resolved_ignorecase,
            resolved_smartcase,
        } => {
            use crate::primitives::LastFind;
            let mut last_find = LastFind::new();
            if let Some(c2) = sneak_c2 {
                last_find.record_sneak_with_case(
                    *direction,
                    *target_char,
                    *c2,
                    *resolved_ignorecase,
                    *resolved_smartcase,
                );
            } else {
                last_find.record_with_case(
                    *direction,
                    *target_char,
                    *resolved_ignorecase,
                    *resolved_smartcase,
                );
            }
            state.set_last_find(last_find);
            EffectDisposition::Keep
        }
        Effect::SetLastSubstitute { replacement } => {
            state.search_mut().set_last_substitute(replacement);
            EffectDisposition::Keep
        }
        Effect::SetLastSubstituteFlags { flags } => {
            state.search_mut().set_last_substitute_flags(*flags);
            EffectDisposition::Keep
        }
        Effect::SetSubstitutePattern { pattern } => {
            state.search_mut().set_substitute_pattern(pattern.as_str());
            EffectDisposition::Keep
        }
        Effect::SetSearchPattern { pattern, direction } => {
            state.search_mut().set_pattern(
                pattern.as_str(),
                crate::primitives::SearchDirection::from(*direction),
            );
            EffectDisposition::Keep
        }
        Effect::StartRecording { register } => {
            if state.macros().is_replaying() {
                EffectDisposition::Consume
            } else {
                parser.set_recording(Some(*register));
                state.macros_mut().start_recording(*register);
                EffectDisposition::InterceptStartRecording(*register)
            }
        }
        Effect::StopRecording => {
            parser.set_recording(None);
            state.macros_mut().stop_recording();
            EffectDisposition::InterceptStopRecording
        }
        Effect::SaveLastVisual { info } => {
            state.set_last_visual(*info);
            EffectDisposition::Keep
        }
        Effect::SetMark {
            name,
            offset,
            topline_offset,
        } => {
            state.marks_mut().set(
                *name,
                crate::primitives::Mark::with_topline_offset(*offset, *topline_offset),
            );
            // Mirror Neovim's changed_common(): the `.` mark and the last
            // changelist entry are always in lockstep.  When a command
            // explicitly sets mark `.` (e.g. visual g<C-a>, :s), the
            // changelist must follow — Neovim's final changed_lines() call
            // updates both simultaneously.
            if *name == crate::primitives::MarkName::LAST_CHANGE {
                state.changelist_mut().update_last(*offset);
            }
            EffectDisposition::Keep
        }
        // Changelist navigation — sync state, cursor already moved by SetCursor
        Effect::ChangelistOlder { count } => {
            for _ in 0..*count {
                state.changelist_mut().older();
            }
            EffectDisposition::Keep
        }
        Effect::ChangelistNewer { count } => {
            for _ in 0..*count {
                state.changelist_mut().newer();
            }
            EffectDisposition::Keep
        }
        // Jump list navigation — cleanup duplicates then sync state.
        // Cleanup mirrors Neovim's cleanup_jumplist() called inside get_jumplist().
        Effect::JumpOlder { count } => {
            state.jump_list_mut().cleanup(None, offset_to_line_fn(text));
            for _ in 0..*count {
                state.jump_list_mut().older();
            }
            EffectDisposition::Keep
        }
        Effect::JumpNewer { count } => {
            state.jump_list_mut().cleanup(None, offset_to_line_fn(text));
            for _ in 0..*count {
                state.jump_list_mut().newer();
            }
            EffectDisposition::Keep
        }
        // Cross-buffer jump — pass through to shell (no state sync needed)
        Effect::JumpToBuffer { .. } => EffectDisposition::Keep,
        Effect::BeginUndoGroup { cursor_strategy } => {
            // Note: double-open can happen legitimately (multi-cursor, macro replay).
            // The undo tree handles it by committing the pending group first.
            // Reset `.` and `[` mark tracking — new undo group starts fresh
            state.set_last_change_mark_set(false);
            state.set_change_start_mark_set(false);
            // Mirror Neovim's b_new_change = true in u_savecommon():
            // the next text mutation will create a new changelist entry.
            state.set_changelist_new_change(true);
            // Capture mark snapshot before mutation begins.
            let marks_snapshot = MarkSnapshot::capture(state.marks());
            // Undo tree: begin a pending group with all cursor positions.
            // Collect ALL cursor offsets from multi-cursor state for
            // multi-cursor position restoration on undo/redo.
            let cursor_offsets: SmallVec<[Offset; 1]> = {
                let sels = state.multi_cursor().selections();
                if sels.len() <= 1 {
                    // Single cursor: use the undo cursor hint (may differ
                    // from selection head due to operator positioning).
                    smallvec::smallvec![state.undo_cursor_hint()]
                } else {
                    sels.iter().map(|r| r.head()).collect()
                }
            };
            let mode = state.mode();
            let last_visual = state.last_visual();
            state.undo_tree_mut().begin_group_multi(
                &cursor_offsets,
                *cursor_strategy,
                marks_snapshot,
                text.map(str::len),
                mode,
                last_visual,
                false,
            );
            EffectDisposition::Keep
        }
        Effect::EndUndoGroup { ref mut node_id } => {
            if !state.undo_tree().has_pending_group() {
                warn!("EndUndoGroup without matching BeginUndoGroup — skipping orphaned close");
                return EffectDisposition::Consume;
            }
            // Undo tree: commit the pending group with current cursor and timestamp.
            // Pass `text` (the pre-edit document) so the tree can compute
            // line-start offsets for undo change marks (`'[`, `']`, `'.`).
            let cursor = state.undo_cursor_hint();
            let timestamp = state.undo_timestamp_hint();
            let assigned = state.undo_tree_mut().end_group(cursor, timestamp, text);
            // Write-back: carry the assigned NodeId so the host can key its store.
            *node_id = assigned;
            // If undolevels is set, prune oldest branches to enforce the limit.
            if let Some(max) = undolevels_max {
                state.undo_tree_mut().prune(max);
            }
            EffectDisposition::Keep
        }
        Effect::CommandLineEdit(edit) => {
            state.command_line_mut().apply_edit(*edit);
            EffectDisposition::Keep
        }

        // === Changelist + marks: adjust existing entries, then record edit position ===
        // Changelist only records edits in normal/visual modes (Neovim records one
        // entry per undoable change, not per keystroke during insert).
        Effect::Insert {
            offset,
            text: insert_text,
        } => {
            // Neovim's insert-mode `0<C-D>` / `^<C-D>` indent prefix handler
            // resets the internal Insstart position when `0` or `^` is typed.
            // Even when the next char is NOT <C-D> (so 0/^ is inserted as
            // literal text), Insstart is already reset.  This has two effects
            // on mark..:
            //
            //   1. The '0'/'^' character ITSELF updates mark.. (because the
            //      reset happens before the insertion in Neovim's edit()).
            //   2. The NEXT character after '0'/'^' also updates mark..
            //      (Insstart stays reset across keystrokes).
            //
            // To replicate both effects we reset the tracking flag BEFORE
            // handle_text_mutation (so sync_change_marks records mark.. for
            // the 0/^ char itself) AND AFTER (so it is cleared again for the
            // next keystroke, since sync_change_marks sets it back to true).
            // In Neovim, typing `0`, `^`, or Enter in insert mode resets
            // Insstart, which causes mark `.` to track the position of the
            // NEXT character typed (not the first character of the session).
            // We replicate this by resetting `last_change_mark_set` before
            // AND after the mutation so both the current and next character
            // get a fresh mark `.` tracking opportunity.
            let is_insert_boundary_char = state.mode().is_insert()
                && (matches!(insert_text.as_bytes(), [b'0' | b'^']) || insert_text.contains('\n'));
            if is_insert_boundary_char {
                state.set_last_change_mark_set(false);
            }
            let crosses_line = insert_text.contains('\n');
            handle_text_mutation(
                state,
                *offset,
                0,
                insert_text.len(),
                crosses_line,
                insert_text.ends_with('\n'),
                text,
                Some(insert_text),
            );
            if let Some(t) = text {
                state.undo_tree_mut().mark_edit_at_with_text(*offset, t);
            } else {
                state.undo_tree_mut().mark_edit_at(*offset);
            }
            state
                .undo_tree_mut()
                .mark_insert_extent(*offset, insert_text.len());
            state.undo_tree_mut().mark_has_insert_or_replace();
            state.undo_tree_mut().accumulate_delta(0, insert_text.len());
            // Compute redo mark `']` for this insert in T1 coordinates.
            // After redo, the text is T1 = T0[..offset] + insert_text + T0[offset..].
            // The mark should point to the line-start of the last affected byte in T1.
            if !insert_text.is_empty() {
                let insert_end_t1 = offset.get() + insert_text.len();
                let redo_mark_end_offset = if let Some(last_nl) = insert_text.rfind('\n') {
                    // Newline in inserted text: last line starts after it
                    offset.get() + last_nl + 1
                } else {
                    // No newline: last affected byte is on the same line as
                    // T0[offset], so find the line-start in the combined text.
                    // In T1, T0[0..offset] is unchanged, so line_start is same as T0.
                    text.map_or(offset.get(), |t| {
                        crate::commands::helpers::line_start_for_offset(
                            t,
                            offset.get().min(t.len()),
                        )
                    })
                };
                // Only update if this extends further than a prior insert's mark.
                // Uses max semantics since multiple inserts in one undo group
                // should use the last (highest offset) mark.
                state.undo_tree_mut().update_redo_mark_end_if_larger(
                    Offset::new(redo_mark_end_offset),
                    Offset::new(insert_end_t1),
                );
            }
            if is_insert_boundary_char {
                state.set_last_change_mark_set(false);
            }
            EffectDisposition::Keep
        }
        Effect::Delete { range } => {
            // Determine if the delete crosses a line boundary (contains '\n').
            // When document text is available, check directly; otherwise
            // conservatively assume line-crossing (safe for dd, etc.).
            // When document text is available, check whether the deleted slice
            // actually contains a newline.  When text is None (e.g. called via
            // sync_effect / sync_effect_with_text(None)), we conservatively
            // default to true so that mark invalidation is never skipped for
            // genuine line-crossing deletes.  Callers who have the document text
            // should use sync_effect_with_text to avoid false positives.
            let crosses_line = text
                .and_then(|t| t.get(range.start().get()..range.start().get() + range.len()))
                .is_none_or(|deleted| deleted.contains('\n'));
            // Invalidate named marks BEFORE handle_text_mutation so marks are
            // checked at their original (pre-adjustment) positions.  Only
            // invalidate when the delete crosses a line boundary (contains '\n'),
            // matching Neovim's line-based mark model: intra-line edits (like
            // `c{motion}` on a single line) preserve marks — the mark's line
            // still exists even though its content changed.  Cross-line deletes
            // may remove entire lines, invalidating marks on those lines.
            if crosses_line {
                state.marks_mut().invalidate_named_in_range(
                    range.start().get(),
                    range.start().get() + range.len(),
                );
            }
            handle_text_mutation(
                state,
                Offset::new(range.start().get()),
                range.len(),
                0,
                crosses_line,
                false,
                text,
                None, // Delete: no replacement text
            );
            if let Some(t) = text {
                state
                    .undo_tree_mut()
                    .mark_edit_range_with_text(range.start(), range.len(), t);
            } else {
                state
                    .undo_tree_mut()
                    .mark_edit_range(range.start(), range.len());
            }
            state.undo_tree_mut().accumulate_delta(range.len(), 0);
            EffectDisposition::Keep
        }
        Effect::Replace {
            range,
            text: replace_text,
        } => {
            // Line-crossing if old or new text contains a newline.
            let old_crosses = text
                .and_then(|t| t.get(range.start().get()..range.start().get() + range.len()))
                .is_none_or(|old| old.contains('\n'));
            let crosses_line = old_crosses || replace_text.contains('\n');
            handle_text_mutation(
                state,
                Offset::new(range.start().get()),
                range.len(),
                replace_text.len(),
                crosses_line,
                replace_text.ends_with('\n'),
                text,
                Some(replace_text),
            );
            if let Some(t) = text {
                state
                    .undo_tree_mut()
                    .mark_edit_range_with_text(range.start(), range.len(), t);
            } else {
                state
                    .undo_tree_mut()
                    .mark_edit_range(range.start(), range.len());
            }
            state.undo_tree_mut().mark_has_insert_or_replace();
            state
                .undo_tree_mut()
                .accumulate_delta(range.len(), replace_text.len());
            EffectDisposition::Keep
        }

        // ── Macro interception (removed from effects, handled by engine) ─
        Effect::PlayMacro { register, count } => {
            EffectDisposition::InterceptMacro(*register, *count)
        }

        // ── Message effects → store in VimState.message + history ──────
        Effect::ShowInfo { info } => {
            let text = match info {
                crate::effects::InfoMessage::Text(t) => t.clone(),
                crate::effects::InfoMessage::Verbose(t) => t.clone(),
                crate::effects::InfoMessage::LineReport(c) => {
                    compact_str::CompactString::from(format!("{} lines", c.total()))
                }
                crate::effects::InfoMessage::Categorized { text, .. } => text.clone(),
            };
            if !text.is_empty() {
                state.message_history_mut().push_info(text.as_str());
                state.set_message(crate::state::StatusMessage::Info(text));
            }
            EffectDisposition::Keep
        }
        Effect::Bell => EffectDisposition::Keep,
        Effect::ShowWarning { text } => {
            state.message_history_mut().push_warning(text.as_str());
            state.set_message(crate::state::StatusMessage::Info(text.clone()));
            EffectDisposition::Keep
        }
        Effect::ShowError { error, .. } => {
            let text = compact_str::CompactString::from(error.to_string());
            // push_error accepts &str, so pass a borrow and move the owned
            // copy into set_message — one allocation total instead of two.
            state.message_history_mut().push_error(text.as_str());
            state.set_message(crate::state::StatusMessage::Error(text));
            EffectDisposition::Keep
        }
        Effect::ClearMessage => {
            state.clear_message();
            EffectDisposition::Keep
        }

        // ── Scroll effects → store in VimState.scroll_hint ───────────
        Effect::ScrollTo { offset } => {
            state.set_scroll_hint(crate::state::ScrollHint::ToOffset(*offset));
            EffectDisposition::Keep
        }
        Effect::CenterCursor => {
            state.set_scroll_hint(crate::state::ScrollHint::CenterCursor);
            EffectDisposition::Keep
        }
        Effect::CursorToTop => {
            state.set_scroll_hint(crate::state::ScrollHint::CursorToTop);
            EffectDisposition::Keep
        }
        Effect::CursorToBottom => {
            state.set_scroll_hint(crate::state::ScrollHint::CursorToBottom);
            EffectDisposition::Keep
        }

        // ── Jumplist effects — sync to engine state, pass through to shell ──
        Effect::PushJumpList { offset } => {
            // Sync to engine's internal jump list (needed for process_click).
            // Tag with current buffer_id for cross-buffer navigation.
            let buffer_id = state.current_buffer_id();
            if let Some(t) = text {
                state.jump_list_mut().push_checked(*offset, buffer_id, |o| {
                    crate::commands::helpers::line_of(t, o.get())
                });
            } else {
                state.jump_list_mut().push(*offset, buffer_id);
            }
            // Any jump also updates the previous position mark (' and `)
            state.marks_mut().set_previous_position(*offset);
            EffectDisposition::Keep // shell also needs to see this
        }

        // ── Non-syncable effects — pass through to shell ─────────────────
        // When the selection is cleared while return_to targets Select mode,
        // clear return_to — the operator consumed the selection, so returning
        // to Select would leave an invalid state (Select with no selection).
        Effect::ClearSelection => {
            if matches!(state.return_to(), ReturnTo::Select(_)) {
                state.set_return_to(ReturnTo::None);
            }
            EffectDisposition::Keep
        }

        // ── Undo/Redo: navigate the tree (swapping mark snapshots) ──
        Effect::Undo {
            count,
            ref mut steps,
        } => {
            let mut last_change_marks: Option<(Option<Offset>, Option<Offset>)> = None;
            let mut last_mark_dot: Option<Offset> = None;
            for _ in 0..*count {
                let undone_node = state.undo_tree().current();
                if let Some(step) = state.undo_with_marks() {
                    last_change_marks = Some((step.change_mark_start(), step.change_mark_end()));
                    last_mark_dot = step.mark_dot();
                    steps.push(UndoNavStep {
                        node_id: undone_node,
                        cursors: SmallVec::from_slice(step.cursors()),
                    });
                    // Mirror Neovim: u_undoredo calls changed_lines → changed_common
                    // which updates the last changelist entry to the edit position
                    // (line-start, col=0). Use change_mark_start (= line-start of
                    // the first edit) as the edit position. Falls back to cursor
                    // when the undo group had no edit position tracked.
                    let edit_pos = step.change_mark_start().unwrap_or(step.cursor());
                    state.changelist_mut().update_last(edit_pos);
                    // Remap jumplist through the undo text change. The undo
                    // REVERSES the DO, so in the current (pre-undo) text the
                    // edited region spans (original_span + edit_delta) bytes.
                    // After undo it shrinks back to original_span bytes.
                    // When last_edit_end is None (pure insert), original_span = 0.
                    if let Some(start) = step.first_edit_offset() {
                        let delta = step.edit_delta();
                        if delta != 0 {
                            let original_span = step
                                .last_edit_end()
                                .map_or(0, |end| end.get().saturating_sub(start.get()));
                            let old_len = byte_delta::shift(original_span, i64::from(delta));
                            let new_len = original_span;
                            let cs_doc_len = start.get() + old_len + (1 << 20);
                            let changeset = crate::primitives::changeset::ChangeSet::from_edit_len(
                                cs_doc_len,
                                start.get(),
                                old_len,
                                new_len,
                            );
                            state.remap_all_positions(&changeset);
                        }
                    }
                    // Restore multi-cursor positions from the undo step's stored
                    // cursors (captured at begin_undo_group time). These are the
                    // correct pre-edit positions in the post-undo text. Using
                    // map_through on current positions would produce wrong results
                    // because the remap models a single contiguous edit region,
                    // not per-cursor edits.
                    if step.cursors().len() > 1 {
                        let restored: smallvec::SmallVec<[SelectionRange; 1]> = step
                            .cursors()
                            .iter()
                            .map(|&off| SelectionRange::new(off, off))
                            .collect();
                        let sels = Selections::from_vec(
                            restored.to_vec(),
                            state
                                .multi_cursor()
                                .selections()
                                .primary_index()
                                .min(step.cursors().len() - 1),
                        )
                        .normalize();
                        state.multi_cursor_mut().set_selections(sels);
                    }
                } else {
                    break;
                }
            }
            if let Some((Some(mark_start), end)) = last_change_marks {
                state
                    .marks_mut()
                    .set_change_region(mark_start, end.unwrap_or(mark_start));
                let dot = last_mark_dot.unwrap_or(mark_start);
                state.marks_mut().set_last_change(dot);
            }
            EffectDisposition::Keep
        }
        Effect::Redo {
            count,
            ref mut steps,
        } => {
            let mut last_change_marks: Option<(Option<Offset>, Option<Offset>)> = None;
            let mut last_mark_dot: Option<Offset> = None;
            for _ in 0..*count {
                if let Some(step) = state.redo_with_marks() {
                    last_change_marks = Some((step.change_mark_start(), step.change_mark_end()));
                    last_mark_dot = step.mark_dot();
                    steps.push(UndoNavStep {
                        node_id: step.node(),
                        cursors: SmallVec::from_slice(step.cursors()),
                    });
                    // Mirror Neovim: redo calls changed_lines → changed_common
                    // which updates the last changelist entry to the edit position.
                    let edit_pos = step.change_mark_start().unwrap_or(step.cursor());
                    state.changelist_mut().update_last(edit_pos);
                    // Remap jumplist through the redo text change. Redo
                    // re-applies the forward edit: original_span → original_span + delta.
                    // When last_edit_end is None (pure insert), original_span = 0.
                    if let Some(start) = step.first_edit_offset() {
                        let delta = step.edit_delta();
                        if delta != 0 {
                            let original_span = step
                                .last_edit_end()
                                .map_or(0, |end| end.get().saturating_sub(start.get()));
                            let old_len = original_span;
                            let new_len = byte_delta::shift(original_span, i64::from(delta));
                            let cs_doc_len = start.get() + old_len + (1 << 20);
                            let changeset = crate::primitives::changeset::ChangeSet::from_edit_len(
                                cs_doc_len,
                                start.get(),
                                old_len,
                                new_len,
                            );
                            state.remap_all_positions(&changeset);
                        }
                    }
                    // Restore multi-cursor positions from stored redo cursors.
                    if step.cursors().len() > 1 {
                        let restored: smallvec::SmallVec<[SelectionRange; 1]> = step
                            .cursors()
                            .iter()
                            .map(|&off| SelectionRange::new(off, off))
                            .collect();
                        let sels = Selections::from_vec(
                            restored.to_vec(),
                            state
                                .multi_cursor()
                                .selections()
                                .primary_index()
                                .min(step.cursors().len() - 1),
                        )
                        .normalize();
                        state.multi_cursor_mut().set_selections(sels);
                    }
                } else {
                    break;
                }
            }
            if let Some((Some(mark_start), end)) = last_change_marks {
                state
                    .marks_mut()
                    .set_change_region(mark_start, end.unwrap_or(mark_start));
                let dot = last_mark_dot.unwrap_or(mark_start);
                state.marks_mut().set_last_change(dot);
            }
            EffectDisposition::Keep
        }

        Effect::SetCursor { .. }
        | Effect::SetSelection { .. }
        | Effect::OperatorToMark { .. }
        | Effect::UndoLine { .. }
        | Effect::HighlightMatches { .. }
        | Effect::ClearHighlights
        | Effect::NormCommand { .. }
        | Effect::OperatorFilter { .. }
        | Effect::OperatorReindent { .. }
        | Effect::ScrollLeft { .. }
        | Effect::ScrollRight { .. }
        | Effect::ScrollHalfScreenLeft { .. }
        | Effect::ScrollHalfScreenRight { .. }
        | Effect::ScrollCursorToLeftEdge
        | Effect::ScrollCursorToRightEdge
        | Effect::CopyToClipboard { .. }
        | Effect::FoldLine { .. }
        | Effect::UnfoldLine { .. }
        | Effect::ToggleFold { .. }
        | Effect::ToggleFoldRecursive { .. }
        | Effect::FoldAll
        | Effect::UnfoldAll
        | Effect::OpenCommandWindow { .. }
        | Effect::CallOperatorFunc { .. }
        | Effect::Event { .. }
        | Effect::WindowSplit
        | Effect::WindowNew
        | Effect::WindowVSplit
        | Effect::WindowClose
        | Effect::WindowOnly
        | Effect::WindowNext
        | Effect::WindowPrev
        | Effect::WindowMoveLeft
        | Effect::WindowMoveRight
        | Effect::WindowMoveUp
        | Effect::WindowMoveDown
        | Effect::WindowEqualSize
        | Effect::WindowIncreaseHeight { .. }
        | Effect::WindowDecreaseHeight { .. }
        | Effect::WindowIncreaseWidth { .. }
        | Effect::WindowDecreaseWidth { .. }
        | Effect::WindowRotateDown
        | Effect::WindowRotateUp
        | Effect::FoldLineRecursive { .. }
        | Effect::UnfoldLineRecursive { .. }
        | Effect::DeleteFold { .. }
        | Effect::DeleteFoldRecursive { .. }
        | Effect::EliminateAllFolds
        | Effect::ToggleFoldEnable
        | Effect::SetFoldEnable { .. }
        | Effect::GotoDefinition
        | Effect::ShowDocumentation
        | Effect::HostAction { .. }
        | Effect::SetHighlightRange { .. }
        | Effect::ClearHighlightRange { .. }
        | Effect::SubstitutePreview { .. }
        | Effect::ClearSubstitutePreview
        | Effect::SetVirtualText { .. }
        | Effect::ClearVirtualText { .. }
        | Effect::SetDiagnostics { .. }
        | Effect::SyncFoldRanges { .. }
        | Effect::UndoTreeSnapshot { .. }
        | Effect::SetCursorStyle { .. }
        | Effect::CursorShapeHint { .. }
        | Effect::ShowMatch { .. }
        | Effect::Noop
        | Effect::HighlightRows { .. }
        | Effect::SetBlockSelections { .. }
        | Effect::SaveSelections { .. }
        | Effect::RestoreSelections { .. }
        | Effect::SelectNextMatch { .. }
        | Effect::SelectPreviousMatch { .. } => EffectDisposition::Keep,

        // ── Search match info (write cache, then pass through to host) ────────
        Effect::SearchMatchInfo {
            current,
            total,
            complete,
        } => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::hash::DefaultHasher::new();
            if let Some(pat) = state.search().pattern() {
                pat.hash(&mut hasher);
            }
            state.set_search_count_cache(crate::state::SearchCountCache {
                pattern_hash: hasher.finish(),
                current: *current,
                total: *total,
                complete: *complete,
            });
            EffectDisposition::Keep
        }

        // ── Register / mark clearing (sync state, then pass through to host) ─
        Effect::ClearNamedRegister { register } => {
            state.registers_mut().clear(*register);
            EffectDisposition::Keep
        }
        Effect::ClearMark { mark } => {
            state.marks_mut().delete(*mark);
            EffectDisposition::Keep
        }

        // ── Extension state (engine-internal, consumed) ───────────────
        Effect::SetExtState { .. } | Effect::ClearExtState { .. } => {
            // Extension state management: consume silently.
            EffectDisposition::Consume
        }

        // ── Syntax selection history (engine-internal, consumed) ─────
        Effect::SyntaxSelectionPush { snapshot } => {
            state.syntax_selection_mut().push(snapshot.clone());
            EffectDisposition::Consume
        }
        Effect::SyntaxSelectionPop => {
            state.syntax_selection_mut().pop();
            EffectDisposition::Consume
        }
        Effect::SyntaxHistoryClear => {
            state.syntax_selection_mut().clear();
            EffectDisposition::Consume
        }
        Effect::SetSyntaxSelections {
            selections: _selections,
        } => {
            {
                state.multi_cursor_mut().set_selections(_selections.clone());
            }
            EffectDisposition::Consume
        }
        Effect::SetScrollHalfCount { count } => {
            state.set_scroll_half_count(*count);
            EffectDisposition::Consume
        }
        Effect::SetStickyColumn { column } => {
            state.set_sticky_column(*column);
            EffectDisposition::Keep
        }

        // ── Substitute confirm state (engine-internal, consumed) ───
        Effect::SetSubstituteConfirmState { payload: ref p } => {
            let matches = p
                .matches
                .iter()
                .map(|m| crate::state::SubstituteConfirmMatch {
                    range: m.range,
                    line_idx: m.line_idx,
                    line_start: m.line_start,
                    line_text: m.line_text.clone(),
                })
                .collect();
            let confirm = crate::state::SubstituteConfirmState::new(
                matches,
                p.replacement.clone(),
                p.pattern.clone(),
                p.flags,
                p.gdefault,
            );
            state.set_substitute_confirm(confirm);
            EffectDisposition::Consume
        }
        Effect::ClearSubstituteConfirmState => {
            state.clear_substitute_confirm();
            EffectDisposition::Consume
        }

        // ── Substitute confirm UI effects (passed to host) ─────────
        Effect::SubstituteConfirmShow { .. } | Effect::SubstituteConfirmEnd => {
            EffectDisposition::Keep
        }

        // ── Variable store (engine-internal, consumed) ───────────
        Effect::SetVariable { scope, name, value } => {
            state.variable_store_mut().set(*scope, name, value.clone());
            EffectDisposition::Consume
        }
        Effect::DeleteVariable { scope, name } => {
            state.variable_store_mut().delete(*scope, name);
            EffectDisposition::Consume
        }

        // ── Cross-buffer edit (pass through to host) ─────────────
        Effect::CrossBufferEdit { .. } => EffectDisposition::Keep,

        // ── Atomic mode transition (sync + pass through) ────────
        Effect::ModeTransition { mode, .. } => {
            handle_set_mode(state, *mode);
            EffectDisposition::Keep
        }

        // ── Timer request (pass through to host) ────────────────
        Effect::RequestTimer { .. } => EffectDisposition::Keep,
    }
}

/// Sync a single effect to internal state (public API for `VimEngine::apply_effect()`).
///
/// Delegates to `process_one_effect()` — the single source of truth — and
/// discards the disposition since the caller only needs the state sync.
///
/// Note: because no document text is provided, `Effect::Delete` will conservatively
/// assume the deletion crosses a line boundary.  Callers who have the document text
/// available should use [`sync_effect_with_text`] instead to avoid false positives
/// in mark invalidation.
pub(crate) fn sync_effect(state: &mut VimState, parser: &mut Parser, effect: &Effect) {
    sync_effect_with_text(state, parser, effect, None);
}

/// Sync a single effect to internal state, optionally providing the document text.
///
/// When `doc_text` is `Some`, `Effect::Delete` ranges can be checked for actual
/// line-crossing, preventing false-positive mark invalidations for single-line
/// host-applied deletes.  When `doc_text` is `None` the behaviour is identical
/// to [`sync_effect`].
pub(crate) fn sync_effect_with_text(
    state: &mut VimState,
    parser: &mut Parser,
    effect: &Effect,
    doc_text: Option<&str>,
) {
    // process_one_effect needs &mut for write-back on 3 variants:
    // EndUndoGroup (node_id), Undo (steps), Redo (steps).
    // For all other variants, use sync_one_effect_readonly which takes
    // &Effect and avoids cloning large payloads (Insert/Replace text).
    match effect {
        Effect::EndUndoGroup { .. } | Effect::Undo { .. } | Effect::Redo { .. } => {
            let mut cloned = effect.clone();
            let _ = process_one_effect(&mut cloned, state, parser, false, doc_text, None);
        }
        _ => {
            sync_one_effect_readonly(effect, state, parser, doc_text);
        }
    }
}

/// Read-only state sync for a single effect, avoiding clone.
///
/// Handles all variants that `process_one_effect` handles EXCEPT the three
/// write-back variants (`EndUndoGroup`, `Undo`, `Redo`) which mutate the
/// effect in place. Those must go through `process_one_effect` with a clone.
///
/// This is the hot path for `sync_effect`/`sync_effect_with_text` — called
/// for every effect in ex-command and public API sync. Avoids cloning large
/// `Insert`/`Replace` text payloads.
fn sync_one_effect_readonly(
    effect: &Effect,
    state: &mut VimState,
    parser: &mut Parser,
    text: Option<&str>,
) {
    match effect {
        // ── Effects that require internal state synchronization ──────────
        Effect::SetMode { mode, .. } => {
            handle_set_mode(state, *mode);
        }
        Effect::BeginInsert { .. } => {
            sync_begin_insert(state, effect);
            state.set_mode(Mode::Insert);
            state.set_return_to(ReturnTo::None);
        }
        Effect::SetBlockInsert {
            lines_below,
            grapheme_col,
            cursor_return_offset,
        } => {
            handle_set_block_insert(state, *lines_below, *grapheme_col, *cursor_return_offset);
        }
        Effect::SetRegister {
            name,
            text: reg_text,
            motion_type,
        } => {
            handle_set_register(state, *name, reg_text, *motion_type);
        }
        Effect::SetLastFind {
            direction,
            target_char,
            sneak_c2,
            resolved_ignorecase,
            resolved_smartcase,
        } => {
            use crate::primitives::LastFind;
            let mut last_find = LastFind::new();
            if let Some(c2) = sneak_c2 {
                last_find.record_sneak_with_case(
                    *direction,
                    *target_char,
                    *c2,
                    *resolved_ignorecase,
                    *resolved_smartcase,
                );
            } else {
                last_find.record_with_case(
                    *direction,
                    *target_char,
                    *resolved_ignorecase,
                    *resolved_smartcase,
                );
            }
            state.set_last_find(last_find);
        }
        Effect::SetLastSubstitute { replacement } => {
            state.search_mut().set_last_substitute(replacement);
        }
        Effect::SetLastSubstituteFlags { flags } => {
            state.search_mut().set_last_substitute_flags(*flags);
        }
        Effect::SetSubstitutePattern { pattern } => {
            state.search_mut().set_substitute_pattern(pattern.as_str());
        }
        Effect::SetSearchPattern { pattern, direction } => {
            state.search_mut().set_pattern(
                pattern.as_str(),
                crate::primitives::SearchDirection::from(*direction),
            );
        }
        Effect::StartRecording { register } => {
            parser.set_recording(Some(*register));
            state.macros_mut().start_recording(*register);
        }
        Effect::StopRecording => {
            parser.set_recording(None);
            state.macros_mut().stop_recording();
        }
        Effect::SaveLastVisual { info } => {
            state.set_last_visual(*info);
        }
        Effect::SetMark {
            name,
            offset,
            topline_offset,
        } => {
            state.marks_mut().set(
                *name,
                crate::primitives::Mark::with_topline_offset(*offset, *topline_offset),
            );
            // Keep changelist in lockstep with mark `.` (see process_one_effect).
            if *name == crate::primitives::MarkName::LAST_CHANGE {
                state.changelist_mut().update_last(*offset);
            }
        }
        Effect::ChangelistOlder { count } => {
            for _ in 0..*count {
                state.changelist_mut().older();
            }
        }
        Effect::ChangelistNewer { count } => {
            for _ in 0..*count {
                state.changelist_mut().newer();
            }
        }
        Effect::JumpOlder { count } => {
            state.jump_list_mut().cleanup(None, offset_to_line_fn(text));
            for _ in 0..*count {
                state.jump_list_mut().older();
            }
        }
        Effect::JumpNewer { count } => {
            state.jump_list_mut().cleanup(None, offset_to_line_fn(text));
            for _ in 0..*count {
                state.jump_list_mut().newer();
            }
        }
        Effect::JumpToBuffer { .. } => {}
        Effect::BeginUndoGroup { cursor_strategy } => {
            // Note: double-open can happen legitimately (multi-cursor, macro replay).
            // The undo tree handles it by committing the pending group first.
            state.set_last_change_mark_set(false);
            state.set_change_start_mark_set(false);
            state.set_changelist_new_change(true);
            let marks_snapshot = MarkSnapshot::capture(state.marks());
            let cursor_offsets: SmallVec<[Offset; 1]> = {
                let sels = state.multi_cursor().selections();
                if sels.len() <= 1 {
                    smallvec::smallvec![state.undo_cursor_hint()]
                } else {
                    sels.iter().map(|r| r.head()).collect()
                }
            };
            let mode = state.mode();
            let last_visual = state.last_visual();
            state.undo_tree_mut().begin_group_multi(
                &cursor_offsets,
                *cursor_strategy,
                marks_snapshot,
                text.map(str::len),
                mode,
                last_visual,
                false,
            );
        }
        Effect::CommandLineEdit(edit) => {
            state.command_line_mut().apply_edit(*edit);
        }
        Effect::Insert {
            offset,
            text: insert_text,
        } => {
            let is_insert_zero_or_caret = insert_text.len() == 1
                && state.mode().is_insert()
                && insert_text
                    .as_bytes()
                    .first()
                    .is_some_and(|&b| b == b'0' || b == b'^');
            if is_insert_zero_or_caret {
                state.set_last_change_mark_set(false);
            }
            let crosses_line = insert_text.contains('\n');
            handle_text_mutation(
                state,
                *offset,
                0,
                insert_text.len(),
                crosses_line,
                insert_text.ends_with('\n'),
                text,
                Some(insert_text),
            );
            if let Some(t) = text {
                state.undo_tree_mut().mark_edit_at_with_text(*offset, t);
            } else {
                state.undo_tree_mut().mark_edit_at(*offset);
            }
            state.undo_tree_mut().mark_has_insert_or_replace();
            state.undo_tree_mut().accumulate_delta(0, insert_text.len());
            if is_insert_zero_or_caret {
                state.set_last_change_mark_set(false);
            }
        }
        Effect::Delete { range } => {
            let crosses_line = text
                .and_then(|t| t.get(range.start().get()..range.start().get() + range.len()))
                .is_none_or(|deleted| deleted.contains('\n'));
            handle_text_mutation(
                state,
                Offset::new(range.start().get()),
                range.len(),
                0,
                crosses_line,
                false,
                text,
                None,
            );
            state
                .marks_mut()
                .invalidate_named_in_range(range.start().get(), range.start().get() + range.len());
            if let Some(t) = text {
                state
                    .undo_tree_mut()
                    .mark_edit_range_with_text(range.start(), range.len(), t);
            } else {
                state
                    .undo_tree_mut()
                    .mark_edit_range(range.start(), range.len());
            }
            state.undo_tree_mut().accumulate_delta(range.len(), 0);
        }
        Effect::Replace {
            range,
            text: replace_text,
        } => {
            let old_crosses = text
                .and_then(|t| t.get(range.start().get()..range.start().get() + range.len()))
                .is_none_or(|old| old.contains('\n'));
            let crosses_line = old_crosses || replace_text.contains('\n');
            handle_text_mutation(
                state,
                Offset::new(range.start().get()),
                range.len(),
                replace_text.len(),
                crosses_line,
                replace_text.ends_with('\n'),
                text,
                Some(replace_text),
            );
            if let Some(t) = text {
                state
                    .undo_tree_mut()
                    .mark_edit_range_with_text(range.start(), range.len(), t);
            } else {
                state
                    .undo_tree_mut()
                    .mark_edit_range(range.start(), range.len());
            }
            state.undo_tree_mut().mark_has_insert_or_replace();
            state
                .undo_tree_mut()
                .accumulate_delta(range.len(), replace_text.len());
        }
        Effect::ShowInfo { info } => {
            let text = match info {
                crate::effects::InfoMessage::Text(t) => t.clone(),
                crate::effects::InfoMessage::Verbose(t) => t.clone(),
                crate::effects::InfoMessage::LineReport(c) => {
                    compact_str::CompactString::from(format!("{} lines", c.total()))
                }
                crate::effects::InfoMessage::Categorized { text, .. } => text.clone(),
            };
            if !text.is_empty() {
                state.message_history_mut().push_info(text.as_str());
                state.set_message(crate::state::StatusMessage::Info(text));
            }
        }
        Effect::Bell => {}
        Effect::ShowWarning { text: msg_text } => {
            state.message_history_mut().push_warning(msg_text.as_str());
            state.set_message(crate::state::StatusMessage::Info(msg_text.clone()));
        }
        Effect::ShowError { error, .. } => {
            let err_text = compact_str::CompactString::from(error.to_string());
            state.message_history_mut().push_error(err_text.as_str());
            state.set_message(crate::state::StatusMessage::Error(err_text));
        }
        Effect::ClearMessage => {
            state.clear_message();
        }
        Effect::ScrollTo { offset } => {
            state.set_scroll_hint(crate::state::ScrollHint::ToOffset(*offset));
        }
        Effect::CenterCursor => {
            state.set_scroll_hint(crate::state::ScrollHint::CenterCursor);
        }
        Effect::CursorToTop => {
            state.set_scroll_hint(crate::state::ScrollHint::CursorToTop);
        }
        Effect::CursorToBottom => {
            state.set_scroll_hint(crate::state::ScrollHint::CursorToBottom);
        }
        Effect::PushJumpList { offset } => {
            let buffer_id = state.current_buffer_id();
            if let Some(t) = text {
                state.jump_list_mut().push_checked(*offset, buffer_id, |o| {
                    crate::commands::helpers::line_of(t, o.get())
                });
            } else {
                state.jump_list_mut().push(*offset, buffer_id);
            }
            state.marks_mut().set_previous_position(*offset);
        }
        Effect::ClearSelection => {
            if matches!(state.return_to(), ReturnTo::Select(_)) {
                state.set_return_to(ReturnTo::None);
            }
        }
        Effect::ClearNamedRegister { register } => {
            state.registers_mut().clear(*register);
        }
        Effect::ClearMark { mark } => {
            state.marks_mut().delete(*mark);
        }
        Effect::SyntaxSelectionPush { snapshot } => {
            state.syntax_selection_mut().push(snapshot.clone());
        }
        Effect::SyntaxSelectionPop => {
            state.syntax_selection_mut().pop();
        }
        Effect::SyntaxHistoryClear => {
            state.syntax_selection_mut().clear();
        }
        Effect::SetSyntaxSelections {
            selections: _selections,
        } => {
            state.multi_cursor_mut().set_selections(_selections.clone());
        }
        Effect::SetScrollHalfCount { count } => {
            state.set_scroll_half_count(*count);
        }
        Effect::SetStickyColumn { column } => {
            state.set_sticky_column(*column);
        }
        Effect::SetVariable { scope, name, value } => {
            state.variable_store_mut().set(*scope, name, value.clone());
        }
        Effect::DeleteVariable { scope, name } => {
            state.variable_store_mut().delete(*scope, name);
        }
        // SetCursor: track hint (same as process_one_effect)
        Effect::SetCursor { offset } => {
            state.set_undo_cursor_hint(*offset);
        }
        // Write-back variants handled by caller via clone + process_one_effect.
        // They should never reach here.
        Effect::EndUndoGroup { .. } | Effect::Undo { .. } | Effect::Redo { .. } => {
            debug_assert!(false, "write-back variants must use process_one_effect");
        }
        // All other variants: no state sync needed (pass-through to host).
        _ => {}
    }
}

/// Sync a batch of effects to state without interception.
///
/// Unlike [`process_effects`], this does NOT intercept dot-repeat,
/// PlayMacro, or recording signals. Used by ex-command and
/// host-completion paths where interception is unwanted.
pub(crate) fn sync_effects(state: &mut VimState, parser: &mut Parser, effects: &mut [Effect]) {
    sync_effects_with_text(state, parser, effects, None);
}

/// Sync a batch of effects to internal state, sharing changelist state.
///
/// Unlike calling [`sync_effect`] in a loop, this shares the
/// `changelist_new_change` flag on VimState across all effects so that
/// multiple text mutations (e.g. from `:g/pat/d` or `:%s/pat/rep/g`)
/// coalesce into one changelist entry per undo group — matching Neovim's
/// `b_new_change` / `changed_common` behavior.
///
/// `BeginUndoGroup` effects set `changelist_new_change = true` (via
/// `sync_one_effect_readonly`), mirroring Neovim's `u_savecommon`.
pub(crate) fn sync_effects_with_text(
    state: &mut VimState,
    parser: &mut Parser,
    effects: &mut [Effect],
    doc_text: Option<&str>,
) {
    for effect in effects.iter_mut() {
        match effect {
            Effect::EndUndoGroup { .. } | Effect::Undo { .. } | Effect::Redo { .. } => {
                let _ = process_one_effect(effect, state, parser, false, doc_text, None);
            }
            _ => {
                sync_one_effect_readonly(effect, state, parser, doc_text);
            }
        }
    }
}

/// Shared logic for Insert/Delete/Replace text mutations.
///
/// Adjusts changelist offsets, mark offsets, and named mark offsets,
/// then pushes to changelist (all modes — mirroring Neovim's `changed_common`)
/// and syncs change marks (`[`, `]`, `.`).
///
/// Delete has one extra step (invalidate named marks in range) that
/// must be done by the caller after this function returns.
#[expect(
    clippy::too_many_arguments,
    reason = "internal helper that funnels every per-mutation side-channel state update; restructuring into a struct would force callers to allocate it and would not improve clarity"
)]
fn handle_text_mutation(
    state: &mut VimState,
    offset: Offset,
    old_len: usize,
    new_len: usize,
    crosses_line: bool,
    new_text_ends_with_newline: bool,
    text: Option<&str>,
    new_text_snippet: Option<&str>,
) {
    // Invalidate search count cache — text changed, match positions are stale.
    state.invalidate_search_count_cache();

    // Track that text was mutated during insert mode so the exit handler
    // can distinguish arrow-only sessions from deletion-only sessions.
    if let Some(is) = state.insert_state_mut() {
        is.set_had_text_mutation();
    }
    // Use a generous upper-bound for doc_len to avoid the ChangeSet map_pos
    // fast-path where `pos >= input_len` returns `output_len`.
    let cs_doc_len = offset.get() + old_len + (1 << 20);
    let changeset = crate::primitives::changeset::ChangeSet::from_edit_len(
        cs_doc_len,
        offset.get(),
        old_len,
        new_len,
    );
    state.remap_all_positions(&changeset);
    // Neovim stores mark.^ as (line, col) and only adjusts the line
    // number via ONE_ADJUST when lines are added/removed.  mark_col_adjust
    // is NOT called for indent/set_indent.  In our byte-offset model:
    //   - Edits entirely before mark.^'s line: shift by delta.
    //   - Edits on the same line: no change (column stays the same).
    //   - Edits that delete mark.^'s line: clamp to edit start.
    //
    // Note: `text` may be stale when multiple Replace effects are processed
    // in sequence (e.g., :%s). The mark offset is updated by prior effects,
    // but `text` still holds the original document. For same-line detection,
    // we clamp positions to text boundaries and tolerate coordinate mismatches.
    if let Some(mark) = state.marks().get(crate::primitives::MarkName::INSERT_STOP) {
        let delta = byte_delta::delta(new_len, old_len);
        let mark_off = mark.offset().get();
        if delta != 0 && mark_off >= offset.get() {
            let mark_line_start = text
                .and_then(|t| {
                    let c = mark_off.min(t.len());
                    t.get(..c).and_then(|s| s.rfind('\n').map(|i| i + 1))
                })
                .unwrap_or(0);
            let edit_end = offset.get() + old_len;
            if edit_end <= mark_line_start {
                // Edit entirely before mark's line: shift by delta.
                let new_off = mark_off.saturating_add_signed(delta);
                state
                    .marks_mut()
                    .set_insert_stop(crate::primitives::Offset::new(new_off));
            } else if offset.get() >= mark_line_start {
                // Edit on same line as mark (or text is stale and coordinates
                // overlap): preserve the mark. Only clear if the line is truly
                // deleted (old_len > 0, new_len == 0) and the entire line is covered.
                let mark_line_len = text
                    .and_then(|t| t.get(mark_line_start..)?.find('\n').map(|i| i + 1))
                    .unwrap_or_else(|| text.map_or(0, |t| t.len().saturating_sub(mark_line_start)));
                if old_len > 0
                    && new_len == 0
                    && offset.get() <= mark_line_start
                    && edit_end >= mark_line_start + mark_line_len
                {
                    state.marks_mut().clear_insert_stop();
                }
                // Otherwise: edit on same line, no column adjustment.
            } else {
                // Cross-line edit or mark's line is after edit region:
                // shift by delta. This handles both edits entirely before
                // the mark's line and edits that overlap but don't delete it.
                let new_off = mark_off.saturating_add_signed(delta);
                state
                    .marks_mut()
                    .set_insert_stop(crate::primitives::Offset::new(new_off));
            }
        }
    }
    let edit_line_end = text
        .and_then(|t| {
            let p = offset.get().min(t.len());
            match t.get(p..)?.find('\n') {
                Some(i) => Some(p + i),
                None if crosses_line => {
                    // Cross-line insert on the last line (no trailing \n):
                    // use text.len() as the line boundary so marks on this
                    // line are correctly identified as "same line" and skipped.
                    Some(t.len())
                }
                None => None,
            }
        })
        .unwrap_or(if crosses_line { 0 } else { usize::MAX });
    // For pure cross-line inserts (e.g. linewise paste P), Neovim's
    // mark_adjust increments line numbers for ALL marks at and below the
    // insertion line. In our byte-offset model, the same-line skip in
    // adjust_named_offsets must be suppressed so marks at the insertion
    // point get shifted forward by the inserted bytes.
    let skip_same_line = !(old_len == 0 && crosses_line);
    state.marks_mut().adjust_named_offsets_ext(
        offset.get(),
        old_len,
        new_len,
        edit_line_end,
        skip_same_line,
    );
    adjust_visual_marks_cross_line(state, offset.get(), old_len, new_len, edit_line_end, text);
    state.syntax_selection_mut().clear();
    // Neovim does NOT set b_new_change during continuous insert-mode typing,
    // including cross-line edits (Enter).  In Neovim, b_new_change is only
    // set by u_savecommon() which is gated by ins_need_undo / arrow_used in
    // stop_arrow().  During a single uninterrupted insert session, stop_arrow()
    // calls u_save_cursor() only ONCE (the first edit) and then sets
    // ins_need_undo=false.  Subsequent Enters call stop_arrow() but it's a
    // no-op since ins_need_undo=false and arrow_used=false.  As a result,
    // the entire insert session produces at most ONE changelist entry that
    // gets updated in-place by the "ALWAYS update last entry" path in
    // changed_common().  Our BeginUndoGroup effect already correctly sets
    // changelist_new_change=true on session start; no further overrides
    // are needed for cross-line edits within the session.
    //
    // Mirror Neovim: changed_common runs for ALL modes (insert, replace,
    // normal, visual). The new_change flag (b_new_change) controls whether
    // a new entry is created vs just updating the last one.
    changelist_push(state, offset, text);
    sync_change_marks(
        state,
        offset,
        old_len,
        new_len,
        new_text_ends_with_newline,
        new_text_snippet,
    );
}

/// Push to changelist with line-based dedup.
///
/// Neovim merges changelist entries on the same line. When `text` is available,
/// compute line numbers and update in-place if lines match.
/// Adjust visual marks (`<`, `>`) for cross-line text edits.
///
/// Mirrors Neovim's `ONE_ADJUST_NODEL` semantics for `b_visual.vi_start/vi_end`:
/// - Same-length replaces (delta == 0): no adjustment (case toggle, etc.)
/// - Marks on the same line as the edit: no adjustment (intra-line edits)
/// - Marks inside a deleted range: move to `line1` preserving column (NODEL)
/// - Marks after the edit: shift by delta
fn adjust_visual_marks_cross_line(
    state: &mut VimState,
    pos: usize,
    old_len: usize,
    new_len: usize,
    edit_line_end: usize,
    text: Option<&str>,
) {
    use crate::primitives::Mark;
    use crate::primitives::MarkName;
    let delta = byte_delta::delta(new_len, old_len);
    // No adjustment needed when the edit doesn't change the document length
    // (e.g. case toggle via ~, gU, gu). Neovim's mark_adjust_buf is only
    // called when lines are added/removed, and mark_col_adjust only shifts
    // columns on the same line. A same-length replace (delta == 0) never
    // triggers either, so visual marks must stay put.
    if delta == 0 {
        return;
    }
    for name in [MarkName::VISUAL_START, MarkName::VISUAL_END] {
        if let Some(mark) = state.marks().get(name) {
            let val = mark.offset().get();
            if val <= edit_line_end {
                continue;
            }
            let adjusted = if val < pos {
                val
            } else if old_len > 0 && val < pos + old_len {
                // Mark is inside deleted range. Neovim's ONE_ADJUST_NODEL moves
                // the mark to line1 (the first deleted line) preserving its column.
                // In byte-offset terms: line_start_of(pos) + column_of(val).
                if let Some(t) = text {
                    let line_start_pos = t[..pos].rfind('\n').map_or(0, |i| i + 1);
                    let mark_line_start = t[..val.min(t.len())].rfind('\n').map_or(0, |i| i + 1);
                    let col = val - mark_line_start;
                    line_start_pos + col
                } else {
                    pos
                }
            } else {
                val.saturating_add_signed(delta)
            };
            if adjusted != val {
                state.marks_mut().set(
                    name,
                    Mark::with_topline_offset(Offset::new(adjusted), mark.topline_offset()),
                );
            }
        }
    }
}

/// Mirror Neovim's `changed_common()` changelist logic.
///
/// Neovim's algorithm (change.c lines 260-318):
/// 1. ALWAYS update `b_last_change` to the current edit position.
/// 2. If `b_new_change` (start of new undo group) or changelist is empty:
///    a. Check line-based dedup: skip new entry if same line and close column.
///    b. If `add`: create a new entry, set `b_new_change = false`.
/// 3. ALWAYS overwrite the last changelist entry with `b_last_change`.
///
/// **Insert/replace mode caveat**: Neovim batches ASCII chars into a single
/// `ins_str()` call, so `changed_common` is called once per batch (with the
/// batch-start column). Our architecture processes each keystroke individually.
/// To avoid the changelist drifting to the end of the typed text, we skip
/// the "always update last" step during insert/replace mode when no new entry
/// was created. This preserves the batch-start offset, matching Neovim.
fn changelist_push(state: &mut VimState, offset: Offset, text: Option<&str>) {
    let new_change = state.changelist_new_change();
    let cl_empty = state.changelist().is_empty();

    if new_change || cl_empty {
        // Potentially create a new entry.
        let add = if cl_empty {
            true
        } else if let Some(text) = text {
            // Line-based dedup: don't create a new entry when on the same line
            // and column is close (Neovim uses textwidth, we use same-line check).
            let last_offset = state.changelist().peek_last().unwrap();
            let last_line =
                crate::commands::helpers::line_of(text, last_offset.get().min(text.len()));
            let new_line = crate::commands::helpers::line_of(text, offset.get().min(text.len()));
            last_line != new_line
        } else {
            // No text available — always add for safety.
            true
        };

        if add {
            state.set_changelist_new_change(false);
            state.changelist_mut().push(offset);
        } else {
            state.set_changelist_new_change(false);
        }

        // Neovim line 313: ALWAYS update the last entry to current position.
        // This runs whether or not a new entry was created. It corresponds
        // to the first `changed_common` call in the new undo group.
        state.changelist_mut().update_last(offset);
    }

    // b_new_change is false and changelist is non-empty.
    // In insert/replace mode: skip (emulates batching — Neovim's
    // ins_str batches ASCII chars so changed_common runs once per batch).
    // In normal mode: also skip. Within a single undo group, Neovim's
    // `:s` only calls changed_common once at the end. Our per-effect
    // calls would drift the entry. Commands with separate undo groups
    // (like `xp`) are handled by the new_change=true path above.
}

/// Handle SetMode effect: start insert session if entering Insert mode
/// and no `BeginInsert` effect has already created one.
fn handle_set_mode(state: &mut VimState, mode: Mode) {
    if mode == Mode::Insert && state.insert_state().is_none() {
        use crate::primitives::InsertEntryType;
        // Fallback: BeforeCursor is the safest default (matches `i`).
        // Normal insert entry uses the BeginInsert effect which carries
        // the correct entry type; this path is only for edge cases where
        // SetMode arrives without a preceding BeginInsert.
        state.start_insert(crate::state::InsertState::new(
            InsertEntryType::BeforeCursor,
        ));
    }
    state.set_mode(mode);
}

/// Handle BeginInsert effect: create InsertState with count, auto-indent, and entry offset.
fn sync_begin_insert(state: &mut VimState, effect: &Effect) {
    use crate::state::InsertState;
    let Effect::BeginInsert {
        entry_type,
        count,
        auto_indent_len,
        entry_offset,
    } = effect
    else {
        return;
    };
    let insert_count = std::num::NonZeroU32::new(*count).unwrap_or(std::num::NonZeroU32::MIN);
    let mut insert_state = InsertState::with_count(*entry_type, insert_count);
    insert_state.set_auto_indent_len(*auto_indent_len);
    insert_state.set_entry_offset(*entry_offset);
    state.start_insert(insert_state);
}

/// Handle SetBlockInsert effect.
const fn handle_set_block_insert(
    state: &mut VimState,
    lines_below: usize,
    grapheme_col: usize,
    cursor_return_offset: Offset,
) {
    use crate::state::BlockInsertContext;
    if let Some(insert_state) = state.insert_state_mut() {
        insert_state.set_block_insert(BlockInsertContext::new(
            lines_below,
            grapheme_col,
            cursor_return_offset,
        ));
    }
}

/// Handle SetRegister effect with delete-register routing.
fn handle_set_register(
    state: &mut VimState,
    name: RegisterName,
    text: &compact_str::CompactString,
    motion_type: crate::primitives::MotionType,
) {
    use crate::primitives::RegisterContent;
    let content = RegisterContent::new(text.clone(), motion_type);

    // The `.` register (LAST_INSERT) is read-only in the register store,
    // so route it to the dedicated VimState field instead.
    if name == RegisterName::LAST_INSERT {
        state.store_last_inserted_text(text);
        return;
    }

    if name == RegisterName::NUMBERED_1 {
        // Shift numbered registers 1→2→...→9 unconditionally.
        // The caller explicitly requested NUMBERED_1 (e.g., search/jump
        // deletes that are sub-line but still shift the numbered chain).
        // Force LineWise routing so on_delete always calls shift_numbered,
        // regardless of the actual motion_type. The content's own
        // motion_type (embedded in RegisterContent) is preserved.
        state
            .registers_mut()
            .on_delete(content, crate::primitives::MotionType::LineWise);
    } else if name == RegisterName::SMALL_DELETE {
        // Small delete register
        state.registers_mut().on_delete(content, motion_type);
    } else {
        state.registers_mut().set(name, content);
    }
}

/// Sync change-related marks (`[`, `]`, `.`) after any text edit.
///
/// Handles insert, delete, and replace uniformly:
/// - `[` and `]` marks bracket the changed region.
///   The `]` mark is **always inclusive** — it points to the last byte of
///   the changed/inserted region (`start + inserted_len - 1`), matching
///   Neovim's behavior for all contexts (normal-mode operators, actions
///   like `p`, and insert-mode typing).
///   For linewise operators whose replacement ends with `\n` (e.g. `gUU`),
///   `]` skips the trailing newline: `start + inserted_len - 2`.
///   For deletes (`inserted_len == 0`), both marks point to `start`.
/// - `.` mark records the start of the FIRST edit in the undo group (Neovim
///   behavior); subsequent edits within the same undo group don't overwrite it.
fn sync_change_marks(
    state: &mut VimState,
    start: Offset,
    old_len: usize,
    inserted_len: usize,
    newline_terminated: bool,
    new_text: Option<&str>,
) {
    let end = start.saturating_add_raw(inserted_len);
    // `]` mark end calculation based on Neovim truth table:
    //   Delete  (ins=0):            `]` = start
    //   Insert  (old=0, ins>0):     `]` = start + ins_len (exclusive)
    //   Insert  (old=0, NL-term):   `]` = start + ins_len - 2 (skip trailing \n, then
    //                                     back to start of prev char)
    //   Replace (old>0, ins>0):     `]` = start of last char in replacement
    //   Replace (old>0, NL-term):   `]` = start of char before trailing \n
    //
    // In Neovim, marks are (line, col) where col is a byte offset into the
    // line. Mark `]` always points to the FIRST BYTE of the last significant
    // character. For ASCII this equals `end - 1`, but for multi-byte chars
    // we must skip back by the full char width using the actual text.
    let mark_end = if inserted_len == 0 {
        // Delete: both marks point to start
        start
    } else if let Some(new_str) = new_text {
        // We have the actual replacement text — use it for precise char-aware
        // mark computation.
        if old_len > 0 {
            // Replace: point to start of last char in replacement.
            // For NL-terminated, point to start of char before trailing \n.
            let effective = if newline_terminated {
                new_str.trim_end_matches('\n')
            } else {
                new_str
            };
            if effective.is_empty() {
                start
            } else {
                let last_char_start =
                    effective.len() - effective.chars().next_back().unwrap().len_utf8();
                start.saturating_add_raw(last_char_start)
            }
        } else {
            // Insert: exclusive end, but for NL-terminated skip trailing \n
            // and point to start of the preceding char.
            if newline_terminated && new_str.len() >= 2 {
                let before_nl = &new_str[..new_str.len() - 1]; // strip trailing \n
                if before_nl.is_empty() {
                    start
                } else {
                    let last_char_start =
                        before_nl.len() - before_nl.chars().next_back().unwrap().len_utf8();
                    start.saturating_add_raw(last_char_start)
                }
            } else {
                end
            }
        }
    } else {
        // Fallback: no text available, use byte arithmetic (correct for ASCII).
        if old_len > 0 {
            if newline_terminated && inserted_len >= 2 {
                end.saturating_sub_raw(2)
            } else {
                end.saturating_sub_raw(1)
            }
        } else if newline_terminated && inserted_len >= 2 {
            end.saturating_sub_raw(2)
        } else {
            end
        }
    };
    // `[` mark = start of FIRST edit in undo group (Neovim behavior).
    // `]` mark = end of LAST edit in undo group — always updated.
    #[expect(
        clippy::if_not_else,
        reason = "sets the FIRST-edit marks in the negative case and adjusts the LAST-edit mark in the else; reversing would need negating the condition and rewriting both branches, hurting readability"
    )]
    if !state.change_start_mark_set() {
        state.marks_mut().set_change_region(start, mark_end);
        state.set_change_start_mark_set(true);
    } else {
        state.marks_mut().set(
            crate::primitives::MarkName::CHANGE_END,
            crate::primitives::Mark::new(mark_end),
        );
    }
    // `.` mark — Neovim's `changed_common()` always updates mark '.' to
    // the position of each text change.  The last `changed_bytes()` call
    // determines the final value.  We replicate this by always updating.
    // Insert mode overrides mark '.' at exit via `exit_finalize` to
    // account for Neovim's ASCII-batching optimisation in `insertchar()`.
    // Operators that need explicit mark '.' positions use SetMark effects.
    state.marks_mut().set_last_change(start);
    state.set_last_change_mark_set(true);
}

/// Sync a single effect to engine state in-place, including node_id write-back.
/// Uses `process_one_effect` — the single source of truth for state sync.
pub(crate) fn sync_effect_mut(
    state: &mut VimState,
    parser: &mut Parser,
    effect: &mut Effect,
    text: Option<&str>,
    undolevels_max: Option<usize>,
) {
    let _ = process_one_effect(effect, state, parser, false, text, undolevels_max);
}

/// Compute the mark `'.'` offset for dot-repeated insert text, matching
/// Neovim's ASCII-batching behaviour in `insertchar()`.
///
/// `text` is the (possibly repeated) insert text.
/// `insert_pos` is the document byte offset where the text was inserted.
///
/// Returns the byte offset for mark `'.'`.
fn compute_mark_dot_for_repeat(text: &str, insert_pos: usize) -> usize {
    if text.is_empty() {
        return insert_pos;
    }
    let bytes = text.as_bytes();
    let mut pos = bytes.len();
    // Walk backward over ASCII bytes (stop at newlines — Neovim's
    // `insertchar()` lookahead stops at special chars including newlines)
    while pos > 0 && bytes[pos - 1] < 0x80 && bytes[pos - 1] != b'\n' {
        pos -= 1;
    }
    // If we didn't move (last byte >= 0x80), find the start of the
    // last multi-byte character.
    if pos == bytes.len() {
        pos -= 1;
        while pos > 0 && bytes[pos] & 0xC0 == 0x80 {
            pos -= 1;
        }
    }
    insert_pos + pos
}

/// Recompute indent for dot-repeat text. For each `\n` with recorded
/// auto-indent bytes, strips the original indent and replaces with
/// context-appropriate indent from the provider.
fn reindent_for_repeat(
    saved_text: &str,
    indent_lens: &[usize],
    indent_provider: Option<&dyn crate::document::IndentProvider>,
    doc_text: &str,
    insert_pos: usize,
    _tabstop: usize,
    autoindent: bool,
) -> compact_str::CompactString {
    let mut result = String::with_capacity(saved_text.len());
    let mut newline_idx = 0;
    let mut chars = saved_text.chars();

    while let Some(c) = chars.next() {
        if c == '\n' {
            result.push('\n');

            let orig_indent_len = indent_lens.get(newline_idx).copied().unwrap_or(0);

            // Compute new indent from provider or autoindent fallback
            let new_indent: compact_str::CompactString = if let Some(provider) = indent_provider {
                let line = crate::commands::helpers::line_of(doc_text, insert_pos);
                provider
                    .indent_for_new_line(crate::primitives::LineNumber::from(line))
                    .indent()
                    .into()
            } else if autoindent {
                // Fallback: copy current line's leading whitespace
                let ls = crate::commands::helpers::line_start_for_offset(doc_text, insert_pos);
                let line_content = &doc_text[ls..];
                let ws_len =
                    line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
                compact_str::CompactString::from(&doc_text[ls..ls + ws_len])
            } else {
                compact_str::CompactString::default()
            };

            result.push_str(&new_indent);

            // Skip original indent characters (count by chars, matching byte length)
            let mut skipped_bytes = 0;
            while skipped_bytes < orig_indent_len {
                match chars.clone().next() {
                    Some(ch) if ch == ' ' || ch == '\t' => {
                        skipped_bytes += ch.len_utf8();
                        chars.next();
                    }
                    _ => break,
                }
            }

            newline_idx += 1;
        } else {
            result.push(c);
        }
    }

    compact_str::CompactString::from(result)
}

/// The offset of the last `SetCursor` in `effects`.
fn last_cursor_offset_in(effects: &[Effect]) -> Option<usize> {
    effects.iter().rev().find_map(|e| match e {
        Effect::SetCursor { offset } => Some(offset.get()),
        _ => None,
    })
}

/// Inject saved text for dot-repeat after all other effects have been processed.
///
/// `last_cursor_offset` comes from the single-pass `ProcessResult`, avoiding
/// a separate linear scan of effects.
///
/// `doc_text` is the document text, needed for Replace mode dot-repeat to
/// know which characters to overwrite.
/// The description of a dot-repeat text injection: how many times, where the
/// cursor was, and the indent settings the replay has to reproduce.
///
/// `inject_repeat_text` and `build_repeat_positional_effects` both need exactly
/// this set, so it travels as one value instead of eight positional arguments.
pub(crate) struct RepeatText<'a> {
    /// Repeat count from the intercepted command (`3.` -> 3).
    pub count: u32,
    /// Cursor offset when the repeated command started.
    pub input_cursor_offset: Offset,
    /// Cursor offset after the previous execution, when known.
    pub last_cursor_offset: Option<Offset>,
    /// Current document text, when the caller has it.
    pub doc_text: Option<&'a str>,
    /// Host indent provider used to re-indent the replayed text.
    pub indent_provider: Option<&'a dyn crate::document::IndentProvider>,
    /// Effective `'tabstop'`.
    pub tabstop: usize,
    /// Effective `'autoindent'`.
    pub autoindent: bool,
    /// Effective formatting options. Vim replays the inserted text as typed,
    /// so the replay breaks lines like typing does.
    pub format: crate::commands::insert::wrap::FormatPolicy<'a>,
}

pub(crate) fn inject_repeat_text(
    state: &mut VimState,
    parser: &mut Parser,
    response: &mut Response,
    undolevels_max: Option<usize>,
    repeat: &RepeatText<'_>,
) {
    use unicode_segmentation::UnicodeSegmentation;

    let &RepeatText {
        count,
        input_cursor_offset,
        last_cursor_offset,
        doc_text,
        indent_provider,
        tabstop,
        autoindent,
        ref format,
    } = repeat;
    let saved_text = state.last_inserted_text();
    if saved_text.is_empty() {
        return;
    }

    let is_replace =
        state.last_insert_entry_type() == crate::primitives::InsertEntryType::ReplaceMode;

    // ── Indent-aware dot-repeat: recompute auto-indent for replay context ──
    let indent_lens = state.last_insert_indent_lens();
    let should_reindent = !indent_lens.is_empty()
        && saved_text.contains('\n')
        && (indent_provider.is_some() || autoindent);

    let insert_pos_for_reindent =
        last_cursor_offset.map_or_else(|| input_cursor_offset.get(), Offset::get);

    let effective_text = if should_reindent {
        reindent_for_repeat(
            saved_text,
            indent_lens,
            indent_provider,
            doc_text.unwrap_or(""),
            insert_pos_for_reindent,
            tabstop,
            autoindent,
        )
    } else {
        compact_str::CompactString::from(saved_text)
    };

    // Build repeated text (for counted inserts like 3iX<Esc>)
    let text_clone = if count > 1 {
        compact_str::CompactString::from(effective_text.repeat(count as usize))
    } else {
        effective_text
    };

    // Use pre-tracked cursor position (last SetCursor wins)
    let insert_pos = last_cursor_offset.map_or_else(|| input_cursor_offset.get(), Offset::get);

    // Calculate last grapheme length for cursor positioning
    let last_grapheme_len = text_clone.graphemes(true).next_back().map_or(0, str::len);

    // Track the byte offset where the LAST Insert effect starts (needed
    // for mark '.' and mark ']' corrections for block insert).
    let mut last_insert_offset = insert_pos;
    // Where the cursor ends when formatting broke the replayed text.
    let mut formatted_cursor: Option<usize> = None;

    if is_replace {
        // Replace mode dot-repeat: delete existing chars then insert
        let effects = insert_effects::repeat_replace(
            insert_pos,
            text_clone.clone(),
            last_grapheme_len,
            doc_text.unwrap_or(""),
        );
        for mut effect in effects.into_inner() {
            sync_effect_mut(state, parser, &mut effect, doc_text, undolevels_max);
            response.effects.push(effect);
        }
    } else {
        // Normal insert dot-repeat
        let mut effects =
            insert_effects::repeat_insert(insert_pos, text_clone.clone(), last_grapheme_len);
        let is_block = state
            .insert_state()
            .is_some_and(|is| is.block_insert().is_some());
        if let (Some(text), false) = (doc_text, is_block) {
            let typed = insert_pos..insert_pos + text_clone.len();
            if crate::commands::insert::wrap::format_inserted_text(
                &mut effects,
                text,
                response.effects.as_slice(),
                typed,
                format,
                None,
            )
            .is_some()
            {
                formatted_cursor = last_cursor_offset_in(effects.as_slice());
            }
        }
        for mut effect in effects.into_inner() {
            sync_effect_mut(state, parser, &mut effect, doc_text, undolevels_max);
            response.effects.push(effect);
        }
    }

    // Block insert replication: if this was a block visual insert/append
    // (InsertState has block_insert context), replicate the text on each
    // secondary line, bottom-to-top so byte offsets stay valid.
    //
    // Extract block context data before the loop to avoid holding an
    // immutable borrow on `state` across `sync_effect_mut` calls.
    let block_info = state.insert_state().and_then(|is| {
        is.block_insert().map(|bc| {
            (
                bc.grapheme_col(),
                bc.lines_below(),
                bc.cursor_return_offset(),
            )
        })
    });
    if let Some((gcol, lines_below, return_offset)) = block_info {
        if let Some(text) = doc_text {
            use crate::commands::helpers::{line_end, line_of, line_start};
            let primary_line = line_of(text, insert_pos.min(text.len()));
            // The primary insert added text_clone.len() bytes on the primary
            // line. All offsets on subsequent lines must be shifted by this
            // amount because effects are applied sequentially by the host.
            let primary_shift = text_clone.len();
            // Replicate from bottom to top so offsets stay valid
            for line_idx in (primary_line + 1..=primary_line + lines_below).rev() {
                if let (Some(ls), Some(le)) = (line_start(text, line_idx), line_end(text, line_idx))
                {
                    let line_text = &text[ls..le];
                    let graphemes: Vec<&str> = line_text.graphemes(true).collect();
                    let col = gcol.min(graphemes.len());
                    let byte_offset: usize = graphemes
                        .get(..col)
                        .unwrap_or(&[])
                        .iter()
                        .map(|g| g.len())
                        .sum::<usize>()
                        + ls
                        + primary_shift;
                    last_insert_offset = byte_offset;
                    let block_effects = insert_effects::repeat_insert(
                        byte_offset,
                        text_clone.clone(),
                        0, // cursor positioning not needed for secondary lines
                    );
                    for mut effect in block_effects.into_inner() {
                        sync_effect_mut(state, parser, &mut effect, doc_text, undolevels_max);
                        response.effects.push(effect);
                    }
                }
            }
            // Restore cursor to block's return position (top-left corner)
            response.effects.push(crate::effects::Effect::SetCursor {
                offset: return_offset,
            });

            // ── Precompute undo/redo marks for block insert groups ─────────
            //
            // Block insert inserts text on multiple lines. The undo tree
            // needs T0 (pre-edit) line-start offsets for undo marks, but at
            // end_group() time only T1 text is available. Compute the correct
            // values here where we know the block geometry.
            //
            // Undo `']`: line-start of the bottom affected line in T0.
            // T1 text has primary_shift extra bytes on the primary line,
            // so T0 line starts are T1 line starts minus primary_shift.
            let bottom_line = primary_line + lines_below;
            if let Some(t1_bottom_ls) = line_start(text, bottom_line) {
                let t0_bottom_ls = t1_bottom_ls.saturating_sub(primary_shift);
                state
                    .undo_tree_mut()
                    .set_precomputed_undo_mark_end(Offset::new(t0_bottom_ls));
            }

            // Redo `']`: line-start of the bottom affected line in T1
            // (post-all-edits). Each secondary insert adds primary_shift
            // bytes, so the bottom line start in the final text is:
            // t1_bottom_ls + lines_below * primary_shift.
            if let Some(t1_bottom_ls) = line_start(text, bottom_line) {
                let post_all_edits_bottom_ls = t1_bottom_ls + lines_below * primary_shift;
                // Redo `'.'`: line-start of the first secondary line in T1
                // (the line below the primary). In T1, this is already the
                // correct offset; after all block inserts it shifts by
                // primary_shift (the primary insert's added bytes).
                let secondary_line = primary_line + 1;
                let redo_dot = line_start(text, secondary_line).map(|ls| ls + primary_shift);
                state.undo_tree_mut().set_precomputed_redo_marks(
                    redo_dot.map(Offset::new),
                    Some(Offset::new(post_all_edits_bottom_ls)),
                );
            }
        }
    }

    // ── Mark '.' correction ────────────────────────────────────────────
    //
    // Replace mode: Neovim does NOT batch, so mark '.' = last replaced
    // position = insert_pos + text_len - last_grapheme_len.
    //
    // Insert mode: Neovim batches consecutive ASCII chars, so apply
    // `compute_mark_dot_for_repeat`.
    //
    // For block insert during dot-repeat, apply the same line-start
    // correction as the initial block insert (exit.rs step 6).
    {
        let mark_dot_offset = if block_info.is_some() {
            if let Some(text) = doc_text {
                let primary_line =
                    crate::commands::helpers::line_of(text, insert_pos.min(text.len()));
                let secondary_line = primary_line + 1;
                let line_start = crate::commands::helpers::line_start(text, secondary_line)
                    .unwrap_or(insert_pos);
                line_start + text_clone.len()
            } else {
                last_insert_offset
            }
        } else if is_replace {
            last_insert_offset + text_clone.len() - last_grapheme_len
        } else if let Some(cursor) = formatted_cursor {
            // The replay was broken into lines: the last change is the
            // last character, where the cursor ends.
            cursor
        } else if count > 1 {
            // Counted insert (e.g., 3iX<Esc>.): mark '.' = last character
            // position, matching exit_finalize's `new_offset.saturating_sub(1)`.
            last_insert_offset + text_clone.len().saturating_sub(1)
        } else {
            compute_mark_dot_for_repeat(&text_clone, last_insert_offset)
        };
        let mut set_mark_dot = Effect::SetMark {
            name: crate::primitives::MarkName::LAST_CHANGE,
            offset: Offset::new(mark_dot_offset),
            topline_offset: None,
        };
        sync_effect_mut(state, parser, &mut set_mark_dot, doc_text, undolevels_max);
        response.effects.push(set_mark_dot);
    }

    // ── Mark ']' correction for block insert ───────────────────────────
    //
    // Neovim sets mark `]` to the end of the **primary** line's insert,
    // not the bottommost secondary line.  `insert_pos + text_clone.len()`
    // For block change (c), mark `]` = end of primary line insert.
    // For block insert/append (I/A), mark `]` = end of bottommost insert.
    if let Some((_, lines_below, _)) = block_info {
        let is_block_change =
            state.last_insert_entry_type() == crate::primitives::InsertEntryType::ChangeOperator;
        let bracket_offset = if is_block_change {
            insert_pos + text_clone.len()
        } else if let Some(text) = doc_text {
            // Bottommost line insert end in post-all-edits coordinates:
            // each secondary insert adds text_clone.len() bytes.
            let primary_line = crate::commands::helpers::line_of(text, insert_pos.min(text.len()));
            let bottom_line = primary_line + lines_below;
            let bottom_ls =
                crate::commands::helpers::line_start(text, bottom_line).unwrap_or(insert_pos);
            // Account for primary shift + all prior block inserts
            bottom_ls + text_clone.len() * (lines_below + 1)
        } else {
            insert_pos + text_clone.len()
        };
        let mut set_mark_bracket = Effect::SetMark {
            name: crate::primitives::MarkName::CHANGE_END,
            offset: Offset::new(bracket_offset),
            topline_offset: None,
        };
        sync_effect_mut(
            state,
            parser,
            &mut set_mark_bracket,
            doc_text,
            undolevels_max,
        );
        response.effects.push(set_mark_bracket);
    }

    // SetMark('^') — mirrors exit_finalize. The '^' mark records where
    // the cursor was during insert mode (one past the last inserted
    // character), which is what `gi` uses to return. For dot-repeat,
    // this is insert_pos + text_len (the end of the injected text),
    // NOT the backed-up SetCursor position.
    let mark_offset = insert_pos + text_clone.len();
    let mut set_mark = Effect::SetMark {
        name: crate::primitives::MarkName::INSERT_STOP,
        offset: Offset::new(mark_offset),
        topline_offset: None,
    };
    sync_effect_mut(state, parser, &mut set_mark, doc_text, undolevels_max);
    response.effects.push(set_mark);

    // SetStickyColumn — ensures curswant reflects the post-repeat cursor
    // column, so subsequent j/k use the correct target.
    //
    // For o/O repeat, the `o` command already inserted a newline BEFORE
    // inject_repeat_text was called, so the cursor is on a fresh line.
    // The final cursor column = bytes of inserted text minus the backed-up
    // grapheme (the cursor position relative to the new line start).
    //
    // For regular insert repeat, compute from doc_text + cursor offset.
    {
        let entry_type = state.last_insert_entry_type();
        let is_open_line = matches!(
            entry_type,
            crate::primitives::InsertEntryType::NewLineBelow
                | crate::primitives::InsertEntryType::NewLineAbove
        );
        // Compute virtual column (curswant) for the post-repeat cursor.
        // We build the post-insert line text and use `byte_to_vcol` to get
        // the correct display-width-based column for multi-byte / ZWJ emoji.
        //
        // The default tabstop is 8; inject_repeat_text doesn't receive the
        // host tabstop, but for text that was just inserted by the user it
        // is extremely rare to contain literal tabs.
        let default_tabstop = 8;
        let cursor_column = if is_open_line || text_clone.contains('\n') {
            // o/O repeat or text with newlines: cursor is on a new line.
            // The line content is the text after the last newline.
            let after_last_nl = if let Some(nl_pos) = text_clone.rfind('\n') {
                &text_clone[nl_pos + 1..]
            } else {
                &text_clone[..]
            };
            let cursor_byte_in_line = after_last_nl.len().saturating_sub(last_grapheme_len);
            crate::commands::helpers::byte_to_vcol(
                after_last_nl,
                cursor_byte_in_line,
                default_tabstop,
            )
        } else if let Some(text) = doc_text {
            // Single-line insert on existing line.
            // Build the post-insert line: prefix (from line start to insert pos)
            // + inserted text + suffix.
            let ls =
                crate::commands::helpers::line_start_for_offset(text, insert_pos.min(text.len()));
            let le =
                crate::commands::helpers::line_end_for_offset(text, insert_pos.min(text.len()));
            let prefix = &text[ls..insert_pos.min(text.len())];
            let suffix = &text[insert_pos.min(text.len())..le];
            let post_line = format!("{}{}{}", prefix, &*text_clone, suffix);
            let cursor_byte_in_line =
                prefix.len() + text_clone.len().saturating_sub(last_grapheme_len);
            crate::commands::helpers::byte_to_vcol(&post_line, cursor_byte_in_line, default_tabstop)
        } else {
            0
        };
        let mut sticky = Effect::SetStickyColumn {
            column: Some(crate::primitives::VirtualColumn::new(cursor_column)),
        };
        sync_effect_mut(state, parser, &mut sticky, doc_text, undolevels_max);
        response.effects.push(sticky);
    }

    // Emit a single EndUndoGroup for the entire dot-repeat operation.
    // This commits the pending undo group opened by the original command's
    // BeginUndoGroup, giving the host a valid node_id for undo navigation.
    let mut end_undo = Effect::EndUndoGroup { node_id: None };
    sync_effect_mut(state, parser, &mut end_undo, doc_text, undolevels_max);
    response.effects.push(end_undo);
}

/// Build positional effects (Insert+SetCursor) for dot-repeat at one cursor.
/// Returns `None` when there is no saved insert text. Counterpart:
/// [`inject_repeat_global_effects`] emits marks/sticky/EndUndoGroup once.
pub(crate) fn build_repeat_positional_effects(
    state: &VimState,
    repeat: &RepeatText<'_>,
) -> Option<crate::effects::Effects> {
    use unicode_segmentation::UnicodeSegmentation;

    let &RepeatText {
        count,
        input_cursor_offset,
        last_cursor_offset,
        doc_text,
        indent_provider,
        tabstop,
        autoindent,
        format: _,
    } = repeat;
    let saved_text = state.last_inserted_text();
    if saved_text.is_empty() {
        return None;
    }
    let is_replace =
        state.last_insert_entry_type() == crate::primitives::InsertEntryType::ReplaceMode;
    let indent_lens = state.last_insert_indent_lens();
    let should_reindent = !indent_lens.is_empty()
        && saved_text.contains('\n')
        && (indent_provider.is_some() || autoindent);
    let insert_pos_for_reindent =
        last_cursor_offset.map_or_else(|| input_cursor_offset.get(), Offset::get);
    let effective_text = if should_reindent {
        reindent_for_repeat(
            saved_text,
            indent_lens,
            indent_provider,
            doc_text.unwrap_or(""),
            insert_pos_for_reindent,
            tabstop,
            autoindent,
        )
    } else {
        compact_str::CompactString::from(saved_text)
    };
    let text_clone = if count > 1 {
        compact_str::CompactString::from(effective_text.repeat(count as usize))
    } else {
        effective_text
    };
    let insert_pos = last_cursor_offset.map_or_else(|| input_cursor_offset.get(), Offset::get);
    let last_grapheme_len = text_clone.graphemes(true).next_back().map_or(0, str::len);
    let effects = if is_replace {
        insert_effects::repeat_replace(
            insert_pos,
            text_clone,
            last_grapheme_len,
            doc_text.unwrap_or(""),
        )
    } else {
        insert_effects::repeat_insert(insert_pos, text_clone, last_grapheme_len)
    };
    Some(effects)
}

/// Emit global effects for multi-cursor dot-repeat (marks, sticky, EndUndoGroup).
/// Counterpart to [`build_repeat_positional_effects`].
pub(crate) fn inject_repeat_global_effects(
    state: &mut VimState,
    parser: &mut Parser,
    count: u32,
    primary_insert_pos: usize,
    response: &mut Response,
    doc_text: Option<&str>,
    undolevels_max: Option<usize>,
) {
    use unicode_segmentation::UnicodeSegmentation;
    let saved_text = state.last_inserted_text();
    if saved_text.is_empty() {
        return;
    }
    let is_replace =
        state.last_insert_entry_type() == crate::primitives::InsertEntryType::ReplaceMode;
    let text_clone = if count > 1 {
        compact_str::CompactString::from(saved_text.repeat(count as usize))
    } else {
        compact_str::CompactString::from(saved_text)
    };
    let last_grapheme_len = text_clone.graphemes(true).next_back().map_or(0, str::len);
    let insert_pos = primary_insert_pos;
    // Mark '.'
    {
        let mark_dot_offset = if is_replace {
            insert_pos + text_clone.len() - last_grapheme_len
        } else if count > 1 {
            insert_pos + text_clone.len().saturating_sub(1)
        } else {
            compute_mark_dot_for_repeat(&text_clone, insert_pos)
        };
        let mut set_mark_dot = Effect::SetMark {
            name: crate::primitives::MarkName::LAST_CHANGE,
            offset: Offset::new(mark_dot_offset),
            topline_offset: None,
        };
        sync_effect_mut(state, parser, &mut set_mark_dot, doc_text, undolevels_max);
        response.effects.push(set_mark_dot);
    }
    // Mark '^'
    {
        let mark_offset = insert_pos + text_clone.len();
        let mut set_mark = Effect::SetMark {
            name: crate::primitives::MarkName::INSERT_STOP,
            offset: Offset::new(mark_offset),
            topline_offset: None,
        };
        sync_effect_mut(state, parser, &mut set_mark, doc_text, undolevels_max);
        response.effects.push(set_mark);
    }
    // SetStickyColumn
    {
        let entry_type = state.last_insert_entry_type();
        let is_open_line = matches!(
            entry_type,
            crate::primitives::InsertEntryType::NewLineBelow
                | crate::primitives::InsertEntryType::NewLineAbove
        );
        let default_tabstop = 8;
        let cursor_column = if is_open_line || text_clone.contains('\n') {
            let after_last_nl = text_clone
                .rfind('\n')
                .map_or(text_clone.as_str(), |p| &text_clone[p + 1..]);
            let cursor_byte_in_line = after_last_nl.len().saturating_sub(last_grapheme_len);
            crate::commands::helpers::byte_to_vcol(
                after_last_nl,
                cursor_byte_in_line,
                default_tabstop,
            )
        } else if let Some(text) = doc_text {
            let ls =
                crate::commands::helpers::line_start_for_offset(text, insert_pos.min(text.len()));
            let le =
                crate::commands::helpers::line_end_for_offset(text, insert_pos.min(text.len()));
            let prefix = &text[ls..insert_pos.min(text.len())];
            let suffix = &text[insert_pos.min(text.len())..le];
            let post_line = format!("{}{}{}", prefix, &*text_clone, suffix);
            let cursor_byte_in_line =
                prefix.len() + text_clone.len().saturating_sub(last_grapheme_len);
            crate::commands::helpers::byte_to_vcol(&post_line, cursor_byte_in_line, default_tabstop)
        } else {
            0
        };
        let mut sticky = Effect::SetStickyColumn {
            column: Some(crate::primitives::VirtualColumn::new(cursor_column)),
        };
        sync_effect_mut(state, parser, &mut sticky, doc_text, undolevels_max);
        response.effects.push(sticky);
    }
    // EndUndoGroup
    let mut end_undo = Effect::EndUndoGroup { node_id: None };
    sync_effect_mut(state, parser, &mut end_undo, doc_text, undolevels_max);
    response.effects.push(end_undo);
}

/// Build a line-lookup closure from optional document text.
///
/// When `text` is available, counts `\n` bytes before the offset to determine
/// the 0-indexed line number — matching Neovim's `(fnum, lnum)` dedup key.
/// When `text` is `None`, falls back to using the raw byte offset as line proxy
/// so each distinct offset gets its own key (preserving pre-existing behavior
/// for callers that don't supply document text).
fn offset_to_line_fn(text: Option<&str>) -> impl Fn(Offset) -> usize + '_ {
    move |off: Offset| match text {
        Some(t) => {
            let pos = off.get().min(t.len());
            t.as_bytes()[..pos].iter().filter(|&&b| b == b'\n').count()
        }
        None => off.get(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::grammar::parser::Parser;
    use crate::primitives::Mode;
    use crate::primitives::{MotionType, Offset, RegisterName};
    use crate::state::VimState;
    use compact_str::CompactString;

    /// Helper: create a response with the given effects.
    fn response_with(effects: Vec<Effect>) -> Response {
        let mut r = Response::default();
        r.effects = smallvec::SmallVec::from_vec(effects);
        r
    }

    // ─── SetMode sync ──────────────────────────────────────────────────

    #[test]
    fn process_effects_syncs_set_mode_to_state() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        assert_eq!(state.mode(), Mode::Normal);

        let mut response = response_with(vec![Effect::set_mode(Mode::Visual(
            crate::primitives::VisualType::Char,
        ))]);
        process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            state.mode(),
            Mode::Visual(crate::primitives::VisualType::Char)
        );
        // Original effect should be kept (not intercepted).
        // +2 from emit_mode_events: VisualEnter + ModeChanged.
        // +1 from emit_cursor_style: SetCursorStyle.
        assert_eq!(response.effects.len(), 4);
    }

    #[test]
    fn process_effects_intercepts_repeat_set_mode_insert() {
        let mut state = VimState::default();
        let mut parser = Parser::new();

        let mut response = response_with(vec![Effect::set_mode(Mode::Insert)]);
        let result = process_effects(&mut state, &mut parser, true, &mut response);

        // SetMode(Insert) during repeat should be intercepted (removed)
        assert!(
            response.effects.is_empty(),
            "repeat SetMode(Insert) should be removed"
        );
        assert_eq!(result.repeat_count, Some(1));
        assert!(
            result.ended_repeat,
            "ended_repeat should be set after interception"
        );
    }

    #[test]
    fn process_effects_collects_play_macro() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let reg = RegisterName::new('a').unwrap();

        let mut response = response_with(vec![Effect::PlayMacro {
            register: reg,
            count: 3,
        }]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);

        // PlayMacro should be removed from effects and collected
        assert!(
            response.effects.is_empty(),
            "PlayMacro should be removed from response"
        );
        assert_eq!(result.macro_plays.len(), 1);
        assert_eq!(result.macro_plays[0], (reg, 3));
    }

    #[test]
    fn process_effects_tracks_last_cursor_offset() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![
            Effect::SetCursor {
                offset: Offset::new(10),
            },
            Effect::SetCursor {
                offset: Offset::new(42),
            },
        ]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);

        // Last SetCursor wins
        assert_eq!(result.last_cursor_offset, Some(Offset::new(42)));
        // SetCursor effects should be kept, plus CursorMoved event
        assert_eq!(response.effects.len(), 3);
        assert!(matches!(
            response.effects[2],
            Effect::Event {
                kind: crate::primitives::VimEvent::CursorMoved
            }
        ));
    }

    // ─── Clipboard routing via ProcessResult ──────────────────────────

    #[test]
    fn process_effects_unnamed_register_written_on_direct_set() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::SetRegister {
            name: RegisterName::UNNAMED,
            text: CompactString::from("yanked"),
            motion_type: MotionType::CharWise,
        }]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            result.unnamed_register_written,
            Some(CompactString::from("yanked")),
            "direct UNNAMED write should be captured"
        );
    }

    #[test]
    fn process_effects_unnamed_register_written_on_delete_numbered() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        // NUMBERED_1 routes through on_delete() which always writes unnamed
        let mut response = response_with(vec![Effect::SetRegister {
            name: RegisterName::NUMBERED_1,
            text: CompactString::from("deleted line"),
            motion_type: MotionType::LineWise,
        }]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            result.unnamed_register_written,
            Some(CompactString::from("deleted line\n")),
            "NUMBERED_1 write should update unnamed and be captured"
        );
    }

    #[test]
    fn process_effects_unnamed_register_written_on_small_delete() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::SetRegister {
            name: RegisterName::SMALL_DELETE,
            text: CompactString::from("x"),
            motion_type: MotionType::CharWise,
        }]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            result.unnamed_register_written,
            Some(CompactString::from("x")),
            "SMALL_DELETE write should update unnamed and be captured"
        );
    }

    #[test]
    fn process_effects_unnamed_register_written_none_for_named_register() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let named_reg = RegisterName::new('a').unwrap();
        let mut response = response_with(vec![Effect::SetRegister {
            name: named_reg,
            text: CompactString::from("test"),
            motion_type: MotionType::CharWise,
        }]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);

        // Named register write does NOT update unnamed, so no clipboard candidate
        assert_eq!(
            result.unnamed_register_written, None,
            "named register write should NOT set unnamed_register_written"
        );
    }

    #[test]
    fn process_effects_unnamed_register_written_none_when_no_register_effect() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::SetCursor {
            offset: Offset::new(10),
        }]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            result.unnamed_register_written, None,
            "no register effect should give None"
        );
    }

    // ─── Register sync ────────────────────────────────────────────────

    #[test]
    fn sync_effect_stores_register_content() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let reg = RegisterName::new('z').unwrap();
        let text = CompactString::from("hello");

        let effect = Effect::SetRegister {
            name: reg,
            text: text.clone(),
            motion_type: MotionType::CharWise,
        };
        sync_effect(&mut state, &mut parser, &effect);

        let content = state.registers().get(reg).expect("register should be set");
        assert_eq!(content.text(), "hello");
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Changelist tracking ──────────────────────────────────────────

    #[test]
    fn process_effects_changelist_records_insert_position() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::from("abc"),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);

        // Changelist should have recorded the insert position
        assert!(
            !state.changelist().is_empty(),
            "changelist should record the edit"
        );
    }

    #[test]
    fn process_effects_insert_sets_change_marks() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::from("abc"),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            state
                .marks()
                .get(crate::primitives::MarkName::CHANGE_START)
                .unwrap()
                .offset()
                .get(),
            5
        );
        // `]` mark is exclusive for inserts: one past last byte (5 + 3 = 8)
        assert_eq!(
            state
                .marks()
                .get(crate::primitives::MarkName::CHANGE_END)
                .unwrap()
                .offset()
                .get(),
            8
        );
        // `.` mark is the start of the first edit in the response (Neovim behavior)
        assert_eq!(
            state
                .marks()
                .get(crate::primitives::MarkName::LAST_CHANGE)
                .unwrap()
                .offset()
                .get(),
            5
        );
    }

    #[test]
    fn delete_with_explicit_setmark_overrides_sync() {
        // Simulate delete operator with explicit SetMark effects.
        // The Delete effect triggers sync_change_marks (sets marks to deletion start).
        // The SetMark effects AFTER should overwrite those values.
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::delete(crate::primitives::Range::from_raw(5, 15)),
            Effect::SetMark {
                name: crate::primitives::MarkName::CHANGE_START,
                offset: Offset::new(42),
                topline_offset: None,
            },
            Effect::SetMark {
                name: crate::primitives::MarkName::CHANGE_END,
                offset: Offset::new(42),
                topline_offset: None,
            },
            Effect::EndUndoGroup { node_id: None },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);

        // SetMark(42) should win over sync_change_marks(5)
        assert_eq!(
            state
                .marks()
                .get(crate::primitives::MarkName::CHANGE_START)
                .unwrap()
                .offset()
                .get(),
            42,
            "SetMark should override sync_change_marks for mark.["
        );
        assert_eq!(
            state
                .marks()
                .get(crate::primitives::MarkName::CHANGE_END)
                .unwrap()
                .offset()
                .get(),
            42,
            "SetMark should override sync_change_marks for mark.]"
        );
    }

    // ─── Undo tree integration ───────────────────────────────────────

    #[test]
    fn undo_tree_records_group_on_begin_end() {
        let mut state = VimState::default();
        let mut parser = Parser::new();

        // Set cursor hint before processing (engine does this)
        state.set_undo_cursor_hint(Offset::new(10));
        state.set_undo_timestamp_hint(100);

        let mut response = response_with(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(10),
                text: CompactString::from("abc"),
            },
            Effect::SetCursor {
                offset: Offset::new(13),
            },
            Effect::EndUndoGroup { node_id: None },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);

        let tree = state.undo_tree();
        assert_eq!(tree.change_count(), 1, "one undo group should be recorded");
        assert!(tree.can_undo(), "should be able to undo");

        let info = tree.current_info();
        assert_eq!(
            info.cursor_before(),
            Offset::new(10),
            "cursor_before from hint"
        );
        assert_eq!(
            info.cursor_after(),
            Offset::new(13),
            "cursor_after updated by SetCursor"
        );
        assert_eq!(
            info.first_edit_offset(),
            Some(Offset::new(10)),
            "first edit tracked"
        );
        assert_eq!(info.timestamp(), 100, "timestamp from hint");
    }

    #[test]
    fn undo_tree_empty_group_still_recorded() {
        // Neovim records ALL undo groups, even without text edits.
        // This ensures 'u' undoes cursor-only operations (guu on lowercase, etc.)
        let mut state = VimState::default();
        let mut parser = Parser::new();

        let mut response = response_with(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::SetCursor {
                offset: Offset::new(5),
            },
            Effect::EndUndoGroup { node_id: None },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            state.undo_tree().change_count(),
            1,
            "empty group should still be recorded"
        );
    }

    #[test]
    fn undo_tree_tracks_minimum_edit_offset() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        state.set_undo_cursor_hint(Offset::new(0));

        let mut response = response_with(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(20),
                text: CompactString::from("x"),
            },
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::from("y"),
            },
            Effect::Insert {
                offset: Offset::new(15),
                text: CompactString::from("z"),
            },
            Effect::EndUndoGroup { node_id: None },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            state.undo_tree().current_info().first_edit_offset(),
            Some(Offset::new(5)),
            "should track minimum edit offset"
        );
    }

    #[test]
    fn undo_tree_tracks_delete_and_replace() {
        use crate::primitives::Range;
        let mut state = VimState::default();
        let mut parser = Parser::new();
        state.set_undo_cursor_hint(Offset::new(0));

        // Delete
        let mut response = response_with(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Delete {
                range: Range::from_raw(3, 8),
            },
            Effect::EndUndoGroup { node_id: None },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(state.undo_tree().change_count(), 1);
        assert_eq!(
            state.undo_tree().current_info().first_edit_offset(),
            Some(Offset::new(3))
        );

        // Replace
        let mut response = response_with(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Replace {
                range: Range::from_raw(7, 9),
                text: CompactString::from("abc"),
            },
            Effect::EndUndoGroup { node_id: None },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(state.undo_tree().change_count(), 2);
        assert_eq!(
            state.undo_tree().current_info().first_edit_offset(),
            Some(Offset::new(7))
        );
    }

    #[test]
    fn undo_tree_multiple_groups_linear() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        state.set_undo_cursor_hint(Offset::new(0));
        state.set_undo_timestamp_hint(10);

        // Group 1
        let mut r1 = response_with(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::from("a"),
            },
            Effect::SetCursor {
                offset: Offset::new(1),
            },
            Effect::EndUndoGroup { node_id: None },
        ]);
        process_effects(&mut state, &mut parser, false, &mut r1);

        // Update hint for next group
        state.set_undo_cursor_hint(Offset::new(1));
        state.set_undo_timestamp_hint(20);

        // Group 2
        let mut r2 = response_with(vec![
            Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(1),
                text: CompactString::from("b"),
            },
            Effect::SetCursor {
                offset: Offset::new(2),
            },
            Effect::EndUndoGroup { node_id: None },
        ]);
        process_effects(&mut state, &mut parser, false, &mut r2);

        let tree = state.undo_tree();
        assert_eq!(tree.change_count(), 2);
        assert_eq!(tree.depth(), 2);
        assert!(tree.can_undo());
        assert!(!tree.can_redo());
    }

    // ─── Observation events ────────────────────────────────────────────

    /// Count VimEvent::X in the effects.
    fn count_events(effects: &[Effect], expected: VimEvent) -> usize {
        effects
            .iter()
            .filter(|e| matches!(e, Effect::Event { kind } if *kind == expected))
            .count()
    }

    #[test]
    fn cursor_moved_emitted_in_normal_mode() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::SetCursor {
            offset: Offset::new(5),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::CursorMoved), 1);
        assert_eq!(count_events(&response.effects, VimEvent::CursorMovedI), 0);
    }

    #[test]
    fn cursor_moved_i_emitted_in_insert_mode() {
        let mut state = VimState::default();
        state.set_mode(Mode::Insert);
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::SetCursor {
            offset: Offset::new(3),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::CursorMovedI), 1);
        assert_eq!(count_events(&response.effects, VimEvent::CursorMoved), 0);
    }

    #[test]
    fn text_changed_emitted_on_insert_effect_normal() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::from("hi"),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::TextChanged), 1);
        assert_eq!(count_events(&response.effects, VimEvent::TextChangedI), 0);
    }

    #[test]
    fn text_changed_i_emitted_in_insert_mode() {
        let mut state = VimState::default();
        state.set_mode(Mode::Insert);
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::from("x"),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::TextChangedI), 1);
        assert_eq!(count_events(&response.effects, VimEvent::TextChanged), 0);
    }

    #[test]
    fn text_changed_on_delete() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::Delete {
            range: crate::primitives::Range::from_raw(0, 3),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::TextChanged), 1);
    }

    #[test]
    fn text_changed_on_replace() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::Replace {
            range: crate::primitives::Range::from_raw(0, 2),
            text: CompactString::from("ab"),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::TextChanged), 1);
    }

    #[test]
    fn recording_enter_emitted() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let reg = RegisterName::new('q').unwrap();
        let mut response = response_with(vec![Effect::StartRecording { register: reg }]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);
        assert!(result.recording_started.is_some());
        assert_eq!(count_events(&response.effects, VimEvent::RecordingEnter), 1);
    }

    #[test]
    fn recording_leave_emitted() {
        let mut state = VimState::default();
        state
            .macros_mut()
            .start_recording(RegisterName::new('q').unwrap());
        let mut parser = Parser::new();
        parser.set_recording(Some(RegisterName::new('q').unwrap()));
        let mut response = response_with(vec![Effect::StopRecording]);
        let result = process_effects(&mut state, &mut parser, false, &mut response);
        assert!(result.recording_stopped);
        assert_eq!(count_events(&response.effects, VimEvent::RecordingLeave), 1);
    }

    #[test]
    fn no_events_when_nothing_happened() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::ClearMessage]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::CursorMoved), 0);
        assert_eq!(count_events(&response.effects, VimEvent::TextChanged), 0);
        assert_eq!(count_events(&response.effects, VimEvent::RecordingEnter), 0);
        assert_eq!(count_events(&response.effects, VimEvent::RecordingLeave), 0);
    }

    #[test]
    fn cursor_and_text_both_emitted() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::from("x"),
            },
            Effect::SetCursor {
                offset: Offset::new(1),
            },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::CursorMoved), 1);
        assert_eq!(count_events(&response.effects, VimEvent::TextChanged), 1);
    }

    #[test]
    fn cursor_moved_i_in_replace_mode() {
        let mut state = VimState::default();
        state.set_mode(Mode::Replace);
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::SetCursor {
            offset: Offset::new(5),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::CursorMovedI), 1);
    }

    #[test]
    fn text_changed_i_in_replace_mode() {
        let mut state = VimState::default();
        state.set_mode(Mode::Replace);
        let mut parser = Parser::new();
        let mut response = response_with(vec![Effect::Replace {
            range: crate::primitives::Range::from_raw(0, 1),
            text: CompactString::from("x"),
        }]);
        process_effects(&mut state, &mut parser, false, &mut response);
        assert_eq!(count_events(&response.effects, VimEvent::TextChangedI), 1);
        assert_eq!(count_events(&response.effects, VimEvent::TextChanged), 0);
    }

    #[test]
    fn multiple_set_cursor_emits_single_cursor_moved() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![
            Effect::SetCursor {
                offset: Offset::new(3),
            },
            Effect::SetCursor {
                offset: Offset::new(7),
            },
            Effect::SetCursor {
                offset: Offset::new(15),
            },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);
        // Multiple SetCursor effects should produce exactly one CursorMoved event
        assert_eq!(count_events(&response.effects, VimEvent::CursorMoved), 1);
    }

    #[test]
    fn multiple_mutations_emit_single_text_changed() {
        let mut state = VimState::default();
        let mut parser = Parser::new();
        let mut response = response_with(vec![
            Effect::Delete {
                range: crate::primitives::Range::from_raw(0, 3),
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::from("abc"),
            },
        ]);
        process_effects(&mut state, &mut parser, false, &mut response);
        // Multiple text mutations should produce exactly one TextChanged event
        assert_eq!(count_events(&response.effects, VimEvent::TextChanged), 1);
    }

    // ─── Bell rate limiting ───────────────────────────────────────────

    /// Count how many Bell effects are in the response.
    fn count_bells(effects: &[Effect]) -> usize {
        effects.iter().filter(|e| matches!(e, Effect::Bell)).count()
    }

    #[test]
    fn bell_rate_limit_allows_first_few() {
        let mut state = VimState::default();
        let mut parser = Parser::new();

        // Emit BELL_RATE_LIMIT bells — all should pass through.
        let bells: Vec<Effect> = (0..VimState::BELL_RATE_LIMIT)
            .map(|_| Effect::Bell)
            .collect();
        let mut response = response_with(bells);
        process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            count_bells(&response.effects),
            VimState::BELL_RATE_LIMIT as usize
        );
    }

    #[test]
    fn bell_rate_limit_suppresses_excess() {
        let mut state = VimState::default();
        let mut parser = Parser::new();

        // Emit many bells in a single batch — only BELL_RATE_LIMIT should pass.
        let bells: Vec<Effect> = (0..10).map(|_| Effect::Bell).collect();
        let mut response = response_with(bells);
        process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(
            count_bells(&response.effects),
            VimState::BELL_RATE_LIMIT as usize
        );
    }

    #[test]
    fn bell_rate_limit_across_calls() {
        let mut state = VimState::default();
        let mut parser = Parser::new();

        // First call: use up the rate limit.
        let mut r1 = response_with(vec![Effect::Bell; VimState::BELL_RATE_LIMIT as usize]);
        process_effects(&mut state, &mut parser, false, &mut r1);
        assert_eq!(count_bells(&r1.effects), VimState::BELL_RATE_LIMIT as usize);

        // Second call with a bell: should be suppressed (count not reset
        // because previous batch had bells).
        let mut r2 = response_with(vec![Effect::Bell]);
        process_effects(&mut state, &mut parser, false, &mut r2);
        assert_eq!(count_bells(&r2.effects), 0);

        // Third call with NO bell: counter resets.
        let mut r3 = response_with(vec![]);
        process_effects(&mut state, &mut parser, false, &mut r3);

        // Fourth call: bell should pass through again.
        let mut r4 = response_with(vec![Effect::Bell]);
        process_effects(&mut state, &mut parser, false, &mut r4);
        assert_eq!(count_bells(&r4.effects), 1);
    }

    // ─── belloff option ───────────────────────────────────────────────

    #[test]
    fn belloff_all_suppresses_all_bells() {
        let mut state = VimState::default();
        state.set_belloff(true);
        let mut parser = Parser::new();

        let mut response = response_with(vec![Effect::Bell]);
        process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(count_bells(&response.effects), 0);
    }

    #[test]
    fn belloff_default_allows_bells() {
        let mut state = VimState::default();
        // belloff defaults to false
        assert!(!state.belloff());
        let mut parser = Parser::new();

        let mut response = response_with(vec![Effect::Bell]);
        process_effects(&mut state, &mut parser, false, &mut response);

        assert_eq!(count_bells(&response.effects), 1);
    }
}
