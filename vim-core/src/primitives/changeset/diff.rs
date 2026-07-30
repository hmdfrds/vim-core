//! Diff-based changeset computation.
//!
//! Given two strings (before and after), compute the `ChangeSet` that
//! transforms `before` into `after`. Uses common prefix/suffix byte
//! trimming — optimal for single-contiguous-change diffs (the common
//! case for formatter output, file reload, and paste operations).
//!
//! # UTF-8 Safety
//!
//! The byte-level prefix/suffix scanner may land mid-character. Both
//! boundaries are snapped to the nearest char boundary before constructing
//! the changeset, ensuring all slices are valid `&str`.

use super::change_set::ChangeSet;

impl ChangeSet {
    /// Compute a `ChangeSet` that transforms `before` into `after`.
    ///
    /// Uses common prefix/suffix byte trimming to identify the changed
    /// region. This produces a minimal `ChangeSet` for texts with a single
    /// contiguous change (the common case for formatters, file reloads,
    /// and paste operations).
    ///
    /// For texts with multiple scattered changes, the result is valid
    /// but may not be byte-optimal — a single replacement covers the
    /// entire span between the first and last differing byte.
    ///
    /// # Algorithm
    ///
    /// 1. Find common prefix (identical bytes from the start)
    /// 2. Find common suffix (identical bytes from the end, after prefix)
    /// 3. Snap both boundaries to UTF-8 char boundaries
    /// 4. The middle is a replacement: `Delete(before_mid) + Insert(after_mid)`
    ///
    /// # Complexity
    ///
    /// O(N) where N = max(before.len(), after.len())
    ///
    /// # Guarantee
    ///
    /// `ChangeSet::from_diff(a, b).apply(a) == Ok(b.to_string())` — always.
    #[must_use]
    #[allow(
        clippy::indexing_slicing,
        reason = "all indices are bounded by string lengths and snapped to char boundaries"
    )]
    pub fn from_diff(before: &str, after: &str) -> Self {
        let before_b = before.as_bytes();
        let after_b = after.as_bytes();

        // ── Step 1: Raw common prefix ────────────────────────────────
        let raw_prefix = before_b
            .iter()
            .zip(after_b.iter())
            .take_while(|(a, b)| a == b)
            .count();

        // ── Step 2: Snap prefix to char boundary ─────────────────────
        // Shared bytes have identical char boundaries, so checking
        // one string suffices.
        let mut prefix_len = raw_prefix;
        while prefix_len > 0 && !before.is_char_boundary(prefix_len) {
            prefix_len -= 1;
        }

        // ── Step 3: Raw common suffix (in remaining bytes) ───────────
        let raw_suffix = before_b[prefix_len..]
            .iter()
            .rev()
            .zip(after_b[prefix_len..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();

        // ── Step 4: Snap suffix to char boundary in BOTH strings ─────
        // The suffix bytes are identical, but the split point sits in
        // different contexts (the non-shared middle differs), so we
        // must verify char boundaries in both strings independently.
        let mut suffix_len = raw_suffix;
        while suffix_len > 0 {
            let before_split = before.len() - suffix_len;
            let after_split = after.len() - suffix_len;
            if before.is_char_boundary(before_split) && after.is_char_boundary(after_split) {
                break;
            }
            suffix_len -= 1;
        }

        // ── Step 5: Extract middle sections ──────────────────────────
        let before_mid_end = before.len() - suffix_len;
        let after_mid_end = after.len() - suffix_len;
        let before_mid_len = before_mid_end - prefix_len;
        let after_mid = &after[prefix_len..after_mid_end];

        // ── Step 6: Build ChangeSet ──────────────────────────────────
        if before_mid_len == 0 && after_mid.is_empty() {
            Self::identity(before.len())
        } else if after_mid.is_empty() {
            Self::from_delete(before.len(), prefix_len, before_mid_end)
        } else if before_mid_len == 0 {
            Self::from_insert(before.len(), prefix_len, after_mid)
        } else {
            Self::from_replace(before.len(), prefix_len, before_mid_end, after_mid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fundamental guarantee: from_diff(a, b).apply(a) == b
    fn verify_diff(before: &str, after: &str) {
        let cs = ChangeSet::from_diff(before, after);
        assert_eq!(
            cs.input_len(),
            before.len(),
            "input_len mismatch for diff({before:?}, {after:?})"
        );
        assert_eq!(
            cs.output_len(),
            after.len(),
            "output_len mismatch for diff({before:?}, {after:?})"
        );
        let result = cs.apply(before).unwrap();
        assert_eq!(
            result, after,
            "from_diff({before:?}, {after:?}).apply({before:?}) = {result:?}, expected {after:?}"
        );
    }

    // ── Identical texts ──────────────────────────────────────────────

    #[test]
    fn identical_texts() {
        let cs = ChangeSet::from_diff("hello", "hello");
        assert!(cs.is_identity());
        verify_diff("hello", "hello");
    }

    #[test]
    fn both_empty() {
        let cs = ChangeSet::from_diff("", "");
        assert!(cs.is_identity());
        assert_eq!(cs.input_len(), 0);
    }

    // ── Pure insertions ──────────────────────────────────────────────

    #[test]
    fn insert_at_start() {
        verify_diff("hello", "XXhello");
    }

    #[test]
    fn insert_at_end() {
        verify_diff("hello", "helloXX");
    }

    #[test]
    fn insert_in_middle() {
        verify_diff("hello", "helXXlo");
    }

    #[test]
    fn insert_into_empty() {
        verify_diff("", "hello");
    }

    // ── Pure deletions ───────────────────────────────────────────────

    #[test]
    fn delete_from_start() {
        verify_diff("hello", "llo");
    }

    #[test]
    fn delete_from_end() {
        verify_diff("hello", "hel");
    }

    #[test]
    fn delete_from_middle() {
        verify_diff("hello", "hlo");
    }

    #[test]
    fn delete_everything() {
        verify_diff("hello", "");
    }

    // ── Replacements ─────────────────────────────────────────────────

    #[test]
    fn replace_at_start() {
        verify_diff("hello", "XXllo");
    }

    #[test]
    fn replace_at_end() {
        verify_diff("hello", "helXX");
    }

    #[test]
    fn replace_in_middle() {
        verify_diff("hello", "hXXXo");
    }

    #[test]
    fn replace_everything() {
        verify_diff("hello", "world");
    }

    #[test]
    fn replace_shorter() {
        verify_diff("hello world", "hello X");
    }

    #[test]
    fn replace_longer() {
        verify_diff("hi", "hello world");
    }

    // ── UTF-8 ────────────────────────────────────────────────────────

    #[test]
    fn utf8_identical() {
        verify_diff("héllo 世界", "héllo 世界");
    }

    #[test]
    fn utf8_insert_before_multibyte() {
        verify_diff("héllo", "Xhéllo");
    }

    #[test]
    fn utf8_insert_after_multibyte() {
        verify_diff("héllo", "héXllo");
    }

    #[test]
    fn utf8_delete_multibyte() {
        // Delete 'é' (2 bytes)
        verify_diff("héllo", "hllo");
    }

    #[test]
    fn utf8_replace_multibyte() {
        // Replace 'é' with 'a'
        verify_diff("héllo", "hallo");
    }

    #[test]
    fn utf8_replace_with_different_multibyte() {
        // Replace 'é' with 'ë' — shared continuation byte A9 vs AB
        verify_diff("héllo", "hëllo");
    }

    #[test]
    fn utf8_cjk() {
        verify_diff("世界", "hello世界");
        verify_diff("世界", "世X界");
        verify_diff("世界", "世");
    }

    #[test]
    fn utf8_emoji() {
        verify_diff("hello 🌍", "hello 🌎");
        verify_diff("🌍🌎", "🌍X🌎");
    }

    #[test]
    fn utf8_shared_continuation_bytes() {
        // 'é' = [C3, A9], 'ĩ' = [C4, A9] — share the A9 continuation byte
        // This tests the suffix char-boundary snapping
        verify_diff("é", "ĩ");
    }

    #[test]
    fn utf8_shared_leading_bytes() {
        // 'é' = [C3, A9], 'ë' = [C3, AB] — share the C3 leading byte
        // This tests the prefix char-boundary snapping
        verify_diff("é", "ë");
    }

    // ── Multiline ────────────────────────────────────────────────────

    #[test]
    fn multiline_insert_line() {
        verify_diff("line1\nline2\n", "line1\nnewline\nline2\n");
    }

    #[test]
    fn multiline_delete_line() {
        verify_diff("line1\nline2\nline3\n", "line1\nline3\n");
    }

    #[test]
    fn multiline_replace_line() {
        verify_diff("line1\nline2\nline3\n", "line1\nchanged\nline3\n");
    }

    // ── ChangeSet properties ─────────────────────────────────────────

    #[test]
    fn from_diff_matches_from_insert() {
        let before = "hello";
        let after = "heXXllo";
        let cs_diff = ChangeSet::from_diff(before, after);
        let cs_insert = ChangeSet::from_insert(5, 2, "XX");
        // Both should produce the same result when applied
        assert_eq!(
            cs_diff.apply(before).unwrap(),
            cs_insert.apply(before).unwrap()
        );
    }

    #[test]
    fn from_diff_matches_from_delete() {
        let before = "hello";
        let after = "hlo";
        let cs_diff = ChangeSet::from_diff(before, after);
        // Delete bytes 1..3 ("el"): "hello" → "hlo"
        let cs_delete = ChangeSet::from_delete(5, 1, 3);
        assert_eq!(
            cs_diff.apply(before).unwrap(),
            cs_delete.apply(before).unwrap()
        );
    }

    #[test]
    fn from_diff_matches_from_replace() {
        let before = "hello";
        let after = "hXYo";
        let cs_diff = ChangeSet::from_diff(before, after);
        let cs_replace = ChangeSet::from_replace(5, 1, 4, "XY");
        assert_eq!(
            cs_diff.apply(before).unwrap(),
            cs_replace.apply(before).unwrap()
        );
    }

    #[test]
    fn from_diff_is_invertible() {
        let before = "hello world";
        let after = "hello WORLD";
        let cs = ChangeSet::from_diff(before, after);
        let inv = cs.invert(before).unwrap();
        let restored = inv.apply(after).unwrap();
        assert_eq!(restored, before);
    }

    #[test]
    fn from_diff_is_composable() {
        let a = "hello";
        let b = "hello world";
        let c = "goodbye world";
        let cs1 = ChangeSet::from_diff(a, b);
        let cs2 = ChangeSet::from_diff(b, c);
        let composed = cs1.compose(&cs2).unwrap();
        assert_eq!(composed.apply(a).unwrap(), c);
    }

    // ── Round-trip with SavePoint ────────────────────────────────────

    #[test]
    fn savepoint_integration() {
        let original = "hello world";
        let mut sp = crate::primitives::SavePoint::new(
            original.len(),
            crate::primitives::Offset::new(0),
            None,
        );

        // Simulate an external formatter changing the text
        let formatted = "Hello World!";
        let cs = ChangeSet::from_diff(original, formatted);
        sp.update(&cs, original).unwrap();

        // Restore
        let restored = sp.restore(formatted).unwrap();
        assert_eq!(restored.text, original);
    }

    // ── Exhaustive ASCII byte pair test ──────────────────────────────

    #[test]
    fn exhaustive_small_strings() {
        // Test all combinations of small strings to verify correctness
        let samples = ["", "a", "ab", "abc", "b", "bc", "xyz", "abcabc"];
        for before in &samples {
            for after in &samples {
                verify_diff(before, after);
            }
        }
    }
}
