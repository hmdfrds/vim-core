//! Undo tree navigation for `:earlier`/`:later`/`:undolist`.
//!
//! Handles the engine-level undo tree mutation that the executor cannot
//! perform (executor has read-only state access). The executor signals
//! navigation intent via [`UndoNavigation`], and the engine applies it
//! here with full `&mut self` access.

use crate::commands::ex::effects as ex_effects;
use crate::effects::Effects;
use crate::execution::executor_ex::UndoNavigation;
use compact_str::CompactString;
use std::fmt::Write;

impl super::VimEngine {
    /// Execute an undo tree navigation request from the executor.
    ///
    /// Navigates the undo tree, emits the corresponding Undo/Redo effects
    /// for the host to execute, and sets the cursor to the tree's suggested
    /// position.
    pub(super) fn apply_undo_navigation(&mut self, nav: &UndoNavigation) -> Effects {
        debug!(target: "vim::engine::undo", "undo navigation: {nav:?}");
        match nav {
            UndoNavigation::Earlier(amount) => self.navigate_earlier(*amount),
            UndoNavigation::Later(amount) => self.navigate_later(*amount),
            UndoNavigation::UndoList => self.format_undolist(),
            UndoNavigation::UndoTree => self.emit_undo_tree_snapshot(),
            UndoNavigation::GotoSequence(seq) => self.navigate_to_sequence(*seq),
        }
    }

    fn emit_undo_tree_snapshot(&self) -> Effects {
        let snapshot = self.state.undo_tree().snapshot();
        Effects::new().undo_tree_snapshot(snapshot)
    }

    fn navigate_earlier(&self, amount: crate::grammar::types::TimeAmount) -> Effects {
        let result = if let Some(count) = amount.as_changes() {
            self.state.undo_tree().earlier_by_count(count)
        } else if let Some(saves) = amount.as_file_saves() {
            self.state.undo_tree().earlier_by_saves(saves as usize)
        } else {
            let seconds = amount.to_seconds().unwrap_or(0);
            let current_time = self.state.undo_timestamp_hint();
            self.state
                .undo_tree()
                .earlier_by_time(seconds, current_time)
        };

        match result {
            Some((undo_count, cursor)) => {
                let msg = format_navigation_message(undo_count, true);
                Effects::new()
                    .undo(undo_count)
                    .set_cursor(cursor)
                    .show_message(msg)
            }
            None => ex_effects::show_message(CompactString::from("Already at oldest change")),
        }
    }

    fn navigate_later(&self, amount: crate::grammar::types::TimeAmount) -> Effects {
        let result = if let Some(count) = amount.as_changes() {
            self.state.undo_tree().later_by_count(count)
        } else if let Some(saves) = amount.as_file_saves() {
            self.state.undo_tree().later_by_saves(saves as usize)
        } else {
            let seconds = amount.to_seconds().unwrap_or(0);
            self.state.undo_tree().later_by_time(seconds)
        };

        match result {
            Some((redo_count, cursor)) => {
                let msg = format_navigation_message(redo_count, false);
                Effects::new()
                    .redo(redo_count)
                    .set_cursor(cursor)
                    .show_message(msg)
            }
            None => ex_effects::show_message(CompactString::from("Already at newest change")),
        }
    }

    fn navigate_to_sequence(&mut self, seq: u64) -> Effects {
        match self.state.undo_tree_mut().goto_sequence(seq) {
            Some((0, 0, _cursor)) => {
                ex_effects::show_message(CompactString::from("Already at that change"))
            }
            Some((undo_count, redo_count, cursor)) => {
                let mut effects = Effects::new();
                if undo_count > 0 {
                    effects = effects.undo(undo_count);
                }
                if redo_count > 0 {
                    effects = effects.redo(redo_count);
                }
                effects = effects.set_cursor(cursor);
                let msg = format!("undo #{seq}");
                effects.show_message(CompactString::from(msg))
            }
            None => ex_effects::show_message(CompactString::from(format!(
                "E830: Undo number {seq} not found"
            ))),
        }
    }

    fn format_undolist(&self) -> Effects {
        if self.state.undo_tree().change_count() == 0 {
            return ex_effects::show_message(CompactString::from("Nothing to undo"));
        }
        let leaves = self.state.undo_tree().leaves();

        let mut out = String::from("number  changes  time\n");
        for leaf in &leaves {
            let _ = writeln!(
                out,
                "{:>6}  {:>7}  {}",
                leaf.node(),
                leaf.depth(),
                format_timestamp(leaf.timestamp()),
            );
        }
        ex_effects::show_message(CompactString::from(out))
    }
}

fn format_navigation_message(count: u32, is_earlier: bool) -> CompactString {
    let direction = if is_earlier { "before" } else { "after" };
    let plural = if count == 1 { "change" } else { "changes" };
    CompactString::from(format!("{count} {plural}; {direction}"))
}

