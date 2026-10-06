//! Internal dispatch pipeline for [`VimEngine`].
//!
//! Routes mode actions through the grammar → resolution → execution pipeline.
//! Contains `execute_mode_action` and all its helper methods.

use super::VimEngine;
use crate::commands::actions::effects as action_effects;
use crate::commands::insert::InsertContext;
use crate::document::Document;
use crate::effects::undo_intent::UndoIntent;
use crate::effects::{Effect, EffectProvenance, Effects};
use crate::execution::response::{Response, ResponseKind};
use crate::execution::trace::trace_event;
#[cfg(feature = "engine-tracing")]
use crate::execution::trace::TraceEvent;
use crate::execution::{
    ExecutionContext, HostRequest, InputContext, PipelineError, PlannedAction, Validated,
};
use crate::grammar::{Command, CommandLineIntent, GrammarResult, InsertKind, PrefixCommand};
use crate::mode::{CommandLineResult, InsertMode, ModeAction};
use crate::primitives::byte_delta;
use crate::primitives::{Mode, StickyTarget};
use crate::primitives::{Offset, RegisterName};
use crate::state::InsertState;
use smallvec::SmallVec;

use super::per_cursor::PerCursorRegisterOverride;

impl VimEngine {
    /// Execute engine-side effects for a mode handler's decision.
    ///
    /// Mode handlers **decide**; this method **executes**.
    pub(in crate::execution::engine) fn execute_mode_action<D: Document>(
        &mut self,
        action: ModeAction,
        mode: Mode,
        key: crate::keymap::KeyEvent,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        debug!(target: "vim::mode", "dispatching parsed command (mode={mode:?}, key={key:?})");
        match action {
            ModeAction::Pipeline(result) => self.process_pipeline_result(result, mode, key, ctx),
            ModeAction::InsertExit => {
                self.handle_insert_exit(ctx.cursor_offset().get(), ctx.doc().text())
            }
            ModeAction::InsertCommand {
                command,
                insert_mode,
            } => self.execute_insert_command(command, insert_mode, ctx),
            ModeAction::CommandLine(cl_result) => self.execute_command_line_result(&cl_result, ctx),
            // Select mode: replace selection with typed char.
            ModeAction::SelectReplace { char: ch } => self.execute_select_replace(ch, ctx),
            // Select mode: delete selection (Backspace/Delete).
            // Routes through the Delete operator pipeline (delete selection + Normal mode).
            ModeAction::SelectDelete => self.execute_select_operator(
                Command::OperatorSelection {
                    operator: crate::grammar::types::Operator::Delete,
                    register: None,
                },
                ctx,
            ),
            ModeAction::Pending => {
                let mut response = Response::pending_response();
                if matches!(
                    self.parser.state(),
                    crate::grammar::InputState::AwaitingInsertCtrlX
                ) {
                    response.message = Some(compact_str::CompactString::from(
                        "-- ^X mode (^]^D^E^F^I^K^L^N^O^P^S^T^U^V^Y)",
                    ));
                }
                response
            }
            ModeAction::Ignored => Response::ignored(),
        }
    }

    /// Route a grammar result through the pipeline (classify → resolve → execute).
    fn process_pipeline_result<D: Document>(
        &mut self,
        result: GrammarResult,
        mode: Mode,
        key: crate::keymap::KeyEvent,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        // Classify Invalid/Cancel as pipeline errors
        let result = match super::super::pipeline::classify_parse_result(mode, key, result) {
            Ok(r) => r,
            Err(ref err) => {
                // Neovim: nv_esc() clears restart_edit when Escape is pressed
                // during C-o one-shot normal mode, which exits insert mode
                // entirely (stop_insert is called, setting marks [, ], ^).
                // Replicate: when an escape-class key cancels in Normal mode
                // while return_to is active, consume the return_to and trigger
                // a full insert exit instead of just clearing the message.
                if mode == Mode::Normal && self.state.return_to().is_some() {
                    let is_escape_class = matches!(
                        err,
                        PipelineError::InvalidKey {
                            mode: Mode::Normal,
                            key: k,
                        }
                        | PipelineError::Cancelled {
                            mode: Mode::Normal,
                            key: k,
                        }
                        if k.key() == crate::keymap::Key::Escape
                            || *k == crate::keymap::KeyEvent::ctrl('c')
                            || *k == crate::keymap::KeyEvent::ctrl('[')
                    );
                    if is_escape_class {
                        self.state.take_return_to(); // consume return_to
                        return self
                            .handle_insert_exit(ctx.cursor_offset().get(), ctx.doc().text());
                    }
                }
                return self.handle_pipeline_error(err);
            }
        };

        // Resolve grammar result into (PlannedAction, was_repeat)
        let (action, was_repeat) = match super::super::pipeline::resolve_grammar_result(
            mode,
            result,
            self.parser.was_repeat(),
        ) {
            Ok(pair) => pair,
            Err(ref err) => return self.handle_pipeline_error(err),
        };

        // Reset parser for visual operator-selection (must happen before execute)
        if mode.is_visual()
            && matches!(
                action,
                PlannedAction::Execute(
                    Command::OperatorSelection { .. } | Command::YankTrimmed { .. }
                )
            )
        {
            self.parser.reset();
        }

        // Dispatch the action
        match action {
            PlannedAction::Execute(command) => {
                self.is_repeating = was_repeat;
                // Capture static tag before moving `command` into execute (zero-alloc).
                let command_name = command.tag();
                let keystroke_seq = self.keystroke_seq;
                // Borrow text for Ctrl-O cursor adjustment (only when one-shot is active).
                // `doc()` returns `&'doc D`, so this survives moving `ctx` — no clone needed.
                let text_for_restart = if self.state.return_to().is_some() {
                    Some(ctx.doc().text())
                } else {
                    None
                };
                // Extract sticky group before command is moved into execute.
                let auto_sticky_target = if self.sticky_session.is_none() {
                    if let Command::Prefix {
                        command: ref prefix_cmd,
                        ..
                    } = command
                    {
                        prefix_cmd.sticky_group()
                    } else {
                        None
                    }
                } else {
                    None
                };
                let mut response = match self.execute_effect_plan(command, was_repeat, ctx) {
                    Ok(response) => response,
                    Err(err) => return self.handle_pipeline_error(&err),
                };
                response.provenance = Some(EffectProvenance::new(command_name, keystroke_seq));
                self.maybe_return_to_mode(&mut response, text_for_restart);

                // Auto-activate sticky mode for configured prefixes.
                // If the executed command was a prefix command belonging to a
                // sticky-enabled group and no session is already active, start
                // one now so the FIRST command triggers sticky mode.
                if self.sticky_session.is_none() {
                    if let Some(target) = auto_sticky_target {
                        if self.is_prefix_sticky(target) {
                            self.sticky_session = Some(super::StickySession::new(target));
                            response.extend_effects(
                                crate::effects::Effects::new()
                                    .show_message(format!("-- {} --", target.display_name())),
                            );
                        }
                    }
                }

                self.maybe_reenter_sticky();
                response
            }
            PlannedAction::ModeChange(new_mode, count) => {
                let intent = if new_mode == Mode::CommandLine {
                    self.parser.take_command_line_intent()
                } else {
                    None
                };
                self.handle_mode_change(new_mode, intent, count)
            }
            PlannedAction::Pending => {
                let mut response = Response::pending_response();
                // Emit cursor shape hint when entering operator-pending state.
                if let Some(op) = self.parser.state().pending_operator() {
                    response
                        .effects
                        .push(crate::effects::Effect::CursorShapeHint {
                            pending_operator: Some(op),
                        });
                }
                response
            }
            PlannedAction::Ignored => Response::ignored(),
        }
    }

    fn handle_mode_change(
        &mut self,
        new_mode: Mode,
        intent: Option<CommandLineIntent>,
        count: Option<u32>,
    ) -> Response {
        if new_mode == Mode::CommandLine {
            return self.enter_command_line(intent, count);
        }
        if matches!(new_mode, Mode::Replace | Mode::VirtualReplace) {
            // Replace/VirtualReplace needs an undo group and insert session.
            // ReplaceMode entry type — distinct from SubstituteChar (`s`) because
            // `R` does not delete characters on entry and count controls repeat on exit.
            use crate::primitives::InsertEntryType;
            use crate::state::InsertState;
            let repeat_count = count
                .and_then(std::num::NonZeroU32::new)
                .unwrap_or(std::num::NonZeroU32::MIN);
            self.state.start_insert(InsertState::with_count(
                InsertEntryType::ReplaceMode,
                repeat_count,
            ));
            let mut response = Response::with_effects(
                crate::effects::Effects::new()
                    .begin_undo()
                    .set_mode(new_mode),
            );
            // Process effects through the effect processor so the engine's
            // undo tree opens a group. Without this, EndUndoGroup on exit
            // finds no pending group and the undo node_id is None — breaking
            // undo for the entire replace-mode session.
            //
            // NOTE: state.set_mode() is NOT called before this — the
            // effect_processor captures mode_before from state.mode() and
            // needs to see the transition (Normal→Replace) so emit_cursor_style
            // fires. The SetMode effect updates state.mode() during processing.
            super::super::effect_processor::process_effects(
                &mut self.state,
                &mut self.parser,
                false,
                &mut response,
            );
            response
        } else {
            self.state.set_mode(new_mode);
            Response::with_effects(action_effects::switch_mode(new_mode))
        }
    }

    fn maybe_return_to_mode(&mut self, response: &mut Response, text: Option<&str>) {
        // After executing in Normal mode, check if return_to is set (Ctrl-O flow).
        let return_to = self.state.take_return_to();
        if let Some(target_mode) = return_to.target_mode() {
            self.state.set_mode(target_mode);
            // Restore last_command from before Ctrl-O so that `.` after
            // returning to insert replays the original command, not the
            // one-shot command.
            self.parser.restore_previous_command();

            // Adjust cursor for insert-mode semantics: if the normal-mode command
            // left the cursor on the last character of a line, advance to the
            // "append" position (one past last char). In normal mode the cursor
            // can't be past the last char, but in insert mode it can — and Vim
            // always places the insert cursor past end-of-line when returning
            // from <C-o>$ or similar end-of-line motions.
            if matches!(
                target_mode,
                Mode::Insert | Mode::Replace | Mode::VirtualReplace
            ) {
                if let Some(text) = text {
                    if let Some(offset) = Self::last_cursor_in_effects(&response.effects) {
                        Self::adjust_cursor_for_insert_eol(response, text, offset.get());
                    }
                }
            }

            response.extend_effects(action_effects::switch_mode(target_mode));
        }
    }

    /// After a prefix sub-command executes, re-enter the prefix parser state
    /// so the next key is also treated as a sub-command (sticky mode).
    ///
    /// Called after `maybe_return_to_mode()` — by this point all effects from
    /// the command have been processed and any mode change (e.g., Ctrl-O return
    /// to Insert) is reflected in `self.state.mode()`.
    ///
    /// If the mode is no longer Normal (e.g., the command entered Insert mode),
    /// the sticky session is cleared. Otherwise, the parser is set to the
    /// appropriate prefix-awaiting state for the next key.
    fn maybe_reenter_sticky(&mut self) {
        let Some(session) = self.sticky_session else {
            return;
        };

        if self.state.mode() != Mode::Normal {
            self.sticky_session = None;
            return;
        }

        match session.target() {
            StickyTarget::Window => {
                self.parser.set_state(
                    crate::grammar::input_state::InputState::AwaitingWindowCommand {
                        count: None,
                        register: None,
                    },
                );
            }
            StickyTarget::ZPrefix => {
                self.parser
                    .set_state(crate::grammar::input_state::InputState::AwaitingPrefix {
                        count: None,
                        register: None,
                        prefix: 'z',
                        operator: None,
                        force_type: None,
                    });
            }
        }
    }

    pub(in crate::execution::engine) fn handle_pipeline_error(
        &mut self,
        err: &PipelineError,
    ) -> Response {
        self.parser.reset();

        // Sticky sub-mode: only clear on escape-class errors.
        // Non-escape invalid keys (typos) should re-enter the prefix state
        // so the key is silently dropped and the user stays in sticky mode.
        let is_escape_class = match err {
            PipelineError::InvalidKey { key, .. } | PipelineError::Cancelled { key, .. } => {
                key.key() == crate::keymap::Key::Escape
                    || *key == crate::keymap::KeyEvent::ctrl('c')
                    || *key == crate::keymap::KeyEvent::ctrl('[')
            }
            _ => true, // unknown error types clear sticky as safety net
        };
        if is_escape_class {
            self.sticky_session = None;
        } else {
            self.maybe_reenter_sticky();
        }

        // Neovim emits E354 when @{register} is cancelled by Ctrl-C
        // (the register name is effectively empty).
        if let PipelineError::Cancelled { key, .. } = err {
            if key.is_ctrl_c() {
                // Also clear secondary cursors on Ctrl-C — same semantics as Escape.
                if self.state.multi_cursor().is_active() {
                    let mc_ctx = crate::execution::multi_cursor_executor::MultiCursorContext {
                        text: "",
                        search_pattern: None,
                        line_count: 0,
                    };
                    let _ = crate::execution::multi_cursor_executor::execute_multi_cursor_command(
                        &mut self.state,
                        &crate::state::MultiCursorCommand::ClearSecondary,
                        &mc_ctx,
                    );
                }
                let effects = crate::effects::Effects::new()
                    .show_error(crate::errors::VimError::InvalidRegisterName("^C".into()));
                return Response::with_effects(effects);
            }
        }

        // Escape-class keys (Escape, Ctrl-C, Ctrl-[) in Normal mode clear the
        // message area — matching Neovim's behavior where Escape dismisses any
        // displayed message. Other invalid keys just beep silently.
        if let PipelineError::InvalidKey {
            mode: Mode::Normal,
            key,
        }
        | PipelineError::Cancelled {
            mode: Mode::Normal,
            key,
        } = err
        {
            if key.key() == crate::keymap::Key::Escape
                || *key == crate::keymap::KeyEvent::ctrl('c')
                || *key == crate::keymap::KeyEvent::ctrl('[')
            {
                // Clear secondary cursors if multi-cursor is active.
                if self.state.multi_cursor().is_active() {
                    let mc_ctx = crate::execution::multi_cursor_executor::MultiCursorContext {
                        text: "",
                        search_pattern: None,
                        line_count: 0,
                    };
                    let _ = crate::execution::multi_cursor_executor::execute_multi_cursor_command(
                        &mut self.state,
                        &crate::state::MultiCursorCommand::ClearSecondary,
                        &mc_ctx,
                    );
                }
                return Response::with_effects(crate::effects::Effects::new().clear_message());
            }
        }

        // No message for other invalid/cancelled keys — Neovim silently
        // ignores unknown keys (just beeps).
        Response::consumed_empty()
    }

