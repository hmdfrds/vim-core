//! VimText-backed mutable `Document` for `VimSession<SessionHost>`.
//!
//! `VimTextDocument` wraps a `vim_text::VimText` persistent B+ tree and
//! implements the `Document` trait. Mutations operate on the tree in O(log n)
//! via Arc-based COW, compared to `OwnedDocument`'s O(n) `String` memmove.
//!
//! # `text() -> &str` — Incremental String
//!
//! The `Document` trait requires `text(&self) -> &str`. Rather than lazily
//! materializing on first access (old `OnceCell` approach), we maintain an
//! always-valid `String` that is spliced in-place on every mutation:
//!
//! - `text()` is a trivial `&self.text` return — O(1), zero branching
//! - Mutations splice BOTH the tree (O(log n)) AND the String (O(n) memmove)
//! - The String and tree are kept in lockstep; debug builds assert equality
//!
//! This eliminates the first-access latency spike that the OnceCell approach
//! had, making performance completely predictable per-keystroke.
//!
//! # When This Wins
//!
//! VimText's persistent COW gives O(1) snapshots for undo (vs O(n) clone).
//! Line queries (`line_count`, `line_of_offset`) are O(log n) via tree
//! summaries, never requiring full traversal.
//!
//! # Feature Gate
//!
//! This module is only compiled when `feature = "vim-text"` is enabled.

use vim_text::VimText;

use crate::document::Document;
use crate::primitives::{LineNumber, Offset, Position};

/// VimText-backed mutable document.
///
/// Uses a persistent B+ tree for O(log n) mutations and O(1) COW snapshots.
/// An always-valid `String` is spliced in-place on every mutation, making
/// `text() -> &str` a trivial field access with zero branching.
#[derive(Debug, Clone)]
pub struct VimTextDocument {
    tree: VimText,
    text: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Construction
// ═══════════════════════════════════════════════════════════════════════════════

impl VimTextDocument {
    /// Create a new document from the given text.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let tree = VimText::from_str(&text);
        Self { tree, text }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Mutations
// ═══════════════════════════════════════════════════════════════════════════════

impl VimTextDocument {
    /// Insert `text` at byte `offset`.
    ///
    /// Offsets are clamped to `[0, len]` and snapped left to the nearest char
    /// boundary — matching `VimText::apply_insert` semantics.
    pub fn apply_insert(&mut self, offset: usize, ins: &str) {
        let offset = self.snap_left(offset);
        self.tree.apply_insert(offset, ins);
        self.text.insert_str(offset, ins);
        debug_assert_eq!(self.text, self.tree.materialize());
    }

    /// Delete the byte range `[start, end)`.
    ///
    /// Offsets are clamped and snapped to char boundaries.
    pub fn apply_delete(&mut self, start: usize, end: usize) {
        let (start, end) = self.snap_range(start, end);
        if start >= end {
            return;
        }
        self.tree.apply_delete(start, end);
        self.text.replace_range(start..end, "");
        debug_assert_eq!(self.text, self.tree.materialize());
    }

    /// Replace the byte range `[start, end)` with `text`.
    ///
    /// Offsets are clamped and snapped to char boundaries.
    pub fn apply_replace(&mut self, start: usize, end: usize, replacement: &str) {
        let (start, end) = self.snap_range(start, end);
        if start >= end && replacement.is_empty() {
            return;
        }
        self.tree.apply_replace(start, end, replacement);
        self.text.replace_range(start..end, replacement);
        debug_assert_eq!(self.text, self.tree.materialize());
    }

    /// Replace the entire document content.
    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.tree.set_text(&text);
        self.text = text;
        debug_assert_eq!(self.text, self.tree.materialize());
    }

    // ── Offset snapping helpers ──────────────────────────────────────

    /// Clamp `offset` to `[0, len]` and snap left to nearest char boundary.
    #[inline]
    fn snap_left(&self, offset: usize) -> usize {
        let offset = offset.min(self.text.len());
        let mut pos = offset;
        while pos > 0 && !self.text.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    /// Clamp and snap a `[start, end)` range to char boundaries.
    #[inline]
    fn snap_range(&self, start: usize, end: usize) -> (usize, usize) {
        let len = self.text.len();
        let start = self.snap_left(start.min(len));
        let end = self.snap_left(end.min(len));
        (start, end)
    }

    /// Get the content of line `n` (excluding trailing newline).
    #[must_use]
    pub fn line(&self, n: LineNumber) -> Option<&str> {
        let idx = n.get();
        if idx >= self.tree.line_count() {
            return None;
        }
        let start = self.tree.line_start(idx)?;
        let text = self.text();
        let rest = &text[start..];
        let line_len = rest.find('\n').unwrap_or(rest.len());
        Some(&rest[..line_len])
    }

    /// Get the 0-indexed line number containing `offset`.
    #[must_use]
    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.tree.line_of_offset(offset)
    }

