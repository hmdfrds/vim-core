//! Direction types for vim-core.
//!
//! Direction of movement or search.

use smart_default::SmartDefault;

/// Direction of movement or search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SmartDefault)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Direction {
    /// Movement toward end of document.
    #[default]
    Forward,
    /// Movement toward start of document.
    Backward,
}

impl Direction {
    /// All variants, in declaration order.
    pub const ALL: [Self; 2] = [Self::Forward, Self::Backward];

    /// Reverse the direction.
    #[inline]
    #[must_use]
    pub const fn reverse(self) -> Self {
        match self {
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
        }
    }

    /// Check if forward.
    #[inline]
    #[must_use]
    pub const fn is_forward(self) -> bool {
        matches!(self, Self::Forward)
    }

    /// Check if backward.
    #[inline]
    #[must_use]
    pub const fn is_backward(self) -> bool {
        matches!(self, Self::Backward)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_all_no_duplicates() {
        use std::collections::HashSet;
        let unique: HashSet<Direction> = Direction::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            Direction::ALL.len(),
            "Duplicate in Direction::ALL"
        );
    }

    #[test]
    fn default_is_forward() {
        assert_eq!(Direction::default(), Direction::Forward);
    }

    #[test]
    fn reverse_forward() {
        assert_eq!(Direction::Forward.reverse(), Direction::Backward);
    }

    #[test]
    fn reverse_backward() {
        assert_eq!(Direction::Backward.reverse(), Direction::Forward);
    }

    #[test]
    fn double_reverse_is_identity() {
        assert_eq!(Direction::Forward.reverse().reverse(), Direction::Forward);
        assert_eq!(Direction::Backward.reverse().reverse(), Direction::Backward);
    }

    #[test]
    fn is_forward() {
        assert!(Direction::Forward.is_forward());
        assert!(!Direction::Backward.is_forward());
    }

    #[test]
    fn is_backward() {
        assert!(Direction::Backward.is_backward());
        assert!(!Direction::Forward.is_backward());
    }
}
