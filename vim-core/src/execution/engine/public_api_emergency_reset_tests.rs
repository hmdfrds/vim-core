//! Tests for [`VimEngine::emergency_reset`].
//!
//! Verifies that emergency_reset() clears all transient/untrustworthy state
//! (parser, mode, recording, changelist, typeahead, pending mapping,
//! dot-repeat, command-line session, cmd buffer) while preserving long-lived
//! user state (registers, marks, jumplist).

use super::super::VimEngine;
use crate::effects::Effect;
use crate::keymap::KeyEvent;
use crate::primitives::Mode;
use crate::primitives::{Mark, MarkName, Offset, RegisterContent, RegisterName};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Create a minimal `InputContext` suitable for processing a single key.
fn make_ctx() -> crate::execution::InputContext<
    'static,
    crate::test_utils::SimpleDocument,
    crate::execution::context::Validated,
> {
    // Leak a static doc — acceptable in unit tests (single allocation per call).
    let doc: &'static crate::test_utils::SimpleDocument = Box::leak(Box::new(
        crate::test_utils::SimpleDocument::new("hello world\nsecond line\nthird line\n"),
    ));
    crate::execution::InputContext::new(doc, 0).validate_clamped()
}

// ── Parser cleared ───────────────────────────────────────────────────────────

/// After `emergency_reset()`, the parser must be in its initial (Ready) state
/// with no pending operator.
#[test]
fn parser_cleared_after_emergency_reset() {
    let mut engine = VimEngine::new();

    // Push 'd' to enter operator-pending state (parser waits for a motion).
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('d'), ctx);
    assert!(
        engine.pending_operator().is_some(),
        "parser should be in operator-pending state after 'd'"
    );

    engine.emergency_reset();

    assert!(
        engine.pending_operator().is_none(),
        "parser must have no pending operator after emergency_reset()"
    );
}

// ── Mode is Normal ───────────────────────────────────────────────────────────

/// Emergency reset must return the engine to Normal mode regardless of the
/// mode it was in before.
#[test]
fn mode_is_normal_after_emergency_reset() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Insert);
    assert!(
        engine.mode().is_insert(),
        "precondition: mode should be Insert"
    );

    engine.emergency_reset();

    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "mode must be Normal after emergency_reset()"
    );
}

/// Emergency reset from Visual mode.
#[test]
fn mode_normal_from_visual() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Visual(crate::primitives::VisualType::Char));

    engine.emergency_reset();

    assert_eq!(engine.mode(), Mode::Normal);
}

/// Emergency reset from CommandLine mode.
#[test]
fn mode_normal_from_command_line() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::CommandLine);

    engine.emergency_reset();

    assert_eq!(engine.mode(), Mode::Normal);
}

/// Emergency reset from Replace mode.
#[test]
fn mode_normal_from_replace() {
    let mut engine = VimEngine::new();
    engine.set_mode(Mode::Replace);

    engine.emergency_reset();

    assert_eq!(engine.mode(), Mode::Normal);
}

// ── Recording cleared ────────────────────────────────────────────────────────

/// After emergency_reset(), `recording_register()` must return `None`.
#[test]
fn recording_cleared_after_emergency_reset() {
    let mut engine = VimEngine::new();

    // Start recording into register 'a' by processing `qa`.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('q'), ctx);
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('a'), ctx);

    // The engine should now be recording into register 'a'.
    // (If the process didn't start recording, we at least verify that
    // emergency_reset clears whatever state exists.)

    engine.emergency_reset();

    assert_eq!(
        engine.recording_register(),
        None,
        "recording must be cleared after emergency_reset()"
    );
}

// ── Changelist cleared ───────────────────────────────────────────────────────

/// After emergency_reset(), the changelist must be empty.
#[test]
fn changelist_cleared_after_emergency_reset() {
    let mut engine = VimEngine::new();

    // Push some entries into the changelist.
    engine.changelist_mut().push(Offset::new(10));
    engine.changelist_mut().push(Offset::new(50));
    engine.changelist_mut().push(Offset::new(100));
    assert_eq!(
        engine.state().changelist().len(),
        3,
        "precondition: changelist should have 3 entries"
    );

    engine.emergency_reset();

    assert!(
        engine.state().changelist().entries().is_empty(),
        "changelist entries must be empty after emergency_reset()"
    );
    assert!(
        engine.state().changelist().is_empty(),
        "changelist must be empty after emergency_reset()"
    );
}

