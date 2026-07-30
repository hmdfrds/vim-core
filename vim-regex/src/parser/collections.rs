//! Collection (`[...]`) parsing for the Vim regex parser.
//!
//! Handles `[abc]`, `[^abc]`, `[a-z]`, `[\d]`, `[]]`, `[-abc]`, `[abc-]`,
//! and `\_[abc]` (collection with newline).

use compact_str::CompactString;

use crate::common::MAX_COLLECTION_DEPTH;
use crate::ir::{CollectionItem, PosixClassName};

use super::{char_to_class, Parser, VimPatternNode, VimRegexError, VimRegexErrorKind};

// ═══════════════════════════════════════════════════════════════════════════════
// COLLECTION PARSING
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse a `[...]` collection.
    ///
    /// Expects the current position to be at `[`.
    pub(super) fn parse_collection(&mut self) -> Result<VimPatternNode, VimRegexError> {
        self.parse_collection_inner(false)
    }

    /// Parse a `\_[...]` collection (includes newline).
    ///
    /// Expects the current position to be at `[`.
    pub(super) fn parse_collection_with_newline(
        &mut self,
    ) -> Result<VimPatternNode, VimRegexError> {
        self.parse_collection_inner(true)
    }

    /// Core collection parser, shared by `parse_collection` and
    /// `parse_collection_with_newline`.
    fn parse_collection_inner(
        &mut self,
        include_newline: bool,
    ) -> Result<VimPatternNode, VimRegexError> {
        let open_pos = self.pos;

        if self.collection_depth >= MAX_COLLECTION_DEPTH {
            return Err(VimRegexErrorKind::PatternTooComplex {
                span: Some(self.span_at(open_pos)),
                detail: "collection nesting too deep (maximum depth is 8)".into(),
            }
            .into());
        }

        self.collection_depth += 1;
        self.advance(); // consume '['

        // Check for negation
        let negated = self.peek() == Some('^');
        if negated {
            self.advance();
        }

        let items = self.parse_collection_items(open_pos)?;

        self.collection_depth -= 1;

        Ok(VimPatternNode::Collection {
            negated,
            items,
            include_newline,
        })
    }

    /// Parse the items inside a collection until `]` is found.
    fn parse_collection_items(
        &mut self,
        open_pos: usize,
    ) -> Result<Vec<CollectionItem>, VimRegexError> {
        let mut items = Vec::new();

        // Special case: `]` or `-` as first character is literal
        if self.peek() == Some(']') {
            self.advance();
            items.push(CollectionItem::Single(']'));
        } else if self.peek() == Some('-') {
            self.advance();
            items.push(CollectionItem::Single('-'));
        }

        while let Some(ch) = self.peek() {
            if ch == ']' {
                self.advance(); // consume ']'
                return Ok(items);
            }

            self.parse_one_collection_item(&mut items, open_pos)?;
        }

        // Reached end of input without finding `]`
        Err(VimRegexErrorKind::UnterminatedCollection {
            span: self.span_from(open_pos),
        }
        .into())
    }

    /// Parse one item (or range) inside a collection.
    fn parse_one_collection_item(
        &mut self,
        items: &mut Vec<CollectionItem>,
        open_pos: usize,
    ) -> Result<(), VimRegexError> {
        // Check for POSIX named class: `[:name:]`
        if self.peek() == Some('[') && self.peek_at(1) == Some(':') {
            if let Some(item) = self.try_parse_posix_class(open_pos)? {
                items.push(item);
                return Ok(());
            }
        }

        // Check for POSIX equivalence class: `=c=` (inside the enclosing `[...]`).
        //
        // Vim's equivalence-class syntax is `[=c=]` where the outer `[` and `]`
        // are the collection delimiters.  Inside the collection the parser sees
        // the opening `=`, a single character, and the closing `=`; the next
        // character is the outer `]` that ends the collection.  We recognise
        // this as `=c=` where the current character is `=`.
        //
        // Vim treats equivalence classes as literal character `c`
        // (locale-independent no-op).
        if self.peek() == Some('=') {
            if let Some(item) = self.try_parse_equiv_class()? {
                items.push(item);
                return Ok(());
            }
        }

        // Check for POSIX collating element: `.c.` (inside the enclosing `[...]`).
        //
        // Same structure as equivalence classes above but with `.` as delimiter.
        // Vim treats collating elements as literal character `c`.
        if self.peek() == Some('.') {
            if let Some(item) = self.try_parse_collating_element()? {
                items.push(item);
                return Ok(());
            }
        }

        let ch = self.peek().ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::UnterminatedCollection {
                span: self.span_from(open_pos),
            })
        })?;

        if ch == '\\' {
            let item = self.parse_collection_escape(open_pos)?;
            // If the escape produced a single character, it can be the start of
            // a range (e.g. `[\t-z]`).  Non-character items (classes, newline)
            // are pushed directly — they can't be range endpoints.
            let range_start = match item {
                CollectionItem::Single(c) => Some(c),
                _ => {
                    items.push(item);
                    return Ok(());
                }
            };
            // Fall through to range detection with the resolved char.
            #[allow(clippy::unwrap_used, reason = "range_start is always Some here")]
            let ch = range_start.unwrap();
            self.parse_collection_item_with_range_check(items, ch, open_pos)
        } else {
            self.advance();
            self.parse_collection_item_with_range_check(items, ch, open_pos)
        }
    }

    /// Check if `ch` is the start of a range (`ch`-`end`) and push accordingly.
    fn parse_collection_item_with_range_check(
        &mut self,
        items: &mut Vec<CollectionItem>,
        ch: char,
        open_pos: usize,
    ) -> Result<(), VimRegexError> {
        // Check if this is the start of a range: `a-z`
        if self.peek() == Some('-') && self.peek_at(1) != Some(']') && self.peek_at(1).is_some() {
            self.advance(); // consume '-'
            let end_ch = self.parse_collection_range_end(open_pos)?;
            if end_ch < ch {
                return Err(VimRegexErrorKind::InvalidCollectionRange {
                    span: self.span_from(open_pos),
                    detail: CompactString::from(format!(
                        "range end '{}' is less than start '{}'",
                        end_ch, ch
                    )),
                }
                .into());
            }
            items.push(CollectionItem::Range(ch, end_ch));
        } else {
            items.push(CollectionItem::Single(ch));
        }

        Ok(())
    }

    /// Parse the end character of a collection range.
    ///
    /// Handles both literal characters and escape sequences (e.g. `\t`, `\x20`).
    fn parse_collection_range_end(&mut self, open_pos: usize) -> Result<char, VimRegexError> {
        if self.peek() == Some('\\') {
            let item = self.parse_collection_escape(open_pos)?;
            match item {
                CollectionItem::Single(c) => Ok(c),
                _ => Err(VimRegexErrorKind::InvalidCollectionRange {
                    span: self.span_from(open_pos),
                    detail: CompactString::from(
                        "range endpoint cannot be a character class or newline",
                    ),
                }
                .into()),
            }
        } else {
            self.advance().ok_or_else(|| {
                VimRegexError::from(VimRegexErrorKind::UnterminatedCollection {
                    span: self.span_from(open_pos),
                })
            })
        }
    }

    /// Try to parse a POSIX named class `[:name:]` at the current position.
    ///
    /// Returns `Ok(Some(item))` if a valid POSIX class was parsed and consumed,
    /// `Ok(None)` if the input does not start with `[:` (caller falls through),
    /// or `Err` if `[:` was found but the class name is invalid or unclosed.
    fn try_parse_posix_class(
        &mut self,
        open_pos: usize,
    ) -> Result<Option<CollectionItem>, VimRegexError> {
        // We already know peek() == '[' and peek_at(1) == ':'
        // Scan ahead to find the closing `:]`
        let start = self.pos + 2; // skip '[' and ':'
        let mut end = start;
        loop {
            let a = self.input.get(end).copied();
            let b = self.input.get(end + 1).copied();
            match (a, b) {
                (Some(':'), Some(']')) => break,
                (None, _) | (_, None) => {
                    // No closing `:]` found — treat `[` as a literal character,
                    // return `None` so the caller falls through.
                    return Ok(None);
                }
                _ => end += 1,
            }
        }

        // The loop above either advances `end` by 1 each step until `(':',']')`
        // is found or returns `Ok(None)` when the input is exhausted, so
        // `start..end` is always a valid sub-slice of `self.input` here.
        #[expect(
            clippy::indexing_slicing,
            reason = "loop only `break`s when the [end, end+1] pair was successfully read; start..end is in bounds by construction"
        )]
        let name_chars: String = self.input[start..end].iter().collect();
        let class = match name_chars.as_str() {
            "alnum" => PosixClassName::Alnum,
            "alpha" => PosixClassName::Alpha,
            "blank" => PosixClassName::Blank,
            "cntrl" => PosixClassName::Cntrl,
            "digit" => PosixClassName::Digit,
            "graph" => PosixClassName::Graph,
            "lower" => PosixClassName::Lower,
            "print" => PosixClassName::Print,
            "punct" => PosixClassName::Punct,
            "space" => PosixClassName::Space,
            "upper" => PosixClassName::Upper,
            "xdigit" => PosixClassName::Xdigit,
            "tab" => PosixClassName::Tab,
            "return" => PosixClassName::Return,
            "backspace" => PosixClassName::Backspace,
            "escape" => PosixClassName::Escape,
            "ident" => PosixClassName::Ident,
            "keyword" => PosixClassName::Keyword,
            "fname" => PosixClassName::Fname,
            _ => {
                return Err(VimRegexErrorKind::UnknownPosixClass {
                    span: self.span_from(open_pos),
                    name: CompactString::from(name_chars.as_str()),
                }
                .into())
            }
        };

        // Advance past `[:name:]` (that is end - self.pos + 2 chars for the closing `:]`)
        self.pos = end + 2; // end points at ':', end+1 is ']', end+2 is past
        Ok(Some(CollectionItem::PosixClass(class)))
    }

    /// Parse a `\` escape inside a collection.
    ///
    /// Handles character classes (`\d`, `\w`, etc.), escape sequences
    /// (`\n`, `\t`, `\r`, `\e`, `\b`), character codes (`\d123`, `\o40`,
    /// `\x20`, `a`, `\U0001F600`), and literal escapes.
    ///
    /// Returns the parsed `CollectionItem` so the caller can feed single-char
    /// items through range detection (e.g. `[\t-z]`).
    fn parse_collection_escape(
        &mut self,
        open_pos: usize,
    ) -> Result<CollectionItem, VimRegexError> {
        self.advance(); // consume '\'

        let ch = self.peek().ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::UnterminatedCollection {
                span: self.span_from(open_pos),
            })
        })?;

        // Character codes: \d followed by digits means decimal char code
        // (Only when 'd' is followed by at least one digit)
        // Similarly for \o (octal), \x (hex), \u (4-digit hex), \U (8-digit hex)
        if self.is_collection_char_code_start(ch) {
            if let Some(item) = self.try_parse_collection_char_code(ch, open_pos)? {
                return Ok(item);
            }
        }

        // Character classes inside collections (only reached when NOT a char code)
        if let Some(class) = char_to_class(ch) {
            self.advance();
            return Ok(CollectionItem::Class(class));
        }

        // Escape sequences: \n is special in collections (matches newline)
        if ch == 'n' {
            self.advance();
            return Ok(CollectionItem::Newline);
        }

        // Escape sequences: \t, \r, \e, \b
        let escape_char = match ch {
            't' => Some('\t'),
            'r' => Some('\r'),
            'e' => Some('\x1B'),
            'b' => Some('\x08'),
            _ => None,
        };
        if let Some(c) = escape_char {
            self.advance();
            return Ok(CollectionItem::Single(c));
        }

        // Other escapes: literal
        self.advance();
        Ok(CollectionItem::Single(ch))
    }

    /// Check if `ch` (the character after `\` in a collection) could start a
    /// character code escape, given the following character is a valid digit
    /// for the respective radix.
    fn is_collection_char_code_start(&self, ch: char) -> bool {
        match ch {
            'd' => self.peek_at(1).is_some_and(|c| c.is_ascii_digit()),
            'o' => self
                .peek_at(1)
                .is_some_and(|c| c.is_ascii_digit() && c <= '7'),
            'x' => self.peek_at(1).is_some_and(|c| c.is_ascii_hexdigit()),
            'u' => self.peek_at(1).is_some_and(|c| c.is_ascii_hexdigit()),
            'U' => self.peek_at(1).is_some_and(|c| c.is_ascii_hexdigit()),
            _ => false,
        }
    }

    /// Try to parse a character code escape inside a collection.
    ///
    /// Handles `\d123` (decimal), `\o40` (octal), `\x20` (2-digit hex),
    /// `a` (4-digit hex), `\U0001F600` (8-digit hex).
    ///
    /// Returns `Ok(Some(item))` if recognized, `Ok(None)` to fall through.
    fn try_parse_collection_char_code(
        &mut self,
        ch: char,
        open_pos: usize,
    ) -> Result<Option<CollectionItem>, VimRegexError> {
        match ch {
            'd' => {
                self.advance(); // consume 'd'
                let code = self.parse_decimal_number();
                let c = char::from_u32(code).ok_or_else(|| {
                    VimRegexError::from(VimRegexErrorKind::InvalidCollectionCharCode {
                        span: self.span_from(open_pos),
                        detail: CompactString::from(format!(
                            "\\d{code} (decimal {code}) is not a valid Unicode code point"
                        )),
                    })
                })?;
                Ok(Some(CollectionItem::Single(c)))
            }
            'o' => {
                self.advance(); // consume 'o'
                let code = self.parse_octal_number();
                let c = char::from_u32(code).ok_or_else(|| {
                    VimRegexError::from(VimRegexErrorKind::InvalidCollectionCharCode {
                        span: self.span_from(open_pos),
                        detail: CompactString::from(format!(
                            "\\o{code:o} (octal, value {code}) is not a valid Unicode code point"
                        )),
                    })
                })?;
                Ok(Some(CollectionItem::Single(c)))
            }
            'x' => {
                self.advance(); // consume 'x'
                let code = self.parse_hex_digits(2);
                let c = char::from_u32(code).ok_or_else(|| {
                    VimRegexError::from(VimRegexErrorKind::InvalidCollectionCharCode {
                        span: self.span_from(open_pos),
                        detail: CompactString::from(format!(
                            "\\x{code:02x} (hex, value {code}) is not a valid Unicode code point"
                        )),
                    })
                })?;
                Ok(Some(CollectionItem::Single(c)))
            }
            'u' => {
                self.advance(); // consume 'u'
                let code = self.parse_hex_digits(4);
                let c = char::from_u32(code).ok_or_else(|| {
                    VimRegexError::from(VimRegexErrorKind::InvalidCollectionCharCode {
                        span: self.span_from(open_pos),
                        detail: CompactString::from(format!(
                            "\\u{code:04x} (4-digit hex, value {code}) is not a valid Unicode code point"
                        )),
                    })
                })?;
                Ok(Some(CollectionItem::Single(c)))
            }
            'U' => {
                self.advance(); // consume 'U'
                let code = self.parse_hex_digits(8);
                let c = char::from_u32(code).ok_or_else(|| {
                    VimRegexError::from(VimRegexErrorKind::InvalidCollectionCharCode {
                        span: self.span_from(open_pos),
                        detail: CompactString::from(format!(
                            "\\U{code:08x} (8-digit hex, value {code}) is not a valid Unicode code point"
                        )),
                    })
                })?;
                Ok(Some(CollectionItem::Single(c)))
            }
            _ => Ok(None),
        }
    }

    /// Try to parse a POSIX equivalence class `=c=` at the current position
    /// (inside the enclosing `[…]` collection).
    ///
    /// Vim's syntax is `[=c=]` where `[` and `]` are the collection delimiters.
    /// When this method is called the outer `[` has already been consumed, so
    /// the current character is `=`.  We recognise the three-character sequence
    /// `= c =` (opening `=`, exactly one character, closing `=`) and treat `c`
    /// as a literal character (equivalence classes are a POSIX extension that
    /// Vim essentially no-ops).
    ///
    /// Returns `Ok(Some(item))` if the layout `= c =` was recognised and the
    /// three characters were consumed, `Ok(None)` otherwise (caller falls
    /// through to normal character parsing).
    fn try_parse_equiv_class(&mut self) -> Result<Option<CollectionItem>, VimRegexError> {
        // We already know peek() == '='
        // Required layout (relative to current pos):
        //   =   c   =
        //  +0  +1  +2
        let c = self.peek_at(1);
        let close_eq = self.peek_at(2);

        match (c, close_eq) {
            (Some(ch), Some('=')) => {
                // Consume the three equivalence-class tokens: = c =
                self.advance(); // consume '='
                self.advance(); // consume c
                self.advance(); // consume '='
                Ok(Some(CollectionItem::Single(ch)))
            }
            // Not a well-formed =c= — fall through.
            _ => Ok(None),
        }
    }

    /// Try to parse a POSIX collating element `.c.` at the current position
    /// (inside the enclosing `[…]` collection).
    ///
    /// Vim's syntax is `[.c.]` where `[` and `]` are the collection delimiters.
    /// When this method is called the outer `[` has already been consumed, so
    /// the current character is `.`.  We recognise the three-character sequence
    /// `. c .` (opening `.`, exactly one character, closing `.`) and treat `c`
    /// as a literal character (collating elements are a POSIX extension that
    /// Vim essentially no-ops).
    ///
    /// Returns `Ok(Some(item))` if the layout `. c .` was recognised and the
    /// three characters were consumed, `Ok(None)` otherwise (caller falls
    /// through to normal character parsing).
    fn try_parse_collating_element(&mut self) -> Result<Option<CollectionItem>, VimRegexError> {
        // We already know peek() == '.'
        // Required layout (relative to current pos):
        //   .   c   .
        //  +0  +1  +2
        let c = self.peek_at(1);
        let close_dot = self.peek_at(2);

        match (c, close_dot) {
            (Some(ch), Some('.')) => {
                // Consume the three collating-element tokens: . c .
                self.advance(); // consume '.'
                self.advance(); // consume c
                self.advance(); // consume '.'
                Ok(Some(CollectionItem::Single(ch)))
            }
            // Not a well-formed .c. — fall through.
            _ => Ok(None),
        }
    }
}
