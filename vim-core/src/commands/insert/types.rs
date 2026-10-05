//! Insert mode context and result types.
//!
//! Core types for insert mode command execution without traits.
//!
//! # Architecture
//!
//! No `dyn` traits in the hot path: enum dispatch plus plain functions,
//! not trait objects.
//!
//! ```text
//! Engine
//!   │
//!   ├─ precompute_insert() ─► InsertPrecomputed (computed ONCE)
//!   │
//!   ├─ apply_insert_mutations() (uses InsertPrecomputed for state tracking)
//!   │
//!   └─ dispatch_insert() ─► CommandResult (uses InsertPrecomputed for effects)
//!        InsertContext
//!        (cursor, text, precomputed)
//! ```

use crate::primitives::{AutoPairs, Offset, ReplacedChar, WordCharSet, WordEraseStyle};
use compact_str::CompactString;

/// Replace-mode backspace restoration action.
///
/// Self-documenting enum replacing `Option<Option<char>>`.
/// The replace stack tracks which characters were overwritten and
/// what should be restored when backspace is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReplaceRestoreAction {
    /// Stack empty — cursor is at the entry point where replace mode
    /// started. Backspace is a no-op.
    StackEmpty,

    /// Restore the original character at cursor.
    Restore(ReplacedChar),

    /// Line boundary — join current line back to previous line.
    ///
    /// When Enter was pressed in Replace mode, it pushed `LineBoundary`
    /// onto the replace stack. Backspace at this boundary deletes the
    /// newline and any autoindent on the current line, moving the cursor
    /// to the end of the previous line.
    JoinLine {
        /// Byte offset of the end of the previous line (where the `\n` is).
        prev_line_end: usize,
        /// Byte length of the newline + any leading whitespace on the current
        /// line (the total bytes to delete to join the lines).
        delete_len: usize,
    },
}

/// Pre-computed operation data for insert commands.
///
/// The engine computes this ONCE. Both state-mutation tracking
/// (`apply_insert_mutations`) and effect construction (`dispatch_insert`)
/// consume the same instance — zero duplication.
///
/// This enum encodes what the operation IS, including mode-specific data.
/// No separate `is_replace` flag needed — the variant itself makes it explicit.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum InsertPrecomputed {
    /// Normal character insertion (not newline, not tab, not replace).
    Char(char),

    /// Newline with pre-computed autoindent.
    Newline {
        /// The newline + indentation string to insert.
        insert_text: CompactString,
        /// Bytes of trailing whitespace to strip before inserting.
        trailing_strip_len: usize,
        /// Bytes of autoindent-only whitespace to strip before cursor.
        ///
        /// Neovim's `trunc_line` behavior: when `did_ai` is set (autoindent
        /// was applied) and the old line from line-start to cursor is entirely
        /// whitespace, the old line is truncated to empty on Enter.
        leading_strip_len: usize,
        /// Absolute cursor position after the insert.
        cursor_advance: usize,
    },

    /// Tab expansion to spaces.
    Tab {
        /// Number of spaces to insert.
        spaces: usize,
    },

    /// Replace-mode tab expansion.
    ///
    /// Tab in replace mode expands to spaces (like normal insert) but also
    /// deletes the character under the cursor (replace semantics).
    ReplaceTab {
        /// Number of spaces to insert.
        spaces: usize,
        /// The original character at cursor (for backspace restoration stack).
        original_char: ReplacedChar,
        /// Bytes to delete before inserting the expanded spaces.
        delete_len: Option<usize>,
    },

    /// Replace-mode character overwrite.
    ReplaceChar {
        /// The character being typed.
        ch: char,
        /// The original character at cursor (for backspace restoration stack).
        original_char: ReplacedChar,
        /// Bytes to delete before inserting the new char.
        delete_len: Option<usize>,
    },

    /// Replace-mode backspace restoration.
    ReplaceBackspace {
        /// What to restore (semantic enum, not nested `Option`).
        action: ReplaceRestoreAction,
        /// Position of previous character boundary.
        prev_pos: usize,
    },

    /// Character insertion with smartindent adjustment.
    ///
    /// The precompute phase detected that this char (`{`, `}`, or `#`) triggers
    /// a smartindent adjustment. The dispatch phase replaces leading whitespace
    /// with `new_indent`, then inserts the character.
    CharWithIndentAdjust {
        /// The character being inserted (`{`, `}`, or `#`).
        ch: char,
        /// Byte offset where leading whitespace starts (line start).
        strip_start: usize,
        /// Byte offset where leading whitespace ends.
        strip_end: usize,
        /// The replacement indent string (may be empty for `#`).
        new_indent: CompactString,
    },

    /// Pre-computed text to insert (Ctrl-A last inserted, Ctrl-R register).
    Text(CompactString),

    /// Error to show instead of inserting (e.g. E32 for empty filename register).
    Error(crate::errors::VimError),

    /// No pre-computation needed (DeleteWord, Indent, OneShot, etc.)
    None,
}

