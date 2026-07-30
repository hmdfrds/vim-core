//! Action commands.
//!
//! Standalone commands that don't require a motion or text object.

use strum::{Display, EnumIter};

/// Standalone action commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Action {
    /// Delete character under cursor (x)
    DeleteChar,
    /// Delete character before cursor (X)
    DeleteCharBack,
    /// Put after cursor (p)
    Put,
    /// Put before cursor (P)
    PutBefore,
    /// Undo (u)
    Undo,
    /// Redo (Ctrl-R)
    Redo,
    /// Undo line changes (U)
    UndoLine,
    /// Join lines (J)
    Join,
    /// Swap case (~)
    SwapCase,
    /// Delete to end of line (D)
    DeleteToEnd,
    /// Change to end of line (C)
    ChangeToEnd,
    /// Yank line (Y)
    YankLine,
    /// Substitute character (s)
    Substitute,
    /// Jump to older position (Ctrl-O)
    JumpOlder,
    /// Jump to newer position (Ctrl-I)
    JumpNewer,
    /// Block visual insert (I) — insert at left edge of block
    BlockInsert,
    /// Block visual append (A) — append at right edge of block
    BlockAppend,
    /// Increment number under/after cursor (Ctrl-A)
    IncrementNumber,
    /// Decrement number under/after cursor (Ctrl-X)
    DecrementNumber,
    /// Put after cursor, cursor after pasted text (gp)
    PutAfterCursorAfter,
    /// Put before cursor, cursor after pasted text (gP)
    PutBeforeCursorAfter,
    /// Show file info (Ctrl-G)
    ShowFileInfo,
    /// Keyword lookup (K)
    KeywordLookup,
    /// Intent-aware repeat (g.) — replay last command's semantic intent.
    ///
    /// Unlike `.` which replays the exact keystroke sequence, `g.` reads
    /// the [`CommandIntent`](crate::state::CommandIntent) and re-resolves motions/text-objects at the
    /// current cursor position before applying the operator.
    IntentRepeat,
    /// Put after cursor with indent adjustment (]p).
    ///
    /// Pastes linewise register content after the current line, reindenting
    /// each pasted line to match the indentation of the current line.
    PutIndentAfter,
    /// Put before cursor with indent adjustment ([p).
    ///
    /// Pastes linewise register content before the current line, reindenting
    /// each pasted line to match the indentation of the current line.
    PutIndentBefore,
    /// Repeat last substitution on current line (&)
    RepeatSubstitute,
    /// Repeat last substitution globally (g&)
    RepeatSubstituteGlobal,
    /// Switch to alternate file (Ctrl-^/Ctrl-6)
    AlternateFile,
    /// Add cursor at next match of word under cursor (gb)
    AddNextMatchCursor,
    /// Add cursor at previous match (gB)
    AddPrevMatchCursor,
    /// Skip current match without adding cursor (gs)
    SkipMatchCursor,
}

impl Action {
    /// Create from key character.
    #[must_use]
    pub const fn from_char(c: char) -> Option<Self> {
        match c {
            'x' => Some(Self::DeleteChar),
            'X' => Some(Self::DeleteCharBack),
            'p' => Some(Self::Put),
            'P' => Some(Self::PutBefore),
            'u' => Some(Self::Undo),
            'U' => Some(Self::UndoLine),
            'J' => Some(Self::Join),
            '~' => Some(Self::SwapCase),
            'D' => Some(Self::DeleteToEnd),
            'C' => Some(Self::ChangeToEnd),
            'Y' => Some(Self::YankLine),
            's' => Some(Self::Substitute),
            'I' => Some(Self::BlockInsert),
            'A' => Some(Self::BlockAppend),
            'K' => Some(Self::KeywordLookup),
            '&' => Some(Self::RepeatSubstitute),
            _ => None,
        }
    }

    /// Create from key character with control modifier.
    ///
    /// Handles Ctrl-R (redo), Ctrl-O (jump older), Ctrl-I (jump newer),
    /// Ctrl-A (increment number), Ctrl-X (decrement number).
    #[must_use]
    pub const fn from_ctrl_char(c: char) -> Option<Self> {
        match c {
            'r' | 'R' => Some(Self::Redo),
            'o' | 'O' => Some(Self::JumpOlder),
            'i' | 'I' => Some(Self::JumpNewer),
            'a' | 'A' => Some(Self::IncrementNumber),
            'x' | 'X' => Some(Self::DecrementNumber),
            'g' | 'G' => Some(Self::ShowFileInfo),
            '^' | '6' => Some(Self::AlternateFile),
            _ => None,
        }
    }

