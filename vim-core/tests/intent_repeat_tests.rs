//! Tests for intent repeat (`g.`) functionality.
//!
//! Verifies that:
//! 1. Intent is captured for operator+motion commands
//! 2. Intent is captured for operator+textobject commands
//! 3. Intent is NOT captured for pure motions
//! 4. Intent is cleared on repeat state clear
//! 5. g. parser action is recognized
//! 6. g. with no prior intent is a no-op

mod common;

use std::num::NonZeroU32;

use common::document::TestDocument;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::grammar::types::{Action, Motion, Operator, TextObjectKind, TextObjectScope};
use vim_core::grammar::{Command, GrammarResult, Parser};
use vim_core::keymap::{KeyEvent, Keymap};
use vim_core::primitives::Mode;
use vim_core::state::{capture_intent, CommandIntent};

const N1: NonZeroU32 = match NonZeroU32::new(1) {
    Some(v) => v,
    None => unreachable!(),
};
const N2: NonZeroU32 = match NonZeroU32::new(2) {
    Some(v) => v,
    None => unreachable!(),
};
const N3: NonZeroU32 = match NonZeroU32::new(3) {
    Some(v) => v,
    None => unreachable!(),
};

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

/// Helper to run a keystroke sequence through the engine and extract state.
#[allow(dead_code)]
fn run_keystrokes(keys: &str) -> VimEngine {
    let doc = TestDocument::new("hello world\nfoo bar\nbaz", (0, 0));
    let mut engine = VimEngine::new();

    let key_events = parse_key_notation(keys);
    for key in key_events {
        let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
        let _response = engine.process(key, ctx);
    }

    engine
}

/// Parse key notation string to KeyEvents (simplified version).
#[allow(dead_code)]
fn parse_key_notation(keys: &str) -> Vec<KeyEvent> {
    let mut result = Vec::new();
    let mut chars = keys.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            // Simple special key parsing
            let mut special = String::new();
            let mut found_close = false;

            while let Some(&nc) = chars.peek() {
                if nc == '>' {
                    chars.next();
                    found_close = true;
                    break;
                }
                special.push(chars.next().unwrap());
            }

            if found_close {
                match special.as_str() {
                    "Esc" | "Escape" => result.push(KeyEvent::escape()),
                    "CR" | "Enter" => result.push(KeyEvent::enter()),
                    _ => result.push(KeyEvent::char('<')),
                }
            } else {
                result.push(KeyEvent::char('<'));
            }
        } else {
            result.push(KeyEvent::char(c));
        }
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn intent_is_captured_for_operator_motion() {
    // Parse "dw" to get the OperatorMotion command
    let result = parse_keys("dw");

    match result {
        GrammarResult::Execute(cmd @ Command::OperatorMotion { .. }) => {
            // Capture the intent from the command
            let intent = capture_intent(&cmd, None);
            assert!(
                intent.is_some(),
                "Intent should be captured for operator+motion"
            );

            if let Some(CommandIntent::OperatorMotion {
                operator, motion, ..
            }) = intent
            {
                assert_eq!(
                    operator,
                    Operator::Delete,
                    "Operator should be Delete for 'dw'"
                );
                assert_eq!(
                    motion,
                    Motion::WordForward,
                    "Motion should be WordForward for 'dw'"
                );
            } else {
                panic!("Expected OperatorMotion intent");
            }
        }
        other => panic!("Expected Execute with OperatorMotion, got {:?}", other),
    }
}

#[test]
fn intent_is_captured_for_operator_textobject() {
    // Parse "diw" to get the OperatorTextObject command
    let result = parse_keys("diw");

    match result {
        GrammarResult::Execute(cmd @ Command::OperatorTextObject { .. }) => {
            // Capture the intent from the command
            let intent = capture_intent(&cmd, None);
            assert!(
                intent.is_some(),
                "Intent should be captured for operator+textobject"
            );

            if let Some(CommandIntent::OperatorTextObject {
                operator,
                text_object,
                ..
            }) = intent
            {
                assert_eq!(
                    operator,
                    Operator::Delete,
                    "Operator should be Delete for 'diw'"
                );
                assert_eq!(
                    text_object.scope,
                    TextObjectScope::Inner,
                    "Scope should be Inner for 'iw'"
                );
                assert_eq!(
                    text_object.kind,
                    TextObjectKind::Word,
                    "Kind should be Word for 'iw'"
                );
            } else {
                panic!("Expected OperatorTextObject intent");
            }
        }
        other => panic!("Expected Execute with OperatorTextObject, got {:?}", other),
    }
}

#[test]
fn intent_is_not_captured_for_pure_motion() {
    // Parse "w" to get a Motion command
    let result = parse_keys("w");

    match result {
        GrammarResult::Execute(cmd @ Command::Motion { .. }) => {
            // Capture the intent from the command - should be None
            let intent = capture_intent(&cmd, None);
            assert!(
                intent.is_none(),
                "Intent should NOT be captured for pure motion"
            );
        }
        other => panic!("Expected Execute with Motion, got {:?}", other),
    }
}

#[test]
fn intent_is_cleared_on_repeat_state_clear() {
    let mut engine = VimEngine::new();

    // Create a dummy intent
    let intent = CommandIntent::OperatorMotion {
        operator: Operator::Delete,
        motion: Motion::WordForward,
        count: Some(N1),
        inserted_text: None,
        register: None,
    };

    // Save the intent
    engine.repeat_state_mut().save_intent(intent.clone());

    // Verify it's saved
    assert!(
        engine.state().repeat_state().last_intent().is_some(),
        "Intent should be saved"
    );

    // Clear the repeat state
    engine.repeat_state_mut().clear();

    // Verify it's cleared
    assert!(
        engine.state().repeat_state().last_intent().is_none(),
        "Intent should be cleared after clear()"
    );
}

