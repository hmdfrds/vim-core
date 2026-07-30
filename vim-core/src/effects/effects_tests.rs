use super::*;
use crate::effects::effect::{EffectKind, EffectTier};
use crate::primitives::{LineNumber, Mode, Offset};

// === Construction ===

#[test]
fn new_is_empty() {
    let e = Effects::new();
    assert!(e.is_empty());
    assert_eq!(e.len(), 0);
}

#[test]
fn single_has_one_element() {
    let e = Effects::single(Effect::ClearMessage);
    assert_eq!(e.len(), 1);
    assert!(!e.is_empty());
    assert_eq!(e.as_slice()[0], Effect::ClearMessage);
}

// === Builder chain ordering ===

#[test]
fn builder_preserves_insertion_order() {
    let e = Effects::new()
        .set_cursor(Offset::new(0))
        .clear_message()
        .set_mode(Mode::Insert);
    assert_eq!(e.len(), 3);
    assert!(matches!(e.as_slice()[0], Effect::SetCursor { .. }));
    assert!(matches!(e.as_slice()[1], Effect::ClearMessage));
    assert!(matches!(
        e.as_slice()[2],
        Effect::SetMode {
            mode: Mode::Insert,
            ..
        }
    ));
}

// === undo_wrap ===

#[test]
fn undo_wrap_wraps_insert() {
    let e = Effects::new().insert(Offset::new(0), "hello").undo_wrap();
    assert_eq!(e.len(), 3); // BeginUndoGroup + Insert + EndUndoGroup
    assert!(matches!(e.as_slice()[0], Effect::BeginUndoGroup { .. }));
    assert!(matches!(e.as_slice()[1], Effect::Insert { .. }));
    assert!(matches!(e.as_slice()[2], Effect::EndUndoGroup { .. }));
}

#[test]
fn undo_wrap_wraps_delete() {
    let e = Effects::new().delete(Range::from_raw(0, 5)).undo_wrap();
    assert_eq!(e.len(), 3);
    assert!(matches!(e.as_slice()[0], Effect::BeginUndoGroup { .. }));
}

#[test]
fn undo_wrap_wraps_replace() {
    let e = Effects::new()
        .replace(Range::from_raw(0, 5), "new")
        .undo_wrap();
    assert_eq!(e.len(), 3);
    assert!(matches!(e.as_slice()[0], Effect::BeginUndoGroup { .. }));
}

#[test]
fn undo_wrap_skips_non_edits() {
    let e = Effects::new()
        .set_cursor(Offset::new(0))
        .clear_message()
        .undo_wrap();
    assert_eq!(e.len(), 2); // No wrapping — same count
    assert!(matches!(e.as_slice()[0], Effect::SetCursor { .. }));
    assert!(matches!(e.as_slice()[1], Effect::ClearMessage));
}

#[test]
fn undo_wrap_empty() {
    let e = Effects::new().undo_wrap();
    assert!(e.is_empty());
}

// === Iterator traits ===

#[test]
fn into_iter_owned() {
    let e = Effects::new().clear_message().center_cursor();
    let collected: Vec<_> = e.into_iter().collect();
    assert_eq!(collected.len(), 2);
}

#[test]
fn into_iter_borrowed() {
    let e = Effects::new().clear_message();
    let collected: Vec<_> = (&e).into_iter().collect();
    assert_eq!(collected.len(), 1);
}

#[test]
fn from_iterator() {
    let effects: Effects = vec![Effect::ClearMessage, Effect::CenterCursor]
        .into_iter()
        .collect();
    assert_eq!(effects.len(), 2);
}

// === effects! macro ===

#[test]
fn effects_macro_empty() {
    let e = effects!();
    assert!(e.is_empty());
}

#[test]
fn effects_macro_multiple() {
    let e = effects![Effect::ClearMessage, Effect::CenterCursor,];
    assert_eq!(e.len(), 2);
}

// === Extend ===

#[test]
fn extend_merges() {
    let mut a = Effects::new().clear_message();
    let b = Effects::new().center_cursor();
    a.extend(b);
    assert_eq!(a.len(), 2);
}

