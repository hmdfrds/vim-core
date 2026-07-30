use super::*;
use crate::grammar::{Command, GrammarResult, InsertKind, Operator, Parser};
use crate::keymap::{KeyEvent, Keymap};
use crate::mode::command_line::CommandLineAction;
use crate::primitives::{Mode, VisualType};
use crate::state::{CommandLinePrompt, VimState};

// ─── Helper ────────────────────────────────────────────────────────

/// Create a fresh (state, keymap, parser) triple for testing.
fn fresh() -> (VimState, Keymap, Parser) {
    (VimState::default(), Keymap::default(), Parser::new())
}

// ═══════════════════════════════════════════════════════════════════
// Structural invariants
// ═══════════════════════════════════════════════════════════════════

#[test]
fn dispatcher_is_zero_sized() {
    assert_eq!(std::mem::size_of::<ModeDispatcher>(), 0);
}

#[test]
fn all_handlers_are_zero_sized() {
    assert_eq!(std::mem::size_of::<NormalModeHandler>(), 0);
    assert_eq!(std::mem::size_of::<VisualModeHandler>(), 0);
    assert_eq!(std::mem::size_of::<SelectModeHandler>(), 0);
    assert_eq!(std::mem::size_of::<OperatorPendingModeHandler>(), 0);
    assert_eq!(std::mem::size_of::<InsertModeHandler>(), 0);
    assert_eq!(std::mem::size_of::<ReplaceModeHandler>(), 0);
    assert_eq!(std::mem::size_of::<CommandLineModeHandler>(), 0);
}

#[test]
fn dispatcher_has_all_seven_handlers() {
    let d = ModeDispatcher::new();
    let _ = d.normal;
    let _ = d.visual;
    let _ = d.select;
    let _ = d.operator_pending;
    let _ = d.insert;
    let _ = d.replace;
    let _ = d.command_line;
}

// ═══════════════════════════════════════════════════════════════════
// ModeContext
// ═══════════════════════════════════════════════════════════════════

#[test]
fn mode_context_read_access() {
    let (mut state, keymap, mut parser) = fresh();
    let ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    assert_eq!(ctx.state().mode(), Mode::Normal);
    let _ = ctx.keymap();
}

#[test]
fn mode_context_mutable_state() {
    let (mut state, keymap, mut parser) = fresh();
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    ctx.state_mut().set_mode(Mode::Insert);
    assert_eq!(ctx.state().mode(), Mode::Insert);
}

#[test]
fn mode_context_split_borrow_compiles() {
    let (mut state, keymap, mut parser) = fresh();
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let (parser, keymap) = ctx.parser_and_keymap();
    let _ = parser;
    let _ = keymap;
}

// ═══════════════════════════════════════════════════════════════════
// CommandLineAction → ModeAction conversion
// ═══════════════════════════════════════════════════════════════════

#[test]
fn cl_ignore_becomes_pending() {
    let action: ModeAction = CommandLineAction::Ignore.into();
    assert!(matches!(action, ModeAction::Pending));
}

#[test]
fn cl_cancel_becomes_command_line_cancel() {
    let action: ModeAction = CommandLineAction::Cancel.into();
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::Cancel)
    ));
}

#[test]
fn cl_commit_becomes_command_line_commit() {
    let action: ModeAction = CommandLineAction::Commit.into();
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::Commit)
    ));
}

#[test]
fn cl_open_command_window_becomes_command_line_open_command_window() {
    let action: ModeAction = CommandLineAction::OpenCommandWindow.into();
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::OpenCommandWindow)
    ));
}

// ═══════════════════════════════════════════════════════════════════
// Full dispatch matrix — every mode routes to its own handler
// ═══════════════════════════════════════════════════════════════════

#[test]
fn dispatch_normal_returns_pipeline() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::Normal, KeyEvent::char('j'), &mut ctx);
    assert!(matches!(action, ModeAction::Pipeline(_)));
}

