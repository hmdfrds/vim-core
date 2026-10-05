//! Operator context and result types.
//!
//! Core types for operator execution without traits.
//!
//! # Architecture
//!
//! No dyn traits in the hot path.
//! We use enum dispatch + plain functions, not trait objects.
//!
//! ```text
//! Command (parsed)
//!        │
//!        ▼
//! OperatorContext ─► execute_operator(enum) ─► CommandResult
//! (range, register)       (match dispatch)      (effects, cursor)
//! ```

use crate::document::CustomOperatorProvider;
use crate::grammar::types::{Motion, Operator};
use crate::primitives::RegisterName;
use crate::primitives::{MotionType, Offset, Range};
use compact_str::CompactString;

/// Context provided to operators during execution.
///
/// Contains all information an operator needs to perform its task.
#[derive(Clone)]
pub struct OperatorContext<'text> {
    /// The full document text.
    pub text: &'text str,

    /// The range to operate on (computed from motion/textobject).
    pub range: Range,

    /// How the range should be treated (char/line/block wise).
    pub motion_type: MotionType,

    /// The register to use (defaults to UNNAMED).
    pub register: RegisterName,

    /// Effective count for the operator (always >= 1).
    pub count: u32,

    /// Current cursor position (for computing final position).
    pub cursor: Offset,

    /// Whether to force numbered register usage (for search/jump motions).
    /// When true, sub-line deletes go to register '1' instead of '-'.
    pub force_numbered_register: bool,

    /// Shiftwidth for indent operators.
    pub shiftwidth: usize,

    /// Tab stop width for indent operators.
    pub tabstop: usize,

    /// Whether to expand tabs to spaces for indent operators.
    pub expandtab: bool,

    /// Textwidth for format operators.
    pub textwidth: usize,

    /// Commentstring for commentary operator (e.g., `"// %s"`).
    pub commentstring: &'text str,

    /// Custom operator provider for `Operator::Custom(id)`.
    ///
    /// When set, `dispatch_operator` queries this provider before falling
    /// back to `CallOperatorFunc`. See [`CustomOperatorProvider`].
    pub custom_operators: Option<&'text dyn CustomOperatorProvider>,
    /// Viewport info for H/M/L motions within operator contexts.
    pub viewport: Option<crate::commands::motions::types::ViewportInfo>,

    /// How the operator's range was derived (motion, text object, or visual selection).
    ///
    /// Used by:
    /// - Format operator: text objects use different cursor placement than motions
    /// - Delete operator: visual-origin linewise delete on empty text stores "\n"
    pub origin: OperatorOrigin,

    /// The motion target position (before linewise expansion).
    ///
    /// For `gu k` from (5,3): motion target = (4,3).
    /// Combined with `cursor` (the pre-motion position), this allows
    /// operators to compute `min(cursor, motion_target)` which is
    /// Neovim's `oap->start`.
    ///
    /// Defaults to `cursor` when not set (e.g., line-repeat `yy`).
    pub motion_target: Offset,

    /// Sticky column (curswant) for nosol cursor placement after linewise delete.
    ///
    /// In Neovim, linewise delete calls `beginline(BL_WHITE)` then
    /// `coladvance(curwin->w_curswant)`. When this is `Some`, linewise
    /// delete uses this virtual column instead of deriving the column
    /// from `cursor`. Set from the engine's sticky_column when the
    /// operator originates from a visual selection.
    pub sticky_column: Option<crate::primitives::VirtualColumn>,
    /// VimText tree for O(log n) queries via B+ tree summaries.
    pub tree: Option<&'text vim_text::VimText>,

    /// Set when the operator range end was adjusted from exclusive to linewise.
    ///
    /// In Vim, when a charwise exclusive motion lands on column 0, the range
    /// end is "adjusted" backward (exclusive->inclusive) or promoted to linewise.
    /// The format operator (`gq`) checks this to decide cursor placement.
    pub end_adjusted: bool,

    /// Whether a motion force override (`dv`, `dV`, `d<C-v>`) was applied.
    /// When true, Vi blank-line delete promotion is suppressed (matching
    /// Neovim's `motion_force == NUL` check in `op_delete`).
    pub force_applied: bool,

    /// Whether `virtualedit` contains "block" or "all".
    /// When true, block visual operations pad short lines with spaces.
    pub virtualedit_block: bool,
}

