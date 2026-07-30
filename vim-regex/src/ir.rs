//! Intermediate representation for Vim regex patterns.
//!
//! The AST types in this module represent a parsed Vim regex pattern.
//! The parser converts a raw pattern string into a tree of `VimPatternNode`
//! values that the compiler can translate into executable matching logic.
//!
//! All types are internal to the regex engine — no serde derives needed.

use compact_str::CompactString;
use std::fmt;

// ═══════════════════════════════════════════════════════════════════════════════
// PRIMARY AST NODE
// ═══════════════════════════════════════════════════════════════════════════════

/// A single node in the parsed Vim regex pattern tree.
///
/// Each variant corresponds to a Vim regex construct. The parser produces a
/// tree of these nodes; the compiler walks the tree to generate matching code.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum VimPatternNode {
    /// A literal character to match exactly.
    Literal(char),
    /// `.` — matches any character except newline.
    AnyChar,
    /// `\_.` — matches any character including newline.
    AnyCharNl,
    /// `^` — matches at the start of a line.
    StartOfLine,
    /// `$` — matches at the end of a line.
    EndOfLine,
    /// `\_^` — matches at start-of-line at any position in the pattern.
    /// Unlike `^`, this is never treated as literal by context-sensitivity.
    AnywhereStartOfLine,
    /// `\_$` — matches at end-of-line at any position in the pattern.
    /// Unlike `$`, this is never treated as literal by context-sensitivity.
    AnywhereEndOfLine,
    /// `\%^` — matches at the start of the file (buffer).
    StartOfFile,
    /// `\%$` — matches at the end of the file (buffer).
    EndOfFile,
    /// `\<` — matches at the start of a word boundary.
    WordBoundaryStart,
    /// `\>` — matches at the end of a word boundary.
    WordBoundaryEnd,
    /// `\zs` — sets the start of the match (excludes preceding atoms from match).
    SetMatchStart,
    /// `\ze` — sets the end of the match (excludes following atoms from match).
    SetMatchEnd,
    /// A sequence of atoms matched left to right.
    Sequence(Vec<Self>),
    /// `\|` — alternation: matches any one of the branches.
    Alternation(Vec<Self>),
    /// `\&` — branch-and: all branches must match at the same position.
    /// Only the last branch's extent determines the match length.
    BranchAnd(Vec<Self>),
    /// `\(` ... `\)` or `\%(` ... `\)` — grouped subexpression.
    Group {
        /// The inner pattern of the group.
        inner: Box<Self>,
        /// `true` for `\(` (capturing), `false` for `\%(` (non-capturing).
        capturing: bool,
    },
    /// `*`, `\+`, `\?`, `\{n,m}`, `\{-}` etc. — repetition quantifier.
    Quantifier {
        /// The atom being quantified.
        node: Box<Self>,
        /// Minimum number of repetitions.
        min: u32,
        /// Maximum number of repetitions (`None` = unbounded).
        max: Option<u32>,
        /// `true` for greedy (default), `false` for non-greedy (`\{-}`).
        greedy: bool,
    },
    /// `\d`, `\w`, `\s`, etc. — a character class (ASCII-only).
    Class(CharClass),
    /// `\_d`, `\_w`, `\_s`, etc. — a character class that also matches newline.
    ClassWithNewline(CharClass),
    /// `[...]` or `[^...]` — a collection (character set).
    Collection {
        /// `true` for `[^...]` (negated).
        negated: bool,
        /// The items in the collection.
        items: Vec<CollectionItem>,
        /// `true` for `\_[...]` (also matches newline).
        include_newline: bool,
    },
    /// `\1` .. `\9` — back-reference to a captured group.
    BackReference(u8),
    /// `\@=`, `\@!`, `\@<=`, `\@<!`, `\@>` — lookaround and atomic groups.
    Lookaround {
        /// The inner pattern of the lookaround.
        inner: Box<Self>,
        /// The kind of lookaround assertion.
        kind: LookaroundKind,
        /// Optional limit for lookbehind (e.g., `\@123<=`).
        limit: Option<u32>,
    },
    /// `\%#` — matches at the cursor position.
    CursorPosition,
    /// `\%V` — matches inside the Visual area.
    VisualArea,
    /// `\%23l`, `\%<23l`, `\%>23l`, `\%.l` — matches at a specific line.
    AtLine(LineSpec),
    /// `\%23c`, `\%<23c`, `\%>23c` — matches at a specific column.
    AtColumn(ColumnSpec),
    /// `\%23v`, `\%<23v`, `\%>23v` — matches at a specific virtual column.
    AtVirtualColumn(ColumnSpec),
    /// `\%'m`, `\%<'m`, `\%>'m` — matches at the position of a mark.
    AtMark {
        /// The mark character (a-z, A-Z, etc.).
        mark: char,
        /// Relationship to the mark position.
        rel: MarkRel,
    },
    /// `\%d123`, `\%x2a`, `\%u20AC`, `\%U1234abcd` — matches a character by its code point.
    CharByCode(char),
    /// `~` — matches the last substitute string.
    LastSubstitute,
    /// `\%[atoms]` — an optionally matched sequence of atoms.
    OptionalSequence(Vec<Self>),
    /// `\n`, `\t`, `\r`, `\e`, `\b` — an escape sequence representing a specific character.
    EscapeSequence(EscapeKind),
    /// `\%C` — matches any composing (combining) character (Unicode category M).
    AnyComposing,
    /// A placeholder node inserted by the parser during error recovery.
    ///
    /// When multi-error recovery is enabled, the parser replaces unrecognized
    /// or invalid constructs with this node instead of aborting. The compiler
    /// treats `ErrorPlaceholder` as a zero-width always-match node so that
    /// the rest of the pattern can still be analyzed for additional errors.
    ///
    /// This node never appears in the output of a successful parse.
    ErrorPlaceholder,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CHARACTER CLASSES
// ═══════════════════════════════════════════════════════════════════════════════

/// ASCII character classes used in Vim regex.
///
/// Each variant has a positive form (e.g., `\d` = digit) and a negated form
/// (e.g., `\D` = non-digit). All matching is ASCII-only per Vim semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CharClass {
    /// `\d` — digit: `[0-9]`.
    Digit,
    /// `\D` — non-digit: `[^0-9]`.
    NotDigit,
    /// `\w` — word character: `[0-9A-Za-z_]`.
    Word,
    /// `\W` — non-word character: `[^0-9A-Za-z_]`.
    NotWord,
    /// `\s` — whitespace: `[ \t]`.
    Whitespace,
    /// `\S` — non-whitespace: `[^ \t]`.
    NotWhitespace,
    /// `\a` — alphabetic: `[A-Za-z]`.
    Alpha,
    /// `\A` — non-alphabetic: `[^A-Za-z]`.
    NotAlpha,
    /// `\l` — lowercase: `[a-z]`.
    Lower,
    /// `\L` — non-lowercase: `[^a-z]`.
    NotLower,
    /// `\u` — uppercase: `[A-Z]`.
    Upper,
    /// `\U` — non-uppercase: `[^A-Z]`.
    NotUpper,
    /// `\x` — hexadecimal digit: `[0-9A-Fa-f]`.
    Hex,
    /// `\X` — non-hexadecimal digit: `[^0-9A-Fa-f]`.
    NotHex,
    /// `\h` — head of word character: `[A-Za-z_]`.
    Head,
    /// `\H` — non-head of word character: `[^A-Za-z_]`.
    NotHead,
    /// `\f` — file name character: `[A-Za-z0-9/._-]` (platform-dependent in Vim).
    FileName,
    /// `\k` — keyword character (same as word for ASCII).
    Keyword,
    /// `\K` — keyword character, no digit: `[A-Za-z_]`.
    KeywordNoDigit,
    /// `\i` — identifier character: `[0-9A-Za-z_]`.
    Ident,
    /// `\I` — identifier character, no digit: `[A-Za-z_]`.
    SIdent,
    /// `\p` — printable character (non-control, non-DEL).
    Print,
    /// `\P` — printable character, no digit.
    SPrint,
    /// `\o` — octal digit: `[0-7]`.
    Octal,
    /// `\O` — non-octal digit: `[^0-7]`.
    NOctal,
    /// `\F` — file name character, no digit.
    FileNameNoDigit,
    /// `\%C` — composing (combining) character: Unicode general category M.
    Composing,
}

// ═══════════════════════════════════════════════════════════════════════════════
// COLLECTION ITEMS
// ═══════════════════════════════════════════════════════════════════════════════

/// An item inside a `[...]` collection (character set).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CollectionItem {
    /// A single character, e.g., `a` in `[abc]`.
    Single(char),
    /// A character range, e.g., `a-z` in `[a-z]`.
    Range(char, char),
    /// A character class inside a collection, e.g., `\d` in `[\d]`.
    Class(CharClass),
    /// A POSIX named class inside a collection, e.g., `[:alpha:]` in `[[:alpha:]]`.
    PosixClass(PosixClassName),
    /// A literal newline inside a collection.
    Newline,
}

/// POSIX named character classes for use inside `[...]` collections.
///
/// These correspond to `[:name:]` syntax inside bracket expressions,
/// e.g., `[[:alpha:]]` matches any alphabetic character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PosixClassName {
    /// `[:alnum:]` — alphanumeric: `[0-9A-Za-z]`.
    Alnum,
    /// `[:alpha:]` — alphabetic: `[A-Za-z]`.
    Alpha,
    /// `[:blank:]` — blank: space or tab.
    Blank,
    /// `[:cntrl:]` — control characters.
    Cntrl,
    /// `[:digit:]` — digits: `[0-9]`.
    Digit,
    /// `[:graph:]` — graphic (printable, non-space).
    Graph,
    /// `[:lower:]` — lowercase letters: `[a-z]`.
    Lower,
    /// `[:print:]` — printable characters (including space).
    Print,
    /// `[:punct:]` — punctuation.
    Punct,
    /// `[:space:]` — whitespace.
    Space,
    /// `[:upper:]` — uppercase letters: `[A-Z]`.
    Upper,
    /// `[:xdigit:]` — hexadecimal digits: `[0-9A-Fa-f]`.
    Xdigit,
    /// `[:tab:]` — tab character.
    Tab,
    /// `[:return:]` — carriage return.
    Return,
    /// `[:backspace:]` — backspace character.
    Backspace,
    /// `[:escape:]` — escape character.
    Escape,
    /// `[:ident:]` — identifier characters: `[0-9A-Za-z_]`.
    Ident,
    /// `[:keyword:]` — keyword characters: `[0-9A-Za-z_]`.
    Keyword,
    /// `[:fname:]` — file name characters: `[0-9A-Za-z_/.-]`.
    Fname,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PERCENT ESCAPE CONTEXT
