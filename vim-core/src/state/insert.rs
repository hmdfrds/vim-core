//! Insert mode state tracking.
//!
//! Tracks the state of an insert session including start position,
//! accumulated text for repeat, and entry type.
//! Pattern derived from Neovim's `InsertState` struct (edit.c lines 85-106).
//!
//! # Layering
//!
//! State holds pure data containers with no execution logic. Imports
//! `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`.

use std::num::NonZeroU32;

use crate::primitives::{Offset, ReplacedChar};
use compact_str::CompactString;
use smart_default::SmartDefault;

use crate::primitives::InsertEntryType;

/// Context for block visual insert/change/I/A replication.
///
/// When insert mode is entered from a block visual operation (change, I, or A),
/// the text typed during insert is replicated to all other lines in the block
/// upon exiting insert mode.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockInsertContext {
    /// Number of additional lines below the primary (top) line that need text replication.
    lines_below: usize,
    /// Grapheme column where text should be inserted on each additional line.
    grapheme_col: usize,
    /// Byte offset where cursor should return after exiting block insert mode.
    /// This is the top-left corner of the original block selection.
    cursor_return_offset: Offset,
}

impl BlockInsertContext {
    /// Create a new block insert context.
    #[inline]
    #[must_use]
    pub const fn new(
        lines_below: usize,
        grapheme_col: usize,
        cursor_return_offset: Offset,
    ) -> Self {
        Self {
            lines_below,
            grapheme_col,
            cursor_return_offset,
        }
    }

    /// Get the number of lines below the primary line.
    #[inline]
    #[must_use]
    pub const fn lines_below(&self) -> usize {
        self.lines_below
    }

    /// Get the grapheme column for insertion.
    #[inline]
    #[must_use]
    pub const fn grapheme_col(&self) -> usize {
        self.grapheme_col
    }

    /// Get the cursor return offset.
    #[inline]
    #[must_use]
    pub const fn cursor_return_offset(&self) -> Offset {
        self.cursor_return_offset
    }

    /// Shift `cursor_return_offset` by a signed delta (for external edits
    /// before the block insert region).
    #[inline]
    pub const fn shift_cursor_return_offset(&mut self, delta: isize) {
        let shifted = self.cursor_return_offset.get().saturating_add_signed(delta);
        self.cursor_return_offset = Offset::new(shifted);
    }
}

/// Where an insert started, as Vim's formatting sees it: `Insstart`,
/// `Insstart_textlen` and `Insstart_blank_vcol` in edit.c.
///
/// The `l` and `b` flags of `formatoptions` look at the line the insert
/// started on, and `v` and `b` only break at blanks typed after the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InsertStart {
    /// Line of the insert start (0-based).
    pub line: usize,
    /// Byte column of the insert start on that line.
    pub col: usize,
    /// Display width of that line when the insert started.
    pub textlen: usize,
    /// Display column of the first blank typed on that line, if any.
    pub blank_vcol: Option<usize>,
}

/// The insert start of a cursor other than the primary one, which formats
/// the text it types as if it typed it alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CursorInsertStart {
    /// Where the last insert command left this cursor, after the edits of
    /// every cursor.
    pub head: Offset,
    /// How many lines the cursor is below its insert start. Other cursors
    /// add and remove lines above, so the start line is kept relative.
    pub lines_below: usize,
    /// The insert start, with its line as it was when it was stored.
    pub start: InsertStart,
}

