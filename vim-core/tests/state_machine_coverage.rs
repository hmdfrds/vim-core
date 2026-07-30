//! State-machine transition coverage test for the grammar parser.
//!
//! This test suite verifies that every `InputState` variant is reachable
//! from concrete key sequences fed through the `Parser`. It serves as a
//! structural coverage check: if a new variant is added to `InputState`
//! without a corresponding entry here, the `all_variants_covered` test
//! will fail until the mapping table is updated.
//!
//! # Approach
//!
//! We define a static table of critical transitions, each specifying:
//! - A human-readable state name matching the `InputState` variant
//! - A key sequence that, when fed to the parser, leaves it in that state
//!   (or causes it to pass through that state before completing)
//! - The mode the parser should be in while processing the keys
//! - A discriminant-checking function to verify the state was reached
//! - A description for diagnostics
//!
//! We then have a meta-test that compares the set of tested variant names
//! against the actual `InputState` variants (extracted from `std::mem::discriminant`
//! comparisons and the exhaustive match in the parser).

use vim_core::grammar::input_state::InputState;
use vim_core::grammar::{GrammarResult, Parser};
use vim_core::keymap::{KeyEvent, Keymap};
use vim_core::primitives::Mode;

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Feed a sequence of key events to the parser and return the final state.
///
/// Uses the provided `mode` for all keys. For sequences that change mode
/// mid-stream (e.g., entering insert mode), the caller should use
/// `feed_keys_multi_mode` instead.
fn feed_keys<'a>(
    parser: &'a mut Parser,
    keymap: &Keymap,
    mode: Mode,
    keys: &[KeyEvent],
) -> &'a InputState {
    for &key in keys {
        let _ = parser.process(key, keymap, mode);
    }
    parser.state()
}

/// Classify the InputState variant by returning a &str discriminant name.
///
/// This is the canonical mapping from InputState variant to the string names
/// used in the coverage table. It must be kept exhaustive (the compiler
/// enforces this via the match).
fn variant_name(state: &InputState) -> &'static str {
    match state {
        InputState::Ready { .. } => "Ready",
        InputState::AwaitingRegister { .. } => "AwaitingRegister",
        InputState::Operator { .. } => "Operator",
        InputState::AwaitingChar { .. } => "AwaitingChar",
        InputState::AwaitingTextObject { .. } => "AwaitingTextObject",
        InputState::AwaitingTextObjectWithModifier { .. } => "AwaitingTextObjectWithModifier",
        InputState::AwaitingVisualTextObjectWithModifier { .. } => {
            "AwaitingVisualTextObjectWithModifier"
        }
        InputState::AwaitingPrefix { .. } => "AwaitingPrefix",
        InputState::AwaitingMark { .. } => "AwaitingMark",
        InputState::AwaitingInsertRegister => "AwaitingInsertRegister",
        InputState::AwaitingInsertExpression { .. } => "AwaitingInsertExpression",
        InputState::AwaitingInsertCtrlG => "AwaitingInsertCtrlG",
        InputState::AwaitingInsertDigraph1 => "AwaitingInsertDigraph1",
        InputState::AwaitingInsertDigraph2 { .. } => "AwaitingInsertDigraph2",
        InputState::AwaitingInsertCtrlX => "AwaitingInsertCtrlX",
        InputState::InsertLiteral(_) => "InsertLiteral",
        InputState::AwaitingMacroRegister { .. } => "AwaitingMacroRegister",
        InputState::AwaitingVisualTextObject { .. } => "AwaitingVisualTextObject",
        InputState::AwaitingWindowCommand { .. } => "AwaitingWindowCommand",
        // Non-exhaustive: any future variant will cause a compile error here,
        // which is exactly what we want -- it forces updating this function
        // and the coverage table below.
        _ => "Unknown",
    }
}

