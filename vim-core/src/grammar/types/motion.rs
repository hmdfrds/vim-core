//! Motion enum.
//!
//! All motion commands that move the cursor.

use strum::Display;

/// Motion commands.
///
/// Motions move the cursor to a new position. They can be used standalone
/// or as arguments to operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Motion {
    // === Character motions ===
    /// Move left (h)
    Left,
    /// Move right (l)
    Right,
    /// Move right wrapping (`<Space>`)
    Space,

    // === Line motions ===
    /// Move up (k) — fold-aware when FoldProvider present
    Up,
    /// Move down (j) — fold-aware when FoldProvider present
    Down,
    /// Move down by display line (gj) — soft-wrap aware
    DisplayDown,
    /// Move up by display line (gk) — soft-wrap aware
    DisplayUp,
    /// Down to first non-blank (+, Enter)
    DownFirstNonBlank,
    /// Up to first non-blank (-)
    UpFirstNonBlank,

    // === Word motions ===
    /// Word forward (w)
    WordForward,
    /// Word backward (b)
    WordBackward,
    /// Word end (e)
    WordEnd,
    /// Word end backward (ge)
    WordEndBackward,
    /// WORD forward (W)
    WORDForward,
    /// WORD backward (B)
    WORDBackward,
    /// WORD end (E)
    WORDEnd,
    /// WORD end backward (gE)
    WORDEndBackward,

    // === Line position ===
    /// Line start (0)
    LineStart,
    /// Line end ($)
    LineEnd,
    /// First non-blank (^)
    FirstNonBlank,
    /// Last non-blank (g_)
    LastNonBlank,
    /// First non-blank of nth line (_)
    FirstNonBlankLine,

    // === Document ===
    /// Go to line / end of document (G)
    GotoLine,
    /// Go to first line (gg)
    GotoFirstLine,

    // === Screen ===
    /// High (H)
    ScreenHigh,
    /// Middle (M)
    ScreenMiddle,
    /// Low (L)
    ScreenLow,

    // === Matching ===
    /// Matching bracket (%)
    MatchingPair,

    // === Search repeat ===
    /// Next search match (n)
    SearchNext,
    /// Previous search match (N)
    SearchPrev,

    // === Character find repeat ===
    /// Repeat last f/F/t/T (;)
    RepeatFind,
    /// Repeat last f/F/t/T reverse (,)
    RepeatFindReverse,

    // === Word search ===
    /// Search word under cursor (*)
    WordSearchForward,
    /// Search word under cursor backward (#)
    WordSearchBackward,

    // === Sentence/Paragraph ===
    /// Sentence forward ())
    SentenceForward,
    /// Sentence backward (()
    SentenceBackward,
    /// Paragraph forward (})
    ParagraphForward,
    /// Paragraph backward ({)
    ParagraphBackward,

    // === Section motions ===
    /// Forward to next `{` at column 0 (`]]`)
    SectionForwardStart,
    /// Backward to previous `{` at column 0 (`[[`)
    SectionBackwardStart,
    /// Forward to next `}` at column 0 (`][`)
    SectionForwardEnd,
    /// Backward to previous `}` at column 0 (`[]`)
    SectionBackwardEnd,

    // === Column ===
    /// Go to column N (|)
    GoToColumn,

    // === Screen-line ===
    /// Go to start of screen line (g0)
    ScreenLineStart,
    /// Go to end of screen line (g$)
    ScreenLineEnd,
    /// Go to first non-blank of screen line (g^)
    ScreenFirstNonBlank,

    // === Misc ===
    /// Go to middle of screen line (gm)
    MiddleOfScreenLine,
    /// Go to middle of text line (gM)
    MiddleOfTextLine,
    /// Go to byte offset N (go)
    GotoByte,

    // === Scroll ===
    /// Scroll half page down (Ctrl-D)
    ScrollHalfDown,
    /// Scroll half page up (Ctrl-U)
    ScrollHalfUp,
    /// Scroll full page down (Ctrl-F)
    ScrollFullDown,
    /// Scroll full page up (Ctrl-B)
    ScrollFullUp,
    /// Scroll viewport one line down (Ctrl-E)
    ScrollLineDown,
    /// Scroll viewport one line up (Ctrl-Y)
    ScrollLineUp,

    // === Changelist ===
    /// Jump to older change position (g;)
    ChangelistOlder,
    /// Jump to newer change position (g,)
    ChangelistNewer,

    // === Search object ===
    /// Search forward and visually select match (gn)
    SearchObjectForward,
    /// Search backward and visually select match (gN)
    SearchObjectBackward,

    // === Unmatched bracket motions ===
    /// Previous unmatched `{` ([{)
    PrevUnmatchedBrace,
    /// Next unmatched `}` (]})
    NextUnmatchedBrace,
    /// Previous unmatched `(` ([(])
    PrevUnmatchedParen,
    /// Next unmatched `)` (]))
    NextUnmatchedParen,

    // === Method boundary motions ===
    /// Previous method/function start ([m)
    PrevMethodStart,
    /// Next method/function start (]m)
    NextMethodStart,
    /// Previous method/function end ([M)
    PrevMethodEnd,
    /// Next method/function end (]M)
    NextMethodEnd,

    // === Comment navigation ===
    /// Previous start of C comment ([/)
    PrevCommentStart,
    /// Next end of C comment (]/)
    NextCommentEnd,

    // === Bracket/quote pair navigation ===
    /// Jump to next opening bracket (`]b`).
    NextBracketPair,
    /// Jump to previous opening bracket (`[b`).
    PrevBracketPair,
    /// Jump to next quote character (`]q`).
    NextQuotePair,
    /// Jump to previous quote character (`[q`).
    PrevQuotePair,

    // === Partial word search ===
    /// g* — partial match forward (no word boundaries)
    PartialWordSearchForward,
    /// g# — partial match backward (no word boundaries)
    PartialWordSearchBackward,

    // === Subword motions ===
    /// Move to start of next subword (camelCase/snake_case boundary).
    SubwordForward,
    /// Move to start of previous subword (camelCase/snake_case boundary).
    SubwordBackward,
    /// Move to end of current/next subword (camelCase/snake_case boundary).
    SubwordEnd,
    /// Move to end of previous subword (camelCase/snake_case boundary).
    SubwordEndBackward,

    // === Mark navigation ===
    /// Jump to next lowercase mark by buffer position (`]'`).
    NextMark,
    /// Jump to previous lowercase mark by buffer position (`['`).
    PreviousMark,

    // === Indent navigation ===
    /// Previous line with same indentation level ([i).
    PrevSameIndent,
    /// Next line with same indentation level (]i).
    NextSameIndent,
    /// Previous line with lesser indentation ([-) — parent scope.
    PrevLesserIndent,
    /// Next line with lesser indentation (]-) — parent scope.
    NextLesserIndent,
    /// Previous line with greater indentation ([+) — child scope.
    PrevGreaterIndent,
    /// Next line with greater indentation (]+) — child scope.
    NextGreaterIndent,

    // === Text object seeking ===
    /// Seek forward/backward to next instance of a text object.
    ///
    /// `]w` = next word, `]"` = next quote pair, `[S` = previous subword, etc.
    /// The grammar routes `]`/`[` + text-object-key here when no existing
    /// specific bracket motion matches.
    SeekTextObject {
        /// Which text object to seek.
        kind: super::TextObjectKind,
        /// Forward (`]`) or backward (`[`).
        direction: crate::primitives::Direction,
    },

    // === Custom (host-registered) ===
    /// Host-registered custom motion (runtime extension).
    ///
    /// The `u32` is a unique ID assigned by the host when registering the motion.
    /// The dispatch layer routes this to the registered `CustomMotionProvider`.
    Custom(u32),
}

