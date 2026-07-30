//! Auto-possessification pass for greedy quantifiers.
//!
//! Walks a `LoweredNode` tree and promotes greedy quantifiers to possessive
//! (atomic) when the quantifier body's first-char set is disjoint from the
//! successor's first-char set.  This eliminates backtracking overhead for
//! patterns like `\w\+:` (word chars can never match `:`) without changing
//! match semantics.
//!
//! # Safety
//!
//! The pass is conservative: it only promotes when the disjointness is provably
//! true.  False negatives (skipping a safe promotion) are allowed.  False
//! positives (promoting when overlap exists) would change semantics and are
//! forbidden.
//!
//! # Limitations
//!
//! - Quantifiers inside lookaround bodies are not promoted (the atomic group
//!   would change the lookaround's backtracking semantics).
//! - Lazy (non-greedy) quantifiers are never promoted — they have different
//!   matching semantics where possessification could change results.
//! - Bodies that may match zero characters are not promoted (`\(\w*\)\+` — the
//!   inner `\w*` can match empty, so the outer quantifier interacts differently).

use super::charset::classes_are_disjoint;
use super::start_set::{can_match_empty, StartSet};
use super::LoweredNode;
use crate::ir::{CaseMode, CollectionItem, LookaroundKind};

// ═══════════════════════════════════════════════════════════════════════════════
// PUBLIC ENTRY POINT
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the auto-possessification pass over a lowered HIR tree.
///
/// Mutates the tree in place, wrapping eligible greedy quantifiers in
/// `Lookaround { kind: Atomic }` where safe.
///
/// Returns `true` if any quantifier was promoted (the tree now contains
/// `Atomic` nodes that were not there before).
pub(crate) fn auto_possessify(node: &mut LoweredNode, case_mode: CaseMode) -> bool {
    walk(node, false, case_mode)
}

// ═══════════════════════════════════════════════════════════════════════════════
// TREE WALK
// ═══════════════════════════════════════════════════════════════════════════════

