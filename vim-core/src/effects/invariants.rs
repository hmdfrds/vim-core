//! Effect invariant verification — precondition checks.
//!
//! Provides functions to validate that effects are consistent with document
//! state before they are applied. Used in debug builds to catch invalid effects
//! early (e.g., inserting past the end of the document, deleting an out-of-bounds
//! range, or setting the cursor beyond document length).
//!
//! # Import constraints
//!
//! This module only depends on:
//! - `primitives` — `Offset`, `Range`
//! - sibling `effect` module — `Effect`
//!
//! It does NOT import `state`, `commands`, `execution`, or any other layer.

use crate::effects::effect::Effect;
use crate::primitives::{Offset, Range};

// ═══════════════════════════════════════════════════════════════════════════════
// PRECONDITION CHECKS
// ═══════════════════════════════════════════════════════════════════════════════

/// Check whether a raw byte offset satisfies the cursor-position invariant.
///
/// For a non-empty document (`doc_len > 0`), the offset must be strictly less
/// than `doc_len` (the cursor gap must be before the last character's end).
/// For an empty document (`doc_len == 0`), only offset 0 is valid.
#[inline]
const fn valid_cursor_offset(offset: usize, doc_len: usize) -> bool {
    if doc_len == 0 {
        offset == 0
    } else {
        offset < doc_len
    }
}

