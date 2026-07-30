//! Tests for `LineIndex` and `OwnedDocument`.
//!
//! Included via `#[path = "shadow_document_tests.rs"] mod tests;`
//! in `shadow_document.rs`.

use super::*;
use crate::primitives::LineNumber;

// ─── LineIndex construction ─────────────────────────────────────

#[test]
fn line_index_empty_string() {
    let idx = LineIndex::new("");
    assert_eq!(idx.line_starts, vec![0]);
    assert_eq!(idx.line_count(), 1);
}

#[test]
fn line_index_single_line() {
    let idx = LineIndex::new("hello");
    assert_eq!(idx.line_starts, vec![0]);
    assert_eq!(idx.line_count(), 1);
}

#[test]
fn line_index_two_lines() {
    let idx = LineIndex::new("hello\nworld");
    assert_eq!(idx.line_starts, vec![0, 6]);
    assert_eq!(idx.line_count(), 2);
}

#[test]
fn line_index_three_lines() {
    let idx = LineIndex::new("hello\nworld\nfoo");
    assert_eq!(idx.line_starts, vec![0, 6, 12]);
    assert_eq!(idx.line_count(), 3);
}

#[test]
fn line_index_consecutive_newlines() {
    let idx = LineIndex::new("\n\n");
    assert_eq!(idx.line_starts, vec![0, 1, 2]);
    assert_eq!(idx.line_count(), 3);
}

// ─── LineIndex::line_of ─────────────────────────────────────────

#[test]
fn line_of_first_char() {
    let idx = LineIndex::new("hello\nworld\nfoo");
    assert_eq!(idx.line_of(0), 0);
}

#[test]
fn line_of_mid_first_line() {
    let idx = LineIndex::new("hello\nworld\nfoo");
    assert_eq!(idx.line_of(3), 0);
}

#[test]
fn line_of_at_newline() {
    // Offset 5 is the '\n' itself — belongs to line 0.
    let idx = LineIndex::new("hello\nworld\nfoo");
    assert_eq!(idx.line_of(5), 0);
}

#[test]
fn line_of_second_line_start() {
    let idx = LineIndex::new("hello\nworld\nfoo");
    assert_eq!(idx.line_of(6), 1);
}

#[test]
fn line_of_third_line() {
    let idx = LineIndex::new("hello\nworld\nfoo");
    assert_eq!(idx.line_of(12), 2);
    assert_eq!(idx.line_of(14), 2);
}

#[test]
fn line_of_at_text_end() {
    // Offset == text.len() should be on the last line.
    let idx = LineIndex::new("hello\nworld");
    assert_eq!(idx.line_of(11), 1);
}

// ─── LineIndex::line_start ──────────────────────────────────────

#[test]
fn line_start_valid() {
    let idx = LineIndex::new("hello\nworld\nfoo");
    assert_eq!(idx.line_start(0), Some(0));
    assert_eq!(idx.line_start(1), Some(6));
    assert_eq!(idx.line_start(2), Some(12));
}

#[test]
fn line_start_out_of_bounds() {
    let idx = LineIndex::new("hello");
    assert_eq!(idx.line_start(1), None);
}

// ─── LineIndex::update_insert ───────────────────────────────────

#[test]
fn insert_text_without_newlines() {
    let mut idx = LineIndex::new("hello\nworld");
    // Insert "XY" at offset 2 ("heXYllo\nworld")
    idx.update_insert(2, "XY");
    assert_eq!(idx.line_starts, vec![0, 8]);
    assert_eq!(idx.line_count(), 2);
}

#[test]
fn insert_text_with_newline_at_beginning() {
    let mut idx = LineIndex::new("hello\nworld");
    // Insert "A\n" at offset 0 → "A\nhello\nworld"
    idx.update_insert(0, "A\n");
    assert_eq!(idx.line_starts, vec![0, 2, 8]);
    assert_eq!(idx.line_count(), 3);
}

