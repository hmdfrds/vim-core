use super::*;
use crate::ir::{CharClass, EscapeKind, VimPatternNode};

// Helper: build a simple literal node.
fn lit(ch: char) -> VimPatternNode {
    VimPatternNode::Literal(ch)
}

// Helper: lower and return the node.
fn lower_to_node(node: &VimPatternNode) -> LoweredNode {
    lower(node).0
}

// Helper: lower and return the properties.
fn lower_to_props(node: &VimPatternNode) -> PatternProperties {
    lower(node).1
}

// ═══════════════════════════════════════════════════════════════════════
// Normalization rule 5: Adjacent literal fusion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn adjacent_literals_fuse_into_literal_string() {
    let node = VimPatternNode::Sequence(vec![lit('h'), lit('e'), lit('l'), lit('l'), lit('o')]);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::LiteralString(CompactString::from("hello"))
    );
}

#[test]
fn literal_fusion_stops_at_non_literal() {
    let node = VimPatternNode::Sequence(vec![
        lit('a'),
        lit('b'),
        VimPatternNode::AnyChar,
        lit('c'),
        lit('d'),
    ]);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Sequence(vec![
            LoweredNode::LiteralString(CompactString::from("ab")),
            LoweredNode::AnyChar,
            LoweredNode::LiteralString(CompactString::from("cd")),
        ])
    );
}

#[test]
fn escape_sequences_fuse_with_adjacent_literals() {
    // EscapeSequence lowered to Literal first, then fused.
    let node = VimPatternNode::Sequence(vec![
        lit('a'),
        VimPatternNode::EscapeSequence(EscapeKind::Tab),
        lit('b'),
    ]);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::LiteralString(CompactString::from("a\tb"))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Normalization rule 6: Nested sequence flattening
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_sequence_is_flattened() {
    let inner = VimPatternNode::Sequence(vec![lit('a'), lit('b')]);
    let outer = VimPatternNode::Sequence(vec![inner, lit('c')]);
    // Inner "ab" fuses with "c" → "abc"
    assert_eq!(
        lower_to_node(&outer),
        LoweredNode::LiteralString(CompactString::from("abc"))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Normalization rule 7: Quantifier {1,1} unwrap
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn quantifier_one_one_unwraps() {
    let node = VimPatternNode::Quantifier {
        node: Box::new(lit('a')),
        min: 1,
        max: Some(1),
        greedy: true,
    };
    assert_eq!(lower_to_node(&node), LoweredNode::Literal('a'));
}

// ═══════════════════════════════════════════════════════════════════════
// Normalization rule 8: Quantifier {0,0} → empty
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn quantifier_zero_zero_becomes_empty_sequence() {
    let node = VimPatternNode::Quantifier {
        node: Box::new(lit('a')),
        min: 0,
        max: Some(0),
        greedy: true,
    };
    assert_eq!(lower_to_node(&node), LoweredNode::Sequence(vec![]));
}

// ═══════════════════════════════════════════════════════════════════════
// Normalization rule 9: Small alternation of literals → Collection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn literal_alternation_becomes_collection() {
    let node = VimPatternNode::Alternation(vec![lit('a'), lit('b'), lit('c')]);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Collection {
            negated: false,
            items: vec![
                CollectionItem::Single('a'),
                CollectionItem::Single('b'),
                CollectionItem::Single('c'),
            ],
            include_newline: false,
        }
    );
}

#[test]
fn mixed_alternation_stays_as_alternation() {
    let node = VimPatternNode::Alternation(vec![lit('a'), VimPatternNode::AnyChar]);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Alternation(vec![LoweredNode::Literal('a'), LoweredNode::AnyChar])
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Property computation: min/max lengths
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn literal_min_max_len_ascii() {
    let props = lower_to_props(&lit('a'));
    assert_eq!(props.minimum_match_len(), 1);
    assert_eq!(props.maximum_match_len(), Some(1));
}

#[test]
fn literal_min_max_len_multibyte() {
    let props = lower_to_props(&VimPatternNode::Literal('€'));
    assert_eq!(props.minimum_match_len(), 3); // € is 3 bytes in UTF-8
    assert_eq!(props.maximum_match_len(), Some(3));
}

#[test]
fn any_char_min_max_len() {
    let props = lower_to_props(&VimPatternNode::AnyChar);
    assert_eq!(props.minimum_match_len(), 1);
    assert_eq!(props.maximum_match_len(), Some(4));
}

#[test]
fn collection_min_max_len() {
    let node = VimPatternNode::Collection {
        negated: false,
        items: vec![CollectionItem::Single('x')],
        include_newline: false,
    };
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 1);
    assert_eq!(props.maximum_match_len(), Some(4));
}

#[test]
fn sequence_min_max_len() {
    let node = VimPatternNode::Sequence(vec![lit('a'), lit('b'), lit('c')]);
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 3);
    assert_eq!(props.maximum_match_len(), Some(3));
}

#[test]
fn alternation_min_max_len() {
    // After lowering: becomes Collection (3 single-char literals)
    let node = VimPatternNode::Alternation(vec![lit('a'), lit('b'), lit('c')]);
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 1);
    assert_eq!(props.maximum_match_len(), Some(4));
}

#[test]
fn alternation_mixed_min_max_len() {
    let node = VimPatternNode::Alternation(vec![
        VimPatternNode::Sequence(vec![lit('a'), lit('b')]),
        lit('c'),
    ]);
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 1); // min of (2, 1)
    assert_eq!(props.maximum_match_len(), Some(2)); // max of (2, 1)
}

#[test]
fn quantifier_min_max_len() {
    let node = VimPatternNode::Quantifier {
        node: Box::new(lit('a')),
        min: 2,
        max: Some(5),
        greedy: true,
    };
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 2);
    assert_eq!(props.maximum_match_len(), Some(5));
}

#[test]
fn quantifier_unbounded_max() {
    let node = VimPatternNode::Quantifier {
        node: Box::new(lit('a')),
        min: 1,
        max: None,
        greedy: true,
    };
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 1);
    assert_eq!(props.maximum_match_len(), None);
}

