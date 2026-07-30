//! Helper functions for grammar parsing.
//!
//! Utility functions used across handler modules.

use crate::grammar::types::Operator;
use crate::keymap::KeyEvent;
use crate::primitives::{InsertEntryType, Mode, VisualType};

/// Get operator from key.
#[must_use]
pub const fn operator_from_key(key: KeyEvent) -> Option<Operator> {
    match key.as_char() {
        Some('d' | 'x' | 'X' | 'D') => Some(Operator::Delete),
        Some('c' | 's' | 'C') => Some(Operator::Change),
        Some('y' | 'Y') => Some(Operator::Yank),
        Some('>') => Some(Operator::Indent),
        Some('<') => Some(Operator::Outdent),
        Some('!') => Some(Operator::Filter),
        Some('=') => Some(Operator::Reindent),
        _ => None,
    }
}

/// Get mode from key.
#[must_use]
pub const fn mode_from_key(key: KeyEvent) -> Option<Mode> {
    match key.as_char() {
        Some('i' | 'a' | 'o' | 'O' | 'I' | 'A') => Some(Mode::Insert),
        Some('v') => Some(Mode::Visual(VisualType::Char)),
        Some('V') => Some(Mode::Visual(VisualType::Line)),
        Some(':') => Some(Mode::CommandLine),
        Some('R') => Some(Mode::Replace),
        _ => None,
    }
}

/// Get insert entry type from key.
///
/// Maps i/I/a/A/o/O/s/S to `InsertEntryType`.
#[must_use]
pub const fn entry_type_from_key(key: KeyEvent) -> Option<InsertEntryType> {
    match key.as_char() {
        Some('i') => Some(InsertEntryType::BeforeCursor),
        Some('I') => Some(InsertEntryType::FirstNonBlank),
        Some('a') => Some(InsertEntryType::AfterCursor),
        Some('A') => Some(InsertEntryType::EndOfLine),
        Some('o') => Some(InsertEntryType::NewLineBelow),
        Some('O') => Some(InsertEntryType::NewLineAbove),
        Some('s') => Some(InsertEntryType::SubstituteChar),
        Some('S') => Some(InsertEntryType::SubstituteLine),
        _ => None,
    }
}

/// Get visual mode type from key.
///
/// Maps v/V/Ctrl-V to visual mode types, or None if not a visual key.
#[must_use]
pub fn visual_type_from_key(key: KeyEvent) -> Option<VisualType> {
    // Check for Ctrl-V first (use key.key.as_char() because key.as_char() returns None with modifiers)
    if key.modifiers.contains(crate::keymap::Modifiers::CTRL) && key.key.as_char() == Some('v') {
        return Some(VisualType::Block);
    }
    match key.as_char() {
        Some('v') => Some(VisualType::Char),
        Some('V') => Some(VisualType::Line),
        _ => None,
    }
}