impl Motion {
    /// Create from key character (simple motions only).
    #[must_use]
    pub const fn from_char(c: char) -> Option<Self> {
        match c {
            'h' => Some(Self::Left),
            'l' => Some(Self::Right),
            ' ' => Some(Self::Space),
            'k' => Some(Self::Up),
            'j' => Some(Self::Down),
            '+' => Some(Self::DownFirstNonBlank),
            '-' => Some(Self::UpFirstNonBlank),
            '_' => Some(Self::FirstNonBlankLine),
            'w' => Some(Self::WordForward),
            'b' => Some(Self::WordBackward),
            'e' => Some(Self::WordEnd),
            'W' => Some(Self::WORDForward),
            'B' => Some(Self::WORDBackward),
            'E' => Some(Self::WORDEnd),
            '0' => Some(Self::LineStart),
            '$' => Some(Self::LineEnd),
            '^' => Some(Self::FirstNonBlank),
            'G' => Some(Self::GotoLine),
            'H' => Some(Self::ScreenHigh),
            'M' => Some(Self::ScreenMiddle),
            'L' => Some(Self::ScreenLow),
            '%' => Some(Self::MatchingPair),
            'n' => Some(Self::SearchNext),
            'N' => Some(Self::SearchPrev),
            ';' => Some(Self::RepeatFind),
            ',' => Some(Self::RepeatFindReverse),
            '*' => Some(Self::WordSearchForward),
            '#' => Some(Self::WordSearchBackward),
            ')' => Some(Self::SentenceForward),
            '(' => Some(Self::SentenceBackward),
            '}' => Some(Self::ParagraphForward),
            '{' => Some(Self::ParagraphBackward),
            '|' => Some(Self::GoToColumn),
            _ => None,
        }
    }

