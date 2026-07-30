//! Quantifier parsing for the Vim regex parser.
//!
//! Handles `*`, `\+`, `\?`, `\=`, `\{n,m}`, `\{-n,m}` and their VeryMagic
//! equivalents (`+`, `?`, `=`, `{n,m}`, `{-n,m}`).

use crate::MagicMode;

use super::{Parser, VimPatternNode, VimRegexError, VimRegexErrorKind};

/// `{` as char. Written as an escape rather than a bare `'{'` literal so that
/// tooling which measures function length by counting braces is not thrown off
/// by braces that are data rather than delimiters.
const OPEN_BRACE: char = '\x7B';

/// `}` as char — same rationale as `OPEN_BRACE`.
const CLOSE_BRACE: char = '\x7D';

// ═══════════════════════════════════════════════════════════════════════════════
// QUANTIFIER PARSING
// ═══════════════════════════════════════════════════════════════════════════════

impl Parser {
    /// Try to parse a quantifier after an atom.
    ///
    /// If a quantifier follows, wraps the atom in a `Quantifier` node.
    /// Otherwise returns the atom unchanged.
    pub(super) fn try_parse_quantifier(
        &mut self,
        atom: VimPatternNode,
    ) -> Result<VimPatternNode, VimRegexError> {
        if self.at_star_quantifier() {
            self.advance_quantifier_star();
            return Ok(VimPatternNode::Quantifier {
                node: Box::new(atom),
                min: 0,
                max: None,
                greedy: true,
            });
        }

        if let Some(kind) = self.at_backslash_quantifier() {
            return self.parse_backslash_quantifier(atom, kind);
        }

        if self.at_verymagic_quantifier() {
            return self.parse_verymagic_quantifier(atom);
        }

        Ok(atom)
    }

    /// Check if we're at a `*` quantifier (meta in Magic/VeryMagic).
    fn at_star_quantifier(&self) -> bool {
        match self.magic {
            MagicMode::VeryMagic | MagicMode::Magic => self.peek() == Some('*'),
            MagicMode::NoMagic | MagicMode::VeryNoMagic => {
                self.peek() == Some('\\') && self.peek_at(1) == Some('*')
            }
        }
    }

    /// Advance past a `*` quantifier.
    fn advance_quantifier_star(&mut self) {
        match self.magic {
            MagicMode::VeryMagic | MagicMode::Magic => {
                self.advance(); // consume '*'
            }
            MagicMode::NoMagic | MagicMode::VeryNoMagic => {
                self.advance(); // consume '\'
                self.advance(); // consume '*'
            }
        }
    }

    /// Check if we're at a `\+`, `\?`, `\=`, or `\{` quantifier.
    /// Returns the character after the backslash if so.
    fn at_backslash_quantifier(&self) -> Option<char> {
        // In VeryMagic, \+ \? \= \{ are literals (backslash removes magic).
        // Only bare +, ?, {  are quantifiers (handled by at_verymagic_quantifier).
        if self.magic == MagicMode::VeryMagic {
            return None;
        }
        if self.peek() != Some('\\') {
            return None;
        }

        let next = self.peek_at(1)?;
        if matches!(next, '+' | '?' | '=') || next == OPEN_BRACE {
            Some(next)
        } else {
            None
        }
    }

    /// Parse a quantifier starting with `\`: `\+`, `\?`, `\=`, `\{n,m}`.
    fn parse_backslash_quantifier(
        &mut self,
        atom: VimPatternNode,
        kind: char,
    ) -> Result<VimPatternNode, VimRegexError> {
        self.advance(); // consume '\'
        self.advance(); // consume the quantifier char

        if kind == OPEN_BRACE {
            return self.parse_brace_quantifier_body(atom);
        }

        match kind {
            '+' => Ok(VimPatternNode::Quantifier {
                node: Box::new(atom),
                min: 1,
                max: None,
                greedy: true,
            }),
            // '?' and '=' are both optional (0-or-1)
            _ => Ok(VimPatternNode::Quantifier {
                node: Box::new(atom),
                min: 0,
                max: Some(1),
                greedy: true,
            }),
        }
    }

