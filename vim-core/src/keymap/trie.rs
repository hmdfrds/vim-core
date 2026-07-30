//! Prefix-tree for efficient multi-key mapping lookup.
//!
//! Replaces `AHashMap<KeyEvent, KeySequence>` with a trie that supports:
//! - Multi-key LHS (e.g. `jk`, `<Leader>w`)
//! - 3-way lookup: `NoMatch` / `ExactOnly` / `Prefix`
//! - `recursive` flag per entry (for `map` vs `noremap`)

use super::{KeyEvent, MappingFlags, MappingKind, MappingOwner};
use ahash::AHashMap;
use compact_str::CompactString;

// ═══════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════

/// A single mapping's target data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingEntry {
    /// The replacement key sequence (RHS). Uses `Vec` instead of `ArrayVec`
    /// because RHS can be arbitrarily long (e.g., `:GodotBreakpoint<CR>` = 17 keys).
    ///
    /// When `flags.expr` is `true`, this is empty — the expression text is
    /// stored in `expression` instead.
    pub(crate) sequence: Vec<KeyEvent>,
    /// Whether this is a recursive mapping (`:map`) or non-recursive (`:noremap`).
    pub(crate) kind: MappingKind,
    /// Consolidated boolean attribute flags (`<nowait>`, `<silent>`, `<expr>`).
    pub(crate) flags: MappingFlags,
    /// The expression text for `<expr>` mappings.
    ///
    /// When `flags.expr` is `true`, this holds the raw RHS string that the
    /// host must evaluate. The host returns the resulting key sequence,
    /// which is then fed into the typeahead buffer.
    pub(crate) expression: Option<CompactString>,
    /// Who owns this mapping (for lifecycle management and conflict reporting).
    pub(crate) owner: MappingOwner,
    /// Optional human-readable description for which-key display.
    pub(crate) description: Option<CompactString>,
}

impl MappingEntry {
    /// Create a new mapping entry with default flags.
    #[must_use]
    pub const fn new(sequence: Vec<KeyEvent>, kind: MappingKind) -> Self {
        Self {
            sequence,
            kind,
            flags: MappingFlags::NONE,
            expression: None,
            owner: MappingOwner::User,
            description: None,
        }
    }

    /// Create a new mapping entry with explicit flags.
    #[must_use]
    pub const fn with_flags(
        sequence: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
        expression: Option<CompactString>,
    ) -> Self {
        Self {
            sequence,
            kind,
            flags,
            expression,
            owner: MappingOwner::User,
            description: None,
        }
    }

    /// Create a new mapping entry with the `<nowait>` flag set.
    #[must_use]
    pub const fn new_nowait(sequence: Vec<KeyEvent>, kind: MappingKind) -> Self {
        Self {
            sequence,
            kind,
            flags: MappingFlags {
                nowait: true,
                silent: false,
                expr: false,
                unique: false,
                script_local: false,
            },
            expression: None,
            owner: MappingOwner::User,
            description: None,
        }
    }

    /// Create a new mapping entry with the `<silent>` flag set.
    #[must_use]
    pub const fn new_silent(sequence: Vec<KeyEvent>, kind: MappingKind) -> Self {
        Self {
            sequence,
            kind,
            flags: MappingFlags {
                nowait: false,
                silent: true,
                expr: false,
                unique: false,
                script_local: false,
            },
            expression: None,
            owner: MappingOwner::User,
            description: None,
        }
    }

    /// Create a new `<expr>` mapping entry.
    ///
    /// The `expression` text is evaluated by the host at expansion time.
    /// The `sequence` field is empty for expr mappings.
    #[must_use]
    pub const fn new_expr(expression: CompactString, kind: MappingKind) -> Self {
        Self {
            sequence: Vec::new(),
            kind,
            flags: MappingFlags {
                nowait: false,
                silent: false,
                expr: true,
                unique: false,
                script_local: false,
            },
            expression: Some(expression),
            owner: MappingOwner::User,
            description: None,
        }
    }

    /// Create a new `<expr>` mapping entry with `<nowait>`.
    #[must_use]
    pub const fn new_expr_nowait(expression: CompactString, kind: MappingKind) -> Self {
        Self {
            sequence: Vec::new(),
            kind,
            flags: MappingFlags {
                nowait: true,
                silent: false,
                expr: true,
                unique: false,
                script_local: false,
            },
            expression: Some(expression),
            owner: MappingOwner::User,
            description: None,
        }
    }

    /// Set the mapping owner (for host lifecycle management).
    #[must_use]
    pub fn with_owner(mut self, owner: MappingOwner) -> Self {
        self.owner = owner;
        self
    }

    /// Set an optional human-readable description for which-key display.
    #[must_use]
    pub fn with_description(mut self, description: Option<CompactString>) -> Self {
        self.description = description;
        self
    }

    /// The owner of this mapping.
    #[inline]
    #[must_use]
    pub const fn owner(&self) -> &MappingOwner {
        &self.owner
    }