impl std::fmt::Debug for OperatorContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperatorContext")
            .field("range", &self.range)
            .field("motion_type", &self.motion_type)
            .field("register", &self.register)
            .field("count", &self.count)
            .field("cursor", &self.cursor)
            .field("force_numbered_register", &self.force_numbered_register)
            .field("shiftwidth", &self.shiftwidth)
            .field("textwidth", &self.textwidth)
            .field("commentstring", &self.commentstring)
            .field("custom_operators", &self.custom_operators.is_some())
            .finish()
    }
}

impl<'text> OperatorContext<'text> {
    /// Create a new operator context.
    #[inline]
    #[must_use]
    pub fn new(
        text: &'text str,
        range: Range,
        motion_type: MotionType,
        register: Option<RegisterName>,
        count: u32,
        cursor: Offset,
    ) -> Self {
        Self {
            text,
            range,
            motion_type,
            register: register.unwrap_or(RegisterName::UNNAMED),
            count,
            cursor,
            force_numbered_register: false,
            shiftwidth: 4,          // overridden by executor via .with_shiftwidth()
            textwidth: 80,          // overridden by executor via .with_textwidth()
            commentstring: "// %s", // overridden by executor via .with_commentstring()
            custom_operators: None,
            viewport: None,
            origin: OperatorOrigin::Motion,
            motion_target: cursor,
            tabstop: 8,
            expandtab: true,
            sticky_column: None,
            tree: None,
            end_adjusted: false,
            force_applied: false,
            virtualedit_block: false,
        }
    }

    /// Set the motion target position (before linewise expansion).
    #[inline]
    #[must_use]
    pub const fn with_motion_target(mut self, target: Offset) -> Self {
        self.motion_target = target;
        self
    }

    /// Mark this context as originating from a visual selection.
    #[inline]
    #[must_use]
    pub const fn with_from_visual(mut self) -> Self {
        self.origin = OperatorOrigin::Visual;
        self
    }

    /// Set the sticky column (curswant) for nosol cursor placement.
    #[inline]
    #[must_use]
    pub const fn with_sticky_column(
        mut self,
        col: Option<crate::primitives::VirtualColumn>,
    ) -> Self {
        self.sticky_column = col;
        self
    }

    /// Create a context for block-visual operations.
    ///
    /// Block operators use selection rectangles rather than linear ranges,
    /// so `range` is set to an empty placeholder. This avoids callers
    /// needing to synthesize a dummy `Range::from_raw(0, 0)`.
    #[inline]
    #[must_use]
    pub fn for_block(text: &'text str, register: Option<RegisterName>, cursor: Offset) -> Self {
        Self {
            text,
            range: Range::from_raw(0, 0),
            motion_type: MotionType::BlockWise,
            register: register.unwrap_or(RegisterName::UNNAMED),
            count: 1,
            cursor,
            force_numbered_register: false,
            shiftwidth: 4,          // overridden by executor via .with_shiftwidth()
            textwidth: 80,          // overridden by executor via .with_textwidth()
            commentstring: "// %s", // overridden by executor via .with_commentstring()
            custom_operators: None,
            viewport: None,
            origin: OperatorOrigin::Motion,
            motion_target: cursor,
            tabstop: 8,
            expandtab: true,
            sticky_column: None,
            tree: None,
            end_adjusted: false,
            force_applied: false,
            virtualedit_block: false,
        }
    }

