//! Test macros for vim-core.
//!
//! Provides one-liner test definitions for fidelity testing.

#[allow(unused_imports)]
pub use super::runner::run_fidelity_test;

/// One-liner fidelity test definition.
///
/// # Examples
///
/// Basic test (uses "basics" category by default):
/// ```ignore
/// vim_test!(cursor_right, "hello", "l");
/// ```
///
/// With cursor position:
/// ```ignore
/// vim_test!(cursor_at, "hello\nworld", cursor(1, 2), "k");
/// ```
///
/// With explicit category:
/// ```ignore
/// vim_test!(motions, word_forward, "hello world", "w");
/// ```
#[allow(unused_macros)]
macro_rules! vim_test {
    // With category: vim_test!(category, test_name, text, keys)
    ($category:ident, $name:ident, $text:expr, $keys:expr) => {
        #[test]
        fn $name() {
            crate::common::run_fidelity_test(
                stringify!($name),
                stringify!($category),
                $text,
                (0, 0),
                $keys,
            );
        }
    };

    // With category and cursor: vim_test!(category, test_name, text, cursor(line, col), keys)
    ($category:ident, $name:ident, $text:expr, cursor($line:expr, $col:expr), $keys:expr) => {
        #[test]
        fn $name() {
            crate::common::run_fidelity_test(
                stringify!($name),
                stringify!($category),
                $text,
                ($line, $col),
                $keys,
            );
        }
    };

    // Basic (default category "basics"): vim_test!(test_name, text, keys)
    ($name:ident, $text:expr, $keys:expr) => {
        #[test]
        fn $name() {
            crate::common::run_fidelity_test(stringify!($name), "basics", $text, (0, 0), $keys);
        }
    };

    // With cursor position (default category "basics"): vim_test!(test_name, text, cursor(line, col), keys)
    ($name:ident, $text:expr, cursor($line:expr, $col:expr), $keys:expr) => {
        #[test]
        fn $name() {
            crate::common::run_fidelity_test(
                stringify!($name),
                "basics",
                $text,
                ($line, $col),
                $keys,
            );
        }
    };
}

// Make the macro available to other modules
#[allow(unused_imports)]
pub(crate) use vim_test;

/// Assert that an effect slice matches an expected sequence of variants.
///
/// Supports two forms:
/// - **Bare variant** (checks discriminant only): `[BeginUndoGroup, Delete, EndUndoGroup]`
/// - **Field guard** (checks variant + field values): `[Delete { range } if range.start().get() == 0]`
///
/// # Examples
///
/// ```ignore
/// assert_effects!(effects, [SetCursor]);
/// assert_effects!(effects, [SetCursor { offset } if offset.get() == 5]);
/// assert_effects!(effects, []); // asserts empty
/// ```
#[allow(unused_macros)]
macro_rules! assert_effects {
    // Internal: check bare variant
    (@check $idx:expr, $effect:expr, $variant:ident) => {
        assert!(
            matches!($effect, vim_core::effects::Effect::$variant { .. }),
            "effect[{}]: expected {}, got {:?}",
            $idx, stringify!($variant), $effect,
        );
    };
    // Internal: check variant with field guard
    (@check $idx:expr, $effect:expr, $variant:ident { $($field:tt)* } if $($guard:tt)*) => {
        assert!(
            matches!($effect, vim_core::effects::Effect::$variant { $($field)*, .. } if $($guard)*),
            "effect[{}]: expected {} with guard `{}`, got {:?}",
            $idx, stringify!($variant), stringify!($($guard)*), $effect,
        );
    };
    // Entry point
    ($effects:expr, [$($variant:ident $( { $($field:tt)* } if $($guard:tt)* )?),* $(,)?]) => {{
        #[allow(unused_mut)]
        let effects = &$effects;
        let mut _expected_count = 0usize;
        $( let _ = stringify!($variant); _expected_count += 1; )*
        assert_eq!(
            effects.len(), _expected_count,
            "expected {} effects, got {}: {:?}",
            _expected_count, effects.len(), effects,
        );
        let mut _idx = 0usize;
        $(
            assert_effects!(@check _idx, effects[_idx], $variant $( { $($field)* } if $($guard)* )?);
            _idx += 1;
        )*
    }};
}

#[allow(unused_imports)]
pub(crate) use assert_effects;

/// Helper to create cursor position tuple.
#[allow(dead_code)]
pub const fn cursor(line: usize, col: usize) -> (usize, usize) {
    (line, col)
}

