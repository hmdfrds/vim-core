//! `ScopedFork` — RAII guard for speculative engine execution.
//!
//! Enables clone-and-restore of mutable engine state: fork the engine,
//! run speculative key processing, then either commit (keep the mutations)
//! or drop (roll back to the snapshot).
//!
//! # Motivation
//!
//! The `VimEngine` is a synchronous state machine: `process(key, ctx) -> Response`.
//! Shadow execution (macro replay) permanently mutates engine state, which prevents
//! speculative "what-if" queries. `ScopedFork` solves this by snapshotting the
//! mutable fields before speculation begins and restoring them on drop unless
//! explicitly committed.
//!
//! # Usage
//!
//! ```ignore
//! let mut fork = engine.fork()?;
//! let response = fork.process(key, ctx);
//! // inspect response...
//! if looks_good {
//!     fork.commit(); // keep the mutations
//! }
//! // otherwise: fork is dropped, state is rolled back
//! ```
//!
//! # Snapshotted Fields
//!
//! Only mutable state that diverges during speculation is cloned:
//! - `state` (VimState)
//! - `parser` (Parser)
//! - `typeahead` (TypeaheadCoordinator)
//! - `recording` (RecordingState)
//! - `is_repeating`
//! - `command_line_session`
//! - `sticky_session`
//! - `sticky_prefixes`
//! - `cmd_buffer`
//! - `keystroke_seq`
//! - `host.sequencer` (HostRequestSequencer — Copy)
//!
//! Config, keymap, providers, host pending-request map, and cold state are NOT
//! cloned — they are either read-only during processing or gated behind
//! `fork_active` suppression.

use super::coordinators::{RecordingState, TypeaheadCoordinator};
use super::{CommandLineSession, StickySession, VimEngine};
use crate::document::Document;
use crate::execution::host::HostRequestSequencer;
use crate::execution::response::Response;
use crate::execution::{InputContext, Validated};
use crate::grammar::Parser;
use crate::keymap::KeyEvent;
use crate::state::VimState;

// ═══════════════════════════════════════════════════════════════════════════════
// ForkError
// ═══════════════════════════════════════════════════════════════════════════════

/// Errors that prevent forking.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ForkError {
    /// A `ScopedFork` is already active on this engine.
    AlreadyForked,
}

impl std::fmt::Display for ForkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyForked => {
                write!(f, "a ScopedFork is already active on this engine")
            }
        }
    }
}

impl std::error::Error for ForkError {}

// ═══════════════════════════════════════════════════════════════════════════════
// EngineSnapshot
// ═══════════════════════════════════════════════════════════════════════════════

/// Snapshot of engine state that can be restored on drop.
///
/// Contains clones of all mutable fields that diverge during speculative
/// execution. Config, keymap, providers, the host pending-request map, and
/// cold state are excluded — they are either read-only or gated by
/// `fork_active`. The host sequencer counter IS included because speculative
/// `process()` calls can advance it when emitting `HostRequest` values.
pub(crate) struct EngineSnapshot {
    state: VimState,
    parser: Parser,
    typeahead: TypeaheadCoordinator,
    recording: RecordingState,
    is_repeating: bool,
    command_line_session: Option<CommandLineSession>,
    sticky_session: Option<StickySession>,
    sticky_prefixes: u8,
    cmd_buffer: Option<String>,
    keystroke_seq: u64,
    sequencer: HostRequestSequencer,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ScopedFork
// ═══════════════════════════════════════════════════════════════════════════════

/// RAII guard for speculative engine execution.
///
/// Created by [`VimEngine::fork()`]. On drop, restores the engine to its
/// pre-fork state unless [`commit()`](ScopedFork::commit) was called.
///
/// # Borrow Exclusivity
///
/// `ScopedFork` holds `&mut VimEngine`, which prevents:
/// - Creating a second fork (the borrow checker enforces single-fork)
/// - Any other `&mut` access to the engine while the fork is alive
///
/// The `fork_active` flag is a runtime guard for cases where the borrow
/// checker alone is insufficient (e.g., re-entrant calls from within
/// engine methods).
#[must_use = "fork is rolled back on drop -- call .commit() to keep changes"]
pub struct ScopedFork<'engine> {
    engine: &'engine mut VimEngine,
    /// `Some` while the fork is alive; taken by `commit()` to discard
    /// the snapshot, or taken by `drop()` to restore from it.
    snapshot: Option<EngineSnapshot>,
    committed: bool,
}

