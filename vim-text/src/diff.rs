//! Structural diff: compare two VimText versions via Arc pointer identity.
//!
//! Exploits `Arc::ptr_eq` to skip unchanged subtrees, giving O(changed leaves)
//! performance for typical single-edit diffs on large documents.

use std::ops::Range;
use std::sync::Arc;

use crate::chunk::TextChunk;
use crate::tree::node::{Node, DEFAULT_B};
use crate::tree::traits::Summary;
use crate::tree::InternalNode;
use crate::VimText;

/// A single difference between two document versions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    /// Byte range in the OLD version.
    pub old_range: Range<usize>,
    /// Byte range in the NEW version.
    pub new_range: Range<usize>,
    /// Line range in the OLD version (0-indexed).
    pub old_line_range: Range<usize>,
    /// Line range in the NEW version (0-indexed).
    pub new_line_range: Range<usize>,
    /// Classification of the change.
    pub kind: DiffKind,
}

/// The kind of difference within a [`DiffHunk`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// Content was inserted (old range is empty).
    Insert,
    /// Content was deleted (new range is empty).
    Delete,
    /// Content was replaced (both ranges non-empty).
    Replace,
}

/// Classify a hunk based on its ranges.
fn classify(old_range: &Range<usize>, new_range: &Range<usize>) -> DiffKind {
    let old_empty = old_range.start == old_range.end;
    let new_empty = new_range.start == new_range.end;
    match (old_empty, new_empty) {
        (true, false) => DiffKind::Insert,
        (false, true) => DiffKind::Delete,
        _ => DiffKind::Replace,
    }
}

impl VimText {
    /// Compare two versions of a document, exploiting shared Arc nodes
    /// to skip unchanged subtrees. O(changed leaves).
    pub fn structural_diff(&self, other: &Self) -> Vec<DiffHunk> {
        // Fast path: same tree (Arc::ptr_eq at root)
        if Arc::ptr_eq(self.tree.root(), other.tree.root()) {
            return Vec::new();
        }

        let mut hunks = Vec::new();
        diff_recursive(self.tree.root(), other.tree.root(), 0, 0, &mut hunks);
        merge_adjacent_hunks(&mut hunks);

        // Compute line ranges from byte ranges.
        for hunk in &mut hunks {
            hunk.old_line_range = byte_range_to_line_range(self, &hunk.old_range);
            hunk.new_line_range = byte_range_to_line_range(other, &hunk.new_range);
        }

        // Filter empty-empty hunks (both old and new ranges empty).
        hunks.retain(|h| !(h.old_range.is_empty() && h.new_range.is_empty()));

        hunks
    }
}

/// Convert a byte range to a half-open line range.
///
/// For non-empty byte ranges, the end line is computed from the last byte in
/// the range (`byte_end - 1`) plus one, yielding a correct half-open interval.
/// For empty ranges (insertions/deletions at a point), both start and end map
/// to the same line, producing an empty line range.
fn byte_range_to_line_range(text: &VimText, byte_range: &Range<usize>) -> Range<usize> {
    let start_line = text.line_of_offset(byte_range.start);
    if byte_range.is_empty() {
        start_line..start_line
    } else {
        let end_line = text
            .try_line_of_offset(byte_range.end.saturating_sub(1))
            .map(|l| l + 1)
            .unwrap_or(text.line_count());
        start_line..end_line
    }
}

/// Compute the total byte length of a node from its summary.
fn node_byte_len(node: &Arc<Node<TextChunk, DEFAULT_B>>) -> usize {
    node.summary().base_len()
}

