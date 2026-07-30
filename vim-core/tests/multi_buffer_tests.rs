//! Integration tests for the multi-buffer architecture.
//!
//! Tests cross-buffer read access via `BufferScope`/`BufferLens`,
//! the `MultiBufferView` API surface, `CrossBufferEdit` effect emission,
//! and capability gating.

use compact_str::CompactString;

use vim_core::document::{BufferLens, BufferMeta, BufferScope, Document};
use vim_core::effects::Effect;
use vim_core::execution::{ApiError, HostSession, InvocationContext, MultiBufferView, VimApi};
use vim_core::primitives::{BufferId, CallerId, CapabilityTier, Offset, Position, TextEdit};

// ===========================================================================
// Mock BufferScope
// ===========================================================================

/// Simple in-memory document for mocking.
struct MockDoc {
    text: String,
}

impl MockDoc {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }
}

impl Document for MockDoc {
    fn text(&self) -> &str {
        &self.text
    }

    fn line_count(&self) -> usize {
        self.text.split('\n').count()
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
            offset += line.len() + 1;
        }
        None
    }
}

/// Mock multi-buffer scope implementation for testing.
struct SimpleMockScope {
    ids: Vec<BufferId>,
    docs: Vec<MockDoc>,
}

impl SimpleMockScope {
    fn new(entries: Vec<(u64, &str)>) -> Self {
        let ids: Vec<_> = entries.iter().map(|(id, _)| BufferId::new(*id)).collect();
        let docs: Vec<_> = entries
            .into_iter()
            .map(|(_, text)| MockDoc::new(text))
            .collect();
        Self { ids, docs }
    }
}

impl BufferScope for SimpleMockScope {
    fn buffer_ids(&self) -> &[BufferId] {
        &self.ids
    }

    fn buffer_meta(&self, id: BufferId) -> Option<BufferMeta> {
        self.ids.iter().position(|bid| *bid == id).map(|idx| {
            let doc = &self.docs[idx];
            BufferMeta {
                id,
                path: Some(CompactString::from(format!("file_{}.rs", id.get()))),
                line_count: doc.line_count(),
                byte_len: doc.len(),
                modified: false,
                generation: 1,
            }
        })
    }

    fn buffer_lens(&self, id: BufferId) -> Option<BufferLens<'_>> {
        self.ids.iter().position(|bid| *bid == id).map(|idx| {
            let doc = &self.docs[idx];
            let meta = BufferMeta {
                id,
                path: Some(CompactString::from(format!("file_{}.rs", id.get()))),
                line_count: doc.line_count(),
                byte_len: doc.len(),
                modified: false,
                generation: 1,
            };
            BufferLens::new(id, doc, meta)
        })
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

fn host_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Host, CapabilityTier::Mutating)
}

fn readonly_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Expression, CapabilityTier::ReadOnly)
}

// ===========================================================================
// 1. Single-buffer host returns None from buffers()
// ===========================================================================

#[test]
fn single_buffer_host_returns_none() {
    let session = HostSession::new("hello world");
    let api = VimApi::from_session(&session, host_ctx());

    // HostSession does not implement buffer_scope(), so buffers() should be None.
    assert!(api.buffers().is_none());
}

// ===========================================================================
// 2. MultiBufferView with mock BufferScope
// ===========================================================================

#[test]
fn multi_buffer_view_lists_buffers() {
    let scope = SimpleMockScope::new(vec![
        (1, "first buffer\nline two"),
        (2, "second buffer"),
        (3, "third\nbuffer\nhere"),
    ]);

    let view = MultiBufferView::new(&scope);

    let ids = view.ids();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], BufferId::new(1));
    assert_eq!(ids[1], BufferId::new(2));
    assert_eq!(ids[2], BufferId::new(3));
}

#[test]
fn multi_buffer_view_meta() {
    let scope = SimpleMockScope::new(vec![(1, "hello\nworld"), (2, "single line")]);

    let view = MultiBufferView::new(&scope);

    let meta = view.meta(BufferId::new(1)).unwrap();
    assert_eq!(meta.id, BufferId::new(1));
    assert_eq!(meta.line_count, 2);
    assert_eq!(meta.byte_len, 11);
    assert!(!meta.modified);
    assert_eq!(meta.path, Some(CompactString::from("file_1.rs")));

    let meta2 = view.meta(BufferId::new(2)).unwrap();
    assert_eq!(meta2.line_count, 1);
    assert_eq!(meta2.byte_len, 11);

    // Non-existent buffer
    assert!(view.meta(BufferId::new(99)).is_none());
}

