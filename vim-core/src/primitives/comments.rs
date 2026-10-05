//! Vim's `comments` option: parsing and comment-leader matching.
//!
//! `comments` is a comma-separated list of `{flags}:{string}` parts
//! (`:help format-comments`). Formatting uses it to decide whether a line is
//! a comment, where its leader ends, and what leader the next line gets when
//! the comment continues.
//!
//! The matcher is a port of Vim's `get_leader_len()` (change.c) and the
//! continuation is a port of the leader handling in `open_line()` for a new
//! line opened below. Both work on bytes like Vim does; a comment string is
//! matched byte for byte against the line.

use std::fmt;

use bitflags::bitflags;
use compact_str::CompactString;
use unicode_width::UnicodeWidthChar;

/// Vim's default `comments` value (`:help 'comments'`, Vim 9.1).
pub const DEFAULT_COMMENTS: &str = "s1:/*,mb:*,ex:*/,://,b:#,:%,:XCOMM,n:>,fb:-";

/// Every flag letter Vim accepts before the colon (`COM_ALL`).
const FLAG_LETTERS: &str = "nbsmexflrO";

bitflags! {
    /// The letter flags of one `comments` part.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct CommentFlags: u16 {
        /// `n`: the comment nests, so further nested leaders may follow.
        const NEST = 1 << 0;
        /// `b`: a blank or end of line must follow the string.
        const BLANK = 1 << 1;
        /// `f`: only the first line has the leader; continuation lines keep
        /// its width as spaces.
        const FIRST = 1 << 2;
        /// `s`: start of a three-piece comment.
        const START = 1 << 3;
        /// `m`: middle of a three-piece comment.
        const MIDDLE = 1 << 4;
        /// `e`: end of a three-piece comment.
        const END = 1 << 5;
        /// `x`: typing the end string's last character ends the comment.
        const AUTO_END = 1 << 6;
        /// `O`: not used for the `O` command.
        const NO_BACK = 1 << 7;
        /// `l`: left-align the inserted leader (the default).
        const LEFT = 1 << 8;
        /// `r`: right-align the inserted leader.
        const RIGHT = 1 << 9;
    }
}

impl CommentFlags {
    /// The flag for one letter, or `None` if the letter is not a flag.
    #[must_use]
    pub const fn from_char(c: char) -> Option<Self> {
        Some(match c {
            'n' => Self::NEST,
            'b' => Self::BLANK,
            'f' => Self::FIRST,
            's' => Self::START,
            'm' => Self::MIDDLE,
            'e' => Self::END,
            'x' => Self::AUTO_END,
            'O' => Self::NO_BACK,
            'l' => Self::LEFT,
            'r' => Self::RIGHT,
            _ => return None,
        })
    }
}

/// A `comments` value that Vim would refuse to set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentsError {
    /// A character before the colon that is neither a flag, a digit nor `-`.
    IllegalChar(char),
    /// A part has no colon.
    MissingColon,
    /// A part has nothing after its colon.
    ZeroLength,
}

impl fmt::Display for CommentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IllegalChar(c) => write!(f, "E539: Illegal character <{c}>"),
            Self::MissingColon => f.write_str("E524: Missing colon"),
            Self::ZeroLength => f.write_str("E525: Zero length string"),
        }
    }
}

impl std::error::Error for CommentsError {}

/// One `{flags}:{string}` part of the `comments` option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentPart {
    flags: CommentFlags,
    offset: i32,
    string: CompactString,
}

impl CommentPart {
    /// The letter flags of this part.
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> CommentFlags {
        self.flags
    }

    /// The numeric offset given with the flags (`s1`, `s-2`), 0 if none.
    #[inline]
    #[must_use]
    pub const fn offset(&self) -> i32 {
        self.offset
    }

    /// The comment string itself, with option escapes removed.
    #[inline]
    #[must_use]
    pub fn string(&self) -> &str {
        &self.string
    }
}

/// A parsed `comments` option.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommentSpec {
    parts: Vec<CommentPart>,
}