    /// Create from full key event (handles Ctrl-modified keys).
    pub fn from_key_event(key: &crate::keymap::KeyEvent) -> Option<Self> {
        use crate::keymap::{Key, Modifiers};
        // First try simple char mapping
        if let Some(motion) = key.as_char().and_then(Self::from_char) {
            return Some(motion);
        }
        // Handle ctrl-modified keys
        if key.modifiers == Modifiers::CTRL {
            if let Key::Char(c) = key.key {
                return match c {
                    'h' => Some(Self::Left),
                    'j' => Some(Self::Down),
                    'd' => Some(Self::ScrollHalfDown),
                    'u' => Some(Self::ScrollHalfUp),
                    'f' => Some(Self::ScrollFullDown),
                    'b' => Some(Self::ScrollFullUp),
                    'e' => Some(Self::ScrollLineDown),
                    'y' => Some(Self::ScrollLineUp),
                    'm' => Some(Self::DownFirstNonBlank),
                    'n' => Some(Self::Down),
                    'p' => Some(Self::Up),
                    _ => None,
                };
            }
        }
        // Handle Ctrl+arrow word movement
        if key.modifiers == Modifiers::CTRL {
            return match key.key {
                Key::Left => Some(Self::WordBackward),
                Key::Right => Some(Self::WordForward),
                // drift: Ctrl+non-arrow keys have no motion mapping here; resolved by the g-prefix or char dispatch instead
                _ => None,
            };
        }
        // Handle Shift+arrow word/page movement
        if key.modifiers == Modifiers::SHIFT {
            return match key.key {
                Key::Left => Some(Self::WordBackward),
                Key::Right => Some(Self::WordForward),
                Key::Up => Some(Self::ScrollFullUp),
                Key::Down => Some(Self::ScrollFullDown),
                // drift: Shift+Home/End/PageUp/PageDown have no motion semantic in this path; handled as pass-through above
                _ => None,
            };
        }
        // Handle arrow keys, Home, End, PageUp, PageDown, Enter, Backspace
        match key.key {
            Key::Up => Some(Self::Up),
            Key::Down => Some(Self::Down),
            Key::Left => Some(Self::Left),
            Key::Right => Some(Self::Right),
            Key::Home => Some(Self::LineStart),
            Key::End => Some(Self::LineEnd),
            Key::PageUp => Some(Self::ScrollFullUp),
            Key::PageDown => Some(Self::ScrollFullDown),
            Key::Enter => Some(Self::DownFirstNonBlank),
            Key::Backspace => Some(Self::Left),
            // drift: F-keys, Insert, Delete, and other named keys have no motion mapping and return None
            _ => None,
        }
    }

    /// Check if this motion is a vertical motion (preserves sticky column).
    ///
    /// Vertical motions read the sticky column (curswant) but do not update it.
    /// Per `:help curswant`: j, k, gj, gk, G, gg, H, M, L, and scroll motions
    /// (Ctrl-D/U/F/B/E/Y) are vertical. All other motions are horizontal and
    /// update sticky to the new cursor column.
    #[must_use]
    pub const fn is_vertical(&self) -> bool {
        matches!(
            self,
            Self::Up
                | Self::Down
                | Self::DisplayDown
                | Self::DisplayUp
                | Self::GotoLine
                | Self::GotoFirstLine
                | Self::ScreenHigh
                | Self::ScreenMiddle
                | Self::ScreenLow
                | Self::ScrollHalfDown
                | Self::ScrollHalfUp
                | Self::ScrollFullDown
                | Self::ScrollFullUp
                | Self::ScrollLineDown
                | Self::ScrollLineUp
                // Indent navigation
                | Self::PrevSameIndent
                | Self::NextSameIndent
                | Self::PrevLesserIndent
                | Self::NextLesserIndent
                | Self::PrevGreaterIndent
                | Self::NextGreaterIndent
        )
    }

