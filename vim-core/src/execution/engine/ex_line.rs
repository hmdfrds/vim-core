//! Running an Ex command line for [`VimEngine`]: each `|` command runs on
//! the text, cursor and marks the command before it left, as Vim's
//! `do_cmdline()` runs them, and `:g` runs its commands one marked line at
//! a time (ex_cmds.c `ex_global()`, `global_exe()`).
//!
//! The host applies the effects in order, so each command's effects are
//! computed on an [`OwnedDocument`] that has the effects before them
//! applied, and the engine's state is synced after each command.

use compact_str::CompactString;
use smallvec::SmallVec;

use super::shadow_document::OwnedDocument;
use super::VimEngine;
use crate::commands::ex::global::{LineMarks, MAX_GLOBAL_RECURSION_DEPTH};
use crate::document::Document;
use crate::effects::{Effect, Effects, InfoMessage, LineModCounts};
use crate::errors::VimError;
use crate::execution::executor_ex::{self, ExExecutionOutput, GlobalCommand};
use crate::execution::{ExecutionContext, HostRequest, InputContext, Validated};
use crate::grammar::split_ex_pipeline;
use crate::grammar::types::{ExCommand, ExRange, ModifierFlags};
use crate::primitives::{Direction, Mode, Position, SelectionRange, UndoCursorStrategy};

/// Vim's default 'report': a `:g` reports more changed lines than this.
const REPORT: usize = 2;

/// Whether an error ends a command line (ex_docmd.c `do_cmdline()`): it
/// does for a typed line, `@:` and a line of `:@r`, not for a `<Cmd>`
/// mapping or the commands `:g` runs on a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OnError {
    /// The commands after the error do not run.
    Stop,
    /// They run.
    Continue,
}

/// What a command line gave.
pub(super) struct ExLineOutput {
    /// Its effects, synced to the engine's state.
    pub effects: Effects,
    /// The host requests of its commands.
    pub host_requests: SmallVec<[HostRequest; 4]>,
}

/// How a line's undo groups are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UndoGroups {
    /// Each command keeps its own.
    Own,
    /// They are folded into one, so the line is one undo step.
    Merged,
    /// Folded, and the one group is open.
    Open,
}

/// The text, cursor and output of a command line as it runs.
struct Run<'d, D: Document> {
    /// The line's input, with the host's text.
    input: InputContext<'d, D, Validated>,
    /// The text once a command changed it.
    shadow: Option<OwnedDocument>,
    cursor: usize,
    /// Dropped once the text changes.
    selection: Option<SelectionRange>,
    resolved_options: bool,
    /// Set after each command's sync, before the engine applies its output.
    mode_after: Option<Mode>,
    effects: Effects,
    host_requests: SmallVec<[HostRequest; 4]>,
    undo: UndoGroups,
    /// A command changed the text.
    dot: bool,
    /// The text is no longer known, as after `:undo`.
    lost: bool,
    /// The modifiers of the commands this line runs for, such as the
    /// `:silent` before a `:g`.
    modifiers: ModifierFlags,
    /// Inside how many `:g`.
    global_depth: u8,
    /// The lines the outer `:g` has yet to run on.
    marks: Option<LineMarks>,
    /// Substitutions and the lines they changed inside `:g`.
    substitutions: (usize, usize),
}

