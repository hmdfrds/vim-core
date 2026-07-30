//! Canonical `impl Document` for unit tests.
//!
//! `SimpleDocument` wraps a `String` and implements all required Document
//! methods. Position translation uses `commands::helpers` for line/column
//! resolution and manual grapheme walking for `pos_to_offset` (no helper
//! exists for the reverse direction).
//!
//! # Why here and not in `document/`?
//!
//! `document/` is a BOTTOM layer that may only import `primitives`.
//! This impl needs `commands::helpers` (a higher layer), so it lives
//! in `test_utils/` at the crate root instead.

use crate::commands::helpers;
use crate::document::Document;
use crate::primitives::{Offset, Position};

/// Canonical Document implementation for unit tests.
///
/// # Example
///
/// ```ignore
/// use vim_core::test_utils::SimpleDocument;
///
/// let doc = SimpleDocument::new("hello\nworld");
/// assert_eq!(doc.line_count(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct SimpleDocument {
    text: String,
}

impl SimpleDocument {
    /// Create from a string.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl Document for SimpleDocument {
    fn text(&self) -> &str {
        &self.text
    }

    /// Returns the number of lines in the document.
    ///
    /// Matches Vim semantics: an empty buffer has 1 line (the empty line).
    /// This is consistent with `ExContext::new` which also uses `.max(1)`.
    fn line_count(&self) -> usize {
        helpers::line_count(&self.text).max(1)
    }

    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        let off = offset.get();
        if off > self.text.len() {
            return None;
        }
        let line = helpers::line_of(&self.text, off);
        let col = helpers::column_of(&self.text, off);
        Some(Position::from_raw(line, col))
    }

    /// Convert Position to byte offset.
    ///
    /// Uses `helpers::line_start`/`line_end` for line resolution, then
    /// walks graphemes manually to find the column's byte offset.
    /// No helper exists for this reverse direction.
    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        let line_start = helpers::line_start(&self.text, pos.line().get())?;
        let line_end = helpers::line_end(&self.text, pos.line().get())?;
        let line_len = line_end - line_start;

        // Column is byte offset within line (matches Neovim)
        let col = pos.col().get().min(line_len);
        Some(Offset::new(line_start + col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── SimpleDocument basics ─────────────────────────────────────

    #[test]
    fn empty_doc() {
        let doc = SimpleDocument::new("");
        assert!(doc.is_empty());
        assert_eq!(doc.len(), 0);
        // Vim semantics: empty buffer still has 1 line
        assert_eq!(doc.line_count(), 1);
    }

    #[test]
    fn single_line() {
        let doc = SimpleDocument::new("hello");
        assert!(!doc.is_empty());
        assert_eq!(doc.len(), 5);
        assert_eq!(doc.line_count(), 1);
    }

    #[test]
    fn multi_line() {
        let doc = SimpleDocument::new("hello\nworld\nfoo");
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.len(), 15);
    }

    // ─── offset_to_pos ─────────────────────────────────────────────

    #[test]
    fn offset_to_pos_first_char() {
        let doc = SimpleDocument::new("hello\nworld");
        assert_eq!(
            doc.offset_to_pos(Offset::new(0)),
            Some(Position::from_raw(0, 0))
        );
    }

    #[test]
    fn offset_to_pos_mid_line() {
        let doc = SimpleDocument::new("hello\nworld");
        assert_eq!(
            doc.offset_to_pos(Offset::new(3)),
            Some(Position::from_raw(0, 3))
        );
    }

    #[test]
    fn offset_to_pos_second_line() {
        let doc = SimpleDocument::new("hello\nworld");
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
        let doc = SimpleDocument::new("hello");
        assert_eq!(doc.offset_to_pos(Offset::new(100)), None);
    }

    #[test]
    fn offset_to_pos_multi_byte() {
        // 'é' is 2 bytes, '世' is 3 bytes
        let doc = SimpleDocument::new("héllo\n世界");
        // offset 0 = 'h' → (0, byte col 0)
        assert_eq!(
            doc.offset_to_pos(Offset::new(0)),
            Some(Position::from_raw(0, 0))
        );
        // offset 1 = 'é' (2 bytes) → byte col 1
        assert_eq!(
            doc.offset_to_pos(Offset::new(1)),
            Some(Position::from_raw(0, 1))
        );
        // offset 3 = 'l' → byte col 3
        assert_eq!(
            doc.offset_to_pos(Offset::new(3)),
            Some(Position::from_raw(0, 3))
        );
        // offset 7 = '世' (first char of line 1) → (1, byte col 0)
        assert_eq!(
            doc.offset_to_pos(Offset::new(7)),
            Some(Position::from_raw(1, 0))
        );
        // offset 10 = '界' → byte col 3
        assert_eq!(
            doc.offset_to_pos(Offset::new(10)),
            Some(Position::from_raw(1, 3))
        );
    }

    // ─── pos_to_offset ─────────────────────────────────────────────

    #[test]
    fn pos_to_offset_origin() {
        let doc = SimpleDocument::new("hello\nworld");
        assert_eq!(
            doc.pos_to_offset(Position::from_raw(0, 0)),
            Some(Offset::new(0))
        );
    }

    #[test]
    fn pos_to_offset_mid_line() {
        let doc = SimpleDocument::new("hello\nworld");
        assert_eq!(
            doc.pos_to_offset(Position::from_raw(0, 3)),
            Some(Offset::new(3))
        );
    }

    #[test]
    fn pos_to_offset_second_line() {
        let doc = SimpleDocument::new("hello\nworld");
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
    fn pos_to_offset_invalid_line() {
        let doc = SimpleDocument::new("hello");
        assert_eq!(doc.pos_to_offset(Position::from_raw(5, 0)), None);
    }

    #[test]
    fn pos_to_offset_multi_byte() {
        let doc = SimpleDocument::new("héllo\n世界");
        // byte col 1 = 'é' → byte offset 1
        assert_eq!(
            doc.pos_to_offset(Position::from_raw(0, 1)),
            Some(Offset::new(1))
        );
        // byte col 3 = 'l' → byte offset 3 (after 'h' + 'é'(2 bytes))
        assert_eq!(
            doc.pos_to_offset(Position::from_raw(0, 3)),
            Some(Offset::new(3))
        );
        // line 1, byte col 3 = '界' → byte offset 10 (7 + 3 bytes for '世')
        assert_eq!(
            doc.pos_to_offset(Position::from_raw(1, 3)),
            Some(Offset::new(10))
        );
    }

    // ─── Roundtrip (offset → pos → offset) ─────────────────────────

    #[test]
    fn roundtrip_ascii() {
        let doc = SimpleDocument::new("hello\nworld\nfoo");
        for raw_offset in 0..doc.len() {
            let offset = Offset::new(raw_offset);
            let pos = doc.offset_to_pos(offset).unwrap();
            let back = doc.pos_to_offset(pos).unwrap();
            assert_eq!(
                offset, back,
                "roundtrip failed at offset {raw_offset} (pos {pos:?})"
            );
        }
    }

    #[test]
    fn roundtrip_multi_byte() {
        let doc = SimpleDocument::new("héllo\n世界\nfoo");
        for raw_offset in 0..doc.len() {
            if !doc.text().is_char_boundary(raw_offset) {
                continue; // skip mid-codepoint offsets
            }
            let offset = Offset::new(raw_offset);
            let pos = doc.offset_to_pos(offset).unwrap();
            let back = doc.pos_to_offset(pos).unwrap();
            assert_eq!(
                offset, back,
                "roundtrip failed at offset {raw_offset} (pos {pos:?})"
            );
        }
    }

    // ─── Default trait methods ──────────────────────────────────────

    #[test]
    fn len_and_is_empty_defaults() {
        let empty = SimpleDocument::new("");
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());

        let non_empty = SimpleDocument::new("x");
        assert_eq!(non_empty.len(), 1);
        assert!(!non_empty.is_empty());
    }
}
