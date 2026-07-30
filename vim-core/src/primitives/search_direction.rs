//! SearchDirection primitive — direction of a search operation.

use super::direction::Direction;

/// Direction of search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, strum::EnumCount, strum::EnumIter)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SearchDirection {
    /// Forward (/) - search from cursor towards end
    #[default]
    Forward,
    /// Backward (?) - search from cursor towards start
    Backward,
}

impl SearchDirection {
    /// Get the opposite direction.
    #[inline]
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
        }
    }
}

impl From<SearchDirection> for Direction {
    fn from(sd: SearchDirection) -> Self {
        match sd {
            SearchDirection::Forward => Self::Forward,
            SearchDirection::Backward => Self::Backward,
        }
    }
}

impl From<Direction> for SearchDirection {
    fn from(d: Direction) -> Self {
        match d {
            Direction::Forward => Self::Forward,
            Direction::Backward => Self::Backward,
        }
    }
}