#[test]
fn dispatch_visual_char_returns_pipeline() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Visual(VisualType::Char));
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(
        Mode::Visual(VisualType::Char),
        KeyEvent::char('d'),
        &mut ctx,
    );
    assert!(matches!(action, ModeAction::Pipeline(_)));
}

#[test]
fn dispatch_visual_line_returns_pipeline() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Visual(VisualType::Line));
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(
        Mode::Visual(VisualType::Line),
        KeyEvent::char('y'),
        &mut ctx,
    );
    assert!(matches!(action, ModeAction::Pipeline(_)));
}

#[test]
fn dispatch_visual_block_returns_pipeline() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Visual(VisualType::Block));
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(
        Mode::Visual(VisualType::Block),
        KeyEvent::char('c'),
        &mut ctx,
    );
    assert!(matches!(action, ModeAction::Pipeline(_)));
}

#[test]
fn dispatch_operator_pending_returns_pipeline() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::OperatorPending(Operator::Delete));
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(
        Mode::OperatorPending(Operator::Delete),
        KeyEvent::char('w'),
        &mut ctx,
    );
    assert!(matches!(action, ModeAction::Pipeline(_)));
}

#[test]
fn dispatch_insert_char_returns_insert_command() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Insert);
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::Insert, KeyEvent::char('a'), &mut ctx);
    match action {
        ModeAction::InsertCommand {
            command: Command::Insert(InsertKind::Char { char: 'a' }),
            insert_mode: InsertMode::Insert,
        } => {}
        other => panic!("Expected InsertChar('a', Insert), got {:?}", other),
    }
}

#[test]
fn dispatch_replace_char_returns_insert_command_with_replace() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Replace);
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::Replace, KeyEvent::char('z'), &mut ctx);
    match action {
        ModeAction::InsertCommand {
            command: Command::Insert(InsertKind::Char { char: 'z' }),
            insert_mode: InsertMode::Replace,
        } => {}
        other => panic!("Expected InsertChar('z', Replace), got {:?}", other),
    }
}

#[test]
fn dispatch_command_line_typing_returns_pending() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::CommandLine);
    state.command_line_mut().begin(CommandLinePrompt::Ex);
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::CommandLine, KeyEvent::char('w'), &mut ctx);
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::Edit(_))
    ));
}

// ═══════════════════════════════════════════════════════════════════
// Escape handling — every relevant mode
// ═══════════════════════════════════════════════════════════════════

#[test]
fn escape_in_insert_returns_exit() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Insert);
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::Insert, KeyEvent::escape(), &mut ctx);
    assert!(matches!(action, ModeAction::InsertExit));
}

#[test]
fn escape_in_replace_returns_exit() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Replace);
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::Replace, KeyEvent::escape(), &mut ctx);
    assert!(matches!(action, ModeAction::InsertExit));
}

#[test]
fn escape_in_command_line_returns_cancel() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::CommandLine);
    state.command_line_mut().begin(CommandLinePrompt::Ex);
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::CommandLine, KeyEvent::escape(), &mut ctx);
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::Cancel)
    ));
}

#[test]
fn escape_in_normal_returns_pipeline() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::Normal, KeyEvent::escape(), &mut ctx);
    // In Normal mode, Escape goes through grammar (may be Invalid/Cancel)
    assert!(matches!(action, ModeAction::Pipeline(_)));
}

#[test]
fn escape_in_operator_pending_returns_pipeline() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::OperatorPending(Operator::Delete));
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(
        Mode::OperatorPending(Operator::Delete),
        KeyEvent::escape(),
        &mut ctx,
    );
    // Escape cancels operator via grammar → GrammarResult::Cancel
    assert!(matches!(action, ModeAction::Pipeline(_)));
}

// ═══════════════════════════════════════════════════════════════════
// Insert/Replace flag guarantees
// ═══════════════════════════════════════════════════════════════════

