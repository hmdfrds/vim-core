//! Command-line lifecycle and search/ex execution for [`VimEngine`].

use super::{CommandLineSession, Response, VimEngine};
use crate::commands::ex::effects as ex_effects;
use crate::commands::helpers;
use crate::document::Document;
use crate::effects::{Effect, Effects};
use crate::execution::host::CmdlineCompletionKind;
use crate::execution::HostRequest;
use crate::grammar::ex_parser::parse_ex_command;
use crate::grammar::types::{ExCommand, LineSpec};
use crate::primitives::byte_delta;
use crate::primitives::{Direction, Range, RegisterName};
use compact_str::CompactString;

use crate::execution::{InputContext, Validated};
use crate::grammar::CommandLineIntent;

use crate::primitives::Mode;
use crate::state::CommandLinePrompt;

impl VimEngine {
    /// Execute an Ex command directly without going through the interactive
    /// command-line widget lifecycle.
    pub fn execute_ex<D: Document>(
        &mut self,
        input: &str,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        let session = command_line_session(None, self.state.mode(), 1);
        self.execute_ex_command_line(input, session, ctx)
    }

    /// Handle a key while awaiting a register name in command-line mode.
    ///
    /// After Ctrl-R is pressed, the next key selects which register to insert.
    /// Special sub-commands: Ctrl-W inserts word under cursor, Ctrl-A inserts
    /// WORD under cursor. Escape cancels the sub-state.
    pub(super) fn handle_command_line_register_key(
        &mut self,
        key: crate::keymap::KeyEvent,
        doc_text: &str,
        cursor_offset: usize,
    ) -> Response {
        use crate::keymap::{Key, Modifiers};

        self.state.command_line_mut().set_awaiting_register(false);

        // Escape or Ctrl-C cancels the register sub-state (no insertion)
        if key.key == Key::Escape || key == crate::keymap::KeyEvent::ctrl('c') {
            return self.register_noop_response();
        }

        // Extract the char from the key; non-char keys are ignored
        let Key::Char(ch) = key.key else {
            return self.register_noop_response();
        };

        // Ctrl-W: insert word under cursor
        if ch == 'w' && key.modifiers == Modifiers::CTRL {
            let word = crate::commands::motions::word_under_cursor(
                doc_text,
                cursor_offset,
                &crate::primitives::WordCharSet::default_vim(),
            );
            if !word.is_empty() {
                self.state.command_line_mut().insert_str(word);
            }
            return self.register_sync_response();
        }

        // Ctrl-A: insert WORD (whitespace-delimited) under cursor
        if ch == 'a' && key.modifiers == Modifiers::CTRL {
            let big_word = big_word_under_cursor(doc_text, cursor_offset);
            if !big_word.is_empty() {
                self.state.command_line_mut().insert_str(big_word);
            }
            return self.register_sync_response();
        }

        // Regular register insertion: validate register name
        let Some(reg) = RegisterName::new(ch) else {
            return self.register_noop_response();
        };

        // Resolve register text and insert
        if let Some(content) = self.state.registers().get_aliased(reg, &self.options) {
            let text = content.text();
            if !text.is_empty() {
                // Strip newlines — command-line is single-line
                let cleaned: String = text.chars().filter(|&c| c != '\n' && c != '\r').collect();
                self.state.command_line_mut().insert_str(&cleaned);
            }
        }

        self.register_sync_response()
    }

    /// Build a pending response with sync request (register sub-state no-op).
    fn register_noop_response(&mut self) -> Response {
        let mut response = Response::pending_response();
        response
            .host_requests
            .push(self.sync_command_line_request());
        response
    }

    /// Build a pending response with sync and optional search preview update.
    fn register_sync_response(&mut self) -> Response {
        let mut response = Response::pending_response();
        response
            .host_requests
            .push(self.sync_command_line_request());

        // Update live search preview if in search prompt
        let input = self.state.command_line().input().to_owned();
        let prompt = self.state.command_line().prompt();
        match prompt {
            CommandLinePrompt::SearchForward | CommandLinePrompt::SearchBackward => {
                let direction = if prompt == CommandLinePrompt::SearchForward {
                    Direction::Forward
                } else {
                    Direction::Backward
                };
                if !input.is_empty() {
                    response
                        .effects
                        .extend(ex_effects::set_search_pattern(&input, direction));
                }
            }
            CommandLinePrompt::Ex | CommandLinePrompt::ExVisual => {}
        }

        response
    }

