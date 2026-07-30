//! Unified host integration API for vim-core.
//!
//! This module defines the host contract: a single trait ([`VimHost`]) and
//! session wrapper ([`VimSession`]) that ALL hosts implement, whether
//! native Rust or FFI.
//!
//! # Architecture
//!
//! ```text
//! Host implements:  VimHost (+ Document)
//!                      │
//!                      ▼
//! Session wraps:   VimSession<H: VimHost>
//!                      │
//!        ┌─────────────┼─────────────┐
//!        ▼             ▼             ▼
//!   build context  process key   deliver effects
//!        │             │             │
//!        └─────────────┼─────────────┘
//!                      ▼
//!              H::apply_effects()
//! ```
//!
//! # Design Principles
//!
//! - **Object-safe**: `dyn VimHost` works for FFI adapters
//! - **Zero-cost for native**: `VimSession<MyHost>` monomorphizes
//! - **Prevention-based**: engine never produces effects the host can't handle
//! - **Minimal obligation**: only methods that every host MUST implement are required

use crate::document::Document;
use crate::effects::{Effect, EffectKind};
use crate::errors::VimError;
use crate::execution::context::{InputContext, Validated};
use crate::execution::engine::VimEngine;
use crate::execution::host::{HostRequest, HostResult, RequestDisposition, SplitDirection};
use crate::execution::safety_harness::SafetyHarness;
use crate::keymap::KeyEvent;
use crate::primitives::Offset;

// ═══════════════════════════════════════════════════════════════════════════════
// VimHost trait — the single contract ALL hosts implement
// ═══════════════════════════════════════════════════════════════════════════════

/// The unified host contract for vim-core integration.
///
/// This is the ONLY required trait. Every host — native Rust, FFI adapter,
/// WASM bridge — implements `VimHost`. The trait extends [`Document`] so the
/// host IS a document: `VimSession` passes `&self.host` directly to
/// `InputContext::new()` with no adapter.
///
/// # Design
///
/// - **`Document` supertrait**: provides `text()`, `line_count()`,
///   `offset_to_pos()`, `pos_to_offset()`. The host owns the document;
///   the engine borrows it for each `process()` call.
///
/// - **`capabilities()`**: declares what the host supports. Called once at
///   construction and cached. The engine uses this to filter effects and
///   adapt command resolution.
///
/// - **`apply_effects()`**: the host applies a batch of filtered effects.
///   The engine has already removed effects the host can't handle.
///
/// - **`handle_request()`**: the host performs I/O for a single request.
///   Returns a [`RequestDisposition`]: `Completed` for sync, `Deferred` if
///   the host will complete later, or `Unsupported` for engine fallback.
///
/// # Object Safety
///
/// All methods use `&self` / `&mut self` receivers and concrete return types.
/// No associated types, no generics on methods. `Document` is also
/// object-safe. This means `dyn VimHost` works for FFI adapters that
/// erase the concrete host type.
pub trait VimHost: Document {
    /// Declare host capabilities.
    ///
    /// Called by `VimSession` at construction time. The returned set is
    /// cached and used for all subsequent effect filtering and command
    /// adaptation. Implications are applied automatically.
    fn capabilities(&self) -> HostCapabilitySet;

    /// Current cursor byte offset.
    fn cursor_offset(&self) -> usize;

    /// Apply a batch of effects to the host.
    ///
    /// The effects have already been filtered by the capability set.
    /// The host should apply them in order. Text mutations (Insert,
    /// Delete, Replace) must be applied to the document. Cursor/mode/undo
    /// effects update host UI state.
    ///
    /// The host MUST update its cursor offset, document, and any other
    /// relevant state before the next `process()` call.
    ///
    /// Effects MUST be fully applied before this method returns. During
    /// `VimSession::drain_pending()`, the session calls `build_context()` between
    /// every drained key, reading `text()`, `cursor_offset()`, and other host state.
    /// If mutations from prior effects are not visible, the engine receives stale state.
    fn apply_effects(&mut self, effects: &[Effect]);

    /// Handle a host request (I/O, navigation, etc.).
    ///
    /// Returns one of three dispositions:
    /// - [`RequestDisposition::Completed`]: handled synchronously.
    /// - [`RequestDisposition::Deferred`]: host will complete later via
    ///   `VimSession::complete_request()`.
    /// - [`RequestDisposition::Unsupported`]: host cannot handle this request;
    ///   the engine applies `host_defaults::default_result()` as a fallback.
    ///
    /// The capability set guarantees that only requests matching declared
    /// capabilities arrive here. A host without `FileSystem` will never
    /// receive `WriteFile`.
    ///
    /// For safe fallback values when the host does not handle a request, see
    /// `host_defaults::default_result()`. For the protocol truth table (which
    /// `HostResult` variant each request expects), see `host_defaults::expected_result_kind()`.
    /// For requests that create data dependencies and pause batch draining, see
    /// `host_defaults::pauses_batch_drain()`.
    fn handle_request(&mut self, request: &HostRequest) -> RequestDisposition;

    // ── Optional methods with defaults ───────────────────────────────────

    /// Viewport dimensions for H/M/L motions and scroll calculations.
    ///
    /// Default: 0x0 (forces hosts to set viewport explicitly).
    fn viewport(&self) -> crate::dispatch::ViewportInfo {
        crate::dispatch::ViewportInfo {
            first_line: 0,
            height: 0,
            width: 0,
        }
    }

    /// Visual selection range, if the host tracks selections externally.
    ///
    /// Default: `None` (no selection).
    fn selection(&self) -> Option<crate::primitives::SelectionRange> {
        None
    }

    /// Buffer identifier for cross-buffer jump list navigation.
    ///
    /// Default: `None` (single-buffer host).
    fn buffer_id(&self) -> Option<crate::primitives::BufferId> {
        None
    }

    /// Fold, indent, syntax, search providers for the current keystroke.
    ///
    /// Called every `process_key()` and every drain iteration.
    /// Host owns provider objects as fields; returns references tied to `&self`.
    /// Default: empty providers (no fold, indent, or search support).
    ///
    /// Most hosts should implement at least `SearchProvider` — without it, search
    /// commands (`/`, `n`, `N`, `*`, `#`, `gn`, `gN`) will silently produce no matches.
    fn providers(&self) -> crate::document::Providers<'_> {
        crate::document::Providers::default()
    }

    /// Called when an effect is suppressed because the host lacks the
    /// required capability. Hosts can override this for logging/debugging.
    ///
    /// Default: no-op.
    fn on_effect_suppressed(&mut self, _kind: EffectKind) {}

    /// Record an undo node created internally by the engine (drift
    /// reconciliation, external edits, force-committed INSERT groups).
    ///
    /// `text_before` is the document text before the internal edit.
    /// The host should compute `ChangeSet::from_diff(text_before, self.text())`
    /// and store the resulting changeset keyed by `node_id`.
    ///
    /// Default: no-op (hosts without an UndoStore ignore internal nodes).
    fn record_internal_undo_node(
        &mut self,
        _node_id: crate::primitives::NodeId,
        _text_before: &str,
    ) {
    }

    /// Called immediately before `drain_pending` executes queued keys.
    ///
    /// `drain_pending` processes macro replay and mapping expansion keys,
    /// each of which may produce text mutations with intermediate-state byte
    /// offsets.  Hosts that serialize edit events (e.g. for a wire protocol)
    /// can capture a text snapshot here so the serialization layer can
    /// coalesce intermediate edits into a single diff.
    ///
    /// Default: no-op.
    fn on_before_drain(&mut self) {}

    /// Multi-buffer scope for cross-buffer read access.
    ///
    /// Single-buffer hosts need not implement this. Returns `None` by default.
    /// Only called when a command explicitly needs cross-buffer access, gated
    /// by [`HostCapability::MultiBuffer`].
    fn buffer_scope(&self) -> Option<&dyn crate::document::BufferScope> {
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VimSession<H> — the universal entry point
// ═══════════════════════════════════════════════════════════════════════════════

/// Action deferred for the caller to handle after `process_key()` returns.
///
/// These represent host-specific UI operations that `VimSession` cannot
/// perform (it has no window manager, no scene tree, no terminal multiplexer).
/// The host reads [`ProcessResult::deferred_actions`] and handles each one
/// using its own window/tab/split system.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeferredAction {
    /// Window navigation command (Ctrl-W series).
    WindowNav(WindowNavAction),
}

/// Discriminator enum for [`DeferredAction`] variants.
///
/// Enables map lookups, filtering, and exhaustiveness checks without
/// pattern-matching on the full payload. Mirrors the variant set of
/// [`DeferredAction`] one-to-one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeferredActionKind {
    /// Corresponds to [`DeferredAction::WindowNav`].
    WindowNav,
}

impl DeferredActionKind {
    /// All known [`DeferredActionKind`] values, in declaration order.
    ///
    /// Used in exhaustiveness guard tests to ensure every variant is covered.
    pub const ALL: [Self; 1] = [Self::WindowNav];
}

impl DeferredAction {
    /// Return the [`DeferredActionKind`] discriminator for this action.
    #[must_use]
    pub const fn kind(&self) -> DeferredActionKind {
        match self {
            DeferredAction::WindowNav(_) => DeferredActionKind::WindowNav,
        }
    }
}

/// Window navigation action from Ctrl-W commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowNavAction {
    /// Move cursor to left window (`Ctrl-W h`).
    MoveLeft,
    /// Move cursor to right window (`Ctrl-W l`).
    MoveRight,
    /// Move cursor to window above (`Ctrl-W k`).
    MoveUp,
    /// Move cursor to window below (`Ctrl-W j`).
    MoveDown,
    /// Cycle to next window (`Ctrl-W w`).
    CycleNext,
    /// Cycle to previous window (`Ctrl-W W`).
    CyclePrev,
    /// Close current window (`Ctrl-W c`).
    Close,
    /// Split window horizontally (`Ctrl-W s`).
    Split,
    /// Split window vertically (`Ctrl-W v`).
    VSplit,
    /// Close all windows except current (`Ctrl-W o`).
    Only,
    /// Open a new empty buffer in a split (`Ctrl-W n`).
    New,
    /// Equalize all window sizes (`Ctrl-W =`).
    EqualSize,
    /// Rotate windows downward/rightward (`Ctrl-W r`).
    RotateDown,
    /// Rotate windows upward/leftward (`Ctrl-W R`).
    RotateUp,
    /// Increase window height by count rows (`Ctrl-W +`).
    IncreaseHeight {
        /// Number of rows to increase by.
        count: u32,
    },
    /// Decrease window height by count rows (`Ctrl-W -`).
    DecreaseHeight {
        /// Number of rows to decrease by.
        count: u32,
    },
    /// Increase window width by count columns (`Ctrl-W >`).
    IncreaseWidth {
        /// Number of columns to increase by.
        count: u32,
    },
    /// Decrease window width by count columns (`Ctrl-W <`).
    DecreaseWidth {
        /// Number of columns to decrease by.
        count: u32,
    },
}

/// Result of processing a key through VimSession.
///
/// Contains the consumed flag, any host requests requiring async completion,
/// and deferred actions for the host to handle (e.g., window navigation).
#[derive(Debug)]
#[must_use]
pub struct ProcessResult {
    /// Whether the key was consumed by the engine.
    pub consumed: bool,
    /// Host requests requiring async completion via `complete_request()`.
    pub host_requests: Vec<HostRequest>,
    /// Actions deferred for the host to handle after this call returns.
    ///
    /// Window navigation effects (Ctrl-W series) are intercepted by
    /// `VimSession` and returned here instead of being passed through
    /// `apply_effects()`. The host handles them using its own window
    /// management system.
    pub deferred_actions: Vec<DeferredAction>,
}

/// Safety limit for mapping/macro drain. Aborts runaway mappings.
const MAX_DRAIN_ITERATIONS: u32 = 100_000;

/// Universal entry point for vim-core integration.
///
/// `VimSession<H>` is the universal entry point for all hosts:
///
/// - **Native Rust** (godot-vim): `VimSession<GodotHost>` — monomorphized,
///   zero-cost trait dispatch.
/// - **Dynamic adapter**: `VimSession<dyn VimHost>` — dynamic dispatch
///   through the `VimHost` vtable, for hosts reached across an ABI boundary.
/// - **Test harness**: `VimSession<TestHost>` — in-memory document, test assertions.
///
/// # Lifecycle
///
/// ```ignore
/// let mut host = MyHost::new("hello world");
/// let mut session = VimSession::with_host(host);
///
/// // Process keystrokes
/// let result = session.process_key(KeyEvent::char('d'));
/// let result = session.process_key(KeyEvent::char('w'));
///
/// // Complete async host requests
/// for request in &result.host_requests {
///     // host handles request asynchronously...
///     session.complete_request(&host_result);
/// }
/// ```
pub struct VimSession<H: VimHost> {
    engine: VimEngine,
    host: H,
    capabilities: HostCapabilitySet,
    safety: SafetyHarness,
    pub(crate) pending_deferred: Vec<DeferredAction>,
}

impl<H: VimHost> VimSession<H> {
    /// Create a new session with the given host.
    ///
    /// Reads capabilities from `host.capabilities()`, applies implications,
    /// and caches the result. The engine is initialized with default options.
    #[must_use]
    pub fn with_host(host: H) -> Self {
        let capabilities = host.capabilities().with_implications();
        let mut engine = VimEngine::new();
        engine.set_native_insert(capabilities.has(HostCapability::NativeInsert));
        Self {
            engine,
            host,
            capabilities,
            safety: SafetyHarness::new(),
            pending_deferred: Vec::new(),
        }
    }

    /// Create a session from a pre-configured engine and host.
    ///
    /// Use this when the engine was configured before the host was available
    /// (e.g., Godot's attach/detach lifecycle where settings are applied to
    /// the engine before an editor is assigned).
    #[must_use]
    pub fn from_parts(mut engine: VimEngine, host: H) -> Self {
        let capabilities = host.capabilities().with_implications();
        engine.set_native_insert(capabilities.has(HostCapability::NativeInsert));
        Self {
            engine,
            host,
            capabilities,
            safety: SafetyHarness::new(),
            pending_deferred: Vec::new(),
        }
    }

    /// Decompose the session into its engine and host.
    ///
    /// Use this to reclaim the engine when the host's lifetime ends (e.g.,
    /// editor detach) so engine configuration and state survive across
    /// host instances.
    pub fn into_parts(self) -> (VimEngine, H) {
        (self.engine, self.host)
    }

    /// Process a single key event through the full engine pipeline.
    ///
    /// 1. Builds a validated `InputContext` from the host's document/cursor
    /// 2. Feeds the key to `VimEngine::process()`
    /// 3. Filters effects by capability set
    /// 4. Delivers effects to the host via `apply_effects()`
    /// 5. Handles host requests (sync completions fed back immediately)
    /// 6. Drains all pending keys (macros, mappings)
    ///
    /// Pre-populate the `+` register with system clipboard text.
    ///
    /// Call this before `process_key()` when `clipboard=unnamedplus` so that
    /// `p` reads fresh clipboard content via transparent register aliasing.
    pub fn sync_clipboard(&mut self, text: &str) {
        self.engine.sync_clipboard(text);
    }

