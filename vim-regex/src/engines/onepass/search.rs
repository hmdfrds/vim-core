//! Anchored search loop for the one-pass DFA.
//!
//! Takes a start position and a mutable slots array, traverses the
//! one-pass DFA transition table recording capture positions along
//! the way. Always anchored: starts at a specific position and
//! does not scan forward for a match.

use smallvec::SmallVec;

use crate::common::MAX_CAPTURE_GROUPS;
use crate::VimMatch;

use super::build::OnePassData;

/// Scratch space for one-pass DFA searches.
/// Stored in `Cache::onepass` and reused across searches.
#[derive(Debug, Clone)]
pub(crate) struct OnePassScratch {
    /// Slot values (positions). Index = slot number (0..slot_count).
    /// Even slots = group open, odd slots = group close.
    slots: Vec<Option<usize>>,
    /// Saved match slots (cloned on each match state visit).
    saved_slots: Vec<Option<usize>>,
}

impl OnePassScratch {
    pub(crate) fn new(slot_count: usize) -> Self {
        Self {
            slots: vec![None; slot_count],
            saved_slots: vec![None; slot_count],
        }
    }
}

/// Run an anchored one-pass DFA search starting at `pos` in `text`.
///
/// Returns `Some(VimMatch)` if the pattern matches at `pos`, with
/// full capture group information. Returns `None` if no match at `pos`.
pub(crate) fn search_anchored(
    data: &OnePassData,
    scratch: &mut OnePassScratch,
    text: &str,
    pos: usize,
) -> Option<VimMatch> {
    let slot_count = scratch.slots.len();

    // Clear scratch slots.
    for slot in scratch.slots.iter_mut() {
        *slot = None;
    }

    let bytes = text.as_bytes();
    let mut state = data.start;
    let mut match_pos: Option<usize> = None;
    let mut at = pos;

    // Check if start state is a match state.
    if state >= data.min_match_id {
        let match_idx = (state - data.min_match_id) as usize;
        if let Some(&slot_mask) = data.match_slots.get(match_idx) {
            apply_slots(slot_mask, at, &mut scratch.slots);
        }
        match_pos = Some(at);
        scratch.saved_slots[..slot_count].copy_from_slice(&scratch.slots[..slot_count]);
    }

    while at < bytes.len() {
        // === ASCII fast path ===
        let byte = bytes[at];
        let (c, char_len) = if byte < 0x80 {
            (byte as char, 1)
        } else {
            // Decode UTF-8 character.
            match text[at..].chars().next() {
                Some(c) => (c, c.len_utf8()),
                None => break, // Invalid UTF-8 at boundary; stop.
            }
        };

        let class = data.classifier.char_class(c);
        let offset = (state as usize) << data.classifier.stride2();
        let idx = offset + class as usize;
        if idx >= data.table.len() {
            break;
        }
        let trans = data.table[idx];

        if trans.is_dead() {
            break;
        }

        // Apply slot saves from this transition.
        let slot_mask = trans.slot_mask();
        if slot_mask != 0 {
            apply_slots(slot_mask, at, &mut scratch.slots);
        }

        state = trans.state_id();
        at += char_len;

        // Check if new state is a match state.
        if state >= data.min_match_id {
            // Apply match-state epsilon slots (recorded at build time).
            let match_idx = (state - data.min_match_id) as usize;
            if let Some(&ms) = data.match_slots.get(match_idx) {
                apply_slots(ms, at, &mut scratch.slots);
            }
            // Save the match.
            match_pos = Some(at);
            scratch.saved_slots[..slot_count].copy_from_slice(&scratch.slots[..slot_count]);

            // Leftmost-first: if match_wins, stop here.
            if trans.match_wins() {
                break;
            }
        }
    }

    // If we ended on a match state but haven't recorded it yet (end of input).
    if state >= data.min_match_id && match_pos.is_none() {
        let match_idx = (state - data.min_match_id) as usize;
        if let Some(&ms) = data.match_slots.get(match_idx) {
            apply_slots(ms, at, &mut scratch.slots);
        }
        match_pos = Some(at);
        scratch.saved_slots[..slot_count].copy_from_slice(&scratch.slots[..slot_count]);
    }

    let end = match_pos?;
    build_vim_match(pos, end, &scratch.saved_slots)
}

/// Apply a slot bitmask: for each set bit i, record `pos` into `slots[i]`.
#[inline]
fn apply_slots(mask: u32, pos: usize, slots: &mut [Option<usize>]) {
    let mut m = mask;
    while m != 0 {
        let i = m.trailing_zeros() as usize;
        if i < slots.len() {
            slots[i] = Some(pos);
        }
        m &= m - 1; // Clear lowest set bit.
    }
}

/// Convert slot array to `VimMatch`.
fn build_vim_match(
    match_start: usize,
    match_end: usize,
    slots: &[Option<usize>],
) -> Option<VimMatch> {
    use std::ops::Range;

    // Slots 0,1 are the match override positions (from \zs/\ze Save transitions).
    // If present, they narrow the reported range.
    let range_start = slots.first().copied().flatten().unwrap_or(match_start);
    let range_end = slots.get(1).copied().flatten().unwrap_or(match_end);

    // Build capture groups from slot pairs (2,3), (4,5), ...
    let mut captures: SmallVec<[Option<Range<usize>>; MAX_CAPTURE_GROUPS]> = SmallVec::new();
    let mut i = 2;
    while i + 1 < slots.len() {
        let open = slots[i];
        let close = slots[i + 1];
        captures.push(match (open, close) {
            (Some(s), Some(e)) => Some(s..e),
            _ => None,
        });
        i += 2;
    }

    Some(VimMatch::new(
        range_start..range_end,
        match_start..match_end,
        captures,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_slots_sets_bits() {
        let mut slots = vec![None; 8];
        apply_slots(0b0000_0101, 42, &mut slots);
        assert_eq!(slots[0], Some(42));
        assert_eq!(slots[1], None);
        assert_eq!(slots[2], Some(42));
        assert_eq!(slots[3], None);
    }

    #[test]
    fn apply_slots_empty_mask() {
        let mut slots = vec![None; 4];
        apply_slots(0, 42, &mut slots);
        assert!(slots.iter().all(|s| s.is_none()));
    }

    #[test]
    fn build_vim_match_no_captures() {
        let slots: Vec<Option<usize>> = vec![];
        let m = build_vim_match(0, 5, &slots).unwrap();
        assert_eq!(m.range, 0..5);
        assert_eq!(m.full_range, 0..5);
        assert!(m.captures.is_empty());
    }

    #[test]
    fn build_vim_match_with_overrides() {
        // Slots 0,1 override the reported range.
        let slots = vec![Some(2), Some(4)];
        let m = build_vim_match(0, 6, &slots).unwrap();
        assert_eq!(m.range, 2..4);
        assert_eq!(m.full_range, 0..6);
    }

    #[test]
    fn build_vim_match_with_captures() {
        // Slots: [override_start, override_end, cap1_open, cap1_close, cap2_open, cap2_close]
        let slots = vec![None, None, Some(0), Some(3), Some(4), Some(7)];
        let m = build_vim_match(0, 7, &slots).unwrap();
        assert_eq!(m.range, 0..7);
        assert_eq!(m.captures.len(), 2);
        assert_eq!(m.captures[0], Some(0..3));
        assert_eq!(m.captures[1], Some(4..7));
    }
}
