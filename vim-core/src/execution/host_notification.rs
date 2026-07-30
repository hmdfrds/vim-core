//! Host-to-engine notification channel.
//!
//! [`HostNotification`] provides a formal interface for hosts to inform the
//! engine of external state changes that affect Vim semantics but are not
//! text edits. This complements [`ExternalEdit`](super::ExternalEdit) which
//! handles text mutations.

use crate::primitives::SelectionRange;
use compact_str::CompactString;
use smallvec::SmallVec;

/// Notification from the host about external state changes.
///
/// Hosts send these via
/// [`VimEngine::process_notification()`](crate::execution::VimEngine::process_notification)
/// to inform the
/// engine of events that originate outside the Vim command loop. The engine
/// may emit effects in response (e.g., breaking an undo group on focus loss).
///
/// # Distinction from `ExternalEdit`
///
/// `HostNotification` covers non-text-mutating state changes (focus, clipboard
/// generation, selection changes). Text mutations are still reported via
/// `apply_external_edit()`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostNotification {
    /// The editor window gained input focus.
    FocusGained,
    /// The editor window lost input focus.
    ///
    /// In insert mode, the engine breaks the current undo group so that
    /// focus-loss acts as a natural undo boundary.
    FocusLost,
    /// The host's selection changed outside of Vim commands.
    ///
    /// For example: mouse selection, drag-select, or IDE "select all occurrences".
    /// The engine may update its internal selection tracking.
    SelectionChangedExternally {
        /// The new selection ranges (primary + any secondary cursors).
        ranges: SmallVec<[SelectionRange; 1]>,
    },
    /// The system clipboard content changed externally.
    ///
    /// Hosts send this when they detect the clipboard was modified by another
    /// application. The engine updates its clipboard generation counter so
    /// paste operations know to request a fresh clipboard read.
    ClipboardChanged {
        /// Monotonically increasing generation counter from the host.
        generation: u64,
    },
    /// Host configuration was reloaded (e.g., settings file changed on disk).
    ///
    /// The engine may invalidate caches or re-derive computed state.
    ConfigReloaded,
    /// A timer previously requested via `Effect::RequestTimer` has fired.
    ///
    /// The host sends this when the requested delay has elapsed. The engine
    /// uses the `id` to determine what action to take (e.g., emit
    /// `VimEvent::CursorHold` for the CursorHold timer).
    TimerFired {
        /// The timer ID that was passed in `Effect::RequestTimer`.
        id: u32,
    },
    /// The visible viewport changed (e.g., the user scrolled).
    ///
    /// Hosts send this to keep the engine's viewport state in sync so that
    /// screen-relative motions (H/M/L) and lazy rendering work correctly.
    ViewportChanged {
        /// First visible line (0-indexed).
        first_line: usize,
        /// Number of visible lines.
        height: usize,
    },
    /// The buffer's file type changed (e.g., language mode switched).
    ///
    /// Triggers filetype-specific key mappings and may emit a
    /// `VimEvent::FileTypeChanged` in the future.
    FileTypeChanged {
        /// The new filetype identifier (e.g., "rust", "python").
        filetype: CompactString,
    },
    /// External diagnostic state was updated (e.g., LSP diagnostics).
    ///
    /// Informs the engine of the current diagnostic count so it can
    /// expose this via variables or status-line expressions.
    DiagnosticsUpdated {
        /// Total number of active diagnostics.
        count: usize,
    },
    /// The host applied a completion item.
    ///
    /// Fired after the host inserts a completion candidate. May emit
    /// a `VimEvent::CompletionDone` in the future.
    CompletionDone {
        /// The text of the completed item.
        item: CompactString,
    },
    /// The terminal/editor window was resized.
    ///
    /// Updates the engine's terminal size for column/row-dependent
    /// computations (e.g., `'columns'`, `'lines'` options).
    WindowResized {
        /// New width in columns.
        cols: usize,
        /// New height in rows.
        rows: usize,
    },

    /// Asynchronous event from the host to be drained at the start of
    /// the next `process_key()` cycle.
    ///
    /// Hosts enqueue these for events that must not interrupt mid-command
    /// processing (e.g., LSP responses, background task completions).
    /// The engine drains the queue at a safe interleaving point.
    AsyncEvent {
        /// Opaque event identifier. The engine routes this to the
        /// appropriate handler based on the id.
        id: u32,
        /// Optional payload (stringified for simplicity).
        payload: CompactString,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_fired_variant_exists() {
        let n = HostNotification::TimerFired { id: 1 };
        assert_eq!(n, HostNotification::TimerFired { id: 1 });
        assert_ne!(n, HostNotification::TimerFired { id: 2 });
    }

    #[test]
    fn timer_fired_debug() {
        let debug = format!("{:?}", HostNotification::TimerFired { id: 42 });
        assert!(debug.contains("TimerFired"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn viewport_changed_variant() {
        let n = HostNotification::ViewportChanged {
            first_line: 10,
            height: 30,
        };
        assert_eq!(
            n,
            HostNotification::ViewportChanged {
                first_line: 10,
                height: 30
            }
        );
        assert_ne!(
            n,
            HostNotification::ViewportChanged {
                first_line: 0,
                height: 30
            }
        );
        let debug = format!("{n:?}");
        assert!(debug.contains("ViewportChanged"));
    }

    #[test]
    fn filetype_changed_variant() {
        let n = HostNotification::FileTypeChanged {
            filetype: CompactString::from("rust"),
        };
        assert_eq!(
            n,
            HostNotification::FileTypeChanged {
                filetype: CompactString::from("rust")
            }
        );
        assert_ne!(
            n,
            HostNotification::FileTypeChanged {
                filetype: CompactString::from("python")
            }
        );
    }

    #[test]
    fn diagnostics_updated_variant() {
        let n = HostNotification::DiagnosticsUpdated { count: 5 };
        assert_eq!(n, HostNotification::DiagnosticsUpdated { count: 5 });
        assert_ne!(n, HostNotification::DiagnosticsUpdated { count: 0 });
    }

    #[test]
    fn completion_done_variant() {
        let n = HostNotification::CompletionDone {
            item: CompactString::from("println!"),
        };
        assert_eq!(
            n,
            HostNotification::CompletionDone {
                item: CompactString::from("println!")
            }
        );
    }

    #[test]
    fn window_resized_variant() {
        let n = HostNotification::WindowResized {
            cols: 120,
            rows: 40,
        };
        assert_eq!(
            n,
            HostNotification::WindowResized {
                cols: 120,
                rows: 40
            }
        );
        assert_ne!(n, HostNotification::WindowResized { cols: 80, rows: 24 });
    }

    #[test]
    fn async_event_variant() {
        let n = HostNotification::AsyncEvent {
            id: 42,
            payload: CompactString::from("test_payload"),
        };
        assert_eq!(
            n,
            HostNotification::AsyncEvent {
                id: 42,
                payload: CompactString::from("test_payload")
            }
        );
        assert_ne!(
            n,
            HostNotification::AsyncEvent {
                id: 99,
                payload: CompactString::from("other")
            }
        );
        let debug = format!("{n:?}");
        assert!(debug.contains("AsyncEvent"));
        assert!(debug.contains("42"));
    }
}
