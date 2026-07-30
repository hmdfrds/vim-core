//! Transient per-keystroke state.
//!
//! These values are set by effects during `process()` and consumed
//! by the host after each keystroke. They are automatically cleared
//! at the start of each `process()` call via `VimState::clear_transient()`.
//!
//! # Architecture
//!
//! By storing transient output in `VimState`, the WASM host can read
//! state directly instead of processing intermediate effect instructions.
//! This eliminates the entire class of double-application bugs.

use crate::primitives::Offset;
use compact_str::CompactString;

/// A message to display in the status bar.
///
/// Set by `ShowMessage`, `ShowWarning`, `ShowError`, `ClearMessage` effects.
/// Cleared at the start of each `process()` call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StatusMessage {
    /// Informational message (e.g., "2 lines yanked").
    Info(CompactString),
    /// Error message (e.g., "E486: Pattern not found").
    Error(CompactString),
}

/// A scroll intent for the host to execute.
///
/// Set by scroll-related effects during `process()`.
/// Cleared at the start of each `process()` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ScrollHint {
    /// Scroll so that `offset` is visible.
    ToOffset(Offset),
    /// Center the cursor line vertically.
    CenterCursor,
    /// Scroll cursor to the top of the viewport.
    CursorToTop,
    /// Scroll cursor to the bottom of the viewport.
    CursorToBottom,
}
