//! UTF-8 character boundary and word-character helpers.
//!
//! These are pure, allocation-free utilities for navigating byte offsets to
//! valid UTF-8 character boundaries within a `&str`.  They live in the
//! `primitives` layer so that every higher layer (effects, grammar, commands,
//! execution) can share a single canonical implementation without creating
//! upward dependency violations.

/// Move `offset` forward to the next UTF-8 character boundary.
///
/// If `offset` is already at or past the end of `text`, returns `text.len()`.
#[must_use]
pub const fn next_char_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let mut pos = offset + 1;
    while pos < text.len() && !text.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

/// Move `offset` backward to the previous UTF-8 character boundary.
///
/// If `offset` is already 0, returns 0.
#[must_use]
pub const fn prev_char_boundary(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let mut pos = offset - 1;
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Snap a byte offset backward to the nearest UTF-8 character boundary.
///
/// Equivalent to Neovim's `utf_head_off` — if `offset` lands inside a
/// multibyte character (on a continuation byte `10xxxxxx`), walk backward
/// to the lead byte.  If already on a boundary, return unchanged.
///
/// Returns `text.len()` when `offset >= text.len()`.
#[must_use]
pub const fn snap_to_char_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let bytes = text.as_bytes();
    let mut pos = offset;
    // UTF-8 continuation bytes match 10xx_xxxx (0x80..=0xBF).
    #[expect(
        clippy::indexing_slicing,
        reason = "loop condition `pos > 0` together with the `pos < text.len()` early-return above guarantee `pos` is a valid index"
    )]
    while pos > 0 && (bytes[pos] & 0xC0) == 0x80 {
        pos -= 1;
    }
    pos
}

/// Returns `true` if `c` is a word character (alphanumeric or underscore).
///
/// Uses `char::is_alphanumeric()` from the standard library, which covers all
/// Unicode alphabetic and numeric categories, not just ASCII. This means
/// accented letters, CJK characters, and other Unicode word characters are
/// correctly recognised.
///
/// This is the canonical, Unicode-aware word-character predicate shared by
/// the changeset position mapper (`Assoc::AfterWord`/`BeforeWord`) and the
/// word-boundary motions (`*`, `#`, `g*`, `g#`).
///
/// **Note:** The regex engine's `\w` matcher (`regex/matchers.rs`) is
/// intentionally ASCII-only (`[0-9A-Za-z_]`) to match Vim regex semantics
/// and is kept as a separate `const fn`.
#[inline]
#[must_use]
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_boundary_ascii() {
        let s = "hello";
        assert_eq!(next_char_boundary(s, 0), 1);
        assert_eq!(next_char_boundary(s, 4), 5);
        // past end clamps to len
        assert_eq!(next_char_boundary(s, 5), 5);
        assert_eq!(next_char_boundary(s, 99), 5);
    }

    #[test]
    fn prev_boundary_ascii() {
        let s = "hello";
        assert_eq!(prev_char_boundary(s, 0), 0);
        assert_eq!(prev_char_boundary(s, 1), 0);
        assert_eq!(prev_char_boundary(s, 5), 4);
    }

    #[test]
    fn next_boundary_multibyte() {
        // '£' is U+00A3, encoded as 0xC2 0xA3 (2 bytes)
        let s = "£x";
        assert_eq!(s.len(), 3);
        // offset 0 → next boundary is 2 (past the 2-byte '£')
        assert_eq!(next_char_boundary(s, 0), 2);
        // offset 2 → next boundary is 3 ('x')
        assert_eq!(next_char_boundary(s, 2), 3);
    }

    #[test]
    fn prev_boundary_multibyte() {
        // '£' is U+00A3, encoded as 0xC2 0xA3 (2 bytes)
        let s = "£x";
        // offset 3 → prev boundary is 2 ('x')
        assert_eq!(prev_char_boundary(s, 3), 2);
        // offset 2 → prev boundary is 0 ('£')
        assert_eq!(prev_char_boundary(s, 2), 0);
        // offset 0 → clamped to 0
        assert_eq!(prev_char_boundary(s, 0), 0);
    }

    // ── snap_to_char_boundary ────────────────────────────────────────

    #[test]
    fn snap_ascii() {
        let s = "hello";
        for i in 0..s.len() {
            assert_eq!(
                snap_to_char_boundary(s, i),
                i,
                "ASCII byte {i} should be its own boundary"
            );
        }
        // past end → clamp
        assert_eq!(snap_to_char_boundary(s, 5), 5);
        assert_eq!(snap_to_char_boundary(s, 99), 5);
    }

    #[test]
    fn snap_empty() {
        assert_eq!(snap_to_char_boundary("", 0), 0);
        assert_eq!(snap_to_char_boundary("", 5), 0);
    }

    #[test]
    fn snap_2byte() {
        // '£' = U+00A3 = 0xC2 0xA3 (2 bytes)
        let s = "£x";
        assert_eq!(s.len(), 3);
        assert_eq!(snap_to_char_boundary(s, 0), 0); // lead byte of £
        assert_eq!(snap_to_char_boundary(s, 1), 0); // mid-char → snap to 0
        assert_eq!(snap_to_char_boundary(s, 2), 2); // 'x'
    }

    #[test]
    fn snap_3byte() {
        // '北' = U+5317 = 0xE5 0x8C 0x97 (3 bytes)
        let s = "a北b";
        assert_eq!(s.len(), 5);
        assert_eq!(snap_to_char_boundary(s, 0), 0); // 'a'
        assert_eq!(snap_to_char_boundary(s, 1), 1); // lead byte of 北
        assert_eq!(snap_to_char_boundary(s, 2), 1); // mid-char → snap to 1
        assert_eq!(snap_to_char_boundary(s, 3), 1); // mid-char → snap to 1
        assert_eq!(snap_to_char_boundary(s, 4), 4); // 'b'
    }

    #[test]
    fn snap_4byte() {
        // '🎉' = U+1F389 = 0xF0 0x9F 0x8E 0x89 (4 bytes)
        let s = "🎉";
        assert_eq!(s.len(), 4);
        assert_eq!(snap_to_char_boundary(s, 0), 0); // lead byte
        assert_eq!(snap_to_char_boundary(s, 1), 0); // mid → snap
        assert_eq!(snap_to_char_boundary(s, 2), 0); // mid → snap
        assert_eq!(snap_to_char_boundary(s, 3), 0); // mid → snap
    }

    #[test]
    fn snap_mixed_widths() {
        // "a£北🎉z" = 1 + 2 + 3 + 4 + 1 = 11 bytes
        let s = "a£北🎉z";
        assert_eq!(s.len(), 11);
        // 'a' at 0
        assert_eq!(snap_to_char_boundary(s, 0), 0);
        // '£' at 1..3
        assert_eq!(snap_to_char_boundary(s, 1), 1);
        assert_eq!(snap_to_char_boundary(s, 2), 1);
        // '北' at 3..6
        assert_eq!(snap_to_char_boundary(s, 3), 3);
        assert_eq!(snap_to_char_boundary(s, 4), 3);
        assert_eq!(snap_to_char_boundary(s, 5), 3);
        // '🎉' at 6..10
        assert_eq!(snap_to_char_boundary(s, 6), 6);
        assert_eq!(snap_to_char_boundary(s, 7), 6);
        assert_eq!(snap_to_char_boundary(s, 8), 6);
        assert_eq!(snap_to_char_boundary(s, 9), 6);
        // 'z' at 10
        assert_eq!(snap_to_char_boundary(s, 10), 10);
    }
}