    /// Check if we're at a VeryMagic-mode bare quantifier: `+`, `?`, `=`, `{`.
    fn at_verymagic_quantifier(&self) -> bool {
        if self.magic != MagicMode::VeryMagic {
            return false;
        }
        matches!(self.peek(), Some('+' | '?' | '=')) || self.peek() == Some(OPEN_BRACE)
    }

    /// Parse a VeryMagic bare quantifier: `+`, `?`, `=`, `{`.
    fn parse_verymagic_quantifier(
        &mut self,
        atom: VimPatternNode,
    ) -> Result<VimPatternNode, VimRegexError> {
        // SAFETY: caller (at_verymagic_quantifier) verified peek() matches.
        #[allow(clippy::unwrap_used, reason = "caller verified peek() is Some")]
        let ch = self.advance().unwrap();

        if ch == OPEN_BRACE {
            return self.parse_brace_quantifier_body(atom);
        }

        match ch {
            '+' => Ok(VimPatternNode::Quantifier {
                node: Box::new(atom),
                min: 1,
                max: None,
                greedy: true,
            }),
            // '?' and '=' are both optional (0-or-1)
            _ => Ok(VimPatternNode::Quantifier {
                node: Box::new(atom),
                min: 0,
                max: Some(1),
                greedy: true,
            }),
        }
    }

    /// Parse the body of a brace quantifier after the opening brace.
    ///
    /// Supports: `n`, `n,`, `n,m`, `,m`, empty (= `*`),
    /// and non-greedy variants with `-`.
    pub(super) fn parse_brace_quantifier_body(
        &mut self,
        atom: VimPatternNode,
    ) -> Result<VimPatternNode, VimRegexError> {
        let brace_pos = self.pos.saturating_sub(1);

        // Check for non-greedy flag
        let greedy = if self.peek() == Some('-') {
            self.advance();
            false
        } else {
            true
        };

        // Check for closing brace immediately
        if self.at_brace_close() {
            self.consume_brace_close();
            return Ok(VimPatternNode::Quantifier {
                node: Box::new(atom),
                min: 0,
                max: None,
                greedy,
            });
        }

        let (min, max) = self.parse_brace_range(brace_pos)?;

        if !self.at_brace_close() {
            return Err(VimRegexErrorKind::InvalidQuantifier {
                span: self.span_from(brace_pos),
                detail: "missing closing brace".into(),
            }
            .into());
        }
        self.consume_brace_close();

        Ok(VimPatternNode::Quantifier {
            node: Box::new(atom),
            min,
            max,
            greedy,
        })
    }

    /// Parse the `n,m` range inside a brace quantifier.
    ///
    /// Returns `(min, max)`.
    fn parse_brace_range(&mut self, brace_pos: usize) -> Result<(u32, Option<u32>), VimRegexError> {
        let has_first_num = self.peek().is_some_and(|c| c.is_ascii_digit());
        let first = if has_first_num {
            self.parse_decimal_number()
        } else {
            0
        };

        if self.peek() == Some(',') {
            self.advance(); // consume ','
            Ok(self.parse_brace_range_after_comma(first))
        } else if has_first_num {
            // Exact count
            Ok((first, Some(first)))
        } else {
            Err(VimRegexErrorKind::InvalidQuantifier {
                span: self.span_from(brace_pos),
                detail: "expected number or comma in quantifier".into(),
            }
            .into())
        }
    }

    /// Parse the range after the comma.
    fn parse_brace_range_after_comma(&mut self, min: u32) -> (u32, Option<u32>) {
        if self.peek().is_some_and(|c| c.is_ascii_digit()) {
            let max = self.parse_decimal_number();
            if min > max {
                (max, Some(min)) // Vim swaps min/max
            } else {
                (min, Some(max))
            }
        } else {
            // Unbounded max
            (min, None)
        }
    }

    /// Check if we're at a closing brace.
    fn at_brace_close(&self) -> bool {
        if self.peek() == Some(CLOSE_BRACE) {
            return true;
        }
        // Vim allows \} as closing: "Allow either \{...} or \{...\}"
        self.peek() == Some('\\') && self.peek_at(1) == Some(CLOSE_BRACE)
    }

    /// Consume the closing brace (and optional preceding backslash).
    fn consume_brace_close(&mut self) {
        if self.peek() == Some('\\') {
            self.advance(); // consume optional '\'
        }
        self.advance(); // consume '}'
    }
}
