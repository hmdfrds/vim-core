//! Type alias for the canonical host-simulation session.
//!
//! `HostSession` is a type alias for `VimSession<SessionHost>`. All 87 public
//! methods are available via inherent methods on `impl VimSession<SessionHost>`
//! in `session_host.rs`.

use super::host_api::VimSession;
use super::session_host::SessionHost;

pub use crate::execution::host::HostRequestId;

// ─────────────────────────────────────────────────────────────────────────────
// Type alias
// ─────────────────────────────────────────────────────────────────────────────

/// Canonical host-simulation session.
///
/// Owns a `VimEngine`, a `SessionDocument`, and all supporting state (undo,
/// dirty tracking, cursor, selection, viewport). Exposes a simple
/// `process_key_host() → HostResponse` API that handles the full lifecycle:
///
/// 1. Build validated `InputContext` from current state
/// 2. Feed key to engine
/// 3. Apply host-owned effects (text mutations, cursor, selection, undo)
/// 4. Drain pending mapping/macro keys (interleaved loop)
/// 5. Build rich `HostResponse` from engine state + host fields
pub type HostSession = VimSession<SessionHost>;

// ─────────────────────────────────────────────────────────────────────────────
// Error types
// ─────────────────────────────────────────────────────────────────────────────

/// Error returned by [`HostSession::complete_request_checked`] when the
/// request ID is not found in the pending set (already completed or invalid).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownRequestError {
    /// The request ID that was not found.
    pub id: HostRequestId,
}

impl std::fmt::Display for UnknownRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown host request id {} (already completed or invalid)",
            self.id.get()
        )
    }
}

impl std::error::Error for UnknownRequestError {}

#[cfg(test)]
#[path = "host_session_tests.rs"]
mod tests;
