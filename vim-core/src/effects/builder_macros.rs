//! Declarative macro for generating simple `Effects` builder methods.
//!
//! Every method follows the same pattern:
//! ```ignore
//! pub fn method_name(mut self, param: Type) -> Self {
//!     self.0.push(Effect::Variant { param });
//!     self
//! }
//! ```
//!
//! Methods with custom logic (conditional pushes, `impl Into<CompactString>`,
//! hardcoded field values, etc.) remain handwritten in `effects.rs`.

/// Generate builder methods on [`super::Effects<S>`] from a declarative table.
///
/// Each entry maps a method name and its parameters to an `Effect` expression.
/// The macro emits `#[inline]` + `#[must_use]` on every generated method.
/// Methods are generic over all undo states.
macro_rules! define_effect_builders {
    ($(
        $(#[doc = $doc:expr])*
        fn $name:ident( $($param:ident : $ty:ty),* ) => $effect:expr
    );* $(;)?) => {
        impl<S: super::undo_state::EffectState> super::Effects<S> {
            $(
                $(#[doc = $doc])*
                #[inline]
                pub fn $name(mut self, $($param: $ty),*) -> Self {
                    self.inner.push($effect);
                    self
                }
            )*
        }
    };
}

use crate::effects::effect::{HighlightStyle, SelectionTag};
use crate::effects::Effect;
use crate::primitives::{
    CursorStyle, LineNumber, LineRange, MarkName, Mode, ModeAppearance, MotionType, Offset, Range,
    RegisterName, SelectionRange, SelectionShape,
};
use smallvec::SmallVec;

define_effect_builders! {
    // ═══════════════════════════════════════════════════════════════════
    // Cursor Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Move cursor to byte offset.
    fn set_cursor(offset: Offset) => Effect::SetCursor { offset };

    /// Set visual selection by anchor and head.
    ///
    /// **Note:** For visual/select mode transitions, prefer
    /// [`Effects::set_visual_selection`](crate::effects::Effects::set_visual_selection) which automatically pairs
    /// `SetSelection` with `SetCursor(head)` — satisfying the shell
    /// dispatch pairing contract.
    fn set_selection(anchor: Offset, head: Offset, shape: SelectionShape) =>
        Effect::SetSelection { anchor, head, shape };

    /// Clear visual selection.
    fn clear_selection() => Effect::ClearSelection;

    /// Save visual selection dimensions for dot repeat.
    fn save_last_visual(info: crate::primitives::LastVisualInfo) =>
        Effect::SaveLastVisual { info };

    // ═══════════════════════════════════════════════════════════════════
    // Mode Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Change the editing mode.
    fn set_mode(mode: Mode) => Effect::SetMode { mode, appearance: ModeAppearance::for_mode(mode) };

    /// Set block insert context for replicating typed text across lines.
    fn set_block_insert(lines_below: usize, grapheme_col: usize, cursor_return_offset: Offset) =>
        Effect::SetBlockInsert { lines_below, grapheme_col, cursor_return_offset };

    // ═══════════════════════════════════════════════════════════════════
    // Mark & Jump Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Set a mark at byte offset with optional relative topline offset.
    fn set_mark(name: MarkName, offset: Offset, topline_offset: Option<i32>) => Effect::SetMark { name, offset, topline_offset };

    /// Delete a named mark (`:delmarks`).
    fn clear_mark(mark: MarkName) => Effect::ClearMark { mark };

    /// Apply operator from cursor to mark position (`d'a`, `y'b`).
    fn operator_to_mark(operator: crate::primitives::Operator, mark: MarkName, linewise: bool, register: Option<RegisterName>, cursor: Offset) =>
        Effect::OperatorToMark { operator, mark, linewise, register, cursor };

    /// Push current position to jump list.
    fn push_jump_list(offset: Offset) => Effect::PushJumpList { offset };

    /// Jump to older position in jump list (`Ctrl-O`).
    fn jump_older(count: u32) => Effect::JumpOlder { count };

    /// Jump to newer position in jump list (`Ctrl-I`).
    fn jump_newer(count: u32) => Effect::JumpNewer { count };

    /// Navigate to a position in a different buffer via jump list.
    fn jump_to_buffer(buffer_id: crate::primitives::BufferId, offset: Offset) =>
        Effect::JumpToBuffer { buffer_id, offset };

    /// Jump to older change position in changelist (`g;`).
    fn changelist_older(count: u32) => Effect::ChangelistOlder { count };

    /// Jump to newer change position in changelist (`g,`).
    fn changelist_newer(count: u32) => Effect::ChangelistNewer { count };

    // ═══════════════════════════════════════════════════════════════════
    // Undo Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Request undo (`u`).
    fn undo(count: u32) => Effect::Undo { count, steps: vec![] };

    /// Request line-local undo (`U`).
    fn undo_line(count: u32) => Effect::UndoLine { count };

    /// Request redo (`Ctrl-R`).
    fn redo(count: u32) => Effect::Redo { count, steps: vec![] };

    // ═══════════════════════════════════════════════════════════════════
    // Search Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Highlight search match ranges.
    fn highlight_matches(ranges: Vec<Range>) => Effect::HighlightMatches { ranges };

    /// Clear search highlights (`:noh`).
    fn clear_highlights() => Effect::ClearHighlights;

    /// Set last find command for `;` and `,` repeat (`f`, `F`, `t`, `T`).
    fn set_last_find(direction: crate::primitives::FindDirection, target_char: char) =>
        Effect::SetLastFind { direction, target_char, sneak_c2: None, resolved_ignorecase: false, resolved_smartcase: false };

    // ═══════════════════════════════════════════════════════════════════
    // Compound Effects (handled by orchestrator)
    // ═══════════════════════════════════════════════════════════════════

    /// Request host-driven operator filter (`!{motion}`).
    fn operator_filter(range: Range, motion_type: MotionType, register: Option<RegisterName>) =>
        Effect::OperatorFilter { range, motion_type, register };

    /// Request host-driven reindentation (`={motion}`).
    fn operator_reindent(range: Range, motion_type: MotionType, start_col: usize, end_col: usize, end_line_in_range: usize, start_byte_offset: usize) =>
        Effect::OperatorReindent { range, motion_type, start_col, end_col, end_line_in_range, start_byte_offset };

    // ═══════════════════════════════════════════════════════════════════
    // UI Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Show an error with Vim error code (no source context).
    fn show_error(error: crate::errors::VimError) => Effect::ShowError { error, source: None };

    /// Clear the message area.
    fn clear_message() => Effect::ClearMessage;

    /// Scroll to bring byte offset into view.
    fn scroll_to(offset: Offset) => Effect::ScrollTo { offset };

    /// Center cursor in viewport (`zz`).
    fn center_cursor() => Effect::CenterCursor;

    /// Scroll cursor to top of viewport (`zt`).
    fn cursor_to_top() => Effect::CursorToTop;

    /// Scroll cursor to bottom of viewport (`zb`).
    fn cursor_to_bottom() => Effect::CursorToBottom;

    /// Scroll viewport left by `count` columns (`zh`).
    fn scroll_left(count: u32) => Effect::ScrollLeft { count };

    /// Scroll viewport right by `count` columns (`zl`).
    fn scroll_right(count: u32) => Effect::ScrollRight { count };

    /// Scroll viewport left by half screen width (`zH`).
    fn scroll_half_screen_left(count: u32) => Effect::ScrollHalfScreenLeft { count };

    /// Scroll viewport right by half screen width (`zL`).
    fn scroll_half_screen_right(count: u32) => Effect::ScrollHalfScreenRight { count };

    /// Scroll so cursor is at left edge (`zs`).
    fn scroll_cursor_to_left_edge() => Effect::ScrollCursorToLeftEdge;

    /// Scroll so cursor is at right edge (`ze`).
    fn scroll_cursor_to_right_edge() => Effect::ScrollCursorToRightEdge;

    // ═══════════════════════════════════════════════════════════════════
    // Fold Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Fold (close) the region at the given line (`zc`).
    fn fold_line(line: LineNumber) => Effect::FoldLine { line };

    /// Unfold (open) the fold at the given line (`zo`).
    fn unfold_line(line: LineNumber) => Effect::UnfoldLine { line };

    /// Toggle the fold at the given line (`za`).
    fn toggle_fold(line: LineNumber) => Effect::ToggleFold { line };

    /// Recursively toggle folds at the given line (`zA`).
    fn toggle_fold_recursive(line: LineNumber) => Effect::ToggleFoldRecursive { line };

    /// Fold all foldable regions in the document (`zM`).
    fn fold_all() => Effect::FoldAll;

    /// Unfold all folds in the document (`zR`).
    fn unfold_all() => Effect::UnfoldAll;

    /// Recursively close all folds at cursor line (`zC`).
    fn fold_line_recursive(line: LineNumber) => Effect::FoldLineRecursive { line };

    /// Recursively open all folds at cursor line (`zO`).
    fn unfold_line_recursive(line: LineNumber) => Effect::UnfoldLineRecursive { line };

    /// Delete fold at cursor (`zd`).
    fn delete_fold(line: LineNumber) => Effect::DeleteFold { line };

    /// Recursively delete all folds at cursor (`zD`).
    fn delete_fold_recursive(line: LineNumber) => Effect::DeleteFoldRecursive { line };

    /// Eliminate all folds in document (`zE`).
    fn eliminate_all_folds() => Effect::EliminateAllFolds;

    /// Toggle foldenable option (`zi`).
    fn toggle_fold_enable() => Effect::ToggleFoldEnable;

    /// Set foldenable to specific value (`zn` = false, `zN` = true).
    fn set_fold_enable(enabled: bool) => Effect::SetFoldEnable { enabled };

    /// Open a command-line history window (`q:`, `q/`, `q?`).
    fn open_command_window(prompt: crate::primitives::CommandLinePrompt, history: Vec<compact_str::CompactString>, prefill: Option<compact_str::CompactString>) =>
        Effect::OpenCommandWindow { prompt, history, prefill };

    // ═══════════════════════════════════════════════════════════════════
    // Extension Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Call host's operatorfunc on a range (`g@{motion}`).
    fn call_operator_func(range: Range, motion_type: MotionType) =>
        Effect::CallOperatorFunc { range, motion_type };

    /// Push selections snapshot onto syntax selection history (engine-internal, consumed by effect processor).
    fn syntax_selection_push(snapshot: crate::primitives::Selections) =>
        Effect::SyntaxSelectionPush { snapshot };

    /// Pop from syntax selection history (engine-internal, consumed by effect processor).
    fn syntax_selection_pop() => Effect::SyntaxSelectionPop;

    /// Clear syntax selection history (engine-internal, consumed by effect processor).
    fn syntax_history_clear() => Effect::SyntaxHistoryClear;

    /// Set syntax selections (engine-internal, consumed by effect processor).
    fn set_syntax_selections(selections: crate::primitives::Selections) =>
        Effect::SetSyntaxSelections { selections };

    // ═══════════════════════════════════════════════════════════════════
    // Window Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Split window horizontally (`Ctrl-W s`, `:split`).
    fn window_split() => Effect::WindowSplit;

    /// Open a new empty buffer in a split window (`Ctrl-W n`, `:new`).
    fn window_new() => Effect::WindowNew;

    /// Split window vertically (`Ctrl-W v`, `:vsplit`).
    fn window_vsplit() => Effect::WindowVSplit;

    /// Close current window (`Ctrl-W c`, `:close`).
    fn window_close() => Effect::WindowClose;

    /// Close all windows except current (`Ctrl-W o`, `:only`).
    fn window_only() => Effect::WindowOnly;

    /// Move to next window (`Ctrl-W w`).
    fn window_next() => Effect::WindowNext;

    /// Move to previous window (`Ctrl-W W`).
    fn window_prev() => Effect::WindowPrev;

    /// Move cursor to left window (`Ctrl-W h`).
    fn window_move_left() => Effect::WindowMoveLeft;

    /// Move cursor to right window (`Ctrl-W l`).
    fn window_move_right() => Effect::WindowMoveRight;

    /// Move cursor to window above (`Ctrl-W k`).
    fn window_move_up() => Effect::WindowMoveUp;

    /// Move cursor to window below (`Ctrl-W j`).
    fn window_move_down() => Effect::WindowMoveDown;

    /// Equalize all window sizes (`Ctrl-W =`).
    fn window_equal_size() => Effect::WindowEqualSize;

    /// Increase window height by count (`Ctrl-W +`).
    fn window_increase_height(count: u32) => Effect::WindowIncreaseHeight { count };

    /// Decrease window height by count (`Ctrl-W -`).
    fn window_decrease_height(count: u32) => Effect::WindowDecreaseHeight { count };

    /// Increase window width by count (`Ctrl-W >`).
    fn window_increase_width(count: u32) => Effect::WindowIncreaseWidth { count };

    /// Decrease window width by count (`Ctrl-W <`).
    fn window_decrease_width(count: u32) => Effect::WindowDecreaseWidth { count };

    /// Rotate windows downward/rightward (`Ctrl-W r`).
    fn window_rotate_down() => Effect::WindowRotateDown;

    /// Rotate windows upward/leftward (`Ctrl-W R`).
    fn window_rotate_up() => Effect::WindowRotateUp;

    // ═══════════════════════════════════════════════════════════════════
    // Event Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Emit a typed Vim event.
    fn event(kind: crate::primitives::VimEvent) => Effect::Event { kind };

    // ═══════════════════════════════════════════════════════════════════
    // Macro Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Start recording a macro to a register (`q{reg}`).
    fn start_recording(register: RegisterName) => Effect::StartRecording { register };

    /// Stop recording the current macro (`q`).
    fn stop_recording() => Effect::StopRecording;

    /// Play a macro from a register (`@{reg}`).
    fn play_macro(register: RegisterName, count: u32) => Effect::PlayMacro { register, count };

    // ═══════════════════════════════════════════════════════════════════
    // Command-Line Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Apply an edit to the command-line input.
    fn command_line_edit(edit: crate::primitives::CommandLineEdit) =>
        Effect::CommandLineEdit(edit);

    // ═══════════════════════════════════════════════════════════════════
    // LSP Navigation Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Go to definition of symbol under cursor (`gd`).
    fn goto_definition() => Effect::GotoDefinition;

    /// Show documentation for symbol under cursor (`K`).
    fn show_documentation() => Effect::ShowDocumentation;

    // ═══════════════════════════════════════════════════════════════════
    // Cursor Style Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Set the recommended cursor shape and blink style for the current mode.
    fn set_cursor_style(style: CursorStyle) => Effect::SetCursorStyle { style };

    // ═══════════════════════════════════════════════════════════════════
    // Multi-Cursor & Syntax Selection Effects
    // ═══════════════════════════════════════════════════════════════════

    /// Highlight rows during command execution.
    fn highlight_rows(lines: LineRange, style: HighlightStyle) =>
        Effect::HighlightRows { lines, style };

    /// Set block selections for multi-cursor visual block mode.
    fn set_block_selections(selections: SmallVec<[SelectionRange; 4]>) =>
        Effect::SetBlockSelections { selections };

    /// Save the host's current selection state under a tag.
    fn save_selections(tag: SelectionTag) => Effect::SaveSelections { tag };

    /// Restore a previously saved selection state.
    fn restore_selections(tag: SelectionTag) => Effect::RestoreSelections { tag };

    /// Add a selection at the next match of a pattern.
    fn select_next_match(pattern: Option<String>, skip_current: bool) =>
        Effect::SelectNextMatch { pattern, skip_current };

    /// Add a selection at the previous match of a pattern.
    fn select_previous_match(pattern: Option<String>, skip_current: bool) =>
        Effect::SelectPreviousMatch { pattern, skip_current };

    // ═══════════════════════════════════════════════════════════════════
    // Insert-Mode Bracket Match Flash
    // ═══════════════════════════════════════════════════════════════════

    /// Signal the host to briefly highlight the matching bracket (`showmatch`).
    ///
    /// Emitted when a closing bracket is typed in insert mode with `'showmatch'`
    /// enabled. `position` is the byte offset of the matching opening bracket.
    fn show_match(position: Offset) => Effect::ShowMatch { position }
}
