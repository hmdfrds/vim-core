//! Sub-coordinator structs for [`VimEngine`].
//!
//! Each coordinator bundles a pair of tightly-coupled fields under a single
//! named concern, reducing the top-level [`VimEngine`] struct from 12 fields
//! to 9 while keeping each group's purpose explicit.
//!
//! Fields use `pub(in crate::execution::engine)` so all `impl VimEngine`
//! sub-modules (siblings in the `engine` module tree) can access them
//! directly without additional accessor boilerplate.

use super::macro_replay::MacroFrame;
use super::typeahead::{TypeaheadBuffer, TypeaheadFlags};
use crate::execution::host::{HostRequestId, HostRequestSequencer};
use crate::execution::HostRequest;
use crate::primitives::RegisterName;

/// Bundles host-request sequencing and the pending-request tracking map.
pub(super) struct HostCoordinator {
    /// Deterministic request-id generator.
    pub(in crate::execution::engine) sequencer: HostRequestSequencer,
    /// Outstanding host requests awaiting shell completion, keyed by ID
    /// for out-of-order completion.
    pub(in crate::execution::engine) pending: ahash::AHashMap<HostRequestId, HostRequest>,
}

impl HostCoordinator {
    pub(super) fn new() -> Self {
        Self {
            sequencer: HostRequestSequencer::default(),
            pending: ahash::AHashMap::new(),
        }
    }
}

/// Unified key replay: typeahead buffer with per-key remap flags and
/// macro replay frame stack for nested `@{reg}` invocations.
#[derive(Clone)]
pub(super) struct TypeaheadCoordinator {
    /// Unified typeahead buffer with per-key remap flags and mapping resolution.
    pub(in crate::execution::engine) buffer: TypeaheadBuffer,
    /// Macro replay frame stack for nested `@{reg}` invocations.
    pub(in crate::execution::engine) macro_stack: Vec<MacroFrame>,
    /// Flags from the most recent `drain_next_key()` call.
    ///
    /// When `drain_next_key()` returns a key, its flags are stored here.
    /// The next `process()` call consumes them via `take_last_drained_flags()`
    /// to determine whether the key was user-typed or replayed, gating
    /// recording and `is_live_insert_input()` accordingly.
    ///
    /// Initialized to `empty()` (no drain has occurred). Reset to `empty()`
    /// after `take_last_drained_flags()` consumes them.
    pub(in crate::execution::engine) last_drained_flags: TypeaheadFlags,
    /// Running count of macro entries pumped during the current replay.
    pub(in crate::execution::engine) macro_effect_counter: usize,
}

impl TypeaheadCoordinator {
    pub(super) fn new() -> Self {
        Self {
            buffer: TypeaheadBuffer::new(),
            macro_stack: Vec::new(),
            last_drained_flags: TypeaheadFlags::empty(),
            macro_effect_counter: 0,
        }
    }
}

/// Macro recording state, separated from replay.
///
/// Recording is independent of the typeahead buffer and macro replay stack.
/// It just accumulates keystrokes into a register buffer while `q{reg}` is active.
#[derive(Clone)]
pub(super) struct RecordingState {
    /// In-progress macro recording `(register, keystrokes)`.
    pub(in crate::execution::engine) buffer: Option<(RegisterName, String)>,
}

impl RecordingState {
    pub(super) const fn new() -> Self {
        Self { buffer: None }
    }
}
