use super::*;
use crate::effects::Effect;
use crate::execution::InputContext;
use crate::grammar::VisualKind;
use crate::primitives::{Offset, SelectionRange};
use crate::test_utils::SimpleDocument;
use std::num::NonZeroU32;

/// Default options for test helpers.
static TEST_OPTIONS: std::sync::LazyLock<crate::primitives::VimOptions> =
    std::sync::LazyLock::new(crate::primitives::VimOptions::default);

/// Helper: create ExecutionContext from text and cursor offset.
fn exec_ctx<'ctx>(
    doc: &'ctx SimpleDocument,
    state: &'ctx crate::state::VimState,
    cursor: usize,
) -> ExecutionContext<'ctx, SimpleDocument> {
    let input = InputContext::new(doc, cursor).validate().unwrap();
    ExecutionContext::new(input, state, &TEST_OPTIONS)
}

fn has_effect(effects: &Effects, check: impl Fn(&Effect) -> bool) -> bool {
    effects.as_slice().iter().any(check)
}

struct MockSyntaxProvider;

impl crate::document::SyntaxProvider for MockSyntaxProvider {
    fn enclosing_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: crate::document::SyntaxNodeKind,
    ) -> Option<(usize, usize)> {
        None
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
    fn ancestor_node(&self, _text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
        if start == 5 && end == 5 {
            return Some((0, 25));
        }
        if start == 0 && end == 25 {
            return None;
        }
        if start == 12 && end == 22 {
            return Some((0, 25));
        }
        if start == 11 && end == 20 {
            return Some((0, 25));
        }
        if start == 5 && end == 10 {
            return Some((0, 25));
        }
        None
    }
    fn descendant_node(&self, _text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
        if start == 0 && end == 25 {
            return Some((12, 22));
        }
        if start == 0 && end == 10 {
            return Some((2, 8));
        }
        if start == 11 && end == 20 {
            return Some((13, 18));
        }
        None
    }
    fn next_sibling_node(&self, _text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
        if start == 0 && end == 10 {
            return Some((11, 20));
        }
        if start == 11 && end == 20 {
            return Some((21, 25));
        }
        None
    }
    fn prev_sibling_node(&self, _text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
        if start == 11 && end == 20 {
            return Some((0, 10));
        }
        if start == 21 && end == 25 {
            return Some((11, 20));
        }
        None
    }
    fn sibling_nodes(&self, _text: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
        if start == 0 && end == 10 {
            return vec![(0, 10), (11, 20), (21, 25)];
        }
        if start == 11 && end == 20 {
            return vec![(11, 15), (16, 20)];
        }
        Vec::new()
    }
    fn child_nodes(&self, _text: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
        if start == 0 && end == 25 {
            return vec![(0, 10), (12, 22)];
        }
        Vec::new()
    }
}

fn exec_ctx_with_syntax<'ctx>(
    doc: &'ctx SimpleDocument,
    state: &'ctx crate::state::VimState,
    cursor: usize,
    syntax: &'ctx dyn crate::document::SyntaxProvider,
) -> ExecutionContext<'ctx, SimpleDocument> {
    let providers = crate::document::Providers::new().with_syntax(syntax);
    let input = InputContext::new(doc, cursor)
        .validate()
        .unwrap()
        .with_providers(providers);
    ExecutionContext::new(input, state, &TEST_OPTIONS)
}

fn exec_ctx_visual_with_syntax<'ctx>(
    doc: &'ctx SimpleDocument,
    state: &'ctx crate::state::VimState,
    cursor: usize,
    anchor: usize,
    head: usize,
    syntax: &'ctx dyn crate::document::SyntaxProvider,
) -> ExecutionContext<'ctx, SimpleDocument> {
    let providers = crate::document::Providers::new().with_syntax(syntax);
    let input = InputContext::new(doc, cursor)
        .validate()
        .unwrap()
        .with_selection(SelectionRange::new(Offset::new(anchor), Offset::new(head)))
        .with_providers(providers);
    ExecutionContext::new(input, state, &TEST_OPTIONS)
}

