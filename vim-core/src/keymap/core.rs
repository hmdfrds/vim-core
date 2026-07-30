//! Core Vim keybindings.
//!
//! Default key classifications for Vim's standard keybindings.

use super::keymap::{MappingMode, ModeMap};
use super::{Key, KeyClass, KeyEvent, Modifiers};
use ahash::AHashMap;
use std::sync::LazyLock;

/// Operator characters shared by Normal and Operator-pending modes.
///
/// These keys initiate (Normal) or double (Operator-pending: dd, yy, etc.)
/// an operator command.
const CORE_OPERATOR_CHARS: &[char] = &['d', 'c', 'y', '>', '<', '!', '='];

/// Find-character motion keys shared across all modes (f/F/t/T).
const FIND_CHAR_MOTION_CHARS: &[char] = &['f', 'F', 't', 'T'];

/// Motion characters shared by Normal, Visual, and Operator-pending modes.
///
/// These are the simple-char keys classified as `KeyClass::Motion` in all
/// three motion-supporting modes.  Mode-specific extras (e.g. `' '` in
/// Normal, `'0'` inline in Visual) are added inside each builder.
const SHARED_MOTION_CHARS: &[char] = &[
    'h', 'j', 'k', 'l', // cursor motions
    'w', 'W', 'b', 'B', 'e', 'E', // word motions
    '$', '^', '|', // line position motions
    '+', '-', '_', // line motions with first non-blank
    'G', 'H', 'M', 'L', // screen/file motions
    '%', // matching pair
    ';', ',', // repeat char motion
    'n', 'N', // search repeat
    '*', '#', // word search
    '(', ')', '{', '}', // sentence/paragraph
];

/// Core Vim keybindings (compile-time defaults).
///
/// Uses `ModeMap` for bounds-safe per-mode indexing. The Insert slot
/// is an empty map — `Keymap::classify_core()` returns `KeyClass::Unknown`
/// for Insert/Replace/CommandLine before ever reaching `CoreKeymap`.
pub struct CoreKeymap {
    tables: ModeMap<AHashMap<KeyEvent, KeyClass>>,
}

impl CoreKeymap {
    /// Create the core keymap with all default bindings.
    fn new() -> Self {
        let mut tables = ModeMap::default();
        tables[MappingMode::Normal] = Self::build_normal_map();
        tables[MappingMode::Operator] = Self::build_operator_pending_map();
        tables[MappingMode::Visual] = Self::build_visual_map();
        // Insert slot: empty map (never queried — classify_core()
        // returns Unknown for Insert before reaching CoreKeymap)
        Self { tables }
    }

    /// Iterate all (key, class) entries in the core keymap for a mode.
    pub fn entries(&self, mm: MappingMode) -> impl Iterator<Item = (&KeyEvent, &KeyClass)> {
        self.tables[mm].iter()
    }

    /// Classify a key event in the given mode.
    #[must_use]
    pub fn classify(&self, event: KeyEvent, mm: MappingMode) -> KeyClass {
        self.tables[mm]
            .get(&event)
            .copied()
            .unwrap_or(KeyClass::Unknown)
    }

    /// Insert motion keys shared across Normal, Visual, and Operator-pending
    /// modes into `map`.
    ///
    /// This is the single source of truth for motion entries that belong in
    /// all three motion-supporting modes.  Mode-specific extras (e.g. `' '`
    /// in Normal, `'0'` dual-role handling, `'r'` CharMotion) are added by
    /// each builder after calling this helper.
    fn add_shared_motion_keys(map: &mut AHashMap<KeyEvent, KeyClass>) {
        // --- Simple-char motions (SHARED_MOTION_CHARS) ---
        for c in SHARED_MOTION_CHARS {
            map.insert(KeyEvent::char(*c), KeyClass::Motion);
        }

        // --- Named keys ---
        map.insert(Key::Enter.into(), KeyClass::Motion);
        map.insert(Key::Backspace.into(), KeyClass::Motion);

        // --- Arrow / navigation keys ---
        map.insert(Key::Up.into(), KeyClass::Motion);
        map.insert(Key::Down.into(), KeyClass::Motion);
        map.insert(Key::Left.into(), KeyClass::Motion);
        map.insert(Key::Right.into(), KeyClass::Motion);
        map.insert(Key::Home.into(), KeyClass::Motion);
        map.insert(Key::End.into(), KeyClass::Motion);
        map.insert(Key::PageUp.into(), KeyClass::Motion);
        map.insert(Key::PageDown.into(), KeyClass::Motion);

        // --- Ctrl+Arrow ---
        map.insert(KeyEvent::new(Key::Left, Modifiers::CTRL), KeyClass::Motion);
        map.insert(KeyEvent::new(Key::Right, Modifiers::CTRL), KeyClass::Motion);

        // --- Shift+Arrow ---
        map.insert(KeyEvent::new(Key::Left, Modifiers::SHIFT), KeyClass::Motion);
        map.insert(
            KeyEvent::new(Key::Right, Modifiers::SHIFT),
            KeyClass::Motion,
        );
        map.insert(KeyEvent::new(Key::Up, Modifiers::SHIFT), KeyClass::Motion);
        map.insert(KeyEvent::new(Key::Down, Modifiers::SHIFT), KeyClass::Motion);

        // --- Ctrl scroll motions ---
        for c in ['f', 'b', 'd', 'u', 'e', 'y'] {
            map.insert(
                KeyEvent::new(Key::Char(c), Modifiers::CTRL),
                KeyClass::Motion,
            );
        }

        // --- Ctrl motion aliases ---
        map.insert(KeyEvent::ctrl('h'), KeyClass::Motion); // same as Backspace
        map.insert(KeyEvent::ctrl('j'), KeyClass::Motion); // same as j / Enter
        map.insert(KeyEvent::ctrl('m'), KeyClass::Motion); // same as Enter
        map.insert(KeyEvent::ctrl('n'), KeyClass::Motion); // same as j
        map.insert(KeyEvent::ctrl('p'), KeyClass::Motion); // same as k

        // --- Find-character motions (f/F/t/T) ---
        for c in FIND_CHAR_MOTION_CHARS {
            map.insert(KeyEvent::char(*c), KeyClass::CharMotion);
        }
    }

