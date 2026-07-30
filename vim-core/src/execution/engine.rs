//! `VimEngine` - Core command execution engine.
//!
//! This is the main entry point for an embedding host.
//! Ties together Grammar, Keymap, State, and Executor.
//!
//! # Architecture
//!
//! ```text
//! Shell
//!   │
//!   ▼
//! VimEngine.process(key, InputContext<Validated>)
//!   │
//!   ├─→ Parser.process(key) → GrammarResult
//!   ├─→ Resolver.resolve(result) → (PlannedAction, was_repeat)
//!   └─→ Executor.execute_plan(plan) → Effects + HostRequests
//!   │
//!   ▼
//! Response { effects, host_requests, kind, message }
//! ```

mod command_line;
mod command_line_exec;
mod coordinators;
mod external_edit;
pub(crate) mod hooks;
mod host_completion;
mod insert;
mod macro_replay;
mod mapping;
mod mode_dispatch;
mod multi_cursor_yank;
mod per_cursor;
mod providers;
mod public_api;
mod select;
mod source;
mod substitute_confirm;
mod undo;
mod would_handle;

pub mod buffer_state;
mod key_interest;

pub use hooks::{HookAction, HookContext, HookHandler, HookId, HookPoint};
pub use key_interest::KeyInterestSet;
pub use macro_replay::{parse_keys_from_string, MacroOutput};
pub use public_api::HostMapping;
mod drift;
pub(crate) mod fork;
mod recording;
mod shadow;
mod shadow_diff;
pub mod shadow_document;
mod shadow_effects;
mod typeahead;
pub mod vim_text_document;

use self::coordinators::{HostCoordinator, RecordingState, TypeaheadCoordinator};
use self::providers::EngineProviders;
use self::typeahead::{ResolveResult, TypeaheadFlags};
use super::response::{Response, ResponseKind};
use super::{InputContext, Validated};
use crate::document::Document;
use crate::effects::{Effect, EffectPipeline};
use crate::execution::replay::SessionRecorder;
use crate::grammar::Parser;
use crate::keymap::{KeyEvent, Keymap, LangmapTable};
use crate::mode::{ModeAction, ModeContext, ModeDispatcher};
#[cfg(test)]
use crate::primitives::RegisterName;
use crate::primitives::{
    AbbrevTable, DigraphRegistry, Offset, OptionOverrides, SelectionShape, VimOptions,
};
use crate::primitives::{Mode, StickyTarget, VisualType};
use crate::state::{CommandLinePrompt, InsertState, VimState};

use crate::execution::trace::trace_event;
#[cfg(feature = "engine-tracing")]
use crate::execution::trace::TraceEvent;

/// The core Vim engine.
///
/// Also re-exported from the crate prelude; this is the type a host drives.
#[allow(clippy::struct_excessive_bools)]
pub struct VimEngine {
    // ── Core ──────────────────────────────────────────────────────────────
    /// Grammar parser state machine.
    parser: Parser,
    /// Default keymap.
    keymap: Keymap,
    /// Vim state (mode, registers, etc).
    state: VimState,
    /// Mode-specific key dispatcher (zero-cost, sealed trait).
    dispatcher: ModeDispatcher,
    /// Whether the current command is a dot-repeat replay.
    /// Set by `execute_plan()` and consumed by `execute_effect_plan()`.
    is_repeating: bool,
    /// Active command-line session metadata.
    command_line_session: Option<CommandLineSession>,
    /// Active sticky sub-mode session (e.g., Ctrl-W window mode, z-prefix mode).
    sticky_session: Option<StickySession>,
    /// Bitflags controlling which prefix groups auto-activate sticky mode.
    sticky_prefixes: u8,

    // ── Configuration ─────────────────────────────────────────────────────
    /// User-configurable Vim options (tabstop, shiftwidth, expandtab, etc.).
    /// This is the global layer; use `resolved_options` for effective values.
    options: VimOptions,

    /// Current buffer's local option overrides.
    ///
    /// Applied on top of `options` in the local-override → global cascade.
    buffer_overrides: OptionOverrides,

    /// Current window's local option overrides.
    ///
    /// Applied on top of `options` in the local-override → global cascade.
    window_overrides: OptionOverrides,

    /// Pre-resolved cache of effective options.
    ///
    /// Result of `VimOptions::resolve_all(&options, &buffer_overrides, &window_overrides)`.
    /// Updated whenever any of the three inputs change.
    /// Commands see this value; the host API reads from `options` (global layer).
    resolved_options: VimOptions,

    /// Dirty flag for the resolved-options cache.
    ///
    /// Set to `true` by `options_mut()` (which hands out `&mut VimOptions` and
    /// cannot detect when the caller stops mutating). Checked at the start of
    /// `process()` so the cache is rebuilt before the next command runs.
    options_dirty: bool,

    /// User-defined digraph registry (populated by host via `set_digraphs`).
    ///
    /// Consulted during the insert pre-compute phase for `InsertKind::Digraph`
    /// resolution. User-defined entries take precedence over the built-in
    /// RFC 1345 table.
    digraph_registry: DigraphRegistry,

    /// Abbreviation table (populated by `:abbreviate`/`:iabbrev`/`:cabbrev`).
    ///
    /// Stores user-defined abbreviations for insert and command-line modes.
    abbrev_table: AbbrevTable,

    /// Langmap character remapping table (`:set langmap=...`).
    ///
    /// Applied in `apply_langmap_and_normalize()` before the host-driven
    /// `latin_key` fallback. Only active in command-key contexts
    /// (Normal/Visual/OP-pending, parser not expecting literal char).
    pub(crate) langmap_table: LangmapTable,

    // ── Sub-coordinators ──────────────────────────────────────────────────
    /// Host request sequencing and pending-request tracking.
    host: HostCoordinator,
    /// Unified typeahead buffer and macro replay frame stack.
    typeahead: TypeaheadCoordinator,
    /// Macro recording state (register + keystroke buffer).
    recording: RecordingState,

    // ── Persistent Providers ───────────────────────────────────────────
    /// Engine-level providers registered once, merged with per-call providers.
    engine_providers: EngineProviders,

    // ── Handler Map ──────────────────────────────────────────────────────
    /// Per-key, per-mode handler delegation (`:sethandler`).
    ///
    /// When a key's handler is `Host` in the current mode, `process()`
    /// returns `Response::ignored()` without any vim processing.
    handler_map: crate::keymap::HandlerMap,

    // ── Provenance Tracking ─────────────────────────────────────────────
    /// Monotonic keystroke counter, incremented on every `process()` call.
    ///
    /// Used to tag responses with [`crate::effects::EffectProvenance`] for debugging,
    /// replay analysis, and session recording correlation.
    keystroke_seq: u64,

    // ── Predictive ────────────────────────────────────────────────────
    /// Adaptive per-session prediction weights for `predict()` / `predict_with_ranges()`.
    ///
    /// Updated by `observe()` after each committed keystroke when the
    /// `predictive` feature is enabled. Consulted by prediction methods
    /// to rank candidates by observed frequency rather than hard-coded
    /// likelihoods.
    pub(in crate::execution) prediction_weights: super::predictive::PredictionWeights,

    // ── Speculative Execution ────────────────────────────────────────
    /// Whether a [`ScopedFork`] is currently active.
    ///
    /// When `true`, recording, federation, pipeline, and hook firing are
    /// suppressed. Set by [`VimEngine::fork()`] and cleared by
    /// [`ScopedFork::commit()`] or [`ScopedFork::drop()`].
    pub(in crate::execution::engine) fork_active: bool,

