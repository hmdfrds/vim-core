//! Shared helper functions for all command modules.
//!
//! **Canonical source of truth** for line operations, character classification,
//! grapheme movement, and boundary detection. All command submodules (`motions/`,
//! `textobjects/`, `actions/`, `operators/`, `insert/`, `ex/`, `visual/`) should
//! import helpers from here rather than defining their own.
//!
//! # Two Families of Line Helpers
//!
//! **Line-number-based** (from motions):
//! - `line_start(text, line)`, `line_end(text, line)` — take a 0-indexed line number
//! - Use `memchr` for SIMD-accelerated newline scanning
//!
//! **Offset-based** (from textobjects):
//! - `line_start_for_offset(text, offset)`, `line_end_for_offset(text, offset)` — take a byte offset
//! - Use string search (`rfind`/`find`)

use memchr::{memchr, memchr_iter, memrchr};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use crate::primitives::{WordCharSet, WordKind};

// ═══════════════════════════════════════════════════════════════════════════════
// Line-Number-Based Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the byte offset of the start of a line (0-indexed line number).
/// Uses memchr for O(n) performance with SIMD on supported platforms.
#[must_use]
pub fn line_start(text: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }

    let bytes = text.as_bytes();
    let mut current_line = 0;

    for offset in memchr_iter(b'\n', bytes) {
        current_line += 1;
        if current_line == line {
            return Some(offset + 1);
        }
    }
    None
}

/// Get the byte offset of the end of a line (before newline, 0-indexed line number).
/// Uses memchr for fast newline detection.
#[must_use]
pub fn line_end(text: &str, line: usize) -> Option<usize> {
    let start = line_start(text, line)?;
    debug_assert!(
        start <= text.len(),
        "line_start returned out-of-bounds offset {start}"
    );
    let bytes = text.as_bytes().get(start..).unwrap_or_default();

    match memchr(b'\n', bytes) {
        Some(pos) => Some(start + pos),
        None => Some(text.len()),
    }
}

/// Get line number (0-indexed) from byte offset.
/// Uses `memchr_iter` for counting newlines.
#[must_use]
pub fn line_of(text: &str, offset: usize) -> usize {
    let clamped = offset.min(text.len());
    debug_assert!(
        clamped <= text.len(),
        "clamped offset {clamped} exceeds text length"
    );
    let bytes = text.as_bytes().get(..clamped).unwrap_or_default();
    memchr_iter(b'\n', bytes).count()
}

/// Get column (byte offset within line) from byte offset.
///
/// Returns the byte distance from the start of the line to the cursor
/// byte offset. This matches Neovim's col() which is byte-based.
#[must_use]
pub fn column_of(text: &str, offset: usize) -> usize {
    let clamped = offset.min(text.len());
    debug_assert!(
        clamped <= text.len(),
        "clamped offset {clamped} exceeds text length"
    );
    let bytes = text.as_bytes().get(..clamped).unwrap_or_default();

    let ls = match memrchr(b'\n', bytes) {
        Some(newline_pos) => newline_pos + 1,
        None => 0, // On first line
    };

    // Byte distance from line start to clamped offset
    clamped - ls
}

/// Count total lines in text.
///
/// Treats `\n` as a separator: `"hello\n"` has 2 lines (`"hello"` and `""`),
/// matching Neovim's buffer model where `nvim_buf_get_lines` returns
/// `["hello", ""]`, which rejoins to the original text with `\n`.
/// Uses `memchr_iter` for fast newline counting.
#[must_use]
pub fn line_count(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    memchr_iter(b'\n', text.as_bytes()).count() + 1
}

/// Get the content of a line (without newline, 0-indexed line number).
#[must_use]
pub fn line_content(text: &str, line: usize) -> Option<&str> {
    let start = line_start(text, line)?;
    let end = line_end(text, line)?;
    Some(&text[start..end])
}

// ═══════════════════════════════════════════════════════════════════════════════
// Offset-Based Line Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Find the start of the current line containing `offset` (byte offset).
///
/// Unlike `line_start()` which takes a line number, this takes a byte offset
/// and finds the start of the line that contains it.
///
/// Uses `memrchr` for SIMD-accelerated reverse newline scanning.
#[must_use]
pub fn line_start_for_offset(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let clamped = offset.min(text.len());
    let bytes = text.as_bytes().get(..clamped).unwrap_or_default();
    memrchr(b'\n', bytes).map_or(0, |pos| pos + 1)
}

/// Find the end of the current line containing `offset` (exclusive, points to \n or len).
///
/// Unlike `line_end()` which takes a line number, this takes a byte offset
/// and finds the end of the line that contains it.
///
/// Uses `memchr` for SIMD-accelerated forward newline scanning.
#[must_use]
pub fn line_end_for_offset(text: &str, offset: usize) -> usize {
    let clamped = offset.min(text.len());
    let bytes = text.as_bytes().get(clamped..).unwrap_or_default();
    memchr(b'\n', bytes).map_or(text.len(), |pos| clamped + pos)
}

/// Get the content of the current line as a slice (by byte offset).
#[must_use]
pub fn current_line(text: &str, offset: usize) -> &str {
    let start = line_start_for_offset(text, offset);
    let end = line_end_for_offset(text, offset);
    &text[start..end]
}

// ═══════════════════════════════════════════════════════════════════════════════
// Non-Blank Character Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Find the first non-blank character in a line.
/// Returns byte offset relative to line start.
/// If line is all whitespace, returns last char position per Vim behavior.
#[must_use]
pub fn first_non_blank_in_line(line: &str) -> usize {
    for (i, c) in line.char_indices() {
        if !c.is_whitespace() {
            return i;
        }
    }
    // All whitespace - return last character's byte offset (not past end).
    // Uses char_indices to ensure we land on a valid char boundary,
    // even with multi-byte whitespace (e.g., U+3000 ideographic space).
    line.char_indices().last().map_or(0, |(i, _)| i)
}