    /// Build Normal mode key classifications.
    fn build_normal_map() -> AHashMap<KeyEvent, KeyClass> {
        let mut map = AHashMap::new();

        // === Digits ===
        for c in '1'..='9' {
            map.insert(KeyEvent::char(c), KeyClass::Digit);
        }
        // NOTE: `0` has dual-role behavior — Motion (LineStart) when no count
        // is being built, Digit (appending 0) when a count already exists.
        // Classified as Motion here; grammar handlers intercept the digit case:
        //   - ready.rs::handle_ready_motion() — `0` after count → digit
        //   - operator.rs::KeyClass::Motion arm — `0` after count2 → digit
        map.insert(KeyEvent::char('0'), KeyClass::Motion);

        // === Operators ===
        for c in CORE_OPERATOR_CHARS {
            map.insert(KeyEvent::char(*c), KeyClass::Operator);
        }
        // gU, gu, g~ are handled as prefix + key

        // === Shared motions (arrow keys, Ctrl scrolls, f/F/t/T, etc.) ===
        Self::add_shared_motion_keys(&mut map);

        // Space acts as 'l' in Normal mode only
        map.insert(KeyEvent::char(' '), KeyClass::Motion);

        // r (replace char) is also a char-awaiting command
        map.insert(KeyEvent::char('r'), KeyClass::CharMotion);

        // === Mode switches ===
        // s and S are substitute commands that enter insert mode
        for c in ['i', 'I', 'a', 'A', 'o', 'O', 'R', 's', 'S'] {
            map.insert(KeyEvent::char(c), KeyClass::ModeSwitch);
        }
        map.insert(KeyEvent::char('v'), KeyClass::ModeSwitch);
        map.insert(KeyEvent::char('V'), KeyClass::ModeSwitch);
        map.insert(
            KeyEvent::new(Key::Char('v'), Modifiers::CTRL),
            KeyClass::ModeSwitch,
        );
        map.insert(KeyEvent::char(':'), KeyClass::ModeSwitch);

        // === Actions ===
        for c in [
            'x', 'X', 'p', 'P', 'u', 'U', '.', 'J', '~', 'D', 'C', 'Y', '&',
        ] {
            map.insert(KeyEvent::char(c), KeyClass::Action);
        }
        // Ctrl-R for redo
        map.insert(
            KeyEvent::new(Key::Char('r'), Modifiers::CTRL),
            KeyClass::Action,
        );
        // Ctrl-O for jump older
        map.insert(
            KeyEvent::new(Key::Char('o'), Modifiers::CTRL),
            KeyClass::Action,
        );
        // Ctrl-I for jump newer
        map.insert(
            KeyEvent::new(Key::Char('i'), Modifiers::CTRL),
            KeyClass::Action,
        );
        // Ctrl-A for increment number
        map.insert(
            KeyEvent::new(Key::Char('a'), Modifiers::CTRL),
            KeyClass::Action,
        );
        // Ctrl-X for decrement number
        map.insert(
            KeyEvent::new(Key::Char('x'), Modifiers::CTRL),
            KeyClass::Action,
        );

        // === Window prefix ===
        // Ctrl-W enters window command mode (awaits sub-command key)
        map.insert(
            KeyEvent::new(Key::Char('w'), Modifiers::CTRL),
            KeyClass::Action,
        );

        // === Prefixes ===
        for c in ['g', 'z', '[', ']', 'Z'] {
            map.insert(KeyEvent::char(c), KeyClass::Prefix);
        }
        // K for keyword lookup
        map.insert(KeyEvent::char('K'), KeyClass::Action);
        // Ctrl-G for file info
        map.insert(
            KeyEvent::new(Key::Char('g'), Modifiers::CTRL),
            KeyClass::Action,
        );
        // Ctrl-^ (Ctrl-6) for alternate file
        map.insert(
            KeyEvent::new(Key::Char('^'), Modifiers::CTRL),
            KeyClass::Action,
        );
        map.insert(
            KeyEvent::new(Key::Char('6'), Modifiers::CTRL),
            KeyClass::Action,
        );

        // === Mark triggers ===
        for c in ['m', '\'', '`'] {
            map.insert(KeyEvent::char(c), KeyClass::MarkTrigger);
        }

        // === Register trigger ===
        map.insert(KeyEvent::char('"'), KeyClass::RegisterTrigger);

        // === Search trigger ===
        map.insert(KeyEvent::char('/'), KeyClass::SearchTrigger);
        map.insert(KeyEvent::char('?'), KeyClass::SearchTrigger);

        // === Macro triggers ===
        map.insert(KeyEvent::char('q'), KeyClass::MacroTrigger);
        map.insert(KeyEvent::char('@'), KeyClass::MacroTrigger);

        // === Named special keys ===
        map.insert(Key::Delete.into(), KeyClass::Action);
        map.insert(Key::Tab.into(), KeyClass::Action);
        map.insert(Key::Insert.into(), KeyClass::ModeSwitch);

        // === Escape ===
        map.insert(KeyEvent::escape(), KeyClass::Escape);
        map.insert(KeyEvent::ctrl('c'), KeyClass::Escape);

        map
    }

