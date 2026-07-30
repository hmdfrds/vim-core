//! Prefilter trees (RE2-style).
//!
//! Builds AND/OR trees of required literals for complex patterns.
//! `foo.*bar` -> `And(["foo", "bar"])`
//! `(foo|bar).*baz` -> `And([Or(["foo", "bar"]), "baz"])`
//!
//! The tree's `is_satisfied` method checks whether all required
//! literals are present in a text, enabling fast-reject before
//! the full match attempt.

use compact_str::CompactString;

use crate::hir::LoweredNode;

/// A node in a prefilter tree representing required literal constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrefilterNode {
    /// A single literal that must appear in the text.
    Literal(CompactString),
    /// All children must be satisfied (conjunction).
    And(Vec<PrefilterNode>),
    /// At least one child must be satisfied (disjunction).
    Or(Vec<PrefilterNode>),
}

impl PrefilterNode {
    /// Check whether the text satisfies this prefilter constraint.
    ///
    /// For `Literal`: uses `memmem` to check if the literal appears in the text.
    /// For `And`: all children must be satisfied.
    /// For `Or`: at least one child must be satisfied.
    pub(crate) fn is_satisfied(&self, text: &str) -> bool {
        match self {
            PrefilterNode::Literal(lit) => {
                memchr::memmem::find(text.as_bytes(), lit.as_bytes()).is_some()
            }
            PrefilterNode::And(children) => children.iter().all(|c| c.is_satisfied(text)),
            PrefilterNode::Or(children) => children.iter().any(|c| c.is_satisfied(text)),
        }
    }

    /// Returns `true` if this is a trivially true node (empty And/no constraints).
    #[allow(dead_code, reason = "used for optimization decisions in compile.rs")]
    pub(crate) fn is_trivial(&self) -> bool {
        match self {
            PrefilterNode::And(children) => children.is_empty(),
            PrefilterNode::Or(children) => children.is_empty(),
            PrefilterNode::Literal(s) => s.is_empty(),
        }
    }

    /// Count the number of literal leaves in the tree.
    #[allow(dead_code, reason = "used for cost estimation")]
    pub(crate) fn literal_count(&self) -> usize {
        match self {
            PrefilterNode::Literal(_) => 1,
            PrefilterNode::And(children) | PrefilterNode::Or(children) => {
                children.iter().map(|c| c.literal_count()).sum()
            }
        }
    }
}

/// Build a prefilter tree from a lowered HIR node.
///
/// Extracts mandatory literals from the pattern structure:
/// - Top-level `Sequence`: AND of mandatory literals from each child
/// - `Alternation`: OR of literals from each branch
/// - `Group`, `Quantifier(min>0)`: recurse into inner
///
/// Returns `None` if fewer than 2 required literals are found (not worth
/// the overhead of tree evaluation for a single literal, which is already
/// handled by the existing prefilter/required_byte infrastructure).
pub(crate) fn build_prefilter_tree(root: &LoweredNode) -> Option<PrefilterNode> {
    let node = extract_prefilter_node(root)?;
    // Only worthwhile if the tree has >= 2 literals.
    if node.literal_count() < 2 {
        return None;
    }
    // Flatten trivial wrappers.
    Some(flatten_prefilter_node(node))
}

/// Extract a prefilter node from a single HIR node.
fn extract_prefilter_node(node: &LoweredNode) -> Option<PrefilterNode> {
    match node {
        LoweredNode::Literal(ch) => {
            Some(PrefilterNode::Literal(CompactString::from(ch.to_string())))
        }
        LoweredNode::LiteralString(s) if !s.is_empty() => Some(PrefilterNode::Literal(s.clone())),
        LoweredNode::Sequence(children) => {
            let mut parts: Vec<PrefilterNode> = Vec::new();
            for child in children {
                if let Some(pf) = extract_prefilter_node(child) {
                    parts.push(pf);
                }
            }
            match parts.len() {
                0 => None,
                1 => Some(parts.into_iter().next().expect("checked non-empty")),
                _ => Some(PrefilterNode::And(parts)),
            }
        }
        LoweredNode::Alternation(branches) => {
            let mut parts: Vec<PrefilterNode> = Vec::new();
            for branch in branches {
                match extract_prefilter_node(branch) {
                    Some(pf) => parts.push(pf),
                    None => return None, // Can't create OR if any branch has no literal
                }
            }
            match parts.len() {
                0 => None,
                1 => Some(parts.into_iter().next().expect("checked non-empty")),
                _ => Some(PrefilterNode::Or(parts)),
            }
        }
        LoweredNode::Group { inner, .. } => extract_prefilter_node(inner),
        LoweredNode::Quantifier { min, node, .. } if *min > 0 => extract_prefilter_node(node),
        // Zero-width nodes, optional quantifiers, etc.: no extractable literal.
        _ => None,
    }
}

