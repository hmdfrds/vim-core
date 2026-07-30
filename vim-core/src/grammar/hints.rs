//! Static hint tables for built-in prefix command groups.
//!
//! Each table pairs continuation keys with human-readable descriptions.
//! Used by the which-key query to provide descriptions for built-in
//! commands. User mappings overlay these via the keymap trie.

/// Display title for the `g` prefix group.
pub const G_PREFIX_TITLE: &str = "Goto / Misc (g)";
/// Continuation keys under `g` that act as operators (take a motion).
pub const G_PREFIX_OPERATOR_KEYS: &[&str] = &["u", "U", "~", "q", "w", "@", "?"];
/// Hint table for the `g` prefix: (key, description) pairs.
pub const G_PREFIX_HINTS: &[(&str, &str)] = &[
    ("g", "Go to first line"),
    ("j", "Display line down"),
    ("k", "Display line up"),
    ("e", "End of previous word"),
    ("E", "End of previous WORD"),
    ("0", "Screen line start"),
    ("$", "Screen line end"),
    ("^", "Screen first non-blank"),
    ("m", "Middle of screen line"),
    ("M", "Middle of text line"),
    ("o", "Go to byte offset"),
    (";", "Older changelist position"),
    (",", "Newer changelist position"),
    ("n", "Search object forward"),
    ("N", "Search object backward"),
    ("*", "Partial word search forward"),
    ("#", "Partial word search backward"),
    ("_", "Last non-blank"),
    ("d", "Go to definition"),
    ("i", "Insert at last insert position"),
    ("I", "Insert at column 0"),
    ("J", "Join lines (no space)"),
    ("a", "Show ASCII value"),
    ("8", "Show UTF-8 bytes"),
    ("p", "Put after, cursor after text"),
    ("P", "Put before, cursor after text"),
    ("R", "Virtual Replace mode"),
    ("v", "Reselect last visual"),
    ("h", "Enter Select mode (char)"),
    ("H", "Enter Select mode (line)"),
    (".", "Intent-aware repeat"),
    ("&", "Repeat :s on all lines"),
    ("-", "Earlier undo state"),
    ("+", "Later undo state"),
    ("u", "Lowercase operator"),
    ("U", "Uppercase operator"),
    ("~", "Toggle case operator"),
    ("q", "Format operator"),
    ("w", "Format (no join) operator"),
    ("@", "Operator-function"),
    ("?", "Rot13 operator"),
    ("[", "Select parent node"),
    ("]", "Select child node"),
    ("{", "Previous sibling node"),
    ("}", "Next sibling node"),
    ("(", "All sibling nodes"),
    (")", "All child nodes"),
    ("b", "Add cursor at next match"),
    ("B", "Add cursor at previous match"),
    ("s", "Skip current match"),
    ("Ctrl-A", "Sequential increment"),
    ("Ctrl-H", "Enter Select mode (block)"),
    ("Ctrl-X", "Sequential decrement"),
];

/// Display title for the `z` prefix group.
pub const Z_PREFIX_TITLE: &str = "Scroll / Fold (z)";
/// Hint table for the `z` prefix: (key, description) pairs.
pub const Z_PREFIX_HINTS: &[(&str, &str)] = &[
    ("z", "Center viewport on cursor"),
    ("t", "Scroll cursor to top"),
    ("b", "Scroll cursor to bottom"),
    ("<CR>", "First non-blank + top"),
    (".", "First non-blank + center"),
    ("-", "First non-blank + bottom"),
    ("h", "Scroll left one column"),
    ("l", "Scroll right one column"),
    ("H", "Scroll left half screen"),
    ("L", "Scroll right half screen"),
    ("s", "Scroll cursor to left edge"),
    ("e", "Scroll cursor to right edge"),
    ("o", "Open fold"),
    ("O", "Open fold recursively"),
    ("c", "Close fold"),
    ("C", "Close fold recursively"),
    ("a", "Toggle fold"),
    ("A", "Toggle fold recursively"),
    ("R", "Open all folds"),
    ("M", "Close all folds"),
    ("d", "Delete fold"),
    ("D", "Delete fold recursively"),
    ("E", "Eliminate all folds"),
    ("i", "Toggle fold enable"),
    ("n", "Disable folding"),
    ("N", "Enable folding"),
];

/// Display title for the `Z` prefix group.
pub const Z_UPPER_PREFIX_TITLE: &str = "Write / Quit (Z)";
/// Hint table for the `Z` prefix: (key, description) pairs.
pub const Z_UPPER_PREFIX_HINTS: &[(&str, &str)] =
    &[("Z", "Write and quit"), ("Q", "Quit without saving")];