// ─── Motion dispatch ──────────────────────────────────────────────

#[test]
fn execute_motion_produces_set_cursor() {
    let doc = SimpleDocument::new("hello\nworld");
    let state = crate::state::VimState::default();
    let ctx = exec_ctx(&doc, &state, 0);

    let effects = execute(
        Command::Motion {
            motion: crate::grammar::types::Motion::Down,
            count: NonZeroU32::MIN,
            explicit_count: false,
        },
        &ctx,
    )
    .effects;

    // 'j' motion should produce a SetCursor effect
    assert!(
        effects
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "Down motion should produce SetCursor"
    );
}

// ─── Visual enter ─────────────────────────────────────────────────

#[test]
fn execute_visual_enter_produces_selection() {
    let doc = SimpleDocument::new("hello");
    let state = crate::state::VimState::default();
    let ctx = exec_ctx(&doc, &state, 0);

    let effects = execute(
        Command::Visual(VisualKind::Enter {
            visual_type: VisualType::Char,
            count: None,
        }),
        &ctx,
    )
    .effects;

    // Entering visual mode should produce a selection and mode change
    let has_mode = effects
        .as_slice()
        .iter()
        .any(|e| matches!(e, Effect::SetMode { .. }));
    let has_selection = effects
        .as_slice()
        .iter()
        .any(|e| matches!(e, Effect::SetSelection { .. }));
    assert!(has_mode, "visual enter must produce a SetMode effect");
    assert!(
        has_selection,
        "visual enter must produce a SetSelection effect"
    );
}

// ─── Sticky column builder ────────────────────────────────────────

#[test]
fn execute_motion_with_count_moves_further() {
    let doc = SimpleDocument::new("hello\nworld\nfoo");
    let state = crate::state::VimState::default();
    let ctx = exec_ctx(&doc, &state, 0);

    // j with count=2 should move 2 lines down
    let effects = execute(
        Command::Motion {
            motion: crate::grammar::types::Motion::Down,
            count: NonZeroU32::new(2).unwrap(),
            explicit_count: true,
        },
        &ctx,
    )
    .effects;

    // Should produce a SetCursor effect
    let cursor_effect = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetCursor { offset } = e {
            Some(offset.get())
        } else {
            None
        }
    });
    assert!(cursor_effect.is_some(), "2j should produce SetCursor");

    let cursor_offset = effects
        .as_slice()
        .iter()
        .filter_map(|e| {
            if let Effect::SetCursor { offset, .. } = e {
                Some(offset.get())
            } else {
                None
            }
        })
        .last();
    assert_eq!(
        cursor_offset,
        Some(12),
        "2j from line 0 should land on line 2 (offset 12)"
    );
}

// ─── Changelist motions ───────────────────────────────────────────

#[test]
fn execute_changelist_older_empty_returns_error() {
    let doc = SimpleDocument::new("hello");
    let state = crate::state::VimState::default();
    let ctx = exec_ctx(&doc, &state, 0);

    let effects = execute(
        Command::Motion {
            motion: crate::grammar::types::Motion::ChangelistOlder,
            count: NonZeroU32::MIN,
            explicit_count: false,
        },
        &ctx,
    )
    .effects;

    assert!(
        effects
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::ShowError { .. })),
        "ChangelistOlder on empty changelist should produce ShowError"
    );
}

#[test]
fn execute_changelist_older_with_entries() {
    let doc = SimpleDocument::new("hello");
    let mut state = crate::state::VimState::default();
    state
        .changelist_mut()
        .push(crate::primitives::Offset::new(3));
    let ctx = exec_ctx(&doc, &state, 0);

    let effects = execute(
        Command::Motion {
            motion: crate::grammar::types::Motion::ChangelistOlder,
            count: NonZeroU32::MIN,
            explicit_count: false,
        },
        &ctx,
    )
    .effects;

    assert!(
        effects
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::ChangelistOlder { .. })),
        "ChangelistOlder with entries should produce ChangelistOlder effect"
    );
    assert!(
        effects
            .as_slice()
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "ChangelistOlder should also produce SetCursor effect"
    );
}

