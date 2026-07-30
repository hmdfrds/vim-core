//! Motion inclusivity for operators.
//!
//! Exhaustive matching - the compiler catches missing cases.
//! This file defines how each motion affects operator ranges.
//!
//! # Vim Inclusivity Rules
//!
//! - **Inclusive**: Last character IS operated on (e, f, $)
//! - **Exclusive**: Last character is NOT operated on (w, t, 0)
//! - **Linewise**: Extends to full lines (j, k, G)
//!
//! # Why This File Exists
//!
//! Without exhaustive matching, new motions default to wrong behavior:
//! ```text
//! // DANGEROUS: new motions silently default to false
//! let inclusive = match motion {
//!     Motion::WordEnd => true,
//!     _ => false,
//! };
//! ```
//!
//! With exhaustive matching, compiler forces you to handle new motions:
//! ```text
//! // SAFE: compiler error if new motion added
//! let inclusive = motion_inclusivity(motion);
//! ```

use crate::grammar::types::Motion;
use crate::primitives::FindDirection;
use crate::primitives::MotionInclusivity;

/// Get the inclusivity for a motion.
///
/// This is EXHAUSTIVE - compiler will error if new Motion variants are added
/// without being handled here. That is intentional: a new motion must state
/// its inclusivity rather than silently inherit a default.
///
/// `last_find_direction` is needed for `RepeatFind`/`RepeatFindReverse` to
/// inherit the inclusivity of the original find (f/F = inclusive, t/T = exclusive).
///
/// # Vim Documentation Reference
///
/// | Motion | Inclusive | Reason |
/// |--------|-----------|--------|
/// | e, E | ✅ | End of word, cursor ON last char |
/// | w, W, b, B | ❌ | Start of next/prev word |
/// | $ | ✅ | End of line, cursor ON last char |
/// | 0, ^ | ❌ | Start of line |
/// | f, F | ✅ | Find includes target char |
/// | t, T | ❌ | Till excludes target char |
/// | j, k, G, gg | linewise | Full lines |
#[must_use]
pub const fn motion_inclusivity(motion: Motion) -> MotionInclusivity {
    motion_inclusivity_with_find(motion, None)
}

