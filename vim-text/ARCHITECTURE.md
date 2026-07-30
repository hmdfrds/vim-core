# vim-text Architecture

## Overview

A persistent B+ tree text buffer with O(log n) edits, O(1) snapshots via Arc COW, and rich cached metadata enabling tree-pruned queries (bracket matching, blank-line detection, indent analysis) plus bloom-filtered substring search.

## Tree Layer (`tree/`)

The generic tree is fully decoupled from text concerns. It can store any `Item` type with any `Summary` monoid.

### Key Traits

Defined in `tree/traits.rs`:

- **`Summary`** — Monoidal aggregate with `compose` and `base_len`. `Default` is the identity element (there is no `identity` method). `base_len` reports the span's length in the tree's base unit — bytes for text, item count for sequences — and is the dimension insert/delete offsets are measured in.
- **`InvertibleSummary`** — Opt-in extension adding `subtract`, for summaries where `subtract(compose(a, b), b) == a` exactly.
- **`Item`** — Leaf content with `summary`, `len`, `split_at`, `try_merge`, plus the associated constants `MIN_LEN` (below which a leaf is undersized and merges) and `MAX_LEN` (above which it splits).
- **`Dimension<S>`** — Monotonic projection enabling O(log n) seeking by any metric (bytes, lines, chars, UTF-16 offsets).

### Node Layout

```
Node<T: Item, const B: usize = 8>
├── Leaf { item, summary }        — content + its cached summary
└── Internal(InternalNode<T, B>)  — children + summaries + aggregate summary + height
```

`InternalNode` holds `children: ArrayVec<Arc<Node<T, B>>, INTERNAL_CAP>` and a parallel
`summaries: ArrayVec<T::Summary, INTERNAL_CAP>`, plus a cached `summary` aggregate that makes
`Node::summary()` O(1) instead of O(B). The `ArrayVec` backing means an internal node needs no
separate heap allocation for its child list. `INTERNAL_CAP` is `DEFAULT_B + 2 = 10`; the two spare
slots absorb the temporary overflow during a split before it propagates, and after any public
operation `children.len() <= B`. `MAX_HEIGHT` is 16, which at `B = 8` covers 8^16 leaves.

The `summaries` array enables O(1) per-child length extraction via `Summary::base_len`, without
recursing into subtrees.

### Edit Operations

All edits (`SumTree::insert`, `delete`, `replace`) are O(log n). The key invariant:
`find_child_for_offset` (`tree/sum_tree.rs`) walks `internal.summaries` and reads each child's length
via `Summary::base_len` — a linear scan over at most `B` cached summaries, never a recursion into
children.

After a mutation, the affected internal node calls `update_summary(index)` to refresh the one stale
per-child summary, then `recompute_aggregate()` to re-fold all summaries into the node aggregate.

### Subtraction (`InvertibleSummary`)

`InvertibleSummary` is implemented only by `MetricsSummary`, whose four fields (bytes, chars,
newlines, utf16_len) are strictly additive and subtract exactly via `wrapping_sub`. `TextSummary`
does **not** implement it: `LineSummary` (max/seam line lengths), `IndentSummary` (min + flags) and
`BracketSummary` (delta/min/max) are all non-invertible.

The tree itself never calls `subtract` — parent summaries are always refreshed by the O(B)
`recompute_aggregate()` fold. The trait is carried as `#[allow(dead_code)]` infrastructure for
future summary types that are fully invertible.

## Text layer (`chunk.rs`, `summary.rs`)

- **TextChunk** — `ArrayString<CHUNK_MAX_BYTES>` leaf, implementing `Item` with
  `MAX_LEN = CHUNK_MAX_BYTES = 1024` and `MIN_LEN = CHUNK_MIN_BYTES = 256`
- **TextSummary** — 44-byte monoid built from four sub-summaries: `MetricsSummary`
  (bytes/chars/newlines/utf16_len), `LineSummary` (first/last/max line length), `IndentSummary`
  (min_indent + `IndentFlags`), `BracketSummary` (delta/min/max per bracket type)
- **Dimensions** — `ByteOffset`, `CharOffset`, `LineOffset`, `Utf16Offset`, each implementing
  `Dimension<TextSummary>`. `Position { line, col }` lives in `vim_text.rs`.
- **`SummaryFlags`** — legacy alias for `IndentFlags`, kept for older vim-core call sites
- **`chunk_text`** — splits a string into chunk-sized pieces, backing off to a char boundary, never
  splitting a CRLF pair, and preferring a newline in the last 25% of the chunk when one is there

Not covered here: `changeset.rs` and `edit_batch.rs` (batched edits and their
inverses), `diff.rs` (structural diffing), `iter/` (byte, char, line and chunk
iterators), `rope_slice.rs` and `regex_adapter.rs`. See `cargo doc`.

## Queries (`queries/`)

`VimQueries` (`queries/blank_lines.rs`) is an extension trait on `VimText`; `VimText` also re-exposes
each method inherently so call sites need no import.

- **Blank lines** — `next_blank_line_from`, `prev_blank_line_from`, `next_nonblank_line_from`,
  `prev_nonblank_line_from`, `is_line_blank`, `has_blank_line_in_range`. Tree-pruned: a subtree whose
  summary lacks `HAS_BLANK_LINE` is skipped whole, giving O(log n) when blank lines are sparse.
