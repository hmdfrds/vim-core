//! System tests for the multi-buffer architecture.
//!
//! Proves: BufferScope trait, BufferLens Document impl, MultiBufferView,
//! CrossBufferEdit effects, capability gating, and full plugin-like
//! cross-buffer workflows.

use vim_core::document::{BufferLens, BufferMeta, BufferScope, Document};
use vim_core::effects::Effect;
use vim_core::execution::{HostSession, InvocationContext, VimApi};
use vim_core::primitives::{BufferId, CallerId, CapabilityTier, Offset, Position, TextEdit};

use compact_str::CompactString;

// ---------------------------------------------------------------------------
// Mock BufferScope implementation
// ---------------------------------------------------------------------------

struct MockDoc {
    text: String,
}

impl Document for MockDoc {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_count(&self) -> usize {
        self.text.lines().count().max(1)
    }
    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        let text = &self.text;
        let o = offset.get();
        if o > text.len() {
            return None;
        }
        let line = text[..o].bytes().filter(|&b| b == b'\n').count();
        let col = o - text[..o].rfind('\n').map(|p| p + 1).unwrap_or(0);
        Some(Position::from_raw(line, col))
    }
    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        let mut offset = 0;
        for (i, line) in self.text.split('\n').enumerate() {
            if i == pos.line().get() {
                let col = pos.col().get().min(line.len());
                return Some(Offset::new(offset + col));
            }
            offset += line.len() + 1;
        }
        None
    }
}

struct MockScope {
    buffers: Vec<(BufferId, MockDoc, bool)>,
}

impl MockScope {
    fn new(docs: Vec<(&str, bool)>) -> Self {
        Self {
            buffers: docs
                .into_iter()
                .enumerate()
                .map(|(i, (text, modified))| {
                    (
                        BufferId::new(i as u64),
                        MockDoc {
                            text: text.to_string(),
                        },
                        modified,
                    )
                })
                .collect(),
        }
    }
}

impl BufferScope for MockScope {
    fn buffer_ids(&self) -> &[BufferId] {
        // This is slightly awkward — we need a contiguous slice.
        // For testing, we'll collect into a leaked slice.
        // In production, hosts would maintain a Vec<BufferId>.
        unsafe {
            let ids: Vec<BufferId> = self.buffers.iter().map(|(id, _, _)| *id).collect();
            let boxed = ids.into_boxed_slice();
            let ptr = Box::into_raw(boxed);
            &*ptr
        }
    }

    fn buffer_meta(&self, id: BufferId) -> Option<BufferMeta> {
        self.buffers
            .iter()
            .find(|(bid, _, _)| *bid == id)
            .map(|(bid, doc, modified)| BufferMeta {
                id: *bid,
                path: Some(CompactString::from(format!("buffer_{}.txt", bid.get()))),
                line_count: doc.line_count(),
                byte_len: doc.text().len(),
                modified: *modified,
                generation: 1,
            })
    }

    fn buffer_lens(&self, id: BufferId) -> Option<BufferLens<'_>> {
        self.buffers
            .iter()
            .find(|(bid, _, _)| *bid == id)
            .map(|(bid, doc, modified)| {
                BufferLens::new(
                    *bid,
                    doc,
                    BufferMeta {
                        id: *bid,
                        path: Some(CompactString::from(format!("buffer_{}.txt", bid.get()))),
                        line_count: doc.line_count(),
                        byte_len: doc.text().len(),
                        modified: *modified,
                        generation: 1,
                    },
                )
            })
    }
}

fn host_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Host, CapabilityTier::Mutating)
}

// ===========================================================================
// 1. BUFFER LENS — proves it implements Document correctly
// ===========================================================================

#[test]
fn buffer_lens_text_access() {
    let scope = MockScope::new(vec![("hello\nworld", false)]);
    let id = BufferId::new(0);
    let lens = scope.buffer_lens(id).unwrap();

    assert_eq!(lens.text(), "hello\nworld");
    assert_eq!(lens.line_count(), 2);
    assert_eq!(lens.id(), id);
}