/// Get inclusivity with optional last-find context for `;`/`,`.
#[must_use]
pub const fn motion_inclusivity_with_find(
    motion: Motion,
    last_find_direction: Option<FindDirection>,
) -> MotionInclusivity {
    match motion {
        // ===== INCLUSIVE MOTIONS =====
        // Cursor lands ON the last character to be changed

        // Word ends - cursor on last char of word
        Motion::WordEnd => MotionInclusivity::Inclusive,
        Motion::WORDEnd => MotionInclusivity::Inclusive,
        Motion::WordEndBackward => MotionInclusivity::Inclusive,
        Motion::WORDEndBackward => MotionInclusivity::Inclusive,

        // Line end - cursor on last char of line
        Motion::LineEnd => MotionInclusivity::Inclusive,
        Motion::LastNonBlank => MotionInclusivity::Inclusive,

        // Matching pair - includes both brackets
        Motion::MatchingPair => MotionInclusivity::Inclusive,

        // Find repeats inherit the original find's inclusivity:
        // f/F → inclusive, t/T → exclusive
        Motion::RepeatFind | Motion::RepeatFindReverse => {
            match last_find_direction {
                Some(FindDirection::FindForward | FindDirection::FindBackward) => {
                    MotionInclusivity::Inclusive
                }
                Some(FindDirection::TillForward | FindDirection::TillBackward) => {
                    MotionInclusivity::Exclusive
                }
                Some(FindDirection::SneakForward | FindDirection::SneakBackward) => {
                    MotionInclusivity::Exclusive
                }
                None => MotionInclusivity::Inclusive, // fallback when no last find
            }
        }

        // ===== EXCLUSIVE MOTIONS =====
        // Cursor lands on first character NOT to be changed

        // Word starts - cursor on first char of next/prev word
        Motion::WordForward => MotionInclusivity::Exclusive,
        Motion::WORDForward => MotionInclusivity::Exclusive,
        Motion::WordBackward => MotionInclusivity::Exclusive,
        Motion::WORDBackward => MotionInclusivity::Exclusive,

        // Line starts - cursor on first char of line
        Motion::LineStart => MotionInclusivity::Exclusive,
        Motion::FirstNonBlank => MotionInclusivity::Exclusive,

        // Character movement
        Motion::Left => MotionInclusivity::Exclusive,
        Motion::Right | Motion::Space => MotionInclusivity::Exclusive,

        // Display-line movement (gj, gk) — exclusive per :help gj
        Motion::DisplayDown => MotionInclusivity::Exclusive,
        Motion::DisplayUp => MotionInclusivity::Exclusive,

        // Sentence movement
        Motion::SentenceForward => MotionInclusivity::Exclusive,
        Motion::SentenceBackward => MotionInclusivity::Exclusive,

        // Search motions (except word search - those position at start)
        Motion::SearchNext => MotionInclusivity::Exclusive,
        Motion::SearchPrev => MotionInclusivity::Exclusive,
        Motion::WordSearchForward => MotionInclusivity::Exclusive,
        Motion::WordSearchBackward => MotionInclusivity::Exclusive,

        // ===== LINEWISE MOTIONS =====
        // Extend to full lines regardless of cursor position

        // Vertical movement
        Motion::Up => MotionInclusivity::Linewise,
        Motion::Down => MotionInclusivity::Linewise,
        Motion::DownFirstNonBlank => MotionInclusivity::Linewise,
        Motion::UpFirstNonBlank => MotionInclusivity::Linewise,
        Motion::FirstNonBlankLine => MotionInclusivity::Linewise,

        // Document navigation
        Motion::GotoLine => MotionInclusivity::Linewise,
        Motion::GotoFirstLine => MotionInclusivity::Linewise,

        // Screen positions
        Motion::ScreenHigh => MotionInclusivity::Linewise,
        Motion::ScreenMiddle => MotionInclusivity::Linewise,
        Motion::ScreenLow => MotionInclusivity::Linewise,

        // Section motions ([[, ]], [], ][) — exclusive per :help ]]
        Motion::SectionForwardStart => MotionInclusivity::Exclusive,
        Motion::SectionBackwardStart => MotionInclusivity::Exclusive,
        Motion::SectionForwardEnd => MotionInclusivity::Exclusive,
        Motion::SectionBackwardEnd => MotionInclusivity::Exclusive,

        // Paragraph movement (exclusive in Vim, not linewise)
        // Per :help } - paragraph motions are exclusive
        Motion::ParagraphForward => MotionInclusivity::Exclusive,
        Motion::ParagraphBackward => MotionInclusivity::Exclusive,

        // Column motion (|) - exclusive
        Motion::GoToColumn => MotionInclusivity::Exclusive,

        // Screen-line motions - same as line start/end
        Motion::ScreenLineStart => MotionInclusivity::Exclusive,
        Motion::ScreenLineEnd => MotionInclusivity::Inclusive,

        // Scroll motions - linewise (move to first non-blank of target line)
        Motion::ScrollHalfDown => MotionInclusivity::Linewise,
        Motion::ScrollHalfUp => MotionInclusivity::Linewise,
        Motion::ScrollFullDown => MotionInclusivity::Linewise,
        Motion::ScrollFullUp => MotionInclusivity::Linewise,
        Motion::ScrollLineDown => MotionInclusivity::Linewise,
        Motion::ScrollLineUp => MotionInclusivity::Linewise,

        // Changelist motions (g;, g,) - linewise (jump to change position)
        Motion::ChangelistOlder => MotionInclusivity::Linewise,
        Motion::ChangelistNewer => MotionInclusivity::Linewise,

        // Search object motions (gn, gN) - inclusive (select the match)
        Motion::SearchObjectForward => MotionInclusivity::Inclusive,
        Motion::SearchObjectBackward => MotionInclusivity::Inclusive,

        // Unmatched bracket motions ([{, ]}, [(, ])) - exclusive per :help [{
        Motion::PrevUnmatchedBrace => MotionInclusivity::Exclusive,
        Motion::NextUnmatchedBrace => MotionInclusivity::Exclusive,
        Motion::PrevUnmatchedParen => MotionInclusivity::Exclusive,
        Motion::NextUnmatchedParen => MotionInclusivity::Exclusive,

        // Screen first non-blank (g^) - exclusive like ^
        Motion::ScreenFirstNonBlank => MotionInclusivity::Exclusive,

        // Misc motions
        Motion::MiddleOfScreenLine => MotionInclusivity::Exclusive,
        Motion::MiddleOfTextLine => MotionInclusivity::Exclusive,
        Motion::GotoByte => MotionInclusivity::Exclusive,

        // Method boundary motions - exclusive per :help [m
        Motion::PrevMethodStart => MotionInclusivity::Exclusive,
        Motion::NextMethodStart => MotionInclusivity::Exclusive,
        Motion::PrevMethodEnd => MotionInclusivity::Exclusive,
        Motion::NextMethodEnd => MotionInclusivity::Exclusive,

        // Comment navigation - exclusive
        Motion::PrevCommentStart => MotionInclusivity::Exclusive,
        Motion::NextCommentEnd => MotionInclusivity::Exclusive,

        // Bracket/quote pair navigation - exclusive (like unmatched bracket motions)
        Motion::NextBracketPair => MotionInclusivity::Exclusive,
        Motion::PrevBracketPair => MotionInclusivity::Exclusive,
        Motion::NextQuotePair => MotionInclusivity::Exclusive,
        Motion::PrevQuotePair => MotionInclusivity::Exclusive,

        // Partial word search (g*, g#) - exclusive like n/N
        Motion::PartialWordSearchForward => MotionInclusivity::Exclusive,
        Motion::PartialWordSearchBackward => MotionInclusivity::Exclusive,

        // Subword motions — same inclusivity as word motions.
        Motion::SubwordForward => MotionInclusivity::Exclusive,
        Motion::SubwordBackward => MotionInclusivity::Exclusive,
        Motion::SubwordEnd => MotionInclusivity::Inclusive,
        Motion::SubwordEndBackward => MotionInclusivity::Inclusive,

        // Mark navigation — linewise (jump to mark position's line).
        Motion::NextMark => MotionInclusivity::Linewise,
        Motion::PreviousMark => MotionInclusivity::Linewise,

        // Indent navigation — linewise (same as method boundary motions).
        Motion::PrevSameIndent => MotionInclusivity::Linewise,
        Motion::NextSameIndent => MotionInclusivity::Linewise,
        Motion::PrevLesserIndent => MotionInclusivity::Linewise,
        Motion::NextLesserIndent => MotionInclusivity::Linewise,
        Motion::PrevGreaterIndent => MotionInclusivity::Linewise,
        Motion::NextGreaterIndent => MotionInclusivity::Linewise,

        // Text object seeking — exclusive (cursor lands on start of found object).
        Motion::SeekTextObject { .. } => MotionInclusivity::Exclusive,

        // Custom (host-registered) motions - exclusive by default.
        // Host providers return the target position; the dispatch layer
        // treats it as exclusive (operator doesn't include the target byte).
        Motion::Custom(_) => MotionInclusivity::Exclusive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::MotionInclusivity;

    #[test]
    fn test_all_motions_have_inclusivity() {
        // Exhaustive coverage is guaranteed by the compiler: `motion_inclusivity_with_find`
        // uses a match without wildcards, so adding a new Motion variant will cause a
        // compile error until handled. This test simply validates a representative custom motion.
        let _ = motion_inclusivity(Motion::Custom(42));
    }

    #[test]
    fn test_word_end_is_inclusive() {
        assert_eq!(
            motion_inclusivity(Motion::WordEnd),
            MotionInclusivity::Inclusive
        );
        assert_eq!(
            motion_inclusivity(Motion::WORDEnd),
            MotionInclusivity::Inclusive
        );
    }

    #[test]
    fn test_word_forward_is_exclusive() {
        assert_eq!(
            motion_inclusivity(Motion::WordForward),
            MotionInclusivity::Exclusive
        );
        assert_eq!(
            motion_inclusivity(Motion::WORDForward),
            MotionInclusivity::Exclusive
        );
    }

    #[test]
    fn test_vertical_is_linewise() {
        assert_eq!(motion_inclusivity(Motion::Up), MotionInclusivity::Linewise);
        assert_eq!(
            motion_inclusivity(Motion::Down),
            MotionInclusivity::Linewise
        );
        assert_eq!(
            motion_inclusivity(Motion::GotoLine),
            MotionInclusivity::Linewise
        );
    }

    #[test]
    fn test_line_end_is_inclusive() {
        assert_eq!(
            motion_inclusivity(Motion::LineEnd),
            MotionInclusivity::Inclusive
        );
    }

    #[test]
    fn test_line_start_is_exclusive() {
        assert_eq!(
            motion_inclusivity(Motion::LineStart),
            MotionInclusivity::Exclusive
        );
        assert_eq!(
            motion_inclusivity(Motion::FirstNonBlank),
            MotionInclusivity::Exclusive
        );
    }
}
