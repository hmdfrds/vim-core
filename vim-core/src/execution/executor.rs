//! Command executor.
//!
//! Single-file dispatcher: maps `Command` variants to dispatch functions.
//! No business logic — only context construction and delegation.

use super::ExecutionContext;
use crate::commands::actions::effects as action_effects;
use crate::commands::ex::effects as ex_effects;
use crate::dispatch::enter_insert_at;
use crate::dispatch::{
    dispatch_action, dispatch_char_command, dispatch_insert_entry, dispatch_join_no_space,
    dispatch_mark, dispatch_motion_with_effects, dispatch_operator_find, dispatch_operator_line,
    dispatch_operator_mark, dispatch_operator_textobject, dispatch_operator_with_motion,
    dispatch_visual, dispatch_visual_textobject, ActionContext, CharCommandInput, MarkContext,
    MotionContext, MotionEffectsContext, OperatorContext, OperatorFindInput, OperatorLineInput,
    OperatorMarkInput, OperatorMotionInput, OperatorTextObjectInput, VisualContext,
};
use crate::document::Document;
use crate::effects::undo_intent::UndoIntent;
use crate::effects::Effects;
use crate::errors::VimError;
use crate::execution::safety_harness::validated_range;
use crate::grammar::types::{Action, MarkType, Motion, Operator, TextObject};
use crate::grammar::{Command, MacroKind, PrefixCommand, VisualKind};
use crate::primitives::byte_delta;
use crate::primitives::Mode;
use crate::primitives::VisualType;
use crate::primitives::{LastVisualInfo, Mark};
use crate::primitives::{MarkName, RegisterName};
use crate::primitives::{MotionType, Offset, Range, SelectionShape, VirtualColumn};
use crate::state::CommandLinePrompt;

/// Structured result from the executor.
///
/// Carries undo intent as explicit metadata alongside effects.
/// The intent is derived once here (the single funnel point for all
/// commands) and carried through multi-cursor dispatch.
pub struct ExecutorOutput {
    /// The raw effects from the command (including undo markers from the command).
    pub effects: Effects,
    /// Undo lifecycle intent, derived from scanning the effects.
    /// Consumed by multi-cursor dispatch (per_cursor + replicate_effects_precise).
    pub undo_intent: UndoIntent,
    /// Multi-cursor command to execute after effect processing.
    /// Used by gb/gB/gs actions to signal cursor mutations.
    pub multi_cursor_command: Option<(crate::state::MultiCursorCommand, u32)>,
}

impl ExecutorOutput {
    /// Wrap raw effects with derived undo intent.
    pub fn new(effects: Effects) -> Self {
        let undo_intent = UndoIntent::from_effects(effects.as_slice());
        Self {
            effects,
            undo_intent,
            multi_cursor_command: None,
        }
    }

    /// Attach a multi-cursor command to this output.
    pub fn with_multi_cursor(mut self, cmd: crate::state::MultiCursorCommand, count: u32) -> Self {
        self.multi_cursor_command = Some((cmd, count));
        self
    }
}

/// Command executor.
///
/// Takes a Command from Grammar and produces an [`ExecutorOutput`]
/// containing effects and undo lifecycle intent.
///
/// Stateless — all functions are module-level. No struct needed.
///
/// The engine calls [`execute_plan()`] which validates the plan and
/// delegates to [`execute()`] for actual command dispatch.
pub fn execute<D: Document>(command: Command, ctx: &ExecutionContext<'_, D>) -> ExecutorOutput {
    // gb/gB/gs: multi-cursor commands that bypass the stateless executor.
    // These need `&mut VimState` which execute_raw doesn't have, so we
    // signal the engine via ExecutorOutput.multi_cursor_command.
    if let Command::Action { count, action, .. } = &command {
        match action {
            Action::AddNextMatchCursor => {
                let cmd = crate::state::MultiCursorCommand::AddNextMatch {
                    direction: crate::primitives::Direction::Forward,
                    skip: false,
                };
                return ExecutorOutput::new(Effects::new()).with_multi_cursor(cmd, count.get());
            }
            Action::AddPrevMatchCursor => {
                let cmd = crate::state::MultiCursorCommand::AddNextMatch {
                    direction: crate::primitives::Direction::Backward,
                    skip: false,
                };
                return ExecutorOutput::new(Effects::new()).with_multi_cursor(cmd, count.get());
            }
            Action::SkipMatchCursor => {
                let cmd = crate::state::MultiCursorCommand::AddNextMatch {
                    direction: crate::primitives::Direction::Forward,
                    skip: true,
                };
                return ExecutorOutput::new(Effects::new()).with_multi_cursor(cmd, count.get());
            }
            _ => {}
        }
    }
    ExecutorOutput::new(execute_raw(command, ctx))
}

