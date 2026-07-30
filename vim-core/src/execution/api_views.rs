//! Domain view structs for the universal API.
//!
//! Read-only wrappers over engine internals that expose a safe, ergonomic
//! query surface. Each view borrows a single domain object and provides
//! clamped, never-panicking accessor methods.

use compact_str::CompactString;

use crate::commands::helpers;
use crate::document::Document;
use crate::execution::engine::VimEngine;
use crate::execution::host_api::VimHost;
use crate::primitives::byte_delta;
use crate::primitives::{
    MarkName, Mode, MotionType, Offset, RegisterContent, RegisterName, SearchDirection,
    SelectionShape, VarScope, VimOptions, VimValue,
};
use crate::state::{Marks, Registers, VariableStore};

// ═══════════════════════════════════════════════════════════════════════════════
// Search result types
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of a regex search within a buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch {
    /// Byte offset of the match start.
    pub start: usize,
    /// Byte offset past the match end.
    pub end: usize,
    /// The matched text.
    pub text: CompactString,
}

/// Result of an HTML/XML tag pair search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagMatch {
    /// Byte offset of the opening tag start (the `<`).
    pub open_start: usize,
    /// Byte offset past the opening tag end (past the `>`).
    pub open_end: usize,
    /// Byte offset of the closing tag start (the `<` in `</`).
    pub close_start: usize,
    /// Byte offset past the closing tag end (past the `>`).
    pub close_end: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Whether a byte is an ASCII word character (alphanumeric or underscore).
const fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ═══════════════════════════════════════════════════════════════════════════════
// BufferView
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only view over a document's text content.
///
/// Wraps `&dyn Document` and provides line/char/slice accessors that
/// clamp out-of-bounds inputs instead of panicking.
#[derive(Clone, Copy)]
pub struct BufferView<'a> {
    /// The underlying document.
    doc: &'a dyn Document,
    /// Resolved options for indent-related queries.
    options: &'a VimOptions,
}

impl<'a> BufferView<'a> {
    /// Create a new buffer view from a document reference and options.
    #[inline]
    #[must_use]
    pub fn new(doc: &'a dyn Document, options: &'a VimOptions) -> Self {
        Self { doc, options }
    }