/// Check whether an effect satisfies its precondition given a document length.
///
/// Returns `true` if the effect is valid for a document of `doc_len` bytes.
/// Returns `false` if the effect would violate a structural invariant:
///
/// | Effect variant           | Precondition                                           |
/// |--------------------------|--------------------------------------------------------|
/// | `Insert { offset, .. }`  | `offset <= doc_len` (can insert at end)                |
/// | `Delete { range }`       | `range.start <= range.end && range.end <= doc_len`     |
/// | `Replace { range, .. }`  | `range.start <= range.end && range.end <= doc_len`     |
/// | `SetCursor { offset }`   | cursor-position rule (see above)                       |
/// | `ScrollTo { offset }`    | cursor-position rule (same as `SetCursor`)             |
/// | `SetSelection { .. }`    | both `anchor` and `head` satisfy cursor-position rule  |
/// | All other variants       | `true` (no precondition to check)                      |
///
/// # Complexity
///
/// Time: O(1)
/// Space: O(1)
#[must_use]
pub const fn precondition(effect: &Effect, doc_len: usize) -> bool {
    match effect {
        Effect::Insert { offset, .. } => offset.get() <= doc_len,

        Effect::Delete { range } => {
            range.start().get() <= range.end().get() && range.end().get() <= doc_len
        }

        Effect::Replace { range, .. } => {
            range.start().get() <= range.end().get() && range.end().get() <= doc_len
        }

        Effect::SetCursor { offset } => valid_cursor_offset(offset.get(), doc_len),

        Effect::ScrollTo { offset } => valid_cursor_offset(offset.get(), doc_len),

        Effect::SetSelection { anchor, head, .. } => {
            valid_cursor_offset(anchor.get(), doc_len) && valid_cursor_offset(head.get(), doc_len)
        }

        _ => true,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VIOLATION DIAGNOSTICS
// ═══════════════════════════════════════════════════════════════════════════════

/// Format a range-based violation: reports inverted range or out-of-bounds end.
fn range_violation(label: &str, start: usize, end: usize, doc_len: usize) -> String {
    if start > end {
        format!("{label} range start {start} > end {end} (document length {doc_len})")
    } else {
        format!("{label} range end {end} exceeds document length {doc_len}")
    }
}

/// Format an offset-based violation for cursor-like effects.
fn offset_violation(label: &str, offset: usize, doc_len: usize) -> String {
    format!("{label} offset {offset} is out of bounds for document length {doc_len}")
}

/// Return a human-readable diagnostic message for a precondition violation.
///
/// If the effect passes its precondition, the message is still well-formed but
/// describes a non-violation. Callers typically only call this after
/// [`precondition`] returns `false`.
#[must_use]
pub fn violation_message(effect: &Effect, doc_len: usize) -> String {
    match effect {
        Effect::Insert { offset, text } => format!(
            "Insert at offset {} (text len {}) exceeds document length {doc_len}",
            offset.get(),
            text.len(),
        ),
        Effect::Delete { range } => {
            range_violation("Delete", range.start().get(), range.end().get(), doc_len)
        }
        Effect::Replace { range, text } => {
            let base = range_violation("Replace", range.start().get(), range.end().get(), doc_len);
            format!("{base} (replacement len {})", text.len())
        }
        Effect::SetCursor { offset } => offset_violation("SetCursor", offset.get(), doc_len),
        Effect::ScrollTo { offset } => offset_violation("ScrollTo", offset.get(), doc_len),
        Effect::SetSelection {
            anchor,
            head,
            shape,
        } => {
            let a_ok = valid_cursor_offset(anchor.get(), doc_len);
            let h_ok = valid_cursor_offset(head.get(), doc_len);
            let (a, h) = (anchor.get(), head.get());
            match (a_ok, h_ok) {
                (false, false) => format!(
                    "SetSelection anchor {a} and head {h} both out of bounds \
                     for doc_len {doc_len} (shape: {shape:?})"
                ),
                (false, true) => format!(
                    "SetSelection anchor {a} out of bounds for doc_len {doc_len} \
                     (head: {h}, shape: {shape:?})"
                ),
                (true, false) => format!(
                    "SetSelection head {h} out of bounds for doc_len {doc_len} \
                     (anchor: {a}, shape: {shape:?})"
                ),
                _ => {
                    format!("SetSelection: no violation (anchor: {a}, head: {h}, shape: {shape:?})")
                }
            }
        }
        other => format!("No precondition defined for effect: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BATCH VERIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Verify all effects in a slice against their preconditions.
///
/// # Panics
///
/// Panics on the first violation with a detailed diagnostic message that
/// includes the effect index, the failing effect, and the document length.
///
/// This is designed to be called in debug builds (via [`VerifyMiddleware`])
/// to catch invalid effects before they reach the host.
///
/// # Complexity
///
/// Time: O(n) where n = number of effects
/// Space: O(1)
///
/// [`VerifyMiddleware`]: super::middleware::VerifyMiddleware
pub fn verify_effects(effects: &[Effect], doc_len: usize) {
    for (i, effect) in effects.iter().enumerate() {
        assert!(
            precondition(effect, doc_len),
            "Effect invariant violation at index {i}: {msg}\n  effect: {effect:?}\n  doc_len: {doc_len}",
            msg = violation_message(effect, doc_len),
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RELEASE-MODE BOUNDS GUARD
// ═══════════════════════════════════════════════════════════════════════════════

/// Clamp a cursor-style offset to the valid range for a document of `doc_len`.
///
/// For non-empty documents the maximum valid cursor offset is `doc_len - 1`.
/// For empty documents only offset 0 is valid.
#[inline]
const fn clamp_cursor_offset(offset: Offset, doc_len: usize) -> Offset {
    if doc_len == 0 {
        Offset::new(0)
    } else if offset.get() >= doc_len {
        Offset::new(doc_len - 1)
    } else {
        offset
    }
}

/// Clamp an insert-style offset to the valid range (`0..=doc_len`).
#[inline]
const fn clamp_insert_offset(offset: Offset, doc_len: usize) -> Offset {
    if offset.get() > doc_len {
        Offset::new(doc_len)
    } else {
        offset
    }
}

/// Clamp a range so that both `start` and `end` are within `0..=doc_len`,
/// preserving the `start <= end` invariant.
#[inline]
const fn clamp_range(range: Range, doc_len: usize) -> Range {
    let start = if range.start().get() > doc_len {
        doc_len
    } else {
        range.start().get()
    };
    let end = if range.end().get() > doc_len {
        doc_len
    } else {
        range.end().get()
    };
    // Preserve start <= end (if start was already <= end, clamping both
    // to the same ceiling cannot invert them).
    Range::from_raw(start, end)
}

/// Lightweight release-mode bounds check for positional effects.
///
/// Returns the number of effects that were clamped. Zero means all effects
/// were already within bounds. This function modifies effects in-place,
/// clamping any out-of-bounds offsets to the valid range for `doc_len`.
///
/// # Clamping rules
///
/// | Effect variant                | Clamping rule                                         |
/// |-------------------------------|-------------------------------------------------------|
/// | `Insert { offset, .. }`       | `offset` clamped to `0..=doc_len`                     |
/// | `Delete { range }`            | `range.start` and `range.end` clamped to `0..=doc_len`|
/// | `Replace { range, .. }`       | same as `Delete`                                      |
/// | `SetCursor { offset }`        | `offset` clamped to valid cursor range                |
/// | `ScrollTo { offset }`         | same as `SetCursor`                                   |
/// | `SetSelection { .. }`         | `anchor` and `head` clamped to valid cursor range     |
/// | `SetMark { offset, .. }`      | `offset` clamped to valid cursor range                |
/// | `PushJumpList { offset }`     | `offset` clamped to valid cursor range                |
/// | `JumpToBuffer { offset, .. }` | `offset` clamped to valid cursor range                |
/// | All other variants            | unchanged                                             |
///
/// # Performance
///
/// Time: O(n) where n = number of effects
/// Space: O(1), zero allocation
///
/// Safe to call unconditionally in release builds. The function is a no-op
/// when all effects are already within bounds.
#[must_use]
pub fn bounds_check_and_clamp(effects: &mut [Effect], doc_len: usize) -> usize {
    let mut clamped = 0;
    for effect in effects.iter_mut() {
        match effect {
            Effect::Insert { ref mut offset, .. } => {
                let new = clamp_insert_offset(*offset, doc_len);
                if new != *offset {
                    *offset = new;
                    clamped += 1;
                }
            }
            Effect::Delete { ref mut range } => {
                let new = clamp_range(*range, doc_len);
                if new != *range {
                    *range = new;
                    clamped += 1;
                }
            }
            Effect::Replace { ref mut range, .. } => {
                let new = clamp_range(*range, doc_len);
                if new != *range {
                    *range = new;
                    clamped += 1;
                }
            }
            Effect::SetCursor { ref mut offset } => {
                let new = clamp_cursor_offset(*offset, doc_len);
                if new != *offset {
                    *offset = new;
                    clamped += 1;
                }
            }
            Effect::ScrollTo { ref mut offset } => {
                let new = clamp_cursor_offset(*offset, doc_len);
                if new != *offset {
                    *offset = new;
                    clamped += 1;
                }
            }
            Effect::SetSelection {
                ref mut anchor,
                ref mut head,
                ..
            } => {
                let new_anchor = clamp_cursor_offset(*anchor, doc_len);
                let new_head = clamp_cursor_offset(*head, doc_len);
                if new_anchor != *anchor || new_head != *head {
                    *anchor = new_anchor;
                    *head = new_head;
                    clamped += 1;
                }
            }
            Effect::SetMark { ref mut offset, .. } => {
                let new = clamp_cursor_offset(*offset, doc_len);
                if new != *offset {
                    *offset = new;
                    clamped += 1;
                }
            }
            Effect::PushJumpList { ref mut offset } => {
                let new = clamp_cursor_offset(*offset, doc_len);
                if new != *offset {
                    *offset = new;
                    clamped += 1;
                }
            }
            Effect::JumpToBuffer { ref mut offset, .. } => {
                let new = clamp_cursor_offset(*offset, doc_len);
                if new != *offset {
                    *offset = new;
                    clamped += 1;
                }
            }
            _ => {}
        }
    }
    clamped
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::effect::Effect;
    use crate::primitives::{Mode, Offset, Range, SelectionShape};
    use compact_str::CompactString;

    // ── Insert preconditions ──────────────────────────────────────────────

    #[test]
    fn insert_at_start_valid() {
        let effect = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("hello"),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn insert_at_middle_valid() {
        let effect = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new("x"),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn insert_at_end_valid() {
        let effect = Effect::Insert {
            offset: Offset::new(10),
            text: CompactString::new("x"),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn insert_past_end_invalid() {
        let effect = Effect::Insert {
            offset: Offset::new(11),
            text: CompactString::new("x"),
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn insert_into_empty_doc_at_zero_valid() {
        let effect = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("a"),
        };
        assert!(precondition(&effect, 0));
    }

    #[test]
    fn insert_into_empty_doc_past_zero_invalid() {
        let effect = Effect::Insert {
            offset: Offset::new(1),
            text: CompactString::new("a"),
        };
        assert!(!precondition(&effect, 0));
    }

    #[test]
    fn insert_empty_text_at_end_valid() {
        let effect = Effect::Insert {
            offset: Offset::new(5),
            text: CompactString::new(""),
        };
        assert!(precondition(&effect, 5));
    }

    // ── Delete preconditions ──────────────────────────────────────────────

    #[test]
    fn delete_valid_range() {
        let effect = Effect::Delete {
            range: Range::from_raw(2, 5),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn delete_entire_doc() {
        let effect = Effect::Delete {
            range: Range::from_raw(0, 10),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn delete_empty_range_valid() {
        let effect = Effect::Delete {
            range: Range::from_raw(5, 5),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn delete_range_end_exceeds_doc_len() {
        let effect = Effect::Delete {
            range: Range::from_raw(5, 11),
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn delete_from_empty_doc_at_zero_valid() {
        let effect = Effect::Delete {
            range: Range::from_raw(0, 0),
        };
        assert!(precondition(&effect, 0));
    }

    #[test]
    fn delete_single_byte_doc() {
        let effect = Effect::Delete {
            range: Range::from_raw(0, 1),
        };
        assert!(precondition(&effect, 1));
    }

    #[test]
    fn delete_past_single_byte_doc() {
        let effect = Effect::Delete {
            range: Range::from_raw(0, 2),
        };
        assert!(!precondition(&effect, 1));
    }

    // ── Replace preconditions ─────────────────────────────────────────────

    #[test]
    fn replace_valid_range() {
        let effect = Effect::Replace {
            range: Range::from_raw(2, 5),
            text: CompactString::new("xyz"),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn replace_range_end_exceeds_doc_len() {
        let effect = Effect::Replace {
            range: Range::from_raw(5, 11),
            text: CompactString::new("x"),
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn replace_empty_range_valid() {
        let effect = Effect::Replace {
            range: Range::from_raw(3, 3),
            text: CompactString::new("inserted"),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn replace_entire_doc() {
        let effect = Effect::Replace {
            range: Range::from_raw(0, 5),
            text: CompactString::new("replacement"),
        };
        assert!(precondition(&effect, 5));
    }

    // ── SetCursor preconditions ───────────────────────────────────────────

    #[test]
    fn set_cursor_at_start_valid() {
        let effect = Effect::SetCursor {
            offset: Offset::new(0),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn set_cursor_at_last_byte_valid() {
        let effect = Effect::SetCursor {
            offset: Offset::new(9),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn set_cursor_at_doc_len_invalid() {
        // Cursor at offset 10 in a 10-byte doc is past the last byte.
        let effect = Effect::SetCursor {
            offset: Offset::new(10),
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn set_cursor_past_doc_len_invalid() {
        let effect = Effect::SetCursor {
            offset: Offset::new(100),
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn set_cursor_empty_doc_at_zero_valid() {
        let effect = Effect::SetCursor {
            offset: Offset::new(0),
        };
        assert!(precondition(&effect, 0));
    }

    #[test]
    fn set_cursor_empty_doc_at_one_invalid() {
        let effect = Effect::SetCursor {
            offset: Offset::new(1),
        };
        assert!(!precondition(&effect, 0));
    }

    #[test]
    fn set_cursor_single_byte_doc_at_zero_valid() {
        let effect = Effect::SetCursor {
            offset: Offset::new(0),
        };
        assert!(precondition(&effect, 1));
    }

    #[test]
    fn set_cursor_single_byte_doc_at_one_invalid() {
        let effect = Effect::SetCursor {
            offset: Offset::new(1),
        };
        assert!(!precondition(&effect, 1));
    }

    // ── ScrollTo preconditions ────────────────────────────────────────────

    #[test]
    fn scroll_to_valid() {
        let effect = Effect::ScrollTo {
            offset: Offset::new(5),
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn scroll_to_at_doc_len_invalid() {
        let effect = Effect::ScrollTo {
            offset: Offset::new(10),
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn scroll_to_empty_doc_at_zero_valid() {
        let effect = Effect::ScrollTo {
            offset: Offset::new(0),
        };
        assert!(precondition(&effect, 0));
    }

    #[test]
    fn scroll_to_empty_doc_at_one_invalid() {
        let effect = Effect::ScrollTo {
            offset: Offset::new(1),
        };
        assert!(!precondition(&effect, 0));
    }

    // ── SetSelection preconditions ────────────────────────────────────────

    #[test]
    fn set_selection_both_valid() {
        let effect = Effect::SetSelection {
            anchor: Offset::new(2),
            head: Offset::new(8),
            shape: SelectionShape::Char,
        };
        assert!(precondition(&effect, 10));
    }

    #[test]
    fn set_selection_anchor_out_of_bounds() {
        let effect = Effect::SetSelection {
            anchor: Offset::new(10),
            head: Offset::new(5),
            shape: SelectionShape::Char,
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn set_selection_head_out_of_bounds() {
        let effect = Effect::SetSelection {
            anchor: Offset::new(5),
            head: Offset::new(10),
            shape: SelectionShape::Char,
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn set_selection_both_out_of_bounds() {
        let effect = Effect::SetSelection {
            anchor: Offset::new(15),
            head: Offset::new(20),
            shape: SelectionShape::Line,
        };
        assert!(!precondition(&effect, 10));
    }

    #[test]
    fn set_selection_empty_doc_at_zero_valid() {
        let effect = Effect::SetSelection {
            anchor: Offset::new(0),
            head: Offset::new(0),
            shape: SelectionShape::Char,
        };
        assert!(precondition(&effect, 0));
    }

    #[test]
    fn set_selection_anchor_head_reversed_still_valid() {
        // anchor > head is allowed (backward selection).
        let effect = Effect::SetSelection {
            anchor: Offset::new(8),
            head: Offset::new(2),
            shape: SelectionShape::Char,
        };
        assert!(precondition(&effect, 10));
    }

    // ── Other variants always pass ────────────────────────────────────────

    #[test]
    fn set_mode_always_passes() {
        let effect = Effect::set_mode(Mode::Insert);
        assert!(precondition(&effect, 0));
        assert!(precondition(&effect, 100));
    }

    #[test]
    fn clear_message_always_passes() {
        assert!(precondition(&Effect::ClearMessage, 0));
        assert!(precondition(&Effect::ClearMessage, 100));
    }

    #[test]
    fn begin_undo_group_always_passes() {
        let effect = Effect::BeginUndoGroup {
            cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
        };
        assert!(precondition(&effect, 0));
    }

    #[test]
    fn end_undo_group_always_passes() {
        let effect = Effect::EndUndoGroup { node_id: None };
        assert!(precondition(&effect, 0));
    }

    #[test]
    fn center_cursor_always_passes() {
        assert!(precondition(&Effect::CenterCursor, 0));
    }

    #[test]
    fn clear_selection_always_passes() {
        assert!(precondition(&Effect::ClearSelection, 0));
    }

    #[test]
    fn show_info_always_passes() {
        let effect = Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text(CompactString::new("test")),
        };
        assert!(precondition(&effect, 0));
    }

    // ── Violation messages ────────────────────────────────────────────────

    #[test]
    fn violation_message_insert() {
        let effect = Effect::Insert {
            offset: Offset::new(100),
            text: CompactString::new("abc"),
        };
        let msg = violation_message(&effect, 50);
        assert!(msg.contains("Insert"), "should mention Insert: {msg}");
        assert!(msg.contains("100"), "should mention offset 100: {msg}");
        assert!(msg.contains("50"), "should mention doc_len 50: {msg}");
    }

    #[test]
    fn violation_message_delete_end_exceeds() {
        let effect = Effect::Delete {
            range: Range::from_raw(5, 20),
        };
        let msg = violation_message(&effect, 10);
        assert!(msg.contains("Delete"), "should mention Delete: {msg}");
        assert!(msg.contains("20"), "should mention range end 20: {msg}");
        assert!(msg.contains("10"), "should mention doc_len 10: {msg}");
    }

    #[test]
    fn violation_message_replace_end_exceeds() {
        let effect = Effect::Replace {
            range: Range::from_raw(0, 20),
            text: CompactString::new("x"),
        };
        let msg = violation_message(&effect, 10);
        assert!(msg.contains("Replace"), "should mention Replace: {msg}");
        assert!(msg.contains("20"), "should mention range end 20: {msg}");
    }

    #[test]
    fn violation_message_set_cursor() {
        let effect = Effect::SetCursor {
            offset: Offset::new(50),
        };
        let msg = violation_message(&effect, 10);
        assert!(msg.contains("SetCursor"), "should mention SetCursor: {msg}");
        assert!(msg.contains("50"), "should mention offset 50: {msg}");
        assert!(msg.contains("10"), "should mention doc_len 10: {msg}");
    }

    #[test]
    fn violation_message_scroll_to() {
        let effect = Effect::ScrollTo {
            offset: Offset::new(50),
        };
        let msg = violation_message(&effect, 10);
        assert!(msg.contains("ScrollTo"), "should mention ScrollTo: {msg}");
        assert!(msg.contains("50"), "should mention offset 50: {msg}");
    }

    #[test]
    fn violation_message_set_selection_anchor_only() {
        let effect = Effect::SetSelection {
            anchor: Offset::new(50),
            head: Offset::new(5),
            shape: SelectionShape::Char,
        };
        let msg = violation_message(&effect, 10);
        assert!(msg.contains("anchor"), "should mention anchor: {msg}",);
        assert!(msg.contains("50"), "should mention offset 50: {msg}");
    }

    #[test]
    fn violation_message_set_selection_head_only() {
        let effect = Effect::SetSelection {
            anchor: Offset::new(5),
            head: Offset::new(50),
            shape: SelectionShape::Char,
        };
        let msg = violation_message(&effect, 10);
        assert!(msg.contains("head"), "should mention head: {msg}");
        assert!(msg.contains("50"), "should mention offset 50: {msg}");
    }

    #[test]
    fn violation_message_set_selection_both_invalid() {
        let effect = Effect::SetSelection {
            anchor: Offset::new(50),
            head: Offset::new(60),
            shape: SelectionShape::Line,
        };
        let msg = violation_message(&effect, 10);
        assert!(msg.contains("both"), "should mention both offsets: {msg}",);
    }

    #[test]
    fn violation_message_other_variant() {
        let msg = violation_message(&Effect::ClearMessage, 10);
        assert!(
            msg.contains("No precondition"),
            "should say no precondition: {msg}",
        );
    }

    // ── verify_effects ────────────────────────────────────────────────────

    #[test]
    fn verify_effects_empty_stream() {
        // Empty stream should not panic.
        verify_effects(&[], 10);
    }

    #[test]
    fn verify_effects_all_valid() {
        let effects = vec![
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("hello"),
            },
            Effect::SetCursor {
                offset: Offset::new(4),
            },
            Effect::set_mode(Mode::Normal),
            Effect::ClearMessage,
        ];
        // Should not panic.
        verify_effects(&effects, 10);
    }

    #[test]
    #[should_panic(expected = "Effect invariant violation at index 1")]
    fn verify_effects_panics_on_first_violation() {
        let effects = vec![
            Effect::SetCursor {
                offset: Offset::new(0),
            },
            // This one is invalid: cursor at offset 10 in a 10-byte doc.
            Effect::SetCursor {
                offset: Offset::new(10),
            },
            // This one is also invalid but should not be reached.
            Effect::SetCursor {
                offset: Offset::new(20),
            },
        ];
        verify_effects(&effects, 10);
    }

    #[test]
    #[should_panic(expected = "Insert")]
    fn verify_effects_panics_insert_violation() {
        let effects = vec![Effect::Insert {
            offset: Offset::new(100),
            text: CompactString::new("x"),
        }];
        verify_effects(&effects, 10);
    }

    #[test]
    #[should_panic(expected = "Delete")]
    fn verify_effects_panics_delete_violation() {
        let effects = vec![Effect::Delete {
            range: Range::from_raw(0, 20),
        }];
        verify_effects(&effects, 10);
    }

    #[test]
    #[should_panic(expected = "Replace")]
    fn verify_effects_panics_replace_violation() {
        let effects = vec![Effect::Replace {
            range: Range::from_raw(0, 20),
            text: CompactString::new("x"),
        }];
        verify_effects(&effects, 10);
    }

    // ── Boundary edge cases ───────────────────────────────────────────────

    #[test]
    fn insert_at_boundary_of_single_byte_doc() {
        // In a 1-byte doc, offset 0 and 1 are both valid for insert.
        let at_0 = Effect::Insert {
            offset: Offset::new(0),
            text: CompactString::new("x"),
        };
        let at_1 = Effect::Insert {
            offset: Offset::new(1),
            text: CompactString::new("x"),
        };
        let at_2 = Effect::Insert {
            offset: Offset::new(2),
            text: CompactString::new("x"),
        };
        assert!(precondition(&at_0, 1));
        assert!(precondition(&at_1, 1));
        assert!(!precondition(&at_2, 1));
    }

    #[test]
    fn delete_at_boundary_of_single_byte_doc() {
        let valid = Effect::Delete {
            range: Range::from_raw(0, 1),
        };
        let invalid = Effect::Delete {
            range: Range::from_raw(0, 2),
        };
        assert!(precondition(&valid, 1));
        assert!(!precondition(&invalid, 1));
    }

    #[test]
    fn cursor_at_max_minus_one_valid() {
        let effect = Effect::SetCursor {
            offset: Offset::new(usize::MAX - 2),
        };
        assert!(precondition(&effect, usize::MAX - 1));
    }

    #[test]
    fn cursor_at_max_invalid_for_max_len() {
        // offset == doc_len is invalid for cursor (must be < doc_len).
        // Offset::MAX is usize::MAX - 1 (the largest representable offset).
        let effect = Effect::SetCursor {
            offset: Offset::MAX,
        };
        assert!(!precondition(&effect, Offset::MAX.get()));
    }

    #[test]
    fn verify_effects_mixed_variant_types_all_valid() {
        // Mixed variant types, all valid — should succeed.
        let effects = vec![
            Effect::set_mode(Mode::Insert),
            Effect::Insert {
                offset: Offset::new(0),
                text: CompactString::new("hello"),
            },
            Effect::ClearMessage,
        ];
        verify_effects(&effects, 10);
    }

    // ── bounds_check_and_clamp ───────────────────────────────────────────

    #[test]
    fn bounds_clamp_empty_slice_returns_zero() {
        assert_eq!(bounds_check_and_clamp(&mut [], 10), 0);
    }

    #[test]
    fn bounds_clamp_all_valid_returns_zero() {
        let mut effects = vec![
            Effect::Insert {
                offset: Offset::new(5),
                text: CompactString::new("x"),
            },
            Effect::SetCursor {
                offset: Offset::new(4),
            },
            Effect::Delete {
                range: Range::from_raw(2, 8),
            },
            Effect::set_mode(Mode::Normal),
        ];
        let original = effects.clone();
        assert_eq!(bounds_check_and_clamp(&mut effects, 10), 0);
        assert_eq!(effects, original, "No effect should have changed");
    }

    #[test]
    fn bounds_clamp_insert_past_end() {
        let mut effects = vec![Effect::Insert {
            offset: Offset::new(20),
            text: CompactString::new("hello"),
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::Insert { offset, .. } => assert_eq!(offset.get(), 10),
            other => panic!("Expected Insert, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_insert_at_end_not_clamped() {
        // Insert at doc_len is valid (appending).
        let mut effects = vec![Effect::Insert {
            offset: Offset::new(10),
            text: CompactString::new("x"),
        }];
        assert_eq!(bounds_check_and_clamp(&mut effects, 10), 0);
        match &effects[0] {
            Effect::Insert { offset, .. } => assert_eq!(offset.get(), 10),
            other => panic!("Expected Insert, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_set_cursor_at_doc_len() {
        // SetCursor at doc_len should be clamped to doc_len - 1.
        let mut effects = vec![Effect::SetCursor {
            offset: Offset::new(10),
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::SetCursor { offset } => assert_eq!(offset.get(), 9),
            other => panic!("Expected SetCursor, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_set_cursor_far_past_end() {
        let mut effects = vec![Effect::SetCursor {
            offset: Offset::new(1000),
        }];
        let count = bounds_check_and_clamp(&mut effects, 5);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::SetCursor { offset } => assert_eq!(offset.get(), 4),
            other => panic!("Expected SetCursor, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_set_cursor_empty_doc() {
        // In an empty doc, only offset 0 is valid.
        let mut effects = vec![Effect::SetCursor {
            offset: Offset::new(5),
        }];
        let count = bounds_check_and_clamp(&mut effects, 0);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::SetCursor { offset } => assert_eq!(offset.get(), 0),
            other => panic!("Expected SetCursor, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_set_cursor_valid_not_clamped() {
        let mut effects = vec![Effect::SetCursor {
            offset: Offset::new(4),
        }];
        assert_eq!(bounds_check_and_clamp(&mut effects, 10), 0);
    }

    #[test]
    fn bounds_clamp_scroll_to() {
        let mut effects = vec![Effect::ScrollTo {
            offset: Offset::new(10),
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::ScrollTo { offset } => assert_eq!(offset.get(), 9),
            other => panic!("Expected ScrollTo, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_delete_range_end_exceeds() {
        let mut effects = vec![Effect::Delete {
            range: Range::from_raw(5, 20),
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::Delete { range } => {
                assert_eq!(range.start().get(), 5);
                assert_eq!(range.end().get(), 10);
            }
            other => panic!("Expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_delete_both_ends_exceed() {
        let mut effects = vec![Effect::Delete {
            range: Range::from_raw(15, 20),
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::Delete { range } => {
                assert_eq!(range.start().get(), 10);
                assert_eq!(range.end().get(), 10);
            }
            other => panic!("Expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_replace_range_end_exceeds() {
        let mut effects = vec![Effect::Replace {
            range: Range::from_raw(3, 15),
            text: CompactString::new("new"),
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::Replace { range, text } => {
                assert_eq!(range.start().get(), 3);
                assert_eq!(range.end().get(), 10);
                assert_eq!(text.as_str(), "new");
            }
            other => panic!("Expected Replace, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_set_selection_anchor_out_of_bounds() {
        let mut effects = vec![Effect::SetSelection {
            anchor: Offset::new(20),
            head: Offset::new(5),
            shape: SelectionShape::Char,
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::SetSelection { anchor, head, .. } => {
                assert_eq!(anchor.get(), 9);
                assert_eq!(head.get(), 5);
            }
            other => panic!("Expected SetSelection, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_set_selection_head_out_of_bounds() {
        let mut effects = vec![Effect::SetSelection {
            anchor: Offset::new(3),
            head: Offset::new(15),
            shape: SelectionShape::Char,
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::SetSelection { anchor, head, .. } => {
                assert_eq!(anchor.get(), 3);
                assert_eq!(head.get(), 9);
            }
            other => panic!("Expected SetSelection, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_set_selection_both_out_of_bounds() {
        let mut effects = vec![Effect::SetSelection {
            anchor: Offset::new(50),
            head: Offset::new(60),
            shape: SelectionShape::Line,
        }];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 1);
        match &effects[0] {
            Effect::SetSelection { anchor, head, .. } => {
                assert_eq!(anchor.get(), 9);
                assert_eq!(head.get(), 9);
            }
            other => panic!("Expected SetSelection, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_mixed_valid_and_invalid() {
        let mut effects = vec![
            // Valid: cursor at 5 in 10-byte doc.
            Effect::SetCursor {
                offset: Offset::new(5),
            },
            // Invalid: cursor at 10 in 10-byte doc.
            Effect::SetCursor {
                offset: Offset::new(10),
            },
            // Valid: insert at end.
            Effect::Insert {
                offset: Offset::new(10),
                text: CompactString::new("x"),
            },
            // Invalid: insert past end.
            Effect::Insert {
                offset: Offset::new(20),
                text: CompactString::new("y"),
            },
            // Valid: mode change (always valid).
            Effect::set_mode(Mode::Insert),
            // Invalid: delete range end exceeds doc_len.
            Effect::Delete {
                range: Range::from_raw(5, 15),
            },
        ];
        let count = bounds_check_and_clamp(&mut effects, 10);
        assert_eq!(count, 3, "Exactly 3 effects should have been clamped");

        // Verify each effect's final state.
        match &effects[0] {
            Effect::SetCursor { offset } => assert_eq!(offset.get(), 5, "First cursor was valid"),
            other => panic!("Expected SetCursor, got {other:?}"),
        }
        match &effects[1] {
            Effect::SetCursor { offset } => {
                assert_eq!(offset.get(), 9, "Second cursor should be clamped to 9");
            }
            other => panic!("Expected SetCursor, got {other:?}"),
        }
        match &effects[2] {
            Effect::Insert { offset, .. } => {
                assert_eq!(offset.get(), 10, "Insert at end was valid")
            }
            other => panic!("Expected Insert, got {other:?}"),
        }
        match &effects[3] {
            Effect::Insert { offset, .. } => {
                assert_eq!(offset.get(), 10, "Insert past end should be clamped to 10");
            }
            other => panic!("Expected Insert, got {other:?}"),
        }
        // effects[4] is SetMode -- unchanged, not counted.
        match &effects[5] {
            Effect::Delete { range } => {
                assert_eq!(range.start().get(), 5);
                assert_eq!(range.end().get(), 10, "Delete end should be clamped to 10");
            }
            other => panic!("Expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn bounds_clamp_non_positional_effects_unchanged() {
        // Non-positional effects should never be counted as clamped.
        let mut effects = vec![
            Effect::set_mode(Mode::Normal),
            Effect::ClearMessage,
            Effect::ClearSelection,
            Effect::Noop,
        ];
        let original = effects.clone();
        assert_eq!(bounds_check_and_clamp(&mut effects, 0), 0);
        assert_eq!(effects, original);
    }

    #[test]
    fn bounds_clamp_then_precondition_passes() {
        // After clamping, all effects should pass precondition checks.
        let mut effects = vec![
            Effect::Insert {
                offset: Offset::new(100),
                text: CompactString::new("x"),
            },
            Effect::SetCursor {
                offset: Offset::new(50),
            },
            Effect::Delete {
                range: Range::from_raw(0, 30),
            },
            Effect::Replace {
                range: Range::from_raw(5, 25),
                text: CompactString::new("new"),
            },
            Effect::ScrollTo {
                offset: Offset::new(20),
            },
            Effect::SetSelection {
                anchor: Offset::new(30),
                head: Offset::new(40),
                shape: SelectionShape::Char,
            },
        ];
        let doc_len = 10;
        bounds_check_and_clamp(&mut effects, doc_len);
        // Every effect should now pass precondition.
        for (i, effect) in effects.iter().enumerate() {
            assert!(
                precondition(effect, doc_len),
                "Effect at index {i} failed precondition after clamping: {:?}",
                effect,
            );
        }
    }
}
