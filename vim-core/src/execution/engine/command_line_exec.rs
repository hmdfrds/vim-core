//! Ex and search command execution for [`VimEngine`].
//!
//! Extracted from `command_line.rs` — contains the methods that run when
//! Enter is pressed in `:` or `/`/`?` command-line mode, plus their
//! helper functions.

use super::{CommandLineSession, Response, VimEngine};
use crate::commands::actions::effects as action_effects;
use crate::commands::ex::effects as ex_effects;
use crate::commands::helpers::line_of;
use crate::document::Document;
use crate::effects::{Effect, Effects};
use crate::execution::executor_ex;
use crate::execution::{ExecutionContext, InputContext, Validated};
use crate::grammar::Command;
use crate::primitives::Direction;
use crate::primitives::{Mode, SearchDirection};
use crate::state::VimState;

impl VimEngine {
    /// Execute a multi-cursor command from an ex command output, if present.
    ///
    /// Shared by `execute_ex_command_line` and `execute_cmd_mapping` to avoid
    /// duplicating the MC dispatch logic.
    fn dispatch_mc_from_ex(
        &mut self,
        mc_cmd: Option<&crate::state::MultiCursorCommand>,
        doc_text: &str,
    ) -> Effects {
        let Some(mc_cmd) = mc_cmd else {
            return Effects::new();
        };
        let search_pat = self.state.search().pattern().map(str::to_owned);
        let mc_ctx = crate::execution::multi_cursor_executor::MultiCursorContext {
            text: doc_text,
            search_pattern: search_pat.as_deref(),
            line_count: crate::commands::helpers::line_count(doc_text).max(1),
        };
        match crate::execution::multi_cursor_executor::execute_multi_cursor_command(
            &mut self.state,
            mc_cmd,
            &mc_ctx,
        ) {
            Ok(efx) => {
                let mut mc_efx = Effects::new();
                for effect in efx {
                    mc_efx.push(effect);
                }
                mc_efx
            }
            Err(err) => ex_effects::show_error(err),
        }
    }