    /// Handle tab-completion in the command line.
    ///
    /// Uses [`resolve_completion_context()`] to determine what kind of
    /// completion is appropriate (command name, file path, setting, etc.)
    /// and then either computes matches locally or defers to the host.
    ///
    /// Currently only command-name completion is fully implemented.
    /// Argument completion (file paths, buffers, settings) will be wired
    /// in Tasks 5 and 6.
    pub(super) fn handle_tab_completion(
        &mut self,
        edit: crate::primitives::CommandLineEdit,
    ) -> Response {
        use crate::commands::ex::completion::{
            resolve_completion_context, ArgCompletionKind, CompletionContext,
        };

        let direction = if matches!(edit, crate::primitives::CommandLineEdit::CompleteNext) {
            crate::primitives::Direction::Forward
        } else {
            crate::primitives::Direction::Backward
        };

        // If a completion session is already active (cached candidates from a
        // previous Tab or a fulfilled host request), just cycle through the
        // existing candidates without re-resolving or re-requesting.
        if self.state.command_line().is_completing() {
            // `complete_cycle` with empty candidates is a no-op for initialization
            // but correctly cycles when the state is already active.
            self.state
                .command_line_mut()
                .complete_cycle(direction, &[], 0..0);
            let mut response = Response::pending_response();
            response
                .host_requests
                .push(self.sync_command_line_request());
            return response;
        }

        let input = self.state.command_line().input().to_owned();
        let cursor = self.state.command_line().cursor();
        let context = resolve_completion_context(&input, cursor);

        match context {
            CompletionContext::CommandName {
                ref prefix,
                ref replace_range,
            } => {
                let raw_matches = crate::commands::ex::completion::complete_ex_command(prefix);
                let candidates: Vec<crate::state::CompletionCandidate> = raw_matches
                    .iter()
                    .map(|&s| crate::state::CompletionCandidate {
                        text: compact_str::CompactString::from(s),
                        description: None,
                        detail: None,
                    })
                    .collect();
                self.state.command_line_mut().complete_cycle(
                    direction,
                    &candidates,
                    replace_range.clone(),
                );
            }
            CompletionContext::Argument {
                kind: ArgCompletionKind::Setting,
                ref arg_prefix,
                ref replace_range,
            } => {
                let candidates = crate::commands::ex::completion::complete_setting_name(
                    arg_prefix,
                    &self.options,
                );
                self.state.command_line_mut().complete_cycle(
                    direction,
                    &candidates,
                    replace_range.clone(),
                );
            }
            CompletionContext::Argument {
                kind: ArgCompletionKind::SettingValue { ref option },
                ref arg_prefix,
                ref replace_range,
            } => {
                let candidates = crate::commands::ex::completion::complete_setting_value(
                    option,
                    arg_prefix,
                    &self.options,
                );
                self.state.command_line_mut().complete_cycle(
                    direction,
                    &candidates,
                    replace_range.clone(),
                );
            }
            CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ref arg_prefix,
                ref replace_range,
            } => {
                // Store context so the fulfillment handler can call complete_cycle.
                self.state
                    .command_line_mut()
                    .set_pending_completion_context(direction, replace_range.clone());
                let meta = self.host.sequencer.next_meta();
                let request = HostRequest::RequestCmdlineCompletion {
                    meta,
                    kind: CmdlineCompletionKind::FilePath,
                    prefix: CompactString::from(arg_prefix.as_str()),
                    replace_range_start: replace_range.start,
                    replace_range_end: replace_range.end,
                };
                let mut response = Response::pending_response();
                response.host_requests.push(request);
                response
                    .host_requests
                    .push(self.sync_command_line_request());
                return response;
            }
            CompletionContext::Argument {
                kind: ArgCompletionKind::Buffer,
                ref arg_prefix,
                ref replace_range,
            } => {
                // Store context so the fulfillment handler can call complete_cycle.
                self.state
                    .command_line_mut()
                    .set_pending_completion_context(direction, replace_range.clone());
                let meta = self.host.sequencer.next_meta();
                let request = HostRequest::RequestCmdlineCompletion {
                    meta,
                    kind: CmdlineCompletionKind::Buffer,
                    prefix: CompactString::from(arg_prefix.as_str()),
                    replace_range_start: replace_range.start,
                    replace_range_end: replace_range.end,
                };
                let mut response = Response::pending_response();
                response.host_requests.push(request);
                response
                    .host_requests
                    .push(self.sync_command_line_request());
                return response;
            }
            CompletionContext::Argument {
                kind: ArgCompletionKind::Action,
                ref arg_prefix,
                ref replace_range,
            } => {
                self.state
                    .command_line_mut()
                    .set_pending_completion_context(direction, replace_range.clone());
                let meta = self.host.sequencer.next_meta();
                let request = HostRequest::RequestCmdlineCompletion {
                    meta,
                    kind: CmdlineCompletionKind::Action,
                    prefix: CompactString::from(arg_prefix.as_str()),
                    replace_range_start: replace_range.start,
                    replace_range_end: replace_range.end,
                };
                let mut response = Response::pending_response();
                response.host_requests.push(request);
                response
                    .host_requests
                    .push(self.sync_command_line_request());
                return response;
            }
            CompletionContext::None => {
                // No meaningful completion possible.
            }
        }

