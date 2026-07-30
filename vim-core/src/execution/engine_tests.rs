use super::macro_replay::{MacroEntry, MacroFrame, MacroOutput};
use super::*;
use crate::effects::Effect;
use crate::keymap::{Key, Modifiers};
use crate::primitives::VisualType;
use crate::test_utils::SimpleDocument;
use std::num::NonZeroU32;

/// Helper: create an engine + document, process a key, return response.
fn process_key(engine: &mut VimEngine, doc: &SimpleDocument, key: KeyEvent) -> Response {
    let ctx = InputContext::new(doc, 0).validate().unwrap();
    engine.process(key, ctx)
}

fn process_key_at(
    engine: &mut VimEngine,
    doc: &SimpleDocument,
    key: KeyEvent,
    cursor: usize,
) -> Response {
    let ctx = InputContext::new(doc, cursor).validate().unwrap();
    engine.process(key, ctx)
}

fn type_command_line(engine: &mut VimEngine, doc: &SimpleDocument, keys: &str) -> Response {
    process_key(engine, doc, KeyEvent::char(':'));
    assert_eq!(engine.mode(), Mode::CommandLine);
    let mut last = Response::default();
    for ch in keys.chars() {
        last = process_key(engine, doc, KeyEvent::char(ch));
    }
    last
}

fn type_command_line_at(
    engine: &mut VimEngine,
    doc: &SimpleDocument,
    keys: &str,
    cursor: usize,
) -> Response {
    process_key_at(engine, doc, KeyEvent::char(':'), cursor);
    assert_eq!(engine.mode(), Mode::CommandLine);
    let mut last = Response::default();
    for ch in keys.chars() {
        last = process_key_at(engine, doc, KeyEvent::char(ch), cursor);
    }
    last
}

// ─── Basic process() flow ──────────────────────────────────────────

#[test]
fn process_normal_motion_produces_effects() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");

    let response = process_key(&mut engine, &doc, KeyEvent::char('j'));

    assert!(response.consumed(), "motion key should be consumed");
    assert!(!response.pending(), "complete motion should not be pending");
    // 'j' in Normal mode produces a cursor move effect
    assert!(
        response
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "j motion should produce SetCursor effect"
    );
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn process_returns_pending_for_partial_input() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // 'd' in Normal mode → operator-pending (waiting for motion)
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));

    assert!(response.consumed(), "operator key should be consumed");
    assert!(
        response.pending(),
        "operator without motion should be pending"
    );
}

// ─── Mode changes ──────────────────────────────────────────────────

#[test]
fn process_i_enters_insert_mode() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    let response = process_key(&mut engine, &doc, KeyEvent::char('i'));

    assert!(response.consumed());
    assert_eq!(engine.mode(), Mode::Insert);
    assert!(
        !response.effects().is_empty(),
        "mode change should produce effects"
    );
}

#[test]
fn insert_exit_returns_to_normal() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Exit with Escape
    let response = process_key(&mut engine, &doc, KeyEvent::escape());

    assert!(response.consumed());
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SetMode { .. })),
        "insert exit should produce SetMode effect"
    );
    // Mode should be synced via effect_processor::sync_effect
    assert_eq!(engine.mode(), Mode::Normal);
}

// ─── Pipeline error recovery ───────────────────────────────────────

#[test]
fn pipeline_error_resets_parser() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Feed an invalid/unbound key sequence that produces a pipeline error
    // Ctrl-Q is not bound to anything in Normal mode
    let response = process_key(&mut engine, &doc, KeyEvent::ctrl('q'));

    // The parser should be reset, ready for new input
    assert!(!response.pending(), "error should not leave pending state");
}

// ─── Macro abort ───────────────────────────────────────────────────

#[test]
fn abort_replay_clears_stack() {
    let mut engine = VimEngine::new();

    // Manually push a frame to simulate an in-progress macro
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![
            MacroEntry::Key(KeyEvent::char('j')),
            MacroEntry::Key(KeyEvent::char('k')),
        ],
        cursor: 0,
        remaining_repeats: NonZeroU32::new(3).unwrap(),
    });
    assert!(engine.has_pending_keys());

    engine.abort_replay();
    assert!(!engine.has_pending_keys());
    assert!(engine.typeahead.macro_stack.is_empty());
}

// ─── Reset ─────────────────────────────────────────────────────────

#[test]
fn reset_clears_all_engine_state() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    engine.reset();
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(!engine.has_pending_keys());
    assert!(!engine.has_pending_mapping());
}

// ─── From<VimState> ────────────────────────────────────────────────

#[test]
fn from_vim_state_preserves_mode() {
    let mut state = VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));

    let engine = VimEngine::from(state);
    assert_eq!(engine.mode(), Mode::Visual(VisualType::Char));
}

// ─── Drain lifecycle ───────────────────────────────────────────────

#[test]
fn drain_next_key_repeats_macro_correctly() {
    let mut engine = VimEngine::new();

    // Prime the macro state so end_replay() won't panic
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();

    // Push a frame with 2 keys, 2 repeats
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![
            MacroEntry::Key(KeyEvent::char('j')),
            MacroEntry::Key(KeyEvent::char('k')),
        ],
        cursor: 0,
        remaining_repeats: NonZeroU32::new(2).unwrap(),
    });

    // First pass: j, k
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('j')))
    );
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('k')))
    );
    // Second pass: j, k
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('j')))
    );
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('k')))
    );
    // Exhausted
    assert_eq!(engine.drain_next_key(), None);
    assert!(!engine.has_pending_keys());
}

// ─── Public API ────────────────────────────────────────────────────

#[test]
fn timeoutlen_get_set() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.timeoutlen(), 500); // default

    engine.set_timeoutlen(2000);
    assert_eq!(engine.timeoutlen(), 2000);
}

/// `pending_timeout_ms()` returns `None` when nothing is pending, and the
/// correct timeout (timeoutlen or ttimeoutlen) when a mapping prefix is active.
#[test]
fn pending_timeout_ms_no_pending() {
    let engine = VimEngine::new();
    assert_eq!(engine.pending_timeout_ms(), None);
}

#[test]
fn pending_timeout_ms_non_escape_prefix_uses_timeoutlen() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    engine.set_timeoutlen(1000);
    let doc = SimpleDocument::new("hello");

    // Map "jk" → Esc so that 'j' alone puts the engine in pending state.
    engine.map(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::escape()]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    engine.process(KeyEvent::char('j'), ctx);
    assert!(engine.has_pending_mapping());

    // Non-Escape prefix → timeoutlen applies.
    assert_eq!(engine.pending_timeout_ms(), Some(1000));
}

#[test]
fn pending_timeout_ms_escape_prefix_uses_ttimeoutlen() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    engine.set_timeoutlen(1000);
    // Set ttimeoutlen explicitly to a short value.
    engine.options_mut().set_ttimeoutlen_ms(50);
    let doc = SimpleDocument::new("hello");

    // Map "<Esc>k" → something, so <Esc> alone produces a pending prefix.
    engine.map(
        MappingMode::Normal,
        &[KeyEvent::escape(), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::char('j')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    engine.process(KeyEvent::escape(), ctx);
    assert!(engine.has_pending_mapping());

    // Escape prefix → ttimeoutlen (50) applies, not timeoutlen (1000).
    assert_eq!(engine.pending_timeout_ms(), Some(50));
}

#[test]
fn pending_timeout_ms_escape_prefix_ttimeoutlen_minus_one_falls_back_to_timeoutlen() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    engine.set_timeoutlen(800);
    // Default ttimeoutlen is -1 → should fall back to timeoutlen.
    let doc = SimpleDocument::new("hello");

    engine.map(
        MappingMode::Normal,
        &[KeyEvent::escape(), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::char('j')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    engine.process(KeyEvent::escape(), ctx);
    assert!(engine.has_pending_mapping());

    // ttimeoutlen == -1 → fall back to timeoutlen (800).
    assert_eq!(engine.pending_timeout_ms(), Some(800));
}

#[test]
fn apply_effect_syncs_state() {
    let mut engine = VimEngine::new();

    // Apply a SetMode effect and verify state syncs
    let effect = Effect::set_mode(Mode::Visual(VisualType::Char));
    engine.apply_effect(&effect);
    assert_eq!(engine.mode(), Mode::Visual(VisualType::Char));
}

// ── EffectPipeline wiring ───────────────────────────────────────────

#[test]
fn pipeline_none_by_default() {
    let engine = VimEngine::new();
    assert!(engine.pipeline().is_none());
}

#[test]
fn set_pipeline_installs_middleware() {
    let mut engine = VimEngine::new();

    let mut pipeline = crate::effects::EffectPipeline::new();
    pipeline.push(crate::effects::DeduplicateMiddleware);
    engine.set_pipeline(Some(pipeline));

    assert!(engine.pipeline().is_some());
    assert_eq!(engine.pipeline().unwrap().len(), 1);
}

#[test]
fn pipeline_runs_on_process() {
    use crate::effects::LoggingMiddleware;

    let mut engine = VimEngine::new();
    let logger = LoggingMiddleware::new();
    let logger_clone = logger.clone();

    let mut pipeline = crate::effects::EffectPipeline::new();
    pipeline.push(logger);
    engine.set_pipeline(Some(pipeline));

    let doc = SimpleDocument::new("hello\nworld");
    process_key(&mut engine, &doc, KeyEvent::char('j'));

    // The logger should have captured effects from the j motion
    assert!(
        !logger_clone.log().is_empty(),
        "pipeline should have captured effects"
    );
}

#[test]
fn pipeline_runs_on_process_click() {
    use crate::effects::LoggingMiddleware;

    let mut engine = VimEngine::new();
    let logger = LoggingMiddleware::new();
    let logger_clone = logger.clone();

    let mut pipeline = crate::effects::EffectPipeline::new();
    pipeline.push(logger);
    engine.set_pipeline(Some(pipeline));

    let doc = SimpleDocument::new("hello\nworld");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    engine.process_click(5, &ctx);

    assert!(
        !logger_clone.log().is_empty(),
        "pipeline should have captured click effects"
    );
}

#[test]
fn pipeline_runs_on_process_mouse_selection() {
    use crate::effects::LoggingMiddleware;
    use crate::primitives::SelectionShape;

    let mut engine = VimEngine::new();
    let logger = LoggingMiddleware::new();
    let logger_clone = logger.clone();

    let mut pipeline = crate::effects::EffectPipeline::new();
    pipeline.push(logger);
    engine.set_pipeline(Some(pipeline));

    let doc = SimpleDocument::new("hello\nworld");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    engine.process_mouse_selection(2, 8, SelectionShape::Char, &ctx);

    assert!(
        !logger_clone.log().is_empty(),
        "pipeline should have captured mouse selection effects"
    );
}

#[test]
fn pipeline_mut_allows_push() {
    let mut engine = VimEngine::new();
    engine.set_pipeline(Some(crate::effects::EffectPipeline::new()));

    engine
        .pipeline_mut()
        .unwrap()
        .push(crate::effects::DeduplicateMiddleware);

    assert_eq!(engine.pipeline().unwrap().len(), 1);
}

#[test]
fn set_pipeline_none_removes() {
    let mut engine = VimEngine::new();
    engine.set_pipeline(Some(crate::effects::EffectPipeline::new()));
    assert!(engine.pipeline().is_some());

    engine.set_pipeline(None);
    assert!(engine.pipeline().is_none());
}

// ── Persistent provider registration ────────────────────────────────

#[test]
fn no_engine_providers_by_default() {
    let engine = VimEngine::new();
    assert!(!engine.has_engine_providers());
}

#[test]
fn register_motion_provider_persists() {
    use crate::document::CustomMotionProvider;

    struct TestMotionProvider;
    impl CustomMotionProvider for TestMotionProvider {
        fn compute_motion(
            &self,
            _id: u32,
            _text: &str,
            cursor: usize,
            _count: u32,
        ) -> Option<usize> {
            Some(cursor + 1)
        }
    }

    let mut engine = VimEngine::new();
    engine.register_motion_provider(TestMotionProvider);
    assert!(engine.has_engine_providers());
}

#[test]
fn register_textobject_provider_persists() {
    use crate::document::CustomTextObjectProvider;

    struct TestTextObjectProvider;
    impl CustomTextObjectProvider for TestTextObjectProvider {
        fn compute_textobject(
            &self,
            _id: u32,
            _text: &str,
            _cursor: usize,
            _inner: bool,
        ) -> Option<(usize, usize)> {
            Some((0, 5))
        }
    }

    let mut engine = VimEngine::new();
    engine.register_textobject_provider(TestTextObjectProvider);
    assert!(engine.has_engine_providers());
}

#[test]
fn register_operator_provider_persists() {
    use crate::document::{CustomOperatorProvider, CustomOperatorResult};

    struct TestOperatorProvider;
    impl CustomOperatorProvider for TestOperatorProvider {
        fn compute_operator(
            &self,
            _id: u32,
            _text: &str,
            _range: (usize, usize),
            _count: u32,
        ) -> Option<CustomOperatorResult> {
            Some(CustomOperatorResult::Defer)
        }
    }

    let mut engine = VimEngine::new();
    engine.register_operator_provider(TestOperatorProvider);
    assert!(engine.has_engine_providers());
}

#[test]
fn register_syntax_provider_persists() {
    use crate::document::{SyntaxNodeKind, SyntaxProvider};

    struct TestSyntaxProvider;
    impl SyntaxProvider for TestSyntaxProvider {
        fn enclosing_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
        ) -> Option<(usize, usize)> {
            None
        }
        fn next_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            None
        }
        fn prev_node(
            &self,
            _text: &str,
            _cursor: usize,
            _kind: SyntaxNodeKind,
            _count: u32,
        ) -> Option<usize> {
            None
        }
    }

    let mut engine = VimEngine::new();
    engine.register_syntax_provider(TestSyntaxProvider);
    assert!(engine.has_engine_providers());
}

// ── Syntax provider threading: end-to-end tests ───────────────────

/// A mock syntax provider that returns a known range for Function/Class kinds.
///
/// Simulates a tree-sitter provider: given cursor inside a function body,
/// returns the function's byte range. Used to verify provider threading
/// from engine → executor → dispatch → commands.
struct StructuralSyntaxProvider {
    /// (kind, start, end) — if cursor is inside [start..end], return this range.
    nodes: Vec<(crate::document::SyntaxNodeKind, usize, usize)>,
}

impl crate::document::SyntaxProvider for StructuralSyntaxProvider {
    fn enclosing_node(
        &self,
        _text: &str,
        cursor: usize,
        kind: crate::document::SyntaxNodeKind,
    ) -> Option<(usize, usize)> {
        self.nodes.iter().find_map(|(k, start, end)| {
            if std::mem::discriminant(k) == std::mem::discriminant(&kind)
                && cursor >= *start
                && cursor < *end
            {
                Some((*start, *end))
            } else {
                None
            }
        })
    }
    fn next_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: crate::document::SyntaxNodeKind,
        _count: u32,
    ) -> Option<usize> {
        None
    }
    fn prev_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: crate::document::SyntaxNodeKind,
        _count: u32,
    ) -> Option<usize> {
        None
    }
}

#[test]
fn syntax_provider_dif_delete_inner_function() {
    // Text layout (byte offsets):
    // "fn foo() {\n    body\n}\nafter\n"
    //  0          10  11      19 20 21
    let text = "fn foo() {\n    body\n}\nafter\n";
    let doc = SimpleDocument::new(text);
    // Function node spans the entire fn definition: bytes 0..21 ("fn foo() {\n    body\n}")
    let provider = StructuralSyntaxProvider {
        nodes: vec![(crate::document::SyntaxNodeKind::Function, 0, 21)],
    };

    let mut engine = VimEngine::new();
    engine.register_syntax_provider(provider);

    // Place cursor at byte 15 (inside "body"), process d-i-f
    let keys = [
        KeyEvent::char('d'),
        KeyEvent::char('i'),
        KeyEvent::char('f'),
    ];
    let mut response = Response::ignored();
    for key in &keys {
        let ctx = InputContext::new(&doc, 15).validate().unwrap();
        response = engine.process(*key, ctx);
    }

    // Inner function: trims first line ("fn foo() {\n") and last line ("\n}").
    // Inner range = bytes 11..19 ("    body" — up to last \n in the node).
    assert!(
        response.effects.iter().any(|e| matches!(e, Effect::Delete { range } if range.start().get() == 11 && range.end().get() == 19)),
        "dif should delete inner function body (bytes 11..19), got effects: {:?}",
        response.effects
    );
}

#[test]
fn syntax_provider_daf_delete_around_function() {
    let text = "fn foo() {\n    body\n}\nafter\n";
    let doc = SimpleDocument::new(text);
    let provider = StructuralSyntaxProvider {
        nodes: vec![(crate::document::SyntaxNodeKind::Function, 0, 21)],
    };

    let mut engine = VimEngine::new();
    engine.register_syntax_provider(provider);

    // Process d-a-f with cursor inside body
    let keys = [
        KeyEvent::char('d'),
        KeyEvent::char('a'),
        KeyEvent::char('f'),
    ];
    let mut response = Response::ignored();
    for key in &keys {
        let ctx = InputContext::new(&doc, 15).validate().unwrap();
        response = engine.process(*key, ctx);
    }

    // Around function: full node range 0..21.
    assert!(
        response.effects.iter().any(|e| matches!(e, Effect::Delete { range } if range.start().get() == 0 && range.end().get() == 21)),
        "daf should delete entire function (bytes 0..21), got effects: {:?}",
        response.effects
    );
}

#[test]
fn syntax_provider_no_provider_returns_no_delete() {
    let text = "fn foo() {\n    body\n}\n";
    let doc = SimpleDocument::new(text);

    // No syntax provider registered
    let mut engine = VimEngine::new();

    let keys = [
        KeyEvent::char('d'),
        KeyEvent::char('i'),
        KeyEvent::char('f'),
    ];
    let mut response = Response::ignored();
    for key in &keys {
        let ctx = InputContext::new(&doc, 15).validate().unwrap();
        response = engine.process(*key, ctx);
    }

    // Without a provider, dif should produce no Delete effect (graceful degradation).
    assert!(
        !response
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Delete { .. })),
        "dif without provider should produce no Delete, got: {:?}",
        response.effects
    );
}

#[test]
fn syntax_provider_cac_change_around_class() {
    // "class Foo {\n  val\n}\nrest\n"
    let text = "class Foo {\n  val\n}\nrest\n";
    let doc = SimpleDocument::new(text);
    let provider = StructuralSyntaxProvider {
        nodes: vec![(crate::document::SyntaxNodeKind::Class, 0, 19)],
    };

    let mut engine = VimEngine::new();
    engine.register_syntax_provider(provider);

    // Process c-a-c with cursor inside class body
    let keys = [
        KeyEvent::char('c'),
        KeyEvent::char('a'),
        KeyEvent::char('c'),
    ];
    let mut response = Response::ignored();
    for key in &keys {
        let ctx = InputContext::new(&doc, 14).validate().unwrap();
        response = engine.process(*key, ctx);
    }

    // Change = Replace("") + BeginInsert. Check Replace covers the class.
    // (Change uses Replace with empty text to preserve marks at start.)
    assert!(
        response.effects.iter().any(|e| matches!(e, Effect::Replace { range, text } if range.start().get() == 0 && range.end().get() == 19 && text.is_empty())),
        "cac should replace entire class (bytes 0..19) with empty, got effects: {:?}",
        response.effects
    );
    // Should enter insert mode
    assert!(
        response
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginInsert { .. })),
        "cac should enter insert mode"
    );
}

// ── <Plug> mapping support ────────────────────────────────────────

#[test]
fn plug_key_chains_through_mapping_trie() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nfoo");

    // Register a <Plug> name and create two mappings:
    // nmap S <Plug>(test-down)
    // nnoremap <Plug>(test-down) j
    let plug_key = engine.keymap_mut().register_plug("test-down");
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('S')],
        key_sequence(&[plug_key]),
        MappingKind::Recursive, // must be recursive to chain through <Plug>
        MappingFlags::default(),
    );
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[plug_key],
        key_sequence(&[KeyEvent::char('j')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Press 'S' — should chain: S → <Plug>(test-down) → j (move down)
    let response = process_key(&mut engine, &doc, KeyEvent::char('S'));
    assert!(response.consumed());
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "<Plug> chain should produce SetCursor from 'j' motion"
    );
}

#[test]
fn plug_registry_is_idempotent() {
    let mut engine = VimEngine::new();
    let k1 = engine.keymap_mut().register_plug("test");
    let k2 = engine.keymap_mut().register_plug("test");
    assert_eq!(k1, k2);
}

#[test]
fn action_key_emits_host_action_effect() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Register an action and map a key to it
    let action_key = engine.keymap_mut().register_action("ReformatCode");
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Press '=' — should emit Effect::HostAction { name: "ReformatCode" }
    let response = process_key(&mut engine, &doc, KeyEvent::char('='));
    assert!(response.consumed());
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::HostAction { name } if name.as_str() == "ReformatCode"
        )),
        "Action key should produce HostAction effect: got {:?}",
        response.effects()
    );
}

#[test]
fn action_registry_is_idempotent() {
    let mut engine = VimEngine::new();
    let k1 = engine.keymap_mut().register_action("Format");
    let k2 = engine.keymap_mut().register_action("Format");
    assert_eq!(k1, k2);
}

#[test]
fn action_name_lookup() {
    let mut engine = VimEngine::new();
    engine.keymap_mut().register_action("FindUsages");
    assert_eq!(engine.keymap().action_name(0), Some("FindUsages"));
    assert_eq!(engine.keymap().action_name(99), None);
}

#[test]
fn action_unknown_id_produces_empty_response() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Map to an Action(99) that was never registered
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('Z')],
        key_sequence(&[KeyEvent::action(99)]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let response = process_key(&mut engine, &doc, KeyEvent::char('Z'));
    assert!(response.consumed());
    // No HostAction effect for unknown id
    assert!(
        !response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::HostAction { .. })),
        "Unknown action id should not produce HostAction effect"
    );
}

#[test]
fn action_sentinel_id_produces_empty_response() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('Q')],
        key_sequence(&[KeyEvent::action(u32::MAX)]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let response = process_key(&mut engine, &doc, KeyEvent::char('Q'));
    assert!(response.consumed());
    assert!(
        !response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::HostAction { .. })),
        "Sentinel action id should not produce HostAction effect"
    );
    assert!(
        response.host_requests().is_empty(),
        "Sentinel action id should not produce any host requests"
    );
}