// ── Typeahead cleared ────────────────────────────────────────────────────────

/// After emergency_reset(), `has_pending_keys()` must return false.
#[test]
fn typeahead_cleared_after_emergency_reset() {
    let mut engine = VimEngine::new();

    // Inject keys into the typeahead buffer.
    engine.feed_keys("jjkk", false);
    assert!(
        engine.has_pending_keys(),
        "precondition: typeahead should have pending keys"
    );

    engine.emergency_reset();

    assert!(
        !engine.has_pending_keys(),
        "has_pending_keys() must be false after emergency_reset()"
    );
}

// ── Pending mapping cleared ──────────────────────────────────────────────────

/// After emergency_reset(), `has_pending_mapping()` must return false.
#[test]
fn pending_mapping_cleared_after_emergency_reset() {
    let mut engine = VimEngine::new();

    // Even without setting up a mapping, verify the invariant holds.
    engine.emergency_reset();

    assert!(
        !engine.has_pending_mapping(),
        "has_pending_mapping() must be false after emergency_reset()"
    );
}

// ── is_repeating cleared ─────────────────────────────────────────────────────

/// After emergency_reset(), `is_repeating()` must return false.
#[test]
fn is_repeating_cleared_after_emergency_reset() {
    let mut engine = VimEngine::new();

    // We cannot easily force is_repeating to true from the public API
    // without replaying a full dot-repeat cycle, but we can verify the
    // invariant that emergency_reset always produces false.
    engine.emergency_reset();

    assert!(
        !engine.is_repeating(),
        "is_repeating() must be false after emergency_reset()"
    );
}

// ── Named registers PRESERVED ────────────────────────────────────────────────

/// Named registers (a-z) must survive emergency_reset().
#[test]
fn named_registers_preserved_after_emergency_reset() {
    let mut engine = VimEngine::new();

    let reg_a = RegisterName::new_unchecked('a');
    let reg_z = RegisterName::new_unchecked('z');
    let content_a = RegisterContent::char_wise("hello from register a");
    let content_z = RegisterContent::char_wise("hello from register z");

    engine.registers_mut().set(reg_a, content_a.clone());
    engine.registers_mut().set(reg_z, content_z.clone());

    // Verify preconditions.
    assert!(engine.state().registers().get(reg_a).is_some());
    assert!(engine.state().registers().get(reg_z).is_some());

    engine.emergency_reset();

    // Registers must still contain the same data.
    let got_a = engine
        .state()
        .registers()
        .get(reg_a)
        .expect("register 'a' must survive emergency_reset()");
    assert_eq!(
        got_a.text(),
        "hello from register a",
        "register 'a' content must be preserved"
    );

    let got_z = engine
        .state()
        .registers()
        .get(reg_z)
        .expect("register 'z' must survive emergency_reset()");
    assert_eq!(
        got_z.text(),
        "hello from register z",
        "register 'z' content must be preserved"
    );
}

/// Multiple named registers across the alphabet are all preserved.
#[test]
fn multiple_named_registers_preserved() {
    let mut engine = VimEngine::new();

    // Set registers b, m, x with distinct content.
    for ch in ['b', 'm', 'x'] {
        let name = RegisterName::new_unchecked(ch);
        let content = RegisterContent::char_wise(format!("content-{ch}"));
        engine.registers_mut().set(name, content);
    }

    engine.emergency_reset();

    for ch in ['b', 'm', 'x'] {
        let name = RegisterName::new_unchecked(ch);
        let got = engine
            .state()
            .registers()
            .get(name)
            .unwrap_or_else(|| panic!("register '{ch}' must survive emergency_reset()"));
        assert_eq!(
            got.text(),
            format!("content-{ch}"),
            "register '{ch}' content must be preserved"
        );
    }
}

// ── Local marks PRESERVED ────────────────────────────────────────────────────