impl<'d, D: Document> Run<'d, D> {
    const fn host(&self) -> &'d D {
        self.input.doc()
    }

    fn text(&self) -> &str {
        self.shadow
            .as_ref()
            .map_or_else(|| self.host().text(), OwnedDocument::text)
    }

    fn line_start(&self, line: usize) -> Option<usize> {
        match &self.shadow {
            Some(doc) => doc.line_index().line_start(line),
            None => self
                .host()
                .pos_to_offset(Position::from_raw(line, 0))
                .map(crate::primitives::Offset::get),
        }
    }

    fn cursor_line(&self) -> usize {
        let text = self.text();
        let cursor = self.cursor.min(text.len());
        crate::commands::helpers::line_count(text.get(..cursor).unwrap_or_default())
            .saturating_sub(1)
    }

    /// Follow `effects` as the host applies them: the text, the `:g` marks
    /// and the cursor.
    fn follow(&mut self, effects: &[Effect]) {
        for effect in effects {
            let (start, end, new) = match effect {
                Effect::Insert { offset, text } => (offset.get(), offset.get(), text.as_str()),
                Effect::Delete { range } => (range.start().get(), range.end().get(), ""),
                Effect::Replace { range, text } => {
                    (range.start().get(), range.end().get(), text.as_str())
                }
                Effect::SetCursor { offset } => {
                    self.cursor = offset.get();
                    continue;
                }
                Effect::Undo { .. } | Effect::Redo { .. } | Effect::UndoLine { .. } => {
                    self.lost = true;
                    continue;
                }
                _ => continue,
            };
            let host = self.host();
            let doc = self
                .shadow
                .get_or_insert_with(|| OwnedDocument::new(host.text()));
            if let Some(marks) = &mut self.marks {
                marks.edit(doc.text(), start, end, new);
            }
            match effect {
                Effect::Insert { .. } => doc.apply_insert(start, new),
                Effect::Delete { .. } => doc.apply_delete(start, end),
                _ => doc.apply_replace(start, end, new),
            }
            self.selection = None;
        }
    }

    /// Take the undo groups out of `effects` and open the line's one group
    /// where the first was, or before the first change. Whether `effects`
    /// change the text in a group a command before opened.
    fn merge_undo_groups(&mut self, effects: &mut Effects) -> bool {
        if self.undo == UndoGroups::Own {
            return false;
        }
        let was_open = self.undo == UndoGroups::Open;
        let mut changes = false;
        let mut kept = Effects::new();
        for effect in effects.drain() {
            match effect {
                Effect::BeginUndoGroup { cursor_strategy } => {
                    self.open_group(&mut kept, cursor_strategy);
                }
                Effect::EndUndoGroup { .. } => {}
                effect => {
                    if matches!(
                        effect,
                        Effect::Insert { .. } | Effect::Delete { .. } | Effect::Replace { .. }
                    ) {
                        changes = true;
                        self.open_group(&mut kept, UndoCursorStrategy::FirstEdit);
                    }
                    kept.push(effect);
                }
            }
        }
        *effects = kept;
        was_open && changes
    }

    fn open_group(&mut self, effects: &mut Effects, cursor_strategy: UndoCursorStrategy) {
        if self.undo != UndoGroups::Open {
            self.undo = UndoGroups::Open;
            effects.push(Effect::BeginUndoGroup { cursor_strategy });
        }
    }
}

/// The counts of a `:s` report, "3 substitutions on 2 lines".
fn substitution_report(effects: &Effects) -> Option<(usize, usize)> {
    effects.iter().find_map(|effect| {
        let Effect::ShowInfo {
            info: InfoMessage::Text(text),
        } = effect
        else {
            return None;
        };
        let (count, rest) = text.split_once(' ')?;
        let count = count.parse().ok()?;
        let rest = rest
            .strip_prefix("substitutions")
            .or_else(|| rest.strip_prefix("substitution"))?;
        let lines = rest
            .strip_prefix(" on ")
            .and_then(|rest| rest.split_once(' '))
            .and_then(|(lines, _)| lines.parse().ok())
            .unwrap_or(1);
        Some((count, lines))
    })
}

/// Whether `piece` is `:undo`, `:redo`, `:earlier` or `:later`.
fn moves_in_undo(piece: &str) -> bool {
    matches!(
        crate::grammar::ex_parser::parse_ex_piece(piece).1,
        Ok(ExCommand::Earlier { .. }
            | ExCommand::Later { .. }
            | ExCommand::Redo
            | ExCommand::UndoSequence { .. })
    )
}

