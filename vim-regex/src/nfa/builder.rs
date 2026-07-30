//! Thompson's construction for Vim regex NFA.
//!
//! Walks a `LoweredNode` tree (from the HIR lowering pass) and builds an
//! `Nfa` bottom-up using the fragment-composition pattern. Each lowered
//! node type maps to a specific fragment construction rule.

use std::collections::VecDeque;

use smallvec::SmallVec;

use super::{
    CaptureGroup, MatcherId, Nfa, NfaFragment, NfaState, QuantifierHint, StateId, SubNfaId,
    Transition, TransitionKind,
};
use crate::hir::LoweredNode;
use crate::ir::{CollectionItem, LookaroundKind, VimRegexError, VimRegexErrorKind};
use crate::matchers::{CharMatcher, Matcher, ZeroWidthMatcher};

/// Maximum number of NFA states allowed per pattern.
///
/// 100,000 states is far beyond any practical Vim regex pattern
/// (typical patterns produce 10-200 states; complex ones reach ~2,000).
/// This limit prevents catastrophic memory consumption from pathological inputs.
const MAX_NFA_STATES: usize = 100_000;

/// Maximum lookaround nesting depth.
///
/// 32 levels is far beyond any practical pattern (typical: 1-3 levels).
/// Prevents stack overflow and exponential sub-NFA construction.
const MAX_LOOKAROUND_DEPTH: u32 = 32;

// ═══════════════════════════════════════════════════════════════════════════════
// NFA BUILDER
// ═══════════════════════════════════════════════════════════════════════════════

/// Builds an `Nfa` from a `LoweredNode` tree using Thompson's construction.
pub(crate) struct NfaBuilder {
    /// Accumulator for all NFA states.
    states: Vec<NfaState>,
    /// Side table: heavy matchers (indexed by `MatcherId`).
    matchers: Vec<Matcher>,
    /// Side table: lookaround sub-NFAs (indexed by `SubNfaId`).
    sub_nfas: Vec<Nfa>,
    /// When true, builds a reverse/approximate NFA: sequences and literals are
    /// reversed, captures are suppressed, and lookarounds collapse to epsilon.
    is_approximate: bool,
    /// Maximum NFA states allowed (from MemoryConfig).
    state_budget: usize,
    /// Current lookaround nesting depth.
    lookaround_depth: u32,
}

impl NfaBuilder {
    /// Build an NFA from a lowered HIR node tree.
    pub(crate) fn build(node: &LoweredNode) -> Result<Nfa, VimRegexError> {
        Self::build_with_depth(node, 0, MAX_NFA_STATES, false)
    }

    pub(crate) fn build_reverse(node: &LoweredNode) -> Result<Nfa, VimRegexError> {
        Self::build_with_depth(node, 0, MAX_NFA_STATES, true)
    }