/// Local marks (a-z) must survive emergency_reset().
#[test]
fn local_marks_preserved_after_emergency_reset() {
    let mut engine = VimEngine::new();

    let mark_a = MarkName::new_unchecked('a');
    let mark_z = MarkName::new_unchecked('z');

    engine.marks_mut().set(mark_a, Mark::new(Offset::new(42)));
    engine.marks_mut().set(mark_z, Mark::new(Offset::new(999)));

    // Verify preconditions.
    assert!(engine.state().marks().get(mark_a).is_some());
    assert!(engine.state().marks().get(mark_z).is_some());

    engine.emergency_reset();

    let got_a = engine
        .state()
        .marks()
        .get(mark_a)
        .expect("mark 'a' must survive emergency_reset()");
    assert_eq!(
        got_a.offset().get(),
        42,
        "mark 'a' offset must be preserved"
    );

    let got_z = engine
        .state()
        .marks()
        .get(mark_z)
        .expect("mark 'z' must survive emergency_reset()");
    assert_eq!(
        got_z.offset().get(),
        999,
        "mark 'z' offset must be preserved"
    );
}

/// Global marks (A-Z) must also survive emergency_reset().
#[test]
fn global_marks_preserved_after_emergency_reset() {
    let mut engine = VimEngine::new();

    let mark_a = MarkName::new_unchecked('A');
    let mark_z = MarkName::new_unchecked('Z');

    engine.marks_mut().set(mark_a, Mark::new(Offset::new(100)));
    engine.marks_mut().set(mark_z, Mark::new(Offset::new(200)));

    engine.emergency_reset();

    let got_a = engine
        .state()
        .marks()
        .get(mark_a)
        .expect("global mark 'A' must survive emergency_reset()");
    assert_eq!(got_a.offset().get(), 100);

    let got_z = engine
        .state()
        .marks()
        .get(mark_z)
        .expect("global mark 'Z' must survive emergency_reset()");
    assert_eq!(got_z.offset().get(), 200);
}

// ── Jumplist PRESERVED ───────────────────────────────────────────────────────

/// Jumplist entries must survive emergency_reset().
#[test]
fn jumplist_preserved_after_emergency_reset() {
    let mut engine = VimEngine::new();

    engine.jump_list_mut().push(Offset::new(10), None);
    engine.jump_list_mut().push(Offset::new(500), None);
    engine.jump_list_mut().push(Offset::new(1000), None);

    assert_eq!(
        engine.state().jump_list().len(),
        3,
        "precondition: jumplist should have 3 entries"
    );

    engine.emergency_reset();

    assert_eq!(
        engine.state().jump_list().len(),
        3,
        "jumplist must still have 3 entries after emergency_reset()"
    );

    // Verify the actual offsets are preserved.
    let entries = engine.state().jump_list().entries();
    assert_eq!(entries[0].offset().get(), 10);
    assert_eq!(entries[1].offset().get(), 500);
    assert_eq!(entries[2].offset().get(), 1000);
}

// ── Idempotent on fresh engine ───────────────────────────────────────────────

/// Calling emergency_reset() on a freshly constructed engine must not panic.
#[test]
fn idempotent_on_fresh_engine() {
    let mut engine = VimEngine::new();
    engine.emergency_reset();

    // Verify the engine is in a usable state.
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(!engine.has_pending_keys());
    assert!(!engine.has_pending_mapping());
    assert!(!engine.is_repeating());
    assert_eq!(engine.recording_register(), None);
    assert!(engine.state().changelist().is_empty());
}

/// Calling emergency_reset() twice in a row must not panic.
#[test]
fn double_emergency_reset_does_not_panic() {
    let mut engine = VimEngine::new();
    engine.emergency_reset();
    engine.emergency_reset();

    assert_eq!(engine.mode(), Mode::Normal);
}

// ── Combined: dirty state then reset preserves user data ─────────────────────

