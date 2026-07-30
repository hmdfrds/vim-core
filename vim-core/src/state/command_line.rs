//! Command-line mode state.
//!
//! Manages `:` command input, editing, and history.
//!
//! # Layering
//!
//! State holds pure data containers with no execution logic. Imports
//! `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`.
//!
//! # String Type Policy
//!
//! - `input` / `saved_input` use **`String`** — actively mutated every keystroke
//!   via `insert`, `remove`, `replace_range`. SSO has no benefit here because
//!   each mutation would require a `to_string()` → mutate → reconstruct round-trip.
//! - `history` uses **`CompactString`** — entries are written once (on commit)
//!   and only read afterward. SSO benefits short commands like `:w`, `:q`.
//!
//! # Design: Why Editing Methods Live Here
//!
//! The editing methods (`insert_char`, `backspace`, `delete_word`, etc.) are
//! self-contained mutations on private fields — they don't touch any external
//! state or produce side effects. Moving them to a separate handler in
//! `commands/` would force all fields to become `pub`, breaking encapsulation
//! for no modularity gain. They remain here intentionally.

use crate::primitives::{CommandLineEdit, Direction};
use compact_str::CompactString;
use std::collections::VecDeque;

/// Maximum history entries to keep.
const MAX_HISTORY: usize = 100;

/// Maximum command-line input length (bytes).
const MAX_INPUT_LENGTH: usize = 10_240;

// Re-exported from primitives (canonical location)
pub use crate::primitives::CommandLinePrompt;

/// A single completion candidate with optional metadata for rich display.
///
/// Used by the wire protocol to send completion candidates from the engine
/// to the host, enabling the host to render a completion menu.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompletionCandidate {
    /// The completion text that would be inserted.
    pub text: CompactString,
    /// Optional short description (e.g. command purpose).
    pub description: Option<CompactString>,
    /// Optional detail text (e.g. full syntax).
    pub detail: Option<CompactString>,
}

/// Active tab-completion state.
///
/// Tracks match list and cycling index for `:` command completion.
/// Created on first Tab press, cleared on any non-completion key.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct CompletionState {
    /// The full input text saved at the time of the first Tab press.
    /// Used to restore the original text when cycling past the last candidate.
    saved_input: String,
    /// The byte range within `saved_input` that is being completed.
    /// Only this range is replaced with the candidate text when cycling.
    replace_range: std::ops::Range<usize>,
    /// Computed completion candidates.
    candidates: Vec<CompletionCandidate>,
    /// Current cycling index into `candidates`.
    index: usize,
}

/// Command-line mode state.
///
/// Tracks the current input text, cursor position, and history
/// for `:` command-line mode.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CommandLineState {
    /// Current input text (actively mutated — uses `String` for zero-copy mutation).
    input: String,
    /// Cursor position (byte offset in input).
    cursor: usize,
    /// Ex command (`:`) history (newest last — uses `CompactString` for SSO).
    ex_history: VecDeque<CompactString>,
    /// Search (`/`, `?`) history (newest last — uses `CompactString` for SSO).
    search_history: VecDeque<CompactString>,
    /// Current history navigation index (None = not navigating).
    history_index: Option<usize>,
    /// Saved input when navigating history (actively mutated — uses `String`).
    saved_input: String,
    /// Current prompt kind (`:`, `/`, `?`).
    prompt: CommandLinePrompt,
    /// Whether we are awaiting a register name character (Ctrl-R sub-state).
    awaiting_register: bool,
    /// Active tab-completion state (None = not completing).
    completion: Option<CompletionState>,
    /// Direction for a pending host-side completion request.
    ///
    /// Set when `handle_tab_completion()` emits `RequestCmdlineCompletion`
    /// and cleared when the host fulfills the request. Stores the Tab
    /// direction (Forward for Tab, Backward for Shift-Tab) so the
    /// fulfillment handler knows which direction to cycle.
    pending_completion_direction: Option<Direction>,
    /// Replace range for a pending host-side completion request.
    ///
    /// Stored alongside `pending_completion_direction` so the fulfillment
    /// handler can pass the correct range to `complete_cycle()`.
    pending_completion_replace_range: Option<std::ops::Range<usize>>,
}

impl CommandLineState {
    /// Create a new empty command-line state.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a new command-line session for a prompt kind.
    pub fn begin(&mut self, prompt: CommandLinePrompt) {
        self.clear();
        self.prompt = prompt;
        self.awaiting_register = false;
    }

    /// Get the current input text.
    #[inline]
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Get the cursor position (byte offset).
    #[inline]
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Check if the input is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    /// Whether the command-line is awaiting a register name (Ctrl-R).
    #[inline]
    #[must_use]
    pub const fn awaiting_register(&self) -> bool {
        self.awaiting_register
    }

    /// Enter register-awaiting sub-state (Ctrl-R pressed).
    #[inline]
    pub const fn set_awaiting_register(&mut self, awaiting: bool) {
        self.awaiting_register = awaiting;
    }

    /// Store the direction and replace range for a pending host-side completion.
    ///
    /// Called when `handle_tab_completion()` emits `RequestCmdlineCompletion`.
    /// The fulfillment handler reads these back via
    /// [`Self::take_pending_completion_context`].
    pub const fn set_pending_completion_context(
        &mut self,
        direction: Direction,
        replace_range: std::ops::Range<usize>,
    ) {
        self.pending_completion_direction = Some(direction);
        self.pending_completion_replace_range = Some(replace_range);
    }

