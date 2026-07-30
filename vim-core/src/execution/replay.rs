//! Effect replay and time-travel debugging for keystroke sessions.
//!
//! Provides [`SessionRecorder`] and [`SessionReplayer`] for recording
//! per-keystroke effect streams and replaying them with forward/backward
//! traversal. This enables time-travel debugging: a host can record an
//! entire editing session, then step through it entry by entry — forwards
//! or backwards — to inspect exactly what effects each keystroke produced.
//!
//! # Usage
//!
//! ```ignore
//! use vim_core::execution::{SessionRecorder, SessionReplayer, ReplayEntry};
//!
//! // Recording
//! engine.start_recording_session();
//! // ... process keystrokes ...
//! let recorder = engine.stop_recording_session().unwrap();
//!
//! // Replay with time-travel
//! let mut replayer = SessionReplayer::new(recorder.into_entries());
//! while let Some(entry) = replayer.next() {
//!     println!("{:?} -> {} effects", entry.key, entry.effects.len());
//! }
//! // Go backward
//! if let Some(entry) = replayer.prev() {
//!     println!("Stepped back to {:?}", entry.key);
//! }
//! ```
//!
//! # Architecture
//!
//! This module lives in the `execution` layer (top layer) and imports from
//! `effects` and `keymap`. It has no forbidden imports.

use crate::effects::Effect;
use crate::keymap::KeyEvent;

// ═══════════════════════════════════════════════════════════════════════════════
// ReplayEntry
// ═══════════════════════════════════════════════════════════════════════════════

/// A single recorded keystroke and its resulting effects.
///
/// Each entry captures the key that was pressed and the complete list of
/// effects the engine produced for that keystroke. This is the atomic unit
/// of session replay.
#[derive(Debug, Clone)]
pub struct ReplayEntry {
    /// The key event that was processed.
    pub key: KeyEvent,
    /// The effects produced by processing this key.
    pub effects: Vec<Effect>,
}

impl ReplayEntry {
    /// Create a new replay entry.
    #[inline]
    #[must_use]
    pub const fn new(key: KeyEvent, effects: Vec<Effect>) -> Self {
        Self { key, effects }
    }

    /// The key event that was processed.
    #[inline]
    #[must_use]
    pub const fn key(&self) -> KeyEvent {
        self.key
    }

    /// The effects produced by processing this key.
    #[inline]
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SessionRecorder
// ═══════════════════════════════════════════════════════════════════════════════

/// Records keystroke-to-effect mappings for an editing session.
///
/// The recorder accumulates [`ReplayEntry`] values — one per keystroke —
/// while the engine processes input. When recording is stopped, the
/// accumulated entries can be consumed to create a [`SessionReplayer`]
/// for time-travel debugging.
///
/// Recording is lightweight: it clones the effect slice for each keystroke
/// but does no other bookkeeping. For long sessions the memory usage grows
/// linearly with the number of keystrokes and total effects produced.
#[derive(Debug, Clone)]
pub struct SessionRecorder {
    /// Recorded entries in chronological order.
    entries: Vec<ReplayEntry>,
}

impl SessionRecorder {
    /// Create a new, empty session recorder.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Record a keystroke and its resulting effects.
    ///
    /// Called automatically by [`VimEngine::process()`](crate::execution::VimEngine::process) when session
    /// recording is active. The effects slice is cloned into owned storage.
    #[inline]
    pub fn record(&mut self, key: KeyEvent, effects: &[Effect]) {
        self.entries.push(ReplayEntry {
            key,
            effects: effects.to_vec(),
        });
    }

    /// Access the recorded entries as a slice.
    #[inline]
    #[must_use]
    pub fn entries(&self) -> &[ReplayEntry] {
        &self.entries
    }

    /// Consume the recorder and return the entries.
    ///
    /// Use this to transfer ownership to a [`SessionReplayer`].
    #[inline]
    #[must_use]
    pub fn into_entries(self) -> Vec<ReplayEntry> {
        self.entries
    }

    /// Number of recorded entries.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the recorder has no entries.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all recorded entries.
    #[inline]
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for SessionRecorder {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SessionReplayer
// ═══════════════════════════════════════════════════════════════════════════════

/// Iterates over recorded session entries with forward and backward traversal.
///
/// The replayer holds a snapshot of recorded entries and maintains a cursor
/// position for time-travel navigation. It supports:
///
/// - **Forward traversal** via [`next()`](Self::next)
/// - **Backward traversal** via [`prev()`](Self::prev) (time-travel)
/// - **Random access** via [`seek()`](Self::seek)
/// - **Current inspection** via [`current()`](Self::current)
///
/// The position is 0-based and starts "before" all entries (position 0 with
/// no current entry). After calling `next()` once, the current entry is at
/// index 0.
#[derive(Debug, Clone)]
pub struct SessionReplayer {
    /// The entries to replay.
    entries: Vec<ReplayEntry>,
    /// Current cursor position.
    ///
    /// When `cursor` is `None`, the replayer is positioned "before" all
    /// entries (initial state). When `Some(i)`, the replayer is positioned
    /// at entry `i`.
    cursor: Option<usize>,
}

impl SessionReplayer {
    /// Create a new replayer from recorded entries.
    ///
    /// The replayer starts positioned before all entries. Call [`next()`](Self::next)
    /// to advance to the first entry.
    #[inline]
    #[must_use]
    pub const fn new(entries: Vec<ReplayEntry>) -> Self {
        Self {
            entries,
            cursor: None,
        }
    }

    /// Advance to the next entry and return a reference to it.
    ///
    /// Returns `None` when the replayer has reached the end or is empty.
    #[must_use]
    #[allow(
        clippy::should_implement_trait,
        reason = "intentionally not implementing Iterator — Replayer is a single-pass cursor"
    )]
    pub fn next(&mut self) -> Option<&ReplayEntry> {
        let next_pos = match self.cursor {
            None => 0,
            Some(i) => i.checked_add(1)?,
        };
        if next_pos < self.entries.len() {
            self.cursor = Some(next_pos);
            self.entries.get(next_pos)
        } else {
            None
        }
    }