// ─── Operator + motion ────────────────────────────────────────────

#[test]
fn select_parent_node_emits_push_and_set_selection() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::cursor(Offset::new(5)));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_with_syntax(&doc, &state, 5, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;

    // Should emit SyntaxSelectionPush + SetMode(Visual) + SetSelection
    assert!(
        has_effect(&effects, |e| matches!(
            e,
            Effect::SyntaxSelectionPush { .. }
        )),
        "Should push current range onto history"
    );
    assert!(
        has_effect(&effects, |e| matches!(
            e,
            Effect::SetMode {
                mode: Mode::Visual(_),
                ..
            }
        )),
        "Should enter visual mode from normal"
    );
    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
        if anchor.get() == 0 && head.get() == 25)
        ),
        "Should set selection to parent range (0, 25)"
    );
}

#[test]
fn select_parent_node_no_provider_returns_empty() {
    let doc = SimpleDocument::new("hello world");
    let state = crate::state::VimState::default();
    let ctx = exec_ctx(&doc, &state, 5); // no syntax provider

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;
    assert!(
        effects.is_empty(),
        "No provider should produce empty effects"
    );
}

#[test]
fn select_parent_node_at_root_returns_empty() {
    // ancestor_node returns None for (0, 25)
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(25),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 25, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;
    assert!(
        effects.is_empty(),
        "At root, expand should produce empty effects"
    );
}

#[test]
fn select_parent_in_visual_mode_does_not_emit_set_mode() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(12),
            Offset::new(22),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 5, 12, 22, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;

    // Should NOT emit SetMode since already in visual
    assert!(
        !has_effect(&effects, |e| matches!(e, Effect::SetMode { .. })),
        "Already in visual mode, should not re-emit SetMode"
    );
    assert!(
        has_effect(&effects, |e| matches!(e, Effect::SetSelection { .. })),
        "Should still set selection"
    );
}

// ─── SelectChildNode (g]) ──────────────────────────────────────────

#[test]
fn select_child_node_with_history_pops_and_sets_selection() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    // Simulate that we previously expanded from (12, 22) to (0, 25)
    state
        .syntax_selection_mut()
        .push(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(12),
            Offset::new(22),
        )));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(25),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 25, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectChildNode,
        },
        &ctx,
    )
    .effects;

    assert!(
        has_effect(&effects, |e| matches!(e, Effect::SyntaxSelectionPop)),
        "Should pop from history"
    );
    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
        if anchor.get() == 12 && head.get() == 22)
        ),
        "Should set selection to popped range (12, 22)"
    );
}

#[test]
fn select_child_node_without_history_uses_descendant() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(25),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 25, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectChildNode,
        },
        &ctx,
    )
    .effects;

    // No history → should NOT emit SyntaxSelectionPop
    assert!(
        !has_effect(&effects, |e| matches!(e, Effect::SyntaxSelectionPop)),
        "No history, should not pop"
    );
    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
        if anchor.get() == 12 && head.get() == 22)
        ),
        "Should use descendant_node result (12, 22)"
    );
}

// ─── SelectNextSibling (g}) / SelectPrevSibling (g{) ───────────────

#[test]
fn select_next_sibling_sets_selection_no_history() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(10),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectNextSibling,
        },
        &ctx,
    )
    .effects;

    assert!(
        !has_effect(&effects, |e| matches!(
            e,
            Effect::SyntaxSelectionPush { .. }
        )),
        "Sibling navigation should NOT push history"
    );
    assert!(
        !has_effect(&effects, |e| matches!(e, Effect::SyntaxSelectionPop)),
        "Sibling navigation should NOT pop history"
    );
    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
        if anchor.get() == 11 && head.get() == 20)
        ),
        "Should select next sibling range (11, 20)"
    );
}