#[test]
fn group_passes_through_properties() {
    let node = VimPatternNode::Group {
        inner: Box::new(lit('x')),
        capturing: false,
    };
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 1);
    assert_eq!(props.maximum_match_len(), Some(1));
}

#[test]
fn backreference_min_max_len() {
    let node = VimPatternNode::BackReference(1);
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 0);
    assert_eq!(props.maximum_match_len(), None);
}

#[test]
fn lookaround_is_zero_width() {
    let node = VimPatternNode::Lookaround {
        inner: Box::new(lit('a')),
        kind: LookaroundKind::PositiveAhead,
        limit: None,
    };
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 0);
    assert_eq!(props.maximum_match_len(), Some(0));
}

#[test]
fn last_substitute_min_max_len() {
    let node = VimPatternNode::LastSubstitute;
    let props = lower_to_props(&node);
    assert_eq!(props.minimum_match_len(), 0);
    assert_eq!(props.maximum_match_len(), None);
}

#[test]
fn zero_width_assertion_lengths() {
    for node in [
        VimPatternNode::StartOfLine,
        VimPatternNode::EndOfLine,
        VimPatternNode::StartOfFile,
        VimPatternNode::EndOfFile,
        VimPatternNode::WordBoundaryStart,
        VimPatternNode::WordBoundaryEnd,
        VimPatternNode::SetMatchStart,
        VimPatternNode::SetMatchEnd,
        VimPatternNode::CursorPosition,
        VimPatternNode::VisualArea,
    ] {
        let props = lower_to_props(&node);
        assert_eq!(
            props.minimum_match_len(),
            0,
            "min_len should be 0 for {node:?}"
        );
        assert_eq!(
            props.maximum_match_len(),
            Some(0),
            "max_len should be Some(0) for {node:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Property computation: prefix extraction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn literal_prefix() {
    let props = lower_to_props(&lit('z'));
    assert_eq!(
        props.literal_prefix().cloned(),
        Some(CompactString::from("z"))
    );
}

#[test]
fn literal_string_prefix() {
    let node = VimPatternNode::Sequence(vec![lit('h'), lit('i')]);
    let props = lower_to_props(&node);
    assert_eq!(
        props.literal_prefix().cloned(),
        Some(CompactString::from("hi"))
    );
}

#[test]
fn sequence_prefix_stops_at_non_literal() {
    let node = VimPatternNode::Sequence(vec![lit('a'), VimPatternNode::AnyChar, lit('b')]);
    let props = lower_to_props(&node);
    assert_eq!(
        props.literal_prefix().cloned(),
        Some(CompactString::from("a"))
    );
}

#[test]
fn any_char_has_no_prefix() {
    let props = lower_to_props(&VimPatternNode::AnyChar);
    assert_eq!(props.literal_prefix(), None);
}

#[test]
fn quantifier_with_min_zero_has_no_prefix() {
    let node = VimPatternNode::Quantifier {
        node: Box::new(lit('a')),
        min: 0,
        max: None,
        greedy: true,
    };
    let props = lower_to_props(&node);
    assert_eq!(props.literal_prefix(), None);
}

#[test]
fn quantifier_with_min_nonzero_has_prefix() {
    let node = VimPatternNode::Quantifier {
        node: Box::new(lit('a')),
        min: 2,
        max: Some(5),
        greedy: true,
    };
    let props = lower_to_props(&node);
    assert_eq!(
        props.literal_prefix().cloned(),
        Some(CompactString::from("a"))
    );
}

#[test]
fn alternation_common_prefix() {
    let node = VimPatternNode::Alternation(vec![
        VimPatternNode::Sequence(vec![lit('f'), lit('o'), lit('o')]),
        VimPatternNode::Sequence(vec![lit('f'), lit('o'), lit('b')]),
    ]);
    let props = lower_to_props(&node);
    assert_eq!(
        props.literal_prefix().cloned(),
        Some(CompactString::from("fo"))
    );
}

#[test]
fn alternation_no_common_prefix() {
    let node = VimPatternNode::Alternation(vec![
        VimPatternNode::Sequence(vec![lit('a')]),
        VimPatternNode::Sequence(vec![lit('b')]),
    ]);
    let props = lower_to_props(&node);
    assert_eq!(props.literal_prefix(), None);
}

// ═══════════════════════════════════════════════════════════════════════
// Property computation: anchoring detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn start_of_line_anchored() {
    let node = VimPatternNode::Sequence(vec![VimPatternNode::StartOfLine, lit('a')]);
    let props = lower_to_props(&node);
    assert!(props.accel_hints.is_anchored_start);
    assert!(!props.accel_hints.is_anchored_end);
}

#[test]
fn end_of_line_anchored() {
    let node = VimPatternNode::Sequence(vec![lit('a'), VimPatternNode::EndOfLine]);
    let props = lower_to_props(&node);
    assert!(!props.accel_hints.is_anchored_start);
    assert!(props.accel_hints.is_anchored_end);
}

#[test]
fn start_and_end_of_file_anchored() {
    let node = VimPatternNode::Sequence(vec![
        VimPatternNode::StartOfFile,
        lit('x'),
        VimPatternNode::EndOfFile,
    ]);
    let props = lower_to_props(&node);
    assert!(props.accel_hints.is_anchored_start);
    assert!(props.accel_hints.is_anchored_end);
}

#[test]
fn bare_literal_not_anchored() {
    let props = lower_to_props(&lit('a'));
    assert!(!props.accel_hints.is_anchored_start);
    assert!(!props.accel_hints.is_anchored_end);
}

// ═══════════════════════════════════════════════════════════════════════
// Property computation: required_line detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn at_line_exact_sets_required_line() {
    let node =
        VimPatternNode::Sequence(vec![VimPatternNode::AtLine(LineSpec::Exact(42)), lit('x')]);
    let props = lower_to_props(&node);
    assert_eq!(props.required_line(), Some(42));
}

#[test]
fn at_line_before_sets_required_line_range() {
    let node =
        VimPatternNode::Sequence(vec![VimPatternNode::AtLine(LineSpec::Before(10)), lit('x')]);
    let props = lower_to_props(&node);
    assert_eq!(props.required_line_range(), Some((Ordering::Less, 10)));
}

