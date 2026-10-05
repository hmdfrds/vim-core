//! Bundled per-buffer engine state for save/restore during buffer switches.
//!
//! [`BufferLocalState`] is returned by [`super::VimEngine::on_buffer_leave`]
//! and accepted by [`super::VimEngine::on_buffer_enter`]. It bundles all
//! per-buffer state so the host can persist it keyed by buffer identity.

use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::keymap::BufferMappings;
use crate::primitives::{LastVisualInfo, OptionId, OptionOverrides, VimValue, VirtualColumn};
use crate::state::{BufferMarks, ChangeList, UndoTree};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::VimEngine;
    use crate::primitives::UndoCursorStrategy;
    use crate::primitives::{Mark, MarkName, Offset};
    use crate::state::mark_snapshot::MarkSnapshot;
    use crate::state::UndoTree;
    use std::collections::BTreeMap;

    fn mn(c: char) -> MarkName {
        MarkName::new(c).unwrap()
    }

    /// Helper: build a populated BufferLocalState for testing.
    fn sample_state() -> BufferLocalState {
        let mut marks = BufferMarks::default();
        marks.local.insert(mn('a'), Mark::from_raw(10));
        marks.local.insert(mn('z'), Mark::from_raw(90));
        marks
            .special
            .insert(MarkName::LAST_CHANGE, Mark::from_raw(42));
        marks
            .special
            .insert(MarkName::VISUAL_START, Mark::from_raw(5));
        marks
            .special
            .insert(MarkName::VISUAL_END, Mark::from_raw(15));

        let mut changelist = ChangeList::default();
        changelist.push(Offset::new(10));
        changelist.push(Offset::new(20));
        changelist.push(Offset::new(30));

        BufferLocalState {
            marks,
            changelist,
            last_visual: Some(LastVisualInfo::char_wise(3)),
            sticky_column: Some(VirtualColumn::new(40)),
            buffer_overrides: OptionOverrides::default(),
            buffer_mappings: BufferMappings::default(),
            scroll_half_count: Some(5),
            undo_tree: UndoTree::default(),
            buffer_variables: BTreeMap::new(),
        }
    }

    #[test]
    fn buffer_local_state_default_is_empty() {
        let state = BufferLocalState::default();
        assert!(state.marks.local.is_empty());
        assert!(state.marks.special.is_empty());
        assert!(state.changelist.is_empty());
        assert!(state.last_visual.is_none());
        assert!(state.sticky_column.is_none());
        assert!(state.buffer_overrides.is_empty());
        assert!(state.buffer_mappings.is_empty());
        assert!(state.scroll_half_count.is_none());
        assert_eq!(state.undo_tree.change_count(), 0);
    }

    #[test]
    fn round_trip_via_enter_then_leave() {
        let mut engine = VimEngine::new();
        engine.marks_mut().set(mn('A'), Mark::from_raw(200)); // global

        // Install per-buffer state
        engine.on_buffer_enter(sample_state());

        // Verify state is active
        assert_eq!(
            engine.state().marks().get(mn('a')).unwrap().offset().get(),
            10
        );
        assert_eq!(
            engine.state().marks().get(mn('A')).unwrap().offset().get(),
            200
        );
        assert!(engine.state().last_visual().is_some());
        assert_eq!(engine.state().sticky_column().unwrap().get(), 40);
        assert_eq!(engine.state().scroll_half_count(), Some(5));

        // Extract per-buffer state
        let saved = engine.on_buffer_leave(100);

        // Engine is clean
        assert!(engine.state().marks().get(mn('a')).is_none());
        assert!(engine.state().changelist().is_empty());
        assert!(engine.state().last_visual().is_none());
        assert!(engine.state().sticky_column().is_none());
        assert!(engine.state().scroll_half_count().is_none());

        // Global marks survive
        assert_eq!(
            engine.state().marks().get(mn('A')).unwrap().offset().get(),
            200
        );

        // Last-position mark in saved state
        assert_eq!(
            saved.marks.special[&MarkName::LAST_POSITION].offset().get(),
            100
        );

        // Re-enter: everything restored
        engine.on_buffer_enter(saved);
        assert_eq!(
            engine.state().marks().get(mn('a')).unwrap().offset().get(),
            10
        );
        assert_eq!(
            engine.state().marks().get(mn('z')).unwrap().offset().get(),
            90
        );
        assert_eq!(
            engine.state().marks().get(mn('<')).unwrap().offset().get(),
            5
        );
        assert_eq!(
            engine.state().marks().get(mn('.')).unwrap().offset().get(),
            42
        );
        assert!(!engine.state().changelist().is_empty());
        assert!(engine.state().last_visual().is_some());
        assert_eq!(engine.state().sticky_column().unwrap().get(), 40);
        assert_eq!(engine.state().scroll_half_count(), Some(5));
        assert_eq!(
            engine.state().marks().get(mn('A')).unwrap().offset().get(),
            200
        );
    }

    #[test]
    fn two_buffer_simulation() {
        let mut engine = VimEngine::new();

        // Buffer A
        engine.on_buffer_enter(BufferLocalState::default());
        engine.marks_mut().set(mn('a'), Mark::from_raw(50));
        engine.changelist_mut().push(Offset::new(10));
        engine.changelist_mut().push(Offset::new(20));
        engine.changelist_mut().push(Offset::new(30));
        let saved_a = engine.on_buffer_leave(0);

        // Buffer B
        engine.on_buffer_enter(BufferLocalState::default());
        engine.marks_mut().set(mn('a'), Mark::from_raw(200));
        engine.changelist_mut().push(Offset::new(100));
        let saved_b = engine.on_buffer_leave(0);

        // Re-enter A
        engine.on_buffer_enter(saved_a);
        assert_eq!(
            engine.state().marks().get(mn('a')).unwrap().offset().get(),
            50
        );
        assert_eq!(engine.state().changelist().entries().len(), 3);

        // Leave A, re-enter B
        let _saved_a2 = engine.on_buffer_leave(0);
        engine.on_buffer_enter(saved_b);
        assert_eq!(
            engine.state().marks().get(mn('a')).unwrap().offset().get(),
            200
        );
        assert_eq!(engine.state().changelist().entries().len(), 1);
    }

    #[test]
    fn first_visit_default_no_panic() {
        let mut engine = VimEngine::new();
        engine.on_buffer_enter(BufferLocalState::default());

        assert!(engine.state().marks().get(mn('a')).is_none());
        assert!(engine.state().changelist().is_empty());
        assert!(engine.state().last_visual().is_none());
        assert!(engine.state().sticky_column().is_none());
    }

    #[test]
    fn global_marks_survive_round_trip() {
        let mut engine = VimEngine::new();
        engine.marks_mut().set(mn('A'), Mark::from_raw(100));
        engine.marks_mut().set(mn('B'), Mark::from_raw(200));
        engine.marks_mut().set(mn('C'), Mark::from_raw(300));

        let _saved = engine.on_buffer_leave(0);

        assert_eq!(
            engine.state().marks().get(mn('A')).unwrap().offset().get(),
            100
        );
        assert_eq!(
            engine.state().marks().get(mn('B')).unwrap().offset().get(),
            200
        );
        assert_eq!(
            engine.state().marks().get(mn('C')).unwrap().offset().get(),
            300
        );

        engine.on_buffer_enter(BufferLocalState::default());

        assert_eq!(
            engine.state().marks().get(mn('A')).unwrap().offset().get(),
            100
        );
        assert_eq!(
            engine.state().marks().get(mn('B')).unwrap().offset().get(),
            200
        );
        assert_eq!(
            engine.state().marks().get(mn('C')).unwrap().offset().get(),
            300
        );
    }

    #[test]
    fn sticky_column_preserved_across_switch() {
        let mut engine = VimEngine::new();
        let mut state = BufferLocalState::default();
        state.sticky_column = Some(VirtualColumn::new(40));
        engine.on_buffer_enter(state);

        let saved = engine.on_buffer_leave(0);
        assert!(engine.state().sticky_column().is_none());

        engine.on_buffer_enter(saved);
        assert_eq!(engine.state().sticky_column().unwrap().get(), 40);
    }

    #[test]
    fn scroll_half_count_preserved_across_switch() {
        let mut engine = VimEngine::new();
        let mut state = BufferLocalState::default();
        state.scroll_half_count = Some(5);
        engine.on_buffer_enter(state);

        let saved = engine.on_buffer_leave(0);
        assert!(engine.state().scroll_half_count().is_none());

        engine.on_buffer_enter(saved);
        assert_eq!(engine.state().scroll_half_count(), Some(5));
    }

    #[test]
    fn scroll_half_count_isolated_between_buffers() {
        let mut engine = VimEngine::new();

        // Buffer A: count = 5
        let mut state_a = BufferLocalState::default();
        state_a.scroll_half_count = Some(5);
        engine.on_buffer_enter(state_a);
        let saved_a = engine.on_buffer_leave(0);

        // Buffer B: count = 10
        let mut state_b = BufferLocalState::default();
        state_b.scroll_half_count = Some(10);
        engine.on_buffer_enter(state_b);
        let saved_b = engine.on_buffer_leave(0);

        // Re-enter A: should be 5, not 10
        engine.on_buffer_enter(saved_a);
        assert_eq!(engine.state().scroll_half_count(), Some(5));

        // Re-enter B: should be 10
        let _a2 = engine.on_buffer_leave(0);
        engine.on_buffer_enter(saved_b);
        assert_eq!(engine.state().scroll_half_count(), Some(10));
    }

    #[test]
    fn on_buffer_leave_sets_last_position_mark() {
        let mut engine = VimEngine::new();
        let saved = engine.on_buffer_leave(500);

        assert_eq!(
            saved.marks.special[&MarkName::LAST_POSITION].offset().get(),
            500
        );
    }

    /// Helper: add N undo groups to a tree.
    fn add_undo_groups(tree: &mut UndoTree, count: u32) {
        for i in 0..count {
            tree.begin_group(
                Offset::new(i as usize * 10),
                UndoCursorStrategy::FirstEdit,
                MarkSnapshot::new(),
                None,
                crate::primitives::Mode::Normal,
                None,
                false,
            );
            tree.mark_edit_at(Offset::new(i as usize * 10 + 1));
            tree.end_group(Offset::new(i as usize * 10 + 5), (i + 1) as u64 * 100, None);
        }
    }

    #[test]
    fn undo_tree_saved_on_buffer_leave() {
        let mut engine = VimEngine::new();

        // Add 3 undo groups to the engine's tree.
        add_undo_groups(engine.undo_tree_mut(), 3);
        assert_eq!(engine.undo_tree().change_count(), 3);

        // on_buffer_leave should extract the tree into BufferLocalState.
        let saved = engine.on_buffer_leave(0);
        assert_eq!(
            saved.undo_tree.change_count(),
            3,
            "saved BufferLocalState must contain the 3 undo groups"
        );

        // Engine's tree should now be a fresh default (empty).
        assert_eq!(
            engine.undo_tree().change_count(),
            0,
            "engine undo tree must be reset after on_buffer_leave"
        );
    }

    #[test]
    fn undo_tree_restored_on_buffer_enter() {
        let mut engine = VimEngine::new();

        // Build a BufferLocalState with a populated undo tree.
        let mut state = BufferLocalState::default();
        add_undo_groups(&mut state.undo_tree, 5);
        assert_eq!(state.undo_tree.change_count(), 5);

        // on_buffer_enter should install the tree.
        engine.on_buffer_enter(state);
        assert_eq!(
            engine.undo_tree().change_count(),
            5,
            "engine undo tree must match restored BufferLocalState"
        );
    }

    #[test]
    fn undo_tree_isolated_across_buffers() {
        let mut engine = VimEngine::new();

        // Buffer A: 3 undo groups.
        add_undo_groups(engine.undo_tree_mut(), 3);
        assert_eq!(engine.undo_tree().change_count(), 3);
        let saved_a = engine.on_buffer_leave(0);

        // Buffer B: 2 undo groups.
        engine.on_buffer_enter(BufferLocalState::default());
        add_undo_groups(engine.undo_tree_mut(), 2);
        assert_eq!(engine.undo_tree().change_count(), 2);
        let saved_b = engine.on_buffer_leave(0);

        // Re-enter A: should have 3 groups, not 5.
        engine.on_buffer_enter(saved_a);
        assert_eq!(
            engine.undo_tree().change_count(),
            3,
            "buffer A must have exactly 3 undo groups after round-trip"
        );

        // Re-enter B: should have 2 groups.
        let _a2 = engine.on_buffer_leave(0);
        engine.on_buffer_enter(saved_b);
        assert_eq!(
            engine.undo_tree().change_count(),
            2,
            "buffer B must have exactly 2 undo groups after round-trip"
        );
    }

    #[test]
    fn syntax_selection_cleared_on_buffer_leave() {
        use crate::effects::Effect;
        use crate::primitives::Offset;

        let mut engine = VimEngine::new();

        // Populate syntax selection history with buffer-specific offsets.
        engine.apply_effect(&Effect::SyntaxSelectionPush {
            snapshot: crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(Offset::new(10), Offset::new(50)),
            ),
        });
        engine.apply_effect(&Effect::SyntaxSelectionPush {
            snapshot: crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(Offset::new(20), Offset::new(80)),
            ),
        });
        assert!(!engine.state().syntax_selection().is_empty());

        // on_buffer_leave should clear syntax_selection.
        let _saved = engine.on_buffer_leave(0);
        assert!(
            engine.state().syntax_selection().is_empty(),
            "syntax_selection must be cleared on buffer leave — offsets are buffer-specific"
        );
    }
}

