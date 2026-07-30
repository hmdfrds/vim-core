//! Syntax-aware provider trait — tree-sitter gateway.
//!
//! This trait enables syntax-aware text objects and motions without
//! coupling the engine to any specific tree-sitter implementation.
//! Hosts implement [`SyntaxProvider`] to expose structural navigation
//! and selection capabilities.
//!
//! # Architecture
//!
//! Lives at the `document` layer (imports `primitives` only) alongside
//! other capability provider traits. The engine queries this provider
//! through `Providers::syntax` when processing syntax-aware text objects
//! (e.g., `if`/`af` for functions, `ic`/`ac` for classes).
//!
//! # Design
//!
//! The trait uses [`SyntaxNodeKind`] to abstract over language-specific
//! node types. Common structural elements (function, class, argument,
//! comment) have dedicated variants; hosts can extend with `Custom(u32)`
//! for language-specific concepts.

/// Kinds of syntax tree nodes the engine can query for.
///
/// Maps Vim text object concepts to structural node types that a
/// tree-sitter (or similar) parser can resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SyntaxNodeKind {
    /// Function / method definition (for `if`/`af` text objects).
    Function,
    /// Class / struct / module definition (for `ic`/`ac` text objects).
    Class,
    /// Function argument / parameter (for `ia`/`aa` text objects).
    Argument,
    /// Comment block or line (for `i/`/`a/` text objects).
    Comment,
    /// Conditional block — if/else/match (for future `ii`/`ai` text objects).
    Conditional,
    /// Loop block — for/while/loop (for future text objects).
    Loop,
    /// Generic block / scope — braces, indentation level.
    Block,
    /// HTML/XML tag (for `it`/`at` text objects).
    Tag,
    /// Host-defined custom syntax node kind.
    ///
    /// Hosts register custom node kinds with an application-specific `u32` ID.
    /// The engine passes this ID through to the provider without interpretation.
    Custom(u32),
}

impl SyntaxNodeKind {
    /// Convert to the tree-sitter query name prefix.
    ///
    /// Returns the object name used in `textobjects.scm` query captures:
    /// - `Function` → `"function"`
    /// - `Class` → `"class"`
    /// - `Argument` → `"parameter"`
    /// - `Comment` → `"comment"`
    /// - Others → their lowercase name
    /// - `Custom(id)` → `None` (no standard query name)
    #[must_use]
    pub const fn query_name(&self) -> Option<&'static str> {
        match self {
            Self::Function => Some("function"),
            Self::Class => Some("class"),
            Self::Argument => Some("parameter"),
            Self::Comment => Some("comment"),
            Self::Conditional => Some("conditional"),
            Self::Loop => Some("loop"),
            Self::Block => Some("block"),
            Self::Tag => Some("tag"),
            Self::Custom(_) => None,
        }
    }
}