    /// Builder method: mark the range end as adjusted (exclusive-to-linewise).
    #[inline]
    #[must_use]
    pub const fn with_end_adjusted(mut self, adjusted: bool) -> Self {
        self.end_adjusted = adjusted;
        self
    }

    /// Builder method: record whether a motion force override was applied.
    #[inline]
    #[must_use]
    pub const fn with_force_applied(mut self, applied: bool) -> Self {
        self.force_applied = applied;
        self
    }

    /// Builder method: enable virtualedit=block padding.
    #[inline]
    #[must_use]
    pub const fn with_virtualedit_block(mut self, enabled: bool) -> Self {
        self.virtualedit_block = enabled;
        self
    }

    /// Builder method: mark this context as derived from a text object.
    #[inline]
    #[must_use]
    pub const fn with_textobject_flag(mut self) -> Self {
        self.origin = OperatorOrigin::TextObject;
        self
    }

    /// Builder method: force numbered register for search/jump motions.
    #[inline]
    #[must_use]
    pub const fn with_force_numbered(mut self) -> Self {
        self.force_numbered_register = true;
        self
    }

    /// Builder method: set shiftwidth for indent operators.
    #[inline]
    #[must_use]
    pub const fn with_shiftwidth(mut self, sw: usize) -> Self {
        self.shiftwidth = sw;
        self
    }

    /// Builder method: set tabstop for indent operators.
    #[inline]
    #[must_use]
    pub const fn with_tabstop(mut self, ts: usize) -> Self {
        self.tabstop = ts;
        self
    }

    /// Builder method: set expandtab for indent operators.
    #[inline]
    #[must_use]
    pub const fn with_expandtab(mut self, et: bool) -> Self {
        self.expandtab = et;
        self
    }

    /// Builder method: set textwidth for format operators.
    #[inline]
    #[must_use]
    pub const fn with_textwidth(mut self, tw: usize) -> Self {
        self.textwidth = tw;
        self
    }

    /// Builder method: set commentstring for commentary operator.
    #[inline]
    #[must_use]
    pub const fn with_commentstring(mut self, cs: &'text str) -> Self {
        self.commentstring = cs;
        self
    }

    /// Builder method: set custom operator provider.
    #[inline]
    #[must_use]
    pub fn with_custom_operators(mut self, provider: &'text dyn CustomOperatorProvider) -> Self {
        self.custom_operators = Some(provider);
        self
    }

    /// Set VimText tree for O(log n) summary queries.
    #[must_use]
    pub const fn with_tree(mut self, tree: &'text vim_text::VimText) -> Self {
        self.tree = Some(tree);
        self
    }

    /// Extract the text within the range.
    /// Adjusts to char boundaries for multibyte safety.
    #[inline]
    #[must_use]
    pub fn range_text(&self) -> &str {
        let clamped = self.range.clamp_end(Offset::new(self.text.len()));
        let (start, end) =
            adjust_to_char_boundaries(self.text, clamped.start().get(), clamped.end().get());
        if start > end {
            return "";
        }
        &self.text[start..end]
    }

    /// Check if the range is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.range.is_empty()
    }

    /// Check if operating on whole lines.
    #[inline]
    #[must_use]
    pub const fn is_linewise(&self) -> bool {
        self.motion_type.is_line_wise()
    }

    /// Check if range spans multiple lines.
    #[must_use]
    pub fn spans_multiple_lines(&self) -> bool {
        self.range_text().contains('\n')
    }

    /// Count lines in range.
    ///
    /// Treats a trailing `\n` as a line terminator (not a separator), matching
    /// Vim's linewise semantics where `"hello\n"` is 1 line, not 2.
    #[must_use]
    pub fn line_count(&self) -> usize {
        let text = self.range_text();
        if text.is_empty() {
            return 0;
        }
        let newlines = text.chars().filter(|&c| c == '\n').count();
        if text.ends_with('\n') {
            newlines // trailing \n is a terminator
        } else {
            newlines + 1 // no trailing \n means last line has no terminator
        }
    }
}