// ═══════════════════════════════════════════════════════════════════════════════

/// Context for `\%` escape errors — what specifically went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PercentEscapeContext {
    /// `\%` at end of pattern — no character follows `%`.
    EndOfInput,
    /// `\%'` not followed by a mark character.
    MissingMarkChar,
    /// `\%<` or `\%>` not followed by a number, `'m`, or `.`.
    MissingRelationTarget,
    /// `\%<number>X` or `\%.X` where X is not `l`, `c`, or `v`.
    InvalidPositionSuffix,
    /// `\%<number>` or `\%.` at end of input — no l/c/v suffix.
    MissingPositionSuffix,
    /// `\%X` where X is not a recognized dispatch character.
    UnrecognizedChar,
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND
// ═══════════════════════════════════════════════════════════════════════════════

/// The kind of lookaround assertion or atomic group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum LookaroundKind {
    /// `\@=` — positive lookahead (match without consuming).
    PositiveAhead,
    /// `\@!` — negative lookahead (fail if match).
    NegativeAhead,
    /// `\@<=` — positive lookbehind (preceded by match).
    PositiveBehind,
    /// `\@<!` — negative lookbehind (not preceded by match).
    NegativeBehind,
    /// `\@>` — atomic group (no backtracking).
    Atomic,
}

// ═══════════════════════════════════════════════════════════════════════════════
// POSITION SPECIFIERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Line position specifier for `\%l` constructs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum LineSpec {
    /// `\%23l` — exactly at line 23.
    Exact(u32),
    /// `\%<23l` — before line 23.
    Before(u32),
    /// `\%>23l` — after line 23.
    After(u32),
    /// `\%.l` — at the current line (cursor line).
    Current,
    /// `\%<.l` — before the current line.
    BeforeCurrent,
    /// `\%>.l` — after the current line.
    AfterCurrent,
}

/// Column position specifier for `\%c` and `\%v` constructs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ColumnSpec {
    /// `\%23c` / `\%23v` — exactly at column 23.
    Exact(u32),
    /// `\%<23c` / `\%<23v` — before column 23.
    Before(u32),
    /// `\%>23c` / `\%>23v` — after column 23.
    After(u32),
    /// `\%.c` / `\%.v` — at the current column / virtual column.
    Current,
    /// `\%<.c` / `\%<.v` — before the current column / virtual column.
    BeforeCurrent,
    /// `\%>.c` / `\%>.v` — after the current column / virtual column.
    AfterCurrent,
}

/// Relationship of a `\%'m` mark anchor to the mark position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MarkRel {
    /// `\%'m` — at the mark position.
    At,
    /// `\%<'m` — before the mark position.
    Before,
    /// `\%>'m` — after the mark position.
    After,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ESCAPE SEQUENCES
// ═══════════════════════════════════════════════════════════════════════════════

/// Escape sequence kinds for `\n`, `\t`, `\r`, `\e`, `\b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum EscapeKind {
    /// `\n` — newline (0x0A).
    Newline,
    /// `\t` — tab (0x09).
    Tab,
    /// `\r` — carriage return (0x0D).
    Return,
    /// `\e` — escape (0x1B).
    Escape,
    /// `\b` — backspace (0x08).
    Backspace,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CASE MODE
// ═══════════════════════════════════════════════════════════════════════════════

/// Case sensitivity mode for pattern matching.
///
/// Set by `\c` (insensitive) and `\C` (sensitive) modifiers in the pattern.
/// `Default` means the engine should use the global `ignorecase` / `smartcase` settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CaseMode {
    /// `\C` — force case-sensitive matching.
    Sensitive,
    /// `\c` — force case-insensitive matching.
    Insensitive,
    /// No explicit modifier — defer to `ignorecase` / `smartcase` options.
    #[default]
    Default,
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMPOSING MODE
// ═══════════════════════════════════════════════════════════════════════════════

/// Composing character handling mode.
///
/// Set by `\Z` modifier in the pattern. When `Ignore`, the engine skips
/// Unicode combining marks (category M) after matching each base character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ComposingMode {
    /// Default: composing characters are significant.
    #[default]
    Respect,
    /// `\Z`: ignore composing characters (skip combining marks).
    Ignore,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PATTERN FEATURES
// ═══════════════════════════════════════════════════════════════════════════════

/// Feature flags extracted during parsing.
///
/// The compiler uses these to select the appropriate matching strategy.
/// For example, patterns with back-references cannot use a simple NFA.
///
/// Each boolean flag indicates whether a particular advanced feature was
/// detected in the pattern. This is a data-oriented flags struct, not a
/// state machine — the bools are independent and orthogonal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
#[allow(
    clippy::struct_excessive_bools,
    reason = "feature flags are independent orthogonal booleans"
)]
pub struct PatternFeatures {
    /// Pattern contains `\1`..`\9` back-references.
    pub has_backreferences: bool,
    /// Pattern contains `\@=`, `\@!`, `\@<=`, `\@<!`, or `\@>`.
    pub has_lookaround: bool,
    /// Pattern contains `\@>` — atomic groups (consume input, need backtracker).
    pub has_atomic: bool,
    /// Pattern contains `\%l`, `\%c`, `\%v`, `\%#`, `\%V`, or `\%'m`.
    pub has_buffer_position: bool,
    /// Pattern contains `\zs` or `\ze`.
    pub has_match_override: bool,
    /// Pattern contains `~` (last substitute string).
    pub has_last_substitute: bool,
    /// Pattern contains `\n`, `\_x`, or `\_.` — can match across line boundaries.
    pub has_multiline: bool,
    /// Pattern contains `\&` (branch-and operator).
    pub has_branch_and: bool,
    /// Number of capturing groups (`\(` ... `\)`), 0..9.
    pub capture_count: u8,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PARSE RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// The result of successfully parsing a Vim regex pattern.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParseResult {
    /// The root node of the parsed pattern tree.
    pub node: VimPatternNode,
    /// Case sensitivity mode extracted from pattern modifiers (`\c`, `\C`).
    pub case_mode: CaseMode,
    /// Composing character mode extracted from pattern modifier (`\Z`).
    pub composing_mode: ComposingMode,
    /// Feature flags detected during parsing.
    pub features: PatternFeatures,
    /// Additional errors collected during multi-error recovery.
    ///
    /// Empty when the parser is in its default single-error mode.
    /// When non-empty, the primary error was already returned as `Err(...)`.
    /// These are secondary errors found after recovery.
    pub additional_errors: Vec<VimRegexError>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERRORS
// ═══════════════════════════════════════════════════════════════════════════════

/// A byte-offset range into the original regex pattern string.
///
/// Used in error reporting to indicate which portion of the pattern triggered
/// the error. Both `start` and `end` are byte offsets; `end` is exclusive.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Span {
    /// Byte offset of the start (inclusive) in the original pattern string.
    pub start: usize,
    /// Byte offset of the end (exclusive) in the original pattern string.
    pub end: usize,
}

impl Span {
    /// Create a new span from start (inclusive) to end (exclusive).
    #[inline]
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "Span start {start} > end {end}");
        Self { start, end }
    }

    /// Create a single-byte span (useful for single-char errors like trailing backslash).
    #[inline]
    pub fn at(pos: usize) -> Self {
        Self {
            start: pos,
            end: pos.saturating_add(1),
        }
    }
}

/// A two-location annotation for diagnostics that reference two pattern regions.
///
/// Used for errors like "duplicate group" or "conflicting flag" that need to
/// point at both the original and the conflicting location.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AuxiliarySpan {
    /// The primary error location.
    pub primary: Span,
    /// The secondary (related) location.
    pub auxiliary: Span,
    /// A label describing the relationship of the auxiliary span.
    /// For example: "first defined here", "previous declaration".
    pub auxiliary_label: &'static str,
}

impl AuxiliarySpan {
    /// Create a new auxiliary span annotation.
    pub fn new(primary: Span, auxiliary: Span, auxiliary_label: &'static str) -> Self {
        Self {
            primary,
            auxiliary,
            auxiliary_label,
        }
    }
}