    pub(super) fn execute_ex_command_line<D: Document>(
        &mut self,
        input: &str,
        session: CommandLineSession,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        let register_command = ex_register_command(input, session);
        let execute_command = ex_command_for_execution(
            &register_command,
            session,
            ctx.doc().text(),
            ctx.selection(),
            ctx.cursor_pos().line().get(),
        );

        let mut prefix_effects = visual_exit_effects(ctx.selection());
        prefix_effects.extend(ex_effects::register_ex_command(register_command.into()));
        // Clear any inccommand preview before executing the real command
        if self.options.inccommand_enabled() {
            prefix_effects.extend(Effects::new().clear_substitute_preview());
        }

        let doc_text = ctx.doc().text();
        let exec_ctx = ExecutionContext::new(ctx, &self.state, &self.options);
        match executor_ex::execute_ex_line(
            execute_command.as_str(),
            &exec_ctx,
            &mut self.host.sequencer,
        ) {
            Ok(mut output) => {
                crate::execution::effect_processor::sync_effects_with_text(
                    &mut self.state,
                    &mut self.parser,
                    prefix_effects.as_mut_slice(),
                    Some(doc_text),
                );
                crate::execution::effect_processor::sync_effects_with_text(
                    &mut self.state,
                    &mut self.parser,
                    output.effects.as_mut_slice(),
                    Some(doc_text),
                );
                self.state.set_mode(Mode::Normal);

                // Apply :set option mutations
                let set_effects = if output.set_assignments.is_empty() {
                    Effects::new()
                } else {
                    let effects = executor_ex::apply_set_assignments(
                        output.set_scope,
                        &mut self.options,
                        &mut self.buffer_overrides,
                        &mut self.window_overrides,
                        &output.set_assignments,
                    );
                    self.rebuild_resolved_cache();
                    self.rebuild_langmap_if_needed();
                    effects
                };

                // Apply :map/:noremap/:unmap mutations
                for change in &output.mapping_changes {
                    self.apply_mapping_change(change);
                }

                // Apply :abbreviate/:unabbreviate/:abclear mutations
                let mut abbrev_effects = Effects::new();
                for change in &output.abbrev_changes {
                    abbrev_effects.extend(self.apply_abbrev_change(change));
                }

                // Apply :sethandler mutations
                for change in &output.handler_changes {
                    self.apply_handler_change(change);
                }

                // Apply :let mapleader mutation
                if let Some(leader) = output.leader_change {
                    self.keymap
                        .set_leader(crate::keymap::KeyEvent::char(leader));
                }

                // Apply :messages clear
                if output.clear_message_history {
                    self.state.message_history_mut().clear();
                }
                // Apply :clearjumps
                if output.clear_jumplist {
                    self.state.jump_list_mut().clear();
                }

                // Apply :undojoin — merge the next undo group into the previous one
                if output.undo_join_pending {
                    self.state.undo_tree_mut().begin_merge();
                }

                // Store last ex command for dot-repeat
                if output.last_ex_for_dot.is_some() {
                    self.state.set_last_ex_for_dot(output.last_ex_for_dot);
                }

                // Apply undo tree navigation
                let mut undo_effects = match &output.undo_navigation {
                    Some(nav) => self.apply_undo_navigation(nav),
                    None => Effects::new(),
                };
                // Sync undo effects so that the engine's undo tree state is
                // updated (Undo/Redo effects navigate the tree in the effect
                // processor). Without this, `:later` after `:earlier` would
                // not find any redo target because the tree never moved.
                crate::execution::effect_processor::sync_effects(
                    &mut self.state,
                    &mut self.parser,
                    undo_effects.as_mut_slice(),
                );

                let mc_effects =
                    self.dispatch_mc_from_ex(output.multi_cursor_command.as_ref(), doc_text);

                let mut effects = prefix_effects;
                effects.extend(output.effects);
                effects.extend(set_effects);
                effects.extend(abbrev_effects);
                effects.extend(undo_effects);
                effects.extend(mc_effects);
                effects.extend(action_effects::switch_mode(Mode::Normal));
                let mut response = Response::with_effects(effects);
                response.host_requests = output.host_requests.into_vec();
                response
            }
            Err(err) => {
                crate::execution::effect_processor::sync_effects_with_text(
                    &mut self.state,
                    &mut self.parser,
                    prefix_effects.as_mut_slice(),
                    Some(doc_text),
                );
                self.state.set_mode(Mode::Normal);
                let mut effects = prefix_effects;
                effects.extend(ex_effects::show_error(err));
                effects.extend(action_effects::switch_mode(Mode::Normal));
                Response::with_effects(effects)
            }
        }
    }

