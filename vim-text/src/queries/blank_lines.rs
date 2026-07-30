//! Summary-accelerated blank-line query methods for VimText.
//!
//! These methods exploit the IndentFlags (HAS_BLANK_LINE, FIRST_LINE_BLANK,
//! LAST_LINE_BLANK, HAS_NONBLANK_LINE) stored at each tree node to enable
//! subtree pruning. The blank-line queries achieve O(log n) in the common
//! case where blank lines are sparse, by skipping entire subtrees whose
//! summary lacks the relevant flag.

use std::sync::Arc;

use crate::chunk::TextChunk;
use crate::summary::{ByteOffset, IndentFlags, TextSummary};
use crate::tree::node::{Node, DEFAULT_B};
use crate::tree::Bias;
use crate::VimText;

use super::brackets;
use super::indent;

/// Vim-domain structural queries. Extension trait on VimText.
pub trait VimQueries {
    /// Find the next blank line after `line` (exclusive of `line` itself).
    /// Returns `None` if no blank line exists after `line`.
    fn next_blank_line_from(&self, line: usize) -> Option<usize>;

    /// Find the previous blank line before `line` (exclusive of `line` itself).
    /// Returns `None` if no blank line exists before `line`.
    fn prev_blank_line_from(&self, line: usize) -> Option<usize>;

    /// Find the next non-blank line after `line` (exclusive of `line` itself).
    /// Returns `None` if no non-blank line exists after `line`.
    fn next_nonblank_line_from(&self, line: usize) -> Option<usize>;

    /// Find the previous non-blank line before `line` (exclusive of `line` itself).
    /// Returns `None` if no non-blank line exists before `line`.
    fn prev_nonblank_line_from(&self, line: usize) -> Option<usize>;

    /// Check if any blank line exists in line range `[start_line, end_line)`.
    /// Returns `false` for empty or inverted ranges.
    fn has_blank_line_in_range(&self, start_line: usize, end_line: usize) -> bool;

    /// Check if a given line is blank (contains only whitespace or is empty).
    fn is_line_blank(&self, line: usize) -> bool;

    /// Find the minimum indentation among non-blank lines in `[start_line, end_line)`.
    ///
    /// "Indent" = number of leading spaces/tabs (tabs count as 1 unit).
    /// Blank lines (all whitespace or empty) are ignored.
    /// Returns `u16::MAX` if all lines in the range are blank (identity element).
    fn min_indent_in_range(&self, start_line: usize, end_line: usize) -> u16;

    /// Find the matching bracket for the bracket character at `offset`.
    ///
    /// Supports `()`, `[]`, and `{}`. Open brackets search forward, close
    /// brackets search backward. Returns `None` if:
    /// - `offset` is out of bounds
    /// - the byte at `offset` is not a bracket character
    /// - no matching bracket exists (unbalanced)
    fn matching_bracket(&self, offset: usize) -> Option<usize>;

    /// Return the parenthesis nesting depth at `offset`.
    ///
    /// Walks from the start of the document to `offset`, counting `(` as +1
    /// and `)` as -1. Only parentheses are counted (not `[]` or `{}`).
    fn bracket_depth_at(&self, offset: usize) -> i16;
}

impl VimQueries for VimText {
    fn next_blank_line_from(&self, line: usize) -> Option<usize> {
        let line_count = self.line_count();
        if line + 1 >= line_count {
            return None;
        }
        find_blank_line_forward(self.tree.root(), line + 1, 0, false)
    }

    fn prev_blank_line_from(&self, line: usize) -> Option<usize> {
        if line == 0 {
            return None;
        }
        let line_count = self.line_count();
        let search_before = line.min(line_count);
        find_blank_line_backward(self.tree.root(), search_before, 0, false)
    }

    fn next_nonblank_line_from(&self, line: usize) -> Option<usize> {
        let line_count = self.line_count();
        if line + 1 >= line_count {
            return None;
        }
        find_nonblank_line_forward(self.tree.root(), line + 1, 0)
    }

    fn prev_nonblank_line_from(&self, line: usize) -> Option<usize> {
        if line == 0 {
            return None;
        }
        let line_count = self.line_count();
        let search_before = line.min(line_count);
        find_nonblank_line_backward(self.tree.root(), search_before, 0, false)
    }

    fn has_blank_line_in_range(&self, start_line: usize, end_line: usize) -> bool {
        if start_line >= end_line {
            return false;
        }
        if start_line == 0 {
            if self.is_line_blank(0) {
                return true;
            }
            self.next_blank_line_from(0)
                .map(|l| l < end_line)
                .unwrap_or(false)
        } else {
            // Search from (start_line - 1) so that start_line itself is included
            // in the next_blank_line_from search.
            self.next_blank_line_from(start_line - 1)
                .map(|l| l < end_line)
                .unwrap_or(false)
        }
    }

    fn is_line_blank(&self, line: usize) -> bool {
        let start = match self.line_start(line) {
            Some(s) => s,
            None => return false,
        };
        let end = match self.line_end(line) {
            Some(e) => e,
            None => return false,
        };
        if start == end {
            return true; // empty line
        }
        // O(log n + line_length): seek cursor to `start`, then walk chunks to `end`.
        // Bias::Right so that seeking to a chunk boundary lands on the chunk
        // that *starts* at that offset.
        let mut cursor = self.tree.cursor::<ByteOffset>();
        cursor.seek(&ByteOffset(start as u32), Bias::Right);
        let mut pos = start;
        while let Some(chunk) = cursor.item() {
            let chunk_start = cursor.start::<ByteOffset>().0 as usize;
            let chunk_bytes = chunk.as_bytes();
            let local_start = pos.saturating_sub(chunk_start);
            let local_end = (end - chunk_start).min(chunk_bytes.len());
            for &b in &chunk_bytes[local_start..local_end] {
                if b != b' ' && b != b'\t' && b != b'\r' {
                    return false;
                }
            }
            pos = chunk_start + chunk_bytes.len();
            if pos >= end {
                break;
            }
            if !cursor.next() {
                break;
            }
        }
        true
    }

    fn min_indent_in_range(&self, start_line: usize, end_line: usize) -> u16 {
        if start_line >= end_line {
            return u16::MAX;
        }
        let line_count = self.line_count();
        let effective_end = end_line.min(line_count);
        if start_line >= effective_end {
            return u16::MAX;
        }

        // Get byte range covering [start_line, effective_end).
        let range_start_byte = match self.line_start(start_line) {
            Some(s) => s,
            None => return u16::MAX,
        };
        let range_end_byte = self
            .line_start(effective_end)
            .unwrap_or_else(|| self.byte_len());

        if range_start_byte >= range_end_byte {
            // All lines in range are empty (zero bytes).
            return u16::MAX;
        }

        indent::min_indent_in_byte_range(self, range_start_byte, range_end_byte)
    }

