//! Host request completion for [`VimEngine`].
//!
//! Handles the asynchronous host-request lifecycle: tracking pending
//! requests and completing them when results arrive from the shell.

use super::typeahead::{TypeaheadEntry, TypeaheadFlags};
use super::Response;
use crate::commands::ex::effects as ex_effects;
use crate::effects::Effects;
use crate::execution::executor_ex;
use crate::execution::{HostRequest, HostResult};
use crate::primitives::byte_delta;
use crate::primitives::Offset;

impl super::VimEngine {
    /// Track host requests from a response for later completion.
    pub(super) fn register_pending_host_requests(&mut self, mut response: Response) -> Response {
        // Convert effect-based host commands to HostRequests before tracking.
        self.route_effects_to_host_requests(&mut response);

        for request in &response.host_requests {
            self.host.pending.insert(request.id(), request.clone());
        }
        // Optimize effects before returning to the shell.
        crate::effects::Effects::<crate::effects::undo_state::Closed>::optimize_vec(
            &mut response.effects,
        );
        response
    }

    /// Convert effect variants that represent host commands into HostRequests.
    ///
    /// Window management, goto definition, show documentation, open command
    /// window, call operator func, and norm command effects are removed from
    /// the effects list and appended as HostRequests.
    ///
    /// Effects that map to EXISTING HostRequest variants (WindowSplit → SplitWindow,
    /// WindowClose → CloseWindow, WindowOnly → CloseOtherWindows) are also routed.
    ///
    /// Note: `HostAction` effects are NOT routed here because the action-key
    /// path in `VimEngine::process()` already creates a `RunAction` HostRequest
    /// directly. Routing here would create duplicates.
    fn route_effects_to_host_requests(&mut self, response: &mut Response) {
        use crate::effects::Effect;
        use crate::execution::host::SplitDirection;

        // Fast path: skip if no routable effects exist.
        let has_routable = response.effects.iter().any(|e| {
            matches!(
                e,
                Effect::WindowSplit
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
                    | Effect::WindowRotateDown
                    | Effect::WindowRotateUp
                    | Effect::WindowEqualSize
                    | Effect::WindowIncreaseHeight { .. }
                    | Effect::WindowDecreaseHeight { .. }
                    | Effect::WindowIncreaseWidth { .. }
                    | Effect::WindowDecreaseWidth { .. }
                    | Effect::GotoDefinition
                    | Effect::ShowDocumentation
                    | Effect::OpenCommandWindow { .. }
                    | Effect::CallOperatorFunc { .. }
                    | Effect::NormCommand { .. }
            )
        });
        if !has_routable {
            return;
        }

        // Two-pass: first collect requests, then remove effects.
        // This avoids borrowing response.effects and response.host_requests simultaneously.
        let mut routed_requests = Vec::new();
        for effect in &response.effects {
            let host_request = match effect {
                // Map to existing HostRequest variants
                Effect::WindowSplit => Some(HostRequest::SplitWindow {
                    meta: self.host.sequencer.next_meta(),
                    direction: SplitDirection::Horizontal,
                    path: None,
                    new_file: false,
                }),
                Effect::WindowVSplit => Some(HostRequest::SplitWindow {
                    meta: self.host.sequencer.next_meta(),
                    direction: SplitDirection::Vertical,
                    path: None,
                    new_file: false,
                }),
                Effect::WindowNew => Some(HostRequest::SplitWindow {
                    meta: self.host.sequencer.next_meta(),
                    direction: SplitDirection::Horizontal,
                    path: None,
                    new_file: true,
                }),
                Effect::WindowClose => Some(HostRequest::CloseWindow {
                    meta: self.host.sequencer.next_meta(),
                    force: false,
                }),
                Effect::WindowOnly => Some(HostRequest::CloseOtherWindows {
                    meta: self.host.sequencer.next_meta(),
                    force: false,
                }),

                // Map to new HostRequest variants
                Effect::WindowNext => Some(HostRequest::WindowNext {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowPrev => Some(HostRequest::WindowPrev {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowMoveLeft => Some(HostRequest::WindowMoveLeft {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowMoveRight => Some(HostRequest::WindowMoveRight {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowMoveUp => Some(HostRequest::WindowMoveUp {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowMoveDown => Some(HostRequest::WindowMoveDown {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowRotateDown => Some(HostRequest::WindowRotateDown {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowRotateUp => Some(HostRequest::WindowRotateUp {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowEqualSize => Some(HostRequest::WindowEqualSize {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::WindowIncreaseHeight { count } => Some(HostRequest::WindowIncreaseHeight {
                    meta: self.host.sequencer.next_meta(),
                    count: *count,
                }),
                Effect::WindowDecreaseHeight { count } => Some(HostRequest::WindowDecreaseHeight {
                    meta: self.host.sequencer.next_meta(),
                    count: *count,
                }),
                Effect::WindowIncreaseWidth { count } => Some(HostRequest::WindowIncreaseWidth {
                    meta: self.host.sequencer.next_meta(),
                    count: *count,
                }),
                Effect::WindowDecreaseWidth { count } => Some(HostRequest::WindowDecreaseWidth {
                    meta: self.host.sequencer.next_meta(),
                    count: *count,
                }),
                Effect::GotoDefinition => Some(HostRequest::GotoDefinition {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::ShowDocumentation => Some(HostRequest::ShowDocumentation {
                    meta: self.host.sequencer.next_meta(),
                }),
                Effect::OpenCommandWindow {
                    prompt,
                    history,
                    prefill,
                } => Some(HostRequest::OpenCommandWindow {
                    meta: self.host.sequencer.next_meta(),
                    prompt: *prompt,
                    history: history.clone(),
                    prefill: prefill.clone(),
                }),
                Effect::CallOperatorFunc { range, motion_type } => {
                    Some(HostRequest::CallOperatorFunc {
                        meta: self.host.sequencer.next_meta(),
                        range: *range,
                        motion_type: *motion_type,
                    })
                }
                Effect::NormCommand {
                    start_line,
                    end_line,
                    keys,
                    remap,
                } => Some(HostRequest::ExecuteNorm {
                    meta: self.host.sequencer.next_meta(),
                    start_line: byte_delta::to_u32(start_line.get()),
                    end_line: byte_delta::to_u32(end_line.get()),
                    keys: keys.clone(),
                    remap: *remap,
                }),
                _ => None,
            };
            if let Some(req) = host_request {
                routed_requests.push(req);
            }
        }

        if !routed_requests.is_empty() {
            // Remove the effects that were routed to host requests.
            response.effects.retain(|e| {
                !matches!(
                    e,
                    Effect::WindowSplit
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
                        | Effect::WindowRotateDown
                        | Effect::WindowRotateUp
                        | Effect::WindowEqualSize
                        | Effect::WindowIncreaseHeight { .. }
                        | Effect::WindowDecreaseHeight { .. }
                        | Effect::WindowIncreaseWidth { .. }
                        | Effect::WindowDecreaseWidth { .. }
                        | Effect::GotoDefinition
                        | Effect::ShowDocumentation
                        | Effect::OpenCommandWindow { .. }
                        | Effect::CallOperatorFunc { .. }
                        | Effect::NormCommand { .. }
                )
            });
            response.host_requests.extend(routed_requests);
        }
    }

    /// Complete a previously issued host request by ID.
    ///
    /// Requests may be completed in any order — the engine looks up the
    /// pending request by the ID embedded in `result` rather than requiring
    /// FIFO completion.
    ///
    /// # Complexity
    ///
    /// Time: O(E) where E = number of effects in the completion response.
    /// The pending request lookup is O(1) amortized (hash map remove).
    /// Effect synchronization and optimization are O(E).
    ///
    /// Space: O(E) for the response effects vector.
    pub fn complete_host_request(&mut self, result: &HostResult) -> Response {
        let id = result.id();
        let Some(pending) = self.host.pending.remove(&id) else {
            return Response::ignored()
                .with_message(format!("unexpected host result id {}", id.get()));
        };

        // ── <expr> mapping completion ────────────────────────────────
        // EvaluateMapping returns keys as a string. Parse them and inject
        // into the typeahead buffer so the engine processes them as if the
        // user typed them (or as noremap, depending on the mapping kind).
        if matches!(&pending, HostRequest::EvaluateMapping { .. }) {
            return self.complete_evaluate_mapping(&pending, result);
        }

        // ── Command-line completion fulfillment ─────────────────────
        // RequestCmdlineCompletion returns candidates as JSON. Parse them,
        // feed into complete_cycle(), and emit SyncCommandLine so the host
        // updates its QuickPick / command-line UI.
        if matches!(&pending, HostRequest::RequestCmdlineCompletion { .. }) {
            return self.complete_cmdline_completion(&pending, result);
        }

        let mut response = completion_response(&pending, result);

        let meta = pending.meta();
        if let Some(msg) = executor_ex::completion_message(result, &meta) {
            response = response.with_message(msg);
        }
        crate::execution::effect_processor::sync_effects(
            &mut self.state,
            &mut self.parser,
            &mut response.effects,
        );
        self.register_pending_host_requests(response)
    }

    /// Complete an `EvaluateMapping` host request.
    ///
    /// Parses the returned key string and injects the keys into the typeahead
    /// buffer. The mapping kind determines whether the keys are remappable
    /// (recursive mapping) or noremap (non-recursive mapping).
    ///
    /// For `:map <expr>` (recursive=true), returned keys go through mapping
    /// expansion again — matching Vim's behaviour where `map` RHS is remappable.
    /// For `:noremap <expr>` (recursive=false), returned keys bypass expansion.
    fn complete_evaluate_mapping(
        &mut self,
        pending: &HostRequest,
        result: &HostResult,
    ) -> Response {
        // Extract the `kind` and `silent` fields from the pending request.
        let recursive = matches!(
            pending,
            HostRequest::EvaluateMapping {
                kind: crate::keymap::MappingKind::Recursive,
                ..
            }
        );
        let silent = matches!(pending, HostRequest::EvaluateMapping { silent: true, .. });
        let mut rhs_flags = if recursive {
            TypeaheadFlags::recursive_rhs()
        } else {
            TypeaheadFlags::noremap_rhs()
        };
        // Propagate SILENT flag to all injected keys so ShowMessage effects
        // are suppressed during the mapping's execution.
        if silent {
            rhs_flags |= TypeaheadFlags::SILENT;
        }

        match result {
            HostResult::Data { data, .. }
            | HostResult::Success {
                message: Some(data),
                ..
            } => {
                if data.is_empty() {
                    return Response::consumed_empty();
                }
                // Parse the returned string as key events.
                // Use `parse_key_notation_sequence` with keymap access so that
                // `<Action>(Name)` resolves to a registered id instead of the
                // `u32::MAX` sentinel that `parse_keys_from_string` would emit.
                let keys = crate::execution::key_notation::parse_key_notation_sequence(
                    data,
                    Some(&mut self.keymap),
                );
                if keys.is_empty() {
                    return Response::consumed_empty();
                }
                // Inject into typeahead buffer front so they're processed next.
                let entries: Vec<TypeaheadEntry> = keys
                    .into_iter()
                    .map(|k| TypeaheadEntry::new(k, rhs_flags))
                    .collect();
                self.typeahead.buffer.inject_front(entries);
                // Return consumed — the injected keys will be processed on the
                // next drain_next_key() / process() call.
                Response::consumed_empty()
            }
            HostResult::Failure { error, .. } => Response::with_effects(ex_effects::show_error(
                crate::errors::VimError::HostFailure(error.to_string().into()),
            )),
            HostResult::Success { message: None, .. } => {
                // No data returned — expression evaluated to empty string.
                Response::consumed_empty()
            }
            _ => {
                let _ = pending; // acknowledged
                let effects = ex_effects::show_error(crate::errors::VimError::InternalError(
                    "EvaluateMapping completion payload mismatch".into(),
                ));
                Response::with_effects(effects)
            }
        }
    }

    /// Complete a `RequestCmdlineCompletion` host request.
    ///
    /// Receives pre-parsed candidates from the boundary layer (via
    /// `HostResult::CmdlineCompletionCandidates`), feeds them into
    /// `CommandLineState::complete_cycle()`, and emits a `SyncCommandLine`
    /// request so the host can update its UI.
    ///
    /// The direction (Tab vs Shift-Tab) and replace range were stored on
    /// `CommandLineState` when the request was emitted. If the stored context
    /// is missing (e.g. the user cancelled command-line mode before the
    /// response arrived), the fulfillment is silently ignored.
    fn complete_cmdline_completion(
        &mut self,
        pending: &HostRequest,
        result: &HostResult,
    ) -> Response {
        match result {
            HostResult::CmdlineCompletionCandidates { candidates, .. } => {
                if candidates.is_empty() {
                    // No matches — nothing to complete. Emit SyncCommandLine
                    // to keep the host UI in sync.
                    self.state
                        .command_line_mut()
                        .take_pending_completion_context();
                    let mut response = Response::pending_response();
                    response
                        .host_requests
                        .push(self.sync_command_line_request());
                    return self.register_pending_host_requests(response);
                }

                // Read the stored direction and replace range.
                let Some((direction, replace_range)) = self
                    .state
                    .command_line_mut()
                    .take_pending_completion_context()
                else {
                    // Context was cleared (e.g. user cancelled command-line
                    // before the host responded). Silently ignore.
                    return Response::consumed_empty();
                };

                // Convert CmdlineCompletionEntry to CompletionCandidate.
                let completion_candidates: Vec<crate::state::CompletionCandidate> = candidates
                    .iter()
                    .map(|c| crate::state::CompletionCandidate {
                        text: c.text.clone(),
                        description: c.description.clone(),
                        detail: c.detail.clone(),
                    })
                    .collect();

                // Feed into the cycling completion engine.
                self.state.command_line_mut().complete_cycle(
                    direction,
                    &completion_candidates,
                    replace_range,
                );

                // Emit SyncCommandLine so the host updates its UI.
                let mut response = Response::pending_response();
                response
                    .host_requests
                    .push(self.sync_command_line_request());
                self.register_pending_host_requests(response)
            }
            HostResult::Failure { error, .. } => {
                // Clear the pending context since the request failed.
                self.state
                    .command_line_mut()
                    .take_pending_completion_context();
                let mut response = Response::with_effects(ex_effects::show_error(
                    crate::errors::VimError::HostFailure(error.to_string().into()),
                ));
                crate::execution::effect_processor::sync_effects(
                    &mut self.state,
                    &mut self.parser,
                    &mut response.effects,
                );
                response
            }
            HostResult::Success { .. } => {
                // Success without data — treat as empty candidates.
                self.state
                    .command_line_mut()
                    .take_pending_completion_context();
                let mut response = Response::pending_response();
                response
                    .host_requests
                    .push(self.sync_command_line_request());
                self.register_pending_host_requests(response)
            }
            _ => {
                let _ = pending; // acknowledged
                self.state
                    .command_line_mut()
                    .take_pending_completion_context();
                let mut response = Response::with_effects(ex_effects::show_error(
                    crate::errors::VimError::InternalError(
                        "RequestCmdlineCompletion completion payload mismatch".into(),
                    ),
                ));
                crate::execution::effect_processor::sync_effects(
                    &mut self.state,
                    &mut self.parser,
                    &mut response.effects,
                );
                response
            }
        }
    }
}

fn completion_response(pending: &HostRequest, result: &HostResult) -> Response {
    match (pending, result) {
        (_, HostResult::Failure { error, .. }) => Response::with_effects(ex_effects::show_error(
            crate::errors::VimError::HostFailure(error.to_string().into()),
        )),
        (_, HostResult::Data { data, offset, .. }) => {
            let effects = executor_ex::completion_effects_for_data(pending, data, *offset);
            if effects.is_empty() {
                Response::ignored()
            } else {
                Response::with_effects(effects)
            }
        }
        (
            HostRequest::ReadClipboard { cursor_offset, .. },
            HostResult::ClipboardText { text, .. },
        ) => clipboard_completion(*cursor_offset, text),
        (
            HostRequest::FilterDocumentRange { range, .. },
            HostResult::FilteredRange {
                replacement,
                cursor_offset,
                ..
            },
        ) => filter_completion(*range, replacement, *cursor_offset),
        (
            HostRequest::ReindentRange {
                range,
                start_col,
                end_col,
                end_line_in_range,
                input_text,
                start_byte_offset,
                ..
            },
            HostResult::FilteredRange {
                replacement,
                cursor_offset,
                mark_dot_offset,
                ..
            },
        ) => reindent_completion(
            *range,
            replacement,
            *cursor_offset,
            *start_col,
            *end_col,
            *end_line_in_range,
            *mark_dot_offset,
            input_text.as_str() == replacement.as_str(),
            *start_byte_offset,
        ),
        (_, HostResult::Success { .. }) => Response::ignored(),
        _ => {
            let effects = ex_effects::show_error(crate::errors::VimError::InternalError(
                "host completion payload mismatch".into(),
            ));
            Response::with_effects(effects)
        }
    }
}

fn clipboard_completion(cursor_offset: usize, text: &compact_str::CompactString) -> Response {
    if text.is_empty() {
        return Response::ignored();
    }
    let end = cursor_offset.saturating_add(text.len());
    Response::with_effects(
        Effects::new()
            .insert(Offset::new(cursor_offset), text.clone())
            .set_cursor(Offset::new(end)),
    )
}

fn filter_completion(
    range: crate::primitives::Range,
    replacement: &compact_str::CompactString,
    cursor_offset: Option<usize>,
) -> Response {
    let effects = ex_effects::filter_completion(range, replacement.clone(), cursor_offset);
    if effects.is_empty() {
        Response::ignored()
    } else {
        Response::with_effects(effects)
    }
}

/// Reindent completion: replace text AND set marks `[`, `]`, `.`.
///
/// Neovim's `op_reindent` (indent.c:1052-1056) sets:
///   `b_op_start = oap->start`  →  mark `[`
///   `b_op_end   = oap->end`    →  mark `]`
///
/// The column values come from the ORIGINAL text (before reindent) but are
/// interpreted as (line, col) positions in the POST-reindent text (because
/// line count doesn't change during reindent). We convert (line, col) to
/// byte offset in the replacement text.
///
/// Mark `.` is set by `changed_lines()` in `op_reindent` to the start of the
/// first changed line (the cursor position after `beginline(BL_SOL|BL_FIX)`).
fn reindent_completion(
    range: crate::primitives::Range,
    replacement: &compact_str::CompactString,
    cursor_offset: Option<usize>,
    start_col: usize,
    end_col: usize,
    end_line_in_range: usize,
    mark_dot_from_host: Option<usize>,
    is_noop: bool,
    start_byte_offset: usize,
) -> Response {
    let mut effects = if is_noop {
        // Replacement is identical to original (e.g., == on a blank line or
        // already-correctly-indented text). Skip the Replace effect to
        // prevent sync_change_marks from incorrectly setting mark '.'.
        // Still emit cursor positioning and marks for '['/']'.
        let mut e = crate::effects::Effects::new();
        if let Some(offset) = cursor_offset {
            e = e.set_cursor(Offset::new(offset));
        }
        e
    } else {
        let e = ex_effects::filter_completion(range, replacement.clone(), cursor_offset);
        if e.is_empty() {
            return Response::ignored();
        }
        e
    };

    // Compute mark positions from (line, col) in the replacement text.
    // The replacement text maps 1:1 to the original range's lines.
    let range_start = range.start().get();

    // Mark `[`: first line of range, column = start_col.
    // Clamp to the first line's actual length in the replacement.
    //
    // When `extend_to_full_lines` includes a preceding newline (EOF case),
    // `range_start + start_col` may be too low because `range_start`
    // includes the preceding newline. In that case, `start_byte_offset`
    // (the true `oap->start` position, = min(cursor, motion_target))
    // points to the correct content line. Take the maximum to handle both
    // the normal case and the EOF-adjusted case correctly.
    let first_line_len = replacement.lines().next().map_or(0, str::len);
    let from_range = range_start + start_col.min(first_line_len);
    let mark_start_offset = from_range.max(start_byte_offset);

    // Mark `]`: walk `end_line_in_range` newlines in the replacement to
    // find the byte start of `oap->end`'s line, then add `end_col`.
    //
    // `end_line_in_range` is the number of newlines between `range.start()`
    // and `oap->end` in the original text.  Since reindent preserves line
    // count, we walk the same number of newlines in the replacement.
    //
    // For motion-based `=j`:  `oap->end` is within the range (on the last
    //   content line), so `end_line_in_range` < total newlines.
    // For `=it` with exclusive col-0 end:  `oap->end` is at/past range end,
    //   so `end_line_in_range` == total newlines in the range, and the
    //   target byte is `replacement.len()` (start of the next line).
    let mut end_line_byte_start: usize = 0;
    let mut newlines_walked: usize = 0;
    for (byte_idx, byte_val) in replacement.as_bytes().iter().enumerate() {
        if *byte_val == b'\n' {
            newlines_walked += 1;
            if newlines_walked == end_line_in_range {
                end_line_byte_start = byte_idx + 1;
                break;
            }
        }
    }
    // If we didn't find enough newlines (end_line_in_range > total newlines
    // in the replacement), fall back to replacement.len() (pointing past
    // the replacement — the start of the next document line).
    if newlines_walked < end_line_in_range {
        end_line_byte_start = replacement.len();
    }

    // Don't clamp end_col to line length. Neovim stores the raw column
    // in b_op_end, and getpos() returns it as-is. The oracle converts
    // (line, col) to byte offset by adding line_start + col, which may
    // exceed the line's actual length (pointing into the next line's
    // territory). This matches Neovim's behavior.
    let mark_end_offset = range_start + end_line_byte_start + end_col;

    // Mark `.`: Neovim's op_reindent calls changed_lines(first_changed, 0, ...)
    // which sets mark `.` to (first_changed, 0). Only set mark `.` when the
    // host reports an actual change (mark_dot_from_host is Some). When None,
    // no lines changed so Neovim's changed_lines() was never called and
    // mark `.` preserves its previous value.
    let mark_dot_offset = mark_dot_from_host;

    effects = effects
        .set_mark(
            crate::primitives::MarkName::CHANGE_START,
            Offset::new(mark_start_offset),
            None,
        )
        .set_mark(
            crate::primitives::MarkName::CHANGE_END,
            Offset::new(mark_end_offset),
            None,
        );
    if let Some(dot_offset) = mark_dot_offset {
        effects = effects.set_mark(
            crate::primitives::MarkName::LAST_CHANGE,
            Offset::new(dot_offset),
            None,
        );
    }

    Response::with_effects(effects)
}