    /// Check if this motion is a "jump" motion.
    ///
    /// Jump motions should push to the jump list before executing.
    /// Per `:help jump-motions` - includes G, gg, %, *, #, (, ), {, }, H, M, L
    /// Note: n and N are NOT jump motions (only the initial / ? * # are).
    #[must_use]
    pub const fn is_jump_motion(&self) -> bool {
        matches!(
            self,
            Self::GotoLine
                | Self::GotoFirstLine
                | Self::MatchingPair
                | Self::WordSearchForward
                | Self::WordSearchBackward
                | Self::SentenceForward
                | Self::SentenceBackward
                | Self::ParagraphForward
                | Self::ParagraphBackward
                | Self::ScreenHigh
                | Self::ScreenMiddle
                | Self::ScreenLow
                | Self::ChangelistOlder
                | Self::ChangelistNewer
                | Self::PrevUnmatchedBrace
                | Self::NextUnmatchedBrace
                | Self::PrevUnmatchedParen
                | Self::NextUnmatchedParen
                | Self::PrevMethodStart
                | Self::NextMethodStart
                | Self::PrevMethodEnd
                | Self::NextMethodEnd
                | Self::PrevCommentStart
                | Self::NextCommentEnd
                | Self::NextBracketPair
                | Self::PrevBracketPair
                | Self::NextQuotePair
                | Self::PrevQuotePair
                | Self::SearchNext
                | Self::SearchPrev
                | Self::GotoByte
                | Self::SectionForwardStart
                | Self::SectionBackwardStart
                | Self::SectionForwardEnd
                | Self::SectionBackwardEnd
                | Self::NextMark
                | Self::PreviousMark
                | Self::SeekTextObject { .. }
        )
    }