- **Brackets** — `matching_bracket`, `bracket_depth_at` (`queries/brackets.rs`), covering `()`, `[]`
  and `{}`. Forward search prunes on `BracketSummary.min` (descend only when `depth + min <= 0`);
  backward search prunes on the `min..max` range.
- **Indent** — `min_indent_in_range` (`queries/indent.rs`). A cursor walk over the chunks covering the
  range: an interior chunk that starts on a line boundary contributes its cached `min_indent` in
  O(1), and only boundary chunks are scanned byte-by-byte — O(log n + 2*CHUNK_MAX_BYTES) plus one
  O(1) step per interior chunk.

## Bloom Filter

`BloomFilter` (`bloom.rs`) is 1024 bits (`[u64; 16]`) hashed from byte bigrams: one multiplicative
hash per bigram, split into two 10-bit indices. No false negatives, false positives possible.
Patterns shorter than two bytes cannot be filtered and always pass.

Bloom filters are **not** stored in the tree and are **not** propagated to internal nodes — doing so
would cost `VimText` its O(1) Arc-COW clone. Instead `queries/search.rs` builds
`BloomFilter::from_text(chunk)` on the fly, per chunk, per search call. That is affordable because
search runs once per command (`:g`, `:s`, `/`), not per keystroke.

`bloom_search_lines` therefore visits every chunk, but only byte-scans the chunks the filter fails to
reject. A carry buffer stitches lines that straddle a chunk boundary, so a bloom-rejected chunk still
contributes its trailing partial line to the next chunk's first line.
`might_contain_literal` is the whole-document early-out.

## vim-core Integration

### Optional Tree

`Document::vim_text_tree()` returns `Option<&VimText>` and defaults to `None`. Only
`VimTextDocument` (`execution/engine/vim_text_document.rs`) overrides it, returning
`Some(&self.tree)`; String-backed documents keep the `None` default.

Command contexts (`MotionContext`, `ActionContext`, `OperatorContext`, `TextObjectContext`,
`ExContext`) hold `tree: Option<&'text VimText>`, initialised to `None`. The executor fills it in via
`.with_tree(tree)` only when `ctx.doc().vim_text_tree()` is `Some`, so command code must handle the
`None` case.

### Where the tree is actually used

- **Document-level metrics** — `VimTextDocument` overrides `Document::len`, `line_count`,
  `offset_to_pos`, `pos_to_offset`, `slice` and `line_of_offset` with tree-backed implementations,
  so commands reaching them through the `Document` trait get the O(log n) path without touching
  `ctx.tree`.
- **Search** — `ctx.tree` is read in `commands/motions/search.rs`, `commands/ex/substitute.rs` and
  `commands/ex/global.rs`. All three call `VimText::find_matching_lines` to bloom-narrow the
  candidate line set before running the regex.

The remaining `VimQueries` methods (`matching_bracket`, `next_blank_line_from` /
`prev_blank_line_from`, `min_indent_in_range`) have no callers in vim-core today; the bracket,
paragraph and indent text objects still go through the string helpers in `commands/helpers.rs`.

## regex-cursor: Intentionally Not Integrated

The `RopeCursor` trait and `VimTextCursor` adapter exist in `regex_adapter.rs`, matching the
`regex-cursor` crate's `Cursor` interface. regex-cursor is not a dependency of any crate in this
workspace, and wiring it into `vim-regex` is **intentionally not done** — it would provide zero
user-visible benefit given the current architecture.

**Why it's unnecessary:**

1. **Multi-line patterns** (`/foo\nbar`) need contiguous bytes. `VimTextDocument` keeps a `String`
   mirror beside the tree, so `Document::text()` is an O(1) borrow — there is no materialization
   cost to avoid.

2. **Single-line search** already works chunk by chunk. `chunk_text` prefers a newline split in the
   last 25% of each chunk, so most lines land inside a single chunk and `RopeSlice::to_cow` borrows
   instead of copying. Lines that do straddle a boundary are handled by the carry buffer in
   `bloom_search_lines` and by an owned `Cow` from `VimText::line`.

3. **Retrofitting regex-cursor into vim-regex** would mean reworking roughly 70 function signatures
   across the lazy DFA, Pike VM, backtracker, one-pass engine and prefilter system, all of which take
   a contiguous `&str` haystack. It would also lose the SIMD acceleration from `memchr` and
   `aho-corasick`, which need contiguous slices.

**When it WOULD become necessary:**

If `VimTextDocument` ever removes the `String` mirror (going tree-only), then cursor-based regex
becomes the only way to search without O(n) materialization. The `VimTextCursor` infrastructure is
ready for that day — implementing `regex_cursor::Cursor` on it is mechanical since the interface is
identical.

**Current search architecture:**
```
Search request
    |
    v
pattern_might_span_lines()?
    |             |
   NO            YES
    |             |
    v             v
Bloom candidate  doc.text() &str
lines, then      contiguous,
per-line regex   always in memory
```

`pattern_might_span_lines` lives in `vim-core/src/commands/motions/search.rs`. On the NO branch,
`find_matching_lines` bloom-filters chunks down to a candidate line list, and each candidate is then
fetched with `VimText::line` — a `Cow`, borrowed for a line inside one chunk, allocated for a line
spanning chunks — and matched against the regex.
