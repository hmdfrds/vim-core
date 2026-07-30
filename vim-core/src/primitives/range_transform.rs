//! Range transformation system for operator dispatch.
//!
//! Formalizes range/inclusivity handling by providing a clean interface for
//! normalizing, expanding, and contracting ranges. Inspired by Evil's range
//! system design where motions produce raw ranges that are then transformed
//! for operator application.
//!
//! # Concept
//!
//! Vim motions and text objects produce ranges with different inclusivity
//! semantics. Before an operator can act on a range, it must be normalized:
//!
//! - **Exclusive** ranges need no end adjustment (the end offset already
//!   points past the last affected character).
//! - **Inclusive** ranges need the end extended by one character (to convert
//!   from "cursor on last char" to "exclusive end past last char").
//! - **Linewise** ranges need expansion to full line boundaries.
//!
//! The `expand` and `contract` methods handle whitespace trimming patterns
//! used by text objects (`a` objects include surrounding whitespace, `i`
//! objects exclude it).
//!
//! # Usage
//!
//! ```ignore
//! use vim_core::primitives::{MotionRange, MotionInclusivity, Range};
//!
//! let motion_range = MotionRange::new(
//!     Range::from_raw(5, 10),
//!     MotionInclusivity::Inclusive,
//! );
//! let normalized = motion_range.normalize("hello world!");
//! // normalized.range is now [5, 11) — end extended for inclusive motion
//! ```

use crate::primitives::text_util::snap_to_char_boundary;
use crate::primitives::{MotionInclusivity, MotionType, Offset, Range};

// ═══════════════════════════════════════════════════════════════════════════════
// Core Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A range paired with its motion inclusivity context.
///
/// This is the input to the transformation pipeline: a raw range as
/// produced by a motion or text object, plus the inclusivity classification
/// that determines how the range end should be adjusted.
///
/// # Examples
///
/// ```ignore
/// // An inclusive motion like `e` (word end):
/// let mr = MotionRange::new(Range::from_raw(0, 4), MotionInclusivity::Inclusive);
///
/// // A linewise motion like `j`:
/// let mr = MotionRange::linewise(Range::from_raw(0, 20));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MotionRange {
    /// The raw byte range.
    range: Range,
    /// How the end position should be treated.
    inclusivity: MotionInclusivity,
}

impl MotionRange {
    /// Create a new motion range.
    #[inline]
    #[must_use]
    pub const fn new(range: Range, inclusivity: MotionInclusivity) -> Self {
        Self { range, inclusivity }
    }

    /// Create an exclusive motion range (e.g., `w`, `b`, `0`).
    #[inline]
    #[must_use]
    pub const fn exclusive(range: Range) -> Self {
        Self::new(range, MotionInclusivity::Exclusive)
    }

    /// Create an inclusive motion range (e.g., `e`, `f`, `$`).
    #[inline]
    #[must_use]
    pub const fn inclusive(range: Range) -> Self {
        Self::new(range, MotionInclusivity::Inclusive)
    }

    /// Create a linewise motion range (e.g., `j`, `k`, `G`).
    #[inline]
    #[must_use]
    pub const fn linewise(range: Range) -> Self {
        Self::new(range, MotionInclusivity::Linewise)
    }

    /// Get the raw range.
    #[inline]
    #[must_use]
    pub const fn range(self) -> Range {
        self.range
    }

    /// Get the inclusivity.
    #[inline]
    #[must_use]
    pub const fn inclusivity(self) -> MotionInclusivity {
        self.inclusivity
    }
}

/// A normalized range ready for operator application.
///
/// After normalization, the range is always expressed with an exclusive end
/// (start..end where end is NOT included), and the motion type indicates
/// whether the operator should treat it as character-wise, line-wise, or
/// block-wise.
///
/// This is the output of the transformation pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NormalizedRange {
    /// The normalized byte range (always exclusive end).
    range: Range,
    /// How the operator should treat this range.
    motion_type: MotionType,
}

impl NormalizedRange {
    /// Create a new normalized range.
    #[inline]
    #[must_use]
    pub const fn new(range: Range, motion_type: MotionType) -> Self {
        Self { range, motion_type }
    }

    /// Get the byte range.
    #[inline]
    #[must_use]
    pub const fn range(self) -> Range {
        self.range
    }

    /// Get the motion type.
    #[inline]
    #[must_use]
    pub const fn motion_type(self) -> MotionType {
        self.motion_type
    }

    /// Start gap offset.
    #[inline]
    #[must_use]
    pub const fn start(self) -> Offset {
        self.range.start()
    }

    /// End gap offset (exclusive).
    #[inline]
    #[must_use]
    pub const fn end(self) -> Offset {
        self.range.end()
    }

    /// Length in bytes.
    #[inline]
    #[must_use]
    pub const fn len(self) -> usize {
        self.range.len()
    }

