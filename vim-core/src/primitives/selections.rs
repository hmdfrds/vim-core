//! # Multi-Cursor Selection State
//!
//! `Selections` (plural) is the multi-cursor-ready selection type.
//! It wraps `SmallVec<[SelectionRange; 1]>` so a single cursor lives
//! inline with zero heap allocation. Multiple cursors spill to the heap.
//!
//! # Invariants
//!
//! - Always contains at least one range (never empty)
//! - `primary_index` is always valid (`< ranges.len()`)
//! - After [`normalize`](Selections::normalize): ranges are sorted by start, non-overlapping
//!
//! # Relationship to `Selection`
//!
//! The existing `Selection` type (singular) remains for the single-cursor
//! path. `Selections` is the foundation for future multi-cursor support.
//! Conversion between the two is zero-cost for single-cursor.
//!
//! # Representation
//!
//! A `SmallVec<[Range; 1]>` paired with a `primary_index`: the common
//! single-cursor case stays inline with no heap allocation, while multiple
//! cursors spill to the heap only when they actually exist.

use smallvec::SmallVec;

use super::changeset::{Assoc, ChangeSet};
use super::{Offset, Selection, SelectionRange};

// ═══════════════════════════════════════════════════════════════════════════
// MULTI-CURSOR MODE
// ═══════════════════════════════════════════════════════════════════════════

/// Whether the editor is in single-cursor or multi-cursor mode.
///
/// When `Multi`, effect replication produces effects for all cursors.
/// When `Single`, only the primary cursor is used (default Vim behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CursorMode {
    /// Standard single-cursor mode (default Vim behavior).
    #[default]
    Single,
    /// Multi-cursor mode — effects are replicated to all cursors.
    Multi,
}

/// Multi-cursor selection state.
///
/// Uses `SmallVec<[SelectionRange; 1]>` — single cursor is inline (no heap).
/// Multiple cursors spill to heap automatically.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Selections {
    ranges: SmallVec<[SelectionRange; 1]>,
    primary_index: usize,
    /// Whether multi-cursor mode is active for this selection set.
    cursor_mode: CursorMode,
}

// ═══════════════════════════════════════════════════════════════════════════
// CONSTRUCTION
// ═══════════════════════════════════════════════════════════════════════════

impl Selections {
    /// Create with a single range.
    #[inline]
    #[must_use]
    pub fn single(range: SelectionRange) -> Self {
        Self {
            ranges: SmallVec::from_elem(range, 1),
            primary_index: 0,
            cursor_mode: CursorMode::Single,
        }
    }

    /// Create a single zero-width cursor (insert-mode).
    ///
    /// # Deprecation bridge
    ///
    /// This creates a **zero-width** selection via `SelectionRange::insert_cursor`.
    /// For a proper 1-char-wide normal-mode cursor, use [`cursor_at`](Self::cursor_at).
    #[inline]
    #[must_use]
    pub fn cursor(offset: Offset) -> Self {
        Self::single(SelectionRange::insert_cursor(offset))
    }

    /// Create a single 1-char-wide cursor (normal-mode) at the given gap position.
    #[inline]
    #[must_use]
    pub fn cursor_at(text: &str, gap: Offset) -> Self {
        Self::single(SelectionRange::cursor(text, gap))
    }

    /// Create from multiple ranges with a primary index.
    ///
    /// # Panics
    ///
    /// Panics if `ranges` is empty or `primary_index >= ranges.len()`.
    #[must_use]
    pub fn new(ranges: SmallVec<[SelectionRange; 1]>, primary_index: usize) -> Self {
        assert!(
            !ranges.is_empty(),
            "Selections must have at least one range"
        );
        assert!(
            primary_index < ranges.len(),
            "primary_index ({primary_index}) out of bounds (len={})",
            ranges.len()
        );
        Self {
            ranges,
            primary_index,
            cursor_mode: CursorMode::Single,
        }
    }

    /// Create from a `Vec` of ranges with a primary index.
    ///
    /// # Panics
    ///
    /// Panics if `ranges` is empty or `primary_index >= ranges.len()`.
    #[must_use]
    pub fn from_vec(ranges: Vec<SelectionRange>, primary_index: usize) -> Self {
        assert!(
            !ranges.is_empty(),
            "Selections must have at least one range"
        );
        assert!(
            primary_index < ranges.len(),
            "primary_index ({primary_index}) out of bounds (len={})",
            ranges.len()
        );
        Self {
            ranges: SmallVec::from_vec(ranges),
            primary_index,
            cursor_mode: CursorMode::Single,
        }
    }

    /// Convert from the single-cursor `Selection` type.
    #[inline]
    #[must_use]
    pub fn from_selection(sel: Selection) -> Self {
        Self::single(*sel.primary())
    }

