//! Selection types for vim-core.
//!
//! Selection with anchor and head for visual mode.
//!
//! # Gap Indexing
//!
//! `SelectionRange` uses [`Offset`] for both `anchor` and `head`. Gap offsets
//! point *between* characters, making selection ranges naturally half-open
//! `[anchor, head)` without off-by-one ambiguity.
//!
//! - A **normal-mode cursor** is a 1-char-wide selection: `head = anchor + char_width`.
//! - An **insert-mode cursor** is zero-width (collapsed): `head == anchor`.
//! - A **visual selection** spans multiple characters: `|head - anchor| > char_width`.

use crate::primitives::Offset;

/// A single selection range using gap indexing.
///
/// With gap indexing, `anchor` and `head` are gap positions
/// (between characters). A forward selection `[anchor, head)` covers the
/// characters whose byte ranges fall within that interval.
///
/// # Key states
///
/// | State | Condition | Example |
/// |-------|-----------|---------|
/// | Zero-width (insert cursor) | `anchor == head` | `insert_cursor(gap)` |
/// | Single-char (normal cursor) | `end - start == char_width` | `cursor(text, gap)` |
/// | Multi-char (visual selection) | `end - start > char_width` | `new(a, b)` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SelectionRange {
    /// Where selection started (gap offset — between characters).
    anchor: Offset,
    /// Where cursor is (gap offset — between characters).
    head: Offset,
    /// Per-cursor sticky column goal for multi-cursor vertical motions.
    ///
    /// When `multi-cursor` feature is enabled, each cursor tracks its own
    /// desired column independently. Single-cursor mode still uses
    /// `VimState.sticky_column`.
    goal: Option<super::VirtualColumn>,
}

impl SelectionRange {
    /// Create a new selection range from two gap offsets.
    #[inline]
    #[must_use]
    pub const fn new(anchor: Offset, head: Offset) -> Self {
        Self {
            anchor,
            head,
            goal: None,
        }
    }

    /// Create a 1-char-wide selection (normal-mode cursor) at the given gap position.
    ///
    /// The selection spans from `gap` to `gap + char_width`, where `char_width`
    /// is the byte length of the character starting at `gap` in `text`.
    ///
    /// # Empty document guard
    ///
    /// If `gap.get() >= text.len()`, returns a zero-width selection (same as
    /// `insert_cursor`). This handles the empty-document edge case where there
    /// is no character to select.
    #[inline]
    #[must_use]
    pub const fn cursor(text: &str, gap: Offset) -> Self {
        if gap.get() >= text.len() {
            return Self {
                anchor: gap,
                head: gap,
                goal: None,
            };
        }
        let end = crate::primitives::text_util::next_char_boundary(text, gap.get());
        Self {
            anchor: gap,
            head: Offset::new(end),
            goal: None,
        }
    }

    /// Zero-width cursor for insert mode.
    ///
    /// Both anchor and head point at the same gap position. This represents
    /// the blinking line cursor between characters in insert mode.
    #[inline]
    #[must_use]
    pub const fn insert_cursor(gap: Offset) -> Self {
        Self {
            anchor: gap,
            head: gap,
            goal: None,
        }
    }

    /// Get the anchor (where selection started).
    #[inline]
    #[must_use]
    pub const fn anchor(self) -> Offset {
        self.anchor
    }

    /// Get the head (where cursor is).
    #[inline]
    #[must_use]
    pub const fn head(self) -> Offset {
        self.head
    }

    /// Return a new SelectionRange with a different head.
    #[inline]
    #[must_use]
    pub const fn with_head(self, head: Offset) -> Self {
        Self {
            anchor: self.anchor,
            head,
            goal: self.goal,
        }
    }

    /// Return a new SelectionRange with a different anchor.
    #[inline]
    #[must_use]
    pub const fn with_anchor(self, anchor: Offset) -> Self {
        Self {
            anchor,
            head: self.head,
            goal: self.goal,
        }
    }

    /// Get the start (smaller of anchor/head).
    #[inline]
    #[must_use]
    pub const fn start(self) -> Offset {
        self.anchor.min(self.head)
    }

    /// Get the end (larger of anchor/head).
    #[inline]
    #[must_use]
    pub const fn end(self) -> Offset {
        self.anchor.max(self.head)
    }

