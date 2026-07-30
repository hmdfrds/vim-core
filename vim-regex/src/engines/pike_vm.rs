//! Upgraded Pike VM — NFA simulation using `SparseSet`, `SlotTable`, and `Cache`.
//!
//! A faithful rewrite of the original `pike_vm.rs` using pre-allocated data
//! structures. The algorithm is identical — only the data structures change:
//!
//! - `SparseSet` replaces `Vec<bool>` visited arrays (O(1) clear)
//! - `SlotTable` replaces per-thread `SmallVec<[Option<usize>; 20]>` captures
//!   (indexed by `StateId`, no per-thread heap allocation)
//! - `Cache` wraps both and is passed by `&mut` — no per-call allocation
//! - Optional `Prefilter` for skipping non-candidate positions during forward scan
//!
//! ## Slot layout
//!
//! Each state's row in the `SlotTable` has `num_capture_slots + 2` entries:
//! - `0..num_cap` — capture group boundaries (even=open, odd=close)
//! - `num_cap` — `\zs` match_start override
//! - `num_cap + 1` — `\ze` match_end override
//!
//! ## Thread list
//!
//! Active threads are tracked as `Vec<StateId>` (not in the SparseSet).
//! The SparseSet is used purely for deduplication during epsilon closure.
//! This preserves insertion-order priority needed by `prune_after_accept`.

use crate::accel::Prefilter;
use crate::cache::Cache;
use crate::common::{
    advance_one_codepoint, align_to_char_boundary, build_vim_match_from_captures,
    retreat_one_codepoint, BACKWARD_SCAN_WINDOW, MAX_CAPTURE_GROUPS,
};
use crate::ir::LookaroundKind;
use crate::matchers::{skip_combining_marks, CharMatcher, MatchContext, Matcher, ZeroWidthMatcher};
use crate::nfa::{Nfa, PendingLookbehind, StateId, TransitionKind};
use crate::VimMatch;
use smallvec::SmallVec;
use std::ops::Range;

/// Captures exported by a successful positive lookaround, indexed by global
/// capture group (index `i` = group `i+1`), as absolute haystack byte ranges
/// (lookbehind ranges already offset by the window start). `None` per slot means
/// that group did not participate inside the lookaround sub-match. Negative
/// lookarounds export an empty set on success (see `check_lookaround`).
pub(crate) type LookaroundCaptures = SmallVec<[Option<Range<usize>>; MAX_CAPTURE_GROUPS]>;

// ═══════════════════════════════════════════════════════════════════════════════
// SLOT LAYOUT HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

#[inline]
const fn ms_slot(num_cap: usize) -> usize {
    num_cap
}
#[inline]
const fn me_slot(num_cap: usize) -> usize {
    num_cap + 1
}
#[inline]
const fn total_slots(num_cap: usize) -> usize {
    num_cap + 2
}

// ═══════════════════════════════════════════════════════════════════════════════
// TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// Optional slot update: `(slot_index, new_value)`.
type SlotUpdate = Option<(usize, Option<usize>)>;

/// Result of a non-consuming epsilon step: target state + optional slot update.
type EpsilonResult = Option<(StateId, SlotUpdate)>;

/// Saved slot data from the moment an accept state is reached.
///
/// Captures are stored in `cache.best_accept_captures` (pre-allocated buffer)
/// rather than inline here, to eliminate per-accept `.to_vec()` allocation.
struct SavedAccept {
    match_start: Option<usize>,
    match_end: Option<usize>,
    accepted_at: usize,
}

/// A thread that reached accept with a pending (unverified) lookbehind.
#[derive(Debug)]
pub(crate) struct LookbehindCandidate {
    pending: PendingLookbehind,
    captures: Vec<Option<usize>>,
    match_start: Option<usize>,
    match_end: Option<usize>,
    accepted_at: usize,
    /// Registration order within this position's accept arrivals (lower = higher
    /// thread priority). Used by `prune_after_accept` to resolve this PIM candidate
    /// against a direct accept and other PIMs in descending priority.
    priority: u32,
    /// Number of threads already pushed to the active thread list (`dest`) at the
    /// moment this accept was recorded. This is the winning thread's priority rank:
    /// threads with index `< thread_rank` are strictly higher priority (they were
    /// pushed first, e.g. the winning construct's greedy loop-back) and survive;
    /// threads at index `>= thread_rank` are lower-priority alternatives. On a win,
    /// `prune_after_accept` truncates the thread list to `thread_rank` — the
    /// standard PikeVM "kill lower-priority threads on accept" rule — preventing a
    /// lower-priority branch from clobbering this win at a later position while
    /// still letting the higher-priority greedy loop-back extend the match.
    thread_rank: usize,
}

/// A thread that reached accept directly (no pending lookbehind). Its assertion
/// is trivially true, so it is the priority-ordered fallback when higher-priority
/// PIM candidates fail their deferred lookbehind.
#[derive(Debug)]
struct DirectAccept {
    captures: Vec<Option<usize>>,
    match_start: Option<usize>,
    match_end: Option<usize>,
    accepted_at: usize,
    /// Registration order within this position's accept arrivals (lower = higher
    /// thread priority). Only the highest-priority direct accept is recorded.
    priority: u32,
    /// Thread priority rank at record time (see `LookbehindCandidate::thread_rank`).
    /// For a direct accept this equals the accept state's position in `dest` (the
    /// `dest.len()` before its push), so `truncate(thread_rank)` reproduces the
    /// original `threads.position(accept)` truncation.
    thread_rank: usize,
}

/// Collects every accept-state arrival at a single input position so they can be
/// resolved in descending thread priority by `prune_after_accept`.
///
/// PikeVM processes threads in priority order (LIFO `work` stack = leftmost-greedy
/// registration order), so `seq` — incremented on each accept arrival — encodes
/// priority: a lower `priority` means a higher-priority thread. The accept-state
/// SparseSet dedup keys only on `state_id`, which would otherwise drop a
/// lower-priority *plain* accept arriving after a higher-priority PIM-carrying
/// accept. This collector records the first (highest-priority) direct accept and
/// every PIM candidate regardless of that dedup, so the resolution in
/// `prune_after_accept` sees the full priority-ordered set.
#[derive(Debug, Default)]
pub(crate) struct AcceptCollector {
    /// PIM (deferred-lookbehind) candidates, each tagged with its priority.
    candidates: Vec<LookbehindCandidate>,
    /// The highest-priority direct accept seen at this position, if any.
    direct: Option<DirectAccept>,
    /// Monotonic accept-arrival counter for the current position.
    seq: u32,
}

impl AcceptCollector {
    /// Clear all per-position state (called before each position's closures and
    /// after `prune_after_accept` consumes the arrivals).
    fn reset(&mut self) {
        self.candidates.clear();
        self.direct = None;
        self.seq = 0;
    }