/// The specific kind of Vim regex error.
///
/// Each variant includes a `Span` indicating the byte range in the original
/// pattern string where the error was detected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum VimRegexErrorKind {
    /// Unmatched `\(` or `\)` — a group was opened but never closed, or vice versa.
    UnmatchedGroup {
        /// Span of the unmatched delimiter.
        span: Span,
    },
    /// Invalid quantifier syntax (e.g., `\{abc}`).
    InvalidQuantifier {
        /// Span of the quantifier.
        span: Span,
        /// Description of what went wrong.
        detail: CompactString,
    },
    /// Invalid escape sequence (e.g., `\z` with no valid continuation).
    InvalidEscape {
        /// Span covering the `\` and the invalid character.
        span: Span,
        /// The character after the backslash.
        ch: char,
    },
    /// Invalid `\%` escape sequence with specific context.
    InvalidPercentEscape {
        /// Span from the `\` through the invalid position.
        span: Span,
        /// What specifically went wrong.
        found: PercentEscapeContext,
    },
    /// Unterminated `[...]` collection — missing closing `]`.
    UnterminatedCollection {
        /// Span from the opening `[` to end of pattern.
        span: Span,
    },
    /// Invalid range inside a collection (e.g., `[z-a]`).
    InvalidCollectionRange {
        /// Span of the offending range expression.
        span: Span,
        /// Description of the range error.
        detail: CompactString,
    },
    /// Unknown POSIX class name inside `[:name:]`.
    UnknownPosixClass {
        /// Span of the `[:name:]` expression.
        span: Span,
        /// The unrecognized class name.
        name: CompactString,
    },
    /// Invalid character code inside a `[...]` collection.
    InvalidCollectionCharCode {
        /// Span of the invalid char code escape.
        span: Span,
        /// Description of what went wrong (e.g., which base and why the code is invalid).
        detail: CompactString,
    },
    /// Invalid character code in `\%d`, `\%x`, `\%u`, or `\%U`.
    InvalidCharCode {
        /// Span of the `\%` prefix through the invalid code.
        span: Span,
        /// Description of what went wrong.
        detail: CompactString,
    },
    /// Character code value exceeds maximum Unicode code point (U+10FFFF).
    ///
    /// Triggered by `\%d`, `\%x`, `\%u`, `\%U` with a value > 0x10FFFF.
    CharCodeOutOfRange {
        /// Span from the `\%` prefix through the invalid code digits.
        span: Span,
        /// The parsed numeric value that exceeded the maximum.
        value: u32,
    },
    /// Character code value falls in the UTF-16 surrogate range (U+D800..U+DFFF).
    ///
    /// Triggered by `\%d`, `\%x`, `\%u`, `\%U` with a value in 0xD800..0xDFFF.
    /// These code points are reserved for UTF-16 surrogate pairs and are not
    /// valid Unicode scalar values.
    CharCodeSurrogate {
        /// Span from the `\%` prefix through the invalid code digits.
        span: Span,
        /// The parsed numeric value in the surrogate range.
        value: u32,
    },
    /// Pattern ends with a trailing backslash.
    TrailingBackslash {
        /// Span of the trailing `\`.
        span: Span,
    },
    /// Empty pattern string.
    EmptyPattern,
    /// Pattern is too complex for the engine.
    PatternTooComplex {
        /// Span of the construct that exceeded limits (if identifiable).
        span: Option<Span>,
        /// Description of the complexity issue.
        detail: CompactString,
    },
    /// `\=` sub-replace expression — not supported in pattern matching.
    ExpressionReplacementNotSupported,
    /// Invalid atom inside `\%[...]` optional sequence.
    InvalidOptionalSequenceAtom {
        /// Span of the `\%[` prefix.
        span: Span,
    },
    /// Internal engine error — should never occur in normal operation.
    ///
    /// Replaces `unreachable!()` in non-mathematical-impossibility cases,
    /// turning panics into recoverable errors.
    InternalError {
        /// Description of what went wrong.
        detail: CompactString,
    },
    /// Runtime/search error: the haystack is too large for the backtracker's
    /// bounded memoization (backreference / atomic / last-substitute patterns).
    /// Raised instead of silently producing a wrong answer. `needed`/`limit`
    /// are in bytes of bitset backing store.
    HaystackTooLarge {
        /// Memoization bytes the haystack would require.
        needed: usize,
        /// The memory cap (bytes).
        limit: usize,
    },
}

/// Maximum pattern length shown in error context (to avoid huge messages).
const MAX_PATTERN_CONTEXT_LEN: usize = 60;

/// Returns the largest byte index `<= index` that is a valid UTF-8 char boundary.
///
/// Equivalent to `str::floor_char_boundary` (stabilized in Rust 1.91), provided
/// here for MSRV compatibility (our MSRV is 1.85).
#[inline]
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    // Walk backwards from `index` until we find a byte that is NOT a UTF-8
    // continuation byte (0b10xxxxxx). At most 3 steps for a 4-byte char.
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// A Vim regex compilation/parse error.
///
/// Contains the error kind and optionally the pattern that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VimRegexError {
    /// The specific error kind.
    pub kind: VimRegexErrorKind,
    /// Truncated pattern context (set at compile entry points).
    #[cfg_attr(
        feature = "serde",
        serde(skip_serializing_if = "Option::is_none", default)
    )]
    context: Option<CompactString>,
}

impl VimRegexError {
    /// Create a new error from a kind (no context attached yet).
    #[inline]
    #[allow(
        dead_code,
        reason = "used by compile paths that construct errors without pattern context"
    )]
    pub(crate) fn new(kind: VimRegexErrorKind) -> Self {
        Self {
            kind,
            context: None,
        }
    }

    /// Attach pattern context. Truncates patterns longer than 60 bytes.
    #[must_use]
    pub fn with_pattern(mut self, pattern: &str) -> Self {
        let truncated = if pattern.len() > MAX_PATTERN_CONTEXT_LEN {
            let boundary = floor_char_boundary(pattern, MAX_PATTERN_CONTEXT_LEN);
            let mut s = pattern[..boundary].to_string();
            s.push_str("...");
            CompactString::from(s)
        } else {
            CompactString::from(pattern)
        };
        self.context = Some(truncated);
        self
    }

    /// Get the pattern context if attached.
    #[inline]
    pub fn pattern_context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    /// Get the span associated with this error (if the kind has one).
    pub fn span(&self) -> Option<&Span> {
        match &self.kind {
            VimRegexErrorKind::UnmatchedGroup { span }
            | VimRegexErrorKind::InvalidQuantifier { span, .. }
            | VimRegexErrorKind::InvalidEscape { span, .. }
            | VimRegexErrorKind::InvalidPercentEscape { span, .. }
            | VimRegexErrorKind::UnterminatedCollection { span }
            | VimRegexErrorKind::InvalidCollectionRange { span, .. }
            | VimRegexErrorKind::UnknownPosixClass { span, .. }
            | VimRegexErrorKind::InvalidCollectionCharCode { span, .. }
            | VimRegexErrorKind::InvalidCharCode { span, .. }
            | VimRegexErrorKind::CharCodeOutOfRange { span, .. }
            | VimRegexErrorKind::CharCodeSurrogate { span, .. }
            | VimRegexErrorKind::TrailingBackslash { span }
            | VimRegexErrorKind::InvalidOptionalSequenceAtom { span } => Some(span),
            VimRegexErrorKind::PatternTooComplex { span, .. } => span.as_ref(),
            VimRegexErrorKind::EmptyPattern
            | VimRegexErrorKind::ExpressionReplacementNotSupported
            | VimRegexErrorKind::InternalError { .. }
            | VimRegexErrorKind::HaystackTooLarge { .. } => None,
        }
    }
}

impl From<VimRegexErrorKind> for VimRegexError {
    #[inline]
    fn from(kind: VimRegexErrorKind) -> Self {
        Self {
            kind,
            context: None,
        }
    }
}

/// Format a byte position or range for Display output.
///
/// Shows "bytes X..Y" when the span covers more than 1 byte, "byte X" otherwise.
fn fmt_byte_range(f: &mut fmt::Formatter<'_>, span: &Span) -> fmt::Result {
    if span.end > span.start + 1 {
        write!(f, "bytes {}..{}", span.start, span.end)
    } else {
        write!(f, "byte {}", span.start)
    }
}

impl fmt::Display for VimRegexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            VimRegexErrorKind::UnmatchedGroup { span } => {
                write!(f, "E54: Unmatched \\( or \\) at ")?;
                fmt_byte_range(f, span)?;
            }
            VimRegexErrorKind::InvalidQuantifier { span, detail } => {
                write!(f, "E476: Invalid quantifier at ")?;
                fmt_byte_range(f, span)?;
                write!(f, ": {detail}")?;
            }
            VimRegexErrorKind::InvalidEscape { span, ch } => {
                write!(f, "E71: Invalid character after \\ '\\{ch}' at ")?;
                fmt_byte_range(f, span)?;
            }
            VimRegexErrorKind::InvalidPercentEscape { span, found } => {
                let detail = match found {
                    PercentEscapeContext::EndOfInput => "incomplete \\% at end of pattern",
                    PercentEscapeContext::MissingMarkChar => {
                        "\\%' requires a mark character (a-z, A-Z, <, >)"
                    }
                    PercentEscapeContext::MissingRelationTarget => {
                        "\\%< or \\%> requires a number, 'm, or .suffix"
                    }
                    PercentEscapeContext::InvalidPositionSuffix => {
                        "expected 'l' (line), 'c' (column), or 'v' (virtual column)"
                    }
                    PercentEscapeContext::MissingPositionSuffix => {
                        "position number requires 'l', 'c', or 'v' suffix"
                    }
                    PercentEscapeContext::UnrecognizedChar => "unrecognized character after \\%",
                };
                write!(f, "E71: Invalid \\% escape at ")?;
                fmt_byte_range(f, span)?;
                write!(f, ": {detail}")?;
            }
            VimRegexErrorKind::UnterminatedCollection { span } => {
                write!(f, "E69: Missing ] at ")?;
                fmt_byte_range(f, span)?;
            }
            VimRegexErrorKind::InvalidCollectionRange { span, detail } => {
                write!(f, "E69: Invalid collection range at ")?;
                fmt_byte_range(f, span)?;
                write!(f, ": {detail}")?;
            }
            VimRegexErrorKind::UnknownPosixClass { span, name } => {
                write!(f, "E69: Unknown POSIX class '[:{}:]' at ", name)?;
                fmt_byte_range(f, span)?;
            }
            VimRegexErrorKind::InvalidCollectionCharCode { span, detail } => {
                write!(f, "E678: Invalid character code in collection at ")?;
                fmt_byte_range(f, span)?;
                write!(f, ": {detail}")?;
            }
            VimRegexErrorKind::InvalidCharCode { span, detail } => {
                write!(f, "E678: Invalid character code at ")?;
                fmt_byte_range(f, span)?;
                write!(f, ": {detail}")?;
            }
            VimRegexErrorKind::CharCodeOutOfRange { span, value } => {
                write!(f, "E678: Character code U+{value:04X} out of range at ")?;
                fmt_byte_range(f, span)?;
                write!(f, ": value exceeds maximum Unicode code point (U+10FFFF)")?;
            }
            VimRegexErrorKind::CharCodeSurrogate { span, value } => {
                write!(f, "E679: Character code U+{value:04X} is a surrogate at ")?;
                fmt_byte_range(f, span)?;
                write!(
                    f,
                    ": values U+D800..U+DFFF are reserved for UTF-16 surrogate pairs"
                )?;
            }
            VimRegexErrorKind::TrailingBackslash { span } => {
                write!(f, "E476: Trailing backslash at ")?;
                fmt_byte_range(f, span)?;
            }
            VimRegexErrorKind::EmptyPattern => {
                write!(f, "E35: Empty pattern")?;
            }
            VimRegexErrorKind::PatternTooComplex { span, detail } => {
                if let Some(s) = span {
                    write!(f, "E339: Pattern too complex at ")?;
                    fmt_byte_range(f, s)?;
                    write!(f, ": {detail}")?;
                } else {
                    write!(f, "E339: Pattern too complex: {detail}")?;
                }
            }
            VimRegexErrorKind::ExpressionReplacementNotSupported => {
                write!(f, "E523: Expression replacement (\\=) not supported")?;
            }
            VimRegexErrorKind::InvalidOptionalSequenceAtom { span } => {
                write!(f, "E369: Invalid item in \\%[] at ")?;
                fmt_byte_range(f, span)?;
            }
            VimRegexErrorKind::InternalError { detail } => {
                write!(f, "E342: Internal regex error: {detail}")?;
            }
            VimRegexErrorKind::HaystackTooLarge { needed, limit } => write!(
                f,
                "haystack too large for backtracker memoization: needs {needed} bytes, limit {limit} bytes"
            )?,
        }

        // Append pattern context if present
        if let Some(ctx) = self.pattern_context() {
            write!(f, " in pattern '{ctx}'")?;
        }

        Ok(())
    }
}

