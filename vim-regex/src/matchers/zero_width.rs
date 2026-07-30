use super::char_match::is_word_char;
use super::context::MatchContext;
use crate::ir::{ColumnSpec, LineSpec, MarkRel};

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER
// ═══════════════════════════════════════════════════════════════════════════════

/// A zero-width assertion that matches a position without consuming input.
///
/// Returns `Some(0)` on match, `None` on failure.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ZeroWidthMatcher {
    /// `^` — start of line.
    StartOfLine,
    /// `$` — end of line.
    EndOfLine,
    /// `\%^` — start of file.
    StartOfFile,
    /// `\%$` — end of file.
    EndOfFile,
    /// `\<` — start of word boundary.
    WordBoundaryStart,
    /// `\>` — end of word boundary.
    WordBoundaryEnd,
    /// `\zs` — set match start.
    SetMatchStart,
    /// `\ze` — set match end.
    SetMatchEnd,
    /// `\%#` — cursor position.
    CursorPosition,
    /// `\%V` — inside visual selection.
    VisualArea,
    /// `\%l` — at a specific line.
    AtLine(LineSpec),
    /// `\%c` — at a specific byte column.
    AtColumn(ColumnSpec),
    /// `\%v` — at a specific virtual column.
    AtVirtualColumn(ColumnSpec),
    /// `\%'m` — at the position of a mark.
    AtMark {
        /// The mark character.
        mark: char,
        /// Relationship to the mark position.
        rel: MarkRel,
    },
}