    /// Check if forward (anchor <= head).
    #[inline]
    #[must_use]
    pub const fn is_forward(self) -> bool {
        self.anchor.get() <= self.head.get()
    }

    /// Check if selection is zero-width (anchor == head).
    ///
    /// # Semantic change from `Offset`-based design
    ///
    /// Previously, `is_collapsed()` meant "normal-mode cursor" (anchor == head,
    /// both pointing at the same character). With gap indexing, `is_collapsed()`
    /// means "zero-width / insert-mode cursor" — no characters are selected.
    /// A normal-mode cursor is now 1-char wide and is NOT collapsed.
    #[inline]
    #[must_use]
    pub const fn is_collapsed(self) -> bool {
        self.anchor.get() == self.head.get()
    }

    /// Return a new selection with anchor and head swapped.
    #[inline]
    #[must_use]
    pub const fn flipped(self) -> Self {
        Self {
            anchor: self.head,
            head: self.anchor,
            goal: self.goal,
        }
    }

    /// Get the per-cursor sticky column goal (multi-cursor only).
    ///
    /// Returns `None` when no goal has been set (vertical motions should
    /// fall back to `VimState.sticky_column` for single-cursor mode).
    #[inline]
    #[must_use]
    pub const fn goal(self) -> Option<super::VirtualColumn> {
        self.goal
    }

    /// Return a new SelectionRange with the given sticky column goal.
    #[inline]
    #[must_use]
    pub const fn with_goal(self, goal: Option<super::VirtualColumn>) -> Self {
        Self {
            anchor: self.anchor,
            head: self.head,
            goal,
        }
    }

    /// Where the block cursor displays (character-pointing offset).
    ///
    /// For a forward selection (`head > anchor`), the cursor visually sits on
    /// the character *before* the head gap, so we step back to the previous
    /// character boundary. For a backward or zero-width selection, the head
    /// gap already points at (before) the displayed character.
    #[inline]
    #[must_use]
    pub fn cursor_offset(&self, text: &str) -> Offset {
        if self.head > self.anchor {
            Offset::new(crate::primitives::text_util::prev_char_boundary(
                text,
                self.head.get(),
            ))
        } else {
            self.head
        }
    }

    /// True when selection spans exactly one character (normal-mode cursor).
    ///
    /// Returns `false` for zero-width (collapsed) selections, even though they
    /// are "at" a character position.
    #[must_use]
    pub fn is_single_char(&self, text: &str) -> bool {
        if self.is_collapsed() {
            return false;
        }
        let start = self.start().get();
        let end = self.end().get();
        text.get(start..)
            .and_then(|s| s.chars().next())
            .is_some_and(|c| start + c.len_utf8() == end)
    }

    /// Extend this selection to encompass the range `[from, to)`.
    ///
    /// Preserves direction: a forward selection stays forward, a backward
    /// selection stays backward. Each endpoint is only moved outward, never
    /// inward.
    #[inline]
    #[must_use]
    pub const fn extend(self, from: Offset, to: Offset) -> Self {
        debug_assert!(from.get() <= to.get());
        if self.is_forward() {
            Self::new(
                if self.anchor.get() < from.get() {
                    self.anchor
                } else {
                    from
                },
                if self.head.get() > to.get() {
                    self.head
                } else {
                    to
                },
            )
        } else {
            Self::new(
                if self.anchor.get() > to.get() {
                    self.anchor
                } else {
                    to
                },
                if self.head.get() < from.get() {
                    self.head
                } else {
                    from
                },
            )
        }
    }

