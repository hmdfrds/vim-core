//! Variable store for g: and b: scoped variables.
//!
//! Provides a [`VariableStore`] that holds global and buffer-local variables,
//! with save/restore support for buffer switches.

use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::primitives::{VarScope, VimValue};

/// Maximum number of variables allowed per scope.
/// Prevents unbounded memory growth from buggy or malicious scripts.
const MAX_VARIABLES_PER_SCOPE: usize = 10_000;

/// Storage for Vim variables in global (g:) and buffer-local (b:) scopes.
///
/// Buffer-local variables are saved/restored during buffer switches via
/// [`take_buffer_vars`](Self::take_buffer_vars) and
/// [`restore_buffer_vars`](Self::restore_buffer_vars).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VariableStore {
    /// Global (g:) variables, shared across all buffers.
    global: BTreeMap<CompactString, VimValue>,
    /// Buffer-local (b:) variables, scoped to the active buffer.
    buffer: BTreeMap<CompactString, VimValue>,
}

impl VariableStore {
    /// Get a variable by scope and name.
    #[inline]
    #[must_use]
    pub fn get(&self, scope: VarScope, name: &str) -> Option<&VimValue> {
        match scope {
            VarScope::Global => self.global.get(name),
            VarScope::Buffer => self.buffer.get(name),
        }
    }

    /// Set a variable in the given scope.
    ///
    /// If the scope already contains `MAX_VARIABLES_PER_SCOPE` variables
    /// and the key is new, the insertion is silently ignored to prevent
    /// unbounded memory growth.
    #[inline]
    pub fn set(&mut self, scope: VarScope, name: &str, value: VimValue) {
        let map = match scope {
            VarScope::Global => &mut self.global,
            VarScope::Buffer => &mut self.buffer,
        };
        if map.len() >= MAX_VARIABLES_PER_SCOPE && !map.contains_key(name) {
            return;
        }
        map.insert(CompactString::from(name), value);
    }

    /// Maximum number of variables per scope before new insertions are ignored.
    #[inline]
    #[must_use]
    pub const fn max_variables_per_scope() -> usize {
        MAX_VARIABLES_PER_SCOPE
    }

    /// Delete a variable from the given scope. Returns `true` if it existed.
    #[inline]
    pub fn delete(&mut self, scope: VarScope, name: &str) -> bool {
        let map = match scope {
            VarScope::Global => &mut self.global,
            VarScope::Buffer => &mut self.buffer,
        };
        map.remove(name).is_some()
    }

    /// Check whether a variable exists in the given scope.
    #[inline]
    #[must_use]
    pub fn exists(&self, scope: VarScope, name: &str) -> bool {
        match scope {
            VarScope::Global => self.global.contains_key(name),
            VarScope::Buffer => self.buffer.contains_key(name),
        }
    }

    /// Iterate over all variables in the given scope.
    #[inline]
    pub fn list(&self, scope: VarScope) -> impl Iterator<Item = (&str, &VimValue)> {
        let map = match scope {
            VarScope::Global => &self.global,
            VarScope::Buffer => &self.buffer,
        };
        map.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Clear all buffer-local variables (called on buffer leave).
    #[inline]
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Take all buffer-local variables, leaving the store's buffer map empty.
    ///
    /// Used by `on_buffer_leave` to extract buffer variables for later restore.
    #[inline]
    pub fn take_buffer_vars(&mut self) -> BTreeMap<CompactString, VimValue> {
        std::mem::take(&mut self.buffer)
    }

    /// Restore previously saved buffer-local variables.
    ///
    /// Used by `on_buffer_enter` to reinstate a buffer's variables.
    #[inline]
    pub fn restore_buffer_vars(&mut self, vars: BTreeMap<CompactString, VimValue>) {
        self.buffer = vars;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_global() {
        let mut store = VariableStore::default();
        store.set(VarScope::Global, "foo", VimValue::Int(42));
        assert_eq!(store.get(VarScope::Global, "foo"), Some(&VimValue::Int(42)));
    }

    #[test]
    fn set_and_get_buffer() {
        let mut store = VariableStore::default();
        store.set(
            VarScope::Buffer,
            "bar",
            VimValue::String(CompactString::from("hello")),
        );
        assert_eq!(
            store.get(VarScope::Buffer, "bar"),
            Some(&VimValue::String(CompactString::from("hello")))
        );
    }

    #[test]
    fn scope_isolation() {
        let mut store = VariableStore::default();
        store.set(VarScope::Global, "x", VimValue::Int(1));
        store.set(VarScope::Buffer, "x", VimValue::Int(2));
        assert_eq!(store.get(VarScope::Global, "x"), Some(&VimValue::Int(1)));
        assert_eq!(store.get(VarScope::Buffer, "x"), Some(&VimValue::Int(2)));
    }

    #[test]
    fn delete_existing() {
        let mut store = VariableStore::default();
        store.set(VarScope::Global, "foo", VimValue::Bool(true));
        assert!(store.delete(VarScope::Global, "foo"));
        assert!(!store.exists(VarScope::Global, "foo"));
    }

    #[test]
    fn delete_nonexistent() {
        let mut store = VariableStore::default();
        assert!(!store.delete(VarScope::Global, "nope"));
    }

    #[test]
    fn overwrite() {
        let mut store = VariableStore::default();
        store.set(VarScope::Global, "x", VimValue::Int(1));
        store.set(VarScope::Global, "x", VimValue::Int(99));
        assert_eq!(store.get(VarScope::Global, "x"), Some(&VimValue::Int(99)));
    }

    #[test]
    fn exists() {
        let mut store = VariableStore::default();
        assert!(!store.exists(VarScope::Buffer, "z"));
        store.set(VarScope::Buffer, "z", VimValue::Nil);
        assert!(store.exists(VarScope::Buffer, "z"));
    }

    #[test]
    fn list_scope() {
        let mut store = VariableStore::default();
        store.set(VarScope::Global, "a", VimValue::Int(1));
        store.set(VarScope::Global, "b", VimValue::Int(2));
        store.set(VarScope::Buffer, "c", VimValue::Int(3));

        let global_vars: Vec<_> = store.list(VarScope::Global).collect();
        assert_eq!(global_vars.len(), 2);

        let buffer_vars: Vec<_> = store.list(VarScope::Buffer).collect();
        assert_eq!(buffer_vars.len(), 1);
    }

    #[test]
    fn take_and_restore_buffer_vars() {
        let mut store = VariableStore::default();
        store.set(VarScope::Buffer, "x", VimValue::Int(10));
        store.set(VarScope::Global, "g", VimValue::Int(20));

        let saved = store.take_buffer_vars();
        assert!(store.get(VarScope::Buffer, "x").is_none());
        assert_eq!(store.get(VarScope::Global, "g"), Some(&VimValue::Int(20)));

        store.restore_buffer_vars(saved);
        assert_eq!(store.get(VarScope::Buffer, "x"), Some(&VimValue::Int(10)));
    }

    #[test]
    fn clear_buffer() {
        let mut store = VariableStore::default();
        store.set(VarScope::Buffer, "a", VimValue::Int(1));
        store.set(VarScope::Buffer, "b", VimValue::Int(2));
        store.set(VarScope::Global, "g", VimValue::Int(3));

        store.clear_buffer();
        assert!(!store.exists(VarScope::Buffer, "a"));
        assert!(!store.exists(VarScope::Buffer, "b"));
        assert!(store.exists(VarScope::Global, "g"));
    }
}