impl ZeroWidthMatcher {
    /// Attempt to match at `pos` in `text`.
    ///
    /// Returns `Some(0)` on success (zero bytes consumed), `None` on failure.
    #[inline]
    pub(crate) fn matches(&self, text: &str, pos: usize, ctx: &MatchContext<'_>) -> Option<usize> {
        match self {
            Self::StartOfLine => Self::match_start_of_line(text, pos),
            Self::EndOfLine => Self::match_end_of_line(text, pos),
            Self::StartOfFile => Self::match_start_of_file(pos),
            Self::EndOfFile => Self::match_end_of_file(text, pos),
            Self::WordBoundaryStart => Self::match_word_boundary_start(text, pos),
            Self::WordBoundaryEnd => Self::match_word_boundary_end(text, pos),
            Self::SetMatchStart | Self::SetMatchEnd => Some(0),
            Self::CursorPosition => Self::match_cursor(pos, ctx),
            Self::VisualArea => Self::match_visual_area(pos, ctx),
            Self::AtLine(spec) => Self::match_at_line(*spec, pos, ctx),
            Self::AtColumn(spec) => Self::match_at_column(*spec, pos, ctx),
            Self::AtVirtualColumn(spec) => Self::match_at_vcol(*spec, pos, ctx),
            Self::AtMark { mark, rel } => Self::match_at_mark(*mark, *rel, pos, ctx),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — LINE BOUNDARY HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl ZeroWidthMatcher {
    /// `^` — matches at pos 0, or immediately after a `\n`.
    fn match_start_of_line(text: &str, pos: usize) -> Option<usize> {
        if pos == 0 {
            return Some(0);
        }
        // Check if the byte immediately before `pos` is '\n'
        let bytes = text.as_bytes();
        if pos <= bytes.len() {
            bytes
                .get(pos - 1)
                .and_then(|&b| if b == b'\n' { Some(0) } else { None })
        } else {
            None
        }
    }

    /// `$` — matches before a `\n`, or at the end of text.
    fn match_end_of_line(text: &str, pos: usize) -> Option<usize> {
        if pos == text.len() {
            return Some(0);
        }
        text.as_bytes()
            .get(pos)
            .and_then(|&b| if b == b'\n' { Some(0) } else { None })
    }

    /// `\%^` — matches only at pos 0.
    const fn match_start_of_file(pos: usize) -> Option<usize> {
        if pos == 0 {
            Some(0)
        } else {
            None
        }
    }

    /// `\%$` — matches only at the end of the text.
    const fn match_end_of_file(text: &str, pos: usize) -> Option<usize> {
        if pos == text.len() {
            Some(0)
        } else {
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — WORD BOUNDARY HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl ZeroWidthMatcher {
    /// `\<` — word boundary start: non-word before, word char at pos.
    fn match_word_boundary_start(text: &str, pos: usize) -> Option<usize> {
        let current = char_at_byte(text, pos)?;
        if !is_word_char(current) {
            return None;
        }
        // At start of text, or previous char is non-word
        let prev_is_word = char_before_byte(text, pos).is_some_and(is_word_char);
        if prev_is_word {
            None
        } else {
            Some(0)
        }
    }

    /// `\>` — word boundary end: word char before, non-word (or end) at pos.
    fn match_word_boundary_end(text: &str, pos: usize) -> Option<usize> {
        let prev = char_before_byte(text, pos)?;
        if !is_word_char(prev) {
            return None;
        }
        // At end of text, or current char is non-word
        let current_is_word = char_at_byte(text, pos).is_some_and(is_word_char);
        if current_is_word {
            None
        } else {
            Some(0)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH MATCHER — BUFFER POSITION HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl ZeroWidthMatcher {
    /// `\%#` — matches only at the cursor position.
    /// Returns `None` when no cursor is set (cursor is `None`).
    const fn match_cursor(pos: usize, ctx: &MatchContext<'_>) -> Option<usize> {
        match ctx.cursor {
            Some(cursor) if cursor == pos => Some(0),
            _ => None,
        }
    }

    /// `\%V` — matches inside the visual selection range.
    fn match_visual_area(pos: usize, ctx: &MatchContext<'_>) -> Option<usize> {
        let (start, end) = ctx.visual_range?;
        if pos >= start && pos <= end {
            Some(0)
        } else {
            None
        }
    }

    /// `\%l` — matches at a specific line position.
    fn match_at_line(spec: LineSpec, pos: usize, ctx: &MatchContext<'_>) -> Option<usize> {
        let resolver = ctx.line_resolver?;
        let line = resolver.byte_to_line(pos);
        let matched = match spec {
            LineSpec::Exact(n) => line == n,
            LineSpec::Before(n) => line < n,
            LineSpec::After(n) => line > n,
            LineSpec::Current => line == resolver.cursor_line(),
            LineSpec::BeforeCurrent => line < resolver.cursor_line(),
            LineSpec::AfterCurrent => line > resolver.cursor_line(),
        };
        if matched {
            Some(0)
        } else {
            None
        }
    }

    /// `\%c` — matches at a specific byte column.
    fn match_at_column(spec: ColumnSpec, pos: usize, ctx: &MatchContext<'_>) -> Option<usize> {
        let resolver = ctx.line_resolver?;
        let col = resolver.byte_to_col(pos);
        let matched = match spec {
            ColumnSpec::Exact(n) => col == n,
            ColumnSpec::Before(n) => col < n,
            ColumnSpec::After(n) => col > n,
            ColumnSpec::Current => col == resolver.byte_to_col(ctx.cursor?),
            ColumnSpec::BeforeCurrent => col < resolver.byte_to_col(ctx.cursor?),
            ColumnSpec::AfterCurrent => col > resolver.byte_to_col(ctx.cursor?),
        };
        if matched {
            Some(0)
        } else {
            None
        }
    }

    /// `\%v` — matches at a specific virtual column.
    fn match_at_vcol(spec: ColumnSpec, pos: usize, ctx: &MatchContext<'_>) -> Option<usize> {
        let resolver = ctx.line_resolver?;
        let vcol = resolver.byte_to_vcol(pos);
        let matched = match spec {
            ColumnSpec::Exact(n) => vcol == n,
            ColumnSpec::Before(n) => vcol < n,
            ColumnSpec::After(n) => vcol > n,
            ColumnSpec::Current => vcol == resolver.byte_to_vcol(ctx.cursor?),
            ColumnSpec::BeforeCurrent => vcol < resolver.byte_to_vcol(ctx.cursor?),
            ColumnSpec::AfterCurrent => vcol > resolver.byte_to_vcol(ctx.cursor?),
        };
        if matched {
            Some(0)
        } else {
            None
        }
    }

    /// `\%'m` — matches at the position of a mark.
    fn match_at_mark(
        mark: char,
        rel: MarkRel,
        pos: usize,
        ctx: &MatchContext<'_>,
    ) -> Option<usize> {
        let resolver = ctx.mark_resolver?;
        let mark_pos = resolver.mark_position(mark)?;
        let matched = match rel {
            MarkRel::At => pos == mark_pos,
            MarkRel::Before => pos < mark_pos,
            MarkRel::After => pos > mark_pos,
        };
        if matched {
            Some(0)
        } else {
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT NAVIGATION HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Returns the character at byte offset `pos` in `text`, if valid.
#[must_use]
pub(super) fn char_at_byte(text: &str, pos: usize) -> Option<char> {
    text.get(pos..)?.chars().next()
}

/// Returns the character immediately before byte offset `pos` in `text`.
#[must_use]
pub(super) fn char_before_byte(text: &str, pos: usize) -> Option<char> {
    if pos == 0 {
        return None;
    }
    // Walk backwards from pos to find the start of the previous character.
    // UTF-8 continuation bytes start with 10xxxxxx, so we look for a
    // byte that does NOT start with 10 (i.e., is a leading byte).
    let bytes = text.as_bytes();
    let mut i = pos - 1;
    while i > 0 && bytes.get(i).is_some_and(|&b| b & 0xC0 == 0x80) {
        i -= 1;
    }
    text.get(i..)?.chars().next()
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINING MARK HELPERS (for \Z composing mode)
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a character is a Unicode combining mark (general category M).
///
/// Covers: Mn (Nonspacing Mark), Mc (Spacing Mark), Me (Enclosing Mark).
/// This is a zero-dependency implementation covering the most common combining
/// mark ranges in the Unicode BMP and supplementary planes.
#[must_use]
pub(crate) fn is_combining_mark(ch: char) -> bool {
    let cp = ch as u32;
    matches!(cp,
        // Combining Diacritical Marks
        0x0300..=0x036F |
        // Cyrillic combining marks
        0x0483..=0x0489 |
        // Hebrew combining marks
        0x0591..=0x05BD |
        0x05BF |
        0x05C1..=0x05C2 |
        0x05C4..=0x05C5 |
        0x05C7 |
        // Arabic combining marks
        0x0610..=0x061A |
        0x064B..=0x065F |
        0x0670 |
        0x06D6..=0x06DC |
        0x06DF..=0x06E4 |
        0x06E7..=0x06E8 |
        0x06EA..=0x06ED |
        // Syriac
        0x0711 |
        0x0730..=0x074A |
        // Thaana
        0x07A6..=0x07B0 |
        // Devanagari
        0x0900..=0x0903 |
        0x093A..=0x094F |
        0x0951..=0x0957 |
        0x0962..=0x0963 |
        // Bengali
        0x0981..=0x0983 |
        0x09BC |
        0x09BE..=0x09C4 |
        0x09C7..=0x09C8 |
        0x09CB..=0x09CD |
        0x09D7 |
        0x09E2..=0x09E3 |
        // Gurmukhi
        0x0A01..=0x0A03 |
        0x0A3C |
        0x0A3E..=0x0A42 |
        0x0A47..=0x0A48 |
        0x0A4B..=0x0A4D |
        0x0A51 |
        0x0A70..=0x0A71 |
        0x0A75 |
        // Thai
        0x0E31 |
        0x0E34..=0x0E3A |
        0x0E47..=0x0E4E |
        // Lao
        0x0EB1 |
        0x0EB4..=0x0EBC |
        0x0EC8..=0x0ECE |
        // Combining Diacritical Marks Extended
        0x1AB0..=0x1AFF |
        // Combining Diacritical Marks Supplement
        0x1DC0..=0x1DFF |
        // Combining Marks for Symbols
        0x20D0..=0x20FF |
        // Combining Half Marks
        0xFE20..=0xFE2F
    )
}

/// Skip past any Unicode combining marks (category M) starting at `pos`.
///
/// Returns the byte position after all combining marks have been skipped.
/// If `pos` is already at a non-combining character or end of text, returns `pos`.
#[must_use]
pub(crate) fn skip_combining_marks(text: &str, pos: usize) -> usize {
    let mut p = pos;
    let slice = match text.get(pos..) {
        Some(s) => s,
        None => return pos,
    };
    for ch in slice.chars() {
        if is_combining_mark(ch) {
            p += ch.len_utf8();
        } else {
            break;
        }
    }
    p
}