// ============================================================================
// Helper Functions (no traits needed!)
// ============================================================================

/// Adjust `start` and `end` to valid UTF-8 char boundaries within `text`.
///
/// `start` is nudged forward, `end` is nudged backward. Returns `(start, end)`.
#[inline]
pub(super) fn adjust_to_char_boundaries(text: &str, start: usize, end: usize) -> (usize, usize) {
    let mut s = start.min(text.len());
    while s < text.len() && !text.is_char_boundary(s) {
        s += 1;
    }
    let mut e = end;
    while e > s && !text.is_char_boundary(e) {
        e -= 1;
    }
    (s, e)
}

/// Compute the cursor position after a deletion.
///
/// - For linewise: first non-blank of the line that will be at the deletion start
///   (or previous line if deleting at end of document)
/// - For charwise: start of deleted range, clamped to line end
#[inline]
#[must_use]
pub fn cursor_after_delete(
    text: &str,
    range: Range,
    motion_type: MotionType,
    cursor: Offset,
) -> Offset {
    let raw_start = range.start().get();
    let raw_end = range.clamp_end(Offset::new(text.len())).end().get();

    // Adjust to valid char boundaries for multibyte safety
    let (start, end) = adjust_to_char_boundaries(text, raw_start, raw_end);

    let deleted_len = end.saturating_sub(start);
    let new_doc_len = text.len().saturating_sub(deleted_len);

    if motion_type.is_line_wise() {
        // After linewise delete, cursor goes to start of the line that remains
        // at the deletion point. If we're deleting at end of document (last line),
        // cursor goes to start of the previous line.

        if start >= new_doc_len {
            // Deleting at/past end - cursor goes to the last surviving line,
            // preserving the pre-deletion cursor column (Neovim nosol behavior).
            if new_doc_len == 0 {
                return Offset::ZERO;
            }
            let remaining = &text[..new_doc_len];
            let last_line_start = if remaining.ends_with('\n') {
                new_doc_len
            } else if let Some(last_nl) = remaining.rfind('\n') {
                last_nl + 1
            } else {
                0
            };
            // Preserve cursor column (matching Neovim's coladvance(old_col))
            let cursor_pos = cursor.get();
            let cursor_line_start = text[..cursor_pos.min(text.len())]
                .rfind('\n')
                .map_or(0, |p| p + 1);
            let old_col = cursor_pos.saturating_sub(cursor_line_start);
            let last_line_text = &remaining[last_line_start..];
            let last_line_len = last_line_text.find('\n').unwrap_or(last_line_text.len());
            if last_line_len == 0 {
                Offset::new(last_line_start)
            } else {
                let clamped_col = old_col.min(last_line_len.saturating_sub(1));
                Offset::new(last_line_start + clamped_col)
            }
        } else {
            // Normal case: cursor goes to start of the line at 'start',
            // with the pre-deletion cursor column clamped to the surviving
            // line's length.
            //
            // Neovim's nosol (default): clamp pre-deletion curswant to
            // the surviving line's length. This is a two-step process in Neovim:
            // 1. beginline(BL_WHITE | BL_FIX) -> first non-blank
            // 2. do_pending_operator overrides with coladvance(old_curswant)
            // Since nosol is default, step 2 always runs, making the final
            // position = min(old_cursor_col, last_col_of_surviving_line).

            // The surviving content at position `start` after deletion is the
            // original text from `end` onwards.
            let after_delete_offset = end.min(text.len());
            let surviving_content = &text[after_delete_offset..];
            let surviving_line_len = surviving_content
                .find('\n')
                .unwrap_or(surviving_content.len());

            if surviving_line_len == 0 {
                // Empty surviving line — col 0
                Offset::new(start)
            } else {
                // Compute the pre-deletion cursor column
                let cursor_pos = cursor.get();
                let cursor_line_start = text[..cursor_pos.min(text.len())]
                    .rfind('\n')
                    .map_or(0, |p| p + 1);
                let old_col = cursor_pos.saturating_sub(cursor_line_start);

                // Clamp old column to surviving line length
                let new_col = old_col.min(surviving_line_len.saturating_sub(1));
                Offset::new(start + new_col)
            }
        }
    } else {
        // After charwise delete, cursor goes to start of deleted range.
        // Clamp to new document length — offset == new_doc_len is valid
        // when the remaining text ends with '\n' (cursor on the empty last line).
        // For non-newline-terminated text, clamp to last char position.
        let post_delete_text_before = &text[..start];
        let post_delete_text_after = &text[end..];

        // Reconstruct what the new text looks like after deletion
        // If text after deletion point is empty and text before ends with '\n',
        // cursor at start is valid (empty line after newline)
        let max_pos = if post_delete_text_after.is_empty()
            && !post_delete_text_before.is_empty()
            && !post_delete_text_before.ends_with('\n')
        {
            // Text doesn't end with newline, cursor on last char.
            // Use prev_char_boundary for multi-byte safety (the last
            // remaining char may be >1 byte).
            crate::primitives::text_util::prev_char_boundary(
                post_delete_text_before,
                post_delete_text_before.len(),
            )
        } else {
            // Text ends with newline or there's content after deletion point
            new_doc_len
        };

        let mut cursor_pos = start.min(max_pos);

        // Vim normal-mode EOL clamp: if the character at the cursor position
        // after deletion would be '\n', back up to the last character on the
        // line (unless we're on an empty line where the line starts at cursor_pos).
        // After deleting [start, end), the char at position `start` in the new
        // text is `text[end]` in the original text.
        if cursor_pos > 0
            && text.as_bytes().get(end) == Some(&b'\n')
            && (start == 0 || text.as_bytes().get(start - 1) != Some(&b'\n'))
        {
            cursor_pos = crate::primitives::text_util::prev_char_boundary(&text[..start], start);
        }

        Offset::new(cursor_pos)
    }
}