        let mut response = Response::pending_response();
        response
            .host_requests
            .push(self.sync_command_line_request());
        response
    }

    /// Handle Ctrl-D: list all matching completions at once.
    ///
    /// Unlike Tab (which cycles one-by-one), Ctrl-D shows all matching
    /// completions as a message. This matches Vim's `c_CTRL-D` behavior.
    pub(super) fn handle_list_completions(&mut self) -> Response {
        use crate::commands::ex::completion::{
            resolve_completion_context, ArgCompletionKind, CompletionContext,
        };

        let input = self.state.command_line().input().to_owned();
        let cursor = self.state.command_line().cursor();
        let context = resolve_completion_context(&input, cursor);

        let matches: Vec<String> = match context {
            CompletionContext::CommandName { ref prefix, .. } => {
                crate::commands::ex::completion::complete_ex_command(prefix)
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect()
            }
            CompletionContext::Argument {
                kind: ArgCompletionKind::Setting,
                ref arg_prefix,
                ..
            } => crate::commands::ex::completion::complete_setting_name(arg_prefix, &self.options)
                .iter()
                .map(|c| c.text.to_string())
                .collect(),
            CompletionContext::Argument {
                kind: ArgCompletionKind::SettingValue { ref option },
                ref arg_prefix,
                ..
            } => crate::commands::ex::completion::complete_setting_value(
                option,
                arg_prefix,
                &self.options,
            )
            .iter()
            .map(|c| c.text.to_string())
            .collect(),
            _ => Vec::new(),
        };

        let mut response = Response::pending_response();
        if !matches.is_empty() {
            let listing = matches.join("\n");
            response.effects.push(crate::effects::Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(compact_str::CompactString::from(listing)),
            });
        }
        response
            .host_requests
            .push(self.sync_command_line_request());
        response
    }

    /// Commit command-line text provided by the host widget.
    ///
    /// This is the host-side fallback path for paste/IME input. Normal
    /// command-line editing still flows through `process()` key by key.
    pub fn submit_command_line_text<D: Document>(
        &mut self,
        input: &str,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        if self.command_line_session.is_none() {
            return self.execute_ex(input, ctx);
        }

        self.state.command_line_mut().clear();
        for ch in input.chars() {
            self.state.command_line_mut().insert_char(ch);
        }

        self.commit_command_line(ctx)
    }

    /// Replace the active command-line buffer from host-owned widget text.
    ///
    /// This is the non-committing fallback path for paste/IME input when the
    /// host widget mutates text without per-key events reaching `process()`.
    pub fn replace_command_line_text<D: Document>(
        &mut self,
        input: &str,
        ctx: &InputContext<'_, D, Validated>,
    ) -> Response {
        let Some(session) = self.command_line_session else {
            return Response::ignored();
        };

        self.state.command_line_mut().clear();
        for ch in input.chars() {
            self.state.command_line_mut().insert_char(ch);
        }

        let mut response = Response::ignored();
        match session.prompt {
            CommandLinePrompt::SearchForward => {
                response.effects.extend(live_search_preview_effects(
                    ctx,
                    input,
                    Direction::Forward,
                ));
            }
            CommandLinePrompt::SearchBackward => {
                response.effects.extend(live_search_preview_effects(
                    ctx,
                    input,
                    Direction::Backward,
                ));
            }
            CommandLinePrompt::Ex | CommandLinePrompt::ExVisual => {
                if self.options.inccommand_enabled() {
                    response
                        .effects
                        .extend(self.live_substitute_preview_effects(input, ctx));
                }
            }
        }
        response
            .host_requests
            .push(self.sync_command_line_request());
        response
    }

    pub(super) fn enter_command_line(
        &mut self,
        intent: Option<CommandLineIntent>,
        count: Option<u32>,
    ) -> Response {
        let session = command_line_session(intent, self.state.mode(), count.unwrap_or(1));
        self.command_line_session = Some(session);
        self.state.command_line_mut().begin(session.prompt);
        // Pre-fill visual range when entering from visual ':'
        if session.prompt == CommandLinePrompt::ExVisual {
            for c in "'<,'>".chars() {
                self.state.command_line_mut().insert_char(c);
            }
        }
        self.state.set_mode(Mode::CommandLine);
        let mut response = Response::with_effects(ex_effects::enter_command_line());
        response
            .host_requests
            .push(self.sync_command_line_request());
        response
    }

    pub(super) fn cancel_command_line(&mut self) -> Response {
        let session = self.command_line_session;
        let sync_req = self.sync_command_line_request();
        self.command_line_session = None;
        self.parser.reset();
        self.state.command_line_mut().clear();
        // Return to visual mode if command-line was entered from visual ':'
        let return_mode = match session {
            Some(s) if s.entered_from_mode.is_visual() => s.entered_from_mode,
            _ => Mode::Normal,
        };
        self.state.set_mode(return_mode);
        let effects = match session {
            Some(session)
                if matches!(
                    session.prompt,
                    CommandLinePrompt::SearchForward | CommandLinePrompt::SearchBackward
                ) =>
            {
                let mut effects = ex_effects::cancel_command_line();
                effects.extend(ex_effects::clear_highlights());
                effects
            }
            Some(session)
                if matches!(
                    session.prompt,
                    CommandLinePrompt::Ex | CommandLinePrompt::ExVisual
                ) && self.options.inccommand_enabled() =>
            {
                let mut effects = ex_effects::cancel_command_line();
                effects.extend(Effects::new().clear_substitute_preview());
                effects
            }
            _ => ex_effects::cancel_command_line(),
        };
        let mut response = Response::with_effects(effects);
        response.host_requests.push(sync_req);
        response
    }

    pub(super) fn commit_command_line<D: Document>(
        &mut self,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        let input = self.state.command_line().input().to_owned();
        let session = self.command_line_session.take().unwrap_or_else(|| {
            debug_assert!(false, "commit_command_line called without active session");
            command_line_session(
                Some(CommandLineIntent {
                    prompt: self.state.command_line().prompt(),
                    operator_search: None,
                }),
                Mode::Normal,
                1,
            )
        });
        self.state.command_line_mut().commit();
        self.parser.reset();

        match session.prompt {
            CommandLinePrompt::Ex | CommandLinePrompt::ExVisual => {
                self.execute_ex_command_line(&input, session, ctx)
            }
            CommandLinePrompt::SearchForward => {
                self.execute_search_command_line(&input, session, ctx, Direction::Forward)
            }
            CommandLinePrompt::SearchBackward => {
                self.execute_search_command_line(&input, session, ctx, Direction::Backward)
            }
        }
    }

    /// Compute live substitute preview effects for inccommand.
    ///
    /// Tries to parse the current command-line input as a `:s` command. If the
    /// input is a substitute with a non-empty pattern, returns
    /// `SubstitutePreview` + search highlight effects. Otherwise returns
    /// `ClearSubstitutePreview` so that stale previews are removed when the
    /// user switches to a non-substitute command (e.g. `:w`).
    ///
    /// Follows the same pattern as [`live_search_preview_effects()`] for `/`
    /// and `?` search mode.
    pub(super) fn live_substitute_preview_effects<D: Document>(
        &self,
        input: &str,
        ctx: &InputContext<'_, D, Validated>,
    ) -> Effects {
        use crate::primitives::IncCommandMode;
        let include_original_lines =
            matches!(self.options.inccommand_mode(), IncCommandMode::Split);
        live_substitute_preview_effects_impl(
            input,
            ctx.doc().text(),
            ctx.cursor_offset_raw(),
            self.options.gdefault(),
            include_original_lines,
        )
    }

    /// Accept a specific completion candidate by index.
    ///
    /// Called from the WASM export when the user clicks a QuickPick item.
    /// Replaces the completion portion of the input with the selected
    /// candidate's text and clears completion state.
    pub(crate) fn accept_cmdline_completion(&mut self, index: usize) -> Response {
        self.state.command_line_mut().accept_completion(index);
        let mut response = Response::pending_response();
        response
            .host_requests
            .push(self.sync_command_line_request());
        response
    }

    /// Build a `SyncCommandLine` host request from current engine state.
    ///
    /// Emitted at each command-line lifecycle point (enter, edit, cancel)
    /// so the host can synchronize its command-line UI.
    pub(super) fn sync_command_line_request(&mut self) -> HostRequest {
        let meta = self.host.sequencer.next_meta();
        HostRequest::SyncCommandLine {
            meta,
            prompt: self.state.command_line().prompt(),
            input: CompactString::from(self.state.command_line().input()),
            cursor: self.state.command_line().cursor(),
        }
    }
}