/// State of an active insert session.
///
/// Created when entering insert mode, destroyed when exiting.
/// Tracks information needed for:
/// - Repeat with `.` command
/// - Undo grouping
/// - Insert-mode navigation
#[derive(Debug, Clone, SmartDefault)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InsertState {
    /// Text accumulated during this insert session.
    /// Used for `.` repeat and `^A` (insert previous).
    accumulated_text: CompactString,
    /// How insert mode was entered.
    entry_type: InsertEntryType,
    /// Count for repeating text on exit (e.g., 3i inserts "X" then "XX" more on Esc).
    #[default(NonZeroU32::MIN)]
    count: NonZeroU32,
    /// Length of auto-indentation inserted by o/O commands.
    /// Used to strip trailing whitespace when no text is typed before `<Esc>`.
    auto_indent_len: usize,
    /// Per-newline auto-indent byte lengths in accumulated_text.
    /// One entry per `\n` pushed. Used by dot-repeat to strip original
    /// indent and recompute for the replay context.
    #[default(Vec::new())]
    newline_indent_lens: Vec<usize>,
    /// Block visual insert context for replicating typed text to additional lines.
    block_insert: Option<BlockInsertContext>,
    /// Byte offset where insert mode was entered (for C-u/C-w boundary).
    entry_offset: Option<Offset>,
    /// Stack of original characters replaced in Replace mode.
    ///
    /// When `R` overwrites a character, the original is pushed here.
    /// Backspace in Replace mode pops from this stack and restores the char.
    /// `InsertedAtEol` entries indicate the cursor was past end-of-line (inserted, not replaced).
    ///
    /// Matches Neovim's `replace_stack` (edit.c).
    replaced_chars: Vec<ReplacedChar>,
    /// When `true`, the next cursor movement in insert mode will not break the
    /// undo sequence. Set by Ctrl-G U; cleared after a single movement.
    pub dont_sync_undo: bool,
    /// Set to `true` when any text mutation (insert/delete/replace) occurs
    /// during this insert session. Used at exit to distinguish arrow-only
    /// sessions (no mutations) from deletion-only sessions (mutations but
    /// empty accumulated_text).
    had_text_mutation: bool,
    /// Minimum edit offset seen during this insert session.
    /// Used for mark `[` on exit when an indent/outdent operation (C-d/C-t)
    /// modifies text at a position before the entry_offset.
    min_change_start: Option<Offset>,
    /// Mark '.' override: byte offset (within accumulated_text) of the
    /// start of the last `changed_bytes()` call equivalent.  Operations
    /// that bypass Neovim's `insertchar()` batching (Tab expansion,
    /// Ctrl-E, Ctrl-Y, Ctrl-R register paste) set this to the position
    /// of the last inserted character.  If `None`, the post-hoc backward
    /// walk in `compute_mark_dot_for_insert` is used.
    mark_dot_override_pos: Option<usize>,
    /// Neovim's `arrow_used` equivalent.  Set when `<C-o>` (one-shot
    /// normal mode) is used.  When this is true at exit and no text was
    /// typed, Neovim's `ins_esc` skips `stop_insert` so the `[`/`]`
    /// marks from the one-shot command are preserved.
    arrow_used: bool,
    /// Bracketed paste mode — suppresses mappings, abbreviations, and auto-indent.
    pasting: bool,
    /// Saved indent from `^^D` (OutdentTemporary).
    ///
    /// When `^^D` is used, the current line's leading whitespace is saved here
    /// before being removed. The next newline restores this indent on the new
    /// line and clears it. Cleared on mode exit (InsertState destruction).
    /// Matches Neovim's `can_si_back` + `old_indent` behavior.
    saved_indent: Option<CompactString>,
    /// Insert start for formatting, recorded by the first insert command
    /// and again after the cursor moved (Vim's `stop_arrow()`).
    insert_start: Option<InsertStart>,
    /// Where the last insert command left the cursor, to tell when the
    /// cursor moved in between and the insert start must be recorded again.
    insert_start_cursor: Option<Offset>,
    /// Insert starts of the cursors other than the primary one.
    cursor_starts: Vec<CursorInsertStart>,
}

impl InsertState {
    /// Create a new insert session.
    #[inline]
    #[must_use]
    pub fn new(entry_type: InsertEntryType) -> Self {
        Self::with_count(entry_type, NonZeroU32::MIN)
    }

    /// Create a new insert session with a repeat count.
    #[inline]
    #[must_use]
    pub fn with_count(entry_type: InsertEntryType, count: NonZeroU32) -> Self {
        Self {
            accumulated_text: CompactString::default(),
            entry_type,
            count,
            auto_indent_len: 0,
            newline_indent_lens: Vec::new(),
            block_insert: None,
            entry_offset: None,
            replaced_chars: Vec::new(),
            dont_sync_undo: false,
            had_text_mutation: false,
            min_change_start: None,
            mark_dot_override_pos: None,
            arrow_used: false,
            pasting: false,
            saved_indent: None,
            insert_start: None,
            insert_start_cursor: None,
            cursor_starts: Vec::new(),
        }
    }