/// Extract text from a range, handling bounds.
/// Adjusts offsets to valid char boundaries for multibyte safety.
#[inline]
#[must_use]
pub fn extract_range_text(text: &str, range: Range) -> CompactString {
    let clamped = range.clamp_end(Offset::new(text.len()));
    let (start, end) = adjust_to_char_boundaries(text, clamped.start().get(), clamped.end().get());
    if start > end {
        return CompactString::new("");
    }
    CompactString::new(&text[start..end])
}

/// Normalize extracted text for linewise register storage.
///
/// Handles the EOF edge case: when an operator range extends backward
/// to include the preceding newline (line separator), that leading `\n`
/// must be stripped — it's a connector between lines, not content.
///
/// For blank lines, the leading `\n` IS content and must be preserved.
/// Detection: if the byte before the range start is a non-newline char,
/// the leading `\n` is a separator. If it's `\n` (or range starts at 0),
/// it's blank-line content.
///
/// # Invariant
///
/// Output always ends with `\n` and is never empty (at minimum `"\n"`).
///
/// Returns `Cow::Borrowed` when no transformation is needed (content has no
/// leading separator and already ends with `\n`), avoiding allocation.
#[must_use]
pub fn normalize_linewise_register<'a>(
    extracted: &'a str,
    full_text: &str,
    range: Range,
) -> std::borrow::Cow<'a, str> {
    use std::borrow::Cow;

    let is_leading_separator = extracted.starts_with('\n')
        && range.start().get() > 0
        && (full_text.as_bytes().get(range.start().prev().get()) != Some(&b'\n')
            // Also treat as separator at EOF-extended ranges: when the range
            // reaches EOF and text doesn't end with '\n', the leading '\n'
            // was added by range extension (e.g., dip on last paragraph).
            || (range.end().get() >= full_text.len() && !full_text.ends_with('\n')));

    // SAFETY: '\n' is always 1 byte in UTF-8, so &text[1..] is valid.
    let content = if is_leading_separator {
        &extracted[1..]
    } else {
        extracted
    };

    if content.ends_with('\n') {
        if is_leading_separator {
            Cow::Owned(content.to_owned())
        } else {
            Cow::Borrowed(extracted)
        }
    } else {
        Cow::Owned(format!("{content}\n"))
    }
}