/// Where a comment leader sits on a line, as returned by
/// [`CommentSpec::match_line`]. All positions are byte offsets into the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderMatch {
    /// End of the white space before the leader.
    pub indent_end: usize,
    /// End of the leader. With nested comments (`n`) this is the end of the
    /// last nested leader.
    pub leader_end: usize,
    /// End of the white space that follows the leader.
    pub ws_end: usize,
    /// Index into [`CommentSpec::parts`] of the part that matched first.
    pub part: usize,
}

/// The leader Vim gives a new line that continues a comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentContinuation {
    /// Indent of the new line in display columns, to be built from tabs or
    /// spaces according to `expandtab`. When 0, any white space the leader
    /// starts with is the indent, copied verbatim from the original line.
    pub indent: usize,
    /// The leader text that follows the indent, including the white space
    /// that separates it from the text.
    pub leader: CompactString,
}

const fn is_white(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Split a `comments` value into parts the way Vim's `copy_option_part()`
/// does: a backslash before a comma escapes it, and spaces after a comma
/// are skipped.
fn split_parts(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&',') => {
                current.push(',');
                chars.next();
            }
            ',' => {
                parts.push(std::mem::take(&mut current));
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Parse the flags before the colon of one part. The last number wins, as
/// in `open_line()`.
fn parse_flags(flags: &str) -> (CommentFlags, i32) {
    let mut result = CommentFlags::empty();
    let mut offset = 0;
    let mut chars = flags.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-' || c.is_ascii_digit() {
            let negative = c == '-';
            let mut n: i32 = c.to_digit(10).map_or(0, u32::cast_signed);
            while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                n = n.saturating_mul(10).saturating_add(d.cast_signed());
                chars.next();
            }
            offset = if negative { -n } else { n };
        } else if let Some(flag) = CommentFlags::from_char(c) {
            result |= flag;
        }
    }
    (result, offset)
}

/// Display cells of `s`, one per character for control characters.
fn cells(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(1)).sum()
}

/// Indent of `s` in display columns, expanding tabs to `tabstop`.
fn indent_columns(s: &str, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    let mut col = 0;
    for b in s.bytes() {
        match b {
            b' ' => col += 1,
            b'\t' => col += tabstop - col % tabstop,
            _ => break,
        }
    }
    col
}

/// Replace every non-white character in `s` with as many spaces as it has
/// cells, keeping tabs so the indent stays the same.
fn blank_out(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == ' ' || c == '\t' {
            out.push(c);
        } else {
            out.extend(std::iter::repeat_n(' ', c.width().unwrap_or(1).max(1)));
        }
    }
    out
}

impl CommentSpec {
    /// Parse a `comments` value, refusing what Vim refuses.
    ///
    /// # Errors
    ///
    /// Returns the first problem Vim's option check would report: an
    /// illegal flag character (E539), a part without a colon (E524) or a
    /// part with an empty string (E525).
    pub fn parse(value: &str) -> Result<Self, CommentsError> {
        Self::validate(value)?;
        Ok(Self::parse_lossy(value))
    }

    /// Check a `comments` value the way Vim's `did_set_comments()` does.
    ///
    /// # Errors
    ///
    /// See [`CommentSpec::parse`].
    pub fn validate(value: &str) -> Result<(), CommentsError> {
        let bytes = value.as_bytes();
        let at = |i: usize| bytes.get(i).copied();
        let mut s = 0;
        while s < bytes.len() {
            let mut error = None;
            while let Some(b) = at(s).filter(|&b| b != b':') {
                if !FLAG_LETTERS.as_bytes().contains(&b) && !b.is_ascii_digit() && b != b'-' {
                    let c = value.get(s..).and_then(|r| r.chars().next()).unwrap_or('?');
                    error = Some(CommentsError::IllegalChar(c));
                    break;
                }
                s += 1;
            }
            // Vim steps over the character it stopped at, colon or not, and
            // then checks for an empty string. That check runs even after an
            // illegal character and replaces its error, so "q" gives E525.
            if at(s).is_none() {
                return Err(CommentsError::MissingColon);
            }
            s += 1;
            if matches!(at(s), None | Some(b',')) {
                return Err(CommentsError::ZeroLength);
            }
            if let Some(error) = error {
                return Err(error);
            }
            while let Some(b) = at(s).filter(|&b| b != b',') {
                if b == b'\\' && at(s + 1).is_some() {
                    s += 1;
                }
                s += 1;
            }
            if at(s) == Some(b',') {
                s += 1;
            }
            while at(s) == Some(b' ') {
                s += 1;
            }
        }
        Ok(())
    }