    /// Merge two selections into one that covers both.
    ///
    /// If both selections are backward, the result is backward. Otherwise the
    /// result is forward, spanning `[min(start), max(end))`.
    #[inline]
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        if !self.is_forward() && !other.is_forward() {
            Self::new(
                if self.anchor.get() > other.anchor.get() {
                    self.anchor
                } else {
                    other.anchor
                },
                if self.head.get() < other.head.get() {
                    self.head
                } else {
                    other.head
                },
            )
        } else {
            Self::new(self.start().min(other.start()), self.end().max(other.end()))
        }
    }

    /// Snap selection boundaries to grapheme cluster boundaries.
    ///
    /// For non-zero-width ranges, the start snaps backward (range grows left)
    /// and the end snaps forward (range grows right). Direction is preserved.
    /// Zero-width ranges snap both endpoints backward.
    #[must_use]
    pub fn grapheme_aligned(self, text: &str) -> Self {
        use unicode_segmentation::UnicodeSegmentation;

        if text.is_empty() {
            return self;
        }

        let boundaries: smallvec::SmallVec<[usize; 64]> = text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(core::iter::once(text.len()))
            .collect();

        let snap_prev = |pos: usize| -> usize {
            match boundaries.binary_search(&pos) {
                Ok(i) => boundaries[i],
                Err(0) => 0,
                Err(i) => boundaries[i - 1],
            }
        };

        let snap_next = |pos: usize| -> usize {
            match boundaries.binary_search(&pos) {
                Ok(i) => boundaries[i],
                Err(i) if i >= boundaries.len() => *boundaries.last().unwrap_or(&0),
                Err(i) => boundaries[i],
            }
        };

        if self.is_collapsed() {
            let snapped = snap_prev(self.anchor.get());
            return Self::new(Offset::new(snapped), Offset::new(snapped));
        }

        if self.is_forward() {
            Self::new(
                Offset::new(snap_prev(self.anchor.get())),
                Offset::new(snap_next(self.head.get())),
            )
        } else {
            Self::new(
                Offset::new(snap_next(self.anchor.get())),
                Offset::new(snap_prev(self.head.get())),
            )
        }
    }
}

/// Selection state for the editor.
///
/// Wraps a single `SelectionRange`. Vim uses one cursor/selection at a time.
/// This wrapper provides the semantic distinction between "a range" and
/// "the editor's current selection state."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Selection {
    /// The active selection range.
    range: SelectionRange,
}

impl Default for Selection {
    /// Default selection: zero-width at offset 0.
    fn default() -> Self {
        Self::cursor(Offset::ZERO)
    }
}

impl Selection {
    /// Create a new selection with a single range.
    #[inline]
    #[must_use]
    pub const fn single(range: SelectionRange) -> Self {
        Self { range }
    }

    /// Create a cursor (zero-width / insert-mode selection).
    ///
    /// This method creates a **zero-width** (insert-mode) selection. For a
    /// proper normal-mode cursor (1-char wide), use [`cursor_at`](Self::cursor_at)
    /// instead.
    #[inline]
    #[must_use]
    pub const fn cursor(offset: Offset) -> Self {
        Self::single(SelectionRange::insert_cursor(offset))
    }

    /// Create a 1-char-wide cursor (normal-mode selection) at the given gap position.
    ///
    /// This is the correct way to create a normal-mode cursor with gap indexing.
    /// The selection will span from `gap` to `gap + char_width`.
    #[inline]
    #[must_use]
    pub const fn cursor_at(text: &str, gap: Offset) -> Self {
        Self::single(SelectionRange::cursor(text, gap))
    }

    /// Get the primary selection.
    #[inline]
    #[must_use]
    pub const fn primary(&self) -> &SelectionRange {
        &self.range
    }

