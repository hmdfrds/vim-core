//! Document abstraction for vim-core.
//!
//! The Document trait provides a read-only interface to text.
//! Capability provider traits allow shells to expose folding, wrapping,
//! and search capabilities to the engine.
//!
//! # Layering
//!
//! Document is a bottom layer alongside `primitives`. It imports
//! `primitives` and nothing else, test code included.
//!
//! # Design
//!
//! The Document trait is intentionally minimal (4 required methods).
//! Line-level operations live in `commands::helpers` as free functions
//! on `&str` with SIMD-accelerated `memchr` scanning.
//!
//! Capability provider traits (`FoldProvider`, `DisplayLineProvider`,
//! `SearchProvider`) are optional — shells implement them to enable
//! fold-aware navigation, soft-wrap motions, and regex search.
//!
//! # Testing
//!
//! `SimpleDocument` (canonical test impl) lives in `test_utils/`, not here,
//! because it needs `commands::helpers` which is a higher layer.
//! See `test_utils::simple_document` for the implementation.

mod buffer_scope;
mod cached_symbol_tree;
mod custom;
mod display_lines;
mod fold;
mod host_display_lines;
mod host_fold;
mod indent;
mod providers;
mod search;
mod semantic;
mod syntax;
#[allow(dead_code)] // No consumers yet; BufferView will wire these in.
pub(crate) mod text_pairs;
mod traits;

pub use buffer_scope::{BufferLens, BufferMeta, BufferScope};
pub use custom::{
    CustomMotionProvider, CustomMotionResult, CustomOperatorProvider, CustomOperatorResult,
    CustomTextObjectProvider, CustomTextObjectResult,
};
pub use display_lines::DisplayLineProvider;
pub use fold::FoldProvider;
pub use host_display_lines::HostDisplayLineProvider;
pub use host_fold::HostFoldProvider;
pub use indent::{
    detect_indent_style, CachedIndentAction, CachedIndentHint, IndentProvider, IndentResult,
    IndentStyle,
};
pub use providers::Providers;
pub use search::SearchProvider;
pub use syntax::{SyntaxNodeKind, SyntaxProvider};
pub use traits::Document;

pub use cached_symbol_tree::{symbol_kind_to_semantic_object, CachedSymbol, CachedSymbolTree};
pub use semantic::{SemanticTextObjectProvider, SemanticTextObjectResult};
