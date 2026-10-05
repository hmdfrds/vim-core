//! Parsed form of Vim's `formatoptions` option.
//!
//! `formatoptions` is a list of single-letter flags (`:help fo-table`). The
//! raw string is what the user sees and edits; the engine reads the parsed
//! [`FormatFlags`] instead, so the hot insert path tests bits rather than
//! scanning a string on every keystroke.

use bitflags::bitflags;

bitflags! {
    /// The flags of Vim's `formatoptions`, one bit per letter.
    ///
    /// Build one with [`FormatFlags::parse`], which rejects letters Vim does
    /// not know, or [`FormatFlags::parse_lossy`], which skips them.
    ///
    /// # Supported flags
    ///
    /// The engine acts on `t`, `c`, `q`, `w`, `2`, `v`, `b`, `l`, `1` and
    /// `p`, and on `M` and `B` when `gq` joins lines. `:set` accepts every flag
    /// Vim knows, but these have no effect yet:
    ///
    /// - `j`: `J` and `:join` keep the comment leader of the joined line;
    /// - `M` and `B` in `J` and `:join`, which put a space between multibyte
    ///   characters as without them;
    /// - `r` and `o`: Enter, `o` and `O` do not continue a comment leader, and
    ///   `/` (which only changes `o`) does nothing either;
    /// - `a`: paragraphs are not reformatted automatically while typing;
    /// - `n`: numbered lists are not recognized;
    /// - `m` and `]`: text without blanks does not break at multibyte
    ///   characters.
    ///
    /// Typing in Virtual Replace mode (`gR`) does not format either.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct FormatFlags: u32 {
        /// `t`: auto-wrap text using `textwidth`.
        const WRAP_TEXT = 1 << 0;
        /// `c`: auto-wrap comments using `textwidth`, inserting the leader.
        const WRAP_COMMENTS = 1 << 1;
        /// `r`: insert the comment leader after `<Enter>` in Insert mode. Not
        /// supported yet.
        const RETURN_COMMENTS = 1 << 2;
        /// `o`: insert the comment leader after `o` or `O`. Not supported yet.
        const OPEN_COMMENTS = 1 << 3;
        /// `/`: with `o`, only insert the leader for a whole-line comment. Not
        /// supported yet.
        const NO_OPEN_TRAILING_COMMENTS = 1 << 4;
        /// `q`: allow formatting comments with `gq`.
        const FORMAT_COMMENTS = 1 << 5;
        /// `w`: trailing white space marks a paragraph that continues.
        const WHITE_PARAGRAPH = 1 << 6;
        /// `a`: automatic formatting of paragraphs. Not supported yet.
        const AUTO_FORMAT = 1 << 7;
        /// `n`: recognize numbered lists (uses `formatlistpat`). Not supported
        /// yet.
        const NUMBERED_LISTS = 1 << 8;
        /// `2`: use the second line's indent for the rest of the paragraph.
        const SECOND_LINE_INDENT = 1 << 9;
        /// `v`: Vi-compatible auto-wrap, only at blanks typed in this insert.
        const VI_WRAP = 1 << 10;
        /// `b`: like `v`, but only when a blank was typed at or before the margin.
        const BLANK_WRAP = 1 << 11;
        /// `l`: do not break lines that were already long when insert started.
        const LONG_LINES = 1 << 12;
        /// `m`: also break at a multibyte character above 255. Not supported yet.
        const MBYTE_BREAK = 1 << 13;
        /// `M`: no space before or after a multibyte character when joining.
        /// Only `gq` follows it yet.
        const MBYTE_JOIN = 1 << 14;
        /// `B`: no space between two multibyte characters when joining. Only
        /// `gq` follows it yet.
        const MBYTE_JOIN_BETWEEN = 1 << 15;
        /// `1`: do not break a line after a one-letter word.
        const ONE_LETTER = 1 << 16;
        /// `]`: respect `textwidth` rigorously. Not supported yet.
        const RIGOROUS_TEXTWIDTH = 1 << 17;
        /// `j`: remove the comment leader when joining lines. Not supported
        /// yet.
        const REMOVE_COMMENT_LEADER = 1 << 18;
        /// `p`: do not break at a single space after a period.
        const PERIOD_ABBREVIATION = 1 << 19;
    }
}

/// Every character Vim accepts in `formatoptions`, in Vim's own order
/// (`FO_ALL`). The comma is accepted for compatibility and has no effect.
const ACCEPTED: &str = "tcro/q2vlb1mMBn,aw]jp";

