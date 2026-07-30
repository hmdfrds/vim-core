//! Grammar test helpers.
//!
//! Provides `parse_grammar()` which feeds a key string through the Vim
//! grammar parser and returns the final `GrammarResult`. Used by the
//! `grammar_test!` macro.

use vim_core::grammar::{GrammarResult, Parser};
use vim_core::keymap::{KeyEvent, Keymap};
use vim_core::primitives::Mode;

/// Parse a key string through the Vim grammar and return the result.
///
/// Creates a fresh `Parser` and `Keymap`, feeds each character as a
/// `KeyEvent`, and returns the final `GrammarResult`. Escape characters
/// (`\x1b`) are converted to `KeyEvent::escape()`.
pub fn parse_grammar(keys: &str) -> GrammarResult {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    let mut result = GrammarResult::Invalid;
    for c in keys.chars() {
        let key = if c == '\x1b' {
            KeyEvent::escape()
        } else {
            KeyEvent::char(c)
        };
        result = parser.process(key, &keymap, Mode::Normal);
    }
    result
}
