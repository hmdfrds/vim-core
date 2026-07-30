//! Ex command execution orchestrator.
//!
//! Dispatches parsed ex commands to core ex implementations or emits host
//! requests for shell-only operations.

use crate::commands::ex::effects as ex_effects;
use crate::commands::ex::types::ExContext;
use crate::dispatch::dispatch_resolve_ex_range;
use crate::document::Document;
use crate::effects::Effects;
use crate::errors::VimError;
use crate::execution::host::{HostRequestSequencer, HostResult, SplitDirection};
use crate::execution::{HostRequest, HostRequestMeta};
use crate::grammar::types::{ExCommand, ExRange, LineSpec, TimeAmount};
use crate::grammar::{parse_ex_command_with_modifiers, split_ex_pipeline};
use crate::keymap::{Handler, KeyEvent, MappingMode};
use crate::primitives::byte_delta;
use crate::primitives::{
    MotionType, OptionId, OptionOverrides, OptionValue, Range, RegisterContent, RegisterName,
};
use compact_str::CompactString;
use smallvec::SmallVec;

use super::shell_expand::expand_shell_tokens;
use super::ExecutionContext;

/// The scope at which a `:set`-family command operates.
///
/// Controls which option layers are written when applying assignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SetScope {
    /// `:set` — writes both the global layer and the appropriate local override.
    #[default]
    Effective,
    /// `:setlocal` — writes only the local layer (buffer or window override).
    ///
    /// For Global-scoped options, falls through to the global layer (Vim behaviour).
    Local,
    /// `:setglobal` — writes only the global layer, never touching overrides.
    Global,
}

/// A handler map mutation from `:sethandler`.
#[derive(Debug)]
pub(crate) struct HandlerChange {
    /// Optional key (if `None`, applies as a global default — currently
    /// we iterate all keys via `set_all_modes`).
    pub key: Option<crate::keymap::KeyEvent>,
    /// Target modes for this assignment.
    pub modes: SmallVec<[crate::keymap::MappingMode; 4]>,
    /// The handler to assign.
    pub handler: crate::keymap::Handler,
}

/// A mapping mutation from `:map`/`:noremap`/`:unmap`.
///
/// Carries raw notation strings so that `<Action>(name)` and `<Plug>(name)`
/// can be resolved against the keymap's name registries at application time
/// (when the engine has `&mut Keymap` available).
#[derive(Debug)]
pub(crate) enum MappingChange {
    /// Define a new mapping.
    Define {
        modes: SmallVec<[crate::keymap::MappingMode; 4]>,
        lhs_raw: CompactString,
        rhs_raw: CompactString,
        kind: crate::keymap::MappingKind,
        flags: crate::keymap::MappingFlags,
    },
    /// Remove a mapping.
    Remove {
        modes: SmallVec<[crate::keymap::MappingMode; 4]>,
        lhs_raw: CompactString,
    },
    /// Clear all mappings for the given modes.
    ClearAll {
        modes: SmallVec<[crate::keymap::MappingMode; 4]>,
    },
}

/// An abbreviation table mutation from `:abbreviate`/`:unabbreviate`/`:abclear`.
///
/// The executor has only `&VimState` so it cannot mutate the `AbbrevTable`
/// directly; it signals the engine to apply the change where `&mut` access
/// is available.
#[derive(Debug)]
pub(crate) enum AbbrevChange {
    /// Define or list an abbreviation.
    Define {
        trigger: Option<CompactString>,
        replacement: Option<CompactString>,
        mode: crate::primitives::AbbrevMode,
        noremap: bool,
    },
    /// Remove an abbreviation.
    Remove {
        trigger: CompactString,
        mode: crate::primitives::AbbrevMode,
    },
    /// Clear all abbreviations for a mode.
    Clear { mode: crate::primitives::AbbrevMode },
}

/// Undo tree navigation request from `:earlier`/`:later`/`:undolist`/`:undo N`.
///
/// The executor cannot perform tree navigation (needs `&mut VimState`),
/// so it signals the engine to handle it.
#[derive(Debug)]
pub(crate) enum UndoNavigation {
    /// Navigate backward in undo history.
    Earlier(TimeAmount),
    /// Navigate forward in undo history.
    Later(TimeAmount),
    /// Display the undo tree leaf nodes.
    UndoList,
    /// Visualize the full undo tree.
    UndoTree,
    /// Jump to the node with the given sequence number (`:undo N`).
    GotoSequence(u64),
}

/// Execution output for an ex command.
#[derive(Debug, Default)]
pub(crate) struct ExExecutionOutput {
    /// Core effects to apply.
    pub effects: Effects,
    /// Host operations to execute outside core.
    pub host_requests: SmallVec<[HostRequest; 4]>,
    /// Option mutations from `:set` (engine applies these to `VimOptions`).
    pub set_assignments: SmallVec<[crate::grammar::types::SetAssignment; 2]>,
    /// Scope for set assignments — controls which option layers are written.
    pub set_scope: SetScope,
    /// Mapping mutations from `:map`/`:noremap`/`:unmap`.
    pub mapping_changes: SmallVec<[MappingChange; 1]>,
    /// Abbreviation mutations from `:abbreviate`/`:unabbreviate`/`:abclear`.
    pub abbrev_changes: SmallVec<[AbbrevChange; 1]>,
    /// Handler map mutations from `:sethandler`.
    pub handler_changes: SmallVec<[HandlerChange; 2]>,
    /// Undo tree navigation (engine handles tree mutation).
    pub undo_navigation: Option<UndoNavigation>,
    /// Leader key change from `:let mapleader = ...` (engine applies to keymap).
    pub leader_change: Option<char>,
    /// Multi-cursor command to execute (engine handles `&mut VimState` mutation).
    ///
    /// The executor has only `&VimState`, so it signals the engine to run
    /// the multi-cursor command where `&mut` access is available.
    pub multi_cursor_command: Option<crate::state::MultiCursorCommand>,
    /// If true, the engine should clear `VimState::message_history` (`:messages clear`).
    ///
    /// The executor has only `&VimState` so it cannot mutate history directly;
    /// it signals the engine via this flag instead.
    pub clear_message_history: bool,
    /// If true, the engine should clear the jump list (`:clearjumps`).
    ///
    /// The executor has only `&VimState` so it cannot mutate the jump list;
    /// it signals the engine via this flag instead.
    pub clear_jumplist: bool,
    /// If true, the next text-mutating command should merge into the
    /// previous undo group (`:undojoin`).
    ///
    /// The executor signals this; the engine applies it by calling
    /// `begin_merge()` on the undo tree before the next edit.
    pub undo_join_pending: bool,
    /// If `Some`, store as the last ex command for dot-repeat.
    ///
    /// Only text-mutating ex commands set this. The engine saves it so that
    /// `.` can replay the ex command with the current line as implicit range.
    pub last_ex_for_dot: Option<CompactString>,
}

/// Parse and execute an ex command line.
///
/// Supports `|`-separated pipelines: `s/a/b/|w|q` executes each command
/// sequentially, stopping on the first error.
///
/// Modifier prefixes (`:silent`, `:keepjumps`, etc.) are parsed and applied
/// as post-processing filters on the effects output.
pub(crate) fn execute_ex_line<D: Document>(
    line: &str,
    ctx: &ExecutionContext<'_, D>,
    sequencer: &mut HostRequestSequencer,
) -> Result<ExExecutionOutput, VimError> {
    let commands = split_ex_pipeline(line);
    if commands.len() <= 1 {
        // Fast path: single command (or empty) -- no allocation overhead.
        let (modifiers, command) = parse_ex_command_with_modifiers(line)?;
        let mut output = execute_ex_command(&command, ctx, sequencer)?;
        super::modifier_filter::apply_modifiers(&mut output.effects, modifiers);
        // Populate last_ex_for_dot for text-mutating commands (dot-repeat).
        if command.mutates_text() && output.last_ex_for_dot.is_none() {
            output.last_ex_for_dot = Some(CompactString::from(line));
        }
        return Ok(output);
    }

    let mut combined = ExExecutionOutput::default();
    let mut any_mutates_text = false;
    for cmd_str in commands {
        let (modifiers, parsed) = match parse_ex_command_with_modifiers(cmd_str) {
            Ok(result) => result,
            Err(err) => {
                combined
                    .effects
                    .extend(crate::commands::ex::effects::show_error(err));
                return Ok(combined);
            }
        };
        if parsed.mutates_text() {
            any_mutates_text = true;
        }
        match execute_ex_command(&parsed, ctx, sequencer) {
            Ok(mut output) => {
                super::modifier_filter::apply_modifiers(&mut output.effects, modifiers);
                merge_output(&mut combined, output);
            }
            Err(err) => {
                combined
                    .effects
                    .extend(crate::commands::ex::effects::show_error(err));
                return Ok(combined);
            }
        }
    }
    // Populate last_ex_for_dot for pipelines containing text-mutating commands.
    if any_mutates_text && combined.last_ex_for_dot.is_none() {
        combined.last_ex_for_dot = Some(CompactString::from(line));
    }
    Ok(combined)
}

/// Merge a pipeline step's output into the accumulated result.
fn merge_output(target: &mut ExExecutionOutput, source: ExExecutionOutput) {
    target.effects.extend(source.effects);
    target.host_requests.extend(source.host_requests);
    target.set_assignments.extend(source.set_assignments);
    target.set_scope = source.set_scope;
    target.mapping_changes.extend(source.mapping_changes);
    target.abbrev_changes.extend(source.abbrev_changes);
    target.handler_changes.extend(source.handler_changes);
    if source.undo_navigation.is_some() {
        target.undo_navigation = source.undo_navigation;
    }
    if source.leader_change.is_some() {
        target.leader_change = source.leader_change;
    }
    if source.multi_cursor_command.is_some() {
        target.multi_cursor_command = source.multi_cursor_command;
    }
    if source.clear_message_history {
        target.clear_message_history = true;
    }
    if source.clear_jumplist {
        target.clear_jumplist = true;
    }
    if source.undo_join_pending {
        target.undo_join_pending = true;
    }
    if source.last_ex_for_dot.is_some() {
        target.last_ex_for_dot = source.last_ex_for_dot;
    }
}

