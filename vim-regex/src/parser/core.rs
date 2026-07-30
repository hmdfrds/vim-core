//! Core parser struct, public API, grammar hierarchy, and helpers.

use crate::MagicMode;

use crate::ir::{
    CaseMode, ComposingMode, EscapeKind, LookaroundKind, ParseResult, PatternFeatures, Span,
    VimPatternNode, VimRegexError, VimRegexErrorKind,
};

// ═══════════════════════════════════════════════════════════════════════════════
// PARSER STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Parser for Vim regex patterns.
///
/// Holds parsing state as it walks through the input character by character.
/// The parser is single-use: create one, call `parse_pattern` or
/// `parse_with_magic`, consume the result.
pub(crate) struct Parser {
    /// Input characters (pre-collected from the pattern string).
    pub(super) input: Vec<char>,
    /// Byte offsets for each char index. `byte_offsets[i]` is the byte position
    /// of `input[i]` in the original pattern string. `byte_offsets[input.len()]`
    /// equals the total byte length (sentinel for end-of-input spans).
    pub(super) byte_offsets: Vec<usize>,
    /// Current position in `input`.
    pub(super) pos: usize,
    /// Active magic mode (controls which characters are metacharacters).
    pub(super) magic: MagicMode,
    /// Case sensitivity mode (set by `\c` / `\C` in pattern).
    pub(super) case_mode: CaseMode,
    /// Composing character mode (set by `\Z` in pattern).
    pub(super) composing_mode: ComposingMode,
    /// Feature flags accumulated during parsing.
    pub(super) features: PatternFeatures,
    /// Number of capturing groups seen so far.
    pub(super) group_count: u8,
    /// Current group nesting depth (for detecting unmatched group close).
    pub(super) group_depth: u32,
    /// Whether the parser is at the start of a branch (for `^` context sensitivity).
    /// Set `true` at: pattern start, after `\|`, after `\(` / `\%(`.
    /// Set `false` after consuming any atom.
    pub(super) at_branch_start: bool,
    /// Current collection nesting depth (for detecting deeply nested `[...]`).
    pub(super) collection_depth: u32,
    /// Accumulated parse errors for multi-error recovery mode.
    ///
    /// When recovery is enabled, non-fatal errors are pushed here instead
    /// of being returned immediately. The first error is still returned as
    /// the primary error for backward compatibility.
    pub(super) errors: Vec<VimRegexError>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PUBLIC API
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a Vim regex pattern using the default `Magic` mode.
///
/// Returns the parsed AST, case mode, and feature flags.
///
/// # Errors
///
/// Returns `VimRegexError` if the pattern is invalid.
#[cfg(test)]
pub(crate) fn parse_pattern(pattern: &str) -> Result<ParseResult, VimRegexError> {
    parse_with_magic(pattern, MagicMode::Magic)
}

/// Parse a Vim regex pattern with an explicit magic mode.
///
/// # Errors
///
/// Returns `VimRegexError` if the pattern is invalid.
pub(crate) fn parse_with_magic(
    pattern: &str,
    magic: MagicMode,
) -> Result<ParseResult, VimRegexError> {
    if pattern.is_empty() {
        return Err(VimRegexErrorKind::EmptyPattern.into());
    }

    let mut parser = Parser {
        input: {
            let mut v = Vec::with_capacity(pattern.len());
            v.extend(pattern.chars());
            v
        },
        byte_offsets: {
            let mut offsets = Vec::with_capacity(pattern.len() + 1);
            let mut byte_pos = 0;
            for ch in pattern.chars() {
                offsets.push(byte_pos);
                byte_pos += ch.len_utf8();
            }
            offsets.push(byte_pos); // sentinel: end of pattern
            offsets
        },
        pos: 0,
        magic,
        case_mode: CaseMode::Default,
        composing_mode: ComposingMode::Respect,
        features: PatternFeatures::default(),
        group_count: 0,
        group_depth: 0,
        at_branch_start: true,
        collection_depth: 0,
        errors: Vec::new(),
    };

    let node = parser.parse_alternation()?;

    Ok(ParseResult {
        node,
        case_mode: parser.case_mode,
        composing_mode: parser.composing_mode,
        features: parser.features,
        additional_errors: parser.errors,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// PARSER INTERNALS — CORE INFRASTRUCTURE
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Peek at the current character without advancing.
    pub(super) fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    /// Peek at the character at a given offset from the current position.
    pub(super) fn peek_at(&self, offset: usize) -> Option<char> {
        self.pos
            .checked_add(offset)
            .and_then(|i| self.input.get(i))
            .copied()
    }

    /// Advance the position and return the consumed character.
    pub(super) fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    /// Convert a char index to its byte offset in the original pattern string.
    ///
    /// If `char_pos` equals `self.input.len()`, returns the pattern's byte length
    /// (the sentinel value).
    #[inline]
    pub(super) fn byte_offset(&self, char_pos: usize) -> usize {
        self.byte_offsets[char_pos.min(self.byte_offsets.len() - 1)]
    }

    /// Create a `Span` from `start_char_pos` to the current position.
    ///
    /// The span covers bytes `[byte_offsets[start], byte_offsets[self.pos])`.
    #[inline]
    pub(super) fn span_from(&self, start_char_pos: usize) -> Span {
        Span::new(self.byte_offset(start_char_pos), self.byte_offset(self.pos))
    }

    /// Create a `Span` for a single character at the given char position.
    #[inline]
    pub(super) fn span_at(&self, char_pos: usize) -> Span {
        let start = self.byte_offset(char_pos);
        let end = if char_pos < self.input.len() {
            self.byte_offset(char_pos + 1)
        } else {
            start.saturating_add(1)
        };
        Span::new(start, end)
    }

    /// Check whether a character is a metacharacter in the current magic mode.
    ///
    /// In Vim, which characters are treated as special (regex metacharacters)
    /// depends on the magic mode:
    /// - `VeryMagic` (`\v`): all ASCII punctuation except `_` is special
    /// - `Magic` (default): `.`, `*`, `[`, `]`, `^`, `$`, `~` are special
    /// - `NoMagic` (`\M`): only `^`, `$` are special
    /// - `VeryNoMagic` (`\V`): nothing is special (only `\` escapes)
    pub(super) const fn is_metachar(&self, c: char) -> bool {
        match self.magic {
            MagicMode::VeryMagic => {
                // Everything except alphanumerics and underscore is special
                c.is_ascii_punctuation() && c != '_'
            }
            MagicMode::Magic => {
                matches!(c, '.' | '*' | '[' | ']' | '^' | '$' | '~')
            }
            MagicMode::NoMagic => {
                matches!(c, '^' | '$')
            }
            MagicMode::VeryNoMagic => false,
        }
    }

    /// Check if the current position is at an alternation separator.
    ///
    /// Returns `true` if the current position is at `\|` (Magic/NoMagic/VeryNoMagic)
    /// or bare `|` (VeryMagic).
    pub(super) fn at_alternation(&self) -> bool {
        match self.magic {
            MagicMode::VeryMagic => self.peek() == Some('|'),
            _ => self.peek() == Some('\\') && self.peek_at(1) == Some('|'),
        }
    }

    /// Check if the current position is at a branch-and separator.
    ///
    /// Returns `true` if at `\&` (Magic/NoMagic/VeryNoMagic)
    /// or bare `&` (VeryMagic).
    pub(super) fn at_branch_and(&self) -> bool {
        match self.magic {
            MagicMode::VeryMagic => self.peek() == Some('&'),
            _ => self.peek() == Some('\\') && self.peek_at(1) == Some('&'),
        }
    }

    /// Check if the current position is at a group close delimiter.
    ///
    /// Returns `true` if at `\)` (Magic/NoMagic/VeryNoMagic)
    /// or bare `)` (VeryMagic).
    pub(super) fn at_group_close(&self) -> bool {
        match self.magic {
            MagicMode::VeryMagic => self.peek() == Some(')'),
            _ => self.peek() == Some('\\') && self.peek_at(1) == Some(')'),
        }
    }

    /// Check if the current position is at the end of a branch.
    ///
    /// `$` is only an anchor when followed by: end of input, `\|`, `\)`, `\n`,
    /// or in VeryMagic mode (where `$` is always an anchor per Vim's MAGIC_ALL).
    ///
    /// Vim also skips modifiers (`\c`, `\C`, `\m`, `\M`, `\v`, `\V`, `\Z`)
    /// when looking ahead from `$`, tracking how they change the effective
    /// magic mode so that branch-end delimiters are checked correctly.
    pub(super) fn is_at_branch_end(&self) -> bool {
        // In VeryMagic mode, $ is always an anchor (Vim's MAGIC_ALL behavior)
        if self.magic == MagicMode::VeryMagic {
            return true;
        }

        // Skip modifiers (\c, \C, \m, \M, \v, \V, \Z) when looking ahead,
        // tracking effective magic mode changes.
        let mut offset = self.pos;
        let mut effective_magic = self.magic;
        loop {
            if offset >= self.input.len() {
                // At end of input
                return true;
            }
            // Check for modifier sequences to skip
            if self.input.get(offset) == Some(&'\\') {
                if let Some(&next_ch) = self.input.get(offset + 1) {
                    match next_ch {
                        'c' | 'C' | 'Z' => {
                            offset += 2;
                            continue;
                        }
                        'v' => {
                            effective_magic = MagicMode::VeryMagic;
                            offset += 2;
                            continue;
                        }
                        'V' => {
                            effective_magic = MagicMode::VeryNoMagic;
                            offset += 2;
                            continue;
                        }
                        'm' => {
                            effective_magic = MagicMode::Magic;
                            offset += 2;
                            continue;
                        }
                        'M' => {
                            effective_magic = MagicMode::NoMagic;
                            offset += 2;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            break;
        }

        // At end of input (after skipping modifiers)
        if offset >= self.input.len() {
            return true;
        }

        // Check what follows based on the effective magic mode after modifiers
        if let Some(&ch) = self.input.get(offset) {
            match effective_magic {
                MagicMode::VeryMagic => {
                    // In VeryMagic, branch-end delimiters are bare: |, &, )
                    if matches!(ch, '|' | '&' | ')') {
                        return true;
                    }
                    // \n is still \n in VeryMagic
                    if ch == '\\' {
                        if let Some(&next_ch) = self.input.get(offset + 1) {
                            if next_ch == 'n' {
                                return true;
                            }
                        }
                    }
                }
                _ => {
                    // In other modes, branch-end delimiters require backslash: \|, \&, \), \n
                    if ch == '\\' {
                        if let Some(&next_ch) = self.input.get(offset + 1) {
                            if matches!(next_ch, '|' | '&' | ')' | 'n') {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        false
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GRAMMAR HIERARCHY
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Parse an alternation: `branch \| branch \| ...`
    ///
    /// Top level of the grammar. Calls `parse_branch_and()` for each branch,
    /// splits on `\|` (or `|` in VeryMagic).
    pub(super) fn parse_alternation(&mut self) -> Result<VimPatternNode, VimRegexError> {
        let first = self.parse_branch_and()?;

        if !self.at_alternation() {
            return Ok(first);
        }

        let mut branches = vec![first];

        while self.at_alternation() {
            self.consume_alternation_sep();
            let branch = self.parse_branch_and()?;
            branches.push(branch);
        }

        Ok(VimPatternNode::Alternation(branches))
    }

    /// Consume the alternation separator (`\|` or `|`).
    fn consume_alternation_sep(&mut self) {
        if self.magic != MagicMode::VeryMagic {
            self.advance(); // consume '\' (VeryMagic has bare '|')
        }
        self.advance(); // consume '|'
    }

    /// Consume the branch-and separator (`\&` or `&`).
    fn consume_branch_and_sep(&mut self) {
        if self.magic != MagicMode::VeryMagic {
            self.advance(); // consume '\'
        }
        self.advance(); // consume '&'
    }

    /// Parse a branch-and: `sequence \& sequence \& ...`
    ///
    /// Second level of the grammar. All branches must match at the same
    /// position; only the last branch's extent determines match length.
    fn parse_branch_and(&mut self) -> Result<VimPatternNode, VimRegexError> {
        let first = self.parse_sequence()?;

        if !self.at_branch_and() {
            return Ok(first);
        }

        self.features.has_branch_and = true;
        let mut branches = vec![first];

        while self.at_branch_and() {
            self.consume_branch_and_sep();
            let branch = self.parse_sequence()?;
            branches.push(branch);
        }

        Ok(VimPatternNode::BranchAnd(branches))
    }

    /// Parse a sequence of pieces (the main concatenation loop).
    ///
    /// Stops at end-of-input, alternation separator, branch-and separator,
    /// or group close (only recognized when inside a group, i.e.,
    /// `group_depth > 0` — otherwise `\)` is an error).
    /// Modifiers (like `\c`, `\v`) produce no node, so parse_piece may
    /// return `None` — those are simply skipped.
    fn parse_sequence(&mut self) -> Result<VimPatternNode, VimRegexError> {
        self.at_branch_start = true;
        let mut nodes = Vec::new();

        while self.peek().is_some()
            && !self.at_alternation()
            && !self.at_branch_and()
            && !self.at_end_of_group()
        {
            if let Some(node) = self.parse_piece()? {
                // \n resets branch-start context: ^ after \n is an anchor
                if node == VimPatternNode::EscapeSequence(EscapeKind::Newline) {
                    self.at_branch_start = true;
                }
                nodes.push(node);
            }
        }

        Ok(Self::simplify_sequence(nodes))
    }

    /// Check if we are at the end of a group (only when inside one).
    fn at_end_of_group(&self) -> bool {
        self.group_depth > 0 && self.at_group_close()
    }

    /// Simplify a list of nodes into a single node.
    ///
    /// - Empty list: returns `Sequence(vec![])` (degenerate but valid)
    /// - Single node: returns that node directly (no wrapping)
    /// - Multiple nodes: returns `Sequence(nodes)`
    fn simplify_sequence(nodes: Vec<VimPatternNode>) -> VimPatternNode {
        if nodes.len() == 1 {
            // SAFETY: len() == 1, so into_iter().next() always yields Some
            // This is a logic invariant, not an unsafe operation.
            #[allow(clippy::unwrap_used, reason = "len() == 1 guarantees Some")]
            nodes.into_iter().next().unwrap()
        } else {
            VimPatternNode::Sequence(nodes)
        }
    }

    /// Returns `true` for atoms that cannot be quantified.
    ///
    /// In Vim, anchors and zero-width assertions silently reject quantifiers —
    /// the quantifier character stays in the input as a literal for the next atom.
    fn is_unquantifiable(node: &VimPatternNode) -> bool {
        matches!(
            node,
            VimPatternNode::StartOfLine
                | VimPatternNode::EndOfLine
                | VimPatternNode::AnywhereStartOfLine
                | VimPatternNode::AnywhereEndOfLine
                | VimPatternNode::StartOfFile
                | VimPatternNode::EndOfFile
                | VimPatternNode::WordBoundaryStart
                | VimPatternNode::WordBoundaryEnd
                | VimPatternNode::SetMatchStart
                | VimPatternNode::SetMatchEnd
        )
    }

    /// Parse a single piece: an atom optionally followed by a quantifier,
    /// then optionally followed by a lookaround operator.
    ///
    /// Returns `None` if the atom was a modifier (e.g., `\c`, `\v`) that
    /// consumed input but produced no AST node.
    pub(super) fn parse_piece(&mut self) -> Result<Option<VimPatternNode>, VimRegexError> {
        let atom = self.parse_atom()?;

        let Some(atom) = atom else {
            return Ok(None);
        };

        self.at_branch_start = false;

        // Anchors and zero-width assertions cannot be quantified.
        // The quantifier character remains in the input as next atom (literal).
        if Self::is_unquantifiable(&atom) {
            return self.try_parse_lookaround(atom).map(Some);
        }

        // Check for quantifier suffix
        let node = self.try_parse_quantifier(atom)?;

        // Check for lookaround postfix: \@=, \@!, \@<=, \@<!, \@>
        self.try_parse_lookaround(node).map(Some)
    }

    /// If the current position is at a `\@` lookaround operator, wrap the
    /// preceding node in a Lookaround. Otherwise return the node unchanged.
    fn try_parse_lookaround(
        &mut self,
        node: VimPatternNode,
    ) -> Result<VimPatternNode, VimRegexError> {
        // Lookaround is always \@ regardless of magic mode
        if self.peek() != Some('\\') || self.peek_at(1) != Some('@') {
            return Ok(node);
        }

        let backslash_pos = self.pos;
        self.advance(); // consume '\'
        self.advance(); // consume '@'

        self.parse_lookaround_kind(node, backslash_pos)
    }

    /// Parse the lookaround kind after `\@`: `=`, `!`, `<=`, `<!`, `>`,
    /// or a numeric limit followed by `<=`/`<!`.
    fn parse_lookaround_kind(
        &mut self,
        node: VimPatternNode,
        backslash_pos: usize,
    ) -> Result<VimPatternNode, VimRegexError> {
        self.features.has_lookaround = true;

        match self.peek() {
            Some('=') => {
                self.advance();
                Ok(VimPatternNode::Lookaround {
                    inner: Box::new(node),
                    kind: LookaroundKind::PositiveAhead,
                    limit: None,
                })
            }
            Some('!') => {
                self.advance();
                Ok(VimPatternNode::Lookaround {
                    inner: Box::new(node),
                    kind: LookaroundKind::NegativeAhead,
                    limit: None,
                })
            }
            Some('>') => {
                self.advance();
                self.features.has_atomic = true;
                Ok(VimPatternNode::Lookaround {
                    inner: Box::new(node),
                    kind: LookaroundKind::Atomic,
                    limit: None,
                })
            }
            Some('<') => {
                self.advance();
                self.parse_lookbehind(node, None)
            }
            Some(c) if c.is_ascii_digit() => {
                let limit = self.parse_decimal_number();
                self.parse_lookbehind_with_limit(node, limit, backslash_pos)
            }
            _ => Err(VimRegexErrorKind::InvalidEscape {
                span: self.span_from(backslash_pos),
                ch: '@',
            }
            .into()),
        }
    }

    /// Parse `<=` or `<!` after the `<` of a lookbehind.
    fn parse_lookbehind(
        &mut self,
        node: VimPatternNode,
        limit: Option<u32>,
    ) -> Result<VimPatternNode, VimRegexError> {
        match self.peek() {
            Some('=') => {
                self.advance();
                Ok(VimPatternNode::Lookaround {
                    inner: Box::new(node),
                    kind: LookaroundKind::PositiveBehind,
                    limit,
                })
            }
            Some('!') => {
                self.advance();
                Ok(VimPatternNode::Lookaround {
                    inner: Box::new(node),
                    kind: LookaroundKind::NegativeBehind,
                    limit,
                })
            }
            _ => Err(VimRegexErrorKind::InvalidEscape {
                span: self.span_at(self.pos),
                ch: '<',
            }
            .into()),
        }
    }

    /// Parse lookbehind with a numeric limit: `\@123<=` or `\@123<!`.
    fn parse_lookbehind_with_limit(
        &mut self,
        node: VimPatternNode,
        limit: u32,
        backslash_pos: usize,
    ) -> Result<VimPatternNode, VimRegexError> {
        if self.peek() == Some('<') {
            self.advance();
            self.parse_lookbehind(node, Some(limit))
        } else {
            Err(VimRegexErrorKind::InvalidEscape {
                span: self.span_from(backslash_pos),
                ch: '@',
            }
            .into())
        }
    }

    /// Parse a decimal number from the current position.
    ///
    /// Consumes all consecutive ASCII digits and returns the parsed value.
    /// Returns 0 if no digits are present (caller should check).
    pub(super) fn parse_decimal_number(&mut self) -> u32 {
        let mut n: u32 = 0;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                n = n
                    .saturating_mul(10)
                    .saturating_add(u32::from(c as u8 - b'0'));
                self.advance();
            } else {
                break;
            }
        }
        n
    }

    /// Parse an octal number (digits 0-7) from the current position.
    ///
    /// Consumes all consecutive octal digits and returns the parsed value.
    /// Returns 0 if no octal digits are present.
    pub(super) fn parse_octal_number(&mut self) -> u32 {
        let mut n: u32 = 0;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() && c <= '7' {
                n = n
                    .saturating_mul(8)
                    .saturating_add(u32::from(c as u8 - b'0'));
                self.advance();
            } else {
                break;
            }
        }
        n
    }

    /// Parse exactly `count` hex digits from the current position.
    ///
    /// Consumes up to `count` consecutive hex digits and returns the parsed value.
    /// Returns 0 if fewer than `count` digits are available (stops early).
    pub(super) fn parse_hex_digits(&mut self, count: usize) -> u32 {
        let mut n: u32 = 0;
        for _ in 0..count {
            if let Some(c) = self.peek() {
                if let Some(digit) = c.to_digit(16) {
                    n = n.saturating_mul(16).saturating_add(digit);
                    self.advance();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        n
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHARACTER CLASS HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Try to parse a character class from the char after `\`.
    ///
    /// Returns `Some(node)` if `ch` maps to a known class, `None` otherwise.
    /// Does NOT advance the position — the caller must do that.
    pub(super) fn try_parse_char_class(&self, ch: char) -> Option<VimPatternNode> {
        super::char_to_class(ch).map(VimPatternNode::Class)
    }

    /// Try to parse an escape sequence kind from the char after `\`.
    ///
    /// Returns `Some(kind)` if `ch` is a recognized escape, `None` otherwise.
    pub(super) const fn try_parse_escape_kind(ch: char) -> Option<EscapeKind> {
        match ch {
            'n' => Some(EscapeKind::Newline),
            't' => Some(EscapeKind::Tab),
            'r' => Some(EscapeKind::Return),
            'e' => Some(EscapeKind::Escape),
            'b' => Some(EscapeKind::Backspace),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-ERROR RECOVERY INFRASTRUCTURE
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Record a non-fatal error for later retrieval.
    ///
    /// Used during multi-error recovery mode. The error is stored
    /// rather than returned immediately, allowing parsing to continue.
    #[allow(dead_code, reason = "infrastructure for future multi-error recovery")]
    pub(super) fn record_error(&mut self, error: VimRegexError) {
        self.errors.push(error);
    }

    /// Synchronize the parser to a recovery point after an error.
    ///
    /// Advances past characters until one of the recovery tokens is found:
    /// `]` (close collection), `\)` (close group), `\|` (alternation),
    /// or end of pattern. Does NOT consume the recovery token itself.
    ///
    /// Returns `true` if a recovery token was found, `false` if end-of-input
    /// was reached.
    #[allow(dead_code, reason = "infrastructure for future multi-error recovery")]
    pub(super) fn synchronize(&mut self) -> bool {
        while let Some(ch) = self.peek() {
            match ch {
                ']' => return true,
                '\\' => {
                    if matches!(self.peek_at(1), Some(')') | Some('|')) {
                        return true;
                    }
                    // Skip the backslash and the next char
                    self.advance();
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
        false
    }
}