/// Internal: produce raw effects from a command dispatch.
fn execute_raw<D: Document>(command: Command, ctx: &ExecutionContext<'_, D>) -> Effects {
    // Shared locals — extracted once, used by all arms consistently.
    let text = ctx.doc().text();
    let cursor = ctx.cursor_offset();

    match command {
        // ═══════════════════════════════════════════════════════════════════
        // Motion commands (h, j, k, l, w, e, b, gg, G, etc.)
        // ═══════════════════════════════════════════════════════════════════
        Command::Motion {
            count,
            motion,
            explicit_count,
        } => execute_motion(motion, count.get(), explicit_count, ctx),

        Command::ModeSwitch { mode } => action_effects::switch_mode(mode),

        // ═══════════════════════════════════════════════════════════════════
        // Find/replace without operator (f, F, t, T, r)
        // ═══════════════════════════════════════════════════════════════════
        Command::CharCommand {
            count,
            command,
            ref target,
            operator: None,
            ..
        } => {
            let selection = ctx.selection();
            let char_effects = dispatch_char_command(&CharCommandInput {
                cmd: command,
                ch: target.chars().next().unwrap_or('\0'),
                count,
                text,
                cursor,
                selection: selection.as_ref(),
                mode: ctx.state.mode(),
                options: ctx.options,
            })
            .effects;
            prepend_visual_exit_marks(char_effects, ctx, text)
        }

        // ═══════════════════════════════════════════════════════════════════
        // Operator + motion (dw, yj, c$, etc.)
        // ═══════════════════════════════════════════════════════════════════
        Command::OperatorMotion {
            count,
            operator,
            motion,
            register,
            force_type,
        } => {
            let count = count.get();

            let search = ctx.state.search().pattern().map(|pattern| {
                let direction = ctx.state.search().direction().into();
                (pattern, direction)
            });
            let last_find = ctx.last_find();
            append_change_marks(
                dispatch_operator_with_motion(&OperatorMotionInput {
                    operator,
                    motion,
                    count,
                    register,
                    text,
                    cursor: Offset::new(cursor),
                    search,
                    last_find,
                    shiftwidth: ctx.options.shiftwidth(),
                    textwidth: ctx.options.textwidth(),
                    options: ctx.options,
                    force_type,
                    custom_operators: ctx.providers().custom_operators,
                    viewport: ctx.viewport(),
                    sticky_column: ctx.state.sticky_column(),
                    fold_provider: ctx.providers().fold,
                })
                .effects,
            )
        }

        // Operator + text object (diw, ci(, yap, etc.)
        Command::OperatorTextObject {
            count,
            operator,
            textobject,
            register,
        } => {
            let count = count.get();

            append_change_marks(
                dispatch_operator_textobject(&OperatorTextObjectInput {
                    operator,
                    textobject,
                    count,
                    register,
                    text,
                    options: ctx.options,
                    cursor,
                    shiftwidth: ctx.options.shiftwidth(),
                    tabstop: ctx.options.tabstop(),
                    expandtab: ctx.options.expandtab(),
                    textwidth: ctx.options.textwidth(),
                    commentstring: ctx.options.commentstring(),
                    providers: *ctx.providers(),
                    custom_operators: ctx.providers().custom_operators,
                })
                .effects,
            )
        }

        // Operator on lines (dd, yy, cc)
        Command::OperatorLine {
            count,
            operator,
            register,
        } => {
            let count = count.get();

            append_change_marks(
                dispatch_operator_line(&OperatorLineInput {
                    operator,
                    count,
                    register,
                    text,
                    cursor,
                    shiftwidth: ctx.options.shiftwidth(),
                    tabstop: ctx.options.tabstop(),
                    expandtab: ctx.options.expandtab(),
                    textwidth: ctx.options.textwidth(),
                    options: ctx.options,
                    commentstring: ctx.options.commentstring(),
                    custom_operators: ctx.providers().custom_operators,
                    sticky_column: ctx.state.sticky_column(),
                })
                .effects,
            )
        }

        // Operator to mark (y'a, d`b, etc.) — mark lookup is orchestration
        Command::OperatorMark {
            count: _,
            operator,
            register,
            mark,
            mark_type,
        } => {
            // ── Cross-buffer global mark guard for operators ─────────────
            // Operators targeting a cross-buffer global mark are nonsensical
            // (the range would span buffers). Emit MarkNotSet error, matching
            // Neovim behaviour.
            if mark.is_global() {
                if let Some(entry) = ctx.state.marks().get_global(mark) {
                    let current_bid = ctx.state.current_buffer_id();
                    if current_bid.is_some() && Some(entry.buffer_id) != current_bid {
                        return ex_effects::show_error(VimError::MarkNotSet(mark.char()));
                    }
                }
            }

            let Some(mark_val) = ctx.state.marks().get(mark) else {
                return ex_effects::show_error(VimError::MarkNotSet(mark.char()));
            };
            let mark_offset = mark_val.offset().get();
            // In Neovim, operator+mark (y'a, d`b) pushes to jumplist when the
            // mark target is on a different line (mark motions are jump motions).
            // Push the pre-operator cursor position.
            let mark_line = crate::commands::helpers::line_of(text, mark_offset.min(text.len()));
            let cursor_line = crate::commands::helpers::line_of(text, cursor);
            let mut jump_effects = Effects::new();
            if mark_line != cursor_line {
                jump_effects = jump_effects.push_jump_list(Offset::new(cursor));
            }
            let op_effects = append_change_marks(
                dispatch_operator_mark(&OperatorMarkInput {
                    operator,
                    text,
                    cursor,
                    mark_offset,
                    mark_type,
                    register,
                    options: ctx.options,
                    custom_operators: ctx.providers().custom_operators,
                })
                .effects,
            );
            jump_effects.extend(op_effects);
            jump_effects
        }

        // Operator + find motion (dfo, dta, cf;, etc.)
        Command::CharCommand {
            count,
            command,
            ref target,
            operator: Some(op),
            register,
        } => {
            let count = count.get();
            let ch = target.chars().next().unwrap_or('\0');

            append_change_marks(
                dispatch_operator_find(&OperatorFindInput {
                    operator: op,
                    cmd: command,
                    char: ch,
                    count,
                    register,
                    text,
                    cursor,
                    options: ctx.options,
                    custom_operators: ctx.providers().custom_operators,
                })
                .effects,
            )
        }

        // ═══════════════════════════════════════════════════════════════════
        // Sneak commands (sab, Sab, dsab, etc.)
        // ═══════════════════════════════════════════════════════════════════
        Command::Sneak {
            count,
            c1,
            c2,
            forward,
            operator,
            register,
        } => {
            use crate::commands::motions::find::{sneak_backward, sneak_forward};
            use crate::commands::motions::{MotionContext as MC, MotionResult};
            use crate::primitives::FindDirection;

            let direction = if forward {
                FindDirection::SneakForward
            } else {
                FindDirection::SneakBackward
            };

            let motion_ctx = MC::new(text, Offset::new(cursor), count.get(), ctx.options);
            let motion_result = if forward {
                sneak_forward(&motion_ctx, c1, c2)
            } else {
                sneak_backward(&motion_ctx, c1, c2)
            };

            // Resolve case flags now so `;`/`,` repeats use the same case sensitivity.
            let ic = ctx.options.ignorecase();
            let sc = ctx.options.smartcase();

            if let Some(op) = operator {
                // Operator + sneak (dsab, csab)
                let mut effects =
                    Effects::new().set_last_find_sneak_with_case(direction, c1, c2, ic, sc);
                if let MotionResult::Position(target) = motion_result {
                    use crate::commands::operators::range::compute_find_range;
                    let range_result = compute_find_range(text, cursor, target.get());
                    let mut op_ctx = crate::dispatch::OperatorContext::new(
                        text,
                        range_result.range,
                        range_result.motion_type,
                        register,
                        count.get(),
                        Offset::new(cursor),
                    )
                    .with_motion_target(target)
                    .with_textwidth(ctx.options.textwidth())
                    .with_format_options(ctx.options)
                    .with_commentstring(ctx.options.commentstring());
                    if let Some(provider) = ctx.providers().custom_operators {
                        op_ctx = op_ctx.with_custom_operators(provider);
                    }
                    let result = crate::dispatch::dispatch_operator(op, &op_ctx);
                    effects.extend(result.effects);
                }
                append_change_marks(effects)
            } else {
                // Standalone sneak — just move cursor
                let mut effects =
                    Effects::new().set_last_find_sneak_with_case(direction, c1, c2, ic, sc);
                if let MotionResult::Position(target) = motion_result {
                    // Visual mode: extend selection
                    let selection = ctx.selection();
                    if let Some(sel) = selection.as_ref() {
                        let shape = ctx
                            .state
                            .mode()
                            .visual_type()
                            .map_or(SelectionShape::Char, SelectionShape::from);
                        effects.extend(crate::commands::visual::extend_selection(
                            sel, target, shape,
                        ));
                        let tabstop = ctx.options.tabstop();
                        let column = Some(crate::primitives::VirtualColumn::new(
                            crate::commands::helpers::curswant_of(text, target.get(), tabstop),
                        ));
                        effects.push(crate::effects::Effect::SetStickyColumn { column });
                    } else {
                        effects = effects.set_cursor(target);
                        let tabstop = ctx.options.tabstop();
                        let column = Some(crate::primitives::VirtualColumn::new(
                            crate::commands::helpers::curswant_of(text, target.get(), tabstop),
                        ));
                        effects.push(crate::effects::Effect::SetStickyColumn { column });
                    }
                }
                prepend_visual_exit_marks(effects, ctx, text)
            }
        }

        // ═══════════════════════════════════════════════════════════════════
        // Action commands (p, P, x, X, J, etc.)
        // ═══════════════════════════════════════════════════════════════════
        Command::Action {
            count,
            action,
            register,
        } => {
            let count_u32 = count.get();
            // Jump list navigation: peek at target position (read-only), then emit
            // SetCursor to move + JumpOlder/Newer so effect_processor updates state.
            // For cross-buffer jumps, emit JumpToBuffer instead of SetCursor.
            match action {
                Action::JumpOlder => {
                    // Mirrors Neovim's get_jumplist(win, -count):
                    //  1. cleanup_jumplist (dedup, phantom removal)
                    //  2. bounds pre-check
                    //  3. if at present: setpcmark + skip new entry + re-check
                    //  4. idx -= count; jump to entry[idx]
                    let cursor_off = Offset::new(ctx.cursor_offset());
                    let buf_id = ctx.state.current_buffer_id();
                    let mut jl_sim = ctx.state.jump_list().clone();
                    let doc = ctx.doc();
                    jl_sim.cleanup(Some(cursor_off), |off| doc.line_of_offset(off.get()));

                    if jl_sim.is_empty() {
                        return Effects::new();
                    }

                    let at_present = jl_sim.position() == jl_sim.len();

                    // Pre-check: can we navigate `count` steps from current?
                    if jl_sim.position() < count_u32 as usize && !at_present {
                        return Effects::new();
                        // At present: still need to try after save-present
                    }

                    if at_present {
                        // Save present position so Ctrl-I can return.
                        jl_sim.push(cursor_off, buf_id);
                        // Skip the just-added entry (Neovim: --idx).
                        let new_pos = jl_sim.position().saturating_sub(1);
                        // Check: can we navigate count from new_pos?
                        let new_pos_u32 = byte_delta::to_u32(new_pos);
                        if new_pos_u32 < count_u32 {
                            // Can't navigate far enough; Neovim returns NULL (no movement).
                            // But we still commit the setpcmark push.
                            // Emit PushJumpList so the engine's JL gets the save,
                            // then emit JumpOlder to sync the position.
                            // Since the cursor doesn't move, we emit no SetCursor.
                            //
                            // Neovim leaves idx at 0 after abort, so the effect
                            // processor needs to advance to match. The number of
                            // older() calls needed = current position after push - 0
                            // = jl_sim.position() (which is len after push).
                            // But since we can't actually move the cursor, just
                            // push the save and sync with the count that reaches 0.
                            let sync_count = byte_delta::to_u32(jl_sim.position());
                            return Effects::new()
                                .push_jump_list(cursor_off)
                                .jump_older(sync_count);
                        }
                        // Navigate: target_idx = new_pos - count
                        let target_idx = new_pos - count_u32 as usize;
                        if let Some(entry) = jl_sim.entries().get(target_idx).copied() {
                            let nav_count = byte_delta::to_u32(jl_sim.position() - target_idx);
                            let mut effects = Effects::new().push_jump_list(cursor_off);
                            let nav = jump_effects_for_entry(&entry, ctx.state.current_buffer_id());
                            effects.extend(nav);
                            return effects.jump_older(nav_count);
                        }
                        return Effects::new();
                    }

                    // Not at present: navigate count steps back.
                    if let Some(entry) = jl_sim.peek_older(count_u32) {
                        let effects = jump_effects_for_entry(&entry, ctx.state.current_buffer_id());
                        return effects.jump_older(count_u32);
                    }
                    return Effects::new();
                }
                Action::JumpNewer => {
                    // Mirrors Neovim's get_jumplist(win, +count).
                    // Strict bounds: if idx + count >= len, return nothing.
                    let cursor_off = Offset::new(ctx.cursor_offset());
                    let mut jl_sim = ctx.state.jump_list().clone();
                    let doc = ctx.doc();
                    jl_sim.cleanup(Some(cursor_off), |off| doc.line_of_offset(off.get()));

                    let pos = jl_sim.position();
                    let len = jl_sim.len();

                    // Neovim check: idx + count < jumplistlen
                    if pos + count_u32 as usize >= len {
                        return Effects::new();
                    }

                    let target_idx = pos + count_u32 as usize;
                    if let Some(entry) = jl_sim.entries().get(target_idx).copied() {
                        let nav = jump_effects_for_entry(&entry, ctx.state.current_buffer_id());
                        return nav.jump_newer(count_u32);
                    }
                    return Effects::new();
                }
                Action::IntentRepeat => {
                    let last_intent = ctx.state.repeat_state().last_intent();
                    return crate::commands::actions::intent_repeat::execute_intent_repeat(
                        last_intent,
                    );
                }
                Action::RepeatSubstitute => {
                    // & in normal mode: repeat last substitution on current line (no flags)
                    return super::executor_ex::execute_repeat_substitute(ctx, false);
                }
                Action::RepeatSubstituteGlobal => {
                    // g& in normal mode: repeat last substitution on all lines with original flags
                    return super::executor_ex::execute_repeat_substitute(ctx, true);
                }
                _ => {}
            }

            let cursor_offset = Offset::new(cursor);
            let mut action_ctx = ActionContext::from_text_and_cursor(text, cursor_offset, count)
                .with_shift_width(ctx.options.shiftwidth())
                .with_tab_options(ctx.options.tabstop(), ctx.options.expandtab());
            if let Some(tree) = ctx.doc().vim_text_tree() {
                action_ctx = action_ctx.with_tree(tree);
            }

            if let Some(selection) = ctx.selection() {
                action_ctx = action_ctx.with_selection(selection);
                if let Some(vt) = ctx.state.mode().visual_type() {
                    action_ctx = action_ctx.with_visual_type(vt);
                }
            } else if matches!(action, Action::BlockInsert | Action::BlockAppend) {
                // Dot-repeat for block insert/append: reconstruct selection
                // from LastVisualInfo so the block replication works.
                if let Some(last_visual) = ctx.state.last_visual() {
                    if last_visual.visual_type() == VisualType::Block {
                        if let Some(sel) = reconstruct_block_selection(text, cursor, &last_visual) {
                            action_ctx = action_ctx.with_selection(sel);
                            action_ctx = action_ctx.with_visual_type(VisualType::Block);
                        }
                    }
                }
            }

            // Block visual $A: detect MAXCOL (END_OF_LINE) sticky column and
            // set dollar_mode so block append inserts at end of each line.
            if matches!(action, Action::BlockAppend)
                && ctx
                    .state
                    .sticky_column()
                    .is_some_and(VirtualColumn::is_end_of_line)
            {
                action_ctx = action_ctx.with_dollar_mode();
            }

            if let Some(reg) = register {
                action_ctx = action_ctx.with_register_name(reg);
            }
            let content_reg = register.unwrap_or(RegisterName::UNNAMED);
            if let Some(content) = ctx.state.registers().get_aliased(content_reg, ctx.options) {
                action_ctx = action_ctx.with_register(content);
            }

            let action_effects = dispatch_action(action, &action_ctx).effects;
            prepend_visual_exit_marks(action_effects, ctx, text)
        }

        // ═══════════════════════════════════════════════════════════════════
        // Mark commands (m{a-z}, '{a-z}, `{a-z})
        // ═══════════════════════════════════════════════════════════════════
        Command::Mark {
            count: _,
            mark_type,
            mark,
        } => {
            // Only compute relative topline offset for mark-set (not jumps).
            // topline_offset = cursor_line - viewport_first_line
            let topline_offset = if mark_type == MarkType::Set {
                ctx.viewport().map(|vp| {
                    let cursor_line = crate::commands::helpers::line_of(text, cursor);
                    crate::primitives::byte_delta::delta_i32(cursor_line, vp.first_line)
                })
            } else {
                None
            };
            let mark_ctx =
                MarkContext::with_topline_offset(mark, Offset::new(cursor), topline_offset);

            // ── Cross-buffer global mark resolution ──────────────────────
            // For global marks (A-Z) being jumped to, check if the mark
            // belongs to a different buffer. If so, emit JumpToBuffer
            // instead of moving the cursor locally — the host handles the
            // buffer switch and cursor positioning.
            if mark.is_global() && matches!(mark_type, MarkType::JumpLine | MarkType::JumpExact) {
                if let Some(entry) = ctx.state.marks().get_global(mark) {
                    let current_bid = ctx.state.current_buffer_id();
                    if current_bid.is_some() && Some(entry.buffer_id) != current_bid {
                        // Cross-buffer: delegate to host via JumpToBuffer effect.
                        return Effects::new()
                            .push_jump_list(Offset::new(cursor))
                            .jump_to_buffer(entry.buffer_id, entry.mark.offset());
                    }
                }
            }

            let target_mark = ctx.state.marks().get(mark);
            if target_mark.is_none() && mark_type != MarkType::Set {
                return ex_effects::show_error(VimError::MarkNotSet(mark.char()));
            }

            // In visual mode, mark jumps act as motions — they move the head
            // and extend/shrink the selection, rather than just setting cursor.
            if ctx.state.mode().is_visual()
                && matches!(mark_type, MarkType::JumpLine | MarkType::JumpExact)
            {
                if let Some(target) = target_mark {
                    let raw_offset = if target.offset().get() >= text.len() && !text.is_empty() {
                        crate::commands::helpers::prev_char_boundary(text, text.len())
                    } else {
                        target.offset().get()
                    };
                    let final_offset = if mark_type == MarkType::JumpLine {
                        let line_text = crate::commands::helpers::current_line(text, raw_offset);
                        let offset_in_line =
                            crate::commands::helpers::first_non_blank_in_line(line_text);
                        let line_start =
                            crate::commands::helpers::line_start_for_offset(text, raw_offset);
                        line_start + offset_in_line
                    } else {
                        raw_offset
                    };
                    let new_offset = Offset::new(final_offset);
                    if let Some(sel) = ctx.selection() {
                        use crate::commands::visual::selection as vs;
                        use crate::primitives::SelectionShape;
                        let shape = ctx
                            .state
                            .mode()
                            .visual_type()
                            .map_or(SelectionShape::Char, SelectionShape::from);
                        return vs::extend_selection(&sel, new_offset, shape);
                    }
                }
            }

            dispatch_mark(mark_type, &mark_ctx, target_mark, text).effects
        }

        // ═══════════════════════════════════════════════════════════════════
        // Prefix commands (g{key}, z{key}, etc.)
        // ═══════════════════════════════════════════════════════════════════
        Command::Prefix {
            count,
            register: _,
            command: prefix_cmd,
        } => {
            let count_u32 = count.get();
            match prefix_cmd {
                // gi — jump to insert-stop mark and enter Insert mode
                PrefixCommand::GotoInsertStop => {
                    let target = ctx
                        .state
                        .marks()
                        .get(crate::primitives::MarkName::INSERT_STOP)
                        .map(|m| m.offset().get());
                    enter_insert_at(cursor, target, text.len(), count_u32).effects
                }
                // gJ — join without spaces
                PrefixCommand::JoinNoSpace => {
                    let cursor_offset = Offset::new(cursor);
                    let mut action_ctx =
                        ActionContext::from_text_and_cursor(text, cursor_offset, count)
                            .with_shift_width(ctx.options.shiftwidth());
                    if let Some(tree) = ctx.doc().vim_text_tree() {
                        action_ctx = action_ctx.with_tree(tree);
                    }
                    if let Some(sel) = ctx.selection() {
                        // In linewise visual mode, expand selection to cover
                        // full lines so the join operates on all selected lines.
                        let expanded = if matches!(
                            ctx.state.mode(),
                            crate::primitives::Mode::Visual(crate::primitives::VisualType::Line)
                        ) {
                            crate::dispatch::expand_selection_to_lines(text, &sel)
                        } else {
                            sel
                        };
                        action_ctx = action_ctx.with_selection(expanded);
                    }
                    let gj_effects = dispatch_join_no_space(&action_ctx).effects;
                    prepend_visual_exit_marks(gj_effects, ctx, text)
                }
                // z-prefix (scroll) commands
                PrefixCommand::ScrollCenter => Effects::new().center_cursor(),
                PrefixCommand::ScrollTop => Effects::new().cursor_to_top(),
                PrefixCommand::ScrollBottom => Effects::new().cursor_to_bottom(),
                PrefixCommand::FirstNonBlankTop => {
                    first_non_blank_then_scroll(text, cursor, Effects::cursor_to_top)
                }
                PrefixCommand::FirstNonBlankCenter => {
                    first_non_blank_then_scroll(text, cursor, Effects::center_cursor)
                }
                PrefixCommand::FirstNonBlankBottom => {
                    first_non_blank_then_scroll(text, cursor, Effects::cursor_to_bottom)
                }
                // z-prefix (horizontal scroll) commands
                PrefixCommand::ScrollColumnLeft => Effects::new().scroll_left(count_u32),
                PrefixCommand::ScrollColumnRight => Effects::new().scroll_right(count_u32),
                PrefixCommand::ScrollHalfScreenLeft => {
                    Effects::new().scroll_half_screen_left(count_u32)
                }
                PrefixCommand::ScrollHalfScreenRight => {
                    Effects::new().scroll_half_screen_right(count_u32)
                }
                PrefixCommand::ScrollCursorToLeft => Effects::new().scroll_cursor_to_left_edge(),
                PrefixCommand::ScrollCursorToRight => Effects::new().scroll_cursor_to_right_edge(),
                // z-prefix (fold) commands
                PrefixCommand::FoldClose => {
                    use crate::commands::helpers::line_of;
                    use crate::primitives::LineNumber;
                    let line = LineNumber::new(line_of(text, cursor));
                    Effects::new().fold_line(line)
                }
                PrefixCommand::FoldOpen => {
                    use crate::commands::helpers::line_of;
                    use crate::primitives::LineNumber;
                    let line = LineNumber::new(line_of(text, cursor));
                    Effects::new().unfold_line(line)
                }
                PrefixCommand::FoldToggle => {
                    use crate::commands::helpers::line_of;
                    use crate::primitives::LineNumber;
                    let line = LineNumber::new(line_of(text, cursor));
                    Effects::new().toggle_fold(line)
                }
                PrefixCommand::FoldToggleRecursive => {
                    use crate::commands::helpers::line_of;
                    use crate::primitives::LineNumber;
                    let line = LineNumber::new(line_of(text, cursor));
                    Effects::new().toggle_fold_recursive(line)
                }
                PrefixCommand::FoldCloseAll => Effects::new().fold_all(),
                PrefixCommand::FoldOpenAll => Effects::new().unfold_all(),
                // Z-prefix commands — ZZ and ZQ are intercepted by the engine
                // (execute_effect_plan) before reaching the executor, since they
                // require host requests which effects cannot carry. These arms
                // are retained for exhaustiveness with a debug_assert guard.
                PrefixCommand::WriteQuit | PrefixCommand::ForceQuit => {
                    debug_assert!(
                        false,
                        "ZZ/ZQ should be intercepted by engine before reaching executor"
                    );
                    Effects::new()
                }
                // g-prefix informational commands
                PrefixCommand::ShowAscii => {
                    use crate::commands::actions::info;
                    info::show_ascii(text, cursor)
                }
                PrefixCommand::ShowUtf8 => {
                    use crate::commands::actions::info;
                    info::show_utf8(text, cursor)
                }
                // gd — go to definition (LSP navigation)
                PrefixCommand::GotoDefinition => Effects::new().goto_definition(),
                // g Ctrl-A / g Ctrl-X — sequential increment/decrement (visual)
                PrefixCommand::SequentialIncrement | PrefixCommand::SequentialDecrement => {
                    use crate::commands::actions::number;
                    let mut ac =
                        ActionContext::from_text_and_cursor(text, Offset::new(cursor), count);
                    if let Some(tree) = ctx.doc().vim_text_tree() {
                        ac = ac.with_tree(tree);
                    }
                    if let Some(sel) = ctx.selection() {
                        // In linewise visual mode, expand selection to cover full lines
                        // so that numbers on the last selected line are included.
                        let expanded = if matches!(
                            ctx.state.mode(),
                            crate::primitives::Mode::Visual(crate::primitives::VisualType::Line)
                        ) {
                            crate::dispatch::expand_selection_to_lines(text, &sel)
                        } else {
                            sel
                        };
                        ac = ac.with_selection(expanded);
                    }
                    let seq_effects = match prefix_cmd {
                        PrefixCommand::SequentialIncrement => {
                            number::execute_sequential_increment(&ac).effects
                        }
                        _ => number::execute_sequential_decrement(&ac).effects,
                    };
                    prepend_visual_exit_marks(seq_effects, ctx, text)
                }
                // q: / q/ / q? — open command-line history window
                PrefixCommand::OpenExHistory => {
                    let history = ctx
                        .state
                        .command_line()
                        .history_for_prompt(CommandLinePrompt::Ex)
                        .iter()
                        .cloned()
                        .collect();
                    Effects::new().open_command_window(CommandLinePrompt::Ex, history, None)
                }
                PrefixCommand::OpenSearchForwardHistory => {
                    let history = ctx
                        .state
                        .command_line()
                        .history_for_prompt(CommandLinePrompt::SearchForward)
                        .iter()
                        .cloned()
                        .collect();
                    Effects::new().open_command_window(
                        CommandLinePrompt::SearchForward,
                        history,
                        None,
                    )
                }
                PrefixCommand::OpenSearchBackwardHistory => {
                    let history = ctx
                        .state
                        .command_line()
                        .history_for_prompt(CommandLinePrompt::SearchBackward)
                        .iter()
                        .cloned()
                        .collect();
                    Effects::new().open_command_window(
                        CommandLinePrompt::SearchBackward,
                        history,
                        None,
                    )
                }
                // Ctrl-W window commands
                PrefixCommand::WindowSplit => Effects::new().window_split(),
                PrefixCommand::WindowNew => Effects::new().window_new(),
                PrefixCommand::WindowVSplit => Effects::new().window_vsplit(),
                PrefixCommand::WindowClose => Effects::new().window_close(),
                PrefixCommand::WindowOnly => Effects::new().window_only(),
                PrefixCommand::WindowNext => Effects::new().window_next(),
                PrefixCommand::WindowPrev => Effects::new().window_prev(),
                PrefixCommand::WindowMoveLeft => Effects::new().window_move_left(),
                PrefixCommand::WindowMoveRight => Effects::new().window_move_right(),
                PrefixCommand::WindowMoveUp => Effects::new().window_move_up(),
                PrefixCommand::WindowMoveDown => Effects::new().window_move_down(),
                PrefixCommand::WindowEqualSize => Effects::new().window_equal_size(),
                PrefixCommand::WindowIncreaseHeight => {
                    Effects::new().window_increase_height(count_u32)
                }
                PrefixCommand::WindowDecreaseHeight => {
                    Effects::new().window_decrease_height(count_u32)
                }
                PrefixCommand::WindowIncreaseWidth => {
                    Effects::new().window_increase_width(count_u32)
                }
                PrefixCommand::WindowDecreaseWidth => {
                    Effects::new().window_decrease_width(count_u32)
                }
                PrefixCommand::WindowRotateDown => Effects::new().window_rotate_down(),
                PrefixCommand::WindowRotateUp => Effects::new().window_rotate_up(),
                // z-prefix remaining fold commands
                PrefixCommand::FoldOpenRecursive => {
                    use crate::commands::helpers::line_of;
                    use crate::primitives::LineNumber;
                    let line = LineNumber::new(line_of(text, cursor));
                    Effects::new().unfold_line_recursive(line)
                }
                PrefixCommand::FoldCloseRecursive => {
                    use crate::commands::helpers::line_of;
                    use crate::primitives::LineNumber;
                    let line = LineNumber::new(line_of(text, cursor));
                    Effects::new().fold_line_recursive(line)
                }
                PrefixCommand::FoldDelete => {
                    use crate::commands::helpers::line_of;
                    use crate::primitives::LineNumber;
                    let line = LineNumber::new(line_of(text, cursor));
                    Effects::new().delete_fold(line)
                }
                PrefixCommand::FoldDeleteRecursive => {
                    use crate::commands::helpers::line_of;
                    use crate::primitives::LineNumber;
                    let line = LineNumber::new(line_of(text, cursor));
                    Effects::new().delete_fold_recursive(line)
                }
                PrefixCommand::FoldEliminateAll => Effects::new().eliminate_all_folds(),
                PrefixCommand::FoldToggleEnable => Effects::new().toggle_fold_enable(),
                PrefixCommand::FoldDisable => Effects::new().set_fold_enable(false),
                PrefixCommand::FoldEnable => Effects::new().set_fold_enable(true),
                // vim-unimpaired bracket commands (removed — no-op)
                PrefixCommand::InsertBlankAbove | PrefixCommand::InsertBlankBelow => Effects::new(),
                // Incremental syntax selection commands
                PrefixCommand::SelectParentNode
                | PrefixCommand::SelectChildNode
                | PrefixCommand::SelectPrevSibling
                | PrefixCommand::SelectNextSibling
                | PrefixCommand::SelectAllSiblings
                | PrefixCommand::SelectAllChildren => {
                    execute_syntax_selection(prefix_cmd, count_u32, ctx)
                }
                // Sticky sub-mode entry — intercepted by the engine before
                // reaching the executor. No-op fallback for safety.
                PrefixCommand::StickyEnter { .. } => Effects::new(),
                // g-/g+ undo branch navigation — intercepted by the engine
                // (needs undo tree access). No-op fallback for safety.
                PrefixCommand::UndoEarlier | PrefixCommand::UndoLater => Effects::new(),
            }
        }

        // ═══════════════════════════════════════════════════════════════════
        // Visual mode commands
        // ═══════════════════════════════════════════════════════════════════
        cmd @ Command::Visual(_) => execute_visual_command(&cmd, ctx),

        // ═══════════════════════════════════════════════════════════════════
        // Select mode entry (gh, gH, g<Ctrl-H>)
        // ═══════════════════════════════════════════════════════════════════
        Command::SelectEnter { visual_type } => {
            let cursor = Offset::new(ctx.cursor_offset());
            crate::dispatch::dispatch_select_enter(cursor, visual_type).effects
        }

        Command::OperatorSelection { operator, register } => {
            append_change_marks(execute_operator_selection(operator, register, ctx))
        }

        Command::YankTrimmed { register } => {
            append_change_marks(execute_yank_trimmed(register, ctx))
        }

        Command::VisualTextObject {
            count, textobject, ..
        } => execute_visual_textobject(textobject, count.get(), ctx),

        // ═══════════════════════════════════════════════════════════════════
        // Macro commands
        // ═══════════════════════════════════════════════════════════════════
        Command::Macro(MacroKind::Record { register }) => action_effects::start_recording(register),
        Command::Macro(MacroKind::Stop) => action_effects::stop_recording(),
        Command::Macro(MacroKind::Play { register, count }) => {
            action_effects::play_macro(register, count.get())
        }
        // RepeatLastEx is intercepted in execute_effect_plan before the executor.
        // Reaching here means there is no last ex command — show an error.
        Command::Macro(MacroKind::RepeatLastEx { .. }) => {
            crate::commands::ex::effects::show_error(crate::errors::VimError::NoPreviousCommand)
        }

        // ═══════════════════════════════════════════════════════════════════
        // Insert Entry — normal-mode command to enter insert mode.
        // Routes directly to entry::execute, NOT through dispatch_insert.
        // InsertExit is intercepted by mode handler (ModeAction::InsertExit)
        // and never reaches the executor.
        // ═══════════════════════════════════════════════════════════════════
        Command::InsertEntry {
            entry_type,
            count,
            register,
            ..
        } => {
            dispatch_insert_entry(
                text,
                cursor,
                entry_type,
                count.get(),
                register,
                ctx.options.tabstop(),
                ctx.options.autoindent(),
                ctx.providers().indent,
            )
            .effects
        }

        // Insert-mode-specific commands bypass the executor entirely —
        // engine routes them directly to dispatch_insert via
        // execute_insert_command. Single source of truth:
        // Command::is_insert_specific().
        //
        // Also covers intercepted variant:
        // - InsertExit: intercepted by mode handler → ModeAction::InsertExit
        //
        // Guard satisfies logic; trailing wildcard satisfies compiler
        // (Rust can't prove exhaustiveness through runtime guards).
        cmd if cmd.is_insert_specific() || matches!(cmd, Command::InsertExit) => {
            debug_assert!(
                false,
                "Insert command reached executor — engine must route directly: {cmd:?}"
            );
            ex_effects::show_error(VimError::InternalError(
                format!("insert command in executor: {cmd:?}").into(),
            ))
        }

        // ═══════════════════════════════════════════════════════════════════
        // Surround operations (ys/ds/cs)
        // ═══════════════════════════════════════════════════════════════════
        Command::SurroundAdd {
            textobject,
            motion,
            char: ch,
            count,
            ..
        } => {
            use crate::commands::operators::surround::add_surround;

            // Determine the range from motion or text-object.
            // For visual S (both None), use the visual selection range.
            if let Some(textobject) = textobject {
                // Resolve text object range
                use crate::dispatch::{dispatch_textobject_with_count, TextObjectContext};
                let text_ctx = TextObjectContext::new(text, cursor)
                    .with_providers(*ctx.providers())
                    .with_options(ctx.options);
                if let Some(tobj_range) =
                    dispatch_textobject_with_count(textobject, &text_ctx, count.get())
                {
                    let range = Range::from_raw(
                        tobj_range.range.start().get(),
                        tobj_range.range.end().get(),
                    );
                    add_surround(text, range, ch)
                } else {
                    Effects::new()
                }
            } else if let Some(motion) = motion {
                // Resolve motion range
                use crate::commands::operators::range::compute_motion_range;
                use crate::dispatch::dispatch_motion;
                let search = ctx.state.search().pattern().map(|pattern| {
                    let direction = ctx.state.search().direction().into();
                    (pattern, direction)
                });
                let last_find = ctx.last_find();
                if let Some(range_result) = compute_motion_range(
                    text,
                    cursor,
                    motion,
                    count.get(),
                    search,
                    last_find,
                    ctx.options,
                    dispatch_motion,
                    ctx.viewport(),
                ) {
                    let range = range_result.range;
                    add_surround(text, range, ch)
                } else {
                    Effects::new()
                }
            } else {
                // Visual S: use the current visual selection
                if let Some(sel) = ctx.selection() {
                    let start = sel.anchor().get().min(sel.head().get());
                    let end = sel.anchor().get().max(sel.head().get());
                    // For visual selection, include the char under cursor
                    let end = crate::primitives::next_char_boundary(text, end).min(text.len());
                    let range = Range::from_raw(start, end);
                    let mut effects = add_surround(text, range, ch);
                    // Exit visual mode
                    effects = effects.set_mode(Mode::Normal).clear_selection();
                    effects
                } else {
                    Effects::new()
                }
            }
        }

        Command::SurroundDelete { char: ch } => {
            use crate::commands::operators::surround::delete_surround;
            delete_surround(text, cursor, ch)
        }

        Command::SurroundChange { old_char, new_char } => {
            use crate::commands::operators::surround::change_surround;
            change_surround(text, cursor, old_char, new_char)
        }

        // Compiler exhaustiveness — all Command variants are covered above.
        // If a new variant is added to Command without adding a handler,
        // this arm fires with an error message instead of panicking.
        other => {
            debug_assert!(
                false,
                "Unhandled command variant reached executor: {other:?}"
            );
            ex_effects::show_error(VimError::InternalError(
                format!("unhandled command: {other:?}").into(),
            ))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Motion orchestration
// ═══════════════════════════════════════════════════════════════════════════════

/// Execute a motion command, delegating all effect construction to dispatch.
fn execute_motion<D: Document>(
    motion: crate::grammar::types::Motion,
    count: u32,
    explicit_count: bool,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    // Changelist motions: peek at target position (read-only), then emit
    // SetCursor to move + ChangelistOlder/Newer so effect_processor updates state.
    match motion {
        Motion::ChangelistOlder => {
            let cl = ctx.state.changelist();
            if cl.is_empty() {
                return Effects::new().show_error(crate::errors::VimError::ChangelistEmpty);
            }
            // Neovim's get_changelist clamping for g; (count < 0):
            // if n + count < 0: if n == 0 → NULL (E662), else clamp to 0.
            let pos = cl.position();
            let pos_u32 = byte_delta::to_u32(pos);
            let target = if pos_u32 < count {
                // Would undershoot: clamp to 0
                if pos == 0 {
                    return Effects::new().show_error(crate::errors::VimError::AtStartOfChangelist);
                }
                0usize
            } else {
                pos - count as usize
            };
            if let Some(entry) = cl.entries().get(target) {
                let offset = entry[0];
                let nav_steps = byte_delta::to_u32(pos - target);
                return Effects::new()
                    .set_cursor(offset)
                    .changelist_older(nav_steps);
            }
            return Effects::new().show_error(crate::errors::VimError::AtStartOfChangelist);
        }
        Motion::ChangelistNewer => {
            let cl = ctx.state.changelist();
            if cl.is_empty() {
                return Effects::new().show_error(crate::errors::VimError::ChangelistEmpty);
            }
            // Neovim's get_changelist clamping for g, (count > 0):
            // if n + count >= len: if n == len-1 → NULL (E663), else clamp to len-1.
            let pos = cl.position();
            let len = cl.len();
            let target = if pos + (count as usize) >= len {
                let last = len - 1;
                if pos == last {
                    return Effects::new().show_error(crate::errors::VimError::AtEndOfChangelist);
                }
                last
            } else {
                pos + count as usize
            };
            if let Some(entry) = cl.entries().get(target) {
                let offset = entry[0];
                // When clamping from past-end (pos > target), we actually
                // move backward, so emit changelist_older to sync state.
                if target < pos {
                    let nav_steps = byte_delta::to_u32(pos - target);
                    return Effects::new()
                        .set_cursor(offset)
                        .changelist_older(nav_steps);
                }
                let nav_steps = byte_delta::to_u32(target - pos);
                return Effects::new()
                    .set_cursor(offset)
                    .changelist_newer(nav_steps);
            }
            return Effects::new().show_error(crate::errors::VimError::AtEndOfChangelist);
        }
        _ => {}
    }

    let cursor_offset = resolve_motion_cursor(ctx);
    let motion_ctx = build_motion_context(ctx, cursor_offset, count, explicit_count, None);

    // Populate search count cache from VimState for n/N cache-hit path.
    let search_count_cache = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        if let Some(pat) = ctx.state.search().pattern() {
            pat.hash(&mut hasher);
        }
        ctx.state.search_count_cache(hasher.finish()).copied()
    };

    let mctx = MotionEffectsContext {
        motion,
        motion_ctx,
        selection: ctx.selection(),
        mode: ctx.state.mode(),
        cursor_offset: crate::primitives::Offset::new(cursor_offset),
        search_count_cache,
    };

    dispatch_motion_with_effects(mctx).effects
}

/// Resolve the effective cursor: selection head in visual, else cursor offset.
const fn resolve_motion_cursor<D: Document>(ctx: &ExecutionContext<'_, D>) -> usize {
    if let Some(selection) = ctx.selection() {
        selection.head().get()
    } else {
        ctx.cursor_offset()
    }
}

/// Build a MotionContext with all optional fields populated.
fn build_motion_context<'ctx, D: Document>(
    ctx: &'ctx ExecutionContext<'ctx, D>,
    cursor_offset: usize,
    count: u32,
    explicit_count: bool,
    line_index: Option<&'ctx crate::commands::line_index::LineIndex>,
) -> MotionContext<'ctx> {
    let mut motion_ctx = MotionContext::new(
        ctx.doc().text(),
        crate::primitives::Offset::new(cursor_offset),
        count,
        ctx.options,
    )
    .with_explicit_count(explicit_count);

    if let Some(idx) = line_index {
        motion_ctx = motion_ctx.with_line_index(idx);
    }
    if let Some(col) = ctx.state.sticky_column() {
        motion_ctx = motion_ctx.with_sticky_column(col);
    }
    if let Some(last_find) = ctx.last_find() {
        motion_ctx = motion_ctx.with_last_find(last_find);
    }
    if let Some(pattern) = ctx.state.search().pattern() {
        let direction = ctx.state.search().direction().into();
        motion_ctx = motion_ctx
            .with_search(pattern, direction)
            .with_search_offset(ctx.state.search().offset());
    }
    if let Some(viewport) = ctx.viewport() {
        motion_ctx = motion_ctx.with_viewport(viewport);
    }
    if ctx.providers().has_any() {
        motion_ctx = motion_ctx.with_providers(*ctx.providers());
    }
    if let Some(sticky) = ctx.state.scroll_half_count() {
        motion_ctx.scroll_half_count = Some(sticky);
    }

    // Populate local marks (a-z) for ]' / [' mark navigation motions.
    {
        use crate::primitives::MarkName;
        let mut local_marks = [None; 26];
        for (i, slot) in local_marks.iter_mut().enumerate() {
            let name = MarkName::from_local_index(i);
            *slot = ctx
                .state
                .marks()
                .get(name)
                .map(crate::primitives::Mark::offset);
        }
        motion_ctx = motion_ctx.with_local_marks(local_marks);
    }

    if let Some(tree) = ctx.doc().vim_text_tree() {
        motion_ctx = motion_ctx.with_tree(tree);
    }

    motion_ctx
}

