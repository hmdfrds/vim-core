//! Bidirectional name ↔ id registry for pseudo-key namespaces.
//!
//! Used by `<Plug>` and `<Action>` key variants to map human-readable
//! names to compact `u32` ids that preserve `Copy` on `Key`.
//!
//! The registry is append-only: ids are never reused. Registering the
//! same name twice returns the existing id (idempotent).

use ahash::AHashMap;
use compact_str::CompactString;

/// Bidirectional name ↔ id registry.
///
/// Names are interned: the first call to `register("foo")` allocates
/// id `0`, the second returns `0` again. This makes `Key::Plug(0)`
/// stable across multiple `register()` calls for the same name.
#[derive(Debug, Clone, Default)]
pub struct NameRegistry {
    /// id → name (dense, index = id)
    names: Vec<CompactString>,
    /// name → id (for dedup on register)
    lookup: AHashMap<CompactString, u32>,
}

impl NameRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a name and return its id.
    ///
    /// Idempotent: if the name already exists, returns the existing id.
    pub fn register(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.lookup.get(name) {
            return id;
        }
        let Ok(id) = u32::try_from(self.names.len()) else {
            debug_assert!(false, "name registry overflow: exceeded u32::MAX entries");
            // Saturate in release — practically unreachable.
            return u32::MAX;
        };
        let compact = CompactString::from(name);
        self.names.push(compact.clone());
        self.lookup.insert(compact, id);
        id
    }

    /// Look up the name for an id.
    #[must_use]
    pub fn get_name(&self, id: u32) -> Option<&str> {
        self.names.get(id as usize).map(CompactString::as_str)
    }

    /// Look up the id for a name.
    #[must_use]
    pub fn get_id(&self, name: &str) -> Option<u32> {
        self.lookup.get(name).copied()
    }

    /// Number of registered names.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.names.len()
    }

    /// Check if the registry is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_returns_sequential_ids() {
        let mut reg = NameRegistry::new();
        assert_eq!(reg.register("foo"), 0);
        assert_eq!(reg.register("bar"), 1);
        assert_eq!(reg.register("baz"), 2);
        assert_eq!(reg.len(), 3);
    }

    #[test]
    fn register_is_idempotent() {
        let mut reg = NameRegistry::new();
        let id1 = reg.register("foo");
        let id2 = reg.register("foo");
        assert_eq!(id1, id2);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn get_name_returns_registered_name() {
        let mut reg = NameRegistry::new();
        let id = reg.register("SearchForward");
        assert_eq!(reg.get_name(id), Some("SearchForward"));
    }

    #[test]
    fn get_name_returns_none_for_unknown_id() {
        let reg = NameRegistry::new();
        assert_eq!(reg.get_name(42), None);
    }

    #[test]
    fn get_id_returns_registered_id() {
        let mut reg = NameRegistry::new();
        reg.register("SearchForward");
        assert_eq!(reg.get_id("SearchForward"), Some(0));
    }

    #[test]
    fn get_id_returns_none_for_unknown_name() {
        let reg = NameRegistry::new();
        assert_eq!(reg.get_id("unknown"), None);
    }

    #[test]
    fn empty_registry() {
        let reg = NameRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }
}