/// Comprehensive test: put the engine into a maximally dirty state, then
/// verify that emergency_reset() clears transient state while preserving
/// all user data.
#[test]
fn dirty_engine_reset_preserves_user_data_clears_transient() {
    let mut engine = VimEngine::new();

    // --- Set up long-lived user state (should survive) ---

    // Registers
    let reg_a = RegisterName::new_unchecked('a');
    let reg_content = RegisterContent::char_wise("precious data");
    engine.registers_mut().set(reg_a, reg_content);

    // Marks
    let mark_b = MarkName::new_unchecked('b');
    engine.marks_mut().set(mark_b, Mark::new(Offset::new(77)));

    // Global marks
    let mark_g = MarkName::new_unchecked('G');
    engine.marks_mut().set(mark_g, Mark::new(Offset::new(300)));

    // Jumplist
    engine.jump_list_mut().push(Offset::new(111), None);
    engine.jump_list_mut().push(Offset::new(222), None);

    // --- Set up transient state (should be cleared) ---

    // Enter operator-pending state
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('d'), ctx);

    // Inject typeahead
    engine.feed_keys("jjkk", false);

    // Push changelist entries
    engine.changelist_mut().push(Offset::new(50));
    engine.changelist_mut().push(Offset::new(60));

    // --- Reset ---
    engine.emergency_reset();

    // --- Verify transient state cleared ---
    assert_eq!(engine.mode(), Mode::Normal, "mode must be Normal");
    assert!(engine.pending_operator().is_none(), "no pending operator");
    assert!(!engine.has_pending_keys(), "no pending keys");
    assert!(!engine.has_pending_mapping(), "no pending mapping");
    assert!(!engine.is_repeating(), "not repeating");
    assert_eq!(engine.recording_register(), None, "no recording");
    assert!(
        engine.state().changelist().is_empty(),
        "changelist must be empty"
    );

    // --- Verify user data preserved ---
    let got_reg = engine
        .state()
        .registers()
        .get(reg_a)
        .expect("register 'a' must survive");
    assert_eq!(got_reg.text(), "precious data");

    let got_mark = engine
        .state()
        .marks()
        .get(mark_b)
        .expect("mark 'b' must survive");
    assert_eq!(got_mark.offset().get(), 77);

    let got_global = engine
        .state()
        .marks()
        .get(mark_g)
        .expect("global mark 'G' must survive");
    assert_eq!(got_global.offset().get(), 300);

    assert_eq!(
        engine.state().jump_list().len(),
        2,
        "jumplist entries must survive"
    );
}

// ── Engine usable after reset ────────────────────────────────────────────────

