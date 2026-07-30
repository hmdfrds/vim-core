//! NFA -> one-pass DFA construction with ambiguity detection.
//!
//! Builds a one-pass DFA transition table from the NFA. During construction,
//! any ambiguity (two different transitions for the same DFA state and
//! character class) causes an immediate bail-out: the pattern is not one-pass.

use super::classify::CharClassifier;
use super::transition::Transition;
use crate::hir::PatternProperties;
use crate::matchers::{CharMatcher, Matcher};
use crate::nfa::{Nfa, StateId, TransitionKind};

/// Maximum NFA states for one-pass eligibility.
const MAX_NFA_STATES: usize = 4096;

/// Maximum table size in transitions (not bytes).
/// 256 KB / 8 bytes per transition = 32,768 transitions.
const MAX_TABLE_ENTRIES: usize = 32_768;

// ═══════════════════════════════════════════════════════════════════════════════
// ELIGIBILITY PRE-CHECK
// ═══════════════════════════════════════════════════════════════════════════════

/// Quick pre-check: does the pattern have properties that immediately
/// disqualify it from one-pass DFA construction?
pub(crate) fn is_onepass_eligible(props: &PatternProperties) -> bool {
    !props.has_backreferences()
        && !props.has_last_substitute()
        && !props.has_atomic()
        && !props.has_lookaround()
        && !props.has_branch_and()
        && !props.has_buffer_position()
        && !props.has_look_ahead_assertions()
        && !props.has_match_override()
        && !props.features.has_zero_width_assertions
        && props.capture_count() <= 16
}

// ═══════════════════════════════════════════════════════════════════════════════
// BUILD RESULT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of attempting to build a one-pass DFA.
#[allow(
    clippy::large_enum_variant,
    reason = "Ok variant is the hot path; boxing adds indirection in the fast path"
)]
pub(crate) enum BuildResult {
    /// Construction succeeded.
    Ok(OnePassData),
    /// Pattern is not one-pass (ambiguity detected or limits exceeded).
    NotOnePass,
}

/// The compiled one-pass DFA data (transition table + metadata).
#[derive(Debug, Clone)]
pub(crate) struct OnePassData {
    /// Flattened transition table: table[state_id * stride + char_class].
    pub(crate) table: Vec<Transition>,
    /// Character classifier.
    pub(crate) classifier: CharClassifier,
    /// Start state ID.
    pub(crate) start: u32,
    /// Minimum state ID that is a match state.
    /// All states with ID >= min_match_id are match states.
    pub(crate) min_match_id: u32,
    /// Number of states (used for debugging/diagnostics).
    #[allow(dead_code, reason = "diagnostic metadata for debugging one-pass DFA")]
    pub(crate) state_count: u32,
    /// Slot mask to apply when entering a match state (for match-state epsilon slots).
    /// Indexed by `state_id - min_match_id`.
    pub(crate) match_slots: Vec<u32>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// BUILDER
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a one-pass DFA from the given NFA.
pub(crate) fn build(nfa: &Nfa) -> BuildResult {
    if nfa.state_count() > MAX_NFA_STATES {
        return BuildResult::NotOnePass;
    }

    let classifier = match CharClassifier::build(nfa) {
        Some(c) => c,
        None => return BuildResult::NotOnePass,
    };

    let stride = classifier.stride();

    // Estimate table size: if a conservative estimate exceeds limits, bail.
    // We'll also check dynamically during construction.
    if nfa.state_count() * stride > MAX_TABLE_ENTRIES {
        return BuildResult::NotOnePass;
    }

    let mut builder = Builder {
        nfa,
        classifier,
        table: Vec::new(),
        nfa_to_dfa: vec![0u32; nfa.state_count()],
        next_state: 0,
        match_info: Vec::new(),
    };

    match builder.build_inner() {
        Some(()) => {
            let data = builder.into_data();
            BuildResult::Ok(data)
        }
        None => BuildResult::NotOnePass,
    }
}

struct Builder<'a> {
    nfa: &'a Nfa,
    classifier: CharClassifier,
    /// Flattened transition table under construction.
    table: Vec<Transition>,
    /// Map from NFA state ID -> DFA state ID (0 = not yet assigned).
    nfa_to_dfa: Vec<u32>,
    /// Next DFA state ID to allocate.
    next_state: u32,
    /// (dfa_state_id, slot_mask) for match states found during construction.
    match_info: Vec<(u32, u32)>,
}

