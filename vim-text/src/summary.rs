// Composable summary types for the B+ tree text buffer.
//
// TextSummary = MetricsSummary + LineSummary + IndentSummary + BracketSummary
//
// Each sub-summary is a monoid with its own compose operation. The top-level
// TextSummary delegates compose to each sub-summary, and `from_str` computes
// all fields in a single byte traversal.

#[cfg(test)]
use crate::tree::traits::InvertibleSummary;
use crate::tree::traits::{Dimension, Summary};

// -----------------------------------------------------------------------
// 1. MetricsSummary — invertible, exact subtraction
// -----------------------------------------------------------------------

/// Additive metrics: bytes, chars, newlines, UTF-16 length.
///
/// All fields are strictly additive, making this fully invertible via
/// wrapping subtraction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetricsSummary {
    /// Total byte count.
    pub bytes: u32,
    /// Total Unicode scalar values (non-continuation bytes in UTF-8).
    pub chars: u32,
    /// Number of `\n` characters.
    pub newlines: u32,
    /// Total UTF-16 code units: chars + extra surrogates for 4-byte codepoints.
    pub utf16_len: u32,
}

impl MetricsSummary {
    /// Compose: add all fields.
    #[inline]
    pub fn compose(&mut self, other: &Self) {
        self.bytes += other.bytes;
        self.chars += other.chars;
        self.newlines += other.newlines;
        self.utf16_len += other.utf16_len;
    }

    /// Exact subtraction via wrapping_sub.
    #[inline]
    pub fn subtract(&mut self, other: &Self) {
        self.bytes = self.bytes.wrapping_sub(other.bytes);
        self.chars = self.chars.wrapping_sub(other.chars);
        self.newlines = self.newlines.wrapping_sub(other.newlines);
        self.utf16_len = self.utf16_len.wrapping_sub(other.utf16_len);
    }
}

impl Summary for MetricsSummary {
    fn compose(&mut self, other: &Self) {
        MetricsSummary::compose(self, other);
    }

    fn base_len(&self) -> usize {
        self.bytes as usize
    }
}

#[cfg(test)]
impl InvertibleSummary for MetricsSummary {
    fn subtract(&mut self, other: &Self) {
        MetricsSummary::subtract(self, other);
    }
}

// -----------------------------------------------------------------------
// 2. LineSummary — non-invertible (seam logic)
// -----------------------------------------------------------------------

/// Line-length tracking for first, last, and maximum line in a span.
///
/// All lengths are in bytes (u16, saturating at 65535), excluding `\n`
/// delimiters. Newline counts live in `MetricsSummary` and are passed
/// into `compose_with_context` when needed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineSummary {
    /// Bytes in the first line (before first `\n`, or all bytes if no `\n`).
    pub first_line_len: u16,
    /// Bytes in the last line (after last `\n`, or all bytes if no `\n`).
    pub last_line_len: u16,
    /// Longest line in bytes.
    pub max_line_len: u16,
}

impl LineSummary {
    /// Compose two adjacent line summaries.
    ///
    /// The "seam line" is formed by self's last line + other's first line.
    /// If self has no newlines, the entire self is part of the seam.
    /// Newline counts are supplied from the corresponding `MetricsSummary`.
    #[inline]
    pub fn compose_with_context(&mut self, other: &Self, self_newlines: u32, other_newlines: u32) {
        let first_line_len = if self_newlines > 0 {
            self.first_line_len
        } else {
            self.first_line_len.saturating_add(other.first_line_len)
        };

        let last_line_len = if other_newlines > 0 {
            other.last_line_len
        } else {
            other.last_line_len.saturating_add(self.last_line_len)
        };

        let seam_line = self.last_line_len.saturating_add(other.first_line_len);
        let max_line_len = self.max_line_len.max(other.max_line_len).max(seam_line);

        self.first_line_len = first_line_len;
        self.last_line_len = last_line_len;
        self.max_line_len = max_line_len;
    }
}

// -----------------------------------------------------------------------
// 3. IndentSummary — non-invertible (min + flags)
// -----------------------------------------------------------------------

bitflags::bitflags! {
    /// Structural boolean properties of a text span, focused on indent
    /// and blank-line detection.
    ///
    /// `HAS_BLANK_LINE` / `HAS_NONBLANK_LINE` track **middle** lines only
    /// (excluding the first and last segments). Use the top-level
    /// `TextSummary::has_blank_line()` / `has_nonblank_line()` for the full
    /// query that includes edge segments.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct IndentFlags: u16 {
        /// The span contains a blank middle line.
        const HAS_BLANK_LINE      = 0x01;
        /// The span contains a non-blank middle line.
        const HAS_NONBLANK_LINE   = 0x02;
        /// The first line segment (before first `\n`) is blank.
        const FIRST_LINE_BLANK    = 0x04;
        /// The last line segment (after last `\n`) is blank.
        const LAST_LINE_BLANK     = 0x08;
        /// The span ends with `\n`.
        const ENDS_WITH_NEWLINE   = 0x10;
        /// The span starts with `\n`.
        const STARTS_WITH_NEWLINE = 0x20;
        /// Every byte in the span is ASCII (< 128).
        const ALL_ASCII           = 0x40;
    }
}

/// Minimum indent and blank-line flags for a text span.
///
/// Default is the identity element: min_indent = u16::MAX (no non-blank lines),
/// flags = empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndentSummary {
    /// Minimum indent across all non-blank lines. u16::MAX = identity (no
    /// non-blank lines observed).
    pub min_indent: u16,
    /// Structural flags (blank lines, edge conditions).
    pub flags: IndentFlags,
}

impl Default for IndentSummary {
    fn default() -> Self {
        Self {
            min_indent: u16::MAX,
            flags: IndentFlags::empty(),
        }
    }
}