impl VimEngine {
    /// Run the Ex command line `line` from `ctx`.
    ///
    /// `resolved_options` gives the commands the options with their local
    /// values; `mode_after` is set once a command's effects are synced.
    pub(super) fn run_ex_line<D: Document>(
        &mut self,
        line: &str,
        ctx: InputContext<'_, D, Validated>,
        on_error: OnError,
        resolved_options: bool,
        mode_after: Option<Mode>,
    ) -> ExLineOutput {
        let pieces = split_ex_pipeline(line);
        let mut run = Run {
            shadow: None,
            cursor: ctx.cursor_offset_raw(),
            selection: ctx.selection(),
            resolved_options,
            mode_after,
            effects: Effects::new(),
            host_requests: SmallVec::new(),
            undo: if pieces.len() > 1 {
                UndoGroups::Merged
            } else {
                UndoGroups::Own
            },
            dot: false,
            lost: false,
            modifiers: ModifierFlags::empty(),
            global_depth: 0,
            marks: None,
            substitutions: (0, 0),
            input: ctx,
        };
        for piece in &pieces {
            if (self.run_piece(piece, &mut run) && on_error == OnError::Stop) || run.lost {
                break;
            }
        }
        if run.undo == UndoGroups::Open {
            let mut end = Effects::new();
            end.push(Effect::EndUndoGroup { node_id: None });
            self.sync_line_effects(&mut end, &run);
            run.effects.extend(end);
        }
        if run.dot {
            self.state
                .set_last_ex_for_dot(Some(CompactString::from(line)));
        }
        ExLineOutput {
            effects: run.effects,
            host_requests: run.host_requests,
        }
    }

    /// Run the commands of `line`, as `:g` runs them on a line or `:@r`
    /// runs a register line. Whether one gave an error.
    fn run_nested_line<D: Document>(
        &mut self,
        line: &str,
        on_error: OnError,
        run: &mut Run<'_, D>,
    ) -> bool {
        if run.undo == UndoGroups::Own {
            run.undo = UndoGroups::Merged;
        }
        let mut error = false;
        for piece in &split_ex_pipeline(line) {
            error |= self.run_piece(piece, run);
            if (error && on_error == OnError::Stop) || run.lost {
                break;
            }
        }
        error
    }

    /// Run one command of a line. Whether it gave an error.
    fn run_piece<D: Document>(&mut self, piece: &str, run: &mut Run<'_, D>) -> bool {
        if run.undo == UndoGroups::Open && moves_in_undo(piece) {
            // u_undo() and undo_time() sync undo first, so the line so far
            // is the change they undo.
            run.undo = UndoGroups::Merged;
            let mut end = Effects::new();
            end.push(Effect::EndUndoGroup { node_id: None });
            self.sync_line_effects(&mut end, run);
            run.effects.extend(end);
        }
        let mut output = match &run.shadow {
            Some(doc) => self.execute_piece_on(piece, doc, run),
            None => self.execute_piece_on(piece, run.host(), run),
        };
        crate::execution::modifier_filter::apply_modifiers(&mut output.effects, run.modifiers);
        if run.global_depth > 0 && output.last_ex_for_dot.is_some() {
            Self::quiet_in_global(&mut output.effects, run);
        }
        if output.last_ex_for_dot.take().is_some() {
            run.dot = true;
        }
        let global = output.global.take();
        let ex_lines = std::mem::take(&mut output.ex_lines);
        let modifiers = output.modifiers | run.modifiers;
        let mut error = self.finish_piece(output, run);
        if let Some(global) = global {
            let outer = std::mem::replace(&mut run.modifiers, modifiers);
            error |= self.run_global(&global, run);
            run.modifiers = outer;
        }
        for line in &ex_lines {
            // flush_buffers() after an error drops the register's other lines.
            if error || run.lost {
                break;
            }
            error |= self.run_nested_line(line, OnError::Stop, run);
        }
        error
    }

    /// Execute `piece` on `doc` at the cursor of `run`.
    fn execute_piece_on<D: Document, E: Document>(
        &mut self,
        piece: &str,
        doc: &E,
        run: &Run<'_, D>,
    ) -> ExExecutionOutput {
        let mut input = InputContext::new(doc, run.cursor)
            .validate_clamped()
            .with_providers(*run.input.providers());
        if let Some(selection) = run.selection {
            input = input.with_selection(selection);
        }
        if let Some(viewport) = run.input.viewport() {
            input = input.with_viewport(viewport);
        }
        if let Some(id) = run.input.buffer_id() {
            input = input.with_buffer_id(id);
        }
        let options = if run.resolved_options {
            &self.resolved_options
        } else {
            &self.options
        };
        let ctx = ExecutionContext::new(input, &self.state, options);
        executor_ex::execute_ex_piece(piece, &ctx, &mut self.host.sequencer)
    }