#[test]
fn select_prev_sibling_sets_selection() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(11),
            Offset::new(20),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 11, 11, 20, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectPrevSibling,
        },
        &ctx,
    )
    .effects;

    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
        if anchor.get() == 10 && head.get() == 0)
        ),
        "Should select prev sibling range with backward direction (anchor=10, head=0)"
    );
}

#[test]
fn select_sibling_no_match_returns_empty() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(25),
        )));
    let syntax = MockSyntaxProvider;
    // next_sibling_node returns None for (0, 25)
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 25, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectNextSibling,
        },
        &ctx,
    )
    .effects;
    assert!(effects.is_empty(), "No sibling → empty effects");
}

#[test]
fn select_next_sibling_direction_is_forward() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(10),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectNextSibling,
        },
        &ctx,
    )
    .effects;

    // Next sibling should produce forward selection: anchor=11 (start), head=20 (end)
    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
                if anchor.get() == 11 && head.get() == 20)
        ),
        "Next sibling should have forward direction (anchor=start, head=end)"
    );
}

#[test]
fn select_prev_sibling_direction_is_backward() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(11),
            Offset::new(20),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 11, 11, 20, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectPrevSibling,
        },
        &ctx,
    )
    .effects;

    // Prev sibling should produce backward selection: anchor=10 (end), head=0 (start)
    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
                if anchor.get() == 10 && head.get() == 0)
        ),
        "Prev sibling should have backward direction (anchor=end, head=start)"
    );
}

#[test]
fn select_next_sibling_multi_cursor() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    // Set up 2 cursors: (0,10) and (11,20) — both have next siblings
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(10)),
                SelectionRange::new(Offset::new(11), Offset::new(20)),
            ],
            0,
        ));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectNextSibling,
        },
        &ctx,
    )
    .effects;

    // Should emit SetSyntaxSelections with both cursors moved
    let sels = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSyntaxSelections { selections } = e {
            Some(selections)
        } else {
            None
        }
    });
    assert!(
        sels.is_some(),
        "Should emit SetSyntaxSelections for multi-cursor"
    );
    let sels = sels.unwrap();
    assert_eq!(sels.len(), 2, "Should have 2 cursors");
    // Cursor 1: (0,10) → next sibling (11,20)
    assert_eq!(sels.ranges()[0].start().get(), 11);
    assert_eq!(sels.ranges()[0].end().get(), 20);
    // Cursor 2: (11,20) → next sibling (21,25)
    assert_eq!(sels.ranges()[1].start().get(), 21);
    assert_eq!(sels.ranges()[1].end().get(), 25);
}

#[test]
fn select_next_sibling_partial_success() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    // Cursor 1: (0,10) has next sibling; Cursor 2: (21,25) does NOT
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(10)),
                SelectionRange::new(Offset::new(21), Offset::new(25)),
            ],
            0,
        ));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectNextSibling,
        },
        &ctx,
    )
    .effects;

    let sels = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSyntaxSelections { selections } = e {
            Some(selections)
        } else {
            None
        }
    });
    assert!(
        sels.is_some(),
        "Partial success should still emit SetSyntaxSelections"
    );
    let sels = sels.unwrap();
    assert_eq!(sels.len(), 2, "Should preserve both cursors");
    // Cursor 1 moved: (0,10) → (11,20)
    assert_eq!(sels.ranges()[0].start().get(), 11);
    assert_eq!(sels.ranges()[0].end().get(), 20);
    // Cursor 2 stayed: (21,25) unchanged
    assert_eq!(sels.ranges()[1].start().get(), 21);
    assert_eq!(sels.ranges()[1].end().get(), 25);
}