    // ── Shadow Execution ──────────────────────────────────────────────
    /// Runtime toggle for shadow execution of macro replays.
    ///
    /// When `true`, `process()` will attempt to execute pending macro keys
    /// entirely in-memory via [`execute_shadow_replay()`] after the
    /// triggering keystroke, batching all text mutations into a single diff.
    /// Default is `false` (traditional per-key host round-trip replay).
    shadow_enabled: bool,

    // ── Self-Healing Shadow Document ─────────────────────────────────
    /// Persistent in-memory copy of the host document text.
    ///
    /// Updated incrementally by `apply_external_edit()` and by effect
    /// processing. Used for position remapping, undo entry creation,
    /// and cross-line detection during external edit reconciliation.
    /// `None` until the host provides initial text via `set_shadow_text()`.
    shadow: Option<shadow_document::OwnedDocument>,

    /// Last-seen text generation counter from the host.
    ///
    /// Used as fast-path to skip text comparison in the drift gate when the
    /// generation hasn't changed.
    shadow_generation: Option<u64>,

    /// NodeId of the most recently created external-edit undo node.
    ///
    /// Set by `apply_external_edit()` after `end_group()` commits. Consumed
    /// by `VimSession<SessionHost>` to record the corresponding text snapshot
    /// in the host's `UndoStore`. Reset to `None` after being consumed.
    last_external_edit_node: Option<crate::primitives::NodeId>,

    /// NodeId of a pending group that was force-committed to make room for
    /// an external edit (e.g. an active INSERT session interrupted by a
    /// non-merging edit like HostDrift). The session layer must create an
    /// `UndoStore` entry for this node to keep the two systems in sync.
    last_force_committed_node: Option<crate::primitives::NodeId>,

    // ── Native Insert ────────────────────────────────────────────────
    /// Whether the host handles printable chars and Enter natively in insert
    /// mode. When `false` (default), the engine processes all insert-mode
    /// keystrokes through the full pipeline. Set from `HostCapability::NativeInsert`
    /// by `VimSession` at construction.
    native_insert: bool,

    // ── <Cmd> collection buffer ───────────────────────────────────────
    /// Accumulates the ex command text between `<Cmd>` and `<CR>` in a mapping RHS.
    ///
    /// `Some(buf)` means we are currently inside a `<Cmd>…<CR>` sequence;
    /// each subsequent `Key::Char` is appended, `Key::Enter` triggers execution,
    /// and `Key::Escape` cancels the sequence. `None` means idle.
    cmd_buffer: Option<String>,

    /// Pending error from macro effect limit exceeded.
    pending_macro_error: Option<crate::errors::VimError>,

    // ── Key Interest Tracking ─────────────────────────────────────────
    /// True when the key interest set needs recomputation (mappings changed).
    ///
    /// Initialized to `true` so the first response includes the interest set.
    /// Set to `true` on any mapping mutation (`:map`, `:unmap`, `:noremap`,
    /// `:sethandler`, `source_config`, buffer mapping changes). Cleared by
    /// the integration layer after emitting the interest set to the host.
    pub(crate) key_interest_dirty: bool,

    // ── Host-pushed viewport/terminal state ──────────────────────────────
    /// First visible line as reported by the host via `ViewportChanged`.
    viewport_first_line: usize,
    /// Visible line count as reported by the host via `ViewportChanged`.
    viewport_height: usize,
    /// Terminal width in columns as reported by `WindowResized`.
    terminal_cols: usize,
    /// Terminal height in rows as reported by `WindowResized`.
    terminal_rows: usize,
    /// Active diagnostic count as reported by `DiagnosticsUpdated`.
    diagnostics_count: usize,

    // ── Cold State ──────────────────────────────────────────────────────
    /// Heap-allocated cold fields that are NOT accessed on the per-keystroke
    /// hot path (`process()`). Moving them behind a `Box` reduces the inline
    /// size of `VimEngine` and improves L1 cache locality for fields that ARE
    /// accessed every keystroke.
    cold: Box<EngineColdState>,
}

/// Rarely-accessed fields of [`VimEngine`], boxed for cache locality.
///
/// These fields are only accessed through public API calls (pipeline
/// configuration, session recording, property overlays, mode profile
/// overrides, federation) and are NOT on the per-keystroke
/// hot path.
pub(in crate::execution::engine) struct EngineColdState {
    // ── Effect Pipeline ────────────────────────────────────────────────
    /// Optional effect middleware pipeline.
    ///
    /// When `Some`, every response's effects pass through this pipeline
    /// before being returned to the caller. Enables logging, deduplication,
    /// and custom transformations without modifying core code.
    pub(in crate::execution::engine) pipeline: Option<EffectPipeline>,

    // ── Session replay ───────────────────────────────────────────────────
    /// Optional session recorder for time-travel debugging.
    ///
    /// When `Some`, every keystroke processed through [`process()`] records
    /// the key and resulting effects into this recorder.
    pub(in crate::execution::engine) recorder: Option<SessionRecorder>,

    // ── Property Overlay ──────────────────────────────────────────────
    /// Runtime overrides for command behavioral properties.
    ///
    /// Allows shells to override how specific `Command` variants interact
    /// with dot-repeat, jump lists, visual mode, etc. without modifying
    /// the grammar layer.
    pub(in crate::execution::engine) property_overlay: super::property_overlay::PropertyOverlay,

    // ── Mode Profile Overrides ─────────────────────────────────────────
    /// Per-mode profile overrides provided by the host.
    ///
    /// Maps a mode discriminant (`u8`, produced by `Mode::discriminant()`)
    /// to a custom [`ModeProfile`]. When resolving the profile for a mode,
    /// [`mode_profile()`] returns the override if one exists, otherwise it
    /// delegates to [`Mode::default_profile()`].
    ///
    /// Populated by [`set_mode_profile()`] and cleared by [`clear_mode_profile()`].
    pub(in crate::execution::engine) profile_overrides:
        ahash::AHashMap<u8, crate::mode::capabilities::ModeProfile>,

    // ── Hook Bus ──────────────────────────────────────────────────────
    /// Runtime event dispatch bus for pre/post-command hooks.
    ///
    /// Handlers registered via [`VimEngine::add_hook()`] are invoked at
    /// specific engine lifecycle points. Firing is suppressed when
    /// `fork_active` is `true` (speculative execution).
    pub(in crate::execution::engine) hooks: hooks::HookBus,

    // ── Event Registry ──────────────────────────────────────────────
    /// Typed autocommand registry for event-driven subscriptions.
    ///
    /// Stores autocmd registrations keyed by event discriminant with
    /// priority ordering. Managed via [`VimEngine::event_registry()`]
    /// and [`VimEngine::event_registry_mut()`].
    pub(in crate::execution::engine) event_registry: super::event_registry::EventRegistry,

    // ── Federation Backend ─────────────────────────────────────────────
    /// Optional cross-instance state federation backend.
    ///
    /// When `Some`, the engine uses this backend to persist and load shared
    /// state (registers, global marks, search/command history, macros).
    /// State events are extracted after each keystroke via `extract_events()`.
    pub(in crate::execution::engine) backend:
        Option<Box<dyn crate::state::federation::StateBackend>>,

    // ── Engine Tracing ────────────────────────────────────────────────
    /// Structured trace event collector for observability.
    ///
    /// When `engine-tracing` is enabled and `trace.enabled` is `true`,
    /// engine methods push [`TraceEvent`]s into this collector. Drained
    /// by the host via `drain_trace_events()`.
    #[cfg(feature = "engine-tracing")]
    pub(in crate::execution::engine) trace: crate::execution::trace::TraceCollector,
}

