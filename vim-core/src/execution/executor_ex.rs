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
use crate::grammar::ex_parser::parse_ex_piece;
use crate::grammar::types::{ExCommand, ExRange, LineSpec, ModifierFlags, TimeAmount};
use crate::keymap::{Handler, KeyEvent, MappingMode};
use crate::primitives::byte_delta;
use crate::primitives::{
    MotionType, OptionId, OptionOverrides, OptionValue, Range, RegisterContent, RegisterName,
    VimValue,
};
use compact_str::CompactString;
use smallvec::SmallVec;

use super::script_expr;
use super::set_operator::{apply_set_operator, parse_vim_number, SetOperator};
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
        /// In the current buffer's mappings.
        buffer: bool,
    },
    /// Remove a mapping.
    Remove {
        modes: SmallVec<[crate::keymap::MappingMode; 4]>,
        lhs_raw: CompactString,
        buffer: bool,
    },
    /// Clear all mappings for the given modes.
    ClearAll {
        modes: SmallVec<[crate::keymap::MappingMode; 4]>,
        buffer: bool,
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
        unique: bool,
    },
    /// Remove an abbreviation.
    Remove {
        trigger: CompactString,
        mode: crate::primitives::AbbrevMode,
    },
    /// Take a mode off every abbreviation.
    Clear { mode: crate::primitives::AbbrevMode },
}

/// A `:let` of `mapleader` or `maplocalleader`.
#[derive(Debug)]
pub(crate) struct LeaderChange {
    /// `maplocalleader`.
    pub local: bool,
    /// The value, which the engine stores in the `g:` variable and takes
    /// the leader from.
    pub value: VimValue,
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
    /// The `:set` commands, which the engine applies to its options in
    /// order.
    pub set_commands: SmallVec<[SetCommand; 1]>,
    /// Mapping mutations from `:map`/`:noremap`/`:unmap`.
    pub mapping_changes: SmallVec<[MappingChange; 1]>,
    /// Abbreviation mutations from `:abbreviate`/`:unabbreviate`/`:abclear`.
    pub abbrev_changes: SmallVec<[AbbrevChange; 1]>,
    /// Handler map mutations from `:sethandler`.
    pub handler_changes: SmallVec<[HandlerChange; 2]>,
    /// Undo tree navigation (engine handles tree mutation).
    pub undo_navigation: Option<UndoNavigation>,
    /// Leader changes from `:let mapleader = ...` and
    /// `:let maplocalleader = ...` (engine applies to the keymap).
    pub leader_changes: SmallVec<[LeaderChange; 1]>,
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
    /// A `:g` or `:v`, which the engine runs one marked line at a time.
    pub global: Option<GlobalCommand>,
    /// The lines of a `:@r` register, which the engine runs as command
    /// lines, one after another.
    pub ex_lines: Vec<CompactString>,
    /// The modifiers of the command, applied to its effects already, and
    /// to be applied to what the engine runs for it.
    pub modifiers: ModifierFlags,
}

/// A `:g/{pattern}/{commands}` or `:v` for the engine to run.
#[derive(Debug, Clone)]
pub(crate) struct GlobalCommand {
    /// The lines it searches.
    pub range: ExRange,
    /// The pattern a line must match, or with `invert` must not.
    pub pattern: CompactString,
    /// The command line run on each line.
    pub commands: CompactString,
    /// `:v` or `:g!`.
    pub invert: bool,
}

/// Parse and execute one command of an Ex command line, as
/// [`split_ex_pipeline`](crate::grammar::split_ex_pipeline) gives it.
///
/// Its modifiers (`:silent`, `:keepjumps`, ...) filter its effects, an
/// error included, so `:silent!` hides it. A `:g` and the lines of `:@r`
/// come back for the engine to run.
pub(crate) fn execute_ex_piece<D: Document>(
    piece: &str,
    ctx: &ExecutionContext<'_, D>,
    sequencer: &mut HostRequestSequencer,
) -> ExExecutionOutput {
    let (modifiers, parsed) = parse_ex_piece(piece);
    let result = parsed.and_then(|command| {
        let mut output = execute_ex_command(&command, ctx, sequencer)?;
        if command.mutates_text() && output.last_ex_for_dot.is_none() {
            output.last_ex_for_dot = Some(CompactString::from(piece));
        }
        Ok(output)
    });
    let mut output = result.unwrap_or_else(|err| ExExecutionOutput {
        effects: crate::commands::ex::effects::show_error(err),
        ..ExExecutionOutput::default()
    });
    super::modifier_filter::apply_modifiers(&mut output.effects, modifiers);
    output.modifiers = modifiers;
    output
}

fn set_output(
    scope: SetScope,
    assignments: &SmallVec<[crate::grammar::types::SetAssignment; 2]>,
) -> ExExecutionOutput {
    ExExecutionOutput {
        set_commands: smallvec::smallvec![SetCommand {
            scope,
            assignments: assignments.clone(),
        }],
        ..ExExecutionOutput::default()
    }
}

