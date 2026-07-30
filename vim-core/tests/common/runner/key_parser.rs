//! Key notation parser.
//!
//! Parses Vim key notation strings like "dd", "2dw", "<Esc>", "<C-a>", "<S-Tab>".
//!
//! # Supported Notation
//!
//! - Plain characters: `a`, `1`, `@`
//! - Special keys: `<Esc>`, `<CR>`, `<Tab>`, `<BS>`, `<Space>`
//! - Arrow keys: `<Up>`, `<Down>`, `<Left>`, `<Right>`
//! - Navigation: `<Home>`, `<End>`, `<PageUp>`, `<PageDown>`
//! - Editing: `<Del>`, `<Insert>`
//! - Function keys: `<F1>` through `<F12>`
//! - Modifiers: `<C-x>` (Ctrl), `<S-x>` (Shift), `<M-x>` or `<A-x>` (Alt/Meta)
//! - Combined: `<C-S-a>`, `<M-C-x>`

use vim_core::keymap::{Key, KeyEvent, Modifiers};

/// Parse key notation string into KeyEvents.
///
/// # Examples
///
/// - `"dd"` → [Char('d'), Char('d')]
/// - `"<Esc>jj"` → [Escape, Char('j'), Char('j')]
/// - `"<C-a>"` → [Ctrl+Char('a')]
/// - `"<S-Tab>"` → [Shift+Tab]
/// - `"<C-S-a>"` → [Ctrl+Shift+Char('a')]
pub fn parse_keys(keys: &str) -> Vec<KeyEvent> {
    let mut result = Vec::new();
    let mut chars = keys.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            // Parse special key notation like <Esc>, <CR>, <C-a>.
            // Max length of valid special key content is ~10 chars (e.g., "C-S-F12").
            // Limit scan to avoid greedily consuming literal '<' in typed text
            // (e.g., "x < 0 { return; }<Esc>" should parse `<` as literal).
            const MAX_SPECIAL_LEN: usize = 12;
            let mut special = String::new();
            let mut found_close = false;

            // Save iterator state for potential backtrack
            let mut lookahead = chars.clone();
            while let Some(&nc) = lookahead.peek() {
                if nc == '>' {
                    lookahead.next(); // consume '>'
                    found_close = true;
                    break;
                }
                if special.len() >= MAX_SPECIAL_LEN {
                    break; // too long to be a valid special key
                }
                special.push(lookahead.next().unwrap());
            }

            if found_close {
                if let Some(key) = parse_special_key(&special) {
                    // Valid special key — advance the real iterator
                    chars = lookahead;
                    result.push(key);
                } else {
                    // Not a recognized special key — treat '<' as literal
                    result.push(KeyEvent::from('<'));
                    // Don't advance chars — they'll be re-parsed naturally
                }
            } else {
                // No closing '>' within limit — treat '<' as literal
                result.push(KeyEvent::from('<'));
            }
        } else {
            // Handle raw control characters (from macro recording)
            match c {
                '\x1b' => result.push(KeyEvent::escape()),     // ESC
                '\r' | '\n' => result.push(KeyEvent::enter()), // CR/LF/Enter
                '\t' => result.push(KeyEvent::tab()),          // Tab
                '\x08' => result.push(KeyEvent::backspace()),  // Backspace
                c if (c as u32) >= 1 && (c as u32) <= 26 => {
                    // Ctrl-A (0x01) through Ctrl-Z (0x1A)
                    let letter = (c as u8 + b'a' - 1) as char;
                    result.push(KeyEvent::ctrl(letter));
                }
                // Handle Neovim's internal 3-byte encoding for special keys
                '\u{80}' => {
                    // Read next two bytes for key identification
                    let b1 = chars.next();
                    let b2 = chars.next();
                    match (b1, b2) {
                        (Some('k'), Some('u')) => {
                            result.push(KeyEvent::new(Key::Up, Modifiers::NONE))
                        }
                        (Some('k'), Some('d')) => {
                            result.push(KeyEvent::new(Key::Down, Modifiers::NONE))
                        }
                        (Some('k'), Some('l')) => {
                            result.push(KeyEvent::new(Key::Left, Modifiers::NONE))
                        }
                        (Some('k'), Some('r')) => {
                            result.push(KeyEvent::new(Key::Right, Modifiers::NONE))
                        }
                        (Some('k'), Some('h')) => {
                            result.push(KeyEvent::new(Key::Home, Modifiers::NONE))
                        }
                        (Some('@'), Some('7')) => {
                            result.push(KeyEvent::new(Key::End, Modifiers::NONE))
                        }
                        (Some('k'), Some('P')) => {
                            result.push(KeyEvent::new(Key::PageUp, Modifiers::NONE))
                        }
                        (Some('k'), Some('N')) => {
                            result.push(KeyEvent::new(Key::PageDown, Modifiers::NONE))
                        }
                        (Some('k'), Some('D')) => {
                            result.push(KeyEvent::new(Key::Delete, Modifiers::NONE))
                        }
                        (Some('k'), Some('I')) => {
                            result.push(KeyEvent::new(Key::Insert, Modifiers::NONE))
                        }
                        _ => {
                            // Unknown sequence, push as chars
                            result.push(KeyEvent::from('\u{80}'));
                            if let Some(c1) = b1 {
                                result.push(KeyEvent::from(c1));
                            }
                            if let Some(c2) = b2 {
                                result.push(KeyEvent::from(c2));
                            }
                        }
                    }
                }
                _ => result.push(KeyEvent::from(c)),
            }
        }
    }

    result
}