/// Execute a parsed ex command.
pub(crate) fn execute_ex_command<D: Document>(
    command: &ExCommand,
    ctx: &ExecutionContext<'_, D>,
    sequencer: &mut HostRequestSequencer,
) -> Result<ExExecutionOutput, VimError> {
    let cursor_line = ctx.cursor_pos().line().get();
    let mark_resolver = |name: crate::primitives::MarkName| {
        ctx.state.marks().get(name).map(|mark| mark.offset().get())
    };
    let last_sub = ctx.state.search().last_substitute();
    let resolved_sub_pat = ctx.state.search().resolve_substitute_pattern();
    let last_sub_pat = ctx.state.search().substitute_pattern();
    let last_sub_flags = ctx.state.search().last_substitute_flags();
    let last_search_pat = ctx.state.search().pattern();
    #[allow(unused_mut)]
    let mut ex_ctx = ExContext::new(ctx.doc().text(), cursor_line)
        .with_mark_resolver(&mark_resolver)
        .with_last_substitute(last_sub)
        .with_gdefault(ctx.options.gdefault())
        .with_edcompatible(ctx.options.edcompatible())
        .with_resolved_substitute_pattern(resolved_sub_pat)
        .with_last_substitute_pattern(last_sub_pat)
        .with_last_substitute_flags(last_sub_flags)
        .with_last_search_pattern(last_search_pat);
    if let Some(tree) = ctx.doc().vim_text_tree() {
        ex_ctx = ex_ctx.with_tree(tree);
    }

    if let Some(result) = crate::dispatch::dispatch_ex_core(command, &ex_ctx) {
        return if command.mutates_text() {
            effects_only_undo(result)
        } else {
            effects_only(result)
        };
    }

    match command {
        ExCommand::Write { path, force } => host_only(HostRequest::WriteFile {
            meta: sequencer.next_meta(),
            path: path.as_ref().map(|p| {
                if p.starts_with('!') {
                    let cur = ctx
                        .state
                        .registers()
                        .get(RegisterName::FILENAME)
                        .map(RegisterContent::text);
                    let alt = ctx
                        .state
                        .registers()
                        .get(RegisterName::ALTERNATE)
                        .map(RegisterContent::text);
                    expand_shell_tokens(p, cur, alt).into()
                } else {
                    p.clone()
                }
            }),
            force: *force,
        }),
        ExCommand::Quit { force } => host_only(HostRequest::Quit {
            meta: sequencer.next_meta(),
            force: *force,
        }),
        ExCommand::WriteQuit { force } => host_only(HostRequest::WriteQuit {
            meta: sequencer.next_meta(),
            force: *force,
        }),
        ExCommand::Edit { path, force } => host_only(HostRequest::EditFile {
            meta: sequencer.next_meta(),
            path: path.clone(),
            force: *force,
        }),
        ExCommand::Read { path, after_line } => {
            let expanded_path = if path.starts_with('!') {
                let cur = ctx
                    .state
                    .registers()
                    .get(RegisterName::FILENAME)
                    .map(RegisterContent::text);
                let alt = ctx
                    .state
                    .registers()
                    .get(RegisterName::ALTERNATE)
                    .map(RegisterContent::text);
                expand_shell_tokens(path, cur, alt).into()
            } else {
                path.clone()
            };
            host_only(HostRequest::ReadFile {
                meta: sequencer.next_meta(),
                path: expanded_path,
                after_line: *after_line,
            })
        }
        ExCommand::Filter { range, command } => {
            let resolved = dispatch_resolve_ex_range(range, &ex_ctx)?;
            let (start, end) = ex_ctx
                .lines_range(resolved.start(), resolved.end())
                .ok_or_else(|| VimError::InvalidAddress(format_ex_range(range).into()))?;
            let raw = &ctx.doc().text()[start.get()..end.get()];
            let current_file = ctx
                .state
                .registers()
                .get(RegisterName::FILENAME)
                .map(RegisterContent::text);
            let alt_file = ctx
                .state
                .registers()
                .get(RegisterName::ALTERNATE)
                .map(RegisterContent::text);
            let expanded = expand_shell_tokens(command, current_file, alt_file);
            host_only(HostRequest::FilterDocumentRange {
                meta: sequencer.next_meta(),
                range: Range::from_raw(start.get(), end.get()),
                motion_type: MotionType::LineWise,
                input_text: raw.into(),
                command: expanded.into(),
            })
        }
        ExCommand::External { command } => {
            let current_file = ctx
                .state
                .registers()
                .get(RegisterName::FILENAME)
                .map(RegisterContent::text);
            let alt_file = ctx
                .state
                .registers()
                .get(RegisterName::ALTERNATE)
                .map(RegisterContent::text);
            let expanded = expand_shell_tokens(command, current_file, alt_file);
            host_only(HostRequest::ExternalCommand {
                meta: sequencer.next_meta(),
                command: expanded.into(),
            })
        }
        ExCommand::Custom { command } => host_only(HostRequest::CustomExCommand {
            meta: sequencer.next_meta(),
            command: command.clone(),
        }),
        ExCommand::Set { assignments } => Ok(ExExecutionOutput {
            set_assignments: assignments.clone(),
            set_scope: SetScope::Effective,
            ..ExExecutionOutput::default()
        }),
        ExCommand::SetLocal { assignments } => Ok(ExExecutionOutput {
            set_assignments: assignments.clone(),
            set_scope: SetScope::Local,
            ..ExExecutionOutput::default()
        }),
        ExCommand::SetGlobal { assignments } => Ok(ExExecutionOutput {
            set_assignments: assignments.clone(),
            set_scope: SetScope::Global,
            ..ExExecutionOutput::default()
        }),
        ExCommand::Registers { filter } => {
            let msg = format_registers(ctx.state.registers(), filter.as_deref());
            effects_only(Ok(ex_effects::show_message(msg.into())))
        }
        ExCommand::Marks { filter } => {
            let msg = format_marks(ctx.state.marks(), ctx.doc().text(), filter.as_deref());
            effects_only(Ok(ex_effects::show_message(msg.into())))
        }
        ExCommand::Jumps => {
            let msg = format_jumps(ctx.state.jump_list(), ctx.doc().text());
            effects_only(Ok(ex_effects::show_message(msg.into())))
        }
        ExCommand::Changes => {
            let msg = format_changes(ctx.state.changelist(), ctx.doc().text());
            effects_only(Ok(ex_effects::show_message(msg.into())))
        }
        ExCommand::ClearJumps => {
            // The executor has only `&VimState` — signal the engine to clear.
            Ok(ExExecutionOutput {
                clear_jumplist: true,
                ..ExExecutionOutput::default()
            })
        }
        ExCommand::Messages { clear } => {
            if *clear {
                // `:messages clear` — the executor cannot mutate state (ctx.state is &VimState).
                // Signal the engine to clear; command_line_exec.rs checks this flag
                // after execute_ex_command returns.
                Ok(ExExecutionOutput {
                    clear_message_history: true,
                    ..ExExecutionOutput::default()
                })
            } else {
                let entries = ctx.state.message_history().to_vec();
                host_only(HostRequest::ShowMessageHistory {
                    meta: sequencer.next_meta(),
                    entries,
                })
            }
        }
        ExCommand::Put {
            range,
            register,
            before,
        } => {
            let reg = register.unwrap_or(crate::primitives::RegisterName::UNNAMED);
            let text = ctx
                .state
                .registers()
                .get(reg)
                .map_or(String::new(), |c| c.text().to_owned());
            if text.is_empty() {
                return effects_only(Ok(Effects::new()));
            }
            // Detect `:0put` — Vim line 0 means "before first line"
            let target = if matches!(&range.start, crate::grammar::types::LineSpec::Absolute(0))
                && range.end.is_none()
            {
                None
            } else {
                Some(dispatch_resolve_ex_range(range, &ex_ctx)?.end())
            };
            effects_only_undo(crate::commands::ex::text_ops::put(
                &ex_ctx, target, &text, *before,
            ))
        }
        ExCommand::Retab {
            range,
            new_tabstop,
            to_tabs,
        } => {
            let ts = new_tabstop.unwrap_or(ctx.options.tabstop());
            effects_only_undo(crate::commands::ex::text_ops::retab(
                range, ts, *to_tabs, &ex_ctx,
            ))
        }
        ExCommand::Left { range, indent } => {
            effects_only_undo(crate::commands::ex::text_ops::left(range, *indent, &ex_ctx))
        }
        ExCommand::Right { range, width } => {
            let w = resolve_width(*width, ctx.options.textwidth());
            effects_only_undo(crate::commands::ex::text_ops::right(range, w, &ex_ctx))
        }
        ExCommand::Center { range, width } => {
            let w = resolve_width(*width, ctx.options.textwidth());
            effects_only_undo(crate::commands::ex::text_ops::center(range, w, &ex_ctx))
        }
        ExCommand::SetHandler { key, assignments } => {
            execute_sethandler(key.as_deref(), assignments)
        }
        ExCommand::Map {
            mode_prefix,
            lhs,
            rhs,
            kind,
            flags,
        } => execute_map_command(*mode_prefix, lhs, rhs.as_deref(), *kind, *flags),
        ExCommand::Unmap { mode_prefix, lhs } => execute_unmap_command(*mode_prefix, lhs),
        ExCommand::MapClear { mode, .. } => execute_mapclear_command(*mode),
        ExCommand::Action { name } => host_only(HostRequest::RunAction {
            meta: sequencer.next_meta(),
            name: name.clone(),
            count: None,
            register: None,
            mode: CompactString::from("COMMAND"),
            selection_anchor: None,
            selection_head: None,
        }),
        ExCommand::ActionList { filter } => host_only(HostRequest::ListActions {
            meta: sequencer.next_meta(),
            filter: filter.clone(),
        }),
        ExCommand::Source { path } => host_only(HostRequest::ReadConfigFile {
            meta: sequencer.next_meta(),
            path: path.clone(),
        }),
        ExCommand::Redo => undo_navigation(UndoNavigation::Later(TimeAmount::Changes(1))),
        ExCommand::Earlier { amount } => undo_navigation(UndoNavigation::Earlier(*amount)),
        ExCommand::Later { amount } => undo_navigation(UndoNavigation::Later(*amount)),
        ExCommand::UndoList => undo_navigation(UndoNavigation::UndoList),
        ExCommand::UndoTree => undo_navigation(UndoNavigation::UndoTree),
        ExCommand::UndoSequence { seq } => undo_navigation(UndoNavigation::GotoSequence(*seq)),
        // Buffer navigation
        ExCommand::Buffer { number } => host_only(HostRequest::SwitchBuffer {
            meta: sequencer.next_meta(),
            number: *number,
        }),
        // Diagnostic navigation
        ExCommand::CNext { count } => host_only(HostRequest::DiagnosticNext {
            meta: sequencer.next_meta(),
            count: *count,
        }),
        ExCommand::CPrev { count } => host_only(HostRequest::DiagnosticPrev {
            meta: sequencer.next_meta(),
            count: *count,
        }),
        ExCommand::CList => host_only(HostRequest::DiagnosticList {
            meta: sequencer.next_meta(),
        }),
        ExCommand::CC { index } => host_only(HostRequest::DiagnosticGoto {
            meta: sequencer.next_meta(),
            index: index.unwrap_or(1),
        }),
        ExCommand::BufferNext { count } => host_only(HostRequest::BufferNext {
            meta: sequencer.next_meta(),
            count: *count,
        }),
        ExCommand::BufferPrev { count } => host_only(HostRequest::BufferPrev {
            meta: sequencer.next_meta(),
            count: *count,
        }),
        ExCommand::BufferFirst => host_only(HostRequest::BufferFirst {
            meta: sequencer.next_meta(),
        }),
        ExCommand::BufferLast => host_only(HostRequest::BufferLast {
            meta: sequencer.next_meta(),
        }),
        ExCommand::BufferList => host_only(HostRequest::BufferList {
            meta: sequencer.next_meta(),
        }),
        // Tab navigation
        ExCommand::TabNew { path } => host_only(HostRequest::TabNew {
            meta: sequencer.next_meta(),
            path: path.clone(),
        }),
        ExCommand::TabNext { count } => host_only(HostRequest::TabNext {
            meta: sequencer.next_meta(),
            count: *count,
        }),
        ExCommand::TabPrev { count } => host_only(HostRequest::TabPrev {
            meta: sequencer.next_meta(),
            count: *count,
        }),
        ExCommand::TabClose { force } => host_only(HostRequest::TabClose {
            meta: sequencer.next_meta(),
            force: *force,
        }),
        // Window / session / buffer management
        ExCommand::Split { path } => host_only(HostRequest::SplitWindow {
            meta: sequencer.next_meta(),
            direction: SplitDirection::Horizontal,
            path: path.clone(),
            new_file: false,
        }),
        ExCommand::VSplit { path } => host_only(HostRequest::SplitWindow {
            meta: sequencer.next_meta(),
            direction: SplitDirection::Vertical,
            path: path.clone(),
            new_file: false,
        }),
        ExCommand::Close { force } => host_only(HostRequest::CloseWindow {
            meta: sequencer.next_meta(),
            force: *force,
        }),
        ExCommand::Only { force } => host_only(HostRequest::CloseOtherWindows {
            meta: sequencer.next_meta(),
            force: *force,
        }),
        ExCommand::New => host_only(HostRequest::SplitWindow {
            meta: sequencer.next_meta(),
            direction: SplitDirection::Horizontal,
            path: None,
            new_file: true,
        }),
        ExCommand::VNew => host_only(HostRequest::SplitWindow {
            meta: sequencer.next_meta(),
            direction: SplitDirection::Vertical,
            path: None,
            new_file: true,
        }),
        ExCommand::WriteAll => host_only(HostRequest::WriteAll {
            meta: sequencer.next_meta(),
        }),
        ExCommand::QuitAll { force } => host_only(HostRequest::QuitAll {
            meta: sequencer.next_meta(),
            force: *force,
        }),
        ExCommand::WriteQuitAll => host_only(HostRequest::WriteQuitAll {
            meta: sequencer.next_meta(),
        }),
        ExCommand::BufferDelete { force, target } => host_only(HostRequest::CloseBuffer {
            meta: sequencer.next_meta(),
            force: *force,
            wipeout: false,
            target: target.clone(),
        }),
        ExCommand::BufferWipeout { force, target } => host_only(HostRequest::CloseBuffer {
            meta: sequencer.next_meta(),
            force: *force,
            wipeout: true,
            target: target.clone(),
        }),
        // Display / Misc
        ExCommand::Echo { message } => effects_only(Ok(ex_effects::show_message(message.clone()))),
        ExCommand::LetMapleader { leader } => Ok(ExExecutionOutput {
            leader_change: Some(*leader),
            ..ExExecutionOutput::default()
        }),
        ExCommand::PrintLines { .. } | ExCommand::ZWindow { .. } => {
            // Display commands handled by core dispatch (already dispatched above).
            effects_only(Ok(Effects::new()))
        }
        // Multi-cursor selection commands — signal the engine to execute.
        ExCommand::SelectMatches { pattern, .. } => multi_cursor_signal(
            crate::state::MultiCursorCommand::SelectOnMatches {
                pattern: pattern.clone(),
            },
            false,
        ),
        ExCommand::SplitMatches { pattern, .. } => multi_cursor_signal(
            crate::state::MultiCursorCommand::SplitOnMatches {
                pattern: pattern.clone(),
            },
            false,
        ),
        ExCommand::KeepMatches { pattern, .. } => multi_cursor_signal(
            crate::state::MultiCursorCommand::KeepMatching {
                pattern: pattern.clone(),
            },
            false,
        ),
        ExCommand::RemoveMatches { pattern, .. } => multi_cursor_signal(
            crate::state::MultiCursorCommand::RemoveMatching {
                pattern: pattern.clone(),
            },
            false,
        ),
        ExCommand::TrimSelections => {
            multi_cursor_signal(crate::state::MultiCursorCommand::TrimSelections, false)
        }
        ExCommand::AlignSelections => {
            multi_cursor_signal(crate::state::MultiCursorCommand::AlignSelections, true)
        }
        ExCommand::RotateContents => multi_cursor_signal(
            crate::state::MultiCursorCommand::RotateContents(crate::primitives::Direction::Forward),
            true,
        ),
        ExCommand::RotateContentsDir { direction } => multi_cursor_signal(
            crate::state::MultiCursorCommand::RotateContents(*direction),
            true,
        ),
        ExCommand::AddNext { .. } => multi_cursor_signal(
            crate::state::MultiCursorCommand::AddNextMatch {
                direction: crate::primitives::Direction::Forward,
                skip: false,
            },
            false,
        ),
        ExCommand::AddPrev { .. } => multi_cursor_signal(
            crate::state::MultiCursorCommand::AddNextMatch {
                direction: crate::primitives::Direction::Backward,
                skip: false,
            },
            false,
        ),
        ExCommand::SkipMatch => multi_cursor_signal(
            crate::state::MultiCursorCommand::AddNextMatch {
                direction: crate::primitives::Direction::Forward,
                skip: true,
            },
            false,
        ),
        ExCommand::AddCursorDir { direction, .. } => multi_cursor_signal(
            crate::state::MultiCursorCommand::AddCursorVertical(*direction),
            false,
        ),
        ExCommand::SelectAll => multi_cursor_signal(
            crate::state::MultiCursorCommand::SelectAllOccurrences,
            false,
        ),
        ExCommand::CursorCollapse => {
            multi_cursor_signal(crate::state::MultiCursorCommand::ClearSecondary, false)
        }
        ExCommand::CursorRemove => {
            let offset = crate::primitives::Offset::new(ctx.cursor_offset());
            multi_cursor_signal(
                crate::state::MultiCursorCommand::RemoveCursor(offset),
                false,
            )
        }
        ExCommand::CursorPrimary { direction } => multi_cursor_signal(
            crate::state::MultiCursorCommand::RotatePrimary(*direction),
            false,
        ),
        ExCommand::CursorSplitBlock => {
            multi_cursor_signal(crate::state::MultiCursorCommand::CursorSplit, false)
        }
        ExCommand::CursorFlip => {
            multi_cursor_signal(crate::state::MultiCursorCommand::FlipSelections, false)
        }
        ExCommand::CursorForward => {
            multi_cursor_signal(crate::state::MultiCursorCommand::EnsureForward, false)
        }
        ExCommand::CursorMerge => {
            multi_cursor_signal(crate::state::MultiCursorCommand::MergeConsecutive, false)
        }
        // ── :delmarks ──────────────────────────────────────────────────────
        ExCommand::DelMarks { marks, clear_all } => {
            let mut effects = Effects::new();
            if *clear_all {
                // Clear all lowercase marks a-z
                for c in 'a'..='z' {
                    if let Some(name) = crate::primitives::MarkName::new(c) {
                        effects = effects.clear_mark(name);
                    }
                }
            } else {
                for c in marks.chars() {
                    if let Some(name) = crate::primitives::MarkName::new(c) {
                        effects = effects.clear_mark(name);
                    }
                }
            }
            effects_only(Ok(effects))
        }
        // ── :cquit ──────────────────────────────────────────────────────────
        ExCommand::CQuit { exit_code } => host_only(HostRequest::CQuit {
            meta: sequencer.next_meta(),
            exit_code: *exit_code,
        }),
        // ── :update ─────────────────────────────────────────────────────────
        ExCommand::Update { path, force } => host_only(HostRequest::UpdateFile {
            meta: sequencer.next_meta(),
            path: path.clone(),
            force: *force,
        }),
        // ── :fold ───────────────────────────────────────────────────────────
        ExCommand::Fold { range } => {
            let resolved = dispatch_resolve_ex_range(range, &ex_ctx)?;
            host_only(HostRequest::FoldRange {
                meta: sequencer.next_meta(),
                start_line: byte_delta::to_u32(resolved.start()),
                end_line: byte_delta::to_u32(resolved.end()),
            })
        }
        // ── :foldopen ───────────────────────────────────────────────────────
        ExCommand::FoldOpen { range, recursive } => {
            let resolved = dispatch_resolve_ex_range(range, &ex_ctx)?;
            host_only(HostRequest::FoldOpenRange {
                meta: sequencer.next_meta(),
                start_line: byte_delta::to_u32(resolved.start()),
                end_line: byte_delta::to_u32(resolved.end()),
                recursive: *recursive,
            })
        }
        // ── :foldclose ──────────────────────────────────────────────────────
        ExCommand::FoldClose { range, recursive } => {
            let resolved = dispatch_resolve_ex_range(range, &ex_ctx)?;
            host_only(HostRequest::FoldCloseRange {
                meta: sequencer.next_meta(),
                start_line: byte_delta::to_u32(resolved.start()),
                end_line: byte_delta::to_u32(resolved.end()),
                recursive: *recursive,
            })
        }
        // ── Iterator commands (:windo, :bufdo, :tabdo) ───────────────────
        ExCommand::WinDo { source, .. } => host_only(HostRequest::ForEachWindow {
            meta: sequencer.next_meta(),
            command: source.clone(),
        }),
        ExCommand::BufDo { source, .. } => host_only(HostRequest::ForEachBuffer {
            meta: sequencer.next_meta(),
            command: source.clone(),
        }),
        ExCommand::TabDo { source, .. } => host_only(HostRequest::ForEachTab {
            meta: sequencer.next_meta(),
            command: source.clone(),
        }),
        // ── :@{register} — execute register as ex commands ───────────
        ExCommand::ExecuteRegister { register } => {
            let text = ctx
                .state
                .registers()
                .get(*register)
                .map(|c| c.text().to_owned())
                .unwrap_or_default();
            if text.is_empty() {
                return effects_only(Ok(Effects::new().show_message(CompactString::from(
                    format!("Register @{} is empty", register.char()),
                ))));
            }
            // Split by newlines and execute each as an ex command.
            // Handle line continuation with trailing backslash.
            let mut combined = ExExecutionOutput::default();
            let mut continuation = String::new();
            for raw_line in text.lines() {
                if raw_line.ends_with('\\') {
                    // Line continuation: strip trailing backslash, append next line.
                    continuation.push_str(&raw_line[..raw_line.len() - 1]);
                    continue;
                }
                let line = if continuation.is_empty() {
                    raw_line.to_owned()
                } else {
                    continuation.push_str(raw_line);
                    std::mem::take(&mut continuation)
                };
                if line.trim().is_empty() {
                    continue;
                }
                match execute_ex_line(&line, ctx, sequencer) {
                    Ok(output) => merge_output(&mut combined, output),
                    Err(e) => {
                        combined
                            .effects
                            .extend(crate::commands::ex::effects::show_error(e));
                        return Ok(combined);
                    }
                }
            }
            // Handle any remaining continuation line.
            if !continuation.is_empty() {
                match execute_ex_line(&continuation, ctx, sequencer) {
                    Ok(output) => merge_output(&mut combined, output),
                    Err(e) => {
                        combined
                            .effects
                            .extend(crate::commands::ex::effects::show_error(e));
                    }
                }
            }
            Ok(combined)
        }
        // ── :undojoin ────────────────────────────────────────────────
        ExCommand::UndoJoin => {
            if ctx.state.undo_tree().last_was_undo() {
                return Err(VimError::E790);
            }
            Ok(ExExecutionOutput {
                undo_join_pending: true,
                ..ExExecutionOutput::default()
            })
        }
        // ── :mkvimrc ────────────────────────────────────────────────────
        ExCommand::MkVimrc { force } => host_only(HostRequest::MkVimrc {
            meta: sequencer.next_meta(),
            force: *force,
        }),
        // Abbreviation commands
        ExCommand::Abbreviate {
            trigger,
            replacement,
            mode,
            noremap,
        } => Ok(ExExecutionOutput {
            abbrev_changes: smallvec::smallvec![AbbrevChange::Define {
                trigger: trigger.clone(),
                replacement: replacement.clone(),
                mode: *mode,
                noremap: *noremap,
            }],
            ..ExExecutionOutput::default()
        }),
        ExCommand::Unabbreviate { trigger, mode } => Ok(ExExecutionOutput {
            abbrev_changes: smallvec::smallvec![AbbrevChange::Remove {
                trigger: trigger.clone(),
                mode: *mode,
            }],
            ..ExExecutionOutput::default()
        }),
        ExCommand::AbClear { mode } => Ok(ExExecutionOutput {
            abbrev_changes: smallvec::smallvec![AbbrevChange::Clear { mode: *mode }],
            ..ExExecutionOutput::default()
        }),
        other => {
            debug_assert!(
                false,
                "Unhandled ex command reached host dispatch: {other:?}"
            );
            Err(crate::errors::VimError::NotEditorCommand(
                format!("{other:?}").into(),
            ))
        }
    }
}

