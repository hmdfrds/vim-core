//! Integration tests for `:set`/`:setlocal`/`:setglobal` scope routing.
//!
//! Verifies that option assignments go to the correct layers (global,
//! buffer override, window override) depending on the command variant and
//! the option's own scope.

mod common;

use common::document::TestDocument;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::keymap::KeyEvent;
use vim_core::primitives::{OptionId, OptionValue};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn run_ex(engine: &mut VimEngine, doc: &TestDocument, command: &str) {
    let _ = press(engine, doc, KeyEvent::char(':'));
    for ch in command.chars() {
        let _ = press(engine, doc, KeyEvent::char(ch));
    }
    let _ = press(engine, doc, KeyEvent::enter());
}

fn press(
    engine: &mut VimEngine,
    doc: &TestDocument,
    key: KeyEvent,
) -> vim_core::execution::Response {
    let ctx = InputContext::new(doc, doc.cursor_offset()).validate_clamped();
    engine.process(key, ctx)
}

// ── Tests ────────────────────────────────────────────────────────────────────

// `:set tabstop=4` — LocalToBuffer option.
// Expected: global tabstop = 4, buffer override has tabstop = 4, effective = 4.
#[test]
fn set_tabstop_writes_global_and_buffer_override() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    run_ex(&mut engine, &doc, "set tabstop=4");

    assert_eq!(
        engine.options().tabstop(),
        4,
        ":set tabstop=4 should update the global layer",
    );
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(4),
        "effective tabstop should be 4",
    );
}

// `:setlocal tabstop=8` — should set only the buffer override.
// Global tabstop remains at the default (8 is the Vim default, so use 4 first).
#[test]
fn setlocal_tabstop_writes_buffer_override_only() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // First set global to a known value.
    run_ex(&mut engine, &doc, "set tabstop=4");
    assert_eq!(engine.options().tabstop(), 4);

    // Now setlocal to 8.
    run_ex(&mut engine, &doc, "setlocal tabstop=8");

    // Global should remain 4.
    assert_eq!(
        engine.options().tabstop(),
        4,
        ":setlocal should not change the global layer",
    );
    // Effective (via resolved cache) should be 8.
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8),
        "effective tabstop should be 8 (from buffer override)",
    );
}

// `:setglobal tabstop=2` — should set only global, not the buffer override.
#[test]
fn setglobal_tabstop_writes_global_only() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // Set a local override first.
    run_ex(&mut engine, &doc, "setlocal tabstop=8");

    // Now setglobal to 2.
    run_ex(&mut engine, &doc, "setglobal tabstop=2");

    // Global should now be 2.
    assert_eq!(
        engine.options().tabstop(),
        2,
        ":setglobal tabstop=2 should update the global layer to 2",
    );
    // Effective should still be 8 (buffer override not touched).
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8),
        "effective tabstop should remain 8 (buffer override unchanged)",
    );
}

// `:set ignorecase` — Global-scoped option.
// `:setlocal ignorecase` should behave the same as `:set` (global only).
#[test]
fn setlocal_global_option_behaves_like_set() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // Default ignorecase is false.
    assert!(!engine.options().ignorecase());

    run_ex(&mut engine, &doc, "setlocal ignorecase");

    // Global should be changed (Global-scoped options fall through to global).
    assert!(
        engine.options().ignorecase(),
        ":setlocal on a Global-scoped option should set the global layer",
    );
    assert_eq!(
        engine.effective_option(OptionId::IgnoreCase),
        OptionValue::Bool(true),
        "effective ignorecase should be true",
    );
}

// `:setlocal scrolloff=5` — LocalToWindow option.
// Expected: window override has scrolloff=5, effective = 5.
// Global should also be updated by :set, but by :setlocal only the window override.
#[test]
fn setlocal_scrolloff_writes_window_override_only() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // Set global to a known value first.
    run_ex(&mut engine, &doc, "set scrolloff=3");
    assert_eq!(engine.options().scrolloff(), 3);

    // Now setlocal scrolloff=5.
    run_ex(&mut engine, &doc, "setlocal scrolloff=5");

    // Global should remain 3.
    assert_eq!(
        engine.options().scrolloff(),
        3,
        ":setlocal should not change global scrolloff",
    );
    // Effective should be 5 via window override.
    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(5),
        "effective scrolloff should be 5 (from window override)",
    );
}

// `:set scrolloff=7` — LocalToWindow option.
// Expected: both global and window override get updated.
#[test]
fn set_scrolloff_writes_global_and_window_override() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    run_ex(&mut engine, &doc, "set scrolloff=7");

    assert_eq!(
        engine.options().scrolloff(),
        7,
        ":set scrolloff=7 should update global",
    );
    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(7),
        "effective scrolloff should be 7",
    );
}