    fn matching_bracket(&self, offset: usize) -> Option<usize> {
        if offset >= self.byte_len() {
            return None;
        }

        // Read the byte at `offset` via the bracket helper (uses Bias::Right).
        let byte = brackets::byte_at_offset(self, offset)?;

        let (open, close, forward) = match byte {
            b'(' => (b'(', b')', true),
            b')' => (b'(', b')', false),
            b'[' => (b'[', b']', true),
            b']' => (b'[', b']', false),
            b'{' => (b'{', b'}', true),
            b'}' => (b'{', b'}', false),
            _ => return None,
        };

        if forward {
            brackets::find_matching_forward(self, offset, open, close)
        } else {
            brackets::find_matching_backward(self, offset, open, close)
        }
    }

    fn bracket_depth_at(&self, offset: usize) -> i16 {
        brackets::compute_bracket_depth(&self.tree, offset)
    }
}

// ---------------------------------------------------------------------------
// Tree-pruned forward blank line search
// ---------------------------------------------------------------------------

/// Find the first blank line at or after `target_line` by walking the tree.
/// `base_line` is the line number of the first line in `node`'s subtree.
/// `first_is_continuation` indicates whether the first text segment of this
/// node is a continuation of a line from a previous sibling.
fn find_blank_line_forward(
    node: &Arc<Node<TextChunk, DEFAULT_B>>,
    target_line: usize,
    base_line: usize,
    first_is_continuation: bool,
) -> Option<usize> {
    let summary = node.summary();

    // Quick prune: if this subtree has no blank line at all, skip it entirely.
    if !summary.has_blank_line() {
        return None;
    }

    match node.as_ref() {
        Node::Internal(internal) => {
            let mut current_line = base_line;
            for i in 0..internal.children.len() {
                let child_summary = &internal.summaries[i];
                let child_lines = lines_in_summary(child_summary);

                // Skip children entirely before our target line.
                if current_line + child_lines <= target_line {
                    current_line += child_lines;
                    continue;
                }

                // This child overlaps with our search range.
                if child_summary.has_blank_line() {
                    let child_continuation = if i == 0 {
                        first_is_continuation
                    } else {
                        !internal.summaries[i - 1]
                            .indent
                            .flags
                            .contains(IndentFlags::ENDS_WITH_NEWLINE)
                    };

                    if let Some(result) = find_blank_line_forward(
                        &internal.children[i],
                        target_line,
                        current_line,
                        child_continuation,
                    ) {
                        return Some(result);
                    }
                }
                current_line += child_lines;
            }
            None
        }
        Node::Leaf { item, .. } => {
            scan_leaf_blank_forward(item.as_str(), base_line, target_line, first_is_continuation)
        }
    }
}

// ---------------------------------------------------------------------------
// Tree-pruned backward blank line search
// ---------------------------------------------------------------------------

/// Find the last blank line before `target_line` by walking the tree backward.
fn find_blank_line_backward(
    node: &Arc<Node<TextChunk, DEFAULT_B>>,
    target_line: usize,
    base_line: usize,
    first_is_continuation: bool,
) -> Option<usize> {
    let summary = node.summary();

    if !summary.has_blank_line() {
        return None;
    }

    match node.as_ref() {
        Node::Internal(internal) => {
            // Compute base line for each child.
            let mut child_base_lines: Vec<usize> = Vec::with_capacity(internal.children.len());
            let mut current_line = base_line;
            for summary in &internal.summaries {
                child_base_lines.push(current_line);
                current_line += lines_in_summary(summary);
            }

            // Walk children in reverse.
            for i in (0..internal.children.len()).rev() {
                let child_base = child_base_lines[i];
                let child_summary = &internal.summaries[i];
                let child_lines = lines_in_summary(child_summary);

                // Skip children entirely at or after our target line.
                if child_base >= target_line {
                    continue;
                }

                let effective_target = target_line.min(child_base + child_lines);

                if child_summary.has_blank_line() {
                    let child_continuation = if i == 0 {
                        first_is_continuation
                    } else {
                        !internal.summaries[i - 1]
                            .indent
                            .flags
                            .contains(IndentFlags::ENDS_WITH_NEWLINE)
                    };

                    if let Some(result) = find_blank_line_backward(
                        &internal.children[i],
                        effective_target,
                        child_base,
                        child_continuation,
                    ) {
                        return Some(result);
                    }
                }
            }
            None
        }
        Node::Leaf { item, .. } => {
            scan_leaf_blank_backward(item.as_str(), base_line, target_line, first_is_continuation)
        }
    }
}

// ---------------------------------------------------------------------------
// Tree-pruned forward non-blank line search
// ---------------------------------------------------------------------------

/// Find the first non-blank line at or after `target_line` by walking the tree.
fn find_nonblank_line_forward(
    node: &Arc<Node<TextChunk, DEFAULT_B>>,
    target_line: usize,
    base_line: usize,
) -> Option<usize> {
    let summary = node.summary();

    if !summary.has_nonblank_line() {
        return None;
    }

    match node.as_ref() {
        Node::Internal(internal) => {
            let mut current_line = base_line;
            for i in 0..internal.children.len() {
                let child_summary = &internal.summaries[i];
                let child_lines = lines_in_summary(child_summary);

                if current_line + child_lines <= target_line {
                    current_line += child_lines;
                    continue;
                }

                if child_summary.has_nonblank_line() {
                    if let Some(result) =
                        find_nonblank_line_forward(&internal.children[i], target_line, current_line)
                    {
                        return Some(result);
                    }
                }
                current_line += child_lines;
            }
            None
        }
        Node::Leaf { item, .. } => {
            scan_leaf_nonblank_forward(item.as_str(), base_line, target_line)
        }
    }
}

// ---------------------------------------------------------------------------
// Tree-pruned backward non-blank line search
// ---------------------------------------------------------------------------

