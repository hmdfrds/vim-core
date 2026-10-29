# Changelog

## v0.9.1

### Bug Fixes

- Fixed `>` and `<` after `f`, `t`, `F`, `T`, a mark (`'a`, `` `a ``) or a
  sneak motion ignoring 'shiftwidth', 'tabstop' and 'expandtab'. They
  counted a Tab as 8 columns and always indented with spaces, so with
  'tabstop' and 'shiftwidth' both at 4, `>f(` wrote 12 spaces where Vim
  writes two Tabs (8 spaces under 'expandtab'). Other motions, `>>`, and
  `>` on a characterwise or linewise Visual selection were right.
- Fixed `>` and `<` on a blockwise Visual selection ignoring 'expandtab' and
  mishandling Tabs. `>` always added spaces, where Vim indents with Tabs
  under 'noexpandtab', and spaces added in front of a Tab could vanish into
  it, so a line indented with a Tab often did not move. `<` took out only
  spaces and left a Tab in. Both now follow Vim: they rebuild the
  whitespace at the block's left edge, and they also shift a line that ends
  right at that edge.
- Fixed a range whose mark is a non-ASCII character, as in `:'é`, `:1,'éd`
  or a `<Cmd>'é<CR>` mapping, crashing the engine. It now gives the mark
  error.
- Fixed a search or `:s` pattern with deeply nested groups or nested `\%[`
  crashing the engine with a stack overflow (about 120 levels of `\%(` on a
  1 MiB stack). Groups now nest at most 10 deep and a deeper pattern gives
  E339. A group or another `\%[` inside `\%[` gives E369, as Vim's
  backtracking engine does.

### Tests

- 265 more cases recorded from Vim 9.1 and Neovim now guard behaviour the
  engine already matches. Every recorded case also runs with an empty fold
  provider, as a host that supports folds has when nothing is folded.
