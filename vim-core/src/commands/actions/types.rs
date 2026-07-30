//! Action context and result types.
//!
//! Core types for action execution without traits.
//!
//! # Architecture
//!
//! No `dyn` traits in the hot path: enum dispatch plus plain functions,
//! not trait objects.
//!
//! ```text
//! Command (parsed)
//!        │
//!        ▼
//! ActionContext ─► dispatch_action(enum) ─► CommandResult
//! (cursor, state)       (match dispatch)     (effects, cursor)
//! ```

#[cfg(test)]
use crate::commands::CommandResult;
#[cfg(test)]
use crate::effects::Effects;
use std::num::NonZeroU32;

use crate::primitives::{MarkName, Offset, RegisterContent, RegisterName};

/// Context provided to actions during execution.
///
/// Contains all information an action needs to perform its task.
#[derive(Debug, Clone)]
pub struct ActionContext<'text> {
    /// The full document text.
    pub text: &'text str,

    /// Current cursor position.
    pub cursor: Offset,

    /// Start of current line (for put operations).
    pub line_start: Offset,

    /// End of current line (for put operations).
    pub line_end: Offset,

    /// Start of next line (if any).
    pub next_line_start: Option<Offset>,

    /// Register content (for put operations).
    pub register_content: Option<&'text RegisterContent>,

    /// Register name that was specified (for error messages).
    pub register_name: Option<RegisterName>,

    /// Effective count for the action (always >= 1).
    pub count: u32,

    /// Current selection range (for visual mode operations like visual put, block insert).
    pub selection: Option<crate::primitives::SelectionRange>,

    /// Visual mode type (Char/Line/Block) — dispatch uses this to decide whether
    /// to expand selections to full lines for line-visual operations.
    pub visual_type: Option<crate::primitives::VisualType>,

    /// Shift width for indent operations.
    pub shift_width: usize,

    /// Tab stop width (for ]p/[p indent adjustment).
    pub tabstop: usize,

    /// Whether to expand tabs to spaces (for ]p/[p indent adjustment).
    pub expandtab: bool,

    /// When true, block visual `$` was active — block append should insert at
    /// end of each line rather than at a fixed column. Corresponds to Neovim's
    /// `curswant == MAXCOL` check in block insert replication.
    pub dollar_mode: bool,

    /// When true, visual paste preserves the unnamed register (does not
    /// overwrite it with the deleted selection text). Numbered registers
    /// are still written. Used by `<Plug>(paste-preserve)`.
    pub preserve_register: bool,
    /// VimText tree for O(log n) queries via B+ tree summaries.
    pub tree: Option<&'text vim_text::VimText>,
}

impl<'text> ActionContext<'text> {
    /// Create a new action context.
    #[inline]
    #[must_use]
    pub const fn new(
        text: &'text str,
        cursor: Offset,
        line_start: Offset,
        line_end: Offset,
        next_line_start: Option<Offset>,
        count: u32,
    ) -> Self {
        Self {
            text,
            cursor,
            line_start,
            line_end,
            next_line_start,
            register_content: None,
            register_name: None,
            count,
            selection: None,
            visual_type: None,
            shift_width: 4, // overridden by executor via .with_shift_width()
            tabstop: 4,
            expandtab: true,
            dollar_mode: false,
            preserve_register: false,
            tree: None,
        }
    }

    /// Create an action context from text and cursor, computing line boundaries.
    ///
    /// This encapsulates line calculation logic that would otherwise need to be
    /// done inline in the executor.
    #[inline]
    pub fn from_text_and_cursor(text: &'text str, cursor: Offset, count: NonZeroU32) -> Self {
        use crate::commands::helpers::{line_end, line_of, line_start};

        let line = line_of(text, cursor.get());
        let line_start_offset = Offset::new(line_start(text, line).unwrap_or(0));
        let line_end_offset = Offset::new(line_end(text, line).unwrap_or(text.len()));
        let next_line_start_offset = line_start(text, line + 1).map(Offset::new);

        Self {
            text,
            cursor,
            line_start: line_start_offset,
            line_end: line_end_offset,
            next_line_start: next_line_start_offset,
            register_content: None,
            register_name: None,
            count: count.get(),
            selection: None,
            visual_type: None,
            shift_width: 4, // overridden by executor via .with_shift_width()
            tabstop: 4,
            expandtab: true,
            dollar_mode: false,
            preserve_register: false,
            tree: None,
        }
    }

    /// Set register content (for put operations).
    #[inline]
    #[must_use]
    pub const fn with_register(mut self, content: &'text RegisterContent) -> Self {
        self.register_content = Some(content);
        self
    }

    /// Set register name (for error messages).
    #[inline]
    #[must_use]
    pub const fn with_register_name(mut self, name: RegisterName) -> Self {
        self.register_name = Some(name);
        self
    }

    /// Set selection for visual mode operations.
    #[inline]
    #[must_use]
    pub const fn with_selection(mut self, selection: crate::primitives::SelectionRange) -> Self {
        self.selection = Some(selection);
        self
    }

    /// Set visual mode type (so dispatch can decide line expansion policy).
    #[inline]
    #[must_use]
    pub const fn with_visual_type(mut self, vt: crate::primitives::VisualType) -> Self {
        self.visual_type = Some(vt);
        self
    }

    /// Set shift width for indent operations.
    #[inline]
    #[must_use]
    pub const fn with_shift_width(mut self, width: usize) -> Self {
        self.shift_width = width;
        self
    }

