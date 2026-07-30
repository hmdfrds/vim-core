//! Key representation.
//!
//! Represents a single key press (without modifiers).

use std::borrow::Cow;

/// Representation of a single key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    // === Characters ===
    /// Any unicode character.
    Char(char),

    // === Named Keys ===
    /// Enter/Return key.
    Enter,
    /// Escape key.
    Escape,
    /// Tab key.
    Tab,
    /// Backspace key.
    Backspace,
    /// Delete key.
    Delete,

    // === Arrow Keys ===
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,

    // === Navigation ===
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Insert key.
    Insert,

    // === Function Keys ===
    /// Function key F1-F12 (1-12).
    F(u8),

    /// Leader key placeholder — resolved at mapping definition time.
    /// Not a real key; used as a sentinel in LHS sequences before
    /// leader substitution in `Keymap::map()`.
    Leader,

    /// Local leader key placeholder — resolved at mapping definition time.
    /// Like `Leader`, but uses the buffer-local leader key (`maplocalleader`).
    /// Typically set to `,` for filetype-specific plugin mappings.
    LocalLeader,

    /// `<Plug>` pseudo-key for plugin mapping namespaces.
    ///
    /// Never generated from user input — only appears in mapping LHS/RHS.
    /// The `u32` is a registry id that maps to a human-readable name
    /// (e.g., `<Plug>(surround-word)` → `Plug(0)`). Use `Keymap::register_plug()`
    /// to allocate ids.
    Plug(u32),

    /// `<Action>` pseudo-key for host action invocations (IdeaVim-style).
    ///
    /// When the engine encounters this key during dispatch, it resolves
    /// the name from the registry and emits `Effect::HostAction`.
    /// Use `Keymap::register_action()` to allocate ids.
    Action(u32),

    /// `<Cmd>` pseudo-key — starts an ex command sequence in a mapping RHS.
    ///
    /// Never generated from user input. In a mapping RHS like
    /// `<Cmd>echo "hi"<CR>`, the engine collects all keys between `<Cmd>`
    /// and the next `<CR>` and executes them as an ex command line without
    /// entering command-line mode or changing the current mode.
    Cmd,

    /// Bracketed paste start marker.
    /// Sent by terminal hosts when paste begins. Suppresses mappings,
    /// abbreviations, and auto-indent until `PasteEnd`.
    PasteStart,

    /// Bracketed paste end marker.
    PasteEnd,
}

impl Key {
    /// Parse from Vim notation.
    ///
    /// # Examples
    /// - `<CR>` → `Key::Enter`
    /// - `<Esc>` → `Key::Escape`
    /// - `<Tab>` → `Key::Tab`
    /// - `<BS>` → `Key::Backspace`
    /// - `<Del>` → `Key::Delete`
    /// - `<Space>` → `Key::Char(' ')`
    /// - `<Up>` → `Key::Up`
    /// - `<F1>` → `Key::F(1)`
    #[must_use]
    pub fn from_vim_notation(s: &str) -> Option<Self> {
        // Handle angle bracket notation
        if s.starts_with('<') && s.ends_with('>') {
            let inner = &s[1..s.len() - 1];
            return Self::from_vim_inner(inner);
        }

        // Single character
        let mut chars = s.chars();
        let first = chars.next()?;
        if chars.next().is_none() {
            return Some(Self::Char(first));
        }

        None
    }