impl std::error::Error for VimRegexError {}

// ═══════════════════════════════════════════════════════════════════════════════
// DISPLAY — serialize VimPatternNode back to a Vim regex pattern string
// ═══════════════════════════════════════════════════════════════════════════════

impl fmt::Display for VimPatternNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal(ch) => display_literal(*ch, f),
            Self::AnyChar => f.write_str("."),
            Self::AnyCharNl => f.write_str("\\_."),
            Self::StartOfLine => f.write_str("^"),
            Self::EndOfLine => f.write_str("$"),
            Self::AnywhereStartOfLine => f.write_str("\\_^"),
            Self::AnywhereEndOfLine => f.write_str("\\_$"),
            Self::StartOfFile => f.write_str("\\%^"),
            Self::EndOfFile => f.write_str("\\%$"),
            Self::WordBoundaryStart => f.write_str("\\<"),
            Self::WordBoundaryEnd => f.write_str("\\>"),
            Self::SetMatchStart => f.write_str("\\zs"),
            Self::SetMatchEnd => f.write_str("\\ze"),
            Self::CursorPosition => f.write_str("\\%#"),
            Self::VisualArea => f.write_str("\\%V"),
            Self::LastSubstitute => f.write_str("~"),

            Self::Class(class) => display_class(*class, f),
            Self::ClassWithNewline(class) => {
                f.write_str("\\_")?;
                display_class_char(*class, f)
            }
            Self::EscapeSequence(kind) => display_escape_kind(*kind, f),
            Self::BackReference(n) => write!(f, "\\{n}"),
            Self::CharByCode(ch) => {
                let cp = *ch as u32;
                if cp <= 0xFF {
                    write!(f, "\\%x{cp:02x}")
                } else if cp <= 0xFFFF {
                    write!(f, "\\%u{cp:04x}")
                } else {
                    write!(f, "\\%U{cp:08x}")
                }
            }

            Self::Collection {
                negated,
                items,
                include_newline,
            } => {
                if *include_newline {
                    f.write_str("\\_")?;
                }
                if *negated {
                    f.write_str("[^")?;
                } else {
                    f.write_str("[")?;
                }
                for item in items {
                    display_collection_item(item, f)?;
                }
                f.write_str("]")
            }

            Self::Sequence(children) => {
                for child in children {
                    write!(f, "{child}")?;
                }
                Ok(())
            }
            Self::Alternation(branches) => {
                for (i, branch) in branches.iter().enumerate() {
                    if i > 0 {
                        f.write_str("\\|")?;
                    }
                    write!(f, "{branch}")?;
                }
                Ok(())
            }
            Self::BranchAnd(branches) => {
                for (i, branch) in branches.iter().enumerate() {
                    if i > 0 {
                        f.write_str("\\&")?;
                    }
                    write!(f, "{branch}")?;
                }
                Ok(())
            }

            Self::Group { inner, capturing } => {
                if *capturing {
                    f.write_str("\\(")?;
                } else {
                    f.write_str("\\%(")?;
                }
                write!(f, "{inner}")?;
                f.write_str("\\)")
            }

            Self::Quantifier {
                node,
                min,
                max,
                greedy,
            } => {
                write!(f, "{node}")?;
                display_quantifier(*min, *max, *greedy, f)
            }

            Self::Lookaround { inner, kind, limit } => {
                write!(f, "{inner}")?;
                match kind {
                    LookaroundKind::PositiveAhead => f.write_str("\\@="),
                    LookaroundKind::NegativeAhead => f.write_str("\\@!"),
                    LookaroundKind::PositiveBehind => {
                        if let Some(lim) = limit {
                            write!(f, "\\@{lim}<=")
                        } else {
                            f.write_str("\\@<=")
                        }
                    }
                    LookaroundKind::NegativeBehind => {
                        if let Some(lim) = limit {
                            write!(f, "\\@{lim}<!")
                        } else {
                            f.write_str("\\@<!")
                        }
                    }
                    LookaroundKind::Atomic => f.write_str("\\@>"),
                }
            }

            Self::AtLine(spec) => display_line_spec(spec, f),
            Self::AtColumn(spec) => display_column_spec(spec, 'c', f),
            Self::AtVirtualColumn(spec) => display_column_spec(spec, 'v', f),

            Self::AtMark { mark, rel } => match rel {
                MarkRel::At => write!(f, "\\%'{mark}"),
                MarkRel::Before => write!(f, "\\%<'{mark}"),
                MarkRel::After => write!(f, "\\%>'{mark}"),
            },

            Self::OptionalSequence(items) => {
                f.write_str("\\%[")?;
                for item in items {
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }

            Self::AnyComposing => f.write_str("\\%C"),
            Self::ErrorPlaceholder => Ok(()),
        }
    }
}

/// Escape literal characters that are special in Magic mode.
fn display_literal(ch: char, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match ch {
        '\\' | '.' | '*' | '[' | ']' | '^' | '$' | '~' => {
            f.write_str("\\")?;
            write!(f, "{ch}")
        }
        _ => write!(f, "{ch}"),
    }
}

/// Display a `CharClass` as its Vim escape sequence (e.g., `\d`, `\W`).
fn display_class(class: CharClass, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("\\")?;
    display_class_char(class, f)
}

/// Display the class character without the leading backslash.
/// Used by both `\d` (standalone) and `\_d` (with-newline) forms.
fn display_class_char(class: CharClass, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let ch = match class {
        CharClass::Digit => 'd',
        CharClass::NotDigit => 'D',
        CharClass::Word => 'w',
        CharClass::NotWord => 'W',
        CharClass::Whitespace => 's',
        CharClass::NotWhitespace => 'S',
        CharClass::Alpha => 'a',
        CharClass::NotAlpha => 'A',
        CharClass::Lower => 'l',
        CharClass::NotLower => 'L',
        CharClass::Upper => 'u',
        CharClass::NotUpper => 'U',
        CharClass::Hex => 'x',
        CharClass::NotHex => 'X',
        CharClass::Head => 'h',
        CharClass::NotHead => 'H',
        CharClass::FileName => 'f',
        CharClass::Keyword => 'k',
        CharClass::KeywordNoDigit => 'K',
        CharClass::Ident => 'i',
        CharClass::SIdent => 'I',
        CharClass::Print => 'p',
        CharClass::SPrint => 'P',
        CharClass::Octal => 'o',
        CharClass::NOctal => 'O',
        CharClass::FileNameNoDigit => 'F',
        CharClass::Composing => {
            return f.write_str("%C");
        }
    };
    write!(f, "{ch}")
}

/// Display a quantifier suffix.
fn display_quantifier(
    min: u32,
    max: Option<u32>,
    greedy: bool,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match (min, max, greedy) {
        (0, None, true) => f.write_str("*"),
        (1, None, true) => f.write_str("\\+"),
        (0, Some(1), true) => f.write_str("\\?"),
        (0, None, false) => f.write_str("\\{-}"),
        (n, Some(m), true) if n == m => write!(f, "\\{{{n}}}"),
        (n, Some(m), true) => write!(f, "\\{{{n},{m}}}"),
        (n, None, true) => write!(f, "\\{{{n},}}"),
        (0, Some(m), false) => write!(f, "\\{{-,{m}}}"),
        (n, Some(m), false) if n == m => write!(f, "\\{{-{n}}}"),
        (n, Some(m), false) => write!(f, "\\{{-{n},{m}}}"),
        (n, None, false) => write!(f, "\\{{-{n},}}"),
    }
}

/// Display an escape kind.
fn display_escape_kind(kind: EscapeKind, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match kind {
        EscapeKind::Newline => f.write_str("\\n"),
        EscapeKind::Tab => f.write_str("\\t"),
        EscapeKind::Return => f.write_str("\\r"),
        EscapeKind::Escape => f.write_str("\\e"),
        EscapeKind::Backspace => f.write_str("\\b"),
    }
}

