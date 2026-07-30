//! Text-mutating selection operations.
//!
//! These produce `Vec<Effect>` because they modify document text
//! (rotate contents, align by inserting spaces).

use compact_str::CompactString;

use crate::effects::Effect;
use crate::errors::VimError;
use crate::primitives::{Direction, Offset, Range, Selections};

/// Rotate text content between selections.
///
/// Forward: each selection gets the content of the previous selection.
/// Backward: each selection gets the content of the next selection.
///
/// No-op if single selection (returns empty vec).
/// `count` controls how many positions to rotate.
#[must_use]
pub fn rotate_contents(
    selections: &Selections,
    text: &str,
    direction: Direction,
    count: usize,
) -> Vec<Effect> {
    if selections.is_single() {
        return Vec::new();
    }

    let ranges = selections.ranges();
    let n = ranges.len();

    // Collect text content of each selection range.
    let contents: Vec<&str> = ranges
        .iter()
        .map(|sel| {
            let start = sel.start().get();
            let end = sel.end().get().min(text.len());
            if start >= end {
                ""
            } else {
                &text[start..end]
            }
        })
        .collect();

    // Build rotated index mapping. After rotation, slot i gets contents[src[i]].
    let effective = count % n;
    if effective == 0 {
        return Vec::new();
    }

    let mut rotated: Vec<&str> = vec![""; n];
    for i in 0..n {
        let src = match direction {
            // Forward: each slot gets content from (count) positions earlier.
            Direction::Forward => (n + i - effective) % n,
            // Backward: each slot gets content from (count) positions later.
            Direction::Backward => (i + effective) % n,
        };
        rotated[i] = contents[src];
    }

    // Build Replace effects bottom-to-top (reverse order preserves offsets).
    let mut effects = Vec::with_capacity(n);
    for i in (0..n).rev() {
        let sel = &ranges[i];
        let range = Range::new(sel.start(), sel.end());
        effects.push(Effect::replace(range, CompactString::new(rotated[i])));
    }

    effects
}

/// Align selection cursors by inserting padding spaces.
///
/// Groups selections by line, then for each "slot" (nth selection on a line),
/// pads to the maximum column across all lines for that slot.
///
/// No-op if single selection (returns empty vec).
///
/// # Errors
///
/// Returns [`VimError::InvalidArgument`] with the message
/// `"align cannot work with multi-line selections"` if any selection spans a
/// newline. Columns are only comparable within a line, so a selection that
/// crosses one has no single column to align. This is the only failure mode:
/// with one selection, or with selections that all stay on their own line, the
/// function always succeeds.
pub fn align_selections(selections: &Selections, text: &str) -> Result<Vec<Effect>, VimError> {
    if selections.is_single() {
        return Ok(Vec::new());
    }

    let ranges = selections.ranges();

    // Reject multi-line selections — alignment is only meaningful for single-line selections.
    for sel in ranges {
        let start = sel.start().get().min(text.len());
        let end = sel.end().get().min(text.len());
        if start < end && text[start..end].contains('\n') {
            return Err(VimError::InvalidArgument(
                "align cannot work with multi-line selections".into(),
            ));
        }
    }

    // For each selection, compute: (line_index, column, offset_of_cursor).
    // We use the start of each selection as the cursor position for alignment.
    struct AlignInfo {
        line_index: usize,
        column: usize,
        offset: usize,
    }

    let mut infos: Vec<AlignInfo> = Vec::with_capacity(ranges.len());

    for sel in ranges {
        let offset = sel.start().get().min(text.len());

        // Find line start by scanning backward for '\n'.
        let line_start = text[..offset].rfind('\n').map_or(0, |pos| pos + 1);

        let column = offset - line_start;

        // Count line index by counting newlines before offset.
        let line_index = text[..offset].bytes().filter(|&b| b == b'\n').count();

        infos.push(AlignInfo {
            line_index,
            column,
            offset,
        });
    }

    // Group by line, tracking the slot index within each line.
    // Since selections are sorted by offset (Selections invariant), selections
    // on the same line appear consecutively and in order.
    let mut slots: Vec<usize> = Vec::with_capacity(infos.len());
    let mut current_line = None;
    let mut slot_counter = 0usize;

    for info in &infos {
        if Some(info.line_index) == current_line {
            slot_counter += 1;
        } else {
            current_line = Some(info.line_index);
            slot_counter = 0;
        }
        slots.push(slot_counter);
    }

    // Find max column per slot.
    let max_slot = slots.iter().copied().max().unwrap_or(0);
    let mut max_col_per_slot: Vec<usize> = vec![0; max_slot + 1];
    for (i, info) in infos.iter().enumerate() {
        let slot = slots[i];
        if info.column > max_col_per_slot[slot] {
            max_col_per_slot[slot] = info.column;
        }
    }

    // Build Insert effects bottom-to-top.
    let mut effects: Vec<Effect> = Vec::new();
    for i in (0..infos.len()).rev() {
        let slot = slots[i];
        let target_col = max_col_per_slot[slot];
        let current_col = infos[i].column;
        let padding = target_col - current_col;
        if padding > 0 {
            let spaces = CompactString::from(" ".repeat(padding));
            effects.push(Effect::insert(Offset::new(infos[i].offset), spaces));
        }
    }

    Ok(effects)
}