    /// Intercept insert-mode commands that require host I/O.
    ///
    /// Returns `Some(Response)` if the command was intercepted (Paste,
    /// RequestCompletion), `None` if it should continue through the
    /// normal insert dispatch pipeline.
    fn try_intercept_insert_host_command<D: Document>(
        &mut self,
        command: &Command,
        ctx: &InputContext<'_, D, Validated>,
    ) -> Option<Response> {
        match command {
            Command::Insert(InsertKind::RequestCompletion { kind }) => {
                let mut response = Response::ignored();
                response.host_requests.push(HostRequest::RequestCompletion {
                    meta: self.host.sequencer.next_meta(),
                    kind: *kind,
                });
                response.kind = ResponseKind::Pending;
                Some(response)
            }
            Command::Insert(InsertKind::Paste) => {
                let mut response = Response::ignored();
                response.host_requests.push(HostRequest::ReadClipboard {
                    meta: self.host.sequencer.next_meta(),
                    cursor_offset: ctx.cursor_offset().get(),
                });
                response.kind = ResponseKind::Pending;
                Some(response)
            }
            Command::Insert(InsertKind::ToggleReplace) => {
                // Toggle between Insert and Replace mode.
                let current_mode = self.state.mode();
                let new_mode = match current_mode {
                    Mode::Insert => Mode::Replace,
                    Mode::Replace | Mode::VirtualReplace => Mode::Insert,
                    _ => return None, // shouldn't happen
                };
                let effects = crate::effects::Effects::new().set_mode(new_mode);
                let mut response = Response::with_effects(effects);
                super::super::effect_processor::process_effects(
                    &mut self.state,
                    &mut self.parser,
                    false,
                    &mut response,
                );
                Some(response)
            }
            Command::Insert(InsertKind::ToggleLangmap) => {
                // Toggle insert-mode langmap (Ctrl-^). Currently consumed as
                // a no-op since vim-core has no :lmap system. Matches Neovim
                // behavior when no :lmap mappings exist.
                Some(Response::consumed_empty())
            }
            _ => None,
        }
    }

    /// Execute a parsed insert-mode command.
    ///
    /// Insert-specific commands bypass the executor entirely. The engine:
    /// 1. Pre-computes derived data ONCE (`precompute_insert`)
    /// 2. Applies state mutations using precomputed data (`apply_insert_mutations`)
    /// 3. Calls `dispatch_insert` directly — no executor middleman
    /// 4. Calls `process_effects` for consistent state synchronization
    ///
    /// Non-insert commands (arrows, Home, End) that arrive via the insert-mode
    /// pipeline are routed through the normal executor pipeline.
    fn execute_insert_command<D: Document>(
        &mut self,
        command: Command,
        insert_mode: InsertMode,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        if let Some(response) = self.try_intercept_insert_host_command(&command, &ctx) {
            return response;
        }

        // Capture static tag for provenance before any potential move (zero-alloc).
        let command_name = command.tag();
        let keystroke_seq = self.keystroke_seq;

        // Guard: Non-insert commands (arrows, Home/End, Ctrl-arrows) arrive here
        // because they're typed during insert mode. Route them through executor.
        if !command.is_insert_specific() {
            // Arrow keys in insert mode break the undo sequence (Neovim behavior).
            // Process the break through effect_processor so the engine-side undo
            // tree sees the boundary (not just the host).  This mirrors the
            // Ctrl-G u path in dispatch_insert (BreakUndoSequence).
            // Exception: if dont_sync_undo is set (Ctrl-G U), skip the undo break
            // for this one movement and clear the flag.
            let skip_undo_break = self
                .state
                .insert_state()
                .is_some_and(|is| is.dont_sync_undo);
            if skip_undo_break {
                if let Some(is) = self.state.insert_state_mut() {
                    is.dont_sync_undo = false;
                }
            }
            let break_response = if skip_undo_break {
                Response::consumed_empty()
            } else {
                // Arrow keys in INSERT break the repeat block (Neovim's start_arrow).
                // Reset accumulated_text so dot-repeat only replays text typed AFTER
                // the arrow, and set arrow_used so insert exit knows marks were split.
                if let Some(is) = self.state.insert_state_mut() {
                    is.reset_accumulated_text();
                    is.set_arrow_used();
                }
                let undo_break =
                    crate::effects::Effects::<crate::effects::undo_state::UndoOpen>::resume_open()
                        .end_undo()
                        .begin_undo()
                        .into_raw_closed();
                let mut r = Response::with_effects(undo_break);
                super::super::effect_processor::process_effects(
                    &mut self.state,
                    &mut self.parser,
                    false,
                    &mut r,
                );
                r
            };

            let was_repeat = self.is_repeating;
            match self.execute_effect_plan(command, was_repeat, ctx) {
                Ok(mut response) => {
                    // Prepend the processed undo break effects before motion effects.
                    let mut prepended: SmallVec<[_; 4]> = break_response.effects;
                    prepended.extend(response.effects.drain(..));
                    response.effects = prepended;
                    response.provenance = Some(EffectProvenance::new(command_name, keystroke_seq));
                    return response;
                }
                Err(ref err) => return self.handle_pipeline_error(err),
            }
        }

        // Resolve InsertKind::Digraph → InsertKind::Char via the digraph registry.
        // The grammar defers resolution so user-defined digraphs are consulted.
        let command = if let Command::Insert(InsertKind::Digraph { c1, c2 }) = &command {
            let resolved = self.digraph_registry.lookup(*c1, *c2).unwrap_or(*c2);
            Command::Insert(InsertKind::Char { char: resolved })
        } else {
            command
        };

        // Resolve ^^D / 0^D: when Ctrl-D is pressed and the last character
        // typed in the insert session was '^' or '0', consume that character
        // from accumulated_text and transform the command.
        let command = if matches!(command, Command::Insert(InsertKind::Outdent)) {
            if let Some(is) = self.state.insert_state_mut() {
                match is.accumulated_text().chars().last() {
                    Some('^') => {
                        is.pop_char();
                        Command::Insert(InsertKind::OutdentTemporary)
                    }
                    Some('0') => {
                        is.pop_char();
                        Command::Insert(InsertKind::OutdentClear)
                    }
                    _ => command,
                }
            } else {
                command
            }
        } else {
            command
        };

        // Merge engine-level providers (set via setIndentAction) with per-call
        // providers so insert-mode Enter gets the host's indent hint.
        let ctx = if self.engine_providers.has_any() {
            let merged = self.engine_providers.merge_with(ctx.providers());
            ctx.with_providers(merged)
        } else {
            ctx
        };

        let (text, mut cursor) = (ctx.doc().text(), ctx.cursor_offset().get());

        // `a` and `A` can leave a cursor of several past the end of the last
        // line. The text goes at the end, and formatting needs the cursor
        // there to find the line it typed on.
        if crate::commands::insert::wrap::FormatPolicy::from_options(&self.resolved_options)
            .is_active()
            && self.state.multi_cursor().is_active()
        {
            cursor = cursor.min(text.len());
            let selections = self.state.multi_cursor().selections();
            if selections.iter().any(|s| s.head().get() > text.len()) {
                let ranges = selections
                    .iter()
                    .map(|s| {
                        if s.head().get() > text.len() {
                            crate::primitives::SelectionRange::insert_cursor(Offset::new(
                                text.len(),
                            ))
                        } else {
                            *s
                        }
                    })
                    .collect();
                let primary = selections.primary_index();
                self.state
                    .multi_cursor_mut()
                    .set_selections(crate::primitives::Selections::from_vec(ranges, primary));
            }
        }

        // Record where the insert started, for formatting (Vim's Insstart):
        // at the first insert command, and again when the cursor is not where
        // the previous insert command left it, as after an arrow key (Vim's
        // stop_arrow()).
        let tabstop = self.resolved_options.tabstop();
        if let Some(is) = self.state.insert_state_mut() {
            if is.insert_start().is_none() || is.insert_start_cursor() != Some(Offset::new(cursor))
            {
                let blank_vcol = is.insert_start().and_then(|s| s.blank_vcol);
                is.set_insert_start(crate::commands::insert::wrap::insert_start_at(
                    text, cursor, tabstop, blank_vcol,
                ));
            }
        }

        // 1. Pre-compute operation data ONCE (read-only state access)
        let precomputed = super::insert::precompute_insert(
            &self.state,
            &command,
            insert_mode,
            text,
            cursor,
            self.resolved_options.tabstop(),
            self.resolved_options.autoindent(),
            ctx.providers().indent,
            &self.resolved_options,
        );
        // 2. Read entry_offset BEFORE mutations (read-only)
        let entry_offset = self
            .state
            .insert_state()
            .filter(|is| !is.accumulated_text().is_empty())
            .and_then(InsertState::entry_offset);

        // 3. Apply state mutations using precomputed data
        super::insert::apply_insert_mutations(&mut self.state, &command, &precomputed);

        // Store expression text for dot-repeat re-evaluation.
        // When `.` replays an insert session that included `<C-r>=expr<CR>`,
        // the engine can re-evaluate the expression instead of using the
        // stale cached result.
        if let Command::Insert(InsertKind::ExpressionResult { ref expression }) = command {
            self.state.store_last_expression_text(expression.as_str());
        }

        // Ctrl-O: record return_to so engine restores mode after normal command.
        // Also set arrow_used on the insert state — Neovim's equivalent flag
        // causes ins_esc to skip stop_insert (preserving marks from the C-o
        // command) when no text is typed after returning.
        if matches!(command, Command::Insert(InsertKind::OneShot)) {
            self.state
                .set_return_to(return_to_for_insert_mode(insert_mode));
            // Save current last_command as previous_command for C-o . replay.
            self.parser.save_previous_command();
            if let Some(is) = self.state.insert_state_mut() {
                is.reset_accumulated_text();
                is.set_arrow_used();
            }
        }
        // Ctrl-G U: set dont_sync_undo flag so next cursor movement skips undo break.
        if matches!(command, Command::Insert(InsertKind::DontSyncUndo)) {
            if let Some(is) = self.state.insert_state_mut() {
                is.dont_sync_undo = true;
            }
        }
        // ^^D (OutdentTemporary): save current line's indent before it is removed.
        // The next newline will restore this indent on the new line.
        if matches!(command, Command::Insert(InsertKind::OutdentTemporary)) {
            let line_start = crate::commands::helpers::line_start_for_offset(text, cursor);
            let leading_ws_len = text[line_start..]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .map(char::len_utf8)
                .sum::<usize>();
            if let Some(is) = self.state.insert_state_mut() {
                let indent = compact_str::CompactString::from(
                    &text[line_start..line_start + leading_ws_len],
                );
                is.set_saved_indent(indent);
            }
        }

        // 4. Build InsertContext and dispatch directly (no executor)
        let mut insert_ctx = InsertContext::new(text, crate::primitives::Offset::new(cursor))
            .with_precomputed(precomputed)
            .with_shift_width(self.resolved_options.shiftwidth())
            .with_tabstop(self.resolved_options.tabstop())
            .with_auto_pairs(self.resolved_options.auto_pairs());
        if let Some(offset) = entry_offset {
            insert_ctx = insert_ctx.with_entry_offset(offset);
        }
        let mut result = crate::dispatch::dispatch_insert(&command, &insert_ctx);

        // 4b. ShowMatch: emit bracket-flash hint when a closing bracket is
        // inserted with 'showmatch' enabled. Scan backward in the pre-edit
        // text (the bracket hasn't been spliced in yet, but the text before
        // `cursor` is identical in both T0 and T1).
        if self.resolved_options.showmatch() {
            if let Command::Insert(InsertKind::Char { char } | InsertKind::LiteralChar { char }) =
                &command
            {
                if let Some(match_pos) =
                    crate::commands::motions::bracket::find_matching_open_bracket(
                        text, cursor, *char,
                    )
                {
                    result.effects.push(crate::effects::Effect::ShowMatch {
                        position: match_pos,
                    });
                }
            }
        }

        // 4c. Formatting while typing (Vim's internal_format()): a non-blank
        // character typed past 'textwidth' breaks the line. The breaks are
        // spliced in before the final SetCursor. An abbreviation that expands
        // is formatted together with its expansion in step 5c instead.
        let abbreviation = self.pending_abbreviation(&command, insert_mode, text, cursor);
        let format_policy =
            crate::commands::insert::wrap::FormatPolicy::from_options(&self.resolved_options);
        // Every change the per-cursor path makes for formatting is gated on
        // this, so that without formatting several cursors type as before.
        let format_active = format_policy.is_active();
        let formats_typed = format_active
            && insert_mode != InsertMode::VirtualReplace
            && !self.state.insert_state().is_some_and(InsertState::pasting)
            && is_formatted_char(&command);
        let primary_effects_for_abbreviation = abbreviation
            .as_ref()
            .filter(|_| formats_typed)
            .map(|_| result.effects.clone());
        if formats_typed && abbreviation.is_none() {
            // The host cursor need not be the primary cursor yet: the
            // per-cursor path below makes the cursor sitting at the host
            // cursor the primary one, and only when no cursor sits there does
            // the primary stand for the host cursor.
            let mc = self.state.multi_cursor();
            let selections = mc.selections();
            let host_is_a_cursor = selections.iter().any(|sr| sr.head().get() == cursor);
            let shares_line = mc.is_active()
                && cursor_shares_line(
                    text,
                    cursor,
                    selections
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| host_is_a_cursor || i != selections.primary_index())
                        .map(|(_, sr)| sr.head().get()),
                );
            if !shares_line {
                let mut start = self
                    .state
                    .insert_state()
                    .and_then(InsertState::insert_start);
                crate::commands::insert::wrap::format_typed_char(
                    &mut result.effects,
                    text,
                    cursor,
                    insert_mode == InsertMode::Replace,
                    &format_policy,
                    start.as_mut(),
                );
                if let (Some(start), Some(is)) = (start, self.state.insert_state_mut()) {
                    is.set_insert_start(start);
                }
            }
        }