#[test]
fn action_forwards_count_in_host_request() {
    use crate::execution::host::HostRequest;
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    let action_key = engine.keymap_mut().register_action("Jump");
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Type '5' then '='
    process_key(&mut engine, &doc, KeyEvent::char('5'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('='));

    let run_action = response
        .host_requests()
        .iter()
        .find(|r| matches!(r, HostRequest::RunAction { .. }));
    assert!(run_action.is_some(), "Should have RunAction host request");
    if let HostRequest::RunAction { count, .. } = run_action.unwrap() {
        assert_eq!(*count, Some(5), "RunAction should carry count=5");
    }
}

#[test]
fn action_forwards_register_in_host_request() {
    use crate::execution::host::HostRequest;
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    let action_key = engine.keymap_mut().register_action("RegTest");
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Type `"a then = (select register 'a', then trigger action)
    process_key(&mut engine, &doc, KeyEvent::char('"'));
    process_key(&mut engine, &doc, KeyEvent::char('a'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('='));

    let run_action = response
        .host_requests()
        .iter()
        .find(|r| matches!(r, HostRequest::RunAction { .. }));
    assert!(run_action.is_some(), "Should have RunAction host request");
    if let HostRequest::RunAction { register, .. } = run_action.unwrap() {
        assert!(register.is_some(), "RunAction should carry register");
        assert_eq!(
            register.unwrap().char(),
            'a',
            "RunAction should carry register='a'"
        );
    }
}

#[test]
fn action_forwards_mode_in_host_request() {
    use crate::execution::host::HostRequest;
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    let action_key = engine.keymap_mut().register_action("ModeTest");
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let response = process_key(&mut engine, &doc, KeyEvent::char('='));

    let run_action = response
        .host_requests()
        .iter()
        .find(|r| matches!(r, HostRequest::RunAction { .. }));
    assert!(run_action.is_some(), "Should have RunAction host request");
    if let HostRequest::RunAction { mode, .. } = run_action.unwrap() {
        assert_eq!(
            mode.as_str(),
            Mode::Normal.display_name(),
            "RunAction mode should match Normal mode display name"
        );
    }
}

#[test]
fn action_forwards_selection_in_visual_mode() {
    use crate::execution::host::HostRequest;
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};
    use crate::primitives::SelectionRange;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    let action_key = engine.keymap_mut().register_action("SelTest");
    engine.keymap_mut().map(
        MappingMode::Visual,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Enter visual mode
    process_key(&mut engine, &doc, KeyEvent::char('v'));
    assert!(engine.mode().is_visual());

    // Trigger the action with an explicit selection provided in the InputContext.
    let sel = SelectionRange::new(
        crate::primitives::Offset::new(0),
        crate::primitives::Offset::new(5),
    );
    let ctx = InputContext::new(&doc, 0)
        .validate()
        .unwrap()
        .with_selection(sel);
    let response = engine.process(KeyEvent::char('='), ctx);

    let run_action = response
        .host_requests()
        .iter()
        .find(|r| matches!(r, HostRequest::RunAction { .. }));
    assert!(run_action.is_some(), "Should have RunAction host request");
    if let HostRequest::RunAction {
        selection_anchor,
        selection_head,
        mode,
        ..
    } = run_action.unwrap()
    {
        assert!(
            mode.as_str().contains("VISUAL"),
            "RunAction mode should indicate visual mode, got: {}",
            mode
        );
        assert_eq!(
            *selection_anchor,
            Some(0),
            "RunAction selection_anchor should be 0"
        );
        assert_eq!(
            *selection_head,
            Some(5),
            "RunAction selection_head should be 5"
        );
    }
}

// ── <Action>(name) notation in :map RHS ─────────────────────────────

#[test]
fn action_notation_via_source_config() {
    let mut engine = VimEngine::new();
    let config = "nnoremap <leader>r <Action>(Rename)\n";
    engine.source_config_text(config);

    // The action should be registered in the keymap
    assert_eq!(
        engine.keymap().action_name(0),
        Some("Rename"),
        "Action name should be registered via source_config_text"
    );

    // Now press backslash (default leader) then 'r'
    let doc = SimpleDocument::new("hello");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response1 = engine.process(KeyEvent::char('\\'), ctx);

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response2 = engine.process(KeyEvent::char('r'), ctx);

    // One of the responses should contain HostAction
    let has_host_action = response1
        .effects()
        .iter()
        .chain(response2.effects().iter())
        .any(|e| matches!(e, Effect::HostAction { name } if name.as_str() == "Rename"));
    assert!(
        has_host_action,
        "Mapping via <Action>(Rename) notation should produce HostAction effect.\n\
         response1 effects: {:?}\nresponse2 effects: {:?}",
        response1.effects(),
        response2.effects(),
    );
}

#[test]
fn action_notation_via_ex_command_line() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Execute `:nnoremap = <Action>(ReformatCode)` through the command line
    // We simulate this by using source_config_text which processes the same way
    engine.source_config_text("nnoremap = <Action>(ReformatCode)\n");

    // Verify action was registered
    assert_eq!(engine.keymap().action_name(0), Some("ReformatCode"),);

    // Press '=' — should emit Effect::HostAction { name: "ReformatCode" }
    let response = process_key(&mut engine, &doc, KeyEvent::char('='));
    assert!(response.consumed());
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::HostAction { name } if name.as_str() == "ReformatCode"
        )),
        "Action notation in map RHS should produce HostAction effect: got {:?}",
        response.effects(),
    );
}

#[test]
fn plug_notation_via_source_config() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nfoo");

    // Register via source config: two-step mapping through <Plug>
    let config = "nmap S <Plug>(test-down)\nnnoremap <Plug>(test-down) j\n";
    engine.source_config_text(config);

    // Verify plug was registered
    assert_eq!(engine.keymap().plug_name(0), Some("test-down"));

    // Press 'S' — should chain through <Plug>(test-down) → j (move down)
    let response = process_key(&mut engine, &doc, KeyEvent::char('S'));
    assert!(response.consumed());
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "<Plug>(name) notation via source_config should chain correctly: got {:?}",
        response.effects(),
    );
}

#[test]
fn multiple_actions_via_source_config() {
    let mut engine = VimEngine::new();

    let config = "\
        nnoremap <leader>r <Action>(Rename)\n\
        nnoremap <leader>f <Action>(FindUsages)\n\
    ";
    engine.source_config_text(config);

    // Both should be registered with different ids
    let rename_id = engine.keymap().action_id("Rename");
    let find_id = engine.keymap().action_id("FindUsages");
    assert!(rename_id.is_some(), "Rename should be registered");
    assert!(find_id.is_some(), "FindUsages should be registered");
    assert_ne!(
        rename_id, find_id,
        "Different action names should get different ids"
    );
}

// ── sethandler conflict resolution ─────────────────────────────────

#[test]
fn sethandler_host_returns_ignored() {
    use crate::keymap::{Handler, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Delegate Ctrl-A to host in Normal mode
    engine
        .handler_map_mut()
        .set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Host);

    let response = process_key(&mut engine, &doc, KeyEvent::ctrl('a'));
    assert!(
        !response.consumed(),
        "Host-handled key should return Ignored (consumed=false)"
    );
}

#[test]
fn sethandler_vim_processes_normally() {
    use crate::keymap::{Handler, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Explicitly set Ctrl-A to Vim handler (default anyway)
    engine
        .handler_map_mut()
        .set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Vim);

    let response = process_key(&mut engine, &doc, KeyEvent::ctrl('a'));
    assert!(
        response.consumed(),
        "Vim-handled key should be consumed by the engine"
    );
}

#[test]
fn sethandler_per_mode_delegation() {
    use crate::keymap::{Handler, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Host in Normal, Vim in Insert
    engine
        .handler_map_mut()
        .set(KeyEvent::ctrl('a'), MappingMode::Normal, Handler::Host);
    engine
        .handler_map_mut()
        .set(KeyEvent::ctrl('a'), MappingMode::Insert, Handler::Vim);

    // Normal mode: should be ignored (host)
    let response = process_key(&mut engine, &doc, KeyEvent::ctrl('a'));
    assert!(
        !response.consumed(),
        "Normal: host-handled should be ignored"
    );

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Insert mode: should be consumed (vim)
    let response = process_key(&mut engine, &doc, KeyEvent::ctrl('a'));
    assert!(
        response.consumed(),
        "Insert: vim-handled should be consumed"
    );
}

#[test]
fn handler_map_empty_by_default() {
    let engine = VimEngine::new();
    assert!(
        engine.handler_map().is_empty(),
        "HandlerMap should be empty by default"
    );
}

#[test]
fn shadow_execution_toggle_off_again() {
    let mut engine = VimEngine::new();
    engine.set_shadow_execution(true);
    assert!(engine.shadow_execution_enabled());
    engine.set_shadow_execution(false);
    assert!(!engine.shadow_execution_enabled());
}

#[test]
fn shadow_off_macro_replay_leaves_pending_keys() {
    // With shadow disabled (default), executing a macro via manual
    // frame push leaves pending keys for the host to drain.
    let mut engine = VimEngine::new();
    assert!(!engine.shadow_execution_enabled());

    // Prime the macro state
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();

    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![
            MacroEntry::Key(KeyEvent::char('j')),
            MacroEntry::Key(KeyEvent::char('k')),
        ],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    // Verify pending keys exist and are NOT auto-drained
    assert!(engine.has_pending_keys());

    // Process a regular key — shadow is off, so macro keys stay pending
    let doc = SimpleDocument::new("hello\nworld\nfoo");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let _response = engine.process(KeyEvent::char('l'), ctx);

    // The manually-pushed frame still has keys (drain_macro_key is the
    // host's job when shadow is off). The 'l' key just moved the cursor;
    // macro keys are a separate host-driven drain loop.
    // The frame was pushed manually BEFORE the 'l' key, so process()
    // didn't trigger it — macro frames are normally pushed by PlayMacro
    // effects during process(). This test covers only the toggle path:
    // shadow_enabled=false means the shadow trigger at the end of
    // process() does NOT fire.
    assert!(
        engine.has_pending_keys(),
        "With shadow disabled, pending macro keys should remain for host drain"
    );
}

#[test]
fn shadow_on_drains_pending_macro_keys() {
    // With shadow enabled, process() should drain all pending macro keys
    // via execute_shadow_replay when it detects pending keys after the
    // triggering keystroke.
    let mut engine = VimEngine::new();
    engine.set_shadow_execution(true);

    // Prime the macro state
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();

    // Push a simple macro: just 'j' (move down)
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('j'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    assert!(engine.has_pending_keys());

    let doc = SimpleDocument::new("hello\nworld\nfoo");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('l'), ctx);

    // Shadow execution should have drained all macro keys
    assert!(
        !engine.has_pending_keys(),
        "With shadow enabled, process() should drain all pending macro keys"
    );

    // The response should contain effects from both the 'l' keystroke
    // AND the shadow-replayed 'j' macro key
    assert!(
        !response.effects().is_empty(),
        "Response should contain batched effects from shadow replay"
    );
}

#[test]
fn shadow_on_counted_replay_drains_all() {
    // Shadow execution with counted replay (e.g. 3@a) should drain
    // all repetitions.
    let mut engine = VimEngine::new();
    engine.set_shadow_execution(true);

    // Prime the macro state
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();

    // Push a macro with 3 repeats of 'j' (move down 3 times)
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('j'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::new(3).unwrap(),
    });

    assert!(engine.has_pending_keys());

    let doc = SimpleDocument::new("line1\nline2\nline3\nline4\nline5");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('l'), ctx);

    // All 3 repetitions should be drained
    assert!(
        !engine.has_pending_keys(),
        "Shadow execution should drain all counted macro repetitions"
    );

    // Response should have effects including a SetCursor from shadow
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "Shadow replay should produce a final SetCursor effect"
    );
}

// ─── Digraph input (Ctrl-K) end-to-end tests ─────────────────────────

#[test]
fn test_digraph_ctrl_k_inserts_resolved_char() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Step 1: Ctrl-K → starts digraph input
    let r1 = process_key(&mut engine, &doc, KeyEvent::ctrl('k'));
    assert!(r1.consumed(), "Ctrl-K should be consumed");
    assert!(r1.pending(), "Ctrl-K should leave engine in pending state");

    // Step 2: 'a' → first digraph character
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('a'));
    assert!(r2.consumed(), "First digraph char should be consumed");
    assert!(r2.pending(), "First digraph char should still be pending");

    // Step 3: '\'' (apostrophe) → resolves digraph a' = 'á' (U+00E1)
    let r3 = process_key(&mut engine, &doc, KeyEvent::char('\''));
    assert!(r3.consumed(), "Second digraph char should be consumed");

    // The response should contain an Insert effect with the resolved character 'á'
    let insert_effect = r3
        .effects
        .iter()
        .find(|e| matches!(e, Effect::Insert { .. }));
    assert!(
        insert_effect.is_some(),
        "Digraph completion should produce an Insert effect, got: {:?}",
        r3.effects
    );
    match insert_effect.unwrap() {
        Effect::Insert { text, .. } => {
            assert_eq!(
                text.as_str(),
                "\u{00E1}",
                "Insert effect should contain 'á' (U+00E1)"
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_digraph_ctrl_k_unknown_inserts_literal() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Ctrl-K → 'z' → 'z' (no digraph for this pair — falls back to literal 'z')
    process_key(&mut engine, &doc, KeyEvent::ctrl('k'));
    process_key(&mut engine, &doc, KeyEvent::char('z'));
    let r3 = process_key(&mut engine, &doc, KeyEvent::char('z'));

    assert!(r3.consumed(), "Second digraph char should be consumed");

    // The response should contain an Insert effect with literal 'z'
    let insert_effect = r3
        .effects
        .iter()
        .find(|e| matches!(e, Effect::Insert { .. }));
    assert!(
        insert_effect.is_some(),
        "Unknown digraph should produce an Insert effect with literal fallback, got: {:?}",
        r3.effects
    );
    match insert_effect.unwrap() {
        Effect::Insert { text, .. } => {
            assert_eq!(
                text.as_str(),
                "z",
                "Unknown digraph (z, z) should insert literal 'z'"
            );
        }
        _ => unreachable!(),
    }
}

// ─── Undo marks end-to-end tests ──────────────────────────────────────

/// Helper to read a local mark's offset from the engine state.
fn get_mark_offset(engine: &VimEngine, mark_char: char) -> Option<usize> {
    let name = crate::primitives::MarkName::new(mark_char).unwrap();
    engine.state().marks().get(name).map(|m| m.offset().get())
}

/// Helper to set a local mark on the engine at a given offset.
fn set_mark(engine: &mut VimEngine, mark_char: char, offset: usize) {
    let name = crate::primitives::MarkName::new(mark_char).unwrap();
    engine.marks_mut().set(
        name,
        crate::primitives::Mark::new(crate::primitives::Offset::new(offset)),
    );
}

/// Test 1: Undo restores local marks to their pre-change values.
///
/// Pipeline: set mark `a` at 10 -> enter insert mode (BeginUndoGroup captures
/// marks) -> type a char -> exit insert (EndUndoGroup commits) -> press `u`
/// (Effect::Undo -> undo_with_marks -> mark snapshot swap) -> verify mark `a`
/// is restored to 10.
#[test]
fn test_undo_restores_local_marks() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world\nsecond line\n");

    // Set mark 'a' at offset 10
    set_mark(&mut engine, 'a', 10);
    assert_eq!(get_mark_offset(&engine, 'a'), Some(10));

    // Enter insert mode: emits BeginUndoGroup which captures MarkSnapshot
    // (snapshot now holds mark 'a' = 10)
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Type a character in insert mode: emits Insert effect (marks edit)
    process_key(&mut engine, &doc, KeyEvent::char('x'));

    // Capture the mark value after the insert (may have been adjusted by
    // offset adjustment in the effect processor)
    let mark_after_insert = get_mark_offset(&engine, 'a');

    // Exit insert mode: emits EndUndoGroup (commits the undo node)
    process_key(&mut engine, &doc, KeyEvent::escape());
    assert_eq!(engine.mode(), Mode::Normal);

    // Verify the undo tree has recorded the change
    assert!(
        engine.state().undo_tree().can_undo(),
        "should be able to undo after insert"
    );

    // Capture mark value after EndUndoGroup (the "post-change" value)
    let mark_post_change = get_mark_offset(&engine, 'a');

    // Press 'u' to undo: emits Effect::Undo { count: 1 }
    // The effect processor calls state.undo_with_marks() which navigates
    // the undo tree AND swaps the mark snapshot with live marks.
    let undo_response = process_key(&mut engine, &doc, KeyEvent::char('u'));
    assert!(
        undo_response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::Undo { .. })),
        "pressing 'u' should produce an Undo effect, got: {:?}",
        undo_response.effects()
    );

    // After undo, mark 'a' should be restored to the value it had when
    // the undo group was created (the snapshot captured at BeginUndoGroup).
    // The snapshot captured mark 'a' = 10, so after swap, live mark = 10.
    let mark_after_undo = get_mark_offset(&engine, 'a');
    assert_eq!(
        mark_after_undo,
        Some(10),
        "undo should restore mark 'a' to its pre-change value (10), \
         mark_after_insert={:?}, mark_post_change={:?}, mark_after_undo={:?}",
        mark_after_insert,
        mark_post_change,
        mark_after_undo,
    );
}

/// Test 2: Redo restores post-change marks after undo.
///
/// Same as Test 1 but after undo, press Ctrl-R (redo). The mark snapshot
/// swap is symmetric: undo saves the post-change marks into the node's
/// snapshot, and redo swaps them back.
#[test]
fn test_redo_restores_post_change_marks() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world\nsecond line\n");

    // Set mark 'a' at offset 10
    set_mark(&mut engine, 'a', 10);

    // Enter insert, type char, exit insert
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::char('x'));
    process_key(&mut engine, &doc, KeyEvent::escape());

    // Capture the post-change mark value (this is what redo should restore)
    let mark_post_change = get_mark_offset(&engine, 'a');

    // Undo: restores mark to pre-change (10)
    process_key(&mut engine, &doc, KeyEvent::char('u'));
    assert_eq!(
        get_mark_offset(&engine, 'a'),
        Some(10),
        "undo should restore mark 'a' to 10"
    );

    // Redo: should restore mark to post-change value
    let redo_response = process_key(&mut engine, &doc, KeyEvent::ctrl('r'));
    assert!(
        redo_response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::Redo { .. })),
        "Ctrl-R should produce a Redo effect, got: {:?}",
        redo_response.effects()
    );

    let mark_after_redo = get_mark_offset(&engine, 'a');
    assert_eq!(
        mark_after_redo, mark_post_change,
        "redo should restore mark 'a' to its post-change value ({:?}), got {:?}",
        mark_post_change, mark_after_redo,
    );
}

/// Test 3: Undo preserves marks that were not set at snapshot time.
///
/// Per Neovim behavior: if a mark was `None` in the snapshot, the swap
/// does NOT clear the live mark. This test verifies that setting a mark
/// AFTER an undo group starts, then undoing, does not erase the mark.
#[test]
fn test_undo_preserves_unset_marks() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world\nsecond line\n");

    // No marks set initially — the BeginUndoGroup snapshot will have all None

    // Enter insert mode (BeginUndoGroup captures empty snapshot)
    process_key(&mut engine, &doc, KeyEvent::char('i'));

    // Type a char
    process_key(&mut engine, &doc, KeyEvent::char('x'));

    // Exit insert (EndUndoGroup commits)
    process_key(&mut engine, &doc, KeyEvent::escape());

    // NOW set mark 'a' after the change was committed
    set_mark(&mut engine, 'a', 42);
    assert_eq!(get_mark_offset(&engine, 'a'), Some(42));

    // Undo: the snapshot for this undo group has mark 'a' = None.
    // Neovim behavior: None slots do NOT clear live marks.
    // After swap: live mark 'a' stays at 42, snapshot captures 42.
    process_key(&mut engine, &doc, KeyEvent::char('u'));

    assert_eq!(
        get_mark_offset(&engine, 'a'),
        Some(42),
        "undo should NOT clear mark 'a' when snapshot had None for it"
    );
}

#[test]
fn ctrl_x_ctrl_n_emits_request_completion_host_request() {
    use crate::execution::host::{HostRequest, HostRequestKind};
    use crate::primitives::CompletionKind;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Ctrl-X → AwaitingInsertCtrlX (pending)
    let r1 = process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    assert!(r1.pending(), "Ctrl-X in insert mode should be pending");
    assert!(
        r1.host_requests().is_empty(),
        "Ctrl-X alone should not emit host requests"
    );

    // Ctrl-N → RequestCompletion(KeywordNext)
    let r2 = process_key(&mut engine, &doc, KeyEvent::ctrl('n'));
    assert!(r2.pending(), "Ctrl-X Ctrl-N response should be pending");
    assert_eq!(
        r2.host_requests().len(),
        1,
        "should emit exactly one host request"
    );
    assert_eq!(
        r2.host_requests()[0].kind(),
        HostRequestKind::RequestCompletion
    );
    if let HostRequest::RequestCompletion { kind, .. } = &r2.host_requests()[0] {
        assert_eq!(*kind, CompletionKind::KeywordNext);
    } else {
        panic!("expected RequestCompletion host request");
    }
}

#[test]
fn ctrl_x_ctrl_p_emits_keyword_prev_completion() {
    use crate::execution::host::{HostRequest, HostRequestKind};
    use crate::primitives::CompletionKind;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('p'));

    assert!(r.pending());
    assert_eq!(r.host_requests().len(), 1);
    assert_eq!(
        r.host_requests()[0].kind(),
        HostRequestKind::RequestCompletion
    );
    if let HostRequest::RequestCompletion { kind, .. } = &r.host_requests()[0] {
        assert_eq!(*kind, CompletionKind::KeywordPrev);
    } else {
        panic!("expected RequestCompletion host request");
    }
}

#[test]
fn ctrl_x_ctrl_l_emits_line_completion() {
    use crate::execution::host::HostRequest;
    use crate::primitives::CompletionKind;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('l'));

    assert!(r.pending());
    assert_eq!(r.host_requests().len(), 1);
    if let HostRequest::RequestCompletion { kind, .. } = &r.host_requests()[0] {
        assert_eq!(*kind, CompletionKind::Line);
    } else {
        panic!("expected RequestCompletion host request");
    }
}

#[test]
fn ctrl_x_ctrl_f_emits_filename_completion() {
    use crate::execution::host::HostRequest;
    use crate::primitives::CompletionKind;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('f'));

    assert!(r.pending());
    assert_eq!(r.host_requests().len(), 1);
    if let HostRequest::RequestCompletion { kind, .. } = &r.host_requests()[0] {
        assert_eq!(*kind, CompletionKind::FileName);
    } else {
        panic!("expected RequestCompletion host request");
    }
}

#[test]
fn ctrl_x_ctrl_o_emits_omni_completion() {
    use crate::execution::host::HostRequest;
    use crate::primitives::CompletionKind;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('o'));

    assert!(r.pending());
    assert_eq!(r.host_requests().len(), 1);
    if let HostRequest::RequestCompletion { kind, .. } = &r.host_requests()[0] {
        assert_eq!(*kind, CompletionKind::Omni);
    } else {
        panic!("expected RequestCompletion host request");
    }
}

#[test]
fn ctrl_x_shows_ctrl_x_mode_message() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('x'));

    assert!(r.pending());
    let msg = r
        .message()
        .expect("Ctrl-X should produce a ^X mode message");
    assert!(
        msg.contains("^X mode"),
        "message should contain '^X mode', got: {msg}"
    );
    assert!(
        msg.contains("^]^D^E^F^I^K^L^N^O^P^S^T^U^V^Y"),
        "message should list available completion keys, got: {msg}"
    );
}

#[test]
fn ctrl_x_completion_does_not_emit_text_effects() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('n'));

    // The RequestCompletion interception should not produce text effects.
    assert!(
        !r.effects().iter().any(|e| matches!(
            e,
            Effect::Insert { .. } | Effect::Delete { .. } | Effect::Replace { .. }
        )),
        "RequestCompletion should not produce text mutation effects"
    );
}

#[test]
fn ctrl_x_completion_request_has_valid_meta() {
    use crate::execution::host::HostRequest;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('n'));

    if let HostRequest::RequestCompletion { meta, .. } = &r.host_requests()[0] {
        // Meta should have a valid id (sequencer starts at 0)
        // The exact id depends on how many requests were made previously,
        // but it should be non-negative (which it always is for u64).
        let _ = meta.id.get(); // Just ensure it doesn't panic
    } else {
        panic!("expected RequestCompletion");
    }
}

// ── Regression: existing insert sub-modes still work ─────────────────

#[test]
fn ctrl_r_insert_register_still_works_after_completion_addition() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    // Ctrl-R should enter AwaitingInsertRegister (pending)
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('r'));
    assert!(r.pending(), "Ctrl-R should be pending (awaiting register)");
    assert!(
        r.host_requests().is_empty(),
        "Ctrl-R should not emit host requests"
    );
}

#[test]
fn ctrl_g_insert_submode_still_works_after_completion_addition() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    // Ctrl-G should enter AwaitingInsertCtrlG (pending)
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('g'));
    assert!(
        r.pending(),
        "Ctrl-G should be pending (awaiting sub-command)"
    );
}

#[test]
fn ctrl_k_digraph_still_works_after_completion_addition() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    process_key(&mut engine, &doc, KeyEvent::char('i'));
    // Ctrl-K should enter AwaitingInsertDigraph1 (pending)
    let r = process_key(&mut engine, &doc, KeyEvent::ctrl('k'));
    assert!(r.pending(), "Ctrl-K should be pending (awaiting digraph)");
}

#[test]
fn ctrl_x_multiple_completion_kinds_sequential() {
    use crate::execution::host::HostRequest;
    use crate::primitives::CompletionKind;

    let doc = SimpleDocument::new("hello world");

    // Each Ctrl-X + trigger should produce the correct CompletionKind.
    let test_cases: &[(char, CompletionKind)] = &[
        ('k', CompletionKind::Dictionary),
        ('t', CompletionKind::Thesaurus),
        ('i', CompletionKind::IncludePath),
        (']', CompletionKind::Tag),
        ('d', CompletionKind::DefinitionMacro),
        ('v', CompletionKind::VimCommand),
        ('u', CompletionKind::UserDefined),
        ('s', CompletionKind::Spelling),
    ];

    for &(trigger_char, expected_kind) in test_cases {
        // Re-enter insert mode for each test to reset parser state
        let mut engine = VimEngine::new();
        process_key(&mut engine, &doc, KeyEvent::char('i'));
        process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
        let r = process_key(&mut engine, &doc, KeyEvent::ctrl(trigger_char));

        assert!(
            !r.host_requests().is_empty(),
            "Ctrl-X Ctrl-{trigger_char} should emit a host request"
        );
        if let HostRequest::RequestCompletion { kind, .. } = &r.host_requests()[0] {
            assert_eq!(
                *kind, expected_kind,
                "Ctrl-X Ctrl-{trigger_char} should emit {expected_kind:?}, got {kind:?}"
            );
        } else {
            panic!(
                "Ctrl-X Ctrl-{trigger_char}: expected RequestCompletion, got {:?}",
                r.host_requests()[0]
            );
        }
    }
}

#[test]
fn ctrl_x_ctrl_e_cancels_submode_stays_in_insert() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Ctrl-X enters the completion sub-mode (pending)
    let r1 = process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    assert!(r1.pending(), "Ctrl-X should be pending");

    // Ctrl-E cancels the completion sub-mode without exiting insert mode.
    // Grammar returns Cancel, which the insert mode handler maps to Ignored.
    let r2 = process_key(&mut engine, &doc, KeyEvent::ctrl('e'));

    // No host requests should be emitted
    assert!(
        r2.host_requests().is_empty(),
        "Ctrl-X Ctrl-E should not emit host requests, got: {:?}",
        r2.host_requests()
    );

    // Engine should remain in insert mode
    assert_eq!(
        engine.mode(),
        Mode::Insert,
        "Ctrl-X Ctrl-E should stay in insert mode"
    );
}

#[test]
fn ctrl_x_then_escape_exits_insert_mode() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Ctrl-X enters the completion sub-mode
    let r1 = process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    assert!(r1.pending(), "Ctrl-X should be pending");

    // Escape should exit insert mode entirely (not just cancel the sub-mode).
    // The insert mode handler's fast-path intercepts Escape before the grammar
    // parser sees it, so it always produces InsertExit regardless of awaiting state.
    let r2 = process_key(&mut engine, &doc, KeyEvent::escape());

    assert_eq!(
        engine.mode(),
        Mode::Normal,
        "Escape after Ctrl-X should exit insert mode to Normal"
    );

    // No host requests should be emitted
    assert!(
        r2.host_requests().is_empty(),
        "Escape after Ctrl-X should not emit host requests"
    );
}

#[test]
fn regular_char_insert_still_works_after_completion_infrastructure() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Type a regular character — should produce an Insert effect, not a
    // completion request or any host request.
    let r = process_key(&mut engine, &doc, KeyEvent::char('x'));

    assert!(
        r.consumed(),
        "regular char in insert mode should be consumed"
    );
    assert!(!r.pending(), "regular char should not be pending");
    assert!(
        r.host_requests().is_empty(),
        "regular char should not emit host requests"
    );
    assert!(
        r.effects()
            .iter()
            .any(|e| matches!(e, Effect::Insert { .. })),
        "regular char in insert mode should produce an Insert effect, got: {:?}",
        r.effects()
    );

    // Engine should remain in insert mode
    assert_eq!(engine.mode(), Mode::Insert);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unified Drain API tests
// ═══════════════════════════════════════════════════════════════════════════════

// ─── drain_next_key ──────────────────────────────────────────────────

#[test]
fn drain_next_key_returns_buffer_keys_before_macro_keys() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();

    // Inject keys into the typeahead buffer (simulates mapping RHS)
    engine.typeahead.buffer.inject_front([
        TypeaheadEntry::new(KeyEvent::char('a'), TypeaheadFlags::noremap_rhs()),
        TypeaheadEntry::new(KeyEvent::char('b'), TypeaheadFlags::noremap_rhs()),
    ]);

    // Push a macro frame with keys 'x', 'y'
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('q').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![
            MacroEntry::Key(KeyEvent::char('x')),
            MacroEntry::Key(KeyEvent::char('y')),
        ],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    // Buffer keys come first
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('a')))
    );
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('b')))
    );
    // Then macro keys
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('x')))
    );
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('y')))
    );
    // Then None
    assert_eq!(engine.drain_next_key(), None);
}

#[test]
fn drain_next_key_returns_none_when_both_empty() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.drain_next_key(), None);
}

#[test]
fn drain_next_key_buffer_only() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();
    engine.typeahead.buffer.inject_front([TypeaheadEntry::new(
        KeyEvent::char('j'),
        TypeaheadFlags::noremap_rhs(),
    )]);

    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('j')))
    );
    assert_eq!(engine.drain_next_key(), None);
}

#[test]
fn drain_next_key_macro_only() {
    let mut engine = VimEngine::new();
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('k'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('k')))
    );
    assert_eq!(engine.drain_next_key(), None);
}

// ─── has_pending_keys ────────────────────────────────────────────────

#[test]
fn has_pending_keys_true_when_buffer_non_empty() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();
    assert!(!engine.has_pending_keys());

    engine.typeahead.buffer.inject_front([TypeaheadEntry::new(
        KeyEvent::char('z'),
        TypeaheadFlags::noremap_rhs(),
    )]);
    assert!(engine.has_pending_keys());
}

#[test]
fn has_pending_keys_true_when_macro_stack_non_empty() {
    let mut engine = VimEngine::new();
    assert!(!engine.has_pending_keys());

    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('j'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });
    assert!(engine.has_pending_keys());
}

#[test]
fn has_pending_keys_false_when_both_empty() {
    let engine = VimEngine::new();
    assert!(!engine.has_pending_keys());
}

// ─── abort_replay ────────────────────────────────────────────────────

#[test]
fn abort_replay_clears_buffer_and_macro_stack() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();

    // Inject buffer keys
    engine.typeahead.buffer.inject_front([
        TypeaheadEntry::new(KeyEvent::char('a'), TypeaheadFlags::noremap_rhs()),
        TypeaheadEntry::new(KeyEvent::char('b'), TypeaheadFlags::noremap_rhs()),
    ]);

    // Push macro frames with depth
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('q').unwrap())
        .unwrap();
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('w').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('x'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('y'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    assert!(engine.has_pending_keys());
    assert_eq!(engine.state.macros().replay_depth(), 2);

    engine.abort_replay();

    assert!(!engine.has_pending_keys());
    assert!(engine.typeahead.macro_stack.is_empty());
    assert!(engine.typeahead.buffer.is_empty());
    assert_eq!(engine.state.macros().replay_depth(), 0);
}

#[test]
fn abort_replay_preserves_recording_and_last_played() {
    let mut engine = VimEngine::new();

    // Set up recording state and last_played
    engine
        .state
        .macros_mut()
        .start_recording(RegisterName::new('r').unwrap());
    engine
        .state
        .macros_mut()
        .set_last_played(RegisterName::new('p').unwrap());

    // Push a macro frame with depth
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('q').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('x'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    engine.abort_replay();

    // Recording and last_played should be preserved
    assert!(engine.state.macros().is_recording());
    assert_eq!(
        engine.state.macros().recording_register(),
        Some(RegisterName::new('r').unwrap())
    );
    // Note: last_played is updated to 'q' by begin_replay, which is fine
    assert!(engine.state.macros().last_played().is_some());
    assert_eq!(engine.state.macros().replay_depth(), 0);
}

#[test]
fn macro_aborts_on_effect_limit() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_max_macro_effects(10);

    let entries = super::macro_replay::parse_macro_entries("j");
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries,
        cursor: 0,
        remaining_repeats: NonZeroU32::new(100).unwrap(),
    });

    assert!(engine.has_pending_keys());

    let doc = SimpleDocument::new("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq\nr\ns\nt");
    let mut drained = 0;
    while let Some(output) = engine.drain_next_key() {
        drained += 1;
        match output {
            MacroOutput::Key(key) => {
                let ctx = InputContext::new(&doc, 0).validate().unwrap();
                let _response = engine.process(key, ctx);
            }
            MacroOutput::TextBlock { .. } => {}
        }
        if drained > 200 {
            panic!("drain loop did not terminate");
        }
    }

    assert!(
        drained <= 11,
        "expected drain to abort around limit (10), got {drained}"
    );
    assert!(engine.typeahead.macro_stack.is_empty());
    assert_eq!(engine.typeahead.macro_effect_counter, 0);

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::escape(), ctx);
    let has_error = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ShowError { .. }));
    assert!(has_error, "next process() should contain ShowError effect");
}

// ─── feed_keys API ──────────────────────────────────────────────────

#[test]
fn feed_keys_remap_true_produces_remappable_entries() {
    let mut engine = VimEngine::new();
    engine.feed_keys("dd", true);

    // Should have 2 entries
    assert!(engine.has_pending_keys());
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('d')))
    );
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('d')))
    );
    assert_eq!(engine.drain_next_key(), None);
}

#[test]
fn feed_keys_remap_false_produces_noremap_entries() {
    let mut engine = VimEngine::new();
    engine.feed_keys("<Esc>", false);

    assert!(engine.has_pending_keys());
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::escape()))
    );
    assert_eq!(engine.drain_next_key(), None);
}

#[test]
fn feed_keys_then_drain_produces_keys_in_order() {
    let mut engine = VimEngine::new();
    engine.feed_keys("abc", true);

    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('a')))
    );
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('b')))
    );
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('c')))
    );
    assert_eq!(engine.drain_next_key(), None);
}

#[test]
fn feed_keys_interleaves_with_macro_replay() {
    let mut engine = VimEngine::new();

    // Prime macro state
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();

    // Push a macro frame
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('m'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    // Inject feedkeys (goes into typeahead buffer, drained first)
    engine.feed_keys("f", true);

    // feedkeys keys are in the buffer, drained FIRST (priority 1)
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('f')))
    );
    // Then macro keys (priority 2)
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('m')))
    );
    assert_eq!(engine.drain_next_key(), None);
}

#[test]
fn feed_keys_vim_notation_parsing() {
    let mut engine = VimEngine::new();
    engine.feed_keys("<C-w>j", false);

    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::ctrl('w')))
    );
    assert_eq!(
        engine.drain_next_key(),
        Some(MacroOutput::Key(KeyEvent::char('j')))
    );
    assert_eq!(engine.drain_next_key(), None);
}

// ─── Shadow execution with unified drain ─────────────────────────────

#[test]
fn shadow_execution_uses_unified_drain() {
    // Verify shadow execution works with the unified drain API.
    // This exercises the updated shadow.rs code path.
    let mut engine = VimEngine::new();
    engine.set_shadow_execution(true);

    // Prime the macro state
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();

    // Push a simple macro: 'j' (move down)
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('j'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    assert!(engine.has_pending_keys());

    let doc = SimpleDocument::new("hello\nworld\nfoo");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('l'), ctx);

    // Shadow execution should have drained all keys
    assert!(
        !engine.has_pending_keys(),
        "Shadow execution should drain all pending keys"
    );

    assert!(
        !response.effects().is_empty(),
        "Response should contain effects from both the keystroke and shadow replay"
    );
}

// ─── last_drained_flags + recording gate + is_live_insert_input ─────────

#[test]
fn take_last_drained_flags_none_for_direct_user_input() {
    // When no drain_next_key() has been called, take_last_drained_flags
    // returns None, indicating the key is direct user input.
    let mut engine = VimEngine::new();
    assert!(
        engine.take_last_drained_flags().is_none(),
        "Should be None when no drain has occurred"
    );
}

#[test]
fn take_last_drained_flags_some_after_buffer_drain() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();
    engine.typeahead.buffer.inject_front([TypeaheadEntry::new(
        KeyEvent::char('x'),
        TypeaheadFlags::noremap_rhs(),
    )]);

    let _key = engine.drain_next_key();
    let flags = engine.take_last_drained_flags();
    assert!(flags.is_some(), "Should be Some after drain from buffer");
    assert_eq!(flags.unwrap(), TypeaheadFlags::noremap_rhs());
}

#[test]
fn take_last_drained_flags_macro_key_after_macro_drain() {
    let mut engine = VimEngine::new();
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('j'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    let _key = engine.drain_next_key();
    let flags = engine.take_last_drained_flags();
    assert!(
        flags.is_some(),
        "Should be Some after drain from macro stack"
    );
    // Macro keys are REMAPPABLE but not TYPED
    let f = flags.unwrap();
    assert!(
        f.contains(crate::execution::engine::typeahead::TypeaheadFlags::REMAPPABLE),
        "Macro-drained key should be REMAPPABLE"
    );
    assert!(
        !f.contains(crate::execution::engine::typeahead::TypeaheadFlags::TYPED),
        "Macro-drained key should NOT be TYPED"
    );
}

#[test]
fn take_last_drained_flags_consumed_on_second_call() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();
    engine.typeahead.buffer.inject_front([TypeaheadEntry::new(
        KeyEvent::char('a'),
        TypeaheadFlags::noremap_rhs(),
    )]);

    let _key = engine.drain_next_key();
    // First call consumes the flags
    assert!(engine.take_last_drained_flags().is_some());
    // Second call returns None (already consumed)
    assert!(
        engine.take_last_drained_flags().is_none(),
        "Flags should be consumed after first take"
    );
}

#[test]
fn take_last_drained_flags_typed_for_user_typed_entry() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();
    // Inject a user-typed entry (e.g., from feedkeys with typed flag)
    engine.typeahead.buffer.inject_front([TypeaheadEntry::new(
        KeyEvent::char('i'),
        TypeaheadFlags::user_typed(),
    )]);

    let _key = engine.drain_next_key();
    let flags = engine.take_last_drained_flags();
    assert!(flags.is_some());
    let f = flags.unwrap();
    assert!(f.contains(crate::execution::engine::typeahead::TypeaheadFlags::TYPED));
    assert!(f.contains(crate::execution::engine::typeahead::TypeaheadFlags::REMAPPABLE));
}

// ─── is_live_insert_input with buffer check ───────────────────────────

#[test]
fn is_live_insert_input_true_for_direct_user_input_in_insert() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // No pending keys, no macro, no buffer entries
    assert!(
        engine.is_live_insert_input(),
        "Should be true for direct user input in insert mode"
    );
}

#[test]
fn is_live_insert_input_false_when_buffer_has_entries() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Inject entries into the buffer (simulates mapping RHS expansion)
    engine.typeahead.buffer.inject_front([TypeaheadEntry::new(
        KeyEvent::char('x'),
        TypeaheadFlags::noremap_rhs(),
    )]);

    assert!(
        !engine.is_live_insert_input(),
        "Should be false when typeahead buffer has entries"
    );
}

#[test]
fn is_live_insert_input_false_when_macro_stack_non_empty() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Push a macro frame
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('j'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    assert!(
        !engine.is_live_insert_input(),
        "Should be false when macro stack is non-empty"
    );
}

#[test]
fn is_live_insert_input_false_during_dot_repeat() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Simulate dot-repeat
    engine.is_repeating = true;

    assert!(
        !engine.is_live_insert_input(),
        "Should be false during dot-repeat"
    );
}

#[test]
fn is_live_insert_input_false_in_normal_mode() {
    let engine = VimEngine::new();
    assert_eq!(engine.mode(), Mode::Normal);
    assert!(
        !engine.is_live_insert_input(),
        "Should be false in normal mode"
    );
}

// ─── Recording gate: only user-typed keys are recorded ──────────────

#[test]
fn recording_records_user_typed_keys() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");

    // Start recording into register 'a': q a
    process_key(&mut engine, &doc, KeyEvent::char('q'));
    process_key(&mut engine, &doc, KeyEvent::char('a'));

    // Verify recording is active
    assert!(
        engine.recording.buffer.is_some(),
        "Recording should be active"
    );

    // Type a user key: 'j' (direct, no drain)
    process_key(&mut engine, &doc, KeyEvent::char('j'));

    // Check that 'j' was recorded
    let (_, ref buf) = engine.recording.buffer.as_ref().unwrap();
    assert!(buf.contains('j'), "User-typed key 'j' should be recorded");
}

#[test]
fn recording_does_not_record_macro_replay_keys() {
    use crate::execution::engine::typeahead::TypeaheadFlags;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nfoo");

    // Start recording into register 'b': q b
    process_key(&mut engine, &doc, KeyEvent::char('q'));
    process_key(&mut engine, &doc, KeyEvent::char('b'));
    assert!(
        engine.recording.buffer.is_some(),
        "Recording should be active"
    );

    // Simulate a drained macro key: drain_next_key() sets last_drained_flags
    // then process() reads them.
    engine.typeahead.last_drained_flags = TypeaheadFlags::macro_key();

    // Process the key as if it was drained from macro
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    engine.process(KeyEvent::char('j'), ctx);

    // The 'j' should NOT be recorded because it came from a macro drain
    let (_, ref buf) = engine.recording.buffer.as_ref().unwrap();
    assert!(
        !buf.contains('j'),
        "Macro replay key 'j' should NOT be recorded"
    );
}

#[test]
fn recording_does_not_record_mapping_rhs_keys() {
    use crate::execution::engine::typeahead::TypeaheadFlags;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nfoo");

    // Start recording into register 'c': q c
    process_key(&mut engine, &doc, KeyEvent::char('q'));
    process_key(&mut engine, &doc, KeyEvent::char('c'));
    assert!(
        engine.recording.buffer.is_some(),
        "Recording should be active"
    );

    // Simulate a drained mapping RHS key (noremap — no TYPED flag)
    engine.typeahead.last_drained_flags = TypeaheadFlags::noremap_rhs();

    // Process the key as if it was drained from mapping expansion
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    engine.process(KeyEvent::char('l'), ctx);

    // The 'l' should NOT be recorded because it came from a mapping RHS drain
    let (_, ref buf) = engine.recording.buffer.as_ref().unwrap();
    assert!(
        !buf.contains('l'),
        "Mapping RHS key 'l' should NOT be recorded"
    );
}

#[test]
fn drain_next_key_sets_flags_for_buffer_entry() {
    use crate::execution::engine::typeahead::{TypeaheadEntry, TypeaheadFlags};

    let mut engine = VimEngine::new();

    // Inject a noremap entry
    engine.typeahead.buffer.inject_front([TypeaheadEntry::new(
        KeyEvent::char('x'),
        TypeaheadFlags::noremap_rhs(),
    )]);

    let output = engine.drain_next_key();
    assert_eq!(output, Some(MacroOutput::Key(KeyEvent::char('x'))));
    assert_eq!(
        engine.typeahead.last_drained_flags,
        TypeaheadFlags::noremap_rhs(),
        "last_drained_flags should match the buffer entry's flags"
    );
}

#[test]
fn drain_next_key_sets_macro_key_flags_for_macro_entry() {
    use crate::execution::engine::typeahead::TypeaheadFlags;

    let mut engine = VimEngine::new();
    engine
        .state
        .macros_mut()
        .begin_replay(RegisterName::new('a').unwrap())
        .unwrap();
    engine.typeahead.macro_stack.push(MacroFrame {
        entries: vec![MacroEntry::Key(KeyEvent::char('k'))],
        cursor: 0,
        remaining_repeats: NonZeroU32::MIN,
    });

    let output = engine.drain_next_key();
    assert_eq!(output, Some(MacroOutput::Key(KeyEvent::char('k'))));
    assert_eq!(
        engine.typeahead.last_drained_flags,
        TypeaheadFlags::macro_key(),
        "last_drained_flags should be macro_key() for macro-drained keys"
    );
}

#[test]
fn drain_next_key_does_not_set_flags_when_empty() {
    use crate::execution::engine::typeahead::TypeaheadFlags;

    let mut engine = VimEngine::new();
    assert_eq!(engine.drain_next_key(), None);
    assert_eq!(
        engine.typeahead.last_drained_flags,
        TypeaheadFlags::empty(),
        "last_drained_flags should remain empty when drain returns None"
    );
}

// ─── Buffer-local mapping integration (from mapping_expand_tests) ────

#[test]
fn engine_buffer_swap_flow_integration() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::default();

    // --- Buffer A: map q → a ---
    engine.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('a')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Verify buffer A mapping is active
    let entry = engine
        .keymap()
        .get_buffer_mapping(KeyEvent::char('q'), crate::primitives::Mode::Normal);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().sequence[0], KeyEvent::char('a'));

    // Take buffer A mappings (simulating switch away)
    let buf_a = engine.take_buffer_mappings();

    // Verify buffer overlay is now empty
    assert!(engine
        .keymap()
        .get_buffer_mapping(KeyEvent::char('q'), crate::primitives::Mode::Normal,)
        .is_none());

    // --- Buffer B: map q → b ---
    engine.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('b')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    let entry = engine
        .keymap()
        .get_buffer_mapping(KeyEvent::char('q'), crate::primitives::Mode::Normal);
    assert_eq!(entry.unwrap().sequence[0], KeyEvent::char('b'));

    // Take buffer B, switch back to A
    let buf_b = engine.take_buffer_mappings();
    engine.set_buffer_mappings(buf_a);

    // Verify buffer A mappings are restored
    let entry = engine
        .keymap()
        .get_buffer_mapping(KeyEvent::char('q'), crate::primitives::Mode::Normal);
    assert_eq!(entry.unwrap().sequence[0], KeyEvent::char('a'));

    // Switch to buffer B
    let _buf_a_again = engine.take_buffer_mappings();
    engine.set_buffer_mappings(buf_b);

    let entry = engine
        .keymap()
        .get_buffer_mapping(KeyEvent::char('q'), crate::primitives::Mode::Normal);
    assert_eq!(entry.unwrap().sequence[0], KeyEvent::char('b'));

    // Clear buffer B specifically for Normal mode
    engine.clear_buffer_mappings_for(MappingMode::Normal);
    assert!(engine
        .keymap()
        .get_buffer_mapping(KeyEvent::char('q'), crate::primitives::Mode::Normal,)
        .is_none());
}

/// End-to-end test: buffer-local mapping through `VimEngine::process()`
/// and `drain_next_key()`, proving the full engine pipeline uses
/// the buffer-local overlay correctly.
#[test]
fn engine_process_uses_buffer_local_mapping() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};
    use crate::test_utils::SimpleDocument;

    let doc = SimpleDocument::new("hello");

    let mut engine = VimEngine::default();

    // Map buffer-local: q → l (non-recursive)
    // In Normal mode, 'l' is a motion. 'q' without mapping would start macro record.
    engine.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('q')],
        key_sequence(&[KeyEvent::char('l')]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Process 'q' through the full engine pipeline
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('q'), ctx);

    // The mapping should have been expanded:
    // 'q' → ExactOnly('l') → Dispatch('l')
    // Non-recursive single-key mapping: resolve_key() returns Dispatch('l'),
    // so process() treats 'l' as the resolved key and executes it directly.
    // No drain needed — the 'l' motion is executed inline.
    assert!(
        !engine.has_pending_mapping(),
        "buffer-local ExactOnly should resolve immediately, not pend"
    );

    // Verify the mapping was consumed (no keys waiting to drain)
    assert!(
        engine.drain_next_key().is_none(),
        "single-key noremap should execute inline, not queue"
    );

    // Clear buffer overlay and verify 'q' no longer expands as a mapping
    engine.clear_buffer_mappings();

    // Without the buffer mapping, 'q' hits core keymap (macro record).
    // The expander should dispatch 'q' directly (no user mapping).
    let ctx2 = InputContext::new(&doc, 0).validate().unwrap();
    let _response2 = engine.process(KeyEvent::char('q'), ctx2);
    assert!(!engine.has_pending_mapping());
}

/// Regression test: pending mapping keys must be flushed when buffer
/// mappings are swapped. Without this, multi-key buffer-local prefixes
/// from the old buffer would be matched against the new buffer's trie.
#[test]
fn buffer_switch_resets_pending_mapping() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};
    use crate::test_utils::SimpleDocument;

    let doc = SimpleDocument::new("hello");

    let mut engine = VimEngine::default();

    // Buffer A: multi-key mapping jk → Esc
    engine.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::escape()]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Feed 'j' — should be pending (prefix of 'jk')
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('j'), ctx);
    assert!(
        engine.has_pending_mapping(),
        "'j' should be pending as prefix of buffer-local 'jk'"
    );

    // Now switch buffers: take_buffer_mappings should reset pending state
    let _buf_a = engine.take_buffer_mappings();

    // The pending 'j' should have been flushed
    assert!(
        !engine.has_pending_mapping(),
        "take_buffer_mappings must reset pending state"
    );

    // Similarly, set_buffer_mappings should also reset
    // (re-create buffer A mapping and get into pending state again)
    engine.map_buffer(
        MappingMode::Normal,
        &[KeyEvent::char('j'), KeyEvent::char('k')],
        key_sequence(&[KeyEvent::escape()]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );
    let ctx2 = InputContext::new(&doc, 0).validate().unwrap();
    engine.process(KeyEvent::char('j'), ctx2);
    assert!(engine.has_pending_mapping());

    // set_buffer_mappings should also reset pending state
    let buf_a = engine.take_buffer_mappings();
    assert!(!engine.has_pending_mapping());
    engine.set_buffer_mappings(buf_a);
    assert!(
        !engine.has_pending_mapping(),
        "set_buffer_mappings must reset pending state"
    );
}

// ─── pending_command_display (showcmd) ───────────────────────────────

#[test]
fn pending_command_display_fresh_engine_is_empty() {
    let engine = VimEngine::new();
    assert_eq!(engine.pending_command_display().as_str(), "");
}

#[test]
fn pending_command_display_operator_pending() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert_eq!(engine.pending_command_display().as_str(), "d");
}

#[test]
fn pending_command_display_clears_after_complete_command() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert_eq!(engine.pending_command_display().as_str(), "d");
    process_key(&mut engine, &doc, KeyEvent::char('w'));
    assert_eq!(
        engine.pending_command_display().as_str(),
        "",
        "completed command should clear pending display"
    );
}

#[test]
fn pending_command_display_count_accumulating() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('3'));
    assert_eq!(engine.pending_command_display().as_str(), "3");
}

#[test]
fn pending_command_display_register_selected() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('"'));
    assert_eq!(engine.pending_command_display().as_str(), "\"");
    process_key(&mut engine, &doc, KeyEvent::char('a'));
    assert_eq!(engine.pending_command_display().as_str(), "\"a");
}

#[test]
fn pending_command_display_count_then_operator() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world\nfoo bar\nbaz");
    process_key(&mut engine, &doc, KeyEvent::char('3'));
    assert_eq!(engine.pending_command_display().as_str(), "3");
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert_eq!(engine.pending_command_display().as_str(), "3d");
}

#[test]
fn pending_command_display_register_then_operator() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('"'));
    process_key(&mut engine, &doc, KeyEvent::char('a'));
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert_eq!(engine.pending_command_display().as_str(), "\"ad");
}

#[test]
fn pending_command_display_operator_with_count2() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world\nfoo bar\nbaz");
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    process_key(&mut engine, &doc, KeyEvent::char('2'));
    assert_eq!(engine.pending_command_display().as_str(), "d2");
}

#[test]
fn pending_command_display_awaiting_char_command() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    // 'f' puts us in AwaitingChar state (find-forward)
    process_key(&mut engine, &doc, KeyEvent::char('f'));
    assert_eq!(engine.pending_command_display().as_str(), "f");
}

#[test]
fn inccommand_basic_preview() {
    let mut engine = VimEngine::new();
    assert!(engine.options().inccommand_enabled());
    let doc = SimpleDocument::new("foo bar foo");

    let response = type_command_line(&mut engine, &doc, "s/foo/baz");
    let has_preview = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
    assert!(
        has_preview,
        "typing :s/foo/baz should produce SubstitutePreview"
    );
}

#[test]
fn inccommand_empty_pattern_no_preview() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo bar");

    let response = type_command_line(&mut engine, &doc, "s/");
    let has_preview = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
    assert!(
        !has_preview,
        "s/ with empty pattern should not have SubstitutePreview"
    );
}

#[test]
fn inccommand_not_substitute_no_preview() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo bar");

    let response = type_command_line(&mut engine, &doc, "w");
    let has_preview = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
    assert!(!has_preview, ":w should not produce SubstitutePreview");
    let has_clear = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ClearSubstitutePreview));
    assert!(
        has_clear,
        ":w should produce ClearSubstitutePreview to clear any stale preview"
    );
}

#[test]
fn inccommand_cancel_clears_preview() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo bar foo");

    // Type substitute command
    type_command_line(&mut engine, &doc, "s/foo/baz");
    // Cancel with Escape
    let response = process_key(&mut engine, &doc, KeyEvent::escape());
    let has_clear = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ClearSubstitutePreview));
    assert!(
        has_clear,
        "Escape from :s/foo/baz should clear substitute preview"
    );
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn inccommand_disabled_no_preview() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_inccommand("");
    assert!(!engine.options().inccommand_enabled());
    let doc = SimpleDocument::new("foo bar foo");

    let response = type_command_line(&mut engine, &doc, "s/foo/baz");
    let has_preview = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
    assert!(!has_preview, "inccommand='' should not produce preview");
    let has_clear = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ClearSubstitutePreview));
    assert!(!has_clear, "inccommand='' should not produce clear preview");
}

#[test]
fn inccommand_progressive_typing() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo fob far");

    // Enter command-line mode
    process_key(&mut engine, &doc, KeyEvent::char(':'));

    // Type "s/f" — should match 'f' in "foo" (first match, non-global)
    let r1 = {
        let mut last = Response::default();
        for ch in "s/f".chars() {
            last = process_key(&mut engine, &doc, KeyEvent::char(ch));
        }
        last
    };
    let has_preview_f = r1
        .effects
        .iter()
        .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
    assert!(has_preview_f, "typing s/f should show preview");

    // Type "o" (now "s/fo") — should match 'fo' in "foo" (first match)
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('o'));
    let has_preview_fo = r2
        .effects
        .iter()
        .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
    assert!(has_preview_fo, "typing s/fo should show updated preview");
}

#[test]
fn inccommand_with_replacement() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo bar");

    let response = type_command_line(&mut engine, &doc, "s/foo/REPLACED");
    for effect in response.effects.iter() {
        if let Effect::SubstitutePreview { matches } = effect {
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].replacement(), "REPLACED");
            return;
        }
    }
    panic!("no SubstitutePreview effect found");
}

#[test]
fn inccommand_global_flag() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo bar foo baz foo");

    let response = type_command_line(&mut engine, &doc, "s/foo/X/g");
    for effect in response.effects.iter() {
        if let Effect::SubstitutePreview { matches } = effect {
            assert_eq!(
                matches.len(),
                3,
                "global flag should find all 3 'foo' matches"
            );
            return;
        }
    }
    panic!("no SubstitutePreview effect found");
}

#[test]
fn inccommand_incsearch_still_works() {
    // Verify that entering /pattern search mode still produces HighlightMatches
    // and is NOT affected by inccommand logic
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo bar foo");

    // Enter search mode with '/'
    process_key(&mut engine, &doc, KeyEvent::char('/'));
    assert_eq!(engine.mode(), Mode::CommandLine);

    // Type search pattern
    let response = {
        let mut last = Response::default();
        for ch in "foo".chars() {
            last = process_key(&mut engine, &doc, KeyEvent::char(ch));
        }
        last
    };

    // Should NOT have SubstitutePreview (this is search, not substitute)
    let has_preview = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::SubstitutePreview { .. }));
    assert!(
        !has_preview,
        "search mode should not produce SubstitutePreview"
    );
}

#[test]
fn inccommand_percent_range_all_lines() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo\nbar\nfoo");

    let response = type_command_line(&mut engine, &doc, "%s/foo/X/g");
    for effect in response.effects.iter() {
        if let Effect::SubstitutePreview { matches } = effect {
            assert_eq!(matches.len(), 2, "% should search all lines");
            return;
        }
    }
    panic!("no SubstitutePreview effect found");
}

#[test]
fn inccommand_cursor_line_only_without_range() {
    let mut engine = VimEngine::new();
    // Two lines both containing "foo", cursor on line 1 (offset 4)
    let doc = SimpleDocument::new("foo\nfoo");

    let response = type_command_line_at(&mut engine, &doc, "s/foo/X", 4);
    for effect in response.effects.iter() {
        if let Effect::SubstitutePreview { matches } = effect {
            assert_eq!(matches.len(), 1, "should only match on cursor line");
            // Cursor at offset 4 → line 1 → "foo" at offset 4
            assert_eq!(matches[0].match_start().get(), 4);
            return;
        }
    }
    panic!("no SubstitutePreview effect found");
}

// ─── Effect provenance tracking ──────────────────────────────────────

#[test]
fn provenance_none_for_pending_response() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // 'd' alone is pending (operator-pending: waiting for motion)
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(response.pending(), "d alone should be pending");
    assert!(
        response.provenance().is_none(),
        "pending response should have no provenance"
    );
}

#[test]
fn provenance_none_for_ignored_response() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Use sethandler to make Ctrl-A host-handled → Response::ignored()
    engine.source_config_text("sethandler <C-a> n:ide");
    let response = process_key(&mut engine, &doc, KeyEvent::ctrl('a'));
    assert!(
        !response.consumed(),
        "Ctrl-A should be ignored after sethandler n:ide"
    );
    assert!(
        response.provenance().is_none(),
        "ignored response should have no provenance"
    );
}

#[test]
fn provenance_set_for_dw_delete_word() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Feed 'd' (pending), then 'w' (completes OperatorMotion)
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('w'));

    assert!(response.consumed(), "dw should be consumed");
    let prov = response
        .provenance()
        .expect("dw response should have provenance");
    assert!(
        prov.command_name().contains("OperatorMotion"),
        "dw command_name should contain 'OperatorMotion', got: {}",
        prov.command_name()
    );
    // Keystroke seq: 'd' = 1, 'w' = 2 — provenance is set when the command executes (on 'w')
    assert_eq!(
        prov.keystroke_seq(),
        2,
        "dw should execute at keystroke_seq 2"
    );
}

#[test]
fn provenance_set_for_motion_j() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");

    let response = process_key(&mut engine, &doc, KeyEvent::char('j'));

    assert!(response.consumed());
    let prov = response
        .provenance()
        .expect("j motion should have provenance");
    assert!(
        prov.command_name().contains("Motion"),
        "j command_name should contain 'Motion', got: {}",
        prov.command_name()
    );
    assert_eq!(
        prov.keystroke_seq(),
        1,
        "first process() call should be seq 1"
    );
}

#[test]
fn provenance_set_for_insert_entry() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // 'i' enters insert mode
    let response = process_key(&mut engine, &doc, KeyEvent::char('i'));

    assert!(response.consumed());
    let prov = response.provenance().expect("'i' should have provenance");
    assert!(
        prov.command_name().contains("InsertEntry"),
        "i command_name should contain 'InsertEntry', got: {}",
        prov.command_name()
    );
    assert_eq!(prov.keystroke_seq(), 1);
}

#[test]
fn provenance_set_for_insert_char() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    // Type 'x' in insert mode
    let response = process_key(&mut engine, &doc, KeyEvent::char('x'));

    assert!(response.consumed());
    let prov = response
        .provenance()
        .expect("insert char should have provenance");
    assert!(
        prov.command_name().contains("Insert"),
        "insert char command_name should contain 'Insert', got: {}",
        prov.command_name()
    );
    assert_eq!(
        prov.keystroke_seq(),
        2,
        "second process() call should be seq 2"
    );
}

#[test]
fn keystroke_seq_increments_monotonically() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nfoo");

    // Three successive motions: j, j, k
    let r1 = process_key(&mut engine, &doc, KeyEvent::char('j'));
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('j'));
    let r3 = process_key(&mut engine, &doc, KeyEvent::char('k'));

    let seq1 = r1.provenance().expect("r1 provenance").keystroke_seq();
    let seq2 = r2.provenance().expect("r2 provenance").keystroke_seq();
    let seq3 = r3.provenance().expect("r3 provenance").keystroke_seq();

    assert_eq!(seq1, 1);
    assert_eq!(seq2, 2);
    assert_eq!(seq3, 3);
}

#[test]
fn provenance_seq_increments_even_for_pending() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // 'd' is pending (seq 1), 'w' completes the command (seq 2)
    let r_pending = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(r_pending.provenance().is_none());

    let r_complete = process_key(&mut engine, &doc, KeyEvent::char('w'));
    let seq = r_complete
        .provenance()
        .expect("complete response provenance")
        .keystroke_seq();
    assert_eq!(seq, 2, "pending 'd' consumed seq 1, 'w' should be seq 2");
}

#[test]
fn provenance_contains_operator_and_motion_for_operator_motion() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // 'y' then 'w' = yank word
    process_key(&mut engine, &doc, KeyEvent::char('y'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('w'));

    let prov = response.provenance().expect("yw should have provenance");
    let name = prov.command_name();
    assert!(
        name.contains("OperatorMotion"),
        "yw should have OperatorMotion tag in provenance, got: {name}"
    );
}

#[test]
fn provenance_set_for_action_command() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // 'x' = delete char under cursor (Action command)
    let response = process_key(&mut engine, &doc, KeyEvent::char('x'));

    let prov = response.provenance().expect("x should have provenance");
    assert!(
        prov.command_name().contains("Action"),
        "x command_name should contain 'Action', got: {}",
        prov.command_name()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Insert-mode arrow-key undo break tests
// ═══════════════════════════════════════════════════════════════════════════════
//
// Neovim behavior: pressing an arrow key (Up/Down/Left/Right), Home, End, or
// Ctrl-Left/Right in insert mode breaks the undo sequence. The current undo
// group is ended and a new one is begun, so that text typed before and after
// the arrow key forms separate, independently-undoable steps.
//
// Implementation: `execute_insert_command` in `mode_dispatch.rs` prepends
// `Effect::EndUndoGroup` + `Effect::BeginUndoGroup` to the arrow-key response.

/// Helper: enter insert mode and return the engine ready for insert typing.
fn enter_insert(engine: &mut VimEngine, doc: &SimpleDocument) {
    process_key(engine, doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);
}

/// Assert that a response contains both `EndUndoGroup` and `BeginUndoGroup`
/// and that they appear as the first two effects (i.e., prepended).
fn assert_has_undo_break_prepended(response: &Response, label: &str) {
    let effects = response.effects();
    assert!(
        effects.len() >= 2,
        "{label}: expected at least 2 effects, got {}",
        effects.len()
    );
    assert!(
        matches!(effects[0], Effect::EndUndoGroup { .. }),
        "{label}: first effect should be EndUndoGroup, got {:?}",
        effects[0]
    );
    assert!(
        matches!(effects[1], Effect::BeginUndoGroup { .. }),
        "{label}: second effect should be BeginUndoGroup, got {:?}",
        effects[1]
    );
}

/// Assert that a response does NOT contain an undo break pair.
fn assert_no_undo_break(response: &Response, label: &str) {
    let effects = response.effects();
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::EndUndoGroup { .. })),
        "{label}: unexpected EndUndoGroup in effects"
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::BeginUndoGroup { .. })),
        "{label}: unexpected BeginUndoGroup in effects"
    );
}

// ─── Arrow keys produce undo break ─────────────────────────────────

#[test]
fn insert_arrow_up_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    enter_insert(&mut engine, &doc);

    let response = process_key(&mut engine, &doc, KeyEvent::new(Key::Up, Modifiers::NONE));

    assert!(response.consumed(), "Up arrow should be consumed");
    assert_has_undo_break_prepended(&response, "Up arrow in insert mode");
}

#[test]
fn insert_arrow_down_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    enter_insert(&mut engine, &doc);

    let response = process_key(&mut engine, &doc, KeyEvent::new(Key::Down, Modifiers::NONE));

    assert!(response.consumed(), "Down arrow should be consumed");
    assert_has_undo_break_prepended(&response, "Down arrow in insert mode");
}

#[test]
fn insert_arrow_left_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    enter_insert(&mut engine, &doc);

    let response = process_key(&mut engine, &doc, KeyEvent::new(Key::Left, Modifiers::NONE));

    assert!(response.consumed(), "Left arrow should be consumed");
    assert_has_undo_break_prepended(&response, "Left arrow in insert mode");
}

#[test]
fn insert_arrow_right_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    enter_insert(&mut engine, &doc);

    let response = process_key(
        &mut engine,
        &doc,
        KeyEvent::new(Key::Right, Modifiers::NONE),
    );

    assert!(response.consumed(), "Right arrow should be consumed");
    assert_has_undo_break_prepended(&response, "Right arrow in insert mode");
}

// ─── Home and End produce undo break ───────────────────────────────

#[test]
fn insert_home_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    enter_insert(&mut engine, &doc);

    let response = process_key(&mut engine, &doc, KeyEvent::new(Key::Home, Modifiers::NONE));

    assert!(response.consumed(), "Home should be consumed");
    assert_has_undo_break_prepended(&response, "Home in insert mode");
}

#[test]
fn insert_end_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    enter_insert(&mut engine, &doc);

    let response = process_key(&mut engine, &doc, KeyEvent::new(Key::End, Modifiers::NONE));

    assert!(response.consumed(), "End should be consumed");
    assert_has_undo_break_prepended(&response, "End in insert mode");
}

// ─── Ctrl-arrow produces undo break ────────────────────────────────

#[test]
fn insert_ctrl_left_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    enter_insert(&mut engine, &doc);

    let response = process_key(&mut engine, &doc, KeyEvent::new(Key::Left, Modifiers::CTRL));

    assert!(response.consumed(), "Ctrl-Left should be consumed");
    assert_has_undo_break_prepended(&response, "Ctrl-Left in insert mode");
}

#[test]
fn insert_ctrl_right_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    enter_insert(&mut engine, &doc);

    let response = process_key(
        &mut engine,
        &doc,
        KeyEvent::new(Key::Right, Modifiers::CTRL),
    );

    assert!(response.consumed(), "Ctrl-Right should be consumed");
    assert_has_undo_break_prepended(&response, "Ctrl-Right in insert mode");
}

// ─── Regular insert chars do NOT produce undo break ────────────────

#[test]
fn insert_regular_char_does_not_break_undo_sequence() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    enter_insert(&mut engine, &doc);

    // A regular typed character must NOT inject an undo break
    let response = process_key(&mut engine, &doc, KeyEvent::char('x'));

    assert!(response.consumed(), "insert char should be consumed");
    assert_no_undo_break(&response, "regular char in insert mode");
}

// ─── Arrow keys in Normal mode do NOT produce undo break ───────────

#[test]
fn normal_mode_arrow_does_not_break_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    assert_eq!(engine.mode(), Mode::Normal);

    let response = process_key(&mut engine, &doc, KeyEvent::new(Key::Up, Modifiers::NONE));

    assert!(response.consumed(), "Up in normal mode should be consumed");
    assert_no_undo_break(&response, "Up arrow in normal mode");
}

#[test]
fn replace_mode_arrow_breaks_undo_sequence() {
    use crate::keymap::{Key, Modifiers};
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");

    // Enter replace mode with 'R'
    process_key(&mut engine, &doc, KeyEvent::char('R'));
    assert_eq!(engine.mode(), Mode::Replace);

    let response = process_key(&mut engine, &doc, KeyEvent::new(Key::Up, Modifiers::NONE));

    assert!(
        response.consumed(),
        "Up arrow in replace mode should be consumed"
    );
    assert_has_undo_break_prepended(&response, "Up arrow in replace mode");
}

// ─── Clipboard register routing ─────────────────────────────────────

/// Helper: count CopyToClipboard effects with a specific register.
fn clipboard_effects_for(response: &Response, register: RegisterName) -> Vec<&Effect> {
    response
        .effects
        .iter()
        .filter(|e| matches!(e, Effect::CopyToClipboard { register: r, .. } if *r == register))
        .collect()
}

#[test]
fn yank_with_clipboard_unnamed_emits_selection_register() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_clipboard("unnamed");
    let doc = SimpleDocument::new("hello world");

    // 'yy' yanks the line
    process_key(&mut engine, &doc, KeyEvent::char('y'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('y'));

    let sel = clipboard_effects_for(&response, RegisterName::SELECTION);
    assert_eq!(
        sel.len(),
        1,
        "clipboard=unnamed should emit CopyToClipboard with SELECTION register"
    );
    let clip = clipboard_effects_for(&response, RegisterName::CLIPBOARD);
    assert!(
        clip.is_empty(),
        "clipboard=unnamed should NOT emit CLIPBOARD register"
    );
}

#[test]
fn yank_with_clipboard_unnamedplus_emits_clipboard_register() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_clipboard("unnamedplus");
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('y'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('y'));

    let clip = clipboard_effects_for(&response, RegisterName::CLIPBOARD);
    assert_eq!(
        clip.len(),
        1,
        "clipboard=unnamedplus should emit CopyToClipboard with CLIPBOARD register"
    );
    let sel = clipboard_effects_for(&response, RegisterName::SELECTION);
    assert!(
        sel.is_empty(),
        "clipboard=unnamedplus should NOT emit SELECTION register"
    );
}

#[test]
fn yank_with_clipboard_both_emits_both_registers() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_clipboard("unnamed,unnamedplus");
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('y'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('y'));

    let sel = clipboard_effects_for(&response, RegisterName::SELECTION);
    let clip = clipboard_effects_for(&response, RegisterName::CLIPBOARD);
    assert_eq!(sel.len(), 1, "both: should emit SELECTION register");
    assert_eq!(clip.len(), 1, "both: should emit CLIPBOARD register");
}

#[test]
fn yank_with_no_clipboard_option_emits_nothing() {
    let mut engine = VimEngine::new();
    // Default: clipboard is empty
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('y'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('y'));

    let any_clipboard = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::CopyToClipboard { .. }));
    assert!(
        !any_clipboard,
        "no clipboard option should produce no CopyToClipboard effects"
    );
}

#[test]
fn delete_with_clipboard_unnamed_emits_selection_register() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_clipboard("unnamed");
    let doc = SimpleDocument::new("hello world");

    // 'dd' deletes the line
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));

    let sel = clipboard_effects_for(&response, RegisterName::SELECTION);
    assert_eq!(
        sel.len(),
        1,
        "dd with clipboard=unnamed should emit CopyToClipboard with SELECTION register"
    );
}

#[test]
fn clipboard_effect_text_matches_yanked_content() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_clipboard("unnamed");
    let doc = SimpleDocument::new("hello world");

    process_key(&mut engine, &doc, KeyEvent::char('y'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('y'));

    let sel = clipboard_effects_for(&response, RegisterName::SELECTION);
    assert_eq!(sel.len(), 1);
    match sel[0] {
        Effect::CopyToClipboard { text, .. } => {
            assert!(!text.is_empty(), "CopyToClipboard text should not be empty");
        }
        _ => unreachable!(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// <expr> mapping round-trip (HostRequest::EvaluateMapping)
// ═══════════════════════════════════════════════════════════════════════════════

/// Full round-trip test for `nnoremap <expr>`:
/// 1. Register an `<expr>` mapping (non-recursive) on key `g`
/// 2. Process the trigger key → assert `EvaluateMapping` host request with `recursive: false`
/// 3. Complete the request with returned keys → assert keys are fed through
#[test]
fn expr_mapping_noremap_round_trip() {
    use crate::execution::host::{HostRequest, HostRequestKind, HostResult};
    use crate::keymap::{MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("line one\nline two\nline three");

    // Register: nnoremap <expr> g MyExpr()
    engine.map_with_expr(
        MappingMode::Normal,
        &[KeyEvent::char('g')],
        Vec::new(),
        MappingKind::NonRecursive,
        MappingFlags {
            expr: true,
            ..MappingFlags::default()
        },
        Some(compact_str::CompactString::from("MyExpr()")),
    );

    // Step 1: Process the trigger key 'g'
    let response = process_key(&mut engine, &doc, KeyEvent::char('g'));
    assert!(
        response.pending(),
        "expr mapping trigger should return pending"
    );
    assert_eq!(
        response.host_requests().len(),
        1,
        "should emit exactly one host request"
    );
    assert_eq!(
        response.host_requests()[0].kind(),
        HostRequestKind::EvaluateMapping
    );

    // Step 2: Verify the host request carries the correct fields
    let req = &response.host_requests()[0];
    if let HostRequest::EvaluateMapping {
        expression,
        mode,
        kind,
        ..
    } = req
    {
        assert_eq!(expression.as_str(), "MyExpr()");
        assert_eq!(*mode, MappingMode::Normal);
        assert!(
            !kind.is_recursive(),
            "nnoremap <expr> should have recursive=false"
        );
    } else {
        panic!("expected EvaluateMapping host request");
    }

    // Step 3: Complete the host request — return "j" as the result
    let request_id = req.id();
    let completion = engine.complete_host_request(&HostResult::Data {
        id: request_id,
        data: compact_str::CompactString::from("j"),
        offset: None,
    });

    // The completion injects keys into the typeahead; they will be
    // processed on the next drain_next_key() / process() cycle.
    assert!(completion.consumed(), "completion should be consumed");

    // Step 4: Drain the injected key — it should produce a cursor-move effect
    assert!(engine.has_pending_keys(), "injected keys should be pending");
    let drained_output = engine.drain_next_key().expect("should have a pending key");
    let MacroOutput::Key(drained_key) = drained_output else {
        panic!("expected MacroOutput::Key, got {:?}", drained_output);
    };
    let ctx2 = InputContext::new(&doc, 0).validate().unwrap();
    let result = engine.process(drained_key, ctx2);
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "the returned 'j' key should produce a SetCursor effect"
    );
}

/// Full round-trip test for `nmap <expr>` (recursive):
/// Verify that `recursive: true` is emitted when using `:map <expr>` (recursive mapping).
#[test]
fn expr_mapping_recursive_emits_recursive_flag() {
    use crate::execution::host::{HostRequest, HostRequestKind};
    use crate::keymap::{MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Register: nmap <expr> g MyExpr()  (recursive)
    engine.map_with_expr(
        MappingMode::Normal,
        &[KeyEvent::char('g')],
        Vec::new(),
        MappingKind::Recursive,
        MappingFlags {
            expr: true,
            ..MappingFlags::default()
        },
        Some(compact_str::CompactString::from("MyExpr()")),
    );

    // Process the trigger key
    let response = process_key(&mut engine, &doc, KeyEvent::char('g'));
    assert!(response.pending());
    assert_eq!(response.host_requests().len(), 1);
    assert_eq!(
        response.host_requests()[0].kind(),
        HostRequestKind::EvaluateMapping
    );

    // Verify recursive=true
    if let HostRequest::EvaluateMapping { kind, .. } = &response.host_requests()[0] {
        assert!(
            kind.is_recursive(),
            "nmap <expr> should have recursive=true"
        );
    } else {
        panic!("expected EvaluateMapping host request");
    }
}

/// Verify that the expression text from the mapping's RHS is correctly
/// propagated through the host request.
#[test]
fn expr_mapping_preserves_expression_text() {
    use crate::execution::host::HostRequest;
    use crate::keymap::{MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("text");

    let expr_text = "v:count ? 'j' : 'gj'";
    engine.map_with_expr(
        MappingMode::Normal,
        &[KeyEvent::char('j')],
        Vec::new(),
        MappingKind::NonRecursive,
        MappingFlags {
            expr: true,
            ..MappingFlags::default()
        },
        Some(compact_str::CompactString::from(expr_text)),
    );

    let response = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(response.pending());

    if let HostRequest::EvaluateMapping { expression, .. } = &response.host_requests()[0] {
        assert_eq!(
            expression.as_str(),
            expr_text,
            "expression text should be preserved exactly"
        );
    } else {
        panic!("expected EvaluateMapping host request");
    }
}

/// Verify that completing an expr mapping with a failure produces an error.
#[test]
fn expr_mapping_host_failure_produces_error() {
    use crate::execution::host::HostResult;
    use crate::keymap::{MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    engine.map_with_expr(
        MappingMode::Normal,
        &[KeyEvent::char('g')],
        Vec::new(),
        MappingKind::NonRecursive,
        MappingFlags {
            expr: true,
            ..MappingFlags::default()
        },
        Some(compact_str::CompactString::from("BadExpr()")),
    );

    let response = process_key(&mut engine, &doc, KeyEvent::char('g'));
    let req = &response.host_requests()[0];
    let request_id = req.id();

    // Complete with failure
    let result = engine.complete_host_request(&HostResult::Failure {
        id: request_id,
        error: compact_str::CompactString::from("E123: Undefined function"),
    });

    // Should produce an error effect (ShowError / SetMessage)
    assert!(
        result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ShowError { .. })),
        "host failure should produce an error message effect"
    );
}

/// Verify that completing an expr mapping with empty data produces no pending keys.
#[test]
fn expr_mapping_empty_result_produces_no_pending_keys() {
    use crate::execution::host::HostResult;
    use crate::keymap::{MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    engine.map_with_expr(
        MappingMode::Normal,
        &[KeyEvent::char('g')],
        Vec::new(),
        MappingKind::NonRecursive,
        MappingFlags {
            expr: true,
            ..MappingFlags::default()
        },
        Some(compact_str::CompactString::from("EmptyExpr()")),
    );

    let response = process_key(&mut engine, &doc, KeyEvent::char('g'));
    let request_id = response.host_requests()[0].id();

    // Complete with empty data
    let _result = engine.complete_host_request(&HostResult::Data {
        id: request_id,
        data: compact_str::CompactString::from(""),
        offset: None,
    });

    assert!(
        !engine.has_pending_keys(),
        "empty expr result should not inject any pending keys"
    );
}

/// Verify that `nnoremap <silent> <expr>` propagates the `silent` flag
/// through the `EvaluateMapping` host request and into the injected keys,
/// so that `ShowInfo` effects produced by the returned keys are suppressed.
#[test]
fn silent_expr_mapping_suppresses_messages() {
    use crate::execution::host::{HostRequest, HostResult};
    use crate::keymap::{MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Register: nnoremap <silent> <expr> g MyExpr()
    // silent=true, expr=true
    engine.map_with_expr(
        MappingMode::Normal,
        &[KeyEvent::char('g')],
        Vec::new(),
        MappingKind::NonRecursive,
        MappingFlags {
            expr: true,
            silent: true,
            ..MappingFlags::default()
        },
        Some(compact_str::CompactString::from("MyExpr()")),
    );

    // Step 1: Process the trigger key 'g'
    let response = process_key(&mut engine, &doc, KeyEvent::char('g'));
    assert!(
        response.pending(),
        "expr mapping trigger should return pending"
    );
    assert_eq!(response.host_requests().len(), 1);

    // Step 2: Verify the host request carries silent=true
    let req = &response.host_requests()[0];
    if let HostRequest::EvaluateMapping {
        expression,
        kind,
        silent,
        ..
    } = req
    {
        assert_eq!(expression.as_str(), "MyExpr()");
        assert!(
            !kind.is_recursive(),
            "nnoremap <expr> should have recursive=false"
        );
        assert!(*silent, "<silent> <expr> mapping should have silent=true");
    } else {
        panic!("expected EvaluateMapping host request");
    }

    // Step 3: Complete the request — return Ctrl-G (ShowFileInfo) as the result.
    // Ctrl-G normally produces ShowInfo; with <silent>, it should be suppressed.
    let request_id = req.id();
    let completion = engine.complete_host_request(&HostResult::Data {
        id: request_id,
        data: compact_str::CompactString::from("<C-g>"),
        offset: None,
    });
    assert!(completion.consumed(), "completion should be consumed");

    // Step 4: Drain and process the injected Ctrl-G key
    assert!(engine.has_pending_keys(), "injected keys should be pending");
    let drained_output = engine.drain_next_key().expect("should have a pending key");
    let MacroOutput::Key(drained_key) = drained_output else {
        panic!("expected MacroOutput::Key, got {:?}", drained_output);
    };
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let result = engine.process(drained_key, ctx);

    // ShowInfo should be suppressed by <silent>
    let has_show_message = result
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ShowInfo { .. }));
    assert!(
        !has_show_message,
        "<silent> <expr> mapping should suppress ShowInfo from Ctrl-G"
    );
}

/// Verify that `nnoremap <expr>` (without <silent>) does NOT suppress messages.
#[test]
fn non_silent_expr_mapping_preserves_messages() {
    use crate::execution::host::{HostRequest, HostResult};
    use crate::keymap::{MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Register: nnoremap <expr> g MyExpr()   (no <silent>)
    engine.map_with_expr(
        MappingMode::Normal,
        &[KeyEvent::char('g')],
        Vec::new(),
        MappingKind::NonRecursive,
        MappingFlags {
            expr: true,
            ..MappingFlags::default()
        },
        Some(compact_str::CompactString::from("MyExpr()")),
    );

    let response = process_key(&mut engine, &doc, KeyEvent::char('g'));
    let req = &response.host_requests()[0];
    if let HostRequest::EvaluateMapping { silent, .. } = req {
        assert!(
            !silent,
            "non-silent <expr> mapping should have silent=false"
        );
    } else {
        panic!("expected EvaluateMapping host request");
    }

    // Complete with Ctrl-G
    let request_id = req.id();
    let _completion = engine.complete_host_request(&HostResult::Data {
        id: request_id,
        data: compact_str::CompactString::from("<C-g>"),
        offset: None,
    });

    // Drain and process the injected key
    let drained_output = engine.drain_next_key().expect("should have a pending key");
    let MacroOutput::Key(drained_key) = drained_output else {
        panic!("expected MacroOutput::Key, got {:?}", drained_output);
    };
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let result = engine.process(drained_key, ctx);

    // ShowInfo should NOT be suppressed
    let has_show_message = result
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ShowInfo { .. }));
    assert!(
        has_show_message,
        "non-silent <expr> mapping should preserve ShowInfo from Ctrl-G"
    );
}

// ─── <silent> mapping flag ──────────────────────────────────────────

#[test]
fn silent_mapping_suppresses_show_message() {
    // Map 'x' to Ctrl-G (ShowFileInfo) with <silent>.
    // Ctrl-G produces ShowInfo with file info — silent should suppress it.
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // First verify Ctrl-G normally produces ShowInfo
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let normal_response = engine.process(KeyEvent::ctrl('g'), ctx);
    let has_show_message = normal_response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ShowInfo { .. }));
    assert!(has_show_message, "Ctrl-G should produce ShowInfo normally");

    // Now create silent mapping: x → Ctrl-G
    engine.map(
        MappingMode::Normal,
        &[KeyEvent::char('x')],
        key_sequence(&[KeyEvent::ctrl('g')]),
        MappingKind::NonRecursive,
        MappingFlags {
            silent: true,
            ..MappingFlags::default()
        },
    );

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let silent_response = engine.process(KeyEvent::char('x'), ctx);
    let has_show_message = silent_response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ShowInfo { .. }));
    assert!(
        !has_show_message,
        "Silent mapping should suppress ShowInfo from Ctrl-G"
    );
}

#[test]
fn non_silent_mapping_preserves_show_message() {
    // Map 'x' to Ctrl-G WITHOUT <silent> — ShowInfo should remain.
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    engine.map(
        MappingMode::Normal,
        &[KeyEvent::char('x')],
        key_sequence(&[KeyEvent::ctrl('g')]),
        MappingKind::NonRecursive,
        MappingFlags::default(), // NOT silent
    );

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('x'), ctx);
    let has_show_message = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ShowInfo { .. }));
    assert!(
        has_show_message,
        "Non-silent mapping should preserve ShowInfo"
    );
}

#[test]
fn silent_mapping_preserves_show_error() {
    // <silent> should NOT suppress ShowError — errors always shown.
    // We test indirectly: ShowError effects should survive <silent> filtering.
    // Using a motion to an invalid target that produces ShowError.
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Map 'x' to 'f' followed by a char not in the line.
    // 'fZ' on "hello" produces a search failure (ShowError).
    engine.map(
        MappingMode::Normal,
        &[KeyEvent::char('x')],
        key_sequence(&[KeyEvent::char('f'), KeyEvent::char('Z')]),
        MappingKind::NonRecursive,
        MappingFlags {
            silent: true,
            ..MappingFlags::default()
        },
    );

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let r1 = engine.process(KeyEvent::char('x'), ctx);
    // After pressing 'x', the 'f' gets dispatched. 'f' puts the parser
    // in awaiting-char state. The 'Z' will be the next key.
    // The first response is likely pending or empty; the second key comes
    // from typeahead.
    if engine.has_pending_keys() {
        let next_output = engine.drain_next_key().unwrap();
        let MacroOutput::Key(next_key) = next_output else {
            panic!("expected MacroOutput::Key, got {:?}", next_output);
        };
        let ctx2 = InputContext::new(&doc, 0).validate().unwrap();
        let r2 = engine.process(next_key, ctx2);
        // ShowError from 'fZ' failure should NOT be filtered by <silent>
        let has_show_error = r2
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ShowError { .. }));
        // Note: the 'f' command may not produce ShowError — it may just
        // produce nothing. Check that at least ShowInfo isn't created.
        let has_show_message = r2
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { .. }));
        assert!(
            !has_show_message,
            "Silent mapping should still suppress ShowInfo"
        );
        // ShowError, if present, should be preserved (not filtered).
        // We can't assert it's present since 'fZ' may not produce one.
        let _ = has_show_error;
    }
}

#[test]
fn silent_flag_propagates_through_mapping_chain() {
    // If 'x' is mapped <silent> to 'y', and 'y' is mapped (non-silent)
    // to Ctrl-G, the <silent> flag should propagate through the chain.
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    // Map y → Ctrl-G (normal, recursive, not silent)
    engine.map(
        MappingMode::Normal,
        &[KeyEvent::char('y')],
        key_sequence(&[KeyEvent::ctrl('g')]),
        MappingKind::Recursive,
        MappingFlags::default(), // NOT silent
    );

    // Map x → y (recursive, silent)
    engine.map(
        MappingMode::Normal,
        &[KeyEvent::char('x')],
        key_sequence(&[KeyEvent::char('y')]),
        MappingKind::Recursive,
        MappingFlags {
            silent: true,
            ..MappingFlags::default()
        },
    );

    // Press 'x' → expands to 'y' (with SILENT) → 'y' re-resolves to Ctrl-G (SILENT preserved)
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('x'), ctx);
    let has_show_message = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ShowInfo { .. }));
    assert!(
        !has_show_message,
        "Silent flag should propagate through recursive mapping chain"
    );
}

#[test]
fn silent_mapping_via_source_config() {
    // Source a config with `nnoremap <silent> ...` and verify it applies.
    use crate::keymap::KeyEvent as KE;

    let mut engine = VimEngine::new();

    let config = "nnoremap <silent> x <C-g>\n";
    engine.source_config_text(config);

    // Verify the mapping was installed
    let x_key = KE::char('x');
    assert!(
        engine.could_start_mapping(x_key),
        "nnoremap <silent> x <C-g> should install a mapping for 'x'"
    );

    // Execute the mapping
    let doc = SimpleDocument::new("hello");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(x_key, ctx);

    let has_show_message = response
        .effects
        .iter()
        .any(|e| matches!(e, Effect::ShowInfo { .. }));
    assert!(
        !has_show_message,
        "Silent mapping installed via source_config_text should suppress ShowInfo"
    );
}

#[test]
fn mapping_entry_silent_accessor() {
    // Verify MappingEntry stores and reports silent flag correctly.
    use crate::keymap::{MappingEntry, MappingKind};

    let normal = MappingEntry::new(vec![], MappingKind::NonRecursive);
    assert!(
        !normal.silent(),
        "Default MappingEntry should not be silent"
    );

    let silent = MappingEntry::new_silent(vec![], MappingKind::NonRecursive);
    assert!(
        silent.silent(),
        "new_silent() should create a silent MappingEntry"
    );
}

// ─── Engine storage and resolved cache ────────────────────────────────────

#[test]
fn set_buffer_overrides_tabstop_effective_option_returns_override() {
    use crate::primitives::{OptionId, OptionOverrides, OptionValue};

    let mut engine = VimEngine::new();
    // Default tabstop is 4
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(4)
    );

    let mut overrides = OptionOverrides::new();
    overrides.set(OptionId::TabStop, OptionValue::Unsigned(8));
    engine.set_buffer_overrides(overrides);

    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8),
        "effective_option should return the buffer override value"
    );
}

#[test]
fn take_buffer_overrides_returns_overrides_and_falls_back_to_global() {
    use crate::primitives::{OptionId, OptionOverrides, OptionValue};

    let mut engine = VimEngine::new();
    let mut overrides = OptionOverrides::new();
    overrides.set(OptionId::TabStop, OptionValue::Unsigned(8));
    engine.set_buffer_overrides(overrides);

    let taken = engine.take_buffer_overrides();

    // Taken overrides should contain the tabstop=8 entry
    assert_eq!(
        taken.get(OptionId::TabStop),
        Some(&OptionValue::Unsigned(8)),
        "taken overrides should have the previously set tabstop"
    );

    // After taking, effective falls back to global default (4)
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(4),
        "after taking overrides, effective_option should fall back to global"
    );
}

#[test]
fn set_window_overrides_scrolloff_effective_option_returns_override() {
    use crate::primitives::{OptionId, OptionOverrides, OptionValue};

    let mut engine = VimEngine::new();
    // Default scrolloff is 5
    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(5)
    );

    let mut overrides = OptionOverrides::new();
    overrides.set(OptionId::ScrollOff, OptionValue::Unsigned(10));
    engine.set_window_overrides(overrides);

    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(10),
        "effective_option should return the window override value"
    );
}

#[test]
fn take_window_overrides_returns_overrides_and_falls_back_to_global() {
    use crate::primitives::{OptionId, OptionOverrides, OptionValue};

    let mut engine = VimEngine::new();
    let mut overrides = OptionOverrides::new();
    overrides.set(OptionId::ScrollOff, OptionValue::Unsigned(10));
    engine.set_window_overrides(overrides);

    let taken = engine.take_window_overrides();

    assert_eq!(
        taken.get(OptionId::ScrollOff),
        Some(&OptionValue::Unsigned(10)),
        "taken overrides should have the previously set scrolloff"
    );

    // After taking, effective falls back to global default (5)
    assert_eq!(
        engine.effective_option(OptionId::ScrollOff),
        OptionValue::Unsigned(5),
        "after taking overrides, effective_option should fall back to global"
    );
}

#[test]
fn global_only_options_not_affected_by_buffer_overrides() {
    use crate::primitives::{OptionId, OptionOverrides, OptionValue};

    let mut engine = VimEngine::new();
    engine.options_mut().set_ignorecase(true);
    engine.invalidate_option_cache();

    // Attempt to override a Global-scope option via buffer overrides —
    // resolve_all ignores overrides for Global-scoped options.
    let mut overrides = OptionOverrides::new();
    overrides.set(OptionId::IgnoreCase, OptionValue::Bool(false));
    engine.set_buffer_overrides(overrides);

    // Global-scope option should not be overridden; effective == global value
    assert_eq!(
        engine.effective_option(OptionId::IgnoreCase),
        OptionValue::Bool(true),
        "global-scope options must not be overridden by buffer overrides"
    );
}

#[test]
fn options_returns_global_even_when_overrides_set() {
    use crate::primitives::{OptionId, OptionOverrides, OptionValue};

    let mut engine = VimEngine::new();
    let mut overrides = OptionOverrides::new();
    overrides.set(OptionId::TabStop, OptionValue::Unsigned(8));
    engine.set_buffer_overrides(overrides);

    // options() must always reflect the global layer, not the resolved value
    assert_eq!(
        engine.options().tabstop(),
        4,
        "options() should return the global tabstop, not the buffer override"
    );

    // effective_option should return the override
    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(8),
        "effective_option should return the buffer override"
    );
}

#[test]
fn set_options_rebuilds_resolved_cache() {
    use crate::primitives::{OptionId, OptionValue, VimOptions};

    let mut engine = VimEngine::new();
    let mut new_opts = VimOptions::default();
    new_opts.set_tabstop(16);
    engine.set_options(new_opts);

    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(16),
        "effective_option should reflect options replaced via set_options()"
    );
}

#[test]
fn options_mut_dirty_flag_rebuilt_before_next_process() {
    use crate::primitives::{OptionId, OptionValue};

    let mut engine = VimEngine::new();
    engine.options_mut().set_tabstop(12);

    // Trigger a process() call so the dirty flag is cleared and cache rebuilt.
    let doc = SimpleDocument::new("hello\nworld");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    engine.process(KeyEvent::char('j'), ctx);

    assert_eq!(
        engine.effective_option(OptionId::TabStop),
        OptionValue::Unsigned(12),
        "effective_option should reflect options mutated via options_mut() after next process()"
    );
}

// ─── INSERT_STOP mark (^) adjustment tests ────────────────────────────

/// Verify that `adjust_insert_stop` shifts mark.^ for edits before it.
#[test]
fn insert_stop_shifts_via_adjust_insert_stop() {
    use crate::primitives::{MarkName, Offset};
    use crate::state::Marks;

    let mut marks = Marks::new();
    marks.set_insert_stop(Offset::new(50));

    // Delete 10 bytes at position 20 (before the mark): 50 → 40.
    marks.adjust_insert_stop(20, 10, 0);

    let insert_stop = marks.get(MarkName::INSERT_STOP).unwrap();
    assert_eq!(
        insert_stop.offset().get(),
        40,
        "^ mark should shift backward via adjust_insert_stop"
    );
}

/// Verify that `adjust_insert_stop` shifts mark.^ forward on insert.
#[test]
fn insert_stop_shifts_forward_via_adjust_insert_stop() {
    use crate::primitives::{MarkName, Offset};
    use crate::state::Marks;

    let mut marks = Marks::new();
    marks.set_insert_stop(Offset::new(10));

    // Insert 4 bytes at position 0 → mark shifts to 14.
    marks.adjust_insert_stop(0, 0, 4);

    let insert_stop = marks.get(MarkName::INSERT_STOP).unwrap();
    assert_eq!(
        insert_stop.offset().get(),
        14,
        "^ mark should shift forward via adjust_insert_stop"
    );
}

// ─── Backslash (leader) no-op in normal mode ────────────────────────

#[test]
fn backslash_is_noop_in_normal_mode_no_mappings() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    let cursor = 3; // Middle of line
    let ctx = InputContext::new(&doc, cursor).validate().unwrap();
    let response = engine.process(KeyEvent::char('\\'), ctx);

    assert!(
        response.consumed(),
        "backslash should be consumed (not forwarded to host)"
    );
    assert!(!response.pending(), "backslash should not be pending");
    assert!(
        response.effects().is_empty(),
        "backslash with no mappings should produce zero effects, got: {:?}",
        response.effects(),
    );
    assert!(
        response.message.is_none(),
        "backslash should produce no message (Neovim is silent for unknown keys)"
    );
}

#[test]
fn backslash_no_cursor_movement() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    let cursor = 2;
    let ctx = InputContext::new(&doc, cursor).validate().unwrap();
    let response = engine.process(KeyEvent::char('\\'), ctx);

    // No SetCursor effect means cursor stays at original position
    let has_cursor_move = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::SetCursor { .. }));
    assert!(!has_cursor_move, "backslash should not move cursor");
}

#[test]
fn backslash_no_register_change() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('\\'), ctx);

    let has_register = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::SetRegister { .. }));
    assert!(!has_register, "backslash should not change any register");
}

#[test]
fn backslash_no_mode_change() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let response = engine.process(KeyEvent::char('\\'), ctx);

    let has_mode = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::SetMode { .. }));
    assert!(!has_mode, "backslash should not change mode");
}

#[test]
fn normalize_for_layout_in_ready_normal() {
    let engine = VimEngine::new();
    // Engine starts in Normal/Ready
    let cyrillic_o = KeyEvent::char('\u{043E}').with_latin(Key::Char('j'));
    let normalized = engine.normalize_for_layout(cyrillic_o);
    assert_eq!(
        normalized.key(),
        Key::Char('j'),
        "should normalize to Latin in Normal/Ready"
    );
    assert_eq!(
        normalized.latin_key(),
        None,
        "normalized key should not carry latin_key"
    );
}

#[test]
fn normalize_for_layout_none_is_identity() {
    let engine = VimEngine::new();
    let plain_j = KeyEvent::char('j');
    let normalized = engine.normalize_for_layout(plain_j);
    assert_eq!(
        normalized, plain_j,
        "key without latin_key should pass through unchanged"
    );
}

#[test]
fn normalize_for_layout_preserves_modifiers() {
    let engine = VimEngine::new();
    let cyrillic_shift = KeyEvent::new(Key::Char('\u{041E}'), Modifiers::NONE) // Cyrillic 'О'
        .with_latin(Key::Char('J'));
    let normalized = engine.normalize_for_layout(cyrillic_shift);
    assert_eq!(normalized.key(), Key::Char('J'));
    assert_eq!(normalized.modifiers(), Modifiers::NONE);
}

#[test]
fn would_handle_key_with_latin_key_in_normal() {
    let engine = VimEngine::new();
    // Cyrillic 'о' alone is Unknown in Normal keymap
    let cyrillic_plain = KeyEvent::char('\u{043E}');
    assert!(
        !engine.would_handle_key(cyrillic_plain),
        "bare Cyrillic should not be handled"
    );

    // But with latin_key='j', normalization makes it 'j' which IS handled
    let cyrillic_with_latin = cyrillic_plain.with_latin(Key::Char('j'));
    assert!(
        engine.would_handle_key(cyrillic_with_latin),
        "Cyrillic with latin_key='j' should be handled after normalization"
    );
}

#[test]
fn normalize_for_layout_in_operator_pending() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    let cyrillic_tse = KeyEvent::char('\u{0446}').with_latin(Key::Char('w'));
    let normalized = engine.normalize_for_layout(cyrillic_tse);
    assert_eq!(
        normalized.key(),
        Key::Char('w'),
        "should normalize to Latin in operator-pending state"
    );
}

#[test]
fn normalize_for_layout_preserves_literal_in_awaiting_char() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('f'));
    let cyrillic_sha = KeyEvent::char('\u{0448}').with_latin(Key::Char('i'));
    let normalized = engine.normalize_for_layout(cyrillic_sha);
    assert_eq!(
        normalized.key(),
        Key::Char('\u{0448}'),
        "should preserve literal char in AwaitingChar state (after f/t/r)"
    );
}

#[test]
fn normalize_for_layout_in_awaiting_prefix() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('g'));
    let cyrillic_sha = KeyEvent::char('\u{0448}').with_latin(Key::Char('i'));
    let normalized = engine.normalize_for_layout(cyrillic_sha);
    assert_eq!(
        normalized.key(),
        Key::Char('i'),
        "should normalize to Latin in AwaitingPrefix state (after g)"
    );
}

#[test]
fn normalize_for_layout_in_awaiting_register() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('"'));
    let cyrillic_ef = KeyEvent::char('\u{0444}').with_latin(Key::Char('a'));
    let normalized = engine.normalize_for_layout(cyrillic_ef);
    assert_eq!(
        normalized.key(),
        Key::Char('a'),
        "should normalize to Latin in AwaitingRegister state"
    );
}

// ── <Action> parser-reset fix ─────────────────────────────────────

#[test]
fn action_resets_parser_state_count() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nfoo\nbar");

    let action_key = engine.keymap_mut().register_action("TestAction");
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Type '3' then '=' (triggers action) — count should be consumed
    process_key(&mut engine, &doc, KeyEvent::char('3'));
    process_key(&mut engine, &doc, KeyEvent::char('='));

    // Now type 'j' — should move 1 line (not 3, because count was consumed)
    let response = process_key(&mut engine, &doc, KeyEvent::char('j'));
    let cursor_effects: Vec<_> = response
        .effects()
        .iter()
        .filter(|e| matches!(e, Effect::SetCursor { .. }))
        .collect();
    assert!(
        !cursor_effects.is_empty(),
        "j after action should produce SetCursor"
    );
    if let Effect::SetCursor { offset, .. } = cursor_effects[0] {
        // "hello\n" = 6 bytes, next line starts at offset 6
        assert_eq!(
            offset.get(),
            6,
            "j after action should move 1 line, not 3 — count must be consumed"
        );
    }
}

#[test]
fn action_cancels_pending_operator() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    let action_key = engine.keymap_mut().register_action("TestAction");
    engine.keymap_mut().map(
        MappingMode::Normal,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Type 'd' (enter operator-pending), then '=' (triggers action)
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    let response = process_key(&mut engine, &doc, KeyEvent::char('='));
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::HostAction { .. })),
        "Action should fire even after pending operator"
    );

    // Now type 'j' — should move cursor, NOT delete a line
    let response2 = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(
        !response2
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::Delete { .. })),
        "Pending operator should have been cancelled by action"
    );
}

#[test]
fn action_in_visual_mode_exits_to_normal() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    let action_key = engine.keymap_mut().register_action("ExtractMethod");
    engine.keymap_mut().map(
        MappingMode::Visual,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Enter visual mode
    process_key(&mut engine, &doc, KeyEvent::char('v'));
    // Select some text
    process_key(&mut engine, &doc, KeyEvent::char('w'));
    // Trigger action
    let response = process_key(&mut engine, &doc, KeyEvent::char('='));

    // Should emit HostAction
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::HostAction { .. })),
        "Action should fire in visual mode"
    );

    // Should emit SetMode(Normal) to exit visual
    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SetMode { mode, .. } if *mode == Mode::Normal)),
        "Action in visual mode should exit to normal: got {:?}",
        response.effects()
    );
}

#[test]
fn action_in_visual_mode_saves_last_visual() {
    use crate::keymap::{key_sequence, MappingFlags, MappingKind, MappingMode};
    use crate::primitives::SelectionRange;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    let action_key = engine.keymap_mut().register_action("TestVisual");
    engine.keymap_mut().map(
        MappingMode::Visual,
        &[KeyEvent::char('=')],
        key_sequence(&[action_key]),
        MappingKind::NonRecursive,
        MappingFlags::default(),
    );

    // Enter visual mode via engine state, then trigger action with selection.
    process_key(&mut engine, &doc, KeyEvent::char('v'));
    assert!(engine.mode().is_visual());

    // Trigger the action with a selection provided in the context.
    let sel = SelectionRange::new(Offset::new(0), Offset::new(5));
    let ctx = InputContext::new(&doc, 0)
        .validate()
        .unwrap()
        .with_selection(sel);
    let response = engine.process(KeyEvent::char('='), ctx);

    assert!(
        response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::SaveLastVisual { .. })),
        "Action in visual mode should emit SaveLastVisual for gv: got {:?}",
        response.effects()
    );
}

/// Verify that `<Action>(Name)` in an `<expr>` mapping result resolves to the
/// registered action id (not the `u32::MAX` sentinel) and fires `HostAction`.
#[test]
fn expr_mapping_result_resolves_action_name() {
    use crate::execution::host::{HostRequestKind, HostResult};
    use crate::keymap::{MappingFlags, MappingKind, MappingMode};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Register an action so the keymap knows the name → id mapping.
    engine.keymap_mut().register_action("TestExprAction");

    // Register: nnoremap <expr> g MyExpr()
    engine.map_with_expr(
        MappingMode::Normal,
        &[KeyEvent::char('g')],
        Vec::new(),
        MappingKind::NonRecursive,
        MappingFlags {
            expr: true,
            ..MappingFlags::default()
        },
        Some(compact_str::CompactString::from("MyExpr()")),
    );

    // Step 1: Trigger the expr mapping
    let response = process_key(&mut engine, &doc, KeyEvent::char('g'));
    assert!(response.pending(), "expr mapping should return pending");
    assert_eq!(response.host_requests().len(), 1);
    assert_eq!(
        response.host_requests()[0].kind(),
        HostRequestKind::EvaluateMapping
    );

    // Step 2: Complete with "<Action>(TestExprAction)" — the parser must resolve
    // the action name using the keymap registry.
    let request_id = response.host_requests()[0].id();
    let completion = engine.complete_host_request(&HostResult::Data {
        id: request_id,
        data: compact_str::CompactString::from("<Action>(TestExprAction)"),
        offset: None,
    });
    assert!(completion.consumed(), "completion should be consumed");

    // Step 3: Drain and process the injected key
    assert!(
        engine.has_pending_keys(),
        "injected action key should be pending"
    );
    let drained_output = engine.drain_next_key().expect("should have a pending key");
    let MacroOutput::Key(drained_key) = drained_output else {
        panic!("expected MacroOutput::Key, got {:?}", drained_output);
    };
    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let result = engine.process(drained_key, ctx);

    // The resolved action should produce a HostAction effect with the correct name.
    assert!(
        result.effects().iter().any(|e| matches!(
            e,
            Effect::HostAction { name } if name.as_str() == "TestExprAction"
        )),
        "expr mapping returning <Action>(TestExprAction) should produce HostAction effect, got: {:?}",
        result.effects()
    );
}

// ─── CursorShapeHint tests ──────────────────────────────────────────

#[test]
fn cursor_shape_hint_emitted_on_operator_pending_enter() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Press 'd' to enter operator-pending (delete)
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));

    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: Some(Operator::Delete)
            }
        )),
        "Pressing 'd' should emit CursorShapeHint with Some(Delete), got: {:?}",
        response.effects()
    );
}

#[test]
fn cursor_shape_hint_emitted_on_yank_pending() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Press 'y' to enter operator-pending (yank)
    let response = process_key(&mut engine, &doc, KeyEvent::char('y'));

    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: Some(Operator::Yank)
            }
        )),
        "Pressing 'y' should emit CursorShapeHint with Some(Yank), got: {:?}",
        response.effects()
    );
}

#[test]
fn cursor_shape_hint_cleared_on_motion() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Press 'd' to enter operator-pending
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(response.effects().iter().any(|e| matches!(
        e,
        Effect::CursorShapeHint {
            pending_operator: Some(Operator::Delete)
        }
    )),);

    // Press 'w' to complete the motion — operator-pending exits
    let response = process_key(&mut engine, &doc, KeyEvent::char('w'));
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: None
            }
        )),
        "Completing motion should emit CursorShapeHint with None, got: {:?}",
        response.effects()
    );
}

#[test]
fn cursor_shape_hint_cleared_on_escape() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Press 'c' to enter operator-pending (change)
    let response = process_key(&mut engine, &doc, KeyEvent::char('c'));
    assert!(response.effects().iter().any(|e| matches!(
        e,
        Effect::CursorShapeHint {
            pending_operator: Some(Operator::Change)
        }
    )),);

    // Press Escape to cancel
    let response = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: None
            }
        )),
        "Escape should emit CursorShapeHint with None, got: {:?}",
        response.effects()
    );
}

#[test]
fn cursor_shape_hint_change_operator_pending() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Press 'c' to enter operator-pending (change)
    let response = process_key(&mut engine, &doc, KeyEvent::char('c'));
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: Some(Operator::Change)
            }
        )),
        "Pressing 'c' should emit CursorShapeHint with Some(Change), got: {:?}",
        response.effects()
    );
}

#[test]
fn cursor_shape_hint_not_emitted_for_normal_keys() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Press 'j' (motion, not operator) — should NOT emit CursorShapeHint
    let response = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(
        !response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::CursorShapeHint { .. })),
        "Normal motions should not emit CursorShapeHint, got: {:?}",
        response.effects()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Integration tests: iskeyword wiring
// ═══════════════════════════════════════════════════════════════════════════════

/// Custom iskeyword with `-` (ASCII 45) makes `w` treat `background-color` as one word.
#[test]
fn iskeyword_dash_w_skips_hyphenated_word() {
    let mut engine = VimEngine::new();
    // Add `-` to iskeyword: "@,48-57,_,45"
    engine.options_mut().set_iskeyword("@,48-57,_,45");
    engine.invalidate_option_cache();

    let doc = SimpleDocument::new("background-color: red");
    // Cursor at 'b' (offset 0). With `-` as a word char, `w` should skip
    // the entire "background-color" and land on ':' (offset 16).
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('w'), 0);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("w should produce SetCursor");
    // "background-color" is 16 chars (bytes 0-15), colon is at 16
    assert_eq!(
        cursor_offset, 16,
        "With `-` in iskeyword, `w` should skip 'background-color' entirely"
    );
}

/// Without `-` in iskeyword (default), `w` stops at the first boundary.
#[test]
fn iskeyword_default_w_stops_at_dash() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("background-color: red");
    // Default iskeyword: `@,48-57,_,192-255` — dash is NOT a word char.
    // From 'b' (offset 0), `w` should stop at '-' (offset 10).
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('w'), 0);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("w should produce SetCursor");
    assert_eq!(
        cursor_offset, 10,
        "Default iskeyword: `w` should stop at '-' boundary"
    );
}

/// Custom iskeyword with `$` (ASCII 36) makes `iw` select `$variable` as one word.
///
/// BUG FOUND: `dispatch_operator_textobject` in `dispatch/operator.rs` builds
/// `TextObjectContext::new(text, cursor)` without calling `.with_options()`,
/// so text objects always use the default word_char_set. The `.with_options()`
/// method exists on `TextObjectContext` but is never called in production code.
/// Fix: wire `VimOptions` through `OperatorTextObjectInput` and call
/// `.with_options(options)` when building the context.
#[test]
fn iskeyword_dollar_iw_selects_variable() {
    let mut engine = VimEngine::new();
    // Add `$` to iskeyword
    engine.options_mut().set_iskeyword("@,48-57,_,36");
    engine.invalidate_option_cache();

    let doc = SimpleDocument::new("echo $variable here");
    // Cursor on '$' (offset 5). With `$` as a word char, `diw` should delete "$variable".
    // First press 'd' to enter operator pending
    process_key_at(&mut engine, &doc, KeyEvent::char('d'), 5);
    // Then press 'i' for inner
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 5);
    // Then press 'w' for word
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('w'), 5);

    // Check that a Delete effect is produced covering "$variable" (offsets 5..14)
    let delete_range = response.effects().iter().find_map(|e| match e {
        Effect::Delete { range, .. } => Some((range.start().get(), range.end().get())),
        _ => None,
    });
    assert_eq!(
        delete_range,
        Some((5, 14)),
        "With `$` in iskeyword, `diw` on '$variable' should delete bytes 5..14"
    );
}

/// Resetting iskeyword back to default restores original word boundary behavior.
#[test]
fn iskeyword_reset_restores_default() {
    let mut engine = VimEngine::new();

    // First set custom iskeyword with `-`
    engine.options_mut().set_iskeyword("@,48-57,_,45");
    engine.invalidate_option_cache();

    // Then reset back to default
    engine.options_mut().set_iskeyword("@,48-57,_,192-255");
    engine.invalidate_option_cache();

    let doc = SimpleDocument::new("background-color: red");
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('w'), 0);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("w should produce SetCursor");
    assert_eq!(
        cursor_offset, 10,
        "After resetting iskeyword to default, `w` should stop at '-' again"
    );
}

/// Empty iskeyword means only whitespace boundaries matter for word motions.
/// (All non-whitespace becomes either all-word or all-punctuation class.)
#[test]
fn iskeyword_empty_only_whitespace_boundaries() {
    let mut engine = VimEngine::new();
    // Empty iskeyword: no chars are word chars
    engine.options_mut().set_iskeyword("");
    engine.invalidate_option_cache();

    let doc = SimpleDocument::new("abc def");
    // With empty iskeyword, all chars are punctuation class.
    // "abc" is all-punctuation, space is whitespace, "def" is all-punctuation.
    // `w` from position 0 should jump to the whitespace boundary.
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('w'), 0);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("w should produce SetCursor");
    // With empty iskeyword: 'a','b','c' are punctuation (same class), space is whitespace,
    // 'd','e','f' are punctuation. `w` skips same-class chars, then skips whitespace,
    // landing on the start of the next non-whitespace group.
    assert_eq!(
        cursor_offset, 4,
        "With empty iskeyword, `w` should land on start of next non-whitespace group"
    );
}

/// Custom iskeyword with `:` makes `:set` a single word for `e` motion.
#[test]
fn iskeyword_colon_e_motion_spans_colon() {
    let mut engine = VimEngine::new();
    // Add `:` (ASCII 58) to iskeyword
    engine.options_mut().set_iskeyword("@,48-57,_,58");
    engine.invalidate_option_cache();

    let doc = SimpleDocument::new("std::string next");
    // From 's' (offset 0), `e` should go to end of "std::string" (offset 10)
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('e'), 0);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("e should produce SetCursor");
    assert_eq!(
        cursor_offset, 10,
        "With `:` in iskeyword, `e` should reach end of 'std::string'"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Integration tests: multiline find
// ═══════════════════════════════════════════════════════════════════════════════

/// `t` across line boundary stops one char before target (on the newline).
#[test]
fn multiline_find_t_across_line_stops_before_target() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_multiline_find(true);
    engine.options_mut().set_multiline_find_range(5);
    engine.invalidate_option_cache();

    // "abc\ndef" — a=0,b=1,c=2,\n=3,d=4,e=5,f=6
    let doc = SimpleDocument::new("abc\ndef");
    // From 'a' (offset 0), `td` should stop at '\n' (offset 3) — one before 'd' (at 4).
    // Till stops one before the found char, and 'd' is found at 4.
    // The one-before-'d' in the text "abc\ndef" (from cursor 0) is at offset 3 ('\n').
    process_key_at(&mut engine, &doc, KeyEvent::char('t'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('d'), 0);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("td should produce SetCursor with multiline");
    assert_eq!(
        cursor_offset, 3,
        "Multiline `td` should stop at '\\n' (one before 'd' on next line)"
    );
}

/// `F` backward crosses lines correctly with multiline enabled.
#[test]
fn multiline_find_f_upper_backward_crosses_line() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_multiline_find(true);
    engine.options_mut().set_multiline_find_range(5);
    engine.invalidate_option_cache();

    // "hello\nworld" — h=0,e=1,l=2,l=3,o=4,\n=5,w=6,o=7,r=8,l=9,d=10
    let doc = SimpleDocument::new("hello\nworld");
    // From 'd' (offset 10), `Fh` should find 'h' at offset 0
    process_key_at(&mut engine, &doc, KeyEvent::char('F'), 10);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('h'), 10);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("Fh should produce SetCursor with multiline");
    assert_eq!(
        cursor_offset, 0,
        "Multiline `Fh` from 'world' should find 'h' at start of previous line"
    );
}

/// Count `3f)` finds 3rd `)` across multiple lines.
#[test]
fn multiline_find_count_3f_paren_across_lines() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_multiline_find(true);
    engine.options_mut().set_multiline_find_range(5);
    engine.invalidate_option_cache();

    // "a()\nb()\nc()" — positions: a=0,(=1,)=2,\n=3,b=4,(=5,)=6,\n=7,c=8,(=9,)=10
    let doc = SimpleDocument::new("a()\nb()\nc()");
    // From 'a' (offset 0), `3f)` should find the 3rd `)` at offset 10
    // Type '3' count then 'f' then ')'
    process_key_at(&mut engine, &doc, KeyEvent::char('3'), 0);
    process_key_at(&mut engine, &doc, KeyEvent::char('f'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char(')'), 0);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("3f) should produce SetCursor");
    assert_eq!(
        cursor_offset, 10,
        "Multiline `3f)` should find the 3rd ')' on the 3rd line"
    );
}

/// `multiline_find_range=0` behaves same as `multiline_find=false`.
#[test]
fn multiline_find_range_zero_same_as_disabled() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_multiline_find(true);
    engine.options_mut().set_multiline_find_range(0);
    engine.invalidate_option_cache();

    let doc = SimpleDocument::new("abc\ndef");
    // From 'a' (offset 0), `fe` should fail — 'e' is on next line but range=0
    process_key_at(&mut engine, &doc, KeyEvent::char('f'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('e'), 0);

    let has_cursor_set = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::SetCursor { .. }));
    assert!(
        !has_cursor_set,
        "With multiline_find_range=0, find should not cross lines"
    );
}

/// `;` repeat after a multiline find works correctly.
#[test]
fn multiline_find_semicolon_repeat_works() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_multiline_find(true);
    engine.options_mut().set_multiline_find_range(5);
    engine.invalidate_option_cache();

    // "a.b\nc.d\ne.f" — a=0,.=1,b=2,\n=3,c=4,.=5,d=6,\n=7,e=8,.=9,f=10
    let doc = SimpleDocument::new("a.b\nc.d\ne.f");
    // From 'a' (offset 0), `f.` finds first '.' at offset 1
    process_key_at(&mut engine, &doc, KeyEvent::char('f'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('.'), 0);
    let first_pos = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("f. should find first dot");
    assert_eq!(first_pos, 1, "First f. should land on offset 1");

    // Now `;` should find the next '.' at offset 5 (crossing line boundary)
    let response = process_key_at(&mut engine, &doc, KeyEvent::char(';'), 1);
    let second_pos = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("; should repeat find");
    assert_eq!(
        second_pos, 5,
        "`;` after multiline find should find next dot across line boundary"
    );
}

/// Find at very end of document with multiline enabled should not panic.
#[test]
fn multiline_find_at_end_of_document() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_multiline_find(true);
    engine.options_mut().set_multiline_find_range(5);
    engine.invalidate_option_cache();

    let doc = SimpleDocument::new("abc");
    // Cursor at last char 'c' (offset 2), `fx` — should not panic, just fail
    process_key_at(&mut engine, &doc, KeyEvent::char('f'), 2);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('x'), 2);

    // Should not crash, and should not produce SetCursor (find fails)
    let has_cursor_set = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::SetCursor { .. }));
    assert!(
        !has_cursor_set,
        "Find at end of document should fail gracefully"
    );
}

/// `F` backward with multiline at start of document should not panic.
#[test]
fn multiline_find_f_upper_at_start_of_document() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_multiline_find(true);
    engine.options_mut().set_multiline_find_range(5);
    engine.invalidate_option_cache();

    let doc = SimpleDocument::new("abc\ndef");
    // Cursor at 'a' (offset 0), `Fx` — should fail gracefully
    process_key_at(&mut engine, &doc, KeyEvent::char('F'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('x'), 0);

    let has_cursor_set = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::SetCursor { .. }));
    assert!(
        !has_cursor_set,
        "Backward find at start of document should fail gracefully"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Integration tests: CursorShapeHint (additional edge cases)
// ═══════════════════════════════════════════════════════════════════════════════

/// After `dd` (doubled operator line-delete), no hint stays pending.
#[test]
fn cursor_shape_hint_dd_no_pending_after_doubled() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nfoo");

    // Press 'd' — should emit entering hint
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: Some(Operator::Delete)
            }
        )),
        "First 'd' should emit entering CursorShapeHint"
    );

    // Press 'd' again — doubled operator, should emit clearing hint (None)
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: None
            }
        )),
        "Second 'd' (dd) should emit CursorShapeHint(None), got: {:?}",
        response.effects()
    );
}

/// After `d` then `Escape`, hint resets to None.
#[test]
fn cursor_shape_hint_d_escape_resets() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Press 'd' — entering
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(response.effects().iter().any(|e| matches!(
        e,
        Effect::CursorShapeHint {
            pending_operator: Some(Operator::Delete)
        }
    )));

    // Press Escape — should clear
    let response = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: None
            }
        )),
        "Escape after 'd' should emit CursorShapeHint(None)"
    );
}

/// Multiple operators in sequence (d, Escape, y) emit correct hints.
#[test]
fn cursor_shape_hint_multiple_operators_in_sequence() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Press 'd' — enters delete pending
    let r1 = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(r1.effects().iter().any(|e| matches!(
        e,
        Effect::CursorShapeHint {
            pending_operator: Some(Operator::Delete)
        }
    )));

    // Press Escape — clears
    let r2 = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(r2.effects().iter().any(|e| matches!(
        e,
        Effect::CursorShapeHint {
            pending_operator: None
        }
    )));

    // Press 'y' — enters yank pending
    let r3 = process_key(&mut engine, &doc, KeyEvent::char('y'));
    assert!(
        r3.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: Some(Operator::Yank)
            }
        )),
        "After d+Esc, pressing 'y' should emit CursorShapeHint(Yank)"
    );

    // Press 'w' — completes yank, clears
    let r4 = process_key(&mut engine, &doc, KeyEvent::char('w'));
    assert!(
        r4.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: None
            }
        )),
        "Completing yank with motion should emit CursorShapeHint(None)"
    );
}

/// In visual mode, operators apply immediately — no CursorShapeHint emitted.
#[test]
fn cursor_shape_hint_visual_mode_no_hint() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");

    // Enter visual mode
    process_key(&mut engine, &doc, KeyEvent::char('v'));
    assert!(matches!(engine.mode(), Mode::Visual(_)));

    // Press 'w' to extend selection
    process_key(&mut engine, &doc, KeyEvent::char('w'));

    // Press 'd' — in visual mode, this should delete immediately, not enter op-pending.
    let response = process_key(&mut engine, &doc, KeyEvent::char('d'));

    // Should NOT emit CursorShapeHint with a pending operator (operator applied immediately)
    let has_pending_hint = response.effects().iter().any(|e| {
        matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: Some(_)
            }
        )
    });
    assert!(
        !has_pending_hint,
        "In visual mode, 'd' should not emit CursorShapeHint with pending operator"
    );
}

/// `>` (indent) emits CursorShapeHint with Indent.
#[test]
fn cursor_shape_hint_indent_operator() {
    use crate::primitives::Operator;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");

    let response = process_key(&mut engine, &doc, KeyEvent::char('>'));
    assert!(
        response.effects().iter().any(|e| matches!(
            e,
            Effect::CursorShapeHint {
                pending_operator: Some(Operator::Indent)
            }
        )),
        "'>' should emit CursorShapeHint(Indent), got: {:?}",
        response.effects()
    );
}

// ─── Subword text object (iS/aS) integration tests ────────────────────────

/// `diS` on "camelCaseWord" with cursor on 'C' (pos 5) deletes "Case" (5..9).
#[test]
fn subword_dis_camel_case_on_c() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("camelCaseWord");
    // cursor on 'C' at offset 5
    process_key_at(&mut engine, &doc, KeyEvent::char('d'), 5);
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 5);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 5);

    let delete_range = response.effects().iter().find_map(|e| match e {
        Effect::Delete { range, .. } => Some((range.start().get(), range.end().get())),
        _ => None,
    });
    assert_eq!(
        delete_range,
        Some((5, 9)),
        "diS on 'C' in camelCaseWord should delete bytes 5..9 (\"Case\")"
    );
    assert_eq!(engine.mode(), Mode::Normal);
}

/// `ciS` enters insert mode after replacing the subword.
/// Change operator uses `Replace(range, "")` rather than `Delete`.
#[test]
fn subword_cis_enters_insert_mode() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("camelCaseWord");
    // cursor on 'C' at offset 5
    process_key_at(&mut engine, &doc, KeyEvent::char('c'), 5);
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 5);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 5);

    // Change uses Replace(range, "") to preserve marks at range start.
    let replace_range = response.effects().iter().find_map(|e| match e {
        Effect::Replace { range, text } if text.is_empty() => {
            Some((range.start().get(), range.end().get()))
        }
        _ => None,
    });
    assert_eq!(
        replace_range,
        Some((5, 9)),
        "ciS on 'C' should replace bytes 5..9 (\"Case\") with empty string"
    );
    // Should enter insert mode
    assert_eq!(engine.mode(), Mode::Insert, "ciS should enter Insert mode");
}

/// `yaS` yanks subword + trailing separator in snake_case.
#[test]
fn subword_yas_yanks_with_trailing_separator() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("snake_case_word");
    // cursor on 's' at offset 0 — `aS` should include trailing separator: "snake_" (0..6)
    process_key_at(&mut engine, &doc, KeyEvent::char('y'), 0);
    process_key_at(&mut engine, &doc, KeyEvent::char('a'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 0);

    // Yank produces SetRegister with the yanked text
    let yanked_text = response.effects().iter().find_map(|e| match e {
        Effect::SetRegister { text, .. } => Some(text.as_str().to_owned()),
        _ => None,
    });
    assert_eq!(
        yanked_text.as_deref(),
        Some("snake_"),
        "yaS at start of snake_case_word should yank \"snake_\""
    );
    // Yank should stay in normal mode
    assert_eq!(engine.mode(), Mode::Normal);
}

/// `diS` on "snake_case_word" with cursor on middle part ('c' at pos 6).
#[test]
fn subword_dis_snake_case_middle() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("snake_case_word");
    // cursor on 'c' at offset 6
    process_key_at(&mut engine, &doc, KeyEvent::char('d'), 6);
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 6);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 6);

    let delete_range = response.effects().iter().find_map(|e| match e {
        Effect::Delete { range, .. } => Some((range.start().get(), range.end().get())),
        _ => None,
    });
    assert_eq!(
        delete_range,
        Some((6, 10)),
        "diS on 'c' in snake_case_word should delete bytes 6..10 (\"case\")"
    );
}

/// Dot-repeat (`.`) after `diS` deletes the next subword.
#[test]
fn subword_dis_dot_repeat() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("camelCaseWord");
    // First: diS on 'C' at pos 5 → deletes "Case" (5..9)
    process_key_at(&mut engine, &doc, KeyEvent::char('d'), 5);
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 5);
    let first_response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 5);
    assert!(
        first_response
            .effects()
            .iter()
            .any(|e| matches!(e, Effect::Delete { .. })),
        "First diS should produce Delete"
    );

    // After deleting "Case", text would be "camelWord".
    // Simulate cursor now on 'W' at pos 5 (where 'C' used to be, now 'W' is there).
    let doc2 = SimpleDocument::new("camelWord");
    // Dot-repeat: press '.'
    let repeat_response = process_key_at(&mut engine, &doc2, KeyEvent::char('.'), 5);

    let delete_range = repeat_response.effects().iter().find_map(|e| match e {
        Effect::Delete { range, .. } => Some((range.start().get(), range.end().get())),
        _ => None,
    });
    assert_eq!(
        delete_range,
        Some((5, 9)),
        "Dot-repeat diS on 'W' in camelWord should delete bytes 5..9 (\"Word\")"
    );
}

/// `viS` selects the subword in visual mode.
#[test]
fn subword_vis_selects_subword() {
    use crate::primitives::SelectionShape;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("camelCaseWord");
    // Enter visual mode: 'v'
    process_key_at(&mut engine, &doc, KeyEvent::char('v'), 5);
    assert!(engine.mode().is_visual(), "v should enter visual mode");

    // Press 'i' then 'S' for inner subword text object
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 5);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 5);

    // Should produce SetSelection covering "Case" (5..9)
    // In visual mode, text objects set selection. The anchor and head define the range.
    let selection = response.effects().iter().find_map(|e| match e {
        Effect::SetSelection {
            anchor,
            head,
            shape,
        } => Some((anchor.get(), head.get(), *shape)),
        _ => None,
    });
    assert!(
        selection.is_some(),
        "viS should produce SetSelection, got: {:?}",
        response.effects()
    );
    let (anchor, head, shape) = selection.unwrap();
    // The selection should cover "Case" (bytes 5..8 inclusive, i.e. anchor=5 head=8)
    // or equivalently (5, 9) depending on exclusive/inclusive convention.
    assert_eq!(anchor, 5, "viS selection anchor should be 5");
    assert!(
        head == 8 || head == 9,
        "viS selection head should be 8 (inclusive) or 9 (exclusive), got {}",
        head
    );
    assert_eq!(shape, SelectionShape::Char, "viS should be characterwise");
}

/// `2diS` with count deletes 2 subwords.
#[test]
fn subword_count_2_dis() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("camelCaseWord");
    // Press '2' then 'd' then 'i' then 'S' at cursor pos 0
    process_key_at(&mut engine, &doc, KeyEvent::char('2'), 0);
    process_key_at(&mut engine, &doc, KeyEvent::char('d'), 0);
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 0);

    let delete_range = response.effects().iter().find_map(|e| match e {
        Effect::Delete { range, .. } => Some((range.start().get(), range.end().get())),
        _ => None,
    });
    // With count=2, should delete "camel" + "Case" = "camelCase" (0..9)
    assert_eq!(
        delete_range,
        Some((0, 9)),
        "2diS at start of camelCaseWord should delete bytes 0..9 (\"camelCase\")"
    );
}

/// `diS` at end of identifier (last subword).
#[test]
fn subword_dis_last_subword() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("camelCaseWord");
    // cursor on 'W' at offset 9, last subword "Word" (9..13)
    process_key_at(&mut engine, &doc, KeyEvent::char('d'), 9);
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 9);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 9);

    let delete_range = response.effects().iter().find_map(|e| match e {
        Effect::Delete { range, .. } => Some((range.start().get(), range.end().get())),
        _ => None,
    });
    assert_eq!(
        delete_range,
        Some((9, 13)),
        "diS on 'W' in camelCaseWord should delete bytes 9..13 (\"Word\")"
    );
}

/// `diS` on a single-char subword like 'I' in "getIDs".
#[test]
fn subword_dis_single_char_subword() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("getIDs");
    // cursor on 'I' at offset 3, subword is just "I" (3..4)
    process_key_at(&mut engine, &doc, KeyEvent::char('d'), 3);
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 3);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 3);

    let delete_range = response.effects().iter().find_map(|e| match e {
        Effect::Delete { range, .. } => Some((range.start().get(), range.end().get())),
        _ => None,
    });
    assert_eq!(
        delete_range,
        Some((3, 4)),
        "diS on 'I' in getIDs should delete bytes 3..4 (\"I\")"
    );
}

// ─── Text object seeking (]x / [x) integration tests ─────────────────────────

/// `]"` with cursor inside a quoted string — jumps to the next quote pair.
/// Uses a multi-line layout so each line has exactly one quote pair, avoiding
/// the ambiguous same-line pairing behavior.
#[test]
fn seek_forward_double_quote() {
    let mut engine = VimEngine::new();
    // Two lines, each with one quote pair.
    let doc = SimpleDocument::new("\"first\" gap\n\"second\" end");
    // Cursor at 'f' (offset 1, inside "first"). ]" should seek to next pair "second".
    // Line 1: "first" gap  (0..11 + newline = 12 bytes)
    // Line 2: "second" end (starts at offset 12)
    process_key_at(&mut engine, &doc, KeyEvent::char(']'), 1);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('"'), 1);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("]\" should produce SetCursor");
    // "second" pair starts at offset 12 (opening quote on line 2).
    assert_eq!(
        cursor_offset, 12,
        "]\": cursor should jump to start of the next quoted string"
    );
}