    /// Whether this action reads document text to compute its effect.
    ///
    /// Content-dependent actions inspect characters, lines, or indentation
    /// at the cursor position and therefore may produce different results
    /// when the same logical command is re-executed on different cursors
    /// over different text.  Position-independent actions operate on state
    /// (registers, undo history, jump list) and don't inspect the buffer.
    #[must_use]
    pub const fn is_content_dependent(&self) -> bool {
        match self {
            // Content-dependent: these read document text
            Self::SwapCase
            | Self::IncrementNumber
            | Self::DecrementNumber
            | Self::DeleteChar
            | Self::DeleteCharBack
            | Self::Join
            | Self::Substitute
            | Self::DeleteToEnd
            | Self::ChangeToEnd
            | Self::YankLine
            | Self::BlockInsert
            | Self::BlockAppend
            | Self::RepeatSubstitute
            | Self::RepeatSubstituteGlobal
            | Self::PutIndentAfter
            | Self::PutIndentBefore
            | Self::ShowFileInfo => true,

            // Position-independent: state or host operations
            Self::Put
            | Self::PutBefore
            | Self::PutAfterCursorAfter
            | Self::PutBeforeCursorAfter
            | Self::Undo
            | Self::Redo
            | Self::UndoLine
            | Self::JumpOlder
            | Self::JumpNewer
            | Self::KeywordLookup
            | Self::IntentRepeat
            | Self::AlternateFile
            | Self::AddNextMatchCursor
            | Self::AddPrevMatchCursor
            | Self::SkipMatchCursor => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    // === is_content_dependent: exhaustiveness guard ===

    /// Ensures every `Action` variant is covered by `is_content_dependent`.
    ///
    /// If a new variant is added to `Action` without updating the match,
    /// this test will still compile (because the match is exhaustive and
    /// the enum is `#[non_exhaustive]`), but this iterator-based test
    /// provides an additional signal that the classification was reviewed.
    #[test]
    fn is_content_dependent_covers_all_variants() {
        for action in Action::iter() {
            // Just call the method — panics would indicate a missed arm,
            // though with an exhaustive match this is mainly a smoke test.
            let _ = action.is_content_dependent();
        }
    }

    // === Content-dependent actions (true) ===

    #[test]
    fn swap_case_is_content_dependent() {
        assert!(Action::SwapCase.is_content_dependent());
    }

    #[test]
    fn increment_number_is_content_dependent() {
        assert!(Action::IncrementNumber.is_content_dependent());
    }

    #[test]
    fn decrement_number_is_content_dependent() {
        assert!(Action::DecrementNumber.is_content_dependent());
    }

    #[test]
    fn delete_char_is_content_dependent() {
        assert!(Action::DeleteChar.is_content_dependent());
    }

    #[test]
    fn delete_char_back_is_content_dependent() {
        assert!(Action::DeleteCharBack.is_content_dependent());
    }

    #[test]
    fn join_is_content_dependent() {
        assert!(Action::Join.is_content_dependent());
    }

    #[test]
    fn substitute_is_content_dependent() {
        assert!(Action::Substitute.is_content_dependent());
    }

    #[test]
    fn delete_to_end_is_content_dependent() {
        assert!(Action::DeleteToEnd.is_content_dependent());
    }

    #[test]
    fn change_to_end_is_content_dependent() {
        assert!(Action::ChangeToEnd.is_content_dependent());
    }

    #[test]
    fn yank_line_is_content_dependent() {
        assert!(Action::YankLine.is_content_dependent());
    }

    #[test]
    fn block_insert_is_content_dependent() {
        assert!(Action::BlockInsert.is_content_dependent());
    }

    #[test]
    fn block_append_is_content_dependent() {
        assert!(Action::BlockAppend.is_content_dependent());
    }

    #[test]
    fn repeat_substitute_is_content_dependent() {
        assert!(Action::RepeatSubstitute.is_content_dependent());
    }

    #[test]
    fn repeat_substitute_global_is_content_dependent() {
        assert!(Action::RepeatSubstituteGlobal.is_content_dependent());
    }

    #[test]
    fn put_indent_after_is_content_dependent() {
        assert!(Action::PutIndentAfter.is_content_dependent());
    }

    #[test]
    fn put_indent_before_is_content_dependent() {
        assert!(Action::PutIndentBefore.is_content_dependent());
    }

    #[test]
    fn show_file_info_is_content_dependent() {
        assert!(Action::ShowFileInfo.is_content_dependent());
    }

    // === Position-independent actions (false) ===

    #[test]
    fn put_is_not_content_dependent() {
        assert!(!Action::Put.is_content_dependent());
    }

    #[test]
    fn put_before_is_not_content_dependent() {
        assert!(!Action::PutBefore.is_content_dependent());
    }

    #[test]
    fn put_after_cursor_after_is_not_content_dependent() {
        assert!(!Action::PutAfterCursorAfter.is_content_dependent());
    }

    #[test]
    fn put_before_cursor_after_is_not_content_dependent() {
        assert!(!Action::PutBeforeCursorAfter.is_content_dependent());
    }

    #[test]
    fn undo_is_not_content_dependent() {
        assert!(!Action::Undo.is_content_dependent());
    }

    #[test]
    fn redo_is_not_content_dependent() {
        assert!(!Action::Redo.is_content_dependent());
    }

    #[test]
    fn undo_line_is_not_content_dependent() {
        assert!(!Action::UndoLine.is_content_dependent());
    }

    #[test]
    fn jump_older_is_not_content_dependent() {
        assert!(!Action::JumpOlder.is_content_dependent());
    }

    #[test]
    fn jump_newer_is_not_content_dependent() {
        assert!(!Action::JumpNewer.is_content_dependent());
    }

    #[test]
    fn keyword_lookup_is_not_content_dependent() {
        assert!(!Action::KeywordLookup.is_content_dependent());
    }

    #[test]
    fn intent_repeat_is_not_content_dependent() {
        assert!(!Action::IntentRepeat.is_content_dependent());
    }

    #[test]
    fn alternate_file_is_not_content_dependent() {
        assert!(!Action::AlternateFile.is_content_dependent());
    }

    #[test]
    fn add_next_match_cursor_is_not_content_dependent() {
        assert!(!Action::AddNextMatchCursor.is_content_dependent());
    }

    #[test]
    fn add_prev_match_cursor_is_not_content_dependent() {
        assert!(!Action::AddPrevMatchCursor.is_content_dependent());
    }

    #[test]
    fn skip_match_cursor_is_not_content_dependent() {
        assert!(!Action::SkipMatchCursor.is_content_dependent());
    }
}
