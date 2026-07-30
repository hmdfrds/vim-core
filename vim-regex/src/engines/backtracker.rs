//! Backtracker — stack-based DFS regex engine with RestoreCapture architecture.
//!
//! Uses the RestoreCapture architecture proven by regex-automata:
//!
//! 1. **24-byte Frame enum** — a discriminated enum, rather than a per-frame
//!    clone of the 212-byte capture array. One mutable capture array lives in
//!    the search function scope. Save transitions push `RestoreCapture` frames
//!    to undo mutations on backtrack.
//!
//! 2. **VisitedSet clear-once** — the `(state, pos)` key is position-absolute, so
//!    entries from earlier start positions never collide with later ones. The
//!    VisitedSet is cleared once before the position loop, not per-position.
//!
//! 3. **backref_reachable dedup bypass** — states reachable from BackRef transition
//!    targets skip VisitedSet dedup entirely (capture state affects path validity).
//!    Eliminates the probabilistic 64-bucket hashing.
//!
//! 4. **Step-loop architecture** — single `nfa.transitions(state)` call per visit.
//!    Single-transition states advance via `try_step` without stack ops.
//!    Multi-transition states push alternatives in reverse order.
//!
//! 5. **Stack byte budget** — 256KB budget (~10,922 frames at 24 bytes each)
//!    bounds DFS depth by memory rather than by a fixed frame count.
//!
//! 6. **Grow-to-fit memoization** — the VisitedSet grows to fit the haystack up
//!    to a 32 MiB cap. Only when a haystack would exceed that cap does `search`
//!    return `SearchResult::Declined`; the terminal dispatch
//!    (`run_terminal_engine`) maps that to `Err(HaystackTooLarge)`. A
//!    backtracker-only pattern NEVER falls through to the backref-incapable
//!    Pike VM.

use smallvec::SmallVec;

use super::pike_vm::LookaroundCaptures;
use crate::cache::Cache;
use crate::common::{
    advance_one_codepoint, align_to_char_boundary, build_vim_match_from_captures, SearchResult,
    BACKWARD_SCAN_WINDOW,
};
use crate::ir::LookaroundKind;
use crate::matchers::{skip_combining_marks, CharMatcher, MatchContext, Matcher, ZeroWidthMatcher};
use crate::nfa::{
    CaptureGroup, CaptureSlot, Nfa, QuantifierHint, StateId, SubNfaId, TransitionKind,
};
use crate::VimMatch;

// ═══════════════════════════════════════════════════════════════════════════════
// STACK FRAME
// ═══════════════════════════════════════════════════════════════════════════════

/// A single frame on the backtracker's DFS work stack.
///
/// Uses the RestoreCapture architecture (from regex-automata):
/// - `Step`: visit a state at a position.
/// - `RestoreCapture`/`RestoreMatchStart`/`RestoreMatchEnd`: undo mutations on backtrack.
///
/// The search function maintains one mutable capture array and one mutable
/// match_start/match_end. Transitions mutate these in-place and push restore
/// frames to undo the mutations when backtracking.
///
/// Size: 24 bytes, versus 212 bytes for a per-frame capture clone.
#[derive(Clone, Debug)]
pub(crate) enum Frame {
    /// Visit an NFA state at a text position, with accumulated fuzzy cost.
    ///
    /// `cost` is 0 for exact matching. During approximate matching,
    /// it tracks the accumulated edit distance for this search path.
    Step {
        state: StateId,
        pos: usize,
        cost: u16,
    },
    /// On backtrack, restore a capture slot to its previous value.
    RestoreCapture {
        slot: CaptureSlot,
        old_value: Option<usize>,
    },
    /// On backtrack, restore match_start (\zs) to its previous value.
    RestoreMatchStart { old: Option<usize> },
    /// On backtrack, restore match_end (\ze) to its previous value.
    RestoreMatchEnd { old: Option<usize> },
    /// RLE-compressed run of consecutive Step frames.
    ///
    /// Represents `count` Step frames for the same `state` at positions
    /// `start_pos`, `start_pos + 1`, ..., `start_pos + count - 1`.
    /// On pop, yields `(state, start_pos + count - 1)` and decrements count.
    /// When count reaches 0, the frame is fully consumed.
    RunLength {
        state: StateId,
        start_pos: usize,
        count: u16,
    },
}

#[cfg(test)]
const _: () = {
    assert!(std::mem::size_of::<Frame>() <= 24);
};

/// Maximum stack memory for the backtracker DFS.
///
/// 256 KB is generous for any reasonable pattern. With 24-byte frames,
/// this allows ~10,922 frames — more than enough for the deepest
/// practical alternation nesting.
const MAX_STACK_BYTES: usize = 256 * 1024; // 256 KB

/// Maximum stack depth derived from the byte budget.
const MAX_STACK_DEPTH: usize = MAX_STACK_BYTES / std::mem::size_of::<Frame>();

// ═══════════════════════════════════════════════════════════════════════════════
// PUBLIC API
// ═══════════════════════════════════════════════════════════════════════════════

/// Cold path for backtracker capacity decline.
#[cold]
#[inline(never)]
fn declined() -> SearchResult {
    SearchResult::Declined
}

/// Find the first match scanning forward from `start`.
///
/// If `anchored` is true, only tries at `start` (no forward scan).
/// If `inner_literal` is provided, prescreens text for the literal;
/// returns `NoMatch` immediately if not found.
///
/// Returns `Declined` if the input exceeds `VisitedSet` capacity.
pub(crate) fn search(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
    anchored: bool,
    inner_literal: Option<&str>,
) -> SearchResult {
    // Grow memoization to fit; decline only if it exceeds the memory cap.
    if !cache.visited_mut().ensure_capacity(ctx.text.len()) {
        return declined(); // mapped to HaystackTooLarge by the terminal dispatch
    }

    // Inner literal prescreen: if the literal is absent, no match is possible.
    if let Some(lit) = inner_literal {
        if memchr::memmem::find(ctx.text.as_bytes(), lit.as_bytes()).is_none() {
            return SearchResult::NoMatch;
        }
    }

    let num_slots = nfa.slot_count();

    // Clear once before the position loop — (state, pos) keys are
    // position-absolute, so entries from earlier positions never collide
    // with later ones.
    cache.visited_mut().clear();

    if anchored {
        return match try_match_at(nfa, cache, ctx, start, num_slots) {
            Some(m) => SearchResult::Match(m),
            None => SearchResult::NoMatch,
        };
    }

    let mut pos = start;
    while pos <= ctx.text.len() {
        match try_match_at(nfa, cache, ctx, pos, num_slots) {
            Some(m) => return SearchResult::Match(m),
            None => pos = advance_one_codepoint(ctx.text, pos),
        }
    }
    SearchResult::NoMatch
}