impl IndentSummary {
    /// Compose two adjacent indent summaries.
    ///
    /// The newlines parameters come from the corresponding MetricsSummary
    /// (needed for seam-line blank detection).
    #[inline]
    pub fn compose(
        &mut self,
        other: &Self,
        self_bytes: u32,
        self_newlines: u32,
        other_bytes: u32,
        other_newlines: u32,
    ) {
        // --- Blank/nonblank flags (middle-only representation) ---
        //
        // HAS_BLANK_LINE / HAS_NONBLANK_LINE track middle lines only.
        // Edge lines are tracked by FIRST_LINE_BLANK / LAST_LINE_BLANK.
        //
        // Result's middle lines come from:
        //   1. Self's middle lines.
        //   2. Other's middle lines.
        //   3. The seam line, IF it's a middle line of the result
        //      (self has newlines AND other has newlines).

        let mut flags = IndentFlags::empty();

        // (1) + (2): OR middle flags from both sides.
        flags |= (self.flags | other.flags)
            & (IndentFlags::HAS_BLANK_LINE | IndentFlags::HAS_NONBLANK_LINE);

        // The seam line: self's last segment + other's first segment.
        let self_last_blank = if self_bytes == 0 {
            true
        } else {
            self.flags.contains(IndentFlags::LAST_LINE_BLANK)
        };
        let other_first_blank = if other_bytes == 0 {
            true
        } else {
            other.flags.contains(IndentFlags::FIRST_LINE_BLANK)
        };
        let seam_blank = self_last_blank && other_first_blank;

        // (3): Add seam to middle if it's interior (both sides have newlines).
        if self_newlines > 0 && other_newlines > 0 {
            if seam_blank {
                flags |= IndentFlags::HAS_BLANK_LINE;
            } else {
                flags |= IndentFlags::HAS_NONBLANK_LINE;
            }
        }

        // --- FIRST_LINE_BLANK for result ---
        let first_line_blank = if self_bytes > 0 {
            if self_newlines > 0 {
                self.flags.contains(IndentFlags::FIRST_LINE_BLANK)
            } else {
                seam_blank
            }
        } else {
            other.flags.contains(IndentFlags::FIRST_LINE_BLANK)
        };

        // --- LAST_LINE_BLANK for result ---
        let last_line_blank = if other_bytes > 0 {
            if other_newlines > 0 {
                other.flags.contains(IndentFlags::LAST_LINE_BLANK)
            } else {
                seam_blank
            }
        } else {
            self.flags.contains(IndentFlags::LAST_LINE_BLANK)
        };

        if first_line_blank {
            flags |= IndentFlags::FIRST_LINE_BLANK;
        }
        if last_line_blank {
            flags |= IndentFlags::LAST_LINE_BLANK;
        }

        // --- Positional newline flags ---
        if self_bytes > 0 {
            flags |= self.flags & IndentFlags::STARTS_WITH_NEWLINE;
        } else {
            flags |= other.flags & IndentFlags::STARTS_WITH_NEWLINE;
        }

        if other_bytes > 0 {
            flags |= other.flags & IndentFlags::ENDS_WITH_NEWLINE;
        } else {
            flags |= self.flags & IndentFlags::ENDS_WITH_NEWLINE;
        }

        // ALL_ASCII: preserved if both non-empty sides are all-ASCII.
        // An empty side (bytes==0) is neutral (identity) and does not affect
        // the result. When both sides are empty, ALL_ASCII stays unset
        // (matching Default).
        if self_bytes > 0 || other_bytes > 0 {
            let self_ascii = self_bytes == 0 || self.flags.contains(IndentFlags::ALL_ASCII);
            let other_ascii = other_bytes == 0 || other.flags.contains(IndentFlags::ALL_ASCII);
            if self_ascii && other_ascii {
                flags |= IndentFlags::ALL_ASCII;
            }
        }

        self.min_indent = self.min_indent.min(other.min_indent);
        self.flags = flags;
    }
}

// -----------------------------------------------------------------------
// 4. BracketSummary — delta invertible, min not
// -----------------------------------------------------------------------

/// Depth tracking for a single bracket type (e.g. parentheses).
///
/// Uses the standard delta+min monoid: when composing two adjacent spans,
/// `new_min = self.min.min(self.delta + other.min)` and
/// `new_delta = self.delta + other.delta`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BracketPair {
    /// Net depth change across the span.
    pub delta: i16,
    /// Minimum depth reached during left-to-right traversal.
    pub min: i16,
    /// Maximum depth reached during left-to-right traversal.
    pub max: i16,
}

impl BracketPair {
    /// Compose two adjacent bracket pairs.
    #[inline]
    pub fn compose(&mut self, other: &Self) {
        // min and max must be computed before delta is updated.
        let new_min = self.min.min(self.delta.saturating_add(other.min));
        let new_max = self.max.max(self.delta.saturating_add(other.max));
        self.delta = self.delta.saturating_add(other.delta);
        self.min = new_min;
        self.max = new_max;
    }
}

/// Bracket depth tracking for all three bracket types: `()`, `[]`, `{}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BracketSummary {
    /// Parentheses `()`.
    pub paren: BracketPair,
    /// Square brackets `[]`.
    pub bracket: BracketPair,
    /// Curly braces `{}`.
    pub brace: BracketPair,
}

impl BracketSummary {
    /// Compose two adjacent bracket summaries.
    #[inline]
    pub fn compose(&mut self, other: &Self) {
        self.paren.compose(&other.paren);
        self.bracket.compose(&other.bracket);
        self.brace.compose(&other.brace);
    }
}

// -----------------------------------------------------------------------
// 5. TextSummary — top-level composite
// -----------------------------------------------------------------------

/// Full per-node aggregate summary for the B+ tree.
///
/// This is a monoid: `Default` is the identity element, and `compose` is the
/// associative binary operation. Built from four independent sub-summaries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSummary {
    /// Additive metrics: bytes, chars, newlines, UTF-16 length.
    pub metrics: MetricsSummary,
    /// Line-length tracking (first, last, max).
    pub lines: LineSummary,
    /// Minimum indent and blank-line flags.
    pub indent: IndentSummary,
    /// Bracket depth tracking for `()`, `[]`, `{}`.
    pub brackets: BracketSummary,
}