    /// Get mutable reference to primary selection.
    #[inline]
    pub const fn primary_mut(&mut self) -> &mut SelectionRange {
        &mut self.range
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === SelectionRange with gap indexing ===

    #[test]
    fn forward_selection() {
        let sr = SelectionRange::new(Offset::new(3), Offset::new(7));
        assert!(sr.is_forward());
        assert_eq!(sr.start(), Offset::new(3));
        assert_eq!(sr.end(), Offset::new(7));
    }

    #[test]
    fn backward_selection() {
        let sr = SelectionRange::new(Offset::new(7), Offset::new(3));
        assert!(!sr.is_forward());
        assert_eq!(sr.start(), Offset::new(3));
        assert_eq!(sr.end(), Offset::new(7));
    }

    #[test]
    fn cursor_is_one_char_wide() {
        // "hello" — cursor at gap 0 → anchor=0, head=1 (selecting 'h')
        let sr = SelectionRange::cursor("hello", Offset::new(0));
        assert!(!sr.is_collapsed()); // NOT collapsed — it's 1-char wide
        assert!(sr.is_forward());
        assert_eq!(sr.anchor(), Offset::new(0));
        assert_eq!(sr.head(), Offset::new(1));
        assert!(sr.is_single_char("hello"));
    }

    #[test]
    fn cursor_at_last_char() {
        // "hello" — cursor at gap 4 → anchor=4, head=5 (selecting 'o')
        let sr = SelectionRange::cursor("hello", Offset::new(4));
        assert!(!sr.is_collapsed());
        assert_eq!(sr.anchor(), Offset::new(4));
        assert_eq!(sr.head(), Offset::new(5));
        assert!(sr.is_single_char("hello"));
    }

    #[test]
    fn cursor_on_multibyte_char() {
        // '£' is 2 bytes (U+00A3)
        let text = "£x";
        let sr = SelectionRange::cursor(text, Offset::new(0));
        assert!(!sr.is_collapsed());
        assert_eq!(sr.anchor(), Offset::new(0));
        assert_eq!(sr.head(), Offset::new(2)); // '£' is 2 bytes
        assert!(sr.is_single_char(text));
    }

    #[test]
    fn cursor_empty_doc_guard() {
        // Empty string — gap 0 is at/past end, so zero-width
        let sr = SelectionRange::cursor("", Offset::new(0));
        assert!(sr.is_collapsed());
        assert_eq!(sr.anchor(), Offset::new(0));
        assert_eq!(sr.head(), Offset::new(0));
    }

    #[test]
    fn cursor_past_end_guard() {
        // gap offset past end of text
        let sr = SelectionRange::cursor("hi", Offset::new(5));
        assert!(sr.is_collapsed());
        assert_eq!(sr.anchor(), Offset::new(5));
        assert_eq!(sr.head(), Offset::new(5));
    }

    #[test]
    fn insert_cursor_is_collapsed() {
        let sr = SelectionRange::insert_cursor(Offset::new(5));
        assert!(sr.is_collapsed());
        assert!(sr.is_forward()); // equal counts as forward
        assert_eq!(sr.start(), sr.end());
        assert_eq!(sr.anchor(), Offset::new(5));
        assert_eq!(sr.head(), Offset::new(5));
    }

    #[test]
    fn insert_cursor_is_not_single_char() {
        let sr = SelectionRange::insert_cursor(Offset::new(2));
        assert!(!sr.is_single_char("hello"));
    }

    #[test]
    fn flipped() {
        let sr = SelectionRange::new(Offset::new(3), Offset::new(7));
        assert!(sr.is_forward());
        let sr = sr.flipped();
        assert!(!sr.is_forward());
        assert_eq!(sr.anchor(), Offset::new(7));
        assert_eq!(sr.head(), Offset::new(3));
    }

    #[test]
    fn double_flip_is_identity() {
        let original = SelectionRange::new(Offset::new(3), Offset::new(7));
        assert_eq!(original.flipped().flipped(), original);
    }

    // === cursor_offset ===

    #[test]
    fn cursor_offset_forward_selection() {
        // Forward selection [0, 5) on "hello" — cursor displays on char before head
        let sr = SelectionRange::new(Offset::new(0), Offset::new(5));
        // prev_char_boundary(5) on "hello" = 4 ('o')
        assert_eq!(sr.cursor_offset("hello"), Offset::new(4));
    }

    #[test]
    fn cursor_offset_single_char() {
        // Cursor on 'h': [0, 1) — cursor displays at 0
        let sr = SelectionRange::cursor("hello", Offset::new(0));
        assert_eq!(sr.cursor_offset("hello"), Offset::new(0));
    }

    #[test]
    fn cursor_offset_backward_selection() {
        // Backward selection: anchor=7, head=3
        let sr = SelectionRange::new(Offset::new(7), Offset::new(3));
        // head <= anchor, so cursor_offset = head.get() = 3
        assert_eq!(sr.cursor_offset("hello world"), Offset::new(3));
    }

    #[test]
    fn cursor_offset_zero_width() {
        // Zero-width: anchor=5, head=5 (insert cursor)
        let sr = SelectionRange::insert_cursor(Offset::new(5));
        // head == anchor, not head > anchor, so returns head.get() = 5
        assert_eq!(sr.cursor_offset("hello"), Offset::new(5));
    }

    #[test]
    fn cursor_offset_multibyte_forward() {
        // "£x" — forward selection [0, 3): cursor on char before gap 3 = 'x' at byte 2
        let text = "£x";
        let sr = SelectionRange::new(Offset::new(0), Offset::new(3));
        assert_eq!(sr.cursor_offset(text), Offset::new(2));
    }

    // === is_single_char ===

    #[test]
    fn is_single_char_ascii() {
        let text = "hello";
        // [2, 3) spans exactly 'l'
        let sr = SelectionRange::new(Offset::new(2), Offset::new(3));
        assert!(sr.is_single_char(text));
    }

    #[test]
    fn is_single_char_multibyte() {
        let text = "£x"; // '£' is 2 bytes
                         // [0, 2) spans exactly '£'
        let sr = SelectionRange::new(Offset::new(0), Offset::new(2));
        assert!(sr.is_single_char(text));
    }

    #[test]
    fn is_single_char_two_chars_is_false() {
        let text = "hello";
        // [2, 4) spans 'l' and 'l' — not single char
        let sr = SelectionRange::new(Offset::new(2), Offset::new(4));
        assert!(!sr.is_single_char(text));
    }

    #[test]
    fn is_single_char_backward() {
        let text = "hello";
        // Backward [3, 2): start=2, end=3, spans exactly 'l'
        let sr = SelectionRange::new(Offset::new(3), Offset::new(2));
        assert!(sr.is_single_char(text));
    }

    // === Selection wrapper ===

    #[test]
    fn single_selection() {
        let sel = Selection::single(SelectionRange::new(Offset::new(3), Offset::new(7)));
        assert_eq!(sel.primary().start(), Offset::new(3));
    }

    #[test]
    fn cursor_selection_bridge() {
        // Bridge: Selection::cursor(Offset) creates zero-width (insert) selection
        let sel = Selection::cursor(Offset::new(5));
        assert!(sel.primary().is_collapsed());
        assert_eq!(sel.primary().anchor(), Offset::new(5));
        assert_eq!(sel.primary().head(), Offset::new(5));
    }

    #[test]
    fn cursor_at_selection() {
        // cursor_at: creates 1-char-wide normal-mode cursor
        let sel = Selection::cursor_at("hello", Offset::new(2));
        assert!(!sel.primary().is_collapsed());
        assert_eq!(sel.primary().anchor(), Offset::new(2));
        assert_eq!(sel.primary().head(), Offset::new(3));
        assert!(sel.primary().is_single_char("hello"));
    }

    #[test]
    fn primary_mut() {
        let mut sel = Selection::cursor(Offset::new(5));
        *sel.primary_mut() = sel.primary().with_head(Offset::new(10));
        assert!(!sel.primary().is_collapsed());
        assert_eq!(sel.primary().head(), Offset::new(10));
    }

    #[test]
    fn selection_is_copy() {
        let sel = Selection::cursor(Offset::new(5));
        let sel2 = sel; // Copy
        assert_eq!(sel, sel2);
    }

    #[test]
    fn default_selection_is_zero_width_at_zero() {
        let sel = Selection::default();
        assert!(sel.primary().is_collapsed());
        assert_eq!(sel.primary().anchor(), Offset::ZERO);
        assert_eq!(sel.primary().head(), Offset::ZERO);
    }

    // === extend ===

    #[test]
    fn extend_forward_grows_outward() {
        // [5,10) extend (3,12) → [3,12)
        let sr = SelectionRange::new(Offset::new(5), Offset::new(10));
        let result = sr.extend(Offset::new(3), Offset::new(12));
        assert_eq!(result.anchor(), Offset::new(3));
        assert_eq!(result.head(), Offset::new(12));
    }

    #[test]
    fn extend_forward_already_encompasses() {
        // [3,12) extend (5,10) → no change
        let sr = SelectionRange::new(Offset::new(3), Offset::new(12));
        let result = sr.extend(Offset::new(5), Offset::new(10));
        assert_eq!(result, sr);
    }

    #[test]
    fn extend_backward_grows_outward() {
        // [10,5) extend (3,12) → [12,3)
        let sr = SelectionRange::new(Offset::new(10), Offset::new(5));
        let result = sr.extend(Offset::new(3), Offset::new(12));
        assert_eq!(result.anchor(), Offset::new(12));
        assert_eq!(result.head(), Offset::new(3));
    }

    #[test]
    fn extend_backward_already_encompasses() {
        // [12,3) already encompasses (5,10) → no change
        let sr = SelectionRange::new(Offset::new(12), Offset::new(3));
        let result = sr.extend(Offset::new(5), Offset::new(10));
        assert_eq!(result, sr);
    }

    #[test]
    fn extend_zero_width_grows() {
        // [5,5) extend (3,8) → [3,8)
        let sr = SelectionRange::insert_cursor(Offset::new(5));
        let result = sr.extend(Offset::new(3), Offset::new(8));
        assert_eq!(result.anchor(), Offset::new(3));
        assert_eq!(result.head(), Offset::new(8));
    }

    // === merge ===

    #[test]
    fn merge_two_forward_overlapping() {
        // [0,5) merge [3,8) → [0,8)
        let a = SelectionRange::new(Offset::new(0), Offset::new(5));
        let b = SelectionRange::new(Offset::new(3), Offset::new(8));
        let result = a.merge(b);
        assert_eq!(result.anchor(), Offset::new(0));
        assert_eq!(result.head(), Offset::new(8));
    }

    #[test]
    fn merge_two_backward() {
        // [5,0) merge [8,3) → [8,0)
        let a = SelectionRange::new(Offset::new(5), Offset::new(0));
        let b = SelectionRange::new(Offset::new(8), Offset::new(3));
        let result = a.merge(b);
        assert_eq!(result.anchor(), Offset::new(8));
        assert_eq!(result.head(), Offset::new(0));
    }

    #[test]
    fn merge_mixed_direction_becomes_forward() {
        // [5,0) merge [3,8) → [0,8)
        let a = SelectionRange::new(Offset::new(5), Offset::new(0));
        let b = SelectionRange::new(Offset::new(3), Offset::new(8));
        let result = a.merge(b);
        assert_eq!(result.anchor(), Offset::new(0));
        assert_eq!(result.head(), Offset::new(8));
    }

    #[test]
    fn merge_non_overlapping() {
        // [0,3) merge [7,10) → [0,10)
        let a = SelectionRange::new(Offset::new(0), Offset::new(3));
        let b = SelectionRange::new(Offset::new(7), Offset::new(10));
        let result = a.merge(b);
        assert_eq!(result.anchor(), Offset::new(0));
        assert_eq!(result.head(), Offset::new(10));
    }

    #[test]
    fn merge_zero_width_with_forward() {
        // [5,5) merge [3,8) → [3,8) (collapsed is forward, result forward)
        let a = SelectionRange::insert_cursor(Offset::new(5));
        let b = SelectionRange::new(Offset::new(3), Offset::new(8));
        let merged = a.merge(b);
        assert_eq!(merged.anchor(), Offset::new(3));
        assert_eq!(merged.head(), Offset::new(8));
    }

    #[test]
    fn merge_identical() {
        // [3,7) merge [3,7) → [3,7)
        let a = SelectionRange::new(Offset::new(3), Offset::new(7));
        let b = SelectionRange::new(Offset::new(3), Offset::new(7));
        let result = a.merge(b);
        assert_eq!(result, a);
    }

    // === grapheme_aligned ===

    #[test]
    fn grapheme_aligned_ascii_no_change() {
        let sr = SelectionRange::new(Offset::new(1), Offset::new(4));
        let aligned = sr.grapheme_aligned("hello");
        assert_eq!(aligned, sr);
    }

    #[test]
    fn grapheme_aligned_combining_mark_snaps_outward() {
        // "e\u{0301}" = é as e + combining acute accent (3 bytes total)
        // Selection [0,1) cuts mid-grapheme — should snap to [0,3)
        let text = "e\u{0301}x";
        let sr = SelectionRange::new(Offset::new(0), Offset::new(1));
        let aligned = sr.grapheme_aligned(text);
        assert_eq!(aligned.anchor(), Offset::new(0));
        assert_eq!(aligned.head(), Offset::new(3)); // full grapheme "é"
    }

    #[test]
    fn grapheme_aligned_zero_width_snaps_backward() {
        // Zero-width at byte 1 (mid-grapheme) → snap both backward to 0
        let text = "e\u{0301}x";
        let sr = SelectionRange::insert_cursor(Offset::new(1));
        let aligned = sr.grapheme_aligned(text);
        assert_eq!(aligned.anchor(), Offset::new(0));
        assert_eq!(aligned.head(), Offset::new(0));
        assert!(aligned.is_collapsed());
    }

    #[test]
    fn grapheme_aligned_zero_width_at_boundary_no_change() {
        let text = "e\u{0301}x";
        // Byte 3 is the boundary between "é" and "x"
        let sr = SelectionRange::insert_cursor(Offset::new(3));
        let aligned = sr.grapheme_aligned(text);
        assert_eq!(aligned, sr);
    }

    #[test]
    fn grapheme_aligned_backward_preserves_direction() {
        // Backward [4,0) on "e\u{0301}x" — already on boundaries → no change
        let text = "e\u{0301}x";
        let sr = SelectionRange::new(Offset::new(4), Offset::new(0));
        let aligned = sr.grapheme_aligned(text);
        assert!(!aligned.is_forward());
        assert_eq!(aligned.anchor(), Offset::new(4));
        assert_eq!(aligned.head(), Offset::new(0));
    }

    #[test]
    fn grapheme_aligned_backward_mid_grapheme() {
        // Backward selection with anchor mid-grapheme
        // "e\u{0301}x" — backward [1, 0) → anchor snaps forward to 3, head stays 0 → [3, 0)
        let text = "e\u{0301}x";
        let sr = SelectionRange::new(Offset::new(1), Offset::new(0));
        let aligned = sr.grapheme_aligned(text);
        assert!(!aligned.is_forward());
        assert_eq!(aligned.anchor(), Offset::new(3)); // snapped forward
        assert_eq!(aligned.head(), Offset::new(0));
    }

    // === Per-cursor SelectionGoal (multi-cursor feature) ===

    mod multi_cursor_goal {
        use super::*;
        use crate::primitives::VirtualColumn;

        #[test]
        fn goal_default_is_none() {
            let sr = SelectionRange::new(Offset::new(0), Offset::new(5));
            assert_eq!(sr.goal(), None);
        }

        #[test]
        fn with_goal_sets_and_gets() {
            let sr = SelectionRange::new(Offset::new(0), Offset::new(5));
            let sr = sr.with_goal(Some(VirtualColumn::new(10)));
            assert_eq!(sr.goal(), Some(VirtualColumn::new(10)));
        }

        #[test]
        fn with_goal_none_clears() {
            let sr = SelectionRange::new(Offset::new(0), Offset::new(5))
                .with_goal(Some(VirtualColumn::new(10)));
            let sr = sr.with_goal(None);
            assert_eq!(sr.goal(), None);
        }

        #[test]
        fn with_goal_end_of_line() {
            let sr = SelectionRange::new(Offset::new(0), Offset::new(5))
                .with_goal(Some(VirtualColumn::END_OF_LINE));
            assert!(sr.goal().unwrap().is_end_of_line());
        }

        #[test]
        fn with_head_preserves_goal() {
            let sr = SelectionRange::new(Offset::new(0), Offset::new(5))
                .with_goal(Some(VirtualColumn::new(7)));
            let sr2 = sr.with_head(Offset::new(10));
            assert_eq!(sr2.goal(), Some(VirtualColumn::new(7)));
        }

        #[test]
        fn with_anchor_preserves_goal() {
            let sr = SelectionRange::new(Offset::new(0), Offset::new(5))
                .with_goal(Some(VirtualColumn::new(7)));
            let sr2 = sr.with_anchor(Offset::new(3));
            assert_eq!(sr2.goal(), Some(VirtualColumn::new(7)));
        }

        #[test]
        fn flipped_preserves_goal() {
            let sr = SelectionRange::new(Offset::new(0), Offset::new(5))
                .with_goal(Some(VirtualColumn::new(7)));
            let flipped = sr.flipped();
            assert_eq!(flipped.goal(), Some(VirtualColumn::new(7)));
        }

        #[test]
        fn selection_range_is_copy_with_goal() {
            let sr = SelectionRange::new(Offset::new(0), Offset::new(5))
                .with_goal(Some(VirtualColumn::new(7)));
            let sr2 = sr; // Copy
            assert_eq!(sr, sr2);
        }
    }
}
