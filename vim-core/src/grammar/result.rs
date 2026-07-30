//! Grammar result enum.
//!
//! Result of processing a key in the grammar parser.

use super::command::Command;
use super::input_state::InputState;
use crate::primitives::Mode;

/// Result of processing a key in the grammar parser.
///
/// The parser processes keys one at a time and returns
/// a result indicating what should happen next.
///
/// This result **must** be handled - ignoring it could leave
/// the parser and editor in an inconsistent state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "grammar result must be handled - ignoring may leave parser in inconsistent state"]
#[non_exhaustive]
pub enum GrammarResult {
    /// Need more input to complete the command.
    ///
    /// The parser transitions to the new state and waits
    /// for the next key.
    Continue(InputState),

    /// Command is complete and ready to execute.
    ///
    /// The parser resets to Ready state after returning this.
    Execute(Command),

    /// Switch to a different mode immediately.
    ///
    /// This is used for mode-switching commands like `i`, `v`, `:`.
    /// The parser resets to Ready state.
    /// The optional count is used by replace mode (`3R` → repeat on exit).
    ModeChange(Mode, Option<u32>),

    /// Invalid key in current context.
    ///
    /// The key press is not valid in the current state.
    /// The parser state is unchanged (user can try again
    /// or press Escape to cancel).
    Invalid,

    /// Escape pressed or command cancelled.
    ///
    /// The parser resets to Ready state.
    Cancel,
}

impl GrammarResult {
    /// Check if this result completes the command.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Execute(_) | Self::ModeChange(..) | Self::Cancel)
    }

    /// Check if this result needs more input.
    #[must_use]
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Continue(_))
    }

    /// Check if this result is invalid.
    #[must_use]
    pub const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid)
    }

    /// Get the command if this is an Execute result.
    #[must_use]
    pub const fn command(&self) -> Option<&Command> {
        match self {
            Self::Execute(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Get the mode if this is a `ModeChange` result.
    #[must_use]
    pub const fn mode(&self) -> Option<Mode> {
        match self {
            Self::ModeChange(mode, _) => Some(*mode),
            _ => None,
        }
    }

    /// Get the next state if this is a Continue result.
    #[must_use]
    pub const fn next_state(&self) -> Option<&InputState> {
        match self {
            Self::Continue(state) => Some(state),
            _ => None,
        }
    }
}
