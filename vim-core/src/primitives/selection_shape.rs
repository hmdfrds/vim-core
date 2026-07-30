//! Visual selection presentation shape.
//!
//! This is the shell-facing contract for how a live selection must be rendered.

/// Render shape for a live selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumCount, strum::EnumIter)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SelectionShape {
    /// Character-wise visual selection.
    Char,
    /// Line-wise visual selection.
    Line,
    /// Block-wise visual selection.
    Block,
}

impl SelectionShape {
    /// All variants, in declaration order.
    pub const ALL: [Self; 3] = [Self::Char, Self::Line, Self::Block];
}

impl From<super::VisualType> for SelectionShape {
    #[allow(
        unreachable_patterns,
        reason = "VisualType is #[non_exhaustive] — wildcard needed for external crates"
    )]
    fn from(vt: super::VisualType) -> Self {
        match vt {
            super::VisualType::Char => Self::Char,
            super::VisualType::Line => Self::Line,
            super::VisualType::Block => Self::Block,
            _ => Self::Char,
        }
    }
}

impl From<SelectionShape> for super::VisualType {
    #[allow(
        unreachable_patterns,
        reason = "SelectionShape is #[non_exhaustive] — wildcard needed for external crates"
    )]
    fn from(shape: SelectionShape) -> Self {
        match shape {
            SelectionShape::Char => Self::Char,
            SelectionShape::Line => Self::Line,
            SelectionShape::Block => Self::Block,
            _ => Self::Char,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::VisualType;

    #[test]
    fn selection_shape_all_no_duplicates() {
        use std::collections::HashSet;
        use strum::EnumCount;
        let unique: HashSet<SelectionShape> = SelectionShape::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            SelectionShape::ALL.len(),
            "Duplicate in SelectionShape::ALL"
        );
        assert_eq!(
            SelectionShape::ALL.len(),
            SelectionShape::COUNT,
            "SelectionShape::ALL is missing a variant"
        );
    }

    #[test]
    fn selection_shape_to_visual_type_roundtrip() {
        assert_eq!(VisualType::from(SelectionShape::Char), VisualType::Char);
        assert_eq!(VisualType::from(SelectionShape::Line), VisualType::Line);
        assert_eq!(VisualType::from(SelectionShape::Block), VisualType::Block);
    }

    #[test]
    fn visual_type_to_selection_shape_roundtrip() {
        assert_eq!(SelectionShape::from(VisualType::Char), SelectionShape::Char);
        assert_eq!(SelectionShape::from(VisualType::Line), SelectionShape::Line);
        assert_eq!(
            SelectionShape::from(VisualType::Block),
            SelectionShape::Block
        );
    }
}
