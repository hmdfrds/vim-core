//! Shared offset-adjustment logic for document edits.
//!
//! When text is inserted or deleted, stored byte offsets (changelist entries,
//! marks, etc.) must be shifted to remain valid. This module provides the
//! canonical implementation of that adjustment, used by both `changelist.rs`
//! and `marks.rs` to avoid duplicated arithmetic.
//!
//! # Layering
//!
//! Imports `primitives` and `std`; must not import `commands`, `effects`,
//! `execution` or `dispatch`. State modules are pure data containers with
//! no execution logic.

/// Adjust a single byte offset in response to a document edit.
///
/// Given an offset `val` and an edit at position `pos` that removed `old_len`
/// bytes and inserted `new_len` bytes, returns the adjusted offset.
///
/// # Rules
///
/// - Offsets at or before `pos` are unchanged.
/// - Offsets inside the deleted region `(pos, pos + old_len)` are clamped to `pos`.
/// - Offsets after the edit are shifted by `(new_len - old_len)`.
///
/// # Parameters
///
/// - `val`: the byte offset to adjust
/// - `pos`: where the edit occurred
/// - `old_len`: bytes removed at `pos`
/// - `delta`: `new_len as isize - old_len as isize` (pre-computed by caller)
/// Its only caller, `MarkState::adjust_insert_stop`, is itself `#[cfg(test)]`
/// (the effect processor now goes through `adjust_insert_stop_line_col`), so
/// this helper is compiled only into test builds.
#[inline]
#[cfg(test)]
pub(super) const fn adjust_offset(val: usize, pos: usize, old_len: usize, delta: isize) -> usize {
    if val <= pos {
        return val;
    }
    // Inside deleted region — clamp to edit position
    if old_len > 0 && val < pos + old_len {
        return pos;
    }
    // After the edit — shift by delta
    val.saturating_add_signed(delta)
}

/// Like `adjust_offset`, but uses a *strict* less-than for the before-edit
/// check.  This means an offset *exactly at* the insertion point will shift
/// forward, which matches Neovim's behaviour for named marks: the mark tracks
/// the character it was set on, so inserting text at the mark pushes it right.
#[inline]
pub(super) const fn adjust_offset_named(
    val: usize,
    pos: usize,
    old_len: usize,
    delta: isize,
) -> usize {
    if val < pos {
        return val;
    }
    // Inside deleted region — clamp to edit position
    if old_len > 0 && val < pos + old_len {
        return pos;
    }
    // At or after the edit — shift by delta
    val.saturating_add_signed(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_before_edit_unchanged() {
        assert_eq!(adjust_offset(10, 30, 0, 5), 10);
    }

    #[test]
    fn offset_at_edit_position_unchanged() {
        assert_eq!(adjust_offset(30, 30, 0, 5), 30);
    }

    #[test]
    fn offset_after_insert_shifts_forward() {
        // Insert 5 bytes at position 30
        assert_eq!(adjust_offset(50, 30, 0, 5), 55);
    }

    #[test]
    fn offset_after_delete_shifts_backward() {
        // Delete 20 bytes at position 30 (old_len=20, new_len=0, delta=-20)
        assert_eq!(adjust_offset(100, 30, 20, -20), 80);
    }

    #[test]
    fn offset_inside_deleted_region_clamped() {
        // Delete 20 bytes at position 30 — offset 45 is inside [30, 50)
        assert_eq!(adjust_offset(45, 30, 20, -20), 30);
    }

    #[test]
    fn offset_at_boundary_of_deleted_region() {
        // Delete 20 bytes at position 30 — offset 50 is at the boundary
        assert_eq!(adjust_offset(50, 30, 20, -20), 30);
    }

    #[test]
    fn replace_shorter_shifts_backward() {
        // Replace 10 bytes at position 30 with 3 bytes (delta = -7)
        assert_eq!(adjust_offset(50, 30, 10, -7), 43);
    }

    #[test]
    fn replace_longer_shifts_forward() {
        // Replace 3 bytes at position 30 with 10 bytes (delta = +7)
        assert_eq!(adjust_offset(50, 30, 3, 7), 57);
    }

    #[test]
    fn zero_delta_no_change() {
        // Replace 5 bytes with 5 bytes (delta = 0)
        assert_eq!(adjust_offset(50, 30, 5, 0), 50);
    }

    #[test]
    fn saturating_sub_prevents_underflow() {
        // Extreme case: delete far more than offset can absorb
        assert_eq!(adjust_offset(5, 2, 0, -100), 0);
    }
}