#[test]
fn insert_handler_never_sets_is_replace() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Insert);

    // Test multiple keys
    for ch in ['a', 'b', 'c', '1', '!'] {
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
        let action = d.dispatch(Mode::Insert, KeyEvent::char(ch), &mut ctx);
        match action {
            ModeAction::InsertCommand {
                insert_mode: InsertMode::Insert,
                ..
            } => {}
            ModeAction::InsertExit | ModeAction::Pending | ModeAction::Ignored => {}
            other => panic!(
                "Insert handler returned insert_mode=Replace for '{}': {:?}",
                ch, other
            ),
        }
    }
}

#[test]
fn replace_handler_always_sets_is_replace() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Replace);

    for ch in ['x', 'y', 'z', '0', '@'] {
        let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
        let action = d.dispatch(Mode::Replace, KeyEvent::char(ch), &mut ctx);
        match action {
            ModeAction::InsertCommand {
                insert_mode: InsertMode::Replace,
                ..
            } => {}
            ModeAction::InsertExit | ModeAction::Pending | ModeAction::Ignored => {}
            other => panic!(
                "Replace handler returned insert_mode=Insert for '{}': {:?}",
                ch, other
            ),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Multi-key sequence: Normal → OperatorPending
// ═══════════════════════════════════════════════════════════════════

#[test]
fn multi_key_d_in_normal_returns_continue() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);

    // First key 'd' — parser should enter OperatorPending state
    let action = d.dispatch(Mode::Normal, KeyEvent::char('d'), &mut ctx);
    match action {
        ModeAction::Pipeline(GrammarResult::Continue(_)) => {
            // Correct — parser is waiting for the motion/text-object
        }
        ModeAction::Pipeline(GrammarResult::ModeChange(Mode::OperatorPending(_), _)) => {
            // Also correct — grammar signals mode change to OP
        }
        other => panic!(
            "Expected Continue or ModeChange(OP) after 'd', got {:?}",
            other
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Command-line commit flow
// ═══════════════════════════════════════════════════════════════════

#[test]
fn command_line_type_then_enter_returns_commit() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::CommandLine);
    state.command_line_mut().begin(CommandLinePrompt::Ex);

    // Type 'w'
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::CommandLine, KeyEvent::char('w'), &mut ctx);
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::Edit(_))
    ));

    // Press Enter
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::CommandLine, KeyEvent::enter(), &mut ctx);
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::Commit)
    ));
}

#[test]
fn command_line_type_then_escape_returns_cancel() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::CommandLine);
    state.command_line_mut().begin(CommandLinePrompt::Ex);

    // Type 'q'
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::CommandLine, KeyEvent::char('q'), &mut ctx);
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::Edit(_))
    ));

    // Press Escape — cancel
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::CommandLine, KeyEvent::escape(), &mut ctx);
    assert!(matches!(
        action,
        ModeAction::CommandLine(CommandLineResult::Cancel)
    ));
}

// ═══════════════════════════════════════════════════════════════════
// Backspace routing per mode
// ═══════════════════════════════════════════════════════════════════

#[test]
fn backspace_in_insert_returns_insert_backspace() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Insert);
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::Insert, KeyEvent::backspace(), &mut ctx);
    match action {
        ModeAction::InsertCommand {
            command: Command::Insert(InsertKind::Backspace),
            insert_mode: InsertMode::Insert,
        } => {}
        other => panic!("Expected InsertBackspace(Insert), got {:?}", other),
    }
}

#[test]
fn backspace_in_replace_returns_replace_backspace() {
    let d = ModeDispatcher::new();
    let (mut state, keymap, mut parser) = fresh();
    state.set_mode(Mode::Replace);
    let mut ctx = ModeContext::new(&mut state, &keymap, &mut parser);
    let action = d.dispatch(Mode::Replace, KeyEvent::backspace(), &mut ctx);
    match action {
        ModeAction::InsertCommand {
            command: Command::Insert(InsertKind::Backspace),
            insert_mode: InsertMode::Replace,
        } => {}
        other => panic!("Expected InsertBackspace(Replace), got {:?}", other),
    }
}
