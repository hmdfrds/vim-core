//! Start bitmap — 256-bit set of possible match-starting bytes.
//!
//! Derived from the sound `StartSet` over-approximation of a `LoweredNode`
//! tree. Used by the engine to skip positions whose first byte cannot begin a
//! match.

use crate::hir::{LoweredNode, StartSet};
use crate::ir::CaseMode;

// ═══════════════════════════════════════════════════════════════════════════════
// COMPUTATION FROM LOWERED NODE
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute a 256-bit start bitmap from a lowered HIR node, derived from the
/// sound `StartSet` over-approximation.
///
/// Returns `None` when no useful filtering is possible:
/// - `StartSet::Anywhere` (nullable/unknown-first/universal start), or
/// - a `Constrained` set that is universal or whose start bitmap is all-ones.
///
/// The bitmap representation is `[u32; 8]` where bit `i` of word `i/32`
/// indicates byte `i` can start a match. `CharSet::as_start_bitmap` is already
/// sound for non-ASCII sets (it sets the UTF-8 lead-byte range 128–255), so no
/// pure-ASCII guard is needed here (unlike the `to_byte_vec` byte prefilter).
pub(crate) fn compute_start_bitmap(node: &LoweredNode, case_mode: CaseMode) -> Option<[u32; 8]> {
    match StartSet::of(node, case_mode) {
        StartSet::Anywhere => None,
        StartSet::Constrained(set) => {
            if set.is_universal() {
                return None;
            }
            let bitmap = set.as_start_bitmap();
            if bitmap.iter().all(|&word| word == 0xFFFF_FFFF) {
                return None;
            }
            Some(bitmap)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── compute_start_bitmap (StartSet-derived) ──────────────────────

    fn bit_set(bitmap: &[u32; 8], b: u8) -> bool {
        bitmap[b as usize >> 5] & (1 << (b & 31)) != 0
    }

    #[test]
    fn start_bitmap_literal() {
        let bitmap = compute_start_bitmap(&LoweredNode::Literal('x'), CaseMode::Sensitive).unwrap();
        assert!(bit_set(&bitmap, b'x'));
    }

    #[test]
    fn start_bitmap_any_char_returns_none() {
        assert!(compute_start_bitmap(&LoweredNode::AnyChar, CaseMode::Sensitive).is_none());
    }

    #[test]
    fn start_bitmap_alternation() {
        let node =
            LoweredNode::Alternation(vec![LoweredNode::Literal('a'), LoweredNode::Literal('b')]);
        let bitmap = compute_start_bitmap(&node, CaseMode::Sensitive).unwrap();
        assert!(bit_set(&bitmap, b'a'));
        assert!(bit_set(&bitmap, b'b'));
        assert!(!bit_set(&bitmap, b'c'));
    }

    #[test]
    fn start_bitmap_nullable_prefix_unions_successor() {
        // a*b — the nullable a* must not exclude 'b' from the start bitmap.
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::Literal('a')),
                min: 0,
                max: None,
                greedy: true,
            },
            LoweredNode::Literal('b'),
        ]);
        let bitmap = compute_start_bitmap(&node, CaseMode::Sensitive).unwrap();
        assert!(bit_set(&bitmap, b'a'));
        assert!(bit_set(&bitmap, b'b'));
    }

    #[test]
    fn start_bitmap_standalone_nullable_returns_none() {
        // a* alone is nullable → Anywhere → no bitmap.
        let node = LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        };
        assert!(compute_start_bitmap(&node, CaseMode::Sensitive).is_none());
    }
}