impl std::fmt::Debug for ScopedFork<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedFork")
            .field("committed", &self.committed)
            .field("has_snapshot", &self.snapshot.is_some())
            .finish_non_exhaustive()
    }
}

impl ScopedFork<'_> {
    /// Mutable reference to the underlying engine.
    ///
    /// Use this for direct engine manipulation beyond what `process()` provides.
    #[inline]
    pub const fn engine_mut(&mut self) -> &mut VimEngine {
        self.engine
    }

    /// Immutable reference to the underlying engine.
    ///
    /// Use this to inspect engine state during speculation.
    #[inline]
    #[must_use]
    pub const fn engine_ref(&self) -> &VimEngine {
        self.engine
    }

    /// Process a keystroke through the forked engine.
    ///
    /// Delegates to [`VimEngine::process()`]. All mutations are speculative
    /// and will be rolled back on drop unless [`commit()`](Self::commit) is called.
    #[inline]
    pub fn process<D: Document>(
        &mut self,
        key: KeyEvent,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        self.engine.process(key, ctx)
    }

    /// Commit the speculative mutations, keeping all state changes.
    ///
    /// After this call, the engine retains whatever state was produced by
    /// the speculative key processing. The snapshot is discarded.
    ///
    /// Consumes the fork guard. The `fork_active` flag is cleared.
    pub fn commit(mut self) {
        self.committed = true;
        // Discard the snapshot — no restore needed.
        self.snapshot = None;
        self.engine.fork_active = false;
    }
}

