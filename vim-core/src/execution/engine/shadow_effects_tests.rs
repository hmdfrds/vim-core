//! Tests for shadow effect classification and coalescing.

use super::{classify_effect, coalesce_effects, coalesce_key, CoalesceKey, EffectCategory};
use crate::effects::Effect;
use crate::effects::EffectKind;
use crate::primitives::{
    CommandLineEdit as CmdLineEdit, Direction, FindDirection, InsertEntryType, LastVisualInfo,
    LineNumber, MarkName, Mode, MotionType, Offset, Operator, Range, RegisterName, SelectionShape,
};

// ═════════════════════════════════════════════════════════════════════════════
// Classification tests
// ═════════════════════════════════════════════════════════════════════════════

// ── ShadowApply: every variant ───────────────────────────────────────

#[test]
fn insert_is_shadow_apply() {
    let effect = Effect::insert(Offset::new(0), "hello");
    assert_eq!(classify_effect(&effect), EffectCategory::ShadowApply);
}

#[test]
fn delete_is_shadow_apply() {
    let effect = Effect::delete(Range::from_raw(0, 5));
    assert_eq!(classify_effect(&effect), EffectCategory::ShadowApply);
}

#[test]
fn replace_is_shadow_apply() {
    let effect = Effect::replace(Range::from_raw(0, 3), "xyz");
    assert_eq!(classify_effect(&effect), EffectCategory::ShadowApply);
}

// ── CursorUpdate: every variant ──────────────────────────────────────

#[test]
fn set_cursor_is_cursor_update() {
    let effect = Effect::set_cursor(Offset::new(10));
    assert_eq!(classify_effect(&effect), EffectCategory::CursorUpdate);
}

#[test]
fn set_selection_is_cursor_update() {
    let effect = Effect::SetSelection {
        anchor: Offset::new(0),
        head: Offset::new(10),
        shape: SelectionShape::Char,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::CursorUpdate);
}

#[test]
fn clear_selection_is_cursor_update() {
    let effect = Effect::ClearSelection;
    assert_eq!(classify_effect(&effect), EffectCategory::CursorUpdate);
}

// ── PassThrough: representative sample (>= 10) ──────────────────────