    /// Get the entry type.
    #[inline]
    #[must_use]
    pub const fn entry_type(&self) -> InsertEntryType {
        self.entry_type
    }

    /// Get the repeat count (always >= 1).
    #[inline]
    #[must_use]
    pub const fn count(&self) -> NonZeroU32 {
        self.count
    }

    /// Get the auto-indent length (for o/O commands).
    #[inline]
    #[must_use]
    pub const fn auto_indent_len(&self) -> usize {
        self.auto_indent_len
    }

    /// Set the auto-indent length (called when o/O inserts indentation).
    #[inline]
    pub const fn set_auto_indent_len(&mut self, len: usize) {
        self.auto_indent_len = len;
    }

    /// Record the auto-indent byte length for a newline in accumulated_text.
    ///
    /// Called once per newline pushed during insert mode. Used by dot-repeat
    /// to strip original indent and recompute for the replay context.
    #[inline]
    pub fn push_newline_indent_len(&mut self, len: usize) {
        self.newline_indent_lens.push(len);
    }

    /// Per-newline auto-indent byte lengths recorded during this insert session.
    #[inline]
    #[must_use]
    pub fn newline_indent_lens(&self) -> &[usize] {
        &self.newline_indent_lens
    }

    /// Pop the last newline indent length from the stack.
    ///
    /// Used when Replace mode backspace joins a line back (undoing a newline
    /// that was entered in Replace mode). The indent length tells us how many
    /// autoindent characters were pushed to accumulated_text after the newline.
    #[inline]
    pub fn pop_newline_indent_len(&mut self) -> Option<usize> {
        self.newline_indent_lens.pop()
    }

    /// Get block insert context.
    #[inline]
    #[must_use]
    pub const fn block_insert(&self) -> Option<&BlockInsertContext> {
        self.block_insert.as_ref()
    }

    /// Get mutable block insert context.
    #[inline]
    #[must_use]
    pub const fn block_insert_mut(&mut self) -> Option<&mut BlockInsertContext> {
        self.block_insert.as_mut()
    }

    /// Set block insert context.
    #[inline]
    pub const fn set_block_insert(&mut self, ctx: BlockInsertContext) {
        self.block_insert = Some(ctx);
    }

    /// Take block insert context (moves it out).
    #[inline]
    pub const fn take_block_insert(&mut self) -> Option<BlockInsertContext> {
        self.block_insert.take()
    }

    /// Append a character to accumulated text.
    #[inline]
    pub fn push_char(&mut self, c: char) {
        self.accumulated_text.push(c);
    }

    /// Append a string to accumulated text.
    #[inline]
    pub fn push_str(&mut self, s: &str) {
        self.accumulated_text.push_str(s);
    }

    /// Remove the last character from accumulated text (for backspace).
    ///
    /// Does not modify `mark_dot_override_pos` — callers that use this
    /// for backspace should set the override explicitly afterward.
    #[inline]
    pub fn pop_char(&mut self) -> Option<char> {
        self.accumulated_text.pop()
    }

    /// Remove `byte_count` bytes from the end of accumulated text.
    ///
    /// Clamps to the string length and snaps down to a char boundary so
    /// the truncation never splits a multi-byte character.
    #[inline]
    pub fn truncate_tail_bytes(&mut self, byte_count: usize) {
        let len = self.accumulated_text.len();
        let remove = byte_count.min(len);
        let mut new_len = len - remove;
        while new_len > 0 && !self.accumulated_text.is_char_boundary(new_len) {
            new_len -= 1;
        }
        self.accumulated_text.truncate(new_len);
    }

    /// Remove `byte_count` bytes from the start of accumulated text.
    ///
    /// Used when an external edit straddles the entry_offset boundary,
    /// deleting text at the beginning of the insert region.
    #[inline]
    pub fn truncate_head_bytes(&mut self, byte_count: usize) {
        let len = self.accumulated_text.len();
        let remove = byte_count.min(len);
        let mut start = remove;
        while start < len && !self.accumulated_text.is_char_boundary(start) {
            start += 1;
        }
        if start >= len {
            self.accumulated_text.clear();
        } else {
            self.accumulated_text =
                compact_str::CompactString::from(&self.accumulated_text[start..]);
        }
    }

