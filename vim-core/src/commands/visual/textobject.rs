//! Visual text object selection computation.
//!
//! Pure functions for computing how text objects interact with visual selections.
//! No dispatch calls, no effects — just `(inputs) → selection result`.

use crate::commands::helpers::line_start_for_offset;
use crate::primitives::{Offset, SelectionRange};

use super::types::VisualTextObjectResult;

/// Compute the new anchor/cursor based on selection state and text object range.
///
/// Three cases:
/// 1. Selection covers range → signals `needs_extend = true` (caller re-dispatches)
/// 2. Single-char selection → reset anchor to range start
/// 3. Otherwise → preserve anchor, extend cursor
///
/// Pure: `(text, range, selection) → VisualTextObjectResult`.
#[must_use]
pub fn compute_selection_update(
    text: &str,
    range_start: usize,
    range_end: usize,
    linewise: bool,
    selection: Option<&SelectionRange>,
    current_mode_linewise: bool,
) -> VisualTextObjectResult {
    // Text object ranges are half-open [start, end). For Neovim compatibility,
    // the selection head/cursor must be ON the last character (inclusive), not
    // past it. Convert range_end to the inclusive position.
    let inclusive_end = if range_end > range_start {
        crate::primitives::text_util::prev_char_boundary(text, range_end)
    } else {
        range_end
    };

    let (anchor, cursor, needs_extend) = match selection {
        Some(sel) => {
            let sel_anchor = sel.anchor().get();
            let head = sel.head().get();
            let single_char = sel.is_collapsed() || sel.is_single_char(text);

            // Detect whether the selection looks like it already came from
            // VisualLine mode: both endpoints sit at line starts (or the
            // selection is collapsed at a line start).  A fresh `v` entry
            // has head mid-line (e.g. sel(0,1)), so sel_is_linewise is
            // false and the first text-object call goes through the normal
            // single_char branch.  After the first linewise text-object
            // snaps both endpoints to line starts, sel_is_linewise becomes
            // true and subsequent calls can trigger extend.
            // Detect linewise selection: text object is linewise AND we're
            // actually in VisualLine mode (passed as current_mode_linewise),
            // OR both endpoints sit at line starts with non-collapsed selection.
            // A collapsed charwise selection at column 0 should NOT be treated
            // as linewise (it's just a fresh `v` at the start of a line).
            let sel_is_linewise = linewise && {
                if current_mode_linewise {
                    true
                } else {
                    let lo = sel_anchor.min(head);
                    let hi = sel_anchor.max(head);
                    lo != hi
                        && line_start_for_offset(text, lo) == lo
                        && line_start_for_offset(text, hi) == hi
                }
            };

            // Selection covers range: for linewise selections, expand each
            // endpoint to the full line it occupies before testing — a
            // collapsed selection at a line start represents one entire line.
            let selection_covers = if sel_is_linewise {
                let (s_lo, s_hi) = if head >= sel_anchor {
                    (
                        line_start_for_offset(text, sel_anchor),
                        crate::commands::helpers::line_end_for_offset(text, head),
                    )
                } else {
                    (
                        line_start_for_offset(text, head),
                        crate::commands::helpers::line_end_for_offset(text, sel_anchor),
                    )
                };
                s_lo <= range_start && s_hi >= inclusive_end
            } else if head >= sel_anchor {
                sel_anchor <= range_start && head >= inclusive_end
            } else {
                head <= range_start && sel_anchor >= inclusive_end
            };

            if selection_covers && (!single_char || sel_is_linewise) {
                // For linewise, a collapsed sel in VisualLine mode still
                // represents one line, so allow extend when single_char
                // is false (which happens when current_mode_linewise=true).
                (sel_anchor, head, true)
            } else if single_char {
                // Single-char or collapsed: reset anchor to range start,
                // cursor to inclusive end (ON last char)
                (range_start, inclusive_end, false)
            } else if head >= sel_anchor {
                // Forward: extend cursor to inclusive end. If the text object
                // starts before the current anchor, move anchor to cover it.
                let new_anchor = if range_start < sel_anchor {
                    range_start
                } else {
                    sel_anchor
                };
                (new_anchor, inclusive_end, false)
            } else {
                (sel_anchor, range_start, false)
            }
        }
        None => (range_start, inclusive_end, false),
    };

    // For linewise text objects, snap anchor/cursor to line starts
    let (final_anchor, final_cursor) = if linewise {
        let anchor_ls = line_start_for_offset(text, anchor);
        let cursor_ls = line_start_for_offset(text, cursor);
        (anchor_ls, cursor_ls)
    } else {
        (anchor, cursor)
    };

    VisualTextObjectResult {
        anchor: Offset::new(final_anchor),
        cursor: Offset::new(final_cursor),
        linewise,
        needs_extend,
    }
}

