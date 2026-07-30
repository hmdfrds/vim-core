use super::*;
use crate::effects::Effect;
use crate::execution::engine::macro_replay::MacroFrame;
use crate::execution::VimEngine;
use crate::primitives::{Offset, RegisterName};
use std::num::NonZeroU32;

// ═══════════════════════════════════════════════════════════════════════════════
// ShadowAbortReason
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn abort_reason_equality() {
    assert_eq!(
        ShadowAbortReason::HostInteractionRequired,
        ShadowAbortReason::HostInteractionRequired
    );
    assert_eq!(
        ShadowAbortReason::EngineError,
        ShadowAbortReason::EngineError
    );
    assert_ne!(
        ShadowAbortReason::HostInteractionRequired,
        ShadowAbortReason::EngineError
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ShadowStatus
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn status_active_equality() {
    assert_eq!(ShadowStatus::Active, ShadowStatus::Active);
}

#[test]
fn status_completed_equality() {
    assert_eq!(ShadowStatus::Completed, ShadowStatus::Completed);
}

#[test]
fn status_aborted_equality() {
    let a = ShadowStatus::Aborted {
        reason: ShadowAbortReason::HostInteractionRequired,
        keys_processed: 5,
    };
    let b = ShadowStatus::Aborted {
        reason: ShadowAbortReason::HostInteractionRequired,
        keys_processed: 5,
    };
    assert_eq!(a, b);
}

#[test]
fn status_aborted_different_reason() {
    let a = ShadowStatus::Aborted {
        reason: ShadowAbortReason::HostInteractionRequired,
        keys_processed: 5,
    };
    let b = ShadowStatus::Aborted {
        reason: ShadowAbortReason::EngineError,
        keys_processed: 5,
    };
    assert_ne!(a, b);
}

#[test]
fn status_aborted_different_keys_processed() {
    let a = ShadowStatus::Aborted {
        reason: ShadowAbortReason::EngineError,
        keys_processed: 3,
    };
    let b = ShadowStatus::Aborted {
        reason: ShadowAbortReason::EngineError,
        keys_processed: 7,
    };
    assert_ne!(a, b);
}

#[test]
fn status_variants_are_distinct() {
    assert_ne!(ShadowStatus::Active, ShadowStatus::Completed);
    assert_ne!(
        ShadowStatus::Active,
        ShadowStatus::Aborted {
            reason: ShadowAbortReason::EngineError,
            keys_processed: 0,
        }
    );
    assert_ne!(
        ShadowStatus::Completed,
        ShadowStatus::Aborted {
            reason: ShadowAbortReason::HostInteractionRequired,
            keys_processed: 0,
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ShadowContext
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn context_new_defaults() {
    let ctx = ShadowContext::new(42);
    assert_eq!(ctx.cursor(), 42);
    assert_eq!(ctx.selection(), None);
    assert_eq!(ctx.deferred_effect_count(), 0);
    assert_eq!(ctx.keys_processed(), 0);
}

#[test]
fn context_cursor_update() {
    let mut ctx = ShadowContext::new(0);
    assert_eq!(ctx.cursor(), 0);
    ctx.set_cursor(100);
    assert_eq!(ctx.cursor(), 100);
    ctx.set_cursor(50);
    assert_eq!(ctx.cursor(), 50);
}

#[test]
fn context_selection_lifecycle() {
    let mut ctx = ShadowContext::new(0);
    assert_eq!(ctx.selection(), None);

    let sel = SelectionRange::new(Offset::new(5), Offset::new(10));
    ctx.set_selection(sel);
    assert_eq!(ctx.selection(), Some(sel));

    ctx.clear_selection();
    assert_eq!(ctx.selection(), None);
}

#[test]
fn context_selection_update() {
    let mut ctx = ShadowContext::new(0);
    let sel1 = SelectionRange::new(Offset::new(0), Offset::new(5));
    let sel2 = SelectionRange::new(Offset::new(3), Offset::new(15));

    ctx.set_selection(sel1);
    assert_eq!(ctx.selection(), Some(sel1));

    ctx.set_selection(sel2);
    assert_eq!(ctx.selection(), Some(sel2));
}

#[test]
fn context_deferred_effect_accumulation() {
    let mut ctx = ShadowContext::new(0);
    assert_eq!(ctx.deferred_effect_count(), 0);

    ctx.push_deferred_effect(Effect::ClearMessage);
    assert_eq!(ctx.deferred_effect_count(), 1);

    ctx.push_deferred_effect(Effect::CenterCursor);
    assert_eq!(ctx.deferred_effect_count(), 2);

    ctx.push_deferred_effect(Effect::ClearHighlights);
    assert_eq!(ctx.deferred_effect_count(), 3);
}

#[test]
fn context_take_deferred_effects() {
    let mut ctx = ShadowContext::new(0);
    ctx.push_deferred_effect(Effect::ClearMessage);
    ctx.push_deferred_effect(Effect::CenterCursor);

    let effects = ctx.take_deferred_effects();
    assert_eq!(effects.len(), 2);
    assert_eq!(ctx.deferred_effect_count(), 0);
}

#[test]
fn context_take_deferred_effects_preserves_order() {
    let mut ctx = ShadowContext::new(0);
    ctx.push_deferred_effect(Effect::ClearMessage);
    ctx.push_deferred_effect(Effect::CenterCursor);
    ctx.push_deferred_effect(Effect::ClearHighlights);

    let effects = ctx.take_deferred_effects();
    assert!(matches!(effects[0], Effect::ClearMessage));
    assert!(matches!(effects[1], Effect::CenterCursor));
    assert!(matches!(effects[2], Effect::ClearHighlights));
}

#[test]
fn context_keys_processed_tracking() {
    let mut ctx = ShadowContext::new(0);
    assert_eq!(ctx.keys_processed(), 0);

    ctx.increment_keys_processed();
    assert_eq!(ctx.keys_processed(), 1);

    ctx.increment_keys_processed();
    ctx.increment_keys_processed();
    assert_eq!(ctx.keys_processed(), 3);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ShadowResult
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn result_completed_no_changes() {
    let result = ShadowResult {
        text_effects: Vec::new(),
        other_effects: Vec::new(),
        final_cursor: None,
        status: ShadowStatus::Completed,
    };
    assert!(result.text_effects.is_empty());
    assert!(result.other_effects.is_empty());
    assert_eq!(result.final_cursor, None);
    assert_eq!(result.status, ShadowStatus::Completed);
}

#[test]
fn result_completed_with_text_effect() {
    let text_effect = Effect::insert(Offset::new(0), "hello");
    let result = ShadowResult {
        text_effects: vec![text_effect],
        other_effects: Vec::new(),
        final_cursor: Some(5),
        status: ShadowStatus::Completed,
    };
    assert_eq!(result.text_effects.len(), 1);
    assert_eq!(result.final_cursor, Some(5));
    assert_eq!(result.status, ShadowStatus::Completed);
}

#[test]
fn result_completed_with_other_effects() {
    let result = ShadowResult {
        text_effects: Vec::new(),
        other_effects: vec![Effect::ClearMessage, Effect::CenterCursor],
        final_cursor: Some(10),
        status: ShadowStatus::Completed,
    };
    assert_eq!(result.other_effects.len(), 2);
    assert_eq!(result.final_cursor, Some(10));
}

#[test]
fn result_aborted_host_interaction() {
    let result = ShadowResult {
        text_effects: Vec::new(),
        other_effects: Vec::new(),
        final_cursor: None,
        status: ShadowStatus::Aborted {
            reason: ShadowAbortReason::HostInteractionRequired,
            keys_processed: 3,
        },
    };
    assert_eq!(
        result.status,
        ShadowStatus::Aborted {
            reason: ShadowAbortReason::HostInteractionRequired,
            keys_processed: 3,
        }
    );
}

#[test]
fn result_aborted_engine_error() {
    let result = ShadowResult {
        text_effects: Vec::new(),
        other_effects: Vec::new(),
        final_cursor: None,
        status: ShadowStatus::Aborted {
            reason: ShadowAbortReason::EngineError,
            keys_processed: 0,
        },
    };
    assert_eq!(
        result.status,
        ShadowStatus::Aborted {
            reason: ShadowAbortReason::EngineError,
            keys_processed: 0,
        }
    );
}

#[test]
fn result_clone() {
    let result = ShadowResult {
        text_effects: vec![Effect::insert(Offset::new(0), "x")],
        other_effects: vec![Effect::ClearMessage],
        final_cursor: Some(1),
        status: ShadowStatus::Completed,
    };
    let cloned = result.clone();
    assert_eq!(cloned.text_effects.len(), 1);
    assert_eq!(cloned.other_effects.len(), 1);
    assert_eq!(cloned.final_cursor, Some(1));
    assert_eq!(cloned.status, ShadowStatus::Completed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper: push macro frames for shadow replay tests
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a key string and push it as a macro frame onto the engine's macro stack.
fn push_macro_keys(engine: &mut VimEngine, keys: &str) {
    let parsed = crate::execution::engine::macro_replay::parse_macro_entries(keys);
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: parsed,
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });
    let _ = engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('q').unwrap());
}

// ═══════════════════════════════════════════════════════════════════════════════
// execute_shadow_replay integration tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn shadow_replay_empty_macro_stack() {
    let mut engine = VimEngine::new();

    // No macro keys pushed — should return Completed immediately with no cursor
    // (no-op shadow must not emit a cursor that could override the real key's cursor).
    let result = engine.execute_shadow_replay("hello world".to_owned(), 0, None, None);
    assert_eq!(result.status, ShadowStatus::Completed);
    assert!(result.text_effects.is_empty());
    assert_eq!(result.final_cursor, None);
    assert!(!engine.fork_active);
}

#[test]
fn shadow_replay_x_deletes_char() {
    let mut engine = VimEngine::new();
    push_macro_keys(&mut engine, "x");

    let result = engine.execute_shadow_replay("hello".to_owned(), 0, None, None);
    assert_eq!(result.status, ShadowStatus::Completed);
    // `x` on 'h' should delete 'h', leaving "ello"
    assert_eq!(result.text_effects.len(), 1);
    assert!(!engine.fork_active);
}

#[test]
fn shadow_replay_j_no_text_change() {
    let mut engine = VimEngine::new();
    push_macro_keys(&mut engine, "j");

    let result = engine.execute_shadow_replay("hello\nworld".to_owned(), 0, None, None);
    assert_eq!(result.status, ShadowStatus::Completed);
    // `j` only moves cursor, no text change
    assert!(result.text_effects.is_empty());
    assert!(result.final_cursor.is_some());
    assert!(!engine.fork_active);
}

#[test]
fn shadow_replay_clears_fork_active_on_completion() {
    let mut engine = VimEngine::new();
    push_macro_keys(&mut engine, "l");

    assert!(!engine.fork_active);
    let _result = engine.execute_shadow_replay("hello".to_owned(), 0, None, None);
    assert!(!engine.fork_active);
}

#[test]
fn shadow_replay_multiple_keys() {
    let mut engine = VimEngine::new();
    // Two x commands should delete two characters
    push_macro_keys(&mut engine, "xx");

    let result = engine.execute_shadow_replay("hello".to_owned(), 0, None, None);
    assert_eq!(result.status, ShadowStatus::Completed);
    // Both 'h' and 'e' should be deleted, producing a single diff effect
    assert_eq!(result.text_effects.len(), 1);
}

#[test]
fn shadow_replay_dd_deletes_line() {
    let mut engine = VimEngine::new();
    push_macro_keys(&mut engine, "dd");

    let result = engine.execute_shadow_replay("hello\nworld\n".to_owned(), 0, None, None);
    assert_eq!(result.status, ShadowStatus::Completed);
    // dd should produce a text diff effect
    assert_eq!(result.text_effects.len(), 1);
}

#[test]
fn shadow_replay_with_initial_selection() {
    let mut engine = VimEngine::new();
    push_macro_keys(&mut engine, "l");

    let sel = SelectionRange::new(Offset::new(0), Offset::new(3));

    let result = engine.execute_shadow_replay("hello world".to_owned(), 0, Some(sel), None);
    assert_eq!(result.status, ShadowStatus::Completed);
}