#[test]
fn select_next_sibling_all_no_change_is_noop() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    // Cursor at last sibling — no next sibling available
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(21),
            Offset::new(25),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 21, 21, 25, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectNextSibling,
        },
        &ctx,
    )
    .effects;

    assert!(effects.is_empty(), "All cursors unchanged → empty effects");
}

// ─── Normal mode entry into visual ─────────────────────────────────

#[test]
fn syntax_selection_from_normal_enters_visual_char() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::cursor(Offset::new(5)));
    let syntax = MockSyntaxProvider;
    // next_sibling_node for (0,0) won't match, so this exercises parent
    let ctx = exec_ctx_with_syntax(&doc, &state, 5, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;

    assert!(
        has_effect(&effects, |e| matches!(
            e,
            Effect::SetMode {
                mode: Mode::Visual(VisualType::Char),
                ..
            }
        )),
        "Normal mode → should enter Visual Char"
    );
}

// ─── Count support ──────────────────────────────────────────────────

#[test]
fn select_parent_count_3_with_single_expansion() {
    // Our mock only supports one level of expansion from (5,5) → (0,25).
    // A count of 3 should expand once and stop (ancestor_node returns None for (0,25)).
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::cursor(Offset::new(5)));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_with_syntax(&doc, &state, 5, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::new(3).unwrap(),
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;

    // Should have exactly 1 push (only 1 expansion level available)
    let push_count = effects
        .as_slice()
        .iter()
        .filter(|e| matches!(e, Effect::SyntaxSelectionPush { .. }))
        .count();
    assert_eq!(
        push_count, 1,
        "Only 1 expansion available, should push once"
    );
    assert!(
        has_effect(&effects, |e| matches!(e, Effect::SetSelection { .. })),
        "Should still set selection"
    );
}

// ─── Expand then shrink retraces path ──────────────────────────────

#[test]
fn expand_then_shrink_retraces_path() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let syntax = MockSyntaxProvider;

    // Step 1: Expand from cursor at 5 (normal mode)
    let mut state = crate::state::VimState::default();
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::cursor(Offset::new(5)));
    let ctx = exec_ctx_with_syntax(&doc, &state, 5, &syntax);
    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;
    // Verify expansion happened
    assert!(has_effect(&effects, |e| matches!(
        e,
        Effect::SyntaxSelectionPush { .. }
    )));

    // Step 2: Shrink back — simulate state after expansion
    let mut state2 = crate::state::VimState::default();
    state2.set_mode(Mode::Visual(VisualType::Char));
    state2
        .syntax_selection_mut()
        .push(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(5),
            Offset::new(5),
        )));
    state2
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(25),
        )));
    let ctx2 = exec_ctx_visual_with_syntax(&doc, &state2, 0, 0, 25, &syntax);
    let effects2 = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectChildNode,
        },
        &ctx2,
    )
    .effects;
    // Should retrace to original (5, 5) via pop
    assert!(has_effect(&effects2, |e| matches!(
        e,
        Effect::SyntaxSelectionPop
    )));
    assert!(
        has_effect(
            &effects2,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
        if anchor.get() == 5 && head.get() == 5)
        ),
        "Should retrace to original range (5, 5)"
    );
}

// ─── Out-of-bounds byte offset validation ──────────────────────────

/// A malicious syntax provider that returns offsets far beyond document bounds.
struct OutOfBoundsSyntaxProvider;

