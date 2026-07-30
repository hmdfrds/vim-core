//! # SavePoint — restorable document snapshot
//!
//! A `SavePoint` captures cursor position and selection at a point in time,
//! then maintains a **rolling inverse** changeset that can revert the document
//! back to that state regardless of how many edits happen afterwards.
//!
//! # How It Works
//!
//! 1. **Create** with `SavePoint::new(doc_len, cursor, selection)` — captures
//!    the current state and starts with an identity (no-op) revert.
//!
//! 2. **Update** after each document edit with `update(changeset, text_before)`.
//!    Internally: `revert = inverse(changeset).compose(revert)`, building a
//!    chain from current state → original state.
//!
//! 3. **Restore** with `restore(current_text)` — applies the accumulated revert,
//!    returns the restored text + cursor + selection, and resets the savepoint
//!    for reuse.
//!
//! # Scope
//!
//! This primitive stores the savepoint itself, in byte offsets and this
//! crate's `ChangeSet` type. It deliberately does not manage lifetimes: an
//! `Arc`/`Weak` scheme for RAII cleanup belongs in the execution layer, not
//! here.

use super::changeset::{ChangeSet, ChangeSetError};
use super::{Offset, SelectionRange};

/// Result of restoring a `SavePoint`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavePointRestore {
    /// The restored document text.
    pub text: String,
    /// The cursor position at the time the savepoint was created.
    pub cursor: Offset,
    /// The selection state at the time the savepoint was created.
    pub selection: Option<SelectionRange>,
}

/// A restorable snapshot of document and cursor state.
///
/// Maintains a rolling inverse changeset that accumulates the reversal
/// of all edits since creation. Applying this changeset to the current
/// document text produces the text that existed when the savepoint was
/// created.
///
/// # Example
///
/// ```ignore
/// let mut sp = SavePoint::new(doc.len(), cursor, None);
///
/// // Edit happens: insert "XX" at offset 2
/// let cs = ChangeSet::from_insert(doc.len(), 2, "XX");
/// sp.update(&cs, doc.text())?;  // pass text BEFORE the edit
/// apply_edit(&mut doc, &cs);
///
/// // Another edit: delete bytes 0..3
/// let cs2 = ChangeSet::from_delete(doc.len(), 0, 3);
/// sp.update(&cs2, doc.text())?;
/// apply_edit(&mut doc, &cs2);
///
/// // Restore: reverts both edits
/// let restored = sp.restore(doc.text())?;
/// assert_eq!(restored.text, original_text);
/// ```
#[derive(Debug, Clone)]
pub struct SavePoint {
    /// Cursor position at creation time.
    cursor: Offset,
    /// Selection state at creation time.
    selection: Option<SelectionRange>,
    /// Rolling inverse: current_doc → original_doc.
    revert: ChangeSet,
}

impl SavePoint {
    /// Create a new savepoint at the current document state.
    ///
    /// `doc_len` is the byte length of the current document.
    /// The revert changeset starts as identity (no changes to undo).
    #[must_use]
    pub fn new(doc_len: usize, cursor: Offset, selection: Option<SelectionRange>) -> Self {
        Self {
            cursor,
            selection,
            revert: ChangeSet::identity(doc_len),
        }
    }

    /// Update the rolling inverse after a document edit.
    ///
    /// `changeset` is the edit that was (or will be) applied.
    /// `text_before_edit` is the document text **before** the edit.
    ///
    /// Internally computes `inverse(changeset, text_before_edit)` and
    /// prepends it to the revert chain:
    /// `revert = inverse.compose(revert)`
    ///
    /// # Errors
    ///
    /// - `LengthMismatch` if `text_before_edit.len() != changeset.input_len()`
    /// - `ComposeMismatch` if the inverse's output doesn't match the revert's input
    pub fn update(
        &mut self,
        changeset: &ChangeSet,
        text_before_edit: &str,
    ) -> Result<(), ChangeSetError> {
        let inverse = changeset.invert(text_before_edit)?;
        self.revert = inverse.compose(&self.revert)?;
        Ok(())
    }

