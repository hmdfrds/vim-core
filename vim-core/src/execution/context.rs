//! Execution context.
//!
//! This module implements the input/output separation principle with
//! compile-time safety guarantees using the type-state pattern.
//!
//! # Architecture
//!
//! ```text
//! Shell                         VimEngine
//!   │                              │
//!   │  InputContext<Unvalidated>   │
//!   │──────────────────────────────│
//!   │         .validate()?         │
//!   │              │               │
//!   │              ▼               │
//!   │  InputContext<Validated>     │
//!   │──────────────────────────────▶ process()
//!   │                              │
//!   │◀──────────────────────────────│
//!   │           Effects             │
//! ```
//!
//! # Type-State Pattern
//!
//! Contexts are created in [`Unvalidated`] state and must be validated
//! before use:
//!
//! ```ignore
//! // Compile error - can't pass unvalidated context
//! let ctx = InputContext::new(&doc, 999999);
//! engine.process(key, ctx);  // ❌ Won't compile!
//!
//! // Correct - validation required
//! let ctx = InputContext::new(&doc, cursor).validate()?;
//! engine.process(key, ctx);  // ✅ Compiles
//! ```
//!
//! # Zero-Cost Abstraction
//!
//! All type-state markers are zero-sized. In release builds, this compiles
//! to identical code as a simple struct with no overhead.

use crate::document::Document;
use crate::primitives::{BufferId, Offset, Position, VimOptions};
use crate::state::VimState;
use std::cell::Cell;
use std::marker::PhantomData;
use thiserror::Error;

// ═══════════════════════════════════════════════════════════════════════════
// Type-State Markers (PhantomData pattern)
// ═══════════════════════════════════════════════════════════════════════════

mod private {
    /// Sealed trait - prevents external implementation.
    #[allow(
        dead_code,
        reason = "sealed trait pattern; implemented but never called directly"
    )]
    pub trait Sealed {}
}

/// Marker: context has not been validated.
///
/// An [`InputContext<Unvalidated>`] cannot be passed to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Unvalidated;

/// Marker: context has been validated and is safe to use.
///
/// Only an [`InputContext<Validated>`] can be passed to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Validated;

impl private::Sealed for Unvalidated {}
impl private::Sealed for Validated {}

// ═══════════════════════════════════════════════════════════════════════════
// Error Types (thiserror)
// ═══════════════════════════════════════════════════════════════════════════

/// Errors that can occur during context creation/validation.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContextError {
    /// Cursor position exceeds document bounds.
    #[error("cursor offset {cursor} exceeds document length {doc_len}")]
    CursorOutOfBounds {
        /// The requested cursor position.
        cursor: usize,
        /// The document length.
        doc_len: usize,
    },

    /// Position doesn't map to a valid offset.
    #[error("invalid position: line {line}, col {col}")]
    InvalidPosition {
        /// The requested line.
        line: usize,
        /// The requested column.
        col: usize,
    },
}

/// Result type for context operations.
pub type ContextResult<T> = Result<T, ContextError>;

// ═══════════════════════════════════════════════════════════════════════════
// InputContext
// ═══════════════════════════════════════════════════════════════════════════