/// Find the last non-blank character in a line.
/// Returns byte offset relative to line start.
#[must_use]
pub fn last_non_blank_in_line(line: &str) -> usize {
    let mut last_non_blank = 0;
    for (i, c) in line.char_indices() {
        if !c.is_whitespace() {
            last_non_blank = i;
        }
    }
    last_non_blank
}

// ═══════════════════════════════════════════════════════════════════════════════
// Grapheme Movement
// ═══════════════════════════════════════════════════════════════════════════════

/// Move left by count graphemes.
/// Returns new byte offset.
///
/// Single-pass from the right via `DoubleEndedIterator`: skips
/// `count` graphemes backwards and subtracts their byte lengths.
/// O(count) time, O(1) memory.
#[must_use]
pub fn move_left(text: &str, offset: usize, count: u32) -> usize {
    if offset == 0 {
        return 0;
    }

    let before = &text[..offset];
    let bytes_back: usize = before
        .graphemes(true)
        .rev()
        .take(count as usize)
        .map(str::len)
        .sum();
    offset - bytes_back
}

/// Move right by count graphemes.
/// Returns new byte offset.
#[must_use]
pub fn move_right(text: &str, offset: usize, count: u32) -> usize {
    let after = &text[offset..];
    let mut graphemes = after.graphemes(true);
    let mut advanced = 0;

    for _ in 0..count {
        if let Some(g) = graphemes.next() {
            advanced += g.len();
        } else {
            break;
        }
    }

    offset + advanced
}

/// Move left but stay on same line.
#[must_use]
pub fn move_left_on_line(text: &str, offset: usize, count: u32) -> usize {
    let line = line_of(text, offset);
    let line_start_pos = line_start(text, line).unwrap_or(0);

    let new_offset = move_left(text, offset, count);
    new_offset.max(line_start_pos)
}

/// Move right but stay on same line (before newline).
#[must_use]
pub fn move_right_on_line(text: &str, offset: usize, count: u32) -> usize {
    let line = line_of(text, offset);
    let line_end_offset = line_end(text, line).unwrap_or(text.len());

    let new_offset = move_right(text, offset, count);

    // Don't go past end of line
    new_offset.min(line_end_offset)
}

/// Get grapheme count from byte offset to line end.
#[must_use]
pub fn graphemes_to_line_end(text: &str, offset: usize) -> usize {
    let line = line_of(text, offset);
    let line_end_pos = line_end(text, line).unwrap_or(text.len());
    text[offset..line_end_pos].graphemes(true).count()
}

/// Convert a grapheme column index to a byte offset on the given line.
///
/// Clamps the column to the last valid grapheme if it exceeds the line length.
///
/// # Arguments
/// * `text` - The document text
/// * `line_idx` - 0-indexed line number
/// * `gcol` - Grapheme column index (0-indexed)
///
/// # Returns
/// Byte offset in `text` corresponding to the grapheme column.
#[must_use]
pub fn gcol_to_byte(text: &str, line_idx: usize, gcol: usize) -> usize {
    let ls = line_start(text, line_idx).unwrap_or(0);
    let le = line_end(text, line_idx).unwrap_or(text.len());
    let line_text = &text[ls..le];

    // Single-pass: walk graphemes accumulating byte offset, stopping at `gcol`.
    // When `gcol` exceeds the grapheme count, `last_start` holds the byte offset
    // of the last grapheme boundary (equivalent to clamping to `total - 1`),
    // preserving the original two-pass behaviour without calling count() first.
    let mut byte_offset = 0usize;
    let mut last_start = 0usize; // byte start of the most recent grapheme seen
    for (i, grapheme) in line_text.graphemes(true).enumerate() {
        last_start = byte_offset;
        if i >= gcol {
            // Landed exactly on the target grapheme -- stop here.
            return byte_offset + ls;
        }
        byte_offset += grapheme.len();
    }
    // `gcol` exceeded the grapheme count: clamp to the last grapheme boundary.
    // `last_start` is 0 for an empty line, which is correct.
    last_start + ls
}

// ═══════════════════════════════════════════════════════════════════════════════
// Character Classification
// ═══════════════════════════════════════════════════════════════════════════════

/// Character class for word/WORD boundary detection.
///
/// Per Neovim `cls()` in textobject.c:
/// - 0 = whitespace
/// - 1 = punctuation/symbols
/// - 2 = keyword characters (letters, digits, underscore)
///
/// Additionally, different CJK script blocks are separate classes,
/// mirroring Neovim's `mb_get_class()` so that script boundaries
/// act as word boundaries (e.g. Kanji vs Katakana).
///
/// For WORD (big=true), all non-whitespace is class 1.
///
/// This is the single canonical classification used by both
/// word motions (`w`, `b`, `e`, `ge`) and text objects (`iw`, `aw`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CharClass {
    /// Whitespace (space, tab, newline)
    Whitespace,
    /// Punctuation/symbols
    Punctuation,
    /// Word characters (alphanumeric + underscore, or general alphabetic)
    Word,
    /// CJK Unified Ideographs (Hanzi/Kanji)
    CjkIdeograph,
    /// Hiragana
    Hiragana,
    /// Katakana
    Katakana,
    /// Hangul
    Hangul,
    /// Emoji (Emoji_Presentation, Emoji_Modifier_Base, Regional_Indicator)
    Emoji,
}

