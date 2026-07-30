//! Character command types.
//!
//! Commands that await a character argument (f, F, t, T, r).

use strum::{Display, EnumIter};

/// Character command type (commands that await a character argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CharCommand {
    /// Find forward (f{char})
    FindForward,
    /// Find backward (F{char})
    FindBackward,
    /// Till forward (t{char})
    TillForward,
    /// Till backward (T{char})
    TillBackward,
    /// Replace character (r{char})
    Replace,
}

impl CharCommand {
    /// Return the key character for this command.
    ///
    /// Used by `InputState::pending_display()` for showcmd display.
    #[must_use]
    pub const fn key_char(&self) -> char {
        match self {
            Self::FindForward => 'f',
            Self::FindBackward => 'F',
            Self::TillForward => 't',
            Self::TillBackward => 'T',
            Self::Replace => 'r',
        }
    }

    /// Create from key character.
    #[must_use]
    pub const fn from_char(c: char) -> Option<Self> {
        match c {
            'f' => Some(Self::FindForward),
            'F' => Some(Self::FindBackward),
            't' => Some(Self::TillForward),
            'T' => Some(Self::TillBackward),
            'r' => Some(Self::Replace),
            _ => None,
        }
    }

    /// Check if this is a find command (f/F).
    #[must_use]
    pub const fn is_find(&self) -> bool {
        matches!(self, Self::FindForward | Self::FindBackward)
    }

    /// Check if this is a till command (t/T).
    #[must_use]
    pub const fn is_till(&self) -> bool {
        matches!(self, Self::TillForward | Self::TillBackward)
    }

    /// Check if this moves forward.
    #[must_use]
    pub const fn is_forward(&self) -> bool {
        matches!(self, Self::FindForward | Self::TillForward)
    }
}
