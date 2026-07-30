//! Tests for `nfa.rs` — NFA graph types, Thompson's construction,
//! lookaround, and backreference transitions.
//!
//! Included via `#[path = "../tests/nfa/mod.rs"] mod tests;` in `nfa/mod.rs`.
//! Split into submodules to respect the 700-line-per-file limit.

use crate::hir;
use crate::matchers::Matcher;
use crate::nfa::builder::NfaBuilder;
use crate::nfa::{Nfa, StateId, Transition, TransitionKind};

// ═══════════════════════════════════════════════════════════════════════════════
// SHARED HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Build an NFA from a VimPatternNode via HIR lowering.
fn build(node: &crate::ir::VimPatternNode) -> Nfa {
    let (lowered, _props) = hir::lower(node);
    NfaBuilder::build(&lowered).unwrap()
}

/// Count transitions of a specific kind leaving a state.
fn count_kind(nfa: &Nfa, state: StateId, kind: &str) -> usize {
    nfa.transitions(state)
        .iter()
        .filter(|t| kind_name(nfa, &t.kind) == kind)
        .count()
}

/// Get the name of a `TransitionKind` variant.
///
/// Maps new inline variants (`Literal`, `AnyChar`, `AnyCharNl`) and
/// `Matcher(id)` back to the legacy "Char" / "ZeroWidth" names so that
/// existing tests that count transitions by kind continue to work.
fn kind_name(nfa: &Nfa, kind: &TransitionKind) -> &'static str {
    match kind {
        TransitionKind::Epsilon => "Epsilon",
        TransitionKind::Literal(_) | TransitionKind::AnyChar | TransitionKind::AnyCharNl => "Char",
        TransitionKind::Matcher(id) => match nfa.matcher(*id) {
            Matcher::Char(_) => "Char",
            Matcher::ZeroWidth(_) => "ZeroWidth",
            Matcher::Lookaround(_) => "Lookaround",
        },
        TransitionKind::Save(_) => "Save",
        TransitionKind::BackRef(_) => "BackRef",
        TransitionKind::LastSubstitute => "LastSubstitute",
    }
}

/// Find all transitions of a given kind from a state.
fn find_kind<'a>(nfa: &'a Nfa, state: StateId, kind: &str) -> Vec<&'a Transition> {
    nfa.transitions(state)
        .iter()
        .filter(|t| kind_name(nfa, &t.kind) == kind)
        .collect()
}

mod construction;

mod advanced;