impl CharClass {
    /// Classify a character using the given word character set.
    ///
    /// # Arguments
    ///
    /// * `c` - The character to classify
    /// * `kind` - `WordKind::Word` for word boundaries, `WordKind::WORD` for WORD (whitespace-only)
    /// * `word_chars` - Precomputed word character set from `iskeyword` option
    #[inline]
    #[must_use]
    pub fn classify(c: char, kind: WordKind, word_chars: &WordCharSet) -> Self {
        if c.is_whitespace() {
            return Self::Whitespace;
        }

        let cp = c as u32;

        // Neovim `utf_class_tab` class-0 ranges that Rust's `is_whitespace()`
        // does not cover (e.g. ZWSP U+200B is Cf, not Zs, so Rust says false).
        if is_neovim_blank(cp) {
            return Self::Whitespace;
        }

        if kind.is_big() {
            // For WORD: all non-whitespace is the same class
            return Self::Word;
        }

        // ASCII fast path — use bitmap lookup from iskeyword
        if cp < 0x80 {
            // `cp < 0x80` guarantees the conversion succeeds; the bitmap only
            // has room for the 128 ASCII slots.
            let is_word = u8::try_from(cp).is_ok_and(|byte| word_chars.contains_ascii(byte));
            return if is_word {
                Self::Word
            } else {
                Self::Punctuation
            };
        }

        // Unicode combining marks (category M) — treat as Word so they don't
        // create false word boundaries with their base character.
        if is_combining_mark(c) {
            return Self::Word;
        }

        Self::classify_unicode(c, cp, word_chars)
    }

    /// Classify a non-ASCII, non-combining Unicode character.
    ///
    /// Ordering is critical. The priority is:
    /// 1. Emoji (before `is_alphabetic` — some emoji have Alphabetic property)
    /// 2. CJK ideographs (including 0x3300-0x33FF compatibility block)
    /// 3. Hiragana / Katakana / Hangul
    /// 4. Non-Latin punctuation (explicit table by Unicode block)
    /// 5. Unicode symbol ranges (currency, arrows, math, etc.)
    /// 6. Superscript / subscript / Braille
    /// 7. `is_alphabetic()` fallback
    /// 8. Punctuation fallback
    fn classify_unicode(c: char, cp: u32, word_chars: &WordCharSet) -> Self {
        // ── 1. Emoji ─────────────────────────────────────────────────────
        // Must come before is_alphabetic() because some emoji codepoints
        // (e.g. Regional Indicators U+1F1E6..U+1F1FF) have the Unicode
        // Alphabetic property.
        if is_emoji_for_word_class(cp) {
            return Self::Emoji;
        }

        match cp {
            // ── 2. CJK ideographs ────────────────────────────────────────
            0x3040..=0x309F => Self::Hiragana,
            0x30A0..=0x30FF | 0x31F0..=0x31FF => Self::Katakana,
            // 0x3300-0x33FF is the CJK Compatibility block
            0x3300..=0x33FF | 0x4E00..=0x9FFF | 0x3400..=0x4DBF => Self::CjkIdeograph,
            0x20000..=0x2A6DF | 0x2A700..=0x2CEAF | 0x2CEB0..=0x2EBEF | 0x30000..=0x3134F => {
                Self::CjkIdeograph
            }
            0xF900..=0xFAFF => Self::CjkIdeograph,
            // CJK Compatibility Ideographs Supplement
            0x2F800..=0x2FA1F => Self::CjkIdeograph,

            // ── 3. Hangul ────────────────────────────────────────────────
            0xAC00..=0xD7AF => Self::Hangul,
            0x1100..=0x11FF
            | 0x3130..=0x318F
            | 0xA960..=0xA97F
            | 0xD7B0..=0xD7FF
            | 0xFFA0..=0xFFDC => Self::Hangul, // Halfwidth Hangul compatibility jamo

            // ── CJK symbols / fullwidth punctuation ──────────────────────
            0x3000..=0x303F => Self::Punctuation,
            0xFF00..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65 => {
                Self::Punctuation
            }
            0xFF10..=0xFF19 | 0xFF21..=0xFF3A | 0xFF41..=0xFF5A => Self::Word,

            // ── 4. Non-Latin punctuation ─────────────────────────────────
            // Greek
            0x037E | 0x0387 => Self::Punctuation,
            // Armenian
            0x055A..=0x055F => Self::Punctuation,
            // Hebrew
            0x05BE | 0x05C0 | 0x05C3 | 0x05C6 | 0x05F3 | 0x05F4 => Self::Punctuation,
            // Arabic
            0x060C..=0x061F | 0x066A..=0x066D | 0x06D4 => Self::Punctuation,
            // Devanagari
            0x0964..=0x0970 => Self::Punctuation,
            // Thai
            0x0E2F | 0x0E46 | 0x0E4F..=0x0E5B => Self::Punctuation,
            // Myanmar
            0x104A..=0x104F => Self::Punctuation,
            // Georgian
            0x10FB => Self::Punctuation,
            // Ethiopic
            0x1361..=0x1368 => Self::Punctuation,
            // Mongolian
            0x1800..=0x180A => Self::Punctuation,
            // Syriac
            0x0700..=0x070D => Self::Punctuation,
            // Tibetan
            0x0F04..=0x0F12 | 0x0F3A..=0x0F3D | 0x0FD0..=0x0FD4 => Self::Punctuation,
            // Canadian Syllabics
            0x1400 | 0x166D..=0x166E => Self::Punctuation,
            // Khmer
            0x17D4..=0x17DA => Self::Punctuation,
            // General punctuation block
            0x2010..=0x2027 | 0x2030..=0x205E => Self::Punctuation,

            // ── 5. Unicode symbol ranges ─────────────────────────────────
            // Must come before is_alphabetic() — math alphanumeric
            // (0x1D400-0x1D7FF) has the Alphabetic property.
            0x20A0..=0x20CF => Self::Punctuation, // Currency symbols
            0x2190..=0x21FF => Self::Punctuation, // Arrows
            0x2200..=0x22FF => Self::Punctuation, // Mathematical operators
            0x1D400..=0x1D7FF => Self::Punctuation, // Math alphanumeric symbols

            // ── 6. Superscript / subscript / Braille ─────────────────────
            0x00B2 | 0x00B3 | 0x00B9 => Self::Punctuation, // Latin-1 superscripts
            0x2070..=0x209F => Self::Punctuation,          // Superscript/subscript block
            0x2800..=0x28FF => Self::Punctuation,          // Braille patterns

            // ── 7. is_alphabetic() fallback ──────────────────────────────
            _ => {
                if word_chars.contains(c) {
                    Self::Word
                } else {
                    Self::Punctuation
                }
            }
        }
    }

