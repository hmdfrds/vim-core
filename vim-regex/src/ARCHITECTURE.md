# Regex Engine Architecture

## Overview

A stratified meta-engine for Vim-compatible regex. Selects the fastest applicable strategy at pattern compilation time. Every search path falls through to the Pike VM as the universal correct fallback.


## Layout

The crate is organised as: `parser/` (recursive-descent, magic-mode aware) →
`hir/` (lowering and pattern properties) → `nfa/` (Thompson construction with
side-table transitions) → `engine/` (strategy selection) over `engines/` (the
matching implementations: Pike VM, bounded backtracker, lazy DFA, one-pass
DFA). `accel/` holds prefilters and range narrowing, `cache/` the
pre-allocated scratch space, `matchers/` the character and zero-width
predicates.

A deliberately un-detailed description: the module tree moves, and a file-by-
file map in prose goes stale silently. `cargo doc --open` and the directory
itself are authoritative.

## Compilation pipeline

Pattern string → parser → HIR lowering (normalises escapes and classes, fuses
adjacent literals, unwraps trivial quantifiers, collapses small alternations,
and computes the acceleration hints) → Thompson NFA → prefilter construction →
a compiled `VimRegex` holding the NFA, properties, prefilter and chosen
strategy.

## Strategy selection

At compile time the meta-engine picks the narrowest strategy the pattern
allows -- range narrowing for buffer-position atoms, literal bypass when the
pattern is pure literal, anchored paths, reverse scans driven by a suffix or
inner literal, one-pass and lazy DFAs where the pattern is eligible. Each can
decline. See `engine/strategies/` for the current set; it changes as new ones
are added.

## Key Design Decisions

**Correctness-first, performance-layered.** The Pike VM never fails, never hangs, always produces a correct answer in O(n*m) time. Fast paths are opportunistic — they can decline and the Pike VM takes over.

**Flat-text model.** The engine operates on `&str`, not line-by-line. This eliminates Vim's entire class of multi-line matching bugs (go_to_nextline, reg_nextline, cross-line position tracking). Newlines are just bytes in the string.

**Trait-abstracted buffer interface.** `LineResolver` and `MarkResolver` traits decouple the engine from any specific editor's buffer representation. The engine is embeddable in Godot or any Rust application.

**Buffer-position atoms narrow before matching.** A pattern containing `\%23l`
has the search restricted to line 23 before the engine starts, rather than the
constraint being re-tested at every candidate position across the buffer. The
same applies to `\%V`.

**Side-table NFA transitions.** Heavy matchers (collections, zero-width assertions) live in a side table indexed by `MatcherId`. Inline transitions (`Literal(char)`, `AnyChar`, `Epsilon`, `Save`) are small and fixed-size. The hot path (epsilon closure) processes inline transitions without touching the side table.

## Invariants

- State IDs are dense in `[0, state_count)` — enables O(1) SparseSet allocation
- SparseSet clear is O(1) — stale entries harmless due to double-check
- VisitedSet provides provable polynomial backtracking — no step limit, no PatternTooComplex errors
- Every search path reaches the Pike VM if all faster engines decline
- Every public entry point (`new`, `find`, `find_at`, `find_backward`,
  `find_all`, `is_match`) goes through the same strategy selection