/// The lines `global` marks in the text of `ctx`, 0-indexed: pass 1 of
/// `ex_global()`.
pub(crate) fn global_lines<D: Document>(
    global: &GlobalCommand,
    ctx: &ExecutionContext<'_, D>,
) -> Result<Vec<usize>, VimError> {
    let mark_resolver = |name: crate::primitives::MarkName| {
        ctx.state.marks().get(name).map(|mark| mark.offset().get())
    };
    let mut ex_ctx = ExContext::new(ctx.doc().text(), ctx.cursor_pos().line().get())
        .with_mark_resolver(&mark_resolver);
    if let Some(tree) = ctx.doc().vim_text_tree() {
        ex_ctx = ex_ctx.with_tree(tree);
    }
    crate::commands::ex::global::global_lines(
        &global.range,
        &global.pattern,
        global.invert,
        &ex_ctx,
    )
}

/// Whether the line `line` of `ctx` matches the pattern of `global`.
pub(crate) fn global_line_matches<D: Document>(
    global: &GlobalCommand,
    line: usize,
    ctx: &ExecutionContext<'_, D>,
) -> Result<bool, VimError> {
    let ex_ctx = ExContext::new(ctx.doc().text(), ctx.cursor_pos().line().get());
    crate::commands::ex::global::line_matches_pattern(&global.pattern, line, &ex_ctx)
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
        ExCommand::Set { assignments } => Ok(set_output(SetScope::Effective, assignments)),
        ExCommand::SetLocal { assignments } => Ok(set_output(SetScope::Local, assignments)),
        ExCommand::SetGlobal { assignments } => Ok(set_output(SetScope::Global, assignments)),
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
            buffer,
        } => execute_map_command(*mode_prefix, lhs, rhs.as_deref(), *kind, *flags, *buffer),
        ExCommand::Unmap {
            mode_prefix,
            lhs,
            buffer,
        } => execute_unmap_command(*mode_prefix, lhs, *buffer),
        ExCommand::MapClear { mode, buffer, .. } => execute_mapclear_command(*mode, *buffer),
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
        ExCommand::LetLeader { local, value } => let_leader(*local, value, ctx),
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
            // A line ending in a backslash continues on the next one.
            let mut ex_lines = Vec::new();
            let mut continuation = String::new();
            for raw_line in text.lines() {
                if let Some(head) = raw_line.strip_suffix('\\') {
                    continuation.push_str(head);
                    continue;
                }
                continuation.push_str(raw_line);
                let line = std::mem::take(&mut continuation);
                if !line.trim().is_empty() {
                    ex_lines.push(CompactString::from(line));
                }
            }
            if !continuation.is_empty() {
                ex_lines.push(CompactString::from(continuation));
            }
            Ok(ExExecutionOutput {
                ex_lines,
                ..ExExecutionOutput::default()
            })
        }
        ExCommand::Global {
            range,
            pattern,
            commands,
            invert,
        } => Ok(ExExecutionOutput {
            global: Some(GlobalCommand {
                range: range.clone(),
                pattern: pattern.clone(),
                commands: commands.clone(),
                invert: *invert,
            }),
            ..ExExecutionOutput::default()
        }),
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
        // Only a sourced file runs script blocks.
        ExCommand::Block { kind, argument } => {
            let mut form = CompactString::from(kind.name());
            if !argument.is_empty() {
                form.push(' ');
                form.push_str(argument);
            }
            Err(VimError::NotAvailable(form))
        }
        ExCommand::Finish => Err(VimError::FinishOutsideSource),
        // Abbreviation commands
        ExCommand::Abbreviate {
            trigger,
            replacement,
            mode,
            noremap,
            unique,
        } => Ok(ExExecutionOutput {
            abbrev_changes: smallvec::smallvec![AbbrevChange::Define {
                trigger: trigger.clone(),
                replacement: replacement.as_deref().map(|rhs| nop_as_nothing(rhs).into()),
                mode: *mode,
                noremap: *noremap,
                unique: *unique,
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
        ExCommand::AbClear { buffer: true, .. } => Ok(ExExecutionOutput::default()),
        ExCommand::AbClear { mode, .. } => Ok(ExExecutionOutput {
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
        "commentstring" | "cms" => Some(OptionId::CommentString),
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
        "formatoptions" | "fo" => Some(OptionId::FormatOptions),
        "comments" | "com" => Some(OptionId::Comments),
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

/// The options of one `:set`, `:setlocal` or `:setglobal`.
#[derive(Debug, Clone)]
pub(crate) struct SetCommand {
    /// The layers it writes.
    pub scope: SetScope,
    /// Its arguments, in order.
    pub assignments: SmallVec<[crate::grammar::types::SetAssignment; 2]>,
}

/// Apply each command of `commands` in turn, as [`apply_set_assignments`]
/// applies one.
pub(crate) fn apply_set_commands(
    global: &mut crate::primitives::VimOptions,
    buffer_overrides: &mut OptionOverrides,
    window_overrides: &mut OptionOverrides,
    commands: &[SetCommand],
) -> Effects {
    let mut effects = Effects::new();
    for command in commands {
        effects.extend(apply_set_assignments(
            command.scope,
            global,
            buffer_overrides,
            window_overrides,
            &command.assignments,
        ));
    }
    effects
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
/// As Vim's `do_set()` does, the first argument that fails ends the
/// command: the ones before it stay applied, and its error follows the
/// values the command showed.
pub(crate) fn apply_set_assignments(
    scope: SetScope,
    global: &mut crate::primitives::VimOptions,
    buffer_overrides: &mut OptionOverrides,
    window_overrides: &mut OptionOverrides,
    assignments: &[crate::grammar::types::SetAssignment],
) -> Effects {
    let mut layers = SetLayers {
        scope,
        global,
        buffer: buffer_overrides,
        window: window_overrides,
    };
    let mut messages = Vec::new();
    let failed = assignments
        .iter()
        .find_map(|assignment| layers.apply(assignment, &mut messages).err());
    let mut effects = if messages.is_empty() {
        Effects::new()
    } else {
        ex_effects::show_message(messages.join("\n").into())
    };
    if let Some(error) = failed {
        effects.extend(ex_effects::show_error(error));
    }
    effects
}

/// The error `assignment` would give, as `:let &name` shows it: without
/// the argument `:set` adds, as `set_option_value_give_err()` gives the
/// message (evalvars.c `ex_let_option()`).
pub(crate) fn let_option_error(
    global: &crate::primitives::VimOptions,
    assignment: &crate::grammar::types::SetAssignment,
) -> Option<VimError> {
    let mut global = global.clone();
    let mut layers = SetLayers {
        scope: SetScope::Global,
        global: &mut global,
        buffer: &mut OptionOverrides::default(),
        window: &mut OptionOverrides::default(),
    };
    let error = layers.apply(assignment, &mut Vec::new()).err()?;
    let suffix = format!(": {}", set_argument(assignment));
    Some(match error {
        VimError::OptionValue(text) => match text.strip_suffix(&suffix) {
            Some(message) => VimError::OptionValue(message.into()),
            None => VimError::OptionValue(text),
        },
        other => other,
    })
}

/// What `:set` writes: the layers of one command.
struct SetLayers<'a> {
    scope: SetScope,
    global: &'a mut crate::primitives::VimOptions,
    buffer: &'a mut OptionOverrides,
    window: &'a mut OptionOverrides,
}

/// The type of an option as `:set` reads its arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetKind {
    Bool,
    Number,
    Text,
}

/// The type of the option `:set` knows by `name`, long or short.
fn set_kind(options: &crate::primitives::VimOptions, name: &str) -> Option<SetKind> {
    use crate::primitives::OptionKind;
    if let Some((_, value)) = option_value(options, name) {
        return Some(match value {
            OptionValue::Bool(_) => SetKind::Bool,
            OptionValue::Unsigned(_) | OptionValue::Signed(_) => SetKind::Number,
            OptionValue::Str(_) => SetKind::Text,
        });
    }
    option_name_to_id(name).map(|id| match id.kind() {
        OptionKind::Bool => SetKind::Bool,
        OptionKind::Number => SetKind::Number,
        _ => SetKind::Text,
    })
}

/// The type of an option of Vim's table that `:set` cannot name, by its
/// long or short name: vim-core acts as a Vim built without it
/// (`*hidden-options*`).
pub(crate) fn hidden_option(
    options: &crate::primitives::VimOptions,
    name: &str,
) -> Option<SetKind> {
    use crate::primitives::vim_option_names::{OptionType, OPTIONS, TERMINAL_CODES};
    if set_kind(options, name).is_some() {
        return None;
    }
    if TERMINAL_CODES.contains(&name) {
        return Some(SetKind::Text);
    }
    let row = OPTIONS
        .iter()
        .find(|row| row.name == name || (!row.short.is_empty() && row.short == name))?;
    Some(match row.kind {
        OptionType::Bool => SetKind::Bool,
        OptionType::Number => SetKind::Number,
        OptionType::String => SetKind::Text,
    })
}

/// The id `:set` writes an option through. 'belloff' is a string to
/// `:set` but a flag to the engine, so it goes by name.
fn set_id(name: &str) -> Option<OptionId> {
    option_name_to_id(name).filter(|&id| id != OptionId::BellOff)
}

/// The name after a `no` or `inv` prefix, as `get_option_prefix()` reads
/// it.
fn strip_set_prefix(name: &str) -> Option<&str> {
    if name.starts_with("novice") {
        return None;
    }
    name.strip_prefix("no").or_else(|| name.strip_prefix("inv"))
}

/// The argument an assignment was written as, for its error, with a
/// control character as `^M`, as `trans_characters()` shows it.
fn set_argument(assignment: &crate::grammar::types::SetAssignment) -> String {
    use crate::grammar::types::SetAssignment as A;
    let written = match assignment {
        A::ShowAll => "all".to_owned(),
        A::SetBool(name) => name.to_string(),
        A::UnsetBool(name) => format!("no{name}"),
        A::ToggleBool(name) => format!("{name}!"),
        A::InvertBool(name) => format!("inv{name}"),
        A::Query(name) => format!("{name}?"),
        A::Assign(name, value) => format!("{name}={value}"),
        A::Append(name, value) => format!("{name}+={value}"),
        A::Remove(name, value) => format!("{name}-={value}"),
        A::Prepend(name, value) => format!("{name}^={value}"),
        A::Reset { argument, .. } | A::Invalid { argument, .. } => argument.to_string(),
    };
    let mut shown = String::with_capacity(written.len());
    for c in written.chars() {
        if c.is_ascii_control() {
            shown.push('^');
            shown.push(char::from(u8::try_from(c).unwrap_or(b'?') ^ 0x40));
        } else {
            shown.push(c);
        }
    }
    shown
}

/// A string value of `:set` without the backslash before each character,
/// as Vim's `stropt_copy_value()` copies it.
fn unescape_set_value(value: &str) -> std::borrow::Cow<'_, str> {
    if !value.contains('\\') {
        return std::borrow::Cow::Borrowed(value);
    }
    let mut unescaped = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(&next) if c == '\\' => {
                unescaped.push(next);
                chars.next();
            }
            _ => unescaped.push(c),
        }
    }
    std::borrow::Cow::Owned(unescaped)
}

/// A `:set` error in Vim's format: the message, then the argument as
/// written (`E539: Illegal character <Z>: fo+=Z`).
fn set_error(message: &str, argument: &str) -> VimError {
    VimError::OptionValue(format!("{message}: {argument}").into())
}

impl SetLayers<'_> {
    /// Apply one argument, as Vim's `do_set_option()` does.
    fn apply(
        &mut self,
        assignment: &crate::grammar::types::SetAssignment,
        messages: &mut Vec<String>,
    ) -> Result<(), VimError> {
        use crate::grammar::types::SetAssignment as A;
        if let Some(result) = self.apply_hidden(assignment) {
            return result;
        }
        let argument = || set_argument(assignment);
        let unknown = || VimError::UnknownOptionArgument(argument().into());
        let invalid = || set_error("E474: Invalid argument", &argument());
        match assignment {
            A::ShowAll => {
                messages.push(format_all_options(self.global));
                Ok(())
            }
            A::SetBool(name) => match set_kind(self.global, name).ok_or_else(unknown)? {
                SetKind::Bool => {
                    self.set_bool(name, true);
                    Ok(())
                }
                // A bare name of a number or string option shows it.
                _ => self.show(name, messages).ok_or_else(unknown),
            },
            A::UnsetBool(name) => match set_kind(self.global, name).ok_or_else(unknown)? {
                SetKind::Bool => {
                    self.set_bool(name, false);
                    Ok(())
                }
                _ => Err(invalid()),
            },
            // do_set_option_value(): `inv` before a number or string option
            // is E474, as `no` is.
            A::InvertBool(name) => match set_kind(self.global, name).ok_or_else(unknown)? {
                SetKind::Bool => {
                    self.toggle_bool(name);
                    Ok(())
                }
                _ => Err(invalid()),
            },
            A::ToggleBool(name) => {
                let (name, prefixed) = self.resolve(name).ok_or_else(unknown)?;
                match set_kind(self.global, name) {
                    Some(SetKind::Bool) => {
                        self.toggle_bool(name);
                        Ok(())
                    }
                    _ if prefixed => Err(invalid()),
                    // `:set tw!` shows 'textwidth', then finds the `!`.
                    _ => {
                        self.show(name, messages).ok_or_else(unknown)?;
                        Err(VimError::TrailingCharacters(argument().into()))
                    }
                }
            }
            A::Query(name) => {
                let (name, _) = self.resolve(name).ok_or_else(unknown)?;
                self.show(name, messages).ok_or_else(unknown)
            }
            A::Assign(name, value) => match self.value_kind(name, &argument)? {
                SetKind::Bool => Err(invalid()),
                SetKind::Number => {
                    let n = parse_vim_number(value)
                        .ok_or_else(|| set_error("E521: Number required after =", &argument()))?;
                    self.set_number(name, n)
                        .map_err(|message| set_error(message, &argument()))
                }
                SetKind::Text => self
                    .set_text(name, &unescape_set_value(value))
                    .map_err(|message| set_error(&message, &argument())),
            },
            A::Append(name, value) | A::Remove(name, value) | A::Prepend(name, value) => {
                let op = match assignment {
                    A::Append(..) => SetOperator::Append,
                    A::Remove(..) => SetOperator::Remove,
                    _ => SetOperator::Prepend,
                };
                let value = match self.value_kind(name, &argument)? {
                    SetKind::Text => unescape_set_value(value),
                    _ => std::borrow::Cow::Borrowed(value.as_str()),
                };
                self.apply_operator(name, op, &value)
                    .map_err(|message| set_error(&message, &argument()))
            }
            A::Invalid { name, .. } => {
                let kind = set_kind(self.global, name).ok_or_else(unknown)?;
                // `:set tw,` shows 'textwidth' first, `:set tw?x` does not.
                let after_name = set_argument(assignment)
                    .strip_prefix(&**name)
                    .is_some_and(|rest| !rest.starts_with(['?', '!', '&', '<', ' ', '\t']));
                if kind != SetKind::Bool && after_name {
                    self.show(name, messages);
                }
                Err(VimError::TrailingCharacters(argument().into()))
            }
            A::Reset { name, .. } => Err(
                if &**name == "all" || set_kind(self.global, name).is_some() {
                    VimError::NotAvailable(argument().into())
                } else {
                    unknown()
                },
            ),
        }
    }

    /// An argument for an option this build lacks, as `do_set_option()`
    /// skips it: any form that sets it does nothing, and a query, or a
    /// bare name of one that is not a boolean, is E519. `None` when the
    /// argument names no such option.
    fn apply_hidden(
        &self,
        assignment: &crate::grammar::types::SetAssignment,
    ) -> Option<Result<(), VimError>> {
        use crate::grammar::types::SetAssignment as A;
        // A name written with its `no` or `inv` prefix.
        let written = |name: &str| {
            hidden_option(self.global, name).or_else(|| {
                strip_set_prefix(name).and_then(|bare| hidden_option(self.global, bare))
            })
        };
        // The type, and the character after the name and its blanks, with
        // `+=`, `-=` and `^=` read as `=`.
        let (kind, next) = match assignment {
            A::ShowAll => return None,
            A::SetBool(name) | A::UnsetBool(name) => (hidden_option(self.global, name)?, None),
            A::InvertBool(name) => (hidden_option(self.global, name)?, None),
            A::ToggleBool(name) => (written(name)?, Some('!')),
            A::Query(name) => (written(name)?, Some('?')),
            A::Assign(name, _) | A::Append(name, _) | A::Remove(name, _) | A::Prepend(name, _) => {
                (written(name)?, Some('='))
            }
            A::Reset { name, .. } => (hidden_option(self.global, name)?, Some('&')),
            A::Invalid { name, argument } => {
                let kind = hidden_option(self.global, name)?;
                let unprefixed = strip_set_prefix(argument).unwrap_or(argument);
                let after = unprefixed.get(name.len()..).unwrap_or_default();
                (kind, after.trim_start_matches([' ', '\t']).chars().next())
            }
        };
        let sets = matches!(next, Some('=' | ':' | '!' | '&' | '<'))
            || (kind == SetKind::Bool && next != Some('?'));
        Some(if sets {
            Ok(())
        } else {
            Err(VimError::OptionNotSupported(
                set_argument(assignment).into(),
            ))
        })
    }

    /// The name `:set` knows, and whether a `no` or `inv` prefix came off
    /// it, which `?` and `!` ignore.
    fn resolve<'n>(&self, name: &'n str) -> Option<(&'n str, bool)> {
        if set_kind(self.global, name).is_some() {
            return Some((name, false));
        }
        strip_set_prefix(name)
            .filter(|stripped| set_kind(self.global, stripped).is_some())
            .map(|stripped| (stripped, true))
    }

    /// The type of the option an operator or `=` sets: E474 for a `no` or
    /// `inv` prefix, which takes no value, and E518 for no such option.
    fn value_kind(&self, name: &str, argument: &dyn Fn() -> String) -> Result<SetKind, VimError> {
        if let Some(kind) = set_kind(self.global, name) {
            return Ok(kind);
        }
        if strip_set_prefix(name).is_some_and(|stripped| set_kind(self.global, stripped).is_some())
        {
            return Err(set_error("E474: Invalid argument", &argument()));
        }
        Err(VimError::UnknownOptionArgument(argument().into()))
    }

    /// Show an option as `:set name?` does; `None` for one `:set` cannot
    /// show.
    fn show(&self, name: &str, messages: &mut Vec<String>) -> Option<()> {
        messages.push(query_option(self.global, name)?);
        Some(())
    }

    fn set_bool(&mut self, name: &str, value: bool) {
        match set_id(name) {
            Some(id) => self.write(id, OptionValue::Bool(value)),
            None => {
                set_bool_option(self.global, name, value);
            }
        }
    }

    fn toggle_bool(&mut self, name: &str) {
        let Some(id) = set_id(name) else {
            toggle_bool_option(self.global, name);
            return;
        };
        // The value in use, so that toggling respects local overrides.
        let current = matches!(
            crate::primitives::resolve_option(
                id,
                self.global,
                Some(self.buffer),
                Some(self.window)
            ),
            OptionValue::Bool(true)
        );
        self.write(id, OptionValue::Bool(!current));
    }

    /// Set a number option from `=`, as `do_set_option_numeric()` and
    /// the option's own check take it.
    fn set_number(&mut self, name: &str, n: i64) -> Result<(), &'static str> {
        let Some(id) = set_id(name) else {
            let n = usize::try_from(n).map_err(|_| "E487: Argument must be positive")?;
            return assign_option(self.global, name, &n.to_string());
        };
        let value = number_option_value(id, &self.global.get_option(id), n)?;
        self.write(id, value);
        Ok(())
    }

    /// Set a string option from `=`.
    fn set_text(&mut self, name: &str, value: &str) -> Result<(), String> {
        match set_id(name) {
            Some(id) => {
                let value = validate_string_value(id, value)?;
                self.write(id, OptionValue::Str(value));
                Ok(())
            }
            None => assign_option(self.global, name, value).map_err(str::to_owned),
        }
    }

    /// Apply `+=`, `-=` or `^=`. Returns Vim's message without the
    /// argument.
    fn apply_operator(&mut self, name: &str, op: SetOperator, value: &str) -> Result<(), String> {
        let Some(id) = set_id(name) else {
            return apply_operator_by_name(self.global, name, op, value).map_err(str::to_owned);
        };
        // `:setglobal` works from the global value; `:set` and `:setlocal`
        // from the effective one, and `:set` then writes the result to both
        // layers, as in Vim.
        let global_value = self.global.get_option(id);
        let local_value = crate::primitives::resolve_option(
            id,
            self.global,
            Some(self.buffer),
            Some(self.window),
        );
        // A local layer that holds another type is not the value in use,
        // as in the resolved options.
        let base = if self.scope == SetScope::Global
            || std::mem::discriminant(&local_value) != std::mem::discriminant(&global_value)
        {
            global_value
        } else {
            local_value
        };
        let new = apply_set_operator(id.kind(), id == OptionId::WhichWrap, &base, op, value)
            .map_err(str::to_owned)?;
        let new = match new {
            OptionValue::Str(s) => OptionValue::Str(validate_string_value(id, &s)?),
            OptionValue::Unsigned(n) => {
                number_option_value(id, &base, i64::try_from(n).unwrap_or(i64::MAX))?
            }
            other => other,
        };
        self.write(id, new);
        Ok(())
    }

    fn write(&mut self, id: OptionId, value: OptionValue) {
        apply_value_with_scope(self.scope, id, value, self.global, self.buffer, self.window);
    }
}