/// Provider for syntax-aware (tree-sitter) structural queries.
///
/// Hosts implement this trait to enable syntax-aware text objects and
/// motions. The engine queries the provider through `Providers::syntax`
/// when it encounters syntax-dependent operations.
///
/// All methods return `Option` — `None` means the provider cannot resolve
/// the query (e.g., cursor is outside any node of the requested kind,
/// or the file has no syntax tree).
///
/// # Example (host-side)
///
/// ```ignore
/// struct TreeSitterSyntax {
///     tree: tree_sitter::Tree,
///     // ...
/// }
///
/// impl SyntaxProvider for TreeSitterSyntax {
///     fn enclosing_node(
///         &self,
///         text: &str,
///         cursor: usize,
///         kind: SyntaxNodeKind,
///     ) -> Option<(usize, usize)> {
///         let node = find_enclosing(self.tree.root_node(), cursor, kind)?;
///         Some((node.start_byte(), node.end_byte()))
///     }
///
///     fn next_node(
///         &self,
///         text: &str,
///         cursor: usize,
///         kind: SyntaxNodeKind,
///         count: u32,
///     ) -> Option<usize> {
///         find_next_sibling(self.tree.root_node(), cursor, kind, count)
///             .map(|n| n.start_byte())
///     }
///
///     fn prev_node(
///         &self,
///         text: &str,
///         cursor: usize,
///         kind: SyntaxNodeKind,
///         count: u32,
///     ) -> Option<usize> {
///         find_prev_sibling(self.tree.root_node(), cursor, kind, count)
///             .map(|n| n.start_byte())
///     }
/// }
/// ```
pub trait SyntaxProvider: Send {
    /// Find the smallest node of the given kind that encloses the cursor.
    ///
    /// Returns `Some((start, end))` byte offsets of the enclosing node,
    /// or `None` if the cursor is not inside any node of this kind.
    ///
    /// Used by syntax-aware text objects (e.g., `if` selects the enclosing
    /// function, `ac` selects the enclosing class).
    fn enclosing_node(
        &self,
        text: &str,
        cursor: usize,
        kind: SyntaxNodeKind,
    ) -> Option<(usize, usize)>;

    /// Find the start of the next node of the given kind after cursor.
    ///
    /// Returns the byte offset of the target node's start, or `None` if
    /// no such node exists after the cursor.
    ///
    /// Used by syntax-aware motions (e.g., `]m` moves to next method start).
    fn next_node(
        &self,
        text: &str,
        cursor: usize,
        kind: SyntaxNodeKind,
        count: u32,
    ) -> Option<usize>;

    /// Find the start of the previous node of the given kind before cursor.
    ///
    /// Returns the byte offset of the target node's start, or `None` if
    /// no such node exists before the cursor.
    ///
    /// Used by syntax-aware motions (e.g., `[m` moves to previous method start).
    fn prev_node(
        &self,
        text: &str,
        cursor: usize,
        kind: SyntaxNodeKind,
        count: u32,
    ) -> Option<usize>;

    // ── Kind-agnostic tree traversal (incremental syntax selection) ───