    /// The human-readable description, if set.
    #[inline]
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The replacement key sequence (RHS).
    #[inline]
    #[must_use]
    pub fn sequence(&self) -> &[KeyEvent] {
        &self.sequence
    }

    /// Whether this is a recursive mapping (`:map`) or non-recursive (`:noremap`).
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> MappingKind {
        self.kind
    }

    /// The consolidated attribute flags for this mapping.
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> MappingFlags {
        self.flags
    }

    /// Whether this mapping has the `<nowait>` flag set.
    ///
    /// When `true`, the trie returns `ExactOnly` instead of `Prefix` even if
    /// longer mappings share this prefix — the mapping fires immediately.
    #[inline]
    #[must_use]
    pub const fn nowait(&self) -> bool {
        self.flags.nowait
    }

    /// Whether this mapping has the `<expr>` flag set.
    ///
    /// When `true`, the RHS is an expression string that must be evaluated
    /// by the host at expansion time. The resulting key sequence is then
    /// fed into the typeahead buffer.
    #[inline]
    #[must_use]
    pub const fn expr(&self) -> bool {
        self.flags.expr
    }

    /// Whether this mapping has the `<silent>` flag set.
    ///
    /// When `true`, `ShowMessage` effects are suppressed during the mapping's
    /// execution. `ShowError` effects are NOT suppressed (errors are always
    /// shown, matching Vim behavior).
    #[inline]
    #[must_use]
    pub const fn silent(&self) -> bool {
        self.flags.silent
    }

    /// Whether this mapping has the `<unique>` flag set.
    ///
    /// When `true`, attempting to overwrite this mapping from a different
    /// [`MappingOwner`] is an error (E227). Same-owner overwrites are allowed.
    #[inline]
    #[must_use]
    pub const fn unique(&self) -> bool {
        self.flags.unique
    }

    /// Whether this mapping has the `<script>` flag set.
    ///
    /// When `true`, remapping during expansion only considers mappings from
    /// the same [`MappingOwner`]. Mappings from other owners are skipped.
    #[inline]
    #[must_use]
    pub const fn script_local(&self) -> bool {
        self.flags.script_local
    }

    /// The expression text for `<expr>` mappings.
    ///
    /// Returns `Some` when `expr()` is `true`, `None` otherwise.
    #[inline]
    #[must_use]
    pub fn expression(&self) -> Option<&str> {
        self.expression.as_deref()
    }
}

/// Result of looking up a key sequence prefix in the trie.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrieLookup<'a> {
    /// No prefix of the input matches any mapping.
    NoMatch,
    /// Exact match found, no longer mappings share this prefix.
    /// The mapping can be executed immediately with no ambiguity.
    ExactOnly(&'a MappingEntry),
    /// The input is a prefix of at least one longer mapping.
    /// If `exact` is `Some`, the input also exactly matches a shorter mapping
    /// (ambiguous — needs timeout to resolve).
    Prefix {
        /// The exact match at this prefix length, if one exists.
        exact: Option<&'a MappingEntry>,
    },
}

// ═══════════════════════════════════════════════════════════════════════
// Internal node
// ═══════════════════════════════════════════════════════════════════════

/// A node in the mapping prefix tree.
#[derive(Debug, Clone, Default)]
struct TrieNode {
    /// If this node completes a mapping, the target.
    entry: Option<MappingEntry>,
    /// Children keyed by the next key in the sequence.
    children: AHashMap<KeyEvent, Self>,
}

// ═══════════════════════════════════════════════════════════════════════
// MappingTrie
// ═══════════════════════════════════════════════════════════════════════

/// Prefix-tree for efficient multi-key mapping lookup.
///
/// Supports insert, remove, clear, and 3-way prefix lookup.
/// Zero-allocation for empty tries (root node has no children).
///
/// # Example
///
/// ```ignore
/// let mut trie = MappingTrie::default();
/// let entry = MappingEntry::new(key_sequence(&[KeyEvent::escape()]), MappingKind::NonRecursive);
/// trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], entry);
///
/// // After pressing 'j', trie reports a prefix match (waiting for 'k')
/// assert!(matches!(trie.lookup(&[KeyEvent::char('j')]), TrieLookup::Prefix { exact: None }));
///
/// // After pressing 'jk', trie reports exact match
/// assert!(matches!(trie.lookup(&[KeyEvent::char('j'), KeyEvent::char('k')]), TrieLookup::ExactOnly(_)));
/// ```
#[derive(Debug, Clone, Default)]
pub struct MappingTrie {
    root: TrieNode,
    /// Total number of mappings in the trie.
    len: usize,
}