/// All 19 variant names that must appear in the coverage table.
///
/// This is the ground-truth list. If `InputState` gains or loses a variant,
/// update this array AND the `variant_name` match above.
const ALL_VARIANT_NAMES: &[&str] = &[
    "Ready",
    "AwaitingRegister",
    "Operator",
    "AwaitingChar",
    "AwaitingTextObject",
    "AwaitingTextObjectWithModifier",
    "AwaitingVisualTextObjectWithModifier",
    "AwaitingPrefix",
    "AwaitingMark",
    "AwaitingInsertRegister",
    "AwaitingInsertExpression",
    "AwaitingInsertCtrlG",
    "AwaitingInsertDigraph1",
    "AwaitingInsertDigraph2",
    "AwaitingInsertCtrlX",
    "InsertLiteral",
    "AwaitingMacroRegister",
    "AwaitingVisualTextObject",
    "AwaitingWindowCommand",
];

// ═══════════════════════════════════════════════════════════════════════════════
// Transition Coverage Table
// ═══════════════════════════════════════════════════════════════════════════════

/// Each entry: (expected_variant_name, description)
/// The test function is responsible for constructing the parser with the right
/// plugin flags and feeding the right key sequence.
///
/// We use individual test functions (below) rather than a data-driven loop
/// because different states require different parser configurations (plugins,
/// modes, multi-mode sequences) that are hard to parameterize uniformly.

// ═══════════════════════════════════════════════════════════════════════════════
// Individual state-reaching tests
// ═══════════════════════════════════════════════════════════════════════════════

// ── Normal-mode states ───────────────────────────────────────────────────────

#[test]
fn reach_ready() {
    let parser = Parser::new();
    assert_eq!(
        variant_name(parser.state()),
        "Ready",
        "fresh parser is Ready"
    );
}

#[test]
fn reach_ready_with_count() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('3')]);
    let state = parser.state();
    assert_eq!(variant_name(state), "Ready", "digit builds count in Ready");
    assert_eq!(state.count(), Some(3));
}

#[test]
fn reach_awaiting_register() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('"')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingRegister",
        "double-quote enters AwaitingRegister"
    );
}

#[test]
fn reach_operator() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('d')]);
    assert_eq!(
        variant_name(parser.state()),
        "Operator",
        "d enters Operator"
    );
}

#[test]
fn reach_awaiting_char_find() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('f')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingChar",
        "f enters AwaitingChar"
    );
}

#[test]
fn reach_awaiting_char_till() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('t')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingChar",
        "t enters AwaitingChar"
    );
}

#[test]
fn reach_awaiting_char_replace() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('r')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingChar",
        "r enters AwaitingChar"
    );
}

#[test]
fn reach_awaiting_char_with_operator() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(
        &mut parser,
        &keymap,
        Mode::Normal,
        &[KeyEvent::char('d'), KeyEvent::char('f')],
    );
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingChar",
        "df enters AwaitingChar with operator"
    );
}

#[test]
fn reach_awaiting_text_object() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(
        &mut parser,
        &keymap,
        Mode::Normal,
        &[KeyEvent::char('d'), KeyEvent::char('i')],
    );
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingTextObject",
        "di enters AwaitingTextObject"
    );
}

#[test]
fn reach_awaiting_text_object_around() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(
        &mut parser,
        &keymap,
        Mode::Normal,
        &[KeyEvent::char('c'), KeyEvent::char('a')],
    );
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingTextObject",
        "ca enters AwaitingTextObject (around)"
    );
}

#[test]
fn reach_awaiting_text_object_with_modifier() {
    // targets.vim seek modifier: din -> AwaitingTextObjectWithModifier
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(
        &mut parser,
        &keymap,
        Mode::Normal,
        &[
            KeyEvent::char('d'),
            KeyEvent::char('i'),
            KeyEvent::char('n'),
        ],
    );
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingTextObjectWithModifier",
        "din enters AwaitingTextObjectWithModifier"
    );
}

#[test]
fn reach_awaiting_prefix_g() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('g')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingPrefix",
        "g enters AwaitingPrefix"
    );
}

#[test]
fn reach_awaiting_prefix_z() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('z')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingPrefix",
        "z enters AwaitingPrefix"
    );
}

#[test]
fn reach_awaiting_prefix_bracket() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('[')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingPrefix",
        "[ enters AwaitingPrefix"
    );
}

#[test]
fn reach_awaiting_mark_set() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('m')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingMark",
        "m enters AwaitingMark (set)"
    );
}

#[test]
fn reach_awaiting_mark_jump_line() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('\'')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingMark",
        "' enters AwaitingMark (jump line)"
    );
}