/// Recursively compare two nodes, emitting DiffHunks for regions that differ.
/// Skips subtrees that share the same `Arc` pointer (unchanged content).
fn diff_recursive(
    old: &Arc<Node<TextChunk, DEFAULT_B>>,
    new: &Arc<Node<TextChunk, DEFAULT_B>>,
    old_offset: usize,
    new_offset: usize,
    hunks: &mut Vec<DiffHunk>,
) {
    if Arc::ptr_eq(old, new) {
        return;
    }

    match (old.as_ref(), new.as_ref()) {
        (Node::Leaf { .. }, Node::Leaf { .. }) => {
            // Both are leaves with different content.
            let old_len = node_byte_len(old);
            let new_len = node_byte_len(new);
            let old_range = old_offset..old_offset + old_len;
            let new_range = new_offset..new_offset + new_len;
            let kind = classify(&old_range, &new_range);
            hunks.push(DiffHunk {
                old_range,
                new_range,
                old_line_range: 0..0,
                new_line_range: 0..0,
                kind,
            });
        }
        (Node::Internal(old_int), Node::Internal(new_int)) if old_int.height == new_int.height => {
            // Same height: try to align children by pointer identity.
            diff_internals(old_int, new_int, old_offset, new_offset, hunks);
        }
        _ => {
            // Different structure (different heights or leaf vs internal).
            // Emit one hunk covering both entire subtrees.
            let old_len = node_byte_len(old);
            let new_len = node_byte_len(new);
            let old_range = old_offset..old_offset + old_len;
            let new_range = new_offset..new_offset + new_len;
            let kind = classify(&old_range, &new_range);
            hunks.push(DiffHunk {
                old_range,
                new_range,
                old_line_range: 0..0,
                new_line_range: 0..0,
                kind,
            });
        }
    }
}

/// Diff two internal nodes at the same height by aligning their children.
///
/// Strategy: walk children left-to-right, matching pairs. For typical edits
/// (single path modified), most children are `ptr_eq` and get skipped. When
/// children counts differ (insertions/deletions shifted children), we use a
/// greedy approach with bounded lookahead to find shared anchors.
fn diff_internals(
    old_int: &InternalNode<TextChunk, DEFAULT_B>,
    new_int: &InternalNode<TextChunk, DEFAULT_B>,
    old_base: usize,
    new_base: usize,
    hunks: &mut Vec<DiffHunk>,
) {
    let old_children = &old_int.children;
    let new_children = &new_int.children;

    // Precompute cumulative byte offsets for each child.
    let old_offsets: Vec<usize> = std::iter::once(0)
        .chain(old_int.summaries.iter().scan(0usize, |acc, cs| {
            *acc += cs.base_len();
            Some(*acc)
        }))
        .collect();
    let new_offsets: Vec<usize> = std::iter::once(0)
        .chain(new_int.summaries.iter().scan(0usize, |acc, cs| {
            *acc += cs.base_len();
            Some(*acc)
        }))
        .collect();

    // Two-pointer approach with lookahead to find shared anchors.
    let mut oi = 0usize;
    let mut ni = 0usize;

    while oi < old_children.len() && ni < new_children.len() {
        if Arc::ptr_eq(&old_children[oi], &new_children[ni]) {
            // Identical subtree -- skip.
            oi += 1;
            ni += 1;
            continue;
        }

        // Try to find a shared anchor ahead.
        let anchor = find_next_anchor(old_children, new_children, oi, ni);

        match anchor {
            Some((anchor_oi, anchor_ni)) => {
                // Everything in old[oi..anchor_oi] and new[ni..anchor_ni] differs.
                let old_span_start = old_base + old_offsets[oi];
                let old_span_end = old_base + old_offsets[anchor_oi];
                let new_span_start = new_base + new_offsets[ni];
                let new_span_end = new_base + new_offsets[anchor_ni];

                // If both sides have exactly one child, recurse for finer granularity.
                if anchor_oi - oi == 1 && anchor_ni - ni == 1 {
                    diff_recursive(
                        &old_children[oi],
                        &new_children[ni],
                        old_span_start,
                        new_span_start,
                        hunks,
                    );
                } else {
                    // Multiple children differ -- emit one hunk for the whole span.
                    let old_range = old_span_start..old_span_end;
                    let new_range = new_span_start..new_span_end;
                    let kind = classify(&old_range, &new_range);
                    hunks.push(DiffHunk {
                        old_range,
                        new_range,
                        old_line_range: 0..0,
                        new_line_range: 0..0,
                        kind,
                    });
                }

                oi = anchor_oi;
                ni = anchor_ni;
            }
            None => {
                // No more anchors -- everything remaining differs.
                let old_span_start = old_base + old_offsets[oi];
                let old_span_end = old_base + old_offsets[old_children.len()];
                let new_span_start = new_base + new_offsets[ni];
                let new_span_end = new_base + new_offsets[new_children.len()];

                // If both sides have exactly one child remaining, recurse.
                if old_children.len() - oi == 1 && new_children.len() - ni == 1 {
                    diff_recursive(
                        &old_children[oi],
                        &new_children[ni],
                        old_span_start,
                        new_span_start,
                        hunks,
                    );
                } else {
                    let old_range = old_span_start..old_span_end;
                    let new_range = new_span_start..new_span_end;
                    let kind = classify(&old_range, &new_range);
                    hunks.push(DiffHunk {
                        old_range,
                        new_range,
                        old_line_range: 0..0,
                        new_line_range: 0..0,
                        kind,
                    });
                }
                oi = old_children.len();
                ni = new_children.len();
            }
        }
    }

    // Handle trailing children on one side (insertions or deletions).
    if oi < old_children.len() {
        let old_span_start = old_base + old_offsets[oi];
        let old_span_end = old_base + old_offsets[old_children.len()];
        let new_span_pos = new_base + new_offsets[ni];
        hunks.push(DiffHunk {
            old_range: old_span_start..old_span_end,
            new_range: new_span_pos..new_span_pos,
            old_line_range: 0..0,
            new_line_range: 0..0,
            kind: DiffKind::Delete,
        });
    }
    if ni < new_children.len() {
        let old_span_pos = old_base + old_offsets[oi];
        let new_span_start = new_base + new_offsets[ni];
        let new_span_end = new_base + new_offsets[new_children.len()];
        hunks.push(DiffHunk {
            old_range: old_span_pos..old_span_pos,
            new_range: new_span_start..new_span_end,
            old_line_range: 0..0,
            new_line_range: 0..0,
            kind: DiffKind::Insert,
        });
    }
}