#[test]
fn at_line_after_sets_required_line_range() {
    let node =
        VimPatternNode::Sequence(vec![VimPatternNode::AtLine(LineSpec::After(20)), lit('x')]);
    let props = lower_to_props(&node);
    assert_eq!(props.required_line_range(), Some((Ordering::Greater, 20)));
}

#[test]
fn at_line_current_no_required_line() {
    let node = VimPatternNode::AtLine(LineSpec::Current);
    let props = lower_to_props(&node);
    assert_eq!(props.required_line(), None);
    assert_eq!(props.required_line_range(), None);
}

#[test]
fn no_at_line_no_required_line() {
    let props = lower_to_props(&lit('x'));
    assert_eq!(props.required_line(), None);
    assert_eq!(props.required_line_range(), None);
}

// ═══════════════════════════════════════════════════════════════════════
// Property computation: is_literal
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn single_literal_is_literal() {
    let props = lower_to_props(&lit('a'));
    assert!(props.is_literal());
}

#[test]
fn literal_sequence_is_literal() {
    let node = VimPatternNode::Sequence(vec![lit('a'), lit('b')]);
    let props = lower_to_props(&node);
    assert!(props.is_literal());
}

#[test]
fn sequence_with_metachar_not_literal() {
    let node = VimPatternNode::Sequence(vec![lit('a'), VimPatternNode::AnyChar]);
    let props = lower_to_props(&node);
    assert!(!props.is_literal());
}

#[test]
fn quantifier_not_literal() {
    let node = VimPatternNode::Quantifier {
        node: Box::new(lit('a')),
        min: 0,
        max: None,
        greedy: true,
    };
    let props = lower_to_props(&node);
    assert!(!props.is_literal());
}

// ═══════════════════════════════════════════════════════════════════════
// Property computation: feature flags
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn backreference_sets_flag() {
    let props = lower_to_props(&VimPatternNode::BackReference(1));
    assert!(props.has_backreferences());
}

#[test]
fn lookaround_sets_flag() {
    let node = VimPatternNode::Lookaround {
        inner: Box::new(lit('a')),
        kind: LookaroundKind::PositiveAhead,
        limit: None,
    };
    let props = lower_to_props(&node);
    assert!(props.has_lookaround());
    assert!(!props.has_atomic());
}

#[test]
fn atomic_group_sets_both_flags() {
    let node = VimPatternNode::Lookaround {
        inner: Box::new(lit('a')),
        kind: LookaroundKind::Atomic,
        limit: None,
    };
    let props = lower_to_props(&node);
    assert!(props.has_lookaround());
    assert!(props.has_atomic());
}

#[test]
fn last_substitute_sets_flag() {
    let props = lower_to_props(&VimPatternNode::LastSubstitute);
    assert!(props.has_last_substitute());
}

#[test]
fn set_match_start_sets_flag() {
    let props = lower_to_props(&VimPatternNode::SetMatchStart);
    assert!(props.has_match_override());
}

#[test]
fn set_match_end_sets_flag() {
    let props = lower_to_props(&VimPatternNode::SetMatchEnd);
    assert!(props.has_match_override());
}

#[test]
fn cursor_position_sets_buffer_position() {
    let props = lower_to_props(&VimPatternNode::CursorPosition);
    assert!(props.has_buffer_position());
}

#[test]
fn visual_area_prefix_detected() {
    let node = VimPatternNode::Sequence(vec![VimPatternNode::VisualArea, lit('x')]);
    let props = lower_to_props(&node);
    assert!(props.has_visual_area_prefix());
    assert!(props.has_buffer_position());
}

#[test]
fn visual_area_not_prefix_when_not_first() {
    let node = VimPatternNode::Sequence(vec![lit('x'), VimPatternNode::VisualArea]);
    let props = lower_to_props(&node);
    assert!(!props.has_visual_area_prefix());
}

#[test]
fn capturing_group_increments_count() {
    let node = VimPatternNode::Group {
        inner: Box::new(lit('a')),
        capturing: true,
    };
    let props = lower_to_props(&node);
    assert_eq!(props.capture_count(), 1);
}

#[test]
fn non_capturing_group_no_increment() {
    let node = VimPatternNode::Group {
        inner: Box::new(lit('a')),
        capturing: false,
    };
    let props = lower_to_props(&node);
    assert_eq!(props.capture_count(), 0);
}

#[test]
fn nested_captures_accumulate() {
    let node = VimPatternNode::Sequence(vec![
        VimPatternNode::Group {
            inner: Box::new(lit('a')),
            capturing: true,
        },
        VimPatternNode::Group {
            inner: Box::new(lit('b')),
            capturing: true,
        },
    ]);
    let props = lower_to_props(&node);
    assert_eq!(props.capture_count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_sequence_lowered() {
    let node = VimPatternNode::Sequence(vec![]);
    let (lowered, props) = lower(&node);
    assert_eq!(lowered, LoweredNode::Sequence(vec![]));
    assert_eq!(props.minimum_match_len(), 0);
    assert_eq!(props.maximum_match_len(), Some(0));
    assert!(props.is_literal());
}

#[test]
fn single_node_sequence_unwraps() {
    let node = VimPatternNode::Sequence(vec![VimPatternNode::AnyChar]);
    assert_eq!(lower_to_node(&node), LoweredNode::AnyChar);
}

#[test]
fn deeply_nested_group() {
    let node = VimPatternNode::Group {
        inner: Box::new(VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Group {
                inner: Box::new(lit('z')),
                capturing: true,
            }),
            capturing: false,
        }),
        capturing: true,
    };
    let (lowered, props) = lower(&node);
    assert_eq!(props.capture_count(), 2);
    assert_eq!(props.minimum_match_len(), 1);
    // Verify structure
    if let LoweredNode::Group {
        inner: outer,
        capturing: true,
        ..
    } = &lowered
    {
        if let LoweredNode::Group {
            inner: mid,
            capturing: false,
            ..
        } = outer.as_ref()
        {
            if let LoweredNode::Group {
                inner: _,
                capturing: true,
                ..
            } = mid.as_ref()
            {
                // OK
            } else {
                panic!("expected innermost capturing group");
            }
        } else {
            panic!("expected middle non-capturing group");
        }
    } else {
        panic!("expected outermost capturing group");
    }
}