    /// Convert to the single-cursor `Selection` type.
    ///
    /// Returns the primary range as a `Selection`.
    #[inline]
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "primary_index is invariant-guaranteed < ranges.len()"
    )]
    pub fn to_selection(&self) -> Selection {
        Selection::single(self.ranges[self.primary_index])
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// QUERIES
// ═══════════════════════════════════════════════════════════════════════════

impl Selections {
    /// Get the primary selection range.
    #[inline]
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "primary_index is invariant-guaranteed < ranges.len()"
    )]
    pub fn primary(&self) -> &SelectionRange {
        &self.ranges[self.primary_index]
    }

    /// Get mutable reference to the primary selection range.
    #[inline]
    #[allow(
        clippy::indexing_slicing,
        reason = "primary_index is invariant-guaranteed < ranges.len()"
    )]
    pub fn primary_mut(&mut self) -> &mut SelectionRange {
        &mut self.ranges[self.primary_index]
    }

    /// Get all selection ranges.
    #[inline]
    #[must_use]
    pub fn ranges(&self) -> &[SelectionRange] {
        &self.ranges
    }

    /// Number of selection ranges.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    /// Always false — `Selections` invariant guarantees at least one range.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// True if there is exactly one selection range (single cursor).
    #[inline]
    #[must_use]
    pub fn is_single(&self) -> bool {
        self.ranges.len() == 1
    }

    /// Index of the primary selection.
    #[inline]
    #[must_use]
    pub const fn primary_index(&self) -> usize {
        self.primary_index
    }

    /// Iterate over all selection ranges.
    pub fn iter(&self) -> impl Iterator<Item = &SelectionRange> + '_ {
        self.ranges.iter()
    }

    /// Check if any selection range contains the given offset.
    ///
    /// A range "contains" an offset if `start.get() <= offset.get() && offset.get() <= end.get()`.
    ///
    /// Note: Uses `.get()` to compare raw `usize` values.
    #[must_use]
    pub fn contains_offset(&self, offset: Offset) -> bool {
        self.ranges
            .iter()
            .any(|r| r.start().get() <= offset.get() && offset.get() <= r.end().get())
    }

    /// Returns `true` if every range in `other` is fully contained
    /// by some range in `self`.
    ///
    /// Both must be [normalized](Self::normalize) (sorted, non-overlapping).
    /// This is the default state for `Selections`.
    ///
    /// O(N + M) single sweep over both sorted range slices.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "si is bounded by self.ranges.len() check"
    )]
    pub fn contains(&self, other: &Selections) -> bool {
        let mut si = 0;
        for o in other.ranges() {
            while si < self.ranges.len() && self.ranges[si].end().get() < o.start().get() {
                si += 1;
            }
            if si >= self.ranges.len() {
                return false;
            }
            if self.ranges[si].start().get() > o.start().get() {
                return false;
            }
            if self.ranges[si].end().get() < o.end().get() {
                return false;
            }
        }
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TRANSFORMATIONS
// ═══════════════════════════════════════════════════════════════════════════

impl Selections {
    /// Apply a function to every range, returning new Selections.
    ///
    /// The primary index is preserved. The result should typically be
    /// followed by [`normalize`](Self::normalize) to maintain invariants.
    #[must_use]
    pub fn transform<F>(self, mut f: F) -> Self
    where
        F: FnMut(SelectionRange) -> SelectionRange,
    {
        Self {
            ranges: self.ranges.into_iter().map(&mut f).collect(),
            primary_index: self.primary_index,
            cursor_mode: self.cursor_mode,
        }
    }

    /// Flat-map each range to zero or more ranges, flatten, and normalize.
    ///
    /// Primary tracking:
    /// - The **first** range produced from the old primary becomes the new primary.
    /// - If the old primary produces nothing, the nearest surviving range
    ///   by `start()` position becomes primary (ties broken by lower index).
    /// - If **all** ranges produce nothing, returns a collapsed cursor at the
    ///   old primary's start position (invariant: never empty).
    ///
    /// The result is always [normalized](Self::normalize).
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "primary_index is invariant-guaranteed < ranges.len()"
    )]
    pub fn transform_iter<F, I>(self, mut f: F) -> Self
    where
        F: FnMut(SelectionRange) -> I,
        I: IntoIterator<Item = SelectionRange>,
    {
        let old_primary_start = self.ranges[self.primary_index].start();
        let old_primary_index = self.primary_index;
        let cursor_mode = self.cursor_mode;

        let mut out: SmallVec<[SelectionRange; 1]> = SmallVec::new();
        let mut new_primary: Option<usize> = None;

        for (i, range) in self.ranges.into_iter().enumerate() {
            let is_old_primary = i == old_primary_index;
            let first_of_group = out.len();

            out.extend(f(range));

            // First range produced from the old primary becomes new primary.
            if is_old_primary && new_primary.is_none() && out.len() > first_of_group {
                new_primary = Some(first_of_group);
            }
        }

        // All produced nothing — return cursor at old primary's start.
        if out.is_empty() {
            return Self {
                ranges: SmallVec::from_elem(SelectionRange::insert_cursor(old_primary_start), 1),
                primary_index: 0,
                cursor_mode,
            };
        }

        // If primary produced nothing, fall back to nearest by start position.
        let primary_index = new_primary.unwrap_or_else(|| {
            out.iter()
                .enumerate()
                .min_by_key(|(_, r)| r.start().get().abs_diff(old_primary_start.get()))
                .map_or(0, |(i, _)| i)
        });

        Self {
            ranges: out,
            primary_index,
            cursor_mode: CursorMode::Multi,
        }
        .normalize()
    }

    /// Keep only ranges where the predicate returns `true`.
    ///
    /// Primary tracking:
    /// - If the primary survives, it stays primary (at its new index).
    /// - If the primary is removed, the nearest surviving range by `start()`
    ///   position becomes primary (ties broken by lower index).
    ///
    /// Returns `None` if all ranges are filtered out (invariant: never empty).
    /// Does **not** call [`normalize`](Self::normalize) — filtering preserves
    /// order and cannot create overlaps.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "primary_index is invariant-guaranteed < ranges.len()"
    )]
    pub fn filter<F>(self, mut f: F) -> Option<Self>
    where
        F: FnMut(&SelectionRange) -> bool,
    {
        let old_primary_start = self.ranges[self.primary_index].start();
        let old_primary_index = self.primary_index;
        let cursor_mode = self.cursor_mode;

        let mut out: SmallVec<[SelectionRange; 1]> = SmallVec::new();
        let mut new_primary: Option<usize> = None;

        for (i, range) in self.ranges.into_iter().enumerate() {
            if f(&range) {
                if i == old_primary_index {
                    new_primary = Some(out.len());
                }
                out.push(range);
            }
        }

        if out.is_empty() {
            return None;
        }

        // If primary was removed, fall back to nearest by start position.
        let primary_index = new_primary.unwrap_or_else(|| {
            out.iter()
                .enumerate()
                .min_by_key(|(_, r)| r.start().get().abs_diff(old_primary_start.get()))
                .map_or(0, |(i, _)| i)
        });

        Some(Self {
            ranges: out,
            primary_index,
            cursor_mode,
        })
    }

    /// Merge ranges that are exactly touching (`end == next.start`).
    ///
    /// Assumes ranges are already sorted by start position (precondition —
    /// call [`normalize`](Self::normalize) first if unsure). Overlapping
    /// ranges are NOT handled here; use `normalize` for that.
    ///
    /// Primary tracking: if the primary range gets merged into an adjacent
    /// range, the merged result becomes primary.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "primary_index is invariant-guaranteed < ranges.len()"
    )]
    pub fn merge_consecutive(self) -> Self {
        if self.ranges.len() <= 1 {
            return self;
        }

        let primary_index = self.primary_index;
        let cursor_mode = self.cursor_mode;

        let mut merged: SmallVec<[(SelectionRange, bool); 1]> = SmallVec::new();

        for (i, range) in self.ranges.into_iter().enumerate() {
            let is_primary = i == primary_index;

            if let Some((last, last_is_primary)) = merged.last_mut() {
                if last.end() == range.start() {
                    // Touching — extend
                    *last = SelectionRange::new(last.start(), range.end());
                    if is_primary {
                        *last_is_primary = true;
                    }
                } else {
                    merged.push((range, is_primary));
                }
            } else {
                merged.push((range, is_primary));
            }
        }

        // Find primary in merged result
        let new_primary = merged
            .iter()
            .position(|(_, is_primary)| *is_primary)
            .unwrap_or(0);

        Self {
            ranges: merged.into_iter().map(|(r, _)| r).collect(),
            primary_index: new_primary,
            cursor_mode,
        }
    }

    /// Sort ranges by start position and merge overlapping ranges.
    ///
    /// After normalization:
    /// - Ranges are sorted by `start()` (ascending)
    /// - No two ranges overlap (they are merged)
    /// - `primary_index` tracks the primary through sorts and merges
    ///
    /// Merged ranges use forward direction (anchor=start, head=end).
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "indices are bounded: sorted_primary < tagged.len(), \
                  merged is non-empty (input is non-empty)"
    )]
    pub fn normalize(self) -> Self {
        if self.ranges.len() <= 1 {
            return self;
        }

        // Tag each range with its original index, then sort by start
        let mut tagged: SmallVec<[(SelectionRange, usize); 1]> = self
            .ranges
            .into_iter()
            .enumerate()
            .map(|(i, r)| (r, i))
            .collect();
        tagged.sort_by_key(|(r, _)| (r.start().get(), r.end().get()));

        // Find primary's position in sorted order
        let sorted_primary = tagged
            .iter()
            .position(|(_, orig)| *orig == self.primary_index)
            .unwrap_or(0);

        // Merge overlapping ranges
        let mut merged: SmallVec<[(SelectionRange, bool); 1]> = SmallVec::new();
        let mut primary_idx = 0;

        for (i, (range, _)) in tagged.iter().enumerate() {
            let is_primary = i == sorted_primary;

            if let Some((last, last_is_primary)) = merged.last_mut() {
                if range.start().get() <= last.end().get() {
                    // Overlapping — extend
                    let new_end = last.end().max(range.end());
                    *last = SelectionRange::new(last.start(), new_end);
                    if is_primary {
                        *last_is_primary = true;
                    }
                } else {
                    // Non-overlapping
                    merged.push((*range, is_primary));
                }
            } else {
                merged.push((*range, is_primary));
            }
        }

        // Find primary in merged result
        for (i, (_, is_primary)) in merged.iter().enumerate() {
            if *is_primary {
                primary_idx = i;
                break;
            }
        }

        Self {
            ranges: merged.into_iter().map(|(r, _)| r).collect(),
            primary_index: primary_idx,
            cursor_mode: self.cursor_mode,
        }
    }

    /// Map all selection positions through a changeset.
    ///
    /// Each range's anchor and head are mapped through the changeset
    /// using the given association. Uses `map_pos` (raw `usize`) since
    /// `SelectionRange` stores `Offset` (which wraps `usize`).
    #[must_use]
    pub fn map_through(&self, changeset: &ChangeSet, assoc: Assoc) -> Self {
        Self {
            ranges: self
                .ranges
                .iter()
                .map(|r| {
                    SelectionRange::new(
                        Offset::new(changeset.map_pos(r.anchor().get(), assoc)),
                        Offset::new(changeset.map_pos(r.head().get(), assoc)),
                    )
                })
                .collect(),
            primary_index: self.primary_index,
            cursor_mode: self.cursor_mode,
        }
    }

    /// Map all selection positions through a changeset using batched mapping.
    ///
    /// Collects all anchor/head positions, sorts them, maps them in a single
    /// sweep via [`ChangeSet::map_positions`], then reconstructs the ranges.
    /// For a single cursor, falls back to [`map_through`](Self::map_through)
    /// (no batching benefit).
    ///
    /// # Complexity
    ///
    /// O(N log N + M) where N = number of ranges, M = number of changeset ops.
    /// The sort is O(N log N), the sweep is O(N + M). Still far better than
    /// the non-batched [`map_through`](Self::map_through) which is O(N * M).
    #[must_use]
    pub fn map_through_batched(&self, changeset: &ChangeSet, assoc: Assoc) -> Self {
        // Single cursor: no batching benefit, delegate.
        if self.ranges.len() <= 1 {
            return self.map_through(changeset, assoc);
        }

        // Collect all positions as (position, range_index, is_head).
        let mut entries: Vec<(usize, usize, bool)> = Vec::with_capacity(self.ranges.len() * 2);
        for (i, r) in self.ranges.iter().enumerate() {
            entries.push((r.anchor().get(), i, false));
            entries.push((r.head().get(), i, true));
        }

        // Sort by position (stable so equal positions preserve order).
        entries.sort_by_key(|&(pos, _, _)| pos);

        // Extract sorted positions for batch mapping.
        let mut positions: Vec<usize> = entries.iter().map(|&(pos, _, _)| pos).collect();
        changeset.map_positions(&mut positions, assoc);

        // Reconstruct ranges from mapped positions.
        let mut anchors = vec![0usize; self.ranges.len()];
        let mut heads = vec![0usize; self.ranges.len()];
        for (idx, &(_, range_idx, is_head)) in entries.iter().enumerate() {
            if is_head {
                heads[range_idx] = positions[idx];
            } else {
                anchors[range_idx] = positions[idx];
            }
        }

        Self {
            ranges: anchors
                .iter()
                .zip(heads.iter())
                .map(|(&a, &h)| SelectionRange::new(Offset::new(a), Offset::new(h)))
                .collect(),
            primary_index: self.primary_index,
            cursor_mode: self.cursor_mode,
        }
    }

    /// Map all selection positions through a changeset with direction-based Assoc.
    ///
    /// Like [`map_through_batched`](Self::map_through_batched), but assigns
    /// per-position Assoc based on range direction:
    /// - Start-of-range (lower of anchor/head) gets [`Assoc::After`]
    /// - End-of-range (higher of anchor/head) gets [`Assoc::Before`]
    /// - Collapsed ranges (anchor == head) get [`Assoc::After`] for both
    ///
    /// This ensures insertions at selection boundaries are absorbed into the
    /// selection rather than pushing it.
    ///
    /// # Complexity
    ///
    /// O(N log N + M) where N = number of ranges, M = number of changeset ops.
    #[must_use]
    pub fn map_through_batched_directed(&self, changeset: &ChangeSet) -> Self {
        // Single cursor: use simple per-position mapping.
        if self.ranges.len() <= 1 {
            if let Some(r) = self.ranges.first() {
                let (anchor_assoc, head_assoc) = directed_assoc(*r);
                let new_anchor = changeset.map_pos(r.anchor().get(), anchor_assoc);
                let new_head = changeset.map_pos(r.head().get(), head_assoc);
                return Self {
                    ranges: SmallVec::from_elem(
                        SelectionRange::new(Offset::new(new_anchor), Offset::new(new_head)),
                        1,
                    ),
                    primary_index: self.primary_index,
                    cursor_mode: self.cursor_mode,
                };
            }
            return self.clone();
        }

        // Collect all positions tagged with their direction-based Assoc.
        let mut entries: Vec<(usize, Assoc, usize, bool)> =
            Vec::with_capacity(self.ranges.len() * 2);
        for (i, r) in self.ranges.iter().enumerate() {
            let (anchor_assoc, head_assoc) = directed_assoc(*r);
            entries.push((r.anchor().get(), anchor_assoc, i, false));
            entries.push((r.head().get(), head_assoc, i, true));
        }

        // Sort by position (stable so equal positions preserve insertion order).
        entries.sort_by_key(|&(pos, _, _, _)| pos);

        // Build the (position, assoc) pairs for the batch mapper.
        let mut positions: Vec<(usize, Assoc)> = entries
            .iter()
            .map(|&(pos, assoc, _, _)| (pos, assoc))
            .collect();
        changeset.map_positions_with_assoc(&mut positions);

        // Scatter results back to anchor/head arrays.
        let mut anchors = vec![0usize; self.ranges.len()];
        let mut heads = vec![0usize; self.ranges.len()];
        for (idx, &(_, _, range_idx, is_head)) in entries.iter().enumerate() {
            if is_head {
                heads[range_idx] = positions[idx].0;
            } else {
                anchors[range_idx] = positions[idx].0;
            }
        }

        Self {
            ranges: anchors
                .iter()
                .zip(heads.iter())
                .map(|(&a, &h)| SelectionRange::new(Offset::new(a), Offset::new(h)))
                .collect(),
            primary_index: self.primary_index,
            cursor_mode: self.cursor_mode,
        }
    }

    /// Push a new range. Does not normalize.
    pub fn push(&mut self, range: SelectionRange) {
        self.ranges.push(range);
    }

    /// Remove the range at the given index.
    ///
    /// Returns `None` if this is the only range (cannot remove the last cursor)
    /// or if the index is out of bounds. Returns `Some(removed_range)` on success.
    ///
    /// Adjusts `primary_index` to remain valid after the removal:
    /// - If the removed index was before the primary, primary shifts down by 1.
    /// - If the removed index was the primary, the primary moves to the previous
    ///   range (or wraps to 0 if it was at index 0).
    pub fn remove(&mut self, index: usize) -> Option<SelectionRange> {
        if self.ranges.len() <= 1 || index >= self.ranges.len() {
            return None;
        }

        let removed = self.ranges.remove(index);

        // Adjust primary_index to remain valid.
        if index < self.primary_index {
            // Removed before primary: primary shifts down.
            self.primary_index -= 1;
        } else if index == self.primary_index {
            // Removed the primary itself: clamp to valid range.
            if self.primary_index >= self.ranges.len() {
                self.primary_index = self.ranges.len() - 1;
            }
        }
        // index > primary_index: no adjustment needed.

        Some(removed)
    }

    /// Find the index of the range nearest to the given offset.
    ///
    /// "Nearest" is defined as the range whose head is closest (by absolute
    /// distance) to `offset`. Ties are broken by preferring the lower index.
    #[must_use]
    pub fn nearest_index(&self, offset: Offset) -> usize {
        self.ranges
            .iter()
            .enumerate()
            .min_by_key(|(_, r)| {
                let head = r.head().get();
                let target = offset.get();
                head.abs_diff(target)
            })
            .map_or(0, |(i, _)| i)
    }

    /// Set the primary index.
    ///
    /// # Panics
    ///
    /// Panics if `index >= self.len()`.
    pub fn set_primary_index(&mut self, index: usize) {
        assert!(
            index < self.ranges.len(),
            "primary_index ({index}) out of bounds (len={})",
            self.ranges.len()
        );
        self.primary_index = index;
    }

    /// Get the current cursor mode.
    #[inline]
    #[must_use]
    pub const fn cursor_mode(&self) -> CursorMode {
        self.cursor_mode
    }

    /// Set the cursor mode.
    #[inline]
    pub const fn set_cursor_mode(&mut self, mode: CursorMode) {
        self.cursor_mode = mode;
    }

    /// Remove all ranges except the primary.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "primary_index is invariant-guaranteed < ranges.len()"
    )]
    pub fn into_single(self) -> Self {
        let range = self.ranges[self.primary_index];
        let mut result = Self::single(range);
        result.cursor_mode = self.cursor_mode;
        result
    }
}