        // 4d. Multi-cursor: replicate insert effects to all cursors.
        //
        // Hybrid dispatch: content-dependent insert commands use per-cursor
        // re-execution (each cursor runs precompute_insert + dispatch_insert
        // independently against the T0 document text). Position-independent
        // insert commands use the existing algebraic rebase path.
        // A character that may break the line depends on its own line, so
        // every cursor formats its own text.
        let is_insert_cd = if let Command::Insert(ref kind) = command {
            kind.is_content_dependent(
                self.resolved_options.expandtab(),
                self.resolved_options.auto_pairs().is_some(),
            ) || formats_typed
        } else {
            false
        };

        // Track per-cursor deltas for CD path selection update.
        let mut insert_cd_deltas: Option<Vec<(usize, crate::primitives::Offset, i64)>> = None;

        let effects = if self.state.multi_cursor().is_active() && is_insert_cd {
            // ── Per-cursor re-execution for CD insert commands ─────────
            //
            // precompute_insert and dispatch_insert already ran for the primary
            // cursor (steps 1-4 above). apply_insert_mutations also ran for
            // the primary (step 3) -- it must only run ONCE because it updates
            // per-session state (accumulated_text, replace stack).
            //
            // For secondary cursors, we re-run precompute_insert + dispatch_insert
            // with each cursor's offset against the T0 text. Effects are collected
            // in descending offset order so higher-offset edits don't
            // invalidate lower-offset positions when the host applies them.

            // Sync the primary selection with the actual cursor. The host
            // cursor can be another cursor than the primary one (it follows
            // the last SetCursor applied), so when a cursor sits there it
            // becomes the primary, as in the algebraic path below.
            // Overwriting the primary head instead would put two cursors on
            // the same offset. Only formatting needs this, so without it
            // the primary head is overwritten as before.
            let cursor_off = crate::primitives::Offset::new(cursor);
            let selections = self.state.multi_cursor_mut().selections_mut();
            if let Some(idx) = selections
                .ranges()
                .iter()
                .position(|s| s.head() == cursor_off)
                .filter(|_| format_active)
            {
                selections.set_primary_index(idx);
            } else {
                *selections.primary_mut() =
                    crate::primitives::SelectionRange::insert_cursor(cursor_off);
            }

            let selections = self.state.multi_cursor().selections().clone();
            let primary_offset = selections.primary().head().get();

            // Sort cursors descending by head offset.
            let mut cursors_desc: Vec<(usize, crate::primitives::SelectionRange)> = selections
                .iter()
                .enumerate()
                .map(|(i, sr)| (i, *sr))
                .collect();
            cursors_desc.sort_by_key(|&(_, sr)| std::cmp::Reverse(sr.head().get()));

            let mut all_effects: Vec<crate::effects::Effect> = Vec::new();
            let mut cursor_deltas: Vec<(usize, crate::primitives::Offset, i64)> = Vec::new();
            let mut secondary_starts: Vec<(usize, usize, crate::state::InsertStart)> = Vec::new();

            // For CopyCharAbove/CopyCharBelow, maintain a working text
            // buffer that includes higher-offset cursors' edits. These commands
            // read from adjacent lines which may be in another cursor's edit
            // region. For all other insert CD commands, T0 is correct because
            // they only read from the current cursor's line.
            let needs_working_buffer = matches!(
                command,
                Command::Insert(InsertKind::CopyCharBelow | InsertKind::CopyCharAbove)
            );
            let mut working_text: Option<String> = if needs_working_buffer {
                Some(text.to_owned())
            } else {
                None
            };

            for &(sel_idx, sel_range) in &cursors_desc {
                let cur = sel_range.head().get();

                // For the primary cursor, use the already-computed effects.
                // For secondary cursors, re-execute precompute + dispatch.
                let effects_i = if cur == primary_offset {
                    result.effects.clone()
                } else {
                    // Use working text for CopyChar, T0 for everything else.
                    let exec_text: &str = working_text.as_deref().unwrap_or(text);

                    let precomputed_i = super::insert::precompute_insert(
                        &self.state,
                        &command,
                        insert_mode,
                        exec_text,
                        cur,
                        self.resolved_options.tabstop(),
                        self.resolved_options.autoindent(),
                        ctx.providers().indent,
                        &self.resolved_options,
                    );

                    let entry_offset_i = self
                        .state
                        .insert_state()
                        .filter(|is| !is.accumulated_text().is_empty())
                        .and_then(InsertState::entry_offset);

                    let mut insert_ctx_i =
                        InsertContext::new(exec_text, crate::primitives::Offset::new(cur))
                            .with_precomputed(precomputed_i)
                            .with_shift_width(self.resolved_options.shiftwidth())
                            .with_tabstop(self.resolved_options.tabstop())
                            .with_auto_pairs(self.resolved_options.auto_pairs());
                    if let Some(offset) = entry_offset_i {
                        insert_ctx_i = insert_ctx_i.with_entry_offset(offset);
                    }

                    let mut r = crate::dispatch::dispatch_insert(&command, &insert_ctx_i);
                    if format_active {
                        // Every cursor has its own insert start for 'l', 'v' and
                        // 'b', recorded again when an insert command did not
                        // leave the cursor here, as for the primary cursor.
                        let line_i = crate::commands::helpers::line_of(exec_text, cur);
                        let mut start_i = self
                            .state
                            .insert_state()
                            .and_then(|is| {
                                is.cursor_start(crate::primitives::Offset::new(cur), line_i)
                            })
                            .unwrap_or_else(|| {
                                crate::commands::insert::wrap::insert_start_at(
                                    exec_text,
                                    cur,
                                    self.resolved_options.tabstop(),
                                    None,
                                )
                            });
                        // Two cursors on one line would break it under each
                        // other, so such a line is left alone.
                        let shares_line = cursor_shares_line(
                            exec_text,
                            cur,
                            selections
                                .iter()
                                .enumerate()
                                .filter(|&(i, _)| i != sel_idx)
                                .map(|(_, sr)| sr.head().get()),
                        );
                        if formats_typed && !shares_line {
                            crate::commands::insert::wrap::format_typed_char(
                                &mut r.effects,
                                exec_text,
                                cur,
                                insert_mode == InsertMode::Replace,
                                &format_policy,
                                Some(&mut start_i),
                            );
                        }
                        let own_cursor = Self::last_cursor_in_effects(r.effects.as_slice())
                            .map_or(cur, crate::primitives::Offset::get);
                        if matches!(command, Command::Insert(InsertKind::Backspace)) {
                            move_insert_start_on_backspace(
                                &mut start_i,
                                exec_text,
                                cur,
                                own_cursor,
                            );
                        }
                        let lines_below =
                            cursor_line_after(exec_text, r.effects.as_slice(), cur, own_cursor)
                                .unwrap_or(line_i)
                                .saturating_sub(start_i.line);
                        secondary_starts.push((own_cursor, lines_below, start_i));
                    }
                    r.effects
                };

                // Track SetCursor and net delta for selection update.
                let mut set_cursor_offset: Option<crate::primitives::Offset> = None;
                let mut net_delta: i64 = 0;

                for effect in effects_i.as_slice() {
                    match effect {
                        crate::effects::Effect::SetCursor { offset } => {
                            set_cursor_offset = Some(*offset);
                        }
                        crate::effects::Effect::Insert { text: t, .. } => {
                            net_delta += byte_delta::to_i64(t.len());
                        }
                        crate::effects::Effect::Delete { range } => {
                            net_delta -=
                                byte_delta::delta_i64(range.end().get(), range.start().get());
                        }
                        crate::effects::Effect::Replace { range, text: t } => {
                            net_delta += byte_delta::to_i64(t.len())
                                - byte_delta::delta_i64(range.end().get(), range.start().get());
                        }
                        _ => {}
                    }

                    // Collect positional effects from all cursors.
                    if super::super::multi_cursor::is_positional_effect(effect) {
                        all_effects.push(effect.clone());
                    }
                }

                let final_offset = set_cursor_offset.unwrap_or(sel_range.head());
                cursor_deltas.push((sel_idx, final_offset, net_delta));

                // Apply this cursor's effects to the working text buffer
                // so the next cursor (lower offset) sees the updated text.
                if let Some(ref mut buf) = working_text {
                    for effect in effects_i.as_slice() {
                        match effect {
                            crate::effects::Effect::Insert { offset, text: t } => {
                                let pos = offset.get().min(buf.len());
                                buf.insert_str(pos, t);
                            }
                            crate::effects::Effect::Delete { range } => {
                                let start = range.start().get().min(buf.len());
                                let end = range.end().get().min(buf.len());
                                if start < end {
                                    buf.drain(start..end);
                                }
                            }
                            crate::effects::Effect::Replace { range, text: t } => {
                                let start = range.start().get().min(buf.len());
                                let end = range.end().get().min(buf.len());
                                if start <= end && end <= buf.len() {
                                    buf.replace_range(start..end, t);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // The insert starts of the secondary cursors go with where the
            // edits of every cursor leave them.
            let new_starts = secondary_starts
                .into_iter()
                .map(|(own, lines_below, start)| {
                    let shift: i64 = cursor_deltas
                        .iter()
                        .filter(|(_, raw, _)| raw.get() < own)
                        .map(|&(_, _, delta)| delta)
                        .sum();
                    crate::state::CursorInsertStart {
                        head: crate::primitives::Offset::new(byte_delta::shift(own, shift)),
                        lines_below,
                        start,
                    }
                })
                .collect();
            if let Some(is) = self.state.insert_state_mut() {
                is.set_cursor_starts(new_starts);
            }
            insert_cd_deltas = Some(cursor_deltas);
            all_effects.into_iter().collect()
        } else if self.state.multi_cursor().is_active() {
            // ── Algebraic rebase for PI insert commands (existing path) ─
            // Sync the primary selection head with the actual cursor so that
            // delta computation in replicate_effects_precise is correct.
            // After replication, update all selection heads from the replicated
            // SetCursor effects so subsequent keystrokes use correct deltas.
            let cursor_off = crate::primitives::Offset::new(cursor);
            let selections = self.state.multi_cursor_mut().selections_mut();
            if let Some(idx) = selections
                .ranges()
                .iter()
                .position(|s| s.head() == cursor_off)
            {
                selections.set_primary_index(idx);
            } else {
                *selections.primary_mut() =
                    crate::primitives::SelectionRange::insert_cursor(cursor_off);
            }
            let replicated = super::super::multi_cursor::replicate_effects_precise(
                &result.effects,
                self.state.multi_cursor().selections(),
                &UndoIntent::None, // insert mode already has an open group
            );
            // Update selection heads to reflect post-all-edits positions.
            //
            // The replicated effects are emitted in descending-offset order.
            // When the host applies them sequentially, lower-offset inserts
            // shift higher-offset cursor positions. We compute the net byte
            // delta from the primary effects (insert bytes - delete bytes)
            // and use it to adjust each cursor's SetCursor value.
            let group_size = result.effects.len();
            let num_cursors = self.state.multi_cursor().selections().len();
            // Net bytes added per cursor group (positive = growth).
            let net_delta_per_cursor: i64 =
                result
                    .effects
                    .as_slice()
                    .iter()
                    .fold(0i64, |acc, e| match e {
                        crate::effects::Effect::Insert { text, .. } => {
                            acc + byte_delta::to_i64(text.len())
                        }
                        crate::effects::Effect::Delete { range } => {
                            acc - byte_delta::delta_i64(range.end().get(), range.start().get())
                        }
                        crate::effects::Effect::Replace { range, text } => {
                            acc + byte_delta::to_i64(text.len())
                                - byte_delta::delta_i64(range.end().get(), range.start().get())
                        }
                        _ => acc,
                    });
            // Extract last SetCursor from each group (descending order).
            let replicated_slice = replicated.as_slice();
            let mut raw_positions: Vec<crate::primitives::Offset> = Vec::with_capacity(num_cursors);
            for i in 0..num_cursors {
                let group_start = i * group_size;
                let group_end = (group_start + group_size).min(replicated_slice.len());
                if let Some(pos) = replicated_slice
                    .get(group_start..group_end)
                    .and_then(|group| {
                        group.iter().rev().find_map(|e| {
                            if let crate::effects::Effect::SetCursor { offset } = e {
                                Some(*offset)
                            } else {
                                None
                            }
                        })
                    })
                {
                    raw_positions.push(pos);
                }
            }
            // Adjust for lower-offset edits shifting higher positions.
            // Cursor at desc index i is shifted by (num_cursors - 1 - i)
            // lower-offset cursor groups, each adding net_delta_per_cursor.
            if raw_positions.len() == num_cursors {
                let mc_sels = self.state.multi_cursor().selections();
                let primary_idx = mc_sels.primary_index();
                let mut sorted_indices: Vec<usize> = (0..num_cursors).collect();
                #[expect(
                    clippy::indexing_slicing,
                    reason = "sorted_indices contains values 0..num_cursors, all valid indices into ranges()"
                )]
                sorted_indices
                    .sort_by_key(|&idx| std::cmp::Reverse(mc_sels.ranges()[idx].head().get()));
                let mut new_ranges: Vec<crate::primitives::SelectionRange> =
                    mc_sels.ranges().to_vec();
                #[expect(
                    clippy::indexing_slicing,
                    reason = "desc_rank < sorted_indices.len() == num_cursors == raw_positions.len()"
                )]
                for (desc_rank, &sel_idx) in sorted_indices.iter().enumerate() {
                    // Number of cursor groups applied AFTER this one (lower offsets).
                    let groups_after = byte_delta::to_i64(num_cursors - 1 - desc_rank);
                    let shift = groups_after.saturating_mul(net_delta_per_cursor);
                    let adjusted = byte_delta::shift(raw_positions[desc_rank].get(), shift);
                    if let Some(slot) = new_ranges.get_mut(sel_idx) {
                        *slot = crate::primitives::SelectionRange::insert_cursor(
                            crate::primitives::Offset::new(adjusted),
                        );
                    }
                }
                self.state.multi_cursor_mut().set_selections(
                    crate::primitives::Selections::from_vec(new_ranges, primary_idx),
                );
            }
            replicated
        } else {
            result.effects
        };

        // 5. Process effects (state sync, changelist tracking)
        let mut response = Response::with_effects(effects);
        let proc_result = super::super::effect_processor::process_effects_with_text(
            &mut self.state,
            &mut self.parser,
            false,
            &mut response,
            Some(text),
            None,
            self.resolved_options.tabstop(),
            self.resolved_options.undo_auto_group_ms(),
            self.resolved_options.cursor_shape_overrides(),
        );

        // Track edit start for operations that modify text before entry_offset.
        // C-d/C-t modify text at line start. BS/C-u at beginning of line
        // delete the preceding newline (joining lines). All of these need
        // min_change_start so insert_handler uses the correct mark `[`.
        if matches!(
            command,
            Command::Insert(
                InsertKind::Indent
                    | InsertKind::Outdent
                    | InsertKind::OutdentTemporary
                    | InsertKind::OutdentClear
            )
        ) {
            if let Some(is) = self.state.insert_state_mut() {
                let ls = crate::commands::helpers::line_start_for_offset(text, cursor);
                is.track_change_start(crate::primitives::Offset::new(ls));
            }
        }
        // BS at beginning of line joins with the previous line, deleting
        // the newline that precedes entry_offset. Track the resulting cursor
        // position so insert_handler uses the correct mark `[`.
        // Only applies when cursor was at column 0 (beginning of line) and
        // no text has been typed yet.
        if matches!(
            command,
            Command::Insert(InsertKind::Backspace | InsertKind::DeleteToStart)
        ) {
            let was_at_bol =
                cursor == 0 || text.as_bytes().get(cursor.wrapping_sub(1)) == Some(&b'\n');
            if was_at_bol {
                if let Some(is) = self.state.insert_state_mut() {
                    if is.accumulated_text().is_empty() {
                        if let Some(new_cur) = proc_result.last_cursor_offset {
                            is.track_change_start(new_cur);
                        }
                    }
                }
            }
        }

        // Correct auto-emitted SetStickyColumn for insert text mutations.
        if let Some(cur) = proc_result.last_cursor_offset {
            correct_sticky_column_for_mutations(
                text,
                cur,
                &mut response,
                &mut self.state,
                self.resolved_options.tabstop(),
            );
        }

        // CD insert selection update: use per-cursor deltas to update
        // selections after process_effects and sticky column correction.
        if let Some(ref deltas) = insert_cd_deltas {
            super::per_cursor::update_selections_from_deltas(
                self.state.multi_cursor_mut().selections_mut(),
                deltas,
                !format_active,
            );
        }

        // 5b. Ctrl-@: LastInsertedAndExit — insert text, then immediately exit.
        // After the text insert effects are processed, perform the same exit
        // orchestration as Escape would trigger.
        if matches!(command, Command::Insert(InsertKind::LastInsertedAndExit)) {
            // Determine the cursor after the insert (the last SetCursor in effects).
            let new_cursor = Self::last_cursor_in_effects(&response.effects)
                .map_or(cursor, crate::primitives::Offset::get);
            let (mut exit_response, _exit_info2) = super::super::insert_handler::handle_insert_exit(
                &mut self.state,
                new_cursor,
                text,
                None,
            );
            super::super::effect_processor::process_effects_with_text(
                &mut self.state,
                &mut self.parser,
                false,
                &mut exit_response,
                Some(text),
                self.resolved_options.undolevels(),
                self.resolved_options.tabstop(),
                self.resolved_options.undo_auto_group_ms(),
                self.resolved_options.cursor_shape_overrides(),
            );
            response.effects.extend(exit_response.effects);
        }

        // 5c. Abbreviation expansion for insert-mode character inserts.
        //
        // After a character is inserted, check if the text typed so far
        // ends with a registered abbreviation trigger. If so, replace the
        // trigger word with the expansion in both the document (via effects)
        // and accumulated_text (state tracking). Vim types the expansion and
        // then the trigger character, so both are formatted as typed text.
        if let (
            Some((delete_count, replacement)),
            Command::Insert(InsertKind::Char { char: trigger_char }),
        ) = (abbreviation, &command)
        {
            let trigger_char = *trigger_char;
            let trigger_byte_len = trigger_char.len_utf8();
            let word_start = cursor - delete_count;
            let range = crate::primitives::Range::new(
                crate::primitives::Offset::new(word_start),
                crate::primitives::Offset::new(cursor),
            );
            // cursor + (replacement.len() - delete_count) + trigger_byte_len
            let new_cursor = cursor - delete_count + replacement.len() + trigger_byte_len;
            let mut expansion = Effects::new();
            expansion.push(Effect::Replace {
                range,
                text: replacement.clone(),
            });
            expansion.push(Effect::SetCursor {
                offset: crate::primitives::Offset::new(new_cursor),
            });
            if let Some(primary_effects) = primary_effects_for_abbreviation {
                Self::format_abbreviation(
                    &mut self.state,
                    &mut expansion,
                    &primary_effects,
                    text,
                    cursor,
                    word_start..new_cursor,
                    &format_policy,
                );
            }
            response.effects.extend(expansion);

            // Fix accumulated_text: replace the trigger word
            // with the expansion. Currently it's
            // "...trigger_wordX" where X is the trigger char.
            // We want "...replacementX".
            if let Some(is_mut) = self.state.insert_state_mut() {
                let acc_len = is_mut.accumulated_text().len();
                // Remove trigger_char + trigger_word from end.
                is_mut.truncate_tail_bytes(delete_count + trigger_byte_len);
                // Re-append expansion + trigger char.
                is_mut.push_str(&replacement);
                is_mut.push_char(trigger_char);
                // If mark_dot_override was set, recalculate
                // for changed accumulated_text length.
                let new_acc_len = is_mut.accumulated_text().len();
                if new_acc_len != acc_len {
                    is_mut.clear_mark_dot_override();
                }
            }
        }

        // Remember where this command left the cursor, to tell at the next
        // one whether the cursor moved in between. The host cursor follows
        // the last SetCursor, also with several cursors.
        let final_cursor =
            Self::last_cursor_in_effects(&response.effects).unwrap_or_else(|| Offset::new(cursor));
        if let Some(is) = self.state.insert_state_mut() {
            // ins_bs(): a backspace at the start of the line the insert
            // started on moves that start to the end of the line above. The
            // line length that 'l' looks at stays.
            if matches!(command, Command::Insert(InsertKind::Backspace)) {
                if let Some(start) = is.insert_start_mut() {
                    move_insert_start_on_backspace(start, text, cursor, final_cursor.get());
                }
            }
            is.set_insert_start_cursor(final_cursor);
        }

        // 6. Tag provenance for insert commands.
        response.provenance = Some(EffectProvenance::new(command_name, keystroke_seq));
        response
    }

    /// The positional exit effects of every cursor, in descending order of
    /// the cursor heads, when formatting is active and a counted insert
    /// repeats its text at several cursors. Each cursor repeats the text and
    /// formats it with its own insert start, as it would alone. `None` when
    /// the repeat is replicated from the primary cursor. The entry for the
    /// primary cursor, at the host cursor `cursor`, is empty: its effects
    /// come from the regular exit.
    fn per_cursor_exit_repeats(
        &self,
        text: &str,
        cursor: usize,
    ) -> Option<Vec<(usize, Vec<crate::effects::Effect>)>> {
        let policy =
            crate::commands::insert::wrap::FormatPolicy::from_options(&self.resolved_options);
        let mc = self.state.multi_cursor();
        let is = self.state.insert_state()?;
        if !mc.is_active()
            || !policy.is_active()
            || is.count().get() <= 1
            || is.block_insert().is_some()
            || is.accumulated_text().is_empty()
        {
            return None;
        }
        // The primary cursor is the host cursor: the cursor there, or else
        // the primary selection moved there, as the exit below does.
        let primary = cursor;
        let mut heads: Vec<usize> = mc.selections().iter().map(|s| s.head().get()).collect();
        if !heads.contains(&cursor) {
            if let Some(head) = heads.get_mut(mc.selections().primary_index()) {
                *head = cursor;
            }
        }
        heads.sort_unstable_by(|a, b| b.cmp(a));
        heads.dedup();
        let mut out = Vec::with_capacity(heads.len());
        for head in heads {
            if head == primary {
                out.push((head, Vec::new()));
                continue;
            }
            let line = crate::commands::helpers::line_of(text, head.min(text.len()));
            let start = is.cursor_start(Offset::new(head), line);
            let (effects, _) =
                crate::dispatch::build_insert_exit_effects(&crate::dispatch::InsertExitParams {
                    text,
                    accumulated_text: is.accumulated_text(),
                    cursor: Offset::new(head.min(text.len())),
                    count: is.count().get(),
                    entry_type: is.entry_type(),
                    auto_indent_len: is.auto_indent_len(),
                    block_insert: None,
                    mark_dot_override_pos: is.mark_dot_override_pos(),
                    entry_offset: None,
                    format: Some(&policy),
                    insert_start: start,
                });
            out.push((
                head,
                effects
                    .into_inner()
                    .into_iter()
                    .filter(super::super::multi_cursor::is_positional_effect)
                    .collect(),
            ));
        }
        Some(out)
    }

    /// The abbreviation that typing `command` expands, as the number of
    /// bytes of the trigger word before the cursor and its replacement.
    ///
    /// Only plain character inserts in Insert mode expand, and only when the
    /// abbreviation table is non-empty, a single cursor is active and no
    /// bracketed paste is in progress.
    fn pending_abbreviation(
        &self,
        command: &Command,
        insert_mode: InsertMode,
        text: &str,
        cursor: usize,
    ) -> Option<(usize, compact_str::CompactString)> {
        let Command::Insert(InsertKind::Char { char: trigger_char }) = command else {
            return None;
        };
        if self.abbrev_table.is_empty()
            || insert_mode != InsertMode::Insert
            || self.state.multi_cursor().is_active()
            || self.state.insert_state().is_some_and(InsertState::pasting)
        {
            return None;
        }
        let acc = self.state.insert_state()?.accumulated_text();
        // accumulated_text already includes the trigger char at the end.
        // Compute text_before = everything before the trigger char.
        let text_before = acc.get(..acc.len().checked_sub(trigger_char.len_utf8())?)?;
        let wcs = self.resolved_options.word_char_set().clone();
        let is_keyword = |c: char| wcs.contains(c);
        let (delete_count, replacement) = self.abbrev_table.try_expand(
            *trigger_char,
            text_before,
            crate::primitives::AbbrevMode::Insert,
            &is_keyword,
        )?;
        // Safety: verify the document text at the expected position actually
        // contains the trigger word. This guards against cursor and
        // accumulated_text desync after arrow key navigation within insert
        // mode.
        let word_start = cursor.checked_sub(delete_count)?;
        let trigger_word = text_before.get(text_before.len().checked_sub(delete_count)?..)?;
        (text.get(word_start..cursor) == Some(trigger_word)).then_some((delete_count, replacement))
    }

    /// Format an abbreviation expansion and its trigger character as typed
    /// text, splicing the line breaks into `expansion`.
    ///
    /// `primary_effects` are the effects that inserted the trigger character
    /// into `text` at `cursor`; `typed` is the range of the expansion and the
    /// trigger character once both are in place.
    fn format_abbreviation(
        state: &mut crate::state::VimState,
        expansion: &mut Effects,
        primary_effects: &Effects,
        text: &str,
        cursor: usize,
        typed: std::ops::Range<usize>,
        policy: &crate::commands::insert::wrap::FormatPolicy<'_>,
    ) {
        use crate::commands::insert::wrap;
        let mut all: Vec<Effect> = primary_effects.as_slice().to_vec();
        all.extend(expansion.as_slice().iter().cloned());
        let Some((window, base)) = wrap::line_after_effects(text, cursor, &all) else {
            return;
        };
        let (Some(run_start), Some(run_end)) =
            (typed.start.checked_sub(base), typed.end.checked_sub(base))
        else {
            return;
        };
        let run = wrap::TypedRun {
            range: run_start..run_end,
            line: crate::commands::helpers::line_of(text, cursor),
            overwritten: 0,
        };
        let mut start = state.insert_state().and_then(InsertState::insert_start);
        if let Some(plan) = wrap::plan_typed_format(&window, &run, policy, start.as_mut()) {
            wrap::splice_format_plan(expansion, &plan.shifted(base));
        }
        if let (Some(start), Some(is)) = (start, state.insert_state_mut()) {
            is.set_insert_start(start);
        }
    }

    /// Handle InsertExit command (Escape/Ctrl-[/Ctrl-C).
    ///
    /// Handles insert exit orchestration (count repeat, auto-indent strip,
    /// block insert replication) then routes through process_effects for
    /// consistent state synchronization.
    #[inline]
    fn handle_insert_exit(&mut self, cursor: usize, text: &str) -> Response {
        let keystroke_seq = self.keystroke_seq;
        // Capture o/O auto-indent info before insert state is consumed.
        // Neovim preserves curswant at the indent level after o/O<Esc> with
        // no typed text, even though the auto-indent is stripped.
        let open_line_indent_override = self.state.insert_state().and_then(|is| {
            let is_open = matches!(
                is.entry_type(),
                crate::primitives::InsertEntryType::NewLineBelow
                    | crate::primitives::InsertEntryType::NewLineAbove
            );
            if is_open && is.accumulated_text().is_empty() && is.auto_indent_len() > 0 {
                let before = &text[..cursor.min(text.len())];
                let actual_ws = before
                    .bytes()
                    .rev()
                    .take_while(|&b| b == b' ' || b == b'\t')
                    .count();
                if actual_ws >= is.auto_indent_len() {
                    Some(is.auto_indent_len())
                } else {
                    None
                }
            } else {
                None
            }
        });
        // Capture block insert info before insert state is consumed by
        // handle_insert_exit. Used below to precompute undo/redo marks.
        let block_insert_info = self.state.insert_state().and_then(|is| {
            is.block_insert()
                .map(|bc| (bc.lines_below(), is.accumulated_text().len()))
        });
        // With several cursors the counted repeat is replicated from the
        // primary cursor. Formatting breaks each cursor's line on its own,
        // so each cursor then repeats the text as it would alone.
        let per_cursor_repeats = self.per_cursor_exit_repeats(text, cursor);
        let (mut response, exit_info) = super::super::insert_handler::handle_insert_exit(
            &mut self.state,
            cursor,
            text,
            Some(&crate::commands::insert::wrap::FormatPolicy::from_options(
                &self.resolved_options,
            )),
        );

        // ── Precompute undo/redo marks for block visual insert ──────────
        //
        // Block insert groups span multiple lines. The undo tree's
        // end_group() receives T1 text (post-primary-insert) but needs T0
        // line-start offsets for undo marks. Precompute the correct values
        // before processing effects (which includes EndUndoGroup).
        if let Some((lines_below, accumulated_len)) = block_insert_info {
            if lines_below > 0 && accumulated_len > 0 {
                use crate::commands::helpers::{line_of, line_start};
                let primary_line = line_of(text, cursor.min(text.len()));
                let bottom_line = primary_line + lines_below;
                let primary_shift = accumulated_len;
                // Undo `']`: line-start of bottom line in T0.
                // T1 line starts are shifted by primary_shift from T0.
                if let Some(t1_bottom_ls) = line_start(text, bottom_line) {
                    let t0_bottom_ls = t1_bottom_ls.saturating_sub(primary_shift);
                    self.state.undo_tree_mut().set_precomputed_undo_mark_end(
                        crate::primitives::Offset::new(t0_bottom_ls),
                    );
                }
                // Redo `']`: line-start of bottom line in post-all-edits text.
                // Each secondary line gets primary_shift bytes. Lines above
                // the bottom shift it down.
                if let Some(t1_bottom_ls) = line_start(text, bottom_line) {
                    // The bottom line's start is shifted only by inserts ABOVE
                    // it. The primary shift is already in T1; the additional
                    // shifts come from the secondary inserts, which land on
                    // lines primary+1 through primary+lines_below. The bottom
                    // line is primary+lines_below, so the inserts above it are
                    // on lines primary+1..primary+lines_below-1 — that is
                    // (lines_below - 1) inserts.
                    let inserts_above_bottom = lines_below - 1;
                    let post_all_edits_bottom_ls =
                        t1_bottom_ls + inserts_above_bottom * primary_shift;
                    // Redo `'.'`: start of first secondary line in post-all-edits.
                    // The first secondary line is primary+1. In T1, its start is
                    // at line_start(text, primary+1). After the primary insert
                    // (already in T1) and after the secondary insert on that line
                    // itself, the line start doesn't change (insert is on the line).
                    // Inserts on subsequent lines shift those lines, not this one,
                    // and inserts go bottom-to-top, so by the time line primary+1
                    // is processed no further inserts shift it. Its post-all-edits
                    // start = T1 start (already includes primary shift).
                    let secondary_line = primary_line + 1;
                    let redo_dot = line_start(text, secondary_line);
                    self.state.undo_tree_mut().set_precomputed_redo_marks(
                        redo_dot.map(crate::primitives::Offset::new),
                        Some(crate::primitives::Offset::new(post_all_edits_bottom_ls)),
                    );
                }
            }
        }
        // Multi-cursor: replicate positional exit effects to all cursors.
        // Global effects (EndUndoGroup, SetMode, SetStickyColumn, etc.) are
        // kept as single instances — only positional edits get replicated.
        if self.state.multi_cursor().is_active() {
            use crate::effects::Effect;

            // Sync primary selection head with the actual cursor offset.
            let primary = self.state.multi_cursor_mut().selections_mut().primary_mut();
            *primary = crate::primitives::SelectionRange::insert_cursor(
                crate::primitives::Offset::new(cursor),
            );

            let mut positional: Vec<Effect> = Vec::new();
            let mut global: Vec<Effect> = Vec::new();
            for effect in &response.effects {
                if super::super::multi_cursor::is_positional_effect(effect) {
                    positional.push(effect.clone());
                } else {
                    global.push(effect.clone());
                }
            }
            if let (false, Some(repeats)) = (positional.is_empty(), per_cursor_repeats) {
                let mut merged: smallvec::SmallVec<[Effect; 4]> = smallvec::SmallVec::new();
                for (head, effects) in repeats {
                    if head == cursor {
                        merged.extend(positional.iter().cloned());
                    } else {
                        merged.extend(effects);
                    }
                }
                merged.extend(global);
                response.effects = merged;
            } else if !positional.is_empty() {
                let positional_effects: crate::effects::Effects = positional.into_iter().collect();
                let replicated = super::super::multi_cursor::replicate_effects_precise(
                    &positional_effects,
                    self.state.multi_cursor().selections(),
                    &UndoIntent::None, // exit undo group is in global effects
                );
                let mut merged: smallvec::SmallVec<[Effect; 4]> = smallvec::SmallVec::new();
                merged.extend(replicated.into_inner());
                merged.extend(global);
                response.effects = merged;
            }
        }

        let proc_result = super::super::effect_processor::process_effects_with_text(
            &mut self.state,
            &mut self.parser,
            false,
            &mut response,
            Some(text),
            self.options.undolevels(),
            self.options.tabstop(),
            self.options.undo_auto_group_ms(),
            self.resolved_options.cursor_shape_overrides(),
        );
        // Neovim's stop_insert() unconditionally sets b_op_end = *end_insert_pos.
        // For Replace mode and non-block Insert, override mark `]` with
        // end_insert_pos.  Block insert sets `]` correctly in
        // build_insert_exit_effects (pointing to the bottommost insertion),
        // so skip the override for block insert.
        if exit_info.had_accumulated_text && !exit_info.is_block_insert {
            self.state.marks_mut().set(
                crate::primitives::MarkName::CHANGE_END,
                crate::primitives::Mark::new(crate::primitives::Offset::new(
                    exit_info.end_insert_pos,
                )),
            );
        }
        // Correct SetStickyColumn: for mutations, reconstruct post-edit text;
        // for non-mutations, fix exit_finalize's byte-column to virtual column.
        if let Some(cur) = proc_result.last_cursor_offset {
            correct_sticky_column_for_mutations(
                text,
                cur,
                &mut response,
                &mut self.state,
                self.options.tabstop(),
            );
            // exit_finalize uses column_of (byte offset) for SetStickyColumn.
            // When there are no text mutations in the response (count=1, no
            // block insert), correct_sticky_column_for_mutations returns early
            // and the byte-column value persists. Fix it with curswant_of.
            // Only needed when there are no mutations — otherwise the mutation
            // corrector already patched using post-edit text.
            let has_mutations = response.effects.iter().any(|e| {
                matches!(
                    e,
                    crate::effects::Effect::Insert { .. }
                        | crate::effects::Effect::Delete { .. }
                        | crate::effects::Effect::Replace { .. }
                )
            });
            if !has_mutations {
                correct_sticky_column_byte_to_vcol(
                    text,
                    cur,
                    &mut response,
                    &mut self.state,
                    self.options.tabstop(),
                );
            }
        }
        // Override sticky column for o/O<Esc> with auto-indent stripped.
        // Neovim preserves the curswant at the indent level so j/k stay at
        // the indent column.
        if let Some(indent_len) = open_line_indent_override {
            let vcol = crate::primitives::VirtualColumn::new(indent_len);
            // Patch the last SetStickyColumn in the response.
            if let Some(idx) = response.effects.iter().rposition(|e| {
                matches!(
                    e,
                    crate::effects::Effect::SetStickyColumn {
                        column: Some(col)
                    } if !col.is_end_of_line()
                )
            }) {
                response.effects[idx] =
                    crate::effects::Effect::SetStickyColumn { column: Some(vcol) };
            }
            self.state.set_sticky_column(Some(vcol));
        }
        response.provenance = Some(EffectProvenance::new("InsertExit", keystroke_seq));
        response
    }

    /// Execute a command-line result (Cancel, Commit, Edit, or AwaitRegister).
    fn execute_command_line_result<D: Document>(
        &mut self,
        result: &CommandLineResult,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        match result {
            CommandLineResult::Cancel => self.cancel_command_line(),
            CommandLineResult::Commit => self.commit_command_line(ctx),
            CommandLineResult::Edit(edit) => {
                // Tab completion is handled at engine level (needs commands layer)
                if matches!(
                    edit,
                    crate::primitives::CommandLineEdit::CompleteNext
                        | crate::primitives::CommandLineEdit::CompletePrev
                ) {
                    return self.handle_tab_completion(*edit);
                }

                // Ctrl-D: list all matching completions at once
                if matches!(edit, crate::primitives::CommandLineEdit::ListCompletions) {
                    return self.handle_list_completions();
                }

                // Vim: Backspace on an empty command line cancels it (returns to Normal).
                if matches!(edit, crate::primitives::CommandLineEdit::Backspace)
                    && self.state.command_line().input().is_empty()
                {
                    return self.cancel_command_line();
                }

                let effects = crate::commands::command_line::effects::edit(*edit);
                let mut response = Response::with_effects(effects);
                super::super::effect_processor::process_effects(
                    &mut self.state,
                    &mut self.parser,
                    false,
                    &mut response,
                );
                response
                    .host_requests
                    .push(self.sync_command_line_request());

                // Inccommand preview: live substitute highlighting as the user types
                if self.options.inccommand_enabled() {
                    let prompt = self.state.command_line().prompt();
                    if matches!(
                        prompt,
                        crate::state::CommandLinePrompt::Ex
                            | crate::state::CommandLinePrompt::ExVisual
                    ) {
                        let input = self.state.command_line().input().to_owned();
                        let preview_effects = self.live_substitute_preview_effects(&input, &ctx);
                        response.effects.extend(preview_effects);
                    }
                }

                response.kind = ResponseKind::Pending;
                response
            }
            CommandLineResult::AwaitRegister => {
                self.state.command_line_mut().set_awaiting_register(true);
                let mut response = Response::pending_response();
                response
                    .host_requests
                    .push(self.sync_command_line_request());
                response
            }
            CommandLineResult::OpenCommandWindow => {
                // Ctrl-F: capture current prompt + input, cancel the command line,
                // then emit OpenCommandWindow with history and prefill.
                let prompt = self.state.command_line().prompt();
                let prefill = {
                    let input = self.state.command_line().input();
                    if input.is_empty() {
                        None
                    } else {
                        Some(compact_str::CompactString::from(input))
                    }
                };
                let history: Vec<compact_str::CompactString> = self
                    .state
                    .command_line()
                    .history_for_prompt(prompt)
                    .iter()
                    .cloned()
                    .collect();

                // Cancel the active command-line session (returns to normal mode)
                let mut response = self.cancel_command_line();

                // Append the OpenCommandWindow effect
                response
                    .effects
                    .push(crate::effects::Effect::OpenCommandWindow {
                        prompt,
                        history,
                        prefill,
                    });
                response
            }
        }
    }

    /// Handle ZZ/ZQ by emitting host requests directly (the executor cannot emit them).
    fn try_intercept_quit_command(&mut self, command: &Command) -> Option<Response> {
        if let Command::Prefix {
            command: ref prefix_cmd,
            ..
        } = *command
        {
            match prefix_cmd {
                PrefixCommand::WriteQuit => {
                    let mut response = Response::pending_response();
                    response.host_requests.push(HostRequest::WriteQuit {
                        meta: self.host.sequencer.next_meta(),
                        force: false,
                    });
                    return Some(response);
                }
                PrefixCommand::ForceQuit => {
                    let mut response = Response::pending_response();
                    response.host_requests.push(HostRequest::Quit {
                        meta: self.host.sequencer.next_meta(),
                        force: true,
                    });
                    return Some(response);
                }
                _ => {}
            }
        }
        None
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "returns Result for consistency with other plan execution paths"
    )]
    pub(in crate::execution::engine) fn execute_effect_plan<D: Document>(
        &mut self,
        command: Command,
        was_repeat: bool,
        ctx: InputContext<'_, D, Validated>,
    ) -> Result<Response, PipelineError> {
        if let Some(response) = self.try_intercept_quit_command(&command) {
            return Ok(response);
        }

        // Sticky sub-mode entry — intercept before executor
        if let Command::Prefix {
            command: PrefixCommand::StickyEnter { target },
            ..
        } = &command
        {
            self.sticky_session = Some(super::StickySession::new(*target));
            match target {
                StickyTarget::Window => {
                    self.parser
                        .set_state(crate::grammar::InputState::AwaitingWindowCommand {
                            count: None,
                            register: None,
                        });
                }
                StickyTarget::ZPrefix => {
                    self.parser
                        .set_state(crate::grammar::InputState::AwaitingPrefix {
                            count: None,
                            register: None,
                            prefix: 'z',
                            operator: None,
                            force_type: None,
                        });
                }
            }
            let effects = crate::effects::Effects::new()
                .show_message(format!("-- {} --", target.display_name()));
            return Ok(Response::with_effects(effects));
        }

        // g-/g+ — undo branch navigation (intercept before executor,
        // needs engine-level undo tree access via apply_undo_navigation)
        if let Command::Prefix {
            count,
            command: ref prefix_cmd @ (PrefixCommand::UndoEarlier | PrefixCommand::UndoLater),
            ..
        } = command
        {
            use crate::execution::executor_ex::UndoNavigation;
            use crate::grammar::types::TimeAmount;
            let nav = match prefix_cmd {
                PrefixCommand::UndoEarlier => {
                    UndoNavigation::Earlier(TimeAmount::Changes(count.get()))
                }
                PrefixCommand::UndoLater => UndoNavigation::Later(TimeAmount::Changes(count.get())),
                _ => unreachable!(),
            };
            let effects = self.apply_undo_navigation(&nav);
            return Ok(Response::with_effects(effects));
        }

        // @: — repeat last ex command
        if let Command::Macro(crate::grammar::MacroKind::RepeatLastEx { count }) = command {
            return Ok(self.execute_repeat_last_ex(count.get(), ctx));
        }

        // Borrow document text for effect processing.
        // Used for: (1) changelist same-line dedup (line number computation),
        // (2) auto-emit SetStickyColumn (curswant) after cursor-moving ops.
        // Always captured — non-mutating commands (motions, searches, marks)
        // still need text for sticky column computation.
        // Note: `doc()` returns `&'doc D` (not tied to `&self`), so this
        // borrow survives moving `ctx` into `ExecutionContext` — no clone needed.
        let doc_text = Some(ctx.doc().text());
        let input_cursor_offset = ctx.cursor_offset();
        // Merge engine-level providers with per-call providers
        let ctx = if self.engine_providers.has_any() {
            let merged = self.engine_providers.merge_with(ctx.providers());
            ctx.with_providers(merged)
        } else {
            ctx
        };
        // Capture semantic intent alongside repeat recording.
        // Must happen before `command` is moved into `execute()`.
        // The parser already saves last_command for dot-repeat; here we
        // derive and store the higher-level intent on VimState for g. replay.
        if command.properties().repeat != crate::primitives::RepeatBehavior::Skip {
            let visual_type = match self.state.mode() {
                Mode::Visual(vt) => Some(vt),
                _ => None,
            };
            if let Some(intent) = crate::state::capture_intent(&command, visual_type) {
                self.state.repeat_state_mut().save_intent(intent);
            }
        }

        // ── PreCommand hook ──────────────────────────────────────
        // Fire before command execution. If any handler returns Cancel,
        // skip execution entirely and return an empty response. During
        // speculative execution (fork_active), hooks are suppressed.
        if !self.fork_active {
            let hook_ctx = super::hooks::HookContext {
                point: super::hooks::HookPoint::PreCommand,
                mode: self.state.mode(),
                effects: &[], // no effects yet — command has not executed
                state: &self.state,
            };
            if self
                .cold
                .hooks
                .fire(super::hooks::HookPoint::PreCommand, &hook_ctx)
                == super::hooks::HookAction::Cancel
            {
                let mut response = Response::consumed_empty();
                response.precommand_cancelled = true;
                return Ok(response);
            }
        }

        // ── Expression register re-evaluation for dot-repeat ────
        // When dot-repeating a command that uses the expression register
        // (`"=p`, etc.), re-evaluate the expression to get a fresh value
        // instead of replaying the stale cached result. This matches Vim's
        // behaviour where `"=expr<CR>p` followed by `.` re-evaluates `expr`.
        //
        // Two-phase approach:
        // 1. Synchronous: re-evaluate the expression using the engine's
        //    internal evaluator and update the expression register immediately.
        //    This ensures the executor sees a fresh value.
        // 2. Async: emit `HostRequest::EvaluateExpression` so the host can
        //    provide a more accurate result for complex expressions. The host
        //    will call `set_expression_result()` which updates the register
        //    for subsequent uses.
        let mut expr_reeval_request: Option<HostRequest> = None;
        if was_repeat {
            if let Some(reg) = command.register() {
                if reg.is_expression() {
                    if let Some(expr_text) =
                        self.state.last_expression_text().map(ToOwned::to_owned)
                    {
                        // Phase 1: synchronous internal re-evaluation
                        let result = super::insert::evaluate_simple_expression(&expr_text);
                        if !result.is_empty() {
                            self.state.registers_mut().set_expression_result(
                                crate::primitives::RegisterContent::char_wise(result),
                            );
                        }
                        // Phase 2: prepare async host request for more accurate evaluation
                        let meta = self.host.sequencer.next_meta();
                        expr_reeval_request = Some(HostRequest::EvaluateExpression {
                            meta,
                            expression: compact_str::CompactString::from(expr_text.as_str()),
                        });
                    }
                }
            }
        }

        // Capture whether this command preserves mark '.' before moving command.
        // Commands like Ctrl-A/Ctrl-X modify text but don't update mark '.'
        // in Neovim — we save and restore it around effect processing.
        // Exception: dot-repeat and macro-replayed Ctrl-A/Ctrl-X DO update
        // mark '.', because Neovim goes through the operator path for repeat
        // and macro replay re-executes the full command.
        let is_macro_replaying = !self.typeahead.macro_stack.is_empty();
        let preserve_mark_dot = command.preserves_mark_dot()
            && !was_repeat
            && !is_macro_replaying
            && !self.state.mode().is_visual();

        // Multi-cursor: detect paste commands before execution consumes the command.
        // If this is a paste with a multi-entry register, we'll fix up the
        // replicated Insert effects after replication.
        let paste_zip_content = {
            let mc = self.state.multi_cursor();
            if mc.is_active() {
                detect_paste_zip_content(&command, &self.state, &self.resolved_options)
            } else {
                None
            }
        };

        // Multi-cursor: capture range source for per-cursor range recomputation.
        let range_source = self
            .state
            .multi_cursor()
            .is_active()
            .then(|| extract_range_source(&command))
            .flatten();

        // ── Hybrid multi-cursor dispatch ────────────────────────────────
        //
        // Three paths:
        //
        // 1. Per-cursor re-execution: content-dependent commands are executed
        //    independently at each cursor against the T0 document snapshot.
        //    This produces correct text mutations for every cursor.
        //
        // 2. Algebraic rebase: position-independent commands are executed once
        //    at the primary cursor and rebased to all cursors (existing path).
        //
        // 3. Single cursor / global-only: no replication needed.
        //
        let mc_active = self.state.multi_cursor().is_active();
        let is_global_only = command.is_global_only();
        let use_per_cursor = mc_active && command.is_content_dependent() && !is_global_only;

        // Track cursor deltas and register override from per-cursor execution.
        let mut per_cursor_deltas: Option<Vec<(usize, Offset, i64)>> = None;
        let mut per_cursor_register: Option<PerCursorRegisterOverride> = None;
        // Primary cursor's net byte delta, used to update selections on the
        // algebraic rebase path.
        let mut rebase_primary_net_delta: Option<i64> = None;

        let (effects, primary_register_info) = if use_per_cursor {
            // ── Per-cursor re-execution path ────────────────────────
            let result =
                self.execute_per_cursor(&command, &ctx, self.state.multi_cursor().selections());
            per_cursor_deltas = Some(result.cursor_deltas);
            per_cursor_register = result.register_override;
            let reg_info = extract_primary_register_info(result.effects.as_slice());

            // Materialise undo markers from the per-cursor intent.
            // This is the SINGLE WRAPPING POINT for per-cursor effects.
            let mut effect_vec: Vec<Effect> = result.effects.into_vec();
            UndoIntent::materialize_undo_markers(&mut effect_vec, &result.undo_intent);
            let effects: Effects = effect_vec.into_iter().collect();

            (effects, reg_info)
        } else {
            // ── Single execution (primary cursor) ───────────────────
            let exec_ctx = ExecutionContext::new(ctx, &self.state, &self.resolved_options);
            let executor_output = super::super::executor::execute(command, &exec_ctx);

            // Extract multi-cursor command before effects are moved.
            let mc_cmd_from_executor = executor_output.multi_cursor_command;

            // Multi-cursor: capture primary register info before replication.
            let primary_register_info = if mc_active {
                extract_primary_register_info(executor_output.effects.as_slice())
            } else {
                None
            };

            // Handle multi-cursor commands from gb/gB/gs actions.
            // These bypass normal replication — they modify cursor state directly.
            if let Some((mc_cmd, count)) = mc_cmd_from_executor {
                let doc_text_owned = exec_ctx.doc().text().to_owned();
                // Capture visual selection before dropping exec_ctx: Visual gb
                // uses the selected text as the match pattern.
                let visual_selection = if self.state.mode().is_visual() {
                    exec_ctx.selection()
                } else {
                    None
                };
                // Drop exec_ctx so we can borrow self.state mutably.
                drop(exec_ctx);

                // Visual gb: when AddNextMatch is triggered from Visual mode,
                // extract the selection text and use it as the match pattern
                // instead of word-under-cursor. Then exit Visual mode.
                if let Some(sel) = visual_selection {
                    if matches!(
                        mc_cmd,
                        crate::state::MultiCursorCommand::AddNextMatch { .. }
                    ) {
                        let sel_start = sel.start().get();
                        let sel_end = sel.end().get();
                        // Include the character under the cursor (Vim visual
                        // selection is inclusive of the head character).
                        let inclusive_end =
                            crate::primitives::next_char_boundary(&doc_text_owned, sel_end)
                                .min(doc_text_owned.len());
                        if inclusive_end > sel_start && inclusive_end <= doc_text_owned.len() {
                            let sel_text = &doc_text_owned[sel_start..inclusive_end];
                            // Set the match-search state so add_next_match uses
                            // the selection text (first priority in pattern resolution).
                            self.state.multi_cursor_mut().set_match_search(
                                crate::state::MatchSearchState {
                                    pattern: sel_text.into(),
                                    last_match_offset: sel_start,
                                    whole_word: false,
                                },
                            );
                            // Also set the search register for consistency
                            // (subsequent n/N should search for this pattern).
                            self.state
                                .search_mut()
                                .set_pattern(sel_text, crate::primitives::SearchDirection::Forward);
                        }
                        // Exit Visual mode -> Normal (the MC command will
                        // activate multi-cursor, resulting in Normal+MC).
                        self.state.set_mode(Mode::Normal);
                    }
                }

                let search_pat = self.state.search().pattern().map(str::to_owned);
                let mc_ctx = crate::execution::multi_cursor_executor::MultiCursorContext {
                    text: &doc_text_owned,
                    search_pattern: search_pat.as_deref(),
                    line_count: crate::commands::helpers::line_count(&doc_text_owned).max(1),
                };
                let mut mc_effects = crate::effects::Effects::new();
                for _ in 0..count {
                    match crate::execution::multi_cursor_executor::execute_multi_cursor_command(
                        &mut self.state,
                        &mc_cmd,
                        &mc_ctx,
                    ) {
                        Ok(effects) => {
                            for effect in effects {
                                mc_effects.push(effect);
                            }
                        }
                        Err(err) => {
                            mc_effects.extend(crate::commands::ex::effects::show_error(err));
                            break;
                        }
                    }
                }
                // Return mc_effects directly — no replication needed.
                (mc_effects, primary_register_info)
            } else {
                // Multi-cursor replication: algebraic rebase for PI commands,
                // or single-cursor / global-only passthrough.
                let effects = if mc_active && !is_global_only {
                    // Capture the primary net delta BEFORE replication. For PI
                    // commands, all cursor groups produce the same net delta.
                    rebase_primary_net_delta =
                        Some(crate::effects::algebra::compute_length_change(
                            executor_output.effects.as_slice(),
                        ));

                    let mc = self.state.multi_cursor();
                    let replicated = super::super::multi_cursor::replicate_effects_precise(
                        &executor_output.effects,
                        mc.selections(),
                        &executor_output.undo_intent,
                    );
                    // Paste-zipping: replace Insert text in each cursor's group
                    // with the corresponding register entry for per-cursor paste.
                    if let Some(ref zip) = paste_zip_content {
                        apply_paste_zip(&replicated, mc.selections(), zip)
                    } else {
                        replicated
                    }
                } else {
                    executor_output.effects
                };

                (effects, primary_register_info)
            }
        };

        let mut response = Response::with_effects(effects);

        // Set undo tree cursor hint to input cursor before effect processing
        self.state.set_undo_cursor_hint(input_cursor_offset);

        // Save mark '.' for commands that should not update it (Ctrl-A/Ctrl-X).
        let saved_mark_dot = if preserve_mark_dot {
            self.state
                .marks()
                .get(crate::primitives::MarkName::LAST_CHANGE)
        } else {
            None
        };

        // Single-pass: sync state + intercept dot-repeat + collect macros + track cursor
        let result = super::super::effect_processor::process_effects_with_text(
            &mut self.state,
            &mut self.parser,
            was_repeat,
            &mut response,
            doc_text,
            self.options.undolevels(),
            self.resolved_options.tabstop(),
            self.options.undo_auto_group_ms(),
            self.resolved_options.cursor_shape_overrides(),
        );

        // Restore mark '.' for commands that should preserve it.
        if let Some(mark) = saved_mark_dot {
            self.state
                .marks_mut()
                .set(crate::primitives::MarkName::LAST_CHANGE, mark);
        }
        // Correct auto-emitted SetStickyColumn when text mutations made the
        // pre-edit document stale. The effect processor computed curswant_of on
        // pre-edit text, but the cursor offset is post-edit. Reconstruct the
        // post-edit text and recompute.
        if let (Some(cursor), Some(text)) = (result.last_cursor_offset, doc_text) {
            correct_sticky_column_for_mutations(
                text,
                cursor,
                &mut response,
                &mut self.state,
                self.resolved_options.tabstop(),
            );
        }

        // Multi-cursor selection update (algebraic rebase and per-cursor paths).
        // After process_effects_with_text and sticky column correction,
        // update selections so subsequent commands see correct positions.
        if let Some(ref deltas) = per_cursor_deltas {
            // Per-cursor re-execution path: use pre-computed deltas.
            super::per_cursor::update_selections_from_deltas(
                self.state.multi_cursor_mut().selections_mut(),
                deltas,
                true,
            );
        }
        // Note: the algebraic rebase selection update is deferred to after
        // the register override, which reads pre-edit selections from T0.

        // After undo/redo, clear sticky column so it gets recomputed from
        // the actual post-undo cursor position. In Neovim, u_undoredo sets
        // w_set_curswant=TRUE which defers curswant computation to
        // update_curswant() after the undo text changes are applied.
        //
        // We can't compute the correct curswant here because we only have
        // the pre-undo text, but the undo cursor refers to the post-undo
        // text. Clearing sticky_column signals "recompute from actual cursor"
        // — the host/test-runner computes it after applying the undo.
        if result.last_undo_redo_cursor.is_some() {
            self.state.set_sticky_column(None);
            response
                .effects
                .push(crate::effects::Effect::SetStickyColumn { column: None });
        }

        if result.ended_repeat {
            self.is_repeating = false;
        }

        // Handle macro recording signals from effect processor
        if let Some(register) = result.recording_started {
            self.recording.buffer = Some((register, String::new()));
        }
        if result.recording_stopped {
            self.flush_recording(&mut response);
        }

        // Process collected macro plays (already removed from effects)
        if !result.macro_plays.is_empty() {
            self.process_macro_plays(result.macro_plays, &mut response);
        }

        // Inject saved text for dot-repeat if intercepted.
        // With multi-cursor active, build the positional effects, replicate
        // them per cursor, then emit the global effects exactly once.
        if let Some(count) = result.repeat_count {
            let indent_provider = self.engine_providers.indent.as_deref();
            let mut repeat = super::super::effect_processor::RepeatText {
                count,
                input_cursor_offset,
                last_cursor_offset: result.last_cursor_offset,
                doc_text,
                indent_provider,
                tabstop: self.resolved_options.tabstop(),
                autoindent: self.resolved_options.autoindent(),
                format: crate::commands::insert::wrap::FormatPolicy::from_options(
                    &self.resolved_options,
                ),
            };
            if self.state.multi_cursor().is_active() {
                let primary_offset = self.state.multi_cursor().selections().primary().head();
                repeat.last_cursor_offset = Some(primary_offset);
                // Vim retypes the text at every cursor, so with formatting
                // each cursor breaks its own line. That is done here when the
                // repeated command made no edit of its own before the text.
                let edited = response.effects.iter().any(|e| {
                    matches!(
                        e,
                        crate::effects::Effect::Insert { .. }
                            | crate::effects::Effect::Delete { .. }
                            | crate::effects::Effect::Replace { .. }
                    )
                });
                if let (true, false, Some(text)) = (repeat.format.is_active(), edited, doc_text) {
                    let mut heads: Vec<Offset> = self
                        .state
                        .multi_cursor()
                        .selections()
                        .iter()
                        .map(|s| s.head())
                        .collect();
                    heads.sort_unstable_by(|a, b| b.cmp(a));
                    heads.dedup();
                    for head in heads {
                        repeat.last_cursor_offset = Some(head);
                        let Some(mut positional) =
                            super::super::effect_processor::build_repeat_positional_effects(
                                &self.state,
                                &repeat,
                            )
                        else {
                            continue;
                        };
                        format_repeated_text(&mut positional, text, &repeat.format);
                        for mut effect in positional.into_inner() {
                            super::super::effect_processor::sync_effect_mut(
                                &mut self.state,
                                &mut self.parser,
                                &mut effect,
                                doc_text,
                                self.options.undolevels(),
                            );
                            response.effects.push(effect);
                        }
                    }
                } else if let Some(positional) =
                    super::super::effect_processor::build_repeat_positional_effects(
                        &self.state,
                        &repeat,
                    )
                {
                    let replicated = super::super::multi_cursor::replicate_effects_precise(
                        &positional,
                        self.state.multi_cursor().selections(),
                        &UndoIntent::None,
                    );
                    for mut effect in replicated.into_inner() {
                        super::super::effect_processor::sync_effect_mut(
                            &mut self.state,
                            &mut self.parser,
                            &mut effect,
                            doc_text,
                            self.options.undolevels(),
                        );
                        response.effects.push(effect);
                    }
                }
                super::super::effect_processor::inject_repeat_global_effects(
                    &mut self.state,
                    &mut self.parser,
                    count,
                    primary_offset.get(),
                    &mut response,
                    doc_text,
                    self.options.undolevels(),
                );
            } else {
                super::super::effect_processor::inject_repeat_text(
                    &mut self.state,
                    &mut self.parser,
                    &mut response,
                    self.options.undolevels(),
                    &repeat,
                );
            }
        }

        // Clipboard routing: if unnamed register was written AND clipboard option is set,
        // emit CopyToClipboard so the host can mirror the yank/delete to the system clipboard.
        // `unnamed` → `*` (primary selection), `unnamedplus` → `+` (system clipboard).
        if let Some(ref text) = result.unnamed_register_written {
            if self.options.clipboard_has_unnamed() {
                response
                    .effects
                    .push(crate::effects::Effect::CopyToClipboard {
                        text: text.clone(),
                        register: crate::primitives::RegisterName::SELECTION,
                    });
            }
            if self.options.clipboard_has_unnamedplus() {
                response
                    .effects
                    .push(crate::effects::Effect::CopyToClipboard {
                        text: text.clone(),
                        register: crate::primitives::RegisterName::CLIPBOARD,
                    });
            }
        }

        // Promote OperatorFilter effects to host requests (only when filter command)
        if let Some(doc_text) = doc_text {
            self.promote_effects_to_host_requests(&mut response, doc_text);
        }

        // Multi-cursor register override.
        if let Some(reg_override) = per_cursor_register {
            // Per-cursor path: apply the consolidated multi-entry register
            // directly. This replaces the algebraic path's heuristic-based
            // register enrichment with exact per-cursor register text.
            for (name, content) in reg_override.pairs {
                self.state.registers_mut().set(name, content);
            }
        } else if let (Some(info), Some(text)) = (primary_register_info, doc_text) {
            // Algebraic rebase path: enrich single-entry register with
            // per-cursor text via range recomputation heuristic.
            override_registers_with_multi_cursor_entries(
                &mut self.state,
                text,
                &info,
                range_source,
                &self.resolved_options,
            );
        }

        // Algebraic rebase selection update.
        // Placed AFTER register override because that logic reads pre-edit
        // selections to compute per-cursor register text from the T0 document.
        //
        // This runs for ALL modes, including when the command transitions to
        // insert/replace mode (o/O/s/S). The insert-mode path
        // (execute_insert_command) manages selection updates for subsequent
        // insert-mode keystrokes, but the initial InsertEntry is dispatched
        // here and its text mutations (newline insertion for o/O, character
        // deletion for s/S) shift cursor positions that must be reflected in
        // the selections before the next keystroke arrives.
        if let Some(primary_delta) = rebase_primary_net_delta {
            super::per_cursor::update_selections_after_rebase(
                &mut self.state,
                &response,
                primary_delta,
            );
        }

        // Emit the deferred expression re-evaluation host request (if any).
        // The synchronous internal evaluation already updated the register
        // before the executor ran; this async request lets the host provide
        // a more accurate result for complex expressions.
        if let Some(req) = expr_reeval_request {
            response.host_requests.push(req);
        }

        // Entering Visual Block with active MC clears secondary cursors:
        // Visual Block has its own column-based cursor model that is
        // incompatible with match-based multi-cursor state.
        if matches!(
            self.state.mode(),
            Mode::Visual(crate::primitives::VisualType::Block)
        ) && self.state.multi_cursor().is_active()
        {
            let mc_ctx = crate::execution::multi_cursor_executor::MultiCursorContext {
                text: "",
                search_pattern: None,
                line_count: 0,
            };
            let _ = crate::execution::multi_cursor_executor::execute_multi_cursor_command(
                &mut self.state,
                &crate::state::MultiCursorCommand::ClearSecondary,
                &mc_ctx,
            );
        }

        Ok(response)
    }