#[test]
fn optional_sequence_lowered() {
    let node = VimPatternNode::OptionalSequence(vec![lit('a'), lit('b')]);
    let (lowered, props) = lower(&node);
    assert_eq!(
        lowered,
        LoweredNode::OptionalSequence(vec![LoweredNode::Literal('a'), LoweredNode::Literal('b'),])
    );
    assert_eq!(props.minimum_match_len(), 0);
    assert_eq!(props.maximum_match_len(), Some(2));
}

#[test]
fn at_mark_lowered() {
    let node = VimPatternNode::AtMark {
        mark: 'a',
        rel: MarkRel::Before,
    };
    let (lowered, props) = lower(&node);
    assert_eq!(
        lowered,
        LoweredNode::AtMark {
            mark: 'a',
            rel: MarkRel::Before,
        }
    );
    assert!(props.has_buffer_position());
}

#[test]
fn at_column_lowered() {
    let node = VimPatternNode::AtColumn(ColumnSpec::Exact(5));
    let (lowered, props) = lower(&node);
    assert_eq!(lowered, LoweredNode::AtColumn(ColumnSpec::Exact(5)));
    assert!(props.has_buffer_position());
}

#[test]
fn at_virtual_column_lowered() {
    let node = VimPatternNode::AtVirtualColumn(ColumnSpec::Before(10));
    let (lowered, props) = lower(&node);
    assert_eq!(
        lowered,
        LoweredNode::AtVirtualColumn(ColumnSpec::Before(10))
    );
    assert!(props.has_buffer_position());
}

#[test]
fn quantifier_general_case() {
    let node = VimPatternNode::Quantifier {
        node: Box::new(lit('x')),
        min: 3,
        max: Some(7),
        greedy: false,
    };
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('x')),
            min: 3,
            max: Some(7),
            greedy: false,
        }
    );
}

#[test]
fn collection_pass_through() {
    let node = VimPatternNode::Collection {
        negated: true,
        items: vec![CollectionItem::Range('a', 'z'), CollectionItem::Newline],
        include_newline: true,
    };
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Collection {
            negated: true,
            items: vec![CollectionItem::Range('a', 'z'), CollectionItem::Newline],
            include_newline: true,
        }
    );
}

#[test]
fn start_of_file_anchored_as_single_node() {
    let props = lower_to_props(&VimPatternNode::StartOfFile);
    assert!(props.accel_hints.is_anchored_start);
    assert!(!props.accel_hints.is_anchored_end);
}

#[test]
fn end_of_file_anchored_as_single_node() {
    let props = lower_to_props(&VimPatternNode::EndOfFile);
    assert!(!props.accel_hints.is_anchored_start);
    assert!(props.accel_hints.is_anchored_end);
}

#[test]
fn feature_flags_propagate_through_sequence() {
    let node = VimPatternNode::Sequence(vec![
        VimPatternNode::BackReference(1),
        VimPatternNode::LastSubstitute,
        VimPatternNode::SetMatchStart,
        VimPatternNode::CursorPosition,
    ]);
    let props = lower_to_props(&node);
    assert!(props.has_backreferences());
    assert!(props.has_last_substitute());
    assert!(props.has_match_override());
    assert!(props.has_buffer_position());
}

#[test]
fn feature_flags_propagate_through_group() {
    let node = VimPatternNode::Group {
        inner: Box::new(VimPatternNode::BackReference(2)),
        capturing: true,
    };
    let props = lower_to_props(&node);
    assert!(props.has_backreferences());
    assert_eq!(props.capture_count(), 1);
}

#[test]
fn lookbehind_sets_lookaround_flag() {
    let node = VimPatternNode::Lookaround {
        inner: Box::new(lit('a')),
        kind: LookaroundKind::NegativeBehind,
        limit: Some(5),
    };
    let (lowered, props) = lower(&node);
    assert!(props.has_lookaround());
    assert!(!props.has_atomic());
    assert_eq!(
        lowered,
        LoweredNode::Lookaround {
            inner: Box::new(LoweredNode::Literal('a')),
            kind: LookaroundKind::NegativeBehind,
            limit: Some(5),
        }
    );
}

#[test]
fn anchored_start_of_file_vs_start_of_line() {
    let node = VimPatternNode::Sequence(vec![VimPatternNode::StartOfFile, lit('a')]);
    let (_, props) = lower(&node);
    assert!(props.accel_hints.is_anchored_start);
    assert!(props.is_anchored_start_of_file());

    let node = VimPatternNode::Sequence(vec![VimPatternNode::StartOfLine, lit('a')]);
    let (_, props) = lower(&node);
    assert!(props.accel_hints.is_anchored_start);
    assert!(!props.is_anchored_start_of_file());
}

#[test]
fn anchored_end_of_file_true_for_end_of_file() {
    // `foo\%$` — trailing EndOfFile
    let node = VimPatternNode::Sequence(vec![lit('f'), VimPatternNode::EndOfFile]);
    let props = lower_to_props(&node);
    assert!(props.accel_hints.is_anchored_end);
    assert!(props.is_anchored_end_of_file());
}

#[test]
fn anchored_end_of_file_false_for_end_of_line() {
    // `foo$` — trailing EndOfLine, not EndOfFile
    let node = VimPatternNode::Sequence(vec![lit('f'), VimPatternNode::EndOfLine]);
    let props = lower_to_props(&node);
    assert!(props.accel_hints.is_anchored_end);
    assert!(!props.is_anchored_end_of_file());
}

#[test]
fn anchored_end_of_file_single_node() {
    let props = lower_to_props(&VimPatternNode::EndOfFile);
    assert!(props.is_anchored_end_of_file());
}

#[test]
fn literal_suffix_extracts_trailing_literals() {
    // `.*foo` — suffix is "foo"
    let node =
        VimPatternNode::Sequence(vec![VimPatternNode::AnyChar, lit('f'), lit('o'), lit('o')]);
    let props = lower_to_props(&node);
    assert_eq!(props.literal_suffix().map(|s| s.as_str()), Some("foo"));
}