impl TextSummary {
    /// True if this span contains at least one blank line (all lines considered).
    #[inline]
    pub fn has_blank_line(&self) -> bool {
        if self.metrics.bytes == 0 {
            return false;
        }
        if self.indent.flags.contains(IndentFlags::HAS_BLANK_LINE) {
            return true;
        }
        if self.indent.flags.contains(IndentFlags::FIRST_LINE_BLANK) {
            return true;
        }
        // The last segment is a real line only when NOT a trailing phantom
        // after a final \n.
        if !self.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE)
            && self.indent.flags.contains(IndentFlags::LAST_LINE_BLANK)
        {
            return true;
        }
        false
    }

    /// True if this span contains at least one nonblank line (all lines considered).
    #[inline]
    pub fn has_nonblank_line(&self) -> bool {
        if self.metrics.bytes == 0 {
            return false;
        }
        if self.indent.flags.contains(IndentFlags::HAS_NONBLANK_LINE) {
            return true;
        }
        if !self.indent.flags.contains(IndentFlags::FIRST_LINE_BLANK) {
            return true;
        }
        // The last segment is real only when not a trailing phantom.
        if !self.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE)
            && self.metrics.newlines > 0
            && !self.indent.flags.contains(IndentFlags::LAST_LINE_BLANK)
        {
            return true;
        }
        false
    }

    /// Compute a summary from a string slice.
    ///
    /// Internally split into two phases for better performance on the hot path:
    ///
    /// **Phase 1 (SIMD-friendly):** Newline counting via `memchr` (SIMD-accelerated),
    /// plus a tight char/UTF-16 counting loop that the compiler can auto-vectorize
    /// better than a mixed loop.
    ///
    /// **Phase 2 (scalar):** Sequential pass for line lengths, indent tracking,
    /// bracket depth, and ALL_ASCII detection. These require per-byte sequential
    /// state and cannot be vectorized.
    ///
    /// Line lengths (`first_line_len`, `last_line_len`, `max_line_len`) are
    /// measured in bytes (excluding the `\n` delimiter).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        let bytes_slice = s.as_bytes();
        let len = bytes_slice.len();

        if len == 0 {
            return Self::default();
        }

        // ---------------------------------------------------------------
        // Phase 1: SIMD-friendly metrics (newlines, chars, utf16)
        // ---------------------------------------------------------------

        // Newlines: memchr uses SIMD (SSE2/AVX2/NEON) for bulk scanning.
        let newlines = memchr::memchr_iter(b'\n', bytes_slice).count() as u32;

        // Chars + UTF-16 surrogates: tight loop the compiler can auto-vectorize.
        // Kept separate from the sequential state above so the compiler can
        // auto-vectorize this loop.
        let mut chars: u32 = 0;
        let mut four_byte_starts: u32 = 0;
        for &b in bytes_slice {
            if (b & 0xC0) != 0x80 {
                chars += 1;
                if b >= 0xF0 {
                    four_byte_starts += 1;
                }
            }
        }
        let utf16_len = chars + four_byte_starts;

        // ---------------------------------------------------------------
        // Phase 2: Scalar pass for sequential state
        // (line lengths, indent, brackets, ALL_ASCII)
        // ---------------------------------------------------------------

        let mut first_line_len: u16 = 0;
        let mut first_line_set = false;
        let mut current_line_len: u16 = 0;
        let mut max_line_len: u16 = 0;
        let mut all_ascii = true;

        let mut paren_delta: i16 = 0;
        let mut paren_min: i16 = 0;
        let mut paren_max: i16 = 0;
        let mut bracket_delta: i16 = 0;
        let mut bracket_min: i16 = 0;
        let mut bracket_max: i16 = 0;
        let mut brace_delta: i16 = 0;
        let mut brace_min: i16 = 0;
        let mut brace_max: i16 = 0;

        let mut min_indent: u16 = u16::MAX;

        // Per-line tracking for blank/indent.
        let mut first_line_is_blank = true;
        let mut current_line_is_blank = true;
        let mut current_line_indent: u16 = 0;
        let mut counting_indent = true;

        // Middle-only blank/nonblank flags (exclude first and last segments).
        let mut has_blank_middle = false;
        let mut has_nonblank_middle = false;

        // Count of lines finalized (at each \n).
        let mut lines_finalized: u32 = 0;

        let mut flags = IndentFlags::empty();

        if bytes_slice[0] == b'\n' {
            flags |= IndentFlags::STARTS_WITH_NEWLINE;
        }
        if bytes_slice[len - 1] == b'\n' {
            flags |= IndentFlags::ENDS_WITH_NEWLINE;
        }

        for &b in bytes_slice {
            // Track ASCII-ness: any byte >= 128 clears the flag.
            if b >= 128 {
                all_ascii = false;
            }

            if b == b'\n' {
                // Finalize the line that just ended.
                if lines_finalized == 0 {
                    first_line_is_blank = current_line_is_blank;
                } else {
                    // Middle line.
                    if current_line_is_blank {
                        has_blank_middle = true;
                    } else {
                        has_nonblank_middle = true;
                    }
                }

                // min_indent: only for nonblank lines.
                if !current_line_is_blank {
                    min_indent = min_indent.min(current_line_indent);
                }

                if !first_line_set {
                    first_line_len = current_line_len;
                    first_line_set = true;
                }
                max_line_len = max_line_len.max(current_line_len);
                current_line_len = 0;
                lines_finalized += 1;

                // Reset per-line state.
                current_line_is_blank = true;
                current_line_indent = 0;
                counting_indent = true;
            } else {
                current_line_len = current_line_len.saturating_add(1);

                match b {
                    b'(' => {
                        paren_delta = paren_delta.saturating_add(1);
                        if paren_delta > paren_max {
                            paren_max = paren_delta;
                        }
                    }
                    b')' => {
                        paren_delta = paren_delta.saturating_add(-1);
                        paren_min = paren_min.min(paren_delta);
                    }
                    b'[' => {
                        bracket_delta = bracket_delta.saturating_add(1);
                        if bracket_delta > bracket_max {
                            bracket_max = bracket_delta;
                        }
                    }
                    b']' => {
                        bracket_delta = bracket_delta.saturating_add(-1);
                        bracket_min = bracket_min.min(bracket_delta);
                    }
                    b'{' => {
                        brace_delta = brace_delta.saturating_add(1);
                        if brace_delta > brace_max {
                            brace_max = brace_delta;
                        }
                    }
                    b'}' => {
                        brace_delta = brace_delta.saturating_add(-1);
                        brace_min = brace_min.min(brace_delta);
                    }
                    _ => {}
                }

                if b != b' ' && b != b'\t' && b != b'\r' {
                    current_line_is_blank = false;
                    counting_indent = false;
                } else if counting_indent && b != b'\r' {
                    current_line_indent += 1;
                }
            }
        }

        // --- Determine last segment's blank status ---
        let last_line_is_blank = if flags.contains(IndentFlags::ENDS_WITH_NEWLINE) {
            true
        } else {
            // The final unterminated line. Account for its blank/indent status.
            if !current_line_is_blank {
                min_indent = min_indent.min(current_line_indent);
            }
            current_line_is_blank
        };

        if !first_line_set {
            // No newlines: the entire string is one segment (first = last).
            first_line_len = current_line_len;
            first_line_is_blank = current_line_is_blank;
        }
        max_line_len = max_line_len.max(current_line_len);
        let last_line_len = current_line_len;

        // Set internal flags.
        if has_blank_middle {
            flags |= IndentFlags::HAS_BLANK_LINE;
        }
        if has_nonblank_middle {
            flags |= IndentFlags::HAS_NONBLANK_LINE;
        }
        if first_line_is_blank {
            flags |= IndentFlags::FIRST_LINE_BLANK;
        }
        if last_line_is_blank {
            flags |= IndentFlags::LAST_LINE_BLANK;
        }
        if all_ascii {
            flags |= IndentFlags::ALL_ASCII;
        }

        Self {
            metrics: MetricsSummary {
                bytes: len as u32,
                chars,
                newlines,
                utf16_len,
            },
            lines: LineSummary {
                first_line_len,
                last_line_len,
                max_line_len,
            },
            indent: IndentSummary { min_indent, flags },
            brackets: BracketSummary {
                paren: BracketPair {
                    delta: paren_delta,
                    min: paren_min,
                    max: paren_max,
                },
                bracket: BracketPair {
                    delta: bracket_delta,
                    min: bracket_min,
                    max: bracket_max,
                },
                brace: BracketPair {
                    delta: brace_delta,
                    min: brace_min,
                    max: brace_max,
                },
            },
        }
    }
}