/// Parse modifiers from a special key string.
///
/// Returns the accumulated modifiers and the remaining key part.
fn parse_modifiers(s: &str) -> (Modifiers, &str) {
    let mut mods = Modifiers::NONE;
    let mut remaining = s;

    loop {
        if let Some(rest) = remaining.strip_prefix("c-") {
            mods |= Modifiers::CTRL;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("s-") {
            mods |= Modifiers::SHIFT;
            remaining = rest;
        } else if let Some(rest) = remaining
            .strip_prefix("m-")
            .or_else(|| remaining.strip_prefix("a-"))
        {
            mods |= Modifiers::ALT;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("d-") {
            mods |= Modifiers::META;
            remaining = rest;
        } else {
            break;
        }
    }

    (mods, remaining)
}

/// Parse special key notation.
///
/// Handles:
/// - `<Esc>`, `<Escape>` → Escape key
/// - `<CR>`, `<Enter>` → Enter key
/// - `<BS>`, `<Backspace>` → Backspace key
/// - `<Tab>` → Tab key
/// - `<Space>` → Space character
/// - `<Up>`, `<Down>`, `<Left>`, `<Right>` → Arrow keys
/// - `<Home>`, `<End>`, `<PageUp>`, `<PageDown>` → Navigation keys
/// - `<Del>`, `<Delete>`, `<Insert>` → Editing keys
/// - `<F1>` through `<F12>` → Function keys
/// - `<C-x>` → Ctrl+x
/// - `<S-x>` → Shift+x
/// - `<M-x>`, `<A-x>` → Alt/Meta+x
/// - `<C-S-x>` → Ctrl+Shift+x (combined modifiers)
fn parse_special_key(special: &str) -> Option<KeyEvent> {
    let lower = special.to_lowercase();

    // Parse modifiers first
    let (modifiers, key_part) = parse_modifiers(&lower);

    // Match the remaining key part
    let (key, extra_mods) = match key_part {
        // Escape variants
        "esc" | "escape" => (Key::Escape, Modifiers::NONE),

        // Enter variants
        "cr" | "enter" | "return" => (Key::Enter, Modifiers::NONE),

        // Backspace variants
        "bs" | "backspace" => (Key::Backspace, Modifiers::NONE),

        // Tab
        "tab" => (Key::Tab, Modifiers::NONE),

        // Space
        "space" => (Key::Char(' '), Modifiers::NONE),

        // Arrow keys
        "up" => (Key::Up, Modifiers::NONE),
        "down" => (Key::Down, Modifiers::NONE),
        "left" => (Key::Left, Modifiers::NONE),
        "right" => (Key::Right, Modifiers::NONE),

        // Navigation keys
        "home" => (Key::Home, Modifiers::NONE),
        "end" => (Key::End, Modifiers::NONE),
        "pageup" | "prior" => (Key::PageUp, Modifiers::NONE),
        "pagedown" | "next" => (Key::PageDown, Modifiers::NONE),

        // Editing keys
        "del" | "delete" => (Key::Delete, Modifiers::NONE),
        "insert" | "ins" => (Key::Insert, Modifiers::NONE),

        // Nul character
        "nul" => (Key::Char('\0'), Modifiers::NONE),

        // Literal angle brackets
        "lt" => (Key::Char('<'), Modifiers::NONE),
        "gt" => (Key::Char('>'), Modifiers::NONE),

        // Alternative escape sequences
        "[" if modifiers.contains(Modifiers::CTRL) => (Key::Escape, Modifiers::NONE),

        // Function keys (f1..f12) - require at least 2 chars (f + digit)
        // to avoid shadowing single-char 'f' when parsing <C-f>
        s if s.starts_with('f') && s.len() >= 2 && s.len() <= 3 => {
            if let Ok(num) = s[1..].parse::<u8>() {
                if (1..=12).contains(&num) {
                    (Key::F(num), Modifiers::NONE)
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }

        // Single character — only valid with modifiers (e.g. <C-a>, <S-x>).
        // Without modifiers, <x> is not a recognized Vim special key and
        // should be treated as a literal '<' followed by the rest.
        s if s.len() == 1 && modifiers != Modifiers::NONE => {
            let c = s.chars().next().unwrap();
            (Key::Char(c), Modifiers::NONE)
        }

        // Unknown
        _ => return None,
    };

    Some(KeyEvent::new(key, modifiers | extra_mods))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_chars() {
        let keys = parse_keys("dd");
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn parse_escape() {
        let keys = parse_keys("<Esc>");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key(), Key::Escape);
    }

    #[test]
    fn parse_ctrl_key() {
        let keys = parse_keys("<C-a>");
        assert_eq!(keys.len(), 1);
        assert!(keys[0].modifiers().contains(Modifiers::CTRL));
    }

    #[test]
    fn parse_shift_key() {
        let keys = parse_keys("<S-Tab>");
        assert_eq!(keys.len(), 1);
        assert!(keys[0].modifiers().contains(Modifiers::SHIFT));
        assert_eq!(keys[0].key(), Key::Tab);
    }

    #[test]
    fn parse_alt_key() {
        let keys = parse_keys("<M-x>");
        assert_eq!(keys.len(), 1);
        assert!(keys[0].modifiers().contains(Modifiers::ALT));
    }

    #[test]
    fn parse_combined_modifiers() {
        let keys = parse_keys("<C-S-a>");
        assert_eq!(keys.len(), 1);
        assert!(keys[0].modifiers().contains(Modifiers::CTRL));
        assert!(keys[0].modifiers().contains(Modifiers::SHIFT));
    }

    #[test]
    fn parse_function_keys() {
        let keys = parse_keys("<F1>");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key(), Key::F(1));

        let keys = parse_keys("<F12>");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key(), Key::F(12));
    }

    #[test]
    fn parse_navigation_keys() {
        let keys = parse_keys("<Home><End><PageUp><PageDown>");
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0].key(), Key::Home);
        assert_eq!(keys[1].key(), Key::End);
        assert_eq!(keys[2].key(), Key::PageUp);
        assert_eq!(keys[3].key(), Key::PageDown);
    }

    #[test]
    fn parse_delete_insert() {
        let keys = parse_keys("<Del><Insert>");
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].key(), Key::Delete);
        assert_eq!(keys[1].key(), Key::Insert);
    }

    #[test]
    fn parse_mixed() {
        let keys = parse_keys("i<Esc>j");
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn parse_ctrl_bracket_as_escape() {
        let keys = parse_keys("<C-[>");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key(), Key::Escape);
    }

    #[test]
    fn parse_literal_angle_brackets() {
        let keys = parse_keys("<lt>");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key(), Key::Char('<'));

        let keys = parse_keys("<gt>");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key(), Key::Char('>'));
    }

    #[test]
    fn parse_ci_angle_sequence() {
        // This is the sequence for: ci< + "new" + Escape
        let keys = parse_keys("ci<lt>new<Esc>");
        assert_eq!(keys.len(), 7); // c, i, <, n, e, w, Esc
        assert_eq!(keys[0].key(), Key::Char('c'));
        assert_eq!(keys[1].key(), Key::Char('i'));
        assert_eq!(keys[2].key(), Key::Char('<'));
        assert_eq!(keys[3].key(), Key::Char('n'));
        assert_eq!(keys[4].key(), Key::Char('e'));
        assert_eq!(keys[5].key(), Key::Char('w'));
        assert_eq!(keys[6].key(), Key::Escape);
    }
}