    /// Check if empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.range.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Implementation
// ═══════════════════════════════════════════════════════════════════════════════

impl MotionRange {
    /// Normalize the range for operator application.
    ///
    /// Handles inclusive-to-exclusive end adjustment, linewise expansion
    /// to full line boundaries, and exclusive pass-through.
    ///
    /// The `text` parameter is needed for linewise expansion (finding line
    /// boundaries) and inclusive adjustment (advancing past multi-byte chars).
    #[must_use]
    pub fn normalize(&self, text: &str) -> NormalizedRange {
        match self.inclusivity {
            MotionInclusivity::Exclusive => {
                // Exclusive: range end already points past last affected char.
                // Clamp to text length for safety.
                let end = self.range.end().get().min(text.len());
                let start = self.range.start().get().min(end);
                NormalizedRange::new(Range::from_raw(start, end), MotionType::CharWise)
            }
            MotionInclusivity::Inclusive => {
                // Inclusive: end points AT the last char to affect.
                // Extend end by one character width to make it exclusive.
                let start = self.range.start().get().min(text.len());
                let raw_end = self.range.end().get();
                let end = advance_past_char(text, raw_end);
                NormalizedRange::new(Range::from_raw(start, end), MotionType::CharWise)
            }
            MotionInclusivity::Linewise => {
                // Linewise: expand to full line boundaries.
                let (line_start, line_end) = expand_to_line_boundaries(
                    text,
                    self.range.start().get(),
                    self.range.end().get(),
                );
                NormalizedRange::new(Range::from_raw(line_start, line_end), MotionType::LineWise)
            }
        }
    }

    /// Expand the range outward to include surrounding whitespace.
    ///
    /// Used by `a` text objects (e.g., `aw`, `as`): includes trailing
    /// whitespace after the range, or leading whitespace before it if
    /// there is no trailing whitespace.
    #[must_use]
    pub fn expand(&self, text: &str) -> NormalizedRange {
        // First normalize, then expand outward to include surrounding whitespace.
        let normalized = self.normalize(text);
        let start = normalized.start().get();
        let end = normalized.end().get();

        // Try trailing whitespace first (Vim prefers trailing).
        let trailing_end = skip_whitespace_forward(text, end);
        if trailing_end > end {
            return NormalizedRange::new(
                Range::from_raw(start, trailing_end),
                normalized.motion_type(),
            );
        }

        // No trailing whitespace: try leading whitespace.
        let leading_start = skip_whitespace_backward(text, start);
        if leading_start < start {
            return NormalizedRange::new(
                Range::from_raw(leading_start, end),
                normalized.motion_type(),
            );
        }

        // No surrounding whitespace to include.
        normalized
    }

    /// Contract the range inward to exclude surrounding whitespace.
    ///
    /// Used by `i` text objects (e.g., `iw`, `is`): trims leading and
    /// trailing whitespace from the range boundaries.
    #[must_use]
    pub fn contract(&self, text: &str) -> NormalizedRange {
        // First normalize, then contract inward by trimming whitespace.
        let normalized = self.normalize(text);
        let start = normalized.start().get();
        let end = normalized.end().get();

        // Trim leading whitespace.
        let trimmed_start = skip_whitespace_forward(text, start);
        // Trim trailing whitespace.
        let trimmed_end = skip_whitespace_backward(text, end);

        // Guard against over-trimming (if range was all whitespace, preserve it).
        if trimmed_start >= trimmed_end {
            return normalized;
        }

        NormalizedRange::new(
            Range::from_raw(trimmed_start, trimmed_end),
            normalized.motion_type(),
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers (private)
// ═══════════════════════════════════════════════════════════════════════════════

/// Advance past the character at `offset`, returning the byte position
/// after it. Handles multi-byte UTF-8 correctly.
///
/// If `offset` is at or past the end of `text`, returns `text.len()`.
fn advance_past_char(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    // Snap to a valid char boundary before slicing.
    let safe = snap_to_char_boundary(text, offset);
    text.get(safe..)
        .and_then(|s| s.chars().next())
        .map_or(text.len(), |c| safe + c.len_utf8())
}

/// Expand a range to full line boundaries.
///
/// Returns `(line_start, line_end)` where:
/// - `line_start` is the byte offset of the start of the line containing `start`
/// - `line_end` is the byte offset past the newline of the line containing `end`
///   (or `text.len()` if the last line has no trailing newline)
fn expand_to_line_boundaries(text: &str, start: usize, end: usize) -> (usize, usize) {
    let start = snap_to_char_boundary(text, start.min(text.len()));
    let end = snap_to_char_boundary(text, end.min(text.len()));

    // Find start of the line containing `start`.
    let line_start = text
        .get(..start)
        .and_then(|s| s.rfind('\n'))
        .map_or(0, |i| i + 1);

    // Find end of the line containing `end` (include the newline).
    let line_end = text
        .get(end..)
        .and_then(|s| s.find('\n'))
        .map_or(text.len(), |i| end + i + 1);

    (line_start, line_end)
}

/// Skip whitespace (spaces and tabs) forward from `offset`.
///
/// Returns the byte offset of the first non-whitespace character after
/// `offset`, or the end of text. Stops at newlines (does not cross line
/// boundaries for `expand`).
fn skip_whitespace_forward(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let safe = snap_to_char_boundary(text, offset);
    let mut pos = safe;
    for ch in text.get(safe..).unwrap_or("").chars() {
        if ch == ' ' || ch == '\t' {
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    pos
}

/// Skip whitespace (spaces and tabs) backward from `offset`.
///
/// Returns the byte offset just after the last non-whitespace character
/// before `offset`, or 0. Stops at newlines (does not cross line
/// boundaries for `contract`).
fn skip_whitespace_backward(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let clamped = snap_to_char_boundary(text, offset.min(text.len()));
    let mut pos = clamped;
    for ch in text.get(..clamped).unwrap_or("").chars().rev() {
        if ch == ' ' || ch == '\t' {
            pos -= ch.len_utf8();
        } else {
            break;
        }
    }
    pos
}

#[cfg(test)]
#[path = "range_transform_tests.rs"]
mod tests;
