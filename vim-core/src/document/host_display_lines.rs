//! Host-side display line provider for FFI/WASM hosts.
//!
//! FFI hosts cannot perform synchronous callbacks across the WASM boundary
//! to ask the editor for soft-wrap information. Instead they push
//! configuration (wrap column, tab size) and this provider computes break
//! positions on the fly from line text.
//!
//! [`HostDisplayLineProvider`] implements [`DisplayLineProvider`] using a
//! monospace column-counting algorithm: breaks happen at exact column
//! boundaries, with no word-aware lookahead.

use unicode_segmentation::UnicodeSegmentation;

use super::DisplayLineProvider;

/// Returns the display width of a character in a monospace terminal.
///
/// - Tab: handled by the caller (context-dependent).
/// - CJK full-width / fullwidth forms: 2 columns.
/// - Everything else: 1 column.
const fn char_display_width(ch: char) -> usize {
    let cp = ch as u32;
    if matches!(cp,
        0x4E00..=0x9FFF       // CJK Unified Ideographs
        | 0x3400..=0x4DBF     // CJK Unified Ideographs Extension A
        | 0xF900..=0xFAFF     // CJK Compatibility Ideographs
        | 0xFF01..=0xFF60     // Fullwidth Forms (excl. halfwidth katakana start)
        | 0xFFE0..=0xFFE6     // Fullwidth Signs (¢ £ ¬ ¯ ¦ ¥ ₩)
        | 0x20000..=0x2FA1F   // CJK Extension B–F + Compat Supplement
        | 0xAC00..=0xD7AF     // Hangul Syllables
    ) {
        2
    } else {
        1
    }
}

/// Display line provider backed by monospace column counting.
///
/// The host sets `wrap_column` (0 = no wrapping) and `tab_size` before each
/// key processing cycle. Break positions are computed on the fly from line
/// text — no cached state is held between calls.
///
/// # Algorithm
///
/// Characters are walked left-to-right, accumulating a column counter:
/// - Tab advances to the next `tab_size` multiple (minimum 1 column).
/// - CJK / fullwidth characters consume 2 columns.
/// - Everything else consumes 1 column.
///
/// When `col + width > wrap_column` and `col > 0`, a break is recorded at
/// the current byte position and the column resets to `width`.
pub struct HostDisplayLineProvider {
    wrap_column: usize,
    tab_size: usize,
}

impl HostDisplayLineProvider {
    /// Create a new provider with the given wrap column and tab size.
    ///
    /// `wrap_column = 0` disables wrapping (all methods treat the line as
    /// a single display line). `tab_size` must be >= 1.
    #[inline]
    #[must_use]
    pub fn new(wrap_column: usize, tab_size: usize) -> Self {
        Self {
            wrap_column,
            tab_size: tab_size.max(1),
        }
    }

    /// Compute byte offsets where soft-wrap breaks occur within `line_text`.
    ///
    /// Returns a `Vec<usize>` of byte positions. Each position marks the
    /// start of a new sub-line. An unwrapped line returns an empty vec.
    fn compute_breaks(&self, line_text: &str) -> Vec<usize> {
        if self.wrap_column == 0 {
            return Vec::new();
        }

        let mut breaks = Vec::new();
        let mut col: usize = 0;

        for (byte_pos, ch) in line_text.char_indices() {
            let width = if ch == '\t' {
                // Advance to next tab_size multiple, minimum 1.
                let next = ((col / self.tab_size) + 1) * self.tab_size;
                next - col
            } else {
                char_display_width(ch)
            };

            if col + width > self.wrap_column && col > 0 {
                breaks.push(byte_pos);
                col = width;
            } else {
                col += width;
            }
        }

        breaks
    }
}

impl DisplayLineProvider for HostDisplayLineProvider {
    fn display_line_count(&self, line_text: &str) -> usize {
        self.compute_breaks(line_text).len() + 1
    }

    fn display_col_to_byte(&self, line_text: &str, sub_line: usize, col: usize) -> Option<usize> {
        let breaks = self.compute_breaks(line_text);
        let num_sub_lines = breaks.len() + 1;

        if sub_line >= num_sub_lines {
            return None;
        }

        // Determine the byte range for this sub-line.
        let start_byte = if sub_line == 0 {
            0
        } else {
            breaks[sub_line - 1]
        };
        let end_byte = if sub_line < breaks.len() {
            breaks[sub_line]
        } else {
            line_text.len()
        };

        let sub_text = &line_text[start_byte..end_byte];

        // Walk graphemes up to `col`, clamping if beyond sub-line length.
        let mut walked = 0;
        for (byte_offset, _grapheme) in sub_text.grapheme_indices(true) {
            if walked == col {
                return Some(start_byte + byte_offset);
            }
            walked += 1;
        }

        // col >= grapheme count: clamp to end of sub-line.
        Some(start_byte + sub_text.len())
    }

