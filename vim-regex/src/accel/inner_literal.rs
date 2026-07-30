//! Inner literal ("regmust") extraction from lowered regex trees.
//!
//! Extracts the longest mandatory literal that appears after the first
//! sub-expression in a top-level Sequence. This literal can be used as
//! a secondary prefilter: if it does not appear in the text, the pattern
//! cannot match.

use compact_str::CompactString;

use crate::hir::LoweredNode;

// ═══════════════════════════════════════════════════════════════════════════════
// INNER LITERAL EXTRACTION
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract the longest mandatory interior literal from a lowered regex tree.
///
/// Walks the top-level `Sequence` (if any), skipping the first sub-expression
/// (which is already handled by the prefix prefilter). Returns the longest
/// `Literal`/`LiteralString` chain found in subsequent sub-expressions that
/// is unconditionally required (not inside an alternation, optional quantifier,
/// or other conditional construct).
///
/// Returns `None` if the root is not a `Sequence`, or no mandatory literal
/// is found after the first element.
pub(crate) fn extract_inner_literal(root: &LoweredNode) -> Option<CompactString> {
    let children = match root {
        LoweredNode::Sequence(children) if children.len() >= 2 => children,
        _ => return None,
    };

    // Skip the first element (it's the prefix, already covered by the prefilter).
    let mut best: Option<CompactString> = None;

    for child in &children[1..] {
        if let Some(lit) = extract_mandatory_literal(child) {
            let is_longer = best.as_ref().is_none_or(|b| lit.len() > b.len());
            if is_longer {
                best = Some(lit);
            }
        }
    }

    best
}

/// Inner literal with its position in the top-level Sequence.
///
/// `split_index` is the index of the child in the top-level `Sequence`
/// that contains the literal. The prefix sub-tree is `children[0..split_index]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InnerLiteral {
    /// The extracted mandatory literal string.
    #[allow(
        dead_code,
        reason = "the ReverseInner cascade consumes only split_index; the literal text itself is carried for Debug output and is what this module's tests assert on"
    )]
    pub literal: CompactString,
    /// Index of the child in the top-level Sequence where this literal lives.
    pub split_index: usize,
}

/// Extract the longest mandatory interior literal with its position.
///
/// Like [`extract_inner_literal`], but also returns the index of the Sequence
/// child containing the literal — needed to build a prefix-reverse NFA for the
/// [`Strategy::ReverseInner`] cascade step, which scans backward from a literal
/// hit to find the true match start.
///
/// [`Strategy::ReverseInner`]: crate::engine::strategy::Strategy::ReverseInner
pub(crate) fn extract_inner_literal_with_position(root: &LoweredNode) -> Option<InnerLiteral> {
    let children = match root {
        LoweredNode::Sequence(children) if children.len() >= 2 => children,
        _ => return None,
    };

    let mut best: Option<(CompactString, usize)> = None;

    for (i, child) in children[1..].iter().enumerate() {
        if let Some(lit) = extract_mandatory_literal(child) {
            let is_longer = best.as_ref().is_none_or(|b| lit.len() > b.0.len());
            if is_longer {
                best = Some((lit, i + 1));
            }
        }
    }

    best.map(|(literal, split_index)| InnerLiteral {
        literal,
        split_index,
    })
}

