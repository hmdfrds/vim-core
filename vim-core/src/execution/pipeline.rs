//! Parse/resolve/execute pipeline orchestration.
//!
//! This module provides:
//! - [`resolve_grammar_result`] — grammar → `(PlannedAction, was_repeat)` resolver
//! - [`PlannedAction`] — typed action produced by resolve
//! - [`PipelineError`] — unified error type for all pipeline stages

use crate::grammar::{Command, GrammarResult, InputState};
use crate::keymap::KeyEvent;
use crate::primitives::Mode;
use thiserror::Error;

// ═══════════════════════════════════════════════════════════════════════════
// PlannedAction
// ═══════════════════════════════════════════════════════════════════════════

/// Planned action variant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlannedAction {
    /// Execute a fully parsed command.
    Execute(Command),
    /// Change mode immediately.
    ModeChange(Mode, Option<u32>),
    /// Wait for more keys.
    Pending,
    /// Ignore input.
    Ignored,
}

// ═══════════════════════════════════════════════════════════════════════════
// PipelineError (flat — replaces ParseStageError + ResolveStageError)
// ═══════════════════════════════════════════════════════════════════════════

/// Unified pipeline error — all stages report through one type.
///
/// Previously three nested types (ParseStageError → PipelineError::Parse,
/// ResolveStageError → PipelineError::Resolve). Flattened because all
/// variants get the same treatment: parser reset + `err.to_string()`.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PipelineError {
    /// Invalid key sequence for current parser state/mode.
    #[error("invalid key {key:?} in mode {mode:?}")]
    InvalidKey {
        /// Active mode during parse.
        mode: Mode,
        /// Key that failed parsing.
        key: KeyEvent,
    },

    /// Parser cancelled current command sequence.
    #[error("command cancelled by key {key:?} in mode {mode:?}")]
    Cancelled {
        /// Active mode during parse.
        mode: Mode,
        /// Key that triggered cancellation.
        key: KeyEvent,
    },

    /// Resolver received a terminal parse result that should have been
    /// handled by parse-stage error classification.
    #[error("resolve received terminal parse result '{result_kind}'")]
    UnexpectedTerminal {
        /// Result discriminator for diagnostics.
        result_kind: &'static str,
    },

    /// Insert-register substate may only occur in Insert/Replace mode.
    #[error("AwaitingInsertRegister in non-insert mode ({mode:?})")]
    InsertRegisterOutsideInsert {
        /// Active mode when resolver observed invalid state.
        mode: Mode,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// Resolve stage
// ═══════════════════════════════════════════════════════════════════════════

/// Resolve a grammar result into a `(PlannedAction, was_repeat)` pair.
///
/// # Errors
///
/// Returns [`PipelineError`] if parser output violates resolver preconditions.
pub fn resolve_grammar_result(
    mode: Mode,
    result: GrammarResult,
    was_repeat: bool,
) -> Result<(PlannedAction, bool), PipelineError> {
    match result {
        GrammarResult::Execute(command) => Ok((PlannedAction::Execute(command), was_repeat)),
        GrammarResult::ModeChange(mode, count) => {
            Ok((PlannedAction::ModeChange(mode, count), false))
        }
        GrammarResult::Continue(next_state) => resolve_continue_state(mode, &next_state),
        GrammarResult::Invalid => Err(PipelineError::UnexpectedTerminal {
            result_kind: "invalid",
        }),
        GrammarResult::Cancel => Err(PipelineError::UnexpectedTerminal {
            result_kind: "cancel",
        }),
    }
}

const fn resolve_continue_state(
    mode: Mode,
    next_state: &InputState,
) -> Result<(PlannedAction, bool), PipelineError> {
    if matches!(next_state, InputState::AwaitingInsertRegister) && !mode.is_insert() {
        return Err(PipelineError::InsertRegisterOutsideInsert { mode });
    }

    if let InputState::Operator {
        register, operator, ..
    } = next_state
    {
        if matches!(mode, Mode::Visual(_)) {
            return Ok((
                PlannedAction::Execute(Command::OperatorSelection {
                    register: *register,
                    operator: *operator,
                }),
                false,
            ));
        }
    }

    Ok((PlannedAction::Pending, false))
}

// ═══════════════════════════════════════════════════════════════════════════
// Parse classification
// ═══════════════════════════════════════════════════════════════════════════

/// Classify a parse result — converts Invalid/Cancel into PipelineError.
pub(crate) fn classify_parse_result(
    mode: Mode,
    key: KeyEvent,
    parsed: GrammarResult,
) -> Result<GrammarResult, PipelineError> {
    match parsed {
        GrammarResult::Invalid => Err(PipelineError::InvalidKey { mode, key }),
        GrammarResult::Cancel => Err(PipelineError::Cancelled { mode, key }),
        other => Ok(other),
    }
}