    /// Inside `:g`, a command that changes the text reports nothing and
    /// `:s` finding no match is no error; the counts of `:s` go to the
    /// report after the `:g` (ex_cmds.c `ex_substitute()`, misc1.c
    /// `msgmore()` with `global_busy`).
    fn quiet_in_global<D: Document>(effects: &mut Effects, run: &mut Run<'_, D>) {
        if let Some((count, lines)) = substitution_report(effects) {
            run.substitutions.0 += count;
            run.substitutions.1 += lines;
        }
        let kept: Vec<Effect> = effects
            .drain()
            .filter(|effect| {
                !matches!(
                    effect,
                    Effect::ShowInfo { .. }
                        | Effect::ShowError {
                            error: VimError::PatternNotFound(_),
                            ..
                        }
                )
            })
            .collect();
        effects.extend(kept);
    }

    /// Sync `output` to the engine, apply what it asks of the engine, follow
    /// its effects and add them to the line. Whether it gave an error.
    fn finish_piece<D: Document>(
        &mut self,
        mut output: ExExecutionOutput,
        run: &mut Run<'_, D>,
    ) -> bool {
        if run.merge_undo_groups(&mut output.effects) {
            // Each command sets '[ and '] and moves the changelist entry.
            self.state.set_change_start_mark_set(false);
            self.state.set_changelist_follow_next(true);
        }
        self.sync_line_effects(&mut output.effects, run);
        self.state.set_changelist_follow_next(false);
        if let Some(mode) = run.mode_after {
            self.state.set_mode(mode);
        }
        let applied = self.apply_ex_output(&mut output, run.text());
        let error = output
            .effects
            .iter()
            .chain(applied.iter())
            .any(|effect| matches!(effect, Effect::ShowError { .. }));
        run.follow(output.effects.as_slice());
        run.follow(applied.as_slice());
        run.effects.extend(output.effects);
        run.effects.extend(applied);
        run.host_requests.extend(output.host_requests);
        error
    }

    fn sync_line_effects<D: Document>(&mut self, effects: &mut Effects, run: &Run<'_, D>) {
        crate::execution::effect_processor::sync_effects_with_text(
            &mut self.state,
            &mut self.parser,
            effects.as_mut_slice(),
            Some(run.text()),
        );
    }

    /// Add `effects` to the line as a command's own.
    fn add_line_effects<D: Document>(&mut self, effects: Effects, run: &mut Run<'_, D>) -> bool {
        self.finish_piece(
            ExExecutionOutput {
                effects,
                ..ExExecutionOutput::default()
            },
            run,
        )
    }

    /// Run `:g` or `:v`: mark the lines first, then run its commands on each
    /// marked line that is left, on the text the line before left
    /// (ex_cmds.c `ex_global()`, `global_exe()`). Whether it gave an error.
    fn run_global<D: Document>(&mut self, global: &GlobalCommand, run: &mut Run<'_, D>) -> bool {
        let pattern_effects =
            Effects::new().set_search_pattern(global.pattern.as_str(), Direction::Forward);
        let error = self.add_line_effects(pattern_effects, run);
        if run.global_depth > 0 {
            return error || self.run_global_on_cursor_line(global, run);
        }
        let lines = match &run.shadow {
            Some(doc) => self.global_lines_on(global, doc, run),
            None => self.global_lines_on(global, run.host(), run),
        };
        let lines = match lines {
            Ok(lines) => lines,
            Err(err) => return self.add_line_effects(Effects::new().show_error(err), run),
        };
        if lines.is_empty() {
            // A message, not an error, outside Vim9 script (ex_global()).
            let message = if global.invert {
                format!("Pattern found in every line: {}", global.pattern)
            } else {
                format!("Pattern not found: {}", global.pattern)
            };
            return self.add_line_effects(Effects::new().show_message(message), run);
        }
        let line_count = run.text().split('\n').count();
        run.marks = Some(LineMarks::new(line_count, &lines));
        run.global_depth = 1;
        run.substitutions = (0, 0);
        let mut error = false;
        while let Some(line) = run.marks.as_mut().and_then(LineMarks::take_first) {
            // global_exe_one(): the cursor goes to the line's start.
            run.cursor = run.line_start(line).unwrap_or(run.cursor);
            // An error ends the loop once the line's commands ran.
            if self.run_nested_line(&global.commands, OnError::Continue, run) {
                error = true;
                break;
            }
            if run.lost {
                break;
            }
        }
        run.marks = None;
        run.global_depth = 0;
        if run.lost {
            return error;
        }
        let report = self.global_report(run, line_count);
        error | self.add_line_effects(report, run)
    }

