//! Sound over-approximation of where a match may begin.
//!
//! `StartSet::of` is the single authoritative producer of a node's start set.
//! Its Sequence fold soundly unions first-char sets across nullable prefixes and
//! yields `Anywhere` whenever it cannot prove a tight constraint, so a
//! `Constrained` set that excludes a real start position is unrepresentable.

use super::charset::CharSet;
use super::LoweredNode;
use crate::ir::CaseMode;

/// Sound over-approximation of the characters at which a match MAY begin.
///
/// INVARIANT: every reachable start character is included. The only producer is
/// `StartSet::of`, which yields `Anywhere` whenever it cannot prove a tight
/// constraint — so a `Constrained` set that excludes a real start is unrepresentable.
#[derive(Debug, Clone)]
pub(crate) enum StartSet {
    /// A match starting here must begin with a character in this set.
    Constrained(CharSet),
    /// Nullable, unknown-first, or universal: no sound start filter is possible
    /// (the caller must try every position / decline the optimization).
    Anywhere,
}

impl StartSet {
    /// The sound MATCH-START set of `node` under `case_mode`: the characters at
    /// which a match of `node` MAY begin.
    ///
    /// If `node` can match the empty string it may begin at any position (an
    /// empty match consumes nothing), so the result is `Anywhere`. Otherwise the
    /// node always consumes, and the start char equals its first-consumed char
    /// (`first_consumed`). This is the producer consumed by `compute_start_bitmap`,
    /// `build_prefilter`, and possessify — all of which want match-start semantics
    /// (a nullable side cannot be soundly constrained).
    pub(crate) fn of(node: &LoweredNode, case_mode: CaseMode) -> StartSet {
        if can_match_empty(node) {
            // Empty match ⇒ a match may begin anywhere ⇒ no sound start filter.
            return StartSet::Anywhere;
        }
        first_consumed(node, case_mode)
    }
}

/// First-consumed-char over-approximation of `node`, ASSUMING it consumes at
/// least one character. (Nullability is decided by the caller — `StartSet::of`
/// for a whole node, or the `seq_start` fold for each sequence child.) Only the
/// fold may legitimately call this on a nullable child, because the fold keeps
/// unioning the successors of a nullable-but-consuming prefix.
fn first_consumed(node: &LoweredNode, case_mode: CaseMode) -> StartSet {
    match node {
        LoweredNode::Literal(ch) => StartSet::Constrained(lit(*ch, case_mode)),
        LoweredNode::LiteralString(s) => match s.chars().next() {
            Some(ch) => StartSet::Constrained(lit(ch, case_mode)),
            None => StartSet::Anywhere,
        },
        LoweredNode::AnyChar | LoweredNode::AnyCharNl => StartSet::Anywhere,
        LoweredNode::Collection {
            negated,
            items,
            include_newline,
        } => StartSet::Constrained(CharSet::from_collection(
            items,
            *negated,
            *include_newline,
            case_mode,
        )),
        LoweredNode::Sequence(children) => seq_start(children, case_mode),
        // `\%[...]`: the same nullable-prefix fold gives its first-consumed
        // union. (Its overall nullability is handled by `StartSet::of`.)
        LoweredNode::OptionalSequence(children) => seq_start(children, case_mode),
        LoweredNode::Alternation(branches) => {
            // Union of branch first-consumed sets; any `Anywhere` poisons it.
            let mut acc: Option<CharSet> = None;
            for branch in branches {
                match first_consumed(branch, case_mode) {
                    StartSet::Anywhere => return StartSet::Anywhere,
                    StartSet::Constrained(cs) => {
                        acc = Some(match acc {
                            Some(a) => a.union(&cs),
                            None => cs,
                        });
                    }
                }
            }
            // No branches (empty acc) ⇒ no constraint.
            acc.map_or(StartSet::Anywhere, StartSet::Constrained)
        }
        LoweredNode::BranchAnd(branches) => {
            // The start char must satisfy ALL constrained branches; `Anywhere`
            // branches add no constraint and are ignored.
            let mut acc: Option<CharSet> = None;
            for branch in branches {
                if let StartSet::Constrained(cs) = first_consumed(branch, case_mode) {
                    acc = Some(match acc {
                        Some(a) => a.intersection(&cs),
                        None => cs,
                    });
                }
            }
            acc.map_or(StartSet::Anywhere, StartSet::Constrained)
        }
        // The quantifier's first consumed char is its body's first consumed
        // char (overall nullability handled by `StartSet::of`).
        LoweredNode::Quantifier { node, .. } => first_consumed(node, case_mode),
        LoweredNode::Group { inner, .. } => first_consumed(inner, case_mode),
        // Unknown first consumed char (backref / `~`): cannot constrain.
        LoweredNode::BackReference(_) | LoweredNode::LastSubstitute => StartSet::Anywhere,
        // Lookaround consumes nothing of the main match (zero-width) — a
        // standalone cannot constrain the start.
        LoweredNode::Lookaround { .. } => StartSet::Anywhere,
        // Zero-width assertions: standalone consume nothing → no constraint.
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
        | LoweredNode::AtMark { .. } => StartSet::Anywhere,
    }
}

