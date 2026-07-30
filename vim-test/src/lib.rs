#![deny(missing_docs)]
#![allow(
    clippy::indexing_slicing,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    reason = "test infrastructure — panics are the assertion mechanism"
)]

//! # vim-test
//!
//! Universal test framework for vim-core. Two authoring styles:
//!
//! - **Fluent builder**: `vim("|hello").keys("dw").expect_text("|").run()`
//! - **Spec table macros**: `vim_suite!(motions { ... })`
//!
//! Both share a single assertion engine backed by `TestSession`.

pub mod builder;
pub mod effects;
pub mod error;
pub mod fidelity;
pub mod golden;
pub mod grammar;
pub mod keys;
pub mod multi_cursor;
pub mod neovim_oracle;
pub mod session;
pub mod snapshot;
pub mod state_diff;
pub mod undo;

// ═══════════════════════════════════════════════════════════════════════════
// SPEC TABLE MACROS
// ═══════════════════════════════════════════════════════════════════════════

/// Runtime backing for `vim_spec!`. Creates a `TestSession`, feeds keys,
/// asserts partial expectations.
pub fn run_spec_test(
    input_annotated: &str,
    keys: &str,
    expected: snapshot::ExpectedState,
    label: &str,
) {
    let mut session = session::TestSession::new(input_annotated);
    session.feed(keys);
    snapshot::assert_state(&session, expected, label);
}

/// Single-test generator from a spec definition.
///
/// # Forms
///
/// ```ignore
/// // Text assertion only
/// vim_spec!(name, "|input", "keys" => "|expected");
///
/// // Text + mode
/// vim_spec!(name, "|input", "keys" => "|expected", mode: Mode::Insert);
///
/// // Text + register
/// vim_spec!(name, "|input", "keys" => "|expected", reg('"' => "content"));
///
/// // No text assertion (smoke test)
/// vim_spec!(name, "|input", "keys");
/// ```
#[macro_export]
macro_rules! vim_spec {
    // Text only
    ($name:ident, $input:expr, $keys:expr => $expected:expr) => {
        #[test]
        fn $name() {
            $crate::run_spec_test(
                $input,
                $keys,
                $crate::snapshot::ExpectedState::EMPTY.text($expected),
                stringify!($name),
            );
        }
    };

    // Text + mode
    ($name:ident, $input:expr, $keys:expr => $expected:expr, mode: $mode:expr) => {
        #[test]
        fn $name() {
            $crate::run_spec_test(
                $input,
                $keys,
                $crate::snapshot::ExpectedState::EMPTY
                    .text($expected)
                    .mode($mode),
                stringify!($name),
            );
        }
    };

    // Text + register
    ($name:ident, $input:expr, $keys:expr => $expected:expr, reg($rn:expr => $rv:expr)) => {
        #[test]
        fn $name() {
            $crate::run_spec_test(
                $input,
                $keys,
                $crate::snapshot::ExpectedState::EMPTY
                    .text($expected)
                    .register($rn, $rv),
                stringify!($name),
            );
        }
    };

    // Text + mode + register
    ($name:ident, $input:expr, $keys:expr => $expected:expr, mode: $mode:expr, reg($rn:expr => $rv:expr)) => {
        #[test]
        fn $name() {
            $crate::run_spec_test(
                $input,
                $keys,
                $crate::snapshot::ExpectedState::EMPTY
                    .text($expected)
                    .mode($mode)
                    .register($rn, $rv),
                stringify!($name),
            );
        }
    };

    // No expected text (smoke test)
    ($name:ident, $input:expr, $keys:expr) => {
        #[test]
        fn $name() {
            $crate::run_spec_test(
                $input,
                $keys,
                $crate::snapshot::ExpectedState::EMPTY,
                stringify!($name),
            );
        }
    };

    // Mode only (no text check)
    ($name:ident, $input:expr, $keys:expr, mode: $mode:expr) => {
        #[test]
        fn $name() {
            $crate::run_spec_test(
                $input,
                $keys,
                $crate::snapshot::ExpectedState::EMPTY.mode($mode),
                stringify!($name),
            );
        }
    };
}

