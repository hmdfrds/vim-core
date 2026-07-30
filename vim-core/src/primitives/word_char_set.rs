//! Bitmap-based word character set for iskeyword.
//!
//! Precomputes a lookup structure from Vim's `iskeyword` option format
//! (e.g., `@,48-57,_,192-255`) so that word boundary classification is O(1)
//! for ASCII characters and fast for Unicode.

use std::collections::HashSet;

/// A precomputed character set for word boundary classification.
///
/// Built from Vim's `iskeyword` option format. ASCII characters use a
/// 128-bit bitmap for O(1) lookup. Non-ASCII extras use a HashSet
/// (rarely populated in practice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordCharSet {
    /// Bitmap for ASCII range 0-127. Bit N set means char N is a word char.
    ascii_bits: [u64; 2],
    /// Non-ASCII characters explicitly added to the word set.
    unicode_extra: HashSet<char>,
    /// Whether the `@` specifier is present (includes all alphabetic chars).
    has_alpha_class: bool,
}

impl WordCharSet {
    /// Build from Vim iskeyword format string.
    ///
    /// Format: comma-separated items:
    /// - `N`: single ASCII value
    /// - `N-M`: ASCII range
    /// - `@`: all alphabetic characters (Unicode-aware)
    /// - `_`: literal underscore (shorthand)
    /// - `^X`: negate — remove X from the set (applied after all inclusions)
    #[must_use]
    pub fn from_iskeyword(spec: &str) -> Self {
        let mut set = Self {
            ascii_bits: [0; 2],
            unicode_extra: HashSet::new(),
            has_alpha_class: false,
        };

        // Collect exclusions to apply after all inclusions.
        let mut exclusions: Vec<&str> = Vec::new();

        for item in spec.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some(negated) = item.strip_prefix('^') {
                exclusions.push(negated);
                continue;
            }
            if item == "@" {
                set.has_alpha_class = true;
                // Also set bitmap bits for all ASCII alphabetic chars
                for byte in b'A'..=b'Z' {
                    set.add_ascii(byte);
                }
                for byte in b'a'..=b'z' {
                    set.add_ascii(byte);
                }
                continue;
            }
            if let Some(range) = item.split_once('-') {
                if let (Ok(lo), Ok(hi)) = (range.0.parse::<u32>(), range.1.parse::<u32>()) {
                    for cp in lo..=hi {
                        set.add_codepoint(cp);
                    }
                }
            } else if item == "_" {
                set.add_ascii(b'_');
            } else if let Ok(cp) = item.parse::<u32>() {
                set.add_codepoint(cp);
            }
        }

        // Apply exclusions: ^X removes X from the set.
        for negated in exclusions {
            if negated == "@" {
                set.has_alpha_class = false;
                for byte in b'A'..=b'Z' {
                    set.remove_ascii(byte);
                }
                for byte in b'a'..=b'z' {
                    set.remove_ascii(byte);
                }
            } else if let Some(range) = negated.split_once('-') {
                if let (Ok(lo), Ok(hi)) = (range.0.parse::<u32>(), range.1.parse::<u32>()) {
                    for cp in lo..=hi {
                        set.remove_codepoint(cp);
                    }
                }
            } else if negated == "_" {
                set.remove_ascii(b'_');
            } else if let Ok(cp) = negated.parse::<u32>() {
                set.remove_codepoint(cp);
            }
        }

        set
    }

    /// The Vim/Neovim default: `@,48-57,_,192-255`
    ///
    /// This produces identical behavior to the previous hardcoded
    /// `c.is_alphanumeric() || c == '_'` for the ASCII range.
    #[must_use]
    pub fn default_vim() -> Self {
        Self::from_iskeyword("@,48-57,_,192-255")
    }

    /// Test whether a character is in the word set.
    #[inline]
    #[must_use]
    pub fn contains(&self, c: char) -> bool {
        // `u8::try_from` succeeds exactly for the Latin-1 range; the extra
        // `is_ascii` guard keeps the bitmap index inside `ascii_bits`, and
        // everything else falls through to the Unicode path.
        match u8::try_from(c) {
            Ok(byte) if c.is_ascii() => self.contains_ascii(byte),
            _ => (self.has_alpha_class && c.is_alphabetic()) || self.unicode_extra.contains(&c),
        }
    }

    /// O(1) ASCII bitmap test.
    #[inline]
    #[must_use]
    pub const fn contains_ascii(&self, byte: u8) -> bool {
        let idx = (byte >> 6) as usize;
        let bit = byte & 63;
        self.ascii_bits[idx] & (1u64 << bit) != 0
    }

    const fn add_ascii(&mut self, byte: u8) {
        let idx = (byte >> 6) as usize;
        let bit = byte & 63;
        self.ascii_bits[idx] |= 1u64 << bit;
    }

    fn add_codepoint(&mut self, cp: u32) {
        match u8::try_from(cp) {
            Ok(byte) if byte < 128 => self.add_ascii(byte),
            _ => {
                if let Some(c) = char::from_u32(cp) {
                    self.unicode_extra.insert(c);
                }
            }
        }
    }

    const fn remove_ascii(&mut self, byte: u8) {
        let idx = (byte >> 6) as usize;
        let bit = byte & 63;
        self.ascii_bits[idx] &= !(1u64 << bit);
    }

    fn remove_codepoint(&mut self, cp: u32) {
        match u8::try_from(cp) {
            Ok(byte) if byte < 128 => self.remove_ascii(byte),
            _ => {
                if let Some(c) = char::from_u32(cp) {
                    self.unicode_extra.remove(&c);
                }
            }
        }
    }
}