    /// Check if this motion is content-dependent.
    ///
    /// Content-dependent motions read document text to determine their target
    /// position. Position-independent motions are determined solely by cursor
    /// position, line/column counts, or viewport geometry.
    ///
    /// This classification is used by multi-cursor re-execution: position-
    /// independent motions can be replayed as-is, while content-dependent
    /// motions must be re-evaluated per cursor against actual buffer content.
    #[must_use]
    pub const fn is_content_dependent(&self) -> bool {
        match self {
            // --- Content-dependent (true) ---
            // Word/WORD motions: scan for word boundaries
            Self::WordForward
            | Self::WordBackward
            | Self::WordEnd
            | Self::WordEndBackward
            | Self::WORDForward
            | Self::WORDBackward
            | Self::WORDEnd
            | Self::WORDEndBackward
            // Subword motions: scan for camelCase/snake_case boundaries
            | Self::SubwordForward
            | Self::SubwordBackward
            | Self::SubwordEnd
            | Self::SubwordEndBackward
            // Sentence/paragraph: scan for sentence/paragraph boundaries
            | Self::SentenceForward
            | Self::SentenceBackward
            | Self::ParagraphForward
            | Self::ParagraphBackward
            // Section: scan for `{`/`}` at column 0
            | Self::SectionForwardStart
            | Self::SectionBackwardStart
            | Self::SectionForwardEnd
            | Self::SectionBackwardEnd
            // Search: scan buffer for pattern matches
            | Self::SearchNext
            | Self::SearchPrev
            | Self::WordSearchForward
            | Self::WordSearchBackward
            | Self::PartialWordSearchForward
            | Self::PartialWordSearchBackward
            | Self::SearchObjectForward
            | Self::SearchObjectBackward
            // Bracket/matching: scan for bracket pairs
            | Self::MatchingPair
            | Self::PrevUnmatchedBrace
            | Self::NextUnmatchedBrace
            | Self::PrevUnmatchedParen
            | Self::NextUnmatchedParen
            | Self::PrevMethodStart
            | Self::NextMethodStart
            | Self::PrevMethodEnd
            | Self::NextMethodEnd
            | Self::PrevCommentStart
            | Self::NextCommentEnd
            | Self::NextBracketPair
            | Self::PrevBracketPair
            | Self::NextQuotePair
            | Self::PrevQuotePair
            // Line-content-dependent: target depends on line text
            | Self::LineEnd
            | Self::FirstNonBlank
            | Self::LastNonBlank
            | Self::FirstNonBlankLine
            | Self::MiddleOfScreenLine
            | Self::MiddleOfTextLine
            | Self::ScreenLineStart
            | Self::ScreenLineEnd
            | Self::ScreenFirstNonBlank
            // Down/up to first non-blank: line motion + text scan
            | Self::DownFirstNonBlank
            | Self::UpFirstNonBlank
            // Character find repeat: replays f/F/t/T which scan text
            | Self::RepeatFind
            | Self::RepeatFindReverse
            // Indent navigation: compare indentation levels
            | Self::PrevSameIndent
            | Self::NextSameIndent
            | Self::PrevLesserIndent
            | Self::NextLesserIndent
            | Self::PrevGreaterIndent
            | Self::NextGreaterIndent
            // Text object seeking: scan for text object boundaries
            | Self::SeekTextObject { .. }
            // Custom: unknown semantics, assume content-dependent
            | Self::Custom(_) => true,

            // --- Position-independent (false) ---
            // Character motions: fixed offset from cursor
            Self::Left
            | Self::Right
            | Self::Space
            // Line motions: fixed vertical offset
            | Self::Up
            | Self::Down
            | Self::DisplayDown
            | Self::DisplayUp
            // Document position: go to absolute line/column/byte
            | Self::GotoLine
            | Self::GotoFirstLine
            | Self::GoToColumn
            | Self::GotoByte
            // Screen position: viewport-relative, not content-dependent
            | Self::ScreenHigh
            | Self::ScreenMiddle
            | Self::ScreenLow
            // Line start: always column 0
            | Self::LineStart
            // Scroll: viewport movement, not content-dependent
            | Self::ScrollHalfDown
            | Self::ScrollHalfUp
            | Self::ScrollFullDown
            | Self::ScrollFullUp
            | Self::ScrollLineDown
            | Self::ScrollLineUp
            // Changelist: jump to stored positions
            | Self::ChangelistOlder
            | Self::ChangelistNewer
            // Mark navigation: jump to stored positions
            | Self::NextMark
            | Self::PreviousMark => false,
        }
    }