fn live_search_preview_effects<D: Document>(
    ctx: &InputContext<'_, D, Validated>,
    input: &str,
    direction: Direction,
) -> Effects {
    if input.is_empty() {
        return ex_effects::clear_highlights();
    }

    let mut effects = ex_effects::set_search_pattern(input, direction);
    let ranges = collect_search_match_ranges(ctx, input);
    if ranges.is_empty() {
        effects.extend(ex_effects::clear_highlights());
    } else {
        effects.extend(ex_effects::highlight_matches(ranges));
    }
    effects
}

/// Maximum number of preview matches to compute (bounds computation time).
const MAX_PREVIEW_MATCHES: usize = 1000;

/// Compute live substitute preview effects from command-line input.
///
/// This is the core implementation used by
/// [`VimEngine::live_substitute_preview_effects()`]. It is a free function
/// to make unit testing easier (no `VimEngine` required).
fn live_substitute_preview_effects_impl(
    input: &str,
    doc_text: &str,
    cursor_offset: usize,
    gdefault: bool,
    include_original_lines: bool,
) -> Effects {
    let Some((range, pattern, replacement, flags)) = parse_substitute_input(input) else {
        return Effects::new().clear_substitute_preview();
    };

    let cursor_line = helpers::line_of(doc_text, cursor_offset);
    let total_lines = helpers::line_count(doc_text).max(1);
    let (start_line, end_line) = resolve_preview_range(&range, cursor_line, total_lines);

    // XOR with gdefault — same logic as substitute() for consistency
    let effective_global = flags.global() ^ gdefault;

    let preview = ex_effects::substitute_preview(
        doc_text,
        start_line,
        end_line,
        &pattern,
        &replacement,
        effective_global,
        MAX_PREVIEW_MATCHES,
        include_original_lines,
    );

    build_preview_effects(&pattern, preview)
}

