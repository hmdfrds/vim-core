//! Visual mode context and result types.
//!
//! Core types for visual mode command execution without traits.

use crate::primitives::VisualType;
use crate::primitives::{Offset, SelectionRange};

/// Context provided to visual mode commands during execution.
#[derive(Clone)]
pub struct VisualContext<'text> {
    /// The document text.
    pub text: &'text str,

    /// Current cursor position.
    pub cursor: Offset,

    /// Current selection range (if any).
    pub selection: Option<SelectionRange>,

    /// Current visual mode type (Char/Line/Block) — used for exit save.
    pub current_visual_type: Option<VisualType>,

    /// Last visual info for reselect.
    pub last_visual_type: Option<VisualType>,

    /// Last visual marks (start, end).
    pub last_visual_marks: Option<(Offset, Offset)>,

    /// Number of lines in last visual selection (for linewise `gv` reselection).
    ///
    /// Linewise `gv` uses `<` mark + line count instead of relying on `>` mark,
    /// because `>` as a byte offset can't reliably track a line through text edits
    /// that change line lengths (e.g., indent). Neovim stores `>` as (line, INT_MAX)
    /// for linewise, which we approximate with this line count approach.
    pub last_visual_lines: Option<usize>,

    /// Whether cursor was at the start (low end) of the last visual selection.
    ///
    /// Used by `gv` to restore the cursor to the correct end of the selection.
    pub last_visual_cursor_at_start: Option<bool>,

    /// Tab stop width for virtual column computation in `LastVisualInfo`.
    ///
    /// Defaults to 4 in the `new()` constructor. The production caller
    /// (`executor.rs`) overrides this from `VimOptions::tabstop()`.
    pub tabstop: usize,

    /// Fold provider for fold-aware visual entry.
    /// When present and cursor is inside a closed fold, visual entry
    /// expands the selection to include the entire fold.
    pub fold_provider: Option<&'text dyn crate::document::FoldProvider>,

    /// Full `LastVisualInfo` for count-based visual re-entry (`3v`).
    pub last_visual_info: Option<crate::primitives::LastVisualInfo>,
}

impl std::fmt::Debug for VisualContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualContext")
            .field("cursor", &self.cursor)
            .field("selection", &self.selection)
            .field("current_visual_type", &self.current_visual_type)
            .field("fold_provider", &self.fold_provider.is_some())
            .finish_non_exhaustive()
    }
}

impl<'text> VisualContext<'text> {
    /// Create a new visual context.
    #[inline]
    #[must_use]
    pub const fn new(text: &'text str, cursor: Offset, selection: Option<SelectionRange>) -> Self {
        Self {
            text,
            cursor,
            selection,
            current_visual_type: None,
            last_visual_type: None,
            last_visual_marks: None,
            last_visual_lines: None,
            last_visual_cursor_at_start: None,
            tabstop: 4,
            fold_provider: None,
            last_visual_info: None,
        }
    }

    /// Create a visual context from components, computing last visual from marks.
    ///
    /// This encapsulates the logic for building `VisualContext` with `last_visual`
    /// info from marks and visual state.
    #[inline]
    #[allow(
        clippy::too_many_arguments,
        reason = "visual context needs all geometry pieces"
    )]
    #[must_use]
    pub const fn from_cursor_and_selection(
        text: &'text str,
        cursor: Offset,
        selection: Option<SelectionRange>,
        current_visual_type: Option<VisualType>,
        last_visual_type: Option<VisualType>,
        start_mark: Option<Offset>,
        end_mark: Option<Offset>,
        last_visual_lines: Option<usize>,
        last_visual_cursor_at_start: Option<bool>,
    ) -> Self {
        let mut ctx = Self::new(text, cursor, selection);
        ctx.current_visual_type = current_visual_type;

        if let (Some(vtype), Some(start), Some(end)) = (last_visual_type, start_mark, end_mark) {
            ctx.last_visual_type = Some(vtype);
            ctx.last_visual_marks = Some((start, end));
        }
        ctx.last_visual_lines = last_visual_lines;
        ctx.last_visual_cursor_at_start = last_visual_cursor_at_start;

        ctx
    }

    /// Set last visual info for reselect.
    #[inline]
    #[must_use]
    pub const fn with_last_visual(
        mut self,
        visual_type: VisualType,
        start: Offset,
        end: Offset,
    ) -> Self {
        self.last_visual_type = Some(visual_type);
        self.last_visual_marks = Some((start, end));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandResult;

    #[test]
    fn test_visual_context_creation() {
        let ctx = VisualContext::new("hello", Offset::new(5), None);
        assert_eq!(ctx.cursor.get(), 5);
        assert_eq!(ctx.text, "hello");
    }

    #[test]
    fn test_visual_result_empty() {
        let result = CommandResult::none();
        assert!(result.is_empty());
    }
}
/// Result of visual text object selection computation.
///
/// Contains fully resolved anchor/cursor positions (already snapped to
/// line starts for linewise text objects) and whether a mode upgrade is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualTextObjectResult {
    /// Resolved anchor offset.
    pub anchor: Offset,
    /// Resolved cursor offset.
    pub cursor: Offset,
    /// Whether the text object is linewise (triggers mode upgrade to Visual Line).
    pub linewise: bool,
    /// Whether the selection already covers this text object and needs extension
    /// to the next one. The caller (execution) is responsible for re-dispatching.
    pub needs_extend: bool,
}