#[test]
fn extend_vec_merges() {
    let mut a = Effects::new().clear_message();
    a.extend(vec![Effect::CenterCursor]);
    assert_eq!(a.len(), 2);
}

// === Scroll/viewport builders ===

#[test]
fn scroll_builders_exist() {
    let e = Effects::new()
        .cursor_to_top()
        .cursor_to_bottom()
        .scroll_left(3)
        .scroll_right(5);
    assert_eq!(e.len(), 4);
    assert!(matches!(e.as_slice()[0], Effect::CursorToTop));
    assert!(matches!(e.as_slice()[1], Effect::CursorToBottom));
    assert!(matches!(e.as_slice()[2], Effect::ScrollLeft { count: 3 }));
    assert!(matches!(e.as_slice()[3], Effect::ScrollRight { count: 5 }));
}

// === Clipboard builders ===

#[test]
fn clipboard_builders_exist() {
    let e = Effects::new().copy_to_clipboard("hello", RegisterName::UNNAMED);
    assert_eq!(e.len(), 1);
    assert!(matches!(e.as_slice()[0], Effect::CopyToClipboard { .. }));
}

// === Highlight builder ===

#[test]
fn highlight_matches_builder() {
    let ranges = vec![Range::from_raw(0, 5), Range::from_raw(10, 15)];
    let e = Effects::new().highlight_matches(ranges);
    assert_eq!(e.len(), 1);
    if let Effect::HighlightMatches { ranges } = &e.as_slice()[0] {
        assert_eq!(ranges.len(), 2);
    } else {
        panic!("Expected HighlightMatches");
    }
}

// === Norm command builder ===

#[test]
fn norm_command_builder() {
    let e = Effects::new().norm_command(LineNumber::new(0), LineNumber::new(5), "dd", false);
    assert_eq!(e.len(), 1);
    if let Effect::NormCommand {
        start_line,
        end_line,
        keys,
        remap,
    } = &e.as_slice()[0]
    {
        assert_eq!(start_line.get(), 0);
        assert_eq!(end_line.get(), 5);
        assert_eq!(keys.as_str(), "dd");
        assert!(!remap);
    } else {
        panic!("Expected NormCommand");
    }
}

// === Drain ===

#[test]
fn drain_empties_collection() {
    let mut e = Effects::new().clear_message().center_cursor();
    let drained: Vec<_> = e.drain().collect();
    assert_eq!(drained.len(), 2);
    assert!(e.is_empty());
}

// === optimize() ===

#[test]
fn optimize_dedup_trailing_set_cursor() {
    // Two SetCursor effects with no text edit between them — only the last survives.
    let mut e = Effects::new()
        .set_cursor(Offset::new(10))
        .set_cursor(Offset::new(20))
        .set_cursor(Offset::new(30));
    e.optimize();
    assert_eq!(e.len(), 1);
    assert_eq!(
        e.as_slice()[0],
        Effect::SetCursor {
            offset: Offset::new(30)
        }
    );
}

#[test]
fn optimize_preserves_set_cursor_before_edit() {
    // SetCursor before a Delete must be preserved; trailing SetCursor deduped.
    let mut e = Effects::new()
        .set_cursor(Offset::new(5))
        .delete(Range::from_raw(5, 10))
        .set_cursor(Offset::new(0))
        .set_cursor(Offset::new(3));
    e.optimize();
    // Keep: SetCursor(5), Delete, SetCursor(3).  SetCursor(0) is a redundant trailing cursor.
    assert_eq!(e.len(), 3);
    assert_eq!(
        e.as_slice()[0],
        Effect::SetCursor {
            offset: Offset::new(5)
        }
    );
    assert!(matches!(e.as_slice()[1], Effect::Delete { .. }));
    assert_eq!(
        e.as_slice()[2],
        Effect::SetCursor {
            offset: Offset::new(3)
        }
    );
}

#[test]
fn optimize_clear_message_before_show_info() {
    let mut e = Effects::new().clear_message().show_message("hello");
    e.optimize();
    assert_eq!(e.len(), 1);
    assert!(matches!(e.as_slice()[0], Effect::ShowInfo { .. }));
}

