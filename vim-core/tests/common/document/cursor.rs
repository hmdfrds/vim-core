//! Cursor and selection tracking for TestDocument.

/// Cursor tracking state.
#[derive(Debug, Clone, Default)]
pub struct CursorState {
    /// Current cursor byte offset.
    pub offset: usize,
    /// Selection anchor (if in visual mode).
    pub selection_anchor: Option<usize>,
    /// Selection head (if in visual mode).
    pub selection_head: Option<usize>,
}

impl CursorState {
    /// Create new cursor at offset.
    pub fn new(offset: usize) -> Self {
        Self {
            offset,
            selection_anchor: None,
            selection_head: None,
        }
    }

    /// Set cursor offset, clamping to max.
    pub fn set_offset(&mut self, offset: usize, max: usize) {
        self.offset = offset.min(max);
    }

    /// Set selection anchor and head.
    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.selection_anchor = Some(anchor);
        self.selection_head = Some(head);
    }

    /// Clear selection.
    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_head = None;
    }

    /// Convert offset to (line, col) using line starts.
    pub fn to_position(&self, line_starts: &[usize]) -> (usize, usize) {
        let line_idx = line_starts
            .iter()
            .rposition(|&start| start <= self.offset)
            .unwrap_or(0);
        let line_start = line_starts[line_idx];
        let col = self.offset - line_start;
        (line_idx, col)
    }
}