/// `n` as the value of the number option `id`, whose value now is `like`:
/// E487 for a negative value of an option that takes none, and Vim's
/// range of 'tabstop', 1 to 9999 (`check_num_option_bounds()`).
fn number_option_value(
    id: OptionId,
    like: &OptionValue,
    n: i64,
) -> Result<OptionValue, &'static str> {
    if let OptionValue::Signed(_) = like {
        return Ok(OptionValue::Signed(n));
    }
    let n = usize::try_from(n).map_err(|_| "E487: Argument must be positive")?;
    match n {
        0 if id == OptionId::TabStop => Err("E487: Argument must be positive"),
        10_000.. if id == OptionId::TabStop => Err("E474: Invalid argument"),
        n => Ok(OptionValue::Unsigned(n)),
    }
}

/// `:let mapleader = {expr}`: the value of the expression, as `ex_let()`
/// evaluates it. The first error stands for an expression that fails.
fn let_leader<D: Document>(
    local: bool,
    expr: &str,
    ctx: &ExecutionContext<'_, D>,
) -> Result<ExExecutionOutput, VimError> {
    let env = script_expr::Env {
        variables: ctx.state.variable_store(),
        options: ctx.options,
        global_options: ctx.options,
        host: &super::config::NoHost,
    };
    let evaluated = script_expr::evaluate(expr, &env);
    let mut errors = evaluated.errors.into_iter();
    let Some(value) = evaluated.value else {
        return Err(errors
            .next()
            .unwrap_or_else(|| VimError::InvalidExpression(expr.into())));
    };
    let mut effects = Effects::new();
    for error in errors {
        effects.extend(ex_effects::show_error(error));
    }
    let mut leader_changes = SmallVec::new();
    leader_changes.push(LeaderChange {
        local,
        value: value.into_variable(),
    });
    Ok(ExExecutionOutput {
        effects,
        leader_changes,
        ..ExExecutionOutput::default()
    })
}