    /// Set tabstop and expandtab (for ]p/[p indent adjustment).
    #[inline]
    #[must_use]
    pub const fn with_tab_options(mut self, tabstop: usize, expandtab: bool) -> Self {
        self.tabstop = tabstop;
        self.expandtab = expandtab;
        self
    }

    /// Mark this action as having a `$`-style block visual selection (MAXCOL).
    ///
    /// When set, block append inserts at the end of each line rather than at a
    /// fixed grapheme column.
    #[inline]
    #[must_use]
    pub const fn with_dollar_mode(mut self) -> Self {
        self.dollar_mode = true;
        self
    }

    /// Mark this action as a register-preserving paste.
    ///
    /// When set, visual paste does NOT overwrite the unnamed register with
    /// the deleted selection text. Numbered registers are still written.
    #[inline]
    #[must_use]
    pub const fn with_preserve_register(mut self) -> Self {
        self.preserve_register = true;
        self
    }

    /// Set VimText tree for O(log n) summary queries.
    #[must_use]
    pub const fn with_tree(mut self, tree: &'text vim_text::VimText) -> Self {
        self.tree = Some(tree);
        self
    }

    /// Count as `usize` for indexing and iteration.
    #[inline]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "u32 as usize is lossless (compile-time assert in lib.rs)"
    )]
    #[must_use]
    pub const fn count_usize(&self) -> usize {
        self.count as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;

    #[test]
    fn test_action_context_creation() {
        let ctx = ActionContext::new(
            "hello world",
            Offset::new(5),
            Offset::new(0),
            Offset::new(10),
            Some(Offset::new(11)),
            1,
        );
        assert_eq!(ctx.cursor.get(), 5);
        assert_eq!(ctx.count, 1);
    }

    #[test]
    fn test_action_result_empty() {
        let result = CommandResult::empty(Offset::new(5));
        assert!(result.is_empty());
        assert_eq!(result.cursor.unwrap().get(), 5);
    }

    #[test]
    fn test_action_result_with_effects() {
        let effects = Effects::new().set_cursor(Offset::new(10));
        let result = CommandResult::new(effects, Offset::new(10));
        assert!(!result.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Mark Context and Result Types
// ═══════════════════════════════════════════════════════════════════════════

/// Context for mark commands.
///
/// Follows the same pattern as `ActionContext`, `OperatorContext`, etc.
#[derive(Debug, Clone)]
pub struct MarkContext {
    /// The mark name (validated).
    pub mark: MarkName,
    /// Current cursor offset.
    pub cursor: Offset,
    /// Relative topline offset: `mark_line - viewport_first_line` at set time.
    ///
    /// Populated by the executor from viewport state and cursor position.
    /// `None` for auto-marks or when viewport info is unavailable.
    pub topline_offset: Option<i32>,
}

impl MarkContext {
    /// Create a new mark context.
    #[inline]
    #[must_use]
    pub const fn new(mark: MarkName, cursor: Offset) -> Self {
        Self {
            mark,
            cursor,
            topline_offset: None,
        }
    }

    /// Create a mark context with relative topline offset.
    #[inline]
    #[must_use]
    pub const fn with_topline_offset(
        mark: MarkName,
        cursor: Offset,
        topline_offset: Option<i32>,
    ) -> Self {
        Self {
            mark,
            cursor,
            topline_offset,
        }
    }
}

/// Context for `r{char}` command — uniform struct like `ActionContext`,
/// `MotionContext`, `OperatorContext`.
///
/// The executor populates all fields; the dispatch function and command
/// implementations read what they need.
#[derive(Debug)]
pub struct ReplaceCharContext<'text> {
    /// Document text.
    pub text: &'text str,
    /// Replacement character.
    pub ch: char,
    /// Cursor byte offset.
    pub cursor: Offset,
    /// Count (for normal mode: how many chars to replace).
    pub count: usize,
    /// Visual selection, if in visual mode.
    pub selection: Option<&'text crate::primitives::SelectionRange>,
    /// Visual mode type, if invoked from visual mode.
    ///
    /// `None` = normal mode, `Some(VisualType::Block)` = block visual,
    /// `Some(VisualType::Line)` = line visual, `Some(VisualType::Char)` = char visual.
    pub visual_type: Option<crate::primitives::VisualType>,
}

/// Computed geometry of a block selection.
pub struct BlockGeometry {
    /// Top line of the block.
    pub top_line: usize,
    /// Bottom line of the block.
    pub bot_line: usize,
    /// Left grapheme column.
    pub left_gcol: usize,
    /// Right grapheme column.
    pub right_gcol: usize,
    /// Left virtual (screen) column — tab-aware and wide-char-aware.
    pub left_vcol: usize,
    /// Right virtual (screen) column — tab-aware and wide-char-aware.
    pub right_vcol: usize,
    /// Anchor byte offset.
    pub anchor: Offset,
    /// Head byte offset.
    pub head: Offset,
}

#[cfg(test)]
mod mark_tests {
    use super::*;

    #[test]
    fn test_mark_context_creation() {
        let ctx = MarkContext::new(MarkName::new('a').unwrap(), Offset::new(100));
        assert_eq!(ctx.mark.char(), 'a');
        assert_eq!(ctx.cursor.get(), 100);
    }

    #[test]
    fn test_mark_result_empty() {
        let result = CommandResult::none();
        assert!(result.is_empty());
    }
}
