//! Rich response type for host sessions.
//!
//! `HostResponse` is the Rust-native output of `VimSession::process_key()`.
//! Consumers convert it to their own wire format.

use compact_str::CompactString;

use crate::execution::dirty_tracker::DirtyInfo;
use crate::execution::host::HostRequest;
use crate::primitives::{KeyHintsInfo, Mode, SelectionShape, SubstitutePreviewMatch, VisualType};
use crate::state::StatusMessage;

// Re-export the canonical CursorShape from primitives.
pub use crate::primitives::CursorShape;

// ─────────────────────────────────────────────────────────────────────────────
// Supporting types
// ─────────────────────────────────────────────────────────────────────────────

/// Scroll target with placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScrollInfo {
    /// Target line to scroll to.
    pub target_line: usize,
    /// Where to place the target line in the viewport.
    pub placement: ScrollPlacement,
}

impl ScrollInfo {
    /// Target line to scroll to.
    #[inline]
    #[must_use]
    pub const fn target_line(&self) -> usize {
        self.target_line
    }

    /// Where to place the target line in the viewport.
    #[inline]
    #[must_use]
    pub const fn placement(&self) -> ScrollPlacement {
        self.placement
    }
}

/// Where to place the scroll target in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScrollPlacement {
    /// Just make it visible (default scroll).
    Visible,
    /// Center in viewport (zz).
    Center,
    /// Top of viewport (zt).
    Top,
    /// Bottom of viewport (zb).
    Bottom,
}

/// Selection in line/column coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SelectionInfo {
    /// Anchor line (0-indexed).
    pub anchor_line: usize,
    /// Anchor column (grapheme-based).
    pub anchor_col: usize,
    /// Head line (0-indexed).
    pub head_line: usize,
    /// Head column (grapheme-based).
    pub head_col: usize,
    /// Selection shape.
    pub shape: SelectionShape,
}

impl SelectionInfo {
    /// Anchor line (0-indexed).
    #[inline]
    #[must_use]
    pub const fn anchor_line(&self) -> usize {
        self.anchor_line
    }

    /// Anchor column (grapheme-based).
    #[inline]
    #[must_use]
    pub const fn anchor_col(&self) -> usize {
        self.anchor_col
    }

    /// Head line (0-indexed).
    #[inline]
    #[must_use]
    pub const fn head_line(&self) -> usize {
        self.head_line
    }

    /// Head column (grapheme-based).
    #[inline]
    #[must_use]
    pub const fn head_col(&self) -> usize {
        self.head_col
    }

    /// Selection shape.
    #[inline]
    #[must_use]
    pub const fn shape(&self) -> SelectionShape {
        self.shape
    }
}

/// Command-line state for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CommandLineInfo {
    /// Prompt character (':', '/', '?').
    pub prompt: char,
    /// Current input text.
    pub input: String,
    /// Cursor position within input (byte offset).
    pub cursor_pos: usize,
    /// Completion candidates, if a completion session is active.
    pub candidates: Option<Vec<crate::state::CompletionCandidate>>,
    /// Selected candidate index, if a completion session is active.
    pub selected_index: Option<usize>,
}

impl CommandLineInfo {
    /// Current input text.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &str {
        &self.input
    }

    /// Cursor position within input (byte offset).
    #[inline]
    #[must_use]
    pub const fn cursor_pos(&self) -> usize {
        self.cursor_pos
    }

    /// Prompt character (':', '/', '?').
    #[inline]
    #[must_use]
    pub const fn prompt(&self) -> char {
        self.prompt
    }
}

/// A mark change event (set or cleared).
///
/// This enum replaces the previous struct-with-sentinel pattern where
/// `u32::MAX` in the `line`/`col` fields indicated a cleared mark.
/// Now the distinction is a compile-time guarantee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkChangeInfo {
    /// A mark was set at the given position.
    Set {
        /// The mark character as a `u8` (e.g. `b'a'` for mark `a`).
        name: u8,
        /// Line number (0-indexed).
        line: u32,
        /// Column number (grapheme-based).
        col: u32,
        /// Whether this mark should be rendered in UI.
        /// `true` for `a-z`/`A-Z`, `false` for special marks (`.`, `^`, `[`, `]`, etc.).
        visible: bool,
    },
    /// A mark was cleared (deleted).
    Cleared {
        /// The mark character as a `u8` (e.g. `b'a'` for mark `a`).
        name: u8,
    },
}