// `:setglobal scrolloff=10` — LocalToWindow option.
// Expected: global updated to 10, window override unchanged.
#[test]
fn setglobal_scrolloff_writes_global_only() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // Install window override.
    run_ex(&mut engine, &doc, "setlocal scrolloff=5");
    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(5)
    );

    run_ex(&mut engine, &doc, "setglobal scrolloff=10");

    assert_eq!(
        engine.options().scrolloff(),
        10,
        ":setglobal scrolloff=10 should update global to 10",
    );
    // Effective should still be the window override (5).
    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(5),
        "effective scrolloff should remain 5 (window override unchanged)",
    );
}

// Multiple sequential :set commands accumulate correctly.
#[test]
fn multiple_set_commands_accumulate() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    run_ex(&mut engine, &doc, "set tabstop=2");
    run_ex(&mut engine, &doc, "set shiftwidth=2");
    run_ex(&mut engine, &doc, "set expandtab");

    assert_eq!(engine.options().tabstop(), 2);
    assert_eq!(engine.options().shiftwidth(), 2);
    assert!(engine.options().expandtab());
}

// ── Additional scope-routing tests ────────────────────────────────────────

// Host lifecycle test:
// set_buffer_overrides(tabstop=8) → :setlocal sw=4 → effective ts=8, sw=4
// → take_buffer_overrides → set_buffer_overrides(empty) → effective ts falls back to global.
#[test]
fn host_lifecycle_buffer_overrides() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // Install a buffer override directly via host API: tabstop=8.
    let mut overrides = vim_core::primitives::OptionOverrides::new();
    overrides.set(OptionId::TabStop, OptionValue::Unsigned(8));
    engine.set_buffer_overrides(overrides);

    // Process :setlocal sw=4 (shiftwidth, LocalToBuffer → buffer override).
    run_ex(&mut engine, &doc, "setlocal shiftwidth=4");

    // Effective tabstop should be 8 (from buffer override), effective shiftwidth should be 4.
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8),
        "effective tabstop should be 8 (host-installed buffer override)",
    );
    assert_eq!(
        engine.effective_option(OptionId::ShiftWidth),
        OptionValue::Unsigned(4),
        "effective shiftwidth should be 4 (setlocal buffer override)",
    );

    // Simulate buffer switch: take overrides, then install empty set.
    let _saved = engine.take_buffer_overrides();
    engine.set_buffer_overrides(vim_core::primitives::OptionOverrides::new());

    // Now effective tabstop should fall back to global default (4).
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(4),
        "effective tabstop should fall back to global default after clearing buffer overrides",
    );
}

// Window override swap test:
// :setlocal scrolloff=5 → effective=5 → take_window_overrides → set_window_overrides(empty)
// → effective scrolloff falls back to global.
#[test]
fn window_override_swap() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    run_ex(&mut engine, &doc, "setlocal scrolloff=5");
    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(5),
        "effective scrolloff should be 5 (window override)",
    );

    // Simulate window switch: take window overrides, install empty set.
    let _saved = engine.take_window_overrides();
    engine.set_window_overrides(vim_core::primitives::OptionOverrides::new());

    // Effective scrolloff should fall back to global (default is 5, so set global to 3 first).
    // Reset with a known global value:
    let doc2 = TestDocument::new("hello", (0, 0));
    let mut engine2 = VimEngine::new();
    run_ex(&mut engine2, &doc2, "set scrolloff=3");
    run_ex(&mut engine2, &doc2, "setlocal scrolloff=7");
    assert_eq!(
        engine2.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(7)
    );

    let _saved2 = engine2.take_window_overrides();
    engine2.set_window_overrides(vim_core::primitives::OptionOverrides::new());

    assert_eq!(
        engine2.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(3),
        "effective scrolloff should fall back to global (3) after clearing window overrides",
    );
}

// GlobalOrLocal sentinel behavior:
// Set buffer override for backspace="" (sentinel) → effective falls through to global.
// Set to "indent,eol" → effective uses local value.
#[test]
fn global_or_local_sentinel_behavior() {
    use compact_str::CompactString;

    let _doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // Install a sentinel ("") buffer override for backspace (GlobalOrLocalBuffer).
    let mut overrides = vim_core::primitives::OptionOverrides::new();
    overrides.set(
        OptionId::Backspace,
        OptionValue::Str(CompactString::new_inline("")),
    );
    engine.set_buffer_overrides(overrides);

    // Sentinel should fall through to global ("indent,eol,start").
    assert_eq!(
        engine.effective_option(OptionId::Backspace),
        OptionValue::Str(CompactString::new("indent,eol,start")),
        "sentinel backspace override should fall through to global value",
    );

    // Now set a real buffer override value.
    let mut overrides2 = vim_core::primitives::OptionOverrides::new();
    overrides2.set(
        OptionId::Backspace,
        OptionValue::Str(CompactString::new_inline("indent,eol")),
    );
    engine.set_buffer_overrides(overrides2);

    assert_eq!(
        engine.effective_option(OptionId::Backspace),
        OptionValue::Str(CompactString::new_inline("indent,eol")),
        "non-sentinel backspace override should use the local value",
    );
}