fn format_timestamp(secs: u64) -> String {
    if secs == 0 {
        return "00:00:00".to_owned();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use crate::effects::Effect;
    use crate::execution::VimEngine;
    use crate::primitives::Offset;
    use crate::state::mark_snapshot::MarkSnapshot;
    use crate::state::{Marks, UndoTree};

    /// Helper: commit N undo groups with sequential cursors and timestamps.
    fn build_tree_with_groups(tree: &mut UndoTree, count: u32) {
        for i in 0..count {
            tree.begin_group(
                Offset::new(i as usize * 10),
                crate::primitives::UndoCursorStrategy::FirstEdit,
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

    /// Undo N times on the tree.
    fn undo_n(tree: &mut UndoTree, marks: &mut Marks, n: u32) {
        let mut lv = None;
        for _ in 0..n {
            if tree.undo(marks, &mut lv).is_none() {
                break;
            }
        }
    }

    #[test]
    fn earlier_by_count_emits_undo_and_cursor() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 5);

        let nav = super::UndoNavigation::Earlier(crate::grammar::types::TimeAmount::Changes(3));
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::Undo { count: 3, .. })));
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })));
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("3 changes"))));
    }

    #[test]
    fn later_by_count_emits_redo_and_cursor() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 5);

        // Undo all 5 (earlier_by_count is now peek-only, so use undo() directly)
        let mut marks = Marks::new();
        undo_n(engine.undo_tree_mut(), &mut marks, 5);

        let nav = super::UndoNavigation::Later(crate::grammar::types::TimeAmount::Changes(2));
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::Redo { count: 2, .. })));
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })));
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("2 changes"))));
    }

    #[test]
    fn earlier_at_root_shows_already_oldest() {
        let mut engine = VimEngine::new();

        let nav = super::UndoNavigation::Earlier(crate::grammar::types::TimeAmount::Changes(1));
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("oldest"))));
        assert!(!effects_vec.iter().any(|e| matches!(e, Effect::Undo { .. })));
    }

    #[test]
    fn later_at_leaf_shows_already_newest() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 3);

        let nav = super::UndoNavigation::Later(crate::grammar::types::TimeAmount::Changes(1));
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("newest"))));
        assert!(!effects_vec.iter().any(|e| matches!(e, Effect::Redo { .. })));
    }

    #[test]
    fn earlier_by_time_navigates_correctly() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 3);

        // Set current time to 300; go earlier by 150s (target: t=150, should reach t=100)
        engine.set_undo_timestamp(300);
        let nav = super::UndoNavigation::Earlier(crate::grammar::types::TimeAmount::Seconds(150));
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::Undo { count, .. } if *count == 2)));
    }

    #[test]
    fn later_by_time_navigates_correctly() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 3);

        // Undo all 3 (earlier_by_count is now peek-only, so use undo() directly)
        let mut marks = Marks::new();
        undo_n(engine.undo_tree_mut(), &mut marks, 3);
        // Current time 0 (at root), later by 250s → target t=250 → reach t=200
        engine.set_undo_timestamp(0);
        let nav = super::UndoNavigation::Later(crate::grammar::types::TimeAmount::Seconds(250));
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::Redo { count, .. } if *count == 2)));
    }

    #[test]
    fn undolist_formats_leaves() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 3);

        let nav = super::UndoNavigation::UndoList;
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        let msg = effects_vec.iter().find_map(|e| match e {
            Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
            } => Some(text.as_str()),
            _ => None,
        });
        let msg = msg.expect("should have ShowInfo");
        assert!(msg.contains("number"));
        assert!(msg.contains("changes"));
        // Should have exactly 1 leaf (linear chain: only the last node is a leaf)
        let lines: Vec<_> = msg.lines().filter(|l| !l.starts_with("number")).collect();
        assert_eq!(lines.len(), 1, "linear chain has 1 leaf");
    }

    #[test]
    fn undolist_with_branches() {
        let mut engine = VimEngine::new();

        // Commit 3 groups linearly
        build_tree_with_groups(engine.undo_tree_mut(), 3);
        // Undo 2 (earlier_by_count is peek-only, use undo() directly)
        let mut marks = Marks::new();
        undo_n(engine.undo_tree_mut(), &mut marks, 2);
        // Fork a new branch with 1 group
        let tree = engine.undo_tree_mut();
        tree.begin_group(
            Offset::new(100),
            crate::primitives::UndoCursorStrategy::FirstEdit,
            MarkSnapshot::new(),
            None,
            crate::primitives::Mode::Normal,
            None,
            false,
        );
        tree.mark_edit_at(Offset::new(101));
        tree.end_group(Offset::new(105), 400, None);

        let nav = super::UndoNavigation::UndoList;
        let effects = engine.apply_undo_navigation(&nav);

        let msg = effects.iter().find_map(|e| match e {
            Effect::ShowInfo {
                info: crate::effects::InfoMessage::Text(text),
            } => Some(text.as_str()),
            _ => None,
        });
        let msg = msg.expect("should have ShowInfo");
        // Should have 2 leaves: node 3 (old branch tip) and node 4 (new branch tip)
        let data_lines: Vec<_> = msg.lines().filter(|l| !l.starts_with("number")).collect();
        assert_eq!(data_lines.len(), 2, "branched tree has 2 leaves");
    }

    #[test]
    fn undolist_empty_tree_shows_nothing_to_undo() {
        let mut engine = VimEngine::new();

        let nav = super::UndoNavigation::UndoList;
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec.iter().any(
            |e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("Nothing to undo"))
        ));
    }

    #[test]
    fn earlier_single_shows_singular_change() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 3);

        let nav = super::UndoNavigation::Earlier(crate::grammar::types::TimeAmount::Changes(1));
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("1 change;"))));
    }

    #[test]
    fn earlier_clamps_to_available_undo_count() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 3);

        // Request 10 earlier steps, only 3 available
        let nav = super::UndoNavigation::Earlier(crate::grammar::types::TimeAmount::Changes(10));
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::Undo { count: 3, .. })));
        assert!(effects_vec
            .iter()
            .any(|e| matches!(e, Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.contains("3 changes"))));
    }

    // ─── :undotree snapshot tests ──────────────────────────────────

    #[test]
    fn undotree_emits_snapshot_effect() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 3);

        let nav = super::UndoNavigation::UndoTree;
        let effects = engine.apply_undo_navigation(&nav);

        let effects_vec: Vec<_> = effects.iter().collect();
        let has_snapshot = effects_vec
            .iter()
            .any(|e| matches!(e, Effect::UndoTreeSnapshot { .. }));
        assert!(
            has_snapshot,
            "UndoTree navigation should emit UndoTreeSnapshot effect"
        );
    }

    #[test]
    fn undotree_snapshot_has_correct_node_count() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 5);

        let nav = super::UndoNavigation::UndoTree;
        let effects = engine.apply_undo_navigation(&nav);

        let snapshot = effects
            .iter()
            .find_map(|e| match e {
                Effect::UndoTreeSnapshot { snapshot } => Some(snapshot),
                _ => None,
            })
            .expect("should have UndoTreeSnapshot effect");

        assert_eq!(snapshot.nodes.len(), 6, "root + 5 change groups");
        assert_eq!(snapshot.change_count, 5);
    }

    #[test]
    fn undotree_snapshot_empty_tree() {
        let mut engine = VimEngine::new();

        let nav = super::UndoNavigation::UndoTree;
        let effects = engine.apply_undo_navigation(&nav);

        let snapshot = effects
            .iter()
            .find_map(|e| match e {
                Effect::UndoTreeSnapshot { snapshot } => Some(snapshot),
                _ => None,
            })
            .expect("should have UndoTreeSnapshot even for empty tree");

        assert_eq!(snapshot.nodes.len(), 1, "empty tree has only root");
        assert_eq!(snapshot.change_count, 0);
        assert!(snapshot.nodes[0].is_current);
    }

    #[test]
    fn undotree_snapshot_with_branches() {
        let mut engine = VimEngine::new();
        build_tree_with_groups(engine.undo_tree_mut(), 3);

        // Undo 2 and fork
        let mut marks = Marks::new();
        undo_n(engine.undo_tree_mut(), &mut marks, 2);
        let tree = engine.undo_tree_mut();
        tree.begin_group(
            Offset::new(100),
            crate::primitives::UndoCursorStrategy::FirstEdit,
            MarkSnapshot::new(),
            None,
            crate::primitives::Mode::Normal,
            None,
            false,
        );
        tree.mark_edit_at(Offset::new(101));
        tree.end_group(Offset::new(105), 400, None);

        let nav = super::UndoNavigation::UndoTree;
        let effects = engine.apply_undo_navigation(&nav);

        let snapshot = effects
            .iter()
            .find_map(|e| match e {
                Effect::UndoTreeSnapshot { snapshot } => Some(snapshot),
                _ => None,
            })
            .expect("should have UndoTreeSnapshot");

        assert_eq!(snapshot.nodes.len(), 5, "root + 3 + 1 branch");
        assert_eq!(snapshot.change_count, 4);

        // Node 1 (A) should have 2 children: B(2) and the branch(4)
        let a_view = &snapshot.nodes[1];
        assert_eq!(a_view.children.len(), 2);

        // Current should be the branch node (4)
        assert!(snapshot.nodes[4].is_current);
        let current_count = snapshot.nodes.iter().filter(|n| n.is_current).count();
        assert_eq!(current_count, 1, "exactly one node should be current");
    }
}
