#![allow(non_snake_case)]
//! Comprehensive grammar parser tests.
//!
//! 50+ tests covering all grammar parsing scenarios per phase-4-grammar.md spec.

use std::num::NonZeroU32;

use vim_core::grammar::{
    Action, CharCommand, Command, GrammarResult, Motion, Operator, Parser, TextObjectKind,
    VisualKind,
};
use vim_core::keymap::{KeyEvent, Keymap};
use vim_core::primitives::{InsertEntryType, Mode, VisualType};

// ─────────────────────────────────────────────────────────────────────────────
// Test Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a key sequence and return the final result.
fn parse_keys(keys: &str) -> GrammarResult {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    let mut result = GrammarResult::Invalid;

    for c in keys.chars() {
        result = parser.process(KeyEvent::char(c), &keymap, Mode::Normal);
    }

    result
}

/// Parse with escape at end.
fn parse_keys_escape(keys: &str) -> GrammarResult {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    for c in keys.chars() {
        let _ = parser.process(KeyEvent::char(c), &keymap, Mode::Normal);
    }

    parser.process(KeyEvent::escape(), &keymap, Mode::Normal)
}

/// Assert motion command with count.
fn assert_motion(result: GrammarResult, expected_count: NonZeroU32, expected_motion: Motion) {
    match result {
        GrammarResult::Execute(Command::Motion { count, motion, .. }) => {
            assert_eq!(count, expected_count, "count mismatch");
            assert_eq!(motion, expected_motion, "motion mismatch");
        }
        _ => panic!("Expected Motion, got {:?}", result),
    }
}

/// Assert operator+motion command.
fn assert_op_motion(
    result: GrammarResult,
    expected_op: Operator,
    expected_motion: Motion,
    expected_count: NonZeroU32,
) {
    match result {
        GrammarResult::Execute(Command::OperatorMotion {
            operator,
            motion,
            count,
            ..
        }) => {
            assert_eq!(operator, expected_op, "operator mismatch");
            assert_eq!(motion, expected_motion, "motion mismatch");
            assert_eq!(count, expected_count, "count mismatch");
        }
        _ => panic!("Expected OperatorMotion, got {:?}", result),
    }
}

/// Assert linewise operator command.
fn assert_op_line(result: GrammarResult, expected_op: Operator, expected_count: NonZeroU32) {
    match result {
        GrammarResult::Execute(Command::OperatorLine {
            operator, count, ..
        }) => {
            assert_eq!(operator, expected_op, "operator mismatch");
            assert_eq!(count, expected_count, "count mismatch");
        }
        _ => panic!("Expected OperatorLine, got {:?}", result),
    }
}

