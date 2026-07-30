//! Per-mode key interest sets for the passthrough system.
//!
//! Provides [`KeyInterestSet`] — a collection of Vim notation strings grouped
//! by mode, representing every key the engine would handle. The host shell
//! uses this to configure keybindings: keys in the interest set get routed to
//! the engine, everything else passes through to the host editor natively.
//!
//! # Architecture
//!
//! The interest set is the core of the five-layer passthrough architecture:
//!
//! 1. **Core keymap** — built-in Normal/Visual/Operator-pending bindings
//! 2. **Grammar keys** — Insert/Command-line mode special keys (not in the
//!    core keymap table, handled by grammar handlers directly)
//! 3. **Langmap FROM-keys** — Normal/Visual FROM-side characters that would be
//!    remapped by `:set langmap`; the host must route them to the engine
//! 4. **User mappings** — first keys from all user mapping tries
//! 5. **Handler map filter** — keys delegated to the host via `:sethandler`
//!    are excluded
//!
//! Printable characters are NOT included. In Insert and Command-line modes,
//! printable characters are routed via the host's `type` command, not through
//! keybindings. The interest set only covers keys that the host must route
//! through keybindings (special keys, Ctrl combos, etc.).

use super::VimEngine;
use crate::keymap::{Key, KeyClass, KeyEvent, MappingMode, Modifiers};

/// Per-mode sets of Vim notation strings representing every key the engine
/// would handle.
///
/// Each field is a sorted, deduplicated `Vec<String>` of Vim notation strings
/// (e.g. `"j"`, `"<C-w>"`, `"<Esc>"`, `"<BS>"`). The host shell registers
/// keybindings for exactly these keys in each mode.
///
/// # Lifecycle
///
/// Recompute whenever the keymap changes (user maps/unmaps, buffer-local
/// mappings change, `:sethandler` changes). The result is a snapshot — it
/// does not track live changes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyInterestSet {
    /// Keys handled in Normal mode (also covers Operator-pending, since OP
    /// is entered from Normal and the host keybinding context is still Normal).
    pub normal: Vec<String>,
    /// Keys handled in Insert/Replace/VirtualReplace modes.
    pub insert: Vec<String>,
    /// Keys handled in Visual/Select modes.
    pub visual: Vec<String>,
    /// Keys handled in Command-line mode.
    pub command: Vec<String>,
}

/// Build the list of insert-mode grammar keys (special keys only).
///
/// These are keys handled by `grammar/handlers/insert.rs` that are NOT in the
/// core keymap table (which has no Insert-mode entries). Printable characters
/// are excluded — they go through the `type` command.
fn insert_grammar_keys() -> Vec<KeyEvent> {
    let mut keys = vec![
        // Escape-class keys
        KeyEvent::escape(),
        KeyEvent::ctrl('c'),
        KeyEvent::ctrl('['),
        // Named special keys (bare, no command modifiers)
        KeyEvent::new(Key::Tab, Modifiers::NONE),
        KeyEvent::new(Key::Tab, Modifiers::SHIFT),
        KeyEvent::new(Key::Enter, Modifiers::NONE),
        KeyEvent::new(Key::Backspace, Modifiers::NONE),
        KeyEvent::new(Key::Delete, Modifiers::NONE),
        KeyEvent::new(Key::Insert, Modifiers::NONE),
        // Arrow keys (bare)
        KeyEvent::new(Key::Up, Modifiers::NONE),
        KeyEvent::new(Key::Down, Modifiers::NONE),
        KeyEvent::new(Key::Left, Modifiers::NONE),
        KeyEvent::new(Key::Right, Modifiers::NONE),
        // Navigation (bare)
        KeyEvent::new(Key::Home, Modifiers::NONE),
        KeyEvent::new(Key::End, Modifiers::NONE),
        KeyEvent::new(Key::PageUp, Modifiers::NONE),
        KeyEvent::new(Key::PageDown, Modifiers::NONE),
        // Shift+arrow keys (word/page movement)
        KeyEvent::new(Key::Left, Modifiers::SHIFT),
        KeyEvent::new(Key::Right, Modifiers::SHIFT),
        KeyEvent::new(Key::Up, Modifiers::SHIFT),
        KeyEvent::new(Key::Down, Modifiers::SHIFT),
        // Ctrl+Left / Ctrl+Right (word movement)
        KeyEvent::new(Key::Left, Modifiers::CTRL),
        KeyEvent::new(Key::Right, Modifiers::CTRL),
    ];

    // Ctrl+character keys (exact set from handle_insert_ctrl)
    for c in [
        'h', 'j', 'k', 'm', 'i', 't', 'd', 'w', 'u', 'o', 'a', 'v', 'r', 'e', 'y', 'g', 'x', 'n',
        'p', '@',
    ] {
        keys.push(KeyEvent::ctrl(c));
    }

    keys
}