#[test]
fn literal_suffix_skips_trailing_end_of_line() {
    // `.*bar$` — suffix is "bar", skipping `$`
    let node = VimPatternNode::Sequence(vec![
        VimPatternNode::AnyChar,
        lit('b'),
        lit('a'),
        lit('r'),
        VimPatternNode::EndOfLine,
    ]);
    let props = lower_to_props(&node);
    assert_eq!(props.literal_suffix().map(|s| s.as_str()), Some("bar"));
}

#[test]
fn literal_suffix_skips_trailing_end_of_file() {
    // `.*baz\%$` — suffix is "baz", skipping `\%$`
    let node = VimPatternNode::Sequence(vec![
        VimPatternNode::AnyChar,
        lit('b'),
        lit('a'),
        lit('z'),
        VimPatternNode::EndOfFile,
    ]);
    let props = lower_to_props(&node);
    assert_eq!(props.literal_suffix().map(|s| s.as_str()), Some("baz"));
}

#[test]
fn literal_suffix_none_when_ending_with_non_literal() {
    // `foo.*` — no literal suffix
    let node = VimPatternNode::Sequence(vec![lit('f'), lit('o'), VimPatternNode::AnyChar]);
    let props = lower_to_props(&node);
    assert!(props.literal_suffix().is_none());
}

#[test]
fn literal_suffix_single_literal() {
    let props = lower_to_props(&lit('x'));
    assert_eq!(props.literal_suffix().map(|s| s.as_str()), Some("x"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// compute_common_prefix UTF-8 boundary tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn common_prefix_utf8_boundary_valid() {
    // "é" is 2 bytes (0xC3 0xA9). Prefix "éa" vs "éb":
    // - Byte 0: C3 == C3
    // - Byte 1: A9 == A9
    // - Byte 2: 'a' vs 'b' → common_len = 2 (valid: "é")
    let a = CompactString::from("éa");
    let b = CompactString::from("éb");
    let result = super::compute_common_prefix(&[Some(&a), Some(&b)]);
    assert_eq!(result.as_deref(), Some("é"));
}

#[test]
fn common_prefix_splits_mid_codepoint_2byte() {
    // "à" is 0xC3 0xA0, "á" is 0xC3 0xA1
    // Byte 0: C3 == C3
    // Byte 1: A0 != A1 → common_len = 1 (NOT a char boundary!)
    // After fix: rounds down to 0 → None
    let a = CompactString::from("à");
    let b = CompactString::from("á");
    let result = super::compute_common_prefix(&[Some(&a), Some(&b)]);
    assert_eq!(result, None);
}

#[test]
fn common_prefix_splits_mid_codepoint_3byte() {
    // "\u{4e00}" (一) = E4 B8 80, "\u{4e01}" (丁) = E4 B8 81
    // Byte 0: E4 == E4
    // Byte 1: B8 == B8
    // Byte 2: 80 != 81 → common_len = 2 (mid-codepoint!)
    // After fix: rounds down to 0 → None
    let a = CompactString::from("\u{4e00}");
    let b = CompactString::from("\u{4e01}");
    let result = super::compute_common_prefix(&[Some(&a), Some(&b)]);
    assert_eq!(result, None);
}

#[test]
fn common_prefix_valid_multibyte_cjk() {
    // "日本x" vs "日本y" → common prefix "日本" (6 bytes, valid boundary)
    let a = CompactString::from("日本x");
    let b = CompactString::from("日本y");
    let result = super::compute_common_prefix(&[Some(&a), Some(&b)]);
    assert_eq!(result.as_deref(), Some("日本"));
}

#[test]
fn common_prefix_splits_mid_codepoint_4byte() {
    // U+1F600 (😀) = F0 9F 98 80, U+1F601 (😁) = F0 9F 98 81
    // Bytes share first 3, differ at byte 3 → common_len = 3 (mid-codepoint!)
    // After fix: rounds down to 0 → None
    let a = CompactString::from("\u{1F600}");
    let b = CompactString::from("\u{1F601}");
    let result = super::compute_common_prefix(&[Some(&a), Some(&b)]);
    assert_eq!(result, None);
}

#[test]
fn common_prefix_ascii_no_regression() {
    // Plain ASCII prefix should still work fine.
    let a = CompactString::from("hello world");
    let b = CompactString::from("hello rust");
    let result = super::compute_common_prefix(&[Some(&a), Some(&b)]);
    assert_eq!(result.as_deref(), Some("hello "));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Lookaround flag propagation (regression test for the bug fixed by
// embedding FeatureFlags in NodeProps)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn lookaround_propagates_branch_and_flag() {
    // A \& (branch-and) inside a lookaround must propagate has_branch_and.
    // Previously, the Lookaround arm in lower_node used ..NodeProps::default()
    // which silently dropped has_branch_and from child props.
    let node = VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::BranchAnd(vec![lit('a'), lit('b')])),
        kind: LookaroundKind::PositiveAhead,
        limit: None,
    };
    let props = lower_to_props(&node);
    assert!(props.has_lookaround());
    assert!(
        props.has_branch_and(),
        "has_branch_and must propagate through lookaround"
    );
}

#[test]
fn lookaround_propagates_alternation_flag() {
    // Alternation inside a lookaround must propagate has_alternation.
    let node = VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Alternation(vec![
            VimPatternNode::Sequence(vec![lit('a'), lit('b')]),
            VimPatternNode::Sequence(vec![lit('c'), lit('d')]),
        ])),
        kind: LookaroundKind::PositiveAhead,
        limit: None,
    };
    let props = lower_to_props(&node);
    assert!(props.has_lookaround());
    assert!(
        props.has_alternation(),
        "has_alternation must propagate through lookaround"
    );
}

#[test]
fn lookaround_propagates_lazy_quantifier_flag() {
    // Lazy quantifier inside a lookaround must propagate has_lazy_quantifier.
    let node = VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Quantifier {
            node: Box::new(lit('x')),
            min: 0,
            max: None,
            greedy: false,
        }),
        kind: LookaroundKind::NegativeAhead,
        limit: None,
    };
    let props = lower_to_props(&node);
    assert!(props.has_lookaround());
    assert!(
        props.has_lazy_quantifier(),
        "has_lazy_quantifier must propagate through lookaround"
    );
}

