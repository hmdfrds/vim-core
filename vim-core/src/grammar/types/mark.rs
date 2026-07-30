//! Mark types.
//!
//! Mark commands for setting and jumping to marks.

use strum::{Display, EnumIter};

/// Mark command type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MarkType {
    /// Set mark (m{mark})
    Set,
    /// Jump to mark line ('{mark})
    JumpLine,
    /// Jump to mark exact position (`{mark})
    JumpExact,
}

impl MarkType {
    /// Return the trigger key character for this mark command.
    ///
    /// Used by `InputState::pending_display()` for showcmd display.
    #[must_use]
    pub const fn key_char(&self) -> char {
        match self {
            Self::Set => 'm',
            Self::JumpLine => '\'',
            Self::JumpExact => '`',
        }
    }

    /// Create from key character.
    #[must_use]
    pub const fn from_char(c: char) -> Option<Self> {
        match c {
            'm' => Some(Self::Set),
            '\'' => Some(Self::JumpLine),
            '`' => Some(Self::JumpExact),
            _ => None,
        }
    }
}