    /// Parse a `comments` value without validating it.
    ///
    /// A part without a colon is skipped, which is what Vim's matcher does
    /// with one. Used for values that did not come through `:set`.
    #[must_use]
    pub fn parse_lossy(value: &str) -> Self {
        let parts = split_parts(value)
            .into_iter()
            .filter_map(|part| {
                let (flags, string) = part.split_once(':')?;
                let (flags, offset) = parse_flags(flags);
                Some(CommentPart {
                    flags,
                    offset,
                    string: CompactString::from(string),
                })
            })
            .collect();
        Self { parts }
    }

    /// The parts in option order.
    #[inline]
    #[must_use]
    pub const fn parts(&self) -> &[CommentPart] {
        self.parts.as_slice()
    }

    /// Whether the option is empty, so no line is a comment.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Find the comment leader at the start of `line`, if any.
    ///
    /// Port of Vim's `get_leader_len()` for a forward match. Leading white
    /// space is skipped; a part whose string starts with white space only
    /// matches after white space; a `b` part needs a blank or the end of the
    /// line after its string; a middle part gives way to a longer end part;
    /// and `n` parts may repeat (`> > text`).
    #[must_use]
    pub fn match_line(&self, line: &str) -> Option<LeaderMatch> {
        self.match_line_impl(line, false)
    }

    fn match_line_impl(&self, line: &str, backward: bool) -> Option<LeaderMatch> {
        let bytes = line.as_bytes();
        let at = |i: usize| bytes.get(i).copied();
        let mut i = bytes.iter().take_while(|&&b| is_white(b)).count();
        let indent_end = i;
        let mut found: Option<LeaderMatch> = None;

        while i < bytes.len() {
            let mut middle: Option<(usize, usize)> = None;
            let mut matched: Option<(usize, usize)> = None;
            for (idx, part) in self.parts.iter().enumerate() {
                let flags = part.flags;
                // After a middle match, only a middle or end part may win.
                if middle.is_some() && !flags.intersects(CommentFlags::MIDDLE | CommentFlags::END) {
                    break;
                }
                // Once inside a comment, only nested parts may follow.
                if found.is_some() && !flags.contains(CommentFlags::NEST) {
                    continue;
                }
                if backward && flags.contains(CommentFlags::NO_BACK) {
                    continue;
                }
                let mut string = part.string.as_str();
                if string.starts_with([' ', '\t']) {
                    if i == 0 || !at(i - 1).is_some_and(is_white) {
                        continue;
                    }
                    string = string.trim_start_matches([' ', '\t']);
                }
                if !line.get(i..).is_some_and(|rest| rest.starts_with(string)) {
                    continue;
                }
                let len = string.len();
                if flags.contains(CommentFlags::BLANK) && at(i + len).is_some_and(|b| !is_white(b))
                {
                    continue;
                }
                if flags.contains(CommentFlags::MIDDLE) {
                    // The middle string can be a prefix of the end string;
                    // keep looking for a longer end match.
                    middle.get_or_insert((idx, len));
                    continue;
                }
                if middle.is_some_and(|(_, m)| len > m) {
                    middle = None;
                }
                matched = Some((idx, len));
                break;
            }

            let Some((idx, len)) = middle.or(matched) else {
                break;
            };
            i += len;
            let leader_end = i;
            while at(i).is_some_and(is_white) {
                i += 1;
            }
            let part = found.map_or(idx, |m| m.part);
            found = Some(LeaderMatch {
                indent_end,
                leader_end,
                ws_end: i,
                part,
            });
            let nests = self
                .parts
                .get(idx)
                .is_some_and(|p| p.flags.contains(CommentFlags::NEST));
            if !nests {
                break;
            }
        }
        found
    }
}