    /// Full document text.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &'a str {
        self.doc.text()
    }

    /// Byte length of the document.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.doc.len()
    }

    /// Whether the document is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.doc.is_empty()
    }

    /// Number of lines in the document.
    #[inline]
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.doc.line_count()
    }

    /// Content of the nth line (0-indexed), including trailing `\n` if not the last line.
    ///
    /// Returns `None` if `n` is out of range.
    #[must_use]
    pub fn line(&self, n: usize) -> Option<&'a str> {
        let text = self.doc.text();
        let start = helpers::line_start(text, n)?;
        // Find end: next line's start or text end.
        let end = helpers::line_start(text, n + 1).unwrap_or(text.len());
        Some(&text[start..end])
    }

    /// Content of the line containing `offset` (without trailing newline).
    ///
    /// Clamps `offset` to document length.
    #[must_use]
    pub fn line_at(&self, offset: usize) -> &'a str {
        let text = self.doc.text();
        helpers::current_line(text, offset)
    }

    /// Line number (0-indexed) containing `offset`.
    ///
    /// Clamps `offset` to document length.
    #[inline]
    #[must_use]
    pub fn line_number(&self, offset: usize) -> usize {
        helpers::line_of(self.doc.text(), offset)
    }

    /// Byte offset of the start of line `n` (0-indexed).
    ///
    /// Returns `self.len()` for out-of-range line numbers so that
    /// `slice(line_start(n), line_end(n))` yields `""` when `n` is past
    /// the last line, rather than returning the entire document.
    #[inline]
    #[must_use]
    pub fn line_start(&self, n: usize) -> usize {
        helpers::line_start(self.doc.text(), n).unwrap_or(self.doc.len())
    }

    /// Byte offset of the end of line `n` (before `\n`).
    ///
    /// Returns `self.len()` for out-of-range line numbers.
    #[inline]
    #[must_use]
    pub fn line_end(&self, n: usize) -> usize {
        helpers::line_end(self.doc.text(), n).unwrap_or(self.doc.len())
    }

    /// Substring from `start` to `end`, clamped to document bounds.
    ///
    /// Offsets that fall inside a multi-byte UTF-8 character are snapped
    /// forward to the next character boundary so this method never panics.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> &'a str {
        let text = self.doc.text();
        let len = text.len();
        let mut s = start.min(len);
        let mut e = end.min(len);
        // Snap to char boundaries to prevent panic on mid-char offsets.
        while s < len && !text.is_char_boundary(s) {
            s += 1;
        }
        while e < len && !text.is_char_boundary(e) {
            e += 1;
        }
        if s >= e {
            return "";
        }
        &text[s..e]
    }

    /// Character at the given byte offset.
    ///
    /// Returns `None` if `offset` is out of range.
    #[inline]
    #[must_use]
    pub fn char_at(&self, offset: usize) -> Option<char> {
        helpers::char_at(self.doc.text(), offset)
    }

    /// UTF-8 byte length of the character at `offset`.
    ///
    /// If `offset` falls inside a multi-byte character, it is snapped back to
    /// the start of that character before computing the length.
    /// Returns `0` if `offset` is out of range.
    #[must_use]
    pub fn char_len_at(&self, offset: usize) -> usize {
        let text = self.doc.text();
        if offset >= text.len() {
            return 0;
        }
        // Snap to the start of the character containing this offset.
        let mut snapped = offset;
        while snapped > 0 && !text.is_char_boundary(snapped) {
            snapped -= 1;
        }
        let next = crate::primitives::text_util::next_char_boundary(text, snapped);
        next - snapped
    }

    /// Previous UTF-8 character boundary before `offset`.
    ///
    /// Returns `0` if already at the start.
    #[inline]
    #[must_use]
    pub fn prev_char_boundary(&self, offset: usize) -> usize {
        crate::primitives::text_util::prev_char_boundary(self.doc.text(), offset)
    }

    /// Next UTF-8 character boundary after `offset`.
    ///
    /// Returns `self.len()` if already at the end.
    #[inline]
    #[must_use]
    pub fn next_char_boundary(&self, offset: usize) -> usize {
        crate::primitives::text_util::next_char_boundary(self.doc.text(), offset)
    }

    /// Whether the byte at `offset` is an ASCII word character (alphanumeric or `_`).
    ///
    /// Returns `false` if `offset` is out of range.
    #[must_use]
    pub fn is_word_byte(&self, offset: usize) -> bool {
        let text = self.doc.text();
        text.as_bytes()
            .get(offset)
            .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
    }

    /// Count of leading whitespace bytes on line `n`.
    ///
    /// Returns `0` if `n` is out of range.
    #[must_use]
    pub fn line_indent(&self, n: usize) -> usize {
        let text = self.doc.text();
        let Some(start) = helpers::line_start(text, n) else {
            return 0;
        };
        let Some(end) = helpers::line_end(text, n) else {
            return 0;
        };
        let line = &text[start..end];
        line.len() - line.trim_start().len()
    }

    // ── Literal search ──────────────────────────────────────────────────

    /// Find the next occurrence of a literal string after `offset`.
    ///
    /// Returns the byte offset of the match start, or `None` if not found.
    /// Offsets beyond the document length are clamped.
    #[must_use]
    pub fn find_forward(&self, offset: usize, pattern: &str) -> Option<usize> {
        let text = self.doc.text();
        let offset = offset.min(text.len());
        text[offset..].find(pattern).map(|pos| offset + pos)
    }

    /// Find the previous occurrence of a literal string before `offset`.
    ///
    /// Returns the byte offset of the match start, or `None` if not found.
    /// Offsets beyond the document length are clamped.
    #[must_use]
    pub fn find_backward(&self, offset: usize, pattern: &str) -> Option<usize> {
        let text = self.doc.text();
        let offset = offset.min(text.len());
        text[..offset].rfind(pattern)
    }

    // ── Regex search ────────────────────────────────────────────────────

    /// Search forward from `offset` using a vim-regex pattern.
    ///
    /// Returns a [`RegexMatch`] with the start, end, and matched text,
    /// or `None` if the pattern does not match or is invalid.
    #[must_use]
    pub fn find_regex_forward(&self, offset: usize, pattern: &str) -> Option<RegexMatch> {
        let text = self.doc.text();
        let offset = offset.min(text.len());
        let regex = vim_regex::VimRegex::new(pattern).ok()?;
        // Mirrors evolve's api_views. `case_sensitive` is omitted because the
        // builder already defaults it to true, matching the old literal.
        let ctx = vim_regex::MatchContext::builder(text)
            .cursor(offset)
            .build();
        let m = regex.find_at(&ctx, offset).ok()??;
        Some(RegexMatch {
            start: m.range.start,
            end: m.range.end,
            text: CompactString::from(&text[m.range.start..m.range.end]),
        })
    }

    /// Search backward from `offset` using a vim-regex pattern.
    ///
    /// Finds the last match whose start is before `offset`.
    /// Returns `None` if the pattern does not match or is invalid.
    #[must_use]
    pub fn find_regex_backward(&self, offset: usize, pattern: &str) -> Option<RegexMatch> {
        let text = self.doc.text();
        let offset = offset.min(text.len());
        let regex = vim_regex::VimRegex::new(pattern).ok()?;
        // Evolve switches this path to the dedicated `find_backward`; the
        // find_all-and-take-last logic below is left as-is so this port changes
        // only the context construction, not the search strategy.
        let ctx = vim_regex::MatchContext::builder(&text[..offset])
            .cursor(offset)
            .build();
        // Find all matches in the prefix and take the last one.
        let matches = regex.find_all(&ctx).ok()?;
        let m = matches.last()?;
        Some(RegexMatch {
            start: m.range.start,
            end: m.range.end,
            text: CompactString::from(&text[m.range.start..m.range.end]),
        })
    }

    // ── Word / identifier ───────────────────────────────────────────────

    /// Find the word boundaries containing `offset`.
    ///
    /// A "word" is a maximal run of ASCII alphanumeric or underscore bytes.
    /// Returns `(start, end)` where `end` is one past the last word byte.
    /// Returns `None` if `offset` is out of range or not on a word character.
    #[must_use]
    pub fn word_at(&self, offset: usize) -> Option<(usize, usize)> {
        let text = self.doc.text();
        if offset >= text.len() {
            return None;
        }
        let bytes = text.as_bytes();
        if !is_word_char(bytes[offset]) {
            return None;
        }
        let start = (0..offset)
            .rev()
            .take_while(|&i| is_word_char(bytes[i]))
            .last()
            .unwrap_or(offset);
        let end = (offset..text.len())
            .take_while(|&i| is_word_char(bytes[i]))
            .last()
            .map_or(offset + 1, |i| i + 1);
        Some((start, end))
    }

    /// Find the identifier boundaries containing `offset`.
    ///
    /// In vim-core, identifiers use the same character class as words
    /// (ASCII alphanumeric and underscore). Delegates to [`word_at`](Self::word_at).
    #[must_use]
    pub fn identifier_at(&self, offset: usize) -> Option<(usize, usize)> {
        self.word_at(offset)
    }

    // ── Structural search ───────────────────────────────────────────────

    /// Find a matching bracket pair containing `offset`.
    ///
    /// Searches backward from `offset` for an unmatched `open` character,
    /// then forward from it for the matching `close`. Handles nesting.
    /// Returns `(open_pos, close_end)` where `close_end` is past the
    /// closing character.
    #[must_use]
    pub fn find_pair(&self, offset: usize, open: char, close: char) -> Option<(usize, usize)> {
        let text = self.doc.text();
        let offset = offset.min(text.len().saturating_sub(1));

        // Search backward for unmatched open.
        let mut depth = 0i32;
        let mut open_pos = None;
        for (i, ch) in text[..=offset].char_indices().rev() {
            if ch == close {
                depth += 1;
            }
            if ch == open {
                if depth == 0 {
                    open_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        let open_pos = open_pos?;

        // Search forward for matching close.
        depth = 0;
        for (i, ch) in text[open_pos..].char_indices() {
            if ch == open {
                depth += 1;
            }
            if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Some((open_pos, open_pos + i + ch.len_utf8()));
                }
            }
        }
        None
    }

    /// Find a quote pair on the current line containing `offset`.
    ///
    /// Quotes use the same character for open and close, so pairs are
    /// matched left-to-right on the line. Returns `(open_pos, close_end)`
    /// where `close_end` is past the closing quote character.
    #[must_use]
    pub fn find_quote(&self, offset: usize, quote: char) -> Option<(usize, usize)> {
        let text = self.doc.text();
        let offset = offset.min(text.len().saturating_sub(1));

        // Find line boundaries.
        let line_start = text[..offset].rfind('\n').map_or(0, |p| p + 1);
        let line_end = text[offset..].find('\n').map_or(text.len(), |p| offset + p);
        let line = &text[line_start..line_end];

        // Collect quote positions on this line.
        let mut quotes: Vec<usize> = Vec::new();
        for (i, ch) in line.char_indices() {
            if ch == quote {
                quotes.push(line_start + i);
            }
        }

        // Find the pair containing offset.
        for pair in quotes.chunks(2) {
            if pair.len() == 2 && pair[0] <= offset && offset <= pair[1] + quote.len_utf8() {
                return Some((pair[0], pair[1] + quote.len_utf8()));
            }
        }
        None
    }

    /// Find an HTML/XML tag pair surrounding `offset`.
    ///
    /// Performs a best-effort search: scans backward for `<tagname`,
    /// then forward for the matching `</tagname>`. Does not attempt
    /// full HTML parsing.
    #[must_use]
    pub fn find_tag(&self, offset: usize) -> Option<TagMatch> {
        let text = self.doc.text();
        if offset >= text.len() {
            return None;
        }

        // Search backward for an opening tag: '<' followed by a tag name.
        let before = &text[..=offset.min(text.len() - 1)];
        let mut search_pos = before.len();
        loop {
            let open_lt = before[..search_pos].rfind('<')?;
            let after_lt = &text[open_lt + 1..];

            // Skip closing tags (</...)
            if after_lt.starts_with('/') {
                search_pos = open_lt;
                continue;
            }

            // Extract tag name (sequence of alphanumeric, hyphen, colon, underscore).
            let tag_name_end = after_lt
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != ':' && c != '_')
                .unwrap_or(after_lt.len());
            if tag_name_end == 0 {
                search_pos = open_lt;
                continue;
            }
            let tag_name = &after_lt[..tag_name_end];

            // Find end of the opening tag.
            let open_end = text[open_lt..].find('>')? + open_lt + 1;

            // Skip self-closing tags.
            if text[open_lt..open_end].ends_with("/>") {
                search_pos = open_lt;
                continue;
            }

            // Search forward for matching closing tag </tagname>.
            let close_pattern = format!("</{tag_name}");
            let mut depth = 1i32;
            let mut scan = open_end;
            let open_pattern = format!("<{tag_name}");

            while scan < text.len() {
                // Check for closing tag at current position.
                if text[scan..].starts_with(&close_pattern) {
                    let rest = &text[scan + close_pattern.len()..];
                    let next_ch = rest.chars().next();
                    if next_ch == Some('>') || next_ch == Some(' ') || next_ch == Some('\t') {
                        depth -= 1;
                        if depth == 0 {
                            let close_start = scan;
                            let close_end = text[scan..].find('>').map(|p| scan + p + 1)?;
                            return Some(TagMatch {
                                open_start: open_lt,
                                open_end,
                                close_start,
                                close_end,
                            });
                        }
                    }
                }
                // Check for nested opening tag.
                if text[scan..].starts_with(&open_pattern) {
                    let rest = &text[scan + open_pattern.len()..];
                    let next_ch = rest.chars().next();
                    if next_ch == Some('>') || next_ch == Some(' ') || next_ch == Some('/') {
                        // Make sure it's not self-closing.
                        if let Some(gt) = text[scan..].find('>') {
                            if !text[scan..scan + gt].ends_with('/') {
                                depth += 1;
                            }
                        }
                    }
                }
                scan += 1;
            }

            // No matching close tag found for this open tag; try an outer one.
            search_pos = open_lt;
            continue;
        }
    }

    // ── Indentation ─────────────────────────────────────────────────────

    /// Generate an indentation string for the given nesting level.
    ///
    /// When `expandtab` is set, produces `level * shiftwidth` spaces.
    /// Otherwise produces `level` tab characters.
    #[must_use]
    pub fn indent_string(&self, level: usize) -> CompactString {
        if self.options.expandtab() {
            let spaces = level * self.options.shiftwidth();
            CompactString::from(" ".repeat(spaces))
        } else {
            CompactString::from("\t".repeat(level))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OffsetSelectionInfo
// ═══════════════════════════════════════════════════════════════════════════════

/// Offset-based selection information (anchor/head as byte offsets plus shape).
///
/// Distinct from the line/column-based `SelectionInfo` in `host_response`:
/// this carries raw byte offsets suitable for buffer-level queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffsetSelectionInfo {
    /// Anchor byte offset (where selection started).
    pub anchor: usize,
    /// Head byte offset (where cursor is).
    pub head: usize,
    /// Visual selection shape.
    pub shape: SelectionShape,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CursorView
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only view over cursor and selection state.
///
/// Wraps `&dyn VimHost` and exposes cursor position and active selection.
#[derive(Clone, Copy)]
pub struct CursorView<'a> {
    /// The underlying host.
    host: &'a dyn VimHost,
}

impl<'a> CursorView<'a> {
    /// Create a new cursor view from a host reference.
    #[inline]
    #[must_use]
    pub fn new(host: &'a dyn VimHost) -> Self {
        Self { host }
    }

    /// Current cursor byte offset.
    #[inline]
    #[must_use]
    pub fn offset(&self) -> usize {
        self.host.cursor_offset()
    }

    /// Current cursor position as `(line, column)` (0-indexed).
    ///
    /// Returns `None` if the offset cannot be translated.
    #[must_use]
    pub fn position(&self) -> Option<crate::primitives::Position> {
        self.host
            .offset_to_pos(Offset::new(self.host.cursor_offset()))
    }

    /// Active selection range, if any.
    ///
    /// Returns `None` when not in a visual/select mode or host has no selection.
    #[must_use]
    pub fn selection(&self) -> Option<crate::primitives::SelectionRange> {
        self.host.selection()
    }

    /// Current viewport information (visible line range, dimensions).
    #[inline]
    #[must_use]
    pub fn viewport(&self) -> crate::dispatch::ViewportInfo {
        self.host.viewport()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RegisterView
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only view over the register file.
///
/// Wraps `&Registers` and provides register content access by name.
#[derive(Clone, Copy)]
pub struct RegisterView<'a> {
    /// The register file.
    registers: &'a Registers,
}

impl<'a> RegisterView<'a> {
    /// Create a new register view.
    #[inline]
    #[must_use]
    pub const fn new(registers: &'a Registers) -> Self {
        Self { registers }
    }

    /// Get register content by name character (e.g., `'a'`, `'"'`).
    ///
    /// Returns `None` if the character is not a valid register name or the
    /// register is empty.
    #[must_use]
    pub fn get(&self, name: char) -> Option<&'a str> {
        let rn = RegisterName::new(name)?;
        self.registers.get(rn).map(RegisterContent::text)
    }

    /// Get register content and its motion type.
    ///
    /// Returns `None` if the character is not a valid register name or the
    /// register is empty.
    #[must_use]
    pub fn get_with_type(&self, name: char) -> Option<(&'a str, MotionType)> {
        let rn = RegisterName::new(name)?;
        self.registers.get(rn).map(|c| (c.text(), c.motion_type()))
    }

    /// Get register text content by name (alias for [`get`](Self::get)).
    #[inline]
    #[must_use]
    pub fn text(&self, name: char) -> Option<&'a str> {
        self.get(name)
    }

    /// Get the motion type of a register's content.
    ///
    /// Returns `None` if the character is not a valid register name or the
    /// register is empty.
    #[must_use]
    pub fn motion_type(&self, name: char) -> Option<MotionType> {
        RegisterName::new(name)
            .and_then(|rn| self.registers.get(rn))
            .map(RegisterContent::motion_type)
    }

    /// Whether a register is empty or unset.
    #[must_use]
    pub fn is_empty(&self, name: char) -> bool {
        self.get(name).is_none_or(str::is_empty)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MarkView
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only view over mark storage.
///
/// Wraps `&Marks` and provides mark position access by name.
#[derive(Clone, Copy)]
pub struct MarkView<'a> {
    /// The mark storage.
    marks: &'a Marks,
}

impl<'a> MarkView<'a> {
    /// Create a new mark view.
    #[inline]
    #[must_use]
    pub const fn new(marks: &'a Marks) -> Self {
        Self { marks }
    }

    /// Get the byte offset of a mark by name character (e.g., `'a'`, `'<'`).
    ///
    /// Returns `None` if the character is not a valid mark name or the mark
    /// is not set.
    #[must_use]
    pub fn get(&self, name: char) -> Option<usize> {
        let mn = MarkName::new(name)?;
        self.marks.get(mn).map(|m| m.offset().get())
    }

    /// Whether a mark with the given name is currently set.
    #[inline]
    #[must_use]
    pub fn is_set(&self, name: char) -> bool {
        self.get(name).is_some()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OptionView
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only view over effective Vim options.
///
/// Wraps `&VimOptions` and exposes commonly queried settings.
#[derive(Clone, Copy)]
pub struct OptionView<'a> {
    /// The resolved options.
    options: &'a VimOptions,
}

impl<'a> OptionView<'a> {
    /// Create a new option view.
    #[inline]
    #[must_use]
    pub const fn new(options: &'a VimOptions) -> Self {
        Self { options }
    }

    /// Tab stop width in columns.
    #[inline]
    #[must_use]
    pub const fn tabstop(&self) -> usize {
        self.options.tabstop()
    }

    /// Shift width for indent/outdent.
    #[inline]
    #[must_use]
    pub const fn shiftwidth(&self) -> usize {
        self.options.shiftwidth()
    }

    /// Whether tabs are expanded to spaces.
    #[inline]
    #[must_use]
    pub const fn expandtab(&self) -> bool {
        self.options.expandtab()
    }

    /// Whether case is ignored in searches.
    #[inline]
    #[must_use]
    pub const fn ignorecase(&self) -> bool {
        self.options.ignorecase()
    }

    /// Whether uppercase chars override ignorecase.
    #[inline]
    #[must_use]
    pub const fn smartcase(&self) -> bool {
        self.options.smartcase()
    }

    /// Maximum line width for formatting.
    #[inline]
    #[must_use]
    pub const fn textwidth(&self) -> usize {
        self.options.textwidth()
    }

    /// Comment string format (e.g., `"// %s"`).
    #[inline]
    #[must_use]
    pub fn commentstring(&self) -> &str {
        self.options.commentstring()
    }

    /// Characters that form keywords (Vim's `iskeyword` option).
    #[inline]
    #[must_use]
    pub fn iskeyword(&self) -> &str {
        self.options.iskeyword()
    }

    /// Look up any option by name, returning its value as a [`VimValue`].
    ///
    /// Supports: `shiftwidth`, `tabstop`, `expandtab`, `ignorecase`,
    /// `smartcase`, `textwidth`, `commentstring`, `iskeyword`.
    /// Returns `None` for unrecognized option names.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<crate::primitives::VimValue> {
        use crate::primitives::VimValue;
        match name {
            "shiftwidth" | "sw" => Some(VimValue::Int(byte_delta::to_i64(self.shiftwidth()))),
            "tabstop" | "ts" => Some(VimValue::Int(byte_delta::to_i64(self.tabstop()))),
            "softtabstop" | "sts" => Some(VimValue::Int(i64::from(self.options.softtabstop()))),
            "expandtab" | "et" => Some(VimValue::Bool(self.expandtab())),
            "ignorecase" | "ic" => Some(VimValue::Bool(self.ignorecase())),
            "smartcase" | "scs" => Some(VimValue::Bool(self.smartcase())),
            "textwidth" | "tw" => Some(VimValue::Int(byte_delta::to_i64(self.textwidth()))),
            "commentstring" | "cms" => {
                Some(VimValue::String(CompactString::from(self.commentstring())))
            }
            "iskeyword" | "isk" => Some(VimValue::String(CompactString::from(self.iskeyword()))),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// StateView
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only view over engine state.
///
/// Wraps `&VimEngine` and exposes mode, search, macro recording, and
/// sub-views for registers, marks, and options.
#[derive(Clone, Copy)]
pub struct StateView<'a> {
    /// The engine.
    engine: &'a VimEngine,
}

impl<'a> StateView<'a> {
    /// Create a new state view.
    #[inline]
    #[must_use]
    pub const fn new(engine: &'a VimEngine) -> Self {
        Self { engine }
    }

    /// Current editing mode.
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.engine.state().mode()
    }

    /// Current search pattern, if any.
    #[must_use]
    pub fn search_pattern(&self) -> Option<&str> {
        self.engine.state().search().pattern()
    }

    /// Current search direction.
    #[inline]
    #[must_use]
    pub const fn search_direction(&self) -> SearchDirection {
        self.engine.state().search().direction()
    }

    /// Whether a macro is currently being recorded.
    #[inline]
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.engine.state().macros().is_recording()
    }

    /// View over the register file.
    #[inline]
    #[must_use]
    pub const fn registers(&self) -> RegisterView<'a> {
        RegisterView::new(self.engine.state().registers())
    }

    /// View over mark storage.
    #[inline]
    #[must_use]
    pub const fn marks(&self) -> MarkView<'a> {
        MarkView::new(self.engine.state().marks())
    }

    /// The register currently being recorded into, if any.
    #[inline]
    #[must_use]
    pub fn recording_register(&self) -> Option<char> {
        self.engine
            .state()
            .macros()
            .recording_register()
            .map(RegisterName::char)
    }

    /// The ID of the currently active buffer, if one is set.
    #[inline]
    #[must_use]
    pub const fn buffer_id(&self) -> Option<crate::primitives::BufferId> {
        self.engine.state().current_buffer_id()
    }

    /// The last ex command executed, if any.
    #[must_use]
    pub fn last_command(&self) -> Option<&str> {
        self.engine
            .state()
            .command_line()
            .ex_history()
            .back()
            .map(CompactString::as_str)
    }

    /// View over effective options.
    #[inline]
    #[must_use]
    pub const fn options(&self) -> OptionView<'a> {
        OptionView::new(self.engine.resolved_options())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VariableView
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only view over the variable store.
///
/// Provides scope-aware access to g: (global) and b: (buffer-local) variables
/// through the universal API layer.
#[derive(Clone, Copy)]
pub struct VariableView<'a> {
    /// The underlying variable store.
    store: &'a VariableStore,
}