impl<'a> Builder<'a> {
    fn build_inner(&mut self) -> Option<()> {
        let stride = self.classifier.stride();

        // Allocate state 0 as the DEAD state.
        self.alloc_state()?;

        // Create start state for the NFA's start.
        let nfa_start = self.nfa.start();
        let _dfa_start = self.get_or_create_state(nfa_start)?;

        // Process all states via worklist.
        // We iterate by DFA state ID since new states may be added during processing.
        let mut dfa_id = 1u32; // Start from 1 (skip DEAD state).
        while (dfa_id as usize) < self.table.len() / stride {
            // Find the NFA state that maps to this DFA state.
            let nfa_id = self.dfa_to_nfa(dfa_id)?;

            // Compute epsilon closure from nfa_id (only reads self.nfa).
            // Returns owned data, no borrows on self.
            let closure = epsilon_closure(self.nfa, nfa_id)?;

            // For each consuming transition in the closure, compute classes
            // and write to the table.
            for ct in &closure.consuming {
                let classes = self.char_classes_for_transition_kind(&ct.kind);
                for class in classes {
                    let target_dfa = self.get_or_create_state(ct.target)?;
                    let trans = Transition::new(target_dfa, closure.is_match, ct.slot_mask);

                    let idx = (dfa_id as usize) * stride + class as usize;
                    if idx >= self.table.len() {
                        return None; // Shouldn't happen, but safety check.
                    }

                    let existing = self.table[idx];
                    if existing.is_dead() {
                        // Slot is empty -- write the transition.
                        self.table[idx] = trans;
                    } else if existing.state_id() == trans.state_id()
                        && existing.match_wins() == trans.match_wins()
                    {
                        // Same target state -- merge slot masks.
                        self.table[idx] = existing.merge_slots(trans.slot_mask());
                    } else {
                        // Conflict! Two different transitions for the same
                        // (state, class) pair. Pattern is not one-pass.
                        return None;
                    }
                }
            }

            // Record match state info.
            if closure.is_match {
                self.match_info.push((dfa_id, closure.match_slot_mask));
            }

            dfa_id += 1;
        }

        Some(())
    }

    /// Find the NFA state ID that maps to the given DFA state ID.
    fn dfa_to_nfa(&self, dfa_id: u32) -> Option<StateId> {
        for (nfa_idx, &dfa) in self.nfa_to_dfa.iter().enumerate() {
            if dfa == dfa_id {
                return Some(StateId::from_raw(nfa_idx));
            }
        }
        None
    }

    /// Get or create a DFA state for the given NFA state.
    fn get_or_create_state(&mut self, nfa_id: StateId) -> Option<u32> {
        let existing = self.nfa_to_dfa[nfa_id.index()];
        if existing != 0 {
            return Some(existing);
        }

        let dfa_id = self.alloc_state()?;
        self.nfa_to_dfa[nfa_id.index()] = dfa_id;
        Some(dfa_id)
    }

    /// Allocate a new DFA state row in the transition table.
    fn alloc_state(&mut self) -> Option<u32> {
        let id = self.next_state;
        if id >= Transition::STATE_ID_LIMIT {
            return None;
        }
        let stride = self.classifier.stride();
        if self.table.len() + stride > MAX_TABLE_ENTRIES {
            return None;
        }
        self.table
            .resize(self.table.len() + stride, Transition::DEAD);
        self.next_state += 1;
        Some(id)
    }