// ═══════════════════════════════════════════════════════════════════════════════
// Visual mode orchestration
// ═══════════════════════════════════════════════════════════════════════════════

/// Execute a visual mode command (enter, exit, switch, swap ends, reselect, toggle select).
fn execute_visual_command<D: Document>(cmd: &Command, ctx: &ExecutionContext<'_, D>) -> Effects {
    // ToggleSelect is handled here because it needs the full Mode (Visual vs Select),
    // which the VisualContext/dispatch layer doesn't carry.
    if matches!(cmd, Command::Visual(VisualKind::ToggleSelect)) {
        return execute_toggle_select(ctx);
    }

    let cursor = Offset::new(ctx.cursor_offset());
    let selection = ctx.selection();

    let current_visual_type = ctx
        .state
        .mode()
        .visual_type()
        .or_else(|| ctx.state.mode().select_type());

    let last_visual = ctx.state.last_visual();
    let last_visual_type = last_visual.map(LastVisualInfo::visual_type);
    let last_visual_lines = last_visual.map(LastVisualInfo::lines);
    let last_visual_cursor_at_start = last_visual.map(LastVisualInfo::cursor_at_start);
    let start_mark = ctx
        .state
        .marks()
        .get(crate::primitives::MarkName::VISUAL_START)
        .map(Mark::offset);
    let end_mark = ctx
        .state
        .marks()
        .get(crate::primitives::MarkName::VISUAL_END)
        .map(Mark::offset);

    let mut visual_ctx = VisualContext::from_cursor_and_selection(
        ctx.doc().text(),
        cursor,
        selection,
        current_visual_type,
        last_visual_type,
        start_mark,
        end_mark,
        last_visual_lines,
        last_visual_cursor_at_start,
    );
    visual_ctx.tabstop = ctx.options.tabstop();
    visual_ctx.fold_provider = ctx.providers().fold;

    dispatch_visual(cmd, &visual_ctx).effects
}