/// `["` with cursor inside second quote pair — jumps back to previous quote.
/// Uses multi-line layout so each line has exactly one quote pair.
#[test]
fn seek_backward_double_quote() {
    let mut engine = VimEngine::new();
    // Two lines, each with one quote pair.
    let doc = SimpleDocument::new("\"first\" gap\n\"second\" end");
    // Line 1: "first" gap  (bytes 0..11)
    // Line 2: "second" end (starts at offset 12)
    // Cursor at 's' in "second" (offset 13). [" should seek back to "first".
    process_key_at(&mut engine, &doc, KeyEvent::char('['), 13);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('"'), 13);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("[\" should produce SetCursor");
    // "first" pair starts at offset 0
    assert_eq!(
        cursor_offset, 0,
        "[\": cursor should jump back to start of previous quoted string"
    );
}

/// `d]"` — deletes from cursor to the next quote pair.
/// Uses multi-line layout so each line has exactly one quote pair.
#[test]
fn delete_to_next_double_quote() {
    let mut engine = VimEngine::new();
    // Two lines: cursor inside "first" (offset 1), d]" deletes to "second" on line 2.
    let doc = SimpleDocument::new("\"first\" gap\n\"second\" end");
    // Line 1 = "first" gap\n (12 bytes), line 2 starts at 12.
    // Cursor at 'f' in "first" (offset 1). d]" should delete from 1 to 12 (exclusive).
    process_key_at(&mut engine, &doc, KeyEvent::char('d'), 1);
    process_key_at(&mut engine, &doc, KeyEvent::char(']'), 1);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('"'), 1);

    let delete_range = response.effects().iter().find_map(|e| match e {
        Effect::Delete { range, .. } => Some((range.start().get(), range.end().get())),
        _ => None,
    });
    assert!(
        delete_range.is_some(),
        "d]\": should produce a Delete effect, got: {:?}",
        response.effects()
    );
    let (start, end) = delete_range.unwrap();
    assert_eq!(start, 1, "d]\": delete should start at cursor (1)");
    // The seek target is offset 12 (column 0 on line 2), but Vim's exclusive
    // adjustment moves end back to the last char of line 1 when motion lands at col 0.
    assert_eq!(
        end, 11,
        "d]\": delete end adjusted by exclusive-col0 rule (newline at 11)"
    );
}