/// Display title for the `[` prefix group.
pub const BRACKET_OPEN_TITLE: &str = "Previous ([)";
/// Hint table for the `[` prefix: (key, description) pairs.
pub const BRACKET_OPEN_HINTS: &[(&str, &str)] = &[
    ("[", "Previous section start"),
    ("]", "Previous section end"),
    ("{", "Unmatched { backward"),
    ("(", "Unmatched ( backward"),
    ("m", "Previous method start"),
    ("M", "Previous method end"),
    ("/", "Previous comment start"),
    ("p", "Put with indent adjust (after)"),
    ("P", "Put with indent adjust (before)"),
    ("<Space>", "Insert blank line above"),
    ("b", "Previous bracket pair"),
    ("q", "Previous quote pair"),
    ("i", "Previous indent block"),
    ("-", "Less indented block"),
    ("+", "More indented block"),
    ("'", "Previous mark"),
];

/// Display title for the `]` prefix group.
pub const BRACKET_CLOSE_TITLE: &str = "Next (])";
/// Hint table for the `]` prefix: (key, description) pairs.
pub const BRACKET_CLOSE_HINTS: &[(&str, &str)] = &[
    ("]", "Next section start"),
    ("[", "Next section end"),
    ("}", "Unmatched } forward"),
    (")", "Unmatched ) forward"),
    ("m", "Next method start"),
    ("M", "Next method end"),
    ("/", "Next comment end"),
    ("p", "Put with indent adjust (after)"),
    ("P", "Put with indent adjust (before)"),
    ("<Space>", "Insert blank line below"),
    ("b", "Next bracket pair"),
    ("q", "Next quote pair"),
    ("i", "Next indent block"),
    ("-", "Less indented block"),
    ("+", "More indented block"),
    ("'", "Next mark"),
];

/// Display title for the Ctrl-W window prefix group.
pub const WINDOW_PREFIX_TITLE: &str = "Window (Ctrl-W)";
/// Hint table for the Ctrl-W prefix: (key, description) pairs.
pub const WINDOW_PREFIX_HINTS: &[(&str, &str)] = &[
    ("S", "Split horizontal"),
    ("s", "Split horizontal"),
    ("v", "Split vertical"),
    ("n", "New empty split"),
    ("c", "Close window"),
    ("o", "Close other windows"),
    ("w", "Next window"),
    ("W", "Previous window"),
    ("h", "Move to left window"),
    ("l", "Move to right window"),
    ("k", "Move to window above"),
    ("j", "Move to window below"),
    ("=", "Equalize window sizes"),
    ("+", "Increase height"),
    ("-", "Decrease height"),
    (">", "Increase width"),
    ("<", "Decrease width"),
    ("r", "Rotate windows down"),
    ("R", "Rotate windows up"),
    ("q", "Close window"),
];

/// Look up the static hint table for a given prefix character.
#[must_use]
pub const fn builtin_prefix_hints(
    prefix: char,
) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
    match prefix {
        'g' => Some((G_PREFIX_TITLE, G_PREFIX_HINTS)),
        'z' => Some((Z_PREFIX_TITLE, Z_PREFIX_HINTS)),
        'Z' => Some((Z_UPPER_PREFIX_TITLE, Z_UPPER_PREFIX_HINTS)),
        '[' => Some((BRACKET_OPEN_TITLE, BRACKET_OPEN_HINTS)),
        ']' => Some((BRACKET_CLOSE_TITLE, BRACKET_CLOSE_HINTS)),
        _ => None,
    }
}

/// Look up the static hint table for the Ctrl-W (window) prefix.
#[must_use]
pub const fn window_prefix_hints() -> (&'static str, &'static [(&'static str, &'static str)]) {
    (WINDOW_PREFIX_TITLE, WINDOW_PREFIX_HINTS)
}