    /// Reset accumulated text for the current repeat block.
    ///
    /// Called when arrow keys break the insert session's repeat chain
    /// (Neovim's `start_arrow`). Text typed after this point becomes
    /// the new repeat block for dot-repeat.
    #[inline]
    pub fn reset_accumulated_text(&mut self) {
        self.accumulated_text.clear();
        self.newline_indent_lens.clear();
        self.mark_dot_override_pos = None;
    }

    /// Get the accumulated text.
    #[inline]
    #[must_use]
    pub fn accumulated_text(&self) -> &str {
        &self.accumulated_text
    }

    /// Set the mark '.' override position (byte offset within accumulated_text).
    ///
    /// Called for operations that bypass `insertchar()` batching in Neovim:
    /// Tab expansion, Ctrl-E/Y, Ctrl-R register paste.  These each produce
    /// their own `changed_bytes()` call, so the last character's position
    /// in accumulated_text determines mark '.'.
    #[inline]
    pub const fn set_mark_dot_override(&mut self, pos: usize) {
        self.mark_dot_override_pos = Some(pos);
    }

    /// Clear the mark '.' override so the backward walk is used.
    #[inline]
    pub const fn clear_mark_dot_override(&mut self) {
        self.mark_dot_override_pos = None;
    }

    /// Get the mark '.' override position, if set.
    #[inline]
    #[must_use]
    pub const fn mark_dot_override_pos(&self) -> Option<usize> {
        self.mark_dot_override_pos
    }

    /// Clear state for reuse.
    pub fn clear(&mut self) {
        self.accumulated_text.clear();
        self.entry_offset = None;
        self.replaced_chars.clear();
        self.newline_indent_lens.clear();
        self.had_text_mutation = false;
        self.min_change_start = None;
        self.mark_dot_override_pos = None;
        self.arrow_used = false;
        self.saved_indent = None;
    }

    /// Clear the replace stack without affecting other insert state.
    ///
    /// Used by external-edit reconciliation when the edit overlaps the
    /// insert region and the replaced chars are no longer valid.
    #[inline]
    pub fn clear_replaced_chars(&mut self) {
        self.replaced_chars.clear();
    }

    /// Push an original character onto the replace stack.
    ///
    /// `InsertedAtEol` = cursor was past EOL (character was inserted, not replaced).
    #[inline]
    pub fn push_replaced(&mut self, ch: ReplacedChar) {
        self.replaced_chars.push(ch);
    }

    /// Pop the last original character from the replace stack.
    #[inline]
    pub fn pop_replaced(&mut self) -> Option<ReplacedChar> {
        self.replaced_chars.pop()
    }

    /// Peek at the last original character on the replace stack (without popping).
    ///
    /// Used by the executor to read the value and pass via InsertContext,
    /// while the engine pops after pipeline execution.
    #[inline]
    #[must_use]
    pub fn peek_replaced(&self) -> Option<ReplacedChar> {
        self.replaced_chars.last().copied()
    }

    /// Check if the replace stack has entries.
    #[inline]
    #[must_use]
    pub const fn has_replaced(&self) -> bool {
        !self.replaced_chars.is_empty()
    }

    /// Set the entry offset (byte offset where insert mode was entered).
    #[inline]
    pub const fn set_entry_offset(&mut self, offset: Offset) {
        self.entry_offset = Some(offset);
    }

    /// Get the entry offset.
    #[inline]
    #[must_use]
    pub const fn entry_offset(&self) -> Option<Offset> {
        self.entry_offset
    }

    /// Mark that a text mutation occurred during this insert session.
    #[inline]
    pub const fn set_had_text_mutation(&mut self) {
        self.had_text_mutation = true;
    }

    /// Check whether any text mutation occurred during this insert session.
    #[inline]
    #[must_use]
    pub const fn had_text_mutation(&self) -> bool {
        self.had_text_mutation
    }

    /// Mark that `<C-o>` (one-shot normal) was used during this session.
    ///
    /// Equivalent to Neovim's `arrow_used` flag.  When set and no text
    /// was typed, `ins_esc` / insert-exit skips mark `[`/`]` override.
    #[inline]
    pub const fn set_arrow_used(&mut self) {
        self.arrow_used = true;
    }