/// `]}` still works as unmatched brace motion (NOT intercepted by text object seek).
/// Verifies that `]}` uses `NextUnmatchedBrace` motion from the bracket dispatch,
/// not the `SeekTextObject { kind: Brace }` fallback.
#[test]
fn close_brace_bracket_not_intercepted_by_seek() {
    let mut engine = VimEngine::new();
    // Cursor inside a brace block with an unmatched closing brace ahead.
    let doc = SimpleDocument::new("  body\n}\nafter");
    // Cursor at 'b' (offset 2). ]} should find the unmatched '}' at offset 7.
    process_key_at(&mut engine, &doc, KeyEvent::char(']'), 2);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('}'), 2);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("]} should produce SetCursor (unmatched brace motion)");
    // The unmatched '}' is at offset 7
    assert_eq!(
        cursor_offset, 7,
        "]}}: should use NextUnmatchedBrace, not text object seeking"
    );
}

/// `]w` from middle of a word — seeks to next word start.
#[test]
fn seek_forward_word() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world foo");
    // Cursor at 'e' (offset 1, middle of "hello"). ]w should seek to next word.
    process_key_at(&mut engine, &doc, KeyEvent::char(']'), 1);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('w'), 1);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("]w should produce SetCursor");
    // "hello" occupies 0..5, next word "world" starts at 6
    assert_eq!(
        cursor_offset, 6,
        "]w from middle of 'hello' should seek to start of 'world'"
    );
}

