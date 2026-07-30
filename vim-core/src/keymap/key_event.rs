//! `KeyEvent` - Key press with modifiers.
//!
//! Represents a complete key event (key + modifiers).

use std::borrow::Cow;

use super::{Key, Modifiers};

/// Key press with modifiers.
///
/// The optional `latin_key` carries the Latin command equivalent when the
/// host bridge detects a non-Latin keyboard layout. It is transport metadata
/// and is **excluded** from `PartialEq` and `Hash` — two `KeyEvent`s that
/// differ only in `latin_key` are considered identical for mapping lookup,
/// core keymap classification, and all other comparisons.
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    /// The key that was pressed.
    pub(crate) key: Key,
    /// Active modifiers.
    pub(crate) modifiers: Modifiers,
    /// Latin command equivalent for non-Latin keyboard layouts.
    ///
    /// Populated by the host bridge when `key` is a non-ASCII character
    /// but the physical key maps to an ASCII letter. `None` for Latin
    /// layouts, non-letter keys, and Ctrl paths.
    latin_key: Option<Key>,
}

impl PartialEq for KeyEvent {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.modifiers == other.modifiers
    }
}

impl Eq for KeyEvent {}

impl std::hash::Hash for KeyEvent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.modifiers.hash(state);
    }
}

impl KeyEvent {
    /// Create a new `KeyEvent`.
    #[must_use]
    pub const fn new(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            modifiers,
            latin_key: None,
        }
    }

    /// Create a `KeyEvent` from a character (no modifiers).
    #[must_use]
    pub const fn char(c: char) -> Self {
        Self {
            key: Key::Char(c),
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a `KeyEvent` with Ctrl modifier.
    #[must_use]
    pub const fn ctrl(c: char) -> Self {
        Self {
            key: Key::Char(c),
            modifiers: Modifiers::CTRL,
            latin_key: None,
        }
    }

    /// Create a `KeyEvent` with Alt modifier.
    #[must_use]
    pub const fn alt(c: char) -> Self {
        Self {
            key: Key::Char(c),
            modifiers: Modifiers::ALT,
            latin_key: None,
        }
    }

    /// Create a `KeyEvent` with Shift modifier.
    ///
    /// **Note:** For printable characters, Shift is typically folded into the
    /// character itself (e.g., 'A' instead of Shift+'a'). This constructor is
    /// primarily useful for non-printable keys like `shift(Key::Tab)` or
    /// when you need to explicitly represent a Shift-modified key event
    /// for special keys (arrows, function keys, etc.).
    #[must_use]
    pub const fn shift(c: char) -> Self {
        Self {
            key: Key::Char(c),
            modifiers: Modifiers::SHIFT,
            latin_key: None,
        }
    }

    /// Create an Escape key event.
    #[must_use]
    pub const fn escape() -> Self {
        Self {
            key: Key::Escape,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create an Enter key event.
    #[must_use]
    pub const fn enter() -> Self {
        Self {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a Tab key event.
    #[must_use]
    pub const fn tab() -> Self {
        Self {
            key: Key::Tab,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a Backspace key event.
    #[must_use]
    pub const fn backspace() -> Self {
        Self {
            key: Key::Backspace,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a Leader placeholder key event.
    ///
    /// This sentinel is resolved to the actual leader key at mapping
    /// definition time in [`Keymap::map()`](crate::keymap::Keymap::map).
    #[must_use]
    pub const fn leader() -> Self {
        Self {
            key: Key::Leader,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a LocalLeader placeholder key event.
    ///
    /// Like [`leader()`](Self::leader), but resolved to `maplocalleader`
    /// instead of `mapleader`. Used for filetype-specific plugin mappings.
    #[must_use]
    pub const fn local_leader() -> Self {
        Self {
            key: Key::LocalLeader,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a `<Plug>` key event with the given registry id.
    ///
    /// Use `Keymap::register_plug()` to get the id for a name.
    #[must_use]
    pub const fn plug(id: u32) -> Self {
        Self {
            key: Key::Plug(id),
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create an `<Action>` key event with the given registry id.
    ///
    /// Use `Keymap::register_action()` to get the id for a name.
    #[must_use]
    pub const fn action(id: u32) -> Self {
        Self {
            key: Key::Action(id),
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a Delete key event.
    #[must_use]
    pub const fn delete() -> Self {
        Self {
            key: Key::Delete,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create an Up arrow key event.
    #[must_use]
    pub const fn up() -> Self {
        Self {
            key: Key::Up,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a Down arrow key event.
    #[must_use]
    pub const fn down() -> Self {
        Self {
            key: Key::Down,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a Left arrow key event.
    #[must_use]
    pub const fn left() -> Self {
        Self {
            key: Key::Left,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a Right arrow key event.
    #[must_use]
    pub const fn right() -> Self {
        Self {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Create a function key event (F1-F12).
    #[must_use]
    pub const fn f(n: u8) -> Self {
        Self {
            key: Key::F(n),
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }

    /// Attach a Latin command equivalent for non-Latin keyboard layouts.
    ///
    /// The engine uses this in command-dispatch contexts (Normal/Visual/OP
    /// Ready state) to normalize non-Latin keys to their Latin equivalents.
    /// In literal-char contexts (Insert, f/t/r arguments), the original
    /// localized `key` is used instead.
    #[must_use]
    pub const fn with_latin(mut self, latin: Key) -> Self {
        self.latin_key = Some(latin);
        self
    }

    /// Get the Latin command equivalent, if any.
    ///
    /// Returns `Some` when the host bridge detected a non-Latin keyboard
    /// layout and the physical key maps to an ASCII letter. Returns `None`
    /// for Latin layouts, non-letter keys, and Ctrl paths.
    #[must_use]
    pub const fn latin_key(&self) -> Option<Key> {
        self.latin_key
    }

    /// Parse a key name string to a `KeyEvent`.
    ///
    /// Case-insensitive matching of Vim's angle-bracket key names:
    /// `"Escape"`, `"Esc"`, `"Enter"`, `"Return"`, `"CR"`, `"Tab"`,
    /// `"Backspace"`, `"BS"`, `"Delete"`, `"Del"`, `"Space"`,
    /// `"Up"`, `"Down"`, `"Left"`, `"Right"`, `"Home"`, `"End"`,
    /// `"PageUp"`, `"PageDown"`, `"Insert"`, `"F1"`-`"F12"`.
    ///
    /// Returns `None` for unrecognized names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        // Delegate to Key's inner parser which already handles all these names
        // case-insensitively.
        let key = Key::from_vim_notation(&format!("<{name}>"))?;
        Some(Self {
            key,
            modifiers: Modifiers::NONE,
            latin_key: None,
        })
    }

    /// Parse from Vim notation.
    ///
    /// # Examples
    /// - `j` → `KeyEvent::char`('j')
    /// - `<C-w>` → `KeyEvent::ctrl`('w')
    /// - `<CR>` → `KeyEvent::enter()`
    /// - `<M-x>` → `KeyEvent::alt`('x')
    #[must_use]
    pub fn from_vim_notation(s: &str) -> Option<Self> {
        // Handle angle bracket notation
        if s.starts_with('<') && s.ends_with('>') {
            let inner = &s[1..s.len() - 1];

            // Parse modifiers first
            let (modifiers, remaining) = Modifiers::from_vim_prefix(inner);

            // Parse the key part
            if remaining.len() == 1 {
                // Single character
                let c = remaining.chars().next()?;
                return Some(Self {
                    key: Key::Char(c),
                    modifiers,
                    latin_key: None,
                });
            } else {
                // Named key
                let key = Key::from_vim_notation(&format!("<{remaining}>"))?;
                return Some(Self {
                    key,
                    modifiers,
                    latin_key: None,
                });
            }
        }

        // Single character (no angle brackets) — handle multi-byte Unicode
        let mut chars = s.chars();
        if let Some(c) = chars.next() {
            if chars.next().is_none() {
                return Some(Self::char(c));
            }
        }

        // Try as named key
        let key = Key::from_vim_notation(s)?;
        Some(Self {
            key,
            modifiers: Modifiers::NONE,
            latin_key: None,
        })
    }

    /// Convert to Vim notation.
    #[must_use]
    pub fn to_vim_notation(&self) -> Cow<'static, str> {
        let mod_prefix = self.modifiers.to_vim_prefix();

        if mod_prefix.is_empty() {
            // No modifiers — pass through Key's Cow directly (zero-alloc
            // for the 20 static-string arms).
            self.key.to_vim_notation()
        } else {
            // Need angle brackets for modifiers — always allocates.
            if let Key::Char(c) = &self.key {
                Cow::Owned(format!("<{mod_prefix}{c}>"))
            } else {
                // Strip existing angle brackets from key
                let key_str = self.key.to_vim_notation();
                if key_str.starts_with('<') && key_str.ends_with('>') {
                    let inner = &key_str[1..key_str.len() - 1];
                    Cow::Owned(format!("<{mod_prefix}{inner}>"))
                } else {
                    Cow::Owned(format!("<{mod_prefix}{key_str}>"))
                }
            }
        }
    }

    /// Get the key.
    #[must_use]
    pub const fn key(&self) -> Key {
        self.key
    }

    /// Get the modifiers.
    #[must_use]
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Check if this has any modifiers.
    #[must_use]
    pub const fn has_modifiers(&self) -> bool {
        !self.modifiers.is_empty()
    }

    /// Check if this is a simple character (no modifiers).
    #[must_use]
    pub const fn is_char(&self) -> bool {
        self.key.is_char() && !self.has_modifiers()
    }

    /// Check if this is Ctrl-C.
    #[must_use]
    pub const fn is_ctrl_c(&self) -> bool {
        self.modifiers.contains(Modifiers::CTRL) && matches!(self.key, Key::Char('c' | 'C'))
    }

    /// Get the character if this is a simple char event.
    #[must_use]
    pub const fn as_char(&self) -> Option<char> {
        if self.has_modifiers() {
            None
        } else {
            self.key.as_char()
        }
    }
}

impl std::fmt::Display for KeyEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_vim_notation())
    }
}

impl From<char> for KeyEvent {
    fn from(c: char) -> Self {
        Self::char(c)
    }
}

impl From<Key> for KeyEvent {
    fn from(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::NONE,
            latin_key: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors() {
        let k = KeyEvent::char('j');
        assert_eq!(k.key, Key::Char('j'));
        assert_eq!(k.modifiers, Modifiers::NONE);

        let k = KeyEvent::ctrl('w');
        assert_eq!(k.key, Key::Char('w'));
        assert_eq!(k.modifiers, Modifiers::CTRL);

        let k = KeyEvent::alt('x');
        assert_eq!(k.key, Key::Char('x'));
        assert_eq!(k.modifiers, Modifiers::ALT);

        let k = KeyEvent::escape();
        assert_eq!(k.key, Key::Escape);

        let k = KeyEvent::enter();
        assert_eq!(k.key, Key::Enter);

        let k = KeyEvent::tab();
        assert_eq!(k.key, Key::Tab);

        let k = KeyEvent::backspace();
        assert_eq!(k.key, Key::Backspace);
    }

    #[test]
    fn accessor_methods() {
        let k = KeyEvent::ctrl('a');
        assert_eq!(k.key(), Key::Char('a'));
        assert_eq!(k.modifiers(), Modifiers::CTRL);
    }

    #[test]
    fn has_modifiers_checks() {
        assert!(!KeyEvent::char('j').has_modifiers());
        assert!(KeyEvent::ctrl('w').has_modifiers());
        assert!(KeyEvent::alt('x').has_modifiers());
        assert!(!KeyEvent::escape().has_modifiers());
    }

    #[test]
    fn is_char_and_as_char() {
        assert!(KeyEvent::char('j').is_char());
        assert_eq!(KeyEvent::char('j').as_char(), Some('j'));

        // Ctrl-w is NOT a simple char
        assert!(!KeyEvent::ctrl('w').is_char());
        assert_eq!(KeyEvent::ctrl('w').as_char(), None);

        // Escape is not a char
        assert!(!KeyEvent::escape().is_char());
        assert_eq!(KeyEvent::escape().as_char(), None);
    }

    #[test]
    fn vim_notation_roundtrip_simple_chars() {
        for c in ['j', 'd', 'w', '0', 'G', 'i'] {
            let k = KeyEvent::from_vim_notation(&c.to_string()).unwrap();
            assert_eq!(k, KeyEvent::char(c));
            // Round-trip
            let notation = k.to_vim_notation();
            let roundtripped = KeyEvent::from_vim_notation(&notation).unwrap();
            assert_eq!(roundtripped, k, "round-trip failed for '{c}'");
        }
    }

    #[test]
    fn vim_notation_roundtrip_modifiers() {
        let k = KeyEvent::from_vim_notation("<C-w>").unwrap();
        assert_eq!(k, KeyEvent::ctrl('w'));
        assert_eq!(k.to_vim_notation(), "<C-w>");

        let k = KeyEvent::from_vim_notation("<M-x>").unwrap();
        assert_eq!(k, KeyEvent::alt('x'));
        assert_eq!(k.to_vim_notation(), "<M-x>");
    }

    #[test]
    fn meta_d_notation_round_trips() {
        let key = KeyEvent::new(Key::Char('d'), Modifiers::META);
        let notation = key.to_vim_notation();
        assert!(
            notation.contains("D-"),
            "Meta modifier should serialize as D-: got {}",
            notation
        );
        // Full round-trip: parse <D-d> back and compare.
        let parsed =
            KeyEvent::from_vim_notation(&notation).expect("should parse back from D- notation");
        assert_eq!(
            parsed, key,
            "round-trip of <D-d> should produce the original KeyEvent"
        );
    }

    #[test]
    fn vim_notation_named_keys() {
        assert_eq!(KeyEvent::from_vim_notation("<CR>"), Some(KeyEvent::enter()));
        assert_eq!(
            KeyEvent::from_vim_notation("<Esc>"),
            Some(KeyEvent::escape())
        );
        assert_eq!(KeyEvent::from_vim_notation("<Tab>"), Some(KeyEvent::tab()));
        assert_eq!(
            KeyEvent::from_vim_notation("<BS>"),
            Some(KeyEvent::backspace())
        );
    }

    #[test]
    fn from_char_impl() {
        let k: KeyEvent = 'z'.into();
        assert_eq!(k, KeyEvent::char('z'));
    }

    #[test]
    fn from_key_impl() {
        let k: KeyEvent = Key::Enter.into();
        assert_eq!(k, KeyEvent::enter());
    }

    #[test]
    fn delete_constructor() {
        let k = KeyEvent::delete();
        assert_eq!(k.key, Key::Delete);
        assert_eq!(k.modifiers, Modifiers::NONE);
    }

    #[test]
    fn arrow_constructors() {
        assert_eq!(KeyEvent::up().key, Key::Up);
        assert_eq!(KeyEvent::down().key, Key::Down);
        assert_eq!(KeyEvent::left().key, Key::Left);
        assert_eq!(KeyEvent::right().key, Key::Right);

        assert_eq!(KeyEvent::up().modifiers, Modifiers::NONE);
        assert_eq!(KeyEvent::down().modifiers, Modifiers::NONE);
        assert_eq!(KeyEvent::left().modifiers, Modifiers::NONE);
        assert_eq!(KeyEvent::right().modifiers, Modifiers::NONE);
    }

    #[test]
    fn function_key_constructors() {
        for n in 1..=12 {
            let k = KeyEvent::f(n);
            assert_eq!(k.key, Key::F(n));
            assert_eq!(k.modifiers, Modifiers::NONE);
        }
    }

    #[test]
    fn shift_constructor() {
        let k = KeyEvent::shift('a');
        assert_eq!(k.key, Key::Char('a'));
        assert_eq!(k.modifiers, Modifiers::SHIFT);
        assert!(k.has_modifiers());
        assert!(!k.is_char()); // shift modifier present
    }

    #[test]
    fn from_name_known_keys() {
        assert_eq!(KeyEvent::from_name("Escape"), Some(KeyEvent::escape()));
        assert_eq!(KeyEvent::from_name("Esc"), Some(KeyEvent::escape()));
        assert_eq!(KeyEvent::from_name("Enter"), Some(KeyEvent::enter()));
        assert_eq!(KeyEvent::from_name("Return"), Some(KeyEvent::enter()));
        assert_eq!(KeyEvent::from_name("CR"), Some(KeyEvent::enter()));
        assert_eq!(KeyEvent::from_name("Tab"), Some(KeyEvent::tab()));
        assert_eq!(
            KeyEvent::from_name("Backspace"),
            Some(KeyEvent::backspace())
        );
        assert_eq!(KeyEvent::from_name("BS"), Some(KeyEvent::backspace()));
        assert_eq!(KeyEvent::from_name("Delete"), Some(KeyEvent::delete()));
        assert_eq!(KeyEvent::from_name("Del"), Some(KeyEvent::delete()));
        assert_eq!(KeyEvent::from_name("Space"), Some(KeyEvent::char(' ')));
        assert_eq!(KeyEvent::from_name("Up"), Some(KeyEvent::up()));
        assert_eq!(KeyEvent::from_name("Down"), Some(KeyEvent::down()));
        assert_eq!(KeyEvent::from_name("Left"), Some(KeyEvent::left()));
        assert_eq!(KeyEvent::from_name("Right"), Some(KeyEvent::right()));
        assert_eq!(
            KeyEvent::from_name("Home"),
            Some(KeyEvent::new(Key::Home, Modifiers::NONE))
        );
        assert_eq!(
            KeyEvent::from_name("End"),
            Some(KeyEvent::new(Key::End, Modifiers::NONE))
        );
        assert_eq!(
            KeyEvent::from_name("PageUp"),
            Some(KeyEvent::new(Key::PageUp, Modifiers::NONE))
        );
        assert_eq!(
            KeyEvent::from_name("PageDown"),
            Some(KeyEvent::new(Key::PageDown, Modifiers::NONE))
        );
        assert_eq!(
            KeyEvent::from_name("Insert"),
            Some(KeyEvent::new(Key::Insert, Modifiers::NONE))
        );
    }

    #[test]
    fn from_name_function_keys() {
        for n in 1..=12u8 {
            assert_eq!(
                KeyEvent::from_name(&format!("F{n}")),
                Some(KeyEvent::f(n)),
                "from_name failed for F{n}"
            );
        }
    }

    #[test]
    fn from_name_case_insensitive() {
        assert_eq!(KeyEvent::from_name("escape"), Some(KeyEvent::escape()));
        assert_eq!(KeyEvent::from_name("Escape"), Some(KeyEvent::escape()));
        assert_eq!(KeyEvent::from_name("ESCAPE"), Some(KeyEvent::escape()));
        assert_eq!(KeyEvent::from_name("cr"), Some(KeyEvent::enter()));
        assert_eq!(KeyEvent::from_name("Cr"), Some(KeyEvent::enter()));
        assert_eq!(KeyEvent::from_name("CR"), Some(KeyEvent::enter()));
        assert_eq!(KeyEvent::from_name("tab"), Some(KeyEvent::tab()));
        assert_eq!(KeyEvent::from_name("TAB"), Some(KeyEvent::tab()));
    }

    #[test]
    fn from_name_unknown() {
        assert_eq!(KeyEvent::from_name("Unknown"), None);
        assert_eq!(KeyEvent::from_name(""), None);
        assert_eq!(KeyEvent::from_name("NotAKey"), None);
    }

    #[test]
    fn display_matches_to_vim_notation() {
        let events = [
            KeyEvent::char('k'),
            KeyEvent::char(' '),
            KeyEvent::char('<'),
            KeyEvent::ctrl('w'),
            KeyEvent::alt('x'),
            KeyEvent::shift('a'),
            KeyEvent::escape(),
            KeyEvent::enter(),
            KeyEvent::tab(),
            KeyEvent::backspace(),
            KeyEvent::delete(),
            KeyEvent::up(),
            KeyEvent::down(),
            KeyEvent::left(),
            KeyEvent::right(),
            KeyEvent::f(1),
            KeyEvent::f(12),
            KeyEvent::leader(),
            KeyEvent::new(Key::Tab, Modifiers::SHIFT),
            KeyEvent::new(Key::Char('f'), Modifiers::CTRL | Modifiers::SHIFT),
            KeyEvent::new(Key::Up, Modifiers::ALT),
        ];
        for event in events {
            assert_eq!(
                format!("{event}"),
                event.to_vim_notation().as_ref(),
                "Display != to_vim_notation for {event:?}"
            );
        }
    }

    #[test]
    fn display_plain_chars() {
        assert_eq!(format!("{}", KeyEvent::char('k')), "k");
        assert_eq!(format!("{}", KeyEvent::char('Z')), "Z");
        assert_eq!(format!("{}", KeyEvent::char('0')), "0");
    }

    #[test]
    fn display_modified_keys() {
        assert_eq!(format!("{}", KeyEvent::ctrl('w')), "<C-w>");
        assert_eq!(format!("{}", KeyEvent::alt('x')), "<M-x>");
        assert_eq!(format!("{}", KeyEvent::shift('a')), "<S-a>");
        assert_eq!(
            format!(
                "{}",
                KeyEvent::new(Key::Char('f'), Modifiers::CTRL | Modifiers::SHIFT)
            ),
            "<C-S-f>"
        );
    }

    #[test]
    fn display_special_keys() {
        assert_eq!(format!("{}", KeyEvent::escape()), "<Esc>");
        assert_eq!(format!("{}", KeyEvent::enter()), "<CR>");
        assert_eq!(format!("{}", KeyEvent::tab()), "<Tab>");
        assert_eq!(format!("{}", KeyEvent::backspace()), "<BS>");
    }

    #[test]
    fn display_modified_special_keys() {
        assert_eq!(
            format!("{}", KeyEvent::new(Key::Tab, Modifiers::SHIFT)),
            "<S-Tab>"
        );
        assert_eq!(
            format!("{}", KeyEvent::new(Key::Up, Modifiers::ALT)),
            "<M-Up>"
        );
        assert_eq!(
            format!("{}", KeyEvent::new(Key::F(5), Modifiers::CTRL)),
            "<C-F5>"
        );
    }

    #[test]
    fn with_latin_sets_field() {
        let k = KeyEvent::char('\u{043E}') // Cyrillic 'о'
            .with_latin(Key::Char('j'));
        assert_eq!(k.latin_key(), Some(Key::Char('j')));
    }

    #[test]
    fn default_latin_key_is_none() {
        assert_eq!(KeyEvent::char('j').latin_key(), None);
        assert_eq!(KeyEvent::ctrl('a').latin_key(), None);
        assert_eq!(KeyEvent::escape().latin_key(), None);
        assert_eq!(KeyEvent::enter().latin_key(), None);
        assert_eq!(KeyEvent::up().latin_key(), None);
    }

    #[test]
    fn latin_key_excluded_from_eq() {
        let plain = KeyEvent::char('\u{043E}'); // Cyrillic 'о'
        let with_latin = plain.with_latin(Key::Char('j'));
        assert_eq!(plain, with_latin, "latin_key must not affect equality");
    }

    #[test]
    fn latin_key_excluded_from_hash() {
        use std::hash::{Hash, Hasher};
        let mut h1 = std::collections::hash_map::DefaultHasher::new();
        let mut h2 = std::collections::hash_map::DefaultHasher::new();

        let plain = KeyEvent::char('\u{043E}');
        let with_latin = plain.with_latin(Key::Char('j'));

        plain.hash(&mut h1);
        with_latin.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish(), "latin_key must not affect hash");
    }

    #[test]
    fn from_impls_have_no_latin_key() {
        let from_char: KeyEvent = 'z'.into();
        assert_eq!(from_char.latin_key(), None);

        let from_key: KeyEvent = Key::Enter.into();
        assert_eq!(from_key.latin_key(), None);
    }
}
