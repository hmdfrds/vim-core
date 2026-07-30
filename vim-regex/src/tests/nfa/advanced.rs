//! Tests for NFA — lookaround, backreference, last-substitute transitions,
//! and complex integration patterns.

use super::*;
use crate::ir::{LookaroundKind, VimPatternNode};
use crate::matchers::LookaroundMatcher;
use crate::nfa::TransitionKind;

/// Extract the `LookaroundMatcher` from the first Lookaround transition in a list.
fn get_lookaround<'a>(nfa: &'a Nfa, trans: &[&Transition]) -> &'a LookaroundMatcher {
    let t = trans.first().unwrap();
    if let TransitionKind::Matcher(mid) = &t.kind {
        if let Matcher::Lookaround(la) = nfa.matcher(*mid) {
            return la;
        }
    }
    panic!("expected Matcher::Lookaround")
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKREFERENCE
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn backref_creates_transition() {
    let nfa = build(&VimPatternNode::BackReference(1));
    assert_eq!(nfa.state_count(), 2);
    assert_eq!(count_kind(&nfa, nfa.start(), "BackRef"), 1);
}

#[test]
fn backref_targets_accept() {
    let nfa = build(&VimPatternNode::BackReference(1));
    let trans = find_kind(&nfa, nfa.start(), "BackRef");
    assert_eq!(trans.first().map(|t| t.target), Some(nfa.accept()));
}

#[test]
fn backref_correct_group_number() {
    let nfa = build(&VimPatternNode::BackReference(3));
    let trans = find_kind(&nfa, nfa.start(), "BackRef");
    if let Some(t) = trans.first() {
        if let TransitionKind::BackRef(group) = &t.kind {
            assert_eq!(group.number(), 3);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn lookahead_positive() {
    let nfa = build(&VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Literal('a')),
        kind: LookaroundKind::PositiveAhead,
        limit: None,
    });
    assert_eq!(nfa.state_count(), 2);
    assert_eq!(count_kind(&nfa, nfa.start(), "Lookaround"), 1);
}

#[test]
fn lookahead_negative() {
    let nfa = build(&VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Literal('x')),
        kind: LookaroundKind::NegativeAhead,
        limit: None,
    });
    let trans = find_kind(&nfa, nfa.start(), "Lookaround");
    assert_eq!(trans.len(), 1);
    let la = get_lookaround(&nfa, &trans);
    assert_eq!(la.kind, LookaroundKind::NegativeAhead);
}

#[test]
fn lookbehind_positive_with_limit() {
    let nfa = build(&VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Literal('b')),
        kind: LookaroundKind::PositiveBehind,
        limit: Some(10),
    });
    let trans = find_kind(&nfa, nfa.start(), "Lookaround");
    assert_eq!(trans.len(), 1);
    let la = get_lookaround(&nfa, &trans);
    assert_eq!(la.kind, LookaroundKind::PositiveBehind);
    assert_eq!(la.limit, Some(10));
}

#[test]
fn lookbehind_negative() {
    let nfa = build(&VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Literal('c')),
        kind: LookaroundKind::NegativeBehind,
        limit: None,
    });
    let trans = find_kind(&nfa, nfa.start(), "Lookaround");
    let la = get_lookaround(&nfa, &trans);
    assert_eq!(la.kind, LookaroundKind::NegativeBehind);
}

#[test]
fn atomic_group() {
    let nfa = build(&VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Literal('d')),
        kind: LookaroundKind::Atomic,
        limit: None,
    });
    let trans = find_kind(&nfa, nfa.start(), "Lookaround");
    let la = get_lookaround(&nfa, &trans);
    assert_eq!(la.kind, LookaroundKind::Atomic);
}

#[test]
fn lookaround_sub_nfa_structurally_correct() {
    let nfa = build(&VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Sequence(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ])),
        kind: LookaroundKind::PositiveAhead,
        limit: None,
    });
    let trans = find_kind(&nfa, nfa.start(), "Lookaround");
    let la = get_lookaround(&nfa, &trans);
    let sub_nfa = nfa.sub_nfa(la.sub_nfa_id);
    assert_eq!(sub_nfa.state_count(), 4);
    assert_eq!(count_kind(sub_nfa, sub_nfa.start(), "Char"), 1);
}

#[test]
fn lookaround_targets_accept() {
    let nfa = build(&VimPatternNode::Lookaround {
        inner: Box::new(VimPatternNode::Literal('z')),
        kind: LookaroundKind::PositiveAhead,
        limit: None,
    });
    let trans = find_kind(&nfa, nfa.start(), "Lookaround");
    assert_eq!(trans.first().map(|t| t.target), Some(nfa.accept()));
}

// ═══════════════════════════════════════════════════════════════════════════════
// LAST SUBSTITUTE
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn last_substitute_placeholder() {
    let nfa = build(&VimPatternNode::LastSubstitute);
    assert_eq!(nfa.state_count(), 2);
    assert_eq!(count_kind(&nfa, nfa.start(), "LastSubstitute"), 1);
}