    /// Execute an ex command from a `<Cmd>…<CR>` mapping RHS.
    ///
    /// Unlike `execute_ex_command_line`, this does **not** change mode —
    /// the current mode is preserved throughout. There is no command-line
    /// history registration and no visual-exit prefix effects.
    pub(super) fn execute_cmd_mapping<D: Document>(
        &mut self,
        cmd: &str,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        let doc_text = ctx.doc().text();
        let exec_ctx = ExecutionContext::new(ctx, &self.state, &self.options);
        match executor_ex::execute_ex_line(cmd, &exec_ctx, &mut self.host.sequencer) {
            Ok(mut output) => {
                crate::execution::effect_processor::sync_effects(
                    &mut self.state,
                    &mut self.parser,
                    output.effects.as_mut_slice(),
                );

                // Apply :set option mutations
                let set_effects = if output.set_assignments.is_empty() {
                    Effects::new()
                } else {
                    let effects = executor_ex::apply_set_assignments(
                        output.set_scope,
                        &mut self.options,
                        &mut self.buffer_overrides,
                        &mut self.window_overrides,
                        &output.set_assignments,
                    );
                    self.rebuild_resolved_cache();
                    self.rebuild_langmap_if_needed();
                    effects
                };

                for change in &output.mapping_changes {
                    self.apply_mapping_change(change);
                }
                let mut abbrev_effects = Effects::new();
                for change in &output.abbrev_changes {
                    abbrev_effects.extend(self.apply_abbrev_change(change));
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
                if output.clear_jumplist {
                    self.state.jump_list_mut().clear();
                }

                // Apply :undojoin — merge the next undo group into the previous one
                if output.undo_join_pending {
                    self.state.undo_tree_mut().begin_merge();
                }

                // Store last ex command for dot-repeat
                if output.last_ex_for_dot.is_some() {
                    self.state.set_last_ex_for_dot(output.last_ex_for_dot);
                }

                let mut undo_effects = match &output.undo_navigation {
                    Some(nav) => self.apply_undo_navigation(nav),
                    None => Effects::new(),
                };
                crate::execution::effect_processor::sync_effects(
                    &mut self.state,
                    &mut self.parser,
                    undo_effects.as_mut_slice(),
                );

                let mc_effects =
                    self.dispatch_mc_from_ex(output.multi_cursor_command.as_ref(), doc_text);

                let mut effects = output.effects;
                effects.extend(set_effects);
                effects.extend(abbrev_effects);
                effects.extend(undo_effects);
                effects.extend(mc_effects);
                let mut response = Response::with_effects(effects);
                response.host_requests = output.host_requests.into_vec();
                response
            }
            Err(err) => Response::with_effects(ex_effects::show_error(err)),
        }
    }

    pub(super) fn execute_search_command_line<D: Document>(
        &mut self,
        input: &str,
        session: CommandLineSession,
        ctx: InputContext<'_, D, Validated>,
        direction: Direction,
    ) -> Response {
        // ── Chained search support ──────────────────────────────────
        // Vim supports semicolon-chained searches: `/foo/;?bar` means
        // "search forward for foo, then from that position search backward
        // for bar".  Split on unescaped `;/` and `;?` boundaries.
        let chain = crate::commands::helpers::split_search_chain(input);
        if chain.len() > 1 {
            return self.execute_chained_search(&chain, session, ctx, direction);
        }

        // ── Single-segment search (common path) ─────────────────────
        // Parse search offset from input (e.g. "/pattern/e+3").
        let (pattern_part, offset) = crate::commands::helpers::parse_search_input(input);

        let Some(pattern) = resolve_search_pattern(pattern_part, self.state.search().pattern())
        else {
            return missing_pattern_response(&mut self.state);
        };

        self.store_search_pattern(&pattern, direction, offset);
        let mut prefix_effects = ex_effects::set_search_pattern(&pattern, direction);
        let cursor_offset = ctx.cursor_offset_raw();

        // Restore the mode the user was in before entering the command line, so
        // that execute_search_command (and the dispatch chain it triggers) sees
        // the correct visual mode and derives the right SelectionShape. Without
        // this, the mode is still CommandLine and visual_type() returns None →
        // SelectionShape::Char even when the user entered from Visual(Line).
        self.state.set_mode(session.entered_from_mode);

        let mut command_response =
            self.execute_search_command(session.intent, direction, ctx, session.count);

        if !command_succeeded(&command_response, session.intent.is_some()) {
            self.state.set_mode(Mode::Normal);
            prefix_effects.extend(ex_effects::search_not_found(&pattern));
            crate::execution::effect_processor::sync_effects(
                &mut self.state,
                &mut self.parser,
                prefix_effects.as_mut_slice(),
            );
            return Response::with_effects(prefix_effects);
        }

        if session.intent.is_none() {
            prefix_effects.extend(ex_effects::search_jump_marks(cursor_offset));
        }
        crate::execution::effect_processor::sync_effects(
            &mut self.state,
            &mut self.parser,
            prefix_effects.as_mut_slice(),
        );
        self.finish_search_mode(session, &mut command_response);
        prefix_effects.extend(command_response.effects);
        Response::with_effects(prefix_effects)
    }

    /// Execute a chained search (`/foo/;?bar/;/baz`).
    ///
    /// Each segment is executed sequentially.  The cursor position from each
    /// segment becomes the starting position for the next.  If any segment
    /// fails to find a match, the entire chain fails and the cursor stays put.
    ///
    /// Vim semantics (`:help //;`):
    /// - The remembered direction for `n`/`N` is from the **first** segment.
    /// - The remembered pattern for `n`/`N` is from the **last** segment.
    /// - If any segment fails, the cursor doesn't move at all.
    fn execute_chained_search<D: Document>(
        &mut self,
        chain: &[crate::commands::helpers::SearchChainSegment<'_>],
        session: CommandLineSession,
        ctx: InputContext<'_, D, Validated>,
        first_direction: Direction,
    ) -> Response {
        let cursor_offset = ctx.cursor_offset_raw();
        let mut current_cursor = cursor_offset;
        let is_operator = session.intent.is_some();

        // Restore mode before executing (same as single-segment path).
        self.state.set_mode(session.entered_from_mode);

        // The remembered direction for n/N is the first segment's direction.
        let remembered_direction = first_direction;

        // Extract shared context components before the loop so we can rebuild
        // InputContext for each segment without fighting move semantics.
        let doc = ctx.doc();
        let selection = ctx.selection();
        let viewport = ctx.viewport();
        let providers = *ctx.providers();
        let buffer_id = ctx.buffer_id();
        // Consume ctx — we rebuild from parts for each segment.
        drop(ctx);

        let chain_len = chain.len();

        for (seg_idx, segment) in chain.iter().enumerate() {
            let seg_direction = segment.direction_override.unwrap_or(first_direction);

            let (pattern_part, offset) =
                crate::commands::helpers::parse_search_input(segment.input);

            let Some(pattern) = resolve_search_pattern(pattern_part, self.state.search().pattern())
            else {
                return missing_pattern_response(&mut self.state);
            };

            // Store pattern with this segment's direction.
            self.store_search_pattern(&pattern, seg_direction, offset);

            let is_last = seg_idx == chain_len - 1;

            // Rebuild context with the current cursor position.
            let mut seg_ctx = InputContext::new(doc, current_cursor)
                .validate_clamped()
                .with_providers(providers);
            if let Some(sel) = selection {
                seg_ctx = seg_ctx.with_selection(sel);
            }
            if let Some(vp) = viewport {
                seg_ctx = seg_ctx.with_viewport(vp);
            }
            if let Some(bid) = buffer_id {
                seg_ctx = seg_ctx.with_buffer_id(bid);
            }

            // For the last segment, preserve operator intent so that the full
            // pipeline (delete, yank, etc.) fires.  Intermediate segments are
            // pure motion: no operator, count=1.
            let seg_response = if is_last {
                self.execute_search_command(session.intent, seg_direction, seg_ctx, session.count)
            } else {
                self.execute_search_command(None, seg_direction, seg_ctx, 1)
            };

            // Check if this segment found a match.
            if !command_succeeded(&seg_response, is_operator && is_last) {
                // Segment failed — abort the entire chain.
                self.state.set_mode(Mode::Normal);
                let mut fail_effects =
                    ex_effects::set_search_pattern(&pattern, remembered_direction);
                fail_effects.extend(ex_effects::search_not_found(&pattern));
                crate::execution::effect_processor::sync_effects(
                    &mut self.state,
                    &mut self.parser,
                    fail_effects.as_mut_slice(),
                );
                return Response::with_effects(fail_effects);
            }

            // Extract cursor position from the response for the next segment.
            if let Some(new_cursor) = extract_cursor_from_effects(&seg_response.effects) {
                current_cursor = new_cursor;
            }

            if is_last {
                // Restore the first segment's direction as the remembered
                // direction for n/N.  The last segment's pattern is already
                // stored by store_search_pattern above.
                if let Some(pat) = self.state.search().pattern().map(ToOwned::to_owned) {
                    let off = self.state.search().offset();
                    self.store_search_pattern(&pat, remembered_direction, off);
                }

                // Build the final response with search-pattern effects,
                // jump marks, and the last segment's command effects.
                let last_pattern = self.state.search().pattern().unwrap_or("").to_owned();
                let mut prefix_effects =
                    ex_effects::set_search_pattern(&last_pattern, remembered_direction);

                if session.intent.is_none() {
                    prefix_effects.extend(ex_effects::search_jump_marks(cursor_offset));
                }
                crate::execution::effect_processor::sync_effects(
                    &mut self.state,
                    &mut self.parser,
                    prefix_effects.as_mut_slice(),
                );

                let mut response = Response::with_effects(prefix_effects);
                // Append the last segment's effects (SetCursor, mode, etc.)
                response.effects.extend(seg_response.effects);
                self.finish_search_mode(session, &mut response);
                return response;
            }

            // Intermediate segments: discard effects (we only need the cursor
            // position, already extracted above).
        }

        // Should never reach here (the loop always returns on the last segment).
        debug_assert!(false, "chained search loop fell through");
        Response::ignored()
    }

    fn store_search_pattern(
        &mut self,
        pattern: &str,
        direction: Direction,
        offset: crate::state::SearchOffset,
    ) {
        let search_direction = if direction.is_forward() {
            SearchDirection::Forward
        } else {
            SearchDirection::Backward
        };
        self.state.search_mut().set_pattern_with_offset(
            pattern.to_owned(),
            search_direction,
            offset,
        );
    }

    fn finish_search_mode(&mut self, session: CommandLineSession, response: &mut Response) {
        // Ctrl-O one-shot: return to original insert/replace mode instead of
        // Normal. Handles `i<C-o>/pattern<CR>` returning to Insert.
        let return_to = self.state.take_return_to();
        if let Some(return_mode) = return_to.target_mode() {
            self.state.set_mode(return_mode);
            response
                .effects
                .extend(action_effects::switch_mode(return_mode).into_vec());
            return;
        }

        let target_mode = default_mode_after_search(session);
        if !command_sets_mode(response) {
            self.state.set_mode(target_mode);
            response
                .effects
                .extend(action_effects::switch_mode(target_mode).into_vec());
        }
    }

    fn execute_search_command<D: Document>(
        &mut self,
        intent: Option<crate::grammar::OperatorSearchIntent>,
        _direction: Direction,
        ctx: InputContext<'_, D, Validated>,
        search_count: u32,
    ) -> Response {
        use crate::grammar::types::Motion;
        use std::num::NonZeroU32;
        // Always use SearchNext: the direction is already stored in engine
        // state by store_search_pattern(), so SearchNext follows it directly.
        // Using SearchPrev here would *reverse* the stored direction, making
        // ?pattern search forward instead of backward.
        let motion = Motion::SearchNext;
        let search_count = NonZeroU32::new(search_count).unwrap_or(NonZeroU32::MIN);

        let command = if let Some(operator) = intent {
            Command::OperatorMotion {
                count: operator.count,
                register: operator.register,
                operator: operator.operator,
                motion,
                force_type: None,
            }
        } else {
            Command::Motion {
                count: search_count,
                motion,
                explicit_count: false,
            }
        };
        match self.execute_effect_plan(command, self.is_repeating, ctx) {
            Ok(response) => response,
            Err(ref err) => self.handle_pipeline_error(err),
        }
    }

    /// Apply a single mapping change from `:map`/`:noremap`/`:unmap`.
    ///
    /// Parses the raw notation strings with access to `self.keymap` so that
    /// `<Action>(name)` and `<Plug>(name)` can be resolved against the
    /// keymap's name registries.
    pub(super) fn apply_mapping_change(&mut self, change: &executor_ex::MappingChange) {
        match change {
            executor_ex::MappingChange::Define {
                modes,
                lhs_raw,
                rhs_raw,
                kind,
                flags,
            } => {
                let lhs_seq = executor_ex::parse_key_notation_sequence_with_keymap(
                    lhs_raw,
                    Some(&mut self.keymap),
                );
                if flags.expr {
                    let expr_text = Some(rhs_raw.clone());
                    for &mode in modes {
                        self.map_with_expr(
                            mode,
                            lhs_seq.as_slice(),
                            Vec::new(),
                            *kind,
                            *flags,
                            expr_text.clone(),
                        );
                    }
                } else {
                    let rhs_seq = executor_ex::parse_key_notation_sequence_with_keymap(
                        rhs_raw,
                        Some(&mut self.keymap),
                    );
                    for &mode in modes {
                        self.map(mode, lhs_seq.as_slice(), rhs_seq.clone(), *kind, *flags);
                    }
                }
            }
            executor_ex::MappingChange::Remove { modes, lhs_raw } => {
                let lhs_seq = executor_ex::parse_key_notation_sequence_with_keymap(
                    lhs_raw,
                    Some(&mut self.keymap),
                );
                for &mode in modes {
                    self.unmap(mode, lhs_seq.as_slice());
                }
            }
            executor_ex::MappingChange::ClearAll { modes } => {
                for &mode in modes {
                    self.clear_mappings_for(mode);
                }
            }
        }
    }

    /// Apply a single abbreviation change from `:abbreviate`/`:unabbreviate`/`:abclear`.
    ///
    /// Returns effects (e.g. info messages for listing) that should be merged
    /// into the response.
    pub(super) fn apply_abbrev_change(&mut self, change: &executor_ex::AbbrevChange) -> Effects {
        use crate::commands::ex::abbreviation;

        match change {
            executor_ex::AbbrevChange::Define {
                trigger,
                replacement,
                mode,
                noremap,
            } => {
                let is_keyword = |c: char| crate::primitives::is_word_char(c);
                abbreviation::abbreviate(
                    trigger.as_deref(),
                    replacement.as_deref(),
                    *mode,
                    *noremap,
                    &mut self.abbrev_table,
                    &is_keyword,
                )
                .unwrap_or_default()
            }
            executor_ex::AbbrevChange::Remove { trigger, mode } => {
                abbreviation::unabbreviate(trigger, *mode, &mut self.abbrev_table)
                    .unwrap_or_default()
            }
            executor_ex::AbbrevChange::Clear { mode } => {
                abbreviation::ab_clear(*mode, &mut self.abbrev_table)
            }
        }
    }

    /// Apply a single handler change from `:sethandler`.
    pub(super) fn apply_handler_change(&mut self, change: &executor_ex::HandlerChange) {
        if let Some(key) = change.key {
            for &mode in &change.modes {
                self.handler_map.set(key, mode, change.handler);
            }
            self.key_interest_dirty = true;
        }
        // No key specified — global default. The HandlerMap defaults to Vim
        // for unset keys. A future enhancement could add a default handler
        // field for the no-key case.
    }
}

fn resolve_search_pattern(input: &str, previous: Option<&str>) -> Option<String> {
    if !input.is_empty() {
        return Some(input.to_owned());
    }
    previous.map(ToOwned::to_owned)
}

fn command_sets_mode(response: &Response) -> bool {
    response
        .effects
        .iter()
        .any(|effect| matches!(effect, Effect::SetMode { .. } | Effect::BeginInsert { .. }))
}

fn command_succeeded(response: &Response, operator_search: bool) -> bool {
    if operator_search {
        return !response.effects.is_empty();
    }
    response
        .effects
        .iter()
        .any(|effect| matches!(effect, Effect::SetCursor { .. }))
}

/// Extract the final cursor position from a response's effects.
///
/// Scans effects for the last `SetCursor` or `SetSelection` and returns
/// the cursor byte offset.  Used by chained searches to determine the
/// starting position for the next segment.
fn extract_cursor_from_effects(effects: &[Effect]) -> Option<usize> {
    let mut cursor = None;
    for effect in effects {
        match effect {
            Effect::SetCursor { offset } => {
                cursor = Some(offset.get());
            }
            Effect::SetSelection { head, .. } => {
                cursor = Some(head.get());
            }
            _ => {}
        }
    }
    cursor
}

const fn default_mode_after_search(session: CommandLineSession) -> Mode {
    if session.intent.is_none() && session.entered_from_mode.is_visual() {
        session.entered_from_mode
    } else {
        Mode::Normal
    }
}

fn missing_pattern_response(state: &mut VimState) -> Response {
    state.set_mode(Mode::Normal);
    Response::with_effects(ex_effects::missing_pattern())
}

fn ex_register_command(input: &str, session: CommandLineSession) -> String {
    if session.entered_from_mode.is_visual() {
        let command = input.trim_start();
        if command.starts_with("'<,'>") {
            command.to_owned()
        } else {
            format!("'<,'>{command}")
        }
    } else {
        // Store the raw input (including leading whitespace) in the `:` register.
        // Vim preserves leading whitespace in the command history/register.
        input.to_owned()
    }
}

fn ex_command_for_execution(
    command: &str,
    session: CommandLineSession,
    text: &str,
    selection: Option<crate::primitives::SelectionRange>,
    cursor_line: usize,
) -> String {
    if !session.entered_from_mode.is_visual() || !command.starts_with("'<,'>") {
        return command.to_owned();
    }
    let (start, end) = selection_line_range(text, selection, cursor_line);
    let rest = &command["'<,'>".len()..];
    format!("{start},{end}{rest}")
}

fn selection_line_range(
    text: &str,
    selection: Option<crate::primitives::SelectionRange>,
    cursor_line: usize,
) -> (usize, usize) {
    let Some(selection) = selection else {
        let line = cursor_line + 1;
        return (line, line);
    };
    let start = line_of(text, selection.start().get()) + 1;
    let end = line_of(text, selection.end().get()) + 1;
    (start.min(end), start.max(end))
}

fn visual_exit_effects(selection: Option<crate::primitives::SelectionRange>) -> Effects {
    match selection {
        Some(sel) => ex_effects::visual_exit_marks(&sel),
        None => Effects::new(),
    }
}