/// Toggle between Visual and Select mode (Ctrl-G).
///
/// Preserves the current selection — only the mode changes.
/// - Visual(vt) → Select(vt)
/// - Select(vt) → Visual(vt)
fn execute_toggle_select<D: Document>(ctx: &ExecutionContext<'_, D>) -> Effects {
    let mode = ctx.state.mode();
    match mode {
        Mode::Visual(vt) => Effects::new().set_mode(Mode::Select(vt)),
        Mode::Select(vt) => Effects::new().set_mode(Mode::Visual(vt)),
        _ => {
            debug_assert!(
                false,
                "ToggleSelect reached from non-visual/select mode: {mode:?}"
            );
            Effects::new()
        }
    }
}

/// Execute a text object in visual mode.
fn execute_visual_textobject<D: Document>(
    textobject: TextObject,
    count: u32,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    let text = ctx.doc().text();
    let selection = ctx.selection();
    let cursor = selection
        .as_ref()
        .map_or_else(|| ctx.cursor_offset(), |s| s.head().get());

    let mode_linewise =
        matches!(ctx.state.mode(), crate::primitives::Mode::Visual(vt) if vt.is_line());
    dispatch_visual_textobject(
        textobject,
        text,
        cursor,
        selection.as_ref(),
        ctx.providers(),
        count,
        mode_linewise,
    )
    .effects
}