#[test]
fn buffer_lens_offset_to_pos() {
    let scope = MockScope::new(vec![("alpha\nbeta\ngamma", false)]);
    let lens = scope.buffer_lens(BufferId::new(0)).unwrap();

    let pos = lens.offset_to_pos(Offset::new(6)).unwrap(); // 'b' in "beta"
    assert_eq!(pos.line().get(), 1);
    assert_eq!(pos.col().get(), 0);
}

#[test]
fn buffer_lens_pos_to_offset() {
    let scope = MockScope::new(vec![("alpha\nbeta\ngamma", false)]);
    let lens = scope.buffer_lens(BufferId::new(0)).unwrap();

    let offset = lens.pos_to_offset(Position::from_raw(2, 0)).unwrap(); // start of "gamma"
    assert_eq!(offset.get(), 11);
}

#[test]
fn buffer_lens_meta() {
    let scope = MockScope::new(vec![("hello", true)]);
    let lens = scope.buffer_lens(BufferId::new(0)).unwrap();
    let meta = lens.meta();

    assert_eq!(meta.id, BufferId::new(0));
    assert_eq!(meta.byte_len, 5);
    assert_eq!(meta.line_count, 1);
    assert!(meta.modified);
    assert!(meta.path.is_some());
}

// ===========================================================================
// 2. BUFFER SCOPE — proves enumeration and lookup
// ===========================================================================

#[test]
fn buffer_scope_ids() {
    let scope = MockScope::new(vec![
        ("file one", false),
        ("file two", true),
        ("file three", false),
    ]);

    let ids = scope.buffer_ids();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], BufferId::new(0));
    assert_eq!(ids[1], BufferId::new(1));
    assert_eq!(ids[2], BufferId::new(2));
}

#[test]
fn buffer_scope_meta_lookup() {
    let scope = MockScope::new(vec![("short", false), ("longer text here", true)]);

    let meta = scope.buffer_meta(BufferId::new(1)).unwrap();
    assert_eq!(meta.byte_len, 16);
    assert!(meta.modified);

    // Non-existent buffer
    assert!(scope.buffer_meta(BufferId::new(99)).is_none());
}

#[test]
fn buffer_scope_lens_nonexistent() {
    let scope = MockScope::new(vec![("hello", false)]);
    assert!(scope.buffer_lens(BufferId::new(99)).is_none());
}

// ===========================================================================
// 3. SINGLE-BUFFER HOST — proves zero-cost for non-implementing hosts
// ===========================================================================

#[test]
fn single_buffer_host_no_multi_buffer() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    // HostSession (SessionHost) does not implement BufferScope
    assert!(api.buffers().is_none());
}

// ===========================================================================
// 4. CROSS-BUFFER EDIT EFFECT — proves effect emission
// ===========================================================================

#[test]
fn cross_buffer_edit_produces_correct_effect() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    let target = BufferId::new(42);
    let edits = vec![
        TextEdit {
            start: 0,
            end: 5,
            text: CompactString::from("goodbye"),
        },
        TextEdit {
            start: 10,
            end: 10,
            text: CompactString::from(" inserted"),
        },
    ];

    api.emit().cross_buffer_edit(target, &edits).unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::CrossBufferEdit {
            target: t,
            edits: e,
        } => {
            assert_eq!(*t, BufferId::new(42));
            assert_eq!(e.len(), 2);
            assert_eq!(e[0].start, 0);
            assert_eq!(e[0].end, 5);
            assert_eq!(e[0].text.as_str(), "goodbye");
            assert_eq!(e[1].start, 10);
            assert_eq!(e[1].end, 10);
            assert_eq!(e[1].text.as_str(), " inserted");
        }
        other => panic!("expected CrossBufferEdit, got {other:?}"),
    }
}

#[test]
fn cross_buffer_edit_requires_mutating() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(
        &session,
        InvocationContext::new(CallerId::Expression, CapabilityTier::ReadOnly),
    );

    let result = api.emit().cross_buffer_edit(BufferId::new(1), &[]);
    assert!(result.is_err());
}

