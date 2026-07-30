#![allow(dead_code)]
//! TestDocument - In-memory Document implementation for testing.
//!
//! This implements the Document trait for testing purposes.
//! Composed of TextBuffer (edits) and CursorState (cursor/selection).

use std::collections::HashMap;
use vim_core::document::Document;
use vim_core::execution::VimTextDocument;
use vim_core::primitives::{Column, LineNumber, Offset, Position};

use super::cursor::CursorState;
use super::edits::TextBuffer;

/// Represents a single undo entry (snapshot of text + cursor).
#[derive(Debug, Clone)]
struct UndoEntry {
    text: String,
    cursor: usize,
    /// Position of the first edit in this undo group (for proper cursor restoration).
    first_edit_offset: Option<usize>,
}

/// In-memory document for testing.
///
/// Composes TextBuffer and CursorState for clean separation.
#[derive(Debug, Clone)]
pub struct TestDocument {
    /// Text content and line indexing.
    buffer: TextBuffer,
    /// Cursor and selection state.
    cursor: CursorState,
    /// Register storage for test verification (text, regtype).
    registers: HashMap<char, (String, String)>,
    /// Mark storage (name -> byte offset).
    marks: HashMap<char, usize>,
    /// Undo stack (snapshots before changes).
    undo_stack: Vec<UndoEntry>,
    /// Redo stack (snapshots for redo).
    redo_stack: Vec<UndoEntry>,
    /// Pending undo group snapshot (captured at BeginUndoGroup).
    pending_group: Option<UndoEntry>,
    /// Last error message (for E353, E20, etc.).
    errmsg: Option<String>,
    /// Jump list for <C-o> and <C-i> navigation.
    jump_list: Vec<usize>,
    /// Current position in jump list (for <C-o>/<C-i> navigation).
    jump_list_pos: usize,
    /// Saved "present" position for <C-i> when navigating backward.
    /// Set on first <C-o> from the end, cleared on push_jump_list.
    jump_list_saved_pos: Option<usize>,
    /// Track the first edit offset during an active undo group.
    pending_first_edit: Option<usize>,
    /// When true, suppresses first-edit tracking (used by o/O undo groups).
    suppress_first_edit: bool,
    /// Saved line number for `U` command (0-indexed).
    /// Set when the first change happens on a line. Subsequent changes on the
    /// same line don't update it; changes on a different line replace it.
    /// Mirrors Neovim's `b_u_line_lnum`.
    u_line_lnum: Option<usize>,
    /// Saved line content for `U` command.
    /// The original content of line `u_line_lnum` BEFORE any changes were
    /// made to it.  Mirrors Neovim's `b_u_line_ptr`.
    u_line_ptr: Option<String>,
    /// Saved column for `U` command (cursor column when line was first saved).
    u_line_colnr: usize,
    /// VimTextDocument mirror — receives the same text mutations as `buffer`.
    /// After every mutation, we assert both backends agree on text, len, and
    /// line_count. This exercises VimTextDocument through every fidelity test.
    vt_mirror: VimTextDocument,
}

