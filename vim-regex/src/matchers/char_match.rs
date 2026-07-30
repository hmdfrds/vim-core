use super::context::MatchContext;
use crate::ir::{CharClass, CollectionItem, PosixClassName};

// ═══════════════════════════════════════════════════════════════════════════════
// CHARACTER MATCHER
// ═══════════════════════════════════════════════════════════════════════════════

/// A matcher that consumes one or more bytes of input on success.
///
/// Each variant matches a single character (possibly multi-byte) and
/// returns the number of bytes consumed, or `None` on mismatch.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CharMatcher {
    /// Match a literal character (case sensitivity determined at match time).
    Literal(char),
    /// `.` — match any character except `\n`.
    AnyChar,
    /// `\_.` — match any character including `\n`.
    AnyCharNl,
    /// Match a `[...]` collection.
    Collection {
        /// Whether the collection is negated (`[^...]`).
        negated: bool,
        /// The items in the collection.
        items: Vec<CollectionItem>,
        /// Whether `\n` is also matched.
        include_newline: bool,
    },
}

impl CharMatcher {
    /// Attempt to match at `pos` in `text`.
    ///
    /// Returns `Some(bytes_consumed)` on success, `None` on failure.
    #[inline]
    pub(crate) fn matches(&self, text: &str, pos: usize, ctx: &MatchContext<'_>) -> Option<usize> {
        // ASCII fast path: avoid UTF-8 decode for the common case.
        if let Some(&byte) = text.as_bytes().get(pos) {
            if byte < 0x80 {
                return self.matches_ascii(byte as char, ctx);
            }
        }

        // Multi-byte / edge-of-string path.
        let remaining = text.get(pos..)?;
        let ch = remaining.chars().next()?;

        match self {
            Self::Literal(c) => Self::match_literal(*c, ch, ctx),
            Self::AnyChar => Self::match_any_char(ch),
            Self::AnyCharNl => Some(ch.len_utf8()),
            Self::Collection {
                negated,
                items,
                include_newline,
            } => Self::match_collection(*negated, items, *include_newline, ch, ctx.case_sensitive),
        }
    }

    /// ASCII-only match. `ch` is guaranteed to be < 0x80.
    /// Returns `Some(1)` on match (ASCII is always 1 byte), `None` on no match.
    #[inline]
    fn matches_ascii(&self, ch: char, ctx: &MatchContext<'_>) -> Option<usize> {
        match self {
            Self::Literal(c) => {
                if Self::literal_matches_ascii(*c, ch, ctx.case_sensitive) {
                    Some(1)
                } else {
                    None
                }
            }
            Self::AnyChar => {
                if ch != '\n' {
                    Some(1)
                } else {
                    None
                }
            }
            Self::AnyCharNl => Some(1),
            Self::Collection {
                negated,
                items,
                include_newline,
            } => Self::match_collection(*negated, items, *include_newline, ch, ctx.case_sensitive),
        }
    }