/// Default text width when `textwidth` is 0 (matches Vim's `:center`/`:right` default).
const DEFAULT_TEXT_WIDTH: usize = 80;

fn resolve_width(explicit: Option<usize>, textwidth: usize) -> usize {
    explicit.unwrap_or(if textwidth > 0 {
        textwidth
    } else {
        DEFAULT_TEXT_WIDTH
    })
}

fn effects_only(result: Result<Effects, VimError>) -> Result<ExExecutionOutput, VimError> {
    let effects = result?;
    Ok(ExExecutionOutput {
        effects,
        ..ExExecutionOutput::default()
    })
}

/// Like `effects_only`, but wraps in undo group if edits are present.
fn effects_only_undo(result: Result<Effects, VimError>) -> Result<ExExecutionOutput, VimError> {
    let effects = result?.undo_wrap();
    Ok(ExExecutionOutput {
        effects,
        ..ExExecutionOutput::default()
    })
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match ExExecutionOutput signature"
)]
fn host_only(request: HostRequest) -> Result<ExExecutionOutput, VimError> {
    Ok(ExExecutionOutput {
        host_requests: smallvec::smallvec![request],
        ..ExExecutionOutput::default()
    })
}

/// Format an `ExCommand` back to its textual representation for host iteration.
///
/// Signal the engine to execute a multi-cursor command.
///
/// The executor cannot mutate `VimState` (only `&` access), so it defers
/// to the engine via the `multi_cursor_command` field. The `mutates_text`
/// flag tells the engine whether to wrap in an undo group.
#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match ExExecutionOutput signature"
)]
fn multi_cursor_signal(
    cmd: crate::state::MultiCursorCommand,
    _mutates_text: bool,
) -> Result<ExExecutionOutput, VimError> {
    Ok(ExExecutionOutput {
        multi_cursor_command: Some(cmd),
        ..ExExecutionOutput::default()
    })
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match ExExecutionOutput signature"
)]
fn undo_navigation(nav: UndoNavigation) -> Result<ExExecutionOutput, VimError> {
    Ok(ExExecutionOutput {
        undo_navigation: Some(nav),
        ..ExExecutionOutput::default()
    })
}