#[test]
fn lookaround_propagates_zero_width_assertions_flag() {
    // Zero-width assertions inside lookaround must propagate.
    let node = VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Sequence(vec![
            VimPatternNode::WordBoundaryStart,
            lit('w'),
        ])),
        kind: LookaroundKind::PositiveBehind,
        limit: Some(10),
    };
    let props = lower_to_props(&node);
    assert!(props.has_lookaround());
    assert!(
        props.has_look_ahead_assertions(),
        "has_look_ahead_assertions must propagate through lookaround"
    );
}

#[test]
fn lookaround_propagates_all_child_flags_combined() {
    // Multiple flags inside a single lookaround: all must propagate.
    let inner = VimPatternNode::Sequence(vec![
        VimPatternNode::BackReference(1),
        VimPatternNode::BranchAnd(vec![lit('a'), lit('b')]),
        VimPatternNode::Quantifier {
            node: Box::new(lit('c')),
            min: 0,
            max: None,
            greedy: false,
        },
        VimPatternNode::CursorPosition,
    ]);
    let node = VimPatternNode::Lookaround {
        inner: Box::new(inner),
        kind: LookaroundKind::Atomic,
        limit: None,
    };
    let props = lower_to_props(&node);
    assert!(props.has_lookaround());
    assert!(props.has_atomic());
    assert!(props.has_backreferences());
    assert!(props.has_branch_and());
    assert!(props.has_lazy_quantifier());
    assert!(props.has_buffer_position());
}

// ═══════════════════════════════════════════════════════════════════════
// has_multiline feature flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multiline_flag_escape_newline() {
    let node = VimPatternNode::EscapeSequence(EscapeKind::Newline);
    let props = lower_to_props(&node);
    assert!(props.has_multiline());
}

#[test]
fn multiline_flag_escape_tab_is_not_multiline() {
    let node = VimPatternNode::EscapeSequence(EscapeKind::Tab);
    let props = lower_to_props(&node);
    assert!(!props.has_multiline());
}

#[test]
fn multiline_flag_any_char_nl() {
    let node = VimPatternNode::AnyCharNl;
    let props = lower_to_props(&node);
    assert!(props.has_multiline());
}

#[test]
fn multiline_flag_any_char_is_not_multiline() {
    let node = VimPatternNode::AnyChar;
    let props = lower_to_props(&node);
    assert!(!props.has_multiline());
}

#[test]
fn multiline_flag_class_with_newline() {
    let node = VimPatternNode::ClassWithNewline(CharClass::Digit);
    let props = lower_to_props(&node);
    assert!(props.has_multiline());
}

#[test]
fn multiline_flag_class_without_newline() {
    let node = VimPatternNode::Class(CharClass::Digit);
    let props = lower_to_props(&node);
    assert!(!props.has_multiline());
}

#[test]
fn multiline_flag_collection_with_newline() {
    let node = VimPatternNode::Collection {
        negated: false,
        items: vec![crate::ir::CollectionItem::Single('a')],
        include_newline: true,
    };
    let props = lower_to_props(&node);
    assert!(props.has_multiline());
}

#[test]
fn multiline_flag_collection_without_newline() {
    let node = VimPatternNode::Collection {
        negated: false,
        items: vec![crate::ir::CollectionItem::Single('a')],
        include_newline: false,
    };
    let props = lower_to_props(&node);
    assert!(!props.has_multiline());
}

#[test]
fn multiline_flag_propagates_through_sequence() {
    let node = VimPatternNode::Sequence(vec![
        lit('a'),
        VimPatternNode::EscapeSequence(EscapeKind::Newline),
        lit('b'),
    ]);
    let props = lower_to_props(&node);
    assert!(props.has_multiline());
}

#[test]
fn multiline_flag_not_set_for_normal_sequence() {
    let node = VimPatternNode::Sequence(vec![lit('a'), lit('b'), lit('c')]);
    let props = lower_to_props(&node);
    assert!(!props.has_multiline());
}

// ═══════════════════════════════════════════════════════════════════════
// Nested quantifier reduction (Oniguruma-style)
// ═══════════════════════════════════════════════════════════════════════

/// Helper: wrap `inner` in `Quantifier { min, max, greedy }`.
fn quant(inner: VimPatternNode, min: u32, max: Option<u32>, greedy: bool) -> VimPatternNode {
    VimPatternNode::Quantifier {
        node: Box::new(inner),
        min,
        max,
        greedy,
    }
}

// ── 9 standard greedy reductions ─────────────────────────────────────

#[test]
fn nested_question_question_reduces_to_question() {
    // a?? → a? — (0,1)(0,1) → (0,1)
    let node = quant(quant(lit('a'), 0, Some(1), true), 0, Some(1), true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: Some(1),
            greedy: true,
        }
    );
}

#[test]
fn nested_question_star_reduces_to_star() {
    // a?* → a* — (0,1)(0,N) → (0,N)
    let node = quant(quant(lit('a'), 0, Some(1), true), 0, None, true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        }
    );
}

#[test]
fn nested_question_plus_reduces_to_star() {
    // a?+ → a* — (0,1)(1,N) → (0,N)
    let node = quant(quant(lit('a'), 0, Some(1), true), 1, None, true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        }
    );
}

#[test]
fn nested_star_question_reduces_to_star() {
    // a*? → a* — (0,N)(0,1) → (0,N)
    let node = quant(quant(lit('a'), 0, None, true), 0, Some(1), true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        }
    );
}

#[test]
fn nested_star_star_reduces_to_star() {
    // a** → a* — (0,N)(0,N) → (0,N)
    let node = quant(quant(lit('a'), 0, None, true), 0, None, true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        }
    );
}

#[test]
fn nested_star_plus_reduces_to_star() {
    // a*+ → a* — (0,N)(1,N) → (0,N)
    let node = quant(quant(lit('a'), 0, None, true), 1, None, true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        }
    );
}

