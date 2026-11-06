//! Running an Ex command line for [`VimEngine`]: each `|` command runs on
//! the text, cursor and marks the command before it left, as Vim's
//! `do_cmdline()` runs them.
//!
//! The host applies the effects in order, so each command's effects are
//! computed on an [`OwnedDocument`] that has the effects before them
//! applied, and the engine's state is synced after each command.

use compact_str::CompactString;
use smallvec::SmallVec;

use super::shadow_document::OwnedDocument;
use super::VimEngine;
use crate::document::Document;
use crate::effects::{Effect, Effects};
use crate::execution::executor_ex::{self, ExExecutionOutput};
use crate::execution::{ExecutionContext, HostRequest, InputContext, Validated};
use crate::grammar::split_ex_pipeline;
use crate::grammar::types::ExCommand;
use crate::primitives::{Mode, SelectionRange, UndoCursorStrategy};

/// Whether an error ends a command line (ex_docmd.c `do_cmdline()`): it
/// does for a typed line, `@:` and a line of `:@r`, not for a `<Cmd>`
/// mapping.
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

    /// Follow `effects` as the host applies them: the text and the cursor.
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

    /// Run the commands of `line`, as `:@r` runs a register line. Whether
    /// one gave an error.
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
        if output.last_ex_for_dot.take().is_some() {
            run.dot = true;
        }
        let ex_lines = std::mem::take(&mut output.ex_lines);
        let mut error = self.finish_piece(output, run);
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
}
