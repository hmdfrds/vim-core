//! Event registry (autocmd system).
//!
//! Provides typed autocommand registration with discriminant-based event matching,
//! priority ordering, and one-shot support. Handlers are Rust function
//! pointers registered directly on the engine.

use compact_str::CompactString;

use crate::primitives::{AutocmdId, BufferId, Mode, VimEvent};

/// Filter that narrows when an autocmd fires.
///
/// Autocmds can be scoped to a specific buffer, file pattern, or mode.
/// When a filter is present, the autocmd only fires if the filter matches
/// the current context.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AutocmdFilter {
    /// Only fire for the specified buffer.
    Buffer(BufferId),
    /// Only fire when the current file matches this glob pattern.
    FilePattern(CompactString),
    /// Only fire when in the specified mode.
    Mode(Mode),
}

/// Context provided to autocmd handlers when an event fires.
///
/// Contains the triggering event plus optional contextual information
/// about the buffer and cursor position at the time of firing.
#[derive(Debug, Clone)]
pub struct EventContext {
    /// The event that triggered this autocmd.
    pub event: VimEvent,
    /// The buffer in which the event occurred, if applicable.
    pub buffer_id: Option<BufferId>,
    /// The byte offset of the cursor when the event occurred, if applicable.
    pub trigger_offset: Option<usize>,
}

/// How an autocmd handler is stored in the registry.
///
/// Rust handlers are function pointers registered directly on the engine.
#[derive(Debug, Clone)]
pub enum StoredAutocmdHandler {
    /// A Rust function pointer handler (registered pre-session by the host).
    Rust {
        /// Human-readable label for debugging/listing.
        label: CompactString,
    },
}

/// A registered autocmd subscription.
///
/// Contains all metadata needed to match an incoming event and dispatch
/// to the correct handler.
#[derive(Debug, Clone)]
pub struct AutocmdRegistration {
    /// Unique identifier for this registration.
    pub id: AutocmdId,
    /// The event discriminant to match against (variant identity without payload).
    pub event_discriminant: std::mem::Discriminant<VimEvent>,
    /// Optional filter to narrow when this autocmd fires.
    pub filter: Option<AutocmdFilter>,
    /// Execution priority (lower values fire first).
    pub priority: i16,
    /// Whether this autocmd should be removed after firing once.
    pub once: bool,
    /// The handler to invoke when the autocmd fires.
    pub handler: StoredAutocmdHandler,
}

/// Registry that holds all autocmd subscriptions.
///
/// Maintains a sorted list of registrations (by priority) and provides
/// methods to subscribe, unsubscribe, and query matching registrations
/// for a given event.
#[derive(Debug, Clone, Default)]
pub struct EventRegistry {
    /// All registered autocmds, sorted by priority.
    subscriptions: Vec<AutocmdRegistration>,
    /// Monotonically increasing counter for generating unique IDs.
    next_id: u64,
}

