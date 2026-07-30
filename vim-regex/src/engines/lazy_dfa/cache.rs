//! DFA state cache with lazy construction and eviction.
//!
//! Stores the lazily-populated transition table, NFA state sets for each
//! DFA state, and acceleration info. Uses a memory budget with thrashing
//! detection to know when to give up and fall back to Pike VM.

use ahash::AHashMap;

use super::byte_classes::ByteClasses;
use super::state_accel::AccelInfo;
use super::state_table::StateTable;
use super::utf8::{
    Utf8Sequences, CONT_MAX, CONT_MIN, LEAD_2B_MIN, LEAD_3B_MIN, LEAD_4B_MAX, LEAD_4B_MIN,
};
use super::TaggedStateId;
use crate::matchers::{collection_item_matches, Matcher, ZeroWidthMatcher};
use crate::nfa::{Nfa, StateId, TransitionKind};

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

const DEFAULT_BUDGET: usize = 4 * 1024 * 1024; // 4 MiB

/// Initial minimum number of cache clears before bail-out is considered.
const INITIAL_MINIMUM_CLEAR_COUNT: u32 = 3;

/// Maximum minimum_clear_count (progressive ceiling).
const MAX_MINIMUM_CLEAR_COUNT: u32 = 16;

/// Efficiency constant: bytes per cached state. If the DFA processes fewer
/// than K bytes per state since the last clear, it's less efficient than
/// Pike VM. RE2 uses K=10.
const BYTES_PER_STATE_THRESHOLD: u64 = 10;

// ═══════════════════════════════════════════════════════════════════════════════
// STATE SAVER — survives cache clears
// ═══════════════════════════════════════════════════════════════════════════════

