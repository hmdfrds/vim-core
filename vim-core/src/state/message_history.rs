//! Message history ring buffer for `:messages`.
//!
//! # Layering
//!
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`. State modules are pure data containers with
//! no execution logic.
//!
//! Stores the last N messages emitted via `ShowMessage` / `ShowError` effects
//! so that the user can review them via `:messages`.

use compact_str::CompactString;
use std::collections::VecDeque;

/// Maximum number of messages retained in the history ring.
const MAX_MESSAGES: usize = 200;

/// Classification of a message entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MessageKind {
    /// Informational message (produced by `ShowMessage`).
    Info,
    /// Warning message.
    Warning,
    /// Error message (produced by `ShowError`).
    Error,
}

/// A single entry in the message history.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MessageEntry {
    /// Message text.
    pub text: CompactString,
    /// Severity / classification.
    pub kind: MessageKind,
    /// Monotonic sequence counter — assigned at push time.
    ///
    /// Starts at 0 and increments by 1 on every push. Never uses
    /// wall-clock time: the engine is a pure library with no `std::time` access.
    pub(crate) sequence: u64,
}

impl MessageEntry {
    /// Construct a new entry.
    #[must_use]
    pub const fn new(text: CompactString, kind: MessageKind, sequence: u64) -> Self {
        Self {
            text,
            kind,
            sequence,
        }
    }
}

/// Bounded ring buffer for Vim message history.
///
/// Retains up to `MAX_MESSAGES` (200) entries in insertion order.
/// When the buffer is full the oldest entry is evicted automatically
/// via O(1) `pop_front()` on the internal `VecDeque`.
///
/// The buffer is **transient** — it is not serialised with `VimState`
/// (marked `serde(skip)`) so messages do not persist across sessions.
/// This matches Neovim's behaviour.
#[derive(Debug, Clone, Default)]
pub struct MessageHistory {
    /// Storage (oldest first). Bounded to [`MAX_MESSAGES`] entries.
    entries: VecDeque<MessageEntry>,
    /// Monotonically increasing counter assigned to each entry.
    next_sequence: u64,
    /// The file of the last error raised in one, as Vim's `msg_source()`
    /// remembers it.
    last_file: Option<CompactString>,
    /// The line of the last line label.
    last_line: u32,
}

impl MessageHistory {
    /// Create a new, empty message history.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            next_sequence: 0,
            last_file: None,
            last_line: 0,
        }
    }

    /// Push an informational message into the history.
    pub fn push_info(&mut self, text: impl Into<CompactString>) {
        self.push(text.into(), MessageKind::Info);
    }

    /// Push a warning message into the history.
    pub fn push_warning(&mut self, text: impl Into<CompactString>) {
        self.push(text.into(), MessageKind::Warning);
    }

    /// Push an error message into the history.
    pub fn push_error(&mut self, text: impl Into<CompactString>) {
        self.push(text.into(), MessageKind::Error);
    }

    /// Push an error raised at `line` of the file `file`, after the entries
    /// Vim's `msg_source()` heads it with: `Error detected while processing
    /// {file}:` when the file is not the last error's, and `line {line}:`
    /// when the file or the line is not the last label's. `None` for an
    /// error raised outside a file, which makes the next one in a file be
    /// headed again.
    ///
    /// Deviation: a file without a name (`""`) gets the line labels, where
    /// Vim gives neither.
    pub(crate) fn push_error_at(&mut self, text: &str, place: Option<(&str, u32)>) {
        match place {
            Some((file, line)) => {
                let other = self.last_file.as_deref() != Some(file);
                if other && !file.is_empty() {
                    let header = format!("Error detected while processing {file}:");
                    self.push(header.into(), MessageKind::Error);
                }
                if (other || line != self.last_line) && line != 0 {
                    self.push(format!("line {line:4}:").into(), MessageKind::Info);
                    self.last_line = line;
                }
                if other {
                    self.last_file = Some(file.into());
                }
            }
            None => self.last_file = None,
        }
        self.push(text.into(), MessageKind::Error);
    }

    /// Forget where the last error was raised, so that the next one is
    /// headed in full, as after Vim's hit-enter prompt.
    pub(crate) fn forget_source(&mut self) {
        self.last_file = None;
        self.last_line = 0;
    }

    /// Internal push: appends an entry, evicting the oldest if at capacity.
    ///
    /// Eviction is O(1) via `VecDeque::pop_front`.
    fn push(&mut self, text: CompactString, kind: MessageKind) {
        if self.entries.len() == MAX_MESSAGES {
            self.entries.pop_front();
        }
        let seq = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.entries.push_back(MessageEntry::new(text, kind, seq));
    }

    /// Clear all message history entries.
    ///
    /// Resets the sequence counter so subsequent entries start from 0.
    /// This matches Neovim's `:messages clear` behaviour — display-only
    /// data with no cross-session persistence.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.next_sequence = 0;
    }

    /// Read-only access to the retained entries, oldest first.
    ///
    /// Returns `&VecDeque<MessageEntry>` which supports `.iter()`,
    /// `.front()`, `.back()`, indexing, etc.
    #[inline]
    #[must_use]
    pub const fn entries(&self) -> &VecDeque<MessageEntry> {
        &self.entries
    }

    /// Collect all entries into an owned `Vec`, oldest first.
    ///
    /// Provides a snapshot suitable for sending via `HostRequest::ShowMessageHistory`.
    #[must_use]
    pub fn to_vec(&self) -> Vec<MessageEntry> {
        self.entries.iter().cloned().collect()
    }

    /// Number of entries currently stored.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction ──────────────────────────────────────────────────────────

    #[test]
    fn test_new_is_empty() {
        let h = MessageHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
    }

    // ── Push Info / Warning / Error ───────────────────────────────────────────

    #[test]
    fn test_push_info_increments_len() {
        let mut h = MessageHistory::new();
        h.push_info("hello");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn test_push_warning() {
        let mut h = MessageHistory::new();
        h.push_warning("warn");
        assert_eq!(h.entries().back().unwrap().kind, MessageKind::Warning);
    }

    #[test]
    fn test_push_error_kind() {
        let mut h = MessageHistory::new();
        h.push_error("oops");
        let entry = h.entries().back().unwrap();
        assert_eq!(entry.kind, MessageKind::Error);
        assert_eq!(entry.text.as_str(), "oops");
    }

    // ── Sequence counter ──────────────────────────────────────────────────────

    #[test]
    fn test_sequence_is_monotonic() {
        let mut h = MessageHistory::new();
        h.push_info("a");
        h.push_info("b");
        h.push_info("c");

        let seqs: Vec<u64> = h.entries().iter().map(|e| e.sequence).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }

    #[test]
    fn test_sequence_resets_on_clear() {
        let mut h = MessageHistory::new();
        h.push_info("a");
        h.push_info("b");
        h.clear();
        h.push_info("c");
        assert_eq!(h.entries().back().unwrap().sequence, 0);
    }

    // ── Ring eviction ─────────────────────────────────────────────────────────

    #[test]
    fn test_evicts_oldest_when_full() {
        let mut h = MessageHistory::new();
        // Fill to capacity.
        for i in 0..MAX_MESSAGES {
            h.push_info(CompactString::from(format!("msg {i}")));
        }
        assert_eq!(h.len(), MAX_MESSAGES);

        // One more push — oldest ("msg 0") should be evicted.
        h.push_info("overflow");
        assert_eq!(h.len(), MAX_MESSAGES);
        assert_eq!(h.entries().front().unwrap().text.as_str(), "msg 1");
        assert_eq!(h.entries().back().unwrap().text.as_str(), "overflow");
    }

    #[test]
    fn test_sequence_does_not_reset_on_eviction() {
        let mut h = MessageHistory::new();
        for _ in 0..MAX_MESSAGES + 5 {
            h.push_info("x");
        }
        // sequence counter continues from MAX_MESSAGES+5, never resets.
        let last_seq = h.entries().back().unwrap().sequence;
        assert_eq!(last_seq, (MAX_MESSAGES + 4) as u64);
    }

    // ── Order preservation ────────────────────────────────────────────────────

    #[test]
    fn test_entries_ordered_oldest_first() {
        let mut h = MessageHistory::new();
        h.push_info("first");
        h.push_info("second");
        h.push_info("third");

        let texts: Vec<&str> = h.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
    }

    // ── to_vec ────────────────────────────────────────────────────────────────

    #[test]
    fn test_to_vec_clones_entries() {
        let mut h = MessageHistory::new();
        h.push_info("x");
        h.push_error("y");

        let v = h.to_vec();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].text.as_str(), "x");
        assert_eq!(v[1].text.as_str(), "y");
        assert_eq!(v[0].kind, MessageKind::Info);
        assert_eq!(v[1].kind, MessageKind::Error);
    }

    // ── Clear ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_clear_empties_history() {
        let mut h = MessageHistory::new();
        h.push_info("a");
        h.push_error("b");
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn test_clear_idempotent() {
        let mut h = MessageHistory::new();
        h.clear();
        h.clear();
        assert!(h.is_empty());
    }

    // ── Mixed kinds ───────────────────────────────────────────────────────────

    #[test]
    fn test_mixed_kinds_preserved() {
        let mut h = MessageHistory::new();
        h.push_info("i");
        h.push_warning("w");
        h.push_error("e");

        let kinds: Vec<MessageKind> = h.entries().iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![MessageKind::Info, MessageKind::Warning, MessageKind::Error]
        );
    }

    // ── Errors raised in a file ───────────────────────────────────────────────

    fn texts(h: &MessageHistory) -> Vec<&str> {
        h.entries().iter().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn errors_in_a_file_are_headed_as_msg_source_heads_them() {
        let mut h = MessageHistory::new();
        h.push_error_at("E1", Some(("a.vim", 2)));
        h.push_error_at("E2", Some(("a.vim", 2)));
        h.push_error_at("E3", Some(("a.vim", 12)));
        h.push_error_at("E4", Some(("b.vim", 12)));
        assert_eq!(
            texts(&h),
            [
                "Error detected while processing a.vim:",
                "line    2:",
                "E1",
                "E2",
                "line   12:",
                "E3",
                "Error detected while processing b.vim:",
                "line   12:",
                "E4",
            ]
        );
        assert_eq!(h.entries()[0].kind, MessageKind::Error);
        assert_eq!(h.entries()[1].kind, MessageKind::Info);
    }

    #[test]
    fn an_error_outside_a_file_makes_the_next_one_headed_again() {
        // msg_source(): the name is forgotten, the line is not.
        let mut h = MessageHistory::new();
        h.push_error_at("E1", Some(("a.vim", 2)));
        h.push_error_at("E2", None);
        h.push_error_at("E3", Some(("a.vim", 2)));
        h.forget_source();
        h.push_error_at("E4", Some(("a.vim", 2)));
        let header = "Error detected while processing a.vim:";
        assert_eq!(
            texts(&h),
            [
                header,
                "line    2:",
                "E1",
                "E2",
                header,
                "line    2:",
                "E3",
                header,
                "line    2:",
                "E4"
            ]
        );
    }

    #[test]
    fn errors_in_a_text_without_a_name_get_line_labels_only() {
        let mut h = MessageHistory::new();
        h.push_error_at("E1", Some(("", 1)));
        h.push_error_at("E2", Some(("", 1)));
        h.push_error_at("E3", Some(("", 3)));
        assert_eq!(texts(&h), ["line    1:", "E1", "E2", "line    3:", "E3"]);
    }

    // ── Empty to_vec ──────────────────────────────────────────────────────────

    #[test]
    fn test_to_vec_empty() {
        let h = MessageHistory::new();
        assert!(h.to_vec().is_empty());
    }
}