/// Returns true if the given key string is a g-prefix operator key.
#[must_use]
pub fn is_g_prefix_operator_key(key: &str) -> bool {
    G_PREFIX_OPERATOR_KEYS.contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_prefix_hints_g() {
        let (title, hints) = builtin_prefix_hints('g').unwrap();
        assert_eq!(title, "Goto / Misc (g)");
        assert!(hints.len() > 30);
        assert!(hints.iter().any(|(k, _)| *k == "d"));
    }

    #[test]
    fn builtin_prefix_hints_z() {
        let (title, hints) = builtin_prefix_hints('z').unwrap();
        assert_eq!(title, "Scroll / Fold (z)");
        assert!(hints.iter().any(|(k, _)| *k == "z"));
    }

    #[test]
    fn builtin_prefix_hints_unknown() {
        assert!(builtin_prefix_hints('x').is_none());
    }

    #[test]
    fn window_hints_not_empty() {
        let (title, hints) = window_prefix_hints();
        assert_eq!(title, "Window (Ctrl-W)");
        assert!(hints.len() >= 18);
    }

    #[test]
    fn g_operator_keys_filter() {
        assert!(is_g_prefix_operator_key("u"));
        assert!(is_g_prefix_operator_key("U"));
        assert!(!is_g_prefix_operator_key("d"));
        assert!(!is_g_prefix_operator_key("g"));
    }

    /// Brute-force cross-check: feed every printable ASCII char AND every
    /// Ctrl+key combination through the parser's prefix handler and verify
    /// the hint table covers all recognized keys. If someone adds a new
    /// prefix command in the handler but forgets the hint, this test fails.
    #[test]
    fn hint_tables_cover_all_handler_recognized_keys() {
        use crate::grammar::Parser;
        use crate::keymap::{KeyEvent, Keymap};
        use crate::primitives::Mode;

        let hint_keys = |table: &[(&str, &str)]| -> std::collections::HashSet<String> {
            table.iter().map(|(k, _)| k.to_string()).collect()
        };

        let keymap = Keymap::new();

        // Build test keys: all printable ASCII + all Ctrl+letter combinations.
        // Exclude Ctrl-C (universal cancel, not a prefix-specific command).
        let mut test_keys: Vec<(KeyEvent, String)> = Vec::new();
        for b in b'!'..=b'~' {
            let c = b as char;
            test_keys.push((KeyEvent::char(c), c.to_string()));
        }
        for b in b'a'..=b'z' {
            let c = b as char;
            if c == 'c' {
                continue; // Ctrl-C is universal cancel, not prefix-specific
            }
            test_keys.push((
                KeyEvent::ctrl(c),
                format!("Ctrl-{}", c.to_ascii_uppercase()),
            ));
        }

        for (prefix, hints_table) in [
            ('g', G_PREFIX_HINTS),
            ('z', Z_PREFIX_HINTS),
            ('Z', Z_UPPER_PREFIX_HINTS),
        ] {
            let known = hint_keys(hints_table);
            let mut recognized = std::collections::HashSet::new();

            for (key, display) in &test_keys {
                let mut parser = Parser::new();
                let prefix_result = parser.process(KeyEvent::char(prefix), &keymap, Mode::Normal);
                if !matches!(prefix_result, crate::grammar::GrammarResult::Continue(_)) {
                    continue;
                }
                let result = parser.process(key.clone(), &keymap, Mode::Normal);
                if !matches!(result, crate::grammar::GrammarResult::Invalid) {
                    recognized.insert(display.clone());
                }
            }

            let missing: Vec<_> = recognized.difference(&known).cloned().collect();
            assert!(
                missing.is_empty(),
                "Prefix '{prefix}': handler recognizes keys {missing:?} but hint table is missing them. \
                 Add entries to the hint table.",
            );
        }

        // Window prefix (Ctrl-W) — the handler accepts both plain chars and
        // Ctrl-modified versions (Ctrl-W Ctrl-H = Ctrl-W h). Only test
        // plain chars since Ctrl variants are aliases, not new commands.
        {
            let known = hint_keys(WINDOW_PREFIX_HINTS);
            let mut recognized = std::collections::HashSet::new();

            for b in b'!'..=b'~' {
                let c = b as char;
                let key = KeyEvent::char(c);
                let mut parser = Parser::new();
                let prefix_result = parser.process(KeyEvent::ctrl('w'), &keymap, Mode::Normal);
                if !matches!(prefix_result, crate::grammar::GrammarResult::Continue(_)) {
                    continue;
                }
                let result = parser.process(key, &keymap, Mode::Normal);
                if !matches!(result, crate::grammar::GrammarResult::Invalid) {
                    recognized.insert(c.to_string());
                }
            }

            let missing: Vec<_> = recognized.difference(&known).cloned().collect();
            assert!(
                missing.is_empty(),
                "Ctrl-W prefix: handler recognizes keys {missing:?} but WINDOW_PREFIX_HINTS is missing them.",
            );
        }

        // Bracket prefixes — exclude text-object seeking keys (catch-all
        // fallback via TextObjectKind::from_char that fires for any text
        // object character not already handled by explicit bracket motions).
        let text_object_chars: std::collections::HashSet<String> = {
            let mut set = std::collections::HashSet::new();
            for b in b'!'..=b'~' {
                let c = b as char;
                if crate::grammar::types::TextObjectKind::from_char(c).is_some() {
                    set.insert(c.to_string());
                }
            }
            set
        };

        for prefix in ['[', ']'] {
            let hints_table = if prefix == '[' {
                BRACKET_OPEN_HINTS
            } else {
                BRACKET_CLOSE_HINTS
            };
            let known = hint_keys(hints_table);
            let mut recognized = std::collections::HashSet::new();

            for (key, display) in &test_keys {
                let mut parser = Parser::new();
                let prefix_result = parser.process(KeyEvent::char(prefix), &keymap, Mode::Normal);
                if !matches!(prefix_result, crate::grammar::GrammarResult::Continue(_)) {
                    continue;
                }
                let result = parser.process(key.clone(), &keymap, Mode::Normal);
                if !matches!(result, crate::grammar::GrammarResult::Invalid) {
                    recognized.insert(display.clone());
                }
            }

            // Filter out text-object seeking keys — those are a generic
            // fallback, not specific prefix commands that need hint entries.
            let explicit: std::collections::HashSet<_> =
                recognized.difference(&text_object_chars).cloned().collect();
            let missing: Vec<_> = explicit.difference(&known).cloned().collect();
            assert!(
                missing.is_empty(),
                "Prefix '{prefix}': handler recognizes keys {missing:?} but hint table is missing them.",
            );
        }
    }
}