/// Find the last match whose start is before `ctx.cursor` (backward search).
///
/// If `inner_literal` is provided, prescreens text for the literal.
///
/// Returns `Declined` if the input exceeds `VisitedSet` capacity.
pub(crate) fn search_backward(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    inner_literal: Option<&str>,
) -> SearchResult {
    // Grow memoization to fit; decline only if it exceeds the memory cap.
    if !cache.visited_mut().ensure_capacity(ctx.text.len()) {
        return declined(); // mapped to HaystackTooLarge by the terminal dispatch
    }

    if let Some(lit) = inner_literal {
        if memchr::memmem::find(ctx.text.as_bytes(), lit.as_bytes()).is_none() {
            return SearchResult::NoMatch;
        }
    }

    let cursor = ctx.cursor.unwrap_or(ctx.text.len());
    let scan_start = align_to_char_boundary(ctx.text, cursor.saturating_sub(BACKWARD_SCAN_WINDOW));
    let num_slots = nfa.slot_count();
    let mut last: Option<VimMatch> = None;
    let mut pos = scan_start;

    // Clear once before the loop.
    cache.visited_mut().clear();

    while pos < cursor {
        match try_match_at(nfa, cache, ctx, pos, num_slots) {
            Some(m) if m.range.start < cursor => {
                let end = m.range.end;
                last = Some(m);
                let next = if end == pos {
                    advance_one_codepoint(ctx.text, pos)
                } else {
                    end
                };
                if next <= pos {
                    break;
                }
                pos = next;
            }
            _ => pos = advance_one_codepoint(ctx.text, pos),
        }
    }

    match last {
        Some(m) => SearchResult::Match(m),
        None => SearchResult::NoMatch,
    }
}

/// Try to match starting exactly at `pos` (anchored, no forward scan).
///
/// Returns `Declined` if the input exceeds `VisitedSet` capacity.
#[allow(
    dead_code,
    reason = "used in tests; will be used when anchored fast-path is implemented"
)]
pub(crate) fn match_anchored(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    pos: usize,
) -> SearchResult {
    if ctx.text.len() > cache.visited_mut().max_haystack_len() {
        return declined();
    }

    let num_slots = nfa.slot_count();
    cache.visited_mut().clear();
    match try_match_at(nfa, cache, ctx, pos, num_slots) {
        Some(m) => SearchResult::Match(m),
        None => SearchResult::NoMatch,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CORE DFS — RestoreCapture Step-Loop Architecture
// ═══════════════════════════════════════════════════════════════════════════════

/// Try to match starting at exactly `start_pos` using DFS with RestoreCapture.
///
/// Maintains one mutable capture array and match_start/match_end in the
/// function scope. Save/SetMatchStart/SetMatchEnd transitions mutate
/// in-place and push restore frames for backtracking.
fn try_match_at(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start_pos: usize,
    num_slots: usize,
) -> Option<VimMatch> {
    // Mutable state — one copy, mutated in place.
    let mut captures: SmallVec<[Option<usize>; 20]> = smallvec::smallvec![None; num_slots];
    let mut match_start: Option<usize> = None;
    let mut match_end: Option<usize> = None;

    // Take the reusable stack from cache (preserves capacity across calls).
    let mut stack = std::mem::take(cache.backtracker_stack_mut());
    stack.clear();
    stack.push(Frame::Step {
        state: nfa.start(),
        pos: start_pos,
        cost: 0,
    });

    let result = 'search: {
        while let Some(frame) = stack.pop() {
            match frame {
                // ── Restore frames: undo mutations on backtrack ──
                Frame::RestoreCapture { slot, old_value } => {
                    if let Some(cap) = captures.get_mut(slot.index()) {
                        *cap = old_value;
                    }
                    continue;
                }
                Frame::RestoreMatchStart { old } => {
                    match_start = old;
                    continue;
                }
                Frame::RestoreMatchEnd { old } => {
                    match_end = old;
                    continue;
                }

                // ── RunLength frame: yield one position, push remainder ──
                Frame::RunLength {
                    state,
                    start_pos: rle_start,
                    count,
                } => {
                    debug_assert!(count > 0, "RunLength with count=0 should not be on stack");
                    // Yield the highest position (greedy: try rightmost first)
                    let yield_pos = rle_start + (count as usize) - 1;
                    if count > 1 {
                        stack.push(Frame::RunLength {
                            state,
                            start_pos: rle_start,
                            count: count - 1,
                        });
                    }
                    // Push a Step frame for the yielded position and continue
                    stack.push(Frame::Step {
                        state,
                        pos: yield_pos,
                        cost: 0,
                    });
                    continue;
                }

                // ── Step frame: visit a state ──
                Frame::Step {
                    state,
                    pos,
                    cost: _,
                } => {
                    // VisitedSet dedup — backref-reachable states skip dedup
                    // because capture state affects path validity.
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "NFA state count limited to u32::MAX"
                    )]
                    if !nfa.backref_reachable(state) {
                        let inserted = cache.visited_mut().insert(state.index() as u32, pos);
                        if !inserted {
                            continue;
                        }
                    }

                    // Accept check.
                    if state == nfa.accept() {
                        let m = build_vim_match_from_captures(
                            &captures,
                            match_start,
                            match_end,
                            start_pos,
                            pos,
                        );
                        break 'search Some(m);
                    }

                    // ── SkipUntilChar fast path ──
                    // When a quantifier loop-entry state has a SkipUntilByte
                    // hint, use memchr to jump directly to candidates instead
                    // of per-character stack frame push/pop.
                    match nfa.quantifier_hint(state) {
                        QuantifierHint::SkipUntilByte(byte) => {
                            skip_until_byte_fast_path(
                                nfa,
                                cache,
                                ctx,
                                state,
                                pos,
                                byte,
                                &mut stack,
                                &mut captures,
                                &mut match_start,
                                &mut match_end,
                            );
                            continue;
                        }
                        QuantifierHint::SkipUntilNotByte(_) => {
                            // No fast path for this hint yet -- fall through.
                        }
                        QuantifierHint::None => {}
                    }

                    // Process transitions via step-loop architecture.
                    let transitions = nfa.transitions(state);
                    match transitions.len() {
                        0 => continue, // dead end

                        1 => {
                            // Linear chain fast path: single transition, no stack push
                            // for alternatives. `try_step` still pushes Restore frames
                            // for any Save/SetMatch/lookaround-capture write, so a
                            // branch-exclusive capture is undone when backtracking past
                            // this transition to a sibling branch.
                            let trans = &transitions[0];
                            if let Some((new_state, new_pos)) = try_step(
                                nfa,
                                cache,
                                ctx,
                                pos,
                                trans,
                                &mut stack,
                                &mut captures,
                                &mut match_start,
                                &mut match_end,
                            ) {
                                stack.push(Frame::Step {
                                    state: new_state,
                                    pos: new_pos,
                                    cost: 0,
                                });
                            }
                        }

                        n => {
                            // Multi-transition: push alternatives in reverse order
                            // (first alternative tried first via LIFO stack).
                            if stack.len() + n > MAX_STACK_DEPTH {
                                break 'search None;
                            }
                            for (i, trans) in transitions.iter().enumerate().rev() {
                                push_transition(
                                    nfa,
                                    cache,
                                    ctx,
                                    pos,
                                    trans,
                                    &mut stack,
                                    &mut captures,
                                    &mut match_start,
                                    &mut match_end,
                                    i == 0,
                                    0,
                                );
                            }
                            try_compress_stack_top(&mut stack);
                        }
                    }
                }
            }
        }
        None
    };

    // Return the stack to cache for reuse.
    *cache.backtracker_stack_mut() = stack;
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// TRANSITION HANDLERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Try a single transition, returning the new (state, pos) if successful.
///
/// For Save/SetMatchStart/SetMatchEnd (and lookaround-capture merges), mutates
/// captures/match_start/match_end in-place AND pushes a matching Restore frame
/// onto `stack` first, so a branch-exclusive write is undone when the search
/// backtracks past this transition to a sibling branch. (Omitting the restore
/// here leaks captures: skipping it is only sound when every sibling branch
/// overwrites the same slots, which branch-exclusive captures violate.)
#[allow(clippy::too_many_arguments)]
fn try_step(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    pos: usize,
    trans: &crate::nfa::Transition,
    stack: &mut Vec<Frame>,
    captures: &mut SmallVec<[Option<usize>; 20]>,
    match_start: &mut Option<usize>,
    match_end: &mut Option<usize>,
) -> Option<(StateId, usize)> {
    match &trans.kind {
        TransitionKind::Epsilon => Some((trans.target, pos)),
        TransitionKind::Literal(ch) => CharMatcher::Literal(*ch)
            .matches(ctx.text, pos, ctx)
            .map(|consumed| (trans.target, composing_advance(ctx, pos + consumed))),
        TransitionKind::AnyChar => CharMatcher::AnyChar
            .matches(ctx.text, pos, ctx)
            .map(|consumed| (trans.target, composing_advance(ctx, pos + consumed))),
        TransitionKind::AnyCharNl => CharMatcher::AnyCharNl
            .matches(ctx.text, pos, ctx)
            .map(|consumed| (trans.target, composing_advance(ctx, pos + consumed))),
        TransitionKind::Matcher(id) => match nfa.matcher(*id) {
            Matcher::Char(matcher) => matcher
                .matches(ctx.text, pos, ctx)
                .map(|consumed| (trans.target, composing_advance(ctx, pos + consumed))),
            Matcher::ZeroWidth(matcher) => match matcher {
                ZeroWidthMatcher::SetMatchStart => {
                    // Push a restore frame BEFORE the caller pushes the Step frame
                    // so backtracking past this single-transition write undoes it.
                    // The fast path cannot rely on an enclosing split to undo it:
                    // a branch-exclusive `\zs` would otherwise leak.
                    stack.push(Frame::RestoreMatchStart { old: *match_start });
                    *match_start = Some(pos);
                    Some((trans.target, pos))
                }
                ZeroWidthMatcher::SetMatchEnd => {
                    stack.push(Frame::RestoreMatchEnd { old: *match_end });
                    *match_end = Some(pos);
                    Some((trans.target, pos))
                }
                _ => matcher
                    .matches(ctx.text, pos, ctx)
                    .map(|_| (trans.target, pos)),
            },
            Matcher::Lookaround(la) => {
                let sub_nfa = nfa.sub_nfa(la.sub_nfa_id);
                handle_lookaround_step(
                    sub_nfa,
                    la.sub_nfa_id,
                    la.kind,
                    la.limit,
                    ctx,
                    pos,
                    cache,
                    trans.target,
                )
                .map(|(target, new_pos, caps)| {
                    // Single-transition path: merge exported lookaround captures,
                    // pushing RestoreCapture frames so backtracking past this write
                    // undoes it. A branch-exclusive lookaround capture must NOT
                    // leak into a sibling branch on backtrack.
                    if let Some(caps) = caps {
                        merge_lookaround_captures_with_restore(captures, &caps, stack);
                    }
                    (target, new_pos)
                })
            }
        },
        TransitionKind::Save(slot) => {
            let idx = slot.index();
            if idx >= captures.len() {
                captures.resize(idx + 1, None);
            }
            // Push a restore frame BEFORE the caller pushes the Step frame so
            // backtracking past this single-transition write undoes it. The fast
            // path CANNOT rely on an enclosing split to undo it — a branch-
            // exclusive capture reached here would otherwise leak.
            stack.push(Frame::RestoreCapture {
                slot: *slot,
                old_value: captures[idx],
            });
            captures[idx] = Some(pos);
            Some((trans.target, pos))
        }
        TransitionKind::BackRef(group) => {
            resolve_backref(captures, ctx, *group, pos).map(|new_pos| (trans.target, new_pos))
        }
        TransitionKind::LastSubstitute => {
            resolve_last_substitute(ctx, pos).map(|new_pos| (trans.target, new_pos))
        }
    }
}