/// Context provided to insert mode commands during execution.
///
/// Contains all information an insert command needs to perform its task.
/// The engine populates `precomputed` with all derived data so dispatch
/// stays state-free — zero VimState access, zero recomputation.
#[derive(Debug, Clone)]
pub struct InsertContext<'text> {
    /// The full document text.
    pub text: &'text str,

    /// Current cursor position.
    pub cursor: Offset,

    /// Offset where insert mode was entered (for C-u/C-w boundary).
    /// When set, C-u/C-w won't delete past this point.
    pub entry_offset: Option<Offset>,

    /// Pre-computed operation data from the engine.
    pub precomputed: InsertPrecomputed,

    /// Shift width for indent/outdent operations (from `VimOptions`).
    pub shift_width: usize,

    /// Tab display width (from `VimOptions`). Used to align smarttab
    /// backspace on display columns rather than raw byte offsets, so a
    /// tab-indented line deletes one indent level instead of the whole
    /// indent.
    pub tabstop: usize,

    /// Auto-pair configuration. `None` means auto-pairing is disabled.
    pub auto_pairs: Option<&'text AutoPairs>,

    /// Word character set for Ctrl-W word deletion boundaries.
    pub word_chars: &'text WordCharSet,

    /// Ctrl-W erase algorithm (Vi / AltWerase / TtyWerase).
    pub word_erase_style: WordEraseStyle,
}

/// Static default word char set for insert contexts constructed without options.
static DEFAULT_INSERT_WORD_CHARS: std::sync::LazyLock<WordCharSet> =
    std::sync::LazyLock::new(WordCharSet::default_vim);

impl<'text> InsertContext<'text> {
    /// Create a new insert context with no pre-computed data.
    #[inline]
    #[must_use]
    pub fn new(text: &'text str, cursor: Offset) -> Self {
        Self {
            text,
            cursor,
            entry_offset: None,
            precomputed: InsertPrecomputed::None,
            shift_width: 4,
            tabstop: 4,
            auto_pairs: None,
            word_chars: &DEFAULT_INSERT_WORD_CHARS,
            word_erase_style: WordEraseStyle::Vi,
        }
    }

    /// Set the entry offset boundary (for C-u/C-w).
    #[inline]
    #[must_use]
    pub const fn with_entry_offset(mut self, offset: Offset) -> Self {
        self.entry_offset = Some(offset);
        self
    }

    /// Set the pre-computed operation data.
    #[inline]
    #[must_use]
    pub fn with_precomputed(mut self, precomputed: InsertPrecomputed) -> Self {
        self.precomputed = precomputed;
        self
    }

    /// Set the shift width for indent/outdent operations.
    #[inline]
    #[must_use]
    pub const fn with_shift_width(mut self, shift_width: usize) -> Self {
        self.shift_width = shift_width;
        self
    }

    /// Set the tab display width (for smarttab backspace column alignment).
    #[inline]
    #[must_use]
    pub const fn with_tabstop(mut self, tabstop: usize) -> Self {
        self.tabstop = tabstop;
        self
    }

    /// Set the auto-pairs configuration.
    #[inline]
    #[must_use]
    pub const fn with_auto_pairs(mut self, auto_pairs: Option<&'text AutoPairs>) -> Self {
        self.auto_pairs = auto_pairs;
        self
    }

