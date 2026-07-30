//! Text object types.
//!
//! Text objects select regions of text (inner word, around quotes, etc).

use strum::Display;

/// Whether a text object selects the inner content or includes delimiters.
///
/// In Vim, `i` selects the inner content (e.g., `ciw` changes the word),
/// while `a` selects around (e.g., `daw` deletes the word plus surrounding space).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TextObjectScope {
    /// Inner (`i`): select content between delimiters, excluding delimiters.
    Inner,
    /// Around (`a`): select content including delimiters and surrounding whitespace.
    Around,
}

impl TextObjectScope {
    /// Return the key character for this scope.
    ///
    /// `Inner` → `'i'`, `Around` → `'a'`.
    /// Used by `InputState::pending_display()` for showcmd display.
    #[inline]
    #[must_use]
    pub const fn key_char(self) -> char {
        match self {
            Self::Inner => 'i',
            Self::Around => 'a',
        }
    }

    /// Whether this is the inner variant.
    #[inline]
    #[must_use]
    pub const fn is_inner(self) -> bool {
        matches!(self, Self::Inner)
    }

    /// Create from `true` = inner, `false` = around.
    #[inline]
    #[must_use]
    pub const fn from_inner_flag(inner: bool) -> Self {
        if inner {
            Self::Inner
        } else {
            Self::Around
        }
    }
}

/// Seek direction for targets.vim next/last text objects.
///
/// When present, the cursor is repositioned to the next or last occurrence
/// of the delimiter before the normal text object is resolved.
///
/// - `Next` (`n`): search **forward** from cursor for the delimiter.
/// - `Last` (`l`): search **backward** from cursor for the delimiter.
///
/// Only meaningful for delimiter-based text objects (quotes, brackets, tags).
/// Word/WORD/sentence/paragraph/etc. do not support seeking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SeekDirection {
    /// Seek forward to the next delimiter occurrence.
    Next,
    /// Seek backward to the last (previous) delimiter occurrence.
    Last,
}

impl SeekDirection {
    /// Return the key character for this direction.
    ///
    /// `Next` → `'n'`, `Last` → `'l'`.
    /// Used by `InputState::pending_display()` for showcmd display.
    #[inline]
    #[must_use]
    pub const fn key_char(self) -> char {
        match self {
            Self::Next => 'n',
            Self::Last => 'l',
        }
    }
}

/// Text object type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextObject {
    /// Inner (i) or around (a).
    pub scope: TextObjectScope,
    /// Object kind.
    pub kind: TextObjectKind,
    /// Optional seek direction (targets.vim `n`/`l` modifier).
    ///
    /// When `Some(Next)`, cursor seeks forward to the next delimiter before
    /// resolving the text object. When `Some(Last)`, seeks backward.
    /// `None` means standard text object behavior (no seeking).
    pub seek: Option<SeekDirection>,
}

/// Text object kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextObjectKind {
    /// Semantic text object — dispatched to `SemanticTextObjectProvider` first,
    /// with syntactic fallback.
    ///
    Semantic(crate::primitives::SemanticObject),
    /// Word (w)
    Word,
    /// WORD (W)
    WORD,
    /// Sentence (s)
    Sentence,
    /// Paragraph (p)
    Paragraph,
    /// Parentheses ()/()
    Paren,
    /// Braces {}/{}
    Brace,
    /// Brackets []/[]
    Bracket,
    /// Angle brackets <>/<>
    Angle,
    /// Double quotes "
    DoubleQuote,
    /// Single quotes '
    SingleQuote,
    /// Backtick `
    Backtick,
    /// Pipe/vertical bar |
    Pipe,
    /// Tag (t)
    Tag,
    /// Function/method (f) — syntax-aware, requires `SyntaxProvider`.
    Function,
    /// Class/struct/module (c) — syntax-aware, requires `SyntaxProvider`.
    Class,
    /// Function argument/parameter (a) — syntax-aware, requires `SyntaxProvider`.
    Argument,
    /// Comment block or line (/) — syntax-aware, requires `SyntaxProvider`.
    Comment,
    /// Entire buffer (e) — vim-textobj-entire.
    ///
    /// `ie` selects the entire buffer excluding leading/trailing blank lines.
    /// `ae` selects the entire buffer unconditionally.
    Entire,
    /// Indent block (i) — vim-indent-object.
    ///
    /// `ii` selects contiguous lines at the same or deeper indent level.
    /// `ai` includes surrounding less-indented lines (above and below).
    IndentBlock,
    /// Indent block without below (I) — vim-indent-object.
    ///
    /// `iI` selects contiguous lines at the same or deeper indent level (same as `ii`).
    /// `aI` includes the header line above but does NOT extend below.
    IndentBlockNoBelow,
    /// Any bracket pair — tries `()`, `[]`, `{}`, `<>`, picks tightest.
    AnyBracket,
    /// Any quote pair — tries `'`, `"`, `` ` ``, picks tightest.
    AnyQuote,
    /// Symbol (m) — a programming symbol: word characters plus `.`, `:`, `->`, `_`, `$`, `@`, `#`.
    ///
    /// Selects a contiguous run of symbol characters, useful for selecting
    /// qualified names like `foo.bar.baz` or `std::collections::HashMap` as one unit.
    /// Similar to Evil mode's symbol text object.
    ///
    /// - `im` selects only the symbol characters.
    /// - `am` includes one side of surrounding whitespace (trailing preferred).
    Symbol,
    /// Subword (S) — camelCase/snake_case sub-part.
    ///
    /// Selects the subword under the cursor using the same boundary logic as
    /// subword motions (case transitions, separator characters, alpha-digit transitions).
    ///
    /// - `iS` selects the subword under cursor (between boundaries).
    /// - `aS` includes trailing separator/whitespace (or leading if at end).
    Subword,
    /// Host-registered custom text object (runtime extension).
    ///
    /// The `u32` is a unique ID assigned by the host when registering.
    /// The dispatch layer routes this to the registered `CustomTextObjectProvider`.
    Custom(u32),
}