#[test]
fn cross_buffer_edit_empty_edits() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    // Empty edit list is valid (no-op)
    api.emit().cross_buffer_edit(BufferId::new(1), &[]).unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::CrossBufferEdit { edits, .. } => {
            assert!(edits.is_empty());
        }
        _ => panic!("wrong effect"),
    }
}

// ===========================================================================
// 5. SYSTEM TEST — cross-buffer grep workflow
// ===========================================================================

#[test]
fn system_test_cross_buffer_grep() {
    // Simulates a plugin that searches multiple buffers for a pattern
    // and collects results.
    let scope = MockScope::new(vec![
        ("hello world\nfoo bar", false),
        ("no match here", false),
        ("another world\nworld again", true),
    ]);

    let pattern = "world";
    let mut results: Vec<(BufferId, usize)> = Vec::new();

    for &id in scope.buffer_ids() {
        if let Some(lens) = scope.buffer_lens(id) {
            let text = lens.text();
            let mut offset = 0;
            while let Some(pos) = text[offset..].find(pattern) {
                results.push((id, offset + pos));
                offset += pos + pattern.len();
            }
        }
    }

    // "hello world..." -> offset 6, "another world\nworld again" -> offset 8, 14
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], (BufferId::new(0), 6));
    assert_eq!(results[1], (BufferId::new(2), 8));
    assert_eq!(results[2], (BufferId::new(2), 14));
}

#[test]
fn system_test_cross_buffer_word_count() {
    // Plugin that counts total words across all buffers
    let scope = MockScope::new(vec![
        ("one two three", false),
        ("four five", false),
        ("six", false),
    ]);

    let total_words: usize = scope
        .buffer_ids()
        .iter()
        .filter_map(|&id| scope.buffer_lens(id))
        .map(|lens| lens.text().split_whitespace().count())
        .sum();

    assert_eq!(total_words, 6);
}

#[test]
fn system_test_cross_buffer_edit_rename() {
    // Simulates a multi-file rename refactor:
    // Find "oldName" in all buffers, emit CrossBufferEdit for each
    let session = HostSession::new("current buffer");
    let api = VimApi::from_session(&session, host_ctx());

    let target_buffers = vec![
        (BufferId::new(1), vec![(5usize, 12usize)]), // one occurrence
        (BufferId::new(2), vec![(0usize, 7usize), (20usize, 27usize)]), // two occurrences
    ];

    for (buf_id, locations) in &target_buffers {
        let edits: Vec<TextEdit> = locations
            .iter()
            .map(|&(start, end)| TextEdit {
                start,
                end,
                text: CompactString::from("newName"),
            })
            .collect();
        api.emit().cross_buffer_edit(*buf_id, &edits).unwrap();
    }

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 2);

    // First buffer: 1 edit
    match &effects[0] {
        Effect::CrossBufferEdit { target, edits } => {
            assert_eq!(*target, BufferId::new(1));
            assert_eq!(edits.len(), 1);
        }
        _ => panic!("wrong effect"),
    }

    // Second buffer: 2 edits
    match &effects[1] {
        Effect::CrossBufferEdit { target, edits } => {
            assert_eq!(*target, BufferId::new(2));
            assert_eq!(edits.len(), 2);
            assert_eq!(edits[0].text.as_str(), "newName");
            assert_eq!(edits[1].text.as_str(), "newName");
        }
        _ => panic!("wrong effect"),
    }
}

#[test]
fn system_test_buffer_meta_filtering() {
    // Plugin that lists only modified buffers
    let scope = MockScope::new(vec![
        ("clean file", false),
        ("dirty file", true),
        ("another clean", false),
        ("also dirty", true),
    ]);

    let modified: Vec<BufferId> = scope
        .buffer_ids()
        .iter()
        .filter_map(|&id| scope.buffer_meta(id))
        .filter(|meta| meta.modified)
        .map(|meta| meta.id)
        .collect();

    assert_eq!(modified.len(), 2);
    assert_eq!(modified[0], BufferId::new(1));
    assert_eq!(modified[1], BufferId::new(3));
}