impl MappingTrie {
    /// Insert a mapping. Overwrites any existing entry at the same LHS.
    ///
    /// # Complexity
    ///
    /// Time: O(k) where k = `lhs.len()` (key sequence length). Each key
    /// requires an O(1) amortized hash map `entry().or_default()` on the
    /// trie node's children map.
    ///
    /// Space: O(k) in the worst case (new intermediate nodes created for
    /// each key in the sequence). Shared prefixes reuse existing nodes.
    pub fn insert(&mut self, lhs: &[KeyEvent], entry: MappingEntry) {
        debug_assert!(!lhs.is_empty(), "mapping LHS must not be empty");
        if lhs.is_empty() {
            return;
        }

        let mut node = &mut self.root;
        for key in lhs {
            node = node.children.entry(*key).or_default();
        }

        if node.entry.is_none() {
            self.len += 1;
        }
        node.entry = Some(entry);
    }

    /// Remove a mapping. Returns the removed entry if it existed.
    ///
    /// After removal, prunes any orphan intermediate nodes bottom-up so
    /// that `lookup()` never returns `Prefix{None}` for dead prefixes.
    ///
    /// # Complexity
    ///
    /// Time: O(k) where k = `lhs.len()` (key sequence length). Walks down
    /// the trie to depth k (O(1) hash lookup per level), then prunes back
    /// up (O(1) hash remove per level for empty nodes).
    ///
    /// Space: O(k) stack frames for the recursive walk.
    pub fn remove(&mut self, lhs: &[KeyEvent]) -> Option<MappingEntry> {
        debug_assert!(!lhs.is_empty(), "mapping LHS must not be empty");
        if lhs.is_empty() {
            return None;
        }

        let removed = Self::remove_recursive(&mut self.root, lhs, 0);
        if removed.is_some() {
            self.len -= 1;
        }
        removed
    }

    /// Recursive helper: walk to depth `idx` in `lhs`, remove the entry,
    /// then prune the child on the way back up if it became empty.
    fn remove_recursive(node: &mut TrieNode, lhs: &[KeyEvent], idx: usize) -> Option<MappingEntry> {
        // Safety: caller guarantees idx < lhs.len() on first call,
        // and we only recurse with idx+1 when idx < lhs.len().
        let key = lhs.get(idx)?;
        let child = node.children.get_mut(key)?;

        let removed = if idx + 1 == lhs.len() {
            // Reached the target node — take its entry
            child.entry.take()
        } else {
            // Recurse deeper
            Self::remove_recursive(child, lhs, idx + 1)
        };

        // Prune on the way back up: if the child has no entry and no
        // children, it's an orphan — remove it from the HashMap.
        if removed.is_some() && child.entry.is_none() && child.children.is_empty() {
            node.children.remove(key);
        }

        removed
    }

    /// Clear all mappings.
    ///
    /// # Complexity
    ///
    /// Time: O(T) where T = total number of trie nodes (all nodes are dropped).
    ///
    /// Space: O(1)
    pub fn clear(&mut self) {
        self.root = TrieNode::default();
        self.len = 0;
    }

    /// Look up a key sequence prefix.
    ///
    /// Returns a 3-way result indicating whether the prefix:
    /// - Doesn't match anything (`NoMatch`)
    /// - Exactly matches a mapping with no longer alternatives (`ExactOnly`)
    /// - Is a prefix of longer mapping(s), possibly with an exact match too (`Prefix`)
    ///
    /// # Complexity
    ///
    /// Time: O(k) where k = `prefix.len()` (key sequence length). Each key
    /// requires an O(1) amortized hash map lookup on the trie node's children.
    ///
    /// Space: O(1)
    #[must_use]
    pub fn lookup(&self, prefix: &[KeyEvent]) -> TrieLookup<'_> {
        if prefix.is_empty() {
            return TrieLookup::NoMatch;
        }

        let mut node = &self.root;
        for key in prefix {
            match node.children.get(key) {
                Some(child) => node = child,
                None => return TrieLookup::NoMatch,
            }
        }

