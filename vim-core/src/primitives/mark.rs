//! Mark primitive — a saved position in the buffer.

use super::position::Offset;

/// A saved position in the buffer.
///
/// Stores the byte offset of the marked position and, optionally, a relative
/// line offset from the mark's line to the viewport's top line at mark-set
/// time. The `topline_offset` field is populated only for user-set marks
/// (`m{a-z}`, `m{A-Z}`); auto-marks leave it `None`.
///
/// # Relative topline encoding
///
/// `topline_offset = mark_line - viewport_first_line` (both 0-indexed line
/// numbers). At jump time the viewport is restored:
/// `topline_line = mark_line - topline_offset`. This encoding is stable for
/// edits above the viewport top or below the mark — the relative distance is
/// preserved. Edits between the viewport top and the mark shift the mark line
/// without updating `topline_offset`, so the restored viewport will be off by
/// the number of inserted/deleted lines. This is acceptable: the viewport is
/// a hint, not a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mark {
    /// Byte offset in the document.
    offset: Offset,
    /// Relative line offset: `mark_line - viewport_first_line` at set time.
    ///
    /// `Some(offset)` for user-set marks; `None` for auto-marks.
    topline_offset: Option<i32>,
}

impl Mark {
    /// Create a new mark (no viewport context).
    #[inline]
    #[must_use]
    pub const fn new(offset: Offset) -> Self {
        Self {
            offset,
            topline_offset: None,
        }
    }

    /// Create a mark with relative topline offset.
    ///
    /// `topline_offset` is `mark_line - viewport_first_line` (both 0-indexed).
    #[inline]
    #[must_use]
    pub const fn with_topline_offset(offset: Offset, topline_offset: Option<i32>) -> Self {
        Self {
            offset,
            topline_offset,
        }
    }

    /// Get the byte offset.
    #[inline]
    #[must_use]
    pub const fn offset(self) -> Offset {
        self.offset
    }

    /// Get the relative topline offset (if captured at mark-set time).
    ///
    /// Value is `mark_line - viewport_first_line` at set time. At jump time,
    /// resolve: `topline_line = mark_line - topline_offset`.
    #[inline]
    #[must_use]
    pub const fn topline_offset(self) -> Option<i32> {
        self.topline_offset
    }

    /// Create from raw offset (no viewport context).
    #[inline]
    #[must_use]
    pub const fn from_raw(offset: usize) -> Self {
        Self {
            offset: Offset::new(offset),
            topline_offset: None,
        }
    }
}