    /// Whether `<C-o>` was used during this insert session.
    #[inline]
    #[must_use]
    pub const fn arrow_used(&self) -> bool {
        self.arrow_used
    }

    /// The insert start used by formatting, if recorded.
    #[inline]
    #[must_use]
    pub const fn insert_start(&self) -> Option<InsertStart> {
        self.insert_start
    }

    /// Mutable access to the recorded insert start.
    #[inline]
    pub const fn insert_start_mut(&mut self) -> Option<&mut InsertStart> {
        self.insert_start.as_mut()
    }

    /// Record the insert start.
    #[inline]
    pub const fn set_insert_start(&mut self, start: InsertStart) {
        self.insert_start = Some(start);
    }

    /// Where the last insert command left the cursor.
    #[inline]
    #[must_use]
    pub const fn insert_start_cursor(&self) -> Option<Offset> {
        self.insert_start_cursor
    }

    /// Remember where the last insert command left the cursor.
    #[inline]
    pub const fn set_insert_start_cursor(&mut self, cursor: Offset) {
        self.insert_start_cursor = Some(cursor);
    }

    /// The insert start of the cursor other than the primary one that the
    /// last insert command left at `head`, with its line moved to `line`
    /// less the lines the cursor was below it. `None` when no insert command
    /// left a cursor there, as after the cursor moved (Vim's `stop_arrow()`).
    #[must_use]
    pub fn cursor_start(&self, head: Offset, line: usize) -> Option<InsertStart> {
        self.cursor_starts
            .iter()
            .find(|c| c.head == head)
            .map(|c| InsertStart {
                line: line.saturating_sub(c.lines_below),
                ..c.start
            })
    }

    /// Replace the insert starts of the cursors other than the primary one.
    #[inline]
    pub fn set_cursor_starts(&mut self, starts: Vec<CursorInsertStart>) {
        self.cursor_starts = starts;
    }

    /// Whether bracketed paste mode is active.
    #[inline]
    #[must_use]
    pub const fn pasting(&self) -> bool {
        self.pasting
    }

    /// Set bracketed paste mode.
    #[inline]
    pub const fn set_pasting(&mut self, pasting: bool) {
        self.pasting = pasting;
    }

    /// Get the saved indent (from `^^D` / OutdentTemporary).
    #[inline]
    #[must_use]
    pub const fn saved_indent(&self) -> Option<&CompactString> {
        self.saved_indent.as_ref()
    }

    /// Save the current indent for later restoration by the next newline.
    ///
    /// Called by `OutdentTemporary` (`^^D`) before removing all indentation.
    #[inline]
    pub fn set_saved_indent(&mut self, indent: CompactString) {
        self.saved_indent = Some(indent);
    }

    /// Take the saved indent (moves it out, leaving `None`).
    ///
    /// Called by the newline handler to restore indent and clear the saved state.
    #[inline]
    pub const fn take_saved_indent(&mut self) -> Option<CompactString> {
        self.saved_indent.take()
    }

    /// Clear the saved indent without returning it.
    #[inline]
    pub fn clear_saved_indent(&mut self) {
        self.saved_indent = None;
    }

    /// Record the minimum edit offset (for C-d/C-t indent operations
    /// that edit at line start, before the entry offset).
    #[inline]
    pub const fn track_change_start(&mut self, offset: Offset) {
        self.min_change_start = Some(match self.min_change_start {
            Some(existing) => existing.min(offset),
            None => offset,
        });
    }

    /// Get the minimum change start offset, if any indent/outdent
    /// operation moved it before the entry offset.
    #[inline]
    #[must_use]
    pub const fn min_change_start(&self) -> Option<Offset> {
        self.min_change_start
    }

    /// Directly set the minimum change start offset.
    ///
    /// Used by external-edit reconciliation to shift this position when
    /// an edit occurs before the insert region.
    #[inline]
    pub const fn set_min_change_start(&mut self, offset: Offset) {
        self.min_change_start = Some(offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    #[test]
    fn test_insert_state_new() {
        let state = InsertState::new(InsertEntryType::AfterCursor);
        assert_eq!(state.entry_type(), InsertEntryType::AfterCursor);
        assert_eq!(state.accumulated_text(), "");
    }

    #[test]
    fn test_accumulate_text() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        state.push_char('h');
        state.push_char('e');
        state.push_str("llo");
        assert_eq!(state.accumulated_text(), "hello");
    }

    #[test]
    fn test_pop_char() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        state.push_str("abc");
        assert_eq!(state.pop_char(), Some('c'));
        assert_eq!(state.accumulated_text(), "ab");
    }