/// Map an option name (long or short) to its [`OptionId`].
///
/// Returns `None` for unknown option names; callers fall through to the
/// existing name-dispatch helpers in that case.
fn option_name_to_id(name: &str) -> Option<OptionId> {
    match name {
        "ignorecase" | "ic" => Some(OptionId::IgnoreCase),
        "smartcase" | "scs" => Some(OptionId::SmartCase),
        "hlsearch" | "hls" => Some(OptionId::HlSearch),
        "incsearch" | "is" => Some(OptionId::IncSearch),
        "wrapscan" | "ws" => Some(OptionId::WrapScan),
        "gdefault" | "gd" => Some(OptionId::GDefault),
        "clipboard" | "cb" => Some(OptionId::Clipboard),
        "inccommand" | "icm" => Some(OptionId::IncCommand),
        "timeoutlen" | "tm" => Some(OptionId::TimeoutLen),
        "undolevels" | "ul" => Some(OptionId::UndoLevels),
        "tabstop" | "ts" => Some(OptionId::TabStop),
        "shiftwidth" | "sw" => Some(OptionId::ShiftWidth),
        "expandtab" | "et" => Some(OptionId::ExpandTab),
        "autoindent" | "ai" => Some(OptionId::AutoIndent),
        "smartindent" | "si" => Some(OptionId::SmartIndent),
        "commentstring" => Some(OptionId::CommentString),
        "iskeyword" | "isk" => Some(OptionId::IsKeyword),
        "textwidth" | "tw" => Some(OptionId::TextWidth),
        "scrolloff" | "so" => Some(OptionId::ScrollOff),
        "number" | "nu" => Some(OptionId::Number),
        "relativenumber" | "rnu" => Some(OptionId::RelativeNumber),
        "sidescrolloff" | "siso" => Some(OptionId::SideScrollOff),
        "virtualedit" | "ve" => Some(OptionId::VirtualEdit),
        "selection" | "sel" => Some(OptionId::Selection),
        "backspace" | "bs" => Some(OptionId::Backspace),
        "whichwrap" | "ww" => Some(OptionId::WhichWrap),
        "visualstar" => Some(OptionId::VisualStar),
        "undoautogroupms" | "uagm" => Some(OptionId::UndoAutoGroupMs),
        "belloff" | "bo" => Some(OptionId::BellOff),
        "softtabstop" | "sts" => Some(OptionId::SoftTabStop),
        _ => None,
    }
}

/// Execute `&` (repeat substitution on current line) or `g&` (repeat globally).
///
/// Builds an `ExCommand::RepeatSubstitute` and dispatches it through the
/// normal ex command pipeline. Returns `Effects` for the executor.
pub(crate) fn execute_repeat_substitute<D: Document>(
    ctx: &ExecutionContext<'_, D>,
    use_previous_flags: bool,
) -> Effects {
    let command = ExCommand::RepeatSubstitute {
        range: if use_previous_flags {
            // g& applies to all lines: 1,$
            Some(ExRange::entire_file())
        } else {
            // & applies to current line only
            None
        },
        use_previous_flags,
    };
    let mut sequencer = HostRequestSequencer::default();
    match execute_ex_command(&command, ctx, &mut sequencer) {
        Ok(output) => output.effects,
        Err(e) => Effects::new().show_error(e),
    }
}

/// Apply `:set` assignments to engine options, routing writes through the
/// correct scope layers.
///
/// # Scope routing
///
/// | [`SetScope`]  | Global option | LocalToBuffer/Window | GlobalOrLocal* |
/// |---------------|---------------|----------------------|----------------|
/// | `Effective`   | global only   | global + local        | global + local |
/// | `Local`       | global only   | local only            | local only     |
/// | `Global`      | global only   | global only           | global only    |
///
/// Returns `ShowMessage` / error effects from query / unknown-option assignments.
pub(crate) fn apply_set_assignments(
    scope: SetScope,
    global: &mut crate::primitives::VimOptions,
    buffer_overrides: &mut OptionOverrides,
    window_overrides: &mut OptionOverrides,
    assignments: &[crate::grammar::types::SetAssignment],
) -> Effects {
    use crate::grammar::types::SetAssignment;

    let mut messages = Vec::new();
    let mut error_effects = Effects::new();

    for assignment in assignments {
        match assignment {
            SetAssignment::ShowAll => {
                messages.push(format_all_options(global));
            }
            SetAssignment::Query(name) => {
                if let Some(val) = query_option(global, name) {
                    messages.push(val);
                } else {
                    error_effects.extend(Effects::new().show_error(VimError::NotEditorCommand(
                        format!("Unknown option: {name}").into(),
                    )));
                }
            }
            SetAssignment::SetBool(name) => {
                // The parser now emits UnsetBool for "no"-prefixed names, so
                // SetBool always means "turn on". Legacy fallback retained for
                // safety: if a SetBool("noX") somehow arrives, resolve it here.
                let (canonical, value) = if let Some(stripped) = name.strip_prefix("no") {
                    if is_known_bool_option(stripped) {
                        (stripped, false)
                    } else {
                        (name.as_str(), true)
                    }
                } else {
                    (name.as_str(), true)
                };

                if let Some(id) = option_name_to_id(canonical) {
                    apply_bool_with_scope(
                        scope,
                        id,
                        value,
                        global,
                        buffer_overrides,
                        window_overrides,
                    );
                } else {
                    error_effects.extend(set_bool_option(global, canonical, value));
                }
            }
            SetAssignment::UnsetBool(name) => {
                if let Some(id) = option_name_to_id(name) {
                    apply_bool_with_scope(
                        scope,
                        id,
                        false,
                        global,
                        buffer_overrides,
                        window_overrides,
                    );
                } else {
                    error_effects.extend(set_bool_option(global, name, false));
                }
            }
            SetAssignment::ToggleBool(name) => {
                if let Some(id) = option_name_to_id(name) {
                    // Read the *effective* value (not just global) so that
                    // toggling respects local overrides.
                    let current = match crate::primitives::resolve_option(
                        id,
                        global,
                        Some(buffer_overrides),
                        Some(window_overrides),
                    ) {
                        OptionValue::Bool(v) => v,
                        _ => false,
                    };
                    apply_bool_with_scope(
                        scope,
                        id,
                        !current,
                        global,
                        buffer_overrides,
                        window_overrides,
                    );
                } else {
                    error_effects.extend(toggle_bool_option(global, name));
                }
            }
            SetAssignment::Assign(name, value) => {
                if let Some(id) = option_name_to_id(name) {
                    // Build the OptionValue by parsing through the existing helper on a temp
                    // copy, then extract the result. Simpler: parse directly.
                    let opt_value = parse_option_value(id, name, value, &mut error_effects);
                    if let Some(opt_value) = opt_value {
                        apply_value_with_scope(
                            scope,
                            id,
                            opt_value,
                            global,
                            buffer_overrides,
                            window_overrides,
                        );
                    }
                } else {
                    error_effects.extend(assign_option(global, name, value));
                }
            }
        }
    }

    let mut effects = if messages.is_empty() {
        Effects::new()
    } else {
        ex_effects::show_message(messages.join("\n").into())
    };
    effects.extend(error_effects);
    effects
}

/// Parse a string value into an [`OptionValue`] for the given option id.
///
/// Emits error effects for parse failures. Returns `None` on failure.
fn parse_option_value(
    id: OptionId,
    name: &str,
    value: &str,
    error_effects: &mut Effects,
) -> Option<OptionValue> {
    // Determine the value type from the global default.
    let dummy = crate::primitives::VimOptions::default();
    match dummy.get_option(id) {
        OptionValue::Bool(_) => {
            // Bool options shouldn't arrive as Assign, but handle gracefully.
            match value {
                "1" | "true" | "on" => Some(OptionValue::Bool(true)),
                "0" | "false" | "off" => Some(OptionValue::Bool(false)),
                _ => {
                    error_effects.extend(Effects::new().show_error(VimError::NotEditorCommand(
                        format!("Invalid boolean value for {name}: {value}").into(),
                    )));
                    None
                }
            }
        }
        OptionValue::Unsigned(_) => {
            if let Ok(v) = value.parse::<usize>() {
                Some(OptionValue::Unsigned(v))
            } else {
                error_effects.extend(Effects::new().show_error(VimError::NotEditorCommand(
                    format!("E521: Number required after =: {name}={value}").into(),
                )));
                None
            }
        }
        OptionValue::Signed(_) => {
            // undolevels: -1 means unlimited
            if let Ok(v) = value.parse::<i64>() {
                Some(OptionValue::Signed(v))
            } else {
                error_effects.extend(Effects::new().show_error(VimError::NotEditorCommand(
                    format!("E521: Number required after =: {name}={value}").into(),
                )));
                None
            }
        }
        OptionValue::Str(_) => Some(OptionValue::Str(CompactString::from(value))),
    }
}