    fn byte_to_display_col(&self, line_text: &str, byte_offset: usize) -> Option<(usize, usize)> {
        if byte_offset > line_text.len() {
            return None;
        }

        let breaks = self.compute_breaks(line_text);

        // Find which sub-line contains byte_offset.
        let sub_line = breaks.partition_point(|&b| b <= byte_offset);

        let start_byte = if sub_line == 0 {
            0
        } else {
            breaks[sub_line - 1]
        };

        // Count graphemes from sub-line start to byte_offset.
        let sub_text = &line_text[start_byte..byte_offset];
        let grapheme_col = sub_text.graphemes(true).count();

        Some((sub_line, grapheme_col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(wrap_col: usize) -> HostDisplayLineProvider {
        HostDisplayLineProvider::new(wrap_col, 4)
    }

    // ── Wrap algorithm tests ──

    #[test]
    fn unwrapped_line() {
        let p = provider(80);
        assert_eq!(p.compute_breaks("hello"), Vec::<usize>::new());
        assert_eq!(p.display_line_count("hello"), 1);
    }

    #[test]
    fn exact_wrap_column() {
        // Exactly 5 chars in a wrap_column=5 line: no break needed.
        let p = provider(5);
        assert_eq!(p.compute_breaks("abcde"), Vec::<usize>::new());
        assert_eq!(p.display_line_count("abcde"), 1);
    }

    #[test]
    fn one_char_over() {
        // 6 chars in wrap_column=5: break before the 6th char.
        let p = provider(5);
        let breaks = p.compute_breaks("abcdef");
        assert_eq!(breaks, vec![5]); // 'f' starts at byte 5
        assert_eq!(p.display_line_count("abcdef"), 2);
    }

    #[test]
    fn multiple_wraps() {
        // 13 chars in wrap_column=5: breaks at 5 and 10.
        let p = provider(5);
        let breaks = p.compute_breaks("abcdefghijklm");
        assert_eq!(breaks, vec![5, 10]);
        assert_eq!(p.display_line_count("abcdefghijklm"), 3);
    }

    #[test]
    fn empty_line() {
        let p = provider(80);
        assert_eq!(p.compute_breaks(""), Vec::<usize>::new());
        assert_eq!(p.display_line_count(""), 1);
    }

    #[test]
    fn tab_expansion_basic() {
        // Tab at col 0 with tab_size=4 expands to 4 columns.
        let p = provider(5);
        // "\tX" → tab=4cols, X=1col → total 5 cols, fits in wrap_column=5
        assert_eq!(p.compute_breaks("\tX"), Vec::<usize>::new());
        // "\tXY" → 4+1+1=6, break before Y at byte 2
        assert_eq!(p.compute_breaks("\tXY"), vec![2]);
    }

    #[test]
    fn tab_at_boundary() {
        // Tab at col 3 with tab_size=4 advances to col 4 (width=1).
        let p = provider(5);
        // "abc\tX" → a(1) b(2) c(3) tab→4(width=1) X(5) → 5 cols, fits
        assert_eq!(p.compute_breaks("abc\tX"), Vec::<usize>::new());
        // "abc\tXY" → 6 cols → break before Y
        let breaks = p.compute_breaks("abc\tXY");
        assert_eq!(breaks, vec![5]);
    }

    #[test]
    fn multibyte_utf8() {
        // é is 2 bytes (U+00E9). wrap_column=3 with "aéb" (3 graphemes, 4 bytes)
        let p = provider(3);
        assert_eq!(p.compute_breaks("aéb"), Vec::<usize>::new());
        // "aébc" → 4 cols → break before 'c' at byte 4
        assert_eq!(p.compute_breaks("aébc"), vec![4]);
    }

    #[test]
    fn cjk_double_width() {
        // CJK char 你 (U+4F60) is 3 bytes, 2 columns.
        let p = provider(5);
        // "你你" = 4 cols → fits
        assert_eq!(p.compute_breaks("你你"), Vec::<usize>::new());
        // "你你你" = 6 cols → break before 3rd 你 at byte 6
        let breaks = p.compute_breaks("你你你");
        assert_eq!(breaks, vec![6]);
        assert_eq!(p.display_line_count("你你你"), 2);
    }

    #[test]
    fn wrap_column_zero() {
        let p = provider(0);
        assert_eq!(p.compute_breaks("hello world"), Vec::<usize>::new());
        assert_eq!(p.display_line_count("hello world"), 1);
    }

    #[test]
    fn cjk_at_boundary() {
        // wrap_column=3, CJK char (2 cols) at col 2 would overflow → break.
        let p = provider(3);
        // "ab你" → a(1) b(2) 你(2+2=4>3) → break before 你 at byte 2
        let breaks = p.compute_breaks("ab你");
        assert_eq!(breaks, vec![2]);
    }

    #[test]
    fn single_wide_char_wider_than_wrap() {
        // wrap_column=1, CJK char (2 cols). col=0 so col>0 is false → no break,
        // the char occupies cols 0–1 (overflows but is the first on the line).
        let p = provider(1);
        // Single CJK char: no break (can't break before the first char).
        assert_eq!(p.compute_breaks("你"), Vec::<usize>::new());
        // Two CJK chars: break before the second.
        assert_eq!(p.compute_breaks("你好"), vec![3]);
    }

    // ── Coordinate conversion tests ──

    #[test]
    fn col_to_byte_first_subline() {
        let p = provider(5);
        // "abcdefgh" → breaks at [5]. Sub-line 0 = "abcde".
        assert_eq!(p.display_col_to_byte("abcdefgh", 0, 0), Some(0));
        assert_eq!(p.display_col_to_byte("abcdefgh", 0, 2), Some(2));
        assert_eq!(p.display_col_to_byte("abcdefgh", 0, 4), Some(4));
    }

    #[test]
    fn col_to_byte_second_subline() {
        let p = provider(5);
        // "abcdefgh" → breaks at [5]. Sub-line 1 = "fgh" starting at byte 5.
        assert_eq!(p.display_col_to_byte("abcdefgh", 1, 0), Some(5));
        assert_eq!(p.display_col_to_byte("abcdefgh", 1, 1), Some(6));
        assert_eq!(p.display_col_to_byte("abcdefgh", 1, 2), Some(7));
    }

    #[test]
    fn col_to_byte_out_of_range() {
        let p = provider(5);
        // Sub-line 3 doesn't exist for "abcdefgh" (only 2 sub-lines).
        assert_eq!(p.display_col_to_byte("abcdefgh", 3, 0), None);
    }

    #[test]
    fn col_to_byte_clamp() {
        let p = provider(5);
        // Sub-line 1 = "fgh" (3 graphemes). col=10 clamps to end.
        assert_eq!(p.display_col_to_byte("abcdefgh", 1, 10), Some(8));
    }

    #[test]
    fn byte_to_col_first_subline() {
        let p = provider(5);
        // "abcdefgh" → breaks at [5]. byte 0 → sub-line 0, col 0.
        assert_eq!(p.byte_to_display_col("abcdefgh", 0), Some((0, 0)));
        assert_eq!(p.byte_to_display_col("abcdefgh", 3), Some((0, 3)));
    }

    #[test]
    fn byte_to_col_second_subline() {
        let p = provider(5);
        // byte 5 → sub-line 1, col 0. byte 7 → sub-line 1, col 2.
        assert_eq!(p.byte_to_display_col("abcdefgh", 5), Some((1, 0)));
        assert_eq!(p.byte_to_display_col("abcdefgh", 7), Some((1, 2)));
    }

    #[test]
    fn byte_to_col_roundtrip() {
        let p = provider(5);
        let text = "abcdefgh";
        // For each byte position that is a char boundary, roundtrip through both.
        for byte in 0..text.len() {
            if text.is_char_boundary(byte) {
                let (sub, col) = p.byte_to_display_col(text, byte).unwrap();
                let back = p.display_col_to_byte(text, sub, col).unwrap();
                assert_eq!(back, byte, "roundtrip failed for byte {byte}");
            }
        }
    }

    #[test]
    fn byte_to_col_at_break() {
        let p = provider(5);
        // "abcdefgh" breaks at byte 5. byte 5 is the start of sub-line 1.
        assert_eq!(p.byte_to_display_col("abcdefgh", 5), Some((1, 0)));
    }

    #[test]
    fn byte_to_col_out_of_range() {
        let p = provider(5);
        assert_eq!(p.byte_to_display_col("abc", 10), None);
    }

    #[test]
    fn multibyte_col_to_byte() {
        let p = provider(10);
        // "aébc" → a(byte 0) é(bytes 1-2) b(byte 3) c(byte 4).
        // No wrapping at wrap_column=10. Grapheme col 2 → byte 3.
        assert_eq!(p.display_col_to_byte("aébc", 0, 2), Some(3));
        // Roundtrip: byte 3 → (0, 2)
        assert_eq!(p.byte_to_display_col("aébc", 3), Some((0, 2)));
    }

    #[test]
    fn cjk_col_to_byte_and_back() {
        let p = provider(10);
        // "a你b" → a(byte 0, 1 col) 你(bytes 1-3, 2 cols) b(byte 4, 1 col).
        // Grapheme col 0 → byte 0 (a), col 1 → byte 1 (你), col 2 → byte 4 (b).
        assert_eq!(p.display_col_to_byte("a你b", 0, 0), Some(0));
        assert_eq!(p.display_col_to_byte("a你b", 0, 1), Some(1));
        assert_eq!(p.display_col_to_byte("a你b", 0, 2), Some(4));
        // Roundtrip
        assert_eq!(p.byte_to_display_col("a你b", 0), Some((0, 0)));
        assert_eq!(p.byte_to_display_col("a你b", 1), Some((0, 1)));
        assert_eq!(p.byte_to_display_col("a你b", 4), Some((0, 2)));
    }

    #[test]
    fn byte_to_col_at_line_end() {
        let p = provider(5);
        // byte_offset == line_text.len() should return valid coordinates
        // pointing past the last grapheme.
        assert_eq!(p.byte_to_display_col("abc", 3), Some((0, 3)));
        assert_eq!(p.byte_to_display_col("abcdefgh", 8), Some((1, 3)));
    }

    #[test]
    fn fullwidth_forms() {
        let p = provider(4);
        // U+FF01 = ！ (fullwidth exclamation), 3 bytes, 2 cols.
        let text = "a！b";
        // a(1 col) ！(2 cols) b(1 col) = 4 cols → fits in wrap_column=4.
        assert_eq!(p.compute_breaks(text), Vec::<usize>::new());
        // "a！bc" → 5 cols → break before 'c' at byte 5.
        assert_eq!(p.compute_breaks("a！bc"), vec![5]);
        // "a！b" with wrap_column=3: a(1) ！(2) → 3 cols → fits, then b at col 3 overflows.
        let p3 = provider(3);
        assert_eq!(p3.compute_breaks("a！b"), vec![4]);
    }

    #[test]
    fn hangul_double_width() {
        let p = provider(4);
        // U+AC00 = 가 (Hangul), 3 bytes, 2 cols.
        // "가가" = 4 cols → fits.
        assert_eq!(p.compute_breaks("가가"), Vec::<usize>::new());
        // "가가가" = 6 cols → break before 3rd at byte 6.
        assert_eq!(p.compute_breaks("가가가"), vec![6]);
    }

    #[test]
    fn char_display_width_coverage() {
        assert_eq!(char_display_width('a'), 1);
        assert_eq!(char_display_width(' '), 1);
        assert_eq!(char_display_width('你'), 2);
        assert_eq!(char_display_width('好'), 2);
        assert_eq!(char_display_width('\u{FF01}'), 2);
        assert_eq!(char_display_width('\u{FFE5}'), 2);
        assert_eq!(char_display_width('\u{AC00}'), 2);
        assert_eq!(char_display_width('\u{D7AF}'), 2);
        assert_eq!(char_display_width('\u{20000}'), 2);
    }

    // ── Navigation pattern tests (verifying gj/gk/g0/g$/g^ algorithms) ──

    #[test]
    fn gj_single_wrap_traversal() {
        // Simulates gj on a wrapped line: line "abcdefghij" with wrap at 5.
        // Cursor at byte 2 (sub-line 0, col 2). gj should move to sub-line 1, col 2 = byte 7.
        let p = provider(5);
        let text = "abcdefghij"; // wraps at byte 5

        // Current position: byte 2
        let (sub, col) = p.byte_to_display_col(text, 2).unwrap();
        assert_eq!((sub, col), (0, 2));

        // Move to next sub-line, same column
        let next_sub = sub + 1;
        let target = p.display_col_to_byte(text, next_sub, col).unwrap();
        assert_eq!(target, 7); // byte 7 = 'h' (sub-line 1, col 2)
    }

    #[test]
    fn gj_crosses_physical_line() {
        // Line 0: "abcde" (5 chars, no wrap at col 5)
        // Line 1: "fghij" (5 chars, no wrap)
        // gj from line 0 sub-line 0 col 2 should cross to line 1 sub-line 0 col 2.
        let p = provider(5);
        let line0 = "abcde";
        let line1 = "fghij";

        let count0 = p.display_line_count(line0);
        assert_eq!(count0, 1); // no wrap

        // At sub 0, can't go to sub 1 (only 1 sub-line). Cross to next physical line.
        let target = p.display_col_to_byte(line1, 0, 2).unwrap();
        assert_eq!(target, 2); // 'h' in line 1
    }

    #[test]
    fn gk_crosses_physical_line_to_last_subline() {
        // Line 0: "abcdefghij" (wraps at 5, 2 sub-lines)
        // Line 1: "xyz"
        // gk from line 1 sub-line 0 should go to line 0's LAST sub-line.
        let p = provider(5);
        let line0 = "abcdefghij";

        let last_sub = p.display_line_count(line0).saturating_sub(1);
        assert_eq!(last_sub, 1); // sub-line 1

        let target = p.display_col_to_byte(line0, last_sub, 2).unwrap();
        assert_eq!(target, 7); // byte 7 = 'h' (sub-line 1 col 2)
    }

    #[test]
    fn gj_with_count_across_wraps() {
        // Line: "a".repeat(20) with wrap at 5 -> 4 sub-lines.
        // Starting at sub 0 col 0, 3gj should land on sub 3.
        let p = provider(5);
        let text = "a".repeat(20); // breaks at [5, 10, 15]

        assert_eq!(p.display_line_count(&text), 4);

        // Simulate 3gj from (sub=0, col=0)
        let mut sub = 0;
        for _ in 0..3 {
            sub += 1;
        }
        assert_eq!(sub, 3);

        let target = p.display_col_to_byte(&text, sub, 0).unwrap();
        assert_eq!(target, 15); // start of sub-line 3
    }

    #[test]
    fn g0_returns_subline_start() {
        let p = provider(5);
        let text = "abcdefghij"; // breaks at [5]

        // Cursor at byte 7 (sub-line 1, col 2)
        let (sub, _col) = p.byte_to_display_col(text, 7).unwrap();
        assert_eq!(sub, 1);

        // g0 = start of sub-line 1 = byte 5
        let start = p.display_col_to_byte(text, sub, 0).unwrap();
        assert_eq!(start, 5);
    }

    #[test]
    fn g_dollar_returns_last_grapheme_before_break() {
        let p = provider(5);
        let text = "abcdefghij"; // breaks at [5]

        // Cursor at byte 2 (sub-line 0, col 2)
        let (sub, _) = p.byte_to_display_col(text, 2).unwrap();
        assert_eq!(sub, 0);

        // g$ on sub-line 0: last grapheme before sub-line 1 start
        let next_start = p.display_col_to_byte(text, sub + 1, 0).unwrap();
        assert_eq!(next_start, 5); // sub-line 1 starts at byte 5
                                   // Last grapheme of sub-line 0 starts at byte 4 ('e')
    }

    #[test]
    fn g_dollar_on_last_subline_is_line_end() {
        let p = provider(5);
        let text = "abcdefghij"; // breaks at [5], 2 sub-lines

        // On sub-line 1 (the last), g$ should return end of line
        let (sub, _) = p.byte_to_display_col(text, 7).unwrap();
        assert_eq!(sub, 1);

        let count = p.display_line_count(text);
        assert_eq!(count, 2);
        // sub + 1 >= count, so g$ delegates to $ (end of physical line)
    }

    #[test]
    fn g_caret_finds_first_nonblank_in_subline() {
        let p = provider(10);
        let text = "hello     world test"; // breaks at [10]
                                           // Sub-line 0: "hello     " (10 chars)
                                           // Sub-line 1: "world test"

        // First non-blank of sub-line 1 starts at byte 10 ('w')
        let start = p.display_col_to_byte(text, 1, 0).unwrap();
        assert_eq!(start, 10);
        let sub_text = &text[start..];
        // "world test" — first non-blank is at offset 0 within sub-line
        assert_eq!(sub_text.chars().next(), Some('w'));
    }

    #[test]
    fn sticky_column_preserved_across_sublines() {
        let p = provider(5);
        let text = "abcdefghij"; // breaks at [5]

        // Start at col 3 on sub-line 0
        let byte0 = p.display_col_to_byte(text, 0, 3).unwrap();
        assert_eq!(byte0, 3); // 'd'

        // Move to sub-line 1 with target_col 3
        let byte1 = p.display_col_to_byte(text, 1, 3).unwrap();
        assert_eq!(byte1, 8); // 'i' (sub-line 1, col 3)

        // Move back to sub-line 0 with target_col 3
        let byte_back = p.display_col_to_byte(text, 0, 3).unwrap();
        assert_eq!(byte_back, 3); // 'd' again
    }
}