impl TextObjectKind {
    /// Create a semantic text object from a key character.
    ///
    /// Maps the semantic key bindings (after `i`/`a`) to
    /// `TextObjectKind::Semantic(SemanticObject::...)`.
    ///
    /// Called before [`from_char`](Self::from_char) when the
    /// `semantic-textobjects` feature is enabled, so that semantic keys
    /// take priority over the base syntax-provider path for `f`, `c`, `a`.
    ///
    /// | Key | Object            |
    /// |-----|------------------|
    /// | `f` | Function          |
    /// | `c` | Class             |
    /// | `a` | Parameter         |
    /// | `C` | Conditional       |
    /// | `o` | Loop              |
    /// | `K` | Comment           |
    /// | `S` | Scope             |
    /// | `F` | Call              |
    /// | `T` | TypeDef           |
    /// | `R` | Return            |
    /// | `U` | Import            |
    /// | `Z` | StringLiteral     |
    #[must_use]
    pub const fn from_semantic_char(c: char) -> Option<Self> {
        use crate::primitives::SemanticObject;
        match c {
            'f' => Some(Self::Semantic(SemanticObject::Function)),
            'c' => Some(Self::Semantic(SemanticObject::Class)),
            'a' => Some(Self::Semantic(SemanticObject::Parameter)),
            'C' => Some(Self::Semantic(SemanticObject::Conditional)),
            'o' => Some(Self::Semantic(SemanticObject::Loop)),
            'K' => Some(Self::Semantic(SemanticObject::Comment)),
            'S' => Some(Self::Semantic(SemanticObject::Scope)),
            'F' => Some(Self::Semantic(SemanticObject::Call)),
            'T' => Some(Self::Semantic(SemanticObject::TypeDef)),
            'R' => Some(Self::Semantic(SemanticObject::Return)),
            'U' => Some(Self::Semantic(SemanticObject::Import)),
            'Z' => Some(Self::Semantic(SemanticObject::StringLiteral)),
            _ => None,
        }
    }

    /// Whether this text object kind supports seek modifiers (`n`/`l`).
    ///
    /// Only delimiter-based text objects (quotes, brackets, tags) support
    /// seeking. Word/WORD/sentence/paragraph/indent/etc. do not have
    /// delimiters to seek to.
    #[must_use]
    pub const fn supports_seek(&self) -> bool {
        matches!(
            self,
            Self::Paren
                | Self::Brace
                | Self::Bracket
                | Self::Angle
                | Self::DoubleQuote
                | Self::SingleQuote
                | Self::Backtick
                | Self::Pipe
                | Self::Tag
                | Self::AnyBracket
                | Self::AnyQuote
        )
    }

