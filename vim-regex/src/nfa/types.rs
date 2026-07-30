//! NFA graph types: states, transitions, and construction fragments.

use smallvec::SmallVec;

use crate::ir::LookaroundKind;
use crate::matchers::Matcher;

// ═══════════════════════════════════════════════════════════════════════════════
// STATE ID
// ═══════════════════════════════════════════════════════════════════════════════

/// Unique identifier for a state in the NFA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct StateId(pub(super) u32);

impl StateId {
    /// Returns the raw numeric index of this state.
    #[inline]
    pub(crate) const fn index(self) -> usize {
        self.0 as usize
    }

    /// Create a `StateId` from a raw index.
    #[inline]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "NFA states limited to u32::MAX"
    )]
    pub(crate) const fn from_raw(index: usize) -> Self {
        Self(index as u32)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SIDE TABLE INDICES
// ═══════════════════════════════════════════════════════════════════════════════

/// Index into the NFA's `matchers` side table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MatcherId(pub(super) u32);

/// Index into the NFA's `sub_nfas` side table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubNfaId(pub(super) u32);

impl SubNfaId {
    /// Returns this ID as a `u16` for use as a memo-table key.
    ///
    /// Truncates to `u16::MAX` for IDs >= 65536, which is safe because
    /// patterns with that many lookarounds are astronomically rare and
    /// the memo table simply produces a hash collision rather than UB.
    #[inline]
    pub(crate) fn as_u16(self) -> u16 {
        #[allow(clippy::cast_possible_truncation, reason = "intentional saturation")]
        {
            (self.0).min(u16::MAX as u32) as u16
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CAPTURE SLOT / GROUP NEWTYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// Capture slot index (0..=17 for groups 0-8, open/close pairs).
///
/// Slot `2*g` is the open marker for group `g`, slot `2*g + 1` is the close.
/// Group 0 is the implicit whole-match group; groups 1-9 are `\(...\)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CaptureSlot(pub(super) u8);

impl CaptureSlot {
    /// Create a slot from a raw index.
    #[inline]
    #[allow(
        dead_code,
        reason = "inverse of index(); the backtracker rebuilds a slot from a raw index when restoring captures"
    )]
    pub(crate) const fn from_raw(slot: u8) -> Self {
        Self(slot)
    }

    /// Raw slot index as `usize` (for indexing into capture arrays).
    #[inline]
    pub(crate) const fn index(self) -> usize {
        self.0 as usize
    }

    /// Which capture group this slot belongs to (0-based: 0 = whole match).
    #[inline]
    #[allow(
        dead_code,
        reason = "decodes slot -> group, the inverse of CaptureGroup::open_slot/close_slot; no caller needs it because engines index capture arrays by slot directly"
    )]
    pub(crate) const fn group(self) -> u8 {
        self.0 / 2
    }

    /// Whether this is the opening (start) slot of a group.
    #[inline]
    #[allow(
        dead_code,
        reason = "recovers the open/close parity of a slot; unused because writers already know which end they hold, having asked for open_slot() or close_slot()"
    )]
    pub(crate) const fn is_open(self) -> bool {
        self.0 % 2 == 0
    }
}

/// Capture group number (1..=9 for `\1`..`\9` backreferences).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct CaptureGroup(pub(super) u8);

impl CaptureGroup {
    /// Create a group from a 1-based number (1..=9).
    #[inline]
    pub(crate) const fn from_one_based(n: u8) -> Self {
        debug_assert!(n >= 1 && n <= 9, "CaptureGroup must be 1..=9");
        Self(n)
    }

    /// The 1-based group number.
    #[inline]
    #[allow(
        dead_code,
        reason = "unwraps the 1-based group number; the NFA moves CaptureGroup values around opaquely and only ever converts them to slots, so this is exercised by unit tests alone"
    )]
    pub(crate) const fn number(self) -> u8 {
        self.0
    }

    /// The opening (start) capture slot for this group.
    #[inline]
    pub(crate) const fn open_slot(self) -> CaptureSlot {
        CaptureSlot((self.0 - 1) * 2)
    }

    /// The closing (end) capture slot for this group.
    #[inline]
    pub(crate) const fn close_slot(self) -> CaptureSlot {
        CaptureSlot((self.0 - 1) * 2 + 1)
    }
}

