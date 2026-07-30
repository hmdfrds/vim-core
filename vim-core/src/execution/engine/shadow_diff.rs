//! Diff engine for shadow execution.
//!
//! Computes the minimal text delta between an original document and a
//! modified document after shadow execution completes, producing at most
//! one [`Effect`] that represents the change.
//!
//! # Algorithm
//!
//! Uses O(n) prefix/suffix scanning:
//! 1. Scan forward byte-by-byte to find the common prefix length.
//! 2. Align the prefix boundary to a UTF-8 char boundary.
//! 3. Scan backward byte-by-byte to find the common suffix length
//!    (without overlapping the prefix).
//! 4. Align the suffix boundary to a UTF-8 char boundary.
//! 5. Extract the differing "middle" from each string and classify:
//!    - Both empty => no change (`None`).
//!    - Original empty => insertion.
//!    - Modified empty => deletion.
//!    - Both non-empty => replacement.

use crate::effects::Effect;
use crate::primitives::{Offset, Range};

/// Compute the minimal text delta between `original` and `modified`.
///
/// Returns `None` when the two strings are identical, or `Some(effect)`
/// containing exactly one [`Effect::Insert`], [`Effect::Delete`], or
/// [`Effect::Replace`] that transforms `original` into `modified`.
pub(in crate::execution::engine) fn compute_diff(original: &str, modified: &str) -> Option<Effect> {
    let orig_bytes = original.as_bytes();
    let mod_bytes = modified.as_bytes();
    let orig_len = orig_bytes.len();
    let mod_len = mod_bytes.len();

    // Step 1: Scan forward to find common prefix length (byte count).
    let raw_prefix = count_common_prefix(orig_bytes, mod_bytes);

    // Step 2: Align prefix to a char boundary (retreat if mid-character).
    let prefix = align_prefix_to_char_boundary(original, modified, raw_prefix);

    // Step 3: Scan backward to find common suffix length, avoiding overlap.
    let max_suffix = orig_len.min(mod_len) - prefix;
    let raw_suffix = count_common_suffix(orig_bytes, mod_bytes, max_suffix);

    // Step 4: Align suffix to a char boundary (retreat if mid-character).
    let suffix = align_suffix_to_char_boundary(original, modified, raw_suffix);

    // Step 5: Extract the differing middles.
    let orig_middle = &original[prefix..orig_len - suffix];
    let mod_middle = &modified[prefix..mod_len - suffix];

    // Step 6: Classify.
    classify_delta(prefix, orig_middle, mod_middle)
}

/// Count how many leading bytes are identical between two byte slices.
fn count_common_prefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Count how many trailing bytes are identical, scanning at most `max` bytes.
fn count_common_suffix(a: &[u8], b: &[u8], max: usize) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take(max)
        .take_while(|(x, y)| x == y)
        .count()
}