/// Check if range spans a full line (or multiple lines).
#[must_use]
pub fn is_full_line_delete(text: &str, range: Range) -> bool {
    if range.is_empty() || text.is_empty() {
        return false;
    }

    let start = range.start().get();
    let end = range.clamp_end(Offset::new(text.len())).end().get();

    // Check if start is at line beginning
    let at_line_start = start == 0 || text.as_bytes().get(start.saturating_sub(1)) == Some(&b'\n');

    // Check if end is at line end (or document end)
    let at_line_end =
        end >= text.len() || text.as_bytes().get(end.saturating_sub(1)) == Some(&b'\n');

    at_line_start && at_line_end
}

/// Origin of the operator's range — how the range was derived.
///
/// Replaces the impossible `(from_textobject: bool, from_visual: bool)` pair.
/// Only one origin can be active at a time; a motion-derived range is neither
/// a text object nor a visual selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OperatorOrigin {
    /// Range derived from a motion (e.g., `dw`, `y3j`, `c$`).
    Motion,
    /// Range derived from a text object (e.g., `diw`, `ci(`, `yap`).
    TextObject,
    /// Range derived from a visual selection (e.g., `Vd`, `viy`).
    Visual,
}

/// Motion inclusivity for operator range calculation.
///
/// Case transformation direction.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum CaseTransform {
    /// Convert to lowercase.
    Lower,
    /// Convert to uppercase.
    Upper,
    /// Toggle case.
    Toggle,
    /// ROT13 cipher.
    Rot13,
    /// ROT47 cipher.
    Rot47,
}

/// Indentation shift direction.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum ShiftDirection {
    /// Shift left (outdent).
    Left,
    /// Shift right (indent).
    Right,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandResult;

    #[test]
    fn test_operator_context_range_text() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );
        assert_eq!(ctx.range_text(), "hello");
    }

    #[test]
    fn test_operator_context_line_count() {
        let ctx = OperatorContext::new(
            "line1\nline2\nline3",
            Range::from_raw(0, 11),
            MotionType::LineWise,
            None,
            1,
            Offset::new(0),
        );
        assert_eq!(ctx.line_count(), 2); // "line1\nline2" has 2 lines
    }

    #[test]
    fn test_cursor_after_delete_charwise() {
        let text = "hello world";
        let range = Range::from_raw(0, 5);
        let cursor = cursor_after_delete(text, range, MotionType::CharWise, Offset::new(0));
        assert_eq!(cursor.get(), 0);
    }

    #[test]
    fn test_is_full_line_delete() {
        assert!(is_full_line_delete("hello\n", Range::from_raw(0, 6)));
        assert!(!is_full_line_delete("hello world", Range::from_raw(0, 5)));
    }

    #[test]
    fn test_operator_result_must_use() {
        // This tests that creating a result doesn't panic
        let result = CommandResult::empty(Offset::new(0));
        assert!(result.is_empty());
    }

    #[test]
    fn end_adjusted_default_false() {
        let ctx = OperatorContext::new(
            "hello",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );
        assert!(!ctx.end_adjusted);
    }

    #[test]
    fn end_adjusted_builder() {
        let ctx = OperatorContext::new(
            "hello",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        )
        .with_end_adjusted(true);
        assert!(ctx.end_adjusted);
    }
}