/// Try to parse command-line input as a substitute command with a non-empty pattern.
///
/// Returns `None` for non-substitute commands, parse errors, or empty patterns.
fn parse_substitute_input(
    input: &str,
) -> Option<(
    crate::grammar::types::ExRange,
    CompactString,
    CompactString,
    crate::primitives::SubFlags,
)> {
    if input.is_empty() {
        return None;
    }
    let ex_cmd = parse_ex_command(input).ok()?;
    if let ExCommand::Substitute {
        range,
        pattern,
        replacement,
        flags,
    } = ex_cmd
    {
        if !pattern.is_empty() {
            return Some((range, pattern, replacement, flags));
        }
    }
    None
}

/// Combine search highlighting and substitute preview into a single effects sequence.
fn build_preview_effects(pattern: &str, preview: Effects) -> Effects {
    let has_matches = preview
        .iter()
        .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
    let mut effects = ex_effects::set_search_pattern(pattern, Direction::Forward);

    if has_matches {
        let highlight_ranges: Vec<Range> = preview
            .iter()
            .filter_map(|e| match e {
                Effect::SubstitutePreview { matches } => Some(
                    matches
                        .iter()
                        .map(|m| Range::new(m.match_start(), m.match_end())),
                ),
                _ => None,
            })
            .flatten()
            .collect();
        effects.extend(ex_effects::highlight_matches(highlight_ranges));
    }

    effects.extend(preview);
    effects
}

/// Resolve an [`ExRange`] to concrete 0-indexed line numbers for preview.
///
/// Handles common cases directly:
/// - `.` (Current) → cursor line
/// - `$` (Last) → last line
/// - `%` (1,$) → entire file
/// - Absolute numbers → converted from 1-indexed
/// - Relative offsets → relative to cursor
///
/// For complex specs (marks, search patterns) that would require a full
/// `ExContext`, falls back to the cursor's line to avoid errors during typing.
fn resolve_preview_range(
    range: &crate::grammar::types::ExRange,
    cursor_line: usize,
    total_lines: usize,
) -> (usize, usize) {
    let resolve_spec = |spec: &LineSpec| -> Option<usize> {
        match spec {
            LineSpec::Current => Some(cursor_line),
            LineSpec::Last => Some(total_lines.saturating_sub(1)),
            LineSpec::Absolute(n) => {
                let line = (*n as usize).saturating_sub(1);
                // Clamp to valid range for preview (user may type a large number)
                Some(line.min(total_lines.saturating_sub(1)))
            }
            LineSpec::Relative(offset) => {
                let line = byte_delta::to_i64(cursor_line).saturating_add(i64::from(*offset));
                usize::try_from(line).ok().filter(|l| *l < total_lines)
            }
            // Marks, search patterns — fall back to None (use cursor line)
            _ => None,
        }
    };

    let start = resolve_spec(&range.start).unwrap_or(cursor_line);
    let end = range.end.as_ref().and_then(resolve_spec).unwrap_or(start);

    // Ensure start <= end; if reversed, swap (user may be typing)
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    // Clamp to valid range
    (
        lo.min(total_lines.saturating_sub(1)),
        hi.min(total_lines.saturating_sub(1)),
    )
}

