//! Typed command-line intent captured at parse boundary.

use std::num::NonZeroU32;

use crate::grammar::types::Operator;
use crate::primitives::CommandLinePrompt;
use crate::primitives::RegisterName;

/// Operator intent attached to `/` or `?` from operator-pending mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorSearchIntent {
    /// Pending operator.
    pub operator: Operator,
    /// Effective count for operator-search (always >= 1).
    pub count: NonZeroU32,
    /// Explicit register, if any.
    pub register: Option<RegisterName>,
}

/// Command-line parse intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLineIntent {
    /// Prompt kind (`:`, `/`, `?`).
    pub prompt: CommandLinePrompt,
    /// Optional operator-search metadata.
    pub operator_search: Option<OperatorSearchIntent>,
}
