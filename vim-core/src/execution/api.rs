//! [`VimApi`] — the universal API entry point for vim-core.
//!
//! `VimApi` ties together domain views ([`BufferView`], [`CursorView`], etc.),
//! an [`EffectEmitter`], and an [`InvocationContext`] that captures caller
//! identity and capability tier.  All API operations flow through this struct.

use std::cell::Cell;

use crate::effects::Effect;
use crate::execution::api_emitter::EffectEmitter;
use crate::execution::api_views::{
    BufferView, CursorView, MarkView, OptionView, RegisterView, StateView, VariableView,
};
use crate::execution::engine::VimEngine;
use crate::execution::expr_engine::ExprContext;
use crate::execution::host_api::{VimHost, VimSession};
use crate::execution::session_host::SessionHost;
use crate::primitives::{CallerId, CapabilityTier};

// ═══════════════════════════════════════════════════════════════════════════════
// InvocationContext
// ═══════════════════════════════════════════════════════════════════════════════

/// Metadata describing who is making an API call and at what privilege level.
#[non_exhaustive]
pub struct InvocationContext {
    /// Identifies the caller (autocommand, host, extension, etc.).
    pub caller_id: CallerId,
    /// The maximum mutation tier this invocation is allowed.
    pub capability_tier: CapabilityTier,
}

impl InvocationContext {
    /// Create a new invocation context.
    #[inline]
    #[must_use]
    pub const fn new(caller_id: CallerId, capability_tier: CapabilityTier) -> Self {
        Self {
            caller_id,
            capability_tier,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VimApi
// ═══════════════════════════════════════════════════════════════════════════════

/// Universal API entry point for vim-core.
///
/// Provides zero-cost domain views over engine state and an [`EffectEmitter`]
/// for producing side-effects.  Created per-invocation with a borrowed engine,
/// host, and caller context.
pub struct VimApi<'a> {
    /// The core engine (owns state, parser, options, etc.).
    engine: &'a VimEngine,
    /// The host document and cursor provider.
    host: &'a dyn VimHost,
    /// Accumulated effects (interior mutability via `Cell`).
    effects: Cell<Vec<Effect>>,
    /// Caller identity and capability tier.
    invocation: InvocationContext,
}

impl<'a> VimApi<'a> {
    /// Create a new `VimApi` from raw engine and host references.
    #[inline]
    #[must_use]
    pub fn new(
        engine: &'a VimEngine,
        host: &'a dyn VimHost,
        invocation: InvocationContext,
    ) -> Self {
        Self {
            engine,
            host,
            effects: Cell::new(Vec::new()),
            invocation,
        }
    }

    /// Convenience constructor from a [`VimSession<SessionHost>`].
    #[inline]
    #[must_use]
    pub fn from_session(
        session: &'a VimSession<SessionHost>,
        invocation: InvocationContext,
    ) -> Self {
        Self::new(session.engine(), session.host(), invocation)
    }

    // ── Domain accessors (zero-cost views) ──────────────────────────────

    /// Read-only view over the document text.
    #[inline]
    #[must_use]
    pub fn buffer(&self) -> BufferView<'a> {
        BufferView::new(self.host, self.engine.resolved_options())
    }

    /// Read-only view over cursor and selection state.
    #[inline]
    #[must_use]
    pub fn cursor(&self) -> CursorView<'a> {
        CursorView::new(self.host)
    }