impl crate::document::SyntaxProvider for OutOfBoundsSyntaxProvider {
    fn enclosing_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: crate::document::SyntaxNodeKind,
    ) -> Option<(usize, usize)> {
        // Return offsets far beyond any reasonable document length
        Some((50, 100))
    }
    fn next_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: crate::document::SyntaxNodeKind,
        _count: u32,
    ) -> Option<usize> {
        Some(999)
    }
    fn prev_node(
        &self,
        _text: &str,
        _cursor: usize,
        _kind: crate::document::SyntaxNodeKind,
        _count: u32,
    ) -> Option<usize> {
        Some(500)
    }
    fn ancestor_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        // Document is 10 chars but we return (50, 100)
        Some((50, 100))
    }
    fn descendant_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        Some((200, 300))
    }
    fn next_sibling_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        Some((80, 150))
    }
    fn prev_sibling_node(&self, _text: &str, _start: usize, _end: usize) -> Option<(usize, usize)> {
        Some((60, 90))
    }
}

#[test]
fn out_of_bounds_ancestor_node_does_not_panic_and_clamps() {
    // Document is 10 chars; provider returns (50, 100).
    let doc = SimpleDocument::new("0123456789");
    let state = crate::state::VimState::default();
    let syntax = OutOfBoundsSyntaxProvider;
    let ctx = exec_ctx_with_syntax(&doc, &state, 5, &syntax);

    // This must not panic — validated_range clamps to doc_len.
    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;

    // The clamped range is (10, 10) since both 50 and 100 clamp to doc_len=10.
    // Since clamped start == clamped end == 10 (same as a zero-width range),
    // and the original selection was (5, 5), the expansion did produce a "change"
    // (from 5,5 to 10,10), so effects should contain a selection.
    assert!(
        has_effect(&effects, |e| matches!(e, Effect::SetSelection { .. })),
        "Clamped out-of-bounds range should still produce a selection"
    );

    // Verify the selection offsets are clamped to document length (10)
    let sel = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSelection { anchor, head, .. } = e {
            Some((anchor.get(), head.get()))
        } else {
            None
        }
    });
    let (anchor, head) = sel.expect("SetSelection should be present");
    assert!(
        anchor <= 10 && head <= 10,
        "Selection offsets must be clamped to doc_len; got anchor={anchor}, head={head}"
    );
}

#[test]
fn out_of_bounds_descendant_node_does_not_panic_and_clamps() {
    // Document is 10 chars; descendant_node returns (200, 300).
    let doc = SimpleDocument::new("0123456789");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    let syntax = OutOfBoundsSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectChildNode,
        },
        &ctx,
    )
    .effects;

    // Both 200 and 300 clamp to 10, giving (10, 10). Since original was (0, 10)
    // and target is (10, 10), a change occurred.
    assert!(
        has_effect(&effects, |e| matches!(e, Effect::SetSelection { .. })),
        "Clamped descendant range should produce a selection"
    );

    let sel = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSelection { anchor, head, .. } = e {
            Some((anchor.get(), head.get()))
        } else {
            None
        }
    });
    let (anchor, head) = sel.expect("SetSelection should be present");
    assert!(
        anchor <= 10 && head <= 10,
        "Descendant selection must be clamped; got anchor={anchor}, head={head}"
    );
}

#[test]
fn out_of_bounds_sibling_node_does_not_panic_and_clamps() {
    // Document is 10 chars; next_sibling_node returns (80, 150).
    let doc = SimpleDocument::new("0123456789");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    let syntax = OutOfBoundsSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 5, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectNextSibling,
        },
        &ctx,
    )
    .effects;

    // Both 80 and 150 clamp to 10. Original was (0, 5), target is (10, 10).
    assert!(
        has_effect(&effects, |e| matches!(e, Effect::SetSelection { .. })),
        "Clamped sibling range should produce a selection"
    );

    let sel = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSelection { anchor, head, .. } = e {
            Some((anchor.get(), head.get()))
        } else {
            None
        }
    });
    let (anchor, head) = sel.expect("SetSelection should be present");
    assert!(
        anchor <= 10 && head <= 10,
        "Sibling selection must be clamped; got anchor={anchor}, head={head}"
    );
}

// ─── Containment validation ──────────────────────────────────────