    /// Parse the inner part of `<...>` notation.
    ///
    /// Zero-allocation: uses `eq_ignore_ascii_case` instead of
    /// heap-allocating `to_lowercase()`. Vim key names are ASCII-only.
    fn from_vim_inner(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("cr")
            || s.eq_ignore_ascii_case("return")
            || s.eq_ignore_ascii_case("enter")
        {
            return Some(Self::Enter);
        }
        if s.eq_ignore_ascii_case("esc") || s.eq_ignore_ascii_case("escape") {
            return Some(Self::Escape);
        }
        if s.eq_ignore_ascii_case("tab") {
            return Some(Self::Tab);
        }
        if s.eq_ignore_ascii_case("bs") || s.eq_ignore_ascii_case("backspace") {
            return Some(Self::Backspace);
        }
        if s.eq_ignore_ascii_case("del") || s.eq_ignore_ascii_case("delete") {
            return Some(Self::Delete);
        }
        if s.eq_ignore_ascii_case("space") {
            return Some(Self::Char(' '));
        }
        if s.eq_ignore_ascii_case("bslash") || s.eq_ignore_ascii_case("backslash") {
            return Some(Self::Char('\\'));
        }
        if s.eq_ignore_ascii_case("lt") {
            return Some(Self::Char('<'));
        }
        if s.eq_ignore_ascii_case("gt") {
            return Some(Self::Char('>'));
        }
        if s.eq_ignore_ascii_case("bar") {
            return Some(Self::Char('|'));
        }
        if s.eq_ignore_ascii_case("up") {
            return Some(Self::Up);
        }
        if s.eq_ignore_ascii_case("down") {
            return Some(Self::Down);
        }
        if s.eq_ignore_ascii_case("left") {
            return Some(Self::Left);
        }
        if s.eq_ignore_ascii_case("right") {
            return Some(Self::Right);
        }
        if s.eq_ignore_ascii_case("home") {
            return Some(Self::Home);
        }
        if s.eq_ignore_ascii_case("end") {
            return Some(Self::End);
        }
        if s.eq_ignore_ascii_case("pageup") || s.eq_ignore_ascii_case("prior") {
            return Some(Self::PageUp);
        }
        if s.eq_ignore_ascii_case("pagedown") || s.eq_ignore_ascii_case("next") {
            return Some(Self::PageDown);
        }
        if s.eq_ignore_ascii_case("insert") || s.eq_ignore_ascii_case("ins") {
            return Some(Self::Insert);
        }
        if s.eq_ignore_ascii_case("leader") {
            return Some(Self::Leader);
        }
        if s.eq_ignore_ascii_case("localleader") {
            return Some(Self::LocalLeader);
        }
        // <Plug> without a name — sentinel. Actual registration happens
        // at the Keymap level via register_plug(name).
        if s.eq_ignore_ascii_case("plug") {
            return Some(Self::Plug(u32::MAX));
        }
        // <Action> without a name — sentinel. Actual registration via register_action(name).
        if s.eq_ignore_ascii_case("action") {
            return Some(Self::Action(u32::MAX));
        }
        // <Cmd> — starts ex command in mapping RHS.
        if s.eq_ignore_ascii_case("cmd") {
            return Some(Self::Cmd);
        }
        if s.eq_ignore_ascii_case("pastestart") {
            return Some(Self::PasteStart);
        }
        if s.eq_ignore_ascii_case("pasteend") {
            return Some(Self::PasteEnd);
        }
        // Function keys: F1..F12
        if let Some(&first) = s.as_bytes().first() {
            if first.eq_ignore_ascii_case(&b'f') && s.len() >= 2 {
                if let Ok(num) = s.get(1..).unwrap_or("").parse::<u8>() {
                    if (1..=24).contains(&num) {
                        return Some(Self::F(num));
                    }
                }
            }
        }
        None
    }