// ═══════════════════════════════════════════════════════════════════════════════
// Operator orchestration
// ═══════════════════════════════════════════════════════════════════════════════

/// Execute an operator on a visual selection.
fn execute_operator_selection<D: Document>(
    operator: Operator,
    register: Option<RegisterName>,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    use crate::dispatch::{dispatch_operator_selection, SelectionOperatorContext};

    let text = ctx.doc().text();

    // Block visual mode gets its own path
    if let Some(sel) = ctx.selection() {
        if let Mode::Visual(vt) = ctx.state.mode() {
            if vt.is_block() {
                return execute_block_visual(operator, register, text, &sel, ctx);
            }
        }
    }

    // Dot-repeat for block visual: when replaying OperatorSelection in Normal
    // mode and LastVisualInfo is Block, reconstruct a synthetic block selection
    // and route through the block visual path.
    if ctx.selection().is_none() {
        if let Some(last_visual) = ctx.state.last_visual() {
            if last_visual.visual_type() == VisualType::Block {
                if let Some(sel) =
                    reconstruct_block_selection(text, ctx.cursor_offset(), &last_visual)
                {
                    return execute_block_visual(operator, register, text, &sel, ctx);
                }
            }
        }
    }

    // Resolve range from live selection or LastVisualInfo
    let Some((range, motion_type, cursor_pos)) = resolve_selection_range(text, ctx) else {
        return Effects::new();
    };

    // Build visual exit info for live visual mode
    let visual_exit = ctx.selection().map(|sel| {
        let visual_type = ctx.state.mode().visual_type().unwrap_or(VisualType::Char);
        (visual_type, sel)
    });

    // For FormatKeepCursor (gw), preserve the actual cursor position rather
    // than the range start. Linewise visual resolve_selection_range returns
    // cursor_pos = range.start(), but gw needs the real cursor to keep it.
    let cursor_pos = if operator == Operator::FormatKeepCursor {
        Offset::new(ctx.cursor_offset())
    } else {
        cursor_pos
    };

    let sel_ctx = SelectionOperatorContext {
        text,
        range,
        motion_type,
        register,
        cursor_pos,
        operator,
        visual_exit,
        commentstring: ctx.options.commentstring(),
        custom_operators: ctx.providers().custom_operators,
        shiftwidth: ctx.options.shiftwidth(),
        tabstop: ctx.options.tabstop(),
        expandtab: ctx.options.expandtab(),
        options: ctx.options,
        viewport: ctx.viewport(),
        sticky_column: ctx.state.sticky_column(),
    };

    let mut result_effects = dispatch_operator_selection(&sel_ctx).effects;

    // Neovim post-operator adjustment (ops.c:3897-3907):
    // For linewise shift with nostartofline (default), restore the cursor
    // to oap->start line at old_col via coladvance(old_col).
    // oap->start = visual anchor (where V was pressed) or min(cursor, motion_target).
    if motion_type.is_line_wise() && matches!(operator, Operator::Indent | Operator::Outdent) {
        // oap->start = min(VIsual, cursor) = min(anchor, head) for visual,
        // or range.start() for non-visual (>>).
        let oap_start = ctx
            .selection()
            .map_or(cursor_pos, crate::primitives::SelectionRange::start)
            .get();
        let oap_start_line = crate::commands::helpers::line_start_for_offset(text, oap_start);
        let old_col = ctx.state.sticky_column().map_or_else(
            || {
                let cur = ctx.cursor_offset();
                let cur_line_start = crate::commands::helpers::line_start_for_offset(text, cur);
                cur.saturating_sub(cur_line_start)
            },
            crate::primitives::VirtualColumn::get,
        );
        let line_text = &text[oap_start_line..];
        let line_len = line_text.find('\n').unwrap_or(line_text.len());
        let clamped_col = old_col.min(line_len.saturating_sub(1));
        let new_cursor = crate::primitives::Offset::new(oap_start_line + clamped_col);
        result_effects = result_effects.set_cursor(new_cursor);
    }

    result_effects
}