/// Canonical combining mark detection for the entire codebase.
///
/// Used by both `CharClass::classify()` (word boundary) and the composing
/// character accumulator in `grammar/handlers/awaiting.rs`. There MUST be
/// only ONE copy of this function — any divergence is a correctness bug.
#[must_use]
pub const fn is_combining_mark(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        // ── Latin / General ──
        0x0300..=0x036F  // Combining Diacritical Marks
        | 0x1AB0..=0x1AFF  // Combining Diacritical Marks Extended
        | 0x1DC0..=0x1DFF  // Combining Diacritical Marks Supplement
        | 0x20D0..=0x20FF  // Combining Diacritical Marks for Symbols
        | 0xFE20..=0xFE2F  // Combining Half Marks
        // ── Cyrillic ──
        | 0x0483..=0x0489
        // ── Hebrew ──
        | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7
        // ── Arabic ──
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670
        | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8 | 0x06EA..=0x06ED
        // ── Syriac ──
        | 0x0730..=0x074A
        // ── Devanagari ──
        | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0951..=0x0957 | 0x0962..=0x0963
        // ── Bengali ──
        | 0x0981..=0x0983 | 0x09BC | 0x09BE..=0x09C4 | 0x09C7..=0x09C8
        | 0x09CB..=0x09CD | 0x09D7 | 0x09E2..=0x09E3
        // ── Gujarati ──
        | 0x0A81..=0x0A83 | 0x0ABC | 0x0ABE..=0x0AC5 | 0x0AC7..=0x0AC9
        | 0x0ACB..=0x0ACD
        // ── Tamil ──
        | 0x0B82..=0x0B83 | 0x0BBE..=0x0BC2 | 0x0BC6..=0x0BC8
        | 0x0BCA..=0x0BCD | 0x0BD7
        // ── Telugu ──
        | 0x0C00..=0x0C04 | 0x0C3E..=0x0C56
        // ── Kannada ──
        | 0x0C80..=0x0C83 | 0x0CBC | 0x0CBE..=0x0CC4 | 0x0CC6..=0x0CC8
        | 0x0CCA..=0x0CCD | 0x0CD5..=0x0CD6
        // ── Malayalam ──
        | 0x0D00..=0x0D03 | 0x0D3B..=0x0D3C | 0x0D3E..=0x0D44 | 0x0D46..=0x0D48
        | 0x0D4A..=0x0D4D | 0x0D57
        // ── Thai ──
        | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
        // ── Lao ──
        | 0x0EB1 | 0x0EB4..=0x0EBC | 0x0EC8..=0x0ECE
        // ── Tibetan ──
        | 0x0F18..=0x0F19 | 0x0F35 | 0x0F37 | 0x0F39 | 0x0F3E..=0x0F3F
        | 0x0F71..=0x0F84 | 0x0F86..=0x0F87 | 0x0F8D..=0x0FBC
        // ── Myanmar ──
        | 0x102B..=0x103E | 0x1056..=0x1059 | 0x105E..=0x1060 | 0x1062..=0x1064
        | 0x1067..=0x106D | 0x1071..=0x1074 | 0x1082..=0x108D
        // ── Khmer ──
        | 0x17B4..=0x17D3
        // ── Balinese ──
        | 0x1B34..=0x1B44 | 0x1B6B..=0x1B73
        // ── Javanese ──
        | 0x1A17..=0x1A1B | 0xA9B4..=0xA9C0
        // ── Sundanese ──
        | 0x1BA1..=0x1BAD
        // ── Hangul Jamo combining ──
        | 0x302A..=0x302F
        // ── CJK compatibility ──
        | 0x3099..=0x309A
    )
}
