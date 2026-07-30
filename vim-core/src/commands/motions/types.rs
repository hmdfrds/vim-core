//! Motion types for range inclusivity.
//!
//! Defines how the end position of a motion range is treated.
//! Distinct from `primitives::MotionType` which describes paste behavior.

use crate::commands::line_index::LineIndex;
use crate::primitives::{MotionInclusivity, Offset};

/// Result of computing a motion.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "motion result must be handled by the caller"]
#[non_exhaustive]
pub enum MotionResult {
    /// Motion succeeded, cursor at new offset (byte offset).
    Position(Offset),
    /// Motion succeeded with a full range (text-object-like motions).
    ///
    /// Used by `gn`/`gN` which select the entire search match.
    /// In operator-pending mode, the operator acts on this exact range
    /// (bypassing normal inclusivity adjustments).
    /// In normal mode, cursor moves to `end`.
    Range {
        /// Start of the range (inclusive).
        start: Offset,
        /// End of the range (exclusive, past the last byte of the match).
        end: Offset,
    },
    /// Motion succeeded with explicit type info (custom motions).
    ///
    /// Like `Position`, but carries inclusivity/linewise from the custom
    /// motion provider. `compute_motion_range` uses these to override
    /// the static `motion_inclusivity()` lookup.
    PositionWithType {
        /// Target cursor byte offset.
        offset: Offset,
        /// How the end position of this motion range is treated
        /// (exclusive, inclusive, or linewise).
        inclusivity: MotionInclusivity,
    },
    /// Motion failed with an error — beeps and stops macro replay.
    ///
    /// Used when a precondition is not met (no pattern, mark not set,
    /// target char not found, no match, etc.).
    Error,
    /// Motion produced no movement — silent, does NOT stop macros.
    ///
    /// Used when there's nothing to do but it's not an error
    /// (e.g., no previous find to repeat, changelist fallback).
    NoMotion,
    /// Motion needs viewport info from shell (H/M/L).
    NeedsViewport,
}

/// Viewport information for screen-relative motions (H/M/L/gm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportInfo {
    /// First visible line (0-indexed).
    pub first_line: usize,
    /// Number of visible lines.
    pub height: usize,
    /// Screen width in columns (for gm motion).
    pub width: usize,
}

/// Context for motion computation.
///
/// Contains all information a motion might need.
#[derive(Clone)]
pub struct MotionContext<'text> {
    /// Full document text.
    pub text: &'text str,
    /// Current cursor byte offset.
    pub cursor: Offset,
    /// Count (number of times to repeat motion, always >= 1).
    pub count: u32,
    /// Target character for f/F/t/T motions.
    pub target_char: Option<char>,
    /// Viewport info for H/M/L motions.
    pub viewport: Option<ViewportInfo>,
    /// Sticky column for vertical motions.
    pub sticky_column: Option<crate::primitives::VirtualColumn>,
    /// Resolved mark offset for '{mark} and `{mark} motions.
    pub mark_offset: Option<Offset>,
    /// Search pattern for n/N/*/# motions.
    pub search_pattern: Option<&'text str>,
    /// Search direction.
    pub search_direction: crate::primitives::Direction,
    /// Post-match cursor offset (/pat/e, /pat/+3, etc.).
    pub search_offset: crate::state::SearchOffset,
    /// Last find info for ; and , motions.
    pub last_find: Option<crate::primitives::LastFind>,
    /// Whether a count was explicitly provided (for G motion behavior).
    pub explicit_count: bool,
    /// Whether cursor is allowed to reach end-of-line (newline position).
    /// True in visual mode and operator-pending, where selections need to
    /// include the last character on a line.
    pub inclusive_end: bool,
    /// Capability providers from the shell (fold, display-line, search).
    pub providers: crate::document::Providers<'text>,
    /// Engine options (search flags, word boundaries, etc.).
    pub options: &'text crate::primitives::VimOptions,
    /// Sticky half-page scroll count (persists across Ctrl-D/Ctrl-U).
    pub scroll_half_count: Option<u32>,
    /// Local marks a-z as byte offsets, indexed by `(c - b'a')`.
    ///
    /// Used by `]'` / `['` mark navigation motions.
    /// `None` means the mark is not set.
    pub local_marks: [Option<Offset>; 26],
    /// Cached line-offset table for O(1) line_start / O(log N) line_of.
    ///
    /// Built lazily once per process() call. When present, paragraph motions
    /// and other line-intensive operations use this instead of scanning from
    /// byte 0 each time.
    pub line_index: Option<&'text LineIndex>,
    /// VimText tree for O(log n) queries via B+ tree summaries.
    pub tree: Option<&'text vim_text::VimText>,
    /// Position-dependent word character classification.
    ///
    /// When `Some`, word motions call this with the current byte offset to get
    /// a position-specific `WordCharSet` (e.g., for mixed-language files where
    /// `-` is a word char in CSS scope but not in JavaScript scope).
    /// When `None`, the global `WordCharSet` from options is used.
    pub word_char_fn: Option<&'text dyn WordCharProvider>,
}