    /// Take the stored pending completion context, clearing it.
    ///
    /// Returns `Some((direction, replace_range))` if a host-side completion
    /// request is pending, or `None` if no context was stored.
    pub fn take_pending_completion_context(
        &mut self,
    ) -> Option<(Direction, std::ops::Range<usize>)> {
        let dir = self.pending_completion_direction.take()?;
        let range = self.pending_completion_replace_range.take()?;
        Some((dir, range))
    }

    /// Clear input and reset cursor.
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.history_index = None;
        self.saved_input.clear();
        self.awaiting_register = false;
        self.completion = None;
        self.pending_completion_direction = None;
        self.pending_completion_replace_range = None;
    }

    /// Apply a pure edit instruction to the state.
    pub fn apply_edit(&mut self, edit: CommandLineEdit) {
        // Non-completion edits clear active completion
        if !matches!(
            edit,
            CommandLineEdit::CompleteNext
                | CommandLineEdit::CompletePrev
                | CommandLineEdit::ListCompletions
        ) {
            self.completion = None;
        }

        match edit {
            CommandLineEdit::InsertChar(c) => self.insert_char(c),
            CommandLineEdit::Backspace => self.backspace(),
            CommandLineEdit::Delete => self.delete(),
            CommandLineEdit::DeleteWord => self.delete_word(),
            CommandLineEdit::DeleteToStart => self.delete_to_start(),
            CommandLineEdit::DeleteToEnd => self.delete_to_end(),
            CommandLineEdit::MoveLeft => self.move_left(),
            CommandLineEdit::MoveRight => self.move_right(),
            CommandLineEdit::MoveToStart => self.move_to_start(),
            CommandLineEdit::MoveToEnd => self.move_to_end(),
            CommandLineEdit::MoveWordLeft => self.move_word_left(),
            CommandLineEdit::MoveWordRight => self.move_word_right(),
            CommandLineEdit::HistoryPrev => self.history_prev(),
            CommandLineEdit::HistoryNext => self.history_next(),
            // CompleteNext/CompletePrev/ListCompletions are handled by the engine
            // layer which computes matches and calls `complete_cycle()` directly.
            // If they somehow reach here, they're no-ops.
            CommandLineEdit::CompleteNext
            | CommandLineEdit::CompletePrev
            | CommandLineEdit::ListCompletions => {}
        }
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        if self.input.len() + c.len_utf8() > MAX_INPUT_LENGTH {
            return;
        }
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.history_index = None;
    }

    /// Insert a string at the cursor position (for register insertion).
    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if self.input.len() + s.len() > MAX_INPUT_LENGTH {
            return;
        }
        self.input.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.history_index = None;
    }

    /// Delete character before cursor (backspace).
    pub fn backspace(&mut self) {
        self.history_index = None;
        if self.cursor > 0 {
            // Find the previous character boundary
            let prev_boundary = self.input[..self.cursor]
                .char_indices()
                .last()
                .map_or(0, |(i, _)| i);
            self.input.remove(prev_boundary);
            self.cursor = prev_boundary;
        }
    }

    /// Delete character at cursor (delete key).
    pub fn delete(&mut self) {
        self.history_index = None;
        if self.cursor < self.input.len() {
            self.input.remove(self.cursor);
        }
    }

    /// Delete word before cursor (Ctrl-W).
    ///
    /// Matches Vim's command-line Ctrl-W behavior:
    /// 1. Skip any whitespace immediately before cursor
    /// 2. If the character before cursor is a keyword character (alnum or `_`),
    ///    delete backward through keyword characters
    /// 3. Otherwise delete backward through non-keyword, non-whitespace characters
    ///
    /// This means `/`, `.`, `*` etc. are treated as word boundaries, matching
    /// Vim's `iskeyword`-based word definition on the command line.
    // Safety: `pos` starts at `chars.len()` and decrements with `> 0` guards;
    // `chars[pos - 1]` and `chars[pos]` are always in-bounds.
    pub fn delete_word(&mut self) {
        self.history_index = None;
        if self.cursor == 0 {
            return;
        }

        let before = &self.input[..self.cursor];
        let is_keyword = |c: char| c.is_alphanumeric() || c == '_';

        // Find the last character (just before cursor) without collecting.
        let last_char = match before.chars().next_back() {
            Some(c) => c,
            None => return,
        };

        // Scan backward using char_indices().rev() — no Vec allocation needed.
        // We track the byte offset of the start of the word-class run.
        let mut start = 0;
        if last_char.is_whitespace() {
            // Delete backward through whitespace
            for (i, c) in before.char_indices().rev() {
                if !c.is_whitespace() {
                    start = i + c.len_utf8();
                    break;
                }
            }
        } else if is_keyword(last_char) {
            // Delete backward through keyword characters
            for (i, c) in before.char_indices().rev() {
                if !is_keyword(c) {
                    start = i + c.len_utf8();
                    break;
                }
            }
        } else {
            // Delete backward through non-keyword, non-whitespace characters
            for (i, c) in before.char_indices().rev() {
                if is_keyword(c) || c.is_whitespace() {
                    start = i + c.len_utf8();
                    break;
                }
            }
        }

        self.input.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Delete from cursor to start of line (Ctrl-U).
    pub fn delete_to_start(&mut self) {
        self.history_index = None;
        self.input.replace_range(..self.cursor, "");
        self.cursor = 0;
    }

    /// Delete from cursor to end of line (Ctrl-K).
    pub fn delete_to_end(&mut self) {
        self.history_index = None;
        self.input.truncate(self.cursor);
    }

    /// Move cursor left.
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.input[..self.cursor]
                .char_indices()
                .last()
                .map_or(0, |(i, _)| i);
        }
    }

    /// Move cursor right.
    pub fn move_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor += self.input[self.cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
        }
    }

    /// Move cursor one word left.
    ///
    /// Scans backward: skip non-word chars, then skip word chars.
    /// Uses same word classification as `delete_word`.
    // Safety: `pos` starts at `chars.len()` and decrements with `> 0` guards;
    // `chars[pos - 1]` and `chars[pos]` are always in-bounds.
    pub fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before = &self.input[..self.cursor];
        let is_keyword = |c: char| c.is_alphanumeric() || c == '_';

        // Two-phase backward scan without collecting into a Vec.
        // Phase 1: skip non-word characters (whitespace + punctuation)
        // Phase 2: skip word characters
        let mut target = 0;
        let mut phase = 0; // 0 = skipping non-word, 1 = skipping word
        for (i, c) in before.char_indices().rev() {
            if phase == 0 {
                if is_keyword(c) {
                    phase = 1; // transition to skipping word chars
                }
            } else if !is_keyword(c) {
                target = i + c.len_utf8();
                break;
            }
        }
        // If we exhausted the iterator in either phase, target stays 0
        self.cursor = target;
    }

    /// Move cursor one word right.
    ///
    /// Scans forward: skip word chars, then skip non-word chars.
    /// Uses same word classification as `delete_word`.
    // Safety: `pos` starts at 0 and increments with `< len` guards;
    // `chars[pos]` is always in-bounds.
    pub fn move_word_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let after = &self.input[self.cursor..];
        let is_keyword = |c: char| c.is_alphanumeric() || c == '_';

        // Three-phase forward scan without collecting into a Vec.
        // Phase 0: skip through same-class chars as the first char
        // Phase 1: skip whitespace
        // Phase 2: done (found start of next word)
        let first_char = match after.chars().next() {
            Some(c) => c,
            None => return,
        };

        let mut advance = after.len(); // default: move to end
        let first_is_keyword = is_keyword(first_char);
        let first_is_non_ws_non_kw = !first_is_keyword && !first_char.is_whitespace();
        let mut past_first_class = false;
        let mut in_whitespace = false;

        for (i, c) in after.char_indices().skip(1) {
            if !past_first_class {
                // Detect class transition: keyword->non-keyword, or non-kw-non-ws->other, or ws->non-ws
                let class_changed = (first_is_keyword && !is_keyword(c))
                    || (first_is_non_ws_non_kw && (is_keyword(c) || c.is_whitespace()));
                if class_changed {
                    past_first_class = true;
                    if c.is_whitespace() {
                        in_whitespace = true;
                    } else {
                        advance = i;
                        break;
                    }
                } else if !first_is_keyword && !first_is_non_ws_non_kw && !c.is_whitespace() {
                    // first char was whitespace, skip whitespace then stop
                    advance = i;
                    break;
                }
            } else if in_whitespace && !c.is_whitespace() {
                advance = i;
                break;
            }
        }
        self.cursor += advance;
    }

    /// Move cursor to start (Home).
    pub const fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end (End).
    pub const fn move_to_end(&mut self) {
        self.cursor = self.input.len();
    }

    /// Get the active history for the current prompt type.
    const fn active_history(&self) -> &VecDeque<CompactString> {
        match self.prompt {
            CommandLinePrompt::Ex | CommandLinePrompt::ExVisual => &self.ex_history,
            CommandLinePrompt::SearchForward | CommandLinePrompt::SearchBackward => {
                &self.search_history
            }
        }
    }

    /// Navigate to previous history entry (Up arrow).
    ///
    /// When the user has typed a prefix before pressing Up, only entries
    /// starting with that prefix are visited (Vim's `c_<Up>` behavior).
    /// The prefix is captured from the current input when navigation starts
    /// (i.e., `history_index` is `None`).
    pub fn history_prev(&mut self) {
        let history = self.active_history();
        if history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // Start navigating - save current input as prefix filter
                self.saved_input = self.input.clone();
                // Search backward from the newest entry for a match
                let prefix = &self.saved_input;
                let len = self.active_history().len();
                let found = (0..len)
                    .rev()
                    .find(|&i| Self::entry_matches_prefix(self.active_history().get(i), prefix));
                if let Some(idx) = found {
                    self.history_index = Some(idx);
                    if let Some(entry) = self.active_history().get(idx) {
                        self.input = entry.to_string();
                        self.cursor = self.input.len();
                    }
                }
                // If no matching entry, stay on current input (no navigation)
            }
            Some(current_idx) => {
                // Already navigating — find the next older matching entry
                let prefix = &self.saved_input;
                let found = (0..current_idx)
                    .rev()
                    .find(|&i| Self::entry_matches_prefix(self.active_history().get(i), prefix));
                if let Some(idx) = found {
                    self.history_index = Some(idx);
                    if let Some(entry) = self.active_history().get(idx) {
                        self.input = entry.to_string();
                        self.cursor = self.input.len();
                    }
                }
                // If no older match, stay at current position
            }
        }
    }

    /// Navigate to next history entry (Down arrow).
    ///
    /// Respects the same prefix filter as [`Self::history_prev`]. Only entries
    /// matching the saved prefix are visited. When past the newest match,
    /// restores the original input.
    pub fn history_next(&mut self) {
        let history_len = self.active_history().len();
        match self.history_index {
            None => {
                // Not navigating
            }
            Some(current_idx) => {
                // Find the next newer matching entry
                let prefix = &self.saved_input;
                let found = (current_idx + 1..history_len)
                    .find(|&i| Self::entry_matches_prefix(self.active_history().get(i), prefix));
                if let Some(idx) = found {
                    self.history_index = Some(idx);
                    if let Some(entry) = self.active_history().get(idx) {
                        self.input = entry.to_string();
                        self.cursor = self.input.len();
                    }
                } else {
                    // Past newest match — return to saved input
                    self.input = std::mem::take(&mut self.saved_input);
                    self.cursor = self.input.len();
                    self.history_index = None;
                }
            }
        }
    }

    /// Check if a history entry matches the prefix filter.
    ///
    /// An empty prefix matches everything.
    fn entry_matches_prefix(entry: Option<&CompactString>, prefix: &str) -> bool {
        if prefix.is_empty() {
            return true;
        }
        entry.is_some_and(|e| e.starts_with(prefix))
    }

    /// Cycle through tab-completion candidates (Tab = forward, Shift-Tab = backward).
    ///
    /// `replace_range` specifies which byte range of the input to replace with
    /// the candidate text. For command-name completion, where the prefix is
    /// the entire input, this covers `0..input.len()`. For setting completion
    /// after `:set `, the range starts after the space.
    ///
    /// If no active completion exists, `candidates` is used to initialize. The
    /// caller computes candidates from `self.input()` via the completion module
    /// (which lives in the `commands` layer — state cannot import it).
    pub fn complete_cycle(
        &mut self,
        direction: Direction,
        candidates: &[CompletionCandidate],
        replace_range: std::ops::Range<usize>,
    ) {
        if let Some(ref mut state) = self.completion {
            // Already completing — cycle
            if state.candidates.is_empty() {
                return;
            }
            if direction.is_forward() {
                state.index = (state.index + 1) % (state.candidates.len() + 1);
            } else {
                state.index = if state.index == 0 {
                    state.candidates.len()
                } else {
                    state.index - 1
                };
            }
            // Index == candidates.len() means "return to original input"
            if state.index == state.candidates.len() {
                self.input = state.saved_input.clone();
            } else if let Some(c) = state.candidates.get(state.index) {
                // Replace only the completion range with the candidate text.
                let mut rebuilt = state.saved_input[..state.replace_range.start].to_string();
                rebuilt.push_str(&c.text);
                rebuilt.push_str(&state.saved_input[state.replace_range.end..]);
                self.input = rebuilt;
            }
            self.cursor = self.input.len();
        } else {
            // First Tab — initialize from provided candidates
            if candidates.is_empty() {
                return;
            }
            let saved_input = self.input.clone();
            let candidates = candidates.to_vec();

            // Apply the first candidate
            if let Some(first) = candidates.first() {
                let mut rebuilt = saved_input[..replace_range.start].to_string();
                rebuilt.push_str(&first.text);
                rebuilt.push_str(&saved_input[replace_range.end..]);
                self.input = rebuilt;
            }
            self.cursor = self.input.len();
            self.completion = Some(CompletionState {
                saved_input,
                replace_range,
                candidates,
                index: 0,
            });
        }
    }

    /// Convenience wrapper for simple string-slice matches (command-name completion).
    ///
    /// Wraps each `&str` in a [`CompletionCandidate`] with no description or
    /// detail, and sets the replace range to the entire input.
    pub fn complete_cycle_simple(&mut self, direction: Direction, matches: &[&str]) {
        let replace_range = 0..self.input.len();
        let candidates: Vec<CompletionCandidate> = matches
            .iter()
            .map(|&s| CompletionCandidate {
                text: CompactString::from(s),
                description: None,
                detail: None,
            })
            .collect();
        self.complete_cycle(direction, &candidates, replace_range);
    }

    /// Whether a tab-completion session is currently active.
    #[inline]
    #[must_use]
    pub const fn is_completing(&self) -> bool {
        self.completion.is_some()
    }

    /// Returns the current completion candidates and selected index, if completing.
    ///
    /// The returned slice contains all candidates; the `usize` is the current
    /// cycling index. When `index == candidates.len()`, the user has cycled
    /// back to the original prefix.
    #[must_use]
    pub fn completion_snapshot(&self) -> Option<(&[CompletionCandidate], usize)> {
        let comp = self.completion.as_ref()?;
        Some((&comp.candidates, comp.index))
    }

    /// Accept a completion candidate by index, replacing the input text and
    /// clearing completion state.
    ///
    /// Called when the host's completion menu reports a pick by index rather
    /// than the user cycling with Tab. The completion portion of the input is
    /// replaced with the selected candidate's text. If `index` is out of range
    /// or no completion session is active, this is a no-op.
    pub fn accept_completion(&mut self, index: usize) {
        if let Some(comp) = self.completion.take() {
            if let Some(candidate) = comp.candidates.get(index) {
                let range = &comp.replace_range;
                let mut new_input = String::with_capacity(
                    range.start + candidate.text.len() + (comp.saved_input.len() - range.end),
                );
                new_input.push_str(&comp.saved_input[..range.start]);
                new_input.push_str(&candidate.text);
                new_input.push_str(&comp.saved_input[range.end..]);
                self.cursor = range.start + candidate.text.len();
                self.input = new_input;
            }
            // completion is already cleared by .take() above
        }
    }

    /// Add the current input to history and clear.
    pub fn commit(&mut self) {
        if !self.input.is_empty() {
            // Inline history selection to avoid borrowing all of `self`
            let history = match self.prompt {
                CommandLinePrompt::Ex | CommandLinePrompt::ExVisual => &mut self.ex_history,
                CommandLinePrompt::SearchForward | CommandLinePrompt::SearchBackward => {
                    &mut self.search_history
                }
            };
            // Avoid duplicates
            if history.back().map(CompactString::as_str) != Some(self.input.as_str()) {
                history.push_back(CompactString::from(self.input.as_str()));
                if history.len() > MAX_HISTORY {
                    history.pop_front();
                }
            }
        }
        self.clear();
    }

    /// Get the command text (without leading `:` or `/` or `?`).
    #[inline]
    #[must_use]
    pub fn command(&self) -> &str {
        &self.input
    }

    /// Get active prompt kind.
    #[inline]
    #[must_use]
    pub const fn prompt(&self) -> CommandLinePrompt {
        self.prompt
    }

    /// Get the ex command (`:`) history (newest last).
    #[inline]
    #[must_use]
    pub const fn ex_history(&self) -> &VecDeque<CompactString> {
        &self.ex_history
    }

    /// Get the search (`/`, `?`) history (newest last).
    #[inline]
    #[must_use]
    pub const fn search_history(&self) -> &VecDeque<CompactString> {
        &self.search_history
    }

    /// Get the history for a given prompt kind.
    ///
    /// Returns ex history for `:` prompts, search history for `/`/`?` prompts.
    #[inline]
    #[must_use]
    pub const fn history_for_prompt(&self, prompt: CommandLinePrompt) -> &VecDeque<CompactString> {
        match prompt {
            CommandLinePrompt::Ex | CommandLinePrompt::ExVisual => &self.ex_history,
            CommandLinePrompt::SearchForward | CommandLinePrompt::SearchBackward => {
                &self.search_history
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_backspace() {
        let mut state = CommandLineState::new();
        state.insert_char('a');
        state.insert_char('b');
        state.insert_char('c');
        assert_eq!(state.input(), "abc");
        assert_eq!(state.cursor(), 3);

        state.backspace();
        assert_eq!(state.input(), "ab");
        assert_eq!(state.cursor(), 2);
    }

    #[test]
    fn test_backspace_at_zero_is_noop() {
        let mut state = CommandLineState::new();
        state.backspace();
        assert_eq!(state.input(), "");
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn test_delete_at_end_is_noop() {
        let mut state = CommandLineState::new();
        state.insert_char('a');
        state.delete();
        assert_eq!(state.input(), "a");
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn test_delete_word() {
        let mut state = CommandLineState::new();
        state.input = "hello world".to_string();
        state.cursor = 11;

        state.delete_word();
        assert_eq!(state.input(), "hello ");
        assert_eq!(state.cursor(), 6);
    }

    #[test]
    fn test_delete_word_at_punctuation() {
        let mut state = CommandLineState::new();
        state.input = "s/hello/world".to_string();
        state.cursor = 13;

        state.delete_word();
        assert_eq!(state.input(), "s/hello/");
        assert_eq!(state.cursor(), 8);
    }

    #[test]
    fn test_delete_word_at_zero_is_noop() {
        let mut state = CommandLineState::new();
        state.input = "hello".to_string();
        state.cursor = 0;
        state.delete_word();
        assert_eq!(state.input(), "hello");
    }

    #[test]
    fn test_delete_to_start() {
        let mut state = CommandLineState::new();
        state.input = "hello world".to_string();
        state.cursor = 6;

        state.delete_to_start();
        assert_eq!(state.input(), "world");
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn test_delete_to_end() {
        let mut state = CommandLineState::new();
        state.input = "hello world".to_string();
        state.cursor = 5;

        state.delete_to_end();
        assert_eq!(state.input(), "hello");
    }

    #[test]
    fn test_cursor_movement() {
        let mut state = CommandLineState::new();
        state.input = "abc".to_string();
        state.cursor = 3;

        state.move_left();
        assert_eq!(state.cursor(), 2);

        state.move_left();
        assert_eq!(state.cursor(), 1);

        state.move_right();
        assert_eq!(state.cursor(), 2);

        state.move_to_start();
        assert_eq!(state.cursor(), 0);

        state.move_to_end();
        assert_eq!(state.cursor(), 3);
    }

    #[test]
    fn test_unicode_cursor_navigation() {
        let mut state = CommandLineState::new();
        // "café" — é is 2 bytes in UTF-8
        state.insert_char('c');
        state.insert_char('a');
        state.insert_char('f');
        state.insert_char('é');
        assert_eq!(state.input(), "café");
        assert_eq!(state.cursor(), 5); // c(1) + a(1) + f(1) + é(2)

        state.move_left();
        assert_eq!(state.cursor(), 3); // before é

        state.backspace();
        assert_eq!(state.input(), "caé");
        assert_eq!(state.cursor(), 2);
    }

    #[test]
    fn test_history_navigation() {
        let mut state = CommandLineState::new();
        state.input = "first".to_string();
        state.commit();
        state.input = "second".to_string();
        state.commit();

        // Empty input — Up/Down navigate all history entries
        state.input = String::new();
        state.cursor = 0;

        state.history_prev();
        assert_eq!(state.input(), "second");

        state.history_prev();
        assert_eq!(state.input(), "first");

        state.history_next();
        assert_eq!(state.input(), "second");

        state.history_next();
        assert_eq!(state.input(), "");
    }

    #[test]
    fn test_commit_empty_does_not_add_to_history() {
        let mut state = CommandLineState::new();
        state.commit();
        state.commit();
        // No history entries from empty commits
        state.history_prev(); // should be noop
        assert_eq!(state.input(), "");
    }

    #[test]
    fn test_commit_deduplicates_consecutive() {
        let mut state = CommandLineState::new();
        state.input = "same".to_string();
        state.commit();
        state.input = "same".to_string();
        state.commit();

        // Empty input to navigate all history
        state.input = String::new();
        state.cursor = 0;

        state.history_prev();
        assert_eq!(state.input(), "same");

        // Going prev again should stay at "same" (only one entry)
        state.history_prev();
        assert_eq!(state.input(), "same");
    }

    #[test]
    fn test_begin_clears_state() {
        let mut state = CommandLineState::new();
        state.insert_char('x');
        state.begin(CommandLinePrompt::SearchForward);
        assert_eq!(state.input(), "");
        assert_eq!(state.cursor(), 0);
        assert_eq!(state.prompt(), CommandLinePrompt::SearchForward);
    }

    #[test]
    fn test_history_eviction_at_max() {
        let mut state = CommandLineState::new();

        // Push MAX_HISTORY + 5 unique entries
        for i in 0..MAX_HISTORY + 5 {
            state.input = format!("cmd{i}");
            state.commit();
        }

        // Navigate to oldest — should be "cmd5" (first 5 were evicted)
        // Empty input to navigate all entries
        state.input = String::new();
        state.cursor = 0;
        for _ in 0..MAX_HISTORY {
            state.history_prev();
        }
        assert_eq!(state.input(), "cmd5");

        // Can't go further back
        state.history_prev();
        assert_eq!(state.input(), "cmd5");
    }

    #[test]
    fn test_insert_at_mid_position() {
        let mut state = CommandLineState::new();
        state.input = "ac".to_string();
        state.cursor = 1;
        state.insert_char('b');
        assert_eq!(state.input(), "abc");
        assert_eq!(state.cursor(), 2);
    }

    #[test]
    fn test_separate_ex_and_search_histories() {
        let mut state = CommandLineState::new();

        // Add Ex commands
        state.begin(CommandLinePrompt::Ex);
        state.input = "write".to_string();
        state.commit();
        state.begin(CommandLinePrompt::Ex);
        state.input = "quit".to_string();
        state.commit();

        // Add search patterns
        state.begin(CommandLinePrompt::SearchForward);
        state.input = "foo".to_string();
        state.commit();
        state.begin(CommandLinePrompt::SearchBackward);
        state.input = "bar".to_string();
        state.commit();

        // Ex history should only contain Ex commands
        state.begin(CommandLinePrompt::Ex);
        // Empty input to navigate all entries
        state.history_prev();
        assert_eq!(state.input(), "quit");
        state.history_prev();
        assert_eq!(state.input(), "write");
        // No further back
        state.history_prev();
        assert_eq!(state.input(), "write");

        // Search history should only contain search patterns
        // (/ and ? share the same search history)
        state.begin(CommandLinePrompt::SearchForward);
        // Empty input to navigate all entries
        state.history_prev();
        assert_eq!(state.input(), "bar");
        state.history_prev();
        assert_eq!(state.input(), "foo");
        state.history_prev();
        assert_eq!(state.input(), "foo");
    }

    // ═══════════════════════════════════════════════════════════════════
    // Public history accessors (used by the command window, q: and q/)
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn ex_history_accessor_returns_committed_entries() {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::Ex);
        state.input = "write".to_string();
        state.commit();
        state.begin(CommandLinePrompt::Ex);
        state.input = "quit".to_string();
        state.commit();

        let history = state.ex_history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].as_str(), "write");
        assert_eq!(history[1].as_str(), "quit");
    }

    #[test]
    fn search_history_accessor_returns_committed_entries() {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::SearchForward);
        state.input = "foo".to_string();
        state.commit();
        state.begin(CommandLinePrompt::SearchBackward);
        state.input = "bar".to_string();
        state.commit();

        let history = state.search_history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].as_str(), "foo");
        assert_eq!(history[1].as_str(), "bar");
    }

    #[test]
    fn history_for_prompt_routes_correctly() {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::Ex);
        state.input = "write".to_string();
        state.commit();
        state.begin(CommandLinePrompt::SearchForward);
        state.input = "foo".to_string();
        state.commit();

        // Ex prompt → ex_history
        let ex = state.history_for_prompt(CommandLinePrompt::Ex);
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].as_str(), "write");

        // ExVisual → same as Ex
        let ex_visual = state.history_for_prompt(CommandLinePrompt::ExVisual);
        assert_eq!(ex_visual.len(), 1);

        // SearchForward → search_history
        let sf = state.history_for_prompt(CommandLinePrompt::SearchForward);
        assert_eq!(sf.len(), 1);
        assert_eq!(sf[0].as_str(), "foo");

        // SearchBackward → same as SearchForward
        let sb = state.history_for_prompt(CommandLinePrompt::SearchBackward);
        assert_eq!(sb.len(), 1);
        assert_eq!(sb[0].as_str(), "foo");
    }

    #[test]
    fn move_word_left_from_end() {
        let mut cl = CommandLineState::new();
        for c in "hello world".chars() {
            cl.apply_edit(CommandLineEdit::InsertChar(c));
        }
        // Cursor at end: "hello world|"
        cl.apply_edit(CommandLineEdit::MoveWordLeft);
        // Should move to start of "world": "hello |world"
        assert_eq!(cl.cursor(), 6);
        cl.apply_edit(CommandLineEdit::MoveWordLeft);
        // Should move to start of "hello": "|hello world"
        assert_eq!(cl.cursor(), 0);
    }

    #[test]
    fn move_word_right_from_start() {
        let mut cl = CommandLineState::new();
        for c in "hello world".chars() {
            cl.apply_edit(CommandLineEdit::InsertChar(c));
        }
        cl.apply_edit(CommandLineEdit::MoveToStart);
        // Cursor at start: "|hello world"
        cl.apply_edit(CommandLineEdit::MoveWordRight);
        // Should move past "hello" to start of "world": "hello |world"
        assert_eq!(cl.cursor(), 6);
        cl.apply_edit(CommandLineEdit::MoveWordRight);
        // Should move to end: "hello world|"
        assert_eq!(cl.cursor(), 11);
    }

    #[test]
    fn move_word_left_at_start_is_noop() {
        let mut cl = CommandLineState::new();
        for c in "hello".chars() {
            cl.apply_edit(CommandLineEdit::InsertChar(c));
        }
        cl.apply_edit(CommandLineEdit::MoveToStart);
        cl.apply_edit(CommandLineEdit::MoveWordLeft);
        assert_eq!(cl.cursor(), 0);
    }

    #[test]
    fn move_word_right_at_end_is_noop() {
        let mut cl = CommandLineState::new();
        for c in "hello".chars() {
            cl.apply_edit(CommandLineEdit::InsertChar(c));
        }
        cl.apply_edit(CommandLineEdit::MoveWordRight);
        assert_eq!(cl.cursor(), 5);
    }

    #[test]
    fn empty_history_accessor_returns_empty() {
        let state = CommandLineState::new();
        assert!(state.ex_history().is_empty());
        assert!(state.search_history().is_empty());
        assert!(state.history_for_prompt(CommandLinePrompt::Ex).is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════
    // Range-based complete_cycle tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn complete_cycle_with_replace_range_setting() {
        // Simulates `:set scr<Tab>` -> should become `set scrolloff`
        let mut cl = CommandLineState::new();
        for c in "set scr".chars() {
            cl.insert_char(c);
        }
        assert_eq!(cl.input(), "set scr");

        let candidates = vec![CompletionCandidate {
            text: CompactString::from("scrolloff"),
            description: None,
            detail: None,
        }];
        // Replace range 4..7 ("scr" part of "set scr")
        cl.complete_cycle(Direction::Forward, &candidates, 4..7);
        assert_eq!(cl.input(), "set scrolloff");
        assert_eq!(cl.cursor(), 13);
    }

    #[test]
    fn complete_cycle_with_replace_range_cycles_back_to_original() {
        let mut cl = CommandLineState::new();
        for c in "set scr".chars() {
            cl.insert_char(c);
        }

        let candidates = vec![CompletionCandidate {
            text: CompactString::from("scrolloff"),
            description: None,
            detail: None,
        }];
        // First Tab: "set scr" -> "set scrolloff"
        cl.complete_cycle(Direction::Forward, &candidates, 4..7);
        assert_eq!(cl.input(), "set scrolloff");

        // Second Tab: cycle past last -> back to original "set scr"
        cl.complete_cycle(Direction::Forward, &candidates, 4..7);
        assert_eq!(cl.input(), "set scr");
    }

    #[test]
    fn complete_cycle_with_replace_range_multiple_candidates() {
        let mut cl = CommandLineState::new();
        for c in "set s".chars() {
            cl.insert_char(c);
        }

        let candidates = vec![
            CompletionCandidate {
                text: CompactString::from("scrolloff"),
                description: None,
                detail: None,
            },
            CompletionCandidate {
                text: CompactString::from("shiftwidth"),
                description: None,
                detail: None,
            },
        ];
        // First Tab: "set s" -> "set scrolloff"
        cl.complete_cycle(Direction::Forward, &candidates, 4..5);
        assert_eq!(cl.input(), "set scrolloff");

        // Second Tab: -> "set shiftwidth"
        cl.complete_cycle(Direction::Forward, &candidates, 4..5);
        assert_eq!(cl.input(), "set shiftwidth");

        // Third Tab: cycle back to original -> "set s"
        cl.complete_cycle(Direction::Forward, &candidates, 4..5);
        assert_eq!(cl.input(), "set s");
    }

    #[test]
    fn complete_cycle_backward_with_replace_range() {
        let mut cl = CommandLineState::new();
        for c in "set scr".chars() {
            cl.insert_char(c);
        }

        let candidates = vec![
            CompletionCandidate {
                text: CompactString::from("scrolloff"),
                description: None,
                detail: None,
            },
            CompletionCandidate {
                text: CompactString::from("smartcase"),
                description: None,
                detail: None,
            },
        ];
        // First forward Tab: -> "set scrolloff"
        cl.complete_cycle(Direction::Forward, &candidates, 4..7);
        assert_eq!(cl.input(), "set scrolloff");

        // Shift-Tab (backward): cycle back to original "set scr"
        cl.complete_cycle(Direction::Backward, &candidates, 4..7);
        assert_eq!(cl.input(), "set scr");

        // Another Shift-Tab: wraps to last candidate -> "set smartcase"
        cl.complete_cycle(Direction::Backward, &candidates, 4..7);
        assert_eq!(cl.input(), "set smartcase");
    }

    #[test]
    fn complete_cycle_simple_replaces_entire_input() {
        // Backward compatibility: complete_cycle_simple replaces the whole input
        let mut cl = CommandLineState::new();
        for c in "wri".chars() {
            cl.insert_char(c);
        }

        cl.complete_cycle_simple(Direction::Forward, &["write", "wq"]);
        assert_eq!(cl.input(), "write");

        cl.complete_cycle_simple(Direction::Forward, &["write", "wq"]);
        assert_eq!(cl.input(), "wq");

        cl.complete_cycle_simple(Direction::Forward, &["write", "wq"]);
        assert_eq!(cl.input(), "wri");
    }

    #[test]
    fn complete_cycle_setting_value_replace_range() {
        // Simulates `:set selection=ex<Tab>` -> `set selection=exclusive`
        let mut cl = CommandLineState::new();
        for c in "set selection=ex".chars() {
            cl.insert_char(c);
        }

        let candidates = vec![CompletionCandidate {
            text: CompactString::from("exclusive"),
            description: None,
            detail: None,
        }];
        // Replace range 14..16 ("ex" after the "=")
        cl.complete_cycle(Direction::Forward, &candidates, 14..16);
        assert_eq!(cl.input(), "set selection=exclusive");
    }

    #[test]
    fn complete_cycle_empty_candidates_is_noop() {
        let mut cl = CommandLineState::new();
        for c in "set zzz".chars() {
            cl.insert_char(c);
        }
        cl.complete_cycle(Direction::Forward, &[], 4..7);
        assert_eq!(cl.input(), "set zzz");
        assert!(!cl.is_completing());
    }

    #[test]
    fn complete_cycle_preserves_candidates_metadata() {
        let mut cl = CommandLineState::new();
        for c in "set scr".chars() {
            cl.insert_char(c);
        }

        let candidates = vec![CompletionCandidate {
            text: CompactString::from("scrolloff"),
            description: Some(CompactString::from("[= 5]")),
            detail: Some(CompactString::from("Min lines above/below cursor")),
        }];
        cl.complete_cycle(Direction::Forward, &candidates, 4..7);

        let (snapshot_candidates, index) = cl.completion_snapshot().unwrap();
        assert_eq!(index, 0);
        assert_eq!(snapshot_candidates.len(), 1);
        assert_eq!(snapshot_candidates[0].description.as_deref(), Some("[= 5]"));
        assert_eq!(
            snapshot_candidates[0].detail.as_deref(),
            Some("Min lines above/below cursor")
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // History prefix filtering
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn history_prefix_filter_matches_set_commands() {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::Ex);
        state.input = "write".to_string();
        state.commit();
        state.begin(CommandLinePrompt::Ex);
        state.input = "set scrolloff=5".to_string();
        state.commit();
        state.begin(CommandLinePrompt::Ex);
        state.input = "quit".to_string();
        state.commit();
        state.begin(CommandLinePrompt::Ex);
        state.input = "set number".to_string();
        state.commit();

        // Type "set " and press Up — should only match "set" entries
        state.begin(CommandLinePrompt::Ex);
        state.input = "set ".to_string();
        state.cursor = 4;

        state.history_prev();
        assert_eq!(
            state.input(),
            "set number",
            "first Up should find most recent 'set' command"
        );

        state.history_prev();
        assert_eq!(
            state.input(),
            "set scrolloff=5",
            "second Up should find older 'set' command"
        );

        // No more "set" entries — should stay on current
        state.history_prev();
        assert_eq!(state.input(), "set scrolloff=5");
    }

    #[test]
    fn history_prefix_filter_navigate_back_with_down() {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::Ex);
        state.input = "write".to_string();
        state.commit();
        state.begin(CommandLinePrompt::Ex);
        state.input = "set scrolloff=5".to_string();
        state.commit();
        state.begin(CommandLinePrompt::Ex);
        state.input = "set number".to_string();
        state.commit();

        state.begin(CommandLinePrompt::Ex);
        state.input = "set ".to_string();
        state.cursor = 4;

        // Navigate to oldest "set" entry
        state.history_prev();
        state.history_prev();
        assert_eq!(state.input(), "set scrolloff=5");

        // Down should go to "set number"
        state.history_next();
        assert_eq!(state.input(), "set number");

        // Down again — restore original input
        state.history_next();
        assert_eq!(state.input(), "set ");
    }

    #[test]
    fn history_prefix_filter_empty_prefix_matches_all() {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::Ex);
        state.input = "write".to_string();
        state.commit();
        state.begin(CommandLinePrompt::Ex);
        state.input = "quit".to_string();
        state.commit();

        // Empty prefix — Up should navigate through all entries
        state.begin(CommandLinePrompt::Ex);
        state.history_prev();
        assert_eq!(state.input(), "quit");
        state.history_prev();
        assert_eq!(state.input(), "write");
    }

    #[test]
    fn history_prefix_filter_no_match_stays_on_input() {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::Ex);
        state.input = "write".to_string();
        state.commit();

        // Type prefix that doesn't match anything
        state.begin(CommandLinePrompt::Ex);
        state.input = "zzz".to_string();
        state.cursor = 3;

        state.history_prev();
        assert_eq!(
            state.input(),
            "zzz",
            "no matching entry: input should not change"
        );
    }

    #[test]
    fn history_prefix_filter_search_history() {
        let mut state = CommandLineState::new();
        state.begin(CommandLinePrompt::SearchForward);
        state.input = "foo".to_string();
        state.commit();
        state.begin(CommandLinePrompt::SearchForward);
        state.input = "bar".to_string();
        state.commit();
        state.begin(CommandLinePrompt::SearchForward);
        state.input = "foobar".to_string();
        state.commit();

        // Type "foo" and press Up — should find "foobar" first
        state.begin(CommandLinePrompt::SearchForward);
        state.input = "foo".to_string();
        state.cursor = 3;

        state.history_prev();
        assert_eq!(state.input(), "foobar");

        state.history_prev();
        assert_eq!(state.input(), "foo");
    }
}