#[test]
fn insert_text_with_newline_in_middle() {
    let mut idx = LineIndex::new("hello\nworld");
    // Insert "\n" at offset 3 → "hel\nlo\nworld"
    idx.update_insert(3, "\n");
    assert_eq!(idx.line_starts, vec![0, 4, 7]);
    assert_eq!(idx.line_count(), 3);
}

#[test]
fn insert_text_with_newline_at_end() {
    let mut idx = LineIndex::new("hello");
    // Insert "\nworld" at offset 5 → "hello\nworld"
    idx.update_insert(5, "\nworld");
    assert_eq!(idx.line_starts, vec![0, 6]);
    assert_eq!(idx.line_count(), 2);
}

#[test]
fn insert_multiple_newlines() {
    let mut idx = LineIndex::new("ab");
    // Insert "X\nY\nZ" at offset 1 → "aX\nY\nZb"
    idx.update_insert(1, "X\nY\nZ");
    assert_eq!(idx.line_starts, vec![0, 3, 5]);
    assert_eq!(idx.line_count(), 3);
}

// ─── LineIndex::update_delete ───────────────────────────────────

#[test]
fn delete_within_single_line() {
    let mut idx = LineIndex::new("hello\nworld");
    // Delete "ll" (offset 2..4) → "heo\nworld"
    idx.update_delete(2, 4);
    assert_eq!(idx.line_starts, vec![0, 4]);
    assert_eq!(idx.line_count(), 2);
}

#[test]
fn delete_spanning_newline() {
    let mut idx = LineIndex::new("hello\nworld");
    // Delete offset 3..7 ("lo\nw") → "helorld"
    idx.update_delete(3, 7);
    assert_eq!(idx.line_starts, vec![0]);
    assert_eq!(idx.line_count(), 1);
}

#[test]
fn delete_multiple_lines() {
    let mut idx = LineIndex::new("aaa\nbbb\nccc\nddd");
    // Delete offset 4..12 ("bbb\nccc\n") → "aaa\nddd"
    idx.update_delete(4, 12);
    assert_eq!(idx.line_starts, vec![0, 4]);
    assert_eq!(idx.line_count(), 2);
}

#[test]
fn delete_from_beginning() {
    let mut idx = LineIndex::new("hello\nworld");
    // Delete offset 0..6 ("hello\n") → "world"
    idx.update_delete(0, 6);
    assert_eq!(idx.line_starts, vec![0]);
    assert_eq!(idx.line_count(), 1);
}

// ─── LineIndex replace (delete + insert) ────────────────────────

#[test]
fn replace_changing_newline_count() {
    let mut idx = LineIndex::new("hello\nworld");
    // Replace "lo\nwor" (offset 3..9) with "L\nM\nN"
    // Result: "helL\nM\nNld"
    idx.update_delete(3, 9);
    idx.update_insert(3, "L\nM\nN");
    assert_eq!(idx.line_count(), 3);
    assert_eq!(idx.line_starts, vec![0, 5, 7]);
}

// ─── OwnedDocument basics (mirror SimpleDocument) ───────────────

#[test]
fn empty_doc() {
    let doc = OwnedDocument::new("");
    assert!(doc.is_empty());
    assert_eq!(doc.len(), 0);
    assert_eq!(doc.line_count(), 1);
}

#[test]
fn single_line() {
    let doc = OwnedDocument::new("hello");
    assert!(!doc.is_empty());
    assert_eq!(doc.len(), 5);
    assert_eq!(doc.line_count(), 1);
}

#[test]
fn multi_line() {
    let doc = OwnedDocument::new("hello\nworld\nfoo");
    assert_eq!(doc.line_count(), 3);
    assert_eq!(doc.len(), 15);
}

// ─── offset_to_pos ─────────────────────────────────────────────

#[test]
fn offset_to_pos_first_char() {
    let doc = OwnedDocument::new("hello\nworld");
    assert_eq!(
        doc.offset_to_pos(Offset::new(0)),
        Some(Position::from_raw(0, 0))
    );
}