#[test]
fn shrink_with_stale_history_clears_and_falls_back() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .syntax_selection_mut()
        .push(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(30),
            Offset::new(40),
        )));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(25),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 25, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectChildNode,
        },
        &ctx,
    )
    .effects;

    assert!(
        has_effect(&effects, |e| matches!(e, Effect::SyntaxHistoryClear)),
        "Should clear stale history"
    );
    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
        if anchor.get() == 12 && head.get() == 22)
        ),
        "Should fall back to descendant_node"
    );
}

// ─── Fan-out tests ───────────────────────────────────────────────

#[test]
fn select_all_siblings_creates_multi_cursor() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(10),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectAllSiblings,
        },
        &ctx,
    )
    .effects;

    assert!(
        has_effect(&effects, |e| matches!(
            e,
            Effect::SyntaxSelectionPush { .. }
        )),
        "Should push snapshot before fan-out"
    );
    assert!(
        has_effect(&effects, |e| matches!(
            e,
            Effect::SetSyntaxSelections { .. }
        )),
        "Should emit SetSyntaxSelections"
    );
}

#[test]
fn select_all_siblings_no_op_for_only_child() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(12),
            Offset::new(22),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 12, 12, 22, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectAllSiblings,
        },
        &ctx,
    )
    .effects;

    assert!(effects.is_empty(), "Only child → no-op");
}

#[test]
fn select_all_children_creates_multi_cursor() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(25),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 25, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectAllChildren,
        },
        &ctx,
    )
    .effects;

    assert!(
        has_effect(&effects, |e| matches!(
            e,
            Effect::SetSyntaxSelections { .. }
        )),
        "Should emit SetSyntaxSelections for children"
    );
}

#[test]
fn select_all_siblings_multi_cursor_fans_out_each() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    // Cursor 1 at (0,10): siblings are (0,10),(11,20),(21,25)
    // Cursor 2 at (11,20): siblings are (11,15),(16,20) — different parent context
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(10)),
                SelectionRange::new(Offset::new(11), Offset::new(20)),
            ],
            0,
        ));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectAllSiblings,
        },
        &ctx,
    )
    .effects;

    let sels = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSyntaxSelections { selections } = e {
            Some(selections)
        } else {
            None
        }
    });
    assert!(sels.is_some(), "Should emit SetSyntaxSelections");
    let sels = sels.unwrap();
    // After normalize: (0,10), (11,15), (16,20), (21,25) — note (11,20) overlaps
    // with (11,15)+(16,20), so normalize merges. Result depends on normalize behavior.
    // At minimum, all 5 original ranges from both fan-outs should be present (possibly merged).
    assert!(
        sels.ranges().len() >= 3,
        "Should have multiple cursors from both fan-outs, got {}",
        sels.ranges().len()
    );
}

#[test]
fn select_all_siblings_preserves_direction() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    let syntax = MockSyntaxProvider;
    // Backward selection: anchor=10, head=0
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(10),
            Offset::new(0),
        )));
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 10, 0, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectAllSiblings,
        },
        &ctx,
    )
    .effects;

    let sels = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSyntaxSelections { selections } = e {
            Some(selections)
        } else {
            None
        }
    });
    assert!(sels.is_some(), "Should emit SetSyntaxSelections");
    let sels = sels.unwrap();
    // All fanned-out ranges should be backward (anchor > head)
    for (i, r) in sels.ranges().iter().enumerate() {
        assert!(
            !r.is_forward() || r.is_collapsed(),
            "Range {i} should be backward: anchor={}, head={}",
            r.anchor().get(),
            r.head().get(),
        );
    }
}