    /// Returns the smallest ancestor node that strictly contains `[start, end]`.
    ///
    /// "Strictly contains" means the returned range `(rs, re)` satisfies
    /// `rs <= start && end <= re` with at least one inequality strict.
    /// Returns `None` if no such ancestor exists (at AST root).
    fn ancestor_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        None
    }

    /// Find the first named child node within the given range.
    ///
    /// Used by incremental syntax selection (shrink to child).
    fn descendant_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        None
    }

    /// Find the next named sibling node after the node at the given range.
    ///
    /// Used by incremental syntax selection (navigate to next sibling).
    fn next_sibling_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        None
    }

    /// Find the previous named sibling node before the node at the given range.
    ///
    /// Used by incremental syntax selection (navigate to previous sibling).
    fn prev_sibling_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        None
    }

    /// All sibling nodes of the smallest node containing `[start, end]`.
    ///
    /// Returns ranges of all named sibling nodes (including the node itself),
    /// sorted by document position. Returns empty `Vec` if unsupported,
    /// at root, or no siblings exist.
    ///
    /// Hosts SHOULD return only named/semantic nodes (skip punctuation,
    /// delimiters, and anonymous syntax nodes).
    fn sibling_nodes(&self, _text: &str, _start: usize, _end: usize) -> Vec<(usize, usize)> {
        Vec::new()
    }

    /// All direct child nodes of the smallest node containing `[start, end]`.
    ///
    /// Returns ranges of all named child nodes, sorted by document position.
    /// Returns empty `Vec` if unsupported or the node is a leaf.
    ///
    /// Hosts SHOULD return only named/semantic nodes.
    fn child_nodes(&self, _text: &str, _start: usize, _end: usize) -> Vec<(usize, usize)> {
        Vec::new()
    }

    /// Compute syntax-based fold ranges for the entire document.
    ///
    /// Returns `(start_line, end_line)` pairs for foldable regions, where
    /// lines are 0-indexed. The host implements this using tree-sitter's
    /// fold queries. Returns `None` if fold computation is not supported.
    fn fold_ranges(&self, _text: &str) -> Option<Vec<(usize, usize)>> {
        None
    }

    // ── Query-based text objects ───────────────────────────────────────

    /// Query a text object by name, following the `textobjects.scm`
    /// convention.
    ///
    /// The `name` follows the pattern `"{object}.{scope}"` where:
    /// - `object` is the structural element: `"function"`, `"class"`,
    ///   `"parameter"`, `"comment"`, `"test"`, `"entry"`, etc.
    /// - `scope` is `"inside"` or `"around"`
    ///
    /// For example: `"function.inside"`, `"class.around"`, `"test.inside"`.
    ///
    /// Returns `Some((start, end))` byte offsets if the query matches a node
    /// enclosing the cursor, `None` otherwise.
    ///
    /// # Design
    ///
    /// This method enables **data-driven text objects**: hosts define new
    /// text objects by writing tree-sitter query files (`.scm`), not Rust
    /// code. The engine routes requests by name; the provider resolves them
    /// against the syntax tree.
    ///
    /// Unlike [`enclosing_node`](SyntaxProvider::enclosing_node) where the
    /// engine handles inner/around trimming, this method returns the exact
    /// range from the query capture — the `.inside` and `.around` captures
    /// in the query file already distinguish the two scopes.
    ///
    /// # Fallback
    ///
    /// The default implementation returns `None`. When both this method and
    /// `enclosing_node` are available, the engine tries `query_textobject`
    /// first and falls back to `enclosing_node` with engine-side trimming.
    ///
    /// # Example (host-side with tree-sitter)
    ///
    /// ```ignore
    /// impl SyntaxProvider for TreeSitterSyntax {
    ///     fn query_textobject(
    ///         &self,
    ///         name: &str,
    ///         text: &str,
    ///         cursor: usize,
    ///     ) -> Option<(usize, usize)> {
    ///         let query = self.textobjects_query.as_ref()?;
    ///         let capture_idx = query.capture_index_for_name(name)?;
    ///         let mut cursor_qc = tree_sitter::QueryCursor::new();
    ///         cursor_qc.matches(query, self.tree.root_node(), text.as_bytes())
    ///             .flat_map(|m| m.captures.iter())
    ///             .filter(|c| c.index == capture_idx)
    ///             .filter(|c| {
    ///                 let r = c.node.byte_range();
    ///                 r.start <= cursor && cursor < r.end
    ///             })
    ///             .min_by_key(|c| c.node.byte_range().len())
    ///             .map(|c| (c.node.start_byte(), c.node.end_byte()))
    ///     }
    /// }
    /// ```
    fn query_textobject(&self, _name: &str, _text: &str, _cursor: usize) -> Option<(usize, usize)> {
        None
    }

    /// Query for the next occurrence of a named text object after the cursor.
    ///
    /// Used for navigation motions like `]f` (next function), `]t` (next test).
    /// The `name` uses the same convention as [`Self::query_textobject`] but with
    /// a `".movement"` suffix or the `".around"` scope.
    ///
    /// Returns the byte offset of the next matching node's start, or `None`.
    fn query_next_textobject(
        &self,
        _name: &str,
        _text: &str,
        _cursor: usize,
        _count: u32,
    ) -> Option<usize> {
        None
    }

    /// Query for the previous occurrence of a named text object before the cursor.
    ///
    /// Used for navigation motions like `[f` (previous function), `[t` (previous test).
    ///
    /// Returns the byte offset of the previous matching node's start, or `None`.
    fn query_prev_textobject(
        &self,
        _name: &str,
        _text: &str,
        _cursor: usize,
        _count: u32,
    ) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal implementor that only provides the 3 required methods.
    /// The 4 new traversal methods should fall through to their default `None`.
    struct MinimalProvider;

    impl SyntaxProvider for MinimalProvider {
        fn enclosing_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
        ) -> Option<(usize, usize)> {
            None
        }

        fn next_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            None
        }

        fn prev_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            None
        }
    }

    #[test]
    fn default_ancestor_node_returns_none() {
        let p = MinimalProvider;
        assert_eq!(p.ancestor_node("hello world", 0, 5), None);
    }

    #[test]
    fn default_descendant_node_returns_none() {
        let p = MinimalProvider;
        assert_eq!(p.descendant_node("hello world", 0, 11), None);
    }

    #[test]
    fn default_next_sibling_node_returns_none() {
        let p = MinimalProvider;
        assert_eq!(p.next_sibling_node("hello world", 0, 5), None);
    }

    #[test]
    fn default_prev_sibling_node_returns_none() {
        let p = MinimalProvider;
        assert_eq!(p.prev_sibling_node("hello world", 6, 11), None);
    }

    /// A provider that overrides all 4 new methods to verify they can be
    /// implemented and dispatched correctly.
    struct FullProvider;

    impl SyntaxProvider for FullProvider {
        fn enclosing_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
        ) -> Option<(usize, usize)> {
            Some((0, 100))
        }

        fn next_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            Some(50)
        }

        fn prev_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            Some(10)
        }

        fn ancestor_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
            Some((0, 200))
        }

        fn descendant_node(
            &self,
            _text: &str,
            _start: usize,
            _end: usize,
        ) -> Option<(usize, usize)> {
            Some((10, 50))
        }

        fn next_sibling_node(
            &self,
            _text: &str,
            _start: usize,
            _end: usize,
        ) -> Option<(usize, usize)> {
            Some((60, 90))
        }

        fn prev_sibling_node(
            &self,
            _text: &str,
            _start: usize,
            _end: usize,
        ) -> Option<(usize, usize)> {
            Some((0, 30))
        }
    }

    #[test]
    fn overridden_traversal_methods_dispatch_correctly() {
        let p = FullProvider;
        assert_eq!(p.ancestor_node("text", 10, 50), Some((0, 200)));
        assert_eq!(p.descendant_node("text", 0, 200), Some((10, 50)));
        assert_eq!(p.next_sibling_node("text", 10, 50), Some((60, 90)));
        assert_eq!(p.prev_sibling_node("text", 60, 90), Some((0, 30)));
    }

    #[test]
    fn minimal_provider_sibling_nodes_default_empty() {
        let p = MinimalProvider;
        assert!(p.sibling_nodes("hello", 0, 5).is_empty());
    }

    #[test]
    fn minimal_provider_child_nodes_default_empty() {
        let p = MinimalProvider;
        assert!(p.child_nodes("hello", 0, 5).is_empty());
    }

    #[test]
    fn default_fold_ranges_returns_none() {
        let p = MinimalProvider;
        assert_eq!(p.fold_ranges("hello world"), None);
    }

    #[test]
    fn overridden_fold_ranges_returns_some() {
        struct FoldProvider;
        impl SyntaxProvider for FoldProvider {
            fn enclosing_node(
                &self,
                _: &str,
                _: usize,
                _: SyntaxNodeKind,
            ) -> Option<(usize, usize)> {
                None
            }
            fn next_node(&self, _: &str, _: usize, _: SyntaxNodeKind, _: u32) -> Option<usize> {
                None
            }
            fn prev_node(&self, _: &str, _: usize, _: SyntaxNodeKind, _: u32) -> Option<usize> {
                None
            }
            fn fold_ranges(&self, _text: &str) -> Option<Vec<(usize, usize)>> {
                Some(vec![(0, 5), (10, 20)])
            }
        }
        let p = FoldProvider;
        let ranges = p.fold_ranges("some code").unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (0, 5));
        assert_eq!(ranges[1], (10, 20));
    }
}