/// Resolve operator range from either live selection or LastVisualInfo.
pub(crate) fn resolve_selection_range<D: Document>(
    text: &str,
    ctx: &ExecutionContext<'_, D>,
) -> Option<(Range, MotionType, Offset)> {
    use crate::dispatch::{reconstruct_from_last_visual, resolve_live_selection};

    if let Some(sel) = ctx.selection() {
        Some(resolve_live_selection(
            text,
            &sel,
            ctx.state.mode(),
            ctx.options.selection_is_exclusive(),
        ))
    } else {
        ctx.state.last_visual().map(|last_visual| {
            reconstruct_from_last_visual(
                text,
                ctx.cursor_offset(),
                &last_visual,
                ctx.options.tabstop(),
            )
        })
    }
}

/// Execute `zy` — yank with trailing whitespace trimmed from register parts.
///
/// In block visual mode, trims trailing whitespace from each line of the
/// yanked text before writing to the register. In char/line visual mode,
/// falls through to a regular yank (trim has no effect on non-block yank).
fn execute_yank_trimmed<D: Document>(
    register: Option<RegisterName>,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    use crate::effects::Effect;

    let effects = execute_operator_selection(Operator::Yank, register, ctx);

    // Post-process: find SetRegister effects and trim trailing whitespace
    // from each line of the block register text.
    let mut result = Effects::new();
    for effect in effects {
        match effect {
            Effect::SetRegister {
                name,
                text,
                motion_type,
            } if motion_type.is_block_wise() => {
                let trimmed_text: String = text
                    .lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n");
                result.push(Effect::SetRegister {
                    name,
                    text: compact_str::CompactString::from(trimmed_text),
                    motion_type,
                });
            }
            other => result.push(other),
        }
    }
    result
}