    /// Capacity of the backing PIM-candidate buffer (for `Cache::memory_usage`).
    pub(crate) fn candidates_capacity(&self) -> usize {
        self.candidates.capacity()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PUBLIC API
// ═══════════════════════════════════════════════════════════════════════════════

/// Scan forward from `start`, return the first (leftmost-longest) match.
///
/// When `anchored`, only tries at `start`. Otherwise scans forward, using
/// `prefilter` (if provided) to skip non-candidate positions.
///
/// `start_bitmap` is a 256-bit bitmap of bytes that can start a match.
/// When provided, positions whose first byte is not in the bitmap are
/// skipped without running the NFA.
pub(crate) fn search(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
    anchored: bool,
    prefilter: Option<&dyn Prefilter>,
    start_bitmap: Option<&[u32; 8]>,
) -> Option<VimMatch> {
    if anchored {
        return try_match_at(nfa, cache, ctx, start);
    }

    let mut pos = start;
    while pos <= ctx.text.len() {
        if let Some(pf) = prefilter {
            match pf.find_next(ctx.text, pos) {
                Some(p) => pos = p,
                None => return None,
            }
        }

        // Start bitmap check: skip positions where no match can start.
        if let Some(bitmap) = start_bitmap {
            if pos < ctx.text.len() {
                let byte = ctx.text.as_bytes()[pos];
                if bitmap[byte as usize >> 5] & (1 << (byte & 31)) == 0 {
                    pos = advance_one_codepoint(ctx.text, pos);
                    continue;
                }
            }
        }

        if let Some(m) = try_match_at(nfa, cache, ctx, pos) {
            return Some(m);
        }
        pos = advance_one_codepoint(ctx.text, pos);
    }
    None
}

/// Scan backward: find the last match starting before `ctx.cursor`.
///
/// When a prefilter is available, uses `find_prev` as a hint to narrow the
/// scan window closer to the cursor, skipping positions that cannot start
/// a match.
pub(crate) fn search_backward(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    prefilter: Option<&dyn Prefilter>,
) -> Option<VimMatch> {
    let cursor = ctx.cursor.unwrap_or(ctx.text.len());
    let scan_start = align_to_char_boundary(ctx.text, cursor.saturating_sub(BACKWARD_SCAN_WINDOW));
    let mut last_match: Option<VimMatch> = None;
    let mut pos = scan_start;

    // Use prefilter to narrow the scan window (only when SIMD-accelerated).
    if let Some(pf) = prefilter.filter(|p| p.is_fast()) {
        if let Some(hint) = pf.find_prev(ctx.text, cursor) {
            if hint > scan_start {
                // Start scanning from a bit before the hint to give headroom
                // for patterns where the prefilter match is after the start.
                let narrowed = align_to_char_boundary(ctx.text, hint.saturating_sub(64));
                if narrowed > pos {
                    pos = narrowed;
                }
            }
        }
    }

    while pos < cursor {
        if let Some(m) = try_match_at(nfa, cache, ctx, pos) {
            if m.range.start < cursor {
                let end = m.range.end;
                last_match = Some(m);
                let next = if end == pos {
                    advance_one_codepoint(ctx.text, pos)
                } else {
                    end
                };
                if next <= pos {
                    break;
                }
                pos = next;
                continue;
            }
        }
        pos = advance_one_codepoint(ctx.text, pos);
    }
    last_match
}

/// Find all non-overlapping matches.
///
/// When a prefilter is available, uses `find_next` to skip to candidate
/// positions, falling back to character-by-character scan when the
/// prefilter produces a false positive.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "used by engine-level tests in tests/engines/pike_vm.rs"
    )
)]
pub(crate) fn search_all(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    prefilter: Option<&dyn Prefilter>,
) -> Vec<VimMatch> {
    let mut results = Vec::new();
    let mut pos = 0;

    while pos <= ctx.text.len() {
        if let Some(pf) = prefilter {
            match pf.find_next(ctx.text, pos) {
                Some(p) => pos = p,
                None => break,
            }
        }
        if let Some(m) = try_match_at(nfa, cache, ctx, pos) {
            let end = m.range.end;
            results.push(m);
            pos = if end == pos {
                advance_one_codepoint(ctx.text, pos)
            } else {
                end
            };
        } else {
            pos = advance_one_codepoint(ctx.text, pos);
        }
    }
    results
}

/// Try to match starting exactly at `pos` (anchored).
pub(crate) fn match_anchored(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    pos: usize,
) -> Option<VimMatch> {
    try_match_at(nfa, cache, ctx, pos)
}

