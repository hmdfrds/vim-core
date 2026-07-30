//! Document trait for vim-core.
//!
//! Read-only interface for text documents.
//! Shells implement this trait to provide text to the engine.
//!
//! # Design Rationale
//!
//! The Document trait is intentionally minimal. Line-level text operations
//! (line_start, line_end, char_at, etc.) are NOT part of this trait because
//! the command layer operates on raw `&str` via `commands::helpers`, which
//! provides SIMD-accelerated (`memchr`) line scanning. This trait exists
//! solely for the execution boundary: providing text to the engine and
//! position translation that only the shell can perform (e.g. rope-backed
//! offset↔position conversion).

use std::borrow::Cow;

use crate::primitives::{Offset, Position};

/// Read-only document interface.
///
/// The engine never mutates documents directly — it produces Effects
/// that the shell applies. This trait provides read access.
///
/// # Required Methods
///
/// - [`text()`](Document::text) — raw text access (used everywhere)
/// - [`line_count()`](Document::line_count) — number of lines (used by Ex commands)
/// - [`offset_to_pos()`](Document::offset_to_pos) — [`Offset`] → Position
///   (used by `InputContext` for cursor validation)
/// - [`pos_to_offset()`](Document::pos_to_offset) — Position → [`Offset`]
///   (used by `InputContext::from_position`)
///
/// # Not Here
///
/// Line-level operations (`line_start`, `line_end`, `char_at`, etc.) live in
/// `commands::helpers` as free functions on `&str`. This is intentional —
/// they use `memchr` SIMD scanning and don't need trait dispatch.
///
/// # Line Endings
///
/// The engine assumes LF (`\n`) line endings. Documents with CRLF (`\r\n`)
/// line endings must be normalized to LF before being passed to the engine.
/// The `\r` character is treated as a regular printable character, not as a
/// line separator. Hosts are responsible for normalizing on load and
/// denormalizing on save if needed.
pub trait Document {
    /// Get the full document text.
    fn text(&self) -> &str;

    /// Get the byte length of the document.
    #[inline]
    fn len(&self) -> usize {
        self.text().len()
    }

    /// Check if document is empty.
    #[inline]
    fn is_empty(&self) -> bool {
        self.text().is_empty()
    }

    /// Get the number of lines.
    fn line_count(&self) -> usize;

    /// Convert byte offset to Position (line, column).
    ///
    /// Returns None if offset is out of bounds.
    /// Only the shell can implement this efficiently (e.g. with a rope).
    fn offset_to_pos(&self, offset: Offset) -> Option<Position>;

    /// Convert Position to byte offset.
    ///
    /// Returns None if position is out of bounds.
    /// Only the shell can implement this efficiently (e.g. with a rope).
    fn pos_to_offset(&self, pos: Position) -> Option<Offset>;

    /// Get a substring of the document text.
    ///
    /// Equivalent to `&self.text()[start..end]`. Implementors may override
    /// for more efficient range access (e.g. avoiding full materialization
    /// in tree-backed documents).
    ///
    /// # Panics
    ///
    /// Panics if `start > end`, `end > self.len()`, or either index is not
    /// on a UTF-8 char boundary.
    fn slice(&self, start: usize, end: usize) -> Cow<'_, str> {
        Cow::Borrowed(&self.text()[start..end])
    }

    /// Find which line contains the given byte `offset` (0-indexed).
    ///
    /// Default scans `text()` for newlines. Implementors with line indexes
    /// should override for O(log n) lookup.
    fn line_of_offset(&self, offset: usize) -> usize {
        let bytes = self.text().as_bytes();
        let slice = bytes.get(..offset).unwrap_or(bytes);
        memchr::memchr_iter(b'\n', slice).count()
    }

    /// Access the underlying `VimText` tree, if this document is tree-backed.
    ///
    /// Returns `None` for non-tree documents (String-backed, rope-backed, etc.).
    /// Commands can use this for O(log n) queries via tree summaries.
    fn vim_text_tree(&self) -> Option<&vim_text::VimText> {
        None
    }

    /// Monotonic text generation counter.
    ///
    /// Hosts that maintain a version counter for their document (e.g.,
    /// Godot's `TextEdit::get_version()`, or an editor's own document
    /// version number) should override this to return `Some(version)`.
    /// The engine uses this as a fast-path to skip full text comparison
    /// when the generation has not changed since the last `process()` call.
    ///
    /// Returns `None` by default (no fast-path; engine falls back to
    /// byte-level shadow comparison).
    fn text_generation(&self) -> Option<u64> {
        None
    }
}