    /// Fast literal comparison for ASCII chars.
    #[inline]
    #[allow(
        clippy::manual_ignore_case_cmp,
        reason = "manual comparison needed for ASCII fast path"
    )]
    const fn literal_matches_ascii(pattern: char, input: char, case_sensitive: bool) -> bool {
        if case_sensitive {
            pattern as u32 == input as u32
        } else {
            // ASCII case-insensitive: compare lowercase forms.
            let p = if pattern.is_ascii_uppercase() {
                (pattern as u8 + 32) as u32
            } else {
                pattern as u32
            };
            let i = if input.is_ascii_uppercase() {
                (input as u8 + 32) as u32
            } else {
                input as u32
            };
            p == i
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHARACTER MATCHER — MATCH HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl CharMatcher {
    /// Match a literal character, respecting case sensitivity from the context.
    ///
    /// Uses ASCII-only case folding (`to_ascii_lowercase`) by design, matching
    /// Vim's built-in case-insensitive behavior.
    #[allow(
        clippy::manual_ignore_case_cmp,
        reason = "manual comparison needed for const fn"
    )]
    const fn match_literal(
        pattern_char: char,
        input_char: char,
        ctx: &MatchContext<'_>,
    ) -> Option<usize> {
        let matched = if ctx.case_sensitive {
            input_char == pattern_char
        } else {
            input_char.to_ascii_lowercase() == pattern_char.to_ascii_lowercase()
        };
        if matched {
            Some(input_char.len_utf8())
        } else {
            None
        }
    }

    /// Match any character except newline.
    const fn match_any_char(ch: char) -> Option<usize> {
        if ch == '\n' {
            None
        } else {
            Some(ch.len_utf8())
        }
    }

    /// Match a collection (`[...]` or `[^...]`).
    ///
    /// Newline handling:
    /// - If `include_newline` is true (from `\_[...]`), newline is unconditionally
    ///   matched (returns `Some(1)`) -- it is an implicit member of the set.
    /// - If the items contain an explicit `CollectionItem::Newline` (from `[\n]`),
    ///   newline is allowed through and matched by that item via the normal
    ///   in-set / negation logic.
    /// - Otherwise, newline is rejected outright (Vim's default behavior).
    fn match_collection(
        negated: bool,
        items: &[CollectionItem],
        include_newline: bool,
        ch: char,
        case_sensitive: bool,
    ) -> Option<usize> {
        if ch == '\n' {
            // `\_[...]` -- newline is always accepted regardless of items.
            if include_newline {
                return Some(1);
            }
            // Explicit `[\n]` in items -- allow through to normal matching.
            let has_explicit_newline = items
                .iter()
                .any(|item| matches!(item, CollectionItem::Newline));
            if !has_explicit_newline {
                return None;
            }
        }

        let in_set = items
            .iter()
            .any(|item| collection_item_matches(item, ch, case_sensitive));
        let matched = if negated { !in_set } else { in_set };
        if matched {
            Some(ch.len_utf8())
        } else {
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHARACTER CLASS — ASCII-ONLY MATCHING
// ═══════════════════════════════════════════════════════════════════════════════

/// Tests whether `ch` belongs to the given ASCII character class.
///
/// All matching is ASCII-only per Vim semantics. Non-ASCII characters
/// never match positive classes and always match negative classes.
#[must_use]
pub(crate) fn class_matches(class: CharClass, ch: char) -> bool {
    match class {
        CharClass::Digit => ch.is_ascii_digit(),
        CharClass::NotDigit => !ch.is_ascii_digit(),
        CharClass::Word => is_word_char(ch),
        CharClass::NotWord => !is_word_char(ch),
        CharClass::Whitespace => matches!(ch, ' ' | '\t'),
        CharClass::NotWhitespace => !matches!(ch, ' ' | '\t'),
        CharClass::Alpha => ch.is_ascii_alphabetic(),
        CharClass::NotAlpha => !ch.is_ascii_alphabetic(),
        CharClass::Lower => ch.is_ascii_lowercase(),
        CharClass::NotLower => !ch.is_ascii_lowercase(),
        CharClass::Upper => ch.is_ascii_uppercase(),
        CharClass::NotUpper => !ch.is_ascii_uppercase(),
        CharClass::Hex => ch.is_ascii_hexdigit(),
        CharClass::NotHex => !ch.is_ascii_hexdigit(),
        CharClass::Head => is_head_char(ch),
        CharClass::NotHead => !is_head_char(ch),
        CharClass::FileName => is_filename_char(ch),
        CharClass::Keyword => is_word_char(ch),
        CharClass::KeywordNoDigit => is_word_char(ch) && !ch.is_ascii_digit(),
        CharClass::Ident => ch.is_ascii_alphanumeric() || ch == '_',
        CharClass::SIdent => ch.is_ascii_alphabetic() || ch == '_',
        CharClass::Print => !ch.is_ascii_control() && ch != '\x7f',
        CharClass::SPrint => !ch.is_ascii_control() && ch != '\x7f' && !ch.is_ascii_digit(),
        CharClass::Octal => matches!(ch, '0'..='7'),
        CharClass::NOctal => !matches!(ch, '0'..='7'),
        CharClass::FileNameNoDigit => is_filename_char(ch) && !ch.is_ascii_digit(),
        // Composing marks are non-ASCII combining characters.
        // `class_matches` is `const`, so we inline the range check.
        CharClass::Composing => {
            !ch.is_ascii() && {
                let cp = ch as u32;
                matches!(cp, 0x0300..=0x036F | 0x0483..=0x0489
                | 0x0591..=0x05C7 | 0x0610..=0x061A | 0x064B..=0x065F
                | 0x0670 | 0x06D6..=0x06ED | 0x0711 | 0x0730..=0x074A
                | 0x07A6..=0x07B0 | 0x07EB..=0x07F3 | 0x0816..=0x082D
                | 0x0859..=0x085B | 0x0900..=0x0903 | 0x093A..=0x0963
                | 0x0981..=0x09CD | 0x09D7 | 0x09E2..=0x09E3
                | 0x0A01..=0x0A75 | 0x0A81..=0x0AFF | 0x0B01..=0x0B63
                | 0x0B82 | 0x0BBE..=0x0BCD | 0x0BD7 | 0x0C00..=0x0C63
                | 0x0C81..=0x0CE3 | 0x0D00..=0x0D63 | 0x0D81..=0x0DFF
                | 0x0E31 | 0x0E34..=0x0E4E | 0x0EB1..=0x0ECD
                | 0x0F18..=0x0FC6 | 0x102B..=0x108D | 0x108F
                | 0x109A..=0x109D | 0x1712..=0x1773 | 0x17B4..=0x17DD
                | 0x180B..=0x180D | 0x18A9 | 0x1920..=0x193B
                | 0x1A17..=0x1A7F | 0x1AB0..=0x1ABE | 0x1B00..=0x1B73
                | 0x1B80..=0x1BF3 | 0x1C24..=0x1CF9 | 0x1DC0..=0x1DFF
                | 0x20D0..=0x20F0 | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F)
            }
        }
    }
}

/// Const-compatible ASCII-only class matching for compile-time table computation.
#[must_use]
pub(crate) const fn class_matches_ascii(class: CharClass, ch: char) -> bool {
    match class {
        CharClass::Digit => ch.is_ascii_digit(),
        CharClass::NotDigit => !ch.is_ascii_digit(),
        CharClass::Word => is_word_char_ascii(ch),
        CharClass::NotWord => !is_word_char_ascii(ch),
        CharClass::Whitespace => matches!(ch, ' ' | '\t'),
        CharClass::NotWhitespace => !matches!(ch, ' ' | '\t'),
        CharClass::Alpha => ch.is_ascii_alphabetic(),
        CharClass::NotAlpha => !ch.is_ascii_alphabetic(),
        CharClass::Lower => ch.is_ascii_lowercase(),
        CharClass::NotLower => !ch.is_ascii_lowercase(),
        CharClass::Upper => ch.is_ascii_uppercase(),
        CharClass::NotUpper => !ch.is_ascii_uppercase(),
        CharClass::Hex => ch.is_ascii_hexdigit(),
        CharClass::NotHex => !ch.is_ascii_hexdigit(),
        CharClass::Head => is_head_char(ch),
        CharClass::NotHead => !is_head_char(ch),
        CharClass::FileName => is_filename_char(ch),
        CharClass::Keyword => is_word_char_ascii(ch),
        CharClass::KeywordNoDigit => is_word_char_ascii(ch) && !ch.is_ascii_digit(),
        CharClass::Ident => ch.is_ascii_alphanumeric() || ch == '_',
        CharClass::SIdent => ch.is_ascii_alphabetic() || ch == '_',
        CharClass::Print => !ch.is_ascii_control() && ch != '\x7f',
        CharClass::SPrint => !ch.is_ascii_control() && ch != '\x7f' && !ch.is_ascii_digit(),
        CharClass::Octal => matches!(ch, '0'..='7'),
        CharClass::NOctal => !matches!(ch, '0'..='7'),
        CharClass::FileNameNoDigit => is_filename_char(ch) && !ch.is_ascii_digit(),
        CharClass::Composing => false, // Composing marks are never ASCII
    }
}

/// `\w` — word character.
///
/// For ASCII, matches `[0-9A-Za-z_]`. For non-ASCII, matches any
/// Unicode alphanumeric character. This matches Vim's behavior
/// where non-ASCII letters (accented, Cyrillic, Greek, etc.) are
/// treated as word characters.
#[must_use]
pub(super) fn is_word_char(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_alphanumeric() || ch == '_'
    } else {
        ch.is_alphanumeric()
    }
}

/// Const-compatible ASCII-only word character check.
/// Used only in compile-time table computation.
#[must_use]
pub(super) const fn is_word_char_ascii(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// `\h` = `[A-Za-z_]`
#[must_use]
const fn is_head_char(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

/// `\f` = `[A-Za-z0-9._/~-]`
#[must_use]
const fn is_filename_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '/' | '~' | '-')
}

/// Tests whether a single collection item matches `ch`.
///
/// When `case_sensitive` is false, performs ASCII case-folding so that
/// `[A-Z]` matches lowercase letters and `[a]` matches `A`, matching
/// Vim's `\c` behavior inside collections.
#[must_use]
#[allow(
    clippy::manual_ignore_case_cmp,
    reason = "case folding needed for range comparison"
)]
pub(crate) fn collection_item_matches(
    item: &CollectionItem,
    ch: char,
    case_sensitive: bool,
) -> bool {
    match item {
        CollectionItem::Single(c) => {
            if case_sensitive {
                ch == *c
            } else {
                ch.to_ascii_lowercase() == c.to_ascii_lowercase()
            }
        }
        CollectionItem::Range(lo, hi) => {
            if case_sensitive {
                ch >= *lo && ch <= *hi
            } else {
                // Check both the original char and its case-folded variant
                let ch_lo = ch.to_ascii_lowercase();
                let ch_hi = ch.to_ascii_uppercase();
                let range_lo = *lo;
                let range_hi = *hi;
                (ch >= range_lo && ch <= range_hi)
                    || (ch_lo >= range_lo && ch_lo <= range_hi)
                    || (ch_hi >= range_lo && ch_hi <= range_hi)
            }
        }
        CollectionItem::Class(class) => class_matches(*class, ch),
        CollectionItem::PosixClass(name) => posix_class_matches(*name, ch),
        CollectionItem::Newline => ch == '\n',
    }
}