/// Check a new string value the way Vim's per-option check does, and
/// normalize it the way Vim stores it.
///
/// Returns the error message without the trailing `: {arg}`.
fn validate_string_value(id: OptionId, value: &str) -> Result<CompactString, String> {
    match id {
        OptionId::FormatOptions => {
            let normalized = crate::primitives::FormatFlags::normalize(value);
            crate::primitives::FormatFlags::parse(&normalized)
                .map_err(|c| format!("E539: Illegal character <{c}>"))?;
            Ok(CompactString::from(normalized))
        }
        OptionId::Comments => {
            crate::primitives::CommentSpec::validate(value).map_err(|e| e.to_string())?;
            Ok(CompactString::from(value))
        }
        _ => Ok(CompactString::from(value)),
    }
}

/// Apply a `:set` operator to an option that has no [`OptionId`] and lives
/// only in the global layer (`langmap`, `multilinefindrange`, 'belloff').
/// Returns Vim's message without the argument.
fn apply_operator_by_name(
    options: &mut crate::primitives::VimOptions,
    name: &str,
    op: SetOperator,
    value: &str,
) -> Result<(), &'static str> {
    use crate::primitives::OptionKind;

    let (kind, base) = match option_value(options, name) {
        Some((_, OptionValue::Unsigned(n))) => (OptionKind::Number, OptionValue::Unsigned(n)),
        Some((_, OptionValue::Str(s))) => (OptionKind::CommaList, OptionValue::Str(s)),
        Some(_) => return Err("E474: Invalid argument"),
        None => return Err("E518: Unknown option"),
    };
    match apply_set_operator(kind, false, &base, op, value)? {
        OptionValue::Unsigned(n) => assign_option(options, name, &n.to_string()),
        OptionValue::Str(s) => assign_option(options, name, &s),
        _ => Ok(()),
    }
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
    let (name, value) = option_value(options, name)?;
    Some(match value {
        OptionValue::Bool(on) => format_bool(name, on),
        OptionValue::Unsigned(n) => format!("{name}={n}"),
        OptionValue::Signed(n) => format!("{name}={n}"),
        OptionValue::Str(s) => format!("{name}={s}"),
    })
}

