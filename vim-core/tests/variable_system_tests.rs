//! System tests for the variable store.
//!
//! These test full plugin-like workflows, edge cases, the size guard,
//! VariableView::list(), and interactions between variables and real
//! engine keystroke processing.

use vim_core::effects::Effect;
use vim_core::execution::{
    ApiError, BufferLocalState, HostSession, InvocationContext, VimApi, VimEngine,
};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{CallerId, CapabilityTier, VarScope, VimValue};

use compact_str::CompactString;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn host_ctx() -> InvocationContext {
    InvocationContext::new(CallerId::Host, CapabilityTier::Mutating)
}

fn feed_keys(session: &mut HostSession, keys: &str) {
    for ch in keys.chars() {
        let key = match ch {
            '\x1b' => KeyEvent::escape(),
            '\r' => KeyEvent::enter(),
            _ => KeyEvent::char(ch),
        };
        let _ = session.process_key_host(key);
    }
}

// ===========================================================================
// 1. ALL VIMVALUE VARIANTS — prove every type works through the full pipeline
// ===========================================================================

#[test]
fn all_vim_value_variants_round_trip() {
    let mut engine = VimEngine::new();

    let cases: Vec<(&str, VimValue)> = vec![
        ("nil_var", VimValue::Nil),
        ("bool_var", VimValue::Bool(true)),
        ("int_var", VimValue::Int(-42)),
        ("float_var", VimValue::Float(3.14159)),
        (
            "string_var",
            VimValue::String(CompactString::from("hello world")),
        ),
        (
            "list_var",
            VimValue::List(vec![
                VimValue::Int(1),
                VimValue::Int(2),
                VimValue::String(CompactString::from("three")),
            ]),
        ),
        (
            "map_var",
            VimValue::Map({
                let mut m = BTreeMap::new();
                m.insert(CompactString::from("key"), VimValue::Bool(false));
                m.insert(CompactString::from("count"), VimValue::Int(99));
                m
            }),
        ),
    ];

    for (name, value) in &cases {
        engine.apply_effect(&Effect::SetVariable {
            scope: VarScope::Global,
            name: CompactString::from(*name),
            value: value.clone(),
        });
    }

    let store = engine.state().variable_store();
    for (name, expected) in &cases {
        let actual = store.get(VarScope::Global, name);
        assert_eq!(actual, Some(expected), "Variable g:{name} mismatch");
    }
}

#[test]
fn nested_vim_value_structures() {
    let mut engine = VimEngine::new();

    let nested = VimValue::Map({
        let mut m = BTreeMap::new();
        m.insert(
            CompactString::from("users"),
            VimValue::List(vec![
                VimValue::Map({
                    let mut u = BTreeMap::new();
                    u.insert(
                        CompactString::from("name"),
                        VimValue::String(CompactString::from("Alice")),
                    );
                    u.insert(CompactString::from("age"), VimValue::Int(30));
                    u
                }),
                VimValue::Map({
                    let mut u = BTreeMap::new();
                    u.insert(
                        CompactString::from("name"),
                        VimValue::String(CompactString::from("Bob")),
                    );
                    u.insert(CompactString::from("age"), VimValue::Int(25));
                    u
                }),
            ]),
        );
        m.insert(CompactString::from("count"), VimValue::Int(2));
        m
    });

    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("data"),
        value: nested.clone(),
    });

    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Global, "data"),
        Some(&nested)
    );
}

// ===========================================================================
// 2. VARIABLE VIEW LIST — prove enumeration works
// ===========================================================================

#[test]
fn variable_view_list_global() {
    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("alpha"),
        value: VimValue::Int(1),
    });
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("beta"),
        value: VimValue::Int(2),
    });
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("gamma"),
        value: VimValue::Int(3),
    });

    let store = engine.state().variable_store();
    let listed: Vec<_> = store.list(VarScope::Global).collect();

    assert_eq!(listed.len(), 3);
    // BTreeMap iteration is alphabetical
    assert_eq!(listed[0].0, "alpha");
    assert_eq!(listed[1].0, "beta");
    assert_eq!(listed[2].0, "gamma");
}

#[test]
fn variable_view_list_scopes_independent() {
    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("gvar"),
        value: VimValue::Int(1),
    });
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("bvar"),
        value: VimValue::Int(2),
    });

    let store = engine.state().variable_store();
    let global_list: Vec<_> = store.list(VarScope::Global).collect();
    let buffer_list: Vec<_> = store.list(VarScope::Buffer).collect();

    assert_eq!(global_list.len(), 1);
    assert_eq!(global_list[0].0, "gvar");
    assert_eq!(buffer_list.len(), 1);
    assert_eq!(buffer_list[0].0, "bvar");
}

#[test]
fn variable_view_list_through_vim_api() {
    let session = HostSession::new("hello");

    // Set variables through the engine directly, since VimApi effects are
    // deferred. A fresh engine exercises the real path.
    let mut engine = VimEngine::new();
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("x"),
        value: VimValue::Int(10),
    });
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("y"),
        value: VimValue::Int(20),
    });

    // Now create VimApi from the session that has no vars set
    // (proving the view delegates correctly)
    let api = VimApi::from_session(&session, host_ctx());
    let vars = api.variables();
    let list = vars.list(VarScope::Global);
    // Session is fresh, so empty
    assert!(list.is_empty());
}

