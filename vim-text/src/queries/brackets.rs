//! Bracket-matching helper functions for VimText.
//!
//! Extracted from `blank_lines.rs`. Called by the `VimQueries` impl.
//! All functions take `&VimText` and return results — they do not define traits.
//!
//! ## Complexity
//!
//! `find_matching_forward` and `find_matching_backward` use **subtree-level
//! pruning** via `Cursor::search_forward` / `search_backward` with a
//! `BracketPair {delta, min, max}` filter. At every tree level, the filter
//! tests whether the running depth can reach the target (0) within that
//! subtree. If not, the entire subtree is skipped — no byte-level scan.
//! This reduces matching to O(height * B + CHUNK_MAX) ~ O(log n).

use crate::chunk::TextChunk;
use crate::summary::{BracketSummary, ByteOffset};
use crate::tree::sum_tree::SumTree;
use crate::tree::Bias;
use crate::VimText;

/// Read the byte at a given offset via cursor seek — O(log n).
///
/// Uses `Bias::Right` so that when `offset` falls exactly on a chunk boundary,
/// the cursor lands on the chunk that *starts* at that offset.
pub(crate) fn byte_at_offset(text: &VimText, offset: usize) -> Option<u8> {
    if offset >= text.byte_len() {
        return None;
    }
    let mut cursor = text.tree.cursor::<ByteOffset>();
    cursor.seek(&ByteOffset(offset as u32), Bias::Right);
    let chunk = cursor.item()?;
    let chunk_start = cursor.start::<ByteOffset>().0 as usize;
    Some(chunk.as_bytes()[offset - chunk_start])
}

/// Select the `BracketPair` from a `BracketSummary` that corresponds to the
/// given open bracket byte.
#[inline]
fn pair_for_open(bs: &BracketSummary, open: u8) -> (i16, i16, i16) {
    let p = match open {
        b'(' => &bs.paren,
        b'[' => &bs.bracket,
        b'{' => &bs.brace,
        _ => unreachable!(),
    };
    (p.delta, p.min, p.max)
}

/// Search forward from `start_offset` (exclusive) for the matching close bracket.
/// `open` and `close` are the bracket bytes for the same type (e.g. `(` and `)`).
///
/// Uses subtree-level pruning via `cursor.search_forward`:
/// - Seek to the chunk containing `start_offset` — O(log n)
/// - Scan the partial first chunk tail byte-by-byte (unavoidable)
/// - `search_forward` tests each subtree's `BracketSummary.min` at every tree
///   level: if `running_depth + min <= 0`, descend (match may be inside);
///   otherwise skip the entire subtree and add `delta` to `running_depth`.
/// - When a leaf is reached, scan it byte-by-byte for the exact match.
/// - Total: O(height * B + CHUNK_MAX) ~ O(log n).
pub(crate) fn find_matching_forward(
    text: &VimText,
    start_offset: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut cursor = text.tree.cursor::<ByteOffset>();
    cursor.seek(&ByteOffset(start_offset as u32), Bias::Right);

    // scan_from: byte right after the bracket at start_offset
    let scan_from = start_offset + 1;

    // --- First chunk: always scan the partial tail (from scan_from to chunk end) ---
    {
        let chunk = cursor.item()?;
        let chunk_start = cursor.start::<ByteOffset>().0 as usize;
        let chunk_bytes = chunk.as_bytes();
        let local_start = scan_from.saturating_sub(chunk_start);

        for (i, &b) in chunk_bytes[local_start..].iter().enumerate() {
            if b == open {
                depth += 1;
            } else if b == close {
                if depth == 0 {
                    return Some(chunk_start + local_start + i);
                }
                depth -= 1;
            }
        }
    }

    // --- Remaining tree: subtree-level pruning via search_forward ---
    //
    // Each call to search_forward resumes from the current cursor position.
    // The filter tests each subtree's BracketSummary.min: if depth + min <= 0,
    // the match MAY be inside — descend. Otherwise skip the subtree and add
    // its delta to the running depth. When search_forward lands on a leaf,
    // we scan byte-by-byte. If the leaf was a false positive (min was low
    // enough due to a different bracket type, or depth recovered), we loop
    // and call search_forward again.
    loop {
        let found = cursor.search_forward(|summary: &_| {
            let (delta, min, _max) = pair_for_open(&summary.brackets, open);
            if depth + i32::from(min) <= 0 {
                true // match MAY be in this subtree — descend
            } else {
                depth += i32::from(delta); // skip subtree
                false
            }
        });

        if !found {
            return None;
        }

        // search_forward landed on a leaf chunk. Scan byte-by-byte.
        let chunk = cursor.item()?;
        let chunk_start = cursor.start::<ByteOffset>().0 as usize;
        let chunk_bytes = chunk.as_bytes();

        for (i, &b) in chunk_bytes.iter().enumerate() {
            if b == open {
                depth += 1;
            } else if b == close {
                if depth == 0 {
                    return Some(chunk_start + i);
                }
                depth -= 1;
            }
        }
    }
}

