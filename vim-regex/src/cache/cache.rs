use super::{SlotTable, SparseSet};
use crate::cache::checkpoints::SearchCheckpoints;
use crate::cache::memo::LookaroundMemo;
use crate::engine::pattern_stats::{PatternStats, Promotion, StrategyKind};
use crate::engine::strategy::PrefilterTracker;
use crate::engines::lazy_dfa::DfaCache;
use crate::engines::onepass::OnePassState;
use crate::engines::pike_vm::AcceptCollector;
use crate::nfa::{PendingLookbehind, StateId};

const BITS_PER_WORD: usize = usize::BITS as usize;
const DEFAULT_VISITED_CAPACITY: usize = 256 * 1024 * 8; // 256 KB in bits

/// Hard cap on VisitedSet backing store (bytes). Beyond this, the backtracker
/// reports HaystackTooLarge rather than allocate unboundedly. 32 MiB of bitset
/// covers haystacks of (32*1024*1024*8 / state_count) bytes — multi-MB files
/// for typical patterns.
pub(crate) const MAX_VISITED_BYTES: usize = 32 * 1024 * 1024;

/// Bitset for backtracker deduplication.
///
/// Tracks (StateId, position) pairs to prevent the backtracker from
/// revisiting the same NFA state at the same input position. Each pair
/// maps to a single bit: bit index = `pos * state_count + state`.
///
/// For backreference patterns, states reachable from backref targets
/// skip VisitedSet dedup entirely (handled by `Nfa::backref_reachable`),
/// so no bucket multiplier is needed. A single 256KB bitset suffices
/// for all patterns.
#[derive(Debug)]
pub(crate) struct VisitedSet {
    bits: Vec<usize>,
    state_count: u32,
    capacity_bits: usize,
    /// Test-only override for the memoization byte cap. When `Some`, it
    /// replaces `MAX_VISITED_BYTES` in `ensure_capacity`, letting a unit test
    /// drive the over-cap → `HaystackTooLarge` path without allocating a
    /// multi-hundred-MB haystack. `None` (production) uses the real cap.
    #[cfg(test)]
    max_bytes_override: Option<usize>,
}

impl VisitedSet {
    /// Allocate a `VisitedSet` for an NFA with `state_count` states.
    ///
    /// Uses `DEFAULT_VISITED_CAPACITY` bits (256 KB) of backing storage.
    /// A single 256KB bitset suffices for all patterns — backref-reachable
    /// states bypass dedup via `Nfa::backref_reachable`.
    pub(crate) fn new(state_count: u32) -> Self {
        let words = DEFAULT_VISITED_CAPACITY.div_ceil(BITS_PER_WORD);
        Self {
            bits: vec![0usize; words],
            state_count,
            capacity_bits: words * BITS_PER_WORD,
            #[cfg(test)]
            max_bytes_override: None,
        }
    }

    /// Test-only: lower the memoization byte cap so a search hits the over-cap
    /// path with a tiny haystack. Setting `0` makes any non-empty NFA exceed
    /// the cap on the first `ensure_capacity` call. Also shrinks the current
    /// backing store to `bytes` so the early "already fits" fast-path does not
    /// mask the lowered cap.
    #[cfg(test)]
    pub(crate) fn set_max_bytes_for_test(&mut self, bytes: usize) {
        self.max_bytes_override = Some(bytes);
        let words = bytes.saturating_mul(8).div_ceil(BITS_PER_WORD);
        self.bits.truncate(words);
        self.capacity_bits = words * BITS_PER_WORD;
    }

    /// Maximum haystack length that can be tracked without overflow.
    ///
    /// Returns 0 when `state_count` is 0 to avoid division by zero.
    pub(crate) fn max_haystack_len(&self) -> usize {
        if self.state_count == 0 {
            0
        } else {
            self.capacity_bits / self.state_count as usize
        }
    }

    /// Record the `(state, pos)` pair. Returns `true` if newly inserted,
    /// `false` if it was already present.
    ///
    /// If the computed index is out of bounds (haystack longer than
    /// `max_haystack_len`), returns `true` (treats it as unseen). Callers
    /// should check `max_haystack_len` before relying on deduplication.
    pub(crate) fn insert(&mut self, state: u32, pos: usize) -> bool {
        let index = pos * self.state_count as usize + state as usize;
        if index >= self.capacity_bits {
            return true;
        }
        let word = index / BITS_PER_WORD;
        let bit = index % BITS_PER_WORD;
        let mask = 1usize << bit;
        if self.bits[word] & mask != 0 {
            false
        } else {
            self.bits[word] |= mask;
            true
        }
    }

