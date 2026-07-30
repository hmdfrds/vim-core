//! Multi-buffer provider trait and associated types.
//!
//! Extends the [`Document`] concept to cross-buffer access. Only multi-buffer
//! hosts with multiple open editors implement [`BufferScope`].
//! Single-buffer hosts (e.g., Godot's TextEdit) ignore this entirely.

use compact_str::CompactString;

use crate::document::Document;
use crate::primitives::{BufferId, Offset, Position};

/// Lightweight metadata about a buffer without materializing its full text.
///
/// Provides cheap introspection for commands that need to enumerate open
/// buffers (e.g., `:ls`, `:bnext`) without paying the cost of accessing
/// the full document text.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BufferMeta {
    /// Unique identifier for this buffer.
    pub id: BufferId,
    /// File path, if the buffer is associated with a file on disk.
    pub path: Option<CompactString>,
    /// Number of lines in the buffer.
    pub line_count: usize,
    /// Total byte length of the buffer text.
    pub byte_len: usize,
    /// Whether the buffer has unsaved modifications.
    pub modified: bool,
    /// Monotonic generation counter (increments on each edit).
    pub generation: u64,
}

/// A lifetime-bounded, read-only view into a non-current buffer.
///
/// The `'a` lifetime ties this lens to the host's borrow scope, making
/// stale-access bugs impossible by construction. Cannot escape the
/// handler invocation that created it.
pub struct BufferLens<'a> {
    id: BufferId,
    doc: &'a dyn Document,
    meta: BufferMeta,
}

impl<'a> BufferLens<'a> {
    /// Create a new buffer lens from a buffer ID, document reference, and metadata.
    pub fn new(id: BufferId, doc: &'a dyn Document, meta: BufferMeta) -> Self {
        Self { id, doc, meta }
    }

    /// The buffer's unique identifier.
    #[inline]
    #[must_use]
    pub const fn id(&self) -> BufferId {
        self.id
    }

    /// Metadata for this buffer (path, line count, modified status, etc.).
    #[inline]
    #[must_use]
    pub const fn meta(&self) -> &BufferMeta {
        &self.meta
    }
}

impl Document for BufferLens<'_> {
    fn text(&self) -> &str {
        self.doc.text()
    }

    fn line_count(&self) -> usize {
        self.doc.line_count()
    }

    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        self.doc.offset_to_pos(offset)
    }

    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        self.doc.pos_to_offset(pos)
    }
}

/// Optional trait for hosts that manage multiple buffers.
///
/// Single-buffer hosts (e.g., Godot's TextEdit) ignore this entirely.
/// The engine only calls through this when a command explicitly needs
/// cross-buffer access, gated by [`HostCapability::MultiBuffer`](crate::execution::host_api::HostCapability::MultiBuffer).
pub trait BufferScope {
    /// List all open buffer IDs.
    fn buffer_ids(&self) -> &[BufferId];

    /// Get metadata for a specific buffer without materializing its text.
    fn buffer_meta(&self, id: BufferId) -> Option<BufferMeta>;

    /// Get a read-only lens into a specific buffer's document.
    fn buffer_lens(&self, id: BufferId) -> Option<BufferLens<'_>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;

    /// Minimal Document implementation for testing.
    struct TestDoc {
        text: String,
    }

    impl TestDoc {
        fn new(text: &str) -> Self {
            Self {
                text: text.to_owned(),
            }
        }
    }

    impl Document for TestDoc {
        fn text(&self) -> &str {
            &self.text
        }

        fn line_count(&self) -> usize {
            self.text.lines().count().max(1)
        }

        fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
            let off = offset.get();
            if off > self.text.len() {
                return None;
            }
            let line = self.text[..off].matches('\n').count();
            let line_start = self.text[..off].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let col = off - line_start;
            Some(Position::from_raw(line, col))
        }

        fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
            let mut offset = 0;
            for (i, line) in self.text.split('\n').enumerate() {
                if i == pos.line().get() {
                    let col = pos.col().get();
                    if col <= line.len() {
                        return Some(Offset::new(offset + col));
                    }
                    return None;
                }
                offset += line.len() + 1; // +1 for '\n'
            }
            None
        }
    }

    #[test]
    fn buffer_lens_implements_document() {
        let doc = TestDoc::new("hello\nworld");
        let meta = BufferMeta {
            id: BufferId::new(1),
            path: Some(CompactString::from("test.txt")),
            line_count: 2,
            byte_len: 11,
            modified: false,
            generation: 1,
        };
        let lens = BufferLens::new(BufferId::new(1), &doc, meta.clone());

        assert_eq!(lens.id(), BufferId::new(1));
        assert_eq!(lens.meta(), &meta);
        assert_eq!(lens.text(), "hello\nworld");
        assert_eq!(lens.line_count(), 2);
        assert_eq!(
            lens.offset_to_pos(Offset::new(6)),
            Some(Position::from_raw(1, 0))
        );
        assert_eq!(
            lens.pos_to_offset(Position::from_raw(0, 0)),
            Some(Offset::new(0))
        );
    }

    #[test]
    fn buffer_meta_construction() {
        let meta = BufferMeta {
            id: BufferId::new(42),
            path: None,
            line_count: 100,
            byte_len: 5000,
            modified: true,
            generation: 7,
        };
        assert_eq!(meta.id, BufferId::new(42));
        assert!(meta.path.is_none());
        assert_eq!(meta.line_count, 100);
        assert_eq!(meta.byte_len, 5000);
        assert!(meta.modified);
        assert_eq!(meta.generation, 7);
    }

    #[test]
    fn buffer_lens_delegates_all_document_methods() {
        let doc = TestDoc::new("abc\ndef\nghi");
        let meta = BufferMeta {
            id: BufferId::new(3),
            path: Some(CompactString::from("/tmp/foo.rs")),
            line_count: 3,
            byte_len: 11,
            modified: false,
            generation: 0,
        };
        let lens = BufferLens::new(BufferId::new(3), &doc, meta);

        // Document trait methods
        assert_eq!(lens.text(), "abc\ndef\nghi");
        assert_eq!(lens.line_count(), 3);
        assert_eq!(lens.len(), 11);
        assert!(!lens.is_empty());
        assert_eq!(
            lens.offset_to_pos(Offset::new(4)),
            Some(Position::from_raw(1, 0))
        );
        assert_eq!(
            lens.pos_to_offset(Position::from_raw(2, 1)),
            Some(Offset::new(9))
        );
    }
}