    /// Process collected `PlayMacro` effects: resolve registers, parse keys, push frames.
    ///
    /// Takes pre-collected macro plays from [`ProcessResult`] — no re-scanning needed.
    fn process_macro_plays(
        &mut self,
        macro_plays: SmallVec<[(RegisterName, u32); 2]>,
        response: &mut Response,
    ) {
        let was_empty = self.typeahead.macro_stack.is_empty();
        #[cfg(feature = "engine-tracing")]
        let first_register = macro_plays.first().map(|(r, _)| r.char());
        let converted: SmallVec<[(RegisterName, std::num::NonZeroU32); 2]> = macro_plays
            .into_iter()
            .map(|(reg, c)| {
                (
                    reg,
                    std::num::NonZeroU32::new(c).unwrap_or(std::num::NonZeroU32::MIN),
                )
            })
            .collect();
        super::macro_replay::process_macro_plays(
            &mut self.state,
            &mut self.typeahead.macro_stack,
            converted,
            response,
            &self.options,
        );
        // When macro replay starts, enable undo group merging so the entire
        // N@a produces a single undo entry (Vim behavior).
        if was_empty && !self.typeahead.macro_stack.is_empty() {
            self.state.undo_tree_mut().begin_merge();
            trace_event!(
                self,
                TraceEvent::UndoMergeBegin {
                    register: first_register.unwrap_or('?'),
                }
            );
        }
    }