        let has_children = !node.children.is_empty();
        match (&node.entry, has_children) {
            (Some(entry), false) => TrieLookup::ExactOnly(entry),
            // When the entry has <nowait>, treat it as an unambiguous exact match
            // even though longer mappings share this prefix.
            (Some(entry), true) if entry.nowait() => TrieLookup::ExactOnly(entry),
            (exact, true) => TrieLookup::Prefix {
                exact: exact.as_ref(),
            },
            (None, false) => {
                // Defensive: remove() now prunes orphan nodes, so this
                // arm should be unreachable in normal operation. Kept
                // for safety in case external code constructs a trie
                // with empty leaf nodes.
                TrieLookup::NoMatch
            }
        }
    }

    /// Get a single-key exact mapping (backward compatibility).
    ///
    /// Returns `Some` only for `ExactOnly` or `Prefix { exact: Some }` matches.
    /// This is the fast path used by `classify()` for single-key lookups.
    ///
    /// # Complexity
    ///
    /// Time: O(1) amortized — single hash map lookup on the root's children.
    ///
    /// Space: O(1)
    #[must_use]
    pub fn get_single(&self, key: KeyEvent) -> Option<&MappingEntry> {
        let node = self.root.children.get(&key)?;
        node.entry.as_ref()
    }

    /// Get the exact entry for a multi-key sequence, if it exists.
    ///
    /// Unlike `get_single`, this walks the full key path. Returns `Some`
    /// only if the final node has an entry (not merely a prefix).
    #[must_use]
    pub fn get_exact(&self, lhs: &[KeyEvent]) -> Option<&MappingEntry> {
        if lhs.is_empty() {
            return None;
        }
        let mut node = &self.root;
        for key in lhs {
            node = node.children.get(key)?;
        }
        node.entry.as_ref()
    }

    /// Check if the trie contains any mappings.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of total mappings in the trie.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Remove all mappings owned by `owner`.
    ///
    /// Walks the entire trie recursively:
    /// - At each node, if the node's entry matches `owner`, removes it.
    /// - After recursing into all children, prunes child nodes that have
    ///   no entry and no children of their own (orphan nodes).
    ///
    /// Returns the number of mappings removed.
    ///
    /// # Complexity
    ///
    /// Time: O(T) where T = total number of trie nodes (full DFS traversal).
    ///
    /// Space: O(d) where d = maximum trie depth (recursion stack).
    pub fn remove_by_owner(&mut self, owner: &MappingOwner) -> usize {
        let removed = Self::remove_by_owner_recursive(&mut self.root, owner);
        self.len -= removed;
        removed
    }

    /// Recursive helper: remove entries matching `owner`, prune orphan nodes.
    ///
    /// Returns the count of entries removed in this subtree.
    fn remove_by_owner_recursive(node: &mut TrieNode, owner: &MappingOwner) -> usize {
        let mut count = 0;

        // Remove this node's entry if it matches the target owner.
        if node.entry.as_ref().is_some_and(|e| e.owner() == owner) {
            node.entry = None;
            count += 1;
        }

        // Recurse into all children, collecting keys of orphaned children.
        let keys: Vec<KeyEvent> = node.children.keys().copied().collect();
        for key in keys {
            if let Some(child) = node.children.get_mut(&key) {
                count += Self::remove_by_owner_recursive(child, owner);
            }
            // Prune the child if it is now an orphan (no entry, no children).
            if let Some(child) = node.children.get(&key) {
                if child.entry.is_none() && child.children.is_empty() {
                    node.children.remove(&key);
                }
            }
        }

        count
    }

    /// Iterate over all mappings in the trie.
    ///
    /// Yields `(lhs, entry)` pairs where `lhs` is the reconstructed key
    /// sequence. Used for `:map` listing / introspection.
    ///
    /// Returns a collected `Vec` rather than a lazy iterator because:
    /// - User mapping counts are small (typically <100)
    /// - HashMap child ordering is non-deterministic, making a streaming
    ///   iterator complex without benefit
    /// - Callers (`:map` listing) need all results at once anyway
    ///
    /// # Complexity
    ///
    /// Time: O(T) where T = total number of trie nodes (DFS traversal).
    /// Each node is visited exactly once.
    ///
    /// Space: O(M * k_avg + d) where M = number of mappings (output vec),
    /// k_avg = average LHS length (cloned per entry), and d = maximum trie
    /// depth (DFS stack).
    #[must_use]
    pub fn entries(&self) -> Vec<(Vec<KeyEvent>, &MappingEntry)> {
        let mut results = Vec::with_capacity(self.len);
        let mut stack: Vec<KeyEvent> = Vec::new();
        Self::collect_entries(&self.root, &mut stack, &mut results);
        results
    }

    /// DFS helper to collect all `(lhs, entry)` pairs.
    fn collect_entries<'a>(
        node: &'a TrieNode,
        prefix: &mut Vec<KeyEvent>,
        out: &mut Vec<(Vec<KeyEvent>, &'a MappingEntry)>,
    ) {
        if let Some(ref entry) = node.entry {
            out.push((prefix.clone(), entry));
        }
        for (key, child) in &node.children {
            prefix.push(*key);
            Self::collect_entries(child, prefix, out);
            prefix.pop();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::key_sequence;

    fn entry(keys: &[KeyEvent]) -> MappingEntry {
        MappingEntry::new(key_sequence(keys), MappingKind::NonRecursive)
    }

    fn entry_recursive(keys: &[KeyEvent]) -> MappingEntry {
        MappingEntry::new(key_sequence(keys), MappingKind::Recursive)
    }

    #[test]
    fn insert_and_lookup_single() {
        let mut trie = MappingTrie::default();
        let e = entry(&[KeyEvent::char('x')]);
        trie.insert(&[KeyEvent::char('j')], e.clone());

        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::ExactOnly(&e)
        );
        assert_eq!(trie.len(), 1);
        assert!(!trie.is_empty());
    }

    #[test]
    fn insert_and_lookup_multi() {
        let mut trie = MappingTrie::default();
        let e = entry(&[KeyEvent::escape()]);
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], e.clone());

        assert_eq!(
            trie.lookup(&[KeyEvent::char('j'), KeyEvent::char('k')]),
            TrieLookup::ExactOnly(&e),
        );
    }

    #[test]
    fn prefix_detection() {
        let mut trie = MappingTrie::default();
        trie.insert(
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            entry(&[KeyEvent::escape()]),
        );

        // 'j' alone is a prefix of 'jk'
        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::Prefix { exact: None },
        );
    }

    #[test]
    fn ambiguous_prefix() {
        let mut trie = MappingTrie::default();
        let e_short = entry(&[KeyEvent::char('x')]);
        let e_long = entry(&[KeyEvent::char('y')]);

        // Both 'j' and 'jk' are mapped
        trie.insert(&[KeyEvent::char('j')], e_short.clone());
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], e_long.clone());

        // 'j' is ambiguous: exact match exists, but longer prefix 'jk' also exists
        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::Prefix {
                exact: Some(&e_short)
            },
        );

        // 'jk' is unambiguous
        assert_eq!(
            trie.lookup(&[KeyEvent::char('j'), KeyEvent::char('k')]),
            TrieLookup::ExactOnly(&e_long),
        );

        assert_eq!(trie.len(), 2);
    }

    #[test]
    fn no_match() {
        let trie = MappingTrie::default();
        assert_eq!(trie.lookup(&[KeyEvent::char('x')]), TrieLookup::NoMatch);
        assert!(trie.is_empty());
    }

    #[test]
    fn no_match_empty_prefix() {
        let mut trie = MappingTrie::default();
        trie.insert(&[KeyEvent::char('j')], entry(&[KeyEvent::char('x')]));
        assert_eq!(trie.lookup(&[]), TrieLookup::NoMatch);
    }

    #[test]
    fn remove_single() {
        let mut trie = MappingTrie::default();
        let e = entry(&[KeyEvent::char('x')]);
        trie.insert(&[KeyEvent::char('j')], e.clone());

        let removed = trie.remove(&[KeyEvent::char('j')]);
        assert_eq!(removed, Some(e));
        assert_eq!(trie.lookup(&[KeyEvent::char('j')]), TrieLookup::NoMatch);
        assert!(trie.is_empty());
    }

    #[test]
    fn remove_multi() {
        let mut trie = MappingTrie::default();
        trie.insert(
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            entry(&[KeyEvent::escape()]),
        );

        trie.remove(&[KeyEvent::char('j'), KeyEvent::char('k')]);
        assert_eq!(
            trie.lookup(&[KeyEvent::char('j'), KeyEvent::char('k')]),
            TrieLookup::NoMatch,
        );
        // After pruning, the intermediate 'j' node should also be removed.
        // lookup(&[j]) must return NoMatch, not Prefix{None}.
        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::NoMatch,
            "intermediate nodes must be pruned after remove",
        );
    }

    #[test]
    fn remove_preserves_siblings() {
        let mut trie = MappingTrie::default();
        let e_a = entry(&[KeyEvent::char('x')]);
        let e_b = entry(&[KeyEvent::char('y')]);

        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('a')], e_a.clone());
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('b')], e_b.clone());

        trie.remove(&[KeyEvent::char('j'), KeyEvent::char('a')]);

        assert_eq!(
            trie.lookup(&[KeyEvent::char('j'), KeyEvent::char('a')]),
            TrieLookup::NoMatch,
        );
        assert_eq!(
            trie.lookup(&[KeyEvent::char('j'), KeyEvent::char('b')]),
            TrieLookup::ExactOnly(&e_b),
        );
        assert_eq!(trie.len(), 1);
    }

    #[test]
    fn clear_empties_everything() {
        let mut trie = MappingTrie::default();
        trie.insert(&[KeyEvent::char('a')], entry(&[KeyEvent::char('x')]));
        trie.insert(
            &[KeyEvent::char('b'), KeyEvent::char('c')],
            entry(&[KeyEvent::char('y')]),
        );

        trie.clear();
        assert!(trie.is_empty());
        assert_eq!(trie.len(), 0);
        assert_eq!(trie.lookup(&[KeyEvent::char('a')]), TrieLookup::NoMatch);
    }

    #[test]
    fn recursive_flag_preserved() {
        let mut trie = MappingTrie::default();
        let e = entry_recursive(&[KeyEvent::char('x')]);
        trie.insert(&[KeyEvent::char('j')], e.clone());

        match trie.lookup(&[KeyEvent::char('j')]) {
            TrieLookup::ExactOnly(found) => {
                assert!(found.kind().is_recursive());
                assert_eq!(found, &e);
            }
            other => panic!("expected ExactOnly, got {other:?}"),
        }
    }

    #[test]
    fn overwrite_entry() {
        let mut trie = MappingTrie::default();
        let e1 = entry(&[KeyEvent::char('x')]);
        let e2 = entry(&[KeyEvent::char('y')]);

        trie.insert(&[KeyEvent::char('j')], e1);
        trie.insert(&[KeyEvent::char('j')], e2.clone());

        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::ExactOnly(&e2)
        );
        assert_eq!(trie.len(), 1); // overwrite, not double-count
    }

    #[test]
    fn get_single_convenience() {
        let mut trie = MappingTrie::default();
        let e = entry(&[KeyEvent::char('x')]);
        trie.insert(&[KeyEvent::char('j')], e.clone());

        assert_eq!(trie.get_single(KeyEvent::char('j')), Some(&e));
        assert_eq!(trie.get_single(KeyEvent::char('k')), None);
    }

    #[test]
    fn get_single_returns_some_even_with_children() {
        let mut trie = MappingTrie::default();
        let e = entry(&[KeyEvent::char('x')]);
        trie.insert(&[KeyEvent::char('j')], e.clone());
        trie.insert(
            &[KeyEvent::char('j'), KeyEvent::char('k')],
            entry(&[KeyEvent::escape()]),
        );

        // get_single returns the entry even if the node also has children
        assert_eq!(trie.get_single(KeyEvent::char('j')), Some(&e));
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut trie = MappingTrie::default();
        assert_eq!(trie.remove(&[KeyEvent::char('z')]), None);
    }

    #[test]
    fn deep_multi_key_sequence() {
        let mut trie = MappingTrie::default();
        let e = entry(&[KeyEvent::char('x')]);
        let lhs = [
            KeyEvent::char('g'),
            KeyEvent::char('c'),
            KeyEvent::char('c'),
        ];
        trie.insert(&lhs, e.clone());

        assert_eq!(
            trie.lookup(&[KeyEvent::char('g')]),
            TrieLookup::Prefix { exact: None }
        );
        assert_eq!(
            trie.lookup(&[KeyEvent::char('g'), KeyEvent::char('c')]),
            TrieLookup::Prefix { exact: None },
        );
        assert_eq!(trie.lookup(&lhs), TrieLookup::ExactOnly(&e));
    }

    #[test]
    fn iter_returns_all_mappings() {
        let mut trie = MappingTrie::default();

        let e1 = entry(&[KeyEvent::char('x')]);
        let e2 = entry(&[KeyEvent::escape()]);
        let e3 = entry(&[KeyEvent::char('y')]);

        trie.insert(&[KeyEvent::char('j')], e1.clone());
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], e2.clone());
        trie.insert(&[KeyEvent::char('q')], e3.clone());

        let items = trie.entries();
        assert_eq!(items.len(), 3);

        // Verify all entries are present (order depends on HashMap, so check by content)
        let entries: Vec<&MappingEntry> = items.iter().map(|(_, e)| *e).collect();
        assert!(entries.contains(&&e1));
        assert!(entries.contains(&&e2));
        assert!(entries.contains(&&e3));

        // Verify LHS reconstruction: find the 2-key mapping
        let jk_mapping = items.iter().find(|(lhs, _)| lhs.len() == 2).unwrap();
        assert_eq!(jk_mapping.0[0], KeyEvent::char('j'));
        assert_eq!(jk_mapping.0[1], KeyEvent::char('k'));
        assert_eq!(jk_mapping.1, &e2);
    }

    #[test]
    fn iter_empty_trie() {
        let trie = MappingTrie::default();
        assert!(trie.entries().is_empty());
    }

    // ── <nowait> tests ──────────────────────────────────────────────────────

    fn entry_nowait(keys: &[KeyEvent]) -> MappingEntry {
        MappingEntry::new_nowait(key_sequence(keys), MappingKind::NonRecursive)
    }

    /// A `<nowait>` exact match fires immediately even when a longer mapping
    /// also starts with the same prefix.
    #[test]
    fn nowait_exact_fires_immediately() {
        let mut trie = MappingTrie::default();
        let e_short = entry_nowait(&[KeyEvent::char('x')]);
        let e_long = entry(&[KeyEvent::char('y')]);

        // Both 'j' and 'jk' are mapped; 'j' has <nowait>
        trie.insert(&[KeyEvent::char('j')], e_short.clone());
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], e_long);

        // Despite 'jk' being a longer match, 'j' with <nowait> returns ExactOnly
        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::ExactOnly(&e_short),
            "<nowait> entry must fire immediately as ExactOnly",
        );
    }

    /// Without `<nowait>`, the same setup returns `Prefix { exact: Some }`.
    #[test]
    fn without_nowait_returns_prefix() {
        let mut trie = MappingTrie::default();
        let e_short = entry(&[KeyEvent::char('x')]);
        let e_long = entry(&[KeyEvent::char('y')]);

        // Both 'j' and 'jk' are mapped; 'j' does NOT have <nowait>
        trie.insert(&[KeyEvent::char('j')], e_short.clone());
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], e_long);

        // Without <nowait>, 'j' is ambiguous → Prefix with exact
        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::Prefix {
                exact: Some(&e_short)
            },
            "without <nowait>, ambiguous prefix must return Prefix {{ exact: Some }}",
        );
    }

    /// `<nowait>` has no effect when there are no longer prefixes — the result
    /// is `ExactOnly` in either case (the flag is redundant but harmless).
    #[test]
    fn nowait_no_children_still_exact_only() {
        let mut trie = MappingTrie::default();
        let e = entry_nowait(&[KeyEvent::escape()]);
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], e.clone());

        assert_eq!(
            trie.lookup(&[KeyEvent::char('j'), KeyEvent::char('k')]),
            TrieLookup::ExactOnly(&e),
        );
    }

    // ── <expr> tests ──────────────────────────────────────────────────────

    fn entry_expr(expression: &str) -> MappingEntry {
        MappingEntry::new_expr(CompactString::from(expression), MappingKind::NonRecursive)
    }

    fn entry_expr_nowait(expression: &str) -> MappingEntry {
        MappingEntry::new_expr_nowait(CompactString::from(expression), MappingKind::NonRecursive)
    }

    fn entry_expr_recursive(expression: &str) -> MappingEntry {
        MappingEntry::new_expr(CompactString::from(expression), MappingKind::Recursive)
    }

    /// `new_expr()` sets `expr` flag and stores the expression text.
    #[test]
    fn expr_entry_has_correct_flags() {
        let e = entry_expr("v:count ? 'j' : 'gj'");
        assert!(e.expr());
        assert!(!e.nowait());
        assert!(e.sequence().is_empty(), "expr entries have empty sequence");
        assert_eq!(e.expression(), Some("v:count ? 'j' : 'gj'"));
        assert!(!e.kind().is_recursive());
    }

    /// `new_expr_nowait()` sets both `expr` and `nowait` flags.
    #[test]
    fn expr_nowait_entry_has_correct_flags() {
        let e = entry_expr_nowait("pumvisible() ? '<C-Y>' : '<CR>'");
        assert!(e.expr());
        assert!(e.nowait());
        assert!(e.sequence().is_empty());
        assert_eq!(e.expression(), Some("pumvisible() ? '<C-Y>' : '<CR>'"));
    }

    /// `new_expr()` with `Recursive` kind preserves the mapping kind.
    #[test]
    fn expr_recursive_preserves_kind() {
        let e = entry_expr_recursive("MyFunc()");
        assert!(e.expr());
        assert!(e.kind().is_recursive());
        assert_eq!(e.expression(), Some("MyFunc()"));
    }

    /// Non-expr entries have `expr() == false` and `expression() == None`.
    #[test]
    fn non_expr_entry_returns_none_expression() {
        let e = entry(&[KeyEvent::char('x')]);
        assert!(!e.expr());
        assert_eq!(e.expression(), None);
    }

    /// Expr entries in the trie are found by lookup just like regular entries.
    #[test]
    fn expr_entry_lookup_exact_only() {
        let mut trie = MappingTrie::default();
        let e = entry_expr("v:count ? 'j' : 'gj'");
        trie.insert(&[KeyEvent::char('j')], e.clone());

        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::ExactOnly(&e),
        );
    }

    /// Expr entries with `<nowait>` fire immediately even when longer mappings exist.
    #[test]
    fn expr_nowait_fires_immediately_over_prefix() {
        let mut trie = MappingTrie::default();
        let e_short = entry_expr_nowait("MyFunc()");
        let e_long = entry(&[KeyEvent::char('y')]);

        trie.insert(&[KeyEvent::char('j')], e_short.clone());
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], e_long);

        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::ExactOnly(&e_short),
            "<expr><nowait> entry must fire immediately as ExactOnly",
        );
    }

    /// Expr entries without `<nowait>` return `Prefix { exact: Some }` when
    /// a longer mapping shares the prefix.
    #[test]
    fn expr_without_nowait_returns_prefix_when_ambiguous() {
        let mut trie = MappingTrie::default();
        let e_short = entry_expr("MyFunc()");
        let e_long = entry(&[KeyEvent::char('y')]);

        trie.insert(&[KeyEvent::char('j')], e_short.clone());
        trie.insert(&[KeyEvent::char('j'), KeyEvent::char('k')], e_long);

        assert_eq!(
            trie.lookup(&[KeyEvent::char('j')]),
            TrieLookup::Prefix {
                exact: Some(&e_short)
            },
        );
    }

    /// Expr entries are listed by `entries()`.
    #[test]
    fn entries_includes_expr_mappings() {
        let mut trie = MappingTrie::default();
        let e = entry_expr("MyFunc()");
        trie.insert(&[KeyEvent::char('j')], e.clone());

        let items = trie.entries();
        assert_eq!(items.len(), 1);
        assert!(items[0].1.expr());
        assert_eq!(items[0].1.expression(), Some("MyFunc()"));
    }

    /// Overwriting a regular entry with an expr entry replaces it correctly.
    #[test]
    fn overwrite_regular_with_expr() {
        let mut trie = MappingTrie::default();
        let regular = entry(&[KeyEvent::char('x')]);
        let expr = entry_expr("MyFunc()");

        trie.insert(&[KeyEvent::char('j')], regular);
        trie.insert(&[KeyEvent::char('j')], expr.clone());

        assert_eq!(trie.len(), 1);
        match trie.lookup(&[KeyEvent::char('j')]) {
            TrieLookup::ExactOnly(found) => {
                assert!(found.expr());
                assert_eq!(found, &expr);
            }
            other => panic!("expected ExactOnly, got {other:?}"),
        }
    }

    #[test]
    fn default_owner_is_user() {
        let e = entry(&[KeyEvent::char('x')]);
        assert_eq!(e.owner(), &MappingOwner::User);
    }

    /// `.with_owner(Core)` sets the owner to `Core`.
    #[test]
    fn with_owner_core() {
        let e = entry(&[KeyEvent::char('x')]).with_owner(MappingOwner::Core);
        assert_eq!(e.owner(), &MappingOwner::Core);
    }

    #[test]
    fn with_owner_host() {
        let e = entry(&[KeyEvent::char('x')])
            .with_owner(MappingOwner::Host(compact_str::CompactString::from("test")));
        assert_eq!(
            e.owner(),
            &MappingOwner::Host(compact_str::CompactString::from("test"))
        );
    }

    /// Two entries with the same sequence but different owners are NOT equal.
    #[test]
    fn different_owners_are_not_equal() {
        let e_user = entry(&[KeyEvent::char('x')]);
        let e_core = entry(&[KeyEvent::char('x')]).with_owner(MappingOwner::Core);
        assert_ne!(e_user, e_core);
    }

    /// Two entries with the same sequence and same owner are equal.
    #[test]
    fn same_owner_and_sequence_are_equal() {
        let e1 = entry(&[KeyEvent::char('x')]);
        let e2 = entry(&[KeyEvent::char('x')]);
        assert_eq!(e1, e2);
    }

    // ── remove_by_owner tests ────────────────────────────────────────────────

    #[test]
    fn remove_by_owner_zero_mappings_returns_zero() {
        let mut trie = MappingTrie::default();
        trie.insert(&[KeyEvent::char('j')], entry(&[KeyEvent::char('x')]));

        let removed = trie.remove_by_owner(&MappingOwner::Core);
        assert_eq!(removed, 0);
        assert_eq!(trie.len(), 1);
    }

    /// Removing an owner from an empty trie returns 0 and doesn't crash.
    #[test]
    fn remove_by_owner_empty_trie_returns_zero() {
        let mut trie = MappingTrie::default();
        let removed = trie.remove_by_owner(&MappingOwner::User);
        assert_eq!(removed, 0);
        assert!(trie.is_empty());
    }

    /// After removing a multi-key mapping by owner, the intermediate node is
    /// pruned (orphan node removal).
    #[test]
    fn remove_by_owner_prunes_orphan_intermediate_nodes() {
        let mut trie = MappingTrie::default();
        let e = entry(&[KeyEvent::char('x')]).with_owner(MappingOwner::Core);
        // Insert 'gc' (g → c)
        trie.insert(&[KeyEvent::char('g'), KeyEvent::char('c')], e);

        let removed = trie.remove_by_owner(&MappingOwner::Core);
        assert_eq!(removed, 1);
        assert!(trie.is_empty());

        // The intermediate 'g' node must also be pruned.
        assert_eq!(
            trie.lookup(&[KeyEvent::char('g')]),
            TrieLookup::NoMatch,
            "intermediate 'g' node must be pruned after removing its only child",
        );
    }

    /// `remove_by_owner` removes all matching entries even when multiple exist.
    #[test]
    fn remove_by_owner_removes_multiple_entries_same_owner() {
        let mut trie = MappingTrie::default();
        trie.insert(
            &[KeyEvent::char('a')],
            entry(&[KeyEvent::char('x')]).with_owner(MappingOwner::Core),
        );
        trie.insert(
            &[KeyEvent::char('b')],
            entry(&[KeyEvent::char('y')]).with_owner(MappingOwner::Core),
        );
        trie.insert(&[KeyEvent::char('c')], entry(&[KeyEvent::char('z')]));

        let removed = trie.remove_by_owner(&MappingOwner::Core);
        assert_eq!(removed, 2);
        assert_eq!(trie.len(), 1);
        assert_eq!(trie.lookup(&[KeyEvent::char('a')]), TrieLookup::NoMatch);
        assert_eq!(trie.lookup(&[KeyEvent::char('b')]), TrieLookup::NoMatch);
    }

    // ── description tests ────────────────────────────────────────────────────

    #[test]
    fn with_description() {
        let e = entry(&[KeyEvent::char('x')])
            .with_description(Some(CompactString::from("Go to definition")));
        assert_eq!(e.description(), Some("Go to definition"));
    }

    #[test]
    fn description_default_none() {
        let e = entry(&[KeyEvent::char('x')]);
        assert_eq!(e.description(), None);
    }
}