    #[test]
    fn test_entry_types() {
        // Verify all entry types are distinct
        let types = [
            InsertEntryType::BeforeCursor,
            InsertEntryType::FirstNonBlank,
            InsertEntryType::AfterCursor,
            InsertEntryType::EndOfLine,
            InsertEntryType::NewLineBelow,
            InsertEntryType::NewLineAbove,
            InsertEntryType::SubstituteChar,
            InsertEntryType::SubstituteLine,
            InsertEntryType::ChangeOperator,
            InsertEntryType::ReplaceMode,
        ];
        for (i, t1) in types.iter().enumerate() {
            for (j, t2) in types.iter().enumerate() {
                if i == j {
                    assert_eq!(t1, t2);
                } else {
                    assert_ne!(t1, t2);
                }
            }
        }
    }

    #[test]
    fn test_default() {
        let state = InsertState::default();
        assert_eq!(state.entry_type(), InsertEntryType::BeforeCursor);
    }

    #[test]
    fn test_with_count_stores_value() {
        let state = InsertState::with_count(InsertEntryType::BeforeCursor, NonZeroU32::MIN);
        assert_eq!(state.count(), NonZeroU32::MIN);

        let state2 =
            InsertState::with_count(InsertEntryType::BeforeCursor, NonZeroU32::new(5).unwrap());
        assert_eq!(state2.count(), NonZeroU32::new(5).unwrap());
    }

    #[test]
    fn test_block_insert_context_roundtrip() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        assert!(state.block_insert().is_none());

        let ctx = BlockInsertContext::new(3, 5, Offset::new(42));
        state.set_block_insert(ctx);

        let got = state.block_insert().unwrap();
        assert_eq!(got.lines_below(), 3);
        assert_eq!(got.grapheme_col(), 5);
        assert_eq!(got.cursor_return_offset(), Offset::new(42));