impl Default for Selections {
    fn default() -> Self {
        Self::cursor(Offset::ZERO)
    }
}

impl From<Selection> for Selections {
    fn from(sel: Selection) -> Self {
        Self::from_selection(sel)
    }
}

/// Compute direction-based Assoc for a range's anchor and head.
///
/// - Forward range (anchor <= head): anchor=After, head=Before
/// - Backward range (anchor > head): anchor=Before, head=After
/// - Collapsed (anchor == head): both=After
fn directed_assoc(r: SelectionRange) -> (Assoc, Assoc) {
    if r.anchor() == r.head() {
        (Assoc::After, Assoc::After)
    } else if r.is_forward() {
        (Assoc::After, Assoc::Before)
    } else {
        (Assoc::Before, Assoc::After)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction ─────────────────────────────────────────────────

    #[test]
    fn single_range() {
        let sel = Selections::single(SelectionRange::new(Offset::new(3), Offset::new(7)));
        assert_eq!(sel.len(), 1);
        assert!(sel.is_single());
        assert_eq!(sel.primary_index(), 0);
        assert_eq!(sel.primary().anchor(), Offset::new(3));
        assert_eq!(sel.primary().head(), Offset::new(7));
    }

    #[test]
    fn cursor_bridge() {
        // Bridge: creates zero-width (insert-mode) selection
        let sel = Selections::cursor(Offset::new(5));
        assert!(sel.is_single());
        assert!(sel.primary().is_collapsed());
        assert_eq!(sel.primary().head(), Offset::new(5));
    }

    #[test]
    fn cursor_at_creates_one_char_wide() {
        let sel = Selections::cursor_at("hello", Offset::new(2));
        assert!(sel.is_single());
        assert!(!sel.primary().is_collapsed());
        assert_eq!(sel.primary().anchor(), Offset::new(2));
        assert_eq!(sel.primary().head(), Offset::new(3));
    }

    #[test]
    fn multiple_ranges() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(10)),
            ],
            1, // primary is the second range
        );
        assert_eq!(sel.len(), 3);
        assert!(!sel.is_single());
        assert_eq!(sel.primary_index(), 1);
        assert_eq!(sel.primary().head(), Offset::new(5));
    }

    #[test]
    #[should_panic(expected = "at least one range")]
    fn empty_panics() {
        Selections::new(SmallVec::new(), 0);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn invalid_primary_panics() {
        Selections::from_vec(vec![SelectionRange::insert_cursor(Offset::new(0))], 1);
    }

    // ── Queries ──────────────────────────────────────────────────────

    #[test]
    fn contains_offset() {
        let sel = Selections::single(SelectionRange::new(Offset::new(3), Offset::new(7)));
        assert!(!sel.contains_offset(Offset::new(2)));
        assert!(sel.contains_offset(Offset::new(3)));
        assert!(sel.contains_offset(Offset::new(5)));
        assert!(sel.contains_offset(Offset::new(7)));
        assert!(!sel.contains_offset(Offset::new(8)));
    }

    #[test]
    fn contains_offset_multiple() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)),
                SelectionRange::new(Offset::new(7), Offset::new(10)),
            ],
            0,
        );
        assert!(sel.contains_offset(Offset::new(1)));
        assert!(!sel.contains_offset(Offset::new(5)));
        assert!(sel.contains_offset(Offset::new(8)));
    }

    #[test]
    fn iter_all_ranges() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            0,
        );
        let offsets: Vec<_> = sel.iter().map(|r| r.head().get()).collect();
        assert_eq!(offsets, vec![0, 5]);
    }

    // ── Conversion ───────────────────────────────────────────────────

    #[test]
    fn from_selection() {
        let sel = Selection::single(SelectionRange::new(Offset::new(3), Offset::new(7)));
        let multi = Selections::from_selection(sel);
        assert!(multi.is_single());
        assert_eq!(multi.primary().anchor(), Offset::new(3));
        assert_eq!(multi.primary().head(), Offset::new(7));
    }

    #[test]
    fn to_selection() {
        let multi = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            1,
        );
        let sel = multi.to_selection();
        assert_eq!(sel.primary().head(), Offset::new(5));
    }

    #[test]
    fn from_trait() {
        let sel = Selection::cursor(Offset::new(5));
        let multi: Selections = sel.into();
        assert_eq!(multi.primary().head(), Offset::new(5));
    }

    // ── Transform ────────────────────────────────────────────────────

    #[test]
    fn transform_all() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(10)),
            ],
            1,
        );
        let moved =
            sel.transform(|r| SelectionRange::insert_cursor(Offset::new(r.head().get() + 1)));
        assert_eq!(moved.len(), 3);
        assert_eq!(moved.primary_index(), 1);
        let offsets: Vec<_> = moved.iter().map(|r| r.head().get()).collect();
        assert_eq!(offsets, vec![1, 6, 11]);
    }

    // ── Normalize ────────────────────────────────────────────────────

    #[test]
    fn normalize_single_is_identity() {
        let sel = Selections::cursor(Offset::new(5));
        let normalized = sel.clone().normalize();
        assert_eq!(normalized, sel);
    }

    #[test]
    fn normalize_sorts_by_start() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            0, // primary at offset 10
        );
        let normalized = sel.normalize();
        let offsets: Vec<_> = normalized.iter().map(|r| r.head().get()).collect();
        assert_eq!(offsets, vec![0, 5, 10]);
        // Primary should track: was at offset 10, now at index 2
        assert_eq!(normalized.primary_index(), 2);
        assert_eq!(normalized.primary().head(), Offset::new(10));
    }

    #[test]
    fn normalize_merges_overlapping() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(3), Offset::new(8)),
            ],
            0,
        );
        let normalized = sel.normalize();
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized.primary().start(), Offset::new(0));
        assert_eq!(normalized.primary().end(), Offset::new(8));
    }

    #[test]
    fn normalize_merges_adjacent() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
            ],
            1,
        );
        let normalized = sel.normalize();
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized.primary().start(), Offset::new(0));
        assert_eq!(normalized.primary().end(), Offset::new(10));
    }

    #[test]
    fn normalize_keeps_non_overlapping() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)),
                SelectionRange::new(Offset::new(7), Offset::new(10)),
            ],
            0,
        );
        let normalized = sel.normalize();
        assert_eq!(normalized.len(), 2);
    }

    #[test]
    fn normalize_primary_tracks_through_merge() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(3), Offset::new(8)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
            ],
            2, // primary is the non-overlapping third range
        );
        let normalized = sel.normalize();
        assert_eq!(normalized.len(), 2); // first two merged
        assert_eq!(normalized.primary_index(), 1); // third moved to index 1
        assert_eq!(normalized.primary().start(), Offset::new(20));
    }

    #[test]
    fn normalize_deduplicates() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            2,
        );
        let normalized = sel.normalize();
        assert_eq!(normalized.len(), 1);
    }

    // ── Map through changeset ────────────────────────────────────────

    #[test]
    fn map_through_insert() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            0,
        );
        // Insert "XX" at offset 2: positions after 2 shift by 2
        let cs = ChangeSet::from_insert(10, 2, "XX");
        let mapped = sel.map_through(&cs, Assoc::After);
        let offsets: Vec<_> = mapped.iter().map(|r| r.head().get()).collect();
        assert_eq!(offsets, vec![0, 7]); // 0 unchanged, 5+2=7
    }

    #[test]
    fn map_through_delete() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(8)),
            ],
            0,
        );
        // Delete bytes 2..6: positions in [2..6] collapse to 2, positions after shift
        let cs = ChangeSet::from_delete(10, 2, 6);
        let mapped = sel.map_through(&cs, Assoc::Before);
        let offsets: Vec<_> = mapped.iter().map(|r| r.head().get()).collect();
        assert_eq!(offsets, vec![0, 2, 4]); // 0, 5->2(deleted), 8->4(shifted)
    }

    // ── map_through_batched ────────────────────────────────────────

    #[test]
    fn map_through_batched_matches_map_through() {
        // Multiple ranges + insert changeset: batched must match non-batched.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)),
                SelectionRange::new(Offset::new(5), Offset::new(8)),
                SelectionRange::new(Offset::new(10), Offset::new(12)),
            ],
            1,
        );
        let cs = ChangeSet::from_insert(15, 4, "XYZ");
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "batched vs map_through mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_delete() {
        // Multiple ranges + delete changeset: batched must match non-batched.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(8)),
            ],
            0,
        );
        let cs = ChangeSet::from_delete(10, 2, 6);
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "batched vs map_through mismatch for delete {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_single_fallback() {
        // Single cursor: batched should produce the same result as map_through.
        let sel = Selections::single(SelectionRange::insert_cursor(Offset::new(3)));
        let cs = ChangeSet::from_insert(10, 2, "AB");
        let expected = sel.map_through(&cs, Assoc::After);
        let batched = sel.map_through_batched(&cs, Assoc::After);
        assert_eq!(batched, expected);
    }

    #[test]
    fn map_through_batched_preserves_primary() {
        // 3 ranges with primary=1: verify primary_index is preserved.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(2)),
                SelectionRange::new(Offset::new(5), Offset::new(7)),
                SelectionRange::new(Offset::new(10), Offset::new(12)),
            ],
            1,
        );
        let cs = ChangeSet::from_insert(15, 3, "Z");
        let batched = sel.map_through_batched(&cs, Assoc::Before);
        assert_eq!(batched.primary_index(), 1);
        // Also verify it matches map_through.
        let expected = sel.map_through(&cs, Assoc::Before);
        assert_eq!(batched, expected);
    }

    #[test]
    fn map_through_batched_empty_changeset() {
        // Identity changeset: positions should not change.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)),
                SelectionRange::new(Offset::new(7), Offset::new(10)),
            ],
            0,
        );
        let cs = ChangeSet::identity(15);
        let batched = sel.map_through_batched(&cs, Assoc::Before);
        assert_eq!(batched, sel);
    }

    // ── map_through_batched edge cases (validation agent) ──────────

    #[test]
    fn map_through_batched_many_cursors_complex_changeset() {
        // 25 cursors spread across a 200-byte document.
        // Changeset: insert at 10, delete 30..40, replace 60..65 with "ABCDE", insert at 100.
        let doc_len = 200;
        let ranges: Vec<_> = (0..25)
            .map(|i| {
                let pos = i * 8;
                SelectionRange::new(Offset::new(pos), Offset::new(pos + 3))
            })
            .collect();
        let sel = Selections::from_vec(ranges, 12);

        let cs = ChangeSet::from_changes(
            doc_len,
            vec![
                (10, 10, Some("ZZ")),    // insert 2 bytes at 10
                (30, 40, None),          // delete 10 bytes at 30..40
                (60, 65, Some("ABCDE")), // replace 5 with 5 (net 0)
                (100, 100, Some("W")),   // insert 1 byte at 100
            ],
        );

        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "25-cursor complex changeset mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_backward_ranges() {
        // Backward ranges: anchor > head (selection goes right-to-left).
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(10), Offset::new(5)), // backward
                SelectionRange::new(Offset::new(20), Offset::new(15)), // backward
                SelectionRange::new(Offset::new(30), Offset::new(25)), // backward
            ],
            1,
        );
        let cs = ChangeSet::from_insert(50, 12, "XX");
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(batched, expected, "backward ranges mismatch for {assoc:?}");
        }
    }

    #[test]
    fn map_through_batched_adjacent_ranges() {
        // Adjacent ranges: end of one == start of next.
        // Test that batched handles positions at boundaries correctly.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
            ],
            0,
        );
        // Insert right at the boundary (offset 5).
        let cs = ChangeSet::from_insert(20, 5, "QQ");
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(batched, expected, "adjacent ranges mismatch for {assoc:?}");
        }
    }

    #[test]
    fn map_through_batched_interleaved_anchor_head() {
        // Ranges where one range's anchor falls between another's anchor and head.
        // Range A: anchor=5, head=20 (forward, start=5, end=20)
        // Range B: anchor=12, head=8 (backward, start=8, end=12)
        // Range C: anchor=18, head=25 (forward, start=18, end=25)
        // B's anchor(12) is between A's anchor(5) and head(20).
        // C's anchor(18) is also between A's anchor(5) and head(20).
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(5), Offset::new(20)),
                SelectionRange::new(Offset::new(12), Offset::new(8)),
                SelectionRange::new(Offset::new(18), Offset::new(25)),
            ],
            0,
        );
        let cs = ChangeSet::from_delete(30, 10, 15);
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "interleaved anchor/head mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_equivalence_10_inputs() {
        // 10 different configurations: verify exact equivalence with map_through.
        let configs: Vec<(
            usize,
            Vec<(usize, usize)>,
            usize,
            Vec<(usize, usize, Option<&str>)>,
        )> = vec![
            // (doc_len, ranges as (anchor, head), primary, changes)
            (
                50,
                vec![(0, 0), (10, 10), (20, 20)],
                1,
                vec![(5, 5, Some("A"))],
            ),
            (
                100,
                vec![(0, 5), (50, 55), (90, 95)],
                2,
                vec![(10, 20, None)],
            ),
            (80, vec![(10, 5), (30, 25)], 0, vec![(15, 15, Some("XY"))]),
            (60, vec![(0, 3), (3, 6), (6, 9)], 1, vec![(3, 3, Some("M"))]),
            (
                100,
                vec![(0, 10), (20, 30), (40, 50), (60, 70)],
                3,
                vec![(25, 35, Some("Z"))],
            ),
            (40, vec![(0, 5), (10, 15)], 0, vec![(0, 5, None)]),
            (
                200,
                vec![(0, 1), (50, 51), (100, 101), (150, 151)],
                2,
                vec![(75, 80, Some("HELLO"))],
            ),
            (
                30,
                vec![(0, 10), (15, 25)],
                0,
                vec![(5, 5, Some("A")), (20, 20, Some("B"))],
            ),
            (
                100,
                vec![(10, 5), (30, 20), (50, 45)],
                1,
                vec![(0, 3, None), (40, 42, None)],
            ),
            (
                150,
                vec![(0, 10), (20, 30), (40, 50), (60, 70), (80, 90)],
                2,
                vec![(15, 25, Some("R")), (55, 65, None)],
            ),
        ];

        for (i, (doc_len, ranges, primary, changes)) in configs.iter().enumerate() {
            let sel = Selections::from_vec(
                ranges
                    .iter()
                    .map(|&(a, h)| SelectionRange::new(Offset::new(a), Offset::new(h)))
                    .collect(),
                *primary,
            );
            let cs = ChangeSet::from_changes(*doc_len, changes.iter().copied());
            for assoc in [Assoc::Before, Assoc::After] {
                let expected = sel.map_through(&cs, assoc);
                let batched = sel.map_through_batched(&cs, assoc);
                assert_eq!(
                    batched, expected,
                    "equivalence test {i} failed for {assoc:?}"
                );
            }
        }
    }

    #[test]
    fn map_through_batched_stress_100_cursors() {
        // 100 cursors, multi-op changeset, verify equivalence.
        let doc_len = 2000;
        let ranges: Vec<_> = (0..100)
            .map(|i| {
                let anchor = i * 20;
                let head = anchor + 5;
                SelectionRange::new(Offset::new(anchor), Offset::new(head))
            })
            .collect();
        let sel = Selections::from_vec(ranges, 50);

        // Multi-op changeset: 10 operations spread across the document.
        let cs = ChangeSet::from_changes(
            doc_len,
            vec![
                (50, 55, Some("AA")),     // replace 5 with 2
                (100, 100, Some("BBB")),  // insert 3
                (200, 210, None),         // delete 10
                (300, 305, Some("C")),    // replace 5 with 1
                (400, 400, Some("DD")),   // insert 2
                (500, 520, None),         // delete 20
                (600, 600, Some("EEEE")), // insert 4
                (700, 710, Some("F")),    // replace 10 with 1
                (800, 800, Some("GG")),   // insert 2
                (900, 950, None),         // delete 50
            ],
        );

        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "100-cursor stress test mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_duplicate_positions() {
        // Multiple ranges share the same positions.
        // E.g., two collapsed cursors at the same offset.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(20)),
            ],
            1,
        );
        let cs = ChangeSet::from_insert(30, 10, "X");
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "duplicate positions mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_all_positions_in_deleted_region() {
        // All cursors fall inside a deleted region.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(5), Offset::new(8)),
                SelectionRange::new(Offset::new(10), Offset::new(12)),
                SelectionRange::new(Offset::new(15), Offset::new(18)),
            ],
            2,
        );
        let cs = ChangeSet::from_delete(30, 3, 20);
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "all-in-deleted-region mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_mixed_forward_backward() {
        // Mix of forward and backward ranges, with various primary indices.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)), // forward
                SelectionRange::new(Offset::new(15), Offset::new(10)), // backward
                SelectionRange::new(Offset::new(20), Offset::new(25)), // forward
                SelectionRange::new(Offset::new(35), Offset::new(30)), // backward
            ],
            2,
        );
        let cs = ChangeSet::from_changes(
            50,
            vec![
                (5, 5, Some("AB")), // insert at boundary of first range
                (15, 20, None),     // delete spanning boundary of ranges 2 and 3
            ],
        );
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "mixed forward/backward mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_replace_changeset() {
        // Replace changeset (simultaneous delete + insert).
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)),
                SelectionRange::new(Offset::new(5), Offset::new(8)),
                SelectionRange::new(Offset::new(12), Offset::new(15)),
            ],
            1,
        );
        let cs = ChangeSet::from_replace(20, 4, 9, "REPLACEMENT");
        for assoc in [Assoc::Before, Assoc::After] {
            let expected = sel.map_through(&cs, assoc);
            let batched = sel.map_through_batched(&cs, assoc);
            assert_eq!(
                batched, expected,
                "replace changeset mismatch for {assoc:?}"
            );
        }
    }

    #[test]
    fn map_through_batched_primary_index_all_positions() {
        // Verify primary_index is preserved for every possible primary value.
        let n = 5;
        let ranges: Vec<_> = (0..n)
            .map(|i| SelectionRange::new(Offset::new(i * 20), Offset::new(i * 20 + 10)))
            .collect();
        let cs = ChangeSet::from_insert(200, 50, "INSERTED");

        for primary in 0..n {
            let sel = Selections::from_vec(ranges.clone(), primary);
            let expected = sel.map_through(&cs, Assoc::After);
            let batched = sel.map_through_batched(&cs, Assoc::After);
            assert_eq!(
                batched.primary_index(),
                expected.primary_index(),
                "primary_index mismatch for primary={primary}"
            );
            assert_eq!(batched, expected, "full mismatch for primary={primary}");
        }
    }

    // ── map_through_batched_directed ────────────────────────────────

    #[test]
    fn map_through_batched_directed_forward_range() {
        let sel = Selections::from_vec(
            vec![SelectionRange::new(Offset::new(10), Offset::new(20))],
            0,
        );
        let cs = ChangeSet::from_insert(30, 10, "XX");
        let mapped = sel.map_through_batched_directed(&cs);
        assert_eq!(mapped.primary().anchor().get(), 12);
        assert_eq!(mapped.primary().head().get(), 22);
    }

    #[test]
    fn map_through_batched_directed_backward_range() {
        let sel = Selections::from_vec(
            vec![SelectionRange::new(Offset::new(20), Offset::new(10))],
            0,
        );
        let cs = ChangeSet::from_insert(30, 10, "XX");
        let mapped = sel.map_through_batched_directed(&cs);
        assert_eq!(mapped.primary().anchor().get(), 22);
        assert_eq!(mapped.primary().head().get(), 12);
    }

    #[test]
    fn map_through_batched_directed_collapsed_cursor() {
        let sel = Selections::from_vec(vec![SelectionRange::insert_cursor(Offset::new(10))], 0);
        let cs = ChangeSet::from_insert(30, 10, "XX");
        let mapped = sel.map_through_batched_directed(&cs);
        assert_eq!(mapped.primary().anchor().get(), 12);
        assert_eq!(mapped.primary().head().get(), 12);
    }

    #[test]
    fn map_through_batched_directed_multi_cursor() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(10), Offset::new(20)),
                SelectionRange::new(Offset::new(40), Offset::new(50)),
                SelectionRange::new(Offset::new(60), Offset::new(70)),
            ],
            0,
        );
        let cs = ChangeSet::from_insert(100, 30, "XXX");
        let mapped = sel.map_through_batched_directed(&cs);
        assert_eq!(mapped.iter().nth(0).unwrap().anchor().get(), 10);
        assert_eq!(mapped.iter().nth(0).unwrap().head().get(), 20);
        assert_eq!(mapped.iter().nth(1).unwrap().anchor().get(), 43);
        assert_eq!(mapped.iter().nth(1).unwrap().head().get(), 53);
        assert_eq!(mapped.iter().nth(2).unwrap().anchor().get(), 63);
        assert_eq!(mapped.iter().nth(2).unwrap().head().get(), 73);
    }

    #[test]
    fn map_through_batched_directed_matches_non_directed_for_non_boundary() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
            ],
            0,
        );
        let cs = ChangeSet::from_insert(20, 7, "YY");
        let directed = sel.map_through_batched_directed(&cs);
        let uniform = sel.map_through_batched(&cs, Assoc::After);
        assert_eq!(directed, uniform);
    }

    #[test]
    fn map_through_batched_directed_preserves_primary() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(10)),
                SelectionRange::insert_cursor(Offset::new(15)),
            ],
            2,
        );
        let cs = ChangeSet::from_insert(20, 3, "A");
        let mapped = sel.map_through_batched_directed(&cs);
        assert_eq!(mapped.primary_index(), 2);
    }

    #[test]
    fn map_through_batched_directed_delete() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(5), Offset::new(10)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
            ],
            0,
        );
        let cs = ChangeSet::from_delete(30, 8, 15);
        let mapped = sel.map_through_batched_directed(&cs);
        assert_eq!(mapped.iter().nth(0).unwrap().anchor().get(), 5);
        assert_eq!(mapped.iter().nth(0).unwrap().head().get(), 8);
        assert_eq!(mapped.iter().nth(1).unwrap().anchor().get(), 13);
        assert_eq!(mapped.iter().nth(1).unwrap().head().get(), 18);
    }

    #[test]
    fn map_through_batched_directed_insert_at_head_boundary() {
        // Forward range anchor=10, head=20. Insert "XX" at position 20 (head).
        // Head (end-of-range) gets Before → stays at 20 (not absorbed).
        // Anchor (start) is at 10, unaffected.
        let sel = Selections::from_vec(
            vec![SelectionRange::new(Offset::new(10), Offset::new(20))],
            0,
        );
        let cs = ChangeSet::from_insert(30, 20, "XX");
        let mapped = sel.map_through_batched_directed(&cs);
        assert_eq!(mapped.primary().anchor().get(), 10);
        assert_eq!(mapped.primary().head().get(), 20); // Before: stays put
    }

    #[test]
    fn map_through_batched_directed_stress_100_cursors() {
        let ranges: Vec<SelectionRange> = (0..100)
            .map(|i| {
                let start = i * 20;
                SelectionRange::new(Offset::new(start), Offset::new(start + 10))
            })
            .collect();
        let sel = Selections::from_vec(ranges, 50);

        let cs = ChangeSet::from_insert(2000, 500, "XXXXX");
        let mapped = sel.map_through_batched_directed(&cs);

        assert_eq!(mapped.len(), 100);
        assert_eq!(mapped.primary_index(), 50);

        // Ranges before position 500 should be unchanged
        for i in 0..25 {
            let r = mapped.iter().nth(i).unwrap();
            assert_eq!(r.anchor().get(), i * 20, "range {i} anchor");
            assert_eq!(r.head().get(), i * 20 + 10, "range {i} head");
        }

        // Range 25: anchor=500 (at insert, gets After → 505),
        // head=510 (after insert → 515)
        let r25 = mapped.iter().nth(25).unwrap();
        assert_eq!(r25.anchor().get(), 505);
        assert_eq!(r25.head().get(), 515);

        // Ranges after position 500+10 should be shifted by 5
        for i in 26..100 {
            let r = mapped.iter().nth(i).unwrap();
            assert_eq!(r.anchor().get(), i * 20 + 5, "range {i} anchor");
            assert_eq!(r.head().get(), i * 20 + 10 + 5, "range {i} head");
        }
    }

    // ── Mutators ─────────────────────────────────────────────────────

    #[test]
    fn push_range() {
        let mut sel = Selections::cursor(Offset::new(0));
        sel.push(SelectionRange::insert_cursor(Offset::new(5)));
        assert_eq!(sel.len(), 2);
        assert_eq!(sel.primary_index(), 0);
    }

    #[test]
    fn set_primary_index() {
        let mut sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            0,
        );
        sel.set_primary_index(1);
        assert_eq!(sel.primary().head(), Offset::new(5));
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn set_primary_index_oob_panics() {
        let mut sel = Selections::cursor(Offset::new(0));
        sel.set_primary_index(1);
    }

    #[test]
    fn into_single_keeps_primary() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(10)),
            ],
            2,
        );
        let single = sel.into_single();
        assert!(single.is_single());
        assert_eq!(single.primary().head(), Offset::new(10));
    }

    // ── Primary mutation ─────────────────────────────────────────────

    #[test]
    fn primary_mut_modifies_in_place() {
        let mut sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(0)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            1,
        );
        *sel.primary_mut() = SelectionRange::new(Offset::new(5), Offset::new(10));
        assert!(!sel.primary().is_collapsed());
        assert_eq!(sel.primary().head(), Offset::new(10));
    }

    // ── Default ──────────────────────────────────────────────────────

    #[test]
    fn default_is_cursor_at_zero() {
        let sel = Selections::default();
        assert!(sel.is_single());
        assert!(sel.primary().is_collapsed());
        assert_eq!(sel.primary().head(), Offset::ZERO);
    }

    // ── SmallVec optimization ────────────────────────────────────────

    #[test]
    fn single_cursor_is_inline() {
        let sel = Selections::cursor(Offset::new(5));
        // SmallVec<[SelectionRange; 1]> stores 1 element inline
        assert!(sel.is_single());
        // Verify the SmallVec is using inline storage (not heap)
        assert!(!sel.ranges.spilled());
    }

    #[test]
    fn two_cursors_spill_to_heap() {
        let mut sel = Selections::cursor(Offset::new(0));
        sel.push(SelectionRange::insert_cursor(Offset::new(5)));
        // SmallVec<[T; 1]> spills at 2 elements
        assert!(sel.ranges.spilled());
    }

    // ── transform_iter ──────────────────────────────────────────────

    #[test]
    fn transform_iter_one_to_many() {
        // Single range producing 2 ranges
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(10)));
        let result = sel.transform_iter(|_r| {
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)),
                SelectionRange::new(Offset::new(7), Offset::new(10)),
            ]
        });
        assert_eq!(result.len(), 2);
        assert_eq!(result.primary_index(), 0);
    }

    #[test]
    fn transform_iter_some_empty() {
        // 2 ranges: first produces nothing, second produces 2
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
            ],
            0, // primary is the first (which will produce nothing)
        );
        let result = sel.transform_iter(|r| {
            if r.start() == Offset::new(0) {
                vec![] // first produces nothing
            } else {
                vec![
                    SelectionRange::new(Offset::new(10), Offset::new(12)),
                    SelectionRange::new(Offset::new(13), Offset::new(15)),
                ]
            }
        });
        assert_eq!(result.len(), 2);
        // Primary was index 0 which produced nothing; nearest surviving
        // range by start position is the one at offset 10 (index 0 after flatten).
        assert_eq!(result.primary().start(), Offset::new(10));
    }

    #[test]
    fn transform_iter_all_empty_returns_cursor() {
        // All ranges produce nothing — must return valid Selections
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(5), Offset::new(10)),
                SelectionRange::new(Offset::new(15), Offset::new(20)),
            ],
            1, // primary at offset 15..20
        );
        let result = sel.transform_iter(|_r| Vec::<SelectionRange>::new());
        assert_eq!(result.len(), 1);
        assert!(result.primary().is_collapsed());
        // Cursor at old primary's start position
        assert_eq!(result.primary().head(), Offset::new(15));
    }

    #[test]
    fn transform_iter_normalizes_output() {
        // Produce overlapping ranges — verify they get merged
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(20)));
        let result = sel.transform_iter(|_r| {
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(8)),
                SelectionRange::new(Offset::new(5), Offset::new(15)),
            ]
        });
        // Overlapping [0..8] and [5..15] should merge to [0..15]
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(0));
        assert_eq!(result.primary().end(), Offset::new(15));
    }

    #[test]
    fn transform_iter_primary_fallback_nearest() {
        // Primary produces nothing, other ranges survive.
        // Verify primary falls to nearest by start position.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)),
                SelectionRange::new(Offset::new(10), Offset::new(13)), // primary
                SelectionRange::new(Offset::new(20), Offset::new(23)),
            ],
            1, // primary at 10..13 (will produce nothing)
        );
        let result = sel.transform_iter(|r| {
            if r.start() == Offset::new(10) {
                vec![] // primary produces nothing
            } else {
                vec![r] // others survive unchanged
            }
        });
        assert_eq!(result.len(), 2);
        // Old primary started at 10. Nearest surviving by start:
        // index 0 starts at 0 (distance 10), index 1 starts at 20 (distance 10).
        // Tie broken by lower index.
        assert_eq!(result.primary_index(), 0);
    }

    // ── transform_iter edge cases (validation agent) ────────────────

    #[test]
    fn transform_iter_large_output_normalizes() {
        // Each of 3 input ranges produces 40 non-overlapping ranges = 120 total.
        // Verify normalization handles a large count correctly.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(1)),
                SelectionRange::new(Offset::new(1000), Offset::new(1001)),
                SelectionRange::new(Offset::new(2000), Offset::new(2001)),
            ],
            1,
        );
        let result = sel.transform_iter(|r| {
            let base = r.start().get();
            (0..40)
                .map(move |i| {
                    let s = base + i * 10;
                    SelectionRange::new(Offset::new(s), Offset::new(s + 5))
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(result.len(), 120);
        // Primary was input index 1 (base 1000). First output from that group
        // is at offset 1000, which should be the new primary.
        assert_eq!(result.primary().start(), Offset::new(1000));
        // Verify sorted order: each range's start < next range's start.
        let ranges: Vec<_> = result.iter().collect();
        for w in ranges.windows(2) {
            assert!(
                w[0].start() < w[1].start(),
                "ranges not sorted: {:?} >= {:?}",
                w[0].start(),
                w[1].start()
            );
        }
    }

    #[test]
    fn transform_iter_reverse_order_output() {
        // Closure returns ranges in reverse order; normalize must fix ordering.
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(100)));
        let result = sel.transform_iter(|_r| {
            vec![
                SelectionRange::new(Offset::new(80), Offset::new(90)),
                SelectionRange::new(Offset::new(40), Offset::new(50)),
                SelectionRange::new(Offset::new(10), Offset::new(20)),
            ]
        });
        assert_eq!(result.len(), 3);
        // After normalize, sorted by start ascending.
        let starts: Vec<_> = result.iter().map(|r| r.start().get()).collect();
        assert_eq!(starts, vec![10, 40, 80]);
        // Primary should be the first produced range (originally at offset 80),
        // which after sorting lands at index 2.
        assert_eq!(result.primary().start(), Offset::new(80));
    }

    #[test]
    fn transform_iter_backward_ranges_preserved() {
        // Backward range: anchor > head. normalize merges by start/end so
        // direction is lost in merged output, but a lone backward range
        // that doesn't overlap should survive.
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(10)));
        let result = sel.transform_iter(|_r| {
            // Return a single backward range (anchor=20, head=10 => start=10, end=20)
            vec![SelectionRange::new(Offset::new(20), Offset::new(10))]
        });
        assert_eq!(result.len(), 1);
        // Single range is not normalized (normalize returns self for len<=1),
        // so backward direction is preserved.
        assert_eq!(result.primary().anchor(), Offset::new(20));
        assert_eq!(result.primary().head(), Offset::new(10));
        assert!(!result.primary().is_forward());
    }

    #[test]
    fn transform_iter_one_to_one_like_transform() {
        // Each input produces exactly 1 output — should behave like transform.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
            ],
            2, // primary at index 2
        );
        let result = sel.transform_iter(|r| {
            // Shift each range right by 1
            vec![SelectionRange::new(
                Offset::new(r.anchor().get() + 1),
                Offset::new(r.head().get() + 1),
            )]
        });
        assert_eq!(result.len(), 3);
        // Primary should track: input primary at index 2 produces 1 output,
        // which becomes new primary.
        assert_eq!(result.primary().start(), Offset::new(21));
        assert_eq!(result.primary().end(), Offset::new(26));
    }

    #[test]
    fn transform_iter_interleaved_overlapping() {
        // Two input ranges produce outputs that interleave and overlap
        // after flattening. Verify normalization merges them.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(100), Offset::new(105)),
            ],
            0,
        );
        let result = sel.transform_iter(|r| {
            if r.start() == Offset::new(0) {
                // First input produces ranges at 10..20, 30..40
                vec![
                    SelectionRange::new(Offset::new(10), Offset::new(20)),
                    SelectionRange::new(Offset::new(30), Offset::new(40)),
                ]
            } else {
                // Second input produces ranges at 15..35 (overlaps both above)
                vec![SelectionRange::new(Offset::new(15), Offset::new(35))]
            }
        });
        // 10..20 overlaps 15..35 => merged to 10..35
        // 10..35 overlaps 30..40 => merged to 10..40
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(10));
        assert_eq!(result.primary().end(), Offset::new(40));
    }

    #[test]
    fn transform_iter_primary_first_of_many() {
        // Primary produces 3 ranges. The first one should become new primary,
        // not the second or third.
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(10)));
        let result = sel.transform_iter(|_r| {
            vec![
                SelectionRange::new(Offset::new(50), Offset::new(60)),
                SelectionRange::new(Offset::new(20), Offset::new(30)),
                SelectionRange::new(Offset::new(70), Offset::new(80)),
            ]
        });
        assert_eq!(result.len(), 3);
        // First produced range was 50..60. After sorting: 20..30, 50..60, 70..80
        // Primary tracks to 50..60 which is now index 1.
        assert_eq!(result.primary().start(), Offset::new(50));
        assert_eq!(result.primary_index(), 1);
    }

    #[test]
    fn transform_iter_primary_fallback_nearest_higher() {
        // Primary is at low offset and produces nothing.
        // Only surviving ranges are at higher offsets.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)), // primary
                SelectionRange::new(Offset::new(100), Offset::new(103)),
                SelectionRange::new(Offset::new(200), Offset::new(203)),
            ],
            0,
        );
        let result = sel.transform_iter(|r| {
            if r.start() == Offset::new(0) {
                vec![]
            } else {
                vec![r]
            }
        });
        assert_eq!(result.len(), 2);
        // Old primary at 0. Distance to 100 = 100, distance to 200 = 200.
        // Nearest is index 0 (which is 100..103 after flatten).
        assert_eq!(result.primary_index(), 0);
        assert_eq!(result.primary().start(), Offset::new(100));
    }

    #[test]
    fn transform_iter_invariant_always_valid() {
        // Property-like test: throw many different inputs at transform_iter
        // and verify the invariant: len >= 1 && primary_index < len.
        let offsets: Vec<usize> = vec![0, 1, 5, 10, 50, 100, 500, 1000];

        // Test 1: Single range, closure returns 0..N ranges.
        for n in 0..=10 {
            let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(10)));
            let result = sel.transform_iter(|_r| {
                (0..n)
                    .map(|i| SelectionRange::new(Offset::new(i * 20), Offset::new(i * 20 + 5)))
                    .collect::<Vec<_>>()
            });
            assert!(
                !result.is_empty(),
                "invariant violated: empty after producing {n} ranges"
            );
            assert!(
                result.primary_index() < result.len(),
                "invariant violated: primary_index {} >= len {} (n={n})",
                result.primary_index(),
                result.len()
            );
        }

        // Test 2: Multiple input ranges, various primary positions,
        // some produce nothing.
        for &primary in &[0usize, 1, 2] {
            for skip_idx in 0..3 {
                let sel = Selections::from_vec(
                    vec![
                        SelectionRange::new(Offset::new(0), Offset::new(5)),
                        SelectionRange::new(Offset::new(10), Offset::new(15)),
                        SelectionRange::new(Offset::new(20), Offset::new(25)),
                    ],
                    primary,
                );
                let result = sel.transform_iter(|r| {
                    let idx = r.start().get() / 10;
                    if idx == skip_idx {
                        vec![]
                    } else {
                        vec![r]
                    }
                });
                assert!(
                    !result.is_empty(),
                    "invariant violated: empty (primary={primary}, skip={skip_idx})"
                );
                assert!(
                    result.primary_index() < result.len(),
                    "invariant violated: primary_index {} >= len {} (primary={primary}, skip={skip_idx})",
                    result.primary_index(),
                    result.len()
                );
            }
        }

        // Test 3: All produce nothing for various primary positions.
        for &primary in &[0usize, 1, 2] {
            let sel = Selections::from_vec(
                vec![
                    SelectionRange::new(Offset::new(0), Offset::new(5)),
                    SelectionRange::new(Offset::new(10), Offset::new(15)),
                    SelectionRange::new(Offset::new(20), Offset::new(25)),
                ],
                primary,
            );
            let result = sel.transform_iter(|_r| Vec::<SelectionRange>::new());
            assert_eq!(result.len(), 1, "all-empty should produce exactly 1 cursor");
            assert_eq!(result.primary_index(), 0);
            assert!(result.primary().is_collapsed());
        }

        // Test 4: Many-to-many with overlapping outputs.
        for &n_inputs in &[1, 5, 10] {
            let ranges: Vec<_> = (0..n_inputs)
                .map(|i| SelectionRange::new(Offset::new(i * 100), Offset::new(i * 100 + 50)))
                .collect();
            let sel = Selections::from_vec(ranges, n_inputs / 2);
            let result = sel.transform_iter(|r| {
                let base = r.start().get();
                // Produce overlapping ranges
                vec![
                    SelectionRange::new(Offset::new(base), Offset::new(base + 30)),
                    SelectionRange::new(Offset::new(base + 20), Offset::new(base + 60)),
                ]
            });
            assert!(
                !result.is_empty(),
                "invariant violated: empty (n_inputs={n_inputs})"
            );
            assert!(
                result.primary_index() < result.len(),
                "invariant violated: primary_index {} >= len {} (n_inputs={n_inputs})",
                result.primary_index(),
                result.len()
            );
            // Verify sorted after normalize.
            let ranges_vec: Vec<_> = result.iter().collect();
            for w in ranges_vec.windows(2) {
                assert!(w[0].start() <= w[1].start());
            }
        }

        // Test 5: Various offsets, single input, single output at each offset.
        for &off in &offsets {
            let sel =
                Selections::single(SelectionRange::new(Offset::new(off), Offset::new(off + 1)));
            let result = sel.transform_iter(|r| vec![r]);
            assert_eq!(result.len(), 1);
            assert_eq!(result.primary_index(), 0);
            assert_eq!(result.primary().start(), Offset::new(off));
        }
    }

    #[test]
    fn transform_iter_all_empty_preserves_each_primary_start() {
        // When all produce nothing, cursor should be at old primary's start.
        // Test with different primary indices.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(10), Offset::new(20)),
                SelectionRange::new(Offset::new(30), Offset::new(40)),
                SelectionRange::new(Offset::new(50), Offset::new(60)),
            ],
            0,
        );
        let result = sel.transform_iter(|_r| Vec::<SelectionRange>::new());
        assert_eq!(result.primary().head(), Offset::new(10));

        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(10), Offset::new(20)),
                SelectionRange::new(Offset::new(30), Offset::new(40)),
                SelectionRange::new(Offset::new(50), Offset::new(60)),
            ],
            2,
        );
        let result = sel.transform_iter(|_r| Vec::<SelectionRange>::new());
        assert_eq!(result.primary().head(), Offset::new(50));
    }

    #[test]
    fn transform_iter_collapsed_cursor_passthrough() {
        // A collapsed cursor (insert_cursor) passed through should work.
        let sel = Selections::single(SelectionRange::insert_cursor(Offset::new(42)));
        let result = sel.transform_iter(|r| vec![r]);
        assert_eq!(result.len(), 1);
        assert!(result.primary().is_collapsed());
        assert_eq!(result.primary().head(), Offset::new(42));
    }

    #[test]
    fn transform_iter_duplicate_ranges_merged() {
        // Producing duplicate ranges should merge them down.
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(10)));
        let result = sel.transform_iter(|_r| {
            vec![
                SelectionRange::new(Offset::new(5), Offset::new(15)),
                SelectionRange::new(Offset::new(5), Offset::new(15)),
                SelectionRange::new(Offset::new(5), Offset::new(15)),
            ]
        });
        // All three are identical, so they merge to 1.
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(5));
        assert_eq!(result.primary().end(), Offset::new(15));
    }

    #[test]
    fn transform_iter_adjacent_ranges_stay_separate() {
        // Adjacent but non-overlapping ranges should NOT merge.
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(30)));
        let result = sel.transform_iter(|_r| {
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(10)),
                SelectionRange::new(Offset::new(11), Offset::new(20)),
                SelectionRange::new(Offset::new(21), Offset::new(30)),
            ]
        });
        assert_eq!(result.len(), 3);
    }

    // ── contains ──────────────────────────────────────────────────────

    #[test]
    fn contains_superset() {
        let outer = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(20)));
        let inner = Selections::single(SelectionRange::new(Offset::new(5), Offset::new(10)));
        assert!(outer.contains(&inner));
    }

    #[test]
    fn contains_exact_match() {
        let a = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(10)));
        assert!(a.contains(&a));
    }

    #[test]
    fn contains_partial_overlap_fails() {
        let a = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(8)));
        let b = Selections::single(SelectionRange::new(Offset::new(5), Offset::new(12)));
        assert!(!a.contains(&b));
    }

    #[test]
    fn contains_disjoint_fails() {
        let a = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(5)));
        let b = Selections::single(SelectionRange::new(Offset::new(10), Offset::new(15)));
        assert!(!a.contains(&b));
    }

    #[test]
    fn contains_multi_range() {
        let outer = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
            ],
            0,
        );
        let inner = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(1), Offset::new(3)),
                SelectionRange::new(Offset::new(11), Offset::new(14)),
            ],
            0,
        );
        assert!(outer.contains(&inner));
    }

    #[test]
    fn contains_multi_range_one_uncovered() {
        let outer = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
            ],
            0,
        );
        let inner = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(1), Offset::new(3)),
                SelectionRange::new(Offset::new(7), Offset::new(9)),
            ],
            0,
        );
        assert!(!outer.contains(&inner));
    }

    #[test]
    fn contains_single_covers_multiple() {
        let outer = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(100)));
        let inner = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(10), Offset::new(20)),
                SelectionRange::new(Offset::new(30), Offset::new(40)),
                SelectionRange::new(Offset::new(50), Offset::new(60)),
            ],
            0,
        );
        assert!(outer.contains(&inner));
    }

    // ── filter ─────────────────────────────────────────────────────

    #[test]
    fn filter_keep_all() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
            ],
            1,
        );
        let result = sel.filter(|_| true);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result.primary_index(), 1);
        assert_eq!(result.primary().start(), Offset::new(10));
    }

    #[test]
    fn filter_keep_none() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
            ],
            0,
        );
        assert!(sel.filter(|_| false).is_none());
    }

    #[test]
    fn filter_keep_some() {
        // 3 ranges, primary at index 1 (offset 10). Remove primary.
        // Survivors: index 0 (offset 0) and index 2 (offset 20).
        // Nearest to old primary start (10): offset 0 (dist 10) vs offset 20 (dist 10).
        // Tie → lower index wins → new primary is index 0.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
            ],
            1,
        );
        let result = sel
            .filter(|r| r.start() != Offset::new(10))
            .expect("should keep 2 ranges");
        assert_eq!(result.len(), 2);
        assert_eq!(result.primary_index(), 0);
        assert_eq!(result.primary().start(), Offset::new(0));
    }

    #[test]
    fn filter_primary_survives() {
        // Primary at index 2 (offset 20). Remove indices 0 and 1.
        // Primary survives as the only range → new primary_index = 0.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
            ],
            2,
        );
        let result = sel
            .filter(|r| r.start() == Offset::new(20))
            .expect("primary should survive");
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary_index(), 0);
        assert_eq!(result.primary().start(), Offset::new(20));
    }

    #[test]
    fn filter_single_range_passes() {
        let sel = Selections::single(SelectionRange::new(Offset::new(5), Offset::new(10)));
        let result = sel.filter(|_| true).expect("single range passes");
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(5));
    }

    #[test]
    fn filter_single_range_fails() {
        let sel = Selections::single(SelectionRange::new(Offset::new(5), Offset::new(10)));
        assert!(sel.filter(|_| false).is_none());
    }

    #[test]
    fn filter_always_false_multi_range_returns_none() {
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
                SelectionRange::new(Offset::new(30), Offset::new(35)),
            ],
            2,
        );
        assert!(sel.filter(|_| false).is_none());
    }

    #[test]
    fn filter_remove_only_primary_from_five_ranges() {
        // 5 ranges, primary at index 2 (offset 20). Remove only primary.
        // Nearest by start to 20: offset 10 (dist 10) and offset 30 (dist 10).
        // Tie → lower index in survivors wins → index 1 (offset 10).
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
                SelectionRange::new(Offset::new(30), Offset::new(35)),
                SelectionRange::new(Offset::new(40), Offset::new(45)),
            ],
            2,
        );
        let result = sel
            .filter(|r| r.start() != Offset::new(20))
            .expect("should keep 4 ranges");
        assert_eq!(result.len(), 4);
        // Survivors: [0..5, 10..15, 30..35, 40..45]
        // Old primary start=20. Distances: 20, 10, 10, 20.
        // Tie at dist 10 between index 1 (10..15) and index 2 (30..35).
        // min_by_key with tie → first encountered → index 1.
        assert_eq!(result.primary_index(), 1);
        assert_eq!(result.primary().start(), Offset::new(10));
    }

    #[test]
    fn filter_primary_is_last_and_removed() {
        // Primary at index 4 (last, offset 40). Remove it.
        // Nearest by start to 40: offset 30 (dist 10) at index 3 in survivors.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
                SelectionRange::new(Offset::new(30), Offset::new(35)),
                SelectionRange::new(Offset::new(40), Offset::new(45)),
            ],
            4,
        );
        let result = sel
            .filter(|r| r.start() != Offset::new(40))
            .expect("should keep 4 ranges");
        assert_eq!(result.len(), 4);
        assert_eq!(result.primary_index(), 3);
        assert_eq!(result.primary().start(), Offset::new(30));
    }

    #[test]
    fn filter_primary_is_first_and_removed() {
        // Primary at index 0 (first, offset 0). Remove it.
        // Nearest by start to 0: offset 10 (dist 10) at index 0 in survivors.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
                SelectionRange::new(Offset::new(30), Offset::new(35)),
                SelectionRange::new(Offset::new(40), Offset::new(45)),
            ],
            0,
        );
        let result = sel
            .filter(|r| r.start() != Offset::new(0))
            .expect("should keep 4 ranges");
        assert_eq!(result.len(), 4);
        assert_eq!(result.primary_index(), 0);
        assert_eq!(result.primary().start(), Offset::new(10));
    }

    #[test]
    fn filter_all_identical_ranges_keep_all() {
        // 4 identical ranges — predicate true. All kept.
        let range = SelectionRange::new(Offset::new(5), Offset::new(10));
        let sel = Selections::from_vec(vec![range, range, range, range], 2);
        let result = sel.filter(|_| true).expect("all should survive");
        assert_eq!(result.len(), 4);
        assert_eq!(result.primary_index(), 2);
        // All ranges identical.
        for r in result.iter() {
            assert_eq!(r.start(), Offset::new(5));
            assert_eq!(r.end(), Offset::new(10));
        }
    }

    #[test]
    fn filter_backward_ranges_preserved() {
        // Backward ranges: anchor > head.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(15), Offset::new(10)), // backward
                SelectionRange::new(Offset::new(25), Offset::new(20)), // backward
                SelectionRange::new(Offset::new(35), Offset::new(30)), // backward
            ],
            1,
        );
        // Keep first and third, remove primary (second).
        let result = sel
            .filter(|r| r.start() != Offset::new(20))
            .expect("should keep 2");
        assert_eq!(result.len(), 2);
        // Survivors: anchor=15,head=10 and anchor=35,head=30.
        // Verify backward direction preserved.
        assert!(!result.ranges()[0].is_forward());
        assert_eq!(result.ranges()[0].anchor(), Offset::new(15));
        assert_eq!(result.ranges()[0].head(), Offset::new(10));
        assert!(!result.ranges()[1].is_forward());
        assert_eq!(result.ranges()[1].anchor(), Offset::new(35));
        assert_eq!(result.ranges()[1].head(), Offset::new(30));
        // Old primary start was 20. Nearest: start=10 (dist 10) vs start=30 (dist 10).
        // Tie → lower index → index 0.
        assert_eq!(result.primary_index(), 0);
    }

    // ── merge_consecutive ───────────────────────────────────────

    #[test]
    fn merge_consecutive_touching() {
        // Two touching ranges: [0..5] [5..10] → [0..10]
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(0));
        assert_eq!(result.primary().end(), Offset::new(10));
    }

    #[test]
    fn merge_consecutive_non_touching() {
        // Two non-touching ranges: [0..5] [7..10] → stays 2
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(7), Offset::new(10)),
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 2);
        assert_eq!(result.ranges()[0].start(), Offset::new(0));
        assert_eq!(result.ranges()[0].end(), Offset::new(5));
        assert_eq!(result.ranges()[1].start(), Offset::new(7));
        assert_eq!(result.ranges()[1].end(), Offset::new(10));
    }

    #[test]
    fn merge_consecutive_mixed() {
        // 4 ranges: [0..5] [5..10] [20..25] [25..30]
        // Two pairs touching → becomes 2: [0..10] [20..30]
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
                SelectionRange::new(Offset::new(25), Offset::new(30)),
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 2);
        assert_eq!(result.ranges()[0].start(), Offset::new(0));
        assert_eq!(result.ranges()[0].end(), Offset::new(10));
        assert_eq!(result.ranges()[1].start(), Offset::new(20));
        assert_eq!(result.ranges()[1].end(), Offset::new(30));
    }

    #[test]
    fn merge_consecutive_single_range() {
        // Single range — passthrough
        let sel = Selections::single(SelectionRange::new(Offset::new(3), Offset::new(7)));
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(3));
        assert_eq!(result.primary().end(), Offset::new(7));
    }

    #[test]
    fn merge_consecutive_three_chain() {
        // 3 touching ranges: [0..5] [5..10] [10..15] → [0..15]
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
                SelectionRange::new(Offset::new(10), Offset::new(15)),
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(0));
        assert_eq!(result.primary().end(), Offset::new(15));
    }

    #[test]
    fn merge_consecutive_primary_in_merged_group() {
        // Primary is index 2 (in the second merged group).
        // [0..5] [5..10] [20..25] [25..30]
        // Merges to: [0..10] [20..30]
        // Primary was in original index 2 (20..25), which merges into [20..30] at index 1.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
                SelectionRange::new(Offset::new(25), Offset::new(30)),
            ],
            2,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 2);
        assert_eq!(result.primary_index(), 1);
        assert_eq!(result.primary().start(), Offset::new(20));
        assert_eq!(result.primary().end(), Offset::new(30));
    }

    // ── merge_consecutive edge cases (validation agent) ────────

    #[test]
    fn merge_consecutive_long_chain_12_touching() {
        // 12 touching ranges: [0..5] [5..10] [10..15] ... [55..60] → [0..60]
        let ranges: Vec<_> = (0..12)
            .map(|i| SelectionRange::new(Offset::new(i * 5), Offset::new(i * 5 + 5)))
            .collect();
        let sel = Selections::from_vec(ranges, 0);
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(0));
        assert_eq!(result.primary().end(), Offset::new(60));
    }

    #[test]
    fn merge_consecutive_long_chain_primary_at_middle() {
        // 10 touching ranges, primary at index 5 (middle of chain).
        // All merge into one. Primary should track into the merged range.
        let ranges: Vec<_> = (0..10)
            .map(|i| SelectionRange::new(Offset::new(i * 5), Offset::new(i * 5 + 5)))
            .collect();
        let sel = Selections::from_vec(ranges, 5);
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary_index(), 0);
        assert_eq!(result.primary().start(), Offset::new(0));
        assert_eq!(result.primary().end(), Offset::new(50));
    }

    #[test]
    fn merge_consecutive_long_chain_primary_at_last() {
        // 10 touching ranges, primary at index 9 (last in chain).
        let ranges: Vec<_> = (0..10)
            .map(|i| SelectionRange::new(Offset::new(i * 5), Offset::new(i * 5 + 5)))
            .collect();
        let sel = Selections::from_vec(ranges, 9);
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary_index(), 0);
        assert_eq!(result.primary().start(), Offset::new(0));
        assert_eq!(result.primary().end(), Offset::new(50));
    }

    #[test]
    fn merge_consecutive_primary_at_each_position_in_pair() {
        // Two touching ranges [0..5] [5..10]. Test primary at 0 and 1.
        for primary in 0..2 {
            let sel = Selections::from_vec(
                vec![
                    SelectionRange::new(Offset::new(0), Offset::new(5)),
                    SelectionRange::new(Offset::new(5), Offset::new(10)),
                ],
                primary,
            );
            let result = sel.merge_consecutive();
            assert_eq!(result.len(), 1, "primary={primary}");
            assert_eq!(result.primary_index(), 0, "primary={primary}");
            assert_eq!(
                result.primary().start(),
                Offset::new(0),
                "primary={primary}"
            );
            assert_eq!(result.primary().end(), Offset::new(10), "primary={primary}");
        }
    }

    #[test]
    fn merge_consecutive_primary_in_non_merged_group() {
        // [0..5] [5..10] [20..25] — primary at 2 (the lone range).
        // Merges to [0..10] [20..25]. Primary should be at index 1.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
                SelectionRange::new(Offset::new(20), Offset::new(25)),
            ],
            2,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 2);
        assert_eq!(result.primary_index(), 1);
        assert_eq!(result.primary().start(), Offset::new(20));
        assert_eq!(result.primary().end(), Offset::new(25));
    }

    #[test]
    fn merge_consecutive_zero_width_touching() {
        // Two zero-width cursors at the same position: both at offset 5.
        // They are touching (end==start since both anchor==head==5).
        // Should merge into a single zero-width cursor.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert!(result.primary().is_collapsed());
        assert_eq!(result.primary().head(), Offset::new(5));
    }

    #[test]
    fn merge_consecutive_zero_width_not_touching() {
        // Two zero-width cursors at different positions: 5 and 10.
        // Not touching (5 != 10). Should stay as 2.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(10)),
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn merge_consecutive_zero_width_at_boundary() {
        // Range [0..5] followed by zero-width cursor at 5.
        // end(5) == start(5): touching, should merge.
        // Result: [0..5] (the zero-width cursor extends nothing).
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::insert_cursor(Offset::new(5)),
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(0));
        assert_eq!(result.primary().end(), Offset::new(5));
    }

    #[test]
    fn merge_consecutive_zero_width_before_range() {
        // Zero-width cursor at 5 followed by range [5..10].
        // end(5) == start(5): touching, should merge.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::insert_cursor(Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
            ],
            1,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary_index(), 0);
        assert_eq!(result.primary().start(), Offset::new(5));
        assert_eq!(result.primary().end(), Offset::new(10));
    }

    #[test]
    fn merge_consecutive_backward_ranges_use_start_end() {
        // Backward ranges: anchor > head.
        // Range A: anchor=10, head=5 → start=5, end=10
        // Range B: anchor=15, head=10 → start=10, end=15
        // last.end()==10 == range.start()==10 → touching, should merge.
        // Merged: SelectionRange::new(start=5, end=15) → anchor=5, head=15 (forward).
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(10), Offset::new(5)), // backward
                SelectionRange::new(Offset::new(15), Offset::new(10)), // backward
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(5));
        assert_eq!(result.primary().end(), Offset::new(15));
    }

    #[test]
    fn merge_consecutive_mixed_forward_backward() {
        // Forward [0..5] followed by backward anchor=10,head=5 (start=5,end=10).
        // last.end()==5 == range.start()==5 → touching, merge to [0..10].
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(5)), // forward
                SelectionRange::new(Offset::new(10), Offset::new(5)), // backward: start=5, end=10
            ],
            0,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary().start(), Offset::new(0));
        assert_eq!(result.primary().end(), Offset::new(10));
    }

    #[test]
    fn merge_consecutive_non_sorted_not_merged() {
        // Unsorted input: [10..15] [0..5] [5..10].
        // Since merge_consecutive assumes sorted and only checks
        // last.end() == next.start(), the [10..15] [0..5] pair won't match
        // (15 != 0), and [0..5] [5..10] will match.
        // Result: [10..15] [0..10]  (2 ranges, unsorted).
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(10), Offset::new(15)),
                SelectionRange::new(Offset::new(0), Offset::new(5)),
                SelectionRange::new(Offset::new(5), Offset::new(10)),
            ],
            0,
        );
        let result = sel.merge_consecutive();
        // [10..15] stays, [0..5]+[5..10] merge to [0..10].
        assert_eq!(result.len(), 2);
        assert_eq!(result.ranges()[0].start(), Offset::new(10));
        assert_eq!(result.ranges()[0].end(), Offset::new(15));
        assert_eq!(result.ranges()[1].start(), Offset::new(0));
        assert_eq!(result.ranges()[1].end(), Offset::new(10));
    }

    #[test]
    fn merge_consecutive_all_non_touching() {
        // No ranges touch: [0..3] [5..8] [10..13] [15..18].
        // Nothing merges — all 4 stay.
        let sel = Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(3)),
                SelectionRange::new(Offset::new(5), Offset::new(8)),
                SelectionRange::new(Offset::new(10), Offset::new(13)),
                SelectionRange::new(Offset::new(15), Offset::new(18)),
            ],
            2,
        );
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 4);
        assert_eq!(result.primary_index(), 2);
    }

    #[test]
    fn merge_consecutive_all_same_offset() {
        // 5 zero-width cursors all at offset 0.
        // Each pair touches (0==0). All merge to one.
        let ranges: Vec<_> = (0..5)
            .map(|_| SelectionRange::insert_cursor(Offset::ZERO))
            .collect();
        let sel = Selections::from_vec(ranges, 3);
        let result = sel.merge_consecutive();
        assert_eq!(result.len(), 1);
        assert_eq!(result.primary_index(), 0);
        assert!(result.primary().is_collapsed());
        assert_eq!(result.primary().head(), Offset::ZERO);
    }

    #[test]
    fn merge_consecutive_invariant_property_test() {
        // Exhaustive check: for various chain configurations,
        // the result always satisfies len >= 1 && primary_index < len.
        // Also: result len <= input len (merging can only reduce count).
        // Also: if all input ranges are touching sequentially, result len == 1.
        for n in 1..=10 {
            for primary in 0..n {
                // Case 1: All touching (full chain).
                let ranges: Vec<_> = (0..n)
                    .map(|i| SelectionRange::new(Offset::new(i * 5), Offset::new(i * 5 + 5)))
                    .collect();
                let sel = Selections::from_vec(ranges, primary);
                let result = sel.merge_consecutive();
                assert!(
                    result.len() >= 1,
                    "invariant: len >= 1 (n={n}, primary={primary})"
                );
                assert!(
                    result.primary_index() < result.len(),
                    "invariant: primary_index {} < len {} (n={n}, primary={primary})",
                    result.primary_index(),
                    result.len()
                );
                assert_eq!(
                    result.len(),
                    1,
                    "all-touching chain should merge to 1 (n={n}, primary={primary})"
                );

                // Case 2: None touching (gaps between).
                let ranges: Vec<_> = (0..n)
                    .map(|i| SelectionRange::new(Offset::new(i * 100), Offset::new(i * 100 + 5)))
                    .collect();
                let sel = Selections::from_vec(ranges, primary);
                let result = sel.merge_consecutive();
                assert!(
                    result.primary_index() < result.len(),
                    "invariant: primary_index {} < len {} (none-touching, n={n}, primary={primary})",
                    result.primary_index(),
                    result.len()
                );
                assert_eq!(
                    result.len(),
                    n,
                    "none-touching should stay at {n} (primary={primary})"
                );

                // Case 3: Alternating touch/gap.
                if n >= 2 {
                    let ranges: Vec<_> = (0..n)
                        .map(|i| {
                            if i % 2 == 0 {
                                // Even: each starts at i*10
                                SelectionRange::new(Offset::new(i * 10), Offset::new(i * 10 + 5))
                            } else {
                                // Odd: touches previous (starts at prev end = (i-1)*10+5)
                                SelectionRange::new(
                                    Offset::new((i - 1) * 10 + 5),
                                    Offset::new((i - 1) * 10 + 10),
                                )
                            }
                        })
                        .collect();
                    let sel = Selections::from_vec(ranges, primary);
                    let result = sel.merge_consecutive();
                    assert!(
                        result.primary_index() < result.len(),
                        "invariant: primary_index {} < len {} (alternating, n={n}, primary={primary})",
                        result.primary_index(),
                        result.len()
                    );
                    assert!(
                        result.len() <= n,
                        "merge can only reduce count (alternating, n={n}, primary={primary})"
                    );
                }
            }
        }
    }

    #[test]
    fn filter_invariant_property_test() {
        // For many combinations of input sizes, primary positions, and filter
        // patterns, verify: result is None (all filtered) OR
        // (len >= 1 AND primary_index < len).
        for n_ranges in 1..=8 {
            for primary in 0..n_ranges {
                // Build ranges at offsets 0, 100, 200, ...
                let ranges: Vec<_> = (0..n_ranges)
                    .map(|i| SelectionRange::new(Offset::new(i * 100), Offset::new(i * 100 + 50)))
                    .collect();

                // Test keeping every possible subset via bitmask.
                for mask in 0u32..(1 << n_ranges) {
                    let sel = Selections::from_vec(ranges.clone(), primary);
                    let mut idx = 0usize;
                    let result = sel.filter(|_r| {
                        let keep = (mask >> idx) & 1 == 1;
                        idx += 1;
                        keep
                    });

                    let kept = mask.count_ones() as usize;
                    if kept == 0 {
                        assert!(
                            result.is_none(),
                            "expected None for mask={mask:#b}, n={n_ranges}, primary={primary}"
                        );
                    } else {
                        let r = result.unwrap_or_else(|| {
                            panic!(
                                "expected Some for mask={mask:#b}, n={n_ranges}, primary={primary}"
                            )
                        });
                        assert_eq!(
                            r.len(),
                            kept,
                            "wrong len for mask={mask:#b}, n={n_ranges}, primary={primary}"
                        );
                        assert!(
                            r.primary_index() < r.len(),
                            "primary_index {} >= len {} for mask={mask:#b}, n={n_ranges}, primary={primary}",
                            r.primary_index(),
                            r.len()
                        );
                    }
                }
            }
        }
    }
}