// Global-scoped option immunity:
// :setlocal ignorecase → sets global (Global-scoped option), effective=true.
// :set noignorecase → resets. Buffer overrides do NOT contain ignorecase.
#[test]
fn global_scoped_option_immunity() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    assert!(
        !engine.options().ignorecase(),
        "default ignorecase should be false"
    );

    run_ex(&mut engine, &doc, "setlocal ignorecase");

    assert!(
        engine.options().ignorecase(),
        ":setlocal on Global option should set global layer"
    );
    assert_eq!(
        engine.effective_option(OptionId::IgnoreCase),
        OptionValue::Bool(true),
        "effective ignorecase should be true",
    );

    // :set noignorecase to reset global.
    run_ex(&mut engine, &doc, "set noignorecase");
    assert!(
        !engine.options().ignorecase(),
        ":set noignorecase should clear global ignorecase"
    );

    // Buffer overrides must NOT contain ignorecase (it's Global-scoped; :setlocal must not store it locally).
    // We verify by checking that the effective value equals the global value (no local shadow).
    assert_eq!(
        engine.effective_option(OptionId::IgnoreCase),
        OptionValue::Bool(false),
        "effective ignorecase should match global after reset, confirming no buffer override",
    );
}

// :setlocal doesn't change global:
// Set global tabstop=4. :setlocal tabstop=8.
// engine.options().tabstop() == 4 (global unchanged).
// engine.effective_option(TabStop) == 8 (local override).
#[test]
fn setlocal_does_not_change_global() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // Global tabstop is already 4 by default; confirm it.
    run_ex(&mut engine, &doc, "set tabstop=4");
    assert_eq!(engine.options().tabstop(), 4, "global tabstop should be 4");

    // :setlocal tabstop=8 — only touches buffer override.
    run_ex(&mut engine, &doc, "setlocal tabstop=8");

    assert_eq!(
        engine.options().tabstop(),
        4,
        "global tabstop should still be 4 after :setlocal",
    );
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8),
        "effective tabstop should be 8 (buffer override from :setlocal)",
    );
}

// Multiple overrides interact correctly:
// Buffer override tabstop=8, window override scrolloff=3, :set shiftwidth=2.
// Verify all three are effective simultaneously, and global shiftwidth=2.
#[test]
fn multiple_overrides_interact_correctly() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    // Install buffer override: tabstop=8.
    let mut buf_overrides = vim_core::primitives::OptionOverrides::new();
    buf_overrides.set(OptionId::TabStop, OptionValue::Unsigned(8));
    engine.set_buffer_overrides(buf_overrides);

    // Install window override: scrolloff=3.
    let mut win_overrides = vim_core::primitives::OptionOverrides::new();
    win_overrides.set(OptionId::ScrollOff, OptionValue::Unsigned(3));
    engine.set_window_overrides(win_overrides);

    // :set shiftwidth=2 — LocalToBuffer with "effective" scope writes global + buffer override.
    run_ex(&mut engine, &doc, "set shiftwidth=2");

    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8),
        "effective tabstop should be 8 (buffer override)",
    );
    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(3),
        "effective scrolloff should be 3 (window override)",
    );
    assert_eq!(
        engine.effective_option(OptionId::ShiftWidth),
        OptionValue::Unsigned(2),
        "effective shiftwidth should be 2 (:set command)",
    );
    assert_eq!(
        engine.options().shiftwidth(),
        2,
        "global shiftwidth should be 2 after :set",
    );
}

// :setlocal then clearing with :setglobal then taking overrides.
#[test]
fn setlocal_then_setglobal_independent() {
    let doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();

    run_ex(&mut engine, &doc, "set tabstop=4");
    run_ex(&mut engine, &doc, "setlocal tabstop=8");

    // Effective = local override = 8
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8)
    );

    // setglobal changes only global to 2
    run_ex(&mut engine, &doc, "setglobal tabstop=2");
    assert_eq!(engine.options().tabstop(), 2);
    // Effective still from local override = 8
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8)
    );
}