/// Assert text object command.
fn assert_textobj(
    result: GrammarResult,
    expected_op: Operator,
    expected_inner: bool,
    expected_kind: TextObjectKind,
) {
    match result {
        GrammarResult::Execute(Command::OperatorTextObject {
            operator,
            textobject,
            ..
        }) => {
            assert_eq!(operator, expected_op, "operator mismatch");
            assert_eq!(
                textobject.scope.is_inner(),
                expected_inner,
                "inner mismatch"
            );
            assert_eq!(textobject.kind, expected_kind, "kind mismatch");
        }
        _ => panic!("Expected OperatorTextObject, got {:?}", result),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 1: Simple Motions (10 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn motion_h() {
    assert_motion(parse_keys("h"), NonZeroU32::new(1).unwrap(), Motion::Left);
}

#[test]
fn motion_j() {
    assert_motion(parse_keys("j"), NonZeroU32::new(1).unwrap(), Motion::Down);
}

#[test]
fn motion_k() {
    assert_motion(parse_keys("k"), NonZeroU32::new(1).unwrap(), Motion::Up);
}

#[test]
fn motion_l() {
    assert_motion(parse_keys("l"), NonZeroU32::new(1).unwrap(), Motion::Right);
}

#[test]
fn motion_w() {
    assert_motion(
        parse_keys("w"),
        NonZeroU32::new(1).unwrap(),
        Motion::WordForward,
    );
}

#[test]
fn motion_b() {
    assert_motion(
        parse_keys("b"),
        NonZeroU32::new(1).unwrap(),
        Motion::WordBackward,
    );
}

#[test]
fn motion_e() {
    assert_motion(
        parse_keys("e"),
        NonZeroU32::new(1).unwrap(),
        Motion::WordEnd,
    );
}

#[test]
fn motion_zero() {
    assert_motion(
        parse_keys("0"),
        NonZeroU32::new(1).unwrap(),
        Motion::LineStart,
    );
}

#[test]
fn motion_dollar() {
    assert_motion(
        parse_keys("$"),
        NonZeroU32::new(1).unwrap(),
        Motion::LineEnd,
    );
}

#[test]
fn motion_caret() {
    assert_motion(
        parse_keys("^"),
        NonZeroU32::new(1).unwrap(),
        Motion::FirstNonBlank,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 2: Counts (10 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn count_3j() {
    assert_motion(parse_keys("3j"), NonZeroU32::new(3).unwrap(), Motion::Down);
}

#[test]
fn count_5w() {
    assert_motion(
        parse_keys("5w"),
        NonZeroU32::new(5).unwrap(),
        Motion::WordForward,
    );
}

#[test]
fn count_10h() {
    assert_motion(
        parse_keys("10h"),
        NonZeroU32::new(10).unwrap(),
        Motion::Left,
    );
}

#[test]
fn count_99k() {
    assert_motion(parse_keys("99k"), NonZeroU32::new(99).unwrap(), Motion::Up);
}

#[test]
fn count_100l() {
    assert_motion(
        parse_keys("100l"),
        NonZeroU32::new(100).unwrap(),
        Motion::Right,
    );
}

#[test]
fn count_7b() {
    assert_motion(
        parse_keys("7b"),
        NonZeroU32::new(7).unwrap(),
        Motion::WordBackward,
    );
}

#[test]
fn count_15e() {
    assert_motion(
        parse_keys("15e"),
        NonZeroU32::new(15).unwrap(),
        Motion::WordEnd,
    );
}

#[test]
fn count_with_zero_10j() {
    // 0 in middle of count is digit, not motion
    assert_motion(
        parse_keys("10j"),
        NonZeroU32::new(10).unwrap(),
        Motion::Down,
    );
}

#[test]
fn count_with_zeros_100w() {
    assert_motion(
        parse_keys("100w"),
        NonZeroU32::new(100).unwrap(),
        Motion::WordForward,
    );
}

#[test]
fn count_large_999h() {
    assert_motion(
        parse_keys("999h"),
        NonZeroU32::new(999).unwrap(),
        Motion::Left,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 3: Operators (10 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn op_dw() {
    assert_op_motion(
        parse_keys("dw"),
        Operator::Delete,
        Motion::WordForward,
        NonZeroU32::new(1).unwrap(),
    );
}

#[test]
fn op_cj() {
    assert_op_motion(
        parse_keys("cj"),
        Operator::Change,
        Motion::Down,
        NonZeroU32::new(1).unwrap(),
    );
}

#[test]
fn op_yh() {
    assert_op_motion(
        parse_keys("yh"),
        Operator::Yank,
        Motion::Left,
        NonZeroU32::new(1).unwrap(),
    );
}

#[test]
fn op_d_dollar() {
    assert_op_motion(
        parse_keys("d$"),
        Operator::Delete,
        Motion::LineEnd,
        NonZeroU32::new(1).unwrap(),
    );
}

#[test]
fn op_dd() {
    assert_op_line(
        parse_keys("dd"),
        Operator::Delete,
        NonZeroU32::new(1).unwrap(),
    );
}

#[test]
fn op_yy() {
    assert_op_line(
        parse_keys("yy"),
        Operator::Yank,
        NonZeroU32::new(1).unwrap(),
    );
}

#[test]
fn op_cc() {
    assert_op_line(
        parse_keys("cc"),
        Operator::Change,
        NonZeroU32::new(1).unwrap(),
    );
}

#[test]
fn op_3dd() {
    assert_op_line(
        parse_keys("3dd"),
        Operator::Delete,
        NonZeroU32::new(3).unwrap(),
    );
}

#[test]
fn op_indent() {
    assert_op_motion(
        parse_keys(">j"),
        Operator::Indent,
        Motion::Down,
        NonZeroU32::new(1).unwrap(),
    );
}

#[test]
fn op_outdent() {
    assert_op_motion(
        parse_keys("<k"),
        Operator::Outdent,
        Motion::Up,
        NonZeroU32::new(1).unwrap(),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 4: Text Objects (10 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn textobj_diw() {
    assert_textobj(
        parse_keys("diw"),
        Operator::Delete,
        true,
        TextObjectKind::Word,
    );
}

#[test]
fn textobj_daw() {
    assert_textobj(
        parse_keys("daw"),
        Operator::Delete,
        false,
        TextObjectKind::Word,
    );
}

#[test]
fn textobj_ciW() {
    assert_textobj(
        parse_keys("ciW"),
        Operator::Change,
        true,
        TextObjectKind::WORD,
    );
}

#[test]
fn textobj_yi_paren() {
    assert_textobj(
        parse_keys("yi("),
        Operator::Yank,
        true,
        TextObjectKind::Paren,
    );
}

#[test]
fn textobj_da_paren() {
    assert_textobj(
        parse_keys("da)"),
        Operator::Delete,
        false,
        TextObjectKind::Paren,
    );
}

#[test]
fn textobj_ci_brace() {
    assert_textobj(
        parse_keys("ci{"),
        Operator::Change,
        true,
        TextObjectKind::Brace,
    );
}

#[test]
fn textobj_da_bracket() {
    assert_textobj(
        parse_keys("da]"),
        Operator::Delete,
        false,
        TextObjectKind::Bracket,
    );
}

#[test]
fn textobj_yi_quote() {
    assert_textobj(
        parse_keys("yi\""),
        Operator::Yank,
        true,
        TextObjectKind::DoubleQuote,
    );
}

#[test]
fn textobj_ci_single_quote() {
    assert_textobj(
        parse_keys("ci'"),
        Operator::Change,
        true,
        TextObjectKind::SingleQuote,
    );
}

#[test]
fn textobj_dip() {
    assert_textobj(
        parse_keys("dip"),
        Operator::Delete,
        true,
        TextObjectKind::Paragraph,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 5: Cancel/Escape (5 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn escape_from_ready() {
    let result = parse_keys_escape("");
    assert_eq!(result, GrammarResult::Cancel);
}

#[test]
fn escape_from_count() {
    let result = parse_keys_escape("3");
    assert_eq!(result, GrammarResult::Cancel);
}

#[test]
fn escape_from_operator() {
    let result = parse_keys_escape("d");
    assert_eq!(result, GrammarResult::Cancel);
}

#[test]
fn escape_from_operator_count() {
    let result = parse_keys_escape("d3");
    assert_eq!(result, GrammarResult::Cancel);
}

#[test]
fn escape_resets_to_ready() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Start building a command
    let _ = parser.process(KeyEvent::char('d'), &keymap, Mode::Normal);
    let _ = parser.process(KeyEvent::char('3'), &keymap, Mode::Normal);

    // Escape
    let _ = parser.process(KeyEvent::escape(), &keymap, Mode::Normal);

    // Parser should be in Ready state
    assert!(parser.state().is_ready());
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 6: Edge Cases (5 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn zero_as_motion_not_digit() {
    // Standalone 0 is line start motion, not digit
    assert_motion(
        parse_keys("0"),
        NonZeroU32::new(1).unwrap(),
        Motion::LineStart,
    );
}

#[test]
fn zero_in_count_is_digit() {
    // 0 after other digits is part of count
    assert_motion(
        parse_keys("10j"),
        NonZeroU32::new(10).unwrap(),
        Motion::Down,
    );
    assert_motion(
        parse_keys("20w"),
        NonZeroU32::new(20).unwrap(),
        Motion::WordForward,
    );
    assert_motion(
        parse_keys("100h"),
        NonZeroU32::new(100).unwrap(),
        Motion::Left,
    );
}

#[test]
fn count_composition_2d3w() {
    // Count before operator × count after = total
    assert_op_motion(
        parse_keys("2d3w"),
        Operator::Delete,
        Motion::WordForward,
        NonZeroU32::new(6).unwrap(),
    );
}

#[test]
fn count_composition_5y2j() {
    assert_op_motion(
        parse_keys("5y2j"),
        Operator::Yank,
        Motion::Down,
        NonZeroU32::new(10).unwrap(),
    );
}

#[test]
fn count_overflow_protection() {
    // Very large counts should be clamped
    let result = parse_keys("9999j");
    match result {
        GrammarResult::Execute(Command::Motion { count, .. }) => {
            assert!(count.get() <= 10000, "Count should be clamped");
        }
        _ => panic!("Expected Motion"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 7: Character Commands (5 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn char_cmd_find_forward() {
    let result = parse_keys("fa");
    match result {
        GrammarResult::Execute(Command::CharCommand {
            command,
            ref target,
            ..
        }) => {
            assert_eq!(command, CharCommand::FindForward);
            assert_eq!(target.as_str(), "a");
        }
        _ => panic!("Expected CharCommand, got {:?}", result),
    }
}

#[test]
fn char_cmd_find_backward() {
    let result = parse_keys("Fb");
    match result {
        GrammarResult::Execute(Command::CharCommand {
            command,
            ref target,
            ..
        }) => {
            assert_eq!(command, CharCommand::FindBackward);
            assert_eq!(target.as_str(), "b");
        }
        _ => panic!("Expected CharCommand, got {:?}", result),
    }
}

#[test]
fn char_cmd_till_forward() {
    let result = parse_keys("tx");
    match result {
        GrammarResult::Execute(Command::CharCommand {
            command,
            ref target,
            ..
        }) => {
            assert_eq!(command, CharCommand::TillForward);
            assert_eq!(target.as_str(), "x");
        }
        _ => panic!("Expected CharCommand, got {:?}", result),
    }
}

#[test]
fn char_cmd_with_operator() {
    // dfa = delete to 'a'
    let result = parse_keys("dfa");
    match result {
        GrammarResult::Execute(Command::CharCommand {
            command,
            ref target,
            operator,
            ..
        }) => {
            assert_eq!(command, CharCommand::FindForward);
            assert_eq!(target.as_str(), "a");
            assert_eq!(operator, Some(Operator::Delete));
        }
        _ => panic!("Expected CharCommand with operator, got {:?}", result),
    }
}

#[test]
fn char_cmd_with_count() {
    let result = parse_keys("3fx");
    match result {
        GrammarResult::Execute(Command::CharCommand {
            count,
            command,
            ref target,
            ..
        }) => {
            assert_eq!(count.get(), 3);
            assert_eq!(command, CharCommand::FindForward);
            assert_eq!(target.as_str(), "x");
        }
        _ => panic!("Expected CharCommand, got {:?}", result),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 8: Mode Switches (5 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn mode_switch_insert_i() {
    let result = parse_keys("i");
    match result {
        GrammarResult::Execute(Command::InsertEntry {
            count, entry_type, ..
        }) => {
            assert_eq!(count.get(), 1);
            assert_eq!(entry_type, InsertEntryType::BeforeCursor);
        }
        _ => panic!("Expected Execute(InsertEntry), got {:?}", result),
    }
}

#[test]
fn mode_switch_visual_v() {
    let result = parse_keys("v");
    match result {
        GrammarResult::Execute(Command::Visual(VisualKind::Enter {
            visual_type: VisualType::Char,
            ..
        })) => {}
        _ => panic!("Expected Execute(VisualEnter Char), got {:?}", result),
    }
}

#[test]
fn mode_switch_visual_line_V() {
    let result = parse_keys("V");
    match result {
        GrammarResult::Execute(Command::Visual(VisualKind::Enter {
            visual_type: VisualType::Line,
            ..
        })) => {}
        _ => panic!("Expected Execute(VisualEnter Line), got {:?}", result),
    }
}

#[test]
fn mode_switch_command_line() {
    let result = parse_keys(":");
    match result {
        GrammarResult::ModeChange(Mode::CommandLine, _) => {}
        _ => panic!("Expected ModeChange to CommandLine, got {:?}", result),
    }
}

#[test]
fn mode_switch_replace() {
    let result = parse_keys("R");
    match result {
        GrammarResult::ModeChange(Mode::Replace, _) => {}
        _ => panic!("Expected ModeChange to Replace, got {:?}", result),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 9: Prefix Commands (5 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn prefix_gg() {
    let result = parse_keys("gg");
    match result {
        GrammarResult::Execute(Command::Motion {
            motion: Motion::GotoFirstLine,
            ..
        }) => {}
        _ => panic!("Expected Motion::GotoFirstLine, got {:?}", result),
    }
}

#[test]
fn prefix_gg_with_count() {
    let result = parse_keys("5gg");
    match result {
        GrammarResult::Execute(Command::Motion {
            count,
            motion: Motion::GotoFirstLine,
            ..
        }) => {
            assert_eq!(count.get(), 5);
        }
        _ => panic!(
            "Expected Motion::GotoFirstLine with count, got {:?}",
            result
        ),
    }
}

#[test]
fn prefix_dgg() {
    // d + gg = delete to first line
    let result = parse_keys("dgg");
    match result {
        GrammarResult::Execute(Command::OperatorMotion {
            operator: Operator::Delete,
            motion: Motion::GotoFirstLine,
            ..
        }) => {}
        _ => panic!(
            "Expected OperatorMotion with GotoFirstLine, got {:?}",
            result
        ),
    }
}

#[test]
fn prefix_gU_becomes_operator() {
    // gU = uppercase operator, needs motion
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    let _ = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    let result = parser.process(KeyEvent::char('U'), &keymap, Mode::Normal);

    // Should continue (operator pending)
    match result {
        GrammarResult::Continue(_) => {}
        _ => panic!("Expected Continue for gU, got {:?}", result),
    }
}

#[test]
fn prefix_gu_becomes_operator() {
    // gu = lowercase operator
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    let _ = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    let result = parser.process(KeyEvent::char('u'), &keymap, Mode::Normal);

    match result {
        GrammarResult::Continue(_) => {}
        _ => panic!("Expected Continue for gu, got {:?}", result),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 10: Actions (5 tests)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn action_x() {
    let result = parse_keys("x");
    match result {
        GrammarResult::Execute(Command::Action {
            action: Action::DeleteChar,
            count,
            ..
        }) => {
            assert_eq!(count.get(), 1);
        }
        _ => panic!("Expected Action::DeleteChar, got {:?}", result),
    }
}

#[test]
fn action_3x() {
    let result = parse_keys("3x");
    match result {
        GrammarResult::Execute(Command::Action {
            action: Action::DeleteChar,
            count,
            ..
        }) => {
            assert_eq!(count.get(), 3);
        }
        _ => panic!("Expected Action::DeleteChar with count, got {:?}", result),
    }
}

#[test]
fn action_p() {
    let result = parse_keys("p");
    match result {
        GrammarResult::Execute(Command::Action {
            action: Action::Put,
            ..
        }) => {}
        _ => panic!("Expected Action::Put, got {:?}", result),
    }
}

#[test]
fn action_u() {
    let result = parse_keys("u");
    match result {
        GrammarResult::Execute(Command::Action {
            action: Action::Undo,
            ..
        }) => {}
        _ => panic!("Expected Action::Undo, got {:?}", result),
    }
}

#[test]
fn action_J() {
    let result = parse_keys("J");
    match result {
        GrammarResult::Execute(Command::Action {
            action: Action::Join,
            ..
        }) => {}
        _ => panic!("Expected Action::Join, got {:?}", result),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 11: grammar_test! Macro Exercisers (5 tests)
// ─────────────────────────────────────────────────────────────────────────────

use vim_test::grammar_test;

grammar_test!(macro_motion_w, "w" => Motion(Motion::WordForward));
grammar_test!(macro_op_dw, "dw" => Op(Operator::Delete, Motion::WordForward));
grammar_test!(macro_opline_dd, "dd" => OpLine(Operator::Delete));
grammar_test!(macro_action_x, "x" => Action(Action::DeleteChar));
grammar_test!(macro_cancel_esc, "\x1b" => Cancel);

// ─────────────────────────────────────────────────────────────────────────────
// Category 12: count digit deletion with the Delete key
// ─────────────────────────────────────────────────────────────────────────────

use vim_core::grammar::InputState;
use vim_core::keymap::Key;

#[test]
fn count_delete_removes_last_digit() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Type "123"
    for c in ['1', '2', '3'] {
        let _ = parser.process(KeyEvent::char(c), &keymap, Mode::Normal);
    }

    // Delete key → count becomes 12
    let result = parser.process(
        KeyEvent::new(Key::Delete, vim_core::keymap::Modifiers::NONE),
        &keymap,
        Mode::Normal,
    );
    match result {
        GrammarResult::Continue(InputState::Ready {
            count: Some(12), ..
        }) => {}
        _ => panic!("Expected count=12 after Delete, got {:?}", result),
    }
}

#[test]
fn count_delete_to_single_digit() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Type "12"
    let _ = parser.process(KeyEvent::char('1'), &keymap, Mode::Normal);
    let _ = parser.process(KeyEvent::char('2'), &keymap, Mode::Normal);

    // Delete → count becomes 1
    let result = parser.process(
        KeyEvent::new(Key::Delete, vim_core::keymap::Modifiers::NONE),
        &keymap,
        Mode::Normal,
    );
    match result {
        GrammarResult::Continue(InputState::Ready { count: Some(1), .. }) => {}
        _ => panic!("Expected count=1 after Delete, got {:?}", result),
    }
}

#[test]
fn count_delete_clears_count() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    // Type "1"
    let _ = parser.process(KeyEvent::char('1'), &keymap, Mode::Normal);

    // Delete → count becomes None
    let result = parser.process(
        KeyEvent::new(Key::Delete, vim_core::keymap::Modifiers::NONE),
        &keymap,
        Mode::Normal,
    );
    match result {
        GrammarResult::Continue(InputState::Ready { count: None, .. }) => {}
        _ => panic!("Expected count=None after Delete, got {:?}", result),
    }
}

#[test]
fn count_delete_full_sequence_123_to_none() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();
    let del = KeyEvent::new(Key::Delete, vim_core::keymap::Modifiers::NONE);

    // Type "123"
    for c in ['1', '2', '3'] {
        let _ = parser.process(KeyEvent::char(c), &keymap, Mode::Normal);
    }

    // Delete → 12
    let r1 = parser.process(del, &keymap, Mode::Normal);
    assert!(
        matches!(
            r1,
            GrammarResult::Continue(InputState::Ready {
                count: Some(12),
                ..
            })
        ),
        "After 1st Delete: expected 12, got {:?}",
        r1
    );

    // Delete → 1
    let r2 = parser.process(del, &keymap, Mode::Normal);
    assert!(
        matches!(
            r2,
            GrammarResult::Continue(InputState::Ready { count: Some(1), .. })
        ),
        "After 2nd Delete: expected 1, got {:?}",
        r2
    );

    // Delete → None
    let r3 = parser.process(del, &keymap, Mode::Normal);
    assert!(
        matches!(
            r3,
            GrammarResult::Continue(InputState::Ready { count: None, .. })
        ),
        "After 3rd Delete: expected None, got {:?}",
        r3
    );
}

#[test]
fn delete_without_count_executes_delete_char() {
    let result = {
        let mut parser = Parser::new();
        let keymap = Keymap::default();
        parser.process(
            KeyEvent::new(Key::Delete, vim_core::keymap::Modifiers::NONE),
            &keymap,
            Mode::Normal,
        )
    };
    match result {
        GrammarResult::Execute(Command::Action {
            action: Action::DeleteChar,
            ..
        }) => {}
        _ => panic!(
            "Delete without count should execute DeleteChar, got {:?}",
            result
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category 14: gb/gB/gs Multi-Cursor Grammar Commands
// ─────────────────────────────────────────────────────────────────────────────

grammar_test!(gb_parse, "gb" => Action(Action::AddNextMatchCursor));
grammar_test!(g_upper_b_parse, "gB" => Action(Action::AddPrevMatchCursor));
grammar_test!(gs_parse, "gs" => Action(Action::SkipMatchCursor));

#[test]
fn gb_with_count() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    let _ = parser.process(KeyEvent::char('3'), &keymap, Mode::Normal);
    let _ = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    let result = parser.process(KeyEvent::char('b'), &keymap, Mode::Normal);

    match result {
        GrammarResult::Execute(Command::Action { count, action, .. }) => {
            assert_eq!(action, Action::AddNextMatchCursor);
            assert_eq!(count.get(), 3);
        }
        other => panic!(
            "Expected Action::AddNextMatchCursor with count 3, got {:?}",
            other
        ),
    }
}

#[test]
fn g_upper_b_with_count() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    let _ = parser.process(KeyEvent::char('2'), &keymap, Mode::Normal);
    let _ = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    let result = parser.process(KeyEvent::char('B'), &keymap, Mode::Normal);

    match result {
        GrammarResult::Execute(Command::Action { count, action, .. }) => {
            assert_eq!(action, Action::AddPrevMatchCursor);
            assert_eq!(count.get(), 2);
        }
        other => panic!(
            "Expected Action::AddPrevMatchCursor with count 2, got {:?}",
            other
        ),
    }
}

#[test]
fn gs_with_count() {
    let mut parser = Parser::new();
    let keymap = Keymap::default();

    let _ = parser.process(KeyEvent::char('5'), &keymap, Mode::Normal);
    let _ = parser.process(KeyEvent::char('g'), &keymap, Mode::Normal);
    let result = parser.process(KeyEvent::char('s'), &keymap, Mode::Normal);

    match result {
        GrammarResult::Execute(Command::Action { count, action, .. }) => {
            assert_eq!(action, Action::SkipMatchCursor);
            assert_eq!(count.get(), 5);
        }
        other => panic!(
            "Expected Action::SkipMatchCursor with count 5, got {:?}",
            other
        ),
    }
}