/// Tests whether `ch` belongs to the given POSIX named character class.
#[must_use]
pub(crate) const fn posix_class_matches(name: PosixClassName, ch: char) -> bool {
    match name {
        PosixClassName::Alnum => ch.is_ascii_alphanumeric(),
        PosixClassName::Alpha => ch.is_ascii_alphabetic(),
        PosixClassName::Blank => matches!(ch, ' ' | '\t'),
        PosixClassName::Cntrl => ch.is_ascii_control(),
        PosixClassName::Digit => ch.is_ascii_digit(),
        PosixClassName::Graph => ch.is_ascii_graphic(),
        PosixClassName::Lower => ch.is_ascii_lowercase(),
        PosixClassName::Print => !ch.is_ascii_control() && ch != '\x7f',
        PosixClassName::Punct => ch.is_ascii_punctuation(),
        PosixClassName::Space => ch.is_ascii_whitespace(),
        PosixClassName::Upper => ch.is_ascii_uppercase(),
        PosixClassName::Xdigit => ch.is_ascii_hexdigit(),
        PosixClassName::Tab => ch == '\t',
        PosixClassName::Return => ch == '\r',
        PosixClassName::Backspace => ch == '\x08',
        PosixClassName::Escape => ch == '\x1b',
        PosixClassName::Ident => ch.is_ascii_alphanumeric() || ch == '_',
        PosixClassName::Keyword => ch.is_ascii_alphanumeric() || ch == '_',
        PosixClassName::Fname => {
            ch.is_ascii_alphanumeric() || matches!(ch, '_' | '/' | '.' | '~' | '-')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::CharClass;

    #[test]
    fn class_digit_matches_digits_only() {
        assert!(class_matches(CharClass::Digit, '0'));
        assert!(class_matches(CharClass::Digit, '9'));
        assert!(!class_matches(CharClass::Digit, 'a'));
        assert!(!class_matches(CharClass::Digit, '\n'));
    }

    #[test]
    fn class_word_includes_underscore() {
        assert!(class_matches(CharClass::Word, '_'));
        assert!(class_matches(CharClass::Word, 'a'));
        assert!(class_matches(CharClass::Word, '0'));
        assert!(!class_matches(CharClass::Word, ' '));
        assert!(!class_matches(CharClass::Word, '-'));
    }

    #[test]
    fn class_print_matches_non_ascii() {
        assert!(class_matches(CharClass::Print, 'a'));
        assert!(class_matches(CharClass::Print, ' '));
        assert!(!class_matches(CharClass::Print, '\n'));
        assert!(!class_matches(CharClass::Print, '\x7f'));
        // Print matches ALL non-ASCII chars
        assert!(class_matches(CharClass::Print, '\u{00E9}'));
        assert!(class_matches(CharClass::Print, '\u{4E16}'));
    }

    #[test]
    fn class_sprint_excludes_digits() {
        assert!(class_matches(CharClass::SPrint, 'a'));
        assert!(class_matches(CharClass::SPrint, ' '));
        assert!(!class_matches(CharClass::SPrint, '0'));
        assert!(!class_matches(CharClass::SPrint, '9'));
        assert!(!class_matches(CharClass::SPrint, '\n'));
        // SPrint also matches non-ASCII
        assert!(class_matches(CharClass::SPrint, '\u{00E9}'));
    }

    #[test]
    fn class_filename_includes_tilde() {
        assert!(class_matches(CharClass::FileName, '~'));
        assert!(class_matches(CharClass::FileName, '/'));
        assert!(class_matches(CharClass::FileName, '.'));
        assert!(class_matches(CharClass::FileName, '_'));
        assert!(!class_matches(CharClass::FileName, ' '));
    }

    #[test]
    fn class_octal_range() {
        for c in '0'..='7' {
            assert!(
                class_matches(CharClass::Octal, c),
                "Octal should match '{c}'"
            );
        }
        assert!(!class_matches(CharClass::Octal, '8'));
        assert!(!class_matches(CharClass::Octal, '9'));
    }

    #[test]
    fn posix_fname_includes_tilde() {
        assert!(posix_class_matches(PosixClassName::Fname, '~'));
        assert!(posix_class_matches(PosixClassName::Fname, '/'));
    }

    #[test]
    fn is_word_char_unicode_letters() {
        // ASCII word chars
        assert!(is_word_char('a'));
        assert!(is_word_char('Z'));
        assert!(is_word_char('0'));
        assert!(is_word_char('_'));
        assert!(!is_word_char(' '));
        assert!(!is_word_char('-'));

        // Non-ASCII letters should be word chars (Vim default)
        assert!(is_word_char('\u{00E9}')); // e with acute (e)
        assert!(is_word_char('\u{00FC}')); // u with umlaut (u)
        assert!(is_word_char('\u{0410}')); // Cyrillic A
        assert!(is_word_char('\u{03B1}')); // Greek alpha

        // Non-ASCII non-alphanumeric should NOT be word chars
        assert!(!is_word_char('\u{00A0}')); // non-breaking space
        assert!(!is_word_char('\u{2014}')); // em dash
    }

    #[test]
    fn class_word_unicode() {
        assert!(class_matches(CharClass::Word, '\u{00E9}')); // e
        assert!(!class_matches(CharClass::NotWord, '\u{00E9}')); // e
        assert!(class_matches(CharClass::Word, '\u{0410}')); // Cyrillic A
        assert!(!class_matches(CharClass::NotWord, '\u{0410}')); // Cyrillic A
    }
}