impl EventRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new autocmd subscription.
    ///
    /// The `event` parameter is used only to extract its discriminant for
    /// matching — the payload data is ignored. Returns a unique [`AutocmdId`]
    /// that can be used to unsubscribe later.
    ///
    /// Registrations are kept sorted by priority (lower values first).
    pub fn subscribe(
        &mut self,
        event: &VimEvent,
        filter: Option<AutocmdFilter>,
        priority: i16,
        once: bool,
        handler: StoredAutocmdHandler,
    ) -> AutocmdId {
        let id = AutocmdId(self.next_id);
        self.next_id += 1;
        self.subscriptions.push(AutocmdRegistration {
            id,
            event_discriminant: std::mem::discriminant(event),
            filter,
            priority,
            once,
            handler,
        });
        // Maintain sort by priority
        self.subscriptions.sort_by_key(|r| r.priority);
        id
    }

    /// Remove a registration by its ID.
    ///
    /// Returns `true` if a registration was found and removed.
    pub fn unsubscribe(&mut self, id: AutocmdId) -> bool {
        let len_before = self.subscriptions.len();
        self.subscriptions.retain(|r| r.id != id);
        self.subscriptions.len() < len_before
    }

    /// Find all registrations that match the given event by discriminant.
    ///
    /// Returns registrations in priority order (lowest first). The caller
    /// is responsible for applying [`AutocmdFilter`] checks against the
    /// current context.
    #[must_use]
    pub fn matching(&self, event: &VimEvent) -> Vec<&AutocmdRegistration> {
        let disc = std::mem::discriminant(event);
        self.subscriptions
            .iter()
            .filter(|r| r.event_discriminant == disc)
            .collect()
    }

    /// Remove registrations that were marked `once` and have fired.
    ///
    /// Called after event dispatch to clean up one-shot autocmds.
    /// Only removes registrations whose IDs are in `fired_ids` AND whose
    /// `once` flag is set.
    pub fn remove_once_fired(&mut self, fired_ids: &[AutocmdId]) {
        self.subscriptions
            .retain(|r| !(r.once && fired_ids.contains(&r.id)));
    }

    /// List all registrations matching a given event (alias for [`Self::matching`]).
    #[must_use]
    pub fn list_for_event(&self, event: &VimEvent) -> Vec<&AutocmdRegistration> {
        self.matching(event)
    }

    /// Return the total number of registered autocmds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.subscriptions.len()
    }

    /// Return whether the registry is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.subscriptions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Mode, VimEvent};

    #[test]
    fn subscribe_and_unsubscribe() {
        let mut registry = EventRegistry::new();
        assert!(registry.is_empty());

        let id = registry.subscribe(
            &VimEvent::InsertEnter,
            None,
            0,
            false,
            StoredAutocmdHandler::Rust {
                label: "test".into(),
            },
        );

        assert_eq!(registry.len(), 1);
        assert!(registry.unsubscribe(id));
        assert!(registry.is_empty());
    }

    #[test]
    fn unsubscribe_nonexistent_returns_false() {
        let mut registry = EventRegistry::new();
        assert!(!registry.unsubscribe(AutocmdId(999)));
    }

    #[test]
    fn matching_by_discriminant() {
        let mut registry = EventRegistry::new();

        registry.subscribe(
            &VimEvent::InsertEnter,
            None,
            0,
            false,
            StoredAutocmdHandler::Rust {
                label: "insert".into(),
            },
        );
        registry.subscribe(
            &VimEvent::CursorMoved,
            None,
            0,
            false,
            StoredAutocmdHandler::Rust {
                label: "cursor".into(),
            },
        );

        let matches = registry.matching(&VimEvent::InsertEnter);
        assert_eq!(matches.len(), 1);

        let matches = registry.matching(&VimEvent::CursorMoved);
        assert_eq!(matches.len(), 1);

        let matches = registry.matching(&VimEvent::InsertLeave);
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn discriminant_matching_ignores_payload() {
        let mut registry = EventRegistry::new();

        // Subscribe with one mode change variant
        registry.subscribe(
            &VimEvent::ModeChanged {
                from: Mode::Normal,
                to: Mode::Insert,
            },
            None,
            0,
            false,
            StoredAutocmdHandler::Rust {
                label: "mode".into(),
            },
        );

        // Should match ANY ModeChanged regardless of from/to values
        let matches = registry.matching(&VimEvent::ModeChanged {
            from: Mode::Insert,
            to: Mode::Normal,
        });
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn priority_ordering() {
        let mut registry = EventRegistry::new();

        let id_low = registry.subscribe(
            &VimEvent::InsertEnter,
            None,
            10,
            false,
            StoredAutocmdHandler::Rust {
                label: "low".into(),
            },
        );
        let id_high = registry.subscribe(
            &VimEvent::InsertEnter,
            None,
            -5,
            false,
            StoredAutocmdHandler::Rust {
                label: "high".into(),
            },
        );
        let id_mid = registry.subscribe(
            &VimEvent::InsertEnter,
            None,
            0,
            false,
            StoredAutocmdHandler::Rust {
                label: "mid".into(),
            },
        );

        let matches = registry.matching(&VimEvent::InsertEnter);
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].id, id_high);
        assert_eq!(matches[1].id, id_mid);
        assert_eq!(matches[2].id, id_low);
    }

    #[test]
    fn once_flag_removal() {
        let mut registry = EventRegistry::new();

        let once_id = registry.subscribe(
            &VimEvent::InsertEnter,
            None,
            0,
            true, // once
            StoredAutocmdHandler::Rust {
                label: "once".into(),
            },
        );
        let persist_id = registry.subscribe(
            &VimEvent::InsertEnter,
            None,
            0,
            false, // persistent
            StoredAutocmdHandler::Rust {
                label: "persist".into(),
            },
        );

        // Simulate firing both
        registry.remove_once_fired(&[once_id, persist_id]);

        // Only the once handler should be removed
        assert_eq!(registry.len(), 1);
        let remaining = registry.matching(&VimEvent::InsertEnter);
        assert_eq!(remaining[0].id, persist_id);
    }

    #[test]
    fn event_context_construction() {
        let ctx = EventContext {
            event: VimEvent::BufEnter,
            buffer_id: Some(BufferId::new(42)),
            trigger_offset: Some(100),
        };
        assert_eq!(ctx.event, VimEvent::BufEnter);
        assert_eq!(ctx.buffer_id, Some(BufferId::new(42)));
        assert_eq!(ctx.trigger_offset, Some(100));
    }

    #[test]
    fn unique_ids_increment() {
        let mut registry = EventRegistry::new();

        let id1 = registry.subscribe(
            &VimEvent::InsertEnter,
            None,
            0,
            false,
            StoredAutocmdHandler::Rust { label: "a".into() },
        );
        let id2 = registry.subscribe(
            &VimEvent::InsertLeave,
            None,
            0,
            false,
            StoredAutocmdHandler::Rust { label: "b".into() },
        );

        assert_ne!(id1, id2);
        assert_eq!(id1.0 + 1, id2.0);
    }
}
