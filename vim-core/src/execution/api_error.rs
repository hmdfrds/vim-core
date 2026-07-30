//! Universal API error type.
//!
//! [`ApiError`] is the single error type returned by all universal API
//! operations. It is `#[non_exhaustive]` so new variants can be added in
//! minor releases without breaking downstream matches.

use std::fmt;

use compact_str::CompactString;

use crate::execution::host_api::HostCapability;
use crate::primitives::{BufferId, CapabilityTier, VarScope};

/// Error returned by universal API operations.
///
/// All variants represent caller-recoverable conditions (bad arguments,
/// missing state, resource limits). Internal engine panics are not
/// represented here.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ApiError {
    /// The caller's capability tier is too low for this operation.
    InsufficientTier {
        /// The tier this operation requires.
        required: CapabilityTier,
        /// The tier the caller actually holds.
        have: CapabilityTier,
    },

    /// The requested host capability is not available on this host.
    UnsupportedCapability(HostCapability),

    /// A byte offset is outside the buffer's valid range.
    OffsetOutOfBounds {
        /// The requested offset.
        offset: usize,
        /// The length of the buffer.
        len: usize,
    },

    /// A byte offset does not lie on a UTF-8 character boundary.
    InvalidCharBoundary {
        /// The invalid offset.
        offset: usize,
    },

    /// A byte range is invalid (start > end, or either end is out of bounds).
    InvalidRange {
        /// The start of the range.
        start: usize,
        /// The end of the range.
        end: usize,
    },

    /// No buffer exists with the given id.
    BufferNotFound(BufferId),

    /// No variable with the given name exists in the given scope.
    VariableNotFound {
        /// The scope that was searched.
        scope: VarScope,
        /// The variable name that was not found.
        name: CompactString,
    },

    /// No register with the given name exists.
    RegisterNotFound(char),

    /// No mark with the given name exists.
    MarkNotFound(char),

    /// No option with the given name exists.
    OptionNotFound(CompactString),

    /// An undo group was opened while another was already open.
    NestedUndoGroup,

    /// An undo group was closed without a matching open.
    UndoGroupNotOpen,

    /// The operation was aborted because the fuel limit was reached.
    FuelExhausted,

    /// The serialised state size exceeds the configured limit.
    StateSizeExceeded {
        /// The current serialised size in bytes.
        current: usize,
        /// The configured limit in bytes.
        limit: usize,
    },

    /// Expression evaluation failed.
    RuntimeError(CompactString),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::InsufficientTier { required, have } => {
                write!(
                    f,
                    "insufficient capability tier: required {required:?}, have {have:?}"
                )
            }
            ApiError::UnsupportedCapability(cap) => {
                write!(f, "host capability not supported: {cap:?}")
            }
            ApiError::OffsetOutOfBounds { offset, len } => {
                write!(
                    f,
                    "offset {offset} is out of bounds for buffer of length {len}"
                )
            }
            ApiError::InvalidCharBoundary { offset } => {
                write!(
                    f,
                    "offset {offset} does not lie on a UTF-8 character boundary"
                )
            }
            ApiError::InvalidRange { start, end } => {
                write!(f, "invalid range {start}..{end}")
            }
            ApiError::BufferNotFound(id) => {
                write!(f, "buffer not found: {id}")
            }
            ApiError::VariableNotFound { scope, name } => {
                write!(f, "variable not found: {name:?} in scope {scope:?}")
            }
            ApiError::RegisterNotFound(reg) => {
                write!(f, "register not found: {reg:?}")
            }
            ApiError::MarkNotFound(mark) => {
                write!(f, "mark not found: {mark:?}")
            }
            ApiError::OptionNotFound(name) => {
                write!(f, "option not found: {name:?}")
            }
            ApiError::NestedUndoGroup => {
                write!(f, "cannot open a nested undo group")
            }
            ApiError::UndoGroupNotOpen => {
                write!(f, "cannot close undo group: none is open")
            }
            ApiError::FuelExhausted => {
                write!(f, "operation aborted: fuel limit exhausted")
            }
            ApiError::StateSizeExceeded { current, limit } => {
                write!(
                    f,
                    "state size {current} bytes exceeds limit of {limit} bytes"
                )
            }
            Self::RuntimeError(msg) => {
                write!(f, "expression evaluation failed: {msg}")
            }
        }
    }
}

impl std::error::Error for ApiError {}