impl<'a> VariableView<'a> {
    /// Create a new variable view from a store reference.
    #[inline]
    #[must_use]
    pub const fn new(store: &'a VariableStore) -> Self {
        Self { store }
    }

    /// Get a global (g:) variable by name.
    #[inline]
    #[must_use]
    pub fn get_global(&self, name: &str) -> Option<&'a VimValue> {
        self.store.get(VarScope::Global, name)
    }

    /// Get a buffer-local (b:) variable by name.
    #[inline]
    #[must_use]
    pub fn get_buffer(&self, name: &str) -> Option<&'a VimValue> {
        self.store.get(VarScope::Buffer, name)
    }

    /// Get a variable by scope and name.
    #[inline]
    #[must_use]
    pub fn get(&self, scope: VarScope, name: &str) -> Option<&'a VimValue> {
        self.store.get(scope, name)
    }

    /// Check whether a variable exists in the given scope.
    #[inline]
    #[must_use]
    pub fn exists(&self, scope: VarScope, name: &str) -> bool {
        self.store.exists(scope, name)
    }

    /// List all variables in the given scope as (name, value) pairs.
    #[must_use]
    pub fn list(&self, scope: VarScope) -> Vec<(&'a str, &'a VimValue)> {
        self.store.list(scope).collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MultiBufferView
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only view over multiple buffers (capability-gated).
///
/// Only available when the host implements [`BufferScope`](crate::document::BufferScope)
/// and declares [`HostCapability::MultiBuffer`](crate::execution::host_api::HostCapability::MultiBuffer).
/// Access via [`VimApi::buffers()`](crate::execution::api::VimApi::buffers).
#[derive(Clone, Copy)]
pub struct MultiBufferView<'a> {
    scope: &'a dyn crate::document::BufferScope,
}

