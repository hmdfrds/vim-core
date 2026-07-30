//! `HookBus` — runtime event dispatch for pre/post-command hooks.
//!
//! Provides a lightweight publish/subscribe system for engine lifecycle events.
//! Handlers are registered with a [`HookPoint`] discriminant and are invoked
//! whenever the engine fires that point. During speculative execution
//! (`fork_active = true`), hook firing is suppressed — handlers only observe
//! committed engine mutations.
//!
//! # Design
//!
//! - **Zero-cost when empty**: The `fire()` method inlines an `is_empty()` check
//!   that short-circuits before any iteration, so engines without hooks pay only
//!   a branch-predicted comparison per keystroke.
//! - **Cancellation**: A handler may return [`HookAction::Cancel`] to signal that
//!   subsequent handlers should be skipped. The first `Cancel` wins.
//! - **Ordering**: Handlers fire in registration order (FIFO).
//! - **Thread safety**: [`HookHandler`] requires `Send + Sync` to support future
//!   async host bridge wrapping.

use crate::effects::Effect;
use crate::primitives::Mode;
use crate::state::VimState;

// ═══════════════════════════════════════════════════════════════════════════════
// HookPoint
// ═══════════════════════════════════════════════════════════════════════════════

/// Lifecycle point at which a hook fires.
///
/// Each variant represents a distinct engine event. Handlers registered for
/// one point are only invoked when that point fires — there is no wildcard.
///
/// # Non-Exhaustive
///
/// New hook points may be added in future versions. External crates matching
/// on `HookPoint` must include a wildcard arm.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPoint {
    /// Fires before a command is executed.
    ///
    /// Context contains the pre-command engine state.
    PreCommand,
    /// Fires after a command has been executed and effects computed.
    ///
    /// Context contains the post-command engine state and the produced effects.
    PostCommand,
    /// Fires when the cursor position changes.
    CursorMoved,
    /// Fires when the editor mode changes (Normal → Insert, etc.).
    ModeChanged,
    /// Fires before a buffer write is initiated.
    BufWritePre,
    /// Fires when entering a buffer.
    BufEnter,
    /// Fires when leaving a buffer.
    BufLeave,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HookAction
// ═══════════════════════════════════════════════════════════════════════════════