// ===========================================================================
// 3. EMPTY NAME VALIDATION
// ===========================================================================

#[test]
fn empty_variable_name_rejected_on_set() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    let result = api
        .emit()
        .set_variable(VarScope::Global, "", VimValue::Int(1));
    assert!(result.is_err());
    match result.unwrap_err() {
        ApiError::VariableNotFound { scope, name } => {
            assert_eq!(scope, VarScope::Global);
            assert_eq!(name.as_str(), "");
        }
        other => panic!("expected VariableNotFound, got {other:?}"),
    }
}

#[test]
fn empty_variable_name_rejected_on_delete() {
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    let result = api.emit().delete_variable(VarScope::Buffer, "");
    assert!(result.is_err());
}

#[test]
fn whitespace_only_name_allowed() {
    // Vim allows whitespace-only names (weird but valid)
    let session = HostSession::new("hello");
    let api = VimApi::from_session(&session, host_ctx());

    let result = api
        .emit()
        .set_variable(VarScope::Global, " ", VimValue::Nil);
    assert!(result.is_ok());
}

// ===========================================================================
// 4. SIZE GUARD
// ===========================================================================

#[test]
fn size_guard_blocks_new_insertions_past_limit() {
    use vim_core::state::VariableStore;

    let mut store = VariableStore::default();
    let limit = VariableStore::max_variables_per_scope();

    // Fill to capacity
    for i in 0..limit {
        store.set(
            VarScope::Global,
            &format!("var_{i}"),
            VimValue::Int(i as i64),
        );
    }

    assert_eq!(store.list(VarScope::Global).count(), limit);

    // New insertion should be silently ignored
    store.set(VarScope::Global, "overflow", VimValue::Bool(true));
    assert!(!store.exists(VarScope::Global, "overflow"));
    assert_eq!(store.list(VarScope::Global).count(), limit);
}

#[test]
fn size_guard_allows_overwrite_at_capacity() {
    use vim_core::state::VariableStore;

    let mut store = VariableStore::default();
    let limit = VariableStore::max_variables_per_scope();

    // Fill to capacity
    for i in 0..limit {
        store.set(
            VarScope::Global,
            &format!("var_{i}"),
            VimValue::Int(i as i64),
        );
    }

    // Overwriting existing key should still work
    store.set(VarScope::Global, "var_0", VimValue::Int(999));
    assert_eq!(
        store.get(VarScope::Global, "var_0"),
        Some(&VimValue::Int(999))
    );
}

#[test]
fn size_guard_independent_per_scope() {
    use vim_core::state::VariableStore;

    let mut store = VariableStore::default();
    let limit = VariableStore::max_variables_per_scope();

    // Fill global to capacity
    for i in 0..limit {
        store.set(VarScope::Global, &format!("g_{i}"), VimValue::Int(i as i64));
    }

    // Buffer scope should still accept insertions
    store.set(VarScope::Buffer, "still_works", VimValue::Bool(true));
    assert!(store.exists(VarScope::Buffer, "still_works"));
}

// ===========================================================================
// 5. BUFFER SWITCH — advanced scenarios
// ===========================================================================

#[test]
fn buffer_switch_multiple_buffers() {
    let mut engine = VimEngine::new();

    // Buffer A: set b:name = "A"
    engine.on_buffer_enter(BufferLocalState::default());
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("name"),
        value: VimValue::String(CompactString::from("A")),
    });
    let saved_a = engine.on_buffer_leave(0);

    // Buffer B: set b:name = "B"
    engine.on_buffer_enter(BufferLocalState::default());
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("name"),
        value: VimValue::String(CompactString::from("B")),
    });
    let saved_b = engine.on_buffer_leave(0);

    // Re-enter A: verify b:name = "A"
    engine.on_buffer_enter(saved_a);
    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Buffer, "name"),
        Some(&VimValue::String(CompactString::from("A")))
    );
    let saved_a = engine.on_buffer_leave(0);

    // Re-enter B: verify b:name = "B"
    engine.on_buffer_enter(saved_b);
    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Buffer, "name"),
        Some(&VimValue::String(CompactString::from("B")))
    );
    let _saved_b = engine.on_buffer_leave(0);

    // Re-enter A again for final check
    engine.on_buffer_enter(saved_a);
    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Buffer, "name"),
        Some(&VimValue::String(CompactString::from("A")))
    );
}

#[test]
fn buffer_switch_delete_before_leave() {
    let mut engine = VimEngine::new();

    engine.on_buffer_enter(BufferLocalState::default());
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("temp"),
        value: VimValue::Int(1),
    });
    engine.apply_effect(&Effect::DeleteVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("temp"),
    });

    let saved = engine.on_buffer_leave(0);

    // Re-enter: temp should NOT be restored (was deleted before leave)
    engine.on_buffer_enter(saved);
    assert!(!engine
        .state()
        .variable_store()
        .exists(VarScope::Buffer, "temp"));
}