/// The sound first-consumed set of a sequence body: union the first-consumed
/// sets across the nullable-but-consuming prefix, stopping at the first mandatory
/// consumer; `Anywhere` if every atom is nullable/zero-width (no consumer pins
/// the first char). Children use `first_consumed` (not `StartSet::of`) so a
/// nullable-but-consuming child like `a*` contributes `{a}` instead of poisoning
/// the fold with `Anywhere`.
fn seq_start(children: &[LoweredNode], case_mode: CaseMode) -> StartSet {
    let mut acc: Option<CharSet> = None;
    for child in children {
        if is_zero_width(child) {
            continue;
        }
        match first_consumed(child, case_mode) {
            StartSet::Anywhere => return StartSet::Anywhere,
            StartSet::Constrained(cs) => {
                acc = Some(match acc {
                    Some(a) => a.union(&cs),
                    None => cs,
                });
            }
        }
        if !can_match_empty(child) {
            return StartSet::Constrained(acc.expect("non-nullable child has a Constrained set"));
        }
    }
    // Every child nullable/zero-width ⇒ no mandatory consumer pins the start.
    StartSet::Anywhere
}

/// Singleton start set for a literal, case-folded under CI.
fn lit(ch: char, case_mode: CaseMode) -> CharSet {
    match case_mode {
        CaseMode::Insensitive => CharSet::from_literal_ci(ch),
        CaseMode::Sensitive | CaseMode::Default => CharSet::from_literal(ch),
    }
}

/// Single nullability oracle (replaces `accel::node_always_consumes` and
/// `possessify::body_always_consumes`). `true` ⇒ node may match the empty string.
///
/// Structural and exhaustive over `LoweredNode` — a future variant forces an
/// explicit decision here.
pub(crate) fn can_match_empty(node: &LoweredNode) -> bool {
    match node {
        // Always consume at least one character.
        LoweredNode::Literal(_)
        | LoweredNode::LiteralString(_)
        | LoweredNode::AnyChar
        | LoweredNode::AnyCharNl
        | LoweredNode::Collection { .. } => false,
        // A sequence matches empty iff every child can.
        LoweredNode::Sequence(children) => children.iter().all(can_match_empty),
        // An alternation matches empty iff any branch can.
        LoweredNode::Alternation(branches) => branches.iter().any(can_match_empty),
        // BranchAnd requires all branches at the same position; matches empty
        // iff all branches can.
        LoweredNode::BranchAnd(branches) => branches.iter().all(can_match_empty),
        LoweredNode::Group { inner, .. } => can_match_empty(inner),
        LoweredNode::Quantifier { min, node, .. } => *min == 0 || can_match_empty(node),
        // OptionalSequence (`\%[...]`) is always nullable.
        LoweredNode::OptionalSequence(_) => true,
        // Backref / last-substitute may have captured/substituted the empty string.
        LoweredNode::BackReference(_) | LoweredNode::LastSubstitute => true,
        // Lookaround consumes nothing of the main match.
        LoweredNode::Lookaround { .. } => true,
        // Zero-width assertions consume nothing.
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
        | LoweredNode::AtMark { .. } => true,
    }
}