    /// Build an NFA inheriting lookaround depth from the parent.
    fn build_with_depth(
        node: &LoweredNode,
        depth: u32,
        state_budget: usize,
        is_approximate: bool,
    ) -> Result<Nfa, VimRegexError> {
        let mut builder = Self {
            states: Vec::new(),
            matchers: Vec::new(),
            sub_nfas: Vec::new(),
            is_approximate,
            state_budget,
            lookaround_depth: depth,
        };
        let fragment = builder.build_node(node)?;
        let mut nfa = Nfa {
            states: builder.states,
            start: fragment.start,
            accept: fragment.accept,
            matchers: builder.matchers,
            sub_nfas: builder.sub_nfas,
            slot_count: 0,
            has_backreferences: false,
            backref_groups: SmallVec::new(),
            backref_reachable: Vec::new(),
            has_consuming: Vec::new(),
            quantifier_hints: Vec::new(),
        };
        if !is_approximate {
            annotate_lookaround_defer(&mut nfa);
        }
        eliminate_trivial_epsilons(&mut nfa);
        nfa.slot_count = compute_nfa_slot_count(&nfa);
        nfa.has_backreferences = compute_nfa_has_backreferences(&nfa);
        nfa.backref_groups = compute_backref_groups(&nfa);
        nfa.backref_reachable = compute_backref_reachable(&nfa);
        nfa.has_consuming = compute_has_consuming(&nfa);
        nfa.quantifier_hints = compute_quantifier_hints(&nfa);
        Ok(nfa)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATE ALLOCATION
// ═══════════════════════════════════════════════════════════════════════════════

impl NfaBuilder {
    /// Allocate a new empty state and return its ID.
    ///
    /// Returns `Err(VimRegexError::PatternTooComplex)` if the NFA state budget
    /// is exceeded.
    fn new_state(&mut self) -> Result<StateId, VimRegexError> {
        if self.states.len() >= self.state_budget {
            return Err(VimRegexErrorKind::PatternTooComplex {
                span: None,
                detail: format!("NFA state budget exceeded ({} states)", self.state_budget).into(),
            }
            .into());
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "NFA states limited to MAX_NFA_STATES < u32::MAX"
        )]
        let id = StateId(self.states.len() as u32);
        self.states.push(NfaState::new());
        Ok(id)
    }

    /// Add a transition from `from` to `to`.
    fn add_transition(&mut self, from: StateId, kind: TransitionKind, to: StateId) {
        if let Some(state) = self.states.get_mut(from.index()) {
            state.transitions.push(Transition { kind, target: to });
        }
    }

    /// Add an epsilon transition from `from` to `to`.
    fn add_epsilon(&mut self, from: StateId, to: StateId) {
        self.add_transition(from, TransitionKind::Epsilon, to);
    }

    /// Push a matcher into the side table and return its index.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "matchers limited to u32::MAX"
    )]
    fn push_matcher(&mut self, matcher: Matcher) -> MatcherId {
        let id = MatcherId(self.matchers.len() as u32);
        self.matchers.push(matcher);
        id
    }

    /// Push a sub-NFA into the side table and return its index.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "sub-NFAs limited to u32::MAX"
    )]
    fn push_sub_nfa(&mut self, nfa: Nfa) -> SubNfaId {
        let id = SubNfaId(self.sub_nfas.len() as u32);
        self.sub_nfas.push(nfa);
        id
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// NODE DISPATCH
// ═══════════════════════════════════════════════════════════════════════════════

impl NfaBuilder {
    /// Build an NFA fragment for a single lowered node.
    fn build_node(&mut self, node: &LoweredNode) -> Result<NfaFragment, VimRegexError> {
        match node {
            LoweredNode::Literal(ch) => self.build_char(CharMatcher::Literal(*ch)),
            LoweredNode::LiteralString(s) => self.build_literal_string(s),
            LoweredNode::AnyChar => self.build_char(CharMatcher::AnyChar),
            LoweredNode::AnyCharNl => self.build_char(CharMatcher::AnyCharNl),
            LoweredNode::Collection {
                negated,
                items,
                include_newline,
            } => self.build_collection(*negated, items, *include_newline),
            LoweredNode::StartOfLine => self.build_zero_width(ZeroWidthMatcher::StartOfLine),
            LoweredNode::EndOfLine => self.build_zero_width(ZeroWidthMatcher::EndOfLine),
            LoweredNode::AnywhereStartOfLine => {
                self.build_zero_width(ZeroWidthMatcher::StartOfLine)
            }
            LoweredNode::AnywhereEndOfLine => self.build_zero_width(ZeroWidthMatcher::EndOfLine),
            LoweredNode::StartOfFile => self.build_zero_width(ZeroWidthMatcher::StartOfFile),
            LoweredNode::EndOfFile => self.build_zero_width(ZeroWidthMatcher::EndOfFile),
            LoweredNode::WordBoundaryStart => {
                self.build_zero_width(ZeroWidthMatcher::WordBoundaryStart)
            }
            LoweredNode::WordBoundaryEnd => {
                self.build_zero_width(ZeroWidthMatcher::WordBoundaryEnd)
            }
            LoweredNode::SetMatchStart => self.build_zero_width(ZeroWidthMatcher::SetMatchStart),
            LoweredNode::SetMatchEnd => self.build_zero_width(ZeroWidthMatcher::SetMatchEnd),
            LoweredNode::CursorPosition => self.build_zero_width(ZeroWidthMatcher::CursorPosition),
            LoweredNode::VisualArea => self.build_zero_width(ZeroWidthMatcher::VisualArea),
            LoweredNode::AtLine(spec) => self.build_zero_width(ZeroWidthMatcher::AtLine(*spec)),
            LoweredNode::AtColumn(spec) => self.build_zero_width(ZeroWidthMatcher::AtColumn(*spec)),
            LoweredNode::AtVirtualColumn(spec) => {
                self.build_zero_width(ZeroWidthMatcher::AtVirtualColumn(*spec))
            }
            LoweredNode::AtMark { mark, rel } => self.build_zero_width(ZeroWidthMatcher::AtMark {
                mark: *mark,
                rel: *rel,
            }),
            LoweredNode::Sequence(nodes) => self.build_sequence(nodes),
            LoweredNode::Alternation(branches) => self.build_alternation(branches),
            LoweredNode::Group { inner, group, .. } => self.build_group(inner, *group),
            LoweredNode::Quantifier {
                node: n,
                min,
                max,
                greedy,
            } => self.build_quantifier(n, *min, *max, *greedy),
            LoweredNode::BackReference(group) => self.build_backref(*group),
            LoweredNode::Lookaround { inner, kind, limit } => {
                self.build_lookaround(inner, *kind, *limit)
            }
            LoweredNode::LastSubstitute => self.build_last_substitute(),
            LoweredNode::OptionalSequence(atoms) => self.build_optional_sequence(atoms),
            LoweredNode::BranchAnd(branches) => self.build_branch_and(branches),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LEAF FRAGMENTS
// ═══════════════════════════════════════════════════════════════════════════════

impl NfaBuilder {
    fn build_char(&mut self, matcher: CharMatcher) -> Result<NfaFragment, VimRegexError> {
        let start = self.new_state()?;
        let accept = self.new_state()?;
        let kind = match matcher {
            CharMatcher::Literal(ch) => TransitionKind::Literal(ch),
            CharMatcher::AnyChar => TransitionKind::AnyChar,
            CharMatcher::AnyCharNl => TransitionKind::AnyCharNl,
            other => {
                let id = self.push_matcher(Matcher::Char(other));
                TransitionKind::Matcher(id)
            }
        };
        self.add_transition(start, kind, accept);
        Ok(NfaFragment { start, accept })
    }

    fn build_literal_string(&mut self, s: &str) -> Result<NfaFragment, VimRegexError> {
        let chars: Vec<char> = if self.is_approximate {
            s.chars().rev().collect()
        } else {
            s.chars().collect()
        };
        if chars.is_empty() {
            let s = self.new_state()?;
            return Ok(NfaFragment {
                start: s,
                accept: s,
            });
        }
        let mut fragments = Vec::with_capacity(chars.len());
        for ch in &chars {
            fragments.push(self.build_char(CharMatcher::Literal(*ch))?);
        }
        self.concat_fragments(&fragments)
    }

    fn build_zero_width(
        &mut self,
        matcher: ZeroWidthMatcher,
    ) -> Result<NfaFragment, VimRegexError> {
        let start = self.new_state()?;
        let accept = self.new_state()?;
        let id = self.push_matcher(Matcher::ZeroWidth(matcher));
        self.add_transition(start, TransitionKind::Matcher(id), accept);
        Ok(NfaFragment { start, accept })
    }

    fn build_collection(
        &mut self,
        negated: bool,
        items: &[CollectionItem],
        include_newline: bool,
    ) -> Result<NfaFragment, VimRegexError> {
        let matcher = CharMatcher::Collection {
            negated,
            items: items.to_vec(),
            include_newline,
        };
        self.build_char(matcher)
    }

    fn build_backref(&mut self, group: CaptureGroup) -> Result<NfaFragment, VimRegexError> {
        let start = self.new_state()?;
        let accept = self.new_state()?;
        self.add_transition(start, TransitionKind::BackRef(group), accept);
        Ok(NfaFragment { start, accept })
    }

    fn build_last_substitute(&mut self) -> Result<NfaFragment, VimRegexError> {
        let start = self.new_state()?;
        let accept = self.new_state()?;
        self.add_transition(start, TransitionKind::LastSubstitute, accept);
        Ok(NfaFragment { start, accept })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMPOSITE FRAGMENTS
// ═══════════════════════════════════════════════════════════════════════════════

impl NfaBuilder {
    fn build_sequence(&mut self, nodes: &[LoweredNode]) -> Result<NfaFragment, VimRegexError> {
        if nodes.is_empty() {
            let s = self.new_state()?;
            return Ok(NfaFragment {
                start: s,
                accept: s,
            });
        }
        let iter: Box<dyn Iterator<Item = &LoweredNode>> = if self.is_approximate {
            Box::new(nodes.iter().rev())
        } else {
            Box::new(nodes.iter())
        };
        let mut fragments = Vec::with_capacity(nodes.len());
        for n in iter {
            fragments.push(self.build_node(n)?);
        }
        self.concat_fragments(&fragments)
    }

    fn concat_fragments(
        &mut self,
        fragments: &[NfaFragment],
    ) -> Result<NfaFragment, VimRegexError> {
        if fragments.is_empty() {
            let s = self.new_state()?;
            return Ok(NfaFragment {
                start: s,
                accept: s,
            });
        }
        let first_start = fragments[0].start;
        for i in 0..fragments.len() - 1 {
            let current_accept = fragments[i].accept;
            let next_start = fragments[i + 1].start;
            self.add_epsilon(current_accept, next_start);
        }
        let last_accept = fragments.last().map_or(first_start, |f| f.accept);
        Ok(NfaFragment {
            start: first_start,
            accept: last_accept,
        })
    }

    fn build_alternation(
        &mut self,
        branches: &[LoweredNode],
    ) -> Result<NfaFragment, VimRegexError> {
        let start = self.new_state()?;
        let accept = self.new_state()?;
        for branch in branches {
            let frag = self.build_node(branch)?;
            self.add_epsilon(start, frag.start);
            self.add_epsilon(frag.accept, accept);
        }
        Ok(NfaFragment { start, accept })
    }

    fn build_group(
        &mut self,
        inner: &LoweredNode,
        group: Option<CaptureGroup>,
    ) -> Result<NfaFragment, VimRegexError> {
        // Non-capturing group, or capture-stripped (approximate/reverse) build:
        // no Save states, just the inner fragment.
        let Some(group) = group else {
            return self.build_node(inner);
        };
        if self.is_approximate {
            return self.build_node(inner);
        }
        let slot_open = group.open_slot();
        let slot_close = group.close_slot();
        let save_start = self.new_state()?;
        let after_open = self.new_state()?;
        self.add_transition(save_start, TransitionKind::Save(slot_open), after_open);
        let inner_frag = self.build_node(inner)?;
        self.add_epsilon(after_open, inner_frag.start);
        let before_close = self.new_state()?;
        self.add_epsilon(inner_frag.accept, before_close);
        let accept = self.new_state()?;
        self.add_transition(before_close, TransitionKind::Save(slot_close), accept);
        Ok(NfaFragment {
            start: save_start,
            accept,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// QUANTIFIER CONSTRUCTION
// ═══════════════════════════════════════════════════════════════════════════════

impl NfaBuilder {
    fn build_quantifier(
        &mut self,
        node: &LoweredNode,
        min: u32,
        max: Option<u32>,
        greedy: bool,
    ) -> Result<NfaFragment, VimRegexError> {
        let mut fragments = Vec::new();
        for _ in 0..min {
            fragments.push(self.build_node(node)?);
        }
        match max {
            Some(max_val) => {
                self.build_bounded_optional(node, min, max_val, greedy, &mut fragments)?;
            }
            None => {
                self.build_unbounded_loop(node, greedy, &mut fragments)?;
            }
        }
        self.concat_fragments(&fragments)
    }

    fn build_bounded_optional(
        &mut self,
        node: &LoweredNode,
        min: u32,
        max: u32,
        greedy: bool,
        fragments: &mut Vec<NfaFragment>,
    ) -> Result<(), VimRegexError> {
        for _ in min..max {
            let opt_frag = self.build_optional_once(node, greedy)?;
            fragments.push(opt_frag);
        }
        Ok(())
    }

    fn build_unbounded_loop(
        &mut self,
        node: &LoweredNode,
        greedy: bool,
        fragments: &mut Vec<NfaFragment>,
    ) -> Result<(), VimRegexError> {
        let loop_frag = self.build_loop(node, greedy)?;
        fragments.push(loop_frag);
        Ok(())
    }

    fn build_optional_once(
        &mut self,
        node: &LoweredNode,
        greedy: bool,
    ) -> Result<NfaFragment, VimRegexError> {
        let start = self.new_state()?;
        let accept = self.new_state()?;
        let inner = self.build_node(node)?;
        if greedy {
            self.add_epsilon(start, inner.start);
            self.add_epsilon(start, accept);
        } else {
            self.add_epsilon(start, accept);
            self.add_epsilon(start, inner.start);
        }
        self.add_epsilon(inner.accept, accept);
        Ok(NfaFragment { start, accept })
    }

    fn build_loop(
        &mut self,
        node: &LoweredNode,
        greedy: bool,
    ) -> Result<NfaFragment, VimRegexError> {
        let start = self.new_state()?;
        let accept = self.new_state()?;
        let inner = self.build_node(node)?;
        if greedy {
            self.add_epsilon(start, inner.start);
            self.add_epsilon(start, accept);
        } else {
            self.add_epsilon(start, accept);
            self.add_epsilon(start, inner.start);
        }
        self.add_epsilon(inner.accept, start);
        Ok(NfaFragment { start, accept })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND AND OPTIONAL SEQUENCE
// ═══════════════════════════════════════════════════════════════════════════════

impl NfaBuilder {
    fn build_lookaround(
        &mut self,
        inner: &LoweredNode,
        kind: LookaroundKind,
        limit: Option<u32>,
    ) -> Result<NfaFragment, VimRegexError> {
        if self.is_approximate {
            // Atomic groups are semantically transparent wrappers (they only
            // suppress backtracking).  In approximate/reverse mode we must
            // still build the inner content so the reverse NFA can match
            // through it.  Other lookaround kinds (look-ahead/behind) are
            // zero-width assertions with no consumable content, so an epsilon
            // is the correct approximation.
            if kind == LookaroundKind::Atomic {
                return self.build_node(inner);
            }
            let s = self.new_state()?;
            let a = self.new_state()?;
            self.add_epsilon(s, a);
            return Ok(NfaFragment {
                start: s,
                accept: a,
            });
        }

        if self.lookaround_depth >= MAX_LOOKAROUND_DEPTH {
            return Err(VimRegexErrorKind::PatternTooComplex {
                span: None,
                detail: format!(
                    "lookaround nesting depth exceeded ({} levels)",
                    MAX_LOOKAROUND_DEPTH
                )
                .into(),
            }
            .into());
        }

        let sub_nfa =
            Self::build_with_depth(inner, self.lookaround_depth + 1, self.state_budget, false)?;
        let sub_nfa_id = self.push_sub_nfa(sub_nfa);
        let start = self.new_state()?;
        let accept = self.new_state()?;
        let matcher = Matcher::Lookaround(crate::matchers::LookaroundMatcher {
            sub_nfa_id,
            kind,
            limit,
            rest_min_len: 0,
            defer_check: false,
        });
        let id = self.push_matcher(matcher);
        self.add_transition(start, TransitionKind::Matcher(id), accept);
        Ok(NfaFragment { start, accept })
    }

    /// Build NFA for `\&`: all branches must match at the same position.
    ///
    /// Strategy: compile as N-1 positive-lookahead checks followed by the
    /// last branch (which determines the match extent). Each non-last branch
    /// becomes a sub-NFA tested via lookahead.
    fn build_branch_and(&mut self, branches: &[LoweredNode]) -> Result<NfaFragment, VimRegexError> {
        if branches.is_empty() {
            let s = self.new_state()?;
            return Ok(NfaFragment {
                start: s,
                accept: s,
            });
        }

        if branches.len() == 1 {
            return self.build_node(&branches[0]);
        }

        if self.is_approximate {
            // In approximate/reverse mode, collapse to just the last branch
            return self.build_node(branches.last().expect("checked non-empty"));
        }

        if self.lookaround_depth >= MAX_LOOKAROUND_DEPTH {
            return Err(VimRegexErrorKind::PatternTooComplex {
                span: None,
                detail: format!(
                    "lookaround nesting depth exceeded ({} levels)",
                    MAX_LOOKAROUND_DEPTH
                )
                .into(),
            }
            .into());
        }

        // Build N-1 lookahead assertions + final branch
        let mut fragments = Vec::with_capacity(branches.len());

        for branch in &branches[..branches.len() - 1] {
            // Each non-last branch becomes a positive lookahead
            let sub_nfa = Self::build_with_depth(
                branch,
                self.lookaround_depth + 1,
                self.state_budget,
                false,
            )?;
            let sub_nfa_id = self.push_sub_nfa(sub_nfa);
            let start = self.new_state()?;
            let accept = self.new_state()?;
            let matcher = Matcher::Lookaround(crate::matchers::LookaroundMatcher {
                sub_nfa_id,
                kind: LookaroundKind::PositiveAhead,
                limit: None,
                rest_min_len: 0,
                defer_check: false,
            });
            let id = self.push_matcher(matcher);
            self.add_transition(start, TransitionKind::Matcher(id), accept);
            fragments.push(NfaFragment { start, accept });
        }

        // Last branch: builds normally (determines match extent)
        let last_frag = self.build_node(branches.last().expect("checked non-empty"))?;
        fragments.push(last_frag);

        self.concat_fragments(&fragments)
    }

    fn build_optional_sequence(
        &mut self,
        atoms: &[LoweredNode],
    ) -> Result<NfaFragment, VimRegexError> {
        if atoms.is_empty() {
            let s = self.new_state()?;
            return Ok(NfaFragment {
                start: s,
                accept: s,
            });
        }
        self.build_optional_seq_iterative(atoms)
    }

    fn build_optional_seq_iterative(
        &mut self,
        atoms: &[LoweredNode],
    ) -> Result<NfaFragment, VimRegexError> {
        let mut rest = {
            let s = self.new_state()?;
            NfaFragment {
                start: s,
                accept: s,
            }
        };
        for atom in atoms.iter().rev() {
            let start = self.new_state()?;
            let accept = self.new_state()?;
            let atom_frag = self.build_node(atom)?;
            self.add_epsilon(start, atom_frag.start);
            self.add_epsilon(atom_frag.accept, rest.start);
            self.add_epsilon(rest.accept, accept);
            self.add_epsilon(start, accept);
            rest = NfaFragment { start, accept };
        }
        Ok(rest)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRECOMPUTED NFA FIELD HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Count the number of capture slots needed (scan NFA for max Save slot).
fn compute_nfa_slot_count(nfa: &Nfa) -> usize {
    let mut max_slot: usize = 0;
    for sid in nfa.states() {
        for trans in nfa.transitions(sid) {
            match &trans.kind {
                TransitionKind::Save(slot) => {
                    let s = slot.index() + 1;
                    if s > max_slot {
                        max_slot = s;
                    }
                }
                // Capture groups inside a lookaround sub-NFA are numbered in the
                // SAME global slot space. A positive lookaround exports
                // those captures into the PARENT match by global index, so the
                // parent's slot row must be sized to cover them — even when the
                // parent itself has no top-level `Save` (e.g. `\(ab\)\@=ab`, whose
                // only group lives inside the lookahead). Recurse into sub-NFAs.
                TransitionKind::Matcher(mid) => {
                    if let Matcher::Lookaround(la) = &nfa.matchers[mid.0 as usize] {
                        let sub_slots = compute_nfa_slot_count(nfa.sub_nfa(la.sub_nfa_id));
                        if sub_slots > max_slot {
                            max_slot = sub_slots;
                        }
                    }
                }
                TransitionKind::Epsilon
                | TransitionKind::Literal(_)
                | TransitionKind::AnyChar
                | TransitionKind::AnyCharNl
                | TransitionKind::BackRef(_)
                | TransitionKind::LastSubstitute => {}
            }
        }
    }
    max_slot
}

/// Check whether an NFA contains any backreference transitions, including
/// in nested lookaround sub-NFAs.
fn compute_nfa_has_backreferences(nfa: &Nfa) -> bool {
    for sid in nfa.states() {
        for trans in nfa.transitions(sid) {
            match &trans.kind {
                TransitionKind::BackRef(_) => return true,
                TransitionKind::Matcher(mid) => {
                    if let Matcher::Lookaround(la) = &nfa.matchers[mid.0 as usize] {
                        if nfa.sub_nfa(la.sub_nfa_id).has_backreferences() {
                            return true;
                        }
                    }
                }
                TransitionKind::Epsilon
                | TransitionKind::Save(_)
                | TransitionKind::Literal(_)
                | TransitionKind::AnyChar
                | TransitionKind::AnyCharNl
                | TransitionKind::LastSubstitute => {}
            }
        }
    }
    false
}

/// Collect the set of group numbers referenced by `\1`-`\9` transitions.
fn compute_backref_groups(nfa: &Nfa) -> SmallVec<[CaptureGroup; 9]> {
    let mut groups = SmallVec::<[CaptureGroup; 9]>::new();
    collect_backref_groups_recursive(nfa, &mut groups);
    groups.sort_unstable();
    groups.dedup();
    groups
}

fn collect_backref_groups_recursive(nfa: &Nfa, groups: &mut SmallVec<[CaptureGroup; 9]>) {
    for sid in nfa.states() {
        for trans in nfa.transitions(sid) {
            match &trans.kind {
                TransitionKind::BackRef(group) if !groups.contains(group) => {
                    groups.push(*group);
                }
                TransitionKind::BackRef(_) => {
                    // Group already recorded — no-op.
                }
                TransitionKind::Matcher(mid) => {
                    if let Matcher::Lookaround(la) = &nfa.matchers[mid.0 as usize] {
                        collect_backref_groups_recursive(nfa.sub_nfa(la.sub_nfa_id), groups);
                    }
                }
                TransitionKind::Epsilon
                | TransitionKind::Save(_)
                | TransitionKind::Literal(_)
                | TransitionKind::AnyChar
                | TransitionKind::AnyCharNl
                | TransitionKind::LastSubstitute => {}
            }
        }
    }
}

/// Pre-compute which NFA states have any consuming (input-advancing) transition.
///
/// Consuming transitions: Literal, AnyChar, AnyCharNl, BackRef, LastSubstitute,
/// and Matcher(Char(_)).
fn compute_has_consuming(nfa: &Nfa) -> Vec<bool> {
    let mut result = vec![false; nfa.states.len()];
    for (i, state) in nfa.states.iter().enumerate() {
        result[i] = state.transitions.iter().any(|t| {
            matches!(
                &t.kind,
                TransitionKind::Literal(_)
                    | TransitionKind::AnyChar
                    | TransitionKind::AnyCharNl
                    | TransitionKind::BackRef(_)
                    | TransitionKind::LastSubstitute
            ) || matches!(&t.kind, TransitionKind::Matcher(id)
                if matches!(nfa.matchers.get(id.0 as usize), Some(Matcher::Char(_))))
        });
    }
    result
}

/// Detect quantifier fast-path patterns and annotate states.
///
/// Scans the NFA for the pattern:
///   State S has two epsilon transitions (greedy quantifier loop entry):
///     - epsilon -> body_start (where body is a single AnyChar/AnyCharNl consuming state)
///     - epsilon -> loop_exit
///   AND body_accept has epsilon -> S (loop-back)
///   AND the first consuming transition after loop_exit is Literal(ch) where ch is ASCII
///
/// When detected, annotates state S with `QuantifierHint::SkipUntilByte(ch as u8)`.
fn compute_quantifier_hints(nfa: &Nfa) -> Vec<QuantifierHint> {
    let n = nfa.state_count();
    let mut hints = vec![QuantifierHint::None; n];

    for sid in nfa.states() {
        let transitions = nfa.transitions(sid);

        // Quantifier loop-entry states have exactly 2 epsilon transitions
        // (greedy: [body_start, loop_exit] or non-greedy: [loop_exit, body_start]).
        if transitions.len() != 2 {
            continue;
        }
        if !matches!(transitions[0].kind, TransitionKind::Epsilon)
            || !matches!(transitions[1].kind, TransitionKind::Epsilon)
        {
            continue;
        }

        // For a greedy quantifier, the first epsilon goes to the body
        // and the second to the exit. For non-greedy, the order is reversed.
        // SkipUntilChar only applies to greedy quantifiers (the fast path
        // pushes rightmost candidates first for greedy semantics).
        //
        // We detect greedy by checking: is transitions[0].target the body
        // (AnyChar/AnyCharNl that loops back)? If so, it's greedy.
        let (body_candidate, exit_candidate) = (transitions[0].target, transitions[1].target);

        // Only try the greedy ordering (body first, exit second)
        if let Some(hint) = try_detect_skip(nfa, sid, body_candidate, exit_candidate) {
            hints[sid.index()] = hint;
        }
    }

    hints
}

/// Check if `body_start` is a single consuming AnyChar/AnyCharNl state that
/// loops back to `loop_state`, and `exit_start` leads to an ASCII literal.
///
/// After epsilon elimination, the body may loop back directly to `loop_state`
/// (body has AnyChar -> loop_state) or indirectly via an intermediate accept
/// state (body has AnyChar -> body_accept, body_accept has epsilon -> loop_state).
fn try_detect_skip(
    nfa: &Nfa,
    loop_state: StateId,
    body_start: StateId,
    exit_start: StateId,
) -> Option<QuantifierHint> {
    // Body must be a single consuming state (AnyChar or AnyCharNl)
    let body_trans = nfa.transitions(body_start);
    if body_trans.len() != 1 {
        return None;
    }
    let body_kind = &body_trans[0].kind;
    let is_any = matches!(
        body_kind,
        TransitionKind::AnyChar | TransitionKind::AnyCharNl
    );
    if !is_any {
        return None;
    }

    // After epsilon elimination, the body's AnyChar may target loop_state directly
    // (common case) or an intermediate state that has an epsilon to loop_state.
    let body_target = body_trans[0].target;
    let loops_back = if body_target == loop_state {
        // Direct loop-back (epsilon elimination collapsed the intermediate state)
        true
    } else {
        // Check if the intermediate state has an epsilon back to loop_state
        nfa.transitions(body_target)
            .iter()
            .any(|t| matches!(t.kind, TransitionKind::Epsilon) && t.target == loop_state)
    };
    if !loops_back {
        return None;
    }

    // Follow epsilons from exit_start to find the first consuming transition
    let successor_byte = find_first_literal_byte(nfa, exit_start, 8)?;

    Some(QuantifierHint::SkipUntilByte(successor_byte))
}

/// Follow up to `max_depth` epsilon transitions from `start` to find
/// the first `Literal(ch)` transition where `ch` is ASCII.
/// Returns `Some(ch as u8)` if found, `None` otherwise.
fn find_first_literal_byte(nfa: &Nfa, start: StateId, max_depth: usize) -> Option<u8> {
    let mut current = start;
    for _ in 0..max_depth {
        let trans = nfa.transitions(current);
        if trans.is_empty() {
            return None;
        }
        for t in trans {
            match &t.kind {
                TransitionKind::Literal(ch) if ch.is_ascii() => {
                    return Some(*ch as u8);
                }
                TransitionKind::Literal(_) => {
                    // Non-ASCII literal -- cannot use single-byte memchr
                    return None;
                }
                TransitionKind::Epsilon => {
                    // Follow the first epsilon (handled below)
                }
                _ => {
                    // Non-literal consuming transition -- bail
                    return None;
                }
            }
        }
        // If we only found epsilons, follow the first one
        if let Some(t) = trans
            .iter()
            .find(|t| matches!(t.kind, TransitionKind::Epsilon))
        {
            current = t.target;
        } else {
            return None;
        }
    }
    None
}

/// Compute which states are reachable from any BackRef transition target.
///
/// Algorithm: collect all states that are targets of BackRef transitions,
/// then compute forward reachability (BFS) from those states.
/// States reachable from backref targets skip VisitedSet dedup because
/// capture state may affect path validity.
fn compute_backref_reachable(nfa: &Nfa) -> Vec<bool> {
    let state_count = nfa.state_count();
    if !nfa.has_backreferences() {
        return vec![false; state_count];
    }

    let mut reachable = vec![false; state_count];
    let mut worklist: Vec<StateId> = Vec::new();

    // Seed: all target states of BackRef transitions.
    for sid in nfa.states() {
        for trans in nfa.transitions(sid) {
            if matches!(trans.kind, TransitionKind::BackRef(_)) && !reachable[trans.target.index()]
            {
                reachable[trans.target.index()] = true;
                worklist.push(trans.target);
            }
        }
    }

    // Forward BFS from seeds.
    while let Some(sid) = worklist.pop() {
        for trans in nfa.transitions(sid) {
            let target = trans.target.index();
            if !reachable[target] {
                reachable[target] = true;
                worklist.push(trans.target);
            }
        }
    }

    reachable
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND DEFER POST-PASS (0-1 BFS)
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the minimum number of consuming characters from each state to accept
/// via 0-1 BFS on the reversed edge graph.
fn compute_rest_min_len(nfa: &Nfa) -> Vec<usize> {
    let n = nfa.state_count();
    let mut dist = vec![usize::MAX; n];
    let mut rev_adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];

    for sid in nfa.states() {
        for trans in nfa.transitions(sid) {
            let weight = match &trans.kind {
                TransitionKind::Literal(_)
                | TransitionKind::AnyChar
                | TransitionKind::AnyCharNl => 1,
                TransitionKind::Matcher(id) => {
                    if matches!(nfa.matcher(*id), Matcher::Char(_)) {
                        1
                    } else {
                        0
                    }
                }
                TransitionKind::Epsilon
                | TransitionKind::Save(_)
                | TransitionKind::BackRef(_)
                | TransitionKind::LastSubstitute => 0,
            };
            rev_adj[trans.target.index()].push((sid.index(), weight));
        }
    }

    let mut deque = VecDeque::new();
    dist[nfa.accept().index()] = 0;
    deque.push_back(nfa.accept().index());

    while let Some(u) = deque.pop_front() {
        for &(v, w) in &rev_adj[u] {
            let new_dist = dist[u].saturating_add(w);
            if new_dist < dist[v] {
                dist[v] = new_dist;
                if w == 0 {
                    deque.push_front(v);
                } else {
                    deque.push_back(v);
                }
            }
        }
    }

    dist
}

/// Decide whether a lookbehind should be deferred to accept time (PIM heuristic).
fn should_defer_lookbehind(kind: LookaroundKind, limit: Option<u32>, rest_min_len: usize) -> bool {
    matches!(
        kind,
        LookaroundKind::PositiveBehind | LookaroundKind::NegativeBehind
    ) && rest_min_len > 0
        && (limit.is_none() || limit.is_some_and(|l| l > 4))
}

/// Post-pass: annotate every `Lookaround` matcher with `rest_min_len` and `defer_check`.
fn annotate_lookaround_defer(nfa: &mut Nfa) {
    let rest_min = compute_rest_min_len(nfa);
    // Collect (matcher_id, target_state_index) pairs to avoid borrow conflict.
    let mut updates: Vec<(u32, usize)> = Vec::new();
    for state in &nfa.states {
        for trans in &state.transitions {
            if let TransitionKind::Matcher(mid) = &trans.kind {
                if matches!(&nfa.matchers[mid.0 as usize], Matcher::Lookaround(_)) {
                    updates.push((mid.0, trans.target.index()));
                }
            }
        }
    }
    for (mid, target_idx) in updates {
        let target_dist = rest_min.get(target_idx).copied().unwrap_or(0);
        if let Matcher::Lookaround(ref mut la) = &mut nfa.matchers[mid as usize] {
            la.rest_min_len = target_dist;
            la.defer_check = should_defer_lookbehind(la.kind, la.limit, target_dist);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EPSILON ELIMINATION POST-PASS
// ═══════════════════════════════════════════════════════════════════════════════

/// Eliminate trivial epsilon states from the NFA.
///
/// A state is trivially eliminable if:
/// - It has exactly one outgoing epsilon transition
/// - It is not the start or accept state
/// - Its sole outgoing transition is not to itself (no self-loops)
///
/// For each such state S with epsilon -> T:
///   Redirect all transitions targeting S to target T instead.
///   Mark S as dead (empty transitions).
///
/// Dead states are not compacted (StateId values remain stable for
/// SubNfaId references and MatcherId lookups). They are cheap (empty
/// SmallVec) and engines skip them naturally since no transitions
/// target them.
fn eliminate_trivial_epsilons(nfa: &mut Nfa) {
    let n = nfa.states.len();
    // Phase 1: Identify eliminable states.
    let mut redirect: Vec<Option<StateId>> = vec![None; n];

    for (idx, state) in nfa.states.iter().enumerate() {
        let sid = StateId::from_raw(idx);
        if sid == nfa.start || sid == nfa.accept {
            continue;
        }
        if state.transitions.len() != 1 {
            continue;
        }
        let trans = &state.transitions[0];
        if !matches!(trans.kind, TransitionKind::Epsilon) {
            continue;
        }
        if trans.target == sid {
            continue; // self-loop
        }
        redirect[idx] = Some(trans.target);
    }

    // Phase 2: Chase redirect chains to final targets.
    for idx in 0..n {
        if redirect[idx].is_some() {
            let mut target = redirect[idx].unwrap();
            let mut depth = 0;
            while let Some(next) = redirect[target.index()] {
                target = next;
                depth += 1;
                if depth > n {
                    break;
                } // safety: prevent infinite loops
            }
            redirect[idx] = Some(target);
        }
    }

    // Phase 3: Rewrite all transition targets.
    for state in &mut nfa.states {
        for trans in &mut state.transitions {
            if let Some(new_target) = redirect[trans.target.index()] {
                trans.target = new_target;
            }
        }
    }

    // Phase 4: Clear eliminated states (mark as dead).
    for (idx, redir) in redirect.iter().enumerate() {
        if redir.is_some() {
            nfa.states[idx].transitions.clear();
        }
    }
}

#[cfg(test)]
mod builder_unit_tests {
    use super::*;

    fn build(node: &LoweredNode) -> Nfa {
        NfaBuilder::build(node).unwrap()
    }

    fn new_builder() -> NfaBuilder {
        NfaBuilder {
            states: Vec::new(),
            matchers: Vec::new(),
            sub_nfas: Vec::new(),
            is_approximate: false,
            state_budget: MAX_NFA_STATES,
            lookaround_depth: 0,
        }
    }

    #[test]
    fn new_state_increments_count() {
        let mut builder = new_builder();
        let s0 = builder.new_state().unwrap();
        let s1 = builder.new_state().unwrap();
        assert_eq!(s0.index(), 0);
        assert_eq!(s1.index(), 1);
        assert_eq!(builder.states.len(), 2);
    }

    #[test]
    fn add_transition_connects_states() {
        let mut builder = new_builder();
        let from = builder.new_state().unwrap();
        let to = builder.new_state().unwrap();
        builder.add_transition(from, TransitionKind::Epsilon, to);
        assert_eq!(builder.states[from.index()].transitions.len(), 1);
        assert_eq!(builder.states[from.index()].transitions[0].target, to);
    }

    #[test]
    fn add_epsilon_creates_epsilon_transition() {
        let mut builder = new_builder();
        let from = builder.new_state().unwrap();
        let to = builder.new_state().unwrap();
        builder.add_epsilon(from, to);
        assert!(matches!(
            builder.states[from.index()].transitions[0].kind,
            TransitionKind::Epsilon
        ));
    }

    #[test]
    fn empty_sequence_start_equals_accept() {
        let nfa = build(&LoweredNode::Sequence(vec![]));
        assert_eq!(nfa.start(), nfa.accept());
        assert_eq!(nfa.state_count(), 1);
    }

    #[test]
    fn literal_creates_two_states() {
        let nfa = build(&LoweredNode::Literal('\t'));
        assert_eq!(nfa.state_count(), 2);
    }

    #[test]
    fn literal_has_char_transition() {
        let nfa = build(&LoweredNode::Literal('\n'));
        let trans = nfa.transitions(nfa.start());
        assert_eq!(trans.len(), 1);
        assert!(matches!(trans[0].kind, TransitionKind::Literal(_)));
    }

    #[test]
    fn literal_string_chains_chars() {
        let nfa = build(&LoweredNode::LiteralString("abc".into()));
        assert_eq!(nfa.state_count(), 6);
    }

    #[test]
    fn empty_literal_string_start_equals_accept() {
        let nfa = build(&LoweredNode::LiteralString("".into()));
        assert_eq!(nfa.start(), nfa.accept());
    }

    #[test]
    fn non_capturing_group_same_as_inner() {
        let inner = LoweredNode::Literal('x');
        let grouped = LoweredNode::Group {
            inner: Box::new(inner.clone()),
            capturing: false,
            group: None,
        };
        let nfa_inner = build(&inner);
        let nfa_grouped = build(&grouped);
        assert_eq!(nfa_inner.state_count(), nfa_grouped.state_count());
    }

    #[test]
    fn capturing_group_adds_save_states() {
        let grouped = LoweredNode::Group {
            inner: Box::new(LoweredNode::Literal('a')),
            capturing: true,
            group: Some(CaptureGroup::from_one_based(1)),
        };
        let nfa = build(&grouped);
        assert!(nfa.state_count() > 2);
    }

    #[test]
    fn capturing_group_has_save_transition_at_start() {
        let grouped = LoweredNode::Group {
            inner: Box::new(LoweredNode::Literal('a')),
            capturing: true,
            group: Some(CaptureGroup::from_one_based(1)),
        };
        let nfa = build(&grouped);
        let start_trans = nfa.transitions(nfa.start());
        assert!(!start_trans.is_empty());
        assert!(matches!(start_trans[0].kind, TransitionKind::Save(slot) if slot.index() == 0));
    }

    #[test]
    fn empty_optional_sequence_start_equals_accept() {
        let nfa = build(&LoweredNode::OptionalSequence(vec![]));
        assert_eq!(nfa.start(), nfa.accept());
    }

    #[test]
    fn greedy_star_match_branch_first() {
        let nfa = build(&LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: true,
        });
        let start_trans = nfa.transitions(nfa.start());
        assert!(start_trans.len() >= 2);
        assert!(matches!(start_trans[0].kind, TransitionKind::Epsilon));
        assert!(matches!(start_trans[1].kind, TransitionKind::Epsilon));
        assert_ne!(start_trans[0].target, nfa.accept());
    }

    #[test]
    fn non_greedy_star_skip_branch_first() {
        let nfa = build(&LoweredNode::Quantifier {
            node: Box::new(LoweredNode::Literal('a')),
            min: 0,
            max: None,
            greedy: false,
        });
        let start_trans = nfa.transitions(nfa.start());
        assert!(start_trans.len() >= 2);
        assert_eq!(start_trans[0].target, nfa.accept());
    }

    #[test]
    fn collection_creates_two_states() {
        let nfa = build(&LoweredNode::Collection {
            negated: false,
            items: vec![CollectionItem::Single('\u{20AC}')],
            include_newline: false,
        });
        assert_eq!(nfa.state_count(), 2);
        let trans = nfa.transitions(nfa.start());
        assert_eq!(trans.len(), 1);
        assert!(matches!(trans[0].kind, TransitionKind::Matcher(_)));
    }

    #[test]
    fn last_substitute_creates_placeholder() {
        let nfa = build(&LoweredNode::LastSubstitute);
        assert_eq!(nfa.state_count(), 2);
        let trans = nfa.transitions(nfa.start());
        assert!(matches!(trans[0].kind, TransitionKind::LastSubstitute));
    }

    #[test]
    fn reverse_nfa_reverses_literal() {
        let node = LoweredNode::LiteralString("abc".into());
        let fwd = NfaBuilder::build(&node).unwrap();
        let rev = NfaBuilder::build_reverse(&node).unwrap();
        assert_ne!(fwd.state_count(), 0);
        assert_ne!(rev.state_count(), 0);
    }

    #[test]
    fn reverse_nfa_strips_captures() {
        let node = LoweredNode::Group {
            inner: Box::new(LoweredNode::LiteralString("foo".into())),
            capturing: true,
            group: Some(CaptureGroup::from_one_based(1)),
        };
        let fwd = NfaBuilder::build(&node).unwrap();
        let rev = NfaBuilder::build_reverse(&node).unwrap();
        // Reverse should have fewer states (no Save states)
        assert!(rev.state_count() < fwd.state_count());
        // Verify no Save transitions exist in the reverse NFA
        for sid in rev.states() {
            for trans in rev.transitions(sid) {
                assert!(
                    !matches!(trans.kind, TransitionKind::Save(_)),
                    "reverse NFA should have no Save transitions"
                );
            }
        }
    }

    #[test]
    fn reverse_nfa_reverses_sequence() {
        // Sequence [foo, bar] reversed should match "raboof" (bar reversed + foo reversed)
        let node = LoweredNode::Sequence(vec![
            LoweredNode::LiteralString("foo".into()),
            LoweredNode::LiteralString("bar".into()),
        ]);
        let rev = NfaBuilder::build_reverse(&node).unwrap();
        // The reverse NFA should have states for both literals
        assert!(rev.state_count() > 4);
        // Verify first consuming transition is 'r' (from reversed "bar"),
        // not 'f' (from "foo")
        let start_trans = rev.transitions(rev.start());
        let _first_consuming = start_trans.iter().find(|t| {
            matches!(t.kind, TransitionKind::Literal(_))
                || matches!(t.kind, TransitionKind::Epsilon)
        });
        // Follow epsilons to find the first literal
        let mut sid = rev.start();
        for _ in 0..20 {
            let trans = rev.transitions(sid);
            if trans.is_empty() {
                break;
            }
            match &trans[0].kind {
                TransitionKind::Literal(ch) => {
                    assert_eq!(
                        *ch, 'r',
                        "first literal in reverse of [foo, bar] should be 'r'"
                    );
                    break;
                }
                _ => sid = trans[0].target,
            }
        }
    }

    #[test]
    fn reverse_nfa_strips_lookaround() {
        use crate::ir::LookaroundKind;
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Lookaround {
                inner: Box::new(LoweredNode::LiteralString("foo".into())),
                kind: LookaroundKind::PositiveAhead,
                limit: None,
            },
            LoweredNode::LiteralString("bar".into()),
        ]);
        let fwd = NfaBuilder::build(&node).unwrap();
        let rev = NfaBuilder::build_reverse(&node).unwrap();
        // Forward NFA has sub-NFAs for lookaround; reverse should have none
        assert!(
            !fwd.sub_nfas.is_empty(),
            "forward NFA should have sub-NFAs for lookaround"
        );
        assert!(
            rev.sub_nfas.is_empty(),
            "reverse NFA should strip lookaround (no sub-NFAs)"
        );
        // Reverse should have no Lookaround matchers
        for sid in rev.states() {
            for trans in rev.transitions(sid) {
                if let TransitionKind::Matcher(mid) = &trans.kind {
                    assert!(
                        !matches!(rev.matchers[mid.0 as usize], Matcher::Lookaround(_)),
                        "reverse NFA should have no Lookaround matchers"
                    );
                }
            }
        }
    }

    #[test]
    fn branch_and_creates_lookahead_sub_nfa() {
        use crate::ir::CharClass;
        use crate::ir::CollectionItem;

        // \d\&f → BranchAnd([Collection(\d), Literal('f')])
        let node = LoweredNode::BranchAnd(vec![
            LoweredNode::Collection {
                negated: false,
                items: vec![CollectionItem::Class(CharClass::Digit)],
                include_newline: false,
            },
            LoweredNode::Literal('f'),
        ]);
        let nfa = build(&node);

        // Should have a sub-NFA for the lookahead
        assert_eq!(
            nfa.sub_nfas.len(),
            1,
            "BranchAnd with 2 branches should create 1 sub-NFA"
        );

        // Find the Lookaround matcher
        let mut found_lookaround = false;
        for sid in nfa.states() {
            for trans in nfa.transitions(sid) {
                if let TransitionKind::Matcher(mid) = &trans.kind {
                    if let Matcher::Lookaround(la) = &nfa.matchers[mid.0 as usize] {
                        assert_eq!(la.kind, LookaroundKind::PositiveAhead);
                        assert!(!la.defer_check);
                        found_lookaround = true;
                    }
                }
            }
        }
        assert!(
            found_lookaround,
            "BranchAnd NFA should contain a Lookaround matcher"
        );
    }

    /// After moving Lookaround to the side-table, Transition should be <= 16 bytes.
    #[test]
    fn transition_size_reduced() {
        assert!(
            std::mem::size_of::<Transition>() <= 16,
            "Transition size is {} bytes, expected <= 16",
            std::mem::size_of::<Transition>()
        );
    }

    #[test]
    fn nfa_state_budget_rejects_when_exceeded() {
        let mut builder = NfaBuilder {
            states: Vec::new(),
            matchers: Vec::new(),
            sub_nfas: Vec::new(),
            is_approximate: false,
            state_budget: 3,
            lookaround_depth: 0,
        };
        assert!(builder.new_state().is_ok());
        assert!(builder.new_state().is_ok());
        assert!(builder.new_state().is_ok());
        // 4th allocation exceeds budget of 3
        let err = builder.new_state().unwrap_err();
        assert!(
            format!("{err}").contains("too complex") || format!("{err}").contains("budget"),
            "Expected PatternTooComplex, got: {err}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // QUANTIFIER HINT DETECTION
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn skip_until_byte_detected_for_dot_star_literal() {
        // Pattern: `.*x` -- Quantifier(AnyChar, 0, None, greedy) followed by Literal('x')
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 0,
                max: None,
                greedy: true,
            },
            LoweredNode::Literal('x'),
        ]);
        let nfa = build(&node);
        // The quantifier loop-entry state should have a SkipUntilByte hint
        let mut found_hint = false;
        for sid in nfa.states() {
            if nfa.quantifier_hint(sid) == super::QuantifierHint::SkipUntilByte(b'x') {
                found_hint = true;
                break;
            }
        }
        assert!(
            found_hint,
            "expected SkipUntilByte(b'x') hint on some state"
        );
    }

    #[test]
    fn no_skip_hint_for_non_greedy_dot_star() {
        // Pattern: `.\{-}x` -- non-greedy, no skip hint
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 0,
                max: None,
                greedy: false,
            },
            LoweredNode::Literal('x'),
        ]);
        let nfa = build(&node);
        for sid in nfa.states() {
            assert_eq!(
                nfa.quantifier_hint(sid),
                super::QuantifierHint::None,
                "non-greedy quantifier should not get a skip hint"
            );
        }
    }

    #[test]
    fn skip_hint_for_dot_plus_literal() {
        // Pattern: `.+x` -- min=1, SkipUntilChar still applies since memchr
        // is valid and the min=1 requirement is enforced by match positions.
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 1,
                max: None,
                greedy: true,
            },
            LoweredNode::Literal('x'),
        ]);
        let nfa = build(&node);
        let mut found_hint = false;
        for sid in nfa.states() {
            if nfa.quantifier_hint(sid) == super::QuantifierHint::SkipUntilByte(b'x') {
                found_hint = true;
                break;
            }
        }
        assert!(found_hint, "expected SkipUntilByte(b'x') hint for .+x too");
    }

    #[test]
    fn skip_hint_only_for_ascii_successor() {
        // Pattern: `.*\u{1F600}` -- non-ASCII successor, no skip hint
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 0,
                max: None,
                greedy: true,
            },
            LoweredNode::Literal('\u{1F600}'),
        ]);
        let nfa = build(&node);
        for sid in nfa.states() {
            assert_eq!(
                nfa.quantifier_hint(sid),
                super::QuantifierHint::None,
                "non-ASCII successor should not get a SkipUntilByte hint"
            );
        }
    }

    #[test]
    fn quantifier_hints_vec_sized_to_state_count() {
        let node = LoweredNode::Sequence(vec![
            LoweredNode::Quantifier {
                node: Box::new(LoweredNode::AnyChar),
                min: 0,
                max: None,
                greedy: true,
            },
            LoweredNode::Literal('z'),
        ]);
        let nfa = build(&node);
        assert_eq!(
            nfa.quantifier_hints.len(),
            nfa.state_count(),
            "quantifier_hints should have one entry per NFA state"
        );
    }
}