/// Write a single character inside a collection, escaping `] \ ^ -` so the
/// output round-trips through the parser.
fn write_collection_char(ch: char, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match ch {
        ']' | '\\' => {
            f.write_str("\\")?;
            write!(f, "{ch}")
        }
        // ^ only special at start, but always escaping is safe.
        '^' => f.write_str("\\^"),
        // - is special between items; always escaping is safe.
        '-' => f.write_str("\\-"),
        _ => write!(f, "{ch}"),
    }
}

/// Display a collection item.
fn display_collection_item(item: &CollectionItem, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match item {
        CollectionItem::Single(ch) => write_collection_char(*ch, f),
        CollectionItem::Range(lo, hi) => {
            write_collection_char(*lo, f)?;
            f.write_str("-")?;
            write_collection_char(*hi, f)
        }
        CollectionItem::Class(class) => display_class(*class, f),
        CollectionItem::PosixClass(name) => display_posix_class(*name, f),
        CollectionItem::Newline => f.write_str("\\n"),
    }
}

/// Display a POSIX class name.
fn display_posix_class(name: PosixClassName, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let s = match name {
        PosixClassName::Alnum => "[:alnum:]",
        PosixClassName::Alpha => "[:alpha:]",
        PosixClassName::Blank => "[:blank:]",
        PosixClassName::Cntrl => "[:cntrl:]",
        PosixClassName::Digit => "[:digit:]",
        PosixClassName::Graph => "[:graph:]",
        PosixClassName::Lower => "[:lower:]",
        PosixClassName::Print => "[:print:]",
        PosixClassName::Punct => "[:punct:]",
        PosixClassName::Space => "[:space:]",
        PosixClassName::Upper => "[:upper:]",
        PosixClassName::Xdigit => "[:xdigit:]",
        PosixClassName::Tab => "[:tab:]",
        PosixClassName::Return => "[:return:]",
        PosixClassName::Backspace => "[:backspace:]",
        PosixClassName::Escape => "[:escape:]",
        PosixClassName::Ident => "[:ident:]",
        PosixClassName::Keyword => "[:keyword:]",
        PosixClassName::Fname => "[:fname:]",
    };
    f.write_str(s)
}

/// Display a line spec.
fn display_line_spec(spec: &LineSpec, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match spec {
        LineSpec::Exact(n) => write!(f, "\\%{n}l"),
        LineSpec::Before(n) => write!(f, "\\%<{n}l"),
        LineSpec::After(n) => write!(f, "\\%>{n}l"),
        LineSpec::Current => f.write_str("\\%.l"),
        LineSpec::BeforeCurrent => f.write_str("\\%<.l"),
        LineSpec::AfterCurrent => f.write_str("\\%>.l"),
    }
}