/// Bulk test suite generator. Each spec generates a `#[test]` function
/// inside a module.
///
/// ```ignore
/// vim_suite!(motions_h {
///     basic:       "hel|lo",       "h"  => "he|llo";
///     at_start:    "|hello",       "h"  => "|hello";
///     with_count:  "hello |world", "3h" => "hel|lo world";
/// });
/// ```
#[macro_export]
macro_rules! vim_suite {
    ($suite:ident { $($body:tt)* }) => {
        mod $suite {
            #[allow(unused_imports)]
            use super::*;
            $crate::vim_suite!(@parse $($body)*);
        }
    };

    // Terminal
    (@parse) => {};

    // Text + mode + register
    (@parse $name:ident : $input:expr, $keys:expr => $expected:expr, mode: $mode:expr, reg($rn:expr => $rv:expr) ; $($rest:tt)*) => {
        $crate::vim_spec!($name, $input, $keys => $expected, mode: $mode, reg($rn => $rv));
        $crate::vim_suite!(@parse $($rest)*);
    };

    // Text + mode
    (@parse $name:ident : $input:expr, $keys:expr => $expected:expr, mode: $mode:expr ; $($rest:tt)*) => {
        $crate::vim_spec!($name, $input, $keys => $expected, mode: $mode);
        $crate::vim_suite!(@parse $($rest)*);
    };

    // Text + register
    (@parse $name:ident : $input:expr, $keys:expr => $expected:expr, reg($rn:expr => $rv:expr) ; $($rest:tt)*) => {
        $crate::vim_spec!($name, $input, $keys => $expected, reg($rn => $rv));
        $crate::vim_suite!(@parse $($rest)*);
    };

    // Text only
    (@parse $name:ident : $input:expr, $keys:expr => $expected:expr ; $($rest:tt)*) => {
        $crate::vim_spec!($name, $input, $keys => $expected);
        $crate::vim_suite!(@parse $($rest)*);
    };

    // Mode only
    (@parse $name:ident : $input:expr, $keys:expr, mode: $mode:expr ; $($rest:tt)*) => {
        $crate::vim_spec!($name, $input, $keys, mode: $mode);
        $crate::vim_suite!(@parse $($rest)*);
    };

    // Smoke test (no assertion)
    (@parse $name:ident : $input:expr, $keys:expr ; $($rest:tt)*) => {
        $crate::vim_spec!($name, $input, $keys);
        $crate::vim_suite!(@parse $($rest)*);
    };
}

/// Runtime backing for `mc_vim_spec!`.
pub fn run_mc_spec_test(
    text: &str,
    cursor_offsets: &[usize],
    keys: &str,
    expected: snapshot::ExpectedState,
    label: &str,
) {
    let mut session_inner =
        vim_core::execution::HostSession::new(text).with_auto_handle_defaults(true);
    if !cursor_offsets.is_empty() {
        session_inner.set_cursor_offset(cursor_offsets[0]);
        for &offset in &cursor_offsets[1..] {
            session_inner
                .add_cursor(offset)
                .unwrap_or_else(|e| panic!("add_cursor({offset}) failed: {e}"));
        }
    }
    let mut session = session::TestSession::from_host_session(session_inner);
    session.feed(keys);
    snapshot::assert_state(&session, expected, label);
}

/// Multi-cursor spec test macro.
///
/// ```ignore
/// mc_vim_spec!(name, "text", [0, 5], "keys" => "expected");
/// ```
#[macro_export]
macro_rules! mc_vim_spec {
    ($name:ident, $text:expr, [$($offset:expr),+ $(,)?], $keys:expr => $expected:expr) => {
        #[test]
        fn $name() {
            $crate::run_mc_spec_test(
                $text,
                &[$($offset),+],
                $keys,
                $crate::snapshot::ExpectedState::EMPTY.text($expected),
                stringify!($name),
            );
        }
    };

    ($name:ident, $text:expr, [$($offset:expr),+ $(,)?], $keys:expr) => {
        #[test]
        fn $name() {
            $crate::run_mc_spec_test(
                $text,
                &[$($offset),+],
                $keys,
                $crate::snapshot::ExpectedState::EMPTY,
                stringify!($name),
            );
        }
    };
}

/// Multi-cursor suite macro.
///
/// ```ignore
/// mc_vim_suite!(tilde {
///     opposite: "AaBb", [0, 2], "~" => "aabb";
/// });
/// ```
#[macro_export]
macro_rules! mc_vim_suite {
    ($suite:ident { $($body:tt)* }) => {
        mod $suite {
            #[allow(unused_imports)]
            use super::*;
            $crate::mc_vim_suite!(@parse $($body)*);
        }
    };

    (@parse) => {};

    (@parse $name:ident : $text:expr, [$($offset:expr),+ $(,)?], $keys:expr => $expected:expr ; $($rest:tt)*) => {
        $crate::mc_vim_spec!($name, $text, [$($offset),+], $keys => $expected);
        $crate::mc_vim_suite!(@parse $($rest)*);
    };

    (@parse $name:ident : $text:expr, [$($offset:expr),+ $(,)?], $keys:expr ; $($rest:tt)*) => {
        $crate::mc_vim_spec!($name, $text, [$($offset),+], $keys);
        $crate::mc_vim_suite!(@parse $($rest)*);
    };
}

