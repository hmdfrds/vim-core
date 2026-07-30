//! Response type for the execution layer.
//!
//! The `Response` struct is what `VimEngine::process()` returns to the shell.
//! It carries the effects to apply, host requests to execute, and metadata
//! about whether the key was consumed or is pending more input.
//!
//! # Layering
//!
//! Execution is the top layer: it may import any internal module, and no
//! internal module below it may import execution.

use crate::effects::undo_state::EffectState;
use crate::effects::{Effect, EffectProvenance, Effects};
use crate::execution::host::HostRequest;
use compact_str::CompactString;
use smallvec::SmallVec;

/// How the engine handled a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ResponseKind {
    /// Key was consumed and fully processed.
    Consumed,
    /// Key was consumed; engine needs more keys before it can act.
    Pending,
    /// Key was not consumed (pass through to host).
    #[default]
    Ignored,
    /// The operation was cancelled by the external cancel flag.
    Cancelled,
}

/// Response from processing a key.
///
/// Response uses `SmallVec<[Effect; 4]>` to match the `Effects` builder's
/// inline capacity, enabling zero-cost moves from `Effects` to `Response`
/// without heap allocation.  Most normal-mode commands produce 3-4 effects
/// (cursor + mode + undo markers) which stay fully inline.
///
/// Fields are `pub(crate)` for efficient internal mutation by the engine,
/// with public accessor methods for shell consumers.
#[derive(Debug, Default)]
#[must_use = "Response contains effects that must be applied to the editor"]
pub struct Response {
    /// Effects to apply to the shell.
    pub(crate) effects: SmallVec<[Effect; 4]>,
    /// Host requests that require shell-side I/O.
    pub(crate) host_requests: Vec<HostRequest>,
    /// How the engine handled the key.
    pub(crate) kind: ResponseKind,
    /// Optional status message.
    pub(crate) message: Option<CompactString>,
    /// Provenance metadata: which command produced these effects and when.
    ///
    /// `None` for pending, ignored, and cancelled responses (no command executed).
    /// `Some(...)` when a command was fully resolved and executed.
    pub(crate) provenance: Option<EffectProvenance>,
    /// `true` when a `PreCommand` hook handler returned `Cancel`, preventing
    /// command execution. The engine uses this to suppress `PostCommand` hook
    /// firing — there is no "post" to a cancelled command.
    pub(crate) precommand_cancelled: bool,
    /// Federation state events extracted from this response's effects.
    ///
    /// Non-empty when the engine has processed at least one effect that represents
    /// a federable state change (register write, global mark set, search pattern
    /// update, or mode change).
    pub(crate) state_events: SmallVec<[crate::state::federation::StateEvent; 2]>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API (for shell consumers)
// ─────────────────────────────────────────────────────────────────────────────

impl Response {
    /// Effects to apply to the editor.
    #[inline]
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// Take ownership of the effects, converting to `Vec<Effect>`.
    ///
    /// Returns a `Vec` for backward compatibility with shell code that
    /// stores effects in a `Vec`. The conversion is only paid here at the
    /// API boundary, not during internal engine processing.
    #[inline]
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects).into_vec()
    }

    /// Host requests that require shell-side I/O.
    #[inline]
    #[must_use]
    pub fn host_requests(&self) -> &[HostRequest] {
        &self.host_requests
    }

    /// Take ownership of the host requests vec.
    #[inline]
    pub fn take_host_requests(&mut self) -> Vec<HostRequest> {
        std::mem::take(&mut self.host_requests)
    }

    /// How the engine handled the key.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> ResponseKind {
        self.kind
    }

    /// Whether the key was consumed by the vim engine.
    #[inline]
    #[must_use]
    pub fn consumed(&self) -> bool {
        self.kind != ResponseKind::Ignored
    }

    /// Whether the engine is waiting for more keys.
    #[inline]
    #[must_use]
    pub fn pending(&self) -> bool {
        self.kind == ResponseKind::Pending
    }

    /// Returns `true` if this response represents a cancelled operation.
    #[inline]
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.kind == ResponseKind::Cancelled
    }

    /// Returns `true` if a `PreCommand` hook handler cancelled command execution.
    ///
    /// When `true`, no command was executed and the effects list is empty.
    /// `PostCommand` hooks are suppressed because there is no "post" to a
    /// cancelled command.
    #[inline]
    #[must_use]
    pub const fn precommand_cancelled(&self) -> bool {
        self.precommand_cancelled
    }

    /// Optional status message.
    #[inline]
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Provenance metadata: which command produced these effects and when.
    ///
    /// Returns `None` for pending, ignored, and cancelled responses (no
    /// command was executed). Returns `Some(...)` when a command was fully
    /// resolved and executed, enabling callers to trace "this Replace
    /// effect came from `dw` at keystroke #7."
    #[inline]
    #[must_use]
    pub const fn provenance(&self) -> Option<&EffectProvenance> {
        self.provenance.as_ref()
    }

    /// Federation state events produced by this response.
    ///
    /// Returns the slice of [`StateEvent`](crate::state::federation::StateEvent)s extracted
    /// from the response's effects.
    /// Non-empty when at least one federable state change occurred (register write,
    /// global mark, search pattern, or mode transition).
    ///
    /// Callers should forward these events to any connected federation backend
    /// or peer instances after applying the response's effects.
    #[inline]
    #[must_use]
    pub fn state_events(&self) -> &[crate::state::federation::StateEvent] {
        &self.state_events
    }

    /// Take ownership of the federation state events, leaving this response empty.
    #[inline]
    pub fn take_state_events(&mut self) -> SmallVec<[crate::state::federation::StateEvent; 2]> {
        std::mem::take(&mut self.state_events)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Constructors and builders
// ─────────────────────────────────────────────────────────────────────────────

impl Response {
    /// Create empty response (key not consumed).
    #[inline]
    pub fn ignored() -> Self {
        Self {
            effects: SmallVec::new(),
            host_requests: Vec::new(),
            kind: ResponseKind::Ignored,
            message: None,
            provenance: None,
            precommand_cancelled: false,
            state_events: SmallVec::new(),
        }
    }

    /// Create empty consumed response (key processed but produced no effects).
    ///
    /// Used when the engine processes a key and determines it is invalid
    /// (e.g., an unrecognized motion after an operator). The parser is
    /// reset, but the key must not be forwarded to the host editor.
    #[inline]
    pub fn consumed_empty() -> Self {
        Self {
            effects: SmallVec::new(),
            host_requests: Vec::new(),
            kind: ResponseKind::Consumed,
            message: None,
            provenance: None,
            precommand_cancelled: false,
            state_events: SmallVec::new(),
        }
    }

    /// Create pending response (waiting for more keys).
    #[inline]
    pub fn pending_response() -> Self {
        Self {
            effects: SmallVec::new(),
            host_requests: Vec::new(),
            kind: ResponseKind::Pending,
            message: None,
            provenance: None,
            precommand_cancelled: false,
            state_events: SmallVec::new(),
        }
    }

    /// Create response with effects.
    ///
    /// Accepts `Effects<S>` for any undo state (dispatch-layer builder backed
    /// by `SmallVec<[Effect; 4]>`) and moves the inner SmallVec directly into
    /// the response — zero-cost, no heap allocation.
    #[inline]
    pub fn with_effects<S: EffectState>(effects: Effects<S>) -> Self {
        Self {
            effects: effects.into_inner(),
            host_requests: Vec::new(),
            kind: ResponseKind::Consumed,
            message: None,
            provenance: None,
            precommand_cancelled: false,
            state_events: SmallVec::new(),
        }
    }

    /// Create response with host requests.
    #[inline]
    pub fn with_host_requests(host_requests: SmallVec<[HostRequest; 4]>) -> Self {
        Self {
            effects: SmallVec::new(),
            host_requests: host_requests.into_vec(),
            kind: ResponseKind::Consumed,
            message: None,
            provenance: None,
            precommand_cancelled: false,
            state_events: SmallVec::new(),
        }
    }

    /// Add message to response.
    #[inline]
    pub fn with_message(mut self, msg: impl Into<CompactString>) -> Self {
        self.message = Some(msg.into());
        self
    }

    /// Append effects from the dispatch/commands layer.
    ///
    /// Accepts `Effects<S>` for any undo state and extends the response's
    /// effect Vec. Used when the engine needs to append additional effects
    /// after initial execution (e.g., dot-repeat injection, one-shot mode
    /// restart).
    #[inline]
    pub fn extend_effects<S: EffectState>(&mut self, effects: Effects<S>) {
        self.effects.extend(effects.into_inner());
    }
}