fn collect_search_match_ranges<D: Document>(
    ctx: &InputContext<'_, D, Validated>,
    pattern: &str,
) -> Vec<Range> {
    if pattern.is_empty() {
        return Vec::new();
    }

    if let Some(search) = ctx.providers().search {
        use crate::execution::safety_harness::validated_range;
        let text_len = ctx.doc().text().len();
        return search
            .find_matches(pattern, text_len)
            .into_iter()
            .map(|range| validated_range(range.start().get(), range.end().get(), text_len))
            .filter(|range| !range.is_empty())
            .collect();
    }

    ctx.doc()
        .text()
        .match_indices(pattern)
        .map(|(start, matched)| Range::from_raw(start, start + matched.len()))
        .collect()
}

const fn command_line_session(
    intent: Option<CommandLineIntent>,
    entered_from_mode: Mode,
    count: u32,
) -> CommandLineSession {
    match intent {
        Some(intent) => CommandLineSession {
            prompt: intent.prompt,
            entered_from_mode,
            intent: intent.operator_search,
            count,
        },
        None => CommandLineSession {
            prompt: CommandLinePrompt::Ex,
            entered_from_mode,
            intent: None,
            count,
        },
    }
}

/// Extract the WORD (whitespace-delimited token) at `cursor` for Ctrl-R Ctrl-A.
fn big_word_under_cursor(text: &str, cursor: usize) -> &str {
    if cursor >= text.len() || !text.is_char_boundary(cursor) {
        return "";
    }

    let ch = match text[cursor..].chars().next() {
        Some(c) if !c.is_whitespace() => c,
        _ => return "",
    };

    // Walk backward to find start of WORD
    let mut start = cursor;
    for (idx, c) in text[..cursor].char_indices().rev() {
        if c.is_whitespace() {
            start = idx + c.len_utf8();
            break;
        }
        start = idx;
    }

    // Walk forward to find end of WORD
    let after = cursor + ch.len_utf8();
    let mut end = after;
    for (idx, c) in text[after..].char_indices() {
        if c.is_whitespace() {
            end = after + idx;
            break;
        }
        end = after + idx + c.len_utf8();
    }

    &text[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::Offset;

    // ── live_substitute_preview_effects_impl ──────────────────────────

    #[test]
    fn preview_basic_substitute_produces_preview_effect() {
        let text = "foo bar foo";
        let effects = live_substitute_preview_effects_impl("s/foo/baz", text, 0, false, false);
        let has_preview = effects
            .iter()
            .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
        assert!(has_preview, "expected SubstitutePreview effect");
    }

    #[test]
    fn preview_basic_substitute_has_correct_matches() {
        let text = "foo bar foo";
        let effects = live_substitute_preview_effects_impl("s/foo/baz", text, 0, false, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                assert_eq!(matches.len(), 1, "non-global should match once");
                assert_eq!(matches[0].match_start(), Offset::new(0));
                assert_eq!(matches[0].match_end(), Offset::new(3));
                assert_eq!(matches[0].replacement(), "baz");
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }

    #[test]
    fn preview_empty_pattern_emits_clear() {
        let text = "foo bar";
        let effects = live_substitute_preview_effects_impl("s/", text, 0, false, false);
        let has_clear = effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSubstitutePreview));
        assert!(has_clear, "empty pattern should clear preview");
        let has_preview = effects
            .iter()
            .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
        assert!(
            !has_preview,
            "empty pattern should NOT produce SubstitutePreview"
        );
    }

    #[test]
    fn preview_not_substitute_emits_clear() {
        let text = "foo bar";
        let effects = live_substitute_preview_effects_impl("w", text, 0, false, false);
        let has_clear = effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSubstitutePreview));
        assert!(has_clear, "non-substitute should clear preview");
    }

    #[test]
    fn preview_empty_input_emits_clear() {
        let text = "foo bar";
        let effects = live_substitute_preview_effects_impl("", text, 0, false, false);
        let has_clear = effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSubstitutePreview));
        assert!(has_clear, "empty input should clear preview");
    }

    #[test]
    fn preview_global_flag_finds_multiple() {
        let text = "foo bar foo";
        let effects = live_substitute_preview_effects_impl("s/foo/baz/g", text, 0, false, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                assert_eq!(matches.len(), 2, "global flag should match all occurrences");
                assert_eq!(matches[0].match_start(), Offset::new(0));
                assert_eq!(matches[1].match_start(), Offset::new(8));
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }

    #[test]
    fn preview_with_replacement_text() {
        let text = "hello world";
        let effects =
            live_substitute_preview_effects_impl("s/hello/GOODBYE", text, 0, false, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].replacement(), "GOODBYE");
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }

    #[test]
    fn preview_emits_set_search_pattern() {
        let text = "foo bar";
        let effects = live_substitute_preview_effects_impl("s/foo/bar", text, 0, false, false);
        let has_search = effects.iter().any(
            |e| matches!(e, Effect::SetSearchPattern { pattern, .. } if pattern.as_str() == "foo"),
        );
        assert!(
            has_search,
            "should emit SetSearchPattern for the substitute pattern"
        );
    }

    #[test]
    fn preview_emits_highlight_matches() {
        let text = "foo bar";
        let effects = live_substitute_preview_effects_impl("s/foo/bar", text, 0, false, false);
        let has_highlight = effects
            .iter()
            .any(|e| matches!(e, Effect::HighlightMatches { .. }));
        assert!(
            has_highlight,
            "should emit HighlightMatches for search visualization"
        );
    }

    #[test]
    fn preview_no_match_emits_clear_preview() {
        let text = "foo bar";
        let effects = live_substitute_preview_effects_impl("s/xyz/bar", text, 0, false, false);
        let has_clear = effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSubstitutePreview));
        assert!(has_clear, "no-match substitute should clear preview");
        let has_preview = effects
            .iter()
            .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
        assert!(
            !has_preview,
            "no-match substitute should NOT produce SubstitutePreview"
        );
    }

    #[test]
    fn preview_no_match_still_emits_search_pattern() {
        let text = "foo bar";
        let effects = live_substitute_preview_effects_impl("s/xyz/bar", text, 0, false, false);
        let has_search = effects.iter().any(
            |e| matches!(e, Effect::SetSearchPattern { pattern, .. } if pattern.as_str() == "xyz"),
        );
        assert!(
            has_search,
            "no-match should still set search pattern for hlsearch"
        );
    }

    #[test]
    fn preview_percent_range_searches_all_lines() {
        let text = "foo\nbar\nfoo";
        let effects = live_substitute_preview_effects_impl("%s/foo/X/g", text, 0, false, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                assert_eq!(matches.len(), 2, "% range should find matches on all lines");
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }

    #[test]
    fn preview_current_line_only_by_default() {
        // Cursor on line 0 ("foo"), line 1 also has "foo"
        let text = "foo\nfoo";
        let effects = live_substitute_preview_effects_impl("s/foo/X", text, 0, false, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                assert_eq!(matches.len(), 1, "no range should only search cursor line");
                assert_eq!(matches[0].match_start(), Offset::new(0));
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }

    #[test]
    fn preview_current_line_respects_cursor_position() {
        // Cursor at offset 4 → line 1 ("foo")
        let text = "bar\nfoo";
        let effects = live_substitute_preview_effects_impl("s/foo/X", text, 4, false, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].match_start(), Offset::new(4));
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }

    #[test]
    fn preview_absolute_line_range() {
        let text = "aaa\nbbb\nccc";
        // :1,2s/a\|b/X/g → lines 0-1
        let effects = live_substitute_preview_effects_impl("1,2s/a\\|b/X/g", text, 0, false, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                // Line 0 has 3 'a's, line 1 has 3 'b's → 6 matches
                assert_eq!(matches.len(), 6);
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }

    #[test]
    fn preview_invalid_regex_emits_clear() {
        let text = "foo bar";
        // Unclosed bracket is invalid regex
        let effects = live_substitute_preview_effects_impl("s/[/bar", text, 0, false, false);
        let has_clear = effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSubstitutePreview));
        assert!(has_clear, "invalid regex should clear preview (graceful)");
    }

    // ── resolve_preview_range ────────────────────────────────────────────

    #[test]
    fn range_current_line() {
        use crate::grammar::types::ExRange;
        let (start, end) = resolve_preview_range(&ExRange::current_line(), 3, 10);
        assert_eq!(start, 3);
        assert_eq!(end, 3);
    }

    #[test]
    fn range_entire_file() {
        use crate::grammar::types::ExRange;
        let (start, end) = resolve_preview_range(&ExRange::entire_file(), 3, 10);
        assert_eq!(start, 0);
        assert_eq!(end, 9);
    }

    #[test]
    fn range_absolute_lines() {
        use crate::grammar::types::ExRange;
        let (start, end) = resolve_preview_range(&ExRange::lines(2, 5), 0, 10);
        assert_eq!(start, 1); // 1-indexed → 0-indexed
        assert_eq!(end, 4);
    }

    #[test]
    fn range_reversed_is_swapped() {
        use crate::grammar::types::ExRange;
        // Lines 5,2 is backwards — should be swapped for preview
        let (start, end) = resolve_preview_range(&ExRange::lines(5, 2), 0, 10);
        assert_eq!(start, 1);
        assert_eq!(end, 4);
    }

    #[test]
    fn range_clamped_to_total_lines() {
        use crate::grammar::types::ExRange;
        // Line 100 in a 5-line doc
        let (start, end) = resolve_preview_range(&ExRange::lines(1, 100), 0, 5);
        assert_eq!(start, 0);
        assert_eq!(end, 4);
    }

    #[test]
    fn progressive_typing_updates_preview() {
        let text = "foo fob bar";
        // Type "s/f" → matches "f" in "foo", "fob"
        let effects1 = live_substitute_preview_effects_impl("s/f/X", text, 0, false, false);
        let count1 = effects1
            .iter()
            .find_map(|e| {
                if let Effect::SubstitutePreview { matches } = e {
                    Some(matches.len())
                } else {
                    None
                }
            })
            .unwrap_or(0);

        // Type "s/fo" → matches "fo" in "foo", "fob"
        let effects2 = live_substitute_preview_effects_impl("s/fo/X", text, 0, false, false);
        let count2 = effects2
            .iter()
            .find_map(|e| {
                if let Effect::SubstitutePreview { matches } = e {
                    Some(matches.len())
                } else {
                    None
                }
            })
            .unwrap_or(0);

        // "f" appears once (non-global, first match only), "fo" also once
        assert_eq!(count1, 1);
        assert_eq!(count2, 1);

        // With global flag both patterns should find different counts
        let effects_g1 = live_substitute_preview_effects_impl("s/f/X/g", text, 0, false, false);
        let count_g1 = effects_g1
            .iter()
            .find_map(|e| {
                if let Effect::SubstitutePreview { matches } = e {
                    Some(matches.len())
                } else {
                    None
                }
            })
            .unwrap_or(0);

        let effects_g2 = live_substitute_preview_effects_impl("s/fo/X/g", text, 0, false, false);
        let count_g2 = effects_g2
            .iter()
            .find_map(|e| {
                if let Effect::SubstitutePreview { matches } = e {
                    Some(matches.len())
                } else {
                    None
                }
            })
            .unwrap_or(0);

        // "f" matches 2 times (foo, fob), "fo" matches 2 times (foo, fob)
        assert_eq!(count_g1, 2, "global 'f' should match 2 times on the line");
        assert_eq!(count_g2, 2, "global 'fo' should match 2 times on the line");
    }

    #[test]
    fn preview_just_delimiter_no_pattern_clears() {
        let text = "foo";
        // "s/" — delimiter present but pattern is empty
        let effects = live_substitute_preview_effects_impl("s/", text, 0, false, false);
        let has_clear = effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSubstitutePreview));
        assert!(has_clear, "s/ with empty pattern should clear preview");
    }

    #[test]
    fn preview_write_command_clears() {
        let text = "foo";
        let effects = live_substitute_preview_effects_impl("w myfile.txt", text, 0, false, false);
        let has_clear = effects
            .iter()
            .any(|e| matches!(e, Effect::ClearSubstitutePreview));
        assert!(has_clear, ":w should emit ClearSubstitutePreview");
    }

    // ── gdefault preview tests ──────────────────────────────────────────

    #[test]
    fn preview_gdefault_bare_substitute_matches_all() {
        // With gdefault=true, bare `:s/foo/bar` should match all "foo" on the line
        let text = "foo bar foo";
        let effects = live_substitute_preview_effects_impl("s/foo/baz", text, 0, true, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                assert_eq!(
                    matches.len(),
                    2,
                    "gdefault=true + no g flag should match all"
                );
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }

    #[test]
    fn preview_gdefault_g_flag_matches_first_only() {
        // With gdefault=true, `g` flag inverts: matches first occurrence only
        let text = "foo bar foo";
        let effects = live_substitute_preview_effects_impl("s/foo/baz/g", text, 0, true, false);
        for effect in effects.iter() {
            if let Effect::SubstitutePreview { matches } = effect {
                assert_eq!(
                    matches.len(),
                    1,
                    "gdefault=true + g flag should match first only"
                );
                return;
            }
        }
        panic!("no SubstitutePreview effect found");
    }
}