    /// Get the byte offset of the start of line `n`.
    #[must_use]
    pub fn line_start_offset(&self, n: LineNumber) -> Option<Offset> {
        let idx = n.get();
        if idx >= self.tree.line_count() {
            return None;
        }
        self.tree.line_start(idx).map(Offset::new)
    }

    /// Access the underlying `VimText` B+ tree.
    #[must_use]
    pub const fn tree(&self) -> &VimText {
        &self.tree
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Document trait implementation
// ═══════════════════════════════════════════════════════════════════════════════

impl Document for VimTextDocument {
    #[inline]
    fn text(&self) -> &str {
        &self.text
    }

    #[inline]
    fn len(&self) -> usize {
        self.tree.byte_len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    fn line_count(&self) -> usize {
        let raw = self.tree.line_count();
        if self.text.ends_with('\n') && raw > 1 {
            raw - 1
        } else {
            raw.max(1)
        }
    }

    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        let off = offset.get();
        if off > self.tree.byte_len() {
            return None;
        }
        let line = self.tree.line_of_offset(off);
        let line_start = self.tree.line_start(line).unwrap_or(off);
        Some(Position::from_raw(line, off - line_start))
    }

    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        let line = pos.line().get();
        let lc = self.line_count();

        if line > lc {
            return None;
        }

        if line == lc {
            if self.tree.byte_len() > 0
                && self
                    .tree
                    .summary()
                    .flags
                    .contains(vim_text::summary::SummaryFlags::ENDS_WITH_NEWLINE)
            {
                return Some(Offset::new(self.tree.byte_len()));
            }
            return None;
        }

        let start = self.tree.line_start(line)?;
        let line_text = self.tree.line(line)?;
        let col = pos.col().get().min(line_text.len());
        Some(Offset::new(start + col))
    }

    fn slice(&self, start: usize, end: usize) -> std::borrow::Cow<'_, str> {
        self.tree
            .slice_range(start, end)
            .unwrap_or(std::borrow::Cow::Borrowed(""))
    }

    #[inline]
    fn line_of_offset(&self, offset: usize) -> usize {
        self.tree.line_of_offset(offset)
    }

    fn vim_text_tree(&self) -> Option<&vim_text::VimText> {
        Some(&self.tree)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::engine::shadow_document::OwnedDocument;

    // ── Construction ─────────────────────────────────────────────────

    #[test]
    fn new_from_string() {
        let doc = VimTextDocument::new("hello world");
        assert_eq!(doc.text(), "hello world");
        assert_eq!(doc.len(), 11);
    }

    #[test]
    fn new_empty() {
        let doc = VimTextDocument::new("");
        assert_eq!(doc.text(), "");
        assert_eq!(doc.len(), 0);
        assert!(doc.is_empty());
    }

    #[test]
    fn new_multiline() {
        let doc = VimTextDocument::new("line1\nline2\nline3\n");
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.len(), 18);
    }

    // ── Insert ───────────────────────────────────────────────────────

    #[test]
    fn insert_at_start() {
        let mut doc = VimTextDocument::new("hello");
        doc.apply_insert(0, "XX");
        assert_eq!(doc.text(), "XXhello");
    }

    #[test]
    fn insert_at_end() {
        let mut doc = VimTextDocument::new("hello");
        doc.apply_insert(5, " world");
        assert_eq!(doc.text(), "hello world");
    }

    #[test]
    fn insert_in_middle() {
        let mut doc = VimTextDocument::new("hello");
        doc.apply_insert(2, "XX");
        assert_eq!(doc.text(), "heXXllo");
    }

    #[test]
    fn insert_clamped() {
        let mut doc = VimTextDocument::new("hi");
        doc.apply_insert(999, "!");
        assert_eq!(doc.text(), "hi!");
    }

    #[test]
    fn insert_newline() {
        let mut doc = VimTextDocument::new("line1line2");
        doc.apply_insert(5, "\n");
        assert_eq!(doc.text(), "line1\nline2");
        assert_eq!(doc.line_count(), 2);
    }

    // ── Delete ───────────────────────────────────────────────────────

    #[test]
    fn delete_from_start() {
        let mut doc = VimTextDocument::new("hello");
        doc.apply_delete(0, 2);
        assert_eq!(doc.text(), "llo");
    }

