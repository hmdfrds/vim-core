//! Replace mode character tracking.
//!
//! `ReplacedChar` represents what was at the cursor position when Replace mode
//! overwrote it. Stored on a per-keystroke stack so backspace can undo overwrites.

/// An entry on the Replace mode undo stack.
///
/// When Replace mode (`R`) overwrites a character, the original is pushed here.
/// When backspace is pressed, this stack is popped to restore the original.
///
/// Semantically distinct from `Option<char>`: `InsertedAtEol` is NOT "no character" —
/// it means a character WAS inserted (because the cursor was past end-of-line),
/// and backspace should delete it rather than restore anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ReplacedChar {
    /// A specific character was overwritten. Backspace restores it.
    Replaced(char),
    /// Cursor was past end-of-line — a character was inserted, not replaced.
    /// Backspace deletes the inserted character; nothing to restore.
    InsertedAtEol,
    /// A character was overwritten in virtual replace mode (`gR`).
    ///
    /// Stores both the original character and its screen column width.
    /// When backspace restores it, the replacement must fill the same
    /// screen width (e.g., a tab might occupy 4-8 screen columns).
    VirtualReplaced {
        /// The original character that was overwritten.
        original: char,
        /// Screen column width of the original character (tab-expanded).
        screen_width: u8,
    },
    /// A line boundary was crossed (Enter pressed in Replace mode).
    ///
    /// In Vim's Replace mode, Enter inserts a newline without overwriting
    /// the character under the cursor. When backspace encounters this
    /// sentinel, it joins the current line back to the previous line
    /// (deleting the newline and any autoindent on the new line).
    LineBoundary,
}

impl ReplacedChar {
    /// Convert an `Option<char>` to `ReplacedChar`.
    ///
    /// `Some(c)` → `Replaced(c)`, `None` → `InsertedAtEol`.
    #[inline]
    #[must_use]
    pub const fn from_option(ch: Option<char>) -> Self {
        match ch {
            Some(c) => Self::Replaced(c),
            None => Self::InsertedAtEol,
        }
    }

    /// Returns the replaced character, or `None` if inserted-at-EOL or line boundary.
    #[inline]
    #[must_use]
    pub const fn char(self) -> Option<char> {
        match self {
            Self::Replaced(c) | Self::VirtualReplaced { original: c, .. } => Some(c),
            Self::InsertedAtEol | Self::LineBoundary => None,
        }
    }

    /// Returns the screen width for virtual replace entries, or `None`.
    #[inline]
    #[must_use]
    pub const fn screen_width(self) -> Option<u8> {
        match self {
            Self::VirtualReplaced { screen_width, .. } => Some(screen_width),
            Self::Replaced(_) | Self::InsertedAtEol | Self::LineBoundary => None,
        }
    }

    /// Create a virtual replace entry with the original char and its screen width.
    #[inline]
    #[must_use]
    pub const fn virtual_replaced(original: char, screen_width: u8) -> Self {
        Self::VirtualReplaced {
            original,
            screen_width,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_char_returns_some() {
        let rc = ReplacedChar::Replaced('x');
        assert_eq!(rc.char(), Some('x'));
        assert_eq!(rc.screen_width(), None);
    }

    #[test]
    fn inserted_at_eol_returns_none() {
        let rc = ReplacedChar::InsertedAtEol;
        assert_eq!(rc.char(), None);
        assert_eq!(rc.screen_width(), None);
    }

    #[test]
    fn virtual_replaced_char_and_width() {
        let rc = ReplacedChar::virtual_replaced('\t', 4);
        assert_eq!(rc.char(), Some('\t'));
        assert_eq!(rc.screen_width(), Some(4));
    }

    #[test]
    fn from_option_some() {
        let rc = ReplacedChar::from_option(Some('a'));
        assert_eq!(rc.char(), Some('a'));
    }

    #[test]
    fn from_option_none() {
        let rc = ReplacedChar::from_option(None);
        assert_eq!(rc.char(), None);
    }

    #[test]
    fn line_boundary_returns_none() {
        let rc = ReplacedChar::LineBoundary;
        assert_eq!(rc.char(), None);
        assert_eq!(rc.screen_width(), None);
    }
}