/// Build the list of command-line mode grammar keys (special keys only).
///
/// These are keys handled by `mode/command_line.rs::handle_key` that produce
/// an action other than `Ignore`. Printable characters are excluded.
fn command_line_grammar_keys() -> Vec<KeyEvent> {
    let mut keys = vec![
        // Escape-class keys
        KeyEvent::escape(),
        KeyEvent::ctrl('c'),
        KeyEvent::ctrl('['),
        // Enter (commit)
        KeyEvent::new(Key::Enter, Modifiers::NONE),
        // Editing keys
        KeyEvent::new(Key::Backspace, Modifiers::NONE),
        KeyEvent::new(Key::Delete, Modifiers::NONE),
        KeyEvent::new(Key::Tab, Modifiers::NONE),
        KeyEvent::new(Key::Tab, Modifiers::SHIFT),
        // Left/Right — bare, Ctrl, Shift (reject Alt/Meta)
        KeyEvent::new(Key::Left, Modifiers::NONE),
        KeyEvent::new(Key::Right, Modifiers::NONE),
        KeyEvent::new(Key::Left, Modifiers::CTRL),
        KeyEvent::new(Key::Right, Modifiers::CTRL),
        KeyEvent::new(Key::Left, Modifiers::SHIFT),
        KeyEvent::new(Key::Right, Modifiers::SHIFT),
        // Home/End/Up/Down — bare or Shift only
        KeyEvent::new(Key::Home, Modifiers::NONE),
        KeyEvent::new(Key::End, Modifiers::NONE),
        KeyEvent::new(Key::Up, Modifiers::NONE),
        KeyEvent::new(Key::Down, Modifiers::NONE),
        KeyEvent::new(Key::Home, Modifiers::SHIFT),
        KeyEvent::new(Key::End, Modifiers::SHIFT),
        KeyEvent::new(Key::Up, Modifiers::SHIFT),
        KeyEvent::new(Key::Down, Modifiers::SHIFT),
    ];

    // Ctrl+character editing commands
    for c in ['w', 'u', 'k', 'h', 'r', 'f'] {
        keys.push(KeyEvent::ctrl(c));
    }

    keys
}