impl<'a> MultiBufferView<'a> {
    /// Create a new multi-buffer view from a buffer scope reference.
    #[inline]
    #[must_use]
    pub fn new(scope: &'a dyn crate::document::BufferScope) -> Self {
        Self { scope }
    }

    /// List all open buffer IDs.
    #[inline]
    #[must_use]
    pub fn ids(&self) -> &[crate::primitives::BufferId] {
        self.scope.buffer_ids()
    }

    /// Get metadata for a specific buffer.
    #[must_use]
    pub fn meta(&self, id: crate::primitives::BufferId) -> Option<crate::document::BufferMeta> {
        self.scope.buffer_meta(id)
    }

    /// Get a read-only lens into a specific buffer's document.
    #[must_use]
    pub fn get(&self, id: crate::primitives::BufferId) -> Option<crate::document::BufferLens<'a>> {
        self.scope.buffer_lens(id)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::SimpleDocument;

    /// Helper: construct a `BufferView` with default options.
    fn bv(doc: &SimpleDocument) -> BufferView<'_> {
        // Leak a default VimOptions so we get a `&'static VimOptions`.
        // This is fine in tests.
        let opts: &VimOptions = Box::leak(Box::new(VimOptions::default()));
        BufferView::new(doc, opts)
    }

    // ── BufferView: basic text/len/line_count ─────────────────────────────

    #[test]
    fn buffer_view_basic() {
        let doc = SimpleDocument::new("hello\nworld");
        let bv = bv(&doc);
        assert_eq!(bv.text(), "hello\nworld");
        assert_eq!(bv.len(), 11);
        assert!(!bv.is_empty());
        assert_eq!(bv.line_count(), 2);
    }

    #[test]
    fn buffer_view_empty() {
        let doc = SimpleDocument::new("");
        let bv = bv(&doc);
        assert_eq!(bv.text(), "");
        assert_eq!(bv.len(), 0);
        assert!(bv.is_empty());
        // SimpleDocument treats empty as 1 line (Vim semantics).
        assert_eq!(bv.line_count(), 1);
    }

    // ── BufferView: line access ──────────────────────────────────────────

    #[test]
    fn buffer_view_line() {
        let doc = SimpleDocument::new("aaa\nbbb\nccc");
        let bv = bv(&doc);
        // First line includes trailing \n.
        assert_eq!(bv.line(0), Some("aaa\n"));
        assert_eq!(bv.line(1), Some("bbb\n"));
        // Last line has no trailing \n.
        assert_eq!(bv.line(2), Some("ccc"));
        assert_eq!(bv.line(3), None);
    }

    #[test]
    fn buffer_view_line_at() {
        let doc = SimpleDocument::new("aaa\nbbb\nccc");
        let bv = bv(&doc);
        assert_eq!(bv.line_at(0), "aaa");
        assert_eq!(bv.line_at(4), "bbb");
        assert_eq!(bv.line_at(8), "ccc");
    }

    #[test]
    fn buffer_view_line_number() {
        let doc = SimpleDocument::new("aaa\nbbb\nccc");
        let bv = bv(&doc);
        assert_eq!(bv.line_number(0), 0);
        assert_eq!(bv.line_number(3), 0); // at the \n
        assert_eq!(bv.line_number(4), 1);
        assert_eq!(bv.line_number(8), 2);
    }

    #[test]
    fn buffer_view_line_start_end() {
        let doc = SimpleDocument::new("aaa\nbbb\nccc");
        let bv = bv(&doc);
        assert_eq!(bv.line_start(0), 0);
        assert_eq!(bv.line_start(1), 4);
        assert_eq!(bv.line_start(2), 8);
        assert_eq!(bv.line_end(0), 3);
        assert_eq!(bv.line_end(1), 7);
        assert_eq!(bv.line_end(2), 11);
    }

    // ── BufferView: slice with clamping ──────────────────────────────────

    #[test]
    fn buffer_view_slice_normal() {
        let doc = SimpleDocument::new("hello world");
        let bv = bv(&doc);
        assert_eq!(bv.slice(0, 5), "hello");
        assert_eq!(bv.slice(6, 11), "world");
    }

    #[test]
    fn buffer_view_slice_clamped() {
        let doc = SimpleDocument::new("hello");
        let bv = bv(&doc);
        // end beyond length.
        assert_eq!(bv.slice(0, 100), "hello");
        // start beyond length.
        assert_eq!(bv.slice(100, 200), "");
        // start == end.
        assert_eq!(bv.slice(3, 3), "");
        // start > end.
        assert_eq!(bv.slice(5, 2), "");
    }

    // ── BufferView: char operations on UTF-8 ─────────────────────────────

    #[test]
    fn buffer_view_char_at_ascii() {
        let doc = SimpleDocument::new("hello");
        let bv = bv(&doc);
        assert_eq!(bv.char_at(0), Some('h'));
        assert_eq!(bv.char_at(4), Some('o'));
        assert_eq!(bv.char_at(5), None);
    }

    #[test]
    fn buffer_view_char_at_utf8() {
        // 'é' is 2 bytes (U+00E9), '日' is 3 bytes (U+65E5).
        let doc = SimpleDocument::new("é日");
        let bv = bv(&doc);
        assert_eq!(bv.char_at(0), Some('é'));
        assert_eq!(bv.char_len_at(0), 2);
        assert_eq!(bv.char_at(2), Some('日'));
        assert_eq!(bv.char_len_at(2), 3);
        assert_eq!(bv.char_len_at(5), 0); // past end
    }

    #[test]
    fn buffer_view_char_boundaries() {
        let doc = SimpleDocument::new("aé");
        let bv = bv(&doc);
        // 'a' at 0, 'é' at 1..3.
        assert_eq!(bv.next_char_boundary(0), 1);
        assert_eq!(bv.next_char_boundary(1), 3);
        assert_eq!(bv.prev_char_boundary(3), 1);
        assert_eq!(bv.prev_char_boundary(1), 0);
    }

    #[test]
    fn buffer_view_is_word_byte() {
        let doc = SimpleDocument::new("a_1 !");
        let bv = bv(&doc);
        assert!(bv.is_word_byte(0)); // 'a'
        assert!(bv.is_word_byte(1)); // '_'
        assert!(bv.is_word_byte(2)); // '1'
        assert!(!bv.is_word_byte(3)); // ' '
        assert!(!bv.is_word_byte(4)); // '!'
        assert!(!bv.is_word_byte(99)); // out of range
    }

    #[test]
    fn buffer_view_line_indent() {
        let doc = SimpleDocument::new("  hello\n\tworld\nno_indent");
        let bv = bv(&doc);
        assert_eq!(bv.line_indent(0), 2);
        assert_eq!(bv.line_indent(1), 1);
        assert_eq!(bv.line_indent(2), 0);
        assert_eq!(bv.line_indent(99), 0); // out of range
    }

    // ── BufferView: find_forward / find_backward ────────────────────────

    #[test]
    fn find_forward_basic() {
        let doc = SimpleDocument::new("hello world hello");
        let bv = bv(&doc);
        assert_eq!(bv.find_forward(0, "world"), Some(6));
        assert_eq!(bv.find_forward(7, "hello"), Some(12));
        assert_eq!(bv.find_forward(0, "hello"), Some(0));
    }

    #[test]
    fn find_forward_not_found() {
        let doc = SimpleDocument::new("hello world");
        let bv = bv(&doc);
        assert_eq!(bv.find_forward(0, "xyz"), None);
        assert_eq!(bv.find_forward(100, "hello"), None);
    }

    #[test]
    fn find_backward_basic() {
        let doc = SimpleDocument::new("hello world hello");
        let bv = bv(&doc);
        assert_eq!(bv.find_backward(17, "hello"), Some(12));
        assert_eq!(bv.find_backward(12, "hello"), Some(0));
        assert_eq!(bv.find_backward(17, "world"), Some(6));
    }

    // ── BufferView: regex search ────────────────────────────────────────

    #[test]
    fn find_regex_forward_basic() {
        let doc = SimpleDocument::new("foo 123 bar 456");
        let bv = bv(&doc);
        let m = bv.find_regex_forward(0, "\\d\\+").unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert_eq!(m.text.as_str(), "123");
    }

    #[test]
    fn find_regex_forward_from_offset() {
        let doc = SimpleDocument::new("foo 123 bar 456");
        let bv = bv(&doc);
        let m = bv.find_regex_forward(8, "\\d\\+").unwrap();
        assert_eq!(m.start, 12);
        assert_eq!(m.text.as_str(), "456");
    }

    #[test]
    fn find_regex_backward_basic() {
        let doc = SimpleDocument::new("foo 123 bar 456");
        let bv = bv(&doc);
        let m = bv.find_regex_backward(15, "\\d\\+").unwrap();
        assert_eq!(m.start, 12);
        assert_eq!(m.text.as_str(), "456");
    }

    #[test]
    fn find_regex_backward_first_match() {
        let doc = SimpleDocument::new("foo 123 bar 456");
        let bv = bv(&doc);
        let m = bv.find_regex_backward(8, "\\d\\+").unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.text.as_str(), "123");
    }

    // ── BufferView: word_at ─────────────────────────────────────────────

    #[test]
    fn word_at_basic() {
        let doc = SimpleDocument::new("hello_world foo");
        let bv = bv(&doc);
        assert_eq!(bv.word_at(0), Some((0, 11)));
        assert_eq!(bv.word_at(5), Some((0, 11)));
        assert_eq!(bv.word_at(12), Some((12, 15)));
    }

    #[test]
    fn word_at_non_word() {
        let doc = SimpleDocument::new("hello world");
        let bv = bv(&doc);
        assert_eq!(bv.word_at(5), None); // space
        assert_eq!(bv.word_at(100), None); // out of range
    }

    #[test]
    fn identifier_at_delegates() {
        let doc = SimpleDocument::new("my_var = 42");
        let bv = bv(&doc);
        assert_eq!(bv.identifier_at(0), Some((0, 6)));
        assert_eq!(bv.identifier_at(7), None); // '='
    }

    // ── BufferView: find_pair ───────────────────────────────────────────

    #[test]
    fn find_pair_basic() {
        let doc = SimpleDocument::new("(hello)");
        let bv = bv(&doc);
        assert_eq!(bv.find_pair(3, '(', ')'), Some((0, 7)));
    }

    #[test]
    fn find_pair_nested() {
        let doc = SimpleDocument::new("(a (b) c)");
        let bv = bv(&doc);
        // From inside the inner parens.
        assert_eq!(bv.find_pair(4, '(', ')'), Some((3, 6)));
        // From outside the inner parens but inside outer.
        assert_eq!(bv.find_pair(7, '(', ')'), Some((0, 9)));
    }

    // ── BufferView: find_quote ──────────────────────────────────────────

    #[test]
    fn find_quote_basic() {
        let doc = SimpleDocument::new("say 'hello' end");
        let bv = bv(&doc);
        assert_eq!(bv.find_quote(6, '\''), Some((4, 11)));
    }

    #[test]
    fn find_quote_not_found() {
        let doc = SimpleDocument::new("no quotes here");
        let bv = bv(&doc);
        assert_eq!(bv.find_quote(3, '"'), None);
    }

    // ── BufferView: find_tag ────────────────────────────────────────────

    #[test]
    fn find_tag_basic() {
        let doc = SimpleDocument::new("<div>hello</div>");
        let bv = bv(&doc);
        let tag = bv.find_tag(6).unwrap();
        assert_eq!(tag.open_start, 0);
        assert_eq!(tag.open_end, 5);
        assert_eq!(tag.close_start, 10);
        assert_eq!(tag.close_end, 16);
    }

    #[test]
    fn find_tag_not_found() {
        let doc = SimpleDocument::new("no tags here");
        let bv = bv(&doc);
        assert!(bv.find_tag(3).is_none());
    }

    // ── BufferView: indent_string ───────────────────────────────────────

    #[test]
    fn indent_string_spaces() {
        let doc = SimpleDocument::new("");
        // Default VimOptions has expandtab=true, shiftwidth=4.
        let opts = VimOptions::default();
        let bv = BufferView::new(&doc, &opts);
        let indent = bv.indent_string(2);
        assert_eq!(indent.len(), 2 * opts.shiftwidth());
        assert!(indent.chars().all(|c| c == ' '));
    }

    #[test]
    fn indent_string_tabs() {
        let doc = SimpleDocument::new("");
        let mut opts = VimOptions::default();
        opts.set_expandtab(false);
        let bv = BufferView::new(&doc, &opts);
        assert_eq!(bv.indent_string(3).as_str(), "\t\t\t");
    }
}
