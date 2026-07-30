//! Langmap table — character remapping for non-Latin keyboard layouts.
//!
//! Implements Vim's `:set langmap=...` syntax: a mapping table that remaps
//! characters in Normal/Visual/Operator-pending modes so users with non-Latin
//! keyboards can type commands without switching layouts.
//!
//! # Formats
//!
//! Langmap supports two formats, separated by commas:
//!
//! - **Pair format**: alternating from/to characters, e.g. `"aAbBcC"` maps
//!   a→A, b→B, c→C. Must have even length.
//! - **Semicolon format**: `"from;to"` where from-chars and to-chars are equal
//!   length, e.g. `"abc;ABC"` maps a→A, b→B, c→C.
//!
//! Special characters (`;`, `,`, `\`) are escaped with backslash.
//!
//! # Architecture
//!
//! This module has zero engine dependencies — it's a pure data structure that
//! lives in the `keymap` layer. Fast path: ASCII codepoints < 256 use a flat
//! array; multibyte characters fall back to hash maps.

use ahash::AHashMap;

use super::{Key, KeyEvent, Modifiers};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when parsing a langmap string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LangmapError {
    /// Pair format segment has an odd number of characters after unescaping.
    ///
    /// The contained string is the problematic segment.
    OddPairLength(String),

    /// Semicolon format segment has unequal from/to list lengths.
    ///
    /// The contained string is the problematic segment.
    UnequalSemicolonLists(String),
}

impl std::fmt::Display for LangmapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OddPairLength(segment) => {
                write!(
                    f,
                    "langmap pair format has odd character count: {segment:?}"
                )
            }
            Self::UnequalSemicolonLists(segment) => {
                write!(
                    f,
                    "langmap semicolon format has unequal from/to lists: {segment:?}",
                )
            }
        }
    }
}

impl std::error::Error for LangmapError {}

// ---------------------------------------------------------------------------
// LangmapTable
// ---------------------------------------------------------------------------

/// Identity-initialized `[u8; 256]` array where `arr[i] == i` for all i.
fn identity_256() -> [u8; 256] {
    let mut arr = [0u8; 256];
    for (slot, byte) in arr.iter_mut().zip(0..=u8::MAX) {
        *slot = byte;
    }
    arr
}

/// A character remapping table parsed from Vim's `:set langmap=...` syntax.
///
/// Two lookup tiers for maximum performance:
/// - **ASCII fast path**: flat `[u8; 256]` arrays for codepoints 0..=255.
/// - **Multibyte fallback**: `AHashMap<char, char>` for everything else.
///
/// Both forward (remap) and reverse (unmap) directions are maintained.
#[derive(Debug, Clone)]
pub struct LangmapTable {
    /// Forward mapping: `ascii[from_byte] = to_byte`. Identity-initialized.
    ascii: [u8; 256],
    /// Reverse mapping: `ascii_reverse[to_byte] = from_byte`. Identity-initialized.
    ascii_reverse: [u8; 256],
    /// Forward mapping for characters with codepoint >= 256, or cross-width
    /// pairs where from < 256 but to >= 256 (can't fit in the u8 array).
    multibyte_forward: AHashMap<char, char>,
    /// Reverse mapping for multibyte characters.
    multibyte_reverse: AHashMap<char, char>,
    /// Number of non-identity mappings (used by `is_empty()`).
    non_identity_count: usize,
}

impl Default for LangmapTable {
    fn default() -> Self {
        Self::new()
    }
}

impl LangmapTable {
    /// Create an empty langmap table (all identity mappings).
    #[must_use]
    pub fn new() -> Self {
        Self {
            ascii: identity_256(),
            ascii_reverse: identity_256(),
            multibyte_forward: AHashMap::new(),
            multibyte_reverse: AHashMap::new(),
            non_identity_count: 0,
        }
    }