/// A byte-level edit operation applied to the document.
///
/// Each `EditOp` describes a contiguous mutation: delete `delete` bytes
/// starting at `offset`, then insert `insert` at that position.
/// Consumers can replay these to keep an external document mirror in sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOp {
    /// Byte offset in the document where the edit starts.
    pub offset: usize,
    /// Number of bytes deleted at `offset`.
    pub delete: usize,
    /// Text inserted at `offset` (after deletion).
    pub insert: CompactString,
}

/// A yank-highlight region for visual feedback (vim-highlighted-yank).
/// Search match position info for "Match N of M" status display.
///
/// Returned by [`HostSession::search_match_info()`](crate::execution::HostSession)
/// after a successful search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatchInfo {
    /// 1-indexed position of the current match.
    pub current: usize,
    /// Total number of matches in the document.
    pub total: usize,
    /// Whether the count is complete (false if timed out or exceeded maxcount).
    pub complete: bool,
}

/// Substitute preview state for inccommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubstitutePreviewState {
    /// Active substitute preview with match entries.
    Matches(Vec<SubstitutePreviewMatch>),
    /// The substitute preview was cleared.
    Cleared,
}

// ─────────────────────────────────────────────────────────────────────────────
// HostResponse
// ─────────────────────────────────────────────────────────────────────────────

/// Complete response from `VimSession::process_key()`.
///
/// Contains everything a shell needs to render UI after processing a key.
/// Consumers convert this to whatever wire format their binding needs.
#[derive(Debug)]
pub struct HostResponse {
    // ── Cursor ───────────────────────────────────────────────────────────
    /// Cursor line (0-indexed).
    pub cursor_line: usize,
    /// Cursor column (byte offset within line, matches Neovim).
    pub cursor_col: usize,
    /// Cursor byte offset in document.
    pub cursor_offset: usize,

    // ── Mode ─────────────────────────────────────────────────────────────
    /// Current mode (full enum including OperatorPending, VirtualReplace).
    pub mode: Mode,
    /// Visual type if in visual mode.
    pub visual_type: Option<VisualType>,
    /// Cursor shape for rendering.
    pub cursor_shape: CursorShape,

    // ── Key handling ─────────────────────────────────────────────────────
    /// Whether the key was consumed by the engine.
    pub consumed: bool,

    // ── Document geometry ────────────────────────────────────────────────
    /// Total line count.
    pub line_count: usize,
    /// Dirty line info for incremental viewport updates.
    pub dirty: DirtyInfo,

    // ── Scroll ───────────────────────────────────────────────────────────
    /// Scroll target if the viewport should move.
    pub scroll: Option<ScrollInfo>,

    // ── Status ───────────────────────────────────────────────────────────
    /// Status message to display.
    pub message: Option<StatusMessage>,

    // ── Clipboard ────────────────────────────────────────────────────────
    /// Clipboard text if the `+` register was written.
    pub clipboard: Option<String>,

    // ── Selection ────────────────────────────────────────────────────────
    /// Visual selection in line/column coordinates.
    pub selection: Option<SelectionInfo>,

    // ── Host requests ────────────────────────────────────────────────────
    /// Host requests emitted during this key processing cycle.
    ///
    /// # Contract
    ///
    /// - All requests **must** be completed (via
    ///   [`VimSession::complete_request`](crate::execution::VimSession::complete_request) or
    ///   [`VimSession::complete_request_checked`](crate::execution::HostSession)) before the next
    ///   [`VimSession::process_key`](crate::execution::VimSession::process_key) call.
    /// - Requests may be completed in **any order** (out-of-order is fine).
    /// - Completing the same request ID twice returns
    ///   [`Err(UnknownRequestError)`](crate::execution::UnknownRequestError)
    ///   via the checked variant; the unchecked variant silently ignores it.
    pub host_requests: Vec<HostRequest>,

