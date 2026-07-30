//! Cursor shape and blink style for mode-driven cursor rendering.
//!
//! `CursorStyle` is emitted alongside `SetMode` (as a separate `SetCursorStyle`
//! effect) so host adapters do not need their own mode-to-cursor mapping.

/// Shape of the cursor caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CursorShape {
    /// Full-cell block cursor (▋).  Default for Normal and Visual modes.
    Block,
    /// Thin vertical bar cursor (|).  Default for Insert mode.
    VerticalBar,
    /// Thin horizontal underline cursor (_).  Default for Replace mode.
    HorizontalBar,
    /// Half-height block cursor (lower half).  Used for modification operators in OP mode.
    HalfBlock,
}

impl CursorShape {
    /// All variants, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::Block,
        Self::VerticalBar,
        Self::HorizontalBar,
        Self::HalfBlock,
    ];

    /// Derive cursor shape from mode.
    ///
    /// Convenience method for hosts that only care about shape without blink.
    /// Delegates to [`CursorStyle::for_mode`].
    #[inline]
    #[must_use]
    pub const fn from_mode(mode: crate::primitives::Mode) -> Self {
        CursorStyle::for_mode(mode).shape
    }
}

/// Combined cursor style: shape + blink flag.
///
/// Produced by the effect processor alongside every `SetMode` effect so the
/// host has everything it needs to render the correct cursor without
/// reimplementing the mode → cursor mapping.
///
/// Hosts that already manage cursor style themselves can safely ignore
/// `SetCursorStyle` effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CursorStyle {
    /// The cursor caret shape.
    pub shape: CursorShape,
    /// Whether the cursor should blink.
    pub blink: bool,
}

/// Number of distinct top-level mode kinds for cursor shape overrides.
///
/// Indices: Normal=0, Insert=1, Visual=2, Select=3, Replace=4,
/// VirtualReplace=5, CommandLine=6, OperatorPending=7.
pub const CURSOR_OVERRIDE_COUNT: usize = 8;

/// Map a [`Mode`](crate::primitives::Mode) to a stable index for cursor shape overrides.
///
/// All sub-variants of a mode kind (e.g. `Visual(Char)`, `Visual(Line)`,
/// `Visual(Block)`) share the same index because the cursor shape is
/// determined by the top-level mode, not the sub-variant.
#[inline]
#[must_use]
pub const fn mode_to_override_index(mode: crate::primitives::Mode) -> usize {
    use crate::primitives::Mode;
    match mode {
        Mode::Normal => 0,
        Mode::Insert => 1,
        Mode::Visual(_) => 2,
        Mode::Select(_) => 3,
        Mode::Replace => 4,
        Mode::VirtualReplace => 5,
        Mode::CommandLine => 6,
        Mode::OperatorPending(_) => 7,
    }
}

impl CursorStyle {
    /// Derive the appropriate cursor style from a Vim [`Mode`](crate::primitives::Mode).
    ///
    /// Mapping follows the Vim defaults:
    /// - Normal          → Block, no blink
    /// - Visual (all)    → Block, no blink
    /// - Select (all)    → Block, no blink
    /// - Insert          → VerticalBar, blink
    /// - Replace         → HorizontalBar, blink
    /// - VirtualReplace  → HorizontalBar, blink
    /// - CommandLine     → Block, blink
    /// - OperatorPending (d/c/y/g~/gu/gU) → HalfBlock, no blink
    /// - OperatorPending (other)           → Block, no blink
    #[inline]
    #[must_use]
    pub const fn for_mode(mode: crate::primitives::Mode) -> Self {
        use crate::primitives::{Mode, Operator};
        match mode {
            Mode::Normal | Mode::Visual(_) | Mode::Select(_) => Self {
                shape: CursorShape::Block,
                blink: false,
            },
            Mode::OperatorPending(op) => Self {
                shape: match op {
                    // Modification operators → HalfBlock to indicate pending change
                    Operator::Delete
                    | Operator::Change
                    | Operator::Yank
                    | Operator::ToggleCase
                    | Operator::Uppercase
                    | Operator::Lowercase => CursorShape::HalfBlock,
                    // All other operators (indent, format, filter, etc.) → Block
                    _ => CursorShape::Block,
                },
                blink: false,
            },
            Mode::Insert => Self {
                shape: CursorShape::VerticalBar,
                blink: true,
            },
            Mode::Replace | Mode::VirtualReplace => Self {
                shape: CursorShape::HorizontalBar,
                blink: true,
            },
            Mode::CommandLine => Self {
                shape: CursorShape::Block,
                blink: true,
            },
        }
    }