    #[test]
    fn delete_from_end() {
        let mut doc = VimTextDocument::new("hello");
        doc.apply_delete(3, 5);
        assert_eq!(doc.text(), "hel");
    }

    #[test]
    fn delete_from_middle() {
        let mut doc = VimTextDocument::new("hello");
        doc.apply_delete(1, 3);
        assert_eq!(doc.text(), "hlo");
    }

    #[test]
    fn delete_clamped() {
        let mut doc = VimTextDocument::new("hi");
        doc.apply_delete(1, 999);
        assert_eq!(doc.text(), "h");
    }

    #[test]
    fn delete_noop() {
        let mut doc = VimTextDocument::new("hi");
        doc.apply_delete(1, 1);
        assert_eq!(doc.text(), "hi");
    }

    #[test]
    fn delete_newline() {
        let mut doc = VimTextDocument::new("line1\nline2");
        doc.apply_delete(5, 6);
        assert_eq!(doc.text(), "line1line2");
        assert_eq!(doc.line_count(), 1);
    }

    // ── Replace ──────────────────────────────────────────────────────

    #[test]
    fn replace_middle() {
        let mut doc = VimTextDocument::new("hello world");
        doc.apply_replace(5, 11, " earth");
        assert_eq!(doc.text(), "hello earth");
    }

    #[test]
    fn replace_with_shorter() {
        let mut doc = VimTextDocument::new("hello world");
        doc.apply_replace(5, 11, "!");
        assert_eq!(doc.text(), "hello!");
    }

    #[test]
    fn replace_with_longer() {
        let mut doc = VimTextDocument::new("hi");
        doc.apply_replace(0, 2, "hello world");
        assert_eq!(doc.text(), "hello world");
    }

    #[test]
    fn replace_clamped() {
        let mut doc = VimTextDocument::new("hi");
        doc.apply_replace(1, 999, "ello");
        assert_eq!(doc.text(), "hello");
    }

    // ── set_text ─────────────────────────────────────────────────────

    #[test]
    fn set_text_replaces_all() {
        let mut doc = VimTextDocument::new("hello");
        doc.set_text("world");
        assert_eq!(doc.text(), "world");
        assert_eq!(doc.len(), 5);
    }

    // ── Document trait ───────────────────────────────────────────────

    #[test]
    fn len_without_materialization() {
        let mut doc = VimTextDocument::new("hello");
        doc.apply_insert(5, " world");
        assert_eq!(doc.len(), 11);
        assert!(!doc.is_empty());
    }

    #[test]
    fn line_count_vim_semantics() {
        let doc = VimTextDocument::new("");
        assert_eq!(doc.line_count(), 1);
    }

    #[test]
    fn offset_to_pos_basic() {
        let doc = VimTextDocument::new("hello\nworld");
        assert_eq!(
            doc.offset_to_pos(Offset::new(0)),
            Some(Position::from_raw(0, 0))
        );
        assert_eq!(
            doc.offset_to_pos(Offset::new(6)),
            Some(Position::from_raw(1, 0))
        );
        assert_eq!(
            doc.offset_to_pos(Offset::new(8)),
            Some(Position::from_raw(1, 2))
        );
    }

    #[test]
    fn offset_to_pos_out_of_bounds() {
        let doc = VimTextDocument::new("hi");
        assert!(doc.offset_to_pos(Offset::new(999)).is_none());
    }

    #[test]
    fn pos_to_offset_basic() {
        let doc = VimTextDocument::new("hello\nworld");
        assert_eq!(
            doc.pos_to_offset(Position::from_raw(0, 0)),
            Some(Offset::new(0))
        );
        assert_eq!(
            doc.pos_to_offset(Position::from_raw(1, 0)),
            Some(Offset::new(6))
        );
        assert_eq!(
            doc.pos_to_offset(Position::from_raw(1, 2)),
            Some(Offset::new(8))
        );
    }

    #[test]
    fn pos_to_offset_out_of_bounds() {
        let doc = VimTextDocument::new("hi");
        assert!(doc.pos_to_offset(Position::from_raw(5, 0)).is_none());
    }

    // ── Line queries ─────────────────────────────────────────────────

    #[test]
    fn line_content() {
        let doc = VimTextDocument::new("hello\nworld\n");
        assert_eq!(doc.line(LineNumber::new(0)), Some("hello"));
        assert_eq!(doc.line(LineNumber::new(1)), Some("world"));
    }

    #[test]
    fn line_of_offset() {
        let doc = VimTextDocument::new("hello\nworld\n");
        assert_eq!(doc.line_of_offset(0), 0);
        assert_eq!(doc.line_of_offset(5), 0);
        assert_eq!(doc.line_of_offset(6), 1);
    }