    /// Go back to the previous entry and return a reference to it.
    ///
    /// Returns `None` when the replayer is already at or before the first
    /// entry. This is the core "time-travel" operation.
    #[must_use]
    pub fn prev(&mut self) -> Option<&ReplayEntry> {
        let current = self.cursor?;
        if current == 0 {
            self.cursor = None;
            return None;
        }
        let prev_pos = current.checked_sub(1)?;
        self.cursor = Some(prev_pos);
        self.entries.get(prev_pos)
    }

    /// Return a reference to the current entry without advancing.
    ///
    /// Returns `None` if the replayer is positioned before all entries
    /// (initial state) or if the entries are empty.
    #[inline]
    #[must_use]
    pub fn current(&self) -> Option<&ReplayEntry> {
        self.cursor.and_then(|i| self.entries.get(i))
    }

    /// Return the current position index.
    ///
    /// Returns `0` when positioned before all entries (initial state).
    /// After calling `next()`, returns the 0-based index of the current entry.
    #[inline]
    #[must_use]
    pub fn position(&self) -> usize {
        self.cursor.unwrap_or(0)
    }

    /// Total number of entries in the replayer.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the replayer has no entries.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Jump to a specific position and return the entry at that position.
    ///
    /// Returns `None` if `pos` is out of bounds. On success, the cursor
    /// is updated to `pos`.
    #[must_use]
    pub fn seek(&mut self, pos: usize) -> Option<&ReplayEntry> {
        if pos < self.entries.len() {
            self.cursor = Some(pos);
            self.entries.get(pos)
        } else {
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::{ReplayEntry, SessionRecorder, SessionReplayer};
    use crate::effects::Effect;
    use crate::keymap::KeyEvent;
    use crate::primitives::Offset;

    /// Helper: create a simple `ReplayEntry` with a character key and a
    /// single `SetCursor` effect.
    fn make_entry(ch: char, cursor_offset: usize) -> ReplayEntry {
        ReplayEntry::new(
            KeyEvent::char(ch),
            vec![Effect::set_cursor(Offset::new(cursor_offset))],
        )
    }

    #[test]
    fn test_record_and_replay() {
        // Record three keystrokes.
        let mut recorder = SessionRecorder::new();
        recorder.record(KeyEvent::char('j'), &[Effect::set_cursor(Offset::new(10))]);
        recorder.record(KeyEvent::char('k'), &[Effect::set_cursor(Offset::new(0))]);
        recorder.record(KeyEvent::char('l'), &[Effect::set_cursor(Offset::new(1))]);

        assert_eq!(recorder.len(), 3);
        assert!(!recorder.is_empty());

        // Create replayer and verify forward traversal.
        let mut replayer = SessionReplayer::new(recorder.into_entries());
        assert_eq!(replayer.len(), 3);
        assert!(replayer.current().is_none(), "starts before all entries");

        let e0 = replayer.next();
        assert!(e0.is_some());
        assert_eq!(e0.map(|e| e.key), Some(KeyEvent::char('j')));
        assert_eq!(replayer.position(), 0);

        let e1 = replayer.next();
        assert!(e1.is_some());
        assert_eq!(e1.map(|e| e.key), Some(KeyEvent::char('k')));
        assert_eq!(replayer.position(), 1);

        let e2 = replayer.next();
        assert!(e2.is_some());
        assert_eq!(e2.map(|e| e.key), Some(KeyEvent::char('l')));
        assert_eq!(replayer.position(), 2);

        // Past the end.
        assert!(replayer.next().is_none());
        assert_eq!(replayer.position(), 2);

        // Backward traversal (time-travel).
        let prev = replayer.prev();
        assert!(prev.is_some());
        assert_eq!(prev.map(|e| e.key), Some(KeyEvent::char('k')));
        assert_eq!(replayer.position(), 1);

        let prev = replayer.prev();
        assert!(prev.is_some());
        assert_eq!(prev.map(|e| e.key), Some(KeyEvent::char('j')));
        assert_eq!(replayer.position(), 0);

        // Before the start.
        assert!(replayer.prev().is_none());
        assert!(replayer.current().is_none());
    }

    #[test]
    fn test_replayer_seek() {
        let entries = vec![
            make_entry('a', 0),
            make_entry('b', 1),
            make_entry('c', 2),
            make_entry('d', 3),
            make_entry('e', 4),
        ];
        let mut replayer = SessionReplayer::new(entries);

        // Seek to middle.
        let entry = replayer.seek(2);
        assert!(entry.is_some());
        assert_eq!(entry.map(|e| e.key), Some(KeyEvent::char('c')));
        assert_eq!(replayer.position(), 2);

        // Seek to start.
        let entry = replayer.seek(0);
        assert!(entry.is_some());
        assert_eq!(entry.map(|e| e.key), Some(KeyEvent::char('a')));
        assert_eq!(replayer.position(), 0);

        // Seek to end.
        let entry = replayer.seek(4);
        assert!(entry.is_some());
        assert_eq!(entry.map(|e| e.key), Some(KeyEvent::char('e')));
        assert_eq!(replayer.position(), 4);

        // Seek out of bounds.
        assert!(replayer.seek(5).is_none());
        assert!(replayer.seek(100).is_none());
        // Position unchanged after failed seek.
        assert_eq!(replayer.position(), 4);

        // Current still valid after failed seek.
        assert!(replayer.current().is_some());
        assert_eq!(replayer.current().map(|e| e.key), Some(KeyEvent::char('e')));
    }

    #[test]
    fn test_recorder_clear() {
        let mut recorder = SessionRecorder::new();
        recorder.record(KeyEvent::char('x'), &[Effect::set_cursor(Offset::new(0))]);
        recorder.record(KeyEvent::char('y'), &[Effect::set_cursor(Offset::new(1))]);
        assert_eq!(recorder.len(), 2);

        recorder.clear();
        assert!(recorder.is_empty());
        assert_eq!(recorder.len(), 0);
        assert!(recorder.entries().is_empty());
    }

    #[test]
    fn test_empty_replayer() {
        let mut replayer = SessionReplayer::new(Vec::new());
        assert!(replayer.is_empty());
        assert_eq!(replayer.len(), 0);
        assert!(replayer.current().is_none());
        assert!(replayer.next().is_none());
        assert!(replayer.prev().is_none());
        assert!(replayer.seek(0).is_none());
        assert_eq!(replayer.position(), 0);
    }

    #[test]
    fn test_recorder_default() {
        let recorder = SessionRecorder::default();
        assert!(recorder.is_empty());
        assert_eq!(recorder.len(), 0);
    }

    #[test]
    fn test_replay_entry_new() {
        let key = KeyEvent::char('w');
        let effects = vec![Effect::set_cursor(Offset::new(5)), Effect::ClearMessage];
        let entry = ReplayEntry::new(key, effects.clone());
        assert_eq!(entry.key, key);
        assert_eq!(entry.effects.len(), 2);
    }

    #[test]
    fn test_replayer_current_after_next() {
        let entries = vec![make_entry('a', 0), make_entry('b', 1)];
        let mut replayer = SessionReplayer::new(entries);

        replayer.next();
        let current = replayer.current();
        assert!(current.is_some());
        assert_eq!(current.map(|e| e.key), Some(KeyEvent::char('a')));

        replayer.next();
        let current = replayer.current();
        assert!(current.is_some());
        assert_eq!(current.map(|e| e.key), Some(KeyEvent::char('b')));
    }

    #[test]
    fn test_replayer_prev_at_start_returns_none() {
        let entries = vec![make_entry('a', 0)];
        let mut replayer = SessionReplayer::new(entries);

        // Advance to first entry.
        replayer.next();
        assert_eq!(replayer.position(), 0);

        // Go back — cursor moves before all entries.
        assert!(replayer.prev().is_none());
        assert!(replayer.current().is_none());

        // Going back again is still None.
        assert!(replayer.prev().is_none());
    }

    #[test]
    fn test_replayer_seek_then_next() {
        let entries = vec![make_entry('a', 0), make_entry('b', 1), make_entry('c', 2)];
        let mut replayer = SessionReplayer::new(entries);

        // Seek to position 1, then next should go to 2.
        replayer.seek(1);
        let entry = replayer.next();
        assert!(entry.is_some());
        assert_eq!(entry.map(|e| e.key), Some(KeyEvent::char('c')));
        assert_eq!(replayer.position(), 2);
    }

    #[test]
    fn test_recorder_multiple_effects_per_key() {
        let mut recorder = SessionRecorder::new();
        recorder.record(
            KeyEvent::char('d'),
            &[
                Effect::set_cursor(Offset::new(0)),
                Effect::set_mode(crate::primitives::Mode::Normal),
                Effect::ClearMessage,
            ],
        );
        assert_eq!(recorder.len(), 1);

        let entries = recorder.entries();
        assert_eq!(entries.len(), 1);
        if let Some(entry) = entries.first() {
            assert_eq!(entry.effects.len(), 3);
        }
    }
}