/// Provider for position-dependent word character classification.
///
/// Hosts with tree-sitter scope information implement this trait to return
/// different `WordCharSet` values depending on the byte offset in the document.
/// For example, `-` might be a word char in CSS scope but punctuation in
/// JavaScript scope.
pub trait WordCharProvider {
    /// Return the `WordCharSet` that should be used at the given byte offset.
    fn word_char_set_at(&self, offset: usize) -> &crate::primitives::WordCharSet;
}

impl std::fmt::Debug for MotionContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MotionContext")
            .field("cursor", &self.cursor)
            .field("count", &self.count)
            .field("word_char_fn", &self.word_char_fn.is_some())
            .finish_non_exhaustive()
    }
}

impl<'text> MotionContext<'text> {
    /// Create a new motion context.
    #[must_use]
    pub fn new(
        text: &'text str,
        cursor: Offset,
        count: u32,
        options: &'text crate::primitives::VimOptions,
    ) -> Self {
        Self {
            text,
            cursor,
            count,
            target_char: None,
            viewport: None,
            sticky_column: None,
            mark_offset: None,
            search_pattern: None,
            search_direction: crate::primitives::Direction::Forward,
            search_offset: crate::state::SearchOffset::NONE,
            last_find: None,
            explicit_count: false,
            inclusive_end: false,
            providers: crate::document::Providers::new(),
            options,
            scroll_half_count: None,
            local_marks: [None; 26],
            line_index: None,
            tree: None,
            word_char_fn: None,
        }
    }

    /// Set target character for f/F/t/T.
    #[must_use]
    pub const fn with_target_char(mut self, c: char) -> Self {
        self.target_char = Some(c);
        self
    }

    /// Set viewport info.
    #[must_use]
    pub const fn with_viewport(mut self, viewport: ViewportInfo) -> Self {
        self.viewport = Some(viewport);
        self
    }

    /// Set sticky column.
    #[must_use]
    pub const fn with_sticky_column(mut self, col: crate::primitives::VirtualColumn) -> Self {
        self.sticky_column = Some(col);
        self
    }

    /// Set resolved mark offset for mark motions.
    #[must_use]
    pub const fn with_mark_offset(mut self, offset: Offset) -> Self {
        self.mark_offset = Some(offset);
        self
    }

    /// Set search pattern for n/N/*/# motions.
    #[must_use]
    pub const fn with_search(
        mut self,
        pattern: &'text str,
        direction: crate::primitives::Direction,
    ) -> Self {
        self.search_pattern = Some(pattern);
        self.search_direction = direction;
        self
    }

    /// Set search offset for post-match cursor adjustment.
    #[must_use]
    pub const fn with_search_offset(mut self, offset: crate::state::SearchOffset) -> Self {
        self.search_offset = offset;
        self
    }

