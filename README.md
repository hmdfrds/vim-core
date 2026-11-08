# vim-core

A pure Rust Vim engine. No threads, no I/O, no rendering. Feed it keys, get
back state changes.

This repository is the subset of vim-core that
[godot-vim](https://github.com/hmdfrds/godot-vim) is built from, published so
that anyone can compile the plugin themselves rather than trusting a binary.

## Scope

This is a **partial publication**, and it is worth being clear about that
up front. Upstream vim-core also contains a C ABI wrapper, an Extism WASM
plugin, a Neovim fidelity oracle, and example editors in four languages.
None of them are here, because godot-vim links none of them.

What is here is everything godot-vim actually compiles against:

```
vim-core/    The engine library
vim-text/    Rope/text buffer (upstream current, newer than v0.7.1)
vim-regex/   Vim-dialect regex engine (upstream current, newer than v0.7.1)
vim-test/    Test harness (dev-dependency of vim-core's suite)
```

`vim-test` is not needed to *use* the engine — only to run its test suite. It
is included so that the suite you get is the suite the engine was developed
against.

## Build

```bash
cargo build                              # the engine
cargo test --workspace                   # the full suite (see the note below)
cargo clippy --workspace                 # about 270 findings, see below
```

`vim-core/src/lib.rs` sets `#![deny(clippy::pedantic)]` and `#![deny(warnings)]`,
and the engine does not satisfy its own policy yet -- about 270 findings, nearly
all inherited from the v0.7.1 tag it started from. **Clippy is not run in CI**, because a gate that is
red on its first run and every run after teaches people to ignore CI. The debt
is real and recorded here rather than hidden behind a suppression. The library
itself compiles clean, which is what matters for building godot-vim.

To build godot-vim against a local checkout of this repo instead of the
pinned git tag, add to godot-vim's `Cargo.toml`:

```toml
[patch."https://github.com/hmdfrds/vim-core.git"]
vim-core = { path = "../vim-core/vim-core" }
```

## Strings

Owned text in vim-core's API is `VimString`, vim-core's own string type. It
derefs to `str`, converts from and into the std string types with `.into()`,
and `format_vim!` builds one as `format!` builds a `String`. It keeps short
text inline using [compact_str](https://crates.io/crates/compact_str), which
is a private dependency: no public item names it, so its releases are not
part of vim-core's API. CI keeps it that way: the build fails when any file
under `vim-core/src` or `vim-core/tests` other than
`vim-core/src/primitives/vim_string.rs` names compact_str. A host that keeps
its own `CompactString` converts through `&str`:
`VimString::from(cs.as_str())`.

## Loading a config

A host hands a vimrc it has read to `VimEngine::source_script` (or
`HostSession::source_script`), with a `ConfigHost`:

```rust
use vim_core::execution::{ConfigText, TrustAll};

let sourced = engine.source_script(ConfigText::new("init.vim", &text), &mut TrustAll::new());
println!("{} errors", sourced.outcome.errors);
```

The config runs the way Vim sources a file: every command of a `|` line,
continuation lines, `:if` blocks, `:let`, nested `:source` and Vim's error
flow, with each error placed by file and line. `source_config_text` still
works but is deprecated, and goes in v0.11.0.

`TrustAll` runs every command. A host that loads configs it does not fully
trust implements `ConfigHost::judge` instead. It sees each command once,
after the engine's own checks and before anything changes, with what the
command does and what it can reach (`Reach`: options, mappings, the shell,
files, the host and more), and answers `Verdict::Allow`, `Refuse` or
`Claim`. For a mapping, `MapDefinition::reach` says what its keys can run
when they are pressed.

A config tagged with `ConfigText::new(name, &text).with_trust(TrustTag::new(n))`
passes its tag to the mappings it defines, and a `CommandLineJudge` installed with
`VimEngine::set_command_line_judge` is asked about each command line such
keys make before it runs. A mapping defined by a line of several tags keeps
the highest, so number tags from the most trusted up.
`reset_config_state` clears what a config defined before it is sourced
again. Where config loading still differs from Vim is listed in
[`CHANGELOG.md`](CHANGELOG.md) under v0.10.0, "Known Limits".

## A note on failing tests

**78 tests across 10 targets fail, and that is the expected state.** Each of
them also fails on the bare upstream `v0.7.1` tag, before any change made
here. Four more that failed there now pass.

They are red on purpose:

- **`mc_ideal_*` (50 failures)**: written-first specifications for
  multi-cursor behaviour that is not implemented yet. The tests describe the
  intended semantics; the engine has not caught up.
- **`fidelity_tests` (21)**: 6,292 pass. The 21 are known points where the
  engine diverges from real Vim.
- **`auto_pairs_tests` (3)** — dot-repeat drops the auto-paired closing
  delimiter. The test source carries an inline `// BUG:` comment naming it.
- **`block_visual_undo_bug` (1)** — says so in the target name.
- **Three singles** in `edge_case_tests`, `engine_property_tests`, and
  `multi_cursor_reexec_tests`.

They have deliberately not been silenced with `#[ignore]` or had their
expectations relaxed. A failing test that documents a real defect is more
useful than a green one that hides it. The library itself builds clean, which
is why godot-vim works despite them.

So that this still catches regressions, the exact set is pinned in
[`tests-known-failing.txt`](tests-known-failing.txt) and asserted by:

```bash
./scripts/known-failures.sh          # CI mode: the set must match exactly
./scripts/known-failures.sh --bless  # rewrite the baseline from reality
```

A **new** failure fails the check — that is a regression. A test that starts
**passing** also fails the check, deliberately: somebody fixed a defect and it
should be crossed off the list rather than quietly absorbed.

Note that `cargo test` alone stops at the first failing target and will show
you only a fraction of them; the script passes `--no-fail-fast`.

## Relationship to upstream

`vim-core` here started as the `v0.7.1` engine and has moved on since (see
`CHANGELOG.md`). `vim-text` and `vim-regex` are **not**
— they are upstream's current versions, well ahead of what v0.7.1 shipped
with (vim-regex went from 60 files to 135, vim-text from 13 to 32). Upstream's
own `vim-test` could not come with them: it imports APIs that exist only
alongside an in-progress rework of vim-core, so it stays at v0.7.1.

Moving the v0.7.1 engine onto current crates took changes at ~35 call sites,
because their APIs had moved:

- `MatchContext` became `#[non_exhaustive]` with a private `text` and
  `cursor: Option<usize>`; it is now built through `MatchContextBuilder`.
- `find_from` / `find_from_with_cache` are `find_at` / `find_at_with_cache`.
- `find_matching_lines` takes the literal and builds the bloom filter itself.
- `VimText::slice` takes a `Range`; `slice_range` keeps the old shape.
- Offsets are snapped to char boundaries before reaching the tree, which is
  what stops an edit landing mid-character from panicking.

Each of those follows how upstream's own vim-core solves the same problem,
rather than a locally invented answer.

The v0.7.1 test suite also did not compile as tagged —
`command_properties_test.rs` was missing a struct field — which had to be
fixed before any of this could be measured.

**What this meant for compiling godot-vim:** on the move, the engine logic
was v0.7.1's, and the full test suite agreed: 82 failures, the same 82 as
the bare tag, with all 6,241 passing fidelity tests still matching real Vim.
But the text and regex implementations underneath were newer than the ones
a released godot-vim binary was built against. It was verified equivalent,
not bit-identical. Four of those 82 have passed since, as the engine was
fixed; the other 78 stay pinned.

Upstream development continues privately. This repository tracks what
godot-vim is built from; it is not where new work lands.

## License

Dual-licensed under **MIT** or **Apache-2.0**, at your option — the Rust
ecosystem's convention. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).

### Third-party attribution

These pieces of this engine derive from Vim, and are therefore under the
**Vim licence** (charityware), not the dual licence above. A copy ships as
[`LICENSE-VIM`](LICENSE-VIM) and must accompany the source and any binary
built from it — keep it when trimming files. Everything else in this
repository is under the dual licence above.

Which of the Vim licence's clauses applies depends on whether a table ported
to Rust and reduced from 1,366 entries to 1,334 counts as an unmodified part
(clause I) or a modified one (clause II, which carries further conditions).
That is a question for a lawyer, not for a README. Including the licence text
satisfies either reading; anyone redistributing this commercially should get
their own advice.

- `vim-core/src/primitives/digraph.rs` — `DIGRAPH_TABLE`, 1,334 entries taken
  from Neovim's `digraphdefault[]`, which Neovim in turn inherited from Vim.
  Neovim is Apache-2.0 *except* for parts contributed under the Vim licence,
  and this table is one of them, so the Vim licence governs it rather than
  Apache-2.0. The data itself originates in RFC 1345.
  <https://github.com/neovim/neovim>

- `vim-core/src/grammar/vim_commands.rs` and
  `vim-core/src/primitives/vim_option_names.rs`: Vim's Ex command table (594
  commands with their `EX_*` flags and address types), its command
  modifiers and autocommand event names, and its option table (436 options
  with their short names, types and `P_SECURE` flag, and 89 terminal codes),
  at tag v9.1.0. `scripts/gen-vim-tables.py` generates both from Vim's
  `ex_cmds.h`, `ex_docmd.c`, `autocmd.c` and `optiondefs.h`.
  `vim-core/src/grammar/command_meta.rs` maps the command table onto the
  engine's own `CmdFlags`, whose encoding is narrower than Vim's. See
  `:help license` in Vim.
  <https://github.com/vim/vim>