impl Drop for ScopedFork<'_> {
    fn drop(&mut self) {
        if !self.committed {
            // Take ownership of the snapshot, moving it out of the Option.
            // This is always `Some` when `committed` is false.
            if let Some(snapshot) = self.snapshot.take() {
                self.engine.state = snapshot.state;
                self.engine.parser = snapshot.parser;
                self.engine.typeahead = snapshot.typeahead;
                self.engine.recording = snapshot.recording;
                self.engine.is_repeating = snapshot.is_repeating;
                self.engine.command_line_session = snapshot.command_line_session;
                self.engine.sticky_session = snapshot.sticky_session;
                self.engine.sticky_prefixes = snapshot.sticky_prefixes;
                self.engine.cmd_buffer = snapshot.cmd_buffer;
                self.engine.keystroke_seq = snapshot.keystroke_seq;
                self.engine.host.sequencer = snapshot.sequencer;
            }
            self.engine.fork_active = false;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VimEngine::fork()
// ═══════════════════════════════════════════════════════════════════════════════

impl VimEngine {
    /// Create a `ScopedFork` for speculative execution.
    ///
    /// Snapshots mutable engine state and sets `fork_active = true`.
    /// Returns `Err` if a fork is already active.
    ///
    /// # Errors
    ///
    /// - [`ForkError::AlreadyForked`] — a `ScopedFork` is already active.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut fork = engine.fork()?;
    /// let response = fork.process(key, ctx);
    /// fork.commit(); // or just drop to roll back
    /// ```
    pub fn fork(&mut self) -> Result<ScopedFork<'_>, ForkError> {
        if self.fork_active {
            return Err(ForkError::AlreadyForked);
        }

        let snapshot = EngineSnapshot {
            state: self.state.clone(),
            parser: self.parser.clone(),
            typeahead: self.typeahead.clone(),
            recording: self.recording.clone(),
            is_repeating: self.is_repeating,
            command_line_session: self.command_line_session,
            sticky_session: self.sticky_session,
            sticky_prefixes: self.sticky_prefixes,
            cmd_buffer: self.cmd_buffer.clone(),
            keystroke_seq: self.keystroke_seq,
            sequencer: self.host.sequencer,
        };

        self.fork_active = true;

        Ok(ScopedFork {
            engine: self,
            snapshot: Some(snapshot),
            committed: false,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::VimEngine;
    use crate::keymap::KeyEvent;
    use crate::primitives::Mode;

    /// Create a minimal `InputContext` suitable for processing a single key.
    ///
    /// Leaks a static document — acceptable in unit tests.
    fn make_ctx() -> InputContext<
        'static,
        crate::test_utils::SimpleDocument,
        crate::execution::context::Validated,
    > {
        let doc: &'static crate::test_utils::SimpleDocument = Box::leak(Box::new(
            crate::test_utils::SimpleDocument::new("hello world\nsecond line\nthird line\n"),
        ));
        InputContext::new(doc, 0).validate_clamped()
    }

    // ── Fork and drop restores state ────────────────────────────────────

    #[test]
    fn fork_drop_restores_mode() {
        let mut engine = VimEngine::new();
        assert_eq!(engine.mode(), Mode::Normal);

        {
            let mut fork = engine.fork().expect("fork should succeed");
            // Enter insert mode via 'i'
            let ctx = make_ctx();
            fork.process(KeyEvent::char('i'), ctx);
            assert!(
                fork.engine_ref().mode().is_insert(),
                "mode should be Insert inside fork"
            );
            // fork dropped here — state rolled back
        }

        assert_eq!(
            engine.mode(),
            Mode::Normal,
            "mode must be Normal after fork drop (rollback)"
        );
    }

    #[test]
    fn fork_drop_restores_parser_state() {
        let mut engine = VimEngine::new();

        {
            let mut fork = engine.fork().expect("fork should succeed");
            // Push 'd' to enter operator-pending state
            let ctx = make_ctx();
            fork.process(KeyEvent::char('d'), ctx);
            assert!(
                fork.engine_ref().pending_operator().is_some(),
                "parser should be in operator-pending state after 'd'"
            );
            // fork dropped here — parser state rolled back
        }

        assert!(
            engine.pending_operator().is_none(),
            "parser must have no pending operator after fork drop"
        );
    }

    // ── Fork and commit keeps state ─────────────────────────────────────

    #[test]
    fn fork_commit_keeps_mode() {
        let mut engine = VimEngine::new();
        assert_eq!(engine.mode(), Mode::Normal);

        {
            let mut fork = engine.fork().expect("fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('i'), ctx);
            assert!(
                fork.engine_ref().mode().is_insert(),
                "mode should be Insert inside fork"
            );
            fork.commit();
        }

        assert!(
            engine.mode().is_insert(),
            "mode must remain Insert after fork commit"
        );
    }

    #[test]
    fn fork_commit_keeps_parser_state() {
        let mut engine = VimEngine::new();

        {
            let mut fork = engine.fork().expect("fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('d'), ctx);
            assert!(
                fork.engine_ref().pending_operator().is_some(),
                "parser should be in operator-pending after 'd'"
            );
            fork.commit();
        }

        assert!(
            engine.pending_operator().is_some(),
            "parser must keep operator-pending state after fork commit"
        );
    }

    // ── keystroke_seq restored on drop ──────────────────────────────────

    #[test]
    fn keystroke_seq_restored_on_drop() {
        let mut engine = VimEngine::new();
        let initial_seq = engine.keystroke_seq;

        {
            let mut fork = engine.fork().expect("fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('j'), ctx);
            assert!(
                fork.engine_ref().keystroke_seq > initial_seq,
                "keystroke_seq should advance after processing a key"
            );
            // fork dropped — seq rolled back
        }

        assert_eq!(
            engine.keystroke_seq, initial_seq,
            "keystroke_seq must be restored after fork drop"
        );
    }

    // ── keystroke_seq kept on commit ────────────────────────────────────

    #[test]
    fn keystroke_seq_kept_on_commit() {
        let mut engine = VimEngine::new();
        let initial_seq = engine.keystroke_seq;

        {
            let mut fork = engine.fork().expect("fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('j'), ctx);
            fork.commit();
        }

        assert!(
            engine.keystroke_seq > initial_seq,
            "keystroke_seq must be advanced after fork commit"
        );
    }

    // ── fork_active flag lifecycle ──────────────────────────────────────

    #[test]
    fn fork_active_set_during_fork() {
        let mut engine = VimEngine::new();
        assert!(!engine.fork_active, "fork_active must start false");

        let fork = engine.fork().expect("fork should succeed");
        assert!(
            fork.engine_ref().fork_active,
            "fork_active must be true while fork is alive"
        );
        drop(fork);
    }

    #[test]
    fn fork_active_false_after_drop() {
        let mut engine = VimEngine::new();

        {
            let _fork = engine.fork().expect("fork should succeed");
            // fork dropped here
        }

        assert!(
            !engine.fork_active,
            "fork_active must be false after fork drop"
        );
    }

    #[test]
    fn fork_active_false_after_commit() {
        let mut engine = VimEngine::new();

        {
            let fork = engine.fork().expect("fork should succeed");
            fork.commit();
        }

        assert!(
            !engine.fork_active,
            "fork_active must be false after fork commit"
        );
    }

    // ── Error conditions ────────────────────────────────────────────────

    #[test]
    fn fork_error_when_already_forked() {
        let mut engine = VimEngine::new();
        engine.fork_active = true;

        let result = engine.fork();
        assert_eq!(
            result.unwrap_err(),
            ForkError::AlreadyForked,
            "fork must fail when fork_active is true"
        );

        engine.fork_active = false; // cleanup
    }

    // ── ForkError Display ───────────────────────────────────────────────

    #[test]
    fn fork_error_display_already_forked() {
        let err = ForkError::AlreadyForked;
        assert_eq!(
            err.to_string(),
            "a ScopedFork is already active on this engine"
        );
    }

    // ── Multiple sequential forks ───────────────────────────────────────

    #[test]
    fn sequential_forks_work() {
        let mut engine = VimEngine::new();

        // First fork: enter insert mode, roll back
        {
            let mut fork = engine.fork().expect("first fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('i'), ctx);
            // dropped — rolled back
        }
        assert_eq!(engine.mode(), Mode::Normal);

        // Second fork: enter operator-pending, commit
        {
            let mut fork = engine.fork().expect("second fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('d'), ctx);
            fork.commit();
        }
        assert!(engine.pending_operator().is_some());
    }

    // ── cmd_buffer restored on drop ─────────────────────────────────────

    #[test]
    fn cmd_buffer_restored_on_drop() {
        let mut engine = VimEngine::new();
        assert!(engine.cmd_buffer.is_none(), "cmd_buffer must start as None");

        {
            let fork = engine.fork().expect("fork should succeed");
            // We can't easily set cmd_buffer from outside, but we verify
            // the snapshot captures None and restores None on drop.
            drop(fork);
        }

        assert!(
            engine.cmd_buffer.is_none(),
            "cmd_buffer must be None after fork drop"
        );
    }

    // ── is_repeating restored on drop ───────────────────────────────────

    #[test]
    fn is_repeating_restored_on_drop() {
        let mut engine = VimEngine::new();
        assert!(!engine.is_repeating(), "is_repeating must start false");

        {
            let _fork = engine.fork().expect("fork should succeed");
            // Drop without modifying is_repeating — verify restore is clean
        }

        assert!(
            !engine.is_repeating(),
            "is_repeating must be false after fork drop"
        );
    }

    // ── recording restored on drop ──────────────────────────────────────

    #[test]
    fn recording_state_restored_on_drop() {
        let mut engine = VimEngine::new();
        assert!(
            engine.recording_register().is_none(),
            "no recording should be active initially"
        );

        {
            let _fork = engine.fork().expect("fork should succeed");
            // Drop — recording state snapshot (None) is restored
        }

        assert!(
            engine.recording_register().is_none(),
            "recording state must be None after fork drop"
        );
    }

    // ── Hook suppression during fork ──────────────────────────────────

    #[test]
    fn hook_fires_on_normal_process() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // Normal (non-fork) processing — hook should fire.
        let ctx = make_ctx();
        engine.process(KeyEvent::char('l'), ctx);

        assert!(
            called.load(Ordering::SeqCst),
            "PostCommand hook must fire during normal process()"
        );
    }

    #[test]
    fn hook_suppressed_during_fork() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // Process inside a fork — hook should NOT fire (fork_active gating).
        {
            let mut fork = engine.fork().expect("fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('l'), ctx);
            // Drop fork — roll back.
        }

        assert!(
            !called.load(Ordering::SeqCst),
            "PostCommand hook must NOT fire during fork (fork_active suppression)"
        );
    }

    #[test]
    fn hook_fires_after_fork_commit() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                count_clone.fetch_add(1, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // During fork: suppressed.
        {
            let mut fork = engine.fork().expect("fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('l'), ctx);
            fork.commit();
        }

        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "hook must not fire during fork even if committed"
        );

        // After fork ends: normal process resumes hook firing.
        let ctx = make_ctx();
        engine.process(KeyEvent::char('l'), ctx);

        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "hook must fire on process() after fork is closed"
        );
    }

    #[test]
    fn remove_hook_via_public_api() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        let id = engine.add_hook(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // Remove the hook before processing.
        assert!(engine.remove_hook(id), "remove should return true");

        let ctx = make_ctx();
        engine.process(KeyEvent::char('l'), ctx);

        assert!(
            !called.load(Ordering::SeqCst),
            "hook must not fire after removal"
        );
    }

    // ── host.sequencer restored on rollback ─────────────────────────────

    /// Drive a `:w<Enter>` sequence through the engine and return the
    /// `HostRequestId` from the resulting `WriteFile` request.
    ///
    /// The `WriteFile` request is always the first host request produced by
    /// `:w` and its id comes directly from `host.sequencer`.
    fn write_request_id(engine: &mut VimEngine) -> crate::execution::host::HostRequestId {
        use crate::execution::host::HostRequest;

        // ':' enters command-line mode
        engine.process(KeyEvent::char(':'), make_ctx());
        // 'w' types the command
        engine.process(KeyEvent::char('w'), make_ctx());
        // Enter executes it
        let response = engine.process(KeyEvent::enter(), make_ctx());

        let requests = response.host_requests();
        assert!(
            !requests.is_empty(),
            "':w<Enter>' must produce at least one host request"
        );
        let HostRequest::WriteFile { meta, .. } = &requests[0] else {
            panic!(
                "expected WriteFile as first host request, got {:?}",
                requests[0].kind()
            );
        };
        meta.id
    }

    #[test]
    fn sequencer_restored_on_fork_drop() {
        // Verify that rolling back a fork also rolls back `host.sequencer`
        // so that IDs are not permanently skipped.

        let mut engine = VimEngine::new();

        // Capture the ID that fork-internal speculation consumes.
        let id_in_fork;
        {
            let mut fork = engine.fork().expect("fork should succeed");
            id_in_fork = write_request_id(fork.engine_mut());
            // fork dropped here — sequencer rolled back
        }

        // After rollback the sequencer is reset, so the next real `:w`
        // must produce the same ID that the speculative run produced.
        let id_after_rollback = write_request_id(&mut engine);

        assert_eq!(
            id_in_fork, id_after_rollback,
            "host.sequencer must be restored on fork drop: \
             speculative and real `:w` must share the same request ID"
        );
    }

    // ── notify_buf_write_pre / notify_buf_enter / notify_buf_leave ──────

    /// Register a BufWritePre handler; verify it is called when
    /// `notify_buf_write_pre()` is invoked.
    #[test]
    fn buf_write_pre_fires() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::BufWritePre,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        let action = engine.notify_buf_write_pre();

        assert!(
            called.load(Ordering::SeqCst),
            "BufWritePre handler must be called by notify_buf_write_pre()"
        );
        assert_eq!(
            action,
            HookAction::Continue,
            "notify_buf_write_pre() must return Continue when handler returns Continue"
        );
    }

    /// A BufWritePre handler that returns Cancel must cause
    /// `notify_buf_write_pre()` to return Cancel.
    #[test]
    fn buf_write_pre_cancel() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::BufWritePre,
            Box::new(|_: &HookContext<'_>| HookAction::Cancel),
        );

        let action = engine.notify_buf_write_pre();

        assert_eq!(
            action,
            HookAction::Cancel,
            "notify_buf_write_pre() must return Cancel when handler returns Cancel"
        );
    }

    /// Register a BufEnter handler; verify it is called when
    /// `notify_buf_enter()` is invoked.
    #[test]
    fn buf_enter_fires() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::BufEnter,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        let action = engine.notify_buf_enter();

        assert!(
            called.load(Ordering::SeqCst),
            "BufEnter handler must be called by notify_buf_enter()"
        );
        assert_eq!(
            action,
            HookAction::Continue,
            "notify_buf_enter() must return Continue when handler returns Continue"
        );
    }

    /// Register a BufLeave handler; verify it is called when
    /// `notify_buf_leave()` is invoked.
    #[test]
    fn buf_leave_fires() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::BufLeave,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        let action = engine.notify_buf_leave();

        assert!(
            called.load(Ordering::SeqCst),
            "BufLeave handler must be called by notify_buf_leave()"
        );
        assert_eq!(
            action,
            HookAction::Continue,
            "notify_buf_leave() must return Continue when handler returns Continue"
        );
    }

    /// Unregistered BufWritePre bus (no handlers) must return Continue —
    /// the host should not abort when no hook is registered.
    #[test]
    fn buf_write_pre_no_handlers_returns_continue() {
        use crate::execution::engine::hooks::HookAction;

        let mut engine = VimEngine::new();
        assert_eq!(
            engine.notify_buf_write_pre(),
            HookAction::Continue,
            "notify_buf_write_pre() must return Continue when no handlers are registered"
        );
    }

    /// Verify that BufWritePre only fires BufWritePre handlers, not BufEnter.
    #[test]
    fn buf_write_pre_does_not_fire_buf_enter_handler() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let enter_called = Arc::new(AtomicBool::new(false));
        let enter_clone = Arc::clone(&enter_called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::BufEnter,
            Box::new(move |_: &HookContext<'_>| {
                enter_clone.store(true, Ordering::SeqCst);
                HookAction::Cancel
            }),
        );

        // Firing BufWritePre must NOT trigger the BufEnter handler.
        let action = engine.notify_buf_write_pre();
        assert_eq!(
            action,
            HookAction::Continue,
            "notify_buf_write_pre() must not trigger BufEnter handlers"
        );
        assert!(
            !enter_called.load(Ordering::SeqCst),
            "BufEnter handler must not fire when notify_buf_write_pre() is called"
        );
    }

    #[test]
    fn sequencer_not_restored_on_commit() {
        // Verify that committing a fork does NOT roll back the sequencer —
        // the committed ID remains consumed and the next `:w` gets a fresh one.

        let mut engine = VimEngine::new();

        // Commit the fork: the speculative `:w` keeps its ID.
        let id_in_fork;
        {
            let mut fork = engine.fork().expect("fork should succeed");
            id_in_fork = write_request_id(fork.engine_mut());
            fork.commit();
        }

        // The committed fork advanced the sequencer, so the next real `:w`
        // must get a strictly higher ID.
        let id_after_commit = write_request_id(&mut engine);

        assert!(
            id_after_commit > id_in_fork,
            "after fork commit, subsequent `:w` must get a higher request ID \
             (got id_in_fork={:?}, id_after_commit={:?})",
            id_in_fork,
            id_after_commit,
        );
    }

    // ── ModeChanged / CursorMoved hook wiring ──────────────────────────

    #[test]
    fn mode_changed_fires_on_insert() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::ModeChanged,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // 'i' enters Insert mode — should fire ModeChanged.
        let ctx = make_ctx();
        engine.process(KeyEvent::char('i'), ctx);

        assert!(
            called.load(Ordering::SeqCst),
            "ModeChanged hook must fire when 'i' switches to Insert mode"
        );
    }

    #[test]
    fn cursor_moved_fires_on_motion() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::CursorMoved,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // 'l' moves cursor right — should fire CursorMoved.
        let ctx = make_ctx();
        engine.process(KeyEvent::char('l'), ctx);

        assert!(
            called.load(Ordering::SeqCst),
            "CursorMoved hook must fire when 'l' moves cursor"
        );
    }

    #[test]
    fn mode_changed_not_fired_during_fork() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::ModeChanged,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // Process 'i' inside a fork — hook must NOT fire (fork_active gating).
        {
            let mut fork = engine.fork().expect("fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('i'), ctx);
            // Drop fork — roll back.
        }

        assert!(
            !called.load(Ordering::SeqCst),
            "ModeChanged hook must NOT fire during fork (fork_active suppression)"
        );
    }

    #[test]
    fn hook_firing_order() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::{Arc, Mutex};

        let order = Arc::new(Mutex::new(Vec::<HookPoint>::new()));

        let order_mc = Arc::clone(&order);
        let order_cm = Arc::clone(&order);
        let order_pc = Arc::clone(&order);

        let mut engine = VimEngine::new();

        engine.add_hook(
            HookPoint::ModeChanged,
            Box::new(move |_: &HookContext<'_>| {
                order_mc.lock().unwrap().push(HookPoint::ModeChanged);
                HookAction::Continue
            }),
        );
        engine.add_hook(
            HookPoint::CursorMoved,
            Box::new(move |_: &HookContext<'_>| {
                order_cm.lock().unwrap().push(HookPoint::CursorMoved);
                HookAction::Continue
            }),
        );
        engine.add_hook(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                order_pc.lock().unwrap().push(HookPoint::PostCommand);
                HookAction::Continue
            }),
        );

        // 'i' triggers mode change (SetMode) and potentially SetCursor,
        // plus PostCommand fires unconditionally.
        let ctx = make_ctx();
        engine.process(KeyEvent::char('i'), ctx);

        let fired = order.lock().unwrap();
        // ModeChanged must be first (if present), PostCommand must be last.
        assert!(!fired.is_empty(), "at least one hook must fire on 'i'");
        assert_eq!(
            *fired.last().unwrap(),
            HookPoint::PostCommand,
            "PostCommand must be the last hook to fire"
        );
        // If ModeChanged is present it must precede everything else.
        if let Some(mc_pos) = fired.iter().position(|p| *p == HookPoint::ModeChanged) {
            assert_eq!(mc_pos, 0, "ModeChanged must be the first hook to fire");
        }
        // If CursorMoved is present it must come after ModeChanged and before PostCommand.
        if let Some(cm_pos) = fired.iter().position(|p| *p == HookPoint::CursorMoved) {
            let pc_pos = fired
                .iter()
                .position(|p| *p == HookPoint::PostCommand)
                .expect("PostCommand must be present");
            assert!(
                cm_pos < pc_pos,
                "CursorMoved (pos {cm_pos}) must fire before PostCommand (pos {pc_pos})"
            );
            if let Some(mc_pos) = fired.iter().position(|p| *p == HookPoint::ModeChanged) {
                assert!(
                    mc_pos < cm_pos,
                    "ModeChanged (pos {mc_pos}) must fire before CursorMoved (pos {cm_pos})"
                );
            }
        }
    }

    // ── PreCommand hook wiring ─────────────────────────────────────────

    #[test]
    fn precommand_fires_before_execution() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let pre_called = Arc::new(AtomicBool::new(false));
        let pre_clone = Arc::clone(&pre_called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::PreCommand,
            Box::new(move |_: &HookContext<'_>| {
                pre_clone.store(true, Ordering::SeqCst);
                HookAction::Continue // allow execution to proceed
            }),
        );

        // 'l' moves cursor right — a single-key Normal command that goes
        // through execute_effect_plan.
        let ctx = make_ctx();
        let response = engine.process(KeyEvent::char('l'), ctx);

        assert!(
            pre_called.load(Ordering::SeqCst),
            "PreCommand hook must fire before command execution"
        );
        // Execution should have proceeded normally (cursor moved).
        assert!(
            response
                .effects()
                .iter()
                .any(|e| matches!(e, crate::effects::Effect::SetCursor { .. })),
            "command must still execute after PreCommand returns Continue"
        );
    }

    #[test]
    fn precommand_cancel_prevents_execution() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let post_called = Arc::new(AtomicBool::new(false));
        let post_clone = Arc::clone(&post_called);

        let mut engine = VimEngine::new();

        // PreCommand handler that cancels execution.
        engine.add_hook(
            HookPoint::PreCommand,
            Box::new(|_: &HookContext<'_>| HookAction::Cancel),
        );

        // PostCommand handler to verify it does NOT fire.
        engine.add_hook(
            HookPoint::PostCommand,
            Box::new(move |_: &HookContext<'_>| {
                post_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // 'l' would normally move cursor — but PreCommand cancels it.
        let ctx = make_ctx();
        let response = engine.process(KeyEvent::char('l'), ctx);

        // Effects must be empty (command never executed).
        assert!(
            response.effects().is_empty(),
            "effects must be empty when PreCommand cancels: got {:?}",
            response.effects()
        );
        // The response must signal precommand cancellation.
        assert!(
            response.precommand_cancelled(),
            "precommand_cancelled() must return true"
        );
        // PostCommand must NOT fire — there is no "post" to a cancelled command.
        assert!(
            !post_called.load(Ordering::SeqCst),
            "PostCommand must NOT fire when PreCommand cancels"
        );
    }

    #[test]
    fn precommand_not_fired_during_fork() {
        use crate::execution::engine::hooks::{HookAction, HookContext, HookPoint};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);

        let mut engine = VimEngine::new();
        engine.add_hook(
            HookPoint::PreCommand,
            Box::new(move |_: &HookContext<'_>| {
                called_clone.store(true, Ordering::SeqCst);
                HookAction::Continue
            }),
        );

        // Process 'l' inside a fork — PreCommand must NOT fire (fork_active gating).
        {
            let mut fork = engine.fork().expect("fork should succeed");
            let ctx = make_ctx();
            fork.process(KeyEvent::char('l'), ctx);
            // Drop fork — roll back.
        }

        assert!(
            !called.load(Ordering::SeqCst),
            "PreCommand hook must NOT fire during fork (fork_active suppression)"
        );
    }
}