    /// Set last find info for ; and , motions.
    #[must_use]
    pub const fn with_last_find(mut self, last_find: crate::primitives::LastFind) -> Self {
        self.last_find = Some(last_find);
        self
    }

    /// Set explicit count flag (for G motion behavior).
    #[must_use]
    pub const fn with_explicit_count(mut self, explicit: bool) -> Self {
        self.explicit_count = explicit;
        self
    }

    /// Set inclusive end flag (for visual/operator mode).
    #[must_use]
    pub const fn with_inclusive_end(mut self, inclusive: bool) -> Self {
        self.inclusive_end = inclusive;
        self
    }

    /// Set capability providers (fold, display-line, search).
    #[must_use]
    pub const fn with_providers(mut self, providers: crate::document::Providers<'text>) -> Self {
        self.providers = providers;
        self
    }

    /// Set local marks a-z for `]'`/`['` mark navigation motions.
    #[must_use]
    pub const fn with_local_marks(mut self, marks: [Option<Offset>; 26]) -> Self {
        self.local_marks = marks;
        self
    }

    /// Set cached line index for O(1) line lookups.
    #[must_use]
    pub const fn with_line_index(mut self, idx: &'text LineIndex) -> Self {
        self.line_index = Some(idx);
        self
    }

    /// Set VimText tree for O(log n) summary queries.
    #[must_use]
    pub const fn with_tree(mut self, tree: &'text vim_text::VimText) -> Self {
        self.tree = Some(tree);
        self
    }

    /// Set position-dependent word character classification provider.
    #[must_use]
    pub const fn with_word_char_fn(mut self, provider: &'text dyn WordCharProvider) -> Self {
        self.word_char_fn = Some(provider);
        self
    }

    /// Get the `WordCharSet` for a given byte offset.
    ///
    /// If a position-dependent provider is set, calls it with the offset.
    /// Otherwise falls back to the global `WordCharSet` from options.
    #[inline]
    #[must_use]
    pub fn word_char_set_at(&self, offset: usize) -> &crate::primitives::WordCharSet {
        if let Some(provider) = self.word_char_fn {
            provider.word_char_set_at(offset)
        } else {
            self.options.word_char_set()
        }
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

/// Miscellaneous motion types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumIter)]
#[non_exhaustive]
pub enum MiscMotion {
    /// gm - middle of screen line
    MiddleOfScreenLine,
    /// gM - middle of text line
    MiddleOfTextLine,
    /// go - go to byte offset
    GotoByte,
}

/// Search motion types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumIter)]
#[non_exhaustive]
pub enum SearchMotion {
    /// / - search forward
    SearchForward,
    /// ? - search backward
    SearchBackward,
    /// n - next match
    NextMatch,
    /// N - previous match
    PrevMatch,
    /// * - word under cursor forward
    WordUnderCursor,
    /// # - word under cursor backward
    WordUnderCursorBack,
    /// g* - partial word forward
    PartialWord,
    /// g# - partial word backward
    PartialWordBack,
}

/// Context for full motion dispatch (execution-layer-independent).
///
/// The execution layer resolves these fields from `ExecutionContext` and passes
/// them here — the dispatch layer never imports execution types.
pub struct MotionEffectsContext<'text> {
    /// The motion to dispatch.
    pub motion: crate::grammar::types::Motion,
    /// Pre-built motion context (text, cursor, count, etc.).
    pub motion_ctx: MotionContext<'text>,
    /// Current selection (if in visual mode).
    pub selection: Option<crate::primitives::SelectionRange>,
    /// Current editor mode — dispatch derives `inclusive_end` and
    /// `search_direction` from this so the executor doesn't have to.
    pub mode: crate::primitives::Mode,
    /// Current cursor offset (for jump list).
    pub cursor_offset: Offset,
    /// Cached search count from previous n/N. When present and the pattern
    /// hash matches, the dispatch layer advances `current` arithmetically
    /// instead of re-scanning the entire document.
    pub search_count_cache: Option<crate::state::SearchCountCache>,
}