impl LeaderMatch {
    /// The leader for a new line opened below `line`, continuing the comment.
    ///
    /// Port of the comment-leader part of Vim's `open_line()` for a line
    /// opened forward with the leader requested (`fo` `r`, `o`, or a wrap
    /// with `c`). `line` is the whole original line and `split_at` the byte
    /// offset where the text moving to the new line begins (`line.len()` for
    /// `o`). `autoindent` and `tabstop` decide the new indent as they do in
    /// Vim.
    ///
    /// - A plain part repeats the leader as written, with its trailing white
    ///   space.
    /// - An `f` part keeps the width of the leader but blanks it out.
    /// - An `s` part is replaced by the middle part, aligned left (or right
    ///   with `r`) and shifted by the part's offset.
    /// - An `m` part repeats.
    /// - An `e` part, or a three-piece comment that ends on this line
    ///   before `split_at`, gives no leader: `None`.
    #[must_use]
    pub fn continuation(
        &self,
        spec: &CommentSpec,
        line: &str,
        split_at: usize,
        autoindent: bool,
        tabstop: usize,
    ) -> Option<CommentContinuation> {
        let part = spec.parts.get(self.part)?;
        let flags = part.flags;
        let mut leader = line.get(..self.ws_end)?.to_owned();
        let mut replacement: Option<&str> = None;
        let mut extra_space = false;

        if flags.intersects(CommentFlags::START | CommentFlags::MIDDLE) {
            // The middle and end parts follow the current part in order.
            let (middle_idx, end_idx) = if flags.contains(CommentFlags::START) {
                (self.part + 1, self.part + 2)
            } else {
                (self.part, self.part + 1)
            };
            let middle = spec.parts.get(middle_idx)?;
            let end = spec.parts.get(end_idx).map_or("", |p| p.string());
            // A comment that ends later on the same line is not continued.
            // Vim only looks at the text that stays on this line, before
            // `split_at`. With no end part it compares zero bytes, which
            // matches as soon as any text follows the leader.
            if line
                .get(self.ws_end..split_at.min(line.len()).max(self.ws_end))
                .is_some_and(|rest| !rest.is_empty() && rest.contains(end))
            {
                return None;
            }
            if flags.contains(CommentFlags::START) {
                replacement = Some(middle.string());
            }
            let require_blank = middle.flags.contains(CommentFlags::BLANK);
            let ends_in_white = self.ws_end > 0
                && line
                    .as_bytes()
                    .get(self.ws_end - 1)
                    .copied()
                    .is_some_and(is_white);
            extra_space = !ends_in_white && (split_at == self.ws_end || require_blank);
        } else if flags.contains(CommentFlags::END) {
            return None;
        } else if flags.contains(CommentFlags::FIRST) {
            replacement = Some("");
        }

        let mut indent = if autoindent {
            indent_columns(line, tabstop)
        } else {
            0
        };

        if let Some(repl) = replacement {
            leader = if flags.contains(CommentFlags::RIGHT) {
                replace_right(&leader, repl)
            } else {
                replace_left(&leader, repl)
            };
            if autoindent {
                indent = indent_columns(&leader, tabstop);
            }
            let mut off = part.offset;
            let shifted = i64::try_from(indent).unwrap_or(i64::MAX) + i64::from(off);
            if shifted < 0 {
                indent = 0;
            } else {
                indent = usize::try_from(shifted).unwrap_or(usize::MAX);
            }
            // Take the shift back out of the trailing spaces so the text
            // after the leader keeps its column.
            if !leader.trim_start_matches([' ', '\t']).contains('\t') {
                while off > 0 && leader.ends_with(' ') {
                    leader.pop();
                    off -= 1;
                }
            }
            if leader.ends_with([' ', '\t']) {
                extra_space = false;
            }
        }

        if extra_space {
            leader.push(' ');
        }
        if indent > 0 {
            let indent_len = leader.len() - leader.trim_start_matches([' ', '\t']).len();
            leader.drain(..indent_len);
        }
        Some(CommentContinuation {
            indent,
            leader: CompactString::from(leader),
        })
    }
}