/// Compute extended anchor/cursor after re-dispatching to the next text object.
///
/// Called by the execution layer after it re-dispatches `dispatch_textobject`
/// with a new cursor position. This function merges the next text object range
/// into the existing selection.
///
/// Pure: `(anchor, head, next_range, direction_forward) → (anchor, cursor)`.
#[must_use]
pub const fn merge_extended_range(
    anchor: usize,
    head: usize,
    next_range_start: usize,
    next_range_end: usize,
) -> (usize, usize) {
    if head >= anchor {
        let new_anchor = if next_range_start < anchor {
            next_range_start
        } else {
            anchor
        };
        (new_anchor, next_range_end)
    } else {
        let new_anchor = if next_range_end > anchor {
            next_range_end
        } else {
            anchor
        };
        (new_anchor, next_range_start)
    }
}

/// Build `CommandResult` from a fully resolved `VisualTextObjectResult`.
///
/// Converts the pure computation result into effects:
/// - If linewise: sets mode to Visual Line
/// - Sets selection (anchor, cursor)
/// - Sets cursor position
pub fn build_textobject_effects(result: &VisualTextObjectResult) -> crate::commands::CommandResult {
    use crate::effects::Effects;
    use crate::primitives::SelectionShape;

    let mut effects = Effects::new();
    if result.linewise {
        effects = effects.set_mode(crate::primitives::Mode::Visual(
            crate::primitives::VisualType::Line,
        ));
    }
    let effects = effects.set_visual_selection(
        result.anchor,
        result.cursor,
        if result.linewise {
            SelectionShape::Line
        } else {
            SelectionShape::Char
        },
    );

    crate::commands::CommandResult::effects_only(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;

    #[test]
    fn test_single_char_selection() {
        let text = "hello world";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(1));
        let result = compute_selection_update(text, 0, 5, false, Some(&sel), false);
        assert_eq!(result.anchor, Offset::new(0));
        assert_eq!(result.cursor, Offset::new(4));
        assert!(!result.needs_extend);
    }

    #[test]
    fn test_collapsed_selection() {
        let text = "hello world";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(0));
        let result = compute_selection_update(text, 0, 5, false, Some(&sel), false);
        assert_eq!(result.anchor, Offset::new(0));
        assert_eq!(result.cursor, Offset::new(4));
        assert!(!result.needs_extend);
    }

    #[test]
    fn test_no_selection() {
        let text = "hello world";
        let result = compute_selection_update(text, 0, 5, false, None, false);
        assert_eq!(result.anchor, Offset::new(0));
        assert_eq!(result.cursor, Offset::new(4));
        assert!(!result.needs_extend);
    }

    #[test]
    fn test_preserve_anchor() {
        let text = "hello world test";
        let sel = SelectionRange::new(Offset::new(3), Offset::new(8));
        let result = compute_selection_update(text, 6, 11, false, Some(&sel), false);
        assert_eq!(result.anchor, Offset::new(3));
        assert_eq!(result.cursor, Offset::new(10));
        assert!(!result.needs_extend);
    }

    #[test]
    fn test_selection_covers_signals_extend() {
        let text = "hello world test";
        let sel = SelectionRange::new(Offset::new(0), Offset::new(5));
        let result = compute_selection_update(text, 0, 5, false, Some(&sel), false);
        assert!(result.needs_extend);
    }

    #[test]
    fn test_linewise_snaps_to_line_start() {
        let text = "line1\nline2\nline3";
        let result = compute_selection_update(text, 6, 11, true, None, false);
        assert_eq!(result.anchor, Offset::new(6));
        assert_eq!(result.cursor, Offset::new(6));
        assert!(result.linewise);
    }

    #[test]
    fn test_merge_extended_forward() {
        let (anchor, cursor) = merge_extended_range(0, 4, 6, 11);
        assert_eq!(anchor, 0);
        assert_eq!(cursor, 11);
    }

    #[test]
    fn test_merge_extended_backward() {
        let (anchor, cursor) = merge_extended_range(10, 3, 0, 5);
        assert_eq!(anchor, 10);
        assert_eq!(cursor, 0);
    }
}