#[test]
fn set_register_is_pass_through() {
    let effect = Effect::SetRegister {
        name: RegisterName::default(),
        text: "text".into(),
        motion_type: MotionType::CharWise,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn set_mark_is_pass_through() {
    let effect = Effect::SetMark {
        name: MarkName::new('a').unwrap(),
        offset: Offset::new(0),
        topline_offset: None,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn set_mode_is_pass_through() {
    let effect = Effect::set_mode(Mode::Normal);
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn show_info_is_pass_through() {
    let effect = Effect::ShowInfo {
        info: crate::effects::InfoMessage::Text("done".into()),
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn clear_message_is_pass_through() {
    let effect = Effect::ClearMessage;
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn scroll_to_is_pass_through() {
    let effect = Effect::ScrollTo {
        offset: Offset::new(0),
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn center_cursor_is_pass_through() {
    let effect = Effect::CenterCursor;
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn push_jump_list_is_pass_through() {
    let effect = Effect::PushJumpList {
        offset: Offset::new(0),
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn set_search_pattern_is_pass_through() {
    let effect = Effect::SetSearchPattern {
        pattern: "foo".into(),
        direction: Direction::Forward,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn begin_undo_group_is_pass_through() {
    let effect = Effect::BeginUndoGroup {
        cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn end_undo_group_is_pass_through() {
    let effect = Effect::EndUndoGroup { node_id: None };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn window_split_is_pass_through() {
    let effect = Effect::WindowSplit;
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn fold_line_is_pass_through() {
    let effect = Effect::FoldLine {
        line: LineNumber::new(0),
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn event_is_pass_through() {
    let effect = Effect::Event {
        kind: crate::primitives::VimEvent::ModeChanged {
            from: Mode::Normal,
            to: Mode::Insert,
        },
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn set_last_find_is_pass_through() {
    let effect = Effect::SetLastFind {
        direction: FindDirection::FindForward,
        target_char: 'x',
        sneak_c2: None,
        resolved_ignorecase: false,
        resolved_smartcase: false,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

// ── HostRequired: every variant ──────────────────────────────────────

#[test]
fn operator_filter_is_host_required() {
    let effect = Effect::OperatorFilter {
        range: Range::from_raw(0, 10),
        motion_type: MotionType::LineWise,
        register: None,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn operator_reindent_is_host_required() {
    let effect = Effect::OperatorReindent {
        range: Range::from_raw(0, 10),
        motion_type: MotionType::LineWise,
        start_col: 0,
        end_col: 0,
        end_line_in_range: 0,
        start_byte_offset: 0,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn host_action_is_host_required() {
    let effect = Effect::HostAction {
        name: "ReformatCode".into(),
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn norm_command_is_host_required() {
    let effect = Effect::NormCommand {
        start_line: LineNumber::new(0),
        end_line: LineNumber::new(5),
        keys: "dd".into(),
        remap: false,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn copy_to_clipboard_is_host_required() {
    let effect = Effect::CopyToClipboard {
        text: "copied".into(),
        register: RegisterName::UNNAMED,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn goto_definition_is_host_required() {
    let effect = Effect::GotoDefinition;
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn show_documentation_is_host_required() {
    let effect = Effect::ShowDocumentation;
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn undo_is_host_required() {
    let effect = Effect::Undo {
        count: 1,
        steps: vec![],
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn redo_is_host_required() {
    let effect = Effect::Redo {
        count: 1,
        steps: vec![],
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn undo_line_is_host_required() {
    let effect = Effect::UndoLine { count: 1 };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn operator_to_mark_is_host_required() {
    let effect = Effect::OperatorToMark {
        operator: Operator::Delete,
        mark: MarkName::new('a').unwrap(),
        linewise: false,
        register: None,
        cursor: Offset::new(0),
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn jump_older_is_pass_through() {
    let effect = Effect::JumpOlder { count: 1 };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn jump_newer_is_pass_through() {
    let effect = Effect::JumpNewer { count: 1 };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn open_command_window_is_host_required() {
    let effect = Effect::OpenCommandWindow {
        prompt: crate::primitives::CommandLinePrompt::Ex,
        history: vec![],
        prefill: None,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

#[test]
fn call_operator_func_is_host_required() {
    let effect = Effect::CallOperatorFunc {
        range: Range::from_raw(0, 10),
        motion_type: MotionType::CharWise,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::HostRequired);
}

// ── Intercepted: every variant ───────────────────────────────────────

#[test]
fn play_macro_is_intercepted() {
    let effect = Effect::PlayMacro {
        register: RegisterName::new('q').unwrap(),
        count: 1,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::Intercepted);
}

#[test]
fn start_recording_is_intercepted() {
    let effect = Effect::StartRecording {
        register: RegisterName::new('q').unwrap(),
    };
    assert_eq!(classify_effect(&effect), EffectCategory::Intercepted);
}

#[test]
fn stop_recording_is_intercepted() {
    let effect = Effect::StopRecording;
    assert_eq!(classify_effect(&effect), EffectCategory::Intercepted);
}

// ── Completeness: ALL variants covered ───────────────────────────────

#[test]
fn all_effect_kinds_are_classified() {
    let total = EffectKind::ALL.len();
    assert_eq!(total, 130, "Expected 130 effect kinds, got {total}");

    let mut shadow_apply = 0u32;
    let mut cursor_update = 0u32;
    let mut pass_through = 0u32;
    let mut intercepted = 0u32;
    let mut host_required = 0u32;

    for kind in &EffectKind::ALL {
        let effect = build_representative_effect(*kind);
        match classify_effect(&effect) {
            EffectCategory::ShadowApply => shadow_apply += 1,
            EffectCategory::CursorUpdate => cursor_update += 1,
            EffectCategory::PassThrough => pass_through += 1,
            EffectCategory::Intercepted => intercepted += 1,
            EffectCategory::HostRequired => host_required += 1,
        }
    }

    let classified = shadow_apply + cursor_update + pass_through + intercepted + host_required;
    assert_eq!(
        classified as usize, total,
        "Only {classified}/{total} variants classified"
    );

    assert_eq!(shadow_apply, 3, "ShadowApply count");
    assert_eq!(cursor_update, 3, "CursorUpdate count");
    assert_eq!(intercepted, 13, "Intercepted count");
    assert_eq!(host_required, 15, "HostRequired count");
    assert_eq!(pass_through, 96, "PassThrough count");
}

// ── Representative effect builders ───────────────────────────────────

/// Build a representative `Effect` instance for the given `EffectKind`.
///
/// Delegates to sub-builders to stay within the 60-line function limit.
/// Field values are arbitrary — only the variant discriminant matters.
fn build_representative_effect(kind: EffectKind) -> Effect {
    build_document_cursor_mode(kind)
        .or_else(|| build_state_search_ui(kind))
        .or_else(|| build_macro_window_fold(kind))
        .unwrap_or_else(|| panic!("unhandled EffectKind in test builder: {kind:?}"))
}

/// Build document mutation, cursor, mode, register, and mark effects.
fn build_document_cursor_mode(kind: EffectKind) -> Option<Effect> {
    let zero = Offset::new(0);
    let range = Range::from_raw(0, 1);
    Some(match kind {
        EffectKind::Insert => Effect::insert(zero, "x"),
        EffectKind::Delete => Effect::delete(range),
        EffectKind::Replace => Effect::replace(range, "y"),
        EffectKind::SetCursor => Effect::set_cursor(zero.into()),
        EffectKind::SetSelection => Effect::SetSelection {
            anchor: zero.into(),
            head: zero.into(),
            shape: SelectionShape::Char,
        },
        EffectKind::ClearSelection => Effect::ClearSelection,
        EffectKind::SaveLastVisual => Effect::SaveLastVisual {
            info: LastVisualInfo::char_wise(1),
        },
        EffectKind::SetMode => Effect::set_mode(Mode::Normal),
        EffectKind::CommandLineEdit => Effect::command_line_edit(CmdLineEdit::Backspace),
        EffectKind::BeginInsert => Effect::BeginInsert {
            entry_type: InsertEntryType::BeforeCursor,
            count: 1,
            auto_indent_len: 0,
            entry_offset: zero,
        },
        EffectKind::SetBlockInsert => Effect::SetBlockInsert {
            lines_below: 0,
            grapheme_col: 0,
            cursor_return_offset: zero,
        },
        EffectKind::SetRegister => Effect::SetRegister {
            name: RegisterName::default(),
            text: "".into(),
            motion_type: MotionType::CharWise,
        },
        EffectKind::SetMark => Effect::SetMark {
            name: MarkName::new('a').unwrap(),
            offset: zero,
            topline_offset: None,
        },
        EffectKind::OperatorToMark => Effect::OperatorToMark {
            operator: Operator::Delete,
            mark: MarkName::new('a').unwrap(),
            linewise: false,
            register: None,
            cursor: zero,
        },
        EffectKind::PushJumpList => Effect::PushJumpList { offset: zero },
        EffectKind::JumpOlder => Effect::JumpOlder { count: 1 },
        EffectKind::JumpNewer => Effect::JumpNewer { count: 1 },
        EffectKind::JumpToBuffer => Effect::JumpToBuffer {
            buffer_id: crate::primitives::BufferId::new(1),
            offset: zero,
        },
        EffectKind::ChangelistOlder => Effect::ChangelistOlder { count: 1 },
        EffectKind::ChangelistNewer => Effect::ChangelistNewer { count: 1 },
        EffectKind::BeginUndoGroup => Effect::BeginUndoGroup {
            cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
        },
        EffectKind::EndUndoGroup => Effect::EndUndoGroup { node_id: None },
        EffectKind::Undo => Effect::Undo {
            count: 1,
            steps: vec![],
        },
        EffectKind::UndoLine => Effect::UndoLine { count: 1 },
        EffectKind::Redo => Effect::Redo {
            count: 1,
            steps: vec![],
        },
        _ => return None,
    })
}

/// Build search, substitute, message, scroll, and operator effects.
fn build_state_search_ui(kind: EffectKind) -> Option<Effect> {
    let zero = Offset::new(0);
    let range = Range::from_raw(0, 1);
    let ln = LineNumber::new(0);
    Some(match kind {
        EffectKind::SetSearchPattern => Effect::SetSearchPattern {
            pattern: "".into(),
            direction: Direction::Forward,
        },
        EffectKind::SetLastSubstitute => Effect::SetLastSubstitute {
            replacement: "".into(),
        },
        EffectKind::SetLastSubstituteFlags => Effect::SetLastSubstituteFlags {
            flags: crate::primitives::SubFlags::default(),
        },
        EffectKind::SetSubstitutePattern => Effect::SetSubstitutePattern { pattern: "".into() },
        EffectKind::HighlightMatches => Effect::HighlightMatches { ranges: vec![] },
        EffectKind::ClearHighlights => Effect::ClearHighlights,
        EffectKind::SetLastFind => Effect::SetLastFind {
            direction: FindDirection::FindForward,
            target_char: 'x',
            sneak_c2: None,
            resolved_ignorecase: false,
            resolved_smartcase: false,
        },
        EffectKind::NormCommand => Effect::NormCommand {
            start_line: ln,
            end_line: ln,
            keys: "".into(),
            remap: false,
        },
        EffectKind::OperatorFilter => Effect::OperatorFilter {
            range,
            motion_type: MotionType::LineWise,
            register: None,
        },
        EffectKind::OperatorReindent => Effect::OperatorReindent {
            range,
            motion_type: MotionType::LineWise,
            start_col: 0,
            end_col: 0,
            end_line_in_range: 0,
            start_byte_offset: 0,
        },
        EffectKind::Bell => Effect::Bell,
        EffectKind::ShowInfo => Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text("".into()),
        },
        EffectKind::ShowWarning => Effect::ShowWarning { text: "".into() },
        EffectKind::ShowError => Effect::ShowError {
            error: crate::errors::VimError::NothingInRegister('x'),
            source: None,
        },
        EffectKind::ClearMessage => Effect::ClearMessage,
        EffectKind::ScrollTo => Effect::ScrollTo { offset: zero },
        EffectKind::CenterCursor => Effect::CenterCursor,
        EffectKind::CursorToTop => Effect::CursorToTop,
        EffectKind::CursorToBottom => Effect::CursorToBottom,
        EffectKind::ScrollLeft => Effect::ScrollLeft { count: 1 },
        EffectKind::ScrollRight => Effect::ScrollRight { count: 1 },
        EffectKind::ScrollHalfScreenLeft => Effect::ScrollHalfScreenLeft { count: 1 },
        EffectKind::ScrollHalfScreenRight => Effect::ScrollHalfScreenRight { count: 1 },
        EffectKind::ScrollCursorToLeftEdge => Effect::ScrollCursorToLeftEdge,
        EffectKind::ScrollCursorToRightEdge => Effect::ScrollCursorToRightEdge,
        EffectKind::CopyToClipboard => Effect::CopyToClipboard {
            text: "".into(),
            register: RegisterName::UNNAMED,
        },
        EffectKind::SearchMatchInfo => Effect::SearchMatchInfo {
            current: 1,
            total: 1,
            complete: true,
        },
        EffectKind::SetScrollHalfCount => Effect::SetScrollHalfCount { count: 10 },
        EffectKind::SetStickyColumn => Effect::SetStickyColumn {
            column: Some(crate::primitives::VirtualColumn::new(5)),
        },
        EffectKind::SetExtState => Effect::SetExtState {
            owner: "test".into(),
            state: vec![1, 2, 3],
        },
        EffectKind::ClearExtState => Effect::ClearExtState {
            owner: "test".into(),
        },
        EffectKind::SetHighlightRange => Effect::SetHighlightRange {
            owner: "test".into(),
            range,
            group: "pending".into(),
            shape: crate::primitives::SelectionShape::Char,
        },
        EffectKind::ClearHighlightRange => Effect::ClearHighlightRange {
            owner: "test".into(),
            group: None,
        },
        EffectKind::SubstitutePreview => Effect::SubstitutePreview { matches: vec![] },
        EffectKind::ClearSubstitutePreview => Effect::ClearSubstitutePreview,
        EffectKind::SyntaxSelectionPush => Effect::SyntaxSelectionPush {
            snapshot: crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(
                    crate::primitives::Offset::new(0),
                    crate::primitives::Offset::new(10),
                ),
            ),
        },
        EffectKind::SyntaxSelectionPop => Effect::SyntaxSelectionPop,
        EffectKind::SyntaxHistoryClear => Effect::SyntaxHistoryClear,
        EffectKind::SetSyntaxSelections => Effect::SetSyntaxSelections {
            selections: crate::primitives::Selections::single(
                crate::primitives::SelectionRange::new(
                    crate::primitives::Offset::new(0),
                    crate::primitives::Offset::new(10),
                ),
            ),
        },
        EffectKind::Event => Effect::Event {
            kind: crate::primitives::VimEvent::ModeChanged {
                from: Mode::Normal,
                to: Mode::Insert,
            },
        },
        EffectKind::HostAction => Effect::HostAction { name: "".into() },
        EffectKind::GotoDefinition => Effect::GotoDefinition,
        EffectKind::ShowDocumentation => Effect::ShowDocumentation,
        EffectKind::SetVirtualText => Effect::SetVirtualText {
            namespace: 0,
            line: LineNumber::new(0),
            col: Offset::new(0),
            text: "".into(),
            position: crate::primitives::VirtualTextPosition::Eol,
        },
        EffectKind::ClearVirtualText => Effect::ClearVirtualText { namespace: 0 },
        EffectKind::SetDiagnostics => Effect::SetDiagnostics {
            namespace: 0,
            diagnostics: vec![],
        },
        EffectKind::CallOperatorFunc => Effect::CallOperatorFunc {
            range,
            motion_type: MotionType::CharWise,
        },
        EffectKind::OpenCommandWindow => Effect::OpenCommandWindow {
            prompt: crate::primitives::CommandLinePrompt::Ex,
            history: vec![],
            prefill: None,
        },
        _ => return None,
    })
}

/// Build macro, recording, window, and fold effects.
fn build_macro_window_fold(kind: EffectKind) -> Option<Effect> {
    let reg_q = RegisterName::new('q').unwrap();
    let ln = LineNumber::new(0);
    Some(match kind {
        EffectKind::StartRecording => Effect::StartRecording { register: reg_q },
        EffectKind::StopRecording => Effect::StopRecording,
        EffectKind::PlayMacro => Effect::PlayMacro {
            register: reg_q,
            count: 1,
        },
        EffectKind::FoldLine => Effect::FoldLine { line: ln },
        EffectKind::UnfoldLine => Effect::UnfoldLine { line: ln },
        EffectKind::ToggleFold => Effect::ToggleFold { line: ln },
        EffectKind::ToggleFoldRecursive => Effect::ToggleFoldRecursive { line: ln },
        EffectKind::FoldAll => Effect::FoldAll,
        EffectKind::UnfoldAll => Effect::UnfoldAll,
        EffectKind::FoldLineRecursive => Effect::FoldLineRecursive { line: ln },
        EffectKind::UnfoldLineRecursive => Effect::UnfoldLineRecursive { line: ln },
        EffectKind::DeleteFold => Effect::DeleteFold { line: ln },
        EffectKind::DeleteFoldRecursive => Effect::DeleteFoldRecursive { line: ln },
        EffectKind::EliminateAllFolds => Effect::EliminateAllFolds,
        EffectKind::ToggleFoldEnable => Effect::ToggleFoldEnable,
        EffectKind::SetFoldEnable => Effect::SetFoldEnable { enabled: true },
        EffectKind::WindowSplit => Effect::WindowSplit,
        EffectKind::WindowNew => Effect::WindowNew,
        EffectKind::WindowVSplit => Effect::WindowVSplit,
        EffectKind::WindowClose => Effect::WindowClose,
        EffectKind::WindowOnly => Effect::WindowOnly,
        EffectKind::WindowNext => Effect::WindowNext,
        EffectKind::WindowPrev => Effect::WindowPrev,
        EffectKind::WindowMoveLeft => Effect::WindowMoveLeft,
        EffectKind::WindowMoveRight => Effect::WindowMoveRight,
        EffectKind::WindowMoveUp => Effect::WindowMoveUp,
        EffectKind::WindowMoveDown => Effect::WindowMoveDown,
        EffectKind::WindowEqualSize => Effect::WindowEqualSize,
        EffectKind::WindowIncreaseHeight => Effect::WindowIncreaseHeight { count: 1 },
        EffectKind::WindowDecreaseHeight => Effect::WindowDecreaseHeight { count: 1 },
        EffectKind::WindowIncreaseWidth => Effect::WindowIncreaseWidth { count: 1 },
        EffectKind::WindowDecreaseWidth => Effect::WindowDecreaseWidth { count: 1 },
        EffectKind::WindowRotateDown => Effect::WindowRotateDown,
        EffectKind::WindowRotateUp => Effect::WindowRotateUp,
        EffectKind::SyncFoldRanges => Effect::SyncFoldRanges { ranges: vec![] },
        EffectKind::UndoTreeSnapshot => Effect::UndoTreeSnapshot {
            snapshot: crate::state::UndoTreeSnapshot {
                nodes: vec![],
                current: crate::state::NodeId::ROOT,
                change_count: 0,
            },
        },
        EffectKind::SetCursorStyle => Effect::SetCursorStyle {
            style: crate::primitives::CursorStyle {
                shape: crate::primitives::CursorShape::Block,
                blink: false,
            },
        },
        EffectKind::CursorShapeHint => Effect::CursorShapeHint {
            pending_operator: None,
        },
        EffectKind::Noop => Effect::Noop,
        EffectKind::ClearNamedRegister => Effect::ClearNamedRegister {
            register: RegisterName::UNNAMED,
        },
        EffectKind::ClearMark => Effect::ClearMark {
            mark: MarkName::new('a').unwrap(),
        },
        // Multi-cursor & syntax selection
        EffectKind::HighlightRows => Effect::HighlightRows {
            lines: crate::primitives::LineRange::new(ln, ln),
            style: crate::effects::HighlightStyle::Active,
        },
        EffectKind::SetBlockSelections => Effect::SetBlockSelections {
            selections: smallvec::SmallVec::new(),
        },
        EffectKind::SaveSelections => Effect::SaveSelections {
            tag: crate::effects::SelectionTag::Search,
        },
        EffectKind::RestoreSelections => Effect::RestoreSelections {
            tag: crate::effects::SelectionTag::Search,
        },
        EffectKind::SelectNextMatch => Effect::SelectNextMatch {
            pattern: None,
            skip_current: false,
        },
        EffectKind::SelectPreviousMatch => Effect::SelectPreviousMatch {
            pattern: None,
            skip_current: false,
        },
        EffectKind::SubstituteConfirmShow => Effect::SubstituteConfirmShow {
            match_range: Range::from_raw(0, 3),
            replacement: "bar".into(),
            match_index: 1,
            total_matches: 3,
        },
        EffectKind::SubstituteConfirmEnd => Effect::SubstituteConfirmEnd,
        EffectKind::SetSubstituteConfirmState => Effect::SetSubstituteConfirmState {
            payload: Box::new(crate::primitives::SubstituteConfirmPayload {
                matches: vec![],
                replacement: "".into(),
                pattern: "".into(),
                flags: crate::primitives::SubFlags::default(),
                gdefault: false,
            }),
        },
        EffectKind::ClearSubstituteConfirmState => Effect::ClearSubstituteConfirmState,
        EffectKind::ShowMatch => Effect::ShowMatch {
            position: Offset::new(0),
        },
        EffectKind::SetVariable => Effect::SetVariable {
            scope: crate::primitives::VarScope::Global,
            name: "x".into(),
            value: crate::primitives::VimValue::Nil,
        },
        EffectKind::DeleteVariable => Effect::DeleteVariable {
            scope: crate::primitives::VarScope::Buffer,
            name: "y".into(),
        },
        EffectKind::CrossBufferEdit => Effect::CrossBufferEdit {
            target: crate::primitives::BufferId::new(1),
            edits: smallvec::SmallVec::new(),
        },
        EffectKind::ModeTransition => Effect::mode_transition(Mode::Normal),
        EffectKind::RequestTimer => Effect::RequestTimer {
            id: 1,
            delay_ms: 4000,
        },
        _ => return None,
    })
}

// ═════════════════════════════════════════════════════════════════════════════
// Coalescing tests
// ═════════════════════════════════════════════════════════════════════════════

// ── Coalesce key grouping ────────────────────────────────────────────

#[test]
fn show_info_has_message_key() {
    let e = Effect::ShowInfo {
        info: crate::effects::InfoMessage::Text("hi".into()),
    };
    assert_eq!(coalesce_key(&e), Some(CoalesceKey::Message));
}

#[test]
fn show_error_has_message_key() {
    let e = Effect::ShowError {
        error: crate::errors::VimError::NothingInRegister('x'),
        source: None,
    };
    assert_eq!(coalesce_key(&e), Some(CoalesceKey::Message));
}

#[test]
fn clear_message_has_message_key() {
    assert_eq!(
        coalesce_key(&Effect::ClearMessage),
        Some(CoalesceKey::Message)
    );
}

#[test]
fn scroll_variants_share_scroll_position_key() {
    let zero = Offset::new(0);
    assert_eq!(
        coalesce_key(&Effect::ScrollTo { offset: zero }),
        Some(CoalesceKey::ScrollPosition),
    );
    assert_eq!(
        coalesce_key(&Effect::CenterCursor),
        Some(CoalesceKey::ScrollPosition),
    );
    assert_eq!(
        coalesce_key(&Effect::CursorToTop),
        Some(CoalesceKey::ScrollPosition),
    );
    assert_eq!(
        coalesce_key(&Effect::CursorToBottom),
        Some(CoalesceKey::ScrollPosition),
    );
}

#[test]
fn horizontal_scroll_variants_share_key() {
    assert_eq!(
        coalesce_key(&Effect::ScrollLeft { count: 1 }),
        Some(CoalesceKey::HorizontalScroll),
    );
    assert_eq!(
        coalesce_key(&Effect::ScrollRight { count: 2 }),
        Some(CoalesceKey::HorizontalScroll),
    );
}

#[test]
fn highlight_variants_share_key() {
    assert_eq!(
        coalesce_key(&Effect::HighlightMatches { ranges: vec![] }),
        Some(CoalesceKey::Highlight),
    );
    assert_eq!(
        coalesce_key(&Effect::ClearHighlights),
        Some(CoalesceKey::Highlight),
    );
}

#[test]
fn non_coalesceable_effects_return_none() {
    let zero = Offset::new(0);
    assert_eq!(coalesce_key(&Effect::insert(zero, "x")), None);
    assert_eq!(coalesce_key(&Effect::set_cursor(zero.into())), None);
    assert_eq!(
        coalesce_key(&Effect::SetRegister {
            name: RegisterName::default(),
            text: "".into(),
            motion_type: MotionType::CharWise,
        }),
        None
    );
    assert_eq!(
        coalesce_key(&Effect::SetMark {
            name: MarkName::new('a').unwrap(),
            offset: zero,
            topline_offset: None,
        }),
        None
    );
    assert_eq!(coalesce_key(&Effect::PushJumpList { offset: zero }), None);
    assert_eq!(
        coalesce_key(&Effect::BeginUndoGroup {
            cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
        }),
        None
    );
    assert_eq!(coalesce_key(&Effect::EndUndoGroup { node_id: None }), None);
    assert_eq!(coalesce_key(&Effect::WindowSplit), None);
    assert_eq!(
        coalesce_key(&Effect::FoldLine {
            line: LineNumber::new(0),
        }),
        None
    );
}

// ── coalesce_effects: message rules ──────────────────────────────────

#[test]
fn two_show_infos_keep_only_last() {
    let effects = vec![
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text("first".into()),
        },
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text("second".into()),
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(
        matches!(&result[0], Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.as_str() == "second")
    );
}

#[test]
fn show_info_then_clear_keeps_only_clear() {
    let effects = vec![
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text("hello".into()),
        },
        Effect::ClearMessage,
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Effect::ClearMessage));
}

#[test]
fn clear_message_then_show_keeps_only_show() {
    let effects = vec![
        Effect::ClearMessage,
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text("hello".into()),
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(
        matches!(&result[0], Effect::ShowInfo { info: crate::effects::InfoMessage::Text(text) } if text.as_str() == "hello")
    );
}

// ── coalesce_effects: scroll rules ───────────────────────────────────

#[test]
fn three_scroll_to_keeps_only_last() {
    let effects = vec![
        Effect::ScrollTo {
            offset: Offset::new(0),
        },
        Effect::ScrollTo {
            offset: Offset::new(10),
        },
        Effect::ScrollTo {
            offset: Offset::new(20),
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        Effect::ScrollTo { offset } if *offset == Offset::new(20)
    ));
}

#[test]
fn scroll_to_then_center_cursor_keeps_only_center() {
    let effects = vec![
        Effect::ScrollTo {
            offset: Offset::new(0),
        },
        Effect::CenterCursor,
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Effect::CenterCursor));
}

// ── coalesce_effects: highlight rules ────────────────────────────────

#[test]
fn two_highlight_matches_keeps_only_last() {
    let range_a = Range::from_raw(0, 5);
    let range_b = Range::from_raw(10, 15);
    let effects = vec![
        Effect::HighlightMatches {
            ranges: vec![range_a],
        },
        Effect::HighlightMatches {
            ranges: vec![range_b],
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        Effect::HighlightMatches { ranges } if ranges == &[range_b]
    ));
}

#[test]
fn clear_highlights_then_highlight_matches_keeps_only_matches() {
    let range = Range::from_raw(0, 5);
    let effects = vec![
        Effect::ClearHighlights,
        Effect::HighlightMatches {
            ranges: vec![range],
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Effect::HighlightMatches { .. }));
}

#[test]
fn highlight_matches_then_clear_keeps_only_clear() {
    let range = Range::from_raw(0, 5);
    let effects = vec![
        Effect::HighlightMatches {
            ranges: vec![range],
        },
        Effect::ClearHighlights,
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Effect::ClearHighlights));
}

// ── coalesce_effects: mode / search pattern ──────────────────────────

#[test]
fn two_set_mode_keeps_only_last() {
    let effects = vec![
        Effect::set_mode(Mode::Insert),
        Effect::set_mode(Mode::Normal),
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0], Effect::SetMode { mode, .. } if *mode == Mode::Normal
    ));
}

#[test]
fn two_set_search_pattern_keeps_only_last() {
    let effects = vec![
        Effect::SetSearchPattern {
            pattern: "foo".into(),
            direction: Direction::Forward,
        },
        Effect::SetSearchPattern {
            pattern: "bar".into(),
            direction: Direction::Backward,
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        Effect::SetSearchPattern { pattern, direction }
            if pattern.as_str() == "bar" && *direction == Direction::Backward
    ));
}

// ── coalesce_effects: non-coalesceable preserved ─────────────────────

#[test]
fn multiple_set_register_with_different_names_all_kept() {
    let effects = vec![
        Effect::SetRegister {
            name: RegisterName::new('a').unwrap(),
            text: "alpha".into(),
            motion_type: MotionType::CharWise,
        },
        Effect::SetRegister {
            name: RegisterName::new('b').unwrap(),
            text: "beta".into(),
            motion_type: MotionType::CharWise,
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 2);
}

#[test]
fn multiple_set_mark_with_different_marks_all_kept() {
    let effects = vec![
        Effect::SetMark {
            name: MarkName::new('a').unwrap(),
            offset: Offset::new(0),
            topline_offset: None,
        },
        Effect::SetMark {
            name: MarkName::new('b').unwrap(),
            offset: Offset::new(10),
            topline_offset: None,
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 2);
}

// ── coalesce_effects: mixed scenario ─────────────────────────────────

#[test]
fn mixed_effects_coalesce_correctly() {
    // [ShowInfo, SetRegister, ShowInfo, SetCursor, ShowError]
    // ShowInfo(0) and ShowInfo(2) share key with ShowError(4).
    // ShowError is last → kept. SetRegister and SetCursor have no key → kept.
    let effects = vec![
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text("first".into()),
        },
        Effect::SetRegister {
            name: RegisterName::default(),
            text: "reg".into(),
            motion_type: MotionType::CharWise,
        },
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text("second".into()),
        },
        Effect::set_cursor(Offset::new(42)),
        Effect::ShowError {
            error: crate::errors::VimError::NothingInRegister('x'),
            source: None,
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 3);
    // SetRegister kept (no key)
    assert!(matches!(&result[0], Effect::SetRegister { .. }));
    // SetCursor kept (no key)
    assert!(matches!(&result[1], Effect::SetCursor { .. }));
    // ShowError kept (last in Message key group)
    assert!(matches!(&result[2], Effect::ShowError { .. }));
}

// ── coalesce_effects: edge cases ─────────────────────────────────────

#[test]
fn empty_input_returns_empty_output() {
    let result = coalesce_effects(vec![]);
    assert!(result.is_empty());
}

#[test]
fn all_non_coalesceable_effects_preserved_in_order() {
    let zero = Offset::new(0);
    let effects = vec![
        Effect::insert(zero, "a"),
        Effect::set_cursor(zero.into()),
        Effect::SetRegister {
            name: RegisterName::default(),
            text: "r".into(),
            motion_type: MotionType::CharWise,
        },
        Effect::PushJumpList { offset: zero },
        Effect::BeginUndoGroup {
            cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
        },
        Effect::EndUndoGroup { node_id: None },
        Effect::WindowSplit,
        Effect::FoldLine {
            line: LineNumber::new(0),
        },
    ];
    let len = effects.len();
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), len);
    // Verify order
    assert!(matches!(&result[0], Effect::Insert { .. }));
    assert!(matches!(&result[1], Effect::SetCursor { .. }));
    assert!(matches!(&result[2], Effect::SetRegister { .. }));
    assert!(matches!(&result[3], Effect::PushJumpList { .. }));
    assert!(matches!(&result[4], Effect::BeginUndoGroup { .. }));
    assert!(matches!(&result[5], Effect::EndUndoGroup { .. }));
    assert!(matches!(&result[6], Effect::WindowSplit));
    assert!(matches!(&result[7], Effect::FoldLine { .. }));
}

// ═════════════════════════════════════════════════════════════════════════════
// Exchange highlight tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn substitute_preview_is_pass_through() {
    let effect = Effect::SubstitutePreview { matches: vec![] };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn clear_substitute_preview_is_pass_through() {
    let effect = Effect::ClearSubstitutePreview;
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn substitute_preview_has_substitute_preview_key() {
    let effect = Effect::SubstitutePreview { matches: vec![] };
    assert_eq!(coalesce_key(&effect), Some(CoalesceKey::SubstitutePreview));
}

#[test]
fn clear_substitute_preview_has_substitute_preview_key() {
    let effect = Effect::ClearSubstitutePreview;
    assert_eq!(coalesce_key(&effect), Some(CoalesceKey::SubstitutePreview));
}

#[test]
fn multiple_substitute_preview_coalesce_to_last() {
    use crate::primitives::SubstitutePreviewMatch;
    let matches_a = vec![SubstitutePreviewMatch::new(
        Offset::new(0),
        Offset::new(3),
        "a".into(),
    )];
    let matches_b = vec![SubstitutePreviewMatch::new(
        Offset::new(5),
        Offset::new(8),
        "b".into(),
    )];
    let effects = vec![
        Effect::SubstitutePreview { matches: matches_a },
        Effect::SubstitutePreview { matches: matches_b },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    if let Effect::SubstitutePreview { matches } = &result[0] {
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].replacement(), "b");
    } else {
        panic!("Expected SubstitutePreview");
    }
}

#[test]
fn clear_substitute_preview_coalesces_with_substitute_preview() {
    use crate::primitives::SubstitutePreviewMatch;
    let matches = vec![SubstitutePreviewMatch::new(
        Offset::new(0),
        Offset::new(3),
        "x".into(),
    )];
    let effects = vec![
        Effect::SubstitutePreview { matches },
        Effect::ClearSubstitutePreview,
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Effect::ClearSubstitutePreview));
}

#[test]
fn clear_then_substitute_preview_coalesces_to_preview() {
    use crate::primitives::SubstitutePreviewMatch;
    let matches = vec![SubstitutePreviewMatch::new(
        Offset::new(0),
        Offset::new(3),
        "y".into(),
    )];
    let effects = vec![
        Effect::ClearSubstitutePreview,
        Effect::SubstitutePreview { matches },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], Effect::SubstitutePreview { .. }));
}

// ═════════════════════════════════════════════════════════════════════════════
// Virtual text / decoration tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn set_virtual_text_is_pass_through() {
    let effect = Effect::SetVirtualText {
        namespace: 1,
        line: LineNumber::new(0),
        col: Offset::new(0),
        text: "hint".into(),
        position: crate::primitives::VirtualTextPosition::Eol,
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn clear_virtual_text_is_pass_through() {
    let effect = Effect::ClearVirtualText { namespace: 1 };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn set_diagnostics_is_pass_through() {
    let effect = Effect::SetDiagnostics {
        namespace: 1,
        diagnostics: vec![],
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn set_virtual_text_has_virtual_text_key() {
    let effect = Effect::SetVirtualText {
        namespace: 1,
        line: LineNumber::new(0),
        col: Offset::new(0),
        text: "hint".into(),
        position: crate::primitives::VirtualTextPosition::Inline,
    };
    assert_eq!(coalesce_key(&effect), Some(CoalesceKey::VirtualText(1)));
}

#[test]
fn clear_virtual_text_has_virtual_text_key() {
    let effect = Effect::ClearVirtualText { namespace: 1 };
    assert_eq!(coalesce_key(&effect), Some(CoalesceKey::VirtualText(1)));
}

#[test]
fn set_diagnostics_has_diagnostics_key() {
    let effect = Effect::SetDiagnostics {
        namespace: 1,
        diagnostics: vec![],
    };
    assert_eq!(coalesce_key(&effect), Some(CoalesceKey::Diagnostics(1)));
}

#[test]
fn multiple_set_virtual_text_coalesce_to_last() {
    let effects = vec![
        Effect::SetVirtualText {
            namespace: 1,
            line: LineNumber::new(0),
            col: Offset::new(0),
            text: "first".into(),
            position: crate::primitives::VirtualTextPosition::Eol,
        },
        Effect::SetVirtualText {
            namespace: 2,
            line: LineNumber::new(5),
            col: Offset::new(10),
            text: "second".into(),
            position: crate::primitives::VirtualTextPosition::Inline,
        },
    ];
    let result = coalesce_effects(effects);
    // Different namespaces are now independent — both survive.
    assert_eq!(result.len(), 2);
    if let Effect::SetVirtualText {
        text, namespace, ..
    } = &result[0]
    {
        assert_eq!(*namespace, 1);
        assert_eq!(text.as_str(), "first");
    } else {
        panic!("Expected SetVirtualText");
    }
    if let Effect::SetVirtualText {
        text, namespace, ..
    } = &result[1]
    {
        assert_eq!(*namespace, 2);
        assert_eq!(text.as_str(), "second");
    } else {
        panic!("Expected SetVirtualText");
    }
}

#[test]
fn clear_virtual_text_coalesces_with_set_virtual_text() {
    let effects = vec![
        Effect::SetVirtualText {
            namespace: 1,
            line: LineNumber::new(0),
            col: Offset::new(0),
            text: "hint".into(),
            position: crate::primitives::VirtualTextPosition::Eol,
        },
        Effect::ClearVirtualText { namespace: 1 },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        Effect::ClearVirtualText { namespace: 1 }
    ));
}

#[test]
fn multiple_set_diagnostics_coalesce_to_last() {
    use crate::primitives::{Diagnostic, DiagnosticSeverity};
    let diag_a = vec![Diagnostic {
        line: LineNumber::new(0),
        col: Offset::new(0),
        end_col: None,
        severity: DiagnosticSeverity::Error,
        message: "first".into(),
        source: None,
    }];
    let diag_b = vec![Diagnostic {
        line: LineNumber::new(1),
        col: Offset::new(0),
        end_col: None,
        severity: DiagnosticSeverity::Warning,
        message: "second".into(),
        source: None,
    }];
    let effects = vec![
        Effect::SetDiagnostics {
            namespace: 1,
            diagnostics: diag_a,
        },
        Effect::SetDiagnostics {
            namespace: 1,
            diagnostics: diag_b,
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    if let Effect::SetDiagnostics { diagnostics, .. } = &result[0] {
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message.as_str(), "second");
    } else {
        panic!("Expected SetDiagnostics");
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// SyncFoldRanges classification and coalescing
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn sync_fold_ranges_is_pass_through() {
    let effect = Effect::SyncFoldRanges {
        ranges: vec![(LineNumber::new(0), LineNumber::new(5))],
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn sync_fold_ranges_has_coalesce_key() {
    let effect = Effect::SyncFoldRanges {
        ranges: vec![(LineNumber::new(0), LineNumber::new(5))],
    };
    assert_eq!(coalesce_key(&effect), Some(CoalesceKey::FoldRanges));
}

#[test]
fn sync_fold_ranges_coalesces_last_wins() {
    let effects = vec![
        Effect::SyncFoldRanges {
            ranges: vec![(LineNumber::new(0), LineNumber::new(5))],
        },
        Effect::SyncFoldRanges {
            ranges: vec![
                (LineNumber::new(0), LineNumber::new(10)),
                (LineNumber::new(15), LineNumber::new(20)),
            ],
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(result.len(), 1);
    if let Effect::SyncFoldRanges { ranges } = &result[0] {
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (LineNumber::new(0), LineNumber::new(10)));
        assert_eq!(ranges[1], (LineNumber::new(15), LineNumber::new(20)));
    } else {
        panic!("Expected SyncFoldRanges");
    }
}

// ─── UndoTreeSnapshot ─────────────────────────────────────────────────

#[test]
fn undo_tree_snapshot_is_pass_through() {
    use crate::state::{NodeId, UndoTreeSnapshot};
    let effect = Effect::UndoTreeSnapshot {
        snapshot: UndoTreeSnapshot {
            nodes: vec![],
            current: NodeId::ROOT,
            change_count: 0,
        },
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn undo_tree_snapshot_has_coalesce_key() {
    use crate::state::{NodeId, UndoTreeSnapshot};
    let effect = Effect::UndoTreeSnapshot {
        snapshot: UndoTreeSnapshot {
            nodes: vec![],
            current: NodeId::ROOT,
            change_count: 0,
        },
    };
    assert_eq!(coalesce_key(&effect), Some(CoalesceKey::UndoTreeSnapshot));
}

#[test]
fn undo_tree_snapshot_coalesces_last_wins() {
    use crate::state::{NodeId, UndoTreeNodeView, UndoTreeSnapshot};
    let snap1 = UndoTreeSnapshot {
        nodes: vec![UndoTreeNodeView {
            id: NodeId::ROOT,
            parent: None,
            children: vec![],
            sequence: 0,
            timestamp: 0,
            cursor_before: Offset::new(0),
            is_current: true,
        }],
        current: NodeId::ROOT,
        change_count: 0,
    };
    let snap2 = UndoTreeSnapshot {
        nodes: vec![
            UndoTreeNodeView {
                id: NodeId::ROOT,
                parent: None,
                children: vec![NodeId::new(1)],
                sequence: 0,
                timestamp: 0,
                cursor_before: Offset::new(0),
                is_current: false,
            },
            UndoTreeNodeView {
                id: NodeId::new(1),
                parent: Some(NodeId::ROOT),
                children: vec![],
                sequence: 1,
                timestamp: 100,
                cursor_before: Offset::new(5),
                is_current: true,
            },
        ],
        current: NodeId::new(1),
        change_count: 1,
    };
    let effects = vec![
        Effect::UndoTreeSnapshot { snapshot: snap1 },
        Effect::UndoTreeSnapshot { snapshot: snap2 },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(
        result.len(),
        1,
        "multiple snapshots should coalesce to last"
    );
    if let Effect::UndoTreeSnapshot { snapshot } = &result[0] {
        assert_eq!(snapshot.change_count, 1, "last snapshot should win");
        assert_eq!(snapshot.nodes.len(), 2);
    } else {
        panic!("Expected UndoTreeSnapshot");
    }
}

// ─── SetCursorStyle: classification + coalescing ────────────────────

#[test]
fn set_cursor_style_is_pass_through() {
    use crate::primitives::{CursorShape, CursorStyle};
    let effect = Effect::SetCursorStyle {
        style: CursorStyle {
            shape: CursorShape::Block,
            blink: false,
        },
    };
    assert_eq!(classify_effect(&effect), EffectCategory::PassThrough);
}

#[test]
fn set_cursor_style_has_coalesce_key() {
    use crate::primitives::{CursorShape, CursorStyle};
    let effect = Effect::SetCursorStyle {
        style: CursorStyle {
            shape: CursorShape::VerticalBar,
            blink: true,
        },
    };
    assert_eq!(coalesce_key(&effect), Some(CoalesceKey::CursorStyle));
}

#[test]
fn set_cursor_style_coalesces_last_wins() {
    use crate::primitives::{CursorShape, CursorStyle};
    let effects = vec![
        Effect::SetCursorStyle {
            style: CursorStyle {
                shape: CursorShape::Block,
                blink: false,
            },
        },
        Effect::SetCursorStyle {
            style: CursorStyle {
                shape: CursorShape::VerticalBar,
                blink: true,
            },
        },
    ];
    let result = coalesce_effects(effects);
    assert_eq!(
        result.len(),
        1,
        "multiple SetCursorStyle should coalesce to last"
    );
    if let Effect::SetCursorStyle { style } = &result[0] {
        assert_eq!(
            style.shape,
            CursorShape::VerticalBar,
            "last style should win"
        );
        assert!(style.blink, "last blink value should win");
    } else {
        panic!("Expected SetCursorStyle");
    }
}