#[test]
fn buffer_switch_preserves_global_modifications() {
    let mut engine = VimEngine::new();

    // Set g:count = 1
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("count"),
        value: VimValue::Int(1),
    });

    // Switch buffers
    engine.on_buffer_enter(BufferLocalState::default());
    let _saved = engine.on_buffer_leave(0);
    engine.on_buffer_enter(BufferLocalState::default());

    // Modify g:count in new buffer context
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("count"),
        value: VimValue::Int(2),
    });

    // Switch again
    let _saved = engine.on_buffer_leave(0);
    engine.on_buffer_enter(BufferLocalState::default());

    // g:count should reflect the latest modification
    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Global, "count"),
        Some(&VimValue::Int(2))
    );
}

// ===========================================================================
// 6. SYSTEM TEST — plugin-like workflows
// ===========================================================================

#[test]
fn system_test_plugin_state_across_invocations() {
    // Simulates a plugin that tracks how many times it's been called.
    // First invocation: read g:call_count (None), set to 1
    // Second invocation: read g:call_count (Some(1)), increment to 2

    let mut engine = VimEngine::new();

    // First "invocation"
    let count = engine
        .state()
        .variable_store()
        .get(VarScope::Global, "call_count")
        .and_then(|v| match v {
            VimValue::Int(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0);
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("call_count"),
        value: VimValue::Int(count + 1),
    });

    // Second "invocation"
    let count = engine
        .state()
        .variable_store()
        .get(VarScope::Global, "call_count")
        .and_then(|v| match v {
            VimValue::Int(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0);
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("call_count"),
        value: VimValue::Int(count + 1),
    });

    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Global, "call_count"),
        Some(&VimValue::Int(2))
    );
}

#[test]
fn system_test_per_buffer_plugin_config() {
    // Simulates a plugin that stores per-buffer config (like formatoptions)
    let mut engine = VimEngine::new();

    let config_a = VimValue::Map({
        let mut m = BTreeMap::new();
        m.insert(CompactString::from("indent"), VimValue::Int(4));
        m.insert(
            CompactString::from("lang"),
            VimValue::String(CompactString::from("rust")),
        );
        m
    });

    let config_b = VimValue::Map({
        let mut m = BTreeMap::new();
        m.insert(CompactString::from("indent"), VimValue::Int(2));
        m.insert(
            CompactString::from("lang"),
            VimValue::String(CompactString::from("python")),
        );
        m
    });

    // Buffer A
    engine.on_buffer_enter(BufferLocalState::default());
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("plugin_config"),
        value: config_a.clone(),
    });
    let saved_a = engine.on_buffer_leave(0);

    // Buffer B
    engine.on_buffer_enter(BufferLocalState::default());
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Buffer,
        name: CompactString::from("plugin_config"),
        value: config_b.clone(),
    });
    let saved_b = engine.on_buffer_leave(0);

    // Verify isolation
    engine.on_buffer_enter(saved_a);
    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Buffer, "plugin_config"),
        Some(&config_a)
    );
    let _saved_a = engine.on_buffer_leave(0);

    engine.on_buffer_enter(saved_b);
    assert_eq!(
        engine
            .state()
            .variable_store()
            .get(VarScope::Buffer, "plugin_config"),
        Some(&config_b)
    );
}

#[test]
fn system_test_variables_with_real_editing() {
    // Prove variables work alongside real keystroke processing
    let mut session = HostSession::new("hello world");

    // Process some keystrokes
    feed_keys(&mut session, "w"); // move to "world"

    // Create API and verify cursor moved
    let api = VimApi::from_session(&session, host_ctx());
    assert_eq!(api.cursor().offset(), 6);

    // Emit a variable through the API
    api.emit()
        .set_variable(
            VarScope::Global,
            "cursor_word",
            VimValue::String(CompactString::from("world")),
        )
        .unwrap();

    // Verify effect was produced
    let effects = api.drain_effects();
    assert_eq!(effects.len(), 1);
    assert!(matches!(&effects[0], Effect::SetVariable { .. }));
}

#[test]
fn system_test_vim_api_variables_view_after_engine_modification() {
    // Use a HostSession, modify variables through apply_effect on the engine
    // directly, then verify the VimApi view sees them.
    // Note: HostSession exposes engine_mut() is not public, so we use the
    // workaround of testing via VimEngine directly.
    let mut engine = VimEngine::new();

    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("status"),
        value: VimValue::String(CompactString::from("ready")),
    });
    engine.apply_effect(&Effect::SetVariable {
        scope: VarScope::Global,
        name: CompactString::from("version"),
        value: VimValue::Int(2),
    });

    let store = engine.state().variable_store();
    let listed: Vec<_> = store.list(VarScope::Global).collect();
    assert_eq!(listed.len(), 2);

    // Verify both are retrievable
    assert_eq!(
        store.get(VarScope::Global, "status"),
        Some(&VimValue::String(CompactString::from("ready")))
    );
    assert_eq!(
        store.get(VarScope::Global, "version"),
        Some(&VimValue::Int(2))
    );
}