/// Left-aligned replacement from `open_line()`: put `repl` where the old
/// leader text starts and blank out what is left of the old leader.
fn replace_left(leader: &str, repl: &str) -> String {
    let start = leader.len() - leader.trim_start_matches([' ', '\t']).len();
    let (indent, rest) = leader.split_at(start);
    let repl_cells = cells(repl);
    // How much of the old leader the replacement covers, in whole chars.
    let mut covered = 0;
    let mut covered_cells = 0;
    for c in rest.chars() {
        let w = c.width().unwrap_or(1);
        if covered_cells + w > repl_cells {
            break;
        }
        covered_cells += w;
        covered += c.len_utf8();
    }
    let tail = rest.get(covered..).unwrap_or("");
    let mut out = String::with_capacity(leader.len() + repl.len());
    out.push_str(indent);
    out.push_str(repl);
    // Blank out the rest of the old leader. Vim drops a character instead
    // of turning it into a space when a tab follows it.
    let tail_chars: Vec<char> = tail.chars().collect();
    for (k, &c) in tail_chars.iter().enumerate() {
        if c == ' ' || c == '\t' {
            out.push(c);
        } else if tail_chars.get(k + 1) != Some(&'\t') {
            out.extend(std::iter::repeat_n(' ', c.width().unwrap_or(1).max(1)));
        }
    }
    out
}