    // ── Command line ─────────────────────────────────────────────────────
    /// Command-line state if in command-line mode.
    pub command_line: Option<CommandLineInfo>,

    // ── Mapping state ────────────────────────────────────────────────────
    /// Whether the mapping expander has pending keys.
    pub has_pending_mapping: bool,

    // ── Mark changes ────────────────────────────────────────────────────
    /// Mark changes (set or cleared) that occurred during this key.
    pub mark_changes: Vec<MarkChangeInfo>,

    // ── Substitute preview ──────────────────────────────────────────────
    /// Substitute preview state change, if any.
    pub substitute_preview: Option<SubstitutePreviewState>,

    // ── Showcmd ────────────────────────────────────────────────────────
    /// Partial command display text (showcmd).
    ///
    /// Shows the partially-typed command (e.g., `"3d"`, `"ci"`, `"\"af"`)
    /// as the user builds a multi-key command. Empty when no command is
    /// in progress.
    pub showcmd_text: CompactString,

    // ── Edits ─────────────────────────────────────────────────────────
    /// Byte-level edit operations applied to the document during this key.
    ///
    /// Empty when the key produced no document mutations (e.g. cursor movement).
    /// Hosts can replay these to keep an external document mirror in sync.
    pub edits: Vec<EditOp>,

    // ── Recording state ─────────────────────────────────────────────
    /// Register currently being recorded into, if any.
    ///
    /// `Some('a')` means macro recording is active into register `a`.
    /// `None` means no macro is being recorded.
    pub recording_register: Option<char>,

    // ── Which-key hints ─────────────────────────────────────────────
    /// Which-key popup data: available continuation keys with descriptions.
    ///
    /// `Some` when the engine is awaiting a prefix continuation (grammar
    /// prefix like `g`/`z`/`Ctrl-W`, or a user mapping prefix like `<Leader>`).
    /// `None` when no prefix is pending.
    pub key_hints: Option<KeyHintsInfo>,
}

impl HostResponse {
    /// Cursor position as `(line, col)`.
    ///
    /// Convenience shorthand for `(self.cursor_line, self.cursor_col)`.
    #[inline]
    #[must_use]
    pub const fn cursor_pos(&self) -> (usize, usize) {
        (self.cursor_line, self.cursor_col)
    }

    /// Cursor line (0-indexed).
    #[inline]
    #[must_use]
    pub const fn cursor_line(&self) -> usize {
        self.cursor_line
    }

    /// Cursor column (byte offset within line, matches Neovim).
    #[inline]
    #[must_use]
    pub const fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    /// Cursor byte offset in document.
    #[inline]
    #[must_use]
    pub const fn cursor_offset(&self) -> usize {
        self.cursor_offset
    }

    /// Current mode (full enum including OperatorPending, VirtualReplace).
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Visual type if in visual mode.
    #[inline]
    #[must_use]
    pub const fn visual_type(&self) -> Option<VisualType> {
        self.visual_type
    }

    /// Cursor shape for rendering.
    #[inline]
    #[must_use]
    pub const fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    /// Whether the key was consumed by the engine.
    #[inline]
    #[must_use]
    pub const fn consumed(&self) -> bool {
        self.consumed
    }

    /// Total line count.
    #[inline]
    #[must_use]
    pub const fn line_count(&self) -> usize {
        self.line_count
    }

    /// Dirty line info for incremental viewport updates.
    #[inline]
    #[must_use]
    pub const fn dirty(&self) -> &DirtyInfo {
        &self.dirty
    }

    /// Scroll target if the viewport should move.
    #[inline]
    #[must_use]
    pub const fn scroll(&self) -> Option<&ScrollInfo> {
        self.scroll.as_ref()
    }

    /// Status message to display.
    #[inline]
    #[must_use]
    pub const fn message(&self) -> Option<&StatusMessage> {
        self.message.as_ref()
    }

    /// Clipboard text if the `+` register was written.
    #[inline]
    #[must_use]
    pub fn clipboard(&self) -> Option<&str> {
        self.clipboard.as_deref()
    }

    /// Visual selection in line/column coordinates.
    #[inline]
    #[must_use]
    pub const fn selection(&self) -> Option<&SelectionInfo> {
        self.selection.as_ref()
    }

