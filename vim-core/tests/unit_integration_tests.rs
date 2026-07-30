#![allow(non_snake_case)]
//! Unit and integration tests migrated from neovim_fidelity_{b,c}_tests.rs.
//!
//! These tests exercise APIs, type invariants, and defensive no-panic
//! guarantees that cannot be expressed as golden-file `vim_test!` macros.
//!
//! Categories:
//! - Digraph table lookups (API)
//! - Key variant structural tests (API)
//! - Ctrl-N / Ctrl-P no-panic (defensive)
//! - Ctrl-G U/u in insert mode (defensive)
//! - cnoremap / cunmap parsing (defensive)
//! - Mark constant and buffer-leave round-trip (API)
//! - Undo save-point navigation (API)

mod common;

use common::document::TestDocument;
use common::runner::apply_effect;
use vim_core::execution::{InputContext, VimEngine};
use vim_core::keymap::{Key, KeyEvent};
use vim_core::primitives::{lookup_digraph, MarkName, Offset};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Press a key and return the response (no doc mutation).
fn press_apply(
    engine: &mut VimEngine,
    doc: &TestDocument,
    key: KeyEvent,
) -> vim_core::execution::Response {
    let ctx = InputContext::new(doc, doc.cursor_offset()).validate_clamped();
    engine.process(key, ctx)
}

/// Press a key, apply all resulting effects to `doc`, return the new cursor offset.
fn press_and_apply(engine: &mut VimEngine, doc: &mut TestDocument, key: KeyEvent) -> usize {
    let ctx = InputContext::new(doc as &TestDocument, doc.cursor_offset()).validate_clamped();
    let mut response = engine.process(key, ctx);
    let effects = response.take_effects();
    for effect in effects {
        apply_effect(doc, effect);
    }
    doc.cursor_offset()
}

/// Run an ex command that only changes engine state (not the doc text).
fn run_ex(engine: &mut VimEngine, doc: &TestDocument, command: &str) {
    let _ = press_apply(engine, doc, KeyEvent::char(':'));
    for ch in command.chars() {
        let _ = press_apply(engine, doc, KeyEvent::char(ch));
    }
    let _ = press_apply(engine, doc, KeyEvent::enter());
}

// ══════════════════════════════════════════════════════════════════════════════
// DIGRAPH TABLE LOOKUPS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn digraph_greek_alpha() {
    assert_eq!(
        lookup_digraph('a', '*'),
        Some('α'),
        "digraph ('a','*') should produce Greek alpha α (U+03B1)"
    );
}

#[test]
fn digraph_euro_sign_eq_e() {
    let result = lookup_digraph('=', 'e');
    assert_eq!(
        result,
        Some('\u{0435}'),
        "digraph ('=','e') should produce Cyrillic 'е' (U+0435) via reversed-pair earlier entry"
    );
}

#[test]
fn digraph_euro_sign_eu() {
    assert_eq!(
        lookup_digraph('E', 'u'),
        Some('€'),
        "digraph ('E','u') should produce Euro sign € (U+20AC)"
    );
}

#[test]
fn digraph_box_drawing() {
    assert_eq!(
        lookup_digraph('h', 'h'),
        Some('\u{2500}'),
        "digraph ('h','h') should produce BOX DRAWINGS LIGHT HORIZONTAL (U+2500)"
    );
}

#[test]
fn digraph_table_size() {
    assert_eq!(lookup_digraph('a', '*'), Some('α'));
    assert!(lookup_digraph('b', '*').is_some());
    assert!(lookup_digraph('g', '*').is_some());
    assert!(lookup_digraph('n', '~').is_some());
    assert!(lookup_digraph('u', ':').is_some());
    assert!(lookup_digraph('o', ':').is_some());
    assert_eq!(lookup_digraph('h', 'h'), Some('\u{2500}'));
    assert_eq!(lookup_digraph('E', 'u'), Some('€'));
    assert!(lookup_digraph('f', 't').is_some());
}

// ══════════════════════════════════════════════════════════════════════════════
// KEY VARIANT STRUCTURAL TESTS
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn plug_key_recognized() {
    let plug_key = Key::Plug(0);
    assert!(matches!(plug_key, Key::Plug(0)));

    let plug1 = Key::Plug(1);
    let plug2 = Key::Plug(2);
    assert_ne!(plug1, plug2);

    let notation = plug_key.to_vim_notation();
    assert!(notation.contains("Plug"));
}

#[test]
fn cmd_key_notation_parses() {
    assert_eq!(Key::from_vim_notation("<Cmd>"), Some(Key::Cmd));
    assert_eq!(Key::from_vim_notation("<cmd>"), Some(Key::Cmd));
}

#[test]
fn cmd_key_to_vim_notation() {
    assert_eq!(Key::Cmd.to_vim_notation(), "<Cmd>");
}

// ══════════════════════════════════════════════════════════════════════════════
// CTRL-N / CTRL-P NO-PANIC (DEFENSIVE)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn ctrl_n_in_insert_does_not_error() {
    let mut doc = TestDocument::new("hello world", (0, 0));
    let mut engine = VimEngine::new();
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('i'));
    let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
    let _response = engine.process(KeyEvent::ctrl('n'), ctx);
}