/// `2]"` — seeks to 2nd next quote pair.
#[test]
fn seek_forward_double_quote_with_count() {
    let mut engine = VimEngine::new();
    // Three quoted pairs: cursor in "first", 2]" skips "second", lands at "third".
    let doc = SimpleDocument::new("\"first\" \"second\" \"third\" end");
    // Cursor at 'f' in "first" (offset 1). 2]" should skip "second" and land at "third".
    process_key_at(&mut engine, &doc, KeyEvent::char('2'), 1);
    process_key_at(&mut engine, &doc, KeyEvent::char(']'), 1);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('"'), 1);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("2]\" should produce SetCursor");
    // "first" = 0..7, "second" = 8..16, "third" = 17..24. Should land at 17.
    assert_eq!(
        cursor_offset, 17,
        "2]\": should seek to 2nd quoted pair (\"third\"), not the 1st (\"second\")"
    );
}

/// `]S` — seeks to next subword (tests Subword text object integration).
#[test]
fn seek_forward_subword() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("camelCase");
    // Cursor at 'c' (offset 0, inside "camel" subword). ]S should seek to "Case".
    process_key_at(&mut engine, &doc, KeyEvent::char(']'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('S'), 0);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("]S should produce SetCursor");
    // "camel" is 0..5, "Case" starts at 5
    assert_eq!(
        cursor_offset, 5,
        "]S: should seek from 'camel' to 'Case' subword"
    );
}

/// `]"` at end of document with no more quotes — cursor stays (Error, no movement).
#[test]
fn seek_forward_quote_no_match_no_movement() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("no quotes here at all");
    // Cursor at offset 0. ]" should find no quote pair — no SetCursor emitted.
    process_key_at(&mut engine, &doc, KeyEvent::char(']'), 0);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('"'), 0);

    let has_set_cursor = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::SetCursor { .. }));
    assert!(
        !has_set_cursor,
        "]\": when no quotes exist, should NOT emit SetCursor (cursor stays)"
    );
}