/// A deferred lookbehind check attached to a Pike VM thread.
///
/// Stored in the `Cache::curr_lookbehinds` / `next_lookbehinds` tables,
/// indexed by NFA state. Created when `defer_check` is true on a
/// `Lookaround` transition; resolved at accept time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingLookbehind {
    /// Sub-NFA to run for verification.
    pub(crate) sub_nfa_id: SubNfaId,
    /// Positive or negative lookbehind.
    pub(crate) kind: LookaroundKind,
    /// Window limit for the lookbehind scan.
    pub(crate) limit: Option<u32>,
    /// Text position where the lookbehind was encountered.
    pub(crate) pos: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// NFA GRAPH
// ═══════════════════════════════════════════════════════════════════════════════

/// A non-deterministic finite automaton for Vim regex matching.
///
/// Built by `NfaBuilder` from a `VimPatternNode` tree. Contains a vector
/// of states, a start state, and an accept state.
#[derive(Debug)]
pub(crate) struct Nfa {
    /// All states in the automaton.
    pub(super) states: Vec<NfaState>,
    /// The initial state.
    pub(super) start: StateId,
    /// The accepting (match) state.
    pub(super) accept: StateId,
    /// Side table for heavy matchers (indexed by `MatcherId`).
    pub(super) matchers: Vec<Matcher>,
    /// Side table for lookaround sub-NFAs (indexed by `SubNfaId`).
    pub(super) sub_nfas: Vec<Self>,
    /// Number of capture slots (max `Save` slot + 1), precomputed at build time.
    pub(super) slot_count: usize,
    /// Whether this NFA (or any sub-NFA) contains backreference transitions.
    pub(super) has_backreferences: bool,
    /// Group numbers (1-9) that feed backreference transitions.
    /// Only populated when `has_backreferences` is true.
    pub(super) backref_groups: SmallVec<[CaptureGroup; 9]>,
    /// Per-state flag: true if this state is reachable from any BackRef
    /// transition's target (forward reachability from backref targets to accept).
    ///
    /// States where `backref_reachable[sid]` is true skip VisitedSet dedup
    /// because capture state may affect which path is valid, making
    /// `(state, pos)` alone insufficient for deduplication.
    pub(super) backref_reachable: Vec<bool>,
    /// Pre-computed flag per state: whether the state has any consuming transition.
    /// Indexed by StateId. Computed once at build time, replacing per-visit
    /// `transitions.iter().any(is_consuming)` scans in epsilon closure.
    pub(super) has_consuming: Vec<bool>,
    /// Per-state quantifier fast-path hints (indexed by StateId).
    /// Only populated for states that are quantifier loop entry points
    /// with a detectable skip pattern. Default: `QuantifierHint::None`.
    pub(super) quantifier_hints: Vec<QuantifierHint>,
}

impl Nfa {
    /// Returns the start state.
    #[inline]
    pub(crate) const fn start(&self) -> StateId {
        self.start
    }

    /// Returns the accept state.
    #[inline]
    pub(crate) const fn accept(&self) -> StateId {
        self.accept
    }

    /// Returns the number of states.
    #[inline]
    pub(crate) fn state_count(&self) -> usize {
        self.states.len()
    }

    /// Returns the transitions leaving a given state.
    #[inline]
    pub(crate) fn transitions(&self, id: StateId) -> &[Transition] {
        self.states.get(id.index()).map_or(&[], |s| &s.transitions)
    }

    /// Returns an iterator over all state IDs in the NFA.
    pub(crate) fn states(&self) -> impl Iterator<Item = StateId> {
        (0..self.states.len()).map(StateId::from_raw)
    }

