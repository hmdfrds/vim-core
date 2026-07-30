//! Key parsing utilities.

use vim_core::keymap::KeyEvent;

/// Parse a Vim notation string into key events.
///
/// Supports `<Esc>`, `<CR>`, `<C-a>`, `<S-Tab>`, `<F1>`, etc.
pub fn parse_keys(notation: &str) -> Vec<KeyEvent> {
    vim_core::execution::parse_keys_from_string(notation)
}
