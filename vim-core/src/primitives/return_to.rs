//! ReturnTo — the mode to restore after a Ctrl-O one-shot command.
//!
//! This generalises the "Ctrl-O in insert mode" concept so that
//! any mode (Insert, Replace, VirtualReplace, Select) can be the
//! target of a return-after-one-command sequence.

use super::{Mode, VisualType};

/// Which mode to return to after executing a single normal-mode command.
///
/// Used by Ctrl-O (insert → normal → one command → insert) and the
/// analogous Select-mode nesting. `None` means "don't return anywhere"
/// (i.e. the normal non-nested behaviour).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReturnTo {
    /// No pending return — normal behaviour.
    #[default]
    None,
    /// Return to Insert mode after the one-shot command.
    Insert,
    /// Return to Replace mode after the one-shot command.
    Replace,
    /// Return to Virtual-Replace mode after the one-shot command.
    VirtualReplace,
    /// Return to Select mode (of the given visual type) after the one-shot command.
    Select(VisualType),
}

impl ReturnTo {
    /// Convert to the [`Mode`] that should be entered, or `None` if
    /// there is nothing to return to.
    #[must_use]
    pub const fn target_mode(&self) -> Option<Mode> {
        match self {
            Self::None => Option::None,
            Self::Insert => Some(Mode::Insert),
            Self::Replace => Some(Mode::Replace),
            Self::VirtualReplace => Some(Mode::VirtualReplace),
            Self::Select(vt) => Some(Mode::Select(*vt)),
        }
    }

    /// Returns `true` when this is `ReturnTo::None`.
    #[inline]
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Returns `true` when this is anything other than `ReturnTo::None`.
    #[inline]
    #[must_use]
    pub const fn is_some(&self) -> bool {
        !self.is_none()
    }
}

impl std::fmt::Display for ReturnTo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Insert => write!(f, "Insert"),
            Self::Replace => write!(f, "Replace"),
            Self::VirtualReplace => write!(f, "VirtualReplace"),
            Self::Select(vt) => write!(f, "Select({vt:?})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_none() {
        assert_eq!(ReturnTo::default(), ReturnTo::None);
    }

    #[test]
    fn is_none_and_is_some() {
        assert!(ReturnTo::None.is_none());
        assert!(!ReturnTo::None.is_some());

        for rt in [
            ReturnTo::Insert,
            ReturnTo::Replace,
            ReturnTo::VirtualReplace,
            ReturnTo::Select(VisualType::Char),
            ReturnTo::Select(VisualType::Line),
            ReturnTo::Select(VisualType::Block),
        ] {
            assert!(!rt.is_none(), "{rt} should not be none");
            assert!(rt.is_some(), "{rt} should be some");
        }
    }

    #[test]
    fn target_mode_none_returns_none() {
        assert_eq!(ReturnTo::None.target_mode(), Option::None);
    }

    #[test]
    fn target_mode_insert() {
        assert_eq!(ReturnTo::Insert.target_mode(), Some(Mode::Insert));
    }

    #[test]
    fn target_mode_replace() {
        assert_eq!(ReturnTo::Replace.target_mode(), Some(Mode::Replace));
    }

    #[test]
    fn target_mode_virtual_replace() {
        assert_eq!(
            ReturnTo::VirtualReplace.target_mode(),
            Some(Mode::VirtualReplace)
        );
    }

    #[test]
    fn target_mode_select_char() {
        assert_eq!(
            ReturnTo::Select(VisualType::Char).target_mode(),
            Some(Mode::Select(VisualType::Char))
        );
    }

    #[test]
    fn target_mode_select_line() {
        assert_eq!(
            ReturnTo::Select(VisualType::Line).target_mode(),
            Some(Mode::Select(VisualType::Line))
        );
    }

    #[test]
    fn target_mode_select_block() {
        assert_eq!(
            ReturnTo::Select(VisualType::Block).target_mode(),
            Some(Mode::Select(VisualType::Block))
        );
    }

    #[test]
    fn partial_eq_works() {
        assert_eq!(ReturnTo::Insert, ReturnTo::Insert);
        assert_ne!(ReturnTo::Insert, ReturnTo::Replace);
        assert_ne!(ReturnTo::None, ReturnTo::Insert);
        assert_eq!(
            ReturnTo::Select(VisualType::Char),
            ReturnTo::Select(VisualType::Char)
        );
        assert_ne!(
            ReturnTo::Select(VisualType::Char),
            ReturnTo::Select(VisualType::Line)
        );
    }
}