// ═══════════════════════════════════════════════════════════════════════════
// EFFECT ASSERTIONS
// ═══════════════════════════════════════════════════════════════════════════

/// Assert that a slice of effects matches expected variants in order.
///
/// ```ignore
/// assert_effects!(effects, [SetCursor]);
/// assert_effects!(effects, [SetCursor { offset } if offset.get() == 5]);
/// assert_effects!(effects, []); // asserts empty
/// ```
#[macro_export]
macro_rules! assert_effects {
    (@check $idx:expr, $effect:expr, $variant:ident) => {
        assert!(
            matches!($effect, ::vim_core::effects::Effect::$variant { .. }),
            "effect[{}]: expected {}, got {:?}",
            $idx, stringify!($variant), $effect,
        );
    };
    (@check $idx:expr, $effect:expr, $variant:ident { $($field:tt)* } if $($guard:tt)*) => {
        assert!(
            matches!($effect, ::vim_core::effects::Effect::$variant { $($field)*, .. } if $($guard)*),
            "effect[{}]: expected {} with guard `{}`, got {:?}",
            $idx, stringify!($variant), stringify!($($guard)*), $effect,
        );
    };
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
            $crate::assert_effects!(@check _idx, effects[_idx], $variant $( { $($field)* } if $($guard)* )?);
            _idx += 1;
        )*
    }};
}

// ═══════════════════════════════════════════════════════════════════════════
// NEOVIM FIDELITY TEST MACRO
// ═══════════════════════════════════════════════════════════════════════════

/// Helper to create cursor position tuple.
pub const fn cursor(line: usize, col: usize) -> (usize, usize) {
    (line, col)
}