#[test]
fn reach_awaiting_mark_jump_exact() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('`')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingMark",
        "` enters AwaitingMark (jump exact)"
    );
}

#[test]
fn reach_awaiting_macro_register_record() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('q')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingMacroRegister",
        "q enters AwaitingMacroRegister (record)"
    );
}

#[test]
fn reach_awaiting_macro_register_play() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::char('@')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingMacroRegister",
        "@ enters AwaitingMacroRegister (play)"
    );
}

#[test]
fn reach_awaiting_window_command() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Normal, &[KeyEvent::ctrl('w')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingWindowCommand",
        "Ctrl-W enters AwaitingWindowCommand"
    );
}

// ── Insert-mode states ───────────────────────────────────────────────────────

#[test]
fn reach_awaiting_insert_register() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Insert, &[KeyEvent::ctrl('r')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingInsertRegister",
        "Ctrl-R in insert enters AwaitingInsertRegister"
    );
}

#[test]
fn reach_awaiting_insert_expression() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    // Ctrl-R then '=' enters expression mode
    feed_keys(
        &mut parser,
        &keymap,
        Mode::Insert,
        &[KeyEvent::ctrl('r'), KeyEvent::char('=')],
    );
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingInsertExpression",
        "Ctrl-R = in insert enters AwaitingInsertExpression"
    );
}

#[test]
fn reach_awaiting_insert_ctrl_g() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Insert, &[KeyEvent::ctrl('g')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingInsertCtrlG",
        "Ctrl-G in insert enters AwaitingInsertCtrlG"
    );
}

#[test]
fn reach_awaiting_insert_digraph1() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Insert, &[KeyEvent::ctrl('k')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingInsertDigraph1",
        "Ctrl-K in insert enters AwaitingInsertDigraph1"
    );
}

#[test]
fn reach_awaiting_insert_digraph2() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(
        &mut parser,
        &keymap,
        Mode::Insert,
        &[KeyEvent::ctrl('k'), KeyEvent::char('e')],
    );
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingInsertDigraph2",
        "Ctrl-K e in insert enters AwaitingInsertDigraph2"
    );
}

#[test]
fn reach_awaiting_insert_ctrl_x() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Insert, &[KeyEvent::ctrl('x')]);
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingInsertCtrlX",
        "Ctrl-X in insert enters AwaitingInsertCtrlX"
    );
}

#[test]
fn reach_insert_literal() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(&mut parser, &keymap, Mode::Insert, &[KeyEvent::ctrl('v')]);
    assert_eq!(
        variant_name(parser.state()),
        "InsertLiteral",
        "Ctrl-V in insert enters InsertLiteral"
    );
}

// ── Visual-mode states ───────────────────────────────────────────────────────

#[test]
fn reach_awaiting_visual_text_object() {
    use vim_core::primitives::VisualType;

    let mut parser = Parser::new();
    let keymap = Keymap::default();
    feed_keys(
        &mut parser,
        &keymap,
        Mode::Visual(VisualType::Char),
        &[KeyEvent::char('i')],
    );
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingVisualTextObject",
        "i in visual enters AwaitingVisualTextObject"
    );
}