impl TestDocument {
    /// Create a new TestDocument from text with cursor at position.
    pub fn new(text: impl Into<String>, cursor_pos: (usize, usize)) -> Self {
        let buffer = TextBuffer::new(text);

        // Convert (line, col) to byte offset.
        //
        // The oracle Lua script treats `col` as a CHARACTER INDEX and
        // converts via `vim.str_byteindex(line, col)`.  When the character
        // index exceeds the character count the pcall fails and Lua falls
        // back to `math.min(col, #line)` (raw byte offset, clamped).
        //
        // We mirror that logic here:
        //   1. Count characters in the line.
        //   2. If col < char_count  → treat as character index, convert to byte offset.
        //   3. If col == char_count → nvim's str_byteindex returns byte-length,
        //      nvim_win_set_cursor clamps to last character.
        //   4. If col > char_count  → str_byteindex errors, Lua falls back to
        //      raw byte offset min(col, #line), then nvim clamps/snaps.
        let cursor_offset = if cursor_pos.0 < buffer.line_starts.len() {
            let line_start = buffer.line_starts[cursor_pos.0];
            let line_end = if cursor_pos.0 + 1 < buffer.line_starts.len() {
                buffer.line_starts[cursor_pos.0 + 1]
            } else {
                buffer.text.len()
            };
            let line_text = &buffer.text[line_start..line_end];
            let line_content = line_text.trim_end_matches('\n');
            let col = cursor_pos.1;

            let char_count = line_content.chars().count();

            let col_byte = if col < char_count {
                // Character index → byte offset
                line_content
                    .char_indices()
                    .nth(col)
                    .map(|(byte_idx, _)| byte_idx)
                    .unwrap_or(0)
            } else if col == char_count && char_count > 0 {
                // One-past-end: nvim clamps to last character
                line_content
                    .char_indices()
                    .last()
                    .map(|(byte_idx, _)| byte_idx)
                    .unwrap_or(0)
            } else {
                // col > char_count: raw byte offset fallback, clamped
                let max_byte = line_content.len().saturating_sub(1);
                let mut b = col.min(max_byte);
                // Snap to nearest valid UTF-8 char boundary
                while b > 0 && !line_content.is_char_boundary(b) {
                    b -= 1;
                }
                b
            };

            line_start + col_byte
        } else {
            0
        };

        // In the Neovim oracle, the buffer starts empty and the test text
        // is loaded via `nvim_buf_set_lines`.  That API call is itself a
        // "change" that triggers `u_saveline` for the first line — saving
        // the empty-buffer state.  We mirror this by pre-populating
        // `u_line_lnum` / `u_line_ptr` with the empty line that existed
        // before the text was loaded.  This ensures `U` on a freshly
        // loaded buffer can undo all the way back to empty, matching the
        // oracle.
        let vt_mirror = VimTextDocument::new(&buffer.text);
        Self {
            buffer,
            cursor: CursorState::new(cursor_offset),
            registers: HashMap::new(),
            marks: HashMap::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            pending_group: None,
            errmsg: None,
            jump_list: Vec::new(),
            jump_list_pos: 0,
            jump_list_saved_pos: None,
            pending_first_edit: None,
            suppress_first_edit: false,
            u_line_lnum: Some(0),
            u_line_ptr: Some(String::new()),
            u_line_colnr: 0,
            vt_mirror,
        }
    }

    /// Create from text with cursor at start.
    pub fn from_text(text: impl Into<String>) -> Self {
        Self::new(text, (0, 0))
    }

    /// Create an empty document.
    pub fn empty() -> Self {
        Self::from_text("")
    }

    /// Create a document with N lines of numbered text.
    pub fn lines(count: usize) -> Self {
        let text = (1..=count)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        Self::from_text(text)
    }

    /// Create from lines slice.
    pub fn from_lines(lines: &[&str]) -> Self {
        Self::from_text(lines.join("\n"))
    }

    /// Get the text (consuming self).
    pub fn into_text(self) -> String {
        self.buffer.text
    }

    // === Cursor methods ===

    /// Get cursor offset.
    pub fn cursor_offset(&self) -> usize {
        self.cursor.offset
    }

    /// Set cursor offset.
    pub fn set_cursor_offset(&mut self, offset: usize) {
        self.cursor.set_offset(offset, self.buffer.len());
    }

    /// Get cursor position as (line, col).
    pub fn cursor_position(&self) -> (usize, usize) {
        self.cursor.to_position(&self.buffer.line_starts)
    }

    // === Selection methods ===