impl Summary for TextSummary {
    fn compose(&mut self, other: &Self) {
        // Capture values needed by IndentSummary::compose before mutation.
        let self_bytes = self.metrics.bytes;
        let self_newlines = self.metrics.newlines;
        let other_bytes = other.metrics.bytes;
        let other_newlines = other.metrics.newlines;

        self.metrics.compose(&other.metrics);
        self.lines
            .compose_with_context(&other.lines, self_newlines, other_newlines);
        self.indent.compose(
            &other.indent,
            self_bytes,
            self_newlines,
            other_bytes,
            other_newlines,
        );
        self.brackets.compose(&other.brackets);
    }

    fn base_len(&self) -> usize {
        self.metrics.bytes as usize
    }
}

// -----------------------------------------------------------------------
// 6. Dimension types
// -----------------------------------------------------------------------

/// Byte offset dimension for seeking.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteOffset(pub u32);

/// Line offset dimension (count of newlines).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineOffset(pub u32);

/// Character (Unicode scalar) offset dimension.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CharOffset(pub u32);

/// UTF-16 code unit offset dimension.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Utf16Offset(pub u32);

impl Dimension<TextSummary> for ByteOffset {
    fn from_summary(summary: &TextSummary) -> Self {
        ByteOffset(summary.metrics.bytes)
    }

    fn add_summary(&mut self, summary: &TextSummary) {
        self.0 += summary.metrics.bytes;
    }
}

impl Dimension<TextSummary> for LineOffset {
    fn from_summary(summary: &TextSummary) -> Self {
        LineOffset(summary.metrics.newlines)
    }

    fn add_summary(&mut self, summary: &TextSummary) {
        self.0 += summary.metrics.newlines;
    }
}

impl Dimension<TextSummary> for CharOffset {
    fn from_summary(summary: &TextSummary) -> Self {
        CharOffset(summary.metrics.chars)
    }

    fn add_summary(&mut self, summary: &TextSummary) {
        self.0 += summary.metrics.chars;
    }
}

impl Dimension<TextSummary> for Utf16Offset {
    fn from_summary(summary: &TextSummary) -> Self {
        Utf16Offset(summary.metrics.utf16_len)
    }

    fn add_summary(&mut self, summary: &TextSummary) {
        self.0 += summary.metrics.utf16_len;
    }
}

// -----------------------------------------------------------------------
// Backward compatibility alias
// -----------------------------------------------------------------------

