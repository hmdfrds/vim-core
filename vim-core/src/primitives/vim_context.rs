//! Lightweight semantic state snapshot for host pre-processing queries.
//!
//! [`VimContext`] captures the engine's current semantic state in a cheap,
//! `Clone + Send + Sync` struct. Hosts query this before key processing to
//! make routing decisions (e.g., whether to pass a key to the engine or
//! handle it natively).

use compact_str::CompactString;

use super::mode::Mode;
use super::operator::Operator;
use super::register_type::RegisterName;

/// Cheap snapshot of the engine's semantic state.
///
/// All fields are already tracked by the engine — constructing this is O(1).
/// Designed for host-side pre-processing queries: "what is the engine doing
/// right now?" without requiring mutable access or key processing.
///
/// # Thread Safety
///
/// `VimContext` is `Send + Sync` by construction (all fields are owned values,
/// no references or interior mutability).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VimContext {
    /// Current editing mode.
    pub mode: Mode,
    /// Pending operator (e.g., `d` waiting for a motion), or `None`.
    pub pending_operator: Option<Operator>,
    /// Whether a macro is currently being recorded (`q{reg}` active).
    pub is_recording: bool,
    /// Whether the engine is executing a dot-repeat replay.
    pub is_repeating: bool,
    /// Whether the typeahead buffer or macro stack has pending keys.
    pub has_pending_keys: bool,
    /// The pending command display string (showcmd) for partial commands.
    pub pending_display: CompactString,
    /// The register selected by `"{reg}` prefix, if any.
    pub active_register: Option<RegisterName>,
}