/// Push the result of a transition onto the stack (multi-transition path).
///
/// For the FIRST alternative of a multi-transition state, mutates captures
/// in-place (no restore frame needed — on backtrack, later alternatives
/// will be tried). For subsequent alternatives, pushes RestoreCapture frames
/// BEFORE the Step frame so backtracking undoes the mutation.
#[allow(clippy::too_many_arguments)]
fn push_transition(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    pos: usize,
    trans: &crate::nfa::Transition,
    stack: &mut Vec<Frame>,
    captures: &mut SmallVec<[Option<usize>; 20]>,
    match_start: &mut Option<usize>,
    match_end: &mut Option<usize>,
    is_first: bool,
    cost: u16,
) {
    match &trans.kind {
        TransitionKind::Epsilon => {
            stack.push(Frame::Step {
                state: trans.target,
                pos,
                cost,
            });
        }

        TransitionKind::Literal(ch) => {
            if let Some(consumed) = CharMatcher::Literal(*ch).matches(ctx.text, pos, ctx) {
                let new_pos = composing_advance(ctx, pos + consumed);
                stack.push(Frame::Step {
                    state: trans.target,
                    pos: new_pos,
                    cost,
                });
            }
        }

        TransitionKind::AnyChar => {
            if let Some(consumed) = CharMatcher::AnyChar.matches(ctx.text, pos, ctx) {
                let new_pos = composing_advance(ctx, pos + consumed);
                stack.push(Frame::Step {
                    state: trans.target,
                    pos: new_pos,
                    cost,
                });
            }
        }

        TransitionKind::AnyCharNl => {
            if let Some(consumed) = CharMatcher::AnyCharNl.matches(ctx.text, pos, ctx) {
                let new_pos = composing_advance(ctx, pos + consumed);
                stack.push(Frame::Step {
                    state: trans.target,
                    pos: new_pos,
                    cost,
                });
            }
        }

        TransitionKind::Matcher(id) => match nfa.matcher(*id) {
            Matcher::Char(matcher) => {
                if let Some(consumed) = matcher.matches(ctx.text, pos, ctx) {
                    let new_pos = composing_advance(ctx, pos + consumed);
                    stack.push(Frame::Step {
                        state: trans.target,
                        pos: new_pos,
                        cost,
                    });
                }
            }
            Matcher::ZeroWidth(matcher) => {
                push_zero_width(
                    matcher,
                    pos,
                    trans,
                    stack,
                    ctx,
                    captures,
                    match_start,
                    match_end,
                    is_first,
                    cost,
                );
            }
            Matcher::Lookaround(la) => {
                let sub_nfa = nfa.sub_nfa(la.sub_nfa_id);
                if let Some((target, result_pos, caps)) = handle_lookaround_step(
                    sub_nfa,
                    la.sub_nfa_id,
                    la.kind,
                    la.limit,
                    ctx,
                    pos,
                    cache,
                    trans.target,
                ) {
                    // Merge exported lookaround captures into the frame slots,
                    // pushing RestoreCapture frames first so backtracking undoes
                    // the merge — mirrors the `Save` restore discipline below.
                    if let Some(caps) = caps {
                        merge_lookaround_captures_with_restore(captures, &caps, stack);
                    }
                    stack.push(Frame::Step {
                        state: target,
                        pos: result_pos,
                        cost,
                    });
                }
            }
        },

        TransitionKind::Save(slot) => {
            let idx = slot.index();
            if idx >= captures.len() {
                captures.resize(idx + 1, None);
            }
            let old_value = captures[idx];
            // Push the restore frame BEFORE the Step frame so on backtrack the
            // capture is restored before trying the next alternative. This is
            // pushed for EVERY alternative, including the first: skipping it on
            // the first branch is only sound when every sibling branch writes the
            // SAME slot (so a sibling overwrites it). A branch-EXCLUSIVE capture
            // (one branch's `\(...\)` that a sibling lacks) would otherwise leak
            // into the winning branch on backtrack.
            stack.push(Frame::RestoreCapture {
                slot: *slot,
                old_value,
            });
            captures[idx] = Some(pos);
            stack.push(Frame::Step {
                state: trans.target,
                pos,
                cost,
            });
        }

        TransitionKind::BackRef(group) => {
            if let Some(new_pos) = resolve_backref(captures, ctx, *group, pos) {
                stack.push(Frame::Step {
                    state: trans.target,
                    pos: new_pos,
                    cost,
                });
            }
        }

        TransitionKind::LastSubstitute => {
            if let Some(new_pos) = resolve_last_substitute(ctx, pos) {
                stack.push(Frame::Step {
                    state: trans.target,
                    pos: new_pos,
                    cost,
                });
            }
        }
    }
}