        let taken = state.take_block_insert();
        assert!(taken.is_some());
        assert!(state.block_insert().is_none());
    }

    #[test]
    fn test_replace_stack() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        assert!(!state.has_replaced());

        state.push_replaced(ReplacedChar::Replaced('a'));
        state.push_replaced(ReplacedChar::InsertedAtEol); // past-EOL insert
        state.push_replaced(ReplacedChar::Replaced('b'));

        assert!(state.has_replaced());
        assert_eq!(state.pop_replaced(), Some(ReplacedChar::Replaced('b')));
        assert_eq!(state.pop_replaced(), Some(ReplacedChar::InsertedAtEol));
        assert_eq!(state.pop_replaced(), Some(ReplacedChar::Replaced('a')));
        assert_eq!(state.pop_replaced(), None);
        assert!(!state.has_replaced());
    }

    #[test]
    fn test_clear_resets_transient() {
        let mut state = InsertState::new(InsertEntryType::AfterCursor);
        state.push_str("hello");
        state.set_entry_offset(Offset::new(42));
        state.push_replaced(ReplacedChar::Replaced('x'));

        state.clear();

        assert_eq!(state.accumulated_text(), "");
        assert!(state.entry_offset().is_none());
        assert!(!state.has_replaced());
        // entry_type preserved (it defines the session)
        assert_eq!(state.entry_type(), InsertEntryType::AfterCursor);
    }

    #[test]
    fn test_entry_offset_roundtrip() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        assert!(state.entry_offset().is_none());

        state.set_entry_offset(Offset::new(100));
        assert_eq!(state.entry_offset(), Some(Offset::new(100)));
    }

    // ── saved_indent (^^D) tests ─────────────────────────────────────

    #[test]
    fn test_saved_indent_initially_none() {
        let state = InsertState::new(InsertEntryType::BeforeCursor);
        assert!(state.saved_indent().is_none());
    }

    #[test]
    fn test_saved_indent_set_and_get() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        state.set_saved_indent(CompactString::from("    "));
        assert_eq!(state.saved_indent().map(|s| s.as_str()), Some("    "));
    }

    #[test]
    fn test_saved_indent_take() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        state.set_saved_indent(CompactString::from("\t\t"));
        let taken = state.take_saved_indent();
        assert_eq!(taken.as_deref(), Some("\t\t"));
        assert!(state.saved_indent().is_none());
    }

    #[test]
    fn test_saved_indent_clear() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        state.set_saved_indent(CompactString::from("  "));
        state.clear_saved_indent();
        assert!(state.saved_indent().is_none());
    }

    #[test]
    fn test_saved_indent_cleared_by_clear() {
        let mut state = InsertState::new(InsertEntryType::AfterCursor);
        state.set_saved_indent(CompactString::from("        "));
        state.clear();
        assert!(state.saved_indent().is_none());
    }

    #[test]
    fn test_saved_indent_default_is_none() {
        let state = InsertState::default();
        assert!(state.saved_indent().is_none());
    }

    #[test]
    fn test_saved_indent_overwrite() {
        let mut state = InsertState::new(InsertEntryType::BeforeCursor);
        state.set_saved_indent(CompactString::from("    "));
        state.set_saved_indent(CompactString::from("\t"));
        assert_eq!(state.saved_indent().map(|s| s.as_str()), Some("\t"));
    }

    // ── Replace stack LineBoundary tests ─────────────────────────────

    #[test]
    fn test_replace_stack_line_boundary() {
        let mut state = InsertState::new(InsertEntryType::ReplaceMode);
        state.push_replaced(ReplacedChar::Replaced('a'));
        state.push_replaced(ReplacedChar::LineBoundary);
        state.push_replaced(ReplacedChar::Replaced('b'));

        assert_eq!(state.pop_replaced(), Some(ReplacedChar::Replaced('b')));
        assert_eq!(state.pop_replaced(), Some(ReplacedChar::LineBoundary));
        assert_eq!(state.pop_replaced(), Some(ReplacedChar::Replaced('a')));
        assert_eq!(state.pop_replaced(), None);
    }

    #[test]
    fn test_pop_newline_indent_len() {
        let mut state = InsertState::new(InsertEntryType::ReplaceMode);
        state.push_newline_indent_len(4);
        state.push_newline_indent_len(2);
        assert_eq!(state.pop_newline_indent_len(), Some(2));
        assert_eq!(state.pop_newline_indent_len(), Some(4));
        assert_eq!(state.pop_newline_indent_len(), None);
    }

    #[test]
    fn test_line_boundary_with_accumulated_text() {
        let mut state = InsertState::new(InsertEntryType::ReplaceMode);
        // Simulate: type "ab", Enter (newline + 4 spaces indent), type "cd"
        state.push_char('a');
        state.push_char('b');
        state.push_replaced(ReplacedChar::Replaced('x'));
        state.push_replaced(ReplacedChar::Replaced('y'));
        // Enter: push newline + indent to accumulated, LineBoundary to stack
        state.push_char('\n');
        state.push_char(' ');
        state.push_char(' ');
        state.push_char(' ');
        state.push_char(' ');
        state.push_newline_indent_len(4);
        state.push_replaced(ReplacedChar::LineBoundary);
        // Type "cd"
        state.push_char('c');
        state.push_char('d');
        state.push_replaced(ReplacedChar::Replaced('z'));
        state.push_replaced(ReplacedChar::Replaced('w'));

        // Now simulate backspace of "cd" then the line boundary
        assert_eq!(state.pop_replaced(), Some(ReplacedChar::Replaced('w')));
        state.pop_char(); // 'd'
        assert_eq!(state.pop_replaced(), Some(ReplacedChar::Replaced('z')));
        state.pop_char(); // 'c'
                          // Hit LineBoundary: pop indent chars + newline
        assert_eq!(state.pop_replaced(), Some(ReplacedChar::LineBoundary));
        let indent_len = state.pop_newline_indent_len().unwrap_or(0);
        assert_eq!(indent_len, 4);
        for _ in 0..indent_len {
            state.pop_char(); // 4 spaces
        }
        state.pop_char(); // '\n'
        assert_eq!(state.accumulated_text(), "ab");
    }
}
