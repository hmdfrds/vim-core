//! Last visual mode info.
//!
//! Pure value type storing selection dimensions from the last visual session.
//! Used by effects (to record) and state (to store) for `gv` and `.` repeat.

use super::VisualType;

/// Information stored when exiting visual mode.
///
/// Used for:
/// - `gv` command: restores visual type (bounds come from `<`/`>` marks)
/// - `.` repeat: remembers selection dimensions for repeating visual operators
///
/// This mirrors Neovim's `resel_VIsual_*` variables:
/// - `resel_VIsual_mode` → `visual_type`
/// - `resel_VIsual_line_count` → `lines`
/// - `resel_VIsual_vcol` → `columns`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LastVisualInfo {
    /// The visual type that was active (Char, Line, Block).
    visual_type: VisualType,
    /// Number of lines in the selection (for `.` repeat).
    lines: usize,
    /// Virtual columns (tab-expanded screen columns) for `.` repeat.
    ///
    /// For charwise single-line: vcol width of the selection.
    /// For charwise multi-line: absolute vcol of the end position on the last line.
    /// For block: grapheme column width.
    /// Matches Neovim's `resel_VIsual_vcol`.
    columns: usize,
    /// Whether cursor was at the start (low end) of the selection.
    ///
    /// When `true`, the cursor/head was at the `<` mark (low end).
    /// When `false` (the default), the cursor/head was at the `>` mark (high end).
    /// Used by `gv` to restore cursor to the correct end of the selection.
    cursor_at_start: bool,
}

impl LastVisualInfo {
    /// Create new last visual info.
    #[inline]
    #[must_use]
    pub const fn new(visual_type: VisualType, lines: usize, columns: usize) -> Self {
        Self {
            visual_type,
            lines,
            columns,
            cursor_at_start: false,
        }
    }

    /// Get the visual type.
    #[inline]
    #[must_use]
    pub const fn visual_type(self) -> VisualType {
        self.visual_type
    }

    /// Get the number of lines.
    #[inline]
    #[must_use]
    pub const fn lines(self) -> usize {
        self.lines
    }

    /// Get the number of columns.
    #[inline]
    #[must_use]
    pub const fn columns(self) -> usize {
        self.columns
    }

    /// Whether the cursor was at the start (low end) of the selection.
    #[inline]
    #[must_use]
    pub const fn cursor_at_start(self) -> bool {
        self.cursor_at_start
    }

    /// Return a copy with `cursor_at_start` set.
    #[inline]
    #[must_use]
    pub const fn with_cursor_at_start(mut self, at_start: bool) -> Self {
        self.cursor_at_start = at_start;
        self
    }

    /// Create for character-wise visual.
    #[inline]
    #[must_use]
    pub const fn char_wise(lines: usize) -> Self {
        Self::new(VisualType::Char, lines, 0)
    }

    /// Create for line-wise visual.
    #[inline]
    #[must_use]
    pub const fn line_wise(lines: usize) -> Self {
        Self::new(VisualType::Line, lines, 0)
    }

    /// Create for block-wise visual.
    #[inline]
    #[must_use]
    pub const fn block_wise(lines: usize, columns: usize) -> Self {
        Self::new(VisualType::Block, lines, columns)
    }
}
