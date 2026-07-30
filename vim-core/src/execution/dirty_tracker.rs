//! Document mutation tracking for incremental viewport updates.
//!
//! `full_redraw` is set ONLY when line count changes (newline
//! inserted/deleted). Single-line edits produce specific dirty
//! line numbers for targeted viewport updates.
//!
//! ## Fine-grained dirty flags
//!
//! [`DirtyFlags`] provides per-aspect bitflags alongside the existing
//! line-level tracking. Hosts query `dirty_flags()` after each
//! keystroke to determine *what category* of state changed (lines,
//! marks, search highlights, etc.) and only refresh the affected UI.

use bitflags::bitflags;
use smallvec::SmallVec;

bitflags! {
    /// Per-aspect dirty flags for fine-grained host refresh.
    ///
    /// Set during effect processing and drained alongside line-level
    /// dirty info. Each flag indicates that a particular category of
    /// visible state may have changed since the last drain.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DirtyFlags: u16 {
        /// Text lines were modified (insert, delete, replace).
        const LINES             = 1 << 0;
        /// A mark was set or cleared.
        const MARKS             = 1 << 1;
        /// Search highlights changed (new search, highlight toggle).
        const SEARCH_HIGHLIGHTS = 1 << 2;
        /// Fold state changed.
        const FOLDS             = 1 << 3;
        /// Cursor position may have become invalid (e.g. text shrunk).
        const CURSOR_VALID      = 1 << 4;
        /// Viewport scroll offset changed.
        const VIEWPORT          = 1 << 5;
        /// Status-line content changed.
        const STATUS            = 1 << 6;
        /// Sign column changed.
        const SIGNS             = 1 << 7;
    }
}

/// Compact dirty region: first/last modified line + net line count change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DirtyRange {
    /// First modified line (0-indexed).
    pub top: usize,
    /// Last modified line in the original numbering (0-indexed).
    pub bot: usize,
    /// Net lines added (positive) or removed (negative).
    pub lines_added: isize,
}

/// Accumulated dirty-line information from document mutations.
#[derive(Debug, Clone)]
pub struct DirtyInfo {
    /// Line numbers that were modified.
    lines: SmallVec<[usize; 16]>,
    /// Whether a full viewport redraw is needed (line count changed).
    full_redraw: bool,
    /// Per-aspect dirty flags accumulated since last drain.
    flags: DirtyFlags,
    /// Compact dirty region (O(1) space regardless of edit size).
    range: Option<DirtyRange>,
}

impl DirtyInfo {
    /// Line numbers that were modified.
    #[must_use]
    pub fn lines(&self) -> &[usize] {
        &self.lines
    }

    /// Whether a full viewport redraw is needed (line count changed).
    #[must_use]
    pub const fn full_redraw(&self) -> bool {
        self.full_redraw
    }

    /// Per-aspect dirty flags accumulated since last drain.
    #[must_use]
    pub const fn dirty_flags(&self) -> DirtyFlags {
        self.flags
    }

    /// Compact dirty region (O(1) space regardless of edit size).
    #[must_use]
    pub const fn range(&self) -> Option<DirtyRange> {
        self.range
    }
}

/// Tracks which lines have been modified since last drain.
#[derive(Debug, Clone)]
pub struct DirtyTracker {
    lines: SmallVec<[usize; 16]>,
    full_redraw: bool,
    flags: DirtyFlags,
    range: Option<DirtyRange>,
}

impl DirtyTracker {
    /// Create an empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            lines: SmallVec::new(),
            full_redraw: false,
            flags: DirtyFlags::empty(),
            range: None,
        }
    }

    /// Record specific dirty line numbers.
    pub fn record_lines(&mut self, affected: &[usize]) {
        self.lines.extend_from_slice(affected);
        self.flags |= DirtyFlags::LINES;
        for &line in affected {
            self.merge_range(line, line, 0);
        }
    }

    /// Record a contiguous range of dirty line numbers.
    pub fn record_line_range(&mut self, range: std::ops::RangeInclusive<usize>) {
        let top = *range.start();
        let bot = *range.end();
        self.lines.extend(range);
        self.flags |= DirtyFlags::LINES;
        self.merge_range(top, bot, 0);
    }

    fn merge_range(&mut self, top: usize, bot: usize, delta: isize) {
        match &mut self.range {
            Some(r) => {
                r.top = r.top.min(top);
                r.bot = r.bot.max(bot);
                r.lines_added += delta;
            }
            None => {
                self.range = Some(DirtyRange {
                    top,
                    bot,
                    lines_added: delta,
                });
            }
        }
    }

    /// Mark that a full redraw is needed (line count changed).
    pub const fn record_full_redraw(&mut self) {
        self.full_redraw = true;
    }

    /// Set one or more fine-grained dirty flags.
    pub fn set_flags(&mut self, flags: DirtyFlags) {
        self.flags |= flags;
    }

    /// Current dirty flags (without draining).
    #[must_use]
    pub const fn dirty_flags(&self) -> DirtyFlags {
        self.flags
    }

    /// Whether there are no recorded changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && !self.full_redraw && self.flags.is_empty()
    }

    /// Drain and reset, returning accumulated dirty info.
    pub fn take(&mut self) -> DirtyInfo {
        DirtyInfo {
            lines: std::mem::take(&mut self.lines),
            full_redraw: std::mem::replace(&mut self.full_redraw, false),
            flags: std::mem::replace(&mut self.flags, DirtyFlags::empty()),
            range: self.range.take(),
        }
    }
}

impl Default for DirtyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "dirty_tracker_tests.rs"]
mod tests;
