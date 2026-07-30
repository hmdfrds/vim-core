//! `ChangeSetRecorder` — builds a `ChangeSet` from sequential post-mutation effects.
//!
//! vim-core effects use **post-mutation coordinates**: each `Insert`/`Delete`/`Replace`
//! describes its position in the document **as it exists after all preceding effects**.
//! A `ChangeSet` uses **original coordinates**: one transformation from the original
//! document to the final document.
//!
//! The recorder bridges this gap by maintaining a running composition:
//! each effect creates a mini-changeset against the current document length,
//! which is then composed onto the accumulated total.
//!
//! # Example
//!
//! ```text
//! Original doc: "abcdef" (6 bytes)
//! Effect 1: Insert "X" at offset 2  → doc becomes "abXcdef" (7 bytes)
//! Effect 2: Delete [4..6]           → doc becomes "abXcf" (5 bytes)
//!
//! recorder.record_insert(2, "X")   → mini: =2, +"X", =4  (6→7)
//! recorder.record_delete(4, 6)     → mini: =4, -2, =1    (7→5)
//! recorder.finish()                → compose(mini1, mini2) = original→final
//! ```

use super::change_set::ChangeSet;
use super::error::ChangeSetError;

/// Incrementally builds a `ChangeSet` from sequential post-mutation effects.
///
/// Each `record_*` call creates a mini-changeset and composes it onto
/// the accumulated result.
#[derive(Debug, Clone)]
pub struct ChangeSetRecorder {
    /// The composed changeset so far (original → current).
    accumulated: ChangeSet,
    /// Current document length after all recorded effects.
    current_len: usize,
}

impl ChangeSetRecorder {
    /// Start recording from a document of `initial_len` bytes.
    #[must_use]
    pub fn new(initial_len: usize) -> Self {
        Self {
            accumulated: ChangeSet::identity(initial_len),
            current_len: initial_len,
        }
    }

    /// Record an insertion at `offset` in the **current** document.
    ///
    /// # Errors
    ///
    /// Returns `ComposeMismatch` if internal composition fails (should not
    /// happen with correct usage).
    pub fn record_insert(&mut self, offset: usize, text: &str) -> Result<(), ChangeSetError> {
        if text.is_empty() {
            return Ok(());
        }
        let mini = ChangeSet::from_insert(self.current_len, offset, text);
        self.accumulated = self.accumulated.compose(&mini)?;
        self.current_len += text.len();
        Ok(())
    }

    /// Record a deletion of `start..end` in the **current** document.
    ///
    /// # Errors
    ///
    /// Returns `ComposeMismatch` if internal composition fails.
    pub fn record_delete(&mut self, start: usize, end: usize) -> Result<(), ChangeSetError> {
        if start >= end {
            return Ok(());
        }
        let mini = ChangeSet::from_delete(self.current_len, start, end);
        let del_len = end.saturating_sub(start);
        self.accumulated = self.accumulated.compose(&mini)?;
        self.current_len = self.current_len.saturating_sub(del_len);
        Ok(())
    }

    /// Record a replacement of `start..end` with `text` in the **current** document.
    ///
    /// # Errors
    ///
    /// Returns `ComposeMismatch` if internal composition fails.
    pub fn record_replace(
        &mut self,
        start: usize,
        end: usize,
        text: &str,
    ) -> Result<(), ChangeSetError> {
        let del_len = end.saturating_sub(start);
        if del_len == 0 && text.is_empty() {
            return Ok(());
        }
        let mini = ChangeSet::from_replace(self.current_len, start, end, text);
        self.accumulated = self.accumulated.compose(&mini)?;
        self.current_len = self.current_len.saturating_sub(del_len) + text.len();
        Ok(())
    }

    /// Consume the recorder, returning the accumulated `ChangeSet`
    /// from the **original** document to the **final** document.
    #[must_use]
    pub fn finish(self) -> ChangeSet {
        self.accumulated
    }