/// Write a boolean option value through the correct scope layers.
fn apply_bool_with_scope(
    scope: SetScope,
    id: OptionId,
    value: bool,
    global: &mut crate::primitives::VimOptions,
    buffer_overrides: &mut OptionOverrides,
    window_overrides: &mut OptionOverrides,
) {
    let opt_value = OptionValue::Bool(value);
    apply_value_with_scope(
        scope,
        id,
        opt_value,
        global,
        buffer_overrides,
        window_overrides,
    );
}

/// Write an option value through the correct scope layers.
fn apply_value_with_scope(
    scope: SetScope,
    id: OptionId,
    value: OptionValue,
    global: &mut crate::primitives::VimOptions,
    buffer_overrides: &mut OptionOverrides,
    window_overrides: &mut OptionOverrides,
) {
    use crate::primitives::OptionScope;

    let opt_scope = id.scope();

    match scope {
        SetScope::Global => {
            // Always write to global only; never touch overrides.
            global.set_option(id, &value);
        }
        SetScope::Local => {
            match opt_scope {
                OptionScope::Global => {
                    // Global-scoped options: `:setlocal` behaves like `:set`
                    global.set_option(id, &value);
                }
                OptionScope::LocalToBuffer | OptionScope::GlobalOrLocalBuffer => {
                    // Write only the buffer override.
                    buffer_overrides.set(id, value);
                }
                OptionScope::LocalToWindow | OptionScope::GlobalOrLocalWindow => {
                    // Write only the window override.
                    window_overrides.set(id, value);
                }
            }
        }
        SetScope::Effective => {
            match opt_scope {
                OptionScope::Global => {
                    // Global-scoped options: write global only.
                    global.set_option(id, &value);
                }
                OptionScope::LocalToBuffer => {
                    // Write global AND buffer override.
                    global.set_option(id, &value);
                    buffer_overrides.set(id, value);
                }
                OptionScope::GlobalOrLocalBuffer => {
                    // Write global AND buffer override.
                    global.set_option(id, &value);
                    buffer_overrides.set(id, value);
                }
                OptionScope::LocalToWindow => {
                    // Write global AND window override.
                    global.set_option(id, &value);
                    window_overrides.set(id, value);
                }
                OptionScope::GlobalOrLocalWindow => {
                    // Write global AND window override.
                    global.set_option(id, &value);
                    window_overrides.set(id, value);
                }
            }
        }
    }
}

fn query_option(options: &crate::primitives::VimOptions, name: &str) -> Option<String> {
    Some(match name {
        "tabstop" | "ts" => format!("tabstop={}", options.tabstop()),
        "shiftwidth" | "sw" => format!("shiftwidth={}", options.shiftwidth()),
        "expandtab" | "et" => format_bool("expandtab", options.expandtab()),
        "autoindent" | "ai" => format_bool("autoindent", options.autoindent()),
        "smartindent" | "si" => format_bool("smartindent", options.smartindent()),
        "ignorecase" | "ic" => format_bool("ignorecase", options.ignorecase()),
        "smartcase" | "scs" => format_bool("smartcase", options.smartcase()),
        "hlsearch" | "hls" => format_bool("hlsearch", options.hlsearch()),
        "incsearch" | "is" => format_bool("incsearch", options.incsearch()),
        "wrapscan" | "ws" => format_bool("wrapscan", options.wrapscan()),
        "scrolloff" | "so" => format!("scrolloff={}", options.scrolloff()),
        "sidescrolloff" | "siso" => format!("sidescrolloff={}", options.sidescrolloff()),
        "number" | "nu" => format_bool("number", options.number()),
        "relativenumber" | "rnu" => format_bool("relativenumber", options.relativenumber()),
        "textwidth" | "tw" => format!("textwidth={}", options.textwidth()),
        "timeoutlen" | "tm" => format!("timeoutlen={}", options.timeoutlen_ms()),
        "undolevels" | "ul" => match options.undolevels() {
            Some(n) => format!("undolevels={n}"),
            None => "undolevels=-1".to_owned(),
        },
        "whichwrap" | "ww" => format!("whichwrap={}", options.whichwrap()),
        "backspace" | "bs" => format!("backspace={}", options.backspace()),
        "virtualedit" | "ve" => format!("virtualedit={}", options.virtualedit()),
        "selection" | "sel" => format!("selection={}", options.selection()),
        "clipboard" | "cb" => format!("clipboard={}", options.clipboard()),
        "iskeyword" | "isk" => format!("iskeyword={}", options.iskeyword()),
        "inccommand" | "icm" => format!("inccommand={}", options.inccommand()),
        "gdefault" | "gd" => format_bool("gdefault", options.gdefault()),
        "visualstar" => format_bool("visualstar", options.visualstar()),
        "langmap" | "lmap" => format!("langmap={}", options.langmap()),
        "langremap" | "lrm" => format_bool("langremap", options.langremap()),
        "undoautogroupms" | "uagm" => match options.undo_auto_group_ms() {
            Some(ms) => format!("undoautogroupms={ms}"),
            None => "undoautogroupms=-1".to_owned(),
        },
        "multilinefind" | "mlf" => format_bool("multilinefind", options.multiline_find()),
        "multilinefindrange" | "mlfr" => {
            format!("multilinefindrange={}", options.multiline_find_range())
        }
        "belloff" | "bo" => {
            if options.belloff() {
                "belloff=all".to_owned()
            } else {
                "belloff=".to_owned()
            }
        }
        "softtabstop" | "sts" => format!("softtabstop={}", options.softtabstop()),
        _ => return None,
    })
}

fn format_bool(name: &str, value: bool) -> String {
    if value {
        name.to_owned()
    } else {
        format!("no{name}")
    }
}

fn is_known_bool_option(name: &str) -> bool {
    matches!(
        name,
        "expandtab"
            | "et"
            | "autoindent"
            | "ai"
            | "smartindent"
            | "si"
            | "ignorecase"
            | "ic"
            | "smartcase"
            | "scs"
            | "hlsearch"
            | "hls"
            | "incsearch"
            | "is"
            | "wrapscan"
            | "ws"
            | "number"
            | "nu"
            | "relativenumber"
            | "rnu"
            | "gdefault"
            | "gd"
            | "visualstar"
            | "langremap"
            | "lrm"
            | "multilinefind"
            | "mlf"
    )
}

fn set_bool_option(
    options: &mut crate::primitives::VimOptions,
    name: &str,
    value: bool,
) -> Effects {
    match name {
        "expandtab" | "et" => {
            options.set_expandtab(value);
            Effects::new()
        }
        "autoindent" | "ai" => {
            options.set_autoindent(value);
            Effects::new()
        }
        "smartindent" | "si" => {
            options.set_smartindent(value);
            Effects::new()
        }
        "ignorecase" | "ic" => {
            options.set_ignorecase(value);
            Effects::new()
        }
        "smartcase" | "scs" => {
            options.set_smartcase(value);
            Effects::new()
        }
        "hlsearch" | "hls" => {
            options.set_hlsearch(value);
            Effects::new()
        }
        "incsearch" | "is" => {
            options.set_incsearch(value);
            Effects::new()
        }
        "wrapscan" | "ws" => {
            options.set_wrapscan(value);
            Effects::new()
        }
        "number" | "nu" => {
            options.set_number(value);
            Effects::new()
        }
        "relativenumber" | "rnu" => {
            options.set_relativenumber(value);
            Effects::new()
        }
        "gdefault" | "gd" => {
            options.set_gdefault(value);
            Effects::new()
        }
        "visualstar" => {
            options.set_visualstar(value);
            Effects::new()
        }
        "langremap" | "lrm" => {
            options.set_langremap(value);
            Effects::new()
        }
        "multilinefind" | "mlf" => {
            options.set_multiline_find(value);
            Effects::new()
        }
        _ => Effects::new().show_error(VimError::NotEditorCommand(
            format!("Unknown option: {name}").into(),
        )),
    }
}

fn toggle_bool_option(options: &mut crate::primitives::VimOptions, name: &str) -> Effects {
    let current = match name {
        "expandtab" | "et" => Some(options.expandtab()),
        "autoindent" | "ai" => Some(options.autoindent()),
        "smartindent" | "si" => Some(options.smartindent()),
        "ignorecase" | "ic" => Some(options.ignorecase()),
        "smartcase" | "scs" => Some(options.smartcase()),
        "hlsearch" | "hls" => Some(options.hlsearch()),
        "incsearch" | "is" => Some(options.incsearch()),
        "wrapscan" | "ws" => Some(options.wrapscan()),
        "number" | "nu" => Some(options.number()),
        "relativenumber" | "rnu" => Some(options.relativenumber()),
        "gdefault" | "gd" => Some(options.gdefault()),
        "visualstar" => Some(options.visualstar()),
        "langremap" | "lrm" => Some(options.langremap()),
        "multilinefind" | "mlf" => Some(options.multiline_find()),
        _ => None,
    };
    match current {
        Some(v) => set_bool_option(options, name, !v),
        None => Effects::new().show_error(VimError::NotEditorCommand(
            format!("Unknown option: {name}").into(),
        )),
    }
}

fn number_required_error(name: &str, value: &str) -> Effects {
    Effects::new().show_error(VimError::NotEditorCommand(
        format!("E521: Number required after =: {name}={value}").into(),
    ))
}

fn assign_option(options: &mut crate::primitives::VimOptions, name: &str, value: &str) -> Effects {
    /// Set a numeric (`usize`) option, returning error effects on parse failure.
    macro_rules! set_num {
        ($setter:ident) => {
            match value.parse::<usize>() {
                Ok(v) => {
                    options.$setter(v);
                    Effects::new()
                }
                Err(_) => number_required_error(name, value),
            }
        };
    }
    match name {
        "tabstop" | "ts" => set_num!(set_tabstop),
        "shiftwidth" | "sw" => set_num!(set_shiftwidth),
        "scrolloff" | "so" => set_num!(set_scrolloff),
        "sidescrolloff" | "siso" => set_num!(set_sidescrolloff),
        "textwidth" | "tw" => set_num!(set_textwidth),
        "timeoutlen" | "tm" => match value.parse::<u32>() {
            Ok(v) => {
                options.set_timeoutlen_ms(v);
                Effects::new()
            }
            Err(_) => number_required_error(name, value),
        },
        "undolevels" | "ul" => {
            // Vim uses -1 to mean "unlimited". We map negative → None, >=0 → Some(n).
            match value.parse::<i64>() {
                Ok(v) if v < 0 => {
                    options.set_undolevels(None);
                    Effects::new()
                }
                Ok(v) => {
                    options.set_undolevels(Some(usize::try_from(v).unwrap_or(usize::MAX)));
                    Effects::new()
                }
                Err(_) => number_required_error(name, value),
            }
        }
        "undoautogroupms" | "uagm" => match value.parse::<i64>() {
            Ok(v) if v < 0 => {
                options.set_undo_auto_group_ms(None);
                Effects::new()
            }
            Ok(v) => {
                let ms = u32::try_from(v).unwrap_or(u32::MAX);
                options.set_undo_auto_group_ms(Some(ms));
                Effects::new()
            }
            Err(_) => number_required_error(name, value),
        },
        "whichwrap" | "ww" => {
            options.set_whichwrap(value);
            Effects::new()
        }
        "backspace" | "bs" => {
            options.set_backspace(value);
            Effects::new()
        }
        "virtualedit" | "ve" => {
            options.set_virtualedit(value);
            Effects::new()
        }
        "selection" | "sel" => {
            options.set_selection(value);
            Effects::new()
        }
        "clipboard" | "cb" => {
            options.set_clipboard(value);
            Effects::new()
        }
        "iskeyword" | "isk" => {
            options.set_iskeyword(value);
            Effects::new()
        }
        "inccommand" | "icm" => {
            options.set_inccommand(value);
            Effects::new()
        }
        "langmap" | "lmap" => {
            options.set_langmap(value);
            Effects::new()
        }
        "multilinefindrange" | "mlfr" => set_num!(set_multiline_find_range),
        "softtabstop" | "sts" => match value.parse::<i32>() {
            Ok(v) => {
                options.set_softtabstop(v);
                Effects::new()
            }
            Err(_) => number_required_error(name, value),
        },
        "belloff" | "bo" => match value {
            "" => {
                options.set_belloff(false);
                Effects::new()
            }
            "all" => {
                options.set_belloff(true);
                Effects::new()
            }
            _ => Effects::new().show_error(VimError::NotEditorCommand(
                format!("E474: Invalid argument: belloff={value}").into(),
            )),
        },
        _ => Effects::new().show_error(VimError::NotEditorCommand(
            format!("Unknown option: {name}").into(),
        )),
    }
}