    /// Create from key character.
    #[must_use]
    pub const fn from_char(c: char) -> Option<Self> {
        match c {
            'w' => Some(Self::Word),
            'W' => Some(Self::WORD),
            's' => Some(Self::Sentence),
            'p' => Some(Self::Paragraph),
            '(' | ')' => Some(Self::Paren),
            '{' | '}' | 'B' => Some(Self::Brace),
            '[' | ']' => Some(Self::Bracket),
            '<' | '>' => Some(Self::Angle),
            '"' => Some(Self::DoubleQuote),
            '\'' => Some(Self::SingleQuote),
            '`' => Some(Self::Backtick),
            '|' => Some(Self::Pipe),
            't' => Some(Self::Tag),
            'f' => Some(Self::Function),
            'c' => Some(Self::Class),
            'a' => Some(Self::Argument),
            '/' => Some(Self::Comment),
            'e' => Some(Self::Entire),
            'i' => Some(Self::IndentBlock),
            'I' => Some(Self::IndentBlockNoBelow),
            'b' => Some(Self::AnyBracket),
            'q' => Some(Self::AnyQuote),
            'm' => Some(Self::Symbol),
            'S' => Some(Self::Subword),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AnyBracket / AnyQuote new mappings ──────────────────────────────

    #[test]
    fn from_char_b_returns_any_bracket() {
        assert_eq!(
            TextObjectKind::from_char('b'),
            Some(TextObjectKind::AnyBracket)
        );
    }

    #[test]
    fn from_char_q_returns_any_quote() {
        assert_eq!(
            TextObjectKind::from_char('q'),
            Some(TextObjectKind::AnyQuote)
        );
    }

    // ── Paren still works without 'b' ───────────────────────────────────

    #[test]
    fn from_char_open_paren_returns_paren() {
        assert_eq!(TextObjectKind::from_char('('), Some(TextObjectKind::Paren));
    }

    #[test]
    fn from_char_close_paren_returns_paren() {
        assert_eq!(TextObjectKind::from_char(')'), Some(TextObjectKind::Paren));
    }

    // ── Full regression check: every documented mapping ─────────────────

    #[test]
    fn from_char_regression_all_mappings() {
        let cases: &[(char, TextObjectKind)] = &[
            ('w', TextObjectKind::Word),
            ('W', TextObjectKind::WORD),
            ('s', TextObjectKind::Sentence),
            ('p', TextObjectKind::Paragraph),
            ('(', TextObjectKind::Paren),
            (')', TextObjectKind::Paren),
            ('{', TextObjectKind::Brace),
            ('}', TextObjectKind::Brace),
            ('B', TextObjectKind::Brace),
            ('[', TextObjectKind::Bracket),
            (']', TextObjectKind::Bracket),
            ('<', TextObjectKind::Angle),
            ('>', TextObjectKind::Angle),
            ('"', TextObjectKind::DoubleQuote),
            ('\'', TextObjectKind::SingleQuote),
            ('`', TextObjectKind::Backtick),
            ('|', TextObjectKind::Pipe),
            ('t', TextObjectKind::Tag),
            ('f', TextObjectKind::Function),
            ('c', TextObjectKind::Class),
            ('a', TextObjectKind::Argument),
            ('/', TextObjectKind::Comment),
            ('e', TextObjectKind::Entire),
            ('i', TextObjectKind::IndentBlock),
            ('b', TextObjectKind::AnyBracket),
            ('q', TextObjectKind::AnyQuote),
            ('m', TextObjectKind::Symbol),
            ('S', TextObjectKind::Subword),
            ('I', TextObjectKind::IndentBlockNoBelow),
        ];

        for &(ch, expected) in cases {
            assert_eq!(
                TextObjectKind::from_char(ch),
                Some(expected),
                "from_char({ch:?}) should return Some({expected:?})"
            );
        }
    }

    #[test]
    fn from_char_capital_i_returns_indent_block_no_below() {
        assert_eq!(
            TextObjectKind::from_char('I'),
            Some(TextObjectKind::IndentBlockNoBelow)
        );
    }

    #[test]
    fn from_char_pipe_returns_pipe() {
        assert_eq!(TextObjectKind::from_char('|'), Some(TextObjectKind::Pipe));
    }

    #[test]
    fn pipe_supports_seek() {
        assert!(TextObjectKind::Pipe.supports_seek());
    }

    #[test]
    fn from_char_unmapped_returns_none() {
        for ch in ['x', 'z', 'Z', '!', '0', '\n', ' '] {
            assert_eq!(
                TextObjectKind::from_char(ch),
                None,
                "from_char({ch:?}) should return None"
            );
        }
    }

    // ── TextObjectScope::key_char() ─────────────────────────────────────

    #[test]
    fn scope_key_char_inner() {
        assert_eq!(TextObjectScope::Inner.key_char(), 'i');
    }

    #[test]
    fn scope_key_char_around() {
        assert_eq!(TextObjectScope::Around.key_char(), 'a');
    }

    #[test]
    fn scope_key_char_round_trips_with_is_inner() {
        assert_eq!(TextObjectScope::from_inner_flag(true).key_char(), 'i');
        assert_eq!(TextObjectScope::from_inner_flag(false).key_char(), 'a');
    }
}
