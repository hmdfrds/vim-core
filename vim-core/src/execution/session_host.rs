//! Session-owned host implementation for the `VimHost` framework.
//!
//! `SessionHost` owns all non-engine state (document, cursor, undo store,
//! viewport, pending events) and implements `Document + VimHost`. The public
//! API surface is `pub type HostSession = VimSession<SessionHost>` (defined
//! in `host_session.rs`), which provides backward-compatible inherent methods
//! alongside the generic `VimSession<H>` interface.

use std::borrow::Cow;

use compact_str::CompactString;

use crate::dispatch::ViewportInfo;
use crate::document::Document;
use crate::effects::Effect;
use crate::execution::host::{HostRequest, HostRequestId, HostResult, RequestDisposition};
use crate::execution::host_response::{
    CommandLineInfo, CursorShape, EditOp, HostResponse, MarkChangeInfo, ScrollInfo,
    ScrollPlacement, SearchMatchInfo, SelectionInfo, SubstitutePreviewState,
};
use crate::keymap::{KeyEvent, MappingFlags, MappingKind, MappingMode};
use crate::primitives::byte_delta;
use crate::primitives::{
    CommandLinePrompt, Mode, MotionType, Offset, Position, RegisterContent, RegisterName,
    SelectionRange, SelectionShape, TextOp, VimOptions, VisualType,
};
use crate::state::ScrollHint;

use super::dirty_tracker::{DirtyFlags, DirtyTracker};
use super::engine::vim_text_document::VimTextDocument;
use super::host_api::{HostCapabilitySet, VimHost, VimSession};
use super::host_session::UnknownRequestError;
use super::undo_stack::UndoStore;

#[cfg(feature = "engine-tracing")]
use crate::execution::trace::TraceEvent;

// ─────────────────────────────────────────────────────────────────────────────
// SessionDocument type alias — mirrors host_session.rs
// ─────────────────────────────────────────────────────────────────────────────

type SessionDocument = VimTextDocument;

// ─────────────────────────────────────────────────────────────────────────────
// Free functions: changeset_to_edit_ops
// ─────────────────────────────────────────────────────────────────────────────

/// Get the byte range `[start, end)` of line content (excluding `\n`).
fn line_range_of(text: &str, line: usize) -> (usize, usize) {
    let mut current = 0usize;
    let mut start = 0usize;
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if current == line && b == b'\n' {
            return (start, i);
        }
        if b == b'\n' {
            current += 1;
            start = i + 1;
        }
    }
    if current == line {
        (start, text.len())
    } else {
        (text.len(), text.len())
    }
}

/// Clamp undo/redo cursor to avoid landing on `\n` in Normal mode.
///
/// Mirrors Neovim's `check_cursor_col()` called after `u_undoredo()`.
/// When the undo cursor lands on a newline at end of a non-empty line,
/// backs up to the last character before the newline.
fn clamp_undo_cursor(text: &str, raw: usize) -> usize {
    if text.is_empty() {
        return 0;
    }
    let max = crate::primitives::text_util::prev_char_boundary(text, text.len());
    let clamped = raw.min(max);
    if clamped < text.len() && text.as_bytes()[clamped] == b'\n' && clamped > 0 {
        let before = clamped - 1;
        if text.as_bytes()[before] != b'\n' {
            return before;
        }
    }
    clamped
}