fn format_all_options(options: &crate::primitives::VimOptions) -> String {
    let undolevels_str = match options.undolevels() {
        Some(n) => format!("undolevels={n}"),
        None => "undolevels=-1".to_owned(),
    };
    let langmap_str = format!("langmap={}", options.langmap());
    format!(
        "tabstop={} shiftwidth={} softtabstop={} {} {} {} {} {} {} {} {} {} scrolloff={} sidescrolloff={} {} {} textwidth={} timeoutlen={} {} {} {} {} {} multilinefindrange={} belloff={}",
        options.tabstop(),
        options.shiftwidth(),
        options.softtabstop(),
        format_bool("expandtab", options.expandtab()),
        format_bool("autoindent", options.autoindent()),
        format_bool("smartindent", options.smartindent()),
        format_bool("ignorecase", options.ignorecase()),
        format_bool("smartcase", options.smartcase()),
        format_bool("hlsearch", options.hlsearch()),
        format_bool("incsearch", options.incsearch()),
        format_bool("wrapscan", options.wrapscan()),
        format_bool("visualstar", options.visualstar()),
        options.scrolloff(),
        options.sidescrolloff(),
        format_bool("number", options.number()),
        format_bool("relativenumber", options.relativenumber()),
        options.textwidth(),
        options.timeoutlen_ms(),
        format_bool("gdefault", options.gdefault()),
        undolevels_str,
        langmap_str,
        format_bool("langremap", options.langremap()),
        format_bool("multilinefind", options.multiline_find()),
        options.multiline_find_range(),
        if options.belloff() { "all" } else { "" },
    )
}

fn format_ex_range(range: &ExRange) -> String {
    match &range.end {
        Some(end) => format!(
            "{},{}",
            format_line_spec(&range.start),
            format_line_spec(end)
        ),
        None => format_line_spec(&range.start),
    }
}

fn format_line_spec(spec: &LineSpec) -> String {
    match spec {
        LineSpec::Current => ".".to_owned(),
        LineSpec::Last => "$".to_owned(),
        LineSpec::Absolute(line) => line.to_string(),
        LineSpec::Mark(mark) => format!("'{mark}"),
        LineSpec::SearchForward(pattern) => format!("/{pattern}/"),
        LineSpec::SearchBackward(pattern) => format!("?{pattern}?"),
        LineSpec::Relative(0) => ".".to_owned(),
        LineSpec::Relative(offset) if *offset > 0 => format!("+{offset}"),
        LineSpec::Relative(offset) => offset.to_string(),
        LineSpec::WithOffset { base, offset } => {
            let base_str = format_line_spec(base);
            if *offset >= 0 {
                format!("{base_str}+{offset}")
            } else {
                format!("{base_str}{offset}")
            }
        }
    }
}

/// Build read-file completion effects from host result payload.
pub(crate) fn complete_read_file(offset: Option<usize>, data: &CompactString) -> Effects {
    ex_effects::read_file_completion(offset.unwrap_or(0), data.clone())
}