    /// Host requests with full typed enum + correlation IDs.
    #[inline]
    #[must_use]
    pub fn host_requests(&self) -> &[HostRequest] {
        &self.host_requests
    }

    /// Command-line state if in command-line mode.
    #[inline]
    #[must_use]
    pub const fn command_line(&self) -> Option<&CommandLineInfo> {
        self.command_line.as_ref()
    }

    /// Whether the mapping expander has pending keys.
    #[inline]
    #[must_use]
    pub const fn has_pending_mapping(&self) -> bool {
        self.has_pending_mapping
    }

    /// Mark changes (set or cleared) that occurred during this key.
    #[inline]
    #[must_use]
    pub fn mark_changes(&self) -> &[MarkChangeInfo] {
        &self.mark_changes
    }

    /// Substitute preview state change, if any.
    #[inline]
    #[must_use]
    pub const fn substitute_preview(&self) -> Option<&SubstitutePreviewState> {
        self.substitute_preview.as_ref()
    }

    /// Partial command display text (showcmd).
    ///
    /// Returns the partially-typed command as the user would see it in the
    /// bottom-right corner of the Vim status line. Empty when no command
    /// is in progress.
    #[inline]
    #[must_use]
    pub fn showcmd_text(&self) -> &str {
        &self.showcmd_text
    }

    /// Byte-level edit operations applied to the document during this key.
    ///
    /// Empty when the key produced no document mutations.
    #[inline]
    #[must_use]
    pub fn edits(&self) -> &[EditOp] {
        &self.edits
    }

    /// Register currently being recorded into, if any.
    #[inline]
    #[must_use]
    pub const fn recording_register(&self) -> Option<char> {
        self.recording_register
    }

    /// Which-key popup data: available continuation keys with descriptions.
    ///
    /// `Some` when the engine is awaiting a prefix continuation.
    /// `None` when no prefix is pending.
    #[inline]
    #[must_use]
    pub const fn key_hints(&self) -> Option<&KeyHintsInfo> {
        self.key_hints.as_ref()
    }

    /// Compute the viewport top line from the engine's scroll state.
    ///
    /// This is a convenience method that eliminates the need for every
    /// host to reimplement the `(placement, target, height) → top` formula.
    /// Terminal-style hosts that maintain their own `viewport_top` integer
    /// can call this after every `process_key()` instead of writing their
    /// own scroll logic.
    ///
    /// Widget-backed hosts that delegate scrolling to their editor widget
    /// can ignore this and read `scroll()` directly.
    ///
    /// # Arguments
    ///
    /// * `current_top` — The current viewport top line index.
    /// * `viewport_height` — Number of document lines visible in the viewport.
    ///
    /// # Returns
    ///
    /// The new viewport top line (always ≥ 0).
    #[must_use]
    pub fn compute_viewport_top(&self, current_top: usize, viewport_height: usize) -> usize {
        let max_top = self.line_count().saturating_sub(viewport_height);
        let new_top = if let Some(scroll) = self.scroll() {
            let target = scroll.target_line();
            match scroll.placement() {
                // Explicit placements (zt, zz, zb) honor the user's intent
                // without clamping — Vim allows scrolling past the end for these.
                ScrollPlacement::Top => target,
                ScrollPlacement::Center => target.saturating_sub(viewport_height / 2),
                ScrollPlacement::Bottom => target.saturating_sub(viewport_height.saturating_sub(1)),
                _ => {
                    // ScrollPlacement::Visible and any future variants:
                    // keep target in view, then clamp to document end.
                    let mut top = current_top;
                    if target < top {
                        top = target;
                    } else if viewport_height > 0 && target >= top + viewport_height {
                        top = target - viewport_height + 1;
                    }
                    top.min(max_top)
                }
            }
        } else {
            // No engine scroll signal — just keep cursor visible,
            // then clamp to document end.
            let cursor = self.cursor_line();
            let mut top = current_top;
            if cursor < top {
                top = cursor;
            } else if viewport_height > 0 && cursor >= top + viewport_height {
                top = cursor - viewport_height + 1;
            }
            top.min(max_top)
        };
        new_top
    }
}
