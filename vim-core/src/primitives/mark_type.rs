//! Mark name types for vim-core.
//!
//! A validated mark name, analogous to `RegisterName`.
//! Marks associate a single character with a buffer position.

use derive_more::Display;

/// A validated mark name.
///
/// Valid marks: a-z (local), A-Z (global), 0-9 (numbered/file),
/// and special marks: `'`, `` ` ``, `.`, `^`, `[`, `]`, `<`, `>`, `"`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[display(fmt = "{_0}")]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct MarkName(char);

impl MarkName {
    // === Special mark constants ===

    /// Previous jump position (`''` / `` `` ``).
    pub const PREV_JUMP: Self = Self('\'');
    /// Previous jump exact position (`` ` ``).
    pub const PREV_JUMP_EXACT: Self = Self('`');
    /// Last change position (`.`).
    pub const LAST_CHANGE: Self = Self('.');
    /// Insert-mode stop position (`^`).
    pub const INSERT_STOP: Self = Self('^');
    /// Start of last change or yank (`[`).
    pub const CHANGE_START: Self = Self('[');
    /// End of last change or yank (`]`).
    pub const CHANGE_END: Self = Self(']');
    /// Start of last visual selection (`<`).
    pub const VISUAL_START: Self = Self('<');
    /// End of last visual selection (`>`).
    pub const VISUAL_END: Self = Self('>');
    /// Last exited position in buffer (`"`).
    pub const LAST_POSITION: Self = Self('"');

    /// Create a new mark name, returning `None` if the character is invalid.
    ///
    /// Valid characters: a-z, A-Z, 0-9, ', `, ., ^, \[, \], <, >, "
    #[inline]
    #[must_use]
    pub const fn new(c: char) -> Option<Self> {
        if Self::is_valid(c) {
            Some(Self(c))
        } else {
            None
        }
    }

    /// Create a mark name without validation (const-compatible).
    ///
    /// The caller must ensure `c` is a valid mark character.
    /// Use only for well-known constants defined within this crate.
    #[inline]
    #[must_use]
    #[allow(dead_code, reason = "reserved for future mark operations")]
    pub(crate) const fn new_unchecked(c: char) -> Self {
        Self(c)
    }

    /// Create a local mark name from an index 0..26 (a=0, b=1, ..., z=25).
    ///
    /// # Panics
    ///
    /// Panics if `i >= 26`.
    #[inline]
    #[must_use]
    pub(crate) fn from_local_index(i: usize) -> Self {
        assert!(i < 26, "local mark index must be 0..26, got {i}");
        // The assert above guarantees the conversion succeeds.
        let offset = u8::try_from(i).unwrap_or(0);
        Self::new_unchecked(char::from(b'a' + offset))
    }

    /// Check if a character is a valid mark name.
    #[inline]
    #[must_use]
    pub const fn is_valid(c: char) -> bool {
        matches!(
            c,
            'a'..='z'
                | 'A'..='Z'
                | '0'..='9'
                | '\''
                | '`'
                | '.'
                | '^'
                | '['
                | ']'
                | '<'
                | '>'
                | '"'
        )
    }

    /// Get the underlying character.
    #[inline]
    #[must_use]
    pub const fn char(self) -> char {
        self.0
    }

    /// Local mark (a-z) — buffer-specific.
    #[inline]
    #[must_use]
    pub const fn is_local(self) -> bool {
        self.0.is_ascii_lowercase()
    }

    /// Global mark (A-Z) — cross-buffer.
    #[inline]
    #[must_use]
    pub const fn is_global(self) -> bool {
        self.0.is_ascii_uppercase()
    }

    /// Special mark (', `, ., ^, \[, \], <, >, ").
    #[inline]
    #[must_use]
    pub const fn is_special(self) -> bool {
        matches!(self.0, '\'' | '`' | '.' | '^' | '[' | ']' | '<' | '>' | '"')
    }

    /// Settable by the user via `m{mark}` — alphabetic marks only.
    #[inline]
    #[must_use]
    pub const fn is_settable(self) -> bool {
        self.0.is_ascii_alphabetic()
    }