/// Input context for processing a keystroke.
///
/// Contains all read-only state needed from the shell to execute a command.
/// This is the "input" side of the pure function:
///
/// ```text
/// process(key, context) → effects
/// ```
///
/// # Design Philosophy
///
/// Explicit dependencies over implicit. By requiring the shell to pass
/// cursor position each call, we ensure:
///
/// 1. **Testability** - Same doc, different cursors → different tests
/// 2. **Purity** - No hidden state, deterministic behavior
/// 3. **Correctness** - Cursor always from authoritative source (shell)
///
/// # Type-State Pattern
///
/// Contexts are created in [`Unvalidated`] state and must be validated:
///
/// ```ignore
/// let ctx = InputContext::new(&doc, cursor)
///     .validate()?;  // Returns ContextResult<InputContext<_, Validated>>
/// ```
///
/// This ensures at **compile time** that only valid contexts reach the engine.
/// Invalid cursors are caught during validation, not during execution.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```ignore
/// use vim_core::execution::InputContext;
///
/// let doc = TestDocument::new("hello world");
/// let ctx = InputContext::new(&doc, 5).validate()?;
/// let response = engine.process(Key::Char('l'), ctx);
/// ```
///
/// ## With Clamping (Never Fails)
///
/// ```ignore
/// // If cursor is out of bounds, clamp to document end
/// let ctx = InputContext::new(&doc, 999999).validate_clamped();
/// // ctx.cursor_offset() <= doc.len() guaranteed
/// ```
///
/// ## Building from Position
///
/// ```ignore
/// let pos = Position::from_raw(5, 10);  // Line 5, Col 10
/// let ctx = InputContext::from_position(&doc, pos).validate()?;
/// ```
///
/// # Performance
///
/// - Zero allocation on creation
/// - All methods `#[inline]` for elimination
/// - Position computed lazily via `cursor_pos()`
/// - Type-state markers are zero-sized (no runtime cost)
///
/// # Optional State
///
/// Optional state (selection, viewport, providers, buffer id) is attached
/// after validation via the `with_*` builder methods.
#[derive(Debug, Clone)]
#[must_use = "context must be passed to engine.process()"]
pub struct InputContext<'doc, D: Document, State = Validated> {
    /// Read-only document access.
    doc: &'doc D,

    /// Current cursor byte offset (type-safe newtype).
    cursor: Offset,

    /// Lazily-cached cursor position.
    ///
    /// Populated eagerly by `from_position()`, or lazily on first
    /// `cursor_pos()` call. `Cell` enables caching through `&self`.
    cursor_pos: Cell<Option<Position>>,

    /// Visual mode selection.
    selection: Option<crate::primitives::SelectionRange>,
    /// Viewport info for H/M/L motions.
    viewport: Option<crate::dispatch::ViewportInfo>,
    /// Capability providers from the shell (fold, display-line, search).
    providers: crate::document::Providers<'doc>,
    /// Buffer identifier for cross-buffer jump list navigation.
    ///
    /// Set by the host via `with_buffer_id()`. Used to tag jump list entries
    /// with their source buffer, enabling Ctrl-O / Ctrl-I across buffers.
    buffer_id: Option<BufferId>,
    /// Type-state marker (zero-sized).
    _state: PhantomData<State>,
}

// Manual PartialEq/Eq: the `cursor_pos` Cell is a transparent cache,
// not semantic state. Two contexts are equal if their inputs match.
impl<D: Document + PartialEq, State> PartialEq for InputContext<'_, D, State> {
    fn eq(&self, other: &Self) -> bool {
        self.cursor == other.cursor
            && self.doc == other.doc
            && self.selection == other.selection
            && self.viewport == other.viewport
            && self.buffer_id == other.buffer_id
    }
}

impl<D: Document + Eq, State> Eq for InputContext<'_, D, State> {}

// ─────────────────────────────────────────────────────────────────────────────
// Unvalidated Methods
// ─────────────────────────────────────────────────────────────────────────────

impl<'doc, D: Document> InputContext<'doc, D, Unvalidated> {
    /// Create a new unvalidated input context.
    ///
    /// The context must be validated before it can be passed to the engine.
    ///
    /// # Arguments
    ///
    /// * `doc` - Document reference (text access)
    /// * `cursor_offset` - Cursor byte offset (will be validated)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ctx = InputContext::new(&doc, 5);
    /// // ctx is Unvalidated - must call validate() before use
    /// let validated = ctx.validate()?;
    /// ```
    #[inline]
    pub fn new(doc: &'doc D, cursor_offset: usize) -> Self {
        Self {
            doc,
            cursor: Offset::new(cursor_offset),
            cursor_pos: Cell::new(None),
            selection: None,
            viewport: None,
            providers: crate::document::Providers::new(),
            buffer_id: None,
            _state: PhantomData,
        }
    }

    /// Create from a Position (line, col).
    ///
    /// The offset is computed from the position. If the position is invalid,
    /// offset 0 is used (will fail validation if doc is empty).
    #[inline]
    pub fn from_position(doc: &'doc D, pos: Position) -> Self {
        let offset = doc.pos_to_offset(pos).unwrap_or(Offset::ZERO);
        Self {
            doc,
            cursor: offset,
            cursor_pos: Cell::new(Some(pos)),
            selection: None,
            viewport: None,
            providers: crate::document::Providers::new(),
            buffer_id: None,
            _state: PhantomData,
        }
    }

