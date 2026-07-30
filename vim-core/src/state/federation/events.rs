//! State events for cross-instance federation.
//!
//! [`StateEvent`] describes a discrete state change that should be propagated
//! to other editor instances. [`EventSource`] identifies the origin of the change
//! so that receivers can avoid re-broadcasting their own events.

use super::backend::GlobalMark;
use crate::primitives::{MarkName, Mode, Offset, RegisterContent, RegisterName, SearchDirection};
use compact_str::CompactString;

/// Origin of a state change event.
///
/// Used by federation consumers to distinguish locally-produced events (which
/// should be forwarded to peers) from events received from peers (which should
/// be applied locally but not re-broadcast to avoid loops).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSource {
    /// Event originated from a user action in this editor instance.
    Local,
    /// Event was received from another federated editor instance.
    Federated,
    /// Event was loaded from the persistence backend (startup restore).
    Backend,
}

/// A discrete Vim state change that can be propagated across instances.
///
/// Each variant represents one category of state that is meaningful to share
/// across editor sessions. Text-buffer contents are NOT included — only
/// editor-level state (registers, marks, history, mode, macros) is federated.
#[derive(Debug)]
#[non_exhaustive]
pub enum StateEvent {
    /// A register's content changed.
    RegisterChanged {
        /// The register whose content changed.
        name: RegisterName,
        /// The new register content.
        content: RegisterContent,
        /// Where this change originated.
        source: EventSource,
    },

    /// A global mark (A-Z) was set or updated.
    GlobalMarkChanged {
        /// The global mark name (always A-Z for federated marks).
        name: MarkName,
        /// The new mark value with file context.
        mark: GlobalMark,
        /// Where this change originated.
        source: EventSource,
    },

    /// A local (buffer-local) mark was set or updated.
    ///
    /// Local marks are primarily useful for same-session synchronisation
    /// (e.g., multi-cursor extensions); they carry no file context.
    LocalMarkChanged {
        /// The local mark name.
        name: MarkName,
        /// Byte offset of the new mark position.
        offset: Offset,
        /// Where this change originated.
        source: EventSource,
    },

    /// The current search pattern or direction changed.
    SearchPatternChanged {
        /// The new search pattern string.
        pattern: CompactString,
        /// The search direction.
        direction: SearchDirection,
        /// Where this change originated.
        source: EventSource,
    },

    /// The editor mode changed (e.g., Normal → Insert).
    ModeChanged {
        /// The mode that was active before the transition.
        from: Mode,
        /// The mode that is now active.
        to: Mode,
    },

    /// Macro recording started or stopped.
    MacroRecordingChanged {
        /// `Some(register)` when recording started, `None` when stopped.
        register: Option<RegisterName>,
    },

    /// A new entry was added to the jump list.
    JumpAdded {
        /// The byte offset that was pushed onto the jump list.
        offset: Offset,
        /// A hint identifying the file (path or buffer name), if known.
        file_hint: Option<CompactString>,
    },

    /// The undo tree branched at a specific point.
    ///
    /// Carries the `node_id` of the last committed undo group, which
    /// can be used to correlate undo state across instances.
    UndoBranched {
        /// The `NodeId` of the undo tree node at the branch point.
        branch_point: u64,
    },
}