/// Retreat `prefix` until it sits on a valid UTF-8 char boundary in both strings.
const fn align_prefix_to_char_boundary(original: &str, modified: &str, mut prefix: usize) -> usize {
    while prefix > 0 && (!original.is_char_boundary(prefix) || !modified.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    prefix
}

/// Retreat `suffix` until the cut point is a valid char boundary in both strings.
///
/// The cut point for the suffix is `len - suffix`; we need that index to be a
/// char boundary in both the original and modified strings.
const fn align_suffix_to_char_boundary(original: &str, modified: &str, mut suffix: usize) -> usize {
    while suffix > 0 {
        let orig_cut = original.len() - suffix;
        let mod_cut = modified.len() - suffix;
        if original.is_char_boundary(orig_cut) && modified.is_char_boundary(mod_cut) {
            break;
        }
        suffix -= 1;
    }
    suffix
}

/// Convert the prefix offset and middle slices into an `Option<Effect>`.
fn classify_delta(prefix: usize, orig_middle: &str, mod_middle: &str) -> Option<Effect> {
    let offset = Offset::new(prefix);
    match (orig_middle.is_empty(), mod_middle.is_empty()) {
        (true, true) => None,
        (true, false) => Some(Effect::insert(offset, mod_middle)),
        (false, true) => {
            let range = Range::new(offset, Offset::new(prefix + orig_middle.len()));
            Some(Effect::delete(range))
        }
        (false, false) => {
            let range = Range::new(offset, Offset::new(prefix + orig_middle.len()));
            Some(Effect::replace(range, mod_middle))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════
    // Helper to extract Effect details for assertion readability
    // ═══════════════════════════════════════════════════════════════════════

    /// Assert the result is `None` (identical strings).
    fn assert_none(original: &str, modified: &str) {
        assert_eq!(
            compute_diff(original, modified),
            None,
            "expected None for original={original:?}, modified={modified:?}"
        );
    }

    /// Assert an Insert effect at the given offset with the given text.
    fn assert_insert(original: &str, modified: &str, at: usize, text: &str) {
        let effect = compute_diff(original, modified).unwrap_or_else(|| {
            panic!("expected Insert, got None for {original:?} -> {modified:?}")
        });
        match &effect {
            Effect::Insert { offset, text: t } => {
                assert_eq!(
                    offset.get(),
                    at,
                    "Insert offset mismatch: expected {at}, got {}",
                    offset.get()
                );
                assert_eq!(
                    t.as_str(),
                    text,
                    "Insert text mismatch: expected {text:?}, got {t:?}"
                );
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    /// Assert a Delete effect over the given byte range.
    fn assert_delete(original: &str, modified: &str, start: usize, end: usize) {
        let effect = compute_diff(original, modified).unwrap_or_else(|| {
            panic!("expected Delete, got None for {original:?} -> {modified:?}")
        });
        match &effect {
            Effect::Delete { range } => {
                assert_eq!(
                    range.start().get(),
                    start,
                    "Delete start mismatch: expected {start}, got {}",
                    range.start().get()
                );
                assert_eq!(
                    range.end().get(),
                    end,
                    "Delete end mismatch: expected {end}, got {}",
                    range.end().get()
                );
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    /// Assert a Replace effect over the given byte range with the given text.
    fn assert_replace(original: &str, modified: &str, start: usize, end: usize, text: &str) {
        let effect = compute_diff(original, modified).unwrap_or_else(|| {
            panic!("expected Replace, got None for {original:?} -> {modified:?}")
        });
        match &effect {
            Effect::Replace { range, text: t } => {
                assert_eq!(
                    range.start().get(),
                    start,
                    "Replace start mismatch: expected {start}, got {}",
                    range.start().get()
                );
                assert_eq!(
                    range.end().get(),
                    end,
                    "Replace end mismatch: expected {end}, got {}",
                    range.end().get()
                );
                assert_eq!(
                    t.as_str(),
                    text,
                    "Replace text mismatch: expected {text:?}, got {t:?}"
                );
            }
            other => panic!("expected Replace, got {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Test cases
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn identical_strings() {
        assert_none("hello world", "hello world");
    }

    #[test]
    fn both_empty() {
        assert_none("", "");
    }

    #[test]
    fn empty_to_non_empty() {
        assert_insert("", "hello", 0, "hello");
    }

    #[test]
    fn non_empty_to_empty() {
        assert_delete("hello", "", 0, 5);
    }

    #[test]
    fn prefix_change() {
        // "hello world" -> "Hello world": replace 'h' with 'H' at offset 0
        assert_replace("hello world", "Hello world", 0, 1, "H");
    }

    #[test]
    fn suffix_change() {
        // "hello world" -> "hello World": replace 'w' with 'W' at offset 6
        assert_replace("hello world", "hello World", 6, 7, "W");
    }

    #[test]
    fn middle_change() {
        // "hello world" -> "hello WORLD": replace "world" with "WORLD" at offset 6
        assert_replace("hello world", "hello WORLD", 6, 11, "WORLD");
    }

    #[test]
    fn complete_replacement() {
        // "abc" -> "xyz": replace entire string
        assert_replace("abc", "xyz", 0, 3, "xyz");
    }

    #[test]
    fn pure_insertion_in_middle() {
        // "helloworld" -> "hello world": insert " " at offset 5
        assert_insert("helloworld", "hello world", 5, " ");
    }

    #[test]
    fn pure_deletion_in_middle() {
        // "hello world" -> "helloworld": delete " " at offset 5
        assert_delete("hello world", "helloworld", 5, 6);
    }

    #[test]
    fn multibyte_replace() {
        // "héllo" -> "hèllo"
        // 'h' = 1 byte, 'é' = 2 bytes (0xC3 0xA9), 'è' = 2 bytes (0xC3 0xA8)
        // Common prefix: 'h' (1 byte), then 0xC3 matches but 0xA9 != 0xA8
        // The raw prefix would be 2, but that's mid-char for é/è.
        // After alignment, prefix retreats to 1.
        // Common suffix: "llo" (3 bytes)
        // orig_middle = "é" (2 bytes at offset 1..3), mod_middle = "è" (2 bytes)
        assert_replace("héllo", "hèllo", 1, 3, "\u{00E8}");
    }

    #[test]
    fn unicode_cjk() {
        // "世界" -> "世間"
        // '世' = 3 bytes (0xE4 0xB8 0x96), '界' = 3 bytes (0xE7 0x95 0x8C)
        // '間' = 3 bytes (0xE9 0x96 0x93)
        // Common prefix: 3 bytes ('世'), common suffix: 0 bytes
        // orig_middle = "界" (3 bytes at offset 3..6)
        assert_replace("世界", "世間", 3, 6, "間");
    }

    #[test]
    fn single_char_diff_at_end() {
        // "abcd" -> "abce": replace last char
        assert_replace("abcd", "abce", 3, 4, "e");
    }

    #[test]
    fn prefix_only_match() {
        // "abcXYZ" -> "abcPQR": common prefix "abc", no common suffix
        assert_replace("abcXYZ", "abcPQR", 3, 6, "PQR");
    }

    #[test]
    fn suffix_only_match() {
        // "XYZabc" -> "PQRabc": common suffix "abc", no common prefix
        assert_replace("XYZabc", "PQRabc", 0, 3, "PQR");
    }

    #[test]
    fn single_char_insert_at_start() {
        assert_insert("bc", "abc", 0, "a");
    }

    #[test]
    fn single_char_insert_at_end() {
        assert_insert("ab", "abc", 2, "c");
    }

    #[test]
    fn single_char_delete_at_start() {
        assert_delete("abc", "bc", 0, 1);
    }

    #[test]
    fn single_char_delete_at_end() {
        assert_delete("abc", "ab", 2, 3);
    }

    #[test]
    fn multi_line_insertion() {
        // Prefix greedily matches "line1\nline" (10 bytes), suffix matches "\n" + the
        // leftover "3" can't extend further because max_suffix = min(12,18)-10 = 2.
        // So suffix = 2 ("3\n"), orig_middle = "" (10..10), mod_middle = "2\nline" (10..16).
        assert_insert("line1\nline3\n", "line1\nline2\nline3\n", 10, "2\nline");
    }

    #[test]
    fn multi_line_deletion() {
        // Symmetric: prefix = "line1\nline" (10), suffix = "3\n" (2),
        // orig_middle = "2\nline" (10..16), mod_middle = "" (10..10).
        assert_delete("line1\nline2\nline3\n", "line1\nline3\n", 10, 16);
    }

    #[test]
    fn emoji_handling() {
        // Emoji are 4 bytes each in UTF-8.
        // "hello 😀 world" -> "hello 😢 world"
        // Common prefix: "hello " (6 bytes)
        // '😀' = 4 bytes, '😢' = 4 bytes — first bytes differ
        // Common suffix: " world" (6 bytes)
        assert_replace("hello 😀 world", "hello 😢 world", 6, 10, "😢");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Pathological edge cases — stress-testing prefix/suffix algorithm
    // ═══════════════════════════════════════════════════════════════════════

    // --- Case 5: suffix overlap with prefix (repeated chars, shrink) ---

    #[test]
    fn repeated_chars_shrink() {
        // "aaaa" -> "aa": prefix greedily matches 2 'a's, then max_suffix =
        // min(4,2)-2 = 0, so suffix=0.
        // orig_middle = "aa" (pos 2..4), mod_middle = "" (pos 2..2).
        // Result: Delete(2, 4). Applied: "aa" + "" = "aa". Correct.
        assert_delete("aaaa", "aa", 2, 4);
    }

    // --- Case 6: repeated characters grow ---

    #[test]
    fn repeated_chars_grow() {
        // "aaaa" -> "aaaaaa": prefix=4 (all of original), max_suffix =
        // min(4,6)-4 = 0, suffix=0.
        // orig_middle = "" (pos 4..4), mod_middle = "aa" (pos 4..6).
        // Result: Insert(4, "aa"). Applied: "aaaa" + "aa" = "aaaaaa". Correct.
        assert_insert("aaaa", "aaaaaa", 4, "aa");
    }

    #[test]
    fn all_same_char_one_added() {
        // "aaa" -> "aaaa": prefix=3, max_suffix = min(3,4)-3 = 0, suffix=0.
        // Insert at position 3.
        assert_insert("aaa", "aaaa", 3, "a");
    }

    #[test]
    fn all_same_char_one_removed() {
        // "aaaa" -> "aaa": prefix=3, max_suffix = min(4,3)-3 = 0, suffix=0.
        // orig_middle = "a" (pos 3..4), mod_middle = "" (pos 3..3).
        assert_delete("aaaa", "aaa", 3, 4);
    }

    // --- Case 7: café -> cafe (2-byte accent removal) ---

    #[test]
    fn cafe_accent_removal() {
        // "café" bytes: [63, 61, 66, c3, a9] = 5 bytes
        // "cafe" bytes: [63, 61, 66, 65] = 4 bytes
        // Prefix: c=c, a=a, f=f, then c3 != 65. raw_prefix=3.
        // Position 3 in "café" is char boundary (start of 'é'), position 3 in
        // "cafe" is char boundary (start of 'e'). prefix=3.
        // max_suffix = min(5,4)-3 = 1. Backward: a9 vs 65 — mismatch. suffix=0.
        // orig_middle = "é" (2 bytes, pos 3..5), mod_middle = "e" (1 byte, pos 3..4).
        assert_replace("café", "cafe", 3, 5, "e");
    }

    // --- Case 8: 4-byte emoji swap with shared leading bytes ---

    #[test]
    fn four_byte_emoji_swap() {
        // "a🎉b" bytes: [61, f0,9f,8e,89, 62] (6 bytes total)
        // "a🎊b" bytes: [61, f0,9f,8e,8a, 62] (6 bytes total)
        // Prefix: 61=61(1), f0=f0(2), 9f=9f(3), 8e=8e(4), 89!=8a(stop).
        // raw_prefix = 4. Position 4 in "a🎉b": 'a'=0, '🎉'=1..5, 'b'=5.
        // Position 4 is mid-emoji — NOT a char boundary. Retreat to 1. prefix=1.
        // Suffix: 62=62(1), then 89!=8a(stop). raw_suffix=1.
        // Suffix cut: orig 6-1=5, mod 6-1=5. Both are char boundary ('b' starts
        // at 5). suffix=1.
        // orig_middle = "🎉" (pos 1..5, 4 bytes), mod_middle = "🎊" (pos 1..5).
        assert_replace("a\u{1F389}b", "a\u{1F38A}b", 1, 5, "\u{1F38A}");
    }

    #[test]
    fn four_byte_emoji_prefix_boundary_at_zero() {
        // "🎉b" -> "🎊b": first bytes f0=f0, 9f=9f, 8e=8e, 89!=8a.
        // raw_prefix=3. Position 3 in "🎉b": mid-emoji. Retreat to 0. prefix=0.
        // Suffix: 62=62(1), 89!=8a(stop). suffix=1.
        // orig_middle = "🎉" (0..4), mod_middle = "🎊" (0..4).
        assert_replace("\u{1F389}b", "\u{1F38A}b", 0, 4, "\u{1F38A}");
    }

    // --- Case 10: interleaved changes ---

    #[test]
    fn interleaved_changes() {
        // "abcabc" -> "xbcxbc"
        // Prefix: a!=x at byte 0. prefix=0.
        // max_suffix = min(6,6)-0 = 6. Backward scan:
        //   c=c(1), b=b(2), a!=x(stop). suffix=2.
        // orig_middle = original[0..6-2] = "abca", mod_middle = modified[0..6-2] = "xbcx".
        // Result: Replace(0, 4, "xbcx"). Applied: "xbcx" + "bc" = "xbcxbc". Correct.
        assert_replace("abcabc", "xbcxbc", 0, 4, "xbcx");
    }

    #[test]
    fn interleaved_three_repetitions() {
        // "abcabcabc" -> "xbcxbcxbc"
        // Prefix: a!=x. prefix=0.
        // Suffix backward: c=c(1), b=b(2), a!=x(stop). suffix=2.
        // orig_middle = original[0..9-2] = "abcabca", mod_middle = modified[0..9-2] = "xbcxbcx".
        // Replace(0, 7, "xbcxbcx"). Applied: "xbcxbcx" + "bc" = "xbcxbcxbc". Correct.
        assert_replace("abcabcabc", "xbcxbcxbc", 0, 7, "xbcxbcx");
    }

    // --- Suffix alignment retreating across multi-byte characters ---

    #[test]
    fn suffix_alignment_retreat() {
        // When the raw suffix cut lands mid-character in one string but not the
        // other, the suffix must retreat.
        // "xé" bytes: [78, c3, a9] (3 bytes)
        // "yée" bytes: [79, c3, a9, 65] (4 bytes)
        // Prefix: 78!=79. prefix=0.
        // max_suffix = min(3,4)-0 = 3. Backward scan:
        //   a9=65? No — mismatch at first byte. suffix=0.
        // orig_middle = "xé", mod_middle = "yée".
        // Replace(0, 3, "yée"). Applied: "yée". Correct.
        assert_replace("xé", "yée", 0, 3, "yée");
    }

    #[test]
    fn suffix_mid_char_boundary_retreat() {
        // Construct a case where suffix raw scan matches bytes that cross a char
        // boundary in one string but not the other.
        // "aé" = [61, c3, a9], "bé" = [62, c3, a9]
        // Prefix: 61!=62. prefix=0.
        // max_suffix = min(3,3) - 0 = 3. Backward:
        //   a9=a9(1), c3=c3(2), 61!=62(stop). suffix=2.
        // Suffix cut: orig 3-2=1, mod 3-2=1.
        // Is position 1 a char boundary in "aé"? 'a'=0, 'é'=1..3. Yes, 1 is a
        // char boundary. Same for "bé". suffix=2.
        // orig_middle = "a" (0..1), mod_middle = "b" (0..1).
        assert_replace("aé", "bé", 0, 1, "b");
    }

    #[test]
    fn suffix_retreat_due_to_different_char_widths() {
        // "aé" = [61, c3, a9] and "aバ" where バ = [e3, 83, 90] (3 bytes).
        // The trailing byte a9 != 90. No suffix match. prefix: 61=61(1), then
        // c3!=e3(stop). raw_prefix=1. Both are char boundary at 1. prefix=1.
        // Suffix: raw_suffix=0.
        // orig_middle = "é" (1..3), mod_middle = "バ" (1..4).
        assert_replace("aé", "aバ", 1, 3, "バ");
    }

    // --- Length-asymmetric cases ---

    #[test]
    fn short_to_very_long() {
        // "a" -> "abcdefghij" (1 byte to 10 bytes)
        // Prefix: a=a(1). prefix=1.
        // max_suffix = min(1,10)-1 = 0. suffix=0.
        // orig_middle = "" (1..1), mod_middle = "bcdefghij" (1..10).
        assert_insert("a", "abcdefghij", 1, "bcdefghij");
    }

    #[test]
    fn very_long_to_short() {
        // "abcdefghij" -> "a" (10 bytes to 1 byte)
        // Prefix: a=a(1). prefix=1.
        // max_suffix = min(10,1)-1 = 0. suffix=0.
        // orig_middle = "bcdefghij" (1..10), mod_middle = "" (1..1).
        assert_delete("abcdefghij", "a", 1, 10);
    }

    // --- Single-character strings ---

    #[test]
    fn single_char_replace() {
        assert_replace("a", "b", 0, 1, "b");
    }

    #[test]
    fn single_char_to_empty() {
        assert_delete("a", "", 0, 1);
    }

    #[test]
    fn empty_to_single_char() {
        assert_insert("", "a", 0, "a");
    }

    // --- Prefix consumes entire shorter string ---

    #[test]
    fn prefix_consumes_shorter_string_insert() {
        // "abc" -> "abcdef": prefix=3, max_suffix=min(3,6)-3=0, suffix=0.
        // Insert "def" at position 3.
        assert_insert("abc", "abcdef", 3, "def");
    }

    #[test]
    fn prefix_consumes_shorter_string_delete() {
        // "abcdef" -> "abc": prefix=3, max_suffix=min(6,3)-3=0, suffix=0.
        // Delete "def" (3..6).
        assert_delete("abcdef", "abc", 3, 6);
    }

    // --- Suffix consumes entire shorter string ---

    #[test]
    fn suffix_consumes_shorter_string_insert() {
        // "def" -> "abcdef": prefix: d!=a. prefix=0.
        // max_suffix = min(3,6)-0 = 3. Backward: f=f(1), e=e(2), d=d(3). suffix=3.
        // orig_middle = "" (0..0), mod_middle = modified[0..6-3] = "abc".
        // Insert "abc" at position 0.
        assert_insert("def", "abcdef", 0, "abc");
    }

    #[test]
    fn suffix_consumes_shorter_string_delete() {
        // "abcdef" -> "def": prefix: a!=d. prefix=0.
        // max_suffix = min(6,3)-0 = 3. Backward: f=f(1), e=e(2), d=d(3). suffix=3.
        // orig_middle = original[0..6-3] = "abc", mod_middle = modified[0..3-3] = "".
        // Delete "abc" (0..3).
        assert_delete("abcdef", "def", 0, 3);
    }

    // --- All-same-character pathology with different lengths ---

    #[test]
    fn all_a_lengths_differ_by_one() {
        // "aaaaa" (5) -> "aaaaaa" (6): prefix=5, max_suffix=min(5,6)-5=0.
        // Insert "a" at 5.
        assert_insert("aaaaa", "aaaaaa", 5, "a");
    }

    #[test]
    fn all_a_shrink_by_three() {
        // "aaaaa" (5) -> "aa" (2): prefix=2, max_suffix=min(5,2)-2=0. suffix=0.
        // Delete "aaa" at 2..5.
        assert_delete("aaaaa", "aa", 2, 5);
    }

    // --- Newlines and whitespace ---

    #[test]
    fn newline_only_strings() {
        assert_none("\n\n\n", "\n\n\n");
    }

    #[test]
    fn add_trailing_newline() {
        // "hello" -> "hello\n"
        assert_insert("hello", "hello\n", 5, "\n");
    }

    #[test]
    fn remove_trailing_newline() {
        assert_delete("hello\n", "hello", 5, 6);
    }

    #[test]
    fn change_line_ending_crlf_to_lf() {
        // "line\r\n" -> "line\n": prefix "line" (4), suffix "\n" (1).
        // max_suffix = min(6,5)-4 = 1. Backward: 0a=0a(1). suffix=1.
        // orig_middle = original[4..6-1] = "\r" (pos 4..5), mod_middle = "".
        assert_delete("line\r\n", "line\n", 4, 5);
    }

    // --- Multi-byte: mixed widths ---

    #[test]
    fn mixed_multibyte_widths() {
        // Mix 1-byte, 2-byte, 3-byte, 4-byte characters.
        // "aé世🎉" = [61, c3,a9, e4,b8,96, f0,9f,8e,89] = 10 bytes
        // "aé世🎊" = [61, c3,a9, e4,b8,96, f0,9f,8e,8a] = 10 bytes
        // Prefix: match up to byte 9 (61,c3,a9,e4,b8,96,f0,9f,8e), then 89!=8a.
        // raw_prefix=9. Position 9 in "aé世🎉": the emoji starts at byte 6
        // (1+2+3=6), so bytes 6..10 are 🎉. Position 9 is mid-emoji. Retreat:
        // 8 mid-emoji, 7 mid-emoji, 6 — char boundary! prefix=6.
        // Suffix: max_suffix = min(10,10)-6 = 4. Backward:
        //   89!=8a(stop). suffix=0.
        // orig_middle = "🎉" (6..10), mod_middle = "🎊" (6..10).
        assert_replace("aé世\u{1F389}", "aé世\u{1F38A}", 6, 10, "\u{1F38A}");
    }

    #[test]
    fn three_byte_char_prefix_retreat() {
        // "a世b" = [61, e4,b8,96, 62] (5 bytes)
        // "a界b" = [61, e7,95,8c, 62] (5 bytes)
        // Prefix: 61=61(1), e4!=e7(stop). raw_prefix=1. Position 1 is char
        // boundary in both (start of CJK). prefix=1.
        // Suffix: 62=62(1), 96!=8c(stop). raw_suffix=1.
        // Cut at 5-1=4: position 4 in "a世b" = start of 'b'. Char boundary. suffix=1.
        // orig_middle = "世" (1..4), mod_middle = "界" (1..4).
        assert_replace("a世b", "a界b", 1, 4, "界");
    }

    // --- Prefix and suffix consume everything (identical strings, alternate check) ---

    #[test]
    fn identical_single_char() {
        assert_none("x", "x");
    }

    #[test]
    fn identical_multibyte() {
        assert_none("日本語", "日本語");
    }

    #[test]
    fn identical_emoji() {
        assert_none("🎉🎊🎁", "🎉🎊🎁");
    }

    // --- Adversarial: max_suffix clamp prevents overlap ---

    #[test]
    fn max_suffix_clamp_ab_to_b() {
        // "ab" -> "b": prefix: a!=b. prefix=0.
        // max_suffix = min(2,1)-0 = 1. Backward: b=b(1). suffix=1.
        // orig_middle = original[0..2-1] = "a", mod_middle = modified[0..1-1] = "".
        // Delete "a" (0..1). Applied: "" + "b" = "b". Correct.
        assert_delete("ab", "b", 0, 1);
    }

    #[test]
    fn max_suffix_clamp_a_to_ba() {
        // "a" -> "ba": prefix: a!=b. prefix=0.
        // max_suffix = min(1,2)-0 = 1. Backward: a=a(1). suffix=1.
        // orig_middle = original[0..1-1] = "", mod_middle = modified[0..2-1] = "b".
        // Insert "b" at 0. Applied: "b" + "a" = "ba". Correct.
        assert_insert("a", "ba", 0, "b");
    }

    #[test]
    fn max_suffix_clamp_prevents_overlap_with_prefix() {
        // "aba" -> "a": prefix: a=a(1). prefix=1.
        // max_suffix = min(3,1)-1 = 0. suffix=0.
        // orig_middle = "ba" (1..3), mod_middle = "" (1..1).
        // Delete "ba" (1..3). Applied: "a" + "" = "a". Correct.
        assert_delete("aba", "a", 1, 3);
    }

    // --- Worst case for greedy prefix: ambiguous placement ---

    #[test]
    fn ambiguous_insert_in_repeated_region() {
        // "aab" -> "aaab": could be insert 'a' at position 0, 1, or 2.
        // Algorithm: prefix: a=a(1), a=a(2), b!=a(stop). prefix=2.
        // max_suffix = min(3,4)-2 = 1. Backward: b=b(1). suffix=1.
        // orig_middle = "" (2..2), mod_middle = "a" (2..3).
        // Insert "a" at 2. Applied: "aa" + "a" + "b" = "aaab". Correct.
        // (The placement is at position 2, which is valid even if not unique.)
        assert_insert("aab", "aaab", 2, "a");
    }

    #[test]
    fn ambiguous_delete_in_repeated_region() {
        // "aaab" -> "aab": symmetric.
        // "aaab" = [61,61,61,62], "aab" = [61,61,62]
        // prefix: 61=61(1), 61=61(2), 61!=62(stop). prefix=2.
        // max_suffix = min(4,3)-2 = 1. Backward: 62=62(1). suffix=1.
        // orig_middle = "a" (2..3), mod_middle = "" (2..2).
        assert_delete("aaab", "aab", 2, 3);
    }

    // --- Two-byte char where prefix retreat is needed at different offsets ---

    #[test]
    fn two_byte_chars_with_shared_continuation_byte() {
        // 'é' = c3 a9, 'ê' = c3 aa. They share the leading byte c3.
        // "é" -> "ê": prefix: c3=c3(1), a9!=aa(stop). raw_prefix=1.
        // Position 1 in "é": mid-character (é is bytes 0..2). Retreat to 0.
        // prefix=0. suffix=0. Replace(0, 2, "ê").
        assert_replace("é", "ê", 0, 2, "ê");
    }

    #[test]
    fn two_byte_prefix_retreat_with_context() {
        // "xéy" -> "xêy"
        // [78, c3,a9, 79] -> [78, c3,aa, 79]
        // prefix: 78=78(1), c3=c3(2), a9!=aa(stop). raw_prefix=2. Position 2
        // in "xéy": 'x'=0, 'é'=1..3. 2 is mid-char. Retreat to 1. prefix=1.
        // suffix: 79=79(1), a9!=aa(stop). suffix=1. Cut: 4-1=3, char boundary
        // in both. suffix=1.
        // orig_middle = "é" (1..3), mod_middle = "ê" (1..3).
        assert_replace("xéy", "xêy", 1, 3, "ê");
    }

    // --- Null bytes (valid in Rust strings? No — but \0 is valid UTF-8) ---

    #[test]
    fn strings_with_null_bytes() {
        // "a\0b" -> "a\0c": prefix "a\0" (2 bytes), suffix=0.
        // Replace "b" with "c" at offset 2.
        assert_replace("a\0b", "a\0c", 2, 3, "c");
    }

    // --- Verify the diff round-trips: applying the result to original yields modified ---

    /// Apply a computed diff to the original string and verify it produces
    /// the expected modified string.
    fn assert_roundtrip(original: &str, modified: &str) {
        let effect = compute_diff(original, modified);
        let result = match effect {
            None => original.to_string(),
            Some(Effect::Insert { offset, ref text }) => {
                let pos = offset.get();
                let mut s = original.to_string();
                s.insert_str(pos, text.as_str());
                s
            }
            Some(Effect::Delete { ref range }) => {
                let start = range.start().get();
                let end = range.end().get();
                let mut s = original.to_string();
                s.replace_range(start..end, "");
                s
            }
            Some(Effect::Replace {
                ref range,
                ref text,
            }) => {
                let start = range.start().get();
                let end = range.end().get();
                let mut s = original.to_string();
                s.replace_range(start..end, text.as_str());
                s
            }
            Some(other) => panic!("unexpected effect variant: {other:?}"),
        };
        assert_eq!(
            result, modified,
            "roundtrip failed: {original:?} -> {modified:?}, got {result:?}"
        );
    }

    #[test]
    fn roundtrip_identical() {
        assert_roundtrip("hello", "hello");
    }

    #[test]
    fn roundtrip_empty_to_nonempty() {
        assert_roundtrip("", "test");
    }

    #[test]
    fn roundtrip_nonempty_to_empty() {
        assert_roundtrip("test", "");
    }

    #[test]
    fn roundtrip_interleaved() {
        assert_roundtrip("abcabc", "xbcxbc");
    }

    #[test]
    fn roundtrip_repeated_chars() {
        assert_roundtrip("aaaa", "aa");
        assert_roundtrip("aa", "aaaa");
        assert_roundtrip("aaaa", "aaaaaa");
    }

    #[test]
    fn roundtrip_emoji() {
        assert_roundtrip("a\u{1F389}b", "a\u{1F38A}b");
        assert_roundtrip("a\u{1F389}b", "ab");
        assert_roundtrip("ab", "a\u{1F389}b");
    }

    #[test]
    fn roundtrip_mixed_multibyte() {
        assert_roundtrip("aé世\u{1F389}", "aé世\u{1F38A}");
        assert_roundtrip("café", "cafe");
        assert_roundtrip("日本語", "日本人");
    }

    #[test]
    fn roundtrip_multiline() {
        assert_roundtrip("line1\nline2\nline3\n", "line1\nline3\n");
        assert_roundtrip("line1\nline3\n", "line1\nline2\nline3\n");
        assert_roundtrip("a\nb\nc\n", "x\ny\nz\n");
    }

    #[test]
    fn roundtrip_crlf() {
        assert_roundtrip("line\r\n", "line\n");
        assert_roundtrip("line\n", "line\r\n");
        assert_roundtrip("a\r\nb\r\n", "a\nb\n");
    }

    #[test]
    fn roundtrip_adversarial_suffix_clamp() {
        assert_roundtrip("ab", "b");
        assert_roundtrip("a", "ba");
        assert_roundtrip("aba", "a");
    }

    #[test]
    fn roundtrip_ambiguous_repeated() {
        assert_roundtrip("aab", "aaab");
        assert_roundtrip("aaab", "aab");
        assert_roundtrip("aaaaa", "aa");
    }
}