/// Find the next pair of indices (oi2, ni2) where old[oi2] ptr_eq new[ni2],
/// searching from (oi_start, ni_start) onwards. Uses bounded lookahead to
/// avoid quadratic behavior on pathological inputs.
fn find_next_anchor(
    old_children: &[Arc<Node<TextChunk, DEFAULT_B>>],
    new_children: &[Arc<Node<TextChunk, DEFAULT_B>>],
    oi_start: usize,
    ni_start: usize,
) -> Option<(usize, usize)> {
    const MAX_LOOKAHEAD: usize = 8;

    let old_limit = old_children.len().min(oi_start + MAX_LOOKAHEAD + 1);
    let new_limit = new_children.len().min(ni_start + MAX_LOOKAHEAD + 1);

    // Search in a diagonal pattern: prefer anchors that are close and balanced.
    for dist in 1..MAX_LOOKAHEAD {
        // Check (oi_start + dist, ni_start + dist) first -- the common case
        // where one child was modified in-place.
        let oi2 = oi_start + dist;
        let ni2 = ni_start + dist;
        if oi2 < old_limit && ni2 < new_limit && Arc::ptr_eq(&old_children[oi2], &new_children[ni2])
        {
            return Some((oi2, ni2));
        }

        // Check off-diagonal: insertion (new has extra child).
        for d in 0..dist {
            let oi2 = oi_start + d + 1;
            let ni2 = ni_start + dist;
            if oi2 < old_limit
                && ni2 < new_limit
                && Arc::ptr_eq(&old_children[oi2], &new_children[ni2])
            {
                return Some((oi2, ni2));
            }
        }

        // Check off-diagonal: deletion (old has extra child).
        for d in 0..dist {
            let oi2 = oi_start + dist;
            let ni2 = ni_start + d + 1;
            if oi2 < old_limit
                && ni2 < new_limit
                && Arc::ptr_eq(&old_children[oi2], &new_children[ni2])
            {
                return Some((oi2, ni2));
            }
        }
    }

    None
}