/// All per-buffer engine state bundled for save/restore during buffer switches.
///
/// Returned by [`super::VimEngine::on_buffer_leave`], accepted by
/// [`super::VimEngine::on_buffer_enter`]. Implements [`Default`] for
/// first-visit buffers (empty marks, empty changelist, etc.).
///
/// # Compile-time completeness
///
/// Adding a new field requires updating both `on_buffer_leave` (exhaustive
/// construction) and `on_buffer_enter` (exhaustive destructure), producing
/// compile errors until handled. The `buffer_local_state_field_inventory`
/// test in `vim_state.rs` provides a third layer of protection.
#[derive(Debug, Clone, Default)]
pub struct BufferLocalState {
    /// Local (a-z) and special (. ^ [ ] < > ' ` ") marks.
    pub marks: BufferMarks,
    /// Edit positions for g;/g, navigation.
    pub changelist: ChangeList,
    /// Visual mode info for gv reselection.
    pub last_visual: Option<LastVisualInfo>,
    /// Cursor column intent (Vim's curswant). Saved per-buffer —
    /// strictly superior to Neovim which loses this on buffer switch.
    pub sticky_column: Option<VirtualColumn>,
    /// `:setlocal` overrides for buffer-scoped options.
    pub buffer_overrides: OptionOverrides,
    /// Buffer-local key mappings.
    pub buffer_mappings: BufferMappings,
    /// Sticky half-page scroll count (Vim's `scroll` option).
    /// Once the user supplies an explicit count to Ctrl-D/Ctrl-U,
    /// that count persists for subsequent scrolls in this buffer.
    pub scroll_half_count: Option<u32>,
    /// Undo group metadata tree for `:earlier`/`:later`/`:undolist`/`:undotree`.
    /// Saved/restored per-buffer — undo history is buffer-specific.
    pub undo_tree: UndoTree,
    /// Buffer-local (b:) variables saved during buffer switch.
    pub buffer_variables: BTreeMap<CompactString, VimValue>,
}

impl BufferLocalState {
    /// Drop the buffer's local value of option `id`, so the buffer sees
    /// the global value again when it is entered.
    ///
    /// [`VimEngine::set_option`](super::VimEngine::set_option) drops the
    /// local value of the current buffer only, as `:set` does. A host that
    /// applies a setting to every buffer, such as an editor setting the
    /// user just changed, calls this on each state it saved from
    /// [`on_buffer_leave`](super::VimEngine::on_buffer_leave) as well, so
    /// an earlier `:set` in a buffer the user has left does not win over
    /// the newer setting.
    pub fn clear_local_option(&mut self, id: OptionId) {
        self.buffer_overrides.remove(id);
    }
}