    /// Validate the context.
    ///
    /// Returns `Ok(InputContext<Validated>)` if cursor is within bounds,
    /// otherwise returns `Err(ContextError)`.
    ///
    /// # Errors
    ///
    /// - [`ContextError::CursorOutOfBounds`] if cursor > `doc.len()`
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ctx = InputContext::new(&doc, 5).validate()?;
    /// // Now ctx is Validated and can be used
    /// ```
    #[inline]
    pub fn validate(self) -> ContextResult<InputContext<'doc, D, Validated>> {
        let doc_len = self.doc.len();
        if self.cursor.get() > doc_len {
            return Err(ContextError::CursorOutOfBounds {
                cursor: self.cursor.get(),
                doc_len,
            });
        }

        // Snap cursor to UTF-8 char boundary (Neovim's mark_mb_adjustpos equivalent).
        // The shell may pass an offset that lands inside a multibyte character
        // after an external edit or resize.
        let snapped =
            crate::primitives::text_util::snap_to_char_boundary(self.doc.text(), self.cursor.get());
        let cursor = Offset::new(snapped);
        let cursor_pos = if snapped == self.cursor.get() {
            self.cursor_pos.get()
        } else {
            None // Position cache invalid after snap
        };
        Ok(self.transition(cursor, cursor_pos))
    }

    /// Validate, clamping cursor to document bounds.
    ///
    /// Never fails - cursor is clamped if out of bounds.
    /// Use this when you want guaranteed success.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Even with invalid cursor, this never fails
    /// let ctx = InputContext::new(&doc, 999999).validate_clamped();
    /// assert!(ctx.cursor_offset_raw() <= doc.text().len());
    /// ```
    #[inline]
    pub fn validate_clamped(self) -> InputContext<'doc, D, Validated> {
        let doc_len = self.doc.len();
        let clamped = self.cursor.get().min(doc_len);
        // Snap to UTF-8 char boundary after clamping.
        let snapped = crate::primitives::text_util::snap_to_char_boundary(self.doc.text(), clamped);
        let cursor = Offset::new(snapped);
        let cursor_pos = if snapped == self.cursor.get() {
            self.cursor_pos.get()
        } else {
            None
        };
        self.transition(cursor, cursor_pos)
    }

    /// Shared type-state transition: moves all fields into a Validated context.
    ///
    /// Only `cursor` and `cursor_pos` vary between validate paths.
    /// All other fields are transferred identically — adding a new field
    /// only requires updating this single method.
    #[inline]
    const fn transition(
        self,
        cursor: Offset,
        cursor_pos: Option<Position>,
    ) -> InputContext<'doc, D, Validated> {
        InputContext {
            doc: self.doc,
            cursor,
            cursor_pos: Cell::new(cursor_pos),
            selection: self.selection,
            viewport: self.viewport,
            providers: self.providers,
            buffer_id: self.buffer_id,
            _state: PhantomData,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Validated Methods (only available after validation)
// ─────────────────────────────────────────────────────────────────────────────

impl<'doc, D: Document> InputContext<'doc, D, Validated> {
    /// Get cursor byte offset (type-safe).
    ///
    /// Returns an [`Offset`] newtype for type safety.
    #[inline]
    pub const fn cursor_offset(&self) -> Offset {
        self.cursor
    }

    /// Get cursor byte offset as raw usize.
    ///
    /// Prefer `cursor_offset()` for type safety.
    #[inline]
    pub const fn cursor_offset_raw(&self) -> usize {
        self.cursor.get()
    }

    /// Get cursor position (line, col).
    ///
    /// Lazily computed on first call and cached for subsequent calls.
    /// Uses `Cell` interior mutability for zero-cost caching through `&self`.
    #[inline]
    pub fn cursor_pos(&self) -> Position {
        if let Some(pos) = self.cursor_pos.get() {
            return pos;
        }
        let pos = self
            .doc
            .offset_to_pos(self.cursor)
            .unwrap_or(Position::ORIGIN);
        self.cursor_pos.set(Some(pos));
        pos
    }

    /// Get document reference.
    ///
    /// Returns `&'doc D` (the document's own lifetime, not tied to `&self`).
    /// This allows callers to extract the document reference before moving
    /// the `InputContext`, avoiding unnecessary text clones.
    #[inline]
    pub const fn doc(&self) -> &'doc D {
        self.doc
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Builder Methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Add selection to context.
    ///
    /// ```ignore
    /// let ctx = InputContext::new(&doc, 0)
    ///     .validate()?
    ///     .with_selection(SelectionRange::new(Offset::new(0), Offset::new(10)));
    /// ```
    #[inline]
    pub const fn with_selection(mut self, sel: crate::primitives::SelectionRange) -> Self {
        self.selection = Some(sel);
        self
    }

    /// Get selection if visual mode is active.
    #[inline]
    pub const fn selection(&self) -> Option<crate::primitives::SelectionRange> {
        self.selection
    }

    /// Set viewport info for H/M/L motions.
    #[inline]
    pub const fn with_viewport(mut self, viewport: crate::dispatch::ViewportInfo) -> Self {
        self.viewport = Some(viewport);
        self
    }

    /// Get viewport info.
    #[inline]
    pub const fn viewport(&self) -> Option<crate::dispatch::ViewportInfo> {
        self.viewport
    }

    /// Set capability providers (fold, display-line, search).
    #[inline]
    pub const fn with_providers(mut self, providers: crate::document::Providers<'doc>) -> Self {
        self.providers = providers;
        self
    }

    /// Get capability providers.
    #[inline]
    pub const fn providers(&self) -> &crate::document::Providers<'doc> {
        &self.providers
    }

    /// Set buffer identifier for cross-buffer jump list navigation.
    ///
    /// The host should call this with a unique `BufferId` for each open buffer
    /// so that jump list entries are tagged with their source buffer.
    ///
    /// ```ignore
    /// let ctx = InputContext::new(&doc, 0)
    ///     .validate()?
    ///     .with_buffer_id(BufferId::new(1));
    /// ```
    #[inline]
    pub const fn with_buffer_id(mut self, id: BufferId) -> Self {
        self.buffer_id = Some(id);
        self
    }

    /// Get the buffer identifier, if set by the host.
    #[inline]
    pub const fn buffer_id(&self) -> Option<BufferId> {
        self.buffer_id
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Trait Implementations (From/Into, AsRef)
// ─────────────────────────────────────────────────────────────────────────────

/// Allow `ctx.as_ref()` to get document reference.
impl<D: Document> AsRef<D> for InputContext<'_, D, Validated> {
    #[inline]
    fn as_ref(&self) -> &D {
        self.doc
    }
}

/// Allow `InputContext::from((&doc, cursor))`.
impl<'doc, D: Document> From<(&'doc D, usize)> for InputContext<'doc, D, Unvalidated> {
    #[inline]
    fn from((doc, cursor): (&'doc D, usize)) -> Self {
        Self::new(doc, cursor)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ExecutionContext (Internal)
// ═══════════════════════════════════════════════════════════════════════════

/// Internal execution context (combines validated input + engine state).
///
/// This struct is created internally by the engine and passed to the executor.
/// It bundles the validated shell input with the current vim state.
///
/// # Invariants
///
/// - Input is always validated (enforced by type system)
/// - State reference is always valid for the duration of execution
///
/// Note: PartialEq/Eq not derived because `VimState` doesn't implement them.
#[derive(Debug)]
#[must_use = "execution context must be passed to executor"]
pub struct ExecutionContext<'ctx, D: Document> {
    /// Validated input from shell (document + cursor).
    pub input: InputContext<'ctx, D, Validated>,
    /// Vim engine state reference.
    pub state: &'ctx VimState,
    /// User-configurable options (tabstop, shiftwidth, etc.).
    pub options: &'ctx VimOptions,
}

impl<'ctx, D: Document> ExecutionContext<'ctx, D> {
    /// Create from validated input, state, and options.
    #[inline]
    pub const fn new(
        input: InputContext<'ctx, D, Validated>,
        state: &'ctx VimState,
        options: &'ctx VimOptions,
    ) -> Self {
        Self {
            input,
            state,
            options,
        }
    }

    /// Convenience: get document reference.
    #[inline]
    pub const fn doc(&self) -> &D {
        self.input.doc()
    }

    /// Convenience: get cursor offset as usize.
    #[inline]
    pub const fn cursor_offset(&self) -> usize {
        self.input.cursor_offset_raw()
    }

    /// Convenience: get cursor position.
    #[inline]
    pub fn cursor_pos(&self) -> Position {
        self.input.cursor_pos()
    }

    /// Convenience: get visual mode selection if present.
    #[inline]
    pub const fn selection(&self) -> Option<crate::primitives::SelectionRange> {
        self.input.selection()
    }

    /// Convenience: get last find state for ; and , repeat.
    #[inline]
    pub fn last_find(&self) -> Option<crate::primitives::LastFind> {
        self.state.last_find()
    }

    /// Convenience: get viewport info for H/M/L motions.
    #[inline]
    pub const fn viewport(&self) -> Option<crate::dispatch::ViewportInfo> {
        self.input.viewport()
    }

    /// Convenience: get capability providers.
    #[inline]
    pub const fn providers(&self) -> &crate::document::Providers<'_> {
        self.input.providers()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::SimpleDocument;

    /// Minimal document for testing.
    fn test_doc(text: &str) -> SimpleDocument {
        SimpleDocument::new(text)
    }

    #[test]
    fn valid_cursor_validates() {
        let doc = test_doc("hello");
        let ctx = InputContext::new(&doc, 3).validate();
        assert!(ctx.is_ok());
        assert_eq!(ctx.unwrap().cursor_offset_raw(), 3);
    }

    #[test]
    fn cursor_at_end_validates() {
        let doc = test_doc("hello"); // len = 5
        let ctx = InputContext::new(&doc, 5).validate();
        assert!(ctx.is_ok());
    }

    #[test]
    fn cursor_past_end_fails() {
        let doc = test_doc("hello"); // len = 5
        let ctx = InputContext::new(&doc, 6).validate();
        assert!(ctx.is_err());
        assert_eq!(
            ctx.unwrap_err(),
            ContextError::CursorOutOfBounds {
                cursor: 6,
                doc_len: 5
            }
        );
    }

    #[test]
    fn validate_clamped_never_fails() {
        let doc = test_doc("hi"); // len = 2
        let ctx = InputContext::new(&doc, 9999).validate_clamped();
        assert_eq!(ctx.cursor_offset_raw(), 2); // Clamped to end
    }

    #[test]
    fn from_tuple_works() {
        let doc = test_doc("test");
        let ctx: InputContext<_, Unvalidated> = (&doc, 2).into();
        let validated = ctx.validate().unwrap();
        assert_eq!(validated.cursor_offset_raw(), 2);
    }

    #[test]
    fn from_position_works() {
        let doc = test_doc("hello");
        let pos = Position::ORIGIN;
        let ctx = InputContext::from_position(&doc, pos);
        let validated = ctx.validate().unwrap();
        assert_eq!(validated.cursor_offset_raw(), 0);
        assert_eq!(validated.cursor_pos(), pos);
    }

    #[test]
    fn empty_document_cursor_zero_validates() {
        let doc = test_doc("");
        let ctx = InputContext::new(&doc, 0).validate();
        assert!(ctx.is_ok());
    }

    #[test]
    fn empty_document_cursor_one_fails() {
        let doc = test_doc("");
        let ctx = InputContext::new(&doc, 1).validate();
        assert!(ctx.is_err());
    }

    #[test]
    fn asref_trait_works() {
        let doc = test_doc("test");
        let ctx = InputContext::new(&doc, 0).validate_clamped();
        let doc_ref: &SimpleDocument = ctx.as_ref();
        assert_eq!(doc_ref.text(), "test");
    }

    #[test]
    fn providers_none_by_default() {
        let doc = test_doc("hello");
        let ctx = InputContext::new(&doc, 0).validate_clamped();
        assert!(!ctx.providers().has_any());
    }

    #[test]
    fn providers_bundle_builder() {
        use crate::document::{FoldProvider, Providers};
        use crate::primitives::Direction;

        struct MockFold;
        impl FoldProvider for MockFold {
            fn next_visible_line(
                &self,
                line: crate::primitives::LineNumber,
                _dir: Direction,
            ) -> crate::primitives::LineNumber {
                line
            }
            fn is_folded(&self, _line: crate::primitives::LineNumber) -> bool {
                false
            }
        }

        let doc = test_doc("hello\nworld");
        let fold = MockFold;
        let providers = Providers::new().with_fold(&fold);
        let ctx = InputContext::new(&doc, 0)
            .validate_clamped()
            .with_providers(providers);
        assert!(ctx.providers().fold.is_some());
        assert!(!ctx
            .providers()
            .fold
            .unwrap()
            .is_folded(crate::primitives::LineNumber::new(0)));
    }
}