    /// Parse a Vim langmap string into a `LangmapTable`.
    ///
    /// The input string follows Vim's `:set langmap=...` syntax:
    /// - Segments separated by unescaped commas.
    /// - Each segment is either pair format or semicolon format.
    /// - Backslash escapes: `\;` → `;`, `\,` → `,`, `\\` → `\`.
    ///
    /// # Errors
    ///
    /// Returns [`LangmapError::OddPairLength`] if a pair-format segment has
    /// an odd number of characters, or [`LangmapError::UnequalSemicolonLists`]
    /// if a semicolon-format segment has mismatched from/to lengths.
    pub fn parse(s: &str) -> Result<Self, LangmapError> {
        let mut table = Self::new();

        if s.is_empty() {
            return Ok(table);
        }

        let segments = split_on_unescaped_commas(s);

        for segment in &segments {
            if segment.is_empty() {
                continue;
            }

            // Check for unescaped semicolon to determine format.
            if let Some(semi_pos) = find_unescaped_semicolon(segment) {
                // Semicolon format: "from_chars;to_chars"
                let from_raw = segment.get(..semi_pos).unwrap_or_default();
                let to_raw = segment.get(semi_pos + 1..).unwrap_or_default();
                let from_chars = unescape_chars(from_raw);
                let to_chars = unescape_chars(to_raw);

                if from_chars.len() != to_chars.len() {
                    return Err(LangmapError::UnequalSemicolonLists((*segment).to_owned()));
                }

                for (from, to) in from_chars.into_iter().zip(to_chars) {
                    table.insert_mapping(from, to);
                }
            } else {
                // Pair format: alternating from/to characters.
                let chars = unescape_chars(segment);

                if !chars.len().is_multiple_of(2) {
                    return Err(LangmapError::OddPairLength((*segment).to_owned()));
                }

                let mut i = 0;
                while i + 1 < chars.len() {
                    if let (Some(&from), Some(&to)) = (chars.get(i), chars.get(i + 1)) {
                        table.insert_mapping(from, to);
                    }
                    i += 2;
                }
            }
        }

        Ok(table)
    }

    /// Forward lookup: remap a character through the langmap.
    ///
    /// Returns `c` unchanged if no mapping exists.
    #[must_use]
    pub fn remap(&self, c: char) -> char {
        let cp = c as u32;
        if cp < 256 {
            // Check multibyte map first — handles ASCII→multibyte cross-width
            // mappings that can't be stored in the u8 ascii array.
            if let Some(&to) = self.multibyte_forward.get(&c) {
                return to;
            }
            // SAFETY of indexing: cp < 256 guarantees cp fits in [u8; 256].
            self.ascii
                .get(cp as usize)
                .copied()
                .map_or(c, |b| b as char)
        } else {
            self.multibyte_forward.get(&c).copied().unwrap_or(c)
        }
    }

    /// Reverse lookup: given a *target* character, find the *source* character.
    ///
    /// Returns `c` unchanged if no mapping exists.
    #[must_use]
    pub fn unmap(&self, c: char) -> char {
        let cp = c as u32;
        if cp < 256 {
            // Check multibyte map first — handles reverse of multibyte→ASCII
            // mappings stored in the multibyte reverse table.
            if let Some(&from) = self.multibyte_reverse.get(&c) {
                return from;
            }
            self.ascii_reverse
                .get(cp as usize)
                .copied()
                .map_or(c, |b| b as char)
        } else {
            self.multibyte_reverse.get(&c).copied().unwrap_or(c)
        }
    }