/// Flatten single-child And/Or nodes.
fn flatten_prefilter_node(node: PrefilterNode) -> PrefilterNode {
    match node {
        PrefilterNode::And(mut children) => {
            children = children.into_iter().map(flatten_prefilter_node).collect();
            // Flatten nested And nodes.
            let mut flat: Vec<PrefilterNode> = Vec::new();
            for child in children {
                match child {
                    PrefilterNode::And(grandchildren) => flat.extend(grandchildren),
                    other => flat.push(other),
                }
            }
            if flat.len() == 1 {
                flat.into_iter().next().expect("checked non-empty")
            } else {
                PrefilterNode::And(flat)
            }
        }
        PrefilterNode::Or(mut children) => {
            children = children.into_iter().map(flatten_prefilter_node).collect();
            if children.len() == 1 {
                children.into_iter().next().expect("checked non-empty")
            } else {
                PrefilterNode::Or(children)
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::LoweredNode;
    use compact_str::CompactString;

    // -- is_satisfied ---------------------------------------------------------

    #[test]
    fn literal_satisfied() {
        let pf = PrefilterNode::Literal(CompactString::from("foo"));
        assert!(pf.is_satisfied("hello foo bar"));
        assert!(!pf.is_satisfied("hello bar"));
    }

    #[test]
    fn and_satisfied() {
        let pf = PrefilterNode::And(vec![
            PrefilterNode::Literal(CompactString::from("foo")),
            PrefilterNode::Literal(CompactString::from("bar")),
        ]);
        assert!(pf.is_satisfied("foo and bar"));
        assert!(!pf.is_satisfied("foo only"));
        assert!(!pf.is_satisfied("bar only"));
    }

    #[test]
    fn or_satisfied() {
        let pf = PrefilterNode::Or(vec![
            PrefilterNode::Literal(CompactString::from("foo")),
            PrefilterNode::Literal(CompactString::from("bar")),
        ]);
        assert!(pf.is_satisfied("just foo"));
        assert!(pf.is_satisfied("just bar"));
        assert!(!pf.is_satisfied("neither"));
    }

    #[test]
    fn nested_and_or() {
        // AND(OR("foo", "bar"), "baz")
        let pf = PrefilterNode::And(vec![
            PrefilterNode::Or(vec![
                PrefilterNode::Literal(CompactString::from("foo")),
                PrefilterNode::Literal(CompactString::from("bar")),
            ]),
            PrefilterNode::Literal(CompactString::from("baz")),
        ]);
        assert!(pf.is_satisfied("foo and baz"));
        assert!(pf.is_satisfied("bar and baz"));
        assert!(!pf.is_satisfied("foo only"));
        assert!(!pf.is_satisfied("baz only"));
    }

    // -- build_prefilter_tree -------------------------------------------------

    #[test]
    fn sequence_with_two_literals_builds_and() {
        // foo.*bar -> AND("foo", "bar")
        let node = LoweredNode::Sequence(vec![
            LoweredNode::LiteralString(CompactString::from("foo")),
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 0,
                max: None,
                greedy: true,
            },
            LoweredNode::LiteralString(CompactString::from("bar")),
        ]);
        let tree = build_prefilter_tree(&node).unwrap();
        assert_eq!(
            tree,
            PrefilterNode::And(vec![
                PrefilterNode::Literal(CompactString::from("foo")),
                PrefilterNode::Literal(CompactString::from("bar")),
            ])
        );
    }

    #[test]
    fn alternation_builds_or_inside_and() {
        // (foo|bar).*baz -> AND(OR("foo", "bar"), "baz")
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Alternation(vec![
                LoweredNode::LiteralString(CompactString::from("foo")),
                LoweredNode::LiteralString(CompactString::from("bar")),
            ]),
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 0,
                max: None,
                greedy: true,
            },
            LoweredNode::LiteralString(CompactString::from("baz")),
        ]);
        let tree = build_prefilter_tree(&node).unwrap();
        assert_eq!(
            tree,
            PrefilterNode::And(vec![
                PrefilterNode::Or(vec![
                    PrefilterNode::Literal(CompactString::from("foo")),
                    PrefilterNode::Literal(CompactString::from("bar")),
                ]),
                PrefilterNode::Literal(CompactString::from("baz")),
            ])
        );
    }

    #[test]
    fn single_literal_returns_none() {
        // Single literal: not worth a tree (handled by prefilter/required_byte).
        let node = LoweredNode::LiteralString(CompactString::from("hello"));
        assert_eq!(build_prefilter_tree(&node), None);
    }

    #[test]
    fn no_extractable_literal_returns_none() {
        let node = LoweredNode::AnyChar;
        assert_eq!(build_prefilter_tree(&node), None);
    }

    #[test]
    fn alternation_with_non_literal_branch_returns_none() {
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Alternation(vec![
                LoweredNode::LiteralString(CompactString::from("foo")),
                LoweredNode::AnyChar,
            ]),
            LoweredNode::LiteralString(CompactString::from("bar")),
        ]);
        // The OR node can't be built because one branch has no literal.
        // But the Sequence still finds "bar", which is only 1 literal -> None.
        assert_eq!(build_prefilter_tree(&node), None);
    }

    // -- is_satisfied integration ---------------------------------------------

    #[test]
    fn built_tree_rejects_text_missing_required_literal() {
        // foo.*bar
        let node = LoweredNode::Sequence(vec![
            LoweredNode::LiteralString(CompactString::from("foo")),
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 0,
                max: None,
                greedy: true,
            },
            LoweredNode::LiteralString(CompactString::from("bar")),
        ]);
        let tree = build_prefilter_tree(&node).unwrap();
        assert!(tree.is_satisfied("foo something bar"));
        assert!(!tree.is_satisfied("foo something baz"));
        assert!(!tree.is_satisfied("boo something bar"));
    }
}