#[test]
fn offset_to_pos_mid_line() {
    let doc = OwnedDocument::new("hello\nworld");
    assert_eq!(
        doc.offset_to_pos(Offset::new(3)),
        Some(Position::from_raw(0, 3))
    );
}

#[test]
fn offset_to_pos_second_line() {
    let doc = OwnedDocument::new("hello\nworld");
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
    let doc = OwnedDocument::new("hello");
    assert_eq!(doc.offset_to_pos(Offset::new(100)), None);
}

#[test]
fn offset_to_pos_multi_byte() {
    // 'e' with acute = 2 bytes, '世' = 3 bytes
    let doc = OwnedDocument::new("h\u{00e9}llo\n\u{4e16}\u{754c}");
    assert_eq!(
        doc.offset_to_pos(Offset::new(0)),
        Some(Position::from_raw(0, 0))
    );
    // offset 1 = 'e' with acute (2 bytes) → grapheme col 1
    assert_eq!(
        doc.offset_to_pos(Offset::new(1)),
        Some(Position::from_raw(0, 1))
    );
    // offset 3 = 'l' → byte col 3
    assert_eq!(
        doc.offset_to_pos(Offset::new(3)),
        Some(Position::from_raw(0, 3))
    );
    // offset 7 = '世' (first char of line 1) → (1, 0)
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

#[test]
fn offset_to_pos_at_text_len() {
    // Cursor-after-last-char: offset == text.len() is valid.
    let doc = OwnedDocument::new("hello\nworld");
    assert_eq!(
        doc.offset_to_pos(Offset::new(11)),
        Some(Position::from_raw(1, 5))
    );
}

// ─── pos_to_offset ─────────────────────────────────────────────

#[test]
fn pos_to_offset_origin() {
    let doc = OwnedDocument::new("hello\nworld");
    assert_eq!(
        doc.pos_to_offset(Position::from_raw(0, 0)),
        Some(Offset::new(0))
    );
}

#[test]
fn pos_to_offset_mid_line() {
    let doc = OwnedDocument::new("hello\nworld");
    assert_eq!(
        doc.pos_to_offset(Position::from_raw(0, 3)),
        Some(Offset::new(3))
    );
}

#[test]
fn pos_to_offset_second_line() {
    let doc = OwnedDocument::new("hello\nworld");
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
    let doc = OwnedDocument::new("hello");
    assert_eq!(doc.pos_to_offset(Position::from_raw(5, 0)), None);
}

#[test]
fn pos_to_offset_multi_byte() {
    let doc = OwnedDocument::new("h\u{00e9}llo\n\u{4e16}\u{754c}");
    // grapheme col 1 = 'e with acute' → byte offset 1
    assert_eq!(
        doc.pos_to_offset(Position::from_raw(0, 1)),
        Some(Offset::new(1))
    );
    // byte col 3 = 'l' → byte offset 3
    assert_eq!(
        doc.pos_to_offset(Position::from_raw(0, 3)),
        Some(Offset::new(3))
    );
    // line 1, byte col 3 = '界' → byte offset 10
    assert_eq!(
        doc.pos_to_offset(Position::from_raw(1, 3)),
        Some(Offset::new(10))
    );
}

// ─── Roundtrip (offset → pos → offset) ─────────────────────────

#[test]
fn roundtrip_ascii() {
    let doc = OwnedDocument::new("hello\nworld\nfoo");
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
    let doc = OwnedDocument::new("h\u{00e9}llo\n\u{4e16}\u{754c}\nfoo");
    for raw_offset in 0..doc.len() {
        if !doc.text().is_char_boundary(raw_offset) {
            continue;
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
    let empty = OwnedDocument::new("");
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());

    let non_empty = OwnedDocument::new("x");
    assert_eq!(non_empty.len(), 1);
    assert!(!non_empty.is_empty());
}

// ─── Mutation: apply_insert ─────────────────────────────────────

#[test]
fn apply_insert_at_beginning() {
    let mut doc = OwnedDocument::new("world");
    doc.apply_insert(0, "hello ");
    assert_eq!(doc.text(), "hello world");
    assert_eq!(doc.line_count(), 1);
    assert_eq!(
        doc.offset_to_pos(Offset::new(6)),
        Some(Position::from_raw(0, 6))
    );
}

#[test]
fn apply_insert_at_middle() {
    let mut doc = OwnedDocument::new("hllo");
    doc.apply_insert(1, "e");
    assert_eq!(doc.text(), "hello");
    assert_eq!(doc.line_count(), 1);
}

#[test]
fn apply_insert_at_end() {
    let mut doc = OwnedDocument::new("hello");
    doc.apply_insert(5, "\nworld");
    assert_eq!(doc.text(), "hello\nworld");
    assert_eq!(doc.line_count(), 2);
    assert_eq!(
        doc.offset_to_pos(Offset::new(6)),
        Some(Position::from_raw(1, 0))
    );
}

#[test]
fn apply_insert_with_multiple_newlines() {
    let mut doc = OwnedDocument::new("ab");
    doc.apply_insert(1, "\n\n");
    assert_eq!(doc.text(), "a\n\nb");
    assert_eq!(doc.line_count(), 3);
    // Offset 2 = second '\n', which is the start of line 1.
    assert_eq!(
        doc.offset_to_pos(Offset::new(2)),
        Some(Position::from_raw(1, 0))
    );
    // Offset 3 = 'b', which is the start of line 2.
    assert_eq!(
        doc.offset_to_pos(Offset::new(3)),
        Some(Position::from_raw(2, 0))
    );
}

#[test]
fn apply_insert_on_empty_doc() {
    let mut doc = OwnedDocument::new("");
    doc.apply_insert(0, "hello\nworld");
    assert_eq!(doc.text(), "hello\nworld");
    assert_eq!(doc.line_count(), 2);
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

// ─── Mutation: apply_delete ─────────────────────────────────────

#[test]
fn apply_delete_at_beginning() {
    let mut doc = OwnedDocument::new("hello world");
    doc.apply_delete(0, 6);
    assert_eq!(doc.text(), "world");
    assert_eq!(doc.line_count(), 1);
}

#[test]
fn apply_delete_at_middle() {
    let mut doc = OwnedDocument::new("hello\nworld");
    doc.apply_delete(3, 7);
    assert_eq!(doc.text(), "helorld");
    assert_eq!(doc.line_count(), 1);
}

#[test]
fn apply_delete_at_end() {
    let mut doc = OwnedDocument::new("hello\nworld");
    doc.apply_delete(5, 11);
    assert_eq!(doc.text(), "hello");
    assert_eq!(doc.line_count(), 1);
}

// ─── Mutation: apply_replace ────────────────────────────────────

#[test]
fn apply_replace_same_length() {
    let mut doc = OwnedDocument::new("hello");
    doc.apply_replace(1, 4, "ELL");
    assert_eq!(doc.text(), "hELLo");
    assert_eq!(doc.line_count(), 1);
}

#[test]
fn apply_replace_adding_newlines() {
    let mut doc = OwnedDocument::new("hello world");
    doc.apply_replace(5, 6, "\n");
    assert_eq!(doc.text(), "hello\nworld");
    assert_eq!(doc.line_count(), 2);
}

#[test]
fn apply_replace_removing_newlines() {
    let mut doc = OwnedDocument::new("hello\nworld");
    doc.apply_replace(5, 6, " ");
    assert_eq!(doc.text(), "hello world");
    assert_eq!(doc.line_count(), 1);
}

// ─── Mutation followed by roundtrip ─────────────────────────────

#[test]
fn roundtrip_after_insert() {
    let mut doc = OwnedDocument::new("hello\nworld");
    doc.apply_insert(5, "\nMIDDLE");
    // Text is now "hello\nMIDDLE\nworld"
    assert_eq!(doc.line_count(), 3);
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
fn roundtrip_after_delete() {
    let mut doc = OwnedDocument::new("aaa\nbbb\nccc");
    doc.apply_delete(4, 8);
    // Text is now "aaa\nccc"
    assert_eq!(doc.line_count(), 2);
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
fn roundtrip_after_replace() {
    let mut doc = OwnedDocument::new("hello world");
    doc.apply_replace(5, 6, "\n");
    // Text is now "hello\nworld"
    assert_eq!(doc.line_count(), 2);
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

// ─── Mutation with multi-byte text ──────────────────────────────

#[test]
fn insert_cjk_characters() {
    let mut doc = OwnedDocument::new("hello");
    doc.apply_insert(5, "\n\u{4e16}\u{754c}");
    // Text: "hello\n世界"
    assert_eq!(doc.line_count(), 2);
    assert_eq!(
        doc.offset_to_pos(Offset::new(6)),
        Some(Position::from_raw(1, 0))
    );
    // offset 9 = '界' → byte col 3 on line 1
    assert_eq!(
        doc.offset_to_pos(Offset::new(9)),
        Some(Position::from_raw(1, 3))
    );
}

#[test]
fn delete_cjk_characters() {
    let mut doc = OwnedDocument::new("\u{4e16}\u{754c}\nhello");
    // Delete '世' (3 bytes at offset 0..3)
    doc.apply_delete(0, 3);
    // Text: "界\nhello"
    assert_eq!(doc.text(), "\u{754c}\nhello");
    assert_eq!(doc.line_count(), 2);
    assert_eq!(
        doc.offset_to_pos(Offset::new(0)),
        Some(Position::from_raw(0, 0))
    );
}

#[test]
fn roundtrip_after_cjk_insert() {
    let mut doc = OwnedDocument::new("a\nb");
    doc.apply_insert(1, "\u{4e16}\u{754c}");
    // Text: "a世界\nb"
    for raw_offset in 0..doc.len() {
        if !doc.text().is_char_boundary(raw_offset) {
            continue;
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

// ─── OwnedDocument convenience methods ──────────────────────────

#[test]
fn owned_doc_line_returns_content_without_newline() {
    let doc = OwnedDocument::new("hello\nworld\n");
    assert_eq!(doc.line(LineNumber::new(0)), Some("hello"));
    assert_eq!(doc.line(LineNumber::new(1)), Some("world"));
    assert_eq!(doc.line(LineNumber::new(2)), Some(""));
    assert_eq!(doc.line(LineNumber::new(3)), None);
}

#[test]
fn owned_doc_line_of_offset() {
    let doc = OwnedDocument::new("abc\ndef\nghi");
    assert_eq!(doc.line_of_offset(0), 0);
    assert_eq!(doc.line_of_offset(3), 0); // the '\n'
    assert_eq!(doc.line_of_offset(4), 1); // 'd'
    assert_eq!(doc.line_of_offset(8), 2); // 'g'
}

#[test]
fn owned_doc_line_start_offset() {
    let doc = OwnedDocument::new("abc\ndef\nghi");
    assert_eq!(
        doc.line_start_offset(LineNumber::new(0)),
        Some(Offset::new(0))
    );
    assert_eq!(
        doc.line_start_offset(LineNumber::new(1)),
        Some(Offset::new(4))
    );
    assert_eq!(
        doc.line_start_offset(LineNumber::new(2)),
        Some(Offset::new(8))
    );
    assert_eq!(doc.line_start_offset(LineNumber::new(3)), None);
}

#[test]
fn owned_doc_set_text_rebuilds_index() {
    let mut doc = OwnedDocument::new("one");
    assert_eq!(doc.line_count(), 1);
    doc.set_text("one\ntwo\nthree");
    assert_eq!(doc.line_count(), 3);
    assert_eq!(doc.line(LineNumber::new(2)), Some("three"));
}