/// Action returned by a hook handler to control dispatch flow.
///
/// # Non-Exhaustive
///
/// Future versions may add actions like `Retry` or `Redirect`. External
/// crates matching on `HookAction` must include a wildcard arm.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAction {
    /// Continue to the next handler (or return `Continue` if last).
    Continue,
    /// Cancel further handler dispatch for this hook point.
    ///
    /// The first handler returning `Cancel` stops iteration. For informational
    /// hook points (like `PostCommand`), `Cancel` still terminates handler
    /// iteration but does not undo the command — the effects have already
    /// been computed.
    Cancel,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HookContext
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only context passed to hook handlers.
///
/// Contains the hook point, current mode, produced effects, and a reference
/// to the full engine state. Handlers use this to inspect engine state
/// without mutation.
pub struct HookContext<'a> {
    /// Which hook point triggered this invocation.
    pub point: HookPoint,
    /// The engine's current mode at the time of firing.
    pub mode: Mode,
    /// Effects produced by the command (empty for `PreCommand`).
    pub effects: &'a [Effect],
    /// Read-only reference to the full engine state.
    pub state: &'a VimState,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HookHandler
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait for hook handler implementations.
///
/// Implementors receive a read-only [`HookContext`] and return a [`HookAction`]
/// to control dispatch flow. Handlers must be `Send + Sync` to support future
/// async host bridge wrapping.
///
/// # Closure Implementation
///
/// A blanket implementation is provided for closures:
///
/// ```ignore
/// use vim_core::execution::{HookAction, HookContext};
///
/// let handler = |ctx: &HookContext<'_>| -> HookAction {
///     // inspect ctx.effects, ctx.mode, etc.
///     HookAction::Continue
/// };
/// ```
pub trait HookHandler: Send + Sync {
    /// Handle a hook invocation and return an action.
    fn handle(&self, ctx: &HookContext<'_>) -> HookAction;
}

/// Blanket implementation for closures and function pointers.
impl<F> HookHandler for F
where
    F: Fn(&HookContext<'_>) -> HookAction + Send + Sync,
{
    fn handle(&self, ctx: &HookContext<'_>) -> HookAction {
        self(ctx)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HookId
// ═══════════════════════════════════════════════════════════════════════════════

/// Opaque identifier for a registered hook handler.
///
/// Returned by `HookBus::add()` and used by `HookBus::remove()` to
/// unregister a specific handler. IDs are unique within a single `HookBus`
/// instance and are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HookId(u64);

// ═══════════════════════════════════════════════════════════════════════════════
// HookBus
// ═══════════════════════════════════════════════════════════════════════════════

/// Event dispatch bus for engine lifecycle hooks.
///
/// Maintains an ordered list of `(HookPoint, HookId, Box<dyn HookHandler>)`
/// entries. When fired, only handlers matching the given [`HookPoint`] are
/// invoked, in registration order.
///
/// # Performance
///
/// The `fire()` method is `#[inline]` and checks `handlers.is_empty()` first.
/// When no hooks are registered (the common case), this compiles down to a
/// single branch-predicted comparison — zero overhead on the hot path. The
/// slow path (`fire_slow`) is marked `#[cold]` for branch prediction hints.
pub(crate) struct HookBus {
    /// Registered handlers: `(point, id, handler)`.
    handlers: Vec<(HookPoint, HookId, Box<dyn HookHandler>)>,
    /// Monotonic counter for generating unique [`HookId`]s.
    next_id: u64,
}

impl HookBus {
    /// Create an empty hook bus with no registered handlers.
    pub(crate) fn new() -> Self {
        Self {
            handlers: Vec::new(),
            next_id: 0,
        }
    }

    /// Register a handler for the given hook point.
    ///
    /// Returns a [`HookId`] that can be used to remove the handler later.
    /// Handlers are invoked in registration order (FIFO).
    pub(crate) fn add(&mut self, point: HookPoint, handler: Box<dyn HookHandler>) -> HookId {
        let id = HookId(self.next_id);
        self.next_id += 1;
        self.handlers.push((point, id, handler));
        id
    }

    /// Remove a previously registered handler by its [`HookId`].
    ///
    /// Returns `true` if the handler was found and removed, `false` if the
    /// ID was not present (already removed or never registered).
    pub(crate) fn remove(&mut self, id: HookId) -> bool {
        let len_before = self.handlers.len();
        self.handlers.retain(|(_, handler_id, _)| *handler_id != id);
        self.handlers.len() < len_before
    }

    /// Fire all handlers registered for the given hook point.
    ///
    /// Returns [`HookAction::Continue`] if no handlers are registered or all
    /// handlers returned `Continue`. Returns [`HookAction::Cancel`] as soon as
    /// any handler returns `Cancel` (short-circuit).
    ///
    /// # Fast Path
    ///
    /// When no handlers are registered (`handlers.is_empty()`), returns
    /// `Continue` immediately without any iteration. This is the expected
    /// case for most engine instances.
    #[inline]
    pub(crate) fn fire(&self, point: HookPoint, ctx: &HookContext<'_>) -> HookAction {
        if self.handlers.is_empty() {
            return HookAction::Continue;
        }
        self.fire_slow(point, ctx)
    }

    /// Slow path for `fire()` — iterates matching handlers.
    ///
    /// Separated from the inline `fire()` for branch prediction: the compiler
    /// marks this as cold, keeping the fast path (empty check) tight.
    #[cold]
    fn fire_slow(&self, point: HookPoint, ctx: &HookContext<'_>) -> HookAction {
        for (handler_point, _id, handler) in &self.handlers {
            if *handler_point == point && handler.handle(ctx) == HookAction::Cancel {
                return HookAction::Cancel;
            }
        }
        HookAction::Continue
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Build a minimal `HookContext` for testing.
    fn test_ctx(point: HookPoint) -> HookContext<'static> {
        // Leak a VimState for the 'static lifetime — acceptable in tests.
        let state: &'static VimState = Box::leak(Box::new(VimState::default()));
        HookContext {
            point,
            mode: Mode::Normal,
            effects: &[],
            state,
        }
    }

    // ── 1. Empty bus returns Continue ──────────────────────────────────

    #[test]
    fn empty_bus_returns_continue() {
        let bus = HookBus::new();
        let ctx = test_ctx(HookPoint::PostCommand);
        assert_eq!(bus.fire(HookPoint::PostCommand, &ctx), HookAction::Continue);
    }

    // ── 2. Single handler fires ────────────────────────────────────────

    #[test]
    fn single_handler_fires_and_returns_cancel() {
        let mut bus = HookBus::new();
        bus.add(
            HookPoint::PostCommand,
            Box::new(|_: &HookContext<'_>| HookAction::Cancel),
        );
        let ctx = test_ctx(HookPoint::PostCommand);
        assert_eq!(bus.fire(HookPoint::PostCommand, &ctx), HookAction::Cancel);
    }

    // ── 3. Matching filter: handler only fires for its registered point ─

    #[test]
    fn handler_only_fires_for_matching_point() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        let mut bus = HookBus::new();
        bus.add(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // Fire CursorMoved — PostCommand handler should NOT fire.
        let ctx = test_ctx(HookPoint::CursorMoved);
        let action = bus.fire(HookPoint::CursorMoved, &ctx);
        assert_eq!(action, HookAction::Continue);
        assert!(
            !called.load(Ordering::SeqCst),
            "handler registered for PostCommand must not fire for CursorMoved"
        );
    }

    // ── 4. Multiple handlers: first Cancel wins ────────────────────────

    #[test]
    fn first_cancel_wins() {
        let second_called = Arc::new(AtomicBool::new(false));
        let second_called_clone = Arc::clone(&second_called);

        let mut bus = HookBus::new();
        // First handler: Cancel
        bus.add(
            HookPoint::PostCommand,
            Box::new(|_: &HookContext<'_>| HookAction::Cancel),
        );
        // Second handler: should never be reached
        bus.add(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                second_called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        let ctx = test_ctx(HookPoint::PostCommand);
        let action = bus.fire(HookPoint::PostCommand, &ctx);
        assert_eq!(action, HookAction::Cancel);
        assert!(
            !second_called.load(Ordering::SeqCst),
            "second handler must not fire after first returns Cancel"
        );
    }

    // ── 5. Remove handler: after remove, handler no longer fires ───────

    #[test]
    fn remove_handler_stops_firing() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut bus = HookBus::new();
        let id = bus.add(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Cancel
            }),
        );

        // Remove should return true (found).
        assert!(bus.remove(id), "remove should return true for existing ID");

        // Fire — handler should not run.
        let ctx = test_ctx(HookPoint::PostCommand);
        let action = bus.fire(HookPoint::PostCommand, &ctx);
        assert_eq!(
            action,
            HookAction::Continue,
            "bus should return Continue after handler removed"
        );
        assert!(
            !called.load(Ordering::SeqCst),
            "handler must not fire after removal"
        );
    }

    // ── 5b. Remove returns false for unknown ID ────────────────────────

    #[test]
    fn remove_returns_false_for_unknown_id() {
        let mut bus = HookBus::new();
        assert!(
            !bus.remove(HookId(999)),
            "remove should return false for non-existent ID"
        );
    }

    // ── 6. HookId uniqueness ───────────────────────────────────────────

    #[test]
    fn hook_id_uniqueness() {
        let mut bus = HookBus::new();
        let id1 = bus.add(
            HookPoint::PostCommand,
            Box::new(|_: &HookContext<'_>| HookAction::Continue),
        );
        let id2 = bus.add(
            HookPoint::PostCommand,
            Box::new(|_: &HookContext<'_>| HookAction::Continue),
        );
        assert_ne!(
            id1, id2,
            "consecutive add() calls must return different IDs"
        );
    }

    // ── 7. Closure handler ─────────────────────────────────────────────

    #[test]
    fn closure_handler_fires() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut bus = HookBus::new();
        let closure = move |_: &HookContext<'_>| -> HookAction {
            called_clone.store(true, Ordering::SeqCst);
            HookAction::Continue
        };
        bus.add(HookPoint::PostCommand, Box::new(closure));

        let ctx = test_ctx(HookPoint::PostCommand);
        bus.fire(HookPoint::PostCommand, &ctx);
        assert!(
            called.load(Ordering::SeqCst),
            "closure handler must be called on fire"
        );
    }

    // ── 8. Handler with captured state (Arc<AtomicBool>) ───────────────

    #[test]
    fn handler_with_captured_state() {
        let flag = Arc::new(AtomicBool::new(false));

        struct StatefulHandler {
            flag: Arc<AtomicBool>,
        }
        impl HookHandler for StatefulHandler {
            fn handle(&self, _ctx: &HookContext<'_>) -> HookAction {
                self.flag.store(true, Ordering::SeqCst);
                HookAction::Continue
            }
        }

        let mut bus = HookBus::new();
        bus.add(
            HookPoint::PostCommand,
            Box::new(StatefulHandler {
                flag: Arc::clone(&flag),
            }),
        );

        let ctx = test_ctx(HookPoint::PostCommand);
        bus.fire(HookPoint::PostCommand, &ctx);
        assert!(
            flag.load(Ordering::SeqCst),
            "stateful handler must set flag when fired"
        );
    }

    // ── 9. Multiple handlers for different points ──────────────────────

    #[test]
    fn multiple_points_fire_independently() {
        let pre_called = Arc::new(AtomicBool::new(false));
        let post_called = Arc::new(AtomicBool::new(false));

        let pre_clone = Arc::clone(&pre_called);
        let post_clone = Arc::clone(&post_called);

        let mut bus = HookBus::new();
        bus.add(
            HookPoint::PreCommand,
            Box::new(move |_: &HookContext<'_>| {
                pre_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );
        bus.add(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                post_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // Fire only PostCommand.
        let ctx = test_ctx(HookPoint::PostCommand);
        bus.fire(HookPoint::PostCommand, &ctx);

        assert!(
            !pre_called.load(Ordering::SeqCst),
            "PreCommand handler must not fire when PostCommand is fired"
        );
        assert!(
            post_called.load(Ordering::SeqCst),
            "PostCommand handler must fire"
        );
    }
}