#[test]
fn intent_repeat_action_is_recognized_by_parser() {
    // Parse "g." to get the IntentRepeat action
    let result = parse_keys("g.");

    match result {
        GrammarResult::Execute(Command::Action { action, .. }) => {
            assert_eq!(
                action,
                Action::IntentRepeat,
                "Parser should recognize g. as IntentRepeat action"
            );
        }
        other => panic!(
            "Expected Execute with Action::IntentRepeat, got {:?}",
            other
        ),
    }
}

#[test]
fn g_dot_with_no_prior_intent_is_no_op() {
    let doc = TestDocument::new("hello world", (0, 0));
    let mut engine = VimEngine::new();

    // Fresh engine has no intent
    assert!(
        engine.state().repeat_state().last_intent().is_none(),
        "Fresh engine should have no intent"
    );

    // Process "g." keystroke sequence
    let key_events = vec![KeyEvent::char('g'), KeyEvent::char('.')];
    for key in key_events {
        let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
        let _response = engine.process(key, ctx);
    }

    // Engine should still be in normal mode with no crash
    assert_eq!(engine.mode(), Mode::Normal, "Should remain in normal mode");

    // No intent should be captured from the g. command itself
    // (g. reads intent, doesn't create one)
    assert!(
        engine.state().repeat_state().last_intent().is_none(),
        "g. should not create a new intent"
    );
}

#[test]
fn intent_preserved_through_engine_operations() {
    // Create a fresh engine
    let mut engine = VimEngine::new();

    // Parse "dw" command
    let result = parse_keys("dw");
    let cmd = match result {
        GrammarResult::Execute(cmd) => cmd,
        other => panic!("Expected Execute, got {:?}", other),
    };

    // Capture and save the intent
    if let Some(intent) = capture_intent(&cmd, None) {
        engine.repeat_state_mut().save_intent(intent.clone());

        // Verify intent is saved
        let saved = engine.state().repeat_state().last_intent();
        assert!(saved.is_some(), "Intent should be saved");

        // Verify the saved intent matches what we saved
        if let Some(saved_intent) = saved {
            assert_eq!(*saved_intent, intent, "Saved intent should match");
        }
    } else {
        panic!("Failed to capture intent from 'dw' command");
    }
}

#[test]
fn operator_motion_intent_has_correct_fields() {
    let result = parse_keys("3dw");

    match result {
        GrammarResult::Execute(cmd @ Command::OperatorMotion { .. }) => {
            let intent = capture_intent(&cmd, None).expect("Should capture intent");

            match intent {
                CommandIntent::OperatorMotion {
                    operator,
                    motion,
                    count,
                    inserted_text,
                    register,
                } => {
                    assert_eq!(operator, Operator::Delete);
                    assert_eq!(motion, Motion::WordForward);
                    assert_eq!(count, Some(N3));
                    assert!(inserted_text.is_none()); // Not set until insert mode exits
                    assert!(register.is_none()); // No register for this command
                }
                _ => panic!("Expected OperatorMotion intent"),
            }
        }
        other => panic!("Expected OperatorMotion command, got {:?}", other),
    }
}

#[test]
fn operator_textobject_intent_has_correct_fields() {
    let result = parse_keys("2diw");

    match result {
        GrammarResult::Execute(cmd @ Command::OperatorTextObject { .. }) => {
            let intent = capture_intent(&cmd, None).expect("Should capture intent");

            match intent {
                CommandIntent::OperatorTextObject {
                    operator,
                    text_object,
                    count,
                    inserted_text,
                    register,
                } => {
                    assert_eq!(operator, Operator::Delete);
                    assert_eq!(text_object.scope, TextObjectScope::Inner);
                    assert_eq!(text_object.kind, TextObjectKind::Word);
                    assert_eq!(count, Some(N2));
                    assert!(inserted_text.is_none());
                    assert!(register.is_none());
                }
                _ => panic!("Expected OperatorTextObject intent"),
            }
        }
        other => panic!("Expected OperatorTextObject command, got {:?}", other),
    }
}

#[test]
fn repeat_state_mut_provides_mutable_access() {
    let mut engine = VimEngine::new();

    // Verify we can access repeat_state_mut() and modify it
    let intent = CommandIntent::OperatorMotion {
        operator: Operator::Yank,
        motion: Motion::LineEnd,
        count: Some(N1),
        inserted_text: None,
        register: None,
    };

    // Get mutable access through repeat_state_mut()
    engine.repeat_state_mut().save_intent(intent);

    // Verify it was saved
    assert!(
        engine.state().repeat_state().last_intent().is_some(),
        "Intent should be saved via repeat_state_mut()"
    );
}

#[test]
fn intent_display_is_readable() {
    let intent = CommandIntent::OperatorMotion {
        operator: Operator::Delete,
        motion: Motion::WordForward,
        count: Some(N1),
        inserted_text: None,
        register: None,
    };

    let display = format!("{}", intent);
    assert!(
        display.contains("Delete"),
        "Display should contain operator"
    );
    assert!(
        display.contains("WordForward"),
        "Display should contain motion"
    );
}