/// Recursively walk the node tree, applying possessification to sequences.
/// Returns `true` if any promotion was performed in this subtree.
fn walk(node: &mut LoweredNode, inside_lookaround: bool, case_mode: CaseMode) -> bool {
    match node {
        LoweredNode::Sequence(children) => {
            let mut changed = possessify_sequence(children, inside_lookaround, case_mode);
            for child in children.iter_mut() {
                changed |= walk(child, inside_lookaround, case_mode);
            }
            changed
        }

        LoweredNode::Alternation(branches) | LoweredNode::BranchAnd(branches) => {
            let mut changed = false;
            for branch in branches.iter_mut() {
                changed |= walk(branch, inside_lookaround, case_mode);
            }
            changed
        }

        LoweredNode::OptionalSequence(children) => {
            let mut changed = false;
            for child in children.iter_mut() {
                changed |= walk(child, inside_lookaround, case_mode);
            }
            changed
        }

        LoweredNode::Group { inner, .. } => walk(inner.as_mut(), inside_lookaround, case_mode),

        LoweredNode::Quantifier { node: inner, .. } => {
            walk(inner.as_mut(), inside_lookaround, case_mode)
        }

        LoweredNode::Lookaround { inner, .. } => walk(inner.as_mut(), true, case_mode),

        // Leaf nodes — nothing to do.
        LoweredNode::Literal(_)
        | LoweredNode::LiteralString(_)
        | LoweredNode::AnyChar
        | LoweredNode::AnyCharNl
        | LoweredNode::Collection { .. }
        | LoweredNode::BackReference(_)
        | LoweredNode::LastSubstitute
        | LoweredNode::StartOfLine
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
        | LoweredNode::AtMark { .. } => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SEQUENCE POSSESSIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Scan a sequence's children and promote eligible greedy quantifiers.
///
/// For each pair `(children[i], children[i+1])` where `children[i]` is a
/// greedy quantifier: if the quantifier body always consumes input and
/// the body's first-char set is disjoint from the successor's first-char
/// set, wrap the ENTIRE quantifier in an atomic group.
///
/// We wrap the whole quantifier (not just its body) so the backtracker
/// runs the greedy quantifier to completion inside the atomic scope and
/// then commits.  This avoids the PikeVM limitation where a Lookaround
/// inside a quantifier body would be treated as zero-width.
fn possessify_sequence(
    children: &mut [LoweredNode],
    inside_lookaround: bool,
    case_mode: CaseMode,
) -> bool {
    if inside_lookaround {
        return false;
    }

    let len = children.len();
    if len < 2 {
        return false;
    }

    let mut changed = false;

    for i in 0..len - 1 {
        // Split the slice so we can borrow children[i] mutably and
        // children[i+1] immutably at the same time.
        let (left, right) = children.split_at_mut(i + 1);
        let current = &mut left[i];
        let successor = &right[0];

        if let LoweredNode::Quantifier {
            node: body,
            greedy: true,
            ..
        } = current
        {
            if can_match_empty(body) {
                continue;
            }

            // Never possessify a quantifier whose body contains a capturing
            // group: wrapping it in an atomic group moves the capture inside a
            // lookaround sub-NFA, whose internal `Save` markers are not
            // propagated to the main match. That would silently drop captures
            // (e.g. `\(a\)\{2}\(b\)` losing group 1). The repeat-count benefit
            // of possessification does not justify changing capture semantics.
            if body_contains_capture(body) {
                continue;
            }

            // Fast path: O(1) disjointness table lookup for single-class
            // quantifier body vs single-class or single-literal successor.
            if let Some(disjoint) = try_fast_disjoint(body, successor) {
                if disjoint {
                    let quantifier = std::mem::replace(current, LoweredNode::Sequence(vec![]));
                    *current = LoweredNode::Lookaround {
                        inner: Box::new(quantifier),
                        kind: LookaroundKind::Atomic,
                        limit: None,
                    };
                    changed = true;
                }
                continue;
            }

            // General path: compute full start sets and check disjointness.
            // `Anywhere` on either side means we cannot prove disjointness, so
            // decline the promotion.
            let (StartSet::Constrained(body_set), StartSet::Constrained(succ_set)) = (
                StartSet::of(body, case_mode),
                StartSet::of(successor, case_mode),
            ) else {
                continue;
            };

            // If the body can match ANY character, it can never be disjoint
            // from any successor — skip the disjointness check entirely.
            if body_set.is_universal() {
                continue;
            }

            if body_set.is_disjoint(&succ_set) {
                // Promote: wrap the entire quantifier node in Atomic.
                let quantifier = std::mem::replace(current, LoweredNode::Sequence(vec![]));
                *current = LoweredNode::Lookaround {
                    inner: Box::new(quantifier),
                    kind: LookaroundKind::Atomic,
                    limit: None,
                };
                changed = true;
            }
        }
    }

    changed
}

// ═══════════════════════════════════════════════════════════════════════════════
// FAST DISJOINTNESS — O(1) TABLE LOOKUP
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract the single `CharClass` from a node, if the node is a `Collection`
/// with exactly one `Class(class)` item (no negation, no newline override).
fn extract_single_class(node: &LoweredNode) -> Option<crate::ir::CharClass> {
    match node {
        LoweredNode::Collection {
            negated: false,
            items,
            include_newline: false,
        } if items.len() == 1 => match items[0] {
            CollectionItem::Class(class) => Some(class),
            _ => None,
        },
        _ => None,
    }
}

/// Try O(1) disjointness via the precomputed class table.
///
/// Returns `Some(true)` if provably disjoint, `Some(false)` if provably
/// overlapping, or `None` if the fast path is not applicable (caller
/// should fall through to the general `CharSet` path).
fn try_fast_disjoint(body: &LoweredNode, successor: &LoweredNode) -> Option<bool> {
    let body_class = extract_single_class(body)?;
    let succ_class = extract_single_class(successor)?;
    classes_are_disjoint(body_class, succ_class)
}

// ═══════════════════════════════════════════════════════════════════════════════
// BODY ANALYSIS
// ═══════════════════════════════════════════════════════════════════════════════

/// Returns `true` if the subtree contains a capturing `Group`.
///
/// Used to keep possessification from wrapping a capturing group in an atomic
/// group, which would route the capture through a lookaround sub-NFA and lose
/// its `Save` markers. Walks every structural child so a capture nested at any
/// depth (sequence, alternation, quantifier, etc.) is detected.
fn body_contains_capture(node: &LoweredNode) -> bool {
    match node {
        LoweredNode::Group {
            inner, capturing, ..
        } => *capturing || body_contains_capture(inner),
        LoweredNode::Quantifier { node: inner, .. } | LoweredNode::Lookaround { inner, .. } => {
            body_contains_capture(inner)
        }
        LoweredNode::Sequence(children)
        | LoweredNode::Alternation(children)
        | LoweredNode::BranchAnd(children)
        | LoweredNode::OptionalSequence(children) => children.iter().any(body_contains_capture),
        LoweredNode::Literal(_)
        | LoweredNode::LiteralString(_)
        | LoweredNode::AnyChar
        | LoweredNode::AnyCharNl
        | LoweredNode::Collection { .. }
        | LoweredNode::BackReference(_)
        | LoweredNode::LastSubstitute
        | LoweredNode::StartOfLine
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
        | LoweredNode::AtMark { .. } => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::lower;
    use crate::ir::LookaroundKind;
    use crate::parser::parse_pattern;

    /// Parse a Vim regex string, lower to HIR, run auto_possessify, return the result.
    fn possessify_pattern(pattern: &str) -> LoweredNode {
        let result = parse_pattern(pattern).expect("parse should succeed");
        let (mut lowered, _props) = lower(&result.node);
        auto_possessify(&mut lowered, result.case_mode);
        lowered
    }

    /// Check whether a node tree contains any Atomic lookaround.
    fn contains_atomic(node: &LoweredNode) -> bool {
        match node {
            LoweredNode::Lookaround {
                kind: LookaroundKind::Atomic,
                ..
            } => true,
            LoweredNode::Sequence(children)
            | LoweredNode::Alternation(children)
            | LoweredNode::BranchAnd(children)
            | LoweredNode::OptionalSequence(children) => children.iter().any(contains_atomic),
            LoweredNode::Group { inner, .. }
            | LoweredNode::Quantifier { node: inner, .. }
            | LoweredNode::Lookaround { inner, .. } => contains_atomic(inner),
            _ => false,
        }
    }

    /// Count the number of Atomic lookarounds in a node tree.
    fn count_atomic(node: &LoweredNode) -> usize {
        match node {
            LoweredNode::Lookaround {
                kind: LookaroundKind::Atomic,
                inner,
                ..
            } => 1 + count_atomic(inner),
            LoweredNode::Sequence(children)
            | LoweredNode::Alternation(children)
            | LoweredNode::BranchAnd(children)
            | LoweredNode::OptionalSequence(children) => children.iter().map(count_atomic).sum(),
            LoweredNode::Group { inner, .. }
            | LoweredNode::Quantifier { node: inner, .. }
            | LoweredNode::Lookaround { inner, .. } => count_atomic(inner),
            _ => 0,
        }
    }

    // ─── Positive cases: should possessify ──────────────────────────────

    #[test]
    fn word_plus_colon_is_possessified() {
        // \w\+: → word chars are disjoint from ':'
        let node = possessify_pattern(r"\w\+:");
        assert!(
            contains_atomic(&node),
            r"expected \w\+: to be possessified, got: {node:?}"
        );
    }

    #[test]
    fn digit_plus_whitespace_is_possessified() {
        // \d\+\s → digits are disjoint from whitespace
        let node = possessify_pattern(r"\d\+\s");
        assert!(
            contains_atomic(&node),
            r"expected \d\+\s to be possessified, got: {node:?}"
        );
    }

    #[test]
    fn digit_plus_dot_digit_plus_chain() {
        // \d\+\.\d\+ → first \d\+ possessified (digits vs '.'), second NOT (no successor)
        let node = possessify_pattern(r"\d\+\.\d\+");
        assert_eq!(
            count_atomic(&node),
            1,
            r"expected exactly 1 atomic in \d\+\.\d\+, got: {node:?}"
        );
    }

    #[test]
    fn word_plus_colon_inside_group_is_possessified() {
        // \(\w\+:\) → should still possessify inside the group
        let node = possessify_pattern(r"\(\w\+:\)");
        assert!(
            contains_atomic(&node),
            r"expected \(\w\+:\) to be possessified, got: {node:?}"
        );
    }

    // ─── Negative cases: should NOT possessify ──────────────────────────

    #[test]
    fn word_plus_digit_not_possessified() {
        // \w\+\d → word contains digits, NOT disjoint
        let node = possessify_pattern(r"\w\+\d");
        assert!(
            !contains_atomic(&node),
            r"expected \w\+\d to NOT be possessified, got: {node:?}"
        );
    }

    #[test]
    fn lazy_word_plus_colon_not_possessified() {
        // \w\{-1,}: → lazy quantifier, never possessify
        let node = possessify_pattern(r"\w\{-1,}:");
        assert!(
            !contains_atomic(&node),
            r"expected lazy \w\{{-1,}}: to NOT be possessified, got: {node:?}"
        );
    }

    #[test]
    fn inside_lookaround_not_possessified() {
        // \(\w\+:\)\@= → inside lookaround, do not possessify
        let result = parse_pattern(r"\(\w\+:\)\@=").expect("parse should succeed");
        let (mut lowered, _props) = lower(&result.node);
        auto_possessify(&mut lowered, result.case_mode);
        // The outer node is a lookaround — its inner group/sequence should NOT
        // have been possessified.
        assert!(
            !contains_atomic(&lowered),
            r"expected lookaround body to NOT be possessified, got: {lowered:?}"
        );
    }

    // ─── Case-mode-aware possessification ──────────────────────────────

    #[test]
    fn ci_literal_overlap_not_possessified() {
        // \ca\+A — 'a' and 'A' overlap in CI mode, must NOT possessify
        let result = parse_pattern(r"\ca\+A").expect("parse");
        let (mut lowered, _) = lower(&result.node);
        auto_possessify(&mut lowered, result.case_mode);
        assert!(
            !contains_atomic(&lowered),
            r"\ca\+A should NOT be possessified (a and A overlap in CI)"
        );
    }

    #[test]
    fn ci_disjoint_classes_still_possessified() {
        // \c\d\+: — digits and colon are disjoint even in CI
        let result = parse_pattern(r"\c\d\+:").expect("parse");
        let (mut lowered, _) = lower(&result.node);
        auto_possessify(&mut lowered, result.case_mode);
        assert!(
            contains_atomic(&lowered),
            r"\c\d\+: should still be possessified (digits and : are disjoint in CI)"
        );
    }

    #[test]
    fn cs_literal_disjoint_still_possessified() {
        // \Ca\+A — case-sensitive, 'a' and 'A' are disjoint
        let result = parse_pattern(r"\Ca\+A").expect("parse");
        let (mut lowered, _) = lower(&result.node);
        auto_possessify(&mut lowered, result.case_mode);
        assert!(
            contains_atomic(&lowered),
            r"\Ca\+A should be possessified (a and A are disjoint in CS mode)"
        );
    }

    #[test]
    fn ci_possessify_behavioral_match() {
        use crate::{MatchContext, VimRegex};
        let re = VimRegex::new(r"\ca\+A").unwrap();
        let ctx = MatchContext::simple("aaA");
        let m = re.find(&ctx).unwrap();
        assert!(m.is_some(), r"\ca\+A should match 'aaA'");
        assert_eq!(m.unwrap().range, 0..3);
    }

    // ─── Behavioral companions for structural possessify tests ─────────

    #[test]
    fn word_plus_colon_behavioral() {
        // \w\+: — possessified, must still match word chars followed by colon
        use crate::test_builder::regex;
        regex(r"\w\+:").text("hello:world").expect_match(0..6).run();
    }

    #[test]
    fn word_plus_colon_behavioral_no_match() {
        use crate::test_builder::regex;
        regex(r"\w\+:").text("helloworld").expect_no_match().run();
    }

    #[test]
    fn digit_plus_whitespace_behavioral() {
        // \d\+\s — possessified, must still match digits followed by whitespace
        use crate::test_builder::regex;
        regex(r"\d\+\s").text("123 abc").expect_match(0..4).run();
    }

    #[test]
    fn digit_plus_dot_digit_plus_chain_behavioral() {
        // \d\+\.\d\+ — first quantifier possessified, must match float-like
        use crate::test_builder::regex;
        regex(r"\d\+\.\d\+").text("3.14").expect_match(0..4).run();
    }

    #[test]
    fn digit_plus_dot_digit_plus_chain_behavioral_multi() {
        use crate::test_builder::regex;
        regex(r"\d\+\.\d\+")
            .text("x12.34y")
            .expect_match(1..6)
            .run();
    }

    #[test]
    fn word_plus_colon_inside_group_behavioral() {
        // \(\w\+:\) — possessified inside group, must still match and capture
        use crate::test_builder::regex;
        regex(r"\(\w\+:\)")
            .text("key:val")
            .expect_match(0..4)
            .expect_capture(1, 0..4)
            .run();
    }

    #[test]
    fn word_plus_digit_behavioral() {
        // \w\+\d — NOT possessified (overlap), must still match via backtracking
        use crate::test_builder::regex;
        regex(r"\w\+\d").text("abc123").expect_match(0..6).run();
    }

    #[test]
    fn lazy_word_plus_colon_behavioral() {
        // \w\{-1,}: — lazy quantifier, NOT possessified, must still match
        use crate::test_builder::regex;
        regex(r"\w\{-1,}:")
            .text("hello:world")
            .expect_match(0..6)
            .run();
    }

    #[test]
    fn inside_lookaround_behavioral() {
        // \(\w\+:\)\@= — lookaround body not possessified, still matches
        use crate::test_builder::regex;
        regex(r"\(\w\+:\)\@=")
            .text("key:val")
            .expect_match(0..0)
            .run();
    }

    #[test]
    fn ci_literal_overlap_behavioral() {
        // \ca\+A — CI mode, NOT possessified, must match via backtracking
        use crate::test_builder::regex;
        regex(r"\ca\+A").text("aaA").expect_match(0..3).run();
    }

    #[test]
    fn ci_disjoint_classes_behavioral() {
        // \c\d\+: — CI mode, possessified, must still match
        use crate::test_builder::regex;
        regex(r"\c\d\+:").text("42:x").expect_match(0..3).run();
    }

    #[test]
    fn cs_literal_disjoint_behavioral() {
        // \Ca\+A — CS mode, possessified, must still match
        use crate::test_builder::regex;
        regex(r"\Ca\+A").text("aaA").expect_match(0..3).run();
    }

    #[test]
    fn cs_literal_disjoint_behavioral_no_match() {
        // \Ca\+A — CS mode, lowercase only input: no 'A' to match
        use crate::test_builder::regex;
        regex(r"\Ca\+A").text("aaa").expect_no_match().run();
    }
}