/// Find the last non-blank line before `target_line` by walking the tree backward.
fn find_nonblank_line_backward(
    node: &Arc<Node<TextChunk, DEFAULT_B>>,
    target_line: usize,
    base_line: usize,
    first_is_continuation: bool,
) -> Option<usize> {
    let summary = node.summary();

    if !summary.has_nonblank_line() {
        return None;
    }

    match node.as_ref() {
        Node::Internal(internal) => {
            let mut child_base_lines: Vec<usize> = Vec::with_capacity(internal.children.len());
            let mut current_line = base_line;
            for summary in &internal.summaries {
                child_base_lines.push(current_line);
                current_line += lines_in_summary(summary);
            }

            for i in (0..internal.children.len()).rev() {
                let child_base = child_base_lines[i];
                let child_summary = &internal.summaries[i];
                let child_lines = lines_in_summary(child_summary);

                if child_base >= target_line {
                    continue;
                }

                let effective_target = target_line.min(child_base + child_lines);

                if child_summary.has_nonblank_line() {
                    let child_continuation = if i == 0 {
                        first_is_continuation
                    } else {
                        !internal.summaries[i - 1]
                            .indent
                            .flags
                            .contains(IndentFlags::ENDS_WITH_NEWLINE)
                    };

                    if let Some(result) = find_nonblank_line_backward(
                        &internal.children[i],
                        effective_target,
                        child_base,
                        child_continuation,
                    ) {
                        return Some(result);
                    }
                }
            }
            None
        }
        Node::Leaf { item, .. } => scan_leaf_nonblank_backward(
            item.as_str(),
            base_line,
            target_line,
            first_is_continuation,
        ),
    }
}

// ---------------------------------------------------------------------------
// Leaf-level scanning helpers
// ---------------------------------------------------------------------------

/// Scan a leaf's text for the first blank line at or after `target_line`.
fn scan_leaf_blank_forward(
    text: &str,
    base_line: usize,
    target_line: usize,
    first_is_continuation: bool,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut line_in_leaf: usize = 0;
    let mut line_start: usize = 0;

    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' {
            let actual_line = base_line + line_in_leaf;
            if actual_line >= target_line
                && !(line_in_leaf == 0 && first_is_continuation)
                && is_blank_bytes(&bytes[line_start..i])
            {
                return Some(actual_line);
            }
            line_in_leaf += 1;
            line_start = i + 1;
        }
    }

    // Check the last segment (no trailing newline).
    if line_start < bytes.len() {
        let actual_line = base_line + line_in_leaf;
        if actual_line >= target_line
            && !(line_in_leaf == 0 && first_is_continuation)
            && is_blank_bytes(&bytes[line_start..])
        {
            return Some(actual_line);
        }
    }
    None
}

/// Scan a leaf's text for the last blank line before `target_line`.
fn scan_leaf_blank_backward(
    text: &str,
    base_line: usize,
    target_line: usize,
    first_is_continuation: bool,
) -> Option<usize> {
    let bytes = text.as_bytes();

    // Build line boundaries.
    let mut line_starts: Vec<usize> = Vec::with_capacity(32);
    line_starts.push(0);
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }

    let ends_with_newline = bytes.last() == Some(&b'\n');
    let num_lines = if ends_with_newline {
        line_starts.len() - 1
    } else {
        line_starts.len()
    };

    for line_idx in (0..num_lines).rev() {
        let actual_line = base_line + line_idx;

        if actual_line >= target_line {
            continue;
        }

        if line_idx == 0 && first_is_continuation {
            continue;
        }

        let start = line_starts[line_idx];
        let end = if line_idx + 1 < line_starts.len() {
            line_starts[line_idx + 1] - 1
        } else {
            bytes.len()
        };

        if is_blank_bytes(&bytes[start..end]) {
            return Some(actual_line);
        }
    }
    None
}

/// Scan a leaf's text for the first non-blank line at or after `target_line`.
fn scan_leaf_nonblank_forward(text: &str, base_line: usize, target_line: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut line_in_leaf: usize = 0;
    let mut line_start: usize = 0;

    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' {
            let actual_line = base_line + line_in_leaf;
            if actual_line >= target_line && !is_blank_bytes(&bytes[line_start..i]) {
                return Some(actual_line);
            }
            line_in_leaf += 1;
            line_start = i + 1;
        }
    }

    // Check the last segment.
    if line_start < bytes.len() {
        let actual_line = base_line + line_in_leaf;
        if actual_line >= target_line && !is_blank_bytes(&bytes[line_start..]) {
            return Some(actual_line);
        }
    }
    None
}