    /// Grow the bitset so `(state, pos)` indices up to `haystack_len` fit.
    /// Returns `false` if the required capacity exceeds `MAX_VISITED_BYTES`
    /// (caller must then report `HaystackTooLarge`); `true` if it now fits.
    pub(crate) fn ensure_capacity(&mut self, haystack_len: usize) -> bool {
        if self.state_count == 0 {
            return true;
        }
        // Need indices up to (haystack_len + 1) * state_count (pos ranges 0..=len).
        let needed_bits = haystack_len
            .saturating_add(1)
            .saturating_mul(self.state_count as usize);
        if needed_bits <= self.capacity_bits {
            return true;
        }
        #[cfg(test)]
        let cap_bytes = self.max_bytes_override.unwrap_or(MAX_VISITED_BYTES);
        #[cfg(not(test))]
        let cap_bytes = MAX_VISITED_BYTES;
        let max_bits = cap_bytes.saturating_mul(8);
        if needed_bits > max_bits {
            return false;
        }
        let words = needed_bits.div_ceil(BITS_PER_WORD);
        self.bits.resize(words, 0);
        self.capacity_bits = words * BITS_PER_WORD;
        true
    }

    /// Bytes of memoization a haystack of `haystack_len` would require for this
    /// NFA — used to populate the `HaystackTooLarge` error.
    pub(crate) fn required_bytes(&self, haystack_len: usize) -> usize {
        let needed_bits = haystack_len
            .saturating_add(1)
            .saturating_mul(self.state_count as usize);
        needed_bits.div_ceil(BITS_PER_WORD) * (BITS_PER_WORD / 8)
    }