impl Default for WordCharSet {
    fn default() -> Self {
        Self::default_vim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_includes_alpha_digits_underscore() {
        let set = WordCharSet::default_vim();
        assert!(set.contains('a'));
        assert!(set.contains('Z'));
        assert!(set.contains('0'));
        assert!(set.contains('9'));
        assert!(set.contains('_'));
        assert!(!set.contains(' '));
        assert!(!set.contains('('));
        assert!(!set.contains('-'));
    }

    #[test]
    fn custom_iskeyword_with_dash() {
        let set = WordCharSet::from_iskeyword("@,48-57,_,45");
        assert!(set.contains('-')); // ASCII 45
        assert!(set.contains('a'));
        assert!(set.contains('_'));
    }

    #[test]
    fn unicode_range() {
        let set = WordCharSet::from_iskeyword("@,48-57,_,192-255");
        assert!(set.contains('\u{00C0}')); // A-grave (192)
        assert!(set.contains('\u{00FF}')); // y-diaeresis (255)
    }

    #[test]
    fn default_matches_hardcoded_for_ascii() {
        // The default WordCharSet must produce the same result as
        // `c.is_alphanumeric() || c == '_'` for all ASCII characters.
        let set = WordCharSet::default_vim();
        for byte in 0u8..128 {
            let c = byte as char;
            let hardcoded = c.is_alphanumeric() || c == '_';
            assert_eq!(
                set.contains(c),
                hardcoded,
                "mismatch for {:?} (byte {})",
                c,
                byte
            );
        }
    }

    #[test]
    fn empty_iskeyword() {
        let set = WordCharSet::from_iskeyword("");
        assert!(!set.contains('a'));
        assert!(!set.contains('0'));
        assert!(!set.contains('_'));
        assert!(!set.contains(' '));
    }

    #[test]
    fn at_sign_only_matches_alphabetic() {
        let set = WordCharSet::from_iskeyword("@");
        assert!(set.contains('a'));
        assert!(set.contains('Z'));
        assert!(!set.contains('0')); // digits are NOT alphabetic
        assert!(!set.contains('_'));
        assert!(!set.contains(' '));
    }

    #[test]
    fn custom_with_dollar_sign() {
        // Lisp-like: $ is a word char
        let set = WordCharSet::from_iskeyword("@,48-57,_,36");
        assert!(set.contains('$')); // ASCII 36
        assert!(set.contains('a'));
        assert!(set.contains('_'));
        assert!(!set.contains('-'));
    }

    // ── iskeyword `^` negate prefix ──

    #[test]
    fn negate_removes_single_char() {
        // @,48-57,_,36 includes $, then ^36 removes it
        let set = WordCharSet::from_iskeyword("@,48-57,_,36,^36");
        assert!(!set.contains('$')); // removed by ^36
        assert!(set.contains('a')); // still included
        assert!(set.contains('0')); // still included
        assert!(set.contains('_')); // still included
    }

    #[test]
    fn negate_dollar_sign_by_value() {
        // @,48-57,_ includes alpha+digits+underscore, ^$ (^36) removes dollar
        let set = WordCharSet::from_iskeyword("@,48-57,_,^36");
        assert!(!set.contains('$'));
        assert!(set.contains('a'));
        assert!(set.contains('0'));
        assert!(set.contains('_'));
    }

    #[test]
    fn negate_underscore() {
        let set = WordCharSet::from_iskeyword("@,48-57,_,^_");
        assert!(!set.contains('_'));
        assert!(set.contains('a'));
        assert!(set.contains('0'));
    }

    #[test]
    fn negate_range() {
        // Include 48-57 (digits), then remove 50-53 (chars '2'-'5')
        let set = WordCharSet::from_iskeyword("@,48-57,_,^50-53");
        assert!(set.contains('0')); // 48
        assert!(set.contains('1')); // 49
        assert!(!set.contains('2')); // 50 — removed
        assert!(!set.contains('3')); // 51 — removed
        assert!(!set.contains('4')); // 52 — removed
        assert!(!set.contains('5')); // 53 — removed
        assert!(set.contains('6')); // 54
        assert!(set.contains('9')); // 57
    }

    #[test]
    fn negate_alpha_class() {
        let set = WordCharSet::from_iskeyword("@,48-57,_,^@");
        assert!(!set.contains('a'));
        assert!(!set.contains('Z'));
        assert!(set.contains('0'));
        assert!(set.contains('_'));
    }

    #[test]
    fn negate_applied_after_inclusions() {
        // Order shouldn't matter: ^36 before 36 should still negate
        // because exclusions are collected and applied after all inclusions
        let set = WordCharSet::from_iskeyword("@,48-57,_,^36,36");
        assert!(!set.contains('$')); // ^36 applied after 36 inclusion
    }
}