    /// Build Operator-pending mode key classifications.
    fn build_operator_pending_map() -> AHashMap<KeyEvent, KeyClass> {
        let mut map = AHashMap::new();

        // === Digits ===
        for c in '0'..='9' {
            map.insert(KeyEvent::char(c), KeyClass::Digit);
        }

        // === Operators (for dd, yy, cc, >>, <<) ===
        for c in CORE_OPERATOR_CHARS {
            map.insert(KeyEvent::char(*c), KeyClass::Operator);
        }

        // === Shared motions (arrow keys, Ctrl scrolls, f/F/t/T, etc.) ===
        Self::add_shared_motion_keys(&mut map);

        // 0 is motion in OP mode (not digit) — overrides Digit from above
        map.insert(KeyEvent::char('0'), KeyClass::Motion);

        // === Text object triggers ===
        map.insert(KeyEvent::char('i'), KeyClass::TextObjectTrigger);
        map.insert(KeyEvent::char('a'), KeyClass::TextObjectTrigger);

        // === Prefixes ===
        for c in ['g', '[', ']'] {
            map.insert(KeyEvent::char(c), KeyClass::Prefix);
        }

        // === Mark triggers (' and ` for mark motions) ===
        for c in ['\'', '`'] {
            map.insert(KeyEvent::char(c), KeyClass::MarkTrigger);
        }

        // === Register trigger (for d"aw, c"bw, y"z$ etc.) ===
        map.insert(KeyEvent::char('"'), KeyClass::RegisterTrigger);

        // === Search trigger (for d/, d?, c/, y/ etc.) ===
        map.insert(KeyEvent::char('/'), KeyClass::SearchTrigger);
        map.insert(KeyEvent::char('?'), KeyClass::SearchTrigger);

        // === Mode switches (forced-motion + vim-surround) ===
        // v/V/Ctrl-V: forced-motion override (:help forced-motion)
        // s: vim-surround operator target (ys, ds, cs)
        map.insert(KeyEvent::char('v'), KeyClass::ModeSwitch);
        map.insert(KeyEvent::char('V'), KeyClass::ModeSwitch);
        map.insert(
            KeyEvent::new(Key::Char('v'), Modifiers::CTRL),
            KeyClass::ModeSwitch,
        );
        map.insert(KeyEvent::char('s'), KeyClass::ModeSwitch);

        // === Escape ===
        map.insert(KeyEvent::escape(), KeyClass::Escape);
        map.insert(KeyEvent::ctrl('c'), KeyClass::Escape);

        map
    }