/// Legacy alias: old vim-core code used `SummaryFlags`, now renamed to `IndentFlags`.
pub type SummaryFlags = IndentFlags;

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // =======================================================================
    // from_str tests
    // =======================================================================

    #[test]
    fn empty_string() {
        let s = TextSummary::from_str("");
        assert_eq!(s.metrics.bytes, 0);
        assert_eq!(s.metrics.chars, 0);
        assert_eq!(s.metrics.newlines, 0);
        assert_eq!(s.metrics.utf16_len, 0);
        assert_eq!(s.lines.first_line_len, 0);
        assert_eq!(s.lines.last_line_len, 0);
        assert_eq!(s.lines.max_line_len, 0);
        assert_eq!(s.indent.min_indent, u16::MAX);
        assert_eq!(s.indent.flags, IndentFlags::empty());
        assert_eq!(s.brackets.paren.delta, 0);
        assert_eq!(s.brackets.bracket.delta, 0);
        assert_eq!(s.brackets.brace.delta, 0);
        assert_eq!(s, TextSummary::default());
    }

    #[test]
    fn ascii_no_newline() {
        let s = TextSummary::from_str("hello");
        assert_eq!(s.metrics.bytes, 5);
        assert_eq!(s.metrics.chars, 5);
        assert_eq!(s.metrics.newlines, 0);
        assert_eq!(s.metrics.utf16_len, 5);
        assert_eq!(s.lines.first_line_len, 5);
        assert_eq!(s.lines.last_line_len, 5);
        assert_eq!(s.lines.max_line_len, 5);
        assert_eq!(s.indent.min_indent, 0);
        assert!(!s.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE));
        assert!(s.has_nonblank_line());
        assert!(!s.has_blank_line());
    }

    #[test]
    fn single_newline() {
        let s = TextSummary::from_str("hello\nworld");
        assert_eq!(s.metrics.bytes, 11);
        assert_eq!(s.metrics.chars, 11);
        assert_eq!(s.metrics.newlines, 1);
        assert_eq!(s.lines.first_line_len, 5);
        assert_eq!(s.lines.last_line_len, 5);
        assert_eq!(s.lines.max_line_len, 5);
        assert!(!s.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE));
    }

    #[test]
    fn trailing_newline() {
        let s = TextSummary::from_str("hello\n");
        assert_eq!(s.metrics.bytes, 6);
        assert_eq!(s.metrics.newlines, 1);
        assert_eq!(s.lines.first_line_len, 5);
        assert_eq!(s.lines.last_line_len, 0);
        assert_eq!(s.lines.max_line_len, 5);
        assert!(s.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE));
        assert!(s.has_nonblank_line());
        assert!(!s.has_blank_line());
    }

    #[test]
    fn blank_line() {
        let s = TextSummary::from_str("hello\n\nworld");
        assert!(s.has_blank_line());
        assert!(s.has_nonblank_line());
        assert!(s.indent.flags.contains(IndentFlags::HAS_BLANK_LINE));
    }

    #[test]
    fn unicode_chars() {
        // "héllo 世界" -> 'h'(1B) 'é'(2B) 'l'(1B) 'l'(1B) 'o'(1B) ' '(1B) '世'(3B) '界'(3B)
        let s = TextSummary::from_str("h\u{00E9}llo \u{4E16}\u{754C}");
        assert_eq!(s.metrics.bytes, 13);
        assert_eq!(s.metrics.chars, 8);
        // UTF-16: all BMP chars -> 1 code unit each, no surrogates
        assert_eq!(s.metrics.utf16_len, 8);
    }

    #[test]
    fn emoji_with_surrogates() {
        // "🎉" -> 4 bytes, 1 char, 2 UTF-16 code units
        let s = TextSummary::from_str("\u{1F389}");
        assert_eq!(s.metrics.bytes, 4);
        assert_eq!(s.metrics.chars, 1);
        assert_eq!(s.metrics.utf16_len, 2); // 1 char + 1 extra surrogate
    }

    #[test]
    fn brackets_balanced() {
        let s = TextSummary::from_str("(hello [world])");
        assert_eq!(s.brackets.paren.delta, 0);
        assert_eq!(s.brackets.bracket.delta, 0);
        // paren: ( at start, ) at end -> never goes below 0
        assert_eq!(s.brackets.paren.min, 0);
        assert_eq!(s.brackets.bracket.min, 0);
    }

    #[test]
    fn unbalanced_brackets() {
        let s = TextSummary::from_str("(((");
        assert_eq!(s.brackets.paren.delta, 3);
        assert_eq!(s.brackets.paren.min, 0);

        let s2 = TextSummary::from_str(")))");
        assert_eq!(s2.brackets.paren.delta, -3);
        assert_eq!(s2.brackets.paren.min, -3);
    }

    #[test]
    fn indent_tracking() {
        let s = TextSummary::from_str("  hello\n    world");
        assert_eq!(s.indent.min_indent, 2);
    }

    #[test]
    fn indent_tabs() {
        let s = TextSummary::from_str("\thello\n\t\tworld\n");
        assert_eq!(s.indent.min_indent, 1);
    }

    #[test]
    fn indent_only_blank_lines() {
        let s = TextSummary::from_str("\n\n  \n");
        assert_eq!(s.indent.min_indent, u16::MAX);
    }

    #[test]
    fn crlf_handling() {
        let s = TextSummary::from_str("hello\r\nworld");
        assert_eq!(s.metrics.newlines, 1);
        // \r is counted in line length (6 bytes: "hello\r")
        assert_eq!(s.lines.first_line_len, 6);
        assert_eq!(s.lines.last_line_len, 5);
    }

    #[test]
    fn starts_with_newline() {
        let s = TextSummary::from_str("\nhello");
        assert!(s.indent.flags.contains(IndentFlags::STARTS_WITH_NEWLINE));
        assert!(!s.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE));
        assert_eq!(s.lines.first_line_len, 0);
        assert_eq!(s.lines.last_line_len, 5);
    }

    #[test]
    fn starts_and_ends_with_newline() {
        let s = TextSummary::from_str("\nhello\n");
        assert!(s.indent.flags.contains(IndentFlags::STARTS_WITH_NEWLINE));
        assert!(s.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE));
    }

    #[test]
    fn only_newline() {
        let s = TextSummary::from_str("\n");
        assert_eq!(s.metrics.bytes, 1);
        assert_eq!(s.metrics.newlines, 1);
        assert_eq!(s.lines.first_line_len, 0);
        assert_eq!(s.lines.last_line_len, 0);
        assert!(s.indent.flags.contains(IndentFlags::STARTS_WITH_NEWLINE));
        assert!(s.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE));
        assert!(s.has_blank_line());
    }

    #[test]
    fn whitespace_only_no_newline() {
        let s = TextSummary::from_str("   ");
        assert!(s.has_blank_line());
        assert!(!s.has_nonblank_line());
        assert_eq!(s.indent.min_indent, u16::MAX);
    }

    #[test]
    fn indent_of_zero() {
        let s = TextSummary::from_str("x\n  y\n");
        assert_eq!(s.indent.min_indent, 0);
    }

    #[test]
    fn multiple_lines() {
        let s = TextSummary::from_str("ab\ncdef\ng\n");
        assert_eq!(s.metrics.bytes, 10);
        assert_eq!(s.metrics.newlines, 3);
        assert_eq!(s.lines.first_line_len, 2);
        assert_eq!(s.lines.last_line_len, 0);
        assert_eq!(s.lines.max_line_len, 4);
    }

    #[test]
    fn mixed_unicode() {
        // a(1B) é(2B) 世(3B) 😀(4B) -> bytes=10, chars=4, utf16_len=5
        let s = TextSummary::from_str("a\u{00E9}\u{4E16}\u{1F600}");
        assert_eq!(s.metrics.bytes, 10);
        assert_eq!(s.metrics.chars, 4);
        assert_eq!(s.metrics.utf16_len, 5); // 4 chars + 1 surrogate
    }

    #[test]
    fn bracket_in_multiline() {
        let s = TextSummary::from_str("{\n  x\n}\n");
        assert_eq!(s.brackets.brace.delta, 0);
        assert_eq!(s.brackets.brace.min, 0);
    }

    #[test]
    fn bracket_reverse_then_open() {
        let s = TextSummary::from_str(")(");
        assert_eq!(s.brackets.paren.delta, 0);
        assert_eq!(s.brackets.paren.min, -1);
    }

    // =======================================================================
    // compose tests
    // =======================================================================

    /// Helper: compose(from_str(a), from_str(b)) should match from_str(a+b)
    /// for all fields except min_indent (which is a conservative lower bound).
    fn assert_compose_matches_concat(a: &str, b: &str) {
        let mut sa = TextSummary::from_str(a);
        let sb = TextSummary::from_str(b);
        Summary::compose(&mut sa, &sb);
        let concatenated = TextSummary::from_str(&format!("{}{}", a, b));

        // Check all fields except min_indent.
        assert_eq!(
            sa.metrics, concatenated.metrics,
            "metrics mismatch for compose({:?}, {:?})",
            a, b
        );
        assert_eq!(
            sa.lines, concatenated.lines,
            "lines mismatch for compose({:?}, {:?})",
            a, b
        );
        assert_eq!(
            sa.brackets, concatenated.brackets,
            "brackets mismatch for compose({:?}, {:?})",
            a, b
        );
        assert_eq!(
            sa.indent.flags, concatenated.indent.flags,
            "indent flags mismatch for compose({:?}, {:?})",
            a, b
        );
        // min_indent: compose is a lower bound (<= true value).
        assert!(
            sa.indent.min_indent <= concatenated.indent.min_indent,
            "compose({:?}, {:?}): min_indent {} > {} (should be <=)",
            a,
            b,
            sa.indent.min_indent,
            concatenated.indent.min_indent
        );
    }

    #[test]
    fn compose_is_associative() {
        fn check(a: &str, b: &str, c: &str) {
            let sa = TextSummary::from_str(a);
            let sb = TextSummary::from_str(b);
            let sc = TextSummary::from_str(c);

            let mut left = sa.clone();
            Summary::compose(&mut left, &sb);
            Summary::compose(&mut left, &sc);

            let mut bc = sb.clone();
            Summary::compose(&mut bc, &sc);
            let mut right = sa.clone();
            Summary::compose(&mut right, &bc);

            assert_eq!(
                left, right,
                "associativity failed for ({:?}, {:?}, {:?})",
                a, b, c
            );
        }

        check("hello\n", "world\n", "foo\n");
        check("ab", "cd", "ef");
        check("\n", "hello", "\n");
        check("{", "()", "}");
        check("  a\n", "    b\n", " c\n");
        check("", "hello", "");
        check("", "", "hello");
        check("\u{1F600}", "\n", "x");
    }

    #[test]
    fn compose_identity() {
        let cases = [
            "hello\n",
            "abc",
            "\nhello\n",
            "{foo}\n  bar\n",
            "\u{1F600}\n",
            "",
        ];
        for text in &cases {
            let x = TextSummary::from_str(text);

            // Left identity.
            let mut left = TextSummary::default();
            Summary::compose(&mut left, &x);
            assert_eq!(left, x, "left identity failed for {:?}", text);

            // Right identity.
            let mut right = x.clone();
            Summary::compose(&mut right, &TextSummary::default());
            assert_eq!(right, x, "right identity failed for {:?}", text);
        }
    }

    #[test]
    fn compose_line_seam() {
        // "hel" + "lo\nworld" -> first_line_len=5
        let mut sa = TextSummary::from_str("hel");
        let sb = TextSummary::from_str("lo\nworld");
        Summary::compose(&mut sa, &sb);
        assert_eq!(sa.lines.first_line_len, 5);
        assert_eq!(sa.lines.last_line_len, 5);
        assert_eq!(sa.lines.max_line_len, 5);
    }

    #[test]
    fn compose_blank_line_across_boundary() {
        // "hello\n" + "\nworld" -> seam line is empty -> HAS_BLANK_LINE
        assert_compose_matches_concat("hello\n", "\nworld");
        let mut sa = TextSummary::from_str("hello\n");
        let sb = TextSummary::from_str("\nworld");
        Summary::compose(&mut sa, &sb);
        assert!(sa.has_blank_line());
    }

    #[test]
    fn compose_matches_concat_basic() {
        assert_compose_matches_concat("hello", " world");
        assert_compose_matches_concat("hello\n", "world\n");
        assert_compose_matches_concat("", "hello");
        assert_compose_matches_concat("hello", "");
    }

    #[test]
    fn compose_matches_concat_newlines() {
        assert_compose_matches_concat("ab\n", "cd\n");
        assert_compose_matches_concat("ab", "\ncd");
        assert_compose_matches_concat("\n", "\n");
        assert_compose_matches_concat("a\nb\n", "c\nd\n");
    }

    #[test]
    fn compose_matches_concat_brackets() {
        assert_compose_matches_concat("{(", ")}");
        assert_compose_matches_concat("{{", "");
        assert_compose_matches_concat("", "}}");
    }

    #[test]
    fn compose_matches_concat_indent() {
        assert_compose_matches_concat("  hello\n", "    world\n");
        assert_compose_matches_concat("  hello\n", "\n");
    }

    #[test]
    fn compose_matches_concat_unicode() {
        assert_compose_matches_concat("\u{1F600}", "\u{1F600}");
        assert_compose_matches_concat("a\u{00E9}", "\u{4E16}\u{1F600}");
    }

    #[test]
    fn compose_seam_line_lengths() {
        let mut sa = TextSummary::from_str("ab");
        let sb = TextSummary::from_str("cdef");
        Summary::compose(&mut sa, &sb);
        assert_eq!(sa.lines.max_line_len, 6);
        assert_eq!(sa.lines.first_line_len, 6);
        assert_eq!(sa.lines.last_line_len, 6);
    }

    #[test]
    fn compose_seam_across_newlines() {
        assert_compose_matches_concat("xx\nab", "cdef\nyy");
        let mut sa = TextSummary::from_str("xx\nab");
        let sb = TextSummary::from_str("cdef\nyy");
        Summary::compose(&mut sa, &sb);
        assert_eq!(sa.lines.max_line_len, 6);
    }

    #[test]
    fn compose_preserves_starts_with_newline_from_left() {
        let mut left = TextSummary::from_str("\nhello");
        let right = TextSummary::from_str("world");
        Summary::compose(&mut left, &right);
        assert!(left.indent.flags.contains(IndentFlags::STARTS_WITH_NEWLINE));
    }

    #[test]
    fn compose_does_not_take_starts_with_newline_from_right() {
        let mut left = TextSummary::from_str("hello");
        let right = TextSummary::from_str("\nworld");
        Summary::compose(&mut left, &right);
        assert!(!left.indent.flags.contains(IndentFlags::STARTS_WITH_NEWLINE));
    }

    #[test]
    fn compose_preserves_ends_with_newline_from_right() {
        let mut left = TextSummary::from_str("hello");
        let right = TextSummary::from_str("world\n");
        Summary::compose(&mut left, &right);
        assert!(left.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE));
    }

    #[test]
    fn compose_does_not_take_ends_with_newline_from_left() {
        let mut left = TextSummary::from_str("hello\n");
        let right = TextSummary::from_str("world");
        Summary::compose(&mut left, &right);
        assert!(!left.indent.flags.contains(IndentFlags::ENDS_WITH_NEWLINE));
    }

    // =======================================================================
    // MetricsSummary subtract
    // =======================================================================

    #[test]
    fn metrics_subtract_roundtrip() {
        let a = TextSummary::from_str("hello\n").metrics;
        let b = TextSummary::from_str("world\n").metrics;

        let mut ab = a;
        MetricsSummary::compose(&mut ab, &b);
        MetricsSummary::subtract(&mut ab, &b);
        assert_eq!(ab, a);
    }

    #[test]
    fn metrics_subtract_with_unicode() {
        let a = TextSummary::from_str("\u{1F600}hello").metrics;
        let b = TextSummary::from_str("\u{4E16}\n").metrics;

        let mut ab = a;
        MetricsSummary::compose(&mut ab, &b);
        MetricsSummary::subtract(&mut ab, &b);
        assert_eq!(ab, a);
    }

    // =======================================================================
    // Dimension tests
    // =======================================================================

    #[test]
    fn byte_offset_from_summary() {
        let s = TextSummary::from_str("hello\nworld");
        let d = ByteOffset::from_summary(&s);
        assert_eq!(d, ByteOffset(11));
    }

    #[test]
    fn line_offset_from_summary() {
        let s = TextSummary::from_str("a\nb\nc\n");
        let d = LineOffset::from_summary(&s);
        assert_eq!(d, LineOffset(3));
    }

    #[test]
    fn char_offset_from_summary() {
        let s = TextSummary::from_str("a\u{00E9}\u{1F600}");
        let d = CharOffset::from_summary(&s);
        assert_eq!(d, CharOffset(3));
    }

    #[test]
    fn utf16_offset_from_summary() {
        let s = TextSummary::from_str("a\u{1F600}"); // 1 + 2 = 3 UTF-16 units
        let d = Utf16Offset::from_summary(&s);
        assert_eq!(d, Utf16Offset(3));
    }

    #[test]
    fn byte_offset_add_summary() {
        let s1 = TextSummary::from_str("hello");
        let s2 = TextSummary::from_str(" world");
        let mut d = ByteOffset::default();
        d.add_summary(&s1);
        d.add_summary(&s2);
        assert_eq!(d, ByteOffset(11));
    }

    #[test]
    fn line_offset_monotonic() {
        let chunks = ["ab\n", "cd\nef\n", "gh"];
        let mut d = LineOffset::default();
        let mut prev = d;
        for chunk in &chunks {
            let s = TextSummary::from_str(chunk);
            d.add_summary(&s);
            assert!(
                d >= prev,
                "line offset must be monotonically non-decreasing"
            );
            prev = d;
        }
        assert_eq!(d, LineOffset(3));
    }

    // =======================================================================
    // Exhaustive compose-matches-concat
    // =======================================================================

    #[test]
    fn compose_matches_concat_exhaustive_patterns() {
        let patterns = [
            "",
            "a",
            "\n",
            "a\n",
            "\na",
            "a\nb",
            "\n\n",
            "a\n\n",
            "\n\na",
            "a\nb\n",
            "\na\nb",
            "a\nb\nc",
            "  ",
            "  \n",
            "\n  ",
            "\u{00E9}",
            "\u{1F600}",
        ];
        for a in &patterns {
            for b in &patterns {
                assert_compose_matches_concat(a, b);
            }
        }
    }

    #[test]
    fn compose_associativity_exhaustive() {
        let patterns = ["", "a", "\n", "a\n", "\na", "a\nb", "  ", "\u{00E9}"];
        for a in &patterns {
            for b in &patterns {
                for c in &patterns {
                    let sa = TextSummary::from_str(a);
                    let sb = TextSummary::from_str(b);
                    let sc = TextSummary::from_str(c);

                    let mut left = sa.clone();
                    Summary::compose(&mut left, &sb);
                    Summary::compose(&mut left, &sc);

                    let mut bc = sb.clone();
                    Summary::compose(&mut bc, &sc);
                    let mut right = sa.clone();
                    Summary::compose(&mut right, &bc);

                    assert_eq!(
                        left, right,
                        "associativity failed: ({:?}, {:?}, {:?})",
                        a, b, c
                    );
                }
            }
        }
    }

    // =======================================================================
    // Bracket compose detailed
    // =======================================================================

    #[test]
    fn bracket_min_compose_detailed() {
        let mut a = TextSummary::from_str("(("); // paren: delta=2, min=0
        let b = TextSummary::from_str(")))("); // paren: delta=-2, min=-3

        Summary::compose(&mut a, &b);
        // min = 0.min(2 + (-3)) = 0.min(-1) = -1
        assert_eq!(a.brackets.paren.min, -1);
        // delta = 2 + (-2) = 0
        assert_eq!(a.brackets.paren.delta, 0);
    }

    #[test]
    fn bracket_min_compose_is_associative() {
        let a = TextSummary::from_str("(("); // delta=2, min=0
        let b = TextSummary::from_str(")))("); // delta=-2, min=-3
        let c = TextSummary::from_str(")"); // delta=-1, min=-1

        let mut ab = a.clone();
        Summary::compose(&mut ab, &b);
        let mut abc_left = ab;
        Summary::compose(&mut abc_left, &c);

        let mut bc = b.clone();
        Summary::compose(&mut bc, &c);
        let mut abc_right = a.clone();
        Summary::compose(&mut abc_right, &bc);

        assert_eq!(
            abc_left.brackets.paren.delta,
            abc_right.brackets.paren.delta
        );
        assert_eq!(abc_left.brackets.paren.min, abc_right.brackets.paren.min);
        assert_eq!(abc_left.brackets.paren.max, abc_right.brackets.paren.max);
    }

    // =======================================================================
    // Bracket max tracking
    // =======================================================================

    #[test]
    fn bracket_pair_max_compose() {
        // "((" -> delta=2, min=0, max=2
        let mut a = BracketPair {
            delta: 2,
            min: 0,
            max: 2,
        };
        // "))" -> delta=-2, min=-2, max=0
        let b = BracketPair {
            delta: -2,
            min: -2,
            max: 0,
        };
        a.compose(&b);
        // "(())" -> delta=0, min=0, max=2
        assert_eq!(a.delta, 0);
        assert_eq!(a.min, 0);
        assert_eq!(a.max, 2);
    }

    #[test]
    fn bracket_pair_max_from_str() {
        let s = TextSummary::from_str("(()");
        assert_eq!(s.brackets.paren.delta, 1);
        assert_eq!(s.brackets.paren.min, 0);
        assert_eq!(s.brackets.paren.max, 2);

        let s = TextSummary::from_str(")))(");
        assert_eq!(s.brackets.paren.delta, -2);
        assert_eq!(s.brackets.paren.min, -3);
        // Depths L->R: 0, -1, -2, -3, -2. Max excursion is 0 (never goes positive).
        assert_eq!(s.brackets.paren.max, 0);
    }

    #[test]
    fn bracket_pair_max_identity() {
        let bp = BracketPair::default();
        assert_eq!(bp.max, 0);
    }

    // =======================================================================
    // MetricsSummary as Summary trait
    // =======================================================================

    #[test]
    fn metrics_summary_trait_roundtrip() {
        let mut a = MetricsSummary {
            bytes: 10,
            chars: 8,
            newlines: 2,
            utf16_len: 9,
        };
        let b = MetricsSummary {
            bytes: 5,
            chars: 5,
            newlines: 1,
            utf16_len: 5,
        };

        Summary::compose(&mut a, &b);
        assert_eq!(a.bytes, 15);
        assert_eq!(a.chars, 13);
        assert_eq!(a.newlines, 3);
        assert_eq!(a.utf16_len, 14);
        assert_eq!(a.base_len(), 15);

        InvertibleSummary::subtract(&mut a, &b);
        assert_eq!(a.bytes, 10);
        assert_eq!(a.chars, 8);
        assert_eq!(a.newlines, 2);
        assert_eq!(a.utf16_len, 9);
    }

    // =======================================================================
    // Edge case: compose with blank and nonblank
    // =======================================================================

    #[test]
    fn compose_blank_and_nonblank() {
        let mut a = TextSummary::from_str("\n");
        let b = TextSummary::from_str("hello\n");
        Summary::compose(&mut a, &b);
        assert!(a.has_blank_line());
        assert!(a.has_nonblank_line());
    }

    #[test]
    fn all_ascii_flag_set_for_ascii() {
        let s = TextSummary::from_str("hello world\n");
        assert!(s.indent.flags.contains(IndentFlags::ALL_ASCII));
    }

    #[test]
    fn all_ascii_flag_cleared_for_non_ascii() {
        let s = TextSummary::from_str("h\u{00E9}llo");
        assert!(!s.indent.flags.contains(IndentFlags::ALL_ASCII));
    }

    #[test]
    fn compose_first_and_last_line_len() {
        let mut a = TextSummary::from_str("ab\ncd");
        let b = TextSummary::from_str("ef\ngh");
        Summary::compose(&mut a, &b);
        assert_eq!(a.lines.first_line_len, 2);
        assert_eq!(a.lines.last_line_len, 2);
        assert_eq!(a.lines.max_line_len, 4);
    }

    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn compose_associativity_random_unicode(
                a in "\\PC{0,50}",
                b in "\\PC{0,50}",
                c in "\\PC{0,50}",
            ) {
                let sa = TextSummary::from_str(&a);
                let sb = TextSummary::from_str(&b);
                let sc = TextSummary::from_str(&c);

                // (a . b) . c
                let mut ab = sa.clone();
                ab.compose(&sb);
                let mut ab_c = ab.clone();
                ab_c.compose(&sc);

                // a . (b . c)
                let mut bc = sb.clone();
                bc.compose(&sc);
                let mut a_bc = sa.clone();
                a_bc.compose(&bc);

                // Metrics must be exactly equal (additive)
                prop_assert_eq!(ab_c.metrics, a_bc.metrics, "metrics not associative");
                // Brackets must be exactly equal (delta+min monoid)
                prop_assert_eq!(ab_c.brackets, a_bc.brackets, "brackets not associative");
                // Lines: max_line_len must be equal
                prop_assert_eq!(ab_c.lines.max_line_len, a_bc.lines.max_line_len, "max_line_len not associative");
                // Indent: min_indent must be equal
                prop_assert_eq!(ab_c.indent.min_indent, a_bc.indent.min_indent, "min_indent not associative");
            }

            #[test]
            fn compose_matches_concat_random_unicode(
                a in "\\PC{0,80}",
                b in "\\PC{0,80}",
            ) {
                let sa = TextSummary::from_str(&a);
                let sb = TextSummary::from_str(&b);

                let mut composed = sa.clone();
                composed.compose(&sb);

                let concatenated = TextSummary::from_str(&format!("{}{}", a, b));

                // Metrics must match exactly
                prop_assert_eq!(composed.metrics, concatenated.metrics,
                    "metrics: compose({:?}, {:?}) != from_str(concat)", a, b);
                // Brackets must match exactly
                prop_assert_eq!(composed.brackets, concatenated.brackets,
                    "brackets mismatch");
                // Lines first/last/max must match
                prop_assert_eq!(composed.lines, concatenated.lines,
                    "lines mismatch");
                // Indent: compose is a lower bound (may be <= concatenated)
                prop_assert!(composed.indent.min_indent <= concatenated.indent.min_indent,
                    "compose indent {} > concat indent {}", composed.indent.min_indent, concatenated.indent.min_indent);
                prop_assert_eq!(composed.indent.flags, concatenated.indent.flags,
                    "indent flags mismatch");
            }
        }
    }
}