/// Display a column/virtual-column spec.
fn display_column_spec(spec: &ColumnSpec, suffix: char, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match spec {
        ColumnSpec::Exact(n) => write!(f, "\\%{n}{suffix}"),
        ColumnSpec::Before(n) => write!(f, "\\%<{n}{suffix}"),
        ColumnSpec::After(n) => write!(f, "\\%>{n}{suffix}"),
        ColumnSpec::Current => write!(f, "\\%.{suffix}"),
        ColumnSpec::BeforeCurrent => write!(f, "\\%<.{suffix}"),
        ColumnSpec::AfterCurrent => write!(f, "\\%>.{suffix}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROPTEST ARBITRARY — structurally valid AST generation
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
pub(crate) mod arbitrary_ast {
    use super::*;
    use proptest::prelude::*;

    /// Maximum recursion depth for AST generation.
    const MAX_DEPTH: u32 = 4;

    /// Maximum children in Sequence/Alternation/BranchAnd.
    const MAX_CHILDREN: usize = 4;

    /// Maximum items in a Collection.
    const MAX_COLLECTION_ITEMS: usize = 5;

    /// Maximum items in an OptionalSequence.
    const MAX_OPTIONAL_ITEMS: usize = 3;

    /// Strategy that generates a structurally valid `VimPatternNode` tree.
    pub fn arb_vim_pattern_node() -> impl Strategy<Value = VimPatternNode> {
        arb_node(MAX_DEPTH)
    }

    /// Strategy for a character class.
    fn arb_char_class() -> impl Strategy<Value = CharClass> {
        prop_oneof![
            Just(CharClass::Digit),
            Just(CharClass::NotDigit),
            Just(CharClass::Word),
            Just(CharClass::NotWord),
            Just(CharClass::Whitespace),
            Just(CharClass::NotWhitespace),
            Just(CharClass::Alpha),
            Just(CharClass::NotAlpha),
            Just(CharClass::Lower),
            Just(CharClass::NotLower),
            Just(CharClass::Upper),
            Just(CharClass::NotUpper),
            Just(CharClass::Hex),
            Just(CharClass::NotHex),
            Just(CharClass::Head),
            Just(CharClass::NotHead),
            Just(CharClass::FileName),
            Just(CharClass::FileNameNoDigit),
            Just(CharClass::Keyword),
            Just(CharClass::KeywordNoDigit),
            Just(CharClass::Ident),
            Just(CharClass::SIdent),
            Just(CharClass::Print),
            Just(CharClass::SPrint),
            Just(CharClass::Octal),
            Just(CharClass::NOctal),
        ]
    }

    /// Strategy for a POSIX class name.
    fn arb_posix_class() -> impl Strategy<Value = PosixClassName> {
        prop_oneof![
            Just(PosixClassName::Alnum),
            Just(PosixClassName::Alpha),
            Just(PosixClassName::Blank),
            Just(PosixClassName::Cntrl),
            Just(PosixClassName::Digit),
            Just(PosixClassName::Graph),
            Just(PosixClassName::Lower),
            Just(PosixClassName::Print),
            Just(PosixClassName::Punct),
            Just(PosixClassName::Space),
            Just(PosixClassName::Upper),
            Just(PosixClassName::Xdigit),
            Just(PosixClassName::Tab),
            Just(PosixClassName::Return),
            Just(PosixClassName::Backspace),
            Just(PosixClassName::Escape),
            Just(PosixClassName::Ident),
            Just(PosixClassName::Keyword),
            Just(PosixClassName::Fname),
        ]
    }

    /// Strategy for a collection item.
    fn arb_collection_item() -> impl Strategy<Value = CollectionItem> {
        prop_oneof![
            // Single printable ASCII char (avoid NUL, control chars).
            (0x20u32..0x7Eu32)
                .prop_map(|c| { CollectionItem::Single(char::from_u32(c).unwrap_or('a')) }),
            // Range: ensure start <= end.
            (0x20u32..0x7Eu32, 0x20u32..0x7Eu32).prop_map(|(a, b)| {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                CollectionItem::Range(
                    char::from_u32(lo).unwrap_or('a'),
                    char::from_u32(hi).unwrap_or('z'),
                )
            }),
            arb_char_class().prop_map(CollectionItem::Class),
            arb_posix_class().prop_map(CollectionItem::PosixClass),
            Just(CollectionItem::Newline),
        ]
    }

    /// Strategy for an escape kind.
    fn arb_escape_kind() -> impl Strategy<Value = EscapeKind> {
        prop_oneof![
            Just(EscapeKind::Newline),
            Just(EscapeKind::Tab),
            Just(EscapeKind::Return),
            Just(EscapeKind::Escape),
            Just(EscapeKind::Backspace),
        ]
    }

    /// Terminal (leaf) node strategy — no recursion.
    fn arb_leaf() -> BoxedStrategy<VimPatternNode> {
        prop_oneof![
            // Literal: printable ASCII.
            (0x20u32..0x7Eu32)
                .prop_map(|c| { VimPatternNode::Literal(char::from_u32(c).unwrap_or('a')) }),
            Just(VimPatternNode::AnyChar),
            Just(VimPatternNode::AnyCharNl),
            Just(VimPatternNode::StartOfLine),
            Just(VimPatternNode::EndOfLine),
            Just(VimPatternNode::AnywhereStartOfLine),
            Just(VimPatternNode::AnywhereEndOfLine),
            Just(VimPatternNode::StartOfFile),
            Just(VimPatternNode::EndOfFile),
            Just(VimPatternNode::WordBoundaryStart),
            Just(VimPatternNode::WordBoundaryEnd),
            Just(VimPatternNode::SetMatchStart),
            Just(VimPatternNode::SetMatchEnd),
            Just(VimPatternNode::CursorPosition),
            Just(VimPatternNode::VisualArea),
            arb_char_class().prop_map(VimPatternNode::Class),
            arb_char_class().prop_map(VimPatternNode::ClassWithNewline),
            arb_escape_kind().prop_map(VimPatternNode::EscapeSequence),
            // BackReference: 1..=9.
            (1u8..=9u8).prop_map(VimPatternNode::BackReference),
            // CharByCode: printable ASCII.
            (0x20u32..0x7Eu32)
                .prop_map(|c| { VimPatternNode::CharByCode(char::from_u32(c).unwrap_or('a')) }),
            // Collection with generated items.
            (
                any::<bool>(),
                any::<bool>(),
                proptest::collection::vec(arb_collection_item(), 1..=MAX_COLLECTION_ITEMS),
            )
                .prop_map(|(negated, include_newline, items)| {
                    VimPatternNode::Collection {
                        negated,
                        items,
                        include_newline,
                    }
                }),
        ]
        .boxed()
    }

    /// Recursive node strategy with depth bounding.
    fn arb_node(depth: u32) -> BoxedStrategy<VimPatternNode> {
        if depth == 0 {
            return arb_leaf();
        }

        // Weight leaves higher to keep trees manageable.
        prop_oneof![
            40 => arb_leaf(),
            // Sequence: 2..=MAX_CHILDREN children.
            10 => proptest::collection::vec(arb_node(depth - 1), 2..=MAX_CHILDREN)
                .prop_map(VimPatternNode::Sequence),
            // Alternation: 2..=MAX_CHILDREN branches.
            8 => proptest::collection::vec(arb_node(depth - 1), 2..=MAX_CHILDREN)
                .prop_map(VimPatternNode::Alternation),
            // BranchAnd: 2..=3 branches.
            3 => proptest::collection::vec(arb_node(depth - 1), 2..=3)
                .prop_map(VimPatternNode::BranchAnd),
            // Group (capturing).
            8 => arb_node(depth - 1).prop_map(|inner| VimPatternNode::Group {
                inner: Box::new(inner),
                capturing: true,
            }),
            // Group (non-capturing).
            5 => arb_node(depth - 1).prop_map(|inner| VimPatternNode::Group {
                inner: Box::new(inner),
                capturing: false,
            }),
            // Quantifier: greedy.
            10 => (arb_quantifiable(depth - 1), arb_quantifier_bounds())
                .prop_map(|(node, (min, max, greedy))| VimPatternNode::Quantifier {
                    node: Box::new(node),
                    min,
                    max,
                    greedy,
                }),
            // Lookaround.
            5 => (arb_node(depth - 1), arb_lookaround_kind(), proptest::option::of(1u32..=100))
                .prop_map(|(inner, kind, limit)| VimPatternNode::Lookaround {
                    inner: Box::new(inner),
                    kind,
                    limit,
                }),
            // OptionalSequence.
            3 => proptest::collection::vec(arb_leaf(), 1..=MAX_OPTIONAL_ITEMS)
                .prop_map(VimPatternNode::OptionalSequence),
        ]
        .boxed()
    }

    /// Strategy for a node suitable for quantification (no anchors).
    fn arb_quantifiable(depth: u32) -> BoxedStrategy<VimPatternNode> {
        if depth == 0 {
            prop_oneof![
                (0x20u32..0x7Eu32)
                    .prop_map(|c| { VimPatternNode::Literal(char::from_u32(c).unwrap_or('a')) }),
                Just(VimPatternNode::AnyChar),
                arb_char_class().prop_map(VimPatternNode::Class),
                (
                    any::<bool>(),
                    any::<bool>(),
                    proptest::collection::vec(arb_collection_item(), 1..=MAX_COLLECTION_ITEMS),
                )
                    .prop_map(|(negated, include_newline, items)| {
                        VimPatternNode::Collection {
                            negated,
                            items,
                            include_newline,
                        }
                    }),
            ]
            .boxed()
        } else {
            prop_oneof![
                (0x20u32..0x7Eu32)
                    .prop_map(|c| { VimPatternNode::Literal(char::from_u32(c).unwrap_or('a')) }),
                Just(VimPatternNode::AnyChar),
                arb_char_class().prop_map(VimPatternNode::Class),
                arb_node(depth - 1).prop_map(|inner| VimPatternNode::Group {
                    inner: Box::new(inner),
                    capturing: true,
                }),
            ]
            .boxed()
        }
    }

    /// Strategy for quantifier bounds (min, max, greedy).
    fn arb_quantifier_bounds() -> impl Strategy<Value = (u32, Option<u32>, bool)> {
        prop_oneof![
            // * (0, None, greedy)
            Just((0u32, None, true)),
            // \+ (1, None, greedy)
            Just((1, None, true)),
            // \? (0, Some(1), greedy)
            Just((0, Some(1), true)),
            // \{-} (0, None, non-greedy)
            Just((0, None, false)),
            // \{-1,} (1, None, non-greedy)
            Just((1, None, false)),
            // \{n,m} bounded
            (0u32..=3, 1u32..=5).prop_map(|(min, extra)| (min, Some(min + extra), true)),
            // \{-n,m} bounded non-greedy
            (0u32..=3, 1u32..=5).prop_map(|(min, extra)| (min, Some(min + extra), false)),
            // \{n} exact
            (1u32..=5).prop_map(|n| (n, Some(n), true)),
        ]
    }

    /// Strategy for a lookaround kind.
    fn arb_lookaround_kind() -> impl Strategy<Value = LookaroundKind> {
        prop_oneof![
            Just(LookaroundKind::PositiveAhead),
            Just(LookaroundKind::NegativeAhead),
            Just(LookaroundKind::PositiveBehind),
            Just(LookaroundKind::NegativeBehind),
            Just(LookaroundKind::Atomic),
        ]
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ARBITRARY (cargo-fuzz integration)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod arbitrary_impl {
    use super::*;
    use arbitrary::{Arbitrary, Unstructured};

    /// Maximum recursion depth for fuzzer-driven AST generation.
    const MAX_DEPTH: usize = 5;
    /// Maximum children in compound nodes.
    const MAX_CHILDREN: usize = 4;
    /// Maximum collection items.
    const MAX_ITEMS: usize = 5;

    impl<'a> Arbitrary<'a> for VimPatternNode {
        fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
            arb_node(u, MAX_DEPTH)
        }
    }

    fn arb_node(u: &mut Unstructured<'_>, depth: usize) -> arbitrary::Result<VimPatternNode> {
        if depth == 0 {
            return arb_leaf(u);
        }
        // 14 variants, weighted toward leaves for smaller trees.
        let choice: u8 = u.int_in_range(0..=13)?;
        match choice {
            0..=5 => arb_leaf(u),
            6 => {
                let len = u.int_in_range(2..=MAX_CHILDREN)?;
                let children = (0..len)
                    .map(|_| arb_node(u, depth - 1))
                    .collect::<arbitrary::Result<Vec<_>>>()?;
                Ok(VimPatternNode::Sequence(children))
            }
            7 => {
                let len = u.int_in_range(2..=MAX_CHILDREN)?;
                let branches = (0..len)
                    .map(|_| arb_node(u, depth - 1))
                    .collect::<arbitrary::Result<Vec<_>>>()?;
                Ok(VimPatternNode::Alternation(branches))
            }
            8 => {
                let inner = arb_node(u, depth - 1)?;
                let capturing: bool = u.arbitrary()?;
                Ok(VimPatternNode::Group {
                    inner: Box::new(inner),
                    capturing,
                })
            }
            9 => {
                let inner = arb_node(u, depth - 1)?;
                let min: u32 = u.int_in_range(0..=3)?;
                let has_max: bool = u.arbitrary()?;
                let max = if has_max {
                    Some(u.int_in_range(min..=min + 5)?)
                } else {
                    None
                };
                let greedy: bool = u.arbitrary()?;
                Ok(VimPatternNode::Quantifier {
                    node: Box::new(inner),
                    min,
                    max,
                    greedy,
                })
            }
            10 => {
                let inner = arb_node(u, depth - 1)?;
                let kind = arb_lookaround_kind(u)?;
                let limit = if u.arbitrary::<bool>()? {
                    Some(u.int_in_range(1..=100)?)
                } else {
                    None
                };
                Ok(VimPatternNode::Lookaround {
                    inner: Box::new(inner),
                    kind,
                    limit,
                })
            }
            11 => {
                let len = u.int_in_range(2..=3)?;
                let branches = (0..len)
                    .map(|_| arb_node(u, depth - 1))
                    .collect::<arbitrary::Result<Vec<_>>>()?;
                Ok(VimPatternNode::BranchAnd(branches))
            }
            12 => {
                let len = u.int_in_range(1..=MAX_ITEMS)?;
                let items = (0..len)
                    .map(|_| arb_leaf(u))
                    .collect::<arbitrary::Result<Vec<_>>>()?;
                Ok(VimPatternNode::OptionalSequence(items))
            }
            _ => arb_leaf(u),
        }
    }

    fn arb_leaf(u: &mut Unstructured<'_>) -> arbitrary::Result<VimPatternNode> {
        let choice: u8 = u.int_in_range(0..=11)?;
        match choice {
            0 => {
                let c = u.int_in_range(0x20u32..=0x7E)?;
                Ok(VimPatternNode::Literal(char::from_u32(c).unwrap_or('a')))
            }
            1 => Ok(VimPatternNode::AnyChar),
            2 => Ok(VimPatternNode::AnyCharNl),
            3 => Ok(VimPatternNode::StartOfLine),
            4 => Ok(VimPatternNode::EndOfLine),
            5 => Ok(VimPatternNode::WordBoundaryStart),
            6 => Ok(VimPatternNode::WordBoundaryEnd),
            7 => Ok(VimPatternNode::Class(arb_char_class(u)?)),
            8 => {
                let negated: bool = u.arbitrary()?;
                let include_newline: bool = u.arbitrary()?;
                let len = u.int_in_range(1..=MAX_ITEMS)?;
                let items = (0..len)
                    .map(|_| arb_collection_item(u))
                    .collect::<arbitrary::Result<Vec<_>>>()?;
                Ok(VimPatternNode::Collection {
                    negated,
                    items,
                    include_newline,
                })
            }
            9 => {
                let n = u.int_in_range(1u8..=9)?;
                Ok(VimPatternNode::BackReference(n))
            }
            10 => Ok(VimPatternNode::EscapeSequence(arb_escape_kind(u)?)),
            _ => Ok(VimPatternNode::SetMatchStart),
        }
    }

    fn arb_char_class(u: &mut Unstructured<'_>) -> arbitrary::Result<CharClass> {
        let classes = [
            CharClass::Digit,
            CharClass::NotDigit,
            CharClass::Word,
            CharClass::NotWord,
            CharClass::Whitespace,
            CharClass::NotWhitespace,
            CharClass::Alpha,
            CharClass::NotAlpha,
            CharClass::Lower,
            CharClass::NotLower,
            CharClass::Upper,
            CharClass::NotUpper,
            CharClass::Hex,
            CharClass::NotHex,
            CharClass::Head,
            CharClass::NotHead,
            CharClass::FileName,
            CharClass::FileNameNoDigit,
            CharClass::Keyword,
            CharClass::KeywordNoDigit,
            CharClass::Ident,
            CharClass::SIdent,
            CharClass::Print,
            CharClass::SPrint,
            CharClass::Octal,
            CharClass::NOctal,
        ];
        Ok(classes[u.int_in_range(0..=classes.len() - 1)?])
    }

    fn arb_collection_item(u: &mut Unstructured<'_>) -> arbitrary::Result<CollectionItem> {
        let choice: u8 = u.int_in_range(0..=4)?;
        match choice {
            0 => {
                let c = u.int_in_range(0x20u32..=0x7E)?;
                Ok(CollectionItem::Single(char::from_u32(c).unwrap_or('a')))
            }
            1 => {
                let a = u.int_in_range(0x20u32..=0x7E)?;
                let b = u.int_in_range(0x20u32..=0x7E)?;
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                Ok(CollectionItem::Range(
                    char::from_u32(lo).unwrap_or('a'),
                    char::from_u32(hi).unwrap_or('z'),
                ))
            }
            2 => Ok(CollectionItem::Class(arb_char_class(u)?)),
            3 => Ok(CollectionItem::PosixClass(arb_posix_class(u)?)),
            _ => Ok(CollectionItem::Newline),
        }
    }

    fn arb_posix_class(u: &mut Unstructured<'_>) -> arbitrary::Result<PosixClassName> {
        let classes = [
            PosixClassName::Alnum,
            PosixClassName::Alpha,
            PosixClassName::Blank,
            PosixClassName::Cntrl,
            PosixClassName::Digit,
            PosixClassName::Graph,
            PosixClassName::Lower,
            PosixClassName::Print,
            PosixClassName::Punct,
            PosixClassName::Space,
            PosixClassName::Upper,
            PosixClassName::Xdigit,
            PosixClassName::Tab,
            PosixClassName::Return,
            PosixClassName::Backspace,
            PosixClassName::Escape,
            PosixClassName::Ident,
            PosixClassName::Keyword,
            PosixClassName::Fname,
        ];
        Ok(classes[u.int_in_range(0..=classes.len() - 1)?])
    }

    fn arb_escape_kind(u: &mut Unstructured<'_>) -> arbitrary::Result<EscapeKind> {
        let kinds = [
            EscapeKind::Newline,
            EscapeKind::Tab,
            EscapeKind::Return,
            EscapeKind::Escape,
            EscapeKind::Backspace,
        ];
        Ok(kinds[u.int_in_range(0..=kinds.len() - 1)?])
    }

    fn arb_lookaround_kind(u: &mut Unstructured<'_>) -> arbitrary::Result<LookaroundKind> {
        let kinds = [
            LookaroundKind::PositiveAhead,
            LookaroundKind::NegativeAhead,
            LookaroundKind::PositiveBehind,
            LookaroundKind::NegativeBehind,
            LookaroundKind::Atomic,
        ];
        Ok(kinds[u.int_in_range(0..=kinds.len() - 1)?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════
    // VimPatternNode construction and equality
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn literal_node_equality() {
        let a = VimPatternNode::Literal('a');
        let b = VimPatternNode::Literal('a');
        assert_eq!(a, b);
    }

    #[test]
    fn literal_node_inequality() {
        let a = VimPatternNode::Literal('a');
        let b = VimPatternNode::Literal('b');
        assert_ne!(a, b);
    }

    #[test]
    fn any_char_is_not_any_char_nl() {
        assert_ne!(VimPatternNode::AnyChar, VimPatternNode::AnyCharNl);
    }

    #[test]
    fn sequence_preserves_order() {
        let seq = VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ]);
        let seq_rev = VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('a'),
        ]);
        assert_ne!(seq, seq_rev);
    }

    #[test]
    fn empty_sequence() {
        let seq = VimPatternNode::Sequence(vec![]);
        assert_eq!(seq, VimPatternNode::Sequence(vec![]));
    }

    #[test]
    fn group_capturing_vs_non_capturing() {
        let cap = VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: true,
        };
        let non_cap = VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: false,
        };
        assert_ne!(cap, non_cap);
    }

    #[test]
    fn quantifier_greedy_vs_non_greedy() {
        let greedy = VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        };
        let non_greedy = VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: None,
            greedy: false,
        };
        assert_ne!(greedy, non_greedy);
    }

    #[test]
    fn quantifier_bounded_vs_unbounded() {
        let bounded = VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: Some(5),
            greedy: true,
        };
        let unbounded = VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        };
        assert_ne!(bounded, unbounded);
    }

    #[test]
    fn backreference_different_groups() {
        assert_ne!(
            VimPatternNode::BackReference(1),
            VimPatternNode::BackReference(2)
        );
    }

    #[test]
    fn collection_negated_vs_positive() {
        let positive = VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Single('a')],
            include_newline: false,
        };
        let negated = VimPatternNode::Collection {
            negated: true,
            items: vec![CollectionItem::Single('a')],
            include_newline: false,
        };
        assert_ne!(positive, negated);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // CharClass variants
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn char_class_digit_is_not_word() {
        assert_ne!(CharClass::Digit, CharClass::Word);
    }

    #[test]
    fn char_class_copy_trait() {
        let a = CharClass::Digit;
        let b = a; // Copy
        assert_eq!(a, b);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // CollectionItem variants
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn collection_item_single() {
        let item = CollectionItem::Single('x');
        assert_eq!(item, CollectionItem::Single('x'));
    }

    #[test]
    fn collection_item_range() {
        let item = CollectionItem::Range('a', 'z');
        assert_eq!(item, CollectionItem::Range('a', 'z'));
    }

    #[test]
    fn collection_item_class() {
        let item = CollectionItem::Class(CharClass::Digit);
        assert_eq!(item, CollectionItem::Class(CharClass::Digit));
    }

    #[test]
    fn collection_item_posix() {
        let item = CollectionItem::PosixClass(PosixClassName::Alpha);
        assert_eq!(item, CollectionItem::PosixClass(PosixClassName::Alpha));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // LookaroundKind variants
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn lookaround_kinds_are_distinct() {
        assert_ne!(LookaroundKind::PositiveAhead, LookaroundKind::NegativeAhead);
        assert_ne!(
            LookaroundKind::PositiveBehind,
            LookaroundKind::NegativeBehind
        );
        assert_ne!(LookaroundKind::PositiveAhead, LookaroundKind::Atomic);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // LineSpec and ColumnSpec
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn line_spec_exact() {
        assert_eq!(LineSpec::Exact(10), LineSpec::Exact(10));
        assert_ne!(LineSpec::Exact(10), LineSpec::Exact(20));
    }

    #[test]
    fn line_spec_before_after() {
        assert_ne!(LineSpec::Before(5), LineSpec::After(5));
    }

    #[test]
    fn column_spec_current() {
        assert_eq!(ColumnSpec::Current, ColumnSpec::Current);
    }

    #[test]
    fn column_spec_exact_vs_before() {
        assert_ne!(ColumnSpec::Exact(3), ColumnSpec::Before(3));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // MarkRel
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn mark_rel_at_vs_before() {
        assert_ne!(MarkRel::At, MarkRel::Before);
    }

    #[test]
    fn mark_rel_at_vs_after() {
        assert_ne!(MarkRel::At, MarkRel::After);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // EscapeKind
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn escape_kind_distinct() {
        assert_ne!(EscapeKind::Newline, EscapeKind::Tab);
        assert_ne!(EscapeKind::Return, EscapeKind::Escape);
        assert_ne!(EscapeKind::Backspace, EscapeKind::Newline);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // CaseMode
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn case_mode_default_is_default() {
        assert_eq!(CaseMode::default(), CaseMode::Default);
    }

    #[test]
    fn case_mode_sensitive_vs_insensitive() {
        assert_ne!(CaseMode::Sensitive, CaseMode::Insensitive);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PatternFeatures
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn pattern_features_default_all_false() {
        let features = PatternFeatures::default();
        assert!(!features.has_backreferences);
        assert!(!features.has_lookaround);
        assert!(!features.has_atomic);
        assert!(!features.has_buffer_position);
        assert!(!features.has_match_override);
        assert!(!features.has_last_substitute);
        assert!(!features.has_branch_and);
        assert_eq!(features.capture_count, 0);
    }

    #[test]
    fn pattern_features_with_backreferences() {
        let features = PatternFeatures {
            has_backreferences: true,
            ..PatternFeatures::default()
        };
        assert!(features.has_backreferences);
        assert!(!features.has_lookaround);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ParseResult construction
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parse_result_constructible() {
        let result = ParseResult {
            node: VimPatternNode::AnyChar,
            case_mode: CaseMode::Sensitive,
            composing_mode: ComposingMode::Respect,
            features: PatternFeatures::default(),
            additional_errors: Vec::new(),
        };
        assert_eq!(result.case_mode, CaseMode::Sensitive);
        assert_eq!(result.node, VimPatternNode::AnyChar);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // VimRegexError Display
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn error_unmatched_group_display() {
        let err = VimRegexError::from(VimRegexErrorKind::UnmatchedGroup {
            span: Span::new(5, 7),
        });
        let msg = format!("{err}");
        assert!(msg.contains("E54"));
        assert!(msg.contains("5"));
    }

    #[test]
    fn error_invalid_quantifier_display() {
        let err = VimRegexError::from(VimRegexErrorKind::InvalidQuantifier {
            span: Span::new(3, 5),
            detail: "bad range".into(),
        });
        let msg = format!("{err}");
        assert!(msg.contains("3"));
        assert!(msg.contains("bad range"));
    }

    #[test]
    fn error_invalid_escape_display() {
        let err = VimRegexError::from(VimRegexErrorKind::InvalidEscape {
            span: Span::new(2, 4),
            ch: 'z',
        });
        let msg = format!("{err}");
        assert!(msg.contains("E71"));
        assert!(msg.contains("z"));
    }

    #[test]
    fn error_unterminated_collection_display() {
        let err = VimRegexError::from(VimRegexErrorKind::UnterminatedCollection {
            span: Span::new(7, 10),
        });
        let msg = format!("{err}");
        assert!(msg.contains("E69"));
    }

    #[test]
    fn error_invalid_char_code_display() {
        let err = VimRegexError::from(VimRegexErrorKind::InvalidCharCode {
            span: Span::new(0, 3),
            detail: "no digits after base specifier".into(),
        });
        let msg = format!("{err}");
        assert!(msg.contains("E678"));
    }

    #[test]
    fn error_trailing_backslash_display() {
        let err = VimRegexError::from(VimRegexErrorKind::TrailingBackslash { span: Span::at(10) });
        let msg = format!("{err}");
        assert!(msg.contains("Trailing"));
        assert!(msg.contains("10"));
    }

    #[test]
    fn error_empty_pattern_display() {
        let err = VimRegexError::from(VimRegexErrorKind::EmptyPattern);
        let msg = format!("{err}");
        assert_eq!(msg, "E35: Empty pattern");
    }

    #[test]
    fn error_pattern_too_complex_display() {
        let err = VimRegexError::from(VimRegexErrorKind::PatternTooComplex {
            span: None,
            detail: "state limit exceeded".into(),
        });
        let msg = format!("{err}");
        assert!(msg.contains("state limit exceeded"));
    }

    #[test]
    fn error_expression_replacement_display() {
        let err = VimRegexError::from(VimRegexErrorKind::ExpressionReplacementNotSupported);
        let msg = format!("{err}");
        assert!(msg.contains("\\="));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // VimRegexError implements std::error::Error
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn vim_regex_error_is_error_trait() {
        let err: Box<dyn std::error::Error> =
            Box::new(VimRegexError::from(VimRegexErrorKind::EmptyPattern));
        // Should be able to upcast to dyn Error
        assert!(!err.to_string().is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // VimPatternNode clone
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn pattern_node_clone_is_equal() {
        let node = VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Literal('x')),
            min: 1,
            max: Some(3),
            greedy: true,
        };
        let cloned = node.clone();
        assert_eq!(node, cloned);
    }

    #[test]
    fn pattern_node_alternation_clone() {
        let node = VimPatternNode::Alternation(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ]);
        assert_eq!(node, node.clone());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Serde round-trips (only compiled with --features serde)
    // ═══════════════════════════════════════════════════════════════════════

    #[cfg(feature = "serde")]
    mod serde_tests {
        use super::*;

        fn round_trip_json<
            T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
        >(
            value: &T,
        ) {
            let json = serde_json::to_string(value).expect("serialize");
            let back: T = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*value, back);
        }

        #[test]
        fn serde_vim_pattern_node_literal() {
            round_trip_json(&VimPatternNode::Literal('x'));
        }

        #[test]
        fn serde_vim_pattern_node_quantifier() {
            let node = VimPatternNode::Quantifier {
                node: Box::new(VimPatternNode::AnyChar),
                min: 1,
                max: Some(5),
                greedy: true,
            };
            round_trip_json(&node);
        }

        #[test]
        fn serde_vim_regex_error_empty() {
            round_trip_json(&VimRegexError::from(VimRegexErrorKind::EmptyPattern));
        }

        #[test]
        fn serde_vim_regex_error_invalid_quantifier() {
            round_trip_json(&VimRegexError::from(VimRegexErrorKind::InvalidQuantifier {
                span: Span::new(3, 5),
                detail: "bad".into(),
            }));
        }

        #[test]
        fn serde_pattern_features_full() {
            let features = PatternFeatures {
                has_backreferences: true,
                has_lookaround: true,
                has_atomic: true,
                has_buffer_position: true,
                has_match_override: true,
                has_last_substitute: true,
                has_multiline: true,
                has_branch_and: true,
                capture_count: 5,
            };
            round_trip_json(&features);
        }

        #[test]
        fn serde_case_mode() {
            round_trip_json(&CaseMode::Sensitive);
            round_trip_json(&CaseMode::Insensitive);
            round_trip_json(&CaseMode::Default);
        }

        #[test]
        fn serde_char_class() {
            round_trip_json(&CharClass::Digit);
            round_trip_json(&CharClass::Word);
            round_trip_json(&CharClass::Hex);
        }

        #[test]
        fn serde_collection_item() {
            round_trip_json(&CollectionItem::Single('a'));
            round_trip_json(&CollectionItem::Range('a', 'z'));
            round_trip_json(&CollectionItem::Class(CharClass::Digit));
            round_trip_json(&CollectionItem::PosixClass(PosixClassName::Alpha));
        }

        #[test]
        fn serde_lookaround_kind() {
            round_trip_json(&LookaroundKind::PositiveAhead);
            round_trip_json(&LookaroundKind::NegativeBehind);
            round_trip_json(&LookaroundKind::Atomic);
        }

        #[test]
        fn serde_line_spec() {
            round_trip_json(&LineSpec::Exact(10));
            round_trip_json(&LineSpec::Current);
        }

        #[test]
        fn serde_column_spec() {
            round_trip_json(&ColumnSpec::Before(5));
            round_trip_json(&ColumnSpec::AfterCurrent);
        }

        #[test]
        fn serde_mark_rel() {
            round_trip_json(&MarkRel::At);
            round_trip_json(&MarkRel::Before);
            round_trip_json(&MarkRel::After);
        }

        #[test]
        fn serde_escape_kind() {
            round_trip_json(&EscapeKind::Newline);
            round_trip_json(&EscapeKind::Tab);
        }

        #[test]
        fn serde_composing_mode() {
            round_trip_json(&ComposingMode::Respect);
            round_trip_json(&ComposingMode::Ignore);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PosixClassName variants
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn posix_class_alnum_copy() {
        let a = PosixClassName::Alnum;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn posix_class_distinct_variants() {
        assert_ne!(PosixClassName::Alpha, PosixClassName::Digit);
        assert_ne!(PosixClassName::Lower, PosixClassName::Upper);
        assert_ne!(PosixClassName::Space, PosixClassName::Blank);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // AtMark construction
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn at_mark_construction() {
        let node = VimPatternNode::AtMark {
            mark: 'a',
            rel: MarkRel::At,
        };
        let node2 = VimPatternNode::AtMark {
            mark: 'a',
            rel: MarkRel::Before,
        };
        assert_ne!(node, node2);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Display roundtrip (hand-crafted cases)
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn display_literal() {
        assert_eq!(VimPatternNode::Literal('a').to_string(), "a");
    }

    #[test]
    fn display_literal_escaped() {
        assert_eq!(VimPatternNode::Literal('.').to_string(), "\\.");
        assert_eq!(VimPatternNode::Literal('*').to_string(), "\\*");
        assert_eq!(VimPatternNode::Literal('\\').to_string(), "\\\\");
        assert_eq!(VimPatternNode::Literal('^').to_string(), "\\^");
        assert_eq!(VimPatternNode::Literal('$').to_string(), "\\$");
        assert_eq!(VimPatternNode::Literal('~').to_string(), "\\~");
        assert_eq!(VimPatternNode::Literal('[').to_string(), "\\[");
    }

    #[test]
    fn display_any_char() {
        assert_eq!(VimPatternNode::AnyChar.to_string(), ".");
    }

    #[test]
    fn display_char_class() {
        assert_eq!(VimPatternNode::Class(CharClass::Digit).to_string(), "\\d");
        assert_eq!(VimPatternNode::Class(CharClass::NotWord).to_string(), "\\W");
    }

    #[test]
    fn display_class_with_newline() {
        assert_eq!(
            VimPatternNode::ClassWithNewline(CharClass::Digit).to_string(),
            "\\_d"
        );
    }

    #[test]
    fn display_quantifier_star() {
        let q = VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::AnyChar),
            min: 0,
            max: None,
            greedy: true,
        };
        assert_eq!(q.to_string(), ".*");
    }

    #[test]
    fn display_quantifier_plus() {
        let q = VimPatternNode::Quantifier {
            node: Box::new(VimPatternNode::Class(CharClass::Word)),
            min: 1,
            max: None,
            greedy: true,
        };
        assert_eq!(q.to_string(), "\\w\\+");
    }

    #[test]
    fn display_group_capturing() {
        let g = VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: true,
        };
        assert_eq!(g.to_string(), "\\(a\\)");
    }

    #[test]
    fn display_group_non_capturing() {
        let g = VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: false,
        };
        assert_eq!(g.to_string(), "\\%(a\\)");
    }

    #[test]
    fn display_alternation() {
        let a = VimPatternNode::Alternation(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ]);
        assert_eq!(a.to_string(), "a\\|b");
    }

    #[test]
    fn display_collection() {
        let c = VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Range('a', 'z')],
            include_newline: false,
        };
        assert_eq!(c.to_string(), "[a-z]");
    }

    #[test]
    fn display_collection_negated() {
        let c = VimPatternNode::Collection {
            negated: true,
            items: vec![CollectionItem::Single('x')],
            include_newline: false,
        };
        assert_eq!(c.to_string(), "[^x]");
    }

    #[test]
    fn display_collection_with_newline() {
        let c = VimPatternNode::Collection {
            negated: false,
            items: vec![CollectionItem::Single('a')],
            include_newline: true,
        };
        assert_eq!(c.to_string(), "\\_[a]");
    }

    #[test]
    fn display_backreference() {
        assert_eq!(VimPatternNode::BackReference(3).to_string(), "\\3");
    }

    #[test]
    fn display_lookaround() {
        let la = VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('x')),
            kind: LookaroundKind::PositiveAhead,
            limit: None,
        };
        assert_eq!(la.to_string(), "x\\@=");
    }

    #[test]
    fn display_lookbehind_with_limit() {
        let lb = VimPatternNode::Lookaround {
            inner: Box::new(VimPatternNode::Literal('x')),
            kind: LookaroundKind::PositiveBehind,
            limit: Some(5),
        };
        assert_eq!(lb.to_string(), "x\\@5<=");
    }
}