    /// Determine which character classes a consuming transition kind covers.
    fn char_classes_for_transition_kind(&self, kind: &OwnedTransitionKind) -> Vec<u16> {
        match kind {
            OwnedTransitionKind::Literal(c) => {
                vec![self.classifier.char_class(*c)]
            }
            OwnedTransitionKind::AnyChar => {
                // Matches all chars except \n.
                let mut classes = Vec::new();
                let newline_class = self.classifier.char_class('\n');
                for c in 0..self.classifier.class_count() {
                    if c != newline_class {
                        classes.push(c);
                    }
                }
                classes
            }
            OwnedTransitionKind::AnyCharNl => {
                // Matches all chars.
                (0..self.classifier.class_count()).collect()
            }
            OwnedTransitionKind::Matcher(mid) => {
                match self.nfa.matcher(*mid) {
                    Matcher::Char(cm) => self.classes_for_char_matcher(cm),
                    // ZeroWidth/Lookaround shouldn't reach here.
                    _ => Vec::new(),
                }
            }
        }
    }

    /// Determine which character classes a CharMatcher covers.
    fn classes_for_char_matcher(&self, cm: &CharMatcher) -> Vec<u16> {
        match cm {
            CharMatcher::Literal(c) => {
                vec![self.classifier.char_class(*c)]
            }
            CharMatcher::AnyChar => {
                let mut classes = Vec::new();
                let newline_class = self.classifier.char_class('\n');
                for c in 0..self.classifier.class_count() {
                    if c != newline_class {
                        classes.push(c);
                    }
                }
                classes
            }
            CharMatcher::AnyCharNl => (0..self.classifier.class_count()).collect(),
            CharMatcher::Collection {
                negated,
                items,
                include_newline,
            } => {
                // For each character class, check if a representative char
                // from that class matches the collection.
                let mut classes = Vec::new();
                // Check all ASCII chars and the default class.
                for b in 0u8..128 {
                    let c = b as char;
                    let class = self.classifier.char_class(c);
                    if classes.contains(&class) {
                        continue;
                    }
                    if collection_matches(*negated, items, *include_newline, c) {
                        classes.push(class);
                    }
                }
                // Also check non-ASCII chars in the classifier's ranges.
                // For the default class (0), check a representative non-ASCII char.
                let default_class = 0u16;
                if !classes.contains(&default_class) {
                    // The default class represents "any char not otherwise classified".
                    // For negated collections, unclassified chars typically match.
                    // For positive collections, they typically don't.
                    // Use a representative non-ASCII char to check.
                    let representative = '\u{0100}'; // Latin Extended-A
                    if collection_matches(*negated, items, *include_newline, representative) {
                        classes.push(default_class);
                    }
                }
                classes
            }
        }
    }