#[test]
fn optimize_coalesce_consecutive_show_info() {
    let mut e = Effects::new()
        .show_message("first")
        .show_message("second")
        .show_message("third");
    e.optimize();
    assert_eq!(e.len(), 1);
    if let Effect::ShowInfo {
        info: crate::effects::InfoMessage::Text(text),
    } = &e.as_slice()[0]
    {
        assert_eq!(text.as_str(), "third");
    } else {
        panic!("Expected ShowInfo");
    }
}

#[test]
fn optimize_clear_then_consecutive_show() {
    // ClearMessage + ShowInfo("a") + ShowInfo("b") => ShowInfo("b")
    let mut e = Effects::new()
        .clear_message()
        .show_message("a")
        .show_message("b");
    e.optimize();
    assert_eq!(e.len(), 1);
    if let Effect::ShowInfo {
        info: crate::effects::InfoMessage::Text(text),
    } = &e.as_slice()[0]
    {
        assert_eq!(text.as_str(), "b");
    } else {
        panic!("Expected ShowInfo");
    }
}

#[test]
fn optimize_no_effect_on_empty() {
    let mut e = Effects::new();
    e.optimize();
    assert!(e.is_empty());
}

#[test]
fn optimize_no_effect_on_single() {
    let mut e = Effects::single(Effect::ClearMessage);
    e.optimize();
    assert_eq!(e.len(), 1);
}

#[test]
fn optimize_interleaved_cursors_and_edits_all_preserved() {
    // SetCursor before each edit must be preserved.
    let mut e = Effects::new()
        .set_cursor(Offset::new(0))
        .insert(Offset::new(0), "a")
        .set_cursor(Offset::new(5))
        .delete(Range::from_raw(5, 10))
        .set_cursor(Offset::new(3));
    e.optimize();
    // All 5 effects survive: the two SetCursors before edits are not trailing,
    // and the last SetCursor(3) is the sole trailing cursor.
    assert_eq!(e.len(), 5);
}

#[test]
fn optimize_clear_message_not_removed_when_not_followed_by_show() {
    // ClearMessage followed by SetCursor — ClearMessage must survive.
    let mut e = Effects::new().clear_message().set_cursor(Offset::new(0));
    e.optimize();
    assert_eq!(e.len(), 2);
    assert!(matches!(e.as_slice()[0], Effect::ClearMessage));
}

#[test]
fn optimize_show_info_separated_by_other_effect() {
    // Two ShowInfo with a non-ShowInfo between them: both survive.
    let mut e = Effects::new()
        .show_message("first")
        .clear_message()
        .show_message("second");
    e.optimize();
    // ClearMessage is followed by ShowInfo so it gets removed.
    // But "first" is followed by ClearMessage (not ShowInfo), so it stays.
    assert_eq!(e.len(), 2);
    assert!(matches!(e.as_slice()[0], Effect::ShowInfo { .. }));
    assert!(matches!(e.as_slice()[1], Effect::ShowInfo { .. }));
}

// === From<SmallVec> impls ===

#[test]
fn from_smallvec_8() {
    let mut sv = SmallVec::<[Effect; 8]>::new();
    sv.push(Effect::ClearMessage);
    let e: Effects = sv.into();
    assert_eq!(e.len(), 1);
}

#[test]
fn from_smallvec_4() {
    let mut sv = SmallVec::<[Effect; 4]>::new();
    sv.push(Effect::ClearMessage);
    let e: Effects = sv.into();
    assert_eq!(e.len(), 1);
}

#[test]
fn from_smallvec_3() {
    let mut sv = SmallVec::<[Effect; 3]>::new();
    sv.push(Effect::ClearMessage);
    let e: Effects = sv.into();
    assert_eq!(e.len(), 1);
}

#[test]
fn from_smallvec_2() {
    let mut sv = SmallVec::<[Effect; 2]>::new();
    sv.push(Effect::ClearMessage);
    let e: Effects = sv.into();
    assert_eq!(e.len(), 1);
}

