//! Suffix literal extraction for regex patterns.
//!
//! Extracts the longest literal suffix from the lowered HIR tree.
//! This is the complement of prefix extraction: while prefixes guide
//! forward search, suffixes guide backward search and enable fast
//! rejection for end-anchored patterns.
//!
//! Distinct from `PatternProperties::literal_suffix` (computed during lowering
//! in `hir.rs::compute_literal_suffix`), this extraction operates on the full
//! lowered tree and handles deeper nesting (groups, required quantifiers,
//! nested sequences).

use compact_str::CompactString;

use crate::hir::LoweredNode;

/// Extract the longest literal suffix from the lowered HIR tree.
///
/// Walks backward through the tree, skipping zero-width assertions,
/// collecting literal characters from the end of the pattern.
///
/// Returns `None` if no suffix literal can be extracted.
pub(crate) fn extract_suffix_literal(node: &LoweredNode) -> Option<CompactString> {
    let mut suffix = CompactString::new("");
    collect_suffix(node, &mut suffix);
    if suffix.is_empty() {
        None
    } else {
        Some(suffix)
    }
}

/// Recursively collect literal characters from the end of a node.
///
/// Appends characters to `suffix` in reverse order, then the caller
/// gets the correct order because we process children from back to front
/// and prepend.
fn collect_suffix(node: &LoweredNode, suffix: &mut CompactString) {
    match node {
        LoweredNode::Literal(ch) => {
            let mut new = CompactString::from(ch.to_string());
            new.push_str(suffix);
            *suffix = new;
        }
        LoweredNode::LiteralString(s) => {
            let mut new = s.clone();
            new.push_str(suffix);
            *suffix = new;
        }
        LoweredNode::Sequence(children) => {
            for child in children.iter().rev() {
                if is_zero_width_for_suffix(child) {
                    continue;
                }
                // Try to extract suffix from this child.
                collect_suffix(child, suffix);
                // If this child is a literal, continue to collect more
                // (previous siblings might also be literals).
                if !is_literal_node(child) {
                    break;
                }
            }
        }
        LoweredNode::Group { inner, .. } => collect_suffix(inner, suffix),
        LoweredNode::Quantifier { min, node, .. } if *min > 0 => {
            collect_suffix(node, suffix);
        }
        // For alternation: only extract if all branches share the same suffix.
        LoweredNode::Alternation(branches) => {
            if branches.is_empty() {
                return;
            }
            let first = extract_suffix_literal(&branches[0]);
            let first = match first {
                Some(s) => s,
                None => return,
            };
            for branch in &branches[1..] {
                let other = extract_suffix_literal(branch);
                match other {
                    Some(s) if s == first => continue,
                    _ => return,
                }
            }
            let mut new = first;
            new.push_str(suffix);
            *suffix = new;
        }
        _ => {}
    }
}

/// Check whether a node is a literal (single char or literal string).
fn is_literal_node(node: &LoweredNode) -> bool {
    matches!(
        node,
        LoweredNode::Literal(_) | LoweredNode::LiteralString(_)
    )
}

/// Check whether a node is zero-width for suffix extraction purposes.
fn is_zero_width_for_suffix(node: &LoweredNode) -> bool {
    matches!(
        node,
        LoweredNode::StartOfLine
            | LoweredNode::EndOfLine
            | LoweredNode::AnywhereStartOfLine
            | LoweredNode::AnywhereEndOfLine
            | LoweredNode::StartOfFile
            | LoweredNode::EndOfFile
            | LoweredNode::WordBoundaryStart
            | LoweredNode::WordBoundaryEnd
            | LoweredNode::SetMatchStart
            | LoweredNode::SetMatchEnd
            | LoweredNode::CursorPosition
            | LoweredNode::VisualArea
            | LoweredNode::AtLine(_)
            | LoweredNode::AtColumn(_)
            | LoweredNode::AtVirtualColumn(_)
            | LoweredNode::AtMark { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::LoweredNode;
    use compact_str::CompactString;

    #[test]
    fn single_literal() {
        let node = LoweredNode::Literal('x');
        assert_eq!(
            extract_suffix_literal(&node),
            Some(CompactString::from("x"))
        );
    }

    #[test]
    fn literal_string() {
        let node = LoweredNode::LiteralString(CompactString::from("hello"));
        assert_eq!(
            extract_suffix_literal(&node),
            Some(CompactString::from("hello"))
        );
    }

    #[test]
    fn sequence_trailing_literals() {
        let node = LoweredNode::Sequence(vec![
            LoweredNode::AnyChar,
            LoweredNode::LiteralString(CompactString::from("bar")),
        ]);
        assert_eq!(
            extract_suffix_literal(&node),
            Some(CompactString::from("bar"))
        );
    }

    #[test]
    fn sequence_trailing_literals_with_zero_width() {
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::LiteralString(CompactString::from("bc")),
            LoweredNode::EndOfLine,
        ]);
        assert_eq!(
            extract_suffix_literal(&node),
            Some(CompactString::from("abc"))
        );
    }

    #[test]
    fn sequence_no_trailing_literal() {
        let node = LoweredNode::Sequence(vec![LoweredNode::Literal('a'), LoweredNode::AnyChar]);
        assert_eq!(extract_suffix_literal(&node), None);
    }

    #[test]
    fn any_char_returns_none() {
        assert_eq!(extract_suffix_literal(&LoweredNode::AnyChar), None);
    }

    #[test]
    fn group_unwraps() {
        let node = LoweredNode::Group {
            inner: Box::new(LoweredNode::Literal('g')),
            capturing: true,
            group: Some(crate::nfa::CaptureGroup::from_one_based(1)),
        };
        assert_eq!(
            extract_suffix_literal(&node),
            Some(CompactString::from("g"))
        );
    }

    #[test]
    fn required_quantifier() {
        let node = LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('z')),
            min: 2,
            max: Some(5),
            greedy: true,
        };
        assert_eq!(
            extract_suffix_literal(&node),
            Some(CompactString::from("z"))
        );
    }

    #[test]
    fn optional_quantifier_returns_none() {
        let node = LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('z')),
            min: 0,
            max: None,
            greedy: true,
        };
        assert_eq!(extract_suffix_literal(&node), None);
    }

    #[test]
    fn alternation_same_suffix() {
        let node = LoweredNode::Alternation(vec![
            LoweredNode::LiteralString(CompactString::from("foo")),
            LoweredNode::LiteralString(CompactString::from("foo")),
        ]);
        assert_eq!(
            extract_suffix_literal(&node),
            Some(CompactString::from("foo"))
        );
    }

    #[test]
    fn alternation_different_suffix_returns_none() {
        let node =
            LoweredNode::Alternation(vec![LoweredNode::Literal('x'), LoweredNode::Literal('y')]);
        assert_eq!(extract_suffix_literal(&node), None);
    }

    #[test]
    fn empty_sequence_returns_none() {
        let node = LoweredNode::Sequence(vec![]);
        assert_eq!(extract_suffix_literal(&node), None);
    }

    #[test]
    fn multiple_trailing_literals_fused() {
        let node = LoweredNode::Sequence(vec![
            LoweredNode::AnyChar,
            LoweredNode::Literal('x'),
            LoweredNode::Literal('y'),
            LoweredNode::Literal('z'),
        ]);
        assert_eq!(
            extract_suffix_literal(&node),
            Some(CompactString::from("xyz"))
        );
    }
}
