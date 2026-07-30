//! Vim regex pattern parser.
//!
//! Converts a raw pattern string into a `VimPatternNode` AST tree.
//! Supports all four magic modes and Vim-specific regex constructs.
//!
//! Grammar hierarchy (top → bottom):
//!   `parse_alternation` → `parse_sequence` → `parse_piece` → `parse_atom`
//!
//! # Layering
//!
//! This is an internal submodule of `regex`. It inherits the import rules
//! from its parent `regex/mod.rs`: it imports `primitives` (`MagicMode`) and
//! `regex::ir` (AST types); it must not import `commands`, `grammar`,
//! `effects`, `state`, `execution`, `mode`, `keymap`, `dispatch`, `errors`
//! or `document`.

mod atoms;
mod collections;
mod core;
mod percent;
mod quantifiers;

// Re-export the public API.
#[cfg(test)]
pub(crate) use core::parse_pattern;
pub(crate) use core::parse_with_magic;

// Re-export the Parser struct and IR types for sibling submodules
// (atoms, collections, percent, quantifiers) that extend Parser via impl blocks.
pub(super) use self::core::Parser;
pub(super) use super::ir::{CaseMode, CharClass, VimPatternNode, VimRegexError, VimRegexErrorKind};

/// Map a character to its `CharClass` variant (if any).
///
/// Shared by both the collection parser (`\d` inside `[...]`) and the
/// core atom parser (`\d` as a standalone escape). Centralised here to
/// eliminate duplication and ensure both paths stay in sync.
#[must_use]
const fn char_to_class(ch: char) -> Option<CharClass> {
    match ch {
        'd' => Some(CharClass::Digit),
        'D' => Some(CharClass::NotDigit),
        'w' => Some(CharClass::Word),
        'W' => Some(CharClass::NotWord),
        's' => Some(CharClass::Whitespace),
        'S' => Some(CharClass::NotWhitespace),
        'a' => Some(CharClass::Alpha),
        'A' => Some(CharClass::NotAlpha),
        'l' => Some(CharClass::Lower),
        'L' => Some(CharClass::NotLower),
        'u' => Some(CharClass::Upper),
        'U' => Some(CharClass::NotUpper),
        'x' => Some(CharClass::Hex),
        'X' => Some(CharClass::NotHex),
        'h' => Some(CharClass::Head),
        'H' => Some(CharClass::NotHead),
        'f' => Some(CharClass::FileName),
        'F' => Some(CharClass::FileNameNoDigit),
        'k' => Some(CharClass::Keyword),
        'K' => Some(CharClass::KeywordNoDigit),
        'i' => Some(CharClass::Ident),
        'I' => Some(CharClass::SIdent),
        'p' => Some(CharClass::Print),
        'P' => Some(CharClass::SPrint),
        'o' => Some(CharClass::Octal),
        'O' => Some(CharClass::NOctal),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../tests/parser/mod.rs"]
mod tests;