// === validate_ordering ===

#[test]
fn test_validate_ordering_valid_sequence() {
    // A well-formed sequence: undo group with edit, cursor after edit.
    let e = Effects::new()
        .begin_undo()
        .delete(Range::from_raw(0, 5))
        .insert(Offset::new(0), "hello")
        .end_undo()
        .set_cursor(Offset::new(5));
    assert!(e.validate_ordering().is_ok());
}

#[test]
fn test_validate_ordering_edit_after_final_cursor() {
    // SetCursor appears before the last text edit — violation.
    let e = Effects::new()
        .set_cursor(Offset::new(0))
        .insert(Offset::new(0), "hello");
    assert_eq!(
        e.validate_ordering(),
        Err(OrderingError::EditAfterFinalCursor),
    );
}

#[test]
fn test_validate_ordering_empty_undo_group() {
    // Undo group with no edits inside — valid, matches Neovim behavior for cursor-only ops.
    let e = Effects::new()
        .begin_undo()
        .set_cursor(Offset::new(0))
        .end_undo();
    assert_eq!(e.validate_ordering(), Ok(()),);
}

#[test]
fn test_validate_ordering_mismatched_undo() {
    // Unclosed undo group.
    let e = Effects::new().begin_undo().insert(Offset::new(0), "hello");
    assert_eq!(e.validate_ordering(), Err(OrderingError::UndoGroupMismatch),);
}

#[test]
fn test_validate_ordering_empty_effects() {
    let e = Effects::new();
    assert!(e.validate_ordering().is_ok());
}

#[test]
fn test_validate_ordering_extra_end_undo() {
    // EndUndoGroup without matching BeginUndoGroup.
    let effects = vec![Effect::EndUndoGroup { node_id: None }];
    assert_eq!(
        validate_ordering(&effects),
        Err(OrderingError::UndoGroupMismatch),
    );
}

#[test]
fn test_validate_ordering_no_cursor_with_edits_passes() {
    // Edits without any SetCursor — invariant 3 only fires when *both* exist
    // and the cursor is before the last edit. No cursor at all is valid
    // (some commands don't emit SetCursor, the host preserves cursor position).
    let e = Effects::new()
        .begin_undo()
        .insert(Offset::new(0), "hi")
        .end_undo();
    assert!(e.validate_ordering().is_ok());
}

#[test]
fn test_validate_ordering_cursor_only_passes() {
    // Only cursor effects, no edits — valid.
    let e = Effects::new()
        .set_cursor(Offset::new(0))
        .set_cursor(Offset::new(10));
    assert!(e.validate_ordering().is_ok());
}

#[test]
fn test_validate_ordering_standalone_function() {
    // Verify the standalone function works on raw slices.
    let effects = vec![
        Effect::BeginUndoGroup {
            cursor_strategy: crate::primitives::UndoCursorStrategy::FirstEdit,
        },
        Effect::Insert {
            offset: Offset::new(0),
            text: "a".into(),
        },
        Effect::EndUndoGroup { node_id: None },
        Effect::SetCursor {
            offset: Offset::new(1),
        },
    ];
    assert!(validate_ordering(&effects).is_ok());
}

// === Substitute preview builders ===

#[test]
fn substitute_preview_match_construction() {
    use crate::primitives::SubstitutePreviewMatch;
    let m = SubstitutePreviewMatch::new(Offset::new(5), Offset::new(10), "bar".into());
    assert_eq!(m.match_start(), Offset::new(5));
    assert_eq!(m.match_end(), Offset::new(10));
    assert_eq!(m.replacement(), "bar");
}

#[test]
fn substitute_preview_builder_produces_correct_variant() {
    use crate::primitives::SubstitutePreviewMatch;
    let matches = vec![SubstitutePreviewMatch::new(
        Offset::new(0),
        Offset::new(3),
        "xyz".into(),
    )];
    let e = Effects::new().substitute_preview(matches);
    assert_eq!(e.len(), 1);
    if let Effect::SubstitutePreview { matches } = &e.as_slice()[0] {
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].match_start(), Offset::new(0));
        assert_eq!(matches[0].match_end(), Offset::new(3));
        assert_eq!(matches[0].replacement(), "xyz");
    } else {
        panic!("Expected SubstitutePreview");
    }
}