    /// Reset all bits to 0.
    pub(crate) fn clear(&mut self) {
        self.bits.fill(0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SUB-CACHE STRUCTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Pre-allocated NFA simulation state: sparse sets, slot tables, lookbehinds.
///
/// Shared by both Pike VM and backtracker for active-state tracking
/// and capture-group bookkeeping.
#[derive(Debug)]
pub(crate) struct NfaSimCache {
    pub(crate) curr: SparseSet,
    pub(crate) next: SparseSet,
    pub(crate) curr_slots: SlotTable,
    pub(crate) next_slots: SlotTable,
    /// Per-state deferred lookbehind data (PIM). Empty for non-lookbehind patterns.
    pub(crate) curr_lookbehinds: Vec<Option<PendingLookbehind>>,
    /// Per-state deferred lookbehind data for the next step.
    pub(crate) next_lookbehinds: Vec<Option<PendingLookbehind>>,
}

impl NfaSimCache {
    fn new(state_count: usize, slot_count: usize) -> Self {
        Self {
            curr: SparseSet::new(state_count),
            next: SparseSet::new(state_count),
            curr_slots: SlotTable::new(state_count, slot_count),
            next_slots: SlotTable::new(state_count, slot_count),
            curr_lookbehinds: Vec::new(),
            next_lookbehinds: Vec::new(),
        }
    }

    fn new_no_captures(state_count: usize) -> Self {
        Self::new(state_count, 0)
    }
}

/// Reusable scratch buffers for Pike VM thread management.
///
/// These are separate from NfaSimCache because the Pike VM uses them
/// for its specific thread-list algorithm, while the backtracker
/// doesn't touch them.
#[derive(Debug)]
pub(crate) struct PikeVmScratch {
    /// Reusable Pike VM thread list (current step). Capacity preserved across calls.
    pub(crate) current: Vec<StateId>,
    /// Reusable Pike VM thread list (next step). Capacity preserved across calls.
    pub(crate) next: Vec<StateId>,
    /// Reusable Pike VM DFS work stack for epsilon closure.
    pub(crate) work: Vec<(StateId, usize)>,
    /// Reusable Pike VM accept-arrival collector (direct accept + PIM candidates).
    pub(crate) lb_candidates: AcceptCollector,
}

impl PikeVmScratch {
    fn new() -> Self {
        Self {
            current: Vec::new(),
            next: Vec::new(),
            work: Vec::new(),
            lb_candidates: AcceptCollector::default(),
        }
    }
}

/// Lazily-allocated backtracker scratch state.
///
/// Contains the VisitedSet (256KB+ allocation) and the DFS stack.
/// Wrapped in `Option` on Cache -- only allocated when the backtracker
/// is first invoked for a pattern.
#[derive(Debug)]
pub(crate) struct BacktrackerScratch {
    /// Deduplication bitset for (state, pos) pairs.
    pub(crate) visited: VisitedSet,
    /// Reusable DFS stack. Preserves capacity across searches.
    pub(crate) stack: Vec<crate::engines::backtracker::Frame>,
}

impl BacktrackerScratch {
    pub(crate) fn new(state_count: u32) -> Self {
        Self {
            visited: VisitedSet::new(state_count),
            stack: Vec::new(),
        }
    }
}

/// Pooled caches for lookaround and reverse matching.
///
/// Avoids fresh allocation per assertion. When a nested lookaround
/// needs a cache, it takes from the pool and returns it after use.
#[derive(Debug)]
pub(crate) struct LookaroundPool {
    /// Pooled sub-cache for lookaround checks.
    /// `None` when not yet populated or when taken by a nested lookaround.
    pub(crate) sub_cache: Option<Box<Cache>>,
    /// Pooled cache for reverse NFA matching.
    /// `None` until first reverse match attempt.
    pub(crate) reverse_cache: Option<Box<Cache>>,
    /// Pool of reusable capture buffers for LookbehindCandidate construction.
    /// Max 4 buffers (typical lookbehind fan-out is very small).
    pub(crate) lb_capture_pool: Vec<Vec<Option<usize>>>,
}

impl LookaroundPool {
    fn new() -> Self {
        Self {
            sub_cache: None,
            reverse_cache: None,
            lb_capture_pool: Vec::with_capacity(4),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CACHE (PUBLIC FACADE)
// ═══════════════════════════════════════════════════════════════════════════════

/// Pre-allocated scratch space shared across all regex engine passes.
///
/// Decomposes into 5 logical sub-caches:
/// - `nfa` -- NFA simulation state (SparseSet, SlotTable, lookbehinds)
/// - `pike` -- Pike VM thread management scratch
/// - `backtracker` -- Lazily-allocated VisitedSet + DFS stack
/// - `dfa` -- Lazily-allocated DFA state cache
/// - `lookaround` -- Pooled sub-caches for lookaround/reverse matching
///
/// `Cache` remains the public facade. Engine code accesses sub-caches
/// via `cache.nfa`, `cache.pike`, etc.
#[derive(Debug)]
pub struct Cache {
    pub(crate) nfa: NfaSimCache,
    pub(crate) pike: PikeVmScratch,
    /// Lazy: only allocated when backtracker is first invoked.
    pub(crate) backtracker: Option<BacktrackerScratch>,
    /// Lazy: only allocated when DFA-eligible pattern is first searched.
    pub(crate) dfa: Option<DfaCache>,
    /// Lazy: only allocated when reverse DFA search is first invoked.
    pub(crate) reverse_dfa: Option<DfaCache>,
    /// Lazy one-pass DFA engine state. `None` until first search with a
    /// one-pass eligible pattern.
    pub(crate) onepass: Option<OnePassState>,
    pub(crate) lookaround: LookaroundPool,
    /// Pre-allocated buffer for the best accept's capture data.
    /// Reused across accepts to avoid `.to_vec()` allocation on every accept.
    pub(crate) best_accept_captures: Vec<Option<usize>>,
    /// Tracks prefilter effectiveness; disables prefilter when hit rate drops below 1%.
    /// Reset at the start of each `search_internal` call.
    pub(crate) prefilter_tracker: PrefilterTracker,
    /// Lookaround result memo table. Cleared per top-level search call.
    pub(crate) lookaround_memo: LookaroundMemo,
    /// Per-pattern execution statistics for adaptive strategy promotion.
    /// Accumulated across searches; callers can check `recommend_promotion()`
    /// and recompile with a promoted strategy when thresholds are exceeded.
    pub(crate) pattern_stats: PatternStats,
    /// DFA state checkpoints at line boundaries for edit-incremental search.
    /// Populated by the lazy DFA forward search; callers can use
    /// `search_checkpoints.nearest_before(pos)` to resume from a checkpoint.
    pub(crate) search_checkpoints: SearchCheckpoints,
    /// NFA state count, stored for deferred allocations.
    state_count: u32,
    /// Whether the NFA uses backreferences.
    pub(crate) has_backreferences: bool,
}

impl Cache {
    /// Allocate a `Cache` for an NFA with `state_count` states and
    /// `slot_count` capture slots per state.
    ///
    /// The `VisitedSet` is **not** allocated here -- it is created lazily
    /// on the first call to `visited_mut()`, which only the backtracker
    /// triggers. This saves ~256KB per Pike VM search.
    pub(crate) fn new(state_count: u32, slot_count: usize, has_backreferences: bool) -> Self {
        let cap = state_count as usize;
        Self {
            nfa: NfaSimCache::new(cap, slot_count),
            pike: PikeVmScratch::new(),
            backtracker: None,
            dfa: None,
            reverse_dfa: None,
            onepass: None,
            lookaround: LookaroundPool::new(),
            best_accept_captures: Vec::new(),
            prefilter_tracker: PrefilterTracker::new(),
            lookaround_memo: LookaroundMemo::new(),
            pattern_stats: PatternStats::new(),
            search_checkpoints: SearchCheckpoints::new("", 0),
            state_count,
            has_backreferences,
        }
    }

    /// Allocate a `Cache` for an NFA with `state_count` states and zero
    /// capture slots per state.
    ///
    /// Use this when no capture-group information is needed (e.g. `is_match`),
    /// to avoid the per-state slot allocations required by the full `new`.
    /// The `VisitedSet` is deferred, same as `new()`.
    pub(crate) fn new_no_captures(state_count: u32) -> Self {
        let cap = state_count as usize;
        Self {
            nfa: NfaSimCache::new_no_captures(cap),
            pike: PikeVmScratch::new(),
            backtracker: None,
            dfa: None,
            reverse_dfa: None,
            onepass: None,
            lookaround: LookaroundPool::new(),
            best_accept_captures: Vec::new(),
            prefilter_tracker: PrefilterTracker::new(),
            lookaround_memo: LookaroundMemo::new(),
            pattern_stats: PatternStats::new(),
            search_checkpoints: SearchCheckpoints::new("", 0),
            state_count,
            has_backreferences: false,
        }
    }

    /// Lazily allocate and return a mutable reference to the `VisitedSet`.
    ///
    /// On first call this creates the bitset (256KB for all patterns);
    /// subsequent calls return the existing allocation.
    pub(crate) fn visited_mut(&mut self) -> &mut VisitedSet {
        let sc = self.state_count;
        &mut self
            .backtracker
            .get_or_insert_with(|| BacktrackerScratch::new(sc))
            .visited
    }

    /// Access the backtracker stack, lazily initializing if needed.
    pub(crate) fn backtracker_stack_mut(&mut self) -> &mut Vec<crate::engines::backtracker::Frame> {
        let sc = self.state_count;
        &mut self
            .backtracker
            .get_or_insert_with(|| BacktrackerScratch::new(sc))
            .stack
    }

    /// Returns the NFA state count this cache was allocated for.
    #[inline]
    pub fn state_count(&self) -> u32 {
        self.state_count
    }

    /// Check whether this cache is compatible with a regex that has
    /// `nfa_state_count` NFA states.
    ///
    /// A cache is compatible if its sparse sets and slot tables are large
    /// enough to hold the regex's state count.
    #[inline]
    pub fn is_compatible_with_state_count(&self, nfa_state_count: usize) -> bool {
        (self.state_count as usize) >= nfa_state_count
    }

    /// Resize internal data structures if the cache is too small for an NFA
    /// with `nfa_state_count` states and `slot_count` slots per state.
    ///
    /// This is a release-mode safety net -- in debug builds, the caller should
    /// have already caught the mismatch via `debug_assert!`.
    pub(crate) fn ensure_capacity(&mut self, nfa_state_count: u32, slot_count: usize) {
        if (self.state_count as usize) >= (nfa_state_count as usize) {
            return;
        }
        let cap = nfa_state_count as usize;
        self.nfa = NfaSimCache::new(cap, slot_count);
        self.state_count = nfa_state_count;
        // Invalidate lazy caches -- state count changed.
        self.backtracker = None;
        self.dfa = None;
        self.reverse_dfa = None;
        self.onepass = None;
        self.lookaround.sub_cache = None;
    }

    /// Take a sub-cache for a lookaround sub-NFA.
    ///
    /// Tries to reuse the pooled sub-cache. When the pool is empty
    /// (already taken by an outer nesting level) or undersized, falls
    /// through to a fresh allocation.
    ///
    /// Callers MUST return the sub-cache via `return_sub_cache` after use.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "NFA state count limited to u32::MAX"
    )]
    pub(crate) fn take_sub_cache(&mut self, nfa: &crate::nfa::Nfa) -> Cache {
        if let Some(mut sc) = self.lookaround.sub_cache.take() {
            if sc.nfa.curr.capacity() >= nfa.state_count() {
                sc.nfa.curr.clear();
                sc.nfa.next.clear();
                sc.nfa.curr_slots.reset_all();
                sc.nfa.next_slots.reset_all();
                return *sc;
            }
        }
        Cache::new(
            nfa.state_count() as u32,
            nfa.slot_count(),
            nfa.has_backreferences(),
        )
    }

    /// Return a sub-cache to the pool for reuse.
    pub(crate) fn return_sub_cache(&mut self, sub: Cache) {
        self.lookaround.sub_cache = Some(Box::new(sub));
    }

    /// Take a capture buffer from the pool, or create a new one.
    pub(crate) fn take_lb_captures(&mut self, len: usize) -> Vec<Option<usize>> {
        if let Some(mut buf) = self.lookaround.lb_capture_pool.pop() {
            buf.clear();
            buf.resize(len, None);
            buf
        } else {
            vec![None; len]
        }
    }

    /// Return a capture buffer to the pool (max 4 retained).
    pub(crate) fn return_lb_captures(&mut self, buf: Vec<Option<usize>>) {
        if self.lookaround.lb_capture_pool.len() < 4 {
            self.lookaround.lb_capture_pool.push(buf);
        }
    }

    /// Returns the approximate heap memory used by this cache, in bytes.
    ///
    /// This includes sparse sets, slot tables, Pike VM scratch buffers,
    /// the backtracker VisitedSet (if allocated), the DFA cache (if present),
    /// and pooled sub-caches.
    ///
    /// Does NOT include stack size (`std::mem::size_of::<Cache>()`).
    #[must_use]
    pub fn memory_usage(&self) -> usize {
        use std::mem::size_of;

        let mut total = 0;
        let cap = self.state_count as usize;

        // NFA SparseSets: each has dense Vec<u32> + sparse Vec<u32>.
        total += cap * size_of::<u32>() * 2; // curr (dense + sparse)
        total += cap * size_of::<u32>() * 2; // next (dense + sparse)

        // SlotTables (arena + indirection + refcounts).
        // Arena: row_capacity * slots_per_state * size_of::<Option<usize>>
        // We approximate by counting the arena allocation of each table.
        let sps = self.nfa.curr_slots.slots_per_state();
        let row_cap = cap + 1; // row_capacity = state_capacity + 1
        total += row_cap * sps * size_of::<Option<usize>>() * 2; // curr + next arenas
        total += cap * size_of::<u32>() * 2; // indirection tables (RowId per state)
        total += row_cap * size_of::<u32>() * 2; // refcount arrays

        // Pike VM scratch buffers (capacity * element size).
        total += self.pike.current.capacity() * size_of::<StateId>();
        total += self.pike.next.capacity() * size_of::<StateId>();
        total += self.pike.work.capacity() * size_of::<(StateId, usize)>();
        total += self.pike.lb_candidates.candidates_capacity()
            * size_of::<crate::engines::pike_vm::LookbehindCandidate>();

        // Backtracker scratch (if allocated).
        if let Some(ref bt) = self.backtracker {
            // VisitedSet bits Vec.
            total += bt.visited.bits.capacity() * size_of::<usize>();
            // DFS stack.
            total += bt.stack.capacity() * size_of::<crate::engines::backtracker::Frame>();
        }

        // DFA cache (state table + auxiliary structures).
        if let Some(ref dfa) = self.dfa {
            total += dfa.memory_usage();
        }

        // Reverse DFA cache (state table + auxiliary structures).
        if let Some(ref rev_dfa) = self.reverse_dfa {
            total += rev_dfa.memory_usage();
        }

        // Lookbehind tables.
        total += self.nfa.curr_lookbehinds.capacity() * size_of::<Option<PendingLookbehind>>();
        total += self.nfa.next_lookbehinds.capacity() * size_of::<Option<PendingLookbehind>>();

        // Best accept captures buffer.
        total += self.best_accept_captures.capacity() * size_of::<Option<usize>>();

        // Lookaround pool: sub-cache + reverse_cache (recursive).
        if let Some(ref sub) = self.lookaround.sub_cache {
            total += size_of::<Cache>() + sub.memory_usage();
        }
        if let Some(ref rev) = self.lookaround.reverse_cache {
            total += size_of::<Cache>() + rev.memory_usage();
        }

        // Lookbehind capture pool.
        for buf in &self.lookaround.lb_capture_pool {
            total += buf.capacity() * size_of::<Option<usize>>();
        }

        // Lookaround memo table.
        total += self.lookaround_memo.memory_usage();

        total
    }

    /// Get or build the one-pass DFA state for the given NFA.
    /// Returns a mutable reference to the one-pass state.
    pub(crate) fn onepass_state(&mut self, nfa: &crate::nfa::Nfa) -> &mut OnePassState {
        if self.onepass.is_none() {
            self.onepass = Some(OnePassState::NotBuilt);
        }
        let state = self.onepass.as_mut().unwrap();
        state.ensure_built(nfa);
        state
    }

    /// Check whether the pattern should be promoted to a faster strategy
    /// based on accumulated execution statistics.
    ///
    /// The caller provides pattern-level eligibility flags from
    /// `PatternProperties`. Returns `Promotion::None` if no promotion
    /// is recommended.
    pub fn recommend_promotion(
        &self,
        is_dfa_eligible: bool,
        is_onepass_eligible: bool,
        is_literal: bool,
    ) -> Promotion {
        self.pattern_stats
            .recommend_promotion(is_dfa_eligible, is_onepass_eligible, is_literal)
    }

    /// Record a search execution in the pattern statistics.
    pub(crate) fn record_search(
        &mut self,
        bytes_searched: usize,
        time_ns: u64,
        strategy: StrategyKind,
    ) {
        self.pattern_stats.record(bytes_searched, time_ns, strategy);
    }

    /// Swap `curr`/`next` SparseSets and their associated `SlotTable`s,
    /// then eagerly reset the new `next_slots`.
    ///
    /// After the swap the old `curr_slots` (now `next_slots`) has its
    /// indirection table and refcounts bulk-cleared via `reset_all()`,
    /// returning all physical rows to the free list. This eliminates
    /// phantom refcounts from `prune_after_accept` truncation or
    /// dedup-rejected epsilon closures.
    pub(crate) fn swap(&mut self) {
        std::mem::swap(&mut self.nfa.curr, &mut self.nfa.next);
        std::mem::swap(&mut self.nfa.curr_slots, &mut self.nfa.next_slots);
        self.nfa.next_slots.reset_all();
        std::mem::swap(
            &mut self.nfa.curr_lookbehinds,
            &mut self.nfa.next_lookbehinds,
        );
        self.nfa.next_lookbehinds.fill(None);
    }
}

#[cfg(test)]
mod tests {
    use super::{Cache, VisitedSet, MAX_VISITED_BYTES};

    #[test]
    fn cache_swap() {
        let mut cache = Cache::new(4, 2, false);
        cache.nfa.curr.insert(1);
        cache.nfa.next.insert(2);
        cache.nfa.curr_slots.get_mut(0)[0] = Some(10);
        cache.nfa.next_slots.get_mut(0)[0] = Some(20);
        cache.swap();
        // SparseSets are swapped (not reset).
        assert!(cache.nfa.curr.contains(2), "curr should now contain 2");
        assert!(
            !cache.nfa.curr.contains(1),
            "curr should no longer contain 1"
        );
        assert!(cache.nfa.next.contains(1), "next should now contain 1");
        assert!(
            !cache.nfa.next.contains(2),
            "next should no longer contain 2"
        );
        // curr_slots got the old next_slots data (preserved).
        assert_eq!(cache.nfa.curr_slots.get(0)[0], Some(20));
        // next_slots was eagerly reset -- the old curr_slots data is wiped.
        assert_eq!(cache.nfa.next_slots.get(0)[0], None);
    }

    #[test]
    fn visited_insert_and_dedup() {
        let mut v = VisitedSet::new(8);
        assert!(v.insert(3, 0), "(3,0) should be new");
        assert!(!v.insert(3, 0), "(3,0) should be a duplicate");
        assert!(v.insert(3, 1), "(3,1) should be new");
        assert!(v.insert(4, 0), "(4,0) should be new");
    }

    #[test]
    fn visited_clear() {
        let mut v = VisitedSet::new(8);
        v.insert(3, 0);
        assert!(!v.insert(3, 0), "should be duplicate before clear");
        v.clear();
        assert!(v.insert(3, 0), "should be new after clear");
    }

    #[test]
    fn visited_max_haystack_len() {
        let v = VisitedSet::new(100);
        assert!(
            v.max_haystack_len() > 20_000,
            "expected > 20000, got {}",
            v.max_haystack_len()
        );
    }

    #[test]
    fn visited_zero_states() {
        let v = VisitedSet::new(0);
        assert_eq!(v.max_haystack_len(), 0);
    }

    #[test]
    fn visited_grows_to_fit_then_caps() {
        let mut v = VisitedSet::new(100);
        assert!(v.ensure_capacity(1_000_000)); // grows
        assert!(v.insert(50, 999_999)); // in-bounds now
                                        // Over the 32 MiB cap (needs > 32*1024*1024*8 bits):
        let huge = (MAX_VISITED_BYTES * 8usize / 100) + 1;
        assert!(!v.ensure_capacity(huge));
    }

    #[test]
    fn is_compatible_with_state_count_matching() {
        let cache = Cache::new(8, 4, false);
        assert!(cache.is_compatible_with_state_count(8));
        assert!(cache.is_compatible_with_state_count(4));
        assert!(cache.is_compatible_with_state_count(0));
    }

    #[test]
    fn is_compatible_with_state_count_too_small() {
        let cache = Cache::new(4, 2, false);
        assert!(!cache.is_compatible_with_state_count(5));
        assert!(!cache.is_compatible_with_state_count(100));
    }

    #[test]
    fn ensure_capacity_grows() {
        let mut cache = Cache::new(4, 2, false);
        assert_eq!(cache.state_count(), 4);
        assert!(!cache.is_compatible_with_state_count(8));

        cache.ensure_capacity(8, 4);
        assert_eq!(cache.state_count(), 8);
        assert!(cache.is_compatible_with_state_count(8));
    }

    #[test]
    fn ensure_capacity_noop_when_already_large_enough() {
        let mut cache = Cache::new(8, 4, false);
        // Should be a no-op: cache is already compatible.
        cache.ensure_capacity(4, 2);
        assert_eq!(cache.state_count(), 8);
    }

    #[test]
    fn ensure_capacity_invalidates_backtracker_and_sub_cache() {
        let mut cache = Cache::new(4, 2, false);
        // Populate visited set (triggers backtracker allocation).
        let _ = cache.visited_mut();
        assert!(cache.backtracker.is_some());
        // Populate sub_cache.
        cache.lookaround.sub_cache = Some(Box::new(Cache::new(2, 1, false)));
        assert!(cache.lookaround.sub_cache.is_some());

        cache.ensure_capacity(8, 4);
        assert!(
            cache.backtracker.is_none(),
            "backtracker should be invalidated after resize"
        );
        assert!(
            cache.lookaround.sub_cache.is_none(),
            "sub_cache should be invalidated after resize"
        );
    }

    #[test]
    fn cache_sub_cache_field_access() {
        let cache = Cache::new(10, 4, false);
        assert!(cache.backtracker.is_none()); // lazy
        assert!(cache.dfa.is_none()); // lazy
        assert!(cache.lookaround.sub_cache.is_none());
    }

    #[test]
    fn reverse_dfa_cache_field_initially_none() {
        let cache = Cache::new(8, 4, false);
        assert!(cache.reverse_dfa.is_none());
    }

    #[test]
    fn reverse_dfa_cache_field_in_memory_usage() {
        let cache = Cache::new(8, 4, false);
        let base = cache.memory_usage();
        // Just verify memory_usage doesn't panic with reverse_dfa = None.
        assert!(base > 0);
    }
}