/// Right-aligned replacement from `open_line()`: line `repl` up with the
/// last non-white character of the old leader and blank out the rest.
fn replace_right(leader: &str, repl: &str) -> String {
    let content_end = leader.trim_end_matches([' ', '\t']).len();
    let (head, trailing) = leader.split_at(content_end);
    let repl_cells = cells(repl);
    let mut start = head.len();
    let mut old_cells = 0;
    for (pos, c) in head.char_indices().rev() {
        if old_cells >= repl_cells {
            break;
        }
        old_cells += c.width().unwrap_or(1);
        start = pos;
    }
    let mut out = blank_out(head.get(..start).unwrap_or(""));
    out.push_str(repl);
    out.push_str(trailing);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_spec() -> CommentSpec {
        CommentSpec::parse(DEFAULT_COMMENTS).unwrap()
    }

    fn leader(spec: &CommentSpec, line: &str) -> Option<(usize, usize, usize)> {
        spec.match_line(line)
            .map(|m| (m.indent_end, m.leader_end, m.ws_end))
    }

    /// The new line `o` produces after `line`, before the typed text:
    /// indent built from spaces plus the continuation leader.
    fn open_below(spec: &CommentSpec, line: &str, autoindent: bool) -> String {
        let Some(m) = spec.match_line(line) else {
            return String::new();
        };
        match m.continuation(spec, line, line.len(), autoindent, 8) {
            Some(c) => format!("{}{}", " ".repeat(c.indent), c.leader),
            None => String::new(),
        }
    }

    // ── Parsing ──────────────────────────────────────────────────────────

    #[test]
    fn parses_vim_default() {
        let spec = default_spec();
        let strings: Vec<&str> = spec.parts().iter().map(CommentPart::string).collect();
        assert_eq!(
            strings,
            ["/*", "*", "*/", "//", "#", "%", "XCOMM", ">", "-"]
        );
        let first = &spec.parts()[0];
        assert_eq!(first.flags(), CommentFlags::START);
        assert_eq!(first.offset(), 1);
        assert_eq!(
            spec.parts()[1].flags(),
            CommentFlags::MIDDLE | CommentFlags::BLANK
        );
        assert_eq!(
            spec.parts()[2].flags(),
            CommentFlags::END | CommentFlags::AUTO_END
        );
        assert_eq!(
            spec.parts()[8].flags(),
            CommentFlags::FIRST | CommentFlags::BLANK
        );
    }

    #[test]
    fn parses_negative_offset() {
        let spec = CommentSpec::parse("s-2:/*,mb:*,ex:*/").unwrap();
        assert_eq!(spec.parts()[0].offset(), -2);
    }

    #[test]
    fn empty_value_is_empty_spec() {
        let spec = CommentSpec::parse("").unwrap();
        assert!(spec.is_empty());
        assert_eq!(spec.match_line("# x"), None);
    }

    // Each expectation below is what Vim 9.1 reports for `:set com=<value>`.
    #[test]
    fn validation_matches_vim() {
        assert_eq!(CommentSpec::validate("x"), Err(CommentsError::MissingColon));
        assert_eq!(CommentSpec::validate("b"), Err(CommentsError::MissingColon));
        assert_eq!(CommentSpec::validate(":"), Err(CommentsError::ZeroLength));
        assert_eq!(CommentSpec::validate("s:"), Err(CommentsError::ZeroLength));
        assert_eq!(
            CommentSpec::validate(":a,,:b"),
            Err(CommentsError::IllegalChar(','))
        );
        assert_eq!(CommentSpec::validate("q"), Err(CommentsError::ZeroLength));
        assert_eq!(
            CommentSpec::validate("a,,b"),
            Err(CommentsError::ZeroLength)
        );
        assert_eq!(
            CommentSpec::validate(":x,q"),
            Err(CommentsError::ZeroLength)
        );
        assert_eq!(
            CommentSpec::validate("a:"),
            Err(CommentsError::IllegalChar('a'))
        );
        assert_eq!(
            CommentSpec::validate("qq:x"),
            Err(CommentsError::IllegalChar('q'))
        );
        assert_eq!(
            CommentSpec::validate("q:x"),
            Err(CommentsError::IllegalChar('q'))
        );
        assert_eq!(
            CommentSpec::validate(",:a"),
            Err(CommentsError::IllegalChar(','))
        );
        // `:set com=:a\,b` reaches the option as ":a,b" (the :set line
        // drops the backslash), so the second part has no colon. An escaped
        // comma that survives into the value is part of the string.
        assert_eq!(
            CommentSpec::validate(":a,b"),
            Err(CommentsError::MissingColon)
        );
        assert_eq!(CommentSpec::validate(":a\\,b"), Ok(()));
        for ok in [
            "",
            ":a,",
            "::",
            ":a:b",
            "fb:-",
            "bf:-",
            "sO:x",
            "x:x",
            "ss:x",
            "s12:/*",
            "s-2:/*",
            "n:>",
            ":a,:a",
            ":x, :y",
            "b:x,",
            DEFAULT_COMMENTS,
        ] {
            assert_eq!(CommentSpec::validate(ok), Ok(()), "{ok:?}");
        }
    }

    #[test]
    fn error_messages_use_vim_numbers() {
        assert_eq!(
            CommentsError::IllegalChar('q').to_string(),
            "E539: Illegal character <q>"
        );
        assert_eq!(
            CommentsError::MissingColon.to_string(),
            "E524: Missing colon"
        );
        assert_eq!(
            CommentsError::ZeroLength.to_string(),
            "E525: Zero length string"
        );
    }

    #[test]
    fn lossy_parse_skips_parts_without_colon() {
        let spec = CommentSpec::parse_lossy("x,b:#");
        assert_eq!(spec.parts().len(), 1);
        assert_eq!(spec.parts()[0].string(), "#");
    }

    #[test]
    fn escaped_comma_is_part_of_string() {
        let spec = CommentSpec::parse_lossy(":a\\,b");
        assert_eq!(spec.parts()[0].string(), "a,b");
    }

    // ── Matching ─────────────────────────────────────────────────────────

    #[test]
    fn matches_line_comment_with_trailing_space() {
        let spec = default_spec();
        assert_eq!(leader(&spec, "// foo"), Some((0, 2, 3)));
        assert_eq!(leader(&spec, "  // foo"), Some((2, 4, 5)));
        assert_eq!(leader(&spec, "\t//  foo"), Some((1, 3, 5)));
        assert_eq!(leader(&spec, "//"), Some((0, 2, 2)));
    }

    #[test]
    fn blank_flag_needs_white_space_after() {
        let spec = default_spec();
        assert_eq!(leader(&spec, "# foo"), Some((0, 1, 2)));
        assert_eq!(leader(&spec, "#"), Some((0, 1, 1)));
        assert_eq!(leader(&spec, "#foo"), None);
        assert_eq!(leader(&spec, "#region"), None);
    }

    #[test]
    fn not_a_comment() {
        let spec = default_spec();
        assert_eq!(leader(&spec, "x = 1 # foo"), None);
        assert_eq!(leader(&spec, ""), None);
        assert_eq!(leader(&spec, "    "), None);
        assert_eq!(leader(&spec, "-foo"), None);
    }

    #[test]
    fn three_piece_parts() {
        let spec = default_spec();
        let start = spec.match_line("/* foo").unwrap();
        assert_eq!((start.leader_end, start.part), (2, 0));
        let middle = spec.match_line(" * foo").unwrap();
        assert_eq!((middle.leader_end, middle.part), (2, 1));
        // "*/" is longer than the middle "*", so the end part wins.
        let end = spec.match_line(" */").unwrap();
        assert_eq!((end.leader_end, end.part), (3, 2));
    }

    #[test]
    fn middle_without_blank_falls_back_to_other_parts() {
        let spec = default_spec();
        // "*foo" fails mb:* (no blank) and ex:*/ (no '/'), so no leader.
        assert_eq!(leader(&spec, "*foo"), None);
    }

    #[test]
    fn nested_leaders_repeat() {
        let spec = default_spec();
        assert_eq!(leader(&spec, "> > foo"), Some((0, 3, 4)));
        assert_eq!(leader(&spec, ">> foo"), Some((0, 2, 3)));
        // '#' is not nested, so only the first '#' counts.
        let spec = CommentSpec::parse(":#").unwrap();
        assert_eq!(leader(&spec, "# # x"), Some((0, 1, 2)));
        let spec = CommentSpec::parse("n:#").unwrap();
        assert_eq!(leader(&spec, "# # x"), Some((0, 3, 4)));
    }

    #[test]
    fn first_matching_part_wins() {
        // b:# is tried before b:## and fails on "##", so b:## matches.
        let spec = CommentSpec::parse("b:#,b:##").unwrap();
        let m = spec.match_line("## doc").unwrap();
        assert_eq!((m.leader_end, m.part), (2, 1));
        // Without b, ":#" matches the first '#' of "##".
        let spec = CommentSpec::parse(":#,:##").unwrap();
        let m = spec.match_line("## doc").unwrap();
        assert_eq!((m.leader_end, m.part), (1, 0));
    }

    #[test]
    fn string_with_leading_space_needs_white_space_before() {
        let spec = CommentSpec::parse_lossy(": x");
        assert_eq!(leader(&spec, "x foo"), None);
        assert_eq!(leader(&spec, "  x foo"), Some((2, 3, 4)));
    }

    // ── Continuation ─────────────────────────────────────────────────────
    //
    // Expected strings are what Vim 9.1 produced for `oX<Esc>` with
    // fo=tcqro, ts=8, et, and the given 'comments' and 'autoindent',
    // with the typed "X" removed.

    #[test]
    fn continuation_matches_vim_noautoindent() {
        let spec = default_spec();
        for (line, want) in [
            ("// foo", "// "),
            ("  // foo", "  // "),
            ("\t//  foo", "\t//  "),
            ("# foo", "# "),
            ("#foo", ""),
            ("/* foo", " * "),
            ("  /* foo", " * "),
            ("/*foo", " * "),
            (" * foo", " * "),
            (" */", ""),
            ("> > foo", "> > "),
            (">> foo", ">> "),
            ("- foo", "  "),
            ("  - foo", "    "),
            ("% foo", "% "),
            ("XCOMM foo", "XCOMM "),
            ("x = 1 # foo", ""),
            (" *", " * "),
            ("-", " "),
        ] {
            assert_eq!(open_below(&spec, line, false), want, "{line:?}");
        }
    }

    #[test]
    fn continuation_matches_vim_other_specs() {
        for (com, line, want) in [
            ("s:/*,m:**,e:*/", "/* foo", "** "),
            ("sr:/*,m:**,e:*/", "/* foo", "** "),
            ("s2:/*,mb:*,ex:*/", "/* foo", "  *"),
            ("s-1:/*,mb:*,ex:*/", "  /* foo", "  *  "),
            ("sl:/*,mb:*,ex:*/", "/* foo", "*  "),
            ("b:##,b:#,fb:-", "## doc", "## "),
            ("b:##,b:#,fb:-", "# c", "# "),
            ("b:#,b:##", "## doc", "## "),
            (":#,:##", "## doc", "#"),
            ("n:#", "# # x", "# # "),
            (":#", "# # x", "# "),
            ("n:>,:#", "> # x", "> "),
            (":>,n:#", "> # x", "> "),
            ("fb:*", "* item", "  "),
            ("f:-", "-item", " "),
            ("://,mb:*", "* foo", ""),
            ("mb:*,://", "* foo", "* "),
            ("sr:/***,m:**,e:*/", "/*** foo", "  ** "),
            ("sr:/***,m:*,e:*/", "/*** foo", "   * "),
            ("s:/***,m:*,e:*/", "/*** foo", "*    "),
        ] {
            let spec = CommentSpec::parse(com).unwrap();
            assert_eq!(open_below(&spec, line, false), want, "{com} {line:?}");
        }
    }

    #[test]
    fn continuation_with_autoindent() {
        let spec = default_spec();
        // Vim 9.1 with 'autoindent': the start's indent is kept and the
        // offset is added on top.
        assert_eq!(open_below(&spec, "  /* foo", true), "   * ");
        assert_eq!(open_below(&spec, "  // foo", true), "  // ");
        assert_eq!(open_below(&spec, "    - foo", true), "      ");
        assert_eq!(open_below(&spec, "  /*", true), "   * ");
        let m = spec.match_line("\t// foo").unwrap();
        let c = m.continuation(&spec, "\t// foo", 7, true, 4).unwrap();
        assert_eq!((c.indent, c.leader.as_str()), (4, "// "));
        let spec = CommentSpec::parse("s1:/*,mb:*,ex:*/").unwrap();
        let m = spec.match_line("\t/* foo").unwrap();
        let c = m.continuation(&spec, "\t/* foo", 8, true, 8).unwrap();
        assert_eq!((c.indent, c.leader.as_str()), (9, "* "));
    }

    #[test]
    fn three_piece_comment_closed_on_same_line_is_not_continued() {
        let spec = default_spec();
        assert_eq!(open_below(&spec, "/* foo */", false), "");
    }

    #[test]
    fn three_piece_comment_closed_after_the_break_is_continued() {
        // Only the text that stays on the line counts: an end string that
        // moves to the new line does not stop the leader.
        let spec = default_spec();
        let line = "/* aaaa bbbb*/";
        let m = spec.match_line(line).unwrap();
        let c = m.continuation(&spec, line, 7, false, 8).unwrap();
        assert_eq!((c.indent, c.leader.as_str()), (1, "* "));
        assert_eq!(m.continuation(&spec, line, line.len(), false, 8), None);
    }

    #[test]
    fn extra_space_after_bare_start_leader() {
        let spec = default_spec();
        // "/*" alone: the middle gets a space after it.
        assert_eq!(open_below(&spec, "/*", false), " * ");
        // Breaking right after "/*" (split at the leader end) adds the
        // space too.
        let m = spec.match_line("/*foo").unwrap();
        let c = m.continuation(&spec, "/*foo", 2, false, 8).unwrap();
        assert_eq!(c.leader.as_str(), "* ");
    }
}