    #[test]
    fn line_start_offset() {
        let doc = VimTextDocument::new("hello\nworld\n");
        assert_eq!(
            doc.line_start_offset(LineNumber::new(0)),
            Some(Offset::new(0))
        );
        assert_eq!(
            doc.line_start_offset(LineNumber::new(1)),
            Some(Offset::new(6))
        );
    }

    // ── UTF-8 ────────────────────────────────────────────────────────

    #[test]
    fn utf8_insert() {
        let mut doc = VimTextDocument::new("héllo");
        doc.apply_insert(0, "🌍 ");
        assert_eq!(doc.text(), "🌍 héllo");
    }

    #[test]
    fn utf8_delete_multibyte() {
        let mut doc = VimTextDocument::new("héllo");
        doc.apply_delete(1, 3);
        assert_eq!(doc.text(), "hllo");
    }

    #[test]
    fn utf8_replace_multibyte() {
        let mut doc = VimTextDocument::new("héllo");
        doc.apply_replace(1, 3, "a");
        assert_eq!(doc.text(), "hallo");
    }

    #[test]
    fn utf8_cjk() {
        let mut doc = VimTextDocument::new("世界");
        doc.apply_insert(3, "X");
        assert_eq!(doc.text(), "世X界");
    }

    // ── Incremental sync ───────────────────────────────────────────────

    #[test]
    fn text_correct_after_insert() {
        let doc = VimTextDocument::new("hello");
        let _ = doc.text();
        let mut doc = doc;
        doc.apply_insert(5, "!");
        assert_eq!(doc.text(), "hello!");
    }

    #[test]
    fn text_correct_after_delete() {
        let doc = VimTextDocument::new("hello");
        let _ = doc.text();
        let mut doc = doc;
        doc.apply_delete(4, 5);
        assert_eq!(doc.text(), "hell");
    }

    #[test]
    fn multiple_mutations_correct() {
        let mut doc = VimTextDocument::new("hello world");
        doc.apply_delete(5, 11);
        doc.apply_insert(5, " earth");
        doc.apply_insert(0, "Say ");
        assert_eq!(doc.text(), "Say hello earth");
    }

    // ── Tree access ──────────────────────────────────────────────────

    #[test]
    fn tree_access() {
        let doc = VimTextDocument::new("hello\nworld");
        assert_eq!(doc.tree().byte_len(), 11);
        assert_eq!(doc.tree().char_count(), 11);
    }

    // ── Cross-verification with OwnedDocument ────────────────────────

    fn verify_cross(operations: &[(&str, Op)]) {
        let mut vt_doc = VimTextDocument::new(operations[0].0);
        let mut owned_doc = OwnedDocument::new(operations[0].0);

        for (_, op) in &operations[1..] {
            match op {
                Op::Insert(offset, text) => {
                    vt_doc.apply_insert(*offset, text);
                    owned_doc.apply_insert(*offset, text);
                }
                Op::Delete(start, end) => {
                    vt_doc.apply_delete(*start, *end);
                    owned_doc.apply_delete(*start, *end);
                }
                Op::Replace(start, end, text) => {
                    vt_doc.apply_replace(*start, *end, text);
                    owned_doc.apply_replace(*start, *end, text);
                }
            }
            assert_eq!(
                vt_doc.text(),
                owned_doc.text(),
                "text mismatch after {op:?}"
            );
            assert_eq!(vt_doc.len(), owned_doc.len(), "len mismatch after {op:?}");
            assert_eq!(
                vt_doc.line_count(),
                owned_doc.line_count(),
                "line_count mismatch after {op:?}"
            );
        }
    }

