//! Virtual column (curswant) for sticky column tracking.
//!
//! `VirtualColumn` is a newtype over `usize` that represents the desired cursor
//! column for vertical motions (`j`, `k`, `G`, `gg`, `H`, `M`, `L`, `Ctrl-D/U/F/B`).
//!
//! In Vim, horizontal motions set `curswant` to the current column, while `$` and
//! `g$` set it to `MAXCOL` (end-of-line stickiness). Vertical motions then use
//! this stored column to position the cursor on the target line, clamping to the
//! line's actual length.
//!
//! The `END_OF_LINE` sentinel replaces raw `usize::MAX` usage, making the intent
//! explicit and preventing accidental arithmetic on sentinel values.

/// A virtual column index for sticky column (curswant) tracking.
///
/// Wraps a `usize` representing the desired display column for vertical motions.
/// The special constant [`VirtualColumn::END_OF_LINE`] indicates end-of-line
/// stickiness (Vim's `MAXCOL`), where vertical motions should go to the end of
/// each target line rather than a fixed column.
///
/// # Examples
///
/// ```
/// use vim_core::primitives::VirtualColumn;
///
/// // A regular column position
/// let col = VirtualColumn::new(5);
/// assert_eq!(col.get(), 5);
/// assert!(!col.is_end_of_line());
///
/// // End-of-line stickiness (after `$` motion)
/// let eol = VirtualColumn::END_OF_LINE;
/// assert!(eol.is_end_of_line());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct VirtualColumn(usize);

impl VirtualColumn {
    /// Sentinel value indicating end-of-line stickiness (Vim's `MAXCOL`).
    ///
    /// When the sticky column is set to this value, vertical motions (`j`, `k`,
    /// `G`, `gg`, etc.) move the cursor to the end of each target line rather
    /// than to a fixed column. This is the behavior after the `$` motion.
    pub const END_OF_LINE: Self = Self(usize::MAX);

    /// Create a new virtual column at the given display column index.
    #[inline]
    #[must_use]
    pub const fn new(column: usize) -> Self {
        Self(column)
    }

    /// Get the raw column value.
    ///
    /// For regular columns, this is the display column index.
    /// For [`END_OF_LINE`](Self::END_OF_LINE), this returns `usize::MAX`.
    #[inline]
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }

    /// Check whether this represents end-of-line stickiness.
    ///
    /// Returns `true` when this is [`END_OF_LINE`](Self::END_OF_LINE),
    /// indicating that vertical motions should go to the end of each line
    /// (the behavior after `$`).
    #[inline]
    #[must_use]
    pub const fn is_end_of_line(self) -> bool {
        self.0 == usize::MAX
    }
}

impl core::fmt::Display for VirtualColumn {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_end_of_line() {
            write!(f, "EOL")
        } else {
            write!(f, "{}", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_get() {
        let col = VirtualColumn::new(42);
        assert_eq!(col.get(), 42);
    }

    #[test]
    fn zero_column() {
        let col = VirtualColumn::new(0);
        assert_eq!(col.get(), 0);
        assert!(!col.is_end_of_line());
    }

    #[test]
    fn end_of_line_sentinel() {
        let eol = VirtualColumn::END_OF_LINE;
        assert!(eol.is_end_of_line());
        assert_eq!(eol.get(), usize::MAX);
    }

    #[test]
    fn regular_column_is_not_end_of_line() {
        assert!(!VirtualColumn::new(0).is_end_of_line());
        assert!(!VirtualColumn::new(100).is_end_of_line());
        assert!(!VirtualColumn::new(usize::MAX - 1).is_end_of_line());
    }

    #[test]
    fn equality() {
        assert_eq!(VirtualColumn::new(5), VirtualColumn::new(5));
        assert_ne!(VirtualColumn::new(5), VirtualColumn::new(6));
        assert_eq!(VirtualColumn::END_OF_LINE, VirtualColumn::END_OF_LINE);
    }

    #[test]
    fn ordering() {
        assert!(VirtualColumn::new(0) < VirtualColumn::new(1));
        assert!(VirtualColumn::new(100) < VirtualColumn::END_OF_LINE);
    }

    #[test]
    fn copy_semantics() {
        let col = VirtualColumn::new(7);
        let col2 = col;
        assert_eq!(col, col2); // both valid after copy
    }

    #[test]
    fn display_regular() {
        assert_eq!(format!("{}", VirtualColumn::new(42)), "42");
    }

    #[test]
    fn display_eol() {
        assert_eq!(format!("{}", VirtualColumn::END_OF_LINE), "EOL");
    }
}