/// Search backward from `start_offset` (exclusive) for the matching open bracket.
/// `open` and `close` are the bracket bytes for the same type (e.g. `(` and `)`).
///
/// Uses subtree-level pruning via `cursor.search_backward`:
/// - Seek to the chunk containing `start_offset` — O(log n)
/// - Scan the partial first chunk backward (from `start_offset` to chunk start)
/// - `search_backward` tests each subtree's summary at every tree level using
///   the backward pruning derivation below. Subtrees that cannot contain the
///   match are skipped entirely.
/// - When a leaf is reached, scan it backward byte-by-byte for the exact match.
/// - Total: O(height * B + CHUNK_MAX) ~ O(log n).
///
/// ### Pruning derivation (backward)
///
/// Walking R→L through a subtree whose L→R profile has `(delta, min, max)`:
/// Let `d[i]` be the cumulative L→R depth after processing byte `i`, with `d[-1]=0`
/// (before any bytes) and `d[n-1]=delta`. The summary's `min` and `max` track the
/// running extremes of `d[i]` during L→R traversal (including `d[-1]=0`).
///
/// Entering from the right with `running_depth = D`, the depth upon arrival at
/// L→R position `i` (having processed bytes `[n-1, ..., i+1]` in R→L order) is
/// `D + (d[i] - delta)`.
///
/// We need this to equal 0 at some `(` byte: `d[i] = delta - D`.
/// For a solution to exist: `min <= delta - D <= max`.
/// After fully skipping (R→L traversal): `D_new = D + (0 - delta) = D - delta`.
pub(crate) fn find_matching_backward(
    text: &VimText,
    start_offset: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut cursor = text.tree.cursor::<ByteOffset>();
    cursor.seek(&ByteOffset(start_offset as u32), Bias::Right);

    // scan_up_to: the exclusive upper bound within the current chunk
    let scan_up_to = start_offset;

    // --- First chunk: always scan the partial head (from chunk start to scan_up_to) ---
    {
        let chunk = cursor.item()?;
        let chunk_start = cursor.start::<ByteOffset>().0 as usize;
        let chunk_bytes = chunk.as_bytes();
        let local_end = (scan_up_to - chunk_start).min(chunk_bytes.len());

        for i in (0..local_end).rev() {
            let b = chunk_bytes[i];
            if b == close {
                depth += 1;
            } else if b == open {
                if depth == 0 {
                    return Some(chunk_start + i);
                }
                depth -= 1;
            }
        }
    }

    // --- Remaining tree: subtree-level pruning via search_backward ---
    //
    // Each call to search_backward resumes from the current cursor position.
    // The filter tests each subtree's BracketSummary using the backward
    // derivation: if `min <= delta - depth <= max`, the match MAY be inside —
    // descend. Otherwise skip the subtree and subtract its delta from depth.
    // When search_backward lands on a leaf, we scan byte-by-byte in reverse.
    // If the leaf was a false positive, we loop and call search_backward again.
    loop {
        let found = cursor.search_backward(|summary: &_| {
            let (delta, min, max) = pair_for_open(&summary.brackets, open);
            let target = i32::from(delta) - depth;
            if target >= i32::from(min) && target <= i32::from(max) {
                true // match MAY be in this subtree — descend
            } else {
                depth -= i32::from(delta); // skip subtree
                false
            }
        });

        if !found {
            return None;
        }

        // search_backward landed on a leaf chunk. Scan backward byte-by-byte.
        let chunk = cursor.item()?;
        let chunk_bytes = chunk.as_bytes();
        let chunk_start = cursor.start::<ByteOffset>().0 as usize;

        for i in (0..chunk_bytes.len()).rev() {
            let b = chunk_bytes[i];
            if b == close {
                depth += 1;
            } else if b == open {
                if depth == 0 {
                    return Some(chunk_start + i);
                }
                depth -= 1;
            }
        }
    }
}

/// Compute parenthesis nesting depth at a byte offset.  O(log n + CHUNK_MAX).
///
/// Uses `cursor.prefix_summary()` for an O(height * B) prefix computation
/// that gives the cumulative bracket state up to the start of the boundary
/// chunk, then scans only within that chunk from its start to `offset`.
pub(crate) fn compute_bracket_depth(tree: &SumTree<TextChunk>, offset: usize) -> i16 {
    let mut cursor = tree.cursor::<ByteOffset>();
    cursor.seek(&ByteOffset(offset as u32), Bias::Right);

    // O(height * B): prefix summary gives bracket state up to chunk start.
    let prefix = cursor.prefix_summary();
    let mut depth = prefix.brackets.paren.delta;

    // O(CHUNK_MAX): scan within the boundary chunk from chunk_start to offset.
    if let Some(chunk) = cursor.item() {
        let chunk_start = cursor.start::<ByteOffset>().0 as usize;
        let local_end = offset.saturating_sub(chunk_start);
        for &byte in &chunk.as_bytes()[..local_end] {
            match byte {
                b'(' => depth = depth.saturating_add(1),
                b')' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    depth
}
