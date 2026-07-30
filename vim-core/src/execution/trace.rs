//! Engine observability: structured trace events for debugging and profiling.
//!
//! Feature-gated behind `engine-tracing`. When the feature is off, the
//! `trace_event!` macro compiles to nothing and `TraceCollector` is absent.

// ─────────────────────────────────────────────────────────────────────────────
// TraceEvent
// ─────────────────────────────────────────────────────────────────────────────

/// A structured event emitted by the engine during processing.
///
/// Collected into the engine's internal `TraceCollector` when `engine-tracing` is enabled and
/// tracing is active. Designed for post-hoc analysis, debugging UIs, and
/// integration-test assertions.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TraceEvent {
    /// A key was submitted to `VimEngine::process()`.
    ProcessKey {
        /// Debug representation of the key event.
        key: compact_str::CompactString,
        /// Display name of the current mode.
        mode: compact_str::CompactString,
        /// Monotonic keystroke counter at entry.
        keystroke_seq: u64,
    },
    /// `process()` is about to return.
    ProcessKeyDone {
        /// Monotonic keystroke counter.
        keystroke_seq: u64,
        /// Number of effects in the response.
        effect_count: usize,
        /// Whether the key was consumed by the engine.
        consumed: bool,
    },
    /// Shadow execution entered.
    ShadowEnter {
        /// Number of pending keys at entry.
        pending_keys: usize,
        /// Document length in bytes at entry.
        text_len: usize,
    },
    /// Shadow execution exited.
    ShadowExit {
        /// Number of keys processed during shadow.
        keys_processed: usize,
        /// Outcome status description.
        status: compact_str::CompactString,
        /// Whether the document text changed.
        text_changed: bool,
    },
    /// Host-side undo group opened.
    UndoBeginGroup {
        /// Cursor offset at group open.
        cursor: usize,
        /// Document length at group open.
        text_len: usize,
    },
    /// Host-side undo group closed.
    UndoEndGroup {
        /// Undo tree node ID, if a node was committed.
        node_id: Option<u64>,
        /// Cursor offset at group close.
        cursor: usize,
    },
    /// Macro-replay undo merge started.
    UndoMergeBegin {
        /// Register being replayed.
        register: char,
    },
    /// Macro-replay undo merge ended.
    UndoMergeEnd,
    /// `drain_pending_keys` entered.
    DrainPendingStart {
        /// Number of pending keys at entry.
        pending_count: usize,
        /// Whether undo merging is active.
        is_merging: bool,
    },
    /// A single key drained during `drain_pending_keys`.
    DrainPendingKey {
        /// Debug representation of the drained key.
        key: compact_str::CompactString,
        /// Zero-based iteration index within this drain.
        iteration: usize,
    },
    /// `drain_pending_keys` finished.
    DrainPendingDone {
        /// Total number of keys drained.
        iterations: usize,
    },
    /// Macro recording state changed.
    MacroRecord {
        /// Target register.
        register: char,
        /// Action description (e.g. "start", "stop").
        action: compact_str::CompactString,
    },
    /// Macro replay initiated.
    MacroReplay {
        /// Register being replayed.
        register: char,
        /// Repeat count.
        count: u32,
    },
    /// Mode changed.
    ModeTransition {
        /// Display name of the previous mode.
        from: compact_str::CompactString,
        /// Display name of the new mode.
        to: compact_str::CompactString,
    },
    /// An effect was emitted.
    EffectEmitted {
        /// Effect variant name.
        kind: compact_str::CompactString,
        /// Short description of the effect payload.
        detail: compact_str::CompactString,
    },
    /// Free-form diagnostic message.
    Diagnostic {
        /// The diagnostic text.
        message: compact_str::CompactString,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// InspectSnapshot
// ─────────────────────────────────────────────────────────────────────────────

/// A point-in-time snapshot of engine + host state for observability.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InspectSnapshot {
    /// Current editing mode (e.g. "NORMAL", "INSERT").
    pub mode: compact_str::CompactString,
    /// Monotonic keystroke counter.
    pub keystroke_seq: u64,
    /// Whether the engine has pending keys (typeahead/macro).
    pub has_pending_keys: bool,
    /// Whether a macro is currently being recorded.
    pub is_recording: bool,
    /// Whether the undo tree is in merge mode (macro replay).
    pub is_merging: bool,
    /// Number of nodes in the undo tree.
    pub undo_node_count: usize,
    /// Current undo node ID.
    pub undo_current_node: u64,
    /// Parser pending-command display string.
    pub pending_command: compact_str::CompactString,
    /// Whether shadow execution is enabled.
    pub shadow_enabled: bool,
    /// Document length in bytes (filled by `VimSession`).
    pub document_len: usize,
    /// Cursor byte offset (filled by `VimSession`).
    pub cursor_offset: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// TraceCollector (feature-gated)
// ─────────────────────────────────────────────────────────────────────────────

/// Collects [`TraceEvent`]s during engine processing.
///
/// Only present when `feature = "engine-tracing"` is enabled.
#[cfg(feature = "engine-tracing")]
pub(in crate::execution) struct TraceCollector {
    events: Vec<TraceEvent>,
    pub(in crate::execution) enabled: bool,
}

#[cfg(feature = "engine-tracing")]
impl TraceCollector {
    /// Create a new empty collector (disabled by default).
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            enabled: false,
        }
    }

    /// Push an event into the collector.
    #[allow(dead_code, reason = "called by trace_event! macro in emit points")]
    pub fn push(&mut self, event: TraceEvent) {
        self.events.push(event);
    }

    /// Drain all collected events, returning them and leaving the collector empty.
    pub fn drain(&mut self) -> Vec<TraceEvent> {
        std::mem::take(&mut self.events)
    }

    /// Enable or disable event collection.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Whether event collection is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// trace_event! macro
// ─────────────────────────────────────────────────────────────────────────────

/// Emit a [`TraceEvent`] into the engine's trace collector.
///
/// When `engine-tracing` is off, this compiles to nothing.
#[allow(
    unused_macros,
    reason = "used by emit points in engine.rs and host_session.rs"
)]
#[cfg(feature = "engine-tracing")]
macro_rules! trace_event {
    ($engine:expr, $event:expr) => {
        if $engine.cold.trace.enabled {
            $engine.cold.trace.push($event);
        }
    };
}

#[allow(
    unused_macros,
    reason = "used by emit points in engine.rs and host_session.rs"
)]
#[cfg(not(feature = "engine-tracing"))]
macro_rules! trace_event {
    ($engine:expr, $event:expr) => {};
}

#[allow(
    unused_imports,
    reason = "used by emit points in engine.rs and host_session.rs"
)]
pub(crate) use trace_event;