    /// A `:g` inside `:g` runs on the cursor line only, if it matches
    /// (`ex_global()` with `global_busy`).
    fn run_global_on_cursor_line<D: Document>(
        &mut self,
        global: &GlobalCommand,
        run: &mut Run<'_, D>,
    ) -> bool {
        let refusal = if global.range != ExRange::entire_file() {
            Some(VimError::GlobalRecursiveWithRange)
        } else if run.global_depth > MAX_GLOBAL_RECURSION_DEPTH {
            Some(VimError::GlobalRecursionLimitExceeded {
                limit: MAX_GLOBAL_RECURSION_DEPTH,
            })
        } else {
            None
        };
        if let Some(error) = refusal {
            return self.add_line_effects(Effects::new().show_error(error), run);
        }
        let line = run.cursor_line();
        let matches = match &run.shadow {
            Some(doc) => self.global_line_matches_on(global, line, doc, run),
            None => self.global_line_matches_on(global, line, run.host(), run),
        };
        match matches {
            Ok(matches) if matches != global.invert => {
                run.global_depth += 1;
                let error = self.run_nested_line(&global.commands, OnError::Continue, run);
                run.global_depth -= 1;
                error
            }
            Ok(_) => false,
            Err(err) => self.add_line_effects(Effects::new().show_error(err), run),
        }
    }

    fn global_lines_on<D: Document, E: Document>(
        &self,
        global: &GlobalCommand,
        doc: &E,
        run: &Run<'_, D>,
    ) -> Result<Vec<usize>, VimError> {
        let input = InputContext::new(doc, run.cursor).validate_clamped();
        let ctx = ExecutionContext::new(input, &self.state, &self.options);
        executor_ex::global_lines(global, &ctx)
    }

    fn global_line_matches_on<D: Document, E: Document>(
        &self,
        global: &GlobalCommand,
        line: usize,
        doc: &E,
        run: &Run<'_, D>,
    ) -> Result<bool, VimError> {
        let input = InputContext::new(doc, run.cursor).validate_clamped();
        let ctx = ExecutionContext::new(input, &self.state, &self.options);
        executor_ex::global_line_matches(global, line, &ctx)
    }

    /// The report and cursor after `:g`: the substitutions, or else the
    /// lines added or removed, when more than 'report'; after a
    /// substitution the cursor goes to the first non-blank of its line
    /// (ex_cmds.c `global_exe()`, `do_sub_msg()`; misc1.c `msgmore()`).
    fn global_report<D: Document>(&self, run: &Run<'_, D>, line_count: usize) -> Effects {
        // global_exe_one() moved the cursor without an effect.
        let mut effects = Effects::new().set_cursor(crate::primitives::Offset::new(
            run.cursor.min(run.text().len()),
        ));
        let (count, lines) = run.substitutions;
        if count > 0 {
            if let Some(start) = run.line_start(run.cursor_line()) {
                let text = run.text();
                let blank = text
                    .get(start..)
                    .unwrap_or_default()
                    .bytes()
                    .take_while(|b| matches!(b, b' ' | b'\t'))
                    .count();
                let line_end = text
                    .get(start..)
                    .and_then(|rest| rest.find('\n'))
                    .map_or(text.len(), |end| start + end);
                effects = effects.set_cursor(crate::primitives::Offset::new(
                    (start + blank).min(line_end),
                ));
            }
        }
        if count > REPORT {
            let plural = |n: usize, word: &str| {
                if n == 1 {
                    format!("{n} {word}")
                } else {
                    format!("{n} {word}s")
                }
            };
            let message = format!(
                "{} on {}",
                plural(count, "substitution"),
                plural(lines, "line")
            );
            return effects.show_message(message);
        }
        let now = run.text().split('\n').count();
        let counts = if now > line_count + REPORT {
            LineModCounts {
                added: u32::try_from(now - line_count).unwrap_or(u32::MAX),
                ..LineModCounts::default()
            }
        } else if line_count > now + REPORT {
            LineModCounts {
                deleted: u32::try_from(line_count - now).unwrap_or(u32::MAX),
                ..LineModCounts::default()
            }
        } else {
            return effects;
        };
        effects.show_info(InfoMessage::LineReport(counts))
    }
}