    /// Check if this motion is infallible.
    ///
    /// Infallible motions always produce a valid cursor position regardless of
    /// document content because they clamp to document or line bounds. They
    /// never fail to find a target (unlike search or find-char motions).
    #[must_use]
    pub const fn is_infallible(&self) -> bool {
        matches!(
            self,
            Self::LineStart
                | Self::LineEnd
                | Self::FirstNonBlank
                | Self::FirstNonBlankLine
                | Self::GotoFirstLine
                | Self::GotoLine
                | Self::GoToColumn
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_h_resolves_to_left() {
        use crate::keymap::KeyEvent;
        let key = KeyEvent::ctrl('h');
        assert_eq!(Motion::from_key_event(&key), Some(Motion::Left));
    }

    #[test]
    fn ctrl_j_resolves_to_down() {
        use crate::keymap::KeyEvent;
        let key = KeyEvent::ctrl('j');
        assert_eq!(Motion::from_key_event(&key), Some(Motion::Down));
    }

    // === is_infallible tests ===

    #[test]
    fn line_start_is_infallible() {
        assert!(Motion::LineStart.is_infallible());
    }

    #[test]
    fn line_end_is_infallible() {
        assert!(Motion::LineEnd.is_infallible());
    }

    #[test]
    fn first_non_blank_is_infallible() {
        assert!(Motion::FirstNonBlank.is_infallible());
    }

    #[test]
    fn first_non_blank_line_is_infallible() {
        assert!(Motion::FirstNonBlankLine.is_infallible());
    }

    #[test]
    fn goto_first_line_is_infallible() {
        assert!(Motion::GotoFirstLine.is_infallible());
    }

    #[test]
    fn goto_line_is_infallible() {
        assert!(Motion::GotoLine.is_infallible());
    }

    #[test]
    fn go_to_column_is_infallible() {
        assert!(Motion::GoToColumn.is_infallible());
    }

    #[test]
    fn word_forward_is_not_infallible() {
        assert!(!Motion::WordForward.is_infallible());
    }

    #[test]
    fn search_next_is_not_infallible() {
        assert!(!Motion::SearchNext.is_infallible());
    }

    #[test]
    fn search_prev_is_not_infallible() {
        assert!(!Motion::SearchPrev.is_infallible());
    }

    #[test]
    fn matching_pair_is_not_infallible() {
        assert!(!Motion::MatchingPair.is_infallible());
    }

    #[test]
    fn paragraph_forward_is_not_infallible() {
        assert!(!Motion::ParagraphForward.is_infallible());
    }

    // === is_content_dependent tests ===

    // Content-dependent motions (true)

    #[test]
    fn word_motions_are_content_dependent() {
        assert!(Motion::WordForward.is_content_dependent());
        assert!(Motion::WordBackward.is_content_dependent());
        assert!(Motion::WordEnd.is_content_dependent());
        assert!(Motion::WordEndBackward.is_content_dependent());
        assert!(Motion::WORDForward.is_content_dependent());
        assert!(Motion::WORDBackward.is_content_dependent());
        assert!(Motion::WORDEnd.is_content_dependent());
        assert!(Motion::WORDEndBackward.is_content_dependent());
    }

    #[test]
    fn subword_motions_are_content_dependent() {
        assert!(Motion::SubwordForward.is_content_dependent());
        assert!(Motion::SubwordBackward.is_content_dependent());
        assert!(Motion::SubwordEnd.is_content_dependent());
        assert!(Motion::SubwordEndBackward.is_content_dependent());
    }

    #[test]
    fn sentence_paragraph_are_content_dependent() {
        assert!(Motion::SentenceForward.is_content_dependent());
        assert!(Motion::SentenceBackward.is_content_dependent());
        assert!(Motion::ParagraphForward.is_content_dependent());
        assert!(Motion::ParagraphBackward.is_content_dependent());
    }

    #[test]
    fn section_motions_are_content_dependent() {
        assert!(Motion::SectionForwardStart.is_content_dependent());
        assert!(Motion::SectionBackwardStart.is_content_dependent());
        assert!(Motion::SectionForwardEnd.is_content_dependent());
        assert!(Motion::SectionBackwardEnd.is_content_dependent());
    }

    #[test]
    fn search_motions_are_content_dependent() {
        assert!(Motion::SearchNext.is_content_dependent());
        assert!(Motion::SearchPrev.is_content_dependent());
        assert!(Motion::WordSearchForward.is_content_dependent());
        assert!(Motion::WordSearchBackward.is_content_dependent());
        assert!(Motion::PartialWordSearchForward.is_content_dependent());
        assert!(Motion::PartialWordSearchBackward.is_content_dependent());
        assert!(Motion::SearchObjectForward.is_content_dependent());
        assert!(Motion::SearchObjectBackward.is_content_dependent());
    }

    #[test]
    fn bracket_motions_are_content_dependent() {
        assert!(Motion::MatchingPair.is_content_dependent());
        assert!(Motion::PrevUnmatchedBrace.is_content_dependent());
        assert!(Motion::NextUnmatchedBrace.is_content_dependent());
        assert!(Motion::PrevUnmatchedParen.is_content_dependent());
        assert!(Motion::NextUnmatchedParen.is_content_dependent());
        assert!(Motion::PrevMethodStart.is_content_dependent());
        assert!(Motion::NextMethodStart.is_content_dependent());
        assert!(Motion::PrevMethodEnd.is_content_dependent());
        assert!(Motion::NextMethodEnd.is_content_dependent());
        assert!(Motion::PrevCommentStart.is_content_dependent());
        assert!(Motion::NextCommentEnd.is_content_dependent());
        assert!(Motion::NextBracketPair.is_content_dependent());
        assert!(Motion::PrevBracketPair.is_content_dependent());
        assert!(Motion::NextQuotePair.is_content_dependent());
        assert!(Motion::PrevQuotePair.is_content_dependent());
    }

    #[test]
    fn line_content_motions_are_content_dependent() {
        assert!(Motion::LineEnd.is_content_dependent());
        assert!(Motion::FirstNonBlank.is_content_dependent());
        assert!(Motion::LastNonBlank.is_content_dependent());
        assert!(Motion::FirstNonBlankLine.is_content_dependent());
        assert!(Motion::MiddleOfScreenLine.is_content_dependent());
        assert!(Motion::MiddleOfTextLine.is_content_dependent());
        assert!(Motion::ScreenLineStart.is_content_dependent());
        assert!(Motion::ScreenLineEnd.is_content_dependent());
        assert!(Motion::ScreenFirstNonBlank.is_content_dependent());
    }

    #[test]
    fn down_up_first_non_blank_are_content_dependent() {
        assert!(Motion::DownFirstNonBlank.is_content_dependent());
        assert!(Motion::UpFirstNonBlank.is_content_dependent());
    }

    #[test]
    fn repeat_find_motions_are_content_dependent() {
        assert!(Motion::RepeatFind.is_content_dependent());
        assert!(Motion::RepeatFindReverse.is_content_dependent());
    }

    #[test]
    fn indent_motions_are_content_dependent() {
        assert!(Motion::PrevSameIndent.is_content_dependent());
        assert!(Motion::NextSameIndent.is_content_dependent());
        assert!(Motion::PrevLesserIndent.is_content_dependent());
        assert!(Motion::NextLesserIndent.is_content_dependent());
        assert!(Motion::PrevGreaterIndent.is_content_dependent());
        assert!(Motion::NextGreaterIndent.is_content_dependent());
    }

    #[test]
    fn seek_text_object_is_content_dependent() {
        use crate::grammar::types::TextObjectKind;
        use crate::primitives::Direction;
        assert!(Motion::SeekTextObject {
            kind: TextObjectKind::Word,
            direction: Direction::Forward,
        }
        .is_content_dependent());
    }

    #[test]
    fn custom_motion_is_content_dependent() {
        assert!(Motion::Custom(42).is_content_dependent());
    }

    // Position-independent motions (false)

    #[test]
    fn character_motions_are_not_content_dependent() {
        assert!(!Motion::Left.is_content_dependent());
        assert!(!Motion::Right.is_content_dependent());
        assert!(!Motion::Space.is_content_dependent());
    }

    #[test]
    fn line_motions_are_not_content_dependent() {
        assert!(!Motion::Up.is_content_dependent());
        assert!(!Motion::Down.is_content_dependent());
        assert!(!Motion::DisplayDown.is_content_dependent());
        assert!(!Motion::DisplayUp.is_content_dependent());
    }

    #[test]
    fn goto_motions_are_not_content_dependent() {
        assert!(!Motion::GotoLine.is_content_dependent());
        assert!(!Motion::GotoFirstLine.is_content_dependent());
        assert!(!Motion::GoToColumn.is_content_dependent());
        assert!(!Motion::GotoByte.is_content_dependent());
    }

    #[test]
    fn screen_position_motions_are_not_content_dependent() {
        assert!(!Motion::ScreenHigh.is_content_dependent());
        assert!(!Motion::ScreenMiddle.is_content_dependent());
        assert!(!Motion::ScreenLow.is_content_dependent());
    }

    #[test]
    fn line_start_is_not_content_dependent() {
        assert!(!Motion::LineStart.is_content_dependent());
    }

    #[test]
    fn scroll_motions_are_not_content_dependent() {
        assert!(!Motion::ScrollHalfDown.is_content_dependent());
        assert!(!Motion::ScrollHalfUp.is_content_dependent());
        assert!(!Motion::ScrollFullDown.is_content_dependent());
        assert!(!Motion::ScrollFullUp.is_content_dependent());
        assert!(!Motion::ScrollLineDown.is_content_dependent());
        assert!(!Motion::ScrollLineUp.is_content_dependent());
    }

    #[test]
    fn changelist_motions_are_not_content_dependent() {
        assert!(!Motion::ChangelistOlder.is_content_dependent());
        assert!(!Motion::ChangelistNewer.is_content_dependent());
    }

    #[test]
    fn mark_motions_are_not_content_dependent() {
        assert!(!Motion::NextMark.is_content_dependent());
        assert!(!Motion::PreviousMark.is_content_dependent());
    }
}