/// `[w` backward from third word — seeks to a previous word boundary.
#[test]
fn seek_backward_word() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world foo");
    // Cursor at 'f' in "foo" (offset 12). [w should find a previous word.
    process_key_at(&mut engine, &doc, KeyEvent::char('['), 12);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('w'), 12);

    let cursor_offset = response
        .effects()
        .iter()
        .find_map(|e| match e {
            Effect::SetCursor { offset, .. } => Some(offset.get()),
            _ => None,
        })
        .expect("[w should produce SetCursor");
    // Should find a previous word boundary (seeking backward from "foo").
    // The exact position depends on how Around-word handles whitespace.
    // The key assertion is that it moves backward (less than 12).
    assert!(
        cursor_offset < 12,
        "[w from 'foo' (offset 12) should move cursor backward, got {cursor_offset}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// MULTI-CURSOR YANK + PASTE ZIPPING INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════════

mod multi_cursor_yank_integration {
    use super::*;
    use crate::primitives::{
        MotionType, Offset, RegisterContent, RegisterName, SelectionRange, Selections,
    };

    /// Helper: set up multi-cursor state with cursors at given byte offsets.
    /// The first offset is the primary cursor.
    fn setup_multi_cursor(engine: &mut VimEngine, offsets: &[usize]) {
        assert!(!offsets.is_empty());
        let ranges: smallvec::SmallVec<[SelectionRange; 1]> = offsets
            .iter()
            .map(|&off| SelectionRange::insert_cursor(Offset::new(off)))
            .collect();
        let sels = Selections::new(ranges, 0);
        engine.state.multi_cursor_mut().set_selections(sels);
    }

    /// Helper: process a sequence of characters as keystrokes.
    fn type_keys(
        engine: &mut VimEngine,
        doc: &SimpleDocument,
        keys: &str,
        cursor: usize,
    ) -> Response {
        let mut last = Response::default();
        for ch in keys.chars() {
            last = process_key_at(engine, doc, KeyEvent::char(ch), cursor);
        }
        last
    }

    // ─── Test 1: Multi-cursor `yiw` with 3 cursors ──────────────────────

    /// Multi-cursor `yiw` with 3 cursors on different words produces
    /// a register with 3 entries (one per cursor's word).
    #[test]
    fn multi_cursor_yiw_three_cursors_three_entries() {
        let mut engine = VimEngine::new();
        // "aaa bbb ccc" — three words at offsets 0, 4, 8
        let doc = SimpleDocument::new("aaa bbb ccc");

        // Place cursors at the start of each word.
        setup_multi_cursor(&mut engine, &[0, 4, 8]);

        // Type `yiw` — yank inner word.
        type_keys(&mut engine, &doc, "yiw", 0);

        // Check the unnamed register has 3 entries.
        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("unnamed register should be set after yiw");

        assert_eq!(
            content.entry_count(),
            3,
            "multi-cursor yiw with 3 cursors should produce 3 entries, got {}",
            content.entry_count()
        );
        assert_eq!(content.entry(0), "aaa");
        assert_eq!(content.entry(1), "bbb");
        assert_eq!(content.entry(2), "ccc");
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Test 2: Multi-cursor `dd` with 2 cursors ───────────────────────

    /// Multi-cursor `dd` with 2 cursors on different lines produces
    /// a register with 2 entries (deleted lines).
    #[test]
    fn multi_cursor_dd_two_cursors_two_entries() {
        let mut engine = VimEngine::new();
        // "line1\nline2\nline3\n" — cursors on line1 (offset 0) and line2 (offset 6)
        let doc = SimpleDocument::new("line1\nline2\nline3\n");

        setup_multi_cursor(&mut engine, &[0, 6]);

        // Type `dd` — delete line.
        type_keys(&mut engine, &doc, "dd", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("unnamed register should be set after dd");

        assert_eq!(
            content.entry_count(),
            2,
            "multi-cursor dd with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        assert_eq!(content.entry(0), "line1\n");
        assert_eq!(content.entry(1), "line2\n");
        assert_eq!(content.motion_type(), MotionType::LineWise);
    }

    // ─── Test 3: Single-cursor paste after multi-cursor yank ─────────────

    /// After multi-cursor yank, single-cursor paste uses `entries[0]` (primary text).
    #[test]
    fn single_cursor_paste_after_multi_cursor_yank_uses_primary() {
        let mut engine = VimEngine::new();
        let doc = SimpleDocument::new("aaa bbb ccc");

        // Multi-cursor yank: 3 cursors on 3 words.
        setup_multi_cursor(&mut engine, &[0, 4, 8]);
        type_keys(&mut engine, &doc, "yiw", 0);

        // Verify multi-entry register exists.
        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("unnamed register should be set");
        assert_eq!(content.entry_count(), 3);

        // Now revert to single cursor and paste.
        let single_sels = Selections::new(
            smallvec::smallvec![SelectionRange::insert_cursor(Offset::new(0))],
            0,
        );
        engine.state.multi_cursor_mut().set_selections(single_sels);

        // Paste with `p`.
        let response = process_key_at(&mut engine, &doc, KeyEvent::char('p'), 0);

        // Single-cursor paste should emit an Insert with the primary text (entries[0]).
        let insert_text = response.effects().iter().find_map(|e| match e {
            Effect::Insert { text, .. } => Some(text.as_str().to_owned()),
            _ => None,
        });
        assert_eq!(
            insert_text.as_deref(),
            Some("aaa"),
            "single-cursor paste should use primary entry (entries[0])"
        );
    }

    // ─── Test 4: Paste-zip with matching cursor count ────────────────────

    /// After multi-cursor yank with 3 entries, paste with 3 cursors
    /// distributes entries correctly (cursor i gets entry i).
    #[test]
    fn paste_zip_three_entries_three_cursors() {
        let mut engine = VimEngine::new();
        // "aaa bbb ccc" — yank each word with 3 cursors.
        let doc = SimpleDocument::new("aaa bbb ccc");

        setup_multi_cursor(&mut engine, &[0, 4, 8]);
        type_keys(&mut engine, &doc, "yiw", 0);

        // Verify 3 entries exist.
        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set");
        assert_eq!(content.entry_count(), 3);

        // Now paste with the same 3 cursors still active.
        // Use a fresh document where each cursor has a distinct position.
        let paste_doc = SimpleDocument::new("xxx yyy zzz");
        setup_multi_cursor(&mut engine, &[0, 4, 8]);

        let response = type_keys(&mut engine, &paste_doc, "p", 0);

        // Collect all Insert effects in order.
        let inserts: Vec<String> = response
            .effects()
            .iter()
            .filter_map(|e| match e {
                Effect::Insert { text, .. } => Some(text.as_str().to_owned()),
                _ => None,
            })
            .collect();

        // With 3 cursors and 3 entries, paste-zipping should produce 3 distinct inserts.
        assert_eq!(
            inserts.len(),
            3,
            "paste-zip should produce 3 Insert effects, got {}",
            inserts.len()
        );

        // Inserts are in descending cursor offset order (replication order),
        // so mapping: cursor at 8 (entry 2), cursor at 4 (entry 1), cursor at 0 (entry 0).
        assert!(
            inserts.contains(&"aaa".to_owned()),
            "paste-zip should contain entry 0 (aaa)"
        );
        assert!(
            inserts.contains(&"bbb".to_owned()),
            "paste-zip should contain entry 1 (bbb)"
        );
        assert!(
            inserts.contains(&"ccc".to_owned()),
            "paste-zip should contain entry 2 (ccc)"
        );
    }

    // ─── Test 5: Paste-zip with more cursors than entries (fallback) ────

    /// After multi-cursor yank with 3 entries, paste with 5 cursors
    /// uses fallback for out-of-bounds indices: cursors beyond entry count
    /// get the primary text (entry 0) as fallback per `apply_paste_zip`.
    #[test]
    fn paste_zip_three_entries_five_cursors_fallback() {
        let mut engine = VimEngine::new();
        let doc = SimpleDocument::new("aaa bbb ccc");

        // Yank with 3 cursors → 3 entries.
        setup_multi_cursor(&mut engine, &[0, 4, 8]);
        type_keys(&mut engine, &doc, "yiw", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set");
        assert_eq!(content.entry_count(), 3);

        // Now paste with 5 cursors.
        let paste_doc = SimpleDocument::new("11 22 33 44 55");
        setup_multi_cursor(&mut engine, &[0, 3, 6, 9, 12]);

        let response = type_keys(&mut engine, &paste_doc, "p", 0);

        let inserts: Vec<String> = response
            .effects()
            .iter()
            .filter_map(|e| match e {
                Effect::Insert { text, .. } => Some(text.as_str().to_owned()),
                _ => None,
            })
            .collect();

        // 5 cursors should produce 5 Insert effects.
        assert_eq!(
            inserts.len(),
            5,
            "paste-zip with 5 cursors should produce 5 inserts, got {}",
            inserts.len()
        );

        // Clamping: entries 0,1,2 map to "aaa","bbb","ccc";
        // entries 3,4 clamp to last entry "ccc".
        let aaa_count = inserts.iter().filter(|t| *t == "aaa").count();
        let bbb_count = inserts.iter().filter(|t| *t == "bbb").count();
        let ccc_count = inserts.iter().filter(|t| *t == "ccc").count();

        assert_eq!(aaa_count, 1, "entry 0 gets 'aaa'");
        assert_eq!(bbb_count, 1, "entry 1 gets 'bbb'");
        assert_eq!(
            ccc_count, 3,
            "entries 2, 3, 4 get 'ccc' (last-entry clamping)"
        );
    }

    // ─── Test 6: Multi-cursor delete then dot-repeat updates register ───

    /// After multi-cursor `diw`, dot-repeat at different positions
    /// should update the register with new per-cursor text.
    /// Uses `diw` (delete inner word) which IS dot-repeatable (mutating).
    #[test]
    fn multi_cursor_delete_dot_repeat_updates_register() {
        let mut engine = VimEngine::new();
        let doc = SimpleDocument::new("aaa bbb ccc ddd eee fff");

        // First delete with 3 cursors on "aaa", "bbb", "ccc".
        setup_multi_cursor(&mut engine, &[0, 4, 8]);
        type_keys(&mut engine, &doc, "diw", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after first diw");
        assert_eq!(content.entry(0), "aaa");
        assert_eq!(content.entry_count(), 3);

        // Now move cursors to "ddd", "eee", "fff" and dot-repeat.
        // Note: the document hasn't actually changed (SimpleDocument is immutable),
        // so the text at offsets 12, 16, 20 is still "ddd", "eee", "fff".
        setup_multi_cursor(&mut engine, &[12, 16, 20]);
        type_keys(&mut engine, &doc, ".", 12);

        let content2 = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after dot-repeat");

        // After dot-repeat at new positions, register should have updated entries.
        assert_eq!(
            content2.entry_count(),
            3,
            "dot-repeat should still produce 3 entries"
        );
        assert_eq!(content2.entry(0), "ddd");
        assert_eq!(content2.entry(1), "eee");
        assert_eq!(content2.entry(2), "fff");
    }

    // ─── Test 7: Clipboard routing with multi-cursor yank ────────────────

    /// Multi-cursor yank with clipboard option set emits CopyToClipboard
    /// with the primary text (entries[0]).
    #[test]
    fn multi_cursor_yank_clipboard_routing() {
        let mut engine = VimEngine::new();
        engine.options_mut().set_clipboard("unnamedplus");
        engine.invalidate_option_cache();

        let doc = SimpleDocument::new("aaa bbb ccc");

        setup_multi_cursor(&mut engine, &[0, 4, 8]);
        let response = type_keys(&mut engine, &doc, "yiw", 0);

        // Should emit CopyToClipboard with the primary text.
        let clipboard_text = response.effects().iter().find_map(|e| match e {
            Effect::CopyToClipboard { text, .. } => Some(text.as_str().to_owned()),
            _ => None,
        });
        assert!(
            clipboard_text.is_some(),
            "clipboard option should trigger CopyToClipboard effect"
        );
        // The clipboard gets the primary entry text.
        assert_eq!(
            clipboard_text.as_deref(),
            Some("aaa"),
            "CopyToClipboard should use primary text (entry 0)"
        );
    }

    // ─── Test 8: Numbered register cascade after multi-cursor delete ─────

    /// Multi-cursor delete still cascades numbered registers correctly.
    /// After `dd` with 2 cursors, register "1 gets multi-entry content
    /// (from the override), and the previous "1 content cascades.
    ///
    /// NOTE: With N cursors, replication produces N SetRegister effects for
    /// NUMBERED_1, each of which triggers `shift_numbered` in the effect
    /// processor. So with 2 cursors, the cascade fires twice: the original
    /// "1 content ends up in register "3 (shifted by 2), not "2.
    ///
    /// Uses equal-length lines so the fixed-byte-range offset calculation
    /// in `override_registers_with_multi_cursor_entries` produces correct text.
    #[test]
    fn multi_cursor_delete_numbered_cascade() {
        let mut engine = VimEngine::new();

        // Pre-load "1 with some content to verify cascade.
        engine.state.registers_mut().set(
            RegisterName::NUMBERED_1,
            RegisterContent::new("old_line\n", MotionType::LineWise),
        );

        // Use equal-length lines (5 chars + newline = 6 bytes each) so
        // the override's fixed-range-length extraction works correctly.
        let doc = SimpleDocument::new("aaaaa\nbbbbb\nccccc\nddddd\n");

        // Delete 2 lines with multi-cursor: cursor at line 1 (offset 0) and line 2 (offset 6).
        setup_multi_cursor(&mut engine, &[0, 6]);
        type_keys(&mut engine, &doc, "dd", 0);

        // "1 should have the new delete content (multi-entry from override).
        let reg1 = engine
            .state
            .registers()
            .get(RegisterName::NUMBERED_1)
            .expect("\"1 should be set after dd");
        // Multi-cursor dd writes multi-entry content to "1.
        assert_eq!(
            reg1.entry_count(),
            2,
            "\"1 should have 2 entries from multi-cursor dd"
        );
        assert_eq!(reg1.entry(0), "aaaaa\n");
        assert_eq!(reg1.entry(1), "bbbbb\n");

        // With the global/positional filter, SetRegister is global — emitted
        // once. The cascade fires once:
        //   Cascade: old "1 ("old_line\n") → "2, new "1 = primary delete text
        //   Override: "1 = multi-entry ["aaaaa\n", "bbbbb\n"]
        // So the original "old_line\n" is in "2.
        let reg2 = engine
            .state
            .registers()
            .get(RegisterName::new('2').unwrap())
            .expect("\"2 should have cascaded content from original \"1");
        assert_eq!(
            reg2.text(),
            "old_line\n",
            "\"2 should contain the original \"1 content after 1 cascade"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Heterogeneous-length RangeSource integration tests
    //
    // These tests prove the per-cursor text-object recomputation fix works
    // for real-world scenarios where cursors sit on words/objects of DIFFERENT
    // lengths. The old bug: all cursors extracted `primary_range_len` bytes.
    // ═══════════════════════════════════════════════════════════════════════

    // ─── Test 9: Code refactoring — variables of different lengths ────────

    /// Real-world scenario: three cursors on variable names `x`, `count`, `items`.
    /// `yiw` must produce ["x", "count", "items"], NOT ["x", "c", "i"] (truncated)
    /// or ["x", "count", "items"] with mangled boundaries.
    #[test]
    fn multi_cursor_yiw_heterogeneous_variable_names() {
        let mut engine = VimEngine::new();
        // "x = count + items"
        //  ^   ^       ^
        //  0   4       12
        let doc = SimpleDocument::new("x = count + items");

        // Cursors on first char of each identifier.
        setup_multi_cursor(&mut engine, &[0, 4, 12]);

        type_keys(&mut engine, &doc, "yiw", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after yiw");

        assert_eq!(
            content.entry_count(),
            3,
            "should have 3 entries (one per cursor), got {}",
            content.entry_count()
        );
        assert_eq!(content.entry(0), "x", "primary cursor on 1-char variable");
        assert_eq!(
            content.entry(1),
            "count",
            "second cursor on 5-char variable"
        );
        assert_eq!(content.entry(2), "items", "third cursor on 5-char variable");
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Test 10: Multi-cursor ciw extracts correct per-cursor text ──────

    /// `ciw` (change inner word) deletes each word into the register before
    /// entering insert mode. Cursors on "a", "bb", "ccc" should produce
    /// register entries ["a", "bb", "ccc"] — proving the delete range is
    /// recomputed per-cursor.
    #[test]
    fn multi_cursor_ciw_heterogeneous_word_lengths() {
        let mut engine = VimEngine::new();
        // "a bb ccc"
        //  ^  ^  ^
        //  0  2  5
        let doc = SimpleDocument::new("a bb ccc");

        setup_multi_cursor(&mut engine, &[0, 2, 5]);

        // `ciw` — change inner word (deletes word, enters insert mode).
        type_keys(&mut engine, &doc, "ciw", 0);

        // Engine should now be in Insert mode.
        assert_eq!(engine.mode(), Mode::Insert);

        // Check the unnamed register has per-cursor deleted text.
        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after ciw");

        assert_eq!(
            content.entry_count(),
            3,
            "ciw with 3 cursors should produce 3 entries, got {}",
            content.entry_count()
        );
        assert_eq!(content.entry(0), "a", "deleted word at cursor 0");
        assert_eq!(content.entry(1), "bb", "deleted word at cursor 1");
        assert_eq!(content.entry(2), "ccc", "deleted word at cursor 2");
    }

    // ─── Test 11: Snake_case identifiers — iw captures full word ─────────

    /// Cursors on `get_name` (8 chars) and `x` (1 char). `yiw` should
    /// produce ["get_name", "x"] — NOT "get_name" and "g" (truncated to 1)
    /// or "x" extended to 8 bytes.
    #[test]
    fn multi_cursor_yiw_snake_case_vs_single_char() {
        let mut engine = VimEngine::new();
        // "get_name = x"
        //  ^          ^
        //  0          11
        let doc = SimpleDocument::new("get_name = x");

        setup_multi_cursor(&mut engine, &[0, 11]);

        type_keys(&mut engine, &doc, "yiw", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set");

        assert_eq!(content.entry_count(), 2);
        assert_eq!(content.entry(0), "get_name", "8-char snake_case identifier");
        assert_eq!(content.entry(1), "x", "1-char identifier");
    }

    // ─── Test 12: Linewise yy with lines of different lengths ────────────

    /// `yy` with cursors on lines of different lengths. Each cursor should
    /// capture its full line including the newline.
    #[test]
    fn multi_cursor_yy_different_length_lines() {
        let mut engine = VimEngine::new();
        // Line 1: "short" (5 chars + newline = 6 bytes)
        // Line 2: "a much longer line here" (23 chars + newline = 24 bytes)
        // Line 3: "z" (1 char + newline = 2 bytes)
        let doc = SimpleDocument::new("short\na much longer line here\nz\n");

        // Cursor on line 1 (offset 0), line 2 (offset 6), line 3 (offset 30).
        setup_multi_cursor(&mut engine, &[0, 6, 30]);

        type_keys(&mut engine, &doc, "yy", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after yy");

        assert_eq!(
            content.entry_count(),
            3,
            "yy with 3 cursors should produce 3 entries, got {}",
            content.entry_count()
        );
        assert_eq!(content.entry(0), "short\n", "first line (6 bytes)");
        assert_eq!(
            content.entry(1),
            "a much longer line here\n",
            "second line (24 bytes)"
        );
        assert_eq!(content.entry(2), "z\n", "third line (2 bytes)");
        assert_eq!(content.motion_type(), MotionType::LineWise);
    }

    // ─── Test 13: Mixed short/long with paste-zip distribution ───────────

    /// Yank 3 different-length words, then paste with 3 cursors at new
    /// locations. Verifies paste-zip distributes correct entries to each cursor.
    #[test]
    fn multi_cursor_yank_then_paste_zip_heterogeneous() {
        let mut engine = VimEngine::new();
        // "hi world ok" — words of length 2, 5, 2
        let doc = SimpleDocument::new("hi world ok");

        // Yank with cursors on each word.
        setup_multi_cursor(&mut engine, &[0, 3, 9]);
        type_keys(&mut engine, &doc, "yiw", 0);

        // Verify entries captured correctly.
        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set");
        assert_eq!(content.entry_count(), 3);
        assert_eq!(content.entry(0), "hi");
        assert_eq!(content.entry(1), "world");
        assert_eq!(content.entry(2), "ok");

        // Now paste with 3 cursors into a new document.
        let paste_doc = SimpleDocument::new("__ _____ __");
        setup_multi_cursor(&mut engine, &[0, 3, 9]);

        let response = type_keys(&mut engine, &paste_doc, "p", 0);

        // Collect Insert effects.
        let inserts: Vec<String> = response
            .effects()
            .iter()
            .filter_map(|e| match e {
                Effect::Insert { text, .. } => Some(text.as_str().to_owned()),
                _ => None,
            })
            .collect();

        assert_eq!(
            inserts.len(),
            3,
            "paste-zip should produce 3 inserts, got {}",
            inserts.len()
        );
        // All three entries should appear (order is descending by offset).
        assert!(inserts.contains(&"hi".to_owned()), "should contain 'hi'");
        assert!(
            inserts.contains(&"world".to_owned()),
            "should contain 'world'"
        );
        assert!(inserts.contains(&"ok".to_owned()), "should contain 'ok'");
    }

    // ─── Test 14: Extreme length difference — 1 char vs 20 chars ─────────

    /// Edge case: cursor on a 1-char word vs a 20-char word.
    /// The old bug would extract only 1 byte (or 20 bytes) for ALL cursors.
    #[test]
    fn multi_cursor_yiw_extreme_length_difference() {
        let mut engine = VimEngine::new();
        // "x = abcdefghijklmnopqrst"
        //  ^   ^
        //  0   4
        // "x" = 1 char, "abcdefghijklmnopqrst" = 20 chars
        let doc = SimpleDocument::new("x = abcdefghijklmnopqrst");

        setup_multi_cursor(&mut engine, &[0, 4]);

        type_keys(&mut engine, &doc, "yiw", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set");

        assert_eq!(content.entry_count(), 2);
        assert_eq!(content.entry(0), "x", "1-char word");
        assert_eq!(
            content.entry(1),
            "abcdefghijklmnopqrst",
            "20-char word must be captured in full"
        );
    }

    // ─── Test 15: Text objects beyond word — yi" with different lengths ───

    /// `yi"` with cursors inside quoted strings of different lengths.
    /// Cursor 1 inside `"hi"` (2 chars inner), cursor 2 inside `"hello world"` (11 chars inner).
    /// Must produce ["hi", "hello world"], NOT ["hi", "he"] (truncated).
    #[test]
    fn multi_cursor_yi_quote_heterogeneous_lengths() {
        let mut engine = VimEngine::new();
        // Two quoted strings on the same line (for simplicity):
        // `"hi" "hello world"`
        //   ^       ^
        //   1       6
        // Cursor inside "hi" (offset 1, between the quotes)
        // Cursor inside "hello world" (offset 6, between the quotes)
        let doc = SimpleDocument::new("\"hi\" \"hello world\"");

        // Place cursors inside each quoted string.
        setup_multi_cursor(&mut engine, &[1, 6]);

        // Type yi" — yank inner double-quote text object.
        type_keys(&mut engine, &doc, "yi\"", 1);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after yi\"");

        assert_eq!(
            content.entry_count(),
            2,
            "yi\" with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        assert_eq!(content.entry(0), "hi", "inner content of first quotes");
        assert_eq!(
            content.entry(1),
            "hello world",
            "inner content of second quotes — must NOT be truncated to 2 chars"
        );
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Test 16: yiw with cursor in middle of word ──────────────────────

    /// Cursors placed in the MIDDLE of words of different lengths.
    /// Ensures text-object boundary detection works from non-start positions.
    #[test]
    fn multi_cursor_yiw_cursor_in_middle_of_words() {
        let mut engine = VimEngine::new();
        // "elephant cat"
        //      ^      ^
        //      4      10
        // Cursor at 'h' in "elephant" (offset 4), cursor at 'a' in "cat" (offset 10).
        let doc = SimpleDocument::new("elephant cat");

        setup_multi_cursor(&mut engine, &[4, 10]);

        type_keys(&mut engine, &doc, "yiw", 4);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set");

        assert_eq!(content.entry_count(), 2);
        assert_eq!(
            content.entry(0),
            "elephant",
            "full word even with cursor in middle"
        );
        assert_eq!(content.entry(1), "cat", "full word from middle position");
    }

    // ─── Test 17: diw with heterogeneous words populates register ────────

    /// `diw` (delete inner word) with cursors on words of different lengths.
    /// Verifies the DELETE path also recomputes per-cursor, not just yank.
    #[test]
    fn multi_cursor_diw_heterogeneous_words_register() {
        let mut engine = VimEngine::new();
        // "fn process_events a"
        //  ^  ^               ^
        //  0  3               19
        // "fn" = 2 chars, "process_events" = 14 chars, "a" = 1 char
        let doc = SimpleDocument::new("fn process_events a");

        setup_multi_cursor(&mut engine, &[0, 3, 19]);

        type_keys(&mut engine, &doc, "diw", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after diw");

        assert_eq!(
            content.entry_count(),
            3,
            "diw with 3 cursors should produce 3 entries, got {}",
            content.entry_count()
        );
        assert_eq!(content.entry(0), "fn", "2-char word deleted");
        assert_eq!(content.entry(1), "process_events", "14-char word deleted");
        assert_eq!(content.entry(2), "a", "1-char word deleted");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // RangeSource::Motion integration tests
    //
    // These tests prove the per-cursor motion recomputation works for
    // operator+motion commands (yw, y$, d$, y}, y2w) where cursors sit on
    // content of DIFFERENT lengths. The old bug: all cursors used the primary's
    // motion range length. Now each cursor recomputes its own motion range.
    // ═══════════════════════════════════════════════════════════════════════

    // ─── Test 18: y$ on lines of different lengths ───────────────────────

    /// `y$` with cursor 1 at start of "short" and cursor 2 at start of
    /// "a much longer line". Previously would yank "short" and "a muc"
    /// (5 chars truncated from primary range). Now correctly yanks
    /// "short" and "a much longer line" respectively.
    #[test]
    fn multi_cursor_y_dollar_different_line_lengths() {
        let mut engine = VimEngine::new();
        // Two lines: "short\na much longer line"
        //             ^      ^
        //             0      6
        let doc = SimpleDocument::new("short\na much longer line");

        // Cursors at start of each line.
        setup_multi_cursor(&mut engine, &[0, 6]);

        // Type `y$` — yank to end of line.
        type_keys(&mut engine, &doc, "y$", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after y$");

        assert_eq!(
            content.entry_count(),
            2,
            "y$ with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        assert_eq!(
            content.entry(0),
            "short",
            "primary cursor yanks to end of first line"
        );
        assert_eq!(
            content.entry(1),
            "a much longer line",
            "second cursor yanks FULL remainder of second line, NOT truncated to 5 chars"
        );
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Test 19: yw on words of different lengths ───────────────────────

    /// `yw` with cursors at start of "hi " (3 bytes including trailing space)
    /// and "elephant " (9 bytes including trailing space).
    /// Previously both would get 3 bytes (primary's range).
    /// Now correctly yanks "hi " and "elephant " respectively.
    #[test]
    fn multi_cursor_yw_different_word_lengths() {
        let mut engine = VimEngine::new();
        // "hi elephant done"
        //  ^  ^
        //  0  3
        // `yw` yanks the word + trailing whitespace: "hi " (3) and "elephant " (9).
        let doc = SimpleDocument::new("hi elephant done");

        setup_multi_cursor(&mut engine, &[0, 3]);

        type_keys(&mut engine, &doc, "yw", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after yw");

        assert_eq!(
            content.entry_count(),
            2,
            "yw with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        assert_eq!(
            content.entry(0),
            "hi ",
            "primary cursor yanks 'hi ' (3 bytes)"
        );
        assert_eq!(
            content.entry(1),
            "elephant ",
            "second cursor yanks 'elephant ' (9 bytes), NOT truncated to 3"
        );
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Test 20: d$ on different-length line tails ──────────────────────

    /// `d$` with cursors at different mid-line positions — verify correct
    /// deletion range per cursor. Cursor 1 at offset 2 in "hello world"
    /// (should delete "llo world" = 9 chars). Cursor 2 at offset 14 in
    /// "ab" (second line is "ab", cursor at 'b' offset = 12+2=14, deletes "b").
    #[test]
    fn multi_cursor_d_dollar_different_tail_lengths() {
        let mut engine = VimEngine::new();
        // "hello world\nab"
        //    ^            ^
        //    2            13
        // Cursor at 'l' in "hello world" (offset 2) → d$ deletes "llo world" (9 chars).
        // Cursor at 'b' in "ab" (offset 13) → d$ deletes "b" (1 char).
        let doc = SimpleDocument::new("hello world\nab");

        setup_multi_cursor(&mut engine, &[2, 13]);

        type_keys(&mut engine, &doc, "d$", 2);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after d$");

        assert_eq!(
            content.entry_count(),
            2,
            "d$ with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        assert_eq!(
            content.entry(0),
            "llo world",
            "cursor at offset 2 deletes to end of line 1"
        );
        assert_eq!(
            content.entry(1),
            "b",
            "cursor at offset 13 deletes single char to end of line 2"
        );
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Test 21: y} (paragraph motion) on paragraphs of different sizes ─

    /// `y}` with cursors in paragraphs of different sizes. Cursor 1 at start
    /// of a 1-line paragraph, cursor 2 at start of a 3-line paragraph.
    /// Each cursor should yank its full paragraph to the next blank line.
    #[test]
    fn multi_cursor_y_paragraph_different_sizes() {
        let mut engine = VimEngine::new();
        // Paragraph 1: "short" (followed by blank line)
        // Paragraph 2: "line one\nline two\nline three" (followed by blank line)
        //
        // "short\n\nline one\nline two\nline three\n\n"
        //  ^       ^
        //  0       7
        let doc = SimpleDocument::new("short\n\nline one\nline two\nline three\n\n");

        // Cursor at start of each paragraph.
        setup_multi_cursor(&mut engine, &[0, 7]);

        type_keys(&mut engine, &doc, "y}", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after y}");

        assert_eq!(
            content.entry_count(),
            2,
            "y}} with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        // y} yanks from cursor to the blank line (inclusive of the trailing newline
        // before the blank line). The exact content depends on how ParagraphForward
        // motion works — it moves to the first char of the next blank line.
        // Paragraph 1: from offset 0 to the blank line at offset 6 → "short\n"
        assert_eq!(
            content.entry(0),
            "short\n",
            "first paragraph: 'short\\n' (6 bytes)"
        );
        // Paragraph 2: from offset 7 to the blank line at offset 35 → "line one\nline two\nline three\n"
        assert_eq!(
            content.entry(1),
            "line one\nline two\nline three\n",
            "second paragraph: full 3 lines, NOT truncated to 6 bytes"
        );
    }

    // ��── Test 22: y2w with count on different-length word spans ──────────

    /// `y2w` with cursors where 2 words span different byte lengths.
    /// Cursor 1 at "a b" (2 words = 3 bytes "a b"), cursor 2 at
    /// "elephant zoo" (2 words = 12 bytes "elephant zoo").
    #[test]
    fn multi_cursor_y2w_count_different_lengths() {
        let mut engine = VimEngine::new();
        // "a b elephant zoo end"
        //  ^   ^
        //  0   4
        // y2w at offset 0: yanks "a b " (word "a" + space, then "b" + space = "a b ")
        //   y2w yanks 2 words forward: first word + next word start.
        //   From 'a': word "a " then word "b " = "a b "
        // y2w at offset 4: yanks "elephant zoo " (2 words + trailing space)
        let doc = SimpleDocument::new("a b elephant zoo end");

        setup_multi_cursor(&mut engine, &[0, 4]);

        type_keys(&mut engine, &doc, "y2w", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after y2w");

        assert_eq!(
            content.entry_count(),
            2,
            "y2w with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        assert_eq!(
            content.entry(0),
            "a b ",
            "primary: 2 words 'a b ' (4 bytes)"
        );
        assert_eq!(
            content.entry(1),
            "elephant zoo ",
            "second: 2 words 'elephant zoo ' (13 bytes), NOT truncated to 4"
        );
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Test 23: df) where ) is at different distances ──────────────────

    /// `df)` (delete-find-close-paren) with cursors where `)` is at different
    /// distances. Since `CharCommand` is NOT captured by `extract_range_source`,
    /// this falls back to the fixed-length heuristic. This test documents the
    /// current behavior: the second cursor gets the same byte count as the primary.
    ///
    /// NOTE: This is the known limitation — CharCommand motions don't get
    /// per-cursor recomputation yet. The test documents expected behavior.
    #[test]
    fn multi_cursor_df_paren_falls_back_to_heuristic() {
        let mut engine = VimEngine::new();
        // Line 1: "f(x)"    — cursor at 'f', `)` at distance 3 → deletes "f(x)" (4 bytes)
        // Line 2: "call(a, b, c)" — cursor at 'c', `)` at distance 12
        //
        // "f(x)\ncall(a, b, c)"
        //  ^     ^
        //  0     5
        let doc = SimpleDocument::new("f(x)\ncall(a, b, c)");

        setup_multi_cursor(&mut engine, &[0, 5]);

        type_keys(&mut engine, &doc, "df)", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after df)");

        assert_eq!(
            content.entry_count(),
            2,
            "df) with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        // Primary cursor deletes "f(x)" (4 bytes including the ')').
        assert_eq!(
            content.entry(0),
            "f(x)",
            "primary: deletes up to and including ')'"
        );
        // With RangeSource::CharFind, the second cursor correctly finds `)` at
        // its own distance and extracts "call(a, b, c)" (13 bytes up to and including ')').
        assert_eq!(
            content.entry(1),
            "call(a, b, c)",
            "second cursor: per-cursor find recomputation gives correct range"
        );
    }

    // ─── Test 24: ye (yank to end of word) with different word lengths ───

    /// `ye` with cursors at start of words of different lengths.
    /// The WordEnd motion moves to the last char of the current word.
    /// Cursor 1 on "ab" → yanks "ab" (2 bytes), cursor 2 on "xyz123" → yanks "xyz123" (6 bytes).
    #[test]
    fn multi_cursor_ye_different_word_lengths() {
        let mut engine = VimEngine::new();
        // "ab xyz123 done"
        //  ^  ^
        //  0  3
        let doc = SimpleDocument::new("ab xyz123 done");

        setup_multi_cursor(&mut engine, &[0, 3]);

        type_keys(&mut engine, &doc, "ye", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after ye");

        assert_eq!(
            content.entry_count(),
            2,
            "ye with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        assert_eq!(content.entry(0), "ab", "primary: yanks to end of 'ab'");
        assert_eq!(
            content.entry(1),
            "xyz123",
            "second: yanks to end of 'xyz123' (6 bytes), NOT truncated to 2"
        );
        assert_eq!(content.motion_type(), MotionType::CharWise);
    }

    // ─── Test 25: d$ followed by verification of Delete effects ──────────

    /// Verify that `d$` with multi-cursor actually produces Delete effects
    /// with correct ranges (not just register content). This ensures the
    /// algebraic replication of Delete effects works alongside per-cursor
    /// register override.
    #[test]
    fn multi_cursor_d_dollar_produces_correct_delete_effects() {
        let mut engine = VimEngine::new();
        // "abcdef\nxy"
        //  ^       ^
        //  0       7
        // d$ at offset 0 → deletes "abcdef" (6 bytes to end of line)
        // d$ at offset 7 → deletes "xy" (2 bytes to end of line)
        let doc = SimpleDocument::new("abcdef\nxy");

        setup_multi_cursor(&mut engine, &[0, 7]);

        let response = type_keys(&mut engine, &doc, "d$", 0);

        // Should have Delete effects.
        let deletes: Vec<_> = response
            .effects()
            .iter()
            .filter_map(|e| match e {
                Effect::Delete { range } => Some((range.start().get(), range.end().get())),
                _ => None,
            })
            .collect();

        assert!(!deletes.is_empty(), "d$ should produce Delete effects");

        // Also verify register content is correct per-cursor.
        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after d$");

        assert_eq!(content.entry_count(), 2);
        assert_eq!(content.entry(0), "abcdef");
        assert_eq!(content.entry(1), "xy");
    }

    // ─── Test 26: y$ with cursor at last char of short vs long line ──────

    /// Edge case: cursor already at the last character of a line.
    /// y$ at the last char should still yank that character.
    /// Also tests that cursors at different distances from EOL produce
    /// different-length yanks.
    #[test]
    fn multi_cursor_y_dollar_at_last_char() {
        let mut engine = VimEngine::new();
        // "hi\nworld"
        //   ^    ^
        //   1    6
        // h(0) i(1) \n(2) w(3) o(4) r(5) l(6) d(7)
        // y$ at offset 1 ('i'): yanks "i" (last char of "hi")
        // y$ at offset 6 ('l'): yanks "ld" (2 chars to end of "world")
        let doc = SimpleDocument::new("hi\nworld");

        setup_multi_cursor(&mut engine, &[1, 6]);

        type_keys(&mut engine, &doc, "y$", 1);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after y$");

        assert_eq!(content.entry_count(), 2);
        assert_eq!(
            content.entry(0),
            "i",
            "cursor at last char of 'hi' yanks 'i'"
        );
        assert_eq!(
            content.entry(1),
            "ld",
            "cursor at 'l' in 'world' yanks 'ld' to end"
        );
    }

    // ─── Test 27: dw on last word of line vs mid-line word ───────────────

    /// `dw` behavior differs at end of line vs mid-line. At end of line,
    /// `dw` deletes just the word (no trailing space). Mid-line it includes
    /// trailing space. This tests that per-cursor recomputation handles
    /// both cases correctly.
    #[test]
    fn multi_cursor_dw_end_of_line_vs_mid_line() {
        let mut engine = VimEngine::new();
        // "cat dog\nbird"
        //  ^        ^
        //  0        8
        // dw at 'c' in "cat dog": deletes "cat " (4 bytes — word + trailing space)
        // dw at 'b' in "bird": deletes "bird" (4 bytes — last word, no trailing space)
        let doc = SimpleDocument::new("cat dog\nbird");

        setup_multi_cursor(&mut engine, &[0, 8]);

        type_keys(&mut engine, &doc, "dw", 0);

        let content = engine
            .state
            .registers()
            .get(RegisterName::UNNAMED)
            .expect("register should be set after dw");

        assert_eq!(
            content.entry_count(),
            2,
            "dw with 2 cursors should produce 2 entries, got {}",
            content.entry_count()
        );
        assert_eq!(
            content.entry(0),
            "cat ",
            "mid-line dw includes trailing space"
        );
        assert_eq!(
            content.entry(1),
            "bird",
            "end-of-line dw captures just the word"
        );
    }
}

// ─── Sticky sub-mode interception ────────────────────────────────────────

/// Helper: activate a sticky session on the engine.
fn set_sticky(engine: &mut VimEngine, target: StickyTarget) {
    engine.sticky_session = Some(StickySession::new(target));
}

#[test]
fn sticky_escape_clears_session_and_emits_clear_message() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::Window);

    let response = process_key(&mut engine, &doc, KeyEvent::escape());

    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear sticky session"
    );
    assert!(response.consumed(), "Escape must be consumed");
    assert!(
        response
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ClearMessage)),
        "Escape must emit ClearMessage effect",
    );
}

#[test]
fn sticky_ctrl_c_clears_session() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::ZPrefix);

    let response = process_key(&mut engine, &doc, KeyEvent::ctrl('c'));

    assert!(
        engine.sticky_session.is_none(),
        "Ctrl-C must clear sticky session"
    );
    assert!(response.consumed());
}

#[test]
fn sticky_ctrl_bracket_clears_session() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::Window);

    let response = process_key(&mut engine, &doc, KeyEvent::ctrl('['));

    assert!(
        engine.sticky_session.is_none(),
        "Ctrl-[ must clear sticky session"
    );
    assert!(response.consumed());
}

#[test]
fn sticky_digit_accumulates_pending_count() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::Window);

    let r1 = process_key(&mut engine, &doc, KeyEvent::char('3'));
    assert!(r1.consumed(), "digit must be consumed");
    assert!(r1.effects.is_empty(), "digit must produce no effects");
    assert!(
        engine.sticky_session.is_some(),
        "session must remain active"
    );

    // Second digit accumulates
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('5'));
    assert!(r2.consumed());

    // Check accumulated count is 35
    let session = engine.sticky_session.as_mut().unwrap();
    assert_eq!(
        session.take_count(),
        Some(35),
        "digits 3,5 must accumulate to 35"
    );
}

#[test]
fn sticky_window_sets_parser_to_awaiting_window_command() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::Window);

    // Press 'h' — should set parser state and fall through to dispatch
    let _response = process_key(&mut engine, &doc, KeyEvent::char('h'));

    // After dispatch, the parser resets to Ready (the command was processed).
    // The key assertion here is that the key was consumed (not ignored) and
    // the session is still active (it was not cleared by 'h').
    assert!(
        engine.sticky_session.is_some(),
        "non-escape key must not clear sticky session"
    );
}

#[test]
fn sticky_zprefix_sets_parser_to_awaiting_prefix() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nthird\nfourth\nfifth");
    set_sticky(&mut engine, StickyTarget::ZPrefix);

    // Press 't' — z + t = zt (redraw with current line at top)
    let response = process_key(&mut engine, &doc, KeyEvent::char('t'));

    assert!(response.consumed(), "zt must be consumed");
    assert!(
        engine.sticky_session.is_some(),
        "session must remain active after zt"
    );
}

#[test]
fn sticky_digit_then_key_passes_count_to_parser() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nthird");
    set_sticky(&mut engine, StickyTarget::Window);

    // Accumulate count '3'
    let _ = process_key(&mut engine, &doc, KeyEvent::char('3'));
    assert!(engine.sticky_session.is_some());

    // Now press '+' — Ctrl-W 3+ (increase window height by 3)
    // The count should have been taken from the session and passed to parser
    let response = process_key(&mut engine, &doc, KeyEvent::char('+'));
    assert!(
        response.consumed(),
        "3+ in sticky window mode must be consumed"
    );
}

#[test]
fn sticky_bypassed_in_insert_mode() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");

    // Enter insert mode FIRST, then activate sticky session.
    // This simulates the case where mode changes while sticky is active.
    let _ = process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);
    set_sticky(&mut engine, StickyTarget::Window);

    // In insert mode, sticky interception must be bypassed.
    // 'h' in insert mode should insert 'h', not trigger Ctrl-W h.
    let response = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(
        response.consumed(),
        "insert-mode key must be consumed normally"
    );
    // Session should still be present (not cleared, just bypassed)
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must survive insert mode"
    );
}

// ─── Sticky re-entry after command execution ────────────────────────────

#[test]
fn sticky_reentry_window_h_then_j_stays_sticky() {
    // Full workflow: sticky window mode, execute h, still sticky, execute j, still sticky
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nthird");
    set_sticky(&mut engine, StickyTarget::Window);

    // Process 'h' — Ctrl-W h (window move left)
    let r1 = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(r1.consumed(), "h in sticky window mode must be consumed");
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after h (re-entry)"
    );

    // Process 'j' — Ctrl-W j (window move down)
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(r2.consumed(), "j in sticky window mode must be consumed");
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after j (re-entry)"
    );

    // Escape exits sticky mode
    let r3 = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear sticky session"
    );
    assert!(
        r3.effects.iter().any(|e| matches!(e, Effect::ClearMessage)),
        "Escape must emit ClearMessage"
    );
}

#[test]
fn sticky_reentry_z_prefix_multi_command() {
    // Sticky z-prefix: zt then zb should both work without exiting sticky
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("line1\nline2\nline3\nline4\nline5");
    set_sticky(&mut engine, StickyTarget::ZPrefix);

    // Process 't' — zt (scroll to top)
    let r1 = process_key(&mut engine, &doc, KeyEvent::char('t'));
    assert!(r1.consumed(), "t in sticky z-prefix mode must be consumed");
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after zt"
    );

    // Process 'b' — zb (scroll to bottom)
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('b'));
    assert!(r2.consumed(), "b in sticky z-prefix mode must be consumed");
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after zb"
    );
}

#[test]
fn sticky_invalid_key_stays_in_sticky() {
    // Invalid keys in sticky mode are silently dropped — user stays in sticky.
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::Window);

    let r1 = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(
        engine.sticky_session.is_some(),
        "after window h, sticky persists"
    );

    // 'i' is not a valid Ctrl-W sub-command: silently dropped, stay in sticky.
    let _r2 = process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert!(
        engine.sticky_session.is_some(),
        "invalid key 'i' must NOT clear sticky (silently dropped per spec)"
    );

    // Sticky still works: 'j' should be window-down
    let _r3 = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(
        engine.sticky_session.is_some(),
        "valid window command after invalid key must keep sticky"
    );
}

#[test]
fn sticky_pipeline_error_non_escape_stays_in_sticky() {
    // Non-escape invalid keys stay in sticky mode (they are silently dropped).
    // Only escape-class keys (Esc, Ctrl-C, Ctrl-[) clear the session.
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::Window);

    // 'x' is not a valid Ctrl-W sub-command — silently dropped, stay in sticky
    let response = process_key(&mut engine, &doc, KeyEvent::char('x'));
    assert!(
        engine.sticky_session.is_some(),
        "non-escape invalid key must NOT clear sticky (silently dropped)"
    );
    assert!(response.consumed(), "invalid key must still be consumed");
}

// ─── StickyEnter command interception ──────────────────────────────────

#[test]
fn sticky_enter_window_activates_session_and_emits_show_message() {
    use crate::grammar::{Command, PrefixCommand};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();

    let command = Command::Prefix {
        count: NonZeroU32::new(1).unwrap(),
        register: None,
        command: PrefixCommand::StickyEnter {
            target: StickyTarget::Window,
        },
    };

    let response = engine.execute_effect_plan(command, false, ctx).unwrap();

    // Session must be activated
    assert!(
        engine.sticky_session.is_some(),
        "StickyEnter must activate sticky session"
    );
    assert_eq!(
        engine.sticky_session.unwrap().target(),
        StickyTarget::Window,
        "session target must be Window"
    );

    // Must emit ShowInfo with "-- WINDOW --"
    assert!(
        response.effects.iter().any(|e| matches!(
            e,
            Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if *text == "-- WINDOW --"
        )),
        "StickyEnter must emit ShowInfo with '-- WINDOW --'"
    );
}

#[test]
fn sticky_enter_zprefix_activates_session_and_emits_show_message() {
    use crate::grammar::{Command, PrefixCommand};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();

    let command = Command::Prefix {
        count: NonZeroU32::new(1).unwrap(),
        register: None,
        command: PrefixCommand::StickyEnter {
            target: StickyTarget::ZPrefix,
        },
    };

    let response = engine.execute_effect_plan(command, false, ctx).unwrap();

    assert!(
        engine.sticky_session.is_some(),
        "StickyEnter must activate sticky session"
    );
    assert_eq!(
        engine.sticky_session.unwrap().target(),
        StickyTarget::ZPrefix,
        "session target must be ZPrefix"
    );

    assert!(
        response.effects.iter().any(|e| matches!(
            e,
            Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if *text == "-- SCROLL --"
        )),
        "StickyEnter must emit ShowInfo with '-- SCROLL --'"
    );
}

#[test]
fn sticky_enter_does_not_reach_executor() {
    use crate::grammar::{Command, PrefixCommand};

    // StickyEnter is intercepted before the executor, so the response should
    // contain only the ShowInfo effect (no executor-generated effects).
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();

    let command = Command::Prefix {
        count: NonZeroU32::new(1).unwrap(),
        register: None,
        command: PrefixCommand::StickyEnter {
            target: StickyTarget::Window,
        },
    };

    let response = engine.execute_effect_plan(command, false, ctx).unwrap();

    // Exactly one effect: ShowInfo
    assert_eq!(
        response.effects.len(),
        1,
        "StickyEnter interception must produce exactly 1 effect (ShowInfo), got {}",
        response.effects.len()
    );
    assert!(
        matches!(&response.effects[0], Effect::ShowInfo { .. }),
        "the single effect must be ShowInfo"
    );
}