impl std::fmt::Debug for VimEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VimEngine")
            .field("mode", &self.state.mode())
            .field("parser_state", &self.parser)
            .field("is_repeating", &self.is_repeating)
            .field("fork_active", &self.fork_active)
            .field("shadow_enabled", &self.shadow_enabled)
            .field("native_insert", &self.native_insert)
            .field(
                "has_command_line_session",
                &self.command_line_session.is_some(),
            )
            .field("keystroke_seq", &self.keystroke_seq)
            .field("has_pipeline", &self.cold.pipeline.is_some())
            .field("has_recorder", &self.cold.recorder.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy)]
struct CommandLineSession {
    prompt: CommandLinePrompt,
    entered_from_mode: Mode,
    intent: Option<crate::grammar::OperatorSearchIntent>,
    /// Count prefix from normal mode (e.g., `2/pattern` → count=2).
    count: u32,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::execution::engine) struct StickySession {
    target: StickyTarget,
    pending_count: Option<u32>,
}

impl StickySession {
    pub(in crate::execution::engine) const fn new(target: StickyTarget) -> Self {
        Self {
            target,
            pending_count: None,
        }
    }

    pub(in crate::execution::engine) fn accumulate_digit(&mut self, digit: u32) {
        let current = self.pending_count.unwrap_or(0);
        self.pending_count = Some(current.saturating_mul(10).saturating_add(digit));
    }

    pub(in crate::execution::engine) const fn take_count(&mut self) -> Option<u32> {
        self.pending_count.take()
    }

    pub(in crate::execution::engine) const fn target(&self) -> StickyTarget {
        self.target
    }
}