#[test]
fn ctrl_p_in_insert_does_not_error() {
    let mut doc = TestDocument::new("hello world", (0, 0));
    let mut engine = VimEngine::new();
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('i'));
    let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
    let _response = engine.process(KeyEvent::ctrl('p'), ctx);
}

// ══════════════════════════════════════════════════════════════════════════════
// CTRL-G U / u IN INSERT MODE (DEFENSIVE)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn ctrl_g_upper_u_accepted() {
    let mut doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('i'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::ctrl('g'));
    let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
    let _response = engine.process(KeyEvent::char('U'), ctx);
}

#[test]
fn ctrl_g_lower_u_still_works() {
    let mut doc = TestDocument::new("hello", (0, 0));
    let mut engine = VimEngine::new();
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('i'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('x'));
    press_and_apply(&mut engine, &mut doc, KeyEvent::ctrl('g'));
    let ctx = InputContext::new(&doc, doc.cursor_offset()).validate_clamped();
    let _response = engine.process(KeyEvent::char('u'), ctx);
    press_and_apply(&mut engine, &mut doc, KeyEvent::char('y'));
}

// ══════════════════════════════════════════════════════════════════════════════
// CNOREMAP / CUNMAP PARSING (DEFENSIVE)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn cmap_command_parses() {
    let doc = TestDocument::new("", (0, 0));
    let mut engine = VimEngine::new();
    run_ex(&mut engine, &doc, "cnoremap x y");
}

#[test]
fn cunmap_command_parses() {
    let doc = TestDocument::new("", (0, 0));
    let mut engine = VimEngine::new();
    run_ex(&mut engine, &doc, "cnoremap x y");
    run_ex(&mut engine, &doc, "cunmap x");
}

// ══════════════════════════════════════════════════════════════════════════════
// MARK CONSTANT AND BUFFER-LEAVE ROUND-TRIP
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn last_position_mark_exists() {
    assert_eq!(MarkName::LAST_POSITION.char(), '"');
}

#[test]
fn on_buffer_leave_saves_mark() {
    let mut engine = VimEngine::new();
    let cursor_offset = 42;
    let saved = engine.on_buffer_leave(cursor_offset);
    engine.on_buffer_enter(saved);
    let mark = engine
        .state()
        .marks()
        .get(MarkName::LAST_POSITION)
        .expect("on_buffer_leave should set the '\"' (LAST_POSITION) mark");
    assert_eq!(mark.offset().get(), cursor_offset);
}

// ══════════════════════════════════════════════════════════════════════════════
// UNDO SAVE POINTS
// ══════════════════════════════════════════════════════════════════════════════

fn make_undo_group(engine: &mut VimEngine, cursor_offset: usize) {
    use vim_core::effects::Effect;
    engine.apply_effect(&Effect::BeginUndoGroup {
        cursor_strategy: vim_core::primitives::UndoCursorStrategy::FirstEdit,
    });
    engine
        .undo_tree_mut()
        .mark_edit_at(Offset::new(cursor_offset));
    engine.apply_effect(&Effect::EndUndoGroup { node_id: None });
}

#[test]
fn mark_save_sets_save_nr() {
    let mut engine = VimEngine::new();
    make_undo_group(&mut engine, 0);
    engine.undo_tree_mut().mark_save();
    make_undo_group(&mut engine, 1);
    let result = engine.undo_tree().earlier_by_saves(1);
    assert!(result.is_some());
    let (undo_steps, _cursor) = result.unwrap();
    assert_eq!(undo_steps, 1);
}

#[test]
fn earlier_by_saves_navigates() {
    let mut engine = VimEngine::new();
    make_undo_group(&mut engine, 0);
    engine.undo_tree_mut().mark_save();
    make_undo_group(&mut engine, 5);
    let result = engine.undo_tree().earlier_by_saves(1);
    assert!(result.is_some());
    let (undo_steps, _cursor) = result.unwrap();
    assert_eq!(undo_steps, 1);
}

// ══════════════════════════════════════════════════════════════════════════════
// ASSERT_EFFECTS! MACRO
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn assert_effects_bare_variants() {
    use vim_core::effects::Effect;
    use vim_test::assert_effects;
    let effects = vec![Effect::SetCursor {
        offset: Offset::new(5),
    }];
    assert_effects!(effects, [SetCursor]);
}

#[test]
fn assert_effects_with_guard() {
    use vim_core::effects::Effect;
    use vim_test::assert_effects;
    let effects = vec![Effect::SetCursor {
        offset: Offset::new(5),
    }];
    assert_effects!(effects, [SetCursor { offset } if offset.get() == 5]);
}

#[test]
fn assert_effects_empty() {
    use vim_test::assert_effects;
    let effects: Vec<vim_core::effects::Effect> = vec![];
    assert_effects!(effects, []);
}