    /// Set selection.
    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.cursor.set_selection(anchor, head);
    }

    /// Clear selection.
    pub fn clear_selection(&mut self) {
        self.cursor.clear_selection();
    }

    /// Get current selection as SelectionRange if in visual mode.
    pub fn selection(&self) -> Option<vim_core::primitives::SelectionRange> {
        match (self.cursor.selection_anchor, self.cursor.selection_head) {
            (Some(anchor), Some(head)) => Some(vim_core::primitives::SelectionRange::new(
                vim_core::primitives::Offset::new(anchor),
                vim_core::primitives::Offset::new(head),
            )),
            _ => None,
        }
    }

    // === Register methods ===

    /// Set a register value with regtype.
    /// For uppercase registers (A-Z), appends to existing content in the lowercase version.
    pub fn set_register(&mut self, name: char, text: String, regtype: String) {
        if name.is_ascii_uppercase() {
            let lowercase_key = name.to_ascii_lowercase();
            // Append to existing register content, or create new
            let entry = self
                .registers
                .entry(lowercase_key)
                .or_insert_with(|| (String::new(), regtype.clone()));
            entry.0.push_str(&text);
            entry.1 = regtype.clone(); // Update regtype per Vim spec
                                       // Also update unnamed register with full appended content
            let unnamed_text = entry.0.clone();
            let unnamed_regtype = entry.1.clone();
            self.registers.insert('"', (unnamed_text, unnamed_regtype));
        } else {
            self.registers.insert(name, (text, regtype));
        }
    }

    /// Get a register value.
    pub fn get_register(&self, name: char) -> Option<(&String, &String)> {
        self.registers.get(&name).map(|(t, r)| (t, r))
    }

    // === Mark methods ===

    /// Set a mark at a position.
    pub fn set_mark(&mut self, name: char, offset: usize) {
        self.marks.insert(name, offset);
    }

    /// Get a mark position.
    pub fn get_mark(&self, name: char) -> Option<usize> {
        self.marks.get(&name).copied()
    }

    // === Error message methods ===

    /// Set the last error message.
    pub fn set_errmsg(&mut self, msg: String) {
        self.errmsg = Some(msg);
    }

    // === Jump list methods ===

    /// Push current position to jump list.
    /// This is called before jumping to a mark or search result.
    pub fn push_jump_list(&mut self, offset: usize) {
        // Don't push duplicates of the last entry
        if self.jump_list.last() == Some(&offset) {
            return;
        }
        // Global dedup: remove any existing entry with the same offset
        // (matches Neovim behavior and the engine's JumpList::push).
        let mut i = 0;
        while i < self.jump_list.len() {
            if self.jump_list[i] == offset {
                self.jump_list.remove(i);
                if i < self.jump_list_pos {
                    self.jump_list_pos = self.jump_list_pos.saturating_sub(1);
                }
            } else {
                i += 1;
            }
        }
        self.jump_list.push(offset);
        self.jump_list_pos = self.jump_list.len();
        self.jump_list_saved_pos = None;
    }

    /// Jump to older position in jump list (<C-o>).
    /// Returns the offset to jump to, if available.
    pub fn jump_older(&mut self) -> Option<usize> {
        if self.jump_list_pos > 0 {
            self.jump_list_pos -= 1;
            self.jump_list.get(self.jump_list_pos).copied()
        } else {
            None
        }
    }

    /// Save the current cursor position for jump-back navigation.
    /// Called once before a series of jump_older calls when we're at the
    /// "present" position (end of jump list). This ensures <C-i> can return.
    pub fn save_position_for_jump_back(&mut self) {
        // Only save once when at the actual end of the list
        if self.jump_list_pos == self.jump_list.len() && self.jump_list_saved_pos.is_none() {
            let current = self.cursor.offset;
            self.jump_list_saved_pos = Some(current);
        }
    }

    /// Jump to newer position in jump list (<C-i>).
    /// Returns the offset to jump to, if available.
    /// This variant can return the saved "present" position when at the end.
    pub fn jump_newer(&mut self) -> Option<usize> {
        if self.jump_list_pos + 1 < self.jump_list.len() {
            // Navigate within regular entries
            self.jump_list_pos += 1;
            self.jump_list.get(self.jump_list_pos).copied()
        } else if self.jump_list_pos < self.jump_list.len() {
            // At the last regular entry — return saved "present" position if available
            if let Some(saved) = self.jump_list_saved_pos.take() {
                self.jump_list_pos = self.jump_list.len(); // Move past the end
                Some(saved)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Jump to newer position without using saved position.
    /// Used for count-based `<C-i>` (e.g., `2<C-i>`) where Neovim only
    /// navigates regular jump list entries.
    pub fn jump_newer_no_saved(&mut self) -> Option<usize> {
        if self.jump_list_pos + 1 < self.jump_list.len() {
            self.jump_list_pos += 1;
            self.jump_list.get(self.jump_list_pos).copied()
        } else {
            None
        }
    }

    // === Edit methods (delegate to buffer) ===

    /// Save a line for the `U` command, mirroring Neovim's `u_saveline()`.
    ///
    /// Called before any text modification. If the change is on the same line
    /// that's already saved, this is a no-op. If it's a different line, the
    /// old save is replaced with the new line's content.
    fn u_saveline(&mut self, change_offset: usize) {
        let change_line = Self::line_of_offset_static(&self.buffer.text, change_offset);
        if self.u_line_lnum == Some(change_line) {
            // Line already saved — nothing to do.
            return;
        }
        // Save the new line's content before modification.
        let line_content = Self::line_content_at(&self.buffer.text, change_line)
            .unwrap_or("")
            .to_string();
        self.u_line_lnum = Some(change_line);
        self.u_line_ptr = Some(line_content);
        self.u_line_colnr = if self.cursor_line_number() == change_line {
            // Save cursor column within the line
            let line_start = self.line_start_offset(change_line);
            self.cursor.offset.saturating_sub(line_start)
        } else {
            0
        };
    }

    /// Compute 0-indexed line number for a byte offset (static version).
    fn line_of_offset_static(text: &str, offset: usize) -> usize {
        let clamped = offset.min(text.len());
        text[..clamped].bytes().filter(|&b| b == b'\n').count()
    }

    /// Insert text at offset.
    pub fn insert(&mut self, offset: usize, text: &str) {
        self.u_saveline(offset);
        self.track_first_edit(offset);
        self.buffer.insert(offset, text);
    }

    /// Delete range.
    pub fn delete(&mut self, range: std::ops::Range<usize>) {
        // Neovim clears the U-line saved state after linewise delete (`dd`)
        // and multi-line change.  We approximate this by clearing U-line
        // whenever a delete removes a newline (indicating a line-crossing or
        // linewise operation).
        let deleted_text = &self.buffer.text[range.start..range.end.min(self.buffer.text.len())];
        if deleted_text.contains('\n') {
            self.u_line_lnum = None;
            self.u_line_ptr = None;
        } else {
            self.u_saveline(range.start);
        }
        self.track_first_edit(range.start);
        self.buffer.delete(range);
    }

    /// Apply a simple text edit for testing.
    pub fn apply_delete(&mut self, start: usize, end: usize) {
        self.buffer.delete(start..end);
    }

    /// Apply an insert at offset.
    pub fn apply_insert(&mut self, offset: usize, text: &str) {
        self.buffer.insert(offset, text);
    }

    /// Apply a replace (delete + insert).
    pub fn apply_replace(&mut self, start: usize, end: usize, text: &str) {
        self.buffer.replace(start..end, text);
    }

    // === Undo/Redo methods ===

    /// Begin an undo group (capture snapshot).
    ///
    /// When `force_entry_cursor` is true, first-edit tracking is suppressed
    /// so undo will use the pre-command cursor position. Used by o/O commands.
    pub fn begin_undo_group(&mut self, force_entry_cursor: bool) {
        if self.pending_group.is_none() {
            self.pending_group = Some(UndoEntry {
                text: self.buffer.text.clone(),
                cursor: self.cursor.offset,
                first_edit_offset: None,
            });
            self.pending_first_edit = None;
            self.suppress_first_edit = force_entry_cursor;
        }
    }

    /// End an undo group (push snapshot to undo stack).
    pub fn end_undo_group(&mut self) {
        if let Some(mut entry) = self.pending_group.take() {
            entry.first_edit_offset = self.pending_first_edit.take();
            self.undo_stack.push(entry);
            self.redo_stack.clear();
            self.suppress_first_edit = false;
        }
    }

    /// Track the minimum edit offset during an active undo group.
    /// Uses minimum to ensure undo cursor goes to the topmost edit position,
    /// which matches Neovim's behavior for multi-line operations like block visual.
    fn track_first_edit(&mut self, offset: usize) {
        if self.pending_group.is_some() && !self.suppress_first_edit {
            match self.pending_first_edit {
                None => self.pending_first_edit = Some(offset),
                Some(current) => {
                    if offset < current {
                        self.pending_first_edit = Some(offset);
                    }
                }
            }
        }
    }

    /// Compute undo/redo cursor position with nvim-compatible adjustment.
    ///
    /// Uses `first_edit_offset` when available and valid, otherwise `entry_cursor`.
    /// When the chosen position lands on `\n` at end of a non-empty line,
    /// adjusts to a valid normal-mode position:
    /// - If `entry_cursor` is on the next line, use it (linewise delete undo)
    /// - Otherwise, back up to last char on the line (insert undo)
    fn compute_undo_cursor(text: &str, first_edit: Option<usize>, entry_cursor: usize) -> usize {
        let max = if text.is_empty() {
            0
        } else {
            vim_core::primitives::text_util::prev_char_boundary(text, text.len())
        };

        if let Some(edit_pos) = first_edit {
            let clamped = edit_pos.min(max);
            // If edit position lands on a non-newline, use it directly
            if clamped < text.len() && text.as_bytes()[clamped] != b'\n' {
                return clamped;
            }
            // Edit position is on \n at end of a non-empty line.
            // Check if entry_cursor points to valid content on/after the next line.
            let next_line_start = clamped + 1;
            if entry_cursor >= next_line_start && entry_cursor <= max {
                // entry_cursor is on the restored line (linewise delete undo)
                return entry_cursor;
            }
            // Otherwise back up to last char before the \n (insert undo)
            if clamped > 0 && (clamped == 0 || text.as_bytes()[clamped - 1] != b'\n') {
                return clamped - 1;
            }
            return clamped;
        }

        // No first_edit_offset — use entry cursor
        entry_cursor.min(max)
    }

    /// Undo last change (restore from undo stack).
    pub fn undo(&mut self, count: u32) {
        for _ in 0..count {
            if let Some(entry) = self.undo_stack.pop() {
                // Save current state to redo stack.
                // Preserve first_edit_offset so redo can also restore cursor
                // to the change position (Vim: both u and Ctrl-R land on
                // the first changed character).
                self.redo_stack.push(UndoEntry {
                    text: self.buffer.text.clone(),
                    cursor: entry.cursor,
                    first_edit_offset: entry.first_edit_offset,
                });
                // Restore text from undo stack
                self.buffer = TextBuffer::new(entry.text);
                self.vt_mirror.set_text(self.buffer.text.clone());
                let max = self.buffer.len();
                // Compute undo cursor: prefer first_edit_offset if valid,
                // fall back to entry cursor. Handles \n avoidance.
                let adjusted = Self::compute_undo_cursor(
                    &self.buffer.text,
                    entry.first_edit_offset,
                    entry.cursor,
                );
                self.cursor.set_offset(adjusted, max);
            }
        }
    }

    /// Undo all changes (for U command simulation).
    ///
    /// In Neovim, `U` undoes all changes on the current line since the cursor
    /// entered it.  For the test harness, the oracle's buffer was loaded via
    /// `nvim_buf_set_lines` (which creates an undo entry), so `U` on a
    /// freshly loaded buffer can undo back to the empty state.
    ///
    /// We simulate this by undoing all regular undo entries, then also undoing
    /// the implicit initial buffer load (setting text to empty).
    /// Undo all changes on the current line (U command simulation).
    pub fn undo_line(&mut self) {
        // Mirrors Neovim's u_undoline():
        // - Operates on the SAVED line (u_line_lnum), not the current cursor line.
        // - Replaces the saved line's current content with the saved original.
        // - Swaps saved content (so pressing U again undoes the U).
        // - Moves cursor to the saved line at the saved column.
        // - Creates a new undo entry (so `u` can undo the U).

        let target_line = match self.u_line_lnum {
            Some(l) => l,
            None => return, // Nothing saved — beep
        };
        let saved_content = match self.u_line_ptr.take() {
            Some(c) => c,
            None => return,
        };

        // Get current content of the target line.
        let current_content = Self::line_content_at(&self.buffer.text, target_line)
            .unwrap_or("")
            .to_string();

        // Push current state as undo entry (makes U undoable via `u`).
        let line_start = Self::line_start_offset_in(&self.buffer.text, target_line);
        self.undo_stack.push(UndoEntry {
            text: self.buffer.text.clone(),
            cursor: self.cursor.offset,
            first_edit_offset: Some(line_start),
        });
        self.redo_stack.clear();

        // Replace the target line with saved content.
        let line_end = line_start + current_content.len();
        let mut new_text = String::with_capacity(
            self.buffer.text.len() - current_content.len() + saved_content.len(),
        );
        new_text.push_str(&self.buffer.text[..line_start]);
        new_text.push_str(&saved_content);
        new_text.push_str(&self.buffer.text[line_end..]);

        // Swap: save old content so pressing U again swaps back.
        self.u_line_ptr = Some(current_content);
        // u_line_colnr: if cursor was on the target line, save current col.
        let old_colnr = self.u_line_colnr;
        if self.cursor_line_number() == target_line {
            self.u_line_colnr = self.cursor.offset.saturating_sub(line_start);
        }

        self.buffer = TextBuffer::new(new_text);
        self.vt_mirror.set_text(self.buffer.text.clone());
        let max = self.buffer.len();

        // Move cursor to the saved line at the saved column.
        let new_line_start = self.line_start_offset(target_line);
        let cursor_offset = (new_line_start + old_colnr).min(max);
        self.cursor.set_offset(cursor_offset, max);
    }

    /// Get the byte offset of the start of line N (0-indexed) in current buffer.
    fn line_start_offset(&self, line: usize) -> usize {
        Self::line_start_offset_in(&self.buffer.text, line)
    }

    /// Get the byte offset of the start of line N (0-indexed) in given text.
    fn line_start_offset_in(text: &str, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        let mut current_line = 0;
        for (i, &b) in text.as_bytes().iter().enumerate() {
            if b == b'\n' {
                current_line += 1;
                if current_line == line {
                    return i + 1;
                }
            }
        }
        text.len()
    }

    fn cursor_line_number(&self) -> usize {
        let mut off = self.cursor.offset.min(self.buffer.text.len());
        // Snap backward to a char boundary so the slice is valid UTF-8.
        while off > 0 && !self.buffer.text.is_char_boundary(off) {
            off -= 1;
        }
        self.buffer.text[..off]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
    }

    fn line_content_at(text: &str, line_num: usize) -> Option<&str> {
        text.split('\n').nth(line_num)
    }

    pub fn undo_all(&mut self) {
        // First undo all regular entries
        let count = self.undo_stack.len() as u32;
        if count > 0 {
            self.undo(count);
        }
        // Then undo the implicit initial buffer load (nvim_buf_set_lines).
        // Save current state to redo so this is reversible.
        if !self.buffer.text.is_empty() {
            self.redo_stack.push(UndoEntry {
                text: self.buffer.text.clone(),
                cursor: self.cursor.offset,
                first_edit_offset: Some(0),
            });
            self.buffer = TextBuffer::new("");
            self.vt_mirror.set_text("");
            self.cursor.set_offset(0, 0);
        }
    }

    /// Redo last undone change (restore from redo stack).
    pub fn redo(&mut self, count: u32) {
        for _ in 0..count {
            if let Some(entry) = self.redo_stack.pop() {
                // Save current state to undo stack, preserving first_edit_offset
                // for potential future undo operations.
                self.undo_stack.push(UndoEntry {
                    text: self.buffer.text.clone(),
                    cursor: self.cursor.offset,
                    first_edit_offset: entry.first_edit_offset,
                });
                // Restore from redo stack
                self.buffer = TextBuffer::new(entry.text);
                self.vt_mirror.set_text(self.buffer.text.clone());
                let max = self.buffer.len();
                // Like undo, use first_edit_offset to land cursor on the
                // first changed character. This matches Vim behavior where
                // Ctrl-R positions cursor at the start of the redone change.
                let adjusted = Self::compute_undo_cursor(
                    &self.buffer.text,
                    entry.first_edit_offset,
                    entry.cursor,
                );
                self.cursor.set_offset(adjusted, max);
            }
        }
    }

    /// Convert to GoldenState for test comparison.
    pub fn to_golden_state(
        &self,
        engine: &vim_core::execution::VimEngine,
    ) -> crate::common::golden::GoldenState {
        let (cursor_line, cursor_col) = self.cursor_position();

        // Build register map from VimEngine's internal register state.
        // This captures numbered register shifting (1-9) done by on_delete/shift_numbered,
        // which doesn't emit individual SetRegister effects for each shifted register.
        let mut registers: HashMap<String, crate::common::golden::RegisterSnapshot> =
            HashMap::new();

        // Query all meaningful registers from engine state
        let register_names = [
            '"', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-', '/', 'a', 'b', 'c', 'd',
            'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u',
            'v', 'w', 'x', 'y', 'z',
        ];
        for &name in &register_names {
            let reg_name = vim_core::primitives::RegisterName::new(name).unwrap();
            if let Some(content) = engine.state().registers().get(reg_name) {
                if !content.text().is_empty() {
                    let regtype = match content.motion_type() {
                        vim_core::primitives::MotionType::LineWise => "V".to_string(),
                        vim_core::primitives::MotionType::BlockWise => {
                            // Neovim BlockWise regtype includes the block width
                            // e.g., "\x164" for a block of width 4
                            // Width accounts for tab expansion (tabstop=4).
                            let ts = 4usize;
                            let width = content
                                .text()
                                .lines()
                                .map(|l| {
                                    l.chars()
                                        .map(|c| if c == '\t' { ts } else { 1 })
                                        .sum::<usize>()
                                })
                                .max()
                                .unwrap_or(0);
                            format!("\x16{}", width)
                        }
                        _ => "v".to_string(),
                    };
                    registers.insert(
                        name.to_string(),
                        crate::common::golden::RegisterSnapshot {
                            text: content.text().to_string(),
                            regtype,
                        },
                    );
                }
            }
        }

        // Also include registers from TestDocument that the engine doesn't manage
        // (e.g., '/', ':', '.', '^', etc. set by the test harness)
        for (&name, (text, regtype)) in &self.registers {
            let key = name.to_string();
            if !registers.contains_key(&key) && !text.is_empty() {
                registers.insert(
                    key,
                    crate::common::golden::RegisterSnapshot {
                        text: text.clone(),
                        regtype: regtype.clone(),
                    },
                );
            }
        }

        // Convert Mode to Neovim-compatible string format
        let mode_str = match engine.mode() {
            vim_core::primitives::Mode::Normal => "Normal".to_string(),
            vim_core::primitives::Mode::Insert => "Insert".to_string(),
            vim_core::primitives::Mode::Visual(vim_core::primitives::VisualType::Char) => {
                "Visual".to_string()
            }
            vim_core::primitives::Mode::Visual(vim_core::primitives::VisualType::Line) => {
                "VisualLine".to_string()
            }
            vim_core::primitives::Mode::Visual(vim_core::primitives::VisualType::Block) => {
                "VisualBlock".to_string()
            }
            vim_core::primitives::Mode::Replace => "Replace".to_string(),
            vim_core::primitives::Mode::CommandLine => "CommandLine".to_string(),
            vim_core::primitives::Mode::OperatorPending(_) => "OperatorPending".to_string(),
            vim_core::primitives::Mode::Select(vim_core::primitives::VisualType::Char) => {
                "SelectChar".to_string()
            }
            vim_core::primitives::Mode::Select(vim_core::primitives::VisualType::Line) => {
                "SelectLine".to_string()
            }
            vim_core::primitives::Mode::Select(vim_core::primitives::VisualType::Block) => {
                "SelectBlock".to_string()
            }
            vim_core::primitives::Mode::VirtualReplace => "VirtualReplace".to_string(),
            m => format!("{:?}", m).to_lowercase(),
        };

        crate::common::golden::GoldenState {
            text: self.buffer.text.clone(),
            cursor_offset: self.cursor.offset,
            cursor_line,
            cursor_col,
            mode: mode_str,
            visual_type: None,
            selection_anchor: self.cursor.selection_anchor,
            registers,
            marks: {
                // Build mark map from engine state, mirroring the Neovim oracle's capture:
                // a-z local marks and special marks [, ], <, >, ., ^
                let mut marks_map: HashMap<String, usize> = HashMap::new();
                let state = engine.state();

                // a-z local marks
                for c in 'a'..='z' {
                    if let Some(mn) = vim_core::primitives::MarkName::new(c) {
                        if let Some(mark) = state.marks().get(mn) {
                            marks_map.insert(c.to_string(), mark.offset().get());
                        }
                    }
                }

                // Special marks: [, ], <, >, ., ^
                for c in ['[', ']', '<', '>', '.', '^'] {
                    if let Some(mn) = vim_core::primitives::MarkName::new(c) {
                        if let Some(mark) = state.marks().get(mn) {
                            marks_map.insert(c.to_string(), mark.offset().get());
                        }
                    }
                }

                marks_map
            },
            search_pattern: None,
            search_direction: None,
            // Window, sticky column and navigation lists — the rest of the
            // state the Neovim oracle captures.
            window: None, // TestDocument doesn't have window state
            curswant: {
                // Populate from engine's sticky column (curswant).
                // VirtualColumn::END_OF_LINE (usize::MAX) represents Neovim's MAXCOL,
                // which winsaveview() reports as 2147483647 (i32::MAX).
                //
                // When sticky_column is None (e.g. after undo/redo clears it),
                // compute curswant from the actual cursor position on the current
                // text, matching Neovim's w_set_curswant=TRUE behavior which
                // defers computation to update_curswant().
                Some(
                    engine
                        .state()
                        .sticky_column()
                        .map(|vc| vc.get())
                        .unwrap_or_else(|| {
                            let text = self.text();
                            let offset = self.cursor_offset();
                            vim_core::commands::helpers::curswant_of(text, offset, 4)
                            // tabstop=4 matches oracle
                        }),
                )
            },
            jumplist: {
                let jl = engine.state().jump_list();
                if jl.is_empty() {
                    None
                } else {
                    Some(jl.entries().iter().map(|e| e.offset().get()).collect())
                }
            },
            jumplist_idx: {
                let jl = engine.state().jump_list();
                if jl.is_empty() {
                    None
                } else {
                    Some(jl.position())
                }
            },
            changelist: {
                let cl = engine.state().changelist();
                if cl.is_empty() {
                    None
                } else {
                    Some(cl.entries().iter().map(|e| e[0].get()).collect())
                }
            },
            changelist_idx: {
                let cl = engine.state().changelist();
                if cl.is_empty() {
                    None
                } else {
                    Some(cl.position())
                }
            },
            errmsg: self.errmsg.clone(),
        }
    }
}

impl PartialEq for TestDocument {
    fn eq(&self, other: &Self) -> bool {
        self.buffer.text == other.buffer.text
    }
}

impl Eq for TestDocument {}

// ── Inherent methods that were previously on the Document trait ──────────────
// These were removed from the Document trait during the slimming (the trait
// now only requires text, line_count, offset_to_pos, pos_to_offset).
// TestDocument keeps them as inherent methods for test infrastructure use.
impl TestDocument {
    /// Get the content of line `n` (excluding the trailing newline).
    pub fn line(&self, n: LineNumber) -> Option<&str> {
        let idx = n.get();
        if idx >= self.buffer.line_starts.len() {
            return None;
        }

        let start = self.buffer.line_starts[idx];
        let end = if idx + 1 < self.buffer.line_starts.len() {
            self.buffer.line_starts[idx + 1] - 1
        } else {
            self.buffer.text.len()
        };

        Some(&self.buffer.text[start..end])
    }

    /// Get the byte offset of the start of line `n`.
    pub fn line_start(&self, n: LineNumber) -> Option<Offset> {
        self.buffer
            .line_starts
            .get(n.get())
            .map(|&o| Offset::new(o))
    }

    /// Get the byte offset of the end of line `n` (before the newline).
    pub fn line_end(&self, n: LineNumber) -> Option<Offset> {
        let idx = n.get();
        if idx >= self.buffer.line_starts.len() {
            return None;
        }

        let end = if idx + 1 < self.buffer.line_starts.len() {
            self.buffer.line_starts[idx + 1] - 1
        } else {
            self.buffer.text.len()
        };

        Some(Offset::new(end))
    }

    /// Get the character at `offset`.
    pub fn char_at(&self, offset: Offset) -> Option<char> {
        self.buffer.text[offset.get()..].chars().next()
    }
}

impl Document for TestDocument {
    fn text(&self) -> &str {
        &self.buffer.text
    }

    fn line_count(&self) -> usize {
        self.buffer.line_starts.len()
    }

    fn offset_to_pos(&self, offset: Offset) -> Option<Position> {
        let off = offset.get();
        if off > self.buffer.text.len() {
            return None;
        }

        let line_idx = self
            .buffer
            .line_starts
            .iter()
            .rposition(|&start| start <= off)
            .unwrap_or(0);

        let line_start = self.buffer.line_starts[line_idx];
        let col = off - line_start;

        Some(Position::new(LineNumber::new(line_idx), Column::new(col)))
    }

    fn pos_to_offset(&self, pos: Position) -> Option<Offset> {
        let line_start = self.line_start(pos.line())?;
        let offset = line_start.get() + pos.col().get();

        if offset > self.buffer.text.len() {
            return None;
        }

        Some(Offset::new(offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_document() {
        let doc = TestDocument::empty();
        assert!(doc.buffer.is_empty());
        assert_eq!(doc.line_count(), 1);
    }

    #[test]
    fn test_single_line() {
        let doc = TestDocument::from_text("hello");
        assert_eq!(doc.text(), "hello");
        assert_eq!(doc.line_count(), 1);
        assert_eq!(doc.line(LineNumber::new(0)), Some("hello"));
    }

    #[test]
    fn test_multiple_lines() {
        let doc = TestDocument::from_text("hello\nworld");
        assert_eq!(doc.line_count(), 2);
        assert_eq!(doc.line(LineNumber::new(0)), Some("hello"));
        assert_eq!(doc.line(LineNumber::new(1)), Some("world"));
    }

    #[test]
    fn test_offset_to_pos() {
        let doc = TestDocument::from_text("hello\nworld");
        assert_eq!(
            doc.offset_to_pos(Offset::new(0)),
            Some(Position::from_raw(0, 0))
        );
        assert_eq!(
            doc.offset_to_pos(Offset::new(6)),
            Some(Position::from_raw(1, 0))
        );
    }

    #[test]
    fn test_lines_factory() {
        let doc = TestDocument::lines(3);
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.line(LineNumber::new(0)), Some("line 1"));
    }
}