#[test]
fn clear_substitute_preview_builder_produces_correct_variant() {
    let e = Effects::new().clear_substitute_preview();
    assert_eq!(e.len(), 1);
    assert!(matches!(e.as_slice()[0], Effect::ClearSubstitutePreview));
}

#[test]
fn substitute_preview_kind_returns_correct_effect_kind() {
    let effect = Effect::SubstitutePreview { matches: vec![] };
    assert_eq!(effect.kind(), crate::effects::EffectKind::SubstitutePreview);
}

#[test]
fn clear_substitute_preview_kind_returns_correct_effect_kind() {
    let effect = Effect::ClearSubstitutePreview;
    assert_eq!(
        effect.kind(),
        crate::effects::EffectKind::ClearSubstitutePreview
    );
}

#[test]
fn substitute_preview_builder_chain() {
    use crate::primitives::SubstitutePreviewMatch;
    let matches = vec![SubstitutePreviewMatch::new(
        Offset::new(0),
        Offset::new(5),
        "new".into(),
    )];
    let e = Effects::new()
        .substitute_preview(matches)
        .set_cursor(Offset::new(0))
        .clear_substitute_preview();
    assert_eq!(e.len(), 3);
    assert!(matches!(e.as_slice()[0], Effect::SubstitutePreview { .. }));
    assert!(matches!(e.as_slice()[1], Effect::SetCursor { .. }));
    assert!(matches!(e.as_slice()[2], Effect::ClearSubstitutePreview));
}

// === Virtual text builders ===

#[test]
fn set_virtual_text_builder_produces_correct_variant() {
    use crate::primitives::VirtualTextPosition;
    let e = Effects::new().set_virtual_text(
        42,
        LineNumber::new(10),
        Offset::new(5),
        "type: i32",
        VirtualTextPosition::Inline,
    );
    assert_eq!(e.len(), 1);
    if let Effect::SetVirtualText {
        namespace,
        line,
        col,
        text,
        position,
    } = &e.as_slice()[0]
    {
        assert_eq!(*namespace, 42);
        assert_eq!(*line, LineNumber::new(10));
        assert_eq!(*col, Offset::new(5));
        assert_eq!(text.as_str(), "type: i32");
        assert_eq!(*position, VirtualTextPosition::Inline);
    } else {
        panic!("Expected SetVirtualText");
    }
}

#[test]
fn clear_virtual_text_builder_produces_correct_variant() {
    let e = Effects::new().clear_virtual_text(7);
    assert_eq!(e.len(), 1);
    if let Effect::ClearVirtualText { namespace } = &e.as_slice()[0] {
        assert_eq!(*namespace, 7);
    } else {
        panic!("Expected ClearVirtualText");
    }
}

#[test]
fn set_diagnostics_builder_produces_correct_variant() {
    use crate::primitives::{Diagnostic, DiagnosticSeverity};
    let diagnostics = vec![Diagnostic {
        line: LineNumber::new(3),
        col: Offset::new(10),
        end_col: Some(Offset::new(15)),
        severity: DiagnosticSeverity::Error,
        message: "undefined".into(),
        source: Some("rust-analyzer".into()),
    }];
    let e = Effects::new().set_diagnostics(1, diagnostics);
    assert_eq!(e.len(), 1);
    if let Effect::SetDiagnostics {
        namespace,
        diagnostics,
    } = &e.as_slice()[0]
    {
        assert_eq!(*namespace, 1);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostics[0].message.as_str(), "undefined");
    } else {
        panic!("Expected SetDiagnostics");
    }
}

#[test]
fn set_virtual_text_kind_returns_correct_effect_kind() {
    use crate::primitives::VirtualTextPosition;
    let effect = Effect::SetVirtualText {
        namespace: 0,
        line: LineNumber::new(0),
        col: Offset::new(0),
        text: "hint".into(),
        position: VirtualTextPosition::Eol,
    };
    assert_eq!(effect.kind(), crate::effects::EffectKind::SetVirtualText);
}