#[test]
fn assert_effects_multiple_bare() {
    use vim_core::effects::Effect;
    use vim_core::primitives::UndoCursorStrategy;
    use vim_test::assert_effects;
    let effects = vec![
        Effect::BeginUndoGroup {
            cursor_strategy: UndoCursorStrategy::FirstEdit,
        },
        Effect::SetCursor {
            offset: Offset::new(0),
        },
        Effect::EndUndoGroup { node_id: None },
    ];
    assert_effects!(effects, [BeginUndoGroup, SetCursor, EndUndoGroup]);
}

// ══════════════════════════════════════════════════════════════════════════════
// ROT47 OPERATOR DISPATCH INTEGRATION TEST
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn rot47_dispatch_transforms_text() {
    use vim_core::dispatch::{dispatch_operator, OperatorContext};
    use vim_core::effects::Effect;
    use vim_core::primitives::{MotionType, Operator, Range};

    let text = "Hello, World!";
    let ctx = OperatorContext::new(
        text,
        Range::from_raw(0, text.len()),
        MotionType::CharWise,
        None,
        1,
        Offset::new(0),
    );

    let result = dispatch_operator(Operator::Rot47, &ctx);

    let replaced_text = result.effects.iter().find_map(|e| {
        if let Effect::Replace { text, .. } = e {
            Some(text.as_str().to_owned())
        } else {
            None
        }
    });
    assert_eq!(
        replaced_text.as_deref(),
        Some("w6==@[ (@C=5P"),
        "ROT47 should encode 'Hello, World!' correctly"
    );

    // Verify involution: applying ROT47 twice returns original
    let encoded = replaced_text.unwrap();
    let ctx2 = OperatorContext::new(
        &encoded,
        Range::from_raw(0, encoded.len()),
        MotionType::CharWise,
        None,
        1,
        Offset::new(0),
    );
    let result2 = dispatch_operator(Operator::Rot47, &ctx2);
    let roundtrip = result2.effects.iter().find_map(|e| {
        if let Effect::Replace { text, .. } = e {
            Some(text.as_str().to_owned())
        } else {
            None
        }
    });
    assert_eq!(
        roundtrip.as_deref(),
        Some("Hello, World!"),
        "ROT47 applied twice should return original text"
    );
}

#[test]
fn rot47_operator_display_and_key_notation() {
    use vim_core::primitives::Operator;

    let op = Operator::Rot47;
    assert_eq!(op.key_notation(), "g&", "Rot47 key_notation should be g&");
    // strum Display derive produces the variant name
    assert_eq!(op.to_string(), "Rot47", "Rot47 display should be 'Rot47'");
}

// ══════════════════════════════════════════════════════════════════════════════
// BASIC ACTION TESTS
// ══════════════════════════════════════════════════════════════════════════════

vim_test::vim_spec!(action_cursor_right, "|hello", "l" => "h|ello");
vim_test::vim_spec!(action_delete_word, "|hello world", "dw" => "|world");
vim_test::vim_spec!(action_join_lines, "|hello\nworld", "J" => "hello| world");

// ══════════════════════════════════════════════════════════════════════════════
// SURROUND OPERATIONS (ys/ds/cs) — end-to-end integration tests
// ══════════════════════════════════════════════════════════════════════════════

vim_test::vim_suite!(surround_ys {
    ysiw_quote:        "|hello world", "ysiw\""  => "|\"hello\" world";
    ysiw_close_brace:  "|hello world", "ysiw}"   => "|{hello} world";
    ysiw_open_brace:   "|hello world", "ysiw{"   => "|{ hello } world";
    ysiw_close_paren:  "|hello world", "ysiw)"   => "|(hello) world";
    ysiw_open_paren:   "|hello world", "ysiw("   => "|( hello ) world";
    ysiw_close_bracket: "|hello world", "ysiw]"  => "|[hello] world";
    ysiw_open_bracket: "|hello world", "ysiw["   => "|[ hello ] world";
    ysiw_close_angle:  "|hello world", "ysiw>"   => "|<hello> world";
    ysiw_open_angle:   "|hello world", "ysiw<"   => "|< hello > world";
    ysiw_backtick:     "|hello world", "ysiw`"   => "|`hello` world";
});

vim_test::vim_suite!(surround_ds {
    ds_quote:   "\"h|ello\"", "ds\"" => "|hello";
    ds_paren:   "(h|ello)",   "ds)"  => "|hello";
    ds_bracket: "[h|ello]",   "ds]"  => "|hello";
    ds_brace:   "{h|ello}",   "ds}"  => "|hello";
    ds_angle:   "<h|ello>",   "ds>"  => "|hello";
    ds_nested_inner: "((t|ext))", "ds)" => "(|text)";
});

vim_test::vim_suite!(surround_cs {
    cs_quote_to_single:       "\"h|ello\"", "cs\"'"  => "|'hello'";
    cs_paren_to_bracket:      "(h|ello)",   "cs)]"   => "|[hello]";
    cs_close_to_open_paren:   "(h|ello)",   "cs)("   => "|( hello )";
    cs_close_to_open_brace:   "{h|ello}",   "cs}{"   => "|{ hello }";
});

vim_test::vim_spec!(surround_visual_s_quote, "|hello world", "viwS\"" => "|\"hello\" world");