    /// Check if this is whitespace.
    #[inline]
    #[must_use]
    pub const fn is_whitespace(self) -> bool {
        matches!(self, Self::Whitespace)
    }
}

// Re-export from primitives — canonical location for is_combining_mark.
pub use crate::primitives::is_combining_mark;

/// Check if a codepoint should be classified as Emoji for word-class purposes.
///
/// Covers Emoji_Presentation, Emoji_Modifier_Base, and Regional_Indicator
/// codepoints. Reuses ranges from `is_neovim_emoji_wide()` and adds lower
/// emoji blocks (Dingbats, Misc Symbols, etc.) that are commonly used as
/// standalone emoji.
///
/// This function must be checked BEFORE `is_alphabetic()` because Regional
/// Indicator symbols (U+1F1E6..U+1F1FF) have the Unicode Alphabetic property.
#[inline]
const fn is_emoji_for_word_class(cp: u32) -> bool {
    matches!(cp,
        // Zero Width Joiner — joins adjacent emoji into a single grapheme
        // cluster (e.g., 👨‍👩‍👧). Classified as Emoji so word motions
        // treat the entire ZWJ sequence as one word, matching Vim behavior.
        0x200D |
        // Variation Selector 16 (emoji presentation) — follows a base
        // character to request emoji display (e.g., ☺️ vs ☺).
        0xFE0F |
        // Miscellaneous Symbols (common emoji like ☀ ☁ ☂ ♠ ♣ ♥ ♦)
        0x2600..=0x26FF |
        // Dingbats (✂ ✈ ✉ ✏ ✒ ✔ ✖ ❌ ❤ etc.)
        0x2700..=0x27BF |
        // Emoji Modifier Fitzpatrick (skin tone modifiers)
        0x1F3FB..=0x1F3FF |
        // Regional Indicator Symbols (flag emoji components)
        0x1F1E6..=0x1F1FF |
        // Common emoji blocks (same as is_neovim_emoji_wide)
        0x1F300..=0x1F5FF |   // Misc Symbols and Pictographs
        0x1F600..=0x1F64F |   // Emoticons
        0x1F680..=0x1F6FF |   // Transport and Map
        0x1F900..=0x1F9FF |   // Supplemental Symbols and Pictographs
        0x1FA00..=0x1FA6F |   // Chess Symbols
        0x1FA70..=0x1FAFF |   // Symbols and Pictographs Extended-A
        0x1F000..=0x1F02F |   // Mahjong Tiles
        0x1F030..=0x1F09F |   // Domino Tiles
        0x1F0A0..=0x1F0FF |   // Playing Cards
        0x1F100..=0x1F1DF     // Enclosed Alphanumeric Supplement (partial)
    )
}

/// Check if a codepoint is "blank" (class 0) in Neovim's `utf_class_tab`
/// but NOT covered by Rust's `char::is_whitespace()`.
///
/// Neovim's class-0 entries from `mbyte.c` `utf_class_tab`:
/// - 0x1680 (Ogham space mark) — Rust `is_whitespace` = true, skip
/// - 0x2000..=0x200B (various width spaces + ZWSP) — Rust misses 0x200B
/// - 0x2028..=0x2029 (line/paragraph separators) — Rust `is_whitespace` = true, skip
/// - 0x202F (narrow no-break space) — Rust `is_whitespace` = true, skip
/// - 0x205F (medium mathematical space) — Rust `is_whitespace` = true, skip
/// - 0x3000 (ideographic space) — Rust `is_whitespace` = true, skip
///
/// In practice the only gap is U+200B (ZWSP), which is category Cf.
#[inline]
const fn is_neovim_blank(cp: u32) -> bool {
    // U+200B is the only character in Neovim's class-0 table that
    // Rust's `char::is_whitespace()` does not recognize. We keep the
    // match arm extensible in case future Neovim versions add more.
    matches!(cp, 0x200B)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Character Boundary Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the character at a byte offset, if valid.
///
/// If `offset` lands inside a multibyte character, it is snapped backward
/// to the lead byte (matching Neovim's `utf_head_off` semantics).
/// In debug builds, a mid-char offset is logged to stderr so upstream
/// callers can be identified and fixed.
#[inline]
#[must_use]
pub fn char_at(text: &str, offset: usize) -> Option<char> {
    if offset >= text.len() {
        return None;
    }
    let safe = crate::primitives::text_util::snap_to_char_boundary(text, offset);
    #[cfg(debug_assertions)]
    if safe != offset {
        // Debug-only diagnostic: catch callers that pass non-boundary offsets.
        // Production builds drop this; clippy::print_stderr is intentionally
        // suppressed because this fires only on a programming error during
        // development and the diagnostic is the whole point of the branch.
        #[expect(
            clippy::print_stderr,
            reason = "debug-only diagnostic for caller bug detection in dev builds"
        )]
        {
            eprintln!(
                "[vim-core] char_at: offset {offset} snapped to {safe} (not a char boundary)"
            );
        }
    }
    text[safe..].chars().next()
}