impl FormatFlags {
    /// The flag for one `formatoptions` letter.
    ///
    /// Returns `None` for characters that are not flags, including the
    /// comma that Vim accepts but ignores.
    #[must_use]
    pub const fn from_char(c: char) -> Option<Self> {
        Some(match c {
            't' => Self::WRAP_TEXT,
            'c' => Self::WRAP_COMMENTS,
            'r' => Self::RETURN_COMMENTS,
            'o' => Self::OPEN_COMMENTS,
            '/' => Self::NO_OPEN_TRAILING_COMMENTS,
            'q' => Self::FORMAT_COMMENTS,
            'w' => Self::WHITE_PARAGRAPH,
            'a' => Self::AUTO_FORMAT,
            'n' => Self::NUMBERED_LISTS,
            '2' => Self::SECOND_LINE_INDENT,
            'v' => Self::VI_WRAP,
            'b' => Self::BLANK_WRAP,
            'l' => Self::LONG_LINES,
            'm' => Self::MBYTE_BREAK,
            'M' => Self::MBYTE_JOIN,
            'B' => Self::MBYTE_JOIN_BETWEEN,
            '1' => Self::ONE_LETTER,
            ']' => Self::RIGOROUS_TEXTWIDTH,
            'j' => Self::REMOVE_COMMENT_LEADER,
            'p' => Self::PERIOD_ABBREVIATION,
            _ => return None,
        })
    }

    /// Parse a `formatoptions` value, rejecting characters Vim does not accept.
    ///
    /// # Errors
    ///
    /// Returns the first illegal character. Vim reports it as
    /// `E539: Illegal character <c>`.
    pub fn parse(value: &str) -> Result<Self, char> {
        let mut flags = Self::empty();
        for c in value.chars() {
            if !ACCEPTED.contains(c) {
                return Err(c);
            }
            if let Some(flag) = Self::from_char(c) {
                flags |= flag;
            }
        }
        Ok(flags)
    }

    /// Parse a `formatoptions` value, skipping characters Vim does not accept.
    ///
    /// Used where a value arrives without validation, such as a host call to
    /// `VimOptions::set_formatoptions`.
    #[must_use]
    pub fn parse_lossy(value: &str) -> Self {
        value
            .chars()
            .filter_map(Self::from_char)
            .fold(Self::empty(), |acc, flag| acc | flag)
    }

    /// Normalize a `formatoptions` value the way Vim stores it.
    ///
    /// Vim removes a flag that appears again later in the value, so the
    /// last occurrence wins: `"tcqt"` becomes `"cqt"`. This is what makes
    /// `:set fo+=t` move `t` to the end instead of duplicating it.
    #[must_use]
    pub fn normalize(value: &str) -> String {
        value
            .char_indices()
            .filter(|&(i, c)| {
                value
                    .get(i + c.len_utf8()..)
                    .is_none_or(|rest| !rest.contains(c))
            })
            .map(|(_, c)| c)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_neovim_default() {
        let flags = FormatFlags::parse("tcqj").unwrap();
        assert_eq!(
            flags,
            FormatFlags::WRAP_TEXT
                | FormatFlags::WRAP_COMMENTS
                | FormatFlags::FORMAT_COMMENTS
                | FormatFlags::REMOVE_COMMENT_LEADER
        );
    }

    #[test]
    fn parse_empty_is_empty() {
        assert_eq!(FormatFlags::parse(""), Ok(FormatFlags::empty()));
    }

    #[test]
    fn parse_accepts_every_vim_flag() {
        // The set Vim 9.1 accepts, probed with `:set fo=<c>` for every
        // printable ASCII character.
        let flags = FormatFlags::parse(",/12BM]abcjlmnopqrtvw").unwrap();
        assert_eq!(flags, FormatFlags::all());
    }

    #[test]
    fn parse_rejects_unknown_flag() {
        assert_eq!(FormatFlags::parse("tZ"), Err('Z'));
        assert_eq!(FormatFlags::parse("O"), Err('O'));
        assert_eq!(FormatFlags::parse("t c"), Err(' '));
    }

    #[test]
    fn comma_is_accepted_but_sets_nothing() {
        assert_eq!(FormatFlags::parse(","), Ok(FormatFlags::empty()));
        assert_eq!(FormatFlags::from_char(','), None);
    }

    #[test]
    fn parse_lossy_skips_unknown_flags() {
        assert_eq!(
            FormatFlags::parse_lossy("tZc"),
            FormatFlags::WRAP_TEXT | FormatFlags::WRAP_COMMENTS
        );
    }

    #[test]
    fn every_flag_has_a_distinct_letter() {
        let mut seen = FormatFlags::empty();
        for c in ACCEPTED.chars() {
            if let Some(flag) = FormatFlags::from_char(c) {
                assert!(!seen.intersects(flag), "{c} reuses a bit");
                seen |= flag;
            }
        }
        assert_eq!(seen, FormatFlags::all());
    }

    #[test]
    fn normalize_keeps_last_occurrence() {
        // Vim 9.1: `:set fo=tt` gives "t", `:set fo=tcq | set fo+=t`
        // gives "cqt", and `:set fo^=q` on "tcq" gives "tcq".
        assert_eq!(FormatFlags::normalize("tt"), "t");
        assert_eq!(FormatFlags::normalize("tcqt"), "cqt");
        assert_eq!(FormatFlags::normalize("qtcq"), "tcq");
        assert_eq!(FormatFlags::normalize("tcqc,q"), "tc,q");
        assert_eq!(FormatFlags::normalize(""), "");
    }
}