/// After emergency_reset(), the engine must be able to process keys normally.
///
/// Verifies that fundamental operations produce the expected effects:
/// - `j` → SetCursor effect (cursor motion)
/// - `i` → transitions to Insert mode
/// - `:` → transitions to CommandLine mode
/// - `dd` → Delete effect (line deletion)
#[test]
fn engine_usable_after_emergency_reset() {
    let mut engine = VimEngine::new();

    // Dirty up the engine maximally.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('d'), ctx);
    engine.feed_keys("abc", false);
    engine.changelist_mut().push(Offset::new(42));

    // Reset.
    engine.emergency_reset();

    // ── `j` produces a SetCursor effect ──────────────────────────────
    let ctx = make_ctx();
    let response = engine.process(KeyEvent::char('j'), ctx);
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "j after emergency_reset() must produce a SetCursor effect, got: {:?}",
        response.effects(),
    );
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "mode must remain Normal after j",
    );

    // ── `i` transitions to Insert mode ───────────────────────────────
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('i'), ctx);
    assert!(
        engine.mode().is_insert(),
        "i after emergency_reset() must enter Insert mode, got: {:?}",
        engine.mode(),
    );

    // Return to Normal with Escape before next test.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::escape(), ctx);
    assert_eq!(engine.mode(), Mode::Normal, "Escape must return to Normal");

    // ── `:` transitions to CommandLine mode ──────────────────────────
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char(':'), ctx);
    assert_eq!(
        engine.mode(),
        Mode::CommandLine,
        ": after emergency_reset() must enter CommandLine mode",
    );

    // Cancel command line with Escape.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::escape(), ctx);
    assert_eq!(engine.mode(), Mode::Normal, "Escape must return to Normal");

    // ── `dd` produces a Delete effect ────────────────────────────────
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('d'), ctx);
    let ctx = make_ctx();
    let response = engine.process(KeyEvent::char('d'), ctx);
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::Delete { .. })),
        "dd after emergency_reset() must produce a Delete effect, got: {:?}",
        response.effects(),
    );
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "mode must remain Normal after dd",
    );

    // ── `v` enters Visual mode with SetSelection ──────────────────────
    let ctx = make_ctx();
    let response = engine.process(KeyEvent::char('v'), ctx);
    assert!(
        engine.mode().is_visual(),
        "v after emergency_reset() must enter Visual mode, got: {:?}",
        engine.mode(),
    );
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SetSelection { .. })),
        "v after emergency_reset() must produce a SetSelection effect, got: {:?}",
        response.effects(),
    );

    // Return to Normal with Escape before next test.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::escape(), ctx);
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "Escape must return to Normal after v"
    );

    // ── `n` with no prior search — must not panic ─────────────────────
    let ctx = make_ctx();
    let _response = engine.process(KeyEvent::char('n'), ctx);
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "n with no prior search must stay in Normal mode",
    );

    // ── Macro record+replay (`qa` j `q` then `@a`) ───────────────────
    // Start recording into register 'a'.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('q'), ctx);
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('a'), ctx);

    // Record `j` motion.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('j'), ctx);

    // Stop recording.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('q'), ctx);
    assert_eq!(
        engine.recording_register(),
        None,
        "recording must stop after second q",
    );

    // Replay macro @a.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('@'), ctx);
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('a'), ctx);

    // The macro replay injects keys into the typeahead buffer.
    // Drain and process them to complete the replay.
    let mut saw_set_cursor = false;
    while engine.has_pending_keys() {
        if let Some(output) = engine.drain_next_key() {
            if let super::super::MacroOutput::Key(key) = output {
                let ctx = make_ctx();
                let r = engine.process(key, ctx);
                if r.effects()
                    .iter()
                    .any(|e| matches!(e, Effect::SetCursor { .. }))
                {
                    saw_set_cursor = true;
                }
            }
        } else {
            break;
        }
    }
    assert!(
        saw_set_cursor,
        "@a macro replay of `j` must produce at least one SetCursor effect",
    );
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "mode must remain Normal after macro replay",
    );

    // ── Dot-repeat (`.`) — should not panic ───────────────────────────
    let ctx = make_ctx();
    let _response = engine.process(KeyEvent::char('.'), ctx);
    // The prior `dd` set the last command. Dot-repeat may produce a
    // Delete effect or be a no-op if the document is too small. At
    // minimum it must not panic and remain in Normal mode.
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "mode must remain Normal after dot-repeat",
    );

    // ── Undo (`u`) — should not panic ─────────────────────────────────
    let ctx = make_ctx();
    let _response = engine.process(KeyEvent::char('u'), ctx);
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "mode must remain Normal after undo",
    );

    // ── Yank/paste (`yy` + `p`) ──────────────────────────────────────
    // Yank current line.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('y'), ctx);
    let ctx = make_ctx();
    let response = engine.process(KeyEvent::char('y'), ctx);
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SetRegister { .. })),
        "yy must produce a SetRegister effect, got: {:?}",
        response.effects(),
    );
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "mode must remain Normal after yy",
    );

    // Paste.
    let ctx = make_ctx();
    let response = engine.process(KeyEvent::char('p'), ctx);
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::Insert { .. })),
        "p after yy must produce an Insert effect, got: {:?}",
        response.effects(),
    );
    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "mode must remain Normal after p",
    );
}

// ── Field-inventory test ──────────────────────────────────────────────────────