    /// Restore the document to the saved state.
    ///
    /// Applies the accumulated revert changeset to `current_text`,
    /// producing the document text from when the savepoint was created.
    /// Returns the restored text along with the saved cursor and selection.
    ///
    /// After this call, the savepoint is **reset**: the revert becomes
    /// identity for the restored document length. The savepoint can be
    /// reused for another save/restore cycle.
    ///
    /// # Errors
    ///
    /// - `LengthMismatch` if `current_text.len() != revert.input_len()`
    pub fn restore(&mut self, current_text: &str) -> Result<SavePointRestore, ChangeSetError> {
        let restored_text = self.revert.apply(current_text)?;
        let result = SavePointRestore {
            text: restored_text,
            cursor: self.cursor,
            selection: self.selection,
        };
        // Reset to identity for the restored state
        self.revert = ChangeSet::identity(result.text.len());
        Ok(result)
    }

    /// Get the saved cursor position.
    #[inline]
    #[must_use]
    pub const fn cursor(&self) -> Offset {
        self.cursor
    }

    /// Get the saved selection state.
    #[inline]
    #[must_use]
    pub const fn selection(&self) -> Option<SelectionRange> {
        self.selection
    }

    /// Get a reference to the accumulated revert changeset.
    ///
    /// This changeset maps current document → original document.
    #[inline]
    #[must_use]
    pub const fn revert_changeset(&self) -> &ChangeSet {
        &self.revert
    }

    /// True if any edits have been tracked since creation (or last restore).
    #[inline]
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.revert.has_changes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Identity (no changes) ────────────────────────────────────────

    #[test]
    fn new_savepoint_has_no_changes() {
        let sp = SavePoint::new(5, Offset::new(2), None);
        assert!(!sp.has_changes());
        assert_eq!(sp.cursor(), Offset::new(2));
        assert_eq!(sp.selection(), None);
    }

    #[test]
    fn restore_without_changes_returns_same_text() {
        let mut sp = SavePoint::new(5, Offset::new(3), None);
        let restored = sp.restore("hello").unwrap();
        assert_eq!(restored.text, "hello");
        assert_eq!(restored.cursor, Offset::new(3));
        assert_eq!(restored.selection, None);
    }

    // ── Single edit ──────────────────────────────────────────────────

    #[test]
    fn restore_after_insert() {
        let original = "hello";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);

        // Insert "XX" at offset 2: "hello" → "heXXllo"
        let cs = ChangeSet::from_insert(5, 2, "XX");
        sp.update(&cs, original).unwrap();

        let current = cs.apply(original).unwrap();
        assert_eq!(current, "heXXllo");