    /// Numbered mark (0-9) — file marks set by viminfo/shada.
    #[inline]
    #[must_use]
    pub const fn is_numbered(self) -> bool {
        self.0.is_ascii_digit()
    }

    /// Visual selection mark (`<` or `>`).
    ///
    /// Neovim does NOT adjust these on text edits — they preserve the
    /// original selection positions for `gv` reselection.
    #[inline]
    #[must_use]
    pub const fn is_visual(self) -> bool {
        matches!(self.0, '<' | '>')
    }

    /// Insert-stop mark (`^`).
    ///
    /// Neovim does NOT adjust this mark on subsequent text edits.
    /// It records the byte position where insert mode was last exited
    /// and is only updated by a new insert-mode exit (`exit_finalize`).
    #[inline]
    #[must_use]
    pub const fn is_insert_stop(self) -> bool {
        self.0 == '^'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_marks() {
        for c in 'a'..='z' {
            let m = MarkName::new(c).unwrap();
            assert!(m.is_local());
            assert!(!m.is_global());
            assert!(!m.is_special());
            assert!(m.is_settable());
            assert_eq!(m.char(), c);
        }
    }

    #[test]
    fn test_global_marks() {
        for c in 'A'..='Z' {
            let m = MarkName::new(c).unwrap();
            assert!(m.is_global());
            assert!(!m.is_local());
            assert!(!m.is_special());
            assert!(m.is_settable());
        }
    }

    #[test]
    fn test_numbered_marks() {
        for c in '0'..='9' {
            let m = MarkName::new(c).unwrap();
            assert!(m.is_numbered());
            assert!(!m.is_local());
            assert!(!m.is_global());
            assert!(!m.is_settable());
        }
    }

    #[test]
    fn test_special_marks() {
        let specials = ['\'', '`', '.', '^', '[', ']', '<', '>', '"'];
        for c in specials {
            let m = MarkName::new(c).unwrap();
            assert!(m.is_special(), "expected {} to be special", c);
            assert!(!m.is_local());
            assert!(!m.is_global());
            assert!(!m.is_settable());
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(MarkName::PREV_JUMP.char(), '\'');
        assert_eq!(MarkName::PREV_JUMP_EXACT.char(), '`');
        assert_eq!(MarkName::LAST_CHANGE.char(), '.');
        assert_eq!(MarkName::INSERT_STOP.char(), '^');
        assert_eq!(MarkName::CHANGE_START.char(), '[');
        assert_eq!(MarkName::CHANGE_END.char(), ']');
        assert_eq!(MarkName::VISUAL_START.char(), '<');
        assert_eq!(MarkName::VISUAL_END.char(), '>');
        assert_eq!(MarkName::LAST_POSITION.char(), '"');
    }

    #[test]
    fn test_invalid_marks_rejected() {
        let invalids = ['!', '@', '#', '$', '~', ' ', '\n', '\t', '{', '}'];
        for c in invalids {
            assert!(MarkName::new(c).is_none(), "expected {} to be invalid", c);
        }
    }

    #[test]
    fn test_display() {
        let m = MarkName::new('a').unwrap();
        assert_eq!(format!("{}", m), "a");
    }

    #[test]
    fn test_debug() {
        let m = MarkName::new('a').unwrap();
        assert_eq!(format!("{:?}", m), "MarkName('a')");
    }

    #[test]
    fn test_mutual_exclusion() {
        // Each mark belongs to exactly one category
        for c in ('a'..='z').chain('A'..='Z').chain('0'..='9') {
            let m = MarkName::new(c).unwrap();
            let flags = [m.is_local(), m.is_global(), m.is_special(), m.is_numbered()];
            assert_eq!(
                flags.iter().filter(|&&f| f).count(),
                1,
                "mark '{}' should be in exactly one category",
                c
            );
        }
        for c in ['\'', '`', '.', '^', '[', ']', '<', '>', '"'] {
            let m = MarkName::new(c).unwrap();
            let flags = [m.is_local(), m.is_global(), m.is_special(), m.is_numbered()];
            assert_eq!(
                flags.iter().filter(|&&f| f).count(),
                1,
                "mark '{}' should be in exactly one category",
                c
            );
        }
    }
}