/// Execute an operator on a block visual selection.
fn execute_block_visual<D: Document>(
    operator: Operator,
    register: Option<RegisterName>,
    text: &str,
    selection: &crate::primitives::SelectionRange,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    use crate::dispatch::block_visual;

    let ve = ctx.options.virtualedit();
    let ve_block = ve.contains("block") || ve.contains("all");
    #[allow(unused_mut)]
    let mut op_ctx = OperatorContext::for_block(text, register, Offset::new(ctx.cursor_offset()))
        .with_shiftwidth(ctx.options.shiftwidth())
        .with_tabstop(ctx.options.tabstop())
        .with_textwidth(ctx.options.textwidth())
        .with_format_options(ctx.options)
        .with_commentstring(ctx.options.commentstring())
        .with_virtualedit_block(ve_block);
    if let Some(tree) = ctx.doc().vim_text_tree() {
        op_ctx = op_ctx.with_tree(tree);
    }
    block_visual::execute_block_operator(operator, &op_ctx, selection).effects
}

/// Reconstruct a [`SelectionRange`] for dot-repeat of a block visual operator.
///
/// Uses `LastVisualInfo` (lines + columns) from the previous visual session
/// to synthesise anchor/head offsets relative to the current cursor position.
fn reconstruct_block_selection(
    text: &str,
    cursor: usize,
    last_visual: &LastVisualInfo,
) -> Option<crate::primitives::SelectionRange> {
    use crate::commands::helpers::{line_end, line_of, line_start};
    use unicode_segmentation::UnicodeSegmentation;

    let cursor_line = line_of(text, cursor);
    let cursor_ls = line_start(text, cursor_line)?;
    let cursor_gcol = text[cursor_ls..cursor].graphemes(true).count();

    // Anchor = cursor position (top-left of block)
    let anchor = Offset::new(cursor);

    // Head = last_visual.lines()-1 lines below, at cursor_gcol + columns
    let target_line = cursor_line + last_visual.lines().saturating_sub(1);
    let target_line_start = line_start(text, target_line)?;
    let target_line_end = line_end(text, target_line).unwrap_or(text.len());
    let target_graphemes: Vec<&str> = text[target_line_start..target_line_end]
        .graphemes(true)
        .collect();
    let target_gcol =
        (cursor_gcol + last_visual.columns()).min(target_graphemes.len().saturating_sub(1));
    let head_byte: usize = target_graphemes
        .get(..=target_gcol)
        .unwrap_or(&[])
        .iter()
        .map(|g| g.len())
        .sum::<usize>()
        + target_line_start;
    // head should point at the last char, not past it
    let head_byte = if head_byte > target_line_start {
        // Back up by one grapheme
        let last_g_len = target_graphemes.get(target_gcol).map_or(0, |g| g.len());
        head_byte - last_g_len
    } else {
        target_line_start
    };

    let head = Offset::new(head_byte.min(text.len()));
    Some(crate::primitives::SelectionRange::new(anchor, head))
}

/// Build effects for a jump list navigation target.
///
/// If the target entry is in a different buffer, emits `JumpToBuffer` so the
/// shell can switch buffers and place the cursor. Otherwise emits `SetCursor`
/// for same-buffer navigation.
fn jump_effects_for_entry(
    entry: &crate::state::JumpEntry,
    current_buffer_id: Option<crate::primitives::BufferId>,
) -> Effects {
    if let Some(bid) = entry.buffer_id() {
        if Some(bid) != current_buffer_id {
            // Cross-buffer: shell handles buffer switch + cursor
            return Effects::new().jump_to_buffer(bid, entry.offset());
        }
    }
    // Same buffer (or no buffer tracking): move cursor directly
    Effects::new().set_cursor(entry.offset())
}

/// Compute the first non-blank character offset on the current line.
///
/// Used by z<CR>, z., z- which scroll AND move cursor to first non-blank.
fn first_non_blank_cursor(text: &str, cursor: usize) -> usize {
    use crate::commands::helpers::{first_non_blank_in_line, line_content, line_of, line_start};
    let line = line_of(text, cursor);
    let ls = line_start(text, line).unwrap_or(0);
    if let Some(content) = line_content(text, line) {
        ls + first_non_blank_in_line(content)
    } else {
        ls
    }
}

