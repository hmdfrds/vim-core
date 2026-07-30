//! Persistent provider registration on the engine.
//!
//! Document-independent providers (custom motions, text objects, operators,
//! syntax) can be registered once on the engine rather than being passed
//! in every `InputContext`. The engine merges persistent providers with
//! per-call providers, with per-call providers taking precedence.
//!
//! # Design
//!
//! Providers registered on the engine are stored as boxed trait objects
//! with `'static` lifetime (they outlive any single `process()` call).
//! Per-call providers from `InputContext` take precedence — if both the
//! engine and the call have a custom motion provider, the per-call one wins.
//!
//! This enables the "register once, use everywhere" pattern:
//!
//! ```ignore
//! // Register once at setup
//! engine.register_motion_provider(MyTreeSitterMotions::new());
//! engine.register_syntax_provider(MyTreeSitterSyntax::new());
//!
//! // No need to pass providers in every InputContext
//! let ctx = InputContext::new(&doc, cursor).validate()?;
//! let response = engine.process(key, ctx);
//! ```

use crate::document::SemanticTextObjectProvider;
use crate::document::{
    CustomMotionProvider, CustomOperatorProvider, CustomTextObjectProvider, DisplayLineProvider,
    FoldProvider, IndentProvider, Providers, SyntaxProvider,
};

/// Persistent providers registered on the engine.
///
/// These are merged with per-call `Providers` from `InputContext` before
/// dispatch. Per-call providers take precedence over engine-level ones.
pub(in crate::execution::engine) struct EngineProviders {
    /// Custom motion provider (e.g., tree-sitter structural motions).
    pub(in crate::execution::engine) custom_motions: Option<Box<dyn CustomMotionProvider>>,
    /// Custom text object provider (e.g., tree-sitter function/class objects).
    pub(in crate::execution::engine) custom_textobjects: Option<Box<dyn CustomTextObjectProvider>>,
    /// Custom operator provider (e.g., host-specific operators).
    pub(in crate::execution::engine) custom_operators: Option<Box<dyn CustomOperatorProvider>>,
    /// Syntax provider (tree-sitter gateway).
    pub(in crate::execution::engine) syntax: Option<Box<dyn SyntaxProvider>>,
    /// Fold provider for fold-aware navigation (j/k skip folded lines, zj/zk).
    ///
    /// FFI/WASM hosts push fold state via [`VimEngine::set_fold_state`] before
    /// each `processKey` call. Native hosts may register a persistent provider
    /// here instead of passing one per-call through `Providers`.
    pub(in crate::execution::engine) fold: Option<Box<dyn FoldProvider>>,
    /// Indent provider for language-aware indentation (o/O, Enter).
    ///
    /// When set, the engine uses this instead of basic autoindent
    /// (copying the previous line's whitespace). The host pushes a
    /// [`CachedIndentHint`](crate::document::CachedIndentHint) before
    /// each `processKey` call.
    pub(in crate::execution::engine) indent: Option<Box<dyn IndentProvider>>,
    /// Display line provider for soft-wrap navigation (gj/gk/g0/g$/g^).
    ///
    /// FFI/WASM hosts push wrap parameters via
    /// [`VimEngine::set_display_line_state`] before each `processKey` call.
    pub(in crate::execution::engine) display_lines: Option<Box<dyn DisplayLineProvider>>,
    /// Semantic text object provider (language-server / tree-sitter protocol).
    pub(in crate::execution::engine) semantic_textobjects:
        Option<Box<dyn SemanticTextObjectProvider>>,
}

impl EngineProviders {
    /// Create empty engine providers (all `None`).
    pub(in crate::execution::engine) const fn new() -> Self {
        Self {
            custom_motions: None,
            custom_textobjects: None,
            custom_operators: None,
            syntax: None,
            fold: None,
            indent: None,
            display_lines: None,
            semantic_textobjects: None,
        }
    }

    /// Merge engine-level providers with per-call providers.
    ///
    /// Per-call providers from `InputContext` take precedence. Engine-level
    /// providers fill in the gaps. This produces a `Providers<'_>` with
    /// references that are valid for the duration of the borrow.
    pub(in crate::execution::engine) fn merge_with<'a>(
        &'a self,
        call_providers: &Providers<'a>,
    ) -> Providers<'a> {
        let mut merged = *call_providers;

        if merged.custom_motions.is_none() {
            if let Some(ref p) = self.custom_motions {
                merged.custom_motions = Some(p.as_ref());
            }
        }
        if merged.custom_textobjects.is_none() {
            if let Some(ref p) = self.custom_textobjects {
                merged.custom_textobjects = Some(p.as_ref());
            }
        }
        if merged.custom_operators.is_none() {
            if let Some(ref p) = self.custom_operators {
                merged.custom_operators = Some(p.as_ref());
            }
        }
        if merged.syntax.is_none() {
            if let Some(ref p) = self.syntax {
                merged.syntax = Some(p.as_ref());
            }
        }
        if merged.indent.is_none() {
            if let Some(ref p) = self.indent {
                merged.indent = Some(p.as_ref());
            }
        }
        if merged.fold.is_none() {
            if let Some(ref p) = self.fold {
                merged.fold = Some(p.as_ref());
            }
        }
        if merged.display_lines.is_none() {
            if let Some(ref p) = self.display_lines {
                merged.display_lines = Some(p.as_ref());
            }
        }
        if merged.semantic_textobjects.is_none() {
            if let Some(ref p) = self.semantic_textobjects {
                merged.semantic_textobjects = Some(p.as_ref());
            }
        }

        merged
    }

    /// Check if any engine-level provider is registered.
    #[must_use]
    pub(in crate::execution::engine) fn has_any(&self) -> bool {
        self.custom_motions.is_some()
            || self.custom_textobjects.is_some()
            || self.custom_operators.is_some()
            || self.syntax.is_some()
            || self.fold.is_some()
            || self.indent.is_some()
            || self.display_lines.is_some()
            || self.semantic_textobjects.is_some()
    }
}