/// One-liner grammar parser test definition.
///
/// Generates a `#[test]` function that feeds keys into a fresh `Parser`
/// and asserts the resulting `GrammarResult` / `Command` variant.
///
/// # Arms
///
/// **Motion** — pure motion command:
/// ```ignore
/// grammar_test!(word_fwd, "w" => Motion(Motion::WordForward));
/// ```
///
/// **Op** — operator + motion:
/// ```ignore
/// grammar_test!(delete_word, "dw" => Op(Operator::Delete, Motion::WordForward));
/// ```
///
/// **OpLine** — linewise operator (doubled key):
/// ```ignore
/// grammar_test!(delete_line, "dd" => OpLine(Operator::Delete));
/// ```
///
/// **Action** — standalone action:
/// ```ignore
/// grammar_test!(del_char, "x" => Action(Action::DeleteChar));
/// ```
///
/// **Cancel** — escape / cancel:
/// ```ignore
/// grammar_test!(esc_cancel, "\x1b" => Cancel);
/// ```
#[allow(unused_macros)]
macro_rules! grammar_test {
    // Motion: grammar_test!(name, "w" => Motion(Motion::WordForward));
    ($name:ident, $keys:expr => Motion($expected_motion:expr)) => {
        #[test]
        fn $name() {
            use vim_core::grammar::{Command, GrammarResult, Motion, Parser};
            use vim_core::keymap::{KeyEvent, Keymap};
            use vim_core::primitives::Mode;

            let mut parser = Parser::new();
            let keymap = Keymap::default();
            let mut result = GrammarResult::Invalid;
            for c in $keys.chars() {
                result = parser.process(KeyEvent::char(c), &keymap, Mode::Normal);
            }
            match result {
                GrammarResult::Execute(Command::Motion { motion, .. }) => {
                    assert_eq!(motion, $expected_motion, "motion mismatch");
                }
                other => panic!(
                    "Expected Execute(Motion {{ motion: {:?}, .. }}), got {:?}",
                    $expected_motion, other
                ),
            }
        }
    };

    // Op: grammar_test!(name, "dw" => Op(Operator::Delete, Motion::WordForward));
    ($name:ident, $keys:expr => Op($expected_op:expr, $expected_motion:expr)) => {
        #[test]
        fn $name() {
            use vim_core::grammar::{Command, GrammarResult, Motion, Operator, Parser};
            use vim_core::keymap::{KeyEvent, Keymap};
            use vim_core::primitives::Mode;

            let mut parser = Parser::new();
            let keymap = Keymap::default();
            let mut result = GrammarResult::Invalid;
            for c in $keys.chars() {
                result = parser.process(KeyEvent::char(c), &keymap, Mode::Normal);
            }
            match result {
                GrammarResult::Execute(Command::OperatorMotion {
                    operator, motion, ..
                }) => {
                    assert_eq!(operator, $expected_op, "operator mismatch");
                    assert_eq!(motion, $expected_motion, "motion mismatch");
                }
                other => panic!(
                    "Expected Execute(OperatorMotion {{ operator: {:?}, motion: {:?}, .. }}), got {:?}",
                    $expected_op, $expected_motion, other
                ),
            }
        }
    };

    // OpLine: grammar_test!(name, "dd" => OpLine(Operator::Delete));
    ($name:ident, $keys:expr => OpLine($expected_op:expr)) => {
        #[test]
        fn $name() {
            use vim_core::grammar::{Command, GrammarResult, Operator, Parser};
            use vim_core::keymap::{KeyEvent, Keymap};
            use vim_core::primitives::Mode;

            let mut parser = Parser::new();
            let keymap = Keymap::default();
            let mut result = GrammarResult::Invalid;
            for c in $keys.chars() {
                result = parser.process(KeyEvent::char(c), &keymap, Mode::Normal);
            }
            match result {
                GrammarResult::Execute(Command::OperatorLine { operator, .. }) => {
                    assert_eq!(operator, $expected_op, "operator mismatch");
                }
                other => panic!(
                    "Expected Execute(OperatorLine {{ operator: {:?}, .. }}), got {:?}",
                    $expected_op, other
                ),
            }
        }
    };

    // Action: grammar_test!(name, "x" => Action(Action::DeleteChar));
    ($name:ident, $keys:expr => Action($expected_action:expr)) => {
        #[test]
        fn $name() {
            use vim_core::grammar::{Action, Command, GrammarResult, Parser};
            use vim_core::keymap::{KeyEvent, Keymap};
            use vim_core::primitives::Mode;

            let mut parser = Parser::new();
            let keymap = Keymap::default();
            let mut result = GrammarResult::Invalid;
            for c in $keys.chars() {
                result = parser.process(KeyEvent::char(c), &keymap, Mode::Normal);
            }
            match result {
                GrammarResult::Execute(Command::Action { action, .. }) => {
                    assert_eq!(action, $expected_action, "action mismatch");
                }
                other => panic!(
                    "Expected Execute(Action {{ action: {:?}, .. }}), got {:?}",
                    $expected_action, other
                ),
            }
        }
    };

    // Cancel: grammar_test!(name, "\x1b" => Cancel);
    ($name:ident, $keys:expr => Cancel) => {
        #[test]
        fn $name() {
            use vim_core::grammar::{GrammarResult, Parser};
            use vim_core::keymap::{KeyEvent, Keymap};
            use vim_core::primitives::Mode;

            let mut parser = Parser::new();
            let keymap = Keymap::default();
            let mut result = GrammarResult::Invalid;
            for c in $keys.chars() {
                let key = if c == '\x1b' {
                    KeyEvent::escape()
                } else {
                    KeyEvent::char(c)
                };
                result = parser.process(key, &keymap, Mode::Normal);
            }
            assert_eq!(result, GrammarResult::Cancel, "expected Cancel");
        }
    };
}