/// Convert a sequence of [`TextOp`]s (from a [`ChangeSet`]) into [`EditOp`]s.
pub(crate) fn changeset_to_edit_ops(ops: &[TextOp]) -> Vec<EditOp> {
    let mut result = Vec::new();
    let mut offset: usize = 0;

    for op in ops {
        match op {
            TextOp::Retain(n) => {
                offset += n;
            }
            TextOp::Insert(text) => {
                result.push(EditOp {
                    offset,
                    delete: 0,
                    insert: text.clone(),
                });
                offset += text.len();
            }
            TextOp::Delete(n) => {
                result.push(EditOp {
                    offset,
                    delete: *n,
                    insert: CompactString::default(),
                });
            }
        }
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// SessionHost
// ─────────────────────────────────────────────────────────────────────────────

/// Standalone host that owns a document and all supporting state.
///
/// Contains all 15 non-engine fields from `HostSession`. Implements
/// `Document` (delegating to the inner `SessionDocument`) and `VimHost`
/// (with the same `apply_effects` logic as `HostSession`).
///
/// Used as: `VimSession<SessionHost>`.
pub struct SessionHost {
    pub(crate) document: SessionDocument,
    pub(crate) cursor_offset: usize,
    pub(crate) selection: Option<SelectionRange>,
    pub(crate) viewport: ViewportInfo,
    pub(crate) undo_store: UndoStore,
    pub(crate) dirty_tracker: DirtyTracker,
    pub(crate) frame_id: u64,
    pub(crate) pending_mark_changes: Vec<MarkChangeInfo>,
    pub(crate) pending_substitute_preview: Option<SubstitutePreviewState>,
    pub(crate) pending_edits: Vec<EditOp>,
    pub(crate) last_search_match_info: Option<SearchMatchInfo>,
    pub(crate) search_highlights_suppressed: bool,
    pub(crate) auto_handle_defaults: bool,
    pub(crate) clipboard_dirty: bool,
    pub(crate) last_error: Option<String>,
    /// Undo-line state: saved line number (0-indexed) for `U` command.
    u_line_lnum: Option<usize>,
    /// Undo-line state: saved original line content for `U` command.
    u_line_content: Option<String>,
    #[cfg(feature = "testing")]
    pub(crate) captured_effects: Vec<Effect>,
}

impl SessionHost {
    /// Create a new `SessionHost` with the given document text.
    ///
    /// Uses the same defaults as `HostSession::new` minus the engine.
    #[must_use]
    pub fn with_text(text: &str) -> Self {
        Self {
            document: SessionDocument::new(text),
            cursor_offset: 0,
            selection: None,
            viewport: ViewportInfo {
                first_line: 0,
                height: 24,
                width: 80,
            },
            undo_store: UndoStore::new(),
            dirty_tracker: DirtyTracker::new(),
            frame_id: 0,
            pending_mark_changes: Vec::new(),
            pending_substitute_preview: None,
            pending_edits: Vec::new(),
            last_search_match_info: None,
            search_highlights_suppressed: false,
            auto_handle_defaults: false,
            clipboard_dirty: false,
            last_error: None,
            u_line_lnum: None,
            u_line_content: None,
            #[cfg(feature = "testing")]
            captured_effects: Vec::new(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Document impl — delegate to inner SessionDocument
// ─────────────────────────────────────────────────────────────────────────────

impl Document for SessionHost {
    fn text(&self) -> &str {
        self.document.text()
    }

    fn line_count(&self) -> usize {
        self.document.line_count()
    }

    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        self.document.offset_to_pos(offset)
    }

    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        self.document.pos_to_offset(pos)
    }

    fn line_of_offset(&self, offset: usize) -> usize {
        self.document.line_of_offset(offset)
    }

    fn slice(&self, start: usize, end: usize) -> Cow<'_, str> {
        self.document.slice(start, end)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// VimHost impl
// ─────────────────────────────────────────────────────────────────────────────

impl VimHost for SessionHost {
    fn capabilities(&self) -> HostCapabilitySet {
        HostCapabilitySet::FULL
    }

    fn cursor_offset(&self) -> usize {
        self.cursor_offset
    }

    fn viewport(&self) -> ViewportInfo {
        self.viewport
    }

    fn selection(&self) -> Option<SelectionRange> {
        self.selection
    }

    fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
        RequestDisposition::Deferred
    }

    fn apply_effects(&mut self, effects: &[Effect]) {
        #[cfg(feature = "testing")]
        self.captured_effects.extend_from_slice(effects);

        for effect in effects {
            // Track clipboard writes for dirty-flag optimization.
            if let Effect::SetRegister { name, .. } = effect {
                if *name == RegisterName::CLIPBOARD {
                    self.clipboard_dirty = true;
                }
            }
            match effect {
                // ── Text mutations ──────────────────────────────────────
                Effect::Insert { offset, text } => {
                    let off = offset.get();
                    self.pending_edits.push(EditOp {
                        offset: off,
                        delete: 0,
                        insert: text.clone(),
                    });
                    self.document.apply_insert(off, text);
                    if text.contains('\n') {
                        self.dirty_tracker.record_full_redraw();
                    }
                    let start_line = self.document.line_of_offset(off);
                    let end_line = self
                        .document
                        .line_of_offset((off + text.len()).min(self.document.len()));
                    self.dirty_tracker.record_line_range(start_line..=end_line);
                }
                Effect::Delete { range } => {
                    let start = range.start().get();
                    let end = range.end().get();
                    self.pending_edits.push(EditOp {
                        offset: start,
                        delete: end - start,
                        insert: CompactString::default(),
                    });
                    let start_line = self.document.line_of_offset(start);
                    let deleted_text = self.document.slice(start, end);
                    let has_newlines = deleted_text.contains('\n');
                    self.document.apply_delete(start, end);
                    if has_newlines {
                        self.dirty_tracker.record_full_redraw();
                        // Clear U (undo-line) state on linewise deletes.
                        // Matches Neovim's u_clearline() called from ops.c
                        // after dd, cc on 2+ lines, etc.
                        self.u_line_lnum = None;
                        self.u_line_content = None;
                    }
                    self.dirty_tracker.record_lines(&[start_line]);
                }
                Effect::Replace { range, text } => {
                    let start = range.start().get();
                    let end = range.end().get();
                    self.pending_edits.push(EditOp {
                        offset: start,
                        delete: end - start,
                        insert: text.clone(),
                    });
                    let deleted_text = self.document.slice(start, end);
                    let has_newlines =
                        deleted_text.contains('\n') || text.contains('\n');
                    self.document.apply_replace(start, end, text);
                    if has_newlines {
                        self.dirty_tracker.record_full_redraw();
                    }
                    let start_line = self.document.line_of_offset(start);
                    let end_line = self
                        .document
                        .line_of_offset((start + text.len()).min(self.document.len()));
                    self.dirty_tracker.record_line_range(start_line..=end_line);
                }

                // ── Cursor + selection ──────────────────────────────────
                Effect::SetCursor { offset } => {
                    self.cursor_offset = offset.get().min(self.document.len());
                }
                Effect::SetSelection { anchor, head, .. } => {
                    let doc_len = self.document.len();
                    let max_offset = Offset::new(doc_len);
                    let clamped_anchor = (*anchor).min(max_offset);
                    let clamped_head = (*head).min(max_offset);
                    self.selection = Some(SelectionRange::new(clamped_anchor, clamped_head));
                }
                Effect::ClearSelection => {
                    self.selection = None;
                }

                // ── Undo ────────────────────────────────────────────────
                Effect::BeginUndoGroup { .. } => {
                    // Track undo-line state: save current line content on first
                    // edit so `U` can restore it (mirrors Neovim's u_save_cursor).
                    let text = self.document.text();
                    let cursor_line =
                        text.as_bytes()[..self.cursor_offset.min(text.len())]
                            .iter()
                            .filter(|&&b| b == b'\n')
                            .count();
                    if self.u_line_lnum != Some(cursor_line) {
                        self.u_line_lnum = Some(cursor_line);
                        let (start, end) = line_range_of(text, cursor_line);
                        self.u_line_content = Some(text[start..end].to_string());
                    }
                    self.undo_store
                        .begin_group(self.document.text(), Offset::new(self.cursor_offset));
                }
                Effect::EndUndoGroup { node_id } => {
                    self.undo_store
                        .end_group(*node_id, self.document.text(), Offset::new(self.cursor_offset));
                }
                Effect::Undo { steps, .. } => {
                    for step in steps {
                        if let Some(result) =
                            self.undo_store.undo_step(step.node_id, self.document.text())
                        {
                            let edit_ops = changeset_to_edit_ops(&result.ops);
                            self.pending_edits.extend(edit_ops);
                            self.document.set_text(&result.text);
                            self.cursor_offset =
                                result.cursor.get().min(self.document.len());
                        }
                    }
                    if let Some(last) = steps.last() {
                        self.cursor_offset = clamp_undo_cursor(
                            self.document.text(),
                            last.cursor().get(),
                        );
                        self.dirty_tracker.record_full_redraw();
                    }
                }
                Effect::Redo { steps, .. } => {
                    for step in steps {
                        if let Some(result) =
                            self.undo_store.redo_step(step.node_id, self.document.text())
                        {
                            let edit_ops = changeset_to_edit_ops(&result.ops);
                            self.pending_edits.extend(edit_ops);
                            self.document.set_text(&result.text);
                            self.cursor_offset =
                                result.cursor.get().min(self.document.len());
                        }
                    }
                    if let Some(last) = steps.last() {
                        self.cursor_offset = clamp_undo_cursor(
                            self.document.text(),
                            last.cursor().get(),
                        );
                        self.dirty_tracker.record_full_redraw();
                    }
                }

                Effect::UndoLine { .. } => {
                    if let (Some(target_line), Some(saved_content)) =
                        (self.u_line_lnum, self.u_line_content.take())
                    {
                        let text = self.document.text();
                        let (line_start, line_end) =
                            line_range_of(text, target_line);
                        let current_content =
                            text[line_start..line_end].to_string();

                        // Create undo entry so U is undoable via `u`
                        let cursor = Offset::new(self.cursor_offset);
                        let old_text = text.to_owned();
                        self.undo_store.begin_group(&old_text, cursor);

                        let mut new_text = String::with_capacity(
                            text.len() - current_content.len()
                                + saved_content.len(),
                        );
                        new_text.push_str(&text[..line_start]);
                        new_text.push_str(&saved_content);
                        new_text.push_str(&text[line_end..]);
                        self.document.set_text(&new_text);

                        let node_id = None; // engine will assign
                        self.undo_store.end_group(
                            node_id,
                            &new_text,
                            Offset::new(line_start),
                        );

                        self.cursor_offset =
                            line_start.min(new_text.len().saturating_sub(1));
                        self.u_line_content = Some(current_content);
                        self.dirty_tracker.record_full_redraw();
                    }
                }

                // ── Search & highlights ─────────────────────────────────
                Effect::SetSearchPattern { .. } => {
                    self.search_highlights_suppressed = false;
                }
                Effect::HighlightMatches { .. } => {
                    self.search_highlights_suppressed = false;
                    self.dirty_tracker.set_flags(DirtyFlags::SEARCH_HIGHLIGHTS);
                }
                Effect::ClearHighlights => {
                    self.search_highlights_suppressed = true;
                    self.last_search_match_info = None;
                    self.dirty_tracker.set_flags(DirtyFlags::SEARCH_HIGHLIGHTS);
                }
                Effect::SearchMatchInfo { current, total, complete } => {
                    self.last_search_match_info = Some(SearchMatchInfo {
                        current: *current as usize,
                        total: *total as usize,
                        complete: *complete,
                    });
                    self.dirty_tracker.set_flags(DirtyFlags::SEARCH_HIGHLIGHTS);
                }

                // ── Substitute preview ──────────────────────────────────
                Effect::SubstitutePreview { matches } => {
                    self.pending_substitute_preview =
                        Some(SubstitutePreviewState::Matches(matches.clone()));
                }
                Effect::ClearSubstitutePreview => {
                    self.pending_substitute_preview = Some(SubstitutePreviewState::Cleared);
                }

                // ── Marks ───────────────────────────────────────────────
                Effect::SetMark { name, offset, .. } if name.is_settable() => {
                    let off = offset.get();
                    let (line, col) = self
                        .document
                        .offset_to_pos(Offset::new(off))
                        .map_or((0u32, 0u32), |pos| {
                            (
                                byte_delta::to_u32(pos.line().get()),
                                byte_delta::to_u32(pos.col().get()),
                            )
                        });
                    self.pending_mark_changes.push(MarkChangeInfo::Set {
                        name: name.char() as u8,
                        line,
                        col,
                        visible: (name.char() as u8).is_ascii_alphabetic(),
                    });
                    self.dirty_tracker.set_flags(DirtyFlags::MARKS);
                }
                Effect::SetMark { .. } => {
                    // Special marks — tracked internally, not forwarded to host.
                }
                Effect::ClearMark { mark } if mark.is_settable() => {
                    self.pending_mark_changes.push(MarkChangeInfo::Cleared {
                        name: mark.char() as u8,
                    });
                    self.dirty_tracker.set_flags(DirtyFlags::MARKS);
                }
                Effect::ClearMark { .. } => {
                    // Special mark cleared — not forwarded.
                }

                // ── Macro recording (engine-internal) ───────────────────
                Effect::StartRecording { .. } | Effect::StopRecording => {}

                // ── Error tracking ─────────────────────────────────────
                Effect::ShowError { error, .. } => {
                    self.last_error = Some(error.to_string());
                }

                // ── Scroll / viewport dirty flags ─────────────────────
                Effect::ScrollTo { .. }
                | Effect::CenterCursor
                | Effect::CursorToTop
                | Effect::CursorToBottom
                | Effect::ScrollLeft { .. }
                | Effect::ScrollRight { .. }
                | Effect::ScrollHalfScreenLeft { .. }
                | Effect::ScrollHalfScreenRight { .. }
                | Effect::ScrollCursorToLeftEdge
                | Effect::ScrollCursorToRightEdge
                | Effect::SetScrollHalfCount { .. } => {
                    self.dirty_tracker.set_flags(DirtyFlags::VIEWPORT);
                }

                // ── Fold dirty flags ───────────────────────────────────
                Effect::FoldLine { .. }
                | Effect::UnfoldLine { .. }
                | Effect::ToggleFold { .. }
                | Effect::ToggleFoldRecursive { .. }
                | Effect::FoldAll
                | Effect::UnfoldAll
                | Effect::FoldLineRecursive { .. }
                | Effect::UnfoldLineRecursive { .. }
                | Effect::DeleteFold { .. }
                | Effect::DeleteFoldRecursive { .. }
                | Effect::EliminateAllFolds
                | Effect::ToggleFoldEnable
                | Effect::SetFoldEnable { .. } => {
                    self.dirty_tracker.set_flags(DirtyFlags::FOLDS);
                }

                // ── Remaining effects ───────────────────────────────────
                Effect::SetMode { .. }
                | Effect::BeginInsert { .. }
                | Effect::CommandLineEdit(_)
                | Effect::SetBlockInsert { .. }
                | Effect::SetRegister { .. }
                | Effect::ClearNamedRegister { .. }
                | Effect::OperatorToMark { .. }
                | Effect::Bell
                | Effect::ShowInfo { .. }
                | Effect::ShowWarning { .. }
                | Effect::ClearMessage
                | Effect::CopyToClipboard { .. }
                | Effect::SubstituteConfirmShow { .. }
                | Effect::SubstituteConfirmEnd
                | Effect::SetHighlightRange { .. }
                | Effect::ClearHighlightRange { .. }
                | Effect::SetVirtualText { .. }
                | Effect::ClearVirtualText { .. }
                | Effect::SetDiagnostics { .. }
                | Effect::SetCursorStyle { .. }
                | Effect::CursorShapeHint { .. }
                | Effect::SetBlockSelections { .. }
                | Effect::SaveSelections { .. }
                | Effect::RestoreSelections { .. }
                | Effect::SelectNextMatch { .. }
                | Effect::SelectPreviousMatch { .. }
                | Effect::HighlightRows { .. }
                | Effect::SyncFoldRanges { .. }
                | Effect::UndoTreeSnapshot { .. }
                | Effect::Event { .. }
                // Engine-internal (no HostEvent):
                | Effect::SaveLastVisual { .. }
                | Effect::PushJumpList { .. }
                | Effect::JumpOlder { .. }
                | Effect::JumpNewer { .. }
                | Effect::JumpToBuffer { .. }
                | Effect::ChangelistOlder { .. }
                | Effect::ChangelistNewer { .. }
                | Effect::SetLastSubstitute { .. }
                | Effect::SetLastSubstituteFlags { .. }
                | Effect::SetSubstitutePattern { .. }
                | Effect::SetLastFind { .. }
                | Effect::NormCommand { .. }
                | Effect::OperatorFilter { .. }
                | Effect::OperatorReindent { .. }
                | Effect::PlayMacro { .. }
                | Effect::SetStickyColumn { .. }
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
                | Effect::GotoDefinition
                | Effect::ShowDocumentation
                | Effect::OpenCommandWindow { .. }
                | Effect::CallOperatorFunc { .. }
                | Effect::HostAction { .. }
                | Effect::SetExtState { .. }
                | Effect::ClearExtState { .. }
                | Effect::SetSubstituteConfirmState { .. }
                | Effect::ClearSubstituteConfirmState
                | Effect::SyntaxSelectionPush { .. }
                | Effect::SyntaxSelectionPop
                | Effect::SyntaxHistoryClear
                | Effect::SetSyntaxSelections { .. }
                | Effect::ShowMatch { .. }
                | Effect::SetVariable { .. }
                | Effect::DeleteVariable { .. }
                | Effect::CrossBufferEdit { .. }
                | Effect::Noop => {}
            // Mode transition: no side effects here (mode sync handled by
            // effect processor), but does generate a host event.
            Effect::ModeTransition { .. } => {}
            // Timer request: no side effects in session, host event handles it.
            Effect::RequestTimer { .. } => {}
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Free function: normalize_edit_offsets
// ─────────────────────────────────────────────────────────────────────────────

/// Normalize cumulative edit offsets to original-relative.
///
/// Mirrors `host_session::normalize_edit_offsets`.
fn normalize_edit_offsets(edits: &mut [EditOp]) {
    if edits.len() <= 1 {
        return;
    }
    let mut cumulative_delta: isize = 0;
    for edit in edits.iter_mut() {
        if cumulative_delta != 0 {
            let adjusted = edit
                .offset
                .cast_signed()
                .saturating_sub(cumulative_delta)
                .max(0)
                .cast_unsigned();
            edit.offset = adjusted;
        }
        let delta = edit.insert.len().cast_signed() - edit.delete.cast_signed();
        cumulative_delta = cumulative_delta.saturating_add(delta);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// impl VimSession<SessionHost> — inherent convenience methods
// ═════════════════════════════════════════════════════════════════════════════
//
// These methods are available on `HostSession` (= `VimSession<SessionHost>`)
// and provide session-owned-document ergonomics on top of the generic
// `VimSession<H: VimHost>` interface. They handle HostResponse construction,
// undo-store management, and other SessionHost-specific concerns.
//
// Sections:
//   1. Lifecycle (new, with_auto_handle_defaults, with_viewport)
//   2. Processing API (process_key_host, process_click_host, …)
//   3. Callback-based API
//   4. Document access (text, line, line_count, set_text, replace_text, …)
//   5. State queries (mode, cursor_offset, frame_id, …)
//   6. Register access
//   7. Configuration delegates
//   8. Buffer lifecycle
//   9. Mark access
//  10. Search state
//  11. (reserved)
//  12. Multi-cursor API
//  13. Test helpers (#[cfg(test)])

impl VimSession<SessionHost> {
    // ─────────────────────────────────────────────────────────────────────
    // 1. Lifecycle
    // ─────────────────────────────────────────────────────────────────────

    /// Create a new session with the given document text.
    ///
    /// Uses a default viewport of 24 lines x 80 columns starting at line 0.
    /// This is the backward-compatible constructor matching the old
    /// `HostSession::new(text)` API.
    #[must_use]
    pub fn new(text: &str) -> Self {
        VimSession::with_host(SessionHost::with_text(text))
    }

    /// Builder: enable auto-handling of host requests with known-correct defaults.
    #[must_use]
    pub const fn with_auto_handle_defaults(mut self, enabled: bool) -> Self {
        self.host_mut().auto_handle_defaults = enabled;
        self
    }

    /// Builder: set the viewport info.
    #[must_use]
    pub const fn with_viewport(mut self, viewport: ViewportInfo) -> Self {
        self.host_mut().viewport = viewport;
        self
    }

    // ─────────────────────────────────────────────────────────────────────
    // 2. Processing API
    // ─────────────────────────────────────────────────────────────────────

    /// Snapshot-aware wrapper around the generic `drain_pending`.
    /// Process a single key event through the full engine pipeline.
    ///
    /// Returns `HostResponse` (not `ProcessResult`) — shadows the generic
    /// `VimSession::process_key` for SessionHost consumers.
    pub fn process_key_host(&mut self, key: KeyEvent) -> HostResponse {
        self.host_mut().frame_id += 1;
        self.host_mut().clipboard_dirty = false;
        self.host_mut().last_error = None;
        #[cfg(feature = "testing")]
        self.host_mut().captured_effects.clear();

        // Capture pre-drift shadow text for UndoStore snapshot.
        // If the drift gate fires inside engine.process(), the undo tree gets a
        // node but the UndoStore has no text snapshot. We capture the shadow text
        // (= pre-edit text) here, then check after process() whether a drift
        // edit was created and record the snapshot.
        let pre_drift_shadow = self.engine().shadow_text().map(str::to_owned);

        // Split borrow: engine + host + safety are disjoint fields.
        let response = {
            let (engine, host, safety) = self.engine_host_safety();
            let ctx = Self::build_context(host, safety);
            engine.process(key, ctx)
        };
        let consumed = response.consumed();

        // Take both undo nodes produced by the drift gate in one pass.
        let fc_node = self.engine_mut().take_last_force_committed_node();
        let ext_node = self.engine_mut().take_last_external_edit_node();

        // If the drift gate force-committed an INSERT pending group, the
        // UndoStore's existing pending snapshot (from BeginUndoGroup) belongs
        // to that INSERT group. Commit it under the force-committed NodeId.
        if let Some(fc_node) = fc_node {
            let host = self.host_mut();
            let cursor = Offset::new(host.cursor_offset);
            let post_text = host.document.text().to_owned();
            host.undo_store.end_group(Some(fc_node), &post_text, cursor);
        }

        // If the drift gate created its own undo node, create an UndoStore
        // entry for it (begin + end in one shot using the pre-drift shadow).
        if let Some(ext_node) = ext_node {
            if let Some(ref pre_text) = pre_drift_shadow {
                let host = self.host_mut();
                let cursor = Offset::new(host.cursor_offset);
                host.undo_store.begin_group(pre_text, cursor);
                let post_text = host.document.text().to_owned();
                host.undo_store
                    .end_group(Some(ext_node), &post_text, cursor);
            }
        }

        // If a force-commit happened and INSERT is still active, the engine
        // re-opened a continuation pending group in the UndoTree. Open a
        // matching pending in the UndoStore so EndUndoGroup on Esc can commit.
        if fc_node.is_some() && self.engine().mode().is_insert() {
            let host = self.host_mut();
            let cursor = Offset::new(host.cursor_offset);
            let text = host.document.text().to_owned();
            host.undo_store.begin_group(&text, cursor);
        }

        self.deliver_effects(response.effects());

        // Sync the shadow document after undo/redo so the drift gate on the
        // next process() call does not see stale shadow text and create a
        // phantom external-edit node.
        self.sync_shadow_after_undo_redo(response.effects());

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);
        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        let (engine, host) = self.engine_and_host_mut();
        Self::build_host_response_impl(engine, host, consumed, host_requests)
    }

    /// Process a mouse click at the given byte offset.
    pub fn process_click_host(&mut self, offset: usize) -> HostResponse {
        self.host_mut().frame_id += 1;
        self.host_mut().clipboard_dirty = false;

        let response = {
            let (engine, host, safety) = self.engine_host_safety();
            let ctx = Self::build_context(host, safety);
            engine.process_click(offset, &ctx)
        };
        let consumed = response.consumed();

        self.deliver_effects(response.effects());

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);
        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        let (engine, host) = self.engine_and_host_mut();
        Self::build_host_response_impl(engine, host, consumed, host_requests)
    }

    /// Process a mouse selection at the given anchor and head byte offsets.
    pub fn process_mouse_selection_host(
        &mut self,
        anchor_offset: usize,
        head_offset: usize,
        shape: SelectionShape,
    ) -> HostResponse {
        self.host_mut().frame_id += 1;
        self.host_mut().clipboard_dirty = false;

        let response = {
            let (engine, host, safety) = self.engine_host_safety();
            let ctx = Self::build_context(host, safety);
            engine.process_mouse_selection(anchor_offset, head_offset, shape, &ctx)
        };
        let consumed = response.consumed();

        self.deliver_effects(response.effects());

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);
        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        let (engine, host) = self.engine_and_host_mut();
        Self::build_host_response_impl(engine, host, consumed, host_requests)
    }

    /// Resolve a mapping timeout (no more keys arrived within `timeoutlen`).
    pub fn resolve_timeout(&mut self) -> HostResponse {
        self.host_mut().frame_id += 1;
        self.host_mut().clipboard_dirty = false;

        self.engine_mut().resolve_timeout();
        let host_requests = self.drain_pending();

        let (engine, host) = self.engine_and_host_mut();
        Self::build_host_response_impl(engine, host, true, host_requests)
    }

    /// Abort all pending macro replay and typeahead.
    pub fn abort_replay(&mut self) -> HostResponse {
        let host = self.host_mut();
        host.frame_id += 1;
        host.clipboard_dirty = false;

        let (engine, host) = self.engine_and_host_mut();
        engine.abort_replay();
        Self::build_host_response_impl(engine, host, true, Vec::new())
    }

    /// Complete a pending host request with the given result.
    pub fn complete_request_host(&mut self, result: &HostResult) -> HostResponse {
        self.host_mut().frame_id += 1;
        self.host_mut().clipboard_dirty = false;

        let response = self.engine_mut().complete_host_request(result);
        let consumed = response.consumed();

        self.deliver_effects(response.effects());

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);
        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        let (engine, host) = self.engine_and_host_mut();
        Self::build_host_response_impl(engine, host, consumed, host_requests)
    }

    /// Complete a pending host request with the given result (checked variant).
    ///
    /// # Errors
    ///
    /// Returns [`UnknownRequestError`] carrying `result.id()` when the engine
    /// has no host request outstanding under that id — the id was never issued
    /// by this engine, or the request was already completed by an earlier call.
    /// Nothing is delivered to the engine in that case.
    pub fn complete_request_checked(
        &mut self,
        result: &HostResult,
    ) -> Result<HostResponse, UnknownRequestError> {
        let id = result.id();
        if !self.engine().has_pending_host_request(id) {
            return Err(UnknownRequestError { id });
        }
        Ok(self.complete_request_host(result))
    }

    /// Source a config string (sequence of ex commands).
    pub fn source_config(&mut self, text: &str) -> HostResponse {
        self.host_mut().frame_id += 1;
        self.host_mut().clipboard_dirty = false;

        let (consumed, requests) = {
            let mut response = self.engine_mut().source_config_text(text);
            let consumed = response.consumed();
            let requests = response.take_host_requests();
            self.deliver_effects(response.effects());
            (consumed, requests)
        };

        let mut host_requests = Vec::new();
        self.handle_sync_requests(&requests, &mut host_requests, 0);

        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        let (engine, host) = self.engine_and_host_mut();
        Self::build_host_response_impl(engine, host, consumed, host_requests)
    }

    /// Record text typed during insert mode for dot-repeat and macro recording.
    pub fn record_insert_text(&mut self, text: &str) {
        self.engine_mut().record_insert_text(text);
    }

    /// Inject keys into the typeahead buffer for processing on the next drain.
    pub fn feed_keys(&mut self, keys: &str, remap: bool) {
        self.engine_mut().feed_keys(keys, remap);
    }

    /// Paste text at the current cursor position.
    pub fn paste(&mut self, text: &str) -> Vec<HostResponse> {
        use crate::keymap::{Key, Modifiers};

        let mut responses = Vec::new();
        let needs_insert = !self.engine().mode().is_insert();
        if needs_insert {
            responses.push(self.process_key_host(KeyEvent::new(Key::Char('i'), Modifiers::NONE)));
        }
        for ch in text.chars() {
            responses.push(self.process_key_host(KeyEvent::new(Key::Char(ch), Modifiers::NONE)));
        }
        if needs_insert {
            responses.push(self.process_key_host(KeyEvent::new(Key::Escape, Modifiers::NONE)));
        }
        if responses.is_empty() {
            responses.push(self.process_key_host(KeyEvent::new(Key::Escape, Modifiers::NONE)));
        }
        responses
    }

    /// Accept a specific command-line completion candidate by index.
    pub fn accept_cmdline_completion(&mut self, index: usize) -> HostResponse {
        self.host_mut().frame_id += 1;
        self.host_mut().clipboard_dirty = false;

        let (consumed, requests) = {
            let mut response = self.engine_mut().accept_cmdline_completion(index);
            let consumed = response.consumed();
            let requests = response.take_host_requests();
            self.deliver_effects(response.effects());
            (consumed, requests)
        };

        let mut host_requests = Vec::new();
        self.handle_sync_requests(&requests, &mut host_requests, 0);

        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        let (engine, host) = self.engine_and_host_mut();
        Self::build_host_response_impl(engine, host, consumed, host_requests)
    }

    // ─────────────────────────────────────────────────────────────────────
    // 3. Callback-based API
    // ─────────────────────────────────────────────────────────────────────

    /// Process a key and handle all host requests via callbacks.
    pub fn process_key_with_callbacks(
        &mut self,
        key: KeyEvent,
        callbacks: &mut dyn super::host_callbacks::HostCallbacks,
    ) -> HostResponse {
        let response = self.process_key_host(key);
        let requests: Vec<HostRequest> = response.host_requests.clone();
        if requests.is_empty() {
            return response;
        }
        self.dispatch_callbacks(&requests, callbacks, 0);
        // Rebuild response with empty host_requests.
        let (engine, host) = self.engine_and_host_mut();
        Self::build_host_response_impl(engine, host, true, Vec::new())
    }

    /// Dispatch host requests to callbacks with depth limiting.
    fn dispatch_callbacks(
        &mut self,
        requests: &[HostRequest],
        callbacks: &mut dyn super::host_callbacks::HostCallbacks,
        depth: usize,
    ) {
        const MAX_DEPTH: usize = 5;
        if depth >= MAX_DEPTH || requests.is_empty() {
            for req in requests {
                let result = HostResult::Failure {
                    id: req.id(),
                    error: CompactString::from("max host request depth exceeded"),
                };
                let _ = self.engine_mut().complete_host_request(&result);
            }
            return;
        }

        for req in requests {
            let result = self.dispatch_single_callback(req, callbacks);
            let (engine, host) = self.engine_and_host_mut();
            let mut sub_response = engine.complete_host_request(&result);
            host.apply_effects(sub_response.effects());
            let sub_requests = sub_response.take_host_requests();
            if !sub_requests.is_empty() {
                self.dispatch_callbacks(&sub_requests, callbacks, depth + 1);
            }
        }
    }

    /// Map a single host request to a HostResult via the appropriate callback.
    fn dispatch_single_callback(
        &self,
        request: &HostRequest,
        callbacks: &mut dyn super::host_callbacks::HostCallbacks,
    ) -> HostResult {
        use compact_str::CompactString as CS;
        let id = request.id();

        match request {
            HostRequest::WriteFile { path, .. } => {
                let target = path.as_deref().unwrap_or("");
                if target.is_empty() {
                    return HostResult::Failure {
                        id,
                        error: CS::from("E32: No file name"),
                    };
                }
                match callbacks.write_file(target, self.host().document.text()) {
                    Ok(()) => {
                        let lines = self.host().document.line_count();
                        let bytes = self.host().document.len();
                        HostResult::Success {
                            id,
                            message: Some(CS::from(format!(
                                "\"{target}\" {lines}L, {bytes}B written"
                            ))),
                        }
                    }
                    Err(e) => HostResult::Failure {
                        id,
                        error: CS::from(format!("E212: Can't save: {e}")),
                    },
                }
            }

            HostRequest::Quit { force, .. } => {
                callbacks.quit(*force);
                HostResult::Success { id, message: None }
            }

            HostRequest::WriteQuit { force, .. } => {
                callbacks.quit(*force);
                HostResult::Success { id, message: None }
            }

            HostRequest::ReadFile {
                path, after_line, ..
            } => match callbacks.read_file(path.as_str()) {
                Ok(content) => HostResult::Data {
                    id,
                    data: CS::from(content),
                    offset: after_line.map(|l| l as usize),
                },
                Err(e) => HostResult::Failure {
                    id,
                    error: CS::from(format!("E484: Can't open file {path}: {e}")),
                },
            },

            HostRequest::ReadConfigFile { path, .. } => match callbacks.read_file(path.as_str()) {
                Ok(content) => HostResult::Data {
                    id,
                    data: CS::from(content),
                    offset: None,
                },
                Err(e) => HostResult::Failure {
                    id,
                    error: CS::from(format!("E484: Can't open file {path}: {e}")),
                },
            },

            HostRequest::ReadClipboard { .. } => {
                let text = callbacks.read_clipboard();
                HostResult::ClipboardText {
                    id,
                    text: CS::from(text),
                }
            }

            HostRequest::EditFile { path, force, .. } => {
                match callbacks.edit_file(path.as_str(), *force) {
                    Ok(()) => HostResult::Success { id, message: None },
                    Err(e) => HostResult::Failure {
                        id,
                        error: CS::from(e),
                    },
                }
            }

            HostRequest::ExternalCommand { command, .. } => {
                match callbacks.run_shell_command(command.as_str()) {
                    Ok(output) => HostResult::Data {
                        id,
                        data: CS::from(output),
                        offset: None,
                    },
                    Err(e) => HostResult::Failure {
                        id,
                        error: CS::from(e),
                    },
                }
            }

            HostRequest::FilterDocumentRange {
                input_text,
                command,
                ..
            } => match callbacks.filter_text(command.as_str(), input_text.as_str()) {
                Ok(replacement) => HostResult::FilteredRange {
                    id,
                    replacement: CS::from(replacement),
                    cursor_offset: None,
                    stderr: None,
                    mark_dot_offset: None,
                },
                Err(e) => HostResult::Failure {
                    id,
                    error: CS::from(e),
                },
            },

            HostRequest::ReindentRange { input_text, .. } => {
                match callbacks.reindent(input_text.as_str()) {
                    Ok(replacement) => HostResult::FilteredRange {
                        id,
                        replacement: CS::from(replacement),
                        cursor_offset: None,
                        stderr: None,
                        mark_dot_offset: None,
                    },
                    Err(e) => HostResult::Failure {
                        id,
                        error: CS::from(e),
                    },
                }
            }

            HostRequest::EvaluateExpression { expression, .. }
            | HostRequest::EvaluateMapping { expression, .. } => {
                if let Some(result) = self.try_evaluate_internal(expression.as_str()) {
                    HostResult::Data {
                        id,
                        data: CS::from(result),
                        offset: None,
                    }
                } else {
                    match callbacks.eval_expression(expression.as_str()) {
                        Ok(result) => HostResult::Data {
                            id,
                            data: CS::from(result),
                            offset: None,
                        },
                        Err(e) => HostResult::Failure {
                            id,
                            error: CS::from(e),
                        },
                    }
                }
            }

            HostRequest::CustomExCommand { command, .. } => {
                match callbacks.custom_ex_command(command.as_str()) {
                    Ok(Some(text)) => HostResult::Data {
                        id,
                        data: CS::from(text),
                        offset: None,
                    },
                    Ok(None) => HostResult::Success { id, message: None },
                    Err(e) => HostResult::Failure {
                        id,
                        error: CS::from(e),
                    },
                }
            }

            _ => HostResult::Success { id, message: None },
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // 4. Document access
    // ─────────────────────────────────────────────────────────────────────

    /// Get the full document text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.host().document.text()
    }

    /// Get a single line by 0-indexed line number.
    #[must_use]
    pub fn line(&self, n: usize) -> Option<&str> {
        use crate::primitives::LineNumber;
        self.host().document.line(LineNumber::new(n))
    }

    /// Get the total number of lines in the document.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.host().document.line_count()
    }

    /// Replace the entire document text.
    ///
    /// This is a raw document mutator that does NOT update the shadow document.
    /// Use [`replace_text`](Self::replace_text) for intentional full-document
    /// replacements that should keep the engine in sync.
    pub fn set_text(&mut self, text: &str) {
        self.host_mut().document.set_text(text.to_owned());
    }

    /// Reset the undo tree after a document sync.
    ///
    /// After `syncText` replaces the document wholesale, existing undo entries
    /// reference pre-sync text and would produce garbage if replayed. This
    /// fences the undo tree so `u` cannot cross the sync boundary.
    pub fn fence_undo(&mut self) {
        self.engine_mut().undo_tree_mut().fence();
    }

    /// Replace the entire document text with undo control.
    pub fn replace_text(&mut self, new_text: &str, undoable: bool) {
        use crate::primitives::UndoCursorStrategy;
        use crate::state::mark_snapshot::MarkSnapshot;
        use crate::state::UndoTree;

        let (engine, host) = self.engine_and_host_mut();

        // 1. Unconditional cleanup: abandon any pending undo group
        engine.undo_tree_mut().abandon_pending();

        // 2. Force normal mode
        engine.set_mode(Mode::Normal);

        // 3. Early exit for identical text in undoable mode
        if undoable && host.document.text() == new_text {
            return;
        }

        // 4. Flush pruned-id drain channel before replacing stores
        let _ = engine.undo_tree_mut().take_pruned_ids();

        // 5. Undo handling
        if undoable {
            let old_text = host.document.text().to_owned();
            let cursor_before = Offset::new(host.cursor_offset);

            *engine.undo_tree_mut() = UndoTree::new();
            host.undo_store = UndoStore::new();

            host.undo_store.begin_group(&old_text, cursor_before);
            let mode = engine.mode();
            let last_visual = engine.state().last_visual();
            engine.undo_tree_mut().begin_group(
                cursor_before,
                UndoCursorStrategy::FirstEdit,
                MarkSnapshot::new(),
                Some(old_text.len()),
                mode,
                last_visual,
                false,
            );
            engine.undo_tree_mut().mark_edit_at(Offset::ZERO);

            host.document.set_text(new_text.to_owned());

            let cursor_after = Offset::ZERO;
            let node_id = engine
                .undo_tree_mut()
                .end_group(cursor_after, 0, Some(&old_text));
            host.undo_store
                .end_group(node_id, host.document.text(), cursor_after);
        } else {
            *engine.undo_tree_mut() = UndoTree::new();
            host.undo_store = UndoStore::new();
            host.document.set_text(new_text.to_owned());
        }

        // 6. Wipe local marks (a-z) and changelist
        engine.marks_mut().clear_local();
        engine.changelist_mut().clear();

        // 7. Cursor to origin
        host.cursor_offset = 0;

        // 8. Update shadow to prevent spurious drift on next process()
        engine.set_shadow_text(new_text);
    }

    /// Get a shared reference to the underlying document.
    #[must_use]
    pub const fn document(&self) -> &SessionDocument {
        &self.host().document
    }

    /// Apply an external edit to the document without touching the undo stack.
    pub fn apply_external_edit(&mut self, offset: usize, delete_count: usize, insert_text: &str) {
        let host = self.host_mut();
        let doc_len = host.document.len();
        let offset = offset.min(doc_len);
        let delete_end = (offset + delete_count).min(doc_len);
        let actual_delete = delete_end - offset;

        if actual_delete > 0 {
            host.document.apply_delete(offset, delete_end);
        }
        if !insert_text.is_empty() {
            host.document.apply_insert(offset, insert_text);
        }

        let insert_len = insert_text.len();
        if delete_end <= host.cursor_offset {
            let delta = byte_delta::delta(insert_len, actual_delete);
            host.cursor_offset = host.cursor_offset.saturating_add_signed(delta);
        } else if offset <= host.cursor_offset {
            host.cursor_offset = offset + insert_len;
        }

        let new_len = host.document.len();
        host.cursor_offset = host.cursor_offset.min(new_len);

        if let Some(sel) = host.selection {
            let anchor_raw = sel.anchor().get();
            let head_raw = sel.head().get();

            let sel_min = anchor_raw.min(head_raw);
            let sel_max = anchor_raw.max(head_raw);
            if offset <= sel_min && delete_end >= sel_max {
                host.selection = None;
            } else {
                let adjust = |pos: usize| -> usize {
                    if delete_end <= pos {
                        let delta = byte_delta::delta(insert_len, actual_delete);
                        pos.saturating_add_signed(delta).min(new_len)
                    } else if offset <= pos {
                        (offset + insert_len).min(new_len)
                    } else {
                        pos.min(new_len)
                    }
                };

                let new_anchor = Offset::new(adjust(anchor_raw));
                let new_head = Offset::new(adjust(head_raw));
                host.selection = Some(SelectionRange::new(new_anchor, new_head));
            }
        }
    }

    /// Notify the engine of an external edit AND record the undo snapshot.
    ///
    /// Unlike the generic `VimSession::notify_external_edit`, this variant
    /// captures the pre-edit document text so the `UndoStore` can produce
    /// correct text snapshots when the user presses `u`. The generic path
    /// cannot do this because the host document is already mutated by the
    /// time `apply_effects` runs.
    ///
    /// Use this instead of `notify_external_edit` when the host document
    /// has ALREADY been mutated (the common case for `SessionHost`).
    pub fn notify_external_edit_host(
        &mut self,
        edit: crate::execution::ExternalEdit,
    ) -> crate::execution::host_api::ProcessResult {
        // Capture pre-edit text from the engine's SHADOW (not host document).
        // The host document is already mutated by the time the host calls this,
        // so self.host().document.text() is post-edit. The shadow still holds
        // the pre-edit state until engine.apply_external_edit() updates it.
        let pre_edit_text = self
            .engine()
            .shadow_text()
            .map_or_else(|| self.host().document.text().to_owned(), str::to_owned);
        let cursor_before = Offset::new(self.host().cursor_offset);

        let response = self.engine_mut().apply_external_edit(edit);
        let consumed = response.consumed();

        let fc_node = self.engine_mut().take_last_force_committed_node();
        let ext_node = self.engine_mut().take_last_external_edit_node();

        if let Some(fc_node) = fc_node {
            let host = self.host_mut();
            let cursor_after = Offset::new(host.cursor_offset);
            let post_text = host.document.text().to_owned();
            host.undo_store
                .end_group(Some(fc_node), &post_text, cursor_after);
        }

        if let Some(ext_node) = ext_node {
            let host = self.host_mut();
            let cursor_after = Offset::new(host.cursor_offset);
            host.undo_store.begin_group(&pre_edit_text, cursor_before);
            let post_text = host.document.text().to_owned();
            host.undo_store
                .end_group(Some(ext_node), &post_text, cursor_after);
        }

        if fc_node.is_some() && self.engine().mode().is_insert() {
            let host = self.host_mut();
            let cursor = Offset::new(host.cursor_offset);
            let text = host.document.text().to_owned();
            host.undo_store.begin_group(&text, cursor);
        }

        self.deliver_effects(response.effects());

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);

        crate::execution::host_api::ProcessResult {
            consumed,
            host_requests,
            deferred_actions: std::mem::take(&mut self.pending_deferred),
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // 5. State queries
    // ─────────────────────────────────────────────────────────────────────

    /// Set the current mode programmatically.
    #[inline]
    pub const fn set_mode(&mut self, mode: Mode) {
        self.engine_mut().set_mode(mode);
    }

    /// Current cursor byte offset.
    #[must_use]
    pub const fn cursor_offset(&self) -> usize {
        self.host().cursor_offset
    }

    /// Set the cursor byte offset, clamping to the document length.
    pub fn set_cursor_offset(&mut self, offset: usize) {
        let clamped = offset.min(self.host().document.len());
        self.host_mut().cursor_offset = clamped;
        // Sync engine's multi-cursor primary selection so add_cursor() sees
        // the correct primary position, not a stale one from before motions.
        self.engine_mut().sync_primary_cursor(clamped);
    }

    /// Monotonically increasing frame counter.
    #[must_use]
    pub const fn frame_id(&self) -> u64 {
        self.host().frame_id
    }

    /// Whether the engine would handle this key in the current state.
    #[must_use]
    pub fn would_handle_key(&self, key: KeyEvent) -> bool {
        if self.engine().would_handle_key(key) {
            return true;
        }
        self.engine().could_start_mapping(key)
    }

    /// Whether the mapping expander has buffered keys awaiting timeout.
    #[must_use]
    pub const fn has_pending_mapping(&self) -> bool {
        self.engine().has_pending_mapping()
    }

    /// Current mapping timeout in milliseconds.
    #[must_use]
    pub const fn timeoutlen(&self) -> u32 {
        self.engine().timeoutlen()
    }

    /// Set the mapping timeout in milliseconds.
    pub const fn set_timeoutlen(&mut self, ms: u32) {
        self.engine_mut().set_timeoutlen(ms);
    }

    /// Close a stale auto-opened undo group if the time window has expired.
    pub fn tick_undo_auto_group(&mut self, now_ms: u64) -> bool {
        self.engine_mut().tick_undo_auto_group(now_ms)
    }

    /// Return a serializable snapshot of the undo tree for visualization.
    pub fn undo_tree_snapshot(&self) -> crate::primitives::UndoTreeSnapshot {
        self.engine().undo_tree().snapshot()
    }

    /// Return the undo tree's next sequence number (cheap change check).
    pub const fn undo_tree_sequence(&self) -> u64 {
        self.engine().undo_tree().next_sequence()
    }

    /// Start recording a debugging session.
    pub fn start_recording_replay(&mut self) {
        self.engine_mut().start_recording_session();
    }

    /// Stop recording and return the recorder.
    pub const fn stop_recording_replay(&mut self) -> Option<super::SessionRecorder> {
        self.engine_mut().stop_recording_session()
    }

    // ─────────────────────────────────────────────────────────────────────
    // 6. Register access
    // ─────────────────────────────────────────────────────────────────────

    /// Get the contents of a register by name (primary entry only).
    #[must_use]
    pub fn get_register(&self, name: char) -> Option<(String, MotionType)> {
        let reg_name = RegisterName::new(name)?;
        if reg_name == RegisterName::LAST_INSERT {
            let text = self.engine().state().last_inserted_text();
            if text.is_empty() {
                return None;
            }
            return Some((text.to_owned(), MotionType::CharWise));
        }
        let content = self.engine().state().registers().get(reg_name)?;
        Some((content.text().to_owned(), content.motion_type()))
    }

    /// Get the number of entries in a register (1 for single-cursor, N for multi-cursor yank).
    #[must_use]
    pub fn get_register_entry_count(&self, name: char) -> usize {
        let Some(reg_name) = RegisterName::new(name) else {
            return 0;
        };
        self.engine()
            .state()
            .registers()
            .get(reg_name)
            .map_or(0, RegisterContent::entry_count)
    }

    /// Get a specific entry from a multi-entry register.
    #[must_use]
    pub fn get_register_entry(&self, name: char, index: usize) -> Option<String> {
        let reg_name = RegisterName::new(name)?;
        let content = self.engine().state().registers().get(reg_name)?;
        Some(content.entry(index).to_owned())
    }

    /// Set the contents of a register.
    pub fn set_register(&mut self, name: char, content: &str, motion_type: MotionType) {
        let Some(reg_name) = RegisterName::new(name) else {
            return;
        };
        self.engine_mut()
            .registers_mut()
            .set(reg_name, RegisterContent::new(content, motion_type));
    }

    // ─────────────────────────────────────────────────────────────────────
    // 7. Configuration delegates
    // ─────────────────────────────────────────────────────────────────────

    /// Returns the current viewport geometry.
    #[must_use]
    pub const fn viewport(&self) -> ViewportInfo {
        self.host().viewport
    }

    /// Set the viewport geometry.
    pub const fn set_viewport(&mut self, viewport: ViewportInfo) {
        self.host_mut().viewport = viewport;
    }

    /// Set a cached indent hint for the next `o`/`O`/Enter command.
    #[inline]
    pub fn set_indent_hint(&mut self, indent: &str) {
        self.engine_mut().set_indent_hint(indent);
    }

    /// Clear the cached indent hint.
    #[inline]
    pub fn clear_indent_hint(&mut self) {
        self.engine_mut().clear_indent_hint();
    }

    /// Set a simple indent action (indent + optional append text).
    #[inline]
    pub fn set_indent_action_simple(&mut self, indent: &str, append: Option<&str>) {
        self.engine_mut().set_indent_action_simple(indent, append);
    }

    /// Set an indent-outdent action (two newlines for bracket pairs).
    #[inline]
    pub fn set_indent_action_outdent(
        &mut self,
        indent: &str,
        append: Option<&str>,
        closing_indent: &str,
    ) {
        self.engine_mut()
            .set_indent_action_outdent(indent, append, closing_indent);
    }

    /// Push fold state for FFI/WASM hosts.
    ///
    /// `hidden` is a list of `(start, end)` inclusive hidden-line ranges.
    /// Ranges must be non-overlapping and sorted in ascending order.
    #[inline]
    pub fn set_fold_state(
        &mut self,
        hidden: Vec<(crate::primitives::LineNumber, crate::primitives::LineNumber)>,
    ) {
        self.engine_mut().set_fold_state(hidden);
    }

    /// Clear the pushed fold state (all lines visible).
    #[inline]
    pub fn clear_fold_state(&mut self) {
        self.engine_mut().clear_fold_state();
    }

    /// Push display line state for FFI/WASM hosts.
    ///
    /// `wrap_column` is the column at which lines wrap (e.g. 80).
    /// `tab_size` is the number of spaces per tab stop (e.g. 4).
    #[inline]
    pub fn set_display_line_state(&mut self, wrap_column: usize, tab_size: usize) {
        self.engine_mut()
            .set_display_line_state(wrap_column, tab_size);
    }

    /// Clear the display line state (all lines unwrapped).
    #[inline]
    pub fn clear_display_line_state(&mut self) {
        self.engine_mut().clear_display_line_state();
    }

    /// Whether the engine is currently in dot-repeat mode.
    #[inline]
    #[must_use]
    pub const fn is_repeating(&self) -> bool {
        self.engine().is_repeating()
    }

    /// Whether the engine is processing live (non-repeat, non-macro) insert input.
    #[inline]
    #[must_use]
    pub fn is_live_insert_input(&self) -> bool {
        self.engine().is_live_insert_input()
    }

    /// Set the current buffer's file type for filetype-specific mappings.
    pub fn set_filetype(&mut self, filetype: Option<&str>) {
        self.engine_mut().set_filetype(filetype);
    }

    /// Enable or disable shadow execution for macro replays.
    pub const fn set_shadow_execution(&mut self, enabled: bool) {
        self.engine_mut().set_shadow_execution(enabled);
    }

    /// Query whether shadow execution is currently enabled.
    #[must_use]
    pub const fn shadow_execution_enabled(&self) -> bool {
        self.engine().shadow_execution_enabled()
    }

    /// Set (or replace) the self-healing shadow document text.
    ///
    /// Delegates to [`VimEngine::set_shadow_text`](crate::execution::VimEngine::set_shadow_text).
    pub fn set_shadow_text(&mut self, text: impl Into<String>) {
        self.engine_mut().set_shadow_text(text);
    }

    /// Enable or disable structured trace event collection.
    #[cfg(feature = "engine-tracing")]
    pub fn set_tracing_enabled(&mut self, enabled: bool) {
        self.engine_mut().set_tracing_enabled(enabled);
    }

    /// Query whether trace event collection is currently enabled.
    #[cfg(feature = "engine-tracing")]
    #[must_use]
    pub fn tracing_enabled(&self) -> bool {
        self.engine().tracing_enabled()
    }

    /// Drain all collected trace events, returning them.
    #[cfg(feature = "engine-tracing")]
    pub fn drain_trace_events(&mut self) -> Vec<TraceEvent> {
        self.engine_mut().drain_trace_events()
    }

    /// Build an [`InspectSnapshot`](crate::execution::trace::InspectSnapshot) with both engine and host state.
    #[cfg(feature = "engine-tracing")]
    #[must_use]
    pub fn inspect(&self) -> crate::execution::trace::InspectSnapshot {
        let mut snapshot = self.engine().inspect();
        snapshot.document_len = self.host().document.len();
        snapshot.cursor_offset = self.host().cursor_offset;
        snapshot
    }

    /// Take all effects captured since the last `process_key_host` call.
    #[cfg(feature = "testing")]
    pub fn take_captured_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.host_mut().captured_effects)
    }

    /// Number of times the undo store used the checkpoint fallback path.
    #[cfg(feature = "testing")]
    #[must_use]
    pub const fn undo_store_checkpoint_fallback_count(&self) -> u32 {
        self.host().undo_store.checkpoint_fallback_count()
    }

    /// Set engine options.
    pub fn set_options(&mut self, opts: VimOptions) {
        self.engine_mut().set_options(opts);
    }

    /// Get current engine options.
    #[must_use]
    pub const fn options(&self) -> &VimOptions {
        self.engine().options()
    }

    /// Mutable reference to the engine's global options.
    pub const fn options_mut(&mut self) -> &mut VimOptions {
        self.engine_mut().options_mut()
    }

    /// Set one option the way `:set` does, so that it takes effect in the
    /// current buffer over an earlier `:set` or `:setlocal`. See
    /// [`VimEngine::set_option`](crate::execution::VimEngine::set_option).
    pub fn set_option(
        &mut self,
        id: crate::primitives::OptionId,
        value: &crate::primitives::OptionValue,
    ) {
        self.engine_mut().set_option(id, value);
    }

    /// Force an immediate rebuild of the resolved-options cache.
    pub fn invalidate_option_cache(&mut self) {
        self.engine_mut().invalidate_option_cache();
    }

    /// Return the effective value of an option.
    #[must_use]
    pub fn effective_option(
        &self,
        id: crate::primitives::OptionId,
    ) -> crate::primitives::OptionValue {
        self.engine().effective_option(id)
    }

    /// Replace all user-defined digraphs.
    pub fn set_digraphs<I>(&mut self, digraphs: I)
    where
        I: IntoIterator<Item = (char, char, char)>,
    {
        let reg = self.engine_mut().digraph_registry_mut();
        reg.clear();
        for (c1, c2, result) in digraphs {
            reg.add(c1, c2, result);
        }
    }

    /// Replace the cached document symbol tree used by semantic text objects.
    pub fn set_document_symbols(&mut self, tree: crate::document::CachedSymbolTree) {
        self.engine_mut()
            .register_semantic_textobject_provider(Box::new(tree));
    }

    /// Add a key mapping.
    pub fn map(
        &mut self,
        mode: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
    ) {
        self.engine_mut().map(mode, from, to, kind, flags);
    }

    /// Remove a key mapping.
    pub fn unmap(&mut self, mode: MappingMode, from: &[KeyEvent]) {
        let _ = self.engine_mut().unmap(mode, from);
    }

    /// Clear all key mappings.
    pub fn clear_mappings(&mut self) {
        self.engine_mut().clear_mappings();
    }

    /// Register a batch of key mappings contributed by a host extension.
    pub fn register_host_mappings(&mut self, name: &str, mappings: &[super::HostMapping]) {
        self.engine_mut().register_host_mappings(name, mappings);
    }

    /// Remove all key mappings previously registered by a host extension.
    pub fn unregister_host_mappings(&mut self, name: &str) {
        self.engine_mut().unregister_host_mappings(name);
    }

    /// Set the leader key.
    pub const fn set_leader(&mut self, key: KeyEvent) {
        self.engine_mut().set_leader(key);
    }

    /// Take the key interest set if dirty, clearing the dirty flag.
    #[must_use]
    pub fn take_key_interest_if_dirty(&mut self) -> Option<crate::execution::KeyInterestSet> {
        let engine = self.engine_mut();
        if engine.key_interest_dirty {
            engine.key_interest_dirty = false;
            Some(engine.compute_key_interest())
        } else {
            None
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // 8. Buffer lifecycle
    // ─────────────────────────────────────────────────────────────────────

    /// Save all per-buffer state and reset the engine for a buffer switch.
    pub fn on_buffer_leave_host(
        &mut self,
        cursor_offset: usize,
    ) -> super::engine::buffer_state::BufferLocalState {
        self.engine_mut().on_buffer_leave(cursor_offset)
    }

    /// Restore per-buffer state from a previous `on_buffer_leave` call.
    pub fn on_buffer_enter_host(&mut self, state: super::engine::buffer_state::BufferLocalState) {
        self.engine_mut().on_buffer_enter(state);
    }

    // ─────────────────────────────────────────────────────────────────────
    // 9. Mark access
    // ─────────────────────────────────────────────────────────────────────

    /// Get the position of a mark by name.
    #[must_use]
    pub fn get_mark(&self, name: char) -> Option<usize> {
        use crate::primitives::MarkName;
        let mark_name = MarkName::new(name)?;
        self.engine()
            .state()
            .marks()
            .get(mark_name)
            .map(|m| m.offset().get())
    }

    /// Set the position of a mark.
    pub fn set_mark(&mut self, name: char, offset: usize) -> bool {
        use crate::primitives::{Mark, MarkName};
        let Some(mark_name) = MarkName::new(name) else {
            return false;
        };
        self.engine_mut()
            .marks_mut()
            .set(mark_name, Mark::from_raw(offset));
        true
    }

    /// Iterate over all global marks (A-Z) that have buffer associations.
    pub fn global_marks(
        &self,
    ) -> impl Iterator<Item = (crate::primitives::MarkName, &crate::state::GlobalMarkEntry)> + '_
    {
        self.engine().state().marks().globals()
    }

    /// Set a global mark (A-Z) with a buffer association.
    pub fn set_global_mark(
        &mut self,
        name: crate::primitives::MarkName,
        mark: crate::primitives::Mark,
        buffer_id: crate::primitives::BufferId,
    ) {
        self.engine_mut()
            .marks_mut()
            .set_with_buffer_id(name, mark, Some(buffer_id));
    }

    // ─────────────────────────────────────────────────────────────────────
    // 10. Search state
    // ─────────────────────────────────────────────────────────────────────

    /// Get the current search pattern.
    #[must_use]
    pub fn search_pattern(&self) -> Option<&str> {
        self.engine().state().search().pattern()
    }

    /// Get the current search direction.
    #[must_use]
    pub const fn search_direction(&self) -> crate::primitives::SearchDirection {
        self.engine().state().search().direction()
    }

    /// Get search match position info ("Match N of M").
    #[must_use]
    pub const fn search_match_info(&self) -> Option<&SearchMatchInfo> {
        self.host().last_search_match_info.as_ref()
    }

    /// Get search highlight ranges for the current search pattern.
    #[must_use]
    pub fn search_highlights(&self) -> Vec<(usize, usize)> {
        let host = self.host();
        if host.search_highlights_suppressed {
            return Vec::new();
        }
        let Some(pattern) = self.engine().state().search().pattern() else {
            return Vec::new();
        };
        if pattern.is_empty() {
            return Vec::new();
        }
        let text = host.document.text();
        let (actual_pat, word_boundary) =
            if pattern.starts_with("\\<") && pattern.ends_with("\\>") && pattern.len() > 4 {
                (&pattern[2..pattern.len() - 2], true)
            } else {
                (pattern, false)
            };
        if actual_pat.is_empty() {
            return Vec::new();
        }
        text.match_indices(actual_pat)
            .filter(|&(pos, _)| {
                if !word_boundary {
                    return true;
                }
                let at_word_start = pos == 0
                    || text[..pos]
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '_');
                let end = pos + actual_pat.len();
                let at_word_end = end >= text.len()
                    || text[end..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '_');
                at_word_start && at_word_end
            })
            .map(|(start, matched)| (start, start + matched.len()))
            .collect()
    }

    /// Get the current error message (equivalent to Neovim's `v:errmsg`).
    #[must_use]
    pub fn errmsg(&self) -> Option<String> {
        self.host().last_error.clone()
    }

    /// Set the search pattern and direction.
    pub fn set_search_pattern(
        &mut self,
        pattern: &str,
        direction: crate::primitives::SearchDirection,
    ) {
        self.engine_mut().set_search_pattern(pattern, direction);
    }

    // ─────────────────────────────────────────────────────────────────────
    // 11. (reserved)
    // ─────────────────────────────────────────────────────────────────────

    // ─────────────────────────────────────────────────────────────────────
    // Private: internal expression evaluation
    // ─────────────────────────────────────────────────────────────────────

    /// Try to evaluate a Vim expression engine-side without host involvement.
    fn try_evaluate_internal(&self, expr: &str) -> Option<String> {
        let norm = expr.trim().replace('"', "'");
        match norm.as_str() {
            "line('.')" => {
                let (line, _) = self
                    .host()
                    .document
                    .offset_to_pos(Offset::new(self.host().cursor_offset))
                    .map_or((0, 0), |pos| (pos.line().get(), pos.col().get()));
                Some((line + 1).to_string())
            }
            "line('$')" => Some(self.host().document.line_count().to_string()),
            "col('.')" => {
                let (_, col) = self
                    .host()
                    .document
                    .offset_to_pos(Offset::new(self.host().cursor_offset))
                    .map_or((0, 0), |pos| (pos.line().get(), pos.col().get()));
                Some((col + 1).to_string())
            }
            "mode()" => Some(self.engine().mode().short_name().to_owned()),
            _ => None,
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Private: gc_pruned_undo_snapshots
    // ─────────────────────────────────────────────────────────────────────

    fn gc_pruned_undo_snapshots_impl(
        engine: &mut crate::execution::engine::VimEngine,
        host: &mut SessionHost,
    ) {
        let pruned = engine.undo_tree_mut().take_pruned_ids();
        if !pruned.is_empty() {
            host.undo_store.remove_pruned(&pruned);
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Private: build_host_response
    // ─────────────────────────────────────────────────────────────────────

    /// Build a `HostResponse` from current engine state and host-owned fields.
    fn build_host_response_impl(
        engine: &mut crate::execution::engine::VimEngine,
        host: &mut SessionHost,
        consumed: bool,
        host_requests: Vec<HostRequest>,
    ) -> HostResponse {
        // Auto-handle defaults need &self for expression evaluation. Since we
        // can't pass &self with split borrows, we inline the auto-handle logic
        // directly here (the expression evaluation reads host.document and
        // engine.state(), both available as split borrows).
        let host_requests = if host.auto_handle_defaults {
            Self::auto_handle_requests_split(engine, host, host_requests)
        } else {
            host_requests
        };

        Self::gc_pruned_undo_snapshots_impl(engine, host);
        let state = engine.state();
        let mode = state.mode();

        let (cursor_line, cursor_col) = host
            .document
            .offset_to_pos(Offset::new(host.cursor_offset))
            .map_or((0, 0), |pos| (pos.line().get(), pos.col().get()));

        let scroll = state.scroll_hint().map(|hint| match hint {
            ScrollHint::ToOffset(o) => ScrollInfo {
                target_line: host.document.line_of_offset(o.get()),
                placement: ScrollPlacement::Visible,
            },
            ScrollHint::CenterCursor => ScrollInfo {
                target_line: host.document.line_of_offset(host.cursor_offset),
                placement: ScrollPlacement::Center,
            },
            ScrollHint::CursorToTop => ScrollInfo {
                target_line: host.document.line_of_offset(host.cursor_offset),
                placement: ScrollPlacement::Top,
            },
            ScrollHint::CursorToBottom => ScrollInfo {
                target_line: host.document.line_of_offset(host.cursor_offset),
                placement: ScrollPlacement::Bottom,
            },
        });

        let selection = host.selection.map(|sel| {
            let (a_line, a_col) = host
                .document
                .offset_to_pos(Offset::new(sel.anchor().get()))
                .map_or((0, 0), |p| (p.line().get(), p.col().get()));
            let (h_line, h_col) = host
                .document
                .offset_to_pos(Offset::new(sel.head().get()))
                .map_or((0, 0), |p| (p.line().get(), p.col().get()));
            SelectionInfo {
                anchor_line: a_line,
                anchor_col: a_col,
                head_line: h_line,
                head_col: h_col,
                shape: mode
                    .visual_type()
                    .map_or(SelectionShape::Char, |vt| match vt {
                        VisualType::Char => SelectionShape::Char,
                        VisualType::Line => SelectionShape::Line,
                        VisualType::Block => SelectionShape::Block,
                    }),
            }
        });

        let command_line = if mode == Mode::CommandLine {
            let cl = state.command_line();
            let prompt = match cl.prompt() {
                CommandLinePrompt::Ex | CommandLinePrompt::ExVisual => ':',
                CommandLinePrompt::SearchForward => '/',
                CommandLinePrompt::SearchBackward => '?',
            };
            let (candidates, selected_index) = cl
                .completion_snapshot()
                .map_or((None, None), |(cands, idx)| {
                    (Some(cands.to_vec()), Some(idx))
                });
            Some(CommandLineInfo {
                prompt,
                input: cl.input().to_owned(),
                cursor_pos: cl.cursor(),
                candidates,
                selected_index,
            })
        } else {
            None
        };

        let clipboard = if host.clipboard_dirty {
            state
                .registers()
                .get(RegisterName::CLIPBOARD)
                .map(|r| r.text().to_owned())
        } else {
            None
        };

        let mark_changes = std::mem::take(&mut host.pending_mark_changes);
        let substitute_preview = host.pending_substitute_preview.take();
        normalize_edit_offsets(&mut host.pending_edits);
        let edits = std::mem::take(&mut host.pending_edits);

        let showcmd_text = {
            let raw = engine.pending_command_display();
            if let Some(ref sel) = selection {
                let rows = sel.anchor_line.abs_diff(sel.head_line) + 1;
                let cols = sel.anchor_col.abs_diff(sel.head_col) + 1;
                let prefix = match sel.shape {
                    SelectionShape::Block => format!("{rows}x{cols}"),
                    SelectionShape::Line => format!("{rows}"),
                    SelectionShape::Char if rows > 1 => format!("{rows}"),
                    SelectionShape::Char => format!("{cols}"),
                };
                if raw.is_empty() {
                    CompactString::from(prefix)
                } else {
                    CompactString::from(format!("{prefix} {raw}"))
                }
            } else {
                raw
            }
        };

        let key_hints = if host
            .capabilities()
            .has(super::host_api::HostCapability::WhichKey)
        {
            engine.key_hints(engine.keymap())
        } else {
            None
        };

        HostResponse {
            cursor_line,
            cursor_col,
            cursor_offset: host.cursor_offset,
            mode,
            visual_type: mode.visual_type(),
            cursor_shape: CursorShape::from_mode(mode),
            consumed,
            line_count: host.document.line_count(),
            dirty: host.dirty_tracker.take(),
            scroll,
            message: state.message().cloned(),
            clipboard,
            selection,
            host_requests,
            command_line,
            has_pending_mapping: engine.has_pending_mapping(),
            mark_changes,
            substitute_preview,
            showcmd_text,
            edits,
            recording_register: engine.recording_register().map(RegisterName::char),
            key_hints,
        }
    }

    /// Auto-complete host requests with split borrows (no &self needed).
    fn auto_handle_requests_split(
        engine: &mut crate::execution::engine::VimEngine,
        host: &mut SessionHost,
        mut host_requests: Vec<HostRequest>,
    ) -> Vec<HostRequest> {
        const MAX_DEPTH: usize = 5;
        let mut depth = 0;

        loop {
            let (pass_through, auto_handle): (Vec<_>, Vec<_>) = host_requests
                .into_iter()
                .partition(|req| super::host_defaults::default_result(req).is_none());

            if auto_handle.is_empty() || depth >= MAX_DEPTH {
                return pass_through;
            }

            let mut sub_requests = Vec::new();
            for req in &auto_handle {
                // Try engine-internal expression evaluation
                let internal_result = match req {
                    HostRequest::EvaluateExpression { expression, .. }
                    | HostRequest::EvaluateMapping { expression, .. } => {
                        try_evaluate_internal_split(
                            expression.as_str(),
                            &host.document,
                            host.cursor_offset,
                            engine,
                            req.id(),
                        )
                    }
                    _ => None,
                };
                let result = internal_result.or_else(|| super::host_defaults::default_result(req));
                if let Some(result) = result {
                    let mut response = engine.complete_host_request(&result);
                    host.apply_effects(response.effects());
                    sub_requests.extend(response.take_host_requests());
                }
            }

            host_requests = pass_through;
            host_requests.extend(sub_requests);
            depth += 1;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Multi-cursor API
// ─────────────────────────────────────────────────────────────────────────────

impl VimSession<SessionHost> {
    /// Add a cursor at the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns `VimError` if the operation fails.
    pub fn add_cursor(&mut self, offset: usize) -> Result<(), crate::errors::VimError> {
        // Sync primary selection to the host's cursor position before adding.
        // Without this, the primary selection can be stale after single-cursor
        // motions (the engine doesn't update selections when mc is inactive).
        let current = self.host().cursor_offset;
        self.engine_mut().sync_primary_cursor(current);

        let (engine, host) = self.engine_and_host_mut();
        let ctx = crate::execution::MultiCursorContext {
            text: host.document.text(),
            search_pattern: None,
            line_count: host.document.line_count(),
        };
        engine
            .execute_multi_cursor(
                &crate::state::MultiCursorCommand::AddCursor(Offset::new(offset)),
                &ctx,
            )
            .map(|_| ())
    }

    /// Remove the cursor nearest to the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns `VimError` if the last remaining cursor would be removed.
    pub fn remove_cursor(&mut self, offset: usize) -> Result<(), crate::errors::VimError> {
        let (engine, host) = self.engine_and_host_mut();
        let ctx = crate::execution::MultiCursorContext {
            text: host.document.text(),
            search_pattern: None,
            line_count: host.document.line_count(),
        };
        engine
            .execute_multi_cursor(
                &crate::state::MultiCursorCommand::RemoveCursor(Offset::new(offset)),
                &ctx,
            )
            .map(|_| ())
    }

    /// Remove all secondary cursors, leaving only the primary.
    pub fn clear_secondary_cursors(&mut self) {
        let (engine, host) = self.engine_and_host_mut();
        let ctx = crate::execution::MultiCursorContext {
            text: host.document.text(),
            search_pattern: None,
            line_count: host.document.line_count(),
        };
        let _ =
            engine.execute_multi_cursor(&crate::state::MultiCursorCommand::ClearSecondary, &ctx);
    }

    /// Rotate the primary cursor designation forward or backward.
    pub fn rotate_primary(&mut self, forward: bool) {
        use crate::primitives::Direction;
        let dir = if forward {
            Direction::Forward
        } else {
            Direction::Backward
        };
        let (engine, host) = self.engine_and_host_mut();
        let ctx = crate::execution::MultiCursorContext {
            text: host.document.text(),
            search_pattern: None,
            line_count: host.document.line_count(),
        };
        let _ = engine
            .execute_multi_cursor(&crate::state::MultiCursorCommand::RotatePrimary(dir), &ctx);
    }

    /// Select all occurrences of the word under the primary cursor.
    ///
    /// Replaces all cursors with one cursor per occurrence. Primary is set
    /// to the occurrence closest to the original cursor.
    ///
    /// # Errors
    ///
    /// Returns [`VimError::NoStringUnderCursor`](crate::errors::VimError::NoStringUnderCursor)
    /// if the primary cursor is not on a word character, so there is no word to
    /// search for. Once a word has been extracted the document is searched for
    /// that exact substring, which always matches at least where it came from,
    /// so the search itself does not fail.
    pub fn select_all_occurrences(&mut self) -> Result<(), crate::errors::VimError> {
        let current = self.host().cursor_offset;
        self.engine_mut().sync_primary_cursor(current);

        let (engine, host) = self.engine_and_host_mut();
        let ctx = crate::execution::MultiCursorContext {
            text: host.document.text(),
            search_pattern: None,
            line_count: host.document.line_count(),
        };
        engine
            .execute_multi_cursor(
                &crate::state::MultiCursorCommand::SelectAllOccurrences,
                &ctx,
            )
            .map(|_| ())
    }

    /// Add a cursor at the next occurrence of the word under cursor (Ctrl+D).
    ///
    /// If single cursor: keeps current + adds next match.
    /// If multiple cursors: removes primary + adds next match.
    ///
    /// # Errors
    ///
    /// Returns [`VimError::NoPreviousPattern`](crate::errors::VimError::NoPreviousPattern)
    /// when no pattern can be resolved: there is no match-search already in
    /// progress, the primary cursor is not on a word, and the search register
    /// is empty.
    ///
    /// Returns [`VimError::PatternNotFound`](crate::errors::VimError::PatternNotFound)
    /// carrying the resolved pattern when the search — which wraps around the
    /// end of the document — finds no occurrence that does not already hold a
    /// cursor. Reaching this means every match is already selected.
    pub fn add_next_match(&mut self) -> Result<(), crate::errors::VimError> {
        let current = self.host().cursor_offset;
        self.engine_mut().sync_primary_cursor(current);

        let search_pattern = self.engine().state().search().pattern().map(str::to_owned);

        let (engine, host) = self.engine_and_host_mut();
        let ctx = crate::execution::MultiCursorContext {
            text: host.document.text(),
            search_pattern: search_pattern.as_deref(),
            line_count: host.document.line_count(),
        };
        engine
            .execute_multi_cursor(
                &crate::state::MultiCursorCommand::AddNextMatch {
                    direction: crate::primitives::Direction::Forward,
                    skip: true,
                },
                &ctx,
            )
            .map(|_| ())
    }

    /// Return the number of active cursors.
    #[must_use]
    pub fn cursor_count(&self) -> usize {
        self.engine().state().multi_cursor().selections().len()
    }

    /// Return the position of each cursor as `(line, col, offset)`.
    #[must_use]
    pub fn cursor_positions(&self) -> Vec<(usize, usize, usize)> {
        let selections = self.engine().state().multi_cursor().selections();
        selections
            .ranges()
            .iter()
            .map(|r| {
                let offset = r.head().get();
                let (line, col) = self
                    .host()
                    .document
                    .offset_to_pos(Offset::new(offset))
                    .map_or((0, 0), |p| (p.line().get(), p.col().get()));
                (line, col, offset)
            })
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

impl VimSession<SessionHost> {
    /// Get selection as (anchor, head) raw offsets.
    pub fn selection_raw(&self) -> Option<(usize, usize)> {
        self.host()
            .selection
            .map(|s| (s.anchor().get(), s.head().get()))
    }
}

#[cfg(test)]
impl VimSession<SessionHost> {
    /// Test-only: set selection directly.
    pub(crate) fn set_selection_raw(&mut self, anchor: usize, head: usize) {
        self.host_mut().selection =
            Some(SelectionRange::new(Offset::new(anchor), Offset::new(head)));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Free function: try_evaluate_internal_split
// ─────────────────────────────────────────────────────────────────────────────

/// Internal expression evaluation with split borrows (no &self).
///
/// Same logic as `HostSession::try_evaluate_internal` but accepts split-borrowed
/// fields instead of `&self`.
fn try_evaluate_internal_split(
    expr: &str,
    document: &SessionDocument,
    cursor_offset: usize,
    engine: &crate::execution::engine::VimEngine,
    request_id: HostRequestId,
) -> Option<HostResult> {
    let norm = expr.trim().replace('"', "'");
    let result_str = match norm.as_str() {
        "line('.')" => {
            let (line, _) = document
                .offset_to_pos(Offset::new(cursor_offset))
                .map_or((0, 0), |pos| (pos.line().get(), pos.col().get()));
            (line + 1).to_string()
        }
        "line('$')" => document.line_count().to_string(),
        "col('.')" => {
            let (_, col) = document
                .offset_to_pos(Offset::new(cursor_offset))
                .map_or((0, 0), |pos| (pos.line().get(), pos.col().get()));
            (col + 1).to_string()
        }
        "mode()" => engine.mode().short_name().to_owned(),
        _ => return None,
    };
    Some(HostResult::Data {
        id: request_id,
        data: CompactString::from(result_str),
        offset: None,
    })
}

#[cfg(test)]
#[path = "session_host_errmsg_tests.rs"]
mod errmsg_tests;