/// Merge adjacent hunks that are contiguous (the end of one equals the start
/// of the next on both sides). This collapses fragmented output from
/// multi-child diffs into larger, cleaner hunks.
fn merge_adjacent_hunks(hunks: &mut Vec<DiffHunk>) {
    if hunks.len() <= 1 {
        return;
    }
    let mut merged = Vec::with_capacity(hunks.len());
    merged.push(hunks[0].clone());
    for hunk in hunks.iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if last.old_range.end == hunk.old_range.start && last.new_range.end == hunk.new_range.start
        {
            last.old_range.end = hunk.old_range.end;
            last.new_range.end = hunk.new_range.end;
            // Reclassify the merged hunk.
            last.kind = classify(&last.old_range, &last.new_range);
        } else {
            merged.push(hunk.clone());
        }
    }
    *hunks = merged;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changeset::Change;
    use crate::ChangeSet;

    #[test]
    fn identical_trees_no_hunks() {
        let a = VimText::from_str("hello world");
        let b = a.clone(); // same Arc
        assert_eq!(a.structural_diff(&b), vec![]);
    }

    #[test]
    fn completely_different() {
        let a = VimText::from_str("hello");
        let b = VimText::from_str("world");
        let hunks = a.structural_diff(&b);
        assert!(!hunks.is_empty());
    }

    #[test]
    fn single_edit_produces_localized_hunk() {
        let a = VimText::from_str("hello world");
        let mut b = a.snapshot();
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 5,
                end: 5,
                text: " beautiful".into(),
            }],
        );
        b.apply(&cs);
        let hunks = a.structural_diff(&b);
        // Should have few hunks, localized to the changed region
        assert!(!hunks.is_empty());
        assert!(hunks.len() <= 3); // at most a few hunks for a single edit
    }

    #[test]
    fn diff_symmetry() {
        let a = VimText::from_str("hello world");
        let mut b = a.snapshot();
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 6,
                end: 11,
                text: "rust".into(),
            }],
        );
        b.apply(&cs);
        let forward = a.structural_diff(&b);
        let backward = b.structural_diff(&a);
        assert_eq!(forward.len(), backward.len());
    }

    #[test]
    fn empty_to_content() {
        let a = VimText::new();
        let b = VimText::from_str("hello");
        let hunks = a.structural_diff(&b);
        assert!(!hunks.is_empty());
    }

    #[test]
    fn content_to_empty() {
        let a = VimText::from_str("hello");
        let b = VimText::new();
        let hunks = a.structural_diff(&b);
        assert!(!hunks.is_empty());
    }

    #[test]
    fn diff_kind_correct() {
        let a = VimText::from_str("hello");
        let b = VimText::from_str("world");
        let hunks = a.structural_diff(&b);
        // All hunks should be Replace (both have content)
        for hunk in &hunks {
            assert_eq!(hunk.kind, DiffKind::Replace);
        }
    }

    #[test]
    fn insert_kind_empty_old() {
        let a = VimText::new();
        let b = VimText::from_str("hello");
        let hunks = a.structural_diff(&b);
        // There should be at least one Insert or Replace hunk
        let has_insert_or_replace = hunks
            .iter()
            .any(|h| h.kind == DiffKind::Insert || h.kind == DiffKind::Replace);
        assert!(has_insert_or_replace);
    }

    #[test]
    fn delete_kind_empty_new() {
        let a = VimText::from_str("hello");
        let b = VimText::new();
        let hunks = a.structural_diff(&b);
        // There should be at least one Delete or Replace hunk
        let has_delete_or_replace = hunks
            .iter()
            .any(|h| h.kind == DiffKind::Delete || h.kind == DiffKind::Replace);
        assert!(has_delete_or_replace);
    }

    #[test]
    fn large_document_shared_subtrees() {
        // Build a large document, snapshot, make a small edit.
        // The diff should be localized.
        let text: String = "line\n".repeat(1000);
        let a = VimText::from_str(&text);
        let mut b = a.snapshot();
        let cs = ChangeSet::from_changes(
            a.byte_len(),
            vec![Change {
                start: 2500,
                end: 2500,
                text: "INSERTED".into(),
            }],
        );
        b.apply(&cs);
        let hunks = a.structural_diff(&b);
        assert!(!hunks.is_empty());
        // For a single insert in a large doc, we expect very few hunks
        // (the structural diff walks O(height) nodes).
        assert!(
            hunks.len() <= 10,
            "expected few hunks for single insert, got {}",
            hunks.len()
        );
    }

    #[test]
    fn hunk_ranges_cover_changes() {
        let a = VimText::from_str("hello world");
        let mut b = a.snapshot();
        let cs = ChangeSet::from_changes(
            11,
            vec![Change {
                start: 6,
                end: 11,
                text: "rust".into(),
            }],
        );
        b.apply(&cs);
        let hunks = a.structural_diff(&b);

        // The changed region is old[6..11] -> new[6..10].
        // At least one hunk should overlap with this region.
        let has_overlapping = hunks
            .iter()
            .any(|h| h.old_range.start <= 11 && h.old_range.end >= 6);
        assert!(has_overlapping);
    }

    #[test]
    fn diff_of_snapshots_after_multiple_edits() {
        let a = VimText::from_str("the quick brown fox");
        let snap_a = a.snapshot();

        let mut b = a.clone();
        let cs1 = ChangeSet::from_changes(
            19,
            vec![Change {
                start: 4,
                end: 9,
                text: "slow".into(),
            }],
        );
        b.apply(&cs1);

        let cs2 = ChangeSet::from_changes(
            b.byte_len(),
            vec![Change {
                start: 14,
                end: 17,
                text: "cat".into(),
            }],
        );
        b.apply(&cs2);

        let hunks = snap_a.structural_diff(&b);
        assert!(!hunks.is_empty());
    }

    #[test]
    fn identical_content_different_trees() {
        // Two trees built independently from the same string.
        // They share no Arc pointers, so diff will report hunks covering
        // everything (structural diff is pointer-based, not content-based).
        let a = VimText::from_str("hello world");
        let b = VimText::from_str("hello world");
        let hunks = a.structural_diff(&b);
        // They are NOT ptr_eq at root, so we get hunks even though text is same.
        assert!(!hunks.is_empty());
    }

    #[test]
    fn diff_hunk_has_line_ranges() {
        // "line0\nline1\nline2\nline3"
        let a = VimText::from_str("line0\nline1\nline2\nline3");
        let mut b = a.snapshot();
        // Replace "line1" (bytes 6..11) with "REPLACED"
        let cs = ChangeSet::from_changes(
            a.byte_len(),
            vec![Change {
                start: 6,
                end: 11,
                text: "REPLACED".into(),
            }],
        );
        b.apply(&cs);

        let hunks = a.structural_diff(&b);
        assert!(!hunks.is_empty());

        // At least one hunk should have line ranges that cover line 1 in the old doc.
        let covers_line_1 = hunks
            .iter()
            .any(|h| h.old_line_range.start <= 1 && h.old_line_range.end >= 1);
        assert!(
            covers_line_1,
            "expected a hunk covering old line 1, got: {:?}",
            hunks
        );

        // The new_line_range should also be populated (non-default for non-empty hunks).
        for hunk in &hunks {
            // All hunks should have line ranges computed (not all zeros for Replace hunks).
            if hunk.kind == DiffKind::Replace {
                let has_old_lines = hunk.old_line_range.start != 0 || hunk.old_line_range.end != 0;
                let has_new_lines = hunk.new_line_range.start != 0 || hunk.new_line_range.end != 0;
                assert!(
                    has_old_lines || has_new_lines,
                    "Replace hunk should have non-trivial line ranges: {:?}",
                    hunk
                );
            }
        }
    }

    #[test]
    fn diff_hunk_line_range_single_line() {
        // structural_diff works at chunk granularity. For a multi-chunk doc,
        // only the changed chunk gets a hunk. Build text large enough for
        // multiple chunks so we can test single-line precision.
        let mut old_text = String::new();
        let mut new_text = String::new();
        // First chunk: ~1000 bytes of "aaa\n" lines
        for _ in 0..250 {
            old_text.push_str("aaa\n");
            new_text.push_str("aaa\n");
        }
        // Second chunk: starts with the change
        old_text.push_str("bbb\n");
        new_text.push_str("XXX\n");
        for _ in 0..250 {
            old_text.push_str("ccc\n");
            new_text.push_str("ccc\n");
        }
        let old = VimText::from_str(&old_text);
        let new_vt = VimText::from_str(&new_text);
        let hunks = old.structural_diff(&new_vt);
        assert!(!hunks.is_empty(), "should detect the change");
        // The hunk covering the changed chunk should have non-empty line range
        let h = &hunks[0];
        assert!(
            h.old_line_range.end > h.old_line_range.start,
            "line range should not be empty: {:?}",
            h.old_line_range
        );
        // Line 250 is where the change is — it should be within the hunk's range
        assert!(
            h.old_line_range.start <= 250 && h.old_line_range.end > 250,
            "changed line 250 should be in range {:?}",
            h.old_line_range
        );
    }

    #[test]
    fn diff_no_empty_empty_hunks() {
        let a = VimText::new();
        let b = VimText::new();
        let hunks = a.structural_diff(&b);
        for h in &hunks {
            assert!(
                !h.old_range.is_empty() || !h.new_range.is_empty(),
                "Found empty-empty hunk"
            );
        }
    }
}
