//! `\%` dispatch for the Vim regex parser.
//!
//! Handles buffer-position atoms (`\%#`, `\%V`, `\%l`, `\%c`, `\%v`, `\%'m`),
//! character codes (`\%d`, `\%x`, `\%o`, `\%u`, `\%U`), non-capturing groups
//! (`\%(`), and optional sequences (`\%[`).

use compact_str::CompactString;

use crate::ir::{ColumnSpec, LineSpec, MarkRel, PercentEscapeContext};

use super::{Parser, VimPatternNode, VimRegexError, VimRegexErrorKind};

// ═══════════════════════════════════════════════════════════════════════════════
// \% DISPATCH
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse the `\%` prefix and dispatch to the appropriate handler.
    pub(super) fn parse_percent_dispatch(
        &mut self,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume '%'

        let Some(ch) = self.peek() else {
            return Err(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::EndOfInput,
            }
            .into());
        };

        match ch {
            '#' => {
                self.advance();
                self.features.has_buffer_position = true;
                Ok(Some(VimPatternNode::CursorPosition))
            }
            'V' => {
                self.advance();
                self.features.has_buffer_position = true;
                Ok(Some(VimPatternNode::VisualArea))
            }
            '^' => {
                self.advance();
                self.features.has_buffer_position = true;
                Ok(Some(VimPatternNode::StartOfFile))
            }
            '$' => {
                self.advance();
                self.features.has_buffer_position = true;
                Ok(Some(VimPatternNode::EndOfFile))
            }
            '(' => self.parse_group(false),
            '[' => self.parse_optional_sequence(backslash_pos),
            '\'' => self.parse_percent_mark(backslash_pos),
            '<' | '>' => self.parse_percent_relation(ch, backslash_pos),
            '.' => self.parse_percent_current(backslash_pos),
            'C' => {
                self.advance();
                Ok(Some(VimPatternNode::AnyComposing))
            }
            'd' | 'x' | 'o' | 'u' | 'U' => self.parse_percent_char_code(ch, backslash_pos),
            _ if ch.is_ascii_digit() => self.parse_percent_number(backslash_pos),
            _ => Err(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::UnrecognizedChar,
            }
            .into()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OPTIONAL SEQUENCE
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse `\%[atoms]` — an optionally matched sequence.
    fn parse_optional_sequence(
        &mut self,
        open_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume '['

        let mut items = Vec::new();

        while let Some(ch) = self.peek() {
            if ch == ']' {
                self.advance(); // consume ']'
                if items.is_empty() {
                    return Err(VimRegexErrorKind::InvalidOptionalSequenceAtom {
                        span: self.span_from(open_pos),
                    }
                    .into());
                }
                return Ok(Some(VimPatternNode::OptionalSequence(items)));
            }

            match self.parse_atom()? {
                Some(node) => {
                    if !Self::is_valid_optional_sequence_atom(&node) {
                        return Err(VimRegexErrorKind::InvalidOptionalSequenceAtom {
                            span: self.span_from(open_pos),
                        }
                        .into());
                    }
                    items.push(node);
                }
                None => break,
            }
        }

        Err(VimRegexErrorKind::UnterminatedCollection {
            span: self.span_from(open_pos),
        }
        .into())
    }

    /// Returns `true` if the node is allowed inside `\%[...]`.
    ///
    /// Vim permits only: literals, any-char (`.`), any-char-nl (`\_.`),
    /// character classes (`\d` etc.), classes-with-newline (`\_d`),
    /// collections (`[...]`), escape sequences (`\n`, `\t`), and
    /// character-by-code (`\%d123`).
    fn is_valid_optional_sequence_atom(node: &VimPatternNode) -> bool {
        matches!(
            node,
            VimPatternNode::Literal(_)
                | VimPatternNode::AnyChar
                | VimPatternNode::AnyCharNl
                | VimPatternNode::Class(_)
                | VimPatternNode::ClassWithNewline(_)
                | VimPatternNode::Collection { .. }
                | VimPatternNode::EscapeSequence(_)
                | VimPatternNode::CharByCode(_)
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MARK POSITION
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse `\%'m` — position at mark.
    fn parse_percent_mark(
        &mut self,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume '\''

        let mark = self.advance().ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::MissingMarkChar,
            })
        })?;

        self.features.has_buffer_position = true;
        Ok(Some(VimPatternNode::AtMark {
            mark,
            rel: MarkRel::At,
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RELATIONAL POSITIONS: \%< and \%>
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse `\%<` or `\%>` followed by a number+suffix, `'m`, or `.` + suffix.
    fn parse_percent_relation(
        &mut self,
        rel_char: char,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume '<' or '>'

        if self.peek() == Some('\'') {
            return self.parse_percent_relation_mark(rel_char, backslash_pos);
        }

        // Handle \%<.l, \%>.l, \%<.c, \%>.c, \%<.v, \%>.v (current position relations)
        if self.peek() == Some('.') {
            return self.parse_percent_relation_current(rel_char, backslash_pos);
        }

        // Expect a number followed by l/c/v
        let start_pos = self.pos;
        let num = self.parse_decimal_number();
        if self.pos == start_pos {
            return Err(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::MissingRelationTarget,
            }
            .into());
        }

        self.parse_position_suffix_with_rel(num, rel_char, backslash_pos)
    }

    /// Parse `\%<.l`, `\%>.l`, `\%<.c`, `\%>.c`, `\%<.v`, `\%>.v`.
    fn parse_percent_relation_current(
        &mut self,
        rel_char: char,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume '.'

        let suffix = self.advance().ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::MissingPositionSuffix,
            })
        })?;

        self.features.has_buffer_position = true;

        match suffix {
            'l' => {
                let spec = if rel_char == '<' {
                    LineSpec::BeforeCurrent
                } else {
                    LineSpec::AfterCurrent
                };
                Ok(Some(VimPatternNode::AtLine(spec)))
            }
            'c' => {
                let spec = if rel_char == '<' {
                    ColumnSpec::BeforeCurrent
                } else {
                    ColumnSpec::AfterCurrent
                };
                Ok(Some(VimPatternNode::AtColumn(spec)))
            }
            'v' => {
                let spec = if rel_char == '<' {
                    ColumnSpec::BeforeCurrent
                } else {
                    ColumnSpec::AfterCurrent
                };
                Ok(Some(VimPatternNode::AtVirtualColumn(spec)))
            }
            _ => Err(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::InvalidPositionSuffix,
            }
            .into()),
        }
    }

    /// Parse `\%<'m` or `\%>'m` (mark relation).
    fn parse_percent_relation_mark(
        &mut self,
        rel_char: char,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume '\''

        let mark = self.advance().ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::MissingMarkChar,
            })
        })?;

        let rel = if rel_char == '<' {
            MarkRel::Before
        } else {
            MarkRel::After
        };

        self.features.has_buffer_position = true;
        Ok(Some(VimPatternNode::AtMark { mark, rel }))
    }

    /// Parse a `l`/`c`/`v` suffix after a number with a `<`/`>` relation.
    fn parse_position_suffix_with_rel(
        &mut self,
        num: u32,
        rel_char: char,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        let suffix = self.advance().ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::MissingPositionSuffix,
            })
        })?;

        self.features.has_buffer_position = true;

        match suffix {
            'l' => {
                let spec = if rel_char == '<' {
                    LineSpec::Before(num)
                } else {
                    LineSpec::After(num)
                };
                Ok(Some(VimPatternNode::AtLine(spec)))
            }
            'c' => {
                let spec = if rel_char == '<' {
                    ColumnSpec::Before(num)
                } else {
                    ColumnSpec::After(num)
                };
                Ok(Some(VimPatternNode::AtColumn(spec)))
            }
            'v' => {
                let spec = if rel_char == '<' {
                    ColumnSpec::Before(num)
                } else {
                    ColumnSpec::After(num)
                };
                Ok(Some(VimPatternNode::AtVirtualColumn(spec)))
            }
            _ => Err(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::InvalidPositionSuffix,
            }
            .into()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CURRENT POSITION: \%.l, \%.c, \%.v
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse `\%.l`, `\%.c`, `\%.v` — current line/column/virtual column.
    fn parse_percent_current(
        &mut self,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume '.'

        let suffix = self.advance().ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::MissingPositionSuffix,
            })
        })?;

        self.features.has_buffer_position = true;

        match suffix {
            'l' => Ok(Some(VimPatternNode::AtLine(LineSpec::Current))),
            'c' => Ok(Some(VimPatternNode::AtColumn(ColumnSpec::Current))),
            'v' => Ok(Some(VimPatternNode::AtVirtualColumn(ColumnSpec::Current))),
            _ => Err(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::InvalidPositionSuffix,
            }
            .into()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ABSOLUTE POSITION: \%23l, \%23c, \%23v
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse `\%<number>l/c/v` — absolute line/column/virtual column.
    fn parse_percent_number(
        &mut self,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        let num = self.parse_decimal_number();

        let suffix = self.advance().ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::MissingPositionSuffix,
            })
        })?;

        self.features.has_buffer_position = true;

        match suffix {
            'l' => Ok(Some(VimPatternNode::AtLine(LineSpec::Exact(num)))),
            'c' => Ok(Some(VimPatternNode::AtColumn(ColumnSpec::Exact(num)))),
            'v' => Ok(Some(VimPatternNode::AtVirtualColumn(ColumnSpec::Exact(
                num,
            )))),
            _ => Err(VimRegexErrorKind::InvalidPercentEscape {
                span: self.span_from(backslash_pos),
                found: PercentEscapeContext::InvalidPositionSuffix,
            }
            .into()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHARACTER CODES: \%d, \%x, \%o, \%u, \%U
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse `\%d123`, `\%x1a`, `\%o177`, `\%u20AC`, `\%U0010FFFF`.
    fn parse_percent_char_code(
        &mut self,
        base_char: char,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume the base character

        let (radix, max_digits) = match base_char {
            'd' => (10, 10),
            'x' => (16, 2),
            'o' => (8, 3),
            'u' => (16, 4),
            'U' => (16, 8),
            _ => {
                return Err(VimRegexErrorKind::InvalidCharCode {
                    span: self.span_from(backslash_pos),
                    detail: CompactString::from("unknown base character"),
                }
                .into())
            }
        };

        self.parse_char_code_digits(radix, max_digits, backslash_pos)
    }

    /// Parse digits in the given radix and convert to a character.
    fn parse_char_code_digits(
        &mut self,
        radix: u32,
        max_digits: usize,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        let mut value: u32 = 0;
        let mut count = 0;

        while count < max_digits {
            let Some(c) = self.peek() else { break };
            let Some(digit) = c.to_digit(radix) else {
                break;
            };
            value = value.saturating_mul(radix).saturating_add(digit);
            self.advance();
            count += 1;
        }

        if count == 0 {
            return Err(VimRegexErrorKind::InvalidCharCode {
                span: self.span_from(backslash_pos),
                detail: CompactString::from("no digits after base specifier"),
            }
            .into());
        }

        // Check surrogate range first (more specific), then general range
        if (0xD800..=0xDFFF).contains(&value) {
            return Err(VimRegexErrorKind::CharCodeSurrogate {
                span: self.span_from(backslash_pos),
                value,
            }
            .into());
        }

        let ch = char::from_u32(value).ok_or_else(|| {
            VimRegexError::from(VimRegexErrorKind::CharCodeOutOfRange {
                span: self.span_from(backslash_pos),
                value,
            })
        })?;

        Ok(Some(VimPatternNode::CharByCode(ch)))
    }
}