        let restored = sp.restore(&current).unwrap();
        assert_eq!(restored.text, original);
    }

    #[test]
    fn restore_after_delete() {
        let original = "hello world";
        let mut sp = SavePoint::new(original.len(), Offset::new(5), None);

        // Delete bytes 5..6 (the space): "hello world" → "helloworld"
        let cs = ChangeSet::from_delete(11, 5, 6);
        sp.update(&cs, original).unwrap();

        let current = cs.apply(original).unwrap();
        assert_eq!(current, "helloworld");

        let restored = sp.restore(&current).unwrap();
        assert_eq!(restored.text, original);
        assert_eq!(restored.cursor, Offset::new(5));
    }

    #[test]
    fn restore_after_replace() {
        let original = "hello";
        let mut sp = SavePoint::new(original.len(), Offset::new(1), None);

        // Replace bytes 1..4 with "XY": "hello" → "hXYo"
        let cs = ChangeSet::from_replace(5, 1, 4, "XY");
        sp.update(&cs, original).unwrap();

        let current = cs.apply(original).unwrap();
        assert_eq!(current, "hXYo");

        let restored = sp.restore(&current).unwrap();
        assert_eq!(restored.text, original);
    }

    // ── Multiple edits ───────────────────────────────────────────────

    #[test]
    fn restore_after_two_inserts() {
        let original = "abc";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);

        // Edit 1: insert "X" at 1: "abc" → "aXbc"
        let cs1 = ChangeSet::from_insert(3, 1, "X");
        sp.update(&cs1, original).unwrap();
        let after1 = cs1.apply(original).unwrap();
        assert_eq!(after1, "aXbc");

        // Edit 2: insert "Y" at 3: "aXbc" → "aXbYc"
        let cs2 = ChangeSet::from_insert(4, 3, "Y");
        sp.update(&cs2, &after1).unwrap();
        let after2 = cs2.apply(&after1).unwrap();
        assert_eq!(after2, "aXbYc");

        let restored = sp.restore(&after2).unwrap();
        assert_eq!(restored.text, original);
    }

    #[test]
    fn restore_after_insert_then_delete() {
        let original = "hello";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);

        // Edit 1: insert "XX" at 2: "hello" → "heXXllo"
        let cs1 = ChangeSet::from_insert(5, 2, "XX");
        sp.update(&cs1, original).unwrap();
        let after1 = cs1.apply(original).unwrap();

        // Edit 2: delete bytes 0..2: "heXXllo" → "XXllo"
        let cs2 = ChangeSet::from_delete(7, 0, 2);
        sp.update(&cs2, &after1).unwrap();
        let after2 = cs2.apply(&after1).unwrap();
        assert_eq!(after2, "XXllo");

        let restored = sp.restore(&after2).unwrap();
        assert_eq!(restored.text, original);
    }

    #[test]
    fn restore_after_three_edits() {
        let original = "abcdefghij";
        let mut sp = SavePoint::new(original.len(), Offset::new(5), None);

        // Edit 1: replace "cde" with "X": "abcdefghij" → "abXfghij"
        let cs1 = ChangeSet::from_replace(10, 2, 5, "X");
        sp.update(&cs1, original).unwrap();
        let after1 = cs1.apply(original).unwrap();
        assert_eq!(after1, "abXfghij");

        // Edit 2: insert "YY" at 4: "abXfghij" → "abXfYYghij"
        let cs2 = ChangeSet::from_insert(8, 4, "YY");
        sp.update(&cs2, &after1).unwrap();
        let after2 = cs2.apply(&after1).unwrap();
        assert_eq!(after2, "abXfYYghij");

        // Edit 3: delete bytes 7..10: "abXfYYghij" → "abXfYYg"
        let cs3 = ChangeSet::from_delete(10, 7, 10);
        sp.update(&cs3, &after2).unwrap();
        let after3 = cs3.apply(&after2).unwrap();
        assert_eq!(after3, "abXfYYg");

        let restored = sp.restore(&after3).unwrap();
        assert_eq!(restored.text, original);
        assert_eq!(restored.cursor, Offset::new(5));
    }

    // ── Selection preservation ───────────────────────────────────────

    #[test]
    fn restore_preserves_selection() {
        let original = "hello world";
        let selection = Some(SelectionRange::new(Offset::new(6), Offset::new(11)));
        let mut sp = SavePoint::new(original.len(), Offset::new(6), selection);

        let cs = ChangeSet::from_insert(11, 0, ">> ");
        sp.update(&cs, original).unwrap();
        let current = cs.apply(original).unwrap();

        let restored = sp.restore(&current).unwrap();
        assert_eq!(restored.text, original);
        assert_eq!(restored.cursor, Offset::new(6));
        assert_eq!(restored.selection, selection);
    }

    // ── Restore + reuse ──────────────────────────────────────────────

    #[test]
    fn savepoint_reusable_after_restore() {
        let original = "hello";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);

        // First cycle: edit → restore
        let cs1 = ChangeSet::from_insert(5, 2, "XX");
        sp.update(&cs1, original).unwrap();
        let after1 = cs1.apply(original).unwrap();
        let restored1 = sp.restore(&after1).unwrap();
        assert_eq!(restored1.text, original);

        // After restore, savepoint is reset — no changes
        assert!(!sp.has_changes());

        // Second cycle: different edit from the restored state
        let cs2 = ChangeSet::from_delete(5, 0, 2);
        sp.update(&cs2, original).unwrap();
        let after2 = cs2.apply(original).unwrap();
        assert_eq!(after2, "llo");

        let restored2 = sp.restore(&after2).unwrap();
        assert_eq!(restored2.text, original);
    }

    // ── has_changes ──────────────────────────────────────────────────

    #[test]
    fn has_changes_after_edit() {
        let original = "hello";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);
        assert!(!sp.has_changes());

        let cs = ChangeSet::from_insert(5, 2, "X");
        sp.update(&cs, original).unwrap();
        assert!(sp.has_changes());
    }

    #[test]
    fn has_changes_false_after_restore() {
        let original = "hello";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);

        let cs = ChangeSet::from_insert(5, 2, "X");
        sp.update(&cs, original).unwrap();
        let current = cs.apply(original).unwrap();

        sp.restore(&current).unwrap();
        assert!(!sp.has_changes());
    }

    // ── Error cases ──────────────────────────────────────────────────

    #[test]
    fn update_with_wrong_text_length_fails() {
        let mut sp = SavePoint::new(5, Offset::new(0), None);
        let cs = ChangeSet::from_insert(5, 2, "X");
        // Pass text with wrong length
        let err = sp.update(&cs, "abc").unwrap_err();
        assert!(matches!(err, ChangeSetError::LengthMismatch { .. }));
    }

    #[test]
    fn restore_with_wrong_text_length_fails() {
        let mut sp = SavePoint::new(5, Offset::new(0), None);
        let err = sp.restore("abc").unwrap_err();
        assert!(matches!(err, ChangeSetError::LengthMismatch { .. }));
    }

    // ── UTF-8 ────────────────────────────────────────────────────────

    #[test]
    fn restore_with_multibyte_text() {
        let original = "héllo 世界";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);

        // Delete 'é' (bytes 1..3): "héllo 世界" → "hllo 世界"
        let cs = ChangeSet::from_delete(original.len(), 1, 3);
        sp.update(&cs, original).unwrap();
        let current = cs.apply(original).unwrap();
        assert_eq!(current, "hllo 世界");

        let restored = sp.restore(&current).unwrap();
        assert_eq!(restored.text, original);
    }

    #[test]
    fn restore_after_replacing_multibyte() {
        let original = "世界";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);

        // Replace '世' (bytes 0..3) with "AB"
        let cs = ChangeSet::from_replace(6, 0, 3, "AB");
        sp.update(&cs, original).unwrap();
        let current = cs.apply(original).unwrap();
        assert_eq!(current, "AB界");

        let restored = sp.restore(&current).unwrap();
        assert_eq!(restored.text, original);
    }

    // ── Empty document ───────────────────────────────────────────────

    #[test]
    fn savepoint_on_empty_document() {
        let original = "";
        let mut sp = SavePoint::new(0, Offset::new(0), None);

        let cs = ChangeSet::from_insert(0, 0, "hello");
        sp.update(&cs, original).unwrap();
        let current = cs.apply(original).unwrap();
        assert_eq!(current, "hello");

        let restored = sp.restore(&current).unwrap();
        assert_eq!(restored.text, "");
    }

    #[test]
    fn restore_to_empty_document() {
        let original = "hello";
        let mut sp = SavePoint::new(original.len(), Offset::new(0), None);

        let cs = ChangeSet::from_delete(5, 0, 5);
        sp.update(&cs, original).unwrap();
        let current = cs.apply(original).unwrap();
        assert_eq!(current, "");

        let restored = sp.restore(&current).unwrap();
        assert_eq!(restored.text, original);
    }

    // ── Accessors ────────────────────────────────────────────────────

    #[test]
    fn accessors() {
        let sel = Some(SelectionRange::new(Offset::new(1), Offset::new(4)));
        let sp = SavePoint::new(10, Offset::new(3), sel);
        assert_eq!(sp.cursor(), Offset::new(3));
        assert_eq!(sp.selection(), sel);
        assert!(!sp.has_changes());
        assert!(sp.revert_changeset().is_identity());
    }
}