#[test]
fn last_substitute_targets_accept() {
    let nfa = build(&VimPatternNode::LastSubstitute);
    let trans = find_kind(&nfa, nfa.start(), "LastSubstitute");
    assert_eq!(trans.first().map(|t| t.target), Some(nfa.accept()));
}

// ═══════════════════════════════════════════════════════════════════════════════
// INTEGRATION — COMPLEX PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn group_with_alternation() {
    // After HIR lowering, Alternation([Literal('a'), Literal('b')]) becomes a Collection.
    // Capturing group wraps it: Save(0) -> after_open -> ε -> Collection(2 states) -> ε -> Save(1) -> accept
    // That's 6 states for the group wrapper + 2 for the collection = 6 total (some shared).
    let nfa = build(&VimPatternNode::Group {
        inner: Box::new(VimPatternNode::Alternation(vec![
            VimPatternNode::Literal('a'),
            VimPatternNode::Literal('b'),
        ])),
        capturing: true,
    });
    let saves = find_kind(&nfa, nfa.start(), "Save");
    assert_eq!(saves.len(), 1);
    // 6 states: save_start, after_open, coll_start, coll_accept, before_close, accept
    assert!(nfa.state_count() >= 6);
}

#[test]
fn quantified_group() {
    let nfa = build(&VimPatternNode::Quantifier {
        node: Box::new(VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: true,
        }),
        min: 0,
        max: None,
        greedy: true,
    });
    let mut has_save = false;
    let mut has_epsilon = false;
    for i in 0..nfa.state_count() {
        let sid = StateId(i as u32);
        for t in nfa.transitions(sid) {
            match &t.kind {
                TransitionKind::Save(_) => has_save = true,
                TransitionKind::Epsilon => has_epsilon = true,
                _ => {}
            }
        }
    }
    assert!(has_save);
    assert!(has_epsilon);
}

#[test]
fn sequence_with_backreference() {
    let nfa = build(&VimPatternNode::Sequence(vec![
        VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: true,
        },
        VimPatternNode::BackReference(1),
    ]));
    let mut has_save = false;
    let mut has_backref = false;
    for i in 0..nfa.state_count() {
        let sid = StateId(i as u32);
        for t in nfa.transitions(sid) {
            match &t.kind {
                TransitionKind::Save(_) => has_save = true,
                TransitionKind::BackRef(_) => has_backref = true,
                _ => {}
            }
        }
    }
    assert!(has_save);
    assert!(has_backref);
}

// ═══════════════════════════════════════════════════════════════════════════════
// BRANCH-AND NFA
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn branch_and_nfa_has_lookaround() {
    use crate::ir::CharClass;
    let node = VimPatternNode::BranchAnd(vec![
        VimPatternNode::Class(CharClass::Digit),
        VimPatternNode::Literal('f'),
    ]);
    let nfa = build(&node);

    let mut found_la = false;
    for sid in nfa.states() {
        for trans in nfa.transitions(sid) {
            if let TransitionKind::Matcher(mid) = &trans.kind {
                if matches!(nfa.matcher(*mid), Matcher::Lookaround(_)) {
                    found_la = true;
                }
            }
        }
    }
    assert!(
        found_la,
        "BranchAnd NFA should have a Lookaround transition"
    );
}

#[test]
fn branch_and_nfa_negative_match() {
    use crate::cache::Cache;
    use crate::engines::pike_vm;
    use crate::ir::CharClass;
    use crate::matchers::MatchContext;

    // \d\&f — digit AND 'f' can never both match
    let node = VimPatternNode::BranchAnd(vec![
        VimPatternNode::Class(CharClass::Digit),
        VimPatternNode::Literal('f'),
    ]);
    let nfa = build(&node);
    let ctx = MatchContext::simple("f123");
    let mut cache = Cache::new(
        nfa.state_count() as u32,
        nfa.slot_count() + 2,
        nfa.has_backreferences(),
    );
    let result = pike_vm::match_anchored(&nfa, &mut cache, &ctx, 0);
    assert!(result.is_none());
}

#[test]
fn branch_and_nfa_positive_match() {
    use crate::cache::Cache;
    use crate::engines::pike_vm;
    use crate::ir::CharClass;
    use crate::matchers::MatchContext;

    // \w\&f — word char AND 'f' → should match 'f'
    let node = VimPatternNode::BranchAnd(vec![
        VimPatternNode::Class(CharClass::Word),
        VimPatternNode::Literal('f'),
    ]);
    let nfa = build(&node);
    let ctx = MatchContext::simple("f123");
    let mut cache = Cache::new(
        nfa.state_count() as u32,
        nfa.slot_count() + 2,
        nfa.has_backreferences(),
    );
    let result = pike_vm::match_anchored(&nfa, &mut cache, &ctx, 0);
    assert!(result.is_some());
    assert_eq!(result.unwrap().range, 0..1);
}