/// Push a zero-width matcher result with restore frame support.
#[allow(clippy::too_many_arguments)]
fn push_zero_width(
    matcher: &ZeroWidthMatcher,
    pos: usize,
    trans: &crate::nfa::Transition,
    stack: &mut Vec<Frame>,
    ctx: &MatchContext<'_>,
    captures: &mut SmallVec<[Option<usize>; 20]>,
    match_start: &mut Option<usize>,
    match_end: &mut Option<usize>,
    is_first: bool,
    cost: u16,
) {
    // Suppress unused-variable warnings. The captures parameter is part of
    // the uniform push_transition interface; zero-width matchers other than
    // SetMatchStart/SetMatchEnd don't touch captures.
    let _ = captures;

    match matcher {
        ZeroWidthMatcher::SetMatchStart => {
            let old = *match_start;
            if !is_first {
                stack.push(Frame::RestoreMatchStart { old });
            }
            *match_start = Some(pos);
            stack.push(Frame::Step {
                state: trans.target,
                pos,
                cost,
            });
        }
        ZeroWidthMatcher::SetMatchEnd => {
            let old = *match_end;
            if !is_first {
                stack.push(Frame::RestoreMatchEnd { old });
            }
            *match_end = Some(pos);
            stack.push(Frame::Step {
                state: trans.target,
                pos,
                cost,
            });
        }
        _ => {
            if matcher.matches(ctx.text, pos, ctx).is_some() {
                stack.push(Frame::Step {
                    state: trans.target,
                    pos,
                    cost,
                });
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SKIP-UNTIL-CHAR FAST PATH
// ═══════════════════════════════════════════════════════════════════════════════

/// SkipUntilByte fast path: instead of pushing per-character stack frames
/// for a `.*` quantifier followed by a literal byte, use `memchr` to find
/// all candidate positions and push Step frames for the successor state
/// directly at those positions.
///
/// For a greedy `.*x` quantifier at position `pos`:
/// 1. Find all occurrences of byte `x` in `text[pos..]` via memchr iterator
/// 2. Push Step frames for the exit path at each candidate (in reverse order
///    for greedy: rightmost first = tried first via LIFO stack)
/// 3. Also push a "no match at this quantifier" fallback (the exit epsilon
///    that skips the quantifier body)
///
/// This replaces O(n) individual Step frames with O(k) frames where k is the
/// number of occurrences of the target byte.
#[allow(clippy::too_many_arguments)]
fn skip_until_byte_fast_path(
    nfa: &Nfa,
    _cache: &mut Cache,
    ctx: &MatchContext<'_>,
    state: StateId,
    pos: usize,
    byte: u8,
    stack: &mut Vec<Frame>,
    captures: &mut SmallVec<[Option<usize>; 20]>,
    match_start: &mut Option<usize>,
    match_end: &mut Option<usize>,
) {
    let transitions = nfa.transitions(state);

    // The quantifier loop-entry state has 2 epsilon transitions:
    // For greedy: [body_start, loop_exit]
    // We need to identify the exit path and the body path.
    if transitions.len() != 2 {
        // Fallback: push transitions normally if structure doesn't match
        if stack.len() + transitions.len() > MAX_STACK_DEPTH {
            return;
        }
        for (i, trans) in transitions.iter().enumerate().rev() {
            push_transition(
                nfa,
                _cache,
                ctx,
                pos,
                trans,
                stack,
                captures,
                match_start,
                match_end,
                i == 0,
                0,
            );
        }
        return;
    }

    // Find the body and exit targets by checking which one leads to the
    // consuming AnyChar/AnyCharNl state that loops back.
    let t0_target = transitions[0].target;
    let t1_target = transitions[1].target;

    let (_, exit_target) = {
        // Check if t0 leads to a consuming state that loops back to `state`
        let t0_is_body = is_quantifier_body(nfa, t0_target, state);
        if t0_is_body {
            (t0_target, t1_target)
        } else {
            (t1_target, t0_target)
        }
    };

    // Use memchr to find all occurrences of the target byte in remaining text.
    let text_bytes = ctx.text.as_bytes();
    if pos > text_bytes.len() {
        // Push just the exit (no-match) path
        stack.push(Frame::Step {
            state: exit_target,
            pos,
            cost: 0,
        });
        return;
    }

    let candidates: SmallVec<[usize; 32]> = memchr::memchr_iter(byte, &text_bytes[pos..])
        .map(|offset| pos + offset)
        .collect();

    // Push exit (skip the quantifier entirely) -- tried last (pushed first in LIFO)
    stack.push(Frame::Step {
        state: exit_target,
        pos,
        cost: 0,
    });

    // Push candidate positions in forward order (leftmost pushed first,
    // rightmost on top of stack = tried first for greedy).
    // At each candidate position, we want to continue from the exit_target
    // because that's where the literal follows.
    for &cand_pos in &candidates {
        if stack.len() >= MAX_STACK_DEPTH {
            break;
        }
        stack.push(Frame::Step {
            state: exit_target,
            pos: cand_pos,
            cost: 0,
        });
    }
}

/// Check if a state is a quantifier body: it has a single consuming transition
/// (AnyChar/AnyCharNl) whose target loops back to `loop_state` (either directly
/// or via an intermediate epsilon state).
fn is_quantifier_body(nfa: &Nfa, body_start: StateId, loop_state: StateId) -> bool {
    let trans = nfa.transitions(body_start);
    if trans.len() != 1 {
        return false;
    }
    if !matches!(
        trans[0].kind,
        TransitionKind::AnyChar | TransitionKind::AnyCharNl
    ) {
        return false;
    }
    let body_target = trans[0].target;
    // Direct loop-back (common after epsilon elimination)
    if body_target == loop_state {
        return true;
    }
    // Indirect: body_target has epsilon back to loop_state
    nfa.transitions(body_target)
        .iter()
        .any(|t| matches!(t.kind, TransitionKind::Epsilon) && t.target == loop_state)
}

// ═══════════════════════════════════════════════════════════════════════════════
// RLE STACK COMPRESSION
// ═══════════════════════════════════════════════════════════════════════════════

/// Attempt to compress the top of the stack by merging consecutive Step frames
/// with the same state and sequential positions into a RunLength frame.
///
/// Called after pushing a batch of Step frames from a multi-transition state.
/// Only compresses if 3 or more consecutive frames can be merged.
fn try_compress_stack_top(stack: &mut Vec<Frame>) {
    if stack.len() < 3 {
        return;
    }

    // Check the top frames for compressibility
    let len = stack.len();
    let top = &stack[len - 1];
    let (top_state, top_pos) = match top {
        Frame::Step { state, pos, .. } => (*state, *pos),
        _ => return,
    };

    // Count how many consecutive frames below have the same state and sequential pos
    let mut run_count: u16 = 1;
    for i in (0..len - 1).rev() {
        match &stack[i] {
            Frame::Step { state, pos, .. }
                if *state == top_state && *pos + (run_count as usize) == top_pos =>
            {
                run_count += 1;
                if run_count == u16::MAX {
                    break;
                }
            }
            _ => break,
        }
    }

    // Only compress if we found a run of 3+ frames
    if run_count < 3 {
        return;
    }

    let start_idx = len - (run_count as usize);
    let start_pos = match &stack[start_idx] {
        Frame::Step { pos, .. } => *pos,
        _ => return,
    };

    // Replace the run with a single RunLength frame
    stack.truncate(start_idx);
    stack.push(Frame::RunLength {
        state: top_state,
        start_pos,
        count: run_count,
    });
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMPOSING ADVANCE HELPER
// ═══════════════════════════════════════════════════════════════════════════════

/// Advance position past combining marks when `\Z` (ignore_composing) is active.
#[inline]
fn composing_advance(ctx: &MatchContext<'_>, pos: usize) -> usize {
    if ctx.ignore_composing {
        skip_combining_marks(ctx.text, pos)
    } else {
        pos
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BACKREF / LAST SUBSTITUTE HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Resolve a backreference: check if the text at `pos` matches the captured group.
fn resolve_backref(
    captures: &[Option<usize>],
    ctx: &MatchContext<'_>,
    group: CaptureGroup,
    pos: usize,
) -> Option<usize> {
    let start = captures.get(group.open_slot().index()).copied().flatten()?;
    let end = captures
        .get(group.close_slot().index())
        .copied()
        .flatten()?;

    let captured = ctx.text.get(start..end)?;
    let remaining = ctx.text.get(pos..)?;

    let matches = if ctx.case_sensitive {
        remaining.starts_with(captured)
    } else {
        case_insensitive_prefix_match(remaining, captured)
    };

    if matches {
        Some(pos + captured.len())
    } else {
        None
    }
}

/// Resolve the `~` (last substitute) transition.
fn resolve_last_substitute(ctx: &MatchContext<'_>, pos: usize) -> Option<usize> {
    let sub_str = ctx.last_substitute?;
    let remaining = ctx.text.get(pos..)?;

    let matches = if ctx.case_sensitive {
        remaining.starts_with(sub_str)
    } else {
        case_insensitive_prefix_match(remaining, sub_str)
    };

    if matches {
        Some(pos + sub_str.len())
    } else {
        None
    }
}

/// Case-insensitive prefix comparison.
///
/// Uses ASCII-only case folding (`to_ascii_lowercase`). This is a known
/// simplification — Vim uses locale-aware folding for non-ASCII letters.
#[allow(
    clippy::manual_ignore_case_cmp,
    reason = "need char-by-char comparison for prefix matching"
)]
fn case_insensitive_prefix_match(haystack: &str, needle: &str) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }
    let mut h_iter = haystack.chars();
    for n_ch in needle.chars() {
        match h_iter.next() {
            Some(h_ch) if h_ch.to_ascii_lowercase() == n_ch.to_ascii_lowercase() => {}
            _ => return false,
        }
    }
    true
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKAROUND
// ═══════════════════════════════════════════════════════════════════════════════

/// Handle a lookaround transition, returning `(target_state, new_pos, caps)` on
/// success. `caps` is `Some(exported_captures)` for a CAPTURING positive
/// lookaround (the inner sub-NFA contains capture groups) — the caller merges
/// them into the current frame's slot row by global index — or `None` when there
/// is nothing to export (non-capturing lookaround, or any negative lookaround).
///
/// Delegates to the Pike VM for sub-NFA matching. For atomic groups, advances
/// past the sub-match. For other lookarounds, the position is zero-width.
///
/// MEMO CONTENTION: `cache.lookaround_memo` stores a bare bool keyed by
/// `(pos, sub_nfa_id)`. That bool loses captures on a memo hit, so a
/// CAPTURING positive lookaround (`sub_nfa.slot_count() > 0`) BYPASSES the bool
/// memo entirely and recomputes its captures every time. A NON-capturing
/// lookaround (the common case) keeps the fast bool-memo path unchanged. Storing
/// captures in the memo entry was rejected as more complex for no measurable win:
/// capturing lookarounds are rare, and recompute is correct and bounded.
/// Negative lookarounds never export captures regardless.
#[allow(clippy::too_many_arguments)]
fn handle_lookaround_step(
    sub_nfa: &Nfa,
    sub_nfa_id: SubNfaId,
    kind: LookaroundKind,
    limit: Option<u32>,
    ctx: &MatchContext<'_>,
    pos: usize,
    cache: &mut Cache,
    target: StateId,
) -> Option<(StateId, usize, Option<LookaroundCaptures>)> {
    // A positive lookaround whose inner sub-NFA contains capture groups must
    // export them; it bypasses the bool memo (which would drop captures).
    let capturing_positive = sub_nfa.slot_count() > 0
        && matches!(
            kind,
            LookaroundKind::Atomic | LookaroundKind::PositiveAhead | LookaroundKind::PositiveBehind
        );

    match kind {
        LookaroundKind::Atomic => {
            // Atomic groups consume text — not memoizable (end position varies).
            let mut sub_cache = cache.take_sub_cache(sub_nfa);
            let result =
                super::pike_vm::match_anchored(sub_nfa, &mut sub_cache, ctx, pos).map(|m| {
                    let caps =
                        capturing_positive.then(|| super::pike_vm::export_sub_captures(&m, 0));
                    (target, m.full_range.end, caps)
                });
            cache.return_sub_cache(sub_cache);
            result
        }
        LookaroundKind::PositiveAhead => {
            if capturing_positive {
                // Capturing: bypass the bool memo; recompute and export captures.
                let mut sub_cache = cache.take_sub_cache(sub_nfa);
                let result =
                    super::pike_vm::match_anchored(sub_nfa, &mut sub_cache, ctx, pos).map(|m| {
                        (
                            target,
                            pos,
                            Some(super::pike_vm::export_sub_captures(&m, 0)),
                        )
                    });
                cache.return_sub_cache(sub_cache);
                return result;
            }
            let la_key = sub_nfa_id.as_u16();
            if let Some(cached) = cache.lookaround_memo.get(pos, la_key) {
                return if cached {
                    Some((target, pos, None))
                } else {
                    None
                };
            }
            let mut sub_cache = cache.take_sub_cache(sub_nfa);
            let found = super::pike_vm::match_anchored(sub_nfa, &mut sub_cache, ctx, pos).is_some();
            cache.return_sub_cache(sub_cache);
            cache.lookaround_memo.insert(pos, la_key, found);
            if found {
                Some((target, pos, None))
            } else {
                None
            }
        }
        LookaroundKind::NegativeAhead => {
            let la_key = sub_nfa_id.as_u16();
            if let Some(cached) = cache.lookaround_memo.get(pos, la_key) {
                return if cached {
                    Some((target, pos, None))
                } else {
                    None
                };
            }
            let mut sub_cache = cache.take_sub_cache(sub_nfa);
            let found = super::pike_vm::match_anchored(sub_nfa, &mut sub_cache, ctx, pos).is_none();
            cache.return_sub_cache(sub_cache);
            cache.lookaround_memo.insert(pos, la_key, found);
            if found {
                Some((target, pos, None))
            } else {
                None
            }
        }
        LookaroundKind::PositiveBehind => {
            if capturing_positive {
                // Capturing: bypass the bool memo; recompute window-offset captures.
                let caps = check_lookbehind_bt_captures(cache, sub_nfa, ctx, pos, limit);
                return caps.map(|c| (target, pos, Some(c)));
            }
            let la_key = sub_nfa_id.as_u16();
            if let Some(cached) = cache.lookaround_memo.get(pos, la_key) {
                return if cached {
                    Some((target, pos, None))
                } else {
                    None
                };
            }
            let result = check_lookbehind_bt(cache, sub_nfa, ctx, pos, limit, true);
            cache.lookaround_memo.insert(pos, la_key, result);
            if result {
                Some((target, pos, None))
            } else {
                None
            }
        }
        LookaroundKind::NegativeBehind => {
            let la_key = sub_nfa_id.as_u16();
            if let Some(cached) = cache.lookaround_memo.get(pos, la_key) {
                return if cached {
                    Some((target, pos, None))
                } else {
                    None
                };
            }
            let result = check_lookbehind_bt(cache, sub_nfa, ctx, pos, limit, false);
            cache.lookaround_memo.insert(pos, la_key, result);
            if result {
                Some((target, pos, None))
            } else {
                None
            }
        }
    }
}

/// Merge a positive lookaround's exported captures into the multi-transition
/// frame slot row, pushing a `RestoreCapture` frame for EVERY slot it writes so
/// backtracking undoes the merge. Slot layout matches
/// `super::pike_vm::merge_lookaround_captures`: group `g` (1-indexed) occupies
/// slots `(g-1)*2` (open) / `(g-1)*2+1` (close), and `sub_caps[i]` is group
/// `i+1`. Restore frames are pushed BEFORE the Step frame (LIFO).
///
/// The restore frame is pushed unconditionally (no `is_first` skip): a lookaround
/// capture is branch-exclusive — it belongs only to the branch whose lookaround
/// matched — so a sibling branch will not overwrite it. Skipping the restore on
/// the first branch would leak the capture into the winning branch on backtrack.
/// This is the same restore discipline `push_transition` applies to `Save`.
fn merge_lookaround_captures_with_restore(
    captures: &mut SmallVec<[Option<usize>; 20]>,
    sub_caps: &LookaroundCaptures,
    stack: &mut Vec<Frame>,
) {
    for (i, cap) in sub_caps.iter().enumerate() {
        if let Some(r) = cap {
            for (slot_idx, value) in [(i * 2, r.start), (i * 2 + 1, r.end)] {
                if slot_idx >= captures.len() {
                    captures.resize(slot_idx + 1, None);
                }
                stack.push(Frame::RestoreCapture {
                    slot: CaptureSlot::from_raw(slot_idx as u8),
                    old_value: captures[slot_idx],
                });
                captures[slot_idx] = Some(value);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOOKBEHIND
// ═══════════════════════════════════════════════════════════════════════════════

/// Lookbehind check for the backtracker.
///
/// For bounded lookbehind (`limit` is `Some`), uses a fixed window.
/// For unbounded lookbehind, progressively expands the window starting
/// at 64 bytes, doubling until a match is found or the beginning of
/// text is reached.
fn check_lookbehind_bt(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    limit: Option<u32>,
    positive: bool,
) -> bool {
    if let Some(l) = limit {
        return check_lookbehind_bt_fixed(parent_cache, sub_nfa, ctx, pos, l as usize, positive);
    }
    check_lookbehind_bt_progressive(parent_cache, sub_nfa, ctx, pos, positive)
}

/// Lookbehind with a fixed window size.
fn check_lookbehind_bt_fixed(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    max_lookback: usize,
    positive: bool,
) -> bool {
    let window_start = align_to_char_boundary(ctx.text, pos.saturating_sub(max_lookback));
    let found = run_lookbehind_bt_window(parent_cache, sub_nfa, ctx, pos, window_start);
    if positive {
        found
    } else {
        !found
    }
}

/// Unbounded lookbehind: start small and double the window until a
/// match is found or the beginning of text is reached.
fn check_lookbehind_bt_progressive(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    positive: bool,
) -> bool {
    let mut window_size: usize = 64;
    loop {
        let window_start = align_to_char_boundary(ctx.text, pos.saturating_sub(window_size));
        if run_lookbehind_bt_window(parent_cache, sub_nfa, ctx, pos, window_start) {
            return positive;
        }
        if window_start == 0 {
            return !positive;
        }
        window_size = window_size.saturating_mul(2);
    }
}

/// Captures-aware lookbehind for a CAPTURING positive lookbehind. Returns
/// `Some(caps)` (sub-captures by global index, offset by the window start to
/// absolute haystack ranges) when some sub-match ends exactly at `pos`, or
/// `None` when the assertion fails. Always positive: a capturing
/// lookbehind is only ever asked to export on a positive assertion. Mirrors the
/// fixed/progressive window strategy of `check_lookbehind_bt`.
fn check_lookbehind_bt_captures(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    limit: Option<u32>,
) -> Option<LookaroundCaptures> {
    if let Some(l) = limit {
        let window_start = align_to_char_boundary(ctx.text, pos.saturating_sub(l as usize));
        return run_lookbehind_bt_window_captures(parent_cache, sub_nfa, ctx, pos, window_start);
    }
    let mut window_size: usize = 64;
    loop {
        let window_start = align_to_char_boundary(ctx.text, pos.saturating_sub(window_size));
        let found =
            run_lookbehind_bt_window_captures(parent_cache, sub_nfa, ctx, pos, window_start);
        if found.is_some() {
            return found;
        }
        if window_start == 0 {
            return None;
        }
        window_size = window_size.saturating_mul(2);
    }
}

/// Captures-aware twin of `run_lookbehind_bt_window`. Runs the sub-NFA against
/// `[window_start..pos]` and, if any sub-match ends exactly at `pos`, returns its
/// captures offset by `window_start` to absolute haystack ranges. Routes
/// backreference sub-NFAs through the recursive backtracker (which the Pike VM
/// cannot handle).
fn run_lookbehind_bt_window_captures(
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

    if sub_nfa.has_backreferences() {
        run_lookbehind_bt_with_backtracker_captures(sub_nfa, &sub_ctx, window.len(), window_start)
    } else {
        let mut sub_cache = parent_cache.take_sub_cache(sub_nfa);
        let result = has_match_ending_at_captures(
            sub_nfa,
            &mut sub_cache,
            &sub_ctx,
            window.len(),
            window_start,
        );
        parent_cache.return_sub_cache(sub_cache);
        result
    }
}

/// Captures-aware twin of `has_match_ending_at` (Pike VM path). On the first
/// sub-match ending exactly at `target_end`, exports its captures offset by
/// `offset` (the window's absolute base).
fn has_match_ending_at_captures(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    target_end: usize,
    offset: usize,
) -> Option<LookaroundCaptures> {
    let mut pos = 0;
    while pos <= target_end {
        if let Some(m) = super::pike_vm::match_anchored(nfa, cache, ctx, pos) {
            if m.full_range.end == target_end {
                return Some(super::pike_vm::export_sub_captures(&m, offset));
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
    None
}

/// Captures-aware twin of `run_lookbehind_bt_with_backtracker` (backreference
/// path). On the first sub-match ending exactly at `target_end`, exports its
/// captures offset by `offset` to absolute haystack ranges.
#[allow(
    clippy::cast_possible_truncation,
    reason = "NFA state count limited to u32::MAX"
)]
fn run_lookbehind_bt_with_backtracker_captures(
    nfa: &Nfa,
    ctx: &MatchContext<'_>,
    target_end: usize,
    offset: usize,
) -> Option<LookaroundCaptures> {
    let num_slots = nfa.slot_count();
    let mut sub_cache = Cache::new(
        nfa.state_count() as u32,
        num_slots,
        nfa.has_backreferences(),
    );
    let mut pos = 0;

    sub_cache.visited_mut().clear();

    while pos <= target_end {
        match try_match_at(nfa, &mut sub_cache, ctx, pos, num_slots) {
            Some(m) => {
                if m.full_range.end == target_end {
                    return Some(super::pike_vm::export_sub_captures(&m, offset));
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
            }
            None => {
                pos = advance_one_codepoint(ctx.text, pos);
            }
        }
    }
    None
}

/// Run the sub-NFA against the window `[window_start..pos]` and check
/// if any match ends exactly at `pos`.
///
/// If the sub-NFA contains backreferences, uses a recursive backtracker
/// search (via `search` with a fresh cache) instead of the Pike VM.
fn run_lookbehind_bt_window(
    parent_cache: &mut Cache,
    sub_nfa: &Nfa,
    ctx: &MatchContext<'_>,
    pos: usize,
    window_start: usize,
) -> bool {
    let window = match ctx.text.get(window_start..pos) {
        Some(w) => w,
        None => return false,
    };

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

    if sub_nfa.has_backreferences() {
        run_lookbehind_bt_with_backtracker(sub_nfa, &sub_ctx, window.len())
    } else {
        let mut sub_cache = parent_cache.take_sub_cache(sub_nfa);
        let result = has_match_ending_at(sub_nfa, &mut sub_cache, &sub_ctx, window.len());
        parent_cache.return_sub_cache(sub_cache);
        result
    }
}

/// Check if any match ends exactly at `target_end` using the Pike VM.
fn has_match_ending_at(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    target_end: usize,
) -> bool {
    let mut pos = 0;
    while pos <= target_end {
        if let Some(m) = super::pike_vm::match_anchored(nfa, cache, ctx, pos) {
            if m.full_range.end == target_end {
                return true;
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
    false
}

/// Run lookbehind matching using the backtracker engine (for backreference
/// patterns). Scans for all matches in the window and checks if any ends
/// exactly at `target_end`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "NFA state count limited to u32::MAX"
)]
fn run_lookbehind_bt_with_backtracker(
    nfa: &Nfa,
    ctx: &MatchContext<'_>,
    target_end: usize,
) -> bool {
    let num_slots = nfa.slot_count();
    let mut sub_cache = Cache::new(
        nfa.state_count() as u32,
        num_slots,
        nfa.has_backreferences(),
    );
    let mut pos = 0;

    // Clear once before the loop.
    sub_cache.visited_mut().clear();

    while pos <= target_end {
        match try_match_at(nfa, &mut sub_cache, ctx, pos, num_slots) {
            Some(m) => {
                if m.full_range.end == target_end {
                    return true;
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
            }
            None => {
                pos = advance_one_codepoint(ctx.text, pos);
            }
        }
    }
    false
}

// ═══════════════════════════════════════════════════════════════════════════════
// APPROXIMATE / FUZZY MATCHING
// ═══════════════════════════════════════════════════════════════════════════════

/// Find all approximate matches within the given cost budget.
///
/// Returns matches ranked by cost (lowest first). Each result is a
/// `(VimMatch, cost)` pair where `cost` is the accumulated edit distance.
///
/// The search explores three additional paths at each consuming transition:
/// - **Insertion**: consume an input character without advancing the NFA
/// - **Deletion**: advance the NFA without consuming input
/// - **Substitution**: advance both NFA and input on a character mismatch
///
/// Paths with `cost > config.max_cost` are pruned immediately.
/// No `VisitedSet` dedup is used because the same `(state, pos)` pair
/// may be reached via different costs.
pub(crate) fn search_approximate(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start: usize,
    config: &crate::FuzzyConfig,
) -> Vec<(VimMatch, u16)> {
    let num_slots = nfa.slot_count();
    let mut results: Vec<(VimMatch, u16)> = Vec::new();

    let mut pos = start;
    while pos <= ctx.text.len() {
        search_approximate_at(nfa, cache, ctx, pos, num_slots, config, &mut results);
        pos = advance_one_codepoint(ctx.text, pos);
    }

    results.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| a.0.range.start.cmp(&b.0.range.start))
    });
    results.dedup_by(|a, b| a.0.range == b.0.range);
    results
}

/// Try approximate matching starting at exactly `start_pos`.
#[allow(clippy::too_many_arguments)]
fn search_approximate_at(
    nfa: &Nfa,
    cache: &mut Cache,
    ctx: &MatchContext<'_>,
    start_pos: usize,
    num_slots: usize,
    config: &crate::FuzzyConfig,
    results: &mut Vec<(VimMatch, u16)>,
) {
    let mut captures: SmallVec<[Option<usize>; 20]> = smallvec::smallvec![None; num_slots];
    let mut match_start: Option<usize> = None;
    let mut match_end: Option<usize> = None;

    let mut stack = std::mem::take(cache.backtracker_stack_mut());
    stack.clear();
    stack.push(Frame::Step {
        state: nfa.start(),
        pos: start_pos,
        cost: 0,
    });

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::RestoreCapture { slot, old_value } => {
                if let Some(cap) = captures.get_mut(slot.index()) {
                    *cap = old_value;
                }
                continue;
            }
            Frame::RestoreMatchStart { old } => {
                match_start = old;
                continue;
            }
            Frame::RestoreMatchEnd { old } => {
                match_end = old;
                continue;
            }
            Frame::RunLength {
                state,
                start_pos: rle_start,
                count,
            } => {
                if count > 1 {
                    stack.push(Frame::RunLength {
                        state,
                        start_pos: rle_start,
                        count: count - 1,
                    });
                }
                let yield_pos = rle_start + (count as usize) - 1;
                stack.push(Frame::Step {
                    state,
                    pos: yield_pos,
                    cost: 0,
                });
                continue;
            }

            Frame::Step { state, pos, cost } => {
                if cost > config.max_cost {
                    continue;
                }

                if state == nfa.accept() {
                    let m = build_vim_match_from_captures(
                        &captures,
                        match_start,
                        match_end,
                        start_pos,
                        pos,
                    );
                    results.push((m, cost));
                    continue;
                }

                if stack.len() > MAX_STACK_DEPTH - 4 {
                    continue;
                }

                let transitions = nfa.transitions(state);
                if transitions.is_empty() {
                    continue;
                }

                for trans in transitions {
                    match &trans.kind {
                        TransitionKind::Epsilon => {
                            stack.push(Frame::Step {
                                state: trans.target,
                                pos,
                                cost,
                            });
                        }

                        TransitionKind::Literal(ch) => {
                            let matched = CharMatcher::Literal(*ch).matches(ctx.text, pos, ctx);
                            if let Some(consumed) = matched {
                                let new_pos = composing_advance(ctx, pos + consumed);
                                stack.push(Frame::Step {
                                    state: trans.target,
                                    pos: new_pos,
                                    cost,
                                });
                            }

                            let sub_cost = cost.saturating_add(config.cost_substitute);
                            if sub_cost <= config.max_cost
                                && pos < ctx.text.len()
                                && matched.is_none()
                                && check_error_budget(config, sub_cost)
                            {
                                let consumed =
                                    ctx.text[pos..].chars().next().map_or(1, |c| c.len_utf8());
                                let new_pos = composing_advance(ctx, pos + consumed);
                                stack.push(Frame::Step {
                                    state: trans.target,
                                    pos: new_pos,
                                    cost: sub_cost,
                                });
                            }

                            let del_cost = cost.saturating_add(config.cost_delete);
                            if del_cost <= config.max_cost && check_error_budget(config, del_cost) {
                                stack.push(Frame::Step {
                                    state: trans.target,
                                    pos,
                                    cost: del_cost,
                                });
                            }

                            let ins_cost = cost.saturating_add(config.cost_insert);
                            if ins_cost <= config.max_cost
                                && pos < ctx.text.len()
                                && check_error_budget(config, ins_cost)
                            {
                                let consumed =
                                    ctx.text[pos..].chars().next().map_or(1, |c| c.len_utf8());
                                let new_pos = composing_advance(ctx, pos + consumed);
                                stack.push(Frame::Step {
                                    state,
                                    pos: new_pos,
                                    cost: ins_cost,
                                });
                            }
                        }

                        TransitionKind::AnyChar | TransitionKind::AnyCharNl => {
                            let matcher = if matches!(trans.kind, TransitionKind::AnyChar) {
                                CharMatcher::AnyChar
                            } else {
                                CharMatcher::AnyCharNl
                            };
                            if let Some(consumed) = matcher.matches(ctx.text, pos, ctx) {
                                let new_pos = composing_advance(ctx, pos + consumed);
                                stack.push(Frame::Step {
                                    state: trans.target,
                                    pos: new_pos,
                                    cost,
                                });
                            }
                            let del_cost = cost.saturating_add(config.cost_delete);
                            if del_cost <= config.max_cost && check_error_budget(config, del_cost) {
                                stack.push(Frame::Step {
                                    state: trans.target,
                                    pos,
                                    cost: del_cost,
                                });
                            }
                        }

                        TransitionKind::Matcher(id) => match nfa.matcher(*id) {
                            Matcher::Char(matcher) => {
                                let matched = matcher.matches(ctx.text, pos, ctx);
                                if let Some(consumed) = matched {
                                    let new_pos = composing_advance(ctx, pos + consumed);
                                    stack.push(Frame::Step {
                                        state: trans.target,
                                        pos: new_pos,
                                        cost,
                                    });
                                }

                                let sub_cost = cost.saturating_add(config.cost_substitute);
                                if sub_cost <= config.max_cost
                                    && pos < ctx.text.len()
                                    && matched.is_none()
                                    && check_error_budget(config, sub_cost)
                                {
                                    let consumed =
                                        ctx.text[pos..].chars().next().map_or(1, |c| c.len_utf8());
                                    let new_pos = composing_advance(ctx, pos + consumed);
                                    stack.push(Frame::Step {
                                        state: trans.target,
                                        pos: new_pos,
                                        cost: sub_cost,
                                    });
                                }

                                let del_cost = cost.saturating_add(config.cost_delete);
                                if del_cost <= config.max_cost
                                    && check_error_budget(config, del_cost)
                                {
                                    stack.push(Frame::Step {
                                        state: trans.target,
                                        pos,
                                        cost: del_cost,
                                    });
                                }

                                let ins_cost = cost.saturating_add(config.cost_insert);
                                if ins_cost <= config.max_cost
                                    && pos < ctx.text.len()
                                    && check_error_budget(config, ins_cost)
                                {
                                    let consumed =
                                        ctx.text[pos..].chars().next().map_or(1, |c| c.len_utf8());
                                    let new_pos = composing_advance(ctx, pos + consumed);
                                    stack.push(Frame::Step {
                                        state,
                                        pos: new_pos,
                                        cost: ins_cost,
                                    });
                                }
                            }
                            Matcher::ZeroWidth(zw_matcher) => match zw_matcher {
                                ZeroWidthMatcher::SetMatchStart => {
                                    let old = match_start;
                                    stack.push(Frame::RestoreMatchStart { old });
                                    match_start = Some(pos);
                                    stack.push(Frame::Step {
                                        state: trans.target,
                                        pos,
                                        cost,
                                    });
                                }
                                ZeroWidthMatcher::SetMatchEnd => {
                                    let old = match_end;
                                    stack.push(Frame::RestoreMatchEnd { old });
                                    match_end = Some(pos);
                                    stack.push(Frame::Step {
                                        state: trans.target,
                                        pos,
                                        cost,
                                    });
                                }
                                _ => {
                                    if zw_matcher.matches(ctx.text, pos, ctx).is_some() {
                                        stack.push(Frame::Step {
                                            state: trans.target,
                                            pos,
                                            cost,
                                        });
                                    }
                                }
                            },
                            Matcher::Lookaround(la) => {
                                let sub_nfa = nfa.sub_nfa(la.sub_nfa_id);
                                if let Some((target, result_pos, caps)) = handle_lookaround_step(
                                    sub_nfa,
                                    la.sub_nfa_id,
                                    la.kind,
                                    la.limit,
                                    ctx,
                                    pos,
                                    cache,
                                    trans.target,
                                ) {
                                    // Fuzzy path always pushes restore frames
                                    // (transitions iterate forward), matching the
                                    // `Save` discipline below.
                                    if let Some(caps) = caps {
                                        merge_lookaround_captures_with_restore(
                                            &mut captures,
                                            &caps,
                                            &mut stack,
                                        );
                                    }
                                    stack.push(Frame::Step {
                                        state: target,
                                        pos: result_pos,
                                        cost,
                                    });
                                }
                            }
                        },

                        TransitionKind::Save(slot) => {
                            let idx = slot.index();
                            if idx >= captures.len() {
                                captures.resize(idx + 1, None);
                            }
                            let old_value = captures[idx];
                            stack.push(Frame::RestoreCapture {
                                slot: *slot,
                                old_value,
                            });
                            captures[idx] = Some(pos);
                            stack.push(Frame::Step {
                                state: trans.target,
                                pos,
                                cost,
                            });
                        }

                        TransitionKind::BackRef(group) => {
                            if let Some(new_pos) = resolve_backref(&captures, ctx, *group, pos) {
                                stack.push(Frame::Step {
                                    state: trans.target,
                                    pos: new_pos,
                                    cost,
                                });
                            }
                        }

                        TransitionKind::LastSubstitute => {
                            if let Some(new_pos) = resolve_last_substitute(ctx, pos) {
                                stack.push(Frame::Step {
                                    state: trans.target,
                                    pos: new_pos,
                                    cost,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    *cache.backtracker_stack_mut() = stack;
}

/// Check whether an edit operation is within the error count budget.
fn check_error_budget(config: &crate::FuzzyConfig, new_cost: u16) -> bool {
    if let Some(max_errors) = config.max_errors {
        let min_unit = config
            .cost_insert
            .min(config.cost_delete)
            .min(config.cost_substitute)
            .max(1);
        let approx_errors = new_cost / min_unit;
        if approx_errors > max_errors {
            return false;
        }
    }
    true
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "../tests/engines/backtracker.rs"]
mod tests;