    #[derive(Debug)]
    enum Op {
        Insert(usize, &'static str),
        Delete(usize, usize),
        Replace(usize, usize, &'static str),
    }

    #[test]
    fn cross_verify_insert_sequence() {
        verify_cross(&[
            ("hello", Op::Insert(0, "")),
            ("", Op::Insert(5, " world")),
            ("", Op::Insert(0, "Say ")),
            ("", Op::Insert(9, " beautiful")),
        ]);
    }

    #[test]
    fn cross_verify_delete_sequence() {
        verify_cross(&[
            ("hello beautiful world", Op::Delete(0, 0)),
            ("", Op::Delete(5, 15)),
            ("", Op::Delete(5, 11)),
        ]);
    }

    #[test]
    fn cross_verify_mixed_operations() {
        verify_cross(&[
            ("hello world", Op::Insert(0, "")),
            ("", Op::Replace(5, 11, " earth")),
            ("", Op::Insert(0, "Say ")),
            ("", Op::Delete(9, 15)),
            ("", Op::Insert(9, " mars")),
        ]);
    }

    #[test]
    fn cross_verify_multiline() {
        verify_cross(&[
            ("line1\nline2\nline3\n", Op::Insert(0, "")),
            ("", Op::Insert(6, "new\n")),
            ("", Op::Delete(6, 10)),
            ("", Op::Replace(6, 11, "CHANGED")),
        ]);
    }

    #[test]
    fn cross_verify_utf8() {
        verify_cross(&[
            ("héllo 世界", Op::Insert(0, "")),
            ("", Op::Insert(0, "🌍 ")),
            ("", Op::Delete(4, 5)),
            ("", Op::Replace(4, 10, "earth")),
        ]);
    }

    // ── Position mapping cross-verification ──────────────────────────

    #[test]
    fn offset_to_pos_matches_owned() {
        for text in [
            "hello\nworld\nfoo bar\n",
            "hello\nworld\nfoo bar",
            "",
            "\n",
            "\n\n\n",
            "single",
        ] {
            let vt_doc = VimTextDocument::new(text);
            let owned_doc = OwnedDocument::new(text);

            for offset in 0..=text.len() {
                assert_eq!(
                    vt_doc.offset_to_pos(Offset::new(offset)),
                    owned_doc.offset_to_pos(Offset::new(offset)),
                    "offset_to_pos mismatch at offset {offset} for {text:?}"
                );
            }
        }
    }

    #[test]
    fn pos_to_offset_matches_owned() {
        for text in [
            "hello\nworld\nfoo bar\n",
            "hello\nworld\nfoo bar",
            "",
            "\n",
            "\n\n\n",
            "single",
        ] {
            let vt_doc = VimTextDocument::new(text);
            let owned_doc = OwnedDocument::new(text);

            for line in 0..6 {
                for col in 0..12 {
                    assert_eq!(
                        vt_doc.pos_to_offset(Position::from_raw(line, col)),
                        owned_doc.pos_to_offset(Position::from_raw(line, col)),
                        "pos_to_offset mismatch at ({line}, {col}) for {text:?}"
                    );
                }
            }
        }
    }

    // ── Document trait method cross-verification ────────────────────

    #[test]
    fn line_of_offset_matches_owned() {
        for text in [
            "hello\nworld\nfoo bar\n",
            "hello\nworld\nfoo bar",
            "",
            "\n",
            "\n\n\n",
            "single",
        ] {
            let vt_doc = VimTextDocument::new(text);
            let owned_doc = OwnedDocument::new(text);

            for offset in 0..=text.len() + 1 {
                assert_eq!(
                    Document::line_of_offset(&vt_doc, offset),
                    Document::line_of_offset(&owned_doc, offset),
                    "line_of_offset mismatch at offset {offset} for {text:?}"
                );
            }
        }
    }

    #[test]
    fn slice_matches_owned() {
        for text in [
            "hello\nworld\nfoo bar\n",
            "hello\nworld\nfoo bar",
            "\n",
            "\n\n\n",
            "single",
        ] {
            let vt_doc = VimTextDocument::new(text);
            let owned_doc = OwnedDocument::new(text);

            for start in 0..text.len() {
                for end in start..=text.len() {
                    if text.is_char_boundary(start) && text.is_char_boundary(end) {
                        assert_eq!(
                            vt_doc.slice(start, end),
                            owned_doc.slice(start, end),
                            "slice mismatch at [{start}..{end}] for {text:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn len_equals_text_len() {
        for text in [
            "hello\nworld\nfoo bar\n",
            "hello\nworld\nfoo bar",
            "",
            "\n",
            "\n\n\n",
            "single",
        ] {
            let doc = VimTextDocument::new(text);
            assert_eq!(
                doc.len(),
                doc.text().len(),
                "len() != text().len() for {text:?}"
            );
        }
    }

    #[test]
    fn vim_text_tree_returns_tree() {
        let doc = VimTextDocument::new("hello\nworld");
        assert_eq!(
            doc.vim_text_tree()
                .expect("VimTextDocument always has a tree")
                .byte_len(),
            11
        );
    }

    #[test]
    fn owned_doc_has_no_vim_text_tree() {
        // Carried over from evolve's vim-core, where OwnedDocument is itself
        // VimText-backed and hands out a tree. In this tree OwnedDocument is
        // still String-backed, so it takes the trait default and correctly
        // reports None -- that is the contract the bloom pre-filter paths rely
        // on to decide whether a tree-accelerated scan is available at all.
        let doc = OwnedDocument::new("hello\nworld");
        assert!(doc.vim_text_tree().is_none());
    }
}