    /// Execute `@:` — repeat the last ex command from history.
    ///
    /// Looks up the most recent entry in the ex command history and
    /// re-runs it through the ex executor. If there is no previous
    /// command, emits `E34: No previous command`.
    ///
    /// Vim ignores the count for `@:` and always runs the command once.
    fn execute_repeat_last_ex<D: Document>(
        &mut self,
        count: u32,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        use crate::commands::ex::effects as ex_effects;
        use crate::errors::VimError;

        // Look up the last ex command (newest-last in history deque).
        let last_cmd = self.state.command_line().ex_history().back().cloned();

        let Some(cmd_str) = last_cmd else {
            return Response::with_effects(ex_effects::show_error(VimError::NoPreviousCommand));
        };

        let exec_ctx =
            crate::execution::ExecutionContext::new(ctx, &self.state, &self.resolved_options);
        // Capture doc text while exec_ctx is alive — needed later for multi-cursor
        // commands that execute after exec_ctx's immutable borrow on self.state ends.
        let mc_doc_text = exec_ctx.doc().text().to_owned();
        let mut combined = Response::with_effects(crate::effects::Effects::new());
        match crate::execution::executor_ex::execute_ex_line(
            cmd_str.as_str(),
            &exec_ctx,
            &mut self.host.sequencer,
        ) {
            Ok(mut output) => {
                crate::execution::effect_processor::sync_effects(
                    &mut self.state,
                    &mut self.parser,
                    output.effects.as_mut_slice(),
                );

                if !output.set_assignments.is_empty() {
                    let set_efx = crate::execution::executor_ex::apply_set_assignments(
                        output.set_scope,
                        &mut self.options,
                        &mut self.buffer_overrides,
                        &mut self.window_overrides,
                        &output.set_assignments,
                    );
                    self.rebuild_resolved_cache();
                    self.rebuild_langmap_if_needed();
                    combined.extend_effects(set_efx);
                }

                for change in &output.mapping_changes {
                    self.apply_mapping_change(change);
                }
                for change in &output.abbrev_changes {
                    let abbrev_efx = self.apply_abbrev_change(change);
                    combined.extend_effects(abbrev_efx);
                }
                for change in &output.handler_changes {
                    self.apply_handler_change(change);
                }
                if let Some(leader) = output.leader_change {
                    self.keymap
                        .set_leader(crate::keymap::KeyEvent::char(leader));
                }
                if output.clear_message_history {
                    self.state.message_history_mut().clear();
                }
                if let Some(nav) = &output.undo_navigation {
                    let undo_efx = self.apply_undo_navigation(nav);
                    combined.extend_effects(undo_efx);
                }

                if let Some(mc_cmd) = &output.multi_cursor_command {
                    let search_pat = self.state.search().pattern().map(str::to_owned);
                    let mc_ctx = crate::execution::multi_cursor_executor::MultiCursorContext {
                        text: &mc_doc_text,
                        search_pattern: search_pat.as_deref(),
                        line_count: crate::commands::helpers::line_count(&mc_doc_text).max(1),
                    };
                    match crate::execution::multi_cursor_executor::execute_multi_cursor_command(
                        &mut self.state,
                        mc_cmd,
                        &mc_ctx,
                    ) {
                        Ok(effects) => {
                            let mut mc_efx = crate::effects::Effects::new();
                            for effect in effects {
                                mc_efx.push(effect);
                            }
                            combined.extend_effects(mc_efx);
                        }
                        Err(err) => {
                            combined.extend_effects(ex_effects::show_error(err));
                        }
                    }
                }

                combined.extend_effects(output.effects);
                for req in output.host_requests {
                    combined.host_requests.push(req);
                }
            }
            Err(err) => {
                combined.extend_effects(ex_effects::show_error(err));
            }
        }

        // For count > 1, inject remaining repetitions into the typeahead buffer.
        // Each repetition is `:cmd<CR>` — the host applies effects between
        // iterations so subsequent executions see updated document state.
        if count > 1 {
            use super::typeahead::{TypeaheadEntry, TypeaheadFlags};
            let flags = TypeaheadFlags::noremap_rhs();
            let mut entries = Vec::new();
            for _ in 1..count {
                // Inject `:` to enter command line
                entries.push(TypeaheadEntry::new(
                    crate::keymap::KeyEvent::char(':'),
                    flags,
                ));
                // Inject each char of the command
                for ch in cmd_str.chars() {
                    entries.push(TypeaheadEntry::new(
                        crate::keymap::KeyEvent::char(ch),
                        flags,
                    ));
                }
                // Inject <CR> to execute
                entries.push(TypeaheadEntry::new(crate::keymap::KeyEvent::enter(), flags));
            }
            self.typeahead.buffer.inject_front(entries);
        }

        combined
    }
}