#[test]
fn multi_buffer_view_get_lens() {
    let scope = SimpleMockScope::new(vec![(1, "alpha\nbeta\ngamma"), (2, "delta")]);

    let view = MultiBufferView::new(&scope);

    let lens = view.get(BufferId::new(1)).unwrap();
    assert_eq!(lens.id(), BufferId::new(1));
    assert_eq!(lens.text(), "alpha\nbeta\ngamma");
    assert_eq!(lens.line_count(), 3);
    assert_eq!(
        lens.offset_to_pos(Offset::new(6)),
        Some(Position::from_raw(1, 0))
    );

    let lens2 = view.get(BufferId::new(2)).unwrap();
    assert_eq!(lens2.text(), "delta");
    assert_eq!(lens2.line_count(), 1);

    // Non-existent buffer
    assert!(view.get(BufferId::new(99)).is_none());
}

// ===========================================================================
// 3. CrossBufferEdit effect emission
// ===========================================================================

#[test]
fn cross_buffer_edit_emitted() {
    let session = HostSession::new("current buffer");
    let api = VimApi::from_session(&session, host_ctx());

    let edits = vec![
        TextEdit {
            start: 0,
            end: 5,
            text: CompactString::from("replaced"),
        },
        TextEdit {
            start: 10,
            end: 10,
            text: CompactString::from(" inserted"),
        },
    ];

    api.emit()
        .cross_buffer_edit(BufferId::new(42), &edits)
        .unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);

    match &effects[0] {
        Effect::CrossBufferEdit { target, edits: e } => {
            assert_eq!(*target, BufferId::new(42));
            assert_eq!(e.len(), 2);
            assert_eq!(e[0].start, 0);
            assert_eq!(e[0].end, 5);
            assert_eq!(e[0].text.as_str(), "replaced");
            assert_eq!(e[1].start, 10);
            assert_eq!(e[1].end, 10);
            assert_eq!(e[1].text.as_str(), " inserted");
        }
        other => panic!("Expected CrossBufferEdit, got {:?}", other),
    }
}

#[test]
fn cross_buffer_edit_empty_edits() {
    let session = HostSession::new("buf");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit().cross_buffer_edit(BufferId::new(1), &[]).unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::CrossBufferEdit { target, edits } => {
            assert_eq!(*target, BufferId::new(1));
            assert!(edits.is_empty());
        }
        _ => panic!("Expected CrossBufferEdit"),
    }
}

// ===========================================================================
// 4. Capability gating — ReadOnly tier cannot emit CrossBufferEdit
// ===========================================================================

#[test]
fn cross_buffer_edit_requires_mutating_tier() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, readonly_ctx());

    let edits = vec![TextEdit {
        start: 0,
        end: 1,
        text: CompactString::from("X"),
    }];

    let result = api.emit().cross_buffer_edit(BufferId::new(1), &edits);
    assert!(result.is_err());
    match result.unwrap_err() {
        ApiError::InsufficientTier { required, have } => {
            assert_eq!(required, CapabilityTier::Mutating);
            assert_eq!(have, CapabilityTier::ReadOnly);
        }
        other => panic!("Expected InsufficientTier, got {:?}", other),
    }
}

// ===========================================================================
// 5. BufferLens Document trait compliance
// ===========================================================================

#[test]
fn buffer_lens_slice_and_line_of_offset() {
    let scope = SimpleMockScope::new(vec![(1, "line one\nline two\nline three")]);
    let view = MultiBufferView::new(&scope);
    let lens = view.get(BufferId::new(1)).unwrap();

    // Document trait: text(), len(), is_empty(), line_count()
    assert_eq!(lens.text(), "line one\nline two\nline three");
    assert_eq!(lens.len(), 28);
    assert!(!lens.is_empty());
    assert_eq!(lens.line_count(), 3);

    // offset_to_pos for various positions
    assert_eq!(
        lens.offset_to_pos(Offset::new(0)),
        Some(Position::from_raw(0, 0))
    );
    assert_eq!(
        lens.offset_to_pos(Offset::new(9)),
        Some(Position::from_raw(1, 0))
    );
    assert_eq!(
        lens.offset_to_pos(Offset::new(18)),
        Some(Position::from_raw(2, 0))
    );

    // pos_to_offset round-trip
    assert_eq!(
        lens.pos_to_offset(Position::from_raw(0, 4)),
        Some(Offset::new(4))
    );
    assert_eq!(
        lens.pos_to_offset(Position::from_raw(1, 5)),
        Some(Offset::new(14))
    );
}

// ===========================================================================
// 6. BufferMeta equality and debug
// ===========================================================================

#[test]
fn buffer_meta_equality() {
    let meta1 = BufferMeta {
        id: BufferId::new(1),
        path: Some(CompactString::from("a.rs")),
        line_count: 10,
        byte_len: 100,
        modified: false,
        generation: 5,
    };
    let meta2 = meta1.clone();
    assert_eq!(meta1, meta2);

    let meta3 = BufferMeta {
        id: BufferId::new(2),
        ..meta1.clone()
    };
    assert_ne!(meta1, meta3);
}