impl VimEngine {
    /// Create a new `VimEngine` with default state.
    ///
    /// Delegates to `From<VimState>` to avoid field-list duplication.
    ///
    /// # Example
    /// ```
    /// use vim_core::execution::VimEngine;
    /// use vim_core::primitives::Mode;
    /// let engine = VimEngine::new();
    /// assert_eq!(engine.mode(), Mode::Normal);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::from(VimState::default())
    }

    /// Rebuild the resolved-options cache from the global options and local overrides.
    ///
    /// Called whenever `options`, `buffer_overrides`, or `window_overrides` changes.
    /// Internal helper; public callers use the override setter/taker API.
    fn rebuild_resolved_cache(&mut self) {
        self.resolved_options = VimOptions::resolve_all(
            &self.options,
            &self.buffer_overrides,
            &self.window_overrides,
        );
        // Sync parser-level flags derived from options.
        self.parser
            .set_sneak_mode(self.resolved_options.sneak_mode());
        // Sync belloff to state for use in effect processing.
        self.state.set_belloff(self.resolved_options.belloff());
        self.options_dirty = false;
    }

    /// Rebuild the langmap table from the current options if the string changed.
    ///
    /// Called after any `:set` that might have changed langmap. Parses the raw
    /// langmap string from `self.options` into a [`LangmapTable`] and replaces
    /// the engine's table. On parse error, the table is cleared (safe default).
    /// Always sets `key_interest_dirty` so the host interest set is recomputed.
    fn rebuild_langmap_if_needed(&mut self) {
        let current = self.options.langmap();
        // If both the option and the table are empty, nothing changed.
        if current.is_empty() && self.langmap_table.is_empty() {
            return;
        }
        if let Ok(table) = LangmapTable::parse(current) {
            self.langmap_table = table;
        } else {
            // On parse error, clear the table (safe default).
            self.langmap_table.clear();
        }
        self.key_interest_dirty = true;
    }

    /// Consume and return the flags from the most recent `drain_next_key()`.
    ///
    /// Returns `Some(flags)` if the previous call was `drain_next_key()`,
    /// allowing `process()` to determine whether the key was user-typed or
    /// replayed (macro/mapping). Returns `None` if no drain occurred since
    /// the last consumption (i.e., this is a direct user keystroke).
    ///
    /// The flags are consumed (reset to empty) on call, so a second call
    /// without an intervening `drain_next_key()` returns `None`.
    const fn take_last_drained_flags(&mut self) -> Option<TypeaheadFlags> {
        let flags = self.typeahead.last_drained_flags;
        if flags.is_empty() {
            None // Direct user input — not from drain
        } else {
            self.typeahead.last_drained_flags = TypeaheadFlags::empty();
            Some(flags)
        }
    }

    /// Process a keystroke.
    ///
    /// This is the main entry point. Given a key and input context,
    /// returns effects + host requests that the shell should apply/execute.
    ///
    /// # Type-State Enforcement
    ///
    /// The context MUST be validated before calling this method.
    /// Passing an unvalidated context is a **compile error**.
    ///
    /// # Complexity
    ///
    /// Time: O(M + E) where M = mapping expansion chain length (typically 1-3
    /// recursive expansions, bounded by recursion overflow limit of 100) and
    /// E = number of effects produced by mode dispatch (typically 1-10).
    /// The mapping resolution loop is O(k) per expansion where k = key sequence
    /// length in the trie. Effect processing is O(E) for state synchronization.
    /// Shadow execution, if enabled, adds O(K * E_avg) where K = pending macro
    /// keys and E_avg = average effects per key.
    ///
    /// Space: O(E + H) where E = effects count and H = host requests count.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Create and validate context
    /// let ctx = InputContext::new(&doc, cursor_offset).validate()?;
    /// let response = engine.process(key, ctx);
    /// for effect in response.effects {
    ///     shell.apply(effect);
    /// }
    /// for request in response.host_requests {
    ///     shell.execute_host_request(request);
    /// }
    /// ```
    pub fn process<D: Document>(
        &mut self,
        key: KeyEvent,
        ctx: InputContext<'_, D, Validated>,
    ) -> Response {
        if !self.state.mode().is_insert() {
            self.sync_primary_cursor(ctx.cursor_offset_raw());
        }

        // Rebuild resolved-options cache if the host mutated options via `options_mut()`.
        if self.options_dirty {
            self.rebuild_resolved_cache();
            self.rebuild_langmap_if_needed();
        }

        // ──── Drift Gate ────────────────────────────────────────────────
        // Compare the shadow document against the host's text BEFORE any
        // state mutation for this keystroke. If they differ, auto-heal via
        // reconcile_drift() which applies the diff as an ExternalEdit.
        // Skipped inside fork (shadow replay) — the fork's OwnedDocument
        // is the source of truth there, not the host.
        if !self.fork_active {
            let host_text = ctx.doc().text();
            let host_gen = ctx.doc().text_generation();

            // Fast-path: generation counter matches → no drift possible.
            let needs_check = match (host_gen, self.shadow_generation) {
                (Some(hg), Some(sg)) if hg == sg => false,
                _ => true,
            };

            let drift = needs_check && self.shadow.as_ref().is_some_and(|s| s.text() != host_text);
            if drift {
                self.reconcile_drift(host_text, ctx.cursor_offset());
            }

            // Update stored generation for next call.
            self.shadow_generation = host_gen;
        }

        // Increment monotonic keystroke counter for provenance tracking.
        self.keystroke_seq += 1;

        trace_event!(
            self,
            TraceEvent::ProcessKey {
                key: compact_str::CompactString::from(format!("{key:?}")),
                mode: compact_str::CompactString::from(self.state.mode().display_name()),
                keystroke_seq: self.keystroke_seq,
            }
        );

        // Clear per-keystroke transient output (message, scroll hint)
        // so the host only sees fresh output from THIS keystroke.
        self.state.clear_transient();

        if let Some(err) = self.pending_macro_error.take() {
            let effects = crate::effects::Effects::new().show_error(err);
            return Response::with_effects(effects);
        }

        // Sync current buffer identity from InputContext for cross-buffer
        // jump list tagging (see effect_processor::PushJumpList handler).
        self.state.set_current_buffer_id(ctx.buffer_id());

        // ── Origin detection ─────────────────────────────────────
        // Determine if this key was user-typed or drained from the
        // typeahead buffer / macro stack. When `drain_next_key()` was
        // called, it stores the origin flags; consuming them here
        // tells us whether to record this key and whether it counts
        // as live user input.
        let drained_flags = self.take_last_drained_flags();
        let is_typed_key = drained_flags.is_none_or(|f| f.contains(TypeaheadFlags::TYPED));

        // ── Shadow text snapshot ─────────────────────────────────
        // Capture document state for shadow execution BEFORE any processing.
        // Only taken on the outer (user) keystroke — fork_active is true
        // during the inner shadow/fork loop, so the guard skips this.
        let (shadow_text_snapshot, shadow_cursor, shadow_selection) =
            if self.shadow_enabled && !self.fork_active {
                (
                    Some(ctx.doc().text().to_owned()),
                    ctx.cursor_offset_raw(),
                    ctx.selection(),
                )
            } else {
                (None, 0, None)
            };

        // Snapshot recording state BEFORE this key is processed.
        let was_recording = self.recording.buffer.is_some();

        let mode = self.state.mode();

        // ── sethandler delegation ─────────────────────────────────
        // If this key is delegated to the host in the current mode,
        // return Ignored immediately — the host handles it natively.
        // Skipped during fork/shadow execution (no host delegation inside fork).
        if !self.fork_active {
            if let Some(mm) = crate::keymap::MappingMode::from_mode(mode) {
                if self.handler_map.is_host_handled(key, mm) {
                    return Response::ignored();
                }
            }
        }

        // ── Layout normalization ────────────────────────────────────
        // When a non-Latin keyboard layout is active, the host bridge
        // provides a Latin command equivalent via latin_key. Normalize
        // to the Latin key when we're in a command-dispatch context
        // (Ready state in Normal/Visual/OP). Preserve the localized
        // key in literal-char contexts (f/t/r argument, Insert mode,
        // CommandLine text) so the user can type/find non-Latin chars.
        let key = self.apply_langmap_and_normalize(key);

        // ── Sticky sub-mode interception ─────────────────────────
        // When a sticky session is active and we're in Normal mode,
        // intercept keys before mapping expansion to avoid user mappings
        // capturing Escape or digits intended for the sticky session.
        if let Some(ref mut session) = self.sticky_session {
            if self.state.mode() == Mode::Normal {
                // Escape / Ctrl-C / Ctrl-[ exits sticky mode
                if key.key() == crate::keymap::Key::Escape
                    || key == KeyEvent::ctrl('c')
                    || key == KeyEvent::ctrl('[')
                {
                    self.sticky_session = None;
                    // Reset parser — maybe_reenter_sticky() may have set it to
                    // AwaitingWindowCommand / AwaitingPrefix; without reset, the
                    // next key would be misinterpreted as a sub-command.
                    self.parser.reset();
                    return Response::with_effects(crate::effects::Effects::new().clear_message());
                }

                // Digits accumulate into the session's pending count
                if let Some(digit) = key.as_char().and_then(|c| c.to_digit(10)) {
                    session.accumulate_digit(digit);
                    return Response::consumed_empty();
                }

                // All other keys: set parser to the prefix-awaiting state
                // with accumulated count, then fall through to normal processing
                let count = session.take_count();
                match session.target() {
                    StickyTarget::Window => {
                        self.parser.set_state(
                            crate::grammar::input_state::InputState::AwaitingWindowCommand {
                                count,
                                register: None,
                            },
                        );
                    }
                    StickyTarget::ZPrefix => {
                        self.parser.set_state(
                            crate::grammar::input_state::InputState::AwaitingPrefix {
                                count,
                                register: None,
                                prefix: 'z',
                                operator: None,
                                force_type: None,
                            },
                        );
                    }
                }
                // Fall through to normal mapping expansion + parser dispatch
            }
        }

        // ── Bracketed paste markers ──────────────────────────────
        // PasteStart/PasteEnd toggle the paste flag on InsertState.
        // During paste: mappings are forced NOREMAP, abbreviations and
        // auto-indent are suppressed (checked at their respective sites).
        match key.key() {
            crate::keymap::Key::PasteStart => {
                if let Some(is) = self.state.insert_state_mut() {
                    is.set_pasting(true);
                }
                return Response::consumed_empty();
            }
            crate::keymap::Key::PasteEnd => {
                if let Some(is) = self.state.insert_state_mut() {
                    is.set_pasting(false);
                }
                return Response::consumed_empty();
            }
            _ => {} // drift: only PasteStart/PasteEnd are intercepted; all other keys fall through to mapping
        }

        // ── Mapping expansion layer ─────────────────────────────
        // Loop to handle recursive mapping chains (e.g., key → <Plug>(name) → actual keys).
        // Each iteration feeds the dispatched key back through the unified typeahead
        // buffer's resolve_key() until the key doesn't match any further mapping.
        //
        // Skip mapping expansion when the parser expects a literal character
        // (r, f, t, F, T, surround, sneak, etc.). In Vim, mappings are not
        // expanded for character arguments — the raw key is used directly.
        //
        // Initial flags come from the drain origin: keys flushed by a mapping
        // timeout carry NOREMAP to prevent re-capture by the same mapping prefix.
        // User-typed keys (no drain) default to user_typed() for normal expansion.
        let mut current_key = key;
        let mut current_flags = drained_flags.unwrap_or_else(TypeaheadFlags::user_typed);
        if self.parser.state().expects_literal_char() {
            current_flags |= TypeaheadFlags::NOREMAP;
        }
        // Suppress mapping for `0` when the grammar parser is accumulating a
        // count.  A common user mapping is `nmap 0 ^`.  Without this guard,
        // typing `10j` would expand `0` to `^` instead of treating it as part
        // of the count "10".  At the *start* of input (no count yet), `0` is
        // the column-zero motion and mapping expansion is intentional.
        if matches!(key.key(), crate::keymap::Key::Char('0'))
            && self.parser.state().is_accumulating_count()
        {
            current_flags |= TypeaheadFlags::NOREMAP;
        }
        // During bracketed paste: suppress all mapping expansion.
        if self.state.insert_state().is_some_and(InsertState::pasting) {
            current_flags |= TypeaheadFlags::NOREMAP;
        }
        let resolved_key;
        let resolved_flags;
        loop {
            match self
                .typeahead
                .buffer
                .resolve_key(current_key, current_flags, &self.keymap, mode)
            {
                ResolveResult::Dispatch(k, flags) => {
                    // If the dispatched key came from a recursive mapping expansion
                    // (REMAPPABLE flag, key changed), re-resolve to handle chains
                    // like <Plug>(name) → real keys.
                    if flags.contains(TypeaheadFlags::REMAPPABLE)
                        && k != current_key
                        && !flags.contains(TypeaheadFlags::NOREMAP)
                    {
                        current_key = k;
                        current_flags = flags;
                        continue;
                    }
                    resolved_key = k;
                    resolved_flags = flags;
                    break;
                }
                ResolveResult::Pending => return Response::pending_response(),
                ResolveResult::ExprMapping {
                    expression,
                    kind,
                    mode: mapping_mode,
                    silent,
                } => {
                    return self.handle_expr_mapping(expression, mapping_mode, kind, silent);
                }
                ResolveResult::RecursionOverflow => {
                    self.typeahead.buffer.clear();
                    self.state.clear_transient();
                    let effects = crate::effects::Effects::new().show_error(
                        crate::errors::VimError::RecursiveMacro {
                            register: ' ',
                            depth: 100,
                        },
                    );
                    let response = Response::with_effects(effects);
                    return if self.fork_active {
                        response
                    } else {
                        self.register_pending_host_requests(response)
                    };
                }
            }
        }

        // ── Alt-key decomposition for terminal hosts ──────────
        // When an Alt-modified key passed through mapping expansion without
        // matching, decompose to Esc + base key. This matches terminal
        // behavior where Alt sends Esc prefix. Only active when the
        // `alt_sends_esc` option is enabled (off by default for GUI hosts).
        if resolved_key
            .modifiers
            .contains(crate::keymap::Modifiers::ALT)
            && self.resolved_options.alt_sends_esc()
        {
            let base = KeyEvent::new(
                resolved_key.key(),
                resolved_key.modifiers & !crate::keymap::Modifiers::ALT,
            );
            self.typeahead.buffer.inject_front_noremap(base);
            return self.process(KeyEvent::escape(), ctx);
        }

        // ── <Action>(name) interception ─────────────────────────
        // When a mapping expands to <Action>(id), resolve the name
        // from the action registry and emit Effect::HostAction directly,
        // bypassing mode dispatch entirely.  Also emit a RunAction host
        // request so the host can actually dispatch the action.
        if let crate::keymap::Key::Action(id) = resolved_key.key() {
            if id == u32::MAX {
                return Response::consumed_empty();
            }

            // Snapshot context BEFORE any mutation.
            let pending_count = self.parser.state().count();
            let pending_register = self.parser.state().register();
            let invocation_mode = self.state.mode();
            let selection = ctx.selection();

            self.parser.reset();

            let mut response = if let Some(name) = self.keymap.action_name(id) {
                let effects = crate::effects::Effects::new().host_action(name);
                let mut resp = Response::with_effects(effects);
                let meta = self.host.sequencer.next_meta();
                resp.host_requests.push(super::HostRequest::RunAction {
                    meta,
                    name: name.into(),
                    count: pending_count,
                    register: pending_register,
                    mode: compact_str::CompactString::from(invocation_mode.display_name()),
                    selection_anchor: selection.map(|s| s.anchor().get()),
                    selection_head: selection.map(|s| s.head().get()),
                });
                resp
            } else {
                Response::consumed_empty()
            };

            // Exit visual mode like operators do.
            if invocation_mode.is_visual() {
                // Save visual info for gv command before exiting.
                if let (Some(vt), Some(sel)) = (invocation_mode.visual_type(), selection) {
                    let text = ctx.doc().text();
                    let tabstop = self.options.tabstop();
                    let cursor_at_start = !sel.is_forward();
                    let info = crate::commands::visual::selection::compute_last_visual_info(
                        text,
                        vt,
                        sel.start().get(),
                        sel.end().get(),
                        tabstop,
                    )
                    .with_cursor_at_start(cursor_at_start);
                    response.effects.push(Effect::SaveLastVisual { info });
                }
                response.effects.push(Effect::set_mode(Mode::Normal));
                response.effects.push(Effect::ClearSelection);
                self.state.set_mode(Mode::Normal);
            }

            let is_silent = resolved_flags.contains(TypeaheadFlags::SILENT)
                || drained_flags.is_some_and(|f| f.contains(TypeaheadFlags::SILENT));
            if is_silent {
                response
                    .effects
                    .retain(|e| !matches!(e, Effect::ShowInfo { .. }));
            }

            if !self.fork_active {
                if was_recording && self.recording.buffer.is_some() && is_typed_key {
                    self.record_key(key, &mut response);
                }

                if let Some(ref pipeline) = self.cold.pipeline {
                    pipeline.run(&mut response.effects);
                }
            }

            return self.register_pending_host_requests(response);
        }

        // ── <Cmd>…<CR> interception ──────────────────────────────
        // When inside a <Cmd>…<CR> mapping sequence, collect chars until <CR>
        // and then execute the accumulated string as an ex command without
        // changing mode. <Esc> cancels the sequence.
        if resolved_key.key() == crate::keymap::Key::Cmd {
            self.cmd_buffer = Some(String::new());
            return Response::consumed_empty();
        }
        if let Some(ref mut buf) = self.cmd_buffer {
            match resolved_key.key() {
                crate::keymap::Key::Enter => {
                    let cmd = std::mem::take(buf);
                    self.cmd_buffer = None;
                    let mut response = self.execute_cmd_mapping(&cmd, ctx);
                    if !self.fork_active {
                        if let Some(ref pipeline) = self.cold.pipeline {
                            pipeline.run(&mut response.effects);
                        }
                    }
                    return response;
                }
                crate::keymap::Key::Escape => {
                    self.cmd_buffer = None;
                    return Response::consumed_empty();
                }
                crate::keymap::Key::Char(c) => {
                    buf.push(c);
                    return Response::consumed_empty();
                }
                // drift: non-character keys (arrows, F-keys, etc.) have no meaning inside a <Cmd>…<CR> sequence and are silently consumed
                _ => {
                    // Any other key (e.g. <BS>, special keys) in cmd buffer — ignore/skip.
                    return Response::consumed_empty();
                }
            }
        }

        // ── Ctrl-R register sub-state interception ────────────
        // When the command-line is awaiting a register name (Ctrl-R was pressed),
        // intercept the next key before mode dispatch to resolve and insert the
        // register contents. Document text is passed for Ctrl-R Ctrl-W / Ctrl-R Ctrl-A.
        if mode == Mode::CommandLine && self.state.command_line().awaiting_register() {
            let mut response = self.handle_command_line_register_key(
                resolved_key,
                ctx.doc().text(),
                ctx.cursor_offset().get(),
            );

            if !self.fork_active {
                if was_recording && self.recording.buffer.is_some() && is_typed_key {
                    self.record_key(key, &mut response);
                }

                return self.register_pending_host_requests(response);
            }

            return response;
        }

        // :s///c confirm interception
        if let Some(mut response) = self.try_handle_substitute_confirm(resolved_key) {
            if !self.fork_active {
                if was_recording && self.recording.buffer.is_some() && is_typed_key {
                    self.record_key(key, &mut response);
                }
                return self.register_pending_host_requests(response);
            }
            return response;
        }

        // Trait-based dispatch: each mode handler decides what to do,
        // the engine handles execution.
        let was_operator_pending = self.parser.state().is_operator_pending();
        let action = {
            let dispatcher = &self.dispatcher;
            let mut mode_ctx = ModeContext::new(&mut self.state, &self.keymap, &mut self.parser);
            dispatcher.dispatch(mode, resolved_key, &mut mode_ctx)
        };

        // ── Composing char finalization (Neovim parity) ──────────────
        // Neovim's normal_get_additional_char() uses non-blocking vpeekc()
        // to accumulate composing chars already in the buffer, then exits
        // immediately if nothing's pending. Mirror that: if the parser
        // entered AwaitingComposingChars and no more keys are queued,
        // finalize the grapheme now instead of blocking.
        let action = if matches!(
            self.parser.state(),
            crate::grammar::InputState::AwaitingComposingChars { .. }
        ) && !self.has_pending_keys()
        {
            if let Some(result) = self.parser.try_finalize_composing() {
                ModeAction::Pipeline(result)
            } else {
                action
            }
        } else {
            action
        };

        // Neovim vungetc(): if composing finalization was triggered by a
        // non-combining key, the parser saved it for re-processing. Inject
        // it back into the typeahead so it's dispatched as a normal key.
        if let Some(reprocess) = self.parser.take_reprocess_key() {
            self.typeahead.buffer.inject_front_noremap(reprocess);
        }

        let mut response = self.execute_mode_action(action, mode, resolved_key, ctx);

        // Emit CursorShapeHint(None) when leaving operator-pending state.
        // The entering hint is emitted by the Pending handler in mode_dispatch.
        if was_operator_pending && response.kind != ResponseKind::Pending {
            response.effects.push(Effect::CursorShapeHint {
                pending_operator: None,
            });
        }

        // ── <silent> filtering ─────────────────────────────────────
        // If the current key came from a `<silent>` mapping expansion,
        // suppress ShowInfo effects. ShowError is NOT filtered — errors
        // are always shown, matching Vim behavior.
        let is_silent = resolved_flags.contains(TypeaheadFlags::SILENT)
            || drained_flags.is_some_and(|f| f.contains(TypeaheadFlags::SILENT));
        if is_silent {
            response
                .effects
                .retain(|e| !matches!(e, Effect::ShowInfo { .. }));
        }

        // ── Macro replay undo group merging ───────────────────────────
        // In Vim, N@a produces a single undo entry. During macro replay,
        // strip intermediate BeginUndoGroup/EndUndoGroup from the response
        // so the shell sees a single merged undo group. The engine's
        // internal undo tree already handles merging via begin_merge/end_merge.
        if self.state.undo_tree().is_merging() {
            response.effects.retain(|e| {
                !matches!(
                    e,
                    Effect::BeginUndoGroup { .. } | Effect::EndUndoGroup { .. }
                )
            });
        }

        if !self.fork_active {
            // ── Adaptive prediction weights ─────────────────────────
            // Update prediction weights with the key the user just typed.
            // Only for user-typed keys (not macro replay or mapping expansion)
            // and only when the response was actually consumed (the engine
            // did something meaningful with the keystroke).
            if is_typed_key && response.kind == super::response::ResponseKind::Consumed {
                let notation = resolved_key.to_vim_notation();
                self.prediction_weights.observe(&notation);
            }

            // ── Engine-side macro recording ────────��─────────────────
            // Record key only if:
            // 1. Recording was stable (active before AND after this key).
            //    StartRecording inits buffer during execute → was_recording=false → skip.
            //    StopRecording clears buffer during execute → buffer is None → skip.
            // 2. Key was user-typed (TYPED flag). Macro replay keys and mapping
            //    expansion RHS keys are NOT recorded — only the original user
            //    keystroke that triggered them is recorded.
            if was_recording && self.recording.buffer.is_some() && is_typed_key {
                self.record_key(key, &mut response);
            }

            response = self.register_pending_host_requests(response);

            // ── Effect pipeline ──────────────────────────────────────
            // Run middleware (logging, dedup, custom transforms) on effects
            // before they reach the host. Pipeline runs BEFORE session
            // recording so recorded effects match what the host actually sees.
            if let Some(ref pipeline) = self.cold.pipeline {
                pipeline.run(&mut response.effects);
            }

            // ── ModeChanged hook ─────────────────────────────────────
            // Fire when any effect in the response changed the editing mode.
            // Fires before CursorMoved and PostCommand so that mode-dependent
            // handlers see the transition before positional updates.
            // Both SetMode and BeginInsert represent mode transitions.
            {
                let has_mode_change = response
                    .effects
                    .iter()
                    .any(|e| matches!(e, Effect::SetMode { .. } | Effect::BeginInsert { .. }));
                if has_mode_change {
                    let hook_ctx = hooks::HookContext {
                        point: hooks::HookPoint::ModeChanged,
                        mode: self.state.mode(),
                        effects: &response.effects,
                        state: &self.state,
                    };
                    self.cold
                        .hooks
                        .fire(hooks::HookPoint::ModeChanged, &hook_ctx);
                }
            }

            // ── CursorMoved hook ────────────────────────────────────────
            // Fire when any effect in the response repositioned the cursor.
            // Fires after ModeChanged but before PostCommand so that
            // position-dependent handlers see the final cursor location.
            {
                let has_cursor_move = response
                    .effects
                    .iter()
                    .any(|e| matches!(e, Effect::SetCursor { .. }));
                if has_cursor_move {
                    let hook_ctx = hooks::HookContext {
                        point: hooks::HookPoint::CursorMoved,
                        mode: self.state.mode(),
                        effects: &response.effects,
                        state: &self.state,
                    };
                    self.cold
                        .hooks
                        .fire(hooks::HookPoint::CursorMoved, &hook_ctx);
                }
            }

            // ── PostCommand hook ─────────────────────────────────────
            // Fire registered PostCommand handlers after effect pipeline
            // (effects are finalized) but before session recording.
            // The result is informational — Cancel is not meaningful here
            // since effects have already been computed.
            // Suppressed when PreCommand cancelled — there is no "post" to
            // a command that never executed.
            if !response.precommand_cancelled {
                let hook_ctx = hooks::HookContext {
                    point: hooks::HookPoint::PostCommand,
                    mode: self.state.mode(),
                    effects: &response.effects,
                    state: &self.state,
                };
                self.cold
                    .hooks
                    .fire(hooks::HookPoint::PostCommand, &hook_ctx);
            }

            // ── Session recording ────────────────────────────────────
            // Record the key and its final effects for time-travel debugging.
            if let Some(ref mut recorder) = self.cold.recorder {
                recorder.record(key, &response.effects);
            }
        }

        // Shadow execution: run pending macro keys in-memory if enabled.
        // Update cursor/selection from the response effects so shadow replay
        // starts from the post-processing state, not the stale pre-snapshot.
        // (Text mutations already cause shadow to be skipped via the
        // is_text_mutation guard inside maybe_shadow_replay.)
        let mut shadow_cursor = shadow_cursor;
        let mut shadow_selection = shadow_selection;
        for effect in &response.effects {
            match effect {
                Effect::SetCursor { offset } => {
                    shadow_cursor = offset.get();
                }
                Effect::SetSelection { anchor, head, .. } => {
                    shadow_selection = Some(crate::primitives::SelectionRange::new(*anchor, *head));
                }
                Effect::ClearSelection => {
                    shadow_selection = None;
                }
                _ => {}
            }
        }
        self.maybe_shadow_replay(
            shadow_text_snapshot,
            shadow_cursor,
            shadow_selection,
            &mut response,
        );

        // ── Shadow document update ────────────────────────────────
        // Apply text mutation effects to the shadow document so it stays
        // current for the next process() call. Only Insert/Delete/Replace
        // are applied incrementally; Undo/Redo effects cannot be tracked
        // here — the drift gate on the next call will catch any mismatch.
        if !self.fork_active {
            self.update_shadow_from_effects(&response.effects);
        }

        // ── Federation state event extraction ────────────────────
        // After all effects are final (pipeline + shadow replay applied),
        // extract federable state events for cross-instance sharing.
        if !self.fork_active {
            response.state_events =
                super::federation_extractor::extract_events(&response.effects, &self.state);
        }

        trace_event!(
            self,
            TraceEvent::ProcessKeyDone {
                keystroke_seq: self.keystroke_seq,
                effect_count: response.effects.len(),
                consumed: response.consumed(),
            }
        );

        response
    }

    /// If shadow execution is enabled and macro keys are pending, execute them
    /// in-memory and merge the resulting effects into `response`.
    fn maybe_shadow_replay(
        &mut self,
        text_snapshot: Option<String>,
        cursor: usize,
        selection: Option<crate::primitives::SelectionRange>,
        response: &mut Response,
    ) {
        if !self.shadow_enabled || self.fork_active || !self.has_pending_keys() {
            return;
        }
        // Skip shadow during macro replay when drain_pending_keys is managing
        // the undo merge. Shadow would consume all remaining macro keys and
        // call end_merge(), corrupting the UndoStore group that
        // drain_pending_keys opened. Let the host drain handle it.
        if self.state.undo_tree().is_merging() {
            return;
        }
        // Skip shadow when the current key already produced text mutations
        // (e.g., mapping flush dispatched a pending key that inserted text).
        // The text snapshot was captured before those mutations, so shadow
        // would process remaining keys against a stale document baseline.
        // Let the host drain the remaining keys normally instead.
        if response
            .effects
            .iter()
            .any(crate::effects::Effect::is_text_mutation)
        {
            return;
        }
        // Skip shadow when the current key's response already has host requests.
        // This is a strong signal that remaining pending keys will also need host
        // interaction (e.g., every command-line keystroke produces SyncCommandLine).
        // Shadow would drain one key, process it via the real engine (mutating real
        // state), abort on the host request, and discard the request — causing the
        // alternating-key-drop bug where mapping expansion in command-line mode
        // loses every other character and the final <CR>'s CustomExCommand.
        if !response.host_requests.is_empty() {
            return;
        }
        let Some(initial_text) = text_snapshot else {
            return;
        };

        trace_event!(
            self,
            TraceEvent::ShadowEnter {
                pending_keys: self.typeahead.macro_stack.len(),
                text_len: initial_text.len(),
            }
        );

        let node_before_shadow = self.state.undo_tree().current();
        let result = self.execute_shadow_replay(initial_text, cursor, selection, None);
        // Shadow execution produces two separate lists:
        //   - other_effects: per-keystroke undo groups + UI/state effects
        //   - text_effects: a single coalesced diff (0 or 1 entries)
        //
        // The per-keystroke undo groups in other_effects DON'T wrap the
        // coalesced text effect — they wrapped the original per-keystroke
        // edits that were consumed by the shadow document. To fix this,
        // we strip undo group markers from other_effects and wrap the
        // text effects in a single undo group with the correct node_id.
        //
        // During macro replay, the engine strips undo group effects
        // (is_merging = true), so collecting node_id from effects yields
        // None. Instead, read the engine's undo tree current() after the
        // shadow replay — end_merge() already committed the merged node
        // inside drain_next_key() during the shadow loop.
        let mut last_node_id: Option<crate::primitives::NodeId> = None;
        // First try effects (works for non-macro shadow replay).
        for effect in &result.other_effects {
            if let Effect::EndUndoGroup { node_id: Some(id) } = effect {
                last_node_id = Some(*id);
            }
        }
        // Fallback: if effects had no node_id (macro merge stripped them),
        // use the undo tree's current node — but only if it actually advanced
        // (indicating the merge committed a new node with edits).
        if last_node_id.is_none() {
            let node_after = self.state.undo_tree().current();
            if node_after != node_before_shadow {
                last_node_id = Some(node_after);
            }
        }
        // Emit non-undo other_effects first (mode, registers, marks, etc.)
        response
            .effects
            .extend(result.other_effects.into_iter().filter(|e| {
                !matches!(
                    e,
                    Effect::BeginUndoGroup { .. } | Effect::EndUndoGroup { .. }
                )
            }));
        // Wrap text effects in a single undo group
        let shadow_text_changed = !result.text_effects.is_empty();
        if shadow_text_changed {
            response.effects.push(Effect::BeginUndoGroup {
                cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
            });
            response.effects.extend(result.text_effects);
            response.effects.push(Effect::EndUndoGroup {
                node_id: last_node_id,
            });
        }
        if let Some(cursor) = result.final_cursor {
            response
                .effects
                .push(Effect::set_cursor(Offset::new(cursor)));
        }

        trace_event!(
            self,
            TraceEvent::ShadowExit {
                keys_processed: 0, // exact count lives inside ShadowContext
                status: compact_str::CompactString::from(format!("{:?}", result.status)),
                text_changed: shadow_text_changed,
            }
        );
    }

    /// Process a mouse click at a target offset.
    ///
    /// Parallel to [`Self::process()`] — same pipeline, same `Response`,
    /// same shell code. The engine handles all Vim semantics:
    ///
    /// - **Jumplist**: saves current position for `Ctrl-O`.
    /// - **Parser reset**: cancels pending operators (`d`, `y`, etc.).
    /// - **Visual mode exit**: clicks return to Normal mode.
    /// - **Macro abort**: cancels any in-flight macro replay.
    ///
    /// The shell treats the `Response` identically to a key response.
    ///
    /// # Complexity
    ///
    /// Time: O(E) where E = number of effects produced (typically 2-3:
    /// SetCursor, possibly SetMode). Includes O(1) jumplist push, O(1)
    /// parser reset, and O(E) effect processing.
    ///
    /// Space: O(E) for the response effects vector.
    pub fn process_click<D: Document>(
        &mut self,
        target_offset: usize,
        ctx: &InputContext<'_, D, Validated>,
    ) -> Response {
        use crate::effects::Effects;

        self.sticky_session = None;
        self.state.clear_transient();
        self.state.set_current_buffer_id(ctx.buffer_id());

        // 1. Save current position to jumplist (Ctrl-O jumps back)
        self.state
            .jump_list_mut()
            .push(ctx.cursor_offset(), ctx.buffer_id());

        // 2. Cancel any pending operator / awaiting state + mapping expansion
        self.parser.reset();
        self.typeahead.buffer.clear();

        // 3. Abort all pending replay if active (click interrupts macros)
        if self.has_pending_keys() {
            self.abort_replay();
        }

        // 4. Build effects — cursor move + normalize to Normal mode
        let target = Offset::new(target_offset);
        let mut effects = Effects::new().set_cursor(target);

        // Only emit SetMode if we're actually leaving a non-Normal mode
        let mode = self.state.mode();
        if mode != Mode::Normal {
            effects = effects.set_mode(Mode::Normal);
        }

        let mut response = Response::with_effects(effects);

        // 5. Sync state through the same effect processor as keyboard input.
        // Pass document text for auto-emit SetStickyColumn (curswant).
        super::effect_processor::process_effects_with_text(
            &mut self.state,
            &mut self.parser,
            false,
            &mut response,
            Some(ctx.doc().text()),
            None,
            self.options.tabstop(),
            self.options.undo_auto_group_ms(),
            self.resolved_options.cursor_shape_overrides(),
        );

        // 6. Run effect pipeline (same as process())
        if let Some(ref pipeline) = self.cold.pipeline {
            pipeline.run(&mut response.effects);
        }

        response
    }

    /// Process a mouse selection (drag) at the given anchor and head byte offsets.
    ///
    /// Similar to [`process_click`](Self::process_click) but enters Visual mode
    /// with a selection instead of Normal mode with a cursor move. Handles the
    /// same cleanup: jumplist push, parser reset, mapping clear, macro abort.
    ///
    /// # Parameters
    /// - `anchor_offset`: byte offset where the drag started
    /// - `head_offset`: byte offset where the drag ended (cursor position)
    /// - `shape`: visual selection shape (Char, Line, Block)
    /// - `ctx`: validated input context for the current document
    ///
    /// # Complexity
    ///
    /// Time: O(E) where E = number of effects produced (typically 3-4:
    /// SetSelection, SetCursor, SetMode). Includes O(1) jumplist push,
    /// O(1) parser reset, and O(E) effect processing.
    ///
    /// Space: O(E) for the response effects vector.
    pub fn process_mouse_selection<D: Document>(
        &mut self,
        anchor_offset: usize,
        head_offset: usize,
        shape: SelectionShape,
        ctx: &InputContext<'_, D, Validated>,
    ) -> Response {
        use crate::effects::Effects;

        self.sticky_session = None;
        self.state.clear_transient();
        self.state.set_current_buffer_id(ctx.buffer_id());

        // 1. Save current position to jumplist (Ctrl-O jumps back)
        self.state
            .jump_list_mut()
            .push(ctx.cursor_offset(), ctx.buffer_id());

        // 2. Cancel any pending operator / awaiting state + mapping expansion
        self.parser.reset();
        self.typeahead.buffer.clear();

        // 3. Abort all pending replay if active (mouse interrupts macros)
        if self.has_pending_keys() {
            self.abort_replay();
        }

        // 4. Build effects — visual selection + mode transition
        let anchor = Offset::new(anchor_offset);
        let head = Offset::new(head_offset);
        let visual_type = VisualType::from(shape);
        let effects = Effects::new()
            .set_visual_selection(anchor, head, shape)
            .set_mode(Mode::Visual(visual_type));

        let mut response = Response::with_effects(effects);

        // 5. Sync state through the same effect processor as keyboard input.
        // Pass document text for auto-emit SetStickyColumn (curswant).
        super::effect_processor::process_effects_with_text(
            &mut self.state,
            &mut self.parser,
            false,
            &mut response,
            Some(ctx.doc().text()),
            None,
            self.options.tabstop(),
            self.options.undo_auto_group_ms(),
            self.resolved_options.cursor_shape_overrides(),
        );

        // 6. Run effect pipeline (same as process())
        if let Some(ref pipeline) = self.cold.pipeline {
            pipeline.run(&mut response.effects);
        }

        response
    }

    /// Find the last SetCursor offset in an effects list.
    fn last_cursor_in_effects(effects: &[Effect]) -> Option<Offset> {
        effects.iter().rev().find_map(|e| {
            if let Effect::SetCursor { offset } = e {
                Some(*offset)
            } else {
                None
            }
        })
    }

    /// Handle an `<expr>` mapping trigger by emitting a host request.
    ///
    /// The host evaluates the expression and returns the resulting key sequence
    /// via `complete_host_request()`, which feeds them into the typeahead.
    ///
    /// The `kind` is forwarded to the host request so that
    /// `complete_evaluate_mapping()` can inject the returned keys with the
    /// correct remapping behaviour: remappable for `:map <expr>`, noremap
    /// for `:noremap <expr>`.
    fn handle_expr_mapping(
        &mut self,
        expression: compact_str::CompactString,
        mapping_mode: crate::keymap::MappingMode,
        kind: crate::keymap::MappingKind,
        silent: bool,
    ) -> Response {
        let mut response = Response::pending_response();
        response
            .host_requests
            .push(super::HostRequest::EvaluateMapping {
                meta: self.host.sequencer.next_meta(),
                expression,
                mode: mapping_mode,
                kind,
                silent,
            });
        if self.fork_active {
            response
        } else {
            self.register_pending_host_requests(response)
        }
    }

    /// Normalize a key event for non-Latin keyboard layouts.
    ///
    /// When the host bridge provides a Latin command equivalent (`latin_key`),
    /// this replaces the key with its Latin equivalent in command-dispatch
    /// contexts (Normal/Visual/OP Ready state). In literal-char contexts
    /// (Insert, f/t/r arguments, CommandLine text), the original localized
    /// key is preserved so the user can type/find non-Latin characters.
    pub(crate) const fn normalize_for_layout(&self, key: KeyEvent) -> KeyEvent {
        let Some(latin) = key.latin_key() else {
            return key;
        };

        if self.expects_command_key() {
            KeyEvent::new(latin, key.modifiers())
        } else {
            key
        }
    }

    /// Whether langmap should be applied to this key.
    ///
    /// Returns `true` when the langmap table is non-empty and the engine is
    /// in a command-key context (Normal/Visual/OP-pending, parser not
    /// expecting a literal character argument).
    const fn should_apply_langmap(&self) -> bool {
        if self.langmap_table.is_empty() {
            return false;
        }
        if !self.expects_command_key() {
            return false;
        }
        true
    }

    /// Apply langmap (engine table) then normalize_for_layout (host fallback).
    ///
    /// Langmap takes priority: if the engine's langmap table remaps the key,
    /// the remapped version is used. Otherwise, the host-driven `latin_key`
    /// fallback in `normalize_for_layout` kicks in.
    fn apply_langmap_and_normalize(&self, key: KeyEvent) -> KeyEvent {
        let key = if self.should_apply_langmap() {
            self.langmap_table.remap_key_event(key)
        } else {
            key
        };
        self.normalize_for_layout(key)
    }

    /// Whether the engine is in a context that expects a command key
    /// (as opposed to a literal character).
    ///
    /// Returns `true` in Normal/Visual/OP modes when the parser is NOT
    /// awaiting a literal character (f/t/r target, surround char, sneak char).
    /// Returns `false` in Insert/Replace/CommandLine modes and when the
    /// parser expects a literal character argument.
    const fn expects_command_key(&self) -> bool {
        match self.state.mode() {
            Mode::Normal | Mode::Visual(_) | Mode::OperatorPending(_) => {
                !self.parser.state().expects_literal_char()
            }
            _ => false,
        }
    }

    /// If cursor is on the last char of a line, advance to the insert-mode
    /// "append" position (one past last char). Vim does this on `<C-o>$`.
    fn adjust_cursor_for_insert_eol(response: &mut Response, text: &str, pos: usize) {
        let bytes = text.as_bytes();
        if let Some(&ch) = bytes.get(pos) {
            if ch != b'\n' {
                let char_len = text
                    .get(pos..)
                    .and_then(|s| s.chars().next())
                    .map_or(1, char::len_utf8);
                let next = pos + char_len;
                let at_eol = next >= text.len() || bytes.get(next).copied() == Some(b'\n');
                if at_eol {
                    use crate::effects::Effects;
                    response.extend_effects(Effects::new().set_cursor(Offset::new(next)));
                }
            }
        }
    }

    /// Resolved (effective) options, after local overrides are applied.
    #[inline]
    #[must_use]
    pub const fn resolved_options(&self) -> &VimOptions {
        &self.resolved_options
    }

    /// Pre-populate the `+` (clipboard) and/or `*` (selection) register with
    /// the current system clipboard text. Called by the host before
    /// `process()` so that `p` with `clipboard=unnamedplus` reads fresh
    /// clipboard content via transparent register aliasing.
    pub fn sync_clipboard(&mut self, text: &str) {
        use crate::primitives::{MotionType, RegisterContent, RegisterName};

        let motion_type = if text.ends_with('\n') {
            MotionType::LineWise
        } else {
            MotionType::CharWise
        };
        let content = RegisterContent::new(text, motion_type);
        self.state
            .registers_mut()
            .set(RegisterName::CLIPBOARD, content.clone());
        self.state
            .registers_mut()
            .set(RegisterName::SELECTION, content);
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