/// Map an [`InsertMode`] to its corresponding [`ReturnTo`] variant.
///
/// Used by Ctrl-O to record which mode to restore after the one-shot command.
const fn return_to_for_insert_mode(mode: InsertMode) -> crate::primitives::ReturnTo {
    match mode {
        InsertMode::Insert => crate::primitives::ReturnTo::Insert,
        InsertMode::Replace => crate::primitives::ReturnTo::Replace,
        InsertMode::VirtualReplace => crate::primitives::ReturnTo::VirtualReplace,
    }
}

/// Whether `command` types a character that formatting looks at: a plain or
/// literal character, but not a newline. CTRL-Y and CTRL-E are left out, as
/// Vim's ins_ctrl_ey() turns 'textwidth' off while it inserts the copy.
const fn is_formatted_char(command: &Command) -> bool {
    matches!(command, Command::Insert(InsertKind::LiteralChar { .. }))
        || matches!(command, Command::Insert(InsertKind::Char { char }) if *char != '\n')
}

/// Break the lines of the text a dot-repeat inserts with `effects` into
/// `text`, as Vim retypes it. In Replace mode only the text past the end of
/// the line formats.
fn format_repeated_text(
    effects: &mut crate::effects::Effects,
    text: &str,
    policy: &crate::commands::insert::wrap::FormatPolicy<'_>,
) {
    let Some((at, len)) = effects.as_slice().iter().find_map(|e| match e {
        Effect::Insert { offset, text } => Some((offset.get(), text.len())),
        _ => None,
    }) else {
        return;
    };
    let deleted_from = effects.as_slice().iter().find_map(|e| match e {
        Effect::Delete { range } => Some(range.start().get()),
        _ => None,
    });
    let overwritten = if deleted_from.is_some() {
        crate::commands::insert::wrap::overwritten_chars(effects.as_slice(), text, at)
    } else {
        0
    };
    let _ = crate::commands::insert::wrap::format_inserted_text(
        effects,
        text,
        &[],
        at..at + len,
        policy,
        None,
        overwritten,
    );
}

