//! Integration tests for the variable store (g: and b: scopes).
//!
//! Tests both the direct `VariableStore` CRUD and the `VimApi` + `EffectEmitter`
//! round-trip through effects.

use vim_core::effects::Effect;
use vim_core::execution::{HostSession, InvocationContext, VimApi};
use vim_core::primitives::{CallerId, CapabilityTier, VarScope, VimValue};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn host_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Host, CapabilityTier::Mutating)
}

fn readonly_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Expression, CapabilityTier::ReadOnly)
}

// ===========================================================================
// 1. VimApi::variables() — read-only view
// ===========================================================================

#[test]
fn variable_view_initially_empty() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());
    let vars = api.variables();
    assert!(!vars.exists(VarScope::Global, "foo"));
    assert!(!vars.exists(VarScope::Buffer, "bar"));
    assert!(vars.get_global("any").is_none());
    assert!(vars.get_buffer("any").is_none());
}

// ===========================================================================
// 2. EffectEmitter::set_variable — produces correct effect
// ===========================================================================

#[test]
fn emitter_set_variable_produces_effect() {
    let session = HostSession::new("text");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit()
        .set_variable(VarScope::Global, "count", VimValue::Int(42))
        .unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::SetVariable { scope, name, value } => {
            assert_eq!(*scope, VarScope::Global);
            assert_eq!(name.as_str(), "count");
            assert_eq!(*value, VimValue::Int(42));
        }
        other => panic!("expected SetVariable, got {other:?}"),
    }
}

#[test]
fn emitter_delete_variable_produces_effect() {
    let session = HostSession::new("text");
    let api = VimApi::from_session(&session, host_ctx());

    api.emit()
        .delete_variable(VarScope::Buffer, "temp")
        .unwrap();

    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::DeleteVariable { scope, name } => {
            assert_eq!(*scope, VarScope::Buffer);
            assert_eq!(name.as_str(), "temp");
        }
        other => panic!("expected DeleteVariable, got {other:?}"),
    }
}

// ===========================================================================
// 3. Capability tier enforcement
// ===========================================================================

#[test]
fn set_variable_requires_mutating_tier() {
    let session = HostSession::new("text");
    let api = VimApi::from_session(&session, readonly_ctx());

    let result = api
        .emit()
        .set_variable(VarScope::Global, "x", VimValue::Bool(true));
    assert!(result.is_err());
}

#[test]
fn delete_variable_requires_mutating_tier() {
    let session = HostSession::new("text");
    let api = VimApi::from_session(&session, readonly_ctx());

    let result = api.emit().delete_variable(VarScope::Buffer, "x");
    assert!(result.is_err());
}

// ===========================================================================
// 4. Effect kind classification
// ===========================================================================

#[test]
fn set_variable_effect_kind() {
    use vim_core::effects::{EffectKind, EffectTier};
    assert_eq!(EffectKind::SetVariable.tier(), EffectTier::Internal);
    assert_eq!(EffectKind::DeleteVariable.tier(), EffectTier::Internal);
}

// ===========================================================================
// 5. Effect round-trip through engine (using VimEngine::apply_effect)
// ===========================================================================

#[test]
fn variable_set_and_get_global_via_engine() {
    use compact_str::CompactString;
    use vim_core::execution::VimEngine;

    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("myvar"),
        value: VimValue::Int(123),
    });

    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Global, "myvar"),
        Some(&VimValue::Int(123))
    );
}

#[test]
fn variable_set_and_get_buffer_via_engine() {
    use compact_str::CompactString;
    use vim_core::execution::VimEngine;

    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("bufvar"),
        value: VimValue::String(CompactString::from("hello")),
    });

    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Buffer, "bufvar"),
        Some(&VimValue::String(CompactString::from("hello")))
    );
}

#[test]
fn variable_scope_isolation_via_engine() {
    use compact_str::CompactString;
    use vim_core::execution::VimEngine;

    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("x"),
        value: VimValue::Int(1),
    });
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("x"),
        value: VimValue::Int(2),
    });

    assert_eq!(
        engine.state().variable_store().get(VarScope::Global, "x"),
        Some(&VimValue::Int(1))
    );
    assert_eq!(
        engine.state().variable_store().get(VarScope::Buffer, "x"),
        Some(&VimValue::Int(2))
    );
}

#[test]
fn variable_delete_via_engine() {
    use compact_str::CompactString;
    use vim_core::execution::VimEngine;

    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("del_me"),
        value: VimValue::Bool(true),
    });
    assert!(engine
        .state()
        .variable_store()
        .exists(VarScope::Global, "del_me"));

    engine.apply_effect(&Effect::DeleteVariable {
        scope: VarScope::Global,
        name: CompactString::from("del_me"),
    });
    assert!(!engine
        .state()
        .variable_store()
        .exists(VarScope::Global, "del_me"));
}

#[test]
fn variable_overwrite_via_engine() {
    use compact_str::CompactString;
    use vim_core::execution::VimEngine;

    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("ow"),
        value: VimValue::Int(1),
    });
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("ow"),
        value: VimValue::Int(99),
    });
    assert_eq!(
        engine.state().variable_store().get(VarScope::Global, "ow"),
        Some(&VimValue::Int(99))
    );
}

// ===========================================================================
// 6. Buffer switch — b: variables saved/restored
// ===========================================================================

#[test]
fn variable_buffer_switch() {
    use compact_str::CompactString;
    use vim_core::execution::{BufferLocalState, VimEngine};

    let mut engine = VimEngine::new();

    // Enter buffer A, set b:foo
    engine.on_buffer_enter(BufferLocalState::default());
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("foo"),
        value: VimValue::Int(10),
    });
    assert_eq!(
        engine.state().variable_store().get(VarScope::Buffer, "foo"),
        Some(&VimValue::Int(10))
    );

    // Leave buffer A
    let saved_a = engine.on_buffer_leave(0);

    // b:foo should be gone from engine state
    assert!(engine
        .state()
        .variable_store()
        .get(VarScope::Buffer, "foo")
        .is_none());

    // Enter buffer B — b:foo not visible
    engine.on_buffer_enter(BufferLocalState::default());
    assert!(engine
        .state()
        .variable_store()
        .get(VarScope::Buffer, "foo")
        .is_none());

    // Leave buffer B, re-enter buffer A
    let _saved_b = engine.on_buffer_leave(0);
    engine.on_buffer_enter(saved_a);

    // b:foo should be restored
    assert_eq!(
        engine.state().variable_store().get(VarScope::Buffer, "foo"),
        Some(&VimValue::Int(10))
    );
}

#[test]
fn variable_global_survives_buffer_switch() {
    use compact_str::CompactString;
    use vim_core::execution::{BufferLocalState, VimEngine};

    let mut engine = VimEngine::new();

    // Set g:keep
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("keep"),
        value: VimValue::Float(3.14),
    });

    // Leave and enter another buffer
    engine.on_buffer_enter(BufferLocalState::default());
    let _saved = engine.on_buffer_leave(0);
    engine.on_buffer_enter(BufferLocalState::default());

    // g:keep should still be there
    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Global, "keep"),
        Some(&VimValue::Float(3.14))
    );
}