/// Neovim fidelity test — generates a `#[test]` that compares vim-core
/// output against a Neovim golden file.
///
/// ```ignore
/// // Uncategorized, default cursor (0,0)
/// neovim_test!(cursor_right, "hello", "l");
///
/// // Categorized
/// neovim_test!(motions, word_forward, "hello world", "w");
///
/// // Categorized with explicit cursor
/// neovim_test!(motions, word_at_end, "hello world", cursor(0, 5), "w");
///
/// // Uncategorized with explicit cursor
/// neovim_test!(cursor_mid, "hello", cursor(0, 2), "l");
/// ```
#[macro_export]
macro_rules! neovim_test {
    // Uncategorized, default cursor
    ($name:ident, $text:expr, $keys:expr) => {
        #[test]
        fn $name() {
            $crate::fidelity::run_neovim_test(
                stringify!($name),
                "uncategorized",
                $text,
                (0, 0),
                $keys,
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    };

    // Uncategorized, explicit cursor
    ($name:ident, $text:expr, cursor($line:expr, $col:expr), $keys:expr) => {
        #[test]
        fn $name() {
            $crate::fidelity::run_neovim_test(
                stringify!($name),
                "uncategorized",
                $text,
                ($line, $col),
                $keys,
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    };

    // Categorized, default cursor
    ($category:ident, $name:ident, $text:expr, $keys:expr) => {
        #[test]
        fn $name() {
            $crate::fidelity::run_neovim_test(
                stringify!($name),
                stringify!($category),
                $text,
                (0, 0),
                $keys,
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    };

    // Categorized, explicit cursor
    ($category:ident, $name:ident, $text:expr, cursor($line:expr, $col:expr), $keys:expr) => {
        #[test]
        fn $name() {
            $crate::fidelity::run_neovim_test(
                stringify!($name),
                stringify!($category),
                $text,
                ($line, $col),
                $keys,
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    };
}

// ═══════════════════════════════════════════════════════════════════════════
// GRAMMAR TEST MACRO
// ═══════════════════════════════════════════════════════════════════════════

/// Grammar parser test — feeds keys through `Parser` and asserts the result.
///
/// ```ignore
/// grammar_test!(word_fwd, "w" => Motion(Motion::WordForward));
/// grammar_test!(delete_word, "dw" => Op(Operator::Delete, Motion::WordForward));
/// grammar_test!(delete_line, "dd" => OpLine(Operator::Delete));
/// grammar_test!(del_char, "x" => Action(Action::DeleteChar));
/// grammar_test!(esc_cancel, "\x1b" => Cancel);
/// ```
#[macro_export]
macro_rules! grammar_test {
    // Motion
    ($name:ident, $keys:expr => Motion($expected_motion:expr)) => {
        #[test]
        fn $name() {
            let result = $crate::grammar::parse_grammar($keys);
            match result {
                ::vim_core::grammar::GrammarResult::Execute(
                    ::vim_core::grammar::Command::Motion { motion, .. },
                ) => {
                    assert_eq!(motion, $expected_motion, "motion mismatch");
                }
                other => panic!(
                    "Expected Execute(Motion {{ motion: {:?}, .. }}), got {:?}",
                    $expected_motion, other
                ),
            }
        }
    };

    // Op (operator + motion)
    ($name:ident, $keys:expr => Op($expected_op:expr, $expected_motion:expr)) => {
        #[test]
        fn $name() {
            let result = $crate::grammar::parse_grammar($keys);
            match result {
                ::vim_core::grammar::GrammarResult::Execute(
                    ::vim_core::grammar::Command::OperatorMotion {
                        operator, motion, ..
                    },
                ) => {
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

    // OpLine (linewise operator)
    ($name:ident, $keys:expr => OpLine($expected_op:expr)) => {
        #[test]
        fn $name() {
            let result = $crate::grammar::parse_grammar($keys);
            match result {
                ::vim_core::grammar::GrammarResult::Execute(
                    ::vim_core::grammar::Command::OperatorLine { operator, .. },
                ) => {
                    assert_eq!(operator, $expected_op, "operator mismatch");
                }
                other => panic!(
                    "Expected Execute(OperatorLine {{ operator: {:?}, .. }}), got {:?}",
                    $expected_op, other
                ),
            }
        }
    };

    // Action
    ($name:ident, $keys:expr => Action($expected_action:expr)) => {
        #[test]
        fn $name() {
            let result = $crate::grammar::parse_grammar($keys);
            match result {
                ::vim_core::grammar::GrammarResult::Execute(
                    ::vim_core::grammar::Command::Action { action, .. },
                ) => {
                    assert_eq!(action, $expected_action, "action mismatch");
                }
                other => panic!(
                    "Expected Execute(Action {{ action: {:?}, .. }}), got {:?}",
                    $expected_action, other
                ),
            }
        }
    };

    // Cancel
    ($name:ident, $keys:expr => Cancel) => {
        #[test]
        fn $name() {
            let result = $crate::grammar::parse_grammar($keys);
            assert_eq!(
                result,
                ::vim_core::grammar::GrammarResult::Cancel,
                "expected Cancel"
            );
        }
    };
}

/// Prelude — import everything needed to write tests.
///
/// ```ignore
/// use vim_test::prelude::*;
/// ```
pub mod prelude {
    pub use crate::builder::{vim, vim_mc, VimTestBuilder};
    pub use crate::effects::{EffectInspector, EffectLog};
    pub use crate::error::TerminalRenderer;
    pub use crate::fidelity::FidelitySession;
    pub use crate::golden::{GoldenFile, GoldenState, TestInput};
    pub use crate::keys::parse_keys;
    pub use crate::multi_cursor::{assert_cursor_count, assert_cursors};
    pub use crate::neovim_oracle::NeovimOracle;
    pub use crate::session::TestSession;
    pub use crate::snapshot::{assert_mode, assert_state, assert_text, ExpectedState, VimSnapshot};
    pub use crate::snapshot::{CursorSnapshot, RegisterEntry};
    pub use crate::state_diff::{compare_states, StateDiff};
    pub use crate::undo::{assert_atomic, assert_round_trip, UndoSnapshot};
    pub use crate::{
        assert_effects, grammar_test, mc_vim_spec, mc_vim_suite, neovim_test, vim_block, vim_spec,
        vim_suite,
    };

    pub use vim_core::effects::{Effect, EffectKind};
    pub use vim_core::grammar::{Action, Command, GrammarResult, Motion, Operator};
    pub use vim_core::primitives::{
        Mode, MotionType, Offset, OptionValue, SelectionRange, VisualType,
    };
    pub use vim_core::test_utils::annotated_text::{
        annotate, annotate_multi, parse, parse_multi, CursorSpec, MultiCursorSpec,
    };
}