/// Vim's ins_bs(): a backspace at the start of the line the insert started
/// on moves that start to the end of the line above. The line length that
/// 'l' looks at stays. `cursor` is where the backspace was typed in `text`,
/// and `after` where it left the cursor.
fn move_insert_start_on_backspace(
    start: &mut crate::state::InsertStart,
    text: &str,
    cursor: usize,
    after: usize,
) {
    let line = crate::commands::helpers::line_of(text, cursor);
    let at_line_start = crate::commands::helpers::line_start_for_offset(text, cursor) == cursor;
    if at_line_start && line > 0 && start.line == line && after < cursor {
        let above = crate::commands::helpers::line_start(text, line - 1).unwrap_or(0);
        start.line = line - 1;
        start.col = cursor.saturating_sub(1) - above;
    }
}

/// The line a cursor is on once the text edits in `effects` are applied to
/// `text`, where they leave it at `cursor`. It was at `before` in `text`.
/// Only an edit that may add or remove a line break needs the text rebuilt.
fn cursor_line_after(
    text: &str,
    effects: &[Effect],
    before: usize,
    cursor: usize,
) -> Option<usize> {
    let changes_lines = effects.iter().any(|e| match e {
        Effect::Insert { text: t, .. } => t.contains('\n'),
        Effect::Delete { .. } | Effect::Replace { .. } => true,
        _ => false,
    });
    if !changes_lines {
        return Some(crate::commands::helpers::line_of(text, before));
    }
    let after = crate::commands::insert::wrap::apply_text_effects(text, effects)?;
    Some(crate::commands::helpers::line_of(
        &after,
        cursor.min(after.len()),
    ))
}

