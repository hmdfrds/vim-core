//! Required-byte extraction (PCRE2-style).
//!
//! Identifies the last fixed literal byte that must appear in every match
//! of a pattern. Used for fast-rejection: if memchr cannot find this byte
//! in the remaining text, the entire text can be skipped.

use crate::hir::LoweredNode;

/// Extract the last fixed literal byte that must appear in any match.
///
/// Returns `Some((byte, case_insensitive))` where `byte` is the raw byte
/// value and `case_insensitive` indicates whether the pattern might match
/// the byte's case-folded variant.
///
/// The extraction walks the HIR tree to find the *last* unconditionally
/// required literal byte. "Unconditionally required" means the byte appears
/// on every possible match path (not inside an alternation with a different
/// literal, not inside an optional quantifier).
///
/// Returns `None` if no such byte can be determined.
pub(crate) fn extract_required_byte(root: &LoweredNode) -> Option<(u8, bool)> {
    // Walk from the end of the pattern backward to find the last mandatory literal byte.
    extract_last_mandatory_byte(root)
}

/// Walk backward through the HIR to find the last mandatory literal byte.
fn extract_last_mandatory_byte(node: &LoweredNode) -> Option<(u8, bool)> {
    match node {
        LoweredNode::Literal(ch) => {
            if ch.is_ascii() {
                Some((*ch as u8, false))
            } else {
                // Multi-byte UTF-8: use the last byte of the encoding.
                let mut buf = [0u8; 4];
                let encoded = ch.encode_utf8(&mut buf);
                Some((encoded.as_bytes()[encoded.len() - 1], false))
            }
        }
        LoweredNode::LiteralString(s) => s.as_bytes().last().map(|&b| (b, false)),
        LoweredNode::Sequence(children) => {
            // Walk children in reverse, skipping zero-width assertions at the end.
            for child in children.iter().rev() {
                if is_zero_width_for_required(child) {
                    continue;
                }
                return extract_last_mandatory_byte(child);
            }
            None
        }
        LoweredNode::Group { inner, .. } => extract_last_mandatory_byte(inner),
        LoweredNode::Quantifier { min, node, .. } if *min > 0 => extract_last_mandatory_byte(node),
        // Alternation: only if ALL branches end with the same required byte.
        LoweredNode::Alternation(branches) => {
            if branches.is_empty() {
                return None;
            }
            let first = extract_last_mandatory_byte(&branches[0])?;
            for branch in &branches[1..] {
                let other = extract_last_mandatory_byte(branch)?;
                if other.0 != first.0 {
                    return None;
                }
            }
            Some(first)
        }
        // Everything else: cannot determine a required byte.
        _ => None,
    }
}

/// Check whether a node is zero-width (consumes no input) for required byte purposes.
fn is_zero_width_for_required(node: &LoweredNode) -> bool {
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
    fn single_ascii_literal() {
        let node = LoweredNode::Literal('x');
        assert_eq!(extract_required_byte(&node), Some((b'x', false)));
    }

    #[test]
    fn literal_string_returns_last_byte() {
        let node = LoweredNode::LiteralString(CompactString::from("hello"));
        assert_eq!(extract_required_byte(&node), Some((b'o', false)));
    }

    #[test]
    fn sequence_returns_last_literal_byte() {
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::AnyChar,
            LoweredNode::LiteralString(CompactString::from("bar")),
        ]);
        assert_eq!(extract_required_byte(&node), Some((b'r', false)));
    }

    #[test]
    fn sequence_skips_trailing_zero_width() {
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Literal('a'),
            LoweredNode::LiteralString(CompactString::from("bc")),
            LoweredNode::EndOfLine,
        ]);
        assert_eq!(extract_required_byte(&node), Some((b'c', false)));
    }

    #[test]
    fn any_char_returns_none() {
        assert_eq!(extract_required_byte(&LoweredNode::AnyChar), None);
    }

    #[test]
    fn optional_quantifier_returns_none() {
        let node = LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('x')),
            min: 0,
            max: None,
            greedy: true,
        };
        assert_eq!(extract_required_byte(&node), None);
    }

    #[test]
    fn required_quantifier_extracts_byte() {
        let node = LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('z')),
            min: 2,
            max: Some(5),
            greedy: true,
        };
        assert_eq!(extract_required_byte(&node), Some((b'z', false)));
    }

    #[test]
    fn alternation_same_last_byte() {
        let node = LoweredNode::Alternation(vec![
            LoweredNode::LiteralString(CompactString::from("foo")),
            LoweredNode::LiteralString(CompactString::from("boo")),
        ]);
        assert_eq!(extract_required_byte(&node), Some((b'o', false)));
    }

    #[test]
    fn alternation_different_last_bytes_returns_none() {
        let node =
            LoweredNode::Alternation(vec![LoweredNode::Literal('x'), LoweredNode::Literal('y')]);
        assert_eq!(extract_required_byte(&node), None);
    }

    #[test]
    fn group_unwraps() {
        let node = LoweredNode::Group {
            inner: Box::new(LoweredNode::Literal('g')),
            capturing: true,
            group: Some(crate::nfa::CaptureGroup::from_one_based(1)),
        };
        assert_eq!(extract_required_byte(&node), Some((b'g', false)));
    }

    #[test]
    fn multibyte_char_returns_last_utf8_byte() {
        // '€' = U+20AC = 0xE2 0x82 0xAC
        let node = LoweredNode::Literal('\u{20AC}');
        let result = extract_required_byte(&node).unwrap();
        assert_eq!(result.0, 0xAC);
    }

    #[test]
    fn empty_alternation_returns_none() {
        let node = LoweredNode::Alternation(vec![]);
        assert_eq!(extract_required_byte(&node), None);
    }

    #[test]
    fn sequence_all_zero_width_returns_none() {
        let node = LoweredNode::Sequence(vec![LoweredNode::StartOfLine, LoweredNode::EndOfLine]);
        assert_eq!(extract_required_byte(&node), None);
    }
}