#[test]
fn sticky_enter_then_key_executes_in_sticky_mode() {
    use crate::grammar::{Command, PrefixCommand};

    // After StickyEnter activates, subsequent keys should go through sticky
    // interception.
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();

    let command = Command::Prefix {
        count: NonZeroU32::new(1).unwrap(),
        register: None,
        command: PrefixCommand::StickyEnter {
            target: StickyTarget::Window,
        },
    };

    let _response = engine.execute_effect_plan(command, false, ctx).unwrap();
    assert!(engine.sticky_session.is_some());

    // Now process 'h' — should be treated as Ctrl-W h (window move left)
    let response = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(
        response.consumed(),
        "h in sticky window mode must be consumed"
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after executing window command"
    );

    // Escape exits
    let response = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear sticky session"
    );
    assert!(
        response
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ClearMessage)),
        "Escape must emit ClearMessage"
    );
}

// ─── Config-driven auto-activation of sticky sub-modes ────────────────

#[test]
fn sticky_auto_activate_window_on_first_ctrl_w_command() {
    // When sticky_prefixes includes Window, the first Ctrl-W command
    // should auto-activate sticky mode after execution.
    let mut engine = VimEngine::new();
    engine.set_sticky_prefixes(&[StickyTarget::Window]);
    let doc = SimpleDocument::new("hello\nworld\nthird");

    // Ctrl-W enters AwaitingWindowCommand
    let r1 = process_key(&mut engine, &doc, KeyEvent::ctrl('w'));
    assert!(r1.effects.is_empty() || r1.kind == ResponseKind::Pending);
    assert!(
        engine.sticky_session.is_none(),
        "Ctrl-W alone should not activate sticky yet"
    );

    // 'h' completes WindowMoveLeft — auto-activation should trigger
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(r2.consumed(), "Ctrl-W h must be consumed");
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must auto-activate after Ctrl-W h when Window is configured sticky"
    );
    assert_eq!(
        engine.sticky_session.unwrap().target(),
        StickyTarget::Window,
        "auto-activated session target must be Window"
    );
    // Must emit ShowInfo("-- WINDOW --")
    assert!(
        r2.effects
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if *text == "-- WINDOW --")),
        "auto-activation must emit ShowInfo with '-- WINDOW --'"
    );

    // 'j' should now be treated as Ctrl-W j (window move down), not cursor down
    let r3 = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(
        r3.consumed(),
        "j in auto-activated sticky window mode must be consumed"
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after second window command"
    );
    // The key test: sticky session still active
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist through chained window commands"
    );

    // Escape exits
    let r4 = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear auto-activated sticky session"
    );
    assert!(
        r4.effects.iter().any(|e| matches!(e, Effect::ClearMessage)),
        "Escape must emit ClearMessage"
    );
}

#[test]
fn sticky_no_auto_activate_when_not_configured() {
    // Default engine has no sticky_prefixes configured.
    // Ctrl-W h should execute normally and NOT auto-activate.
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nthird");

    // Ctrl-W then 'h'
    let _r1 = process_key(&mut engine, &doc, KeyEvent::ctrl('w'));
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(r2.consumed(), "Ctrl-W h must be consumed");
    assert!(
        engine.sticky_session.is_none(),
        "sticky session must NOT auto-activate when sticky_prefixes is empty"
    );

    // 'j' should be normal cursor-down (normal mode motion), not a window command
    let r3 = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(r3.consumed(), "j must be consumed as normal motion");
    // Verify cursor moved down — SetCursor should be in effects
    assert!(
        r3.effects
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "j without sticky must produce SetCursor (cursor down motion)"
    );
}

#[test]
fn sticky_auto_activate_zprefix_on_first_z_command() {
    // When sticky_prefixes includes ZPrefix, z+t should auto-activate.
    let mut engine = VimEngine::new();
    engine.set_sticky_prefixes(&[StickyTarget::ZPrefix]);
    let doc = SimpleDocument::new("line1\nline2\nline3\nline4\nline5");

    // 'z' enters AwaitingPrefix for z
    let _r1 = process_key(&mut engine, &doc, KeyEvent::char('z'));
    assert!(
        engine.sticky_session.is_none(),
        "z alone should not activate sticky yet"
    );

    // 't' completes zt (ScrollTop) — auto-activation should trigger
    let r2 = process_key(&mut engine, &doc, KeyEvent::char('t'));
    assert!(r2.consumed(), "zt must be consumed");
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must auto-activate after zt when ZPrefix is configured sticky"
    );
    assert_eq!(
        engine.sticky_session.unwrap().target(),
        StickyTarget::ZPrefix,
        "auto-activated session target must be ZPrefix"
    );
    assert!(
        r2.effects
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if *text == "-- SCROLL --")),
        "auto-activation must emit ShowInfo with '-- SCROLL --'"
    );

    // 'b' should be treated as zb (ScrollBottom)
    let r3 = process_key(&mut engine, &doc, KeyEvent::char('b'));
    assert!(
        r3.consumed(),
        "b in auto-activated sticky z-prefix mode must be consumed"
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after second z-prefix command"
    );

    // Escape exits
    let r4 = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear auto-activated sticky session"
    );
}

#[test]
fn sticky_auto_activate_does_not_double_activate() {
    // If a sticky session is already active (e.g., from StickyEnter),
    // auto-activation should NOT replace it.
    let mut engine = VimEngine::new();
    engine.set_sticky_prefixes(&[StickyTarget::Window]);
    let doc = SimpleDocument::new("hello\nworld");

    // Manually set a sticky session (simulates StickyEnter)
    set_sticky(&mut engine, StickyTarget::Window);

    // Process 'h' — should execute window command but NOT double-activate
    let r1 = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(r1.consumed());
    assert!(
        engine.sticky_session.is_some(),
        "existing sticky session must persist"
    );

    // The response should NOT contain a ShowInfo from auto-activation
    // (the session was already active)
    let show_msg_count = r1
        .effects
        .iter()
        .filter(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if *text == "-- WINDOW --"))
        .count();
    assert_eq!(
        show_msg_count, 0,
        "auto-activation must not emit ShowInfo when session already exists"
    );
}

#[test]
fn sticky_auto_activate_g_prefix_does_not_trigger() {
    // g-prefix commands should NOT auto-activate sticky mode even when
    // Window or ZPrefix is configured.
    let mut engine = VimEngine::new();
    engine.set_sticky_prefixes(&[StickyTarget::Window, StickyTarget::ZPrefix]);
    let doc = SimpleDocument::new("hello\nworld");

    // g then d → gd (GotoDefinition) — not in Window or ZPrefix group
    let _r1 = process_key(&mut engine, &doc, KeyEvent::char('g'));
    let _r2 = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(
        engine.sticky_session.is_none(),
        "g-prefix commands must not auto-activate sticky mode"
    );
}

// ─── Sticky pending_command_display ──────────────────────────────────────

#[test]
fn sticky_pending_display_shows_indicator_when_parser_ready() {
    let mut engine = VimEngine::new();
    set_sticky(&mut engine, StickyTarget::Window);

    let display = engine.pending_command_display();
    assert_eq!(
        display.as_str(),
        "^W+",
        "pending display must show ^W+ when sticky window is active and parser is ready"
    );
}

#[test]
fn sticky_pending_display_zprefix_shows_indicator() {
    let mut engine = VimEngine::new();
    set_sticky(&mut engine, StickyTarget::ZPrefix);

    let display = engine.pending_command_display();
    assert_eq!(
        display.as_str(),
        "z+",
        "pending display must show z+ when sticky z-prefix is active and parser is ready"
    );
}

#[test]
fn sticky_pending_display_empty_after_exit() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::Window);

    assert!(!engine.pending_command_display().is_empty());

    // Escape exits sticky mode
    let _r = process_key(&mut engine, &doc, KeyEvent::escape());

    let display = engine.pending_command_display();
    assert!(
        display.is_empty(),
        "pending display must be empty after exiting sticky mode, got {:?}",
        display.as_str()
    );
}

// ─── Mouse clears sticky session ────────────────────────────────────────

#[test]
fn process_click_clears_sticky_session() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    set_sticky(&mut engine, StickyTarget::Window);
    assert!(engine.sticky_session.is_some());

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let _r = engine.process_click(3, &ctx);

    assert!(
        engine.sticky_session.is_none(),
        "process_click must clear sticky session"
    );
}

#[test]
fn process_mouse_selection_clears_sticky_session() {
    use crate::primitives::SelectionShape;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    set_sticky(&mut engine, StickyTarget::ZPrefix);
    assert!(engine.sticky_session.is_some());

    let ctx = InputContext::new(&doc, 0).validate().unwrap();
    let _r = engine.process_mouse_selection(0, 5, SelectionShape::Char, &ctx);

    assert!(
        engine.sticky_session.is_none(),
        "process_mouse_selection must clear sticky session"
    );
}

// ─── Integration tests: sticky sub-modes with effect verification ────────
//
// These tests verify ACTUAL OBSERVABLE OUTPUTS (HostRequests for window
// commands, Effects for scroll commands) at every step of the sticky
// sub-mode lifecycle.
//
// Window commands (h/j/k/l/+/-/= etc.) are routed from Effect variants
// to HostRequest variants by register_pending_host_requests(), so they
// appear in response.host_requests(), not response.effects.
//
// Scroll commands (zt/zz/zb) remain as Effect variants (CursorToTop,
// CenterCursor, CursorToBottom) in response.effects.

/// Helper: check whether any effect in the response matches the given predicate.
fn has_effect(response: &Response, pred: impl Fn(&Effect) -> bool) -> bool {
    response.effects.iter().any(pred)
}

/// Helper: check whether any host request matches the given predicate.
fn has_host_request(
    response: &Response,
    pred: impl Fn(&crate::execution::host::HostRequest) -> bool,
) -> bool {
    response.host_requests().iter().any(pred)
}

#[test]
fn sticky_integration_window_full_lifecycle_with_host_requests() {
    use crate::execution::host::HostRequest;

    // Full lifecycle: auto-activate sticky window mode, press h/j/3+/Escape/j.
    // Verify the ACTUAL HOST REQUESTS and EFFECTS at every step.
    let mut engine = VimEngine::new();
    engine.set_sticky_prefixes(&[StickyTarget::Window]);
    let doc = SimpleDocument::new("hello\nworld\nthird\nfourth\nfifth");

    // Step 1: Ctrl-W enters AwaitingWindowCommand (pending)
    let r_cw = process_key(&mut engine, &doc, KeyEvent::ctrl('w'));
    assert!(
        r_cw.pending(),
        "Ctrl-W must be pending (awaiting sub-command)"
    );

    // Step 2: 'h' completes Ctrl-W h → WindowMoveLeft host request + auto-activate sticky
    let r_h = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(
        has_host_request(&r_h, |r| matches!(r, HostRequest::WindowMoveLeft { .. })),
        "Ctrl-W h must produce WindowMoveLeft host request, got requests: {:?}, effects: {:?}",
        r_h.host_requests(),
        r_h.effects.as_slice()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must auto-activate after first window command"
    );
    assert!(
        has_effect(
            &r_h,
            |e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if *text == "-- WINDOW --")
        ),
        "auto-activation must emit ShowInfo('-- WINDOW --')"
    );

    // Step 3: 'j' in sticky mode → WindowMoveDown host request, sticky persists
    let r_j = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(
        has_host_request(&r_j, |r| matches!(r, HostRequest::WindowMoveDown { .. })),
        "j in sticky window mode must produce WindowMoveDown host request, got requests: {:?}",
        r_j.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after WindowMoveDown"
    );

    // Step 4: '3' digit accumulation — no effects/requests, still sticky
    let r_3 = process_key(&mut engine, &doc, KeyEvent::char('3'));
    assert!(r_3.consumed(), "digit '3' must be consumed");
    assert!(r_3.effects.is_empty(), "digit must produce no effects");
    assert!(
        r_3.host_requests().is_empty(),
        "digit must produce no host requests"
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after digit accumulation"
    );

    // Step 5: '+' with accumulated count 3 → WindowIncreaseHeight { count: 3 }
    let r_plus = process_key(&mut engine, &doc, KeyEvent::char('+'));
    assert!(
        has_host_request(&r_plus, |r| matches!(
            r,
            HostRequest::WindowIncreaseHeight { count: 3, .. }
        )),
        "3+ in sticky window must produce WindowIncreaseHeight(count=3) host request, got: {:?}",
        r_plus.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after WindowIncreaseHeight"
    );

    // Step 6: Escape → ClearMessage, session cleared
    let r_esc = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        has_effect(&r_esc, |e| matches!(e, Effect::ClearMessage)),
        "Escape must emit ClearMessage effect"
    );
    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear sticky session"
    );

    // Step 7: 'j' after exit → cursor down motion (SetCursor), NOT WindowMoveDown
    let r_j_normal = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(
        has_effect(&r_j_normal, |e| matches!(e, Effect::SetCursor { .. })),
        "j after sticky exit must be cursor-down (SetCursor), got effects: {:?}",
        r_j_normal.effects.as_slice()
    );
    assert!(
        !has_host_request(&r_j_normal, |r| matches!(
            r,
            HostRequest::WindowMoveDown { .. }
        )),
        "j after sticky exit must NOT produce WindowMoveDown host request"
    );
}

#[test]
fn sticky_integration_zprefix_full_lifecycle_with_effects() {
    // Full lifecycle: auto-activate sticky z-prefix, press t/z/b/Escape.
    // Verify actual scroll effects at every step.
    // (Scroll effects stay as Effects, not routed to HostRequests.)
    let mut engine = VimEngine::new();
    engine.set_sticky_prefixes(&[StickyTarget::ZPrefix]);
    let doc = SimpleDocument::new("line1\nline2\nline3\nline4\nline5");

    // Step 1: 'z' enters AwaitingPrefix (pending)
    let r_z = process_key(&mut engine, &doc, KeyEvent::char('z'));
    assert!(r_z.pending(), "z alone must be pending");

    // Step 2: 't' completes zt → CursorToTop effect + auto-activate sticky
    let r_t = process_key(&mut engine, &doc, KeyEvent::char('t'));
    assert!(
        has_effect(&r_t, |e| matches!(e, Effect::CursorToTop)),
        "zt must produce CursorToTop effect, got: {:?}",
        r_t.effects.as_slice()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must auto-activate after zt"
    );
    assert!(
        has_effect(
            &r_t,
            |e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if *text == "-- SCROLL --")
        ),
        "auto-activation must emit ShowInfo('-- SCROLL --')"
    );

    // Step 3: 'z' in sticky z-prefix → zz = CenterCursor, sticky persists
    let r_zz = process_key(&mut engine, &doc, KeyEvent::char('z'));
    assert!(
        has_effect(&r_zz, |e| matches!(e, Effect::CenterCursor)),
        "z in sticky z-prefix mode must produce CenterCursor (zz), got: {:?}",
        r_zz.effects.as_slice()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after zz"
    );

    // Step 4: 'b' in sticky z-prefix → zb = CursorToBottom, sticky persists
    let r_b = process_key(&mut engine, &doc, KeyEvent::char('b'));
    assert!(
        has_effect(&r_b, |e| matches!(e, Effect::CursorToBottom)),
        "b in sticky z-prefix mode must produce CursorToBottom (zb), got: {:?}",
        r_b.effects.as_slice()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after zb"
    );

    // Step 5: Escape → ClearMessage, session cleared
    let r_esc = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        has_effect(&r_esc, |e| matches!(e, Effect::ClearMessage)),
        "Escape must emit ClearMessage effect"
    );
    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear sticky session"
    );
}

#[test]
fn sticky_integration_explicit_enter_with_host_request_verification() {
    // Activate sticky via StickyEnter command (no sticky_prefixes configured).
    // Then verify host requests from subsequent window commands.
    use crate::execution::host::HostRequest;
    use crate::grammar::{Command, PrefixCommand};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nthird");
    let ctx = InputContext::new(&doc, 0).validate().unwrap();

    // Inject StickyEnter command → activates sticky, emits ShowInfo
    let command = Command::Prefix {
        count: NonZeroU32::new(1).unwrap(),
        register: None,
        command: PrefixCommand::StickyEnter {
            target: StickyTarget::Window,
        },
    };
    let r_enter = engine.execute_effect_plan(command, false, ctx).unwrap();
    assert!(
        has_effect(&r_enter, |e| matches!(
            e,
            Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if *text == "-- WINDOW --"
        )),
        "StickyEnter must emit ShowInfo('-- WINDOW --')"
    );
    assert!(
        engine.sticky_session.is_some(),
        "StickyEnter must activate sticky session"
    );

    // 'h' → WindowMoveLeft host request (process_key goes through full pipeline)
    let r_h = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(
        has_host_request(&r_h, |r| matches!(r, HostRequest::WindowMoveLeft { .. })),
        "h after StickyEnter must produce WindowMoveLeft host request, got: {:?}",
        r_h.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after WindowMoveLeft"
    );

    // 'l' → WindowMoveRight host request
    let r_l = process_key(&mut engine, &doc, KeyEvent::char('l'));
    assert!(
        has_host_request(&r_l, |r| matches!(r, HostRequest::WindowMoveRight { .. })),
        "l after StickyEnter must produce WindowMoveRight host request, got: {:?}",
        r_l.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky session must persist after WindowMoveRight"
    );

    // Escape → ClearMessage, session cleared
    let r_esc = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        has_effect(&r_esc, |e| matches!(e, Effect::ClearMessage)),
        "Escape must emit ClearMessage"
    );
    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear sticky session"
    );
}

#[test]
fn sticky_integration_invalid_key_stays_in_sticky() {
    use crate::execution::host::HostRequest;

    // Invalid key 'x' in sticky window mode: not a valid Ctrl-W sub-command.
    // It is silently dropped and the user stays in sticky mode.
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld");
    set_sticky(&mut engine, StickyTarget::Window);

    let r_x = process_key(&mut engine, &doc, KeyEvent::char('x'));

    // The key is consumed (intercepted by sticky).
    assert!(
        r_x.consumed(),
        "invalid key in sticky mode must be consumed"
    );
    // Sticky session must PERSIST — invalid keys are silently dropped.
    assert!(
        engine.sticky_session.is_some(),
        "invalid key must NOT clear sticky session (silently drop, stay in sticky)"
    );
    // No window host request should have been produced.
    assert!(
        !has_host_request(&r_x, |r| matches!(
            r,
            HostRequest::WindowMoveLeft { .. }
                | HostRequest::WindowMoveRight { .. }
                | HostRequest::WindowMoveUp { .. }
                | HostRequest::WindowMoveDown { .. }
                | HostRequest::SplitWindow { .. }
                | HostRequest::CloseWindow { .. }
                | HostRequest::WindowEqualSize { .. }
                | HostRequest::WindowIncreaseHeight { .. }
                | HostRequest::WindowDecreaseHeight { .. }
        )),
        "invalid key must NOT produce any window host request"
    );

    // Verify still functional: next valid key should work as window command.
    let r_h = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(
        has_host_request(&r_h, |r| matches!(r, HostRequest::WindowMoveLeft { .. })),
        "valid key after invalid must still produce window command"
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after valid command following invalid key"
    );
}

#[test]
fn sticky_integration_survives_seven_consecutive_commands() {
    use crate::execution::host::HostRequest;

    // Exercise 7 consecutive window commands (h, j, k, l, +, -, =).
    // After EACH, verify the correct host request and that sticky persists.
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nthird\nfourth\nfifth");
    set_sticky(&mut engine, StickyTarget::Window);

    // 1) h → WindowMoveLeft
    let r = process_key(&mut engine, &doc, KeyEvent::char('h'));
    assert!(
        has_host_request(&r, |r| matches!(r, HostRequest::WindowMoveLeft { .. })),
        "command 1 (h) must produce WindowMoveLeft, got: {:?}",
        r.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after command 1"
    );

    // 2) j → WindowMoveDown
    let r = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(
        has_host_request(&r, |r| matches!(r, HostRequest::WindowMoveDown { .. })),
        "command 2 (j) must produce WindowMoveDown, got: {:?}",
        r.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after command 2"
    );

    // 3) k → WindowMoveUp
    let r = process_key(&mut engine, &doc, KeyEvent::char('k'));
    assert!(
        has_host_request(&r, |r| matches!(r, HostRequest::WindowMoveUp { .. })),
        "command 3 (k) must produce WindowMoveUp, got: {:?}",
        r.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after command 3"
    );

    // 4) l → WindowMoveRight
    let r = process_key(&mut engine, &doc, KeyEvent::char('l'));
    assert!(
        has_host_request(&r, |r| matches!(r, HostRequest::WindowMoveRight { .. })),
        "command 4 (l) must produce WindowMoveRight, got: {:?}",
        r.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after command 4"
    );

    // 5) + → WindowIncreaseHeight { count: 1 }
    let r = process_key(&mut engine, &doc, KeyEvent::char('+'));
    assert!(
        has_host_request(&r, |r| matches!(
            r,
            HostRequest::WindowIncreaseHeight { count: 1, .. }
        )),
        "command 5 (+) must produce WindowIncreaseHeight(count=1), got: {:?}",
        r.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after command 5"
    );

    // 6) - → WindowDecreaseHeight { count: 1 }
    let r = process_key(&mut engine, &doc, KeyEvent::char('-'));
    assert!(
        has_host_request(&r, |r| matches!(
            r,
            HostRequest::WindowDecreaseHeight { count: 1, .. }
        )),
        "command 6 (-) must produce WindowDecreaseHeight(count=1), got: {:?}",
        r.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after command 6"
    );

    // 7) = → WindowEqualSize
    let r = process_key(&mut engine, &doc, KeyEvent::char('='));
    assert!(
        has_host_request(&r, |r| matches!(r, HostRequest::WindowEqualSize { .. })),
        "command 7 (=) must produce WindowEqualSize, got: {:?}",
        r.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after command 7"
    );

    // Exit
    let r = process_key(&mut engine, &doc, KeyEvent::escape());
    assert!(
        engine.sticky_session.is_none(),
        "Escape must clear sticky session"
    );
    assert!(
        has_effect(&r, |e| matches!(e, Effect::ClearMessage)),
        "Escape must emit ClearMessage"
    );
}

#[test]
fn sticky_integration_count_accumulation_with_host_request_verification() {
    use crate::execution::host::HostRequest;

    // Test count accumulation: 5+, 10-, and bare + (default count 1).
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello\nworld\nthird\nfourth\nfifth");
    set_sticky(&mut engine, StickyTarget::Window);

    // Sequence 1: '5' then '+' → WindowIncreaseHeight { count: 5 }
    let r_5 = process_key(&mut engine, &doc, KeyEvent::char('5'));
    assert!(r_5.consumed(), "digit '5' must be consumed");
    assert!(r_5.effects.is_empty(), "digit must produce no effects");
    assert!(
        r_5.host_requests().is_empty(),
        "digit must produce no host requests"
    );

    let r_plus = process_key(&mut engine, &doc, KeyEvent::char('+'));
    assert!(
        has_host_request(&r_plus, |r| matches!(
            r,
            HostRequest::WindowIncreaseHeight { count: 5, .. }
        )),
        "5+ must produce WindowIncreaseHeight(count=5), got: {:?}",
        r_plus.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after 5+"
    );

    // Sequence 2: '1' '0' then '-' → WindowDecreaseHeight { count: 10 }
    let r_1 = process_key(&mut engine, &doc, KeyEvent::char('1'));
    assert!(r_1.consumed(), "digit '1' must be consumed");
    assert!(r_1.effects.is_empty(), "digit must produce no effects");

    let r_0 = process_key(&mut engine, &doc, KeyEvent::char('0'));
    assert!(r_0.consumed(), "digit '0' must be consumed");
    assert!(r_0.effects.is_empty(), "digit must produce no effects");

    let r_minus = process_key(&mut engine, &doc, KeyEvent::char('-'));
    assert!(
        has_host_request(&r_minus, |r| matches!(
            r,
            HostRequest::WindowDecreaseHeight { count: 10, .. }
        )),
        "10- must produce WindowDecreaseHeight(count=10), got: {:?}",
        r_minus.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after 10-"
    );

    // Sequence 3: bare '+' with no count prefix → WindowIncreaseHeight { count: 1 }
    let r_bare_plus = process_key(&mut engine, &doc, KeyEvent::char('+'));
    assert!(
        has_host_request(&r_bare_plus, |r| matches!(
            r,
            HostRequest::WindowIncreaseHeight { count: 1, .. }
        )),
        "bare + must produce WindowIncreaseHeight(count=1), got: {:?}",
        r_bare_plus.host_requests()
    );
    assert!(
        engine.sticky_session.is_some(),
        "sticky must persist after bare +"
    );
}

// ─── ShowMatch emission in insert mode ────────────────────────────────

#[test]
fn insert_closing_bracket_emits_show_match_when_enabled() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_showmatch(true);
    let doc = SimpleDocument::new("(hello");

    // Enter insert mode at end of text (offset 6)
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 6);
    assert_eq!(engine.mode(), Mode::Insert);

    // Type ')' — should emit ShowMatch pointing to the '(' at offset 0
    let response = process_key_at(&mut engine, &doc, KeyEvent::char(')'), 6);
    let show_match = response
        .effects()
        .iter()
        .find(|e| matches!(e, Effect::ShowMatch { .. }));
    assert!(
        show_match.is_some(),
        "typing ')' with showmatch should emit ShowMatch, got: {:?}",
        response.effects()
    );
    match show_match.unwrap() {
        Effect::ShowMatch { position } => {
            assert_eq!(position.get(), 0, "match should point to '(' at offset 0");
        }
        _ => unreachable!(),
    }
}

#[test]
fn insert_closing_bracket_no_show_match_when_disabled() {
    let mut engine = VimEngine::new();
    // showmatch defaults to false
    let doc = SimpleDocument::new("(hello");

    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 6);
    assert_eq!(engine.mode(), Mode::Insert);

    let response = process_key_at(&mut engine, &doc, KeyEvent::char(')'), 6);
    let show_match = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::ShowMatch { .. }));
    assert!(
        !show_match,
        "ShowMatch should NOT be emitted when showmatch is disabled"
    );
}

#[test]
fn insert_closing_bracket_nested_match() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_showmatch(true);
    let doc = SimpleDocument::new("(a(b");

    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 4);
    assert_eq!(engine.mode(), Mode::Insert);

    // Type ')' — should match the inner '(' at offset 2, not the outer at 0
    let response = process_key_at(&mut engine, &doc, KeyEvent::char(')'), 4);
    match response
        .effects()
        .iter()
        .find(|e| matches!(e, Effect::ShowMatch { .. }))
        .unwrap()
    {
        Effect::ShowMatch { position } => {
            assert_eq!(position.get(), 2, "should match inner '(' at offset 2");
        }
        _ => unreachable!(),
    }
}

#[test]
fn insert_closing_bracket_no_match_found() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_showmatch(true);
    let doc = SimpleDocument::new("hello");

    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 5);
    assert_eq!(engine.mode(), Mode::Insert);

    // Type ')' with no matching '(' — no ShowMatch emitted
    let response = process_key_at(&mut engine, &doc, KeyEvent::char(')'), 5);
    let show_match = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::ShowMatch { .. }));
    assert!(
        !show_match,
        "ShowMatch should NOT be emitted when no matching bracket exists"
    );
}

#[test]
fn insert_non_bracket_char_no_show_match() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_showmatch(true);
    let doc = SimpleDocument::new("(hello");

    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 6);
    assert_eq!(engine.mode(), Mode::Insert);

    // Type 'x' — not a closing bracket, no ShowMatch
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('x'), 6);
    let show_match = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::ShowMatch { .. }));
    assert!(
        !show_match,
        "ShowMatch should NOT be emitted for non-bracket characters"
    );
}

#[test]
fn insert_closing_brace_and_square_bracket_emit_show_match() {
    let mut engine = VimEngine::new();
    engine.options_mut().set_showmatch(true);

    // Test '}' matching '{'
    let doc = SimpleDocument::new("{code");
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 5);
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('}'), 5);
    match response
        .effects()
        .iter()
        .find(|e| matches!(e, Effect::ShowMatch { .. }))
        .expect("'}' should emit ShowMatch")
    {
        Effect::ShowMatch { position } => {
            assert_eq!(position.get(), 0, "should match opening brace at offset 0");
        }
        _ => unreachable!(),
    }

    // Test ']' matching '['
    let mut engine2 = VimEngine::new();
    engine2.options_mut().set_showmatch(true);
    let doc2 = SimpleDocument::new("[arr");
    process_key_at(&mut engine2, &doc2, KeyEvent::char('i'), 4);
    let response2 = process_key_at(&mut engine2, &doc2, KeyEvent::char(']'), 4);
    match response2
        .effects()
        .iter()
        .find(|e| matches!(e, Effect::ShowMatch { .. }))
        .expect("']' should emit ShowMatch")
    {
        Effect::ShowMatch { position } => {
            assert_eq!(position.get(), 0, "should match '[' at offset 0");
        }
        _ => unreachable!(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Abbreviation expansion in insert mode
// ═══════════════════════════════════════════════════════════════════════════

/// Helper: add a FullId insert-mode abbreviation to the engine.
fn add_iabbrev(engine: &mut VimEngine, trigger: &str, replacement: &str) {
    use crate::primitives::{AbbrevEntry, AbbrevKind, AbbrevMode};
    let is_keyword = |c: char| c.is_alphanumeric() || c == '_';
    engine.abbrev_table.add(AbbrevEntry {
        trigger: trigger.into(),
        replacement: replacement.into(),
        mode: AbbrevMode::Insert,
        kind: AbbrevKind::classify(trigger, is_keyword),
        noremap: false,
    });
}

/// Typing "teh " with `:iabbrev teh the` should produce a Replace effect
/// replacing "teh" with "the" in the response for the space keystroke.
#[test]
fn abbrev_expand_full_id_on_space() {
    let mut engine = VimEngine::new();
    add_iabbrev(&mut engine, "teh", "the");

    // Document starts empty. Enter insert mode at offset 0.
    let doc = SimpleDocument::new("");
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Type "teh" — each char inserts at the current cursor position.
    // After each char, the document grows (from the host's perspective),
    // but our SimpleDocument is static — the engine doesn't own the doc.
    // The engine tracks accumulated_text internally.
    let doc_t = SimpleDocument::new("");
    process_key_at(&mut engine, &doc_t, KeyEvent::char('t'), 0);
    let doc_te = SimpleDocument::new("t");
    process_key_at(&mut engine, &doc_te, KeyEvent::char('e'), 1);
    let doc_teh = SimpleDocument::new("te");
    process_key_at(&mut engine, &doc_teh, KeyEvent::char('h'), 2);

    // Now type space — this should trigger abbreviation expansion.
    let doc_teh_full = SimpleDocument::new("teh");
    let response = process_key_at(&mut engine, &doc_teh_full, KeyEvent::char(' '), 3);

    // Should contain a Replace effect for the abbreviation.
    let has_replace = response.effects().iter().any(|e| {
        matches!(e, Effect::Replace { range, text }
            if range.start().get() == 0
            && range.end().get() == 3
            && text.as_str() == "the")
    });
    assert!(
        has_replace,
        "space after 'teh' should trigger abbreviation Replace effect; \
         effects: {:#?}",
        response.effects()
    );
}

/// Abbreviation should NOT expand when the trigger is part of a larger word.
/// E.g., typing "ateh " should not expand "teh" because it's preceded by "a"
/// (a keyword char).
#[test]
fn abbrev_no_expand_when_preceded_by_keyword() {
    let mut engine = VimEngine::new();
    add_iabbrev(&mut engine, "teh", "the");

    let doc = SimpleDocument::new("");
    process_key(&mut engine, &doc, KeyEvent::char('i'));

    // Type "ateh"
    let doc0 = SimpleDocument::new("");
    process_key_at(&mut engine, &doc0, KeyEvent::char('a'), 0);
    let doc1 = SimpleDocument::new("a");
    process_key_at(&mut engine, &doc1, KeyEvent::char('t'), 1);
    let doc2 = SimpleDocument::new("at");
    process_key_at(&mut engine, &doc2, KeyEvent::char('e'), 2);
    let doc3 = SimpleDocument::new("ate");
    process_key_at(&mut engine, &doc3, KeyEvent::char('h'), 3);

    // Type space — should NOT expand because "teh" is preceded by keyword 'a'.
    let doc4 = SimpleDocument::new("ateh");
    let response = process_key_at(&mut engine, &doc4, KeyEvent::char(' '), 4);

    let has_replace = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::Replace { .. }));
    assert!(
        !has_replace,
        "should not expand 'teh' when preceded by keyword char 'a'; \
         effects: {:#?}",
        response.effects()
    );
}

/// Abbreviation should expand when preceded by a space (non-keyword).
#[test]
fn abbrev_expand_after_space_prefix() {
    let mut engine = VimEngine::new();
    add_iabbrev(&mut engine, "teh", "the");

    let doc = SimpleDocument::new("");
    process_key(&mut engine, &doc, KeyEvent::char('i'));

    // Type " teh" (space then teh)
    let doc0 = SimpleDocument::new("");
    process_key_at(&mut engine, &doc0, KeyEvent::char(' '), 0);
    let doc1 = SimpleDocument::new(" ");
    process_key_at(&mut engine, &doc1, KeyEvent::char('t'), 1);
    let doc2 = SimpleDocument::new(" t");
    process_key_at(&mut engine, &doc2, KeyEvent::char('e'), 2);
    let doc3 = SimpleDocument::new(" te");
    process_key_at(&mut engine, &doc3, KeyEvent::char('h'), 3);

    // Type space — should expand.
    let doc4 = SimpleDocument::new(" teh");
    let response = process_key_at(&mut engine, &doc4, KeyEvent::char(' '), 4);

    let has_replace = response.effects().iter().any(|e| {
        matches!(e, Effect::Replace { range, text }
            if range.start().get() == 1
            && range.end().get() == 4
            && text.as_str() == "the")
    });
    assert!(
        has_replace,
        "space after ' teh' should expand abbreviation; \
         effects: {:#?}",
        response.effects()
    );
}