#[test]
fn clear_virtual_text_kind_returns_correct_effect_kind() {
    let effect = Effect::ClearVirtualText { namespace: 0 };
    assert_eq!(effect.kind(), crate::effects::EffectKind::ClearVirtualText);
}

#[test]
fn set_diagnostics_kind_returns_correct_effect_kind() {
    let effect = Effect::SetDiagnostics {
        namespace: 0,
        diagnostics: vec![],
    };
    assert_eq!(effect.kind(), crate::effects::EffectKind::SetDiagnostics);
}

#[test]
fn virtual_text_builder_chain() {
    use crate::primitives::{Diagnostic, DiagnosticSeverity, VirtualTextPosition};
    let diagnostics = vec![Diagnostic {
        line: LineNumber::new(0),
        col: Offset::new(0),
        end_col: None,
        severity: DiagnosticSeverity::Hint,
        message: "hint".into(),
        source: None,
    }];
    let e = Effects::new()
        .set_virtual_text(
            1,
            LineNumber::new(0),
            Offset::new(0),
            "ghost",
            VirtualTextPosition::Eol,
        )
        .set_cursor(Offset::new(5))
        .clear_virtual_text(1)
        .set_diagnostics(2, diagnostics);
    assert_eq!(e.len(), 4);
    assert!(matches!(e.as_slice()[0], Effect::SetVirtualText { .. }));
    assert!(matches!(e.as_slice()[1], Effect::SetCursor { .. }));
    assert!(matches!(e.as_slice()[2], Effect::ClearVirtualText { .. }));
    assert!(matches!(e.as_slice()[3], Effect::SetDiagnostics { .. }));
}

// === SyncFoldRanges ===

#[test]
fn sync_fold_ranges_builder() {
    let ranges = vec![
        (LineNumber::new(0), LineNumber::new(5)),
        (LineNumber::new(10), LineNumber::new(20)),
    ];
    let e = Effects::new().sync_fold_ranges(ranges.clone());
    assert_eq!(e.len(), 1);
    match &e.as_slice()[0] {
        Effect::SyncFoldRanges { ranges: r } => {
            assert_eq!(r.len(), 2);
            assert_eq!(r[0], (LineNumber::new(0), LineNumber::new(5)));
            assert_eq!(r[1], (LineNumber::new(10), LineNumber::new(20)));
        }
        other => panic!("Expected SyncFoldRanges, got {other:?}"),
    }
}

#[test]
fn sync_fold_ranges_kind_mapping() {
    let e = Effect::SyncFoldRanges {
        ranges: vec![(LineNumber::new(0), LineNumber::new(10))],
    };
    assert_eq!(e.kind(), crate::effects::EffectKind::SyncFoldRanges);
}

// === UndoTreeSnapshot ===