    /// Look up a matcher by its side-table index.
    #[inline]
    #[allow(
        clippy::indexing_slicing,
        reason = "MatcherId is only created by NfaBuilder which guarantees valid indices"
    )]
    pub(crate) fn matcher(&self, id: MatcherId) -> &Matcher {
        &self.matchers[id.0 as usize]
    }

    /// Look up a lookaround sub-NFA by its side-table index.
    #[inline]
    #[allow(
        clippy::indexing_slicing,
        reason = "SubNfaId is only created by NfaBuilder which guarantees valid indices"
    )]
    pub(crate) fn sub_nfa(&self, id: SubNfaId) -> &Self {
        &self.sub_nfas[id.0 as usize]
    }

    /// Returns the precomputed number of capture slots.
    #[inline]
    pub(crate) const fn slot_count(&self) -> usize {
        self.slot_count
    }

    /// Returns whether this NFA (or any nested sub-NFA) contains backreferences.
    #[inline]
    pub(crate) const fn has_backreferences(&self) -> bool {
        self.has_backreferences
    }

    /// Returns the capture group numbers that feed backreference transitions.
    /// Empty if no backreferences exist.
    #[inline]
    #[allow(
        dead_code,
        reason = "reads the list computed by compute_backref_groups() at build time; engines gate on has_backreferences() alone and never need the specific group numbers"
    )]
    pub(crate) fn backref_groups(&self) -> &[CaptureGroup] {
        &self.backref_groups
    }

    /// Whether this state is reachable from a BackRef transition target.
    /// States where this returns true skip VisitedSet dedup.
    #[inline]
    pub(crate) fn backref_reachable(&self, sid: StateId) -> bool {
        self.backref_reachable
            .get(sid.index())
            .copied()
            .unwrap_or(false)
    }

    /// Returns whether the given state has any consuming (input-advancing) transition.
    ///
    /// Consuming transitions: Literal, AnyChar, AnyCharNl, BackRef, LastSubstitute,
    /// Matcher(Char(_)).
    #[inline]
    pub(crate) fn has_consuming(&self, id: StateId) -> bool {
        self.has_consuming.get(id.index()).copied().unwrap_or(false)
    }

    /// Returns the quantifier fast-path hint for a given state.
    ///
    /// Returns `QuantifierHint::None` if no hint exists for this state.
    #[inline]
    pub(crate) fn quantifier_hint(&self, id: StateId) -> QuantifierHint {
        self.quantifier_hints
            .get(id.index())
            .copied()
            .unwrap_or(QuantifierHint::None)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// NFA STATE AND TRANSITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// A single state in the NFA.
///
/// Each state holds a list of outgoing transitions. Most states have
/// 1-2 transitions (hence `SmallVec<[_;2]>`).
#[derive(Debug)]
pub(crate) struct NfaState {
    /// Outgoing transitions from this state.
    pub(super) transitions: SmallVec<[Transition; 2]>,
}

impl NfaState {
    /// Creates a new state with no transitions.
    pub(super) fn new() -> Self {
        Self {
            transitions: SmallVec::new(),
        }
    }
}

/// A transition from one NFA state to another.
#[derive(Debug)]
pub(crate) struct Transition {
    /// What must be satisfied to traverse this transition.
    pub(crate) kind: TransitionKind,
    /// The target state.
    pub(crate) target: StateId,
}

/// The kind of condition on a transition.
#[derive(Debug)]
pub(crate) enum TransitionKind {
    /// Unconditional transition (no input consumed).
    Epsilon,
    /// Record a capture group boundary (slot index).
    Save(CaptureSlot),
    /// Match and consume a literal character (inline, small fixed-size).
    Literal(char),
    /// `.` — match any character except newline (inline).
    AnyChar,
    /// `\_.` — match any character including newline (inline).
    AnyCharNl,
    /// Match via a heavy matcher in the NFA side table.
    Matcher(MatcherId),
    /// Match text previously captured by group `n`.
    BackRef(CaptureGroup),
    /// Placeholder for `~` (last substitute) — resolved at match time.
    LastSubstitute,
}

// ═══════════════════════════════════════════════════════════════════════════════
// NFA FRAGMENT (for Thompson's construction)
// ═══════════════════════════════════════════════════════════════════════════════

/// A fragment of an NFA under construction.
///
/// Thompson's construction builds the NFA bottom-up by combining fragments.
/// Each fragment has a single start state and a single accept state with
/// no outgoing transitions (dangling accept).
#[derive(Debug, Clone, Copy)]
pub(crate) struct NfaFragment {
    /// Entry point of the fragment.
    pub(super) start: StateId,
    /// The dangling accept state (no transitions yet).
    pub(super) accept: StateId,
}

// ═══════════════════════════════════════════════════════════════════════════════
// QUANTIFIER HINTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Hint for quantifier fast-path optimization in the backtracker.
///
/// Detected at NFA build time when a `Quantifier` over `AnyChar` or a
/// negated single-char collection is followed by a literal. The backtracker
/// uses this to skip directly to the next candidate position via `memchr`,
/// eliminating per-character stack frame push/pop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum QuantifierHint {
    /// No fast-path applicable.
    #[default]
    None,
    /// `.*x` pattern: memchr for byte `x` to skip directly.
    SkipUntilByte(u8),
    /// `[^x]*y` pattern: scan past bytes matching `x`, then check for `y`.
    #[allow(
        dead_code,
        reason = "never constructed: builder.rs only emits SkipUntilByte, so no NFA state carries this hint. The backtracker already has a match arm for it that falls through to the normal path"
    )]
    SkipUntilNotByte(u8),
}

#[cfg(test)]
const _QUANTIFIER_HINT_SIZE: () = {
    assert!(std::mem::size_of::<QuantifierHint>() <= 4);
};