/// Context for visual operator selection dispatch.
///
/// Contains pre-resolved parameters — execution layer resolves these
/// from `ExecutionContext`, then passes them here. This keeps the dispatch
/// layer independent of execution types.
pub struct SelectionOperatorContext<'text> {
    /// Document text.
    pub text: &'text str,
    /// Pre-resolved operator range (from live selection or LastVisualInfo).
    pub range: Range,
    /// Motion type (CharWise or LineWise).
    pub motion_type: MotionType,
    /// Target register.
    pub register: Option<RegisterName>,
    /// Final cursor position (computed from selection resolution).
    pub cursor_pos: Offset,
    /// The operator being applied.
    pub operator: crate::grammar::types::Operator,
    /// Visual exit info — Some if we're in live visual mode and need exit effects.
    /// Contains (visual_type, selection) for computing LastVisualInfo + marks.
    pub visual_exit: Option<(
        crate::primitives::VisualType,
        crate::primitives::SelectionRange,
    )>,
    /// Comment string for commentary operator (from VimOptions).
    pub commentstring: &'text str,
    /// Custom operator provider for `Operator::Custom(id)`.
    pub custom_operators: Option<&'text dyn CustomOperatorProvider>,
    /// Viewport info for H/M/L motions within operator contexts.
    pub viewport: Option<crate::commands::motions::types::ViewportInfo>,
    /// Shiftwidth for indent operators.
    pub shiftwidth: usize,
    /// Tab stop width for virtual column computation in `LastVisualInfo`.
    pub tabstop: usize,
    /// Whether to use spaces instead of tabs for indentation.
    pub expandtab: bool,
    /// Engine options, for the format operators.
    pub options: &'text crate::primitives::VimOptions,
    /// Sticky column (curswant) for nosol cursor placement after linewise delete.
    pub sticky_column: Option<crate::primitives::VirtualColumn>,
}

/// Input context for operator+motion dispatch.
///
/// Bundles the full context needed for `dispatch_operator_with_motion`,
/// avoiding a long positional argument list.
pub struct OperatorMotionInput<'text> {
    /// The operator to apply (e.g., `Delete`, `Yank`, `Change`).
    pub operator: Operator,
    /// The motion that defines the range (e.g., `WordForward`, `Down`).
    pub motion: Motion,
    /// Repeat count (product of count1 and count2, always >= 1).
    pub count: u32,
    /// Target register, if explicitly specified.
    pub register: Option<RegisterName>,
    /// Full document text.
    pub text: &'text str,
    /// Current cursor byte offset.
    pub cursor: Offset,
    /// Active search pattern and direction, for search-based motions.
    pub search: Option<(&'text str, crate::primitives::Direction)>,
    /// Last `f`/`t`/`F`/`T` find, for `;` and `,` repeat.
    pub last_find: Option<crate::primitives::LastFind>,
    /// Shiftwidth for indent operators (from VimOptions).
    pub shiftwidth: usize,
    /// Textwidth for format operators (from VimOptions).
    pub textwidth: usize,
    /// Engine options (for motion dispatch).
    pub options: &'text crate::primitives::VimOptions,
    /// Motion force override (`dv$` = charwise, `dVj` = linewise, `d<C-v>j` = blockwise).
    pub force_type: Option<MotionType>,
    /// Custom operator provider for `Operator::Custom(id)`.
    pub custom_operators: Option<&'text dyn CustomOperatorProvider>,
    /// Viewport info for H/M/L motions within operator contexts.
    pub viewport: Option<crate::commands::motions::types::ViewportInfo>,
    /// Sticky column (curswant) for vertical motions (j/k/G/gg).
    /// Propagated from the engine state so operator-internal motions
    /// like the `gg` in `gUgg` honor `$`-set MAXCOL.
    pub sticky_column: Option<crate::primitives::VirtualColumn>,
    /// Fold provider for fold-aware range expansion.
    /// When present, operator ranges touching a closed fold are expanded
    /// to include the entire fold.
    pub fold_provider: Option<&'text dyn crate::document::FoldProvider>,
}