#[allow(unused_imports)]
pub(crate) use grammar_test;

// ── vim_action_test! ────────────────────────────────────────────────────────

/// Parse annotated text: find `|` cursor marker, remove it, return (text, byte_offset).
///
/// Panics if no `|` is found.
fn parse_annotated_simple(annotated: &str) -> (String, usize) {
    let offset = annotated.find('|').unwrap_or_else(|| {
        panic!(
            "annotated text must contain a '|' cursor marker: {:?}",
            annotated
        )
    });
    let mut text = String::with_capacity(annotated.len() - 1);
    text.push_str(&annotated[..offset]);
    text.push_str(&annotated[offset + 1..]);
    (text, offset)
}

/// Convert a byte offset to a 0-indexed (line, col) pair.
fn offset_to_line_col(text: &str, offset: usize) -> (usize, usize) {
    let mut line = 0;
    let mut line_start = 0;
    for (i, ch) in text[..offset].char_indices() {
        if ch == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    (line, offset - line_start)
}

/// Insert a `|` cursor marker at `offset` in `text`.
fn insert_cursor_marker(text: &str, offset: usize) -> String {
    let mut s = String::with_capacity(text.len() + 1);
    s.push_str(&text[..offset.min(text.len())]);
    s.push('|');
    s.push_str(&text[offset.min(text.len())..]);
    s
}

/// Run an action test using annotated text input/output.
///
/// Parses `|` markers from `input_annotated` and `expected_annotated`,
/// runs the key sequence through the full `run_vim_commands` infrastructure
/// (which includes invariant checking), then compares results.
pub fn run_action_test(
    input_annotated: &str,
    keys: &str,
    expected_annotated: &str,
    check_mode: Option<&str>,
    check_registers: &[(&str, &str)],
) {
    use crate::common::golden::TestInput;
    use crate::common::runner::run_vim_commands;

    // 1. Parse input
    let (input_text, input_offset) = parse_annotated_simple(input_annotated);
    let (input_line, input_col) = offset_to_line_col(&input_text, input_offset);
    let input = TestInput::new(&input_text, (input_line, input_col), keys);

    // 2. Run through VimEngine via full test runner (gets invariant checking for free)
    let actual = run_vim_commands(&input, None, false);

    // 3. Parse expected
    let (expected_text, expected_offset) = parse_annotated_simple(expected_annotated);

    // 4. Compare text (strip trailing '\n' from actual, same normalization as fidelity tests)
    let actual_text = actual.text.strip_suffix('\n').unwrap_or(&actual.text);
    if actual_text != expected_text {
        let actual_annotated = insert_cursor_marker(actual_text, actual.cursor_offset);
        panic!(
            "\n\nACTION TEST FAILED — text mismatch\n\
             \x20 Input:    \"{}\"\n\
             \x20 Keys:     \"{}\"\n\
             \x20 Expected: \"{}\"\n\
             \x20 Actual:   \"{}\"\n\
             \x20 Expected text: {:?}\n\
             \x20 Actual text:   {:?}\n",
            input_annotated, keys, expected_annotated, actual_annotated, expected_text, actual_text,
        );
    }

    // 5. Compare cursor offset
    if actual.cursor_offset != expected_offset {
        let actual_annotated = insert_cursor_marker(actual_text, actual.cursor_offset);
        panic!(
            "\n\nACTION TEST FAILED — cursor mismatch\n\
             \x20 Input:    \"{}\"\n\
             \x20 Keys:     \"{}\"\n\
             \x20 Expected: \"{}\"\n\
             \x20 Actual:   \"{}\"\n\
             \x20 Expected offset: {}\n\
             \x20 Actual offset:   {}\n",
            input_annotated,
            keys,
            expected_annotated,
            actual_annotated,
            expected_offset,
            actual.cursor_offset,
        );
    }

    // 6. Check mode (if requested)
    if let Some(mode) = check_mode {
        if actual.mode != mode {
            panic!(
                "\n\nACTION TEST FAILED — mode mismatch\n\
                 \x20 Input:    \"{}\"\n\
                 \x20 Keys:     \"{}\"\n\
                 \x20 Expected mode: {}\n\
                 \x20 Actual mode:   {}\n",
                input_annotated, keys, mode, actual.mode,
            );
        }
    }

    // 7. Check registers (if requested)
    for &(reg_name, expected_content) in check_registers {
        let actual_content = actual
            .registers
            .get(reg_name)
            .map(|r| r.text.as_str())
            .unwrap_or("");
        if actual_content != expected_content {
            panic!(
                "\n\nACTION TEST FAILED — register '{}' mismatch\n\
                 \x20 Input:    \"{}\"\n\
                 \x20 Keys:     \"{}\"\n\
                 \x20 Expected register '{}': {:?}\n\
                 \x20 Actual register '{}':   {:?}\n",
                reg_name,
                input_annotated,
                keys,
                reg_name,
                expected_content,
                reg_name,
                actual_content,
            );
        }
    }
}

/// One-liner action test definition using annotated text.
///
/// Uses `|` as cursor marker in input/output text. Runs through the full
/// `run_vim_commands` infrastructure, which includes per-key invariant
/// checking.
///
/// # Arms
///
/// Basic:
/// ```ignore
/// vim_action_test!(cursor_right, "|hello", "l", "h|ello");
/// ```
///
/// With mode check:
/// ```ignore
/// vim_action_test!(enter_visual, "|hello", "v", "|hello", mode = "Visual");
/// ```
///
/// With register check:
/// ```ignore
/// vim_action_test!(yank_word, "|hello world", "yw", "|hello world", reg("\"" => "hello "));
/// ```
///
/// Mode + registers:
/// ```ignore
/// vim_action_test!(name, "|text", "keys", "|text", mode = "Normal", reg("\"" => "x"));
/// ```
#[allow(unused_macros)]
macro_rules! vim_action_test {
    // Basic: vim_action_test!(name, input, keys, expected)
    ($name:ident, $input:expr, $keys:expr, $expected:expr) => {
        #[test]
        fn $name() {
            crate::common::macros::run_action_test($input, $keys, $expected, None, &[]);
        }
    };

    // With mode: vim_action_test!(name, input, keys, expected, mode = "Visual")
    ($name:ident, $input:expr, $keys:expr, $expected:expr, mode = $mode:expr) => {
        #[test]
        fn $name() {
            crate::common::macros::run_action_test($input, $keys, $expected, Some($mode), &[]);
        }
    };

    // With registers: vim_action_test!(name, input, keys, expected, reg("\"" => "hello"))
    ($name:ident, $input:expr, $keys:expr, $expected:expr, reg($($rname:expr => $rval:expr),+)) => {
        #[test]
        fn $name() {
            crate::common::macros::run_action_test(
                $input, $keys, $expected, None,
                &[$(($rname, $rval)),+],
            );
        }
    };

    // Mode + registers
    ($name:ident, $input:expr, $keys:expr, $expected:expr, mode = $mode:expr, reg($($rname:expr => $rval:expr),+)) => {
        #[test]
        fn $name() {
            crate::common::macros::run_action_test(
                $input, $keys, $expected, Some($mode),
                &[$(($rname, $rval)),+],
            );
        }
    };
}

#[allow(unused_imports)]
pub(crate) use vim_action_test;