/// The long name and the value of an option that `:set` can show, by its
/// long or short name.
pub(crate) fn option_value(
    options: &crate::primitives::VimOptions,
    name: &str,
) -> Option<(&'static str, OptionValue)> {
    use OptionValue::{Bool, Signed, Str, Unsigned};
    let text = |s: &str| Str(CompactString::from(s));
    Some(match name {
        "tabstop" | "ts" => ("tabstop", Unsigned(options.tabstop())),
        "shiftwidth" | "sw" => ("shiftwidth", Unsigned(options.shiftwidth())),
        "expandtab" | "et" => ("expandtab", Bool(options.expandtab())),
        "autoindent" | "ai" => ("autoindent", Bool(options.autoindent())),
        "smartindent" | "si" => ("smartindent", Bool(options.smartindent())),
        "ignorecase" | "ic" => ("ignorecase", Bool(options.ignorecase())),
        "smartcase" | "scs" => ("smartcase", Bool(options.smartcase())),
        "hlsearch" | "hls" => ("hlsearch", Bool(options.hlsearch())),
        "incsearch" | "is" => ("incsearch", Bool(options.incsearch())),
        "wrapscan" | "ws" => ("wrapscan", Bool(options.wrapscan())),
        "scrolloff" | "so" => ("scrolloff", Unsigned(options.scrolloff())),
        "sidescrolloff" | "siso" => ("sidescrolloff", Unsigned(options.sidescrolloff())),
        "number" | "nu" => ("number", Bool(options.number())),
        "relativenumber" | "rnu" => ("relativenumber", Bool(options.relativenumber())),
        "textwidth" | "tw" => ("textwidth", Unsigned(options.textwidth())),
        "formatoptions" | "fo" => ("formatoptions", text(options.formatoptions())),
        "comments" | "com" => ("comments", text(options.comments())),
        "commentstring" | "cms" => ("commentstring", text(options.commentstring())),
        "timeoutlen" | "tm" => ("timeoutlen", Signed(i64::from(options.timeoutlen_ms()))),
        "undolevels" | "ul" => (
            "undolevels",
            Signed(options.undolevels().map_or(-1, byte_delta::to_i64)),
        ),
        "whichwrap" | "ww" => ("whichwrap", text(options.whichwrap())),
        "backspace" | "bs" => ("backspace", text(options.backspace())),
        "virtualedit" | "ve" => ("virtualedit", text(options.virtualedit())),
        "selection" | "sel" => ("selection", text(options.selection())),
        "clipboard" | "cb" => ("clipboard", text(options.clipboard())),
        "iskeyword" | "isk" => ("iskeyword", text(options.iskeyword())),
        "inccommand" | "icm" => ("inccommand", text(options.inccommand())),
        "gdefault" | "gd" => ("gdefault", Bool(options.gdefault())),
        "visualstar" => ("visualstar", Bool(options.visualstar())),
        "langmap" | "lmap" => ("langmap", text(options.langmap())),
        "langremap" | "lrm" => ("langremap", Bool(options.langremap())),
        "langnoremap" | "lnr" => ("langnoremap", Bool(!options.langremap())),
        "undoautogroupms" | "uagm" => (
            "undoautogroupms",
            Signed(options.undo_auto_group_ms().map_or(-1, i64::from)),
        ),
        "multilinefind" | "mlf" => ("multilinefind", Bool(options.multiline_find())),
        "multilinefindrange" | "mlfr" => (
            "multilinefindrange",
            Unsigned(options.multiline_find_range()),
        ),
        "belloff" | "bo" => ("belloff", text(if options.belloff() { "all" } else { "" })),
        "softtabstop" | "sts" => ("softtabstop", Signed(i64::from(options.softtabstop()))),
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

/// Set a boolean option `:set` knows only by name. Returns `false` for a
/// name it does not know.
fn set_bool_option(options: &mut crate::primitives::VimOptions, name: &str, value: bool) -> bool {
    match name {
        "expandtab" | "et" => options.set_expandtab(value),
        "autoindent" | "ai" => options.set_autoindent(value),
        "smartindent" | "si" => options.set_smartindent(value),
        "ignorecase" | "ic" => options.set_ignorecase(value),
        "smartcase" | "scs" => options.set_smartcase(value),
        "hlsearch" | "hls" => options.set_hlsearch(value),
        "incsearch" | "is" => options.set_incsearch(value),
        "wrapscan" | "ws" => options.set_wrapscan(value),
        "number" | "nu" => options.set_number(value),
        "relativenumber" | "rnu" => options.set_relativenumber(value),
        "gdefault" | "gd" => options.set_gdefault(value),
        "visualstar" => options.set_visualstar(value),
        "langremap" | "lrm" => options.set_langremap(value),
        // did_set_langnoremap(): 'langnoremap' is 'langremap' inverted.
        "langnoremap" | "lnr" => options.set_langremap(!value),
        "multilinefind" | "mlf" => options.set_multiline_find(value),
        _ => return false,
    }
    true
}

/// Invert a boolean option `:set` knows only by name. Returns `false` for
/// a name it does not know.
fn toggle_bool_option(options: &mut crate::primitives::VimOptions, name: &str) -> bool {
    match option_value(options, name) {
        Some((_, OptionValue::Bool(current))) => set_bool_option(options, name, !current),
        _ => false,
    }
}

/// Set an option `:set` knows only by name from the text of `=`. Returns
/// Vim's message without the argument.
fn assign_option(
    options: &mut crate::primitives::VimOptions,
    name: &str,
    value: &str,
) -> Result<(), &'static str> {
    const NUMBER_REQUIRED: &str = "E521: Number required after =";
    let number = || value.parse::<usize>().map_err(|_| NUMBER_REQUIRED);
    let signed = || value.parse::<i64>().map_err(|_| NUMBER_REQUIRED);
    match name {
        "tabstop" | "ts" => options.set_tabstop(number()?),
        "shiftwidth" | "sw" => options.set_shiftwidth(number()?),
        "scrolloff" | "so" => options.set_scrolloff(number()?),
        "sidescrolloff" | "siso" => options.set_sidescrolloff(number()?),
        "textwidth" | "tw" => options.set_textwidth(number()?),
        "timeoutlen" | "tm" => {
            options.set_timeoutlen_ms(value.parse::<u32>().map_err(|_| NUMBER_REQUIRED)?);
        }
        // -1 means no limit, and undo groups by time are off.
        "undolevels" | "ul" => options.set_undolevels(usize::try_from(signed()?).ok()),
        "undoautogroupms" | "uagm" => {
            let ms = signed()?;
            options
                .set_undo_auto_group_ms((ms >= 0).then(|| u32::try_from(ms).unwrap_or(u32::MAX)));
        }
        "whichwrap" | "ww" => options.set_whichwrap(value),
        "backspace" | "bs" => options.set_backspace(value),
        "virtualedit" | "ve" => options.set_virtualedit(value),
        "selection" | "sel" => options.set_selection(value),
        "clipboard" | "cb" => options.set_clipboard(value),
        "iskeyword" | "isk" => options.set_iskeyword(value),
        "inccommand" | "icm" => options.set_inccommand(value),
        "langmap" | "lmap" => options.set_langmap(value),
        "multilinefindrange" | "mlfr" => options.set_multiline_find_range(number()?),
        "softtabstop" | "sts" => {
            options.set_softtabstop(value.parse::<i32>().map_err(|_| NUMBER_REQUIRED)?);
        }
        "belloff" | "bo" => match value {
            "" => options.set_belloff(false),
            "all" => options.set_belloff(true),
            _ => return Err("E474: Invalid argument"),
        },
        _ => return Err("E518: Unknown option"),
    }
    Ok(())
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
        MapModePrefix::InsertCommand => {
            smallvec::smallvec![MappingMode::Insert, MappingMode::Command]
        }
    }
}

/// A right-hand side of `<Nop>`, in any case, is empty, as `do_map()`
/// reads it; `<Nop> ` and `<Nop>x` are keys.
const fn nop_as_nothing(rhs: &str) -> &str {
    if rhs.eq_ignore_ascii_case("<nop>") {
        ""
    } else {
        rhs
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
    buffer: bool,
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
            rhs_raw: CompactString::from(nop_as_nothing(rhs)),
            kind,
            flags,
            buffer,
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
    buffer: bool,
) -> Result<ExExecutionOutput, VimError> {
    let modes = modes_for_prefix(mode_prefix);

    Ok(ExExecutionOutput {
        mapping_changes: smallvec::smallvec![MappingChange::Remove {
            modes,
            lhs_raw: CompactString::from(lhs),
            buffer,
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
    buffer: bool,
) -> Result<ExExecutionOutput, VimError> {
    let modes = modes_for_prefix(mode_prefix);

    Ok(ExExecutionOutput {
        mapping_changes: smallvec::smallvec![MappingChange::ClearAll { modes, buffer }],
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
                    // 'v' maps to Visual, which covers Visual and Select
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
            ("formatoptions", OptionId::FormatOptions),
            ("comments", OptionId::Comments),
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
    fn option_name_formatting_short_forms() {
        assert_eq!(option_name_to_id("fo"), Some(OptionId::FormatOptions));
        assert_eq!(option_name_to_id("com"), Some(OptionId::Comments));
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
        assert!(set_bool_option(&mut opts, "expandtab", true));
        assert!(opts.expandtab());
    }

    #[test]
    fn set_bool_option_expandtab_false() {
        let mut opts = crate::primitives::VimOptions::default();
        opts.set_expandtab(true);
        assert!(set_bool_option(&mut opts, "expandtab", false));
        assert!(!opts.expandtab());
    }

    #[test]
    fn set_bool_option_unknown_is_refused() {
        let mut opts = crate::primitives::VimOptions::default();
        assert!(!set_bool_option(&mut opts, "nonexistent", true));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // assign_option
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn assign_option_tabstop_valid() {
        let mut opts = crate::primitives::VimOptions::default();
        assert_eq!(assign_option(&mut opts, "tabstop", "4"), Ok(()));
        assert_eq!(opts.tabstop(), 4);
    }

    #[test]
    fn assign_option_tabstop_invalid() {
        let mut opts = crate::primitives::VimOptions::default();
        assert_eq!(
            assign_option(&mut opts, "tabstop", "abc"),
            Err("E521: Number required after =")
        );
    }

    #[test]
    fn assign_option_clipboard() {
        let mut opts = crate::primitives::VimOptions::default();
        assert_eq!(assign_option(&mut opts, "clipboard", "unnamed"), Ok(()));
    }

    #[test]
    fn assign_option_unknown_returns_error() {
        let mut opts = crate::primitives::VimOptions::default();
        assert_eq!(
            assign_option(&mut opts, "nonexistent", "value"),
            Err("E518: Unknown option")
        );
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
        let output = execute_ex_piece("undojoin", &ctx, &mut seq);
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
        let output = execute_ex_piece("undoj", &ctx, &mut seq);
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
        let output = execute_ex_piece("s/foo/baz/", &ctx, &mut seq);
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
        let output = execute_ex_piece("delete", &ctx, &mut seq);
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
        let output = execute_ex_piece("set tabstop=4", &ctx, &mut seq);
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
        let output = execute_ex_piece("undojoin", &ctx, &mut seq);
        assert!(
            output.last_ex_for_dot.is_none(),
            ":undojoin should NOT populate last_ex_for_dot (not text-mutating)"
        );
    }
}