/// Extract a mandatory literal from a single node.
///
/// Only returns a literal if the node unconditionally requires it — does not
/// descend into alternations, optional quantifiers, or other structures where
/// the literal might be skipped.
fn extract_mandatory_literal(node: &LoweredNode) -> Option<CompactString> {
    match node {
        LoweredNode::Literal(ch) => Some(CompactString::from(ch.to_string())),
        LoweredNode::LiteralString(s) => Some(s.clone()),
        LoweredNode::Group { inner, .. } => extract_mandatory_literal(inner),
        LoweredNode::Quantifier { min, node, .. } if *min > 0 => extract_mandatory_literal(node),
        LoweredNode::Sequence(children) => {
            // Within a nested sequence, find the longest mandatory literal.
            let mut best: Option<CompactString> = None;
            for child in children {
                if let Some(lit) = extract_mandatory_literal(child) {
                    let is_longer = best.as_ref().is_none_or(|b| lit.len() > b.len());
                    if is_longer {
                        best = Some(lit);
                    }
                }
            }
            best
        }
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_with_inner_literal() {
        // Pattern like: a.foo → inner literal "foo"
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::AnyChar,
            LoweredNode::LiteralString("foo".into()),
        ]);
        assert_eq!(
            extract_inner_literal(&root),
            Some(CompactString::from("foo"))
        );
    }

    #[test]
    fn sequence_picks_longest_literal() {
        // Pattern: a.xy.hello → picks "hello" (longest)
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::AnyChar,
            LoweredNode::LiteralString("xy".into()),
            LoweredNode::AnyChar,
            LoweredNode::LiteralString("hello".into()),
        ]);
        assert_eq!(
            extract_inner_literal(&root),
            Some(CompactString::from("hello"))
        );
    }

    #[test]
    fn non_sequence_returns_none() {
        assert_eq!(extract_inner_literal(&LoweredNode::Literal('a')), None);
        assert_eq!(extract_inner_literal(&LoweredNode::AnyChar), None);
    }

    #[test]
    fn single_element_sequence_returns_none() {
        let root = LoweredNode::Sequence(vec![LoweredNode::Literal('a')]);
        assert_eq!(extract_inner_literal(&root), None);
    }

    #[test]
    fn all_literal_sequence_extracts_after_first() {
        // "abc" → first element is 'a', inner literal is "bc"
        // (but since they're separate nodes after lowering, it'll find 'b' and 'c' separately)
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::LiteralString("bc".into()),
        ]);
        assert_eq!(
            extract_inner_literal(&root),
            Some(CompactString::from("bc"))
        );
    }

    #[test]
    fn skips_non_literal_nodes() {
        // a . b → inner literal is "b"
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::AnyChar,
            LoweredNode::Literal('b'),
        ]);
        assert_eq!(extract_inner_literal(&root), Some(CompactString::from("b")));
    }

    #[test]
    fn no_mandatory_literal_after_first() {
        // a . . → no inner literal
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::AnyChar,
            LoweredNode::AnyChar,
        ]);
        assert_eq!(extract_inner_literal(&root), None);
    }

    #[test]
    fn optional_quantifier_not_extracted() {
        // a .* b → 'b' is after an optional quantifier but 'b' itself is mandatory
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 0,
                max: None,
                greedy: true,
            },
            LoweredNode::Literal('b'),
        ]);
        assert_eq!(extract_inner_literal(&root), Some(CompactString::from("b")));
    }

    #[test]
    fn literal_inside_group_extracted() {
        // a \(foo\) → inner literal "foo"
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::Group {
                inner: Box::new(LoweredNode::LiteralString("foo".into())),
                capturing: true,
                group: Some(crate::nfa::CaptureGroup::from_one_based(1)),
            },
        ]);
        assert_eq!(
            extract_inner_literal(&root),
            Some(CompactString::from("foo"))
        );
    }

    #[test]
    fn literal_inside_required_quantifier_extracted() {
        // a x{2,5} → inner literal "x"
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::Literal('x')),
                min: 2,
                max: Some(5),
                greedy: true,
            },
        ]);
        assert_eq!(extract_inner_literal(&root), Some(CompactString::from("x")));
    }

    #[test]
    fn empty_sequence_returns_none() {
        let root = LoweredNode::Sequence(vec![]);
        assert_eq!(extract_inner_literal(&root), None);
    }

    #[test]
    fn alternation_not_extracted() {
        // a \(x\|y\) → alternation is not a mandatory literal
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::Alternation(vec![LoweredNode::Literal('x'), LoweredNode::Literal('y')]),
        ]);
        assert_eq!(extract_inner_literal(&root), None);
    }

    #[test]
    fn with_position_basic_split_index() {
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::AnyChar,
            LoweredNode::LiteralString("foo".into()),
        ]);
        let info = extract_inner_literal_with_position(&root).unwrap();
        assert_eq!(info.literal, "foo");
        assert_eq!(info.split_index, 2);
    }

    #[test]
    fn with_position_picks_longest_with_correct_index() {
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::AnyChar,
            LoweredNode::LiteralString("xy".into()),
            LoweredNode::AnyChar,
            LoweredNode::LiteralString("hello".into()),
        ]);
        let info = extract_inner_literal_with_position(&root).unwrap();
        assert_eq!(info.literal, "hello");
        assert_eq!(info.split_index, 4);
    }

    #[test]
    fn with_position_second_element_literal() {
        let root = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::LiteralString("bc".into()),
        ]);
        let info = extract_inner_literal_with_position(&root).unwrap();
        assert_eq!(info.literal, "bc");
        assert_eq!(info.split_index, 1);
    }

    #[test]
    fn with_position_non_sequence_returns_none() {
        assert!(extract_inner_literal_with_position(&LoweredNode::Literal('a')).is_none());
    }

    #[test]
    fn with_position_single_element_returns_none() {
        let root = LoweredNode::Sequence(vec![LoweredNode::Literal('a')]);
        assert!(extract_inner_literal_with_position(&root).is_none());
    }
}