    /// Remap a [`KeyEvent`] through the langmap.
    ///
    /// Only remaps `Key::Char(c)` when no Ctrl, Alt, or Meta modifiers are
    /// present. Shift-only is allowed (uppercase letters are already folded
    /// into the character). Special keys (Escape, Backspace, arrows, function
    /// keys, etc.) pass through unchanged.
    #[must_use]
    pub fn remap_key_event(&self, key: KeyEvent) -> KeyEvent {
        // Only remap plain Char keys without Ctrl/Alt/Meta.
        let blocking_modifiers = Modifiers::CTRL | Modifiers::ALT | Modifiers::META;
        if key.modifiers().intersects(blocking_modifiers) {
            return key;
        }

        match key.key() {
            Key::Char(c) => {
                let remapped = self.remap(c);
                if remapped == c {
                    key
                } else {
                    KeyEvent::new(Key::Char(remapped), key.modifiers())
                }
            }
            // Special keys pass through unchanged.
            // drift: langmap only remaps printable Char keys; all named keys (arrows, F-keys, etc.) pass through unchanged
            _ => key,
        }
    }

    /// Returns `true` if this table has no non-identity mappings.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.non_identity_count == 0
    }

    /// Reset the table to all-identity (no mappings).
    pub fn clear(&mut self) {
        self.ascii = identity_256();
        self.ascii_reverse = identity_256();
        self.multibyte_forward.clear();
        self.multibyte_reverse.clear();
        self.non_identity_count = 0;
    }

    /// Iterate over all non-identity mappings as `(from, to)` pairs.
    ///
    /// ASCII mappings come first (in codepoint order), then multibyte mappings
    /// (in arbitrary hash-map order).
    pub fn entries(&self) -> impl Iterator<Item = (char, char)> + '_ {
        let ascii_iter = self
            .ascii
            .iter()
            .zip(0..=u8::MAX)
            .filter_map(|(&to, from)| (from != to).then_some((char::from(from), char::from(to))));

        let multibyte_iter = self.multibyte_forward.iter().map(|(&from, &to)| (from, to));

        ascii_iter.chain(multibyte_iter)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Insert a single (from → to) mapping, updating both forward and reverse
    /// lookup tables.
    fn insert_mapping(&mut self, from: char, to: char) {
        if from == to {
            return;
        }

        // `u8::try_from(char)` succeeds exactly when the codepoint is < 256,
        // which is the condition for both ends fitting in the ASCII arrays.
        if let (Ok(from_byte), Ok(to_byte)) = (u8::try_from(from), u8::try_from(to)) {
            // Check if this slot was previously identity before overwriting.
            let was_identity = self
                .ascii
                .get(from_byte as usize)
                .copied()
                .is_some_and(|v| v == from_byte);

            if let Some(slot) = self.ascii.get_mut(from_byte as usize) {
                *slot = to_byte;
            }
            if let Some(slot) = self.ascii_reverse.get_mut(to_byte as usize) {
                *slot = from_byte;
            }

            if was_identity {
                self.non_identity_count += 1;
            }
        } else {
            // At least one of from/to is multibyte — use hash maps.
            // When from < 256 but to >= 256, the ascii array can't hold the
            // target, so we store in multibyte maps and remap() checks them
            // first for ASCII codepoints.
            let was_absent = self.multibyte_forward.insert(from, to).is_none();
            self.multibyte_reverse.insert(to, from);

            if was_absent {
                self.non_identity_count += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Split a langmap string on unescaped commas.
///
/// A comma is "escaped" if preceded by a backslash.
/// We iterate character-by-character, tracking escape state.
fn split_on_unescaped_commas(s: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut escaped = false;

    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == ',' {
            segments.push(s.get(start..i).unwrap_or_default());
            start = i + 1; // ',' is always 1 byte in UTF-8
        }
    }

    // Push the final segment.
    segments.push(s.get(start..).unwrap_or_default());

    segments
}

/// Find the byte position of the first unescaped semicolon in a segment.
fn find_unescaped_semicolon(s: &str) -> Option<usize> {
    let mut escaped = false;

    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == ';' {
            return Some(i);
        }
    }

    None
}

/// Unescape a langmap segment: `\;` → `;`, `\,` → `,`, `\\` → `\`.
///
/// Returns the unescaped characters as a `Vec<char>`.
fn unescape_chars(s: &str) -> Vec<char> {
    let mut chars = Vec::new();
    let mut escaped = false;

    for c in s.chars() {
        if escaped {
            // The character after a backslash is taken literally.
            chars.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        chars.push(c);
    }

    // A trailing backslash with nothing after it — treat as literal backslash.
    if escaped {
        chars.push('\\');
    }

    chars
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // === Test 1: Empty langmap is identity ===

    #[test]
    fn langmap_empty_is_identity() {
        let table = LangmapTable::new();
        for c in 'a'..='z' {
            assert_eq!(table.remap(c), c, "remap should be identity for '{c}'");
            assert_eq!(table.unmap(c), c, "unmap should be identity for '{c}'");
        }
        for c in 'A'..='Z' {
            assert_eq!(table.remap(c), c);
            assert_eq!(table.unmap(c), c);
        }
        // Multibyte chars too.
        assert_eq!(table.remap('й'), 'й');
        assert_eq!(table.unmap('й'), 'й');
    }

    // === Test 2: Pair format basic ===

    #[test]
    fn langmap_pair_format_basic() {
        let table = LangmapTable::parse("aAbBcC").unwrap();
        assert_eq!(table.remap('a'), 'A');
        assert_eq!(table.remap('b'), 'B');
        assert_eq!(table.remap('c'), 'C');
        // Unmapped chars are identity.
        assert_eq!(table.remap('d'), 'd');
        // Reverse works too.
        assert_eq!(table.unmap('A'), 'a');
        assert_eq!(table.unmap('B'), 'b');
        assert_eq!(table.unmap('C'), 'c');
    }

    // === Test 3: Semicolon format basic ===

    #[test]
    fn langmap_semicolon_format_basic() {
        let table = LangmapTable::parse("abc;ABC").unwrap();
        assert_eq!(table.remap('a'), 'A');
        assert_eq!(table.remap('b'), 'B');
        assert_eq!(table.remap('c'), 'C');
        assert_eq!(table.unmap('A'), 'a');
        assert_eq!(table.unmap('B'), 'b');
        assert_eq!(table.unmap('C'), 'c');
    }

    // === Test 4: Mixed format with commas ===

    #[test]
    fn langmap_mixed_format_with_commas() {
        let table = LangmapTable::parse("aA,bcd;BCD,eEfF").unwrap();
        // Pair: a→A
        assert_eq!(table.remap('a'), 'A');
        // Semicolon: b→B, c→C, d→D
        assert_eq!(table.remap('b'), 'B');
        assert_eq!(table.remap('c'), 'C');
        assert_eq!(table.remap('d'), 'D');
        // Pair: e→E, f→F
        assert_eq!(table.remap('e'), 'E');
        assert_eq!(table.remap('f'), 'F');
    }

    // === Test 5: Cyrillic to QWERTY ===

    #[test]
    fn langmap_cyrillic_to_qwerty() {
        let table = LangmapTable::parse("йцукен;qwerty").unwrap();
        assert_eq!(table.remap('й'), 'q');
        assert_eq!(table.remap('ц'), 'w');
        assert_eq!(table.remap('у'), 'e');
        assert_eq!(table.remap('к'), 'r');
        assert_eq!(table.remap('е'), 't');
        assert_eq!(table.remap('н'), 'y');
        // Reverse.
        assert_eq!(table.unmap('q'), 'й');
        assert_eq!(table.unmap('w'), 'ц');
        assert_eq!(table.unmap('y'), 'н');
    }

    // === Test 6: Escaped semicolon ===

    #[test]
    fn langmap_escaped_semicolon() {
        // "\;a" means ; → a (semicolon is the from-char, a is the to-char).
        let table = LangmapTable::parse("\\;a").unwrap();
        assert_eq!(table.remap(';'), 'a');
        assert_eq!(table.unmap('a'), ';');
    }

    // === Test 7: Escaped comma ===

    #[test]
    fn langmap_escaped_comma() {
        // "\,a" means , → a.
        let table = LangmapTable::parse("\\,a").unwrap();
        assert_eq!(table.remap(','), 'a');
        assert_eq!(table.unmap('a'), ',');
    }

    // === Test 8: Escaped backslash ===

    #[test]
    fn langmap_escaped_backslash() {
        // "\\\\a" in a Rust string literal is the raw string `\\a`.
        // That's: backslash-backslash-a, which unescapes to: \, a → pair format
        // mapping \ → a.
        let table = LangmapTable::parse("\\\\a").unwrap();
        assert_eq!(table.remap('\\'), 'a');
        assert_eq!(table.unmap('a'), '\\');
    }

    // === Test 9: Pair format odd length is error ===

    #[test]
    fn langmap_pair_format_odd_length_error() {
        let result = LangmapTable::parse("abc");
        assert!(result.is_err());
        match result.unwrap_err() {
            LangmapError::OddPairLength(seg) => assert_eq!(seg, "abc"),
            other => panic!("expected OddPairLength, got: {other:?}"),
        }
    }

    // === Test 10: Semicolon format unequal lists is error ===

    #[test]
    fn langmap_semicolon_format_unequal_error() {
        let result = LangmapTable::parse("abc;AB");
        assert!(result.is_err());
        match result.unwrap_err() {
            LangmapError::UnequalSemicolonLists(seg) => assert_eq!(seg, "abc;AB"),
            other => panic!("expected UnequalSemicolonLists, got: {other:?}"),
        }
    }

    // === Test 11: remap_key_event remaps plain char ===

    #[test]
    fn langmap_remap_key_event_plain_char() {
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::char('a');
        let output = table.remap_key_event(input);
        assert_eq!(output.key(), Key::Char('A'));
        assert_eq!(output.modifiers(), Modifiers::NONE);
    }

    // === Test 12: remap_key_event skips Ctrl-modified keys ===

    #[test]
    fn langmap_remap_key_event_skips_ctrl() {
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::ctrl('a');
        let output = table.remap_key_event(input);
        assert_eq!(
            output.key(),
            Key::Char('a'),
            "Ctrl-a should NOT be remapped"
        );
        assert_eq!(output.modifiers(), Modifiers::CTRL);
    }

    // === Test 13: remap_key_event skips special keys (Escape) ===

    #[test]
    fn langmap_remap_key_event_skips_special_keys() {
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::escape();
        let output = table.remap_key_event(input);
        assert_eq!(output.key(), Key::Escape);
    }

    // === Test 14: clear resets to identity ===

    #[test]
    fn langmap_clear_resets_to_identity() {
        let mut table = LangmapTable::parse("aAbBcC").unwrap();
        assert!(!table.is_empty());
        assert_eq!(table.remap('a'), 'A');

        table.clear();

        assert!(table.is_empty());
        assert_eq!(table.remap('a'), 'a');
        assert_eq!(table.remap('b'), 'b');
        assert_eq!(table.remap('c'), 'c');
    }

    // === Test 15: entries iterates non-identity mappings ===

    #[test]
    fn langmap_entries_iterates_non_identity() {
        let table = LangmapTable::parse("aAbB").unwrap();
        let mut entries: Vec<(char, char)> = table.entries().collect();
        entries.sort();
        assert_eq!(entries, vec![('a', 'A'), ('b', 'B')]);
    }

    // === Test 16: Multibyte forward and reverse (Hebrew aleph → 'a') ===

    #[test]
    fn langmap_multibyte_forward_and_reverse() {
        // Hebrew Aleph (U+05D0) → 'a'
        let table = LangmapTable::parse("\u{05D0}a").unwrap();
        assert_eq!(table.remap('\u{05D0}'), 'a');
        assert_eq!(table.unmap('a'), '\u{05D0}');
    }

    // === Test 17: is_empty returns true for new(), false after parse ===

    #[test]
    fn langmap_is_empty_semantics() {
        let empty = LangmapTable::new();
        assert!(empty.is_empty());

        let nonempty = LangmapTable::parse("aA").unwrap();
        assert!(!nonempty.is_empty());
    }

    // === Additional edge case tests ===

    #[test]
    fn langmap_empty_string_parses_to_identity() {
        let table = LangmapTable::parse("").unwrap();
        assert!(table.is_empty());
        assert_eq!(table.remap('x'), 'x');
    }

    #[test]
    fn langmap_remap_key_event_allows_shift_only() {
        // Shift-only modifier should still allow remapping.
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::shift('a');
        let output = table.remap_key_event(input);
        assert_eq!(output.key(), Key::Char('A'), "Shift+'a' should be remapped");
        assert_eq!(output.modifiers(), Modifiers::SHIFT);
    }

    #[test]
    fn langmap_remap_key_event_skips_alt() {
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::alt('a');
        let output = table.remap_key_event(input);
        assert_eq!(output.key(), Key::Char('a'), "Alt-a should NOT be remapped");
    }

    #[test]
    fn langmap_remap_key_event_skips_meta() {
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::new(Key::Char('a'), Modifiers::META);
        let output = table.remap_key_event(input);
        assert_eq!(
            output.key(),
            Key::Char('a'),
            "Meta-a should NOT be remapped"
        );
    }

    #[test]
    fn langmap_remap_key_event_passes_through_enter() {
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::enter();
        let output = table.remap_key_event(input);
        assert_eq!(output, input);
    }

    #[test]
    fn langmap_remap_key_event_passes_through_arrows() {
        let table = LangmapTable::parse("aA").unwrap();
        for input in [
            KeyEvent::up(),
            KeyEvent::down(),
            KeyEvent::left(),
            KeyEvent::right(),
        ] {
            let output = table.remap_key_event(input);
            assert_eq!(output, input);
        }
    }

    #[test]
    fn langmap_remap_key_event_passes_through_function_keys() {
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::f(5);
        let output = table.remap_key_event(input);
        assert_eq!(output, input);
    }

    #[test]
    fn langmap_entries_includes_multibyte() {
        let table = LangmapTable::parse("йq").unwrap();
        let entries: Vec<(char, char)> = table.entries().collect();
        assert_eq!(entries, vec![('й', 'q')]);
    }

    #[test]
    fn langmap_semicolon_format_with_cyrillic_entries() {
        // Full Cyrillic row mapped via semicolon format.
        let table = LangmapTable::parse("йцукенгшщзхъ;qwertyuiop[]").unwrap();
        assert_eq!(table.remap('й'), 'q');
        assert_eq!(table.remap('щ'), 'o');
        assert_eq!(table.remap('х'), '[');
        assert_eq!(table.remap('ъ'), ']');
    }

    #[test]
    fn langmap_default_impl() {
        // Default should be same as new().
        let table = LangmapTable::default();
        assert!(table.is_empty());
        assert_eq!(table.remap('x'), 'x');
    }

    #[test]
    fn langmap_display_error_messages() {
        let err = LangmapError::OddPairLength("abc".into());
        let msg = format!("{err}");
        assert!(msg.contains("odd character count"));

        let err = LangmapError::UnequalSemicolonLists("abc;AB".into());
        let msg = format!("{err}");
        assert!(msg.contains("unequal from/to lists"));
    }

    #[test]
    fn langmap_remap_key_event_unmapped_char_unchanged() {
        let table = LangmapTable::parse("aA").unwrap();
        let input = KeyEvent::char('z');
        let output = table.remap_key_event(input);
        assert_eq!(output, input);
    }

    #[test]
    fn langmap_multiple_commas_with_empty_segments() {
        // "aA,,bB" has an empty segment in the middle — should be fine.
        let table = LangmapTable::parse("aA,,bB").unwrap();
        assert_eq!(table.remap('a'), 'A');
        assert_eq!(table.remap('b'), 'B');
    }
}
