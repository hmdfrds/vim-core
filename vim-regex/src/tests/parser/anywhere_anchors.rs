//! Parser tests for `\_^` and `\_$` (anywhere anchors).

use crate::ir::{VimPatternNode, VimRegexError};
use crate::parser::parse_pattern;

fn parse(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_pattern(pattern).map(|r| r.node)
}

// ═══════════════════════════════════════════════════════════════════════
// Basic parsing: \_^ and \_$ produce the correct nodes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn anywhere_start_of_line_standalone() {
    assert_eq!(parse("\\_^"), Ok(VimPatternNode::AnywhereStartOfLine));
}

#[test]
fn anywhere_end_of_line_standalone() {
    assert_eq!(parse("\\_$"), Ok(VimPatternNode::AnywhereEndOfLine));
}

// ═══════════════════════════════════════════════════════════════════════
// In sequences: these are NOT subject to context-sensitivity rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn anywhere_start_in_sequence() {
    // \_^foo — start-of-line assertion then "foo"
    let result = parse("\\_^foo").unwrap();
    assert_eq!(
        result,
        VimPatternNode::Sequence(vec![
            VimPatternNode::AnywhereStartOfLine,
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
        ])
    );
}

#[test]
fn anywhere_end_in_sequence() {
    // foo\_$ — "foo" then end-of-line assertion
    let result = parse("foo\\_$").unwrap();
    assert_eq!(
        result,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
            VimPatternNode::AnywhereEndOfLine,
        ])
    );
}

#[test]
fn anywhere_start_mid_pattern_not_literal() {
    // foo\_^bar — \_^ is always an anchor regardless of position
    let result = parse("foo\\_^bar").unwrap();
    assert_eq!(
        result,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
            VimPatternNode::AnywhereStartOfLine,
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('r'),
        ])
    );
}

#[test]
fn anywhere_end_mid_pattern_not_literal() {
    // foo\_$bar — \_$ is always an anchor regardless of position
    let result = parse("foo\\_$bar").unwrap();
    assert_eq!(
        result,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
            VimPatternNode::AnywhereEndOfLine,
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('r'),
        ])
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Inside groups — works like any other atom
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn anywhere_start_inside_group() {
    let result = parse("\\(\\_^foo\\)").unwrap();
    match result {
        VimPatternNode::Group { inner, capturing } => {
            assert!(capturing);
            assert_eq!(
                *inner,
                VimPatternNode::Sequence(vec![
                    VimPatternNode::AnywhereStartOfLine,
                    VimPatternNode::Literal('f'),
                    VimPatternNode::Literal('o'),
                    VimPatternNode::Literal('o'),
                ])
            );
        }
        _ => panic!("expected Group, got {result:?}"),
    }
}

#[test]
fn anywhere_end_inside_group() {
    let result = parse("\\(foo\\_$\\)").unwrap();
    match result {
        VimPatternNode::Group { inner, capturing } => {
            assert!(capturing);
            assert_eq!(
                *inner,
                VimPatternNode::Sequence(vec![
                    VimPatternNode::Literal('f'),
                    VimPatternNode::Literal('o'),
                    VimPatternNode::Literal('o'),
                    VimPatternNode::AnywhereEndOfLine,
                ])
            );
        }
        _ => panic!("expected Group, got {result:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Contrast with bare ^ and $ (which ARE context-sensitive)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn bare_caret_mid_pattern_is_literal() {
    // In magic mode, ^ mid-pattern becomes literal
    let result = parse("foo^bar").unwrap();
    assert_eq!(
        result,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('^'),
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('r'),
        ])
    );
}

#[test]
fn bare_dollar_mid_pattern_is_literal() {
    // In magic mode, $ mid-pattern becomes literal
    let result = parse("foo$bar").unwrap();
    assert_eq!(
        result,
        VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('f'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('o'),
            VimPatternNode::Literal('$'),
            VimPatternNode::Literal('b'),
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('r'),
        ])
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Combined usage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn both_anywhere_anchors_in_one_pattern() {
    // \_$\_^ — end-of-line followed by start-of-line (matches newline boundary)
    let result = parse("\\_$\\_^").unwrap();
    assert_eq!(
        result,
        VimPatternNode::Sequence(vec![
            VimPatternNode::AnywhereEndOfLine,
            VimPatternNode::AnywhereStartOfLine,
        ])
    );
}

#[test]
fn anywhere_start_after_alternation() {
    // a\|\_^b — alternation with \_^ in second branch
    let result = parse("a\\|\\_^b").unwrap();
    match result {
        VimPatternNode::Alternation(branches) => {
            assert_eq!(branches.len(), 2);
            assert_eq!(branches[0], VimPatternNode::Literal('a'));
            assert_eq!(
                branches[1],
                VimPatternNode::Sequence(vec![
                    VimPatternNode::AnywhereStartOfLine,
                    VimPatternNode::Literal('b'),
                ])
            );
        }
        _ => panic!("expected Alternation, got {result:?}"),
    }
}
