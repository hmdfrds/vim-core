//! Fold provider trait for shell integration.
//!
//! Shells that support code folding implement this trait to let the engine
//! skip folded lines during vertical navigation.

use crate::primitives::{Direction, LineNumber};

/// Provider for fold-aware navigation.
///
/// Shells implement this to expose their folding state to the engine.
/// The engine uses this for fold-aware `j`/`k` motions and `zj`/`zk`.
///
/// If no `FoldProvider` is supplied, the engine treats all lines as visible.
///
/// # External implementors
///
/// External crates (e.g. `godot-vim`'s `GodotFoldProvider`) will need to
/// update their signatures to use `LineNumber` instead of bare `usize`.
pub trait FoldProvider: Send {
    /// Return the next visible line in the given direction.
    ///
    /// If `line` is visible, return it unchanged.
    /// If `line` is folded, skip to the next visible line in `direction`.
    fn next_visible_line(&self, line: LineNumber, direction: Direction) -> LineNumber;

    /// Check if a line is currently folded (hidden).
    fn is_folded(&self, line: LineNumber) -> bool;

    /// Return the enclosing closed fold range for a line, if any.
    ///
    /// When `line` is inside a closed fold, returns `Some((fold_start, fold_end))`
    /// where both bounds are inclusive line numbers of the hidden region.
    /// Returns `None` if the line is not folded.
    ///
    /// Default implementation derives the range from `next_visible_line`:
    /// - fold_start = next_visible_line(line, Backward) + 1
    /// - fold_end   = next_visible_line(line, Forward) - 1
    ///
    /// Implementors with direct access to fold ranges (e.g. `HostFoldProvider`)
    /// may override for O(log n) lookup.
    fn enclosing_fold(&self, line: LineNumber) -> Option<(LineNumber, LineNumber)> {
        if !self.is_folded(line) {
            return None;
        }
        // next_visible_line(line, Backward) returns the visible line BEFORE the fold.
        // The fold starts one line after that.
        let before = self.next_visible_line(line, Direction::Backward);
        let fold_start = if self.is_folded(before) {
            // The "visible" line is itself folded — fold extends to line 0
            // (next_visible_line saturated at 0).
            before
        } else {
            before.next()
        };
        // next_visible_line(line, Forward) returns the visible line AFTER the fold.
        // The fold ends one line before that.
        let after = self.next_visible_line(line, Direction::Forward);
        let fold_end = after.prev();
        Some((fold_start, fold_end))
    }
}