#[test]
fn shrink_child_fallback_multi_cursor() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    // No history — forces fallback path
    // 2 cursors: (0,10) and (11,20) — both have descendants
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(10)),
                SelectionRange::new(Offset::new(11), Offset::new(20)),
            ],
            0,
        ));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectChildNode,
        },
        &ctx,
    )
    .effects;

    let sels = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSyntaxSelections { selections } = e {
            Some(selections)
        } else {
            None
        }
    });
    assert!(
        sels.is_some(),
        "Should emit SetSyntaxSelections for multi-cursor shrink"
    );
    let sels = sels.unwrap();
    assert_eq!(sels.len(), 2, "Should have 2 cursors");
    // Cursor 1: (0,10) → descendant (2,8)
    assert_eq!(sels.ranges()[0].start().get(), 2);
    assert_eq!(sels.ranges()[0].end().get(), 8);
    // Cursor 2: (11,20) → descendant (13,18)
    assert_eq!(sels.ranges()[1].start().get(), 13);
    assert_eq!(sels.ranges()[1].end().get(), 18);
}

#[test]
fn expand_parent_preserves_backward_direction() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(10),
            Offset::new(5),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 5, 10, 5, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;

    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
                if anchor.get() > head.get())
        ),
        "Expand should preserve backward direction"
    );
}

#[test]
fn expand_parent_normalizes_duplicates() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(12), Offset::new(22)),
                SelectionRange::new(Offset::new(11), Offset::new(20)),
            ],
            0,
        ));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 12, 12, 22, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectParentNode,
        },
        &ctx,
    )
    .effects;

    let sels = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSyntaxSelections { selections } = e {
            Some(selections)
        } else {
            None
        }
    });
    assert!(sels.is_some(), "Should emit SetSyntaxSelections");
    let sels = sels.unwrap();
    assert_eq!(
        sels.len(),
        1,
        "Two cursors expanding to same parent should merge to 1"
    );
}

#[test]
fn shrink_pop_restores_backward_direction() {
    let doc = SimpleDocument::new("fn main() { let x = 1; } ");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    // Push backward history entry: anchor=10, head=5
    state
        .syntax_selection_mut()
        .push(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(10),
            Offset::new(5),
        )));
    // Current selection is (0,25) — contains history entry (5,10)
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::single(SelectionRange::new(
            Offset::new(0),
            Offset::new(25),
        )));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 25, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectChildNode,
        },
        &ctx,
    )
    .effects;

    assert!(
        has_effect(&effects, |e| matches!(e, Effect::SyntaxSelectionPop)),
        "Should pop history"
    );
    assert!(
        has_effect(
            &effects,
            |e| matches!(e, Effect::SetSelection { anchor, head, .. }
                if anchor.get() == 10 && head.get() == 5)
        ),
        "Should restore backward direction: anchor=10, head=5"
    );
}

#[test]
fn select_next_sibling_normalizes_duplicates() {
    let doc = SimpleDocument::new("aaaaaaaaaa bbbbbbbbb cccccccccc");
    let mut state = crate::state::VimState::default();
    state.set_mode(Mode::Visual(VisualType::Char));
    // Two identical cursors — both will navigate to the same next sibling
    state
        .multi_cursor_mut()
        .set_selections(crate::primitives::Selections::from_vec(
            vec![
                SelectionRange::new(Offset::new(0), Offset::new(10)),
                SelectionRange::new(Offset::new(0), Offset::new(10)),
            ],
            0,
        ));
    let syntax = MockSyntaxProvider;
    let ctx = exec_ctx_visual_with_syntax(&doc, &state, 0, 0, 10, &syntax);

    let effects = execute(
        Command::Prefix {
            count: NonZeroU32::MIN,
            register: None,
            command: PrefixCommand::SelectNextSibling,
        },
        &ctx,
    )
    .effects;

    let sels = effects.as_slice().iter().find_map(|e| {
        if let Effect::SetSyntaxSelections { selections } = e {
            Some(selections)
        } else {
            None
        }
    });
    assert!(sels.is_some(), "Should emit SetSyntaxSelections");
    let sels = sels.unwrap();
    assert_eq!(
        sels.len(),
        1,
        "Two cursors navigating to same sibling should merge to 1"
    );
}