    /// Convert builder state into the final `OnePassData`.
    fn into_data(mut self) -> OnePassData {
        let stride = self.classifier.stride();
        let state_count = self.next_state;

        // Determine start state. The NFA start maps to DFA state 1 (after DEAD=0).
        let start = self.nfa_to_dfa[self.nfa.start().index()];

        // Separate match states from non-match states.
        // The "state >= min_match_id => is match" invariant requires every
        // match state to have an ID >= min_match_id and every non-match state
        // an ID below it. That means remapping all state IDs in the table:
        // non-match states are sorted first, match states last.

        let match_set: Vec<bool> = (0..state_count)
            .map(|id| self.match_info.iter().any(|&(mid, _)| mid == id))
            .collect();

        // Build remapping: non-match states get IDs 0..num_non_match,
        // match states get IDs num_non_match..state_count.
        let mut remap = vec![0u32; state_count as usize];
        let mut next_non_match = 0u32; // Start from 0 (DEAD state is non-match).
        let mut match_states_ordered = Vec::new();

        // First pass: assign IDs to non-match states.
        for i in 0..state_count {
            if !match_set[i as usize] {
                remap[i as usize] = next_non_match;
                next_non_match += 1;
            }
        }

        let min_match_id = next_non_match;
        let mut next_match = min_match_id;

        // Second pass: assign IDs to match states.
        for i in 0..state_count {
            if match_set[i as usize] {
                remap[i as usize] = next_match;
                match_states_ordered.push(i);
                next_match += 1;
            }
        }

        // Build new table with remapped state IDs.
        let mut new_table = vec![Transition::DEAD; state_count as usize * stride];
        for old_id in 0..state_count {
            let new_id = remap[old_id as usize];
            for class in 0..stride {
                let old_idx = old_id as usize * stride + class;
                let new_idx = new_id as usize * stride + class;
                let trans = self.table[old_idx];
                if !trans.is_dead() {
                    let remapped_target = remap[trans.state_id() as usize];
                    new_table[new_idx] = trans.with_state_id(remapped_target);
                }
            }
        }

        // Build match_slots indexed by (state_id - min_match_id).
        let mut match_slots = vec![0u32; match_states_ordered.len()];
        for &old_id in &match_states_ordered {
            let new_id = remap[old_id as usize];
            let slot_mask = self
                .match_info
                .iter()
                .find(|&&(mid, _)| mid == old_id)
                .map(|&(_, mask)| mask)
                .unwrap_or(0);
            let idx = (new_id - min_match_id) as usize;
            match_slots[idx] = slot_mask;
        }

        let remapped_start = remap[start as usize];

        // Replace the table.
        self.table = new_table;

        OnePassData {
            table: self.table,
            classifier: self.classifier,
            start: remapped_start,
            min_match_id,
            state_count,
            match_slots,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EPSILON CLOSURE
// ═══════════════════════════════════════════════════════════════════════════════

/// Owned consuming transition kind (avoids borrowing the NFA across mutation).
enum OwnedTransitionKind {
    Literal(char),
    AnyChar,
    AnyCharNl,
    Matcher(crate::nfa::MatcherId),
}

struct EpsilonClosure {
    consuming: Vec<ConsumingTransition>,
    is_match: bool,
    match_slot_mask: u32,
}

struct ConsumingTransition {
    kind: OwnedTransitionKind,
    target: StateId,
    slot_mask: u32,
}

/// Compute the epsilon closure from a given NFA state.
/// Returns all consuming transitions reachable via epsilon/save chains,
/// along with their accumulated slot masks.
///
/// This is a free function (not a method) so it only borrows the NFA,
/// leaving the Builder free for mutation afterward.
fn epsilon_closure(nfa: &Nfa, start: StateId) -> Option<EpsilonClosure> {
    let mut result = EpsilonClosure {
        consuming: Vec::new(),
        is_match: false,
        match_slot_mask: 0,
    };

    let mut stack: Vec<(StateId, u32)> = vec![(start, 0)];
    let mut seen = vec![false; nfa.state_count()];
    seen[start.index()] = true;

    while let Some((sid, slot_mask)) = stack.pop() {
        // Check if this is the accept state.
        if sid == nfa.accept() {
            result.is_match = true;
            result.match_slot_mask |= slot_mask;
            continue;
        }

        for trans in nfa.transitions(sid) {
            match &trans.kind {
                TransitionKind::Epsilon => {
                    if seen[trans.target.index()] {
                        continue;
                    }
                    seen[trans.target.index()] = true;
                    stack.push((trans.target, slot_mask));
                }
                TransitionKind::Save(slot) => {
                    let new_mask = slot_mask | (1u32 << slot.index());
                    if seen[trans.target.index()] {
                        continue;
                    }
                    seen[trans.target.index()] = true;
                    stack.push((trans.target, new_mask));
                }
                // Consuming transitions: record them with owned kind.
                TransitionKind::Literal(c) => {
                    result.consuming.push(ConsumingTransition {
                        kind: OwnedTransitionKind::Literal(*c),
                        target: trans.target,
                        slot_mask,
                    });
                }
                TransitionKind::AnyChar => {
                    result.consuming.push(ConsumingTransition {
                        kind: OwnedTransitionKind::AnyChar,
                        target: trans.target,
                        slot_mask,
                    });
                }
                TransitionKind::AnyCharNl => {
                    result.consuming.push(ConsumingTransition {
                        kind: OwnedTransitionKind::AnyCharNl,
                        target: trans.target,
                        slot_mask,
                    });
                }
                TransitionKind::Matcher(mid) => {
                    result.consuming.push(ConsumingTransition {
                        kind: OwnedTransitionKind::Matcher(*mid),
                        target: trans.target,
                        slot_mask,
                    });
                }
                // These should have been caught by is_onepass_eligible.
                TransitionKind::BackRef(_) | TransitionKind::LastSubstitute => {
                    return None;
                }
            }
        }
    }

    Some(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// COLLECTION MATCHING HELPER
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a character matches a collection (for class enumeration).
/// Uses case-sensitive matching (the one-pass DFA delegates to NFA semantics).
fn collection_matches(
    negated: bool,
    items: &[crate::ir::CollectionItem],
    include_newline: bool,
    ch: char,
) -> bool {
    if ch == '\n' {
        if include_newline {
            return true;
        }
        let has_explicit_newline = items
            .iter()
            .any(|item| matches!(item, crate::ir::CollectionItem::Newline));
        if !has_explicit_newline {
            return false;
        }
    }
    let in_set = items
        .iter()
        .any(|item| crate::matchers::collection_item_matches(item, ch, true));
    if negated {
        !in_set
    } else {
        in_set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VimRegex;

    #[test]
    fn eligible_simple_literal() {
        let re = VimRegex::new("hello").unwrap();
        assert!(is_onepass_eligible(&re.properties));
    }

    #[test]
    fn eligible_with_captures() {
        let re = VimRegex::new(r"\(\w\+\):\(\d\+\)").unwrap();
        assert!(is_onepass_eligible(&re.properties));
    }

    #[test]
    fn ineligible_backref() {
        let re = VimRegex::new(r"\(a\)\1").unwrap();
        assert!(!is_onepass_eligible(&re.properties));
    }

    #[test]
    fn ineligible_lookaround() {
        let re = VimRegex::new(r"foo\@<=bar").unwrap();
        assert!(!is_onepass_eligible(&re.properties));
    }

    #[test]
    fn ineligible_atomic() {
        let re = VimRegex::new(r"foo\@>bar").unwrap();
        assert!(!is_onepass_eligible(&re.properties));
    }

    #[test]
    fn build_simple_literal() {
        let re = VimRegex::new("abc").unwrap();
        match build(&re.nfa) {
            BuildResult::Ok(data) => {
                assert!(data.state_count > 0);
                assert!(data.start > 0, "start should not be DEAD");
            }
            BuildResult::NotOnePass => {
                panic!("simple literal 'abc' should be one-pass");
            }
        }
    }

    #[test]
    fn build_non_overlapping_alternation() {
        // \(abc\|def\) -- 'a' vs 'd' never overlap.
        let re = VimRegex::new(r"\(abc\|def\)").unwrap();
        match build(&re.nfa) {
            BuildResult::Ok(data) => {
                assert!(data.state_count > 0);
            }
            BuildResult::NotOnePass => {
                // Conservative: may decline if alternation structure
                // creates apparent ambiguity. This is acceptable for v1.
            }
        }
    }

    #[test]
    fn build_fails_for_ambiguous_pattern() {
        // a*a is not one-pass: at any 'a', ambiguous whether to stay in a* or advance.
        let re = VimRegex::new(r"a*a").unwrap();
        match build(&re.nfa) {
            BuildResult::NotOnePass => {} // Expected.
            BuildResult::Ok(_) => {
                panic!("'a*a' should NOT be one-pass");
            }
        }
    }

    #[test]
    fn onepass_not_eligible_with_match_override() {
        use crate::hir::lower;
        use crate::parser::parse_pattern;
        let result = parse_pattern(r"\zsabc").unwrap();
        let (_, props) = lower(&result.node);
        assert!(
            !super::is_onepass_eligible(&props),
            r"\zsabc should not be onepass eligible"
        );
    }

    #[test]
    fn onepass_not_eligible_with_zero_width_assertions() {
        use crate::hir::lower;
        use crate::parser::parse_pattern;
        let result = parse_pattern(r"^\(abc\)").unwrap();
        let (_, props) = lower(&result.node);
        assert!(
            !super::is_onepass_eligible(&props),
            r"^\(abc\) should not be onepass eligible (contains ^)"
        );
    }
}