/// Whether one of the cursors at `others` is on the same line as `cursor`.
fn cursor_shares_line(text: &str, cursor: usize, others: impl Iterator<Item = usize>) -> bool {
    let start = crate::commands::helpers::line_start_for_offset(text, cursor);
    let end = text
        .get(cursor..)
        .and_then(|rest| rest.find('\n'))
        .map_or(text.len(), |n| cursor + n);
    others
        .filter(|&head| head != cursor)
        .any(|head| (start..=end).contains(&head))
}

/// Correct the auto-emitted `SetStickyColumn` when text mutations made the
/// pre-edit text stale.
///
/// The effect processor computes `curswant_of(pre_edit_text, cursor_offset)`,
/// which produces wrong results when Insert/Delete/Replace effects changed
/// the document (the cursor offset is post-edit but the text is pre-edit).
///
/// This function reconstructs the post-edit text by replaying mutations,
/// recomputes the column using tab-aware `curswant_of`, and patches both
/// the response effects and the engine state.
///
/// Only runs when there are text mutations — for non-mutating operations,
/// the motion dispatcher already emits correct sticky columns via
/// `curswant_of`, and the auto-emit (when it fires) uses the correct
/// document text since pre-edit == post-edit.
fn correct_sticky_column_for_mutations(
    pre_edit_text: &str,
    cursor_offset: crate::primitives::Offset,
    response: &mut Response,
    state: &mut crate::state::VimState,
    tabstop: usize,
) {
    use crate::effects::Effect;

    // Only correct when text mutations made the pre-edit text stale.
    let has_mutations = response.effects.iter().any(|e| {
        matches!(
            e,
            Effect::Insert { .. } | Effect::Delete { .. } | Effect::Replace { .. }
        )
    });
    if !has_mutations {
        return;
    }

    // Find the last SetStickyColumn that ISN'T END_OF_LINE (those are intentional).
    // The auto-emitted one will have a concrete column value.
    let sticky_idx = response.effects.iter().rposition(|e| {
        matches!(
            e,
            Effect::SetStickyColumn {
                column: Some(col)
            } if !col.is_end_of_line()
        )
    });
    let Some(idx) = sticky_idx else {
        return;
    };

    // Reconstruct post-edit text by applying mutations in order.
    let mut post_text = pre_edit_text.to_owned();
    for effect in &response.effects {
        match effect {
            Effect::Insert { offset, text } => {
                let pos = offset.get().min(post_text.len());
                post_text.insert_str(pos, text);
            }
            Effect::Delete { range } => {
                let start = range.start().get().min(post_text.len());
                let end = range.end().get().min(post_text.len());
                if start < end {
                    post_text.drain(start..end);
                }
            }
            Effect::Replace { range, text } => {
                let start = range.start().get().min(post_text.len());
                let end = range.end().get().min(post_text.len());
                if start <= end {
                    post_text.replace_range(start..end, text);
                }
            }
            _ => {}
        }
    }

    // Recompute column using post-edit text and tab-aware curswant_of.
    let clamped = cursor_offset.get().min(post_text.len());
    let col = crate::commands::helpers::curswant_of(&post_text, clamped, tabstop);
    let vcol = crate::primitives::VirtualColumn::new(col);

    // Only patch if the value actually changed (avoid unnecessary state writes).
    if let Some(Effect::SetStickyColumn {
        column: Some(existing),
    }) = response.effects.get(idx)
    {
        if *existing == vcol {
            return;
        }
    }

    // Patch the SetStickyColumn effect.
    if let Some(slot) = response.effects.get_mut(idx) {
        *slot = Effect::SetStickyColumn { column: Some(vcol) };
        state.set_sticky_column(Some(vcol));
    }
}

/// Fix `SetStickyColumn` emitted by `exit_finalize` which uses byte-column
/// (`column_of`) instead of virtual column (`curswant_of`).
///
/// Called after insert exit processing when no text mutations are present
/// in the response (count=1, no block insert, no strip). The pre-edit text
/// is the final document state, so we compute `curswant_of` directly.
fn correct_sticky_column_byte_to_vcol(
    text: &str,
    cursor_offset: crate::primitives::Offset,
    response: &mut Response,
    state: &mut crate::state::VimState,
    tabstop: usize,
) {
    use crate::effects::Effect;

    let sticky_idx = response.effects.iter().rposition(|e| {
        matches!(
            e,
            Effect::SetStickyColumn {
                column: Some(col)
            } if !col.is_end_of_line()
        )
    });
    let Some(idx) = sticky_idx else {
        return;
    };

    let clamped = cursor_offset.get().min(text.len());
    let col = crate::commands::helpers::curswant_of(text, clamped, tabstop);
    let vcol = crate::primitives::VirtualColumn::new(col);

    if let Some(Effect::SetStickyColumn {
        column: Some(existing),
    }) = response.effects.get(idx)
    {
        if *existing == vcol {
            return;
        }
    }

    if let Some(slot) = response.effects.get_mut(idx) {
        *slot = Effect::SetStickyColumn { column: Some(vcol) };
        state.set_sticky_column(Some(vcol));
    }
}

// Multi-cursor yank/paste helpers are in the `multi_cursor_yank` sibling module.
use super::multi_cursor_yank::{
    apply_paste_zip, detect_paste_zip_content, extract_primary_register_info, extract_range_source,
    override_registers_with_multi_cursor_entries,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode::InsertMode;
    use crate::primitives::ReturnTo;

    // ═══════════════════════════════════════════════════════════════════════
    // return_to_for_insert_mode
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn return_to_insert() {
        let result = return_to_for_insert_mode(InsertMode::Insert);
        assert_eq!(result, ReturnTo::Insert);
    }

    #[test]
    fn return_to_replace() {
        let result = return_to_for_insert_mode(InsertMode::Replace);
        assert_eq!(result, ReturnTo::Replace);
    }

    #[test]
    fn return_to_virtual_replace() {
        let result = return_to_for_insert_mode(InsertMode::VirtualReplace);
        assert_eq!(result, ReturnTo::VirtualReplace);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ModeAction variant coverage — verify that the dispatch arms exist
    // by constructing each ModeAction variant that mode_dispatch handles.
    // These are structural tests ensuring the match arms are reachable.
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn mode_action_pipeline_is_constructible() {
        let result = crate::grammar::GrammarResult::Invalid;
        let action = ModeAction::Pipeline(result);
        assert!(matches!(action, ModeAction::Pipeline(_)));
    }

    #[test]
    fn mode_action_insert_exit_is_constructible() {
        let action = ModeAction::InsertExit;
        assert!(matches!(action, ModeAction::InsertExit));
    }

    #[test]
    fn mode_action_insert_command_is_constructible() {
        let command = Command::Insert(InsertKind::Char { char: 'a' });
        let action = ModeAction::InsertCommand {
            command,
            insert_mode: InsertMode::Insert,
        };
        assert!(matches!(action, ModeAction::InsertCommand { .. }));
    }

    #[test]
    fn mode_action_pending_is_constructible() {
        let action = ModeAction::Pending;
        assert!(matches!(action, ModeAction::Pending));
    }

    #[test]
    fn mode_action_ignored_is_constructible() {
        let action = ModeAction::Ignored;
        assert!(matches!(action, ModeAction::Ignored));
    }

    #[test]
    fn mode_action_select_replace_is_constructible() {
        let action = ModeAction::SelectReplace { char: 'x' };
        assert!(matches!(action, ModeAction::SelectReplace { char: 'x' }));
    }

    #[test]
    fn mode_action_select_delete_is_constructible() {
        let action = ModeAction::SelectDelete;
        assert!(matches!(action, ModeAction::SelectDelete));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Response construction helpers
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn response_pending_has_pending_kind() {
        let response = Response::pending_response();
        assert_eq!(response.kind, ResponseKind::Pending);
    }

    #[test]
    fn response_ignored_has_ignored_kind() {
        let response = Response::ignored();
        assert_eq!(response.kind, ResponseKind::Ignored);
    }

    #[test]
    fn response_consumed_empty_has_consumed_kind() {
        let response = Response::consumed_empty();
        assert_eq!(response.kind, ResponseKind::Consumed);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // CommandLineResult variant smoke tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn command_line_cancel_is_constructible() {
        let result = CommandLineResult::Cancel;
        assert!(matches!(result, CommandLineResult::Cancel));
    }

    #[test]
    fn command_line_commit_is_constructible() {
        let result = CommandLineResult::Commit;
        assert!(matches!(result, CommandLineResult::Commit));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PlannedAction variant coverage
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn planned_action_pending_is_constructible() {
        let action = PlannedAction::Pending;
        assert!(matches!(action, PlannedAction::Pending));
    }

    #[test]
    fn planned_action_ignored_is_constructible() {
        let action = PlannedAction::Ignored;
        assert!(matches!(action, PlannedAction::Ignored));
    }

    #[test]
    fn planned_action_mode_change_is_constructible() {
        let action = PlannedAction::ModeChange(Mode::Normal, None);
        assert!(matches!(
            action,
            PlannedAction::ModeChange(Mode::Normal, None)
        ));
    }

    #[test]
    fn planned_action_execute_is_constructible() {
        let cmd = Command::InsertExit;
        let action = PlannedAction::Execute(cmd);
        assert!(matches!(
            action,
            PlannedAction::Execute(Command::InsertExit)
        ));
    }
}