    /// Build Visual mode key classifications.
    fn build_visual_map() -> AHashMap<KeyEvent, KeyClass> {
        let mut map = AHashMap::new();

        // === Digits (for counts like V2j, v3w) ===
        for c in '1'..='9' {
            map.insert(KeyEvent::char(c), KeyClass::Digit);
        }

        // === Shared motions (arrow keys, Ctrl scrolls, f/F/t/T, etc.) ===
        Self::add_shared_motion_keys(&mut map);

        // 0 is always a motion in Visual mode (no count prefix ambiguity)
        map.insert(KeyEvent::char('0'), KeyClass::Motion);

        // Space — motion (extend selection); mode-specific
        map.insert(KeyEvent::char(' '), KeyClass::Motion);

        // r (replace selection) is also a char-awaiting command in Visual
        map.insert(KeyEvent::char('r'), KeyClass::CharMotion);

        // === Text object triggers ===
        map.insert(KeyEvent::char('i'), KeyClass::TextObjectTrigger);
        map.insert(KeyEvent::char('a'), KeyClass::TextObjectTrigger);

        // === Operators (apply to selection) ===
        for c in [
            'd', 'c', 'y', '>', '<', 'x', 'X', 's', '!', '=', 'D', 'C', 'Y',
        ] {
            map.insert(KeyEvent::char(c), KeyClass::Operator);
        }

        // === Actions ===
        for c in ['J', 'u', 'U', '~', 'p', 'P', 'I', 'A', 'O', 'o'] {
            map.insert(KeyEvent::char(c), KeyClass::Action);
        }

        // Ctrl-G — toggle between Visual and Select mode
        map.insert(KeyEvent::ctrl('g'), KeyClass::Action);

        // S — substitute lines (standard Vim) / surround selection (vim-surround)
        map.insert(KeyEvent::char('S'), KeyClass::Operator);

        // === Mode switches ===
        map.insert(KeyEvent::char('v'), KeyClass::ModeSwitch);
        map.insert(KeyEvent::char('V'), KeyClass::ModeSwitch);
        map.insert(
            KeyEvent::new(Key::Char('v'), Modifiers::CTRL),
            KeyClass::ModeSwitch,
        );

        // === Prefixes (for gg, ge, g$, etc.) ===
        for c in ['g', 'z', '[', ']'] {
            map.insert(KeyEvent::char(c), KeyClass::Prefix);
        }

        // === Register trigger ===
        map.insert(KeyEvent::char('"'), KeyClass::RegisterTrigger);

        // === Macro triggers ===
        map.insert(KeyEvent::char('q'), KeyClass::MacroTrigger);
        map.insert(KeyEvent::char('@'), KeyClass::MacroTrigger);

        // === Mark triggers ===
        for c in ['m', '\'', '`'] {
            map.insert(KeyEvent::char(c), KeyClass::MarkTrigger);
        }

        // === Ctrl actions ===
        // Ctrl-A (increment number), Ctrl-X (decrement number)
        map.insert(
            KeyEvent::new(Key::Char('a'), Modifiers::CTRL),
            KeyClass::Action,
        );
        map.insert(
            KeyEvent::new(Key::Char('x'), Modifiers::CTRL),
            KeyClass::Action,
        );

        // === Named special keys ===
        map.insert(Key::Delete.into(), KeyClass::Action);

        // === Escape ===
        map.insert(KeyEvent::escape(), KeyClass::Escape);
        map.insert(KeyEvent::ctrl('c'), KeyClass::Escape);

        // === CommandLine triggers (: for ex-commands, / and ? for search) ===
        map.insert(KeyEvent::char(':'), KeyClass::ModeSwitch);
        // === Search triggers ===
        map.insert(KeyEvent::char('/'), KeyClass::SearchTrigger);
        map.insert(KeyEvent::char('?'), KeyClass::SearchTrigger);

        map
    }
}

impl std::ops::Index<MappingMode> for CoreKeymap {
    type Output = AHashMap<KeyEvent, KeyClass>;

    fn index(&self, mm: MappingMode) -> &AHashMap<KeyEvent, KeyClass> {
        &self.tables[mm]
    }
}

/// Global core keymap instance.
pub static CORE_KEYMAP: LazyLock<CoreKeymap> = LazyLock::new(CoreKeymap::new);

#[cfg(test)]
mod tests {
    use super::super::keymap::MappingMode;
    use super::*;

    // Shorthand for readability in tests
    const N: MappingMode = MappingMode::Normal;
    const V: MappingMode = MappingMode::Visual;
    const O: MappingMode = MappingMode::Operator;