impl VimEngine {
    /// Compute the per-mode key interest set.
    ///
    /// Combines core keymap entries, grammar-handled special keys, and user
    /// mapping first keys, then filters out keys delegated to the host via
    /// `:sethandler`. Each mode's list is deduplicated and sorted.
    ///
    /// # When to call
    ///
    /// Recompute whenever:
    /// - User maps or unmaps a key (`:nmap`, `:nunmap`, etc.)
    /// - Buffer-local mappings change (`set_buffer_mappings`)
    /// - `:sethandler` changes
    ///
    /// # Complexity
    ///
    /// Time: O(K log K) where K = total keys across all modes (dominated by
    /// the sort/dedup pass). K is typically ~200-400 for a default keymap.
    ///
    /// Space: O(K) for the output vectors.
    #[must_use]
    pub fn compute_key_interest(&self) -> KeyInterestSet {
        fn to_sorted_notation(keys: Vec<KeyEvent>) -> Vec<String> {
            let mut notations: Vec<String> = keys
                .into_iter()
                .map(|k| k.to_vim_notation().into_owned())
                .collect();
            notations.sort();
            notations.dedup();
            notations
        }

        let mut normal_keys: Vec<KeyEvent> = Vec::new();
        let mut visual_keys: Vec<KeyEvent> = Vec::new();
        let mut insert_keys: Vec<KeyEvent> = Vec::new();
        let mut command_keys: Vec<KeyEvent> = Vec::new();

        // ── 1. Core keymap entries ──────────────────────────────────────
        // Normal + Operator-pending both contribute to the "normal" interest
        // set because OP mode is entered from Normal and the host's keybinding
        // context remains Normal.
        for (key, class) in self.keymap.core_entries(MappingMode::Normal) {
            if *class != KeyClass::Unknown {
                normal_keys.push(*key);
            }
        }
        for (key, class) in self.keymap.core_entries(MappingMode::Operator) {
            if *class != KeyClass::Unknown {
                normal_keys.push(*key);
            }
        }
        for (key, class) in self.keymap.core_entries(MappingMode::Visual) {
            if *class != KeyClass::Unknown {
                visual_keys.push(*key);
            }
        }

        // ── 2. Grammar-handled special keys ─────────────────────────────
        insert_keys.extend(insert_grammar_keys());
        command_keys.extend(command_line_grammar_keys());

        // ── 3. Escape-class keys for Normal and Visual ──────────────────
        // These are always handled (exit visual, cancel pending, beep in
        // normal). The core keymap already includes Escape as KeyClass::Escape
        // for Normal/Visual, but Ctrl-C and Ctrl-[ may or may not be in the
        // table depending on how they're classified. Add all three explicitly
        // to be safe — dedup will remove duplicates.
        let escape_keys = [KeyEvent::escape(), KeyEvent::ctrl('c'), KeyEvent::ctrl('[')];
        normal_keys.extend_from_slice(&escape_keys);
        visual_keys.extend_from_slice(&escape_keys);

        // ── 4. Langmap FROM-side keys ────────────────────────────────────
        // Keys the host should route to the engine because they will be
        // remapped to command keys via langmap in Normal/Visual/OP-pending
        // modes. Insert and Command-line modes are excluded: Vim only applies
        // langmap in Normal, Visual, and Operator-pending modes.
        if !self.langmap_table.is_empty() {
            for (from, _to) in self.langmap_table.entries() {
                let event = KeyEvent::char(from);
                normal_keys.push(event);
                visual_keys.push(event);
            }
        }

        // ── 5. User mapping first keys ──────────────────────────────────
        for (key, mm) in self.keymap.user_mapping_first_keys() {
            match mm {
                MappingMode::Normal | MappingMode::Operator => {
                    normal_keys.push(key);
                }
                MappingMode::Visual | MappingMode::VisualOnly | MappingMode::SelectOnly => {
                    visual_keys.push(key);
                }
                MappingMode::Insert => {
                    insert_keys.push(key);
                }
                MappingMode::Command => {
                    command_keys.push(key);
                }
            }
        }

        // ── 6. Filter out host-delegated keys ───────────────────────────
        normal_keys.retain(|k| !self.handler_map.is_host_handled(*k, MappingMode::Normal));
        visual_keys.retain(|k| !self.handler_map.is_host_handled(*k, MappingMode::Visual));
        insert_keys.retain(|k| !self.handler_map.is_host_handled(*k, MappingMode::Insert));
        command_keys.retain(|k| !self.handler_map.is_host_handled(*k, MappingMode::Command));

        // ── 7. Convert to Vim notation, sort, dedup ─────────────────────
        KeyInterestSet {
            normal: to_sorted_notation(normal_keys),
            insert: to_sorted_notation(insert_keys),
            visual: to_sorted_notation(visual_keys),
            command: to_sorted_notation(command_keys),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::VimEngine;

    fn make_interest() -> KeyInterestSet {
        let engine = VimEngine::new();
        engine.compute_key_interest()
    }

    // ── Normal mode interest ────────────────────────────────────────────

    #[test]
    fn normal_contains_basic_motions() {
        let interest = make_interest();
        assert!(
            interest.normal.contains(&"j".to_string()),
            "normal should contain j"
        );
        assert!(
            interest.normal.contains(&"k".to_string()),
            "normal should contain k"
        );
        assert!(
            interest.normal.contains(&"h".to_string()),
            "normal should contain h"
        );
        assert!(
            interest.normal.contains(&"l".to_string()),
            "normal should contain l"
        );
        assert!(
            interest.normal.contains(&"w".to_string()),
            "normal should contain w"
        );
        assert!(
            interest.normal.contains(&"b".to_string()),
            "normal should contain b"
        );
    }

    #[test]
    fn normal_contains_operators() {
        let interest = make_interest();
        assert!(
            interest.normal.contains(&"d".to_string()),
            "normal should contain d"
        );
        assert!(
            interest.normal.contains(&"c".to_string()),
            "normal should contain c"
        );
        assert!(
            interest.normal.contains(&"y".to_string()),
            "normal should contain y"
        );
    }

    #[test]
    fn normal_contains_escape() {
        let interest = make_interest();
        assert!(
            interest.normal.contains(&"<Esc>".to_string()),
            "normal should contain <Esc>"
        );
    }

    #[test]
    fn normal_does_not_contain_host_keys() {
        let interest = make_interest();
        // Ctrl+S and Ctrl+Z are not in the core keymap — they should not
        // appear in the interest set.
        assert!(
            !interest.normal.contains(&"<C-s>".to_string()),
            "normal should NOT contain <C-s>"
        );
        assert!(
            !interest.normal.contains(&"<C-z>".to_string()),
            "normal should NOT contain <C-z>"
        );
    }

    // ── Insert mode interest ────────────────────────────────────────────

    #[test]
    fn insert_contains_special_keys() {
        let interest = make_interest();
        assert!(
            interest.insert.contains(&"<BS>".to_string()),
            "insert should contain <BS>"
        );
        assert!(
            interest.insert.contains(&"<Del>".to_string()),
            "insert should contain <Del>"
        );
        assert!(
            interest.insert.contains(&"<CR>".to_string()),
            "insert should contain <CR>"
        );
        assert!(
            interest.insert.contains(&"<Tab>".to_string()),
            "insert should contain <Tab>"
        );
    }

    #[test]
    fn insert_contains_navigation_keys() {
        let interest = make_interest();
        assert!(
            interest.insert.contains(&"<Up>".to_string()),
            "insert should contain <Up>"
        );
        assert!(
            interest.insert.contains(&"<Down>".to_string()),
            "insert should contain <Down>"
        );
        assert!(
            interest.insert.contains(&"<Left>".to_string()),
            "insert should contain <Left>"
        );
        assert!(
            interest.insert.contains(&"<Right>".to_string()),
            "insert should contain <Right>"
        );
        assert!(
            interest.insert.contains(&"<Home>".to_string()),
            "insert should contain <Home>"
        );
        assert!(
            interest.insert.contains(&"<End>".to_string()),
            "insert should contain <End>"
        );
    }

    #[test]
    fn insert_contains_ctrl_r() {
        let interest = make_interest();
        assert!(
            interest.insert.contains(&"<C-r>".to_string()),
            "insert should contain <C-r>"
        );
    }

    #[test]
    fn insert_contains_escape() {
        let interest = make_interest();
        assert!(
            interest.insert.contains(&"<Esc>".to_string()),
            "insert should contain <Esc>"
        );
    }

    // ── Command-line mode interest ──────────────────────────────────────

    #[test]
    fn command_contains_core_keys() {
        let interest = make_interest();
        assert!(
            interest.command.contains(&"<CR>".to_string()),
            "command should contain <CR>"
        );
        assert!(
            interest.command.contains(&"<Esc>".to_string()),
            "command should contain <Esc>"
        );
        assert!(
            interest.command.contains(&"<BS>".to_string()),
            "command should contain <BS>"
        );
        assert!(
            interest.command.contains(&"<Tab>".to_string()),
            "command should contain <Tab>"
        );
    }

    // ── Deduplication ───────────────────────────────────────────────────

    #[test]
    fn sets_are_deduplicated() {
        let interest = make_interest();

        fn has_no_duplicates(v: &[String], mode: &str) {
            let mut seen = std::collections::HashSet::new();
            for s in v {
                assert!(seen.insert(s), "{mode} interest set has duplicate: {s}");
            }
        }

        has_no_duplicates(&interest.normal, "normal");
        has_no_duplicates(&interest.insert, "insert");
        has_no_duplicates(&interest.visual, "visual");
        has_no_duplicates(&interest.command, "command");
    }

    // ── Sorting ─────────────────────────────────────────────────────────

    #[test]
    fn sets_are_sorted() {
        let interest = make_interest();

        fn is_sorted(v: &[String], mode: &str) {
            for pair in v.windows(2) {
                assert!(
                    pair[0] <= pair[1],
                    "{mode} interest set is not sorted: {:?} > {:?}",
                    pair[0],
                    pair[1]
                );
            }
        }

        is_sorted(&interest.normal, "normal");
        is_sorted(&interest.insert, "insert");
        is_sorted(&interest.visual, "visual");
        is_sorted(&interest.command, "command");
    }

    // ── Non-empty sanity checks ─────────────────────────────────────────

    #[test]
    fn all_mode_sets_are_non_empty() {
        let interest = make_interest();
        assert!(
            !interest.normal.is_empty(),
            "normal interest set should not be empty"
        );
        assert!(
            !interest.insert.is_empty(),
            "insert interest set should not be empty"
        );
        assert!(
            !interest.visual.is_empty(),
            "visual interest set should not be empty"
        );
        assert!(
            !interest.command.is_empty(),
            "command interest set should not be empty"
        );
    }

    // ── Visual mode interest ────────────────────────────────────────────

    #[test]
    fn visual_contains_escape() {
        let interest = make_interest();
        assert!(
            interest.visual.contains(&"<Esc>".to_string()),
            "visual should contain <Esc>"
        );
    }

    #[test]
    fn visual_contains_motions_and_operators() {
        let interest = make_interest();
        assert!(
            interest.visual.contains(&"j".to_string()),
            "visual should contain j"
        );
        assert!(
            interest.visual.contains(&"d".to_string()),
            "visual should contain d"
        );
    }

    // ── Insert mode does NOT contain printable characters ───────────────

    #[test]
    fn insert_does_not_contain_printable_chars() {
        let interest = make_interest();
        // Printable characters go through the `type` command, not keybindings.
        assert!(
            !interest.insert.contains(&"a".to_string()),
            "insert should NOT contain 'a' (printable)"
        );
        assert!(
            !interest.insert.contains(&"Z".to_string()),
            "insert should NOT contain 'Z' (printable)"
        );
        assert!(
            !interest.insert.contains(&"0".to_string()),
            "insert should NOT contain '0' (printable)"
        );
    }

    // ── Command mode does NOT contain printable characters ──────────────

    #[test]
    fn command_does_not_contain_printable_chars() {
        let interest = make_interest();
        assert!(
            !interest.command.contains(&"a".to_string()),
            "command should NOT contain 'a' (printable)"
        );
        assert!(
            !interest.command.contains(&"<Space>".to_string()),
            "command should NOT contain <Space> (printable)"
        );
    }

    // ── sethandler filtering ────────────────────────────────────────────

    #[test]
    fn sethandler_filters_keys() {
        use crate::keymap::Handler;

        let mut engine = VimEngine::new();
        // Delegate <C-a> to host in Normal mode.
        engine
            .handler_map_mut()
            .set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Host);
        let interest = engine.compute_key_interest();
        // <C-a> is normally in the core keymap for Normal mode, but
        // sethandler delegation should exclude it.
        assert!(
            !interest.normal.contains(&"<C-a>".to_string()),
            "normal should NOT contain <C-a> after sethandler delegation"
        );
    }

    // ── Langmap FROM-key interest ───────────────────────────────────────

    #[test]
    fn interest_set_includes_langmap_from_keys() {
        let mut engine = VimEngine::new();
        engine.langmap_table = crate::keymap::LangmapTable::parse("йц;qw").unwrap();
        let interest = engine.compute_key_interest();

        assert!(
            interest.normal.contains(&"й".to_string()),
            "langmap FROM key й should be in normal interest"
        );
        assert!(
            interest.normal.contains(&"ц".to_string()),
            "langmap FROM key ц should be in normal interest"
        );
        assert!(
            interest.visual.contains(&"й".to_string()),
            "langmap FROM key й should be in visual interest"
        );
        assert!(
            interest.visual.contains(&"ц".to_string()),
            "langmap FROM key ц should be in visual interest"
        );
    }

    #[test]
    fn interest_set_langmap_from_keys_absent_when_empty() {
        // With an empty langmap table the interest set should not gain any
        // extra characters beyond the core keymap. This verifies the is_empty
        // short-circuit path.
        let engine_no_langmap = VimEngine::new();
        let interest_no = engine_no_langmap.compute_key_interest();

        let mut engine_with_langmap = VimEngine::new();
        engine_with_langmap.langmap_table = crate::keymap::LangmapTable::parse("йq").unwrap();
        let interest_with = engine_with_langmap.compute_key_interest();

        // The langmap variant should have й in normal…
        assert!(
            interest_with.normal.contains(&"й".to_string()),
            "langmap FROM key й should appear in normal with langmap set"
        );
        // …but not in the empty-langmap variant.
        assert!(
            !interest_no.normal.contains(&"й".to_string()),
            "й should NOT appear in normal with no langmap"
        );

        // Insert and command-line sets should be identical in both cases:
        // langmap does not apply in those modes.
        assert_eq!(
            interest_no.insert, interest_with.insert,
            "insert interest set must not be affected by langmap"
        );
        assert_eq!(
            interest_no.command, interest_with.command,
            "command interest set must not be affected by langmap"
        );
    }
}