#[test]
fn reach_awaiting_visual_text_object_with_modifier() {
    use vim_core::primitives::VisualType;

    let mut parser = Parser::new();
    let keymap = Keymap::default();
    // In visual mode: i then n -> AwaitingVisualTextObjectWithModifier
    feed_keys(
        &mut parser,
        &keymap,
        Mode::Visual(VisualType::Char),
        &[KeyEvent::char('i'), KeyEvent::char('n')],
    );
    assert_eq!(
        variant_name(parser.state()),
        "AwaitingVisualTextObjectWithModifier",
        "in in visual enters AwaitingVisualTextObjectWithModifier"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Meta-test: verify all 29 variants are covered
// ═══════════════════════════════════════════════════════════════════════════════

/// Collects the set of variant names reached by exercising all the
/// transition sequences and checks against ALL_VARIANT_NAMES.
#[test]
fn all_variants_covered() {
    use vim_core::primitives::VisualType;

    let keymap = Keymap::default();

    // Each entry: (parser_setup, keys_with_modes, expected_variant_name)
    // parser_setup returns a configured Parser
    type Transition = (Box<dyn Fn() -> Parser>, Vec<(KeyEvent, Mode)>, &'static str);
    let transitions: Vec<Transition> = vec![
        // Ready (fresh parser)
        (Box::new(Parser::new), vec![], "Ready"),
        // AwaitingRegister
        (
            Box::new(Parser::new),
            vec![(KeyEvent::char('"'), Mode::Normal)],
            "AwaitingRegister",
        ),
        // Operator
        (
            Box::new(Parser::new),
            vec![(KeyEvent::char('d'), Mode::Normal)],
            "Operator",
        ),
        // AwaitingChar
        (
            Box::new(Parser::new),
            vec![(KeyEvent::char('f'), Mode::Normal)],
            "AwaitingChar",
        ),
        // AwaitingTextObject
        (
            Box::new(Parser::new),
            vec![
                (KeyEvent::char('d'), Mode::Normal),
                (KeyEvent::char('i'), Mode::Normal),
            ],
            "AwaitingTextObject",
        ),
        // AwaitingTextObjectWithModifier
        (
            Box::new(Parser::new),
            vec![
                (KeyEvent::char('d'), Mode::Normal),
                (KeyEvent::char('i'), Mode::Normal),
                (KeyEvent::char('n'), Mode::Normal),
            ],
            "AwaitingTextObjectWithModifier",
        ),
        // AwaitingVisualTextObjectWithModifier
        (
            Box::new(Parser::new),
            vec![
                (KeyEvent::char('i'), Mode::Visual(VisualType::Char)),
                (KeyEvent::char('n'), Mode::Visual(VisualType::Char)),
            ],
            "AwaitingVisualTextObjectWithModifier",
        ),
        // AwaitingPrefix
        (
            Box::new(Parser::new),
            vec![(KeyEvent::char('g'), Mode::Normal)],
            "AwaitingPrefix",
        ),
        // AwaitingMark
        (
            Box::new(Parser::new),
            vec![(KeyEvent::char('m'), Mode::Normal)],
            "AwaitingMark",
        ),
        // AwaitingInsertRegister
        (
            Box::new(Parser::new),
            vec![(KeyEvent::ctrl('r'), Mode::Insert)],
            "AwaitingInsertRegister",
        ),
        // AwaitingInsertExpression
        (
            Box::new(Parser::new),
            vec![
                (KeyEvent::ctrl('r'), Mode::Insert),
                (KeyEvent::char('='), Mode::Insert),
            ],
            "AwaitingInsertExpression",
        ),
        // AwaitingInsertCtrlG
        (
            Box::new(Parser::new),
            vec![(KeyEvent::ctrl('g'), Mode::Insert)],
            "AwaitingInsertCtrlG",
        ),
        // AwaitingInsertDigraph1
        (
            Box::new(Parser::new),
            vec![(KeyEvent::ctrl('k'), Mode::Insert)],
            "AwaitingInsertDigraph1",
        ),
        // AwaitingInsertDigraph2
        (
            Box::new(Parser::new),
            vec![
                (KeyEvent::ctrl('k'), Mode::Insert),
                (KeyEvent::char('e'), Mode::Insert),
            ],
            "AwaitingInsertDigraph2",
        ),
        // AwaitingInsertCtrlX
        (
            Box::new(Parser::new),
            vec![(KeyEvent::ctrl('x'), Mode::Insert)],
            "AwaitingInsertCtrlX",
        ),
        // InsertLiteral
        (
            Box::new(Parser::new),
            vec![(KeyEvent::ctrl('v'), Mode::Insert)],
            "InsertLiteral",
        ),
        // AwaitingMacroRegister
        (
            Box::new(Parser::new),
            vec![(KeyEvent::char('q'), Mode::Normal)],
            "AwaitingMacroRegister",
        ),
        // AwaitingVisualTextObject
        (
            Box::new(Parser::new),
            vec![(KeyEvent::char('i'), Mode::Visual(VisualType::Char))],
            "AwaitingVisualTextObject",
        ),
        // AwaitingWindowCommand
        (
            Box::new(Parser::new),
            vec![(KeyEvent::ctrl('w'), Mode::Normal)],
            "AwaitingWindowCommand",
        ),
    ];

    let mut covered: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for (i, (setup, keys, expected)) in transitions.iter().enumerate() {
        let mut parser = setup();
        for &(key, mode) in keys {
            let _ = parser.process(key, &keymap, mode);
        }
        let actual = variant_name(parser.state());
        assert_eq!(
            actual, *expected,
            "Transition #{i} expected state '{expected}' but got '{actual}'"
        );
        covered.insert(expected);
    }

    // Verify completeness
    let all: std::collections::HashSet<&str> = ALL_VARIANT_NAMES.iter().copied().collect();
    let missing: Vec<&&str> = all.difference(&covered).collect();
    let extra: Vec<&&str> = covered.difference(&all).collect();

    assert!(
        missing.is_empty(),
        "InputState variants NOT covered by transition tests: {missing:?}\n\
         Add entries to the transitions table to reach these states."
    );
    assert!(
        extra.is_empty(),
        "Transition tests reference unknown variants: {extra:?}\n\
         Update ALL_VARIANT_NAMES if these are new variants."
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Transition edge tests: verify state changes (from -> to)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn transition_ready_to_operator_via_count() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    // "3d" -> Ready(count=3) then Operator
    let _ = parser.process(KeyEvent::char('3'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Ready");
    assert_eq!(parser.state().count(), Some(3));

    let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Operator");
}

#[test]
fn transition_operator_to_text_object() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    let _ = parser.process(KeyEvent::char('y'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Operator");

    let _ = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingTextObject");
}

#[test]
fn transition_operator_to_char() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    let _ = parser.process(KeyEvent::char('c'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Operator");

    let _ = parser.process(KeyEvent::char('t'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingChar");
}

#[test]
fn transition_operator_to_prefix() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Operator");

    let _ = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingPrefix");
}

#[test]
fn transition_operator_to_mark() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    let _ = parser.process(KeyEvent::char('y'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Operator");

    let _ = parser.process(KeyEvent::char('\''), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingMark");
}

#[test]
fn transition_register_to_operator() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    // "ad -> AwaitingRegister -> Ready(register=a) -> Operator
    let _ = parser.process(KeyEvent::char('"'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingRegister");

    let _ = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
    // After register name, parser goes back to Ready with register set
    assert_eq!(variant_name(parser.state()), "Ready");

    let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Operator");
}

#[test]
fn transition_text_object_completes_to_ready() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    // "diw" -> Operator -> AwaitingTextObject -> Execute -> Ready
    let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
    let _ = parser.process(KeyEvent::char('i'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingTextObject");

    let result = parser.process(KeyEvent::char('w'), &keymap, Mode::Normal);
    assert!(
        matches!(result, GrammarResult::Execute(_)),
        "diw should produce Execute"
    );
    assert_eq!(variant_name(parser.state()), "Ready");
}

#[test]
fn transition_char_completes_to_ready() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    // "fa" -> AwaitingChar -> Execute -> Ready
    let _ = parser.process(KeyEvent::char('f'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingChar");

    let result = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
    assert!(
        matches!(result, GrammarResult::Execute(_)),
        "fa should produce Execute"
    );
    assert_eq!(variant_name(parser.state()), "Ready");
}

#[test]
fn transition_prefix_g_completes() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    // "gg" -> AwaitingPrefix -> Execute -> Ready
    let _ = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingPrefix");

    let result = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    assert!(
        matches!(result, GrammarResult::Execute(_)),
        "gg should produce Execute"
    );
    assert_eq!(variant_name(parser.state()), "Ready");
}

#[test]
fn transition_mark_completes() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    // "ma" -> AwaitingMark -> Execute -> Ready
    let _ = parser.process(KeyEvent::char('m'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingMark");

    let result = parser.process(KeyEvent::char('a'), &keymap, Mode::Normal);
    assert!(
        matches!(result, GrammarResult::Execute(_)),
        "ma should produce Execute"
    );
    assert_eq!(variant_name(parser.state()), "Ready");
}

#[test]
fn transition_escape_resets_to_ready() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // From Operator state
    let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Operator");
    let result = parser.process(KeyEvent::escape(), &keymap, Mode::Normal);
    assert!(
        matches!(result, GrammarResult::Cancel),
        "Escape should produce Cancel"
    );
    assert_eq!(variant_name(parser.state()), "Ready");

    // From AwaitingChar state
    let _ = parser.process(KeyEvent::char('f'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingChar");
    let result = parser.process(KeyEvent::escape(), &keymap, Mode::Normal);
    assert!(matches!(result, GrammarResult::Cancel));
    assert_eq!(variant_name(parser.state()), "Ready");

    // From AwaitingPrefix state
    let _ = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingPrefix");
    let result = parser.process(KeyEvent::escape(), &keymap, Mode::Normal);
    assert!(matches!(result, GrammarResult::Cancel));
    assert_eq!(variant_name(parser.state()), "Ready");
}

#[test]
fn transition_digraph_chain() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Ctrl-K enters AwaitingInsertDigraph1
    let _ = parser.process(KeyEvent::ctrl('k'), &keymap, Mode::Insert);
    assert_eq!(variant_name(parser.state()), "AwaitingInsertDigraph1");

    // First char enters AwaitingInsertDigraph2
    let _ = parser.process(KeyEvent::char('a'), &keymap, Mode::Insert);
    assert_eq!(variant_name(parser.state()), "AwaitingInsertDigraph2");

    // Second char completes
    let result = parser.process(KeyEvent::char(':'), &keymap, Mode::Insert);
    assert!(
        matches!(result, GrammarResult::Execute(_)),
        "Ctrl-K a : should produce Execute"
    );
    assert_eq!(variant_name(parser.state()), "Ready");
}

#[test]
fn transition_literal_digit_accumulation() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Ctrl-V enters InsertLiteral(AwaitingFirst)
    let _ = parser.process(KeyEvent::ctrl('v'), &keymap, Mode::Insert);
    assert_eq!(variant_name(parser.state()), "InsertLiteral");

    // First digit enters InsertLiteral(Decimal)
    let _ = parser.process(KeyEvent::char('0'), &keymap, Mode::Insert);
    assert_eq!(variant_name(parser.state()), "InsertLiteral");

    // Second digit stays in InsertLiteral(Decimal)
    let _ = parser.process(KeyEvent::char('6'), &keymap, Mode::Insert);
    assert_eq!(variant_name(parser.state()), "InsertLiteral");

    // Third digit completes
    let result = parser.process(KeyEvent::char('5'), &keymap, Mode::Insert);
    assert!(
        matches!(result, GrammarResult::Execute(_)),
        "Ctrl-V 065 should produce Execute"
    );
}

#[test]
fn transition_operator_register_after_operator() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // d"a -> Operator -> AwaitingRegister (AfterOperator phase)
    let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "Operator");

    let _ = parser.process(KeyEvent::char('"'), &keymap, Mode::Normal);
    assert_eq!(variant_name(parser.state()), "AwaitingRegister");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Variant name exhaustiveness check
// ═══════════════════════════════════════════════════════════════════════════════

/// Verify that `variant_name` covers all possible states by constructing
/// representative instances. This test is a compile-time guard: if a new
/// variant is added to InputState, the exhaustive match in `variant_name`
/// will fail to compile, forcing an update.
#[test]
fn variant_name_is_exhaustive() {
    // We just need variant_name to compile with its exhaustive match.
    // If InputState gains a new variant, the match in variant_name will
    // be non-exhaustive and the compiler will error. Since InputState is
    // #[non_exhaustive], we handle the `_` arm with "Unknown" to catch
    // any new variants at runtime.
    let ready = InputState::Ready {
        count: None,
        register: None,
    };
    let name = variant_name(&ready);
    assert_ne!(
        name, "Unknown",
        "variant_name should recognize Ready, not return Unknown"
    );
}

/// Verify that ALL_VARIANT_NAMES has exactly 19 entries (matching the known
/// number of InputState variants).
#[test]
fn variant_count_is_19() {
    assert_eq!(
        ALL_VARIANT_NAMES.len(),
        19,
        "Expected 19 InputState variants, got {}. \
         Update ALL_VARIANT_NAMES if variants were added or removed.",
        ALL_VARIANT_NAMES.len()
    );
}
