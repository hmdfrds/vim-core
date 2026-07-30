//! Parser tests for `\&` (branch-and operator).

use crate::ir::{VimPatternNode, VimRegexError};
use crate::parser::parse_pattern;

fn parse(pattern: &str) -> Result<VimPatternNode, VimRegexError> {
    parse_pattern(pattern).map(|r| r.node)
}

// ═══════════════════════════════════════════════════════════════════════
// Basic structure tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn branch_and_two_branches() {
    let result = parse("a\\&b").unwrap();
    assert!(matches!(result, VimPatternNode::BranchAnd(ref v) if v.len() == 2));
}

#[test]
fn branch_and_three_branches() {
    let result = parse("a\\&b\\&c").unwrap();
    assert!(matches!(result, VimPatternNode::BranchAnd(ref v) if v.len() == 3));
}

#[test]
fn branch_and_preserves_inner_nodes() {
    let result = parse("a\\&b").unwrap();
    match result {
        VimPatternNode::BranchAnd(branches) => {
            assert_eq!(branches[0], VimPatternNode::Literal('a'));
            assert_eq!(branches[1], VimPatternNode::Literal('b'));
        }
        _ => panic!("expected BranchAnd, got {result:?}"),
    }
}

#[test]
fn branch_and_with_sequences() {
    let result = parse("ab\\&cd").unwrap();
    match result {
        VimPatternNode::BranchAnd(branches) => {
            assert_eq!(branches.len(), 2);
            assert_eq!(
                branches[0],
                VimPatternNode::Sequence(vec![
                    VimPatternNode::Literal('a'),
                    VimPatternNode::Literal('b'),
                ])
            );
            assert_eq!(
                branches[1],
                VimPatternNode::Sequence(vec![
                    VimPatternNode::Literal('c'),
                    VimPatternNode::Literal('d'),
                ])
            );
        }
        _ => panic!("expected BranchAnd, got {result:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Precedence tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn branch_and_lower_precedence_than_concatenation() {
    // "ab\&cd" should be BranchAnd(["ab", "cd"]), not a different grouping
    let result = parse("ab\\&cd").unwrap();
    assert!(matches!(result, VimPatternNode::BranchAnd(_)));
}

#[test]
fn alternation_lower_precedence_than_branch_and() {
    // "a\&b\|c" → Alternation([BranchAnd([a, b]), c])
    let result = parse("a\\&b\\|c").unwrap();
    match result {
        VimPatternNode::Alternation(branches) => {
            assert_eq!(branches.len(), 2);
            assert!(matches!(branches[0], VimPatternNode::BranchAnd(ref v) if v.len() == 2));
            assert_eq!(branches[1], VimPatternNode::Literal('c'));
        }
        _ => panic!("expected Alternation, got {result:?}"),
    }
}

#[test]
fn alternation_lower_precedence_reversed() {
    // "c\|a\&b" → Alternation([c, BranchAnd([a, b])])
    let result = parse("c\\|a\\&b").unwrap();
    match result {
        VimPatternNode::Alternation(branches) => {
            assert_eq!(branches.len(), 2);
            assert_eq!(branches[0], VimPatternNode::Literal('c'));
            assert!(matches!(branches[1], VimPatternNode::BranchAnd(ref v) if v.len() == 2));
        }
        _ => panic!("expected Alternation, got {result:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Groups
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn branch_and_inside_group() {
    let result = parse("\\(a\\&b\\)").unwrap();
    match result {
        VimPatternNode::Group { inner, capturing } => {
            assert!(capturing);
            assert!(matches!(*inner, VimPatternNode::BranchAnd(ref v) if v.len() == 2));
        }
        _ => panic!("expected Group, got {result:?}"),
    }
}

#[test]
fn branch_and_inside_non_capturing_group() {
    let result = parse("\\%(a\\&b\\)").unwrap();
    match result {
        VimPatternNode::Group { inner, capturing } => {
            assert!(!capturing);
            assert!(matches!(*inner, VimPatternNode::BranchAnd(ref v) if v.len() == 2));
        }
        _ => panic!("expected Group, got {result:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Feature flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn branch_and_sets_feature_flag() {
    let result = parse_pattern("a\\&b").unwrap();
    assert!(result.features.has_branch_and);
}

#[test]
fn no_branch_and_feature_flag_when_absent() {
    let result = parse_pattern("abc").unwrap();
    assert!(!result.features.has_branch_and);
}

#[test]
fn alternation_without_branch_and_has_no_flag() {
    let result = parse_pattern("a\\|b").unwrap();
    assert!(!result.features.has_branch_and);
}

// ═══════════════════════════════════════════════════════════════════════
// Dollar-sign anchoring before \&
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dollar_before_branch_and_is_anchor() {
    // "$\&^" — $ is anchor because it's followed by \&
    let result = parse("$\\&^").unwrap();
    match result {
        VimPatternNode::BranchAnd(branches) => {
            assert_eq!(branches[0], VimPatternNode::EndOfLine);
            assert_eq!(branches[1], VimPatternNode::StartOfLine);
        }
        _ => panic!("expected BranchAnd, got {result:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Single branch degenerate case
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn branch_and_with_quantifier_and_literal() {
    // \d\+\&foo — should be BranchAnd([Quantifier(\d, 1+), Sequence(f,o,o)])
    let result = parse("\\d\\+\\&foo").unwrap();
    match result {
        VimPatternNode::BranchAnd(branches) => {
            assert_eq!(branches.len(), 2);
            // First branch should be a Quantifier wrapping \d with min=1
            assert!(
                matches!(&branches[0], VimPatternNode::Quantifier { min: 1, .. }),
                "first branch should be Quantifier, got {:?}",
                branches[0]
            );
            // Second branch should be "foo" sequence
            assert_eq!(
                branches[1],
                VimPatternNode::Sequence(vec![
                    VimPatternNode::Literal('f'),
                    VimPatternNode::Literal('o'),
                    VimPatternNode::Literal('o'),
                ])
            );
        }
        _ => panic!("expected BranchAnd, got {result:?}"),
    }
}
