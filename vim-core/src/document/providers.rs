//! Capability provider bundle for shell integration.
//!
//! Bundles optional shell-provided capabilities (fold, display-line, search)
//! into a single struct. This keeps host structs (`InputContext`, `MotionContext`)
//! clean — one field instead of N, derives work, and adding a provider is a
//! one-line change to this bundle rather than a shotgun edit across the crate.

use super::SemanticTextObjectProvider;
use super::{
    CustomMotionProvider, CustomOperatorProvider, CustomTextObjectProvider, DisplayLineProvider,
    FoldProvider, IndentProvider, SearchProvider, SyntaxProvider,
};

/// Bundle of optional capability providers from the shell.
///
/// Shells populate the fields they support. The engine passes the bundle
/// through to motions that need it. Fields default to `None` (no provider).
///
/// # Example
///
/// ```ignore
/// let providers = Providers::new()
///     .with_fold(&my_fold_impl)
///     .with_search(&my_search_impl);
/// let ctx = InputContext::new(&doc, 0)
///     .validate()?
///     .with_providers(providers);
/// ```
#[derive(Default, Copy, Clone)]
pub struct Providers<'shell> {
    /// Fold provider for fold-aware navigation (j/k skip folded lines, zj/zk).
    pub fold: Option<&'shell dyn FoldProvider>,
    /// Display line provider for soft-wrap navigation (gj/gk).
    pub display_lines: Option<&'shell dyn DisplayLineProvider>,
    /// Search provider for regex-powered search (n/N/gn/gN).
    pub search: Option<&'shell dyn SearchProvider>,
    /// Indent provider for language-aware indentation (o/O, Enter).
    pub indent: Option<&'shell dyn IndentProvider>,
    /// Custom motion provider (host-registered motions).
    pub custom_motions: Option<&'shell dyn CustomMotionProvider>,
    /// Custom text object provider (host-registered text objects).
    pub custom_textobjects: Option<&'shell dyn CustomTextObjectProvider>,
    /// Custom operator provider (host-registered operators).
    pub custom_operators: Option<&'shell dyn CustomOperatorProvider>,
    /// Syntax provider for tree-sitter-aware text objects and motions.
    pub syntax: Option<&'shell dyn SyntaxProvider>,
    /// Semantic text object provider (language-server / tree-sitter protocol).
    pub semantic_textobjects: Option<&'shell dyn SemanticTextObjectProvider>,
}

impl std::fmt::Debug for Providers<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Providers")
            .field("fold", &self.fold.is_some())
            .field("display_lines", &self.display_lines.is_some())
            .field("search", &self.search.is_some())
            .field("indent", &self.indent.is_some())
            .field("custom_motions", &self.custom_motions.is_some())
            .field("custom_textobjects", &self.custom_textobjects.is_some())
            .field("custom_operators", &self.custom_operators.is_some())
            .field("syntax", &self.syntax.is_some())
            .field("semantic_textobjects", &self.semantic_textobjects.is_some())
            .finish()
    }
}

impl<'shell> Providers<'shell> {
    /// Create an empty provider bundle (all `None`).
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set fold provider.
    #[inline]
    #[must_use]
    pub fn with_fold(mut self, provider: &'shell dyn FoldProvider) -> Self {
        self.fold = Some(provider);
        self
    }

    /// Set display line provider.
    #[inline]
    #[must_use]
    pub fn with_display_lines(mut self, provider: &'shell dyn DisplayLineProvider) -> Self {
        self.display_lines = Some(provider);
        self
    }

    /// Set search provider.
    #[inline]
    #[must_use]
    pub fn with_search(mut self, provider: &'shell dyn SearchProvider) -> Self {
        self.search = Some(provider);
        self
    }

    /// Set indent provider.
    #[inline]
    #[must_use]
    pub fn with_indent(mut self, provider: &'shell dyn IndentProvider) -> Self {
        self.indent = Some(provider);
        self
    }

    /// Set the custom motion provider.
    #[inline]
    #[must_use]
    pub fn with_custom_motions(mut self, provider: &'shell dyn CustomMotionProvider) -> Self {
        self.custom_motions = Some(provider);
        self
    }

    /// Set the custom text object provider.
    #[inline]
    #[must_use]
    pub fn with_custom_textobjects(
        mut self,
        provider: &'shell dyn CustomTextObjectProvider,
    ) -> Self {
        self.custom_textobjects = Some(provider);
        self
    }

    /// Set the custom operator provider.
    #[inline]
    #[must_use]
    pub fn with_custom_operators(mut self, provider: &'shell dyn CustomOperatorProvider) -> Self {
        self.custom_operators = Some(provider);
        self
    }

    /// Set the syntax provider (tree-sitter gateway).
    #[inline]
    #[must_use]
    pub fn with_syntax(mut self, provider: &'shell dyn SyntaxProvider) -> Self {
        self.syntax = Some(provider);
        self
    }

    /// Set the semantic text object provider.
    #[inline]
    #[must_use]
    pub fn with_semantic_textobjects(
        mut self,
        provider: &'shell dyn SemanticTextObjectProvider,
    ) -> Self {
        self.semantic_textobjects = Some(provider);
        self
    }

    /// Check if any provider is set.
    #[inline]
    #[must_use]
    pub fn has_any(&self) -> bool {
        self.fold.is_some()
            || self.display_lines.is_some()
            || self.search.is_some()
            || self.indent.is_some()
            || self.custom_motions.is_some()
            || self.custom_textobjects.is_some()
            || self.custom_operators.is_some()
            || self.syntax.is_some()
            || self.semantic_textobjects.is_some()
    }
}
