/// Result returned by a [`SemanticTextObjectProvider`] resolve call.
///
/// Contains the byte range for the text object and optional structural metadata.
pub struct SemanticTextObjectResult {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// Whether the text object should be treated as linewise.
    pub linewise: bool,
    /// Byte range of the parent node, if available.
    ///
    /// Allows the dispatch layer to support count-based expansion without
    /// re-querying the provider.
    pub parent: Option<(usize, usize)>,
}

/// Provider for semantic (language-server / tree-sitter) text objects.
///
/// Hosts implement this trait and register it via
/// [`Providers::with_semantic_textobjects`](crate::document::Providers::with_semantic_textobjects).
///
/// When the engine encounters `TextObjectKind::Semantic(obj)`, the dispatch
/// layer calls [`SemanticTextObjectProvider::resolve`] first. If the provider
/// returns `None`, the engine falls back to a syntactic approximation.
///
/// # Example (host-side)
///
/// ```ignore
/// struct LspTextObjects { /* LSP state */ }
///
/// impl SemanticTextObjectProvider for LspTextObjects {
///     fn resolve(
///         &self,
///         object: SemanticObject,
///         text: &str,
///         cursor: usize,
///         inner: bool,
///         count: u32,
///     ) -> Option<SemanticTextObjectResult> {
///         match object {
///             SemanticObject::Function => find_function(text, cursor, inner),
///             _ => None,
///         }
///     }
///
///     fn supported_objects(&self) -> &[SemanticObject] {
///         &[SemanticObject::Function, SemanticObject::Class]
///     }
/// }
/// ```
pub trait SemanticTextObjectProvider: Send {
    /// Resolve a semantic text object at the given cursor position.
    ///
    /// - `object`: the semantic object kind requested
    /// - `text`: full document text as a UTF-8 string
    /// - `cursor`: current cursor byte offset
    /// - `inner`: `true` for inner scope (`i`), `false` for around scope (`a`)
    /// - `count`: repeat count (e.g. `3` in `3if`)
    ///
    /// Returns `Some(result)` with the matched range, or `None` if the object
    /// cannot be resolved at this position.
    fn resolve(
        &self,
        object: crate::primitives::SemanticObject,
        text: &str,
        cursor: usize,
        inner: bool,
        count: u32,
    ) -> Option<SemanticTextObjectResult>;

    /// Return the set of [`SemanticObject`](crate::primitives::SemanticObject) variants
    /// this provider can handle.
    ///
    /// The engine uses this list for introspection and tooling; it does not gate
    /// `resolve` calls — providers must still return `None` for unsupported objects.
    fn supported_objects(&self) -> &[crate::primitives::SemanticObject];
}