    /// Current document length after all recorded effects.
    #[inline]
    #[must_use]
    pub const fn current_len(&self) -> usize {
        self.current_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_empty() {
        let rec = ChangeSetRecorder::new(10);
        let cs = rec.finish();
        assert!(cs.is_identity());
        assert_eq!(cs.input_len(), 10);
        assert_eq!(cs.output_len(), 10);
    }

    #[test]
    fn recorder_single_insert() {
        let mut rec = ChangeSetRecorder::new(5);
        rec.record_insert(2, "XX").unwrap();
        let cs = rec.finish();
        assert_eq!(cs.input_len(), 5);
        assert_eq!(cs.output_len(), 7);
        assert_eq!(cs.apply("hello").unwrap(), "heXXllo");
    }

    #[test]
    fn recorder_single_delete() {
        let mut rec = ChangeSetRecorder::new(5);
        rec.record_delete(1, 4).unwrap();
        let cs = rec.finish();
        assert_eq!(cs.input_len(), 5);
        assert_eq!(cs.output_len(), 2);
        assert_eq!(cs.apply("hello").unwrap(), "ho");
    }

    #[test]
    fn recorder_single_replace() {
        let mut rec = ChangeSetRecorder::new(5);
        rec.record_replace(1, 4, "XY").unwrap();
        let cs = rec.finish();
        assert_eq!(cs.input_len(), 5);
        assert_eq!(cs.output_len(), 4);
        assert_eq!(cs.apply("hello").unwrap(), "hXYo");
    }

    #[test]
    fn recorder_sequential_effects() {
        // "abcdef" (6 bytes)
        // 1. Insert "X" at 2 -> "abXcdef" (7)
        // 2. Delete [4..6] -> "abXcf" (5)  (in post-mutation coords: deletes "de")
        let mut rec = ChangeSetRecorder::new(6);
        rec.record_insert(2, "X").unwrap();
        assert_eq!(rec.current_len(), 7);
        rec.record_delete(4, 6).unwrap();
        assert_eq!(rec.current_len(), 5);

        let cs = rec.finish();
        assert_eq!(cs.input_len(), 6);
        assert_eq!(cs.output_len(), 5);
        assert_eq!(cs.apply("abcdef").unwrap(), "abXcf");
    }

    #[test]
    fn recorder_insert_then_delete_same_text() {
        // Insert "XY" then delete it — should be identity
        let mut rec = ChangeSetRecorder::new(5);
        rec.record_insert(2, "XY").unwrap();
        rec.record_delete(2, 4).unwrap();
        let cs = rec.finish();
        assert!(cs.is_identity(), "should be identity: {cs}");
    }

    #[test]
    fn recorder_multiple_inserts() {
        // "abc" (3 bytes)
        // Insert "X" at 0 -> "Xabc" (4)
        // Insert "Y" at 4 -> "XabcY" (5)
        // Insert "Z" at 2 -> "XaZbcY" (6)
        let mut rec = ChangeSetRecorder::new(3);
        rec.record_insert(0, "X").unwrap();
        rec.record_insert(4, "Y").unwrap();
        rec.record_insert(2, "Z").unwrap();
        let cs = rec.finish();
        assert_eq!(cs.apply("abc").unwrap(), "XaZbcY");
    }

    #[test]
    fn recorder_replace_then_insert() {
        // "hello" (5)
        // Replace [1..3] with "X" -> "hXlo" (4)
        // Insert "!" at 4 -> "hXlo!" (5)
        let mut rec = ChangeSetRecorder::new(5);
        rec.record_replace(1, 3, "X").unwrap();
        rec.record_insert(4, "!").unwrap();
        let cs = rec.finish();
        assert_eq!(cs.apply("hello").unwrap(), "hXlo!");
    }

    #[test]
    fn recorder_current_len_tracking() {
        let mut rec = ChangeSetRecorder::new(10);
        assert_eq!(rec.current_len(), 10);
        rec.record_insert(5, "abc").unwrap();
        assert_eq!(rec.current_len(), 13);
        rec.record_delete(0, 3).unwrap();
        assert_eq!(rec.current_len(), 10);
        rec.record_replace(2, 6, "X").unwrap();
        assert_eq!(rec.current_len(), 7);
    }

    #[test]
    fn recorder_empty_effects_are_noops() {
        let mut rec = ChangeSetRecorder::new(5);
        rec.record_insert(2, "").unwrap();
        rec.record_delete(3, 3).unwrap();
        rec.record_replace(1, 1, "").unwrap();
        let cs = rec.finish();
        assert!(cs.is_identity());
    }

    #[test]
    fn recorder_round_trip_with_invert() {
        // Record some effects, then verify invert round-trip
        let original = "hello world";
        let mut rec = ChangeSetRecorder::new(original.len());
        rec.record_delete(5, 6).unwrap(); // "helloworld" (10)
        rec.record_insert(5, "_").unwrap(); // "hello_world" (11)
        let cs = rec.finish();

        let transformed = cs.apply(original).unwrap();
        assert_eq!(transformed, "hello_world");

        let inv = cs.invert(original).unwrap();
        let restored = inv.apply(&transformed).unwrap();
        assert_eq!(restored, original);
    }
}