    /// Read-only view over the register file.
    #[inline]
    #[must_use]
    pub const fn registers(&self) -> RegisterView<'a> {
        RegisterView::new(self.engine.state().registers())
    }

    /// Read-only view over mark storage.
    #[inline]
    #[must_use]
    pub const fn marks(&self) -> MarkView<'a> {
        MarkView::new(self.engine.state().marks())
    }

    /// Read-only view over effective Vim options.
    #[inline]
    #[must_use]
    pub const fn options(&self) -> OptionView<'a> {
        OptionView::new(self.engine.resolved_options())
    }

    /// Read-only view over engine state (mode, search, macros, etc.).
    #[inline]
    #[must_use]
    pub const fn state(&self) -> StateView<'a> {
        StateView::new(self.engine)
    }

    /// Read-only view over the variable store.
    #[inline]
    #[must_use]
    pub const fn variables(&self) -> VariableView<'a> {
        VariableView::new(self.engine.state().variable_store())
    }

    /// Multi-buffer view, if the host supports it.
    ///
    /// Returns `None` if the host does not implement [`BufferScope`](crate::document::BufferScope)
    /// or does not declare [`HostCapability::MultiBuffer`](crate::execution::host_api::HostCapability::MultiBuffer).
    #[must_use]
    pub fn buffers(&self) -> Option<crate::execution::api_views::MultiBufferView<'a>> {
        self.host
            .buffer_scope()
            .map(crate::execution::api_views::MultiBufferView::new)
    }

    /// Read-only view of the autocmd registry.
    ///
    /// Allows querying registered autocmds without mutation. To subscribe
    /// or unsubscribe, use
    /// [`VimEngine::event_registry_mut()`](crate::execution::VimEngine::event_registry_mut)
    /// directly.
    #[inline]
    #[must_use]
    pub fn event_registry(&self) -> &crate::execution::event_registry::EventRegistry {
        self.engine.event_registry()
    }

    /// Create an expression evaluation context from this API handle.
    ///
    /// Provides read-only access to the variable store and resolved options,
    /// suitable for passing to an [`ExprEngine`](crate::execution::expr_engine::ExprEngine).
    #[inline]
    #[must_use]
    pub const fn expr_context(&self) -> ExprContext<'_> {
        ExprContext {
            variables: self.engine.state().variable_store(),
            options: self.engine.resolved_options(),
        }
    }

    /// Effect emitter for producing side-effects.
    ///
    /// The emitter borrows the shared effect accumulator and enforces the
    /// caller's capability tier on mutation operations.
    #[inline]
    #[must_use]
    pub fn emit(&self) -> EffectEmitter<'_> {
        EffectEmitter {
            effects: &self.effects,
            tier: self.invocation.capability_tier,
        }
    }

    // ── Metadata ────────────────────────────────────────────────────────

    /// The identity of the caller that created this API handle.
    #[inline]
    #[must_use]
    pub const fn caller_id(&self) -> &CallerId {
        &self.invocation.caller_id
    }

    /// The capability tier of the current invocation.
    #[inline]
    #[must_use]
    pub const fn capability_tier(&self) -> CapabilityTier {
        self.invocation.capability_tier
    }

    // ── Internal ────────────────────────────────────────────────────────

    /// Drain all accumulated effects, returning them and leaving the
    /// internal accumulator empty.
    ///
    /// Called by the engine after a handler returns to validate and apply
    /// the accumulated effects atomically.
    #[inline]
    pub fn drain_effects(&self) -> Vec<Effect> {
        self.effects.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::HostSession;
    use crate::primitives::{CallerId, CapabilityTier, Mode};

    /// Helper: build a mutating invocation context for tests.
    fn test_invocation(tier: CapabilityTier) -> InvocationContext {
        InvocationContext::new(CallerId::Internal, tier)
    }

    #[test]
    fn vim_api_domain_accessors() {
        let session = HostSession::new("hello\nworld");
        let api = VimApi::from_session(&session, test_invocation(CapabilityTier::Mutating));

        assert_eq!(api.buffer().text(), "hello\nworld");
        assert_eq!(api.cursor().offset(), 0);
        assert_eq!(api.state().mode(), Mode::Normal);
    }

    #[test]
    fn vim_api_emit_and_drain() {
        let session = HostSession::new("hello\nworld");
        let api = VimApi::from_session(&session, test_invocation(CapabilityTier::Mutating));

        api.emit().set_cursor(3).unwrap();
        api.emit().insert(0, "x").unwrap();

        let effects = api.drain_effects();
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn vim_api_readonly_blocks_mutation() {
        let session = HostSession::new("hello\nworld");
        let api = VimApi::from_session(&session, test_invocation(CapabilityTier::ReadOnly));

        // Mutation should be rejected.
        let err = api.emit().insert(0, "x");
        assert!(err.is_err());

        // Read-tier operations should succeed.
        api.emit().set_cursor(3).unwrap();
        api.emit().message("info").unwrap();

        // Buffer/cursor reads always work.
        assert_eq!(api.buffer().text(), "hello\nworld");
        assert_eq!(api.cursor().offset(), 0);

        let effects = api.drain_effects();
        assert_eq!(effects.len(), 2);
    }
}