/// Saved DFA search state that survives a cache reset.
///
/// Before `reset()`, the search loop saves the current DFA state's NFA state
/// set, the search position, and the last match position. After `reset()`,
/// the saved NFA state set is re-inserted into the fresh cache to reconstruct
/// the DFA state, and the search continues from the saved position.
#[derive(Debug, Clone)]
pub(super) enum StateSaver {
    /// No state saved. The search loop has not yet triggered a reset.
    Empty,
    /// State captured before a reset.
    Saved {
        /// The NFA state set of the DFA state that was active at reset time.
        nfa_state_set: smallvec::SmallVec<[u32; 16]>,
        /// The prev_word context of the saved state.
        prev_word: bool,
        /// The byte position in the haystack at reset time.
        pos: usize,
        /// The last recorded match end position, if any.
        last_match: Option<usize>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════════
// BYTE-PROGRESS MARKERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Bit 31 set indicates this is a byte-progress marker, not a real NFA state.
const BYTE_PROGRESS_BIT: u32 = 1 << 31;
/// Bits 29-30 encode the byte position (1, 2, or 3).
const BYTE_POS_SHIFT: u32 = 29;
/// 2-bit mask for byte position (values 1-3).
const BYTE_POS_MASK: u32 = 0x03;
/// Bits 25-28 store a transition index within the NFA state (0-15).
const TRANS_IDX_SHIFT: u32 = 25;
/// 4-bit mask for transition index.
const TRANS_IDX_MASK: u32 = 0x0F;
/// Bits 23-24 store the expected total UTF-8 byte length minus 2 (for wildcards).
/// For non-wildcard transitions this is 0. Values: 0=2-byte, 1=3-byte, 2=4-byte.
const EXPECTED_LEN_SHIFT: u32 = 23;
/// 2-bit mask for expected length code.
const EXPECTED_LEN_MASK: u32 = 0x03;
/// Lower 23 bits store the NFA state index (max 8,388,607).
const NFA_STATE_MASK: u32 = 0x007F_FFFF;

/// Encode a byte-progress marker.
///
/// The marker encodes: which NFA state, which transition within that state,
/// which byte position we've reached, and (for wildcards) the expected total length.
#[inline]
fn encode_byte_progress(nfa_state: u32, trans_idx: u8, byte_pos: u8) -> u32 {
    debug_assert!((1..=3).contains(&byte_pos));
    debug_assert!(nfa_state <= NFA_STATE_MASK);
    debug_assert!(trans_idx <= 15);
    BYTE_PROGRESS_BIT
        | ((byte_pos as u32) << BYTE_POS_SHIFT)
        | ((trans_idx as u32) << TRANS_IDX_SHIFT)
        | nfa_state
}

/// Encode a byte-progress marker for a wildcard (AnyChar/AnyCharNl).
///
/// Stores the expected total byte length (2, 3, or 4) in the expected_len field.
#[inline]
fn encode_wildcard_byte_progress(
    nfa_state: u32,
    trans_idx: u8,
    byte_pos: u8,
    expected_total_len: u8,
) -> u32 {
    debug_assert!((1..=3).contains(&byte_pos));
    debug_assert!(nfa_state <= NFA_STATE_MASK);
    debug_assert!(trans_idx <= 15);
    debug_assert!((2..=4).contains(&expected_total_len));
    let len_code = (expected_total_len - 2) as u32;
    BYTE_PROGRESS_BIT
        | ((byte_pos as u32) << BYTE_POS_SHIFT)
        | ((trans_idx as u32) << TRANS_IDX_SHIFT)
        | (len_code << EXPECTED_LEN_SHIFT)
        | nfa_state
}

/// Returns true if this entry is a byte-progress marker.
#[inline]
fn is_byte_progress(entry: u32) -> bool {
    entry & BYTE_PROGRESS_BIT != 0
}

/// Decode a byte-progress marker into (nfa_state, trans_idx, byte_pos).
#[inline]
fn decode_byte_progress(entry: u32) -> (u32, u8, u8) {
    let nfa_state = entry & NFA_STATE_MASK;
    let byte_pos = ((entry >> BYTE_POS_SHIFT) & BYTE_POS_MASK) as u8;
    let trans_idx = ((entry >> TRANS_IDX_SHIFT) & TRANS_IDX_MASK) as u8;
    (nfa_state, trans_idx, byte_pos)
}

/// Decode the expected total length from a wildcard byte-progress marker.
/// Returns 2, 3, or 4.
#[inline]
fn decode_expected_len(entry: u32) -> u8 {
    (((entry >> EXPECTED_LEN_SHIFT) & EXPECTED_LEN_MASK) as u8) + 2
}

/// Determine the expected UTF-8 byte length from a lead byte.
#[inline]
fn utf8_byte_len_from_lead(lead: u8) -> usize {
    if lead < CONT_MIN {
        1
    } else if lead < LEAD_2B_MIN {
        0 // invalid
    } else if lead < LEAD_3B_MIN {
        2
    } else if lead < LEAD_4B_MIN {
        3
    } else if lead <= LEAD_4B_MAX {
        4
    } else {
        0 // invalid
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// FLAT STATE SETS
// ═══════════════════════════════════════════════════════════════════════════════

/// Contiguous storage for NFA state sets, eliminating per-DFA-state Vec allocations.
///
/// Each DFA state's NFA set is stored as a contiguous slice within `data`,
/// indexed by `(start, len)` pairs in `indices`.
#[derive(Debug)]
struct FlatStateSets {
    /// All NFA state sets concatenated.
    data: Vec<u32>,
    /// Per-DFA-state: (start_offset, length) into `data`.
    indices: Vec<(u32, u32)>,
}

impl FlatStateSets {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Push a new state set, returning its ordinal.
    fn push(&mut self, set: &[u32]) -> usize {
        #[allow(clippy::cast_possible_truncation)]
        let start = self.data.len() as u32;
        #[allow(clippy::cast_possible_truncation)]
        let len = set.len() as u32;
        self.data.extend_from_slice(set);
        self.indices.push((start, len));
        self.indices.len() - 1
    }

    /// Get the NFA state set for a DFA state ordinal.
    #[inline]
    fn get(&self, ordinal: usize) -> &[u32] {
        let (start, len) = self.indices[ordinal];
        &self.data[start as usize..(start + len) as usize]
    }

    /// Number of DFA states stored.
    #[inline]
    fn len(&self) -> usize {
        self.indices.len()
    }

    /// Clear all state sets.
    fn clear(&mut self) {
        self.data.clear();
        self.indices.clear();
    }

    /// Approximate heap memory usage in bytes.
    fn memory_usage(&self) -> usize {
        self.data.capacity() * std::mem::size_of::<u32>()
            + self.indices.capacity() * std::mem::size_of::<(u32, u32)>()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DFA CACHE
// ═══════════════════════════════════════════════════════════════════════════════

/// Lazily-populated DFA state cache.
///
/// Each DFA state corresponds to a set of NFA states (its epsilon closure).
/// Transitions are computed on demand when first encountered during search.
/// The TaggedStateId encoding allows the hot loop to detect special states
/// with a single bit-test.
#[derive(Debug)]
pub(crate) struct DfaCache {
    /// Premultiplied transition table.
    pub(super) table: StateTable,
    /// Byte equivalence classes.
    pub(super) classes: ByteClasses,
    /// NFA state sets per DFA state, stored contiguously to reduce allocations.
    nfa_state_sets: FlatStateSets,
    /// Map from hash of NFA state set to `TaggedStateId` (with collision chaining).
    /// Uses a u64 hash key to eliminate per-lookup Vec<u32> allocation.
    state_map: AHashMap<u64, smallvec::SmallVec<[TaggedStateId; 1]>>,
    /// Acceleration info per state ordinal. `None` = not accelerable.
    pub(super) accel: Vec<Option<AccelInfo>>,
    /// Number of full clears performed.
    clear_count: u32,
    /// Bytes searched since last clear.
    bytes_since_clear: u64,
    /// Approximate memory usage in bytes.
    memory_used: usize,
    /// Memory budget in bytes.
    budget: usize,
    /// Adaptive minimum clear count. Starts at INITIAL_MINIMUM_CLEAR_COUNT (3),
    /// increments after each healthy clear (up to MAX_MINIMUM_CLEAR_COUNT).
    minimum_clear_count: u32,
    /// Number of DFA states that were cached at the time of the last clear.
    /// Used to compute the bytes-per-state efficiency metric.
    states_at_last_clear: u32,
    /// Start states indexed by look context: [prev_newline * 2 + prev_word].
    start_states: [TaggedStateId; 4],
    /// Case sensitivity used to build byte classes.
    case_sensitive: bool,
    /// Whether this pattern uses look-ahead assertions.
    has_look_ahead: bool,
    /// NFA accept state index.
    accept_state: u32,
    /// Reusable epsilon-closure visited set.
    epsilon_seen: Vec<bool>,
    /// Reusable scratch buffer for transition targets.
    scratch_targets: Vec<u32>,
    /// Reusable scratch buffer for byte-progress markers during compute_transition.
    scratch_byte_progress: Vec<u32>,
    /// Reusable scratch buffer for real NFA targets during compute_transition.
    scratch_real_targets: Vec<u32>,
    /// Reusable scratch buffer for epsilon_closure result.
    scratch_closure_result: Vec<u32>,
    /// Reusable stack for epsilon_closure traversal.
    scratch_closure_stack: Vec<u32>,
    /// Reusable visited set for collect_post_assertion_consuming and epsilon_reaches_accept.
    scratch_visited: Vec<bool>,
    /// Reusable stack for epsilon traversal in assertion/accept checks.
    scratch_stack: Vec<u32>,
    /// Per-state: prev_word context (for word-boundary tracking).
    state_prev_word: Vec<bool>,
    /// Per-state: whether the NFA set contains pending look-ahead assertions.
    has_pending_assertions: Vec<bool>,
    /// Parallel to the transition table: whether the transition `(source, class)`
    /// triggers a zero-width assertion match at the position BEFORE consuming.
    /// Layout: `assertion_match_flags[sid.index() + class]`.
    assertion_match_flags: Vec<bool>,
    /// Per-state: whether acceleration analysis has been performed.
    accel_analyzed: Vec<bool>,
    /// Newline-boundary checkpoint buffer populated during forward DFA search.
    /// Each entry is `(byte_offset, dfa_state_ordinal)` recorded when the
    /// scanner crosses a `\n` byte. Cleared at the start of each `dfa_search`.
    pub(crate) newline_checkpoints: Vec<(usize, u32)>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTRUCTION
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(clippy::cast_possible_truncation)]
impl DfaCache {
    /// Create a new DFA cache for the given NFA.
    pub(crate) fn new(nfa: &Nfa, case_sensitive: bool, has_look_ahead: bool) -> Self {
        let classes = ByteClasses::build(nfa, case_sensitive);
        let table = StateTable::new(classes.num_classes());

        let mut cache = Self {
            table,
            classes,
            nfa_state_sets: FlatStateSets::new(),
            state_map: AHashMap::new(),
            accel: Vec::new(),
            clear_count: 0,
            bytes_since_clear: 0,
            memory_used: 0,
            budget: DEFAULT_BUDGET,
            minimum_clear_count: INITIAL_MINIMUM_CLEAR_COUNT,
            states_at_last_clear: 0,
            start_states: [TaggedStateId::UNKNOWN; 4],
            case_sensitive,
            has_look_ahead,
            accept_state: nfa.accept().index() as u32,
            epsilon_seen: vec![false; nfa.state_count()],
            scratch_targets: Vec::new(),
            scratch_byte_progress: Vec::new(),
            scratch_real_targets: Vec::new(),
            scratch_closure_result: Vec::new(),
            scratch_closure_stack: Vec::with_capacity(64),
            scratch_visited: vec![false; nfa.state_count()],
            scratch_stack: Vec::with_capacity(32),
            state_prev_word: Vec::new(),
            has_pending_assertions: Vec::new(),
            assertion_match_flags: Vec::new(),
            accel_analyzed: Vec::new(),
            newline_checkpoints: Vec::new(),
        };

        cache.init_states(nfa);
        cache
    }

    /// Whether this cache was built for case-sensitive matching.
    #[inline]
    pub(crate) const fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Returns the approximate heap memory used by this DFA cache, in bytes.
    pub(crate) fn memory_usage(&self) -> usize {
        use std::mem::size_of;
        let mut total = 0;
        total += self.table.memory_usage();
        total += self.nfa_state_sets.memory_usage();
        total += self.accel.capacity() * size_of::<Option<AccelInfo>>();
        total += self.epsilon_seen.capacity() * size_of::<bool>();
        total += self.scratch_targets.capacity() * size_of::<u32>();
        total += self.scratch_byte_progress.capacity() * size_of::<u32>();
        total += self.scratch_real_targets.capacity() * size_of::<u32>();
        total += self.scratch_closure_result.capacity() * size_of::<u32>();
        total += self.scratch_closure_stack.capacity() * size_of::<u32>();
        total += self.scratch_visited.capacity() * size_of::<bool>();
        total += self.scratch_stack.capacity() * size_of::<u32>();
        total += self.state_prev_word.capacity() * size_of::<bool>();
        total += self.has_pending_assertions.capacity() * size_of::<bool>();
        total += self.assertion_match_flags.capacity() * size_of::<bool>();
        total += self.accel_analyzed.capacity() * size_of::<bool>();
        total
    }

    /// Whether the DFA should give up due to persistent cache inefficiency.
    ///
    /// Uses the RE2-style bytes-per-state metric: if the DFA has processed
    /// fewer than K bytes per cached state since the last clear, AND the
    /// clear count has exceeded the (progressive) minimum, the DFA is less
    /// efficient than Pike VM and should bail.
    #[inline]
    pub(crate) fn should_bail(&self) -> bool {
        if self.clear_count < self.minimum_clear_count {
            return false;
        }
        // If we haven't cached any states since the last reset, bail.
        if self.states_at_last_clear == 0 {
            return self.clear_count >= self.minimum_clear_count;
        }
        // Efficiency check: bytes_since_clear < K * num_cached_states
        self.bytes_since_clear < BYTES_PER_STATE_THRESHOLD * self.states_at_last_clear as u64
    }

    /// Returns the current progressive minimum clear count (for testing).
    #[cfg(test)]
    pub(crate) fn minimum_clear_count(&self) -> u32 {
        self.minimum_clear_count
    }

    /// Thrashing check -- delegates to the bytes-per-state efficiency metric.
    #[inline]
    pub(crate) fn is_thrashing(&self) -> bool {
        self.should_bail()
    }

    /// Whether this pattern uses look-ahead assertions.
    #[allow(
        dead_code,
        reason = "getter for the has_look_ahead flag; every consumer lives in this file and reads the field directly, so nothing calls the accessor"
    )]
    #[inline]
    pub(super) const fn has_look_ahead(&self) -> bool {
        self.has_look_ahead
    }

    /// Whether the DFA must immediately Quit because byte class construction
    /// overflowed (> 255 distinct classes).
    ///
    /// Multi-byte UTF-8 transitions are handled natively via byte-progress
    /// markers in the DFA state composition, so they never force a Quit.
    #[cold]
    #[inline]
    pub(super) const fn must_quit(&self) -> bool {
        self.classes.overflowed()
    }

    /// Record bytes consumed for thrashing detection.
    #[inline]
    pub(super) fn record_bytes(&mut self, n: u64) {
        self.bytes_since_clear += n;
    }

    /// Get start state for the given look context.
    #[inline]
    pub(super) const fn start_state(&self, prev_newline: bool, prev_word: bool) -> TaggedStateId {
        let idx = (prev_newline as usize) * 2 + (prev_word as usize);
        self.start_states[idx]
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATE INITIALIZATION
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(clippy::cast_possible_truncation)]
impl DfaCache {
    /// Initialize dead state and context-sensitive start states.
    fn init_states(&mut self, nfa: &Nfa) {
        // Allocate dead state at ordinal 0. All transitions loop to DEAD.
        // Dead state allocation cannot fail (it's always the first state at index 0).
        let _dead_premul = self
            .table
            .alloc_state_filled(TaggedStateId::DEAD)
            .expect("dead state allocation at index 0 cannot overflow");
        self.nfa_state_sets.push(&[]);
        self.accel.push(None);
        self.accel_analyzed.push(true); // Dead state: analyzed, not accelerable.
        self.state_prev_word.push(false);
        self.has_pending_assertions.push(false);
        // Grow assertion_match_flags parallel to the table.
        let stride = self.table.stride();
        self.assertion_match_flags
            .resize(self.assertion_match_flags.len() + stride, false);

        // Compute 4 context-sensitive start states.
        let seeds = [nfa.start().index() as u32];
        let contexts: [(bool, bool); 4] =
            [(false, false), (false, true), (true, false), (true, true)];

        for (i, &(prev_newline, prev_word)) in contexts.iter().enumerate() {
            let mut start_set = std::mem::take(&mut self.scratch_closure_result);
            start_set.clear();
            self.epsilon_closure(nfa, &seeds, prev_newline, prev_word, &mut start_set);
            let is_match = start_set.contains(&self.accept_state);
            let hash_key = Self::hash_state_key(&start_set, prev_word, self.has_look_ahead);

            let start_id =
                if let Some(existing) = self.lookup_state(hash_key, &start_set, prev_word) {
                    existing
                } else {
                    // Start state allocation should never fail (few states at init time).
                    self.alloc_state(&start_set, is_match, prev_word, nfa, hash_key)
                        .expect("start state allocation cannot overflow at init")
                };
            self.scratch_closure_result = start_set;

            // Tag as START so the search loop can detect it for prefilter restarts.
            self.start_states[i] = start_id.with_start();
        }
    }

    /// Allocate a new DFA state. Returns `None` if the premultiplied index
    /// would overflow the `TaggedStateId` index space.
    fn alloc_state(
        &mut self,
        nfa_set: &[u32],
        is_match: bool,
        prev_word: bool,
        nfa: &Nfa,
        hash_key: u64,
    ) -> Option<TaggedStateId> {
        let premul = self.table.alloc_state()?;

        // Build the tagged ID.
        let mut tags = 0u32;
        if is_match {
            tags |= TaggedStateId::MATCH_BIT;
        }

        let sid = TaggedStateId::new(premul, tags);

        // Compute has_pending_assertions for this state.
        let pending = if self.has_look_ahead {
            self.scan_for_pending_assertions(nfa_set, nfa)
        } else {
            false
        };

        // Track memory.
        let set_mem = nfa_set.len() * 4 + 8; // 4 bytes per u32 + 8 bytes index entry
        self.memory_used += self.table.stride() * 4 + set_mem + 64;

        self.state_map.entry(hash_key).or_default().push(sid);
        self.nfa_state_sets.push(nfa_set);
        self.accel.push(None);
        self.accel_analyzed.push(false);
        self.state_prev_word.push(prev_word);
        self.has_pending_assertions.push(pending);
        // Grow assertion_match_flags parallel to the table.
        let stride = self.table.stride();
        self.assertion_match_flags
            .resize(self.assertion_match_flags.len() + stride, false);
        Some(sid)
    }

    /// Compute a u64 hash key from a sorted NFA state set + prev_word context.
    /// Uses AHash's fast non-cryptographic hashing to avoid Vec allocation.
    fn hash_state_key(nfa_set: &[u32], prev_word: bool, has_look_ahead: bool) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        nfa_set.hash(&mut hasher);
        if has_look_ahead {
            prev_word.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Look up an existing DFA state by hash key, verifying against FlatStateSets on collision.
    fn lookup_state(
        &self,
        hash_key: u64,
        nfa_set: &[u32],
        prev_word: bool,
    ) -> Option<TaggedStateId> {
        let bucket = self.state_map.get(&hash_key)?;
        for &sid in bucket.iter() {
            let ordinal = self.table.premul_to_ordinal(sid.index() as u32);
            let stored_set = self.nfa_state_sets.get(ordinal);
            let stored_prev_word = self.state_prev_word[ordinal];
            if stored_set == nfa_set && (!self.has_look_ahead || stored_prev_word == prev_word) {
                return Some(sid);
            }
        }
        None
    }

    /// Scan an NFA state set for outgoing look-ahead assertion transitions.
    fn scan_for_pending_assertions(&self, nfa_set: &[u32], nfa: &Nfa) -> bool {
        for &nfa_sid in nfa_set {
            // Skip byte-progress markers.
            if is_byte_progress(nfa_sid) {
                continue;
            }
            let state_id = StateId::from_raw(nfa_sid as usize);
            for trans in nfa.transitions(state_id) {
                if let TransitionKind::Matcher(mid) = &trans.kind {
                    if let Matcher::ZeroWidth(zwm) = nfa.matcher(*mid) {
                        if matches!(
                            zwm,
                            ZeroWidthMatcher::EndOfLine
                                | ZeroWidthMatcher::WordBoundaryStart
                                | ZeroWidthMatcher::WordBoundaryEnd
                        ) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TRANSITION COMPUTATION (lazy fill)
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(clippy::cast_possible_truncation)]
impl DfaCache {
    /// Look up or lazily compute a transition.
    ///
    /// Returns the target TaggedStateId, or `None` if budget is exceeded.
    pub(super) fn transition(
        &mut self,
        sid: TaggedStateId,
        class: u8,
        nfa: &Nfa,
    ) -> Option<TaggedStateId> {
        let cached = self.table.next(sid, class);
        if !cached.is_unknown() {
            return Some(cached);
        }

        // Compute the target state lazily.
        let target = self.compute_transition(sid, class, nfa)?;

        // If the target state has been analyzed and found accelerable,
        // set the ACCEL bit so the search loop can detect it with a
        // single bit test instead of ordinal+bounds+Option lookup.
        let target = if self.is_state_accelerable(target) {
            target.with_accel()
        } else {
            target
        };
        self.table.set(sid, class, target);

        // Try to analyze the source state for acceleration now that another
        // transition has been computed. Analysis only succeeds once all
        // transitions are known (no UNKNOWN slots remain).
        self.try_analyze_accel(sid);

        Some(target)
    }

    /// Compute the target state for transition `sid` on `class`.
    fn compute_transition(
        &mut self,
        sid: TaggedStateId,
        class: u8,
        nfa: &Nfa,
    ) -> Option<TaggedStateId> {
        let mut targets = std::mem::take(&mut self.scratch_targets);
        targets.clear();

        let ordinal = self.table.premul_to_ordinal(sid.index() as u32);
        let nfa_set = self.nfa_state_sets.get(ordinal);
        let nfa_set_len = nfa_set.len();

        // Phase 1a: Process regular NFA states (no byte-progress bit).
        for &nfa_sid in &nfa_set[..nfa_set_len] {
            if is_byte_progress(nfa_sid) {
                continue; // Handled in Phase 1b.
            }
            let state_id = StateId::from_raw(nfa_sid as usize);
            for (trans_idx, trans) in nfa.transitions(state_id).iter().enumerate() {
                match &trans.kind {
                    TransitionKind::Literal(ch) => {
                        if ch.len_utf8() == 1 {
                            // Single-byte: direct match.
                            if self.classes.classify(*ch as u8) == class {
                                targets.push(trans.target.index() as u32);
                            }
                        } else {
                            // Multi-byte: check if this class matches the first byte.
                            let seq = Utf8Sequences::single(*ch);
                            let first_range = seq.as_slice()[0];
                            if self.class_matches_utf8_range(class, &first_range) {
                                if seq.len() == 1 {
                                    // Shouldn't happen for multi-byte, but safety.
                                    targets.push(trans.target.index() as u32);
                                } else {
                                    // Push byte-progress marker: position 1 of N.
                                    let marker =
                                        encode_byte_progress(nfa_sid, trans_idx.min(15) as u8, 1);
                                    targets.push(marker);
                                }
                            }
                        }
                    }
                    TransitionKind::AnyChar => {
                        if class != self.classes.classify(b'\n') {
                            let rep = self.classes.representative(class);
                            if rep < CONT_MIN {
                                // ASCII: single byte char, emit target directly.
                                targets.push(trans.target.index() as u32);
                            } else if (LEAD_2B_MIN..=LEAD_4B_MAX).contains(&rep) {
                                // UTF-8 lead byte: start multi-byte wildcard tracking.
                                let expected_len = utf8_byte_len_from_lead(rep) as u8;
                                if expected_len <= 1 {
                                    targets.push(trans.target.index() as u32);
                                } else {
                                    let marker = encode_wildcard_byte_progress(
                                        nfa_sid,
                                        trans_idx.min(15) as u8,
                                        1,
                                        expected_len,
                                    );
                                    targets.push(marker);
                                }
                            }
                            // Continuation bytes (CONT_MIN-CONT_MAX) and invalid bytes
                            // never start a character, so no match from here.
                        }
                    }
                    TransitionKind::AnyCharNl => {
                        let rep = self.classes.representative(class);
                        if rep < CONT_MIN {
                            // ASCII: single byte char, emit target directly.
                            targets.push(trans.target.index() as u32);
                        } else if (LEAD_2B_MIN..=LEAD_4B_MAX).contains(&rep) {
                            // UTF-8 lead byte: start multi-byte wildcard tracking.
                            let expected_len = utf8_byte_len_from_lead(rep) as u8;
                            if expected_len <= 1 {
                                targets.push(trans.target.index() as u32);
                            } else {
                                let marker = encode_wildcard_byte_progress(
                                    nfa_sid,
                                    trans_idx.min(15) as u8,
                                    1,
                                    expected_len,
                                );
                                targets.push(marker);
                            }
                        }
                        // Continuation bytes and invalid bytes don't start a char.
                    }
                    TransitionKind::Matcher(id) => match nfa.matcher(*id) {
                        Matcher::Char(cm) => {
                            let mut quit_flag = false;
                            self.process_char_matcher_transition(
                                cm,
                                class,
                                nfa_sid,
                                trans_idx.min(15) as u8,
                                trans.target.index() as u32,
                                &mut targets,
                                &mut quit_flag,
                            );
                            if quit_flag {
                                self.scratch_targets = targets;
                                return Some(TaggedStateId::QUIT);
                            }
                        }
                        Matcher::ZeroWidth(_) | Matcher::Lookaround(_) => {}
                    },
                    // BackRef and LastSubstitute are consuming transitions
                    // the DFA cannot simulate. Return QUIT immediately.
                    TransitionKind::BackRef(_) | TransitionKind::LastSubstitute => {
                        self.scratch_targets = targets;
                        return Some(TaggedStateId::QUIT);
                    }
                    // Non-consuming transitions.
                    TransitionKind::Epsilon | TransitionKind::Save(_) => {}
                }
            }
        }

        // Phase 1b: Process byte-progress markers.
        for &entry in &nfa_set[..nfa_set_len] {
            if !is_byte_progress(entry) {
                continue;
            }
            let (nfa_state_idx, trans_idx, byte_pos) = decode_byte_progress(entry);
            let state_id = StateId::from_raw(nfa_state_idx as usize);
            let transitions = nfa.transitions(state_id);
            let tidx = trans_idx as usize;
            if tidx >= transitions.len() {
                continue;
            }
            let trans = &transitions[tidx];

            match &trans.kind {
                TransitionKind::Literal(ch) => {
                    let seq = Utf8Sequences::single(*ch);
                    let ranges = seq.as_slice();
                    let pos = byte_pos as usize;
                    if pos < ranges.len() && self.class_matches_utf8_range(class, &ranges[pos]) {
                        if pos == ranges.len() - 1 {
                            // Final byte: emit the transition target.
                            targets.push(trans.target.index() as u32);
                        } else {
                            // More bytes to go.
                            let marker =
                                encode_byte_progress(nfa_state_idx, trans_idx, byte_pos + 1);
                            targets.push(marker);
                        }
                    }
                }
                TransitionKind::AnyChar | TransitionKind::AnyCharNl => {
                    // Wildcard byte-progress: check if this is a valid
                    // continuation byte (CONT_MIN-CONT_MAX).
                    let rep = self.classes.representative(class);
                    if (CONT_MIN..=CONT_MAX).contains(&rep) {
                        // Decode the expected total byte length from the marker.
                        let expected_len = decode_expected_len(entry) as usize;
                        let pos = byte_pos as usize;
                        if pos == expected_len - 1 {
                            // We've consumed all continuation bytes.
                            targets.push(trans.target.index() as u32);
                        } else if pos < expected_len - 1 {
                            // More continuation bytes expected. Preserve the
                            // expected_len encoding when creating the next marker.
                            let marker = encode_wildcard_byte_progress(
                                nfa_state_idx,
                                trans_idx,
                                byte_pos + 1,
                                expected_len as u8,
                            );
                            targets.push(marker);
                        }
                    }
                }
                TransitionKind::Matcher(id) => {
                    if let Matcher::Char(cm) = nfa.matcher(*id) {
                        self.process_char_matcher_byte_progress(
                            cm,
                            class,
                            nfa_state_idx,
                            trans_idx,
                            byte_pos,
                            entry,
                            trans.target.index() as u32,
                            &mut targets,
                        );
                    }
                }
                _ => {}
            }
        }

        // Phase 2: Evaluate look-ahead assertions.
        let mut assertion_match = false;
        if self.has_look_ahead && self.has_pending_assertions[ordinal] {
            assertion_match =
                self.evaluate_assertions_with_match(ordinal, class, nfa, &mut targets);
        }

        // Store the assertion-match flag.
        let flag_idx = sid.index() + class as usize;
        if flag_idx < self.assertion_match_flags.len() {
            self.assertion_match_flags[flag_idx] = assertion_match;
        }

        // If no targets, this is a dead transition.
        if targets.is_empty() {
            self.scratch_targets = targets;
            return Some(TaggedStateId::DEAD);
        }

        // Separate byte-progress markers from real NFA state targets.
        let mut byte_progress_markers = std::mem::take(&mut self.scratch_byte_progress);
        let mut real_targets = std::mem::take(&mut self.scratch_real_targets);
        byte_progress_markers.clear();
        real_targets.clear();

        for &t in &targets {
            if is_byte_progress(t) {
                byte_progress_markers.push(t);
            } else {
                real_targets.push(t);
            }
        }

        // Derive look-context from the consumed byte class.
        let prev_newline = class == self.classes.classify(b'\n');
        let prev_word = self.classes.is_word_class(class);

        // Compute epsilon closure of real targets.
        let mut closure_result = std::mem::take(&mut self.scratch_closure_result);
        closure_result.clear();
        self.epsilon_closure(
            nfa,
            &real_targets,
            prev_newline,
            prev_word,
            &mut closure_result,
        );

        // Return scratch buffers.
        self.scratch_real_targets = real_targets;
        self.scratch_targets = targets;

        // Merge byte-progress markers into the closed set.
        // They sort after regular states (bit 31 set) so dedup works naturally.
        closure_result.extend_from_slice(&byte_progress_markers);
        closure_result.sort_unstable();
        closure_result.dedup();
        self.scratch_byte_progress = byte_progress_markers;

        if closure_result.is_empty() {
            self.scratch_closure_result = closure_result;
            return Some(TaggedStateId::DEAD);
        }

        // Look up existing state or allocate new one.
        let hash_key = Self::hash_state_key(&closure_result, prev_word, self.has_look_ahead);
        if let Some(existing) = self.lookup_state(hash_key, &closure_result, prev_word) {
            self.scratch_closure_result = closure_result;
            return Some(existing);
        }

        // Check budget.
        let projected = self.memory_used + self.table.stride() * 4 + closure_result.len() * 4 + 88;
        if projected > self.budget {
            self.scratch_closure_result = closure_result;
            self.reset(nfa);
            return None;
        }

        let is_match = closure_result
            .iter()
            .any(|&s| !is_byte_progress(s) && s == self.accept_state);
        let sid = match self.alloc_state(&closure_result, is_match, prev_word, nfa, hash_key) {
            Some(sid) => sid,
            None => {
                // Premultiplied index overflow — clear cache and signal Quit.
                self.scratch_closure_result = closure_result;
                self.reset(nfa);
                return None;
            }
        };
        self.scratch_closure_result = closure_result;
        Some(sid)
    }

    /// Check if a byte-class contains any byte within the given Utf8Range.
    ///
    /// Since all bytes in the same class produce identical transitions, we only
    /// need to check if the representative byte for this class falls within
    /// the Utf8Range.
    fn class_matches_utf8_range(&self, class: u8, range: &super::utf8::Utf8Range) -> bool {
        let rep = self.classes.representative(class);
        range.matches(rep)
    }

    /// Process a CharMatcher transition for Phase 1a (initial byte).
    ///
    /// For collections with non-ASCII items, decomposes them into UTF-8
    /// byte sequences and pushes byte-progress markers as needed.
    ///
    /// Sets `quit` to `true` if the DFA cannot handle this matcher
    /// (negated collection with non-ASCII items). The caller must return QUIT.
    #[allow(clippy::too_many_arguments)]
    fn process_char_matcher_transition(
        &self,
        cm: &crate::matchers::CharMatcher,
        class: u8,
        nfa_sid: u32,
        trans_idx: u8,
        target: u32,
        targets: &mut Vec<u32>,
        quit: &mut bool,
    ) {
        use crate::matchers::CharMatcher;

        match cm {
            CharMatcher::Literal(ch) => {
                if ch.len_utf8() == 1 {
                    if self.classes.classify(*ch as u8) == class {
                        targets.push(target);
                    }
                } else {
                    let seq = Utf8Sequences::single(*ch);
                    let first_range = seq.as_slice()[0];
                    if self.class_matches_utf8_range(class, &first_range) {
                        if seq.len() == 1 {
                            targets.push(target);
                        } else {
                            let marker = encode_byte_progress(nfa_sid, trans_idx, 1);
                            targets.push(marker);
                        }
                    }
                }
            }
            CharMatcher::AnyChar => {
                if class != self.classes.classify(b'\n') {
                    let rep = self.classes.representative(class);
                    if rep < CONT_MIN {
                        targets.push(target);
                    } else if (LEAD_2B_MIN..=LEAD_4B_MAX).contains(&rep) {
                        let expected_len = utf8_byte_len_from_lead(rep) as u8;
                        if expected_len <= 1 {
                            targets.push(target);
                        } else {
                            let marker =
                                encode_wildcard_byte_progress(nfa_sid, trans_idx, 1, expected_len);
                            targets.push(marker);
                        }
                    }
                }
            }
            CharMatcher::AnyCharNl => {
                let rep = self.classes.representative(class);
                if rep < CONT_MIN {
                    targets.push(target);
                } else if (LEAD_2B_MIN..=LEAD_4B_MAX).contains(&rep) {
                    let expected_len = utf8_byte_len_from_lead(rep) as u8;
                    if expected_len <= 1 {
                        targets.push(target);
                    } else {
                        let marker =
                            encode_wildcard_byte_progress(nfa_sid, trans_idx, 1, expected_len);
                        targets.push(marker);
                    }
                }
            }
            CharMatcher::Collection {
                negated,
                items,
                include_newline,
            } => {
                self.process_collection_transition(
                    items,
                    *negated,
                    *include_newline,
                    class,
                    nfa_sid,
                    trans_idx,
                    target,
                    targets,
                    quit,
                );
            }
        }
    }

    /// Process a collection transition at the first byte position.
    ///
    /// For ASCII bytes, checks directly. For non-ASCII items, decomposes
    /// into UTF-8 sequences and pushes byte-progress markers.
    ///
    /// Sets `quit` to `true` if the DFA cannot handle this collection
    /// (negated collection with non-ASCII items). The caller must return QUIT.
    #[allow(clippy::too_many_arguments)]
    fn process_collection_transition(
        &self,
        items: &[crate::ir::CollectionItem],
        negated: bool,
        include_newline: bool,
        class: u8,
        nfa_sid: u32,
        trans_idx: u8,
        target: u32,
        targets: &mut Vec<u32>,
        quit: &mut bool,
    ) {
        let representative = self.classes.representative(class);

        // ASCII representative: check directly using existing logic.
        if representative < CONT_MIN {
            let ch = representative as char;
            if ch == '\n' {
                if include_newline {
                    targets.push(target);
                    return;
                }
                let has_explicit_nl = items
                    .iter()
                    .any(|item| matches!(item, crate::ir::CollectionItem::Newline));
                if !has_explicit_nl && !negated {
                    return;
                }
                if has_explicit_nl && negated {
                    return;
                }
                if !has_explicit_nl && negated {
                    targets.push(target);
                    return;
                }
            }
            let in_set = items
                .iter()
                .any(|item| collection_item_matches(item, ch, self.case_sensitive));
            if negated {
                if !in_set {
                    targets.push(target);
                }
            } else if in_set {
                targets.push(target);
            }
            return;
        }

        // Non-ASCII representative: this byte class is a UTF-8 lead byte or
        // continuation byte. We need to check if any collection item's UTF-8
        // decomposition has a first byte matching this class.
        //
        // For non-negated collections: if any item starts with this byte class,
        // push a byte-progress marker (or direct target for 1-byte items).
        //
        // For negated collections: a non-ASCII byte class could start characters
        // NOT in the set. We push a byte-progress marker and resolve at the
        // final byte.
        if !negated {
            let mut matched = false;
            for item in items {
                match item {
                    crate::ir::CollectionItem::Single(ch) if ch.len_utf8() > 1 => {
                        let seq = Utf8Sequences::single(*ch);
                        let first_range = seq.as_slice()[0];
                        if self.class_matches_utf8_range(class, &first_range) {
                            if seq.len() == 1 {
                                targets.push(target);
                                matched = true;
                            } else {
                                let marker = encode_byte_progress(nfa_sid, trans_idx, 1);
                                targets.push(marker);
                                matched = true;
                            }
                        }
                    }
                    crate::ir::CollectionItem::Range(lo, hi)
                        if !lo.is_ascii() || !hi.is_ascii() =>
                    {
                        for seq in Utf8Sequences::new(*lo, *hi) {
                            let first_range = seq.as_slice()[0];
                            if self.class_matches_utf8_range(class, &first_range) {
                                if seq.len() == 1 {
                                    targets.push(target);
                                } else {
                                    let marker = encode_byte_progress(nfa_sid, trans_idx, 1);
                                    targets.push(marker);
                                }
                                matched = true;
                                break; // One marker per transition is sufficient.
                            }
                        }
                    }
                    _ => {} // ASCII items already handled above.
                }
                if matched {
                    break;
                }
            }
        } else {
            // Negated collection: check if any item contains non-ASCII characters.
            let has_non_ascii_items = items.iter().any(|item| match item {
                crate::ir::CollectionItem::Single(ch) => !ch.is_ascii(),
                crate::ir::CollectionItem::Range(lo, hi) => !lo.is_ascii() || !hi.is_ascii(),
                // CharClass and PosixClass items are ASCII-only by definition.
                crate::ir::CollectionItem::Class(_)
                | crate::ir::CollectionItem::PosixClass(_)
                | crate::ir::CollectionItem::Newline => false,
            });

            if has_non_ascii_items {
                // Cannot validate negated non-ASCII collections at byte level.
                // Signal the caller to return QUIT (fall through to Pike VM).
                *quit = true;
                return;
            }

            // All-ASCII negated collection: any multi-byte character is definitionally
            // not in the set, so it matches. Use wildcard byte-progress to consume
            // valid UTF-8 sequences.
            if (LEAD_2B_MIN..=LEAD_4B_MAX).contains(&representative) {
                let expected_len = utf8_byte_len_from_lead(representative) as u8;
                if expected_len >= 2 {
                    let marker = encode_wildcard_byte_progress(nfa_sid, trans_idx, 1, expected_len);
                    targets.push(marker);
                }
            }
        }
    }

    /// Process a CharMatcher at byte-progress position > 0.
    ///
    /// Called when we're partway through a multi-byte UTF-8 sequence and
    /// need to check if the current class matches the next expected byte.
    #[allow(clippy::too_many_arguments)]
    fn process_char_matcher_byte_progress(
        &self,
        cm: &crate::matchers::CharMatcher,
        class: u8,
        nfa_state_idx: u32,
        trans_idx: u8,
        byte_pos: u8,
        entry: u32,
        target: u32,
        targets: &mut Vec<u32>,
    ) {
        use crate::matchers::CharMatcher;

        match cm {
            CharMatcher::Literal(ch) => {
                let seq = Utf8Sequences::single(*ch);
                let ranges = seq.as_slice();
                let pos = byte_pos as usize;
                if pos < ranges.len() && self.class_matches_utf8_range(class, &ranges[pos]) {
                    if pos == ranges.len() - 1 {
                        targets.push(target);
                    } else {
                        let marker = encode_byte_progress(nfa_state_idx, trans_idx, byte_pos + 1);
                        targets.push(marker);
                    }
                }
            }
            CharMatcher::Collection {
                negated,
                items,
                include_newline: _,
            } => {
                let pos = byte_pos as usize;
                if !*negated {
                    // Non-negated: check if any item's UTF-8 sequence at this byte position matches.
                    let mut matched = false;
                    for item in items.iter() {
                        if matched {
                            break;
                        }
                        match item {
                            crate::ir::CollectionItem::Single(ch) if ch.len_utf8() > 1 => {
                                let seq = Utf8Sequences::single(*ch);
                                let ranges = seq.as_slice();
                                if pos < ranges.len()
                                    && self.class_matches_utf8_range(class, &ranges[pos])
                                {
                                    if pos == ranges.len() - 1 {
                                        targets.push(target);
                                    } else {
                                        let marker = encode_byte_progress(
                                            nfa_state_idx,
                                            trans_idx,
                                            byte_pos + 1,
                                        );
                                        targets.push(marker);
                                    }
                                    matched = true;
                                }
                            }
                            crate::ir::CollectionItem::Range(lo, hi)
                                if !lo.is_ascii() || !hi.is_ascii() =>
                            {
                                for seq in Utf8Sequences::new(*lo, *hi) {
                                    let ranges = seq.as_slice();
                                    if pos < ranges.len()
                                        && self.class_matches_utf8_range(class, &ranges[pos])
                                    {
                                        if pos == ranges.len() - 1 {
                                            targets.push(target);
                                        } else {
                                            let marker = encode_byte_progress(
                                                nfa_state_idx,
                                                trans_idx,
                                                byte_pos + 1,
                                            );
                                            targets.push(marker);
                                        }
                                        matched = true;
                                        break;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    // All-ASCII negated collection with wildcard byte-progress.
                    // Non-ASCII negated collections are QUIT'd at Phase 1a, so
                    // this path only handles all-ASCII negated collections using
                    // wildcard markers. Continuation byte validation only.
                    let rep = self.classes.representative(class);
                    if (CONT_MIN..=CONT_MAX).contains(&rep) {
                        let expected_len = decode_expected_len(entry) as usize;
                        let pos = byte_pos as usize;
                        if pos == expected_len - 1 {
                            targets.push(target);
                        } else if pos < expected_len - 1 {
                            let marker = encode_wildcard_byte_progress(
                                nfa_state_idx,
                                trans_idx,
                                byte_pos + 1,
                                expected_len as u8,
                            );
                            targets.push(marker);
                        }
                    }
                }
            }
            CharMatcher::AnyChar | CharMatcher::AnyCharNl => {
                let rep = self.classes.representative(class);
                if (CONT_MIN..=CONT_MAX).contains(&rep) {
                    let expected_len = decode_expected_len(entry) as usize;
                    let pos = byte_pos as usize;
                    if pos == expected_len - 1 {
                        // Final continuation byte consumed.
                        targets.push(target);
                    } else if pos < expected_len - 1 {
                        // More continuation bytes expected.
                        let marker = encode_wildcard_byte_progress(
                            nfa_state_idx,
                            trans_idx,
                            byte_pos + 1,
                            expected_len as u8,
                        );
                        targets.push(marker);
                    }
                }
            }
        }
    }

    /// Check if a consuming transition matches the given byte class.
    ///
    /// This is only used for assertion handling (post-assertion consuming
    /// transitions). Multi-byte transitions are handled by the Phase 1a/1b
    /// logic in `compute_transition`.
    fn transition_matches_byte_class(&self, kind: &TransitionKind, class: u8, nfa: &Nfa) -> bool {
        match kind {
            TransitionKind::Literal(ch) => {
                if ch.len_utf8() == 1 {
                    self.classes.classify(*ch as u8) == class
                } else {
                    // Multi-byte literals are handled by byte-progress markers.
                    // For assertion post-processing, match on the first byte only.
                    let seq = Utf8Sequences::single(*ch);
                    let first_range = seq.as_slice()[0];
                    self.class_matches_utf8_range(class, &first_range)
                }
            }
            TransitionKind::AnyChar => class != self.classes.classify(b'\n'),
            TransitionKind::AnyCharNl => true,
            TransitionKind::Matcher(id) => match nfa.matcher(*id) {
                Matcher::Char(cm) => self.char_matcher_matches_byte_class(cm, class),
                Matcher::ZeroWidth(_) | Matcher::Lookaround(_) => false,
            },
            // Non-consuming transitions.
            TransitionKind::Epsilon
            | TransitionKind::Save(_)
            | TransitionKind::BackRef(_)
            | TransitionKind::LastSubstitute => false,
        }
    }

    /// Check if a CharMatcher matches any byte in the given class.
    ///
    /// Uses O(1) representative lookup instead of scanning 0..255.
    fn char_matcher_matches_byte_class(
        &self,
        cm: &crate::matchers::CharMatcher,
        class: u8,
    ) -> bool {
        let representative = self.classes.representative(class);
        if representative < CONT_MIN {
            self.byte_matches_char_matcher(cm, representative)
        } else {
            // Non-ASCII class: only AnyChar/AnyCharNl matchers can match.
            matches!(
                cm,
                crate::matchers::CharMatcher::AnyChar | crate::matchers::CharMatcher::AnyCharNl
            )
        }
    }

    /// Test if a single ASCII byte matches a CharMatcher.
    fn byte_matches_char_matcher(&self, cm: &crate::matchers::CharMatcher, byte: u8) -> bool {
        use crate::matchers::CharMatcher;
        if byte >= CONT_MIN {
            return matches!(cm, CharMatcher::AnyChar | CharMatcher::AnyCharNl);
        }
        let ch = byte as char;
        match cm {
            CharMatcher::Literal(c) => {
                if self.case_sensitive {
                    ch == *c
                } else {
                    ch.eq_ignore_ascii_case(c)
                }
            }
            CharMatcher::AnyChar => ch != '\n',
            CharMatcher::AnyCharNl => true,
            CharMatcher::Collection {
                negated,
                items,
                include_newline,
            } => {
                if ch == '\n' {
                    if *include_newline {
                        return true;
                    }
                    let has_explicit_nl = items
                        .iter()
                        .any(|item| matches!(item, crate::ir::CollectionItem::Newline));
                    if !has_explicit_nl {
                        return false;
                    }
                }
                let in_set = items
                    .iter()
                    .any(|item| collection_item_matches(item, ch, self.case_sensitive));
                if *negated {
                    !in_set
                } else {
                    in_set
                }
            }
        }
    }

    /// Evaluate look-ahead assertions for Phase 2 of transition computation.
    ///
    /// Returns `true` if any assertion leads to accept (zero-width match at
    /// this position).
    fn evaluate_assertions_with_match(
        &mut self,
        ordinal: usize,
        class: u8,
        nfa: &Nfa,
        targets: &mut Vec<u32>,
    ) -> bool {
        let next_is_newline = class == self.classes.classify(b'\n');
        let next_is_word = self.classes.is_word_class(class);
        let source_prev_word = self.state_prev_word[ordinal];

        // Collect assertion targets into a local buffer to avoid holding a borrow
        // on self.nfa_state_sets while calling &mut self methods.
        let mut assertion_targets: smallvec::SmallVec<[u32; 8]> = smallvec::SmallVec::new();
        {
            let nfa_set = self.nfa_state_sets.get(ordinal);
            let nfa_set_len = nfa_set.len();
            for &nfa_sid in &nfa_set[..nfa_set_len] {
                // Skip byte-progress markers.
                if is_byte_progress(nfa_sid) {
                    continue;
                }
                let state_id = StateId::from_raw(nfa_sid as usize);
                for trans in nfa.transitions(state_id) {
                    if let TransitionKind::Matcher(mid) = &trans.kind {
                        if let Matcher::ZeroWidth(zwm) = nfa.matcher(*mid) {
                            let satisfied = match zwm {
                                ZeroWidthMatcher::EndOfLine => next_is_newline,
                                ZeroWidthMatcher::WordBoundaryStart => {
                                    !source_prev_word && next_is_word
                                }
                                ZeroWidthMatcher::WordBoundaryEnd => {
                                    source_prev_word && !next_is_word
                                }
                                _ => continue,
                            };
                            if satisfied {
                                assertion_targets.push(trans.target.index() as u32);
                            }
                        }
                    }
                }
            }
        }

        let mut assertion_match = false;
        for target in assertion_targets {
            // Check if assertion leads to accept (zero-width match)
            if self.epsilon_reaches_accept(nfa, target) {
                assertion_match = true;
            }
            // Follow assertion target's consuming transitions
            self.collect_post_assertion_consuming(nfa, target, class, targets);
        }
        assertion_match
    }

    /// Collect consuming transitions reachable through the assertion target
    /// via epsilon/save transitions.
    ///
    /// Uses reusable scratch buffers to avoid per-call allocations.
    fn collect_post_assertion_consuming(
        &mut self,
        nfa: &Nfa,
        assertion_target: u32,
        class: u8,
        targets: &mut Vec<u32>,
    ) {
        let nfa_count = nfa.state_count();

        // Ensure scratch_visited is large enough.
        self.scratch_visited.resize(nfa_count, false);

        let mut stack = std::mem::take(&mut self.scratch_stack);
        stack.clear();
        stack.push(assertion_target);

        if (assertion_target as usize) < nfa_count {
            self.scratch_visited[assertion_target as usize] = true;
        }

        while let Some(sid) = stack.pop() {
            let state_id = StateId::from_raw(sid as usize);
            for trans in nfa.transitions(state_id) {
                let target = trans.target.index() as u32;

                // Check consuming transitions against the class.
                if self.transition_matches_byte_class(&trans.kind, class, nfa) {
                    targets.push(target);
                    continue;
                }

                // Follow epsilon/save transitions.
                if (target as usize) >= nfa_count || self.scratch_visited[target as usize] {
                    continue;
                }
                let follow = match &trans.kind {
                    TransitionKind::Epsilon | TransitionKind::Save(_) => true,
                    TransitionKind::Matcher(id) => matches!(
                        nfa.matcher(*id),
                        Matcher::ZeroWidth(
                            ZeroWidthMatcher::SetMatchStart | ZeroWidthMatcher::SetMatchEnd,
                        )
                    ),
                    _ => false,
                };
                if follow {
                    self.scratch_visited[target as usize] = true;
                    stack.push(target);
                }
            }
        }

        // Reset visited entries.
        self.scratch_visited[..nfa_count].fill(false);
        self.scratch_stack = stack;
    }

    /// Whether the transition `(sid, class)` triggers a zero-width assertion
    /// match at the position BEFORE consuming the character.
    #[inline]
    pub(super) fn has_assertion_match(&self, sid: TaggedStateId, class: u8) -> bool {
        if !self.has_look_ahead {
            return false;
        }
        let ordinal = self.table.premul_to_ordinal(sid.index() as u32);
        if ordinal >= self.has_pending_assertions.len() || !self.has_pending_assertions[ordinal] {
            return false;
        }
        let idx = sid.index() + class as usize;
        self.assertion_match_flags
            .get(idx)
            .copied()
            .unwrap_or(false)
    }

    /// Check end-of-text assertions for the current DFA state.
    ///
    /// Called once after the byte loop to handle $ at EOT, \> at EOT.
    pub(super) fn check_eot_assertions(&mut self, sid: TaggedStateId, nfa: &Nfa) -> bool {
        if !self.has_look_ahead {
            return false;
        }
        let ordinal = self.table.premul_to_ordinal(sid.index() as u32);
        if ordinal >= self.has_pending_assertions.len() || !self.has_pending_assertions[ordinal] {
            return false;
        }

        let source_prev_word = self.state_prev_word[ordinal];

        // Collect assertion targets to avoid holding a borrow on nfa_state_sets.
        let mut eot_targets: smallvec::SmallVec<[u32; 4]> = smallvec::SmallVec::new();
        {
            let nfa_set = self.nfa_state_sets.get(ordinal);
            let nfa_set_len = nfa_set.len();
            for &nfa_sid in &nfa_set[..nfa_set_len] {
                // Skip byte-progress markers.
                if is_byte_progress(nfa_sid) {
                    continue;
                }
                let state_id = StateId::from_raw(nfa_sid as usize);
                for trans in nfa.transitions(state_id) {
                    if let TransitionKind::Matcher(mid) = &trans.kind {
                        if let Matcher::ZeroWidth(zwm) = nfa.matcher(*mid) {
                            let satisfied = match zwm {
                                ZeroWidthMatcher::EndOfLine => true,
                                ZeroWidthMatcher::WordBoundaryEnd => source_prev_word,
                                ZeroWidthMatcher::WordBoundaryStart => false,
                                _ => continue,
                            };
                            if satisfied {
                                eot_targets.push(trans.target.index() as u32);
                            }
                        }
                    }
                }
            }
        }

        for target in eot_targets {
            if self.epsilon_reaches_accept(nfa, target) {
                return true;
            }
        }
        false
    }

    /// Check if the NFA accept state is reachable from `start` through
    /// epsilon/save transitions only.
    ///
    /// Uses reusable scratch buffers to avoid per-call allocations.
    fn epsilon_reaches_accept(&mut self, nfa: &Nfa, start: u32) -> bool {
        let nfa_count = nfa.state_count();

        // Ensure scratch_visited is large enough.
        self.scratch_visited.resize(nfa_count, false);

        let mut stack = std::mem::take(&mut self.scratch_stack);
        stack.clear();
        stack.push(start);

        if (start as usize) < nfa_count {
            self.scratch_visited[start as usize] = true;
        }

        let mut found = false;
        while let Some(sid) = stack.pop() {
            if sid == self.accept_state {
                found = true;
                break;
            }
            let state_id = StateId::from_raw(sid as usize);
            for trans in nfa.transitions(state_id) {
                let target = trans.target.index() as u32;
                if (target as usize) >= nfa_count || self.scratch_visited[target as usize] {
                    continue;
                }
                let follow = match &trans.kind {
                    TransitionKind::Epsilon | TransitionKind::Save(_) => true,
                    TransitionKind::Matcher(id) => matches!(
                        nfa.matcher(*id),
                        Matcher::ZeroWidth(
                            ZeroWidthMatcher::SetMatchStart | ZeroWidthMatcher::SetMatchEnd,
                        )
                    ),
                    _ => false,
                };
                if follow {
                    self.scratch_visited[target as usize] = true;
                    stack.push(target);
                }
            }
        }

        self.scratch_visited[..nfa_count].fill(false);
        self.scratch_stack = stack;
        found
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EPSILON CLOSURE
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(clippy::cast_possible_truncation)]
impl DfaCache {
    /// Compute epsilon closure of a set of NFA states.
    ///
    /// Byte-progress markers are NOT epsilon-closed (they are synthetic
    /// position markers, not real NFA states). They should not be passed
    /// as seeds to this function — they are merged into the state set
    /// separately by `compute_transition`.
    ///
    /// Results are appended to `result` (which should be pre-cleared by the caller).
    fn epsilon_closure(
        &mut self,
        nfa: &Nfa,
        seeds: &[u32],
        prev_newline: bool,
        _prev_word: bool,
        result: &mut Vec<u32>,
    ) {
        let nfa_count = nfa.state_count();
        if self.epsilon_seen.len() < nfa_count {
            self.epsilon_seen.resize(nfa_count, false);
        }

        let mut stack = std::mem::take(&mut self.scratch_closure_stack);
        stack.clear();
        let seen = &mut self.epsilon_seen;

        for &s in seeds {
            // Skip byte-progress markers — they are not real NFA states.
            if is_byte_progress(s) {
                continue;
            }
            if (s as usize) < nfa_count && !seen[s as usize] {
                seen[s as usize] = true;
                stack.push(s);
            }
        }

        while let Some(sid) = stack.pop() {
            result.push(sid);
            let state_id = StateId::from_raw(sid as usize);

            for trans in nfa.transitions(state_id) {
                let target = trans.target.index() as u32;
                if (target as usize) >= nfa_count || seen[target as usize] {
                    continue;
                }

                let follow = match &trans.kind {
                    TransitionKind::Epsilon => true,
                    TransitionKind::Save(_) => true,
                    TransitionKind::Matcher(id) => match nfa.matcher(*id) {
                        Matcher::ZeroWidth(ZeroWidthMatcher::StartOfLine) => prev_newline,
                        Matcher::ZeroWidth(ZeroWidthMatcher::EndOfLine) => false,
                        Matcher::ZeroWidth(ZeroWidthMatcher::WordBoundaryStart) => false,
                        Matcher::ZeroWidth(ZeroWidthMatcher::WordBoundaryEnd) => false,
                        Matcher::ZeroWidth(_) => true,
                        Matcher::Char(_) => false,
                        Matcher::Lookaround(la) => la.defer_check,
                    },
                    _ => false,
                };

                if follow {
                    seen[target as usize] = true;
                    stack.push(target);
                }
            }
        }

        // Return scratch stack buffer.
        self.scratch_closure_stack = stack;

        // Clean visited bits.
        for &s in result.iter() {
            self.epsilon_seen[s as usize] = false;
        }
        for &s in seeds {
            if !is_byte_progress(s) && (s as usize) < nfa_count {
                self.epsilon_seen[s as usize] = false;
            }
        }

        result.sort_unstable();
        result.dedup();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CACHE MANAGEMENT
// ═══════════════════════════════════════════════════════════════════════════════

impl DfaCache {
    /// Full reset and re-initialization.
    #[cold]
    pub(super) fn reset(&mut self, nfa: &Nfa) {
        // Record efficiency before clearing.
        let num_states = self.table.state_count() as u32;
        let was_healthy = if num_states > 0 {
            self.bytes_since_clear >= BYTES_PER_STATE_THRESHOLD * num_states as u64
        } else {
            false
        };
        self.states_at_last_clear = num_states;

        // Progressive threshold: increment if healthy, don't exceed ceiling.
        if was_healthy && self.minimum_clear_count < MAX_MINIMUM_CLEAR_COUNT {
            self.minimum_clear_count += 1;
        }

        self.table.clear();
        self.nfa_state_sets.clear();
        self.state_map.clear();
        self.accel.clear();
        self.accel_analyzed.clear();
        self.state_prev_word.clear();
        self.has_pending_assertions.clear();
        self.assertion_match_flags.clear();
        self.memory_used = 0;
        self.clear_count += 1;
        self.bytes_since_clear = 0;
        self.start_states = [TaggedStateId::UNKNOWN; 4];
        self.epsilon_seen.resize(nfa.state_count(), false);
        self.epsilon_seen.fill(false);
        self.scratch_byte_progress.clear();
        self.scratch_real_targets.clear();
        self.scratch_closure_result.clear();
        self.scratch_closure_stack.clear();
        self.scratch_visited.resize(nfa.state_count(), false);
        self.scratch_visited.fill(false);
        self.scratch_stack.clear();

        // Re-create the table with correct stride.
        self.table = StateTable::new(self.classes.num_classes());
        self.init_states(nfa);
    }

    /// Save the current DFA state for survival across a cache reset.
    ///
    /// Extracts the NFA state set from the state table's flat storage.
    /// Returns `StateSaver::Empty` if the state is DEAD or UNKNOWN (nothing
    /// worth saving).
    pub(super) fn save_current_state(
        &self,
        sid: TaggedStateId,
        pos: usize,
        last_match: Option<usize>,
    ) -> StateSaver {
        if sid.is_dead() || sid.is_unknown() || sid.is_quit() {
            return StateSaver::Empty;
        }

        let ordinal = self.table.premul_to_ordinal(sid.index() as u32);
        if ordinal >= self.nfa_state_sets.len() {
            return StateSaver::Empty;
        }
        let nfa_set = self.nfa_state_sets.get(ordinal);
        let prev_word = if ordinal < self.state_prev_word.len() {
            self.state_prev_word[ordinal]
        } else {
            false
        };

        StateSaver::Saved {
            nfa_state_set: nfa_set.iter().copied().collect(),
            prev_word,
            pos,
            last_match,
        }
    }

    /// Restore a saved DFA state into a (freshly reset) cache.
    ///
    /// Recomputes the epsilon closure of the saved NFA state set and inserts
    /// the resulting DFA state into the cache. Returns the new `TaggedStateId`,
    /// the saved position, and the saved last-match.
    ///
    /// Returns `(UNKNOWN, 0, None)` if the saver is `Empty`.
    #[allow(clippy::cast_possible_truncation)]
    pub(super) fn restore_saved_state(
        &mut self,
        saver: &StateSaver,
        nfa: &Nfa,
    ) -> (TaggedStateId, usize, Option<usize>) {
        let (nfa_set, prev_word, pos, last_match) = match saver {
            StateSaver::Empty => return (TaggedStateId::UNKNOWN, 0, None),
            StateSaver::Saved {
                nfa_state_set,
                prev_word,
                pos,
                last_match,
            } => (nfa_state_set.as_slice(), *prev_word, *pos, *last_match),
        };

        // The saved set might contain byte-progress markers. Separate them
        // for the epsilon closure (which only handles real NFA states).
        let mut real_seeds: smallvec::SmallVec<[u32; 16]> = smallvec::SmallVec::new();
        let mut byte_progress: smallvec::SmallVec<[u32; 8]> = smallvec::SmallVec::new();
        for &s in nfa_set {
            if is_byte_progress(s) {
                byte_progress.push(s);
            } else {
                real_seeds.push(s);
            }
        }

        // Compute epsilon closure (handles prev_word context).
        // Use prev_newline=false since we're mid-search (not at text start).
        let mut closure = std::mem::take(&mut self.scratch_closure_result);
        closure.clear();
        self.epsilon_closure(nfa, &real_seeds, false, prev_word, &mut closure);

        // Merge byte-progress markers back in.
        closure.extend_from_slice(&byte_progress);
        closure.sort_unstable();
        closure.dedup();

        if closure.is_empty() {
            self.scratch_closure_result = closure;
            return (TaggedStateId::DEAD, pos, last_match);
        }

        let is_match = closure
            .iter()
            .any(|&s| !is_byte_progress(s) && s == self.accept_state);
        let hash_key = Self::hash_state_key(&closure, prev_word, self.has_look_ahead);

        let sid = if let Some(existing) = self.lookup_state(hash_key, &closure, prev_word) {
            existing
        } else {
            match self.alloc_state(&closure, is_match, prev_word, nfa, hash_key) {
                Some(sid) => sid,
                None => {
                    self.scratch_closure_result = closure;
                    return (TaggedStateId::UNKNOWN, pos, last_match);
                }
            }
        };

        self.scratch_closure_result = closure;
        (sid, pos, last_match)
    }

    /// Try to compute acceleration for a state. Only succeeds when all
    /// transitions for the state have been computed (no UNKNOWN entries).
    ///
    /// When acceleration is discovered, the ACCEL bit is propagated to all
    /// existing transition table entries that target this state. Future
    /// transitions targeting this state get the ACCEL bit set in `transition()`.
    fn try_analyze_accel(&mut self, sid: TaggedStateId) {
        let ordinal = self.table.premul_to_ordinal(sid.index() as u32);
        if ordinal >= self.accel_analyzed.len() {
            return;
        }
        if self.accel_analyzed[ordinal] {
            return;
        }

        // Check if all transitions are computed (no UNKNOWN entries).
        let num_classes = self.classes.num_classes();
        for c in 0..num_classes as u8 {
            if self.table.next(sid, c).is_unknown() {
                return; // Not all transitions computed yet.
            }
        }

        // All transitions known — run analysis.
        let premul = sid.index() as u32;
        let accel = AccelInfo::analyze(
            premul,
            num_classes,
            |c| self.table.next(sid, c),
            &self.classes,
        );

        if accel.is_some() {
            // Set ACCEL bit on all existing transition table entries
            // that target this state, so the search loop can detect
            // accelerable states with a single bit test.
            self.propagate_accel_bit(sid);
        }

        self.accel[ordinal] = accel;
        self.accel_analyzed[ordinal] = true;
    }

    /// Set the ACCEL bit on all transition table entries that point to
    /// the given state. Iterates all (source_state, class) pairs.
    fn propagate_accel_bit(&mut self, accel_sid: TaggedStateId) {
        let target_index = accel_sid.index();
        let num_states = self.table.state_count();
        let num_classes = self.classes.num_classes();
        for state_ord in 0..num_states {
            let premul = self.table.ordinal_to_premul(state_ord);
            let source = TaggedStateId::normal(premul);
            for c in 0..num_classes as u8 {
                let entry = self.table.next(source, c);
                if !entry.is_unknown() && !entry.is_dead() && entry.index() == target_index {
                    self.table.set(source, c, entry.with_accel());
                }
            }
        }
    }

    /// Check if the given state has been analyzed and found to be accelerable.
    fn is_state_accelerable(&self, sid: TaggedStateId) -> bool {
        if sid.is_dead() || sid.is_unknown() {
            return false;
        }
        let ordinal = self.table.premul_to_ordinal(sid.index() as u32);
        ordinal < self.accel.len() && self.accel[ordinal].is_some()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_progress_encode_decode_roundtrip() {
        for nfa_state in [0, 1, 100, NFA_STATE_MASK] {
            for trans_idx in 0..=15u8 {
                for byte_pos in 1..=3u8 {
                    let encoded = encode_byte_progress(nfa_state, trans_idx, byte_pos);
                    assert!(is_byte_progress(encoded));
                    let (dec_state, dec_tidx, dec_bpos) = decode_byte_progress(encoded);
                    assert_eq!(
                        dec_state, nfa_state,
                        "state mismatch for tidx={trans_idx}, bpos={byte_pos}"
                    );
                    assert_eq!(dec_tidx, trans_idx, "trans_idx mismatch");
                    assert_eq!(dec_bpos, byte_pos, "byte_pos mismatch");
                }
            }
        }
    }

    #[test]
    fn byte_progress_wildcard_encode_decode_roundtrip() {
        for expected_len in 2..=4u8 {
            for trans_idx in 0..=15u8 {
                let encoded = encode_wildcard_byte_progress(100, trans_idx, 1, expected_len);
                assert!(is_byte_progress(encoded));
                let dec_len = decode_expected_len(encoded);
                assert_eq!(dec_len, expected_len);
                let (_, dec_tidx, _) = decode_byte_progress(encoded);
                assert_eq!(dec_tidx, trans_idx);
            }
        }
    }

    #[test]
    fn byte_progress_nfa_state_max() {
        // NFA_STATE_MASK is now 23 bits (0x007F_FFFF = 8,388,607).
        let encoded = encode_byte_progress(NFA_STATE_MASK, 0, 1);
        let (dec_state, _, _) = decode_byte_progress(encoded);
        assert_eq!(dec_state, NFA_STATE_MASK);
    }

    #[test]
    fn wildcard_byte_progress_full_round_trip() {
        // Exhaustive test over all valid (nfa_state, trans_idx, byte_pos, expected_len)
        // combinations at boundary values.
        for nfa_state in [0, 42, NFA_STATE_MASK] {
            for trans_idx in [0u8, 7, 15] {
                for byte_pos in 1..=3u8 {
                    for expected_len in 2..=4u8 {
                        let encoded = encode_wildcard_byte_progress(
                            nfa_state,
                            trans_idx,
                            byte_pos,
                            expected_len,
                        );
                        assert!(is_byte_progress(encoded));
                        let (dec_state, dec_trans, dec_pos) = decode_byte_progress(encoded);
                        let dec_len = decode_expected_len(encoded);
                        assert_eq!(dec_state, nfa_state, "state mismatch");
                        assert_eq!(dec_trans, trans_idx, "trans_idx mismatch");
                        assert_eq!(dec_pos, byte_pos, "byte_pos mismatch");
                        assert_eq!(dec_len, expected_len, "expected_len mismatch");
                    }
                }
            }
        }
    }

    #[test]
    fn non_byte_progress_entries_not_detected() {
        // Values below BYTE_PROGRESS_BIT should not be flagged as byte-progress markers.
        for val in [0u32, 1, 100, 1_000_000, BYTE_PROGRESS_BIT - 1] {
            assert!(
                !is_byte_progress(val),
                "falsely detected {val} as byte progress"
            );
        }
    }

    #[test]
    fn byte_progress_bit_boundary() {
        // Exactly BYTE_PROGRESS_BIT is a byte-progress marker (with zero payload).
        assert!(is_byte_progress(BYTE_PROGRESS_BIT));
        // One below is not.
        assert!(!is_byte_progress(BYTE_PROGRESS_BIT - 1));
    }

    // ─── FlatStateSets lifecycle tests ──────────────────────────────────────

    #[test]
    fn flat_state_sets_push_get() {
        let mut fss = FlatStateSets::new();

        let set0 = vec![1u32, 2, 3];
        let set1 = vec![10u32, 20];
        let set2 = vec![100u32];
        let set3: Vec<u32> = vec![];

        let idx0 = fss.push(&set0);
        let idx1 = fss.push(&set1);
        let idx2 = fss.push(&set2);
        let idx3 = fss.push(&set3);

        assert_eq!(fss.get(idx0), &[1, 2, 3]);
        assert_eq!(fss.get(idx1), &[10, 20]);
        assert_eq!(fss.get(idx2), &[100]);
        assert_eq!(fss.get(idx3), &[] as &[u32]);
        assert_eq!(fss.len(), 4);
    }

    #[test]
    fn flat_state_sets_clear() {
        let mut fss = FlatStateSets::new();
        fss.push(&[1, 2, 3]);
        fss.push(&[4, 5]);
        assert_eq!(fss.len(), 2);

        fss.clear();
        assert_eq!(fss.len(), 0);

        // Can push again after clear.
        let idx = fss.push(&[10]);
        assert_eq!(fss.get(idx), &[10]);
        assert_eq!(fss.len(), 1);
    }

    #[test]
    fn flat_state_sets_memory_usage_grows() {
        let mut fss = FlatStateSets::new();
        let initial = fss.memory_usage();

        // Push several large sets.
        for _ in 0..50 {
            fss.push(&[1, 2, 3, 4, 5, 6, 7, 8]);
        }
        assert!(
            fss.memory_usage() > initial,
            "memory_usage should grow with content"
        );
    }

    #[test]
    fn flat_state_sets_empty_sets() {
        let mut fss = FlatStateSets::new();
        // Multiple empty sets should be independently addressable.
        let a = fss.push(&[]);
        let b = fss.push(&[]);
        let c = fss.push(&[42]);
        assert_eq!(fss.get(a), &[] as &[u32]);
        assert_eq!(fss.get(b), &[] as &[u32]);
        assert_eq!(fss.get(c), &[42]);
        assert_eq!(fss.len(), 3);
    }

    // ─── Thrashing / bail detection tests ─────────────────────────────────

    #[test]
    fn thrashing_detection_not_triggered_initially() {
        let re = crate::engine::VimRegex::new("a").expect("trivial pattern");
        let cache = DfaCache::new(&re.nfa, true, false);
        assert_eq!(cache.clear_count, 0);
        assert!(!cache.is_thrashing());
        assert!(!cache.should_bail());
    }

    #[test]
    fn thrashing_detection_after_resets() {
        let re = crate::engine::VimRegex::new("a").expect("trivial pattern");
        let mut cache = DfaCache::new(&re.nfa, true, false);

        // Simulate multiple resets with very few bytes searched.
        // Need at least INITIAL_MINIMUM_CLEAR_COUNT clears.
        for _ in 0..INITIAL_MINIMUM_CLEAR_COUNT {
            cache.record_bytes(10);
            cache.reset(&re.nfa);
        }
        // After enough clears with very few bytes, should_bail should trigger.
        // bytes_since_clear is 0 right after reset, and states_at_last_clear
        // will be small (just the start states). With 0 bytes, efficiency is bad.
        assert!(cache.should_bail());
        assert!(cache.is_thrashing());
    }

    #[test]
    fn thrashing_not_triggered_with_enough_bytes() {
        let re = crate::engine::VimRegex::new("a").expect("trivial pattern");
        let mut cache = DfaCache::new(&re.nfa, true, false);

        // Simulate resets with enough bytes between them.
        for _ in 0..INITIAL_MINIMUM_CLEAR_COUNT {
            cache.record_bytes(100_000);
            cache.reset(&re.nfa);
        }
        // Record enough bytes after last reset so efficiency metric is satisfied.
        cache.record_bytes(100_000);
        assert!(!cache.should_bail());
        assert!(!cache.is_thrashing());
    }

    #[test]
    fn thrashing_bytes_since_clear_resets_on_clear() {
        let re = crate::engine::VimRegex::new("a").expect("trivial pattern");
        let mut cache = DfaCache::new(&re.nfa, true, false);

        cache.record_bytes(1000);
        assert_eq!(cache.bytes_since_clear, 1000);
        cache.reset(&re.nfa);
        assert_eq!(cache.bytes_since_clear, 0);
    }

    // ─── StateSaver tests ──────────────────────────────────────────────────

    #[test]
    fn state_saver_empty_by_default() {
        let saver = StateSaver::Empty;
        assert!(matches!(saver, StateSaver::Empty));
    }

    #[test]
    fn state_saver_saved_roundtrip() {
        let saver = StateSaver::Saved {
            nfa_state_set: smallvec::smallvec![1, 2, 3],
            prev_word: false,
            pos: 42,
            last_match: Some(10),
        };
        match &saver {
            StateSaver::Saved {
                nfa_state_set,
                prev_word,
                pos,
                last_match,
            } => {
                assert_eq!(nfa_state_set.as_slice(), &[1, 2, 3]);
                assert!(!prev_word);
                assert_eq!(*pos, 42);
                assert_eq!(*last_match, Some(10));
            }
            StateSaver::Empty => panic!("expected Saved"),
        }
    }

    #[test]
    fn save_current_state_captures_nfa_set() {
        let re = crate::engine::VimRegex::new("[a-z]").expect("valid pattern");
        let cache = DfaCache::new(&re.nfa, true, false);

        // Get the start state, which has a known NFA set.
        let start = cache.start_state(true, false);
        assert!(!start.is_dead());

        let saver = cache.save_current_state(start, 5, Some(3));
        match &saver {
            StateSaver::Saved {
                nfa_state_set,
                pos,
                last_match,
                ..
            } => {
                assert!(
                    !nfa_state_set.is_empty(),
                    "start state should have NFA states"
                );
                assert_eq!(*pos, 5);
                assert_eq!(*last_match, Some(3));
            }
            StateSaver::Empty => panic!("expected Saved"),
        }
    }

    #[test]
    fn save_dead_state_returns_empty() {
        let re = crate::engine::VimRegex::new("a").expect("valid pattern");
        let cache = DfaCache::new(&re.nfa, true, false);

        let saver = cache.save_current_state(TaggedStateId::DEAD, 0, None);
        assert!(matches!(saver, StateSaver::Empty));
    }

    #[test]
    fn restore_saved_state_after_reset() {
        let re = crate::engine::VimRegex::new("[a-z]").expect("valid pattern");
        let mut cache = DfaCache::new(&re.nfa, true, false);

        let start = cache.start_state(true, false);
        let saver = cache.save_current_state(start, 10, Some(5));

        // Reset the cache (simulating budget exceeded).
        cache.reset(&re.nfa);

        // Restore the saved state.
        let (restored_sid, restored_pos, restored_match) =
            cache.restore_saved_state(&saver, &re.nfa);

        assert!(!restored_sid.is_dead(), "restored state should not be dead");
        assert!(
            !restored_sid.is_unknown(),
            "restored state should be computed"
        );
        assert_eq!(restored_pos, 10);
        assert_eq!(restored_match, Some(5));
    }

    #[test]
    fn restore_empty_saver_returns_unknown() {
        let re = crate::engine::VimRegex::new("a").expect("valid pattern");
        let mut cache = DfaCache::new(&re.nfa, true, false);

        let (sid, pos, last_match) = cache.restore_saved_state(&StateSaver::Empty, &re.nfa);

        assert!(sid.is_unknown());
        assert_eq!(pos, 0);
        assert_eq!(last_match, None);
    }

    // ─── Efficiency metric tests ───────────────────────────────────────────

    #[test]
    fn efficiency_metric_healthy_cache_does_not_bail() {
        let re = crate::engine::VimRegex::new("[a-z]").expect("valid pattern");
        let mut cache = DfaCache::new(&re.nfa, true, false);

        // Simulate: many bytes searched, few clears.
        cache.record_bytes(100_000);
        cache.reset(&re.nfa);
        cache.record_bytes(100_000);
        cache.reset(&re.nfa);
        cache.record_bytes(100_000);
        cache.reset(&re.nfa);

        // Even at 3 clears, if the efficiency is healthy we should not bail.
        cache.record_bytes(100_000);
        assert!(!cache.should_bail());
    }

    #[test]
    fn efficiency_metric_unhealthy_cache_bails() {
        let re = crate::engine::VimRegex::new("[a-z]").expect("valid pattern");
        let mut cache = DfaCache::new(&re.nfa, true, false);

        // Simulate: very few bytes per clear.
        for _ in 0..5 {
            cache.record_bytes(10);
            cache.reset(&re.nfa);
        }
        // After 5 clears with only 10 bytes each, efficiency is terrible.
        // should_bail should return true.
        assert!(cache.should_bail());
    }

    #[test]
    fn progressive_minimum_clear_count_increments() {
        let re = crate::engine::VimRegex::new("[a-z]").expect("valid pattern");
        let mut cache = DfaCache::new(&re.nfa, true, false);

        // Initial minimum_clear_count is 3.
        assert_eq!(cache.minimum_clear_count(), 3);

        // After healthy resets, minimum_clear_count increments.
        cache.record_bytes(100_000);
        cache.reset(&re.nfa);
        cache.record_bytes(100_000);
        cache.reset(&re.nfa);
        cache.record_bytes(100_000);
        cache.reset(&re.nfa);
        // 3 clears with healthy efficiency -> minimum advances.
        assert!(cache.minimum_clear_count() > 3);
    }
}