    /// Convert to Vim notation.
    #[must_use]
    pub fn to_vim_notation(&self) -> Cow<'static, str> {
        match self {
            Self::Char(c) => match c {
                ' ' => Cow::Borrowed("<Space>"),
                '<' => Cow::Borrowed("<lt>"),
                '>' => Cow::Borrowed("<gt>"),
                '|' => Cow::Borrowed("<Bar>"),
                '\\' => Cow::Borrowed("<Bslash>"),
                _ => Cow::Owned(c.to_string()),
            },
            Self::Enter => Cow::Borrowed("<CR>"),
            Self::Escape => Cow::Borrowed("<Esc>"),
            Self::Tab => Cow::Borrowed("<Tab>"),
            Self::Backspace => Cow::Borrowed("<BS>"),
            Self::Delete => Cow::Borrowed("<Del>"),
            Self::Up => Cow::Borrowed("<Up>"),
            Self::Down => Cow::Borrowed("<Down>"),
            Self::Left => Cow::Borrowed("<Left>"),
            Self::Right => Cow::Borrowed("<Right>"),
            Self::Home => Cow::Borrowed("<Home>"),
            Self::End => Cow::Borrowed("<End>"),
            Self::PageUp => Cow::Borrowed("<PageUp>"),
            Self::PageDown => Cow::Borrowed("<PageDown>"),
            Self::Insert => Cow::Borrowed("<Insert>"),
            Self::F(n) => Cow::Owned(format!("<F{n}>")),
            Self::Leader => Cow::Borrowed("<Leader>"),
            Self::LocalLeader => Cow::Borrowed("<LocalLeader>"),
            Self::Plug(id) => Cow::Owned(format!("<Plug>({id})")),
            Self::Action(id) => Cow::Owned(format!("<Action>({id})")),
            Self::Cmd => Cow::Borrowed("<Cmd>"),
            Self::PasteStart => Cow::Borrowed("<PasteStart>"),
            Self::PasteEnd => Cow::Borrowed("<PasteEnd>"),
        }
    }

    /// Check if this is a printable character.
    #[must_use]
    pub const fn is_char(&self) -> bool {
        matches!(self, Self::Char(_))
    }

    /// Get the character if this is a Char variant.
    #[must_use]
    pub const fn as_char(&self) -> Option<char> {
        match self {
            Self::Char(c) => Some(*c),
            _ => None,
        }
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_vim_notation())
    }
}

