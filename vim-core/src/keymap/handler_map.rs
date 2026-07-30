//! Per-key, per-mode handler delegation (`:sethandler`).
//!
//! Implements IdeaVim's `sethandler` concept: each key can be delegated
//! to either `Vim` or `Host` on a per-mode basis. When a key's handler
//! is `Host`, the engine returns `Ignored` and the host handles it natively.
//!
//! # Example
//!
//! ```ignore
//! // Ctrl-A: Vim handles it in normal mode, host in insert mode
//! handler_map.set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Vim);
//! handler_map.set(KeyEvent::ctrl('a'), MappingMode::Insert, Handler::Host);
//! ```

use super::{KeyEvent, MappingMode};
use ahash::AHashMap;

/// Who handles a particular key in a particular mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Handler {
    /// The Vim engine handles this key (default).
    #[default]
    Vim,
    /// The host editor handles this key natively.
    /// The engine will return `ResponseKind::Ignored`.
    Host,
}

/// Per-key, per-mode handler delegation map.
///
/// Sparse storage: only keys with explicit handler assignments are stored.
/// Keys not in the map default to `Handler::Vim`.
#[derive(Debug, Clone, Default)]
pub struct HandlerMap {
    /// (key, mode) → handler
    entries: AHashMap<(KeyEvent, MappingMode), Handler>,
}

impl HandlerMap {
    /// Create an empty handler map (all keys default to Vim).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the handler for a key in a specific mode.
    pub fn set(&mut self, key: KeyEvent, mode: MappingMode, handler: Handler) {
        if handler == Handler::Vim {
            // Remove entry to save memory — Vim is the default
            self.entries.remove(&(key, mode));
        } else {
            self.entries.insert((key, mode), handler);
        }
    }

    /// Set the handler for a key across all modes at once.
    pub fn set_all_modes(&mut self, key: KeyEvent, handler: Handler) {
        for mm in MappingMode::ALL {
            self.set(key, mm, handler);
        }
    }

    /// Get the handler for a key in a specific mode.
    ///
    /// Returns `Handler::Vim` for keys not explicitly set.
    #[must_use]
    pub fn get(&self, key: KeyEvent, mode: MappingMode) -> Handler {
        self.entries
            .get(&(key, mode))
            .copied()
            .unwrap_or(Handler::Vim)
    }

    /// Check if a key is handled by the host in the given mode.
    #[must_use]
    pub fn is_host_handled(&self, key: KeyEvent, mode: MappingMode) -> bool {
        self.get(key, mode) == Handler::Host
    }

    /// Remove handler assignment for a key in a specific mode (reverts to Vim).
    pub fn remove(&mut self, key: KeyEvent, mode: MappingMode) {
        self.entries.remove(&(key, mode));
    }

    /// Remove all handler assignments for a key across all modes.
    pub fn remove_all_modes(&mut self, key: KeyEvent) {
        for mm in MappingMode::ALL {
            self.entries.remove(&(key, mm));
        }
    }

    /// Clear all handler assignments.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Check if there are any explicit handler assignments.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of explicit handler assignments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_vim() {
        let map = HandlerMap::new();
        assert_eq!(
            map.get(KeyEvent::ctrl('a'), MappingMode::Normal),
            Handler::Vim
        );
        assert!(!map.is_host_handled(KeyEvent::ctrl('a'), MappingMode::Normal));
    }

    #[test]
    fn set_host_handler() {
        let mut map = HandlerMap::new();
        map.set(KeyEvent::ctrl('a'), MappingMode::Insert, Handler::Host);

        assert!(map.is_host_handled(KeyEvent::ctrl('a'), MappingMode::Insert));
        assert!(!map.is_host_handled(KeyEvent::ctrl('a'), MappingMode::Normal));
    }

    #[test]
    fn set_vim_removes_entry() {
        let mut map = HandlerMap::new();
        map.set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Host);
        assert_eq!(map.len(), 1);

        map.set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Vim);
        assert!(map.is_empty());
    }

    #[test]
    fn set_all_modes() {
        let mut map = HandlerMap::new();
        map.set_all_modes(KeyEvent::ctrl('c'), Handler::Host);

        for mm in MappingMode::ALL {
            assert!(map.is_host_handled(KeyEvent::ctrl('c'), mm));
        }
    }

    #[test]
    fn per_mode_override() {
        let mut map = HandlerMap::new();
        // Ctrl-A: host in insert, vim in normal
        map.set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Vim);
        map.set(KeyEvent::ctrl('a'), MappingMode::Insert, Handler::Host);

        assert!(!map.is_host_handled(KeyEvent::ctrl('a'), MappingMode::Normal));
        assert!(map.is_host_handled(KeyEvent::ctrl('a'), MappingMode::Insert));
    }

    #[test]
    fn remove_reverts_to_vim() {
        let mut map = HandlerMap::new();
        map.set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Host);
        map.remove(KeyEvent::ctrl('a'), MappingMode::Normal);

        assert!(!map.is_host_handled(KeyEvent::ctrl('a'), MappingMode::Normal));
        assert!(map.is_empty());
    }

    #[test]
    fn remove_all_modes_clears_key() {
        let mut map = HandlerMap::new();
        map.set_all_modes(KeyEvent::ctrl('c'), Handler::Host);
        assert_eq!(map.len(), MappingMode::COUNT);

        map.remove_all_modes(KeyEvent::ctrl('c'));
        assert!(map.is_empty());
    }

    #[test]
    fn clear_empties_map() {
        let mut map = HandlerMap::new();
        map.set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Host);
        map.set(KeyEvent::ctrl('b'), MappingMode::Insert, Handler::Host);

        map.clear();
        assert!(map.is_empty());
    }
}