    /// Set the Ctrl-W word-erase algorithm.
    #[inline]
    #[must_use]
    pub const fn with_word_erase_style(mut self, style: WordEraseStyle) -> Self {
        self.word_erase_style = style;
        self
    }

    /// Get cursor position as usize for calculations.
    #[inline]
    #[must_use]
    pub const fn cursor_usize(&self) -> usize {
        self.cursor.get()
    }
}

/// The full context of the insert session needed for exit effects.
pub struct InsertExitParams<'text> {
    /// The full document text.
    pub text: &'text str,
    /// Text accumulated during this insert session.
    pub accumulated_text: &'text str,
    /// The current byte offset of the cursor.
    pub cursor: Offset,
    /// Command execution count for repeats (always >= 1).
    pub count: u32,
    /// How the insert mode was entered.
    pub entry_type: crate::primitives::InsertEntryType,
    /// The length of auto-indentation inserted by o/O.
    pub auto_indent_len: usize,
    /// The block visual insert context, if active.
    pub block_insert: Option<&'text crate::state::BlockInsertContext>,
    /// Mark '.' override: byte offset within accumulated_text of the last
    /// non-batching insert operation (Tab, Ctrl-E/Y, Ctrl-R).  When `Some`,
    /// this overrides the backward-walk computation in `compute_mark_dot_for_insert`.
    pub mark_dot_override_pos: Option<usize>,
    /// The byte offset where insert mode was entered (Insstart in Neovim).
    /// Used to set mark `[` on exit.  `None` means unknown / use computed fallback.
    pub entry_offset: Option<Offset>,
    /// Formatting options for the count repeat, which Vim types like the
    /// first round. `None` leaves the repeated text unformatted.
    pub format: Option<&'text crate::commands::insert::wrap::FormatPolicy<'text>>,
    /// Insert start of the session, for the `l`, `v` and `b` flags.
    pub insert_start: Option<crate::state::InsertStart>,
}

/// Context for insert mode exit computations.
///
/// Bundles the shared parameters needed by [`compute_exit_cursor`](crate::commands::insert::compute_exit_cursor) and
/// [`compute_block_insert_offsets`](crate::commands::insert::compute_block_insert_offsets) into a consistent context struct,
/// matching the `Context → Result` pattern used throughout `commands/`.
pub struct InsertExitContext<'text> {
    /// The full document text (after all insert-mode edits).
    pub text: &'text str,
    /// Text accumulated during this insert session.
    pub accumulated_text: &'text str,
    /// Whether this was an o/O entry with no text typed (triggers auto-indent strip).
    pub is_open_line_no_text: bool,
    /// Byte offset where the cursor currently sits (end of inserted text).
    pub insert_offset: Offset,
    /// Block visual insert context, if active.
    pub block_insert: Option<&'text crate::state::BlockInsertContext>,
}

/// Result of newline insert computation.
#[derive(Debug, Clone)]
pub struct NewlineInsert {
    /// The text to insert (newline + indentation).
    pub insert_text: CompactString,
    /// Byte length of insert_text.
    pub insert_len: usize,
    /// Number of bytes of trailing whitespace to strip after cursor.
    pub trailing_strip_len: usize,
    /// Number of bytes of autoindent whitespace to strip before cursor.
    ///
    /// Neovim's `trunc_line` behavior: when `did_ai` is set and the user
    /// presses Enter, the old line's trailing whitespace (which is the
    /// autoindent) is removed.  This field is non-zero only when the
    /// text from line start to cursor is entirely whitespace.
    pub leading_strip_len: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandResult;

    #[test]
    fn test_insert_context_creation() {
        let ctx = InsertContext::new("hello world", Offset::new(5));
        assert_eq!(ctx.cursor_usize(), 5);
        assert!(matches!(ctx.precomputed, InsertPrecomputed::None));
    }

    #[test]
    fn test_insert_context_with_precomputed() {
        let ctx = InsertContext::new("hello", Offset::new(3))
            .with_precomputed(InsertPrecomputed::Char('x'));
        assert!(matches!(ctx.precomputed, InsertPrecomputed::Char('x')));
    }

    #[test]
    fn test_insert_result_empty() {
        let result = CommandResult::none();
        assert!(result.is_empty());
    }
}