/// Compile-time-enforced field inventory for `VimEngine`.
///
/// This test destructures `VimEngine` **without a `..` wildcard** so that
/// adding a new field to the struct is a compile error until the developer
/// categorizes it here as either "cleared by emergency_reset" (with an
/// assertion) or "preserved" (with a `let _ = field;` + comment).
///
/// The test also verifies at runtime that all cleared fields are actually
/// clean after calling `emergency_reset()` on a maximally-dirtied engine.
#[test]
fn emergency_reset_field_inventory() {
    let mut engine = VimEngine::new();

    // ── Dirty transient state ────────────────────────────────────────
    // Process some keys to make the parser enter operator-pending.
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('d'), ctx);
    assert!(
        engine.pending_operator().is_some(),
        "precondition: parser should be operator-pending after 'd'",
    );

    // Set is_repeating (accessible from this module since it's a descendant of engine).
    engine.is_repeating = true;

    // Start macro recording via `qa`.
    // First reset parser from the 'd' above, then record.
    engine.parser.reset();
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('q'), ctx);
    let ctx = make_ctx();
    let _ = engine.process(KeyEvent::char('a'), ctx);

    // Set parser recording flag directly.
    engine
        .parser
        .set_recording(Some(RegisterName::new_unchecked('a')));

    // Inject typeahead keys.
    engine.feed_keys("jjkk", false);
    assert!(
        engine.has_pending_keys(),
        "precondition: typeahead has keys"
    );

    // Push changelist entries.
    engine.changelist_mut().push(Offset::new(10));
    engine.changelist_mut().push(Offset::new(20));

    // Set cmd_buffer to simulate a <Cmd>...<CR> in progress.
    engine.cmd_buffer = Some("set nu".to_string());

    // Set fork_active to true (directly accessible from this module).
    engine.fork_active = true;

    // Push a syntax selection entry.
    engine
        .state
        .syntax_selection_mut()
        .push(crate::primitives::Selections::single(
            crate::primitives::SelectionRange::new(Offset::new(0), Offset::new(10)),
        ));

    // Set sticky_column to a non-default value.
    engine
        .state
        .set_sticky_column(Some(crate::primitives::VirtualColumn::new(42)));

    // Set user data that should be PRESERVED.
    let reg_a = RegisterName::new_unchecked('a');
    engine
        .registers_mut()
        .set(reg_a, RegisterContent::char_wise("preserved register"));
    let mark_b = MarkName::new_unchecked('b');
    engine.marks_mut().set(mark_b, Mark::new(Offset::new(77)));
    engine.jump_list_mut().push(Offset::new(100), None);

    // ── Emergency reset ──────────────────────────────────────────────
    engine.emergency_reset();

    // ── Exhaustive destructure ───────────────────────────────────────
    // NO `..` wildcard — adding a field to VimEngine without updating
    // this list is a compile error.
    let VimEngine {
        // === CLEARED fields (asserted clean) ===
        ref parser,
        ref state,
        ref typeahead,
        ref host,
        ref recording,
        ref is_repeating,
        ref command_line_session,
        ref sticky_session,
        ref cmd_buffer,
        ref fork_active,

        // === PRESERVED fields (intentionally not cleared) ===
        ref keymap,
        ref dispatcher,
        ref options,
        ref buffer_overrides,
        ref window_overrides,
        ref resolved_options,
        ref options_dirty,
        ref digraph_registry,
        ref abbrev_table,
        ref langmap_table,
        ref engine_providers,
        ref handler_map,
        ref keystroke_seq,
        ref prediction_weights,
        ref shadow_enabled,
        ref shadow,
        ref shadow_generation,
        ref native_insert,
        ref sticky_prefixes,
        ref key_interest_dirty,
        ref viewport_first_line,
        ref viewport_height,
        ref terminal_cols,
        ref terminal_rows,
        ref diagnostics_count,

        // === Cold state (preserved, boxed) ===
        ref cold,
        ref pending_macro_error,
        ref last_external_edit_node,
        ref last_force_committed_node,
    } = engine;

    // ── Assert CLEARED fields ────────────────────────────────────────

    // parser: reset to Ready state, not recording.
    assert!(
        matches!(
            parser.state(),
            crate::grammar::input_state::InputState::Ready {
                count: None,
                register: None
            }
        ),
        "parser must be in Ready state with no count/register after emergency_reset(), got: {:?}",
        parser.state(),
    );
    assert!(
        !parser.is_recording(),
        "parser recording flag must be cleared after emergency_reset()",
    );

    // state: mode is Normal.
    assert_eq!(
        state.mode(),
        Mode::Normal,
        "mode must be Normal after emergency_reset()",
    );
    // state: insert_state cleared.
    assert!(
        state.insert_state().is_none(),
        "insert_state must be None after emergency_reset()",
    );
    // state: message cleared.
    assert!(
        state.message().is_none(),
        "message must be None after emergency_reset()",
    );
    // state: scroll_hint cleared.
    assert!(
        state.scroll_hint().is_none(),
        "scroll_hint must be None after emergency_reset()",
    );
    // state: changelist cleared.
    assert!(
        state.changelist().is_empty(),
        "changelist must be empty after emergency_reset()",
    );
    // state: macros not replaying.
    assert!(
        !state.macros().is_replaying(),
        "macros must not be replaying after emergency_reset()",
    );
    // state: macros recording register cleared.
    assert!(
        state.macros().recording_register().is_none(),
        "macros recording_register must be None after emergency_reset()",
    );
    // state: undo tree has no pending group.
    assert!(
        !state.undo_tree().has_pending_group(),
        "undo_tree must have no pending group after emergency_reset()",
    );
    // state: syntax selection cleared.
    assert!(
        state.syntax_selection().is_empty(),
        "syntax_selection must be empty after emergency_reset()",
    );
    // state: sticky_column cleared.
    assert!(
        state.sticky_column().is_none(),
        "sticky_column must be None after emergency_reset()",
    );

    // typeahead: buffer empty.
    assert!(
        typeahead.buffer.is_empty(),
        "typeahead buffer must be empty after emergency_reset()",
    );
    // typeahead: macro stack empty.
    assert!(
        typeahead.macro_stack.is_empty(),
        "typeahead macro_stack must be empty after emergency_reset()",
    );
    // typeahead: last_drained_flags cleared.
    assert!(
        typeahead.last_drained_flags.is_empty(),
        "typeahead last_drained_flags must be empty after emergency_reset()",
    );

    // host: pending map empty.
    assert!(
        host.pending.is_empty(),
        "host pending requests must be empty after emergency_reset()",
    );

    // recording: buffer cleared.
    assert!(
        recording.buffer.is_none(),
        "recording buffer must be None after emergency_reset()",
    );

    // is_repeating: false.
    assert!(
        !is_repeating,
        "is_repeating must be false after emergency_reset()",
    );

    // command_line_session: None.
    assert!(
        command_line_session.is_none(),
        "command_line_session must be None after emergency_reset()",
    );

    // sticky_session: None.
    assert!(
        sticky_session.is_none(),
        "sticky_session must be None after emergency_reset()",
    );

    // cmd_buffer: None.
    assert!(
        cmd_buffer.is_none(),
        "cmd_buffer must be None after emergency_reset()",
    );

    // fork_active: false.
    assert!(
        !fork_active,
        "fork_active must be false after emergency_reset()",
    );

    // ── Acknowledge PRESERVED fields ─────────────────────────────────
    // These fields are intentionally NOT cleared by emergency_reset().
    // Using `let _ = field;` to suppress unused-variable warnings while
    // keeping them in the exhaustive destructure for compile-time coverage.

    let _ = keymap; // User-configured keymap — expensive to rebuild.
    let _ = dispatcher; // Stateless mode dispatcher — no transient state.
    let _ = options; // User's global Vim options (tabstop, etc.).
    let _ = buffer_overrides; // Buffer-local option overrides.
    let _ = window_overrides; // Window-local option overrides.
    let _ = resolved_options; // Derived cache — rebuilt lazily via options_dirty.
    let _ = options_dirty; // Cache dirty flag — harmless if stale.
    let _ = engine_providers; // Registered providers (motions, syntax, etc.).
    let _ = handler_map; // Per-key handler delegation (:sethandler).
    let _ = keystroke_seq; // Monotonic counter — harmless if stale.
    let _ = shadow_enabled; // User toggle for shadow execution.
    let _ = shadow; // Self-healing shadow document (preserved across reset).
    let _ = shadow_generation; // Last-seen generation counter (preserved across reset).
    let _ = native_insert; // Host capability for native insert mode.
    let _ = sticky_prefixes; // User-configured sticky prefix bitflags (config, not session).
    let _ = cold; // Cold state (pipeline, recorder, overlays, federation).
    let _ = viewport_first_line; // Host-pushed viewport state (preserved).
    let _ = viewport_height; // Host-pushed viewport height (preserved).
    let _ = terminal_cols; // Host-pushed terminal width (preserved).
    let _ = terminal_rows; // Host-pushed terminal height (preserved).
    let _ = diagnostics_count; // Host-pushed diagnostic count (preserved).
}
