//! Tests for NFA Thompson's construction — leaf nodes, sequences,
//! alternation, quantifiers, groups, and optional sequences.

use super::*;
use crate::ir::VimPatternNode;
use crate::nfa::TransitionKind;

// ═══════════════════════════════════════════════════════════════════════════════
// SEQUENCE (concatenation)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn sequence_chains_with_epsilon() {
    // After epsilon elimination, the trivial epsilon state between 'a' and 'b'
    // is eliminated. The 'a' transition now targets 'b's start directly.
    let nfa = build(&VimPatternNode::Sequence(vec![
        VimPatternNode::Literal('a'),
        VimPatternNode::Literal('b'),
    ]));
    let trans = nfa.transitions(nfa.start());
    assert_eq!(trans.len(), 1);
    let after_a = trans.first().map(|t| t.target);
    if let Some(s1) = after_a {
        // After epsilon elimination, s1 is either the eliminated state (dead,
        // 0 transitions) or directly the start of 'b' (1 Char transition).
        // The rewritten target goes straight to 'b's start state.
        let eps = count_kind(&nfa, s1, "Epsilon");
        let chr = count_kind(&nfa, s1, "Char");
        assert!(
            eps == 0 && chr == 1,
            "after epsilon elimination, sequence should chain directly (eps={eps}, chr={chr})"
        );
    }
}

#[test]
fn sequence_empty() {
    let nfa = build(&VimPatternNode::Sequence(vec![]));
    assert_eq!(nfa.state_count(), 1);
    assert_eq!(nfa.start(), nfa.accept());
}

// ═══════════════════════════════════════════════════════════════════════════════
// ALTERNATION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn alternation_two_literal_branches_lowered_to_collection() {
    // HIR lowering collapses Alternation([Literal('a'), Literal('b')]) into a
    // single Collection matcher, yielding 2 states (start --Matcher--> accept).
    let nfa = build(&VimPatternNode::Alternation(vec![
        VimPatternNode::Literal('a'),
        VimPatternNode::Literal('b'),
    ]));
    assert_eq!(nfa.state_count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// GROUP
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn capturing_group_has_save() {
    let nfa = build(&VimPatternNode::Group {
        inner: Box::new(VimPatternNode::Literal('a')),
        capturing: true,
    });
    let saves = find_kind(&nfa, nfa.start(), "Save");
    assert_eq!(saves.len(), 1);
    if let TransitionKind::Save(slot) = &saves.first().unwrap().kind {
        assert_eq!(slot.index(), 0);
    }
}

#[test]
fn capturing_group_close_save() {
    let nfa = build(&VimPatternNode::Group {
        inner: Box::new(VimPatternNode::Literal('a')),
        capturing: true,
    });
    // Walk: start --Save(0)--> s1 --eps--> s2 --Char--> s3 --eps--> s4 --Save(1)--> accept
    let s1 = nfa.transitions(nfa.start()).first().map(|t| t.target);
    if let Some(s1) = s1 {
        let inner_start = find_kind(&nfa, s1, "Epsilon").first().map(|t| t.target);
        if let Some(is) = inner_start {
            let inner_accept = find_kind(&nfa, is, "Char").first().map(|t| t.target);
            if let Some(ia) = inner_accept {
                let before_close = find_kind(&nfa, ia, "Epsilon").first().map(|t| t.target);
                if let Some(bc) = before_close {
                    let saves = find_kind(&nfa, bc, "Save");
                    assert_eq!(saves.len(), 1);
                    if let TransitionKind::Save(slot) = &saves.first().unwrap().kind {
                        assert_eq!(slot.index(), 1);
                    }
                    assert_eq!(saves.first().map(|t| t.target), Some(nfa.accept()));
                }
            }
        }
    }
}

#[test]
fn multiple_groups_sequential_slots() {
    let nfa = build(&VimPatternNode::Sequence(vec![
        VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('a')),
            capturing: true,
        },
        VimPatternNode::Group {
            inner: Box::new(VimPatternNode::Literal('b')),
            capturing: true,
        },
    ]));
    let mut slots = Vec::new();
    for i in 0..nfa.state_count() {
        let sid = StateId(i as u32);
        for t in nfa.transitions(sid) {
            if let TransitionKind::Save(slot) = &t.kind {
                slots.push(slot.index());
            }
        }
    }
    slots.sort();
    assert_eq!(slots, vec![0, 1, 2, 3]);
}