#[test]
fn undo_tree_snapshot_builder_produces_correct_variant() {
    use crate::primitives::{NodeId, UndoTreeNodeView, UndoTreeSnapshot};
    let snapshot = UndoTreeSnapshot {
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
    let e = Effects::new().undo_tree_snapshot(snapshot.clone());
    assert_eq!(e.len(), 1);
    match &e.as_slice()[0] {
        Effect::UndoTreeSnapshot { snapshot: s } => {
            assert_eq!(s.nodes.len(), 1);
            assert_eq!(s.current, NodeId::ROOT);
            assert_eq!(s.change_count, 0);
        }
        other => panic!("Expected UndoTreeSnapshot, got {other:?}"),
    }
}

#[test]
fn undo_tree_snapshot_kind_mapping() {
    use crate::primitives::{NodeId, UndoTreeSnapshot};
    let e = Effect::UndoTreeSnapshot {
        snapshot: UndoTreeSnapshot {
            nodes: vec![],
            current: NodeId::ROOT,
            change_count: 0,
        },
    };
    assert_eq!(e.kind(), crate::effects::EffectKind::UndoTreeSnapshot);
}

#[test]
fn set_visual_selection_emits_paired_effects() {
    use crate::effects::Effect;
    use crate::primitives::{Offset, SelectionShape};

    let anchor = Offset::new(5);
    let head = Offset::new(15);
    let effects = Effects::new().set_visual_selection(anchor, head, SelectionShape::Char);

    assert_eq!(
        effects.len(),
        2,
        "set_visual_selection should emit exactly 2 effects"
    );
    assert!(
        matches!(effects.as_slice()[0], Effect::SetSelection { anchor: a, head: h, shape: SelectionShape::Char } if a == anchor && h == head),
        "First effect should be SetSelection"
    );
    assert!(
        matches!(effects.as_slice()[1], Effect::SetCursor { offset } if offset == head),
        "Second effect should be SetCursor(head)"
    );
}

// === SourceContext ===

#[test]
fn show_error_with_source_context() {
    use crate::effects::effect::SourceContext;
    let effects = Effects::new().show_error_with_source(
        crate::errors::VimError::PatternNotFound("x".into()),
        SourceContext {
            file: "init.vim".into(),
            line: 42,
        },
    );
    match &effects.as_slice()[0] {
        Effect::ShowError { error, source } => {
            assert!(matches!(error, crate::errors::VimError::PatternNotFound(_)));
            let src = source.as_ref().unwrap();
            assert_eq!(src.file.as_str(), "init.vim");
            assert_eq!(src.line, 42);
        }
        _ => panic!("Expected ShowError"),
    }
}

#[test]
fn show_error_without_source() {
    let effects = Effects::new().show_error(crate::errors::VimError::InvalidRange);
    match &effects.as_slice()[0] {
        Effect::ShowError { source, .. } => assert!(source.is_none()),
        _ => panic!("Expected ShowError"),
    }
}

// === Bell + ShowInfo + LineModCounts tests ===

#[test]
fn bell_effect_is_standard_tier() {
    assert_eq!(EffectKind::Bell.tier(), EffectTier::Standard);
}

#[test]
fn show_info_effect_is_standard_tier() {
    assert_eq!(EffectKind::ShowInfo.tier(), EffectTier::Standard);
}

#[test]
fn bell_builder() {
    let effects = Effects::new().bell();
    assert!(matches!(effects.as_slice()[0], Effect::Bell));
}

#[test]
fn show_line_report_builder() {
    let counts = crate::effects::LineModCounts {
        yanked: 5,
        ..Default::default()
    };
    let effects = Effects::new().show_line_report(counts);
    match &effects.as_slice()[0] {
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::LineReport(c),
        } => assert_eq!(c.yanked, 5),
        _ => panic!("Expected ShowInfo::LineReport"),
    }
}

#[test]
fn show_verbose_builder() {
    let effects = Effects::new().show_verbose("debug info");
    match &effects.as_slice()[0] {
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Verbose(text),
        } => assert_eq!(text.as_str(), "debug info"),
        _ => panic!("Expected ShowInfo::Verbose"),
    }
}

#[test]
fn show_info_builder() {
    let info = crate::effects::InfoMessage::Text("hello".into());
    let effects = Effects::new().show_info(info);
    match &effects.as_slice()[0] {
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text(text),
        } => assert_eq!(text.as_str(), "hello"),
        _ => panic!("Expected ShowInfo::Text"),
    }
}

#[test]
fn show_message_builder_creates_show_info() {
    let effects = Effects::new().show_message("hello");
    match &effects.as_slice()[0] {
        Effect::ShowInfo {
            info: crate::effects::InfoMessage::Text(text),
        } => assert_eq!(text.as_str(), "hello"),
        _ => panic!("Expected ShowInfo with InfoMessage::Text"),
    }
}

// === ShowMatch ===

#[test]
fn show_match_is_standard_tier() {
    assert_eq!(EffectKind::ShowMatch.tier(), EffectTier::Standard);
}

#[test]
fn show_match_builder() {
    let effects = Effects::new().show_match(Offset::new(42));
    match &effects.as_slice()[0] {
        Effect::ShowMatch { position } => assert_eq!(position.get(), 42),
        _ => panic!("Expected ShowMatch"),
    }
}