impl From<char> for Key {
    fn from(c: char) -> Self {
        Self::Char(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_vim_notation_named_keys() {
        assert_eq!(Key::from_vim_notation("<CR>"), Some(Key::Enter));
        assert_eq!(Key::from_vim_notation("<Esc>"), Some(Key::Escape));
        assert_eq!(Key::from_vim_notation("<Tab>"), Some(Key::Tab));
        assert_eq!(Key::from_vim_notation("<BS>"), Some(Key::Backspace));
        assert_eq!(Key::from_vim_notation("<Del>"), Some(Key::Delete));
        assert_eq!(Key::from_vim_notation("<Space>"), Some(Key::Char(' ')));
    }

    #[test]
    fn from_vim_notation_arrows() {
        assert_eq!(Key::from_vim_notation("<Up>"), Some(Key::Up));
        assert_eq!(Key::from_vim_notation("<Down>"), Some(Key::Down));
        assert_eq!(Key::from_vim_notation("<Left>"), Some(Key::Left));
        assert_eq!(Key::from_vim_notation("<Right>"), Some(Key::Right));
    }

    #[test]
    fn from_vim_notation_navigation() {
        assert_eq!(Key::from_vim_notation("<Home>"), Some(Key::Home));
        assert_eq!(Key::from_vim_notation("<End>"), Some(Key::End));
        assert_eq!(Key::from_vim_notation("<PageUp>"), Some(Key::PageUp));
        assert_eq!(Key::from_vim_notation("<PageDown>"), Some(Key::PageDown));
    }

    #[test]
    fn from_vim_notation_function_keys() {
        assert_eq!(Key::from_vim_notation("<F1>"), Some(Key::F(1)));
        assert_eq!(Key::from_vim_notation("<F12>"), Some(Key::F(12)));
        assert_eq!(Key::from_vim_notation("<F13>"), Some(Key::F(13)));
        assert_eq!(Key::from_vim_notation("<F24>"), Some(Key::F(24)));
        assert_eq!(Key::from_vim_notation("<F25>"), None);
    }

    #[test]
    fn f13_through_f24_parsed() {
        for num in 13..=24u8 {
            let notation = format!("F{num}");
            let parsed = Key::from_vim_inner(&notation);
            assert_eq!(parsed, Some(Key::F(num)), "<F{num}> should parse correctly");
        }
    }

    #[test]
    fn f25_rejected() {
        assert_eq!(Key::from_vim_inner("F25"), None, "F25 should not parse");
    }

    #[test]
    fn from_vim_notation_single_char() {
        assert_eq!(Key::from_vim_notation("j"), Some(Key::Char('j')));
        assert_eq!(Key::from_vim_notation("D"), Some(Key::Char('D')));
    }

    #[test]
    fn from_vim_notation_invalid() {
        assert_eq!(Key::from_vim_notation("abc"), None);
        assert_eq!(Key::from_vim_notation("<Unknown>"), None);
    }

    #[test]
    fn to_vim_notation_roundtrip() {
        let keys = [
            Key::Enter,
            Key::Escape,
            Key::Tab,
            Key::Backspace,
            Key::Delete,
            Key::Char(' '),
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::F(1),
            Key::F(12),
        ];
        for key in keys {
            let notation = key.to_vim_notation();
            let parsed = Key::from_vim_notation(&notation);
            assert_eq!(parsed, Some(key), "round-trip failed for {notation}");
        }
    }

    #[test]
    fn is_char_and_as_char() {
        assert!(Key::Char('x').is_char());
        assert_eq!(Key::Char('x').as_char(), Some('x'));
        assert!(!Key::Enter.is_char());
        assert_eq!(Key::Enter.as_char(), None);
    }

    #[test]
    fn from_char_impl() {
        let key: Key = 'z'.into();
        assert_eq!(key, Key::Char('z'));
    }

    #[test]
    fn leader_vim_notation_roundtrip() {
        assert_eq!(Key::from_vim_notation("<Leader>"), Some(Key::Leader));
        assert_eq!(Key::from_vim_notation("<leader>"), Some(Key::Leader));
        assert_eq!(Key::Leader.to_vim_notation(), "<Leader>");
    }

    #[test]
    fn display_matches_to_vim_notation() {
        let keys = [
            Key::Char('k'),
            Key::Char(' '),
            Key::Char('<'),
            Key::Char('>'),
            Key::Char('|'),
            Key::Char('\\'),
            Key::Enter,
            Key::Escape,
            Key::Tab,
            Key::Backspace,
            Key::Delete,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Insert,
            Key::F(1),
            Key::F(12),
            Key::Leader,
            Key::Plug(0),
            Key::Plug(42),
            Key::Action(0),
            Key::Action(7),
            Key::Cmd,
        ];
        for key in keys {
            assert_eq!(
                format!("{key}"),
                key.to_vim_notation().as_ref(),
                "Display != to_vim_notation for {key:?}"
            );
        }
    }

    #[test]
    fn display_plain_char() {
        assert_eq!(format!("{}", Key::Char('k')), "k");
        assert_eq!(format!("{}", Key::Char('Z')), "Z");
    }

    #[test]
    fn display_special_keys() {
        assert_eq!(format!("{}", Key::Escape), "<Esc>");
        assert_eq!(format!("{}", Key::Enter), "<CR>");
        assert_eq!(format!("{}", Key::Tab), "<Tab>");
        assert_eq!(format!("{}", Key::Backspace), "<BS>");
        assert_eq!(format!("{}", Key::F(1)), "<F1>");
        assert_eq!(format!("{}", Key::F(12)), "<F12>");
    }

    #[test]
    fn display_special_chars() {
        assert_eq!(format!("{}", Key::Char(' ')), "<Space>");
        assert_eq!(format!("{}", Key::Char('<')), "<lt>");
        assert_eq!(format!("{}", Key::Char('>')), "<gt>");
        assert_eq!(format!("{}", Key::Char('|')), "<Bar>");
        assert_eq!(format!("{}", Key::Char('\\')), "<Bslash>");
    }
}