    /// Derive cursor style from mode, applying per-mode shape overrides.
    ///
    /// If `overrides` has an entry at the mode's index and that entry is
    /// `Some(shape)`, the returned style uses that shape with the default
    /// blink for that mode. Otherwise falls back to [`for_mode`](Self::for_mode).
    #[inline]
    #[must_use]
    pub fn for_mode_with_overrides(
        mode: crate::primitives::Mode,
        overrides: &[Option<CursorShape>],
    ) -> Self {
        let idx = mode_to_override_index(mode);
        if let Some(Some(shape)) = overrides.get(idx) {
            let default = Self::for_mode(mode);
            Self {
                shape: *shape,
                blink: default.blink,
            }
        } else {
            Self::for_mode(mode)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Mode, VisualType};

    #[test]
    fn cursor_shape_all_no_duplicates() {
        use std::collections::HashSet;
        let unique: HashSet<CursorShape> = CursorShape::ALL.iter().copied().collect();
        assert_eq!(
            unique.len(),
            CursorShape::ALL.len(),
            "Duplicate in CursorShape::ALL"
        );
    }

    #[test]
    fn normal_is_block_no_blink() {
        let s = CursorStyle::for_mode(Mode::Normal);
        assert_eq!(s.shape, CursorShape::Block);
        assert!(!s.blink);
    }

    #[test]
    fn insert_is_vertical_bar_blink() {
        let s = CursorStyle::for_mode(Mode::Insert);
        assert_eq!(s.shape, CursorShape::VerticalBar);
        assert!(s.blink);
    }

    #[test]
    fn replace_is_horizontal_bar_blink() {
        let s = CursorStyle::for_mode(Mode::Replace);
        assert_eq!(s.shape, CursorShape::HorizontalBar);
        assert!(s.blink);
    }

    #[test]
    fn virtual_replace_is_horizontal_bar_blink() {
        let s = CursorStyle::for_mode(Mode::VirtualReplace);
        assert_eq!(s.shape, CursorShape::HorizontalBar);
        assert!(s.blink);
    }

    #[test]
    fn command_line_is_block_blink() {
        let s = CursorStyle::for_mode(Mode::CommandLine);
        assert_eq!(s.shape, CursorShape::Block);
        assert!(s.blink);
    }

    #[test]
    fn visual_char_is_block_no_blink() {
        let s = CursorStyle::for_mode(Mode::Visual(VisualType::Char));
        assert_eq!(s.shape, CursorShape::Block);
        assert!(!s.blink);
    }

    #[test]
    fn visual_line_is_block_no_blink() {
        let s = CursorStyle::for_mode(Mode::Visual(VisualType::Line));
        assert_eq!(s.shape, CursorShape::Block);
        assert!(!s.blink);
    }

    #[test]
    fn visual_block_is_block_no_blink() {
        let s = CursorStyle::for_mode(Mode::Visual(VisualType::Block));
        assert_eq!(s.shape, CursorShape::Block);
        assert!(!s.blink);
    }

    #[test]
    fn select_all_is_block_no_blink() {
        for vt in [VisualType::Char, VisualType::Line, VisualType::Block] {
            let s = CursorStyle::for_mode(Mode::Select(vt));
            assert_eq!(s.shape, CursorShape::Block);
            assert!(!s.blink);
        }
    }

    #[test]
    fn from_mode_matches_for_mode_shape() {
        for mode in [
            Mode::Normal,
            Mode::Insert,
            Mode::Replace,
            Mode::VirtualReplace,
            Mode::CommandLine,
            Mode::Visual(VisualType::Char),
            Mode::Visual(VisualType::Line),
            Mode::Visual(VisualType::Block),
            Mode::Select(VisualType::Char),
        ] {
            assert_eq!(
                CursorShape::from_mode(mode),
                CursorStyle::for_mode(mode).shape,
                "from_mode and for_mode.shape should agree for {mode:?}"
            );
        }
    }

    // -- mode_to_override_index tests --

    #[test]
    fn override_index_stable_values() {
        use super::mode_to_override_index;
        use crate::primitives::Operator;
        assert_eq!(mode_to_override_index(Mode::Normal), 0);
        assert_eq!(mode_to_override_index(Mode::Insert), 1);
        assert_eq!(mode_to_override_index(Mode::Visual(VisualType::Char)), 2);
        assert_eq!(mode_to_override_index(Mode::Visual(VisualType::Line)), 2);
        assert_eq!(mode_to_override_index(Mode::Visual(VisualType::Block)), 2);
        assert_eq!(mode_to_override_index(Mode::Select(VisualType::Char)), 3);
        assert_eq!(mode_to_override_index(Mode::Replace), 4);
        assert_eq!(mode_to_override_index(Mode::VirtualReplace), 5);
        assert_eq!(mode_to_override_index(Mode::CommandLine), 6);
        assert_eq!(
            mode_to_override_index(Mode::OperatorPending(Operator::Delete)),
            7
        );
    }

    // -- for_mode_with_overrides tests --

    #[test]
    fn override_insert_cursor_to_block() {
        let mut overrides = vec![None; super::CURSOR_OVERRIDE_COUNT];
        overrides[1] = Some(CursorShape::Block);

        let s = CursorStyle::for_mode_with_overrides(Mode::Insert, &overrides);
        assert_eq!(s.shape, CursorShape::Block);
        assert!(s.blink);
    }

    #[test]
    fn no_override_uses_default() {
        let overrides = vec![None; super::CURSOR_OVERRIDE_COUNT];

        for mode in [
            Mode::Normal,
            Mode::Insert,
            Mode::Replace,
            Mode::VirtualReplace,
            Mode::CommandLine,
            Mode::Visual(VisualType::Char),
        ] {
            assert_eq!(
                CursorStyle::for_mode_with_overrides(mode, &overrides),
                CursorStyle::for_mode(mode),
                "with all-None overrides, result should match for_mode for {mode:?}"
            );
        }
    }

    #[test]
    fn empty_overrides_vec_uses_default() {
        let overrides: Vec<Option<CursorShape>> = vec![];

        for mode in [Mode::Normal, Mode::Insert, Mode::Replace, Mode::CommandLine] {
            assert_eq!(
                CursorStyle::for_mode_with_overrides(mode, &overrides),
                CursorStyle::for_mode(mode),
                "with empty overrides, result should match for_mode for {mode:?}"
            );
        }
    }

    #[test]
    fn override_normal_cursor_to_vertical_bar() {
        let mut overrides = vec![None; super::CURSOR_OVERRIDE_COUNT];
        overrides[0] = Some(CursorShape::VerticalBar);

        let s = CursorStyle::for_mode_with_overrides(Mode::Normal, &overrides);
        assert_eq!(s.shape, CursorShape::VerticalBar);
        assert!(!s.blink);
    }

    #[test]
    fn override_does_not_affect_other_modes() {
        let mut overrides = vec![None; super::CURSOR_OVERRIDE_COUNT];
        overrides[1] = Some(CursorShape::Block);

        let normal = CursorStyle::for_mode_with_overrides(Mode::Normal, &overrides);
        assert_eq!(normal, CursorStyle::for_mode(Mode::Normal));
    }

    // -- Operator-pending cursor shape differentiation --

    #[test]
    fn operator_pending_delete_is_half_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::Delete));
        assert_eq!(s.shape, CursorShape::HalfBlock);
        assert!(!s.blink);
    }

    #[test]
    fn operator_pending_change_is_half_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::Change));
        assert_eq!(s.shape, CursorShape::HalfBlock);
        assert!(!s.blink);
    }

    #[test]
    fn operator_pending_yank_is_half_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::Yank));
        assert_eq!(s.shape, CursorShape::HalfBlock);
        assert!(!s.blink);
    }

    #[test]
    fn operator_pending_toggle_case_is_half_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::ToggleCase));
        assert_eq!(s.shape, CursorShape::HalfBlock);
        assert!(!s.blink);
    }

    #[test]
    fn operator_pending_uppercase_is_half_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::Uppercase));
        assert_eq!(s.shape, CursorShape::HalfBlock);
        assert!(!s.blink);
    }

    #[test]
    fn operator_pending_lowercase_is_half_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::Lowercase));
        assert_eq!(s.shape, CursorShape::HalfBlock);
        assert!(!s.blink);
    }

    #[test]
    fn operator_pending_indent_is_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::Indent));
        assert_eq!(s.shape, CursorShape::Block);
        assert!(!s.blink);
    }

    #[test]
    fn operator_pending_format_is_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::Format));
        assert_eq!(s.shape, CursorShape::Block);
        assert!(!s.blink);
    }

    #[test]
    fn operator_pending_filter_is_block() {
        use crate::primitives::Operator;
        let s = CursorStyle::for_mode(Mode::OperatorPending(Operator::Filter));
        assert_eq!(s.shape, CursorShape::Block);
        assert!(!s.blink);
    }
}
