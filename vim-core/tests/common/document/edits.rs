#![allow(dead_code)]
//! Text editing operations for TestDocument.

use std::ops::Range;

/// Compute line start offsets from text.
pub fn compute_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, c) in text.char_indices() {
        if c == '\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Text buffer with edit operations.
#[derive(Debug, Clone)]
pub struct TextBuffer {
    /// The document text.
    pub text: String,
    /// Cached line start offsets.
    pub line_starts: Vec<usize>,
}

impl TextBuffer {
    /// Create new buffer from text.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let line_starts = compute_line_starts(&text);
        Self { text, line_starts }
    }

    /// Create empty buffer.
    pub fn empty() -> Self {
        Self::new("")
    }

    /// Get text length.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Snap a byte offset to the nearest valid char boundary (rounding down).
    fn snap_to_char_boundary(&self, offset: usize) -> usize {
        let clamped = offset.min(self.text.len());
        // Walk backward to find a valid char boundary
        let mut idx = clamped;
        while idx > 0 && !self.text.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    /// Insert text at offset.
    pub fn insert(&mut self, offset: usize, text: &str) {
        let clamped = self.snap_to_char_boundary(offset);
        self.text.insert_str(clamped, text);
        self.recompute_lines();
    }

    /// Delete range.
    pub fn delete(&mut self, range: Range<usize>) {
        let start = self.snap_to_char_boundary(range.start);
        let end = self.snap_to_char_boundary(range.end);
        if start < end && end <= self.text.len() {
            self.text.replace_range(start..end, "");
            self.recompute_lines();
        }
    }

    /// Replace range with text.
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let start = self.snap_to_char_boundary(range.start);
        let end = self.snap_to_char_boundary(range.end);
        if start <= end && end <= self.text.len() {
            self.text.replace_range(start..end, text);
            self.recompute_lines();
        }
    }

    /// Recompute line starts after edit.
    fn recompute_lines(&mut self) {
        self.line_starts = compute_line_starts(&self.text);
    }
}