/// Scan a leaf's text for the last non-blank line before `target_line`.
fn scan_leaf_nonblank_backward(
    text: &str,
    base_line: usize,
    target_line: usize,
    first_is_continuation: bool,
) -> Option<usize> {
    let bytes = text.as_bytes();

    let mut line_starts: Vec<usize> = Vec::with_capacity(32);
    line_starts.push(0);
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }

    let ends_with_newline = bytes.last() == Some(&b'\n');
    let num_lines = if ends_with_newline {
        line_starts.len() - 1
    } else {
        line_starts.len()
    };

    for line_idx in (0..num_lines).rev() {
        let actual_line = base_line + line_idx;

        if actual_line >= target_line {
            continue;
        }

        if line_idx == 0 && first_is_continuation {
            continue;
        }

        let start = line_starts[line_idx];
        let end = if line_idx + 1 < line_starts.len() {
            line_starts[line_idx + 1] - 1
        } else {
            bytes.len()
        };

        if !is_blank_bytes(&bytes[start..end]) {
            return Some(actual_line);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute how many complete line boundaries a subtree contributes.
///
/// Each newline terminates a line. A child with N newlines contains lines
/// `base_line` through `base_line + N - 1` fully, with a partial (N+1)th line
/// that flows into the next child.
#[inline]
fn lines_in_summary(summary: &TextSummary) -> usize {
    summary.metrics.newlines as usize
}

/// Check if a byte slice represents a blank line (empty or only whitespace).
#[inline]
fn is_blank_bytes(bytes: &[u8]) -> bool {
    bytes.iter().all(|&b| b == b' ' || b == b'\t' || b == b'\r')
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::VimText;

    // === is_line_blank ===

    #[test]
    fn is_line_blank_basic() {
        let t = VimText::from_str("hello\n\nworld\n   \nfoo");
        assert!(!t.is_line_blank(0)); // "hello"
        assert!(t.is_line_blank(1)); // ""
        assert!(!t.is_line_blank(2)); // "world"
        assert!(t.is_line_blank(3)); // "   "
        assert!(!t.is_line_blank(4)); // "foo"
    }

    #[test]
    fn is_line_blank_empty_doc() {
        let t = VimText::from_str("");
        assert!(t.is_line_blank(0));
    }

    #[test]
    fn is_line_blank_tab_only() {
        let t = VimText::from_str("hello\n\t\nworld");
        assert!(t.is_line_blank(1)); // "\t"
    }

    #[test]
    fn is_line_blank_cr_only() {
        let t = VimText::from_str("hello\n\r\nworld");
        assert!(t.is_line_blank(1)); // "\r"
    }

    #[test]
    fn is_line_blank_out_of_bounds() {
        let t = VimText::from_str("hello");
        assert!(!t.is_line_blank(99));
    }

    // === next_blank_line_from ===

    #[test]
    fn next_blank_line_basic() {
        let t = VimText::from_str("hello\nworld\n\nfoo\n\nbar");
        assert_eq!(t.next_blank_line_from(0), Some(2));
        assert_eq!(t.next_blank_line_from(2), Some(4));
        assert_eq!(t.next_blank_line_from(4), None);
    }

    #[test]
    fn next_blank_line_empty_doc() {
        let t = VimText::from_str("");
        assert_eq!(t.next_blank_line_from(0), None);
    }

    #[test]
    fn next_blank_line_no_blanks() {
        let t = VimText::from_str("a\nb\nc");
        assert_eq!(t.next_blank_line_from(0), None);
    }

    #[test]
    fn next_blank_line_from_beyond_end() {
        let t = VimText::from_str("hello\nworld");
        assert_eq!(t.next_blank_line_from(10), None);
    }

    #[test]
    fn next_blank_line_consecutive() {
        let t = VimText::from_str("hello\n\n\n\nworld");
        assert_eq!(t.next_blank_line_from(0), Some(1));
        assert_eq!(t.next_blank_line_from(1), Some(2));
        assert_eq!(t.next_blank_line_from(2), Some(3));
        assert_eq!(t.next_blank_line_from(3), None);
    }

    #[test]
    fn next_blank_line_whitespace_only() {
        let t = VimText::from_str("hello\n   \nworld");
        assert_eq!(t.next_blank_line_from(0), Some(1));
    }

    // === prev_blank_line_from ===

    #[test]
    fn prev_blank_line_basic() {
        let t = VimText::from_str("hello\n\nworld\n\nfoo");
        assert_eq!(t.prev_blank_line_from(4), Some(3));
        assert_eq!(t.prev_blank_line_from(3), Some(1));
        assert_eq!(t.prev_blank_line_from(1), None);
    }

    #[test]
    fn prev_blank_line_empty_doc() {
        let t = VimText::from_str("");
        assert_eq!(t.prev_blank_line_from(0), None);
    }

    #[test]
    fn prev_blank_line_from_zero() {
        let t = VimText::from_str("\nhello");
        assert_eq!(t.prev_blank_line_from(0), None);
    }

    #[test]
    fn prev_blank_line_consecutive() {
        let t = VimText::from_str("hello\n\n\n\nworld");
        assert_eq!(t.prev_blank_line_from(4), Some(3));
        assert_eq!(t.prev_blank_line_from(3), Some(2));
        assert_eq!(t.prev_blank_line_from(2), Some(1));
        assert_eq!(t.prev_blank_line_from(1), None);
    }

    #[test]
    fn prev_blank_line_from_beyond_end() {
        let t = VimText::from_str("hello\n\nworld");
        assert_eq!(t.prev_blank_line_from(10), Some(1));
    }

    // === next_nonblank_line_from ===

    #[test]
    fn next_nonblank_line_basic() {
        let t = VimText::from_str("\n\nhello\n\n\nworld");
        assert_eq!(t.next_nonblank_line_from(0), Some(2));
        assert_eq!(t.next_nonblank_line_from(2), Some(5));
        assert_eq!(t.next_nonblank_line_from(5), None);
    }

    #[test]
    fn next_nonblank_line_all_blank() {
        let t = VimText::from_str("\n\n\n");
        assert_eq!(t.next_nonblank_line_from(0), None);
    }

    #[test]
    fn next_nonblank_line_empty_doc() {
        let t = VimText::from_str("");
        assert_eq!(t.next_nonblank_line_from(0), None);
    }

    // === prev_nonblank_line_from ===

    #[test]
    fn prev_nonblank_line_basic() {
        let t = VimText::from_str("hello\n\n\nworld\n\n");
        assert_eq!(t.prev_nonblank_line_from(5), Some(3));
        assert_eq!(t.prev_nonblank_line_from(3), Some(0));
        assert_eq!(t.prev_nonblank_line_from(0), None);
    }

    #[test]
    fn prev_nonblank_line_all_blank() {
        let t = VimText::from_str("\n\n\n");
        assert_eq!(t.prev_nonblank_line_from(3), None);
    }

    #[test]
    fn prev_nonblank_line_empty_doc() {
        let t = VimText::from_str("");
        assert_eq!(t.prev_nonblank_line_from(0), None);
    }

    // === has_blank_line_in_range ===

    #[test]
    fn has_blank_line_in_range_basic() {
        let t = VimText::from_str("hello\n\nworld\nfoo");
        assert!(t.has_blank_line_in_range(0, 3));
        assert!(!t.has_blank_line_in_range(2, 4));
    }

    #[test]
    fn has_blank_line_in_range_exact_blank() {
        let t = VimText::from_str("hello\n\nworld");
        assert!(t.has_blank_line_in_range(1, 2)); // exactly the blank line
    }

    #[test]
    fn has_blank_line_in_range_empty_range() {
        let t = VimText::from_str("hello\n\nworld");
        assert!(!t.has_blank_line_in_range(2, 2));
        assert!(!t.has_blank_line_in_range(5, 3)); // inverted
    }

    #[test]
    fn has_blank_line_in_range_all_blank() {
        let t = VimText::from_str("\n\n\n");
        assert!(t.has_blank_line_in_range(0, 3));
    }

    #[test]
    fn has_blank_line_in_range_no_blanks() {
        let t = VimText::from_str("a\nb\nc");
        assert!(!t.has_blank_line_in_range(0, 3));
    }

    // === blank_queries_empty_doc ===

    #[test]
    fn blank_queries_empty_doc() {
        let t = VimText::from_str("");
        assert!(t.is_line_blank(0));
        assert_eq!(t.next_blank_line_from(0), None);
        assert_eq!(t.prev_blank_line_from(0), None);
    }

    // === Large file tree-pruning tests ===

    #[test]
    fn next_blank_line_large_file() {
        let mut text = "content\n".repeat(10000);
        text.push('\n'); // blank line at line 10000
        text.push_str(&"more\n".repeat(10000));
        let t = VimText::from_str(&text);
        assert_eq!(t.next_blank_line_from(0), Some(10000));
        assert_eq!(t.next_blank_line_from(10000), None);
    }

    #[test]
    fn prev_blank_line_large_file() {
        let mut text = "content\n".repeat(10000);
        text.push('\n'); // blank line at line 10000
        text.push_str(&"more\n".repeat(10000));
        let t = VimText::from_str(&text);
        assert_eq!(t.prev_blank_line_from(20001), Some(10000));
        assert_eq!(t.prev_blank_line_from(10000), None);
    }

    #[test]
    fn next_blank_line_no_blank_large_file() {
        let text = "content\n".repeat(10000);
        let t = VimText::from_str(&text);
        assert_eq!(t.next_blank_line_from(0), None);
    }

    #[test]
    fn prev_blank_line_no_blank_large_file() {
        let text = "content\n".repeat(10000);
        let t = VimText::from_str(&text);
        assert_eq!(t.prev_blank_line_from(9999), None);
    }

    #[test]
    fn next_nonblank_line_large_file() {
        let mut text = "\n".repeat(1000);
        text.push_str("content\n");
        let t = VimText::from_str(&text);
        assert_eq!(t.next_nonblank_line_from(0), Some(1000));
    }

    // === First/last line edge cases ===

    #[test]
    fn first_line_blank() {
        let t = VimText::from_str("\nhello\nworld");
        assert_eq!(t.prev_blank_line_from(1), Some(0));
        assert_eq!(t.prev_blank_line_from(2), Some(0));
    }

    #[test]
    fn last_line_blank_trailing_newline() {
        let t = VimText::from_str("hello\nworld\n");
        // Line 2 is the empty trailing line after \n.
        assert_eq!(t.line_count(), 3);
        // The trailing empty line IS blank.
        assert!(t.is_line_blank(2));
    }

    // === min_indent_in_range ===

    #[test]
    fn min_indent_basic() {
        let t = VimText::from_str("  hello\n    world\n  foo");
        assert_eq!(t.min_indent_in_range(0, 3), 2); // min of 2, 4, 2
    }

    #[test]
    fn min_indent_ignores_blank() {
        let t = VimText::from_str("  hello\n\n    world");
        assert_eq!(t.min_indent_in_range(0, 3), 2); // blank line ignored
    }

    #[test]
    fn min_indent_all_blank() {
        let t = VimText::from_str("\n\n\n");
        assert_eq!(t.min_indent_in_range(0, 4), u16::MAX);
    }

    #[test]
    fn min_indent_single_line() {
        let t = VimText::from_str("    hello");
        assert_eq!(t.min_indent_in_range(0, 1), 4);
    }

    #[test]
    fn min_indent_zero() {
        let t = VimText::from_str("hello\n  world");
        assert_eq!(t.min_indent_in_range(0, 2), 0);
    }

    #[test]
    fn min_indent_tabs() {
        let t = VimText::from_str("\thello\n\t\tworld");
        assert_eq!(t.min_indent_in_range(0, 2), 1); // tab counts as 1
    }

    #[test]
    fn min_indent_partial_range() {
        let t = VimText::from_str("hello\n  world\n    foo\nbar");
        assert_eq!(t.min_indent_in_range(1, 3), 2); // lines 1-2: "  world" (2), "    foo" (4)
    }

    #[test]
    fn min_indent_empty_range() {
        let t = VimText::from_str("hello\nworld");
        assert_eq!(t.min_indent_in_range(1, 1), u16::MAX); // empty range
        assert_eq!(t.min_indent_in_range(3, 1), u16::MAX); // inverted range
    }

    #[test]
    fn min_indent_empty_doc() {
        let t = VimText::from_str("");
        assert_eq!(t.min_indent_in_range(0, 1), u16::MAX); // single blank line
    }

    #[test]
    fn min_indent_whitespace_only_lines() {
        let t = VimText::from_str("  hello\n   \n    world");
        // Line 1 is "   " (blank), should be ignored
        assert_eq!(t.min_indent_in_range(0, 3), 2);
    }

    #[test]
    fn min_indent_mixed_tabs_and_spaces() {
        let t = VimText::from_str("\t hello\n  world");
        // Line 0: tab + space = 2 indent units
        // Line 1: 2 spaces = 2 indent units
        assert_eq!(t.min_indent_in_range(0, 2), 2);
    }

    #[test]
    fn min_indent_range_clamp_beyond_end() {
        let t = VimText::from_str("  hello\n    world");
        // end_line beyond line_count is clamped
        assert_eq!(t.min_indent_in_range(0, 100), 2);
    }

    #[test]
    fn min_indent_trailing_newline() {
        let t = VimText::from_str("  hello\n");
        // Line 0: "  hello" (indent 2), Line 1: "" (blank, ignored)
        assert_eq!(t.min_indent_in_range(0, 2), 2);
    }

    #[test]
    fn min_indent_large_file() {
        let mut text = String::new();
        for _ in 0..1000 {
            text.push_str("    content\n");
        }
        text.push_str("  min\n");
        for _ in 0..1000 {
            text.push_str("      content\n");
        }
        let t = VimText::from_str(&text);
        assert_eq!(t.min_indent_in_range(0, 2001), 2);
    }

    #[test]
    fn min_indent_short_circuit_at_zero() {
        // Should return 0 quickly when first line has no indent.
        let t = VimText::from_str("hello\n    world\n      deep");
        assert_eq!(t.min_indent_in_range(0, 3), 0);
    }

    // === min_indent summary aggregation tests ===

    #[test]
    fn min_indent_large_range_uses_summary() {
        // Build 500 lines where line 250 has indent 1, all others have indent 4.
        // This spans multiple chunks, so interior chunks use summary aggregation.
        let mut text = String::new();
        for i in 0..500 {
            let indent = if i == 250 { 1 } else { 4 };
            text.push_str(&" ".repeat(indent));
            text.push_str("code\n");
        }
        let vt = VimText::from_str(&text);
        // Min indent across all 500 lines should be 1 (line 250).
        assert_eq!(vt.min_indent_in_range(0, 500), 1);
        // Min indent in range [0, 250) should be 4.
        assert_eq!(vt.min_indent_in_range(0, 250), 4);
        // Min indent in range [250, 251) should be 1.
        assert_eq!(vt.min_indent_in_range(250, 251), 1);
        // Min indent in range [251, 500) should be 4.
        assert_eq!(vt.min_indent_in_range(251, 500), 4);
    }

    #[test]
    fn min_indent_summary_with_blank_only_chunks() {
        // Some chunks are entirely blank lines — they should not contribute
        // to min_indent (should remain u16::MAX for those chunks).
        let mut text = String::new();
        for _ in 0..200 {
            text.push_str("    code\n");
        }
        for _ in 0..200 {
            text.push('\n'); // 200 blank lines
        }
        for _ in 0..200 {
            text.push_str("  code\n");
        }
        let vt = VimText::from_str(&text);
        // Min across all should be 2 (last section).
        assert_eq!(vt.min_indent_in_range(0, 600), 2);
        // Range covering only blank lines should return u16::MAX.
        assert_eq!(vt.min_indent_in_range(200, 400), u16::MAX);
    }

    #[test]
    fn min_indent_summary_consistency_with_scan() {
        // Compare the summary-accelerated result against the known answer for
        // various sub-ranges of a large document.
        let mut text = String::new();
        let indents = [4, 2, 6, 0, 8, 1, 3, 5, 7, 2];
        for i in 0..1000 {
            let indent = indents[i % indents.len()];
            text.push_str(&" ".repeat(indent));
            text.push_str("x\n");
        }
        let vt = VimText::from_str(&text);

        // Full range: min indent should be 0 (from pattern index 3).
        assert_eq!(vt.min_indent_in_range(0, 1000), 0);

        // Skip the 0-indent lines: pattern repeats every 10 lines, 0-indent
        // at index 3 (lines 3, 13, 23, ...). Range [4, 10) has indents
        // 8, 1, 3, 5, 7, 2 -> min 1.
        assert_eq!(vt.min_indent_in_range(4, 10), 1);

        // Range [0, 3) has indents 4, 2, 6 -> min 2.
        assert_eq!(vt.min_indent_in_range(0, 3), 2);
    }

    // === matching_bracket ===

    #[test]
    fn matching_bracket_paren_forward() {
        let t = VimText::from_str("(hello)");
        assert_eq!(t.matching_bracket(0), Some(6));
    }

    #[test]
    fn matching_bracket_paren_backward() {
        let t = VimText::from_str("(hello)");
        assert_eq!(t.matching_bracket(6), Some(0));
    }

    #[test]
    fn matching_bracket_nested() {
        let t = VimText::from_str("(a(b)c)");
        assert_eq!(t.matching_bracket(0), Some(6)); // outer (
        assert_eq!(t.matching_bracket(2), Some(4)); // inner (
        assert_eq!(t.matching_bracket(6), Some(0)); // outer )
        assert_eq!(t.matching_bracket(4), Some(2)); // inner )
    }

    #[test]
    fn matching_bracket_square() {
        let t = VimText::from_str("[a[b]c]");
        assert_eq!(t.matching_bracket(0), Some(6));
        assert_eq!(t.matching_bracket(2), Some(4));
        assert_eq!(t.matching_bracket(6), Some(0));
        assert_eq!(t.matching_bracket(4), Some(2));
    }

    #[test]
    fn matching_bracket_curly() {
        let t = VimText::from_str("{a{b}c}");
        assert_eq!(t.matching_bracket(0), Some(6));
        assert_eq!(t.matching_bracket(2), Some(4));
        assert_eq!(t.matching_bracket(6), Some(0));
        assert_eq!(t.matching_bracket(4), Some(2));
    }

    #[test]
    fn matching_bracket_not_a_bracket() {
        let t = VimText::from_str("hello");
        assert_eq!(t.matching_bracket(0), None);
        assert_eq!(t.matching_bracket(2), None);
        assert_eq!(t.matching_bracket(4), None);
    }

    #[test]
    fn matching_bracket_unmatched_open() {
        let t = VimText::from_str("(hello");
        assert_eq!(t.matching_bracket(0), None);
    }

    #[test]
    fn matching_bracket_unmatched_close() {
        let t = VimText::from_str("hello)");
        assert_eq!(t.matching_bracket(5), None);
    }

    #[test]
    fn matching_bracket_mixed_types() {
        let t = VimText::from_str("([{hello}])");
        assert_eq!(t.matching_bracket(0), Some(10)); // ( matches )
        assert_eq!(t.matching_bracket(1), Some(9)); // [ matches ]
        assert_eq!(t.matching_bracket(2), Some(8)); // { matches }
        assert_eq!(t.matching_bracket(8), Some(2)); // } matches {
        assert_eq!(t.matching_bracket(9), Some(1)); // ] matches [
        assert_eq!(t.matching_bracket(10), Some(0)); // ) matches (
    }

    #[test]
    fn matching_bracket_types_dont_interfere() {
        // ( should not match ] or }
        let t = VimText::from_str("(]");
        assert_eq!(t.matching_bracket(0), None); // ( has no matching )
        let t2 = VimText::from_str("[)");
        assert_eq!(t2.matching_bracket(0), None); // [ has no matching ]
    }

    #[test]
    fn matching_bracket_out_of_bounds() {
        let t = VimText::from_str("(hello)");
        assert_eq!(t.matching_bracket(7), None);
        assert_eq!(t.matching_bracket(100), None);
    }

    #[test]
    fn matching_bracket_empty_doc() {
        let t = VimText::from_str("");
        assert_eq!(t.matching_bracket(0), None);
    }

    #[test]
    fn matching_bracket_adjacent() {
        let t = VimText::from_str("()");
        assert_eq!(t.matching_bracket(0), Some(1));
        assert_eq!(t.matching_bracket(1), Some(0));
    }

    #[test]
    fn matching_bracket_multiline() {
        let t = VimText::from_str("(\n  hello\n)");
        assert_eq!(t.matching_bracket(0), Some(10));
        assert_eq!(t.matching_bracket(10), Some(0));
    }

    #[test]
    fn matching_bracket_deeply_nested() {
        let t = VimText::from_str("((((x))))");
        assert_eq!(t.matching_bracket(0), Some(8));
        assert_eq!(t.matching_bracket(1), Some(7));
        assert_eq!(t.matching_bracket(2), Some(6));
        assert_eq!(t.matching_bracket(3), Some(5));
        assert_eq!(t.matching_bracket(5), Some(3));
        assert_eq!(t.matching_bracket(6), Some(2));
        assert_eq!(t.matching_bracket(7), Some(1));
        assert_eq!(t.matching_bracket(8), Some(0));
    }

    #[test]
    fn matching_bracket_multiple_groups() {
        let t = VimText::from_str("(a)(b)(c)");
        assert_eq!(t.matching_bracket(0), Some(2)); // first (
        assert_eq!(t.matching_bracket(3), Some(5)); // second (
        assert_eq!(t.matching_bracket(6), Some(8)); // third (
        assert_eq!(t.matching_bracket(2), Some(0)); // first )
        assert_eq!(t.matching_bracket(5), Some(3)); // second )
        assert_eq!(t.matching_bracket(8), Some(6)); // third )
    }

    #[test]
    fn matching_bracket_large_text() {
        // Build a string: open paren, 5000 'x' chars, close paren
        let mut text = String::with_capacity(5002);
        text.push('(');
        for _ in 0..5000 {
            text.push('x');
        }
        text.push(')');
        let t = VimText::from_str(&text);
        assert_eq!(t.matching_bracket(0), Some(5001));
        assert_eq!(t.matching_bracket(5001), Some(0));
    }

    // === bracket_depth_at ===

    #[test]
    fn bracket_depth_at_basic() {
        let t = VimText::from_str("((hello))");
        assert_eq!(t.bracket_depth_at(0), 0); // before first (
        assert_eq!(t.bracket_depth_at(1), 1); // after first (
        assert_eq!(t.bracket_depth_at(2), 2); // after second (
        assert_eq!(t.bracket_depth_at(7), 2); // still inside both
        assert_eq!(t.bracket_depth_at(8), 1); // after first )
        assert_eq!(t.bracket_depth_at(9), 0); // after second )
    }

    #[test]
    fn bracket_depth_no_brackets() {
        let t = VimText::from_str("hello");
        assert_eq!(t.bracket_depth_at(0), 0);
        assert_eq!(t.bracket_depth_at(3), 0);
        assert_eq!(t.bracket_depth_at(5), 0);
    }

    #[test]
    fn bracket_depth_only_parens() {
        // Depth only counts parentheses, not [] or {}
        let t = VimText::from_str("[{(x)}]");
        assert_eq!(t.bracket_depth_at(0), 0);
        assert_eq!(t.bracket_depth_at(1), 0);
        assert_eq!(t.bracket_depth_at(2), 0);
        assert_eq!(t.bracket_depth_at(3), 1); // after (
        assert_eq!(t.bracket_depth_at(4), 1); // after x
        assert_eq!(t.bracket_depth_at(5), 0); // after )
    }

    #[test]
    fn bracket_depth_empty_doc() {
        let t = VimText::from_str("");
        assert_eq!(t.bracket_depth_at(0), 0);
    }

    #[test]
    fn bracket_depth_unbalanced() {
        let t = VimText::from_str("(((");
        assert_eq!(t.bracket_depth_at(0), 0);
        assert_eq!(t.bracket_depth_at(1), 1);
        assert_eq!(t.bracket_depth_at(2), 2);
        assert_eq!(t.bracket_depth_at(3), 3);
    }

    #[test]
    fn bracket_depth_close_before_open() {
        let t = VimText::from_str(")(");
        assert_eq!(t.bracket_depth_at(0), 0);
        assert_eq!(t.bracket_depth_at(1), -1); // ) decrements depth below zero
        assert_eq!(t.bracket_depth_at(2), 0); // ( increments back to 0
    }

    #[test]
    fn bracket_depth_multiline() {
        let t = VimText::from_str("(\n  hello\n)");
        assert_eq!(t.bracket_depth_at(0), 0);
        assert_eq!(t.bracket_depth_at(1), 1); // after (
        assert_eq!(t.bracket_depth_at(5), 1); // in middle
        assert_eq!(t.bracket_depth_at(10), 1); // before )
        assert_eq!(t.bracket_depth_at(11), 0); // after )
    }

    #[test]
    fn bracket_depth_at_large_doc_uses_prefix() {
        // Build a large document with known bracket structure.
        // 500 lines of "((hello))\n" — each line is 10 bytes.
        let mut text = String::new();
        for _ in 0..500 {
            text.push_str("((hello))\n"); // 10 chars per line
        }
        let vt = VimText::from_str(&text);

        // At offset 0: depth 0
        assert_eq!(vt.bracket_depth_at(0), 0);
        // After first "(": depth 1
        assert_eq!(vt.bracket_depth_at(1), 1);
        // After "((": depth 2
        assert_eq!(vt.bracket_depth_at(2), 2);
        // After "((hello)": depth 1
        assert_eq!(vt.bracket_depth_at(8), 1);
        // After "((hello))": depth 0
        assert_eq!(vt.bracket_depth_at(9), 0);

        // Line 200 starts at offset 2000
        let off = 200 * 10;
        assert_eq!(vt.bracket_depth_at(off), 0);
        assert_eq!(vt.bracket_depth_at(off + 1), 1);
        assert_eq!(vt.bracket_depth_at(off + 2), 2);

        // Last line
        let last = 499 * 10;
        assert_eq!(vt.bracket_depth_at(last + 9), 0);
    }

    // === Chunk boundary tests (Bias::Right) ===

    #[test]
    fn byte_at_offset_at_chunk_boundary() {
        use crate::chunk::CHUNK_MAX_BYTES;

        // Create text that spans multiple chunks. Each chunk holds at most
        // CHUNK_MAX_BYTES bytes, so a string longer than that is guaranteed
        // to have at least one chunk boundary.
        let text: String = "x".repeat(CHUNK_MAX_BYTES) + &"y".repeat(CHUNK_MAX_BYTES);
        let vt = VimText::from_str(&text);

        // Byte at the first position of what should be the second chunk.
        assert_eq!(vt.byte_at(CHUNK_MAX_BYTES), Some(b'y'));

        // The bracket helpers should not panic at chunk boundaries either.
        // byte_at_offset is used by matching_bracket internally.
        assert_eq!(vt.matching_bracket(CHUNK_MAX_BYTES), None); // 'y' is not a bracket
        let _ = vt.bracket_depth_at(CHUNK_MAX_BYTES);
    }

    #[test]
    fn is_line_blank_at_chunk_boundary() {
        use crate::chunk::CHUNK_MAX_BYTES;

        // Create a document where a blank line straddles a chunk boundary.
        // Fill first chunk with content lines, then put a blank line right
        // after the chunk boundary.
        let prefix = "x".repeat(CHUNK_MAX_BYTES - 1) + "\n"; // fills first chunk
        let text = prefix.clone() + "\n" + "hello"; // blank line at chunk boundary
        let vt = VimText::from_str(&text);

        // The blank line should be detected without panicking.
        assert!(vt.is_line_blank(1)); // the empty line after the chunk boundary
    }

    #[test]
    fn min_indent_at_chunk_boundary() {
        use crate::chunk::CHUNK_MAX_BYTES;

        // Fill first chunk, then put indented content starting right after.
        let prefix = "x".repeat(CHUNK_MAX_BYTES - 1) + "\n";
        let text = prefix + "  hello\n" + "    world";
        let vt = VimText::from_str(&text);
        let line_count = vt.line_count();

        // min_indent_in_range should work across chunk boundaries.
        assert_eq!(vt.min_indent_in_range(1, line_count), 2);
    }

    #[test]
    fn matching_bracket_across_chunk_boundary() {
        use crate::chunk::CHUNK_MAX_BYTES;

        // Place an open paren near the end of the first chunk and the
        // matching close paren in the second chunk.
        let prefix = "x".repeat(CHUNK_MAX_BYTES - 2) + "(";
        let suffix = ")".to_string() + &"y".repeat(100);
        let text = prefix + &suffix;
        let vt = VimText::from_str(&text);

        let open_pos = CHUNK_MAX_BYTES - 2;
        let close_pos = CHUNK_MAX_BYTES - 1;
        assert_eq!(vt.matching_bracket(open_pos), Some(close_pos));
        assert_eq!(vt.matching_bracket(close_pos), Some(open_pos));
    }

    #[test]
    fn matching_bracket_large_doc_pruned() {
        // 500 lines of "((hello))\n" — each line is 10 bytes, total 5000 bytes.
        // This spans multiple chunks, exercising the chunk-level pruning path.
        let mut text = String::new();
        for _ in 0..500 {
            text.push_str("((hello))\n");
        }
        let vt = VimText::from_str(&text);

        // Forward: ( at 0 matches ) at 8
        assert_eq!(vt.matching_bracket(0), Some(8));
        // Forward: ( at 1 matches ) at 7
        assert_eq!(vt.matching_bracket(1), Some(7));
        // Backward: ) at 8 matches ( at 0
        assert_eq!(vt.matching_bracket(8), Some(0));
        // Backward: ) at 7 matches ( at 1
        assert_eq!(vt.matching_bracket(7), Some(1));

        // Cross-chunk: line 200 at offset 2000
        let off = 200 * 10;
        assert_eq!(vt.matching_bracket(off), Some(off + 8));
        assert_eq!(vt.matching_bracket(off + 8), Some(off));

        // Also test [] and {} across many chunks
        let mut bracket_text = String::new();
        for _ in 0..500 {
            bracket_text.push_str("[[hello]]\n");
        }
        let vt2 = VimText::from_str(&bracket_text);
        assert_eq!(vt2.matching_bracket(0), Some(8));
        assert_eq!(vt2.matching_bracket(8), Some(0));
        let off2 = 300 * 10;
        assert_eq!(vt2.matching_bracket(off2), Some(off2 + 8));
        assert_eq!(vt2.matching_bracket(off2 + 8), Some(off2));

        let mut brace_text = String::new();
        for _ in 0..500 {
            brace_text.push_str("{{hello}}\n");
        }
        let vt3 = VimText::from_str(&brace_text);
        assert_eq!(vt3.matching_bracket(0), Some(8));
        assert_eq!(vt3.matching_bracket(8), Some(0));
        let off3 = 400 * 10;
        assert_eq!(vt3.matching_bracket(off3), Some(off3 + 8));
        assert_eq!(vt3.matching_bracket(off3 + 8), Some(off3));
    }

    #[test]
    fn matching_bracket_pruning_skips_chunks() {
        // Create a document where the match is far away: open paren at offset 0,
        // then many chunks of non-bracket content, then close paren.
        // The pruning should skip all intermediate chunks.
        let inner = "x".repeat(10000);
        let text = format!("({})", inner);
        let vt = VimText::from_str(&text);

        assert_eq!(vt.matching_bracket(0), Some(10001));
        assert_eq!(vt.matching_bracket(10001), Some(0));
    }

    #[test]
    fn matching_bracket_pruning_nested_across_chunks() {
        // Deeply nested brackets spanning many chunks:
        // 100 open parens, 10000 x's, 100 close parens.
        let opens: String = "(".repeat(100);
        let inner = "x".repeat(10000);
        let closes: String = ")".repeat(100);
        let text = format!("{}{}{}", opens, inner, closes);
        let vt = VimText::from_str(&text);

        // Outermost: ( at 0 matches ) at len-1
        assert_eq!(vt.matching_bracket(0), Some(text.len() - 1));
        assert_eq!(vt.matching_bracket(text.len() - 1), Some(0));

        // Second level: ( at 1 matches ) at len-2
        assert_eq!(vt.matching_bracket(1), Some(text.len() - 2));
        assert_eq!(vt.matching_bracket(text.len() - 2), Some(1));

        // Innermost: ( at 99 matches ) at len-100
        assert_eq!(vt.matching_bracket(99), Some(text.len() - 100));
        assert_eq!(vt.matching_bracket(text.len() - 100), Some(99));
    }

    // === Bug regression: continuation line counted as indent 0 ===

    #[test]
    fn min_indent_continuation_line_not_counted() {
        // Create a line longer than CHUNK_MAX_BYTES to force mid-line chunk split
        let long_line = format!("    {}\n", "x".repeat(1200)); // 1205 bytes, indent 4
        let text = format!("{}  short\n", long_line); // second line indent 2
        let vt = VimText::from_str(&text);
        // The continuation in the second chunk should NOT be counted as indent 0
        assert_eq!(
            vt.min_indent_in_range(0, vt.line_count()),
            2,
            "continuation line should not produce indent 0"
        );
    }

    #[test]
    fn min_indent_last_segment_nonblank() {
        // Create text where a chunk's last segment (after last \n) is non-blank
        let mut text = String::new();
        text.push_str(&" ".repeat(8));
        text.push_str("deep\n"); // indent 8
                                 // Force a chunk split, then have non-blank content without trailing \n
        text.push_str(&"y".repeat(1100)); // fills chunk, no trailing \n
        text.push('\n');
        text.push_str("  end\n"); // indent 2
        let vt = VimText::from_str(&text);
        let result = vt.min_indent_in_range(0, vt.line_count());
        assert!(result <= 2, "should find indent 2 or lower, got {}", result);
    }
}
