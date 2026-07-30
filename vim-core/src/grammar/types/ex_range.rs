//! Ex command range types.
//!
//! Represents line addresses and ranges for Ex commands like `:5,10d`.
//!
//! Per EBNF:
//! ```text
//! range     = line_spec ["," line_spec]
//! line_spec = "." | "$" | "%" | number | "'" mark_name
//! ```

use compact_str::CompactString;
use smart_default::SmartDefault;

use crate::primitives::MarkName;

/// Separator between two line specs in an Ex range.
///
/// `,` is the standard separator. `;` means "set cursor to the first address
/// before evaluating the second", so `5;/foo/` searches for `/foo/` starting
/// from line 5 rather than from the current cursor position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SmartDefault)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RangeSeparator {
    /// Standard comma separator (`,`). The second address is resolved from
    /// the current cursor position.
    #[default]
    Comma,
    /// Semicolon separator (`;`). The cursor is moved to the first address
    /// before the second address is evaluated.
    Semicolon,
}

/// A single line specifier in an Ex range.
///
/// Line specs include:
/// - `.` current line
/// - `$` last line
/// - `%` entire file (expands to 1,$)
/// - `5` absolute line number
/// - `'a` line of mark
/// - `/pat/` search forward
/// - `?pat?` search backward
/// - `+n` or `-n` relative offset
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum LineSpec {
    /// Current line (`.`).
    Current,
    /// Last line of file (`$`).
    Last,
    /// Absolute line number (1-indexed).
    Absolute(u32),
    /// Line of a mark (`'a`, `'<`, `'>`).
    Mark(MarkName),
    /// Search forward for pattern (`/pat/`).
    SearchForward(CompactString),
    /// Search backward for pattern (`?pat?`).
    SearchBackward(CompactString),
    /// Relative offset from current position (`+3`, `-2`).
    Relative(i32),
    /// A base line spec with an additional line offset (e.g. `.+1`, `$-2`, `'a+3`).
    WithOffset {
        /// The base line spec.
        base: Box<Self>,
        /// The line offset (positive = down, negative = up).
        offset: i32,
    },
}

impl LineSpec {
    /// Create an absolute line spec.
    #[inline]
    #[must_use]
    pub const fn absolute(line: u32) -> Self {
        Self::Absolute(line)
    }

    /// Create a mark line spec.
    #[inline]
    #[must_use]
    pub const fn mark(name: MarkName) -> Self {
        Self::Mark(name)
    }

    /// Wrap this line spec with an additional offset.
    #[must_use]
    pub fn with_offset(self, offset: i32) -> Self {
        if offset == 0 {
            self
        } else {
            Self::WithOffset {
                base: Box::new(self),
                offset,
            }
        }
    }
}

/// An Ex command range (e.g., `5,10` or `%`).
///
/// Represents a contiguous range of lines for Ex commands to operate on.
#[derive(Debug, Clone, PartialEq, Eq, SmartDefault)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExRange {
    /// First line specifier.
    #[default(LineSpec::Current)]
    pub start: LineSpec,
    /// Second line specifier (None = single line).
    pub end: Option<LineSpec>,
    /// Separator between the two line specs (default: comma).
    #[default(RangeSeparator::Comma)]
    pub separator: RangeSeparator,
}

impl ExRange {
    /// Create a range for the current line only.
    #[inline]
    #[must_use]
    pub const fn current_line() -> Self {
        Self {
            start: LineSpec::Current,
            end: None,
            separator: RangeSeparator::Comma,
        }
    }

    /// Create a range for the entire file (`%`).
    #[inline]
    #[must_use]
    pub const fn entire_file() -> Self {
        Self {
            start: LineSpec::Absolute(1),
            end: Some(LineSpec::Last),
            separator: RangeSeparator::Comma,
        }
    }

    /// Create a range from start to end lines (1-indexed).
    #[inline]
    #[must_use]
    pub const fn lines(start: u32, end: u32) -> Self {
        Self {
            start: LineSpec::Absolute(start),
            end: Some(LineSpec::Absolute(end)),
            separator: RangeSeparator::Comma,
        }
    }

    /// Create a range for the visual selection (`*` expands to `'<,'>`).
    #[inline]
    #[must_use]
    pub const fn visual_selection() -> Self {
        Self {
            start: LineSpec::Mark(MarkName::VISUAL_START),
            end: Some(LineSpec::Mark(MarkName::VISUAL_END)),
            separator: RangeSeparator::Comma,
        }
    }

    /// Create a single line range.
    #[inline]
    #[must_use]
    pub const fn single_line(line: u32) -> Self {
        Self {
            start: LineSpec::Absolute(line),
            end: None,
            separator: RangeSeparator::Comma,
        }
    }

    /// Check if this is a single-line range.
    #[inline]
    #[must_use]
    pub const fn is_single_line(&self) -> bool {
        self.end.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_line() {
        let range = ExRange::current_line();
        assert_eq!(range.start, LineSpec::Current);
        assert!(range.is_single_line());
    }

    #[test]
    fn test_entire_file() {
        let range = ExRange::entire_file();
        assert_eq!(range.start, LineSpec::Absolute(1));
        assert_eq!(range.end, Some(LineSpec::Last));
    }

    #[test]
    fn test_line_range() {
        let range = ExRange::lines(5, 10);
        assert_eq!(range.start, LineSpec::Absolute(5));
        assert_eq!(range.end, Some(LineSpec::Absolute(10)));
    }
}