    /// Returns a [`ProcessResult`] with the consumed flag and any async
    /// host requests the caller must complete via `complete_request()`.
    pub fn process_key(&mut self, key: KeyEvent) -> ProcessResult {
        let pre_text = self.host.text().to_owned();
        let ctx = Self::build_context(&self.host, &self.safety);
        let response = self.engine.process(key, ctx);
        let consumed = response.consumed();

        self.deliver_effects(response.effects());
        self.sync_shadow_after_undo_redo(response.effects());
        self.sync_internal_undo_nodes(&pre_text);

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);

        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        ProcessResult {
            consumed,
            host_requests,
            deferred_actions: std::mem::take(&mut self.pending_deferred),
        }
    }

    /// Drain the next pending key (macro, mapping) and process it.
    ///
    /// Returns `true` if a key was drained and processed, `false` if
    /// no keys were pending.
    ///
    /// Note: [`process_key`](Self::process_key) now auto-drains all pending
    /// keys. This method is retained for callers that need single-step
    /// drain control.
    ///
    /// **Limitation:** Async host requests produced during single-step drain
    /// are silently dropped. This is acceptable for the primary use case
    /// (debugger step-mode) where no async I/O is expected. Callers that
    /// need async request collection should use [`process_key`](Self::process_key)
    /// instead, which drains all pending keys and returns collected requests.
    pub fn drain_and_process_one(&mut self) -> bool {
        use crate::execution::engine::MacroOutput;

        let Some(output) = self.engine.drain_next_key() else {
            return false;
        };

        match output {
            MacroOutput::Key(key) => {
                let pre_text = self.host.text().to_owned();
                let ctx = Self::build_context(&self.host, &self.safety);
                let response = self.engine.process(key, ctx);

                self.deliver_effects(response.effects());
                self.sync_shadow_after_undo_redo(response.effects());
                self.sync_internal_undo_nodes(&pre_text);
                // Async requests are intentionally discarded in single-step
                // drain mode (see method doc comment).
                let mut discarded = Vec::new();
                self.handle_sync_requests(response.host_requests(), &mut discarded, 0);
            }
            MacroOutput::TextBlock {
                text,
                cursor_offset,
            } => {
                // Text blocks are direct insertions from macro replay.
                // Build Insert + SetCursor effects and deliver them.
                let cursor = self.host.cursor_offset();
                let insert_effect = Effect::insert(Offset::new(cursor), &text);
                let cursor_effect =
                    Effect::set_cursor(Offset::new(cursor.saturating_add(cursor_offset)));
                self.host.apply_effects(&[insert_effect, cursor_effect]);
            }
        }

        true
    }

    /// Whether there are pending keys to drain.
    #[inline]
    #[must_use]
    pub fn has_pending_keys(&self) -> bool {
        self.engine.has_pending_keys()
    }

    /// Complete an asynchronous host request.
    ///
    /// Delivers the completion effects and drains any pending keys that
    /// the completion may have unblocked.
    ///
    /// Returns a [`ProcessResult`] with any new async host requests.
    pub fn complete_request(&mut self, result: &HostResult) -> ProcessResult {
        let response = self.engine.complete_host_request(result);
        let consumed = response.consumed();
        self.deliver_effects(response.effects());

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);

        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        ProcessResult {
            consumed,
            host_requests,
            deferred_actions: std::mem::take(&mut self.pending_deferred),
        }
    }

    /// Cancel a pending async request.
    ///
    /// Use when the host can no longer fulfill a request (e.g., buffer
    /// switched, operation timed out). The engine removes it from pending
    /// and applies the default fallback result.
    ///
    /// Returns `true` if the request was found and cancelled, `false` if
    /// the ID was not in the pending queue (already completed or invalid).
    pub fn cancel_request(&mut self, id: crate::execution::host::HostRequestId) -> bool {
        use crate::execution::host_defaults;

        let Some(pending) = self.engine.remove_pending_host_request(id) else {
            return false;
        };

        // Apply fallback result if one exists for this request kind.
        if let Some(fallback) = host_defaults::default_result(&pending) {
            let response = self.engine.complete_host_request(&fallback);
            self.deliver_effects(response.effects());
        }

        true
    }

    /// Number of pending (unfulfilled) async requests.
    #[inline]
    #[must_use]
    pub fn pending_request_count(&self) -> usize {
        self.engine.pending_request_count()
    }

    /// Direct access to the underlying engine for advanced configuration.
    ///
    /// Use this for engine-level APIs: options, mappings, hooks,
    /// providers, buffer lifecycle. The `VimSession` wrapper handles the
    /// common case; the engine is exposed for everything else.
    #[inline]
    pub const fn engine(&self) -> &VimEngine {
        &self.engine
    }

    /// Mutable access to the engine.
    #[inline]
    pub const fn engine_mut(&mut self) -> &mut VimEngine {
        &mut self.engine
    }

    /// Direct access to the host.
    #[inline]
    pub const fn host(&self) -> &H {
        &self.host
    }

    /// Mutable access to the host.
    #[inline]
    pub const fn host_mut(&mut self) -> &mut H {
        &mut self.host
    }

    /// Current Vim mode. Convenience for `self.engine().mode()`.
    ///
    /// Use this rather than caching mode from `SetMode` effects, which
    /// can drift if effects are filtered. The engine is always authoritative.
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> crate::primitives::Mode {
        self.engine.mode()
    }

    /// Split borrow: simultaneous mutable access to engine and host.
    #[inline]
    pub const fn engine_and_host_mut(&mut self) -> (&mut VimEngine, &mut H) {
        (&mut self.engine, &mut self.host)
    }

    /// Split borrow: mutable engine, shared host, shared safety harness.
    ///
    /// Use when you need to build context (requires `&host` + `&safety`) and
    /// then mutate the engine.
    #[inline]
    pub(crate) const fn engine_host_safety(&mut self) -> (&mut VimEngine, &H, &SafetyHarness) {
        (&mut self.engine, &self.host, &self.safety)
    }

    /// The resolved capability set (with implications applied).
    #[inline]
    #[must_use]
    pub const fn capabilities(&self) -> HostCapabilitySet {
        self.capabilities
    }

    /// Add a capability mid-session.
    ///
    /// Use when a host service comes online (e.g., tree-sitter finished parsing,
    /// LSP connected). The engine immediately starts using the new capability.
    /// Implications are applied automatically (e.g., upgrading WindowManagement
    /// also enables Scrolling).
    pub const fn upgrade_capability(&mut self, cap: HostCapability) {
        self.capabilities = self.capabilities.with(cap).with_implications();
        self.engine
            .set_native_insert(self.capabilities.has(HostCapability::NativeInsert));
    }

    /// Remove a capability mid-session.
    ///
    /// Use when a host service goes offline (e.g., LSP disconnected, tree-sitter
    /// unavailable for this file type). The engine immediately stops generating
    /// effects that require this capability.
    pub const fn downgrade_capability(&mut self, cap: HostCapability) {
        self.capabilities = self.capabilities.without(cap);
        self.engine
            .set_native_insert(self.capabilities.has(HostCapability::NativeInsert));
    }

    // ── Session Lifecycle ────────────────────────────────────────────────

    /// Notify the engine that the host changed the document externally.
    ///
    /// Call this when text is modified outside of vim-core (e.g., an AI
    /// completion insertion, IME commit, external paste, auto-format). The
    /// engine adjusts internal byte offsets (marks, jump list, changelist) and
    /// delivers any resulting effects to the host.
    ///
    /// Returns a [`ProcessResult`] with the consumed flag and any async
    /// host requests.
    pub fn notify_external_edit(&mut self, edit: crate::execution::ExternalEdit) -> ProcessResult {
        let pre_text = self.host.text().to_owned();
        let response = self.engine.apply_external_edit(edit);
        let consumed = response.consumed();
        self.deliver_effects(response.effects());
        self.sync_shadow_after_undo_redo(response.effects());
        self.sync_internal_undo_nodes(&pre_text);

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);

        ProcessResult {
            consumed,
            host_requests,
            deferred_actions: std::mem::take(&mut self.pending_deferred),
        }
    }

    /// Save per-buffer state before leaving a buffer.
    ///
    /// Sets the `'"` (last-position) mark at `cursor_offset`, then extracts
    /// all per-buffer state (marks, changelist, visual, overrides, undo tree,
    /// etc.) from the engine. The caller should persist the returned
    /// [`BufferLocalState`](crate::execution::BufferLocalState) keyed by buffer identity
    /// and pass it back to
    /// [`on_buffer_enter`](Self::on_buffer_enter) when returning to this buffer.
    pub fn on_buffer_leave(&mut self, cursor_offset: usize) -> crate::execution::BufferLocalState {
        self.engine.on_buffer_leave(cursor_offset)
    }

    /// Restore per-buffer state when entering a buffer.
    ///
    /// For first-visit buffers, pass
    /// [`BufferLocalState::default()`](crate::execution::BufferLocalState) to
    /// initialize with empty per-buffer state.
    pub fn on_buffer_enter(&mut self, state: crate::execution::BufferLocalState) {
        self.engine.on_buffer_enter(state);
    }

    /// Process a mouse click at the given byte offset.
    ///
    /// Resets parser state, pushes to jumplist, normalizes to Normal mode,
    /// and positions the cursor at `target_offset`. Equivalent to
    /// [`HostSession::process_click_host`](crate::execution::HostSession) but through the
    /// `VimSession` wrapper.
    ///
    /// Returns a [`ProcessResult`] with any async host requests.
    pub fn process_click(&mut self, target_offset: usize) -> ProcessResult {
        let ctx = Self::build_context(&self.host, &self.safety);
        let response = self.engine.process_click(target_offset, &ctx);
        let consumed = response.consumed();
        self.deliver_effects(response.effects());

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);

        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        ProcessResult {
            consumed,
            host_requests,
            deferred_actions: std::mem::take(&mut self.pending_deferred),
        }
    }

    /// Process a mouse selection (drag) at the given anchor and head byte offsets.
    ///
    /// Enters Visual mode with a selection spanning `anchor_offset..head_offset`.
    /// Like [`process_click`](Self::process_click), handles jumplist push,
    /// parser reset, mapping clear, and macro abort.
    ///
    /// Returns a [`ProcessResult`] with any async host requests.
    pub fn process_mouse_selection(
        &mut self,
        anchor_offset: usize,
        head_offset: usize,
        shape: crate::primitives::SelectionShape,
    ) -> ProcessResult {
        let ctx = Self::build_context(&self.host, &self.safety);
        let response = self
            .engine
            .process_mouse_selection(anchor_offset, head_offset, shape, &ctx);
        let consumed = response.consumed();
        self.deliver_effects(response.effects());

        let mut host_requests = Vec::new();
        self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);

        let drain_requests = self.drain_pending();
        host_requests.extend(drain_requests);

        ProcessResult {
            consumed,
            host_requests,
            deferred_actions: std::mem::take(&mut self.pending_deferred),
        }
    }

    // ── Internal ─────────────────────────────────────────────────────────

    /// Safety limit for recursive sub-request handling.
    /// Prevents unbounded recursion when sync completions produce new requests
    /// that themselves produce more requests (e.g. :source loading config
    /// that triggers :write).
    const MAX_REQUEST_DEPTH: u32 = 5;

    /// Handle host requests, recursively processing sub-requests from sync completions.
    ///
    /// For each request, asks the host to handle it. If the host returns `Some(result)`
    /// (synchronous completion), feeds it back to the engine and delivers the completion's
    /// effects. Crucially, also handles any *sub-requests* that the completion produces,
    /// which were previously silently dropped.
    ///
    /// Requests the host returns `None` for (async) are collected into `async_collector`.
    ///
    /// `depth` tracks recursion to prevent unbounded chains. When exceeded, all
    /// remaining requests are failed with an error message.
    pub(crate) fn handle_sync_requests(
        &mut self,
        requests: &[HostRequest],
        async_collector: &mut Vec<HostRequest>,
        depth: u32,
    ) {
        if depth > Self::MAX_REQUEST_DEPTH {
            for request in requests {
                let failure = HostResult::Failure {
                    id: request.id(),
                    error: compact_str::CompactString::from("host request depth limit exceeded"),
                };
                let _ = self.engine.complete_host_request(&failure);
            }
            return;
        }
        for request in requests {
            // Intercept window navigation requests → DeferredAction.
            // These are handled by the caller via ProcessResult::deferred_actions
            // instead of being passed to the host.
            if let Some(action) = host_request_to_window_action(request) {
                self.pending_deferred
                    .push(DeferredAction::WindowNav(action));
                // Auto-complete the request so the engine doesn't wait forever.
                let success = HostResult::Success {
                    id: request.id(),
                    message: None,
                };
                let _ = self.engine.complete_host_request(&success);
                continue;
            }

            // Intercept :norm requests → execute internally.
            // VimSession has both engine and host access, so it can feed keys
            // and drain pending for each line without host-side code.
            if let HostRequest::ExecuteNorm {
                meta,
                start_line,
                end_line,
                ref keys,
                remap,
            } = *request
            {
                self.execute_norm_command(start_line, end_line, keys, remap);
                // Auto-complete the request so the engine doesn't wait forever.
                let success = HostResult::Success {
                    id: meta.id,
                    message: None,
                };
                let _ = self.engine.complete_host_request(&success);
                continue;
            }

            match self.host.handle_request(request) {
                RequestDisposition::Completed(result) => {
                    let completion = self.engine.complete_host_request(&result);
                    self.deliver_effects(completion.effects());
                    if !completion.host_requests().is_empty() {
                        self.handle_sync_requests(
                            completion.host_requests(),
                            async_collector,
                            depth + 1,
                        );
                    }
                }
                RequestDisposition::Deferred => {
                    async_collector.push(request.clone());
                }
                RequestDisposition::Unsupported => {
                    use crate::execution::host_defaults;
                    if let Some(fallback) = host_defaults::default_result(request) {
                        let completion = self.engine.complete_host_request(&fallback);
                        self.deliver_effects(completion.effects());
                        if !completion.host_requests().is_empty() {
                            self.handle_sync_requests(
                                completion.host_requests(),
                                async_collector,
                                depth + 1,
                            );
                        }
                    } else {
                        // No fallback available — treat as deferred so the host
                        // can still complete it later if needed.
                        async_collector.push(request.clone());
                    }
                }
            }
        }
    }

    /// Build a validated InputContext from the current host state.
    ///
    /// Takes `&H` rather than `&self` so the borrow checker can see
    /// disjoint borrows of `self.host` vs `self.engine`.
    ///
    /// Called at the start of every process_key() and drain iteration.
    /// Reads document, cursor, viewport, selection, buffer_id, and providers
    /// from the host.
    pub(crate) fn build_context<'a>(
        host: &'a H,
        safety: &SafetyHarness,
    ) -> InputContext<'a, H, Validated> {
        let cursor = host.cursor_offset();
        let mut providers = safety.query(|| Some(host.providers())).unwrap_or_default();

        // Per-capability granular downgrade: null out provider fields whose
        // corresponding capabilities have been auto-disabled due to repeated
        // panics. This preserves working providers even when one misbehaves.
        if safety.has_any_disabled() {
            if safety.is_disabled(HostCapability::Folding as u8) {
                providers.fold = None;
            }
            if safety.is_disabled(HostCapability::Reindent as u8) {
                providers.indent = None;
            }
            if safety.is_disabled(HostCapability::SearchHighlight as u8) {
                providers.search = None;
            }
        }

        let ctx = InputContext::new(host, cursor)
            .validate_clamped()
            .with_viewport(host.viewport())
            .with_providers(providers);
        let ctx = match host.selection() {
            Some(sel) => ctx.with_selection(sel),
            None => ctx,
        };
        match host.buffer_id() {
            Some(id) => ctx.with_buffer_id(id),
            None => ctx,
        }
    }

    /// Drain all pending keys (macros, mappings) and process each one.
    ///
    /// Returns any async host requests that accumulated during draining.
    /// Applies a safety limit ([`MAX_DRAIN_ITERATIONS`]) to abort runaway
    /// mappings/macros.
    pub(crate) fn drain_pending(&mut self) -> Vec<HostRequest> {
        use crate::execution::engine::MacroOutput;

        if self.engine.has_pending_keys() {
            self.host.on_before_drain();
        }

        let merging = self.engine.undo_tree().is_merging();
        if merging {
            self.host.apply_effects(&[Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            }]);
        }

        let mut host_requests = Vec::new();
        let mut iterations: u32 = 0;

        while let Some(output) = self.engine.drain_next_key() {
            iterations += 1;
            if iterations > MAX_DRAIN_ITERATIONS {
                self.engine.abort_replay();
                self.host.apply_effects(&[Effect::show_error(
                    VimError::MacroEffectLimitExceeded {
                        limit: MAX_DRAIN_ITERATIONS as usize,
                    },
                )]);
                break;
            }

            match output {
                MacroOutput::Key(key) => {
                    let pre_text = self.host.text().to_owned();
                    let ctx = Self::build_context(&self.host, &self.safety);
                    let response = self.engine.process(key, ctx);
                    let consumed = response.consumed();

                    let has_error = response
                        .effects()
                        .iter()
                        .any(|e| matches!(e, Effect::ShowError { .. }));

                    self.deliver_effects(response.effects());
                    self.sync_shadow_after_undo_redo(response.effects());
                    self.sync_internal_undo_nodes(&pre_text);
                    self.handle_sync_requests(response.host_requests(), &mut host_requests, 0);

                    if !consumed || has_error {
                        self.engine.abort_replay();
                        break;
                    }
                }
                MacroOutput::TextBlock {
                    text,
                    cursor_offset,
                } => {
                    let cursor = self.host.cursor_offset();
                    let effects = [
                        Effect::insert(Offset::new(cursor), &text),
                        Effect::set_cursor(Offset::new(cursor.saturating_add(cursor_offset))),
                    ];
                    self.host.apply_effects(&effects);
                }
            }
        }

        if merging {
            // Pass the current undo tree node so the host's UndoStore can
            // associate the drain's text mutations with a real undo group.
            // `None` would discard the group, losing undo-ability for macro
            // replay and mapping expansion.
            let node_id = if iterations > 0 {
                Some(self.engine.undo_tree().current())
            } else {
                None
            };
            self.host.apply_effects(&[Effect::EndUndoGroup { node_id }]);
        }

        host_requests
    }

    /// Filter effects by capability and deliver to host.
    ///
    /// Defense-in-depth: filter effects the host can't handle.
    /// Sync the engine's shadow document from the host after Undo/Redo.
    ///
    /// `update_shadow_from_effects` cannot track Undo/Redo incrementally
    /// because the engine doesn't know the resulting text. After the host
    /// applies the undo/redo changeset, the shadow must be refreshed from
    /// the host's authoritative text. Without this, the drift gate on the
    /// next `process()` call would detect a "drift" and create a phantom
    /// external-edit undo node that the host's UndoStore knows nothing
    /// about, causing "no snapshot for node N" on subsequent undos.
    pub(crate) fn sync_shadow_after_undo_redo(&mut self, effects: &[Effect]) {
        if effects
            .iter()
            .any(|e| matches!(e, Effect::Undo { .. } | Effect::Redo { .. }))
        {
            let host_text = self.host.text().to_owned();
            self.engine.set_shadow_text(host_text);
        }
    }

    /// Record any undo nodes the engine created internally during the last
    /// `process()` call.
    ///
    /// Handles two cases:
    /// 1. **Force-committed node** — an INSERT session's pending undo group
    ///    was committed by `begin_external_group`. The host's UndoStore already
    ///    has `pending_text` from the INSERT's `BeginUndoGroup`, so a synthetic
    ///    `EndUndoGroup` commits it under the force-committed `NodeId`.
    /// 2. **External-edit node** — a drift reconciliation or non-merging
    ///    external edit created its own undo group. `pre_text` is the document
    ///    text captured *before* `engine.process()`, providing the T0 needed
    ///    to compute the changeset.
    ///
    /// If a force-commit occurred during INSERT mode, a synthetic
    /// `BeginUndoGroup` re-opens the pending group for continuation typing.
    fn sync_internal_undo_nodes(&mut self, pre_text: &str) {
        let fc_node = self.engine.take_last_force_committed_node();
        let ext_node = self.engine.take_last_external_edit_node();

        if fc_node.is_none() && ext_node.is_none() {
            return;
        }

        // Step 1: commit the force-committed INSERT pending group.
        if let Some(fc_id) = fc_node {
            self.host.apply_effects(&[Effect::EndUndoGroup {
                node_id: Some(fc_id),
            }]);
        }

        // Step 2: record the external-edit node using pre-edit text as T0.
        if let Some(ext_id) = ext_node {
            self.host.record_internal_undo_node(ext_id, pre_text);
        }

        // Step 3: re-open a pending group for INSERT continuation.
        if fc_node.is_some() && self.engine.mode().is_insert() {
            self.host.apply_effects(&[Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            }]);
        }
    }

    /// For FULL hosts (both current hosts), this is skipped entirely (fast path).
    /// For non-FULL hosts, this catches any effects the engine produces that
    /// the host lacks capabilities for. Once capability-aware command resolution
    /// is implemented, this becomes a pure safety net.
    ///
    /// Also suppresses `SubstitutePreview` and `ClearSubstitutePreview` during
    /// macro replay, mapping expansion, and dot repeat. Intermediate `:s`
    /// commands produce preview effects that flicker the UI with no user
    /// benefit. The check is free on the normal path (single bool OR).
    pub(crate) fn deliver_effects(&mut self, effects: &[Effect]) {
        if effects.is_empty() {
            return;
        }

        // Suppress substitute preview during macro replay / dot repeat.
        // Intermediate :s commands produce preview effects that would
        // flicker the UI with no user benefit. The condition is cheap
        // (two bool checks) and almost never true during normal typing.
        let suppress_preview = self.engine.has_pending_keys() || self.engine.is_repeating();

        // Fast path: if host has FULL capabilities and no preview suppression needed.
        if self.capabilities == HostCapabilitySet::FULL {
            if suppress_preview
                && effects.iter().any(|e| {
                    matches!(
                        e,
                        Effect::SubstitutePreview { .. } | Effect::ClearSubstitutePreview
                    )
                })
            {
                let owned: smallvec::SmallVec<[Effect; 8]> = effects
                    .iter()
                    .filter(|e| {
                        !matches!(
                            e,
                            Effect::SubstitutePreview { .. } | Effect::ClearSubstitutePreview
                        )
                    })
                    .cloned()
                    .collect();
                if !owned.is_empty() {
                    self.host.apply_effects(&owned);
                }
            } else {
                self.host.apply_effects(effects);
            }
            return;
        }

        // Filter effects the host can handle, also suppressing previews during replay.
        let filtered: smallvec::SmallVec<[&Effect; 8]> = effects
            .iter()
            .filter(|e| {
                // Suppress substitute preview during replay.
                if suppress_preview
                    && matches!(
                        e,
                        Effect::SubstitutePreview { .. } | Effect::ClearSubstitutePreview
                    )
                {
                    return false;
                }

                let kind = e.kind();
                if should_deliver(kind, self.capabilities) {
                    true
                } else {
                    self.host.on_effect_suppressed(kind);
                    false
                }
            })
            .collect();

        if filtered.is_empty() {
            return;
        }

        // Build a contiguous slice for the host. Most filtered batches
        // are the full original (host has the capability), so we try to
        // avoid allocation.
        if filtered.len() == effects.len() {
            // No filtering happened — pass original slice.
            self.host.apply_effects(effects);
        } else {
            // Some effects were filtered — clone the survivors.
            let owned: smallvec::SmallVec<[Effect; 8]> = filtered.into_iter().cloned().collect();
            self.host.apply_effects(&owned);
        }
    }
    /// Execute `:norm` command internally: feed keys to each line in range.
    ///
    /// For each line in `[start_line, end_line]`, positions the cursor at
    /// line start, ensures Normal mode, feeds keys into the engine, and
    /// drains all pending keys. The whole operation is wrapped in a single
    /// undo group for single-step undo.
    ///
    /// Called from `handle_sync_requests` when a `HostRequest::ExecuteNorm`
    /// is intercepted. The engine routes `Effect::NormCommand` to
    /// `HostRequest::ExecuteNorm` via `host_completion.rs`, so interception
    /// happens at the request level, not the effect level.
    fn execute_norm_command(&mut self, start_line: u32, end_line: u32, keys: &str, remap: bool) {
        if keys.is_empty() {
            return;
        }

        // Wrap in undo group for single undo step.
        self.host.apply_effects(&[Effect::BeginUndoGroup {
            cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
        }]);

        let start = start_line as usize;
        let end = end_line as usize;

        for line_idx in start..=end {
            // Recompute line start from CURRENT document state (prior lines
            // may have changed text/line count).
            let line_start = self
                .host
                .pos_to_offset(crate::primitives::Position::from_raw(line_idx, 0));

            if let Some(offset) = line_start {
                // Position cursor at line start.
                self.host.apply_effects(&[Effect::set_cursor(offset)]);
            } else {
                // Line no longer exists (prior :norm operations may have
                // deleted lines). Stop processing.
                break;
            }

            // Reset to Normal mode if not already.
            if !self.engine.mode().is_normal() {
                self.engine.set_mode(crate::primitives::Mode::Normal);
            }

            // Feed keys into typeahead.
            self.engine.feed_keys(keys, remap);

            // Drain all pending keys (full pipeline for each key).
            let _ = self.drain_pending();
        }

        // End undo group with real node_id so the host's UndoStore
        // associates text mutations with a proper undo group.
        let node_id = Some(self.engine.undo_tree().current());
        self.host.apply_effects(&[Effect::EndUndoGroup { node_id }]);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HostCapability enum — individual capability flags
// ═══════════════════════════════════════════════════════════════════════════════

/// A single host capability.
///
/// Capabilities form a partial order (lattice). Some capabilities imply others:
/// - `WindowManagement` implies `Scrolling`
/// - `SubstitutePreview` implies `SearchHighlight`
///
/// The engine uses declared capabilities to:
/// 1. Filter/transform effects before delivery
/// 2. Adapt command resolution (e.g., `zc` without `Folding` → error message)
/// 3. Gate host requests that require specific host support
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum HostCapability {
    // ── Core (always present, bits 0-3) ──────────────────────────────────
    /// Insert/Delete/Replace text mutations.
    TextMutation = 0,
    /// SetCursor positioning.
    CursorMovement = 1,
    /// SetMode tracking.
    ModeTracking = 2,
    /// Begin/EndUndoGroup.
    UndoGrouping = 3,

    // ── Standard (bits 4-12) ─────────────────────────────────────────────
    /// ScrollTo, CenterCursor, CursorToTop, CursorToBottom, ScrollLeft/Right.
    Scrolling = 4,
    /// HighlightMatches, ClearHighlights, SearchMatchInfo.
    SearchHighlight = 5,
    /// ShowInfo, ShowWarning, ClearMessage. (ShowError is unconditional.)
    StatusMessages = 6,
    /// SetRegister, ClearNamedRegister.
    Registers = 7,
    /// CommandLineEdit, SyncCommandLine.
    CommandLine = 8,
    /// FoldLine, UnfoldLine, ToggleFold, FoldAll, UnfoldAll, etc.
    Folding = 9,
    /// CopyToClipboard, ReadClipboard.
    Clipboard = 10,
    /// CursorStyle hint delivery.
    CursorStyle = 11,
    /// Highlight feedback (persistent and flash highlights).
    YankHighlight = 12,

    // ── Advanced (bits 13-19) ────────────────────────────────────────────
    /// WriteFile, ReadFile, EditFile.
    FileSystem = 13,
    /// ExternalCommand, FilterDocumentRange.
    Shell = 14,
    /// SwitchBuffer, BufferNext/Prev/First/Last, BufferList.
    MultiBuffer = 15,
    /// TabNew, TabNext/Prev/Close.
    Tabs = 16,
    /// WindowSplit, WindowClose, WindowMove*, WindowResize, etc.
    WindowManagement = 17,
    /// GotoDefinition, ShowDocumentation.
    LspNavigation = 18,
    /// EvaluateExpression, EvaluateMapping.
    ExpressionEval = 19,

    // ── Specialist (bits 20-24) ──────────────────────────────────────────
    /// RequestCompletion (insert-mode completion).
    Completion = 20,
    /// ReindentRange.
    Reindent = 21,
    /// SubstitutePreview, ClearSubstitutePreview.
    SubstitutePreview = 22,
    /// SetVirtualText, ClearVirtualText, SetDiagnostics.
    VirtualText = 23,
    /// OpenCommandWindow (q:, q/, q?).
    CommandWindow = 24,

    // ── Host Input Model (bits 25+) ─────────────────────────────────────
    /// Host handles printable characters and Enter natively in insert mode,
    /// with its own text-change synchronization (e.g., a document-change
    /// notification that reports the inserted text back to the engine).
    ///
    /// When declared: printable chars and Enter pass through to the host.
    /// When absent: the engine handles all insert-mode keys through the
    /// full pipeline (undo, dot-repeat, macro recording, changelist).
    NativeInsert = 25,

    // ── Which-Key (bit 26) ──────────────────────────────────────────────
    /// Which-key hint popup data delivery.
    /// When declared: `HostResponse.key_hints` is computed from engine state.
    /// When absent: `key_hints` is always `None` (zero computation cost).
    WhichKey = 26,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Operator range extraction
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// HostCapabilitySet — bitmask of host capabilities
// ═══════════════════════════════════════════════════════════════════════════════

/// Bitmask of [`HostCapability`] values.
///
/// Stored as `u64` — supports up to 64 capabilities with zero-cost
/// membership checks. Implements `BitOr` for ergonomic construction.
///
/// # Examples
///
/// ```ignore
/// use vim_core::execution::host_api::{HostCapabilitySet, HostCapability};
///
/// let caps = HostCapabilitySet::CORE
///     .with(HostCapability::Scrolling)
///     .with(HostCapability::Clipboard)
///     .with(HostCapability::StatusMessages);
///
/// assert!(caps.has(HostCapability::TextMutation)); // Core always present
/// assert!(caps.has(HostCapability::Clipboard));
/// assert!(!caps.has(HostCapability::Folding));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostCapabilitySet(u64);

impl HostCapabilitySet {
    /// Empty set — no capabilities declared. Not useful for real hosts,
    /// but serves as the identity element for `union`.
    pub const EMPTY: Self = Self(0);

    /// Core capabilities that every host MUST support.
    /// TextMutation + CursorMovement + ModeTracking + UndoGrouping.
    pub const CORE: Self = Self(
        (1 << HostCapability::TextMutation as u64)
            | (1 << HostCapability::CursorMovement as u64)
            | (1 << HostCapability::ModeTracking as u64)
            | (1 << HostCapability::UndoGrouping as u64),
    );

    /// Standard capabilities — Core + all bits 0-12 for good UX.
    pub const STANDARD: Self = Self(
        Self::CORE.0
            | (1 << HostCapability::Scrolling as u64)
            | (1 << HostCapability::SearchHighlight as u64)
            | (1 << HostCapability::StatusMessages as u64)
            | (1 << HostCapability::Registers as u64)
            | (1 << HostCapability::CommandLine as u64)
            | (1 << HostCapability::Folding as u64)
            | (1 << HostCapability::Clipboard as u64)
            | (1 << HostCapability::CursorStyle as u64)
            | (1 << HostCapability::YankHighlight as u64),
    );

    /// Full capabilities — everything vim-core can produce.
    pub const FULL: Self = Self(
        (1 << HostCapability::TextMutation as u64)
            | (1 << HostCapability::CursorMovement as u64)
            | (1 << HostCapability::ModeTracking as u64)
            | (1 << HostCapability::UndoGrouping as u64)
            | (1 << HostCapability::Scrolling as u64)
            | (1 << HostCapability::SearchHighlight as u64)
            | (1 << HostCapability::StatusMessages as u64)
            | (1 << HostCapability::Registers as u64)
            | (1 << HostCapability::CommandLine as u64)
            | (1 << HostCapability::Folding as u64)
            | (1 << HostCapability::Clipboard as u64)
            | (1 << HostCapability::CursorStyle as u64)
            | (1 << HostCapability::YankHighlight as u64)
            | (1 << HostCapability::FileSystem as u64)
            | (1 << HostCapability::Shell as u64)
            | (1 << HostCapability::MultiBuffer as u64)
            | (1 << HostCapability::Tabs as u64)
            | (1 << HostCapability::WindowManagement as u64)
            | (1 << HostCapability::LspNavigation as u64)
            | (1 << HostCapability::ExpressionEval as u64)
            | (1 << HostCapability::Completion as u64)
            | (1 << HostCapability::Reindent as u64)
            | (1 << HostCapability::SubstitutePreview as u64)
            | (1 << HostCapability::VirtualText as u64)
            | (1 << HostCapability::CommandWindow as u64)
            | (1 << HostCapability::NativeInsert as u64)
            | (1 << HostCapability::WhichKey as u64),
    );

    /// Add a single capability.
    #[inline]
    #[must_use]
    pub const fn with(self, cap: HostCapability) -> Self {
        Self(self.0 | (1 << cap as u64))
    }

    /// Remove a single capability.
    #[inline]
    #[must_use]
    pub const fn without(self, cap: HostCapability) -> Self {
        Self(self.0 & !(1 << cap as u64))
    }

    /// Check if a capability is present.
    #[inline]
    #[must_use]
    pub const fn has(self, cap: HostCapability) -> bool {
        (self.0 & (1 << cap as u64)) != 0
    }

    /// Union of two capability sets.
    #[inline]
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Apply capability implications (transitive closure).
    ///
    /// Some capabilities imply others:
    /// - `WindowManagement` → `Scrolling` (windows need scroll)
    /// - `SubstitutePreview` → `SearchHighlight` (preview needs highlight)
    #[must_use]
    pub const fn with_implications(self) -> Self {
        let mut bits = self.0;
        if bits & (1 << HostCapability::WindowManagement as u64) != 0 {
            bits |= 1 << HostCapability::Scrolling as u64;
        }
        if bits & (1 << HostCapability::SubstitutePreview as u64) != 0 {
            bits |= 1 << HostCapability::SearchHighlight as u64;
        }
        Self(bits)
    }

    /// Check if this set is a superset of another.
    #[inline]
    #[must_use]
    pub const fn contains_all(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Get the raw bitmask (for FFI serialization).
    #[inline]
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Construct from a raw bitmask (for FFI deserialization).
    #[inline]
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Number of capabilities in this set.
    #[inline]
    #[must_use]
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }
}

impl std::ops::BitOr for HostCapabilitySet {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for HostCapabilitySet {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Effect metadata — maps EffectKind → required HostCapability
// ═══════════════════════════════════════════════════════════════════════════════

/// Required capability for a single [`EffectKind`].
///
/// `None` means the effect is engine-internal (consumed by the effect
/// processor before reaching the host) or unconditionally delivered.
#[must_use]
pub const fn required_capability(kind: EffectKind) -> Option<HostCapability> {
    match kind {
        // ── Core (always delivered) ──────────────────────────────────────
        EffectKind::Insert | EffectKind::Delete | EffectKind::Replace => {
            Some(HostCapability::TextMutation)
        }

        EffectKind::SetCursor => Some(HostCapability::CursorMovement),

        EffectKind::SetSelection | EffectKind::ClearSelection => {
            Some(HostCapability::CursorMovement)
        }

        EffectKind::SetMode => Some(HostCapability::ModeTracking),

        EffectKind::BeginInsert | EffectKind::SetBlockInsert | EffectKind::OperatorToMark => {
            Some(HostCapability::ModeTracking)
        }

        EffectKind::BeginUndoGroup
        | EffectKind::EndUndoGroup
        | EffectKind::Undo
        | EffectKind::UndoLine
        | EffectKind::Redo => Some(HostCapability::UndoGrouping),

        // ── Scrolling ────────────────────────────────────────────────────
        EffectKind::ScrollTo
        | EffectKind::CenterCursor
        | EffectKind::CursorToTop
        | EffectKind::CursorToBottom
        | EffectKind::ScrollLeft
        | EffectKind::ScrollRight
        | EffectKind::ScrollHalfScreenLeft
        | EffectKind::ScrollHalfScreenRight
        | EffectKind::ScrollCursorToLeftEdge
        | EffectKind::ScrollCursorToRightEdge => Some(HostCapability::Scrolling),

        // ── Search Highlight ─────────────────────────────────────────────
        EffectKind::HighlightMatches
        | EffectKind::ClearHighlights
        | EffectKind::SearchMatchInfo
        | EffectKind::SetSearchPattern => Some(HostCapability::SearchHighlight),

        // ── Status Messages ──────────────────────────────────────────────
        EffectKind::Bell
        | EffectKind::ShowInfo
        | EffectKind::ShowWarning
        | EffectKind::ClearMessage => Some(HostCapability::StatusMessages),

        // ShowError is unconditionally delivered — it is the engine's only
        // error reporting channel. Without it, capability fallbacks and
        // invalid commands become invisible to the user.
        EffectKind::ShowError => None,

        // ── Registers ────────────────────────────────────────────────────
        EffectKind::SetRegister | EffectKind::ClearNamedRegister => Some(HostCapability::Registers),

        // ── Command Line ─────────────────────────────────────────────────
        EffectKind::CommandLineEdit => Some(HostCapability::CommandLine),

        // ── Folding ──────────────────────────────────────────────────────
        EffectKind::FoldLine
        | EffectKind::UnfoldLine
        | EffectKind::ToggleFold
        | EffectKind::ToggleFoldRecursive
        | EffectKind::FoldAll
        | EffectKind::UnfoldAll
        | EffectKind::FoldLineRecursive
        | EffectKind::UnfoldLineRecursive
        | EffectKind::DeleteFold
        | EffectKind::DeleteFoldRecursive
        | EffectKind::EliminateAllFolds
        | EffectKind::ToggleFoldEnable
        | EffectKind::SetFoldEnable
        | EffectKind::SyncFoldRanges => Some(HostCapability::Folding),

        // ── Clipboard ────────────────────────────────────────────────────
        EffectKind::CopyToClipboard => Some(HostCapability::Clipboard),

        // ── Cursor Style ─────────────────────────────────────────────────
        EffectKind::SetCursorStyle | EffectKind::CursorShapeHint => {
            Some(HostCapability::CursorStyle)
        }

        // ── Window Management ────────────────────────────────────────────
        EffectKind::WindowSplit
        | EffectKind::WindowNew
        | EffectKind::WindowVSplit
        | EffectKind::WindowClose
        | EffectKind::WindowOnly
        | EffectKind::WindowNext
        | EffectKind::WindowPrev
        | EffectKind::WindowMoveLeft
        | EffectKind::WindowMoveRight
        | EffectKind::WindowMoveUp
        | EffectKind::WindowMoveDown
        | EffectKind::WindowEqualSize
        | EffectKind::WindowIncreaseHeight
        | EffectKind::WindowDecreaseHeight
        | EffectKind::WindowIncreaseWidth
        | EffectKind::WindowDecreaseWidth
        | EffectKind::WindowRotateDown
        | EffectKind::WindowRotateUp => Some(HostCapability::WindowManagement),

        // ── LSP Navigation ───────────────────────────────────────────────
        EffectKind::GotoDefinition | EffectKind::ShowDocumentation => {
            Some(HostCapability::LspNavigation)
        }

        // ── Substitute Preview ───────────────────────────────────────────
        EffectKind::SubstitutePreview | EffectKind::ClearSubstitutePreview => {
            Some(HostCapability::SubstitutePreview)
        }

        // ── Virtual Text ─────────────────────────────────────────────────
        EffectKind::SetVirtualText | EffectKind::ClearVirtualText | EffectKind::SetDiagnostics => {
            Some(HostCapability::VirtualText)
        }

        // ── Command Window ───────────────────────────────────────────────
        EffectKind::OpenCommandWindow => Some(HostCapability::CommandWindow),

        // ── Completion (effects flow via HostRequest, not Effect) ────────
        EffectKind::CallOperatorFunc => None, // Extension point — always delivered

        // ── Engine-internal (consumed before reaching host) ──────────────
        EffectKind::SaveLastVisual
        | EffectKind::SetMark
        | EffectKind::ClearMark
        | EffectKind::PushJumpList
        | EffectKind::JumpOlder
        | EffectKind::JumpNewer
        | EffectKind::JumpToBuffer
        | EffectKind::ChangelistOlder
        | EffectKind::ChangelistNewer
        | EffectKind::SetLastSubstitute
        | EffectKind::SetLastSubstituteFlags
        | EffectKind::SetSubstitutePattern
        | EffectKind::SetLastFind
        | EffectKind::SetScrollHalfCount
        | EffectKind::SetStickyColumn
        | EffectKind::StartRecording
        | EffectKind::StopRecording
        | EffectKind::PlayMacro
        | EffectKind::SetExtState
        | EffectKind::ClearExtState
        | EffectKind::SyntaxSelectionPush
        | EffectKind::SyntaxSelectionPop
        | EffectKind::SyntaxHistoryClear
        | EffectKind::SetSyntaxSelections
        | EffectKind::SetSubstituteConfirmState
        | EffectKind::ClearSubstituteConfirmState
        | EffectKind::Noop => None,

        // ── Passthrough (always delivered if present) ────────────────────
        EffectKind::HostAction => None,
        EffectKind::Event => None,
        EffectKind::UndoTreeSnapshot => None,
        EffectKind::NormCommand => None,
        EffectKind::OperatorFilter => None,
        EffectKind::OperatorReindent => None,

        // ── Standard passthrough ─────────────────────────────────────────
        EffectKind::SetHighlightRange | EffectKind::ClearHighlightRange => None,

        EffectKind::HighlightRows
        | EffectKind::SetBlockSelections
        | EffectKind::SaveSelections
        | EffectKind::RestoreSelections => Some(HostCapability::CursorMovement),

        EffectKind::SelectNextMatch | EffectKind::SelectPreviousMatch => {
            Some(HostCapability::SearchHighlight)
        }

        EffectKind::SubstituteConfirmShow | EffectKind::SubstituteConfirmEnd => {
            Some(HostCapability::StatusMessages)
        }

        // ShowMatch: bracket-flash hint — always delivered (hosts may ignore)
        EffectKind::ShowMatch => None,

        // Variable store: internal, consumed by effect processor
        EffectKind::SetVariable | EffectKind::DeleteVariable => None,

        // Cross-buffer edit: requires MultiBuffer capability
        EffectKind::CrossBufferEdit => Some(HostCapability::MultiBuffer),

        // Atomic mode transition: no capability required (core)
        EffectKind::ModeTransition => None,

        // Timer request: always delivered (host manages timers)
        EffectKind::RequestTimer => None,
    }
}

/// Check whether a host with the given capabilities should receive this effect.
///
/// Returns `true` if the effect requires no capability (engine-internal or
/// unconditional) OR the host has the required capability.
#[inline]
#[must_use]
pub const fn should_deliver(kind: EffectKind, caps: HostCapabilitySet) -> bool {
    match required_capability(kind) {
        None => true,
        Some(cap) => caps.has(cap),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Window request interception — DeferredAction conversion helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert a window-related [`HostRequest`] to its corresponding [`WindowNavAction`].
///
/// Returns `None` for non-window requests. Used by `VimSession::handle_sync_requests`
/// to intercept window navigation before it reaches the host.
#[must_use]
const fn host_request_to_window_action(request: &HostRequest) -> Option<WindowNavAction> {
    match request {
        HostRequest::WindowMoveLeft { .. } => Some(WindowNavAction::MoveLeft),
        HostRequest::WindowMoveRight { .. } => Some(WindowNavAction::MoveRight),
        HostRequest::WindowMoveUp { .. } => Some(WindowNavAction::MoveUp),
        HostRequest::WindowMoveDown { .. } => Some(WindowNavAction::MoveDown),
        HostRequest::WindowNext { .. } => Some(WindowNavAction::CycleNext),
        HostRequest::WindowPrev { .. } => Some(WindowNavAction::CyclePrev),
        HostRequest::CloseWindow { .. } => Some(WindowNavAction::Close),
        HostRequest::SplitWindow {
            direction,
            new_file,
            ..
        } => {
            if *new_file {
                Some(WindowNavAction::New)
            } else {
                match direction {
                    SplitDirection::Horizontal => Some(WindowNavAction::Split),
                    SplitDirection::Vertical => Some(WindowNavAction::VSplit),
                }
            }
        }
        HostRequest::CloseOtherWindows { .. } => Some(WindowNavAction::Only),
        HostRequest::WindowEqualSize { .. } => Some(WindowNavAction::EqualSize),
        HostRequest::WindowRotateDown { .. } => Some(WindowNavAction::RotateDown),
        HostRequest::WindowRotateUp { .. } => Some(WindowNavAction::RotateUp),
        HostRequest::WindowIncreaseHeight { count, .. } => {
            Some(WindowNavAction::IncreaseHeight { count: *count })
        }
        HostRequest::WindowDecreaseHeight { count, .. } => {
            Some(WindowNavAction::DecreaseHeight { count: *count })
        }
        HostRequest::WindowIncreaseWidth { count, .. } => {
            Some(WindowNavAction::IncreaseWidth { count: *count })
        }
        HostRequest::WindowDecreaseWidth { count, .. } => {
            Some(WindowNavAction::DecreaseWidth { count: *count })
        }
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper functions — convenience utilities for hosts
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert byte offset to Position using linear memchr scan.
///
/// Convenience for simple hosts implementing `Document::offset_to_pos`.
/// Returns None if offset > text.len().
#[must_use]
pub fn simple_offset_to_pos(text: &str, offset: usize) -> Option<crate::primitives::Position> {
    if offset > text.len() {
        return None;
    }
    let prefix = &text[..offset];
    let line = memchr::memchr_iter(b'\n', prefix.as_bytes()).count();
    let line_start = prefix.rfind('\n').map_or(0, |pos| pos + 1);
    let col = offset - line_start;
    Some(crate::primitives::Position::from_raw(line, col))
}

/// Convert Position to byte offset using linear memchr scan.
///
/// Convenience for simple hosts implementing `Document::pos_to_offset`.
/// Clamps column to line length (never returns None for valid lines).
/// Returns None only if the line number exceeds the document.
#[must_use]
pub fn simple_pos_to_offset(
    text: &str,
    pos: crate::primitives::Position,
) -> Option<crate::primitives::Offset> {
    let target_line = pos.line().get();
    let target_col = pos.col().get();
    let mut offset = 0;
    for _ in 0..target_line {
        offset = memchr::memchr(b'\n', text[offset..].as_bytes()).map(|i| offset + i + 1)?;
    }
    let line_end =
        memchr::memchr(b'\n', text[offset..].as_bytes()).map_or(text.len(), |i| offset + i);
    let line_len = line_end - offset;
    let col = target_col.min(line_len);
    Some(crate::primitives::Offset::new(offset + col))
}

/// Filter a slice of effects based on the host's capability set.
///
/// This is the standalone function for hosts that use `VimEngine` directly
/// instead of going through `VimSession`. It applies the same filtering
/// logic as `VimSession::deliver_effects()`.
///
/// Returns a `Vec<Effect>` containing only effects the host can handle.
/// Engine-internal effects (required_capability = None) are always included.
///
/// # Examples
///
/// ```ignore
/// let caps = HostCapabilitySet::CORE.with(HostCapability::Scrolling);
/// let response = engine.process(key, ctx);
/// let filtered = filter_effects_for_host(response.effects(), caps);
/// host.apply(filtered);
/// ```
#[must_use]
pub fn filter_effects_for_host(effects: &[Effect], caps: HostCapabilitySet) -> Vec<Effect> {
    effects
        .iter()
        .filter(|e| should_deliver(e.kind(), caps))
        .cloned()
        .collect()
}

/// Standalone utility for hosts using VimEngine directly.
///
/// Produces user-visible feedback (ShowInfo) when effects are suppressed
/// due to missing capabilities. Not called by VimSession (which uses the
/// `on_effect_suppressed` callback instead).
///
/// Some suppressed effects should be replaced with user-visible feedback
/// rather than silently dropped. This function returns `Some(fallback)` when
/// a meaningful replacement exists, `None` when silent suppression is correct.
///
/// # Fallback chains
///
/// - `FoldLine/UnfoldLine/...` → `ShowInfo("folds not supported")`
/// - `WindowSplit/WindowClose/...` → `ShowInfo("splits not supported")`
/// - `CopyToClipboard` → `ShowInfo("clipboard not available")`
/// - `SubstitutePreview` → `None` (silent — preview is advisory)
/// - `HighlightYank` → `None` (silent — flash is advisory)
#[must_use]
pub fn fallback_effect(kind: EffectKind, caps: HostCapabilitySet) -> Option<Effect> {
    // Only produce fallbacks if the host has StatusMessages capability.
    if !caps.has(HostCapability::StatusMessages) {
        return None;
    }

    match required_capability(kind)? {
        HostCapability::Folding => Some(Effect::show_message("folds not supported by this host")),
        HostCapability::WindowManagement => Some(Effect::show_message(
            "window splits not supported by this host",
        )),
        HostCapability::Clipboard => Some(Effect::show_message("clipboard not available")),
        HostCapability::Shell => Some(Effect::show_message("shell commands not available")),
        HostCapability::LspNavigation => Some(Effect::show_message(
            "LSP navigation not supported by this host",
        )),
        // Advisory effects — silent suppression is correct.
        HostCapability::YankHighlight
        | HostCapability::SubstitutePreview
        | HostCapability::VirtualText
        | HostCapability::CursorStyle
        | HostCapability::WhichKey => None,
        // Core capabilities — should never be missing in practice.
        HostCapability::TextMutation
        | HostCapability::CursorMovement
        | HostCapability::ModeTracking
        | HostCapability::UndoGrouping => None,
        // Standard/advanced capabilities — silent suppression.
        HostCapability::Scrolling
        | HostCapability::SearchHighlight
        | HostCapability::StatusMessages
        | HostCapability::Registers
        | HostCapability::CommandLine
        | HostCapability::FileSystem
        | HostCapability::MultiBuffer
        | HostCapability::Tabs
        | HostCapability::ExpressionEval
        | HostCapability::Completion
        | HostCapability::Reindent
        | HostCapability::CommandWindow
        | HostCapability::NativeInsert => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Examples
// ═══════════════════════════════════════════════════════════════════════════════

/// Example of how godot-vim would implement `VimHost` for native Rust integration.
///
/// ```ignore
/// use vim_core::document::Document;
/// use vim_core::execution::host_api::*;
/// use vim_core::effects::Effect;
/// use vim_core::execution::host::{HostRequest, HostResult};
/// use vim_core::primitives::{Offset, Position};
///
/// struct GodotHost {
///     // Godot's CodeEdit text (borrowed from the scene tree)
///     text: String,
///     cursor: usize,
/// }
///
/// impl Document for GodotHost {
///     fn text(&self) -> &str { &self.text }
///     fn line_count(&self) -> usize {
///         memchr::memchr_iter(b'\n', self.text.as_bytes()).count().max(1)
///     }
///     fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
///         // ... scan for newlines
///         # todo!()
///     }
///     fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
///         // ... scan for target line
///         # todo!()
///     }
/// }
///
/// impl VimHost for GodotHost {
///     fn capabilities(&self) -> HostCapabilitySet {
///         HostCapabilitySet::STANDARD
///             .with(HostCapability::Folding)
///             .with(HostCapability::Clipboard)
///             .with(HostCapability::LspNavigation)
///             .with(HostCapability::WindowManagement)
///     }
///
///     fn cursor_offset(&self) -> usize { self.cursor }
///
///     fn apply_effects(&mut self, effects: &[Effect]) {
///         for effect in effects {
///             match effect {
///                 Effect::Insert { offset, text } => {
///                     self.text.insert_str(offset.get(), text);
///                 }
///                 Effect::Delete { range } => {
///                     self.text.drain(range.start().get()..range.end().get());
///                 }
///                 Effect::SetCursor { offset } => {
///                     self.cursor = offset.get();
///                 }
///                 // ... handle other effects
///                 _ => {}
///             }
///         }
///     }
///
///     fn handle_request(&mut self, request: &HostRequest) -> RequestDisposition {
///         match request {
///             HostRequest::WriteFile { meta, path, force } => {
///                 // Godot: save via ResourceSaver
///                 RequestDisposition::Completed(HostResult::Success { id: meta.id, message: None })
///             }
///             HostRequest::Quit { meta, force } => {
///                 // Godot: queue_free() or signal editor close
///                 RequestDisposition::Completed(HostResult::Success { id: meta.id, message: None })
///             }
///             _ => RequestDisposition::Unsupported, // Engine applies fallback
///         }
///     }
/// }
///
/// // Usage:
/// let host = GodotHost { text: "hello".into(), cursor: 0 };
/// let mut session = VimSession::with_host(host);
/// let response = session.process_key(KeyEvent::char('x'));
/// ```
#[cfg(doc)]
mod godot_example {}

/// Example of how an ABI-boundary adapter would use `dyn VimHost`.
///
/// ```ignore
/// use vim_core::document::Document;
/// use vim_core::execution::host_api::*;
/// use vim_core::effects::Effect;
/// use vim_core::execution::host::{HostRequest, HostResult};
/// use vim_core::primitives::{Offset, Position};
///
/// /// FFI adapter that bridges C function pointers to VimHost.
/// struct FfiHost {
///     /// Pointer to the C-side document text buffer.
///     text_ptr: *const u8,
///     text_len: usize,
///     cursor: usize,
///     line_count: usize,
///     caps: HostCapabilitySet,
///     /// C callback for applying effects (encoded as TLV).
///     apply_fn: extern "C" fn(*const u8, usize),
///     /// C callback for handling requests (encoded as TLV).
///     request_fn: extern "C" fn(u64, *const u8, usize) -> i32,
/// }
///
/// impl Document for FfiHost {
///     fn text(&self) -> &str {
///         unsafe {
///             std::str::from_utf8_unchecked(
///                 std::slice::from_raw_parts(self.text_ptr, self.text_len)
///             )
///         }
///     }
///     fn line_count(&self) -> usize { self.line_count }
///     fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
///         // ... scan for newlines
///         # todo!()
///     }
///     fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
///         // ... scan for target line
///         # todo!()
///     }
/// }
///
/// impl VimHost for FfiHost {
///     fn capabilities(&self) -> HostCapabilitySet { self.caps }
///     fn cursor_offset(&self) -> usize { self.cursor }
///
///     fn apply_effects(&mut self, effects: &[Effect]) {
///         // Encode effects as TLV and call the C callback
///         let encoded = encode_effects_tlv(effects);
///         (self.apply_fn)(encoded.as_ptr(), encoded.len());
///     }
///
///     fn handle_request(&mut self, request: &HostRequest) -> RequestDisposition {
///         // All requests are async in FFI — host completes later
///         RequestDisposition::Deferred
///     }
/// }
///
/// // Usage: wrap in a concrete adapter, not Box<dyn VimHost>
/// struct FfiAdapter { /* fields */ }
/// impl Document for FfiAdapter { /* ... */ }
/// impl VimHost for FfiAdapter { /* ... */ }
/// let session = VimSession::with_host(FfiAdapter { /* ... */ });
/// ```
#[cfg(doc)]
mod ffi_example {}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── HostCapabilitySet tests ──────────────────────────────────────────────

    #[test]
    fn core_has_four_capabilities() {
        let core = HostCapabilitySet::CORE;
        assert_eq!(core.count(), 4);
        assert!(core.has(HostCapability::TextMutation));
        assert!(core.has(HostCapability::CursorMovement));
        assert!(core.has(HostCapability::ModeTracking));
        assert!(core.has(HostCapability::UndoGrouping));
    }

    #[test]
    fn core_does_not_have_scrolling() {
        assert!(!HostCapabilitySet::CORE.has(HostCapability::Scrolling));
    }

    #[test]
    fn with_adds_capability() {
        let caps = HostCapabilitySet::CORE.with(HostCapability::Folding);
        assert!(caps.has(HostCapability::Folding));
        assert!(caps.has(HostCapability::TextMutation)); // core preserved
    }

    #[test]
    fn without_removes_capability() {
        let caps = HostCapabilitySet::STANDARD.without(HostCapability::Scrolling);
        assert!(!caps.has(HostCapability::Scrolling));
        assert!(caps.has(HostCapability::StatusMessages)); // others preserved
    }

    #[test]
    fn implications_applied() {
        let caps = HostCapabilitySet::CORE
            .with(HostCapability::WindowManagement)
            .with_implications();
        assert!(caps.has(HostCapability::Scrolling)); // implied by WindowManagement
    }

    #[test]
    fn substitute_preview_implies_search_highlight() {
        let caps = HostCapabilitySet::CORE
            .with(HostCapability::SubstitutePreview)
            .with_implications();
        assert!(caps.has(HostCapability::SearchHighlight));
    }

    #[test]
    fn union_combines_sets() {
        let a = HostCapabilitySet::CORE.with(HostCapability::Folding);
        let b = HostCapabilitySet::CORE.with(HostCapability::Clipboard);
        let c = a.union(b);
        assert!(c.has(HostCapability::Folding));
        assert!(c.has(HostCapability::Clipboard));
    }

    #[test]
    fn bitor_same_as_union() {
        let a = HostCapabilitySet::CORE.with(HostCapability::Folding);
        let b = HostCapabilitySet::CORE.with(HostCapability::Clipboard);
        assert_eq!(a | b, a.union(b));
    }

    #[test]
    fn full_contains_everything() {
        for cap in [
            HostCapability::TextMutation,
            HostCapability::Folding,
            HostCapability::WindowManagement,
            HostCapability::CommandWindow,
        ] {
            assert!(HostCapabilitySet::FULL.has(cap));
        }
    }

    #[test]
    fn full_has_exactly_27_capabilities() {
        assert_eq!(
            HostCapabilitySet::FULL.count(),
            27,
            "FULL should have exactly 27 capabilities"
        );
    }

    #[test]
    fn round_trip_through_bits() {
        let caps = HostCapabilitySet::STANDARD.with(HostCapability::Folding);
        let bits = caps.bits();
        let restored = HostCapabilitySet::from_bits(bits);
        assert_eq!(caps, restored);
    }

    #[test]
    fn contains_all_checks_superset() {
        assert!(HostCapabilitySet::STANDARD.contains_all(HostCapabilitySet::CORE));
        assert!(!HostCapabilitySet::CORE.contains_all(HostCapabilitySet::STANDARD));
    }

    // ── Effect metadata tests ────────────────────────────────────────────

    #[test]
    fn core_effects_require_core_capabilities() {
        assert_eq!(
            required_capability(EffectKind::Insert),
            Some(HostCapability::TextMutation)
        );
        assert_eq!(
            required_capability(EffectKind::SetCursor),
            Some(HostCapability::CursorMovement)
        );
        assert_eq!(
            required_capability(EffectKind::SetMode),
            Some(HostCapability::ModeTracking)
        );
        assert_eq!(
            required_capability(EffectKind::BeginUndoGroup),
            Some(HostCapability::UndoGrouping)
        );
    }

    #[test]
    fn fold_effects_require_folding() {
        assert_eq!(
            required_capability(EffectKind::FoldLine),
            Some(HostCapability::Folding)
        );
        assert_eq!(
            required_capability(EffectKind::UnfoldAll),
            Some(HostCapability::Folding)
        );
    }

    #[test]
    fn window_effects_require_window_management() {
        assert_eq!(
            required_capability(EffectKind::WindowSplit),
            Some(HostCapability::WindowManagement)
        );
    }

    #[test]
    fn internal_effects_have_no_requirement() {
        assert_eq!(required_capability(EffectKind::Noop), None);
        assert_eq!(required_capability(EffectKind::SetExtState), None);
        assert_eq!(required_capability(EffectKind::SyntaxSelectionPush), None);
    }

    #[test]
    fn should_deliver_respects_capabilities() {
        let core_only = HostCapabilitySet::CORE;
        // Core effects always delivered
        assert!(should_deliver(EffectKind::Insert, core_only));
        assert!(should_deliver(EffectKind::SetCursor, core_only));
        // Non-core effects NOT delivered
        assert!(!should_deliver(EffectKind::FoldLine, core_only));
        assert!(!should_deliver(EffectKind::WindowSplit, core_only));
        // Internal effects always delivered
        assert!(should_deliver(EffectKind::Noop, core_only));
    }

    #[test]
    fn show_error_unconditionally_delivered() {
        assert_eq!(required_capability(EffectKind::ShowError), None);
        assert!(should_deliver(
            EffectKind::ShowError,
            HostCapabilitySet::CORE
        ));
        assert!(should_deliver(
            EffectKind::ShowError,
            HostCapabilitySet::EMPTY
        ));
    }

    #[test]
    fn every_effect_kind_is_mapped() {
        // Verify that required_capability is total: every EffectKind::ALL
        // variant can be called without panic.
        for kind in EffectKind::ALL {
            let _ = required_capability(kind);
        }
    }

    // ── Filter tests ─────────────────────────────────────────────────────

    #[test]
    fn filter_removes_unsupported_effects() {
        let effects = vec![
            Effect::insert(Offset::new(0), "hello"),
            Effect::show_message("test"),
            Effect::set_cursor(Offset::new(5)),
        ];
        let core_only = HostCapabilitySet::CORE;
        let filtered = filter_effects_for_host(&effects, core_only);
        // Insert and SetCursor pass, ShowInfo does not (requires StatusMessages)
        assert_eq!(filtered.len(), 2);
        assert!(matches!(filtered[0], Effect::Insert { .. }));
        assert!(matches!(filtered[1], Effect::SetCursor { .. }));
    }

    #[test]
    fn filter_with_standard_passes_messages() {
        let effects = vec![
            Effect::insert(Offset::new(0), "hello"),
            Effect::show_message("test"),
        ];
        let filtered = filter_effects_for_host(&effects, HostCapabilitySet::STANDARD);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filter_full_passes_everything() {
        let effects = vec![
            Effect::insert(Offset::new(0), "hello"),
            Effect::show_message("test"),
            Effect::FoldAll,
            Effect::WindowSplit,
        ];
        let filtered = filter_effects_for_host(&effects, HostCapabilitySet::FULL);
        assert_eq!(filtered.len(), effects.len());
    }

    // ── Fallback tests ───────────────────────────────────────────────────

    #[test]
    fn fold_fallback_shows_message() {
        let caps = HostCapabilitySet::CORE.with(HostCapability::StatusMessages);
        let fb = fallback_effect(EffectKind::FoldLine, caps);
        assert!(fb.is_some());
        assert!(matches!(fb.unwrap(), Effect::ShowInfo { .. }));
    }

    #[test]
    fn highlight_range_fallback_is_silent() {
        let caps = HostCapabilitySet::CORE.with(HostCapability::StatusMessages);
        let fb = fallback_effect(EffectKind::SetHighlightRange, caps);
        assert!(fb.is_none());
    }

    #[test]
    fn no_fallback_without_status_messages() {
        let caps = HostCapabilitySet::CORE; // No StatusMessages
        let fb = fallback_effect(EffectKind::FoldLine, caps);
        assert!(fb.is_none());
    }

    // ── VimSession integration test ──────────────────────────────────────

    #[test]
    fn vim_session_processes_key() {
        use crate::primitives::Mode;
        use std::cell::RefCell;

        struct InMemoryHost {
            text: String,
            cursor: usize,
            mode: Mode,
            effects_received: RefCell<Vec<EffectKind>>,
        }

        impl InMemoryHost {
            fn new(text: &str) -> Self {
                Self {
                    text: text.to_string(),
                    cursor: 0,
                    mode: Mode::Normal,
                    effects_received: RefCell::new(Vec::new()),
                }
            }
        }

        impl crate::document::Document for InMemoryHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                let off = offset.get();
                if off > self.text.len() {
                    return None;
                }
                let prefix = &self.text[..off];
                let line = memchr::memchr_iter(b'\n', prefix.as_bytes()).count();
                let line_start = prefix.rfind('\n').map_or(0, |pos| pos + 1);
                let col = off - line_start;
                Some(crate::primitives::Position::from_raw(line, col))
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                let text = &self.text;
                let target_line = pos.line().get();
                let target_col = pos.col().get();
                let mut offset = 0;
                for _ in 0..target_line {
                    offset =
                        memchr::memchr(b'\n', text[offset..].as_bytes()).map(|i| offset + i + 1)?;
                }
                let line_end = memchr::memchr(b'\n', text[offset..].as_bytes())
                    .map(|i| offset + i)
                    .unwrap_or(text.len());
                let line_len = line_end - offset;
                let col = target_col.min(line_len);
                Some(crate::primitives::Offset::new(offset + col))
            }
        }

        impl VimHost for InMemoryHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::STANDARD
            }

            fn cursor_offset(&self) -> usize {
                self.cursor
            }

            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    self.effects_received.borrow_mut().push(effect.kind());

                    match effect {
                        Effect::SetCursor { offset } => {
                            self.cursor = offset.get();
                        }
                        Effect::SetMode { mode, .. } => {
                            self.mode = *mode;
                        }
                        Effect::Insert { offset, text } => {
                            self.text.insert_str(offset.get(), text);
                        }
                        Effect::Delete { range } => {
                            self.text.drain(range.start().get()..range.end().get());
                        }
                        _ => {}
                    }
                }
            }

            fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
                RequestDisposition::Deferred
            }
        }

        let host = InMemoryHost::new("hello");
        let mut session = VimSession::with_host(host);

        // Press 'l' — should move cursor right
        let _result = session.process_key(KeyEvent::char('l'));
        assert_eq!(session.host().cursor, 1);

        // The host should have received a SetCursor effect
        let kinds = session.host().effects_received.borrow();
        assert!(
            kinds.contains(&EffectKind::SetCursor),
            "expected SetCursor in {:?}",
            &*kinds,
        );
    }

    // ── Window nav deferred action tests ──────────────────────────────────

    #[test]
    fn window_nav_effects_become_deferred_actions() {
        struct WindowNavHost {
            text: String,
            cursor: usize,
        }

        impl WindowNavHost {
            fn new(text: &str) -> Self {
                Self {
                    text: text.to_string(),
                    cursor: 0,
                }
            }
        }

        impl crate::document::Document for WindowNavHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                simple_offset_to_pos(&self.text, offset.get())
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                simple_pos_to_offset(&self.text, pos)
            }
        }

        impl VimHost for WindowNavHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::FULL
            }
            fn cursor_offset(&self) -> usize {
                self.cursor
            }
            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    match effect {
                        Effect::SetCursor { offset } => {
                            self.cursor = offset.get();
                        }
                        Effect::Insert { offset, text } => {
                            self.text.insert_str(offset.get(), text);
                        }
                        Effect::Delete { range } => {
                            self.text.drain(range.start().get()..range.end().get());
                        }
                        _ => {}
                    }
                }
            }
            fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
                RequestDisposition::Deferred
            }
        }

        let host = WindowNavHost::new("hello\nworld");
        let mut session = VimSession::with_host(host);

        // Ctrl-W h = window move left: first key starts the Ctrl-W prefix
        let result = session.process_key(KeyEvent::ctrl('w'));
        assert!(
            result.deferred_actions.is_empty(),
            "Ctrl-W alone should not produce deferred actions"
        );

        // Second key 'h' completes the window command
        let result = session.process_key(KeyEvent::char('h'));
        assert!(
            result
                .deferred_actions
                .contains(&DeferredAction::WindowNav(WindowNavAction::MoveLeft)),
            "Ctrl-W h should produce WindowNav(MoveLeft), got: {:?}",
            result.deferred_actions,
        );
    }

    #[test]
    fn window_effects_not_delivered_to_host() {
        use std::cell::RefCell;

        struct TrackingHost {
            text: String,
            cursor: usize,
            effects_received: RefCell<Vec<EffectKind>>,
        }

        impl TrackingHost {
            fn new(text: &str) -> Self {
                Self {
                    text: text.to_string(),
                    cursor: 0,
                    effects_received: RefCell::new(Vec::new()),
                }
            }
        }

        impl crate::document::Document for TrackingHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                simple_offset_to_pos(&self.text, offset.get())
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                simple_pos_to_offset(&self.text, pos)
            }
        }

        impl VimHost for TrackingHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::FULL
            }
            fn cursor_offset(&self) -> usize {
                self.cursor
            }
            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    self.effects_received.borrow_mut().push(effect.kind());
                    match effect {
                        Effect::SetCursor { offset } => self.cursor = offset.get(),
                        Effect::Insert { offset, text } => {
                            self.text.insert_str(offset.get(), text);
                        }
                        Effect::Delete { range } => {
                            self.text.drain(range.start().get()..range.end().get());
                        }
                        _ => {}
                    }
                }
            }
            fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
                RequestDisposition::Deferred
            }
        }

        let host = TrackingHost::new("hello\nworld");
        let mut session = VimSession::with_host(host);

        // Ctrl-W h
        let _ = session.process_key(KeyEvent::ctrl('w'));
        let _ = session.process_key(KeyEvent::char('h'));

        // WindowMoveLeft should NOT appear in effects delivered to the host
        let kinds = session.host().effects_received.borrow();
        assert!(
            !kinds.contains(&EffectKind::WindowMoveLeft),
            "WindowMoveLeft should NOT be delivered to host, got: {:?}",
            &*kinds,
        );
    }

    #[test]
    fn host_request_to_window_action_covers_all_window_variants() {
        use crate::execution::host::HostRequestMeta;
        let meta = || HostRequestMeta {
            id: crate::execution::host::HostRequestId::new(1),
        };

        let window_requests: Vec<(HostRequest, WindowNavAction)> = vec![
            (
                HostRequest::WindowMoveLeft { meta: meta() },
                WindowNavAction::MoveLeft,
            ),
            (
                HostRequest::WindowMoveRight { meta: meta() },
                WindowNavAction::MoveRight,
            ),
            (
                HostRequest::WindowMoveUp { meta: meta() },
                WindowNavAction::MoveUp,
            ),
            (
                HostRequest::WindowMoveDown { meta: meta() },
                WindowNavAction::MoveDown,
            ),
            (
                HostRequest::WindowNext { meta: meta() },
                WindowNavAction::CycleNext,
            ),
            (
                HostRequest::WindowPrev { meta: meta() },
                WindowNavAction::CyclePrev,
            ),
            (
                HostRequest::CloseWindow {
                    meta: meta(),
                    force: false,
                },
                WindowNavAction::Close,
            ),
            (
                HostRequest::SplitWindow {
                    meta: meta(),
                    direction: SplitDirection::Horizontal,
                    path: None,
                    new_file: false,
                },
                WindowNavAction::Split,
            ),
            (
                HostRequest::SplitWindow {
                    meta: meta(),
                    direction: SplitDirection::Vertical,
                    path: None,
                    new_file: false,
                },
                WindowNavAction::VSplit,
            ),
            (
                HostRequest::SplitWindow {
                    meta: meta(),
                    direction: SplitDirection::Horizontal,
                    path: None,
                    new_file: true,
                },
                WindowNavAction::New,
            ),
            (
                HostRequest::CloseOtherWindows {
                    meta: meta(),
                    force: false,
                },
                WindowNavAction::Only,
            ),
            (
                HostRequest::WindowEqualSize { meta: meta() },
                WindowNavAction::EqualSize,
            ),
            (
                HostRequest::WindowRotateDown { meta: meta() },
                WindowNavAction::RotateDown,
            ),
            (
                HostRequest::WindowRotateUp { meta: meta() },
                WindowNavAction::RotateUp,
            ),
            (
                HostRequest::WindowIncreaseHeight {
                    meta: meta(),
                    count: 3,
                },
                WindowNavAction::IncreaseHeight { count: 3 },
            ),
            (
                HostRequest::WindowDecreaseHeight {
                    meta: meta(),
                    count: 2,
                },
                WindowNavAction::DecreaseHeight { count: 2 },
            ),
            (
                HostRequest::WindowIncreaseWidth {
                    meta: meta(),
                    count: 5,
                },
                WindowNavAction::IncreaseWidth { count: 5 },
            ),
            (
                HostRequest::WindowDecreaseWidth {
                    meta: meta(),
                    count: 1,
                },
                WindowNavAction::DecreaseWidth { count: 1 },
            ),
        ];

        for (request, expected) in &window_requests {
            let action = host_request_to_window_action(request);
            assert_eq!(
                action.as_ref(),
                Some(expected),
                "{request:?} should produce {expected:?}",
            );
        }

        // Non-window request should return None
        let non_window = HostRequest::GotoDefinition { meta: meta() };
        assert!(
            host_request_to_window_action(&non_window).is_none(),
            "GotoDefinition should NOT be a window action",
        );
    }

    #[test]
    fn host_request_to_window_action_preserves_count() {
        use crate::execution::host::HostRequestMeta;
        let meta = HostRequestMeta {
            id: crate::execution::host::HostRequestId::new(1),
        };

        assert_eq!(
            host_request_to_window_action(&HostRequest::WindowIncreaseHeight {
                meta: meta.clone(),
                count: 7
            }),
            Some(WindowNavAction::IncreaseHeight { count: 7 }),
        );
        assert_eq!(
            host_request_to_window_action(&HostRequest::WindowDecreaseWidth { meta, count: 3 }),
            Some(WindowNavAction::DecreaseWidth { count: 3 }),
        );
    }

    // ── simple_offset_to_pos / simple_pos_to_offset tests ───────────────

    #[test]
    fn simple_offset_to_pos_works() {
        let text = "hello\nworld\nfoo";
        let pos = simple_offset_to_pos(text, 6).unwrap();
        assert_eq!(pos.line().get(), 1);
        assert_eq!(pos.col().get(), 0);
        let pos = simple_offset_to_pos(text, 0).unwrap();
        assert_eq!(pos.line().get(), 0);
        assert_eq!(pos.col().get(), 0);
        assert!(simple_offset_to_pos(text, 100).is_none());
    }

    #[test]
    fn simple_pos_to_offset_clamps_column() {
        let text = "hi\nworld";
        let off = simple_pos_to_offset(text, crate::primitives::Position::from_raw(0, 99)).unwrap();
        assert_eq!(off.get(), 2);
    }

    // ── RequestDisposition::Unsupported fallback test ────────────────────

    #[test]
    fn unsupported_disposition_applies_default_fallback_without_pending() {
        use std::cell::RefCell;

        /// A host that returns `Unsupported` for ALL host requests.
        /// This exercises the fallback path in `handle_sync_requests`.
        struct UnsupportedHost {
            text: String,
            cursor: usize,
            requests_seen: RefCell<Vec<crate::execution::host::HostRequestKind>>,
        }

        impl UnsupportedHost {
            fn new(text: &str) -> Self {
                Self {
                    text: text.to_string(),
                    cursor: 0,
                    requests_seen: RefCell::new(Vec::new()),
                }
            }
        }

        impl crate::document::Document for UnsupportedHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                simple_offset_to_pos(&self.text, offset.get())
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                simple_pos_to_offset(&self.text, pos)
            }
        }

        impl VimHost for UnsupportedHost {
            fn capabilities(&self) -> HostCapabilitySet {
                // FULL so no effects are suppressed — all requests are generated.
                HostCapabilitySet::FULL
            }

            fn cursor_offset(&self) -> usize {
                self.cursor
            }

            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    match effect {
                        Effect::SetCursor { offset } => {
                            self.cursor = offset.get();
                        }
                        Effect::Insert { offset, text } => {
                            self.text.insert_str(offset.get(), text);
                        }
                        Effect::Delete { range } => {
                            self.text.drain(range.start().get()..range.end().get());
                        }
                        _ => {}
                    }
                }
            }

            fn handle_request(&mut self, request: &HostRequest) -> RequestDisposition {
                // Record that we saw the request, then return Unsupported.
                self.requests_seen.borrow_mut().push(request.kind());
                RequestDisposition::Unsupported
            }
        }

        let host = UnsupportedHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // Press 'g' then 'd' — produces HostRequest::GotoDefinition.
        // GotoDefinition has a default_result() of Some(Success), so the
        // Unsupported path should apply the fallback immediately.
        let _ = session.process_key(KeyEvent::char('g'));
        let result = session.process_key(KeyEvent::char('d'));

        // 1. The host saw the GotoDefinition request (proves it reached handle_request).
        let seen = session.host().requests_seen.borrow();
        assert!(
            seen.contains(&crate::execution::host::HostRequestKind::GotoDefinition),
            "host should have been asked to handle GotoDefinition, saw: {:?}",
            &*seen,
        );
        drop(seen);

        // 2. No crash, no hang — we reached this point.

        // 3. The request does NOT appear in the async pending queue.
        //    When Unsupported has a fallback, it's completed immediately via
        //    engine.complete_host_request(), so it never reaches async_collector.
        assert!(
            result.host_requests.is_empty(),
            "Unsupported request with fallback should NOT be in pending queue, got: {:?}",
            result.host_requests,
        );
    }

    // ── cancel_request / pending_request_count tests ─────────────────────

    /// Minimal host that defers all requests (they stay pending).
    struct DeferringHost {
        text: String,
        cursor: usize,
    }

    impl DeferringHost {
        fn new(text: &str) -> Self {
            Self {
                text: text.to_string(),
                cursor: 0,
            }
        }
    }

    impl crate::document::Document for DeferringHost {
        fn text(&self) -> &str {
            &self.text
        }
        fn line_count(&self) -> usize {
            memchr::memchr_iter(b'\n', self.text.as_bytes())
                .count()
                .max(1)
        }
        fn offset_to_pos(
            &self,
            offset: crate::primitives::Offset,
        ) -> Option<crate::primitives::Position> {
            simple_offset_to_pos(&self.text, offset.get())
        }
        fn pos_to_offset(
            &self,
            pos: crate::primitives::Position,
        ) -> Option<crate::primitives::Offset> {
            simple_pos_to_offset(&self.text, pos)
        }
    }

    impl VimHost for DeferringHost {
        fn capabilities(&self) -> HostCapabilitySet {
            HostCapabilitySet::FULL
        }
        fn cursor_offset(&self) -> usize {
            self.cursor
        }
        fn apply_effects(&mut self, effects: &[Effect]) {
            for effect in effects {
                match effect {
                    Effect::SetCursor { offset } => self.cursor = offset.get(),
                    Effect::Insert { offset, text } => {
                        self.text.insert_str(offset.get(), text);
                    }
                    Effect::Delete { range } => {
                        self.text.drain(range.start().get()..range.end().get());
                    }
                    _ => {}
                }
            }
        }
        fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
            RequestDisposition::Deferred
        }
    }

    #[test]
    fn cancel_request_removes_from_pending() {
        let host = DeferringHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // `gd` produces GotoDefinition, which DeferringHost defers.
        let _ = session.process_key(KeyEvent::char('g'));
        let result = session.process_key(KeyEvent::char('d'));

        // Should have exactly one deferred request.
        assert_eq!(result.host_requests.len(), 1);
        let request_id = result.host_requests[0].id();
        assert_eq!(session.pending_request_count(), 1);

        // Cancel it.
        let cancelled = session.cancel_request(request_id);
        assert!(
            cancelled,
            "cancel_request should return true for pending ID"
        );
        assert_eq!(session.pending_request_count(), 0);
    }

    #[test]
    fn cancel_nonexistent_request_returns_false() {
        let host = DeferringHost::new("hello");
        let mut session = VimSession::with_host(host);

        let fake_id = crate::execution::host::HostRequestId::new(9999);
        let cancelled = session.cancel_request(fake_id);
        assert!(
            !cancelled,
            "cancel_request should return false for unknown ID"
        );
    }

    #[test]
    fn cancel_request_applies_fallback() {
        let host = DeferringHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // `gd` produces GotoDefinition. Its default_result is
        // Some(HostResult::Success { .. }) — a fire-and-forget no-op.
        let _ = session.process_key(KeyEvent::char('g'));
        let result = session.process_key(KeyEvent::char('d'));

        assert_eq!(result.host_requests.len(), 1);
        let request_id = result.host_requests[0].id();

        // Verify it is pending.
        assert_eq!(session.pending_request_count(), 1);

        // Cancel: should apply the default fallback (Success no-op).
        let cancelled = session.cancel_request(request_id);
        assert!(cancelled);

        // After cancellation, the request is no longer pending.
        assert_eq!(session.pending_request_count(), 0);

        // The engine should NOT have the request in pending anymore —
        // double-cancel returns false.
        let double_cancel = session.cancel_request(request_id);
        assert!(!double_cancel);
    }

    // ── Adversarial cancel_request tests ────────────────────────────────

    #[test]
    fn cancel_same_request_twice_second_returns_false() {
        let host = DeferringHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // Trigger a deferred request via `gd` (GotoDefinition).
        let _ = session.process_key(KeyEvent::char('g'));
        let result = session.process_key(KeyEvent::char('d'));

        assert_eq!(result.host_requests.len(), 1);
        let request_id = result.host_requests[0].id();
        assert_eq!(session.pending_request_count(), 1);

        // First cancel succeeds.
        assert!(session.cancel_request(request_id));
        assert_eq!(session.pending_request_count(), 0);

        // Second cancel of the same ID returns false — idempotent, no panic.
        assert!(!session.cancel_request(request_id));
        assert_eq!(session.pending_request_count(), 0);
    }

    #[test]
    fn cancel_then_complete_handles_gracefully() {
        let host = DeferringHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // Trigger a deferred request via `gd` (GotoDefinition).
        let _ = session.process_key(KeyEvent::char('g'));
        let result = session.process_key(KeyEvent::char('d'));

        assert_eq!(result.host_requests.len(), 1);
        let request_id = result.host_requests[0].id();

        // Cancel the request first.
        assert!(session.cancel_request(request_id));
        assert_eq!(session.pending_request_count(), 0);

        // Now try to complete the already-cancelled request.
        // The engine should handle this gracefully (return ignored response).
        let completion_result = session.complete_request(&HostResult::Success {
            id: request_id,
            message: None,
        });

        // Should not panic, should not produce meaningful output.
        // The request was already removed, so complete_request sees an unknown ID.
        // It returns a ProcessResult with consumed=false and no requests.
        assert!(!completion_result.consumed);
        assert!(completion_result.host_requests.is_empty());
    }

    #[test]
    fn cancel_all_pending_requests_in_loop() {
        let host = DeferringHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // Trigger first deferred request: `gd` (GotoDefinition).
        let _ = session.process_key(KeyEvent::char('g'));
        let result1 = session.process_key(KeyEvent::char('d'));
        assert_eq!(result1.host_requests.len(), 1);
        let id1 = result1.host_requests[0].id();

        // Trigger second deferred request: `K` (ShowDocumentation / KeywordLookup).
        let result2 = session.process_key(KeyEvent::char('K'));
        assert_eq!(result2.host_requests.len(), 1);
        let id2 = result2.host_requests[0].id();

        // Both should be pending.
        assert_eq!(session.pending_request_count(), 2);

        // Cancel all using pending_request_count() as the loop guard.
        let ids = [id1, id2];
        let mut cancelled_count = 0;
        for id in &ids {
            if session.pending_request_count() == 0 {
                break;
            }
            if session.cancel_request(*id) {
                cancelled_count += 1;
            }
        }

        assert_eq!(cancelled_count, 2);
        assert_eq!(session.pending_request_count(), 0);

        // All IDs should now be invalid — re-cancel returns false.
        for id in &ids {
            assert!(!session.cancel_request(*id));
        }
    }

    #[test]
    fn cancel_one_of_multiple_pending_others_remain() {
        let host = DeferringHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // Trigger first deferred request: `gd` (GotoDefinition).
        let _ = session.process_key(KeyEvent::char('g'));
        let result1 = session.process_key(KeyEvent::char('d'));
        assert_eq!(result1.host_requests.len(), 1);
        let id1 = result1.host_requests[0].id();

        // Trigger second deferred request: `K` (ShowDocumentation / KeywordLookup).
        let result2 = session.process_key(KeyEvent::char('K'));
        assert_eq!(result2.host_requests.len(), 1);
        let id2 = result2.host_requests[0].id();

        // Both should be pending.
        assert_eq!(session.pending_request_count(), 2);

        // Cancel only the first request.
        assert!(session.cancel_request(id1));

        // One should remain.
        assert_eq!(session.pending_request_count(), 1);

        // The cancelled one can't be cancelled again.
        assert!(!session.cancel_request(id1));

        // The other can still be completed normally.
        let _completion = session.complete_request(&HostResult::Success {
            id: id2,
            message: None,
        });
        // After completing id2, it should be removed from pending.
        assert_eq!(session.pending_request_count(), 0);

        // Attempting to complete id2 again should be a no-op (already removed).
        let stale = session.complete_request(&HostResult::Success {
            id: id2,
            message: None,
        });
        assert!(!stale.consumed);
    }

    // ── Runtime capability upgrade/downgrade tests ───────────────────────

    /// Minimal host with configurable capabilities (no-op for effects/requests).
    struct CapHost(HostCapabilitySet);

    impl crate::document::Document for CapHost {
        fn text(&self) -> &str {
            ""
        }
        fn line_count(&self) -> usize {
            1
        }
        fn offset_to_pos(
            &self,
            _offset: crate::primitives::Offset,
        ) -> Option<crate::primitives::Position> {
            Some(crate::primitives::Position::from_raw(0, 0))
        }
        fn pos_to_offset(
            &self,
            _pos: crate::primitives::Position,
        ) -> Option<crate::primitives::Offset> {
            Some(crate::primitives::Offset::new(0))
        }
    }

    impl VimHost for CapHost {
        fn capabilities(&self) -> HostCapabilitySet {
            self.0
        }
        fn cursor_offset(&self) -> usize {
            0
        }
        fn apply_effects(&mut self, _effects: &[Effect]) {}
        fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
            RequestDisposition::Unsupported
        }
    }

    #[test]
    fn upgrade_capability_adds_to_active_set() {
        let host = CapHost(HostCapabilitySet::CORE);
        let mut session = VimSession::with_host(host);
        assert!(!session.capabilities().has(HostCapability::Folding));

        session.upgrade_capability(HostCapability::Folding);
        assert!(session.capabilities().has(HostCapability::Folding));
    }

    #[test]
    fn downgrade_capability_removes_from_active_set() {
        let host = CapHost(HostCapabilitySet::STANDARD);
        let mut session = VimSession::with_host(host);
        assert!(session.capabilities().has(HostCapability::Scrolling));

        session.downgrade_capability(HostCapability::Scrolling);
        assert!(!session.capabilities().has(HostCapability::Scrolling));
    }

    #[test]
    fn upgrade_applies_implications() {
        let host = CapHost(HostCapabilitySet::CORE);
        let mut session = VimSession::with_host(host);
        assert!(!session.capabilities().has(HostCapability::Scrolling));

        session.upgrade_capability(HostCapability::WindowManagement);
        // WindowManagement implies Scrolling
        assert!(session.capabilities().has(HostCapability::Scrolling));
        assert!(session.capabilities().has(HostCapability::WindowManagement));
    }

    #[test]
    fn downgrade_does_not_remove_implications() {
        // Start with both WindowManagement and Scrolling explicitly
        let host = CapHost(
            HostCapabilitySet::CORE
                .with(HostCapability::WindowManagement)
                .with(HostCapability::Scrolling),
        );
        let mut session = VimSession::with_host(host);
        assert!(session.capabilities().has(HostCapability::Scrolling));

        // Downgrade WindowManagement — Scrolling should remain because it was
        // independently declared
        session.downgrade_capability(HostCapability::WindowManagement);
        assert!(!session.capabilities().has(HostCapability::WindowManagement));
        assert!(session.capabilities().has(HostCapability::Scrolling));
    }

    #[test]
    fn upgrade_downgrade_affects_effect_filtering() {
        use std::cell::RefCell;

        /// Host that tracks which EffectKinds are delivered via apply_effects.
        struct TrackingCapHost {
            text: String,
            cursor: usize,
            caps: HostCapabilitySet,
            delivered: RefCell<Vec<EffectKind>>,
        }

        impl TrackingCapHost {
            fn new(text: &str, caps: HostCapabilitySet) -> Self {
                Self {
                    text: text.to_string(),
                    cursor: 0,
                    caps,
                    delivered: RefCell::new(Vec::new()),
                }
            }
        }

        impl crate::document::Document for TrackingCapHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                simple_offset_to_pos(&self.text, offset.get())
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                simple_pos_to_offset(&self.text, pos)
            }
        }

        impl VimHost for TrackingCapHost {
            fn capabilities(&self) -> HostCapabilitySet {
                self.caps
            }
            fn cursor_offset(&self) -> usize {
                self.cursor
            }
            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    self.delivered.borrow_mut().push(effect.kind());
                    match effect {
                        Effect::SetCursor { offset } => self.cursor = offset.get(),
                        Effect::Insert { offset, text } => {
                            self.text.insert_str(offset.get(), text);
                        }
                        Effect::Delete { range } => {
                            self.text.drain(range.start().get()..range.end().get());
                        }
                        _ => {}
                    }
                }
            }
            fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
                RequestDisposition::Unsupported
            }
        }

        // Start with CORE only — no StatusMessages capability.
        let host = TrackingCapHost::new("hello", HostCapabilitySet::CORE);
        let mut session = VimSession::with_host(host);
        assert!(!session.capabilities().has(HostCapability::StatusMessages));

        // `ga` produces ShowInfo (character info). Without StatusMessages,
        // the ShowInfo effect should be filtered out by deliver_effects.
        let _ = session.process_key(KeyEvent::char('g'));
        let _ = session.process_key(KeyEvent::char('a'));

        let delivered_before: Vec<EffectKind> = session.host().delivered.borrow().clone();
        assert!(
            !delivered_before.contains(&EffectKind::ShowInfo),
            "ShowInfo should NOT be delivered without StatusMessages capability, got: {:?}",
            delivered_before,
        );

        // Upgrade: add StatusMessages capability mid-session.
        session.upgrade_capability(HostCapability::StatusMessages);
        assert!(session.capabilities().has(HostCapability::StatusMessages));

        // Clear the tracking buffer for the second attempt.
        session.host().delivered.borrow_mut().clear();

        // Same command `ga` — now ShowInfo SHOULD be delivered.
        let _ = session.process_key(KeyEvent::char('g'));
        let _ = session.process_key(KeyEvent::char('a'));

        let delivered_after: Vec<EffectKind> = session.host().delivered.borrow().clone();
        assert!(
            delivered_after.contains(&EffectKind::ShowInfo),
            "ShowInfo SHOULD be delivered after upgrading StatusMessages, got: {:?}",
            delivered_after,
        );
    }

    // ── Safety harness: panicking providers ─────────────────────────────────

    #[test]
    fn panicking_providers_does_not_crash_session() {
        use crate::document::Providers;
        use crate::execution::host::{HostRequest, RequestDisposition};

        struct PanickingProviderHost {
            text: String,
            cursor: usize,
        }

        impl crate::document::Document for PanickingProviderHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                let off = offset.get();
                if off > self.text.len() {
                    return None;
                }
                let prefix = &self.text[..off];
                let line = memchr::memchr_iter(b'\n', prefix.as_bytes()).count();
                let line_start = prefix.rfind('\n').map_or(0, |pos| pos + 1);
                let col = off - line_start;
                Some(crate::primitives::Position::from_raw(line, col))
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                let text = &self.text;
                let target_line = pos.line().get();
                let target_col = pos.col().get();
                let mut offset = 0;
                for _ in 0..target_line {
                    offset =
                        memchr::memchr(b'\n', text[offset..].as_bytes()).map(|i| offset + i + 1)?;
                }
                let line_end = memchr::memchr(b'\n', text[offset..].as_bytes())
                    .map(|i| offset + i)
                    .unwrap_or(text.len());
                let line_len = line_end - offset;
                let col = target_col.min(line_len);
                Some(crate::primitives::Offset::new(offset + col))
            }
        }

        impl VimHost for PanickingProviderHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::CORE
            }

            fn cursor_offset(&self) -> usize {
                self.cursor
            }

            fn providers(&self) -> Providers<'_> {
                panic!("tree-sitter crashed");
            }

            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    match effect {
                        Effect::SetCursor { offset } => {
                            self.cursor = offset.get();
                        }
                        _ => {}
                    }
                }
            }

            fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
                RequestDisposition::Unsupported
            }
        }

        let host = PanickingProviderHost {
            text: "hello world".to_string(),
            cursor: 0,
        };
        let mut session = VimSession::with_host(host);

        // Process several keys — none should crash despite providers() panicking.
        let _ = session.process_key(KeyEvent::char('l'));
        let _ = session.process_key(KeyEvent::char('l'));
        let _ = session.process_key(KeyEvent::char('h'));

        // Cursor should have moved (l, l, h => net +1).
        assert_eq!(session.host().cursor, 1);
    }

    #[test]
    fn panicking_providers_on_second_call_continues_working() {
        use crate::document::Providers;
        use crate::execution::host::{HostRequest, RequestDisposition};
        use std::cell::Cell;

        struct SecondCallPanicsHost {
            text: String,
            cursor: usize,
            call_count: Cell<u32>,
        }

        impl crate::document::Document for SecondCallPanicsHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                let off = offset.get();
                if off > self.text.len() {
                    return None;
                }
                let prefix = &self.text[..off];
                let line = memchr::memchr_iter(b'\n', prefix.as_bytes()).count();
                let line_start = prefix.rfind('\n').map_or(0, |pos| pos + 1);
                let col = off - line_start;
                Some(crate::primitives::Position::from_raw(line, col))
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                let text = &self.text;
                let target_line = pos.line().get();
                let target_col = pos.col().get();
                let mut offset = 0;
                for _ in 0..target_line {
                    offset =
                        memchr::memchr(b'\n', text[offset..].as_bytes()).map(|i| offset + i + 1)?;
                }
                let line_end = memchr::memchr(b'\n', text[offset..].as_bytes())
                    .map(|i| offset + i)
                    .unwrap_or(text.len());
                let line_len = line_end - offset;
                let col = target_col.min(line_len);
                Some(crate::primitives::Offset::new(offset + col))
            }
        }

        impl VimHost for SecondCallPanicsHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::CORE
            }

            fn cursor_offset(&self) -> usize {
                self.cursor
            }

            fn providers(&self) -> Providers<'_> {
                let count = self.call_count.get() + 1;
                self.call_count.set(count);
                if count >= 2 {
                    panic!("providers() panicked on call #{}", count);
                }
                Providers::default()
            }

            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    match effect {
                        Effect::SetCursor { offset } => {
                            self.cursor = offset.get();
                        }
                        _ => {}
                    }
                }
            }

            fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
                RequestDisposition::Unsupported
            }
        }

        let host = SecondCallPanicsHost {
            text: "hello world".to_string(),
            cursor: 0,
            call_count: Cell::new(0),
        };
        let mut session = VimSession::with_host(host);

        // First process_key: providers() succeeds (call #1).
        let _ = session.process_key(KeyEvent::char('l'));
        assert_eq!(session.host().cursor, 1);

        // Second process_key: providers() panics (call #2).
        // Session must not crash — safety harness catches the panic.
        let _ = session.process_key(KeyEvent::char('l'));
        assert_eq!(session.host().cursor, 2);

        // Third and beyond: providers() continues panicking.
        // Session must remain fully functional.
        let _ = session.process_key(KeyEvent::char('l'));
        assert_eq!(session.host().cursor, 3);

        let _ = session.process_key(KeyEvent::char('h'));
        assert_eq!(session.host().cursor, 2);

        // Verify the harness caught panics (at least 2: calls #2 and #3).
        // Call count includes the first successful call + all panicking calls.
        assert!(session.host().call_count.get() >= 3);
    }

    // ── Per-capability granular downgrade in build_context ───────────────────

    #[test]
    fn build_context_nulls_fold_provider_when_folding_disabled() {
        use crate::document::{FoldProvider, Providers};
        use crate::execution::host::{HostRequest, RequestDisposition};
        use crate::primitives::{Direction, LineNumber};

        struct StubFold;
        impl FoldProvider for StubFold {
            fn next_visible_line(&self, line: LineNumber, _dir: Direction) -> LineNumber {
                line
            }
            fn is_folded(&self, _line: LineNumber) -> bool {
                false
            }
        }

        struct FoldProviderHost {
            text: String,
            fold: StubFold,
        }

        impl crate::document::Document for FoldProviderHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                1
            }
            fn offset_to_pos(
                &self,
                _offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                Some(crate::primitives::Position::from_raw(0, 0))
            }
            fn pos_to_offset(
                &self,
                _pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                Some(crate::primitives::Offset::new(0))
            }
        }

        impl VimHost for FoldProviderHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::FULL
            }
            fn cursor_offset(&self) -> usize {
                0
            }
            fn providers(&self) -> Providers<'_> {
                Providers::new().with_fold(&self.fold)
            }
            fn apply_effects(&mut self, _effects: &[Effect]) {}
            fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
                RequestDisposition::Unsupported
            }
        }

        let host = FoldProviderHost {
            text: "hello".to_string(),
            fold: StubFold,
        };

        // Without any disabled capabilities, fold provider is present.
        let safety = SafetyHarness::new();
        let ctx = VimSession::<FoldProviderHost>::build_context(&host, &safety);
        assert!(
            ctx.providers().fold.is_some(),
            "fold provider should be present when Folding capability is not disabled"
        );

        // Disable Folding via 3 panics.
        for _ in 0..3 {
            let _: Option<()> =
                safety.query_for_capability(HostCapability::Folding as u8, || panic!("fold panic"));
        }
        assert!(safety.is_disabled(HostCapability::Folding as u8));

        // Now build_context should null out the fold provider.
        let ctx = VimSession::<FoldProviderHost>::build_context(&host, &safety);
        assert!(
            ctx.providers().fold.is_none(),
            "fold provider should be None after Folding capability is disabled"
        );
    }

    #[test]
    fn build_context_nulls_indent_and_search_independently() {
        use crate::document::{IndentProvider, Providers, SearchProvider};
        use crate::execution::host::{HostRequest, RequestDisposition};
        use crate::primitives::{Direction, LineNumber, Range, SearchFlags};
        use compact_str::CompactString;

        struct StubIndent;
        impl IndentProvider for StubIndent {
            fn indent_for_new_line(&self, _line: LineNumber) -> crate::document::IndentResult {
                crate::document::IndentResult::Simple {
                    indent: CompactString::from("  "),
                    append: None,
                }
            }
        }

        struct StubSearch;
        impl SearchProvider for StubSearch {
            fn find_match(
                &self,
                _pattern: &str,
                _from: usize,
                _direction: Direction,
                _flags: &SearchFlags,
            ) -> Option<Range> {
                None
            }
        }

        struct MultiProviderHost {
            text: String,
            indent: StubIndent,
            search: StubSearch,
        }

        impl crate::document::Document for MultiProviderHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                1
            }
            fn offset_to_pos(
                &self,
                _offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                Some(crate::primitives::Position::from_raw(0, 0))
            }
            fn pos_to_offset(
                &self,
                _pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                Some(crate::primitives::Offset::new(0))
            }
        }

        impl VimHost for MultiProviderHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::FULL
            }
            fn cursor_offset(&self) -> usize {
                0
            }
            fn providers(&self) -> Providers<'_> {
                Providers::new()
                    .with_indent(&self.indent)
                    .with_search(&self.search)
            }
            fn apply_effects(&mut self, _effects: &[Effect]) {}
            fn handle_request(&mut self, _request: &HostRequest) -> RequestDisposition {
                RequestDisposition::Unsupported
            }
        }

        let host = MultiProviderHost {
            text: "hello".to_string(),
            indent: StubIndent,
            search: StubSearch,
        };
        let safety = SafetyHarness::new();

        // Both present initially.
        let ctx = VimSession::<MultiProviderHost>::build_context(&host, &safety);
        assert!(ctx.providers().indent.is_some());
        assert!(ctx.providers().search.is_some());

        // Disable only Reindent — indent nulled, search preserved.
        for _ in 0..3 {
            let _: Option<()> = safety
                .query_for_capability(HostCapability::Reindent as u8, || panic!("indent panic"));
        }

        let ctx = VimSession::<MultiProviderHost>::build_context(&host, &safety);
        assert!(
            ctx.providers().indent.is_none(),
            "indent should be None after Reindent disabled"
        );
        assert!(
            ctx.providers().search.is_some(),
            "search should be preserved — only Reindent was disabled"
        );

        // Now also disable SearchHighlight.
        for _ in 0..3 {
            let _: Option<()> = safety
                .query_for_capability(HostCapability::SearchHighlight as u8, || {
                    panic!("search panic")
                });
        }

        let ctx = VimSession::<MultiProviderHost>::build_context(&host, &safety);
        assert!(ctx.providers().indent.is_none(), "indent still None");
        assert!(
            ctx.providers().search.is_none(),
            "search should be None after SearchHighlight disabled"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // RequestDisposition end-to-end tests
    // ══════════════════���═══════════════════════════════════════════════════

    /// Test host that returns `Completed(HostResult::Success)` for all requests.
    /// This exercises the synchronous completion path.
    struct CompletingHost {
        text: String,
        cursor: usize,
    }

    impl CompletingHost {
        fn new(text: &str) -> Self {
            Self {
                text: text.to_string(),
                cursor: 0,
            }
        }
    }

    impl crate::document::Document for CompletingHost {
        fn text(&self) -> &str {
            &self.text
        }
        fn line_count(&self) -> usize {
            memchr::memchr_iter(b'\n', self.text.as_bytes())
                .count()
                .max(1)
        }
        fn offset_to_pos(
            &self,
            offset: crate::primitives::Offset,
        ) -> Option<crate::primitives::Position> {
            simple_offset_to_pos(&self.text, offset.get())
        }
        fn pos_to_offset(
            &self,
            pos: crate::primitives::Position,
        ) -> Option<crate::primitives::Offset> {
            simple_pos_to_offset(&self.text, pos)
        }
    }

    impl VimHost for CompletingHost {
        fn capabilities(&self) -> HostCapabilitySet {
            HostCapabilitySet::FULL
        }
        fn cursor_offset(&self) -> usize {
            self.cursor
        }
        fn apply_effects(&mut self, effects: &[Effect]) {
            for effect in effects {
                match effect {
                    Effect::SetCursor { offset } => self.cursor = offset.get(),
                    Effect::Insert { offset, text } => {
                        self.text.insert_str(offset.get(), text);
                    }
                    Effect::Delete { range } => {
                        self.text.drain(range.start().get()..range.end().get());
                    }
                    _ => {}
                }
            }
        }
        fn handle_request(&mut self, request: &HostRequest) -> RequestDisposition {
            // Synchronously complete every request with Success.
            RequestDisposition::Completed(HostResult::Success {
                id: request.id(),
                message: None,
            })
        }
    }

    #[test]
    fn disposition_completed_processes_immediately_no_pending() {
        // Host returns Completed(Success) for GotoDefinition.
        // Verify: processed immediately, no pending entry.
        let host = CompletingHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // `gd` produces GotoDefinition request.
        let _ = session.process_key(KeyEvent::char('g'));
        let result = session.process_key(KeyEvent::char('d'));

        // No async requests should be returned — all completed synchronously.
        assert!(
            result.host_requests.is_empty(),
            "Completed disposition should NOT produce async host_requests, got: {:?}",
            result.host_requests,
        );

        // Nothing pending in the engine.
        assert_eq!(
            session.pending_request_count(),
            0,
            "Completed disposition should leave zero pending requests"
        );
    }

    #[test]
    fn disposition_deferred_adds_to_pending_then_complete_request_resolves() {
        // Host returns Deferred for GotoDefinition.
        // Verify: request IS in pending. Then complete it via complete_request().
        let host = DeferringHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // `gd` produces GotoDefinition, DeferringHost defers it.
        let _ = session.process_key(KeyEvent::char('g'));
        let result = session.process_key(KeyEvent::char('d'));

        // Should have one deferred request returned to the caller.
        assert_eq!(
            result.host_requests.len(),
            1,
            "Deferred disposition should return the request to the caller"
        );
        assert_eq!(
            result.host_requests[0].kind(),
            crate::execution::host::HostRequestKind::GotoDefinition,
        );

        let request_id = result.host_requests[0].id();

        // Should be in pending.
        assert_eq!(
            session.pending_request_count(),
            1,
            "Deferred request should be in pending"
        );

        // Complete it.
        let completion = session.complete_request(&HostResult::Success {
            id: request_id,
            message: None,
        });

        // After completion, pending is empty.
        assert_eq!(
            session.pending_request_count(),
            0,
            "After complete_request(), pending should be zero"
        );

        // complete_request returns a ProcessResult (may or may not have consumed).
        // The important thing: no panic, no hang, pending cleared.
        assert!(
            completion.host_requests.is_empty(),
            "Completing GotoDefinition should not produce further requests"
        );
    }

    #[test]
    fn disposition_unsupported_with_fallback_completes_immediately() {
        // Host returns Unsupported for GotoDefinition (which has a default_result).
        // Verify: default_result() is called, no pending entry, engine continues.
        use std::cell::RefCell;

        struct UnsupportedForAllHost {
            text: String,
            cursor: usize,
            requests_seen: RefCell<Vec<crate::execution::host::HostRequestKind>>,
        }

        impl UnsupportedForAllHost {
            fn new(text: &str) -> Self {
                Self {
                    text: text.to_string(),
                    cursor: 0,
                    requests_seen: RefCell::new(Vec::new()),
                }
            }
        }

        impl crate::document::Document for UnsupportedForAllHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                simple_offset_to_pos(&self.text, offset.get())
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                simple_pos_to_offset(&self.text, pos)
            }
        }

        impl VimHost for UnsupportedForAllHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::FULL
            }
            fn cursor_offset(&self) -> usize {
                self.cursor
            }
            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    match effect {
                        Effect::SetCursor { offset } => self.cursor = offset.get(),
                        Effect::Insert { offset, text } => {
                            self.text.insert_str(offset.get(), text);
                        }
                        Effect::Delete { range } => {
                            self.text.drain(range.start().get()..range.end().get());
                        }
                        _ => {}
                    }
                }
            }
            fn handle_request(&mut self, request: &HostRequest) -> RequestDisposition {
                self.requests_seen.borrow_mut().push(request.kind());
                RequestDisposition::Unsupported
            }
        }

        let host = UnsupportedForAllHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // `gd` -> GotoDefinition. Its default_result() is Some(Success).
        let _ = session.process_key(KeyEvent::char('g'));
        let result = session.process_key(KeyEvent::char('d'));

        // Host was asked to handle it (proving it reached handle_request).
        let seen = session.host().requests_seen.borrow();
        assert!(
            seen.contains(&crate::execution::host::HostRequestKind::GotoDefinition),
            "host should have been asked GotoDefinition, saw: {:?}",
            &*seen,
        );
        drop(seen);

        // No async request (fallback was applied immediately).
        assert!(
            result.host_requests.is_empty(),
            "Unsupported with fallback should NOT produce async host_requests, got: {:?}",
            result.host_requests,
        );

        // No pending entry.
        assert_eq!(
            session.pending_request_count(),
            0,
            "Unsupported with fallback should leave zero pending"
        );
    }

    #[test]
    fn disposition_unsupported_without_fallback_becomes_deferred() {
        // Host returns Unsupported for WriteFile (which has NO default_result).
        // The code pushes it to async_collector when no fallback exists.
        use std::cell::RefCell;

        struct UnsupportedWriteHost {
            text: String,
            cursor: usize,
            requests_seen: RefCell<Vec<crate::execution::host::HostRequestKind>>,
        }

        impl UnsupportedWriteHost {
            fn new(text: &str) -> Self {
                Self {
                    text: text.to_string(),
                    cursor: 0,
                    requests_seen: RefCell::new(Vec::new()),
                }
            }
        }

        impl crate::document::Document for UnsupportedWriteHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                simple_offset_to_pos(&self.text, offset.get())
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                simple_pos_to_offset(&self.text, pos)
            }
        }

        impl VimHost for UnsupportedWriteHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::FULL
            }
            fn cursor_offset(&self) -> usize {
                self.cursor
            }
            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    match effect {
                        Effect::SetCursor { offset } => self.cursor = offset.get(),
                        Effect::Insert { offset, text } => {
                            self.text.insert_str(offset.get(), text);
                        }
                        Effect::Delete { range } => {
                            self.text.drain(range.start().get()..range.end().get());
                        }
                        _ => {}
                    }
                }
            }
            fn handle_request(&mut self, request: &HostRequest) -> RequestDisposition {
                self.requests_seen.borrow_mut().push(request.kind());
                RequestDisposition::Unsupported
            }
        }

        let host = UnsupportedWriteHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // Type `:w<CR>` to trigger WriteFile request.
        let _ = session.process_key(KeyEvent::char(':'));
        let _ = session.process_key(KeyEvent::char('w'));
        let result = session.process_key(KeyEvent::from_name("CR").unwrap());

        // Host was asked to handle it.
        let seen = session.host().requests_seen.borrow();
        assert!(
            seen.contains(&crate::execution::host::HostRequestKind::WriteFile),
            "host should have been asked WriteFile, saw: {:?}",
            &*seen,
        );
        drop(seen);

        // WriteFile has no default_result (returns None), so when Unsupported
        // is returned, the request is pushed to async_collector.
        assert!(
            result
                .host_requests
                .iter()
                .any(|r| r.kind() == crate::execution::host::HostRequestKind::WriteFile),
            "Unsupported without fallback should push request to async host_requests, got: {:?}",
            result.host_requests,
        );

        // It should be in pending.
        assert!(
            session.pending_request_count() >= 1,
            "Unsupported without fallback should leave the request pending"
        );
    }

    #[test]
    fn disposition_mixed_host_handles_all_three_paths_independently() {
        // A host that returns different dispositions based on request kind:
        // - GotoDefinition -> Completed(Success) [immediate]
        // - ShowDocumentation -> Deferred [async pending]
        // - WriteFile -> Unsupported (has no fallback -> deferred)
        use std::cell::RefCell;

        struct MixedDispositionHost {
            text: String,
            cursor: usize,
            requests_seen: RefCell<Vec<crate::execution::host::HostRequestKind>>,
        }

        impl MixedDispositionHost {
            fn new(text: &str) -> Self {
                Self {
                    text: text.to_string(),
                    cursor: 0,
                    requests_seen: RefCell::new(Vec::new()),
                }
            }
        }

        impl crate::document::Document for MixedDispositionHost {
            fn text(&self) -> &str {
                &self.text
            }
            fn line_count(&self) -> usize {
                memchr::memchr_iter(b'\n', self.text.as_bytes())
                    .count()
                    .max(1)
            }
            fn offset_to_pos(
                &self,
                offset: crate::primitives::Offset,
            ) -> Option<crate::primitives::Position> {
                simple_offset_to_pos(&self.text, offset.get())
            }
            fn pos_to_offset(
                &self,
                pos: crate::primitives::Position,
            ) -> Option<crate::primitives::Offset> {
                simple_pos_to_offset(&self.text, pos)
            }
        }

        impl VimHost for MixedDispositionHost {
            fn capabilities(&self) -> HostCapabilitySet {
                HostCapabilitySet::FULL
            }
            fn cursor_offset(&self) -> usize {
                self.cursor
            }
            fn apply_effects(&mut self, effects: &[Effect]) {
                for effect in effects {
                    match effect {
                        Effect::SetCursor { offset } => self.cursor = offset.get(),
                        Effect::Insert { offset, text } => {
                            self.text.insert_str(offset.get(), text);
                        }
                        Effect::Delete { range } => {
                            self.text.drain(range.start().get()..range.end().get());
                        }
                        _ => {}
                    }
                }
            }
            fn handle_request(&mut self, request: &HostRequest) -> RequestDisposition {
                self.requests_seen.borrow_mut().push(request.kind());
                match request.kind() {
                    // Sync completion for GotoDefinition.
                    crate::execution::host::HostRequestKind::GotoDefinition => {
                        RequestDisposition::Completed(HostResult::Success {
                            id: request.id(),
                            message: None,
                        })
                    }
                    // Defer ShowDocumentation (host will complete later).
                    crate::execution::host::HostRequestKind::ShowDocumentation => {
                        RequestDisposition::Deferred
                    }
                    // Unsupported for everything else (including WriteFile).
                    _ => RequestDisposition::Unsupported,
                }
            }
        }

        let host = MixedDispositionHost::new("hello world");
        let mut session = VimSession::with_host(host);

        // === Path 1: Completed ===
        // `gd` -> GotoDefinition -> Completed(Success)
        let _ = session.process_key(KeyEvent::char('g'));
        let result_gd = session.process_key(KeyEvent::char('d'));

        // Should NOT be in host_requests (completed immediately).
        assert!(
            !result_gd
                .host_requests
                .iter()
                .any(|r| r.kind() == crate::execution::host::HostRequestKind::GotoDefinition),
            "Completed GotoDefinition should not appear in host_requests"
        );
        // Pending should be 0 after completed path.
        assert_eq!(session.pending_request_count(), 0);

        // === Path 2: Deferred ===
        // `K` -> ShowDocumentation -> Deferred
        let result_k = session.process_key(KeyEvent::char('K'));

        assert!(
            result_k
                .host_requests
                .iter()
                .any(|r| r.kind() == crate::execution::host::HostRequestKind::ShowDocumentation),
            "Deferred ShowDocumentation should appear in host_requests, got: {:?}",
            result_k.host_requests,
        );
        assert_eq!(
            session.pending_request_count(),
            1,
            "Deferred request should be pending"
        );

        // Complete the deferred ShowDocumentation.
        let show_doc_id = result_k
            .host_requests
            .iter()
            .find(|r| r.kind() == crate::execution::host::HostRequestKind::ShowDocumentation)
            .unwrap()
            .id();
        let _ = session.complete_request(&HostResult::Success {
            id: show_doc_id,
            message: None,
        });
        assert_eq!(
            session.pending_request_count(),
            0,
            "After completing ShowDocumentation, pending should be 0"
        );

        // === Path 3: Unsupported (no fallback) ===
        // `:w<CR>` -> WriteFile -> Unsupported -> no fallback -> pushed to async
        let _ = session.process_key(KeyEvent::char(':'));
        let _ = session.process_key(KeyEvent::char('w'));
        let result_w = session.process_key(KeyEvent::from_name("CR").unwrap());

        assert!(
            result_w
                .host_requests
                .iter()
                .any(|r| r.kind() == crate::execution::host::HostRequestKind::WriteFile),
            "Unsupported WriteFile (no fallback) should appear in host_requests, got: {:?}",
            result_w.host_requests,
        );
        assert!(
            session.pending_request_count() >= 1,
            "Unsupported WriteFile should be pending"
        );

        // Verify the host saw all three request kinds.
        let seen = session.host().requests_seen.borrow();
        assert!(
            seen.contains(&crate::execution::host::HostRequestKind::GotoDefinition),
            "Host should have seen GotoDefinition"
        );
        assert!(
            seen.contains(&crate::execution::host::HostRequestKind::ShowDocumentation),
            "Host should have seen ShowDocumentation"
        );
        assert!(
            seen.contains(&crate::execution::host::HostRequestKind::WriteFile),
            "Host should have seen WriteFile"
        );
    }

    // ── DeferredActionKind guard tests ──────────────────────────────────────

    #[test]
    fn deferred_action_kind_all_has_no_duplicates() {
        let unique: std::collections::HashSet<DeferredActionKind> =
            DeferredActionKind::ALL.iter().copied().collect();
        assert_eq!(unique.len(), DeferredActionKind::ALL.len());
    }

    #[test]
    fn deferred_action_kind_covers_every_variant() {
        let actions = [DeferredAction::WindowNav(WindowNavAction::MoveLeft)];
        let kinds: std::collections::HashSet<DeferredActionKind> =
            actions.iter().map(|a| a.kind()).collect();
        let all: std::collections::HashSet<DeferredActionKind> =
            DeferredActionKind::ALL.iter().copied().collect();
        let missing: Vec<_> = all.difference(&kinds).collect();
        assert!(
            missing.is_empty(),
            "DeferredActionKind variants not covered: {missing:?}"
        );
    }
}