#[cfg(test)]
mod tests {
    use smallvec::SmallVec;

    use super::*;
    use crate::primitives::{Offset, SelectionRange, Selections};

    // ── rotate_contents tests ───────────────────────────────────────

    #[test]
    fn rotate_forward_basic() {
        // 3 selections: "aaa", "bbb", "ccc"
        // Forward rotation: each gets previous content.
        // Result: "ccc", "aaa", "bbb"
        let text = "aaa bbb ccc";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)), // "aaa"
            SelectionRange::new(Offset::new(4), Offset::new(7)), // "bbb"
            SelectionRange::new(Offset::new(8), Offset::new(11)), // "ccc"
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = rotate_contents(&sel, text, Direction::Forward, 1);

        // Effects are bottom-to-top: index 2, 1, 0.
        assert_eq!(effects.len(), 3);

        // Slot 2 gets content of slot 1 ("bbb").
        assert_eq!(
            effects[0],
            Effect::replace(
                Range::new(Offset::new(8), Offset::new(11)),
                CompactString::new("bbb")
            )
        );
        // Slot 1 gets content of slot 0 ("aaa").
        assert_eq!(
            effects[1],
            Effect::replace(
                Range::new(Offset::new(4), Offset::new(7)),
                CompactString::new("aaa")
            )
        );
        // Slot 0 gets content of slot 2 ("ccc").
        assert_eq!(
            effects[2],
            Effect::replace(
                Range::new(Offset::new(0), Offset::new(3)),
                CompactString::new("ccc")
            )
        );
    }

    #[test]
    fn rotate_backward_basic() {
        // 3 selections: "aaa", "bbb", "ccc"
        // Backward rotation: each gets next content.
        // Result: "bbb", "ccc", "aaa"
        let text = "aaa bbb ccc";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)),
            SelectionRange::new(Offset::new(4), Offset::new(7)),
            SelectionRange::new(Offset::new(8), Offset::new(11)),
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = rotate_contents(&sel, text, Direction::Backward, 1);

        assert_eq!(effects.len(), 3);

        // Slot 2 gets content of slot 0 ("aaa").
        assert_eq!(
            effects[0],
            Effect::replace(
                Range::new(Offset::new(8), Offset::new(11)),
                CompactString::new("aaa")
            )
        );
        // Slot 1 gets content of slot 2 ("ccc").
        assert_eq!(
            effects[1],
            Effect::replace(
                Range::new(Offset::new(4), Offset::new(7)),
                CompactString::new("ccc")
            )
        );
        // Slot 0 gets content of slot 1 ("bbb").
        assert_eq!(
            effects[2],
            Effect::replace(
                Range::new(Offset::new(0), Offset::new(3)),
                CompactString::new("bbb")
            )
        );
    }

    #[test]
    fn rotate_single_noop() {
        let text = "hello";
        let sel = Selections::single(SelectionRange::new(Offset::new(0), Offset::new(5)));
        let effects = rotate_contents(&sel, text, Direction::Forward, 1);
        assert!(effects.is_empty());
    }

    #[test]
    fn rotate_count_2() {
        // 4 selections: "a", "b", "c", "d"
        // Forward count=2: each gets content from 2 positions earlier.
        // Result: "c", "d", "a", "b"
        let text = "a b c d";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(1)), // "a"
            SelectionRange::new(Offset::new(2), Offset::new(3)), // "b"
            SelectionRange::new(Offset::new(4), Offset::new(5)), // "c"
            SelectionRange::new(Offset::new(6), Offset::new(7)), // "d"
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = rotate_contents(&sel, text, Direction::Forward, 2);

        assert_eq!(effects.len(), 4);

        // Bottom-to-top: slot 3 gets "b", slot 2 gets "a", slot 1 gets "d", slot 0 gets "c".
        assert_eq!(
            effects[0],
            Effect::replace(
                Range::new(Offset::new(6), Offset::new(7)),
                CompactString::new("b")
            )
        );
        assert_eq!(
            effects[1],
            Effect::replace(
                Range::new(Offset::new(4), Offset::new(5)),
                CompactString::new("a")
            )
        );
        assert_eq!(
            effects[2],
            Effect::replace(
                Range::new(Offset::new(2), Offset::new(3)),
                CompactString::new("d")
            )
        );
        assert_eq!(
            effects[3],
            Effect::replace(
                Range::new(Offset::new(0), Offset::new(1)),
                CompactString::new("c")
            )
        );
    }

    #[test]
    fn rotate_count_equals_len_is_noop() {
        // count == n means effective rotation is 0 → no-op.
        let text = "aaa bbb ccc";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(3)),
            SelectionRange::new(Offset::new(4), Offset::new(7)),
            SelectionRange::new(Offset::new(8), Offset::new(11)),
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = rotate_contents(&sel, text, Direction::Forward, 3);
        assert!(effects.is_empty());
    }

    // ── align_selections tests ──────────────────────────────────────

    #[test]
    fn align_basic() {
        // Three lines with "=" at different columns:
        // "x = 1\n"       — "=" at column 2
        // "foo = 2\n"     — "=" at column 4
        // "ab = 3\n"      — "=" at column 3
        // After alignment, all "=" should be at column 4 (max).
        let text = "x = 1\nfoo = 2\nab = 3\n";
        //          0123456789...

        // Offsets of "=" on each line:
        // Line 0: "x = 1\n" starts at 0, "=" at offset 2
        // Line 1: "foo = 2\n" starts at 6, "=" at offset 10
        // Line 2: "ab = 3\n" starts at 14, "=" at offset 17
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(2), Offset::new(3)), // "=" line 0, col 2
            SelectionRange::new(Offset::new(10), Offset::new(11)), // "=" line 1, col 4
            SelectionRange::new(Offset::new(17), Offset::new(18)), // "=" line 2, col 3
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = align_selections(&sel, text).unwrap();

        // Max column for slot 0 is 4 (from line 1).
        // Line 0 needs 2 spaces (col 2 → 4).
        // Line 1 needs 0 spaces (already at col 4).
        // Line 2 needs 1 space (col 3 → 4).
        // Effects are bottom-to-top.
        assert_eq!(effects.len(), 2);

        // Line 2 gets 1 space inserted at offset 17.
        assert_eq!(
            effects[0],
            Effect::insert(Offset::new(17), CompactString::new(" "))
        );
        // Line 0 gets 2 spaces inserted at offset 2.
        assert_eq!(
            effects[1],
            Effect::insert(Offset::new(2), CompactString::new("  "))
        );
    }

    #[test]
    fn align_single_noop() {
        let text = "hello = world";
        let sel = Selections::single(SelectionRange::new(Offset::new(6), Offset::new(7)));
        let effects = align_selections(&sel, text).unwrap();
        assert!(effects.is_empty());
    }

    #[test]
    fn align_already_aligned() {
        // All selections at the same column → no padding needed.
        let text = "aa = 1\nbb = 2\ncc = 3\n";
        // "=" is at column 3 on all lines.
        // Line 0 starts at 0, "=" at 3.
        // Line 1 starts at 7, "=" at 10.
        // Line 2 starts at 14, "=" at 17.
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(3), Offset::new(4)),
            SelectionRange::new(Offset::new(10), Offset::new(11)),
            SelectionRange::new(Offset::new(17), Offset::new(18)),
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = align_selections(&sel, text).unwrap();
        assert!(effects.is_empty());
    }

    // ── rotate_contents edge-case tests ────────────────────────────

    #[test]
    fn rotate_forward_two_selections() {
        // 2 selections swap contents on forward rotation by 1.
        let text = "xx yy";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(2)), // "xx"
            SelectionRange::new(Offset::new(3), Offset::new(5)), // "yy"
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = rotate_contents(&sel, text, Direction::Forward, 1);

        assert_eq!(effects.len(), 2);
        // Slot 1 gets "xx" (from slot 0).
        assert_eq!(
            effects[0],
            Effect::replace(
                Range::new(Offset::new(3), Offset::new(5)),
                CompactString::new("xx")
            )
        );
        // Slot 0 gets "yy" (from slot 1).
        assert_eq!(
            effects[1],
            Effect::replace(
                Range::new(Offset::new(0), Offset::new(2)),
                CompactString::new("yy")
            )
        );
    }

    #[test]
    fn rotate_backward_two_selections() {
        // Backward by 1 on 2 selections also swaps (same as forward by 1).
        let text = "xx yy";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(2)),
            SelectionRange::new(Offset::new(3), Offset::new(5)),
        ]);
        let sel = Selections::new(ranges, 0);
        let fwd = rotate_contents(&sel, text, Direction::Forward, 1);
        let bwd = rotate_contents(&sel, text, Direction::Backward, 1);
        // With n=2, forward-1 == backward-1 (both are swaps).
        assert_eq!(fwd, bwd);
    }

    #[test]
    fn rotate_unequal_length_content() {
        // Selections with different string sizes: "a", "bb", "ccc".
        // Forward by 1: slot 0 ← "ccc", slot 1 ← "a", slot 2 ← "bb".
        let text = "a bb ccc";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(1)), // "a"
            SelectionRange::new(Offset::new(2), Offset::new(4)), // "bb"
            SelectionRange::new(Offset::new(5), Offset::new(8)), // "ccc"
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = rotate_contents(&sel, text, Direction::Forward, 1);

        assert_eq!(effects.len(), 3);
        // Slot 2 gets "bb".
        assert_eq!(
            effects[0],
            Effect::replace(
                Range::new(Offset::new(5), Offset::new(8)),
                CompactString::new("bb")
            )
        );
        // Slot 1 gets "a".
        assert_eq!(
            effects[1],
            Effect::replace(
                Range::new(Offset::new(2), Offset::new(4)),
                CompactString::new("a")
            )
        );
        // Slot 0 gets "ccc".
        assert_eq!(
            effects[2],
            Effect::replace(
                Range::new(Offset::new(0), Offset::new(1)),
                CompactString::new("ccc")
            )
        );
    }

    #[test]
    fn rotate_preserves_total_text_bytes() {
        // The sum of replacement text bytes equals the sum of original selection bytes.
        let text = "short medium muchlonger";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(5)), // "short"
            SelectionRange::new(Offset::new(6), Offset::new(12)), // "medium"
            SelectionRange::new(Offset::new(13), Offset::new(23)), // "muchlonger"
        ]);
        let sel = Selections::new(ranges, 0);

        let original_total: usize = ranges_text_len(&sel, text);

        let effects = rotate_contents(&sel, text, Direction::Forward, 1);
        let replacement_total: usize = effects
            .iter()
            .map(|e| match e {
                Effect::Replace { text: t, .. } => t.len(),
                _ => 0,
            })
            .sum();

        assert_eq!(original_total, replacement_total);
    }

    // ── align_selections edge-case tests ───────────────────────────

    #[test]
    fn align_with_tabs_in_prefix() {
        // Tab characters are single bytes; alignment counts byte-columns from
        // line start, not visual tab stops. Verify padding is based on byte offset.
        // Line 0: "\tx"  — tab at 0, "x" at byte-column 1 (offset 1)
        // Line 1: "\t\ty" — two tabs, "y" at byte-column 2 (offset 5)
        let text = "\tx\n\t\ty";
        // "x" at offset 1, column 1.
        // "y" at offset 5, column 2.
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(1), Offset::new(2)), // "x" col 1
            SelectionRange::new(Offset::new(5), Offset::new(6)), // "y" col 2
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = align_selections(&sel, text).unwrap();

        // Max column for slot 0 is 2 (from line 1). Line 0 needs 1 space.
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0],
            Effect::insert(Offset::new(1), CompactString::new(" "))
        );
    }

    #[test]
    fn align_multi_selection_same_line() {
        // Two selections per line, different columns.
        // Line 0: "a = b"     — "=" at col 2, "b" at col 4
        // Line 1: "foo = bar" — "=" at col 4, "bar" at col 6
        let text = "a = b\nfoo = bar";
        // Line 0 "=" at offset 2 (col 2), "b" at offset 4 (col 4).
        // Line 1 "=" at offset 10 (col 4), "b" at offset 12 (col 6).
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(2), Offset::new(3)), // "=" line 0, slot 0
            SelectionRange::new(Offset::new(4), Offset::new(5)), // "b" line 0, slot 1
            SelectionRange::new(Offset::new(10), Offset::new(11)), // "=" line 1, slot 0
            SelectionRange::new(Offset::new(12), Offset::new(13)), // "b" line 1, slot 1
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = align_selections(&sel, text).unwrap();

        // Slot 0 max col = 4 (line 1). Line 0 slot 0 needs 2 spaces at offset 2.
        // Slot 1 max col = 6 (line 1). Line 0 slot 1 needs 2 spaces at offset 4.
        // Effects bottom-to-top: first line 0 slot 1, then line 0 slot 0.
        assert_eq!(effects.len(), 2);
        assert_eq!(
            effects[0],
            Effect::insert(Offset::new(4), CompactString::new("  "))
        );
        assert_eq!(
            effects[1],
            Effect::insert(Offset::new(2), CompactString::new("  "))
        );
    }

    #[test]
    fn align_with_unicode() {
        // Unicode: byte offsets differ from character counts.
        // Line 0: "é = 1"  — 'é' is 2 bytes, "=" at byte offset 3 (col 3)
        // Line 1: "abcde = 2" — "=" at byte offset 6 (col 6)
        let text = "é = 1\nabcde = 2";
        // 'é' = 0xC3 0xA9 = 2 bytes. Line 0: é(2 bytes) + ' '(1) = 3 bytes to '='.
        // Line 0 starts at 0. "=" at offset 3, column 3.
        // "é = 1\n" = 2+1+1+1+1+1 = 7 bytes. Line 1 starts at offset 7.
        // "abcde = 2": "=" at offset 7+6 = 13, column 6.
        let eq0_offset = "é = 1\n".len(); // should be 7
        assert_eq!(eq0_offset, 7); // sanity check

        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(3), Offset::new(4)), // "=" line 0, col 3
            SelectionRange::new(Offset::new(13), Offset::new(14)), // "=" line 1, col 6
        ]);
        let sel = Selections::new(ranges, 0);
        let effects = align_selections(&sel, text).unwrap();

        // Max column for slot 0 is 6. Line 0 needs 3 spaces at offset 3.
        assert_eq!(effects.len(), 1);
        assert_eq!(
            effects[0],
            Effect::insert(Offset::new(3), CompactString::new("   "))
        );
    }

    // ── helpers ────────────────────────────────────────────────────

    fn ranges_text_len(selections: &Selections, text: &str) -> usize {
        selections
            .ranges()
            .iter()
            .map(|sel| {
                let s = sel.start().get();
                let e = sel.end().get().min(text.len());
                e.saturating_sub(s)
            })
            .sum()
    }

    #[test]
    fn align_rejects_multiline_selection() {
        let text = "hello\nworld";
        let ranges: SmallVec<[SelectionRange; 1]> = SmallVec::from_vec(vec![
            SelectionRange::new(Offset::new(0), Offset::new(11)), // spans newline
            SelectionRange::new(Offset::new(0), Offset::new(5)),
        ]);
        let sel = Selections::new(ranges, 0);
        let result = align_selections(&sel, text);
        assert!(result.is_err());
    }
}