    #[test]
    fn normal_operators() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('d'), N),
            KeyClass::Operator
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('c'), N),
            KeyClass::Operator
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('y'), N),
            KeyClass::Operator
        );
    }

    #[test]
    fn normal_motions() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('h'), N),
            KeyClass::Motion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('j'), N),
            KeyClass::Motion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('k'), N),
            KeyClass::Motion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('l'), N),
            KeyClass::Motion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('w'), N),
            KeyClass::Motion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('b'), N),
            KeyClass::Motion
        );
    }

    #[test]
    fn normal_char_motions() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('f'), N),
            KeyClass::CharMotion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('F'), N),
            KeyClass::CharMotion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('t'), N),
            KeyClass::CharMotion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('T'), N),
            KeyClass::CharMotion
        );
    }

    #[test]
    fn normal_mode_switches() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('i'), N),
            KeyClass::ModeSwitch
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('a'), N),
            KeyClass::ModeSwitch
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('v'), N),
            KeyClass::ModeSwitch
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('V'), N),
            KeyClass::ModeSwitch
        );
    }

    #[test]
    fn normal_actions() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('x'), N),
            KeyClass::Action
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('p'), N),
            KeyClass::Action
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('u'), N),
            KeyClass::Action
        );
    }

    #[test]
    fn normal_unknown_fallback() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('φ'), N),
            KeyClass::Unknown
        );
    }

    #[test]
    fn visual_motions_at_least_hjkl() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('h'), V),
            KeyClass::Motion
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('j'), V),
            KeyClass::Motion
        );
    }

    #[test]
    fn visual_operators() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('d'), V),
            KeyClass::Operator
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('y'), V),
            KeyClass::Operator
        );
    }

    #[test]
    fn operator_pending_text_objects() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('i'), O),
            KeyClass::TextObjectTrigger,
        );
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('a'), O),
            KeyClass::TextObjectTrigger,
        );
    }

    #[test]
    fn operator_pending_motions() {
        assert_eq!(
            CORE_KEYMAP.classify(KeyEvent::char('w'), O),
            KeyClass::Motion,
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // Core ↔ Grammar consistency guards
    //
    // These tests ensure the keymap classification tables and the
    // grammar's command-resolution tables stay in sync. If a key is
    // classified as Motion in core.rs, the grammar MUST be able to
    // resolve it to a Motion variant (and vice versa).
    // ═══════════════════════════════════════════════════════════════════

    /// Every simple-char key classified as Motion in core.rs must be
    /// resolvable by `Motion::from_char`.
    ///
    /// Exceptions: '0' (dual-role: motion at line start, digit otherwise),
    /// '[', ']' (prefix-based, handled by grammar state machine).
    #[test]
    fn consistency_motion_keys_resolve_in_grammar() {
        use crate::grammar::types::Motion;

        let exceptions = ['0'];
        let motion_chars: Vec<char> = CORE_KEYMAP[N]
            .iter()
            .filter(|(_, class)| **class == KeyClass::Motion)
            .filter_map(|(event, _)| event.as_char())
            .filter(|c| !exceptions.contains(c))
            .collect();

        for c in &motion_chars {
            assert!(
                Motion::from_char(*c).is_some(),
                "core.rs classifies '{c}' as Motion but Motion::from_char('{c}') returns None — \
                 add it to grammar/types/motion.rs",
            );
        }
    }

    /// Every Ctrl-modified key classified as Motion in core.rs must be
    /// resolvable by `Motion::from_key_event`.
    #[test]
    fn consistency_ctrl_motion_keys_resolve_in_grammar() {
        use crate::grammar::types::Motion;

        let ctrl_motion_keys: Vec<KeyEvent> = CORE_KEYMAP[N]
            .iter()
            .filter(|(event, class)| **class == KeyClass::Motion && event.has_modifiers())
            .map(|(event, _)| *event)
            .collect();

        for key in &ctrl_motion_keys {
            assert!(
                Motion::from_key_event(key).is_some(),
                "core.rs classifies {} as Motion but Motion::from_key_event returns None — \
                 add it to grammar/types/motion.rs",
                key.to_vim_notation(),
            );
        }
    }

    /// Reverse check: Every Ctrl+letter that `Motion::from_key_event` maps
    /// to a motion must be classified as Motion in core.rs's keymap.
    ///
    /// The forward test (`consistency_ctrl_motion_keys_resolve_in_grammar`)
    /// checks keymap → grammar. This test checks grammar → keymap.
    /// Together they guarantee the two systems stay in sync.
    #[test]
    fn consistency_grammar_ctrl_motions_classified_in_core() {
        use crate::grammar::types::Motion;

        // All Ctrl+letter combinations that from_key_event handles.
        for c in 'a'..='z' {
            let key = KeyEvent::ctrl(c);
            if Motion::from_key_event(&key).is_some() {
                let class = CORE_KEYMAP.classify(key, N);
                assert_eq!(
                    class,
                    KeyClass::Motion,
                    "Motion::from_key_event(Ctrl+{c}) returns Some but core.rs classifies \
                     Ctrl+{c} as {class:?} in Normal — add it to add_shared_motion_keys",
                );
            }
        }
    }

    /// Every simple-char key classified as Operator in core.rs must be
    /// resolvable by `operator_from_key`.
    #[test]
    fn consistency_operator_keys_resolve_in_grammar() {
        use crate::grammar::handlers::helpers::operator_from_key;

        let op_chars: Vec<char> = CORE_KEYMAP[N]
            .iter()
            .filter(|(_, class)| **class == KeyClass::Operator)
            .filter_map(|(event, _)| event.as_char())
            .collect();

        for c in &op_chars {
            let result = operator_from_key(KeyEvent::char(*c));
            assert!(
                result.is_some(),
                "core.rs classifies '{c}' as Operator but operator_from_key('{c}') returns None \
                 — add it to grammar/handlers/helpers.rs",
            );
        }
    }

    /// Every simple-char key classified as Action in core.rs must be
    /// resolvable by `Action::from_char` (or from_ctrl_char for Ctrl keys).
    ///
    /// Exception: '.' (repeat) is handled specially by the grammar state
    /// machine, not through Action::from_char.
    #[test]
    fn consistency_action_keys_resolve_in_grammar() {
        use crate::grammar::types::Action;

        let exceptions = ['.'];
        let action_chars: Vec<char> = CORE_KEYMAP[N]
            .iter()
            .filter(|(_, class)| **class == KeyClass::Action)
            .filter_map(|(event, _)| event.as_char())
            .filter(|c| !exceptions.contains(c))
            .collect();

        for c in &action_chars {
            assert!(
                Action::from_char(*c).is_some(),
                "core.rs classifies '{c}' as Action but Action::from_char('{c}') returns None — \
                 add it to grammar/types/action.rs",
            );
        }
    }

    /// Every Ctrl-modified key classified as Action in core.rs must be
    /// resolvable by `Action::from_ctrl_char`.
    ///
    /// Exception: Ctrl-W (window prefix) is classified as Action but handled
    /// as a special-case prefix in handle_ready_action, not via Action enum.
    #[test]
    fn consistency_ctrl_action_keys_resolve_in_grammar() {
        use crate::grammar::types::Action;

        let exceptions = ['w']; // Ctrl-W: window prefix, not a standard Action

        let ctrl_action_keys: Vec<(char, KeyEvent)> = CORE_KEYMAP[N]
            .iter()
            .filter(|(event, class)| **class == KeyClass::Action && event.has_modifiers())
            .filter_map(|(event, _)| event.key.as_char().map(|c| (c, *event)))
            .filter(|(c, _)| !exceptions.contains(c))
            .collect();

        for (c, key) in &ctrl_action_keys {
            assert!(
                Action::from_ctrl_char(*c).is_some(),
                "core.rs classifies {} as Action but Action::from_ctrl_char('{c}') returns None — \
                 add it to grammar/types/action.rs",
                key.to_vim_notation(),
            );
        }
    }

    /// Reverse check: Every char in `Motion::from_char` must be classified
    /// as Motion (or CharMotion) in core.rs.
    #[test]
    fn consistency_grammar_motions_classified_in_core() {
        use crate::grammar::types::Motion;

        // All chars that from_char handles
        let motion_chars = [
            'h', 'l', 'k', 'j', ' ', '+', '-', '_', 'w', 'b', 'e', 'W', 'B', 'E', '0', '$', '^',
            'G', 'H', 'M', 'L', '%', ';', ',', 'n', 'N', '*', '#', ')', '(', '}', '{', '|',
        ];
        for c in motion_chars {
            if Motion::from_char(c).is_some() {
                let class = CORE_KEYMAP.classify(KeyEvent::char(c), N);
                assert!(
                    class == KeyClass::Motion || class == KeyClass::CharMotion,
                    "Motion::from_char('{c}') returns Some but core.rs classifies '{c}' as {class:?} \
                     — add it to core.rs build_normal_map",
                );
            }
        }
    }

    #[test]
    fn arrow_keys_classified_as_motion_in_all_modes() {
        let km = &*CORE_KEYMAP;
        for key in [Key::Up, Key::Down, Key::Left, Key::Right] {
            let event: KeyEvent = key.into();
            assert_eq!(
                km.classify(event, MappingMode::Normal),
                KeyClass::Motion,
                "{key:?} should be Motion in Normal"
            );
            assert_eq!(
                km.classify(event, MappingMode::Operator),
                KeyClass::Motion,
                "{key:?} should be Motion in Operator"
            );
            assert_eq!(
                km.classify(event, MappingMode::Visual),
                KeyClass::Motion,
                "{key:?} should be Motion in Visual"
            );
        }
    }

    #[test]
    fn home_end_classified_as_motion_in_all_modes() {
        let km = &*CORE_KEYMAP;
        for key in [Key::Home, Key::End] {
            let event: KeyEvent = key.into();
            assert_eq!(
                km.classify(event, MappingMode::Normal),
                KeyClass::Motion,
                "{key:?} should be Motion in Normal"
            );
            assert_eq!(
                km.classify(event, MappingMode::Operator),
                KeyClass::Motion,
                "{key:?} should be Motion in Operator"
            );
            assert_eq!(
                km.classify(event, MappingMode::Visual),
                KeyClass::Motion,
                "{key:?} should be Motion in Visual"
            );
        }
    }

    #[test]
    fn ctrl_arrow_classified_as_motion_in_all_modes() {
        let km = &*CORE_KEYMAP;
        for key in [Key::Left, Key::Right] {
            let event = KeyEvent::new(key, Modifiers::CTRL);
            assert_eq!(
                km.classify(event, MappingMode::Normal),
                KeyClass::Motion,
                "Ctrl+{key:?} should be Motion in Normal"
            );
            assert_eq!(
                km.classify(event, MappingMode::Operator),
                KeyClass::Motion,
                "Ctrl+{key:?} should be Motion in Operator"
            );
            assert_eq!(
                km.classify(event, MappingMode::Visual),
                KeyClass::Motion,
                "Ctrl+{key:?} should be Motion in Visual"
            );
        }
    }

    #[test]
    fn op_mode_switch_keys_classified() {
        let km = &*CORE_KEYMAP;
        assert_eq!(
            km.classify(KeyEvent::char('v'), MappingMode::Operator),
            KeyClass::ModeSwitch,
            "v should be ModeSwitch in OP for forced-motion"
        );
        assert_eq!(
            km.classify(KeyEvent::char('V'), MappingMode::Operator),
            KeyClass::ModeSwitch,
            "V should be ModeSwitch in OP for forced-motion"
        );
        assert_eq!(
            km.classify(
                KeyEvent::new(Key::Char('v'), Modifiers::CTRL),
                MappingMode::Operator
            ),
            KeyClass::ModeSwitch,
            "Ctrl-V should be ModeSwitch in OP for forced-motion"
        );
        assert_eq!(
            km.classify(KeyEvent::char('s'), MappingMode::Operator),
            KeyClass::ModeSwitch,
            "s should be ModeSwitch in OP for vim-surround"
        );
    }

    #[test]
    fn op_enter_backspace_classified_as_motion() {
        let km = &*CORE_KEYMAP;
        assert_eq!(
            km.classify(Key::Enter.into(), MappingMode::Operator),
            KeyClass::Motion,
            "Enter should be Motion in OP"
        );
        assert_eq!(
            km.classify(Key::Backspace.into(), MappingMode::Operator),
            KeyClass::Motion,
            "Backspace should be Motion in OP"
        );
    }

    #[test]
    fn visual_enter_backspace_space_classified_as_motion() {
        let km = &*CORE_KEYMAP;
        assert_eq!(
            km.classify(Key::Enter.into(), MappingMode::Visual),
            KeyClass::Motion,
            "Enter should be Motion in Visual"
        );
        assert_eq!(
            km.classify(Key::Backspace.into(), MappingMode::Visual),
            KeyClass::Motion,
            "Backspace should be Motion in Visual"
        );
        assert_eq!(
            km.classify(KeyEvent::char(' '), MappingMode::Visual),
            KeyClass::Motion,
            "Space should be Motion in Visual"
        );
    }

    #[test]
    fn pageup_pagedown_classified_as_motion_in_all_modes() {
        let km = &*CORE_KEYMAP;
        for key in [Key::PageUp, Key::PageDown] {
            let event: KeyEvent = key.into();
            assert_eq!(
                km.classify(event, MappingMode::Normal),
                KeyClass::Motion,
                "{key:?} should be Motion in Normal"
            );
            assert_eq!(
                km.classify(event, MappingMode::Operator),
                KeyClass::Motion,
                "{key:?} should be Motion in Operator"
            );
            assert_eq!(
                km.classify(event, MappingMode::Visual),
                KeyClass::Motion,
                "{key:?} should be Motion in Visual"
            );
        }
    }

    #[test]
    fn visual_macro_triggers_classified() {
        let km = &*CORE_KEYMAP;
        assert_eq!(
            km.classify(KeyEvent::char('q'), MappingMode::Visual),
            KeyClass::MacroTrigger,
            "q should be MacroTrigger in Visual"
        );
        assert_eq!(
            km.classify(KeyEvent::char('@'), MappingMode::Visual),
            KeyClass::MacroTrigger,
            "@ should be MacroTrigger in Visual"
        );
    }

    #[test]
    fn delete_key_classified_as_action_in_normal_and_visual() {
        let km = &*CORE_KEYMAP;
        let event: KeyEvent = Key::Delete.into();
        assert_eq!(
            km.classify(event, MappingMode::Normal),
            KeyClass::Action,
            "Delete should be Action in Normal"
        );
        assert_eq!(
            km.classify(event, MappingMode::Visual),
            KeyClass::Action,
            "Delete should be Action in Visual"
        );
    }

    #[test]
    fn tab_classified_as_action_in_normal() {
        let km = &*CORE_KEYMAP;
        assert_eq!(
            km.classify(Key::Tab.into(), MappingMode::Normal),
            KeyClass::Action,
            "Tab should be Action in Normal"
        );
    }

    #[test]
    fn insert_key_classified_as_mode_switch_in_normal() {
        let km = &*CORE_KEYMAP;
        assert_eq!(
            km.classify(Key::Insert.into(), MappingMode::Normal),
            KeyClass::ModeSwitch,
            "Insert should be ModeSwitch in Normal"
        );
    }

    #[test]
    fn shift_arrow_classified_as_motion_in_all_modes() {
        let km = &*CORE_KEYMAP;
        for key in [Key::Left, Key::Right, Key::Up, Key::Down] {
            let event = KeyEvent::new(key, Modifiers::SHIFT);
            assert_eq!(
                km.classify(event, MappingMode::Normal),
                KeyClass::Motion,
                "Shift+{key:?} should be Motion in Normal"
            );
            assert_eq!(
                km.classify(event, MappingMode::Operator),
                KeyClass::Motion,
                "Shift+{key:?} should be Motion in Operator"
            );
            assert_eq!(
                km.classify(event, MappingMode::Visual),
                KeyClass::Motion,
                "Shift+{key:?} should be Motion in Visual"
            );
        }
    }

    #[test]
    fn op_scroll_motions_classified() {
        for c in ['f', 'b', 'd', 'u', 'e', 'y'] {
            let key = KeyEvent::new(Key::Char(c), Modifiers::CTRL);
            assert_eq!(
                CORE_KEYMAP.classify(key, O),
                KeyClass::Motion,
                "Ctrl+{} should be Motion in OP mode",
                c
            );
        }
    }

    #[test]
    fn shared_motions_present_in_all_three_modes() {
        let shared_motion_keys = [
            KeyEvent::new(Key::Up, Modifiers::NONE),
            KeyEvent::new(Key::Down, Modifiers::NONE),
            KeyEvent::new(Key::Left, Modifiers::NONE),
            KeyEvent::new(Key::Right, Modifiers::NONE),
            KeyEvent::new(Key::Home, Modifiers::NONE),
            KeyEvent::new(Key::End, Modifiers::NONE),
            KeyEvent::new(Key::PageUp, Modifiers::NONE),
            KeyEvent::new(Key::PageDown, Modifiers::NONE),
            KeyEvent::new(Key::Left, Modifiers::CTRL),
            KeyEvent::new(Key::Right, Modifiers::CTRL),
            KeyEvent::new(Key::Left, Modifiers::SHIFT),
            KeyEvent::new(Key::Right, Modifiers::SHIFT),
            KeyEvent::new(Key::Up, Modifiers::SHIFT),
            KeyEvent::new(Key::Down, Modifiers::SHIFT),
            KeyEvent::ctrl('f'),
            KeyEvent::ctrl('b'),
            KeyEvent::ctrl('d'),
            KeyEvent::ctrl('u'),
            KeyEvent::ctrl('e'),
            KeyEvent::ctrl('y'),
            KeyEvent::ctrl('m'),
            KeyEvent::ctrl('n'),
            KeyEvent::ctrl('p'),
            KeyEvent::new(Key::Enter, Modifiers::NONE),
            KeyEvent::new(Key::Backspace, Modifiers::NONE),
        ];
        let shared_char_motion_keys = [
            KeyEvent::char('f'),
            KeyEvent::char('F'),
            KeyEvent::char('t'),
            KeyEvent::char('T'),
        ];

        for key in &shared_motion_keys {
            assert_eq!(
                CORE_KEYMAP.classify(*key, N),
                KeyClass::Motion,
                "Normal: {:?} should be Motion",
                key
            );
            assert_eq!(
                CORE_KEYMAP.classify(*key, V),
                KeyClass::Motion,
                "Visual: {:?} should be Motion",
                key
            );
            assert_eq!(
                CORE_KEYMAP.classify(*key, O),
                KeyClass::Motion,
                "OP: {:?} should be Motion",
                key
            );
        }
        for key in &shared_char_motion_keys {
            assert_eq!(
                CORE_KEYMAP.classify(*key, N),
                KeyClass::CharMotion,
                "Normal: {:?} should be CharMotion",
                key
            );
            assert_eq!(
                CORE_KEYMAP.classify(*key, V),
                KeyClass::CharMotion,
                "Visual: {:?} should be CharMotion",
                key
            );
            assert_eq!(
                CORE_KEYMAP.classify(*key, O),
                KeyClass::CharMotion,
                "OP: {:?} should be CharMotion",
                key
            );
        }
    }

    /// Reverse check: Every char in `Action::from_char` must be classified
    /// as Action in core.rs.
    #[test]
    fn consistency_grammar_actions_classified_in_core() {
        use crate::grammar::types::Action;

        let action_chars = [
            'x', 'X', 'p', 'P', 'u', 'U', 'J', '~', 'D', 'C', 'Y', 's', 'I', 'A',
        ];
        for c in action_chars {
            if Action::from_char(c).is_some() {
                let class = CORE_KEYMAP.classify(KeyEvent::char(c), N);
                // s, I, A are classified as ModeSwitch (they enter insert mode),
                // not Action, which is correct — they're dual-purpose
                if matches!(c, 's' | 'I' | 'A') {
                    continue;
                }
                assert!(
                    class == KeyClass::Action,
                    "Action::from_char('{c}') returns Some but core.rs classifies '{c}' as {class:?} \
                     — check classification in core.rs",
                );
            }
        }
    }

    #[test]
    fn ctrl_h_j_classified_as_motion_in_all_modes() {
        let km = &*CORE_KEYMAP;
        for (c, label) in [('h', "Ctrl+H"), ('j', "Ctrl+J")] {
            let event = KeyEvent::ctrl(c);
            assert_eq!(
                km.classify(event, N),
                KeyClass::Motion,
                "{label} should be Motion in Normal"
            );
            assert_eq!(
                km.classify(event, O),
                KeyClass::Motion,
                "{label} should be Motion in Operator"
            );
            assert_eq!(
                km.classify(event, V),
                KeyClass::Motion,
                "{label} should be Motion in Visual"
            );
        }
    }
}
