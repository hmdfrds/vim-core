use super::*;

#[test]
fn empty_tracker() {
    let mut tracker = DirtyTracker::new();
    assert!(tracker.is_empty());
    let info = tracker.take();
    assert!(info.lines().is_empty());
    assert!(!info.full_redraw());
    assert!(info.dirty_flags().is_empty());
}

#[test]
fn record_lines() {
    let mut tracker = DirtyTracker::new();
    tracker.record_lines(&[5, 6, 7]);
    assert!(!tracker.is_empty());
    let info = tracker.take();
    assert_eq!(&*info.lines(), &[5, 6, 7]);
    assert!(!info.full_redraw());
    assert!(info.dirty_flags().contains(DirtyFlags::LINES));
}

#[test]
fn record_full_redraw() {
    let mut tracker = DirtyTracker::new();
    tracker.record_lines(&[3]);
    tracker.record_full_redraw();
    let info = tracker.take();
    assert!(info.full_redraw());
}

#[test]
fn take_resets() {
    let mut tracker = DirtyTracker::new();
    tracker.record_lines(&[1, 2]);
    tracker.record_full_redraw();
    tracker.set_flags(DirtyFlags::MARKS | DirtyFlags::SEARCH_HIGHLIGHTS);
    let _ = tracker.take();
    assert!(tracker.is_empty());
    let info = tracker.take();
    assert!(info.lines().is_empty());
    assert!(!info.full_redraw());
    assert!(info.dirty_flags().is_empty());
}

// ── DirtyFlags tests ──────────────────────────────────────────────────────

#[test]
fn flags_record_lines_sets_lines_flag() {
    let mut tracker = DirtyTracker::new();
    tracker.record_lines(&[10]);
    assert!(tracker.dirty_flags().contains(DirtyFlags::LINES));
    // Other flags should NOT be set.
    assert!(!tracker.dirty_flags().contains(DirtyFlags::MARKS));
    assert!(!tracker
        .dirty_flags()
        .contains(DirtyFlags::SEARCH_HIGHLIGHTS));
}

#[test]
fn flags_record_line_range_sets_lines_flag() {
    let mut tracker = DirtyTracker::new();
    tracker.record_line_range(0..=5);
    assert!(tracker.dirty_flags().contains(DirtyFlags::LINES));
}

#[test]
fn flags_set_marks() {
    let mut tracker = DirtyTracker::new();
    tracker.set_flags(DirtyFlags::MARKS);
    assert!(!tracker.is_empty());
    assert!(tracker.dirty_flags().contains(DirtyFlags::MARKS));
    assert!(!tracker.dirty_flags().contains(DirtyFlags::LINES));
}

#[test]
fn flags_set_search_highlights() {
    let mut tracker = DirtyTracker::new();
    tracker.set_flags(DirtyFlags::SEARCH_HIGHLIGHTS);
    assert!(tracker
        .dirty_flags()
        .contains(DirtyFlags::SEARCH_HIGHLIGHTS));
}

#[test]
fn flags_are_independent() {
    let mut tracker = DirtyTracker::new();
    tracker.set_flags(DirtyFlags::MARKS);
    tracker.set_flags(DirtyFlags::FOLDS);
    tracker.set_flags(DirtyFlags::VIEWPORT);
    let flags = tracker.dirty_flags();
    assert!(flags.contains(DirtyFlags::MARKS));
    assert!(flags.contains(DirtyFlags::FOLDS));
    assert!(flags.contains(DirtyFlags::VIEWPORT));
    assert!(!flags.contains(DirtyFlags::LINES));
    assert!(!flags.contains(DirtyFlags::SEARCH_HIGHLIGHTS));
    assert!(!flags.contains(DirtyFlags::CURSOR_VALID));
    assert!(!flags.contains(DirtyFlags::STATUS));
    assert!(!flags.contains(DirtyFlags::SIGNS));
}

#[test]
fn flags_combined_with_lines() {
    let mut tracker = DirtyTracker::new();
    tracker.record_lines(&[5]);
    tracker.set_flags(DirtyFlags::MARKS);
    let info = tracker.take();
    assert_eq!(&*info.lines(), &[5]);
    assert!(info.dirty_flags().contains(DirtyFlags::LINES));
    assert!(info.dirty_flags().contains(DirtyFlags::MARKS));
}

#[test]
fn flags_drain_resets() {
    let mut tracker = DirtyTracker::new();
    tracker.set_flags(DirtyFlags::STATUS | DirtyFlags::SIGNS);
    let info = tracker.take();
    assert!(info.dirty_flags().contains(DirtyFlags::STATUS));
    assert!(info.dirty_flags().contains(DirtyFlags::SIGNS));
    // After take, flags should be empty.
    assert!(tracker.dirty_flags().is_empty());
    assert!(tracker.is_empty());
}

#[test]
fn flags_all_variants_constructable() {
    let all = [
        DirtyFlags::LINES,
        DirtyFlags::MARKS,
        DirtyFlags::SEARCH_HIGHLIGHTS,
        DirtyFlags::FOLDS,
        DirtyFlags::CURSOR_VALID,
        DirtyFlags::VIEWPORT,
        DirtyFlags::STATUS,
        DirtyFlags::SIGNS,
    ];
    for flag in &all {
        let mut tracker = DirtyTracker::new();
        tracker.set_flags(*flag);
        assert!(tracker.dirty_flags().contains(*flag));
    }
}