/// Build a response message from host completion.
pub(crate) fn completion_message(
    result: &HostResult,
    _request: &HostRequestMeta,
) -> Option<String> {
    match result {
        HostResult::Success {
            message: Some(message),
            ..
        } => Some(message.to_string()),
        HostResult::Success { .. } => None,
        HostResult::Failure { error, .. } => Some(error.to_string()),
        HostResult::Data { .. }
        | HostResult::ClipboardText { .. }
        | HostResult::CmdlineCompletionCandidates { .. } => None,
        HostResult::FilteredRange { stderr, .. } => stderr.as_ref().map(ToString::to_string),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Informational ex command formatting
// ─────────────────────────────────────────────────────────────────────────────

/// Convert byte offset to (1-based line, 0-based col).
fn offset_to_line_col(text: &str, offset: usize) -> (usize, usize) {
    use crate::commands::helpers::{line_of, line_start_for_offset};
    let line = line_of(text, offset);
    let ls = line_start_for_offset(text, offset);
    (line + 1, offset.saturating_sub(ls))
}

/// Maximum number of characters shown in a line snippet for `:marks`/`:jumps`/`:changes`.
const LINE_SNIPPET_MAX_CHARS: usize = 50;

/// Snippet of the line at `offset` for display (truncated to `LINE_SNIPPET_MAX_CHARS` chars).
fn line_snippet(text: &str, offset: usize) -> &str {
    use crate::commands::helpers::line_start_for_offset;
    let ls = line_start_for_offset(text, offset);
    let rest = &text[ls..];
    let end = rest.find('\n').unwrap_or(rest.len());
    let line = &rest[..end];
    match line.char_indices().nth(LINE_SNIPPET_MAX_CHARS) {
        Some((byte_pos, _)) => &line[..byte_pos],
        None => line,
    }
}

fn format_registers(regs: &crate::state::Registers, filter: Option<&str>) -> String {
    use crate::primitives::RegisterName;
    use std::fmt::Write;

    // Order: ", 0, 1-9, -, a-z, /, +, *, ., :
    const REGISTER_ORDER: &[char] = &[
        '"', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-', 'a', 'b', 'c', 'd', 'e', 'f',
        'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x',
        'y', 'z', '/', '+', '*', '.', ':', '=',
    ];

    let mut out = String::from("--- Registers ---\n");
    for &c in REGISTER_ORDER {
        if let Some(f) = filter {
            if !f.contains(c) {
                continue;
            }
        }
        let name = RegisterName::new_unchecked(c);
        if let Some(content) = regs.get(name) {
            let type_char = match content.motion_type() {
                crate::primitives::MotionType::LineWise => 'l',
                crate::primitives::MotionType::BlockWise => 'b',
                crate::primitives::MotionType::CharWise => 'c',
            };
            let text = content.text().replace('\n', "^J");
            let _ = writeln!(out, "\"{c}   {type_char}  {text}");
        }
    }
    out
}

fn format_marks(marks: &crate::state::Marks, text: &str, filter: Option<&str>) -> String {
    use crate::primitives::MarkName;
    use std::fmt::Write;

    const MARK_ORDER: &[char] = &[
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J',
        'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '\'', '`',
        '.', '^', '[', ']', '<', '>',
    ];

    let mut out = String::from("mark line  col\n");
    for &c in MARK_ORDER {
        if let Some(f) = filter {
            if !f.contains(c) {
                continue;
            }
        }
        let Some(name) = MarkName::new(c) else {
            continue;
        };
        if let Some(mark) = marks.get(name) {
            let off = mark.offset().get();
            let (line, col) = offset_to_line_col(text, off);
            let snippet = line_snippet(text, off);
            let _ = writeln!(out, " {c:>1}  {line:>4} {col:>4} {snippet}");
        }
    }
    out
}

fn format_jumps(jump_list: &crate::state::JumpList, text: &str) -> String {
    use std::fmt::Write;

    let entries = jump_list.entries();
    let current = jump_list.position();
    let mut out = String::from(" jump line  col\n");
    for (i, entry) in entries.iter().enumerate() {
        let off = entry.offset().get();
        let (line, col) = offset_to_line_col(text, off);
        let snippet = line_snippet(text, off);
        let distance = current.abs_diff(i);
        let marker = if i + 1 == current { ">" } else { " " };
        let _ = writeln!(out, "{marker}{distance:>3} {line:>4} {col:>4} {snippet}");
    }
    if current == entries.len() {
        out.push_str(">\n");
    }
    out
}

fn format_changes(changelist: &crate::state::ChangeList, text: &str) -> String {
    use std::fmt::Write;

    let entries = changelist.entries();
    let current = changelist.position();
    let mut out = String::from("change line  col\n");
    for (i, entry) in entries.iter().enumerate() {
        let off = entry[0].get();
        let (line, col) = offset_to_line_col(text, off);
        let snippet = line_snippet(text, off);
        let distance = current.abs_diff(i);
        let marker = if i + 1 == current { ">" } else { " " };
        let _ = writeln!(out, "{marker}{distance:>3} {line:>4} {col:>4} {snippet}");
    }
    if current == entries.len() {
        out.push_str(">\n");
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Mapping command execution
// ─────────────────────────────────────────────────────────────────────────────

/// Re-export key notation parser from its dedicated module.
pub(crate) use super::key_notation::parse_key_notation_sequence as parse_key_notation_sequence_with_keymap;

pub(crate) fn modes_for_prefix(
    prefix: crate::grammar::types::MapModePrefix,
) -> SmallVec<[crate::keymap::MappingMode; 4]> {
    use crate::grammar::types::MapModePrefix;
    use crate::keymap::MappingMode;
    match prefix {
        MapModePrefix::All => smallvec::smallvec![
            MappingMode::Normal,
            MappingMode::Visual,
            MappingMode::Operator,
        ],
        MapModePrefix::Normal => smallvec::smallvec![MappingMode::Normal],
        MapModePrefix::Visual => smallvec::smallvec![MappingMode::Visual],
        MapModePrefix::Insert => smallvec::smallvec![MappingMode::Insert],
        MapModePrefix::Operator => smallvec::smallvec![MappingMode::Operator],
        MapModePrefix::Command => smallvec::smallvec![MappingMode::Command],
        MapModePrefix::VisualOnly => smallvec::smallvec![MappingMode::VisualOnly],
        MapModePrefix::SelectOnly => smallvec::smallvec![MappingMode::SelectOnly],
    }
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match execute_ex_command dispatch"
)]
fn execute_map_command(
    mode_prefix: crate::grammar::types::MapModePrefix,
    lhs: &str,
    rhs: Option<&str>,
    kind: crate::keymap::MappingKind,
    flags: crate::keymap::MappingFlags,
) -> Result<ExExecutionOutput, VimError> {
    // `:map` or `:map lhs` without rhs — list mappings (not yet implemented)
    let Some(rhs) = rhs else {
        return effects_only(Ok(Effects::new()));
    };

    if lhs.is_empty() {
        return effects_only(Ok(Effects::new()));
    }

    let modes = modes_for_prefix(mode_prefix);

    Ok(ExExecutionOutput {
        mapping_changes: smallvec::smallvec![MappingChange::Define {
            modes,
            lhs_raw: CompactString::from(lhs),
            rhs_raw: CompactString::from(rhs),
            kind,
            flags,
        }],
        ..ExExecutionOutput::default()
    })
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match execute_ex_command dispatch"
)]
fn execute_unmap_command(
    mode_prefix: crate::grammar::types::MapModePrefix,
    lhs: &str,
) -> Result<ExExecutionOutput, VimError> {
    let modes = modes_for_prefix(mode_prefix);

    Ok(ExExecutionOutput {
        mapping_changes: smallvec::smallvec![MappingChange::Remove {
            modes,
            lhs_raw: CompactString::from(lhs),
        }],
        ..ExExecutionOutput::default()
    })
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match execute_ex_command dispatch"
)]
fn execute_mapclear_command(
    mode_prefix: crate::grammar::types::MapModePrefix,
) -> Result<ExExecutionOutput, VimError> {
    let modes = modes_for_prefix(mode_prefix);

    Ok(ExExecutionOutput {
        mapping_changes: smallvec::smallvec![MappingChange::ClearAll { modes }],
        ..ExExecutionOutput::default()
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Sethandler command execution
// ─────────────────────────────────────────────────────────────────────────────

/// Execute a `:sethandler` command.
///
/// Parses the key notation (if present) and mode:handler assignments into
/// `HandlerChange` entries that the engine applies to its `HandlerMap`.
/// Execute a `:sethandler` command from already-parsed fields.
///
/// Public within the crate so `source.rs` can call it directly without
/// needing a full `ExecutionContext`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match execute_ex_command dispatch"
)]
pub(crate) fn execute_sethandler(
    key_notation: Option<&str>,
    assignments: &[(CompactString, CompactString)],
) -> Result<ExExecutionOutput, VimError> {
    // Parse the key notation into a KeyEvent if provided.
    let key_event = match key_notation {
        Some(notation) => {
            let ke = KeyEvent::from_vim_notation(notation).ok_or_else(|| {
                VimError::InvalidArgument(CompactString::from(format!(
                    "Invalid key notation: {notation}"
                )))
            })?;
            Some(ke)
        }
        None => None,
    };

    let mut handler_changes = SmallVec::new();

    for (mode_chars, handler_name) in assignments {
        // Parse handler: "vim" → Vim, "ide"/"host" → Host
        let handler = match handler_name.as_str() {
            "vim" => Handler::Vim,
            "ide" | "host" => Handler::Host,
            _ => {
                return Err(VimError::InvalidArgument(CompactString::from(format!(
                    "Unknown handler: {handler_name} (expected vim, ide, or host)"
                ))));
            }
        };

        // Parse mode characters: split by '-', map each to MappingMode
        let modes = parse_sethandler_modes(mode_chars)?;

        handler_changes.push(HandlerChange {
            key: key_event,
            modes,
            handler,
        });
    }

    Ok(ExExecutionOutput {
        handler_changes,
        ..ExExecutionOutput::default()
    })
}

/// Parse a mode-chars string like `"n"`, `"n-v"`, `"a"` into a list of `MappingMode`s.
fn parse_sethandler_modes(mode_chars: &str) -> Result<SmallVec<[MappingMode; 4]>, VimError> {
    let mut modes = SmallVec::new();
    for part in mode_chars.split('-') {
        for ch in part.chars() {
            match ch {
                'n' => {
                    if !modes.contains(&MappingMode::Normal) {
                        modes.push(MappingMode::Normal);
                    }
                }
                'i' => {
                    if !modes.contains(&MappingMode::Insert) {
                        modes.push(MappingMode::Insert);
                    }
                }
                'v' => {
                    // 'v' maps to Visual (which covers Visual + Select in IdeaVim)
                    if !modes.contains(&MappingMode::Visual) {
                        modes.push(MappingMode::Visual);
                    }
                }
                'x' => {
                    // 'x' is Visual-only (same MappingMode in our system)
                    if !modes.contains(&MappingMode::Visual) {
                        modes.push(MappingMode::Visual);
                    }
                }
                'o' => {
                    if !modes.contains(&MappingMode::Operator) {
                        modes.push(MappingMode::Operator);
                    }
                }
                'a' => {
                    // 'a' means all modes
                    modes.clear();
                    modes.extend_from_slice(&MappingMode::ALL);
                    return Ok(modes);
                }
                _ => {
                    return Err(VimError::InvalidArgument(CompactString::from(format!(
                        "Unknown mode character: '{ch}' (expected n, i, v, x, o, or a)"
                    ))));
                }
            }
        }
    }

    if modes.is_empty() {
        return Err(VimError::InvalidArgument(CompactString::from(
            "No mode characters specified",
        )));
    }

    Ok(modes)
}

/// Build completion effects for a data-bearing host result.
///
/// # Panics (debug only)
///
/// Debug-asserts if the completed request type does not accept data,
/// indicating a host protocol violation.
pub(crate) fn completion_effects_for_data(
    request: &HostRequest,
    data: &CompactString,
    offset: Option<usize>,
) -> Effects {
    match request {
        HostRequest::ReadFile { .. } => complete_read_file(offset, data),
        HostRequest::ExternalCommand { .. }
        | HostRequest::CustomExCommand { .. }
        | HostRequest::FilterDocumentRange { .. }
        | HostRequest::ReindentRange { .. }
        | HostRequest::ListActions { .. }
        | HostRequest::ReadConfigFile { .. }
        | HostRequest::EvaluateExpression { .. } => ex_effects::show_message(data.clone()),
        other => {
            debug_assert!(false, "host returned data for non-data request: {other:?}");
            Effects::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════
    // resolve_width
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn resolve_width_explicit_value() {
        assert_eq!(resolve_width(Some(42), 80), 42);
    }

    #[test]
    fn resolve_width_no_explicit_uses_textwidth() {
        assert_eq!(resolve_width(None, 120), 120);
    }

    #[test]
    fn resolve_width_no_explicit_zero_textwidth_uses_default() {
        assert_eq!(resolve_width(None, 0), DEFAULT_TEXT_WIDTH);
    }

    #[test]
    fn resolve_width_default_is_80() {
        assert_eq!(DEFAULT_TEXT_WIDTH, 80);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // format_bool
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn format_bool_true_returns_name() {
        assert_eq!(format_bool("hlsearch", true), "hlsearch");
    }

    #[test]
    fn format_bool_false_returns_no_prefix() {
        assert_eq!(format_bool("hlsearch", false), "nohlsearch");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // is_known_bool_option
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn known_bool_option_expandtab() {
        assert!(is_known_bool_option("expandtab"));
    }

    #[test]
    fn known_bool_option_short_form_et() {
        assert!(is_known_bool_option("et"));
    }

    #[test]
    fn known_bool_option_ignorecase() {
        assert!(is_known_bool_option("ignorecase"));
    }

    #[test]
    fn known_bool_option_short_form_ic() {
        assert!(is_known_bool_option("ic"));
    }

    #[test]
    fn unknown_bool_option_tabstop() {
        // tabstop is numeric, not boolean
        assert!(!is_known_bool_option("tabstop"));
    }

    #[test]
    fn unknown_bool_option_shiftwidth() {
        assert!(!is_known_bool_option("shiftwidth"));
    }

    #[test]
    fn unknown_bool_option_arbitrary() {
        assert!(!is_known_bool_option("nonexistent"));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // option_name_to_id
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn option_name_ignorecase_long() {
        assert_eq!(option_name_to_id("ignorecase"), Some(OptionId::IgnoreCase));
    }

    #[test]
    fn option_name_ignorecase_short() {
        assert_eq!(option_name_to_id("ic"), Some(OptionId::IgnoreCase));
    }

    #[test]
    fn option_name_tabstop_long() {
        assert_eq!(option_name_to_id("tabstop"), Some(OptionId::TabStop));
    }

    #[test]
    fn option_name_tabstop_short() {
        assert_eq!(option_name_to_id("ts"), Some(OptionId::TabStop));
    }

    #[test]
    fn option_name_unknown() {
        assert_eq!(option_name_to_id("nonexistent"), None);
    }

    #[test]
    fn option_name_all_long_forms_resolve() {
        // Exhaustively verify all long-form option names in the match
        let expected = [
            ("ignorecase", OptionId::IgnoreCase),
            ("smartcase", OptionId::SmartCase),
            ("hlsearch", OptionId::HlSearch),
            ("incsearch", OptionId::IncSearch),
            ("wrapscan", OptionId::WrapScan),
            ("gdefault", OptionId::GDefault),
            ("clipboard", OptionId::Clipboard),
            ("inccommand", OptionId::IncCommand),
            ("timeoutlen", OptionId::TimeoutLen),
            ("undolevels", OptionId::UndoLevels),
            ("tabstop", OptionId::TabStop),
            ("shiftwidth", OptionId::ShiftWidth),
            ("expandtab", OptionId::ExpandTab),
            ("autoindent", OptionId::AutoIndent),
            ("smartindent", OptionId::SmartIndent),
            ("iskeyword", OptionId::IsKeyword),
            ("textwidth", OptionId::TextWidth),
            ("scrolloff", OptionId::ScrollOff),
            ("number", OptionId::Number),
            ("relativenumber", OptionId::RelativeNumber),
            ("sidescrolloff", OptionId::SideScrollOff),
            ("virtualedit", OptionId::VirtualEdit),
            ("selection", OptionId::Selection),
            ("backspace", OptionId::Backspace),
            ("whichwrap", OptionId::WhichWrap),
            ("visualstar", OptionId::VisualStar),
            ("commentstring", OptionId::CommentString),
            ("belloff", OptionId::BellOff),
            ("softtabstop", OptionId::SoftTabStop),
        ];
        for (name, expected_id) in expected {
            assert_eq!(
                option_name_to_id(name),
                Some(expected_id),
                "failed for option: {name}"
            );
        }
    }

    #[test]
    fn option_name_softtabstop_long() {
        assert_eq!(
            option_name_to_id("softtabstop"),
            Some(OptionId::SoftTabStop)
        );
    }

    #[test]
    fn option_name_softtabstop_short() {
        assert_eq!(option_name_to_id("sts"), Some(OptionId::SoftTabStop));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // parse_sethandler_modes
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parse_modes_single_normal() {
        let modes = parse_sethandler_modes("n").unwrap();
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0], MappingMode::Normal);
    }

    #[test]
    fn parse_modes_single_insert() {
        let modes = parse_sethandler_modes("i").unwrap();
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0], MappingMode::Insert);
    }

    #[test]
    fn parse_modes_combined_n_v() {
        let modes = parse_sethandler_modes("n-v").unwrap();
        assert_eq!(modes.len(), 2);
        assert!(modes.contains(&MappingMode::Normal));
        assert!(modes.contains(&MappingMode::Visual));
    }

    #[test]
    fn parse_modes_all_with_a() {
        let modes = parse_sethandler_modes("a").unwrap();
        assert_eq!(modes.len(), MappingMode::ALL.len());
    }

    #[test]
    fn parse_modes_visual_x_maps_to_visual() {
        let modes = parse_sethandler_modes("x").unwrap();
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0], MappingMode::Visual);
    }

    #[test]
    fn parse_modes_deduplicates_v_and_x() {
        let modes = parse_sethandler_modes("v-x").unwrap();
        // v and x both map to Visual, so should only appear once
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0], MappingMode::Visual);
    }

    #[test]
    fn parse_modes_empty_string_errors() {
        assert!(parse_sethandler_modes("").is_err());
    }

    #[test]
    fn parse_modes_unknown_char_errors() {
        assert!(parse_sethandler_modes("z").is_err());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // offset_to_line_col
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn offset_to_line_col_first_char() {
        let (line, col) = offset_to_line_col("hello\nworld", 0);
        assert_eq!(line, 1);
        assert_eq!(col, 0);
    }

    #[test]
    fn offset_to_line_col_second_line() {
        let (line, col) = offset_to_line_col("hello\nworld", 6);
        assert_eq!(line, 2);
        assert_eq!(col, 0);
    }

    #[test]
    fn offset_to_line_col_middle_of_second_line() {
        let (line, col) = offset_to_line_col("hello\nworld", 8);
        assert_eq!(line, 2);
        assert_eq!(col, 2);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // line_snippet
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn line_snippet_returns_line_content() {
        let snippet = line_snippet("hello\nworld\n", 0);
        assert_eq!(snippet, "hello");
    }

    #[test]
    fn line_snippet_second_line() {
        let snippet = line_snippet("hello\nworld\n", 6);
        assert_eq!(snippet, "world");
    }

    #[test]
    fn line_snippet_truncates_at_max_chars() {
        let long_line = "a".repeat(100);
        let snippet = line_snippet(&long_line, 0);
        assert_eq!(snippet.len(), LINE_SNIPPET_MAX_CHARS);
    }

    #[test]
    fn line_snippet_short_line_not_truncated() {
        let snippet = line_snippet("short", 0);
        assert_eq!(snippet, "short");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // format_line_spec
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn format_line_spec_current() {
        assert_eq!(format_line_spec(&LineSpec::Current), ".");
    }

    #[test]
    fn format_line_spec_last() {
        assert_eq!(format_line_spec(&LineSpec::Last), "$");
    }

    #[test]
    fn format_line_spec_absolute() {
        assert_eq!(format_line_spec(&LineSpec::Absolute(42)), "42");
    }

    #[test]
    fn format_line_spec_relative_positive() {
        assert_eq!(format_line_spec(&LineSpec::Relative(3)), "+3");
    }

    #[test]
    fn format_line_spec_relative_negative() {
        assert_eq!(format_line_spec(&LineSpec::Relative(-2)), "-2");
    }

    #[test]
    fn format_line_spec_relative_zero_is_current() {
        assert_eq!(format_line_spec(&LineSpec::Relative(0)), ".");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // format_ex_range
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn format_ex_range_single_line() {
        let range = ExRange {
            start: LineSpec::Absolute(5),
            end: None,
            separator: crate::grammar::types::RangeSeparator::Comma,
        };
        assert_eq!(format_ex_range(&range), "5");
    }

    #[test]
    fn format_ex_range_two_lines() {
        let range = ExRange {
            start: LineSpec::Absolute(1),
            end: Some(LineSpec::Last),
            separator: crate::grammar::types::RangeSeparator::Comma,
        };
        assert_eq!(format_ex_range(&range), "1,$");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // modes_for_prefix
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn modes_for_prefix_all() {
        use crate::grammar::types::MapModePrefix;
        let modes = modes_for_prefix(MapModePrefix::All);
        assert_eq!(modes.len(), 3); // Normal, Visual, Operator
    }

    #[test]
    fn modes_for_prefix_normal() {
        use crate::grammar::types::MapModePrefix;
        let modes = modes_for_prefix(MapModePrefix::Normal);
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0], MappingMode::Normal);
    }

    #[test]
    fn modes_for_prefix_insert() {
        use crate::grammar::types::MapModePrefix;
        let modes = modes_for_prefix(MapModePrefix::Insert);
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0], MappingMode::Insert);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // query_option
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn query_option_known_returns_some() {
        let opts = crate::primitives::VimOptions::default();
        let result = query_option(&opts, "tabstop");
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("tabstop="));
    }

    #[test]
    fn query_option_short_form_returns_some() {
        let opts = crate::primitives::VimOptions::default();
        let result = query_option(&opts, "ts");
        assert!(result.is_some());
    }

    #[test]
    fn query_option_unknown_returns_none() {
        let opts = crate::primitives::VimOptions::default();
        assert!(query_option(&opts, "nonexistent").is_none());
    }

    #[test]
    fn query_option_bool_true_no_prefix() {
        let opts = crate::primitives::VimOptions::default();
        // wrapscan defaults to true in Vim
        let result = query_option(&opts, "wrapscan");
        assert!(result.is_some());
        let val = result.unwrap();
        // Should be either "wrapscan" or "nowrapscan" depending on default
        assert!(val == "wrapscan" || val == "nowrapscan");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // set_bool_option
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn set_bool_option_expandtab_true() {
        let mut opts = crate::primitives::VimOptions::default();
        let effects = set_bool_option(&mut opts, "expandtab", true);
        assert!(opts.expandtab());
        assert!(effects.as_slice().is_empty());
    }

    #[test]
    fn set_bool_option_expandtab_false() {
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_expandtab(true);
        let effects = set_bool_option(&mut opts, "expandtab", false);
        assert!(!opts.expandtab());
        assert!(effects.as_slice().is_empty());
    }

    #[test]
    fn set_bool_option_unknown_returns_error() {
        let mut opts = crate::primitives::VimOptions::default();
        let effects = set_bool_option(&mut opts, "nonexistent", true);
        // Should produce an error effect
        assert!(!effects.as_slice().is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // assign_option
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn assign_option_tabstop_valid() {
        let mut opts = crate::primitives::VimOptions::default();
        let effects = assign_option(&mut opts, "tabstop", "4");
        assert_eq!(opts.tabstop(), 4);
        assert!(effects.as_slice().is_empty());
    }

    #[test]
    fn assign_option_tabstop_invalid() {
        let mut opts = crate::primitives::VimOptions::default();
        let effects = assign_option(&mut opts, "tabstop", "abc");
        // Should produce a number-required error
        assert!(!effects.as_slice().is_empty());
    }

    #[test]
    fn assign_option_clipboard() {
        let mut opts = crate::primitives::VimOptions::default();
        let effects = assign_option(&mut opts, "clipboard", "unnamed");
        assert!(effects.as_slice().is_empty());
    }

    #[test]
    fn assign_option_unknown_returns_error() {
        let mut opts = crate::primitives::VimOptions::default();
        let effects = assign_option(&mut opts, "nonexistent", "value");
        assert!(!effects.as_slice().is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // completion_message
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn completion_message_success_with_message() {
        use crate::execution::host::HostRequestId;
        let result = HostResult::Success {
            id: HostRequestId::new(1),
            message: Some(CompactString::from("done")),
        };
        let meta = HostRequestMeta {
            id: HostRequestId::new(1),
        };
        let msg = completion_message(&result, &meta);
        assert_eq!(msg, Some("done".to_string()));
    }

    #[test]
    fn completion_message_success_without_message() {
        use crate::execution::host::HostRequestId;
        let result = HostResult::Success {
            id: HostRequestId::new(1),
            message: None,
        };
        let meta = HostRequestMeta {
            id: HostRequestId::new(1),
        };
        assert_eq!(completion_message(&result, &meta), None);
    }

    #[test]
    fn completion_message_failure() {
        use crate::execution::host::HostRequestId;
        let result = HostResult::Failure {
            id: HostRequestId::new(1),
            error: CompactString::from("oops"),
        };
        let meta = HostRequestMeta {
            id: HostRequestId::new(1),
        };
        let msg = completion_message(&result, &meta);
        assert_eq!(msg, Some("oops".to_string()));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // :undojoin — undo_join_pending flag
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn undojoin_sets_undo_join_pending() {
        let doc = crate::test_utils::SimpleDocument::new("hello");
        let state = crate::state::VimState::new();
        let input = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        let opts = crate::primitives::VimOptions::default();
        let ctx = super::ExecutionContext::new(input, &state, &opts);
        let mut seq = crate::execution::host::HostRequestSequencer::default();
        let output = execute_ex_line("undojoin", &ctx, &mut seq).unwrap();
        assert!(
            output.undo_join_pending,
            ":undojoin should set undo_join_pending"
        );
    }

    #[test]
    fn undojoin_abbreviated_sets_undo_join_pending() {
        let doc = crate::test_utils::SimpleDocument::new("hello");
        let state = crate::state::VimState::new();
        let input = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        let opts = crate::primitives::VimOptions::default();
        let ctx = super::ExecutionContext::new(input, &state, &opts);
        let mut seq = crate::execution::host::HostRequestSequencer::default();
        let output = execute_ex_line("undoj", &ctx, &mut seq).unwrap();
        assert!(
            output.undo_join_pending,
            ":undoj (abbreviated) should set undo_join_pending"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // last_ex_for_dot — dot-repeat population
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn substitute_sets_last_ex_for_dot() {
        let doc = crate::test_utils::SimpleDocument::new("foo bar");
        let state = crate::state::VimState::new();
        let input = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        let opts = crate::primitives::VimOptions::default();
        let ctx = super::ExecutionContext::new(input, &state, &opts);
        let mut seq = crate::execution::host::HostRequestSequencer::default();
        let output = execute_ex_line("s/foo/baz/", &ctx, &mut seq).unwrap();
        assert_eq!(
            output.last_ex_for_dot.as_deref(),
            Some("s/foo/baz/"),
            ":s should populate last_ex_for_dot"
        );
    }

    #[test]
    fn delete_sets_last_ex_for_dot() {
        let doc = crate::test_utils::SimpleDocument::new("line one\nline two\n");
        let state = crate::state::VimState::new();
        let input = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        let opts = crate::primitives::VimOptions::default();
        let ctx = super::ExecutionContext::new(input, &state, &opts);
        let mut seq = crate::execution::host::HostRequestSequencer::default();
        let output = execute_ex_line("delete", &ctx, &mut seq).unwrap();
        assert_eq!(
            output.last_ex_for_dot.as_deref(),
            Some("delete"),
            ":delete should populate last_ex_for_dot"
        );
    }

    #[test]
    fn non_mutating_command_does_not_set_last_ex_for_dot() {
        let doc = crate::test_utils::SimpleDocument::new("foo bar");
        let state = crate::state::VimState::new();
        let input = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        let opts = crate::primitives::VimOptions::default();
        let ctx = super::ExecutionContext::new(input, &state, &opts);
        let mut seq = crate::execution::host::HostRequestSequencer::default();
        let output = execute_ex_line("set tabstop=4", &ctx, &mut seq).unwrap();
        assert!(
            output.last_ex_for_dot.is_none(),
            ":set should NOT populate last_ex_for_dot"
        );
    }

    #[test]
    fn undojoin_does_not_set_last_ex_for_dot() {
        let doc = crate::test_utils::SimpleDocument::new("foo bar");
        let state = crate::state::VimState::new();
        let input = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        let opts = crate::primitives::VimOptions::default();
        let ctx = super::ExecutionContext::new(input, &state, &opts);
        let mut seq = crate::execution::host::HostRequestSequencer::default();
        let output = execute_ex_line("undojoin", &ctx, &mut seq).unwrap();
        assert!(
            output.last_ex_for_dot.is_none(),
            ":undojoin should NOT populate last_ex_for_dot (not text-mutating)"
        );
    }

    #[test]
    fn pipeline_with_mutating_command_sets_last_ex_for_dot() {
        let doc = crate::test_utils::SimpleDocument::new("foo bar");
        let state = crate::state::VimState::new();
        let input = crate::execution::InputContext::new(&doc, 0)
            .validate()
            .unwrap();
        let opts = crate::primitives::VimOptions::default();
        let ctx = super::ExecutionContext::new(input, &state, &opts);
        let mut seq = crate::execution::host::HostRequestSequencer::default();
        // Pipeline: undojoin (non-mutating) | s/foo/baz/ (mutating)
        let output = execute_ex_line("undojoin | s/foo/baz/", &ctx, &mut seq).unwrap();
        assert!(
            output.last_ex_for_dot.is_some(),
            "pipeline containing :s should populate last_ex_for_dot"
        );
    }
}