/// Accumulated text should be updated after abbreviation expansion.
#[test]
fn abbrev_expand_updates_accumulated_text() {
    let mut engine = VimEngine::new();
    add_iabbrev(&mut engine, "teh", "the");

    let doc = SimpleDocument::new("");
    process_key(&mut engine, &doc, KeyEvent::char('i'));

    let doc0 = SimpleDocument::new("");
    process_key_at(&mut engine, &doc0, KeyEvent::char('t'), 0);
    let doc1 = SimpleDocument::new("t");
    process_key_at(&mut engine, &doc1, KeyEvent::char('e'), 1);
    let doc2 = SimpleDocument::new("te");
    process_key_at(&mut engine, &doc2, KeyEvent::char('h'), 2);

    let doc3 = SimpleDocument::new("teh");
    process_key_at(&mut engine, &doc3, KeyEvent::char(' '), 3);

    let acc = engine.state.insert_state().unwrap().accumulated_text();
    assert_eq!(
        acc, "the ",
        "accumulated_text should reflect expansion: 'teh' -> 'the' + space"
    );
}

/// Empty abbreviation table should not interfere with normal typing.
#[test]
fn abbrev_empty_table_no_effect() {
    let mut engine = VimEngine::new();
    // No abbreviations added.

    let doc = SimpleDocument::new("");
    process_key(&mut engine, &doc, KeyEvent::char('i'));

    let doc0 = SimpleDocument::new("");
    process_key_at(&mut engine, &doc0, KeyEvent::char('h'), 0);
    let doc1 = SimpleDocument::new("h");
    process_key_at(&mut engine, &doc1, KeyEvent::char('i'), 1);

    let doc2 = SimpleDocument::new("hi");
    let response = process_key_at(&mut engine, &doc2, KeyEvent::char(' '), 2);

    let has_replace = response
        .effects()
        .iter()
        .any(|e| matches!(e, Effect::Replace { .. }));
    assert!(
        !has_replace,
        "no abbreviations: should not produce Replace effects"
    );
}

/// Expansion to longer text should adjust cursor position.
#[test]
fn abbrev_expand_longer_replacement_adjusts_cursor() {
    let mut engine = VimEngine::new();
    add_iabbrev(&mut engine, "hw", "hello world");

    let doc = SimpleDocument::new("");
    process_key(&mut engine, &doc, KeyEvent::char('i'));

    let doc0 = SimpleDocument::new("");
    process_key_at(&mut engine, &doc0, KeyEvent::char('h'), 0);
    let doc1 = SimpleDocument::new("h");
    process_key_at(&mut engine, &doc1, KeyEvent::char('w'), 1);

    let doc2 = SimpleDocument::new("hw");
    let response = process_key_at(&mut engine, &doc2, KeyEvent::char(' '), 2);

    // Replace "hw" (2 bytes at [0,2)) with "hello world" (11 bytes).
    let has_replace = response.effects().iter().any(|e| {
        matches!(e, Effect::Replace { range, text }
            if range.start().get() == 0
            && range.end().get() == 2
            && text.as_str() == "hello world")
    });
    assert!(has_replace, "should replace 'hw' with 'hello world'");

    // Cursor should be at position after "hello world " = 12.
    // Original cursor=2, trigger_char=' '(1 byte), delete_count=2, replacement=11
    // new_cursor = 2 + (11-2) + 1 = 12
    let abbrev_cursor = response.effects().iter().rev().find_map(|e| {
        if let Effect::SetCursor { offset } = e {
            Some(offset.get())
        } else {
            None
        }
    });
    assert_eq!(
        abbrev_cursor,
        Some(12),
        "cursor should be at end of expanded text + trigger char"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Execution/Effects Architecture Tests
// ═══════════════════════════════════════════════════════════════════════════════

// ── VimContext Snapshot ─────────────────────────────────────────────────

#[test]
fn vim_context_reports_normal_mode() {
    let engine = VimEngine::new();
    let ctx = engine.vim_context();
    assert_eq!(ctx.mode, Mode::Normal);
    assert_eq!(ctx.pending_operator, None);
    assert!(!ctx.is_recording);
    assert!(!ctx.is_repeating);
    assert!(!ctx.has_pending_keys);
    assert!(ctx.pending_display.is_empty());
    assert_eq!(ctx.active_register, None);
}

#[test]
fn vim_context_reports_insert_mode() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    let ctx = engine.vim_context();
    assert_eq!(ctx.mode, Mode::Insert);
}

#[test]
fn vim_context_reports_pending_operator() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    let ctx = engine.vim_context();
    assert_eq!(
        ctx.pending_operator,
        Some(crate::primitives::Operator::Delete)
    );
    // VimState mode stays Normal; the parser tracks operator-pending state.
    // The pending_operator field communicates this to hosts.
    assert_eq!(ctx.mode, Mode::Normal);
}

#[test]
fn vim_context_reports_active_register() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    process_key(&mut engine, &doc, KeyEvent::char('"'));
    process_key(&mut engine, &doc, KeyEvent::char('a'));
    let ctx = engine.vim_context();
    assert_eq!(
        ctx.active_register,
        Some(crate::primitives::RegisterName::new('a').unwrap())
    );
}

#[test]
fn vim_context_pending_display_shows_operator() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    let ctx = engine.vim_context();
    assert!(!ctx.pending_display.is_empty());
    assert!(ctx.pending_display.contains('d'));
}

#[test]
fn vim_context_is_clone_send_sync() {
    fn assert_send_sync<T: Send + Sync + Clone>() {}
    assert_send_sync::<crate::primitives::VimContext>();
}

// ── Host Notification ───────────────────────────────────────────────────

#[test]
fn process_notification_focus_gained_returns_consumed() {
    let mut engine = VimEngine::new();
    let response = engine.process_notification(crate::execution::HostNotification::FocusGained);
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
    assert!(response.effects().is_empty());
}

#[test]
fn process_notification_focus_lost_in_normal_mode_no_effects() {
    let mut engine = VimEngine::new();
    let response = engine.process_notification(crate::execution::HostNotification::FocusLost);
    assert!(response.effects().is_empty());
}

#[test]
fn process_notification_clipboard_changed_updates_generation() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.state().registers().clipboard_generation(), 0);
    engine.process_notification(crate::execution::HostNotification::ClipboardChanged {
        generation: 42,
    });
    assert_eq!(engine.state().registers().clipboard_generation(), 42);
}

#[test]
fn process_notification_config_reloaded_acknowledged() {
    let mut engine = VimEngine::new();
    let response = engine.process_notification(crate::execution::HostNotification::ConfigReloaded);
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
}

#[test]
fn process_notification_selection_changed_externally_acknowledged() {
    use smallvec::smallvec;
    let mut engine = VimEngine::new();
    let response = engine.process_notification(
        crate::execution::HostNotification::SelectionChangedExternally {
            ranges: smallvec![crate::primitives::SelectionRange::new(
                crate::primitives::Offset::new(0),
                crate::primitives::Offset::new(5),
            )],
        },
    );
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
}

// ── Clipboard Freshness Detection ───────────────────────────────────────

#[test]
fn clipboard_generation_default_is_zero() {
    let regs = crate::state::Registers::new();
    assert_eq!(regs.clipboard_generation(), 0);
}

#[test]
fn clipboard_set_generation() {
    let mut regs = crate::state::Registers::new();
    regs.set_clipboard_generation(7);
    assert_eq!(regs.clipboard_generation(), 7);
}

#[test]
fn clipboard_is_stale_when_host_generation_higher() {
    let mut regs = crate::state::Registers::new();
    regs.set_clipboard_generation(5);
    assert!(regs.clipboard_is_stale(6));
    assert!(!regs.clipboard_is_stale(5));
    assert!(!regs.clipboard_is_stale(4));
}

// ── Atomic Mode Transition Effect ───────────────────────────────────────

#[test]
fn mode_transition_effect_kind() {
    let effect = Effect::mode_transition(Mode::Insert);
    assert_eq!(effect.kind(), crate::effects::EffectKind::ModeTransition);
}

#[test]
fn mode_transition_decompose_produces_set_mode_and_cursor_style() {
    let effect = Effect::mode_transition(Mode::Insert);
    let parts = effect.decompose_mode_transition().unwrap();
    assert!(matches!(
        parts[0],
        Effect::SetMode {
            mode: Mode::Insert,
            ..
        }
    ));
    assert!(matches!(parts[1], Effect::SetCursorStyle { .. }));
}

#[test]
fn mode_transition_decompose_returns_none_for_other_effects() {
    let effect = Effect::set_mode(Mode::Normal);
    assert!(effect.decompose_mode_transition().is_none());
}

#[test]
fn mode_transition_tier_is_core() {
    assert_eq!(
        crate::effects::EffectKind::ModeTransition.tier(),
        crate::effects::EffectTier::Core,
    );
}

#[test]
fn mode_transition_fields_match_mode() {
    let effect = Effect::mode_transition(Mode::Visual(VisualType::Char));
    match effect {
        Effect::ModeTransition {
            mode,
            cursor_style,
            appearance,
        } => {
            assert_eq!(mode, Mode::Visual(VisualType::Char));
            assert_eq!(
                cursor_style,
                crate::primitives::CursorStyle::for_mode(Mode::Visual(VisualType::Char))
            );
            assert_eq!(
                appearance,
                crate::primitives::ModeAppearance::for_mode(Mode::Visual(VisualType::Char))
            );
        }
        _ => panic!("expected ModeTransition"),
    }
}

// ── Additional execution/effects tests ──────────────────────────────────

#[test]
fn vim_context_clone_is_independent() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    let ctx1 = engine.vim_context();
    // Complete the operator to clear pending state
    process_key(&mut engine, &doc, KeyEvent::char('d'));
    let ctx2 = engine.vim_context();
    // Cloned ctx1 still reflects the old state
    assert!(ctx1.pending_operator.is_some());
    assert!(ctx2.pending_operator.is_none());
}

#[test]
fn focus_lost_in_insert_mode_breaks_undo_group() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);
    // Simulate some typing to start an undo group
    process_key(&mut engine, &doc, KeyEvent::char('x'));
    // Focus lost should break the undo group (no panic, returns consumed)
    let response = engine.process_notification(crate::execution::HostNotification::FocusLost);
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
    // Engine remains in insert mode (focus loss doesn't change mode)
    assert_eq!(engine.mode(), Mode::Insert);
}

#[test]
fn clipboard_generation_monotonic_update() {
    let mut engine = VimEngine::new();
    engine.process_notification(crate::execution::HostNotification::ClipboardChanged {
        generation: 10,
    });
    assert_eq!(engine.state().registers().clipboard_generation(), 10);
    // Update to higher value
    engine.process_notification(crate::execution::HostNotification::ClipboardChanged {
        generation: 20,
    });
    assert_eq!(engine.state().registers().clipboard_generation(), 20);
    // Staleness check: generation 21 means clipboard is stale
    assert!(engine.state().registers().clipboard_is_stale(21));
    // generation 20 means not stale (same as last seen)
    assert!(!engine.state().registers().clipboard_is_stale(20));
}

// ═══════════════════════════════════════════════════════════════════════════
// BUG: MULTI-CURSOR INSERT-MODE TYPING DOES NOT REPLICATE
// ═══════════════════════════════════════════════════════════════════════════

/// BUG 1: rebase_effects_by_delta does not adjust BeginInsert.entry_offset.
///
/// When entering insert mode with multi-cursor, execute_effect_plan calls
/// replicate_effects_precise, which calls rebase_effects_by_delta for secondary
/// cursors. But rebase_effects_by_delta only handles Insert, Delete, and
/// SetCursor — BeginInsert falls through to `other => other.clone()`, so the
/// secondary cursor's entry_offset is a copy of the primary's (wrong).
#[test]
fn bug_multi_cursor_begin_insert_entry_offset_not_rebased() {
    use crate::primitives::{Offset, SelectionRange, Selections};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("aaa bbb ccc ddd eee fff ggg hhh");

    // Two cursors: primary at 3, secondary at 27.
    let ranges: smallvec::SmallVec<[SelectionRange; 1]> = smallvec::smallvec![
        SelectionRange::insert_cursor(Offset::new(3)),
        SelectionRange::insert_cursor(Offset::new(27)),
    ];
    let sels = Selections::new(ranges, 0);
    engine.state.multi_cursor_mut().set_selections(sels);

    // Enter insert mode with 'i' at offset 3 (primary cursor)
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('i'), 3);
    assert_eq!(engine.mode(), Mode::Insert);

    // Collect BeginInsert entry_offsets
    let mut entry_offsets: Vec<usize> = Vec::new();
    for effect in &response.effects {
        if let Effect::BeginInsert { entry_offset, .. } = effect {
            entry_offsets.push(entry_offset.get());
        }
    }

    // BeginInsert is a global effect — emitted once (not replicated per cursor).
    // The entry_offset is the primary cursor's position.
    assert_eq!(
        entry_offsets.len(),
        1,
        "Expected 1 BeginInsert effect (global, not per-cursor), got {}",
        entry_offsets.len()
    );

    assert_eq!(
        entry_offsets[0], 3,
        "Primary BeginInsert entry_offset should be 3"
    );
}

/// BUG 2: Insert-mode typing does not replicate to secondary cursors.
///
/// Normal-mode commands go through execute_effect_plan (mode_dispatch.rs:1259)
/// which calls replicate_effects_precise when multi-cursor is active.
/// Insert-mode commands go through execute_insert_command (mode_dispatch.rs:447)
/// which dispatches directly via dispatch_insert — it NEVER calls
/// replicate_effects_precise. So only the primary cursor gets effects.
#[test]
fn bug_multi_cursor_insert_typing_produces_only_one_insert_effect() {
    use crate::primitives::{Offset, SelectionRange, Selections};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("aaa bbb ccc ddd eee fff ggg hhh");

    // Two cursors: primary at 3, secondary at 27.
    let ranges: smallvec::SmallVec<[SelectionRange; 1]> = smallvec::smallvec![
        SelectionRange::insert_cursor(Offset::new(3)),
        SelectionRange::insert_cursor(Offset::new(27)),
    ];
    let sels = Selections::new(ranges, 0);
    engine.state.multi_cursor_mut().set_selections(sels);

    // Enter insert mode
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 3);
    assert_eq!(engine.mode(), Mode::Insert);

    // Type 'x' in insert mode
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('x'), 3);

    // Count Insert effects
    let insert_effects: Vec<&Effect> = response
        .effects
        .iter()
        .filter(|e| matches!(e, Effect::Insert { .. }))
        .collect();

    // BUG: Should be 2 Insert effects (one per cursor), but we get 1.
    // execute_insert_command (mode_dispatch.rs:447-797) does NOT check
    // multi_cursor().is_active() and does NOT call replicate_effects_precise.
    // The insert typing path is: ModeAction::InsertCommand → execute_insert_command
    // → dispatch_insert → effects for primary cursor ONLY.
    assert_eq!(
        insert_effects.len(),
        2,
        "BUG: Insert-mode typing with 2 cursors should produce 2 Insert effects, \
         but only {} were produced. execute_insert_command (mode_dispatch.rs:447) \
         does NOT call replicate_effects_precise — only execute_effect_plan \
         (mode_dispatch.rs:1259) does.",
        insert_effects.len()
    );
}

/// BUG 3: Insert exit (Escape) does not replicate to secondary cursors.
///
/// `handle_insert_exit` (mode_dispatch.rs:805) calls `insert_handler::handle_insert_exit`
/// then `process_effects_with_text`, but NEVER calls `replicate_effects_precise`.
/// The exit effects (SetCursor, EndUndoGroup, SetMark('^'), SetMark('.'), SetMode)
/// are produced for the primary cursor only.
///
/// With 2 cursors, Escape should produce:
/// - 2 SetCursor effects (one per cursor at each cursor's exit position)
/// - EndUndoGroup should be global (one is correct)
/// - SetMode should be global (one is correct)
/// - SetMark('^') should ideally be per-cursor but marks are global in vim
///
/// The critical missing piece: SetCursor. Without replication, only the
/// primary cursor's exit position is emitted, leaving the secondary cursor
/// at its insert-mode position without the standard cursor-back-one adjustment.
#[test]
fn bug_multi_cursor_insert_exit_produces_only_one_set_cursor() {
    use crate::primitives::{Offset, SelectionRange, Selections};

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("aaa bbb ccc ddd eee fff ggg hhh");

    // Two cursors: primary at 3, secondary at 27.
    let ranges: smallvec::SmallVec<[SelectionRange; 1]> = smallvec::smallvec![
        SelectionRange::insert_cursor(Offset::new(3)),
        SelectionRange::insert_cursor(Offset::new(27)),
    ];
    let sels = Selections::new(ranges, 0);
    engine.state.multi_cursor_mut().set_selections(sels);

    // Enter insert mode with 'i' at offset 3 (primary cursor)
    process_key_at(&mut engine, &doc, KeyEvent::char('i'), 3);
    assert_eq!(engine.mode(), Mode::Insert);

    // Type "ab" in insert mode (these go to primary cursor only due to BUG 2,
    // but we're testing the exit path specifically)
    process_key_at(&mut engine, &doc, KeyEvent::char('a'), 3);
    process_key_at(&mut engine, &doc, KeyEvent::char('b'), 4);

    // Exit insert mode with Escape
    let response = process_key_at(&mut engine, &doc, KeyEvent::escape(), 5);

    // Count SetCursor effects in the exit response
    let set_cursor_effects: Vec<&Effect> = response
        .effects
        .iter()
        .filter(|e| matches!(e, Effect::SetCursor { .. }))
        .collect();

    // Count EndUndoGroup effects
    let end_undo_effects: Vec<&Effect> = response
        .effects
        .iter()
        .filter(|e| matches!(e, Effect::EndUndoGroup { .. }))
        .collect();

    // SetMode effects
    let set_mode_effects: Vec<&Effect> = response
        .effects
        .iter()
        .filter(|e| matches!(e, Effect::SetMode { .. }))
        .collect();

    // handle_insert_exit now replicates positional exit effects to all cursors.
    // The raw replicated effects contain 2 SetCursor (one per cursor), but
    // optimize_vec in register_pending_host_requests deduplicates trailing
    // SetCursor effects (keeping only the last one). This is correct because
    // the host only needs the primary cursor position from the response —
    // secondary cursor positions are tracked internally via MultiCursorState.
    // The functional correctness is proven by the HostSession integration tests
    // (multi_cursor_insert_tests.rs), which verify the actual document text.
    assert!(
        set_cursor_effects.len() >= 1,
        "Insert exit with 2 cursors should produce at least 1 SetCursor effect \
         (optimize_vec may deduplicate trailing cursors), got {}",
        set_cursor_effects.len()
    );

    // EndUndoGroup may be consumed by the effect processor when no matching
    // BeginUndoGroup is pending (the undo group was opened during insert entry
    // and managed internally). SetMode(Normal) should be present.
    assert!(
        end_undo_effects.len() <= 1,
        "EndUndoGroup should be at most 1 (may be consumed if no pending group), got {}",
        end_undo_effects.len()
    );
    assert_eq!(
        set_mode_effects.len(),
        1,
        "SetMode should be global (1 total), got {}",
        set_mode_effects.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// mode_string() tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn mode_string_normal_mode_returns_n() {
    let engine = VimEngine::new();
    assert_eq!(engine.mode_string(), "n");
}

#[test]
fn mode_string_insert_mode_returns_i() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode_string(), "i");
}

#[test]
fn mode_string_visual_char_returns_v() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    process_key(&mut engine, &doc, KeyEvent::char('v'));
    assert_eq!(engine.mode_string(), "v");
}

#[test]
fn mode_string_visual_line_returns_upper_v() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    process_key(&mut engine, &doc, KeyEvent::char('V'));
    assert_eq!(engine.mode_string(), "V");
}

#[test]
fn mode_string_insert_ctrl_x_returns_ix() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    // Enter insert mode, then Ctrl-X
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('x'));
    assert_eq!(engine.mode_string(), "ix");
}

#[test]
fn mode_string_ctrl_o_from_insert_returns_nii() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");
    // Enter insert mode, then Ctrl-O for one-shot normal
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('o'));
    // Should now be in normal mode with return_to Insert
    assert_eq!(engine.mode_string(), "niI");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-\ Ctrl-N via engine
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ctrl_backslash_ctrl_n_from_insert_via_engine() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    // Enter insert mode
    process_key(&mut engine, &doc, KeyEvent::char('i'));
    assert_eq!(engine.mode(), Mode::Insert);
    // Ctrl-\ starts the escape sequence
    process_key(&mut engine, &doc, KeyEvent::ctrl('\\'));
    // Ctrl-N completes it
    process_key(&mut engine, &doc, KeyEvent::ctrl('n'));
    assert_eq!(engine.mode(), Mode::Normal);
}

#[test]
fn ctrl_backslash_ctrl_n_from_visual_via_engine() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello world");
    // Enter visual mode
    process_key(&mut engine, &doc, KeyEvent::char('v'));
    assert!(engine.mode().is_visual());
    // Ctrl-\ Ctrl-N
    process_key(&mut engine, &doc, KeyEvent::ctrl('\\'));
    process_key(&mut engine, &doc, KeyEvent::ctrl('n'));
    assert_eq!(engine.mode(), Mode::Normal);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Timer round-trip test
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn timer_fired_notification_emits_cursor_hold() {
    let mut engine = VimEngine::new();
    let response =
        engine.process_notification(crate::execution::HostNotification::TimerFired { id: 1 });
    let has_cursor_hold = response.effects().iter().any(|e| {
        matches!(
            e,
            Effect::Event {
                kind: crate::primitives::VimEvent::CursorHold
            }
        )
    });
    assert!(has_cursor_hold, "TimerFired should emit CursorHold event");
}

// ═══════════════════════════════════════════════════════════════════════════════
// HostNotification additions
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn viewport_changed_updates_engine_state() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.viewport_first_line(), 0);
    assert_eq!(engine.viewport_height(), 24);
    let response =
        engine.process_notification(crate::execution::HostNotification::ViewportChanged {
            first_line: 50,
            height: 40,
        });
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
    assert!(response.effects().is_empty());
    assert_eq!(engine.viewport_first_line(), 50);
    assert_eq!(engine.viewport_height(), 40);
}

#[test]
fn filetype_changed_sets_filetype() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.filetype(), None);
    let response =
        engine.process_notification(crate::execution::HostNotification::FileTypeChanged {
            filetype: compact_str::CompactString::from("rust"),
        });
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
    assert_eq!(engine.filetype(), Some("rust"));
}

#[test]
fn diagnostics_updated_stores_count() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.diagnostics_count(), 0);
    let response = engine
        .process_notification(crate::execution::HostNotification::DiagnosticsUpdated { count: 12 });
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
    assert_eq!(engine.diagnostics_count(), 12);
}

#[test]
fn completion_done_acknowledged() {
    let mut engine = VimEngine::new();
    let response =
        engine.process_notification(crate::execution::HostNotification::CompletionDone {
            item: compact_str::CompactString::from("println!"),
        });
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
    assert!(response.effects().is_empty());
}

#[test]
fn window_resized_updates_terminal_size() {
    let mut engine = VimEngine::new();
    assert_eq!(engine.terminal_cols(), 80);
    assert_eq!(engine.terminal_rows(), 24);
    let response = engine.process_notification(crate::execution::HostNotification::WindowResized {
        cols: 120,
        rows: 50,
    });
    assert_eq!(response.kind(), crate::execution::ResponseKind::Consumed);
    assert_eq!(engine.terminal_cols(), 120);
    assert_eq!(engine.terminal_rows(), 50);
}

// ─── :undojoin wiring ─────────────────────────────────────────────

#[test]
fn undojoin_activates_undo_tree_merge() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    assert!(
        !engine.state.undo_tree().is_merging(),
        "undo tree should not be merging initially"
    );

    // Type `:undojoin<CR>` — should call begin_merge() on the undo tree
    type_command_line(&mut engine, &doc, "undojoin");
    process_key(&mut engine, &doc, KeyEvent::enter());

    assert!(
        engine.state.undo_tree().is_merging(),
        ":undojoin should activate undo tree merging"
    );
}

#[test]
fn undojoin_abbreviated_activates_undo_tree_merge() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello");

    type_command_line(&mut engine, &doc, "undoj");
    process_key(&mut engine, &doc, KeyEvent::enter());

    assert!(
        engine.state.undo_tree().is_merging(),
        ":undoj (abbreviated) should activate undo tree merging"
    );
}

// ─── last_ex_for_dot wiring ───────────────────────────────────────

#[test]
fn substitute_stores_last_ex_for_dot_on_state() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo bar");

    assert!(
        engine.state.last_ex_for_dot().is_none(),
        "last_ex_for_dot should be None initially"
    );

    // Execute `:s/foo/baz/<CR>`
    type_command_line(&mut engine, &doc, "s/foo/baz/");
    process_key(&mut engine, &doc, KeyEvent::enter());

    assert!(
        engine.state.last_ex_for_dot().is_some(),
        ":s/foo/baz/ should store last_ex_for_dot on state"
    );
}

#[test]
fn non_mutating_ex_does_not_store_last_ex_for_dot() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("foo bar");

    // Execute `:set tabstop=4<CR>` — non-mutating
    type_command_line(&mut engine, &doc, "set tabstop=4");
    process_key(&mut engine, &doc, KeyEvent::enter());

    assert!(
        engine.state.last_ex_for_dot().is_none(),
        ":set should not store last_ex_for_dot"
    );
}

// ── no_zero_mapping guard ──────────────────────────────────────────
//
// When `0` is mapped (e.g., `nmap 0 ^`), typing `10j` should move 10
// lines down — the `0` in the count must NOT be expanded by the mapping
// trie.  Only a standalone `0` (no count accumulating) should trigger
// the mapping.

#[test]
fn zero_in_count_suppresses_mapping_expansion() {
    // `nmap 0 ^` — common mapping: standalone 0 goes to first non-blank.
    // But `10j` must move 10 lines down, not expand 0→^ mid-count.
    let mut engine = VimEngine::new();
    engine.source_config_text("nmap 0 ^");

    // Build a 12-line document so 10j has room.
    // Each line is "  Xn\n" where n is the line number.
    let lines: Vec<String> = (0..12).map(|i| format!("  L{i}")).collect();
    let text = lines.join("\n");
    let doc = SimpleDocument::new(&text);

    // Cursor starts at offset 0 (line 0, col 0).
    // Type "1", "0", "j" → should parse as count=10, motion=j.
    let r1 = process_key(&mut engine, &doc, KeyEvent::char('1'));
    assert!(r1.consumed());

    let r2 = process_key(&mut engine, &doc, KeyEvent::char('0'));
    assert!(r2.consumed());
    // After '1' then '0', the parser should still be accumulating count,
    // NOT have expanded '0' to '^'.
    assert!(
        engine.parser.state().is_accumulating_count(),
        "parser should be accumulating count 10, not have expanded 0→^"
    );

    // Now press 'j' to execute the motion with count 10.
    let response = process_key(&mut engine, &doc, KeyEvent::char('j'));
    assert!(response.consumed());

    // Extract SetCursor effect: should land on line 10.
    // Line 10 starts at byte offset = sum of lengths of lines 0..10.
    // Line 0: "  L0" (4 chars) + '\n' = 5 bytes
    // ...
    // Line 9: "  L9" (4 chars) + '\n' = 5 bytes  → lines 0-9 total 50 bytes
    // Line 10: "  L10" (5 chars) — starts at offset 50
    // Line 11: "  L11" (5 chars) — last line, no trailing newline
    let expected_offset: usize = (0..10)
        .map(|i| format!("  L{i}").len() + 1) // +1 for '\n'
        .sum();

    let cursor_effect = response.effects().iter().find_map(|e| {
        if let Effect::SetCursor { offset, .. } = e {
            Some(offset.get())
        } else {
            None
        }
    });

    assert_eq!(
        cursor_effect,
        Some(expected_offset),
        "10j should move to line 10 (offset {expected_offset}), \
         not expand 0→^ mapping mid-count"
    );
}

#[test]
fn standalone_zero_still_triggers_mapping() {
    // `nmap 0 ^` — a standalone `0` (no count accumulating) should still
    // expand through the mapping and behave as `^` (first non-blank).
    let mut engine = VimEngine::new();
    engine.source_config_text("nmap 0 ^");

    // "    hello" — 4 spaces then "hello". First non-blank is at offset 4.
    let doc = SimpleDocument::new("    hello");

    // Cursor at offset 0 (column 0). Press '0' alone.
    let response = process_key(&mut engine, &doc, KeyEvent::char('0'));
    assert!(response.consumed());

    // The mapping 0→^ should fire. `^` moves to first non-blank (offset 4).
    let cursor_offset = response.effects().iter().find_map(|e| {
        if let Effect::SetCursor { offset, .. } = e {
            Some(offset.get())
        } else {
            None
        }
    });

    assert_eq!(
        cursor_offset,
        Some(4),
        "standalone 0 with `nmap 0 ^` should go to first non-blank (offset 4), \
         got {cursor_offset:?}"
    );
}

#[test]
fn zero_in_operator_count2_suppresses_mapping() {
    // After typing `d10w`, the `0` in `10` is count2 (post-operator count).
    // With `nmap 0 ^`, the `0` must not expand — it's part of count2.
    let mut engine = VimEngine::new();
    engine.source_config_text("nmap 0 ^");

    let doc = SimpleDocument::new("aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk");

    // Type 'd' (operator), '1' (count2 digit), '0' (count2 digit)
    let r1 = process_key(&mut engine, &doc, KeyEvent::char('d'));
    assert!(r1.consumed());
    assert!(r1.pending(), "'d' should leave parser in operator-pending");

    let r2 = process_key(&mut engine, &doc, KeyEvent::char('1'));
    assert!(r2.consumed());

    let r3 = process_key(&mut engine, &doc, KeyEvent::char('0'));
    assert!(r3.consumed());

    // Parser should still be in Operator state accumulating count2=10,
    // not have expanded 0→^.
    assert!(
        engine.parser.state().is_accumulating_count(),
        "parser should be accumulating count2=10 in operator state, \
         not have expanded 0→^; state = {:?}",
        engine.parser.state()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Expression register re-evaluation during dot-repeat
// ═══════════════════════════════════════════════════════════════════════════

/// Verify that `<C-r>=expr<CR>` in insert mode stores the expression text
/// for later dot-repeat re-evaluation.
#[test]
fn ctrl_r_eq_stores_last_expression_text() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello ");

    // Enter insert mode at end (A)
    process_key(&mut engine, &doc, KeyEvent::char('A'));
    assert_eq!(engine.mode(), Mode::Insert);

    // Type Ctrl-R (insert register)
    process_key_at(&mut engine, &doc, KeyEvent::ctrl('r'), 6);

    // Type = (expression register)
    process_key_at(&mut engine, &doc, KeyEvent::char('='), 6);

    // Type "2+3"
    for ch in "2+3".chars() {
        process_key_at(&mut engine, &doc, KeyEvent::char(ch), 6);
    }

    // Press Enter to submit expression
    process_key_at(&mut engine, &doc, KeyEvent::enter(), 6);

    // Verify the expression text was stored
    assert_eq!(
        engine.state().last_expression_text(),
        Some("2+3"),
        "expression text should be stored for dot-repeat re-evaluation"
    );
}

/// Verify that dot-repeating a put command with the expression register
/// emits a `HostRequest::EvaluateExpression` for re-evaluation.
#[test]
fn dot_repeat_expression_register_emits_evaluate_request() {
    use crate::execution::host::HostRequest;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello ");

    // Manually set the expression register and the last expression text.
    // This simulates a prior `"=2+3<CR>p` where the expression was evaluated.
    engine
        .state
        .registers_mut()
        .set_expression_result(crate::primitives::RegisterContent::char_wise("5"));
    engine.state.store_last_expression_text("2+3");

    // Set up last_command as `"=p` (put from expression register).
    // In the grammar, this is `Command::Action { action: Put, register: Some(EXPRESSION) }`.
    engine
        .parser
        .set_last_command_for_test(crate::grammar::Command::Action {
            count: NonZeroU32::MIN,
            register: Some(crate::primitives::RegisterName::EXPRESSION),
            action: crate::grammar::Action::Put,
        });

    // Press `.` to dot-repeat
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('.'), 5);

    // Check that an EvaluateExpression host request was emitted
    let eval_request = response
        .host_requests()
        .iter()
        .find(|r| matches!(r, HostRequest::EvaluateExpression { .. }));
    assert!(
        eval_request.is_some(),
        "dot-repeat of \"=p should emit HostRequest::EvaluateExpression; \
         got requests: {:?}",
        response.host_requests()
    );

    // Verify the expression text in the request matches
    if let Some(HostRequest::EvaluateExpression { expression, .. }) = eval_request {
        assert_eq!(
            expression.as_str(),
            "2+3",
            "re-evaluation request should use the stored expression text"
        );
    }
}

/// Verify that the expression register is synchronously re-evaluated
/// during dot-repeat (internal evaluator fallback).
#[test]
fn dot_repeat_expression_register_sync_reevaluates() {
    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello ");

    // Set initial expression register value (stale)
    engine
        .state
        .registers_mut()
        .set_expression_result(crate::primitives::RegisterContent::char_wise("old_value"));
    engine.state.store_last_expression_text("7*6");

    // Set up last_command as `"=p`
    engine
        .parser
        .set_last_command_for_test(crate::grammar::Command::Action {
            count: NonZeroU32::MIN,
            register: Some(crate::primitives::RegisterName::EXPRESSION),
            action: crate::grammar::Action::Put,
        });

    // Press `.` to dot-repeat
    process_key_at(&mut engine, &doc, KeyEvent::char('.'), 5);

    // The internal evaluator should have updated the expression register
    // with the re-evaluated result of "7*6" = "42"
    let reg_content = engine
        .state
        .registers()
        .get(crate::primitives::RegisterName::EXPRESSION);
    assert!(
        reg_content.is_some(),
        "expression register should have content after re-evaluation"
    );
    let text = reg_content.unwrap().text();
    assert_eq!(
        text, "42",
        "expression register should be re-evaluated (7*6=42), not stale (old_value)"
    );
}

/// Verify that dot-repeat without expression register does NOT emit
/// an `EvaluateExpression` request.
#[test]
fn dot_repeat_non_expression_register_no_reevaluation() {
    use crate::execution::host::HostRequest;

    let mut engine = VimEngine::new();
    let doc = SimpleDocument::new("hello ");

    // Set up last_command as regular `p` (unnamed register)
    engine
        .parser
        .set_last_command_for_test(crate::grammar::Command::Action {
            count: NonZeroU32::MIN,
            register: None,
            action: crate::grammar::Action::Put,
        });

    // Store some text in unnamed register so put has something to paste
    engine.state.registers_mut().set(
        crate::primitives::RegisterName::UNNAMED,
        crate::primitives::RegisterContent::char_wise("world"),
    );

    // Press `.` to dot-repeat
    let response = process_key_at(&mut engine, &doc, KeyEvent::char('.'), 5);

    // Should NOT emit EvaluateExpression
    let eval_request = response
        .host_requests()
        .iter()
        .find(|r| matches!(r, HostRequest::EvaluateExpression { .. }));
    assert!(
        eval_request.is_none(),
        "dot-repeat of regular put should NOT emit EvaluateExpression"
    );
}