/// Get the character class at a byte offset.
#[inline]
#[must_use]
pub fn class_at(
    text: &str,
    offset: usize,
    kind: WordKind,
    word_chars: &WordCharSet,
) -> Option<CharClass> {
    char_at(text, offset).map(|c| CharClass::classify(c, kind, word_chars))
}

/// Move to the next character boundary (UTF-8 aware).
///
/// Delegates to [`crate::primitives::text_util::next_char_boundary`].
#[inline]
#[must_use]
pub const fn next_char_boundary(text: &str, offset: usize) -> usize {
    crate::primitives::text_util::next_char_boundary(text, offset)
}

/// Move to the previous character boundary (UTF-8 aware).
///
/// Delegates to [`crate::primitives::text_util::prev_char_boundary`].
#[inline]
#[must_use]
pub const fn prev_char_boundary(text: &str, offset: usize) -> usize {
    crate::primitives::text_util::prev_char_boundary(text, offset)
}

/// Check if the line at offset is blank (empty or whitespace only).
pub fn is_blank_line(text: &str, offset: usize) -> bool {
    current_line(text, offset).chars().all(char::is_whitespace)
}

/// Check if the text in `[start..end]` is all whitespace (blank line).
///
/// Use when line boundaries are already known (e.g., from [`LineIndex`](crate::commands::line_index::LineIndex)).
/// Avoids the cost of re-deriving line boundaries from a byte offset.
///
/// Only checks ASCII whitespace (`' '`, `'\t'`, `'\r'`) since Vim source files
/// virtually never contain non-ASCII whitespace on otherwise-blank lines.
#[inline]
#[must_use]
pub fn is_blank_line_range(text: &str, start: usize, end: usize) -> bool {
    text.get(start..end)
        .is_none_or(|s| s.bytes().all(|b| b == b' ' || b == b'\t' || b == b'\r'))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tab Expansion
// ═══════════════════════════════════════════════════════════════════════════════

/// Expand tab characters to spaces, maintaining column alignment.
///
/// Each tab advances to the next multiple of `tab_width` columns. Non-tab
/// characters pass through unchanged. This is the canonical implementation
/// used by both autoindent (`insert/autoindent.rs`) and indent operators
/// (`operators/indent.rs`).
///
/// # Arguments
///
/// * `text` - The text containing tabs to expand (may be a full line or just indent)
/// * `tab_width` - Number of columns per tab stop
#[must_use]
pub fn expand_tabs(text: &str, tab_width: usize) -> String {
    let mut result = String::with_capacity(text.len() * tab_width.max(1));
    expand_tabs_into(text, tab_width, &mut result);
    result
}

/// Expand tabs into spaces, appending the result to `out`.
///
/// This is the buffer-reuse variant of [`expand_tabs`]. The caller can
/// clear and reuse `out` across multiple lines to avoid per-line allocation.
///
/// Non-tab characters are counted by display width via `unicode_width`:
/// CJK ideographs and other full-width characters count as 2 columns,
/// combining marks count as 0, and all other characters count as 1.
pub fn expand_tabs_into(text: &str, tab_width: usize, out: &mut String) {
    let tab_width = tab_width.max(1); // guard against zero (division by zero in modulo)
    out.reserve(text.len());
    let mut col = 0;
    for c in text.chars() {
        if c == '\t' {
            let spaces = tab_width - (col % tab_width);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(c);
            col += UnicodeWidthChar::width(c).unwrap_or(0);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Virtual Column Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert a byte offset within a line to a virtual (screen) column.
///
/// Virtual columns account for tab expansion (each tab advances to the next
/// multiple of `tabstop` columns) and Unicode display width (CJK and other
/// full-width characters count as 2 columns, combining marks as 0). This
/// matches Neovim's `getvvcol()` behavior.
///
/// # Arguments
/// * `line` - The line text (must NOT contain newlines)
/// * `byte_offset` - Byte offset from line start (clamped to line length)
/// * `tabstop` - Tab stop width (clamped to >= 1)
///
/// # Returns
/// The 0-based virtual column at the given byte offset.
#[must_use]
pub fn byte_to_vcol(line: &str, byte_offset: usize, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    let mut byte_offset = byte_offset.min(line.len());
    while byte_offset > 0 && !line.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    let mut vcol = 0;
    let mut consumed = 0;
    for grapheme in line.graphemes(true) {
        if consumed >= byte_offset {
            break;
        }
        let g_len = grapheme.len();
        // If the byte_offset falls inside this grapheme, stop before it.
        if consumed + g_len > byte_offset {
            break;
        }
        vcol += grapheme_display_width(grapheme, vcol, tabstop);
        consumed += g_len;
    }
    vcol
}

/// Display width of a single grapheme cluster.
///
/// For a tab, advances to the next tab stop.  For multi-codepoint grapheme
/// clusters (e.g. ZWJ emoji sequences like 👨‍👩‍👧), the width matches
/// Neovim's `utf_ptr2cells` semantics:
///
/// 1. Emoji codepoints ≥ 0x1F000 that are Extended_Pictographic or
///    Regional_Indicator get width 2 (Neovim `utf_char2cells` with `p_emoji`).
/// 2. Any character followed by U+FE0F (variation selector-16, emoji
///    presentation) gets width 2, even if the base character is normally
///    narrow (e.g. ❤ U+2764).
/// 3. Otherwise, use `UnicodeWidthChar::width` for the first codepoint.
#[must_use]
pub fn grapheme_display_width(grapheme: &str, vcol: usize, tabstop: usize) -> usize {
    let mut chars = grapheme.chars();
    let first_char = match chars.next() {
        Some(c) => c,
        None => return 0,
    };
    if first_char == '\t' {
        return tabstop - (vcol % tabstop);
    }

    let cp = first_char as u32;

    // Neovim non-printable characters: displayed as <xx> (4 cells) for cp <= 0xFF,
    // or <xxxx> (6 cells) for cp > 0xFF.  This matches `utf_char2cells` in mbyte.c
    // which calls `vim_isprintc` → `utf_printable`.  ASCII control chars (0x00-0x1F
    // excluding tab handled above, and 0x7F) get width 2 in Neovim (displayed as
    // ^X), but UnicodeWidthChar handles those correctly already.  For non-ASCII
    // non-printable characters, we must override the UnicodeWidthChar result.
    if cp >= 0x100 && is_neovim_nonprintable(cp) {
        return if cp > 0xFF { 6 } else { 4 };
    }

    // Rule 1: Emoji characters >= 0x1F000 that Neovim considers "emoji-like"
    // (Extended_Pictographic or Regional_Indicator) → width 2.
    if cp >= 0x1F000 && is_neovim_emoji_wide(cp) {
        return 2;
    }

    // Rule 2: Base char followed by U+FE0F (variation selector-16) → width 2.
    // This handles emoji presentation sequences like ❤️ (U+2764 U+FE0F).
    if cp >= 0x80 {
        if let Some(second) = chars.next() {
            if second == '\u{FE0F}' {
                return 2;
            }
        }
    }

    UnicodeWidthChar::width(first_char).unwrap_or(0)
}

/// Check if a codepoint >= 0x1F000 is considered "emoji-like" by Neovim.
///
/// Neovim's `prop_is_emojilike` returns true for Extended_Pictographic and
/// Regional_Indicator characters. This is an approximation covering the
/// most common emoji ranges. Regional Indicators (U+1F1E6..U+1F1FF) are
/// used for flag emoji sequences.
#[inline]
const fn is_neovim_emoji_wide(cp: u32) -> bool {
    matches!(cp,
        // Regional Indicator Symbols (flag emoji components)
        0x1F1E6..=0x1F1FF |
        // Common emoji blocks
        0x1F300..=0x1F5FF |   // Misc Symbols and Pictographs
        0x1F600..=0x1F64F |   // Emoticons
        0x1F680..=0x1F6FF |   // Transport and Map
        0x1F900..=0x1F9FF |   // Supplemental Symbols and Pictographs
        0x1FA00..=0x1FA6F |   // Chess Symbols
        0x1FA70..=0x1FAFF |   // Symbols and Pictographs Extended-A
        // Dingbats and symbols that Neovim treats as emoji
        0x1F000..=0x1F02F |   // Mahjong Tiles
        0x1F030..=0x1F09F |   // Domino Tiles
        0x1F0A0..=0x1F0FF |   // Playing Cards
        0x1F100..=0x1F1DF     // Enclosed Alphanumeric Supplement (partial)
    )
}

/// Check if a codepoint is non-printable in Neovim's sense.
///
/// Neovim's `utf_printable()` in `mbyte.c` checks against a hardcoded table of
/// non-printable ranges.  Characters in these ranges are displayed as `<xxxx>`
/// (6 screen cells for cp > 0xFF) or `<xx>` (4 cells for cp <= 0xFF).
///
/// This function only covers codepoints >= 0x100 (matching `vim_isprintc`'s
/// delegation to `utf_printable` for `c >= 0x100`).
#[inline]
const fn is_neovim_nonprintable(cp: u32) -> bool {
    matches!(cp,
        0x070f           |
        0x180b..=0x180e  |
        0x200b..=0x200f  |
        0x202a..=0x202e  |
        0x2060..=0x206f  |
        0xd800..=0xdfff  |
        0xfeff           |
        0xfff9..=0xfffb  |
        0xfffe..=0xffff
    )
}

/// Convert a virtual (screen) column to a byte offset within a line.
///
/// Inverse of [`byte_to_vcol`]. Walks the line character by character,
/// expanding tabs, until the accumulated virtual column reaches or exceeds
/// `target_vcol`. Returns the byte offset of that position.
///
/// If `target_vcol` exceeds the line width, returns the line length.
///
/// # Arguments
/// * `line` - The line text (must NOT contain newlines)
/// * `target_vcol` - Target 0-based virtual column
/// * `tabstop` - Tab stop width (clamped to >= 1)
///
/// # Returns
/// The byte offset corresponding to the target virtual column.
#[must_use]
pub fn vcol_to_byte(line: &str, target_vcol: usize, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    let mut vcol = 0;
    let mut consumed = 0;
    for grapheme in line.graphemes(true) {
        if vcol >= target_vcol {
            return consumed;
        }
        vcol += grapheme_display_width(grapheme, vcol, tabstop);
        consumed += grapheme.len();
    }
    if vcol >= target_vcol {
        return consumed;
    }
    // target_vcol beyond end of line
    line.len()
}

/// Compute the Neovim-style curswant for a byte offset in the document.
///
/// Curswant is the virtual column of the **last screen cell** that the
/// character at `offset` occupies.  For regular characters this equals
/// `byte_to_vcol(line, col_byte, tabstop)` (the virtual column of the byte
/// position).  For a tab character it equals
/// `byte_to_vcol(line, col_byte + 1, tabstop) - 1` (the last vcol the tab
/// occupies).
///
/// This matches the value Neovim stores in `curwin->w_curswant` and uses for
/// `coladvance()` during vertical motions (`j`/`k`).
#[must_use]
pub fn curswant_of(text: &str, offset: usize, tabstop: usize) -> usize {
    let ls = line_start_for_offset(text, offset);
    let col_byte = offset.saturating_sub(ls);
    let line = text
        .get(ls..)
        .and_then(|s| s.split('\n').next())
        .unwrap_or("");

    if line.as_bytes().get(col_byte) == Some(&b'\t') {
        // Tab: curswant = last vcol of the tab
        byte_to_vcol(line, col_byte + 1, tabstop).saturating_sub(1)
    } else {
        byte_to_vcol(line, col_byte, tabstop)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Block Selection Geometry
// ═══════════════════════════════════════════════════════════════════════════════

use crate::commands::actions::BlockGeometry;
use crate::primitives::Offset;

/// Compute block selection geometry from anchor/head gap offsets.
///
/// Determines the top/bottom lines and left/right grapheme columns for a
/// rectangular (block) visual selection. This is the **single canonical
/// implementation** used by both `actions::visual_block` (block insert/append)
/// and `operators::block_visual` (block delete/yank/change/case/indent).
///
/// The `tabstop` parameter controls tab expansion width for virtual column
/// computation. Virtual columns (`left_vcol`/`right_vcol`) are tab-aware
/// and wide-char-aware, matching Neovim's `getvvcol()` behavior.
#[must_use]
pub fn compute_block_geometry(
    text: &str,
    anchor: Offset,
    head: Offset,
    tabstop: usize,
) -> BlockGeometry {
    let anchor_line = line_of(text, anchor.get());
    let head_line = line_of(text, head.get());
    let anchor_ls = line_start(text, anchor_line).unwrap_or(0);
    let head_ls = line_start(text, head_line).unwrap_or(0);

    let anchor_grapheme_col = text[anchor_ls..anchor.get()].graphemes(true).count();
    let head_grapheme_col = text[head_ls..head.get()].graphemes(true).count();

    // Extract line text (up to newline) for vcol computation.
    let anchor_line_text = text
        .get(anchor_ls..)
        .and_then(|s| s.split('\n').next())
        .unwrap_or("");
    let head_line_text = text
        .get(head_ls..)
        .and_then(|s| s.split('\n').next())
        .unwrap_or("");

    let anchor_col_byte = anchor.get() - anchor_ls;
    let head_col_byte = head.get() - head_ls;

    let anchor_vcol = byte_to_vcol(anchor_line_text, anchor_col_byte, tabstop);
    let head_vcol = byte_to_vcol(head_line_text, head_col_byte, tabstop);

    BlockGeometry {
        top_line: anchor_line.min(head_line),
        bot_line: anchor_line.max(head_line),
        left_gcol: anchor_grapheme_col.min(head_grapheme_col),
        right_gcol: anchor_grapheme_col.max(head_grapheme_col),
        left_vcol: anchor_vcol.min(head_vcol),
        right_vcol: anchor_vcol.max(head_vcol),
        anchor,
        head,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Regex Escaping
// ═══════════════════════════════════════════════════════════════════════════════

/// Escape Vim regex metacharacters in a word for use in search patterns.
///
/// Used by `*`/`#` (star search) to prevent word characters like `.`, `$`, `^`
/// from being interpreted as regex operators when wrapped in `\<...\>` word
/// boundary markers.
///
/// Escapes: `.` `*` `^` `$` `~` `[` `]` `\` `/`
#[must_use]
pub fn vim_regex_escape(word: &str) -> compact_str::CompactString {
    use compact_str::CompactString;

    // Fast path: no metacharacters (common case for identifiers)
    if !word.bytes().any(|b| {
        matches!(
            b,
            b'.' | b'*' | b'^' | b'$' | b'~' | b'[' | b']' | b'\\' | b'/'
        )
    }) {
        return CompactString::from(word);
    }

    let mut escaped = CompactString::with_capacity(word.len() + 4);
    for c in word.chars() {
        match c {
            '.' | '*' | '^' | '$' | '~' | '[' | ']' | '\\' | '/' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    escaped
}

// ═══════════════════════════════════════════════════════════════════════════════
// Search input parsing
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a raw search input (`/pattern/offset`) into the pattern and offset.
///
/// Vim search syntax: `/pattern[/offset]` where offset can be:
/// - `e[+N]` or `e[-N]` — cursor at end of match (+ N chars)
/// - `s[+N]` or `b[+N]` — cursor at start/begin (+ N chars)
/// - `+N` or `-N` — N lines below/above the match line
///
/// The trailing delimiter is optional. If the input ends with the offset
/// portion, both `/pattern/e` and `/pattern/e+2` are accepted.
///
/// Returns `(pattern, SearchOffset)`.
#[must_use]
pub fn parse_search_input(input: &str) -> (&str, crate::state::SearchOffset) {
    use crate::state::SearchOffset;

    // Find the offset separator: a `/` (or `?`) that isn't escaped.
    // We scan from the end, looking for an unescaped delimiter.
    let pattern_end = find_offset_separator(input);

    match pattern_end {
        Some(sep_pos) => {
            let pattern = &input[..sep_pos];
            let offset_str = &input[sep_pos + 1..];
            let offset = parse_offset_str(offset_str);
            (pattern, offset)
        }
        None => (input, SearchOffset::NONE),
    }
}

/// Find the position of the offset separator in the search input.
///
/// The separator is a `/` that isn't the first character and isn't preceded
/// by `\` (escaped). Returns the byte position of the separator, or `None`.
fn find_offset_separator(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    // Search backward for unescaped `/`
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        if bytes.get(i) == Some(&b'/') || bytes.get(i) == Some(&b'?') {
            // Check it's not escaped
            let backslashes = bytes
                .get(..i)
                .map_or(0, |s| s.iter().rev().take_while(|&&b| b == b'\\').count());
            if backslashes % 2 == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Parse the offset string portion (after the separator).
fn parse_offset_str(s: &str) -> crate::state::SearchOffset {
    use crate::state::SearchOffset;

    let s = s.trim();
    if s.is_empty() {
        return SearchOffset::NONE;
    }

    // `/pat/e` or `/pat/e+N` or `/pat/e-N`
    if let Some(rest) = s.strip_prefix('e') {
        let n = parse_signed_number(rest);
        return SearchOffset::End(n);
    }

    // `/pat/s+N` or `/pat/b+N` (start/begin)
    if let Some(rest) = s.strip_prefix('s').or_else(|| s.strip_prefix('b')) {
        let n = parse_signed_number(rest);
        return SearchOffset::Start(n);
    }

    // `/pat/+N` or `/pat/-N` (line offset)
    let n = parse_signed_number(s);
    if n != 0 || s.starts_with('+') || s.starts_with('-') || s.starts_with('0') {
        return SearchOffset::Lines(n);
    }

    SearchOffset::NONE
}

/// Parse an optional signed number from a string like `+3`, `-2`, ``, `5`.
fn parse_signed_number(s: &str) -> i32 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    s.parse::<i32>().unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Chained search splitting
// ═══════════════════════════════════════════════════════════════════════════════

/// A single segment in a chained search (`/foo/;?bar`).
///
/// Vim supports semicolon-chained searches where each segment starts from the
/// position found by the previous segment.  The first segment inherits the
/// direction from the command-line prompt; subsequent segments get their
/// direction from the `;/` or `;?` delimiter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchChainSegment<'a> {
    /// The raw search input for this segment (pattern + optional offset).
    pub input: &'a str,
    /// `Some(direction)` for chained segments (from `;/` or `;?`).
    /// `None` for the first segment (inherits the command-line direction).
    pub direction_override: Option<crate::primitives::Direction>,
}

/// Split a raw search input at unescaped `;/` and `;?` boundaries.
///
/// Vim's `:help search-commands` describes chained searches:
///
/// > The search command can be followed by a `;` to do another search.
/// > For example: `/foo/;/bar` first searches for "foo", then for "bar"
/// > starting from the match position.
///
/// The input arrives without the leading `/` or `?` prompt character.
/// For example, if the user types `/foo/;?bar<CR>`, the input is `foo/;?bar`.
///
/// Each segment is a raw search input that can be passed to
/// [`parse_search_input`] to extract its pattern and offset.
///
/// # Returns
///
/// A vector of segments.  The first segment always has
/// `direction_override == None` (inherits from the command-line prompt).
/// Subsequent segments have `Some(Forward)` or `Some(Backward)`.
///
/// If the input contains no `;/` or `;?`, returns a single segment
/// covering the entire input.
#[must_use]
pub fn split_search_chain(input: &str) -> Vec<SearchChainSegment<'_>> {
    use crate::primitives::Direction;

    let mut segments = Vec::new();
    let bytes = input.as_bytes();
    let mut seg_start = 0;
    let mut pending_dir: Option<Direction> = None; // None for first segment
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Skip escaped character
            i = (i + 2).min(bytes.len());
            continue;
        }

        if bytes[i] == b';' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'/' || next == b'?' {
                // Chain delimiter found: end current segment, start next
                segments.push(SearchChainSegment {
                    input: &input[seg_start..i],
                    direction_override: pending_dir,
                });
                pending_dir = Some(if next == b'/' {
                    Direction::Forward
                } else {
                    Direction::Backward
                });
                i += 2;
                seg_start = i;
                continue;
            }
        }

        i += 1;
    }

    // Push the final segment
    segments.push(SearchChainSegment {
        input: &input[seg_start..],
        direction_override: pending_dir,
    });

    segments
}

// ═══════════════════════════════════════════════════════════════════════════════
// Case Transformation
// ═══════════════════════════════════════════════════════════════════════════════

/// Toggle case of each character in the input string.
///
/// Uppercase characters become lowercase and vice versa. Non-alphabetic
/// characters are preserved unchanged. This is the canonical implementation
/// used by both `actions::case` (single-char `~`) and `operators::case` (`g~`).
#[must_use]
pub fn toggle_case_chars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_uppercase() {
            result.extend(ch.to_lowercase());
        } else if ch.is_lowercase() {
            result.extend(ch.to_uppercase());
        } else {
            result.push(ch);
        }
    }
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Fold-Aware Snapping
// ═══════════════════════════════════════════════════════════════════════════════

/// Snap a byte offset to a fold boundary if it falls inside a closed fold.
///
/// If `pos` is inside a closed fold:
/// - `Direction::Backward` → returns the byte offset of the fold's first line start
/// - `Direction::Forward`  → returns the byte offset of the fold's last line end
///
/// If `pos` is not inside a fold, returns `pos` unchanged.
///
/// This is the single shared helper used by fold-aware operators, search,
/// word motions, visual entry, and text objects.
#[must_use]
pub fn fold_snap(
    text: &str,
    pos: usize,
    direction: crate::primitives::Direction,
    fold_provider: &dyn crate::document::FoldProvider,
) -> usize {
    use crate::primitives::LineNumber;

    let line = line_of(text, pos);
    let Some((fold_start, fold_end)) = fold_provider.enclosing_fold(LineNumber::new(line)) else {
        return pos;
    };

    match direction {
        crate::primitives::Direction::Backward => {
            // Snap to start of the first folded line.
            line_start(text, fold_start.get()).unwrap_or(pos)
        }
        crate::primitives::Direction::Forward => {
            // Snap to end of the last folded line (byte offset of newline or text end).
            line_end(text, fold_end.get()).unwrap_or(pos)
        }
    }
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod helpers_tests;