#[test]
fn nested_plus_question_reduces_to_star() {
    // a+? → a* — (1,N)(0,1) → (0,N)
    let node = quant(quant(lit('a'), 1, None, true), 0, Some(1), true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        }
    );
}

#[test]
fn nested_plus_star_reduces_to_star() {
    // a+* → a* — (1,N)(0,N) → (0,N)
    let node = quant(quant(lit('a'), 1, None, true), 0, None, true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        }
    );
}

#[test]
fn nested_plus_plus_reduces_to_plus() {
    // a++ → a+ — (1,N)(1,N) → (1,N)
    let node = quant(quant(lit('a'), 1, None, true), 1, None, true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 1,
            max: None,
            greedy: true,
        }
    );
}

// ── Exact × Exact ────────────────────────────────────────────────────

#[test]
fn nested_exact_three_times_exact_four_reduces_to_twelve() {
    // a{3}{4} → a{12}
    let node = quant(quant(lit('a'), 3, Some(3), true), 4, Some(4), true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 12,
            max: Some(12),
            greedy: true,
        }
    );
}

// ── No reduction: mixed greediness ───────────────────────────────────

#[test]
fn nested_mixed_greediness_no_reduction() {
    // greedy star wrapping lazy star → no reduction
    let node = quant(quant(lit('a'), 0, None, false), 0, None, true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Quantifier {
                node: Box::new(LoweredNode::Literal('a')),
                min: 0,
                max: None,
                greedy: false,
            }),
            min: 0,
            max: None,
            greedy: true,
        }
    );
}

// ── No reduction: non-exact range quantifiers ────────────────────────

#[test]
fn nested_range_quantifiers_no_reduction() {
    // a{2,5}{3,7} → no reduction (non-exact ranges)
    let node = quant(quant(lit('a'), 2, Some(5), true), 3, Some(7), true);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Quantifier {
                node: Box::new(LoweredNode::Literal('a')),
                min: 2,
                max: Some(5),
                greedy: true,
            }),
            min: 3,
            max: Some(7),
            greedy: true,
        }
    );
}

// ── Lazy reductions work too ─────────────────────────────────────────

#[test]
fn nested_lazy_question_question_reduces() {
    // a{-0,1}{-0,1} → a{-0,1} (lazy)
    let node = quant(quant(lit('a'), 0, Some(1), false), 0, Some(1), false);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: Some(1),
            greedy: false,
        }
    );
}

#[test]
fn nested_lazy_star_star_reduces() {
    // lazy a** → lazy a*
    let node = quant(quant(lit('a'), 0, None, false), 0, None, false);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: false,
        }
    );
}

#[test]
fn nested_lazy_plus_plus_reduces() {
    // lazy a++ → lazy a+
    let node = quant(quant(lit('a'), 1, None, false), 1, None, false);
    assert_eq!(
        lower_to_node(&node),
        LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 1,
            max: None,
            greedy: false,
        }
    );
}

// ═══════════════════════════════════════════════════════════════════════
// derive_has_multiline post-pass (end-to-end via VimRegex)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn has_multiline_alternation_collapse_with_newline() {
    use crate::VimRegex;
    let re = VimRegex::new(r"\n\|a").unwrap();
    assert!(
        re.features().has_multiline,
        r"\n\|a should have has_multiline=true"
    );
}

#[test]
fn has_multiline_char_by_code_newline() {
    use crate::VimRegex;
    let re = VimRegex::new(r"\%d10").unwrap();
    assert!(
        re.features().has_multiline,
        r"\%d10 should have has_multiline=true"
    );
}

#[test]
fn has_multiline_collection_with_newline_item() {
    use crate::VimRegex;
    let re = VimRegex::new(r"[\n]").unwrap();
    assert!(
        re.features().has_multiline,
        r"[\n] should have has_multiline=true"
    );
}

#[test]
fn has_multiline_hex_code_newline() {
    use crate::VimRegex;
    let re = VimRegex::new(r"\%x0a").unwrap();
    assert!(
        re.features().has_multiline,
        r"\%x0a should have has_multiline=true"
    );
}