/// True for zero-width assertions (consume no input). The 16 assertion variants;
/// moved here from `accel::start_desc` so `hir` owns the single source of truth.
pub(crate) fn is_zero_width(node: &LoweredNode) -> bool {
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

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{CharClass, CollectionItem};
    use crate::nfa::CaptureGroup;

    /// Helper: is `ch` (ASCII) present in a Constrained start set?
    fn constrained_has(node: &LoweredNode, case_mode: CaseMode, ch: char) -> bool {
        match StartSet::of(node, case_mode) {
            StartSet::Constrained(cs) => {
                cs.as_start_bitmap()[(ch as usize) >> 5] & (1u32 << ((ch as u32) & 31)) != 0
            }
            StartSet::Anywhere => panic!("expected Constrained, got Anywhere"),
        }
    }

    fn star(inner: LoweredNode) -> LoweredNode {
        LoweredNode::Quantifier {
            node: Box::new(inner),
            min: 0,
            max: None,
            greedy: true,
        }
    }

    #[test]
    fn nullable_prefix_unions_successor() {
        // [a*, b] → Constrained({a, b}) — the nullable a* keeps unioning.
        let node = LoweredNode::Sequence(vec![
            star(LoweredNode::Literal('a')),
            LoweredNode::Literal('b'),
        ]);
        assert!(constrained_has(&node, CaseMode::Sensitive, 'a'));
        assert!(constrained_has(&node, CaseMode::Sensitive, 'b'));
        assert!(!constrained_has(&node, CaseMode::Sensitive, 'c'));
    }

    #[test]
    fn standalone_star_is_anywhere() {
        // a* alone is nullable → Anywhere.
        let node = star(LoweredNode::Literal('a'));
        assert!(matches!(
            StartSet::of(&node, CaseMode::Sensitive),
            StartSet::Anywhere
        ));
    }

    #[test]
    fn zero_width_prefix_skipped() {
        // [^assert, b] → Constrained({b}) — the assertion is skipped, b pins it.
        let node = LoweredNode::Sequence(vec![LoweredNode::StartOfLine, LoweredNode::Literal('b')]);
        assert!(constrained_has(&node, CaseMode::Sensitive, 'b'));
        assert!(!constrained_has(&node, CaseMode::Sensitive, 'a'));
    }

    #[test]
    fn backref_prefix_is_anywhere() {
        // [backref, b] → Anywhere — the backref's first char is unknown.
        let node = LoweredNode::Sequence(vec![
            LoweredNode::BackReference(CaptureGroup::from_one_based(1)),
            LoweredNode::Literal('b'),
        ]);
        assert!(matches!(
            StartSet::of(&node, CaseMode::Sensitive),
            StartSet::Anywhere
        ));
    }

    #[test]
    fn nested_group_delegates() {
        // \(\(a\)\) → Constrained({a}) through nested groups.
        let inner = LoweredNode::Group {
            inner: Box::new(LoweredNode::Literal('a')),
            capturing: true,
            group: Some(CaptureGroup::from_one_based(2)),
        };
        let node = LoweredNode::Group {
            inner: Box::new(inner),
            capturing: true,
            group: Some(CaptureGroup::from_one_based(1)),
        };
        assert!(constrained_has(&node, CaseMode::Sensitive, 'a'));
        assert!(!constrained_has(&node, CaseMode::Sensitive, 'b'));
    }

    #[test]
    fn ci_collection_folds_case() {
        // \c[a-c] → Constrained including A-C as well.
        let node = LoweredNode::Collection {
            negated: false,
            items: vec![CollectionItem::Range('a', 'c')],
            include_newline: false,
        };
        assert!(constrained_has(&node, CaseMode::Insensitive, 'a'));
        assert!(constrained_has(&node, CaseMode::Insensitive, 'A'));
        assert!(constrained_has(&node, CaseMode::Insensitive, 'C'));
        // Case-sensitive must NOT include the upper-case variants.
        assert!(!constrained_has(&node, CaseMode::Sensitive, 'A'));
    }

    #[test]
    fn ci_class_collection_folds_case() {
        // \c[\l] (lower-case class) under CI also matches upper-case.
        let node = LoweredNode::Collection {
            negated: false,
            items: vec![CollectionItem::Class(CharClass::Lower)],
            include_newline: false,
        };
        assert!(constrained_has(&node, CaseMode::Insensitive, 'a'));
        assert!(constrained_has(&node, CaseMode::Insensitive, 'A'));
    }

    #[test]
    fn can_match_empty_basics() {
        assert!(!can_match_empty(&LoweredNode::Literal('a')));
        assert!(can_match_empty(&star(LoweredNode::Literal('a'))));
        assert!(can_match_empty(&LoweredNode::Sequence(vec![])));
        assert!(can_match_empty(&LoweredNode::StartOfLine));
        assert!(!can_match_empty(&LoweredNode::Sequence(vec![
            star(LoweredNode::Literal('a')),
            LoweredNode::Literal('b'),
        ])));
    }

    #[test]
    fn all_nullable_sequence_is_anywhere() {
        // [a*, b*] — both nullable → Anywhere.
        let node = LoweredNode::Sequence(vec![
            star(LoweredNode::Literal('a')),
            star(LoweredNode::Literal('b')),
        ]);
        assert!(matches!(
            StartSet::of(&node, CaseMode::Sensitive),
            StartSet::Anywhere
        ));
    }
}
