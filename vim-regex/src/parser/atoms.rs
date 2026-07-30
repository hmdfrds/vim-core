//! Atom parsing for the Vim regex parser.
//!
//! An "atom" is the smallest unit in the grammar: a literal, anchor, character
//! class, escape sequence, group, or collection. This module implements
//! `parse_atom` and all its sub-dispatchers.

use crate::MagicMode;

use crate::common::{MAX_CAPTURE_GROUPS, MAX_GROUP_NESTING};

use crate::ir::ComposingMode;

use super::{CaseMode, Parser, VimPatternNode, VimRegexError, VimRegexErrorKind};

// ═══════════════════════════════════════════════════════════════════════════════
// ATOM PARSING
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse a single atom from the input.
    ///
    /// Returns `Ok(None)` when the input is exhausted or a group/alternation
    /// terminator is encountered. Returns `Ok(Some(node))` for a successfully
    /// parsed atom.
    pub(super) fn parse_atom(&mut self) -> Result<Option<VimPatternNode>, VimRegexError> {
        let Some(ch) = self.peek() else {
            return Ok(None);
        };

        // Backslash: escape sequence or escaped metachar
        if ch == '\\' {
            return self.parse_backslash_atom();
        }

        // Metacharacters in current magic mode
        if self.is_metachar(ch) {
            return self.parse_metachar_atom(ch);
        }

        // Literal character
        self.advance();
        Ok(Some(VimPatternNode::Literal(ch)))
    }

    /// Parse a metacharacter atom.
    ///
    /// In VeryMagic mode, many more characters are metacharacters, including
    /// `(`, `)`, `|`, `+`, `?`, `{`, `}`. These are handled by the grammar
    /// hierarchy (parse_alternation handles `|`, parse_piece handles quantifiers)
    /// so we only handle the atom-level metacharacters here.
    fn parse_metachar_atom(&mut self, ch: char) -> Result<Option<VimPatternNode>, VimRegexError> {
        match ch {
            '.' => {
                self.advance();
                Ok(Some(VimPatternNode::AnyChar))
            }
            '^' => {
                self.advance();
                // In VeryMagic mode, ^ is always an anchor (Vim's MAGIC_ALL).
                // In other modes, ^ is only an anchor at branch start.
                if self.magic == MagicMode::VeryMagic || self.at_branch_start {
                    Ok(Some(VimPatternNode::StartOfLine))
                } else {
                    Ok(Some(VimPatternNode::Literal('^')))
                }
            }
            '$' => {
                self.advance();
                if self.is_at_branch_end() {
                    Ok(Some(VimPatternNode::EndOfLine))
                } else {
                    Ok(Some(VimPatternNode::Literal('$')))
                }
            }
            '~' => {
                self.advance();
                self.features.has_last_substitute = true;
                Ok(Some(VimPatternNode::LastSubstitute))
            }
            '*' => {
                // `*` as metachar in parse_atom means no preceding atom
                // (e.g., pattern starts with `*`). Vim treats this as literal.
                self.advance();
                Ok(Some(VimPatternNode::Literal('*')))
            }
            '[' => self.parse_collection_from_bracket(),
            '(' if self.magic == MagicMode::VeryMagic => self.parse_group(true),
            // VeryMagic `)` at top level (no enclosing group) is an error
            ')' if self.magic == MagicMode::VeryMagic && self.group_depth == 0 => {
                let span = self.span_at(self.pos);
                Err(VimRegexErrorKind::UnmatchedGroup { span }.into())
            }
            // `|` and `)` in VeryMagic are handled by parse_alternation/parse_sequence
            // `+`, `?`, `{`, `}` in VeryMagic are handled by try_parse_quantifier
            // All other metachar in VeryMagic that we don't handle: treat as literal
            _ => {
                self.advance();
                Ok(Some(VimPatternNode::Literal(ch)))
            }
        }
    }

    /// Parse `[` to start a collection in magic modes where `[` is a metachar.
    fn parse_collection_from_bracket(&mut self) -> Result<Option<VimPatternNode>, VimRegexError> {
        // Don't advance — parse_collection expects to see `[`
        self.parse_collection().map(Some)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKSLASH DISPATCH
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse an atom starting with backslash.
    ///
    /// Dispatches to specialized handlers based on the character after `\`.
    pub(super) fn parse_backslash_atom(&mut self) -> Result<Option<VimPatternNode>, VimRegexError> {
        let backslash_pos = self.pos;
        self.advance(); // consume '\'

        let Some(ch) = self.peek() else {
            return Err(VimRegexErrorKind::TrailingBackslash {
                span: self.span_from(backslash_pos),
            }
            .into());
        };

        self.parse_escaped(ch, backslash_pos)
    }

    /// Parse the character(s) after a backslash.
    ///
    /// Handles character classes, escape sequences, word boundaries,
    /// match overrides, groups, alternation, backreferences, and literal escapes.
    fn parse_escaped(
        &mut self,
        ch: char,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        // Character classes: \d, \D, \w, \W, \s, \S, etc.
        if let Some(node) = self.try_parse_char_class(ch) {
            self.advance();
            return Ok(Some(node));
        }

        // Escape sequences: \n, \t, \r, \e, \b
        if let Some(kind) = Self::try_parse_escape_kind(ch) {
            self.advance();
            return Ok(Some(VimPatternNode::EscapeSequence(kind)));
        }

        self.parse_escaped_special(ch, backslash_pos)
    }

    /// Second-level dispatch for escaped characters (non-class, non-escape).
    fn parse_escaped_special(
        &mut self,
        ch: char,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        match ch {
            // Word boundaries: \<, \>
            '<' => {
                self.advance();
                Ok(Some(VimPatternNode::WordBoundaryStart))
            }
            '>' => {
                self.advance();
                Ok(Some(VimPatternNode::WordBoundaryEnd))
            }
            // Match overrides: \zs, \ze
            'z' => self.parse_z_sequence(backslash_pos),
            // Newline-including variants: \_., \_d, etc.
            '_' => self.parse_underscore_sequence(backslash_pos),
            // Case modifiers: \c, \C — no AST node produced, the calling loop
            // in parse_sequence will continue parsing the next atom.
            'c' => {
                self.advance();
                self.case_mode = CaseMode::Insensitive;
                Ok(None)
            }
            'C' => {
                self.advance();
                self.case_mode = CaseMode::Sensitive;
                Ok(None)
            }
            // Magic mode switches: \v, \m, \M, \V — same pattern as \c/\C.
            'v' => {
                self.advance();
                self.magic = MagicMode::VeryMagic;
                Ok(None)
            }
            'm' => {
                self.advance();
                self.magic = MagicMode::Magic;
                Ok(None)
            }
            'M' => {
                self.advance();
                self.magic = MagicMode::NoMagic;
                Ok(None)
            }
            'V' => {
                self.advance();
                self.magic = MagicMode::VeryNoMagic;
                Ok(None)
            }
            // Composing character modifier: \Z — ignore combining marks.
            'Z' => {
                self.advance();
                self.composing_mode = ComposingMode::Ignore;
                Ok(None)
            }
            _ => self.parse_escaped_remaining(ch, backslash_pos),
        }
    }

    /// Third-level dispatch for escaped characters (groups, backrefs, percent, etc.).
    fn parse_escaped_remaining(
        &mut self,
        ch: char,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        match ch {
            // Groups: \( and \) — but only in non-VeryMagic modes
            '(' if self.magic != MagicMode::VeryMagic => self.parse_group(true),
            ')' if self.magic != MagicMode::VeryMagic => Err(VimRegexErrorKind::UnmatchedGroup {
                span: self.span_from(backslash_pos),
            }
            .into()),
            // In VeryMagic: \( and \) are literal
            '(' | ')' if self.magic == MagicMode::VeryMagic => {
                self.advance();
                Ok(Some(VimPatternNode::Literal(ch)))
            }
            // \% dispatch — buffer positions, non-capturing groups, etc.
            '%' => self.parse_percent_dispatch(backslash_pos),
            // Backreferences: \1 through \9
            '1'..='9' => {
                let digit = ch as u8 - b'0';
                self.advance();
                self.features.has_backreferences = true;
                Ok(Some(VimPatternNode::BackReference(digit)))
            }
            // Magic inversion: when a char is NOT a metachar in the current
            // mode, `\` makes it meta. Handle the chars that have meta meaning.
            _ if !self.is_metachar(ch) => self.parse_magic_inversion(ch),
            // Fallthrough: literal escape (char IS meta, `\` makes it literal)
            _ => {
                self.advance();
                Ok(Some(VimPatternNode::Literal(ch)))
            }
        }
    }

    /// Handle magic mode inversion: `\` before a non-meta char activates its
    /// meta meaning. For example, `\.` in NoMagic → `AnyChar`.
    fn parse_magic_inversion(&mut self, ch: char) -> Result<Option<VimPatternNode>, VimRegexError> {
        match ch {
            '.' => {
                self.advance();
                Ok(Some(VimPatternNode::AnyChar))
            }
            '~' => {
                self.advance();
                self.features.has_last_substitute = true;
                Ok(Some(VimPatternNode::LastSubstitute))
            }
            '[' => self.parse_collection_from_bracket(),
            '^' => {
                self.advance();
                if self.at_branch_start {
                    Ok(Some(VimPatternNode::StartOfLine))
                } else {
                    Ok(Some(VimPatternNode::Literal('^')))
                }
            }
            '$' => {
                self.advance();
                if self.is_at_branch_end() {
                    Ok(Some(VimPatternNode::EndOfLine))
                } else {
                    Ok(Some(VimPatternNode::Literal('$')))
                }
            }
            // For any other non-meta char, `\x` is just literal `x`
            _ => {
                self.advance();
                Ok(Some(VimPatternNode::Literal(ch)))
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKSLASH SUB-SEQUENCES
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse `\z` sequences: `\zs` (set match start) and `\ze` (set match end).
    fn parse_z_sequence(
        &mut self,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume 'z'

        match self.peek() {
            Some('s') => {
                self.advance();
                self.features.has_match_override = true;
                Ok(Some(VimPatternNode::SetMatchStart))
            }
            Some('e') => {
                self.advance();
                self.features.has_match_override = true;
                Ok(Some(VimPatternNode::SetMatchEnd))
            }
            // \z1–\z9: external sub-match references (not supported).
            // Consume the digit so error position is accurate and the digit
            // is not re-parsed as a literal after error recovery.
            Some(ch) if ch.is_ascii_digit() && ch != '0' => {
                self.advance(); // consume the digit
                Err(VimRegexErrorKind::InvalidEscape {
                    span: self.span_from(backslash_pos),
                    ch: 'z',
                }
                .into())
            }
            Some(_) | None => Err(VimRegexErrorKind::InvalidEscape {
                span: self.span_from(backslash_pos),
                ch: 'z',
            }
            .into()),
        }
    }

    /// Parse `\_` sequences: `\_.` (any char including newline), `\_d`, `\_w`,
    /// etc., and `\_[` (collection with newline).
    fn parse_underscore_sequence(
        &mut self,
        backslash_pos: usize,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        self.advance(); // consume '_'

        let Some(ch) = self.peek() else {
            return Err(VimRegexErrorKind::InvalidEscape {
                span: self.span_from(backslash_pos),
                ch: '_',
            }
            .into());
        };

        // \_. = any char including newline
        if ch == '.' {
            self.advance();
            return Ok(Some(VimPatternNode::AnyCharNl));
        }

        // \_[ = collection with include_newline flag
        if ch == '[' {
            return self.parse_collection_with_newline().map(Some);
        }

        // \_^ = start-of-line anywhere in pattern
        if ch == '^' {
            self.advance();
            return Ok(Some(VimPatternNode::AnywhereStartOfLine));
        }

        // \_$ = end-of-line anywhere in pattern
        if ch == '$' {
            self.advance();
            return Ok(Some(VimPatternNode::AnywhereEndOfLine));
        }

        // \_d, \_w, etc. = class with newline
        if let Some(VimPatternNode::Class(class)) = self.try_parse_char_class(ch) {
            self.advance();
            return Ok(Some(VimPatternNode::ClassWithNewline(class)));
        }

        Err(VimRegexErrorKind::InvalidEscape {
            span: self.span_from(backslash_pos),
            ch: '_',
        }
        .into())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GROUP PARSING
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse a group: `\(` ... `\)` (capturing) or `\%(` ... `\)` (non-capturing).
    ///
    /// In VeryMagic mode, `(` starts a capturing group and `)` closes it.
    /// The `capturing` parameter indicates whether this is a capturing group.
    pub(super) fn parse_group(
        &mut self,
        capturing: bool,
    ) -> Result<Option<VimPatternNode>, VimRegexError> {
        let open_pos = self.pos;
        self.advance(); // consume '(' (or the '(' after '\' which was already consumed)

        if self.group_depth >= MAX_GROUP_NESTING {
            return Err(VimRegexErrorKind::PatternTooComplex {
                span: Some(self.span_at(open_pos)),
                detail: "group nesting too deep".into(),
            }
            .into());
        }

        if capturing {
            // Vim only supports \1 through \9 backreferences.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "MAX_CAPTURE_GROUPS is 9, always fits in u8"
            )]
            if self.group_count >= MAX_CAPTURE_GROUPS as u8 {
                return Err(VimRegexErrorKind::PatternTooComplex {
                    span: Some(self.span_at(open_pos)),
                    detail: "more than 9 capturing groups".into(),
                }
                .into());
            }
            self.group_count = self.group_count.saturating_add(1);
            self.features.capture_count = self.group_count;
        }

        self.group_depth += 1;
        let inner = self.parse_alternation()?;
        self.group_depth -= 1;

        if !self.at_group_close() {
            return Err(VimRegexErrorKind::UnmatchedGroup {
                span: self.span_from(open_pos),
            }
            .into());
        }

        self.consume_group_close();

        Ok(Some(VimPatternNode::Group {
            inner: Box::new(inner),
            capturing,
        }))
    }

    /// Consume the group close delimiter: `\)` or bare `)`.
    fn consume_group_close(&mut self) {
        if self.magic != MagicMode::VeryMagic {
            self.advance(); // consume '\' (VeryMagic has bare ')')
        }
        self.advance(); // consume ')'
    }
}