/// Reverse-anchored match: scan backward from `end_pos` toward `lower_bound`.
///
/// Runs the reverse NFA on the original text, consuming characters
/// right-to-left via `retreat_one_codepoint`. Returns the leftmost byte
/// position where the reverse NFA accepted (= the forward match start).
///
/// Uses a pooled `Cache` from `parent_cache.reverse_cache` to avoid per-call
/// allocation. No captures are tracked (the reverse NFA has no Save
/// transitions). The caller uses the returned start position with
/// `match_anchored` on the forward NFA to get captures.
///
/// `lower_bound` must be a valid char boundary in `ctx.text`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "NFA state count limited to u32::MAX"
)]
pub(crate) fn try_match_at_reverse(
    nfa: &Nfa,
    parent_cache: &mut Cache,
    ctx: &MatchContext<'_>,
    end_pos: usize,
    lower_bound: usize,
) -> Option<usize> {
    let lower_bound = align_to_char_boundary(ctx.text, lower_bound);
    let num_cap = nfa.slot_count();
    let total = total_slots(num_cap);

    // Get or create the reverse cache from the parent's pool.
    let mut rev_cache = parent_cache
        .lookaround
        .reverse_cache
        .take()
        .unwrap_or_else(|| Box::new(Cache::new_no_captures(nfa.state_count() as u32)));
    if rev_cache.nfa.curr.capacity() < nfa.state_count() {
        *rev_cache = Cache::new_no_captures(nfa.state_count() as u32);
    }
    let cache = &mut *rev_cache;
    ensure_cache(cache, nfa.state_count(), total);

    // Take reusable vectors from reverse cache (preserves capacity across calls).
    let mut current = std::mem::take(&mut cache.pike.current);
    let mut next = std::mem::take(&mut cache.pike.next);
    let mut work = std::mem::take(&mut cache.pike.work);
    let mut lb_candidates = std::mem::take(&mut cache.pike.lb_candidates);
    current.clear();
    next.clear();
    work.clear();
    lb_candidates.reset();

    cache.nfa.curr.clear();
    cache.nfa.curr_slots.clear_row(nfa.start().index());
    epsilon_closure(
        nfa,
        cache,
        ctx,
        nfa.start(),
        end_pos,
        nfa.start().index(),
        &mut current,
        num_cap,
        &mut work,
        SlotTarget::Curr,
        &mut lb_candidates,
    );

    let mut best: Option<SavedAccept> = None;
    prune_after_accept(nfa, cache, &mut current, &mut best, &mut lb_candidates, ctx);

    let mut pos = end_pos;
    while pos > lower_bound && !current.is_empty() {
        let prev_pos = retreat_one_codepoint(ctx.text, pos);
        if ctx
            .text
            .get(prev_pos..pos)
            .and_then(|s| s.chars().next())
            .is_none()
        {
            break;
        }

        next.clear();
        cache.nfa.next.clear();

        step(
            nfa,
            cache,
            ctx,
            &current,
            prev_pos,
            prev_pos,
            &mut next,
            num_cap,
            &mut work,
            &mut lb_candidates,
        );

        std::mem::swap(&mut current, &mut next);
        cache.swap();
        cache.nfa.next.clear();

        prune_after_accept(nfa, cache, &mut current, &mut best, &mut lb_candidates, ctx);

        pos = prev_pos;
    }

    let result = best.map(|sa| sa.accepted_at);
    // Return pooled vectors to reverse cache for reuse.
    cache.pike.current = current;
    cache.pike.next = next;
    cache.pike.work = work;
    cache.pike.lb_candidates = lb_candidates;
    // Return the reverse cache to the parent's pool.
    parent_cache.lookaround.reverse_cache = Some(rev_cache);
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// CORE SIMULATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Anchored match at `start_pos`. This is the heart of the Pike VM.
fn try_match_at(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start_pos: usize,
) -> Option<VimMatch> {
    let num_cap = nfa.slot_count();
    let total = total_slots(num_cap);

    ensure_cache(cache, nfa.state_count(), total);

    // Take reusable vectors from cache (preserves capacity across calls).
    let mut current = std::mem::take(&mut cache.pike.current);
    let mut next = std::mem::take(&mut cache.pike.next);
    let mut work = std::mem::take(&mut cache.pike.work);
    let mut lookbehind_candidates = std::mem::take(&mut cache.pike.lb_candidates);
    current.clear();
    next.clear();
    work.clear();
    lookbehind_candidates.reset();

    // Clear dedup set and seed start state.
    cache.nfa.curr.clear();
    cache.nfa.curr_lookbehinds.clear();
    cache.nfa.curr_slots.clear_row(nfa.start().index());
    epsilon_closure(
        nfa,
        cache,
        ctx,
        nfa.start(),
        start_pos,
        nfa.start().index(),
        &mut current,
        num_cap,
        &mut work,
        SlotTarget::Curr,
        &mut lookbehind_candidates,
    );

    let mut best: Option<SavedAccept> = None;
    prune_after_accept(
        nfa,
        cache,
        &mut current,
        &mut best,
        &mut lookbehind_candidates,
        ctx,
    );

    let mut pos = start_pos;
    while pos < ctx.text.len() && !current.is_empty() {
        let Some(ch) = ctx.text.get(pos..).and_then(|s| s.chars().next()) else {
            break;
        };
        let byte_len = ch.len_utf8();
        // When \Z is active, skip combining marks after the base character.
        // The `next_text_pos` accounts for both the base char and any trailing marks.
        let next_pos = if ctx.ignore_composing {
            skip_combining_marks(ctx.text, pos + byte_len)
        } else {
            pos + byte_len
        };
        next.clear();
        cache.nfa.next.clear();
        cache.nfa.next_lookbehinds.clear();

        step(
            nfa,
            cache,
            ctx,
            &current,
            pos,
            next_pos,
            &mut next,
            num_cap,
            &mut work,
            &mut lookbehind_candidates,
        );

        std::mem::swap(&mut current, &mut next);
        cache.swap();
        cache.nfa.next.clear();
        cache.nfa.next_lookbehinds.clear();

        prune_after_accept(
            nfa,
            cache,
            &mut current,
            &mut best,
            &mut lookbehind_candidates,
            ctx,
        );

        pos = next_pos;
    }

    // Return reusable vectors to cache (preserves capacity for next call).
    cache.pike.current = current;
    cache.pike.next = next;
    cache.pike.work = work;
    cache.pike.lb_candidates = lookbehind_candidates;

    best.map(|sa| {
        build_vim_match_from_captures(
            &cache.best_accept_captures,
            sa.match_start,
            sa.match_end,
            start_pos,
            sa.accepted_at,
        )
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// SLOT TARGET SELECTOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Which SparseSet + SlotTable pair to use.
#[derive(Clone, Copy)]
enum SlotTarget {
    Curr,
    Next,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PIM PROPAGATION HELPER
// ═══════════════════════════════════════════════════════════════════════════════

/// Set (or clear) a pending lookbehind entry, growing the vector if needed.
#[inline]
fn set_lookbehind(
    lookbehinds: &mut Vec<Option<PendingLookbehind>>,
    idx: usize,
    val: Option<PendingLookbehind>,
) {
    if lookbehinds.len() <= idx {
        lookbehinds.resize(idx + 1, None);
    }
    lookbehinds[idx] = val;
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCEPT RECORDING
// ═══════════════════════════════════════════════════════════════════════════════

/// Record an accept-state arrival into the priority-ordered collector.
///
/// `state_idx` is the accept state's row in the active SlotTable selected by
/// `target`. This runs **regardless of the SparseSet dedup**, so that a
/// lower-priority *plain* accept arriving after a higher-priority PIM-carrying
/// accept (which already inserted `accept` into the dedup set) is still recorded
/// and can serve as the fallback when the PIM fails its deferred lookbehind.
///
/// - If the thread carries a `PendingLookbehind` (PIM), a `LookbehindCandidate`
///   is pushed with the next priority — every distinct PIM thread is kept (their
///   count is bounded by the number of deferred-lookbehind branches).
/// - Otherwise it is a direct accept. Only the **highest-priority** direct accept
///   is kept (later direct accepts are equivalent, lower-priority duplicates) and
///   the accept state is pushed to `dest` once so `prune_after_accept` can use its
///   position for the leftmost-longest `truncate`.
///
/// Each arrival increments `collector.seq`, which the priority order encodes
/// (lower = higher priority, matching PikeVM leftmost-greedy registration order).
#[allow(clippy::too_many_arguments)]
fn record_accept(
    cache: &mut Cache,
    target: SlotTarget,
    state_idx: usize,
    num_cap: usize,
    pos: usize,
    accept_state: StateId,
    dest: &mut Vec<StateId>,
    collector: &mut AcceptCollector,
) {
    let pim = match target {
        SlotTarget::Curr => cache.nfa.curr_lookbehinds.get(state_idx).copied().flatten(),
        SlotTarget::Next => cache.nfa.next_lookbehinds.get(state_idx).copied().flatten(),
    };
    let priority = collector.seq;
    collector.seq += 1;
    // Priority rank = number of (strictly higher-priority) threads already pushed
    // to `dest` when this accept arrived. `dest` is filled in priority order, so
    // truncating to this length on a win keeps exactly the higher-priority threads
    // (incl. the winning construct's greedy loop-back) and kills lower-priority
    // alternatives. For a direct accept this is the `dest.len()` BEFORE its own
    // push, i.e. the accept state's index in `dest`.
    let thread_rank = dest.len();

    let (match_start, raw_match_end) = {
        let slots = match target {
            SlotTarget::Curr => cache.nfa.curr_slots.get(state_idx),
            SlotTarget::Next => cache.nfa.next_slots.get(state_idx),
        };
        (
            slots.get(ms_slot(num_cap)).copied().flatten(),
            slots.get(me_slot(num_cap)).copied().flatten(),
        )
    };
    let match_end = if raw_match_end.is_some() {
        raw_match_end
    } else {
        Some(pos)
    };

    let mut captures = cache.take_lb_captures(num_cap);
    {
        let slots = match target {
            SlotTarget::Curr => cache.nfa.curr_slots.get(state_idx),
            SlotTarget::Next => cache.nfa.next_slots.get(state_idx),
        };
        if let Some(src) = slots.get(..num_cap) {
            captures[..src.len()].copy_from_slice(src);
        }
    }

    if let Some(pending) = pim {
        collector.candidates.push(LookbehindCandidate {
            pending,
            captures,
            match_start,
            match_end,
            accepted_at: pos,
            priority,
            thread_rank,
        });
    } else if collector.direct.is_none() {
        // First (highest-priority) direct accept: snapshot its slots and push the
        // accept state to `dest` so the greedy `truncate` in `prune_after_accept`
        // can prune lower-priority alternatives at the accept's position.
        collector.direct = Some(DirectAccept {
            captures,
            match_start,
            match_end,
            accepted_at: pos,
            priority,
            thread_rank,
        });
        dest.push(accept_state);
    } else {
        // A lower-priority direct accept is an equivalent duplicate of the one
        // already recorded; drop it and return its scratch buffer to the pool.
        cache.return_lb_captures(captures);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STEP
// ═══════════════════════════════════════════════════════════════════════════════

/// Advance all active threads by one character.
#[allow(clippy::too_many_arguments)]
fn step(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    current: &[StateId],
    char_pos: usize,
    next_text_pos: usize,
    next: &mut Vec<StateId>,
    num_cap: usize,
    work: &mut Vec<(StateId, usize)>,
    lookbehind_candidates: &mut AcceptCollector,
) {
    let total = total_slots(num_cap);
    for &state_id in current {
        for trans in nfa.transitions(state_id) {
            let consumed = match &trans.kind {
                TransitionKind::Literal(ch) => {
                    CharMatcher::Literal(*ch).matches(ctx.text, char_pos, ctx)
                }
                TransitionKind::AnyChar => CharMatcher::AnyChar.matches(ctx.text, char_pos, ctx),
                TransitionKind::AnyCharNl => {
                    CharMatcher::AnyCharNl.matches(ctx.text, char_pos, ctx)
                }
                TransitionKind::Matcher(id) => match nfa.matcher(*id) {
                    Matcher::Char(m) => m.matches(ctx.text, char_pos, ctx),
                    Matcher::ZeroWidth(_) | Matcher::Lookaround(_) => None,
                },
                TransitionKind::Epsilon
                | TransitionKind::Save(_)
                | TransitionKind::BackRef(_)
                | TransitionKind::LastSubstitute => None,
            };
            if consumed.is_some() {
                let parent_idx = state_id.index();
                let target_idx = trans.target.index();
                copy_curr_to_next_row(cache, parent_idx, target_idx, total);
                // Propagate pending lookbehind from curr to next
                if let Some(&pim) = cache.nfa.curr_lookbehinds.get(parent_idx) {
                    set_lookbehind(&mut cache.nfa.next_lookbehinds, target_idx, pim);
                }
                epsilon_closure(
                    nfa,
                    cache,
                    ctx,
                    trans.target,
                    next_text_pos,
                    target_idx,
                    next,
                    num_cap,
                    work,
                    SlotTarget::Next,
                    lookbehind_candidates,
                );
            }
        }
    }
}

/// Copy one state's slot row from `curr_slots` to `next_slots`.
///
/// Uses field destructuring on the NfaSimCache to borrow `curr_slots`
/// and `next_slots` simultaneously, avoiding the temporary `Vec` allocation.
#[inline]
fn copy_curr_to_next_row(cache: &mut Cache, from: usize, to: usize, total: usize) {
    if total == 0 {
        return;
    }
    let crate::cache::NfaSimCache {
        ref curr_slots,
        ref mut next_slots,
        ..
    } = cache.nfa;
    let src = curr_slots.get(from);
    next_slots.assign_from_slice(to, src);
}

// ═══════════════════════════════════════════════════════════════════════════════
// EPSILON CLOSURE
// ═══════════════════════════════════════════════════════════════════════════════

/// Expand epsilon transitions from `initial`, adding reachable states to `dest`.
///
/// Uses `work` as the DFS stack (LIFO for priority-correct expansion).
/// Deduplication uses the SparseSet indicated by `target`.
/// Slot data is stored in the SlotTable indicated by `target`.
#[allow(clippy::too_many_arguments)]
fn epsilon_closure(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    initial: StateId,
    pos: usize,
    parent_idx: usize,
    dest: &mut Vec<StateId>,
    num_cap: usize,
    work: &mut Vec<(StateId, usize)>,
    target: SlotTarget,
    lookbehind_candidates: &mut AcceptCollector,
) {
    work.clear();
    work.push((initial, parent_idx));

    while let Some((sid, pidx)) = work.pop() {
        let sidx = sid.index();

        // Dedup: insert into the appropriate SparseSet.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "NFA state count limited to u32::MAX"
        )]
        let sid_u32 = sidx as u32;
        let inserted = match target {
            SlotTarget::Curr => cache.nfa.curr.insert(sid_u32),
            SlotTarget::Next => cache.nfa.next.insert(sid_u32),
        };
        if !inserted {
            // The accept state is excluded from dedup-based dropping: a
            // lower-priority accept arrival (e.g. a plain accept after a
            // higher-priority PIM already inserted `accept`) must still be
            // recorded so it can serve as the priority-ordered fallback. We read
            // this arrival's slots from its parent row `pidx` (the dedup `continue`
            // means the accept row was NOT overwritten by this thread). Accept is
            // terminal (no outgoing transitions), so there is nothing else to do.
            if sid == nfa.accept() {
                record_accept(
                    cache,
                    target,
                    pidx,
                    num_cap,
                    pos,
                    nfa.accept(),
                    dest,
                    lookbehind_candidates,
                );
            }
            continue;
        }

        // Copy parent's slots to this state's row (if different).
        if pidx != sidx {
            match target {
                SlotTarget::Curr => {
                    cache.nfa.curr_slots.copy_slots(pidx, sidx);
                    if let Some(&pim) = cache.nfa.curr_lookbehinds.get(pidx) {
                        set_lookbehind(&mut cache.nfa.curr_lookbehinds, sidx, pim);
                    }
                }
                SlotTarget::Next => {
                    cache.nfa.next_slots.copy_slots(pidx, sidx);
                    if let Some(&pim) = cache.nfa.next_lookbehinds.get(pidx) {
                        set_lookbehind(&mut cache.nfa.next_lookbehinds, sidx, pim);
                    }
                }
            }
        }

        // Fast path: follow single-epsilon-successor chains without stack overhead.
        // When a state has exactly one outgoing epsilon transition (no Save, no
        // ZeroWidth side-effects), we can walk directly to the chain end in a
        // tight loop, avoiding push/pop on `work` for every intermediate state.
        let (sid, sidx) = {
            let mut chain_sid = sid;
            let mut chain_sidx = sidx;
            loop {
                let chain_trans = nfa.transitions(chain_sid);
                if chain_trans.len() != 1 {
                    break;
                }
                let t = &chain_trans[0];
                if !matches!(t.kind, TransitionKind::Epsilon) {
                    break;
                }
                let next_sid = t.target;
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "NFA state count limited to u32::MAX"
                )]
                let next_u32 = next_sid.index() as u32;
                let inserted = match target {
                    SlotTarget::Curr => cache.nfa.curr.insert(next_u32),
                    SlotTarget::Next => cache.nfa.next.insert(next_u32),
                };
                if !inserted {
                    // Same accept exclusion as the main path: a deduped accept
                    // arrival in the fast-path chain is still recorded (its slots
                    // live in the chain parent `chain_sidx`, since the dedup `break`
                    // skips the copy below). Accept is terminal, so we stop here.
                    if next_sid == nfa.accept() {
                        record_accept(
                            cache,
                            target,
                            chain_sidx,
                            num_cap,
                            pos,
                            nfa.accept(),
                            dest,
                            lookbehind_candidates,
                        );
                    }
                    break;
                }
                let next_idx = next_sid.index();
                // Copy slots from chain parent to this next state.
                if chain_sidx != next_idx {
                    match target {
                        SlotTarget::Curr => {
                            cache.nfa.curr_slots.copy_slots(chain_sidx, next_idx);
                            if let Some(&pim) = cache.nfa.curr_lookbehinds.get(chain_sidx) {
                                set_lookbehind(&mut cache.nfa.curr_lookbehinds, next_idx, pim);
                            }
                        }
                        SlotTarget::Next => {
                            cache.nfa.next_slots.copy_slots(chain_sidx, next_idx);
                            if let Some(&pim) = cache.nfa.next_lookbehinds.get(chain_sidx) {
                                set_lookbehind(&mut cache.nfa.next_lookbehinds, next_idx, pim);
                            }
                        }
                    }
                }
                // If the chain successor can consume input, add to dest.
                // If it is accept, record it via the shared priority-aware path.
                let has_consuming_next = nfa.has_consuming(next_sid);
                if has_consuming_next {
                    dest.push(next_sid);
                } else if next_sid == nfa.accept() {
                    record_accept(
                        cache,
                        target,
                        next_idx,
                        num_cap,
                        pos,
                        nfa.accept(),
                        dest,
                        lookbehind_candidates,
                    );
                }
                chain_sid = next_sid;
                chain_sidx = next_idx;
            }
            (chain_sid, chain_sidx)
        };

        let transitions = nfa.transitions(sid);
        let has_consuming = nfa.has_consuming(sid);

        if has_consuming {
            dest.push(sid);
        } else if sid == nfa.accept() {
            record_accept(
                cache,
                target,
                sidx,
                num_cap,
                pos,
                nfa.accept(),
                dest,
                lookbehind_candidates,
            );
        }

        // Push in reverse order for LIFO priority (first transition = highest priority).
        for trans in transitions.iter().rev() {
            // PIM: handle deferred lookbehinds (via Matcher::Lookaround with defer_check)
            if let TransitionKind::Matcher(mid) = &trans.kind {
                if let Matcher::Lookaround(la) = nfa.matcher(*mid) {
                    if la.defer_check {
                        let tidx = trans.target.index();
                        // If thread already has a pending lookbehind, evaluate the old one eagerly
                        let existing = match target {
                            SlotTarget::Curr => {
                                if sidx < cache.nfa.curr_lookbehinds.len() {
                                    cache.nfa.curr_lookbehinds[sidx]
                                } else {
                                    None
                                }
                            }
                            SlotTarget::Next => {
                                if sidx < cache.nfa.next_lookbehinds.len() {
                                    cache.nfa.next_lookbehinds[sidx]
                                } else {
                                    None
                                }
                            }
                        };
                        if let Some(old) = existing {
                            let old_sub = nfa.sub_nfa(old.sub_nfa_id);
                            match check_lookaround(
                                cache, old_sub, old.kind, old.limit, ctx, old.pos,
                            ) {
                                None => continue, // Old lookbehind failed — kill thread
                                Some(sub_caps) => {
                                    // The old positive lookbehind passed: merge its
                                    // sub-captures (absolute, offset already applied) into
                                    // this thread's slot row by global index before the row
                                    // is copied to the target below. Negative lookbehinds
                                    // export an empty set, so this is a no-op for them.
                                    let slots = match target {
                                        SlotTarget::Curr => cache.nfa.curr_slots.get_mut(sidx),
                                        SlotTarget::Next => cache.nfa.next_slots.get_mut(sidx),
                                    };
                                    merge_lookaround_captures(slots, &sub_caps);
                                }
                            }
                        }
                        // Copy parent slots to target
                        let pim_val = Some(PendingLookbehind {
                            sub_nfa_id: la.sub_nfa_id,
                            kind: la.kind,
                            limit: la.limit,
                            pos,
                        });
                        match target {
                            SlotTarget::Curr => {
                                cache.nfa.curr_slots.copy_slots(sidx, tidx);
                                set_lookbehind(&mut cache.nfa.curr_lookbehinds, tidx, pim_val);
                            }
                            SlotTarget::Next => {
                                cache.nfa.next_slots.copy_slots(sidx, tidx);
                                set_lookbehind(&mut cache.nfa.next_lookbehinds, tidx, pim_val);
                            }
                        }
                        work.push((trans.target, tidx));
                        continue; // Skip the normal try_epsilon_step path
                    }

                    // Immediate (non-deferred) lookaround: check it here so a
                    // successful POSITIVE lookaround can merge its sub-captures
                    // into the target's slot row by global index. Copy the parent
                    // row to the target first, then merge, then push the target as
                    // its own parent so the merge is not overwritten by the
                    // top-of-loop `pidx==sidx` slot copy. On failure the thread dies
                    // (no push). Negative lookarounds merge an empty set (no-op).
                    let tidx = trans.target.index();
                    let sub_nfa = nfa.sub_nfa(la.sub_nfa_id);
                    match check_lookaround(cache, sub_nfa, la.kind, la.limit, ctx, pos) {
                        None => continue, // Lookaround failed — kill thread
                        Some(sub_caps) => {
                            match target {
                                SlotTarget::Curr => {
                                    cache.nfa.curr_slots.copy_slots(sidx, tidx);
                                    merge_lookaround_captures(
                                        cache.nfa.curr_slots.get_mut(tidx),
                                        &sub_caps,
                                    );
                                    if let Some(&pim) = cache.nfa.curr_lookbehinds.get(sidx) {
                                        set_lookbehind(&mut cache.nfa.curr_lookbehinds, tidx, pim);
                                    }
                                }
                                SlotTarget::Next => {
                                    cache.nfa.next_slots.copy_slots(sidx, tidx);
                                    merge_lookaround_captures(
                                        cache.nfa.next_slots.get_mut(tidx),
                                        &sub_caps,
                                    );
                                    if let Some(&pim) = cache.nfa.next_lookbehinds.get(sidx) {
                                        set_lookbehind(&mut cache.nfa.next_lookbehinds, tidx, pim);
                                    }
                                }
                            }
                            work.push((trans.target, tidx));
                            continue; // Skip the normal try_epsilon_step path
                        }
                    }
                }
            }

            if let Some((tgt, slot_update)) = try_epsilon_step(nfa, ctx, trans, pos, num_cap) {
                let tidx = tgt.index();
                if let Some((si, sv)) = slot_update {
                    // Copy parent slots to target, then apply update.
                    match target {
                        SlotTarget::Curr => {
                            cache.nfa.curr_slots.copy_slots(sidx, tidx);
                            if let Some(slot) = cache.nfa.curr_slots.get_mut(tidx).get_mut(si) {
                                *slot = sv;
                            }
                            // Propagate PIM: since tidx becomes its own parent
                            // the top-of-loop pidx==sidx check won't copy it.
                            if let Some(&pim) = cache.nfa.curr_lookbehinds.get(sidx) {
                                set_lookbehind(&mut cache.nfa.curr_lookbehinds, tidx, pim);
                            }
                        }
                        SlotTarget::Next => {
                            cache.nfa.next_slots.copy_slots(sidx, tidx);
                            if let Some(slot) = cache.nfa.next_slots.get_mut(tidx).get_mut(si) {
                                *slot = sv;
                            }
                            // Propagate PIM: since tidx becomes its own parent
                            // the top-of-loop pidx==sidx check won't copy it.
                            if let Some(&pim) = cache.nfa.next_lookbehinds.get(sidx) {
                                set_lookbehind(&mut cache.nfa.next_lookbehinds, tidx, pim);
                            }
                        }
                    }
                    // Target's slots are fully set — use itself as parent.
                    work.push((tgt, tidx));
                } else {
                    // No slot change — use current state as parent.
                    work.push((tgt, sidx));
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PRUNE AFTER ACCEPT
// ═══════════════════════════════════════════════════════════════════════════════

/// Resolve all accept-state arrivals at this position by descending thread
/// priority and update `best` with the winner.
///
/// Every accept arrival — a direct accept (no pending lookbehind) and each PIM
/// (deferred-lookbehind) candidate — was recorded by `record_accept` with a
/// priority equal to its registration order (lower = higher priority, matching
/// PikeVM leftmost-greedy order). The winner is the **highest-priority** thread
/// whose assertion passes:
/// - a PIM passes iff its deferred lookbehind verifies via `check_lookaround`;
/// - a direct accept's assertion is trivially true.
///
/// So a higher-priority *passing* PIM beats a lower-priority direct accept, while
/// a direct accept is the correct fallback when every higher-priority PIM fails.
///
/// Greedy-continuation rule (unchanged):
/// - **PIM win** → update `best` but do NOT clear `threads`: the PIM candidate
///   records the shortest greedy match (first exit from a quantifier loop);
///   higher-priority greedy loop-back threads must keep running to find the
///   longest match (e.g. `\(x\)\@<=\w\+` on "xhello" → "hello", not "h").
/// - **Direct-accept win** → update `best` and `threads.truncate` at the accept's
///   position to prune lower-priority alternatives (leftmost-longest).
fn prune_after_accept(
    nfa: &Nfa,
    cache: &mut Cache,
    threads: &mut Vec<StateId>,
    best: &mut Option<SavedAccept>,
    lookbehind_candidates: &mut AcceptCollector,
    ctx: &MatchContext<'_>,
) {
    // Take ownership of this position's accept arrivals so we can iterate them
    // while borrowing `cache` mutably for lookbehind verification. `candidates`
    // is drained in place (its allocation is restored to the collector at the
    // end to preserve pooled capacity). The collector's `seq`/`direct` are reset.
    let mut candidates = std::mem::take(&mut lookbehind_candidates.candidates);
    let mut direct = lookbehind_candidates.direct.take();
    lookbehind_candidates.reset();

    // `candidates` is already sorted ascending by priority (push order in
    // `record_accept` follows `seq`). Merge the single direct accept into that
    // order: before evaluating any PIM whose priority exceeds the direct accept's,
    // the direct accept (which always passes) wins.
    let direct_priority = direct.as_ref().map(|d| d.priority);
    let mut winner: Option<SavedAccept> = None;
    // Priority rank of the winning accept, used to truncate the active thread list
    // (the cross-position priority guard — see below).
    let mut winner_rank: Option<usize> = None;

    for candidate in candidates.drain(..) {
        // The direct accept outranks this PIM → it wins (trivially passes).
        if winner.is_none() {
            if let Some(dp) = direct_priority {
                if dp < candidate.priority {
                    if let Some(d) = direct.take() {
                        cache.best_accept_captures.clear();
                        cache.best_accept_captures.extend_from_slice(&d.captures);
                        cache.return_lb_captures(d.captures);
                        winner = Some(SavedAccept {
                            match_start: d.match_start,
                            match_end: d.match_end,
                            accepted_at: d.accepted_at,
                        });
                        winner_rank = Some(d.thread_rank);
                    }
                }
            }
        }

        if winner.is_some() {
            // Already decided; drain remaining candidate buffers to the pool.
            cache.return_lb_captures(candidate.captures);
            continue;
        }

        let sub_nfa = nfa.sub_nfa(candidate.pending.sub_nfa_id);
        let mut candidate = candidate;
        let passed = check_lookaround(
            cache,
            sub_nfa,
            candidate.pending.kind,
            candidate.pending.limit,
            ctx,
            candidate.pending.pos,
        );
        if let Some(sub_caps) = passed {
            // The deferred positive lookbehind verified: merge its sub-captures
            // (absolute, offset already applied by `check_lookaround`) into the
            // winning candidate's slot row by global index before recording it.
            // Negative lookbehinds export an empty set (no-op).
            merge_lookaround_captures(&mut candidate.captures, &sub_caps);
            cache.best_accept_captures.clear();
            cache
                .best_accept_captures
                .extend_from_slice(&candidate.captures);
            cache.return_lb_captures(candidate.captures);
            winner = Some(SavedAccept {
                match_start: candidate.match_start,
                match_end: candidate.match_end,
                accepted_at: candidate.accepted_at,
            });
            winner_rank = Some(candidate.thread_rank);
        } else {
            cache.return_lb_captures(candidate.captures);
        }
    }

    // The direct accept is the lowest-priority fallback: if no higher-priority
    // PIM won, it wins now.
    if winner.is_none() {
        if let Some(d) = direct.take() {
            cache.best_accept_captures.clear();
            cache.best_accept_captures.extend_from_slice(&d.captures);
            cache.return_lb_captures(d.captures);
            winner = Some(SavedAccept {
                match_start: d.match_start,
                match_end: d.match_end,
                accepted_at: d.accepted_at,
            });
            winner_rank = Some(d.thread_rank);
        }
    }

    // A higher-priority PIM may have won, leaving the direct accept unused; return
    // its scratch buffer to the pool to avoid leaking pooled capacity.
    if let Some(d) = direct.take() {
        cache.return_lb_captures(d.captures);
    }

    if let Some(saved) = winner {
        *best = Some(saved);
        // Cross-position priority guard (standard PikeVM "kill lower-priority
        // threads on accept"): truncate the active thread list to the winning
        // thread's priority rank. Threads with index `< winner_rank` are strictly
        // higher priority than the winner — e.g. the winning construct's greedy
        // loop-back, which was pushed to `dest` before the accept arrived — and
        // survive to extend the match at later positions. Threads at index
        // `>= winner_rank` are lower-priority alternatives; killing them prevents a
        // later, lower-priority branch from clobbering this win (Vim alternation is
        // ordered: the first matching alternative wins, not the longest). This
        // applies identically to direct and PIM wins.
        if let Some(rank) = winner_rank {
            if rank < threads.len() {
                threads.truncate(rank);
            }
        }
    }

    // Restore the (now empty) candidate buffer to preserve pooled capacity.
    lookbehind_candidates.candidates = candidates;
}

// ═══════════════════════════════════════════════════════════════════════════════
// EPSILON STEP
// ═══════════════════════════════════════════════════════════════════════════════

/// Try a non-consuming transition. Returns `(target, optional_slot_update)`.
#[inline]
fn try_epsilon_step(
    nfa: &Nfa,
    ctx: &MatchContext<'_>,
    trans: &crate::nfa::Transition,
    pos: usize,
    num_cap: usize,
) -> EpsilonResult {
    match &trans.kind {
        TransitionKind::Epsilon => Some((trans.target, None)),

        TransitionKind::Save(slot) => Some((trans.target, Some((slot.index(), Some(pos))))),

        TransitionKind::Matcher(id) => match nfa.matcher(*id) {
            Matcher::ZeroWidth(matcher) => match matcher {
                ZeroWidthMatcher::SetMatchStart => {
                    Some((trans.target, Some((ms_slot(num_cap), Some(pos)))))
                }
                ZeroWidthMatcher::SetMatchEnd => {
                    Some((trans.target, Some((me_slot(num_cap), Some(pos)))))
                }
                _ => {
                    matcher.matches(ctx.text, pos, ctx)?;
                    Some((trans.target, None))
                }
            },
            Matcher::Char(_) => None,
            // Lookarounds (deferred PIM and immediate) are handled directly in the
            // `epsilon_closure` transition loop so positive lookarounds can merge
            // their sub-captures into the slot row; they never reach here.
            Matcher::Lookaround(_) => None,
        },

        TransitionKind::Literal(_)
        | TransitionKind::AnyChar
        | TransitionKind::AnyCharNl
        | TransitionKind::BackRef(_)
        | TransitionKind::LastSubstitute => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND — uses a fresh sub-cache to avoid contention with parent match
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract a positive-lookaround sub-match's captures (by global group index)
/// as absolute haystack ranges, applying `offset` to every range. For
/// lookahead/atomic the sub-NFA runs on the full `ctx` at `pos`, so `offset = 0`
/// (already absolute); for lookbehind the sub-NFA runs on a window slice, so
/// `offset = window_start` makes the window-relative ranges absolute.
///
/// `VimMatch.captures[i]` is global group `i + 1`; the output preserves that
/// indexing so the merge into the parent slot row is a direct by-index copy.
pub(crate) fn export_sub_captures(m: &VimMatch, offset: usize) -> LookaroundCaptures {
    m.captures
        .iter()
        .map(|cap| cap.as_ref().map(|r| (r.start + offset)..(r.end + offset)))
        .collect()
}

/// Merge a positive lookaround's exported captures into a parent thread's slot
/// row by global capture index. Slot layout: group `g` (1-indexed) occupies
/// slots `(g-1)*2` (open) and `(g-1)*2+1` (close); `sub_caps[i]` is group `i+1`.
/// Only non-`None` sub-captures overwrite; `None` leaves the parent slot intact
/// (a group that did not participate inside the lookaround does not clear a
/// same-indexed parent group — though slice-#2 global numbering means they never
/// collide). Writes nothing when `sub_caps` is empty (negative lookaround).
pub(crate) fn merge_lookaround_captures(
    slots: &mut [Option<usize>],
    sub_caps: &LookaroundCaptures,
) {
    for (i, cap) in sub_caps.iter().enumerate() {
        if let Some(r) = cap {
            let open = i * 2;
            let close = i * 2 + 1;
            if let Some(s) = slots.get_mut(open) {
                *s = Some(r.start);
            }
            if let Some(s) = slots.get_mut(close) {
                *s = Some(r.end);
            }
        }
    }
}

/// Run a lookaround assertion. Returns `None` if it failed, `Some(caps)` if it
/// passed — where `caps` are the positive lookaround's sub-captures by global
/// index (absolute ranges; lookbehind offset already applied), or an empty set
/// for a negative lookaround (a passing negative assertion means the inner did
/// NOT match, so there are no submatches to export).
fn check_lookaround(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    kind: LookaroundKind,
    limit: Option<u32>,
    ctx: &MatchContext<'_>,
    pos: usize,
) -> Option<LookaroundCaptures> {
    match kind {
        LookaroundKind::PositiveAhead | LookaroundKind::Atomic => {
            let mut sub_cache = parent_cache.take_sub_cache(sub_nfa);
            let result = match_anchored(sub_nfa, &mut sub_cache, ctx, pos)
                // Lookahead/atomic run on the full ctx at `pos`: captures are
                // already absolute (offset 0).
                .map(|m| export_sub_captures(&m, 0));
            parent_cache.return_sub_cache(sub_cache);
            result
        }
        LookaroundKind::NegativeAhead => {
            let mut sub_cache = parent_cache.take_sub_cache(sub_nfa);
            let passed = match_anchored(sub_nfa, &mut sub_cache, ctx, pos).is_none();
            parent_cache.return_sub_cache(sub_cache);
            passed.then(LookaroundCaptures::new)
        }
        LookaroundKind::PositiveBehind => {
            check_lookbehind(parent_cache, sub_nfa, ctx, pos, limit, true)
        }
        LookaroundKind::NegativeBehind => {
            check_lookbehind(parent_cache, sub_nfa, ctx, pos, limit, false)
        }
    }
}

fn check_lookbehind(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    limit: Option<u32>,
    positive: bool,
) -> Option<LookaroundCaptures> {
    if let Some(l) = limit {
        return check_lookbehind_fixed(parent_cache, sub_nfa, ctx, pos, l as usize, positive);
    }
    check_lookbehind_progressive(parent_cache, sub_nfa, ctx, pos, positive)
}

fn check_lookbehind_fixed(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    max_lookback: usize,
    positive: bool,
) -> Option<LookaroundCaptures> {
    let window_start = align_to_char_boundary(ctx.text, pos.saturating_sub(max_lookback));
    let found = run_lookbehind_window(parent_cache, sub_nfa, ctx, pos, window_start);
    resolve_lookbehind(found, positive)
}

fn check_lookbehind_progressive(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    positive: bool,
) -> Option<LookaroundCaptures> {
    let mut window_size: usize = 64;
    loop {
        let window_start = align_to_char_boundary(ctx.text, pos.saturating_sub(window_size));
        let found = run_lookbehind_window(parent_cache, sub_nfa, ctx, pos, window_start);
        if found.is_some() {
            return resolve_lookbehind(found, positive);
        }
        if window_start == 0 {
            return resolve_lookbehind(None, positive);
        }
        window_size = window_size.saturating_mul(2);
    }
}

/// Convert a lookbehind window result into the `check_lookaround` contract.
/// - positive + found → `Some(caps)` (the matched sub-captures, already absolute);
/// - positive + not found → `None` (assertion failed);
/// - negative + found → `None` (the inner matched, so the negative fails);
/// - negative + not found → `Some(empty)` (passes, exports nothing).
fn resolve_lookbehind(
    found: Option<LookaroundCaptures>,
    positive: bool,
) -> Option<LookaroundCaptures> {
    match (found, positive) {
        (Some(caps), true) => Some(caps),
        (Some(_), false) => None,
        (None, true) => None,
        (None, false) => Some(LookaroundCaptures::new()),
    }
}

/// Run the lookbehind sub-NFA on the `window_start..pos` slice. Returns
/// `Some(caps)` if some sub-match ends exactly at `pos` (the assertion's anchor),
/// with captures offset by `window_start` to absolute haystack ranges; `None`
/// otherwise. The offset is the slice base (HARD POINT #2): the sub-NFA's ranges
/// are window-relative and must be shifted by `window_start` to be absolute.
fn run_lookbehind_window(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    window_start: usize,
) -> Option<LookaroundCaptures> {
    let window = ctx.text.get(window_start..pos)?;
    let sub_ctx = MatchContext {
        text: window,
        cursor: ctx.cursor.map(|c| c.saturating_sub(window_start)),
        visual_range: ctx.visual_range.map(|(s, e)| {
            (
                s.saturating_sub(window_start),
                e.saturating_sub(window_start),
            )
        }),
        case_sensitive: ctx.case_sensitive,
        ignore_composing: ctx.ignore_composing,
        line_resolver: None,
        mark_resolver: None,
        last_substitute: ctx.last_substitute,
    };
    has_match_ending_at(parent_cache, sub_nfa, &sub_ctx, window.len(), window_start)
}

/// Find a sub-match ending exactly at `target_end` within the window. On
/// success, returns its captures by global index offset by `offset` (the
/// window's absolute base) so the ranges are absolute haystack offsets.
fn has_match_ending_at(
    parent_cache: &mut Cache,
    nfa: &Nfa,
    ctx: &MatchContext<'_>,
    target_end: usize,
    offset: usize,
) -> Option<LookaroundCaptures> {
    let mut sub_cache = parent_cache.take_sub_cache(nfa);
    let mut pos = 0;
    while pos <= target_end {
        if let Some(m) = match_anchored(nfa, &mut sub_cache, ctx, pos) {
            if m.full_range.end == target_end {
                let caps = export_sub_captures(&m, offset);
                parent_cache.return_sub_cache(sub_cache);
                return Some(caps);
            }
            let next = if m.full_range.end == pos {
                advance_one_codepoint(ctx.text, pos)
            } else {
                m.full_range.end
            };
            pos = if next <= pos || next > target_end {
                advance_one_codepoint(ctx.text, pos)
            } else {
                next
            };
        } else {
            pos = advance_one_codepoint(ctx.text, pos);
        }
    }
    parent_cache.return_sub_cache(sub_cache);
    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

#[inline]
#[allow(
    dead_code,
    reason = "used by compute_has_consuming at build time; kept for debugging"
)]
fn is_consuming(kind: &TransitionKind, nfa: &Nfa) -> bool {
    match kind {
        TransitionKind::Literal(_)
        | TransitionKind::AnyChar
        | TransitionKind::AnyCharNl
        | TransitionKind::BackRef(_)
        | TransitionKind::LastSubstitute => true,
        TransitionKind::Matcher(id) => matches!(nfa.matcher(*id), Matcher::Char(_)),
        TransitionKind::Epsilon | TransitionKind::Save(_) => false,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "NFA state count limited to u32::MAX"
)]
fn ensure_cache(cache: &mut Cache, state_count: usize, slots_per_state: usize) {
    // Reallocate when EITHER the state capacity OR the per-state slot width is
    // insufficient. The slot-width check is essential for pooled sub-caches: a
    // sub-cache pooled by an earlier, narrower lookaround (e.g. group 1 only,
    // slot_count=2) must be re-sized before a later, wider lookaround (e.g. a
    // group-2 capture, slot_count=4) writes its Save slots — otherwise the wide
    // Save writes fall outside the narrow rows and are silently dropped, losing
    // the second lookaround's captures.
    if cache.nfa.curr.capacity() < state_count
        || cache.nfa.curr_slots.slots_per_state() < slots_per_state
    {
        let dfa = cache.dfa.take();
        let pike_current = std::mem::take(&mut cache.pike.current);
        let pike_next = std::mem::take(&mut cache.pike.next);
        let pike_work = std::mem::take(&mut cache.pike.work);
        let pike_lb = std::mem::take(&mut cache.pike.lb_candidates);
        let sub = cache.lookaround.sub_cache.take();
        let has_br = cache.has_backreferences;
        *cache = Cache::new(state_count as u32, slots_per_state, has_br);
        cache.dfa = dfa;
        cache.pike.current = pike_current;
        cache.pike.next = pike_next;
        cache.pike.work = pike_work;
        cache.pike.lb_candidates = pike_lb;
        cache.lookaround.sub_cache = sub;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "../tests/engines/pike_vm.rs"]
mod tests;