#[test]
fn has_multiline_plain_literal_false() {
    use crate::VimRegex;
    let re = VimRegex::new(r"hello").unwrap();
    assert!(
        !re.features().has_multiline,
        "hello should NOT have has_multiline"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Nested exact quantifier min_len (double-counting fix)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_exact_quantifier_correct_min_len() {
    // a{3}{4} reduces to a{12}, min_len should be 12 (not 36)
    let node = quant(quant(lit('a'), 3, Some(3), true), 4, Some(4), true);
    let props = lower_to_props(&node);
    assert_eq!(
        props.minimum_match_len(),
        12,
        "a{{3}}{{4}} should have min_len=12"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Behavioral tests for "doubled quantifier" patterns in Vim's Magic mode.
//
// In Magic mode, a quantifier after another quantifier is always literal:
// `a**` → `a*` + literal `*`, `a\+\+` → `a\+` + literal `+`, etc.
// Vim never produces nested quantifiers from parsed patterns.
//
// The nested quantifier *reduction* logic (try_reduce_nested_quantifier) is
// tested structurally above using manually-constructed AST nodes. These tests
// verify the correct Vim parsing behavior for doubled-quantifier syntax.
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nested_star_star_behavioral() {
    // a** in Magic: `a*` + literal `*` — matches zero+ a's followed by '*'
    use crate::test_builder::regex;
    regex("a**").text("aaa*").expect_match(0..4).run();
}

#[test]
fn nested_star_star_behavioral_empty() {
    // a** in Magic: `a*` + literal `*` — no '*' in text, no match
    use crate::test_builder::regex;
    regex("a**").text("bbb").expect_no_match().run();
}

#[test]
fn nested_plus_plus_behavioral() {
    // a\+\+ in Magic: `a\+` + literal `+` — matches one+ a's followed by '+'
    use crate::test_builder::regex;
    regex(r"a\+\+").text("aaa+").expect_match(0..4).run();
}

#[test]
fn nested_plus_plus_behavioral_no_match() {
    // a\+\+ in Magic: `a\+` + literal `+` — no '+' in text, no match
    use crate::test_builder::regex;
    regex(r"a\+\+").text("bbb").expect_no_match().run();
}

#[test]
fn nested_question_question_behavioral() {
    // a\=\= in Magic: `a\=` + literal `=` — matches optional a followed by '='
    use crate::test_builder::regex;
    regex(r"a\=\=").text("a=").expect_match(0..2).run();
}

#[test]
fn nested_question_question_behavioral_empty() {
    // a\=\= in Magic: `a\=` + literal `=` — no '=' in text means no match
    use crate::test_builder::regex;
    regex(r"a\=\=").text("abc").expect_no_match().run();
}

#[test]
fn nested_question_star_behavioral() {
    // a\=* in Magic: `a\=` + literal `*` — matches optional a followed by '*'
    use crate::test_builder::regex;
    regex(r"a\=*").text("a*").expect_match(0..2).run();
}

#[test]
fn nested_question_plus_behavioral() {
    // a\=\+ in Magic: `a\=` + literal `+` — matches optional a followed by '+'
    use crate::test_builder::regex;
    regex(r"a\=\+").text("a+").expect_match(0..2).run();
}

#[test]
fn nested_star_question_behavioral() {
    // a*\= in Magic: `a*` + literal `=` — matches zero+ a's followed by '='
    use crate::test_builder::regex;
    regex(r"a*\=").text("aaa=").expect_match(0..4).run();
}

#[test]
fn nested_star_plus_behavioral() {
    // a*\+ in Magic: `a*` + literal `+` — matches zero+ a's followed by '+'
    use crate::test_builder::regex;
    regex(r"a*\+").text("aaa+").expect_match(0..4).run();
}

#[test]
fn nested_plus_question_behavioral() {
    // a\+\= in Magic: `a\+` + literal `=` — matches one+ a's followed by '='
    use crate::test_builder::regex;
    regex(r"a\+\=").text("aaa=").expect_match(0..4).run();
}

#[test]
fn nested_plus_star_behavioral() {
    // a\+* in Magic: `a\+` + literal `*` — matches one+ a's followed by '*'
    use crate::test_builder::regex;
    regex(r"a\+*").text("aaa*").expect_match(0..4).run();
}

#[test]
fn nested_exact_three_four_behavioral() {
    // a\{3}\{4} in Magic: the second \{ enters parse_atom (not
    // try_parse_quantifier) so it becomes Literal('{') via magic inversion.
    // Result: a\{3} + Literal('{') + Literal('4') + Literal('}')
    // Matches exactly 3 a's followed by literal "{4}".
    use crate::test_builder::regex;
    regex(r"a\{3}\{4}").text("aaa{4}").expect_match(0..6).run();
}

#[test]
fn nested_exact_three_four_behavioral_no_match() {
    // a\{3}\{4} = `a\{3}` + literal "{4}" — text without "{4}" suffix fails
    use crate::test_builder::regex;
    regex(r"a\{3}\{4}")
        .text("aaaaaaaaaaa")
        .expect_no_match()
        .run();
}

// ═══════════════════════════════════════════════════════════════════════
// NoMagic and VeryNoMagic behavioral tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn nomagic_literal_dot() {
    // In NoMagic, `.` is literal — matches a literal dot
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex(".")
        .magic(MagicMode::NoMagic)
        .text("a.b")
        .expect_match(1..2)
        .run();
}

#[test]
fn nomagic_escaped_dot_is_wildcard() {
    // In NoMagic, `\.` is the wildcard (any char)
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex(r"\.")
        .magic(MagicMode::NoMagic)
        .text("abc")
        .expect_match(0..1)
        .run();
}

#[test]
fn nomagic_star_is_literal() {
    // In NoMagic, `*` is literal — matches a literal asterisk
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex("a*")
        .magic(MagicMode::NoMagic)
        .text("a*b")
        .expect_match(0..2)
        .run();
}

#[test]
fn nomagic_escaped_star_is_quantifier() {
    // In NoMagic, `\*` is the Kleene star quantifier
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex(r"a\*")
        .magic(MagicMode::NoMagic)
        .text("aaa")
        .expect_match(0..3)
        .run();
}

#[test]
fn nomagic_caret_still_special() {
    // In NoMagic, `^` is still start-of-line
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex("^a")
        .magic(MagicMode::NoMagic)
        .text("abc")
        .expect_match(0..1)
        .run();
}

#[test]
fn nomagic_dollar_still_special() {
    // In NoMagic, `$` is still end-of-line
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex("c$")
        .magic(MagicMode::NoMagic)
        .text("abc")
        .expect_match(2..3)
        .run();
}

#[test]
fn verynomagic_everything_literal() {
    // In VeryNoMagic, `.` `*` `[` are all literal
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex("a.b")
        .magic(MagicMode::VeryNoMagic)
        .text("a.b")
        .expect_match(0..3)
        .run();
}

#[test]
fn verynomagic_dot_does_not_match_other() {
    // In VeryNoMagic, `.` is literal, not wildcard
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex("a.b")
        .magic(MagicMode::VeryNoMagic)
        .text("axb")
        .expect_no_match()
        .run();
}

#[test]
fn verynomagic_star_is_literal() {
    // In VeryNoMagic, `*` is literal
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex("a*")
        .magic(MagicMode::VeryNoMagic)
        .text("a*")
        .expect_match(0..2)
        .run();
}

#[test]
fn verynomagic_escaped_dot_is_wildcard() {
    // In VeryNoMagic, `\.` is wildcard
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex(r"\.")
        .magic(MagicMode::VeryNoMagic)
        .text("x")
        .expect_match(0..1)
        .run();
}

#[test]
fn verynomagic_caret_is_literal() {
    // In VeryNoMagic, `^` is literal (unlike NoMagic)
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex("^a")
        .magic(MagicMode::VeryNoMagic)
        .text("^a")
        .expect_match(0..2)
        .run();
}

#[test]
fn verynomagic_backslash_escapes_work() {
    // In VeryNoMagic, `\d` still matches digits
    use crate::test_builder::regex;
    use crate::MagicMode;
    regex(r"\d")
        .magic(MagicMode::VeryNoMagic)
        .text("a1b")
        .expect_match(1..2)
        .run();
}