/// Move to first non-blank of current line, then apply a scroll positioning effect.
///
/// Shared logic for `z<CR>`, `z.`, `z-` — which differ only in scroll direction.
fn first_non_blank_then_scroll(
    text: &str,
    cursor: usize,
    scroll: fn(Effects) -> Effects,
) -> Effects {
    let fnb = first_non_blank_cursor(text, cursor);
    let mut effects = Effects::new();
    if fnb != cursor {
        effects = effects.set_cursor(Offset::new(fnb));
    }
    scroll(effects)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Incremental syntax selection
// ═══════════════════════════════════════════════════════════════════════════════

/// Execute an incremental syntax selection command (g[, g], g{, g}, g(, g)).
fn execute_syntax_selection<D: Document>(
    prefix_cmd: PrefixCommand,
    count: u32,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    let Some(syntax) = ctx.providers().syntax else {
        return Effects::new();
    };
    let text = ctx.doc().text();
    let (sel_start, sel_end) = if let Some(sel) = ctx.selection() {
        let (a, h) = (sel.anchor().get(), sel.head().get());
        (a.min(h), a.max(h))
    } else {
        (ctx.cursor_offset(), ctx.cursor_offset())
    };
    match prefix_cmd {
        PrefixCommand::SelectParentNode => {
            execute_syntax_expand_parent(syntax, text, sel_start, sel_end, count, ctx)
        }
        PrefixCommand::SelectChildNode => {
            execute_syntax_shrink_child(syntax, text, sel_start, sel_end, count, ctx)
        }
        PrefixCommand::SelectNextSibling => {
            execute_syntax_sibling(syntax, text, sel_start, sel_end, count, true, ctx)
        }
        PrefixCommand::SelectPrevSibling => {
            execute_syntax_sibling(syntax, text, sel_start, sel_end, count, false, ctx)
        }
        PrefixCommand::SelectAllSiblings => {
            execute_syntax_fan_out(syntax, text, sel_start, sel_end, true, ctx)
        }
        PrefixCommand::SelectAllChildren => {
            execute_syntax_fan_out(syntax, text, sel_start, sel_end, false, ctx)
        }
        _ => Effects::new(),
    }
}

/// Expand selection to parent syntax node(s), pushing history.
fn execute_syntax_expand_parent<D: Document>(
    syntax: &dyn crate::document::SyntaxProvider,
    text: &str,
    _sel_start: usize,
    _sel_end: usize,
    count: u32,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    let is_visual = ctx.state.mode().is_visual();

    // Build current Selections from all cursors (multi-cursor aware)
    let current_selections = ctx.state.multi_cursor().selections().clone();

    // Expand each cursor independently
    let doc_len = text.len();
    let expanded = {
        let mut sels = current_selections.clone();
        for _ in 0..count {
            let prev = sels.clone();
            sels = sels
                .transform(|range| {
                    let (s, e) = (range.start().get(), range.end().get());
                    match syntax.ancestor_node(text, s, e) {
                        Some((ns, ne)) => {
                            let r = validated_range(ns, ne, doc_len);
                            if range.is_forward() {
                                crate::primitives::SelectionRange::new(r.start(), r.end())
                            } else {
                                crate::primitives::SelectionRange::new(r.end(), r.start())
                            }
                        }
                        None => range,
                    }
                })
                .normalize();
            if sels.ranges() == prev.ranges() {
                break;
            }
        }
        sels
    };

    if expanded.ranges() == current_selections.ranges() {
        return Effects::new();
    }

    let mut effects = Effects::new();
    effects = effects.syntax_selection_push(current_selections);
    effects = effects.set_syntax_selections(expanded.clone());

    if !is_visual {
        effects = effects.set_mode(Mode::Visual(crate::primitives::VisualType::Char));
    }

    let primary = expanded.primary();
    effects.set_visual_selection(primary.anchor(), primary.head(), SelectionShape::Char)
}

/// Shrink selection to child syntax node (pop history or query provider).
fn execute_syntax_shrink_child<D: Document>(
    syntax: &dyn crate::document::SyntaxProvider,
    text: &str,
    _sel_start: usize,
    _sel_end: usize,
    count: u32,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    let mut effects = Effects::new();
    let is_visual = ctx.state.mode().is_visual();
    let history = ctx.state.syntax_selection();

    // Build current Selections from all cursors (multi-cursor aware)
    let current = ctx.state.multi_cursor().selections().clone();

    // Try to pop from history with containment validation
    let mut restored: Option<crate::primitives::Selections> = None;

    if let Some(peeked) = history.peek() {
        if crate::state::selections_contained_by(peeked, &current) {
            restored = Some(peeked.clone());
            effects = effects.syntax_selection_pop();
        } else {
            effects = effects.syntax_history_clear();
        }
    }

    if let Some(ref sels) = restored {
        // Restore full multi-cursor state from history
        effects = effects.set_syntax_selections(sels.clone());

        if !is_visual {
            effects = effects.set_mode(Mode::Visual(crate::primitives::VisualType::Char));
        }

        let primary = sels.primary();
        return effects.set_visual_selection(
            primary.anchor(),
            primary.head(),
            SelectionShape::Char,
        );
    }

    // No valid history — fallback to descendant_node per cursor
    let doc_len = text.len();
    let shrunk = {
        let mut sels = current.clone();
        for _ in 0..count {
            let prev = sels.clone();
            sels = sels
                .transform(|range| {
                    let (s, e) = (range.start().get(), range.end().get());
                    match syntax.descendant_node(text, s, e) {
                        Some((ds, de)) => {
                            let r = validated_range(ds, de, doc_len);
                            if range.is_forward() {
                                crate::primitives::SelectionRange::new(r.start(), r.end())
                            } else {
                                crate::primitives::SelectionRange::new(r.end(), r.start())
                            }
                        }
                        None => range,
                    }
                })
                .normalize();
            if sels.ranges() == prev.ranges() {
                break;
            }
        }
        sels
    };

    if shrunk.ranges() == current.ranges() {
        return effects;
    }

    effects = effects.set_syntax_selections(shrunk.clone());

    if !is_visual {
        effects = effects.set_mode(Mode::Visual(crate::primitives::VisualType::Char));
    }

    let primary = shrunk.primary();
    effects.set_visual_selection(primary.anchor(), primary.head(), SelectionShape::Char)
}

/// Navigate to next/previous sibling syntax node.
fn execute_syntax_sibling<D: Document>(
    syntax: &dyn crate::document::SyntaxProvider,
    text: &str,
    _sel_start: usize,
    _sel_end: usize,
    count: u32,
    forward: bool,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    let is_visual = ctx.state.mode().is_visual();
    let doc_len = text.len();

    let current = ctx.state.multi_cursor().selections().clone();

    let navigated = current
        .clone()
        .transform(|range| {
            let (mut s, mut e) = (range.start().get(), range.end().get());
            for _ in 0..count {
                let result = if forward {
                    syntax.next_sibling_node(text, s, e)
                } else {
                    syntax.prev_sibling_node(text, s, e)
                };
                match result {
                    Some((ns, ne)) => {
                        let r = validated_range(ns, ne, doc_len);
                        s = r.start().get();
                        e = r.end().get();
                    }
                    None => break,
                }
            }
            if s == range.start().get() && e == range.end().get() {
                return range;
            }
            if forward {
                crate::primitives::SelectionRange::new(Offset::new(s), Offset::new(e))
            } else {
                crate::primitives::SelectionRange::new(Offset::new(e), Offset::new(s))
            }
        })
        .normalize();

    if navigated.ranges() == current.ranges() {
        return Effects::new();
    }

    let mut effects = Effects::new();
    effects = effects.set_syntax_selections(navigated.clone());

    if !is_visual {
        effects = effects.set_mode(Mode::Visual(crate::primitives::VisualType::Char));
    }

    let primary = navigated.primary();
    effects.set_visual_selection(primary.anchor(), primary.head(), SelectionShape::Char)
}

/// Fan-out: select all sibling or child syntax nodes.
fn execute_syntax_fan_out<D: Document>(
    syntax: &dyn crate::document::SyntaxProvider,
    text: &str,
    _sel_start: usize,
    _sel_end: usize,
    siblings: bool,
    ctx: &ExecutionContext<'_, D>,
) -> Effects {
    let is_visual = ctx.state.mode().is_visual();
    let doc_len = text.len();

    let current = ctx.state.multi_cursor().selections().clone();

    let fanned = current.clone().transform_iter(|range| {
        let (s, e) = (range.start().get(), range.end().get());
        let nodes = if siblings {
            syntax.sibling_nodes(text, s, e)
        } else {
            syntax.child_nodes(text, s, e)
        };
        if nodes.len() <= 1 {
            vec![range]
        } else {
            nodes
                .iter()
                .map(|&(ns, ne)| {
                    let r = validated_range(ns, ne, doc_len);
                    if range.is_forward() {
                        crate::primitives::SelectionRange::new(r.start(), r.end())
                    } else {
                        crate::primitives::SelectionRange::new(r.end(), r.start())
                    }
                })
                .collect()
        }
    });

    if fanned.ranges() == current.ranges() {
        return Effects::new();
    }

    let mut effects = Effects::new();
    effects = effects.syntax_selection_push(current);
    effects = effects.set_syntax_selections(fanned.clone());

    if !is_visual {
        effects = effects.set_mode(Mode::Visual(crate::primitives::VisualType::Char));
    }

    let primary = fanned.primary();
    effects.set_visual_selection(primary.anchor(), primary.head(), SelectionShape::Char)
}

// ═══════════════════════════════════════════════════════════════════════
// Change marks (`[` and `]`) — auto-set after operator execution
// ═══════════════════════════════════════════════════════════════════════

/// Append `[` (CHANGE_START) and `]` (CHANGE_END) marks to effects after
/// an operator executes. Scans the effects for mutation primitives
/// (Delete, Replace, Insert) and derives the changed region.
///
/// This is a central post-processing step so individual operators don't
/// need to emit these marks themselves. (Yank already emits them, but
/// the duplicates are harmless — the last SetMark wins.)
fn append_change_marks(mut effects: Effects) -> Effects {
    use crate::effects::Effect;

    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;

    for effect in effects.iter() {
        match effect {
            Effect::Delete { range } => {
                let s = range.start().get();
                // After a delete, both marks point at the deletion point
                let e = s;
                start = Some(start.map_or(s, |prev: usize| prev.min(s)));
                end = Some(end.map_or(e, |prev: usize| prev.max(e)));
            }
            Effect::Replace { range, text } => {
                let s = range.start().get();
                // For linewise operators (gUU, etc.) the replacement text includes
                // a trailing '\n'. Neovim's `]` mark points to the last character
                // *before* the newline, so we exclude it from the end calculation.
                let effective_len = if text.ends_with('\n') && text.len() > 1 {
                    text.len() - 1
                } else {
                    text.len()
                };
                let e = s + effective_len;
                start = Some(start.map_or(s, |prev: usize| prev.min(s)));
                end = Some(end.map_or(e, |prev: usize| prev.max(e)));
            }
            Effect::Insert { offset, text } => {
                let s = offset.get();
                let e = s + text.len();
                start = Some(start.map_or(s, |prev: usize| prev.min(s)));
                end = Some(end.map_or(e, |prev: usize| prev.max(e)));
            }
            _ => {}
        }
    }

    if let (Some(s), Some(e)) = (start, end) {
        // Skip if the command already set these marks explicitly (e.g., indent
        // operator computes mark.] as end-of-last-line, not end-of-inserted-text).
        let has_explicit_marks = effects.iter().any(|ef| {
            matches!(ef, Effect::SetMark { name, .. }
                if *name == MarkName::CHANGE_START || *name == MarkName::CHANGE_END)
        });
        if !has_explicit_marks {
            // `]` mark points to the last byte of the changed region (inclusive).
            // Clamp end to at least start (empty regions are valid).
            let end_inclusive = if e > s { e.saturating_sub(1) } else { s };
            effects = effects
                .set_mark(MarkName::CHANGE_START, Offset::new(s), None)
                .set_mark(MarkName::CHANGE_END, Offset::new(end_inclusive), None);
        }
    }

    effects
}

// ═══════════════════════════════════════════════════════════════════════
// Visual exit marks — prepend `<`/`>` marks + SaveLastVisual for actions
// ═══════════════════════════════════════════════════════════════════════

/// Prepend visual exit marks (`<`, `>`, `SaveLastVisual`, `ClearSelection`)
/// before `inner` effects when the mode is visual and a selection exists.
///
/// Mirrors the operator path's `dispatch_operator_selection` which calls
/// `visual_exit_with_marks` before dispatching the operator. Actions and
/// char commands handle their own `ClearSelection + SetMode(Normal)` but
/// omit the marks; this helper fills the gap.
fn prepend_visual_exit_marks<D: Document>(
    inner: Effects,
    ctx: &ExecutionContext<'_, D>,
    text: &str,
) -> Effects {
    let Some(sel) = ctx.selection() else {
        return inner;
    };
    if !ctx.state.mode().is_visual() {
        return inner;
    }
    let vt = ctx.state.mode().visual_type().unwrap_or(VisualType::Char);
    let mut combined = crate::commands::visual::selection::visual_exit_with_marks(
        text,
        vt,
        &sel,
        ctx.options.tabstop(),
    );
    combined.extend(inner);
    combined
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "executor_tests.rs"]
mod executor_tests;
